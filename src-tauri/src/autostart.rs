use std::path::Path;

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
fn apply_macos(enabled: bool, root: &Path) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|error| error.to_string())?;
    let dir = Path::new(&home).join("Library/LaunchAgents");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let plist = dir.join("com.clipanchor.desktop.plist");
    if !enabled {
        let _ = std::fs::remove_file(plist);
        return Ok(());
    }
    let exe = root.join("ClipAnchor.app/Contents/MacOS/ClipAnchor");
    let content = format!(r#"<plist>
<dict>
  <key>Label</key><string>com.clipanchor.desktop</string>
  <key>ProgramArguments</key><array><string>{}</string><string>--portable</string><string>--clipanchor-startup</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>"#, exe.to_string_lossy());
    std::fs::write(plist, content).map_err(|error| error.to_string())
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
