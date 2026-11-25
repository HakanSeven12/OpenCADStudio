//! Windows: the maximize button the app draws answers the system as a real
//! one, so hovering it opens the Snap Layouts flyout of Windows 11.
//!
//! The main window's procedure is subclassed: over the button the hit test
//! reports `HTMAXBUTTON`. The system then sends the button's presses as
//! non-client messages, so the click is handled here too, and the hover is
//! kept in [`maximize_hovered`] for the button's style. The same hit test
//! names the window's edges, so they resize it with the system's cursors
//! and snapping, as a framed window's do.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, IsZoomed, PostMessageW, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT,
    HTMAXBUTTON, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, SC_MAXIMIZE, SC_RESTORE, WM_NCHITTEST,
    WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_SYSCOMMAND,
};

use crate::ui::style::ribbon_metrics as metrics;

/// The pointer is over the maximize button (the system's hit test says so).
static HOVERED: AtomicBool = AtomicBool::new(false);
/// The app's UI scale (`f32` bits), on top of the monitor's DPI.
static UI_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

const SUBCLASS_ID: usize = 0x0C5_717;

/// The UI scale the app draws at (its scale factor, without the DPI).
pub fn set_ui_scale(scale: f32) {
    UI_SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

/// Whether the maximize button shows as hovered: the system, not iced,
/// tracks the pointer over it.
pub fn maximize_hovered() -> bool {
    HOVERED.load(Ordering::Relaxed)
}

/// Subclass the main window `hwnd` (once).
pub fn install(hwnd: isize) {
    // SAFETY: `hwnd` is the live main window, handed over by winit on the
    // UI thread that owns it; the procedure is a plain function.
    unsafe {
        SetWindowSubclass(hwnd as HWND, Some(procedure), SUBCLASS_ID, 0);
    }
}

/// The client area and the scale from logical to client pixels: the
/// monitor's DPI times the app's scale.
fn client_and_scale(hwnd: HWND) -> Option<(RECT, f32)> {
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: plain queries on a valid window handle.
    let dpi = unsafe {
        if GetClientRect(hwnd, &mut client) == 0 {
            return None;
        }
        GetDpiForWindow(hwnd)
    };
    Some((
        client,
        dpi as f32 / 96.0 * f32::from_bits(UI_SCALE.load(Ordering::Relaxed)),
    ))
}

/// The maximize button in client pixels: second from the right, the bar's
/// height.
fn maximize_rect(hwnd: HWND) -> Option<RECT> {
    let (client, scale) = client_and_scale(hwnd)?;
    let w = (metrics::CAPTION_W * scale).round() as i32;
    let h = (metrics::TITLE_BAR_H * scale).round() as i32;
    Some(RECT {
        left: client.right - 2 * w,
        top: 0,
        right: client.right - w,
        bottom: h,
    })
}

/// The hit-test point in client pixels.
fn client_point(hwnd: HWND, lparam: LPARAM) -> Option<POINT> {
    let mut point = POINT {
        x: (lparam & 0xFFFF) as i16 as i32,
        y: ((lparam >> 16) & 0xFFFF) as i16 as i32,
    };
    // SAFETY: converts a point for a valid window handle.
    (unsafe { ScreenToClient(hwnd, &mut point) } != 0).then_some(point)
}

fn over_maximize(hwnd: HWND, point: POINT) -> bool {
    maximize_rect(hwnd).is_some_and(|r| {
        point.x >= r.left && point.x < r.right && point.y >= r.top && point.y < r.bottom
    })
}

/// The edge or corner of a restored window under `point`, as the system's
/// hit-test code; `None` inside.
fn edge(hwnd: HWND, point: POINT) -> Option<u32> {
    // SAFETY: plain query on a valid window handle.
    if unsafe { IsZoomed(hwnd) } != 0 {
        return None;
    }
    let (client, scale) = client_and_scale(hwnd)?;
    let e = (metrics::RESIZE_EDGE * scale).round().max(1.0) as i32;
    let left = point.x < client.left + e;
    let right = point.x >= client.right - e;
    let top = point.y < client.top + e;
    let bottom = point.y >= client.bottom - e;
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, ..) => HTLEFT,
        (_, true, ..) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => return None,
    })
}

/// Light or dim the button: redraw, so its style reads the new state.
fn set_hovered(hwnd: HWND, hovered: bool) {
    if HOVERED.swap(hovered, Ordering::Relaxed) == hovered {
        return;
    }
    // SAFETY: valid window handle; the leave tracking ends with the window.
    unsafe {
        if hovered {
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE | TME_NONCLIENT,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut track);
        }
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let on_button = wparam == HTMAXBUTTON as WPARAM;
    match msg {
        WM_NCHITTEST => {
            if let Some(point) = client_point(hwnd, lparam) {
                // The edges first: the top edge runs over the buttons.
                if let Some(code) = edge(hwnd, point) {
                    set_hovered(hwnd, false);
                    return code as LRESULT;
                }
                let over = over_maximize(hwnd, point);
                set_hovered(hwnd, over);
                if over {
                    return HTMAXBUTTON as LRESULT;
                }
            }
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if on_button => return 0,
        WM_NCLBUTTONUP if on_button => {
            // SAFETY: valid window handle; the command is the system's own.
            let command = if unsafe { IsZoomed(hwnd) } != 0 {
                SC_RESTORE
            } else {
                SC_MAXIMIZE
            };
            unsafe { PostMessageW(hwnd, WM_SYSCOMMAND, command as WPARAM, 0) };
            return 0;
        }
        WM_NCMOUSELEAVE => set_hovered(hwnd, false),
        _ => {}
    }
    // SAFETY: hands every other message to the next procedure (winit's).
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
