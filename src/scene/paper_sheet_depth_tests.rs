use super::*;
use document::entities::Line;
use document::types::Vector3;

/// Clip-space depth of `point` through `camera`: inside the view volume when
/// it lies in `0.0..=1.0`.
fn clip_depth(camera: &view::camera::Camera, point: glam::DVec3) -> f32 {
    let bounds = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1200.0,
        height: 800.0,
    };
    let relative = (point - camera.eye()).as_vec3();
    let clip = camera.view_proj_rte(bounds) * relative.extend(1.0);
    clip.z / clip.w
}

/// A layout opened on its saved sheet view keeps in depth what the sheet
/// holds far off its plane. The model is flat (its extents reach Z = 10),
/// while the layout's own saved extents reach Z = -6040, where a legend was
/// placed; paper space is seen straight down, so its Z must not hide it.
#[test]
fn a_layout_opened_on_its_saved_view_keeps_paper_objects_off_the_plane() {
    let mut scene = Scene::new();
    scene.document.header.model_space_extents_min = Vector3::new(0.0, 0.0, 0.0);
    scene.document.header.model_space_extents_max = Vector3::new(100.0, 100.0, 10.0);
    scene.document.add_layout("Sheet").expect("unique layout");
    for object in scene.document.objects.values_mut() {
        if let ObjectType::Layout(layout) = object {
            if layout.name == "Sheet" {
                layout.min_limits = (0.0, 0.0);
                layout.max_limits = (420.0, 297.0);
                layout.min_extents = (0.0, 0.0, -6040.0);
                layout.max_extents = (420.0, 297.0, 0.0);
            }
        }
    }
    scene.set_current_layout("Sheet".to_string());
    let mut legend = Line::from_points(
        Vector3::new(300.0, 250.0, -6040.0),
        Vector3::new(380.0, 250.0, -6040.0),
    );
    legend.common.layer = "0".into();
    scene.add_entity(EntityType::Line(legend));

    scene.restore_saved_camera();

    let camera = scene.camera.borrow().clone();
    for point in [
        glam::DVec3::new(300.0, 250.0, -6040.0),
        glam::DVec3::new(380.0, 250.0, -6040.0),
        glam::DVec3::new(210.0, 150.0, 0.0),
    ] {
        let depth = clip_depth(&camera, point);
        assert!(
            (0.0..=1.0).contains(&depth),
            "the sheet point {point} is clipped at depth {depth}",
        );
    }
}
