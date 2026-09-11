use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{ErrorEvent, MessageEvent, Worker, WorkerOptions, WorkerType};

const HASH_MARKER: &str = "\nreport-source-sha256:";
const PROTOCOL_VERSION: u16 = 5;
const WORKER_URL: &str = "ocs-parse-worker.js?v=5";

#[derive(serde::Deserialize)]
struct EntityRuntimeFields {
    handle: u64,
    linetype_handle: Option<u64>,
    color_book_handle: Option<u64>,
    face_visual_style_handle: Option<u64>,
    edge_visual_style_handle: Option<u64>,
    material_flags: u8,
    material_handle: Option<u64>,
    shadow_flags: u8,
    plotstyle_flags: u8,
    plotstyle_handle: Option<u64>,
    entity_mode: Option<u8>,
    has_ds_data: bool,
}

#[derive(serde::Deserialize)]
struct RawObjectFields {
    handle: acadrust::Handle,
    dxf_codes: Option<Vec<(i32, String)>>,
    dwg_data: Option<Vec<u8>>,
    dwg_version: Option<acadrust::types::DxfVersion>,
}

pub(super) async fn parse_document(
    name: &str,
    bytes: &[u8],
    recovery_mode: bool,
    initial_error: &str,
) -> Result<(acadrust::ReadOutcome, Option<String>), super::OpenLoadError> {
    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let worker = Worker::new_with_options(WORKER_URL, &options)
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;

    let (sender, receiver) =
        iced::futures::channel::oneshot::channel::<Result<Uint8Array, super::OpenLoadError>>();
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
                .map(|value| Uint8Array::new(&value))
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
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let error_sender = sender;
    let on_error = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
        if let Some(sender) = error_sender.borrow_mut().take() {
            let _ = sender.send(Err(super::OpenLoadError::from(event.message())));
        }
    });
    worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));

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
    worker
        .post_message_with_transfer(&payload, &transfer)
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;

    let result = receiver.await;
    worker.set_onmessage(None);
    worker.set_onerror(None);
    // Release the parser's wasm heap before allocating the UI document.
    worker.terminate();
    let data = result
        .map_err(|_| super::OpenLoadError::from("CAD parser worker closed without a result"))??;
    crate::scene::yield_to_browser().await;
    let bytes = data.to_vec();
    let decoder = flate2::read::ZlibDecoder::new(bytes.as_slice());
    let payload: (
        u16,
        Result<acadrust::ReadOutcome, (String, Option<acadrust::ReadStats>)>,
        Option<String>,
        bool,
        Vec<EntityRuntimeFields>,
        Vec<RawObjectFields>,
    ) = bincode::deserialize_from(std::io::BufReader::new(decoder))
        .map_err(|error| super::OpenLoadError::from(error.to_string()))?;
    if payload.0 != PROTOCOL_VERSION {
        return Err(super::OpenLoadError::from(format!(
            "parser worker protocol mismatch: expected {}, received {}",
            PROTOCOL_VERSION, payload.0
        )));
    }
    match payload.1 {
        Ok(mut outcome) => {
            restore_entity_runtime_fields(&mut outcome.document, payload.4);
            for fields in payload.5 {
                if let Some(acadrust::objects::ObjectType::Unknown {
                    raw_dxf_codes,
                    raw_dwg_data,
                    raw_dwg_version,
                    ..
                }) = outcome.document.objects.get_mut(&fields.handle)
                {
                    *raw_dxf_codes = fields.dxf_codes;
                    *raw_dwg_data = fields.dwg_data;
                    *raw_dwg_version = fields.dwg_version;
                }
            }
            Ok((outcome, payload.2))
        }
        Err((message, read_stats)) => Err(super::OpenLoadError {
            message,
            source_sha256: payload.2,
            read_stats,
            recovery_available: payload.3,
        }),
    }
}

fn restore_entity_runtime_fields(
    document: &mut acadrust::CadDocument,
    fields: Vec<EntityRuntimeFields>,
) {
    let handle = |value: Option<u64>| value.map(acadrust::Handle::new);
    for fields in fields {
        let Some(entity) = document.get_entity_mut(acadrust::Handle::new(fields.handle)) else {
            continue;
        };
        let common = entity.common_mut();
        common.linetype_handle = handle(fields.linetype_handle);
        common.color_book_handle = handle(fields.color_book_handle);
        common.face_visual_style_handle = handle(fields.face_visual_style_handle);
        common.edge_visual_style_handle = handle(fields.edge_visual_style_handle);
        common.material_flags = fields.material_flags;
        common.material_handle = handle(fields.material_handle);
        common.shadow_flags = fields.shadow_flags;
        common.plotstyle_flags = fields.plotstyle_flags;
        common.plotstyle_handle = handle(fields.plotstyle_handle);
        common.entity_mode = fields.entity_mode;
        common.has_ds_data = fields.has_ds_data;
    }
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
        let result = Reflect::get(&data, &JsValue::from_str("digest"))
            .map_err(|error| super::OpenLoadError::from(js_error(error)))
            .and_then(|value| {
                value
                    .as_string()
                    .ok_or_else(|| super::OpenLoadError::from("hash worker returned no digest"))
            });
        if let Some(sender) = message_sender.borrow_mut().take() {
            let _ = sender.send(result);
        }
    });
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let error_sender = sender;
    let on_error = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
        if let Some(sender) = error_sender.borrow_mut().take() {
            let _ = sender.send(Err(super::OpenLoadError::from(event.message())));
        }
    });
    worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));

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
    worker
        .post_message_with_transfer(&payload, &transfer)
        .map_err(|error| super::OpenLoadError::from(js_error(error)))?;

    let result = receiver
        .await
        .map_err(|_| super::OpenLoadError::from("hash worker closed without a result"))?;
    worker.terminate();
    result
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

fn js_error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("browser worker error: {value:?}"))
}
