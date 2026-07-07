use std::{env, fs, path::{Path, PathBuf}};

#[derive(Clone, Debug)]
pub struct DataPaths {
    pub root: PathBuf,
    pub data: PathBuf,
    pub database: PathBuf,
    pub settings: PathBuf,
    pub resources: PathBuf,
    pub exports: PathBuf,
    pub logs: PathBuf,
}

pub fn resolve() -> Result<DataPaths, String> {
    let exe = env::current_exe().map_err(|error| error.to_string())?;
    let root = exe.parent().ok_or_else(|| "Cannot resolve executable directory".to_string())?.to_path_buf();
    let data = resolve_data_dir(&root)?;

    Ok(DataPaths {
        root,
        database: data.join("clipanchor.db"),
        settings: data.join("settings.json"),
        resources: data.join("resources"),
        exports: data.join("exports"),
        logs: data.join("logs"),
        data,
    })
}

#[cfg(target_os = "macos")]
fn resolve_data_dir(root: &Path) -> Result<PathBuf, String> {
    let home = env::var("HOME").map_err(|error| error.to_string())?;
    let data = Path::new(&home).join("Library/Application Support/ClipAnchor");
    migrate_macos_bundle_data(root, &data)?;
    Ok(data)
}

#[cfg(not(target_os = "macos"))]
fn resolve_data_dir(root: &Path) -> Result<PathBuf, String> {
    // 非 macOS 平台继续使用可执行文件同级 data 目录，保持 Windows/Linux 便携包的既有行为。
    // Non-macOS platforms keep using the data directory beside the executable to preserve the existing Windows/Linux portable behavior.
    Ok(root.join("data"))
}

#[cfg(target_os = "macos")]
fn migrate_macos_bundle_data(root: &Path, persistent_data: &Path) -> Result<(), String> {
    let legacy_data = root.join("data");
    if !legacy_data.exists() || legacy_data == persistent_data {
        return Ok(());
    }

    fs::create_dir_all(persistent_data).map_err(|error| error.to_string())?;
    let persistent_has_settings = persistent_data.join("settings.json").exists();
    let persistent_has_database = persistent_data.join("clipanchor.db").exists();
    if persistent_has_settings || persistent_has_database {
        return Ok(());
    }

    // macOS 更新会整体替换 .app；首次启动新版时把旧包内 data 迁移到 Application Support，才能从根源上避免设置随应用覆盖而丢失。
    // macOS updates replace the whole .app bundle; migrating legacy in-bundle data to Application Support on first launch prevents settings from being lost during replacement.
    copy_dir_contents(&legacy_data, persistent_data)
}

#[cfg(target_os = "macos")]
fn copy_dir_contents(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(from).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            copy_dir_contents(&source, &target)?;
        } else if metadata.is_file() && !target.exists() {
            fs::copy(&source, &target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn ensure(paths: &DataPaths) -> Result<(), String> {
    for path in [&paths.data, &paths.resources, &paths.exports, &paths.logs] {
        fs::create_dir_all(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}
