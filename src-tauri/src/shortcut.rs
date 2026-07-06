use crate::{app_log, models::ShortcutSettings, settings};
use std::{collections::HashSet, sync::OnceLock, thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ShortcutAction {
    TogglePinService,
    ToggleHistoryService,
    ToggleMainWindow,
    EnterLightMode,
    ToggleThemeMode,
}

#[cfg(target_os = "windows")]
static WINDOWS_FALLBACK_STARTED: OnceLock<()> = OnceLock::new();

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new().with_handler(|app, shortcut, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }
        let Some(state) = app.try_state::<crate::models::AppState>() else { return; };
        let settings_snapshot = match state.settings.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        let Some(action) = action_for_pressed_shortcut(&settings_snapshot.shortcuts, shortcut) else { return; };
        let _ = handle_action(app, &state, action);
    }).build()
}

pub fn sync_shortcuts(app: &AppHandle, shortcuts: &ShortcutSettings) -> Result<(), String> {
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    let configured = [
        &shortcuts.toggle_pin_service,
        &shortcuts.toggle_history_service,
        &shortcuts.toggle_main_window,
        &shortcuts.enter_light_mode,
        &shortcuts.toggle_theme_mode,
    ];

    let mut errors = Vec::new();
    for shortcut in configured {
        match parse_shortcut(shortcut) {
            Ok(parsed) => {
                if let Err(error) = manager.register(parsed) {
                    errors.push(format!("{}: {}", shortcut, error));
                }
            }
            Err(error) => errors.push(format!("{}: {}", shortcut, error)),
        }
    }

    if errors.is_empty() {
        if let Some(state) = app.try_state::<crate::models::AppState>() { app_log::info(&state.paths, "shortcut", "global shortcuts registered"); }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let _ = manager.unregister_all();
        ensure_windows_keyboard_fallback(app.clone());
        let message = format!("native global shortcut registration failed ({}). Windows keyboard fallback enabled.", errors.join("; "));
        if let Some(state) = app.try_state::<crate::models::AppState>() {
            // 快捷键降级属于可恢复运行状态，记录到日志即可，避免污染正常命令行输出。
            // Shortcut fallback is a recoverable runtime state, so logging it is enough and keeps normal command output clean.
            app_log::warn(&state.paths, "shortcut", &message);
        }
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(errors.join("; "))
    }
}

fn action_for_pressed_shortcut(shortcuts: &ShortcutSettings, pressed: &Shortcut) -> Option<ShortcutAction> {
    if shortcut_matches(&shortcuts.toggle_pin_service, pressed) {
        Some(ShortcutAction::TogglePinService)
    } else if shortcut_matches(&shortcuts.toggle_history_service, pressed) {
        Some(ShortcutAction::ToggleHistoryService)
    } else if shortcut_matches(&shortcuts.toggle_main_window, pressed) {
        Some(ShortcutAction::ToggleMainWindow)
    } else if shortcut_matches(&shortcuts.enter_light_mode, pressed) {
        Some(ShortcutAction::EnterLightMode)
    } else if shortcut_matches(&shortcuts.toggle_theme_mode, pressed) {
        Some(ShortcutAction::ToggleThemeMode)
    } else {
        None
    }
}

fn handle_action(app: &AppHandle, state: &crate::models::AppState, action: ShortcutAction) -> Result<(), String> {
    app_log::info(&state.paths, "shortcut", format!("shortcut action: {:?}", action));
    match action {
        ShortcutAction::ToggleMainWindow => return toggle_main_window(app),
        ShortcutAction::EnterLightMode => return hide_main_window(app),
        ShortcutAction::ToggleThemeMode => {}
        ShortcutAction::TogglePinService | ShortcutAction::ToggleHistoryService => {}
    }

    let mut settings_guard = state.settings.lock().map_err(|error| error.to_string())?;
    match action {
        ShortcutAction::TogglePinService => settings_guard.pin_service_enabled = !settings_guard.pin_service_enabled,
        ShortcutAction::ToggleHistoryService => settings_guard.history_service_enabled = !settings_guard.history_service_enabled,
        ShortcutAction::ToggleThemeMode => {
            // 快捷键只在深色和浅色之间切换，是为了让结果明确可见，并避免“跟随系统”在不同系统设置下产生不确定反馈。
            // The shortcut toggles only between dark and light so the result is immediately visible and avoids ambiguous system-theme feedback.
            settings_guard.theme = if settings_guard.theme == "light" { "dark".into() } else { "light".into() };
        }
        ShortcutAction::ToggleMainWindow | ShortcutAction::EnterLightMode => {}
    }

    let updated = settings_guard.clone();
    settings::save(&state.paths, &updated)?;
    drop(settings_guard);
    // 快捷键修改的是后台真实状态，必须立即广播给前端，否则侧栏状态会滞后并让用户误以为快捷键没有生效。
    // Shortcuts mutate backend state directly, so the update is broadcast immediately to prevent the sidebar from looking stale.
    let _ = app.emit("clipanchor-settings-changed", updated);
    Ok(())
}

fn toggle_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            return crate::window_control::hide_main_window(app);
        }
    }
    // 快捷键唤醒与托盘菜单共享同一个强激活入口，是为了避免隐藏窗口只能 show 不能置前的 Windows 边缘情况。
    // Shortcut wake shares the same strong activation path as the tray menu to avoid Windows edge cases where a hidden window shows but does not come forward.
    crate::window_control::activate_main_window(app)
}

fn hide_main_window(app: &AppHandle) -> Result<(), String> {
    crate::window_control::hide_main_window(app)
}

fn shortcut_matches(configured: &str, pressed: &Shortcut) -> bool {
    parse_shortcut(configured).map(|shortcut| &shortcut == pressed).unwrap_or(false)
}

fn parse_shortcut(value: &str) -> Result<Shortcut, String> {
    let trimmed = value.trim();
    let mut candidates = vec![trimmed.to_string()];
    if trimmed.contains("Ctrl") {
        candidates.push(trimmed.replace("Ctrl", "Control"));
        candidates.push(trimmed.replace("Ctrl", "CommandOrControl"));
    }
    if trimmed.contains("Control") {
        candidates.push(trimmed.replace("Control", "Ctrl"));
    }
    for candidate in candidates {
        if let Ok(shortcut) = Shortcut::try_from(candidate.as_str()) {
            return Ok(shortcut);
        }
    }
    Err("invalid shortcut format".into())
}

#[cfg(target_os = "windows")]
fn ensure_windows_keyboard_fallback(app: AppHandle) {
    if WINDOWS_FALLBACK_STARTED.set(()).is_err() {
        return;
    }
    thread::spawn(move || {
        let mut active = HashSet::<ShortcutAction>::new();
        loop {
            thread::sleep(Duration::from_millis(70));
            let Some(state) = app.try_state::<crate::models::AppState>() else { continue; };
            let settings_snapshot = match state.settings.lock() {
                Ok(guard) => guard.clone(),
                Err(_) => continue,
            };
            let candidates = [
                (ShortcutAction::TogglePinService, settings_snapshot.shortcuts.toggle_pin_service.as_str()),
                (ShortcutAction::ToggleHistoryService, settings_snapshot.shortcuts.toggle_history_service.as_str()),
                (ShortcutAction::ToggleMainWindow, settings_snapshot.shortcuts.toggle_main_window.as_str()),
                (ShortcutAction::EnterLightMode, settings_snapshot.shortcuts.enter_light_mode.as_str()),
                (ShortcutAction::ToggleThemeMode, settings_snapshot.shortcuts.toggle_theme_mode.as_str()),
            ];
            let mut currently_down = HashSet::new();
            for (action, shortcut) in candidates {
                if windows_shortcut_is_down(shortcut) {
                    currently_down.insert(action);
                    if !active.contains(&action) {
                        let _ = handle_action(&app, state.inner(), action);
                    }
                }
            }
            active = currently_down;
        }
    });
}

#[cfg(target_os = "windows")]
fn windows_shortcut_is_down(value: &str) -> bool {
    const VK_CONTROL: i32 = 0x11;
    const VK_SHIFT: i32 = 0x10;
    const VK_MENU: i32 = 0x12;
    const KEY_DOWN_MASK: i16 = i16::MIN;
    let normalized = value.replace(' ', "").replace("CommandOrControl", "Ctrl");
    let mut key_code: Option<i32> = None;
    let mut needs_ctrl = false;
    let mut needs_shift = false;
    let mut needs_alt = false;

    for part in normalized.split('+') {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => needs_ctrl = true,
            "shift" => needs_shift = true,
            "alt" | "option" => needs_alt = true,
            other => key_code = key_code_from_name(other),
        }
    }

    let Some(key) = key_code else { return false; };
    unsafe {
        if needs_ctrl && (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(VK_CONTROL) & KEY_DOWN_MASK) == 0 {
            return false;
        }
        if needs_shift && (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(VK_SHIFT) & KEY_DOWN_MASK) == 0 {
            return false;
        }
        if needs_alt && (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(VK_MENU) & KEY_DOWN_MASK) == 0 {
            return false;
        }
        (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(key) & KEY_DOWN_MASK) != 0
    }
}

#[cfg(target_os = "windows")]
fn key_code_from_name(name: &str) -> Option<i32> {
    let upper = name.to_ascii_uppercase();
    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];
        if byte.is_ascii_alphanumeric() {
            return Some(byte as i32);
        }
    }
    match upper.as_str() {
        "ESC" | "ESCAPE" => Some(0x1B),
        "SPACE" => Some(0x20),
        "TAB" => Some(0x09),
        "ENTER" | "RETURN" => Some(0x0D),
        "BACKSPACE" => Some(0x08),
        "DELETE" | "DEL" => Some(0x2E),
        "INSERT" | "INS" => Some(0x2D),
        "HOME" => Some(0x24),
        "END" => Some(0x23),
        "PAGEUP" => Some(0x21),
        "PAGEDOWN" => Some(0x22),
        "UP" | "ARROWUP" => Some(0x26),
        "DOWN" | "ARROWDOWN" => Some(0x28),
        "LEFT" | "ARROWLEFT" => Some(0x25),
        "RIGHT" | "ARROWRIGHT" => Some(0x27),
        _ if upper.starts_with('F') => upper[1..].parse::<i32>().ok().filter(|n| (1..=24).contains(n)).map(|n| 0x70 + n - 1),
        _ => None,
    }
}
