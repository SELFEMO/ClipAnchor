use crate::{app_log, database, models::{AppState, AppSettings, ClipItem, ClipKind, HistoryRecord}, popup};
use arboard::{Clipboard, ImageData};
use chrono::Utc;
use image::{ImageBuffer, Rgba};
use std::{borrow::Cow, fs, io::Cursor, mem, panic::{catch_unwind, AssertUnwindSafe}, path::Path, ptr, sync::{atomic::{AtomicBool, Ordering}, Arc}, thread, time::{Duration, SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[cfg(target_os = "windows")]
use std::{ffi::OsString, os::windows::ffi::{OsStrExt, OsStringExt}, ptr::null_mut};
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData},
        Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE},
    },
    UI::Shell::DragQueryFileW,
};

#[cfg(target_os = "windows")]
// CF_HDROP 在 windows-sys 0.59 中没有从 DataExchange 模块导出，直接使用 Win32 固定格式编号可以避免依赖版本差异导致编译失败。
// CF_HDROP is not exported from the DataExchange module in windows-sys 0.59, so using the stable Win32 format id avoids compile failures across dependency versions.
const CF_HDROP: u32 = 15;

#[cfg(target_os = "windows")]
// CF_UNICODETEXT 同样使用 Win32 固定格式编号，是为了在 arboard 无法读取长文本时仍能从系统剪贴板读取完整 Unicode 文本。
// CF_UNICODETEXT also uses the stable Win32 format id so long Unicode text can still be read when arboard fails to parse it.
const CF_UNICODETEXT: u32 = 13;

#[cfg(target_os = "windows")]
// 部分旧程序或特殊富文本来源只提供 ANSI/OEM 文本格式；补充读取这些格式可以避免长文本被系统剪贴板记录但 ClipAnchor 漏掉。
// Some legacy apps or rich-text sources expose only ANSI/OEM text formats; reading them prevents ClipAnchor from missing text that the system clipboard still has.
const CF_TEXT: u32 = 1;

#[cfg(target_os = "windows")]
// OEM 文本是 Windows 剪贴板的兼容格式之一，作为最后兜底能覆盖少数非 Unicode 来源。
// OEM text is one of Windows clipboard compatibility formats, so it is kept as a final fallback for rare non-Unicode sources.
const CF_OEMTEXT: u32 = 7;

const MONITOR_POLL_MS: u64 = 650;
const MONITOR_WATCHDOG_MS: u64 = 30_000;
const MONITOR_STALE_SECONDS: i64 = 90;

pub fn ensure_monitor(app: AppHandle, state: AppState) -> Result<(), String> {
    let mut guard = state.monitor_stop.lock().map_err(|error| error.to_string())?;
    if guard.is_some() {
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    *guard = Some(stop.clone());
    drop(guard);

    state.monitor_heartbeat.store(unix_now(), Ordering::Relaxed);
    app_log::info(&state.paths, "clipboard", "clipboard monitor started");
    thread::spawn(move || {
        let mut last_signature = initial_signature(&state).unwrap_or_default();
        while !stop.load(Ordering::Relaxed) {
            state.monitor_heartbeat.store(unix_now(), Ordering::Relaxed);
            // 剪贴板来源不可控，轮询体必须捕获 panic，避免某次系统剪贴板异常导致后台服务永久退出。
            // Clipboard providers are outside our control, so the polling body catches panics to keep one OS clipboard failure from permanently killing the background service.
            match catch_unwind(AssertUnwindSafe(|| poll_once(&app, &state, &mut last_signature))) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    app_log::warn(&state.paths, "clipboard", format!("poll skipped: {}", error));
                    let _ = app.emit("clipanchor-log", error);
                }
                Err(_) => {
                    app_log::error(&state.paths, "clipboard", "clipboard poll panicked and was recovered");
                }
            }
            thread::sleep(Duration::from_millis(MONITOR_POLL_MS));
        }
        app_log::info(&state.paths, "clipboard", "clipboard monitor thread stopped");
    });
    Ok(())
}

pub fn start_monitor_watchdog(app: AppHandle, state: AppState) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(MONITOR_WATCHDOG_MS));
        let should_monitor = state
            .settings
            .lock()
            .map(|settings| settings.pin_service_enabled || settings.history_service_enabled)
            .unwrap_or(true);
        if !should_monitor {
            continue;
        }
        let last_tick = state.monitor_heartbeat.load(Ordering::Relaxed);
        if last_tick <= 0 {
            continue;
        }
        let elapsed = unix_now().saturating_sub(last_tick);
        if elapsed <= MONITOR_STALE_SECONDS {
            continue;
        }

        app_log::warn(&state.paths, "clipboard", format!("clipboard monitor heartbeat stale for {} seconds; restarting monitor", elapsed));
        force_restart_monitor(&app, &state);
    });
}

fn force_restart_monitor(app: &AppHandle, state: &AppState) {
    // 看门狗先撤销旧停止句柄再启动新线程，是为了修复长时间后台后轮询线程死亡但状态仍显示运行的问题。
    // The watchdog clears the old stop handle before starting a new thread to recover cases where the poll thread died while the service still looks enabled.
    if let Ok(mut guard) = state.monitor_stop.lock() {
        if let Some(stop) = guard.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
    thread::sleep(Duration::from_millis(200));
    if let Err(error) = ensure_monitor(app.clone(), state.clone()) {
        app_log::error(&state.paths, "clipboard", format!("clipboard monitor restart failed: {}", error));
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}


fn initial_signature(state: &AppState) -> Result<String, String> {
    let settings = state.settings.lock().map_err(|error| error.to_string())?.clone();
    // 启动监听时先记录当前剪贴板指纹，是为了避免把启动前已经存在的内容误认为新复制并弹窗。
    // The monitor records the existing clipboard signature on startup so pre-existing clipboard content is not mistaken for a new copy.
    if settings.filter_file {
        if let Ok(paths) = read_file_paths_from_clipboard() {
            if !paths.is_empty() {
                return Ok(clipboard_change_signature(&content_hash_for_paths(&paths)));
            }
        }
    }
    if settings.filter_text {
        if let Ok(Some(text)) = read_clipboard_text() {
            return Ok(clipboard_change_signature(&content_hash_for_bytes("text", text.as_bytes())));
        }
    }
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    if settings.filter_image {
        if let Ok(image) = clipboard.get_image() {
            return Ok(clipboard_change_signature(&content_hash_for_bytes("image", image.bytes.as_ref())));
        }
    }
    Ok(String::new())
}

pub fn stop_monitor(state: &AppState) {
    app_log::info(&state.paths, "clipboard", "clipboard monitor stop requested");
    if let Ok(mut guard) = state.monitor_stop.lock() {
        if let Some(stop) = guard.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

fn poll_once(app: &AppHandle, state: &AppState, last_signature: &mut String) -> Result<(), String> {
    let settings = state.settings.lock().map_err(|error| error.to_string())?.clone();
    if !settings.pin_service_enabled && !settings.history_service_enabled {
        return Ok(());
    }
    if settings.filter_file {
        if let Ok(paths) = read_file_paths_from_clipboard() {
            if !paths.is_empty() {
                let signature = clipboard_change_signature(&content_hash_for_paths(&paths));
                if signature != *last_signature {
                    *last_signature = signature;
                    let item = item_from_files(paths);
                    process_item(app, state, &settings, item)?;
                    return Ok(());
                }
            }
        }
    }

    if settings.filter_text {
        if let Ok(Some(text)) = read_clipboard_text() {
            let signature = clipboard_change_signature(&content_hash_for_bytes("text", text.as_bytes()));
            if signature != *last_signature {
                *last_signature = signature;
                let item = item_from_text(text, &settings);
                process_item(app, state, &settings, item)?;
                return Ok(());
            }
        }
    }

    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;

    if settings.filter_image {
        if let Ok(image) = clipboard.get_image() {
            let bytes = image.bytes.to_vec();
            let signature = clipboard_change_signature(&content_hash_for_bytes("image", &bytes));
            if signature != *last_signature {
                *last_signature = signature;
                let item = item_from_image(image, state)?;
                process_item(app, state, &settings, item)?;
            }
        }
    }
    Ok(())
}

fn process_item(app: &AppHandle, state: &AppState, settings: &AppSettings, item: ClipItem) -> Result<(), String> {
    app_log::info(&state.paths, "clipboard", format!("captured item kind={:?} id={} bytes={} hash={}", item.kind, item.id, item.bytes, short_hash(&item.content_hash)));
    if should_filter_sensitive(settings, &item) {
        // 敏感过滤按设置级别运行；轻量模式只做正则/启发式检查，避免拖慢剪贴板捕获。
        // Sensitive filtering follows the configured level; light mode only uses regex-like heuristics so clipboard capture stays fast.
        app_log::warn(&state.paths, "privacy", format!("sensitive item skipped kind={:?} hash={}", item.kind, short_hash(&item.content_hash)));
        return Ok(());
    }
    close_duplicate_temp_popups(app, state, &item)?;
    state.temp_items.lock().map_err(|error| error.to_string())?.insert(item.id.clone(), item.clone());
    if settings.history_service_enabled {
        database::insert(&state.paths, &item)?;
        app_log::info(&state.paths, "history", format!("record stored id={} kind={:?}", item.id, item.kind));
        let _ = app.emit("history-updated", &item.id);
    }
    if settings.pin_service_enabled {
        app_log::info(&state.paths, "popup", format!("creating popup for id={} kind={:?}", item.id, item.kind));
        popup::create_popup(app, state, &item, settings)?;
    }
    Ok(())
}

fn short_hash(hash: &str) -> String {
    // 日志只记录哈希前缀，是为了定位重复内容问题，同时避免把完整剪贴板指纹写入诊断文件。
    // Logs keep only a hash prefix to diagnose duplicate handling without writing the full clipboard fingerprint to diagnostic files.
    hash.chars().take(12).collect()
}

fn should_filter_sensitive(settings: &AppSettings, item: &ClipItem) -> bool {
    let mode = settings.privacy_filter_mode.as_str();
    match mode {
        "off" => false,
        "smart" => item_looks_sensitive_smart(item),
        "light" => item_looks_sensitive_light(item),
        _ => settings.privacy_mode && item_looks_sensitive_light(item),
    }
}

fn item_looks_sensitive_light(item: &ClipItem) -> bool {
    match item.kind {
        ClipKind::Text => item.text_content.as_deref().map(text_looks_sensitive).unwrap_or(false),
        ClipKind::Image => false,
        ClipKind::File | ClipKind::Mixed => item.file_paths.iter().any(|path| file_path_looks_sensitive(path)),
    }
}

fn item_looks_sensitive_smart(item: &ClipItem) -> bool {
    // 智能模式目前在轻量启发式基础上对直接截图/图片采取保守策略，后续可替换为本地高级分类器。
    // Smart mode currently adds conservative direct-image filtering on top of light heuristics and can later be replaced by a local classifier.
    item_looks_sensitive_light(item) || matches!(item.kind, ClipKind::Image)
}


fn text_looks_sensitive(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if contains_private_key_block(trimmed) {
        return true;
    }

    let char_len = trimmed.chars().count();
    let line_count = trimmed.lines().count();
    if char_len > 280 || line_count > 6 {
        // 长文本最容易被隐私启发式误伤；这里只保留明确密钥赋值，避免普通文章、代码片段或日志被静默丢弃。
        // Long text is where privacy heuristics most often false-positive, so only explicit secret assignments are blocked instead of silently dropping prose, code, or logs.
        return trimmed.lines().any(line_has_explicit_secret_assignment);
    }

    let lower = trimmed.to_lowercase();
    if has_high_confidence_sensitive_marker(trimmed, &lower) || looks_like_long_secret(trimmed) {
        return true;
    }

    if looks_like_credit_card(trimmed)
        || looks_like_phone_number(trimmed)
        || looks_like_china_identity_number(trimmed)
        || looks_like_otp_code(trimmed)
    {
        return true;
    }
    false
}

fn contains_private_key_block(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("-----begin") && lower.contains("private key")
}

fn has_high_confidence_sensitive_marker(text: &str, lower: &str) -> bool {
    let direct_markers = [
        "private key", "-----begin", "ssh-rsa", "bearer ", "authorization:", "set-cookie",
        "client_secret", "refresh_token", "api_key", "apikey", "access_key",
        "验证码", "校验码", "动态码",
    ];
    if direct_markers.iter().any(|marker| lower.contains(marker)) {
        return true;
    }

    text.lines().any(line_has_explicit_secret_assignment)
}

fn line_has_explicit_secret_assignment(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() || line.chars().count() > 260 {
        return false;
    }
    let lower = line.to_lowercase();
    let assignment_markers = [
        "password", "passwd", "pwd", "secret", "token", "密钥", "密码", "令牌", "私钥", "身份证", "银行卡",
    ];
    if !assignment_markers.iter().any(|marker| lower.contains(marker)) {
        return false;
    }
    let Some(separator) = line.find('=').or_else(|| line.find(':')).or_else(|| line.find('：')) else {
        return false;
    };
    let value = line[separator + 1..].trim().trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | ',' | ';'));
    if value.is_empty() || value.contains(' ') || value.chars().count() < 4 {
        return false;
    }
    // 赋值语境下短密码也可能敏感；但仍要求是紧凑值，避免“Password: enter your password”这类说明文本被过滤。
    // Short passwords can be sensitive in assignment context, but the value must be compact so explanatory text is not filtered.
    value.chars().count() >= 8 || token_looks_like_secret(value)
}

fn looks_like_phone_number(text: &str) -> bool {
    let trimmed = text.trim();
    let char_len = trimmed.chars().count();
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() == 11 && char_len <= 24 {
        let bytes = digits.as_bytes();
        if bytes.first() == Some(&b'1') && matches!(bytes.get(1), Some(b'3'..=b'9')) {
            return true;
        }
    }
    let lower = trimmed.to_lowercase();
    let phone_markers = ["phone", "mobile", "cell", "tel", "telephone", "whatsapp", "手机号", "手机", "电话", "联系方式"];
    if digits.len() >= 7 && digits.len() <= 15 && phone_markers.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    let has_phone_shape = trimmed.contains('+')
        || trimmed.matches('-').count() >= 1
        || trimmed.contains('(')
        || trimmed.contains(')');
    if char_len <= 34 && digits.len() >= 8 && digits.len() <= 15 && has_phone_shape {
        return true;
    }
    false
}


fn looks_like_otp_code(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() > 120 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let markers = ["otp", "2fa", "mfa", "verification", "验证码", "校验码", "动态码"];
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    digits.len() >= 4 && digits.len() <= 8 && markers.iter().any(|marker| lower.contains(marker))
}

fn looks_like_china_identity_number(text: &str) -> bool {
    let compact: String = text.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect();
    if compact.len() != 18 {
        return false;
    }
    let upper = compact.to_ascii_uppercase();
    let chars: Vec<char> = upper.chars().collect();
    if !chars.iter().take(17).all(|ch| ch.is_ascii_digit()) || !(chars[17].is_ascii_digit() || chars[17] == 'X') {
        return false;
    }
    let year = chars[6..10].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    let month = chars[10..12].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    let day = chars[12..14].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    (1900..=2099).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn file_path_looks_sensitive(path: &str) -> bool {
    let lower = path.to_lowercase();
    ["password", "secret", "token", "key", "credential", "private", "密码", "密钥", "凭证", "私密"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn looks_like_credit_card(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let card_markers = ["card", "credit", "银行卡", "信用卡", "卡号"];
    if trimmed.chars().count() > 80 && !card_markers.iter().any(|marker| lower.contains(marker)) {
        return false;
    }
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut double_digit = false;
    for ch in digits.chars().rev() {
        let mut value = ch.to_digit(10).unwrap_or(0);
        if double_digit {
            value *= 2;
            if value > 9 { value -= 9; }
        }
        sum += value;
        double_digit = !double_digit;
    }
    sum % 10 == 0
}

fn looks_like_long_secret(text: &str) -> bool {
    text.split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | '<' | '>' | ',' | ';'))
        .any(token_looks_like_secret)
}

fn token_looks_like_secret(raw: &str) -> bool {
    let token = raw.trim_matches(|ch: char| matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '.'));
    let len = token.len();
    if !(32..=240).contains(&len) || token.contains('\\') || token.contains('/') || token.contains(':') {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    let known_prefixes = ["sk-", "ghp_", "gho_", "ghs_", "github_pat_", "xoxb-", "xoxp-", "akia"];
    if known_prefixes.iter().any(|prefix| lower.starts_with(prefix)) {
        return true;
    }
    if token.matches('.').count() == 2 {
        let jwt_like = token.split('.').all(|part| part.len() >= 8 && part.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
        if jwt_like {
            return true;
        }
    }
    if !token.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
        return false;
    }
    let has_upper = token.chars().any(|ch| ch.is_ascii_uppercase());
    let has_lower = token.chars().any(|ch| ch.is_ascii_lowercase());
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let separator_count = token.chars().filter(|ch| matches!(ch, '_' | '-' | '.')).count();
    let alnum_count = token.chars().filter(|ch| ch.is_ascii_alphanumeric()).count();
    // 长密钥必须是单个高熵 token，而不是把整段文本去掉空格后拼成的长串，否则普通说明文字会被误过滤。
    // A long secret must be one high-entropy token rather than a whole paragraph with spaces removed; otherwise normal prose is filtered by mistake.
    alnum_count >= 32 && has_upper && has_lower && has_digit && separator_count <= 8
}

fn close_duplicate_temp_popups(app: &AppHandle, state: &AppState, item: &ClipItem) -> Result<(), String> {
    let duplicate_ids = {
        let guard = state.temp_items.lock().map_err(|error| error.to_string())?;
        guard.iter()
            .filter(|(id, existing)| {
                *id != &item.id
                    && existing.kind == item.kind
                    && !existing.content_hash.is_empty()
                    && existing.content_hash == item.content_hash
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
    };
    for id in duplicate_ids {
        app_log::info(&state.paths, "popup", format!("duplicate popup closed: {}", id));
        // 新内容去重时同步关闭同内容旧弹窗，是为了让“最新复制”在历史记录和桌面弹窗中都保持唯一。
        // Duplicate popups are closed when newer identical content arrives so the newest copy stays unique in both history and desktop cards.
        let _ = popup::close_popup(app, &id);
        state.temp_items.lock().map_err(|error| error.to_string())?.remove(&id);
    }
    Ok(())
}

fn item_from_files(file_paths: Vec<String>) -> ClipItem {
    let bytes = file_paths.iter().filter_map(|path| fs::metadata(path).ok()).map(|meta| meta.len() as i64).sum();
    let image_count = file_paths.iter().filter(|path| is_image_path(path)).count();
    let summary = if file_paths.len() > 1 {
        if image_count == file_paths.len() {
            format!("{} images · {}", file_paths.len(), human_size(bytes))
        } else if image_count > 0 {
            format!("{} files · {} images · {}", file_paths.len(), image_count, human_size(bytes))
        } else {
            format!("{} files · {}", file_paths.len(), human_size(bytes))
        }
    } else {
        let path = Path::new(&file_paths[0]);
        format!("{} · {}", path.file_name().and_then(|name| name.to_str()).unwrap_or("file"), human_size(bytes))
    };
    let content_hash = content_hash_for_paths(&file_paths);

    // Windows Explorer 复制文件时使用 CF_HDROP 而不是普通文本，因此文件项需要单独构造。
    // Windows Explorer uses CF_HDROP instead of plain text for copied files, so file items need their own constructor.
    ClipItem {
        id: Uuid::new_v4().to_string(),
        kind: ClipKind::File,
        summary,
        text_content: None,
        image_path: None,
        file_paths,
        bytes,
        created_at: Utc::now().to_rfc3339(),
        content_hash,
        is_pinned: false,
    }
}

fn item_from_text(text: String, settings: &AppSettings) -> ClipItem {
    let maybe_files = if settings.filter_file { parse_file_paths(&text) } else { Vec::new() };
    let kind = if maybe_files.is_empty() { ClipKind::Text } else { ClipKind::File };
    let bytes = if maybe_files.is_empty() { text.as_bytes().len() as i64 } else { maybe_files.iter().filter_map(|path| fs::metadata(path).ok()).map(|meta| meta.len() as i64).sum() };
    let summary = if maybe_files.len() > 1 {
        format!("{} files · {}", maybe_files.len(), human_size(bytes))
    } else if maybe_files.len() == 1 {
        let path = Path::new(&maybe_files[0]);
        format!("{} · {}", path.file_name().and_then(|name| name.to_str()).unwrap_or("file"), human_size(bytes))
    } else {
        text.chars().take(200).collect()
    };
    let content_hash = if maybe_files.is_empty() {
        content_hash_for_bytes("text", text.as_bytes())
    } else {
        content_hash_for_paths(&maybe_files)
    };

    // 文本剪贴板里常见 file:// 或路径列表，因此先识别文件再决定摘要，避免把文件复制误显示为普通文本。
    // Text clipboards often carry file:// or path lists, so files are recognized before summary generation to avoid misleading text cards.
    ClipItem {
        id: Uuid::new_v4().to_string(),
        kind,
        summary,
        text_content: if maybe_files.is_empty() { Some(text) } else { None },
        image_path: None,
        file_paths: maybe_files,
        bytes,
        created_at: Utc::now().to_rfc3339(),
        content_hash,
        is_pinned: false,
    }
}

fn item_from_image(image: ImageData<'_>, state: &AppState) -> Result<ClipItem, String> {
    let id = Uuid::new_v4().to_string();
    let path = state.paths.resources.join(format!("{}.clipanchorrgba", id));
    let thumb_path = state.paths.resources.join(format!("{}-thumb.png", id));
    let bytes = image.bytes.to_vec();
    let content_hash = content_hash_for_bytes("image", &bytes);
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(image.width as u32, image.height as u32, bytes.clone())
        .ok_or_else(|| "Cannot decode clipboard image buffer".to_string())?;

    // 只同步写入无压缩 RGBA 原始数据，是为了避免大图 PNG 压缩阻塞剪贴板监听线程和弹窗首帧渲染。
    // Only uncompressed RGBA data is written synchronously so large PNG compression cannot block the clipboard monitor or popup first paint.
    write_raw_clipanchor_image(&path, image.width as u32, image.height as u32, &bytes)?;

    // 缩略图尺寸很小，可以安全用于弹窗预览；真实 Copy 仍使用上面的原始 RGBA 数据。
    // The thumbnail is small enough for popup preview while Copy still restores the original RGBA payload above.
    let dynamic = image::DynamicImage::ImageRgba8(buffer);
    let thumbnail = dynamic.thumbnail(360, 220);
    thumbnail.save(&thumb_path).map_err(|error| error.to_string())?;

    Ok(ClipItem {
        id,
        kind: ClipKind::Image,
        summary: format!("Image · {} × {}", image.width, image.height),
        text_content: None,
        image_path: Some(path.to_string_lossy().to_string()),
        file_paths: Vec::new(),
        bytes: bytes.len() as i64,
        created_at: Utc::now().to_rfc3339(),
        content_hash,
        is_pinned: false,
    })
}

fn read_clipboard_text() -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(Some(text)) = read_windows_text_with_retries() {
            return Ok(Some(text));
        }
    }
    match Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
        Ok(text) => Ok(Some(text)),
        Err(_) => Ok(None),
    }
}

#[cfg(target_os = "windows")]
fn read_windows_text_with_retries() -> Result<Option<String>, String> {
    for attempt in 0..6 {
        if let Some(text) = read_windows_text_once()? {
            return Ok(Some(text));
        }
        if attempt < 5 {
            // Windows 剪贴板在大段文本或富文本复制时可能会短暂被来源程序占用；轻量重试可以补上这一瞬间，而不会阻塞监听线程太久。
            // Windows clipboard can be briefly owned by the source app during large or rich-text copies; a short retry catches that window without blocking the monitor for long.
            thread::sleep(Duration::from_millis(35));
        }
    }
    Ok(None)
}

#[cfg(target_os = "windows")]
fn read_windows_text_once() -> Result<Option<String>, String> {
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return Ok(None);
        }
        let mut value = None;
        if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
            value = read_windows_unicode_text_from_open_clipboard().and_then(normalize_clipboard_text);
        }
        if value.is_none() && IsClipboardFormatAvailable(CF_TEXT) != 0 {
            value = read_windows_ansi_text_from_open_clipboard(CF_TEXT).and_then(normalize_clipboard_text);
        }
        if value.is_none() && IsClipboardFormatAvailable(CF_OEMTEXT) != 0 {
            value = read_windows_ansi_text_from_open_clipboard(CF_OEMTEXT).and_then(normalize_clipboard_text);
        }
        if value.is_none() {
            value = read_windows_html_text_from_open_clipboard();
        }
        CloseClipboard();
        Ok(value)
    }
}

#[cfg(target_os = "windows")]
unsafe fn read_windows_unicode_text_from_open_clipboard() -> Option<String> {
    let handle = GetClipboardData(CF_UNICODETEXT);
    if handle.is_null() {
        return None;
    }
    let locked = GlobalLock(handle) as *const u16;
    if locked.is_null() {
        return None;
    }
    let unit_count = (GlobalSize(handle) / mem::size_of::<u16>()).max(1);
    let slice = std::slice::from_raw_parts(locked, unit_count);
    let end = slice.iter().position(|value| *value == 0).unwrap_or(slice.len());
    let text = OsString::from_wide(&slice[..end]).to_string_lossy().to_string();
    GlobalUnlock(handle);
    Some(text)
}

#[cfg(target_os = "windows")]
unsafe fn read_windows_ansi_text_from_open_clipboard(format: u32) -> Option<String> {
    let handle = GetClipboardData(format);
    if handle.is_null() {
        return None;
    }
    let locked = GlobalLock(handle) as *const u8;
    if locked.is_null() {
        return None;
    }
    let byte_count = GlobalSize(handle).max(1);
    let slice = std::slice::from_raw_parts(locked, byte_count);
    let end = slice.iter().position(|value| *value == 0).unwrap_or(slice.len());
    let text = String::from_utf8_lossy(&slice[..end]).to_string();
    GlobalUnlock(handle);
    Some(text)
}

#[cfg(target_os = "windows")]
unsafe fn read_windows_html_text_from_open_clipboard() -> Option<String> {
    let mut format_name: Vec<u16> = "HTML Format".encode_utf16().collect();
    format_name.push(0);
    let format = RegisterClipboardFormatW(format_name.as_ptr());
    if format == 0 || IsClipboardFormatAvailable(format) == 0 {
        return None;
    }
    let handle = GetClipboardData(format);
    if handle.is_null() {
        return None;
    }
    let locked = GlobalLock(handle) as *const u8;
    if locked.is_null() {
        return None;
    }
    let byte_count = GlobalSize(handle).max(1);
    let data = std::slice::from_raw_parts(locked, byte_count).to_vec();
    GlobalUnlock(handle);
    parse_windows_html_clipboard(&data)
}

#[cfg(target_os = "windows")]
fn normalize_clipboard_text(text: String) -> Option<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.trim().is_empty() { None } else { Some(normalized) }
}

#[cfg(target_os = "windows")]
fn parse_windows_html_clipboard(data: &[u8]) -> Option<String> {
    let header_len = data.len().min(2048);
    let header = String::from_utf8_lossy(&data[..header_len]);
    let start = read_cf_html_offset(&header, "StartFragment").or_else(|| read_cf_html_offset(&header, "StartHTML"));
    let end = read_cf_html_offset(&header, "EndFragment").or_else(|| read_cf_html_offset(&header, "EndHTML"));
    let fragment = match (start, end) {
        (Some(start), Some(end)) if start < end && end <= data.len() => &data[start..end],
        _ => data,
    };
    let html = String::from_utf8_lossy(fragment).to_string();
    let text = strip_html_for_clipboard(&html);
    // 富文本来源有时不给 CF_UNICODETEXT，只给 HTML Format；提取片段文本可以保证长网页文本仍会进入历史记录。
    // Some rich-text sources expose HTML Format without CF_UNICODETEXT; extracting fragment text keeps long web copies in history.
    normalize_clipboard_text(text)
}

#[cfg(target_os = "windows")]
fn read_cf_html_offset(header: &str, key: &str) -> Option<usize> {
    header.lines().find_map(|line| {
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(':'))
            .and_then(|value| value.trim().parse::<usize>().ok())
    })
}

#[cfg(target_os = "windows")]
fn strip_html_for_clipboard(html: &str) -> String {
    let without_comments = html
        .replace("<!--StartFragment-->", "")
        .replace("<!--EndFragment-->", "");
    let mut output = String::new();
    let mut in_tag = false;
    for ch in without_comments.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn parse_file_paths(text: &str) -> Vec<String> {
    let cleaned = text.replace('\r', "");
    let candidates: Vec<String> = cleaned.lines()
        .filter_map(normalize_clipboard_path)
        .filter(|line| !line.is_empty())
        .collect();
    if !candidates.is_empty() && candidates.iter().all(|path| Path::new(path).exists()) {
        candidates
    } else {
        Vec::new()
    }
}

fn normalize_clipboard_path(line: &str) -> Option<String> {
    let mut value = line.trim().trim_matches('"').to_string();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("file:///") {
        value = rest.to_string();
    } else if let Some(rest) = value.strip_prefix("file://") {
        value = rest.to_string();
    }
    Some(value.replace("%20", " "))
}

#[cfg(target_os = "windows")]
fn read_file_paths_from_clipboard() -> Result<Vec<String>, String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP) == 0 {
            return Ok(Vec::new());
        }
        if OpenClipboard(null_mut()) == 0 {
            return Err("Cannot open Windows clipboard".into());
        }

        let handle = GetClipboardData(CF_HDROP);
        if handle.is_null() {
            CloseClipboard();
            return Ok(Vec::new());
        }

        let count = DragQueryFileW(handle, u32::MAX, null_mut(), 0);
        let mut paths = Vec::new();
        for index in 0..count {
            let len = DragQueryFileW(handle, index, null_mut(), 0);
            if len == 0 {
                continue;
            }
            let mut buffer = vec![0u16; (len + 1) as usize];
            let written = DragQueryFileW(handle, index, buffer.as_mut_ptr(), len + 1);
            if written > 0 {
                let value = OsString::from_wide(&buffer[..written as usize]).to_string_lossy().to_string();
                if Path::new(&value).exists() {
                    paths.push(value);
                }
            }
        }
        CloseClipboard();
        Ok(paths)
    }
}

#[cfg(not(target_os = "windows"))]
fn read_file_paths_from_clipboard() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

pub fn content_hash_for_bytes(prefix: &str, bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in prefix.as_bytes().iter().chain(bytes.iter()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{}:{:016x}", prefix, hash)
}

pub fn content_hash_for_paths(paths: &[String]) -> String {
    // 路径列表使用稳定哈希而不是数据库 id，是为了让同一批文件重复复制时能识别为同一内容。
    // File lists use a stable hash instead of the database id so repeated copies of the same files are recognized as the same content.
    content_hash_for_bytes("file", paths.join("\u{1f}").as_bytes())
}

fn clipboard_change_signature(content_hash: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if sequence != 0 {
            // Windows 会在“复制同一内容”时递增序列号；把序列号并入监听指纹，才能按需求每次复制都触发新弹窗与去重写入。
            // Windows increments the sequence number even when the same content is copied; including it lets every copy create a fresh popup and deduplicated record.
            return format!("seq:{}:{}", sequence, content_hash);
        }
    }
    content_hash.to_string()
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

pub fn copy_to_clipboard(record: &HistoryRecord) -> Result<(), String> {
    match record.kind {
        ClipKind::Image => {
            let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
            let path = record.image_path.as_ref().ok_or_else(|| "Image path is missing".to_string())?;
            let (width, height, bytes) = read_image_rgba_for_clipboard(path)?;
            let data = ImageData { width: width as usize, height: height as usize, bytes: Cow::Owned(bytes) };
            clipboard.set_image(data).map_err(|error| error.to_string())?;
        }
        ClipKind::File => {
            copy_file_paths_to_clipboard(&record.file_paths)?;
        }
        _ => {
            let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
            clipboard.set_text(record.text_content.clone().unwrap_or_else(|| record.summary.clone())).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

const RAW_IMAGE_MAGIC: &[u8; 8] = b"CLIPRGBA";

fn write_raw_clipanchor_image(path: &Path, width: u32, height: u32, bytes: &[u8]) -> Result<(), String> {
    let mut output = Vec::with_capacity(16 + bytes.len());
    output.extend_from_slice(RAW_IMAGE_MAGIC);
    output.extend_from_slice(&width.to_le_bytes());
    output.extend_from_slice(&height.to_le_bytes());
    output.extend_from_slice(bytes);
    fs::write(path, output).map_err(|error| error.to_string())
}

pub fn is_raw_clipanchor_image(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("clipanchorrgba"))
        .unwrap_or(false)
}

fn read_raw_clipanchor_image(path: &str) -> Result<Option<(u32, u32, Vec<u8>)>, String> {
    if !is_raw_clipanchor_image(path) {
        return Ok(None);
    }
    let data = fs::read(path).map_err(|error| error.to_string())?;
    if data.len() < 16 || &data[..8] != RAW_IMAGE_MAGIC {
        return Err("Invalid ClipAnchor image payload".into());
    }
    let width = u32::from_le_bytes(data[8..12].try_into().map_err(|_| "Invalid image width".to_string())?);
    let height = u32::from_le_bytes(data[12..16].try_into().map_err(|_| "Invalid image height".to_string())?);
    let bytes = data[16..].to_vec();
    if bytes.len() != width as usize * height as usize * 4 {
        return Err("Invalid ClipAnchor image byte length".into());
    }
    Ok(Some((width, height, bytes)))
}

fn read_image_rgba_for_clipboard(path: &str) -> Result<(u32, u32, Vec<u8>), String> {
    if let Some((width, height, bytes)) = read_raw_clipanchor_image(path)? {
        return Ok((width, height, bytes));
    }
    let image = image::open(path).map_err(|error| error.to_string())?.to_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}

pub fn thumbnail_bytes_for_path(path: &str, max_width: u32, max_height: u32) -> Result<Vec<u8>, String> {
    let dynamic = if let Some((width, height, bytes)) = read_raw_clipanchor_image(path)? {
        let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, bytes)
            .ok_or_else(|| "Cannot rebuild raw ClipAnchor image".to_string())?;
        image::DynamicImage::ImageRgba8(buffer)
    } else {
        image::open(path).map_err(|error| error.to_string())?
    };
    let thumbnail = dynamic.thumbnail(max_width, max_height);
    let mut cursor = Cursor::new(Vec::new());
    thumbnail.write_to(&mut cursor, image::ImageFormat::Png).map_err(|error| error.to_string())?;
    Ok(cursor.into_inner())
}

pub fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "tif" | "tiff"))
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn copy_file_paths_to_clipboard(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No file paths to copy".into());
    }
    let valid_paths: Vec<String> = paths.iter().filter(|path| Path::new(path.as_str()).exists()).cloned().collect();
    if valid_paths.is_empty() {
        return Err("No valid files to copy".into());
    }

    let mut encoded_paths: Vec<u16> = Vec::new();
    for path in &valid_paths {
        encoded_paths.extend(OsString::from(path).encode_wide());
        encoded_paths.push(0);
    }
    encoded_paths.push(0);

    let header_size = 20usize;
    let total_size = header_size + encoded_paths.len() * mem::size_of::<u16>();

    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, total_size);
        if memory.is_null() {
            return Err("Cannot allocate clipboard file payload".into());
        }
        let locked = GlobalLock(memory) as *mut u8;
        if locked.is_null() {
            return Err("Cannot lock clipboard file payload".into());
        }

        // CF_HDROP 需要 DROPFILES 头和双零结尾的 UTF-16 路径列表；这样复制出来的是文件对象而不是路径文本。
        // CF_HDROP needs a DROPFILES header plus a double-null-terminated UTF-16 path list, so the clipboard contains file objects rather than path text.
        ptr::write_unaligned(locked.add(0) as *mut u32, header_size as u32);
        ptr::write_unaligned(locked.add(4) as *mut i32, 0);
        ptr::write_unaligned(locked.add(8) as *mut i32, 0);
        ptr::write_unaligned(locked.add(12) as *mut i32, 0);
        ptr::write_unaligned(locked.add(16) as *mut i32, 1);
        ptr::copy_nonoverlapping(encoded_paths.as_ptr() as *const u8, locked.add(header_size), encoded_paths.len() * mem::size_of::<u16>());
        GlobalUnlock(memory);

        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("Cannot open clipboard for files".into());
        }
        EmptyClipboard();
        let placed = SetClipboardData(CF_HDROP, memory);
        CloseClipboard();
        if placed.is_null() {
            return Err("Cannot place files on clipboard".into());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn copy_file_paths_to_clipboard(paths: &[String]) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    // 非 Windows 平台先退化为路径文本，是为了保持 Copy 按钮可用；后续可接入平台原生文件剪贴板格式。
    // Non-Windows builds fall back to path text to keep Copy usable; native file clipboard formats can be added per platform later.
    clipboard.set_text(paths.join("\n")).map_err(|error| error.to_string())
}
