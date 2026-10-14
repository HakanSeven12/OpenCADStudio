//! Shared SAVE/serialize pipeline pieces.
//!
//! Both sides of the browser worker boundary use this crate so the DWG/DXF
//! byte serialization and the worker wire format have exactly one
//! implementation:
//!
//! - the app crate's inline `io::save_to_bytes` (native host fallback and the
//!   web fallback path when the worker is unavailable), and
//! - `ocs_web_worker`'s `serialize_document` action, which runs the same
//!   serialization on the dedicated worker thread.
//!
//! The document-crossing helpers (bincode encode/decode plus the runtime-field
//! sidecar) are also shared with the existing `parse_document` action, which
//! returns documents over the same boundary in the opposite direction.

use codec::{CadDocument, DwgWriter, DxfWriter};

/// Worker protocol version. Bumped whenever a request/response payload shape
/// changes; the `?v=` cache-buster on the worker URL must match. v5 adds the
/// `serialize` action and extends the runtime-field sidecar with
/// `graphic_data`.
pub const PROTOCOL_VERSION: u16 = 5;

/// Serialize a document to an in-memory byte buffer, format chosen by `ext`
/// (`dxf` → DXF, anything else → DWG). The document is expected to be fully
/// prepared for saving (version stamped, current styles synced, dimension
/// blocks baked) — see the app crate's `io::prepare_document_for_save`, which
/// owns that app-side half of the pipeline.
pub fn serialize_document(doc: &CadDocument, ext: &str) -> Result<Vec<u8>, String> {
    match ext.to_lowercase().as_str() {
        "dxf" => DxfWriter::new(doc)
            .write_to_vec()
            .map_err(|e| e.to_string()),
        _ => {
            let mut buf = std::io::Cursor::new(Vec::new());
            DwgWriter::write_to_writer(&mut buf, doc).map_err(|e| e.to_string())?;
            Ok(buf.into_inner())
        }
    }
}

// ── Runtime-field sidecar ──────────────────────────────────────────────────

/// Per-entity fields the codec marks `serde(skip)` on `EntityCommon` (raw
/// DWG storage/round-trip data the logical model does not own). They do not
/// survive a bincode crossing on their own, so every document that crosses
/// the worker boundary carries them beside the payload and re-installs them
/// after decode — otherwise the saved file would silently lose per-entity
/// linetype/material/plotstyle handles and raw graphic bytes.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct EntityRuntimeFields {
    pub handle: u64,
    pub linetype_handle: Option<u64>,
    pub color_book_handle: Option<u64>,
    pub face_visual_style_handle: Option<u64>,
    pub edge_visual_style_handle: Option<u64>,
    pub material_flags: u8,
    pub material_handle: Option<u64>,
    pub shadow_flags: u8,
    pub plotstyle_flags: u8,
    pub plotstyle_handle: Option<u64>,
    pub entity_mode: Option<u8>,
    pub has_ds_data: bool,
    pub graphic_data: Option<Vec<u8>>,
}

/// Collect the serde-skipped runtime fields of every entity in `doc`.
pub fn extract_entity_runtime_fields(doc: &CadDocument) -> Vec<EntityRuntimeFields> {
    doc.entities()
        .map(|entity| {
            let common = entity.common();
            let handle = |value: Option<codec::Handle>| value.map(|handle| handle.value());
            EntityRuntimeFields {
                handle: common.handle.value(),
                linetype_handle: handle(common.linetype_handle),
                color_book_handle: handle(common.color_book_handle),
                face_visual_style_handle: handle(common.face_visual_style_handle),
                edge_visual_style_handle: handle(common.edge_visual_style_handle),
                material_flags: common.material_flags,
                material_handle: handle(common.material_handle),
                shadow_flags: common.shadow_flags,
                plotstyle_flags: common.plotstyle_flags,
                plotstyle_handle: handle(common.plotstyle_handle),
                entity_mode: common.entity_mode,
                has_ds_data: common.has_ds_data,
                graphic_data: common.graphic_data.clone(),
            }
        })
        .collect()
}

/// Re-install runtime fields collected by [`extract_entity_runtime_fields`]
/// into a document that crossed a serde boundary. Entities whose handle no
/// longer resolves are skipped silently (mirrors the parse direction).
pub fn restore_entity_runtime_fields(document: &mut CadDocument, fields: Vec<EntityRuntimeFields>) {
    let handle = |value: Option<u64>| value.map(codec::Handle::new);
    for fields in fields {
        let Some(entity) = document.get_entity_mut(codec::Handle::new(fields.handle)) else {
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
        common.graphic_data = fields.graphic_data;
    }
}

// ── Worker wire format (`serialize` action) ────────────────────────────────
//
// Request  (main → worker): bincode `(PROTOCOL_VERSION, ext, &CadDocument,
//           Vec<EntityRuntimeFields>)` transferred as an ArrayBuffer.
// Response (worker → main): bincode `(PROTOCOL_VERSION, Result<Vec<u8>,
//           String>)` — the encoded DWG/DXF bytes on success, the writer's
//           error text on failure.

/// Encode a prepared document for the trip main → worker. Costs one bincode
/// pass over the document plus the runtime-field sidecar; see the crate docs
/// for why the sidecar exists.
pub fn encode_request(doc: &CadDocument, ext: &str) -> Result<Vec<u8>, String> {
    let runtime_fields = extract_entity_runtime_fields(doc);
    bincode::serialize(&(PROTOCOL_VERSION, ext, doc, runtime_fields))
        .map_err(|error| format!("encode save document for worker: {error}"))
}

/// Decode a `serialize` request on the worker: returns the document (with
/// runtime fields restored) and the target extension.
pub fn decode_request(bytes: &[u8]) -> Result<(CadDocument, String), String> {
    let (version, ext, mut document, runtime_fields): (
        u16,
        String,
        CadDocument,
        Vec<EntityRuntimeFields>,
    ) = bincode::deserialize(bytes).map_err(|error| format!("decode save document: {error}"))?;
    if version != PROTOCOL_VERSION {
        return Err(format!(
            "serializer worker protocol mismatch: expected {PROTOCOL_VERSION}, received {version}"
        ));
    }
    restore_entity_runtime_fields(&mut document, runtime_fields);
    Ok((document, ext))
}

/// Encode the worker's reply: the serialized drawing bytes, or the error text.
pub fn encode_response(result: Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    bincode::serialize(&(PROTOCOL_VERSION, result))
        .map_err(|error| format!("encode save response: {error}"))
}

/// Decode the worker's reply on the main thread, rejecting stale-protocol
/// payloads.
pub fn decode_response(bytes: &[u8]) -> Result<Result<Vec<u8>, String>, String> {
    let (version, result): (u16, Result<Vec<u8>, String>) =
        bincode::deserialize(bytes).map_err(|error| format!("decode save response: {error}"))?;
    if version != PROTOCOL_VERSION {
        return Err(format!(
            "serializer worker protocol mismatch: expected {PROTOCOL_VERSION}, received {version}"
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{EntityType, Line, Point};
    use codec::types::Vector3;

    fn sample_document() -> CadDocument {
        let mut doc = CadDocument::new();
        let mut pt = Point::new();
        pt.common.handle = doc.allocate_handle();
        pt.common.layer = "RUNTIME-FIELDS".to_string();
        // Fill every serde-skipped field with a recognizable value.
        pt.common.linetype_handle = Some(codec::Handle::new(0xABCD));
        pt.common.color_book_handle = Some(codec::Handle::new(0x101));
        pt.common.material_flags = 0b11;
        pt.common.material_handle = Some(codec::Handle::new(0x202));
        pt.common.plotstyle_flags = 0b11;
        pt.common.plotstyle_handle = Some(codec::Handle::new(0x303));
        pt.common.shadow_flags = 7;
        pt.common.entity_mode = Some(2);
        pt.common.has_ds_data = true;
        pt.common.graphic_data = Some(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let _ = doc.add_entity(EntityType::Point(pt));
        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        line.common.handle = doc.allocate_handle();
        let _ = doc.add_entity(EntityType::Line(line));
        doc
    }

    // The sidecar is the whole reason documents don't lose raw DWG storage
    // fields when they cross the worker boundary: serde skips them, so
    // encode → decode must restore every one.
    #[test]
    fn request_round_trip_restores_runtime_fields() {
        let doc = sample_document();
        let encoded = encode_request(&doc, "dwg").expect("encode");
        let (decoded, ext) = decode_request(&encoded).expect("decode");
        assert_eq!(ext, "dwg");
        assert_eq!(decoded.entities().count(), 2);
        let point = decoded
            .entities()
            .find(|e| matches!(e, EntityType::Point(_)))
            .expect("point survived");
        let common = point.common();
        assert_eq!(common.layer, "RUNTIME-FIELDS");
        assert_eq!(common.linetype_handle.map(|h| h.value()), Some(0xABCD));
        assert_eq!(common.color_book_handle.map(|h| h.value()), Some(0x101));
        assert_eq!(common.material_flags, 0b11);
        assert_eq!(common.material_handle.map(|h| h.value()), Some(0x202));
        assert_eq!(common.plotstyle_flags, 0b11);
        assert_eq!(common.plotstyle_handle.map(|h| h.value()), Some(0x303));
        assert_eq!(common.shadow_flags, 7);
        assert_eq!(common.entity_mode, Some(2));
        assert!(common.has_ds_data);
        assert_eq!(
            common.graphic_data.as_deref(),
            Some(&[0xDE, 0xAD, 0xBE, 0xEF][..])
        );
    }

    #[test]
    fn response_round_trip_ok_and_err() {
        let ok = decode_response(&encode_response(Ok(vec![1, 2, 3])).expect("encode"))
            .expect("decode")
            .expect("inner ok");
        assert_eq!(ok, vec![1, 2, 3]);
        let err =
            decode_response(&encode_response(Err("writer failed".to_string())).expect("encode"))
                .expect("decode")
                .expect_err("inner err");
        assert_eq!(err, "writer failed");
    }

    #[test]
    fn response_rejects_foreign_protocol_version() {
        let stale =
            bincode::serialize(&(PROTOCOL_VERSION - 1, Ok::<Vec<u8>, String>(vec![]))).unwrap();
        assert!(decode_response(&stale).is_err());
    }

    #[test]
    fn request_rejects_foreign_protocol_version() {
        let stale = bincode::serialize(&(
            PROTOCOL_VERSION - 1,
            "dwg",
            CadDocument::new(),
            Vec::<EntityRuntimeFields>::new(),
        ))
        .unwrap();
        assert!(decode_request(&stale).is_err());
    }

    // The ext dispatch both save paths (inline fallback and worker) share:
    // `dxf` must produce an ASCII DXF, anything else the DWG sentinel.
    #[test]
    fn serialize_document_picks_format_by_extension() {
        let mut doc = CadDocument::new();
        doc.version = codec::DxfVersion::AC1032;
        let dxf = serialize_document(&doc, "dxf").expect("dxf bytes");
        assert!(!dxf.is_empty());
        assert!(
            dxf.starts_with(b"  0") || dxf.starts_with(b"0\n"),
            "not a DXF header"
        );
        let dwg = serialize_document(&doc, "dwg").expect("dwg bytes");
        assert!(dwg.starts_with(b"AC10"), "not a DWG sentinel");
        // Case-insensitive: the tab state and save dialog both pass through.
        let dwg_upper = serialize_document(&doc, "DWG").expect("dwg bytes");
        assert!(dwg_upper.starts_with(b"AC10"));
    }

    // The worker path must serialize to the same bytes the inline fallback
    // would — the whole point of sharing this implementation.
    #[test]
    fn serialize_document_is_deterministic_across_calls() {
        let doc = sample_document();
        let a = serialize_document(&doc, "dxf").expect("bytes a");
        let b = serialize_document(&doc, "dxf").expect("bytes b");
        assert_eq!(a, b);
    }
}
