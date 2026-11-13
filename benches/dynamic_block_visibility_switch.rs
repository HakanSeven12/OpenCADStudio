//! OpenCADStudio dynamic block visibility-state switch benchmark.
//!
//! Measures switching a dynamic block's visibility state followed by the
//! next-frame `Scene::entity_wires()` rebuild on a large drawing.
//!
//! Run: `cargo bench --bench dynamic_block_visibility_switch`

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::scene::Scene;

const BACKGROUND_LINES: usize = 10_000;
const OTHER_BLOCKS: usize = 40;
const ENTITIES_PER_BLOCK: usize = 25;
const RUNS: usize = 5;
const ITERATIONS: usize = 10;
const WARMUP: usize = 2;

fn report(name: &str, samples: &[u64], sink: usize) {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];
    println!(
        "{name:<28} median={:.3} ms  min={:.3} ms  max={:.3} ms  (sink={sink})",
        median as f64 / 1_000_000.0,
        min as f64 / 1_000_000.0,
        max as f64 / 1_000_000.0,
    );
}

fn main() {
    println!(
        "Dynamic block visibility switch bench (bg_lines={BACKGROUND_LINES}, other_blocks={OTHER_BLOCKS}x{ENTITIES_PER_BLOCK})"
    );
    let mut first_samples = Vec::with_capacity(RUNS);
    let mut first_sink = 0usize;
    for r in 0..RUNS {
        let (mut first_scene, first_insert, first_def, first_param) =
            Scene::bench_setup_visibility_scene(BACKGROUND_LINES, OTHER_BLOCKS, ENTITIES_PER_BLOCK);
        let start = Instant::now();
        first_sink = first_sink.wrapping_add(first_scene.bench_switch_visibility_state(
            black_box(first_insert),
            black_box(first_def),
            black_box(first_param),
            black_box(r % 2),
        ));
        first_samples.push(start.elapsed().as_nanos() as u64);
    }
    report("first_visibility_switch", &first_samples, first_sink);

    let (mut scene, insert, def, param) =
        Scene::bench_setup_visibility_scene(BACKGROUND_LINES, OTHER_BLOCKS, ENTITIES_PER_BLOCK);
    let mut sink = 0usize;
    for i in 0..WARMUP {
        sink = sink.wrapping_add(scene.bench_switch_visibility_state(insert, def, param, i % 2));
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for i in 0..ITERATIONS {
            sink = sink.wrapping_add(scene.bench_switch_visibility_state(
                black_box(insert),
                black_box(def),
                black_box(param),
                black_box(i % 2),
            ));
        }
        samples.push((start.elapsed().as_nanos() / ITERATIONS as u128) as u64);
    }
    report("visibility_state_switch", &samples, sink);
}
