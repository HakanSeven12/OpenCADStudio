//! OpenCADStudio parametric pre-solve fixpoint benchmark.
//!
//! Measures `Scene::solve_parametric_constraints_preview` — the per-pointer-move
//! grip-drag re-solve — on a scope whose cost is the pre-solve propagation
//! rather than the numeric solve. Before the kernel runs, `solve_scope`
//! propagates Equal followers and implied line axes, and the preview expands
//! the connected component, each by repeated passes over every constraint
//! until nothing changes. A chain whose constraints run against the drag
//! direction advances one link per pass.
//!
//! Fixture: `LINES` parallel horizontal lines of equal length, stacked 10
//! units apart. Each adjacent pair carries an Equal and a Parallel
//! constraint, added from the first line to the last; one Horizontal
//! constraint sits on the last line. The timed drag holds the last line's end
//! grip where it is, so the geometry already satisfies every constraint, the
//! numeric solve converges at once and nothing is written back.
//!
//! Sink: every timed call must return no changed entity. After timing, the
//! grip is pulled one unit along the line and solved once (untimed): every
//! other line follows its length, and the handles and coordinates of that
//! result are folded into a digest that must be identical before and after a
//! fix.
//!
//! Deterministic: fixed geometry, no randomness.
//! Run: `cargo bench --bench parametric_presolve_fixpoint`

use std::hint::black_box;
use std::time::Instant;

use codec::entities::{EntityType, Line};
use codec::types::{Handle, Vector3};
use OpenCADStudio::scene::parametric_constraints::{
    ConstraintKind, ParametricRef, ParametricScope,
};
use OpenCADStudio::scene::Scene;

const LINES: usize = 300;
const RUNS: usize = 3;
const ITERATIONS: usize = 10;
const WARMUP: usize = 1;

fn build_chain() -> (Scene, Vec<Handle>) {
    let mut scene = Scene::new();
    let mut lines = Vec::with_capacity(LINES);
    for i in 0..LINES {
        let mut line = Line::new();
        line.start = Vector3::new(0.0, i as f64 * 10.0, 0.0);
        line.end = Vector3::new(10.0, i as f64 * 10.0, 0.0);
        lines.push(scene.add_entity(EntityType::Line(line)));
    }
    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    for pair in lines.windows(2) {
        for kind in [ConstraintKind::Equal, ConstraintKind::Parallel] {
            set.add(
                kind,
                vec![ParametricRef::whole(pair[0]), ParametricRef::whole(pair[1])],
                None,
            );
        }
    }
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(lines[LINES - 1])],
        None,
    );
    (scene, lines)
}

fn mix(digest: &mut u64, value: u64) {
    *digest ^= value;
    *digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
}

fn digest(solved: &[(Handle, EntityType)]) -> u64 {
    let mut sorted: Vec<_> = solved.iter().collect();
    sorted.sort_by_key(|(handle, _)| handle.value());
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    for (handle, entity) in sorted {
        mix(&mut digest, handle.value());
        if let EntityType::Line(line) = entity {
            for value in [line.start.x, line.start.y, line.end.x, line.end.y] {
                mix(&mut digest, value.to_bits());
            }
        }
    }
    digest
}

fn main() {
    let (mut scene, lines) = build_chain();
    let dragged = lines[LINES - 1];
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
    let touched = [dragged];
    let driven = [ParametricRef::point(dragged, 1)];
    let solve = |scene: &Scene| {
        scene.solve_parametric_constraints_preview(
            black_box(&touched),
            black_box(&driven),
            true,
            black_box(&originals),
        )
    };

    for _ in 0..WARMUP {
        assert!(solve(&scene).is_empty(), "a held grip moves nothing");
    }
    let mut best = f64::MAX;
    for run in 0..RUNS {
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            let solved = solve(&scene);
            assert!(solved.is_empty(), "a held grip moves nothing");
            black_box(solved);
        }
        let per_call = started.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64;
        best = best.min(per_call);
        println!("parametric_presolve_fixpoint: run {run} {ITERATIONS} calls: {per_call:.3} ms/call");
    }

    // The grip edit production applies before a moving preview solve.
    if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(dragged) {
        line.end.x += 1.0;
    }
    let moved = solve(&scene);
    assert_eq!(moved.len(), LINES - 1, "every other line follows the length");
    println!(
        "parametric_presolve_fixpoint: {LINES} lines, {} constraints, moved-grip digest {:#018x}; best {best:.3} ms/call",
        2 * (LINES - 1) + 1,
        digest(&moved),
    );
}
