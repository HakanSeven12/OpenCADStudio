//! A synthesized STEP model shared by the reader benchmarks.
//!
//! One product whose solid is a tessellated shell of [`TRIANGLES`] disjoint
//! triangles — 3 CARTESIAN_POINTs, 3 DIRECTIONs and a VECTOR magnitude per
//! triangle, so the file is dominated by numeric tokens (`8000.`
//! trailing-dot forms, `-1.`, `1.E-1` exponent forms, plain `0.5`) alongside
//! the entity keywords and `.T.` enum words. About 110,000 entity records
//! total. Deterministic: fixed geometry, fixed grid coordinates.

#![allow(dead_code)]

pub const TRIANGLES: usize = 4000;

/// The synthesized AP203 file, as text.
pub fn build_step() -> String {
    let mut out = String::with_capacity(1 << 23);
    out.push_str(
        "ISO-10303-21;\n\
         HEADER;\n\
         FILE_DESCRIPTION(('io3 bench'),'2;1');\n\
         FILE_NAME('io3','','',(''),'',' ',' ');\n\
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

    // The shared unit axis every triangle's placement points along.
    let z = emit(&mut out, &mut next, "DIRECTION('',(0.,0.,1.))".into());

    // The tessellated shell: disjoint triangles laid out along x, each
    // with its own points, directions, edges and face. The second point
    // of every triangle carries a `1.E-1` exponent coordinate and the
    // shared directions carry `-1.`/`0.`/`1.` trailing-dot forms, so the
    // numeric normalization paths all run on every triangle.
    let mut face_ids = Vec::with_capacity(TRIANGLES);
    for t in 0..TRIANGLES {
        let base = t * 2;
        let p0 = emit(
            &mut out,
            &mut next,
            format!("CARTESIAN_POINT('',({base}.,0.,0.))"),
        );
        let p1 = emit(
            &mut out,
            &mut next,
            format!("CARTESIAN_POINT('',({},1.E-1,0.))", base + 1),
        );
        let p2 = emit(
            &mut out,
            &mut next,
            format!("CARTESIAN_POINT('',({base}.,1.,0.))"),
        );
        let v0 = emit(&mut out, &mut next, format!("VERTEX_POINT('',#{p0})"));
        let v1 = emit(&mut out, &mut next, format!("VERTEX_POINT('',#{p1})"));
        let v2 = emit(&mut out, &mut next, format!("VERTEX_POINT('',#{p2})"));
        let d01 = emit(&mut out, &mut next, "DIRECTION('',(1.,0.,0.))".into());
        let d12 = emit(&mut out, &mut next, "DIRECTION('',(-1.,1.,0.5))".into());
        let d20 = emit(&mut out, &mut next, "DIRECTION('',(0.,-1.,0.))".into());
        let v01 = emit(&mut out, &mut next, format!("VECTOR('',#{d01},1.)"));
        let v12 = emit(&mut out, &mut next, format!("VECTOR('',#{d12},1.)"));
        let v20 = emit(&mut out, &mut next, format!("VECTOR('',#{d20},1.)"));
        let l01 = emit(&mut out, &mut next, format!("LINE('',#{p0},#{v01})"));
        let l12 = emit(&mut out, &mut next, format!("LINE('',#{p1},#{v12})"));
        let l20 = emit(&mut out, &mut next, format!("LINE('',#{p2},#{v20})"));
        let e01 = emit(
            &mut out,
            &mut next,
            format!("EDGE_CURVE('',#{v0},#{v1},#{l01},.T.)"),
        );
        let e12 = emit(
            &mut out,
            &mut next,
            format!("EDGE_CURVE('',#{v1},#{v2},#{l12},.T.)"),
        );
        let e20 = emit(
            &mut out,
            &mut next,
            format!("EDGE_CURVE('',#{v2},#{v0},#{l20},.T.)"),
        );
        let o01 = emit(
            &mut out,
            &mut next,
            format!("ORIENTED_EDGE('',*,*,#{e01},.T.)"),
        );
        let o12 = emit(
            &mut out,
            &mut next,
            format!("ORIENTED_EDGE('',*,*,#{e12},.T.)"),
        );
        let o20 = emit(
            &mut out,
            &mut next,
            format!("ORIENTED_EDGE('',*,*,#{e20},.T.)"),
        );
        // One loop keyword written in lower case on purpose: the reader
        // must still match it against `EDGE_LOOP`, or this triangle loses
        // its loop (and its three vertices) from the built body.
        let loop_name = if t == 7 { "edge_loop" } else { "EDGE_LOOP" };
        let loop_ = emit(
            &mut out,
            &mut next,
            format!("{loop_name}('',(#{o01},#{o12},#{o20}))"),
        );
        let axis = emit(
            &mut out,
            &mut next,
            format!("AXIS2_PLACEMENT_3D('',#{p0},#{z},#{d01})"),
        );
        let plane = emit(&mut out, &mut next, format!("PLANE('',#{axis})"));
        let bound = emit(
            &mut out,
            &mut next,
            format!("FACE_OUTER_BOUND('',#{loop_},.T.)"),
        );
        face_ids.push(emit(
            &mut out,
            &mut next,
            format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)"),
        ));
    }
    let mut shell_faces = String::with_capacity(face_ids.len() * 8);
    for (k, face) in face_ids.iter().enumerate() {
        if k > 0 {
            shell_faces.push(',');
        }
        shell_faces.push_str(&format!("#{face}"));
    }
    let shell = emit(
        &mut out,
        &mut next,
        format!("CLOSED_SHELL('',({shell_faces}))"),
    );
    let solid = emit(
        &mut out,
        &mut next,
        format!("MANIFOLD_SOLID_BREP('',#{shell})"),
    );

    // The product: definition, shape and a representation holding the solid.
    let rep = emit(
        &mut out,
        &mut next,
        format!("SHAPE_REPRESENTATION('',(#{solid}),$)"),
    );
    let prod = emit(
        &mut out,
        &mut next,
        "PRODUCT('P','Tessellated','',())".into(),
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

    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}
