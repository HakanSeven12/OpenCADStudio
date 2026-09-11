//! Persists the drawing-wide named-parameter table in a versioned XRecord.

use super::named_parameters::ParameterTable;
use super::Scene;
use bincode::Options;

const XRECORD_KEY: &str = "OCS_NAMED_PARAMETERS";

/// See `sketch_persist::MAX_CHUNK_BYTES`'s doc comment: the vendored DWG
/// writer truncates any single `XRecordValue::Chunk` entry to `u8::MAX`
/// bytes, so a table past a handful of parameters must be split across
/// multiple same-code `310` entries on write and concatenated back on read.
const MAX_CHUNK_BYTES: usize = u8::MAX as usize;
const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;

/// Prefixed onto the serialized blob so a future schema change can be
/// detected and gracefully skipped rather than silently misreading bytes —
/// same call-out as `sketch_persist::FORMAT_VERSION`, same caveat that the
/// migration path itself is still undesigned.
const FORMAT_VERSION: u8 = 1;

fn encode(table: &ParameterTable) -> Option<Vec<u8>> {
    let mut bytes = vec![FORMAT_VERSION];
    let body = bincode::serialize(table).ok()?;
    if body.len() >= MAX_RECORD_BYTES {
        return None;
    }
    bytes.extend(body);
    Some(bytes)
}

fn decode(bytes: &[u8]) -> Option<ParameterTable> {
    let (&version, body) = bytes.split_first()?;
    if version != FORMAT_VERSION {
        return None;
    }
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(MAX_RECORD_BYTES as u64)
        .deserialize(body)
        .ok()
}

impl Scene {
    /// Writes `named_parameters` into the document's `OCS_NAMED_PARAMETERS`
    /// XRecord, ready for whatever save call happens next to serialize
    /// `self.document` as-is. Called from the same save entry points as
    /// `materialize_sketch_constraints_for_save`
    /// (`OpenCADStudio::prepare_native_save`, the wasm save path).
    ///
    /// An empty table is skipped rather than writing an empty XRecord — an
    /// unconstrained-by-parameters drawing (the common case, at least until
    /// this feature is used) should not gain persisted-but-empty
    /// bookkeeping on every save.
    pub(crate) fn materialize_named_parameters_for_save(&mut self) {
        if self.named_parameters.is_empty() {
            return;
        }
        let Some(bytes) = encode(&self.named_parameters) else {
            return;
        };
        let owner = self.document.header.model_space_block_handle;
        if owner.is_null() {
            return;
        }
        self.document.ensure_xrecord(owner, XRECORD_KEY);
        if let Some(record) = self.document.xrecord_mut(owner, XRECORD_KEY) {
            // Overwrite, not append: a resave must replace the prior blob,
            // not accumulate more Chunk entries every time.
            record.entries.clear();
            for chunk in bytes.chunks(MAX_CHUNK_BYTES) {
                record.entries.push(acadrust::objects::XRecordEntry::new(
                    310,
                    acadrust::objects::XRecordValue::Chunk(chunk.to_vec()),
                ));
            }
        }
    }

    /// Populates `named_parameters` from the just-loaded `self.document`'s
    /// `OCS_NAMED_PARAMETERS` XRecord, if any — called right after a
    /// document open installs its `CadDocument` into this `Scene`, the same
    /// call sites `load_sketch_constraints_from_document` already has
    /// (`OpenCADStudio::on_file_opened`, the automation `"open"`/`"new"`
    /// ops). Replaces whatever was already in `named_parameters` (a fresh
    /// table for a real open; `"new"`'s blank document has nothing to find
    /// anyway).
    pub(crate) fn load_named_parameters_from_document(&mut self) {
        self.named_parameters = ParameterTable::new();
        let owner = self.document.header.model_space_block_handle;
        if owner.is_null() {
            return;
        }
        let Some(record) = self.document.xrecord(owner, XRECORD_KEY) else {
            return;
        };
        // Concatenate every Chunk entry in order, not just the first — see
        // `MAX_CHUNK_BYTES`'s doc comment.
        let mut bytes = Vec::new();
        for entry in &record.entries {
            if let acadrust::objects::XRecordValue::Chunk(chunk) = &entry.value {
                if bytes.len().saturating_add(chunk.len()) > MAX_RECORD_BYTES {
                    bytes.clear();
                    break;
                }
                bytes.extend_from_slice(chunk);
            }
        }
        if bytes.is_empty() {
            return;
        }
        if let Some(table) = decode(&bytes) {
            self.named_parameters = table;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parameter_table_survives_materialize_and_reload() {
        let mut scene = Scene::new();
        scene.named_parameters_mut().set("hole_dia", "5").unwrap();
        scene
            .named_parameters_mut()
            .set("hole_spacing", "2 * hole_dia + 1.5")
            .unwrap();

        scene.materialize_named_parameters_for_save();
        // Simulate the load-time side of a save/reload round trip: drop the
        // in-memory table (a fresh open starts empty) and repopulate purely
        // from what materialize just wrote into `document.objects`.
        scene.named_parameters = ParameterTable::new();
        scene.load_named_parameters_from_document();

        assert_eq!(scene.named_parameters().len(), 2);
        assert_eq!(scene.named_parameters().resolve("hole_spacing"), Ok(11.5));
        assert_eq!(
            scene.named_parameters().get("hole_dia").unwrap().source,
            "5"
        );
    }

    /// The above test only proves `materialize`/`load` agree with each
    /// other on the same in-memory `CadDocument` — this drives a real
    /// `save_to_bytes`/`load_bytes` round trip (the same primitives
    /// `OpenCADStudio::on_file_opened`'s native save/open path uses),
    /// through both supported formats, mirroring
    /// `sketch_persist`'s own real-bytes tests.
    fn full_bytes_roundtrip(ext: &str) -> ParameterTable {
        let mut scene = Scene::new();
        scene
            .named_parameters_mut()
            .set("plate_len", "42.5")
            .unwrap();
        scene
            .named_parameters_mut()
            .set("plate_width", "plate_len / 2")
            .unwrap();

        scene.materialize_named_parameters_for_save();
        let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
            .unwrap_or_else(|e| panic!("save to {ext} bytes: {e}"));
        let mut reloaded_scene = Scene::new();
        reloaded_scene.document =
            crate::io::load_bytes(&format!("named_params_roundtrip.{ext}"), bytes)
                .unwrap_or_else(|e| panic!("reload {ext} bytes: {e}"));
        reloaded_scene.load_named_parameters_from_document();

        reloaded_scene.named_parameters
    }

    #[test]
    fn a_parameter_table_survives_a_real_dxf_save_and_load_round_trip() {
        let restored = full_bytes_roundtrip("dxf");
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.resolve("plate_width"), Ok(21.25));
    }

    #[test]
    fn a_parameter_table_survives_a_real_dwg_save_and_load_round_trip() {
        let restored = full_bytes_roundtrip("dwg");
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.resolve("plate_width"), Ok(21.25));
    }

    #[test]
    fn an_empty_table_is_not_materialized() {
        let mut scene = Scene::new();
        scene.materialize_named_parameters_for_save();
        let owner = scene.document.header.model_space_block_handle;
        assert!(
            scene.document.xrecord(owner, XRECORD_KEY).is_none(),
            "an empty table should not create an XRecord"
        );
    }

    #[test]
    fn decode_rejects_a_mismatched_format_version() {
        let mut bytes = vec![FORMAT_VERSION.wrapping_add(1)];
        bytes.extend(bincode::serialize(&ParameterTable::new()).unwrap());
        assert!(
            decode(&bytes).is_none(),
            "a future/unknown format version must be rejected, not misread"
        );
    }
}
