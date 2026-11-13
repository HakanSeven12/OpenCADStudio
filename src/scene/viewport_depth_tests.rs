use super::*;
use document::entities::{Line, Viewport};
use document::types::Vector3;

/// Clip-space depth of `point` through `camera`: inside the view volume
/// when it lies in `0.0..=1.0`.
fn clip_depth(camera: &view::camera::Camera, point: glam::DVec3) -> f32 {
    let bounds = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: 750.0,
        height: 584.0,
    };
    let relative = (point - camera.eye()).as_vec3();
    let clip = camera.view_proj_rte(bounds) * relative.extend(1.0);
    clip.z / clip.w
}

/// A layout viewport whose view target sits 808.49 units above the drawing
/// plane, with a top view and no front or back clipping, as detail
/// viewports placed on a sheet can carry. The view looks straight down, so
/// how high the target sits must not hide the drawing below it.
#[test]
fn a_viewport_target_above_the_drawing_keeps_the_drawing_in_depth() {
    let mut scene = Scene::new();
    let mut line = Line::new();
    line.start = Vector3::new(995.0, 1996.0, 0.0);
    line.end = Vector3::new(1005.0, 2004.0, 0.0);
    scene.add_entity(EntityType::Line(line));

    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());

    // The two view heights of one row of detail viewports: 13.0043 kept
    // the drawing, 11.6811 lost it.
    for view_height in [13.0043, 11.6811] {
        let mut viewport = Viewport::new();
        viewport.center = Vector3::new(100.0, 60.0, 0.0);
        viewport.width = 75.0;
        viewport.height = view_height * 5.0;
        viewport.view_direction = Vector3::new(0.0, 0.0, 1.0);
        viewport.view_target = Vector3::new(600.0, 400.0, 808.4866);
        viewport.view_center = Vector3::new(400.0, 1600.0, 0.0);
        viewport.view_height = view_height;
        viewport.custom_scale = 5.0;
        let handle = scene.add_entity(EntityType::Viewport(Box::new(viewport)));

        let camera = scene
            .camera_for_viewport(handle)
            .expect("the viewport decodes to a camera");
        assert!(
            (camera.target.x - 1000.0).abs() < 1e-6 && (camera.target.y - 2000.0).abs() < 1e-6,
            "view centre {:?}",
            camera.target
        );
        for point in [
            glam::DVec3::new(995.0, 1996.0, 0.0),
            glam::DVec3::new(1005.0, 2004.0, 0.0),
        ] {
            let depth = clip_depth(&camera, point);
            assert!(
                (0.0..=1.0).contains(&depth),
                "view height {view_height}: the line end {point} is clipped at depth {depth}",
            );
        }
    }
}
