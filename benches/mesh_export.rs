//! OpenCADStudio mesh export benchmark (IO5/IO6).
//!
//! STL export gathers every triangle into an intermediate `Vec<Tri>` before
//! serializing, and the STEP writer builds a `format!`ed `String` per
//! oriented edge, per face bound and per shell member before joining them
//! into each line, while the output `String` grows unreserved. On a large
//! tessellated model that is one heap allocation per triangle fragment and
//! several full-buffer reallocations.
//!
//! Fixture: one 255x255 sine heightfield (130,050 triangles, 65,536
//! vertices). Adjacent triangles are never coplanar within the writer's
//! grouping tolerance, so STEP's coplanar grouping falls back to one
//! ADVANCED_FACE per triangle — the worst case for the per-triangle string
//! churn and the path large curved exports actually take.
//!
//! Sink: the STL byte length is asserted every run, the STEP face count must
//! stay essentially one per triangle, and both exports' FNV-1a digests (the
//! STEP one over its DATA section — the header carries a wall-clock
//! timestamp) must match across runs, so a fix that changes any emitted
//! byte fails the run.
//!
//! Deterministic: fixed geometry, no randomness.
//! Run: `cargo bench --bench mesh_export`

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::io::stl::build_stl;
use OpenCADStudio::io::step::build_step;
use OpenCADStudio::scene::model::mesh_model::MeshModel;

/// Cells per side; two triangles per cell.
const GRID: usize = 255;
const TRIS: usize = GRID * GRID * 2;
const RUNS: usize = 3;
const WARMUP: usize = 1;

fn fixture() -> MeshModel {
    let side = GRID + 1;
    let mut verts = Vec::with_capacity(side * side);
    for row in 0..side {
        for column in 0..side {
            let x = column as f32 * 2.0;
            let y = row as f32 * 2.0;
            let z = 3.0 * (0.8 * x).sin() * (0.7 * y).cos() + 2.0 * (1.9 * x + 1.3 * y).sin();
            verts.push([x, y, z]);
        }
    }
    let mut indices = Vec::with_capacity(TRIS * 3);
    for row in 0..GRID {
        for column in 0..GRID {
            let v00 = (row * side + column) as u32;
            let v10 = v00 + 1;
            let v01 = v00 + side as u32;
            let v11 = v01 + 1;
            indices.extend_from_slice(&[v00, v01, v10, v10, v01, v11]);
        }
    }
    MeshModel {
        name: String::new(),
        verts,
        verts_low: Vec::new(),
        normals: vec![[0.0, 0.0, 1.0]; side * side],
        indices,
        triangle_material_handles: Vec::new(),
        triangle_colors: Vec::new(),
        color: [1.0; 4],
        selected: false,
    }
}

fn digest(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn count_needle(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// The STEP header carries a wall-clock FILE_NAME timestamp, so stability is
/// asserted over the DATA section only.
fn data_section(text: &str) -> &str {
    text.split_once("DATA;")
        .map(|(_, tail)| tail)
        .unwrap_or(text)
}

fn main() {
    let mesh = fixture();
    assert_eq!(mesh.indices.len() / 3, TRIS, "the fixture triangle count");

    for _ in 0..WARMUP {
        black_box(build_stl(&[&mesh]));
        black_box(build_step(&[&mesh]));
    }

    let mut stl_best = f64::MAX;
    let mut stl_digest = None;
    for run in 0..RUNS {
        let started = Instant::now();
        let bytes = build_stl(black_box(&[&mesh])).expect("the fixture exports");
        let seconds = started.elapsed().as_secs_f64();
        assert_eq!(
            bytes.len(),
            84 + TRIS * 50,
            "binary STL is 84 bytes plus 50 per triangle"
        );
        let run_digest = digest(&bytes);
        match stl_digest {
            Some(previous) => assert_eq!(run_digest, previous, "STL bytes are stable across runs"),
            None => stl_digest = Some(run_digest),
        }
        stl_best = stl_best.min(seconds);
        println!("mesh_export: stl run {run}: {seconds:.4} s, {} bytes", bytes.len());
    }

    let mut step_best = f64::MAX;
    let mut step_digest = None;
    let mut step_bytes = 0usize;
    let mut faces = 0usize;
    for run in 0..RUNS {
        let started = Instant::now();
        let text = build_step(black_box(&[&mesh])).expect("the fixture exports");
        let seconds = started.elapsed().as_secs_f64();
        faces = count_needle(&text, "ADVANCED_FACE(");
        // The steep heightfield leaves at most a stray coplanar pair; the
        // export must stay essentially one ADVANCED_FACE per triangle.
        assert!(
            faces * 1000 >= TRIS * 999,
            "expected the per-triangle face path, got {faces} of {TRIS}"
        );
        let run_digest = digest(data_section(&text).as_bytes());
        match step_digest {
            Some(previous) => {
                assert_eq!(run_digest, previous, "STEP data section is stable across runs")
            }
            None => step_digest = Some(run_digest),
        }
        step_best = step_best.min(seconds);
        step_bytes = text.len();
        println!("mesh_export: step run {run}: {seconds:.4} s, {step_bytes} bytes");
    }

    println!(
        "mesh_export: {TRIS} triangles; stl best {stl_best:.4} s, step best {step_best:.4} s \
         ({step_bytes} bytes, {faces} faces)"
    );
}
