//! Benchmark for full-scene wire rebuild (`wires-build`) on a large drawing
//! with block definitions (exercising the `block-index` visibility pass).
//!
//! Run: `cargo bench --bench wires_build_visible`

use std::hint::black_box;
use std::time::Instant;

use document::entities::{Insert, Line};
use document::types::Vector3;
use document::EntityType;
use OpenCADStudio::scene::Scene;

const MODEL_LINES: usize = 200_000;
const BLOCK_DEFS: usize = 50;
const ENTITIES_PER_BLOCK: usize = 20;
const RUNS: usize = 5;
const WARMUP: usize = 1;

fn build_scene() -> Scene {
    let mut scene = Scene::new();
    for b in 0..BLOCK_DEFS {
        let name = format!("Blk{b}");
        let mut ents = Vec::with_capacity(ENTITIES_PER_BLOCK);
        for i in 0..ENTITIES_PER_BLOCK {
            ents.push(EntityType::Line(Line::from_points(
                Vector3::new(0.0, i as f64, 0.0),
                Vector3::new(10.0, i as f64, 0.0),
            )));
        }
        scene.define_block_raw(&name, Vector3::ZERO, ents);
        scene.add_entity(EntityType::Insert(Insert::new(
            &name,
            Vector3::new((b as f64) * 20.0, -50.0, 0.0),
        )));
    }
    for i in 0..MODEL_LINES {
        let x = (i % 500) as f64 * 10.0;
        let y = (i / 500) as f64 * 10.0;
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(x, y, 0.0),
            Vector3::new(x + 8.0, y + 2.0, 0.0),
        )));
    }
    let cam = scene.camera.borrow().clone();
    let _ = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
    scene
}

fn main() {
    let mut scene = build_scene();
    let cam = scene.camera.borrow().clone();
    println!("wires_build_visible bench (model_lines={MODEL_LINES}, blocks={BLOCK_DEFS}x{ENTITIES_PER_BLOCK})");

    let mut sink = 0usize;
    for _ in 0..WARMUP {
        scene.bump_geometry();
        sink = sink.wrapping_add(scene.model_tile_wires_arc(0, &cam, 1.0, 1.0).len());
    }

    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        scene.bump_geometry();
        let start = Instant::now();
        let wires = scene.model_tile_wires_arc(0, black_box(&cam), 1.0, 1.0);
        samples.push(start.elapsed().as_nanos() as u64);
        sink = sink.wrapping_add(wires.len());
    }

    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let min = samples[0];
    let max = samples[samples.len() - 1];
    println!(
        "{:<28} median={:.3} ms  min={:.3} ms  max={:.3} ms  (sink={sink})",
        "full_wires_rebuild",
        median as f64 / 1_000_000.0,
        min as f64 / 1_000_000.0,
        max as f64 / 1_000_000.0,
    );
}
