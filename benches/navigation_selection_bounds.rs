//! OpenCADStudio navigation selection-bounds benchmark.
//!
//! Orbiting with a selection pivots about the selection's centre. The bounds
//! were found by building the wires of the whole active space — every entity,
//! not only the selected ones — and keeping the selected handles' points, so
//! the cost of an orbit start (and of every touch-orbit event, which asks
//! again each time) grew with the drawing instead of with the selection.
//!
//! Fixture: 100,000 lines in model space, four of them selected. One warm-up
//! call lets the tessellation memo fill, as the renderer would already have
//! done in a live session; the timed calls are `Scene::orbit_pivot`.
//!
//! Sink: every call must return the centre of the four selected lines,
//! (50, 250, 0); an empty selection must return no pivot, and the drawing
//! keeps its line count.
//!
//! Deterministic: fixed geometry, no randomness.
//! Run: `cargo bench --bench navigation_selection_bounds`

use std::hint::black_box;
use std::time::Instant;

use codec::entities::{EntityType, Line};
use codec::types::Vector3;
use OpenCADStudio::scene::Scene;

const LINES: usize = 100_000;
const SELECTED: [usize; 4] = [10, 20, 30, 40];
const ITERATIONS: usize = 20;
const RUNS: usize = 3;
const WARMUP: usize = 1;

fn main() {
    let mut scene = Scene::new();
    let lines = (0..LINES)
        .map(|index| {
            let y = index as f64 * 10.0;
            EntityType::Line(Line::from_points(
                Vector3::new(0.0, y, 0.0),
                Vector3::new(100.0, y, 0.0),
            ))
        })
        .collect();
    let handles = scene.add_entities(lines);
    assert_eq!(scene.document.entities().count(), LINES);

    assert!(scene.orbit_pivot().is_none(), "no selection, no pivot");
    for index in SELECTED {
        scene.selected.insert(handles[index]);
    }
    scene.selection_generation += 1;

    let expected = glam::DVec3::new(50.0, 250.0, 0.0);
    let pivot = |scene: &Scene| {
        let pivot = black_box(scene).orbit_pivot().expect("selection pivot");
        assert!(
            (pivot - expected).length() < 1e-6,
            "pivot {pivot:?} is not the selection centre {expected:?}"
        );
        pivot
    };
    for _ in 0..WARMUP {
        black_box(pivot(&scene));
    }

    let mut best = f64::MAX;
    for run in 0..RUNS {
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(pivot(&scene));
        }
        let per_call_ms = started.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64;
        best = best.min(per_call_ms);
        println!("navigation_selection_bounds: run {run} {ITERATIONS} calls: {per_call_ms:.4} ms/call");
    }

    assert_eq!(scene.document.entities().count(), LINES);
    println!(
        "navigation_selection_bounds: {LINES} lines, {} selected, {ITERATIONS} calls x {RUNS} runs; \
         best {best:.4} ms/call",
        SELECTED.len()
    );
}
