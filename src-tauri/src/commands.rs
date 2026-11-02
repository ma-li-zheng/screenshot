use std::collections::HashMap;

use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_autostart::ManagerExt;

use crate::capture::{CaptureSession, MonitorInfo};
use crate::ocr::OcrLine;
use crate::settings::{self, Settings, Usage};
use crate::state::{AppState, ImageWin};
use crate::{capture, clipboard, scroll, shortcut, translate, tray, windows_mgr};

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------- 截图 ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureView {
    session: u64,
    mode: String,
    monitor: MonitorInfo,
    focus: bool,
    cursor: Option<(i32, i32)>,
}

pub fn capture_view(s: &CaptureSession, label: &str) -> Option<CaptureView> {
    let idx: usize = label.strip_prefix("overlay-")?.parse().ok()?;
    let m = s.monitors.get(idx)?.clone();
    Some(CaptureView {
        session: s.id,
        mode: s.mode.clone(),
        monitor: m,
        focus: s.focus == idx,
        cursor: (s.focus == idx).then_some(s.cursor),
    })
}

#[tauri::command]
pub fn capture_info(app: AppHandle, window: WebviewWindow) -> Option<CaptureView> {
    let st = app.state::<AppState>();
    let s = st.capture.lock().unwrap();
    s.as_ref().and_then(|s| capture_view(s, window.label()))
}

#[tauri::command]
pub fn capture_start(app: AppHandle) {
    capture::start(&app, 0);
}

#[tauri::command]
pub fn capture_end(app: AppHandle) {
    capture::end(&app);
}

/// 前端把导出的 PNG 以原始字节传过来；动作通过请求头指定
#[tauri::command]
pub async fn output_image(app: AppHandle, window: WebviewWindow, request: Request<'_>) -> CmdResult<bool> {
    let InvokeBody::Raw(png) = request.body() else {
        return Err("需要原始图片数据".into());
    };
    let png = png.clone();
    let header = |k: &str| {
        request
            .headers()
            .get(k)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let action = header("x-action");
    let x: i32 = header("x-x").parse().unwrap_or(0);
    let y: i32 = header("x-y").parse().unwrap_or(0);
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<bool> {
        match action.as_str() {
            "copy" => clipboard::set_png(&png).map(|_| true),
            "save" => windows_mgr::save_png_dialog(&app, &window, &png),
            "pin" => windows_mgr::create_pin(&app, png, x, y).map(|_| true),
            "ocr" | "translate" => windows_mgr::open_image_window(&app, png, &action).map(|_| true),
            other => anyhow::bail!("未知操作: {other}"),
        }
    })
    .await
    .map_err(err)?
    .map_err(err)
}

// ---------------- 长截图 ----------------

#[tauri::command]
pub fn scroll_start(app: AppHandle, x: i32, y: i32, w: u32, h: u32) -> CmdResult<()> {
    let app2 = app.clone();
    // 创建窗口要在后台线程进行
    std::thread::spawn(move || {
        if let Err(e) = scroll::start(&app2, x, y, w, h) {
            eprintln!("scroll start failed: {e:?}");
            scroll::stop(&app2, true);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn scroll_auto(app: AppHandle, on: bool) {
    scroll::set_auto(&app, on);
}

#[tauri::command]
pub fn scroll_stop(app: AppHandle, cancel: bool) {
    scroll::stop(&app, cancel);
}

// ---------------- 图片窗口（贴图 / OCR / 长图） ----------------

#[tauri::command]
pub fn image_info(app: AppHandle, window: WebviewWindow) -> Option<ImageWin> {
    let st = app.state::<AppState>();
    let info = st.images.lock().unwrap().get(window.label()).cloned();
    info
}

#[tauri::command]
pub fn image_close(app: AppHandle, window: WebviewWindow) {
    windows_mgr::close_image_window(&app, window.label());
}

fn image_png(app: &AppHandle, label: &str) -> CmdResult<Vec<u8>> {
    let st = app.state::<AppState>();
    let key = st
        .images
        .lock()
        .unwrap()
        .get(label)
        .map(|i| i.key.clone())
        .ok_or("图片不存在")?;
    let blob = st.get_blob(&key).ok_or("图片不存在")?;
    Ok(blob.data.as_ref().clone())
}

#[tauri::command]
pub async fn pin_copy(app: AppHandle, window: WebviewWindow) -> CmdResult<()> {
    let png = image_png(&app, window.label())?;
    clipboard::set_png(&png).map_err(err)
}

#[tauri::command]
pub async fn pin_save(app: AppHandle, window: WebviewWindow) -> CmdResult<bool> {
    let png = image_png(&app, window.label())?;
    tauri::async_runtime::spawn_blocking(move || windows_mgr::save_png_dialog(&app, &window, &png))
        .await
        .map_err(err)?
        .map_err(err)
}

#[tauri::command]
pub fn pin_close_all(app: AppHandle) {
    let labels: Vec<String> = app
        .webview_windows()
        .keys()
        .filter(|l| l.starts_with("pin-"))
        .cloned()
        .collect();
    for l in labels {
        windows_mgr::close_image_window(&app, &l);
    }
}

#[tauri::command]
pub fn copy_text(text: String) -> CmdResult<()> {
    clipboard::set_text(&text).map_err(err)
}

// ---------------- OCR ----------------

#[tauri::command]
pub async fn ocr_image(app: AppHandle, key: String) -> CmdResult<Vec<OcrLine>> {
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Vec<OcrLine>> {
        let st = app.state::<AppState>();
        let blob = st.get_blob(&key).ok_or_else(|| anyhow::anyhow!("图片不存在"))?;
        let img = image::load_from_memory(&blob.data)?.to_rgb8();
        let mut guard = st.ocr.lock().unwrap();
        if guard.is_none() {
            let dir = app.path().resource_dir()?.join("ocr");
            *guard = Some(crate::ocr::Engine::load(&dir)?);
        }
        guard.as_mut().unwrap().run(&img)
    })
    .await
    .map_err(err)?
    .map_err(|e| format!("{e:#}"))
}

// ---------------- 翻译 ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateResult {
    texts: Vec<String>,
    to: String,
    provider: String,
}

fn provider_name(p: &str) -> &'static str {
    match p {
        "baidu" => "百度翻译",
        "azure" => "Azure 翻译",
        "openai" => "OpenAI 兼容接口",
        _ => "腾讯云翻译",
    }
}

fn target_lang(s: &Settings, texts: &[String]) -> String {
    match s.translate_target.as_str() {
        "zh" | "en" => s.translate_target.clone(),
        _ if translate::is_mostly_chinese(texts) => "en".into(),
        _ => "zh".into(),
    }
}

async fn translate_online(app: &AppHandle, provider: &str, texts: &[String], to: &str) -> CmdResult<Vec<String>> {
    let st = app.state::<AppState>();
    let s = st.settings.lock().unwrap().clone();
    let chars: u64 = texts.iter().map(|t| t.chars().count() as u64).sum();
    let month = settings::current_month();
    let used = s
        .usage
        .get(provider)
        .filter(|u| u.month == month)
        .map(|u| u.chars)
        .unwrap_or(0);
    let quota = s.quota.get(provider).copied().unwrap_or(0);
    let limit = (quota as f64 * s.quota_ratio.clamp(0.0, 1.0)) as u64;
    if quota > 0 && used + chars > limit {
        return Err(format!(
            "{} 本月已用 {} 字符，再翻译会超过免费额度的 {:.0}%（{} 字符），已暂停。可在设置中调整额度或更换服务。",
            provider_name(provider),
            used,
            s.quota_ratio * 100.0,
            limit
        ));
    }
    let secret = settings::get_secret(provider);
    let out = translate::translate(provider, &secret, &s, texts, to)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let mut cur = st.settings.lock().unwrap();
    let u = cur.usage.entry(provider.to_string()).or_insert_with(Usage::default);
    if u.month != month {
        *u = Usage { month, chars: 0 };
    }
    u.chars += chars;
    let _ = settings::save(app, &cur);
    Ok(out)
}

async fn do_translate(app: &AppHandle, provider: Option<String>, texts: Vec<String>) -> CmdResult<TranslateResult> {
    let s = app.state::<AppState>().settings.lock().unwrap().clone();
    let to = target_lang(&s, &texts);
    let provider = provider.unwrap_or(s.translate_provider);
    let texts = translate_online(app, &provider, &texts, &to).await?;
    Ok(TranslateResult {
        texts,
        to,
        provider: provider_name(&provider).to_string(),
    })
}

#[tauri::command]
pub async fn translate_texts(app: AppHandle, texts: Vec<String>) -> CmdResult<TranslateResult> {
    do_translate(&app, None, texts).await
}

#[tauri::command]
pub async fn test_translate(app: AppHandle, provider: String) -> CmdResult<String> {
    let r = do_translate(&app, Some(provider), vec!["Hello, world!".to_string()]).await?;
    Ok(format!("「Hello, world!」→「{}」", r.texts.join("")))
}

// ---------------- 录屏 ----------------

#[tauri::command]
pub fn record_start(app: AppHandle, x: i32, y: i32, w: u32, h: u32) {
    std::thread::spawn(move || {
        if let Err(e) = crate::record::start(&app, x, y, w, h) {
            let _ = crate::record::stop(&app, true);
            use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
            app.dialog()
                .message(format!("{e:#}"))
                .title("无法开始录屏")
                .kind(MessageDialogKind::Error)
                .show(|_| {});
        }
    });
}

#[tauri::command]
pub fn record_status(app: AppHandle) -> crate::record::RecordStatus {
    crate::record::status(&app)
}

#[tauri::command]
pub fn record_pause(app: AppHandle, paused: bool) {
    crate::record::set_paused(&app, paused);
}

#[tauri::command]
pub fn record_mute(app: AppHandle, source: String, mute: bool) {
    crate::record::set_mute(&app, &source, mute);
}

#[tauri::command]
pub fn record_stop(app: AppHandle, cancel: bool) {
    std::thread::spawn(move || {
        if cancel {
            let _ = crate::record::stop(&app, true);
        } else {
            crate::record::finish_and_show(&app);
        }
    });
}

// ---------------- 文件（录屏结果、GIF） ----------------

fn file_path(app: &AppHandle, id: &str) -> CmdResult<std::path::PathBuf> {
    app.state::<AppState>()
        .files
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .ok_or_else(|| "文件不存在".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    id: String,
    path: String,
    name: String,
    size: u64,
}

#[tauri::command]
pub fn file_info(app: AppHandle, id: String) -> CmdResult<FileInfo> {
    let p = file_path(&app, &id)?;
    Ok(FileInfo {
        size: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
        name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: p.to_string_lossy().into_owned(),
        id,
    })
}

#[tauri::command]
pub fn file_copy(app: AppHandle, id: String) -> CmdResult<()> {
    let p = file_path(&app, &id)?;
    crate::win32::copy_files_to_clipboard(&[p]).map_err(err)
}

#[tauri::command]
pub fn file_reveal(app: AppHandle, id: String) -> CmdResult<()> {
    let p = file_path(&app, &id)?;
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", p.display()))
        .spawn()
        .map(|_| ())
        .map_err(err)
}

#[tauri::command]
pub async fn file_save_as(app: AppHandle, window: WebviewWindow, id: String) -> CmdResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    let src = file_path(&app, &id)?;
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<bool> {
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let picked = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(&name)
            .add_filter(ext.to_uppercase(), &[ext.as_str()])
            .blocking_save_file();
        let Some(dst) = picked else { return Ok(false) };
        std::fs::copy(&src, dst.into_path()?)?;
        Ok(true)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// 保存前端生成的 GIF：与原视频同目录同名，返回新文件的 id
#[tauri::command]
pub fn gif_save(app: AppHandle, request: Request<'_>) -> CmdResult<String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("需要 GIF 数据".into());
    };
    let src_id = request
        .headers()
        .get("x-source")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let src = file_path(&app, &src_id)?;
    let mut dst = src.with_extension("gif");
    let mut n = 2;
    while dst.exists() {
        let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        dst = src.with_file_name(format!("{stem}_{n}.gif"));
        n += 1;
    }
    std::fs::write(&dst, bytes).map_err(err)?;
    let st = app.state::<AppState>();
    let id = st.next_id().to_string();
    st.files.lock().unwrap().insert(id.clone(), dst);
    Ok(id)
}

// ---------------- 设置 ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPayload {
    settings: Settings,
    month: String,
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsPayload {
    let st = app.state::<AppState>();
    let s = st.settings.lock().unwrap().clone();
    SettingsPayload {
        settings: s,
        month: settings::current_month(),
    }
}

#[tauri::command]
pub fn save_settings(app: AppHandle, value: Settings) -> CmdResult<Vec<String>> {
    let st = app.state::<AppState>();
    {
        let mut cur = st.settings.lock().unwrap();
        // 用量由后端维护，不接受前端覆盖
        let usage = cur.usage.clone();
        *cur = value;
        cur.usage = usage;
        settings::save(&app, &cur).map_err(err)?;
    }
    let mut errors = shortcut::apply(&app);
    tray::refresh(&app);
    let autostart = st.settings.lock().unwrap().autostart;
    let al = app.autolaunch();
    let r = if autostart { al.enable() } else { al.disable() };
    if let Err(e) = r {
        if autostart || al.is_enabled().unwrap_or(false) {
            errors.push(format!("开机自启设置失败：{e}"));
        }
    }
    Ok(errors)
}

#[tauri::command]
pub fn get_secret(provider: String) -> HashMap<String, String> {
    settings::get_secret(&provider)
}

#[tauri::command]
pub fn set_secret(provider: String, value: HashMap<String, String>) -> CmdResult<()> {
    settings::set_secret(&provider, &value).map_err(err)
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle, window: WebviewWindow, current: String) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_parent(&window)
            .set_directory(current)
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    windows_mgr::open_settings(&app);
}

#[tauri::command]
pub fn hotkey_errors(app: AppHandle) -> Vec<String> {
    shortcut::apply(&app)
}
