use crate::{models::AppSettings, paths::DataPaths, secrets};
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

pub fn normalize_runtime_settings(settings: &mut AppSettings) {
    match settings.privacy_filter_mode.trim() {
        "off" | "light" => {}
        "smart" => settings.privacy_filter_mode = "light".into(),
        "" => {
            // 旧配置只有布尔隐私开关；空模式按该开关迁移，避免升级后过滤级别与界面不一致。
            // Older configs only had a boolean privacy switch; empty mode follows that flag so upgrades keep filter level and UI aligned.
            settings.privacy_filter_mode = if settings.privacy_mode { "light".into() } else { "off".into() };
        }
        _ => settings.privacy_filter_mode = "light".into(),
    }
    settings.privacy_mode = settings.privacy_filter_mode != "off";
    if settings.auto_destroy_seconds == 0 {
        settings.auto_destroy_seconds = 3;
    }
    if settings.animation_mode.trim().is_empty() {
        settings.animation_mode = "elegant".into();
    }
    if settings.ui_scale_percent == 0 {
        settings.ui_scale_percent = 100;
    }
}

pub fn load(paths: &DataPaths) -> Result<AppSettings, String> {
    if !paths.settings.exists() {
        let default = AppSettings::default();
        save(paths, &default)?;
        return Ok(default);
    }
    let text = fs::read_to_string(&paths.settings).map_err(|error| error.to_string())?;
    let mut loaded: AppSettings = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let disk_had_keys = !loaded.translation_api_key.trim().is_empty() || !loaded.translation_api_keys.is_empty();
    normalize_translation_settings(&mut loaded, false);
    normalize_runtime_settings(&mut loaded);

    let secret_keys = secrets::load_translation_keys(paths)?;
    if secret_keys.is_empty() && disk_had_keys {
        // 首次启动把旧 settings.json 里的明文密钥迁走，是为了升级后不再把凭据留在可被同步或复制的配置文件中。
        // The first launch migrates plaintext keys out of settings.json so upgrades stop leaving credentials in a file that can be copied or synced.
        secrets::save_translation_keys(paths, &loaded.translation_api_keys)?;
        save(paths, &loaded)?;
    } else if !secret_keys.is_empty() {
        loaded.translation_api_keys = secret_keys;
        normalize_translation_settings(&mut loaded, false);
    }
    Ok(loaded)
}

pub fn save(paths: &DataPaths, settings: &AppSettings) -> Result<(), String> {
    secrets::save_translation_keys(paths, &settings.translation_api_keys)?;
    let mut disk = settings.clone();
    // JSON 只保留服务商名称和开关；密钥走操作系统保护存储，避免便携 data 目录被复制时泄露凭据。
    // JSON keeps provider names and switches only; keys use OS-protected storage so copying the portable data directory does not leak credentials.
    disk.translation_api_key.clear();
    disk.translation_api_keys.clear();
    let text = serde_json::to_string_pretty(&disk).map_err(|error| error.to_string())?;
    fs::write(&paths.settings, text).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::DataPaths;
    use std::fs;

    fn temp_paths() -> (DataPaths, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "clipanchor-settings-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let paths = DataPaths {
            root: root.clone(),
            data: root.clone(),
            database: root.join("clipanchor.db"),
            settings: root.join("settings.json"),
            resources: root.join("resources"),
            exports: root.join("exports"),
            locales: root.join("locales"),
            logs: root.join("logs"),
        };
        (paths, root)
    }

    #[test]
    fn save_strips_translation_keys_from_settings_json() {
        let (paths, root) = temp_paths();
        let mut settings = AppSettings::default();
        settings.translation_api_provider = "uapis".into();
        settings.translation_api_key = "plain-secret".into();
        settings.translation_api_keys.insert("uapis".into(), "plain-secret".into());
        save(&paths, &settings).unwrap();
        let disk = fs::read_to_string(&paths.settings).unwrap();
        assert!(!disk.contains("plain-secret"));
        let loaded = load(&paths).unwrap();
        assert_eq!(loaded.translation_api_key, "plain-secret");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_legacy_privacy_boolean_and_rejects_smart_mode() {
        let mut settings = AppSettings::default();
        settings.privacy_mode = true;
        settings.privacy_filter_mode.clear();
        normalize_runtime_settings(&mut settings);
        assert_eq!(settings.privacy_filter_mode, "light");
        settings.privacy_filter_mode = "smart".into();
        normalize_runtime_settings(&mut settings);
        assert_eq!(settings.privacy_filter_mode, "light");
        assert!(settings.privacy_mode);
    }
}
