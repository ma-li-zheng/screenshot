use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::capture::CaptureSession;
use crate::scroll::ScrollHandle;
use crate::settings::Settings;

/// 通过 shot:// 协议提供给前端的二进制数据
#[derive(Clone)]
pub struct Blob {
    pub mime: &'static str,
    pub data: Arc<Vec<u8>>,
}

/// 贴图窗口、OCR 窗口、长图编辑窗口共用的图片信息
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageWin {
    pub key: String,
    pub width: u32,
    pub height: u32,
    /// pin | ocr | translate | image
    pub mode: String,
}

pub struct AppState {
    pub blobs: Mutex<HashMap<String, Blob>>,
    pub capture: Mutex<Option<CaptureSession>>,
    pub settings: Mutex<Settings>,
    pub scroll: Mutex<Option<ScrollHandle>>,
    pub images: Mutex<HashMap<String, ImageWin>>,
    pub ocr: Mutex<Option<crate::ocr::Engine>>,
    /// 已加载的离线翻译模型，按方向（zh-en / en-zh）
    pub mt: Mutex<HashMap<String, crate::mt::Translator>>,
    /// 正在下载的模型及其取消标志
    pub downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
    seq: AtomicU64,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
            capture: Mutex::new(None),
            settings: Mutex::new(settings),
            scroll: Mutex::new(None),
            images: Mutex::new(HashMap::new()),
            ocr: Mutex::new(None),
            mt: Mutex::new(HashMap::new()),
            downloads: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(1),
        }
    }

    pub fn next_id(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn put_blob(&self, key: impl Into<String>, mime: &'static str, data: Vec<u8>) {
        self.blobs.lock().unwrap().insert(
            key.into(),
            Blob {
                mime,
                data: Arc::new(data),
            },
        );
    }

    pub fn get_blob(&self, key: &str) -> Option<Blob> {
        self.blobs.lock().unwrap().get(key).cloned()
    }

    pub fn remove_blobs_with_prefix(&self, prefix: &str) {
        self.blobs.lock().unwrap().retain(|k, _| !k.starts_with(prefix));
    }
}
