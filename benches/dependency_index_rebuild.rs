//! OpenCADStudio dependency-index rebuild benchmark.
//!
//! Any edit that can move an entity between style targets (add, erase, paste,
//! undo, a layer or style change) drops `Scene`'s dependency index, and the
//! next style query rebuilds it from scratch: one full pass over every entity
//! for block roots, and a second one for signatures and per-style category
//! maps. On a 100k-entity drawing that scan runs on the next LAYER/STYLE
//! command or the first query after an undo.
//!
//! Fixture: 96,950 model lines on layer "0", 1,000 texts on layer "NOTES"
//! with text style "Standard", and 1,000 inserts of a 50-line block, so the
//! rebuild exercises the block-root fixed point, per-entity membership
//! lookups, and the per-style maps. The timed call is the real production
//! rebuild behind `Scene::dependency_targets`.
//!
//! Sink: the rebuild's key counts are asserted every run — 2 layer keys, 1
//! text-style key, and one block root covering all 1,000 inserts.
//!
//! Deterministic: fixed geometry, no randomness.
//! Run: `cargo bench --bench dependency_index_rebuild`

use std::hint::black_box;
use std::time::Instant;

use codec::entities::{EntityType, Insert, Line, Text};
use codec::tables::BlockRecord;
use codec::types::Vector3;
use OpenCADStudio::scene::Scene;

const BLOCK_LINES: usize = 50;
const INSERTS: usize = 1_000;
const TEXTS: usize = 2_000;
const LINES: usize = 96_950;
const RUNS: usize = 5;
const ITERATIONS: usize = 4;
const WARMUP: usize = 2;

fn fixture() -> Scene {
    let mut scene = Scene::new();

    // The referenced block and its member lines, owned by the block record.
    let mut bracket = BlockRecord::new("Bracket");
    bracket.handle = scene.document.allocate_handle();
    let bracket_handle = bracket.handle;
    scene
        .document
        .block_records
        .add(bracket)
        .expect("the block name is unique");
    for index in 0..BLOCK_LINES {
        let mut line = Line::from_points(
            Vector3::new(index as f64, 0.0, 0.0),
            Vector3::new(index as f64, 10.0, 0.0),
        );
        line.common.owner_handle = bracket_handle;
        scene
            .document
            .add_entity(EntityType::Line(line))
            .expect("the block member is accepted");
    }

    for index in 0..LINES {
        let line = Line::from_points(
            Vector3::new(0.0, index as f64, 0.0),
            Vector3::new(100.0, index as f64, 0.0),
        );
        scene.add_entity(EntityType::Line(line));
    }
    for index in 0..TEXTS {
        let mut text =
            Text::with_value(format!("note {index}"), Vector3::new(0.0, index as f64, 0.0));
        text.common.layer = "NOTES".to_string();
        scene.add_entity(EntityType::Text(text));
    }
    for index in 0..INSERTS {
        let insert = Insert::new("Bracket", Vector3::new(index as f64 * 20.0, 0.0, 0.0));
        scene.add_entity(EntityType::Insert(insert));
    }
    scene
}

fn main() {
    let scene = fixture();
    assert_eq!(
        scene.document.entities().count(),
        LINES + BLOCK_LINES + TEXTS + INSERTS,
        "the fixture entity count"
    );
    for _ in 0..WARMUP {
        black_box(scene.dependency_index_rebuild_for_bench());
    }

    let mut best = f64::MAX;
    for run in 0..RUNS {
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(scene.dependency_index_rebuild_for_bench());
        }
        let per_rebuild = started.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64;
        best = best.min(per_rebuild);
        println!("dependency_index_rebuild: run {run} {ITERATIONS} rebuilds: {per_rebuild:.3} ms/rebuild");
    }

    let (layers, text_styles, root_handles) = scene.dependency_index_rebuild_for_bench();
    assert_eq!(
        (layers, text_styles, root_handles),
        (2, 1, INSERTS),
        "layer keys, text-style keys and block-root handles"
    );
    println!(
        "dependency_index_rebuild: {LINES} lines + {BLOCK_LINES} block lines + {INSERTS} inserts + {TEXTS} texts; \
         best {best:.3} ms/rebuild"
    );
}
