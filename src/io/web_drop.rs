//! Web drag & drop file opening (#344).
//!
//! winit's web backend never emits `DroppedFile`, so the page itself listens
//! for DOM `dragover`/`drop` events, reads each dropped drawing's bytes, and
//! queues them for the 50 ms control poll to open through the web-picker flow.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// A stray multi-drop must not grow the queue without bound; extras are
/// rejected with a console warning instead of being silently dropped.
const QUEUE_CAP: usize = 8;

static DROPPED: Mutex<VecDeque<(String, Vec<u8>)>> = Mutex::new(VecDeque::new());
static INSTALLED: OnceLock<()> = OnceLock::new();

fn warn(line: &str) {
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(line));
}

fn enqueue(name: String, bytes: Vec<u8>) {
    let mut queue = DROPPED.lock().unwrap();
    if queue.len() >= QUEUE_CAP {
        drop(queue);
        warn(&format!(
            "ocs: dropped-file queue full; ignoring \"{name}\""
        ));
        return;
    }
    queue.push_back((name, bytes));
}

/// JS-callable entry (same shape as `ocs_control_submit`) for hosts that
/// forward drops themselves.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn ocs_web_drop_file(name: String, bytes: js_sys::Uint8Array) {
    enqueue(name, bytes.to_vec());
}

/// Pop the oldest dropped file that the poll has not started opening yet.
pub fn take_dropped() -> Option<(String, Vec<u8>)> {
    DROPPED.lock().unwrap().pop_front()
}

/// Feed already-read bytes through the same loader tail the web picker uses
/// (including the OPFS recent-copy payload), so `WebFileOpened` cannot tell
/// the two doors apart.
pub async fn open_dropped(
    name: String,
    bytes: Vec<u8>,
    progress: std::sync::Arc<crate::io::OpenProgressState>,
) -> crate::io::WebOpenOutcome {
    progress.set(crate::app::OPEN_PHASE_READING, 500, 1, 2);
    let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(bytes);
    let size_bytes = bytes.len() as u64;
    let result = super::load_web_bytes(&name, &bytes, progress, false, "", None).await;
    let keep_for_recovery = result
        .as_ref()
        .err()
        .is_some_and(|error| error.recovery_available);
    let cache_bytes = result.is_ok().then(|| std::sync::Arc::clone(&bytes));
    crate::io::WebOpenOutcome {
        name,
        size_bytes,
        result,
        recovery_bytes: keep_for_recovery.then(|| std::sync::Arc::clone(&bytes)),
        cache_bytes,
        record_recent: false,
    }
}

/// Register the document-level `dragover`/`drop` listeners once. `dragover`
/// MUST preventDefault or the browser navigates to the dropped file instead
/// of delivering it to the page.
pub fn install() {
    if INSTALLED.set(()).is_err() {
        return;
    }
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let on_dragover = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
        move |event: web_sys::Event| {
            event.prevent_default();
        },
    );
    let _ =
        document.add_event_listener_with_callback("dragover", on_dragover.as_ref().unchecked_ref());
    on_dragover.forget();

    let on_drop = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
        move |event: web_sys::Event| {
            event.prevent_default();
            event.stop_propagation();
            let Some(drag) = event.dyn_ref::<web_sys::DragEvent>() else {
                return;
            };
            let Some(files) = drag.data_transfer().and_then(|transfer| transfer.files()) else {
                return;
            };
            for index in 0..files.length() {
                let Some(file) = files.get(index) else {
                    continue;
                };
                let name = file.name();
                // A drop has no picker filter: accept the formats the web Open
                // dialog accepts; anything else is logged, never parsed.
                if !is_drawing_name(&name) {
                    warn(&format!(
                        "ocs: ignored dropped file \"{name}\" (not .dwg/.dxf)"
                    ));
                    continue;
                }
                wasm_bindgen_futures::spawn_local(async move {
                    match read_file(&file).await {
                        Ok(bytes) => enqueue(name, bytes),
                        Err(error) => web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(
                            &format!("ocs: failed to read dropped file \"{name}\": {error}"),
                        )),
                    }
                });
            }
        },
    );
    let _ = document.add_event_listener_with_callback("drop", on_drop.as_ref().unchecked_ref());
    on_drop.forget();
}

fn is_drawing_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(extension.to_ascii_lowercase().as_str(), "dwg" | "dxf")
    })
}

/// Same Blob → bytes read as the OPFS recent copies (`web_recent`).
async fn read_file(file: &web_sys::File) -> Result<Vec<u8>, String> {
    let buffer = JsFuture::from(file.array_buffer())
        .await
        .map_err(|error| error.as_string().unwrap_or_else(|| format!("{error:?}")))?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
