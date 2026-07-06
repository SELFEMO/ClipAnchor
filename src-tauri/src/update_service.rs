use crate::{app_log, models::UpdateStatusPayload, paths::DataPaths};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use tauri::{AppHandle, Emitter};
use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
};

const UPDATE_STATUS_FILE: &str = "update-status.json";
const UPDATE_DIR: &str = "updates";
const RELEASE_API_URL: &str = "https://api.github.com/repos/SELFEMO/ClipAnchor/releases";
const DOWNLOAD_USER_AGENT: &str = "ClipAnchor-Updater";

#[derive(Clone, Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    draft: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
}

#[derive(Clone, Debug)]
struct SelectedRelease {
    latest_version: String,
    release_tag: String,
    release_name: String,
    release_notes: String,
    asset: Option<GitHubAsset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PlatformKind {
    Windows,
    Macos,
    Linux,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LinuxPackageKind {
    Deb,
    Rpm,
}

pub fn startup_background_check(app: &AppHandle, paths: &DataPaths, started_in_lite_mode: bool) -> UpdateStatusPayload {
    // 启动检查放入后台线程，是为了让自启动轻量模式和普通启动都不被 GitHub 网络延迟阻塞。
    // Startup checks run in a background thread so GitHub network latency never blocks Lite startup or normal launch.
    let _ = clear_update_packages(paths);
    let checking = checking_status("startup_background");
    let _ = save_status(paths, &checking);
    app_log::info(
        paths,
        "update",
        format!(
            "startup update check scheduled silently; lite_mode={}",
            started_in_lite_mode
        ),
    );

    let app_for_thread = app.clone();
    let paths_for_thread = paths.clone();
    thread::spawn(move || {
        let status = perform_update_check(&paths_for_thread, "startup_background", true, false);
        let _ = save_status(&paths_for_thread, &status);
        if !started_in_lite_mode && status.update_available {
            // 普通手动启动时发现新版本要主动通知前端，是为了避免启动检查已经完成但用户看不到更新提示。
            // A normal manual startup emits the update result so users see the prompt even when the check finishes after the main UI has loaded.
            let _ = app_for_thread.emit("clipanchor-update-status", status);
        }
    });

    checking
}

pub fn main_open_check(paths: &DataPaths) -> UpdateStatusPayload {
    let status = read_status(paths).unwrap_or_else(|| idle_status("main_open"));
    app_log::info(paths, "update", format!("main window update status loaded; status={}", status.status));
    status
}

pub fn manual_check(paths: &DataPaths, source: &str) -> UpdateStatusPayload {
    // 每次重新检查前清理旧安装包，是为了避免旧版本残留被误认为当前可安装更新。
    // Old installers are removed before every new check so stale packages cannot be mistaken for the currently selected update.
    if let Err(error) = clear_update_packages(paths) {
        app_log::warn(paths, "update", format!("old update package cleanup failed: {}", error));
    }
    // 手动检查也放入后台线程，是为了让前端立即显示更新页面，并通过轮询看到检查与下载阶段变化。
    // Manual checks also run in a background thread so the frontend opens the update page immediately and polls checking/download state changes.
    let checking = checking_status(source);
    let _ = save_status(paths, &checking);
    app_log::info(paths, "update", format!("manual update check scheduled; source={}", source));

    let paths_for_thread = paths.clone();
    let source_for_thread = source.to_string();
    thread::spawn(move || {
        let status = perform_update_check(&paths_for_thread, &source_for_thread, true, true);
        let _ = save_status(&paths_for_thread, &status);
    });

    checking
}

pub fn install_downloaded_update(paths: &DataPaths) -> Result<UpdateStatusPayload, String> {
    let mut status = read_status(paths).ok_or_else(|| "No downloaded update is available".to_string())?;
    let local_path = status.downloaded_path.clone().unwrap_or_default();
    if !local_path.trim().is_empty() && Path::new(&local_path).exists() {
        open_installer_path(Path::new(&local_path))?;
    } else if let Some(url) = status.asset_url.clone().filter(|value| !value.trim().is_empty()) {
        open_external_url(&url)?;
    } else {
        return Err("No installer package or release URL is available".into());
    }

    // 安装动作由系统安装器接管，是为了避免 ClipAnchor 在运行中直接替换自身可执行文件导致平台权限和文件锁问题。
    // Installation is handed to the system installer so ClipAnchor does not replace its own executable while platform permissions and file locks are active.
    status.status = "installing".into();
    status.prompt_on_main_open = false;
    status.attention_required = false;
    status.message = Some("installer_opened".into());
    status.checked_at = now_string();
    let _ = save_status(paths, &status);
    app_log::info(paths, "update", "installer opened for downloaded update");
    Ok(status)
}

fn perform_update_check(paths: &DataPaths, source: &str, auto_download: bool, interactive: bool) -> UpdateStatusPayload {
    let current_version = current_version();
    let releases_text = match fetch_url_text(RELEASE_API_URL) {
        Ok(text) => text,
        Err(error) => {
            app_log::warn(paths, "update", format!("release check failed: {}", error));
            let mut status = failed_status(source, interactive, "release_check_failed");
            status.current_version = Some(current_version);
            return status;
        }
    };

    let releases = match serde_json::from_str::<Vec<GitHubRelease>>(&releases_text) {
        Ok(value) => value,
        Err(error) => {
            app_log::warn(paths, "update", format!("release response could not be parsed: {}", error));
            let mut status = failed_status(source, interactive, "release_metadata_invalid");
            status.current_version = Some(current_version);
            return status;
        }
    };

    let selected = select_newer_release(&releases, &current_version);
    let Some(selected) = selected else {
        app_log::info(paths, "update", format!("no update found; current={}", current_version));
        let mut status = base_status("no_update", source);
        status.service_enabled = true;
        status.current_version = Some(current_version);
        status.prompt_on_main_open = interactive;
        status.message = Some("up_to_date".into());
        return status;
    };

    let mut status = base_status("update_available", source);
    status.service_enabled = true;
    status.update_available = true;
    status.prompt_on_main_open = true;
    status.attention_required = true;
    status.current_version = Some(current_version.clone());
    status.latest_version = Some(selected.latest_version.clone());
    status.release_tag = Some(selected.release_tag.clone());
    status.release_name = Some(selected.release_name.clone());
    status.release_notes = Some(selected.release_notes.clone());

    let Some(asset) = selected.asset.clone() else {
        app_log::warn(paths, "update", format!("no compatible release asset found for tag {}", selected.release_tag));
        status.status = "asset_unavailable".into();
        status.update_failed = true;
        status.message = Some("asset_unavailable".into());
        return status;
    };

    status.asset_name = Some(asset.name.clone());
    status.asset_url = Some(asset.browser_download_url.clone());
    status.total_bytes = asset.size;

    if !auto_download {
        return status;
    }

    status.status = "downloading".into();
    status.message = Some("downloading_package".into());
    let _ = save_status(paths, &status);

    match download_asset(paths, &asset) {
        Ok(path) => {
            app_log::info(paths, "update", format!("update package downloaded: {}", path.to_string_lossy()));
            status.status = "downloaded".into();
            status.install_ready = true;
            status.downloaded_path = Some(path.to_string_lossy().to_string());
            status.downloaded_bytes = path.metadata().ok().map(|metadata| metadata.len());
            status.message = Some("package_ready".into());
            status
        }
        Err(error) => {
            app_log::warn(paths, "update", format!("update package download failed: {}", error));
            status.status = "update_failed".into();
            status.update_failed = true;
            status.install_ready = false;
            status.message = Some("download_failed".into());
            status
        }
    }
}

fn select_newer_release(releases: &[GitHubRelease], current_version: &str) -> Option<SelectedRelease> {
    // 发布标签只按 release-v / pre-release-v 中的语义版本比较，是为了兼容当前仓库的发布命名规则且不依赖额外配置文件。
    // Release tags are compared only by the semantic value inside release-v / pre-release-v so the current repository convention works without extra manifests.
    let mut candidates = releases
        .iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let latest_version = version_from_tag(&release.tag_name)?;
            if compare_versions(&latest_version, current_version) != Ordering::Greater {
                return None;
            }
            Some((release, latest_version))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|(_, left), (_, right)| compare_versions(right, left));
    let (release, latest_version) = candidates.into_iter().next()?;
    Some(SelectedRelease {
        latest_version,
        release_tag: release.tag_name.clone(),
        release_name: release.name.clone().unwrap_or_else(|| release.tag_name.clone()),
        release_notes: release.body.clone().unwrap_or_default(),
        asset: select_asset_for_current_system(&release.assets),
    })
}

fn select_asset_for_current_system(assets: &[GitHubAsset]) -> Option<GitHubAsset> {
    // 资产选择集中在后端评分，是为了让 Windows、macOS、Linux 共用一套规则，前端无需维护平台分支。
    // Asset selection is scored in the backend so Windows, macOS, and Linux share one rule set and the frontend needs no platform branches.
    let platform = current_platform();
    let arch = current_arch();
    let lang = system_language();
    let linux_package = preferred_linux_package();

    let mut ranked = assets
        .iter()
        .filter_map(|asset| asset_score(asset, &platform, &arch, &lang, &linux_package).map(|score| (score, asset.clone())))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0));
    ranked.into_iter().next().map(|(_, asset)| asset)
}

fn asset_score(asset: &GitHubAsset, platform: &PlatformKind, arch: &str, lang: &str, linux_package: &LinuxPackageKind) -> Option<i32> {
    let name = asset.name.to_lowercase();
    let extension = Path::new(&asset.name).extension()?.to_string_lossy().to_lowercase();
    let mut score = 0i32;

    match platform {
        PlatformKind::Windows => {
            if extension == "exe" {
                score += 700;
            } else if extension == "msi" {
                score += 500;
            } else {
                return None;
            }
            if contains_any(&name, &["windows", "win"]) {
                score += 120;
            }
            if extension == "msi" {
                if lang.starts_with("zh") && contains_any(&name, &["zh-cn", "zh_cn", "simpchinese", "chinese"]) {
                    score += 80;
                } else if !lang.starts_with("zh") && contains_any(&name, &["en-us", "en_us", "english"]) {
                    score += 80;
                }
            }
        }
        PlatformKind::Macos => {
            if extension != "dmg" {
                return None;
            }
            score += 650;
            if contains_any(&name, &["macos", "darwin", "osx", "mac"]) {
                score += 120;
            }
        }
        PlatformKind::Linux => {
            match (linux_package, extension.as_str()) {
                (LinuxPackageKind::Deb, "deb") => score += 650,
                (LinuxPackageKind::Rpm, "rpm") => score += 650,
                (_, "deb") | (_, "rpm") => score += 420,
                _ => return None,
            }
            if name.contains("linux") {
                score += 120;
            }
        }
    }

    if !asset.name.to_lowercase().contains("clipanchor") {
        score -= 30;
    }
    if arch_matches(&name, arch) {
        score += 90;
    }
    Some(score)
}

fn clear_update_packages(paths: &DataPaths) -> Result<(), String> {
    // 更新目录只保存可重新下载的安装包，因此检查前清空可以避免多个旧版本占用空间并干扰“立即更新”。
    // The update directory stores only re-downloadable installers, so clearing it before checks prevents old versions from wasting space or confusing Install Now.
    let dir = paths.data.join(UPDATE_DIR);
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
        } else {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn download_asset(paths: &DataPaths, asset: &GitHubAsset) -> Result<PathBuf, String> {
    // 更新包存入便携 data 目录，是为了保持项目“所有运行数据跟随软件根目录”的设计约束。
    // Update packages are stored under the portable data directory to preserve the project rule that runtime data stays beside the app.
    let dir = paths.data.join(UPDATE_DIR);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let target = dir.join(safe_asset_name(&asset.name));
    if target.exists() && target.metadata().map(|metadata| metadata.len()).unwrap_or(0) > 0 {
        return Ok(target);
    }
    download_url_to_path(&asset.browser_download_url, &target)?;
    if !target.exists() || target.metadata().map(|metadata| metadata.len()).unwrap_or(0) == 0 {
        return Err("Downloaded package is empty".into());
    }
    Ok(target)
}

fn fetch_url_text(url: &str) -> Result<String, String> {
    let mut attempts = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let escaped = powershell_quote(url);
        attempts.push(command_output(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &format!(
                    "$ProgressPreference='SilentlyContinue'; [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; (Invoke-WebRequest -UseBasicParsing -Uri '{}' -Headers @{{'User-Agent'='{}'}}).Content",
                    escaped, DOWNLOAD_USER_AGENT
                ),
            ],
        ));
    }

    attempts.push(command_output(
        "curl",
        &["-fsSL", "-A", DOWNLOAD_USER_AGENT, "-H", "Accept: application/vnd.github+json", url],
    ));

    attempts.into_iter().find_map(Result::ok).ok_or_else(|| "No supported HTTP downloader could read GitHub releases".into())
}

fn download_url_to_path(url: &str, target: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    let target_text = target.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        let escaped_url = powershell_quote(url);
        let escaped_target = powershell_quote(&target_text);
        match command_status(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &format!(
                    "$ProgressPreference='SilentlyContinue'; [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -UseBasicParsing -Uri '{}' -OutFile '{}' -Headers @{{'User-Agent'='{}'}}",
                    escaped_url, escaped_target, DOWNLOAD_USER_AGENT
                ),
            ],
        ) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(error),
        }
    }

    match command_status(
        "curl",
        &["-fL", "--silent", "--show-error", "-A", DOWNLOAD_USER_AGENT, "-o", &target_text, url],
    ) {
        Ok(()) => Ok(()),
        Err(error) => {
            errors.push(error);
            Err(errors.join("; "))
        }
    }
}

fn command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(program);
    command.args(args);
    configure_silent_child_process(&mut command);
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn command_status(program: &str, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(args);
    configure_silent_child_process(&mut command);
    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn spawn_silent_child_process(mut command: Command) -> Result<(), String> {
    configure_silent_child_process(&mut command);
    command.spawn().map(|_| ()).map_err(|error| error.to_string())
}

fn configure_silent_child_process(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // 更新检查会在后台触发 PowerShell 或 curl 兜底下载器，隐藏子进程控制台是为了保证后台任务不打断用户当前操作。
        // The updater may invoke PowerShell or curl as fallback downloaders in the background, so hiding child consoles keeps background work from interrupting the user.
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn open_installer_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase();
        if extension == "msi" {
            let mut command = Command::new("msiexec");
            command.arg("/i").arg(path);
            spawn_silent_child_process(command)?;
        } else {
            let command = Command::new(path);
            spawn_silent_child_process(command)?;
        }
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn().map_err(|error| error.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(path).spawn().map_err(|error| error.to_string())?;
        return Ok(());
    }
}

fn open_external_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        open_target_with_shell_execute(url)?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn().map_err(|error| error.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(url).spawn().map_err(|error| error.to_string())?;
        return Ok(());
    }
}

#[cfg(target_os = "windows")]
fn open_target_with_shell_execute(target: &str) -> Result<(), String> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    fn wide_null(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(std::iter::once(0)).collect()
    }

    let operation = wide_null("open");
    let target_wide = wide_null(target);
    // 打开外部链接交给 ShellExecute，而不是 cmd /C start，是为了避免“稍后打开发布页”也闪出命令行窗口。
    // External links are opened through ShellExecute instead of cmd /C start so release-page fallbacks never flash a console window.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            target_wide.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        ) as isize
    };
    if result <= 32 {
        Err(format!("System could not open the link ({})", result))
    } else {
        Ok(())
    }
}

fn current_platform() -> PlatformKind {
    if cfg!(target_os = "windows") {
        PlatformKind::Windows
    } else if cfg!(target_os = "macos") {
        PlatformKind::Macos
    } else {
        PlatformKind::Linux
    }
}

fn current_arch() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "x64".into(),
        "aarch64" => "arm64".into(),
        "x86" => "x86".into(),
        value => value.to_lowercase(),
    }
}

fn preferred_linux_package() -> LinuxPackageKind {
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default().to_lowercase();
    if contains_any(&os_release, &["fedora", "rhel", "centos", "suse", "rpm"]) {
        LinuxPackageKind::Rpm
    } else {
        LinuxPackageKind::Deb
    }
}

fn system_language() -> String {
    #[cfg(target_os = "windows")]
    {
        if let Some(value) = windows_locale_name() {
            return value.to_lowercase();
        }
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return value.to_lowercase();
            }
        }
    }
    "en-us".into()
}

#[cfg(target_os = "windows")]
fn windows_locale_name() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buffer = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 1 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..(len as usize - 1)]))
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn arch_matches(name: &str, arch: &str) -> bool {
    match arch {
        "x64" => contains_any(name, &["x64", "x86_64", "amd64"]),
        "arm64" => contains_any(name, &["arm64", "aarch64"]),
        "x86" => contains_any(name, &["x86", "ia32", "i386"]),
        other => name.contains(other),
    }
}

fn version_from_tag(tag: &str) -> Option<String> {
    for prefix in ["pre-release-v", "release-v", "v"] {
        if let Some(value) = tag.strip_prefix(prefix) {
            return Some(clean_version(value));
        }
    }
    tag.rsplit_once('v').map(|(_, value)| clean_version(value))
}

fn clean_version(value: &str) -> String {
    value
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>()
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left_parts = version_parts(left);
    let right_parts = version_parts(right);
    let length = left_parts.len().max(right_parts.len()).max(3);
    for index in 0..length {
        let left_value = left_parts.get(index).copied().unwrap_or(0);
        let right_value = right_parts.get(index).copied().unwrap_or(0);
        match left_value.cmp(&right_value) {
            Ordering::Equal => continue,
            order => return order,
        }
    }
    Ordering::Equal
}

fn version_parts(value: &str) -> Vec<u64> {
    value
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn safe_asset_name(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') { ch } else { '_' })
        .collect()
}

fn powershell_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn checking_status(source: &str) -> UpdateStatusPayload {
    let mut status = base_status("checking", source);
    status.service_enabled = true;
    status.current_version = Some(current_version());
    status.message = Some("checking".into());
    status
}

fn failed_status(source: &str, prompt: bool, message: &str) -> UpdateStatusPayload {
    let mut status = base_status("update_failed", source);
    status.service_enabled = true;
    status.update_failed = true;
    status.prompt_on_main_open = prompt;
    status.attention_required = prompt;
    status.message = Some(message.into());
    status
}

fn idle_status(source: &str) -> UpdateStatusPayload {
    base_status("idle", source)
}

fn base_status(status: &str, source: &str) -> UpdateStatusPayload {
    UpdateStatusPayload {
        status: status.into(),
        service_enabled: false,
        update_available: false,
        update_failed: false,
        prompt_on_main_open: false,
        attention_required: false,
        checked_at: now_string(),
        source: source.into(),
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

fn now_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn read_status(paths: &DataPaths) -> Option<UpdateStatusPayload> {
    let text = fs::read_to_string(status_path(paths)).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_status(paths: &DataPaths, status: &UpdateStatusPayload) -> Result<(), String> {
    fs::create_dir_all(&paths.data).map_err(|error| error.to_string())?;
    let text = serde_json::to_string_pretty(status).map_err(|error| error.to_string())?;
    fs::write(status_path(paths), text).map_err(|error| error.to_string())
}

fn status_path(paths: &DataPaths) -> PathBuf {
    paths.data.join(UPDATE_STATUS_FILE)
}
