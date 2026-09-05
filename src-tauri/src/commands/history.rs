use crate::{app_log, clipboard_service, database, fs_guard, models::{AppState, ClipItem, ClipKind, HistoryRecord}, popup};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use std::{collections::HashMap, fs, io::{Read, Write}, path::{Path, PathBuf}, thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

#[tauri::command]
pub fn list_history(query: String, kind: String, state: State<'_, AppState>) -> Result<Vec<HistoryRecord>, String> {
    let limit = state.settings.lock().map_err(|error| error.to_string())?.history_limit;
    database::list(&state.paths, &query, &kind, limit)
}

#[tauri::command]
pub fn delete_records(ids: Vec<String>, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    app_log::info(&state.paths, "history", format!("delete requested for {} record(s), preserve favorites", ids.len()));
    let deleted = database::delete(&state.paths, &ids)?;
    cleanup_record_resources(&state, &deleted)?;
    emit_history_updated(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_records_force(ids: Vec<String>, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    app_log::warn(&state.paths, "history", format!("force delete requested for {} record(s)", ids.len()));
    let deleted = database::delete_force(&state.paths, &ids)?;
    cleanup_record_resources(&state, &deleted)?;
    emit_history_updated(&app);
    Ok(())
}

#[tauri::command]
pub fn clear_all_data(preserve_pinned: bool, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
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
    emit_history_updated(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_history_before_days(days: u32, preserve_pinned: bool, app: AppHandle, state: State<'_, AppState>) -> Result<usize, String> {
    app_log::warn(&state.paths, "history", format!("delete older than {} day(s); preserve favorites: {}", days, preserve_pinned));
    if days == 0 {
        return Err("Days must be greater than zero".into());
    }
    let deleted = database::delete_older_than(&state.paths, days, preserve_pinned)?;
    let count = deleted.len();
    // 先取回即将删除的记录再清理资源，是为了只删除 ClipAnchor 自己缓存的图片，绝不碰用户原始文件路径。
    // Records are collected before resource cleanup so only ClipAnchor-owned cached images are removed and original user files are never touched.
    cleanup_record_resources(&state, &deleted)?;
    emit_history_updated(&app);
    Ok(count)
}

fn emit_history_updated(app: &AppHandle) {
    let _ = app.emit("history-updated", ());
}

fn cleanup_record_resources(state: &State<'_, AppState>, records: &[HistoryRecord]) -> Result<(), String> {
    for record in records {
        if let Some(path) = record.image_path.as_ref() {
            let path = Path::new(path);
            if let Ok(canonical) = fs_guard::assert_resource_file(&state.paths, path) {
                if canonical.is_file() {
                    // 只删除 ClipAnchor 自己生成的资源，避免历史记录清理误删用户原始文件。
                    // Only ClipAnchor-owned resources are removed so history cleanup cannot delete a user's original files.
                    fs::remove_file(canonical).map_err(|error| error.to_string())?;
                }
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

#[derive(serde::Serialize)]
pub struct PopupItemPayload {
    #[serde(flatten)]
    pub item: ClipItem,
    pub is_favorited: bool,
}

fn popup_is_favorited(state: &State<'_, AppState>, id: &str) -> Result<bool, String> {
    Ok(database::get(&state.paths, &source_record_id(id))?.map(|record| record.is_pinned).unwrap_or(false))
}

#[tauri::command]
pub fn get_popup_item(id: String, state: State<'_, AppState>) -> Result<PopupItemPayload, String> {
    if let Some(item) = state.temp_items.lock().map_err(|error| error.to_string())?.get(&id).cloned() {
        let is_favorited = popup_is_favorited(&state, &id)?;
        return Ok(PopupItemPayload { item, is_favorited });
    }
    if let Some(source_id) = id.split("-pinned-").next() {
        if source_id != id {
            if let Some(record) = database::get(&state.paths, source_id)? {
                // 历史记录弹窗优先读临时缓存；若 WebView 加载晚于缓存写入可见性，则退回数据库重建，避免弹窗卡在加载态。
                // History popups prefer the temp cache; if WebView loading races cache visibility, the database fallback rebuilds the item instead of leaving the popup stuck.
                let is_favorited = record.is_pinned;
                return Ok(PopupItemPayload {
                    item: ClipItem {
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
                    },
                    is_favorited,
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
    let source = Path::new(&path);
    let allowed = fs_guard::assert_resource_file(&state.paths, source)?;
    let preview_path = cached_preview_path(&allowed.to_string_lossy());
    let bytes = if preview_path.exists() {
        let preview = fs_guard::assert_resource_file(&state.paths, &preview_path)?;
        fs::read(&preview).map_err(|error| error.to_string())?
    } else {
        clipboard_service::thumbnail_bytes_for_path(&allowed.to_string_lossy(), 420, 260)?
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
    if let Some(state) = app.try_state::<AppState>() {
        app_log::info(&state.paths, "popup", format!("pin popup requested: {}", id));
        if let Ok(mut items) = state.temp_items.lock() {
            if let Some(item) = items.get_mut(&id) {
                // 后端也记录弹窗置顶状态，是为了重复复制时能保留已 Pin 窗口，而不是把它误当成普通临时弹窗关闭。
                // The backend also records popup pin state so duplicate copies keep an already pinned window instead of treating it as a disposable transient popup.
                item.is_pinned = true;
            }
        }
    }
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

fn history_format_kind(format: &str) -> &'static str {
    if format.trim().eq_ignore_ascii_case("csv") {
        "csv"
    } else {
        "json"
    }
}

fn picked_dialog_path(picked: Option<tauri_plugin_dialog::FilePath>) -> Result<Option<PathBuf>, String> {
    match picked {
        Some(file) => file.into_path().map(Some).map_err(|error| error.to_string()),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn export_history(format: String, app: AppHandle, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let kind = history_format_kind(&format);
    let filter_name = if kind == "csv" {
        "ClipAnchor CSV history"
    } else {
        "ClipAnchor JSON history"
    };
    // 导出路径只来自系统保存对话框，是为了禁止前端传入任意磁盘路径并覆盖用户文件。
    // Export paths come only from the native save dialog so the frontend cannot pass an arbitrary disk path and overwrite user files.
    let picked = app
        .dialog()
        .file()
        .set_title(filter_name)
        .set_file_name(&format!("clipanchor-history.{}", kind))
        .add_filter(filter_name, &[kind])
        .blocking_save_file();
    let Some(path) = picked_dialog_path(picked)? else {
        return Ok(None);
    };
    Ok(Some(write_history_export(kind, &path, &state)?))
}

#[tauri::command]
pub async fn import_history(format: String, app: AppHandle, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let kind = history_format_kind(&format);
    let filter_name = if kind == "csv" {
        "ClipAnchor CSV history"
    } else {
        "ClipAnchor JSON history"
    };
    // 导入同样只接受对话框选中的文件，避免任意路径被当成历史备份读取。
    // Import likewise accepts only a dialog-chosen file so arbitrary paths cannot be read as history backups.
    let picked = app
        .dialog()
        .file()
        .set_title(filter_name)
        .add_filter(filter_name, &[kind])
        .blocking_pick_file();
    let Some(path) = picked_dialog_path(picked)? else {
        return Ok(None);
    };
    Ok(Some({
        let imported = import_history_from_file(kind, &path, &state)?;
        emit_history_updated(&app);
        imported
    }))
}

fn write_history_export(format: &str, path: &Path, state: &State<'_, AppState>) -> Result<String, String> {
    app_log::info(&state.paths, "data", format!("history export requested: {} -> {}", format, path.display()));
    fs_guard::assert_history_export_path(path, format)?;
    let records = database::list(&state.paths, "", "all", 0)?;
    match format {
        "csv" => export_csv_history(path, &records)?,
        _ => {
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

fn import_history_from_file(format: &str, path: &Path, state: &State<'_, AppState>) -> Result<String, String> {
    app_log::info(&state.paths, "data", format!("history import requested: {} <- {}", format, path.display()));
    fs_guard::assert_history_import_file(path, format)?;
    match format {
        "csv" => import_csv_history(path, state),
        _ => import_json_history(path, state),
    }
}

fn import_image_into_resources(state: &State<'_, AppState>, image_path: Option<String>) -> Result<Option<String>, String> {
    let Some(source) = image_path.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let source_path = Path::new(&source);
    if !source_path.is_file() {
        return Ok(None);
    }
    if let Ok(existing) = fs_guard::assert_resource_file(&state.paths, source_path) {
        return Ok(Some(existing.to_string_lossy().to_string()));
    }
    fs::create_dir_all(&state.paths.resources).map_err(|error| error.to_string())?;
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bin");
    let destination = state.paths.resources.join(format!("{}.{}", Uuid::new_v4(), extension));
    fs::copy(source_path, &destination).map_err(|error| error.to_string())?;
    Ok(Some(destination.to_string_lossy().to_string()))
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
    fs_guard::assert_import_record_count(count)?;
    for record in records {
        // JSON 导入把图片拷进 data/resources，是为了拒绝任意磁盘路径作为历史资源被后续读取。
        // JSON import copies images into data/resources so arbitrary disk paths cannot become readable history resources later.
        let image_path = import_image_into_resources(state, record.image_path)?;
        let item = ClipItem {
            id: if record.id.trim().is_empty() { Uuid::new_v4().to_string() } else { record.id },
            kind: record.kind,
            summary: record.summary,
            text_content: record.text_content,
            image_path,
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
    fs_guard::assert_import_record_count(rows.len().saturating_sub(1))?;
    let header_map = csv_header_map(rows.first().map(|row| row.as_slice()).unwrap_or(&[]));
    let mut count = 0usize;
    for row in rows.into_iter().skip(1) {
        let kind = export_value_to_kind(&csv_cell(&row, &header_map, "kind"));
        let text_content = none_if_blank(csv_cell(&row, &header_map, "text_content"));
        let image_path = import_image_into_resources(state, none_if_blank(csv_cell(&row, &header_map, "image_path")))?;
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
    fs_guard::assert_import_record_count(rows.len())?;
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
