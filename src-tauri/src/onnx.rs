//! ONNX Runtime 全局环境：一个进程只能有一个，OCR 和离线翻译共用
use std::sync::Mutex;

use anyhow::anyhow;
use ort::environment::Environment;

static ENV: Mutex<Option<Environment>> = Mutex::new(None);

pub fn env() -> anyhow::Result<Environment> {
    let mut guard = ENV.lock().unwrap();
    if let Some(e) = guard.as_ref() {
        return Ok(e.clone());
    }
    let e = ort::init().with_name("screenshot").build().map_err(|e| anyhow!("{e}"))?;
    *guard = Some(e.clone());
    Ok(e)
}
