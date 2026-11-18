//! A layer frozen in one layout viewport hides the lines a block draws on it
//! inside that viewport, the way it hides lines drawn straight on the layer.

use std::sync::Arc;

use document::entities::{Insert, Line, Viewport, ViewportRenderMode};
use document::tables::Layer;
use document::types::Vector3;
use document::{EntityType, Handle};

use crate::scene::model::instance_model::expand_stubs;
use crate::scene::Scene;

fn line_on(layer: &str, y: f64) -> EntityType {
    let mut line = Line::from_points(Vector3::new(0.0, y, 0.0), Vector3::new(1.0, y, 0.0));
    line.common.layer = layer.to_string();
    EntityType::Line(line)
}

/// A layout over a model that holds one block reference on REF, whose
/// definition draws a line on GREEN and a line on layer 0. Returns GREEN.
fn sheet_over_a_block_with_a_line_on_green() -> (Scene, Handle) {
    let mut scene = Scene::new();
    for name in ["REF", "GREEN"] {
        let mut layer = Layer::new(name);
        layer.handle = scene.document.allocate_handle();
        scene.document.layers.add(layer).unwrap();
    }
    let green = scene.document.layers.get("GREEN").unwrap().handle;
    scene
        .define_block_from_owned_entities(
            vec![line_on("GREEN", 0.0), line_on("0", 1.0)],
            "INNER",
            glam::DVec3::ZERO,
        )
        .expect("the definition is created");
    let mut placed = Insert::new("INNER", Vector3::ZERO);
    placed.common.layer = "REF".into();
    scene.add_entity(EntityType::Insert(placed));

    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());
    (scene, green)
}

/// A 40 x 40 viewport centred on `x` that shows `view_height` of the model
/// and freezes `frozen`.
fn viewport_at(scene: &mut Scene, x: f64, view_height: f64, frozen: Vec<Handle>) -> Handle {
    let mut viewport = Viewport::new();
    viewport.center = Vector3::new(x, 0.0, 0.0);
    viewport.width = 40.0;
    viewport.height = 40.0;
    viewport.view_target = Vector3::new(0.5, 0.5, 0.0);
    viewport.view_height = view_height;
    viewport.frozen_layers = frozen;
    scene.add_entity(EntityType::Viewport(Box::new(viewport)))
}

/// The layers the lines seen through `viewport` are drawn on.
fn layers_seen(scene: &Scene, viewport: Handle) -> Vec<String> {
    let wires = scene.model_wires_for_viewport_arc(viewport, 200.0);
    let mut layers: Vec<String> = expand_stubs(&wires)
        .iter()
        .filter_map(|wire| wire.effective_layer.as_deref().map(str::to_string))
        .collect();
    layers.sort();
    layers.dedup();
    layers
}

#[test]
fn a_viewport_freezing_a_layer_hides_the_block_lines_on_it() {
    let (mut scene, green) = sheet_over_a_block_with_a_line_on_green();
    let plain = viewport_at(&mut scene, -30.0, 4.0, Vec::new());
    let freezing = viewport_at(&mut scene, 30.0, 4.0, vec![green]);

    assert_eq!(layers_seen(&scene, plain), ["GREEN", "REF"]);
    assert_eq!(
        layers_seen(&scene, freezing),
        ["REF"],
        "the viewport froze GREEN; the block's line on layer 0 shows on REF"
    );
}

/// Two viewports that freeze GREEN and show the model at 1:1 and at 1:10,
/// so each draws from a wire set of its own.
fn two_scales_freezing_green() -> (Scene, Handle, Handle) {
    let (mut scene, green) = sheet_over_a_block_with_a_line_on_green();
    scene.add_scale("1:1", 1.0, 1.0);
    scene.add_scale("1:10", 1.0, 10.0);
    let near = viewport_at(&mut scene, -30.0, 40.0, vec![green]);
    let far = viewport_at(&mut scene, 30.0, 400.0, vec![green]);
    (scene, near, far)
}

#[test]
fn viewports_freezing_the_same_layers_keep_a_wire_set_each() {
    let (scene, near, far) = two_scales_freezing_green();

    let near_wires = scene.model_wires_for_viewport_arc(near, 200.0);
    let far_wires = scene.model_wires_for_viewport_arc(far, 200.0);
    assert!(
        !Arc::ptr_eq(&near_wires, &far_wires),
        "the two scales draw from different sets"
    );
    assert_eq!(layers_seen(&scene, near), ["REF"]);
    assert_eq!(layers_seen(&scene, far), ["REF"]);

    // A frame keeps a viewport's set only weakly from the moment it is built
    // until it is drawn: the scene has to keep it alive meanwhile.
    let near_held = Arc::downgrade(&near_wires);
    let far_held = Arc::downgrade(&far_wires);
    drop((near_wires, far_wires));
    assert!(
        near_held.upgrade().is_some(),
        "the second viewport's set replaced the first one's"
    );
    assert!(far_held.upgrade().is_some());

    // And neither is made again while the model stays as it is.
    for (viewport, held) in [(near, &near_held), (far, &far_held), (near, &near_held)] {
        let again = scene.model_wires_for_viewport_arc(viewport, 200.0);
        assert!(
            held.upgrade()
                .is_some_and(|first| Arc::ptr_eq(&first, &again)),
            "the set is reused from one frame to the next"
        );
    }
}

#[test]
fn a_frame_reaches_the_wires_of_every_viewport_freezing_the_same_layers() {
    let (scene, _, _) = two_scales_freezing_green();
    // The sheet view holds both viewports.
    scene.camera.borrow_mut().fit_to_bounds(
        glam::Vec3::new(-60.0, -30.0, 0.0),
        glam::Vec3::new(60.0, 30.0, 0.0),
        2.0,
    );
    let canvas = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 400.0,
    };
    for frame in 0..2 {
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
            3,
            "the sheet and the two viewports"
        );
        for (index, viewport) in primitive.viewports.iter().enumerate() {
            assert!(
                viewport.wires.upgrade().is_some(),
                "frame {frame}: viewport {index} lost its wires before it was drawn"
            );
        }
    }
}

#[test]
fn the_results_made_from_replaced_sets_are_let_go() {
    let (mut scene, near, far) = two_scales_freezing_green();
    for y in 0..8 {
        scene.set_current_layout("Model".to_string());
        scene.add_entity(line_on("REF", 2.0 + f64::from(y)));
        scene.set_current_layout("Sheet".to_string());
        assert_eq!(layers_seen(&scene, near), ["REF"]);
        assert_eq!(layers_seen(&scene, far), ["REF"]);
    }
    let kept = scene.viewport_frozen_member_cache.borrow().len();
    assert!(
        kept <= 2,
        "{kept} results kept for the two sets the viewports draw from now"
    );
}

#[test]
fn freezing_a_layer_no_block_line_is_on_takes_no_hold_on_the_set() {
    let (mut scene, _) = sheet_over_a_block_with_a_line_on_green();
    let mut layer = Layer::new("EMPTY");
    layer.handle = scene.document.allocate_handle();
    scene.document.layers.add(layer).unwrap();
    let empty = scene.document.layers.get("EMPTY").unwrap().handle;
    let viewport = viewport_at(&mut scene, 0.0, 40.0, vec![empty]);

    // Nothing to leave out: the viewport draws the resident set itself, and
    // an edit patches that set in place only while the scene alone holds it.
    let wires = scene.model_wires_for_viewport_arc(viewport, 200.0);
    assert_eq!(layers_seen(&scene, viewport), ["GREEN", "REF"]);
    assert_eq!(
        Arc::strong_count(&wires),
        2,
        "this handle and the scene's own"
    );
}
