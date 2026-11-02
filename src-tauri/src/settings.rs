use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// 形如 2026-11
    pub month: String,
    pub chars: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkey_capture: String,
    pub hotkey_pin: String,
    pub save_dir: String,
    /// png | jpg
    pub image_format: String,
    pub file_name_prefix: String,
    pub autostart: bool,
    /// offline（内置 Opus-MT）| online（在线 API）
    pub translate_engine: String,
    /// 离线模型未下载或失败时改用在线 API
    pub offline_fallback: bool,
    /// 模型下载源：modelscope | huggingface
    pub model_mirror: String,
    /// tencent | baidu | azure | openai
    pub translate_provider: String,
    /// auto | zh | en
    pub translate_target: String,
    pub azure_region: String,
    /// OpenAI 兼容接口：地址（到 /v1 为止）和模型名
    pub openai_base_url: String,
    pub openai_model: String,
    /// 每个服务商每月的免费额度（字符）
    pub quota: HashMap<String, u64>,
    /// 用到额度的多少比例就暂停，0~1
    pub quota_ratio: f64,
    pub usage: HashMap<String, Usage>,
}

impl Default for Settings {
    fn default() -> Self {
        let save_dir = dirs::picture_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
            .join("Screenshot");
        Self {
            hotkey_capture: "Alt+Shift+A".into(),
            hotkey_pin: "Alt+Shift+P".into(),
            save_dir: save_dir.to_string_lossy().into_owned(),
            image_format: "png".into(),
            file_name_prefix: "Screenshot".into(),
            autostart: false,
            translate_engine: "offline".into(),
            offline_fallback: false,
            model_mirror: "modelscope".into(),
            translate_provider: "tencent".into(),
            translate_target: "auto".into(),
            azure_region: String::new(),
            openai_base_url: "https://api.deepseek.com/v1".into(),
            openai_model: "deepseek-chat".into(),
            quota: HashMap::from([
                ("tencent".to_string(), 5_000_000),
                ("baidu".to_string(), 1_000_000),
                ("azure".to_string(), 2_000_000),
                ("openai".to_string(), 0),
            ]),
            quota_ratio: 0.95,
            usage: HashMap::new(),
        }
    }
}

fn path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("settings.json")
}

pub fn load(app: &AppHandle) -> Settings {
    std::fs::read_to_string(path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, s: &Settings) -> anyhow::Result<()> {
    let p = path(app);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(s)?)?;
    Ok(())
}

pub fn current_month() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

// ---------- 翻译密钥：存放在 Windows 凭据管理器 ----------

const KEYRING_SERVICE: &str = "Screenshot-Translate";

pub fn get_secret(provider: &str) -> HashMap<String, String> {
    keyring::Entry::new(KEYRING_SERVICE, provider)
        .ok()
        .and_then(|e| e.get_password().ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn set_secret(provider: &str, value: &HashMap<String, String>) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, provider)?;
    if value.values().all(|v| v.trim().is_empty()) {
        let _ = entry.delete_credential();
    } else {
        entry.set_password(&serde_json::to_string(value)?)?;
    }
    Ok(())
}
