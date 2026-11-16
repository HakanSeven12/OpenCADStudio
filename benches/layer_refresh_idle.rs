//! OpenCADStudio per-message Layer manager refresh benchmark.
//!
//! `refresh_layer_counts` runs after EVERY app message while the Layers panel
//! shows. Its count fold is already gated by `(geometry_epoch, name-hash)`, but
//! `refresh_layer_tree` still Debug-formats every layer and the whole filter
//! list and rebuilds the session filters from the drawing on each call — even
//! when nothing changed (the common hover / timer / redraw case).
//!
//! Measures the REAL entry production calls (`OpenCADStudio::refresh_layer_counts`
//! through the `#[doc(hidden)]` `LayerRefreshBenchHarness` bridge) on a hermetic
//! app: default config, a fresh drawing tab, the Layers panel floated open.
//!
//! Fixture: 300 layers with varied color / linetype / description / plottable,
//! 4000 lines spread across them, 12 property filters persisted in the drawing,
//! and a 40-object selection. Deterministic: no randomness, fixed cycles.
//!
//! Scenarios:
//!   (a) `idle_refresh` — primed once, then repeated refresh with NO state
//!       change: the steady-state per-message cost the optimization targets.
//!   (b) `edit_refresh` — a real `edit_layers` color edit before each refresh:
//!       the changed path, which must still rebuild the tree (no regression).
//!
//! Sink (semantic-equivalence proof): per-layer object counts plus filter
//! membership plus selected-object-layer / depth / highlight totals, accumulated
//! across every timed call. Identical before/after means identical refresh
//! output (equal work both runs); a gate that wrongly skips a needed rebuild
//! diverges here.
//!
//! Run: `cargo bench --bench layer_refresh_idle`

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::app::LayerRefreshBenchHarness;

const LAYERS: usize = 300;
const ENTITIES: usize = 4000;
const FILTERS: usize = 12;
const SELECTED: usize = 40;
const RUNS: usize = 7;
const ITERATIONS: usize = 20;
const WARMUP: usize = 3;

fn report(name: &str, samples_ns: &[u64], sink: usize) {
    let mut sorted = samples_ns.to_vec();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];
    println!(
        "{name:<26} median={:.4} ms  min={:.4} ms  max={:.4} ms  (sink={sink})",
        median as f64 / 1_000_000.0,
        min as f64 / 1_000_000.0,
        max as f64 / 1_000_000.0,
    );
}

fn bench_op(name: &str, mut f: impl FnMut(usize) -> usize) {
    let mut sink = 0usize;
    for i in 0..WARMUP {
        sink = sink.wrapping_add(f(i));
    }
    let mut samples = Vec::with_capacity(RUNS);
    for r in 0..RUNS {
        let start = Instant::now();
        for it in 0..ITERATIONS {
            sink = sink.wrapping_add(f(black_box(r * ITERATIONS + it)));
        }
        samples.push((start.elapsed().as_nanos() / ITERATIONS as u128) as u64);
    }
    report(name, &samples, sink);
}

fn run() {
    println!(
        "Layer refresh bench (layers={LAYERS}, entities={ENTITIES}, filters={FILTERS}, selected={SELECTED})"
    );

    let mut idle_app = LayerRefreshBenchHarness::setup(LAYERS, ENTITIES, FILTERS, SELECTED);
    bench_op("idle_refresh", |_| idle_app.refresh_idle());

    let mut edit_app = LayerRefreshBenchHarness::setup(LAYERS, ENTITIES, FILTERS, SELECTED);
    bench_op("edit_refresh", |step| {
        edit_app.refresh_after_color_edit(step)
    });
}

fn main() {
    std::thread::Builder::new()
        .name("layer_refresh_idle".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(run)
        .expect("spawn bench thread")
        .join()
        .expect("bench thread panicked");
}
