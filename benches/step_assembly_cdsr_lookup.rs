//! OpenCADStudio STEP assembly CDSR lookup benchmark suite.
//!
//! The placement of every NEXT_ASSEMBLY_USAGE_OCCURRENCE was found by a
//! linear scan over every entity id looking for the
//! CONTEXT_DEPENDENT_SHAPE_REPRESENTATION that
//! claims the occurrence's PRODUCT_DEFINITION_SHAPE, and each product's
//! items are gated through a Vec::contains over `used_items`. On a file of
//! O occurrences and N entities that is O(O*N) per load.
//!
//! Measures the REAL production entry point: `step_import::read` on a
//! synthesized AP203 assembly of `OCCURRENCES` NEXT_ASSEMBLY_USAGE_OCCURRENCE
//! records (each with its own PRODUCT_DEFINITION_SHAPE, complex
//! REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION and
//! ITEM_DEFINED_TRANSFORMATION) over `CHILDREN` child products, each child
//! carrying one MANIFOLD_SOLID_BREP triangle so the used_items dedup and the
//! placement compose run for every occurrence. About 6100 entity ids total.
//!
//! The sink hashes the import result (products, roots, children with their
//! placement matrices, bodies) so both sides of the optimization must
//! produce the identical model.
//!
//! Deterministic: fixed geometry, fixed placements, no randomness.
//! Run: `cargo bench --bench step_assembly_cdsr_lookup`

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::io::step_import::read;

const OCCURRENCES: usize = 500;
const CHILDREN: usize = 100;
const RUNS: usize = 3;
const ITERATIONS: usize = 6;
const WARMUP: usize = 1;

/// A parent product assembling `OCCURRENCES` placements of `CHILDREN`
/// distinct solid-carrying child products.
fn build_step() -> String {
    let mut out = String::with_capacity(1 << 22);
    out.push_str(
        "ISO-10303-21;\n\
         HEADER;\n\
         FILE_DESCRIPTION(('io2 bench'),'2;1');\n\
         FILE_NAME('io2','','',(''),'',' ',' ');\n\
         FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\n\
         ENDSEC;\n\
         DATA;\n",
    );
    let mut next: u64 = 1;
    let emit = |out: &mut String, next: &mut u64, record: String| {
        let id = *next;
        *next += 1;
        out.push_str(&format!("#{id} = {record};\n"));
        id
    };

    // Shared axes: the untransformed origin and the unit axes.
    let origin = emit(&mut out, &mut next, "CARTESIAN_POINT('',(0.,0.,0.))".into());
    let z = emit(&mut out, &mut next, "DIRECTION('',(0.,0.,1.))".into());
    let x = emit(&mut out, &mut next, "DIRECTION('',(1.,0.,0.))".into());
    let from = emit(
        &mut out,
        &mut next,
        format!("AXIS2_PLACEMENT_3D('',#{origin},#{z},#{x})"),
    );

    // A one-triangle manifold solid in the z = 0 plane, offset by `dx`.
    let triangle = |out: &mut String, next: &mut u64, dx: f64| {
        let p0 = emit(out, next, format!("CARTESIAN_POINT('',({dx},0.,0.))"));
        let p1 = emit(
            out,
            next,
            format!("CARTESIAN_POINT('',({},0.,0.))", dx + 1.0),
        );
        let p2 = emit(out, next, format!("CARTESIAN_POINT('',({dx},1.,0.))"));
        let v0 = emit(out, next, format!("VERTEX_POINT('',#{p0})"));
        let v1 = emit(out, next, format!("VERTEX_POINT('',#{p1})"));
        let v2 = emit(out, next, format!("VERTEX_POINT('',#{p2})"));
        let d01 = emit(out, next, "DIRECTION('',(1.,0.,0.))".into());
        let d12 = emit(out, next, "DIRECTION('',(-1.,1.,0.))".into());
        let d20 = emit(out, next, "DIRECTION('',(0.,-1.,0.))".into());
        let v01 = emit(out, next, format!("VECTOR('',#{d01},1.)"));
        let v12 = emit(out, next, format!("VECTOR('',#{d12},1.)"));
        let v20 = emit(out, next, format!("VECTOR('',#{d20},1.)"));
        let l01 = emit(out, next, format!("LINE('',#{p0},#{v01})"));
        let l12 = emit(out, next, format!("LINE('',#{p1},#{v12})"));
        let l20 = emit(out, next, format!("LINE('',#{p2},#{v20})"));
        let e01 = emit(out, next, format!("EDGE_CURVE('',#{v0},#{v1},#{l01},.T.)"));
        let e12 = emit(out, next, format!("EDGE_CURVE('',#{v1},#{v2},#{l12},.T.)"));
        let e20 = emit(out, next, format!("EDGE_CURVE('',#{v2},#{v0},#{l20},.T.)"));
        let o01 = emit(out, next, format!("ORIENTED_EDGE('',*,*,#{e01},.T.)"));
        let o12 = emit(out, next, format!("ORIENTED_EDGE('',*,*,#{e12},.T.)"));
        let o20 = emit(out, next, format!("ORIENTED_EDGE('',*,*,#{e20},.T.)"));
        let loop_ = emit(out, next, format!("EDGE_LOOP('',(#{o01},#{o12},#{o20}))"));
        let axis = emit(
            out,
            next,
            format!("AXIS2_PLACEMENT_3D('',#{p0},#{z},#{d01})"),
        );
        let plane = emit(out, next, format!("PLANE('',#{axis})"));
        let bound = emit(out, next, format!("FACE_OUTER_BOUND('',#{loop_},.T.)"));
        let face = emit(
            out,
            next,
            format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)"),
        );
        let shell = emit(out, next, format!("CLOSED_SHELL('',(#{face}))"));
        emit(out, next, format!("MANIFOLD_SOLID_BREP('',#{shell})"))
    };

    // The parent product: definition, shape and a representation holding
    // its own solid.
    let parent_solid = triangle(&mut out, &mut next, 0.0);
    let parent_rep = emit(
        &mut out,
        &mut next,
        format!("SHAPE_REPRESENTATION('',(#{parent_solid}),$)"),
    );
    let parent_prod = emit(&mut out, &mut next, "PRODUCT('P','Parent','',())".into());
    let parent_form = emit(
        &mut out,
        &mut next,
        format!("PRODUCT_DEFINITION_FORMATION('','',#{parent_prod})"),
    );
    let parent_def = emit(
        &mut out,
        &mut next,
        format!("PRODUCT_DEFINITION('','',#{parent_form},())"),
    );
    let parent_shape = emit(
        &mut out,
        &mut next,
        format!("PRODUCT_DEFINITION_SHAPE('','',#{parent_def})"),
    );
    emit(
        &mut out,
        &mut next,
        format!("SHAPE_DEFINITION_REPRESENTATION(#{parent_shape},#{parent_rep})"),
    );

    // The child products, each with one solid-carrying representation.
    let mut child_defs = Vec::with_capacity(CHILDREN);
    let mut child_reps = Vec::with_capacity(CHILDREN);
    for c in 0..CHILDREN {
        let solid = triangle(&mut out, &mut next, 10.0 + c as f64);
        let rep = emit(
            &mut out,
            &mut next,
            format!("SHAPE_REPRESENTATION('',(#{solid}),$)"),
        );
        let prod = emit(
            &mut out,
            &mut next,
            format!("PRODUCT('C{c}','Child{c}','',())"),
        );
        let form = emit(
            &mut out,
            &mut next,
            format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"),
        );
        let def = emit(
            &mut out,
            &mut next,
            format!("PRODUCT_DEFINITION('','',#{form},())"),
        );
        let shape = emit(
            &mut out,
            &mut next,
            format!("PRODUCT_DEFINITION_SHAPE('','',#{def})"),
        );
        emit(
            &mut out,
            &mut next,
            format!("SHAPE_DEFINITION_REPRESENTATION(#{shape},#{rep})"),
        );
        child_defs.push(def);
        child_reps.push(rep);
    }

    // The occurrences: each its own shape, transformation and complex
    // representation relationship, claiming one CDSR by its shape.
    for o in 0..OCCURRENCES {
        let child = o % CHILDREN;
        let nauo = emit(
            &mut out,
            &mut next,
            format!(
                "NEXT_ASSEMBLY_USAGE_OCCURRENCE('N{o}','','',#{parent_def},#{})",
                child_defs[child]
            ),
        );
        let shape = emit(
            &mut out,
            &mut next,
            format!("PRODUCT_DEFINITION_SHAPE('','',#{nauo})"),
        );
        let org = emit(
            &mut out,
            &mut next,
            format!("CARTESIAN_POINT('',({}.,0.,0.))", o),
        );
        let to = emit(
            &mut out,
            &mut next,
            format!("AXIS2_PLACEMENT_3D('',#{org},#{z},#{x})"),
        );
        let xform = emit(
            &mut out,
            &mut next,
            format!("ITEM_DEFINED_TRANSFORMATION('','',#{from},#{to})"),
        );
        let relation = emit(
            &mut out,
            &mut next,
            format!(
                "(REPRESENTATION_RELATIONSHIP('','',#{parent_rep},#{})\
                 REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{xform})\
                 SHAPE_REPRESENTATION_RELATIONSHIP())",
                child_reps[child]
            ),
        );
        emit(
            &mut out,
            &mut next,
            format!("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{relation},#{shape})"),
        );
    }

    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}

/// A hash of everything the assembly import produces: product and root
/// counts, each child placement and every body, so both sides of the
/// optimization must agree bit for bit.
fn sink(model: &OpenCADStudio::io::step_import::StepModel) -> u64 {
    let mut hash: u64 = 0x9e37_79b9_7f4a_7c15;
    let mix = |hash: &mut u64, value: u64| {
        *hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(value);
    };
    mix(&mut hash, model.products.len() as u64);
    mix(&mut hash, model.roots.len() as u64);
    mix(&mut hash, model.unit_mm.unwrap_or(f64::NAN).to_bits());
    for product in &model.products {
        mix(&mut hash, product.children.len() as u64);
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
        }
    }
    mix(&mut hash, model.direct.len() as u64);
    hash
}

fn main() {
    let bytes = build_step().into_bytes();
    let first = read(&bytes).expect("the synthesized STEP assembly must import");
    println!(
        "step_assembly_cdsr_lookup: {} bytes, {} products, {} roots, {} children, {} bodies",
        bytes.len(),
        first.products.len(),
        first.roots.len(),
        first
            .products
            .iter()
            .map(|p| p.children.len())
            .sum::<usize>(),
        first.products.iter().map(|p| p.bodies.len()).sum::<usize>(),
    );
    let expected = sink(&first);

    for _ in 0..WARMUP {
        let model = read(black_box(&bytes)).expect("the synthesized STEP assembly must import");
        black_box(sink(black_box(&model)));
    }
    let mut best = f64::MAX;
    for _ in 0..RUNS {
        let started = Instant::now();
        let mut value = 0u64;
        for _ in 0..ITERATIONS {
            let model = read(black_box(&bytes)).expect("the synthesized STEP assembly must import");
            value = sink(black_box(&model));
        }
        let per_read = started.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64;
        best = best.min(per_read);
        assert_eq!(value, expected, "every read must produce the same model");
        black_box(value);
    }
    println!(
        "step_assembly_cdsr_lookup: {ITERATIONS} reads x {RUNS} runs; best {best:.3} ms/read; sink {expected:#x}"
    );
}
