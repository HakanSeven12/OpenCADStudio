//! OpenCADStudio snap intersection sweep benchmark suite.
//!
//! Drives the production snap evaluation entry (`Snapper::snap`) with
//! Intersection and ApparentIntersection snapping over indexed wire
//! candidates on a dense grid of crossing line segments — the per-pointer-move
//! workload that runs the cursor-local world intersection sweep and the
//! screen-space apparent intersection sweep on every draw command.
//!
//! Each evaluation mirrors the production flat-orthographic path: a
//! world-aperture query against the interaction index feeds an indexed
//! `InteractionCandidates`, then the full snap evaluation runs at a fixed
//! deterministic cursor. Sink: the number of evaluations whose best hit is an
//! Intersection or ApparentIntersection snap, plus a world-position checksum —
//! both must be identical across runs and before/after sweep changes.
//!
//! Run: `cargo bench --bench snap_intersection_sweep`

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use document::entities::Line;
use document::types::Vector3;
use document::EntityType;
use glam::{DVec3, Mat4, Vec3};
use iced::{Point, Rectangle};

use OpenCADStudio::scene::pick::interaction_index::InteractionIndex;
use OpenCADStudio::scene::view::camera::Camera;
use OpenCADStudio::scene::Scene;
use OpenCADStudio::snap::{SnapType, Snapper};

#[path = "support/harness.rs"]
mod harness;

use harness::BenchmarkRunner;

const BENCH_METRIC: &str = "snap_intersection_sweep";
/// Horizontal + vertical lines → `2 * LINES_PER_AXIS` single-segment wires.
const LINES_PER_AXIS: usize = 1_200;
const EXTENT: f64 = 78.0;
const SPACING: f64 = EXTENT / (LINES_PER_AXIS - 1) as f64;
/// World units per pixel scale of the flat-orthographic view: a grid cell is
/// ~0.25 px, so the default 25 px aperture spans ~200 crossings per axis —
/// dense in-aperture geometry with the pairwise work budget still respected.
const VIEW_SCALE: f32 = 0.004;
const EVALS_PER_SAMPLE: usize = 150;
const WARMUP_EVALS: usize = 10;
const RUNS: usize = 7;

/// Mirrors the engine's world-to-screen projection for this scale-only view.
fn project(world: DVec3, eye: DVec3, bounds: Rectangle) -> Point {
    let rel = (world - eye).as_vec3() * VIEW_SCALE;
    Point::new(
        (rel.x + 1.0) * 0.5 * bounds.width,
        (1.0 - rel.y) * 0.5 * bounds.height,
    )
}

/// Deterministic low-discrepancy cursor walk across the grid. Cursors sit
/// half a cell off any crossing so sweeps run to their natural end instead of
/// tripping the exact-cursor early exit.
fn cursor_at(step: usize, eye: DVec3, bounds: Rectangle) -> (DVec3, Point) {
    let u = (step as f64 * 0.618_033_988_749_894_9) % 1.0;
    let v = (step as f64 * 0.754_877_666_246_692_7) % 1.0;
    let world = DVec3::new(
        (u * EXTENT + SPACING * 0.37).min(EXTENT - SPACING * 0.1),
        (v * EXTENT + SPACING * 0.61).min(EXTENT - SPACING * 0.1),
        0.0,
    );
    (world, project(world, eye, bounds))
}

fn main() {
    let mut runner = BenchmarkRunner::new(
        "OpenCADStudio Snap Intersection Sweep Benchmark",
        "snap_sweep_metrics.json",
    );
    if !runner.should_run(BENCH_METRIC) {
        return;
    }

    let mut scene = Scene::new();
    for i in 0..LINES_PER_AXIS {
        let c = i as f64 * SPACING;
        let mut horizontal = Line::new();
        horizontal.start = Vector3::new(0.0, c, 0.0);
        horizontal.end = Vector3::new(EXTENT, c, 0.0);
        scene.add_entity(EntityType::Line(horizontal));
        let mut vertical = Line::new();
        vertical.start = Vector3::new(c, 0.0, 0.0);
        vertical.end = Vector3::new(c, EXTENT, 0.0);
        scene.add_entity(EntityType::Line(vertical));
    }

    let cam = Camera::default();
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1000.0);
    let index = InteractionIndex::build(&wires);

    let mut snapper = Snapper::default();
    snapper.snap_enabled = true;
    snapper.snap3d_enabled = false;
    snapper.otrack_enabled = false;
    snapper.enabled = [SnapType::Intersection, SnapType::ApparentIntersection]
        .into_iter()
        .collect();

    let view_rot = Mat4::from_scale(Vec3::splat(VIEW_SCALE));
    let eye = DVec3::new(EXTENT * 0.5, EXTENT * 0.5, 1000.0);
    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };
    // World-space aperture radius exactly as the production flat-ortho query
    // computes it from the snap pixel radius.
    let world_per_px = (VIEW_SCALE * bounds.width * 0.5)
        .min(VIEW_SCALE * bounds.height * 0.5) as f64;
    let radius_world = snapper.osnap_radius_px as f64 / world_per_px;

    let eval_once = |step: usize| -> (bool, u64) {
        let (cursor_world, cursor_screen) = cursor_at(step, eye, bounds);
        let aabb = [
            cursor_world.x - radius_world,
            cursor_world.y - radius_world,
            cursor_world.x + radius_world,
            cursor_world.y + radius_world,
        ];
        let candidates = index.query_xy(Arc::clone(&wires), aabb);
        let hit = snapper.snap(
            cursor_world,
            cursor_screen,
            &candidates,
            view_rot,
            eye,
            bounds,
            Vec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
            None,
        );
        match hit {
            Some(h)
                if matches!(
                    h.snap_type,
                    SnapType::Intersection | SnapType::ApparentIntersection
                ) =>
            {
                (true, h.world.x.to_bits() ^ h.world.y.to_bits())
            }
            _ => (false, 0),
        }
    };

    println!(
        "snap intersection sweep: {} crossing grid segments, {EVALS_PER_SAMPLE} cursor positions/sample, aperture radius {radius_world:.2} world units",
        2 * LINES_PER_AXIS
    );

    let mut all_hits = 0u64;
    let mut checksum = 0u64;
    for step in 0..WARMUP_EVALS {
        let (hit, bits) = eval_once(step);
        all_hits += hit as u64;
        checksum = checksum.wrapping_add(bits);
    }

    let mut samples_us = Vec::with_capacity(RUNS);
    let mut first_sample_hits: Option<u64> = None;
    for _ in 0..RUNS {
        let mut sample_hits = 0u64;
        let start = Instant::now();
        for step in 0..EVALS_PER_SAMPLE {
            let (hit, bits) = eval_once(step);
            sample_hits += hit as u64;
            checksum = checksum.wrapping_add(bits);
        }
        let elapsed = start.elapsed();
        samples_us.push(elapsed.as_secs_f64() * 1_000_000.0 / EVALS_PER_SAMPLE as f64);
        all_hits += sample_hits;
        match first_sample_hits {
            None => first_sample_hits = Some(sample_hits),
            Some(expected) => assert_eq!(
                expected, sample_hits,
                "intersection snap hits must be identical across samples"
            ),
        }
    }

    black_box(checksum);
    let sample_hits = first_sample_hits.unwrap_or_default();
    println!(
        "  intersection-family snap hits: {sample_hits} per sample, {all_hits} total across warmup + {RUNS} samples"
    );
    println!("  world-position checksum: {checksum:#018x}");

    let median_us = {
        let mut sorted = samples_us.clone();
        sorted.sort_by(f64::total_cmp);
        sorted[sorted.len() / 2]
    };
    runner.record(
        BENCH_METRIC,
        "Production snap evaluation with Intersection + ApparentIntersection on an indexed crossing grid",
        "µs",
        samples_us,
        Some((1_000_000.0 / median_us, "snaps/s")),
        None,
    );
    runner.finish();
}
