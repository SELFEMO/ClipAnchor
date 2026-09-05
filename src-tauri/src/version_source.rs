#[cfg(test)]
mod tests {
    use serde_json::Value;
    use std::{fs, path::PathBuf};

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn cargo_package_version(text: &str) -> &str {
        let mut in_package = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_package = trimmed == "[package]";
                continue;
            }
            if !in_package {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("version") {
                let value = rest.trim().trim_start_matches('=').trim().trim_matches('"');
                return value;
            }
        }
        panic!("src-tauri/Cargo.toml is missing [package].version");
    }

    #[test]
    fn cargo_toml_is_the_only_product_version() {
        let cargo_text = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        let cargo_version = cargo_package_version(cargo_text);
        assert_eq!(cargo_version, env!("CARGO_PKG_VERSION"));

        let tauri_conf: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tauri.conf.json"
        )))
        .unwrap();
        assert!(
            tauri_conf.get("version").is_none(),
            "tauri.conf.json must omit version so the installer inherits Cargo.toml"
        );

        let package: Value = serde_json::from_str(
            &fs::read_to_string(repo_root().join("package.json")).expect("package.json"),
        )
        .unwrap();
        assert_eq!(package["version"].as_str(), Some(cargo_version));

        let lock: Value = serde_json::from_str(
            &fs::read_to_string(repo_root().join("package-lock.json")).expect("package-lock.json"),
        )
        .unwrap();
        assert_eq!(lock["version"].as_str(), Some(cargo_version));
        assert_eq!(lock["packages"][""]["version"].as_str(), Some(cargo_version));
    }
}
