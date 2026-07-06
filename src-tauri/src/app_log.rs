use crate::{paths::DataPaths, settings};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::{fs::{self, OpenOptions}, io::Write, path::{Path, PathBuf}};

const CURRENT_LOG_NAME: &str = "clipanchor.log";
const MAX_CURRENT_LOG_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_LOG_RETENTION_DAYS: u32 = 7;
const MAX_ARCHIVE_FILES: usize = 20;

#[derive(Clone, Debug, Serialize)]
pub struct LogFileInfo {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub display_size: String,
    pub modified_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LogStatusPayload {
    pub directory: String,
    pub current_file: String,
    pub bytes: u64,
    pub display_size: String,
    pub files: Vec<LogFileInfo>,
    pub max_current_file_mb: u64,
    pub retention_days: u32,
    pub max_archive_files: usize,
}

pub fn init(paths: &DataPaths) -> Result<(), String> {
    fs::create_dir_all(&paths.logs).map_err(|error| error.to_string())?;
    rotate_if_needed(paths)?;
    cleanup_archives(paths, configured_retention_days(paths))?;
    info(paths, "startup", "logger initialized");
    Ok(())
}

pub fn info(paths: &DataPaths, area: &str, message: impl AsRef<str>) {
    write(paths, "INFO", area, message.as_ref());
}

pub fn warn(paths: &DataPaths, area: &str, message: impl AsRef<str>) {
    write(paths, "WARN", area, message.as_ref());
}

pub fn error(paths: &DataPaths, area: &str, message: impl AsRef<str>) {
    write(paths, "ERROR", area, message.as_ref());
}

pub fn status(paths: &DataPaths) -> Result<LogStatusPayload, String> {
    fs::create_dir_all(&paths.logs).map_err(|error| error.to_string())?;
    rotate_if_needed(paths)?;
    let retention_days = configured_retention_days(paths);
    cleanup_archives(paths, retention_days)?;
    let files = list_log_files(paths)?;
    let bytes = files.iter().map(|file| file.bytes).sum();
    Ok(LogStatusPayload {
        directory: paths.logs.to_string_lossy().to_string(),
        current_file: current_log_path(paths).to_string_lossy().to_string(),
        bytes,
        display_size: human_size(bytes),
        files,
        max_current_file_mb: MAX_CURRENT_LOG_BYTES / 1024 / 1024,
        retention_days,
        max_archive_files: MAX_ARCHIVE_FILES,
    })
}

pub fn clear(paths: &DataPaths) -> Result<usize, String> {
    fs::create_dir_all(&paths.logs).map_err(|error| error.to_string())?;
    let mut removed = 0usize;
    for entry in fs::read_dir(&paths.logs).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if is_log_file(&path) && path.is_file() {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
            removed += 1;
        }
    }
    info(paths, "log", format!("cleared {} log file(s)", removed));
    Ok(removed)
}

fn write(paths: &DataPaths, level: &str, area: &str, message: &str) {
    if fs::create_dir_all(&paths.logs).is_err() {
        return;
    }
    let _ = rotate_if_needed(paths);
    let line = format!(
        "{} [{}] [{}] {}\n",
        Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        level,
        sanitize_area(area),
        sanitize_message(message)
    );
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(current_log_path(paths)) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn rotate_if_needed(paths: &DataPaths) -> Result<(), String> {
    let current = current_log_path(paths);
    if !current.exists() {
        return Ok(());
    }
    let size = current.metadata().map_err(|error| error.to_string())?.len();
    if size < MAX_CURRENT_LOG_BYTES {
        return Ok(());
    }
    let archive = archive_path(paths);
    fs::rename(&current, archive).map_err(|error| error.to_string())?;
    cleanup_archives(paths, configured_retention_days(paths))
}

fn cleanup_archives(paths: &DataPaths, retention_days: u32) -> Result<(), String> {
    if !paths.logs.exists() {
        return Ok(());
    }
    let cutoff = Utc::now() - Duration::days(i64::from(retention_days));
    let mut archives = archive_log_paths(paths)?;
    for path in archives.iter() {
        if let Ok(modified) = modified_utc(path) {
            if modified < cutoff {
                let _ = fs::remove_file(path);
            }
        }
    }
    archives = archive_log_paths(paths)?;
    archives.sort_by_key(|path| modified_utc(path).ok());
    while archives.len() > MAX_ARCHIVE_FILES {
        if let Some(path) = archives.first().cloned() {
            let _ = fs::remove_file(&path);
            archives.remove(0);
        } else {
            break;
        }
    }
    Ok(())
}

fn configured_retention_days(paths: &DataPaths) -> u32 {
    // 日志保留天数从用户设置读取，是为了让维护策略可控，同时避免日志在便携目录中长期堆积。
    // The log retention days come from user settings so maintenance is controllable and logs do not pile up in the portable data directory.
    settings::load(paths)
        .map(|value| value.log_retention_days.clamp(1, 90))
        .unwrap_or(DEFAULT_LOG_RETENTION_DAYS)
}

fn list_log_files(paths: &DataPaths) -> Result<Vec<LogFileInfo>, String> {
    let mut files = Vec::new();
    if !paths.logs.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(&paths.logs).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if !is_log_file(&path) || !path.is_file() {
            continue;
        }
        let metadata = path.metadata().map_err(|error| error.to_string())?;
        let modified_at = modified_utc(&path)
            .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            .unwrap_or_default();
        files.push(LogFileInfo {
            name: path.file_name().and_then(|value| value.to_str()).unwrap_or("clipanchor.log").to_string(),
            path: path.to_string_lossy().to_string(),
            bytes: metadata.len(),
            display_size: human_size(metadata.len()),
            modified_at,
        });
    }
    files.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(files)
}

fn archive_log_paths(paths: &DataPaths) -> Result<Vec<PathBuf>, String> {
    let mut archives = Vec::new();
    for entry in fs::read_dir(&paths.logs).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if is_archive_log_file(&path) && path.is_file() {
            archives.push(path);
        }
    }
    Ok(archives)
}

fn current_log_path(paths: &DataPaths) -> PathBuf {
    paths.logs.join(CURRENT_LOG_NAME)
}

fn archive_path(paths: &DataPaths) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let mut candidate = paths.logs.join(format!("clipanchor-{}.log", stamp));
    let mut suffix = 1usize;
    while candidate.exists() {
        candidate = paths.logs.join(format!("clipanchor-{}-{}.log", stamp, suffix));
        suffix += 1;
    }
    candidate
}

fn is_log_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|name| name == CURRENT_LOG_NAME || (name.starts_with("clipanchor-") && name.ends_with(".log")))
        .unwrap_or(false)
}

fn is_archive_log_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|name| name.starts_with("clipanchor-") && name.ends_with(".log"))
        .unwrap_or(false)
}

fn modified_utc(path: &Path) -> Result<DateTime<Utc>, String> {
    let modified = path.metadata().map_err(|error| error.to_string())?.modified().map_err(|error| error.to_string())?;
    Ok(DateTime::<Utc>::from(modified))
}

fn sanitize_area(area: &str) -> String {
    area.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        .take(40)
        .collect::<String>()
}

fn sanitize_message(message: &str) -> String {
    message.replace('\r', " ").replace('\n', " ").chars().take(1200).collect()
}

fn human_size(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value > 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", value, units[unit])
}
