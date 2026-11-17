//! Stage-level baseline for the wire rebuild path.
//!
//! Part 1 times the double-single split (`points_to_ds`) in isolation at
//! several batch sizes, including the per-wire call pattern of 2-point lines,
//! so per-call overhead can be told apart from per-point cost.
//! Part 2 times the slab Z-bounds walk over a rebuilt resident set.
//! Part 3 rebuilds full resident wire sets for four entity compositions and
//! reports the realized point counts, so equal-budget comparisons are honest
//! about what each composition actually tessellates to.
//!
//! Coordinates are UTM-scale: the low residual is load-bearing there.
//!
//! Run: `cargo bench --bench wires_rebuild_stages`

use std::hint::black_box;
use std::time::Instant;

use document::entities::{Line, LwPolyline, LwVertex, Spline, Text};
use document::types::{Vector2, Vector3};
use document::EntityType;
use OpenCADStudio::scene::convert::tessellate::points_to_ds;
use OpenCADStudio::scene::pipeline::wire_arena::wire_run_bounds;
use OpenCADStudio::scene::Scene;

const RUNS: usize = 5;
const WARMUP: usize = 1;
const ORIGIN_X: f64 = 500_000.0;
const ORIGIN_Y: f64 = 4_000_000.0;

fn utm_points(n: usize, nan_every: usize) -> Vec<[f64; 3]> {
    (0..n)
        .map(|i| {
            if nan_every > 0 && i % nan_every == nan_every - 1 {
                [f64::NAN, 0.0, 0.0]
            } else {
                let t = i as f64;
                [
                    ORIGIN_X + t * 0.013,
                    ORIGIN_Y + t * 0.007,
                    (t * 0.001).sin() * 2.5,
                ]
            }
        })
        .collect()
}

fn median_ms(samples: &mut [u64]) -> f64 {
    samples.sort_unstable();
    samples[samples.len() / 2] as f64 / 1_000_000.0
}

fn time_split(points: &[[f64; 3]]) -> f64 {
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..WARMUP {
        let (hi, lo) = points_to_ds(black_box(points).iter().copied());
        black_box((hi.len(), lo.len()));
    }
    for _ in 0..RUNS {
        let start = Instant::now();
        let (hi, lo) = points_to_ds(black_box(points).iter().copied());
        samples.push(start.elapsed().as_nanos() as u64);
        black_box((hi, lo));
    }
    median_ms(&mut samples)
}

fn split_stage() {
    println!("== split stage (points_to_ds, UTM-scale f64 -> ds f32 pairs) ==");
    for &n in &[2usize, 16, 128, 1024, 65536] {
        let clean = utm_points(n, 0);
        let with_nans = utm_points(n, 17);
        println!(
            "  split_bulk_{n:<6} clean={:.4} ms  nan_sep={:.4} ms",
            time_split(&clean),
            time_split(&with_nans),
        );
    }

    // The line-entity call pattern: one 2-point call per wire, allocations
    // included. Same total point count as split_bulk_65536.
    let pairs: Vec<[f64; 3]> = utm_points(65536, 0);
    let mut samples = Vec::with_capacity(WARMUP + RUNS);
    for _ in 0..WARMUP + RUNS {
        let start = Instant::now();
        let mut sink = 0usize;
        for pair in pairs.chunks_exact(2) {
            let (hi, lo) = points_to_ds(black_box(pair).iter().copied());
            sink = sink.wrapping_add(hi.len() + lo.len());
        }
        samples.push(start.elapsed().as_nanos() as u64);
        black_box(sink);
    }
    let timed = &mut samples[WARMUP..];
    println!(
        "  split_per_wire_2pt x32768 calls  median={:.4} ms",
        median_ms(timed),
    );
}

fn build_lines(entities: usize) -> Scene {
    let mut scene = Scene::new();
    for i in 0..entities {
        let x = (i % 500) as f64 * 10.0;
        let y = (i / 500) as f64 * 10.0;
        scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(ORIGIN_X + x, ORIGIN_Y + y, 0.0),
            Vector3::new(ORIGIN_X + x + 8.0, ORIGIN_Y + y + 2.0, 0.0),
        )));
    }
    scene
}

fn build_polylines(entities: usize, verts: usize) -> Scene {
    let mut scene = Scene::new();
    for i in 0..entities {
        let x = (i % 200) as f64 * 60.0;
        let y = (i / 200) as f64 * 60.0;
        let mut pl = LwPolyline::new();
        pl.vertices = (0..verts)
            .map(|v| LwVertex {
                location: Vector2::new(ORIGIN_X + x + v as f64 * 5.0, ORIGIN_Y + y),
                bulge: 0.0,
                start_width: 0.0,
                end_width: 0.0,
                vertex_id: 0,
            })
            .collect();
        scene.add_entity(EntityType::LwPolyline(pl));
    }
    scene
}

fn build_splines(entities: usize, ctrl: usize) -> Scene {
    let mut scene = Scene::new();
    for i in 0..entities {
        let x = (i % 200) as f64 * 60.0;
        let y = (i / 200) as f64 * 60.0;
        let points = (0..ctrl)
            .map(|c| {
                Vector3::new(
                    ORIGIN_X + x + c as f64 * 5.0,
                    ORIGIN_Y + y + ((c % 3) as f64 - 1.0) * 4.0,
                    0.0,
                )
            })
            .collect();
        scene.add_entity(EntityType::Spline(Spline::from_control_points(3, points)));
    }
    scene
}

fn build_texts(entities: usize) -> Scene {
    let mut scene = Scene::new();
    for i in 0..entities {
        let x = (i % 200) as f64 * 60.0;
        let y = (i / 200) as f64 * 60.0;
        scene.add_entity(EntityType::Text(Text::with_value(
            format!("NOTE {i}"),
            Vector3::new(ORIGIN_X + x, ORIGIN_Y + y, 0.0),
        )));
    }
    scene
}

fn wire_point_count(wires: &[OpenCADStudio::scene::model::wire_model::WireModel]) -> usize {
    wires.iter().map(|w| w.points.len()).sum()
}

fn rebuild_scene(label: &str, mut scene: Scene) {
    let cam = scene.camera.borrow().clone();
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
    let points = wire_point_count(&wires);
    let wire_count = wires.len();
    drop(wires);

    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..WARMUP {
        scene.bump_geometry();
        let w = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
        black_box(w.len());
    }
    for _ in 0..RUNS {
        scene.bump_geometry();
        let start = Instant::now();
        let w = scene.model_tile_wires_arc(0, black_box(&cam), 1.0, 1.0);
        samples.push(start.elapsed().as_nanos() as u64);
        black_box(w.len());
    }
    println!(
        "  {label:<24} wires={wire_count:<8} points={points:<9} median={:.3} ms",
        median_ms(&mut samples),
    );

    if label == "lines_200k" {
        let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
        let refs: Vec<_> = wires.iter().collect();
        let mut samples = Vec::with_capacity(RUNS);
        for _ in 0..WARMUP + RUNS {
            let start = Instant::now();
            let bounds = wire_run_bounds(black_box(&refs));
            samples.push(start.elapsed().as_nanos() as u64);
            black_box(bounds);
        }
        let timed = &mut samples[WARMUP..];
        println!(
            "  z_bounds_walk (same set)   median={:.4} ms",
            median_ms(timed),
        );
    }
}

fn main() {
    println!("wires_rebuild_stages bench (UTM-scale coordinates, median of {RUNS})");
    split_stage();
    println!("== full resident rebuild by composition ==");
    rebuild_scene("lines_200k", build_lines(200_000));
    rebuild_scene("polylines_40k_x10", build_polylines(40_000, 10));
    rebuild_scene("splines_40k_x10ctrl", build_splines(40_000, 10));
    rebuild_scene("texts_40k", build_texts(40_000));
}