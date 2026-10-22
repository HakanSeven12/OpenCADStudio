// A large vegetation DWG can contain thousands of copies of one hatch-filled
// block. Hit-testing must reuse each leaf's tessellated boundary instead of
// expanding and tessellating the same definition for every translated INSERT.
use std::sync::Arc;

use codec::entities::hatch::{BoundaryEdge, BoundaryPath, LineEdge};
use codec::entities::{Hatch, Insert};
use codec::types::{Transform, Vector2, Vector3};
use codec::EntityType;
use OpenCADStudio::scene::Scene;

#[test]
fn translated_insert_hatch_pick_models_share_boundary_storage() {
    let mut scene = Scene::new();
    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    let mut boundary = BoundaryPath::new();
    for ((x0, y0), (x1, y1)) in [
        ((0.0, 0.0), (10.0, 0.0)),
        ((10.0, 0.0), (10.0, 10.0)),
        ((10.0, 10.0), (0.0, 10.0)),
        ((0.0, 10.0), (0.0, 0.0)),
    ] {
        boundary.edges.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(x0, y0),
            end: Vector2::new(x1, y1),
        }));
    }
    hatch.paths.push(boundary);

    let original_hatch = scene.add_entity(EntityType::Hatch(Box::new(hatch)));
    let id = Transform::identity();
    let first = scene
        .create_block_from_entities(&[original_hatch], "PICK_CACHE_PLANT", &id, &id)
        .expect("create first block insertion");
    let second = scene.add_entity(EntityType::Insert(Insert::new(
        "PICK_CACHE_PLANT",
        Vector3::new(40.0, 0.0, 0.0),
    )));

    let picks = scene.insert_hatches_for_click();
    let first_fill = picks.get(&first).and_then(|fills| fills.first())
        .expect("first INSERT should contain a visible hatch");
    let second_fill = picks.get(&second).and_then(|fills| fills.first())
        .expect("second INSERT should contain a visible hatch");

    assert!(Arc::ptr_eq(&first_fill.boundary, &second_fill.boundary),
        "translated INSERTs should share their leaf hatch polygon storage");
    assert!((second_fill.world_origin[0] - first_fill.world_origin[0] - 40.0).abs() < 1e-5,
        "translated INSERT must keep its own world-space anchor");
    assert!((second_fill.world_origin[1] - first_fill.world_origin[1]).abs() < 1e-5);
}
