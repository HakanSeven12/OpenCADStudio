//! OpenCADStudio transform_entities dedup benchmark suite.
//!
//! Measures the REAL production entry point `Scene::transform_entities` on a
//! large ROTATE selection in two arms:
//!
//! 1. "constraints elsewhere": 5000 selected lines plus 300 enabled Parallel
//!    constraints between 600 unselected lines. No constraint touches the
//!    selection, so the parametric solver does not run; what remains is the
//!    driven-reference scan that checks every constraint reference against
//!    the selection.
//! 2. "unconstrained": the same 5000-line selection with no constraints. The
//!    pre-edit images and whole-entity refs that only the parametric solve
//!    consumes are not built.
//!
//! Sinks (must be identical before/after the fix): `digest` hashes every
//! selected handle and its post-transform endpoints, and `undo_records` is
//! the number of entity before-images captured by one recorded transform.
//!
//! Deterministic: fixed geometry and a fixed rotation, no randomness.
//! Run: `cargo bench --bench transform_entities_dedup`

use std::hint::black_box;
use std::time::Instant;

use codec::entities::{EntityType, Line};
use codec::types::{Handle, Vector3};
use OpenCADStudio::command::EntityTransform;
use OpenCADStudio::scene::parametric_constraints::{
    ConstraintKind, ParametricRef, ParametricScope,
};
use OpenCADStudio::scene::Scene;

const SELECTED: usize = 5000;
const CONSTRAINTS: usize = 300;
const COLUMNS: usize = 50;
const RUNS: usize = 5;
const ITERATIONS: usize = 8;
const WARMUP: usize = 2;

fn rotate_transform() -> EntityTransform {
    EntityTransform::Rotate {
        center: glam::DVec3::ZERO,
        axis: glam::DVec3::Z,
        angle_rad: 0.005,
    }
}

fn build_fixture(with_constraints: bool) -> (Scene, Vec<Handle>) {
    let mut scene = Scene::new();
    let mut handles = Vec::with_capacity(SELECTED);
    for i in 0..SELECTED {
        let column = i % COLUMNS;
        let row = i / COLUMNS;
        let mut line = Line::new();
        line.start = Vector3::new(column as f64 * 20.0, row as f64 * 20.0, 0.0);
        line.end = Vector3::new(column as f64 * 20.0 + 10.0, row as f64 * 20.0, 0.0);
        handles.push(scene.add_entity(EntityType::Line(line)));
    }
    if with_constraints {
        // Lines outside the selection, so the scan runs but no scope is
        // touched and the solver is not invoked.
        let mut others = Vec::with_capacity(CONSTRAINTS * 2);
        for i in 0..CONSTRAINTS * 2 {
            let x = -1000.0 - i as f64 * 20.0;
            let mut line = Line::new();
            line.start = Vector3::new(x, -1000.0, 0.0);
            line.end = Vector3::new(x + 10.0, -1000.0, 0.0);
            others.push(scene.add_entity(EntityType::Line(line)));
        }
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        for pair in others.chunks_exact(2) {
            set.add(
                ConstraintKind::Parallel,
                vec![
                    ParametricRef::whole(pair[0]),
                    ParametricRef::whole(pair[1]),
                ],
                None,
            );
        }
    }
    (scene, handles)
}

fn mix(digest: &mut u64, value: u64) {
    *digest ^= value;
    *digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
}

fn position_digest(scene: &Scene, handles: &[Handle]) -> u64 {
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    for &handle in handles {
        mix(&mut digest, handle.value());
        let Some(EntityType::Line(line)) = scene.document.get_entity(handle) else {
            continue;
        };
        for value in [
            line.start.x,
            line.start.y,
            line.start.z,
            line.end.x,
            line.end.y,
            line.end.z,
        ] {
            mix(&mut digest, value.to_bits());
        }
    }
    digest
}

fn measure(label: &str, with_constraints: bool) {
    let (mut scene, handles) = build_fixture(with_constraints);
    let rotate = rotate_transform();
    let constraint_count = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .map(|set| set.constraints.len())
        .unwrap_or(0);
    println!(
        "{label}: {SELECTED} selected lines, {constraint_count} constraints, rotation"
    );

    for _ in 0..WARMUP {
        scene.transform_entities(black_box(&handles), black_box(&rotate));
    }

    let mut samples = Vec::with_capacity(RUNS);
    let mut sink = 0u64;
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            scene.transform_entities(black_box(&handles), black_box(&rotate));
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64);
        sink = sink.wrapping_add(position_digest(&scene, &handles));
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[RUNS / 2];

    let digest = position_digest(&scene, &handles);
    scene.begin_undo_recording();
    scene.transform_entities(&handles, &rotate);
    let recording = scene.take_undo_recording().expect("recording was open");
    let (entity_records, object_records, constraint_records, _, _) =
        recording.into_recorded_images();
    let undo_records =
        entity_records.len() + object_records.len() + constraint_records.len();

    println!(
        "  median {median:.4} ms/call  samples {samples:?}  warmup+timed sink {sink}"
    );
    println!("  digest {digest:#018x}  undo_records {undo_records}");
}

fn main() {
    measure("constraints elsewhere", true);
    measure("unconstrained", false);
}
