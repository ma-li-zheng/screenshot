use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::{capture, windows_mgr};

const TRAY_ID: &str = "main";

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let (cap, pin) = {
        let st = app.state::<AppState>();
        let s = st.settings.lock().unwrap();
        (s.hotkey_capture.clone(), s.hotkey_pin.clone())
    };
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "capture", format!("截图\t{cap}"), true, None::<&str>)?,
            &MenuItem::with_id(app, "pin", format!("贴剪贴板图片\t{pin}"), true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?,
        ],
    )
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Screenshot 截图")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            // 等托盘菜单完全消失后再截图
            "capture" => capture::start(app, 200),
            "pin" => windows_mgr::pin_from_clipboard(app),
            "settings" => windows_mgr::open_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                capture::start(tray.app_handle(), 0);
            }
        })
        .build(app)?;
    Ok(())
}

/// 快捷键修改后刷新菜单上的提示
pub fn refresh(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}
