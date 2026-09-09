// Stage 2 of docs/parametric_system_design.md's staged plan: prove a
// SketchConstraintSet, serialized into an XRecord Chunk entry hung off a
// block record's extension dictionary, survives a real save/reload through
// both supported file formats byte-for-byte. Everything after this stage
// (save/load integration, the "lazy" persistence model) is gated on this
// test passing — see the design doc §8 stage 2 and §7 open question 1.

use OpenCADStudio::scene::named_parameters::DrivingValue;
use OpenCADStudio::scene::sketch_constraints::{ConstraintKind, SketchConstraintSet, SketchRef, SketchScope};
use OpenCADStudio::scene::Scene;
use acadrust::entities::EntityType;
use acadrust::objects::{XRecordEntry, XRecordValue};
use acadrust::types::{Handle, Vector3};

const RECORD_KEY: &str = "OCS_SKETCH_CONSTRAINTS_TEST";

fn sample_set(scope: SketchScope) -> SketchConstraintSet {
    let mut set = SketchConstraintSet::new(scope);
    set.add(
        ConstraintKind::Coincident,
        vec![SketchRef::point(Handle::new(101), 0), SketchRef::point(Handle::new(102), 1)],
        None,
    );
    set.add(ConstraintKind::Distance, vec![SketchRef::whole(Handle::new(103))], Some(DrivingValue::Literal(25.0)));
    set.add(
        ConstraintKind::Angle,
        vec![SketchRef::whole(Handle::new(104)), SketchRef::whole(Handle::new(105))],
        Some(DrivingValue::Literal(45.0)),
    );
    set.add(ConstraintKind::Radius, vec![SketchRef::whole(Handle::new(106))], Some(DrivingValue::Literal(3.5)));
    set
}

/// Writes `set` into a fresh XRecord Chunk under `owner`'s extension
/// dictionary, round-trips the whole document through `ext` (`"dxf"` or
/// anything else for DWG), and returns the `SketchConstraintSet` read back
/// out of the reloaded document.
fn roundtrip_through(scene: &Scene, owner: Handle, set: &SketchConstraintSet, ext: &str) -> SketchConstraintSet {
    let mut doc = scene.document.clone();
    let bytes = bincode::serialize(set).expect("serialize SketchConstraintSet");

    doc.ensure_xrecord(owner, RECORD_KEY);
    let record = doc.xrecord_mut(owner, RECORD_KEY).expect("xrecord_mut after ensure_xrecord");
    record.entries.push(XRecordEntry::new(310, XRecordValue::Chunk(bytes)));

    let saved = OpenCADStudio::io::save_to_bytes(&doc, ext, doc.version)
        .unwrap_or_else(|e| panic!("save to {ext} bytes: {e}"));
    let reloaded = OpenCADStudio::io::load_bytes(&format!("roundtrip.{ext}"), saved)
        .unwrap_or_else(|e| panic!("reload {ext} bytes: {e}"));

    let record = reloaded
        .xrecord(owner, RECORD_KEY)
        .unwrap_or_else(|| panic!("XRecord {RECORD_KEY:?} missing after {ext} round-trip"));
    let chunk = record
        .entries
        .iter()
        .find_map(|entry| match &entry.value {
            XRecordValue::Chunk(bytes) => Some(bytes.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no Chunk entry survived the {ext} round-trip"));

    bincode::deserialize::<SketchConstraintSet>(&chunk).unwrap_or_else(|e| panic!("deserialize round-tripped {ext} blob: {e}"))
}

#[test]
fn sketch_constraint_set_survives_a_dxf_roundtrip() {
    let scene = Scene::new();
    let owner = scene.document.header.model_space_block_handle;
    assert!(!owner.is_null(), "a fresh document must already have a model-space block handle");
    let original = sample_set(SketchScope::ModelSpace);

    let restored = roundtrip_through(&scene, owner, &original, "dxf");

    assert_eq!(restored.scope, original.scope);
    assert_eq!(restored.constraints.len(), original.constraints.len());
    for (r, o) in restored.constraints.iter().zip(original.constraints.iter()) {
        assert_eq!(r.id, o.id);
        assert_eq!(r.kind, o.kind);
        assert_eq!(r.refs, o.refs);
        assert_eq!(r.driving_param, o.driving_param);
        assert_eq!(r.enabled, o.enabled);
    }
}

#[test]
fn sketch_constraint_set_survives_a_dwg_roundtrip() {
    let scene = Scene::new();
    let owner = scene.document.header.model_space_block_handle;
    let original = sample_set(SketchScope::ModelSpace);

    let restored = roundtrip_through(&scene, owner, &original, "dwg");

    assert_eq!(restored.scope, original.scope);
    assert_eq!(restored.constraints.len(), original.constraints.len());
    for (r, o) in restored.constraints.iter().zip(original.constraints.iter()) {
        assert_eq!(r.id, o.id);
        assert_eq!(r.kind, o.kind);
        assert_eq!(r.refs, o.refs);
        assert_eq!(r.driving_param, o.driving_param);
    }
}

/// A block-record-scoped set (not model space) — confirms the XRecord
/// extension-dictionary mechanism works for an arbitrary owner handle, not
/// just the one every document already has.
#[test]
fn sketch_constraint_set_survives_roundtrip_when_owned_by_a_block_record() {
    let mut scene = Scene::new();
    // A real block definition via the app's own `create_block_from_entities`
    // (needs at least one source entity) — a bare, entity-less BlockRecord
    // table entry turned out not to survive a DXF round-trip at all (the
    // writer prunes it as unreferenced), so this exercises the actual shape
    // `SketchScope::Block` will see in practice.
    let line_handle = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )));
    let br_handle = scene
        .create_block_from_entities(
            &[line_handle],
            "OCS_TEST_BLOCK",
            &acadrust::types::Transform::identity(),
            &acadrust::types::Transform::identity(),
        )
        .expect("create a block definition to own the constraint set");

    let original = sample_set(SketchScope::Block(br_handle));
    let restored = roundtrip_through(&scene, br_handle, &original, "dxf");

    assert_eq!(restored.scope, SketchScope::Block(br_handle));
    assert_eq!(restored.constraints.len(), original.constraints.len());
}
