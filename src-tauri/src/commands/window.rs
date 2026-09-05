use crate::app_log;
use crate::models::AppState;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{IsZoomed, ShowWindow, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE};

#[tauri::command]
pub fn minimize_window(app: AppHandle) -> Result<(), String> {
    // Windows 上优先走原生 ShowWindow，是因为部分 WebView2 无边框窗口会让 Tauri 高层 minimize 调用返回成功但界面不变化。
    // On Windows we prefer native ShowWindow because some borderless WebView2 windows make Tauri's high-level minimize report success without changing the UI.
    #[cfg(target_os = "windows")]
    {
        if native_minimize_main_window(&app) {
            return Ok(());
        }
    }
    app.get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?
        .minimize()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn toggle_maximize_window(app: AppHandle) -> Result<(), String> {
    // Windows 上优先使用原生最大化/还原，是为了绕开自绘标题栏和 WebView 焦点导致的最大化按钮失效。
    // On Windows native maximize/restore bypasses custom-titlebar and WebView focus issues that can break the maximize button.
    #[cfg(target_os = "windows")]
    {
        if native_toggle_maximize_main_window(&app) {
            return Ok(());
        }
    }
    let window = app.get_webview_window("main").ok_or_else(|| "Main window not found".to_string())?;
    if window.is_maximized().map_err(|error| error.to_string())? {
        window.unmaximize().map_err(|error| error.to_string())
    } else {
        window.maximize().map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub fn close_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(state) = app.try_state::<AppState>() {
        app_log::info(&state.paths, "window", "main window close button requested Lite mode hide");
    }
    // 关闭按钮只隐藏主界面而不销毁 WebView，是为了保证长时间轻量模式后仍能从托盘或快捷键稳定唤醒同一个主界面。
    // The close button only hides the main UI instead of destroying the WebView so tray and shortcut wake-ups remain reliable after long Lite-mode sessions.
    crate::window_control::hide_main_window(&app)
}

#[tauri::command]
pub fn quit_app(app: AppHandle) -> Result<(), String> {
    // 退出程序交给 Tauri 正常清理 WebView2 窗口，是为了避免强制 process::exit 触发 Chrome_WidgetWin_0 注销警告。
    // Quitting through Tauri lets WebView2 windows clean up normally, avoiding the Chrome_WidgetWin_0 unregister warning caused by forced process::exit.
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "windows")]
fn native_minimize_main_window(app: &AppHandle) -> bool {
    let Some(hwnd) = crate::window_control::main_window_hwnd(app) else {
        return false;
    };
    unsafe { ShowWindow(hwnd, SW_MINIMIZE); }
    true
}

#[cfg(target_os = "windows")]
fn native_toggle_maximize_main_window(app: &AppHandle) -> bool {
    let Some(hwnd) = crate::window_control::main_window_hwnd(app) else {
        return false;
    };
    unsafe {
        if IsZoomed(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }
    true
}
