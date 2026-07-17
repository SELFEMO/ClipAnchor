#[cfg(target_os = "windows")]
mod native {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{ffi::c_void, mem::size_of};
    use tauri::{Runtime, WebviewWindow};
    use windows_sys::Win32::{Foundation::HWND, Graphics::Dwm::DwmSetWindowAttribute};

    type NativeRegion = *mut c_void;

    #[repr(C)]
    struct NativeRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClientRect(hwnd: HWND, rect: *mut NativeRect) -> i32;
        fn SetWindowRgn(hwnd: HWND, region: NativeRegion, redraw: i32) -> i32;
        fn GetDpiForWindow(hwnd: HWND) -> u32;
        fn RedrawWindow(hwnd: HWND, rect: *const NativeRect, region: NativeRegion, flags: u32) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateRoundRectRgn(
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
            ellipse_width: i32,
            ellipse_height: i32,
        ) -> NativeRegion;
        fn DeleteObject(object: *mut c_void) -> i32;
    }

    pub fn apply<R: Runtime>(window: &WebviewWindow<R>, radius_logical_px: f64) {
        let Some(hwnd) = hwnd_from_tauri_window(window) else {
            return;
        };

        disable_dwm_extra_corner(hwnd);
        apply_exact_window_region(hwnd, radius_logical_px);
    }

    fn hwnd_from_tauri_window<R: Runtime>(window: &WebviewWindow<R>) -> Option<HWND> {
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return None;
        };

        let hwnd = win32.hwnd.get() as *mut c_void as HWND;
        if hwnd.is_null() {
            None
        } else {
            Some(hwnd)
        }
    }

    fn disable_dwm_extra_corner(hwnd: HWND) {
        const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
        const DWMWCP_DONOTROUND: i32 = 1;
        let preference = DWMWCP_DONOTROUND;

        // 先关闭 DWM 自动圆角，是为了让弹窗只使用既有 Region 圆角，避免 Windows 系统圆角与前端圆角叠出两层弧线。
        // DWM auto corners are disabled first so popups keep one Region radius instead of stacking Windows corners with frontend corners.
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &preference as *const i32 as *const _,
                size_of::<i32>() as u32,
            )
        };
    }

    fn apply_exact_window_region(hwnd: HWND, radius_logical_px: f64) {
        const RDW_INVALIDATE: u32 = 0x0001;
        const RDW_FRAME: u32 = 0x0400;
        const RDW_UPDATENOW: u32 = 0x0100;

        let mut rect = NativeRect { left: 0, top: 0, right: 0, bottom: 0 };
        if unsafe { GetClientRect(hwnd, &mut rect as *mut NativeRect) } == 0 {
            return;
        }

        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f64;
        let scale = dpi / 96.0;
        let radius = (radius_logical_px * scale).round().clamp(18.0, 96.0) as i32;
        let diameter = radius * 2;
        let region = unsafe {
            CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter)
        };

        if region.is_null() {
            return;
        }

        // Region 直接裁掉弹窗 WebView 宿主底板，是为了保留弹窗既有圆角，不让透明层失效时露出直角背景。
        // The Region clips the popup WebView host directly so existing popup corners remain clean even if transparency briefly fails.
        if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
            let _ = unsafe { DeleteObject(region as *mut c_void) };
            return;
        }

        // 强制刷新非客户区，是为了让窗口缩放或首次显示后立即丢弃旧的直角缓存。
        // Redrawing the non-client area immediately drops stale square-border caches after resize or first show.
        let _ = unsafe {
            RedrawWindow(
                hwnd,
                std::ptr::null(),
                std::ptr::null_mut(),
                RDW_INVALIDATE | RDW_FRAME | RDW_UPDATENOW,
            )
        };
    }
}

#[cfg(target_os = "windows")]
pub const CLIPANCHOR_WINDOW_RADIUS: f64 = 30.0;

#[cfg(target_os = "windows")]
pub fn apply_clipanchor_radius<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    native::apply(window, CLIPANCHOR_WINDOW_RADIUS);
}

#[cfg(not(target_os = "windows"))]
pub fn apply_clipanchor_radius<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}
