//! The selection as SVG on the system clipboard, under the format other
//! programs read SVG from.
//!
//! The GUI toolkit writes text only. Drawing programs paste SVG markup that
//! arrives as text as text, or not at all; they look for it under the SVG
//! MIME type: a registered clipboard format on Windows, a selection target
//! on X11 (which XWayland hands on to Wayland programs). On Windows the
//! outlines also go on as an enhanced metafile, the vector format most
//! Windows drawing programs paste (CorelDRAW reads no SVG from there).

use crate::io::svg_clipboard::Stroke;

/// The clipboard format, or selection target, SVG is read from.
pub const SVG_FORMAT: &str = "image/svg+xml";

/// The formats one copy puts on this platform's clipboard, with their
/// bytes: the markup as text, and as SVG. Empty where this module does not
/// write the clipboard.
pub fn formats(svg: &str) -> Vec<(&'static str, Vec<u8>)> {
    if cfg!(target_os = "windows") {
        let text: Vec<u8> = svg
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        vec![
            ("CF_UNICODETEXT", text),
            (SVG_FORMAT, svg.as_bytes().to_vec()),
        ]
    } else if cfg!(target_os = "linux") {
        vec![
            ("UTF8_STRING", svg.as_bytes().to_vec()),
            ("text/plain;charset=utf-8", svg.as_bytes().to_vec()),
            (SVG_FORMAT, svg.as_bytes().to_vec()),
        ]
    } else {
        Vec::new()
    }
}

/// Put `svg` on the system clipboard in all its `formats`, in one write,
/// and on Windows `strokes` as a metafile with them. `false` when this
/// platform has no such write, or it failed; the caller then puts the text
/// there through the GUI toolkit.
pub fn write(svg: &str, strokes: &[Stroke]) -> bool {
    let formats = formats(svg);
    if formats.is_empty() {
        return false;
    }
    #[cfg(test)]
    {
        let _ = strokes;
        test_record::record(formats);
        true
    }
    #[cfg(all(not(test), target_os = "windows"))]
    {
        windows::write(&formats, strokes)
    }
    #[cfg(all(not(test), target_os = "linux"))]
    {
        let _ = strokes;
        x11::write(formats)
    }
    #[cfg(all(not(test), not(any(target_os = "windows", target_os = "linux"))))]
    {
        let _ = (formats, strokes);
        false
    }
}

/// Tests leave the system clipboard alone: each write is kept here, per
/// thread, as the formats it would have put there.
#[cfg(test)]
pub mod test_record {
    use std::cell::RefCell;

    type Write = Vec<(&'static str, Vec<u8>)>;

    thread_local! {
        static WRITES: RefCell<Vec<Write>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn record(formats: Write) {
        WRITES.with(|writes| writes.borrow_mut().push(formats));
    }

    /// The writes made on this thread so far, each as (format, bytes).
    pub fn take() -> Vec<Write> {
        WRITES.with(|writes| std::mem::take(&mut *writes.borrow_mut()))
    }
}

#[cfg(all(not(test), target_os = "windows"))]
mod windows {
    use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        CloseEnhMetaFile, CreateEnhMetaFileW, CreatePen, DeleteEnhMetaFile, DeleteObject,
        Polyline, SelectObject, SetMapMode, HENHMETAFILE, MM_HIMETRIC, PS_SOLID,
    };
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::System::Ole::{CF_ENHMETAFILE, CF_UNICODETEXT};

    use crate::io::svg_clipboard::{strokes_extent, Stroke};

    /// `strokes` recorded as an enhanced metafile, one drawing unit to one
    /// millimetre (scaled down when that does not fit GDI's coordinates),
    /// its lower-left corner at the drawing's.
    fn metafile(strokes: &[Stroke]) -> Option<HENHMETAFILE> {
        let (min, max) = strokes_extent(strokes)?;
        let span = (max[0] - min[0]).max(max[1] - min[1]).max(1e-9);
        // Hundredths of a millimetre per drawing unit, kept inside i32.
        let scale = 100.0_f64.min(1.0e9 / span);
        let to_device = |[x, y]: [f64; 2]| POINT {
            x: ((x - min[0]) * scale).round() as i32,
            // MM_HIMETRIC: Y up, the top edge at 0.
            y: ((y - max[1]) * scale).round() as i32,
        };
        let frame = RECT {
            left: 0,
            top: 0,
            right: ((max[0] - min[0]) * scale).round().max(1.0) as i32,
            bottom: ((max[1] - min[1]) * scale).round().max(1.0) as i32,
        };
        // SAFETY: the recording DC is closed, and every pen made for it is
        // deselected and deleted, before returning.
        unsafe {
            let dc = CreateEnhMetaFileW(
                std::ptr::null_mut(),
                std::ptr::null(),
                &frame,
                std::ptr::null(),
            );
            if dc.is_null() {
                return None;
            }
            SetMapMode(dc, MM_HIMETRIC);
            for stroke in strokes {
                let [r, g, b] = stroke.rgb;
                let colour = u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16;
                let pen = CreatePen(PS_SOLID, 0, colour);
                let old = SelectObject(dc, pen);
                for run in stroke.runs.iter().filter(|run| run.len() > 1) {
                    let points: Vec<POINT> = run.iter().copied().map(to_device).collect();
                    Polyline(dc, points.as_ptr(), points.len() as i32);
                }
                SelectObject(dc, old);
                DeleteObject(pen);
            }
            let handle = CloseEnhMetaFile(dc);
            (!handle.is_null()).then_some(handle)
        }
    }

    /// Hand the metafile of `strokes` to the open clipboard; the clipboard
    /// owns it once it is set.
    fn set_metafile(strokes: &[Stroke]) {
        let Some(handle) = metafile(strokes) else {
            return;
        };
        // SAFETY: the clipboard is open and emptied by this process.
        unsafe {
            if SetClipboardData(u32::from(CF_ENHMETAFILE), handle).is_null() {
                DeleteEnhMetaFile(handle);
            }
        }
    }

    /// A movable global block holding `bytes`, as the clipboard takes data.
    fn global(bytes: &[u8]) -> Option<HGLOBAL> {
        // SAFETY: the block is as long as `bytes`; it is written while
        // locked and freed if it cannot be locked.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
            if handle.is_null() {
                return None;
            }
            let data = GlobalLock(handle) as *mut u8;
            if data.is_null() {
                GlobalFree(handle);
                return None;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
            GlobalUnlock(handle);
            Some(handle)
        }
    }

    /// Hand `bytes` to the open clipboard under `format`. The clipboard owns
    /// the block once it is set; otherwise it is freed here.
    fn set(format: u32, bytes: &[u8]) -> bool {
        let Some(handle) = global(bytes) else {
            return false;
        };
        // SAFETY: the clipboard is open and emptied by this process.
        unsafe {
            if SetClipboardData(format, handle).is_null() {
                GlobalFree(handle);
                return false;
            }
        }
        true
    }

    /// The clipboard's number for a format of [`super::formats`].
    fn format_id(name: &str) -> u32 {
        if name == "CF_UNICODETEXT" {
            return u32::from(CF_UNICODETEXT);
        }
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: `name` is NUL-terminated.
        unsafe { RegisterClipboardFormatW(name.as_ptr()) }
    }

    pub fn write(formats: &[(&str, Vec<u8>)], strokes: &[Stroke]) -> bool {
        // Another program may hold the clipboard for a moment.
        let mut open = false;
        for _ in 0..10 {
            // SAFETY: closed again below.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                open = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !open {
            return false;
        }
        // SAFETY: the clipboard is open; it is closed before returning.
        let written = unsafe { EmptyClipboard() != 0 }
            && formats
                .iter()
                .all(|(name, bytes)| set(format_id(name), bytes));
        if written {
            set_metafile(strokes);
        }
        // SAFETY: this function opened the clipboard.
        unsafe {
            CloseClipboard();
        }
        written
    }
}

#[cfg(all(not(test), target_os = "linux"))]
mod x11 {
    //! The CLIPBOARD selection of the X server, owned by a thread of this
    //! process while it holds the copy. Under Wayland, XWayland hands the
    //! selection on to Wayland programs with the same targets.

    use std::sync::{Arc, Mutex, OnceLock};

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
        SelectionRequestEvent, WindowClass, SELECTION_NOTIFY_EVENT,
    };
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::CURRENT_TIME;

    /// A target of the selection and its bytes.
    type Target = (u32, Arc<[u8]>);

    /// What the selection serves: the targets of the last copy, `None`
    /// once another program owns the clipboard.
    type Served = Arc<Mutex<Option<Vec<Target>>>>;

    struct Owner {
        conn: Arc<RustConnection>,
        window: u32,
        clipboard: u32,
        served: Served,
    }

    fn atom(conn: &RustConnection, name: &str) -> Option<u32> {
        Some(
            conn.intern_atom(false, name.as_bytes())
                .ok()?
                .reply()
                .ok()?
                .atom,
        )
    }

    /// The window that owns the selection and the thread that answers for
    /// it, made at the first copy. `None` with no X server to talk to.
    fn owner() -> Option<&'static Owner> {
        static OWNER: OnceLock<Option<Owner>> = OnceLock::new();
        OWNER
            .get_or_init(|| {
                let (conn, screen) = x11rb::connect(None).ok()?;
                let conn = Arc::new(conn);
                let root = conn.setup().roots.get(screen)?.root;
                let window = conn.generate_id().ok()?;
                conn.create_window(
                    0,
                    window,
                    root,
                    0,
                    0,
                    1,
                    1,
                    0,
                    WindowClass::INPUT_ONLY,
                    0,
                    &CreateWindowAux::new(),
                )
                .ok()?;
                let clipboard = atom(&conn, "CLIPBOARD")?;
                let targets = atom(&conn, "TARGETS")?;
                conn.flush().ok()?;
                let served: Served = Arc::new(Mutex::new(None));
                let (thread_conn, thread_served) = (conn.clone(), served.clone());
                std::thread::Builder::new()
                    .name("svg-clipboard".into())
                    .spawn(move || serve(&thread_conn, window, clipboard, targets, &thread_served))
                    .ok()?;
                Some(Owner {
                    conn,
                    window,
                    clipboard,
                    served,
                })
            })
            .as_ref()
    }

    /// Answer requests for the selection until the connection closes.
    fn serve(conn: &RustConnection, window: u32, clipboard: u32, targets: u32, served: &Served) {
        while let Ok(event) = conn.wait_for_event() {
            match event {
                // Another program took the clipboard. A copy made since took
                // it back, so ask who owns it now.
                Event::SelectionClear(event) if event.selection == clipboard => {
                    let ours = conn
                        .get_selection_owner(clipboard)
                        .ok()
                        .and_then(|cookie| cookie.reply().ok())
                        .is_some_and(|reply| reply.owner == window);
                    if !ours {
                        if let Ok(mut served) = served.lock() {
                            *served = None;
                        }
                    }
                }
                Event::SelectionRequest(event) => {
                    let formats = served.lock().ok().and_then(|s| s.clone());
                    let _ = answer(conn, clipboard, targets, &event, formats.as_deref());
                }
                _ => {}
            }
        }
    }

    /// Put the requested target on the requestor's property, or the list of
    /// targets for TARGETS, and tell it whether that worked. It is told in
    /// either case, so it never waits for an answer that does not come.
    fn answer(
        conn: &RustConnection,
        clipboard: u32,
        targets: u32,
        event: &SelectionRequestEvent,
        formats: Option<&[Target]>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Clients of the old protocol name no property: the target is it.
        let property = if event.property == u32::from(AtomEnum::NONE) {
            event.target
        } else {
            event.property
        };
        let put = || -> Result<bool, Box<dyn std::error::Error>> {
            let Some(formats) = formats.filter(|_| event.selection == clipboard) else {
                return Ok(false);
            };
            if event.target == targets {
                let mut list: Vec<u32> = formats.iter().map(|(target, _)| *target).collect();
                list.push(targets);
                conn.change_property32(
                    PropMode::REPLACE,
                    event.requestor,
                    property,
                    AtomEnum::ATOM,
                    &list,
                )?
                .check()?;
                return Ok(true);
            }
            let Some((target, bytes)) = formats.iter().find(|(target, _)| *target == event.target)
            else {
                return Ok(false);
            };
            conn.change_property8(PropMode::REPLACE, event.requestor, property, *target, bytes)?
                .check()?;
            Ok(true)
        };
        let property = if put().unwrap_or(false) {
            property
        } else {
            AtomEnum::NONE.into()
        };
        conn.send_event(
            false,
            event.requestor,
            EventMask::NO_EVENT,
            SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: event.sequence,
                time: event.time,
                requestor: event.requestor,
                selection: event.selection,
                target: event.target,
                property,
            },
        )?;
        conn.flush()?;
        Ok(())
    }

    pub fn write(formats: Vec<(&'static str, Vec<u8>)>) -> bool {
        let Some(owner) = owner() else {
            return false;
        };
        let Some(formats) = formats
            .into_iter()
            .map(|(name, bytes)| Some((atom(&owner.conn, name)?, Arc::from(bytes))))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let Ok(mut served) = owner.served.lock() else {
            return false;
        };
        *served = Some(formats);
        drop(served);
        let owned = owner
            .conn
            .set_selection_owner(owner.window, owner.clipboard, CURRENT_TIME)
            .ok()
            .and_then(|_| owner.conn.get_selection_owner(owner.clipboard).ok())
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.owner == owner.window);
        if !owned {
            if let Ok(mut served) = owner.served.lock() {
                *served = None;
            }
        }
        owned
    }
}
