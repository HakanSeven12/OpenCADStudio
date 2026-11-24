//! The data key of a frame: the draw guard draws a frame whose draw panicked
//! again only when the scene gives it new data, not at each frame or pan.

use document::entities::{Line, Viewport, ViewportRenderMode};
use document::types::Vector3;
use document::EntityType;

use crate::scene::Scene;

fn line(y: f64) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(0.0, y, 0.0),
        Vector3::new(10.0, y, 0.0),
    ))
}

fn key(scene: &Scene) -> u64 {
    let canvas = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };
    scene
        .build_viewports(
            canvas,
            ViewportRenderMode::Wireframe2D,
            None,
            false,
            false,
            [1.0; 4],
        )
        .content_key()
}

#[test]
fn the_next_frame_of_the_same_scene_has_the_same_key() {
    let mut scene = Scene::new();
    scene.add_entity(line(0.0));
    assert_eq!(key(&scene), key(&scene));
}

#[test]
fn a_pan_keeps_the_key() {
    let mut scene = Scene::new();
    scene.add_entity(line(0.0));
    let before = key(&scene);
    scene.camera.borrow_mut().pan_screen(40.0, 10.0, 600.0);
    assert_eq!(key(&scene), before);
}

#[test]
fn an_added_entity_changes_the_key() {
    let mut scene = Scene::new();
    scene.add_entity(line(0.0));
    let before = key(&scene);
    scene.add_entity(line(5.0));
    assert_ne!(key(&scene), before);
}

#[test]
fn the_next_frame_of_a_layout_with_a_viewport_has_the_same_key() {
    let mut scene = Scene::new();
    scene.add_entity(line(0.0));
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());
    let mut viewport = Viewport::new();
    viewport.width = 40.0;
    viewport.height = 40.0;
    viewport.view_height = 40.0;
    scene.add_entity(EntityType::Viewport(Box::new(viewport)));
    assert_eq!(key(&scene), key(&scene));
}
