//! OpenCADStudio incremental block-invalidation benchmark suite.
//!
//! Measures end-to-end app update + next-frame `Scene::entity_wires()` rebuild
//! across paths targeted by scoped block invalidation:
//! 1. `undo_redo_block_def`: undo + redo of a block-definition entity edit
//! 2. `undo_redo_vis_state`: undo + redo of a block `visibility_states` switch
//! 3. `show_visibility_state`: interactive BEDIT `show_visibility_state` switch
//! 4. `dynamic_action_eval`: interactive dynamic-block stretch parameter commit
//!
//! Run: `cargo bench --bench block_incremental_perf`

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::app::BlockBenchHarness;

const BACKGROUND_LINES: usize = 10_000;
const OTHER_BLOCKS: usize = 40;
const ENTITIES_PER_BLOCK: usize = 25;
const RUNS: usize = 5;
const ITERATIONS: usize = 5;
const WARMUP: usize = 2;

fn report(name: &str, samples_ns: &[u64], sink: usize) {
    let mut sorted = samples_ns.to_vec();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];
    println!(
        "{name:<26} median={:.3} ms  min={:.3} ms  max={:.3} ms  (sink={sink})",
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
        "Block incremental perf bench (bg_lines={BACKGROUND_LINES}, other_blocks={OTHER_BLOCKS}x{ENTITIES_PER_BLOCK})"
    );

    let mut undo_app = BlockBenchHarness::setup_block_edit_undo(
        BACKGROUND_LINES,
        OTHER_BLOCKS,
        ENTITIES_PER_BLOCK,
    );
    bench_op("undo_redo_block_def", |_| undo_app.undo_redo_cycle());

    let (mut vis_app, vis_def, vis_param) = BlockBenchHarness::setup_visibility(
        BACKGROUND_LINES,
        OTHER_BLOCKS,
        ENTITIES_PER_BLOCK,
    );
    bench_op("undo_redo_vis_state", |_| vis_app.undo_redo_cycle());
    bench_op("show_visibility_state", |step| {
        vis_app.show_visibility_state(vis_def, vis_param, step % 2)
    });

    let (mut action_app, insert, action_def, stretch_param) =
        BlockBenchHarness::setup_dynamic_action(
            BACKGROUND_LINES,
            OTHER_BLOCKS,
            ENTITIES_PER_BLOCK,
        );
    bench_op("dynamic_action_eval", |step| {
        let x = 11.0 + ((step % 10) as f64) * 0.5;
        action_app.evaluate_dynamic_action(insert, action_def, stretch_param, x)
    });
}

fn main() {
    std::thread::Builder::new()
        .name("block_incremental_perf".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .expect("spawn bench thread")
        .join()
        .expect("bench thread panicked");
}
