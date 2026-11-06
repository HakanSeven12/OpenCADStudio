//! OpenCADStudio parametric constraint refresh benchmark suite.
//!
//! `Scene::refresh_parametric_constraints_with_initial_policy` is
//! O(scopes × changes × constraints) on every edit batch.
//!
//! Measures the REAL per-command-commit production entry point: the
//! function `bump_entities` (`src/scene/mod.rs`) calls on every command
//! commit and history apply with the batch's full change list.
//!
//! Fixture: 5 constraint scopes (ModelSpace + 4 blocks) × 50 chained
//! horizontal lines each — 49 Coincident + 50 Horizontal per scope,
//! 495 constraints total — plus 5000
//! unconstrained "noise" lines so a bulk selection can carry 5000 changed
//! handles of which only a handful touch constraints. Every timed call
//! runs with an open undo recording, matching production's command-commit
//! path (ERASE writes scope before-images into the same transaction).
//!
//! Scenarios (each call: `H = 5000` changed handles):
//! - `move-few`: 4995 noise + 5 chain lines → only scope 0 is touched, a
//!   handful of changes hit constraints. The touched check dominates.
//! - `move-many`: 4750 noise + all 250 chain lines → every scope is
//!   touched and solved; the touched check runs over all 5 scopes.
//! - `erase`: 4950 noise + scope 0's 50 lines, all `ChangeKind::Removed`
//!   → the erase-removal loop scans every scope for every removed handle
//!   and records a scope before-image for undo. Constraints are re-added
//!   (untimed) after each call so the per-call workload stays constant.
//!
//! Sinks (must be identical before/after the fix): `sink_result` = summed
//! length of the returned solved-entity change list, `sink_removed` =
//! summed constraint count removed by the erase loop.
//!
//! Deterministic: fixed geometry, fixed change lists, fixed perturbation
//! sequence, no randomness.
//! Run: `cargo bench --bench parametric_constraint_refresh`

use std::hint::black_box;
use std::time::Instant;

use document::entities::{EntityType, Line};
use document::types::{Handle, Vector3};
use OpenCADStudio::scene::parametric_constraints::{
    ConstraintKind, ParametricRef, ParametricScope,
};
use OpenCADStudio::scene::{ChangeKind, Scene};

const SCOPES: usize = 5;
const LINES_PER_SCOPE: usize = 50;
const NOISE_LINES: usize = 5000;
const RUNS: usize = 5;
const ITERATIONS: usize = 8;
const WARMUP: usize = 2;

struct Fixture {
    scene: Scene,
    scopes: Vec<ParametricScope>,
    scope_lines: Vec<Vec<Handle>>,
    noise: Vec<Handle>,
}

/// One horizontal-line chain in `scope`: `lines` Coincident-joined
/// end-to-start and each pinned Horizontal — `2 × lines - 1` constraints.
fn add_chain_constraints(scene: &mut Scene, scope: ParametricScope, lines: &[Handle]) {
    let set = scene.parametric_constraint_set_mut(scope);
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
    for &handle in lines {
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(handle)],
            None,
        );
    }
}

fn build_fixture() -> Fixture {
    let mut scene = Scene::new();
    let mut scopes = Vec::with_capacity(SCOPES);
    let mut scope_lines = Vec::with_capacity(SCOPES);
    for j in 0..SCOPES {
        let scope = if j == 0 {
            ParametricScope::ModelSpace
        } else {
            ParametricScope::Block(Handle::new(1000 + j as u64))
        };
        let mut lines = Vec::with_capacity(LINES_PER_SCOPE);
        for i in 0..LINES_PER_SCOPE {
            let mut line = Line::new();
            let y = j as f64 * 1000.0;
            line.start = Vector3::new(i as f64 * 10.0, y, 0.0);
            line.end = Vector3::new((i as f64 + 1.0) * 10.0, y, 0.0);
            lines.push(scene.add_entity(EntityType::Line(line)));
        }
        add_chain_constraints(&mut scene, scope, &lines);
        scopes.push(scope);
        scope_lines.push(lines);
    }
    let mut noise = Vec::with_capacity(NOISE_LINES);
    for i in 0..NOISE_LINES {
        let mut line = Line::new();
        let y = 100_000.0 + i as f64 * 20.0;
        line.start = Vector3::new(0.0, y, 0.0);
        line.end = Vector3::new(10.0, y, 0.0);
        noise.push(scene.add_entity(EntityType::Line(line)));
    }
    Fixture {
        scene,
        scopes,
        scope_lines,
        noise,
    }
}

fn constraints_count(fixture: &Fixture) -> usize {
    fixture
        .scopes
        .iter()
        .filter_map(|&scope| fixture.scene.parametric_constraint_set(scope))
        .map(|set| set.constraints.len())
        .sum()
}

/// One production MOVE's untimed document edit: shift every changed line
/// slightly so the constraint solve has real work to do each call.
fn perturb(scene: &mut Scene, changes: &[(Handle, ChangeKind)], counter: u64) {
    let step = 0.05 + (counter % 8) as f64 * 0.01;
    for (handle, _) in changes {
        if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(*handle) {
            line.end.x += step;
        }
    }
}

struct Scenario {
    name: &'static str,
    note: &'static str,
    changes: Vec<(Handle, ChangeKind)>,
    perturb: bool,
    reset_after_call: bool,
}

fn run_scenario(fixture: &mut Fixture, scenario: &Scenario) -> (f64, u64, u64) {
    let mut sink_result = 0u64;
    let mut sink_removed = 0u64;
    let mut counter = 0u64;
    let mut samples = Vec::with_capacity(RUNS);
    for run in 0..(WARMUP + RUNS) {
        let mut acc_ms = 0.0;
        for _ in 0..ITERATIONS {
            counter += 1;
            if scenario.perturb {
                perturb(&mut fixture.scene, &scenario.changes, counter);
            }
            let before_count = if scenario.reset_after_call {
                constraints_count(fixture)
            } else {
                0
            };
            fixture.scene.begin_undo_recording();
            let start = Instant::now();
            let result = fixture.scene.refresh_parametric_constraints_with_initial_policy(
                black_box(&scenario.changes),
                black_box(&[]),
                false,
                black_box(&[]),
                black_box(&[]),
            );
            acc_ms += start.elapsed().as_secs_f64() * 1000.0;
            sink_result += result.len() as u64;
            if scenario.reset_after_call {
                let after_count = constraints_count(fixture);
                sink_removed += (before_count - after_count) as u64;
                for (&scope, lines) in fixture.scopes.iter().zip(fixture.scope_lines.iter()) {
                    fixture
                        .scene
                        .parametric_constraint_set_mut(scope)
                        .constraints
                        .clear();
                    add_chain_constraints(&mut fixture.scene, scope, lines);
                }
            }
        }
        if run >= WARMUP {
            samples.push(acc_ms / ITERATIONS as f64);
        }
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[RUNS / 2];
    println!(
        "  {name} median {median:.4} ms/call  samples {samples:?}  sink_result {sink_result}  sink_removed {sink_removed}",
        name = scenario.name,
        samples = format!("{samples:?}"),
    );
    println!("      {}", scenario.note);
    (median, sink_result, sink_removed)
}

fn main() {
    let mut fixture = build_fixture();
    let constraint_count = constraints_count(&fixture);
    let entity_count = SCOPES * LINES_PER_SCOPE + NOISE_LINES;
    println!(
        "constraint refresh: {SCOPES} scopes x {LINES_PER_SCOPE} lines ({constraint_count} constraints), {entity_count} entities, H=5000 changed handles per call"
    );
    println!(
        "  measured: Scene::refresh_parametric_constraints_with_initial_policy (per-command-commit bulk-edit entry point; undo recording open)"
    );

    // `move-few`: 4995 unconstrained + 5 chain lines (scope 0). Handles are
    // appended chain-last so the touched check has to look at every change.
    let mut move_few: Vec<(Handle, ChangeKind)> = fixture.noise[..4995]
        .iter()
        .map(|&handle| (handle, ChangeKind::Modified))
        .collect();
    move_few.extend(
        fixture.scope_lines[0][..5]
            .iter()
            .map(|&handle| (handle, ChangeKind::Modified)),
    );
    // `move-many`: 4750 unconstrained + every chain line — all 5 scopes
    // are touched and fully solved.
    let mut move_many: Vec<(Handle, ChangeKind)> = fixture.noise[..4750]
        .iter()
        .map(|&handle| (handle, ChangeKind::Modified))
        .collect();
    move_many.extend(
        fixture
            .scope_lines
            .iter()
            .flat_map(|lines| lines.iter())
            .map(|&handle| (handle, ChangeKind::Modified)),
    );
    // `erase`: 4950 unconstrained + scope 0's whole chain, all Removed.
    let mut erase: Vec<(Handle, ChangeKind)> = fixture.noise[..4950]
        .iter()
        .map(|&handle| (handle, ChangeKind::Removed))
        .collect();
    erase.extend(
        fixture.scope_lines[0]
            .iter()
            .map(|&handle| (handle, ChangeKind::Removed)),
    );

    let scenarios = [
        Scenario {
            name: "move-few",
            note: "5000 Modified, only 5 touch constraints; scope 0 solves",
            changes: move_few,
            perturb: true,
            reset_after_call: false,
        },
        Scenario {
            name: "move-many",
            note: "5000 Modified, 250 touch constraints; all 5 scopes solve",
            changes: move_many,
            perturb: true,
            reset_after_call: false,
        },
        Scenario {
            name: "erase",
            note: "5000 Removed, 50 touch scope 0's constraints; undo before-images recorded",
            changes: erase,
            perturb: false,
            reset_after_call: true,
        },
    ];

    for scenario in &scenarios {
        run_scenario(&mut fixture, scenario);
    }
}
