//! OpenCADStudio associative-refresh dedup benchmark suite.
//!
//! Every geometry edit funnels through `bump_entities_with_solve_policy`.
//! Each associative family (centre lines, centre marks, dimensions, hatches,
//! sweeps, surfaces, parametric constraints) returns the entities it
//! re-solved, and the policy appends a returned handle to the change list
//! only when that handle is not already there. That membership test is a
//! linear scan of the growing change list, so the dedup alone costs
//! O(changes x refreshed) per family: a bulk MOVE over many associated
//! entities walks the whole change list once per re-solved carrier.
//!
//! Fixture: 6,000 associated centre lines, each between its own pair of
//! source lines, plus those 12,000 source lines (18,000 entities). One
//! `Scene::bump_entities` call for a 12,000-handle bulk edit re-solves all
//! 6,000 carriers, so the dedup loop runs 6,000 times over a change list
//! that starts at 12,000 entries and grows to 18,000. The other families
//! return nothing (no centre marks, dimensions, hatches, sweeps, surfaces or
//! constraints), exactly like the common drawing.
//!
//! Sink: every call must bump the geometry epoch exactly once (the refresh
//! really ran through the production policy), every carrier must sit on the
//! midpoint of its source pair afterwards, and the drawing must still hold
//! 18,000 entities.
//!
//! Deterministic: fixed geometry, no randomness.
//! Run: `cargo bench --bench associative_refresh_dedup`

use std::hint::black_box;
use std::time::Instant;

use codec::entities::{
    CenterLineAssociation, CenterLineSource, CenterLineSourceKind, EntityType, Line,
};
use codec::types::{Handle, Vector3};
use OpenCADStudio::scene::{ChangeKind, Scene};

const CARRIERS: usize = 6_000;
const ITERATIONS: usize = 20;
const RUNS: usize = 3;
const WARMUP: usize = 1;

fn line_at(y: f64) -> Line {
    Line::from_points(
        Vector3::new(0.0, y, 0.0),
        Vector3::new(100.0, y, 0.0),
    )
}

fn association(first: Handle, second: Handle, y: f64) -> CenterLineAssociation {
    CenterLineAssociation {
        first: CenterLineSource {
            handle: first,
            kind: CenterLineSourceKind::Line,
            segment_index: -1,
            pick_point: Vector3::new(50.0, y, 0.0),
        },
        second: CenterLineSource {
            handle: second,
            kind: CenterLineSourceKind::Line,
            segment_index: -1,
            pick_point: Vector3::new(50.0, y + 10.0, 0.0),
        },
        plane_origin: Vector3::new(0.0, 0.0, 0.0),
        plane_x: Vector3::new(1.0, 0.0, 0.0),
        plane_y: Vector3::new(0.0, 1.0, 0.0),
        start_extension: 0.0,
        end_extension: 0.0,
        start_length_adjustment: 0.0,
        end_length_adjustment: 0.0,
        associated: true,
    }
}

fn build_scene() -> (Scene, Vec<Handle>, Vec<Handle>) {
    let mut scene = Scene::new();
    let mut sources = Vec::with_capacity(CARRIERS * 2);
    for index in 0..CARRIERS {
        let y = index as f64 * 20.0;
        sources.push(EntityType::Line(line_at(y)));
        sources.push(EntityType::Line(line_at(y + 10.0)));
    }
    let source_handles = scene.add_entities(sources);

    let mut carriers = Vec::with_capacity(CARRIERS);
    for index in 0..CARRIERS {
        let y = index as f64 * 20.0;
        let mut line = line_at(y + 5.0);
        association(source_handles[index * 2], source_handles[index * 2 + 1], y)
            .write(&mut line.common.extended_data);
        carriers.push(EntityType::Line(line));
    }
    let carrier_handles = scene.add_entities(carriers);
    (scene, source_handles, carrier_handles)
}

fn main() {
    let (mut scene, sources, carriers) = build_scene();
    let entities = scene.document.entities().count();
    assert_eq!(
        entities,
        CARRIERS * 3,
        "each pair contributes two sources and its centre line"
    );
    let changes: Vec<(Handle, ChangeKind)> = sources
        .iter()
        .map(|&handle| (handle, ChangeKind::Modified))
        .collect();

    let bump = |scene: &mut Scene| {
        let before = scene.geometry_epoch;
        scene.bump_entities(black_box(&changes));
        scene.geometry_epoch - before
    };
    for _ in 0..WARMUP {
        black_box(bump(&mut scene));
    }

    let mut best = f64::MAX;
    for run in 0..RUNS {
        let started = Instant::now();
        let mut epochs = 0u64;
        for _ in 0..ITERATIONS {
            epochs += bump(&mut scene);
        }
        let per_edit_us = started.elapsed().as_secs_f64() * 1_000_000.0 / ITERATIONS as f64;
        best = best.min(per_edit_us);
        assert_eq!(epochs, ITERATIONS as u64, "one geometry epoch per edit");
        println!(
            "associative_refresh_dedup: run {run} {ITERATIONS} edits: {per_edit_us:.3} us/edit"
        );
    }

    let mut verified = 0usize;
    for (index, handle) in carriers.iter().enumerate() {
        let Some(EntityType::Line(line)) = scene.document.get_entity(*handle) else {
            panic!("carrier {index} is not a line");
        };
        let y = index as f64 * 20.0 + 5.0;
        assert!(
            (line.start.y - y).abs() < 1e-9
                && (line.end.y - y).abs() < 1e-9
                && line.start.x.abs() < 1e-9
                && (line.end.x - 100.0).abs() < 1e-9,
            "carrier {index} was not refreshed onto its midline"
        );
        verified += 1;
    }
    assert_eq!(scene.document.entities().count(), entities);

    println!(
        "associative_refresh_dedup: {CARRIERS} carriers, {} changed sources, {verified} midlines \
         verified; best {best:.3} us/edit",
        changes.len()
    );
}
