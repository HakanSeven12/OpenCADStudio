//! OpenCADStudio centre-line / centre-mark associative refresh benchmark.
//!
//! Once a drawing holds a single associative centre line or centre mark, every
//! geometry edit walks every entity looking for centre carriers: the refresh
//! filters for `EntityType::Line` and parses each line's extended data for the
//! association record. The cost is therefore O(drawing) per edit even when the
//! edit touches nothing near a carrier.
//!
//! Fixture: 100,000 lines in model space, one of them an associative centre
//! line between the first two lines. Each timed edit moves the last line — a
//! line unrelated to the carrier — so the refresh has nothing to re-solve and
//! the whole cost is the scan itself. The line count is the tracker's
//! "≥1 centre line among 100k lines" case.
//!
//! Sink: every call bumps the geometry epoch exactly once, the drawing keeps
//! its line count, and after timing one carrier source is moved by 2 units and
//! the carrier must follow to the new midpoint — so a revision that stopped
//! finding carriers would be caught.
//!
//! Deterministic: fixed geometry, no randomness.
//! Run: `cargo bench --bench centerline_refresh_scan`

use std::hint::black_box;
use std::time::Instant;

use codec::entities::{
    CenterLineAssociation, CenterLineSource, CenterLineSourceKind, EntityType, Line,
};
use codec::types::{Handle, Vector3};
use OpenCADStudio::scene::{ChangeKind, Scene};

const LINES: usize = 100_000;
const ITERATIONS: usize = 100;
const RUNS: usize = 3;
const WARMUP: usize = 1;

fn line_at(y: f64) -> Line {
    Line::from_points(
        Vector3::new(0.0, y, 0.0),
        Vector3::new(100.0, y, 0.0),
    )
}

fn association(first: Handle, second: Handle) -> CenterLineAssociation {
    CenterLineAssociation {
        first: CenterLineSource {
            handle: first,
            kind: CenterLineSourceKind::Line,
            segment_index: -1,
            pick_point: Vector3::new(50.0, 0.0, 0.0),
        },
        second: CenterLineSource {
            handle: second,
            kind: CenterLineSourceKind::Line,
            segment_index: -1,
            pick_point: Vector3::new(50.0, 10.0, 0.0),
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

fn build_scene() -> (Scene, Vec<Handle>, Handle) {
    let mut scene = Scene::new();
    let mut lines = Vec::with_capacity(LINES);
    for index in 0..LINES {
        lines.push(EntityType::Line(line_at(index as f64 * 10.0)));
    }
    let handles = scene.add_entities(lines);

    let mut carrier = line_at(5.0);
    association(handles[0], handles[1]).write(&mut carrier.common.extended_data);
    scene.add_entity(EntityType::Line(carrier));
    let unrelated = handles[LINES - 1];
    (scene, handles, unrelated)
}

fn carrier_y(scene: &Scene, handle: Handle) -> f64 {
    let Some(EntityType::Line(line)) = scene.document.get_entity(handle) else {
        panic!("the centre line carrier is not a line");
    };
    assert!((line.start.y - line.end.y).abs() < 1e-9, "carrier is not horizontal");
    line.start.y
}

fn main() {
    let (mut scene, handles, unrelated) = build_scene();
    let entities = scene.document.entities().count();
    assert_eq!(entities, LINES + 1, "100k lines and their centre line");
    let changes = [(unrelated, ChangeKind::Modified)];

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
        println!("centerline_refresh_scan: run {run} {ITERATIONS} edits: {per_edit_us:.3} us/edit");
    }

    // The carrier is attached to the first two lines. Moving source line 0 up
    // by 2 must move the midpoint from y=5 to y=6.
    let carrier = {
        let mut found = None;
        for entity in scene.document.entities() {
            if let EntityType::Line(line) = entity {
                if CenterLineAssociation::read(&line.common.extended_data).is_some() {
                    found = Some(line.common.handle);
                }
            }
        }
        found.expect("the fixture centre line")
    };
    assert!((carrier_y(&scene, carrier) - 5.0).abs() < 1e-9);
    if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(handles[0]) {
        line.start.y = 2.0;
        line.end.y = 2.0;
    }
    scene.bump_entities(&[(handles[0], ChangeKind::Modified)]);
    assert!(
        (carrier_y(&scene, carrier) - 6.0).abs() < 1e-9,
        "the cached index must still find the carrier after its source moved"
    );
    assert_eq!(scene.document.entities().count(), entities);

    println!(
        "centerline_refresh_scan: {LINES} lines + 1 associative centre line, \
         {ITERATIONS} edits x {RUNS} runs; best {best:.3} us/edit"
    );
}
