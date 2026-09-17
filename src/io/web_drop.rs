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
/// Entries are just name+bytes; 32 drawings is a realistic folder drop.
const QUEUE_CAP: usize = 32;

/// One queued open. `restore_key` tags an autosave-restore file so its
/// snapshot is deleted only when THIS open succeeds — matching by identity,
/// never by display name (same-named files are one OPFS copy on web).
pub struct QueuedOpen {
    pub name: String,
    pub bytes: Vec<u8>,
    pub restore_key: Option<String>,
}

static DROPPED: Mutex<VecDeque<QueuedOpen>> = Mutex::new(VecDeque::new());
// Dropped raster images never open as drawings: they are registered by name
// so RasterImage entities the open drawing references can resolve them.
static DROPPED_IMAGES: Mutex<VecDeque<(String, Vec<u8>)>> = Mutex::new(VecDeque::new());
// `OnceLock::take` needs `&mut self`, which an immutable static cannot
// provide — an AtomicBool swap gives the same set-once marker and can still
// be reverted when no document exists.
static INSTALLED: AtomicBool = AtomicBool::new(false);

fn warn(line: &str) {
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(line));
}

fn lock_queue() -> std::sync::MutexGuard<'static, VecDeque<QueuedOpen>> {
    // The critical sections are push/pop only, so poisoning is unreachable;
    // recover anyway rather than panic across the JS boundary.
    DROPPED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `false` (after a console warning) when the queue is full and the
/// entry was NOT stored — the caller decides how to surface the loss.
fn push(entry: QueuedOpen) -> bool {
    let name = entry.name.clone();
    let mut queue = lock_queue();
    if queue.len() >= QUEUE_CAP {
        drop(queue);
        warn(&format!(
            "ocs: dropped-file queue full; ignoring \"{name}\""
        ));
        return false;
    }
    queue.push_back(entry);
    true
}

/// Queue already-read drawing bytes (drag-and-drop) for the 50 ms poll to
/// open through the guarded web-picker flow. Returns `false` when the queue
/// is full and the bytes were dropped.
pub fn enqueue_open(name: String, bytes: Vec<u8>) -> bool {
    push(QueuedOpen {
        name,
        bytes,
        restore_key: None,
    })
}

/// Queue a dropped raster image for the poll to register as decodable bytes
/// (name-keyed) — the web analogue of the drawing folder a drawing's images
/// live beside natively. Returns `false` when the queue is full.
pub fn enqueue_image(name: String, bytes: Vec<u8>) -> bool {
    let mut queue = DROPPED_IMAGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if queue.len() >= QUEUE_CAP {
        drop(queue);
        warn(&format!(
            "ocs: dropped-image queue full; ignoring \"{name}\""
        ));
        return false;
    }
    queue.push_back((name, bytes));
    true
}

/// Pop the oldest dropped image the poll has not registered yet.
pub fn take_dropped_image() -> Option<(String, Vec<u8>)> {
    DROPPED_IMAGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .pop_front()
}

/// Queue an autosave-restore file tagged with its snapshot key. Returns
/// `false` when the queue is full and the bytes were dropped (the OPFS
/// snapshot survives; the caller must not wait for an open that will
/// never start).
pub fn enqueue_restore(name: String, bytes: Vec<u8>, restore_key: String) -> bool {
    push(QueuedOpen {
        name,
        bytes,
        restore_key: Some(restore_key),
    })
}

/// JS-callable entry (same shape as `ocs_control_submit`) for hosts that
/// forward drops themselves.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn ocs_web_drop_file(name: String, bytes: js_sys::Uint8Array) {
    let _ = enqueue_open(name, bytes.to_vec());
}

/// Pop the oldest dropped file that the poll has not started opening yet.
pub fn take_dropped() -> Option<QueuedOpen> {
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
    let cancel_ok = document
        .add_event_listener_with_callback("dragenter", cancel.as_ref().unchecked_ref())
        .is_ok()
        & document
            .add_event_listener_with_callback("dragover", cancel.as_ref().unchecked_ref())
            .is_ok();
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
            let mut image_reads: Vec<(String, js_sys::Promise)> = Vec::new();
            for index in 0..files.length() {
                let Some(file) = files.get(index) else {
                    continue;
                };
                let name = file.name();
                // A drop has no picker filter: accept the formats the web Open
                // dialog accepts, plus the raster formats the IMAGE command
                // accepts; anything else is logged, never parsed.
                if is_drawing_name(&name) {
                    reads.push((name, file.array_buffer()));
                } else if is_image_name(&name) {
                    image_reads.push((name, file.array_buffer()));
                } else {
                    warn(&format!(
                        "ocs: ignored dropped file \"{name}\" (not .dwg/.dxf/image)"
                    ));
                }
            }
            if reads.is_empty() && image_reads.is_empty() {
                return;
            }
            // The page is the only intended drop target; hide drawing drops
            // from host-page handlers only once there is something to open.
            event.stop_propagation();
            wasm_bindgen_futures::spawn_local(async move {
                for (name, read) in reads {
                    match JsFuture::from(read).await {
                        Ok(buffer) => {
                            // A full queue warns on the console; the drop
                            // handler has no command line to talk to.
                            let _ = enqueue_open(name, js_sys::Uint8Array::new(&buffer).to_vec());
                        }
                        Err(error) => {
                            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                                "ocs: failed to read dropped file \"{name}\": {}",
                                error.as_string().unwrap_or_else(|| format!("{error:?}"))
                            )))
                        }
                    }
                }
                for (name, read) in image_reads {
                    match JsFuture::from(read).await {
                        Ok(buffer) => {
                            let _ = enqueue_image(name, js_sys::Uint8Array::new(&buffer).to_vec());
                        }
                        Err(error) => {
                            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                                "ocs: failed to read dropped image \"{name}\": {}",
                                error.as_string().unwrap_or_else(|| format!("{error:?}"))
                            )))
                        }
                    }
                }
            });
        },
    );
    let drop_ok = document
        .add_event_listener_with_callback("drop", on_drop.as_ref().unchecked_ref())
        .is_ok();
    on_drop.forget();
    if !cancel_ok || !drop_ok {
        // Unlatch so a later install can retry with fresh listeners.
        INSTALLED.store(false, Ordering::SeqCst);
    }
}

fn is_drawing_name(name: &str) -> bool {
    // Same convention as the worker's reader dispatch (`rsplit('.').next()`):
    // a dot-less name is its own extension, so a file literally named
    // "DWG"/"dxf" parses and must not be filtered out here.
    matches!(
        name.rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "dwg" | "dxf"
    )
}

/// The decodable raster set matches the IMAGE picker's filter (and the `image`
/// crate features the app builds with) — anything else would queue bytes that
/// can only ever fail to decode.
fn is_image_name(name: &str) -> bool {
    matches!(
        name.rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "png" | "jpg" | "jpeg" | "bmp" | "tiff" | "tif"
    )
}
