//! OpenCADStudio associative-dimension refresh benchmark suite.
//!
//! Every geometry edit runs `refresh_associative_dimensions` so dimensions
//! whose sources moved are re-measured. The refresh starts by walking every
//! entry of `document.objects` looking for DIMASSOC dimension associations.
//! Drawings without any (the common case) pay that full object-graph scan on
//! every edit and then refresh nothing.
//!
//! Measures the REAL production entry point `Scene::refresh_associative_dimensions`
//! (doc-hidden public) once per edited handle, on a 20,017-object drawing
//! that holds no DIMASSOC association. This is the steady state after the
//! one-time "does this drawing have associations?" pass: the fixed revision
//! answers from a memoised hint and returns without touching the objects,
//! the baseline scans the whole map for every edit. Commands that create or
//! erase an entity re-open the question, so those pay one scan on their next
//! edit; move/rotate/dimension-value edits keep the answer.
//!
//! Sink: the returned change list must stay empty (there is nothing to
//! refresh) and the drawing's object count must be unchanged, so a revision
//! that silently started skipping real associations elsewhere would not be
//! detectable here — that is covered by the library test
//! `dimension_association_gate_tracks_creation_and_reset` and the existing
//! dimension/associativity suites.
//!
//! Deterministic: fixed fixture, no randomness.
//! Run: `cargo bench --bench dimension_association_refresh`

use std::hint::black_box;
use std::time::Instant;

use codec::entities::{EntityType, Line};
use codec::objects::{ObjectType, XRecord};
use codec::types::{Handle, Vector3};
use OpenCADStudio::scene::{ChangeKind, Scene};

const OBJECTS: usize = 20_000;
const ITERATIONS: usize = 200;
const RUNS: usize = 3;
const WARMUP: usize = 1;

fn build_scene() -> (Scene, Handle) {
    let mut document = codec::Document::new();
    for index in 0..OBJECTS {
        let handle = document.allocate_handle();
        let mut record = XRecord::named(format!("object{index:05}"));
        record.handle = handle;
        record.owner = document.header.named_objects_dict_handle;
        document.objects.insert(handle, ObjectType::XRecord(record));
    }
    let mut scene = Scene::new();
    scene.document = document;
    let line = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(100.0, 0.0, 0.0),
    )));
    (scene, line)
}

fn main() {
    let (mut scene, line) = build_scene();
    let objects = scene.document.objects.len();
    let changes = [(line, ChangeKind::Modified)];
    let refresh = |scene: &mut Scene| {
        let refreshed = scene.refresh_associative_dimensions(black_box(&changes));
        (refreshed.len(), refreshed.iter().map(|(handle, _)| handle.value()).sum::<u64>())
    };

    for _ in 0..WARMUP {
        black_box(refresh(&mut scene));
    }

    let mut best = f64::MAX;
    for run in 0..RUNS {
        let started = Instant::now();
        let mut sink = (0usize, 0u64);
        for _ in 0..ITERATIONS {
            sink = refresh(&mut scene);
        }
        let per_edit_us = started.elapsed().as_secs_f64() * 1_000_000.0 / ITERATIONS as f64;
        best = best.min(per_edit_us);
        assert_eq!(sink, (0, 0), "nothing to refresh without associations");
        println!(
            "dimension_association_refresh: run {run} {ITERATIONS} edits: {per_edit_us:.3} us/edit"
        );
    }
    assert_eq!(scene.document.objects.len(), objects, "the scan must not mutate the drawing");

    println!(
        "dimension_association_refresh: {OBJECTS} XRecords, {objects} objects, no DIMASSOC; \
         best {best:.3} us/edit"
    );
}
