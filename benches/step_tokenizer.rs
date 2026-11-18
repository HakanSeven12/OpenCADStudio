//! OpenCADStudio STEP tokenizer benchmark suite.
//!
//! Every numeric token used to run two `str::replace` calls
//! (`".E" -> ".0E"`, `".e" -> ".0e"`), each allocating a fresh String even
//! when the token contains no exponent, and every word token allocates an
//! uppercased copy via `to_ascii_uppercase`. Those two allocations are the
//! constant factor applied to every token of every STEP file the reader
//! parses.
//!
//! Measures the REAL production entry point: `step_import::read` on a
//! synthesized AP203 file holding one product whose solid is a tessellated
//! shell of `TRIANGLES` disjoint triangles — 3 CARTESIAN_POINTs,
//! 3 DIRECTIONs and a VECTOR magnitude per triangle, so the file is
//! dominated by numeric tokens (`8000.` trailing-dot forms, `-1.`,
//! `1.E-1` exponent forms, plain `0.5`) alongside the entity keywords and
//! `.T.` enum words. About 110,000 entity records total.
//!
//! The sink hashes the import result down to entity/point counts and the
//! parsed geometry itself: product and body counts, every body's vertex
//! count and vertex coordinates (the CARTESIAN_POINT numbers), face counts
//! (which fail to build if a keyword no longer matches case-insensitively)
//! and colours. Both sides of the optimization must produce the identical
//! model from the identical bytes.
//!
//! Deterministic: fixed geometry, fixed grid coordinates, no randomness.
//! Run: `cargo bench --bench step_tokenizer`

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::io::step_import::read;

#[path = "support/step_fixture.rs"]
mod step_fixture;

use step_fixture::build_step;

const RUNS: usize = 3;
const ITERATIONS: usize = 4;
const WARMUP: usize = 1;

/// A hash of everything the import produces: the entity and point counts
/// (products, roots, bodies, vertices, faces) and the parsed point
/// coordinates themselves, so both sides of the optimization must agree
/// bit for bit.
fn sink(model: &OpenCADStudio::io::step_import::StepModel) -> u64 {
    let mut hash: u64 = 0x9e37_79b9_7f4a_7c15;
    let mix = |hash: &mut u64, value: u64| {
        *hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(value);
    };
    mix(&mut hash, model.products.len() as u64);
    mix(&mut hash, model.roots.len() as u64);
    mix(&mut hash, model.direct.len() as u64);
    mix(&mut hash, model.unit_mm.unwrap_or(f64::NAN).to_bits());
    for product in &model.products {
        mix(&mut hash, product.name.len() as u64);
        for byte in product.name.bytes() {
            mix(&mut hash, byte as u64);
        }
        mix(&mut hash, product.bodies.len() as u64);
        mix(&mut hash, product.scale.to_bits());
        for (_, placement) in &product.children {
            for row in placement {
                for value in row {
                    mix(&mut hash, value.to_bits());
                }
            }
        }
        for body in &product.bodies {
            mix(&mut hash, body.faces.len() as u64);
            for face in &body.faces {
                match face {
                    Some(rgb) => {
                        mix(&mut hash, 1);
                        for value in rgb {
                            mix(&mut hash, value.to_bits());
                        }
                    }
                    None => mix(&mut hash, 0),
                }
            }
            mix(&mut hash, body.body.vertices.len() as u64);
            for (_, vertex) in body.body.vertices.iter() {
                for value in vertex.point {
                    mix(&mut hash, value.to_bits());
                }
            }
            mix(&mut hash, body.body.faces.len() as u64);
            mix(&mut hash, body.body.edges.len() as u64);
        }
    }
    hash
}

fn main() {
    let bytes = build_step().into_bytes();
    let first = read(&bytes).expect("the synthesized STEP tessellation must import");
    let vertices: usize = first
        .products
        .iter()
        .flat_map(|p| &p.bodies)
        .map(|b| b.body.vertices.len())
        .sum();
    let faces: usize = first
        .products
        .iter()
        .flat_map(|p| &p.bodies)
        .map(|b| b.body.faces.len())
        .sum();
    println!(
        "step_tokenizer: {} bytes, {} products, {} bodies, {faces} faces, {vertices} points",
        bytes.len(),
        first.products.len(),
        first.products.iter().map(|p| p.bodies.len()).sum::<usize>(),
    );
    let expected = sink(&first);

    for _ in 0..WARMUP {
        let model = read(black_box(&bytes)).expect("the synthesized STEP tessellation must import");
        black_box(sink(black_box(&model)));
    }
    let mut best = f64::MAX;
    for _ in 0..RUNS {
        let started = Instant::now();
        let mut value = 0u64;
        for _ in 0..ITERATIONS {
            let model =
                read(black_box(&bytes)).expect("the synthesized STEP tessellation must import");
            value = sink(black_box(&model));
        }
        let per_read = started.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64;
        best = best.min(per_read);
        assert_eq!(value, expected, "every read must produce the same model");
        black_box(value);
    }
    println!(
        "step_tokenizer: {ITERATIONS} reads x {RUNS} runs; best {best:.3} ms/read; sink {expected:#x}"
    );
}
