use std::{collections::HashSet, env, fs, path::{Path, PathBuf}};

#[derive(Clone, Debug)]
pub struct DataPaths {
    pub root: PathBuf,
    pub data: PathBuf,
    pub database: PathBuf,
    pub settings: PathBuf,
    pub resources: PathBuf,
    pub exports: PathBuf,
    pub locales: PathBuf,
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
        locales: data.join("locales"),
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
    if !persistent_has_settings && !persistent_has_database {
        // macOS 更新会整体替换 .app；首次启动新版时把旧包内 data 迁移到 Application Support，才能从根源上避免设置随应用覆盖而丢失。
        // macOS updates replace the whole .app bundle; migrating legacy in-bundle data to Application Support on first launch prevents settings from being lost during replacement.
        copy_dir_contents(&legacy_data, persistent_data)?;
    } else {
        // 已存在数据库时仍单独迁移新增语言包，避免用户放入旧 data/locales 的 JSON 因“主数据已迁移”而被跳过。
        // Even when the database already exists, migrate newly added language packs so JSON files placed in legacy data/locales are not skipped after the main data migration.
        let legacy_locales = legacy_data.join("locales");
        if legacy_locales.exists() {
            copy_dir_contents(&legacy_locales, &persistent_data.join("locales"))?;
        }
    }
    Ok(())
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



pub fn sync_language_pack_sources(paths: &DataPaths, resource_dir: Option<&Path>) -> Result<usize, String> {
    fs::create_dir_all(&paths.locales).map_err(|error| error.to_string())?;

    let mut candidates: Vec<(PathBuf, bool)> = Vec::new();
    if let Some(resources) = resource_dir {
        // Linux 包管理器与 Tauri 的资源映射可能把上级资源放在 extension-locales 或 _up_/extension-locales；同时探测两种布局可避免安装版漏掉内置语言。
        // Linux packages and Tauri resource mapping may place parent resources under extension-locales or _up_/extension-locales; probing both layouts keeps bundled languages discoverable.
        candidates.push((resources.join("extension-locales"), false));
        candidates.push((resources.join("_up_").join("extension-locales"), false));
    }
    candidates.push((paths.root.join("extension-locales"), false));
    candidates.push((paths.root.join("data/locales"), true));

    if let Ok(current_dir) = env::current_dir() {
        candidates.push((current_dir.join("extension-locales"), false));
        candidates.push((current_dir.join("data/locales"), true));
        if current_dir.file_name().and_then(|value| value.to_str()) == Some("src-tauri") {
            if let Some(project_root) = current_dir.parent() {
                candidates.push((project_root.join("extension-locales"), false));
                candidates.push((project_root.join("data/locales"), true));
            }
        }
    }

    let destination_key = normalized_existing_path(&paths.locales);
    let mut visited = HashSet::new();
    let mut synchronized = 0usize;
    for (source, refresh_existing) in candidates {
        if !source.is_dir() {
            continue;
        }
        let source_key = normalized_existing_path(&source);
        if source_key == destination_key || !visited.insert(source_key) {
            continue;
        }

        // Linux 的 DEB/RPM 资源映射可能额外保留一层源目录；有限深度递归可兼容该布局，同时避免遍历整个安装树。
        // Linux DEB/RPM resource mappings may retain an extra source-directory level; bounded recursion supports that layout without walking the entire installation tree.
        for source_file in collect_language_json_files(&source, 3) {
            let Some(file_name) = source_file.file_name() else {
                continue;
            };
            let target = paths.locales.join(file_name);
            if target.exists() {
                if !refresh_existing && !should_refresh_reviewed_language_pack(&source_file, &target) {
                    // 普通随包语言只补齐缺失文件；只有经过人工校正且目标包没有手动修改时才更新，避免覆盖用户维护的翻译。
                    // Ordinary bundled languages only fill missing files; a reviewed pack refreshes an existing file only when that target has no manual edits, preventing user-maintained translations from being overwritten.
                    continue;
                }
                let source_bytes = match fs::read(&source_file) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                };
                let target_matches = fs::read(&target).map(|bytes| bytes == source_bytes).unwrap_or(false);
                if target_matches {
                    continue;
                }
                // 开发目录、旧便携目录或更晚的人工校正包需要刷新活动副本；前面的保护条件已经排除了带手动修改标记的用户翻译。
                // Development directories, legacy portable sources, and newer reviewed packs refresh the active copy; the guard above has already excluded user translations marked as manually edited.
                fs::write(&target, source_bytes).map_err(|error| {
                    format!(
                        "Cannot refresh language file {} from {}: {}",
                        target.display(),
                        source_file.display(),
                        error
                    )
                })?;
                synchronized += 1;
                continue;
            }

            fs::copy(&source_file, &target).map_err(|error| {
                format!(
                    "Cannot copy language file {} to {}: {}",
                    source_file.display(),
                    target.display(),
                    error
                )
            })?;
            synchronized += 1;
        }
    }
    Ok(synchronized)
}


fn should_refresh_reviewed_language_pack(source: &Path, target: &Path) -> bool {
    let source_value = match fs::read(source)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    {
        Some(value) => value,
        None => return false,
    };
    let target_value = match fs::read(target)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    {
        Some(value) => value,
        None => return false,
    };

    let source_provider = source_value
        .get("source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !source_provider.starts_with("ClipAnchor reviewed language pack") {
        return false;
    }

    let source_code = source_value
        .get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let target_code = target_value
        .get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if source_code.is_empty() || !source_code.eq_ignore_ascii_case(target_code) {
        return false;
    }

    let target_provider = target_value
        .get("source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let generated_target = target_provider.starts_with("UAPI translate API")
        || target_provider.starts_with("MyMemory")
        || target_provider.starts_with("ClipAnchor reviewed language pack");
    if !generated_target {
        return false;
    }

    let statuses = match target_value
        .get("message_status")
        .and_then(serde_json::Value::as_object)
    {
        Some(statuses) if !statuses.is_empty() => statuses,
        _ => return false,
    };
    if statuses.values().any(|status| {
        status
            .get("modified")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }) {
        return false;
    }

    let source_generated_at = source_value
        .get("generated_at")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let target_generated_at = target_value
        .get("generated_at")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    // ISO 8601 UTC 时间可以按字符串比较；仅接受更新的人工校正包，避免每次启动重复写入相同文件。
    // ISO 8601 UTC timestamps are lexically sortable; accepting only a newer reviewed pack avoids rewriting the same file on every startup.
    source_generated_at > target_generated_at
}

fn collect_language_json_files(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() && depth < max_depth {
                pending.push((path, depth + 1));
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let is_json = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("json"))
                .unwrap_or(false);
            if is_json {
                files.push(path);
            }
        }
    }
    files.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    files
}

fn normalized_existing_path(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
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
    for path in [&paths.data, &paths.resources, &paths.exports, &paths.locales, &paths.logs] {
        fs::create_dir_all(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}
