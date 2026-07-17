use crate::{models::AppSettings, paths::DataPaths};
use std::fs;

const DEFAULT_TRANSLATION_PROVIDER: &str = "uapis";
const UAPI_TRANSLATION_ENDPOINT: &str = "https://uapis.cn/api/v1/translate/text";
const MYMEMORY_TRANSLATION_ENDPOINT: &str = "https://api.mymemory.translated.net/get";

fn normalized_provider(value: &str, legacy_url: &str) -> String {
    let candidate = value.trim().to_ascii_lowercase();
    if matches!(candidate.as_str(), "uapis" | "mymemory") {
        return candidate;
    }
    if legacy_url.to_ascii_lowercase().contains("mymemory") {
        return "mymemory".into();
    }
    DEFAULT_TRANSLATION_PROVIDER.into()
}

fn provider_endpoint(provider: &str) -> &'static str {
    if provider == "mymemory" {
        MYMEMORY_TRANSLATION_ENDPOINT
    } else {
        UAPI_TRANSLATION_ENDPOINT
    }
}

pub fn normalize_translation_settings(settings: &mut AppSettings, accept_active_key_edit: bool) -> bool {
    let before = serde_json::to_string(settings).unwrap_or_default();
    let provider = normalized_provider(&settings.translation_api_provider, &settings.translation_api_url);

    // 每个服务商保存独立密钥，是为了切换服务时恢复对应凭据，并避免把 UAPI 密钥误发给无密钥的 MyMemory 接口。
    // Provider-specific key storage restores the matching credential on switch and prevents a UAPI key from being sent to keyless MyMemory requests.
    if settings.translation_api_keys.is_empty() && !settings.translation_api_key.trim().is_empty() {
        settings
            .translation_api_keys
            .insert(provider.clone(), settings.translation_api_key.trim().to_string());
    } else if accept_active_key_edit {
        let current = settings
            .translation_api_keys
            .get(&provider)
            .cloned()
            .unwrap_or_default();
        if current != settings.translation_api_key {
            settings
                .translation_api_keys
                .insert(provider.clone(), settings.translation_api_key.trim().to_string());
        }
    }

    settings.translation_api_provider = provider.clone();
    settings.translation_api_url = provider_endpoint(&provider).into();
    settings.translation_api_key = settings
        .translation_api_keys
        .get(&provider)
        .cloned()
        .unwrap_or_default();

    before != serde_json::to_string(settings).unwrap_or_default()
}

pub fn load(paths: &DataPaths) -> Result<AppSettings, String> {
    if !paths.settings.exists() {
        let default = AppSettings::default();
        save(paths, &default)?;
        return Ok(default);
    }
    let text = fs::read_to_string(&paths.settings).map_err(|error| error.to_string())?;
    let mut loaded: AppSettings = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if normalize_translation_settings(&mut loaded, false) {
        save(paths, &loaded)?;
    }
    Ok(loaded)
}

pub fn save(paths: &DataPaths, settings: &AppSettings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(&paths.settings, text).map_err(|error| error.to_string())
}
