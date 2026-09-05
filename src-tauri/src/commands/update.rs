use crate::{models::{AppState, UpdateStatusPayload}, update_service};
use tauri::{AppHandle, State};

#[tauri::command]
pub fn get_update_status(state: State<'_, AppState>) -> Result<UpdateStatusPayload, String> {
    // 主界面首次打开时读取后台检查结果，是为了把更新失败或发现新版本的提示延后到用户看得到的位置。
    // The main UI reads the background check result so update failures or available versions are surfaced only where users can see them.
    Ok(update_service::main_open_check(&state.paths))
}

#[tauri::command]
pub fn check_update(app: AppHandle, state: State<'_, AppState>, source: Option<String>) -> Result<UpdateStatusPayload, String> {
    // 手动检查立即进入统一更新状态流，是为了让前端先显示“正在检查”页面，再等待 GitHub Release 结果返回。
    // Manual checks use the unified update state flow so the UI can show a checking page immediately before GitHub Release results arrive.
    let requested_source = source.unwrap_or_else(|| "manual".into());
    if requested_source == "startup_background" {
        let auto_update_enabled = state
            .settings
            .lock()
            .map(|settings| settings.auto_update_enabled)
            .unwrap_or(true);
        // 前端或托盘主动复用启动检查入口时仍尊重自动更新开关，是为了保证设置含义在所有入口一致。
        // Frontend or tray reuse of the startup-check entry still respects Auto Update so the setting means the same thing from every entry.
        return Ok(update_service::startup_background_check(
            &app,
            &state.paths,
            false,
            auto_update_enabled,
        ));
    }
    Ok(update_service::manual_check(&state.paths, &requested_source))
}

#[tauri::command]
pub fn install_downloaded_update(app: AppHandle, state: State<'_, AppState>) -> Result<UpdateStatusPayload, String> {
    // 安装入口持有 AppHandle，是为了 macOS DMG 可以在启动覆盖脚本后安全退出当前 .app 并自动重开新版。
    // The install entry keeps AppHandle so macOS DMG updates can launch the replacement helper, quit the current app, and reopen the new build.
    update_service::install_downloaded_update(&app, &state.paths)
}

#[tauri::command]
pub fn dismiss_update_prompt(state: State<'_, AppState>) -> Result<UpdateStatusPayload, String> {
    // 用户选择稍后后只收起主动提示，仍保留更新入口红点，是为了避免每次打开主界面都重复打断。
    // Dismissing later hides only the proactive prompt while keeping the update-entry dot so the main window is not interrupted every time it opens.
    update_service::dismiss_prompt(&state.paths)
}
