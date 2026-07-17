use crate::{app_log, models::AppState, settings};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Position};

const STARTUP_LITE_ARGS: [&str; 4] = ["--clipanchor-startup", "--startup", "--background", "--lite"];
const MIN_VISIBLE_EDGE: i32 = 96;

pub fn should_start_in_lite_mode() -> bool {
    std::env::args().any(|arg| STARTUP_LITE_ARGS.iter().any(|candidate| arg == *candidate))
}

fn position_is_visible<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, x: i32, y: i32, width: u32, height: u32) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    let window_right = x.saturating_add(width as i32);
    let window_bottom = y.saturating_add(height as i32);
    monitors.iter().any(|monitor| {
        let origin = monitor.position();
        let size = monitor.size();
        let monitor_right = origin.x.saturating_add(size.width as i32);
        let monitor_bottom = origin.y.saturating_add(size.height as i32);
        let visible_width = window_right.min(monitor_right) - x.max(origin.x);
        let visible_height = window_bottom.min(monitor_bottom) - y.max(origin.y);
        visible_width >= MIN_VISIBLE_EDGE && visible_height >= MIN_VISIBLE_EDGE
    })
}

pub fn restore_main_window_position(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or_else(|| "Main window not found".to_string())?;
    let (saved_x, saved_y) = app
        .try_state::<AppState>()
        .and_then(|state| state.settings.lock().ok().map(|value| (value.main_window_x, value.main_window_y)))
        .unwrap_or((None, None));
    let size = window.outer_size().map_err(|error| error.to_string())?;

    if let (Some(x), Some(y)) = (saved_x, saved_y) {
        if position_is_visible(&window, x, y, size.width, size.height) {
            // 中文：仅恢复仍位于现有显示器可见范围内的位置，是为了避免用户拔掉副屏后主窗口永久落在屏幕外。
            // English: Restore a saved position only when it remains visible on a current monitor, preventing the main window from becoming unreachable after a secondary display is disconnected.
            window.set_position(Position::Physical(PhysicalPosition::new(x, y))).map_err(|error| error.to_string())?;
            return Ok(());
        }
    }

    // 中文：首次启动或旧位置失效时使用系统工作区居中，是为了提供稳定、可预期的默认打开位置。
    // English: Center within the available workspace on first launch or when a saved position is invalid, providing a stable and predictable default opening location.
    window.center().map_err(|error| error.to_string())
}

pub fn save_main_window_position(app: &AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    if window.is_maximized().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
        // 中文：最大化或最小化坐标不是用户的正常窗口布局，忽略它们可避免下次恢复到系统生成的临时位置。
        // English: Maximized and minimized coordinates are not the user's normal layout, so ignoring them avoids restoring system-generated temporary positions next time.
        return Ok(());
    }
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let mut guard = state.settings.lock().map_err(|error| error.to_string())?;
    if guard.main_window_x == Some(position.x) && guard.main_window_y == Some(position.y) {
        return Ok(());
    }
    guard.main_window_x = Some(position.x);
    guard.main_window_y = Some(position.y);
    settings::save(&state.paths, &guard)?;
    app_log::info(&state.paths, "window", format!("saved main window position {},{}", position.x, position.y));
    Ok(())
}

pub fn activate_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or_else(|| "Main window not found".to_string())?;
    let was_visible = window.is_visible().unwrap_or(false);
    // 中文：仅在窗口由隐藏状态恢复时读取记忆位置，是为了避免第二次启动激活一个已显示窗口时把用户刚移动的位置拉回旧坐标。
    // English: Restore remembered coordinates only when reopening from hidden state so activating an already visible window cannot pull a freshly moved window back to stale coordinates.
    if !was_visible {
        if let Err(error) = restore_main_window_position(app) {
            if let Some(state) = app.try_state::<AppState>() {
                app_log::warn(&state.paths, "window", format!("main window position restore skipped: {}", error));
            }
        }
    }
    // 主界面恢复时先还原普通应用策略，是为了让 macOS Dock 与 Cmd-Tab 再次显示主程序入口。
    // Restoring the regular application policy before showing the main UI makes the macOS Dock and Cmd-Tab show the main app entry again.
    crate::macos_native::show_dock_icon(app);
    window.show().map_err(|error| error.to_string())?;
    let _ = window.set_shadow(false);
    let _ = window.unminimize();
    #[cfg(target_os = "windows")]
    native_activate_window(&window);
    let _ = window.set_focus();
    let _ = app.emit("clipanchor-main-window-activated", ());
    Ok(())
}

pub fn hide_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        // 中文：只有已显示过的窗口才保存位置，避免开机轻量模式把尚未居中的隐藏初始坐标误记为用户位置。
        // English: Save coordinates only for a window that has actually been shown, preventing startup Lite mode from recording an uncentered hidden initial position as the user's location.
        if window.is_visible().unwrap_or(false) {
            let _ = save_main_window_position(app);
        }
        let _ = window.set_shadow(false);
        window.hide().map_err(|error| error.to_string())?;
        crate::macos_native::hide_dock_icon(app);
        let _ = app.emit("clipanchor-main-window-hidden", ());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn native_activate_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;
    use windows_sys::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            BringWindowToTop, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_TOP, SWP_NOMOVE,
            SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOW,
        },
    };

    let Ok(handle) = window.window_handle() else { return; };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else { return; };
    let hwnd = win32.hwnd.get() as *mut c_void as HWND;
    if hwnd.is_null() { return; }

    // Windows 对后台程序抢前台有额外限制；组合使用 ShowWindow/SetForegroundWindow 可以修复托盘菜单或快捷键只显示不置前的问题。
    // Windows restricts foreground activation for background apps; combining ShowWindow and SetForegroundWindow fixes tray or shortcut wakes that only show without surfacing.
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(hwnd, SW_RESTORE);
        BringWindowToTop(hwnd);
        SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        SetForegroundWindow(hwnd);
    }
}
