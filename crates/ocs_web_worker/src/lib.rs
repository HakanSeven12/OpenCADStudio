use std::io::Cursor;
use std::sync::Arc;

use acadrust::io::dwg::DwgReader;
use acadrust::{DwgReadOptions, DxfReader, DxfReaderConfiguration};
use js_sys::{Function, Uint8Array};
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;

const HASH_MARKER: &str = "\nreport-source-sha256:";
const PROTOCOL_VERSION: u16 = 5;

#[derive(serde::Serialize, serde::Deserialize)]
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

#[derive(serde::Serialize, serde::Deserialize)]
struct RawObjectFields {
    handle: acadrust::Handle,
    dxf_codes: Option<Vec<(i32, String)>>,
    dwg_data: Option<Vec<u8>>,
    dwg_version: Option<acadrust::types::DxfVersion>,
}

/// Parse DWG/DXF on a dedicated browser worker and return a compact serialized
/// document. The main wasm instance only deserializes and installs it, so the
/// expensive bit/handle/object decode never occupies the browser UI thread.
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
    if !matches!(ext.as_str(), "dwg" | "dxf") {
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
        _ => unreachable!(),
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
    // `recovered_errors` counts every Error notification, including reference
    // resolution warnings. Only actual source loss needs a second read.
    if !recovery_mode && source_data_lost(&outcome.stats) {
        let message = outcome
            .stats
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "drawing records were skipped or the stream ended early".into());
        return encode_result(Err((message, Some(outcome.stats))), None, true, &bytes);
    }
    if recovery_mode {
        outcome.document.notifications.notify(
            acadrust::notification::NotificationType::Error,
            format!("Initial read failed; recovery mode continued: {initial_error}"),
        );
        acadrust::push_read_diagnostic(
            &mut outcome.stats.diagnostics,
            acadrust::ReadDiagnostic::new(
                "strict-read-failed",
                acadrust::ReadStage::RecordStream,
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

fn report_fingerprint_needed(stats: &acadrust::ReadStats) -> bool {
    stats.recovery_mode || source_data_lost(stats)
}

fn source_data_lost(stats: &acadrust::ReadStats) -> bool {
    stats.skipped_source_records > 0 || !stats.stream_completed
}

fn recoverable_reader_error(error: &acadrust::DxfError) -> bool {
    matches!(
        error,
        acadrust::DxfError::Compression(_)
            | acadrust::DxfError::Parse(_)
            | acadrust::DxfError::InvalidDxfCode(_)
            | acadrust::DxfError::InvalidHandle(_)
            | acadrust::DxfError::ObjectNotFound(_)
            | acadrust::DxfError::InvalidEntityType(_)
            | acadrust::DxfError::ChecksumMismatch { .. }
            | acadrust::DxfError::InvalidHeader(_)
            | acadrust::DxfError::InvalidFormat(_)
            | acadrust::DxfError::InvalidSentinel(_)
            | acadrust::DxfError::Decompression(_)
            | acadrust::DxfError::Encoding(_)
    )
}

fn encode_result(
    result: Result<acadrust::ReadOutcome, (String, Option<acadrust::ReadStats>)>,
    source_sha256: Option<String>,
    recoverable_parse_error: bool,
    bytes: &[u8],
) -> Result<Uint8Array, JsValue> {
    let encoded = serialize_result(result, source_sha256, recoverable_parse_error)
        .map_err(|error| worker_error(error, bytes, true))?;
    Ok(Uint8Array::from(encoded.as_slice()))
}

fn serialize_result(
    result: Result<acadrust::ReadOutcome, (String, Option<acadrust::ReadStats>)>,
    source_sha256: Option<String>,
    recoverable_parse_error: bool,
) -> Result<Vec<u8>, String> {
    let runtime_fields = result
        .as_ref()
        .ok()
        .map(|outcome| {
            outcome
                .document
                .entities()
                .map(|entity| {
                    let common = entity.common();
                    EntityRuntimeFields {
                        handle: common.handle.value(),
                        linetype_handle: common.linetype_handle.map(|handle| handle.value()),
                        color_book_handle: common.color_book_handle.map(|handle| handle.value()),
                        face_visual_style_handle: common
                            .face_visual_style_handle
                            .map(|handle| handle.value()),
                        edge_visual_style_handle: common
                            .edge_visual_style_handle
                            .map(|handle| handle.value()),
                        material_flags: common.material_flags,
                        material_handle: common.material_handle.map(|handle| handle.value()),
                        shadow_flags: common.shadow_flags,
                        plotstyle_flags: common.plotstyle_flags,
                        plotstyle_handle: common.plotstyle_handle.map(|handle| handle.value()),
                        entity_mode: common.entity_mode,
                        has_ds_data: common.has_ds_data,
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let raw_objects = result
        .as_ref()
        .ok()
        .map(|outcome| {
            outcome
                .document
                .objects
                .iter()
                .filter_map(|(&handle, object)| match object {
                    acadrust::objects::ObjectType::Unknown {
                        raw_dxf_codes,
                        raw_dwg_data,
                        raw_dwg_version,
                        ..
                    } => Some(RawObjectFields {
                        handle,
                        dxf_codes: raw_dxf_codes.clone(),
                        dwg_data: raw_dwg_data.clone(),
                        dwg_version: *raw_dwg_version,
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // Stream serialization into compression: never allocate a second complete
    // uncompressed document alongside the parsed drawing.
    let encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut buffer = std::io::BufWriter::with_capacity(64 * 1024, encoder);
    bincode::serialize_into(
        &mut buffer,
        &(
            PROTOCOL_VERSION,
            result,
            source_sha256,
            recoverable_parse_error,
            runtime_fields,
            raw_objects,
        ),
    )
    .map_err(|error| error.to_string())?;
    buffer
        .into_inner()
        .map_err(|error| error.to_string())?
        .finish()
        .map_err(|error| error.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    const LINE_DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n100\n8\n0\n10\n0\n20\n0\n30\n0\n11\n10\n21\n10\n31\n0\n0\nENDSEC\n0\nEOF\n";

    #[test]
    fn complete_read_with_reference_warning_does_not_require_recovery() {
        let mut outcome = DxfReader::from_reader(Cursor::new(LINE_DXF.as_bytes()))
            .unwrap()
            .read_with_stats()
            .unwrap();
        assert!(outcome.stats.has_usable_drawing_data());
        outcome.stats.recovered_errors = 1;
        assert!(!source_data_lost(&outcome.stats));
        assert!(!report_fingerprint_needed(&outcome.stats));
        outcome.stats.skipped_source_records = 1;
        assert!(source_data_lost(&outcome.stats));
    }

    #[test]
    fn truncated_dxf_still_requires_recovery() {
        let bytes = LINE_DXF.strip_suffix("0\nEOF\n").unwrap().as_bytes();
        assert!(DxfReader::from_reader(Cursor::new(bytes))
            .unwrap()
            .read_with_stats()
            .is_err());
        let outcome = DxfReader::from_reader(Cursor::new(bytes))
            .unwrap()
            .with_configuration(DxfReaderConfiguration {
                failsafe: true,
                ..Default::default()
            })
            .read_with_stats()
            .unwrap();
        assert!(outcome.stats.has_usable_drawing_data());
        assert!(source_data_lost(&outcome.stats));
        assert!(report_fingerprint_needed(&outcome.stats));
    }

    fn dgn_stroke_record(
        count: i32,
        include_stroke: bool,
    ) -> acadrust::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader {
        use acadrust::io::dwg::{
            dwg_stream_writers::bit_writer::DwgBitWriter, dwg_version::DwgVersion,
        };
        use acadrust::types::DxfVersion;
        let mut writer = DwgBitWriter::new(DwgVersion::AC18, DxfVersion::AC1018);
        writer.write_variable_text("test stroke");
        writer.write_bit_long(1);
        writer.write_bit_long(3);
        writer.write_bytes(&[0; 16]);
        writer.write_bit_double(1.0);
        writer.write_bytes(&[0]);
        writer.write_bit(false);
        writer.write_bit(false);
        writer.write_bit_long(0);
        writer.write_bit_double(0.0);
        writer.write_bit_double(0.0);
        writer.write_bit(false);
        writer.write_bit(false);
        writer.write_bit_long(count);
        if include_stroke {
            for flag in [true, false, false, false, false] {
                writer.write_bit(flag);
            }
            for value in [2.0, 0.0, 0.0] {
                writer.write_bit_double(value);
            }
            writer.write_bit_long(0);
            writer.write_bit_long(0);
        }
        let bits = writer.position_in_bits();
        acadrust::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader::new(
            writer.into_bytes(),
            DxfVersion::AC1018,
            bits,
        )
    }

    #[test]
    fn dgn_count_cannot_expand_past_record_boundary() {
        use acadrust::io::dwg::dwg_stream_readers::object_reader::dgn_linestyle::read_dgn_line_style_data;
        for count in [100_000, i32::MAX, -1, 1] {
            assert!(read_dgn_line_style_data(
                &mut dgn_stroke_record(count, false),
                "LSSTROKEPATTERNCOMPONENT"
            )
            .is_none());
        }
        let value =
            read_dgn_line_style_data(&mut dgn_stroke_record(1, true), "LSSTROKEPATTERNCOMPONENT")
                .unwrap();
        match value {
            acadrust::objects::DgnLineStyleData::Component {
                component: acadrust::objects::DgnLsComponentData::Stroke(pattern),
                ..
            } => {
                assert_eq!(pattern.strokes.len(), 1);
                assert_eq!(pattern.strokes[0].length, 2.0);
            }
            _ => panic!("expected stroke pattern"),
        }
    }

    #[test]
    fn worker_document_survives_binary_transfer() {
        let outcome = DxfReader::from_reader(Cursor::new(LINE_DXF.as_bytes()))
            .unwrap()
            .read_with_stats()
            .unwrap();
        let bytes = bincode::serialize(&outcome).unwrap();
        let decoded: acadrust::ReadOutcome = bincode::deserialize(&bytes).unwrap();
        assert_eq!(outcome.stats, decoded.stats);
        assert_eq!(
            outcome.document.entity_count(),
            decoded.document.entity_count()
        );
        let entity = decoded.document.entities().next().unwrap();
        assert!(decoded
            .document
            .get_entity(entity.common().handle)
            .is_some());
    }

    #[test]
    fn compressed_transfer_preserves_unknown_dwg_payloads() {
        let mut outcome = DxfReader::from_reader(Cursor::new(LINE_DXF.as_bytes()))
            .unwrap()
            .read_with_stats()
            .unwrap();
        let handle = acadrust::Handle::new(0x200);
        outcome.document.objects.insert(
            handle,
            acadrust::objects::ObjectType::Unknown {
                type_name: "LSSTROKEPATTERNCOMPONENT".into(),
                handle,
                owner: acadrust::Handle::NULL,
                raw_dxf_codes: None,
                raw_dwg_data: Some(vec![1, 2, 3, 4]),
                raw_dwg_handle_bits: 8,
                raw_dwg_version: Some(acadrust::types::DxfVersion::AC1032),
            },
        );
        let encoded = serialize_result(Ok(outcome), None, false).unwrap();
        let decoded: (
            u16,
            Result<acadrust::ReadOutcome, (String, Option<acadrust::ReadStats>)>,
            Option<String>,
            bool,
            Vec<EntityRuntimeFields>,
            Vec<RawObjectFields>,
        ) = bincode::deserialize_from(std::io::BufReader::new(flate2::read::ZlibDecoder::new(
            encoded.as_slice(),
        )))
        .unwrap();
        assert_eq!(decoded.0, PROTOCOL_VERSION);
        assert!(decoded.1.unwrap().document.objects.contains_key(&handle));
        let raw = decoded.5.iter().find(|raw| raw.handle == handle).unwrap();
        assert_eq!(raw.dwg_data.as_deref(), Some([1, 2, 3, 4].as_slice()));
        assert_eq!(raw.dwg_version, Some(acadrust::types::DxfVersion::AC1032));
    }

    #[test]
    #[ignore = "Set OCS_TEST_DRAWING to a local DWG to inspect reader and transfer results"]
    fn inspect_local_drawing() {
        let path = std::env::var("OCS_TEST_DRAWING").expect("OCS_TEST_DRAWING path");
        let bytes = std::fs::read(path).unwrap();
        let started = std::time::Instant::now();
        let outcome = DwgReader::from_stream(Cursor::new(bytes))
            .read_with_stats()
            .unwrap();
        eprintln!("parse={:?}, stats={:#?}", started.elapsed(), outcome.stats);
        let entity_bytes: u64 = outcome
            .document
            .entities()
            .map(|e| bincode::serialized_size(e).unwrap())
            .sum();
        let object_bytes = bincode::serialized_size(&outcome.document.objects).unwrap();
        let mut largest: Vec<_> = outcome
            .document
            .objects
            .iter()
            .map(|(h, obj)| (bincode::serialized_size(obj).unwrap(), h, obj))
            .collect();
        largest.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        eprintln!("entities={entity_bytes}, objects={object_bytes}");
        let doc = &outcome.document;
        eprintln!("header={} layers={} linetypes={} textstyles={} blocks={} dims={} appids={} dgn_defs={} dgn_components={}",
            bincode::serialized_size(&doc.header).unwrap(),
            bincode::serialized_size(&doc.layers).unwrap(),
            bincode::serialized_size(&doc.line_types).unwrap(),
            bincode::serialized_size(&doc.text_styles).unwrap(),
            bincode::serialized_size(&doc.block_records).unwrap(),
            bincode::serialized_size(&doc.dim_styles).unwrap(),
            bincode::serialized_size(&doc.app_ids).unwrap(),
            bincode::serialized_size(&doc.dgn_ls_definitions).unwrap(),
            bincode::serialized_size(&doc.dgn_ls_components).unwrap());
        for (size, handle, object) in largest.iter().take(6) {
            eprintln!(
                "object {handle:?}: {size} bytes type={:?}",
                std::mem::discriminant(*object)
            );
        }
        eprintln!(
            "total serialized={}",
            bincode::serialized_size(&outcome).unwrap()
        );
        let expected_entities = outcome.document.entity_count();
        let expected_objects = outcome.document.objects.len();
        let encoded = serialize_result(Ok(outcome), None, false).unwrap();
        eprintln!("compressed transfer={}", encoded.len());
        let decoded: (
            u16,
            Result<acadrust::ReadOutcome, (String, Option<acadrust::ReadStats>)>,
            Option<String>,
            bool,
            Vec<EntityRuntimeFields>,
            Vec<RawObjectFields>,
        ) = bincode::deserialize_from(std::io::BufReader::new(flate2::read::ZlibDecoder::new(
            encoded.as_slice(),
        )))
        .unwrap();
        assert_eq!(decoded.0, PROTOCOL_VERSION);
        let restored = decoded.1.unwrap();
        assert_eq!(restored.document.entity_count(), expected_entities);
        assert_eq!(restored.document.objects.len(), expected_objects);
        eprintln!("total={:?}", started.elapsed());
    }
}
