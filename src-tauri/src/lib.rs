mod app_log;
mod autostart;
mod clipboard_service;
mod commands;
mod database;
mod models;
mod paths;
mod popup;
mod settings;
mod shortcut;
mod tray;
mod update_service;
mod window_control;

use commands::*;
use models::AppState;
use tauri::{Manager, RunEvent};
use std::panic;

fn log_startup_issue(state: &AppState, message: &str) {
    // 启动诊断写入文件而不是标准错误，是为了让普通开发/运行命令保持干净，同时仍能通过日志定位启动问题。
    // Startup diagnostics are written to the log file instead of stderr so normal dev/runtime commands stay clean while issues remain traceable.
    app_log::info(&state.paths, "startup", message);
}

// 将 Tauri 启动流程放在库入口中，是为了匹配 Cargo.toml 中的 clipanchor_lib 目标，并让桌面入口与潜在移动入口复用同一套初始化逻辑。
// The Tauri bootstrap lives in the library entrypoint to match the clipanchor_lib target in Cargo.toml and let desktop and potential mobile entrypoints share the same initialization path.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = match AppState::new() {
        Ok(value) => value,
        Err(error) => {
            // 这里尚未拿到便携日志路径，只保留致命初始化错误输出，避免无日志时静默失败。
            // The portable log path is not available yet, so only fatal storage initialization errors are printed to avoid silent failure.
            eprintln!("ClipAnchor failed to initialize portable data storage: {}", error);
            return;
        }
    };

    let panic_log_paths = state.paths.clone();
    panic::set_hook(Box::new(move |info| {
        // Panic 详情只进入运行日志，是为了避免普通命令行运行时出现噪声，同时保留崩溃排查线索。
        // Panic details go only to runtime logs to avoid noisy console output while preserving crash diagnostics.
        app_log::error(&panic_log_paths, "panic", format!("{}", info));
    }));

    let app = match tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 第二次启动只唤醒已有主窗口，是为了避免两个进程同时监听剪贴板和写入同一个便携数据库。
            // A second launch only wakes the existing main window so two processes cannot monitor the clipboard and write the same portable database.
            app_log::info(&app.state::<AppState>().paths, "single_instance", "second launch requested; activating existing main window");
            let _ = crate::window_control::activate_main_window(app);
        }))
        .plugin(shortcut::plugin())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            save_settings,
            set_pin_service,
            set_history_service,
            set_privacy_mode,
            set_privacy_filter_mode,
            set_autostart,
            list_history,
            delete_records,
            delete_records_force,
            clear_all_data,
            delete_history_before_days,
            toggle_record_pin,
            create_text_record,
            update_text_record,
            pin_history_item,
            validate_record,
            validate_favorites,
            toggle_popup_favorite,
            copy_item,
            get_popup_item,
            read_image_data_url,
            read_file_previews,
            close_popup,
            pin_popup,
            resize_popup,
            refresh_popup_shape,
            save_popup_position,
            open_position_overlay,
            export_history,
            import_history,
            export_history_to_path,
            import_history_from_path,
            get_data_usage,
            get_log_status,
            clear_logs,
            open_log_folder,
            get_update_status,
            check_update,
            minimize_window,
            toggle_maximize_window,
            close_main_window,
            quit_app
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>().inner().clone();
            let _ = app_log::init(&state.paths);
            log_startup_issue(&state, "Tauri setup started");
            // 启动期的托盘、快捷键和剪贴板监听都降级为可恢复错误，避免某个系统能力失败就让整个程序退出。
            // Tray, shortcut, and clipboard startup failures are treated as recoverable so one OS capability cannot close the whole app.
            if let Err(error) = tray::install_tray(app.handle()) {
                log_startup_issue(&state, &format!("Tray initialization skipped: {}", error));
            }
            if let Err(error) = database::init(&state.paths) {
                log_startup_issue(&state, &format!("Database initialization failed: {}", error));
            } else {
                log_startup_issue(&state, "Database initialized");
            }
            if let Err(error) = clipboard_service::ensure_monitor(handle.clone(), state.clone()) {
                log_startup_issue(&state, &format!("Clipboard monitor initialization skipped: {}", error));
            } else {
                log_startup_issue(&state, "Clipboard monitor initialized");
            }
            match state.settings.lock() {
                Ok(settings) => {
                    if let Err(error) = shortcut::sync_shortcuts(&handle, &settings.shortcuts) {
                        log_startup_issue(&state, &format!("Global shortcut registration skipped: {}", error));
                    } else {
                        log_startup_issue(&state, "Global shortcuts initialized");
                    }
                }
                Err(error) => log_startup_issue(&state, &format!("Settings lock unavailable during startup: {}", error)),
            }
            let lite_startup = window_control::should_start_in_lite_mode();
            let _ = update_service::startup_background_check(&state.paths, lite_startup);
            if lite_startup {
                // 自启动参数会保持主窗口隐藏，只留下托盘和后台监听，避免开机时打断用户桌面恢复流程。
                // The startup flag keeps the main window hidden, leaving only tray and background monitoring so sign-in is not interrupted.
                let _ = window_control::hide_main_window(&handle);
                log_startup_issue(&state, "Startup Lite mode active; main window remains hidden");
            } else if let Err(error) = window_control::activate_main_window(&handle) {
                log_startup_issue(&state, &format!("Main window activation skipped: {}", error));
            }
            log_startup_issue(&state, "Tauri setup finished");
            Ok(())
        })
        .build(tauri::generate_context!()) {
            Ok(app) => app,
            Err(error) => {
                // 构建 Tauri 应用失败发生在运行期日志初始化之后但应用尚不可用，因此保留终端错误帮助开发者直接定位。
                // Tauri build failure happens after log setup but before the app is usable, so stderr is kept to help developers diagnose immediately.
                eprintln!("ClipAnchor failed to build Tauri application: {}", error);
                return;
            }
        };

    app.run(|app_handle, event| {
        match event {
            RunEvent::ExitRequested { api, code, .. } => {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    log_startup_issue(state.inner(), &format!("Exit requested with code {:?}", code));
                    clipboard_service::stop_monitor(state.inner());
                }
                if code.is_none() {
                    // 主窗口关闭只隐藏到托盘，是为了避免 WebView2 偶发的窗口销毁错误让 dev 进程直接退出。
                    // Closing the main window hides it to the tray so transient WebView2 destroy errors do not terminate the dev process.
                    api.prevent_exit();
                    let _ = window_control::hide_main_window(app_handle);
                }
            }
            RunEvent::WindowEvent { .. } => {}
            _ => {}
        }
    });
}
