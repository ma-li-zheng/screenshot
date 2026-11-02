mod blobs;
mod capture;
mod clipboard;
mod commands;
mod ocr;
mod onnx;
mod record;
mod scroll;
mod settings;
mod shortcut;
mod state;
mod stitch;
mod translate;
mod tray;
mod win32;
mod windows_mgr;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        // 必须最先注册：再次启动时只打开设置窗口
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            windows_mgr::open_settings(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(shortcut::plugin())
        .register_uri_scheme_protocol("shot", blobs::protocol)
        .on_window_event(|window, event| {
            // 用户点窗口右上角关闭时，释放该窗口的图片
            if let tauri::WindowEvent::Destroyed = event {
                let st = window.state::<state::AppState>();
                let removed = st.images.lock().unwrap().remove(window.label());
                if let Some(info) = removed {
                    st.blobs.lock().unwrap().remove(&info.key);
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(state::AppState::new(settings::load(&handle)));
            tray::create(&handle)?;
            let errors = shortcut::apply(&handle);
            if !errors.is_empty() {
                use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                handle
                    .dialog()
                    .message(format!("{}\n\n请在托盘图标 → 设置 中修改快捷键。", errors.join("\n")))
                    .title("Screenshot")
                    .kind(MessageDialogKind::Warning)
                    .show(|_| {});
            }
            capture::prewarm(&handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::capture_info,
            commands::capture_start,
            commands::capture_end,
            commands::output_image,
            commands::scroll_start,
            commands::scroll_auto,
            commands::scroll_stop,
            commands::image_info,
            commands::image_close,
            commands::pin_copy,
            commands::pin_save,
            commands::pin_close_all,
            commands::copy_text,
            commands::ocr_image,
            commands::translate_texts,
            commands::test_translate,
            commands::get_settings,
            commands::save_settings,
            commands::get_secret,
            commands::set_secret,
            commands::record_start,
            commands::record_status,
            commands::record_pause,
            commands::record_mute,
            commands::record_stop,
            commands::file_info,
            commands::file_copy,
            commands::file_reveal,
            commands::file_save_as,
            commands::gif_save,
            commands::pick_folder,
            commands::open_settings,
            commands::hotkey_errors,
        ])
        .build(tauri::generate_context!())
        .expect("启动失败")
        .run(|_, e| {
            // 没有窗口时保持在托盘运行
            if let tauri::RunEvent::ExitRequested { api, code, .. } = e {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
