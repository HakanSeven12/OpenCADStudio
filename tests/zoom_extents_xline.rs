// Regression for #284: ZOOM Extents must ignore the ±1e6 display segments
// that XLine/Ray tessellate into, instead of fitting the camera to them.
use codec::entities::{Line, XLine};
use codec::types::Vector3;
use codec::EntityType;
use OpenCADStudio::scene::Scene;

fn add_rect(scene: &mut Scene) {
    let corners = [
        (0.0, 0.0, 10.0, 0.0),
        (10.0, 0.0, 10.0, 10.0),
        (10.0, 10.0, 0.0, 10.0),
        (0.0, 10.0, 0.0, 0.0),
    ];
    for (x1, y1, x2, y2) in corners {
        let mut line = Line::new();
        line.start = Vector3::new(x1, y1, 0.0);
        line.end = Vector3::new(x2, y2, 0.0);
        scene.add_entity(EntityType::Line(line));
    }
}

#[test]
fn fit_all_ignores_xline_display_extent() {
    let mut scene = Scene::new();
    add_rect(&mut scene);

    // Construction line through the rectangle centre at 45° — its centroid
    // sits inside the drawing cluster, and in a fresh document
    // `local_extent_max` is still the 1e9 default, so before the fix both
    // outlier rejects passed its ±1e6 display endpoints into the bounds.
    let xl = XLine::new(Vector3::new(5.0, 5.0, 0.0), Vector3::new(1.0, 1.0, 0.0));
    scene.add_entity(EntityType::XLine(xl));

    scene.fit_all();

    let cam = scene.camera.borrow();
    assert!(
        cam.distance.is_finite() && cam.distance < 1000.0,
        "camera must fit the 10x10 rectangle, not the xline's 1e6 display \
         segment (distance = {})",
        cam.distance
    );
    assert!(
        (cam.target.x - 5.0).abs() < 5.0 && (cam.target.y - 5.0).abs() < 5.0,
        "camera target must stay on the rectangle (target = {:?})",
        cam.target
    );
}

#[test]
fn fit_all_with_only_xline_fits_base_point() {
    let mut scene = Scene::new();
    let xl = XLine::new(Vector3::new(100.0, 200.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
    scene.add_entity(EntityType::XLine(xl));

    scene.fit_all();

    let cam = scene.camera.borrow();
    assert!(
        cam.distance.is_finite() && cam.distance < 1000.0,
        "xline-only drawing must fit near the base point, not ±1e6 \
         (distance = {})",
        cam.distance
    );
    assert!(
        (cam.target.x - 100.0).abs() < 5.0 && (cam.target.y - 200.0).abs() < 5.0,
        "camera target must be the xline base point (target = {:?})",
        cam.target
    );
}
