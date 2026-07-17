use tauri::{AppHandle, Emitter, Manager};

const STARTUP_LITE_ARGS: [&str; 4] = ["--clipanchor-startup", "--startup", "--background", "--lite"];

pub fn should_start_in_lite_mode() -> bool {
    std::env::args().any(|arg| STARTUP_LITE_ARGS.iter().any(|candidate| arg == *candidate))
}

pub fn activate_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or_else(|| "Main window not found".to_string())?;
    // 主界面恢复时先还原普通应用策略，是为了让 macOS Dock 与 Cmd-Tab 再次显示主程序入口。
    // Restoring the regular application policy before showing the main UI makes the macOS Dock and Cmd-Tab show the main app entry again.
    crate::macos_native::show_dock_icon(app);
    // 先恢复窗口再请求焦点，是为了覆盖“隐藏到托盘”“最小化”和“自启动轻量模式”三种不同后台状态。
    // Restoring before focusing covers the three background states: hidden-to-tray, minimized, and startup Lite mode.
    window.show().map_err(|error| error.to_string())?;
    let _ = window.set_shadow(false);
    // 主窗口不再套用 Windows Region，是因为无边框可缩放窗口存在隐藏边框，Region 会把该边框裁成截图中的直线残边。
    // The main window no longer uses a Windows Region because borderless resizable windows have hidden borders that Region clipping turns into visible straight artifacts.
    let _ = window.unminimize();
    let _ = window.set_shadow(false);
    #[cfg(target_os = "windows")]
    native_activate_window(&window);
    let _ = window.set_focus();
    // 激活完成后广播事件，后续若需要前端刷新状态栏或动效，可以复用同一个可靠唤醒入口。
    // Emitting after activation gives the frontend a single reliable wake signal for future status refreshes or animations.
    let _ = app.emit("clipanchor-main-window-activated", ());
    Ok(())
}

pub fn hide_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        // 统一隐藏入口，是为了让快捷键、托盘和窗口关闭都进入相同的后台轻量状态。
        // A single hide path keeps shortcuts, tray actions, and window closing in the same background Lite state.
        let _ = window.set_shadow(false);
        // 隐藏前只维持透明无阴影状态，是为了避免托盘恢复时重新出现系统矩形边框缓存。
        // Before hiding we only preserve the transparent no-shadow state so tray restore does not bring back cached rectangular system borders.
        window.hide().map_err(|error| error.to_string())?;
        // 主窗口隐藏后立即移除 Dock 图标，是为了让轻量模式真正表现为菜单栏/托盘后台工具而不是仍占 Dock。
        // Removing the Dock icon right after hiding the main window makes Lite mode behave like a tray/menu-bar utility instead of occupying the Dock.
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

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = win32.hwnd.get() as *mut c_void as HWND;
    if hwnd.is_null() {
        return;
    }

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
