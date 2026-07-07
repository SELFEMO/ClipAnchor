use crate::{app_log, models::UpdateStatusPayload, paths::DataPaths};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use tauri::{AppHandle, Emitter};
#[cfg(target_os = "macos")]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "macos")]
use std::process::Stdio;
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

pub fn startup_background_check(app: &AppHandle, paths: &DataPaths, started_in_lite_mode: bool, auto_update_enabled: bool) -> UpdateStatusPayload {
    if !auto_update_enabled {
        // 自动更新关闭时仍写入明确状态，是为了让设置页显示真实状态且启动阶段不再访问网络。
        // When auto update is disabled, an explicit status is saved so Settings shows the real state and startup never touches the network.
        let disabled = disabled_status("startup_background");
        let _ = save_status(paths, &disabled);
        app_log::info(paths, "update", "startup update check skipped because auto update is disabled");
        return disabled;
    }

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
        if !started_in_lite_mode && status.prompt_on_main_open {
            // 只有可操作的更新结果才主动通知前端，是为了避免启动后把“正在检查/无更新/无安装包”误弹成手动检查窗口。
            // Only actionable update results notify the frontend so startup never misopens a manual-check dialog for checking, no-update, or missing-package states.
            let _ = app_for_thread.emit("clipanchor-update-status", status);
        }
    });

    checking
}

pub fn main_open_check(paths: &DataPaths) -> UpdateStatusPayload {
    let mut status = read_status(paths).unwrap_or_else(|| idle_status("main_open"));
    if status.prompt_on_main_open && !is_promptable_update(&status) {
        // 旧状态文件可能来自上一轮手动检查或无安装包发布；读取时清掉提示位，避免每次进入主界面都重复弹窗。
        // Old status files may come from a previous manual check or a release without packages; clearing the prompt bit prevents repeated dialogs on every main-window entry.
        status.prompt_on_main_open = false;
        status.attention_required = false;
        let _ = save_status(paths, &status);
    }
    app_log::info(paths, "update", format!("main window update status loaded; status={}", status.status));
    status
}

pub fn dismiss_prompt(paths: &DataPaths) -> Result<UpdateStatusPayload, String> {
    let mut status = read_status(paths).unwrap_or_else(|| idle_status("dismiss"));
    // “稍后提醒”只影响主动弹窗，不清除可安装更新状态，这样检查更新按钮仍能显示红点并继续安装。
    // Later reminders affect only proactive dialogs, not the installable update state, so the update button can keep its dot and still install.
    status.prompt_on_main_open = false;
    if !is_promptable_update(&status) {
        status.attention_required = false;
    }
    save_status(paths, &status)?;
    Ok(status)
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

pub fn install_downloaded_update(app: &AppHandle, paths: &DataPaths) -> Result<UpdateStatusPayload, String> {
    let mut status = read_status(paths).ok_or_else(|| "No downloaded update is available".to_string())?;
    let mut quit_after_launch = false;
    let local_path = status.downloaded_path.clone().unwrap_or_default();
    if !local_path.trim().is_empty() && Path::new(&local_path).exists() {
        quit_after_launch = open_installer_path(app, paths, Path::new(&local_path))?;
    } else if let Some(url) = status.asset_url.clone().filter(|value| !value.trim().is_empty()) {
        open_external_url(&url)?;
    } else {
        return Err("No installer package or release URL is available".into());
    }

    // macOS 的 DMG 更新需要由独立脚本在本进程退出后覆盖 .app，否则正在运行的二进制会锁住自身包内容。
    // macOS DMG updates are handed to a helper script after this process exits because a running binary can lock its own app bundle.
    status.status = "installing".into();
    status.prompt_on_main_open = false;
    status.attention_required = false;
    status.message = Some(if quit_after_launch { "macos_dmg_auto_install_started" } else { "installer_opened" }.into());
    status.checked_at = now_string();
    let _ = save_status(paths, &status);
    app_log::info(paths, "update", if quit_after_launch { "macOS DMG auto installer launched" } else { "installer opened for downloaded update" });
    if quit_after_launch {
        app.exit(0);
    }
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
        status.prompt_on_main_open = false;
        status.message = Some("up_to_date".into());
        return status;
    };

    let mut status = base_status("update_available", source);
    status.service_enabled = true;
    status.update_available = true;
    status.prompt_on_main_open = false;
    status.attention_required = false;
    status.current_version = Some(current_version.clone());
    status.latest_version = Some(selected.latest_version.clone());
    status.release_tag = Some(selected.release_tag.clone());
    status.release_name = Some(selected.release_name.clone());
    status.release_notes = Some(selected.release_notes.clone());

    let Some(asset) = selected.asset.clone() else {
        app_log::warn(paths, "update", format!("no compatible release asset found for tag {}", selected.release_tag));
        // 新 tag 没有当前系统安装包时不能进入可安装状态，是为了防止“发现更新”后按钮没有有效更新包可打开。
        // A new tag without a package for the current system must not become installable, preventing update prompts with no valid installer to open.
        status.status = "asset_unavailable".into();
        status.update_failed = true;
        status.prompt_on_main_open = false;
        status.attention_required = false;
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
            status.prompt_on_main_open = should_prompt_after_background_check(source, interactive);
            status.attention_required = true;
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
            status.prompt_on_main_open = should_prompt_after_background_check(source, interactive);
            status.attention_required = should_prompt_after_background_check(source, interactive);
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
    if !asset_arch_compatible(&name, arch) {
        return None;
    }
    if arch_matches(&name, arch) {
        score += 110;
    } else if platform == &PlatformKind::Macos && name.contains("universal") {
        score += 70;
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

#[cfg(target_os = "windows")]
fn spawn_silent_child_process(mut command: Command) -> Result<(), String> {
    // Windows 安装器需要通过隐藏子进程启动，限定平台编译可以避免 macOS/Linux 将该辅助函数报告为未使用。
    // Windows installers must be launched through a hidden child process, and platform-gating this helper prevents macOS/Linux from reporting it as unused.
    configure_silent_child_process(&mut command);
    command.spawn().map(|_| ()).map_err(|error| error.to_string())
}

fn configure_silent_child_process(command: &mut Command) {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // 更新检查会在后台触发 PowerShell 或 curl 兜底下载器，隐藏子进程控制台是为了保证后台任务不打断用户当前操作。
        // The updater may invoke PowerShell or curl as fallback downloaders in the background, so hiding child consoles keeps background work from interrupting the user.
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn open_installer_path(app: &AppHandle, paths: &DataPaths, path: &Path) -> Result<bool, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        let _ = paths;
    }

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
        return Ok(false);
    }

    #[cfg(target_os = "macos")]
    {
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase();
        if extension == "dmg" {
            install_macos_dmg_update(app, paths, path)?;
            return Ok(true);
        }
        Command::new("open").arg(path).spawn().map_err(|error| error.to_string())?;
        return Ok(false);
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(path).spawn().map_err(|error| error.to_string())?;
        return Ok(false);
    }
}

#[cfg(target_os = "macos")]
fn install_macos_dmg_update(app: &AppHandle, paths: &DataPaths, dmg_path: &Path) -> Result<(), String> {
    let _ = app;
    let running_app = current_macos_app_bundle().ok_or_else(|| "Cannot locate the running ClipAnchor.app bundle for automatic DMG installation".to_string())?;
    let target_app = macos_install_target_app_bundle(&running_app);
    let update_dir = paths.data.join(UPDATE_DIR);
    fs::create_dir_all(&update_dir).map_err(|error| error.to_string())?;
    let helper_path = update_dir.join("copy_macos_update.sh");
    let script_path = update_dir.join("apply_macos_update.sh");
    let log_path = paths.logs.join("macos-update.log");

    fs::write(&helper_path, macos_copy_helper_script()).map_err(|error| error.to_string())?;
    fs::set_permissions(&helper_path, fs::Permissions::from_mode(0o755)).map_err(|error| error.to_string())?;
    fs::write(
        &script_path,
        macos_dmg_installer_script(dmg_path, &target_app, &helper_path, &log_path, std::process::id()),
    ).map_err(|error| error.to_string())?;
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).map_err(|error| error.to_string())?;

    // 更新脚本必须脱离当前进程组运行，是为了在 ClipAnchor 退出后继续完成挂载、覆盖和重启，不会被父进程退出顺带中断。
    // The updater script must run detached from the current process group so mounting, replacement, and restart continue after ClipAnchor exits.
    let mut command = Command::new("/usr/bin/nohup");
    command
        .arg("/bin/sh")
        .arg(&script_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().map(|_| ()).map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn current_macos_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()).map(|value| value.eq_ignore_ascii_case("app")).unwrap_or(false))
        .map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn macos_install_target_app_bundle(running_app: &Path) -> PathBuf {
    if running_app.starts_with("/Volumes/") {
        // 从 DMG 只读卷直接运行时不能覆盖自身；改为覆盖 /Applications 中同名应用，避免在挂载卷里复制失败或留下多个测试副本。
        // When running directly from a read-only DMG volume, replacing itself is impossible; targeting /Applications avoids copy failures and duplicate test bundles.
        if let Some(name) = running_app.file_name() {
            return PathBuf::from("/Applications").join(name);
        }
    }
    running_app.to_path_buf()
}

#[cfg(target_os = "macos")]
fn macos_copy_helper_script() -> &'static str {
    r#"#!/bin/sh
set -eu
SOURCE_APP="$1"
TARGET_APP="$2"
rm -rf "$TARGET_APP"
ditto "$SOURCE_APP" "$TARGET_APP"
xattr -dr com.apple.quarantine "$TARGET_APP" 2>/dev/null || true
"#
}

#[cfg(target_os = "macos")]
fn macos_dmg_installer_script(dmg_path: &Path, target_app: &Path, helper_path: &Path, log_path: &Path, app_pid: u32) -> String {
    let target_name = target_app
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ClipAnchor.app");
    format!(
        r###"#!/bin/sh
set -u
DMG_PATH={dmg}
TARGET_APP={target}
TARGET_NAME={target_name}
HELPER_SCRIPT={helper}
LOG_FILE={log}
APP_PID={pid}
MOUNT_POINT="$(mktemp -d /tmp/clipanchor-update.XXXXXX)"

log() {{
  printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$1" >> "$LOG_FILE" 2>/dev/null || true
}}

shell_quote() {{
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}}

applescript_quote() {{
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}}

cleanup() {{
  hdiutil detach "$MOUNT_POINT" -quiet >/dev/null 2>&1 || true
  rmdir "$MOUNT_POINT" >/dev/null 2>&1 || true
}}
trap cleanup EXIT

mkdir -p "$(dirname "$LOG_FILE")"
log "waiting for ClipAnchor to quit"
# 必须等当前进程完全退出后再覆盖 .app，否则 macOS 可能锁住包内二进制，导致复制失败或生成并存的重复应用。
# The running process must fully exit before replacing the .app, otherwise macOS can keep bundle binaries locked and cause copy failures or duplicate app bundles.
WAIT_INDEX=0
while kill -0 "$APP_PID" >/dev/null 2>&1; do
  WAIT_INDEX=$((WAIT_INDEX + 1))
  if [ "$WAIT_INDEX" -gt 120 ]; then
    log "ClipAnchor process did not exit in time; aborting update to avoid partial overwrite"
    exit 1
  fi
  sleep 0.25
done
log "ClipAnchor exited; attaching DMG: $DMG_PATH"
hdiutil attach "$DMG_PATH" -nobrowse -quiet -mountpoint "$MOUNT_POINT"
SOURCE_APP="$(find "$MOUNT_POINT" -maxdepth 2 -name "$TARGET_NAME" -type d | head -n 1)"
if [ -z "$SOURCE_APP" ]; then
  SOURCE_APP="$(find "$MOUNT_POINT" -maxdepth 2 -name '*.app' -type d | head -n 1)"
fi
if [ -z "$SOURCE_APP" ]; then
  log "no app bundle found in DMG"
  exit 1
fi
log "copying $SOURCE_APP to $TARGET_APP"
if ! "$HELPER_SCRIPT" "$SOURCE_APP" "$TARGET_APP" >> "$LOG_FILE" 2>&1; then
  log "direct copy failed; requesting administrator privileges"
  HELPER_CMD="$HELPER_SCRIPT $(shell_quote "$SOURCE_APP") $(shell_quote "$TARGET_APP")"
  osascript -e "do shell script \"$(applescript_quote "$HELPER_CMD")\" with administrator privileges" >> "$LOG_FILE" 2>&1
fi
if [ ! -d "$TARGET_APP" ]; then
  log "target app is missing after copy; aborting restart"
  exit 1
fi
/usr/bin/touch "$TARGET_APP" >/dev/null 2>&1 || true
if [ -x /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister ]; then
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$TARGET_APP" >> "$LOG_FILE" 2>&1 || true
fi
EXECUTABLE_NAME="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$TARGET_APP/Contents/Info.plist" 2>/dev/null || printf 'clipanchor')"
EXECUTABLE_PATH="$TARGET_APP/Contents/MacOS/$EXECUTABLE_NAME"
log "opening updated app: $TARGET_APP"
# 更新完成后必须主动重启新版应用；优先使用 LaunchServices，失败时再直接启动包内可执行文件，避免出现“安装完成但软件没有重新打开”的情况。
# The updated app must be restarted explicitly after replacement; LaunchServices is tried first, then the bundle executable is used as a fallback so the update never appears to finish without reopening.
/usr/bin/open -n "$TARGET_APP" >> "$LOG_FILE" 2>&1 || true
RESTART_INDEX=0
while [ "$RESTART_INDEX" -lt 20 ]; do
  if pgrep -f "$TARGET_APP/Contents/MacOS" >/dev/null 2>&1 || pgrep -x "$EXECUTABLE_NAME" >/dev/null 2>&1; then
    log "updated app is running"
    exit 0
  fi
  RESTART_INDEX=$((RESTART_INDEX + 1))
  sleep 0.5
done
if [ -x "$EXECUTABLE_PATH" ]; then
  log "LaunchServices did not report a running app; starting executable fallback: $EXECUTABLE_PATH"
  /usr/bin/nohup "$EXECUTABLE_PATH" >/dev/null 2>&1 &
else
  log "cannot find executable fallback at $EXECUTABLE_PATH"
  exit 1
fi
"###,
        dmg = shell_quote(&dmg_path.to_string_lossy()),
        target = shell_quote(&target_app.to_string_lossy()),
        target_name = shell_quote(target_name),
        helper = shell_quote(&helper_path.to_string_lossy()),
        log = shell_quote(&log_path.to_string_lossy()),
        pid = app_pid,
    )
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("'{}'", escaped)
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

fn asset_arch_compatible(name: &str, arch: &str) -> bool {
    let has_known_arch = contains_any(name, &["arm64", "aarch64", "x64", "x86_64", "amd64", "ia32", "i386"]);
    if !has_known_arch || name.contains("universal") {
        return true;
    }
    // 带架构后缀的安装包必须严格匹配当前架构，是为了避免 Apple Silicon 误下载 Intel 包或反过来造成更新失败。
    // Installers with architecture suffixes must match the current architecture so Apple Silicon never downloads an Intel package, or vice versa.
    arch_matches(name, arch)
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

#[cfg(target_os = "windows")]
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

fn failed_status(source: &str, _prompt: bool, message: &str) -> UpdateStatusPayload {
    let mut status = base_status("update_failed", source);
    status.service_enabled = true;
    status.update_failed = true;
    // 普通网络失败不弹窗，是为了让启动检查真正静默；手动检查的错误会通过当前已打开的检查窗口展示。
    // Plain network failures do not prompt so startup checks stay silent; manual-check errors are shown in the already-open check window.
    status.prompt_on_main_open = false;
    status.attention_required = false;
    status.message = Some(message.into());
    status
}

fn should_prompt_after_background_check(source: &str, interactive: bool) -> bool {
    source == "startup_background" && !interactive
}

fn is_promptable_update(status: &UpdateStatusPayload) -> bool {
    if status.status == "asset_unavailable" {
        return false;
    }
    status.update_available && (status.install_ready || non_empty_option(status.asset_url.as_ref()))
}

fn non_empty_option(value: Option<&String>) -> bool {
    value.map(|value| !value.trim().is_empty()).unwrap_or(false)
}

fn idle_status(source: &str) -> UpdateStatusPayload {
    base_status("idle", source)
}

fn disabled_status(source: &str) -> UpdateStatusPayload {
    let mut status = base_status("disabled", source);
    status.message = Some("auto_update_disabled".into());
    status.current_version = Some(current_version());
    status
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
