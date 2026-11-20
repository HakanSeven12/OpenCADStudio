//! Benchmark suite for 2D polygon boolean overlay, self-intersecting and
//! overlapping ring resolution, touching-hole hatch fill triangulation,
//! self-crossing curve area evaluation, and concave XCLIP hatch clipping.
//!
//! Run baseline:
//!   `cargo bench --bench polygon_overlay_and_clip -- --output target/polygon_overlay_baseline.json`
//! Run comparison against baseline:
//!   `cargo bench --bench polygon_overlay_and_clip -- --baseline target/polygon_overlay_baseline.json`

use std::f64::consts::TAU;
use std::hint::black_box;
use std::time::Instant;

use ocs_geom::geom2d::{
    signed_area, triangulate_rings, Curve, Polyline, PolylineVertex,
};
use OpenCADStudio::scene::pick::xclip::clip_hatch_boundary;

#[path = "support/harness.rs"]
mod harness;

use harness::BenchmarkRunner;

const WARMUP: usize = 2;
const RUNS: usize = 9;

fn mesh_area(points: &[[f64; 2]], triangles: &[[usize; 3]]) -> f64 {
    triangles
        .iter()
        .map(|tri| {
            let a = points[tri[0]];
            let b = points[tri[1]];
            let c = points[tri[2]];
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
        })
        .sum()
}

fn hatch_loops_area_f32(boundary: &[[f32; 2]]) -> f64 {
    let mut total = 0.0;
    let mut start = 0;
    while start < boundary.len() {
        if !boundary[start][0].is_finite() || !boundary[start][1].is_finite() {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < boundary.len() && boundary[end][0].is_finite() && boundary[end][1].is_finite() {
            end += 1;
        }
        let ring: Vec<[f64; 2]> = boundary[start..end]
            .iter()
            .map(|p| [p[0] as f64, p[1] as f64])
            .collect();
        total += signed_area(&ring).abs();
        start = end;
    }
    total
}

/// Fixture 1: Self-crossing multi-lobe rings at UTM survey coordinates.
/// Exercises `split_self_crossing` inside `triangulate_rings`.
fn build_self_crossing_rings() -> Vec<Vec<[f64; 2]>> {
    let origin = [551_000.0, 4_246_500.0];
    let mut rings = Vec::new();
    for k in 0..8 {
        let ox = origin[0] + (k % 4) as f64 * 250.0;
        let oy = origin[1] + (k / 4) as f64 * 250.0;
        // Lissajous multi-lobe self-crossing polygon (95 vertices, odd count avoids exact vertex-on-vertex degeneracy).
        let n = 95;
        let ring: Vec<[f64; 2]> = (0..n)
            .map(|i| {
                let t = i as f64 / n as f64 * TAU;
                [ox + 90.0 * t.cos(), oy + 80.0 * (3.0 * t).sin()]
            })
            .collect();
        rings.push(ring);
    }
    rings
}

/// Fixture 2: Overlapping simple rings (36 circles/octagons on a dense grid
/// whose edges cross their neighbors). Exercises `uncross` inside `triangulate_rings`.
fn build_overlapping_rings() -> Vec<Vec<[f64; 2]>> {
    let mut rings = Vec::new();
    let sides = 24;
    for row in 0..6 {
        for col in 0..6 {
            let cx = col as f64 * 18.0;
            let cy = row as f64 * 18.0;
            let r = 14.0;
            let ring: Vec<[f64; 2]> = (0..sides)
                .map(|i| {
                    let a = i as f64 / sides as f64 * TAU + 0.13;
                    [cx + r * a.cos(), cy + r * a.sin()]
                })
                .collect();
            rings.push(ring);
        }
    }
    rings
}

/// Fixture 3: Outer frame containing touching/overlapping holes (24 multi-sided
/// holes arranged so adjacent holes share edges/overlap slightly), triggering
/// the touching-hole fallback in `rings_from`.
fn build_touching_holes_rings() -> Vec<Vec<[f64; 2]>> {
    let mut rings = Vec::new();
    rings.push(vec![
        [-50.0, -50.0],
        [450.0, -50.0],
        [450.0, 350.0],
        [-50.0, 350.0],
    ]);
    let sides = 32;
    for row in 0..4 {
        for col in 0..6 {
            // Spacing 50 with radius 25 -> adjacent circular holes touch/share vertices.
            let cx = col as f64 * 50.0 + 25.0;
            let cy = row as f64 * 50.0 + 25.0;
            let r = 25.0;
            let hole: Vec<[f64; 2]> = (0..sides)
                .map(|i| {
                    let a = i as f64 / sides as f64 * TAU;
                    [cx + r * a.cos(), cy + r * a.sin()]
                })
                .collect();
            rings.push(hole);
        }
    }
    rings
}

/// Fixture 4: Self-crossing polyline for `Curve::chord_closed_area` (`covered_area`).
fn build_self_crossing_curve() -> Curve {
    // 105-vertex epitrochoid / multi-loop star curve that winds around multiple lobes
    // without retracing its own path.
    let n = 105;
    let vertices: Vec<PolylineVertex> = (0..n)
        .map(|i| {
            let t = i as f64 / n as f64 * TAU;
            PolylineVertex::straight([
                90.0 * t.cos() + 75.0 * (4.0 * t).cos(),
                90.0 * t.sin() - 75.0 * (4.0 * t).sin(),
            ])
        })
        .collect();
    Curve::Polyline(Polyline {
        vertices,
        closed: true,
    })
}

/// Fixture 5: Multi-loop hatch boundary (outer 128-gon + 16 circular holes)
/// clipped against a 64-vertex concave star XCLIP polygon at UTM coordinates.
fn build_xclip_hatch_fixture() -> (Vec<[f32; 2]>, [f64; 2], Vec<[f64; 2]>) {
    let world_origin = [292_884.0, 4_718_791.0];
    let mut boundary: Vec<[f32; 2]> = (0..128)
        .map(|i| {
            let a = i as f32 / 128.0 * std::f32::consts::TAU;
            [200.0 * a.cos(), 200.0 * a.sin()]
        })
        .collect();
    for row in 0..4 {
        for col in 0..4 {
            boundary.push([f32::NAN, f32::NAN]);
            let cx = -90.0 + col as f32 * 60.0;
            let cy = -90.0 + row as f32 * 60.0;
            for i in 0..24 {
                let a = i as f32 / 24.0 * std::f32::consts::TAU;
                boundary.push([cx + 15.0 * a.cos(), cy + 15.0 * a.sin()]);
            }
        }
    }
    let clip_verts = 64;
    let clip_poly: Vec<[f64; 2]> = (0..clip_verts)
        .map(|i| {
            let a = i as f64 / clip_verts as f64 * TAU;
            let r = if i % 2 == 0 { 180.0 } else { 95.0 };
            [world_origin[0] + r * a.cos(), world_origin[1] + r * a.sin()]
        })
        .collect();
    (boundary, world_origin, clip_poly)
}

fn main() {
    let mut runner = BenchmarkRunner::new(
        "OpenCADStudio 2D Polygon Overlay & Clipping Benchmark",
        "polygon_overlay_metrics.json",
    );

    // 1. Self-crossing rings fill
    if runner.should_run("self_crossing_rings_fill") {
        let rings = build_self_crossing_rings();
        let mut ref_area = None;
        for _ in 0..WARMUP {
            let (pts, tris) = triangulate_rings(black_box(&rings));
            ref_area = Some(mesh_area(&pts, &tris));
        }
        let expected = ref_area.unwrap();
        assert!(expected > 30_000.0, "unexpected self-crossing area: {expected}");

        let mut samples_us = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let (pts, tris) = triangulate_rings(black_box(&rings));
            let elapsed_us = start.elapsed().as_secs_f64() * 1_000_000.0;
            let area = mesh_area(&pts, &tris);
            assert!(
                (area - expected).abs() <= expected * 1e-4,
                "area drifted: {area} vs {expected}"
            );
            samples_us.push(elapsed_us);
        }
        let median_us = harness::median(&samples_us);
        runner.record(
            "self_crossing_rings_fill",
            "Triangulate 8 self-crossing 96-vertex Lissajous rings at UTM coordinates",
            "µs",
            samples_us,
            Some((1_000_000.0 / median_us, "ops/s")),
            None,
        );
    }

    // 2. Overlapping rings even-odd fill
    if runner.should_run("overlapping_rings_even_odd_fill") {
        let rings = build_overlapping_rings();
        let mut ref_area = None;
        for _ in 0..WARMUP {
            let (pts, tris) = triangulate_rings(black_box(&rings));
            ref_area = Some(mesh_area(&pts, &tris));
        }
        let expected = ref_area.unwrap();
        assert!(expected > 4_000.0, "unexpected overlapping rings area: {expected}");

        let mut samples_us = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let (pts, tris) = triangulate_rings(black_box(&rings));
            let elapsed_us = start.elapsed().as_secs_f64() * 1_000_000.0;
            let area = mesh_area(&pts, &tris);
            assert!(
                (area - expected).abs() <= expected * 1e-4,
                "area drifted: {area} vs {expected}"
            );
            samples_us.push(elapsed_us);
        }
        let median_us = harness::median(&samples_us);
        runner.record(
            "overlapping_rings_even_odd_fill",
            "Even-odd uncross and triangulate 36 overlapping 24-gon rings",
            "µs",
            samples_us,
            Some((1_000_000.0 / median_us, "ops/s")),
            None,
        );
    }

    // 3. Touching holes even-odd fill
    if runner.should_run("touching_holes_even_odd_fill") {
        let rings = build_touching_holes_rings();
        let mut ref_area = None;
        let mut tri_count = 0;
        for _ in 0..WARMUP {
            let (pts, tris) = triangulate_rings(black_box(&rings));
            ref_area = Some(mesh_area(&pts, &tris));
            tri_count = tris.len();
        }
        let expected = ref_area.unwrap();
        assert!(expected > 100_000.0, "unexpected touching-holes area: {expected}");
        println!("  touching_holes_even_odd_fill: {tri_count} triangles emitted, area = {expected:.2}");

        let mut samples_us = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let (pts, tris) = triangulate_rings(black_box(&rings));
            let elapsed_us = start.elapsed().as_secs_f64() * 1_000_000.0;
            let area = mesh_area(&pts, &tris);
            assert!(
                (area - expected).abs() <= expected * 1e-4,
                "area drifted: {area} vs {expected}"
            );
            samples_us.push(elapsed_us);
        }
        let median_us = harness::median(&samples_us);
        runner.record(
            "touching_holes_even_odd_fill",
            "Even-odd fill of outer rectangle with 24 touching 32-gon holes",
            "µs",
            samples_us,
            Some((1_000_000.0 / median_us, "ops/s")),
            None,
        );
    }

    // 4. Self-crossing chord-closed area
    if runner.should_run("self_crossing_chord_closed_area") {
        let curve = build_self_crossing_curve();
        let mut ref_area = None;
        for _ in 0..WARMUP {
            ref_area = curve.chord_closed_area();
        }
        let expected = ref_area.expect("area must resolve").abs();
        assert!(expected > 10_000.0, "unexpected self-crossing curve area: {expected}");

        let mut samples_us = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let area = black_box(&curve).chord_closed_area().unwrap().abs();
            let elapsed_us = start.elapsed().as_secs_f64() * 1_000_000.0;
            assert!(
                (area - expected).abs() <= expected * 1e-4,
                "area drifted: {area} vs {expected}"
            );
            samples_us.push(elapsed_us);
        }
        let median_us = harness::median(&samples_us);
        runner.record(
            "self_crossing_chord_closed_area",
            "Non-zero winding covered area of a 120-vertex self-crossing polyline",
            "µs",
            samples_us,
            Some((1_000_000.0 / median_us, "ops/s")),
            None,
        );
    }

    // 5. Concave XCLIP hatch boundary clipping
    if runner.should_run("xclip_concave_hatch_boundary") {
        let (boundary, world_origin, clip_poly) = build_xclip_hatch_fixture();
        let mut ref_area = None;
        for _ in 0..WARMUP {
            let clipped = clip_hatch_boundary(
                black_box(&boundary),
                black_box(world_origin),
                black_box(&clip_poly),
            );
            ref_area = Some(hatch_loops_area_f32(&clipped));
        }
        let expected = ref_area.unwrap();
        assert!(expected > 20_000.0, "unexpected clipped hatch area: {expected}");

        let mut samples_us = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            let clipped = clip_hatch_boundary(
                black_box(&boundary),
                black_box(world_origin),
                black_box(&clip_poly),
            );
            let elapsed_us = start.elapsed().as_secs_f64() * 1_000_000.0;
            let area = hatch_loops_area_f32(&clipped);
            assert!(
                (area - expected).abs() <= expected * 1e-3,
                "area drifted: {area} vs {expected}"
            );
            samples_us.push(elapsed_us);
        }
        let median_us = harness::median(&samples_us);
        runner.record(
            "xclip_concave_hatch_boundary",
            "Clip 17-loop hatch boundary against 64-vertex concave XCLIP polygon at UTM",
            "µs",
            samples_us,
            Some((1_000_000.0 / median_us, "ops/s")),
            None,
        );
    }

    runner.finish();
}
