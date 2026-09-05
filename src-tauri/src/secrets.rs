use crate::paths::DataPaths;
use serde_json::Value;
use std::{collections::HashMap, fs};

#[cfg(target_os = "windows")]
const SECRET_FILE_NAME: &str = "translation-keys.dpapi";
#[cfg(not(target_os = "windows"))]
const SECRET_FILE_NAME: &str = "translation-keys.json";

fn secret_path(paths: &DataPaths) -> std::path::PathBuf {
    paths.data.join(SECRET_FILE_NAME)
}

pub fn load_translation_keys(paths: &DataPaths) -> Result<HashMap<String, String>, String> {
    let path = secret_path(paths);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let json = decrypt_bytes(&bytes)?;
    let value: Value = serde_json::from_slice(&json).unwrap_or(Value::Object(Default::default()));
    let mut keys = HashMap::new();
    if let Some(map) = value.as_object() {
        for (provider, secret) in map {
            if let Some(text) = secret.as_str() {
                if !text.trim().is_empty() {
                    keys.insert(provider.clone(), text.to_string());
                }
            }
        }
    }
    Ok(keys)
}

pub fn save_translation_keys(paths: &DataPaths, keys: &HashMap<String, String>) -> Result<(), String> {
    fs::create_dir_all(&paths.data).map_err(|error| error.to_string())?;
    let filtered: HashMap<String, String> = keys
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let path = secret_path(paths);
    if filtered.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let json = serde_json::to_vec(&filtered).map_err(|error| error.to_string())?;
    let protected = encrypt_bytes(&json)?;
    fs::write(&path, protected).map_err(|error| error.to_string())?;
    restrict_secret_file_permissions(&path)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn encrypt_bytes(plain: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_protect(plain)
}

#[cfg(target_os = "windows")]
fn decrypt_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_unprotect(bytes)
}

#[cfg(not(target_os = "windows"))]
fn encrypt_bytes(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.to_vec())
}

#[cfg(not(target_os = "windows"))]
fn decrypt_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    Ok(bytes.to_vec())
}

#[cfg(not(target_os = "windows"))]
fn restrict_secret_file_permissions(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn restrict_secret_file_permissions(_path: &std::path::Path) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB},
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &mut input,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Could not protect translation credentials".into());
    }
    let protected = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(protected)
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB},
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Could not read protected translation credentials".into());
    }
    let plain = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::DataPaths;
    use std::fs;

    fn temp_paths() -> (DataPaths, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "clipanchor-secrets-{}",
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
    fn round_trips_provider_keys_without_plaintext_json_on_windows() {
        let (paths, root) = temp_paths();
        let mut keys = HashMap::new();
        keys.insert("uapis".into(), "secret-value".into());
        save_translation_keys(&paths, &keys).unwrap();
        let loaded = load_translation_keys(&paths).unwrap();
        assert_eq!(loaded.get("uapis").map(String::as_str), Some("secret-value"));
        let raw = fs::read(secret_path(&paths)).unwrap();
        let as_text = String::from_utf8_lossy(&raw);
        if cfg!(target_os = "windows") {
            assert!(!as_text.contains("secret-value"));
        } else {
            assert!(as_text.contains("secret-value"));
        }
        let _ = fs::remove_dir_all(root);
    }
}
