// Tests of how the scene keeps center marks with their circles
// (`entities::centermark`).
use std::collections::HashSet;

use document::entities::{
    CenterMarkAssociation, CenterMarkSource, CenterMarkSourceKind, Circle, Line,
};
use document::types::Vector3;
use document::{Document, EntityType, Handle};

use crate::scene::{ChangeKind, Scene};

fn source_circle() -> EntityType {
    EntityType::Circle(Circle::from_center_radius(
        Vector3::new(10.0, 10.0, 0.0),
        5.0,
    ))
}

fn carrier(source: Handle) -> EntityType {
    let association = CenterMarkAssociation {
        source: CenterMarkSource {
            handle: source,
            kind: CenterMarkSourceKind::Circle,
            segment_index: -1,
            pick_point: Vector3::new(10.0, 10.0, 0.0),
        },
        plane_origin: Vector3::new(10.0, 10.0, 0.0),
        plane_x: Vector3::new(1.0, 0.0, 0.0),
        plane_y: Vector3::new(0.0, 1.0, 0.0),
        center: Vector3::new(10.0, 10.0, 0.0),
        radius: 5.0,
        cross_size: 1.0,
        cross_gap: 0.5,
        cross_size_relative: false,
        cross_gap_relative: false,
        extension_length: 0.0,
        length_adjustments: [0.0; 4],
        overshoots: [0.0; 4],
        show_extensions: false,
        associated: true,
    };
    let mut line = Line::from_points(Vector3::new(9.5, 10.0, 0.0), Vector3::new(10.5, 10.0, 0.0));
    association.write(&mut line.common.extended_data);
    EntityType::Line(line)
}

fn carrier_y(scene: &Scene, handle: Handle) -> f64 {
    let Some(EntityType::Line(line)) = scene.document.get_entity(handle) else {
        panic!("carrier is not a line");
    };
    assert!(
        (line.start.y - line.end.y).abs() < 1e-9,
        "carrier not horizontal"
    );
    line.start.y
}

#[test]
fn index_finds_marks_when_their_circle_moves() {
    let mut scene = Scene::new();
    let source = scene.add_entity(source_circle());
    let mark = scene.add_entity(carrier(source));
    assert!((carrier_y(&scene, mark) - 10.0).abs() < 1e-9);

    let Some(EntityType::Circle(circle)) = scene.document.get_entity_mut(source) else {
        panic!("source is not a circle");
    };
    circle.center.y = 20.0;
    scene.bump_entities(&[(source, ChangeKind::Modified)]);
    assert!((carrier_y(&scene, mark) - 20.0).abs() < 1e-9);
}

/// Paste inserts straight into the document and bumps `Added`; a mark
/// arriving that way must reach a warm index too.
#[test]
fn index_picks_up_a_mark_added_after_it_was_built() {
    let mut scene = Scene::new();
    let source = scene.add_entity(source_circle());
    let early = scene.add_entity(carrier(source));

    let Some(EntityType::Circle(circle)) = scene.document.get_entity_mut(source) else {
        panic!("source is not a circle");
    };
    circle.center.y = 12.0;
    scene.bump_entities(&[(source, ChangeKind::Modified)]);
    assert!(
        (carrier_y(&scene, early) - 12.0).abs() < 1e-9,
        "index is warm"
    );

    let pasted = scene
        .document
        .add_entity(carrier(source))
        .unwrap_or(Handle::NULL);
    assert!(!pasted.is_null());
    scene.bump_entities(&[(pasted, ChangeKind::Added)]);

    let Some(EntityType::Circle(circle)) = scene.document.get_entity_mut(source) else {
        panic!("source is not a circle");
    };
    circle.center.y = 20.0;
    scene.bump_entities(&[(source, ChangeKind::Modified)]);
    assert!((carrier_y(&scene, pasted) - 20.0).abs() < 1e-9);
    assert!((carrier_y(&scene, early) - 20.0).abs() < 1e-9);
}

/// The cross is the part of a mark a user snaps to, so the wire that
/// carries it must keep the mark's key vertices and segment tangents.
#[test]
fn a_marks_cross_wire_keeps_the_snaps_and_vertices() {
    let mut document = Document::new();
    let source = document.add_entity(source_circle()).unwrap_or(Handle::NULL);
    let entity = carrier(source);
    let wires = crate::scene::tessellate_entity(
        &document,
        &HashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
        None,
        &entity,
        None,
        None,
        None,
        None,
        &HashSet::default(),
    );
    let cross = wires.last().expect("the mark draws a cross wire");
    assert!(
        cross.key_vertices.contains(&[10.0, 10.0, 0.0]),
        "the centre stays a snap vertex: {:?}",
        cross.key_vertices
    );
    assert!(
        !cross.tangent_geoms.is_empty(),
        "each cross segment keeps its tangent"
    );
}
