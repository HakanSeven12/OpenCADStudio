//! OpenCADStudio parametric constraint preview-solve benchmark suite.
//!
//! Measures the REAL per-pointer-move production entry point:
//! `Scene::solve_parametric_constraints_preview` — the function
//! `src/app/command_driver/utilities.rs::solve_grip_constraints` calls on
//! every grip-drag move (from `src/app/update/viewport.rs`). Each timed call
//! re-resolves the touched scope, rebuilds the kernel `System` from scratch,
//! runs the dogleg solve over the whole connected chain, and (on the
//! baseline) deep-clones the full `ParametricConstraintSet` plus runs the
//! `diagnose`/`classify_redundant` diagnostics per move.
//!
//! Fixture: a solvable chain of `LINES` consecutive horizontal line segments
//! (100–300 entities / 200–500 constraints):
//! one Coincident constraint joining each adjacent pair of endpoints
//! (`LINES - 1`) plus one Horizontal constraint per line (`LINES`), all in
//! the ModelSpace `ParametricConstraintSet`. Every entity is connected, so a
//! grip drag on the first line's start point re-solves the entire scope —
//! the worst-case connected component, exactly what production solves when
//! dragging inside a constrained sketch.
//!
//! Per iteration the bench mirrors one production pointer move:
//! 1. move the dragged grip (line 0 start) by a small delta in the document
//!    (the `apply_grip` step production does before the solve; untimed),
//! 2. time `solve_parametric_constraints_preview` with that handle as the
//!    touched set and its start point as the driven grip anchor
//!    (`retain_size = true`, matching a default CONSTRAINTSOLVEMODE drag),
//! 3. write the solved geometry back into the document (the preview
//!    write-back production performs; untimed).
//!
//! Deterministic: fixed geometry, fixed delta sequence, no randomness.
//! Run: `cargo bench --bench parametric_preview_solve`

use std::hint::black_box;
use std::time::Instant;

use document::entities::{EntityType, Line};
use document::types::{Handle, Vector3};
use OpenCADStudio::scene::parametric_constraints::{
    ConstraintKind, ParametricRef, ParametricScope,
};
use OpenCADStudio::scene::Scene;

const LINES: usize = 150;
const SEGMENT: f64 = 10.0;
const RUNS: usize = 5;
const ITERATIONS: usize = 12;
const WARMUP: usize = 2;

/// A chain of `LINES` horizontal segments, `(10i, 0) -> (10(i+1), 0)`,
/// joined end-to-start by Coincident constraints and pinned horizontal.
/// Returns the scene, the line handles, and the handle whose start grip
/// will be dragged.
fn build_chain() -> (Scene, Vec<Handle>, Handle) {
    let mut scene = Scene::new();
    let mut lines = Vec::with_capacity(LINES);
    for i in 0..LINES {
        let mut line = Line::new();
        line.start = Vector3::new(i as f64 * SEGMENT, 0.0, 0.0);
        line.end = Vector3::new((i as f64 + 1.0) * SEGMENT, 0.0, 0.0);
        lines.push(scene.add_entity(EntityType::Line(line)));
    }
    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    for pair in lines.windows(2) {
        set.add(
            ConstraintKind::Coincident,
            vec![
                ParametricRef::point(pair[0], 1),
                ParametricRef::point(pair[1], 0),
            ],
            None,
        );
    }
    for &handle in &lines {
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(handle)],
            None,
        );
    }
    let dragged = lines[0];
    (scene, lines, dragged)
}

/// One production pointer move around the timed preview solve. Returns the
/// number of solved entities plus the timed solve duration in milliseconds.
fn frame(
    scene: &mut Scene,
    dragged: Handle,
    originals: &[(Handle, EntityType)],
    counter: &mut u64,
) -> (usize, f64) {
    // 1. untimed: the apply_grip document edit preceding each move.
    let step = (*counter % 8) as f64 * 0.01;
    *counter += 1;
    if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(dragged) {
        line.start = Vector3::new(-step, step * 0.5, 0.0);
    }
    // 2. timed: the production per-move preview solve.
    let touched = [dragged];
    let driven_refs = [ParametricRef::point(dragged, 0)];
    let start = Instant::now();
    let solved = scene.solve_parametric_constraints_preview(
        black_box(&touched),
        black_box(&driven_refs),
        true,
        black_box(originals),
    );
    let solve_ms = start.elapsed().as_secs_f64() * 1000.0;
    // 3. untimed: the production preview write-back.
    for (handle, entity) in solved.iter() {
        if let Some(slot) = scene.document.get_entity_mut(*handle) {
            *slot = entity.clone();
        }
    }
    (solved.len(), solve_ms)
}

fn main() {
    let (mut scene, lines, dragged) = build_chain();
    let constraint_count = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .map(|set| set.constraints.len())
        .unwrap_or(0);
    println!(
        "grip-drag preview solve: {LINES} lines, {constraint_count} constraints ({} coincident + {} horizontal), whole chain connected",
        LINES - 1,
        LINES
    );
    println!(
        "  measured: Scene::solve_parametric_constraints_preview (per-pointer-move grip-drag re-solve; {ITERATIONS} calls/sample, {RUNS} samples)"
    );

    // Pre-drag snapshots of every connected entity — the `grip_originals`
    // list production accumulates and passes as `retained_originals`.
    let originals: Vec<(Handle, EntityType)> = lines
        .iter()
        .filter_map(|&handle| {
            scene
                .document
                .get_entity(handle)
                .cloned()
                .map(|entity| (handle, entity))
        })
        .collect();

    let mut counter = 0u64;
    let mut sink = 0usize;
    for _ in 0..WARMUP {
        let (solved, _) = frame(&mut scene, dragged, &originals, &mut counter);
        sink = sink.wrapping_add(solved);
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let mut acc_ms = 0.0;
        for _ in 0..ITERATIONS {
            let (solved, solve_ms) = frame(&mut scene, dragged, &originals, &mut counter);
            sink = sink.wrapping_add(solved);
            acc_ms += solve_ms;
        }
        samples.push(acc_ms / ITERATIONS as f64);
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[RUNS / 2];
    println!(
        "  solve_parametric_constraints_preview median {median:.4} ms/call  samples {samples:?}  sink {sink}"
    );
    println!(
        "  one call = one grip-drag mouse move's constraint preview re-solve of the full {LINES}-line connected scope"
    );
}
