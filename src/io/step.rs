// STEP AP214 export: tessellated MeshModels as an ISO 10303-21 file.
//
// Each mesh becomes a faceted B-rep: its triangles are ADVANCED_FACEs on
// their own PLANEs, sharing vertices and edges, in a CLOSED_SHELL when every
// edge bounds two triangles (a MANIFOLD_SOLID_BREP) and an OPEN_SHELL
// otherwise (a SHELL_BASED_SURFACE_MODEL). The shapes hang off the product
// structure (PRODUCT ... SHAPE_DEFINITION_REPRESENTATION) that importers
// start from, with millimetre units.

use crate::scene::model::mesh_model::MeshModel;
use std::fmt::Write as FmtWrite;

/// Build a STEP AP214 text representation from a slice of mesh models.
///
/// Returns `None` if there are no triangles to export.
pub fn build_step(meshes: &[&MeshModel]) -> Option<String> {
    struct Tri {
        v: [usize; 3],
        n: [f64; 3],
    }

    let mut data = String::new();
    // Start past the first growth doublings; the fixed header block alone is
    // already a couple of kilobytes.
    data.reserve(1 << 16);
    let mut next_id: usize = 1;
    let mut alloc = || {
        let id = next_id;
        next_id += 1;
        id
    };
    let mut items: Vec<usize> = Vec::new();
    let mut all_solids = true;

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
        // Faces share vertices by position: the tessellator repeats a vertex
        // per face where the normal changes, and a seam's two copies differ
        // in the last bits, but the shell is one surface. Positions weld on a
        // grid of 1e-9 of the mesh's size.
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for i in 0..verts.len() {
            let p = point(i);
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let size = (hi[0] - lo[0]).hypot(hi[1] - lo[1]).hypot(hi[2] - lo[2]);
        let grid = if size.is_finite() && size > 0.0 { size * 1e-9 } else { 1.0 };
        let mut points: Vec<[f64; 3]> = Vec::new();
        let mut by_position: std::collections::HashMap<[i64; 3], usize> =
            std::collections::HashMap::new();
        let mut vertex = |p: [f64; 3]| {
            let key = [0, 1, 2].map(|k| ((p[k] - lo[k]) / grid).round() as i64);
            *by_position.entry(key).or_insert_with(|| {
                points.push(p);
                points.len() - 1
            })
        };
        let mut tris: Vec<Tri> = Vec::new();
        for t in 0..idx.len() / 3 {
            let i = [idx[t * 3], idx[t * 3 + 1], idx[t * 3 + 2]].map(|i| i as usize);
            if i.iter().any(|&i| i >= verts.len()) {
                continue;
            }
            let [a, b, c] = i.map(point);
            let v = [vertex(a), vertex(b), vertex(c)];
            if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
                continue;
            }
            // The face is placed on the triangle's own plane. A smoothed vertex
            // normal leans off it, so only a triangle whose cross product is
            // zero falls back to the normal the mesh gives it. Small triangles
            // have a tiny cross product but still a plane of their own.
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let len = cross[0].hypot(cross[1]).hypot(cross[2]);
            let n = if len == 0.0 && i[0] < normals.len() {
                normals[i[0]].map(f64::from)
            } else {
                let len = len.max(f64::MIN_POSITIVE);
                cross.map(|value| value / len)
            };
            tris.push(Tri { v, n });
        }
        if tris.is_empty() {
            continue;
        }

        let point_ids: Vec<usize> = points
            .iter()
            .map(|&[x, y, z]| {
                let cp = alloc();
                let vp = alloc();
                writeln!(data, "#{cp} = CARTESIAN_POINT('',({x:.6},{y:.6},{z:.6}));").ok();
                writeln!(data, "#{vp} = VERTEX_POINT('',#{cp});").ok();
                cp
            })
            .collect();
        // VERTEX_POINT ids follow their CARTESIAN_POINT.
        let vertex_id = |v: usize| point_ids[v] + 1;

        // One EDGE_CURVE per vertex pair, oriented from the lower index, and
        // how many faces use it.
        let mut edges: std::collections::HashMap<(usize, usize), (usize, usize)> =
            std::collections::HashMap::new();
        let mut faces = Vec::new();
        // Per-face scratch for the comma-joined entity ids: reused across
        // faces so a large mesh does not allocate a formatted String per
        // oriented edge and bound.
        let mut edge_ids = String::new();
        let mut bound_ids = String::new();
        // A face bounded by `loops` (vertex rings, the outer one first) on
        // the plane through its first vertex with normal `n`.
        let mut write_face = |loops: &[Vec<usize>], n: [f64; 3], data: &mut String| {
            bound_ids.clear();
            for (index, ring) in loops.iter().enumerate() {
                edge_ids.clear();
                for k in 0..ring.len() {
                    let (from, to) = (ring[k], ring[(k + 1) % ring.len()]);
                    let key = (from.min(to), from.max(to));
                    let curve = match edges.get_mut(&key) {
                        Some(entry) => {
                            entry.1 += 1;
                            entry.0
                        }
                        None => {
                            let (start, end) = key;
                            let (p, q) = (points[start], points[end]);
                            let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
                            let length = d[0].hypot(d[1]).hypot(d[2]).max(f64::EPSILON);
                            let (dir, vector, line, curve) = (alloc(), alloc(), alloc(), alloc());
                            writeln!(
                                data,
                                "#{dir} = DIRECTION('',({:.6},{:.6},{:.6}));",
                                d[0] / length,
                                d[1] / length,
                                d[2] / length
                            )
                            .ok();
                            writeln!(data, "#{vector} = VECTOR('',#{dir},{length:.6});").ok();
                            writeln!(data, "#{line} = LINE('',#{},#{vector});", point_ids[start])
                                .ok();
                            writeln!(
                                data,
                                "#{curve} = EDGE_CURVE('',#{},#{},#{line},.T.);",
                                vertex_id(start),
                                vertex_id(end)
                            )
                            .ok();
                            edges.insert(key, (curve, 1));
                            curve
                        }
                    };
                    let id = alloc();
                    let sense = if from == key.0 { ".T." } else { ".F." };
                    writeln!(data, "#{id} = ORIENTED_EDGE('',*,*,#{curve},{sense});").ok();
                    if !edge_ids.is_empty() {
                        edge_ids.push(',');
                    }
                    write!(edge_ids, "#{id}").ok();
                }
                let (lp, bound) = (alloc(), alloc());
                writeln!(data, "#{lp} = EDGE_LOOP('',({edge_ids}));").ok();
                let kind = if index == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" };
                writeln!(data, "#{bound} = {kind}('',#{lp},.T.);").ok();
                if !bound_ids.is_empty() {
                    bound_ids.push(',');
                }
                write!(bound_ids, "#{bound}").ok();
            }
            let (normal, reference, placement, plane, face) =
                (alloc(), alloc(), alloc(), alloc(), alloc());
            let [nx, ny, nz] = n;
            writeln!(data, "#{normal} = DIRECTION('',({nx:.6},{ny:.6},{nz:.6}));").ok();
            // The reference direction: the first edge, made square to the
            // normal (a degenerate triangle's edge need not be).
            let outer = &loops[0];
            let (p, q) = (points[outer[0]], points[outer[1]]);
            let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
            let along = d[0] * nx + d[1] * ny + d[2] * nz;
            let r = [d[0] - along * nx, d[1] - along * ny, d[2] - along * nz];
            let r_len = r[0].hypot(r[1]).hypot(r[2]);
            let r = if r_len > 0.0 {
                r.map(|value| value / r_len)
            } else if nx.abs() < 0.9 {
                [0.0, -nz, ny]
            } else {
                [-nz, 0.0, nx]
            };
            writeln!(
                data,
                "#{reference} = DIRECTION('',({:.6},{:.6},{:.6}));",
                r[0], r[1], r[2]
            )
            .ok();
            writeln!(
                data,
                "#{placement} = AXIS2_PLACEMENT_3D('',#{},#{normal},#{reference});",
                point_ids[outer[0]]
            )
            .ok();
            writeln!(data, "#{plane} = PLANE('',#{placement});").ok();
            writeln!(
                data,
                "#{face} = ADVANCED_FACE('',({bound_ids}),#{plane},.T.);"
            )
            .ok();
            faces.push(face);
        };

        // Coplanar neighbours are one face, as a modeller writes a box's six
        // faces rather than its twelve triangles: triangles that share an
        // edge and lie on one plane are grouped, and each group is written
        // bounded by its outline and holes.
        let mut by_edge: std::collections::HashMap<(usize, usize), Vec<usize>> =
            std::collections::HashMap::new();
        for (t, tri) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (tri.v[k], tri.v[(k + 1) % 3]);
                by_edge.entry((a.min(b), a.max(b))).or_default().push(t);
            }
        }
        let mut parent: Vec<usize> = (0..tris.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let offset = |tri: &Tri| {
            let p = points[tri.v[0]];
            tri.n[0] * p[0] + tri.n[1] * p[1] + tri.n[2] * p[2]
        };
        let mut shared: Vec<_> = by_edge.values().filter(|users| users.len() == 2).collect();
        shared.sort();
        for users in shared {
            let (a, b) = (&tris[users[0]], &tris[users[1]]);
            let facing = a.n[0] * b.n[0] + a.n[1] * b.n[1] + a.n[2] * b.n[2];
            if facing > 1.0 - 1e-9 && (offset(a) - offset(b)).abs() <= grid * 1e3 {
                let (ra, rb) = (root(&mut parent, users[0]), root(&mut parent, users[1]));
                if ra != rb {
                    parent[ra.max(rb)] = ra.min(rb);
                }
            }
        }
        let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
        // Root -> index into `groups`. A linear find here made the grouping
        // quadratic: with one face per triangle it re-scanned every earlier
        // group for each triangle of a large mesh.
        let mut group_of_root: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        for t in 0..tris.len() {
            let r = root(&mut parent, t);
            match group_of_root.get(&r) {
                Some(&index) => groups[index].1.push(t),
                None => {
                    group_of_root.insert(r, groups.len());
                    groups.push((r, vec![t]));
                }
            }
        }
        for (_, members) in &groups {
            let n = tris[members[0]].n;
            match outline(members.iter().map(|&t| tris[t].v), &points, n) {
                Some(loops) => write_face(&loops, n, &mut data),
                // An outline that does not close into rings: its triangles.
                None => {
                    for &t in members {
                        write_face(&[tris[t].v.to_vec()], tris[t].n, &mut data);
                    }
                }
            }
        }

        let mut face_ids = String::new();
        for id in &faces {
            if !face_ids.is_empty() {
                face_ids.push(',');
            }
            write!(face_ids, "#{id}").ok();
        }
        let closed = edges.values().all(|(_, uses)| *uses == 2);
        let (shell, item) = (alloc(), alloc());
        if closed {
            writeln!(data, "#{shell} = CLOSED_SHELL('',({face_ids}));").ok();
            writeln!(data, "#{item} = MANIFOLD_SOLID_BREP('',#{shell});").ok();
        } else {
            all_solids = false;
            writeln!(data, "#{shell} = OPEN_SHELL('',({face_ids}));").ok();
            writeln!(data, "#{item} = SHELL_BASED_SURFACE_MODEL('',(#{shell}));").ok();
        }
        items.push(item);
    }

    if items.is_empty() {
        return None;
    }

    // ── Units, context and the product structure ─────────────────────────
    let (app, protocol, product_context, product, formation, definition_context) =
        (alloc(), alloc(), alloc(), alloc(), alloc(), alloc());
    let (definition, definition_shape, length, angle, solid_angle, uncertainty) =
        (alloc(), alloc(), alloc(), alloc(), alloc(), alloc());
    let (context, representation, shape_definition, category) =
        (alloc(), alloc(), alloc(), alloc());
    writeln!(
        data,
        "#{app} = APPLICATION_CONTEXT('core data for automotive mechanical design processes');"
    )
    .ok();
    writeln!(
        data,
        "#{protocol} = APPLICATION_PROTOCOL_DEFINITION('international standard',\
         'automotive_design',2000,#{app});"
    )
    .ok();
    writeln!(data, "#{product_context} = PRODUCT_CONTEXT('',#{app},'mechanical');").ok();
    writeln!(
        data,
        "#{product} = PRODUCT('OpenCADStudio','OpenCADStudio','',(#{product_context}));"
    )
    .ok();
    writeln!(data, "#{formation} = PRODUCT_DEFINITION_FORMATION('','',#{product});").ok();
    writeln!(
        data,
        "#{definition_context} = PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design');"
    )
    .ok();
    writeln!(
        data,
        "#{definition} = PRODUCT_DEFINITION('design','',#{formation},#{definition_context});"
    )
    .ok();
    writeln!(data, "#{definition_shape} = PRODUCT_DEFINITION_SHAPE('','',#{definition});").ok();
    writeln!(
        data,
        "#{category} = PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{product}));"
    )
    .ok();
    writeln!(data, "#{length} = (LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));").ok();
    writeln!(data, "#{angle} = (NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));").ok();
    writeln!(
        data,
        "#{solid_angle} = (NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT());"
    )
    .ok();
    writeln!(
        data,
        "#{uncertainty} = UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{length},\
         'distance_accuracy_value','confusion accuracy');"
    )
    .ok();
    writeln!(
        data,
        "#{context} = (GEOMETRIC_REPRESENTATION_CONTEXT(3)\
         GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty}))\
         GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{angle},#{solid_angle}))\
         REPRESENTATION_CONTEXT('Context3D','3D Context with 1e-06 uncertainty'));"
    )
    .ok();
    let mut item_ids = String::new();
    for id in &items {
        if !item_ids.is_empty() {
            item_ids.push(',');
        }
        write!(item_ids, "#{id}").ok();
    }
    // Surface models are not B-rep solids, so a mixed export stays a plain
    // shape representation.
    let kind = if all_solids {
        "ADVANCED_BREP_SHAPE_REPRESENTATION"
    } else {
        "SHAPE_REPRESENTATION"
    };
    writeln!(
        data,
        "#{representation} = {kind}('OpenCADStudio',({item_ids}),#{context});"
    )
    .ok();
    writeln!(
        data,
        "#{shape_definition} = SHAPE_DEFINITION_REPRESENTATION(#{definition_shape},\
         #{representation});"
    )
    .ok();

    // ── Assemble file ─────────────────────────────────────────────────────
    let ts = chrono_timestamp();
    let file = format!(
        "ISO-10303-21;\n\
         HEADER;\n\
         FILE_DESCRIPTION(('Open CAD Studio STEP export'),'2;1');\n\
         FILE_NAME('export.step','{ts}',(''),(''),'Open CAD Studio','Open CAD Studio','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\n\
         ENDSEC;\n\
         DATA;\n\
         {data}\
         ENDSEC;\n\
         END-ISO-10303-21;\n"
    );

    Some(file)
}

/// Web: no save dialog — the export continues with a virtual filename and
/// the generated bytes are delivered as a browser download.
#[cfg(target_arch = "wasm32")]
pub async fn pick_step_path_owned(stem: String) -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(format!("{stem}.step")))
}

/// Returns an ISO 8601-like timestamp string for the STEP file header.
/// The rings bounding a set of triangles that share one plane: its edges used
/// once, chained end to start, the ring enclosing the most area (seen along
/// `n`) first. `None` when they do not close into rings.
fn outline(
    triangles: impl Iterator<Item = [usize; 3]>,
    points: &[[f64; 3]],
    n: [f64; 3],
) -> Option<Vec<Vec<usize>>> {
    let mut directed = std::collections::HashSet::new();
    let mut order = Vec::new();
    for v in triangles {
        for k in 0..3 {
            let edge = (v[k], v[(k + 1) % 3]);
            if directed.insert(edge) {
                order.push(edge);
            }
        }
    }
    let mut next: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    let mut left = 0;
    for &(a, b) in &order {
        if !directed.contains(&(b, a)) {
            next.entry(a).or_default().push(b);
            left += 1;
        }
    }
    let mut rings = Vec::new();
    for &(start, _) in &order {
        while next.get(&start).is_some_and(|ends| !ends.is_empty()) {
            let mut ring = vec![start];
            let mut at = start;
            loop {
                let to = next.get_mut(&at)?.pop()?;
                left -= 1;
                if to == start {
                    break;
                }
                ring.push(to);
                at = to;
                if ring.len() > order.len() {
                    return None;
                }
            }
            if ring.len() < 3 {
                return None;
            }
            rings.push(ring);
        }
    }
    if left != 0 || rings.is_empty() {
        return None;
    }
    // Twice the area each ring encloses, signed along the normal.
    let area = |ring: &Vec<usize>| {
        let mut sum = [0.0f64; 3];
        for k in 0..ring.len() {
            let (p, q) = (points[ring[k]], points[ring[(k + 1) % ring.len()]]);
            sum[0] += p[1] * q[2] - p[2] * q[1];
            sum[1] += p[2] * q[0] - p[0] * q[2];
            sum[2] += p[0] * q[1] - p[1] * q[0];
        }
        sum[0] * n[0] + sum[1] * n[1] + sum[2] * n[2]
    };
    let outer = (0..rings.len()).max_by(|&a, &b| area(&rings[a]).total_cmp(&area(&rings[b])))?;
    rings.swap(0, outer);
    Some(rings)
}

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
    // Format: YYYY-MM-DDTHH:MM:SS, UTC.
    let (hh, mm, ss) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let (year, month, day) = crate::patreon::civil_from_days((secs / 86_400) as i64);
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

    /// A unit cube as twelve triangles, each face's corners repeated (as the
    /// tessellator does where the normal changes).
    fn cube() -> MeshModel {
        let corner = |i: usize| [(i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32];
        // Each face's corners, counter-clockwise seen from outside.
        let faces = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let mut verts = Vec::new();
        let mut indices = Vec::new();
        for face in faces {
            let base = verts.len() as u32;
            verts.extend(face.map(corner));
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        MeshModel {
            indices,
            ..triangle(verts, Vec::new())
        }
    }

    #[test]
    fn a_box_exports_six_planar_faces_in_a_closed_shell() {
        let mesh = cube();
        let step = build_step(&[&mesh]).unwrap();
        assert_eq!(step.matches("ADVANCED_FACE").count(), 6, "{step}");
        assert_eq!(step.matches("EDGE_CURVE").count(), 12);
        assert!(step.contains("CLOSED_SHELL") && step.contains("MANIFOLD_SOLID_BREP"));
    }

    #[test]
    fn a_face_with_a_hole_keeps_both_bounds() {
        // A square with a square hole, as eight triangles in one plane.
        let p = |x: f32, y: f32| [x, y, 0.0];
        let verts = vec![
            p(0.0, 0.0), p(3.0, 0.0), p(3.0, 3.0), p(0.0, 3.0),
            p(1.0, 1.0), p(2.0, 1.0), p(2.0, 2.0), p(1.0, 2.0),
        ];
        let indices = vec![
            0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7,
        ];
        let mesh = MeshModel {
            indices,
            ..triangle(verts, Vec::new())
        };
        let step = build_step(&[&mesh]).unwrap();
        assert_eq!(step.matches("ADVANCED_FACE").count(), 1, "{step}");
        assert_eq!(step.matches("FACE_OUTER_BOUND").count(), 1);
        assert_eq!(step.matches("= FACE_BOUND").count(), 1);
    }

}

