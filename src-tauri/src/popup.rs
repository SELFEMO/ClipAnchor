use crate::{app_log, models::{AppSettings, AppState, ClipItem, ClipKind}, settings};
use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Position, Size, WebviewUrl, WebviewWindowBuilder};

pub fn create_popup(app: &AppHandle, state: &AppState, item: &ClipItem, settings: &AppSettings) -> Result<(), String> {
    schedule_popup(app, state, item, settings, false)
}

pub fn create_pinned_popup(app: &AppHandle, state: &AppState, item: &ClipItem, settings: &AppSettings) -> Result<(), String> {
    schedule_popup(app, state, item, settings, true)
}

fn schedule_popup(app: &AppHandle, state: &AppState, item: &ClipItem, settings: &AppSettings, pinned: bool) -> Result<(), String> {
    app_log::info(&state.paths, "popup", format!("popup scheduled id={} pinned={}", item.id, pinned));
    let app_for_window = app.clone();
    let state_for_window = state.clone();
    let item_for_window = item.clone();
    let settings_for_window = settings.clone();

    // Tauri/WebView 窗口必须回到主线程创建，否则剪贴板监听线程直接建窗在 Windows 上可能出现不弹窗或 WebView 卡死。
    // Tauri/WebView windows must be created on the main thread; building them from the clipboard monitor thread can make Windows popups fail or freeze.
    app.run_on_main_thread(move || {
        if let Err(error) = build_popup_window(&app_for_window, &state_for_window, &item_for_window, &settings_for_window, pinned) {
            app_log::error(&state_for_window.paths, "popup", format!("popup creation failed id={}: {}", item_for_window.id, error));
            let _ = app_for_window.emit("clipanchor-log", format!("Popup creation failed: {}", error));
        }
    }).map_err(|error| error.to_string())
}

fn build_popup_window(app: &AppHandle, state: &AppState, item: &ClipItem, settings: &AppSettings, pinned: bool) -> Result<(), String> {
    // macOS 后台弹窗先切换为辅助应用策略，是为了让提示卡跟随当前 Space 而不是被 Dock 主应用状态固定在首个桌面。
    // macOS background popups switch to the accessory app policy first so hint cards follow the current Space instead of the Dock-backed main app Space.
    crate::macos_native::prepare_background_popup(app);
    prune_transient_popups(app, state, pinned)?;
    let (popup_width, popup_height) = popup_size_for_item(settings, item);
    let active_count = active_popup_count(app);
    let offset = ((active_count % 6) as f64) * 20.0;
    let (base_x, base_y) = safe_popup_position_for_app(app, settings.popup_x, settings.popup_y, popup_width, popup_height);
    let (x, y) = safe_popup_position_for_app(app, base_x + offset, base_y + offset, popup_width, popup_height);
    let label = popup_label(&item.id);
    let url = WebviewUrl::App(format!("index.html?view=popup&id={}", encode_query_component(&item.id)).into());

    if let Some(existing) = app.get_webview_window(&label) {
        app_log::info(&state.paths, "popup", format!("existing popup restored label={}", label));
        // 已存在弹窗时只恢复置顶，不主动抢焦点，避免打断用户当前正在输入的应用。
        // When a popup already exists, only restore top-most state without stealing focus from the active application.
        existing.set_always_on_top(true).map_err(|error| error.to_string())?;
        crate::macos_native::configure_popup_for_current_space(&existing);
        let _ = existing.show();
        return Ok(());
    }

    // 使用独立窗口承载每次复制的弹窗，保证新内容不会覆盖旧内容，也便于系统级置顶。
    // Each copy uses its own window so new content never replaces existing popups and can stay always-on-top.
    // 窗口创建时就启用原生可缩放性，是为了避免用户刚点击 Pin 后角标已出现但后端尚未切换 resizable 导致拖拽失效。
    // Native resizability is enabled from creation so the handle cannot appear before the backend has made the window resizable.
    let window = WebviewWindowBuilder::new(app, label.clone(), url)
        .title(label.clone())
        .inner_size(popup_width, popup_height)
        .min_inner_size(260.0, 118.0)
        .position(x, y)
        .decorations(false)
        // 透明窗口是弹窗圆角生效的原生前提；仅靠 CSS 圆角无法裁掉 WebView 宿主窗口的直角底板。
        // A transparent native window is required for popup corners; CSS radius alone cannot clip the square WebView host surface.
        .transparent(true)
        .shadow(false)
        .resizable(true)
        .focused(false)
        .always_on_top(true)
        // 弹窗窗口声明为所有工作区可见，是为了让 macOS 全屏 Space 中的复制提示不被限制到主窗口所在桌面。
        // The popup is declared visible on all workspaces so macOS fullscreen Space hints are not confined to the main window's desktop.
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
    // 关闭原生阴影后再裁剪窗口，是因为 Windows 的无边框阴影会额外画出 1px 白色直角边框。
    // Native shadow is disabled before clipping because Windows borderless shadows can draw an extra 1px white square border.
    let _ = window.set_shadow(false);
    crate::macos_native::configure_popup_for_current_space(&window);
    apply_native_popup_shape(&window);
    window.set_position(Position::Logical(LogicalPosition { x, y })).map_err(|error| error.to_string())?;
    if pinned {
        // 历史记录回放需要一出生就是置顶态，同时启用原生可缩放性，让前端角标能调用系统级 resize 拖拽而不是高频手动改尺寸。
        // History replay must be born pinned and resizable so the frontend corner can use OS-level resize dragging instead of high-frequency manual resizing.
        window.set_always_on_top(true).map_err(|error| error.to_string())?;
        window.set_resizable(true).map_err(|error| error.to_string())?;
    }
    app_log::info(&state.paths, "popup", format!("popup window built label={} size={:.0}x{:.0} pos={:.0},{:.0}", label, popup_width, popup_height, x, y));
    delayed_show_popup(app, &popup_label(&item.id), pinned, x, y);
    Ok(())
}

fn delayed_show_popup(app: &AppHandle, label: &str, pinned: bool, x: f64, y: f64) {
    let handle = app.clone();
    let label = label.to_string();
    thread::spawn(move || {
        // 先让 WebView 完成初始导航再显示窗口，是为了减少历史记录 Pin 弹窗短暂白屏甚至卡住的概率。
        // The window is shown after the first WebView navigation window has a moment to settle, reducing white or frozen popups from history pinning.
        thread::sleep(Duration::from_millis(if pinned { 260 } else { 120 }));
        let ui_handle = handle.clone();
        let label_for_show = label.clone();
        let _ = handle.run_on_main_thread(move || {
            if let Some(window) = ui_handle.get_webview_window(&label_for_show) {
                // Linux 窗口管理器常忽略未显示窗口的初始 position；显示前后都设置一次可避免 deb/rpm 包中弹窗总被居中。
                // Linux window managers often ignore the initial position of hidden windows; setting it before and after show prevents deb/rpm popups from staying centered.
                let _ = window.set_position(Position::Logical(LogicalPosition { x, y }));
                let _ = window.set_shadow(false);
                crate::macos_native::configure_popup_for_current_space(&window);
                let _ = window.show();
                let _ = window.set_position(Position::Logical(LogicalPosition { x, y }));
                // 显示后再次应用圆角 Region，是为了覆盖 WebView2 首次显示时重建底层窗口导致的裁剪丢失。
                // The rounded region is reapplied after show to survive WebView2 recreating its backing window during first paint.
                apply_native_popup_shape(&window);
            }
        });
        schedule_linux_position_reapply(handle, label, x, y);
    });
}

#[cfg(target_os = "linux")]
fn schedule_linux_position_reapply(app: AppHandle, label: String, x: f64, y: f64) {
    for delay in [80_u64, 260_u64] {
        let handle = app.clone();
        let label_for_pass = label.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(delay));
            let ui_handle = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if let Some(window) = ui_handle.get_webview_window(&label_for_pass) {
                    // GTK/WebKit 在映射窗口后可能再次交给窗口管理器居中；短延迟重放位置让最终落点服从用户设置。
                    // GTK/WebKit can hand the mapped window back to the window manager for centering; a short replay makes the final landing follow user settings.
                    let _ = window.set_position(Position::Logical(LogicalPosition { x, y }));
                }
            });
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn schedule_linux_position_reapply(_app: AppHandle, _label: String, _x: f64, _y: f64) {}



fn apply_native_popup_shape(window: &tauri::WebviewWindow) {
    // 弹窗仍使用原生 Region 裁剪，是因为小型置顶窗口没有主界面的隐藏缩放边框问题，可以稳定保留既有圆角效果。
    // Popups still use native Region clipping because small always-on-top windows do not have the main UI's hidden resize-border artifact and can keep the existing radius.
    crate::window_shape::apply_clipanchor_radius(window);
}

pub fn open_position_overlay(_app: &AppHandle) -> Result<(), String> {
    // 旧的独立定位窗口入口保留为安全空操作，是为了兼容旧前端调用但彻底避免再次创建会卡死的 WebView 窗口。
    // The legacy standalone locator entrypoint remains a safe no-op for compatibility while preventing any freezing WebView window from being created again.
    Ok(())
}

pub fn close_popup(app: &AppHandle, id: &str) -> Result<(), String> {
    if let Some(state) = app.try_state::<AppState>() { app_log::info(&state.paths, "popup", format!("close window by id={}", id)); }
    if let Some(window) = app.get_webview_window(&popup_label(id)) {
        window.close().map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn pin_popup(app: &AppHandle, id: &str) -> Result<(), String> {
    if let Some(state) = app.try_state::<AppState>() { app_log::info(&state.paths, "popup", format!("pin window by id={}", id)); }
    if let Some(window) = app.get_webview_window(&popup_label(id)) {
        window.set_always_on_top(true).map_err(|error| error.to_string())?;
        // Pin 后启用原生可缩放性，是为了让右下角角标走系统级 resize 拖拽，避免前端逐帧调用后端 resize 导致鼠标跟随延迟。
        // Pinned popups enable native resizability so the lower-right handle uses OS-level resize dragging instead of per-frame backend resize calls.
        window.set_resizable(true).map_err(|error| error.to_string())?;
        apply_native_popup_shape(&window);
    }
    Ok(())
}

pub fn resize_popup(app: &AppHandle, id: &str, width: f64, height: f64) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(&popup_label(id)) {
        let width = width.clamp(260.0, 720.0);
        let height = height.clamp(118.0, 520.0);
        // 调整的是窗口本身尺寸而不是内容缩放，是为了让用户拖动弹窗右下角时获得接近原生应用的尺寸反馈。
        // The window itself is resized rather than only scaling content so the custom lower-right handle feels like a native resize control.
        window.set_size(Size::Logical(LogicalSize { width, height })).map_err(|error| error.to_string())?;
        apply_native_popup_shape(&window);
    }
    Ok(())
}


pub fn refresh_popup_shape(app: &AppHandle, id: &str) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(&popup_label(id)) {
        // 原生缩放结束后刷新窗口裁剪，是为了保持 Windows 圆角 Region 与最新尺寸一致，而不是在拖动过程中高频刷新。
        // The native clipping region is refreshed after resizing settles so Windows corners match the latest size without high-frequency redraws during drag.
        apply_native_popup_shape(&window);
    }
    Ok(())
}

pub fn save_position(app: &AppHandle, state: &AppState, x: f64, y: f64) -> Result<(), String> {
    app_log::info(&state.paths, "settings", format!("save popup anchor requested: {:.0},{:.0}", x, y));
    let settings_snapshot = state.settings.lock().map_err(|error| error.to_string())?.clone();
    let (popup_width, popup_height) = popup_size(&settings_snapshot);
    let (safe_x, safe_y) = safe_popup_position_for_app(app, x, y, popup_width, popup_height);
    let mut settings_guard = state.settings.lock().map_err(|error| error.to_string())?;
    // 保存前按当前主显示器裁剪坐标，是为了让设置页预览边界与真实弹窗生成边界保持一致。
    // Coordinates are clamped against the current primary monitor so the settings preview boundary matches real popup spawning.
    settings_guard.popup_x = safe_x;
    settings_guard.popup_y = safe_y;
    settings::save(&state.paths, &settings_guard)
}

fn safe_popup_position_for_app(app: &AppHandle, x: f64, y: f64, popup_width: f64, popup_height: f64) -> (f64, f64) {
    let (max_x, max_y) = app.primary_monitor().ok().flatten().map(|monitor| {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let width = size.width as f64 / scale;
        let height = size.height as f64 / scale;
        ((width - popup_width).max(0.0), (height - popup_height).max(0.0))
    }).unwrap_or((4096.0, 4096.0));
    let safe_x = if x.is_finite() { x.clamp(0.0, max_x) } else { 24.0 };
    let safe_y = if y.is_finite() { y.clamp(0.0, max_y) } else { 24.0 };
    (safe_x, safe_y)
}

fn popup_size(settings: &AppSettings) -> (f64, f64) {
    let scale = (settings.popup_scale_percent as f64).clamp(50.0, 200.0) / 100.0;
    let base_width = if settings.popup_width.is_finite() { settings.popup_width.clamp(280.0, 520.0) } else { 340.0 };
    let base_height = if settings.popup_height.is_finite() { settings.popup_height.clamp(160.0, 360.0) } else { 220.0 };
    // 设置页的位置地图仍使用通用尺寸，是为了让用户保存的默认锚点不会随某一次复制内容类型跳变。
    // The settings position map keeps using the generic size so the saved anchor does not jump because of one copied content type.
    (base_width * scale, base_height * scale)
}

fn popup_size_for_item(settings: &AppSettings, item: &ClipItem) -> (f64, f64) {
    let scale = (settings.popup_scale_percent as f64).clamp(50.0, 200.0) / 100.0;
    let base_width = if settings.popup_width.is_finite() { settings.popup_width.clamp(280.0, 520.0) } else { 340.0 };
    let (base_height, max_height) = match item.kind {
        ClipKind::Image => (150.0, 248.0),
        ClipKind::File | ClipKind::Mixed => {
            let rows = ((item.file_paths.len().max(1) as f64) / 2.0).ceil();
            let content_height = 82.0 + rows.min(9.0) * 50.0;
            let default_height = if settings.popup_height.is_finite() { settings.popup_height.clamp(160.0, 360.0) } else { 220.0 };
            // 文件弹窗保留设置中的默认初始高度，是为了少量文件也有舒适边距；这不是最小值，用户 Pin 后仍可继续缩小窗口。
            // File popups keep the configured default initial height so small selections have comfortable margins; this is not a minimum because pinned windows remain resizable.
            (content_height.max(default_height), 500.0)
        }
        ClipKind::Text => (estimate_text_popup_height(item, base_width), 360.0),
    };
    // 初始高度按内容估算，是为了让隐藏按钮成为覆盖层而不是看起来给按钮预留一块空白区域。
    // The initial height is content-based so hidden actions behave like an overlay instead of looking like reserved blank space.
    (base_width * scale, base_height.clamp(112.0, max_height) * scale)
}

fn estimate_text_popup_height(item: &ClipItem, base_width: f64) -> f64 {
    let text = item.text_content.as_deref().unwrap_or(&item.summary);
    let usable_width = (base_width - 32.0).max(220.0);
    let chars_per_line = (usable_width / 8.6).floor().max(24.0);
    let wrapped_lines = text.lines().map(|line| {
        let count = line.chars().count().max(1) as f64;
        (count / chars_per_line).ceil().max(1.0)
    }).sum::<f64>().max(1.0);
    // 文本高度用轻量估算而不等前端回传尺寸，是为了复制后首帧就贴近内容，避免底部出现明显空带。
    // Text height is estimated without a frontend roundtrip so the first frame fits the content and avoids a visible bottom blank band.
    48.0 + wrapped_lines.min(7.0) * 20.0
}

fn prune_transient_popups(app: &AppHandle, state: &AppState, incoming_pinned: bool) -> Result<(), String> {
    if incoming_pinned {
        return Ok(());
    }
    let candidates = {
        let guard = state.temp_items.lock().map_err(|error| error.to_string())?;
        guard.iter()
            .filter(|(_, item)| !item.is_pinned)
            .map(|(id, item)| (id.clone(), item.created_at.clone()))
            .collect::<Vec<_>>()
    };
    if candidates.len() <= 8 {
        return Ok(());
    }
    let mut ordered = candidates;
    ordered.sort_by(|a, b| a.1.cmp(&b.1));
    for (id, _) in ordered.into_iter().take(3) {
        // 未置顶弹窗过多时先回收最旧的临时窗口，是为了避免 Windows WebView 同时创建过多置顶小窗导致白屏或卡死。
        // Old transient popups are recycled when too many are open to avoid Windows WebView freezes from too many always-on-top windows.
        let _ = close_popup(app, &id);
        state.temp_items.lock().map_err(|error| error.to_string())?.remove(&id);
    }
    Ok(())
}

fn active_popup_count(app: &AppHandle) -> usize {
    // 只根据当前仍然存在的弹窗计算堆叠偏移，是为了让所有未钉住弹窗消失后新弹窗回到用户保存的默认位置。
    // The stack offset is based only on currently existing popup windows so a new popup returns to the saved default position after previous temporary popups disappear.
    app.webview_windows().keys().filter(|label| label.starts_with("clipanchor-popup-")).count()
}

fn popup_label(id: &str) -> String {
    format!("clipanchor-popup-{}", id)
}


fn encode_query_component(value: &str) -> String {
    value.bytes().map(|byte| {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            _ => format!("%{:02X}", byte),
        }
    }).collect::<Vec<_>>().join("")
}
