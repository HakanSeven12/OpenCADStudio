//! `UCS FACE` turns a pick on a solid into a drawing plane. The plane is only
//! as good as the face lookup underneath it, so this covers that half: given a
//! body and a point on one of its faces, does the right face come back, and is
//! its outward normal the one a sketch should be built on?
//!
//! The axis construction on top of that normal is unit-tested beside
//! `ucs_from_normal` in `src/app/helpers.rs`, which is `pub(crate)` and so out
//! of reach from here.

use codec::{entities::Circle, types::Vector3, EntityType};
use OpenCADStudio::scene::model::{presspull_model, solid_model};

/// Extrude a disc into a puck 10 units tall, centred on the origin.
fn puck() -> kernel::brep::Body {
    let entity = EntityType::Circle(Circle::from_center_radius(
        Vector3::new(0.0, 0.0, 0.0),
        100.0,
    ));
    let body = presspull_model::extrusion_body(&entity, [0.0, 0.0, 10.0])
        .expect("a closed circular profile extrudes");
    assert!(body.validate().is_empty(), "extrusion produced a valid body");
    body
}

/// The two flat ends point away from the material, in opposite directions.
/// Getting this wrong is what would make a sketch land inside the solid, or
/// extrude back into it.
#[test]
fn opposite_faces_report_opposite_outward_normals() {
    let body = puck();

    let top = solid_model::nearest_planar_face(&body, [0.0, 0.0, 10.0])
        .expect("the top cap is planar");
    let bottom = solid_model::nearest_planar_face(&body, [0.0, 0.0, 0.0])
        .expect("the bottom cap is planar");
    assert_ne!(top, bottom, "the caps are distinct faces");

    let top_normal = solid_model::planar_face_normal(&body, top).expect("top cap has a normal");
    let bottom_normal =
        solid_model::planar_face_normal(&body, bottom).expect("bottom cap has a normal");

    assert!(
        (top_normal[2] - 1.0).abs() < 1e-9,
        "top cap should point +Z, got {top_normal:?}"
    );
    assert!(
        (bottom_normal[2] + 1.0).abs() < 1e-9,
        "bottom cap should point -Z, got {bottom_normal:?}"
    );
}

/// A pick anywhere on a cap resolves to that cap, not merely to the one
/// nearest the origin. A sketch is started by clicking somewhere on a face,
/// rarely at its centre.
#[test]
fn an_off_centre_pick_still_lands_on_its_own_face() {
    let body = puck();
    let centre = solid_model::nearest_planar_face(&body, [0.0, 0.0, 10.0]).unwrap();
    let off_centre = solid_model::nearest_planar_face(&body, [60.0, -25.0, 10.0]).unwrap();
    assert_eq!(centre, off_centre, "both picks are on the top cap");
}

/// The curved side is not a plane, so it cannot become a sketch plane. The
/// lookup has to decline rather than hand back a nonsense normal.
#[test]
fn the_curved_side_is_not_offered_as_a_plane() {
    let body = puck();
    // Halfway up the barrel, on its surface.
    let face = solid_model::nearest_planar_face(&body, [100.0, 0.0, 5.0]);
    if let Some(face) = face {
        // A planar cap may still be the nearest planar face; what must never
        // happen is the barrel itself being reported as planar.
        let normal = solid_model::planar_face_normal(&body, face)
            .expect("whatever came back claims to be planar, so it has a normal");
        assert!(
            normal[2].abs() > 1e-9,
            "a cap normal has Z; a barrel wall wrongly called planar would not"
        );
    }
}

#[test]
fn ucs_pick_command_enables_solid_face_picking() {
    use OpenCADStudio::command::{CadCommand, CmdResult, UcsPickCommand};
    use glam::DVec3;

    let cmd_face = UcsPickCommand { face: true };
    assert!(cmd_face.needs_entity_pick(), "UCS FACE needs entity pick");
    assert!(cmd_face.entity_pick_includes_fills(), "UCS FACE must include fills to raycast solid faces");
    assert!(cmd_face.entity_pick_uses_surface_point(), "UCS FACE must use 3D surface point");
    assert!(cmd_face.entity_pick_highlights_hover(), "UCS FACE must highlight hovered solid face");

    let cmd_obj = UcsPickCommand { face: false };
    assert!(cmd_obj.needs_entity_pick());
    assert!(!cmd_obj.entity_pick_includes_fills(), "UCS OBJECT operates on curves, not fills");
    assert!(!cmd_obj.entity_pick_uses_surface_point());

    let handle = codec::Handle::new(0x2A);
    let pt = DVec3::new(10.5, 20.25, 30.125);
    let mut cmd = cmd_face;
    match cmd.on_entity_pick(handle, pt) {
        CmdResult::Dispatch(cmd_str) => {
            assert_eq!(cmd_str, "UCS FACE 2A 10.5,20.25,30.125");
        }
        _ => panic!("Expected CmdResult::Dispatch"),
    }
}

#[test]
fn face_ucs_snaps_origin_and_aligns_axes_on_box() {
    let box_body = solid_model::box_solid([0.0, 0.0, 0.0], 10.0, 20.0, 30.0)
        .expect("box_solid constructs successfully");

    // Top face is at Z = +15, bounds X in [-5, 5], Y in [-10, 10].
    // Pick near corner (5, 10, 15), closer to the edge along -X than the edge along -Y.
    let face = solid_model::nearest_planar_face(&box_body, [3.0, 9.0, 15.0])
        .expect("top face found");
    let ucs = solid_model::planar_face_ucs(&box_body, face, [3.0, 9.0, 15.0])
        .expect("planar_face_ucs constructed");

    // Origin must snap to the nearest corner (5, 10, 15) instead of the pick coordinate.
    assert!((ucs.origin.x - 5.0).abs() < 1e-6);
    assert!((ucs.origin.y - 10.0).abs() < 1e-6);
    assert!((ucs.origin.z - 15.0).abs() < 1e-6);

    // Z axis must be outward normal (0, 0, 1).
    let z = glam::DVec3::new(ucs.x_axis.x, ucs.x_axis.y, ucs.x_axis.z)
        .cross(glam::DVec3::new(ucs.y_axis.x, ucs.y_axis.y, ucs.y_axis.z));
    assert!((z.z - 1.0).abs() < 1e-6);

    // Closer to edge along -X: X axis must point along (-1, 0, 0).
    assert!((ucs.x_axis.x - (-1.0)).abs() < 1e-6);
    assert!(ucs.x_axis.y.abs() < 1e-6);
    assert!(ucs.x_axis.z.abs() < 1e-6);

    // Y axis = Z x X = (0, 0, 1) x (-1, 0, 0) = (0, -1, 0).
    assert!(ucs.y_axis.x.abs() < 1e-6);
    assert!((ucs.y_axis.y - (-1.0)).abs() < 1e-6);
    assert!(ucs.y_axis.z.abs() < 1e-6);

    // Now pick near the same corner (5, 10, 15), but closer to the edge along -Y.
    let ucs_y_edge = solid_model::planar_face_ucs(&box_body, face, [4.5, 7.0, 15.0])
        .expect("planar_face_ucs constructed");

    // Origin is still the same corner (5, 10, 15).
    assert!((ucs_y_edge.origin.x - 5.0).abs() < 1e-6);
    assert!((ucs_y_edge.origin.y - 10.0).abs() < 1e-6);
    assert!((ucs_y_edge.origin.z - 15.0).abs() < 1e-6);

    // Closer to edge along -Y: X axis must point along (0, -1, 0).
    assert!(ucs_y_edge.x_axis.x.abs() < 1e-6);
    assert!((ucs_y_edge.x_axis.y - (-1.0)).abs() < 1e-6);
    assert!(ucs_y_edge.x_axis.z.abs() < 1e-6);

    // Y axis = Z x X = (0, 0, 1) x (0, -1, 0) = (1, 0, 0).
    assert!((ucs_y_edge.y_axis.x - 1.0).abs() < 1e-6);
    assert!(ucs_y_edge.y_axis.y.abs() < 1e-6);
    assert!(ucs_y_edge.y_axis.z.abs() < 1e-6);
}

#[test]
fn face_ucs_on_puck() {
    let body = puck();
    let top_face = solid_model::nearest_planar_face(&body, [60.0, -25.0, 10.0]).unwrap();
    let ucs = solid_model::planar_face_ucs(&body, top_face, [60.0, -25.0, 10.0]).unwrap();

    // Normal Z = (0, 0, 1).
    let z = glam::DVec3::new(ucs.x_axis.x, ucs.x_axis.y, ucs.x_axis.z)
        .cross(glam::DVec3::new(ucs.y_axis.x, ucs.y_axis.y, ucs.y_axis.z));
    assert!((z.z - 1.0).abs() < 1e-6);

    // Orthonormality of X and Y.
    let x = glam::DVec3::new(ucs.x_axis.x, ucs.x_axis.y, ucs.x_axis.z);
    let y = glam::DVec3::new(ucs.y_axis.x, ucs.y_axis.y, ucs.y_axis.z);
    assert!((x.length() - 1.0).abs() < 1e-6);
    assert!((y.length() - 1.0).abs() < 1e-6);
    assert!(x.dot(y).abs() < 1e-6);
}
