use crate::{app_log, autostart, clipboard_service, database, models::{AppSettings, AppState, BootstrapPayload, ClipItem, ClipKind, HistoryRecord, PathPayload, UpdateStatusPayload}, popup, settings, update_service};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use std::{collections::HashMap, fs, io::{Read, Write}, path::Path, process::Command, thread, time::Duration};

#[cfg(target_os = "windows")]
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::{FindWindowW, IsZoomed, ShowWindow, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE}};
use uuid::Uuid;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub fn minimize_window(app: AppHandle) -> Result<(), String> {
    // Windows 上优先走原生 ShowWindow，是因为部分 WebView2 无边框窗口会让 Tauri 高层 minimize 调用返回成功但界面不变化。
    // On Windows we prefer native ShowWindow because some borderless WebView2 windows make Tauri's high-level minimize report success without changing the UI.
    #[cfg(target_os = "windows")]
    {
        if native_minimize_main_window() {
            return Ok(());
        }
    }
    app.get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?
        .minimize()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn toggle_maximize_window(app: AppHandle) -> Result<(), String> {
    // Windows 上优先使用原生最大化/还原，是为了绕开自绘标题栏和 WebView 焦点导致的最大化按钮失效。
    // On Windows native maximize/restore bypasses custom-titlebar and WebView focus issues that can break the maximize button.
    #[cfg(target_os = "windows")]
    {
        if native_toggle_maximize_main_window() {
            return Ok(());
        }
    }
    let window = app.get_webview_window("main").ok_or_else(|| "Main window not found".to_string())?;
    if window.is_maximized().map_err(|error| error.to_string())? {
        window.unmaximize().map_err(|error| error.to_string())
    } else {
        window.maximize().map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub fn close_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(state) = app.try_state::<AppState>() {
        app_log::info(&state.paths, "window", "main window close button requested Lite mode hide");
    }
    // 关闭按钮只隐藏主界面而不销毁 WebView，是为了保证长时间轻量模式后仍能从托盘或快捷键稳定唤醒同一个主界面。
    // The close button only hides the main UI instead of destroying the WebView so tray and shortcut wake-ups remain reliable after long Lite-mode sessions.
    crate::window_control::hide_main_window(&app)
}

#[tauri::command]
pub fn quit_app(app: AppHandle) -> Result<(), String> {
    // 退出程序交给 Tauri 正常清理 WebView2 窗口，是为了避免强制 process::exit 触发 Chrome_WidgetWin_0 注销警告。
    // Quitting through Tauri lets WebView2 windows clean up normally, avoiding the Chrome_WidgetWin_0 unregister warning caused by forced process::exit.
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "windows")]
fn main_window_hwnd() -> HWND {
    let mut title: Vec<u16> = "ClipAnchor".encode_utf16().collect();
    title.push(0);
    unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) }
}

#[cfg(target_os = "windows")]
fn native_minimize_main_window() -> bool {
    let hwnd = main_window_hwnd();
    if hwnd.is_null() {
        return false;
    }
    unsafe { ShowWindow(hwnd, SW_MINIMIZE); }
    true
}

#[cfg(target_os = "windows")]
fn native_toggle_maximize_main_window() -> bool {
    let hwnd = main_window_hwnd();
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        if IsZoomed(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }
    true
}

#[tauri::command]
pub fn get_bootstrap(state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
    let settings = state.settings.lock().map_err(|error| error.to_string())?.clone();
    Ok(BootstrapPayload {
        settings,
        paths: PathPayload {
            data: state.paths.data.to_string_lossy().to_string(),
            database: state.paths.database.to_string_lossy().to_string(),
            resources: state.paths.resources.to_string_lossy().to_string(),
            logs: state.paths.logs.to_string_lossy().to_string(),
        },
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[tauri::command]
pub fn save_settings(settings_value: AppSettings, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    validate_shortcuts(&settings_value)?;
    app_log::info(&state.paths, "settings", "saving settings from UI");
    {
        let mut guard = state.settings.lock().map_err(|error| error.to_string())?;
        *guard = settings_value.clone();
        settings::save(&state.paths, &settings_value)?;
    }
    crate::shortcut::sync_shortcuts(&app, &settings_value.shortcuts)?;
    let _ = crate::tray::refresh_tray(&app);
    // 设置保存后广播给所有弹窗，是为了让已打开的弹窗也能立即跟随主界面深浅主题变化。
    // Broadcasting saved settings lets already-open popups follow main-window theme changes immediately.
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
pub fn set_autostart(enabled: bool, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    app_log::info(&state.paths, "settings", format!("autostart set to {}", enabled));
    autostart::apply(enabled, &state.paths.root)?;
    let updated = update_settings_flag(&state, |settings| settings.auto_start = enabled)?;
    let _ = crate::tray::refresh_tray(&app);
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
pub fn list_history(query: String, kind: String, state: State<'_, AppState>) -> Result<Vec<HistoryRecord>, String> {
    let limit = state.settings.lock().map_err(|error| error.to_string())?.history_limit;
    database::list(&state.paths, &query, &kind, limit)
}

#[tauri::command]
pub fn delete_records(ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    app_log::info(&state.paths, "history", format!("delete requested for {} record(s), preserve favorites", ids.len()));
    let deleted = database::delete(&state.paths, &ids)?;
    cleanup_record_resources(&state, &deleted)
}

#[tauri::command]
pub fn delete_records_force(ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    app_log::warn(&state.paths, "history", format!("force delete requested for {} record(s)", ids.len()));
    let deleted = database::delete_force(&state.paths, &ids)?;
    cleanup_record_resources(&state, &deleted)
}

#[tauri::command]
pub fn clear_all_data(preserve_pinned: bool, state: State<'_, AppState>) -> Result<(), String> {
    app_log::warn(&state.paths, "history", format!("clear all requested; preserve favorites: {}", preserve_pinned));
    let deleted = database::clear(&state.paths, preserve_pinned)?;
    cleanup_record_resources(&state, &deleted)?;
    if !preserve_pinned && state.paths.resources.exists() {
        for entry in fs::read_dir(&state.paths.resources).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_file() {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn delete_history_before_days(days: u32, preserve_pinned: bool, state: State<'_, AppState>) -> Result<usize, String> {
    app_log::warn(&state.paths, "history", format!("delete older than {} day(s); preserve favorites: {}", days, preserve_pinned));
    if days == 0 {
        return Err("Days must be greater than zero".into());
    }
    let deleted = database::delete_older_than(&state.paths, days, preserve_pinned)?;
    let count = deleted.len();
    // 先取回即将删除的记录再清理资源，是为了只删除 ClipAnchor 自己缓存的图片，绝不碰用户原始文件路径。
    // Records are collected before resource cleanup so only ClipAnchor-owned cached images are removed and original user files are never touched.
    cleanup_record_resources(&state, &deleted)?;
    Ok(count)
}

fn cleanup_record_resources(state: &State<'_, AppState>, records: &[HistoryRecord]) -> Result<(), String> {
    for record in records {
        if let Some(path) = record.image_path.as_ref() {
            let path = Path::new(path);
            if path.starts_with(&state.paths.resources) && path.is_file() {
                // 只删除 ClipAnchor 自己生成的资源，避免历史记录清理误删用户原始文件。
                // Only ClipAnchor-owned resources are removed so history cleanup cannot delete a user's original files.
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn toggle_record_pin(id: String, pinned: bool, state: State<'_, AppState>) -> Result<HistoryRecord, String> {
    app_log::info(&state.paths, "history", format!("record favorite changed: {} -> {}", id, pinned));
    database::set_pinned(&state.paths, &id, pinned)
}

#[tauri::command]
pub fn create_text_record(text: String, pinned: bool, state: State<'_, AppState>) -> Result<HistoryRecord, String> {
    app_log::info(&state.paths, "history", format!("manual text record requested; favorite: {}", pinned));
    let normalized = text.trim().to_string();
    if normalized.is_empty() {
        return Err("Text cannot be empty".into());
    }
    let item = ClipItem {
        id: Uuid::new_v4().to_string(),
        kind: ClipKind::Text,
        summary: normalized.chars().take(200).collect(),
        text_content: Some(normalized.clone()),
        image_path: None,
        file_paths: Vec::new(),
        bytes: normalized.as_bytes().len() as i64,
        created_at: Utc::now().to_rfc3339(),
        content_hash: clipboard_service::content_hash_for_bytes("text", normalized.as_bytes()),
        is_pinned: pinned,
    };
    // 新增文本是否收藏由前端工作区决定，是为了让收藏夹内创建的内容立即拥有收藏保护状态。
    // Whether new text is favorited is decided by the active workspace so Favorites-created content is protected immediately.
    database::upsert_text(&state.paths, &item)
}

#[tauri::command]
pub fn update_text_record(id: String, text: String, state: State<'_, AppState>) -> Result<HistoryRecord, String> {
    app_log::info(&state.paths, "history", format!("text record update requested: {}", id));
    let normalized = text.trim().to_string();
    if normalized.is_empty() {
        return Err("Text cannot be empty".into());
    }
    // 只允许编辑文本型记录，是为了避免破坏图片资源路径或文件列表的有效性校验。
    // Only text records are editable so image resource paths and file lists remain valid for integrity checks.
    database::update_text(&state.paths, &id, &normalized)
}

#[tauri::command]
pub fn pin_history_item(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    app_log::info(&state.paths, "popup", format!("pin history item requested: {}", id));
    let record = database::get(&state.paths, &id)?.ok_or_else(|| "Record not found".to_string())?;
    let item = ClipItem {
        id: format!("{}-pinned-{}", record.id, chrono::Utc::now().timestamp_millis()),
        kind: record.kind.clone(),
        summary: record.summary.clone(),
        text_content: record.text_content.clone(),
        image_path: record.image_path.clone(),
        file_paths: record.file_paths.clone(),
        bytes: record.bytes,
        created_at: record.created_at.clone(),
        content_hash: record.content_hash.clone(),
        is_pinned: true,
    };
    // 历史记录置顶先返回前端，再延迟创建新 WebView，是为了避免 invoke 过程和弹窗 WebView 初始化抢同一事件循环导致白屏。
    // History pinning returns to the frontend before creating the WebView so invoke handling and popup initialization do not contend for the same event loop and produce a white window.
    state.temp_items.lock().map_err(|error| error.to_string())?.insert(item.id.clone(), item.clone());
    let settings_snapshot = state.settings.lock().map_err(|error| error.to_string())?.clone();
    let state_snapshot = state.inner().clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(180));
        let _ = popup::create_pinned_popup(&app, &state_snapshot, &item, &settings_snapshot);
    });
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ValidationPayload {
    pub valid: bool,
    pub reason: String,
}

#[tauri::command]
pub fn validate_record(id: String, state: State<'_, AppState>) -> Result<ValidationPayload, String> {
    let Some(record) = database::get(&state.paths, &id)? else {
        return Ok(ValidationPayload { valid: false, reason: "missing".into() });
    };
    if let Some(path) = record.image_path.as_ref() {
        if !Path::new(path).exists() || (!clipboard_service::is_raw_clipanchor_image(path) && image::open(path).is_err()) {
            return Ok(ValidationPayload { valid: false, reason: "image".into() });
        }
    }
    if !record.file_paths.is_empty() && record.file_paths.iter().any(|path| !Path::new(path).exists()) {
        return Ok(ValidationPayload { valid: false, reason: "file".into() });
    }
    Ok(ValidationPayload { valid: true, reason: "ok".into() })
}

#[tauri::command]
pub fn validate_favorites(state: State<'_, AppState>) -> Result<Vec<HistoryRecord>, String> {
    app_log::info(&state.paths, "history", "favorite validity refresh requested");
    let records = database::list(&state.paths, "", "favorite", 0)?;
    let mut invalid = Vec::new();
    for record in records {
        let image_invalid = record.image_path.as_ref().map(|path| !Path::new(path).exists() || (!clipboard_service::is_raw_clipanchor_image(path) && image::open(path).is_err())).unwrap_or(false);
        let file_invalid = !record.file_paths.is_empty() && record.file_paths.iter().any(|path| !Path::new(path).exists());
        if image_invalid || file_invalid {
            invalid.push(record);
        }
    }
    Ok(invalid)
}

#[tauri::command]
pub fn toggle_popup_favorite(id: String, pinned: bool, state: State<'_, AppState>) -> Result<HistoryRecord, String> {
    app_log::info(&state.paths, "popup", format!("popup favorite changed: {} -> {}", id, pinned));
    let source_id = source_record_id(&id);
    database::set_pinned(&state.paths, &source_id, pinned)
}

fn source_record_id(id: &str) -> String {
    id.split("-pinned-").next().unwrap_or(id).to_string()
}

#[tauri::command]
pub fn copy_item(id: String, state: State<'_, AppState>) -> Result<(), String> {
    app_log::info(&state.paths, "clipboard", format!("copy item requested: {}", id));
    if let Some(item) = state.temp_items.lock().map_err(|error| error.to_string())?.get(&id).cloned() {
        let record = HistoryRecord {
            id: item.id,
            kind: item.kind,
            summary: item.summary,
            text_content: item.text_content,
            image_path: item.image_path,
            file_paths: item.file_paths,
            bytes: item.bytes,
            created_at: item.created_at,
            content_hash: item.content_hash,
            is_pinned: item.is_pinned,
        };
        return clipboard_service::copy_to_clipboard(&record);
    }
    let record = database::get(&state.paths, &id)?.ok_or_else(|| "Record not found".to_string())?;
    clipboard_service::copy_to_clipboard(&record)
}

#[tauri::command]
pub fn get_popup_item(id: String, state: State<'_, AppState>) -> Result<ClipItem, String> {
    if let Some(item) = state.temp_items.lock().map_err(|error| error.to_string())?.get(&id).cloned() {
        return Ok(item);
    }
    if let Some(source_id) = id.split("-pinned-").next() {
        if source_id != id {
            if let Some(record) = database::get(&state.paths, source_id)? {
                // 历史记录弹窗优先读临时缓存；若 WebView 加载晚于缓存写入可见性，则退回数据库重建，避免弹窗卡在加载态。
                // History popups prefer the temp cache; if WebView loading races cache visibility, the database fallback rebuilds the item instead of leaving the popup stuck.
                return Ok(ClipItem {
                    id,
                    kind: record.kind,
                    summary: record.summary,
                    text_content: record.text_content,
                    image_path: record.image_path,
                    file_paths: record.file_paths,
                    bytes: record.bytes,
                    created_at: record.created_at,
                    content_hash: record.content_hash,
                    is_pinned: true,
                });
            }
        }
    }
    Err("Popup item not found".to_string())
}

#[tauri::command]
pub fn read_image_data_url(id: String, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let image_path = if let Some(item) = state.temp_items.lock().map_err(|error| error.to_string())?.get(&id).cloned() {
        item.image_path
    } else {
        database::get(&state.paths, &id)?.and_then(|record| record.image_path)
    };

    let Some(path) = image_path else {
        return Ok(None);
    };
    let preview_path = cached_preview_path(&path);
    let bytes = if preview_path.exists() {
        fs::read(&preview_path).map_err(|error| error.to_string())?
    } else {
        clipboard_service::thumbnail_bytes_for_path(&path, 420, 260)?
    };
    // 弹窗与历史缩略图只返回小尺寸预览，是为了避免大图首次复制时通过 WebView 传输完整 base64 导致界面卡死。
    // Popup and history thumbnails return only a small preview so first-time large-image copies do not freeze the UI with full base64 transfer.
    Ok(Some(format!("data:image/png;base64,{}", general_purpose::STANDARD.encode(bytes))))
}

fn cached_preview_path(path: &str) -> std::path::PathBuf {
    let source = Path::new(path);
    let parent = source.parent().unwrap_or_else(|| Path::new(""));
    let stem = source.file_stem().and_then(|value| value.to_str()).unwrap_or("preview");
    parent.join(format!("{}-thumb.png", stem))
}

#[derive(serde::Serialize)]
pub struct FilePreviewPayload {
    pub name: String,
    pub path: String,
    pub is_image: bool,
    pub thumbnail_data_url: Option<String>,
}

#[tauri::command]
pub fn read_file_previews(id: String, state: State<'_, AppState>) -> Result<Vec<FilePreviewPayload>, String> {
    let file_paths = if let Some(item) = state.temp_items.lock().map_err(|error| error.to_string())?.get(&id).cloned() {
        item.file_paths
    } else {
        database::get(&state.paths, &id)?.map(|record| record.file_paths).unwrap_or_default()
    };
    let mut previews = Vec::new();
    for path in file_paths.iter() {
        // 文件复制不应有人为展示上限；前端用滚动区域承载完整列表，避免用户复制大量文件时误以为内容丢失。
        // File copies should not have an artificial preview limit; the frontend uses a scrollable area so large selections never look truncated.
        let name = Path::new(path).file_name().and_then(|value| value.to_str()).unwrap_or(path).to_string();
        let is_image = clipboard_service::is_image_path(path);
        // 文件类弹窗只返回文件名和类型，不即时解码图片缩略图，避免复制照片文件时阻塞弹窗加载。
        // File popups return only names and type flags without decoding thumbnails, preventing photo-file copies from blocking popup loading.
        previews.push(FilePreviewPayload { name, path: path.clone(), is_image, thumbnail_data_url: None });
    }
    // 文件预览只返回文件名和缩略图，是为了让弹窗像剪贴板对象而不是路径文本列表。
    // File previews return names and thumbnails only so popups feel like clipboard objects rather than path text lists.
    Ok(previews)
}


#[tauri::command]
pub fn close_popup(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    app_log::info(&state.paths, "popup", format!("close popup requested: {}", id));
    state.temp_items.lock().map_err(|error| error.to_string())?.remove(&id);
    popup::close_popup(&app, &id)
}

#[tauri::command]
pub fn pin_popup(id: String, app: AppHandle) -> Result<(), String> {
    if let Some(state) = app.try_state::<AppState>() { app_log::info(&state.paths, "popup", format!("pin popup requested: {}", id)); }
    popup::pin_popup(&app, &id)
}

#[tauri::command]
pub fn resize_popup(id: String, width: f64, height: f64, app: AppHandle) -> Result<(), String> {
    if let Some(state) = app.try_state::<AppState>() { app_log::info(&state.paths, "popup", format!("resize popup requested: {} -> {:.0}x{:.0}", id, width, height)); }
    popup::resize_popup(&app, &id, width, height)
}

#[tauri::command]
pub fn refresh_popup_shape(id: String, app: AppHandle) -> Result<(), String> {
    popup::refresh_popup_shape(&app, &id)
}

#[tauri::command]
pub fn save_popup_position(x: f64, y: f64, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    app_log::info(&state.paths, "settings", format!("popup default position saved: {:.0},{:.0}", x, y));
    // 保存的是默认弹出锚点，不写入每个已钉住窗口的位置，避免用户拖动历史弹窗时污染默认位置。
    // Only the default popup anchor is saved, so dragging pinned popups will not pollute the preferred spawn point.
    popup::save_position(&app, &state, x, y)
}

#[tauri::command]
pub fn open_position_overlay(app: AppHandle) -> Result<(), String> {
    popup::open_position_overlay(&app)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct HistoryExportPayload {
    schema: String,
    exported_at: String,
    records: Vec<HistoryRecord>,
}

const HISTORY_CSV_HEADERS: [&str; 10] = [
    "id",
    "kind",
    "summary",
    "text_content",
    "image_path",
    "file_paths",
    "bytes",
    "created_at",
    "content_hash",
    "is_pinned",
];

#[tauri::command]
pub fn export_history(state: State<'_, AppState>) -> Result<String, String> {
    let output = state.paths.exports.join("clipanchor-history.json");
    export_history_to_path("json".into(), output.to_string_lossy().to_string(), state)
}

#[tauri::command]
pub fn import_history(state: State<'_, AppState>) -> Result<String, String> {
    let input = state.paths.exports.join("clipanchor-history.json");
    if !input.exists() {
        return Err("Choose a JSON or CSV history file before importing".into());
    }
    import_history_from_path("json".into(), input.to_string_lossy().to_string(), state)
}

#[tauri::command]
pub fn export_history_to_path(format: String, output_path: String, state: State<'_, AppState>) -> Result<String, String> {
    app_log::info(&state.paths, "data", format!("history export requested: {} -> {}", format, output_path));
    let records = database::list(&state.paths, "", "all", 0)?;
    let path = Path::new(&output_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }
    match format.as_str() {
        "csv" => export_csv_history(path, &records)?,
        "json" | _ => {
            let payload = HistoryExportPayload {
                schema: "clipanchor.history".into(),
                exported_at: Utc::now().to_rfc3339(),
                records,
            };
            let json = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
            fs::write(path, json).map_err(|error| error.to_string())?;
        }
    }
    Ok(path.to_string_lossy().to_string())
}

fn export_csv_history(path: &Path, records: &[HistoryRecord]) -> Result<(), String> {
    let mut file = fs::File::create(path).map_err(|error| error.to_string())?;
    write_csv_row(&mut file, &HISTORY_CSV_HEADERS.iter().map(|value| value.to_string()).collect::<Vec<_>>())?;
    for record in records {
        // CSV 导出使用与 JSON 对等的字段，是为了让表格软件可读的同时不丢失收藏、类型、时间和资源路径等属性。
        // CSV export uses fields equivalent to JSON so spreadsheet-friendly files do not lose favorites, types, timestamps, or resource paths.
        let file_paths = serde_json::to_string(&record.file_paths).map_err(|error| error.to_string())?;
        write_csv_row(&mut file, &[
            record.id.clone(),
            kind_to_export_value(&record.kind).to_string(),
            record.summary.clone(),
            record.text_content.clone().unwrap_or_default(),
            record.image_path.clone().unwrap_or_default(),
            file_paths,
            record.bytes.to_string(),
            record.created_at.clone(),
            record.content_hash.clone(),
            record.is_pinned.to_string(),
        ])?;
    }
    Ok(())
}

fn write_csv_row(file: &mut fs::File, values: &[String]) -> Result<(), String> {
    let line = values.iter().map(|value| csv_escape(value)).collect::<Vec<_>>().join(",");
    file.write_all(line.as_bytes()).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}

#[tauri::command]
pub fn import_history_from_path(format: String, input_path: String, state: State<'_, AppState>) -> Result<String, String> {
    app_log::info(&state.paths, "data", format!("history import requested: {} <- {}", format, input_path));
    let path = Path::new(&input_path);
    if !path.exists() {
        return Err("Selected history file does not exist".into());
    }
    match format.as_str() {
        "csv" => import_csv_history(path, &state),
        "json" | _ => import_json_history(path, &state),
    }
}

fn import_json_history(path: &Path, state: &State<'_, AppState>) -> Result<String, String> {
    let mut text = String::new();
    fs::File::open(path).map_err(|error| error.to_string())?.read_to_string(&mut text).map_err(|error| error.to_string())?;
    let records = match serde_json::from_str::<HistoryExportPayload>(&text) {
        Ok(payload) => payload.records,
        Err(_) => {
            let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
            if let Some(records_value) = value.get("records") {
                serde_json::from_value::<Vec<HistoryRecord>>(records_value.clone()).map_err(|error| error.to_string())?
            } else {
                serde_json::from_value::<Vec<HistoryRecord>>(value).map_err(|error| error.to_string())?
            }
        }
    };
    let count = records.len();
    for record in records {
        // JSON 导入保留完整类型、资源路径、固定状态和内容哈希，这样完整备份可以恢复为原来的历史对象。
        // JSON import preserves kind, resource paths, pinned state, and content hash so full backups restore original history objects.
        let item = ClipItem {
            id: if record.id.trim().is_empty() { Uuid::new_v4().to_string() } else { record.id },
            kind: record.kind,
            summary: record.summary,
            text_content: record.text_content,
            image_path: record.image_path,
            file_paths: record.file_paths,
            bytes: record.bytes,
            created_at: record.created_at,
            content_hash: record.content_hash,
            is_pinned: record.is_pinned,
        };
        database::insert(&state.paths, &item)?;
    }
    Ok(format!("Imported {} record(s)", count))
}

fn import_csv_history(path: &Path, state: &State<'_, AppState>) -> Result<String, String> {
    let mut raw = String::new();
    fs::File::open(path).map_err(|error| error.to_string())?.read_to_string(&mut raw).map_err(|error| error.to_string())?;
    let rows = parse_csv_rows(&raw);
    if rows.is_empty() {
        return Ok("Imported 0 record(s)".into());
    }
    let headers = rows.first().cloned().unwrap_or_default();
    if is_full_history_csv(&headers) {
        import_full_csv_rows(rows, state)
    } else {
        import_legacy_text_csv_rows(rows, state)
    }
}

fn import_full_csv_rows(rows: Vec<Vec<String>>, state: &State<'_, AppState>) -> Result<String, String> {
    let header_map = csv_header_map(rows.first().map(|row| row.as_slice()).unwrap_or(&[]));
    let mut count = 0usize;
    for row in rows.into_iter().skip(1) {
        let kind = export_value_to_kind(&csv_cell(&row, &header_map, "kind"));
        let text_content = none_if_blank(csv_cell(&row, &header_map, "text_content"));
        let image_path = none_if_blank(csv_cell(&row, &header_map, "image_path"));
        let file_paths = parse_csv_file_paths(&csv_cell(&row, &header_map, "file_paths"));
        let summary = csv_summary(&row, &header_map, &kind, text_content.as_deref(), image_path.as_deref(), &file_paths);
        if summary.trim().is_empty() && text_content.is_none() && image_path.is_none() && file_paths.is_empty() {
            continue;
        }
        let bytes = csv_cell(&row, &header_map, "bytes")
            .trim()
            .parse::<i64>()
            .unwrap_or_else(|_| inferred_record_bytes(text_content.as_deref(), image_path.as_deref(), &file_paths));
        let content_hash = csv_content_hash(&kind, text_content.as_deref(), image_path.as_deref(), &file_paths, &csv_cell(&row, &header_map, "content_hash"));
        let item = ClipItem {
            id: non_empty_or_uuid(csv_cell(&row, &header_map, "id")),
            kind,
            summary,
            text_content,
            image_path,
            file_paths,
            bytes,
            created_at: non_empty_or_now(csv_cell(&row, &header_map, "created_at")),
            content_hash,
            is_pinned: csv_bool(&csv_cell(&row, &header_map, "is_pinned")),
        };
        // CSV 导入按完整字段恢复记录，是为了让用户在表格中审阅或编辑后仍能恢复收藏状态和资源引用。
        // CSV import restores full fields so users can review or edit the spreadsheet and still keep favorite state and resource references.
        database::insert(&state.paths, &item)?;
        count += 1;
    }
    Ok(format!("Imported {} record(s)", count))
}

fn import_legacy_text_csv_rows(rows: Vec<Vec<String>>, state: &State<'_, AppState>) -> Result<String, String> {
    let mut count = 0usize;
    for (index, row) in rows.into_iter().enumerate() {
        let value = row.first().cloned().unwrap_or_default();
        if index == 0 && value.trim().eq_ignore_ascii_case("text") {
            continue;
        }
        let normalized = value.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        let item = ClipItem {
            id: Uuid::new_v4().to_string(),
            kind: ClipKind::Text,
            summary: normalized.chars().take(200).collect(),
            text_content: Some(normalized.clone()),
            image_path: None,
            file_paths: Vec::new(),
            bytes: normalized.as_bytes().len() as i64,
            created_at: Utc::now().to_rfc3339(),
            content_hash: clipboard_service::content_hash_for_bytes("text", normalized.as_bytes()),
            is_pinned: false,
        };
        // 旧版单列 CSV 继续按文本导入，是为了兼容用户已经导出的旧文件，不因格式升级而丢失可导入性。
        // Older single-column CSV files still import as text so existing exports remain usable after the format upgrade.
        database::insert(&state.paths, &item)?;
        count += 1;
    }
    Ok(format!("Imported {} text record(s)", count))
}

fn kind_to_export_value(kind: &ClipKind) -> &'static str {
    match kind {
        ClipKind::Text => "text",
        ClipKind::Image => "image",
        ClipKind::File => "file",
        ClipKind::Mixed => "mixed",
    }
}

fn export_value_to_kind(value: &str) -> ClipKind {
    match value.trim().to_lowercase().as_str() {
        "image" => ClipKind::Image,
        "file" => ClipKind::File,
        "mixed" => ClipKind::Mixed,
        _ => ClipKind::Text,
    }
}

fn is_full_history_csv(headers: &[String]) -> bool {
    let normalized = headers.iter().map(|value| value.trim().to_lowercase()).collect::<Vec<_>>();
    HISTORY_CSV_HEADERS.iter().all(|header| normalized.iter().any(|value| value == header))
}

fn csv_header_map(headers: &[String]) -> HashMap<String, usize> {
    headers
        .iter()
        .enumerate()
        .map(|(index, value)| (value.trim().to_lowercase(), index))
        .collect()
}

fn csv_cell(row: &[String], header_map: &HashMap<String, usize>, key: &str) -> String {
    header_map.get(key).and_then(|index| row.get(*index)).cloned().unwrap_or_default()
}

fn csv_summary(row: &[String], header_map: &HashMap<String, usize>, kind: &ClipKind, text_content: Option<&str>, image_path: Option<&str>, file_paths: &[String]) -> String {
    let summary = csv_cell(row, header_map, "summary");
    if !summary.trim().is_empty() {
        return summary;
    }
    match kind {
        ClipKind::Text => text_content.unwrap_or_default().chars().take(200).collect(),
        ClipKind::Image => image_path.and_then(|path| Path::new(path).file_name().and_then(|name| name.to_str())).unwrap_or("Image").to_string(),
        ClipKind::File | ClipKind::Mixed => file_paths.first().and_then(|path| Path::new(path).file_name().and_then(|name| name.to_str())).unwrap_or("Files").to_string(),
    }
}

fn csv_content_hash(kind: &ClipKind, text_content: Option<&str>, image_path: Option<&str>, file_paths: &[String], provided: &str) -> String {
    let trimmed = provided.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    match kind {
        ClipKind::Text => clipboard_service::content_hash_for_bytes("text", text_content.unwrap_or_default().as_bytes()),
        ClipKind::File | ClipKind::Mixed => clipboard_service::content_hash_for_paths(file_paths),
        ClipKind::Image => clipboard_service::content_hash_for_bytes("image", image_path.unwrap_or_default().as_bytes()),
    }
}

fn parse_csv_file_paths(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<String>>(trimmed).unwrap_or_else(|_| {
        trimmed
            .split(';')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect()
    })
}

fn inferred_record_bytes(text_content: Option<&str>, image_path: Option<&str>, file_paths: &[String]) -> i64 {
    if let Some(text) = text_content {
        return text.as_bytes().len() as i64;
    }
    if let Some(path) = image_path {
        return fs::metadata(path).map(|metadata| metadata.len() as i64).unwrap_or(0);
    }
    file_paths.iter().filter_map(|path| fs::metadata(path).ok().map(|metadata| metadata.len() as i64)).sum()
}

fn none_if_blank(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() { None } else { Some(value) }
}

fn non_empty_or_uuid(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() { Uuid::new_v4().to_string() } else { trimmed.to_string() }
}

fn non_empty_or_now(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() { Utc::now().to_rfc3339() } else { trimmed.to_string() }
}

fn csv_bool(value: &str) -> bool {
    matches!(value.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "y")
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn parse_csv_rows(raw: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                let _ = chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                row.push(field.clone());
                field.clear();
            }
            '\n' if !in_quotes => {
                row.push(field.trim_end_matches('\r').to_string());
                field.clear();
                if !(row.len() == 1 && row[0].trim().is_empty()) {
                    rows.push(row.clone());
                }
                row.clear();
            }
            _ => field.push(ch),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field.trim_end_matches('\r').to_string());
        if !(row.len() == 1 && row[0].trim().is_empty()) {
            rows.push(row);
        }
    }
    rows
}

#[derive(serde::Serialize)]
pub struct DataUsagePayload {
    pub bytes: u64,
    pub display: String,
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

fn open_path_with_system(path: &Path) -> Result<(), String> {
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

fn validate_shortcuts(settings_value: &AppSettings) -> Result<(), String> {
    let shortcuts = [
        &settings_value.shortcuts.toggle_pin_service,
        &settings_value.shortcuts.toggle_history_service,
        &settings_value.shortcuts.toggle_main_window,
        &settings_value.shortcuts.enter_light_mode,
        &settings_value.shortcuts.toggle_theme_mode,
    ];
    for (index, shortcut) in shortcuts.iter().enumerate() {
        if shortcuts.iter().skip(index + 1).any(|other| *other == *shortcut) {
            return Err(format!("Shortcut conflict: {}", shortcut));
        }
    }
    Ok(())
}
