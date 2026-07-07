#[cfg(target_os = "macos")]
use tauri::{AppHandle, menu::{Menu, MenuItem, Submenu}};

#[cfg(target_os = "macos")]
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let hide = MenuItem::with_id(app, "hide-main-window", "Close Window", true, Some("CmdOrCtrl+W"))?;
    let quit = MenuItem::with_id(app, "quit-app", "Quit ClipAnchor", true, Some("CmdOrCtrl+Q"))?;
    let app_menu = Submenu::with_items(app, "ClipAnchor", true, &[&hide, &quit])?;
    let menu = Menu::with_items(app, &[&app_menu])?;

    // Command+W 需要注册到 macOS 原生菜单加速键；WebView keydown 在系统菜单处理前可能收不到，所以仅靠前端监听不可靠。
    // Command+W must be registered as a native macOS menu accelerator; WebView keydown can miss it because the system menu handles the shortcut first.
    app.set_menu(menu)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn install(_app: &tauri::AppHandle) -> tauri::Result<()> {
    Ok(())
}
