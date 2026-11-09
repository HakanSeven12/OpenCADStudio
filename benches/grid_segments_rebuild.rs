//! OpenCADStudio grid overlay rebuild benchmark suite.
//!
//! `grid_segments` rebuild cost on the camera-motion hot path.
//!
//! `GridKey` (src/ui/overlay.rs) embeds the exact eye/view/bounds, so the grid
//! overlay cache only hits when the camera is byte-identical. Every pan, zoom
//! or orbit frame misses and rebuilds via `grid_segments` — the path this bench
//! times. Each op replays a 10-step incremental pan of one plausible camera
//! (perspective, ~56° elevation orbit, adaptive 1-unit GRIDUNIT) around the
//! world origin, calling the real production `grid_segments` once per step.
//!
//! The camera and pan positions are precomputed outside the timed region, so
//! the sample measures the geometry rebuild itself (extent tracing, per-line
//! visibility sampling, horizon trims), not matrix setup.
//!
//! Correctness sink: the total number of emitted segments (minor + major) over
//! the whole pan sequence. It must be identical before/after any change to
//! `grid_segments` — the grid must emit the same lines with the same trims.
//!
//! Run: `cargo bench --bench grid_segments_rebuild`

use std::hint::black_box;
use std::time::Instant;

use glam::{DVec3, Mat4};
use iced::Rectangle;

use OpenCADStudio::scene::view::camera::{Camera, Projection};
use OpenCADStudio::ui::overlay::{compute_grid_steps, grid_segments, GridParams, GridStyle};

/// Cache-miss rebuilds per op — one per frame of a short pan gesture.
const PAN_STEPS: usize = 10;
/// Timed samples; the reported figure is the sample median.
const RUNS: usize = 7;
/// Ops per sample (each op = `PAN_STEPS` rebuilds).
const ITERATIONS: usize = 20;
const WARMUP: usize = 2;

/// Pan increment in world units per step: ~12 minor grid spacings, the scale
/// of a typical hand-pan across a drawing.
const PAN_DELTA: DVec3 = DVec3::new(150.0, 70.0, 0.0);

fn viewport(width: f32, height: f32) -> Rectangle {
    Rectangle {
        x: 0.0,
        y: 0.0,
        width,
        height,
    }
}

/// The plausible camera: perspective, orbited ~30° off the top view, 8000
/// units above/around the drawing origin — the kind of mid-zoom orbit where
/// grid lines run all the way to the perspective horizon.
fn base_camera() -> Camera {
    let mut cam = Camera::default();
    cam.target = DVec3::new(250.0, -120.0, 0.0);
    cam.rotation = glam::Quat::from_rotation_y(0.4) * glam::Quat::from_rotation_x(0.55);
    cam.distance = 8000.0;
    cam.fov_y = 45.0_f32.to_radians();
    cam.projection = Projection::Perspective;
    cam
}

/// Precomputed `(view_rot, eye, step_x, step_y)` per pan step — the exact
/// inputs `grid_segments` receives every rebuild during navigation.
fn pan_sequence(bounds: Rectangle) -> Vec<(Mat4, DVec3, f32, f32)> {
    let base = base_camera();
    let (step_x, step_y) = compute_grid_steps(1.0, 1.0, base.distance, base.fov_y, bounds, true);
    (0..PAN_STEPS)
        .map(|i| {
            let mut cam = base.clone();
            cam.target += PAN_DELTA * i as f64;
            (cam.view_proj_rte(bounds), cam.eye(), step_x, step_y)
        })
        .collect()
}

/// Production-faithful `GridParams` / `GridStyle` for the given frame — built
/// once per op so the timed region is pure `grid_segments`.
fn frame_params(
    bounds: Rectangle,
    view_rot: Mat4,
    eye: DVec3,
    step_x: f32,
    step_y: f32,
) -> (GridParams, GridStyle) {
    let params = GridParams {
        view_rot,
        eye,
        bounds,
        step_x,
        step_y,
        major_every: 5,
        origin: DVec3::ZERO,
        axes: (glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z),
        limits: None,
    };
    (params, GridStyle::default())
}

/// Emitted segment count for one full pan sequence — the correctness sink.
fn sequence_lines(bounds: Rectangle, sequence: &[(Mat4, DVec3, f32, f32)]) -> usize {
    sequence
        .iter()
        .map(|&(view_rot, eye, step_x, step_y)| {
            let (params, _style) = frame_params(bounds, view_rot, eye, step_x, step_y);
            let g = grid_segments(
                params.view_rot,
                params.eye,
                params.bounds,
                params.step_x,
                params.step_y,
                params.major_every,
                params.origin,
                params.axes,
                params.limits,
            );
            g.segments.len() + g.major_segments.len()
        })
        .sum()
}

/// One op: `PAN_STEPS` cache-miss rebuilds along the pan. Inputs and outputs
/// are black-boxed; the returned segment count is the sink.
fn op(bounds: Rectangle, sequence: &[(Mat4, DVec3, f32, f32)]) -> usize {
    let mut lines = 0usize;
    for &(view_rot, eye, step_x, step_y) in sequence {
        let (params, style) = frame_params(bounds, view_rot, eye, step_x, step_y);
        let g = grid_segments(
            black_box(params.view_rot),
            black_box(params.eye),
            black_box(params.bounds),
            black_box(params.step_x),
            black_box(params.step_y),
            black_box(params.major_every),
            black_box(params.origin),
            black_box(params.axes),
            black_box(params.limits),
        );
        black_box(&style);
        lines += black_box(g.segments.len() + g.major_segments.len());
        black_box(g);
    }
    lines
}

/// Median ms/op over `RUNS` samples; the sink must be constant across every
/// warmup and sample or the measurement is meaningless.
fn measure(bounds: Rectangle, sequence: &[(Mat4, DVec3, f32, f32)]) -> (f64, usize) {
    let mut sink = 0usize;
    for _ in 0..WARMUP {
        sink = sink.wrapping_add(op(bounds, black_box(sequence)));
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            sink = sink.wrapping_add(op(bounds, black_box(sequence)));
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64);
    }
    samples.sort_by(f64::total_cmp);
    (samples[RUNS / 2], sink)
}

fn main() {
    let mut total_sink = 0usize;
    for (width, height) in [(1920.0_f32, 1080.0_f32), (2560.0, 1440.0)] {
        let bounds = viewport(width, height);
        let sequence = pan_sequence(bounds);
        let lines_per_op = sequence_lines(bounds, &sequence);
        let (median_ms, sink) = measure(bounds, &sequence);
        total_sink = total_sink.wrapping_add(sink);
        println!(
            "u1 grid_segments {width}x{height}: {PAN_STEPS} pan-step rebuilds/op, median {median_ms:.6} ms/op ({:.6} ms/rebuild), sink lines/op={lines_per_op} sink={sink}",
            median_ms / PAN_STEPS as f64,
        );
    }
    black_box(total_sink);
}
