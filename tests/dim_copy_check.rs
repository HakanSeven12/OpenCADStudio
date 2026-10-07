// Regression for #161: COPY must duplicate a dimension's baked block so the
// copy renders at the copy position, not on top of the original.
use codec::entities::{Dimension, DimensionLinear, Line};
use codec::tables::BlockRecord;
use codec::types::Vector3;
use codec::{EntityType, Handle};
use glam::DVec3;
use OpenCADStudio::command::EntityTransform;
use OpenCADStudio::scene::Scene;

#[test]
fn copy_dimension_duplicates_its_block() {
    let mut scene = Scene::new();

    // A baked *D0 block holding one line at (0,0)-(10,0).
    let br_h = Handle::new(scene.document.next_handle());
    let mut br = BlockRecord::new("*D0");
    br.handle = br_h;
    scene.document.block_records.add(br).unwrap();

    let mut sub = Line::new();
    sub.start = Vector3::new(0.0, 0.0, 0.0);
    sub.end = Vector3::new(10.0, 0.0, 0.0);
    let mut sub_e = EntityType::Line(sub);
    sub_e.common_mut().owner_handle = br_h; // route into *D0
    scene.document.add_entity(sub_e).unwrap();

    // A linear dimension whose drawn geometry is that baked block.
    let mut dim = DimensionLinear::new(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
    );
    dim.base.block_name = "*D0".to_string();
    let dim_h = scene.add_entity(EntityType::Dimension(Dimension::Linear(dim)));

    // Copy the dimension by (0, 50).
    let copies = scene.copy_entities(
        &[dim_h],
        &EntityTransform::Translate(DVec3::new(0.0, 50.0, 0.0)),
    );
    assert_eq!(copies.len(), 1);
    let copy_h = copies[0];

    // The copy must reference its OWN block, not the source's.
    let copy_block = match scene.document.get_entity(copy_h) {
        Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
        _ => panic!("copy is not a dimension"),
    };
    assert_ne!(copy_block, "*D0", "copy must get its own block");
    assert!(!copy_block.trim().is_empty(), "copy block name must be set");

    // The copy block's sub-line must be translated by (0, 50): (0,50)-(10,50).
    let new_br = scene
        .document
        .block_records
        .iter()
        .find(|b| b.name == copy_block)
        .expect("copy block record exists");
    let subh = *new_br
        .entity_handles
        .first()
        .expect("copy block has a sub-entity");
    match scene.document.get_entity(subh) {
        Some(EntityType::Line(l)) => assert!(
            (l.start.y - 50.0).abs() < 1e-6 && (l.end.y - 50.0).abs() < 1e-6,
            "copy block sub-line not translated: {:?}-{:?}",
            (l.start.x, l.start.y),
            (l.end.x, l.end.y)
        ),
        other => panic!("copy block sub is not a line: {:?}", other.map(std::mem::discriminant)),
    }
}
