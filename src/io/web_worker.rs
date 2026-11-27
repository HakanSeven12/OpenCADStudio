use bincode::Options as _;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{ErrorEvent, MessageEvent, Worker, WorkerOptions, WorkerType};

const HASH_MARKER: &str = "\nreport-source-sha256:";
/// Version of the shared worker wire format (`ocs_save`); the `?v=` on the
/// worker URL cache-busts to match. v5 adds the `serialize` action.
const PROTOCOL_VERSION: u16 = ocs_save::PROTOCOL_VERSION;
const WORKER_URL: &str = "ocs-parse-worker.js?v=7";
const MAX_WORKER_DECODE_BYTES: u64 = 512 * 1024 * 1024;

type ParseResult = Result<(document::ReadOutcome, Option<String>), super::OpenLoadError>;
type ParseSender = iced::futures::channel::oneshot::Sender<ParseResult>;
type SharedParseSender = Rc<RefCell<Option<ParseSender>>>;

thread_local! {
    static NEXT_PARSE_ID: Cell<u64> = const { Cell::new(1) };
    static ACTIVE_PARSE_WORKERS: RefCell<std::collections::HashMap<u64, (Worker, SharedParseSender)>> =
        RefCell::new(std::collections::HashMap::new());
}

struct WorkerGuard {
    worker: Worker,
    parse_id: Option<u64>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
}

impl WorkerGuard {
    fn new(
        worker: Worker,
        on_message: Closure<dyn FnMut(MessageEvent)>,
        on_error: Closure<dyn FnMut(ErrorEvent)>,
    ) -> Self {
        worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        Self {
            worker,
            parse_id: None,
            _on_message: on_message,
            _on_error: on_error,
        }
    }

    fn register_parse(&mut self, sender: &SharedParseSender) {
        let id = NEXT_PARSE_ID.with(|cell| {
            let id = cell.get();
            cell.set(id.wrapping_add(1).max(1));
            id
        });
        let worker_clone = self.worker.clone();
        let sender_clone = Rc::clone(sender);
        ACTIVE_PARSE_WORKERS.with(|map| {
            map.borrow_mut().insert(id, (worker_clone, sender_clone));
        });
        self.parse_id = Some(id);
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if let Some(id) = self.parse_id.take() {
            ACTIVE_PARSE_WORKERS.with(|map| {
                map.borrow_mut().remove(&id);
            });
        }
        self.worker.set_onmessage(None);
        self.worker.set_onerror(None);
        self.worker.terminate();
    }
}

pub(crate) fn cancel_active_parse_workers() {
    let active: Vec<(Worker, SharedParseSender)> = ACTIVE_PARSE_WORKERS.with(|map| {
        let mut map = map.borrow_mut();
        map.drain().map(|(_, entry)| entry).collect()
    });
    for (worker, sender) in active {
        worker.set_onmessage(None);
        worker.set_onerror(None);
        worker.terminate();
        if let Some(tx) = sender.borrow_mut().take() {
            let _ = tx.send(Err(super::OpenLoadError::from("Open cancelled")));
        }
    }
}

pub(super) async fn parse_document(
    name: &str,
    bytes: &[u8],
    recovery_mode: bool,
    initial_error: &str,
) -> Result<(document::ReadOutcome, Option<String>), super::OpenLoadError> {
    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let worker = Worker::new_with_options(WORKER_URL, &options)
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;

    let (sender, receiver) = iced::futures::channel::oneshot::channel::<ParseResult>();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let message_sender = sender.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data = event.data();
        let ok = Reflect::get(&data, &JsValue::from_str("ok"))
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let result = if ok {
            Reflect::get(&data, &JsValue::from_str("data"))
                .map_err(|error| super::OpenLoadError::from(js_error(error)))
                .and_then(|value| {
                    let bytes = Uint8Array::new(&value).to_vec();
                    let payload: (
                        u16,
                        Result<document::ReadOutcome, (String, Option<document::ReadStats>)>,
                        Option<String>,
                        bool,
                        Vec<ocs_save::EntityRuntimeFields>,
                    ) = bincode::options()
                        .with_fixint_encoding()
                        .allow_trailing_bytes()
                        .with_limit(MAX_WORKER_DECODE_BYTES)
                        .deserialize(&bytes)
                        .map_err(|error| super::OpenLoadError::from(error.to_string()))?;
                    if payload.0 != PROTOCOL_VERSION {
                        return Err(super::OpenLoadError::from(format!(
                            "parser worker protocol mismatch: expected {}, received {}",
                            PROTOCOL_VERSION, payload.0
                        )));
                    }
                    match payload.1 {
                        Ok(mut outcome) => {
                            ocs_save::restore_entity_runtime_fields(
                                &mut outcome.document,
                                payload.4,
                            );
                            Ok((outcome, payload.2))
                        }
                        Err((message, read_stats)) => Err(super::OpenLoadError {
                            message,
                            source_sha256: payload.2,
                            read_stats,
                            recovery_available: payload.3,
                        }),
                    }
                })
        } else {
            let message = Reflect::get(&data, &JsValue::from_str("error"))
                .ok()
                .and_then(|value| value.as_string())
                .unwrap_or_else(|| "CAD parser worker failed".to_string());
            Err(decode_worker_error(message))
        };
        if let Some(sender) = message_sender.borrow_mut().take() {
            let _ = sender.send(result);
        }
    });

    let error_sender = sender.clone();
    let on_error = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
        if let Some(sender) = error_sender.borrow_mut().take() {
            let _ = sender.send(Err(super::OpenLoadError::from(event.message())));
        }
    });

    let mut guard = WorkerGuard::new(worker, on_message, on_error);
    guard.register_parse(&sender);

    let payload = Object::new();
    Reflect::set(
        &payload,
        &JsValue::from_str("name"),
        &JsValue::from_str(name),
    )
    .map_err(|error| super::OpenLoadError::from(js_error(error)))?;
    Reflect::set(
        &payload,
        &JsValue::from_str("recoveryMode"),
        &JsValue::from_bool(recovery_mode),
    )
    .map_err(|error| super::OpenLoadError::from(js_error(error)))?;
    Reflect::set(
        &payload,
        &JsValue::from_str("initialError"),
        &JsValue::from_str(initial_error),
    )
    .map_err(|error| super::OpenLoadError::from(js_error(error)))?;
    let input = Uint8Array::from(bytes);
    Reflect::set(&payload, &JsValue::from_str("bytes"), &input.buffer())
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;
    let transfer = Array::new();
    transfer.push(&input.buffer());
    guard
        .worker
        .post_message_with_transfer(&payload, &transfer)
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;

    receiver
        .await
        .map_err(|_| super::OpenLoadError::from("CAD parser worker closed without a result"))?
}

pub(super) async fn sha256_document(bytes: &[u8]) -> Result<String, super::OpenLoadError> {
    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let worker = Worker::new_with_options(WORKER_URL, &options)
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;

    let (sender, receiver) =
        iced::futures::channel::oneshot::channel::<Result<String, super::OpenLoadError>>();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let message_sender = sender.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data = event.data();
        let ok = Reflect::get(&data, &JsValue::from_str("ok"))
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let result = if ok {
            Reflect::get(&data, &JsValue::from_str("digest"))
                .map_err(|error| super::OpenLoadError::from(js_error(error)))
                .and_then(|value| {
                    value
                        .as_string()
                        .ok_or_else(|| super::OpenLoadError::from("hash worker returned no digest"))
                })
        } else {
            let message = Reflect::get(&data, &JsValue::from_str("error"))
                .ok()
                .and_then(|value| value.as_string())
                .unwrap_or_else(|| "hash worker failed".to_string());
            Err(decode_worker_error(message))
        };
        if let Some(sender) = message_sender.borrow_mut().take() {
            let _ = sender.send(result);
        }
    });

    let error_sender = sender;
    let on_error = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
        if let Some(sender) = error_sender.borrow_mut().take() {
            let _ = sender.send(Err(super::OpenLoadError::from(event.message())));
        }
    });

    let guard = WorkerGuard::new(worker, on_message, on_error);

    let payload = Object::new();
    Reflect::set(
        &payload,
        &JsValue::from_str("action"),
        &JsValue::from_str("hash"),
    )
    .map_err(|error| super::OpenLoadError::from(js_error(error)))?;
    let input = Uint8Array::from(bytes);
    Reflect::set(&payload, &JsValue::from_str("bytes"), &input.buffer())
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;
    let transfer = Array::new();
    transfer.push(&input.buffer());
    guard
        .worker
        .post_message_with_transfer(&payload, &transfer)
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;

    receiver
        .await
        .map_err(|_| super::OpenLoadError::from("hash worker closed without a result"))?
}

fn decode_worker_error(message: String) -> super::OpenLoadError {
    let Some((message, digest)) = message.rsplit_once(HASH_MARKER) else {
        return super::OpenLoadError::from(message);
    };
    let digest = digest.trim_start().get(..64).unwrap_or_default();
    let source_sha256 = (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| digest.to_ascii_lowercase());
    super::OpenLoadError::new(message.to_string(), source_sha256)
}

/// Run the DWG/DXF writer pass for SAVE/AUTOSAVE on the browser worker.
/// `encoded_request` is the shared `ocs_save` payload (prepared document +
/// runtime-field sidecar + target extension); the worker replies with the
/// encoded drawing bytes. Any worker failure — cannot spawn, script error,
/// stale protocol — falls back to decoding the document here and serializing
/// inline on this thread, so a save never hard-fails because the worker is
/// unavailable (strict CSP, blocked worker script, out-of-memory spawn...).
pub(super) async fn serialize_document(encoded_request: Vec<u8>) -> Result<Vec<u8>, String> {
    match serialize_document_via_worker(&encoded_request).await {
        Ok(bytes) => Ok(bytes),
        Err(worker_error) => {
            log::warn!("save worker unavailable, serializing inline: {worker_error}");
            let (document, ext) = ocs_save::decode_request(&encoded_request)?;
            ocs_save::serialize_document(&document, &ext)
        }
    }
}

async fn serialize_document_via_worker(encoded_request: &[u8]) -> Result<Vec<u8>, String> {
    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let worker = Worker::new_with_options(WORKER_URL, &options).map_err(|error| js_error(error))?;

    let (sender, receiver) = iced::futures::channel::oneshot::channel::<Result<Vec<u8>, String>>();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let message_sender = sender.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data = event.data();
        let ok = Reflect::get(&data, &JsValue::from_str("ok"))
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let result = if ok {
            Reflect::get(&data, &JsValue::from_str("data"))
                .map_err(|error| js_error(error))
                .and_then(|value| {
                    let bytes = Uint8Array::new(&value).to_vec();
                    ocs_save::decode_response(&bytes)
                })
                .and_then(|result| result)
        } else {
            let message = Reflect::get(&data, &JsValue::from_str("error"))
                .ok()
                .and_then(|value| value.as_string())
                .unwrap_or_else(|| "CAD serializer worker failed".to_string());
            Err(message)
        };
        if let Some(sender) = message_sender.borrow_mut().take() {
            let _ = sender.send(result);
        }
    });
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let error_sender = sender;
    let on_error = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
        if let Some(sender) = error_sender.borrow_mut().take() {
            let _ = sender.send(Err(event.message()));
        }
    });
    let guard = WorkerGuard::new(worker, on_message, on_error);

    let payload = Object::new();
    Reflect::set(
        &payload,
        &JsValue::from_str("action"),
        &JsValue::from_str("serialize"),
    )
    .map_err(|error| js_error(error))?;
    let input = Uint8Array::from(encoded_request);
    Reflect::set(&payload, &JsValue::from_str("document"), &input.buffer())
        .map_err(|error| js_error(error))?;
    let transfer = Array::new();
    transfer.push(&input.buffer());
    guard
        .worker
        .post_message_with_transfer(&payload, &transfer)
        .map_err(|error| js_error(error))?;

    receiver
        .await
        .map_err(|_| "CAD serializer worker closed without a result".to_string())?
}

fn js_error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("browser worker error: {value:?}"))
}
