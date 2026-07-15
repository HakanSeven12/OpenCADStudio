//! Windows: give the taskbar button its icon.
//!
//! iced's cross-platform [`window::Settings::icon`] only sets winit's
//! `window_icon`, which on Windows becomes the window's **`ICON_SMALL`** — the
//! little glyph in the title bar and the Alt-Tab list. The **taskbar button**
//! renders the window's **`ICON_BIG`**, and iced/winit never set it, so the
//! taskbar shows a blank/generic icon the whole time the app runs even though
//! the title bar looks correct. (Verified live: `WM_GETICON`/`ICON_BIG`
//! returns null on a freshly opened window.)
//!
//! There is no cross-platform iced API to set `ICON_BIG`, so we do it directly:
//! build an `HICON` from the brand logo and `WM_SETICON(ICON_BIG)` it onto our
//! own top-level window once it exists. Because iced opens the window
//! asynchronously after `app::run()` takes over the thread, a short-lived
//! background thread polls for the window and applies the icon, then exits.
//!
//! The class icon is set too (`GCLP_HICON`/`GCLP_HICONSM`) so any window opened
//! later on the same winit class inherits it without a second pass.

#![cfg(windows)]

use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BI_RGB,
    DIB_RGB_COLORS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId,
    IsWindowVisible, SendMessageW, SetClassLongPtrW, GCLP_HICON, GCLP_HICONSM, ICONINFO, ICON_BIG,
    ICON_SMALL, WM_SETICON,
};

/// Spawn the background pass that stamps `ICON_BIG` onto the app's taskbar
/// button. Safe no-op if the icon can't be built (the taskbar just keeps the
/// blank icon it has today — never worse). Call once, before `app::run()`.
pub fn install_taskbar_icon() {
    std::thread::spawn(|| {
        let Some(hicon) = load_app_hicon() else {
            return;
        };
        // `hicon` is intentionally leaked: it stays referenced by our window(s)
        // for the whole session, and the process owns exactly one of them.
        let pid = std::process::id();
        // Poll for up to ~9s (iced opens the window a beat after we hand it the
        // thread). Stop as soon as we've stamped a real, titled window.
        for _ in 0..60 {
            let mut ctx = Ctx {
                hicon,
                pid,
                stamped: false,
            };
            unsafe {
                EnumWindows(Some(enum_cb), &mut ctx as *mut Ctx as LPARAM);
            }
            if ctx.stamped {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    });
}

struct Ctx {
    hicon: isize,
    pid: u32,
    stamped: bool,
}

/// `EnumWindows` visitor: stamp the icon on each visible, titled top-level
/// window owned by this process. The title check skips winit's hidden
/// message-only helper window so we only target the real app window.
unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> i32 {
    let ctx = unsafe { &mut *(lparam as *mut Ctx) };
    let mut wnd_pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut wnd_pid) };
    let ours = wnd_pid == ctx.pid;
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    let titled = unsafe { GetWindowTextLengthW(hwnd) } > 0;
    if ours && visible && titled {
        unsafe {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, ctx.hicon as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as WPARAM, ctx.hicon as LPARAM);
            // Cover any future window of the same class in one shot.
            SetClassLongPtrW(hwnd, GCLP_HICON, ctx.hicon);
            SetClassLongPtrW(hwnd, GCLP_HICONSM, ctx.hicon);
        }
        ctx.stamped = true;
    }
    1 // TRUE — keep enumerating (there may be several windows)
}

/// Build the icon to hang on the taskbar by rasterizing the brand logo from
/// RGBA — the same source iced uses for the (working) title-bar icon, so the
/// two always match, on both dev and release builds. Returns the `HICON` as an
/// `isize` handle, or `None` if the GDI objects can't be created.
fn load_app_hicon() -> Option<isize> {
    hicon_from_rgba(&crate::app::helpers::build_window_icon(), 32, 32)
}

/// Turn an RGBA buffer (`w*h*4`, row-major, straight alpha) into an `HICON`.
/// Uses a top-down 32bpp DIB section plus an all-zero AND mask, so the color
/// bitmap is shown verbatim — no scanline-order or channel-order ambiguity.
fn hicon_from_rgba(rgba: &[u8], w: i32, h: i32) -> Option<isize> {
    let (uw, uh) = (w as usize, h as usize);
    if rgba.len() < uw * uh * 4 {
        return None;
    }
    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize =
            std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h; // negative → top-down, matching `rgba`
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB as u32;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbm_color = CreateDIBSection(
            hdc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        ReleaseDC(std::ptr::null_mut(), hdc);
        if hbm_color.is_null() || bits.is_null() {
            if !hbm_color.is_null() {
                DeleteObject(hbm_color as _);
            }
            return None;
        }

        // Copy RGBA → BGRA (the byte order a Windows 32bpp DIB expects).
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, uw * uh * 4);
        for i in 0..uw * uh {
            dst[i * 4] = rgba[i * 4 + 2]; // B
            dst[i * 4 + 1] = rgba[i * 4 + 1]; // G
            dst[i * 4 + 2] = rgba[i * 4]; // R
            dst[i * 4 + 3] = rgba[i * 4 + 3]; // A
        }

        // 1bpp AND mask, explicitly zeroed → fully opaque icon. 1bpp scanlines
        // are WORD-aligned; a zero buffer of the right size keeps the mask blank.
        let mask_stride = (((uw) + 15) / 16) * 2;
        let mask_bits = vec![0u8; mask_stride * uh];
        let hbm_mask = CreateBitmap(w, h, 1, 1, mask_bits.as_ptr() as *const _);

        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };
        let hicon = CreateIconIndirect(&icon_info);

        DeleteObject(hbm_color as _);
        if !hbm_mask.is_null() {
            DeleteObject(hbm_mask as _);
        }
        (!hicon.is_null()).then_some(hicon as isize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon;

    /// The RGBA→HICON path (the dev/fallback build's icon source) produces a
    /// real icon handle. Guards the DIB layout / mask sizing from regressing.
    #[test]
    fn rgba_builds_a_valid_hicon() {
        let rgba = crate::app::helpers::build_window_icon();
        let handle = hicon_from_rgba(&rgba, 32, 32).expect("HICON from brand RGBA");
        assert_ne!(handle, 0);
        unsafe {
            DestroyIcon(handle as *mut _);
        }
    }

    /// A buffer smaller than `w*h*4` is rejected rather than reading OOB.
    #[test]
    fn rejects_undersized_buffer() {
        assert!(hicon_from_rgba(&[0u8; 16], 32, 32).is_none());
    }
}
