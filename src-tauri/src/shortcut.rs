use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::AppState;
use crate::{capture, windows_mgr};

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, sc, ev| {
            if ev.state != ShortcutState::Pressed {
                return;
            }
            let (cap, pin, rec) = {
                let st = app.state::<AppState>();
                let s = st.settings.lock().unwrap();
                (
                    s.hotkey_capture.parse::<Shortcut>().ok(),
                    s.hotkey_pin.parse::<Shortcut>().ok(),
                    s.hotkey_record.parse::<Shortcut>().ok(),
                )
            };
            if cap.as_ref() == Some(sc) {
                capture::start(app, 0);
            } else if pin.as_ref() == Some(sc) {
                windows_mgr::pin_from_clipboard(app);
            } else if rec.as_ref() == Some(sc) {
                // 再按一次停止录制
                crate::record::toggle(app, 0);
            }
        })
        .build()
}

/// 按当前设置重新注册快捷键，返回失败信息
pub fn apply(app: &AppHandle) -> Vec<String> {
    let (cap, pin, rec) = {
        let st = app.state::<AppState>();
        let s = st.settings.lock().unwrap();
        (s.hotkey_capture.clone(), s.hotkey_pin.clone(), s.hotkey_record.clone())
    };
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut errors = Vec::new();
    for (name, key) in [("截图", cap), ("贴图", pin), ("录屏", rec)] {
        if key.trim().is_empty() {
            continue;
        }
        match key.parse::<Shortcut>() {
            Ok(s) => {
                if gs.register(s).is_err() {
                    errors.push(format!("{name}快捷键 {key} 注册失败，可能已被其他软件占用"));
                }
            }
            Err(_) => errors.push(format!("{name}快捷键 {key} 格式不正确")),
        }
    }
    errors
}
