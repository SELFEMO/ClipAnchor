#![allow(unexpected_cfgs)]
// 旧版 objc 消息宏内部仍会声明 cargo-clippy cfg；新版 Rust 会把它报告为依赖宏噪声，因此在本模块内收敛该告警，避免正式构建输出被无关警告污染。
// Legacy objc messaging macros still declare the cargo-clippy cfg; newer Rust reports it as dependency-macro noise, so this module contains the allowance to keep release builds free of unrelated warnings.

#[cfg(target_os = "macos")]
#[allow(unused_imports)]
use objc::{msg_send, sel, sel_impl};
#[cfg(target_os = "macos")]
use objc::runtime::{Class, Object};
#[cfg(target_os = "macos")]
use std::{collections::HashSet, ffi::{CStr, CString}, os::raw::c_char, path::Path};
#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "macos")]
use tauri::{AppHandle, Manager, WebviewWindow};


#[cfg(target_os = "macos")]
pub fn read_file_paths_from_pasteboard() -> Result<Vec<String>, String> {
    // macOS Finder 复制文件时优先暴露 pasteboard item 的 file-url，读取它可以得到真实文件对象而不是退化文本。
    // macOS Finder exposes file-url pasteboard items for file copies; reading them preserves real file objects instead of falling back to text.
    unsafe {
        let pasteboard = general_pasteboard()?;
        let mut paths = read_file_url_items_from_pasteboard(pasteboard);
        if paths.is_empty() {
            // 旧应用可能只提供 NSFilenamesPboardType；保留该兜底可以兼容非 Finder 来源的多文件复制。
            // Older apps may only expose NSFilenamesPboardType, so this fallback keeps multi-file copies from non-Finder sources compatible.
            paths = read_legacy_filenames_from_pasteboard(pasteboard);
        }
        Ok(paths)
    }
}

#[cfg(target_os = "macos")]
pub fn write_file_paths_to_pasteboard(paths: &[String]) -> Result<(), String> {
    // 写回剪贴板时使用 NSURL 数组，是为了让 Finder、访达和桌面应用执行真实文件粘贴而不是粘贴路径字符串。
    // Writing an NSURL array back to the pasteboard makes Finder and desktop apps paste real files instead of path strings.
    let valid_paths: Vec<String> = paths
        .iter()
        .filter(|path| Path::new(path.as_str()).exists())
        .cloned()
        .collect();
    if valid_paths.is_empty() {
        return Err("No valid files to copy".into());
    }

    unsafe {
        let pasteboard = general_pasteboard()?;
        let Some(array_class) = Class::get("NSMutableArray") else {
            return Err("NSMutableArray is unavailable".into());
        };
        let Some(url_class) = Class::get("NSURL") else {
            return Err("NSURL is unavailable".into());
        };
        let urls: *mut Object = msg_send![array_class, arrayWithCapacity: valid_paths.len()];
        if urls.is_null() {
            return Err("Cannot create macOS file clipboard payload".into());
        }

        let mut written_count = 0usize;
        for path in valid_paths {
            let ns_path = ns_string(&path)?;
            let url: *mut Object = msg_send![url_class, fileURLWithPath: ns_path];
            if !url.is_null() {
                let _: () = msg_send![urls, addObject: url];
                written_count += 1;
            }
        }
        if written_count == 0 {
            return Err("Cannot create macOS file URLs".into());
        }

        let _: isize = msg_send![pasteboard, clearContents];
        let ok: bool = msg_send![pasteboard, writeObjects: urls];
        if ok {
            Ok(())
        } else {
            Err("Cannot place files on macOS clipboard".into())
        }
    }
}

#[cfg(target_os = "macos")]
pub fn pasteboard_change_count() -> Option<isize> {
    // 轮询时读取 changeCount，是为了区分“用户再次复制同一内容”和“同一次复制的多种剪贴板表示”，避免 macOS 文件复制在文本与文件弹窗间循环刷新。
    // Reading changeCount during polling distinguishes a repeated copy of the same content from multiple representations of one pasteboard change, preventing macOS file copies from cycling between text and file popups.
    unsafe {
        let pasteboard = general_pasteboard().ok()?;
        let change_count: isize = msg_send![pasteboard, changeCount];
        Some(change_count)
    }
}

#[cfg(target_os = "macos")]
unsafe fn general_pasteboard() -> Result<*mut Object, String> {
    let Some(pasteboard_class) = Class::get("NSPasteboard") else {
        return Err("NSPasteboard is unavailable".into());
    };
    let pasteboard: *mut Object = msg_send![pasteboard_class, generalPasteboard];
    if pasteboard.is_null() {
        Err("Cannot open macOS pasteboard".into())
    } else {
        Ok(pasteboard)
    }
}

#[cfg(target_os = "macos")]
unsafe fn read_file_url_items_from_pasteboard(pasteboard: *mut Object) -> Vec<String> {
    let items: *mut Object = msg_send![pasteboard, pasteboardItems];
    if items.is_null() {
        return Vec::new();
    }
    let count: usize = msg_send![items, count];
    let file_url_types = ["public.file-url", "com.apple.pasteboard.promised-file-url"];
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for index in 0..count {
        let item: *mut Object = msg_send![items, objectAtIndex: index];
        if item.is_null() {
            continue;
        }
        for type_name in file_url_types {
            let Ok(ns_type) = ns_string(type_name) else {
                continue;
            };
            let value: *mut Object = msg_send![item, stringForType: ns_type];
            if let Some(text) = ns_string_to_string(value) {
                push_normalized_pasteboard_path(&mut paths, &mut seen, &text);
            }
        }
    }
    paths
}

#[cfg(target_os = "macos")]
unsafe fn read_legacy_filenames_from_pasteboard(pasteboard: *mut Object) -> Vec<String> {
    let Ok(ns_type) = ns_string("NSFilenamesPboardType") else {
        return Vec::new();
    };
    let list: *mut Object = msg_send![pasteboard, propertyListForType: ns_type];
    if list.is_null() {
        return Vec::new();
    }
    let count: usize = msg_send![list, count];
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for index in 0..count {
        let value: *mut Object = msg_send![list, objectAtIndex: index];
        if let Some(text) = ns_string_to_string(value) {
            push_normalized_pasteboard_path(&mut paths, &mut seen, &text);
        }
    }
    paths
}

#[cfg(target_os = "macos")]
unsafe fn ns_string(value: &str) -> Result<*mut Object, String> {
    let c_value = CString::new(value).map_err(|_| "String contains an interior NUL byte".to_string())?;
    let Some(string_class) = Class::get("NSString") else {
        return Err("NSString is unavailable".into());
    };
    let ns_value: *mut Object = msg_send![string_class, stringWithUTF8String: c_value.as_ptr()];
    if ns_value.is_null() {
        Err("Cannot create NSString".into())
    } else {
        Ok(ns_value)
    }
}

#[cfg(target_os = "macos")]
unsafe fn ns_string_to_string(value: *mut Object) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let raw: *const c_char = msg_send![value, UTF8String];
    if raw.is_null() {
        return None;
    }
    Some(CStr::from_ptr(raw).to_string_lossy().to_string())
}

#[cfg(target_os = "macos")]
fn push_normalized_pasteboard_path(paths: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    if let Some(path) = normalize_macos_pasteboard_path(value) {
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
}

#[cfg(target_os = "macos")]
fn normalize_macos_pasteboard_path(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = if let Some(rest) = trimmed.strip_prefix("file://localhost") {
        rest.to_string()
    } else if let Some(rest) = trimmed.strip_prefix("file://") {
        if rest.starts_with('/') {
            rest.to_string()
        } else {
            format!("/{}", rest)
        }
    } else if let Some(rest) = trimmed.strip_prefix("file:") {
        rest.to_string()
    } else {
        trimmed.to_string()
    };
    let decoded = percent_decode_macos_path(&path);
    if Path::new(&decoded).exists() {
        Some(decoded)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn percent_decode_macos_path(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                output.push(hex);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}

#[cfg(target_os = "macos")]
const NS_APPLICATION_ACTIVATION_POLICY_REGULAR: isize = 0;
#[cfg(target_os = "macos")]
const NS_APPLICATION_ACTIVATION_POLICY_ACCESSORY: isize = 1;
#[cfg(target_os = "macos")]
const NS_WINDOW_COLLECTION_CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
#[cfg(target_os = "macos")]
const NS_WINDOW_COLLECTION_TRANSIENT: u64 = 1 << 3;
#[cfg(target_os = "macos")]
const NS_WINDOW_COLLECTION_IGNORES_CYCLE: u64 = 1 << 6;
#[cfg(target_os = "macos")]
const NS_WINDOW_COLLECTION_FULL_SCREEN_AUXILIARY: u64 = 1 << 8;
#[cfg(target_os = "macos")]
const NS_FLOATING_WINDOW_LEVEL: isize = 3;

#[cfg(target_os = "macos")]
pub fn show_dock_icon(app: &AppHandle) {
    set_activation_policy(app, NS_APPLICATION_ACTIVATION_POLICY_REGULAR);
}

#[cfg(target_os = "macos")]
pub fn hide_dock_icon(app: &AppHandle) {
    set_activation_policy(app, NS_APPLICATION_ACTIVATION_POLICY_ACCESSORY);
}

#[cfg(target_os = "macos")]
pub fn prepare_background_popup(app: &AppHandle) {
    if main_window_is_visible(app) {
        return;
    }
    // 主窗口已经隐藏时切到 accessory 策略，是为了让 Dock 不再出现主程序图标，同时允许桌面提示窗停留在当前 Space。
    // Switching to the accessory policy while the main window is hidden removes the Dock icon and lets desktop hint windows stay on the current Space.
    hide_dock_icon(app);
}

#[cfg(target_os = "macos")]
pub fn configure_popup_for_current_space(window: &WebviewWindow) {
    apply_popup_native_behavior(window);
}

#[cfg(target_os = "macos")]
fn main_window_is_visible(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn set_activation_policy(app: &AppHandle, policy: isize) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        // AppKit 状态必须在主线程调整，是为了避免剪贴板监听线程创建弹窗时触发跨线程 UI 行为。
        // AppKit state is changed on the main thread so clipboard-monitor popup creation never performs cross-thread UI work.
        let ok = unsafe { set_activation_policy_now(policy) };
        if !ok {
            if let Some(state) = app_handle.try_state::<crate::models::AppState>() {
                crate::app_log::warn(&state.paths, "macos", "setActivationPolicy returned false");
            }
        }
    });
}

#[cfg(target_os = "macos")]
unsafe fn set_activation_policy_now(policy: isize) -> bool {
    let Some(ns_application_class) = Class::get("NSApplication") else {
        return false;
    };
    // 通过运行时查找类名而不是 class! 宏，是为了避开新版 Rust 对 objc 旧宏内部 cfg 的告警，同时保留 AppKit 原生能力。
    // Looking up the class at runtime instead of using class! avoids new Rust cfg warnings from older objc macros while keeping native AppKit behavior.
    let ns_app: *mut Object = msg_send![ns_application_class, sharedApplication];
    if ns_app.is_null() {
        return false;
    }
    msg_send![ns_app, setActivationPolicy: policy]
}

#[cfg(target_os = "macos")]
fn apply_popup_native_behavior(window: &WebviewWindow) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    let ns_view = appkit.ns_view.as_ptr() as *mut Object;
    if ns_view.is_null() {
        return;
    }
    unsafe {
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return;
        }
        let behavior = NS_WINDOW_COLLECTION_CAN_JOIN_ALL_SPACES
            | NS_WINDOW_COLLECTION_TRANSIENT
            | NS_WINDOW_COLLECTION_IGNORES_CYCLE
            | NS_WINDOW_COLLECTION_FULL_SCREEN_AUXILIARY;
        // 弹窗加入所有 Space 并作为全屏辅助窗口，是为了复制发生在全屏应用中时，提示卡仍出现在用户正在看的桌面上。
        // Joining all Spaces as a fullscreen auxiliary window keeps the hint card on the desktop the user is currently viewing, including fullscreen apps.
        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
        let _: () = msg_send![ns_window, setLevel: NS_FLOATING_WINDOW_LEVEL];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn show_dock_icon(_app: &tauri::AppHandle) {}

#[cfg(not(target_os = "macos"))]
pub fn hide_dock_icon(_app: &tauri::AppHandle) {}

#[cfg(not(target_os = "macos"))]
pub fn prepare_background_popup(_app: &tauri::AppHandle) {}

#[cfg(not(target_os = "macos"))]
pub fn configure_popup_for_current_space(_window: &tauri::WebviewWindow) {}
