//! 贴图窗口、图片结果窗口（OCR / 翻译 / 长图编辑）、设置窗口
use std::path::PathBuf;

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tauri_plugin_dialog::DialogExt;

use crate::clipboard;
use crate::state::{AppState, ImageWin};

/// 贴图窗口四周留给阴影的边距（CSS 像素）
pub const PIN_MARGIN: f64 = 8.0;

fn scale_at(x: i32, y: i32) -> f64 {
    xcap::Monitor::from_point(x, y)
        .ok()
        .and_then(|m| m.scale_factor().ok())
        .map(|s| s as f64)
        .unwrap_or(1.0)
}

/// 把一张 PNG 钉到屏幕上，(x, y) 是图片左上角的屏幕物理坐标
pub fn create_pin(app: &AppHandle, png: Vec<u8>, x: i32, y: i32) -> anyhow::Result<()> {
    let st = app.state::<AppState>();
    let (w, h) = clipboard::png_size(&png)?;
    let id = st.next_id();
    let key = format!("pin/{id}");
    let label = format!("pin-{id}");
    st.put_blob(&key, "image/png", png);
    st.images.lock().unwrap().insert(
        label.clone(),
        ImageWin {
            key,
            width: w,
            height: h,
            mode: "pin".into(),
        },
    );

    let m = (PIN_MARGIN * scale_at(x, y)).round() as i32;
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html#pin".into()))
        .title("贴图")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .build()?;
    win.set_position(PhysicalPosition::new(x - m, y - m))?;
    win.set_size(PhysicalSize::new(w + 2 * m as u32, h + 2 * m as u32))?;
    Ok(())
}

pub fn pin_from_clipboard(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(img) = clipboard::get_image() else {
            return;
        };
        let Ok(png) = clipboard::encode_png(&img) else {
            return;
        };
        let (cx, cy) = crate::win32::cursor_pos();
        let _ = create_pin(&app, png, cx, cy);
    });
}

/// 打开图片结果窗口：mode = ocr | translate | image（长截图编辑）
pub fn open_image_window(app: &AppHandle, png: Vec<u8>, mode: &str) -> anyhow::Result<()> {
    let st = app.state::<AppState>();
    let (w, h) = clipboard::png_size(&png)?;
    let id = st.next_id();
    let key = format!("img/{id}");
    let label = format!("{mode}-{id}");
    st.put_blob(&key, "image/png", png);
    st.images.lock().unwrap().insert(
        label.clone(),
        ImageWin {
            key,
            width: w,
            height: h,
            mode: mode.into(),
        },
    );

    let (cx, cy) = crate::win32::cursor_pos();
    let mon = xcap::Monitor::from_point(cx, cy)?;
    let scale = mon.scale_factor()? as f64;
    let (mw, mh) = (mon.width()? as f64 / scale, mon.height()? as f64 / scale);
    let (title, lw, lh) = match mode {
        "ocr" => ("提取文字", 1100.0_f64.min(mw * 0.9), 700.0_f64.min(mh * 0.85)),
        "translate" => ("翻译", 1100.0_f64.min(mw * 0.9), 700.0_f64.min(mh * 0.85)),
        _ => (
            "长截图",
            (w as f64 / scale + 40.0).clamp(640.0, mw * 0.9),
            mh * 0.85,
        ),
    };
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(format!("index.html#{mode}").into()))
        .title(title)
        .inner_size(lw, lh)
        .min_inner_size(480.0, 360.0)
        .visible(false)
        .build()?;
    // 放到光标所在显示器的中央
    let (ow, oh) = (lw * scale, lh * scale);
    let px = mon.x()? + ((mon.width()? as f64 - ow) / 2.0) as i32;
    let py = mon.y()? + ((mon.height()? as f64 - oh) / 2.0) as i32;
    win.set_position(PhysicalPosition::new(px, py))?;
    win.show()?;
    win.set_focus()?;
    Ok(())
}

/// 录屏结果窗口：预览、复制文件、打开文件夹、另存为、转 GIF
pub fn open_video_window(app: &AppHandle, path: &std::path::Path) -> anyhow::Result<()> {
    let st = app.state::<AppState>();
    let id = st.next_id();
    st.files.lock().unwrap().insert(id.to_string(), path.to_path_buf());
    let label = format!("video-{id}");
    let (cx, cy) = crate::win32::cursor_pos();
    let mon = xcap::Monitor::from_point(cx, cy)?;
    let scale = mon.scale_factor()? as f64;
    let (lw, lh) = (880.0, 640.0);
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(format!("index.html#video={id}").into()))
        .title("录屏完成")
        .inner_size(lw, lh)
        .min_inner_size(520.0, 400.0)
        .visible(false)
        .build()?;
    let px = mon.x()? + ((mon.width()? as f64 - lw * scale) / 2.0) as i32;
    let py = mon.y()? + ((mon.height()? as f64 - lh * scale) / 2.0) as i32;
    win.set_position(PhysicalPosition::new(px, py))?;
    win.show()?;
    win.set_focus()?;
    Ok(())
}

pub fn close_image_window(app: &AppHandle, label: &str) {
    let st = app.state::<AppState>();
    if let Some(info) = st.images.lock().unwrap().remove(label) {
        st.blobs.lock().unwrap().remove(&info.key);
    }
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.destroy();
    }
}

pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("index.html#settings".into()))
            .title("Screenshot 设置")
            .inner_size(720.0, 640.0)
            .min_inner_size(600.0, 480.0)
            .center()
            .build();
    });
}

fn default_file_name(app: &AppHandle) -> (PathBuf, String, String) {
    let st = app.state::<AppState>();
    let s = st.settings.lock().unwrap().clone();
    let ext = if s.image_format == "jpg" { "jpg" } else { "png" };
    let name = format!(
        "{}_{}.{}",
        s.file_name_prefix,
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        ext
    );
    (PathBuf::from(s.save_dir), name, ext.to_string())
}

/// 弹出另存为对话框并写入文件。返回 false 表示用户取消
pub fn save_png_dialog(app: &AppHandle, parent: &WebviewWindow, png: &[u8]) -> anyhow::Result<bool> {
    let (dir, name, ext) = default_file_name(app);
    let _ = std::fs::create_dir_all(&dir);
    let mut dlg = app
        .dialog()
        .file()
        .set_parent(parent)
        .set_directory(&dir)
        .set_file_name(&name);
    dlg = if ext == "jpg" {
        dlg.add_filter("JPEG 图片", &["jpg", "jpeg"]).add_filter("PNG 图片", &["png"])
    } else {
        dlg.add_filter("PNG 图片", &["png"]).add_filter("JPEG 图片", &["jpg", "jpeg"])
    };
    let Some(path) = dlg.blocking_save_file() else {
        return Ok(false);
    };
    let path = path.into_path()?;
    write_image(&path, png)?;
    Ok(true)
}

pub fn write_image(path: &std::path::Path, png: &[u8]) -> anyhow::Result<()> {
    let is_jpg = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
        .unwrap_or(false);
    if is_jpg {
        let img = image::DynamicImage::ImageRgba8(clipboard::decode_png(png)?).to_rgb8();
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, 92).encode_image(&img)?;
    } else {
        std::fs::write(path, png)?;
    }
    Ok(())
}
