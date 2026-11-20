use super::*;

use document::entities::{
    CenterLineAssociation, CenterLineSource, CenterLineSourceKind, EntityType, Line,
};
use document::types::Vector3;

fn line(y: f64) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(0.0, y, 0.0),
        Vector3::new(100.0, y, 0.0),
    ))
}

fn associated(first: Handle, second: Handle) -> EntityType {
    let EntityType::Line(mut carrier) = line(5.0) else {
        unreachable!()
    };
    CenterLineAssociation {
        first: CenterLineSource {
            handle: first,
            kind: CenterLineSourceKind::Line,
            segment_index: -1,
            pick_point: Vector3::new(50.0, 0.0, 0.0),
        },
        second: CenterLineSource {
            handle: second,
            kind: CenterLineSourceKind::Line,
            segment_index: -1,
            pick_point: Vector3::new(50.0, 10.0, 0.0),
        },
        plane_origin: Vector3::new(0.0, 0.0, 0.0),
        plane_x: Vector3::new(1.0, 0.0, 0.0),
        plane_y: Vector3::new(0.0, 1.0, 0.0),
        start_extension: 0.0,
        end_extension: 0.0,
        start_length_adjustment: 0.0,
        end_length_adjustment: 0.0,
        associated: true,
    }
    .write(&mut carrier.common.extended_data);
    EntityType::Line(carrier)
}

fn last_changes(scene: &Scene) -> Vec<(Handle, ChangeKind)> {
    scene
        .geometry_deltas
        .borrow()
        .back()
        .expect("a geometry delta")
        .changes
        .clone()
}

#[test]
fn refreshed_carriers_are_appended_once_and_already_named_handles_are_skipped() {
    let mut scene = Scene::new();
    let first = scene.add_entity(line(0.0));
    let second = scene.add_entity(line(10.0));
    let carrier = scene.add_entity(associated(first, second));

    scene.bump_entities(&[
        (first, ChangeKind::Modified),
        (second, ChangeKind::Modified),
    ]);
    assert_eq!(
        last_changes(&scene),
        vec![
            (first, ChangeKind::Modified),
            (second, ChangeKind::Modified),
            (carrier, ChangeKind::Modified),
        ],
        "the re-solved carrier must be appended once, after the edit's own handles"
    );

    // The edit already names the carrier (the user moved the centre line
    // itself): the refresh must not append it a second time.
    scene.bump_entities(&[
        (carrier, ChangeKind::Modified),
        (first, ChangeKind::Modified),
    ]);
    assert_eq!(
        last_changes(&scene),
        vec![
            (carrier, ChangeKind::Modified),
            (first, ChangeKind::Modified),
        ],
    );
}
