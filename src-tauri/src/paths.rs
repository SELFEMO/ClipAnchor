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

#[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
fn resolve_data_dir(root: &Path) -> Result<PathBuf, String> {
    // 非 macOS/Linux 平台继续使用可执行文件同级 data 目录，保持便携包的既有行为。
    // Non-macOS/Linux platforms keep using the data directory beside the executable to preserve portable-package behavior.
    Ok(root.join("data"))
}

#[cfg(target_os = "linux")]
fn resolve_data_dir(root: &Path) -> Result<PathBuf, String> {
    if portable_mode_requested() {
        // 用户明确要求便携模式时必须坚持软件同级 data 目录，即使系统包安装目录不可写也应暴露真实错误。
        // An explicit portable request must keep data beside the app, even if a system package directory is not writable, so the real permission issue is visible.
        return Ok(root.join("data"));
    }

    let portable_data = root.join("data");
    if directory_is_writable(&portable_data) {
        // 开发环境和解压式 Linux 包通常位于用户可写目录；优先使用同级 data 才能保留真正的便携体验。
        // Development and unpacked Linux builds usually live in writable user folders, so the sibling data directory remains the first choice for portability.
        return Ok(portable_data);
    }

    if let Ok(custom) = env::var("CLIPANCHOR_DATA_DIR") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    // deb/rpm 安装后的可执行文件通常位于 /usr/bin 或 /usr/lib，普通用户无法写入同级 data；回退到用户数据目录可避免 SQLite/settings 初始化失败。
    // After deb/rpm installation the executable usually lives under /usr/bin or /usr/lib, where normal users cannot write sibling data; falling back prevents SQLite/settings startup failures.
    linux_user_data_dir()
}

#[cfg(target_os = "linux")]
fn portable_mode_requested() -> bool {
    env::args().any(|arg| arg == "--portable")
        || env::var("CLIPANCHOR_PORTABLE").map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES")).unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn directory_is_writable(path: &Path) -> bool {
    if fs::create_dir_all(path).is_err() {
        return false;
    }
    let probe = path.join(".clipanchor-write-test");
    match fs::write(&probe, b"ok") {
        Ok(_) => {
            let _ = fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn linux_user_data_dir() -> Result<PathBuf, String> {
    if let Ok(value) = env::var("XDG_DATA_HOME") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(Path::new(trimmed).join("ClipAnchor").join("data"));
        }
    }
    let home = env::var("HOME").map_err(|error| error.to_string())?;
    Ok(Path::new(&home).join(".local/share/ClipAnchor/data"))
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


pub fn configure_webview_storage(paths: &DataPaths) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let webview_data = paths.data.join("webview2");
        fs::create_dir_all(&webview_data).map_err(|error| error.to_string())?;
        // WebView2 默认把 HTTP 缓存写入 LocalAppData；在创建任何 WebView 之前覆盖 UDF，是为了把 f_* 缓存块与其他运行数据统一收敛到 data/。
        // WebView2 writes its HTTP cache under LocalAppData by default; overriding the UDF before any WebView is created keeps f_* cache blocks together with the other runtime data under data/.
        env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview_data);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = paths;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn webview_storage_path(paths: &DataPaths) -> PathBuf {
    paths.data.join("webview2")
}

pub fn ensure(paths: &DataPaths) -> Result<(), String> {
    for path in [&paths.data, &paths.resources, &paths.exports, &paths.logs] {
        fs::create_dir_all(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}
