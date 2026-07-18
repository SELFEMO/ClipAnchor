#[cfg(target_os = "macos")]
use tauri::{AppHandle, menu::{Menu, MenuItem, PredefinedMenuItem, Submenu}};

#[cfg(target_os = "macos")]
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let hide = MenuItem::with_id(app, "hide-main-window", "Close Window", true, Some("CmdOrCtrl+W"))?;
    let quit = MenuItem::with_id(app, "quit-app", "Quit ClipAnchor", true, Some("CmdOrCtrl+Q"))?;
    let app_menu = Submenu::with_items(app, "ClipAnchor", true, &[&hide, &quit])?;

    // macOS 的 WebView 文本输入依赖原生 Edit 菜单提供标准 responder actions；如果应用菜单只保留关闭和退出，⌘V 可能不会送达密码输入框。
    // macOS WebView text fields rely on native Edit-menu responder actions; when the app menu only contains Close and Quit, Command+V may never reach password inputs.
    let undo = PredefinedMenuItem::undo(app, None)?;
    let redo = PredefinedMenuItem::redo(app, None)?;
    let separator_a = PredefinedMenuItem::separator(app)?;
    let cut = PredefinedMenuItem::cut(app, None)?;
    let copy = PredefinedMenuItem::copy(app, None)?;
    let paste = PredefinedMenuItem::paste(app, None)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let select_all = PredefinedMenuItem::select_all(app, None)?;
    let edit_menu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[&undo, &redo, &separator_a, &cut, &copy, &paste, &separator_b, &select_all],
    )?;
    let menu = Menu::with_items(app, &[&app_menu, &edit_menu])?;

    // Command+W 需要注册到 macOS 原生菜单加速键；WebView keydown 在系统菜单处理前可能收不到，所以仅靠前端监听不可靠。
    // Command+W must be registered as a native macOS menu accelerator; WebView keydown can miss it because the system menu handles the shortcut first.
    app.set_menu(menu)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn install(_app: &tauri::AppHandle) -> tauri::Result<()> {
    Ok(())
}
