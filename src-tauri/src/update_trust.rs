use crate::fs_guard;
use crate::paths::DataPaths;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "www.github.com",
    "api.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "github-releases.githubusercontent.com",
];

pub fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

pub fn hex_sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect())
}

pub fn is_allowed_download_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if let Some(port) = url.port() {
        if port != 443 {
            return false;
        }
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    ALLOWED_HOSTS.iter().any(|allowed| host.eq_ignore_ascii_case(allowed))
}

pub fn assert_allowed_download_url(value: &str) -> Result<(), String> {
    if is_allowed_download_url(value) {
        Ok(())
    } else {
        Err("Update URL host is not allowed".into())
    }
}

pub fn parse_sha256_sums(text: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        if hash.len() != 64 || !hash.chars().all(|ch| ch.is_ascii_hexdigit()) {
            continue;
        }
        let Some(name) = parts.next() else {
            continue;
        };
        let name = name.trim_start_matches('*');
        if name.is_empty() {
            continue;
        }
        entries.push((hash.to_ascii_lowercase(), Path::new(name).file_name().and_then(|value| value.to_str()).unwrap_or(name).to_string()));
    }
    entries
}

pub fn published_sha256_for_asset(checksum_text: &str, asset_name: &str) -> Option<String> {
    let file_name = Path::new(asset_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(asset_name);
    parse_sha256_sums(checksum_text)
        .into_iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(file_name))
        .map(|(hash, _)| hash)
}

pub fn is_checksum_asset_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("sha256")
        || lower.contains("checksum")
        || lower == "sha256sums"
        || lower == "sha256sums.txt"
}

pub fn trusted_update_package_path(
    paths: &DataPaths,
    downloaded_path: &str,
    expected_sha256: &str,
) -> Result<PathBuf, String> {
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("Update package hash is missing or invalid".into());
    }
    let candidate = Path::new(downloaded_path);
    if !candidate.is_file() {
        return Err("Downloaded update package is missing".into());
    }
    let canonical = fs_guard::assert_update_file(paths, candidate)?;
    let actual = hex_sha256_file(&canonical)?;
    if actual != expected {
        return Err("Update package hash does not match the download record".into());
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::DataPaths;
    use std::fs;

    #[test]
    fn allows_only_https_github_hosts() {
        assert!(is_allowed_download_url(
            "https://api.github.com/repos/SELFEMO/ClipAnchor/releases"
        ));
        assert!(is_allowed_download_url(
            "https://objects.githubusercontent.com/github-production-release-asset/1"
        ));
        assert!(is_allowed_download_url(
            "https://github.com/SELFEMO/ClipAnchor/releases/download/v1/app.msi"
        ));
        assert!(!is_allowed_download_url("http://api.github.com/releases"));
        assert!(!is_allowed_download_url("https://evil.example/app.msi"));
        assert!(!is_allowed_download_url("https://github.com.evil.example/app.msi"));
        assert!(!is_allowed_download_url("https://user:pass@github.com/app.msi"));
        assert!(!is_allowed_download_url("https://api.github.com:8443/releases"));
    }

    #[test]
    fn parses_checksum_lines_for_matching_assets() {
        let text = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd  ClipAnchor_Windows_x64.msi\n# comment\n";
        assert_eq!(
            published_sha256_for_asset(text, "ClipAnchor_Windows_x64.msi").as_deref(),
            Some("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
        );
        assert!(published_sha256_for_asset(text, "other.exe").is_none());
    }

    #[test]
    fn install_path_must_live_under_updates_and_match_hash() {
        let root = std::env::temp_dir().join(format!(
            "clipanchor-update-trust-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
        let package = paths.data.join("updates").join("ClipAnchor.msi");
        fs::write(&package, b"installer-bytes").unwrap();
        let hash = hex_sha256(b"installer-bytes");
        assert!(trusted_update_package_path(&paths, &package.to_string_lossy(), &hash).is_ok());
        assert!(trusted_update_package_path(&paths, &package.to_string_lossy(), &hex_sha256(b"other")).is_err());

        let outsider = root.join("evil.msi");
        fs::write(&outsider, b"installer-bytes").unwrap();
        assert!(trusted_update_package_path(&paths, &outsider.to_string_lossy(), &hash).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
