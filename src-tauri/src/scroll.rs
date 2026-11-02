//! 长截图：在用户滚动（或自动滚动）时持续截取选区，交给 Stitcher 拼接
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

use crate::state::AppState;
use crate::stitch::{Push, Stitcher};
use crate::{capture, clipboard, win32, windows_mgr};

const MAX_HEIGHT: usize = 30000;
const RUN: u8 = 0;
const FINISH: u8 = 1;
const CANCEL: u8 = 2;

pub struct ScrollHandle {
    stop: Arc<AtomicU8>,
    auto: Arc<AtomicBool>,
    /// 选区中心（屏幕物理坐标），自动滚动时光标移到这里
    center: (i32, i32),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    height: usize,
    width: u32,
    preview: Option<String>,
    lost: bool,
    auto: bool,
}

/// rect 为屏幕物理坐标
pub fn start(app: &AppHandle, x: i32, y: i32, w: u32, h: u32) -> anyhow::Result<()> {
    let st = app.state::<AppState>();
    if st.scroll.lock().unwrap().is_some() {
        return Ok(());
    }
    capture::end(app);

    let mon = xcap::Monitor::from_point(x + w as i32 / 2, y + h as i32 / 2)?;
    let (mx, my) = (mon.x()?, mon.y()?);
    let (mw, mh) = (mon.width()?, mon.height()?);
    let scale = mon.scale_factor()? as f64;
    // 选区限制在显示器内
    let rx = (x - mx).clamp(0, mw as i32 - 1) as u32;
    let ry = (y - my).clamp(0, mh as i32 - 1) as u32;
    let rw = w.min(mw - rx).max(1);
    let rh = h.min(mh - ry).max(1);

    let stop = Arc::new(AtomicU8::new(RUN));
    let auto = Arc::new(AtomicBool::new(false));
    *st.scroll.lock().unwrap() = Some(ScrollHandle {
        stop: stop.clone(),
        auto: auto.clone(),
        center: (mx + (rx + rw / 2) as i32, my + (ry + rh / 2) as i32),
    });

    // 选区边框：透明、鼠标穿透、不出现在截图中
    let b = (3.0 * scale).round() as i32;
    let frame = WebviewWindowBuilder::new(app, "scroll-frame", WebviewUrl::App("index.html#scroll-frame".into()))
        .title("ScrollFrame")
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

    // 控制条 + 实时预览：优先放在选区右侧，其次左侧，最后放在选区内
    let cw = (230.0 * scale) as i32;
    let ch = ((rh as f64).min(560.0 * scale)).max(300.0 * scale) as i32;
    let gap = (10.0 * scale) as i32;
    let right_x = mx + (rx + rw) as i32 + gap;
    let left_x = mx + rx as i32 - gap - cw;
    let cx = if right_x + cw <= mx + mw as i32 {
        right_x
    } else if left_x >= mx {
        left_x
    } else {
        mx + (rx + rw) as i32 - cw - gap
    };
    let cy = (my + ry as i32).min(my + mh as i32 - ch).max(my);
    let ctrl = WebviewWindowBuilder::new(app, "scroll-ctrl", WebviewUrl::App("index.html#scroll-ctrl".into()))
        .title("长截图")
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
    // 让用户的滚轮直接作用到选区下方的窗口
    win32::activate_window_at(mx + (rx + rw / 2) as i32, my + (ry + rh / 2) as i32);

    let app2 = app.clone();
    let probe = (mx + (rx + rw / 2) as i32, my + (ry + rh / 2) as i32);
    std::thread::spawn(move || {
        // Monitor 不能跨线程传递，在采集线程里重新获取
        let result = match xcap::Monitor::from_point(probe.0, probe.1) {
            Ok(mon) => run_loop(&app2, &mon, (rx, ry, rw, rh), &stop, &auto),
            Err(_) => None,
        };
        finish(&app2, result);
    });
    Ok(())
}

fn run_loop(
    app: &AppHandle,
    mon: &xcap::Monitor,
    (rx, ry, rw, rh): (u32, u32, u32, u32),
    stop: &AtomicU8,
    auto: &AtomicBool,
) -> Option<image::RgbaImage> {
    let st = app.state::<AppState>();
    let mut stitcher = Stitcher::new();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut dirty = true;
    let mut still = 0u32;
    let mut lost = false;
    let mut lost_streak = 0u32;
    let mut preview_seq = 0u64;

    loop {
        match stop.load(Ordering::Relaxed) {
            FINISH => break,
            CANCEL => return None,
            _ => {}
        }
        let is_auto = auto.load(Ordering::Relaxed);
        if let Ok(frame) = mon.capture_region(rx, ry, rw, rh) {
            match stitcher.push(frame) {
                Push::Moved => {
                    still = 0;
                    lost = false;
                    lost_streak = 0;
                    dirty = true;
                }
                Push::Same => still += 1,
                Push::Lost => {
                    lost = true;
                    lost_streak += 1;
                    dirty = true;
                }
            }
        }
        if stitcher.height() >= MAX_HEIGHT {
            break;
        }
        if dirty && last_emit.elapsed() > Duration::from_millis(300) {
            dirty = false;
            last_emit = Instant::now();
            // 只预览最近约 3 屏，避免长图很高时每次都复制整张图
            let preview = stitcher.tail(rh as usize * 3).and_then(|img| {
                let tw = 200u32.min(img.width());
                let th = ((img.height() as u64 * tw as u64) / img.width() as u64).max(1) as u32;
                let thumb = image::imageops::thumbnail(&img, tw, th);
                let png = clipboard::encode_png(&thumb).ok()?;
                preview_seq += 1;
                let key = format!("scroll/preview/{preview_seq}");
                st.remove_blobs_with_prefix("scroll/preview/");
                st.put_blob(&key, "image/png", png);
                Some(key)
            });
            let _ = app.emit(
                "scroll:progress",
                Progress {
                    height: stitcher.height(),
                    width: rw,
                    preview,
                    lost,
                    auto: is_auto,
                },
            );
        }
        if is_auto {
            // 连续多帧没有变化，说明已经到底
            if still >= 6 {
                break;
            }
            if lost {
                // 没对上：往回滚一格重新对齐；多次仍失败则停止自动滚动，交给用户
                if lost_streak >= 6 {
                    auto.store(false, Ordering::Relaxed);
                    continue;
                }
                win32::wheel(120);
            } else {
                win32::wheel(-120);
            }
            std::thread::sleep(Duration::from_millis(220));
        } else {
            std::thread::sleep(Duration::from_millis(80));
        }
    }
    eprintln!("[scroll] finished, height = {}", stitcher.height());
    stitcher.result()
}

fn finish(app: &AppHandle, result: Option<image::RgbaImage>) {
    let st = app.state::<AppState>();
    *st.scroll.lock().unwrap() = None;
    st.remove_blobs_with_prefix("scroll/preview/");
    for label in ["scroll-frame", "scroll-ctrl"] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.destroy();
        }
    }
    if let Some(img) = result {
        if let Ok(png) = clipboard::encode_png(&img) {
            if let Err(e) = windows_mgr::open_image_window(app, png, "image") {
                eprintln!("open long image failed: {e:?}");
            }
        }
    }
}

pub fn set_auto(app: &AppHandle, on: bool) {
    let st = app.state::<AppState>();
    let guard = st.scroll.lock().unwrap();
    eprintln!("[scroll] auto = {on}");
    if let Some(h) = guard.as_ref() {
        if on {
            win32::set_cursor_pos(h.center.0, h.center.1);
            win32::activate_window_at(h.center.0, h.center.1);
        }
        h.auto.store(on, Ordering::Relaxed);
    }
}

pub fn stop(app: &AppHandle, cancel: bool) {
    let st = app.state::<AppState>();
    let guard = st.scroll.lock().unwrap();
    eprintln!("[scroll] stop cancel = {cancel}");
    if let Some(h) = guard.as_ref() {
        h.stop.store(if cancel { CANCEL } else { FINISH }, Ordering::Relaxed);
    }
}
