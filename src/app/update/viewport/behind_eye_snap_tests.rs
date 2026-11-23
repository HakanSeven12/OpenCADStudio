use super::*;

use super::*;
use document::{entities::Solid3D, EntityType};

fn app_with_box(center_z: f64) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let base = crate::scene::model::solid_model::box_solid([0.0, 0.0, center_z], 10.0, 10.0, 5.0)
        .expect("kernel box");
    let placed = crate::scene::model::solid_model::placed(
        &base,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    )
    .expect("place box");
    let sat = crate::scene::convert::acis_export::solid_to_sat(&placed).expect("sat");
    let mut entity = Solid3D::new();
    entity.set_sat_document(&sat);
    entity.wires = crate::scene::model::solid_model::edge_wires(&placed);
    app.tabs[i].scene.add_entity(EntityType::Solid3D(entity));
    app
}

/// Face snaps project mesh triangles to screen space. A triangle behind a
/// perspective eye mirrors onto visible pixels, so without an eye-plane
/// guard a hidden face can win a snap right under the cursor (#64).
#[test]
fn behind_eye_solid_face_does_not_snap() {
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    let eye = glam::DVec3::ZERO;
    let view_rot = glam::camera::rh::proj::directx::perspective(
        45.0_f32.to_radians(),
        bounds.width / bounds.height,
        0.1,
        2000.0,
    );
    // Where the raw projection places a point: in front of the eye this is
    // its real pixel, behind it the mirrored pixel the bug would snap to.
    let pixel = |p: glam::DVec3| {
        let ndc = view_rot.project_point3((p - eye).as_vec3());
        Point::new(
            (ndc.x + 1.0) * 0.5 * bounds.width,
            (1.0 - ndc.y) * 0.5 * bounds.height,
        )
    };
    let snap = |app: &OpenCADStudio, cursor: Point| {
        app.tabs[app.active_tab]
            .scene
            .solid_face_snaps(cursor, view_rot, eye, bounds, 15.0, None, false, true)
    };

    // Control: a box in front of the eye snaps on its face.
    let front = app_with_box(-102.5);
    let hit = snap(&front, pixel(glam::DVec3::new(2.0, 3.0, -100.0)))
        .expect("a front solid face must snap");
    assert_eq!(hit.snap_type, crate::snap::SnapType::NearestFace);

    // The same box wholly behind the eye mirrors onto those pixels.
    let behind = app_with_box(102.5);
    assert!(
        snap(&behind, pixel(glam::DVec3::new(2.0, 3.0, 100.0))).is_none(),
        "a behind-eye solid face must not snap"
    );
}
