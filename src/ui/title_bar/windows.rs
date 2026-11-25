//! Windows: the maximize button the app draws answers the system as a real
//! one, so hovering it opens the Snap Layouts flyout of Windows 11.
//!
//! The main window's procedure is subclassed: over the button the hit test
//! reports `HTMAXBUTTON`. The system then sends the button's presses as
//! non-client messages, so the click is handled here too, and the hover is
//! kept in [`maximize_hovered`] for the button's style. The same hit test
//! names the window's edges, so they resize it with the system's cursors
//! and snapping, as a framed window's do.
//!
//! Windows 11 draws a 1 px border round the window; [`set_border_color`]
//! gives it the app's dark hairline colour.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, IsZoomed, PostMessageW, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT,
    HTMAXBUTTON, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, SC_MAXIMIZE, SC_RESTORE, WM_NCDESTROY,
    WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE,
    WM_SYSCOMMAND,
};

use super::geometry::{self, Edge, Rect};

/// The pointer is over the maximize button (the system's hit test says so).
static HOVERED: AtomicBool = AtomicBool::new(false);
/// Nothing of the app's (a dialog, a menu, a floating pallet) lies over the
/// maximize button, which may then answer as the system's.
static MAXIMIZE_FREE: AtomicBool = AtomicBool::new(true);
/// The window is subclassed: the system finds its edges.
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// The subclassed window, for its border colour.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// The border colour last given to the system (`COLORREF`); none yet.
static BORDER: AtomicU32 = AtomicU32::new(u32::MAX);
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

/// Whether the maximize button may answer as the system's: nothing of the
/// app's lies over it (the view says so each time it is built).
pub fn set_maximize_free(free: bool) {
    MAXIMIZE_FREE.store(free, Ordering::Relaxed);
}

/// Whether the window is subclassed, so the system finds its edges; else
/// the app draws edges of its own.
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

/// Subclass the main window `hwnd` (once); whether that worked.
pub fn install(hwnd: isize) -> bool {
    // SAFETY: `hwnd` is the live main window, handed over by winit on the
    // UI thread that owns it; the procedure is a plain function.
    let ok = unsafe { SetWindowSubclass(hwnd as HWND, Some(procedure), SUBCLASS_ID, 0) } != 0;
    INSTALLED.store(ok, Ordering::Relaxed);
    WINDOW.store(hwnd, Ordering::Relaxed);
    BORDER.store(u32::MAX, Ordering::Relaxed);
    ok
}

/// Give the window's border (Windows 11 draws it, 1 px) this colour, as
/// `r`, `g`, `b` in 0..=255; only when it changed. Windows 10 has no such
/// border and ignores it.
pub fn set_border_color(r: u8, g: u8, b: u8) {
    let colorref = u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16;
    let hwnd = WINDOW.load(Ordering::Relaxed);
    if hwnd == 0 || BORDER.swap(colorref, Ordering::Relaxed) == colorref {
        return;
    }
    /// `DWMWA_BORDER_COLOR`, Windows 11 and later.
    const DWMWA_BORDER_COLOR: u32 = 34;
    type SetAttribute = unsafe extern "system" fn(HWND, u32, *const std::ffi::c_void, u32) -> i32;
    // Looked up at run time: dwmapi is there on every supported Windows,
    // the attribute only on Windows 11, which reports anything else as an
    // error that changes nothing.
    let name: Vec<u16> = "dwmapi.dll\0".encode_utf16().collect();
    // SAFETY: a system library and one of its exports, with its documented
    // signature; the colour outlives the call.
    unsafe {
        let module = LoadLibraryW(name.as_ptr());
        if module.is_null() {
            return;
        }
        if let Some(proc) = GetProcAddress(module, b"DwmSetWindowAttribute\0".as_ptr()) {
            let set: SetAttribute = std::mem::transmute(proc);
            set(
                hwnd as HWND,
                DWMWA_BORDER_COLOR,
                (&colorref as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
}

/// The client area and the scale from logical to client pixels: the
/// monitor's DPI times the app's scale.
fn client_and_scale(hwnd: HWND) -> Option<(Rect, f32)> {
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
    let client = Rect {
        left: client.left,
        top: client.top,
        right: client.right,
        bottom: client.bottom,
    };
    Some((
        client,
        dpi as f32 / 96.0 * f32::from_bits(UI_SCALE.load(Ordering::Relaxed)),
    ))
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

/// The system's hit-test code for a point: an edge of a restored window,
/// the maximize button while nothing of the app's lies over it, else
/// `None` (the client area, which winit and iced handle).
fn hit(hwnd: HWND, point: POINT) -> Option<u32> {
    let (client, scale) = client_and_scale(hwnd)?;
    // SAFETY: plain query on a valid window handle.
    let restored = unsafe { IsZoomed(hwnd) } == 0;
    // The edges first: the top edge runs over the buttons.
    if restored {
        if let Some(edge) = geometry::edge(client, scale, point.x, point.y) {
            return Some(match edge {
                Edge::TopLeft => HTTOPLEFT,
                Edge::TopRight => HTTOPRIGHT,
                Edge::BottomLeft => HTBOTTOMLEFT,
                Edge::BottomRight => HTBOTTOMRIGHT,
                Edge::Left => HTLEFT,
                Edge::Right => HTRIGHT,
                Edge::Top => HTTOP,
                Edge::Bottom => HTBOTTOM,
            });
        }
    }
    (MAXIMIZE_FREE.load(Ordering::Relaxed)
        && geometry::maximize_button(client, scale).contains(point.x, point.y))
    .then_some(HTMAXBUTTON)
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
            if let Some(code) = client_point(hwnd, lparam).and_then(|point| hit(hwnd, point)) {
                set_hovered(hwnd, code == HTMAXBUTTON);
                return code as LRESULT;
            }
            set_hovered(hwnd, false);
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
        // As Microsoft asks: the subclass goes before the window does.
        WM_NCDESTROY => {
            INSTALLED.store(false, Ordering::Relaxed);
            WINDOW.store(0, Ordering::Relaxed);
            // SAFETY: removes this procedure from the window being destroyed.
            unsafe { RemoveWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID) };
        }
        _ => {}
    }
    // SAFETY: hands every other message to the next procedure (winit's).
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
