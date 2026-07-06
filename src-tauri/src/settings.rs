use crate::{models::AppSettings, paths::DataPaths};
use std::fs;

pub fn load(paths: &DataPaths) -> Result<AppSettings, String> {
    if !paths.settings.exists() {
        let default = AppSettings::default();
        save(paths, &default)?;
        return Ok(default);
    }
    let text = fs::read_to_string(&paths.settings).map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

pub fn save(paths: &DataPaths, settings: &AppSettings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(&paths.settings, text).map_err(|error| error.to_string())
}
