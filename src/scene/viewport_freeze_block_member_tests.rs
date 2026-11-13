//! A layer frozen in one layout viewport hides the lines a block draws on it
//! inside that viewport, the way it hides lines drawn straight on the layer.

use document::entities::{Insert, Line, Viewport};
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
    let mut scene = Scene::new();
    for name in ["REF", "GREEN"] {
        let mut layer = Layer::new(name);
        layer.handle = scene.document.allocate_handle();
        scene.document.layers.add(layer).unwrap();
    }
    let green = scene.document.layers.get("GREEN").unwrap().handle;
    // INNER: a line on GREEN and a line on layer 0, placed on REF.
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
    let mut viewport_with = |frozen: Vec<Handle>| {
        let mut viewport = Viewport::new();
        viewport.center = Vector3::new(50.0, 50.0, 0.0);
        viewport.width = 40.0;
        viewport.height = 40.0;
        viewport.view_target = Vector3::new(0.5, 0.5, 0.0);
        viewport.view_height = 4.0;
        viewport.frozen_layers = frozen;
        scene.add_entity(EntityType::Viewport(Box::new(viewport)))
    };
    let plain = viewport_with(Vec::new());
    let freezing = viewport_with(vec![green]);

    assert_eq!(layers_seen(&scene, plain), ["GREEN", "REF"]);
    assert_eq!(
        layers_seen(&scene, freezing),
        ["REF"],
        "the viewport froze GREEN; the block's line on layer 0 shows on REF"
    );
}
