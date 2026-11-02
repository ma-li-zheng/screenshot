//! 截图主流程：截取所有显示器 → 显示遮罩窗口 → 前端选区和批注
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

use crate::state::AppState;
use crate::win32::{self, Rect};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WinRect {
    pub rect: Rect,
    pub children: Vec<Rect>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub index: usize,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    /// shot:// 协议里的原始 RGBA 数据
    pub key: String,
    /// 窗口矩形，相对本显示器左上角的物理像素
    pub windows: Vec<WinRect>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSession {
    pub id: u64,
    pub monitors: Vec<MonitorInfo>,
    /// 光标所在的显示器
    pub focus: usize,
    /// 光标位置（相对 focus 显示器，物理像素）
    pub cursor: (i32, i32),
}

pub fn overlay_label(i: usize) -> String {
    format!("overlay-{i}")
}

pub fn is_active(app: &AppHandle) -> bool {
    let st = app.state::<AppState>();
    let active = st.capture.lock().unwrap().is_some() || st.scroll.lock().unwrap().is_some();
    active
}

/// 从快捷键或托盘触发；在后台线程执行，避免阻塞事件循环
pub fn start(app: &AppHandle, delay_ms: u64) {
    if is_active(app) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        if let Err(e) = start_inner(&app) {
            eprintln!("capture failed: {e:?}");
            end(&app);
        }
    });
}

struct Geo {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

fn start_inner(app: &AppHandle) -> anyhow::Result<()> {
    let st = app.state::<AppState>();
    let id = st.next_id();
    let monitors = xcap::Monitor::all()?;
    let (cx, cy) = win32::cursor_pos();
    let wins = win32::enum_windows();

    let mut infos = Vec::new();
    let mut geos = Vec::new();
    let mut focus = 0;
    let mut cursor = (0, 0);
    for (i, m) in monitors.iter().enumerate() {
        let (x, y) = (m.x()?, m.y()?);
        let scale = m.scale_factor()? as f64;
        let img = m.capture_image()?;
        // 截图尺寸以实际图像为准
        let (w, h) = (img.width(), img.height());
        let key = format!("cap/{id}/{i}");
        st.put_blob(&key, "application/octet-stream", img.into_raw());

        let mon = Rect {
            x,
            y,
            w: w as i32,
            h: h as i32,
        };
        if cx >= x && cx < x + w as i32 && cy >= y && cy < y + h as i32 {
            focus = i;
            cursor = (cx - x, cy - y);
        }
        let rel = |r: &Rect| Rect {
            x: r.x - x,
            y: r.y - y,
            w: r.w,
            h: r.h,
        };
        let windows = wins
            .iter()
            .filter(|wi| wi.rect.intersects(&mon))
            .map(|wi| WinRect {
                rect: rel(&wi.rect),
                children: wi
                    .children
                    .iter()
                    .filter(|c| c.intersects(&mon))
                    .map(rel)
                    .collect(),
            })
            .collect();
        infos.push(MonitorInfo {
            index: i,
            x,
            y,
            width: w,
            height: h,
            scale,
            key,
            windows,
        });
        geos.push(Geo { x, y, w, h });
    }

    let session = CaptureSession {
        id,
        monitors: infos,
        focus,
        cursor,
    };
    *st.capture.lock().unwrap() = Some(session.clone());
    ensure_overlays(app, &geos)?;
    app.emit("capture:begin", &session)?;
    Ok(())
}

/// 每个显示器一个遮罩窗口，创建后常驻隐藏，下次截图直接复用
fn ensure_overlays(app: &AppHandle, geos: &[Geo]) -> anyhow::Result<()> {
    for (i, g) in geos.iter().enumerate() {
        let label = overlay_label(i);
        let win = match app.get_webview_window(&label) {
            Some(w) => w,
            None => WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html#capture".into()))
                .title("ScreenshotOverlay")
                .decorations(false)
                .resizable(false)
                .skip_taskbar(true)
                .always_on_top(true)
                .shadow(false)
                .visible(false)
                .focused(false)
                .build()?,
        };
        win.set_position(PhysicalPosition::new(g.x, g.y))?;
        win.set_size(PhysicalSize::new(g.w, g.h))?;
    }
    // 显示器数量变少时，关掉多余的遮罩窗口
    let mut i = geos.len();
    while let Some(w) = app.get_webview_window(&overlay_label(i)) {
        let _ = w.destroy();
        i += 1;
    }
    Ok(())
}

/// 启动时预先创建遮罩窗口，第一次截图也能很快弹出
pub fn prewarm(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let geos: Vec<Geo> = xcap::Monitor::all()
            .unwrap_or_default()
            .iter()
            .filter_map(|m| {
                Some(Geo {
                    x: m.x().ok()?,
                    y: m.y().ok()?,
                    w: m.width().ok()?,
                    h: m.height().ok()?,
                })
            })
            .collect();
        let _ = ensure_overlays(&app, &geos);
    });
}

/// 结束截图：隐藏所有遮罩、释放截图数据
pub fn end(app: &AppHandle) {
    let st = app.state::<AppState>();
    let session = st.capture.lock().unwrap().take();
    for (label, w) in app.webview_windows() {
        if label.starts_with("overlay-") {
            let _ = w.hide();
        }
    }
    if let Some(s) = session {
        st.remove_blobs_with_prefix(&format!("cap/{}/", s.id));
    }
    let _ = app.emit("capture:end", ());
}
