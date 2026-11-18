//! The edge outline that highlights a selected solid placed by many block
//! references is the block's outline moved to each reference, not a copy of
//! every edge point per reference.

use super::Scene;
use crate::scene::WireModel;
use document::entities::{AcisData, Insert, Solid3D, ViewportRenderMode};
use document::types::Vector3;
use document::{EntityType, Handle};

const COLUMNS: usize = 20;
const ROWS: usize = 20;
const PITCH: f64 = 4.0;

fn box_solid() -> EntityType {
    let body = crate::scene::model::solid_model::box_solid([0.0; 3], 2.0, 1.0, 1.5).expect("box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&body).expect("sat");
    let mut solid = Solid3D::new();
    solid.acis_data = AcisData::from_sat(&sat.to_sat_string());
    EntityType::Solid3D(solid)
}

/// 400 references of one block that holds a box, away from the origin and at
/// three heights, the last `turned` of them turned a quarter, all selected.
fn selected_references(turned: usize) -> (Scene, Vec<Handle>) {
    let mut scene = Scene::new();
    scene.define_block_raw("BOX", Vector3::ZERO, vec![box_solid()]);
    let count = COLUMNS * ROWS;
    let references: Vec<Handle> = (0..count)
        .map(|index| {
            let mut insert = Insert::new(
                "BOX",
                Vector3::new(
                    (index % COLUMNS) as f64 * PITCH + 1000.0,
                    (index / COLUMNS) as f64 * PITCH - 500.0,
                    (index % 3) as f64,
                ),
            );
            if index >= count - turned {
                insert.rotation = std::f64::consts::FRAC_PI_2;
            }
            scene.add_entity(EntityType::Insert(insert))
        })
        .collect();
    scene.populate_meshes_from_document();
    scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    scene.fit_all();
    scene.select_entities(&references);
    (scene, references)
}

/// The edge points the outline of `handle` must show, in world space: the
/// block's edges placed by the reference.
fn placed_edges(scene: &Scene, handle: Handle) -> Vec<[f64; 3]> {
    let meshes = scene.interaction_meshes_arc();
    let set = meshes
        .iter()
        .find(|set| set.entity_handle() == Some(handle))
        .expect("the reference's solid");
    let transform = set.instance_transform.expect("a placed solid");
    let (edges, edges_low) = set.geometry_edges();
    edges
        .iter()
        .enumerate()
        .map(|(index, high)| {
            let low = edges_low.get(index).copied().unwrap_or([0.0; 3]);
            let world = transform.apply(Vector3::new(
                high[0] as f64 + low[0] as f64,
                high[1] as f64 + low[1] as f64,
                high[2] as f64 + low[2] as f64,
            ));
            [world.x, world.y, world.z]
        })
        .collect()
}

/// A wire's points in full precision, the breaks between edges left out,
/// moved by `delta`.
fn wire_points(wire: &WireModel, delta: [f64; 3]) -> Vec<[f64; 3]> {
    wire.points
        .iter()
        .enumerate()
        .filter(|(_, point)| point[0].is_finite())
        .map(|(index, point)| {
            let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
            [
                point[0] as f64 + low[0] as f64 + delta[0],
                point[1] as f64 + low[1] as f64 + delta[1],
                point[2] as f64 + low[2] as f64 + delta[2],
            ]
        })
        .collect()
}

fn assert_same_points(got: &[[f64; 3]], want: &[[f64; 3]], handle: Handle) {
    assert_eq!(got.len(), want.len(), "reference {handle:?}: edge points");
    for (got, want) in got.iter().zip(want) {
        for axis in 0..3 {
            assert!(
                (got[axis] - want[axis]).abs() < 1e-6,
                "reference {handle:?}: {got:?} is not {want:?}"
            );
        }
    }
}

fn outlines(scene: &Scene) -> Vec<WireModel> {
    let instance = scene
        .active_viewports(800.0, 600.0, ViewportRenderMode::Wireframe2D)
        .into_iter()
        .next()
        .expect("the model view");
    scene
        .annotation_context_highlight_wires(&instance, true)
        .iter()
        .filter(|wire| wire.name.starts_with("mesh-edge:"))
        .cloned()
        .collect()
}

fn handle_of(wire: &WireModel) -> Handle {
    Handle::new(wire.name["mesh-edge:".len()..].parse().expect("a handle"))
}

/// 400 selected references of a box: one outline each, all moving one shared
/// shape, and none carrying edge points of its own.
#[test]
fn moved_references_share_one_outline_shape() {
    let (scene, references) = selected_references(0);
    let outlines = outlines(&scene);
    assert_eq!(
        outlines.len(),
        references.len(),
        "one outline per reference"
    );
    let copied: usize = outlines.iter().map(|wire| wire.points.len()).sum();
    assert_eq!(
        copied,
        0,
        "the outlines copied {copied} edge points for {} references",
        references.len()
    );
    let shapes: std::collections::HashSet<usize> = outlines
        .iter()
        .map(|wire| {
            let stub = wire.stub.as_ref().expect("an outline that moves a shape");
            std::sync::Arc::as_ptr(&stub.prototype) as usize
        })
        .collect();
    assert_eq!(shapes.len(), 1, "every reference moves the same shape");
}

/// Each outline shows the edges where its reference places them: the shared
/// shape's points, moved by the outline's delta, are the reference's edges.
#[test]
fn a_moved_outline_lands_on_its_reference() {
    let (scene, _) = selected_references(0);
    for wire in outlines(&scene) {
        let stub = wire.stub.as_ref().expect("an outline that moves a shape");
        let shape = &stub.prototype[0];
        assert!(
            shape.render_instance.is_some(),
            "the shape is drawn as instances"
        );
        let handle = handle_of(&wire);
        assert_same_points(
            &wire_points(shape, stub.delta),
            &placed_edges(&scene, handle),
            handle,
        );
    }
}

/// A reference that turns its block keeps an outline of its own placed
/// points, as before.
#[test]
fn a_turned_reference_keeps_its_own_outline() {
    let (scene, references) = selected_references(3);
    let turned_handles = &references[references.len() - 3..];
    let turned: Vec<WireModel> = outlines(&scene)
        .into_iter()
        .filter(|wire| turned_handles.contains(&handle_of(wire)))
        .collect();
    assert_eq!(turned.len(), 3);
    for wire in &turned {
        assert!(
            wire.stub.is_none(),
            "a turned reference is not a moved copy"
        );
        let handle = handle_of(wire);
        assert_same_points(
            &wire_points(wire, [0.0; 3]),
            &placed_edges(&scene, handle),
            handle,
        );
    }
}
