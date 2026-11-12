//! Benchmark measuring interaction candidate overlay gather and remap
//! on a large drawing with 100,000 entities after a single-entity edit.
//!
//! Run: `cargo bench --bench large_overlay_interaction`

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use document::entities::Line;
use document::types::Vector3;
use document::EntityType;
use iced::Rectangle;

use OpenCADStudio::scene::pick::interaction_index::InteractionIndex;
use OpenCADStudio::scene::Scene;

const ENTITY_COUNT: usize = 100_000;
const SPACING: f64 = 1.0;
const RUNS: usize = 5;
const ITERATIONS: usize = 50;
const WARMUP: usize = 5;

const BOUNDS: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 1920.0,
    height: 1080.0,
};

fn main() {
    println!("large_overlay_interaction bench (entities={ENTITY_COUNT})");
    let mut scene = Scene::new();
    let mut handles = Vec::with_capacity(ENTITY_COUNT);
    for i in 0..ENTITY_COUNT {
        let x = (i % 500) as f64 * SPACING;
        let y = (i / 500) as f64 * SPACING;
        handles.push(scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(x, y, 0.0),
            Vector3::new(x + 0.8, y + 0.8, 0.0),
        ))));
    }

    let wires = scene.hit_test_wires();
    let key = scene
        .interaction_index_build_key(&wires, BOUNDS.height)
        .expect("interaction_index_build_key");
    let index = Arc::new(InteractionIndex::build(&wires));
    assert!(scene.install_prepared_interaction_index(
        key.0,
        key.1,
        Arc::downgrade(&wires),
        Arc::clone(&index),
    ));
    drop(wires);

    // Single edit: the epoch advances, base index is preserved, overlay is engaged
    let edited = handles[ENTITY_COUNT / 2];
    let mut moved = Line::from_points(Vector3::new(250.0, 100.0, 0.0), Vector3::new(250.9, 100.9, 0.0));
    moved.common.handle = edited;
    assert!(scene.update_entity(EntityType::Line(moved)));

    // Wide query box covering a substantial slice of the scene (~10,000 lines)
    let query_box = [100.0, 50.0, 400.0, 150.0];

    let mut sink = 0usize;
    for _ in 0..WARMUP {
        let c = scene.interaction_candidates_xy(black_box(query_box));
        sink = sink.wrapping_add(c.len());
    }

    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let c = scene.interaction_candidates_xy(black_box(query_box));
            sink = sink.wrapping_add(c.len());
        }
        samples.push(start.elapsed().as_nanos() as u64 / ITERATIONS as u64);
    }

    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let min = samples[0];
    let max = samples[samples.len() - 1];
    println!(
        "{:<28} median={:.3} ms  min={:.3} ms  max={:.3} ms  (sink={sink})",
        "large_overlay_query",
        median as f64 / 1_000_000.0,
        min as f64 / 1_000_000.0,
        max as f64 / 1_000_000.0,
    );
}
