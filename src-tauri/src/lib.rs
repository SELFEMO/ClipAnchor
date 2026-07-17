#![allow(unexpected_cfgs)]
// objc 运行时桥接宏会展开旧的 cargo-clippy cfg；在 crate 根部收敛该依赖噪声，是为了让 macOS release 构建输出只保留真正需要处理的问题。
// The Objective-C runtime bridge macros expand an old cargo-clippy cfg; containing that dependency noise at the crate root keeps macOS release output focused on actionable issues.

mod app_log;
mod app_menu;
mod autostart;
mod clipboard_service;
mod commands;
mod database;
mod models;
mod macos_native;
mod paths;
mod popup;
mod settings;
mod shortcut;
mod tray;
mod update_service;
mod window_control;
mod window_shape;

use commands::*;
use models::AppState;
use tauri::{Manager, RunEvent, WindowEvent};
use std::panic;

fn log_startup_issue(state: &AppState, message: &str) {
    // 启动诊断写入文件而不是标准错误，是为了让普通开发/运行命令保持干净，同时仍能通过日志定位启动问题。
    // Startup diagnostics are written to the log file instead of stderr so normal dev/runtime commands stay clean while issues remain traceable.
    app_log::info(&state.paths, "startup", message);
}

fn reconcile_autostart_setting(state: &AppState) {
    let configured = match state.settings.lock() {
        Ok(settings) => settings.auto_start,
        Err(error) => {
            log_startup_issue(
                state,
                &format!("Autostart reconciliation skipped because settings are locked: {}", error),
            );
            return;
        }
    };

    match autostart::reconcile(configured, &state.paths.root) {
        Ok(actual) if actual != configured => {
            let mut settings_guard = match state.settings.lock() {
                Ok(settings) => settings,
                Err(error) => {
                    log_startup_issue(
                        state,
                        &format!("Autostart state changed externally but settings could not be updated: {}", error),
                    );
                    return;
                }
            };
            // 系统启动项可能在应用外被修改；把核验后的实际状态写回设置，是为了避免界面显示过期值并在下次操作时覆盖用户选择。
            // OS startup entries can change outside the app; writing the verified state back prevents stale UI and avoids overwriting the user's choice later.
            settings_guard.auto_start = actual;
            if let Err(error) = settings::save(&state.paths, &settings_guard) {
                log_startup_issue(
                    state,
                    &format!("Autostart state was detected but could not be persisted: {}", error),
                );
            } else {
                log_startup_issue(
                    state,
                    &format!("Autostart setting synchronized with operating system state: {}", actual),
                );
            }
        }
        Ok(_) => log_startup_issue(state, "Autostart setting verified"),
        Err(error) => log_startup_issue(
            state,
            &format!("Autostart reconciliation skipped: {}", error),
        ),
    }
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

    if let Err(error) = paths::configure_webview_storage(&state.paths) {
        // WebView 用户数据目录必须在 Builder 创建任何窗口前确定，否则 WebView2 已锁定默认 AppData 路径后再修改将不会生效。
        // The WebView user-data directory must be fixed before Builder creates any window, because changing it after WebView2 locks the default AppData path has no effect.
        eprintln!("ClipAnchor failed to configure WebView storage: {}", error);
        return;
    }

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
        .on_menu_event(|app, event| match event.id.as_ref() {
            "hide-main-window" => {
                if let Some(state) = app.try_state::<AppState>() {
                    log_startup_issue(state.inner(), "macOS Command+W menu accelerator requested Lite mode hide");
                }
                let _ = crate::window_control::hide_main_window(app);
            }
            "quit-app" => {
                if let Some(state) = app.try_state::<AppState>() {
                    log_startup_issue(state.inner(), "macOS Quit menu requested application exit");
                }
                let _ = crate::window_control::save_main_window_position(app);
                app.exit(0);
            }
            _ => {}
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            list_language_packs,
            save_language_pack,
            delete_language_pack,
            log_language_pack_event,
            translate_ui_text,
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
            install_downloaded_update,
            dismiss_update_prompt,
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
            #[cfg(target_os = "windows")]
            log_startup_issue(
                &state,
                &format!(
                    "WebView storage configured at {}",
                    paths::webview_storage_path(&state.paths).display()
                ),
            );
            reconcile_autostart_setting(&state);
            // 启动期的托盘、快捷键和剪贴板监听都降级为可恢复错误，避免某个系统能力失败就让整个程序退出。
            // Tray, shortcut, and clipboard startup failures are treated as recoverable so one OS capability cannot close the whole app.
            if let Err(error) = app_menu::install(app.handle()) {
                log_startup_issue(&state, &format!("Application menu initialization skipped: {}", error));
            }
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
            // 后台看门狗独立于主窗口运行，是为了在 Windows 长时间隐藏到托盘后自动恢复剪贴板监听线程。
            // The background watchdog runs independently from the main window so Windows tray-idle sessions can recover the clipboard monitor automatically.
            clipboard_service::start_monitor_watchdog(handle.clone(), state.clone());
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
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.set_shadow(false);
                // 主窗口使用透明 WebView 加单层 CSS 圆角，是为了避开 Windows 无边框窗口的隐藏缩放边框被 Region 裁剪后露出的直线边缘。
                // The main window uses a transparent WebView plus one CSS radius to avoid exposing straight hidden resize borders after Windows Region clipping.
            }
            let lite_startup = window_control::should_start_in_lite_mode();
            let auto_update_enabled = state.settings.lock().map(|settings| settings.auto_update_enabled).unwrap_or(true);
            // 启动检查是否运行由设置控制，是为了让“自动更新”开关真正阻止后台网络请求。
            // Startup checking is controlled by Settings so the Auto Update switch truly prevents background network requests.
            let _ = update_service::startup_background_check(&handle, &state.paths, lite_startup, auto_update_enabled);
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
                if code.is_none() {
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        log_startup_issue(state.inner(), "Main window close requested; keeping background services alive");
                    }
                    // 主窗口关闭只隐藏到托盘且不停止监听，是为了避免用户无操作或关闭窗口后后台服务被误杀。
                    // Closing the main window only hides it to tray and keeps monitoring alive so idle or close-to-tray sessions do not kill background services.
                    api.prevent_exit();
                    let _ = window_control::hide_main_window(app_handle);
                } else {
                    let _ = window_control::save_main_window_position(app_handle);
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        log_startup_issue(state.inner(), &format!("Application exit requested with code {:?}", code));
                        clipboard_service::stop_monitor(state.inner());
                    }
                }
            }
            RunEvent::WindowEvent { label, event: WindowEvent::CloseRequested { api, .. }, .. } if label == "main" => {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    log_startup_issue(state.inner(), "Main window close event intercepted; hiding instead of destroying WebView");
                }
                // 拦截系统关闭事件并隐藏窗口，是为了避免主窗口 WebView 被销毁后，托盘和快捷键只能保留后台服务却无法重新唤起界面。
                // Intercepting the native close event and hiding the window prevents the main WebView from being destroyed while background services continue running.
                api.prevent_close();
                let _ = window_control::hide_main_window(app_handle);
            }
            RunEvent::WindowEvent { label, event: WindowEvent::Resized(_), .. } if label == "main" => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.set_shadow(false);
                    // 缩放时只确认关闭原生阴影，是为了让主界面外轮廓始终由前端单一圆角负责，而不是再次叠加系统边框。
                    // During resize we only keep the native shadow disabled so the main outline stays owned by one frontend radius instead of stacking system borders again.
                }
            }
            RunEvent::WindowEvent { label, event: WindowEvent::ScaleFactorChanged { .. }, .. } if label == "main" => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.set_shadow(false);
                    // DPI 变化不再重放 Region 裁剪，是为了避免 Windows 在不同缩放比下重新暴露隐藏的非客户区边线。
                    // DPI changes no longer replay Region clipping, which avoids Windows exposing hidden non-client edge lines at different scale factors.
                }
            }
            RunEvent::WindowEvent { .. } => {}
            _ => {}
        }
    });
}
