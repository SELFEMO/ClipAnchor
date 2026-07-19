use crate::{app_log, database, models::{AppState, AppSettings, ClipItem, ClipKind, HistoryRecord}, popup};
use arboard::{Clipboard, ImageData};
use chrono::Utc;
use image::{ImageBuffer, Rgba};
use std::{borrow::Cow, fs, io::Cursor, panic::{catch_unwind, AssertUnwindSafe}, path::Path, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, OnceLock}, thread, time::{Duration, SystemTime, UNIX_EPOCH}};
#[cfg(target_os = "linux")]
use std::{
    cell::RefCell,
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::AtomicU64,
        mpsc,
    },
};
#[cfg(target_os = "windows")]
use std::{mem, ptr};
#[cfg(target_os = "linux")]
use gtk::prelude::*;
use tauri::{AppHandle, Emitter};
#[cfg(target_os = "linux")]
use url::Url;
use uuid::Uuid;

#[cfg(target_os = "windows")]
use std::{ffi::OsString, os::windows::ffi::{OsStrExt, OsStringExt}, ptr::null_mut};
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData},
        Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE},
    },
    UI::Shell::DragQueryFileW,
};

#[cfg(target_os = "windows")]
// CF_HDROP 在当前 windows-sys 接口中没有从 DataExchange 模块导出，直接使用 Win32 固定格式编号可以避免依赖版本差异导致编译失败。
// CF_HDROP is not exported from the DataExchange module by the current windows-sys interface, so using the stable Win32 format id avoids compile failures across dependency versions.
const CF_HDROP: u32 = 15;

#[cfg(target_os = "windows")]
// CF_UNICODETEXT 同样使用 Win32 固定格式编号，是为了在 arboard 无法读取长文本时仍能从系统剪贴板读取完整 Unicode 文本。
// CF_UNICODETEXT also uses the stable Win32 format id so long Unicode text can still be read when arboard fails to parse it.
const CF_UNICODETEXT: u32 = 13;

#[cfg(target_os = "windows")]
// 部分旧程序或特殊富文本来源只提供 ANSI/OEM 文本格式；补充读取这些格式可以避免长文本被系统剪贴板记录但 ClipAnchor 漏掉。
// Some legacy apps or rich-text sources expose only ANSI/OEM text formats; reading them prevents ClipAnchor from missing text that the system clipboard still has.
const CF_TEXT: u32 = 1;

#[cfg(target_os = "windows")]
// OEM 文本是 Windows 剪贴板的兼容格式之一，作为最后兜底能覆盖少数非 Unicode 来源。
// OEM text is one of Windows clipboard compatibility formats, so it is kept as a final fallback for rare non-Unicode sources.
const CF_OEMTEXT: u32 = 7;

const MONITOR_POLL_MS: u64 = 650;
const MONITOR_WATCHDOG_MS: u64 = 30_000;
const MONITOR_STALE_SECONDS: i64 = 90;

#[cfg(target_os = "linux")]
const LINUX_CLIPBOARD_MAIN_THREAD_TIMEOUT_SECONDS: u64 = 5;
#[cfg(target_os = "linux")]
const LINUX_NON_TEXT_RETRY_COUNT: usize = 4;
#[cfg(target_os = "linux")]
const LINUX_NON_TEXT_RETRY_DELAY_MS: u64 = 45;
#[cfg(target_os = "linux")]
const MAX_LINUX_ENCODED_IMAGE_BYTES: usize = 64 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_LINUX_DECODED_IMAGE_BYTES: usize = 256 * 1024 * 1024;

static SYSTEM_CLIPBOARD: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();

#[cfg(target_os = "linux")]
thread_local! {
    // GTK 剪贴板对象只能在创建它的 GTK 主线程使用，因此保存在主线程本地存储中，而不是跨线程放进全局互斥锁。
    // GTK clipboard objects may only be used on the GTK main thread that created them, so the bridge stays in main-thread local storage instead of a cross-thread mutex.
    static LINUX_CLIPBOARD_BRIDGE: RefCell<Option<gtk::Clipboard>> = const { RefCell::new(None) };
}

#[cfg(target_os = "linux")]
static LINUX_CLIPBOARD_BRIDGE_READY: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "linux")]
static LINUX_CLIPBOARD_CHANGE_COUNT: AtomicU64 = AtomicU64::new(1);
#[cfg(target_os = "linux")]
static LAST_LINUX_DIAGNOSTIC: OnceLock<Mutex<String>> = OnceLock::new();

fn with_system_clipboard<T>(operation: impl FnOnce(&mut Clipboard) -> Result<T, arboard::Error>) -> Result<T, String> {
    let clipboard_slot = SYSTEM_CLIPBOARD.get_or_init(|| Mutex::new(None));
    let mut guard = clipboard_slot.lock().map_err(|_| "System clipboard lock is poisoned".to_string())?;
    if guard.is_none() {
        // Linux 剪贴板内容由持有者进程提供，因此复用同一个实例既减少 Wayland/X11 连接抖动，也保证 ClipAnchor 写入的内容不会因对象立即销毁而失效。
        // Linux clipboard data is served by the owning process, so reusing one instance reduces Wayland/X11 connection churn and keeps ClipAnchor-written content alive after the call returns.
        *guard = Some(Clipboard::new().map_err(|error| error.to_string())?);
    }
    let clipboard = guard.as_mut().ok_or_else(|| "System clipboard could not be initialized".to_string())?;
    let result = operation(clipboard);
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let message = error.to_string();
            if !clipboard_content_unavailable(&message) {
                // 连接类错误后丢弃实例，是为了让下一轮轮询重新建立后端，而不是永久复用失效的 Wayland/X11 连接。
                // Dropping the instance after connection errors lets the next poll rebuild the backend instead of permanently reusing a broken Wayland/X11 connection.
                guard.take();
            }
            Err(message)
        }
    }
}

fn clipboard_content_unavailable(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    // 各 Linux 后端对“当前 MIME 不存在”的错误文本并不完全一致；统一归类为无内容可避免把正常的文本/图片格式切换误判成连接故障并反复重建会话。
    // Linux backends phrase a missing MIME payload differently; treating all of them as ordinary absence avoids rebuilding the session whenever clipboard formats switch normally.
    normalized.contains("content not available")
        || normalized.contains("not available")
        || normalized.contains("clipboard is empty")
        || normalized.contains("incompatible format")
        || normalized.contains("not utf-8 text")
        || normalized.contains("not text")
}

pub fn ensure_monitor(app: AppHandle, state: AppState) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        // 初始化失败时仍启动监听线程，是为了兼容应用启动早期 GTK 主循环尚未就绪的桌面环境；后续轮询会继续重试原生桥接，并在此期间使用降级读取。
        // The monitor still starts when early GTK initialization is unavailable because some desktops are not ready during application setup; later polls retry the native bridge and use fallback reads meanwhile.
        if let Err(error) = ensure_linux_clipboard_bridge(&app) {
            app_log::warn(&state.paths, "clipboard", format!("linux native clipboard bridge deferred: {}", error));
        }
    }

    let mut guard = state.monitor_stop.lock().map_err(|error| error.to_string())?;
    if guard.is_some() {
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    *guard = Some(stop.clone());
    drop(guard);

    state.monitor_heartbeat.store(unix_now(), Ordering::Relaxed);
    app_log::info(&state.paths, "clipboard", "clipboard monitor started");
    thread::spawn(move || {
        let mut last_signature = initial_signature(&app, &state).unwrap_or_default();
        while !stop.load(Ordering::Relaxed) {
            state.monitor_heartbeat.store(unix_now(), Ordering::Relaxed);
            // 剪贴板来源不可控，轮询体必须捕获 panic，避免某次系统剪贴板异常导致后台服务永久退出。
            // Clipboard providers are outside our control, so the polling body catches panics to keep one OS clipboard failure from permanently killing the background service.
            match catch_unwind(AssertUnwindSafe(|| poll_once(&app, &state, &mut last_signature))) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    app_log::warn(&state.paths, "clipboard", format!("poll skipped: {}", error));
                    let _ = app.emit("clipanchor-log", error);
                }
                Err(_) => {
                    app_log::error(&state.paths, "clipboard", "clipboard poll panicked and was recovered");
                }
            }
            thread::sleep(Duration::from_millis(MONITOR_POLL_MS));
        }
        app_log::info(&state.paths, "clipboard", "clipboard monitor thread stopped");
    });
    Ok(())
}

pub fn start_monitor_watchdog(app: AppHandle, state: AppState) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(MONITOR_WATCHDOG_MS));
        let should_monitor = state
            .settings
            .lock()
            .map(|settings| settings.pin_service_enabled || settings.history_service_enabled)
            .unwrap_or(true);
        if !should_monitor {
            continue;
        }
        let last_tick = state.monitor_heartbeat.load(Ordering::Relaxed);
        if last_tick <= 0 {
            continue;
        }
        let elapsed = unix_now().saturating_sub(last_tick);
        if elapsed <= MONITOR_STALE_SECONDS {
            continue;
        }

        app_log::warn(&state.paths, "clipboard", format!("clipboard monitor heartbeat stale for {} seconds; restarting monitor", elapsed));
        force_restart_monitor(&app, &state);
    });
}

fn force_restart_monitor(app: &AppHandle, state: &AppState) {
    // 看门狗先撤销旧停止句柄再启动新线程，是为了修复长时间后台后轮询线程死亡但状态仍显示运行的问题。
    // The watchdog clears the old stop handle before starting a new thread to recover cases where the poll thread died while the service still looks enabled.
    if let Ok(mut guard) = state.monitor_stop.lock() {
        if let Some(stop) = guard.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
    thread::sleep(Duration::from_millis(200));
    if let Err(error) = ensure_monitor(app.clone(), state.clone()) {
        app_log::error(&state.paths, "clipboard", format!("clipboard monitor restart failed: {}", error));
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}


fn initial_signature(app: &AppHandle, state: &AppState) -> Result<String, String> {
    let settings = state.settings.lock().map_err(|error| error.to_string())?.clone();
    // 启动监听时先记录当前剪贴板指纹，是为了避免把启动前已经存在的内容误认为新复制并弹窗。
    // The monitor records the existing clipboard signature on startup so pre-existing clipboard content is not mistaken for a new copy.
    #[cfg(target_os = "linux")]
    {
        // Linux 同时读取 arboard 与 GTK 表示，是为了避免文件管理器发布文本回退时把真实文件或图片误判成普通文本。
        // Linux reads both arboard and GTK representations so a file manager's text fallback cannot hide the real file or image payload.
        let snapshot = read_linux_clipboard_snapshot(app, &settings)?;
        log_linux_clipboard_diagnostics(state, &snapshot.diagnostics);
        return Ok(linux_snapshot_signature(&snapshot));
    }

    #[cfg(not(target_os = "linux"))]
    {
        if settings.filter_file {
            if let Ok(paths) = read_file_paths_from_clipboard() {
                if !paths.is_empty() {
                    return Ok(clipboard_change_signature(&content_hash_for_paths(&paths)));
                }
            }
        }
        if settings.filter_image {
            if let Ok(Some(image)) = read_clipboard_image() {
                return Ok(clipboard_change_signature(&content_hash_for_bytes("image", image.bytes.as_ref())));
            }
        }
        if settings.filter_text {
            if let Ok(Some(text)) = read_clipboard_text() {
                return Ok(clipboard_change_signature(&content_hash_for_bytes("text", text.as_bytes())));
            }
        }
        Ok(String::new())
    }
}

pub fn stop_monitor(state: &AppState) {
    app_log::info(&state.paths, "clipboard", "clipboard monitor stop requested");
    if let Ok(mut guard) = state.monitor_stop.lock() {
        if let Some(stop) = guard.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

fn poll_once(app: &AppHandle, state: &AppState, last_signature: &mut String) -> Result<(), String> {
    let settings = state.settings.lock().map_err(|error| error.to_string())?.clone();
    if !settings.pin_service_enabled && !settings.history_service_enabled {
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        return poll_linux_once(app, state, &settings, last_signature);
    }

    #[cfg(not(target_os = "linux"))]
    {
        if settings.filter_file {
            if let Ok(paths) = read_file_paths_from_clipboard() {
                if !paths.is_empty() {
                    let signature = clipboard_change_signature(&content_hash_for_paths(&paths));
                    if signature != *last_signature {
                        *last_signature = signature;
                        let item = item_from_files(paths);
                        process_item(app, state, &settings, item)?;
                    }
                    // Finder 和 Explorer 会同时提供真实文件对象与文本回退；确认文件后停止读取，避免同一复制动作被重复分类。
                    // Finder and Explorer expose real file objects plus text fallbacks; stopping after a confirmed file prevents one copy action from being classified twice.
                    return Ok(());
                }
            }
        }

        // 图片必须先于文本读取，因为截图工具和富媒体应用经常同时提供图像与文本回退；先读文本会把真实图片吞掉。
        // Images must be read before text because screenshot tools and rich-media apps often expose both image data and a text fallback; text-first polling hides the real image.
        if settings.filter_image {
            if let Some(image) = read_clipboard_image()? {
                let bytes = image.bytes.to_vec();
                let signature = clipboard_change_signature(&content_hash_for_bytes("image", &bytes));
                if signature != *last_signature {
                    *last_signature = signature;
                    let item = item_from_image(image, state)?;
                    process_item(app, state, &settings, item)?;
                }
                return Ok(());
            }
        }

        if settings.filter_text {
            match read_clipboard_text() {
                Ok(Some(text)) => {
                    let signature = clipboard_change_signature(&content_hash_for_bytes("text", text.as_bytes()));
                    if signature != *last_signature {
                        *last_signature = signature;
                        let item = item_from_text(text, &settings);
                        process_item(app, state, &settings, item)?;
                    }
                }
                Ok(None) => {}
                Err(error) => return Err(format!("text clipboard read failed: {}", error)),
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
struct LinuxClipboardFilters {
    text: bool,
    image: bool,
    file: bool,
}

#[cfg(target_os = "linux")]
impl From<&AppSettings> for LinuxClipboardFilters {
    fn from(settings: &AppSettings) -> Self {
        Self {
            text: settings.filter_text,
            image: settings.filter_image,
            file: settings.filter_file,
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
struct LinuxClipboardImage {
    width: usize,
    height: usize,
    bytes: Vec<u8>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
enum LinuxClipboardContent {
    Files(Vec<String>),
    Image(LinuxClipboardImage),
    Text(String),
    Empty,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
struct LinuxClipboardSnapshot {
    change_count: u64,
    content: LinuxClipboardContent,
    diagnostics: Vec<String>,
}

#[cfg(target_os = "linux")]
fn ensure_linux_clipboard_bridge(app: &AppHandle) -> Result<(), String> {
    if LINUX_CLIPBOARD_BRIDGE_READY.load(Ordering::Acquire) {
        return Ok(());
    }
    if gtk::is_initialized_main_thread() {
        return install_linux_clipboard_bridge_on_main_thread();
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let result = catch_unwind(AssertUnwindSafe(install_linux_clipboard_bridge_on_main_thread))
            .map_err(|_| "GTK clipboard bridge initialization panicked".to_string())
            .and_then(|result| result);
        let _ = sender.send(result);
    }).map_err(|error| format!("Cannot schedule GTK clipboard bridge initialization: {}", error))?;

    receiver
        .recv_timeout(Duration::from_secs(LINUX_CLIPBOARD_MAIN_THREAD_TIMEOUT_SECONDS))
        .map_err(|error| format!("GTK clipboard bridge initialization timed out: {}", error))?
}

#[cfg(target_os = "linux")]
fn install_linux_clipboard_bridge_on_main_thread() -> Result<(), String> {
    if LINUX_CLIPBOARD_BRIDGE_READY.load(Ordering::Acquire) {
        return Ok(());
    }
    if !gtk::is_initialized_main_thread() {
        return Err("GTK clipboard bridge must be initialized on the GTK main thread".into());
    }

    LINUX_CLIPBOARD_BRIDGE.with(|slot| -> Result<(), String> {
        if slot.borrow().is_none() {
            // 让线程局部闭包显式返回 Result，才能安全传播显示服务器初始化失败，而不是在返回 () 的闭包中误用 ?。
            // Returning Result explicitly from the thread-local closure lets display initialization failures propagate safely instead of using ? inside a closure that returns ().
            let display = gdk::Display::default()
                .ok_or_else(|| "Linux display is unavailable".to_string())?;
            let clipboard = gtk::Clipboard::for_display(&display, &gdk::SELECTION_CLIPBOARD);
            // owner-change 记录的是一次真实的复制所有权变化，因此重复复制相同文件或图片也能被识别，而不会只依赖内容哈希。
            // owner-change records an actual clipboard ownership transition, so copying the same file or image again is detected instead of relying only on content hashes.
            let _owner_change_handler = clipboard.connect_local("owner-change", false, |_| {
                LINUX_CLIPBOARD_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
                None
            });
            *slot.borrow_mut() = Some(clipboard);
        }
        Ok(())
    })?;
    LINUX_CLIPBOARD_BRIDGE_READY.store(true, Ordering::Release);
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_linux_clipboard_snapshot(app: &AppHandle, settings: &AppSettings) -> Result<LinuxClipboardSnapshot, String> {
    let arboard_result = read_linux_arboard_snapshot(settings);
    let gtk_result = read_linux_gtk_snapshot(app, settings);

    match (arboard_result, gtk_result) {
        (Ok(arboard_snapshot), Ok(gtk_snapshot)) => Ok(merge_linux_clipboard_snapshots(arboard_snapshot, gtk_snapshot)),
        (Ok(mut snapshot), Err(error)) => {
            snapshot.diagnostics.push(format!("GTK probe unavailable: {}", error));
            Ok(snapshot)
        }
        (Err(error), Ok(mut snapshot)) => {
            snapshot.diagnostics.push(format!("arboard probe unavailable: {}", error));
            Ok(snapshot)
        }
        (Err(arboard_error), Err(gtk_error)) => Err(format!(
            "Linux clipboard probes failed; arboard: {}; GTK: {}",
            arboard_error, gtk_error
        )),
    }
}

#[cfg(target_os = "linux")]
fn merge_linux_clipboard_snapshots(
    mut arboard_snapshot: LinuxClipboardSnapshot,
    mut gtk_snapshot: LinuxClipboardSnapshot,
) -> LinuxClipboardSnapshot {
    // 两套后端按文件、图片、文本的优先级合并，是为了保留桌面环境提供的最完整数据，而不是让兼容文本覆盖真实对象。
    // Both backends are merged with file, image, then text priority so the richest desktop payload wins instead of a compatibility string.
    let arboard_priority = linux_content_priority(&arboard_snapshot.content);
    let gtk_priority = linux_content_priority(&gtk_snapshot.content);
    let content = if gtk_priority > arboard_priority {
        gtk_snapshot.content
    } else {
        arboard_snapshot.content
    };

    arboard_snapshot.diagnostics.append(&mut gtk_snapshot.diagnostics);
    LinuxClipboardSnapshot {
        change_count: arboard_snapshot.change_count.max(gtk_snapshot.change_count),
        content,
        diagnostics: arboard_snapshot.diagnostics,
    }
}

#[cfg(target_os = "linux")]
fn linux_content_priority(content: &LinuxClipboardContent) -> u8 {
    match content {
        LinuxClipboardContent::Files(_) => 3,
        LinuxClipboardContent::Image(_) => 2,
        LinuxClipboardContent::Text(_) => 1,
        LinuxClipboardContent::Empty => 0,
    }
}

#[cfg(target_os = "linux")]
fn linux_desired_content_priority(filters: LinuxClipboardFilters) -> u8 {
    if filters.file {
        3
    } else if filters.image {
        2
    } else if filters.text {
        1
    } else {
        0
    }
}

#[cfg(target_os = "linux")]
fn linux_snapshot_needs_non_text_retry(
    snapshot: &LinuxClipboardSnapshot,
    last_signature: &str,
    filters: LinuxClipboardFilters,
) -> bool {
    let desired_priority = linux_desired_content_priority(filters);
    desired_priority >= 2
        && linux_content_priority(&snapshot.content) < desired_priority
        && linux_snapshot_signature(snapshot) != last_signature
}

#[cfg(target_os = "linux")]
fn read_linux_snapshot_after_change(
    app: &AppHandle,
    settings: &AppSettings,
    last_signature: &str,
) -> Result<LinuxClipboardSnapshot, String> {
    let filters = LinuxClipboardFilters::from(settings);
    let mut snapshot = read_linux_clipboard_snapshot(app, settings)?;
    if !linux_snapshot_needs_non_text_retry(&snapshot, last_signature, filters) {
        return Ok(snapshot);
    }

    // Linux 剪贴板所有者可能先发布兼容文本，随后才准备图片或文件字节；变化后短暂重协商可以避免把真实对象永久写成文本历史。
    // A Linux clipboard owner may publish compatibility text before image or file bytes are ready; brief renegotiation after a change prevents the real object from being permanently stored as text history.
    for _ in 0..LINUX_NON_TEXT_RETRY_COUNT {
        thread::sleep(Duration::from_millis(LINUX_NON_TEXT_RETRY_DELAY_MS));
        let retry = read_linux_clipboard_snapshot(app, settings)?;
        snapshot = merge_linux_clipboard_snapshots(snapshot, retry);
        if linux_content_priority(&snapshot.content) >= linux_desired_content_priority(filters) {
            break;
        }
    }
    Ok(snapshot)
}

#[cfg(target_os = "linux")]
fn read_linux_arboard_snapshot(settings: &AppSettings) -> Result<LinuxClipboardSnapshot, String> {
    // 读取端每轮建立短生命周期会话，是为了获取最新 Wayland data offer；写入端仍保留长生命周期对象以维持剪贴板所有权。
    // Read polls use a short-lived session to receive the newest Wayland data offer, while writes keep the long-lived owner required by Linux clipboards.
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    let mut diagnostics = Vec::new();

    if settings.filter_file {
        match clipboard.get().file_list() {
            Ok(paths) => {
                let normalized = paths
                    .into_iter()
                    .filter(|path| path.exists())
                    .map(|path| path.to_string_lossy().to_string())
                    .collect::<Vec<_>>();
                if !normalized.is_empty() {
                    return Ok(LinuxClipboardSnapshot {
                        change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
                        content: LinuxClipboardContent::Files(normalized),
                        diagnostics,
                    });
                }
            }
            Err(error) if clipboard_content_unavailable(&error.to_string()) => {}
            Err(error) => diagnostics.push(format!("arboard file list: {}", error)),
        }
    }

    if settings.filter_image {
        match clipboard.get_image() {
            Ok(image) => {
                return Ok(LinuxClipboardSnapshot {
                    change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
                    content: LinuxClipboardContent::Image(LinuxClipboardImage {
                        width: image.width,
                        height: image.height,
                        bytes: image.bytes.into_owned(),
                    }),
                    diagnostics,
                });
            }
            Err(error) if clipboard_content_unavailable(&error.to_string()) => {}
            Err(error) => diagnostics.push(format!("arboard image: {}", error)),
        }
    }

    if settings.filter_file || settings.filter_text {
        match clipboard.get_text() {
            Ok(text) => {
                if let Some(text) = normalize_clipboard_text(text) {
                    if settings.filter_file {
                        // 某些 GNOME/KDE 来源只通过文本通道暴露 URI 列表；解析后再决定类型可以避免把文件路径写成普通文本历史。
                        // Some GNOME/KDE sources expose URI lists only through the text channel; parsing before classification prevents file paths from becoming plain-text history.
                        let paths = parse_linux_file_payload(text.as_bytes());
                        if !paths.is_empty() {
                            return Ok(LinuxClipboardSnapshot {
                                change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
                                content: LinuxClipboardContent::Files(paths),
                                diagnostics,
                            });
                        }
                    }
                    if settings.filter_text {
                        return Ok(LinuxClipboardSnapshot {
                            change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
                            content: LinuxClipboardContent::Text(text),
                            diagnostics,
                        });
                    }
                }
            }
            Err(error) if clipboard_content_unavailable(&error.to_string()) => {}
            Err(error) => diagnostics.push(format!("arboard text: {}", error)),
        }
    }

    Ok(LinuxClipboardSnapshot {
        change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
        content: LinuxClipboardContent::Empty,
        diagnostics,
    })
}

#[cfg(target_os = "linux")]
fn read_linux_gtk_snapshot(app: &AppHandle, settings: &AppSettings) -> Result<LinuxClipboardSnapshot, String> {
    ensure_linux_clipboard_bridge(app)?;
    let filters = LinuxClipboardFilters::from(settings);
    if gtk::is_initialized_main_thread() {
        return read_linux_clipboard_snapshot_on_main_thread(filters);
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let result = catch_unwind(AssertUnwindSafe(|| read_linux_clipboard_snapshot_on_main_thread(filters)))
            .map_err(|_| "GTK clipboard read panicked".to_string())
            .and_then(|result| result);
        let _ = sender.send(result);
    }).map_err(|error| format!("Cannot schedule GTK clipboard read: {}", error))?;

    receiver
        .recv_timeout(Duration::from_secs(LINUX_CLIPBOARD_MAIN_THREAD_TIMEOUT_SECONDS))
        .map_err(|error| format!("GTK clipboard read timed out: {}", error))?
}

#[cfg(target_os = "linux")]
fn read_linux_clipboard_snapshot_on_main_thread(filters: LinuxClipboardFilters) -> Result<LinuxClipboardSnapshot, String> {
    install_linux_clipboard_bridge_on_main_thread()?;
    LINUX_CLIPBOARD_BRIDGE.with(|slot| {
        let borrowed = slot.borrow();
        let clipboard = borrowed.as_ref().ok_or_else(|| "GTK clipboard bridge is unavailable".to_string())?;
        let advertised_targets = linux_clipboard_target_names(clipboard);
        let mut diagnostics = Vec::new();

        // 文件和图片优先于文本，是为了正确处理 Nautilus、截图工具和浏览器同时发布多种 MIME 表示的剪贴板内容。
        // Files and images take priority over text so Nautilus, screenshot tools, and browsers that publish multiple MIME representations are classified by their real payload.
        if filters.file {
            match read_linux_file_paths(clipboard, &advertised_targets) {
                Ok(paths) if !paths.is_empty() => {
                    return Ok(LinuxClipboardSnapshot {
                        change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
                        content: LinuxClipboardContent::Files(paths),
                        diagnostics,
                    });
                }
                Ok(_) => {}
                Err(error) => diagnostics.push(format!("file formats: {}", error)),
            }
        }

        if filters.image {
            match read_linux_image(clipboard, &advertised_targets) {
                Ok(Some(image)) => {
                    return Ok(LinuxClipboardSnapshot {
                        change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
                        content: LinuxClipboardContent::Image(image),
                        diagnostics,
                    });
                }
                Ok(None) => {}
                Err(error) => diagnostics.push(format!("image formats: {}", error)),
            }
        }

        if filters.text {
            // 直接请求文本而不先调用 wait_is_text_available，是为了避免 Wayland 数据源在“探测”和“读取”之间切换 offer 时出现假阴性。
            // Requesting text directly without a wait_is_text_available preflight avoids false negatives when a Wayland source changes its offer between probing and reading.
            if let Some(text) = clipboard.wait_for_text().and_then(|value| normalize_clipboard_text(value.to_string())) {
                return Ok(LinuxClipboardSnapshot {
                    change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
                    content: LinuxClipboardContent::Text(text),
                    diagnostics,
                });
            }
        }

        if (filters.file || filters.image) && !advertised_targets.is_empty() {
            diagnostics.push(format!(
                "advertised targets without readable non-text payload: {}",
                advertised_targets.iter().take(24).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        Ok(LinuxClipboardSnapshot {
            change_count: LINUX_CLIPBOARD_CHANGE_COUNT.load(Ordering::Relaxed),
            content: LinuxClipboardContent::Empty,
            diagnostics,
        })
    })
}

#[cfg(target_os = "linux")]
fn linux_clipboard_target_names(clipboard: &gtk::Clipboard) -> Vec<String> {
    let mut names = clipboard
        .wait_for_targets()
        .unwrap_or_default()
        .into_iter()
        // Atom::name 在当前 GTK 绑定中直接返回 GString，并非 Option；直接转换可避免把字符串错误地当成可选值调用 map。
        // Atom::name returns GString directly in the current GTK bindings rather than Option, so converting it directly avoids treating a string as an optional value.
        .map(|target| target.name().to_string())
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    names
}

#[cfg(target_os = "linux")]
fn read_linux_file_paths(clipboard: &gtk::Clipboard, advertised_targets: &[String]) -> Result<Vec<String>, String> {
    let direct_uris = clipboard.wait_for_uris().into_iter().map(|uri| uri.to_string());
    let direct_paths = parse_linux_file_entries(direct_uris);
    if !direct_paths.is_empty() {
        return Ok(direct_paths);
    }

    // GNOME、KDE、Mozilla 与通用 FreeDesktop 文件管理器发布的目标名称并不完全一致，因此已知目标和动态枚举目标都需要读取。
    // GNOME, KDE, Mozilla, and generic FreeDesktop file managers advertise different target names, so both known and dynamically discovered targets must be read.
    const FILE_TARGETS: [&str; 6] = [
        "x-special/gnome-copied-files",
        "application/x-kde4-urilist",
        "application/x-kde-urilist",
        "text/uri-list",
        "text/x-moz-url",
        "text/x-moz-url-data",
    ];
    let mut target_names = FILE_TARGETS.iter().map(|value| value.to_string()).collect::<Vec<_>>();
    for target in advertised_targets {
        let normalized = target.to_ascii_lowercase();
        if normalized.contains("uri-list")
            || normalized.contains("copied-files")
            || normalized.contains("file-list")
            || normalized.contains("moz-url")
        {
            target_names.push(target.clone());
        }
    }
    target_names.sort_by_key(|name| name.to_ascii_lowercase());
    target_names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    let mut errors = Vec::new();
    for target_name in target_names {
        match read_linux_target_bytes(clipboard, &target_name) {
            Ok(Some(bytes)) => {
                let paths = parse_linux_file_payload(&bytes);
                if !paths.is_empty() {
                    return Ok(paths);
                }
            }
            Ok(None) => {}
            Err(error) => errors.push(format!("{}: {}", target_name, error)),
        }
    }
    if errors.is_empty() {
        Ok(Vec::new())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(target_os = "linux")]
fn read_linux_target_bytes(clipboard: &gtk::Clipboard, target_name: &str) -> Result<Option<Vec<u8>>, String> {
    let target = gdk::Atom::intern(target_name);
    // Wayland 下 TARGETS 探测结果可能在真正请求数据前变化；直接请求目标比先 wait_is_target_available 再读取更可靠。
    // Under Wayland the TARGETS result can change before data is requested, so requesting the target directly is more reliable than a wait_is_target_available preflight.
    Ok(clipboard
        .wait_for_contents(&target)
        .map(|selection| selection.data())
        .filter(|bytes| !bytes.is_empty()))
}

#[cfg(target_os = "linux")]
fn parse_linux_file_payload(bytes: &[u8]) -> Vec<String> {
    let payload = decode_linux_clipboard_string(bytes);
    parse_linux_file_entries(
        payload
            .split(|character| matches!(character, '\r' | '\n' | '\0'))
            .map(str::to_string),
    )
}

#[cfg(target_os = "linux")]
fn decode_linux_clipboard_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    let zero_count = bytes.iter().filter(|byte| **byte == 0).count();
    if bytes.len() >= 4 && zero_count * 4 >= bytes.len() {
        // Mozilla 的 text/x-moz-url 在部分 Linux 桌面仍使用无 BOM 的 UTF-16LE；按零字节密度识别可避免把 URI 解码成乱码。
        // Some Linux desktops still expose Mozilla text/x-moz-url as BOM-less UTF-16LE; detecting its zero-byte density prevents URI corruption.
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(target_os = "linux")]
fn parse_linux_file_entries(entries: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for entry in entries {
        let value = entry.trim_matches(|ch: char| ch.is_whitespace() || ch == '\0');
        if value.is_empty()
            || value.starts_with('#')
            || value.eq_ignore_ascii_case("copy")
            || value.eq_ignore_ascii_case("cut")
        {
            continue;
        }

        let path = if let Ok(uri) = Url::parse(value) {
            if uri.scheme() != "file" {
                continue;
            }
            match uri.to_file_path() {
                Ok(path) => path,
                Err(_) => continue,
            }
        } else {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                continue;
            }
            path
        };
        if !path.exists() {
            continue;
        }
        let normalized = path.to_string_lossy().to_string();
        if seen.insert(normalized.clone()) {
            paths.push(normalized);
        }
    }
    paths
}

#[cfg(target_os = "linux")]
fn read_linux_image(clipboard: &gtk::Clipboard, advertised_targets: &[String]) -> Result<Option<LinuxClipboardImage>, String> {
    let mut errors = Vec::new();
    // wait_for_image 自身会协商 GTK 支持的最佳图片格式；跳过可用性预检可避免 Wayland offer 瞬态变化导致图片被错误判空。
    // wait_for_image negotiates GTK's best supported image format itself; skipping the availability preflight avoids false empties during transient Wayland offer changes.
    if let Some(pixbuf) = clipboard.wait_for_image() {
        match linux_pixbuf_to_image(&pixbuf) {
            Ok(image) => return Ok(Some(image)),
            Err(error) => errors.push(format!("GTK image conversion: {}", error)),
        }
    }

    const IMAGE_TARGETS: [&str; 9] = [
        "image/png",
        "image/jpeg",
        "image/jpg",
        "image/webp",
        "image/gif",
        "image/bmp",
        "image/x-bmp",
        "image/tiff",
        "image/x-tiff",
    ];
    let mut target_names = IMAGE_TARGETS.iter().map(|value| value.to_string()).collect::<Vec<_>>();
    target_names.extend(
        advertised_targets
            .iter()
            .filter(|target| target.to_ascii_lowercase().starts_with("image/"))
            .cloned(),
    );
    target_names.sort_by_key(|name| image_target_priority(name));
    target_names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    for target_name in target_names {
        match read_linux_target_bytes(clipboard, &target_name) {
            Ok(Some(bytes)) => match decode_linux_encoded_image(&bytes) {
                Ok(image) => return Ok(Some(image)),
                Err(error) => errors.push(format!("{}: {}", target_name, error)),
            },
            Ok(None) => {}
            Err(error) => errors.push(format!("{}: {}", target_name, error)),
        }
    }

    if errors.is_empty() {
        Ok(None)
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(target_os = "linux")]
fn image_target_priority(target: &str) -> (u8, String) {
    let normalized = target.to_ascii_lowercase();
    let priority = match normalized.as_str() {
        "image/png" => 0,
        "image/jpeg" | "image/jpg" => 1,
        "image/webp" => 2,
        "image/gif" => 3,
        "image/bmp" | "image/x-bmp" => 4,
        "image/tiff" | "image/x-tiff" => 5,
        _ => 10,
    };
    (priority, normalized)
}

#[cfg(target_os = "linux")]
fn linux_pixbuf_to_image(pixbuf: &gdk_pixbuf::Pixbuf) -> Result<LinuxClipboardImage, String> {
    if pixbuf.colorspace() != gdk_pixbuf::Colorspace::Rgb || pixbuf.bits_per_sample() != 8 {
        return Err("unsupported pixbuf colorspace or sample depth".into());
    }
    let width = usize::try_from(pixbuf.width()).map_err(|_| "invalid pixbuf width".to_string())?;
    let height = usize::try_from(pixbuf.height()).map_err(|_| "invalid pixbuf height".to_string())?;
    let channels = usize::try_from(pixbuf.n_channels()).map_err(|_| "invalid pixbuf channel count".to_string())?;
    let rowstride = usize::try_from(pixbuf.rowstride()).map_err(|_| "invalid pixbuf row stride".to_string())?;
    let pixel_bytes = pixbuf.read_pixel_bytes();
    let bytes = linux_packed_pixels_to_rgba(width, height, channels, rowstride, pixel_bytes.as_ref())?;
    Ok(LinuxClipboardImage { width, height, bytes })
}

#[cfg(target_os = "linux")]
fn linux_packed_pixels_to_rgba(
    width: usize,
    height: usize,
    channels: usize,
    rowstride: usize,
    source: &[u8],
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || !matches!(channels, 3 | 4) {
        return Err("invalid image dimensions or channel count".into());
    }
    let row_bytes = width.checked_mul(channels).ok_or_else(|| "image row size overflow".to_string())?;
    if rowstride < row_bytes {
        return Err("pixbuf row stride is smaller than one pixel row".into());
    }
    let required_source = rowstride
        .checked_mul(height.saturating_sub(1))
        .and_then(|prefix| prefix.checked_add(row_bytes))
        .ok_or_else(|| "pixbuf source size overflow".to_string())?;
    if source.len() < required_source {
        return Err("pixbuf source data is truncated".into());
    }
    let output_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "decoded image size overflow".to_string())?;
    if output_len > MAX_LINUX_DECODED_IMAGE_BYTES {
        return Err("decoded image exceeds the safety limit".into());
    }

    let mut output = Vec::with_capacity(output_len);
    for row in 0..height {
        let start = row * rowstride;
        let row_data = &source[start..start + row_bytes];
        for pixel in row_data.chunks_exact(channels) {
            output.extend_from_slice(&pixel[..3]);
            output.push(if channels == 4 { pixel[3] } else { 255 });
        }
    }
    Ok(output)
}

#[cfg(target_os = "linux")]
fn decode_linux_encoded_image(bytes: &[u8]) -> Result<LinuxClipboardImage, String> {
    if bytes.is_empty() || bytes.len() > MAX_LINUX_ENCODED_IMAGE_BYTES {
        return Err("encoded image is empty or exceeds the safety limit".into());
    }
    let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let output_len = usize::try_from(width)
        .ok()
        .and_then(|width| usize::try_from(height).ok().and_then(|height| width.checked_mul(height)))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "decoded image dimensions overflow".to_string())?;
    if output_len > MAX_LINUX_DECODED_IMAGE_BYTES {
        return Err("decoded image exceeds the safety limit".into());
    }
    Ok(LinuxClipboardImage {
        width: width as usize,
        height: height as usize,
        bytes: rgba.into_raw(),
    })
}

#[cfg(target_os = "linux")]
fn poll_linux_once(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
    last_signature: &mut String,
) -> Result<(), String> {
    // 文件管理器和截图工具的非文本 MIME 往往晚于兼容文本就绪，因此变化轮次必须完成短暂重协商后才能最终分类。
    // File managers and screenshot tools often make non-text MIME data ready after compatibility text, so a changed poll must briefly renegotiate before final classification.
    let snapshot = read_linux_snapshot_after_change(app, settings, last_signature)?;
    log_linux_clipboard_diagnostics(state, &snapshot.diagnostics);
    let signature = linux_snapshot_signature(&snapshot);
    if signature == *last_signature {
        return Ok(());
    }
    *last_signature = signature;

    let item = match snapshot.content {
        LinuxClipboardContent::Files(paths) => Some(item_from_files(paths)),
        LinuxClipboardContent::Image(image) => {
            let data = ImageData {
                width: image.width,
                height: image.height,
                bytes: Cow::Owned(image.bytes),
            };
            Some(item_from_image(data, state)?)
        }
        LinuxClipboardContent::Text(text) => Some(item_from_text(text, settings)),
        LinuxClipboardContent::Empty => None,
    };
    if let Some(item) = item {
        process_item(app, state, settings, item)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_snapshot_signature(snapshot: &LinuxClipboardSnapshot) -> String {
    let content_hash = match &snapshot.content {
        LinuxClipboardContent::Files(paths) => content_hash_for_paths(paths),
        LinuxClipboardContent::Image(image) => content_hash_for_bytes("image", &image.bytes),
        LinuxClipboardContent::Text(text) => content_hash_for_bytes("text", text.as_bytes()),
        LinuxClipboardContent::Empty => "empty".into(),
    };
    format!("linux-change:{}:{}", snapshot.change_count, content_hash)
}

#[cfg(target_os = "linux")]
fn log_linux_clipboard_diagnostics(state: &AppState, diagnostics: &[String]) {
    let current = diagnostics.join(" | ");
    let guard = LAST_LINUX_DIAGNOSTIC.get_or_init(|| Mutex::new(String::new())).lock();
    if let Ok(mut previous) = guard {
        if *previous == current {
            return;
        }
        *previous = current.clone();
        if !current.is_empty() {
            // 诊断只记录 MIME 与转换错误，不记录任何剪贴板正文，便于定位桌面环境兼容问题而不泄露用户内容。
            // Diagnostics record only MIME/conversion failures and never clipboard payloads, enabling desktop compatibility debugging without exposing user content.
            app_log::warn(&state.paths, "clipboard", format!("linux native clipboard fallback details: {}", current));
        }
    }
}

fn process_item(app: &AppHandle, state: &AppState, settings: &AppSettings, item: ClipItem) -> Result<(), String> {
    app_log::info(&state.paths, "clipboard", format!("captured item kind={:?} id={} bytes={} hash={}", item.kind, item.id, item.bytes, short_hash(&item.content_hash)));
    if should_filter_sensitive(settings, &item) {
        // 敏感过滤按设置级别运行；轻量模式只做正则/启发式检查，避免拖慢剪贴板捕获。
        // Sensitive filtering follows the configured level; light mode only uses regex-like heuristics so clipboard capture stays fast.
        app_log::warn(&state.paths, "privacy", format!("sensitive item skipped kind={:?} hash={}", item.kind, short_hash(&item.content_hash)));
        return Ok(());
    }

    let duplicate_popups = duplicate_temp_popups(state, &item)?;
    let has_pinned_duplicate_popup = duplicate_popups.iter().any(|popup| popup.is_pinned);
    let mut effective_item = item.clone();

    if settings.history_service_enabled {
        let outcome = database::insert_or_refresh(&state.paths, &item)?;
        app_log::info(
            &state.paths,
            "history",
            format!("record {} id={} kind={:?}", if outcome.was_duplicate { "refreshed" } else { "stored" }, outcome.record.id, outcome.record.kind)
        );
        effective_item = item_from_record(&outcome.record, item.is_pinned);
        let _ = app.emit("history-updated", &outcome.record.id);
    }

    if !settings.pin_service_enabled {
        return Ok(());
    }

    if has_pinned_duplicate_popup {
        app_log::info(&state.paths, "popup", format!("duplicate pinned popup kept for hash={}", short_hash(&item.content_hash)));
        return Ok(());
    }

    close_duplicate_temp_popups(app, state, &duplicate_popups)?;
    state.temp_items.lock().map_err(|error| error.to_string())?.insert(effective_item.id.clone(), effective_item.clone());
    app_log::info(&state.paths, "popup", format!("creating popup for id={} kind={:?}", effective_item.id, effective_item.kind));
    popup::create_popup(app, state, &effective_item, settings)?;
    Ok(())
}

fn item_from_record(record: &HistoryRecord, pinned: bool) -> ClipItem {
    ClipItem {
        id: record.id.clone(),
        kind: record.kind.clone(),
        summary: record.summary.clone(),
        text_content: record.text_content.clone(),
        image_path: record.image_path.clone(),
        file_paths: record.file_paths.clone(),
        bytes: record.bytes,
        created_at: record.created_at.clone(),
        content_hash: record.content_hash.clone(),
        is_pinned: pinned,
    }
}

fn short_hash(hash: &str) -> String {
    // 日志只记录哈希前缀，是为了定位重复内容问题，同时避免把完整剪贴板指纹写入诊断文件。
    // Logs keep only a hash prefix to diagnose duplicate handling without writing the full clipboard fingerprint to diagnostic files.
    hash.chars().take(12).collect()
}

fn should_filter_sensitive(settings: &AppSettings, item: &ClipItem) -> bool {
    let mode = settings.privacy_filter_mode.as_str();
    match mode {
        "off" => false,
        "smart" => item_looks_sensitive_smart(item),
        "light" => item_looks_sensitive_light(item),
        _ => settings.privacy_mode && item_looks_sensitive_light(item),
    }
}

fn item_looks_sensitive_light(item: &ClipItem) -> bool {
    match item.kind {
        ClipKind::Text => item.text_content.as_deref().map(text_looks_sensitive).unwrap_or(false),
        ClipKind::Image => false,
        ClipKind::File | ClipKind::Mixed => item.file_paths.iter().any(|path| file_path_looks_sensitive(path)),
    }
}

fn item_looks_sensitive_smart(item: &ClipItem) -> bool {
    // 智能模式目前在轻量启发式基础上对直接截图/图片采取保守策略，后续可替换为本地高级分类器。
    // Smart mode currently adds conservative direct-image filtering on top of light heuristics and can later be replaced by a local classifier.
    item_looks_sensitive_light(item) || matches!(item.kind, ClipKind::Image)
}


fn text_looks_sensitive(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if contains_private_key_block(trimmed) {
        return true;
    }

    let char_len = trimmed.chars().count();
    let line_count = trimmed.lines().count();
    if char_len > 280 || line_count > 6 {
        // 长文本最容易被隐私启发式误伤；这里只保留明确密钥赋值，避免普通文章、代码片段或日志被静默丢弃。
        // Long text is where privacy heuristics most often false-positive, so only explicit secret assignments are blocked instead of silently dropping prose, code, or logs.
        return trimmed.lines().any(line_has_explicit_secret_assignment);
    }

    let lower = trimmed.to_lowercase();
    if has_high_confidence_sensitive_marker(trimmed, &lower) || looks_like_long_secret(trimmed) {
        return true;
    }

    if looks_like_credit_card(trimmed)
        || looks_like_phone_number(trimmed)
        || looks_like_china_identity_number(trimmed)
        || looks_like_otp_code(trimmed)
    {
        return true;
    }
    false
}

fn contains_private_key_block(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("-----begin") && lower.contains("private key")
}

fn has_high_confidence_sensitive_marker(text: &str, lower: &str) -> bool {
    let direct_markers = [
        "private key", "-----begin", "ssh-rsa", "bearer ", "authorization:", "set-cookie",
        "client_secret", "refresh_token", "api_key", "apikey", "access_key",
        "验证码", "校验码", "动态码",
    ];
    if direct_markers.iter().any(|marker| lower.contains(marker)) {
        return true;
    }

    text.lines().any(line_has_explicit_secret_assignment)
}

fn line_has_explicit_secret_assignment(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() || line.chars().count() > 260 {
        return false;
    }
    let lower = line.to_lowercase();
    let assignment_markers = [
        "password", "passwd", "pwd", "secret", "token", "密钥", "密码", "令牌", "私钥", "身份证", "银行卡",
    ];
    if !assignment_markers.iter().any(|marker| lower.contains(marker)) {
        return false;
    }
    let Some(separator) = line.find('=').or_else(|| line.find(':')).or_else(|| line.find('：')) else {
        return false;
    };
    let value = line[separator + 1..].trim().trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | ',' | ';'));
    if value.is_empty() || value.contains(' ') || value.chars().count() < 4 {
        return false;
    }
    // 赋值语境下短密码也可能敏感；但仍要求是紧凑值，避免“Password: enter your password”这类说明文本被过滤。
    // Short passwords can be sensitive in assignment context, but the value must be compact so explanatory text is not filtered.
    value.chars().count() >= 8 || token_looks_like_secret(value)
}

fn looks_like_phone_number(text: &str) -> bool {
    let trimmed = text.trim();
    let char_len = trimmed.chars().count();
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() == 11 && char_len <= 24 {
        let bytes = digits.as_bytes();
        if bytes.first() == Some(&b'1') && matches!(bytes.get(1), Some(b'3'..=b'9')) {
            return true;
        }
    }
    let lower = trimmed.to_lowercase();
    let phone_markers = ["phone", "mobile", "cell", "tel", "telephone", "whatsapp", "手机号", "手机", "电话", "联系方式"];
    if digits.len() >= 7 && digits.len() <= 15 && phone_markers.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    let has_phone_shape = trimmed.contains('+')
        || trimmed.matches('-').count() >= 1
        || trimmed.contains('(')
        || trimmed.contains(')');
    if char_len <= 34 && digits.len() >= 8 && digits.len() <= 15 && has_phone_shape {
        return true;
    }
    false
}


fn looks_like_otp_code(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() > 120 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let markers = ["otp", "2fa", "mfa", "verification", "验证码", "校验码", "动态码"];
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    digits.len() >= 4 && digits.len() <= 8 && markers.iter().any(|marker| lower.contains(marker))
}

fn looks_like_china_identity_number(text: &str) -> bool {
    let compact: String = text.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect();
    if compact.len() != 18 {
        return false;
    }
    let upper = compact.to_ascii_uppercase();
    let chars: Vec<char> = upper.chars().collect();
    if !chars.iter().take(17).all(|ch| ch.is_ascii_digit()) || !(chars[17].is_ascii_digit() || chars[17] == 'X') {
        return false;
    }
    let year = chars[6..10].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    let month = chars[10..12].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    let day = chars[12..14].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    (1900..=2099).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn file_path_looks_sensitive(path: &str) -> bool {
    let lower = path.to_lowercase();
    ["password", "secret", "token", "key", "credential", "private", "密码", "密钥", "凭证", "私密"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn looks_like_credit_card(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let card_markers = ["card", "credit", "银行卡", "信用卡", "卡号"];
    if trimmed.chars().count() > 80 && !card_markers.iter().any(|marker| lower.contains(marker)) {
        return false;
    }
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut double_digit = false;
    for ch in digits.chars().rev() {
        let mut value = ch.to_digit(10).unwrap_or(0);
        if double_digit {
            value *= 2;
            if value > 9 { value -= 9; }
        }
        sum += value;
        double_digit = !double_digit;
    }
    sum % 10 == 0
}

fn looks_like_long_secret(text: &str) -> bool {
    text.split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | '<' | '>' | ',' | ';'))
        .any(token_looks_like_secret)
}

fn token_looks_like_secret(raw: &str) -> bool {
    let token = raw.trim_matches(|ch: char| matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '.'));
    let len = token.len();
    if !(32..=240).contains(&len) || token.contains('\\') || token.contains('/') || token.contains(':') {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    let known_prefixes = ["sk-", "ghp_", "gho_", "ghs_", "github_pat_", "xoxb-", "xoxp-", "akia"];
    if known_prefixes.iter().any(|prefix| lower.starts_with(prefix)) {
        return true;
    }
    if token.matches('.').count() == 2 {
        let jwt_like = token.split('.').all(|part| part.len() >= 8 && part.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
        if jwt_like {
            return true;
        }
    }
    if !token.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
        return false;
    }
    let has_upper = token.chars().any(|ch| ch.is_ascii_uppercase());
    let has_lower = token.chars().any(|ch| ch.is_ascii_lowercase());
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let separator_count = token.chars().filter(|ch| matches!(ch, '_' | '-' | '.')).count();
    let alnum_count = token.chars().filter(|ch| ch.is_ascii_alphanumeric()).count();
    // 长密钥必须是单个高熵 token，而不是把整段文本去掉空格后拼成的长串，否则普通说明文字会被误过滤。
    // A long secret must be one high-entropy token rather than a whole paragraph with spaces removed; otherwise normal prose is filtered by mistake.
    alnum_count >= 32 && has_upper && has_lower && has_digit && separator_count <= 8
}

#[derive(Clone, Debug)]
struct DuplicatePopupState {
    id: String,
    is_pinned: bool,
}

fn duplicate_temp_popups(state: &AppState, item: &ClipItem) -> Result<Vec<DuplicatePopupState>, String> {
    let guard = state.temp_items.lock().map_err(|error| error.to_string())?;
    Ok(guard.iter()
        .filter(|(id, existing)| {
            *id != &item.id
                && existing.kind == item.kind
                && !existing.content_hash.is_empty()
                && existing.content_hash == item.content_hash
        })
        .map(|(id, existing)| DuplicatePopupState { id: id.clone(), is_pinned: existing.is_pinned })
        .collect())
}

fn close_duplicate_temp_popups(app: &AppHandle, state: &AppState, duplicates: &[DuplicatePopupState]) -> Result<(), String> {
    for duplicate in duplicates.iter().filter(|duplicate| !duplicate.is_pinned) {
        app_log::info(&state.paths, "popup", format!("duplicate transient popup closed: {}", duplicate.id));
        // 只回收未置顶的同内容临时弹窗，是为了避免重复复制干扰用户已经 Pin 住的内容。
        // Only unpinned duplicate transient popups are recycled so repeated copies never disturb content the user has pinned.
        let _ = popup::close_popup(app, &duplicate.id);
        state.temp_items.lock().map_err(|error| error.to_string())?.remove(&duplicate.id);
    }
    Ok(())
}

fn item_from_files(file_paths: Vec<String>) -> ClipItem {
    let bytes = file_paths.iter().filter_map(|path| fs::metadata(path).ok()).map(|meta| meta.len() as i64).sum();
    let image_count = file_paths.iter().filter(|path| is_image_path(path)).count();
    let summary = if file_paths.len() > 1 {
        if image_count == file_paths.len() {
            format!("{} images · {}", file_paths.len(), human_size(bytes))
        } else if image_count > 0 {
            format!("{} files · {} images · {}", file_paths.len(), image_count, human_size(bytes))
        } else {
            format!("{} files · {}", file_paths.len(), human_size(bytes))
        }
    } else {
        let path = Path::new(&file_paths[0]);
        format!("{} · {}", path.file_name().and_then(|name| name.to_str()).unwrap_or("file"), human_size(bytes))
    };
    let content_hash = content_hash_for_paths(&file_paths);

    // Windows Explorer 复制文件时使用 CF_HDROP 而不是普通文本，因此文件项需要单独构造。
    // Windows Explorer uses CF_HDROP instead of plain text for copied files, so file items need their own constructor.
    ClipItem {
        id: Uuid::new_v4().to_string(),
        kind: ClipKind::File,
        summary,
        text_content: None,
        image_path: None,
        file_paths,
        bytes,
        created_at: Utc::now().to_rfc3339(),
        content_hash,
        is_pinned: false,
    }
}

fn item_from_text(text: String, settings: &AppSettings) -> ClipItem {
    let maybe_files = if settings.filter_file { parse_file_paths(&text) } else { Vec::new() };
    let kind = if maybe_files.is_empty() { ClipKind::Text } else { ClipKind::File };
    let bytes = if maybe_files.is_empty() { text.as_bytes().len() as i64 } else { maybe_files.iter().filter_map(|path| fs::metadata(path).ok()).map(|meta| meta.len() as i64).sum() };
    let summary = if maybe_files.len() > 1 {
        format!("{} files · {}", maybe_files.len(), human_size(bytes))
    } else if maybe_files.len() == 1 {
        let path = Path::new(&maybe_files[0]);
        format!("{} · {}", path.file_name().and_then(|name| name.to_str()).unwrap_or("file"), human_size(bytes))
    } else {
        text.chars().take(200).collect()
    };
    let content_hash = if maybe_files.is_empty() {
        content_hash_for_bytes("text", text.as_bytes())
    } else {
        content_hash_for_paths(&maybe_files)
    };

    // 文本剪贴板里常见 file:// 或路径列表，因此先识别文件再决定摘要，避免把文件复制误显示为普通文本。
    // Text clipboards often carry file:// or path lists, so files are recognized before summary generation to avoid misleading text cards.
    ClipItem {
        id: Uuid::new_v4().to_string(),
        kind,
        summary,
        text_content: if maybe_files.is_empty() { Some(text) } else { None },
        image_path: None,
        file_paths: maybe_files,
        bytes,
        created_at: Utc::now().to_rfc3339(),
        content_hash,
        is_pinned: false,
    }
}

fn item_from_image(image: ImageData<'_>, state: &AppState) -> Result<ClipItem, String> {
    let id = Uuid::new_v4().to_string();
    let path = state.paths.resources.join(format!("{}.clipanchorrgba", id));
    let thumb_path = state.paths.resources.join(format!("{}-thumb.png", id));
    let bytes = image.bytes.to_vec();
    let content_hash = content_hash_for_bytes("image", &bytes);
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(image.width as u32, image.height as u32, bytes.clone())
        .ok_or_else(|| "Cannot decode clipboard image buffer".to_string())?;

    // 只同步写入无压缩 RGBA 原始数据，是为了避免大图 PNG 压缩阻塞剪贴板监听线程和弹窗首帧渲染。
    // Only uncompressed RGBA data is written synchronously so large PNG compression cannot block the clipboard monitor or popup first paint.
    write_raw_clipanchor_image(&path, image.width as u32, image.height as u32, &bytes)?;

    // 缩略图尺寸很小，可以安全用于弹窗预览；真实 Copy 仍使用上面的原始 RGBA 数据。
    // The thumbnail is small enough for popup preview while Copy still restores the original RGBA payload above.
    let dynamic = image::DynamicImage::ImageRgba8(buffer);
    let thumbnail = dynamic.thumbnail(360, 220);
    thumbnail.save(&thumb_path).map_err(|error| error.to_string())?;

    Ok(ClipItem {
        id,
        kind: ClipKind::Image,
        summary: format!("Image · {} × {}", image.width, image.height),
        text_content: None,
        image_path: Some(path.to_string_lossy().to_string()),
        file_paths: Vec::new(),
        bytes: bytes.len() as i64,
        created_at: Utc::now().to_rfc3339(),
        content_hash,
        is_pinned: false,
    })
}

#[cfg(not(target_os = "linux"))]
fn read_clipboard_text() -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(Some(text)) = read_windows_text_with_retries() {
            return Ok(Some(text));
        }
    }

    let mut last_error = None;
    for attempt in 0..4 {
        match with_system_clipboard(|clipboard| clipboard.get_text()) {
            Ok(text) => return Ok(normalize_clipboard_text(text)),
            Err(error) if clipboard_content_unavailable(&error) => {
                if attempt == 3 {
                    return Ok(None);
                }
            }
            Err(error) => last_error = Some(error),
        }
        // Wayland 数据源可能在复制动作后短暂延迟提供 MIME 数据；轻量重试可覆盖该窗口，同时不显著阻塞监听线程。
        // A Wayland source may briefly delay serving MIME data after a copy action; short retries cover that gap without materially blocking the monitor.
        thread::sleep(Duration::from_millis(25));
    }
    match last_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

#[cfg(not(target_os = "linux"))]
fn read_clipboard_image() -> Result<Option<ImageData<'static>>, String> {
    match with_system_clipboard(|clipboard| clipboard.get_image()) {
        Ok(image) => Ok(Some(image)),
        Err(error) if clipboard_content_unavailable(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "windows")]
fn read_windows_text_with_retries() -> Result<Option<String>, String> {
    for attempt in 0..6 {
        if let Some(text) = read_windows_text_once()? {
            return Ok(Some(text));
        }
        if attempt < 5 {
            // Windows 剪贴板在大段文本或富文本复制时可能会短暂被来源程序占用；轻量重试可以补上这一瞬间，而不会阻塞监听线程太久。
            // Windows clipboard can be briefly owned by the source app during large or rich-text copies; a short retry catches that window without blocking the monitor for long.
            thread::sleep(Duration::from_millis(35));
        }
    }
    Ok(None)
}

#[cfg(target_os = "windows")]
fn read_windows_text_once() -> Result<Option<String>, String> {
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return Ok(None);
        }
        let mut value = None;
        if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
            value = read_windows_unicode_text_from_open_clipboard().and_then(normalize_clipboard_text);
        }
        if value.is_none() && IsClipboardFormatAvailable(CF_TEXT) != 0 {
            value = read_windows_ansi_text_from_open_clipboard(CF_TEXT).and_then(normalize_clipboard_text);
        }
        if value.is_none() && IsClipboardFormatAvailable(CF_OEMTEXT) != 0 {
            value = read_windows_ansi_text_from_open_clipboard(CF_OEMTEXT).and_then(normalize_clipboard_text);
        }
        if value.is_none() {
            value = read_windows_html_text_from_open_clipboard();
        }
        CloseClipboard();
        Ok(value)
    }
}

#[cfg(target_os = "windows")]
unsafe fn read_windows_unicode_text_from_open_clipboard() -> Option<String> {
    let handle = GetClipboardData(CF_UNICODETEXT);
    if handle.is_null() {
        return None;
    }
    let locked = GlobalLock(handle) as *const u16;
    if locked.is_null() {
        return None;
    }
    let unit_count = (GlobalSize(handle) / mem::size_of::<u16>()).max(1);
    let slice = std::slice::from_raw_parts(locked, unit_count);
    let end = slice.iter().position(|value| *value == 0).unwrap_or(slice.len());
    let text = OsString::from_wide(&slice[..end]).to_string_lossy().to_string();
    GlobalUnlock(handle);
    Some(text)
}

#[cfg(target_os = "windows")]
unsafe fn read_windows_ansi_text_from_open_clipboard(format: u32) -> Option<String> {
    let handle = GetClipboardData(format);
    if handle.is_null() {
        return None;
    }
    let locked = GlobalLock(handle) as *const u8;
    if locked.is_null() {
        return None;
    }
    let byte_count = GlobalSize(handle).max(1);
    let slice = std::slice::from_raw_parts(locked, byte_count);
    let end = slice.iter().position(|value| *value == 0).unwrap_or(slice.len());
    let text = String::from_utf8_lossy(&slice[..end]).to_string();
    GlobalUnlock(handle);
    Some(text)
}

#[cfg(target_os = "windows")]
unsafe fn read_windows_html_text_from_open_clipboard() -> Option<String> {
    let mut format_name: Vec<u16> = "HTML Format".encode_utf16().collect();
    format_name.push(0);
    let format = RegisterClipboardFormatW(format_name.as_ptr());
    if format == 0 || IsClipboardFormatAvailable(format) == 0 {
        return None;
    }
    let handle = GetClipboardData(format);
    if handle.is_null() {
        return None;
    }
    let locked = GlobalLock(handle) as *const u8;
    if locked.is_null() {
        return None;
    }
    let byte_count = GlobalSize(handle).max(1);
    let data = std::slice::from_raw_parts(locked, byte_count).to_vec();
    GlobalUnlock(handle);
    parse_windows_html_clipboard(&data)
}

fn normalize_clipboard_text(text: String) -> Option<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.trim().is_empty() { None } else { Some(normalized) }
}

#[cfg(target_os = "windows")]
fn parse_windows_html_clipboard(data: &[u8]) -> Option<String> {
    let header_len = data.len().min(2048);
    let header = String::from_utf8_lossy(&data[..header_len]);
    let start = read_cf_html_offset(&header, "StartFragment").or_else(|| read_cf_html_offset(&header, "StartHTML"));
    let end = read_cf_html_offset(&header, "EndFragment").or_else(|| read_cf_html_offset(&header, "EndHTML"));
    let fragment = match (start, end) {
        (Some(start), Some(end)) if start < end && end <= data.len() => &data[start..end],
        _ => data,
    };
    let html = String::from_utf8_lossy(fragment).to_string();
    let text = strip_html_for_clipboard(&html);
    // 富文本来源有时不给 CF_UNICODETEXT，只给 HTML Format；提取片段文本可以保证长网页文本仍会进入历史记录。
    // Some rich-text sources expose HTML Format without CF_UNICODETEXT; extracting fragment text keeps long web copies in history.
    normalize_clipboard_text(text)
}

#[cfg(target_os = "windows")]
fn read_cf_html_offset(header: &str, key: &str) -> Option<usize> {
    header.lines().find_map(|line| {
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(':'))
            .and_then(|value| value.trim().parse::<usize>().ok())
    })
}

#[cfg(target_os = "windows")]
fn strip_html_for_clipboard(html: &str) -> String {
    let without_comments = html
        .replace("<!--StartFragment-->", "")
        .replace("<!--EndFragment-->", "");
    let mut output = String::new();
    let mut in_tag = false;
    for ch in without_comments.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn parse_file_paths(text: &str) -> Vec<String> {
    let cleaned = text.replace('\r', "");
    let candidates: Vec<String> = cleaned.lines()
        .filter_map(normalize_clipboard_path)
        .filter(|line| !line.is_empty())
        .collect();
    if !candidates.is_empty() && candidates.iter().all(|path| Path::new(path).exists()) {
        candidates
    } else {
        Vec::new()
    }
}

fn normalize_clipboard_path(line: &str) -> Option<String> {
    let mut value = line.trim().trim_matches('"').to_string();
    if value.is_empty() {
        return None;
    }
    #[cfg(target_os = "linux")]
    if value.starts_with("file:") {
        // Linux 文件 URI 必须通过标准解析器转换，否则直接裁剪 file:/// 会把绝对路径的根斜杠一起删除。
        // Linux file URIs must be converted through the standard parser because trimming file:/// directly also removes the leading slash of an absolute path.
        return Url::parse(&value).ok()?.to_file_path().ok().map(|path| path.to_string_lossy().to_string());
    }
    if let Some(rest) = value.strip_prefix("file:///") {
        value = rest.to_string();
    } else if let Some(rest) = value.strip_prefix("file://") {
        value = rest.to_string();
    }
    Some(value.replace("%20", " "))
}

#[cfg(target_os = "windows")]
fn read_file_paths_from_clipboard() -> Result<Vec<String>, String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP) == 0 {
            return Ok(Vec::new());
        }
        if OpenClipboard(null_mut()) == 0 {
            return Err("Cannot open Windows clipboard".into());
        }

        let handle = GetClipboardData(CF_HDROP);
        if handle.is_null() {
            CloseClipboard();
            return Ok(Vec::new());
        }

        let count = DragQueryFileW(handle, u32::MAX, null_mut(), 0);
        let mut paths = Vec::new();
        for index in 0..count {
            let len = DragQueryFileW(handle, index, null_mut(), 0);
            if len == 0 {
                continue;
            }
            let mut buffer = vec![0u16; (len + 1) as usize];
            let written = DragQueryFileW(handle, index, buffer.as_mut_ptr(), len + 1);
            if written > 0 {
                let value = OsString::from_wide(&buffer[..written as usize]).to_string_lossy().to_string();
                if Path::new(&value).exists() {
                    paths.push(value);
                }
            }
        }
        CloseClipboard();
        Ok(paths)
    }
}

#[cfg(target_os = "macos")]
fn read_file_paths_from_clipboard() -> Result<Vec<String>, String> {
    // Finder 在多文件复制时会同时提供文件 URL 和文件名文本；优先读取原生文件 URL 才能避免被误判成拼接文本。
    // Finder exposes file URLs and filename text for multi-file copies; reading native file URLs first prevents them from being misclassified as concatenated text.
    crate::macos_native::read_file_paths_from_pasteboard()
}

pub fn content_hash_for_bytes(prefix: &str, bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in prefix.as_bytes().iter().chain(bytes.iter()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{}:{:016x}", prefix, hash)
}

pub fn content_hash_for_paths(paths: &[String]) -> String {
    // 路径列表使用稳定哈希而不是数据库 id，是为了让同一批文件重复复制时能识别为同一内容。
    // File lists use a stable hash instead of the database id so repeated copies of the same files are recognized as the same content.
    content_hash_for_bytes("file", paths.join("\u{1f}").as_bytes())
}

// Linux 使用包含原生 change count 的独立快照签名；只在其他平台编译此函数，可避免生成永远不会被调用的符号与 dead_code 警告。
// Linux uses a dedicated snapshot signature with the native change count; compiling this helper only elsewhere avoids unreachable symbols and dead_code warnings.
#[cfg(not(target_os = "linux"))]
fn clipboard_change_signature(content_hash: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if sequence != 0 {
            // Windows 会在“复制同一内容”时递增序列号；把序列号并入监听指纹，才能按需求每次复制都触发新弹窗与去重写入。
            // Windows increments the sequence number even when the same content is copied; including it lets every copy create a fresh popup and deduplicated record.
            return format!("seq:{}:{}", sequence, content_hash);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(change_count) = crate::macos_native::pasteboard_change_count() {
            // macOS 没有 Windows 的序列号 API，但 NSPasteboard changeCount 会在用户重新复制相同内容时递增；加入它既满足“每次复制都生成新弹窗”，也让同一次文件复制的文件/文本表示共享同一轮变化判断。
            // macOS has no Windows sequence-number API, but NSPasteboard changeCount increments when the user copies the same content again; including it preserves per-copy popups and keeps file/text representations tied to the same pasteboard change.
            return format!("change:{}:{}", change_count, content_hash);
        }
    }
    content_hash.to_string()
}

fn human_size(bytes: i64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value > 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", value, units[unit])
}

pub fn copy_to_clipboard(record: &HistoryRecord) -> Result<(), String> {
    match record.kind {
        ClipKind::Image => {
            let path = record.image_path.as_ref().ok_or_else(|| "Image path is missing".to_string())?;
            let (width, height, bytes) = read_image_rgba_for_clipboard(path)?;
            let data = ImageData { width: width as usize, height: height as usize, bytes: Cow::Owned(bytes) };
            with_system_clipboard(|clipboard| clipboard.set_image(data))?;
        }
        ClipKind::File => {
            copy_file_paths_to_clipboard(&record.file_paths)?;
        }
        _ => {
            let text = record.text_content.clone().unwrap_or_else(|| record.summary.clone());
            with_system_clipboard(|clipboard| clipboard.set_text(text))?;
        }
    }
    Ok(())
}

const RAW_IMAGE_MAGIC: &[u8; 8] = b"CLIPRGBA";

fn write_raw_clipanchor_image(path: &Path, width: u32, height: u32, bytes: &[u8]) -> Result<(), String> {
    let mut output = Vec::with_capacity(16 + bytes.len());
    output.extend_from_slice(RAW_IMAGE_MAGIC);
    output.extend_from_slice(&width.to_le_bytes());
    output.extend_from_slice(&height.to_le_bytes());
    output.extend_from_slice(bytes);
    fs::write(path, output).map_err(|error| error.to_string())
}

pub fn is_raw_clipanchor_image(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("clipanchorrgba"))
        .unwrap_or(false)
}

fn read_raw_clipanchor_image(path: &str) -> Result<Option<(u32, u32, Vec<u8>)>, String> {
    if !is_raw_clipanchor_image(path) {
        return Ok(None);
    }
    let data = fs::read(path).map_err(|error| error.to_string())?;
    if data.len() < 16 || &data[..8] != RAW_IMAGE_MAGIC {
        return Err("Invalid ClipAnchor image payload".into());
    }
    let width = u32::from_le_bytes(data[8..12].try_into().map_err(|_| "Invalid image width".to_string())?);
    let height = u32::from_le_bytes(data[12..16].try_into().map_err(|_| "Invalid image height".to_string())?);
    let bytes = data[16..].to_vec();
    if bytes.len() != width as usize * height as usize * 4 {
        return Err("Invalid ClipAnchor image byte length".into());
    }
    Ok(Some((width, height, bytes)))
}

fn read_image_rgba_for_clipboard(path: &str) -> Result<(u32, u32, Vec<u8>), String> {
    if let Some((width, height, bytes)) = read_raw_clipanchor_image(path)? {
        return Ok((width, height, bytes));
    }
    let image = image::open(path).map_err(|error| error.to_string())?.to_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}

pub fn thumbnail_bytes_for_path(path: &str, max_width: u32, max_height: u32) -> Result<Vec<u8>, String> {
    let dynamic = if let Some((width, height, bytes)) = read_raw_clipanchor_image(path)? {
        let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, bytes)
            .ok_or_else(|| "Cannot rebuild raw ClipAnchor image".to_string())?;
        image::DynamicImage::ImageRgba8(buffer)
    } else {
        image::open(path).map_err(|error| error.to_string())?
    };
    let thumbnail = dynamic.thumbnail(max_width, max_height);
    let mut cursor = Cursor::new(Vec::new());
    thumbnail.write_to(&mut cursor, image::ImageFormat::Png).map_err(|error| error.to_string())?;
    Ok(cursor.into_inner())
}

pub fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "tif" | "tiff"))
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn copy_file_paths_to_clipboard(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No file paths to copy".into());
    }
    let valid_paths: Vec<String> = paths.iter().filter(|path| Path::new(path.as_str()).exists()).cloned().collect();
    if valid_paths.is_empty() {
        return Err("No valid files to copy".into());
    }

    let mut encoded_paths: Vec<u16> = Vec::new();
    for path in &valid_paths {
        encoded_paths.extend(OsString::from(path).encode_wide());
        encoded_paths.push(0);
    }
    encoded_paths.push(0);

    let header_size = 20usize;
    let total_size = header_size + encoded_paths.len() * mem::size_of::<u16>();

    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, total_size);
        if memory.is_null() {
            return Err("Cannot allocate clipboard file payload".into());
        }
        let locked = GlobalLock(memory) as *mut u8;
        if locked.is_null() {
            return Err("Cannot lock clipboard file payload".into());
        }

        // CF_HDROP 需要 DROPFILES 头和双零结尾的 UTF-16 路径列表；这样复制出来的是文件对象而不是路径文本。
        // CF_HDROP needs a DROPFILES header plus a double-null-terminated UTF-16 path list, so the clipboard contains file objects rather than path text.
        ptr::write_unaligned(locked.add(0) as *mut u32, header_size as u32);
        ptr::write_unaligned(locked.add(4) as *mut i32, 0);
        ptr::write_unaligned(locked.add(8) as *mut i32, 0);
        ptr::write_unaligned(locked.add(12) as *mut i32, 0);
        ptr::write_unaligned(locked.add(16) as *mut i32, 1);
        ptr::copy_nonoverlapping(encoded_paths.as_ptr() as *const u8, locked.add(header_size), encoded_paths.len() * mem::size_of::<u16>());
        GlobalUnlock(memory);

        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("Cannot open clipboard for files".into());
        }
        EmptyClipboard();
        let placed = SetClipboardData(CF_HDROP, memory);
        CloseClipboard();
        if placed.is_null() {
            return Err("Cannot place files on clipboard".into());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_file_paths_to_clipboard(paths: &[String]) -> Result<(), String> {
    // macOS 必须写入 NSURL 对象，Finder 和其他应用才会把内容当作文件对象而不是路径文本粘贴。
    // macOS must receive NSURL objects so Finder and other apps paste real file objects instead of path text.
    crate::macos_native::write_file_paths_to_pasteboard(paths)
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn copy_file_paths_to_clipboard(paths: &[String]) -> Result<(), String> {
    let valid_paths = paths
        .iter()
        .map(Path::new)
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if valid_paths.is_empty() {
        return Err("No valid files to copy".into());
    }
    // Linux 通过 arboard 的文件列表 MIME 写入真实文件对象，避免文件管理器只收到不可操作的路径文本。
    // Linux writes real file-list MIME data through arboard so file managers receive pasteable objects instead of inert path text.
    with_system_clipboard(|clipboard| clipboard.set().file_list(&valid_paths))
}


#[cfg(all(test, target_os = "linux"))]
mod linux_clipboard_tests {
    use super::*;

    #[test]
    fn parses_gnome_file_payload_with_crlf_and_encoded_spaces() {
        let root = std::env::temp_dir().join(format!("clipanchor-linux-clipboard-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temporary directory should be created");
        let file = root.join("sample image.png");
        fs::write(&file, b"test").expect("temporary file should be written");
        let uri = Url::from_file_path(&file).expect("temporary file should convert to URI");
        let payload = format!("copy\r\n{}\r\n{}\r\n", uri, uri);

        let paths = parse_linux_file_payload(payload.as_bytes());
        assert_eq!(paths, vec![file.to_string_lossy().to_string()]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn converts_padded_rgb_rows_to_rgba() {
        let source = [
            10, 20, 30, 40, 50, 60, 0, 0,
            70, 80, 90, 100, 110, 120, 0, 0,
        ];
        let rgba = linux_packed_pixels_to_rgba(2, 2, 3, 8, &source).expect("RGB pixbuf should convert");
        assert_eq!(
            rgba,
            vec![
                10, 20, 30, 255, 40, 50, 60, 255,
                70, 80, 90, 255, 100, 110, 120, 255,
            ]
        );
    }

    #[test]
    fn preserves_rgba_alpha_channel() {
        let source = [10, 20, 30, 40, 50, 60, 70, 80];
        let rgba = linux_packed_pixels_to_rgba(2, 1, 4, 8, &source).expect("RGBA pixbuf should convert");
        assert_eq!(rgba, source);
    }

    #[test]
    fn retries_when_new_compatibility_text_may_hide_a_file() {
        let snapshot = LinuxClipboardSnapshot {
            change_count: 8,
            content: LinuxClipboardContent::Text("sample.png".into()),
            diagnostics: Vec::new(),
        };
        let filters = LinuxClipboardFilters { text: true, image: true, file: true };
        assert!(linux_snapshot_needs_non_text_retry(&snapshot, "linux-change:7:old", filters));
    }

    #[test]
    fn merged_snapshot_keeps_the_richer_non_text_payload() {
        let text = LinuxClipboardSnapshot {
            change_count: 9,
            content: LinuxClipboardContent::Text("fallback".into()),
            diagnostics: vec!["text first".into()],
        };
        let image = LinuxClipboardSnapshot {
            change_count: 9,
            content: LinuxClipboardContent::Image(LinuxClipboardImage {
                width: 1,
                height: 1,
                bytes: vec![1, 2, 3, 255],
            }),
            diagnostics: vec!["image ready".into()],
        };
        let merged = merge_linux_clipboard_snapshots(text, image);
        assert!(matches!(merged.content, LinuxClipboardContent::Image(_)));
        assert_eq!(merged.diagnostics.len(), 2);
    }

    #[test]
    fn file_uri_conversion_preserves_the_absolute_root() {
        let root = std::env::temp_dir().join(format!("clipanchor-linux-uri-{}", Uuid::new_v4()));
        fs::write(&root, b"test").expect("temporary file should be written");
        let uri = Url::from_file_path(&root).expect("temporary path should convert to URI");
        let normalized = normalize_clipboard_path(uri.as_str()).expect("file URI should normalize");
        assert_eq!(normalized, root.to_string_lossy().to_string());
        let _ = fs::remove_file(root);
    }
}
