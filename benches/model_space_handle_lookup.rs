//! OpenCADStudio model-space block handle lookup benchmark suite.
//!
//! `Scene::model_space_block_handle` answers "which block record owns model
//! space?". It first looks for a Layout object named "Model" (DWG files
//! carry one) and otherwise falls back to the conventional `*Model_Space`
//! block record (DXF files). The DWG lookup walks every entry of
//! `document.objects` until it finds the layout, so on a drawing with tens of
//! thousands of objects the answer costs a full map traversal — and the
//! scene asks for it from 18 call sites, several of them per rendered frame
//! (displayed-block checks, resident wire sources, viewport visibility).
//!
//! Measures the REAL production lookup through the doc-hidden
//! `Scene::model_space_block_handle` hook, `CALLS_PER_FRAME` lookups per
//! frame for `FRAMES` frames, with no document edit in between — the steady
//! state of an idle or orbiting viewport. `CALLS_PER_FRAME` is the number of
//! call expressions, i.e. the per-frame cost if every one of them runs.
//!
//! Fixture: a `Document::new()` drawing plus `OBJECTS` XRecord objects and
//! `PAPER_LAYOUTS` paper-space layouts. `Document::new()` also creates a
//! Layout named "Model"; the fixture removes it so the lookup must take the
//! fallback path to the `*Model_Space` block record. That is the worst case
//! of the DWG walk (every object entry is visited) and, unlike a present
//! "Model" layout, it is independent of `std::collections::HashMap`'s
//! per-process iteration order — a "Model" layout is found after a random
//! number of entries, so baseline and fixed runs would otherwise disagree
//! for reasons unrelated to the fix.
//!
//! Sink: the returned handle must equal the document's model-space block
//! record on every call, and must still do so after a geometry bump
//! invalidates any memo.
//!
//! Deterministic: fixed fixture, no randomness.
//! Run: `cargo bench --bench model_space_handle_lookup`

use std::hint::black_box;
use std::time::Instant;

use codec::objects::{ObjectType, XRecord};
use OpenCADStudio::scene::Scene;

const OBJECTS: usize = 20_000;
const PAPER_LAYOUTS: usize = 8;
const CALLS_PER_FRAME: usize = 18;
const FRAMES: usize = 20;
const RUNS: usize = 3;
const WARMUP: usize = 1;

fn build_scene() -> Scene {
    let mut document = codec::Document::new();
    // Force the conventional-name fallback (see the module comment).
    let model_layout = document
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::Layout(layout) if layout.name == "Model" => Some(*handle),
            _ => None,
        })
        .expect("Document::new creates a Model layout");
    document.objects.remove(&model_layout);
    for index in 0..OBJECTS {
        let handle = document.allocate_handle();
        let mut record = XRecord::named(format!("object{index:05}"));
        record.handle = handle;
        record.owner = document.header.named_objects_dict_handle;
        document.objects.insert(handle, ObjectType::XRecord(record));
    }
    for index in 0..PAPER_LAYOUTS {
        document
            .add_layout(&format!("Sheet{:02}", index + 1))
            .expect("the synthesized layout names are unique");
    }
    let mut scene = Scene::new();
    scene.document = document;
    scene
}

fn main() {
    let scene = build_scene();
    let objects = scene.document.objects.len();
    let expected = scene.document.header.model_space_block_handle;
    let model_record = scene
        .document
        .block_records
        .get("*Model_Space")
        .expect("Document::new creates the *Model_Space block record")
        .handle;
    assert_eq!(
        expected, model_record,
        "the fixture's header must name the *Model_Space block record"
    );

    let lookup = |scene: &Scene| black_box(scene.model_space_block_handle()).value();

    for _ in 0..WARMUP {
        let mut sink = 0u64;
        for _ in 0..FRAMES {
            for _ in 0..CALLS_PER_FRAME {
                sink = sink.wrapping_add(lookup(&scene));
            }
        }
        black_box(sink);
    }

    let mut best = f64::MAX;
    for run in 0..RUNS {
        let started = Instant::now();
        let mut sink = 0u64;
        for _ in 0..FRAMES {
            for _ in 0..CALLS_PER_FRAME {
                sink = sink.wrapping_add(lookup(&scene));
            }
        }
        let per_frame_us = started.elapsed().as_secs_f64() * 1_000_000.0 / FRAMES as f64;
        best = best.min(per_frame_us);
        let calls = (FRAMES * CALLS_PER_FRAME) as u64;
        assert_eq!(
            sink,
            expected.value().wrapping_mul(calls),
            "every call must return the model-space block record"
        );
        println!(
            "model_space_handle_lookup: run {run} {FRAMES} frames x {CALLS_PER_FRAME} calls: \
             {per_frame_us:.3} us/frame"
        );
    }

    // A geometry bump re-resolves the answer; it must not change it.
    let mut scene = scene;
    scene.bump_geometry();
    assert_eq!(
        scene.model_space_block_handle(),
        expected,
        "a geometry bump must not change the model-space block record"
    );

    println!(
        "model_space_handle_lookup: {OBJECTS} XRecords, {PAPER_LAYOUTS} paper layouts, \
         {objects} objects total; best {best:.3} us/frame; handle {:#x}",
        expected.value()
    );
}
