use std::io::Cursor;
use std::sync::Arc;

use codec::DwgReader;
use codec::{DwgReadOptions, DxfReader, DxfReaderConfiguration};
use js_sys::{Function, Uint8Array};
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;

const HASH_MARKER: &str = "\nreport-source-sha256:";

/// Version of the shared worker wire format (see `ocs_save`). Both sides of
/// the boundary — this crate and the app's `io::web_worker` — check payloads
/// against it.
const PROTOCOL_VERSION: u16 = ocs_save::PROTOCOL_VERSION;

/// Parse DWG/DXF (or a listed converter's format) on a dedicated browser
/// worker and return a compact serialized document. The main wasm instance
/// only deserializes and installs it, so the expensive bit/handle/object
/// decode never occupies the browser UI thread.
#[wasm_bindgen]
pub fn parse_document(
    name: String,
    bytes: Uint8Array,
    recovery_mode: bool,
    initial_error: String,
    report_stage: &Function,
) -> Result<Uint8Array, JsValue> {
    console_error_panic_hook::set_once();
    report_stage.call1(&JsValue::NULL, &JsValue::from_str("copy input"))?;
    let bytes: Arc<[u8]> = Arc::from(bytes.to_vec());
    report_stage.call1(&JsValue::NULL, &JsValue::from_str("parse document"))?;
    let ext = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    let external = ocs_save::external_format(&ext);
    if !matches!(ext.as_str(), "dwg" | "dxf") && external.is_none() {
        return encode_result(
            Err((format!("Unsupported file format: .{ext}"), None)),
            None,
            false,
            &bytes,
        );
    }
    let outcome_result = match ext.as_str() {
        "dwg" => {
            if recovery_mode {
                DwgReader::from_stream_with_options(
                    Cursor::new(Arc::clone(&bytes)),
                    DwgReadOptions::failsafe(),
                )
                .read_with_stats()
            } else {
                DwgReader::from_stream(Cursor::new(Arc::clone(&bytes))).read_with_stats()
            }
        }
        "dxf" => {
            if recovery_mode {
                DxfReader::from_reader(Cursor::new(Arc::clone(&bytes))).and_then(|reader| {
                    reader
                        .with_configuration(DxfReaderConfiguration {
                            failsafe: true,
                            ..DxfReaderConfiguration::default()
                        })
                        .read_with_stats()
                })
            } else {
                DxfReader::from_reader(Cursor::new(Arc::clone(&bytes)))
                    .and_then(|reader| reader.read_with_stats())
            }
        }
        _ => match external {
            Some(format) => {
                ocs_save::read_external(format, &bytes).map_err(codec::DxfError::Custom)
            }
            None => unreachable!(),
        },
    };
    let mut outcome = match outcome_result {
        Ok(outcome) => outcome,
        Err(error) => {
            let recoverable_parse_error = !recovery_mode && recoverable_reader_error(&error);
            let source_sha256 = recovery_mode.then(|| sha256_document_bytes(&bytes));
            return encode_result(
                Err((error.to_string(), None)),
                source_sha256,
                recoverable_parse_error,
                &bytes,
            );
        }
    };
    if !outcome.stats.has_usable_drawing_data() {
        let error = if recovery_mode {
            format!("initial read failed: {initial_error}; recovery found no usable drawing data")
        } else {
            "initial read returned no source drawing records".to_string()
        };
        let source_sha256 = recovery_mode.then(|| sha256_document_bytes(&bytes));
        return encode_result(
            Err((error, Some(outcome.stats))),
            source_sha256,
            !recovery_mode,
            &bytes,
        );
    }
    if !recovery_mode && report_fingerprint_needed(&outcome.stats) {
        let message = outcome
            .stats
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "normal read detected recoverable drawing errors".to_string());
        return encode_result(Err((message, Some(outcome.stats))), None, true, &bytes);
    }
    if recovery_mode {
        outcome.document.notifications.notify(
            codec::notification::NotificationType::Error,
            format!("Initial read failed; recovery mode continued: {initial_error}"),
        );
        codec::push_read_diagnostic(
            &mut outcome.stats.diagnostics,
            codec::ReadDiagnostic::new(
                "strict-read-failed",
                codec::ReadStage::RecordStream,
                initial_error,
            ),
        );
        outcome.stats.recovered_errors = outcome.stats.recovered_errors.saturating_add(1);
    }
    let source_sha256 =
        report_fingerprint_needed(&outcome.stats).then(|| sha256_document_bytes(&bytes));
    report_stage.call1(&JsValue::NULL, &JsValue::from_str("serialize document"))?;
    let encoded = encode_result(Ok(outcome), source_sha256, false, &bytes)?;
    report_stage.call1(&JsValue::NULL, &JsValue::from_str("copy output"))?;
    Ok(encoded)
}

#[wasm_bindgen]
pub fn sha256_document(bytes: Uint8Array) -> String {
    sha256_document_bytes(&bytes.to_vec())
}

/// Serialize an already-prepared document to DWG/DXF bytes on the dedicated
/// browser worker, so the expensive writer pass (bit-level DWG encoding /
/// ASCII DXF formatting) never occupies the UI thread. `encoded_document` is
/// the shared `ocs_save` request payload (document + runtime-field sidecar +
/// target extension); the reply is the shared response payload (protocol
/// version + `Result<bytes, error text>`). The main wasm instance only decodes
/// the result — the save-side counterpart of [`parse_document`].
#[wasm_bindgen]
pub fn serialize_document(
    encoded_document: Uint8Array,
    report_stage: &Function,
) -> Result<Uint8Array, JsValue> {
    console_error_panic_hook::set_once();
    report_stage.call1(&JsValue::NULL, &JsValue::from_str("decode document"))?;
    let (document, ext) = ocs_save::decode_request(&encoded_document.to_vec())?;
    report_stage.call1(&JsValue::NULL, &JsValue::from_str("serialize drawing"))?;
    let result = ocs_save::serialize_document(&document, &ext);
    report_stage.call1(&JsValue::NULL, &JsValue::from_str("copy output"))?;
    let encoded = ocs_save::encode_response(result)?;
    Ok(Uint8Array::from(encoded.as_slice()))
}

fn report_fingerprint_needed(stats: &codec::ReadStats) -> bool {
    stats.recovered() || stats.skipped_source_records > 0 || !stats.stream_completed
}

fn recoverable_reader_error(error: &codec::DxfError) -> bool {
    matches!(
        error,
        codec::DxfError::Compression(_)
            | codec::DxfError::Parse(_)
            | codec::DxfError::InvalidDxfCode(_)
            | codec::DxfError::InvalidHandle(_)
            | codec::DxfError::ObjectNotFound(_)
            | codec::DxfError::InvalidEntityType(_)
            | codec::DxfError::ChecksumMismatch { .. }
            | codec::DxfError::InvalidHeader(_)
            | codec::DxfError::InvalidFormat(_)
            | codec::DxfError::InvalidSentinel(_)
            | codec::DxfError::Decompression(_)
            | codec::DxfError::Encoding(_)
    )
}

fn encode_result(
    result: Result<codec::ReadOutcome, (String, Option<codec::ReadStats>)>,
    source_sha256: Option<String>,
    recoverable_parse_error: bool,
    bytes: &[u8],
) -> Result<Uint8Array, JsValue> {
    let runtime_fields = result
        .as_ref()
        .ok()
        .map(|outcome| ocs_save::extract_entity_runtime_fields(&outcome.document))
        .unwrap_or_default();
    let encoded = bincode::serialize(&(
        PROTOCOL_VERSION,
        result,
        source_sha256,
        recoverable_parse_error,
        runtime_fields,
    ))
    .map_err(|error| worker_error(error.to_string(), bytes, true))?;
    Ok(Uint8Array::from(encoded.as_slice()))
}

fn worker_error(error: String, bytes: &[u8], include_fingerprint: bool) -> JsValue {
    if include_fingerprint {
        JsValue::from_str(&format!(
            "{error}{HASH_MARKER}{}",
            sha256_document_bytes(bytes)
        ))
    } else {
        JsValue::from_str(&error)
    }
}

fn sha256_document_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
