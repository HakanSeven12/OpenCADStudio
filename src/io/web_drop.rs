//! Web drag & drop file opening (#344).
//!
//! winit's web backend never emits `DroppedFile`, so the page itself listens
//! for DOM `dragover`/`drop` events, reads each dropped drawing's bytes, and
//! queues them for the 50 ms control poll to open through the web-picker flow.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// A stray multi-drop must not grow the queue without bound; extras are
/// rejected with a console warning instead of being silently dropped.
const QUEUE_CAP: usize = 8;

static DROPPED: Mutex<VecDeque<(String, Vec<u8>)>> = Mutex::new(VecDeque::new());
// `OnceLock::take` needs `&mut self`, which an immutable static cannot
// provide — an AtomicBool swap gives the same set-once marker and can still
// be reverted when no document exists.
static INSTALLED: AtomicBool = AtomicBool::new(false);

fn warn(line: &str) {
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(line));
}

fn lock_queue() -> std::sync::MutexGuard<'static, VecDeque<(String, Vec<u8>)>> {
    // The critical sections are push/pop only, so poisoning is unreachable;
    // recover anyway rather than panic across the JS boundary.
    DROPPED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn enqueue(name: String, bytes: Vec<u8>) {
    let mut queue = lock_queue();
    if queue.len() >= QUEUE_CAP {
        drop(queue);
        warn(&format!(
            "ocs: dropped-file queue full; ignoring \"{name}\""
        ));
        return;
    }
    queue.push_back((name, bytes));
}

/// Queue already-read drawing bytes (drag-and-drop, autosave restore) for the
/// 50 ms poll to open through the guarded web-picker flow.
pub fn enqueue_open(name: String, bytes: Vec<u8>) {
    enqueue(name, bytes);
}

/// JS-callable entry (same shape as `ocs_control_submit`) for hosts that
/// forward drops themselves.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn ocs_web_drop_file(name: String, bytes: js_sys::Uint8Array) {
    enqueue(name, bytes.to_vec());
}

/// Pop the oldest dropped file that the poll has not started opening yet.
pub fn take_dropped() -> Option<(String, Vec<u8>)> {
    lock_queue().pop_front()
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

/// Register the document-level `dragenter`/`dragover`/`drop` listeners once.
/// `dragover` MUST preventDefault or the browser navigates to the dropped file
/// instead of delivering it to the page.
pub fn install() {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        // Only mark installed once the listeners really are: an early
        // no-document context must not permanently disable drops.
        INSTALLED.store(false, Ordering::SeqCst);
        return;
    };
    let cancel = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
        move |event: web_sys::Event| {
            event.prevent_default();
        },
    );
    // Canceling `dragenter` too is belt-and-braces some engines still want
    // before treating the page as a drop target.
    let _ = document.add_event_listener_with_callback("dragenter", cancel.as_ref().unchecked_ref());
    let _ = document.add_event_listener_with_callback("dragover", cancel.as_ref().unchecked_ref());
    cancel.forget();

    let on_drop = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
        move |event: web_sys::Event| {
            event.prevent_default();
            let Some(drag) = event.dyn_ref::<web_sys::DragEvent>() else {
                return;
            };
            let Some(files) = drag.data_transfer().and_then(|transfer| transfer.files()) else {
                return;
            };
            // Start every read here, inside the handler, and finish them in
            // one task in drop order: legacy WebKit dislikes drag-originated
            // reads that begin after the event returns, and parallel reads
            // would enqueue (and open) in completion order, not drop order.
            let mut reads: Vec<(String, js_sys::Promise)> = Vec::new();
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
                reads.push((name, file.array_buffer()));
            }
            if reads.is_empty() {
                return;
            }
            // The page is the only intended drop target; hide drawing drops
            // from host-page handlers only once there is something to open.
            event.stop_propagation();
            wasm_bindgen_futures::spawn_local(async move {
                for (name, read) in reads {
                    match JsFuture::from(read).await {
                        Ok(buffer) => enqueue(name, js_sys::Uint8Array::new(&buffer).to_vec()),
                        Err(error) => {
                            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                                "ocs: failed to read dropped file \"{name}\": {}",
                                error.as_string().unwrap_or_else(|| format!("{error:?}"))
                            )))
                        }
                    }
                }
            });
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
