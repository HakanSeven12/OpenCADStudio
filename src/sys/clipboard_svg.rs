//! The selection as SVG on the system clipboard, under the format other
//! programs read SVG from.
//!
//! The GUI toolkit writes text only. Drawing programs paste SVG markup that
//! arrives as text as text, or not at all; they look for it under the SVG
//! MIME type, which Windows has as a registered clipboard format.

/// The registered clipboard format SVG is read from.
pub const SVG_FORMAT: &str = "image/svg+xml";

/// Put `svg` on the system clipboard as SVG and as text, in one write.
/// `false` when this platform has no such write here; the caller then puts
/// the text there through the GUI toolkit.
pub fn write(svg: &str) -> bool {
    #[cfg(test)]
    {
        test_record::record(svg);
        cfg!(target_os = "windows")
    }
    #[cfg(all(not(test), target_os = "windows"))]
    {
        windows::write(svg)
    }
    #[cfg(all(not(test), not(target_os = "windows")))]
    {
        let _ = svg;
        false
    }
}

/// Tests leave the system clipboard alone: on Windows each write is kept
/// here, per thread, as the formats it would have put there.
#[cfg(test)]
pub mod test_record {
    use std::cell::RefCell;

    thread_local! {
        static WRITES: RefCell<Vec<Vec<(String, Vec<u8>)>>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn record(svg: &str) {
        if !cfg!(target_os = "windows") {
            return;
        }
        let text: Vec<u8> = svg
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        WRITES.with(|writes| {
            writes.borrow_mut().push(vec![
                ("CF_UNICODETEXT".to_owned(), text),
                (super::SVG_FORMAT.to_owned(), svg.as_bytes().to_vec()),
            ])
        });
    }

    /// The writes made on this thread so far, each as (format, bytes).
    pub fn take() -> Vec<Vec<(String, Vec<u8>)>> {
        WRITES.with(|writes| std::mem::take(&mut *writes.borrow_mut()))
    }
}

#[cfg(all(not(test), target_os = "windows"))]
mod windows {
    use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

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

    pub fn write(svg: &str) -> bool {
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
        let text: Vec<u8> = svg
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        let name: Vec<u16> = super::SVG_FORMAT.encode_utf16().chain([0]).collect();
        // SAFETY: the clipboard is open; it is closed before returning.
        let written = unsafe {
            EmptyClipboard() != 0
                && set(u32::from(CF_UNICODETEXT), &text)
                && set(RegisterClipboardFormatW(name.as_ptr()), svg.as_bytes())
        };
        // SAFETY: this function opened the clipboard.
        unsafe {
            CloseClipboard();
        }
        written
    }
}
