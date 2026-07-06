use crate::{app_log, models::UpdateStatusPayload, paths::DataPaths};
use chrono::{SecondsFormat, Utc};
use std::{fs, path::PathBuf};

const UPDATE_STATUS_FILE: &str = "update-status.json";

pub fn startup_background_check(paths: &DataPaths, started_in_lite_mode: bool) -> UpdateStatusPayload {
    // 启动阶段只写入静默状态，是为了避免后台启动时用更新提示打断用户。
    // Startup only records a silent state so background launch never interrupts users with update notices.
    let status = idle_status("startup_background");
    app_log::info(
        paths,
        "update",
        format!(
            "startup update state checked silently; lite_mode={}; update service is not available yet",
            started_in_lite_mode
        ),
    );
    let _ = save_status(paths, &status);
    status
}

pub fn main_open_check(paths: &DataPaths) -> UpdateStatusPayload {
    // 主界面打开时保留一次检查入口，是为了覆盖“自启动时没有更新，但用户稍后打开界面时已经有新版”的场景。
    // Keeping a main-open check covers the case where startup found nothing but a new version appears before the user opens the interface later.
    let status = read_status(paths).unwrap_or_else(|| idle_status("main_open"));
    app_log::info(paths, "update", format!("main window update status loaded; status={}", status.status));
    status
}

pub fn manual_placeholder_check(paths: &DataPaths) -> UpdateStatusPayload {
    // 手动点击检查更新时返回软件内提示，是为了用统一 UI 告知当前没有开放更新服务。
    // Manual update checks return an in-app notice so users learn the update service is not available through consistent UI.
    let mut status = idle_status("manual");
    status.status = "service_unavailable".into();
    status.prompt_on_main_open = true;
    app_log::info(paths, "update", "manual update notice opened; update service is not available yet");
    let mut persisted = status.clone();
    persisted.prompt_on_main_open = false;
    let _ = save_status(paths, &persisted);
    status
}

fn idle_status(source: &str) -> UpdateStatusPayload {
    UpdateStatusPayload {
        status: "idle".into(),
        service_enabled: false,
        update_available: false,
        update_failed: false,
        prompt_on_main_open: false,
        attention_required: false,
        checked_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        source: source.into(),
    }
}

fn read_status(paths: &DataPaths) -> Option<UpdateStatusPayload> {
    let text = fs::read_to_string(status_path(paths)).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_status(paths: &DataPaths, status: &UpdateStatusPayload) -> Result<(), String> {
    fs::create_dir_all(&paths.data).map_err(|error| error.to_string())?;
    let text = serde_json::to_string_pretty(status).map_err(|error| error.to_string())?;
    fs::write(status_path(paths), text).map_err(|error| error.to_string())
}

fn status_path(paths: &DataPaths) -> PathBuf {
    paths.data.join(UPDATE_STATUS_FILE)
}
