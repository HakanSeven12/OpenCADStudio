//! Parametric-chain staging budget benchmark (api 14, the v2 generic
//! shapes — the envelope was reshaped in place; api_version stays 14).
//!
//! Measures the numbers the parametric-display contract is budgeted
//! against:
//!
//! 1. `build_display_entities` for a 50-segment welded chain — short lines
//!    alternating with gentle curvature-ramp sections (linear 0 -> 1/R,
//!    R=-3000, L=40; the record's straight-to-circle spiral as declared
//!    data), seams exact by construction, the staged footprint under the
//!    MAX_PLUGIN_LIVE_ENTITIES ceiling — at the production tolerance 1e-3
//!    m. The commission's budget: under 5 ms.
//! 2. The same builder on a chord-heavy fixture (five section pairs at
//!    R=-300, L=60, ~200 entities) — the per-entity chord cost.
//! 3. `sample_chain`'s per-call latency over the welded chain (the
//!    placement/basis-curve read path). The commission's budget: under
//!    1 microsecond per sample.
//! 4. The grip-edit review (the drag-policy decision input): per-move
//!    host-local re-derivation cost when a drag revises the section's end
//!    curvature through its own ramp data — the measured-chord extreme
//!    (the linear-curvature ramp) against the exact-carrier extreme (the
//!    constant-curvature arc), plus the point-projection tier on the same
//!    design.
//! 5. The Sampled tier (the new stage-time cumulative table): a chain
//!    whose sections carry dense sampled slope/curvature tables — the
//!    staging budget re-measured for table laws, then per-move
//!    re-derivations with the ramp's end value dragged across the table's
//!    tail (the cumulative's own reading).
//!
//! Deterministic: fixed fixtures, no randomness. Run:
//! `cargo bench --bench parametric_display_staging`

use std::f64::consts::PI;
use std::hint::black_box;
use std::time::{Duration, Instant};

use ocs_plugin_api::host::{
    CurveSegmentDef, MAX_PLUGIN_LIVE_ENTITIES, ParametricChain, ParametricPlane,
    ParametricSegment, Ramp,
};
use OpenCADStudio::parametric::{build_display_entities, sample_chain};

const RUNS: usize = 5;
const WARMUP: usize = 2;
const SAMPLES: usize = 2000;
const WELDED_PAIRS: usize = 25;
const CHORD_PAIRS: usize = 5;
const GRIP_MOVES: usize = 1000;

fn ramp_const(value: f64) -> Ramp {
    Ramp::Polynomial {
        degree: 0,
        coeffs: [value, 0.0, 0.0, 0.0],
    }
}

fn ramp_linear(start: f64, end: f64) -> Ramp {
    Ramp::Polynomial {
        degree: 1,
        coeffs: [start, end - start, 0.0, 0.0],
    }
}

fn stepped(point: [f64; 2], direction: f64, length: f64) -> [f64; 2] {
    [
        point[0] + length * direction.cos(),
        point[1] + length * direction.sin(),
    ]
}

fn push_line(
    segments: &mut Vec<ParametricSegment>,
    cursor: &mut [f64; 2],
    heading: f64,
    length: f64,
) {
    segments.push(ParametricSegment {
        def: CurveSegmentDef {
            length,
            ramp: ramp_const(0.0),
            cross_slope: None,
        },
        start_point: *cursor,
        start_direction: heading,
    });
    *cursor = stepped(*cursor, heading, length);
}

/// The plan's linear-curvature section (kappa 0 -> 1/r over the arc
/// length): the Fresnel-spiral law as ramp data.
fn spiral_section(
    r: f64,
    cursor: [f64; 2],
    heading: f64,
    length: f64,
) -> (ParametricSegment, f64, [f64; 2]) {
    let segment = ParametricSegment {
        def: CurveSegmentDef {
            length,
            ramp: ramp_linear(0.0, 1.0 / r),
            cross_slope: None,
        },
        start_point: cursor,
        start_direction: heading,
    };
    let dtheta = length / (2.0 * r);
    let end = {
        // The Fresnel series places the end (the engine's own oracle math).
        let b = 1.0 / (2.0 * r * length);
        spiral_end(b, length)
    };
    (
        segment,
        heading + dtheta,
        [
            cursor[0] + end.0 * heading.cos() - end.1 * heading.sin(),
            cursor[1] + end.0 * heading.sin() + end.1 * heading.cos(),
        ],
    )
}

fn spiral_end(b: f64, s: f64) -> (f64, f64) {
    fn factorial(n: usize) -> f64 {
        (1..=n as u64).fold(1u128, |acc: u128, v| acc.saturating_mul(v as u128)) as f64
    }
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for k in 0..12usize {
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        x += sign * b.powi(2 * k as i32) * s.powi(4 * k as i32 + 1)
            / (factorial(2 * k) * (4.0 * k as f64 + 1.0));
        y += sign * b.powi(2 * k as i32 + 1) * s.powi(4 * k as i32 + 3)
            / (factorial(2 * k + 1) * (4.0 * k as f64 + 3.0));
    }
    (x, y)
}

/// The cosine-transition law as dense SAMPLED curvature data: kappa(xi) =
/// (1/R) * (1 - cos(pi*xi)) / 2 — the heading half-wave the record's
/// cosine transition restates as a table.
fn sampled_cosine_kappa(r: f64, n: usize) -> Ramp {
    let values: Vec<f64> = (0..n)
        .map(|i| {
            let xi = i as f64 / (n - 1) as f64;
            (1.0 / r) * 0.5 * (1.0 - (PI * xi).cos())
        })
        .collect();
    Ramp::Sampled { n: n as u32, values }
}

/// The welded chain of lines + dense SAMPLED-ramp sections, the seams
/// declared by the engine's own sampling (each front's end re-declares
/// the next section's continuity; no duplicated math at the fixture).
fn welded_sampled_chain(
    r: f64,
    line_length: f64,
    section_length: f64,
    pairs: usize,
) -> ParametricChain {
    let mut segments: Vec<ParametricSegment> = Vec::new();
    let mut cursor = [0.0f64, 0.0];
    let mut heading = 0.0f64;
    let mut front = ParametricChain {
        plane: ParametricPlane::Plan,
        segments: Vec::new(),
        layer: String::new(),
    };
    let advance = |front: &ParametricChain| -> ([f64; 2], f64) {
        let total: f64 = front.segments.iter().map(|s| s.def.length).sum();
        let joint = sample_chain(front, total)
            .unwrap_or_else(|e| panic!("the sampled fixture's front samples: {e}"))
            .expect("the front's own end samples");
        (
            joint.point,
            joint.direction[1].atan2(joint.direction[0]),
        )
    };
    for _ in 0..pairs {
        segments.push(ParametricSegment {
            def: CurveSegmentDef {
                length: line_length,
                ramp: ramp_const(0.0),
                cross_slope: None,
            },
            start_point: cursor,
            start_direction: heading,
        });
        front.segments = segments.clone();
        let (point, direction) = advance(&front);
        cursor = point;
        heading = direction;
        segments.push(ParametricSegment {
            def: CurveSegmentDef {
                length: section_length,
                ramp: sampled_cosine_kappa(r, 513),
                cross_slope: None,
            },
            start_point: cursor,
            start_direction: heading,
        });
        front.segments = segments.clone();
        let (point, direction) = advance(&front);
        cursor = point;
        heading = direction;
    }
    ParametricChain {
        plane: ParametricPlane::Plan,
        segments,
        layer: String::new(),
    }
}

/// The welded chain (lines + sections of the given ramp builder), the
/// seams placed by construction.
fn welded_chain(
    r: f64,
    line_length: f64,
    section_length: f64,
    pairs: usize,
    section: impl Fn(f64, [f64; 2], f64, f64) -> (ParametricSegment, f64, [f64; 2]),
) -> ParametricChain {
    let mut segments = Vec::new();
    let mut cursor = [0.0f64, 0.0];
    let mut heading = 0.0f64;
    for _ in 0..pairs {
        push_line(&mut segments, &mut cursor, heading, line_length);
        let (segment, next_heading, next_cursor) = section(r, cursor, heading, section_length);
        segments.push(segment);
        cursor = next_cursor;
        heading = next_heading;
    }
    ParametricChain {
        plane: ParametricPlane::Plan,
        segments,
        layer: String::new(),
    }
}

fn stage_and_time(chain: &ParametricChain, label: &str) -> (Duration, usize) {
    let (entities, _) =
        build_display_entities(chain, 1e-3).unwrap_or_else(|e| panic!("{label}: {e}"));
    let count = entities.len();
    for _ in 0..WARMUP {
        black_box(build_display_entities(chain, 1e-3))
            .unwrap_or_else(|e| panic!("{label} warmup: {e}"));
    }
    let mut total = Duration::ZERO;
    for _ in 0..RUNS {
        let start = Instant::now();
        black_box(build_display_entities(chain, 1e-3))
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        total += start.elapsed();
    }
    (total / RUNS as u32, count)
}

fn stage_once(chain: &ParametricChain, label: &str) -> (Duration, usize) {
    let (entities, _) =
        build_display_entities(chain, 1e-3).unwrap_or_else(|e| panic!("{label}: {e}"));
    let count = entities.len();
    for _ in 0..WARMUP {
        black_box(build_display_entities(chain, 1e-3))
            .unwrap_or_else(|e| panic!("{label} warmup: {e}"));
    }
    let start = Instant::now();
    black_box(build_display_entities(chain, 1e-3))
        .unwrap_or_else(|e| panic!("{label}: {e}"));
    (start.elapsed(), count)
}

fn main() {
    // Budget 1: the 50-segment welded chain (under the cap), < 5 ms.
    let chain = welded_chain(-3000.0, 2.0, 40.0, WELDED_PAIRS, spiral_section);
    let staging_mean = stage_and_time(&chain, "welded chain").0;
    let (entities, _) = build_display_entities(&chain, 1e-3).expect("welded chain stages");
    let entity_count = entities.len();
    assert!(
        entity_count <= MAX_PLUGIN_LIVE_ENTITIES,
        "fixture must fit under the live cap, got {entity_count}"
    );
    println!("staging: welded {WELDED_PAIRS}-pair chain -> {entity_count} display entities");
    println!("staging: mean {staging_mean:?} over {RUNS} runs (budget < 5 ms)");
    assert!(
        staging_mean < Duration::from_millis(5),
        "staging budget exceeded: {staging_mean:?}"
    );

    // Budget 2: the chord-heavy fixture (~200 entities at 1e-3).
    let chord_chain = welded_chain(-300.0, 2.0, 60.0, CHORD_PAIRS, spiral_section);
    let (elapsed, chord_count) = stage_once(&chord_chain, "chord-heavy chain");
    println!(
        "staging (chord-heavy {CHORD_PAIRS}-pair, {chord_count} entities): {elapsed:?}"
    );

    // Budget 3: sample latency under 1 us.
    let total: f64 = chain.segments.iter().map(|s| s.def.length).sum();
    for _ in 0..WARMUP {
        black_box(sample_chain(&chain, total * 0.5).unwrap());
    }
    let start = Instant::now();
    let mut acc = 0.0f64;
    for i in 0..SAMPLES {
        let s = total * i as f64 / SAMPLES as f64;
        if let Some(sample) = sample_chain(black_box(&chain), s).unwrap() {
            acc += sample.point[0] + sample.point[1];
        }
    }
    let elapsed = start.elapsed();
    let per_sample = elapsed / SAMPLES as u32;
    println!("sample: {SAMPLES} calls in {elapsed:?} -> {per_sample:?} per call");
    println!("sample: digest {acc:.3} over {total} m of chain");
    black_box(acc);

    // Budget 5: the SAMPLED tier — the stage-time cumulative table for
    // table laws: the staging budget for dense sampled tables, then the
    // grip tier below.
    let sampled_chain = welded_sampled_chain(-300.0, 2.0, 60.0, 2);
    let (sampled_mean, sampled_count) = stage_and_time(&sampled_chain, "sampled chain");
    println!(
        "staging (sampled tables, {sampled_count} entities): mean {sampled_mean:?} over {RUNS} runs"
    );
    black_box(sampled_mean);

    // Grip-edit review (the drag-policy decision input).
    //
    // Per-move cost of HOST-LOCAL re-derivation for a grip edit: the drag
    // revises the section's end curvature through its own ramp data and
    // the host re-derives the live display set per coalesced cursor move
    // — no IPC — instead of the plugin streaming preview/live-entity
    // geometry per move (the v12 semantics). The ramp arms at the
    // derivation-cost extremes:
    //
    // - the linear-curvature ramp: the spiral case — every move re-runs
    //   the Gauss-Legendre integration and the measured-deviation chord
    //   refinement;
    // - the constant-curvature ramp: the exact arc carrier — one closed
    //   entity, the cheapest possible per-move re-derivation;
    // - the dense sampled table (the new cumulative tier): the moves drag
    //   the table's END value; the precomputed slot-integral table keeps
    //   the re-derivation's integral reads to a lookup + one exacting
    //   tail.
    //
    // The point-projection tier (`sample_chain` at the cursor's station)
    // is measured on the same dragged design.
    let base_r = -300.0f64;
    for (label, ramp_at) in [
        (
            "spiral ramp (measured chords)",
            Box::new(|r: f64| ramp_linear(0.0, 1.0 / r)) as Box<dyn Fn(f64) -> Ramp>,
        ),
        (
            "constant ramp (exact arc)",
            Box::new(|r: f64| ramp_const(1.0 / r)) as Box<dyn Fn(f64) -> Ramp>,
        ),
        (
            "sampled table (cumulative tier)",
            Box::new(|r: f64| sampled_cosine_kappa(r, 257)) as Box<dyn Fn(f64) -> Ramp>,
        ),
    ] {
        let mut chain = grip_edit_chain(ramp_at(base_r));
        let (per_move_count, _) = stage_once(&chain, &format!("{label} fixture"));
        let _ = per_move_count;
        // (a) Full re-derivation per move; the drag sweeps +-3% around the
        // base end radius, deterministic.
        let mut build_total = Duration::ZERO;
        let mut digest = 0.0f64;
        for m in 0..GRIP_MOVES {
            let t = m as f64 / GRIP_MOVES as f64;
            let r = base_r * (1.0 + 0.03 * (2.0 * t - 1.0));
            chain.segments[1].def.ramp = ramp_at(r);
            let start = Instant::now();
            black_box(build_display_entities(&chain, 1e-3).expect("move re-derivation stages"));
            build_total += start.elapsed();
            digest += (r + 1.0) * 0.001;
        }
        let per_move = build_total / GRIP_MOVES as u32;
        assert!(
            per_move < Duration::from_millis(5),
            "grip-edit re-derivation blew the frame budget: {per_move:?} per move"
        );
        println!(
            "grip_edit ({label}): {GRIP_MOVES} host-local re-derivations, mean {per_move:?} \
             per move (60 fps frame = 16.7 ms)",
        );
        // (b) The projection tier on the same dragged design.
        let mut sample_total = Duration::ZERO;
        for m in 0..GRIP_MOVES {
            let t = m as f64 / GRIP_MOVES as f64;
            let r = base_r * (1.0 + 0.03 * (2.0 * t - 1.0));
            chain.segments[1].def.ramp = ramp_at(r);
            let start = Instant::now();
            black_box(sample_chain(&chain, 30.0).unwrap());
            sample_total += start.elapsed();
        }
        let per_projection = sample_total / GRIP_MOVES as u32;
        println!("grip_edit ({label}): projection tier mean {per_projection:?} per move");
        println!("grip_edit ({label}): digest {digest:.3}");
        black_box(digest);
    }
}

/// A {line, section} plan chain with the 60 m section carrying the ramp
/// as the dragged parameter. The section is the LAST segment: no
/// downstream seam restates its end, so the re-derivation cost is the
/// pure per-ramp derivation.
fn grip_edit_chain(ramp: Ramp) -> ParametricChain {
    let mut segments = vec![ParametricSegment {
        def: CurveSegmentDef {
            length: 2.0,
            ramp: ramp_const(0.0),
            cross_slope: None,
        },
        start_point: [0.0; 2],
        start_direction: 0.0,
    }];
    segments.push(ParametricSegment {
        def: CurveSegmentDef {
            length: 60.0,
            ramp,
            cross_slope: None,
        },
        start_point: stepped([0.0; 2], 0.0, 2.0),
        start_direction: 0.0,
    });
    ParametricChain {
        plane: ParametricPlane::Plan,
        segments,
        layer: String::new(),
    }
}
