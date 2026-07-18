use crate::{paths, settings};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::{atomic::{AtomicBool, AtomicI64}, Arc, Mutex}};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipKind {
    Text,
    Image,
    File,
    Mixed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipItem {
    pub id: String,
    pub kind: ClipKind,
    pub summary: String,
    pub text_content: Option<String>,
    pub image_path: Option<String>,
    pub file_paths: Vec<String>,
    pub bytes: i64,
    pub created_at: String,
    pub content_hash: String,
    pub is_pinned: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: String,
    pub kind: ClipKind,
    pub summary: String,
    pub text_content: Option<String>,
    pub image_path: Option<String>,
    pub file_paths: Vec<String>,
    pub bytes: i64,
    pub created_at: String,
    pub content_hash: String,
    pub is_pinned: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutSettings {
    pub toggle_pin_service: String,
    pub toggle_history_service: String,
    pub toggle_main_window: String,
    pub enter_light_mode: String,
    pub toggle_theme_mode: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            toggle_pin_service: "Ctrl+Shift+P".into(),
            toggle_history_service: "Ctrl+Shift+H".into(),
            toggle_main_window: "Ctrl+Shift+X".into(),
            enter_light_mode: "Ctrl+Shift+L".into(),
            toggle_theme_mode: "Ctrl+Shift+T".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub pin_service_enabled: bool,
    pub history_service_enabled: bool,
    pub privacy_mode: bool,
    pub privacy_filter_mode: String,
    pub auto_start: bool,
    pub locale: String,
    pub theme: String,
    pub scale: String,
    pub ui_scale_percent: u32,
    pub light_mode_minutes: u64,
    pub auto_hide_actions: bool,
    pub auto_destroy_seconds: u64,
    pub animation_mode: String,
    pub popup_x: f64,
    pub popup_y: f64,
    pub popup_width: f64,
    pub popup_height: f64,
    pub popup_scale_percent: u32,
    pub history_limit: u32,
    pub log_retention_days: u32,
    pub filter_text: bool,
    pub filter_image: bool,
    pub filter_file: bool,
    pub auto_update_enabled: bool,
    pub translation_api_provider: String,
    pub translation_api_url: String,
    pub translation_api_key: String,
    pub translation_api_keys: HashMap<String, String>,
    pub shortcuts: ShortcutSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            pin_service_enabled: true,
            history_service_enabled: true,
            privacy_mode: false,
            privacy_filter_mode: "light".into(),
            auto_start: false,
            locale: "auto".into(),
            theme: "system".into(),
            scale: "medium".into(),
            ui_scale_percent: 100,
            light_mode_minutes: 5,
            auto_hide_actions: true,
            auto_destroy_seconds: 10,
            animation_mode: "performance".into(),
            popup_x: 24.0,
            popup_y: 24.0,
            popup_width: 340.0,
            popup_height: 220.0,
            popup_scale_percent: 100,
            history_limit: 0,
            log_retention_days: 7,
            filter_text: true,
            filter_image: true,
            filter_file: true,
            auto_update_enabled: true,
            translation_api_provider: "uapis".into(),
            translation_api_url: "https://uapis.cn/api/v1/translate/text".into(),
            translation_api_key: String::new(),
            translation_api_keys: HashMap::new(),
            shortcuts: ShortcutSettings::default(),
        }
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateStatusPayload {
    pub status: String,
    pub service_enabled: bool,
    pub update_available: bool,
    pub update_failed: bool,
    pub prompt_on_main_open: bool,
    pub attention_required: bool,
    pub checked_at: String,
    pub source: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub release_tag: Option<String>,
    pub release_name: Option<String>,
    pub release_notes: Option<String>,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub downloaded_path: Option<String>,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: Option<u64>,
    pub install_ready: bool,
    pub message: Option<String>,
}

impl Default for UpdateStatusPayload {
    fn default() -> Self {
        Self {
            status: "idle".into(),
            service_enabled: false,
            update_available: false,
            update_failed: false,
            prompt_on_main_open: false,
            attention_required: false,
            checked_at: String::new(),
            source: "unknown".into(),
            current_version: None,
            latest_version: None,
            release_tag: None,
            release_name: None,
            release_notes: None,
            asset_name: None,
            asset_url: None,
            downloaded_path: None,
            total_bytes: None,
            downloaded_bytes: None,
            install_ready: false,
            message: None,
        }
    }
}


#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguageMessageStatus {
    pub source_hash: String,
    pub translation_hash: String,
    pub modified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguagePackPayload {
    pub code: String,
    pub label: String,
    pub native_name: String,
    pub source: String,
    pub generated_at: String,
    pub format: String,
    pub source_locale: String,
    pub messages: HashMap<String, String>,
    pub message_status: HashMap<String, LanguageMessageStatus>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub file_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub integrity: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_keys: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outdated_keys: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed_keys: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modified_keys: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub integrity_error: String,
}

impl Default for LanguagePackPayload {
    fn default() -> Self {
        Self {
            code: String::new(),
            label: String::new(),
            native_name: String::new(),
            source: String::new(),
            generated_at: String::new(),
            format: "clipanchor-language-pack".into(),
            source_locale: "en".into(),
            messages: HashMap::new(),
            message_status: HashMap::new(),
            file_name: String::new(),
            integrity: String::new(),
            missing_keys: Vec::new(),
            outdated_keys: Vec::new(),
            removed_keys: Vec::new(),
            modified_keys: Vec::new(),
            integrity_error: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BootstrapPayload {
    pub settings: AppSettings,
    pub paths: PathPayload,
    pub app_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathPayload {
    pub data: String,
    pub database: String,
    pub resources: String,
    pub locales: String,
    pub logs: String,
}

#[derive(Clone)]
pub struct AppState {
    pub paths: paths::DataPaths,
    pub settings: Arc<Mutex<AppSettings>>,
    pub temp_items: Arc<Mutex<HashMap<String, ClipItem>>>,
    pub monitor_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    pub monitor_heartbeat: Arc<AtomicI64>,
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let paths = paths::resolve()?;
        paths::ensure(&paths)?;
        let loaded = settings::load(&paths).unwrap_or_default();
        Ok(Self {
            paths,
            settings: Arc::new(Mutex::new(loaded)),
            temp_items: Arc::new(Mutex::new(HashMap::new())),
            monitor_stop: Arc::new(Mutex::new(None)),
            monitor_heartbeat: Arc::new(AtomicI64::new(0)),
        })
    }
}
