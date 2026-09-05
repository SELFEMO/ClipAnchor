use crate::paths::DataPaths;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const MAX_HISTORY_IMPORT_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_HISTORY_IMPORT_RECORDS: usize = 20_000;

pub(crate) fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    // Windows canonicalize 会加上 \\?\ 或 \\?\UNC\ 前缀；msiexec 和路径包含判断都需要普通 Win32 路径。
    // Windows canonicalize adds a \\?\ or \\?\UNC\ prefix; msiexec and containment checks both need ordinary Win32 paths.
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

pub fn canonicalize_existing(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("Cannot resolve path {}: {}", path.display(), error))
}

pub fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = strip_verbatim_prefix(path);
    let root = strip_verbatim_prefix(root);
    path.starts_with(&root)
}

pub fn assert_path_within(path: &Path, root: &Path, label: &str) -> Result<PathBuf, String> {
    let canonical = canonicalize_existing(path)?;
    let root = canonicalize_existing(root)?;
    if !path_is_within(&canonical, &root) {
        return Err(format!("{} is outside the allowed directory", label));
    }
    Ok(canonical)
}

pub fn assert_resource_file(paths: &DataPaths, candidate: &Path) -> Result<PathBuf, String> {
    assert_path_within(candidate, &paths.resources, "Image path")
}

pub fn assert_update_file(paths: &DataPaths, candidate: &Path) -> Result<PathBuf, String> {
    let updates = paths.data.join("updates");
    fs::create_dir_all(&updates).map_err(|error| error.to_string())?;
    assert_path_within(candidate, &updates, "Update package")
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn assert_history_file_extension(path: &Path, format: &str) -> Result<(), String> {
    let expected = match format.trim().to_ascii_lowercase().as_str() {
        "csv" => "csv",
        _ => "json",
    };
    let actual = extension_of(path);
    if actual != expected {
        return Err(format!("History file must use .{} extension", expected));
    }
    Ok(())
}

pub fn assert_history_import_file(path: &Path, format: &str) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err("Selected history file does not exist".into());
    }
    assert_history_file_extension(path, format)?;
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_HISTORY_IMPORT_BYTES {
        return Err(format!(
            "History file is larger than {} MB",
            MAX_HISTORY_IMPORT_BYTES / (1024 * 1024)
        ));
    }
    Ok(path.to_path_buf())
}

pub fn assert_history_export_path(path: &Path, format: &str) -> Result<(), String> {
    assert_history_file_extension(path, format)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn assert_import_record_count(count: usize) -> Result<(), String> {
    if count > MAX_HISTORY_IMPORT_RECORDS {
        return Err(format!(
            "History import exceeds {} records",
            MAX_HISTORY_IMPORT_RECORDS
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::DataPaths;
    use std::fs;

    fn temp_paths() -> (DataPaths, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "clipanchor-fs-guard-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("resources")).unwrap();
        fs::create_dir_all(root.join("updates")).unwrap();
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
    fn accepts_files_inside_resources() {
        let (paths, root) = temp_paths();
        let file = paths.resources.join("shot.png");
        fs::write(&file, b"png").unwrap();
        let allowed = assert_resource_file(&paths, &file).unwrap();
        assert!(path_is_within(&allowed, &fs::canonicalize(&paths.resources).unwrap()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_files_outside_resources() {
        let (paths, root) = temp_paths();
        let outsider = root.join("secret.txt");
        fs::write(&outsider, b"nope").unwrap();
        assert!(assert_resource_file(&paths, &outsider).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_parent_directory_escape() {
        let (paths, root) = temp_paths();
        let escaped = paths.resources.join("..").join("secret.txt");
        fs::write(root.join("secret.txt"), b"nope").unwrap();
        assert!(assert_resource_file(&paths, &escaped).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_import_requires_matching_extension_and_size() {
        let dir = std::env::temp_dir().join(format!(
            "clipanchor-history-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let json = dir.join("backup.json");
        fs::write(&json, b"{}").unwrap();
        assert!(assert_history_import_file(&json, "json").is_ok());
        assert!(assert_history_import_file(&json, "csv").is_err());
        let csv = dir.join("backup.csv");
        fs::write(&csv, b"id\n").unwrap();
        assert!(assert_history_import_file(&csv, "csv").is_ok());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn import_record_count_is_capped() {
        assert!(assert_import_record_count(1).is_ok());
        assert!(assert_import_record_count(MAX_HISTORY_IMPORT_RECORDS).is_ok());
        assert!(assert_import_record_count(MAX_HISTORY_IMPORT_RECORDS + 1).is_err());
    }

    #[test]
    fn strip_verbatim_prefix_removes_extended_length_prefix() {
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\D:\updates\ClipAnchor.msi")),
            PathBuf::from(r"D:\updates\ClipAnchor.msi")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"D:\updates\ClipAnchor.msi")),
            PathBuf::from(r"D:\updates\ClipAnchor.msi")
        );
    }

    #[test]
    fn strip_verbatim_prefix_rewrites_unc_extended_paths() {
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\UNC\server\share\ClipAnchor.msi")),
            PathBuf::from(r"\\server\share\ClipAnchor.msi")
        );
    }
}
