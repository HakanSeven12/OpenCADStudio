// G13: CPU hatch rasterisation — `HatchModel::pattern_segments` on dense
// boundaries. Each pattern line is clipped against the whole boundary edge
// list, so the cost is lines x edges; the fixtures use boundaries at the
// 1024-vertex cap (`MAX_HATCH_BOUNDARY_VERTS`) with fine-spaced families to
// hold that product at its worst.

use std::sync::Arc;
use std::time::Instant;

use OpenCADStudio::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};

const RUNS: usize = 5;
const WARMUP: usize = 1;

fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn digest(segments: &[[[f64; 2]; 2]]) -> (u64, usize) {
    let mut bytes = Vec::with_capacity(segments.len() * 32);
    for segment in segments {
        for point in segment {
            for component in point {
                bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
    }
    (fnv1a(&bytes), segments.len())
}

fn model(boundary: Vec<[f32; 2]>, families: Vec<PatFamily>) -> HatchModel {
    HatchModel {
        pattern_origin: None,
        render_instance: None,
        // UTM-sized anchor: the rasteriser resolves absolute WCS in f64, so
        // the fixture keeps the magnitudes it would see in the field.
        world_origin: [512_345.678, 4_512_345.678],
        boundary: Arc::new(boundary),
        boundary_wcs: None,
        fill_plane: None,
        fill_plane_boundary: None,
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: codec::entities::HatchStyleType::Normal,
        pattern: HatchPattern::Pattern(families),
        name: String::new(),
        color: [0.75, 0.75, 0.75, 0.85],
        aci: 0,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    }
}

/// A closed `n`-gon ring approximating a circle.
fn circle_ring(n: usize, radius: f32) -> Vec<[f32; 2]> {
    (0..n)
        .map(|i| {
            let t = (i as f32) / (n as f32) * std::f32::consts::TAU;
            [radius * t.cos(), radius * t.sin()]
        })
        .collect()
}

fn main() {
    println!("hatch_pattern_segments: RUNS={RUNS} WARMUP={WARMUP}");

    // ── Fixture 1: a 1024-gon disc, two families — one dashed at 2-unit
    //    spacing, one solid at 3-unit spacing. ~830 pattern lines against
    //    1024 edges.
    let mut families = vec![PatFamily {
        angle_deg: 45.0,
        x0: 0.0,
        y0: 0.0,
        dx: -std::f32::consts::SQRT_2,
        dy: std::f32::consts::SQRT_2,
        dashes: vec![5.0, -5.0],
    }];
    families.push(PatFamily {
        angle_deg: 135.0,
        x0: 0.0,
        y0: 0.0,
        dx: -3.0 * std::f32::consts::FRAC_1_SQRT_2,
        dy: -3.0 * std::f32::consts::FRAC_1_SQRT_2,
        dashes: Vec::new(),
    });
    let disc = model(circle_ring(1024, 500.0), families);

    // ── Fixture 2: a 24-lobe gear (720 edges) with 8 circular islands
    //    (288 edges) knocked out — pattern lines cross, miss and re-enter,
    //    so many lines produce several spans and many produce none.
    let mut gear: Vec<[f32; 2]> = (0..720)
        .map(|i| {
            let t = (i as f32) / 720.0 * std::f32::consts::TAU;
            let r = 400.0 + 50.0 * (24.0 * t).cos();
            [r * t.cos(), r * t.sin()]
        })
        .collect();
    for island in 0..8 {
        let centre = [
            250.0 * (island as f32 / 8.0 * std::f32::consts::TAU).cos(),
            250.0 * (island as f32 / 8.0 * std::f32::consts::TAU).sin(),
        ];
        gear.push([f32::NAN, f32::NAN]);
        gear.extend(circle_ring(36, 40.0).into_iter().map(move |mut p| {
            p[0] += centre[0];
            p[1] += centre[1];
            p
        }));
    }
    let gear_hatch = model(
        gear,
        vec![
            PatFamily {
                angle_deg: 45.0,
                x0: 0.0,
                y0: 0.0,
                dx: -3.0 / std::f32::consts::SQRT_2,
                dy: 3.0 / std::f32::consts::SQRT_2,
                dashes: vec![8.0, -8.0],
            },
            PatFamily {
                angle_deg: 90.0,
                x0: 0.0,
                y0: 0.0,
                dx: -4.0,
                dy: 0.0,
                dashes: Vec::new(),
            },
        ],
    );

    for (name, hatch) in [("disc", &disc), ("gear", &gear_hatch)] {
        let mut times = Vec::with_capacity(RUNS);
        let mut reference: Option<(u64, usize)> = None;
        for run in 0..WARMUP + RUNS {
            let start = Instant::now();
            let segments = hatch.pattern_segments();
            let elapsed = start.elapsed();
            let found = digest(&segments);
            if run >= WARMUP {
                times.push(elapsed.as_secs_f64());
                match reference {
                    Some(previous) => assert_eq!(
                        found, previous,
                        "{name}: segment digest moved between runs"
                    ),
                    None => reference = Some(found),
                }
            }
        }
        times.sort_by(f64::total_cmp);
        let (hash, count) = reference.expect("at least one run");
        assert!(count > 0, "{name}: no segments emitted");
        println!(
            "{name}: {count} segments, digest {hash:016x}, best {:.4} s, all {:?}",
            times[0],
            times.iter().map(|t| format!("{t:.4}")).collect::<Vec<_>>()
        );
        std::hint::black_box(count as u64 + hash);
    }
}
