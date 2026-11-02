//! 录屏：Windows Graphics Capture 采集选区画面，WASAPI 采集系统声音和麦克风，
//! Media Foundation 硬件编码为 H.264 + AAC 的 MP4。
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as _};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder, VideoSettingsSubType,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};

use crate::state::AppState;
use crate::{capture, win32};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
/// 100 纳秒为单位的时钟
const TICKS: i64 = 10_000_000;

/// 录制用的时钟：扣除暂停时间
struct Clock {
    start: Instant,
    paused_total: Duration,
    paused_at: Option<Instant>,
}

impl Clock {
    fn active(&self) -> Duration {
        let now = Instant::now();
        let paused = self.paused_total + self.paused_at.map(|p| now - p).unwrap_or_default();
        (now - self.start).saturating_sub(paused)
    }
    fn ticks(&self) -> i64 {
        (self.active().as_nanos() / 100) as i64
    }
}

struct Shared {
    stop: AtomicBool,
    paused: AtomicBool,
    mute_sys: AtomicBool,
    mute_mic: AtomicBool,
    clock: Mutex<Clock>,
    encoder: Mutex<Option<VideoEncoder>>,
    /// 最近一帧（BGRA，自下而上），画面静止时重复发送
    last_frame: Mutex<Option<Vec<u8>>>,
    last_ts: Mutex<i64>,
    frame_interval: i64,
    /// 选区（相对显示器的物理像素），宽高为偶数
    region: (u32, u32, u32, u32),
    error: Mutex<Option<String>>,
}

impl Shared {
    fn send_frame(&self, buf: &[u8], ts: i64) {
        if let Some(enc) = self.encoder.lock().unwrap().as_mut() {
            if let Err(e) = enc.send_frame_buffer(buf, ts) {
                self.error.lock().unwrap().get_or_insert_with(|| format!("视频编码失败：{e}"));
            }
        }
        *self.last_ts.lock().unwrap() = ts;
    }

    fn set_paused(&self, paused: bool) {
        let mut clock = self.clock.lock().unwrap();
        match (paused, clock.paused_at) {
            (true, None) => clock.paused_at = Some(Instant::now()),
            (false, Some(p)) => {
                clock.paused_total += p.elapsed();
                clock.paused_at = None;
            }
            _ => {}
        }
        self.paused.store(paused, Ordering::Relaxed);
    }
}

struct Recorder {
    shared: Arc<Shared>,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for Recorder {
    type Flags = Arc<Shared>;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            shared: ctx.flags,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, control: InternalCaptureControl) -> Result<(), Self::Error> {
        let sh = &self.shared;
        if sh.stop.load(Ordering::Relaxed) {
            control.stop();
            return Ok(());
        }
        if sh.paused.load(Ordering::Relaxed) {
            return Ok(());
        }
        let ts = sh.clock.lock().unwrap().ticks();
        // 帧率限制
        let last = *sh.last_ts.lock().unwrap();
        if last > 0 && ts - last < sh.frame_interval * 9 / 10 {
            return Ok(());
        }
        let (x, y, w, h) = sh.region;
        if x + w > frame.width() || y + h > frame.height() {
            return Ok(());
        }
        let buffer = frame.buffer_crop(x, y, x + w, y + h)?;
        let pixels = buffer.as_nopadding_buffer(&mut self.scratch);
        // 编码器要求自下而上的行顺序
        let row = (w * 4) as usize;
        let mut flipped = vec![0u8; row * h as usize];
        for (i, line) in pixels.chunks_exact(row).enumerate() {
            let dst = (h as usize - 1 - i) * row;
            flipped[dst..dst + row].copy_from_slice(line);
        }
        sh.send_frame(&flipped, ts);
        *sh.last_frame.lock().unwrap() = Some(flipped);
        Ok(())
    }
}

/// 一路音频输入：独立线程采集，写入共享缓冲（48kHz 立体声 f32）
fn spawn_audio(shared: Arc<Shared>, loopback: bool, buf: Arc<Mutex<VecDeque<f32>>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        if let Err(e) = audio_loop(&shared, loopback, &buf) {
            eprintln!("[record] audio ({}) failed: {e:?}", if loopback { "system" } else { "mic" });
        }
    })
}

fn audio_loop(shared: &Shared, loopback: bool, buf: &Mutex<VecDeque<f32>>) -> anyhow::Result<()> {
    use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
    let _ = wasapi::initialize_mta();
    let e = |e: wasapi::WasapiError| anyhow!("{e}");
    let enumerator = DeviceEnumerator::new().map_err(e)?;
    // 系统声音 = 对默认播放设备做回环采集
    let device = enumerator
        .get_default_device(if loopback { &Direction::Render } else { &Direction::Capture })
        .map_err(e)?;
    let mut client = device.get_iaudioclient().map_err(e)?;
    let format = WaveFormat::new(32, 32, &SampleType::Float, SAMPLE_RATE as usize, CHANNELS, None);
    let mode = StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: 2_000_000,
    };
    client.initialize_client(&format, &Direction::Capture, &mode).map_err(e)?;
    let capture = client.get_audiocaptureclient().map_err(e)?;
    client.start_stream().map_err(e)?;
    let mut raw: VecDeque<u8> = VecDeque::new();
    while !shared.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
        while let Some(n) = capture.get_next_packet_size().map_err(e)? {
            if n == 0 {
                break;
            }
            capture.read_from_device_to_deque(&mut raw).map_err(e)?;
        }
        let mut out = buf.lock().unwrap();
        while raw.len() >= 4 {
            let b = [raw[0], raw[1], raw[2], raw[3]];
            raw.drain(..4);
            out.push_back(f32::from_le_bytes(b));
        }
        // 最多积压 1 秒
        let max = SAMPLE_RATE as usize * CHANNELS;
        if out.len() > max {
            let extra = out.len() - max;
            out.drain(..extra);
        }
    }
    let _ = client.stop_stream();
    Ok(())
}

type AudioBuf = Arc<Mutex<VecDeque<f32>>>;

/// 按录制时钟持续输出混音后的 16 位 PCM；没有声音时补静音，保证音画同步
fn spawn_mixer(shared: Arc<Shared>, sys: Option<AudioBuf>, mic: Option<AudioBuf>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let take = |src: &Option<AudioBuf>, n: usize, mute: bool| -> Vec<f32> {
            let mut v = vec![0f32; n];
            if let Some(b) = src {
                let mut b = b.lock().unwrap();
                let k = n.min(b.len());
                for (i, s) in b.drain(..k).enumerate() {
                    if !mute {
                        v[i] = s;
                    }
                }
            }
            v
        };
        let mut sent: u64 = 0;
        while !shared.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(10));
            if shared.paused.load(Ordering::Relaxed) {
                // 暂停期间的声音丢弃
                for b in [&sys, &mic].into_iter().flatten() {
                    b.lock().unwrap().clear();
                }
                continue;
            }
            let target = (shared.clock.lock().unwrap().active().as_secs_f64() * SAMPLE_RATE as f64) as u64;
            if target <= sent {
                continue;
            }
            let n = (target - sent) as usize * CHANNELS;
            let a = take(&sys, n, shared.mute_sys.load(Ordering::Relaxed));
            let b = take(&mic, n, shared.mute_mic.load(Ordering::Relaxed));
            let mut pcm = Vec::with_capacity(n * 2);
            for i in 0..n {
                let s = ((a[i] + b[i]).clamp(-1.0, 1.0) * 32767.0) as i16;
                pcm.extend_from_slice(&s.to_le_bytes());
            }
            if let Some(enc) = shared.encoder.lock().unwrap().as_mut() {
                let _ = enc.send_audio_buffer(&pcm, 0);
            }
            sent = target;
        }
    })
}

fn hmonitor_at(x: i32, y: i32) -> *mut std::ffi::c_void {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
    unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST).0 }
}

/// 录制参数
struct PipelineConfig {
    /// 选区中心（屏幕物理坐标），用来确定显示器
    center: (i32, i32),
    /// 选区（相对显示器），宽高为偶数
    region: (u32, u32, u32, u32),
    fps: u32,
    bitrate: u32,
    cursor: bool,
    has_sys: bool,
    has_mic: bool,
    path: PathBuf,
}

/// 采集 + 编码 + 音频线程，不涉及界面窗口
struct Pipeline {
    shared: Arc<Shared>,
    control: Option<CaptureControl<Recorder, Box<dyn std::error::Error + Send + Sync>>>,
    threads: Vec<JoinHandle<()>>,
}

impl Pipeline {
    fn start(cfg: &PipelineConfig) -> anyhow::Result<Self> {
        let (_, _, rw, rh) = cfg.region;
        let encoder = VideoEncoder::new(
            VideoSettingsBuilder::new(rw, rh)
                .sub_type(VideoSettingsSubType::H264)
                .frame_rate(cfg.fps)
                .bitrate(cfg.bitrate),
            AudioSettingsBuilder::default()
                .sample_rate(SAMPLE_RATE)
                .channel_count(CHANNELS as u32)
                .bit_per_sample(16)
                .disabled(!(cfg.has_sys || cfg.has_mic)),
            ContainerSettingsBuilder::default(),
            &cfg.path,
        )
        .map_err(|e| anyhow!("无法创建视频文件：{e}"))?;

        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            mute_sys: AtomicBool::new(false),
            mute_mic: AtomicBool::new(false),
            clock: Mutex::new(Clock {
                start: Instant::now(),
                paused_total: Duration::ZERO,
                paused_at: None,
            }),
            encoder: Mutex::new(Some(encoder)),
            last_frame: Mutex::new(None),
            last_ts: Mutex::new(0),
            frame_interval: TICKS / cfg.fps as i64,
            region: cfg.region,
            error: Mutex::new(None),
        });

        let monitor = Monitor::from_raw_hmonitor(hmonitor_at(cfg.center.0, cfg.center.1));
        let cursor = if cfg.cursor {
            CursorCaptureSettings::WithCursor
        } else {
            CursorCaptureSettings::WithoutCursor
        };
        let interval = Duration::from_millis(1000 / cfg.fps as u64);
        let make = |border: DrawBorderSettings| {
            Settings::new(
                monitor,
                cursor,
                border,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Custom(interval),
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                shared.clone(),
            )
        };
        // Win10 不支持去掉系统黄框时退回默认
        let control = match Recorder::start_free_threaded(make(DrawBorderSettings::WithoutBorder)) {
            Ok(c) => c,
            Err(_) => Recorder::start_free_threaded(make(DrawBorderSettings::Default))
                .map_err(|e| anyhow!("无法开始录制：{e}"))?,
        };

        let mut threads = Vec::new();
        // 画面静止时没有新帧，定时重复上一帧
        {
            let sh = shared.clone();
            threads.push(std::thread::spawn(move || {
                while !sh.stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                    if sh.paused.load(Ordering::Relaxed) {
                        continue;
                    }
                    let ts = sh.clock.lock().unwrap().ticks();
                    if ts - *sh.last_ts.lock().unwrap() > TICKS / 2 {
                        let last = sh.last_frame.lock().unwrap().clone();
                        if let Some(f) = last {
                            sh.send_frame(&f, ts);
                        }
                    }
                }
            }));
        }
        let sys_buf = cfg.has_sys.then(|| Arc::new(Mutex::new(VecDeque::new())));
        let mic_buf = cfg.has_mic.then(|| Arc::new(Mutex::new(VecDeque::new())));
        if let Some(b) = &sys_buf {
            threads.push(spawn_audio(shared.clone(), true, b.clone()));
        }
        if let Some(b) = &mic_buf {
            threads.push(spawn_audio(shared.clone(), false, b.clone()));
        }
        if cfg.has_sys || cfg.has_mic {
            threads.push(spawn_mixer(shared.clone(), sys_buf, mic_buf));
        }
        Ok(Self {
            shared,
            control: Some(control),
            threads,
        })
    }

    /// 停止采集并写完文件
    fn finish(mut self) -> anyhow::Result<()> {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(c) = self.control.take() {
            let _ = c.stop();
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        let encoder = self.shared.encoder.lock().unwrap().take();
        if let Some(enc) = encoder {
            enc.finish().map_err(|e| anyhow!("视频保存失败：{e}"))?;
        }
        Ok(())
    }
}

pub struct RecordHandle {
    pipeline: Pipeline,
    path: PathBuf,
    has_sys: bool,
    has_mic: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordStatus {
    recording: bool,
    paused: bool,
    seconds: f64,
    has_sys: bool,
    has_mic: bool,
    mute_sys: bool,
    mute_mic: bool,
    error: Option<String>,
}

pub fn is_recording(app: &AppHandle) -> bool {
    app.state::<AppState>().record.lock().unwrap().is_some()
}

/// 开始录制。rect 为屏幕物理坐标
pub fn start(app: &AppHandle, x: i32, y: i32, w: u32, h: u32) -> anyhow::Result<()> {
    if is_recording(app) {
        return Ok(());
    }
    capture::end(app);
    let s = app.state::<AppState>().settings.lock().unwrap().clone();

    let mon = xcap::Monitor::from_point(x + w as i32 / 2, y + h as i32 / 2)?;
    let (mx, my, mw, mh) = (mon.x()?, mon.y()?, mon.width()?, mon.height()?);
    let scale = mon.scale_factor()? as f64;
    let rx = (x - mx).clamp(0, mw as i32 - 2) as u32;
    let ry = (y - my).clamp(0, mh as i32 - 2) as u32;
    // H.264 要求宽高为偶数
    let rw = (w.min(mw - rx) & !1).max(2);
    let rh = (h.min(mh - ry) & !1).max(2);

    let fps = s.record_fps.clamp(5, 60);
    let factor = if s.record_quality == "high" { 0.16 } else { 0.08 };
    let bitrate = ((rw as f64 * rh as f64 * fps as f64 * factor) as u32).clamp(1_500_000, 40_000_000);

    let dir = PathBuf::from(&s.save_dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("无法创建保存目录 {}", dir.display()))?;
    let path = dir.join(format!("Recording_{}.mp4", chrono::Local::now().format("%Y%m%d_%H%M%S")));
    let cfg = PipelineConfig {
        center: (x + w as i32 / 2, y + h as i32 / 2),
        region: (rx, ry, rw, rh),
        fps,
        bitrate,
        cursor: s.record_cursor,
        has_sys: s.record_system_audio,
        has_mic: s.record_mic,
        path: path.clone(),
    };

    // 边框和控制条先创建好并排除出录制画面，再开始采集
    let b = (3.0 * scale).round() as i32;
    let frame = WebviewWindowBuilder::new(app, "record-frame", WebviewUrl::App("index.html#record-frame".into()))
        .title("RecordFrame")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()?;
    frame.set_position(PhysicalPosition::new(mx + rx as i32 - b, my + ry as i32 - b))?;
    frame.set_size(PhysicalSize::new(rw + 2 * b as u32, rh + 2 * b as u32))?;
    frame.set_ignore_cursor_events(true)?;
    win32::exclude_from_capture(frame.hwnd()?.0 as isize);
    frame.show()?;

    let (cw, ch) = ((420.0 * scale) as i32, (48.0 * scale) as i32);
    let gap = (8.0 * scale) as i32;
    let cx = (mx + rx as i32 + rw as i32 / 2 - cw / 2).clamp(mx, mx + mw as i32 - cw);
    let below = my + (ry + rh) as i32 + gap;
    let above = my + ry as i32 - gap - ch;
    let cy = if below + ch <= my + mh as i32 {
        below
    } else if above >= my {
        above
    } else {
        my + (ry + rh) as i32 - ch - gap * 3
    };
    let ctrl = WebviewWindowBuilder::new(app, "record-ctrl", WebviewUrl::App("index.html#record-ctrl".into()))
        .title("录屏")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()?;
    ctrl.set_position(PhysicalPosition::new(cx, cy))?;
    ctrl.set_size(PhysicalSize::new(cw as u32, ch as u32))?;
    win32::exclude_from_capture(ctrl.hwnd()?.0 as isize);
    ctrl.show()?;

    let pipeline = Pipeline::start(&cfg)?;
    *app.state::<AppState>().record.lock().unwrap() = Some(RecordHandle {
        pipeline,
        path,
        has_sys: cfg.has_sys,
        has_mic: cfg.has_mic,
    });
    crate::tray::refresh(app);
    Ok(())
}

pub fn status(app: &AppHandle) -> RecordStatus {
    let st = app.state::<AppState>();
    let guard = st.record.lock().unwrap();
    match guard.as_ref() {
        Some(h) => {
            let sh = &h.pipeline.shared;
            RecordStatus {
                recording: true,
                paused: sh.paused.load(Ordering::Relaxed),
                seconds: sh.clock.lock().unwrap().active().as_secs_f64(),
                has_sys: h.has_sys,
                has_mic: h.has_mic,
                mute_sys: sh.mute_sys.load(Ordering::Relaxed),
                mute_mic: sh.mute_mic.load(Ordering::Relaxed),
                error: sh.error.lock().unwrap().clone(),
            }
        }
        None => RecordStatus {
            recording: false,
            paused: false,
            seconds: 0.0,
            has_sys: false,
            has_mic: false,
            mute_sys: false,
            mute_mic: false,
            error: None,
        },
    }
}

pub fn set_paused(app: &AppHandle, paused: bool) {
    let st = app.state::<AppState>();
    let guard = st.record.lock().unwrap();
    if let Some(h) = guard.as_ref() {
        h.pipeline.shared.set_paused(paused);
    }
}

pub fn set_mute(app: &AppHandle, source: &str, mute: bool) {
    let st = app.state::<AppState>();
    let guard = st.record.lock().unwrap();
    if let Some(h) = guard.as_ref() {
        let sh = &h.pipeline.shared;
        let flag = if source == "mic" { &sh.mute_mic } else { &sh.mute_sys };
        flag.store(mute, Ordering::Relaxed);
    }
}

/// 停止录制。cancel 为 true 时丢弃文件。返回保存的文件路径
pub fn stop(app: &AppHandle, cancel: bool) -> anyhow::Result<Option<PathBuf>> {
    let Some(h) = app.state::<AppState>().record.lock().unwrap().take() else {
        return Ok(None);
    };
    for label in ["record-frame", "record-ctrl"] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.destroy();
        }
    }
    crate::tray::refresh(app);
    h.pipeline.finish()?;
    if cancel {
        let _ = std::fs::remove_file(&h.path);
        return Ok(None);
    }
    Ok(Some(h.path))
}

/// 用于托盘和快捷键：正在录制就停止并打开结果，否则开始选区录制
pub fn toggle(app: &AppHandle, delay_ms: u64) {
    if is_recording(app) {
        let app = app.clone();
        std::thread::spawn(move || finish_and_show(&app));
    } else {
        capture::start_with_mode(app, delay_ms, "record");
    }
}

pub fn finish_and_show(app: &AppHandle) {
    match stop(app, false) {
        Ok(Some(path)) => {
            if let Err(e) = crate::windows_mgr::open_video_window(app, &path) {
                eprintln!("[record] open result failed: {e:?}");
            }
        }
        Ok(None) => {}
        Err(e) => {
            use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
            app.dialog()
                .message(format!("{e:#}"))
                .title("录屏")
                .kind(MessageDialogKind::Error)
                .show(|_| {});
        }
    }
    let _ = app.emit("record:stopped", ());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实录制主显示器左上角约 3 秒（含系统声音），中间暂停 1 秒：
    /// cargo test --lib record -- --ignored --nocapture
    #[test]
    #[ignore]
    fn records_mp4_with_audio() {
        let path = std::env::temp_dir().join("screenshot_record_test.mp4");
        let _ = std::fs::remove_file(&path);
        let cfg = PipelineConfig {
            center: (100, 100),
            region: (0, 0, 640, 360),
            fps: 30,
            bitrate: 2_000_000,
            cursor: true,
            has_sys: true,
            has_mic: false,
            path: path.clone(),
        };
        let p = Pipeline::start(&cfg).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        p.shared.set_paused(true);
        std::thread::sleep(Duration::from_millis(1000));
        p.shared.set_paused(false);
        std::thread::sleep(Duration::from_millis(1500));
        let err = p.shared.error.lock().unwrap().clone();
        p.finish().unwrap();
        assert!(err.is_none(), "{err:?}");
        let size = std::fs::metadata(&path).unwrap().len();
        println!("saved {} ({} KB)", path.display(), size / 1024);
        assert!(size > 10_000);
    }
}
