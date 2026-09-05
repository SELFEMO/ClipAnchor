use crate::{app_log, models::{AppState, LanguageMessageStatus, LanguagePackPayload}};
use chrono::Utc;
use std::{collections::{HashMap, HashSet}, fs, time::Duration};
use tauri::{AppHandle, Manager, State};

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
    state.paths.locales.clone()
}

fn language_pack_reference_messages(value: serde_json::Value) -> HashMap<String, String> {
    match value {
        serde_json::Value::Object(messages) => messages
            .into_iter()
            .filter_map(|(key, value)| {
                let key = key.trim().to_string();
                if key.is_empty() {
                    return None;
                }
                Some((key, value.as_str().unwrap_or_default().to_string()))
            })
            .collect(),
        // Mixed-version frontends may still send only a key array. Those packs remain
        // discoverable, but source-change detection needs the current English dictionary.
        serde_json::Value::Array(values) => values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::trim).filter(|key| !key.is_empty()).map(|key| (key.to_string(), String::new())))
            .collect(),
        _ => HashMap::new(),
    }
}

fn language_text_hash(value: &str) -> String {
    // FNV-1a is intentionally used as a lightweight change fingerprint. It matches the
    // existing legacy language-pack metadata and is not intended as a security hash.
    let mut hash: u32 = 0x811c9dc5;
    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("{hash:08x}")
}

fn language_pack_for_disk(pack: &LanguagePackPayload) -> LanguagePackPayload {
    let mut disk = pack.clone();
    // These fields describe the current runtime comparison and are recalculated on scan.
    disk.file_name.clear();
    disk.integrity.clear();
    disk.missing_keys.clear();
    disk.outdated_keys.clear();
    disk.removed_keys.clear();
    disk.modified_keys.clear();
    disk.integrity_error.clear();
    disk
}

#[tauri::command]
pub fn list_language_packs(required_keys: serde_json::Value, app: AppHandle, state: State<'_, AppState>) -> Result<Vec<LanguagePackPayload>, String> {
    let reference_messages = language_pack_reference_messages(required_keys);
    let mut required_keys = reference_messages.keys().cloned().collect::<Vec<_>>();
    required_keys.sort();
    let directory = language_pack_dir(&state);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    // 每次扫描前同步安装包资源与旧目录，是为了让 Linux 安装版和用户手动刷新都能立即发现语言文件，而不依赖一次性的启动时机。
    // Synchronizing bundled and legacy sources before every scan lets Linux packages and manual refresh discover language files immediately instead of relying on a one-time startup window.
    let resource_dir = app.path().resource_dir().ok();
    match crate::paths::sync_language_pack_sources(&state.paths, resource_dir.as_deref()) {
        Ok(copied) if copied > 0 => app_log::info(&state.paths, "i18n", format!("copied {} extension language file(s) before scan", copied)),
        Ok(_) => {}
        Err(error) => app_log::warn(&state.paths, "i18n", format!("language source synchronization failed before scan: {}", error)),
    }
    app_log::info(&state.paths, "i18n", format!("checking language pack directory {}", directory.to_string_lossy()));

    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => {
                // Linux 目录可能含有暂时失效的挂载或符号链接；跳过单个坏条目可避免整个扩展语言列表因此消失。
                // A Linux directory may contain a transient mount or broken link; skipping one bad entry prevents the entire extension-language list from disappearing.
                app_log::warn(&state.paths, "i18n", format!("language directory entry skipped: {}", error));
            }
        }
    }
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());

    let mut packs = Vec::new();
    let mut seen_codes = HashSet::new();
    for entry in entries {
        let path = entry.path();
        let is_json = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if !is_json {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        let file_code = normalize_language_code(
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
        );
        let diagnostic_code = if file_code.is_empty() { "unknown" } else { file_code.as_str() };

        let text = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) => {
                packs.push(damaged_language_pack(diagnostic_code, &file_name, error.to_string()));
                continue;
            }
        };

        // 部分 Linux 编辑器会保存 UTF-8 BOM；解析前移除它可避免内容有效但扩展语言选项被误判为损坏。
        // Some Linux editors save a UTF-8 BOM; removing it before parsing prevents valid language files from being marked corrupt and hidden from selection.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let mut pack = match serde_json::from_str::<LanguagePackPayload>(text) {
            Ok(value) => value,
            Err(error) => {
                packs.push(damaged_language_pack(diagnostic_code, &file_name, error.to_string()));
                continue;
            }
        };
        let declared_code = normalize_language_code(&pack.code);
        // Linux 文件名可能包含非 UTF-8 字节或仅作为用户备注；有效 JSON 内声明的语言代码应优先决定选项，而不是强制依赖文件名。
        // Linux filenames may contain non-UTF-8 bytes or serve only as user notes; a valid JSON-declared language code should define the option instead of requiring the filename.
        pack.code = if declared_code.is_empty() { file_code.clone() } else { declared_code };
        if pack.code.is_empty() || is_core_language_code(&pack.code) {
            continue;
        }
        if !seen_codes.insert(pack.code.clone()) {
            // Linux 文件系统区分大小写且允许任意文件名，因此按 JSON 内声明的规范语言代码去重，避免同一语言出现多个等价选项。
            // Linux file systems are case-sensitive and allow arbitrary filenames, so deduplicating by the canonical code declared in JSON prevents equivalent choices.
            app_log::warn(&state.paths, "i18n", format!("duplicate language code skipped: {} ({})", pack.code, file_name));
            continue;
        }
        if pack.label.trim().is_empty() {
            pack.label = pack.code.to_uppercase();
        }
        if pack.native_name.trim().is_empty() {
            pack.native_name = pack.label.clone();
        }
        if pack.format.trim().is_empty() {
            pack.format = "clipanchor-language-pack".into();
        }
        if pack.source_locale.trim().is_empty() {
            pack.source_locale = "en".into();
        }
        pack.file_name = file_name;
        pack.integrity_error.clear();
        pack.missing_keys.clear();
        pack.outdated_keys.clear();
        pack.removed_keys.clear();
        pack.modified_keys.clear();

        let mut metadata_changed = false;
        for key in &required_keys {
            let Some(translation) = pack.messages.get(key) else {
                pack.missing_keys.push(key.clone());
                continue;
            };

            let current_translation_hash = language_text_hash(translation);
            let current_source = reference_messages.get(key).cloned().unwrap_or_default();
            let current_source_hash = if current_source.is_empty() { String::new() } else { language_text_hash(&current_source) };
            let status = pack.message_status.entry(key.clone()).or_insert_with(|| {
                metadata_changed = true;
                LanguageMessageStatus {
                    source_hash: current_source_hash.clone(),
                    translation_hash: current_translation_hash.clone(),
                    modified: false,
                }
            });

            if status.translation_hash.is_empty() {
                status.translation_hash = current_translation_hash.clone();
                metadata_changed = true;
            } else if status.translation_hash != current_translation_hash {
                // A translation changed outside the generator. Record the new baseline and
                // protect the human edit from automatic overwrite during incremental updates.
                status.translation_hash = current_translation_hash.clone();
                if !status.modified {
                    status.modified = true;
                }
                metadata_changed = true;
            }

            if !current_source_hash.is_empty() {
                if status.source_hash.is_empty() {
                    // Metadata-free legacy packs are migrated without spending API calls.
                    status.source_hash = current_source_hash.clone();
                    metadata_changed = true;
                } else if status.source_hash != current_source_hash {
                    pack.outdated_keys.push(key.clone());
                }
            }

            if status.modified {
                pack.modified_keys.push(key.clone());
            }
        }

        if !required_keys.is_empty() {
            pack.removed_keys = pack
                .messages
                .keys()
                .filter(|key| !reference_messages.contains_key(*key))
                .cloned()
                .collect();
            pack.removed_keys.sort();
        }
        pack.missing_keys.sort();
        pack.outdated_keys.sort();
        pack.modified_keys.sort();

        if pack.messages.is_empty() {
            pack.integrity = "corrupt".into();
            pack.integrity_error = "language pack does not contain any usable messages".into();
        } else if !pack.missing_keys.is_empty() || !pack.outdated_keys.is_empty() || !pack.removed_keys.is_empty() {
            pack.integrity = "update_available".into();
        } else {
            pack.integrity = "complete".into();
        }

        if metadata_changed {
            let disk = language_pack_for_disk(&pack);
            match serde_json::to_string_pretty(&disk) {
                Ok(value) => match fs::write(&path, value) {
                    Ok(()) => app_log::info(&state.paths, "i18n", format!("migrated language metadata for {}", pack.code)),
                    Err(error) => app_log::warn(&state.paths, "i18n", format!("could not persist language metadata for {}: {}", pack.code, error)),
                },
                Err(error) => app_log::warn(&state.paths, "i18n", format!("could not serialize language metadata for {}: {}", pack.code, error)),
            }
        }
        packs.push(pack);
    }

    packs.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    let warning_count = packs.iter().filter(|pack| pack.integrity != "complete").count();
    app_log::info(
        &state.paths,
        "i18n",
        format!("checked language packs: {} pack(s), {} warning(s)", packs.len(), warning_count),
    );
    Ok(packs)
}

fn damaged_language_pack(code: &str, file_name: &str, error: String) -> LanguagePackPayload {
    LanguagePackPayload {
        code: code.to_string(),
        label: code.to_uppercase(),
        native_name: code.to_uppercase(),
        source: "local-file".into(),
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
    if pack.format.trim().is_empty() {
        pack.format = "clipanchor-language-pack".into();
    }
    if pack.source_locale.trim().is_empty() {
        pack.source_locale = "en".into();
    }
    // Ensure every saved translation has a status record. Frontend incremental updates
    // normally provide these values, while this fallback keeps direct/manual callers valid.
    for (key, translation) in &pack.messages {
        let status = pack.message_status.entry(key.clone()).or_default();
        if status.translation_hash.is_empty() {
            status.translation_hash = language_text_hash(translation);
        }
    }
    pack.file_name = format!("{}.json", pack.code);
    pack.integrity = "complete".into();
    pack.missing_keys.clear();
    pack.outdated_keys.clear();
    pack.removed_keys.clear();
    pack.modified_keys.clear();
    pack.integrity_error.clear();
    let directory = language_pack_dir(&state);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let output = directory.join(&pack.file_name);
    let disk = language_pack_for_disk(&pack);
    let text = serde_json::to_string_pretty(&disk).map_err(|error| error.to_string())?;
    // 生成语言包写入 data/locales，是为了让用户可备份、可编辑，同时避免把机器翻译结果混入内置语言源码。
    // Generated language packs are stored in data/locales so users can back them up or edit them without mixing machine translations into built-in source files.
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
pub async fn translate_ui_text(
    provider: String,
    target_code: String,
    text: String,
    api_key: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let app_state = state.inner().clone();
    // 阻塞 HTTP 放到 worker 线程，避免同步命令占住主线程，设置页滚动和其它 IPC 在生成语言包时仍能响应。
    // Blocking HTTP runs on a worker thread so a sync command cannot occupy the main thread, keeping settings scroll and other IPC responsive while a language pack is generated.
    tauri::async_runtime::spawn_blocking(move || {
        translate_ui_text_blocking(provider, target_code, text, api_key, &app_state)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn translate_ui_text_blocking(
    provider: String,
    target_code: String,
    text: String,
    api_key: Option<String>,
    state: &AppState,
) -> Result<String, String> {
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
        "uapis" => translate_with_uapis(&client, &normalized_target, &text, api_key.as_deref().unwrap_or_default(), state),
        _ => translate_with_mymemory(&client, &normalized_target, &text, api_key.as_deref().unwrap_or_default(), state),
    }
}

fn translate_with_mymemory(client: &reqwest::blocking::Client, target_code: &str, text: &str, api_key: &str, state: &AppState) -> Result<String, String> {
    let langpair = format!("en|{}", target_code);
    // 这里不用 RequestBuilder::query，是因为当前 reqwest 版本的 blocking builder 没有暴露该方法；提前构造 URL 可以保持相同请求语义并避免编译失败。
    // RequestBuilder::query is intentionally avoided because the current reqwest blocking builder does not expose it; pre-building the URL keeps the same request semantics and prevents compilation failure.
    let api_key = api_key.trim();
    let mut params = vec![("q", text), ("langpair", langpair.as_str())];
    if !api_key.is_empty() {
        // MyMemory 没有稳定的 header 认证；email 走 de，其它凭据走 key，且绝不把带密钥的完整 URL 写入日志。
        // MyMemory has no stable header auth; emails use de and other credentials use key, and the key-bearing URL is never written to logs.
        if api_key.contains('@') {
            params.push(("de", api_key));
        } else {
            params.push(("key", api_key));
        }
    }
    let url = reqwest::Url::parse_with_params(
        "https://api.mymemory.translated.net/get",
        &params,
    )
    .map_err(|error| error.to_string())?;
    let mut request = client.get(url);
    if !api_key.is_empty() && !api_key.contains('@') {
        request = request.header("X-Mymemory-Key", api_key);
    }
    let response = request
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

fn translate_with_uapis(client: &reqwest::blocking::Client, target_code: &str, text: &str, api_key: &str, state: &AppState) -> Result<String, String> {
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
pub fn read_clipboard_text_for_input(state: State<'_, AppState>) -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    let text = clipboard.get_text().map_err(|error| error.to_string())?;
    // API Key 读取只记录字符数量，不记录内容，既能诊断 macOS 粘贴问题，也不会把密钥写入日志。
    // API-key reads log only character count, never content, so macOS paste issues remain diagnosable without leaking credentials.
    app_log::info(&state.paths, "i18n", format!("clipboard text read for settings input: {} character(s)", text.chars().count()));
    Ok(text)
}

#[tauri::command]
pub fn open_language_pack_folder(state: State<'_, AppState>) -> Result<(), String> {
    fs::create_dir_all(&state.paths.locales).map_err(|error| error.to_string())?;
    app_log::info(&state.paths, "i18n", "open language pack folder requested from UI");
    super::open_path_with_system(&state.paths.locales)
}
