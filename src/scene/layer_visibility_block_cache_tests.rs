//! Switching layers off one after another (LAYOFF picks) rebuilds only the
//! block definitions that hold objects on the switched layer, every time.

use std::sync::Arc;

use document::entities::{Insert, Line};
use document::tables::Layer;
use document::types::Vector3;
use document::EntityType;

use crate::scene::Scene;

fn line_on(layer: &str) -> EntityType {
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    line.common.layer = layer.to_string();
    EntityType::Line(line)
}

/// Blocks `AA` (a line on layer A) and `BB` (a line on layer B), each
/// inserted once in model space.
fn two_blocks() -> Scene {
    let mut scene = Scene::new();
    for name in ["A", "B"] {
        scene.document.layers.add(Layer::new(name)).unwrap();
    }
    for (block, layer) in [("AA", "A"), ("BB", "B")] {
        scene
            .define_block_from_owned_entities(vec![line_on(layer)], block, glam::DVec3::ZERO)
            .expect("the definition is created");
        scene.add_entity(EntityType::Insert(Insert::new(block, Vector3::ZERO)));
    }
    scene
}

fn turn_off(scene: &mut Scene, layer: &str) {
    scene
        .document
        .layers
        .get_mut(layer)
        .expect("the layer exists")
        .turn_off();
    scene.invalidate_layer_visibility(&[layer.to_string()]);
}

fn block_cache(scene: &Scene) -> Arc<crate::scene::cache::block_cache::BlockCache> {
    scene.tessellation_context().3
}

#[test]
fn each_layer_switched_off_rebuilds_only_its_own_definitions() {
    let mut scene = two_blocks();
    let start = block_cache(&scene);

    turn_off(&mut scene, "A");
    let after_a = block_cache(&scene);
    assert!(
        Arc::ptr_eq(start.defn("BB").unwrap(), after_a.defn("BB").unwrap()),
        "BB has nothing on layer A and is kept"
    );
    assert!(
        !Arc::ptr_eq(start.defn("AA").unwrap(), after_a.defn("AA").unwrap()),
        "AA holds the line on layer A and is rebuilt"
    );

    // A second switch, after the cache was rebuilt for the first one.
    turn_off(&mut scene, "B");
    let after_b = block_cache(&scene);
    assert!(
        Arc::ptr_eq(after_a.defn("AA").unwrap(), after_b.defn("AA").unwrap()),
        "AA has nothing on layer B and is kept"
    );
    assert!(
        !Arc::ptr_eq(after_a.defn("BB").unwrap(), after_b.defn("BB").unwrap()),
        "BB holds the line on layer B and is rebuilt"
    );
}
