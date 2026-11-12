//! OpenCADStudio dynamic block grip-preview benchmark suite.
//!
//! Measures the production preview paths for dynamic block grip dragging:
//! 1. Stretch grip preview (`evaluate_value` + `reference_document_preview`)
//! 2. Flip grip preview (`evaluate_value` + `reference_document_preview`)
//! 3. Alignment grip preview (`AlignmentCommand` preview wires/entities)
//!
//! Run: `cargo bench --bench dynamic_block_grip_preview`

use std::hint::black_box;
use std::time::Instant;

use glam::DVec3;
use OpenCADStudio::scene::Scene;

const BACKGROUND_LINES: usize = 2_000;
const BACKGROUND_HATCHES: usize = 20;
const BLOCK_LINES: usize = 80;
const RUNS: usize = 5;
const ITERATIONS: usize = 20;
const WARMUP: usize = 3;

fn report(name: &str, samples_ns: &[u64], sink: usize) {
    let mut sorted = samples_ns.to_vec();
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

fn bench_stretch(scene: &mut Scene, insert: document::types::Handle, stretch_param: document::types::Handle) {
    let mut samples = Vec::with_capacity(RUNS);
    let mut sink = 0usize;
    for i in 0..WARMUP {
        let x = 11.0 + (i as f64) * 0.5;
        let (w, h) = scene
            .bench_block_parameter_preview(insert, stretch_param, DVec3::new(x, 0.0, 0.0))
            .expect("stretch preview");
        sink = sink.wrapping_add(w.len() + h.len());
    }
    for _ in 0..RUNS {
        let start = Instant::now();
        for i in 0..ITERATIONS {
            let x = 10.5 + (i as f64) * 0.25;
            let (w, h) = scene
                .bench_block_parameter_preview(
                    black_box(insert),
                    black_box(stretch_param),
                    black_box(DVec3::new(x, 0.0, 0.0)),
                )
                .expect("stretch preview");
            sink = sink.wrapping_add(w.len() + h.len());
        }
        samples.push((start.elapsed().as_nanos() / ITERATIONS as u128) as u64);
    }
    report("stretch_grip_preview", &samples, sink);
}

fn bench_flip(scene: &mut Scene, insert: document::types::Handle, flip_param: document::types::Handle) {
    let mut samples = Vec::with_capacity(RUNS);
    let mut sink = 0usize;
    for i in 0..WARMUP {
        let (w, h) = scene
            .bench_block_flip_preview(insert, flip_param, i % 2 == 0)
            .expect("flip preview");
        sink = sink.wrapping_add(w.len() + h.len());
    }
    for _ in 0..RUNS {
        let start = Instant::now();
        for i in 0..ITERATIONS {
            let (w, h) = scene
                .bench_block_flip_preview(
                    black_box(insert),
                    black_box(flip_param),
                    black_box(i % 2 == 0),
                )
                .expect("flip preview");
            sink = sink.wrapping_add(w.len() + h.len());
        }
        samples.push((start.elapsed().as_nanos() / ITERATIONS as u128) as u64);
    }
    report("flip_grip_preview", &samples, sink);
}

fn bench_alignment(scene: &Scene, insert: document::types::Handle) {
    let mut cmd = scene
        .bench_start_alignment_command(insert)
        .expect("alignment command");
    let mut samples = Vec::with_capacity(RUNS);
    let mut sink = 0usize;
    for i in 0..WARMUP {
        let cur = DVec3::new(120.0, (i as f64) * 2.0, 0.0);
        let mut wires = cmd.on_preview_wires(cur);
        let entities = cmd.preview_entities(&scene.document, cur);
        wires.extend(scene.wires_for_entities(&entities));
        sink = sink.wrapping_add(wires.len());
    }
    for _ in 0..RUNS {
        let start = Instant::now();
        for i in 0..ITERATIONS {
            let cur = black_box(DVec3::new(120.0 + (i as f64) * 0.5, (i as f64) * 2.0, 0.0));
            let mut wires = cmd.on_preview_wires(cur);
            let entities = cmd.preview_entities(&scene.document, cur);
            wires.extend(scene.wires_for_entities(&entities));
            sink = sink.wrapping_add(wires.len());
        }
        samples.push((start.elapsed().as_nanos() / ITERATIONS as u128) as u64);
    }
    report("alignment_grip_preview", &samples, sink);
}

fn main() {
    let (mut scene, insert, stretch_param, flip_param) =
        Scene::bench_setup_dynamic_block_scene(BACKGROUND_LINES, BACKGROUND_HATCHES, BLOCK_LINES);
    println!(
        "Dynamic block grip preview bench (bg_lines={BACKGROUND_LINES}, bg_hatches={BACKGROUND_HATCHES}, block_lines={BLOCK_LINES})"
    );
    bench_stretch(&mut scene, insert, stretch_param);
    bench_flip(&mut scene, insert, flip_param);
    bench_alignment(&scene, insert);
}
