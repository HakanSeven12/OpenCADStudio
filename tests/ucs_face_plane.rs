//! `UCS FACE` turns a pick on a solid into a drawing plane. The plane is only
//! as good as the face lookup underneath it, so this covers that half: given a
//! body and a point on one of its faces, does the right face come back, and is
//! its outward normal the one a sketch should be built on?
//!
//! The axis construction on top of that normal is unit-tested beside
//! `ucs_from_normal` in `src/app/helpers.rs`, which is `pub(crate)` and so out
//! of reach from here.

use acadrust::{entities::Circle, types::Vector3, EntityType};
use OpenCADStudio::scene::model::{presspull_model, solid_model};

/// Extrude a disc into a puck 10 units tall, centred on the origin.
fn puck() -> cadkernel::brep::Body {
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
