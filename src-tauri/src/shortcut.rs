#![cfg_attr(target_os = "linux", allow(dead_code))]
use crate::{
    app_log,
    models::{ShortcutConflictPayload, ShortcutSettings},
    settings,
};
#[cfg(target_os = "windows")]
use std::{collections::HashSet, sync::OnceLock, thread, time::Duration};
#[cfg(target_os = "linux")]
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub(crate) fn global_shortcuts_supported() -> bool {
    !cfg!(target_os = "linux")
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ShortcutAction {
    TogglePinService,
    ToggleHistoryService,
    ToggleMainWindow,
    EnterLightMode,
    ToggleThemeMode,
}

impl ShortcutAction {
    #[cfg(target_os = "linux")]
    pub(crate) fn cli_id(self) -> &'static str {
        match self {
            Self::TogglePinService => "toggle-pin-service",
            Self::ToggleHistoryService => "toggle-history-service",
            Self::ToggleMainWindow => "toggle-main-window",
            Self::EnterLightMode => "enter-light-mode",
            Self::ToggleThemeMode => "toggle-theme-mode",
        }
    }

    pub(crate) fn keeps_main_window_hidden(self) -> bool {
        !matches!(self, Self::ToggleMainWindow)
    }
}

pub(crate) fn action_from_args<I, S>(args: I) -> Option<ShortcutAction>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if !global_shortcuts_supported() {
        return None;
    }
    args.into_iter().find_map(|argument| {
        argument
            .as_ref()
            .strip_prefix("--clipanchor-shortcut=")
            .and_then(action_from_cli_id)
    })
}

pub(crate) fn handle_external_shortcut_args(
    app: &AppHandle,
    args: &[String],
) -> Result<bool, String> {
    let Some(action) = action_from_args(args.iter().map(String::as_str)) else {
        return Ok(false);
    };
    let state = app
        .try_state::<crate::models::AppState>()
        .ok_or_else(|| "application state is unavailable".to_string())?;
    // 保留命令行快捷键动作作为非 GNOME 桌面和旧启动项的兼容通道；主 GNOME 路径改用本地触发信号后，不再依赖二次启动。
    // Command-line shortcut actions remain as a compatibility path for non-GNOME desktops and legacy entries; the primary GNOME path now uses local trigger signals instead of relaunching the app.
    handle_action(app, state.inner(), action)?;
    Ok(true)
}

pub(crate) fn handle_startup_shortcut(
    app: &AppHandle,
    action: ShortcutAction,
) -> Result<(), String> {
    if matches!(action, ShortcutAction::ToggleMainWindow) {
        // 当快捷键首次启动应用时主窗口尚未完成显示，沿用正常启动激活流程比提前执行“切换”更可靠。
        // When a shortcut starts the app for the first time, the main window is not fully shown yet; using the normal activation path is more reliable than toggling it prematurely.
        return Ok(());
    }
    let state = app
        .try_state::<crate::models::AppState>()
        .ok_or_else(|| "application state is unavailable".to_string())?;
    handle_action(app, state.inner(), action)
}

pub(crate) fn action_from_cli_id(value: &str) -> Option<ShortcutAction> {
    match value {
        "toggle-pin-service" => Some(ShortcutAction::TogglePinService),
        "toggle-history-service" => Some(ShortcutAction::ToggleHistoryService),
        "toggle-main-window" => Some(ShortcutAction::ToggleMainWindow),
        "enter-light-mode" => Some(ShortcutAction::EnterLightMode),
        "toggle-theme-mode" => Some(ShortcutAction::ToggleThemeMode),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
static WINDOWS_FALLBACK_STARTED: OnceLock<()> = OnceLock::new();
#[cfg(target_os = "linux")]
static LINUX_TRIGGER_WATCHER_STARTED: OnceLock<()> = OnceLock::new();
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new().with_handler(|app, shortcut, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }
        let Some(state) = app.try_state::<crate::models::AppState>() else { return; };
        app_log::info(
            &state.paths,
            "shortcut",
            "native shortcut event received",
        );
        let settings_snapshot = match state.settings.lock() {
            Ok(guard) => guard.clone(),
            Err(error) => {
                app_log::warn(
                    &state.paths,
                    "shortcut",
                    format!("native shortcut event ignored because settings are locked: {}", error),
                );
                return;
            }
        };
        let Some(action) = action_for_pressed_shortcut(&settings_snapshot.shortcuts, shortcut) else {
            app_log::warn(
                &state.paths,
                "shortcut",
                "native shortcut event did not match current settings",
            );
            return;
        };
        if let Err(error) = handle_action(app, &state, action) {
            app_log::warn(
                &state.paths,
                "shortcut",
                format!("native shortcut action {:?} failed: {}", action, error),
            );
        }
    }).build()
}

pub fn sync_shortcuts(app: &AppHandle, shortcuts: &ShortcutSettings) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = shortcuts;
        let _ = app.global_shortcut().unregister_all();
        if let Some(state) = app.try_state::<crate::models::AppState>() {
            let mut cleanup_errors = Vec::new();
            if let Err(error) = remove_linux_gnome_fallback_shortcuts(&state.paths) {
                cleanup_errors.push(error);
            }
            remove_legacy_linux_dbus_activation_service(&state.paths);
            let trigger_dir = linux_trigger_dir(&state.paths.data);
            if trigger_dir.exists() {
                if let Err(error) = fs::remove_dir_all(&trigger_dir) {
                    cleanup_errors.push(format!(
                        "cannot remove retired Linux shortcut relay directory {}: {}",
                        trigger_dir.display(),
                        error
                    ));
                }
            }
            if cleanup_errors.is_empty() {
                // Linux 不再初始化全局快捷键，是为了避免不同 Wayland 门户、GNOME 版本和桌面安全策略造成“设置存在但按键不生效”的错误预期。
                // Linux no longer initializes global shortcuts because differing Wayland portals, GNOME versions, and desktop security policies can expose settings without delivering key events.
                app_log::info(
                    &state.paths,
                    "shortcut",
                    "Linux global shortcuts disabled by platform policy; legacy registrations cleaned",
                );
                return Ok(());
            }
            app_log::warn(
                &state.paths,
                "shortcut",
                format!(
                    "Linux global shortcuts remain disabled; legacy cleanup was incomplete: {}",
                    cleanup_errors.join("; ")
                ),
            );
            return Ok(());
        }
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
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
            if let Some(state) = app.try_state::<crate::models::AppState>() {
                app_log::info(
                    &state.paths,
                    "shortcut",
                    "native global shortcut backend accepted all registrations",
                );
            }
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        {
            ensure_windows_keyboard_fallback(app.clone());
            if let Some(state) = app.try_state::<crate::models::AppState>() {
                app_log::warn(
                    &state.paths,
                    "shortcut",
                    format!(
                        "native shortcut registration incomplete; Windows polling fallback enabled: {}",
                        errors.join("; ")
                    ),
                );
            }
            return Ok(());
        }

        Err(errors.join("; "))
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn remove_linux_gnome_fallback_shortcuts(
    log_paths: &crate::paths::DataPaths,
) -> Result<(), String> {
    if !linux_supports_gnome_custom_shortcuts() {
        return Ok(());
    }
    if ensure_linux_gsettings_schema().is_err() {
        return Ok(());
    }

    let existing = linux_read_custom_shortcut_paths().unwrap_or_default();
    let managed = existing
        .iter()
        .filter(|path| linux_is_managed_custom_shortcut_path(path))
        .cloned()
        .collect::<Vec<_>>();
    if managed.is_empty() {
        return Ok(());
    }
    let retained = existing
        .into_iter()
        .filter(|path| !linux_is_managed_custom_shortcut_path(path))
        .collect::<Vec<_>>();
    linux_gsettings_set(
        LINUX_MEDIA_KEYS_SCHEMA,
        "custom-keybindings",
        &gvariant_string_array(&retained),
    )?;

    // 从总列表移除后再重置每个旧子项，可释放组合键并防止后续桌面会话重新加载已经废弃的桥接命令。
    // Resetting each legacy child after removing it from the shared list releases the combinations and prevents later sessions from restoring obsolete bridge commands.
    for path in &managed {
        let schema = format!("{}:{}", LINUX_CUSTOM_KEY_SCHEMA, path);
        let output = Command::new("gsettings")
            .arg("reset-recursively")
            .arg(&schema)
            .output();
        if let Ok(output) = output {
            if !output.status.success() {
                app_log::warn(
                    log_paths,
                    "shortcut",
                    format!(
                        "legacy GNOME shortcut entry could not be reset: path={} detail={}",
                        path,
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                );
            }
        }
    }
    thread::sleep(Duration::from_millis(260));
    app_log::info(
        log_paths,
        "shortcut",
        format!(
            "removed {} GNOME GSettings compatibility shortcut entries",
            managed.len()
        ),
    );
    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn sync_linux_gnome_fallback(
    app: &AppHandle,
    shortcuts: &ShortcutSettings,
) -> Result<(), String> {
    let state = app
        .try_state::<crate::models::AppState>()
        .ok_or_else(|| "application state is unavailable".to_string())?;
    let trigger_dir = ensure_linux_trigger_watcher(app.clone(), state.inner().clone())?;

    // GNOME MediaKeys 启动一个极小的本地转发脚本，再由应用入口把动作写给已运行实例；这避开了桌面守护进程无法可靠调用应用自建 D-Bus 名称的问题。
    // GNOME MediaKeys launches a tiny local relay script and the application entrypoint forwards the action to the running instance; this avoids unreliable calls from the desktop daemon to an app-owned D-Bus name.
    remove_legacy_linux_dbus_activation_service(&state.paths);
    sync_linux_desktop_shortcuts(shortcuts, &state.paths, &trigger_dir)?;
    app_log::info(
        &state.paths,
        "shortcut",
        "GNOME MediaKeys fallback published with trigger-file relay; awaiting first physical activation",
    );
    Ok(())
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

#[cfg(target_os = "linux")]
pub(crate) fn handle_portal_action(
    app: &AppHandle,
    state: &crate::models::AppState,
    action: ShortcutAction,
) -> Result<(), String> {
    // 门户事件已由桌面合成器授权，继续复用统一动作处理器可保证托盘、原生后端和门户后端具有完全一致的状态同步。
    // Portal events are compositor-authorized; reusing the shared action handler keeps tray, native, and portal backends fully consistent.
    handle_action(app, state, action)
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
    // 后端先归一化不同平台的键名，是为了让前端显示 Control/Option/Command 后仍能注册为同一个真实快捷键。
    // The backend normalizes platform-specific key names first so shortcuts displayed as Control/Option/Command still register as the same real accelerator.
    let canonical = canonical_shortcut_string(value);
    let mut candidates = vec![canonical.clone()];
    if canonical.contains("Ctrl") {
        candidates.push(canonical.replace("Ctrl", "Control"));
        candidates.push(canonical.replace("Ctrl", "CommandOrControl"));
    }
    if canonical.contains("Meta") {
        candidates.push(canonical.replace("Meta", "Command"));
        candidates.push(canonical.replace("Meta", "Super"));
        candidates.push(canonical.replace("Meta", "Cmd"));
    }
    if canonical.contains("Alt") {
        candidates.push(canonical.replace("Alt", "Option"));
    }
    candidates.dedup();
    for candidate in candidates {
        if let Ok(shortcut) = Shortcut::try_from(candidate.as_str()) {
            return Ok(shortcut);
        }
    }
    Err("invalid shortcut format".into())
}

fn canonical_shortcut_string(value: &str) -> String {
    value
        .split('+')
        .map(|token| canonical_shortcut_token(token.trim()))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join("+")
}

fn canonical_shortcut_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "ctrl" | "control" => "Ctrl".into(),
        "shift" => "Shift".into(),
        "alt" | "option" => "Alt".into(),
        "meta" | "command" | "cmd" | "super" | "win" | "windows" => "Meta".into(),
        "escape" => "Esc".into(),
        "arrowup" => "Up".into(),
        "arrowdown" => "Down".into(),
        "arrowleft" => "Left".into(),
        "arrowright" => "Right".into(),
        _ => token.to_string(),
    }
}


fn shortcut_entries(shortcuts: &ShortcutSettings) -> [(&'static str, &str); 5] {
    [
        ("toggle_pin_service", shortcuts.toggle_pin_service.as_str()),
        ("toggle_history_service", shortcuts.toggle_history_service.as_str()),
        ("toggle_main_window", shortcuts.toggle_main_window.as_str()),
        ("enter_light_mode", shortcuts.enter_light_mode.as_str()),
        ("toggle_theme_mode", shortcuts.toggle_theme_mode.as_str()),
    ]
}

pub(crate) fn validate_shortcut_settings(shortcuts: &ShortcutSettings) -> Result<(), String> {
    let mut seen = Vec::new();
    for (_, value) in shortcut_entries(shortcuts) {
        parse_shortcut(value).map_err(|_| format!("Invalid shortcut: {}", value))?;
        let normalized = canonical_shortcut_string(value).to_ascii_lowercase();
        if seen.contains(&normalized) {
            return Err(format!("Shortcut conflict: {}", value));
        }
        seen.push(normalized);
    }
    Ok(())
}

pub(crate) fn detect_shortcut_conflicts(
    shortcuts: &ShortcutSettings,
) -> Vec<ShortcutConflictPayload> {
    let entries = shortcut_entries(shortcuts);
    let mut conflicts = Vec::new();
    let mut normalized_entries = Vec::new();

    for (shortcut_key, value) in entries {
        let normalized = canonical_shortcut_string(value);
        if parse_shortcut(value).is_err() {
            conflicts.push(ShortcutConflictPayload {
                shortcut_key: shortcut_key.to_string(),
                shortcut: value.to_string(),
                kind: "invalid".to_string(),
                source: "ClipAnchor".to_string(),
            });
            continue;
        }
        normalized_entries.push((shortcut_key, value, normalized.to_ascii_lowercase()));
    }

    for (index, (shortcut_key, value, normalized)) in normalized_entries.iter().enumerate() {
        if normalized_entries
            .iter()
            .enumerate()
            .any(|(other_index, (_, _, other))| other_index != index && other == normalized)
        {
            conflicts.push(ShortcutConflictPayload {
                shortcut_key: (*shortcut_key).to_string(),
                shortcut: (*value).to_string(),
                kind: "duplicate".to_string(),
                source: "ClipAnchor".to_string(),
            });
        }
    }

    #[cfg(target_os = "linux")]
    {
        if linux_supports_gnome_custom_shortcuts() {
            match linux_system_shortcut_bindings() {
                Ok(system_bindings) => {
                    for (shortcut_key, value, normalized) in &normalized_entries {
                        let sources = system_bindings
                            .iter()
                            .filter(|(_, binding)| binding == normalized)
                            .map(|(source, _)| source.clone())
                            .collect::<Vec<_>>();
                        if !sources.is_empty() {
                            conflicts.push(ShortcutConflictPayload {
                                shortcut_key: (*shortcut_key).to_string(),
                                shortcut: (*value).to_string(),
                                kind: "system".to_string(),
                                source: sources.into_iter().take(3).collect::<Vec<_>>().join(", "),
                            });
                        }
                    }
                }
                Err(error) => conflicts.push(ShortcutConflictPayload {
                    shortcut_key: String::new(),
                    shortcut: String::new(),
                    kind: "check_failed".to_string(),
                    source: error,
                }),
            }
        }
    }

    conflicts
}

#[cfg(target_os = "linux")]
fn linux_system_shortcut_bindings() -> Result<Vec<(String, String)>, String> {
    let schemas_output = Command::new("gsettings")
        .arg("list-schemas")
        .output()
        .map_err(|error| format!("cannot enumerate GNOME shortcut schemas: {}", error))?;
    if !schemas_output.status.success() {
        return Err(command_error("gsettings list-schemas", &schemas_output));
    }

    let schemas = String::from_utf8_lossy(&schemas_output.stdout)
        .lines()
        .map(str::trim)
        .filter(|schema| {
            schema.contains("keybindings")
                || schema.contains("media-keys")
                || schema.contains("keyboard")
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut bindings = Vec::new();

    for schema in schemas {
        let output = match Command::new("gsettings")
            .args(["list-recursively", schema.as_str()])
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.splitn(3, char::is_whitespace);
            let source_schema = parts.next().unwrap_or(schema.as_str());
            let key = parts.next().unwrap_or("binding");
            let raw_value = parts.next().unwrap_or_default();
            for candidate in parse_gvariant_string_array(raw_value) {
                if let Some(binding) = gtk_binding_to_canonical(&candidate) {
                    bindings.push((format!("{}.{}", source_schema, key), binding));
                }
            }
        }
    }

    for path in linux_read_custom_shortcut_paths().unwrap_or_default() {
        if linux_is_managed_custom_shortcut_path(&path) {
            continue;
        }
        let schema = format!("{}:{}", LINUX_CUSTOM_KEY_SCHEMA, path);
        let binding = match linux_gsettings_get(&schema, "binding")
            .ok()
            .and_then(|value| first_gvariant_string(&value))
            .and_then(|value| gtk_binding_to_canonical(&value))
        {
            Some(binding) => binding,
            None => continue,
        };
        let name = linux_gsettings_get(&schema, "name")
            .ok()
            .and_then(|value| first_gvariant_string(&value))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| path.clone());
        bindings.push((name, binding));
    }

    bindings.sort();
    bindings.dedup();
    Ok(bindings)
}

#[cfg(target_os = "linux")]
fn gtk_binding_to_canonical(value: &str) -> Option<String> {
    let mut remaining = value.trim();
    if remaining.is_empty() || remaining.eq_ignore_ascii_case("disabled") {
        return None;
    }
    let mut modifiers = Vec::new();
    while let Some(stripped) = remaining.strip_prefix('<') {
        let end = stripped.find('>')?;
        let modifier = &stripped[..end];
        match modifier.to_ascii_lowercase().as_str() {
            "primary" | "control" | "ctrl" => modifiers.push("Ctrl"),
            "shift" => modifiers.push("Shift"),
            "alt" | "mod1" => modifiers.push("Alt"),
            "super" | "meta" | "mod4" => modifiers.push("Meta"),
            _ => {}
        }
        remaining = stripped[end + 1..].trim();
    }
    if remaining.is_empty() {
        return None;
    }
    modifiers.push(remaining);
    Some(canonical_shortcut_string(&modifiers.join("+")).to_ascii_lowercase())
}

#[cfg(target_os = "linux")]
fn linux_shortcut_actions() -> [ShortcutAction; 5] {
    [
        ShortcutAction::TogglePinService,
        ShortcutAction::ToggleHistoryService,
        ShortcutAction::ToggleMainWindow,
        ShortcutAction::EnterLightMode,
        ShortcutAction::ToggleThemeMode,
    ]
}

#[cfg(target_os = "linux")]
fn linux_trigger_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("shortcut-triggers")
}

#[cfg(target_os = "linux")]
fn linux_trigger_path(trigger_dir: &Path, action: ShortcutAction) -> PathBuf {
    trigger_dir.join(format!("{}.trigger", action.cli_id()))
}

#[cfg(target_os = "linux")]
fn linux_watcher_pid_path(trigger_dir: &Path) -> PathBuf {
    trigger_dir.join("watcher.pid")
}

#[cfg(target_os = "linux")]
fn linux_relay_script_path(trigger_dir: &Path, action: ShortcutAction) -> PathBuf {
    trigger_dir.join(format!("{}.relay.sh", action.cli_id()))
}

#[cfg(target_os = "linux")]
fn ensure_linux_trigger_watcher(
    app: AppHandle,
    state: crate::models::AppState,
) -> Result<PathBuf, String> {
    let trigger_dir = linux_trigger_dir(&state.paths.data);
    fs::create_dir_all(&trigger_dir).map_err(|error| {
        format!(
            "cannot create Linux shortcut trigger directory {}: {}",
            trigger_dir.display(),
            error
        )
    })?;

    fs::write(linux_watcher_pid_path(&trigger_dir), std::process::id().to_string())
        .map_err(|error| format!("cannot write Linux shortcut watcher identity: {}", error))?;
    if LINUX_TRIGGER_WATCHER_STARTED.get().is_some() {
        // 设置页重新注册快捷键时监听线程仍在运行，不能再次删除 trigger 文件，否则恰好到达的物理按键会被同步流程吞掉。
        // The watcher remains active while settings re-register shortcuts, so trigger files must not be cleared again or a physical press arriving during synchronization could be lost.
        return Ok(trigger_dir);
    }

    // 只在首次启动监听器时清理异常退出遗留信号，避免旧事件在新进程中被误执行。
    // Stale signals from an abnormal exit are cleared only when the watcher first starts so old events cannot execute in the new process.
    for action in linux_shortcut_actions() {
        let _ = fs::remove_file(linux_trigger_path(&trigger_dir, action));
        let _ = fs::remove_file(trigger_dir.join(format!("{}.processing", action.cli_id())));
    }

    if LINUX_TRIGGER_WATCHER_STARTED.set(()).is_err() {
        return Ok(trigger_dir);
    }

    app_log::info(
        &state.paths,
        "shortcut",
        format!(
            "Linux shortcut trigger watcher started at {}",
            trigger_dir.display()
        ),
    );

    let watcher_dir = trigger_dir.clone();
    thread::spawn(move || loop {
        for action in linux_shortcut_actions() {
            let trigger = linux_trigger_path(&watcher_dir, action);
            let processing = watcher_dir.join(format!("{}.processing", action.cli_id()));
            // 原子重命名先取得当前触发信号；处理期间再次按键会生成新的 trigger 文件，下一轮仍能独立消费。
            // An atomic rename claims the current signal first; another press during handling creates a new trigger file that remains available for the next cycle.
            if fs::rename(&trigger, &processing).is_err() {
                continue;
            }
            let activation_count = fs::read(&processing)
                .map(|content| content.len().clamp(1, 8))
                .unwrap_or(1);
            let _ = fs::remove_file(&processing);
            app_log::info(
                &state.paths,
                "shortcut",
                format!(
                    "Linux shortcut physical activation received action={:?} count={}",
                    action, activation_count
                ),
            );
            // 同一轮询周期内的快速连按会累积多个字节；逐次执行而不是只看文件是否存在，可避免双击或快速切换丢失第二次动作。
            // Rapid presses within one polling cycle accumulate bytes; executing once per byte instead of testing only file existence preserves double presses and quick toggles.
            for _ in 0..activation_count {
                if let Err(error) = handle_action(&app, &state, action) {
                    app_log::warn(
                        &state.paths,
                        "shortcut",
                        format!("Linux trigger action {:?} failed: {}", action, error),
                    );
                }
            }
        }
        thread::sleep(Duration::from_millis(55));
    });

    Ok(trigger_dir)
}

#[cfg(target_os = "linux")]
fn write_linux_relay_script(
    executable: &Path,
    trigger_dir: &Path,
    action: ShortcutAction,
) -> Result<PathBuf, String> {
    let script_path = linux_relay_script_path(trigger_dir, action);
    let trigger_argument = format!(
        "--clipanchor-trigger-file={}",
        linux_trigger_path(trigger_dir, action).to_string_lossy()
    );
    let action_argument = format!("--clipanchor-shortcut={}", action.cli_id());
    let content = format!(
        "#!/bin/sh\nif [ \"${{1:-}}\" = \"--probe\" ]; then\n  exec {} --clipanchor-shortcut-probe\nfi\nexec {} {} {}\n",
        shell_quote_linux(&executable.to_string_lossy()),
        shell_quote_linux(&executable.to_string_lossy()),
        shell_quote_linux(&trigger_argument),
        shell_quote_linux(&action_argument),
    );
    let temporary = script_path.with_extension("sh.tmp");
    fs::write(&temporary, content).map_err(|error| {
        format!(
            "cannot write Linux shortcut relay {}: {}",
            temporary.display(),
            error
        )
    })?;
    fs::rename(&temporary, &script_path).map_err(|error| {
        format!(
            "cannot install Linux shortcut relay {}: {}",
            script_path.display(),
            error
        )
    })?;
    Ok(script_path)
}

#[cfg(target_os = "linux")]
fn verify_linux_relay_script(script_path: &Path) -> Result<(), String> {
    // 使用与 GNOME 配置相同的 /bin/sh 执行探针，可同时验证脚本内容、路径引用和当前二进制入口，而不会触发任何用户动作。
    // Running the probe through the same /bin/sh path stored in GNOME verifies the script, path references, and binary entrypoint without firing a user action.
    let output = Command::new("/bin/sh")
        .arg(script_path)
        .arg("--probe")
        .output()
        .map_err(|error| format!("cannot execute Linux shortcut relay probe: {}", error))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || stdout.trim() != "clipanchor-shortcut-relay-ok" {
        return Err(command_error("Linux shortcut relay probe", &output));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_relay_command(script_path: &Path) -> String {
    format!(
        "{} {}",
        shell_quote_linux("/bin/sh"),
        shell_quote_linux(&script_path.to_string_lossy())
    )
}

#[cfg(target_os = "linux")]
fn remove_legacy_linux_dbus_activation_service(log_paths: &crate::paths::DataPaths) {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        });
    let Some(data_home) = data_home else {
        return;
    };
    let service_path = data_home
        .join("dbus-1")
        .join("services")
        .join("com.clipanchor.Shortcuts.service");
    let Ok(content) = fs::read_to_string(&service_path) else {
        return;
    };
    if !content.contains("Name=com.clipanchor.Shortcuts") {
        return;
    }

    // 旧 D-Bus 激活文件会在应用退出后尝试拉起已经废弃的服务入口；仅在确认是 ClipAnchor 自身文件时删除，避免触碰用户的其他服务配置。
    // The legacy D-Bus activation file can relaunch an obsolete service entry after exit; it is removed only after confirming ownership so unrelated user services are never touched.
    match fs::remove_file(&service_path) {
        Ok(()) => app_log::info(
            log_paths,
            "shortcut",
            format!(
                "removed legacy Linux shortcut D-Bus activation service at {}",
                service_path.display()
            ),
        ),
        Err(error) => app_log::warn(
            log_paths,
            "shortcut",
            format!(
                "legacy Linux shortcut D-Bus activation service could not be removed: path={} detail={}",
                service_path.display(),
                error
            ),
        ),
    }
}

#[cfg(target_os = "linux")]
const LINUX_MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
#[cfg(target_os = "linux")]
const LINUX_CUSTOM_KEY_SCHEMA: &str =
    "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

#[cfg(target_os = "linux")]
fn linux_supports_gnome_custom_shortcuts() -> bool {
    let desktop = [
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        std::env::var("XDG_SESSION_DESKTOP").unwrap_or_default(),
        std::env::var("DESKTOP_SESSION").unwrap_or_default(),
    ]
    .join(":")
    .to_ascii_lowercase();
    // 这里只在 GNOME 家族桌面写入 gsettings，避免在 KDE 等 Wayland 会话中误用不存在的 GNOME schema。
    // gsettings is used only for the GNOME desktop family so KDE and other Wayland sessions are not sent through schemas they do not provide.
    desktop.contains("gnome")
        || desktop.contains("ubuntu")
        || desktop.contains("unity")
        || desktop.contains("cinnamon")
}

#[cfg(target_os = "linux")]
fn linux_session_is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or_else(|_| std::env::var_os("WAYLAND_DISPLAY").is_some())
}

#[cfg(target_os = "linux")]
fn gnome_media_keys_name_has_owner() -> Result<bool, String> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            "org.gnome.SettingsDaemon.MediaKeys",
        ])
        .output()
        .map_err(|error| format!("cannot query GNOME MediaKeys D-Bus ownership: {}", error))?;
    if !output.status.success() {
        return Err(command_error("GNOME MediaKeys D-Bus ownership query", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains("true"))
}

#[cfg(target_os = "linux")]
fn ensure_gnome_media_keys_service(log_paths: &crate::paths::DataPaths) -> Result<(), String> {
    if gnome_media_keys_name_has_owner()? {
        app_log::info(
            log_paths,
            "shortcut",
            "GNOME MediaKeys D-Bus service is already active",
        );
        return Ok(());
    }

    // RefuseManualStart/Stop 会阻止 systemctl 直接重启该服务，但 D-Bus 激活正是 GNOME 为会话组件保留的受支持启动路径。
    // RefuseManualStart/Stop blocks direct systemctl restarts, while D-Bus activation is the supported startup path GNOME keeps for session components.
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.StartServiceByName",
            "org.gnome.SettingsDaemon.MediaKeys",
            "0",
        ])
        .output()
        .map_err(|error| format!("cannot request GNOME MediaKeys D-Bus activation: {}", error))?;
    if !output.status.success() {
        return Err(command_error("GNOME MediaKeys D-Bus activation", &output));
    }

    for _ in 0..25 {
        if gnome_media_keys_name_has_owner().unwrap_or(false) {
            app_log::info(
                log_paths,
                "shortcut",
                format!(
                    "GNOME MediaKeys D-Bus service is active; activation_reply={}",
                    String::from_utf8_lossy(&output.stdout).trim()
                ),
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(80));
    }

    Err("GNOME MediaKeys service did not acquire its D-Bus name after activation".to_string())
}

#[cfg(target_os = "linux")]
fn recent_gnome_grab_failures(
    since_epoch_seconds: i64,
    managed_paths: &[String],
) -> Vec<String> {
    let since = format!("@{}", since_epoch_seconds);
    let output = match Command::new("journalctl")
        .args([
            "--user",
            "--unit",
            "org.gnome.SettingsDaemon.MediaKeys.service",
            "--since",
            since.as_str(),
            "--no-pager",
            "--output",
            "cat",
        ])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| {
            let path_specific_failure = line.contains("Failed to grab accelerator for keybinding")
                && (managed_paths.is_empty()
                    || managed_paths.iter().any(|path| line.contains(path)));
            let backend_failure = line.contains("Failed to grab accelerators:")
                || line.contains("Failed to create proxy for key grabber");
            path_specific_failure || backend_failure
        })
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(target_os = "linux")]
fn sync_linux_desktop_shortcuts(
    shortcuts: &ShortcutSettings,
    log_paths: &crate::paths::DataPaths,
    trigger_dir: &Path,
) -> Result<(), String> {
    ensure_linux_gsettings_schema()?;
    ensure_gnome_media_keys_service(log_paths)?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot resolve ClipAnchor executable for Linux shortcuts: {}", error))?;

    let raw_definitions = [
        (
            ShortcutAction::TogglePinService,
            "ClipAnchor: Toggle pin service",
            shortcuts.toggle_pin_service.as_str(),
        ),
        (
            ShortcutAction::ToggleHistoryService,
            "ClipAnchor: Toggle history service",
            shortcuts.toggle_history_service.as_str(),
        ),
        (
            ShortcutAction::ToggleMainWindow,
            "ClipAnchor: Show or hide main window",
            shortcuts.toggle_main_window.as_str(),
        ),
        (
            ShortcutAction::EnterLightMode,
            "ClipAnchor: Enter Lite mode",
            shortcuts.enter_light_mode.as_str(),
        ),
        (
            ShortcutAction::ToggleThemeMode,
            "ClipAnchor: Toggle theme",
            shortcuts.toggle_theme_mode.as_str(),
        ),
    ];

    // 所有组合键和转发脚本先完成验证，避免其中一个配置错误时把 GNOME 快捷键列表改成半完成状态。
    // Every accelerator and relay script is verified before GNOME settings are changed so one bad entry cannot leave a partially registered list.
    let definitions = raw_definitions
        .into_iter()
        .map(|(action, label, configured)| {
            let binding = shortcut_to_gtk_binding(configured)?;
            let relay_script = write_linux_relay_script(&executable, trigger_dir, action)?;
            verify_linux_relay_script(&relay_script)?;
            Ok((action, label, binding, relay_script))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let existing_paths = linux_read_custom_shortcut_paths().unwrap_or_default();
    let mut managed_paths = existing_paths
        .iter()
        .filter(|path| linux_is_managed_custom_shortcut_path(path))
        .cloned()
        .collect::<Vec<_>>();
    managed_paths.sort_by_key(|path| linux_custom_shortcut_slot(path).unwrap_or(usize::MAX));
    let retained_paths = existing_paths
        .iter()
        .filter(|path| !linux_is_managed_custom_shortcut_path(path))
        .cloned()
        .collect::<Vec<_>>();

    // 复用稳定 customN 路径并先把旧 binding 设为 disabled，可让 Settings Daemon 明确完成 ungrab；反复换槽位会留下异步订阅竞态，表现为配置存在但物理按键没有事件。
    // Stable customN paths are reused and old bindings are disabled first so Settings Daemon explicitly completes ungrab; rotating slots creates asynchronous subscription races where settings exist but physical presses produce no event.
    let mut paths = managed_paths
        .iter()
        .take(definitions.len())
        .cloned()
        .collect::<Vec<_>>();
    let mut occupied_slots = retained_paths
        .iter()
        .chain(paths.iter())
        .filter_map(|path| linux_custom_shortcut_slot(path))
        .collect::<Vec<_>>();
    occupied_slots.sort_unstable();
    occupied_slots.dedup();
    let mut candidate_slot = 0usize;
    while paths.len() < definitions.len() {
        while occupied_slots.contains(&candidate_slot) {
            candidate_slot = candidate_slot.saturating_add(1);
        }
        paths.push(linux_custom_shortcut_path(candidate_slot));
        occupied_slots.push(candidate_slot);
        candidate_slot = candidate_slot.saturating_add(1);
    }

    for path in &paths {
        let schema = format!("{}:{}", LINUX_CUSTOM_KEY_SCHEMA, path);
        linux_gsettings_set(&schema, "binding", &gvariant_string("disabled"))?;
    }
    thread::sleep(Duration::from_millis(420));

    for ((action, label, binding, relay_script), path) in definitions.iter().zip(paths.iter()) {
        let schema = format!("{}:{}", LINUX_CUSTOM_KEY_SCHEMA, path);
        let command = linux_relay_command(relay_script);

        linux_gsettings_set(&schema, "name", &gvariant_string(label))?;
        linux_gsettings_set(&schema, "command", &gvariant_string(&command))?;
        linux_gsettings_set(&schema, "binding", &gvariant_string(binding))?;

        let stored_command_raw = linux_gsettings_get(&schema, "command")?;
        let stored_binding_raw = linux_gsettings_get(&schema, "binding")?;
        let stored_command = first_gvariant_string(&stored_command_raw).unwrap_or_default();
        let stored_binding = first_gvariant_string(&stored_binding_raw).unwrap_or_default();
        let expected_binding = gtk_binding_to_canonical(binding);
        let actual_binding = gtk_binding_to_canonical(&stored_binding);
        if expected_binding.is_none() || actual_binding != expected_binding {
            return Err(format!(
                "GNOME shortcut binding verification failed for {}: expected {}, stored {}",
                label, binding, stored_binding_raw
            ));
        }
        if !stored_command.contains("/bin/sh")
            || !stored_command.contains(action.cli_id())
            || !stored_command.contains(".relay.sh")
        {
            return Err(format!(
                "GNOME shortcut command verification failed for {}: stored {}",
                label, stored_command_raw
            ));
        }

        app_log::info(
            log_paths,
            "shortcut",
            format!(
                "GNOME shortcut entry prepared action={:?} binding={} path={} relay=trigger-file script={}",
                action,
                binding,
                path,
                relay_script.display()
            ),
        );
    }

    let registration_started_at = chrono::Utc::now().timestamp().saturating_sub(1);
    let mut active_paths = retained_paths.clone();
    active_paths.extend(paths.iter().cloned());
    linux_gsettings_set(
        LINUX_MEDIA_KEYS_SCHEMA,
        "custom-keybindings",
        &gvariant_string_array(&active_paths),
    )?;

    // GNOME 的抓键过程通过异步 D-Bus 往返完成；等待队列稳定后再校验，才能区分配置写入成功与真实抓键失败。
    // GNOME completes grabs through asynchronous D-Bus round trips; validation waits for the queue to settle so persisted settings are not mistaken for successful physical grabs.
    thread::sleep(Duration::from_millis(1250));
    let published_paths = linux_read_custom_shortcut_paths()?;
    if paths.iter().any(|path| !published_paths.contains(path)) {
        return Err(format!(
            "GNOME did not publish every ClipAnchor custom shortcut path; active paths: {:?}",
            published_paths
        ));
    }

    let grab_failures = recent_gnome_grab_failures(registration_started_at, &paths);
    if !grab_failures.is_empty() {
        return Err(format!(
            "GNOME MediaKeys reported accelerator grab failure: {}",
            grab_failures.join(" | ")
        ));
    }

    let stale_paths = managed_paths
        .iter()
        .filter(|path| !paths.contains(path))
        .cloned()
        .collect::<Vec<_>>();
    for path in &stale_paths {
        let schema = format!("{}:{}", LINUX_CUSTOM_KEY_SCHEMA, path);
        match Command::new("gsettings")
            .arg("reset-recursively")
            .arg(&schema)
            .output()
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => app_log::warn(
                log_paths,
                "shortcut",
                format!(
                    "stale GNOME shortcut child could not be reset: path={} detail={}",
                    path,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ),
            Err(error) => app_log::warn(
                log_paths,
                "shortcut",
                format!(
                    "stale GNOME shortcut child reset could not start: path={} detail={}",
                    path, error
                ),
            ),
        }
    }

    app_log::info(
        log_paths,
        "shortcut",
        format!(
            "GNOME MediaKeys published {} stable ClipAnchor shortcut entries; reused={} created={} removed={} reported_grab_failures=0",
            paths.len(),
            managed_paths.len().min(paths.len()),
            paths.len().saturating_sub(managed_paths.len()),
            stale_paths.len()
        ),
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn ensure_linux_gsettings_schema() -> Result<(), String> {
    let fixed = Command::new("gsettings")
        .arg("list-schemas")
        .output()
        .map_err(|error| format!("gsettings is unavailable: {}", error))?;
    if !fixed.status.success() {
        return Err(command_error("gsettings list-schemas", &fixed));
    }
    let fixed_schemas = String::from_utf8_lossy(&fixed.stdout);
    if !fixed_schemas
        .lines()
        .any(|line| line.trim() == LINUX_MEDIA_KEYS_SCHEMA)
    {
        return Err("GNOME media-key schema is unavailable on this desktop environment".to_string());
    }

    let relocatable = Command::new("gsettings")
        .arg("list-relocatable-schemas")
        .output()
        .map_err(|error| format!("cannot inspect relocatable gsettings schemas: {}", error))?;
    if !relocatable.status.success() {
        return Err(command_error(
            "gsettings list-relocatable-schemas",
            &relocatable,
        ));
    }
    let relocatable_schemas = String::from_utf8_lossy(&relocatable.stdout);
    // 自定义快捷键 schema 是可重定位 schema；使用专用列表检测才能避免在 Ubuntu 上把实际存在的能力误判为缺失。
    // The custom-keybinding schema is relocatable; checking the dedicated list prevents Ubuntu from treating an available capability as missing.
    if !relocatable_schemas
        .lines()
        .any(|line| line.trim() == LINUX_CUSTOM_KEY_SCHEMA)
    {
        return Err(
            "GNOME custom-keybinding schema is unavailable on this desktop environment"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_custom_shortcut_path(slot: usize) -> String {
    format!(
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom{}/",
        slot
    )
}

#[cfg(target_os = "linux")]
fn linux_custom_shortcut_slot(path: &str) -> Option<usize> {
    let prefix = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom";
    path.strip_prefix(prefix)?
        .strip_suffix('/')?
        .parse::<usize>()
        .ok()
}

#[cfg(target_os = "linux")]
fn linux_is_managed_custom_shortcut_path(path: &str) -> bool {
    if path.starts_with(
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clipanchor-",
    ) {
        return true;
    }

    let schema = format!("{}:{}", LINUX_CUSTOM_KEY_SCHEMA, path);
    let name = linux_gsettings_get(&schema, "name")
        .ok()
        .and_then(|value| first_gvariant_string(&value))
        .unwrap_or_default();
    if name.starts_with("ClipAnchor:") {
        return true;
    }

    let command = linux_gsettings_get(&schema, "command")
        .ok()
        .and_then(|value| first_gvariant_string(&value))
        .unwrap_or_default();
    command.contains("--clipanchor-shortcut=")
        || command.contains("--clipanchor-trigger-file=")
}

#[cfg(target_os = "linux")]
fn linux_read_custom_shortcut_paths() -> Result<Vec<String>, String> {
    let output = Command::new("gsettings")
        .args([
            "get",
            LINUX_MEDIA_KEYS_SCHEMA,
            "custom-keybindings",
        ])
        .output()
        .map_err(|error| format!("cannot read GNOME custom shortcuts: {}", error))?;
    if !output.status.success() {
        return Err(command_error("gsettings get custom-keybindings", &output));
    }
    Ok(parse_gvariant_string_array(
        &String::from_utf8_lossy(&output.stdout),
    ))
}

#[cfg(target_os = "linux")]
fn linux_gsettings_set(schema: &str, key: &str, value: &str) -> Result<(), String> {
    let output = Command::new("gsettings")
        .args(["set", schema, key, value])
        .output()
        .map_err(|error| format!("cannot update Linux shortcut setting: {}", error))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(
            &format!("gsettings set {} {}", schema, key),
            &output,
        ))
    }
}

#[cfg(target_os = "linux")]
fn linux_gsettings_get(schema: &str, key: &str) -> Result<String, String> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .map_err(|error| format!("cannot read Linux shortcut setting: {}", error))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(command_error(
            &format!("gsettings get {} {}", schema, key),
            &output,
        ))
    }
}

#[cfg(target_os = "linux")]
fn first_gvariant_string(value: &str) -> Option<String> {
    parse_gvariant_string_array(value).into_iter().next()
}

#[cfg(target_os = "linux")]
fn command_error(label: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    if detail.is_empty() {
        format!("{} failed with status {}", label, output.status)
    } else {
        format!("{} failed: {}", label, detail)
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn shortcut_to_gtk_binding(value: &str) -> Result<String, String> {
    let canonical = canonical_shortcut_string(value);
    let mut modifiers = Vec::new();
    let mut key = None::<String>;
    for token in canonical.split('+').filter(|token| !token.is_empty()) {
        match token {
            // GNOME 自定义快捷键最终由 Mutter/Settings Daemon 解析；显式写入 Control 可避免 GTK 专用 Primary 别名在部分 Ubuntu 会话中被保存却没有真正抓键。
            // GNOME custom shortcuts are ultimately parsed by Mutter/Settings Daemon; writing Control explicitly avoids the GTK-oriented Primary alias being persisted without a real grab on some Ubuntu sessions.
            "Ctrl" => modifiers.push("<Control>"),
            "Shift" => modifiers.push("<Shift>"),
            "Alt" => modifiers.push("<Alt>"),
            "Meta" => modifiers.push("<Super>"),
            other if key.is_none() => key = Some(gtk_key_name(other)),
            _ => return Err(format!("invalid Linux shortcut: {}", value)),
        }
    }
    let key = key.ok_or_else(|| format!("shortcut has no key: {}", value))?;
    Ok(format!("{}{}", modifiers.join(""), key))
}

#[cfg(target_os = "linux")]
fn gtk_key_name(value: &str) -> String {
    if value.len() == 1 {
        return value.to_ascii_lowercase();
    }
    match value.to_ascii_lowercase().as_str() {
        "esc" | "escape" => "Escape".to_string(),
        "up" | "arrowup" => "Up".to_string(),
        "down" | "arrowdown" => "Down".to_string(),
        "left" | "arrowleft" => "Left".to_string(),
        "right" | "arrowright" => "Right".to_string(),
        "return" => "Return".to_string(),
        "space" => "space".to_string(),
        "pageup" => "Page_Up".to_string(),
        "pagedown" => "Page_Down".to_string(),
        other => other.to_string(),
    }
}

#[cfg(target_os = "linux")]
fn gvariant_string(value: &str) -> String {
    format!(
        "'{}'",
        value.replace('\\', "\\\\").replace('\'', "\\'")
    )
}

#[cfg(target_os = "linux")]
fn gvariant_string_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| gvariant_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(target_os = "linux")]
fn parse_gvariant_string_array(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quote = None::<char>;
    let mut escaped = false;

    for character in value.chars() {
        match quote {
            None if character == '\'' || character == '"' => {
                quote = Some(character);
                current.clear();
            }
            None => {}
            Some(_) if escaped => {
                current.push(character);
                escaped = false;
            }
            Some(_) if character == '\\' => escaped = true,
            Some(active_quote) if character == active_quote => {
                quote = None;
                result.push(current.clone());
            }
            Some(_) => current.push(character),
        }
    }

    result
}

#[cfg(target_os = "linux")]
fn shell_quote_linux(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
