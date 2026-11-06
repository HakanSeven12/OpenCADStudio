//! OpenCADStudio post-edit overlay interaction candidate benchmark suite.
//!
//! After any geometry edit, `geometry_epoch` advances while the large
//! resident interaction index is kept as an immutable base plus a small
//! changed-handle overlay. As long as the overlay stays under the handle
//! limit, no background rebuild is scheduled, so every hover pick and snap
//! evaluation in the editing steady state runs through the overlay query
//! path in `Scene::indexed_interaction_candidates_xy`.
//!
//! This bench drives the real production entry points end to end:
//!
//! * Build a scene of `ENTITY_COUNT` short lines on a regular grid.
//! * Install the interaction index exactly as the app does (build key →
//!   `InteractionIndex::build` → `install_prepared_interaction_index`).
//! * Apply one small edit so the epoch advances while the base index goes
//!   stale — the overlay engages, and `interaction_index_build_key` returns
//!   `None` (no rebuild is scheduled; verified by assertion).
//! * Repeatedly call `Scene::interaction_hover_candidates_near` (pick) and
//!   `Scene::interaction_candidates_near` (snap, full category) with a fixed
//!   aperture over the edited region — one call per simulated pointer move,
//!   each fetching fresh resident wires like the UI does.
//!
//! The sink accumulates the candidate count plus an order-independent FNV
//! digest of every candidate wire's name and point count, so the exact
//! candidate sets before and after any change to the overlay path must
//! match. Deterministic: fixed geometry, fixed cursor, no randomness.
//!
//! Run: `cargo bench --bench overlay_interaction_candidates`

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use document::entities::Line;
use document::types::Vector3;
use document::EntityType;
use glam::{DVec3, Mat4};
use iced::Rectangle;

use OpenCADStudio::scene::pick::interaction_index::InteractionIndex;
use OpenCADStudio::scene::Scene;

/// A 67x67 grid: comfortably above the index threshold so every query runs
/// through the spatial index rather than a full scan.
const GRID: usize = 67;
const ENTITY_COUNT: usize = GRID * GRID;
/// Grid pitch in world units; each line is a short diagonal well inside one
/// cell, so a cursor aperture catches only its immediate neighbourhood.
const SPACING: f64 = 0.05;
const LINE_LENGTH: f64 = 0.02;
/// Cursor aperture in pixels, converted through the identity ortho view
/// (s = min(width, height)/2 = 540 px per world unit) to a ~0.44-unit box.
const APERTURE_PX: f32 = 120.0;
const RUNS: usize = 5;
const ITERATIONS: usize = 200;
const WARMUP: usize = 10;

const BOUNDS: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 1920.0,
    height: 1080.0,
};

fn line_at(index: usize) -> Line {
    let col = (index % GRID) as f64;
    let row = (index / GRID) as f64;
    let mut line = Line::new();
    line.start = Vector3::new(col * SPACING, row * SPACING, 0.0);
    line.end = Vector3::new(
        col * SPACING + LINE_LENGTH,
        row * SPACING + LINE_LENGTH,
        0.0,
    );
    line
}

/// Order-independent per-candidate digest: FNV-1a over the wire name plus
/// its point count, sorted before folding so a change in candidate iteration
/// order alone cannot move the sink.
fn candidate_digest(candidates: &OpenCADStudio::scene::pick::interaction_index::InteractionCandidates) -> u64 {
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
        .fold(candidates.len() as u64, |acc, digest| acc.wrapping_add(*digest))
}

fn measure(label: &str, mut run: impl FnMut() -> u64) -> f64 {
    let mut sink = 0u64;
    for _ in 0..WARMUP {
        sink = sink.wrapping_add(run());
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            sink = sink.wrapping_add(run());
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64);
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[RUNS / 2];
    println!("  {label} median {median:.6} ms/query  samples {samples:?}  sink {sink}");
    median
}

fn main() {
    println!(
        "post-edit overlay interaction candidates: {ENTITY_COUNT} lines at {SPACING}-unit pitch, \
         fixed {APERTURE_PX}px aperture over one edited entity"
    );

    let mut scene = Scene::new();
    let mut handles = Vec::with_capacity(ENTITY_COUNT);
    for index in 0..ENTITY_COUNT {
        handles.push(scene.add_entity(EntityType::Line(line_at(index))));
    }

    // Install the interaction index exactly as the app's background builder
    // does, while the scene is still at its initial epoch.
    let wires = scene.hit_test_wires();
    assert!(wires.len() >= ENTITY_COUNT, "resident set carries every line");
    let key = scene
        .interaction_index_build_key(&wires, BOUNDS.height)
        .expect("a fresh large resident source needs its index");
    let index = Arc::new(InteractionIndex::build(&wires));
    assert!(scene.install_prepared_interaction_index(
        key.0,
        key.1,
        Arc::downgrade(&wires),
        Arc::clone(&index),
    ));

    // Sanity: before any edit the query runs directly against the installed
    // index (no overlay engaged).
    let cursor_cell = (GRID / 2) * GRID + GRID / 2;
    let cursor = DVec3::new(
        (GRID / 2) as f64 * SPACING + LINE_LENGTH * 0.5,
        (GRID / 2) as f64 * SPACING + LINE_LENGTH * 0.5,
        0.0,
    );
    let view_rot = Mat4::IDENTITY;
    let eye = DVec3::ZERO;
    let pre_edit = scene.interaction_hover_candidates_near(
        Arc::clone(&wires),
        cursor,
        view_rot,
        eye,
        BOUNDS,
        APERTURE_PX,
    );
    let pre_edit_digest = candidate_digest(&pre_edit);
    assert!(pre_edit_digest > 0, "the aperture must catch some wires");
    drop(wires);
    drop(pre_edit);

    // One small geometry edit: the epoch advances, the base index goes stale,
    // and the single changed handle forms the overlay.
    let edited = handles[cursor_cell];
    let mut moved = line_at(cursor_cell);
    moved.common.handle = edited;
    moved.end = Vector3::new(
        moved.end.x + LINE_LENGTH * 0.5,
        moved.end.y - LINE_LENGTH * 0.5,
        0.0,
    );
    assert!(scene.update_entity(EntityType::Line(moved)));

    // Steady state: the overlay is usable, so no background rebuild is ever
    // scheduled — every query below pays the overlay path.
    let resident = scene.hit_test_wires();
    assert!(
        scene
            .interaction_index_build_key(&resident, BOUNDS.height)
            .is_none(),
        "the post-edit steady state must keep scheduling no rebuild"
    );
    drop(resident);

    // The overlay must return exactly the wires the direct pre-edit query
    // saw: the edited line stays inside the aperture and keeps its point
    // count, so the digest cannot move.
    let resident = scene.hit_test_wires();
    let overlay = scene.interaction_hover_candidates_near(
        Arc::clone(&resident),
        cursor,
        view_rot,
        eye,
        BOUNDS,
        APERTURE_PX,
    );
    assert_eq!(
        candidate_digest(&overlay),
        pre_edit_digest,
        "the overlay query must serve the same candidate wires as the direct query"
    );
    drop(overlay);
    drop(resident);

    let pick_median = measure("hover pick  ", || {
        let wires = scene.hit_test_wires();
        let candidates = scene.interaction_hover_candidates_near(
            Arc::clone(&wires),
            black_box(cursor),
            black_box(view_rot),
            black_box(eye),
            BOUNDS,
            black_box(APERTURE_PX),
        );
        candidate_digest(&candidates)
    });
    let snap_median = measure("snap (full) ", || {
        let wires = scene.hit_test_wires();
        let candidates = scene.interaction_candidates_near(
            Arc::clone(&wires),
            black_box(cursor),
            black_box(view_rot),
            black_box(eye),
            BOUNDS,
            black_box(APERTURE_PX),
        );
        candidate_digest(&candidates)
    });

    println!(
        "  ratio snap/pick: {:.2}x (same overlay gather, snap adds feature indexes)",
        snap_median / pick_median
    );
}
