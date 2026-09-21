//! In-place web saves through the File System Access API.
//!
//! The legacy web save serialized the drawing and handed the bytes to a
//! browser download — every SAVE/SAVEAS produced a fresh "drawing (1).dwg"
//! in the Downloads folder, and Ctrl+S could never update the file the user
//! is working on. Where the browser exposes `showSaveFilePicker` (Chromium
//! desktop; Firefox and Safari do not implement it), the picker replaces
//! the download as the save dialog: it returns a `FileSystemFileHandle`
//! whose `createWritable()` writes the real file in place, and — once
//! picked — the handle keeps its read/write permission for the rest of the
//! session, so later plain saves write through it with no picker at all.
//!
//! Gesture timing drives the whole design: `showSaveFilePicker` may only be
//! called while the page holds transient user activation (the click on the
//! Save button), but the serialization that produces the bytes runs as a
//! thumbnail capture + worker round-trip that can far outlast that window.
//! The handle is therefore acquired in the save-confirm handler — the
//! gesture moment, before any serialize task starts — and the later write
//! goes through the retained handle, which needs no activation.
//!
//! `web-sys` 0.3.85 does ship `Window::show_save_file_picker_with_options`,
//! but every File System Access *picker* binding sits behind the
//! unstable-API cfg (`--cfg=web_sys_unstable_apis`), which this project's
//! wasm target does not enable (see `.cargo/config.toml` — turning it on
//! would change build flags for every crate in the graph). The picker is
//! therefore invoked dynamically: `Reflect::get(window,
//! "showSaveFilePicker")` + `Function::call` with a plain `js_sys::Object`
//! of options. Only the *handle* half goes through typed web-sys
//! (`FileSystemFileHandle`/`createWritable` are stable features the OPFS
//! code in `io::web_recent` already uses).
//!
//! Split like `io::web_session`: the routing decision and error
//! classification below are pure and compile (and test) on the native host;
//! everything touching the browser is wasm-only.

/// How a web save should reach the disk, decided at the save-confirm
/// gesture (see `save_route`).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveRoute {
    /// Plain Save on a tab whose retained handle matches the target type:
    /// write through the handle — no picker, no download.
    WriteInPlace,
    /// File System Access is available but no (matching) handle is held:
    /// open `showSaveFilePicker` now, inside the user-activation window.
    PickHandle,
    /// No File System Access (Firefox/Safari): the legacy download flow.
    Download,
}

/// The save-routing decision the web confirm path makes at gesture time.
///
/// `handle_ext` is the extension the tab's retained handle was picked for
/// (`None` when the tab holds no handle). Only a *plain* Save (Save/QSAVE
/// and the save-before-close flow) may reuse it: Save As must re-pick, and
/// a handle picked for `.dwg` must not receive DXF bytes (the file name
/// would lie about its contents).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub fn save_route(
    supported: bool,
    plain_save: bool,
    handle_ext: Option<&str>,
    target_ext: &str,
) -> SaveRoute {
    if !supported {
        return SaveRoute::Download;
    }
    match handle_ext {
        Some(handle_ext) if plain_save && exts_match(handle_ext, target_ext) => {
            SaveRoute::WriteInPlace
        }
        _ => SaveRoute::PickHandle,
    }
}

/// Case-insensitive extension comparison that tolerates a leading dot on
/// either side, so registry keys and format-table extensions ("dwg") never
/// mismatch on ".DWG".
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // via save_route's tests
fn exts_match(a: &str, b: &str) -> bool {
    let strip = |ext: &str| ext.trim_start_matches('.').to_ascii_lowercase();
    strip(a) == strip(b)
}

/// `showSaveFilePicker` rejects with a `DOMException` named "AbortError"
/// when the user dismisses the dialog. That name check is the whole cancel
/// classification — kept pure so it compiles and tests on every target.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub(crate) fn is_cancel_error_name(name: &str) -> bool {
    name == "AbortError"
}

/// The picker's file-type filter for a CAD extension: the MIME key Chrome's
/// own examples use, with the extension list doing the actual filtering.
#[cfg(target_arch = "wasm32")]
fn accept_mime(ext: &str) -> &'static str {
    if ext.eq_ignore_ascii_case("dxf") {
        "application/dxf"
    } else {
        "application/acad"
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    /// A retained save destination: the browser file handle behind one
    /// tab's in-place saves, plus the extension it was picked for.
    ///
    /// The handle's permission persists for the session once picked, so
    /// writing through it needs no user gesture — which is what lets the
    /// write happen long after the click that opened the picker.
    #[derive(Clone, Debug)]
    struct SaveHandle {
        /// Extension ("dwg"/"dxf") the picker filtered for. A later plain
        /// save may only reuse the handle while its target type matches.
        ext: String,
        name: String,
        handle: web_sys::FileSystemFileHandle,
    }

    thread_local! {
        /// One in-place destination per tab, keyed by tab id. The registry —
        /// not an App field or a Message payload — owns the handles because
        /// iced requires `Message: Send` even on wasm, and a JsValue-backed
        /// handle is not Send. (Same pattern as web_recent's OPEN_STORES.)
        /// Closing a tab, and a failed write, drop its entry; Save As
        /// replaces it.
        static SAVE_HANDLES: RefCell<HashMap<u64, SaveHandle>> =
            RefCell::new(HashMap::new());
    }

    /// The extension the tab's retained handle was picked for, if any —
    /// the `handle_ext` input of `save_route`.
    pub fn handle_ext(tab_id: u64) -> Option<String> {
        SAVE_HANDLES.with(|handles| {
            handles
                .borrow()
                .get(&tab_id)
                .map(|handle| handle.ext.clone())
        })
    }

    /// The retained destination's real name (the user may have renamed it
    /// inside the picker) — what save bookkeeping should use.
    pub fn handle_name(tab_id: u64) -> Option<String> {
        SAVE_HANDLES.with(|handles| {
            handles.borrow().get(&tab_id).map(|handle| handle.name.clone())
        })
    }

    /// Forget the tab's retained destination (tab closed, or its handle
    /// stopped working — the next save re-picks).
    pub fn drop_handle(tab_id: u64) {
        SAVE_HANDLES.with(|handles| {
            handles.borrow_mut().remove(&tab_id);
        });
    }

    /// Feature-detect `showSaveFilePicker` — never assume it: Firefox and
    /// Safari keep the download flow precisely because this returns false.
    pub fn supported() -> bool {
        web_sys::window().is_some_and(|window| {
            js_sys::Reflect::has(window.as_ref(), &JsValue::from_str("showSaveFilePicker"))
                .unwrap_or(false)
        })
    }

    /// Open `showSaveFilePicker` for `tab_id` and retain the chosen
    /// destination in the registry.
    ///
    /// MUST be called while the page holds transient user activation (the
    /// Save click); the thumbnail capture + worker serialization that
    /// follow can outlast that window, which is why the handle is picked
    /// up front and only written to later.
    ///
    /// * `Ok(Some(name))` — destination chosen and retained; `name` is its
    ///   real (possibly renamed) file name.
    /// * `Ok(None)` — user dismissed the picker: cancel the save (the
    ///   picker IS the save dialog on this path, same as desktop cancel).
    /// * `Err(message)` — the picker itself failed (blocked, no gesture,
    ///   …); the caller falls back to the legacy download flow.
    pub async fn pick_save_handle(
        tab_id: u64,
        suggested_name: &str,
        ext: &str,
    ) -> Result<Option<String>, String> {
        let window = web_sys::window().ok_or_else(|| "browser window unavailable".to_string())?;
        // Dynamic call — see the module docs for why the typed web-sys
        // binding (`show_save_file_picker_with_options`) is not used.
        let picker = js_sys::Reflect::get(window.as_ref(), &JsValue::from_str("showSaveFilePicker"))
            .ok()
            .and_then(|picker| picker.dyn_into::<js_sys::Function>().ok())
            .ok_or_else(|| "showSaveFilePicker unavailable".to_string())?;
        let options = picker_options(suggested_name, ext);
        let promise = picker
            .call1(window.as_ref(), &options)
            .map_err(crate::io::web_recent::js_error)?
            .dyn_into::<js_sys::Promise>()
            .map_err(crate::io::web_recent::js_error)?;
        match JsFuture::from(promise).await {
            Ok(value) => {
                let handle = value
                    .dyn_into::<web_sys::FileSystemFileHandle>()
                    .map_err(crate::io::web_recent::js_error)?;
                let name = handle.name();
                SAVE_HANDLES.with(|handles| {
                    handles.borrow_mut().insert(
                        tab_id,
                        SaveHandle {
                            ext: ext.trim_start_matches('.').to_ascii_lowercase(),
                            name: name.clone(),
                            handle,
                        },
                    );
                });
                Ok(Some(name))
            }
            // AbortError = the user dismissed the dialog; anything else is a
            // real failure the caller should report/fall back on.
            Err(error) => {
                let cancelled = js_sys::Reflect::get(&error, &JsValue::from_str("name"))
                    .ok()
                    .and_then(|name| name.as_string())
                    .is_some_and(|name| super::is_cancel_error_name(&name));
                if cancelled {
                    Ok(None)
                } else {
                    Err(crate::io::web_recent::js_error(error))
                }
            }
        }
    }

    /// Write `bytes` through the tab's retained handle: `createWritable` →
    /// write → close, the OPFS pattern from `io::web_recent::write_entry`.
    /// A failed write still aborts the stream so the exclusive lock is
    /// released — otherwise every later write to the same handle would fail
    /// the same way for the rest of the session.
    pub async fn write_handle(tab_id: u64, bytes: &[u8]) -> Result<(), String> {
        let handle = SAVE_HANDLES.with(|handles| {
            handles.borrow().get(&tab_id).map(|save| save.handle.clone())
        });
        let Some(handle) = handle else {
            return Err("no retained save destination for this drawing".to_string());
        };
        let writable = JsFuture::from(handle.create_writable())
            .await
            .map_err(crate::io::web_recent::js_error)?
            .dyn_into::<web_sys::FileSystemWritableFileStream>()
            .map_err(crate::io::web_recent::js_error)?;
        if let Err(error) = write_and_close(&writable, bytes).await {
            let _ = JsFuture::from(writable.abort()).await;
            return Err(error);
        }
        Ok(())
    }

    /// The createWritable write+close pair from `io::web_recent`'s OPFS
    /// writer — same failure contract (either promise rejecting is an
    /// error), kept local so the abort-on-failure dance above owns the
    /// stream lifecycle in one place.
    async fn write_and_close(
        writable: &web_sys::FileSystemWritableFileStream,
        bytes: &[u8],
    ) -> Result<(), String> {
        let write = writable
            .write_with_u8_array(bytes)
            .map_err(crate::io::web_recent::js_error)?;
        JsFuture::from(write)
            .await
            .map_err(crate::io::web_recent::js_error)?;
        JsFuture::from(writable.close())
            .await
            .map_err(crate::io::web_recent::js_error)?;
        Ok(())
    }

    /// Build the `SaveFilePickerOptions`-shaped object dynamically (the
    /// typed `web_sys::SaveFilePickerOptions` shares the picker's
    /// unstable-API cfg, see module docs): `suggestedName` plus a types
    /// filter restricted to the target extension.
    fn picker_options(suggested_name: &str, ext: &str) -> js_sys::Object {
        let set = |object: &js_sys::Object, key: &str, value: &JsValue| {
            let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), value);
        };
        let accept = js_sys::Object::new();
        let extensions = js_sys::Array::new();
        extensions.push(&JsValue::from_str(&format!(".{}", ext.trim_start_matches('.'))));
        set(&accept, super::accept_mime(ext), &extensions);
        let type_entry = js_sys::Object::new();
        set(
            &type_entry,
            "description",
            &JsValue::from_str(if ext.eq_ignore_ascii_case("dxf") {
                "DXF Drawing"
            } else {
                "DWG Drawing"
            }),
        );
        set(&type_entry, "accept", &accept);
        let types = js_sys::Array::new();
        types.push(&type_entry);
        let options = js_sys::Object::new();
        set(&options, "suggestedName", &JsValue::from_str(suggested_name));
        set(&options, "types", &types);
        options
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser::{
    drop_handle, handle_ext, handle_name, pick_save_handle, supported, write_handle,
};

#[cfg(test)]
mod tests {
    use super::{is_cancel_error_name, save_route, SaveRoute};

    /// Firefox/Safari: no showSaveFilePicker — every save stays a download,
    /// whatever handle state the tab happens to hold.
    #[test]
    fn unsupported_browsers_always_download() {
        assert_eq!(save_route(false, true, Some("dwg"), "dwg"), SaveRoute::Download);
        assert_eq!(save_route(false, false, None, "dxf"), SaveRoute::Download);
    }

    /// The whole point: plain Save on a tab with a matching handle writes
    /// in place — no picker, no download.
    #[test]
    fn plain_save_with_matching_handle_writes_in_place() {
        assert_eq!(save_route(true, true, Some("dwg"), "dwg"), SaveRoute::WriteInPlace);
        assert_eq!(save_route(true, true, Some("dxf"), "dxf"), SaveRoute::WriteInPlace);
    }

    /// Extension comparison is case-insensitive and dot-tolerant — the
    /// registry stores the normalized form, the format table's is bare.
    #[test]
    fn handle_ext_match_ignores_case_and_dot() {
        assert_eq!(save_route(true, true, Some(".DWG"), "dwg"), SaveRoute::WriteInPlace);
        assert_eq!(save_route(true, true, Some("dwg"), ".DWG"), SaveRoute::WriteInPlace);
    }

    /// A plain Save targeting the other format must re-pick: writing DXF
    /// bytes through a .dwg handle would corrupt the file's identity.
    #[test]
    fn mismatched_format_re_picks() {
        assert_eq!(save_route(true, true, Some("dwg"), "dxf"), SaveRoute::PickHandle);
        assert_eq!(save_route(true, true, Some("dxf"), "dwg"), SaveRoute::PickHandle);
    }

    /// Save As never reuses the handle — the user asked for a destination.
    #[test]
    fn save_as_with_existing_handle_re_picks() {
        assert_eq!(save_route(true, false, Some("dwg"), "dwg"), SaveRoute::PickHandle);
    }

    /// No handle yet (first save, or the previous write dropped it): pick.
    #[test]
    fn missing_handle_picks() {
        assert_eq!(save_route(true, true, None, "dwg"), SaveRoute::PickHandle);
        assert_eq!(save_route(true, false, None, "dwg"), SaveRoute::PickHandle);
    }

    /// Only the picker's AbortError means "user cancelled".
    #[test]
    fn only_abort_error_is_a_cancel() {
        assert!(is_cancel_error_name("AbortError"));
        assert!(!is_cancel_error_name("SecurityError"));
        assert!(!is_cancel_error_name("TypeError"));
        assert!(!is_cancel_error_name(""));
    }
}
