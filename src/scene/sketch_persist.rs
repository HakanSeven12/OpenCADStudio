//! Save/load integration for [`super::sketch_constraints::SketchConstraintSet`]
//! — design doc §5.2/§8 stage 4, the "lazy" persistence model that section
//! recommends: constraint sets live only in `Scene::sketch_constraints`
//! in-memory (§8 stage 3) and are materialized into `document.objects` as
//! XRecords only around a save, and read back out right after a load. The
//! XRecord round-trip mechanism itself (`CadDocument::ensure_xrecord`/
//! `xrecord`/`xrecord_mut`) was proven in stage 2
//! (`tests/sketch_constraints_xrecord_roundtrip.rs`) — this module is just
//! the app-level wiring: which owner handles to check, and when.
//!
//! One named XRecord (keyed [`XRECORD_KEY`]) hangs off each scope's own
//! owner handle's extension dictionary — model space's `BlockRecord` handle,
//! or a `SketchScope::Block`'s handle — rather than a single shared
//! dictionary, since [`super::sketch_constraints::SketchScope::owner_handle`]
//! already gives that handle directly and `ensure_xrecord`/`xrecord` are
//! themselves per-owner.

use super::sketch_constraints::SketchConstraintSet;
use super::Scene;
use acadrust::{CadDocument, Handle};

const XRECORD_KEY: &str = "OCS_SKETCH_CONSTRAINTS";

/// The vendored DWG writer's `XRecordValue::Chunk` encoder packs a single
/// `u8` length prefix per entry and silently truncates anything longer
/// (`cadcodec`'s `io/dwg/dwg_stream_writers/object_writer/objects.rs`,
/// `encode_xrecord_entries`) — unlike the DXF writer, which has no such
/// limit. A constraint set past a handful of entries serializes past 255
/// bytes easily, so a single `XRecordEntry` silently loses the tail and
/// `bincode::deserialize` then fails on the truncated bytes, dropping the
/// *entire* scope's constraints on the next DWG load — reproduced and
/// root-caused live (not from a change log): a 5-entry set round-tripped
/// clean, a 6-entry set of the same shapes lost everything. Splitting the
/// blob across as many same-code `310` entries as needed (below) works
/// around it without touching the vendored crate — `CadDocument::xrecord`'s
/// `entries: Vec<XRecordEntry>` already supports repeating a code.
const MAX_CHUNK_BYTES: usize = u8::MAX as usize;

/// Removes a named XRecord from `owner`'s extension dictionary, if present.
/// The dictionary itself is left in place even if now empty — other code may
/// already reference it via `xdictionary_handle`, and an empty extension
/// dictionary is harmless bookkeeping, unlike a stale XRecord that would
/// resurrect deleted data on the next load.
fn remove_xrecord(document: &mut CadDocument, owner: Handle, key: &str) {
    let Some(dictionary_handle) = document.extension_dictionary_handle(owner) else {
        return;
    };
    let Some(acadrust::objects::ObjectType::Dictionary(dictionary)) =
        document.objects.get_mut(&dictionary_handle)
    else {
        return;
    };
    let Some(index) = dictionary.entries.iter().position(|(name, _)| name.eq_ignore_ascii_case(key)) else {
        return;
    };
    let (_, record_handle) = dictionary.entries.remove(index);
    document.objects.remove(&record_handle);
}

/// Prefixed onto every serialized blob so a future schema change can be
/// detected and gracefully skipped (dropping just that one scope's
/// constraints, not the whole load) rather than silently misreading bytes
/// or panicking — design doc §5.2's explicit call-out, and §7's flagged
/// "no migration path designed yet": bumping this is the signal a real
/// migration is needed, not a substitute for one.
///
/// Bumped 1 -> 2 by `named_parameters_design.md` stage 3:
/// `SketchConstraint::driving_param` changed from a bare `Option<f64>` to
/// `Option<named_parameters::DrivingValue>` (an enum, with its own
/// discriminant), so a version-1 blob's bytes no longer line up with the
/// current struct shape — exactly the "detected and gracefully skipped"
/// case this version byte exists for, not something to migrate (no
/// migration path exists yet, per the call-out above).
const FORMAT_VERSION: u8 = 2;

fn encode(set: &SketchConstraintSet) -> Option<Vec<u8>> {
    let mut bytes = vec![FORMAT_VERSION];
    bytes.extend(bincode::serialize(set).ok()?);
    Some(bytes)
}

fn decode(bytes: &[u8]) -> Option<SketchConstraintSet> {
    let (&version, body) = bytes.split_first()?;
    if version != FORMAT_VERSION {
        return None;
    }
    bincode::deserialize(body).ok()
}

impl Scene {
    /// Writes every current `sketch_constraints` entry into its scope's
    /// owner-handle XRecord, ready for whatever save call happens next to
    /// serialize `self.document` as-is. Called right before a native/web
    /// save (`OpenCADStudio::prepare_native_save` and the wasm save path)
    /// — see those call sites' doc comments for why "right before save"
    /// rather than keeping the XRecord resident and live-updated (design
    /// doc §5.2's "live" model, deferred: it would also let constraint-set
    /// edits ride the ordinary object-delta undo path per §5.1(b), but this
    /// lazy model was the doc's own recommended first step).
    ///
    /// A scope with an empty constraint set writes no XRecord — an
    /// unconstrained drawing (the common case) should not gain
    /// persisted-but-empty bookkeeping on every save. If a *previous* save
    /// already materialized a non-empty XRecord for that scope, the now-empty
    /// set actively removes it instead: leaving the stale blob in place would
    /// resurrect the deleted constraints the next time the document loads.
    pub(crate) fn materialize_sketch_constraints_for_save(&mut self) {
        for index in 0..self.sketch_constraints.len() {
            let set = &self.sketch_constraints[index];
            if set.constraints.is_empty() {
                let owner = set.scope.owner_handle(&self.document);
                if !owner.is_null() {
                    remove_xrecord(&mut self.document, owner, XRECORD_KEY);
                }
                continue;
            }
            let owner = set.scope.owner_handle(&self.document);
            if owner.is_null() {
                continue;
            }
            let Some(bytes) = encode(set) else { continue };
            self.document.ensure_xrecord(owner, XRECORD_KEY);
            if let Some(record) = self.document.xrecord_mut(owner, XRECORD_KEY) {
                // Overwrite, not append: a resave must replace the prior
                // blob, not accumulate more Chunk entries every time.
                record.entries.clear();
                for chunk in bytes.chunks(MAX_CHUNK_BYTES) {
                    record.entries.push(acadrust::objects::XRecordEntry::new(
                        310,
                        acadrust::objects::XRecordValue::Chunk(chunk.to_vec()),
                    ));
                }
            }
        }
    }

    /// Populates `sketch_constraints` from every `OCS_SKETCH_CONSTRAINTS`
    /// XRecord found in the just-loaded `self.document` — called right
    /// after a document open installs its `CadDocument` into this `Scene`
    /// (`OpenCADStudio::on_file_opened` and the automation `"open"`/`"new"`
    /// ops). Every `BlockRecord` (model space, paper space, and named
    /// block definitions) is a candidate owner; most will have no such
    /// XRecord and are skipped cheaply. Replaces whatever was already in
    /// `sketch_constraints` (a fresh/empty `Vec` for a real open; `"new"`'s
    /// blank document has nothing to find anyway).
    pub(crate) fn load_sketch_constraints_from_document(&mut self) {
        self.sketch_constraints.clear();
        let owners: Vec<acadrust::Handle> =
            self.document.block_records.iter().map(|record| record.handle).collect();
        for owner in owners {
            let Some(record) = self.document.xrecord(owner, XRECORD_KEY) else { continue };
            // Concatenate every Chunk entry in order, not just the first —
            // a set materialized past `MAX_CHUNK_BYTES` spans more than one
            // same-code `310` entry (see that constant's doc comment).
            let mut bytes = Vec::new();
            for entry in &record.entries {
                if let acadrust::objects::XRecordValue::Chunk(chunk) = &entry.value {
                    bytes.extend_from_slice(chunk);
                }
            }
            if bytes.is_empty() {
                continue;
            }
            if let Some(set) = decode(&bytes) {
                self.sketch_constraints.push(set);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::sketch_constraints::{ConstraintKind, SketchRef, SketchScope};
    use super::*;
    use acadrust::entities::EntityType;
    use acadrust::types::Vector3;

    #[test]
    fn a_model_space_constraint_set_survives_materialize_and_reload() {
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
        scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
            ConstraintKind::Horizontal,
            vec![SketchRef::whole(a)],
            None,
        );

        scene.materialize_sketch_constraints_for_save();
        // Simulate the load-time side of a save/reload round trip: drop the
        // in-memory sets (a fresh open starts with none) and repopulate
        // purely from what materialize just wrote into `document.objects`.
        scene.sketch_constraints.clear();
        scene.load_sketch_constraints_from_document();

        let set = scene.sketch_constraint_set(SketchScope::ModelSpace).expect("constraint set should have been reloaded");
        assert_eq!(set.constraints.len(), 1);
        assert_eq!(set.constraints[0].kind, ConstraintKind::Horizontal);
        assert_eq!(set.constraints[0].refs, vec![SketchRef::whole(a)]);
    }

    /// The above test only proves `materialize`/`load` agree with each
    /// other on the same in-memory `CadDocument` — it never actually
    /// serializes anything. This drives a real `save_to_bytes`/`load_bytes`
    /// round trip (the same primitives `OpenCADStudio::on_file_opened`'s
    /// native save/open path uses), through both supported formats, closing
    /// the gap stage 2's spike proved for the raw XRecord mechanism up to
    /// this stage's actual app-level wiring.
    fn full_bytes_roundtrip(ext: &str) -> SketchConstraintSet {
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 5.0, 0.0),
        )));
        let b = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(10.0, 5.0, 0.0),
            Vector3::new(20.0, 5.0, 0.0),
        )));
        scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
            ConstraintKind::Coincident,
            vec![SketchRef::point(a, 1), SketchRef::point(b, 0)],
            None,
        );
        scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
            ConstraintKind::Distance,
            vec![SketchRef::whole(b)],
            Some(crate::scene::named_parameters::DrivingValue::Literal(12.5)),
        );

        scene.materialize_sketch_constraints_for_save();
        let bytes = crate::io::save_to_bytes(&scene.document, ext, scene.document.version)
            .unwrap_or_else(|e| panic!("save to {ext} bytes: {e}"));
        let mut reloaded_scene = Scene::new();
        reloaded_scene.document = crate::io::load_bytes(&format!("stage4_roundtrip.{ext}"), bytes)
            .unwrap_or_else(|e| panic!("reload {ext} bytes: {e}"));
        reloaded_scene.load_sketch_constraints_from_document();

        reloaded_scene.sketch_constraint_set(SketchScope::ModelSpace).cloned().unwrap_or_else(|| {
            panic!("no ModelSpace constraint set survived the {ext} save/load round trip")
        })
    }

    #[test]
    fn a_constraint_set_survives_a_real_dxf_save_and_load_round_trip() {
        let restored = full_bytes_roundtrip("dxf");
        assert_eq!(restored.constraints.len(), 2);
        assert_eq!(restored.constraints[0].kind, ConstraintKind::Coincident);
        assert_eq!(restored.constraints[1].kind, ConstraintKind::Distance);
        assert_eq!(restored.constraints[1].driving_param, Some(crate::scene::named_parameters::DrivingValue::Literal(12.5)));
    }

    #[test]
    fn a_constraint_set_survives_a_real_dwg_save_and_load_round_trip() {
        let restored = full_bytes_roundtrip("dwg");
        assert_eq!(restored.constraints.len(), 2);
        assert_eq!(restored.constraints[0].kind, ConstraintKind::Coincident);
        assert_eq!(restored.constraints[1].kind, ConstraintKind::Distance);
        assert_eq!(restored.constraints[1].driving_param, Some(crate::scene::named_parameters::DrivingValue::Literal(12.5)));
    }

    #[test]
    fn a_block_scoped_constraint_set_survives_materialize_and_reload() {
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )));
        // `create_block_from_entities` returns the placed INSERT's handle,
        // not the BlockRecord's own handle — `SketchScope::Block` needs the
        // latter (it's what `BlockEditSession::br_handle` and
        // `SketchScope::owner_handle` both mean by "a block definition's
        // handle"), so look it up from the table by name instead of using
        // the return value directly.
        let _insert_handle = scene
            .create_block_from_entities(&[a], "OCS_STAGE4_TEST_BLOCK", &acadrust::types::Transform::identity(), &acadrust::types::Transform::identity())
            .expect("create a block definition to own the constraint set");
        let br_handle = scene.document.block_records.get("OCS_STAGE4_TEST_BLOCK").expect("block record should exist").handle;
        scene.sketch_constraint_set_mut(SketchScope::Block(br_handle)).add(ConstraintKind::Vertical, vec![SketchRef::whole(a)], None);

        scene.materialize_sketch_constraints_for_save();
        scene.sketch_constraints.clear();
        scene.load_sketch_constraints_from_document();

        let set = scene
            .sketch_constraint_set(SketchScope::Block(br_handle))
            .expect("block-scoped constraint set should have been reloaded");
        assert_eq!(set.constraints.len(), 1);
        assert_eq!(set.constraints[0].kind, ConstraintKind::Vertical);
    }

    #[test]
    fn an_empty_constraint_set_is_not_materialized() {
        // A scope touched (e.g. `sketch_constraint_set_mut` called once,
        // then every constraint removed) but left with zero constraints
        // should not persist an empty XRecord.
        let mut scene = Scene::new();
        let _ = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
        scene.materialize_sketch_constraints_for_save();

        let owner = scene.document.header.model_space_block_handle;
        assert!(scene.document.xrecord(owner, XRECORD_KEY).is_none(), "an empty constraint set should not create an XRecord");
    }

    #[test]
    fn clearing_a_previously_materialized_set_removes_its_stale_xrecord() {
        // Regression for the audit-flagged case `an_empty_constraint_set_is_not_materialized`
        // doesn't cover: a scope that already had a *non-empty* XRecord from a
        // prior save must have that XRecord actively removed once its
        // in-memory set goes back to empty — otherwise the stale blob
        // survives the save and resurrects the deleted constraints on the
        // next load.
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
        scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Horizontal, vec![SketchRef::whole(a)], None);
        scene.materialize_sketch_constraints_for_save();

        let owner = scene.document.header.model_space_block_handle;
        assert!(scene.document.xrecord(owner, XRECORD_KEY).is_some(), "sanity: the first save should have materialized an XRecord");

        // Clear every constraint (mirrors deleting the constrained entity, or
        // removing the last constraint by hand) and save again.
        scene.sketch_constraint_set_mut(SketchScope::ModelSpace).constraints.clear();
        scene.materialize_sketch_constraints_for_save();

        assert!(scene.document.xrecord(owner, XRECORD_KEY).is_none(), "the stale XRecord from the earlier non-empty save must be removed, not left behind");

        // And the load path must agree: nothing resurrects on reload.
        scene.sketch_constraints.clear();
        scene.load_sketch_constraints_from_document();
        assert!(scene.sketch_constraint_set(SketchScope::ModelSpace).is_none(), "no constraints should come back after the set was cleared and re-saved");
    }

    #[test]
    fn decode_rejects_a_mismatched_format_version() {
        let mut bytes = vec![FORMAT_VERSION.wrapping_add(1)];
        bytes.extend(bincode::serialize(&SketchConstraintSet::new(SketchScope::ModelSpace)).unwrap());
        assert!(decode(&bytes).is_none(), "a future/unknown format version must be rejected, not misread");
    }
}
