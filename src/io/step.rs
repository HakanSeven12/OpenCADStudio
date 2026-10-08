// STEP AP203 export — converts tessellated MeshModels to ISO 10303-21 format.
//
// The output is a minimal but valid STEP AP203 file containing:
//   - One SHAPE_REPRESENTATION per solid mesh
//   - ADVANCED_FACE → PLANE → AXIS2_PLACEMENT_3D for each triangle
//   - VERTEX_POINT / EDGE_CURVE / ORIENTED_EDGE topology
//
// Because building full B-Rep topology from a triangle soup is complex, we use
// a simplified encoding: each triangle becomes a CLOSED_SHELL with three
// ADVANCED_FACEs, each face bounded by three oriented edges.
//
// This produces larger-than-optimal files but is universally importable by
// CAD systems that accept AP203.

use crate::scene::model::mesh_model::MeshModel;
use std::fmt::Write as FmtWrite;

/// Build a STEP AP203 text representation from a slice of mesh models.
///
/// Returns `None` if there are no triangles to export.
pub fn build_step(meshes: &[&MeshModel]) -> Option<String> {
    // Collect all triangles as (v0, v1, v2, normal).
    struct Tri {
        v: [[f64; 3]; 3],
        n: [f32; 3],
    }

    let mut tris: Vec<Tri> = Vec::new();
    for mesh in meshes {
        let verts = &mesh.verts;
        let normals = &mesh.normals;
        let idx = &mesh.indices;
        // Positions are stored as an f32 half plus a low residual so they stay
        // precise at survey coordinates; write the sum, as the renderer uses.
        let point = |i: usize| {
            let high = verts[i];
            let low = mesh.verts_low.get(i).copied().unwrap_or([0.0; 3]);
            [0, 1, 2].map(|k| high[k] as f64 + low[k] as f64)
        };
        let n_tri = idx.len() / 3;
        for t in 0..n_tri {
            let i0 = idx[t * 3] as usize;
            let i1 = idx[t * 3 + 1] as usize;
            let i2 = idx[t * 3 + 2] as usize;
            if i0 >= verts.len() || i1 >= verts.len() || i2 >= verts.len() {
                continue;
            }
            let a = point(i0);
            let b = point(i1);
            let c = point(i2);
            // The face is placed on the triangle's own plane. A smoothed vertex
            // normal leans off it, so only a triangle whose cross product is
            // zero falls back to the normal the mesh gives it. Small triangles
            // have a tiny cross product but still a plane of their own.
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let nx = ab[1] * ac[2] - ab[2] * ac[1];
            let ny = ab[2] * ac[0] - ab[0] * ac[2];
            let nz = ab[0] * ac[1] - ab[1] * ac[0];
            let len = nx.hypot(ny).hypot(nz);
            let n = if len == 0.0 && i0 < normals.len() {
                normals[i0]
            } else {
                let len = len.max(f64::MIN_POSITIVE);
                [(nx / len) as f32, (ny / len) as f32, (nz / len) as f32]
            };
            tris.push(Tri { v: [a, b, c], n });
        }
    }

    if tris.is_empty() {
        return None;
    }

    // ── Emit STEP ─────────────────────────────────────────────────────────
    // Entity ID counter (STEP ids start at #1).
    let mut next_id: usize = 1;
    let mut data = String::new();

    // Closure to allocate the next ID.
    let mut alloc = || {
        let id = next_id;
        next_id += 1;
        id
    };

    // Collect face IDs for the shell.
    let mut face_ids: Vec<usize> = Vec::with_capacity(tris.len());

    for tri in &tris {
        // Each triangle: 3 vertices, 3 edges, 1 face.

        // Vertex points.
        let vp: [usize; 3] = [alloc(), alloc(), alloc()];
        // Cartesian points for vertices.
        let cp: [usize; 3] = [alloc(), alloc(), alloc()];
        // Line curves for edges.
        let lc: [usize; 3] = [alloc(), alloc(), alloc()];
        // Direction refs for lines (reusing cp[0] as direction — simplified).
        let dir: [usize; 3] = [alloc(), alloc(), alloc()];
        // Vertex-point refs.
        let vpref: [usize; 3] = [alloc(), alloc(), alloc()];
        // Edge curves.
        let ec: [usize; 3] = [alloc(), alloc(), alloc()];
        // Oriented edges.
        let oe: [usize; 3] = [alloc(), alloc(), alloc()];
        // Edge loop.
        let el = alloc();
        // Plane normal direction and axis placement.
        let norm_dir = alloc();
        let plane_ax = alloc();
        let plane = alloc();
        // Advanced face.
        let face_id = alloc();
        face_ids.push(face_id);

        // Emit cartesian points.
        for k in 0..3 {
            let [x, y, z] = tri.v[k];
            writeln!(
                data,
                "#{} = CARTESIAN_POINT('',({:.6},{:.6},{:.6}));",
                cp[k], x, y, z
            )
            .ok();
            writeln!(data, "#{} = VERTEX_POINT('',#{});", vpref[k], cp[k]).ok();
        }
        // Emit vertex points (binding).
        for k in 0..3 {
            writeln!(data, "#{} = VERTEX_POINT('',#{});", vp[k], cp[k]).ok();
            // (duplicate of vpref; we keep vp[] to reference in edge curves)
            let _ = vp[k]; // suppress unused warning
        }

        // Emit edge directions and line curves.
        for k in 0..3 {
            let k1 = (k + 1) % 3;
            let [dx, dy, dz] = [
                tri.v[k1][0] - tri.v[k][0],
                tri.v[k1][1] - tri.v[k][1],
                tri.v[k1][2] - tri.v[k][2],
            ];
            let len = (dx * dx + dy * dy + dz * dz).sqrt().max(f64::EPSILON);
            writeln!(
                data,
                "#{} = DIRECTION('',({:.6},{:.6},{:.6}));",
                dir[k],
                dx / len,
                dy / len,
                dz / len
            )
            .ok();
            writeln!(
                data,
                "#{} = LINE('',#{},VECTOR('',#{},1.));",
                lc[k], cp[k], dir[k]
            )
            .ok();
            writeln!(
                data,
                "#{} = EDGE_CURVE('',#{},#{},#{},.T.);",
                ec[k], vpref[k], vpref[k1], lc[k]
            )
            .ok();
            writeln!(data, "#{} = ORIENTED_EDGE('',*,*,#{},.T.);", oe[k], ec[k]).ok();
        }

        // Edge loop and face normal.
        writeln!(
            data,
            "#{} = EDGE_LOOP('',({},{},{}));",
            el,
            format!("#{}", oe[0]),
            format!("#{}", oe[1]),
            format!("#{}", oe[2])
        )
        .ok();

        let [nx, ny, nz] = tri.n;
        writeln!(
            data,
            "#{} = DIRECTION('',({:.6},{:.6},{:.6}));",
            norm_dir, nx, ny, nz
        )
        .ok();
        writeln!(
            data,
            "#{} = AXIS2_PLACEMENT_3D('',#{},#{},#{});",
            plane_ax, cp[0], norm_dir, dir[0]
        )
        .ok();
        writeln!(data, "#{} = PLANE('',#{});", plane, plane_ax).ok();
        writeln!(
            data,
            "#{} = ADVANCED_FACE('',(FACE_BOUND('',#{},.T.)),#{},.T.);",
            face_id, el, plane
        )
        .ok();
    }

    // Closed shell wrapping all faces.
    let shell_id = alloc();
    let face_list: String = face_ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    writeln!(data, "#{} = CLOSED_SHELL('',({face_list}));", shell_id).ok();

    // Manifold solid B-rep.
    let msb_id = alloc();
    writeln!(
        data,
        "#{} = MANIFOLD_SOLID_BREP('OpenCADStudio_Solid',#{});",
        msb_id, shell_id
    )
    .ok();

    // Shape representation.
    let sr_id = alloc();
    let pu_id = alloc();
    let gc_id = alloc();
    writeln!(
        data,
        "#{} = (LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));",
        pu_id
    )
    .ok();
    writeln!(data, "#{} = GEOMETRIC_REPRESENTATION_CONTEXT(3);", gc_id).ok();
    writeln!(
        data,
        "#{sr_id} = SHAPE_REPRESENTATION('OpenCADStudio_Shape',(#{}),#{gc_id});",
        msb_id
    )
    .ok();

    // ── Assemble file ─────────────────────────────────────────────────────
    let ts = chrono_timestamp();
    let file = format!(
        "ISO-10303-21;\n\
         HEADER;\n\
         FILE_DESCRIPTION(('Open CAD Studio STEP export'),'2;1');\n\
         FILE_NAME('{ts}','','',(''),'',' ',' ');\n\
         FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\n\
         ENDSEC;\n\
         DATA;\n\
         {data}\
         ENDSEC;\n\
         END-ISO-10303-21;\n"
    );

    Some(file)
}

/// Returns an ISO 8601-like timestamp string for the STEP file header.
fn chrono_timestamp() -> String {
    // Use seconds since Unix epoch for a simple timestamp without chrono dep.
    // `SystemTime::now` panics in the browser; ask the page's clock there. (#761)
    #[cfg(target_arch = "wasm32")]
    let secs = (js_sys::Date::now() / 1000.0) as u64;
    #[cfg(not(target_arch = "wasm32"))]
    let secs = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    };
    // Format: YYYY-MM-DDTHH:MM:SS (approximate UTC from epoch seconds).
    let s = secs;
    let mins = s / 60;
    let hours = mins / 60;
    let days = hours / 24;
    let hh = hours % 24;
    let mm = mins % 60;
    let ss = s % 60;
    // Days since epoch → year/month/day (approximate, ignoring leap years).
    let year = 1970 + days / 365;
    let doy = days % 365;
    let month = doy / 30 + 1;
    let day = doy % 30 + 1;
    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}")
}

#[cfg(test)]
mod tests {
    use super::build_step;
    use crate::scene::model::mesh_model::MeshModel;

    fn triangle(verts: Vec<[f32; 3]>, verts_low: Vec<[f32; 3]>) -> MeshModel {
        MeshModel {
            name: String::new(),
            verts,
            verts_low,
            normals: Vec::new(),
            indices: vec![0, 1, 2],
            triangle_material_handles: Vec::new(),
            triangle_colors: Vec::new(),
            color: [1.0; 4],
            selected: false,
        }
    }

    /// Split an absolute position into the f32 pair the tessellator stores.
    fn split(point: [f64; 3]) -> ([f32; 3], [f32; 3]) {
        let high = point.map(|v| v as f32);
        let low = [0, 1, 2].map(|i| (point[i] - high[i] as f64) as f32);
        (high, low)
    }

    /// At survey coordinates the f32 half alone is 6 cm coarse: 500000.123
    /// is stored as 500000.125 plus a -0.002 residual. The export has to add
    /// the residual back, as the renderer does.
    #[test]
    fn large_coordinates_keep_their_low_residual() {
        let (verts, verts_low): (Vec<_>, Vec<_>) = [
            [500000.123, 500010.456, 0.0],
            [500001.123, 500010.456, 0.0],
            [500000.123, 500011.456, 0.0],
        ]
        .into_iter()
        .map(split)
        .unzip();
        let step = build_step(&[&triangle(verts, verts_low)]).expect("step");
        assert!(
            step.contains("CARTESIAN_POINT('',(500000.123000,500010.456000,0.000000));"),
            "first vertex lost its residual:\n{}",
            step.lines()
                .find(|line| line.contains("CARTESIAN_POINT"))
                .unwrap_or("")
        );
    }

    /// Meshes without residuals (imported OBJ, legacy meshes) export as before.
    #[test]
    fn a_mesh_without_low_residuals_exports_its_positions() {
        let mesh = triangle(
            vec![[1.5, 2.0, 0.0], [2.5, 2.0, 0.0], [1.5, 3.0, 0.0]],
            Vec::new(),
        );
        let step = build_step(&[&mesh]).expect("step");
        assert!(step.contains("CARTESIAN_POINT('',(1.500000,2.000000,0.000000));"));
    }

    /// The direction each face's PLANE is placed with, read back through its
    /// AXIS2_PLACEMENT_3D. The export writes one face per triangle.
    fn plane_normals(step: &str) -> Vec<[f64; 3]> {
        let direction = |id: &str| -> [f64; 3] {
            let prefix = format!("{id} = DIRECTION('',(");
            let line = step
                .lines()
                .find(|line| line.starts_with(&prefix))
                .unwrap_or_else(|| panic!("no DIRECTION {id}"));
            let values: Vec<f64> = line[prefix.len()..]
                .trim_end_matches("));")
                .split(',')
                .map(|value| value.parse().expect("number"))
                .collect();
            [values[0], values[1], values[2]]
        };
        step.lines()
            .filter_map(|line| line.split_once(" = AXIS2_PLACEMENT_3D('',"))
            .map(|(_, refs)| {
                let refs: Vec<&str> = refs.trim_end_matches(");").split(',').collect();
                direction(refs[1])
            })
            .collect()
    }

    /// A curved surface shares smoothed normals across its facets, so a
    /// vertex normal leans away from the triangle. The face's PLANE has to be
    /// the triangle's own plane, or its corners do not lie on it.
    #[test]
    fn a_face_plane_follows_the_triangle_not_its_vertex_normal() {
        let mut mesh = triangle(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            Vec::new(),
        );
        let leaning = [
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
        ];
        mesh.normals = vec![leaning; 3];
        let step = build_step(&[&mesh]).expect("step");
        assert_eq!(plane_normals(&step), vec![[0.0, 0.0, 1.0]]);
    }

    /// A small triangle still has a plane of its own. Its cross product is
    /// tiny (1e-18 here), far below f64::EPSILON, but it is not zero.
    #[test]
    fn a_small_triangle_keeps_its_own_plane() {
        let mut mesh = triangle(
            vec![[0.0, 0.0, 0.0], [1e-9, 0.0, 0.0], [0.0, 1e-9, 0.0]],
            Vec::new(),
        );
        let leaning = [
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
        ];
        mesh.normals = vec![leaning; 3];
        let step = build_step(&[&mesh]).expect("step");
        assert_eq!(plane_normals(&step), vec![[0.0, 0.0, 1.0]]);
    }

    /// A triangle with no area has no plane of its own, so it keeps the
    /// normal the mesh gives it rather than a zero direction.
    #[test]
    fn a_degenerate_triangle_keeps_its_vertex_normal() {
        let mut mesh = triangle(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            Vec::new(),
        );
        mesh.normals = vec![[0.0, 1.0, 0.0]; 3];
        let step = build_step(&[&mesh]).expect("step");
        assert_eq!(plane_normals(&step), vec![[0.0, 1.0, 0.0]]);
    }
}
