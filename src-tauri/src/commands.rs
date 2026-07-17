use crate::{app_log, autostart, clipboard_service, database, models::{AppSettings, AppState, BootstrapPayload, ClipItem, ClipKind, HistoryRecord, LanguageMessageStatus, LanguagePackPayload, PathPayload, UpdateStatusPayload}, popup, settings, update_service};
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
    // 退出前保存主窗口正常位置，是为了让下次启动回到用户最后整理好的工作区，而不是随机落在桌面。
    // Saving the normal main-window position before exit lets the next launch return to the user's arranged workspace instead of landing randomly.
    let _ = crate::window_control::save_main_window_position(&app);
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

fn canonical_language_part(part: &str, index: usize) -> String {
    let cleaned: String = part.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect();
    if cleaned.is_empty() {
        return String::new();
    }
    if index == 0 {
        return cleaned.to_ascii_lowercase();
    }
    if cleaned.len() == 4 && cleaned.chars().all(|ch| ch.is_ascii_alphabetic()) {
        let mut chars = cleaned.chars();
        let first = chars.next().map(|ch| ch.to_ascii_uppercase()).unwrap_or_default();
        let rest: String = chars.map(|ch| ch.to_ascii_lowercase()).collect();
        return format!("{}{}", first, rest);
    }
    if (cleaned.len() == 2 && cleaned.chars().all(|ch| ch.is_ascii_alphabetic()))
        || (cleaned.len() == 3 && cleaned.chars().all(|ch| ch.is_ascii_digit()))
    {
        return cleaned.to_ascii_uppercase();
    }
    cleaned.to_ascii_lowercase()
}

fn normalize_language_code(value: &str) -> String {
    // 后端保存语言包时也保持 BCP-47 标准大小写，是为了让 zh-Hant/zh-TW 不再被当作内置简体中文处理。
    // The backend also preserves BCP-47 casing when saving packs so zh-Hant/zh-TW are not collapsed into the built-in Simplified Chinese locale.
    value
        .trim()
        .replace('_', "-")
        .split('-')
        .enumerate()
        .map(|(index, part)| canonical_language_part(part, index))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn is_core_language_code(code: &str) -> bool {
    code == "en"
        || code.starts_with("en-")
        || code == "zh"
        || code == "zh-CN"
        || code == "zh-Hans"
        || code.starts_with("zh-Hans-")
}

fn language_pack_dir(state: &AppState) -> std::path::PathBuf {
    state.paths.data.join("locales")
}

fn language_source_hash(value: &str) -> String {
    // 稳定的轻量指纹只用于判断内置英文文案是否变化，避免未变化条目重复调用翻译接口。
    // A stable lightweight fingerprint detects changes in built-in English copy, avoiding repeated API calls for unchanged entries.
    let mut hash: u32 = 0x811c9dc5;
    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("{hash:08x}")
}

#[tauri::command]
pub fn list_language_packs(reference_messages: HashMap<String, String>, state: State<'_, AppState>) -> Result<Vec<LanguagePackPayload>, String> {
    let directory = language_pack_dir(&state);
    app_log::info(&state.paths, "i18n", format!("checking language pack directory {}", directory.to_string_lossy()));
    if !directory.exists() {
        app_log::info(&state.paths, "i18n", "checked language packs: directory missing, 0 pack(s)");
        return Ok(Vec::new());
    }

    let mut packs = Vec::new();
    for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let is_json = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if !is_json {
            continue;
        }

        let file_name = path.file_name().and_then(|value| value.to_str()).unwrap_or_default().to_string();
        let file_code = normalize_language_code(path.file_stem().and_then(|value| value.to_str()).unwrap_or_default());
        if file_code.is_empty() || is_core_language_code(&file_code) {
            continue;
        }

        let text = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) => {
                // 中文：无法读取的手动语言文件仍显示在设置页，是为了让用户看到错误标记并直接修复。
                // English: Unreadable manual language files remain visible in Settings so users can see an error marker and repair them.
                packs.push(damaged_language_pack(&file_code, &file_name, error.to_string()));
                continue;
            }
        };
        let mut pack = match serde_json::from_str::<LanguagePackPayload>(&text) {
            Ok(value) => value,
            Err(error) => {
                packs.push(damaged_language_pack(&file_code, &file_name, error.to_string()));
                continue;
            }
        };

        // 中文：文件名作为稳定语言代号，是为了让刷新、更新和删除始终操作同一个本地文件。
        // English: The filename is used as the stable language code so refresh, update, and delete always target the same local file.
        pack.code = file_code.clone();
        if pack.label.trim().is_empty() {
            pack.label = pack.code.to_uppercase();
        }
        if pack.native_name.trim().is_empty() {
            pack.native_name = pack.label.clone();
        }

        let mut metadata_changed = false;
        if pack.format != "clipanchor-language-pack" {
            pack.format = "clipanchor-language-pack".into();
            metadata_changed = true;
        }
        if pack.source_locale.trim().is_empty() {
            pack.source_locale = "en".into();
            metadata_changed = true;
        }
        if pack.file_name != file_name {
            pack.file_name = file_name;
            metadata_changed = true;
        }

        pack.integrity_error.clear();
        pack.missing_keys.clear();
        pack.outdated_keys.clear();
        pack.removed_keys.clear();

        for (key, source_text) in &reference_messages {
            let current_source_hash = language_source_hash(source_text);
            let Some(translated_text) = pack.messages.get(key) else {
                pack.missing_keys.push(key.clone());
                continue;
            };
            let current_translation_hash = language_source_hash(translated_text);

            match pack.message_status.get_mut(key) {
                Some(status) => {
                    // 中文：译文指纹变化说明用户或外部工具手动改过该项；保留此标记可避免增量更新覆盖人工修订。
                    // English: A changed translation fingerprint means the entry was edited manually or by an external tool; preserving this marker prevents incremental updates from overwriting human revisions.
                    if !status.translation_hash.is_empty() && status.translation_hash != current_translation_hash && !status.modified {
                        status.modified = true;
                        metadata_changed = true;
                    }
                    if status.translation_hash.is_empty() {
                        status.translation_hash = current_translation_hash.clone();
                        metadata_changed = true;
                    }
                    if status.source_hash.is_empty() {
                        // 中文：旧格式文件没有源文案指纹时，以当前英文建立基线而不整包重译，是为了避免升级后突然消耗大量 API 次数。
                        // English: Legacy files without source fingerprints establish the current English copy as a baseline without retranslating the whole pack, avoiding an unexpected API spike after upgrade.
                        status.source_hash = current_source_hash.clone();
                        metadata_changed = true;
                    } else if status.source_hash != current_source_hash {
                        pack.outdated_keys.push(key.clone());
                    }
                }
                None => {
                    // 中文：为旧语言文件逐项补充状态，而不修改译文正文，使用户直接复制的 JSON 也能从此获得增量更新能力。
                    // English: Add per-entry status to legacy language files without changing translated copy, so directly copied JSON files gain incremental update support.
                    pack.message_status.insert(
                        key.clone(),
                        LanguageMessageStatus {
                            source_hash: current_source_hash,
                            translation_hash: current_translation_hash,
                            modified: false,
                        },
                    );
                    metadata_changed = true;
                }
            }
        }

        let obsolete_status_keys = pack
            .message_status
            .keys()
            .filter(|key| !pack.messages.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>();
        for key in obsolete_status_keys {
            pack.message_status.remove(&key);
            metadata_changed = true;
        }

        pack.removed_keys = pack
            .messages
            .keys()
            .filter(|key| !reference_messages.contains_key(*key))
            .cloned()
            .collect();
        pack.missing_keys.sort();
        pack.outdated_keys.sort();
        pack.removed_keys.sort();
        pack.integrity = if pack.messages.is_empty() {
            "incomplete".into()
        } else if !pack.missing_keys.is_empty() || !pack.outdated_keys.is_empty() || !pack.removed_keys.is_empty() {
            "update_available".into()
        } else {
            "complete".into()
        };

        if metadata_changed {
            // 中文：扫描只补充兼容元数据而不改写翻译正文，是为了让手动语言文件可升级、可校验且不丢失用户内容。
            // English: Scanning adds compatibility metadata without rewriting translated copy, making manual packs upgradeable and verifiable without losing user content.
            match serde_json::to_string_pretty(&pack) {
                Ok(serialized) => {
                    if let Err(error) = fs::write(&path, serialized) {
                        app_log::warn(&state.paths, "i18n", format!("failed to persist language status metadata for {}: {}", pack.code, error));
                    }
                }
                Err(error) => app_log::warn(&state.paths, "i18n", format!("failed to serialize language status metadata for {}: {}", pack.code, error)),
            }
        }
        packs.push(pack);
    }

    packs.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    let update_count = packs.iter().filter(|pack| pack.integrity == "update_available").count();
    let error_count = packs.iter().filter(|pack| matches!(pack.integrity.as_str(), "corrupt" | "incomplete")).count();
    app_log::info(
        &state.paths,
        "i18n",
        format!("checked language packs: {} pack(s), {} update(s), {} error(s)", packs.len(), update_count, error_count),
    );
    Ok(packs)
}

fn damaged_language_pack(code: &str, file_name: &str, error: String) -> LanguagePackPayload {
    LanguagePackPayload {
        code: code.to_string(),
        label: code.to_uppercase(),
        native_name: code.to_uppercase(),
        source: "local-file".into(),
        format: "clipanchor-language-pack".into(),
        source_locale: "en".into(),
        file_name: file_name.to_string(),
        integrity: "corrupt".into(),
        integrity_error: error.chars().take(180).collect(),
        ..LanguagePackPayload::default()
    }
}

#[tauri::command]
pub fn save_language_pack(mut pack: LanguagePackPayload, state: State<'_, AppState>) -> Result<LanguagePackPayload, String> {
    pack.code = normalize_language_code(&pack.code);
    if pack.code.is_empty() || pack.code == "auto" || is_core_language_code(&pack.code) {
        return Err("Invalid language code".into());
    }
    if pack.messages.is_empty() {
        return Err("Language pack has no messages".into());
    }
    if pack.label.trim().is_empty() {
        pack.label = pack.code.to_uppercase();
    }
    if pack.native_name.trim().is_empty() {
        pack.native_name = pack.label.clone();
    }
    if pack.generated_at.trim().is_empty() {
        pack.generated_at = Utc::now().to_rfc3339();
    }
    if pack.source.trim().is_empty() {
        pack.source = "generated".into();
    }
    pack.format = "clipanchor-language-pack".into();
    pack.source_locale = "en".into();
    pack.file_name = format!("{}.json", pack.code);

    // 中文：保存时同步译文指纹并删除孤立状态项，是为了让下一次扫描能识别手动修改，同时避免被删文本的元数据长期堆积。
    // English: Saving synchronizes translation fingerprints and removes orphaned status entries so the next scan detects manual edits without accumulating metadata for deleted copy.
    let message_keys = pack.messages.keys().cloned().collect::<std::collections::HashSet<_>>();
    pack.message_status.retain(|key, _| message_keys.contains(key));
    for (key, value) in &pack.messages {
        let current_translation_hash = language_source_hash(value);
        let status = pack.message_status.entry(key.clone()).or_default();
        status.translation_hash = current_translation_hash;
    }

    pack.integrity = "complete".into();
    pack.missing_keys.clear();
    pack.outdated_keys.clear();
    pack.removed_keys.clear();
    pack.integrity_error.clear();
    let directory = language_pack_dir(&state);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let output = directory.join(&pack.file_name);
    let text = serde_json::to_string_pretty(&pack).map_err(|error| error.to_string())?;
    // 中文：生成语言包写入 data/locales，是为了让用户可备份、可编辑，同时避免机器翻译混入内置语言源码。
    // English: Generated packs are stored in data/locales so users can back them up or edit them without mixing machine translations into built-in sources.
    fs::write(&output, text).map_err(|error| error.to_string())?;
    app_log::info(&state.paths, "i18n", format!("saved generated language pack {} with {} message(s)", pack.code, pack.messages.len()));
    Ok(pack)
}


#[tauri::command]
pub fn delete_language_pack(code: String, state: State<'_, AppState>) -> Result<bool, String> {
    let normalized = normalize_language_code(&code);
    if normalized.is_empty() || normalized == "auto" || is_core_language_code(&normalized) {
        return Err("Invalid language code".into());
    }
    let directory = language_pack_dir(&state);
    let target = directory.join(format!("{}.json", normalized));
    if !target.exists() {
        app_log::warn(&state.paths, "i18n", format!("delete generated language pack requested but file is missing: {}", normalized));
        return Ok(false);
    }
    // 删除只允许命中 data/locales 下的标准语言包文件，是为了让用户能安全清理机器翻译结果而不会误删内置语言源码。
    // Deletion is restricted to standard pack files under data/locales so users can safely clean generated translations without touching built-in locale sources.
    fs::remove_file(&target).map_err(|error| error.to_string())?;
    app_log::info(&state.paths, "i18n", format!("deleted generated language pack {}", normalized));
    Ok(true)
}


#[tauri::command]
pub fn log_language_pack_event(
    event: String,
    code: String,
    provider: Option<String>,
    success: Option<bool>,
    detail: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let normalized_code = normalize_language_code(&code);
    let safe_event = event.chars().filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')).take(60).collect::<String>();
    let safe_provider = provider.unwrap_or_default().chars().filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ' ')).take(80).collect::<String>();
    let safe_detail = detail.unwrap_or_default().replace('\r', " ").replace('\n', " ").chars().take(220).collect::<String>();
    let outcome = success.map(|value| if value { "ok" } else { "failed" }).unwrap_or("noted");
    let message = format!(
        "language event={} code={} provider={} outcome={} detail={}",
        if safe_event.is_empty() { "unknown" } else { safe_event.as_str() },
        if normalized_code.is_empty() { "none" } else { normalized_code.as_str() },
        if safe_provider.is_empty() { "none" } else { safe_provider.as_str() },
        outcome,
        if safe_detail.is_empty() { "none" } else { safe_detail.as_str() }
    );
    // 语言包生成涉及第三方翻译接口，只记录语言代号和阶段结果，避免把具体界面文案或用户数据写入日志。
    // Language pack generation touches third-party translation APIs, so only locale codes and stage outcomes are logged instead of UI strings or user data.
    if success == Some(false) {
        app_log::warn(&state.paths, "i18n", message);
    } else {
        app_log::info(&state.paths, "i18n", message);
    }
    Ok(())
}


#[tauri::command]
pub fn translate_ui_text(provider: String, target_code: String, text: String, api_key: Option<String>, state: State<'_, AppState>) -> Result<String, String> {
    let normalized_provider = provider.trim().to_ascii_lowercase();
    let normalized_target = normalize_language_code(&target_code);
    if text.trim().is_empty() {
        return Ok(text);
    }
    if normalized_target.is_empty() {
        return Err("Invalid target language".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(24))
        .user_agent("ClipAnchor-i18n/desktop")
        .build()
        .map_err(|error| error.to_string())?;
    match normalized_provider.as_str() {
        "uapis" => translate_with_uapis(&client, &normalized_target, &text, api_key.as_deref().unwrap_or_default(), &state),
        _ => translate_with_mymemory(&client, &normalized_target, &text, api_key.as_deref().unwrap_or_default(), &state),
    }
}

fn translate_with_mymemory(client: &reqwest::blocking::Client, target_code: &str, text: &str, api_key: &str, state: &State<'_, AppState>) -> Result<String, String> {
    let langpair = format!("en|{}", target_code);
    // 这里不用 RequestBuilder::query，是因为当前 reqwest 版本的 blocking builder 没有暴露该方法；提前构造 URL 可以保持相同请求语义并避免编译失败。
    // RequestBuilder::query is intentionally avoided because the current reqwest blocking builder does not expose it; pre-building the URL keeps the same request semantics and prevents compilation failure.
    let api_key = api_key.trim();
    let mut params = vec![("q", text), ("langpair", langpair.as_str())];
    if !api_key.is_empty() {
        // MyMemory 的公开接口用 de 参数标识调用者，是为了在用户提供凭据时使用更稳定的调用配额，同时不改变免费匿名模式。
        // MyMemory's public endpoint uses the de parameter to identify callers, enabling a more stable quota when the user provides credentials while keeping anonymous free mode unchanged.
        params.push(("de", api_key));
    }
    let url = reqwest::Url::parse_with_params(
        "https://api.mymemory.translated.net/get",
        &params,
    )
    .map_err(|error| error.to_string())?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| format!("NETWORK_ERROR: {}", error))?;
    let status = response.status();
    let payload = response.text().map_err(|error| error.to_string())?;
    if !status.is_success() {
        if status.as_u16() == 429 {
            return Err("TRANSLATION_RATE_LIMITED".into());
        }
        return Err(format!("{} {}", status.as_u16(), summarize_http_payload(&payload)));
    }
    let value: serde_json::Value = serde_json::from_str(&payload).map_err(|error| error.to_string())?;
    extract_json_text(&value, &[&["responseData", "translatedText"], &["translatedText"], &["matches", "0", "translation"]])
        .filter(|translated| !translated.trim().is_empty())
        .ok_or_else(|| {
            app_log::warn(&state.paths, "i18n", "MyMemory response did not contain translated text");
            "Translation response is missing translated text".to_string()
        })
}

fn translate_with_uapis(client: &reqwest::blocking::Client, target_code: &str, text: &str, api_key: &str, state: &State<'_, AppState>) -> Result<String, String> {
    let api_key = api_key.trim();
    // UAPI 把目标语言定义为 URL 查询参数，正文只接收 text；严格按该契约发送，避免服务端计数成功但实际没有返回翻译结果。
    // UAPI defines the target locale as a URL query parameter and accepts only text in the JSON body; following that contract prevents counted requests that return no usable translation.
    let url = reqwest::Url::parse_with_params(
        "https://uapis.cn/api/v1/translate/text",
        &[("to_lang", target_code)],
    )
    .map_err(|error| error.to_string())?;
    let mut request = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({ "text": text }));
    if !api_key.is_empty() {
        // UAPI 的标准鉴权使用 Bearer 令牌；只发送官方头部，是为了避免密钥被重复投递到未定义的自定义头。
        // UAPI uses standard Bearer authentication; sending only the documented header avoids duplicating a secret into an undefined custom header.
        request = request.header(reqwest::header::AUTHORIZATION, format!("Bearer {}", api_key));
    }
    let response = request
        .send()
        .map_err(|error| format!("NETWORK_ERROR: {}", error))?;
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let payload = response.text().map_err(|error| error.to_string())?;
    if !status.is_success() {
        if status.as_u16() == 429 {
            return Err("TRANSLATION_RATE_LIMITED".into());
        }
        let suffix = if request_id.is_empty() { String::new() } else { format!(" request-id={}", request_id) };
        return Err(format!("{} {}{}", status.as_u16(), summarize_http_payload(&payload), suffix));
    }
    let value: serde_json::Value = serde_json::from_str(&payload).map_err(|error| error.to_string())?;
    extract_json_text(&value, &[
        &["data", "translated_text"],
        &["data", "translatedText"],
        &["data", "translation"],
        &["data", "translate"],
        &["data", "result"],
        &["data", "text"],
        &["result", "translated_text"],
        &["result", "translatedText"],
        &["result", "translation"],
        &["result", "text"],
        &["result"],
        &["translated_text"],
        &["translatedText"],
        &["translation"],
        &["translate"],
    ])
    .or_else(|| find_translation_string(&value))
    .filter(|translated| !translated.trim().is_empty())
    .ok_or_else(|| {
        app_log::warn(&state.paths, "i18n", format!("UAPI response did not contain translated text; keys={}", summarize_json_keys(&value)));
        "Translation response is missing translated text".to_string()
    })
}


fn extract_json_text(value: &serde_json::Value, paths: &[&[&str]]) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    for path in paths {
        let mut current = Some(value);
        for segment in *path {
            current = current.and_then(|node| {
                if let Ok(index) = segment.parse::<usize>() {
                    node.get(index)
                } else {
                    node.get(*segment)
                }
            });
            if current.is_none() {
                break;
            }
        }
        if let Some(text) = current.and_then(serde_json::Value::as_str) {
            return Some(text.to_string());
        }
    }
    None
}

fn find_translation_string(value: &serde_json::Value) -> Option<String> {
    const TRANSLATION_KEYS: &[&str] = &[
        "translated_text",
        "translatedText",
        "translation",
        "translate",
    ];
    match value {
        serde_json::Value::Object(object) => {
            for key in TRANSLATION_KEYS {
                if let Some(text) = object.get(*key).and_then(serde_json::Value::as_str) {
                    return Some(text.to_string());
                }
            }
            for key in ["data", "result"] {
                if let Some(found) = object.get(key).and_then(find_translation_string) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => values.iter().find_map(find_translation_string),
        _ => None,
    }
}


fn summarize_json_keys(value: &serde_json::Value) -> String {
    value.as_object()
        .map(|object| object.keys().take(8).cloned().collect::<Vec<_>>().join(","))
        .unwrap_or_else(|| "non-object".into())
}

fn summarize_http_payload(payload: &str) -> String {
    let mut compact = payload.replace('\r', " ").replace('\n', " ");
    compact.truncate(120);
    compact
}

#[tauri::command]
pub fn get_bootstrap(state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
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
    let settings = settings_guard.clone();
    drop(settings_guard);
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
pub fn save_settings(mut settings_value: AppSettings, app: AppHandle, state: State<'_, AppState>) -> Result<AppSettings, String> {
    settings::normalize_translation_settings(&mut settings_value, true);
    validate_shortcuts(&settings_value)?;
    app_log::info(&state.paths, "settings", "saving settings from UI");
    {
        let mut guard = state.settings.lock().map_err(|error| error.to_string())?;
        if guard.locale != settings_value.locale {
            app_log::info(&state.paths, "i18n", format!("active language changed from {} to {}", guard.locale, settings_value.locale));
        }
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
