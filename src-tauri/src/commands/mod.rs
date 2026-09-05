mod window;
mod i18n;
mod history;
mod update;

pub use window::*;
pub use i18n::*;
pub use history::*;
pub use update::*;

use crate::{app_log, autostart, models::{AppSettings, AppState, BootstrapPayload, PathPayload, PlatformCapabilities, ShortcutConflictPayload, ShortcutSettings}, popup, settings};
use std::{fs, path::Path, process::Command};
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn get_bootstrap(window: tauri::WebviewWindow, state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
    let mut settings_guard = state.settings.lock().map_err(|error| error.to_string())?;
    let actual_autostart = match autostart::reconcile(settings_guard.auto_start, &state.paths.root) {
        Ok(actual) => actual,
        Err(error) => {
            // 注册表状态读取失败不应阻断整个主界面加载；保留上次设置并记录错误，用户仍可进入设置页再次操作修复。
            // A registry-state read failure must not block the entire main UI; keeping the last setting and logging the error lets the user reopen Settings and retry the repair.
            app_log::warn(
                &state.paths,
                "autostart",
                format!("system autostart state could not be read: {}", error),
            );
            settings_guard.auto_start
        }
    };
    if actual_autostart != settings_guard.auto_start {
        // 设置页加载时再次读取系统状态，是为了捕获用户在任务管理器中刚做出的切换，而无需重启客户端才能看到正确开关。
        // Reading the OS state again when Settings loads captures a recent Task Manager toggle without requiring the client to restart before showing the correct switch.
        settings_guard.auto_start = actual_autostart;
        settings::save(&state.paths, &settings_guard)?;
    }
    let mut settings = settings_guard.clone();
    drop(settings_guard);
    if window.label().starts_with("clipanchor-popup-") {
        // 弹窗只需要主题、语言和销毁时间；密钥不下发到剪贴板卡片 WebView。
        // Popups only need theme, locale, and destroy timing; credentials are not sent to clipboard-card WebViews.
        settings.translation_api_key.clear();
        settings.translation_api_keys.clear();
    }
    Ok(BootstrapPayload {
        settings,
        paths: PathPayload {
            data: state.paths.data.to_string_lossy().to_string(),
            database: state.paths.database.to_string_lossy().to_string(),
            resources: state.paths.resources.to_string_lossy().to_string(),
            locales: state.paths.locales.to_string_lossy().to_string(),
            logs: state.paths.logs.to_string_lossy().to_string(),
        },
        capabilities: PlatformCapabilities {
            platform: std::env::consts::OS.to_string(),
            // Linux 桌面尤其是 Wayland 不允许应用可靠指定顶层窗口坐标，因此前端必须隐藏会产生错误预期的定位入口。
            // Linux desktops, especially Wayland, do not let apps reliably choose top-level window coordinates, so the UI must hide a control that would create a false promise.
            popup_position_supported: popup::popup_position_supported(),
            // Linux 桌面对全局快捷键的授权与实现差异较大；显式关闭能力可让前端像弹窗定位一样隐藏不可靠的入口。
            // Linux desktop authorization and global-shortcut implementations vary widely; disabling the capability lets the frontend hide the unreliable entry just like popup positioning.
            global_shortcuts_supported: crate::shortcut::global_shortcuts_supported(),
        },
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[tauri::command]
pub fn check_shortcut_conflicts(
    shortcuts: ShortcutSettings,
) -> Result<Vec<ShortcutConflictPayload>, String> {
    if !crate::shortcut::global_shortcuts_supported() {
        // Linux 不展示快捷键设置，因此也不执行无意义的系统冲突探测，避免触发桌面环境兼容逻辑。
        // Linux does not expose shortcut settings, so system conflict probing is skipped to avoid invoking desktop-integration compatibility paths.
        return Ok(Vec::new());
    }
    // 冲突扫描是只读诊断，独立于保存流程运行；这样默认组合一打开设置页就能提示，而不会先修改系统快捷键。
    // Conflict scanning is read-only and separate from saving, so default bindings can warn immediately when Settings opens without first changing OS shortcuts.
    Ok(crate::shortcut::detect_shortcut_conflicts(&shortcuts))
}

#[tauri::command]
pub fn save_settings(mut settings_value: AppSettings, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    settings::normalize_translation_settings(&mut settings_value, true);
    settings::normalize_runtime_settings(&mut settings_value);

    let previous_settings = state
        .settings
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    if !crate::shortcut::global_shortcuts_supported() {
        // Linux 前端不会提交快捷键修改；继续保留已存字段是为了兼容现有配置文件，同时确保普通设置保存不会重新启用旧后端。
        // The Linux frontend never submits shortcut edits; preserving stored fields keeps existing settings compatible while ensuring normal saves cannot re-enable the retired backend.
        settings_value.shortcuts = previous_settings.shortcuts.clone();
    }
    validate_shortcuts(&settings_value)?;
    let shortcuts_changed = crate::shortcut::global_shortcuts_supported()
        && previous_settings.shortcuts != settings_value.shortcuts;

    // 只有快捷键字段真正变化时才重新注册系统快捷键，避免切换语言、主题等普通设置被 Linux 桌面能力故障阻断。
    // System shortcuts are re-registered only when shortcut fields actually change, preventing Linux desktop integration failures from blocking ordinary language or theme changes.
    if shortcuts_changed {
        crate::shortcut::sync_shortcuts(&app, &settings_value.shortcuts)?;
    }

    app_log::info(
        &state.paths,
        "settings",
        format!("saving settings from UI; shortcuts_changed={}", shortcuts_changed),
    );

    {
        let mut guard = state.settings.lock().map_err(|error| error.to_string())?;
        *guard = settings_value.clone();
        if let Err(error) = settings::save(&state.paths, &settings_value) {
            // 保存失败时恢复内存设置和旧快捷键，避免界面、配置文件与系统注册状态分别停留在不同版本。
            // On persistence failure, restore both in-memory settings and the previous shortcuts so the UI, settings file, and OS registration cannot diverge.
            *guard = previous_settings.clone();
            drop(guard);
            if shortcuts_changed {
                if let Err(restore_error) = crate::shortcut::sync_shortcuts(&app, &previous_settings.shortcuts) {
                    app_log::warn(
                        &state.paths,
                        "shortcut",
                        format!("could not restore previous shortcuts after settings save failure: {}", restore_error),
                    );
                }
            }
            return Err(error);
        }
    }

    if previous_settings.locale != settings_value.locale {
        app_log::info(
            &state.paths,
            "i18n",
            format!(
                "active language changed from {} to {}",
                previous_settings.locale, settings_value.locale
            ),
        );
    }
    if previous_settings.theme != settings_value.theme {
        app_log::info(
            &state.paths,
            "theme",
            format!(
                "active theme changed from {} to {}",
                previous_settings.theme, settings_value.theme
            ),
        );
    }

    let _ = crate::tray::refresh_tray(&app);
    // 设置保存后广播给所有弹窗，是为了让已打开的弹窗也能立即跟随主界面深浅主题或扩展语言变化。
    // Broadcasting saved settings lets already-open popups immediately follow main-window theme or extension-language changes.
    let _ = app.emit("clipanchor-settings-changed", settings_value.clone());
    Ok(settings_value)
}

#[tauri::command]
pub fn set_pin_service(enabled: bool, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    app_log::info(&state.paths, "settings", format!("pin service set to {}", enabled));
    let updated = update_settings_flag(&state, |settings| settings.pin_service_enabled = enabled)?;
    let _ = crate::tray::refresh_tray(&app);
    // 手动点击和快捷键都必须广播同一个设置事件，避免主界面、设置页和弹窗出现状态不一致。
    // Manual clicks and shortcuts must broadcast the same settings event so the main UI, settings page, and popups never drift apart.
    let _ = app.emit("clipanchor-settings-changed", updated.clone());
    Ok(updated)
}

#[tauri::command]
pub fn set_history_service(enabled: bool, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    app_log::info(&state.paths, "settings", format!("history service set to {}", enabled));
    let updated = update_settings_flag(&state, |settings| settings.history_service_enabled = enabled)?;
    let _ = crate::tray::refresh_tray(&app);
    // 手动点击和快捷键都必须广播同一个设置事件，避免主界面、设置页和弹窗出现状态不一致。
    // Manual clicks and shortcuts must broadcast the same settings event so the main UI, settings page, and popups never drift apart.
    let _ = app.emit("clipanchor-settings-changed", updated.clone());
    Ok(updated)
}

#[tauri::command]
pub fn set_privacy_mode(enabled: bool, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    app_log::info(&state.paths, "settings", format!("legacy privacy mode set to {}", enabled));
    let updated = update_settings_flag(&state, |settings| {
        settings.privacy_mode = enabled;
        settings.privacy_filter_mode = if enabled { "light".into() } else { "off".into() };
    })?;
    let _ = crate::tray::refresh_tray(&app);
    let _ = app.emit("clipanchor-settings-changed", updated.clone());
    Ok(updated)
}

#[tauri::command]
pub fn set_privacy_filter_mode(mode: String, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    app_log::info(&state.paths, "settings", format!("privacy filter mode requested: {}", mode));
    let normalized = match mode.as_str() {
        "off" | "light" => mode,
        "smart" => "light".into(),
        _ => "light".into(),
    };
    let updated = update_settings_flag(&state, |settings| {
        // 新旧设置同时写入，是为了兼容已有 settings.json 中的布尔隐私字段和新三段式过滤模式。
        // Both the legacy boolean and the new three-level mode are written so existing settings.json files remain compatible.
        settings.privacy_mode = normalized != "off";
        settings.privacy_filter_mode = normalized;
    })?;
    let _ = crate::tray::refresh_tray(&app);
    let _ = app.emit("clipanchor-settings-changed", updated.clone());
    Ok(updated)
}

#[tauri::command]
pub fn set_clipboard_paused(paused: bool, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    app_log::info(&state.paths, "settings", format!("clipboard pause set to {}", paused));
    let updated = update_settings_flag(&state, |settings| settings.clipboard_paused = paused)?;
    let _ = crate::tray::refresh_tray(&app);
    let _ = app.emit("clipanchor-settings-changed", updated.clone());
    Ok(updated)
}

#[tauri::command]
pub fn set_autostart(enabled: bool, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    app_log::info(&state.paths, "settings", format!("autostart set to {}", enabled));
    autostart::apply(enabled, &state.paths.root)?;
    // 写入后立即从系统启动项重新读取，是为了让界面展示真实状态，而不是仅相信刚才的布尔参数。
    // Reading the OS entry immediately after writing keeps the UI tied to the real autostart state instead of trusting only the requested boolean.
    let actual = autostart::reconcile(enabled, &state.paths.root)?;
    let updated = update_settings_flag(&state, |settings| settings.auto_start = actual)?;
    let _ = crate::tray::refresh_tray(&app);
    // 自启动状态也广播统一设置事件，是为了让同一进程内的设置页、托盘和其他窗口立即使用同一个真实值。
    // Autostart also emits the shared settings event so Settings, tray, and other windows in the same process immediately use one authoritative value.
    let _ = app.emit("clipanchor-settings-changed", updated.clone());
    Ok(updated)
}

fn update_settings_flag<F>(state: &State<'_, AppState>, change: F) -> Result<AppSettings, String>
where
    F: FnOnce(&mut AppSettings),
{
    let mut guard = state.settings.lock().map_err(|error| error.to_string())?;
    change(&mut guard);
    settings::save(&state.paths, &guard)?;
    Ok(guard.clone())
}

#[tauri::command]
pub fn get_data_usage(state: State<'_, AppState>) -> Result<DataUsagePayload, String> {
    let bytes = directory_size(&state.paths.data)?;
    Ok(DataUsagePayload { bytes, display: human_size(bytes as i64) })
}

fn directory_size(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            total += directory_size(&entry.path())?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}

fn human_size(bytes: i64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value > 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", value, units[unit])
}

#[tauri::command]
pub fn get_log_status(state: State<'_, AppState>) -> Result<app_log::LogStatusPayload, String> {
    app_log::status(&state.paths)
}

#[tauri::command]
pub fn clear_logs(state: State<'_, AppState>) -> Result<app_log::LogStatusPayload, String> {
    // 清理日志后立即重建一条当前日志，是为了让维护人员能确认清理动作本身并继续记录后续问题。
    // After clearing logs, a new current log entry is created so maintainers can confirm the cleanup action and continue diagnosing later issues.
    let removed = app_log::clear(&state.paths)?;
    app_log::info(&state.paths, "log", format!("log cleanup completed from UI; removed {} file(s)", removed));
    app_log::status(&state.paths)
}

#[tauri::command]
pub fn open_log_folder(state: State<'_, AppState>) -> Result<(), String> {
    fs::create_dir_all(&state.paths.logs).map_err(|error| error.to_string())?;
    app_log::info(&state.paths, "log", "open log folder requested from UI");
    open_path_with_system(&state.paths.logs)
}

pub(super) fn open_path_with_system(path: &Path) -> Result<(), String> {
    // 日志目录用系统文件管理器打开，是为了让用户可以直接打包或删除日志，同时不把诊断文件内容塞进主界面造成卡顿。
    // The log directory opens in the system file manager so users can package or remove diagnostic files without loading them into the main UI.
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer");
        command.arg(path);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };
    command.spawn().map_err(|error| error.to_string())?;
    Ok(())
}
fn validate_shortcuts(settings_value: &AppSettings) -> Result<(), String> {
    crate::shortcut::validate_shortcut_settings(&settings_value.shortcuts)
}
