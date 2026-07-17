use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

pub fn apply(enabled: bool, root: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return apply_windows(enabled, root);
    #[cfg(target_os = "macos")]
    return apply_macos(enabled, root);
    #[cfg(all(unix, not(target_os = "macos")))]
    return apply_linux(enabled, root);
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(target_os = "windows")]
const WINDOWS_RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(target_os = "windows")]
const WINDOWS_STARTUP_APPROVED_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
#[cfg(target_os = "windows")]
const CURRENT_RUN_VALUE: &str = "ClipAnchor";

pub fn reconcile(enabled_from_settings: bool, root: &Path) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    return reconcile_windows(enabled_from_settings, root);
    #[cfg(target_os = "macos")]
    return reconcile_macos(enabled_from_settings, root);
    #[cfg(all(unix, not(target_os = "macos")))]
    return reconcile_linux(enabled_from_settings, root);
    #[allow(unreachable_code)]
    Ok(enabled_from_settings)
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct WindowsAutostartState {
    present: bool,
    enabled: bool,
    command_matches_current_executable: bool,
}

#[cfg(target_os = "windows")]
fn apply_windows(enabled: bool, root: &Path) -> Result<(), String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu.create_subkey(WINDOWS_RUN_KEY_PATH).map_err(|error| error.to_string())?;
    let (approved_key, _) = hkcu
        .create_subkey(WINDOWS_STARTUP_APPROVED_KEY_PATH)
        .map_err(|error| error.to_string())?;
    let legacy_run_values = legacy_autostart_value_names();

    // 同时清理 Run 与 StartupApproved 中的旧名称，是为了避免任务管理器保留幽灵启动项或让旧禁用状态覆盖当前品牌项。
    // Cleaning legacy names from both Run and StartupApproved prevents ghost entries in Task Manager and stops stale disabled state from overriding the current branded entry.
    cleanup_legacy_run_values(&run_key, &approved_key, &legacy_run_values)?;

    let command = windows_startup_command(root);
    // 无论启用还是禁用都保留 Run 项，是为了让 Windows 任务管理器的“启动应用”页面始终可以继续开启或关闭该项目。
    // Keeping the Run entry for both enabled and disabled states lets Windows Task Manager continue to enable or disable the item from Startup apps.
    run_key.set_value(CURRENT_RUN_VALUE, &command).map_err(|error| error.to_string())?;
    set_windows_startup_approved(&approved_key, CURRENT_RUN_VALUE, enabled)
}

#[cfg(target_os = "windows")]
fn reconcile_windows(enabled_from_settings: bool, root: &Path) -> Result<bool, String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let state = read_windows_autostart_state(&hkcu, root)?;
    if !state.present {
        // 首次安装且设置为关闭时不创建任何启动项，是为了让“默认关闭”在系统层面也真实生效，而不是留下一个禁用占位项。
        // On a first install with the setting off, no startup entry is created so the default-off choice is real at the OS level instead of leaving a disabled placeholder.
        if enabled_from_settings {
            apply_windows(true, root)?;
        }
        return Ok(enabled_from_settings);
    }

    if !state.command_matches_current_executable {
        let (run_key, _) = hkcu.create_subkey(WINDOWS_RUN_KEY_PATH).map_err(|error| error.to_string())?;
        // 只修复可执行文件路径而不改变 StartupApproved 状态，是为了尊重用户在任务管理器中做出的启用或禁用选择。
        // Repairing only the executable path while preserving StartupApproved respects the user's enabled or disabled choice in Task Manager.
        run_key
            .set_value(CURRENT_RUN_VALUE, &windows_startup_command(root))
            .map_err(|error| error.to_string())?;
    }

    Ok(state.enabled)
}

#[cfg(target_os = "windows")]
fn read_windows_autostart_state(hkcu: &winreg::RegKey, root: &Path) -> Result<WindowsAutostartState, String> {
    use std::io::ErrorKind;

    let run_key = match hkcu.open_subkey(WINDOWS_RUN_KEY_PATH) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(WindowsAutostartState {
                present: false,
                enabled: false,
                command_matches_current_executable: false,
            });
        }
        Err(error) => return Err(error.to_string()),
    };
    let command: String = match run_key.get_value(CURRENT_RUN_VALUE) {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(WindowsAutostartState {
                present: false,
                enabled: false,
                command_matches_current_executable: false,
            });
        }
        Err(error) => return Err(error.to_string()),
    };
    let enabled = match hkcu.open_subkey(WINDOWS_STARTUP_APPROVED_KEY_PATH) {
        Ok(key) => windows_startup_approved_enabled(&key, CURRENT_RUN_VALUE)?,
        Err(error) if error.kind() == ErrorKind::NotFound => true,
        Err(error) => return Err(error.to_string()),
    };
    let expected_exe = std::env::current_exe().unwrap_or_else(|_| root.join("ClipAnchor.exe"));

    Ok(WindowsAutostartState {
        present: true,
        enabled,
        command_matches_current_executable: startup_command_executable(&command)
            .map(|configured| windows_paths_equal(&configured, &expected_exe))
            .unwrap_or(false),
    })
}

#[cfg(target_os = "windows")]
fn windows_startup_command(root: &Path) -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| root.join("ClipAnchor.exe"));
    format!("\"{}\" --portable --clipanchor-startup", exe.to_string_lossy())
}

#[cfg(target_os = "windows")]
fn startup_command_executable(command: &str) -> Option<std::path::PathBuf> {
    let value = command.trim();
    if let Some(rest) = value.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(std::path::PathBuf::from(&rest[..end]));
    }
    value.split_whitespace().next().map(std::path::PathBuf::from)
}

#[cfg(target_os = "windows")]
fn windows_paths_equal(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> String {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    }
    normalized(left) == normalized(right)
}

#[cfg(target_os = "windows")]
fn set_windows_startup_approved(approved_key: &winreg::RegKey, name: &str, enabled: bool) -> Result<(), String> {
    use std::io::ErrorKind;
    use winreg::{enums::REG_BINARY, RegValue};

    let mut bytes = match approved_key.get_raw_value(name) {
        Ok(value) => {
            // winreg 的 RegValue 已经直接拥有 Vec<u8>，这里移动所有权可避免调用仅适用于 Cow 的 into_owned，并且不会产生无意义的克隆。
            // winreg's RegValue already owns a Vec<u8>; moving it directly avoids the Cow-only into_owned API and prevents an unnecessary clone.
            value.bytes
        }
        Err(error) if error.kind() == ErrorKind::NotFound => vec![0u8; 12],
        Err(error) => return Err(error.to_string()),
    };
    if bytes.len() < 12 {
        bytes.resize(12, 0);
    }
    bytes[0] = if enabled { 0x02 } else { 0x03 };
    bytes[1..4].fill(0);
    // 保留 Windows 已写入的其余 StartupApproved 字节，是为了只改变启用标志，不破坏任务管理器用于维护该启动项的附加状态。
    // Preserving the remaining StartupApproved bytes changes only the enabled flag without discarding auxiliary state maintained by Task Manager.
    approved_key
        .set_raw_value(name, &RegValue { bytes: bytes.into(), vtype: REG_BINARY })
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn windows_startup_approved_enabled(approved_key: &winreg::RegKey, name: &str) -> Result<bool, String> {
    use std::io::ErrorKind;

    match approved_key.get_raw_value(name) {
        Ok(value) => Ok(matches!(value.bytes.first().copied(), Some(0x02) | Some(0x06))),
        // 没有 StartupApproved 值时 Windows 默认执行 Run 项，因此这里必须视为启用。
        // When no StartupApproved value exists Windows executes the Run entry by default, so the state must be treated as enabled.
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(target_os = "windows")]
fn legacy_autostart_value_names() -> Vec<String> {
    const LEGACY_VALUE_BYTES: &[&[u8]] = &[&[67, 108, 105, 112, 105, 110], &[67, 104, 105, 112, 105, 110]];
    // 迁移清理仍需要识别旧注册表值，但旧名称不应作为普通文本散落在项目中，避免改名后继续被误认为当前品牌。
    // Migration cleanup still needs old registry value names, but they should not appear as ordinary project text after the rename.
    LEGACY_VALUE_BYTES
        .iter()
        .filter_map(|bytes| std::str::from_utf8(bytes).ok().map(ToOwned::to_owned))
        .collect()
}

#[cfg(target_os = "windows")]
fn cleanup_legacy_run_values(run_key: &winreg::RegKey, approved_key: &winreg::RegKey, names: &[String]) -> Result<(), String> {
    for name in names {
        // 旧名称只作为迁移清理对象处理，是为了保留当前 ClipAnchor 自启动项的单一来源，避免重复写入多个 Run 值。
        // Legacy names are migration cleanup targets only, keeping the current ClipAnchor autostart entry as the single source of truth.
        delete_registry_value(run_key, name)?;
        delete_registry_value(approved_key, name)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn delete_registry_value(key: &winreg::RegKey, name: &str) -> Result<(), String> {
    match key.delete_value(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(target_os = "macos")]
const MACOS_LOGIN_ITEM_NAME: &str = "ClipAnchor";
#[cfg(target_os = "macos")]
const MACOS_LAUNCH_AGENT_LABEL: &str = "com.clipanchor.desktop";
#[cfg(target_os = "macos")]
const MACOS_LAUNCH_AGENT_FILE: &str = "com.clipanchor.desktop.plist";
#[cfg(target_os = "macos")]
const MACOS_LOGIN_HELPER_APP_RELATIVE: &str = "Library/Application Support/ClipAnchor/LoginItems/ClipAnchor.app";
#[cfg(target_os = "macos")]
const MACOS_LEGACY_LOGIN_HELPER_APP_RELATIVE: &str = "Library/Application Support/ClipAnchor/LoginItems/ClipAnchor Login Item.app";
#[cfg(target_os = "macos")]
const MACOS_LEGACY_LOGIN_ITEM_NAME: &str = "ClipAnchor Login Item";
#[cfg(target_os = "macos")]
const MACOS_LOGIN_HELPER_EXECUTABLE: &str = "clipanchor-login-item";

#[cfg(target_os = "macos")]
#[derive(Debug)]
enum MacosLaunchTarget {
    AppBundle(PathBuf),
    Executable(PathBuf),
}

#[cfg(target_os = "macos")]
fn reconcile_macos(enabled_from_settings: bool, root: &Path) -> Result<bool, String> {
    let home = std::env::var("HOME").map_err(|error| error.to_string())?;
    let helper_app = macos_login_helper_app_path(&home);
    let legacy_helper = macos_legacy_login_helper_app_path(&home);
    let legacy_agent = Path::new(&home)
        .join("Library/LaunchAgents")
        .join(MACOS_LAUNCH_AGENT_FILE);

    if !enabled_from_settings {
        // 没有任何自启动痕迹时直接返回关闭，避免首次启动为了检查一个不存在的 Login Item 而触发系统自动化权限请求。
        // When no autostart artifacts exist, return disabled immediately so a first launch does not request automation permission merely to inspect a nonexistent Login Item.
        if !helper_app.exists() && !legacy_helper.exists() && !legacy_agent.exists() {
            return Ok(false);
        }
        apply_macos(false, root)?;
        return Ok(false);
    }

    let login_item_exists = macos_login_item_exists()?;
    let helper_ready = helper_app.join("Contents/MacOS").join(MACOS_LOGIN_HELPER_EXECUTABLE).is_file();
    if !login_item_exists || !helper_ready {
        // 用户开启后每次启动都修复缺失的辅助 app 或 Login Item，是为了防止系统清理、移动安装目录后设置仍显示已开启但实际不执行。
        // After the user enables autostart, every launch repairs a missing helper app or Login Item so system cleanup or app relocation cannot leave a stale enabled switch.
        apply_macos(true, root)?;
    }
    Ok(macos_login_item_exists()?
        && helper_app.join("Contents/MacOS").join(MACOS_LOGIN_HELPER_EXECUTABLE).is_file())
}

#[cfg(target_os = "macos")]
fn macos_login_item_exists() -> Result<bool, String> {
    let script = format!(
        r#"tell application "System Events"
  return exists login item "{}"
end tell"#,
        escape_applescript(MACOS_LOGIN_ITEM_NAME)
    );
    let output = run_macos_osascript_output(&script)?;
    Ok(output.trim().eq_ignore_ascii_case("true"))
}

#[cfg(target_os = "macos")]
fn apply_macos(enabled: bool, root: &Path) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|error| error.to_string())?;
    let agent_dir = Path::new(&home).join("Library/LaunchAgents");
    std::fs::create_dir_all(&agent_dir).map_err(|error| error.to_string())?;
    let plist = agent_dir.join(MACOS_LAUNCH_AGENT_FILE);
    let helper_app = macos_login_helper_app_path(&home);
    let helper_app_text = helper_app.to_string_lossy().into_owned();

    if !enabled {
        // 关闭时同时清理可见 Login Item、登录辅助 app 和旧 LaunchAgent，是为了让系统设置与真实登录启动行为完全一致。
        // Turning the switch off removes the visible Login Item, the helper app, and the legacy LaunchAgent so System Settings and actual sign-in behavior stay in sync.
        let _ = remove_macos_login_item_for_path(Some(&helper_app_text));
        let _ = remove_macos_login_item();
        let _ = remove_macos_login_item_by_name(MACOS_LEGACY_LOGIN_ITEM_NAME);
        unload_macos_launch_agent(&plist);
        let _ = std::fs::remove_file(plist);
        let _ = std::fs::remove_dir_all(helper_app);
        let _ = std::fs::remove_dir_all(macos_legacy_login_helper_app_path(&home));
        return Ok(());
    }

    let target = resolve_macos_launch_target(root);

    // 新版使用可见 Login Item 辅助 app，先移除历史 LaunchAgent，避免登录时一个可见项和一个隐藏项同时拉起两个实例。
    // The new flow uses a visible Login Item helper app; legacy LaunchAgent entries are removed first to avoid launching duplicate instances at sign-in.
    unload_macos_launch_agent(&plist);
    let _ = std::fs::remove_file(&plist);

    let _ = remove_macos_login_item();
    let _ = remove_macos_login_item_by_name(MACOS_LEGACY_LOGIN_ITEM_NAME);
    let _ = std::fs::remove_dir_all(macos_legacy_login_helper_app_path(&home));

    let helper_app = create_macos_login_helper_app(&home, &target)?;
    create_macos_login_item(&helper_app)
}

#[cfg(target_os = "macos")]
fn resolve_macos_launch_target(root: &Path) -> MacosLaunchTarget {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bundle) = find_containing_app_bundle(&exe) {
            return MacosLaunchTarget::AppBundle(bundle);
        }
        if let Some(bundle) = find_installed_macos_app_bundle(root) {
            // dev 模式运行的是 target/debug/clipanchor，不是 .app；如果能找到真实 .app，就优先让辅助登录项打开 .app。
            // In dev mode the process is target/debug/clipanchor rather than an .app; if a real .app can be found, the helper Login Item should launch that app first.
            return MacosLaunchTarget::AppBundle(bundle);
        }
        // 没有可用 .app 时仍创建“可见的辅助 .app”作为登录项，辅助 app 再启动当前可执行文件，避免退回到系统设置不可见的 LaunchAgent。
        // When no real .app is available, a visible helper .app is still created as the Login Item and it launches the current executable, avoiding an invisible LaunchAgent fallback.
        return MacosLaunchTarget::Executable(exe);
    }

    if let Some(bundle) = find_installed_macos_app_bundle(root) {
        return MacosLaunchTarget::AppBundle(bundle);
    }

    let bundled_from_root = root
        .ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
        .map(Path::to_path_buf);
    if let Some(bundle) = bundled_from_root {
        return MacosLaunchTarget::AppBundle(bundle);
    }

    let lowercase = root.join("ClipAnchor.app/Contents/MacOS/clipanchor");
    if lowercase.exists() {
        return MacosLaunchTarget::Executable(lowercase);
    }

    // Tauri 打包名可能随 productName 或二进制名大小写变化，按实际存在路径回退是为了让 Apple Silicon .app 自启动不绑死单一文件名。
    // Tauri bundle names may vary with productName or binary-name casing, so falling back by real paths keeps Apple Silicon .app autostart from depending on one filename.
    MacosLaunchTarget::Executable(root.join("ClipAnchor.app/Contents/MacOS/ClipAnchor"))
}

#[cfg(target_os = "macos")]
fn find_containing_app_bundle(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.extension().and_then(|value| value.to_str()) == Some("app"))
        .filter(|bundle| is_valid_macos_app_bundle(bundle))
        .map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn find_installed_macos_app_bundle(root: &Path) -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("/Applications/ClipAnchor.app")];
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(Path::new(&home).join("Applications/ClipAnchor.app"));
    }
    for ancestor in root.ancestors() {
        candidates.push(ancestor.join("target/release/bundle/macos/ClipAnchor.app"));
        candidates.push(ancestor.join("target/debug/bundle/macos/ClipAnchor.app"));
        candidates.push(ancestor.join("release/bundle/macos/ClipAnchor.app"));
        candidates.push(ancestor.join("debug/bundle/macos/ClipAnchor.app"));
        candidates.push(ancestor.join("bundle/macos/ClipAnchor.app"));
    }
    candidates.into_iter().find(|path| is_valid_macos_app_bundle(path))
}

#[cfg(target_os = "macos")]
fn is_valid_macos_app_bundle(path: &Path) -> bool {
    path.is_dir() && path.join("Contents/Info.plist").is_file() && path.join("Contents/MacOS").is_dir()
}

#[cfg(target_os = "macos")]
fn macos_login_helper_app_path(home: &str) -> PathBuf {
    Path::new(home).join(MACOS_LOGIN_HELPER_APP_RELATIVE)
}

#[cfg(target_os = "macos")]
fn macos_legacy_login_helper_app_path(home: &str) -> PathBuf {
    Path::new(home).join(MACOS_LEGACY_LOGIN_HELPER_APP_RELATIVE)
}

#[cfg(target_os = "macos")]
fn create_macos_login_helper_app(home: &str, target: &MacosLaunchTarget) -> Result<PathBuf, String> {
    let helper_app = macos_login_helper_app_path(home);
    let contents_dir = helper_app.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    let resources_dir = contents_dir.join("Resources");
    let executable_path = macos_dir.join(MACOS_LOGIN_HELPER_EXECUTABLE);

    let _ = std::fs::remove_dir_all(&helper_app);
    std::fs::create_dir_all(&macos_dir).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&resources_dir).map_err(|error| error.to_string())?;

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>{}</string>
  <key>CFBundleExecutable</key>
  <string>{}</string>
  <key>CFBundleIdentifier</key>
  <string>com.clipanchor.desktop.loginitem</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>{}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSBackgroundOnly</key>
  <true/>
  <key>LSUIElement</key>
  <true/>
</dict>
</plist>
"#,
        escape_xml(MACOS_LOGIN_ITEM_NAME),
        escape_xml(MACOS_LOGIN_HELPER_EXECUTABLE),
        escape_xml(MACOS_LOGIN_ITEM_NAME)
    );
    std::fs::write(contents_dir.join("Info.plist"), plist).map_err(|error| error.to_string())?;

    let script = macos_login_helper_script(target);
    std::fs::write(&executable_path, script).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }

    // 创建一个用户目录下的辅助 .app 作为 Open at Login 条目，是为了让系统设置能显示 ClipAnchor，同时还能在登录时传入轻量启动参数。
    // A helper .app in the user's Application Support directory is used as the Open at Login entry so System Settings can show ClipAnchor while the helper can still pass Lite-start arguments.
    Ok(helper_app)
}

#[cfg(target_os = "macos")]
fn macos_login_helper_script(target: &MacosLaunchTarget) -> String {
    match target {
        MacosLaunchTarget::AppBundle(bundle) => {
            let bundle_text = bundle.to_string_lossy().into_owned();
            let bundle_path = shell_quote(&bundle_text);
            format!(
                r#"#!/bin/sh
APP_BUNDLE={}
if [ -d "$APP_BUNDLE" ]; then
  /usr/bin/open -g -j "$APP_BUNDLE" --args --portable --clipanchor-startup
  exit 0
fi
for CANDIDATE in "/Applications/ClipAnchor.app" "$HOME/Applications/ClipAnchor.app"; do
  if [ -d "$CANDIDATE" ]; then
    /usr/bin/open -g -j "$CANDIDATE" --args --portable --clipanchor-startup
    exit 0
  fi
done
/usr/bin/open -g -j -a "ClipAnchor" --args --portable --clipanchor-startup >/dev/null 2>&1 || true
exit 0
"#,
                bundle_path
            )
        }
        MacosLaunchTarget::Executable(executable) => {
            let executable_text = executable.to_string_lossy().into_owned();
            let exe_path = shell_quote(&executable_text);
            format!(
                r#"#!/bin/sh
TARGET_EXE={}
if [ -x "$TARGET_EXE" ]; then
  /usr/bin/nohup "$TARGET_EXE" --portable --clipanchor-startup >/dev/null 2>&1 &
  exit 0
fi
for CANDIDATE in "/Applications/ClipAnchor.app" "$HOME/Applications/ClipAnchor.app"; do
  if [ -d "$CANDIDATE" ]; then
    /usr/bin/open -g -j "$CANDIDATE" --args --portable --clipanchor-startup
    exit 0
  fi
done
/usr/bin/open -g -j -a "ClipAnchor" --args --portable --clipanchor-startup >/dev/null 2>&1 || true
exit 0
"#,
                exe_path
            )
        }
    }
}

#[cfg(target_os = "macos")]
fn create_macos_login_item(helper_app: &Path) -> Result<(), String> {
    let item_path = helper_app.to_string_lossy().into_owned();
    let script = format!(
        r#"tell application "System Events"
  set targetPathText to "{}"
  if exists login item "{}" then delete login item "{}"
  if exists login item "{}" then delete login item "{}"
  delay 0.1
  make new login item at end with properties {{path:targetPathText, hidden:false}}
end tell"#,
        escape_applescript(&item_path),
        escape_applescript(MACOS_LOGIN_ITEM_NAME),
        escape_applescript(MACOS_LOGIN_ITEM_NAME),
        escape_applescript(MACOS_LEGACY_LOGIN_ITEM_NAME),
        escape_applescript(MACOS_LEGACY_LOGIN_ITEM_NAME)
    );

    // 这里不再额外用“exists login item ClipAnchor”做验证：System Events 会按 .app 包名决定显示名称，旧版本的自定义校验会把已经创建成功但名称未立即刷新的登录项误判为失败。
    // Do not verify with an extra “exists login item ClipAnchor” check: System Events derives the visible name from the .app bundle and the old custom check could report failure even after creation succeeded.
    run_macos_osascript(&script).map_err(|error| format!("MACOS_LOGIN_ITEM_FAILED:{}", error))
}

#[cfg(target_os = "macos")]
fn remove_macos_login_item() -> Result<(), String> {
    remove_macos_login_item_for_path(None)
}

#[cfg(target_os = "macos")]
fn remove_macos_login_item_for_path(_path: Option<&str>) -> Result<(), String> {
    remove_macos_login_item_by_name(MACOS_LOGIN_ITEM_NAME)
}

#[cfg(target_os = "macos")]
fn remove_macos_login_item_by_name(name: &str) -> Result<(), String> {
    let script = format!(
        r#"tell application "System Events"
  if exists login item "{}" then delete login item "{}"
end tell"#,
        escape_applescript(name),
        escape_applescript(name)
    );
    // 删除只按名称处理，是为了让关闭自启动在存在损坏登录项时仍保持幂等，不被其他应用的登录项状态拖累。
    // Removal works by name only so disabling autostart remains idempotent even when another app has a broken login item.
    run_macos_osascript(&script)
}

#[cfg(target_os = "macos")]
fn run_macos_osascript(script: &str) -> Result<(), String> {
    run_macos_osascript_output(script).map(|_| ())
}

#[cfg(target_os = "macos")]
fn run_macos_osascript_output(script: &str) -> Result<String, String> {
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        Err(if detail.is_empty() {
            format!("osascript failed with status {}", output.status)
        } else {
            detail
        })
    }
}

#[cfg(target_os = "macos")]
fn unload_macos_launch_agent(plist: &Path) {
    if let Ok(service) = macos_launchctl_service() {
        let _ = run_macos_launchctl(&["bootout", &service]);
    }
    if let Ok(domain) = macos_launchctl_domain() {
        let plist_path = plist.to_string_lossy().into_owned();
        let _ = run_macos_launchctl(&["bootout", &domain, &plist_path]);
    }
}

#[cfg(target_os = "macos")]
fn macos_launchctl_domain() -> Result<String, String> {
    Ok(format!("gui/{}", macos_current_uid()?))
}

#[cfg(target_os = "macos")]
fn macos_launchctl_service() -> Result<String, String> {
    Ok(format!("{}/{}", macos_launchctl_domain()?, MACOS_LAUNCH_AGENT_LABEL))
}

#[cfg(target_os = "macos")]
fn macos_current_uid() -> Result<String, String> {
    let output = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(target_os = "macos")]
fn run_macos_launchctl(args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("/bin/launchctl")
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        Err(if detail.is_empty() {
            format!("launchctl {:?} failed with status {}", args, output.status)
        } else {
            detail
        })
    }
}

#[cfg(target_os = "macos")]
fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn reconcile_linux(enabled_from_settings: bool, root: &Path) -> Result<bool, String> {
    let desktop = linux_autostart_path()?;
    if !enabled_from_settings {
        if desktop.exists() {
            apply_linux(false, root)?;
        }
        return Ok(false);
    }

    let expected = linux_autostart_content(root);
    let current = std::fs::read_to_string(&desktop).unwrap_or_default();
    if current != expected {
        // 每次启动比较实际 desktop 文件，是为了在可执行文件移动或系统清理启动项后自动恢复用户已开启的配置。
        // Comparing the real desktop file on every launch restores an enabled configuration after executable relocation or system cleanup.
        apply_linux(true, root)?;
    }
    Ok(desktop.is_file() && std::fs::read_to_string(desktop).unwrap_or_default() == expected)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_autostart_path() -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME").map_err(|error| error.to_string())?;
    Ok(Path::new(&home).join(".config/autostart/clipanchor.desktop"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_executable(root: &Path) -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| root.join("clipanchor"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn desktop_exec_quote(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy().replace('\"', "\\\""))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_autostart_content(root: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=ClipAnchor\nExec={} --portable --clipanchor-startup\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_exec_quote(&linux_executable(root))
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn apply_linux(enabled: bool, root: &Path) -> Result<(), String> {
    let desktop = linux_autostart_path()?;
    if !enabled {
        let _ = std::fs::remove_file(desktop);
        return Ok(());
    }
    if let Some(dir) = desktop.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    std::fs::write(desktop, linux_autostart_content(root)).map_err(|error| error.to_string())
}
