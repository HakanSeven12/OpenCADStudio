//! A frame of a layout draws every viewport it was built with: the wires each
//! one points at are still there when the frame is drawn, however many wire
//! sets the layout shows, and they are not made again for the next frame.

use std::sync::Arc;

use document::entities::{Face3D, Line, Viewport, ViewportRenderMode};
use document::types::Vector3;
use document::EntityType;

use crate::scene::view::render::Primitive;
use crate::scene::Scene;

/// Viewports in a row of the sheet.
const COLUMNS: u32 = 10;

fn line(y: f64) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(0.0, y, 0.0),
        Vector3::new(10.0, y, 0.0),
    ))
}

fn face(y: f64) -> EntityType {
    EntityType::Face3D(Face3D::new(
        Vector3::new(0.0, y, 0.0),
        Vector3::new(10.0, y, 0.0),
        Vector3::new(10.0, y + 5.0, 0.0),
        Vector3::new(0.0, y + 5.0, 0.0),
    ))
}

/// What the drawing holds besides a line in the model and one on the sheet.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Extra {
    Nothing,
    FaceOnTheSheet,
    FaceInTheModel,
    PlotStyleShown,
}

/// A layout of `count` viewports, each at a scale of its own (1:1, 1:2, ...),
/// so each one draws from a wire set of its own.
fn layout_of(count: u32, extra: Extra) -> Scene {
    let mut scene = Scene::new();
    scene.add_entity(line(0.0));
    if extra == Extra::FaceInTheModel {
        scene.add_entity(face(2.0));
    }
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());
    if extra == Extra::PlotStyleShown {
        let mut settings = scene.effective_plot_settings().expect("layout settings");
        settings.current_style_sheet = crate::io::plot_style::MONOCHROME_PLOT_STYLE.into();
        settings.flags.show_plot_styles = true;
        assert!(scene.set_layout_plot_settings("Sheet", &settings));
    }
    scene.add_entity(line(-28.0));
    if extra == Extra::FaceOnTheSheet {
        scene.add_entity(face(-27.0));
    }
    for index in 0..count {
        let ratio = f64::from(index + 1);
        scene.add_scale(&format!("1:{}", index + 1), 1.0, ratio);
        let mut viewport = Viewport::new();
        viewport.center = Vector3::new(
            50.0 * f64::from(index % COLUMNS),
            50.0 * f64::from(index / COLUMNS),
            0.0,
        );
        viewport.width = 40.0;
        viewport.height = 40.0;
        viewport.view_target = Vector3::new(5.0, 2.0, 0.0);
        viewport.view_height = 40.0 * ratio;
        scene.add_entity(EntityType::Viewport(Box::new(viewport)));
    }
    scene
}

/// One frame of the sheet, with every viewport of `count` on the canvas.
fn frame(scene: &Scene, count: u32) -> Primitive {
    let rows = count.div_ceil(COLUMNS) as f32;
    let (width, height) = (50.0 * COLUMNS as f32 + 20.0, 50.0 * rows + 20.0);
    scene.camera.borrow_mut().fit_to_bounds(
        glam::Vec3::new(-30.0, -30.0, 0.0),
        glam::Vec3::new(width - 30.0, height - 30.0, 0.0),
        width / height,
    );
    let canvas = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: 2.0 * width,
        height: 2.0 * height,
    };
    let primitive = scene.build_viewports(
        canvas,
        ViewportRenderMode::Wireframe2D,
        None,
        false,
        false,
        [1.0; 4],
    );
    assert_eq!(
        primitive.viewports.len(),
        count as usize + 1,
        "the sheet and its {count} viewports are on the canvas"
    );
    primitive
}

/// The viewports of `primitive` whose wires are gone, the sheet being 0.
fn without_wires(primitive: &Primitive) -> Vec<usize> {
    primitive
        .viewports
        .iter()
        .enumerate()
        .filter(|(_, viewport)| viewport.wires.upgrade().is_none())
        .map(|(index, _)| index)
        .collect()
}

#[test]
fn a_sheet_with_a_3d_face_keeps_its_wires_beside_twelve_viewports() {
    let scene = layout_of(12, Extra::FaceOnTheSheet);
    for _ in 0..2 {
        assert_eq!(without_wires(&frame(&scene, 12)), Vec::<usize>::new());
    }
}

#[test]
fn twelve_viewports_over_a_3d_face_keep_their_wires() {
    let scene = layout_of(12, Extra::FaceInTheModel);
    for _ in 0..2 {
        assert_eq!(without_wires(&frame(&scene, 12)), Vec::<usize>::new());
    }
}

#[test]
fn thirty_viewports_shown_with_a_plot_style_keep_their_wires() {
    let scene = layout_of(30, Extra::PlotStyleShown);
    for _ in 0..2 {
        assert_eq!(without_wires(&frame(&scene, 30)), Vec::<usize>::new());
    }
}

#[test]
fn a_few_viewports_keep_their_wires_too() {
    for extra in [
        Extra::Nothing,
        Extra::FaceOnTheSheet,
        Extra::FaceInTheModel,
        Extra::PlotStyleShown,
    ] {
        let scene = layout_of(3, extra);
        assert_eq!(
            without_wires(&frame(&scene, 3)),
            Vec::<usize>::new(),
            "{extra:?}"
        );
    }
}

#[test]
fn the_wires_of_a_layout_are_not_made_again_for_the_next_frame() {
    for (count, extra) in [
        (12, Extra::FaceOnTheSheet),
        (12, Extra::FaceInTheModel),
        (30, Extra::PlotStyleShown),
    ] {
        let scene = layout_of(count, extra);
        // The first frame fills the caches; the next two only read them.
        drop(frame(&scene, count));
        let first = frame(&scene, count);
        let second = frame(&scene, count);
        for (index, (a, b)) in first.viewports.iter().zip(&second.viewports).enumerate() {
            assert_eq!(
                a.wire_content_id, b.wire_content_id,
                "{extra:?}: viewport {index} is uploaded again"
            );
            let same_wires = match (a.wires.upgrade(), b.wires.upgrade()) {
                (Some(a), Some(b)) => Arc::ptr_eq(&a, &b),
                _ => false,
            };
            assert!(same_wires, "{extra:?}: the wires of viewport {index}");
            assert!(
                Arc::ptr_eq(&a.face3d_wires, &b.face3d_wires),
                "{extra:?}: the 3D faces of viewport {index}"
            );
            assert!(
                Arc::ptr_eq(&a.hatches, &b.hatches),
                "{extra:?}: the fills of viewport {index}"
            );
        }
    }
}

#[test]
fn a_frame_leaves_the_resident_sets_to_the_scene() {
    // An edit patches a resident set in place only while nothing else holds
    // it, so the frame must not: it holds the copies made for display.
    let scene = layout_of(3, Extra::Nothing);
    let primitive = frame(&scene, 3);
    for (index, viewport) in primitive.viewports.iter().enumerate().skip(1) {
        assert_eq!(
            viewport.wires.strong_count(),
            1,
            "viewport {index} draws the resident set itself"
        );
    }

    let mut model = Scene::new();
    model.add_entity(line(0.0));
    let primitive = model.build_viewports(
        iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        },
        ViewportRenderMode::Wireframe2D,
        None,
        false,
        false,
        [1.0; 4],
    );
    assert_eq!(primitive.viewports.len(), 1);
    assert_eq!(primitive.viewports[0].wires.strong_count(), 1);
}

#[test]
fn a_frame_built_after_an_edit_keeps_its_wires_and_old_copies_are_let_go() {
    for extra in [Extra::FaceOnTheSheet, Extra::FaceInTheModel] {
        let mut scene = layout_of(12, extra);
        for round in 0..6 {
            // A new line on the sheet and one in the model: every set changes,
            // and the copies of the sets drawn before are still cached.
            scene.add_entity(line(-26.0 + f64::from(round)));
            scene.set_current_layout("Model".to_string());
            scene.add_entity(line(1.0 + f64::from(round)));
            scene.set_current_layout("Sheet".to_string());
            assert_eq!(
                without_wires(&frame(&scene, 12)),
                Vec::<usize>::new(),
                "{extra:?}, edit {round}"
            );
        }
        let kept = scene.split_cache.borrow().len();
        assert!(
            kept <= 2 * 13,
            "{extra:?}: {kept} splits kept for the 13 sets the layout draws"
        );
    }
}
