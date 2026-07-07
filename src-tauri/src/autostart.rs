use std::path::{Path, PathBuf};

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
const MACOS_LAUNCH_AGENT_LABEL: &str = "com.clipanchor.desktop";
#[cfg(target_os = "macos")]
const MACOS_LAUNCH_AGENT_FILE: &str = "com.clipanchor.desktop.plist";

#[cfg(target_os = "macos")]
#[derive(Debug)]
enum MacosLaunchTarget {
    AppBundle(PathBuf),
    Executable(PathBuf),
}

#[cfg(target_os = "macos")]
fn apply_macos(enabled: bool, root: &Path) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|error| error.to_string())?;
    let dir = Path::new(&home).join("Library/LaunchAgents");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let plist = dir.join(MACOS_LAUNCH_AGENT_FILE);

    if !enabled {
        // 先从当前登录会话卸载再删文件，是为了让关闭开关立即生效，而不是等到下次登录才停止自启动项。
        // Unloading from the current GUI session before deleting the file makes the off switch effective immediately instead of waiting for the next login.
        unload_macos_launch_agent(&plist);
        let _ = std::fs::remove_file(plist);
        return Ok(());
    }

    let target = resolve_macos_launch_target(root);
    let content = macos_launch_agent_plist(&target);
    write_macos_launch_agent(&plist, &content)?;

    // 重新 bootstrap 是为了让 macOS 立即接受新的 LaunchAgent，并暴露无效 plist 或路径错误，而不是静默等到下次登录失败。
    // Bootstrapping again makes macOS accept the new LaunchAgent immediately and exposes invalid plist/path errors instead of failing silently at the next login.
    unload_macos_launch_agent(&plist);
    bootstrap_macos_launch_agent(&plist)
}

#[cfg(target_os = "macos")]
fn resolve_macos_launch_target(root: &Path) -> MacosLaunchTarget {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bundle) = find_containing_app_bundle(&exe) {
            return MacosLaunchTarget::AppBundle(bundle);
        }
        return MacosLaunchTarget::Executable(exe);
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
        .map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_plist(target: &MacosLaunchTarget) -> String {
    let arguments = match target {
        MacosLaunchTarget::AppBundle(bundle) => vec![
            "/usr/bin/open".to_string(),
            "-g".to_string(),
            "-j".to_string(),
            bundle.to_string_lossy().into_owned(),
            "--args".to_string(),
            "--portable".to_string(),
            "--clipanchor-startup".to_string(),
        ],
        MacosLaunchTarget::Executable(exe) => vec![
            exe.to_string_lossy().into_owned(),
            "--portable".to_string(),
            "--clipanchor-startup".to_string(),
        ],
    };
    let program_arguments = arguments
        .iter()
        .map(|argument| format!("    <string>{}</string>", escape_xml(argument)))
        .collect::<Vec<_>>()
        .join("\n");

    // 通过 open 启动 .app 而不是直接执行 Contents/MacOS 二进制，是为了让 macOS 按 GUI 应用生命周期恢复托盘、Dock 策略和权限上下文。
    // Launching the .app through open instead of executing Contents/MacOS directly lets macOS restore the GUI app lifecycle, tray, Dock policy, and permission context correctly.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
{}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
</dict>
</plist>
"#,
        MACOS_LAUNCH_AGENT_LABEL,
        program_arguments
    )
}

#[cfg(target_os = "macos")]
fn write_macos_launch_agent(plist: &Path, content: &str) -> Result<(), String> {
    let temporary = plist.with_extension("plist.tmp");
    std::fs::write(&temporary, content).map_err(|error| error.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o644))
            .map_err(|error| error.to_string())?;
    }

    // 原子替换可以避免用户连续点击开关时留下半截 plist，半截文件会让 launchctl 拒绝加载自启动项。
    // Atomic replacement prevents a half-written plist when the switch is clicked repeatedly; partial files make launchctl reject the login item.
    std::fs::rename(&temporary, plist).map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn bootstrap_macos_launch_agent(plist: &Path) -> Result<(), String> {
    let domain = macos_launchctl_domain()?;
    let service = macos_launchctl_service()?;
    let plist_path = plist.to_string_lossy().into_owned();

    run_macos_launchctl(&["bootstrap", &domain, &plist_path])?;
    // enable 明确清除用户曾经手动 disable 的状态，是为了避免 plist 存在但 macOS 仍不在登录时运行它。
    // enable explicitly clears any user-level disabled state so an existing plist is not silently skipped at the next login.
    run_macos_launchctl(&["enable", &service]).or_else(|_| Ok(()))
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
