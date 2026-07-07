#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
fn print_cli_line(line: &str) {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetStdHandle, WriteConsoleW, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
    };

    // 发布版使用 Windows GUI 子系统时不会自动继承 PowerShell 控制台；仅在命令行查询版本时附加父控制台，避免普通双击启动出现黑框。
    // Release builds use the Windows GUI subsystem and do not inherit PowerShell automatically; attaching only for version queries avoids a console window on normal launches.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle != std::ptr::null_mut() && handle != INVALID_HANDLE_VALUE {
            let text = format!("{}\r\n", line);
            let wide: Vec<u16> = text.encode_utf16().collect();
            let mut written = 0u32;
            if WriteConsoleW(handle, wide.as_ptr(), wide.len() as u32, &mut written, std::ptr::null_mut()) != 0 {
                return;
            }
        }
    }

    println!("{}", line);
}

#[cfg(not(windows))]
fn print_cli_line(line: &str) {
    println!("{}", line);
}

fn should_print_version() -> bool {
    std::env::args().skip(1).any(|arg| arg == "--version" || arg == "-V")
}

fn main() {
    if should_print_version() {
        // 版本号只从 Cargo 元数据读取，是为了避免命令行输出、安装包版本和应用元数据出现多处维护导致的不一致。
        // The version is read only from Cargo metadata so CLI output, installer metadata, and app metadata cannot drift across duplicated sources.
        print_cli_line(&format!("ClipAnchor v{}", env!("CARGO_PKG_VERSION")));
        return;
    }

    // 将二进制入口保持为最小包装，是为了避免 RustRover/Cargo 同时解析重复模块并保持 Tauri 官方推荐的库入口结构。
    // Keeping the binary entrypoint as a tiny wrapper avoids duplicate module parsing in RustRover/Cargo and preserves Tauri's recommended library-entry structure.
    clipanchor_lib::run();
}
