//! Synthetic benchmark for `InteractionCandidates::materialize_stubs`.
//!
//! Exercises the `stub-materialize` path on a deterministic block-heavy scene
//! (~48k plain lines + ~12k block inserts, where each insert stays as an
//! `InstanceStub` standing for 12 prototype wires) across four steady-state
//! query arms:
//!
//! 1. `warm_viewport_stub_materialize` — indexed query over half the sheet
//!    (~24k plain wires + ~6k stubs -> ~96k materialized wires).
//! 2. `warm_cursor_stub_materialize` — indexed cursor-aperture query over a
//!    local region with a handful of stubs + nearby lines.
//! 3. `post_edit_overlay_stub_materialize` — same viewport query after 1 line
//!    edit (overlay active: `CandidateWires::Shared` + `wire_indices`).
//! 4. `post_edit_cursor_stub_materialize` — cursor-aperture query under overlay.
//!
//! Asserts that `wire.stub.is_some()` exists in resident wires and that stub
//! expansion increases candidate count so optimizations cannot silently bypass
//! expansion, and accumulates an order-independent digest over candidate wires.
//!
//! Run: `cargo bench --bench stub_materialize`

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use document::entities::{Insert, Line};
use document::types::Vector3;
use document::EntityType;
use OpenCADStudio::scene::pick::interaction_index::{InteractionCandidates, InteractionIndex};
use OpenCADStudio::scene::Scene;

const PLAIN_COLS: usize = 400;
const PLAIN_ROWS: usize = 120;
const PLAIN_SPACING: f64 = 10.0;

const BLOCK_DEFS: usize = 4;
const LINES_PER_BLOCK: usize = 12;
const INSERT_COLS: usize = 200;
const INSERT_ROWS: usize = 60;
const INSERT_SPACING: f64 = 20.0;

const BOUNDS_H: f32 = 1080.0;
const WARMUP: usize = 2;
const RUNS: usize = 5;
const VIEWPORT_ITERS: usize = 5;
const CURSOR_ITERS: usize = 100;

fn candidate_digest(candidates: &InteractionCandidates) -> u64 {
    let mut digests: Vec<u64> = candidates
        .iter()
        .map(|wire| {
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            for byte in wire.name.as_bytes() {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
            }
            hash.wrapping_add(wire.points.len() as u64)
        })
        .collect();
    digests.sort_unstable();
    digests
        .iter()
        .fold(candidates.len() as u64, |acc, d| acc.wrapping_add(*d))
}

fn build_scene() -> (Scene, document::Handle) {
    let mut scene = Scene::new();

    for b in 0..BLOCK_DEFS {
        let name = format!("Fixture{b}");
        let mut ents = Vec::with_capacity(LINES_PER_BLOCK);
        for i in 0..LINES_PER_BLOCK {
            let y = i as f64 * 0.8;
            ents.push(EntityType::Line(Line::from_points(
                Vector3::new(0.0, y, 0.0),
                Vector3::new(8.0 + b as f64, y + 0.4, 0.0),
            )));
        }
        scene.define_block_raw(&name, Vector3::ZERO, ents);
    }

    for r in 0..INSERT_ROWS {
        for c in 0..INSERT_COLS {
            let block_idx = (r * INSERT_COLS + c) % BLOCK_DEFS;
            let name = format!("Fixture{block_idx}");
            let pos = Vector3::new(c as f64 * INSERT_SPACING, r as f64 * INSERT_SPACING, 0.0);
            scene.add_entity(EntityType::Insert(Insert::new(&name, pos)));
        }
    }

    let mut first_line = document::Handle::NULL;
    for r in 0..PLAIN_ROWS {
        for c in 0..PLAIN_COLS {
            let x = c as f64 * PLAIN_SPACING + 1.0;
            let y = r as f64 * PLAIN_SPACING + 1.0;
            let h = scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::new(x, y, 0.0),
                Vector3::new(x + 4.0, y + 2.0, 0.0),
            )));
            if first_line.is_null() && r == 20 && c == 50 {
                first_line = h;
            }
        }
    }

    let wires = scene.hit_test_wires();
    let stub_count = wires.iter().filter(|w| w.stub.is_some()).count();
    assert!(
        stub_count >= (INSERT_COLS * INSERT_ROWS) / 2,
        "expected most block inserts to remain stubs in resident wires, got {stub_count}"
    );

    let key = scene
        .interaction_index_build_key(&wires, BOUNDS_H)
        .expect("interaction_index_build_key");
    let index = Arc::new(InteractionIndex::build(&wires));
    assert!(scene.install_prepared_interaction_index(key.0, key.1, Arc::downgrade(&wires), index,));

    (scene, first_line)
}

fn measure(label: &str, iters: usize, mut f: impl FnMut() -> InteractionCandidates) {
    let mut expected_count = 0usize;
    let mut expected_digest = 0u64;
    for w in 0..WARMUP {
        let c = f();
        if w == 0 {
            expected_count = c.len();
            expected_digest = candidate_digest(&c);
        }
    }

    let mut samples_ns = Vec::with_capacity(RUNS);
    let mut sink = 0usize;
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..iters {
            let c = black_box(f());
            sink = sink.wrapping_add(c.len());
        }
        samples_ns.push(start.elapsed().as_nanos() as u64 / iters as u64);
    }

    // Verify digest stays identical at end of run too (untimed).
    let check = f();
    assert_eq!(check.len(), expected_count);
    assert_eq!(candidate_digest(&check), expected_digest);

    samples_ns.sort_unstable();
    let median = samples_ns[RUNS / 2] as f64 / 1_000_000.0;
    let min = samples_ns[0] as f64 / 1_000_000.0;
    let max = samples_ns[RUNS - 1] as f64 / 1_000_000.0;
    println!(
        "{label:<36} median={median:.3} ms  min={min:.3} ms  max={max:.3} ms  (count={expected_count}, digest=0x{expected_digest:016x}, sink={sink})"
    );
}

fn main() {
    let (mut scene, edit_handle) = build_scene();

    let viewport_box = [-10.0, -10.0, 2000.0, 600.0];
    let aperture_box = [500.0, 200.0, 525.0, 225.0];

    measure("warm_viewport_stub_materialize", VIEWPORT_ITERS, || {
        let c = scene.interaction_candidates_xy(black_box(viewport_box));
        assert!(
            c.len() > 40_000,
            "expected stub expansion in viewport: {}",
            c.len()
        );
        c
    });

    measure("warm_cursor_stub_materialize", CURSOR_ITERS, || {
        let c = scene.interaction_candidates_xy(black_box(aperture_box));
        assert!(
            c.len() > 20,
            "expected stub expansion in aperture: {}",
            c.len()
        );
        c
    });

    // Apply 1 small line edit so overlay engages without triggering a rebuild.
    let mut moved = Line::from_points(
        Vector3::new(501.0, 201.0, 0.0),
        Vector3::new(506.0, 204.0, 0.0),
    );
    moved.common.handle = edit_handle;
    assert!(scene.update_entity(EntityType::Line(moved)));

    let wires_after = scene.hit_test_wires();
    assert!(
        scene
            .interaction_index_build_key(&wires_after, BOUNDS_H)
            .is_none(),
        "1-line edit must keep base index as overlay without scheduling rebuild"
    );
    drop(wires_after);

    measure("post_edit_overlay_stub_materialize", VIEWPORT_ITERS, || {
        let c = scene.interaction_candidates_xy(black_box(viewport_box));
        assert!(
            c.len() > 40_000,
            "expected stub expansion in overlay viewport: {}",
            c.len()
        );
        c
    });

    measure("post_edit_cursor_stub_materialize", CURSOR_ITERS, || {
        let c = scene.interaction_candidates_xy(black_box(aperture_box));
        assert!(
            c.len() > 20,
            "expected stub expansion in overlay aperture: {}",
            c.len()
        );
        c
    });
}
