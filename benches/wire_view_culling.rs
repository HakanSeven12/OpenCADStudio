//! Camera culling of the resident wire instance buffer.
//!
//! A drawing is held resident as one instance slab per entity, and each camera
//! change turns the slabs that can reach the viewport into a short list of
//! draw ranges. This probe fixes a large synthetic layout and three cameras
//! (plan overview, plan zoom, tilted 3-D zoom) and reports, per camera, the
//! instances actually submitted, the number of draw ranges, and the CPU cost
//! of choosing them. It also times the per-entity bounds the arena stores for
//! the cull. The synthetic layout, slab order and cameras never change, so the
//! numbers stay comparable between runs.
//!
//! Run: `cargo bench --bench wire_view_culling`

use std::hint::black_box;
use std::time::Instant;

use glam::{DVec3, Mat4, Quat};
use OpenCADStudio::scene::pipeline::wire_arena::{
    visible_wire_ranges, wire_run_bounds, WireCullSlab,
};
use OpenCADStudio::scene::view::camera::{Camera, Projection};
use OpenCADStudio::scene::WireModel;

const ENTITIES: usize = 20_000;
const SEGMENTS: usize = 40;
const COLS: usize = 140;
const SPACING: f32 = 50.0;
const CLIP_W: u32 = 1280;
const CLIP_H: u32 = 720;
const RUNS: usize = 15;
const WARMUP: usize = 2;

fn entity_origin(i: usize) -> (f32, f32) {
    let col = (i % COLS) as f32;
    let row = (i / COLS) as f32;
    (
        col * SPACING - COLS as f32 * SPACING * 0.5,
        row * SPACING - 3500.0,
    )
}

fn entity_z(i: usize) -> f32 {
    ((i * 7) % 11) as f32 * 3.0
}

/// One slab per entity, laid out in submission order like the arena's tail.
fn slab_table() -> Vec<WireCullSlab> {
    (0..ENTITIES)
        .map(|i| {
            let (x, y) = entity_origin(i);
            let z = entity_z(i);
            WireCullSlab {
                inst_off: (i * SEGMENTS) as u32,
                inst_len: SEGMENTS as u32,
                aabb: [x, y, x + 30.0, y + 30.0],
                z: [z - 1.0, z + 1.0],
            }
        })
        .collect()
}

/// The wires those slabs were emitted from, for the bounds-build probe.
fn wire_models() -> Vec<WireModel> {
    (0..ENTITIES)
        .map(|i| {
            let (x, y) = entity_origin(i);
            let z = entity_z(i);
            let points = (0..=SEGMENTS)
                .map(|s| {
                    let t = s as f32 / SEGMENTS as f32;
                    [x + 30.0 * t, y + 15.0 * t, z + (t * 4.0).sin()]
                })
                .collect();
            WireModel {
                name: format!("line{i}"),
                geom: std::sync::Arc::new(OpenCADStudio::scene::WireGeom {
                    points,
                    ..OpenCADStudio::scene::WireGeom::default()
                }),
                aabb: [x, y, x + 30.0, y + 30.0],
                world_width: 0.5,
                ..WireModel::default()
            }
        })
        .collect()
}

fn camera(
    target: DVec3,
    half_height: f64,
    tilt: Option<f32>,
    projection: Projection,
) -> (Mat4, DVec3) {
    let mut cam = Camera::default();
    cam.target = target;
    cam.distance = (half_height / (cam.fov_y * 0.5).tan() as f64) as f32;
    if let Some(tilt) = tilt {
        cam.rotation = Quat::from_euler(glam::EulerRot::XYZ, tilt, 0.0, 0.5);
    }
    cam.projection = projection;
    let bounds = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: CLIP_W as f32,
        height: CLIP_H as f32,
    };
    (cam.view_proj_rte(bounds), cam.eye())
}

fn hash_ranges(ranges: &[(u32, u32)]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &(start, end) in ranges {
        h = (h ^ start as u64).wrapping_mul(0x1000_0000_01b3);
        h = (h ^ end as u64).wrapping_mul(0x1000_0000_01b3);
    }
    h
}

fn run_view(slabs: &[WireCullSlab], name: &str, view: Mat4, eye: DVec3) -> u64 {
    let mut sink = 0u64;
    for _ in 0..WARMUP {
        let ranges = visible_wire_ranges(
            slabs.iter().copied(),
            black_box(view),
            black_box(eye),
            CLIP_W,
            CLIP_H,
        );
        sink ^= hash_ranges(&ranges);
    }
    let mut samples = Vec::with_capacity(RUNS);
    let mut first = (0u64, 0usize, 0u64);
    for run in 0..RUNS {
        let start = Instant::now();
        let ranges = visible_wire_ranges(
            slabs.iter().copied(),
            black_box(view),
            black_box(eye),
            CLIP_W,
            CLIP_H,
        );
        samples.push(start.elapsed().as_nanos() as u64);
        let submitted: u64 = ranges.iter().map(|(s, e)| (e - s) as u64).sum();
        sink ^= hash_ranges(&ranges) ^ submitted;
        if run == 0 {
            first = (submitted, ranges.len(), samples[0]);
        }
    }
    samples.sort_unstable();
    println!(
        "{:<14} submitted={:>7} ranges={:>3} cull median={:>7.3} ms  min={:>7.3} ms  max={:>7.3} ms",
        name,
        first.0,
        first.1,
        samples[samples.len() / 2] as f64 / 1_000_000.0,
        samples[0] as f64 / 1_000_000.0,
        samples[samples.len() - 1] as f64 / 1_000_000.0,
    );
    sink
}

fn run_bounds_build(wires: &[&WireModel]) -> u64 {
    let mut sink = 0u64;
    for _ in 0..WARMUP {
        let (aabb, z) = wire_run_bounds(black_box(wires));
        sink ^= (aabb[0] as u64) ^ ((aabb[2] as u64) << 32) ^ (z[0] as u64) ^ ((z[1] as u64) << 32);
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        let (aabb, z) = wire_run_bounds(black_box(wires));
        samples.push(start.elapsed().as_nanos() as u64);
        sink ^= (aabb[0] as u64) ^ ((aabb[2] as u64) << 32) ^ (z[0] as u64);
    }
    samples.sort_unstable();
    println!(
        "{:<14} runs={} bounds median={:.3} ms  min={:.3} ms  max={:.3} ms",
        "bounds_build",
        wires.len(),
        samples[samples.len() / 2] as f64 / 1_000_000.0,
        samples[0] as f64 / 1_000_000.0,
        samples[samples.len() - 1] as f64 / 1_000_000.0,
    );
    sink
}

fn main() {
    let slabs = slab_table();
    let resident: u64 = slabs.iter().map(|s| s.inst_len as u64).sum();
    println!(
        "wire_view_culling bench (entities={ENTITIES}, segments={SEGMENTS}, instances={resident})"
    );

    let (plan_overview, overview_eye) = camera(DVec3::ZERO, 4000.0, None, Projection::Orthographic);
    let (plan_zoom, zoom_eye) = camera(
        DVec3::new(700.0, 1200.0, 0.0),
        500.0,
        None,
        Projection::Orthographic,
    );
    let (orbit_zoom, orbit_eye) = camera(
        DVec3::new(700.0, 1200.0, 0.0),
        900.0,
        Some(-0.9),
        Projection::Perspective,
    );

    let wires = wire_models();
    let refs: Vec<&WireModel> = wires.iter().collect();

    let mut sink = run_bounds_build(&refs);
    sink ^= run_view(&slabs, "plan_overview", plan_overview, overview_eye);
    sink ^= run_view(&slabs, "plan_zoom", plan_zoom, zoom_eye);
    sink ^= run_view(&slabs, "orbit_zoom", orbit_zoom, orbit_eye);
    println!("sink={sink:016x}");
}
