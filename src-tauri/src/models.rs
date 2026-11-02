//! 离线翻译模型的下载与管理（Opus-MT 中→英、英→中）
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// 支持的方向
pub const DIRECTIONS: [&str; 2] = ["zh-en", "en-zh"];

/// (仓库内路径, 本地文件名, 大约大小)
const FILES: [(&str, &str, u64); 3] = [
    ("onnx/encoder_model_quantized.onnx", "encoder.onnx", 52_899_742),
    ("onnx/decoder_model_merged_quantized.onnx", "decoder.onnx", 60_212_804),
    ("tokenizer.json", "tokenizer.json", 6_381_339),
];

/// 模型目录：优先放在程序目录下（例如 D:\software\Screenshot\models），不可写时放到用户数据目录
pub fn root(app: &AppHandle) -> PathBuf {
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("models"))) {
        if std::fs::create_dir_all(&dir).is_ok() {
            let probe = dir.join(".write-test");
            if std::fs::write(&probe, b"ok").is_ok() {
                let _ = std::fs::remove_file(probe);
                return dir;
            }
        }
    }
    let dir = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("models");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn dir(app: &AppHandle, direction: &str) -> PathBuf {
    root(app).join(format!("opus-mt-{direction}"))
}

pub fn installed(app: &AppHandle, direction: &str) -> bool {
    let d = dir(app, direction);
    FILES.iter().all(|(_, name, _)| d.join(name).is_file())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    direction: String,
    installed: bool,
    downloading: bool,
    size: u64,
    path: String,
}

pub fn status(app: &AppHandle) -> Vec<ModelStatus> {
    let st = app.state::<AppState>();
    let busy = st.downloads.lock().unwrap();
    DIRECTIONS
        .iter()
        .map(|d| {
            let p = dir(app, d);
            ModelStatus {
                direction: d.to_string(),
                installed: installed(app, d),
                downloading: busy.contains_key(*d),
                size: FILES.iter().map(|(_, n, _)| p.join(n).metadata().map(|m| m.len()).unwrap_or(0)).sum(),
                path: p.to_string_lossy().into_owned(),
            }
        })
        .collect()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    direction: String,
    received: u64,
    total: u64,
    /// downloading | done | error | cancelled
    state: String,
    message: String,
}

/// 下载源：ModelScope（国内 CDN，默认）或 Hugging Face，两边文件相同
fn file_url(mirror: &str, direction: &str, remote: &str) -> String {
    if mirror == "huggingface" {
        format!("https://huggingface.co/Xenova/opus-mt-{direction}/resolve/main/{remote}")
    } else {
        format!("https://www.modelscope.cn/models/Xenova/opus-mt-{direction}/resolve/master/{remote}")
    }
}

pub async fn download(app: AppHandle, direction: String, mirror: String) -> anyhow::Result<()> {
    if !DIRECTIONS.contains(&direction.as_str()) {
        bail!("不支持的方向: {direction}");
    }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let st = app.state::<AppState>();
        let mut busy = st.downloads.lock().unwrap();
        if busy.contains_key(&direction) {
            return Ok(());
        }
        busy.insert(direction.clone(), cancel.clone());
    }
    let emit = |received: u64, total: u64, state: &str, message: String| {
        let _ = app.emit(
            "model:progress",
            Progress {
                direction: direction.clone(),
                received,
                total,
                state: state.into(),
                message,
            },
        );
    };
    let result = fetch(&dir(&app, &direction), &direction, &mirror, &cancel, &emit).await;
    app.state::<AppState>().downloads.lock().unwrap().remove(&direction);
    // 已加载的旧模型作废，下次翻译重新加载
    app.state::<AppState>().mt.lock().unwrap().remove(&direction);
    match &result {
        Ok(()) => emit(1, 1, "done", String::new()),
        Err(_) if cancel.load(Ordering::Relaxed) => emit(0, 1, "cancelled", String::new()),
        Err(e) => emit(0, 1, "error", format!("{e:#}")),
    }
    result
}

/// 下载一个方向的全部文件到 target，并校验能否加载
async fn fetch(
    target: &std::path::Path,
    direction: &str,
    mirror: &str,
    cancel: &AtomicBool,
    emit: &impl Fn(u64, u64, &str, String),
) -> anyhow::Result<()> {
    std::fs::create_dir_all(target)?;
    // 下载源会拒绝没有 User-Agent 的请求
    let client = reqwest::Client::builder()
        .user_agent(concat!("Screenshot/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()?;
    let total: u64 = FILES.iter().map(|f| f.2).sum();
    let mut done: u64 = 0;
    for (remote, name, approx) in FILES {
        let url = file_url(mirror, direction, remote);
        let mut resp = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("无法连接下载服务器：{url}"))?;
        if !resp.status().is_success() {
            bail!("下载失败（HTTP {}）：{url}", resp.status());
        }
        let part = target.join(format!("{name}.part"));
        let mut file = std::fs::File::create(&part)?;
        let mut got: u64 = 0;
        let mut last_emit = std::time::Instant::now();
        while let Some(chunk) = resp.chunk().await.context("下载中断，请重试")? {
            if cancel.load(Ordering::Relaxed) {
                drop(file);
                let _ = std::fs::remove_file(&part);
                bail!("已取消");
            }
            std::io::Write::write_all(&mut file, &chunk)?;
            got += chunk.len() as u64;
            if last_emit.elapsed().as_millis() > 200 {
                last_emit = std::time::Instant::now();
                emit(done + got, total, "downloading", String::new());
            }
        }
        drop(file);
        // 大小明显不对说明下载的是错误页面
        if got < approx / 2 {
            let _ = std::fs::remove_file(&part);
            bail!("{name} 下载不完整（{got} 字节），请更换下载源后重试");
        }
        std::fs::rename(&part, target.join(name))?;
        done += got;
    }
    // 校验：能否成功加载
    let dir_clone = target.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || crate::mt::Translator::load(&dir_clone).map(|_| ()))
        .await?
        .context("模型校验失败，请删除后重新下载")?;
    Ok(())
}

pub fn cancel(app: &AppHandle, direction: &str) {
    if let Some(flag) = app.state::<AppState>().downloads.lock().unwrap().get(direction) {
        flag.store(true, Ordering::Relaxed);
    }
}

pub fn delete(app: &AppHandle, direction: &str) -> anyhow::Result<()> {
    app.state::<AppState>().mt.lock().unwrap().remove(direction);
    let d = dir(app, direction);
    if d.exists() {
        std::fs::remove_dir_all(d)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实下载英→中模型：cargo test --lib models -- --ignored --nocapture
    #[test]
    #[ignore]
    fn downloads_from_modelscope() {
        let target = std::path::PathBuf::from(r"E:\screenshot\.downloads\opus\en-zh-dl");
        let _ = std::fs::remove_dir_all(&target);
        let cancel = AtomicBool::new(false);
        let t = std::time::Instant::now();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let last = std::sync::Mutex::new(0u64);
        rt.block_on(fetch(&target, "en-zh", "modelscope", &cancel, &|got, total, _, _| {
            let mut l = last.lock().unwrap();
            if got - *l > 20_000_000 {
                *l = got;
                println!("  {} / {} MB", got / 1_000_000, total / 1_000_000);
            }
        }))
        .unwrap();
        println!("下载并校验完成，用时 {:?}", t.elapsed());
        // OCR 与两个方向的翻译模型同时加载，确认共用同一个 ONNX 环境
        let _ocr = crate::ocr::Engine::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/ocr"))).unwrap();
        let _zh_en = crate::mt::Translator::load(std::path::Path::new(r"E:\screenshot\.downloads\opus\zh-en")).unwrap();
        let mut mt = crate::mt::Translator::load(&target).unwrap();
        let src = vec![
            "The screenshot tool supports OCR and translation.".to_string(),
            "Pinned images stay on top of other windows.".to_string(),
            "Click Save to export the image as PNG.".to_string(),
        ];
        for (a, b) in src.iter().zip(mt.translate(&src).unwrap()) {
            println!("{a}
  → {b}");
        }
    }
}
