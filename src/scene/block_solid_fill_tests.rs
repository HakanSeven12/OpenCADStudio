//! A planar SOLID is filled when the view looks along the direction it was
//! drawn with. For a solid inside a block that is its direction in the
//! block, however the reference of the block is placed.

use super::Scene;
use crate::scene::model::wire_model::WireModel;
use crate::scene::pipeline::face3d_gpu::planar_solid_faces_view;
use document::entities::{Insert, Solid};
use document::types::Vector3;
use document::EntityType;

/// A direction 2.9 degrees off +Z, as a reference placed in a slightly
/// turned coordinate system carries.
fn tilted() -> Vector3 {
    let tilt = 2.9_f64.to_radians();
    let side = tilt.sin() / std::f64::consts::SQRT_2;
    Vector3::new(-side, side, tilt.cos())
}

fn arrow(normal: Vector3) -> EntityType {
    let mut solid = Solid::triangle(
        Vector3::new(0.0, -1.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(3.0, 0.0, 0.0),
    );
    solid.normal = normal;
    EntityType::Solid(solid)
}

/// The fills of the planar solids a scene draws in model space.
fn solid_fills(scene: &mut Scene) -> Vec<WireModel> {
    let camera = scene.camera.borrow().clone();
    let wires = scene.model_tile_wires_arc(0, &camera, 1.0, 1.0);
    crate::scene::model::instance_model::expand_stubs(&wires)
        .iter()
        .filter(|wire| wire.fill_is_2d_solid && !wire.fill_tris.is_empty())
        .cloned()
        .collect()
}

/// Whether the one planar solid of the scene is filled in a view from above.
fn filled_from_above(scene: &mut Scene) -> bool {
    let fills = solid_fills(scene);
    assert_eq!(fills.len(), 1, "the scene draws one planar solid");
    planar_solid_faces_view(&fills[0], glam::Vec3::Z)
}

fn reference_of(solid_normal: Vector3, reference_normal: Vector3) -> Scene {
    let mut scene = Scene::new();
    scene.define_block_raw("ARROW", Vector3::ZERO, vec![arrow(solid_normal)]);
    let mut insert = Insert::new("ARROW", Vector3::ZERO);
    insert.normal = reference_normal;
    scene.add_entity(EntityType::Insert(insert));
    scene
}

/// A solid drawn flat in its block is filled from above, wherever the
/// reference of the block points.
#[test]
fn a_flat_solid_in_a_block_is_filled_under_a_tilted_reference() {
    let mut flat = reference_of(Vector3::UNIT_Z, Vector3::UNIT_Z);
    assert!(filled_from_above(&mut flat), "a flat reference");

    let mut scene = reference_of(Vector3::UNIT_Z, tilted());
    assert!(
        filled_from_above(&mut scene),
        "a flat solid in a block must be filled under a reference tilted 2.9 degrees"
    );
}

/// A solid of its own drawn off the view, and a solid drawn off the view in
/// its block, are outlines from above.
#[test]
fn a_solid_drawn_off_the_view_is_an_outline() {
    let mut scene = Scene::new();
    scene.add_entity(arrow(tilted()));
    assert!(!filled_from_above(&mut scene), "a tilted solid of its own");

    let mut scene = reference_of(tilted(), Vector3::UNIT_Z);
    assert!(
        !filled_from_above(&mut scene),
        "a tilted solid in a block under a flat reference"
    );
}
