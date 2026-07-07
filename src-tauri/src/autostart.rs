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
fn apply_windows(enabled: bool, root: &Path) -> Result<(), String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    const CURRENT_RUN_VALUE: &str = "ClipAnchor";
    let legacy_run_values = legacy_autostart_value_names();

    let exe = std::env::current_exe().unwrap_or_else(|_| root.join("ClipAnchor.exe"));
    let value = format!("\"{}\" --portable --clipanchor-startup", exe.to_string_lossy());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .map_err(|error| error.to_string())?;

    // 旧自启动项只在迁移期按字节生成并清理，是为了避免源码继续残留旧产品名，同时防止历史 Run 值重复拉起旧程序。
    // Legacy startup entries are generated from bytes only during migration cleanup so old product names do not remain in source while stale Run values cannot relaunch old binaries.
    cleanup_legacy_run_values(&run_key, &legacy_run_values)?;

    if enabled {
        // 直接写入注册表而不是启动 reg.exe，是为了让自启动开关立即响应且不会弹出命令行黑框。
        // Writing the registry directly instead of spawning reg.exe keeps the startup switch responsive and prevents a console window flash.
        // 自启动写入轻量模式参数，是为了让系统登录后只启动托盘与后台服务，不主动弹出主界面。
        // The autostart entry includes the Lite-mode flag so sign-in starts only the tray and background services without opening the main window.
        run_key.set_value(CURRENT_RUN_VALUE, &value).map_err(|error| error.to_string())
    } else {
        // 删除不存在的值也视为成功，是为了让设置开关具备幂等性，不因用户手动清理注册表而报错。
        // A missing value is treated as success so the switch stays idempotent when users have already removed the registry entry.
        delete_run_value(&run_key, CURRENT_RUN_VALUE)
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
fn cleanup_legacy_run_values(run_key: &winreg::RegKey, names: &[String]) -> Result<(), String> {
    for name in names {
        // 旧名称只作为迁移清理对象处理，是为了保留当前 ClipAnchor 自启动项的单一来源，避免重复写入多个 Run 值。
        // Legacy names are migration cleanup targets only, keeping the current ClipAnchor autostart entry as the single source of truth.
        delete_run_value(run_key, name)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn delete_run_value(run_key: &winreg::RegKey, name: &str) -> Result<(), String> {
    match run_key.delete_value(name) {
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
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
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
fn apply_linux(enabled: bool, root: &Path) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|error| error.to_string())?;
    let dir = Path::new(&home).join(".config/autostart");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let desktop = dir.join("clipanchor.desktop");
    if !enabled {
        let _ = std::fs::remove_file(desktop);
        return Ok(());
    }
    let exe = root.join("clipanchor");
    let content = format!("[Desktop Entry]\nType=Application\nName=ClipAnchor\nExec={} --portable --clipanchor-startup\nTerminal=false\nX-GNOME-Autostart-enabled=true\n", exe.to_string_lossy());
    std::fs::write(desktop, content).map_err(|error| error.to_string())
}
