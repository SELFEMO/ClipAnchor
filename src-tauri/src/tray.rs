use crate::app_log;
use tauri::{AppHandle, Emitter, Manager, menu::{Menu, MenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent}};

#[cfg(target_os = "windows")]
use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

pub fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_tray_menu(app)?;
    let mut builder = TrayIconBuilder::with_id("clipanchor-tray")
        .tooltip("ClipAnchor")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(state) = app.try_state::<crate::models::AppState>() { app_log::info(&state.paths, "tray", "show menu clicked"); }
                // 托盘菜单统一走强激活入口，是为了修复 Windows 上 show 成功但窗口仍停在后台的问题。
                // The tray menu uses the strong activation path to fix Windows cases where show succeeds but the window stays behind other apps.
                let _ = crate::window_control::activate_main_window(app);
            }
            "privacy" => {
                if let Some(state) = app.try_state::<crate::models::AppState>() {
                    if let Ok(mut settings) = state.settings.lock() {
                        settings.privacy_filter_mode = if settings.privacy_filter_mode == "off" { "light".into() } else { "off".into() };
                        settings.privacy_mode = settings.privacy_filter_mode != "off";
                        let updated = settings.clone();
                        app_log::info(&state.paths, "tray", format!("privacy menu toggled to {}", updated.privacy_filter_mode));
                        let _ = crate::settings::save(&state.paths, &updated);
                        drop(settings);
                        // 托盘切换隐私状态后也广播设置，是为了让主窗口、弹窗和托盘菜单文字保持同一份语言与状态。
                        // Broadcasting after tray privacy changes keeps the main window, popups, and tray labels synchronized in language and state.
                        let _ = app.emit("clipanchor-settings-changed", updated);
                        let _ = refresh_tray(app);
                    }
                }
            }
            "quit" => {
                if let Some(state) = app.try_state::<crate::models::AppState>() { app_log::warn(&state.paths, "tray", "quit menu clicked"); }
                app.exit(0);
            },
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button, button_state: MouseButtonState::Up, .. } = event {
                if matches!(button, MouseButton::Left) {
                    if let Some(state) = tray.app_handle().try_state::<crate::models::AppState>() { app_log::info(&state.paths, "tray", "left click activated main window"); }
                    // 托盘左键/双击都可能由系统合并为点击事件，因此这里也使用同一个强激活入口。
                    // Left click and double click can be coalesced by the OS, so this uses the same strong activation path.
                    let _ = crate::window_control::activate_main_window(tray.app_handle());
                }
                // 不能在右键弹出菜单的同一帧重建菜单，否则 Windows 会先打开旧菜单再因句柄替换而立即关闭。
                // Do not rebuild the menu on the same frame as a right-click, because Windows can open the old menu and then close it when its handle is replaced.
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    if let Some(state) = app.try_state::<crate::models::AppState>() { app_log::info(&state.paths, "tray", "tray installed"); }
    Ok(())
}

pub fn refresh_tray(app: &AppHandle) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id("clipanchor-tray") {
        let menu = build_tray_menu(app)?;
        // 直接替换菜单而不是先传入 None，是为了兼容 Tauri v2 对 set_menu 泛型参数的推断，同时仍让托盘使用最新语言文本。
        // Replacing the menu directly instead of passing None keeps Tauri v2 generic inference valid while still applying the latest localized labels.
        tray.set_menu(Some(menu))?;
        let _ = tray.set_tooltip(Some("ClipAnchor"));
    }
    Ok(())
}

fn build_tray_menu<R: tauri::Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let labels = tray_labels(app);
    let show = MenuItem::with_id(app, "show", labels.show.as_str(), true, None::<&str>)?;
    let privacy = MenuItem::with_id(app, "privacy", labels.privacy.as_str(), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit.as_str(), true, None::<&str>)?;
    Menu::with_items(app, &[&show, &privacy, &quit])
}

struct TrayLabels {
    show: String,
    privacy: String,
    quit: String,
}

fn tray_labels<R: tauri::Runtime>(app: &AppHandle<R>) -> TrayLabels {
    let settings = app.try_state::<crate::models::AppState>()
        .and_then(|state| state.settings.lock().ok().map(|settings| settings.clone()));
    let is_chinese = settings.as_ref().map(|settings| locale_is_chinese(&settings.locale)).unwrap_or_else(system_locale_is_chinese);
    let privacy_enabled = settings.as_ref().map(|settings| settings.privacy_filter_mode != "off").unwrap_or(true);
    match (is_chinese, privacy_enabled) {
        (true, true) => TrayLabels { show: "显示 ClipAnchor".into(), privacy: "关闭隐私过滤".into(), quit: "退出".into() },
        (true, false) => TrayLabels { show: "显示 ClipAnchor".into(), privacy: "开启隐私过滤".into(), quit: "退出".into() },
        (false, true) => TrayLabels { show: "Show ClipAnchor".into(), privacy: "Disable privacy filter".into(), quit: "Quit".into() },
        (false, false) => TrayLabels { show: "Show ClipAnchor".into(), privacy: "Enable privacy filter".into(), quit: "Quit".into() },
    }
}

fn locale_is_chinese(locale: &str) -> bool {
    if locale == "zh" || locale == "zh-CN" {
        return true;
    }
    if locale == "en" {
        return false;
    }
    // 自动语言要和系统 UI 语言一致，否则主界面是中文但托盘仍可能显示英文。
    // Auto language must follow the OS UI language, otherwise the main UI can be Chinese while the tray stays English.
    system_locale_is_chinese()
}

fn system_locale_is_chinese() -> bool {
    #[cfg(target_os = "windows")]
    {
        let lang_id = unsafe { GetUserDefaultUILanguage() };
        if (lang_id & 0x03ff) == 0x0004 {
            return true;
        }
    }
    std::env::var("LANG")
        .or_else(|_| std::env::var("LANGUAGE"))
        .or_else(|_| std::env::var("LC_ALL"))
        .map(|value| value.to_lowercase().contains("zh"))
        .unwrap_or(false)
}
