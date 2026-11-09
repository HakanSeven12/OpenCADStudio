//! OpenCADStudio grip-drag OTRACK candidate source benchmark suite.
//!
//! While a grip is dragged, `viewport.rs` built a `Vec<WireModel>` by
//! deep-cloning every near-cursor snap candidate plus the frozen grip
//! reference wires on EVERY pointer move, solely to feed
//! `Snapper::update_otrack_dwell`. This bench measures the removed cost on
//! representative synthetic wires:
//!
//! * Path A (old code): `candidates.iter().cloned().collect()` then
//!   `extend(refs.iter().cloned())`, iterate the clone (drop included).
//! * Path B (fixed code): iterate the same wires through the `WireSource`
//!   accessors — the sequence the composite `WireSource` chains — with zero
//!   copies.
//!
//! Both paths do identical per-wire work (read `points.len()` through
//! `black_box`); the accumulated sums are printed so the optimizer cannot
//! dead-code-eliminate either path. Deterministic: fixed synthetic geometry,
//! no randomness, warm-up excluded from samples.
//!
//! Run: `cargo bench --bench grip_drag_otrack_candidates`

use std::hint::black_box;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

use OpenCADStudio::scene::model::wire_model::WireModel;
use OpenCADStudio::scene::pick::interaction_index::{InteractionCandidates, WireSource};

const CANDIDATE_WIRES: usize = 80;
const REFERENCE_WIRES: usize = 10;
const POINTS_PER_WIRE: usize = 2_000;
const RUNS: usize = 7;
/// Path A clones ~8 MiB per op; fewer iterations keep the run short.
const ITERATIONS_A: usize = 30;
const WARMUP_A: usize = 3;
/// Path B is sub-microsecond per op; many more iterations for a stable clock.
const ITERATIONS_B: usize = 1_000;
const WARMUP_B: usize = 100;

/// Polylines shaped like production tessellation output: double-single
/// `points`/`points_low` from `solid_f64` plus one `key_vertices` entry per
/// vertex — the buffers a grip-drag move used to deep-clone every move.
fn synthetic_wires(prefix: &str, count: usize) -> Vec<WireModel> {
    (0..count)
        .map(|wire| {
            let points: Vec<[f64; 3]> = (0..POINTS_PER_WIRE)
                .map(|p| {
                    let t = p as f64 * 0.25;
                    [wire as f64 * 100.0 + t, (t * 0.1).sin() * 25.0, 0.0]
                })
                .collect();
            let key_vertices = points.clone();
            let mut model =
                WireModel::solid_f64(format!("{prefix}{wire}"), points, WireModel::WHITE, false);
            model.key_vertices = key_vertices;
            model
        })
        .collect()
}

fn heap_bytes(wires: &[WireModel]) -> usize {
    wires
        .iter()
        .map(|w| {
            w.points.capacity() * size_of::<[f32; 3]>()
                + w.points_low.capacity() * size_of::<[f32; 3]>()
                + w.key_vertices.capacity() * size_of::<[f64; 3]>()
                + w.name.capacity()
        })
        .sum()
}

/// The removed production pattern (viewport.rs grip-drag pointer move):
/// deep-clone every candidate wire, extend with deep-cloned reference wires,
/// iterate the resulting `Vec`, drop it — per move.
fn path_a(candidates: &InteractionCandidates, refs: &[WireModel]) -> usize {
    let src = black_box(candidates);
    let mut cloned: Vec<WireModel> = src.iter().cloned().collect();
    cloned.extend(black_box(refs).iter().cloned());
    let mut total = 0usize;
    for wire in &cloned {
        total += black_box(wire.points.len());
    }
    total
}

/// The fixed pattern: the composite `WireSource` chains exactly these two
/// `WireSource::iter` accessors, so this is the same wire sequence
/// `update_otrack_dwell` now walks — without any clone.
fn path_b(candidates: &InteractionCandidates, refs: &[WireModel]) -> usize {
    let src = black_box(candidates);
    let tail = black_box(refs);
    let mut total = 0usize;
    for wire in WireSource::iter(src).chain(WireSource::iter(tail)) {
        total += black_box(wire.points.len());
    }
    total
}

/// Median ms/op over `RUNS` samples. The accumulated per-op sums escape into
/// the printed `sink`, so neither path can be optimized away.
fn measure(label: &str, iterations: usize, warmup: usize, mut run: impl FnMut() -> usize) -> f64 {
    let mut sink = 0usize;
    for _ in 0..warmup {
        sink = sink.wrapping_add(run());
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..iterations {
            sink = sink.wrapping_add(run());
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.0 / iterations as f64);
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[RUNS / 2];
    println!("  {label} median {median:.6} ms/op  samples {samples:?}  sink {sink}");
    median
}

fn main() {
    let candidate_wires = synthetic_wires("cand_", CANDIDATE_WIRES);
    let ref_wires = synthetic_wires("ref_", REFERENCE_WIRES);
    let payload_bytes = heap_bytes(&candidate_wires) + heap_bytes(&ref_wires);
    let candidates = InteractionCandidates::all(Arc::new(candidate_wires));

    println!(
        "grip-drag snap candidate cloning: {CANDIDATE_WIRES} candidates + {REFERENCE_WIRES} reference wires x {POINTS_PER_WIRE} points = {:.2} MiB deep-cloned per pointer move",
        payload_bytes as f64 / (1024.0 * 1024.0)
    );

    let a_median = measure(
        "Path A (clone into Vec<WireModel>):",
        ITERATIONS_A,
        WARMUP_A,
        || path_a(&candidates, &ref_wires),
    );
    let b_median = measure(
        "Path B (WireSource iteration):    ",
        ITERATIONS_B,
        WARMUP_B,
        || path_b(&candidates, &ref_wires),
    );

    println!(
        "  ratio A/B: {:.1}x (removed per-move clone cost)",
        a_median / b_median
    );
}
