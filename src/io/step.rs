// STEP (ISO 10303-21) export of a drawing's 3D objects.
//
// Solids, regions and surfaces whose ACIS data lifts into a kernel body are
// written as exact B-reps: their planes, cylinders, cones, spheres, tori and
// spline surfaces, bounded by lines, circles, ellipses and spline curves, in
// a MANIFOLD_SOLID_BREP (a BREP_WITH_VOIDS when the solid has voids) or a
// SHELL_BASED_SURFACE_MODEL. Anything else with a display mesh is written as
// planar faces, coplanar triangles merged into one face. The bodies are
// mapped to the neutral model of the vendored `step-export` writer, which
// writes the product structure, millimetre units, and each body's name,
// layer and colour, under the AP203, AP214 or AP242 schema.

use std::collections::HashMap;

use ocs_solid::model::Brep;
use solidcore_record::step::model as m;
pub use step_export::Schema;

use crate::scene::model::mesh_model::MeshModel;

/// One object to export.
pub struct StepItem {
    /// The exact B-rep, when the object has one.
    pub body: Option<Brep>,
    /// Whether the body is a solid (a region or surface is a sheet).
    pub solid: bool,
    /// The display mesh, written when there is no body or it cannot be
    /// mapped.
    pub mesh: Option<MeshModel>,
    /// The representation item's name.
    pub name: String,
    /// Its presentation layer.
    pub layer: Option<String>,
    /// Its colour, as 0–1 RGB.
    pub color: Option<[f64; 3]>,
}

/// The STEP text of `items`, in a single product named `product`. Drawing
/// units are `unit_mm` millimetres each. `Err` when nothing could be
/// written.
pub fn build_step(
    items: &[StepItem],
    unit_mm: f64,
    schema: Schema,
    product: &str,
) -> Result<String, String> {
    let mut model = m::Model::default();
    let mut styles = Vec::new();
    for item in items {
        let bodies = item
            .body
            .as_ref()
            .and_then(|body| body_models(body, unit_mm, item.solid).ok())
            .or_else(|| {
                let mesh = item.mesh.as_ref()?;
                mesh_body(mesh, unit_mm).map(|body| vec![body])
            })
            .unwrap_or_default();
        for mut body in bodies {
            body.step_id = model.bodies.len() as u64 + 1;
            body.name = (!item.name.is_empty()).then(|| item.name.clone());
            model.bodies.push(body);
            styles.push(step_export::BodyStyle {
                layer: item.layer.clone(),
                color: item.color,
            });
        }
    }
    if model.bodies.is_empty() {
        return Err("no 3D data to export".into());
    }
    let mut metadata = step_export::ExportMetadata::new()
        .with_schema(schema)
        .with_product_name(product)
        .with_organization("OpenCADStudio");
    metadata.body_styles = styles;
    step_export::write_with_metadata(&model, &metadata).map_err(|error| error.message)
}

/// Parses a schema keyword: `AP203`, `AP214`, `AP242` (or the bare number).
pub fn parse_schema(text: &str) -> Option<Schema> {
    Schema::parse(text.trim())
}

type Lifted<T> = Result<T, &'static str>;

/// A record as neutral bodies, by Solidcore's own mapping ([`step_export::brep_to_model`]): a
/// solid per material region (a solid with voids where it has them), a shell model of each
/// sheet; scaled from drawing units to millimetres first.
fn body_models(body: &Brep, scale: f64, _solid: bool) -> Lifted<Vec<m::Body>> {
    let scaled;
    let body = if (scale - 1.0).abs() > 1e-12 {
        let placement = ocs_solid::model::Placement {
            x_axis: [scale, 0.0, 0.0],
            y_axis: [0.0, scale, 0.0],
            z_axis: [0.0, 0.0, scale],
            origin: [0.0; 3],
        };
        scaled = ocs_solid::model::placed(body, &placement).map_err(|_| "the body could not be scaled to millimetres")?;
        &scaled
    } else {
        body
    };
    // ponytail: through Solidcore's working body (its doc-hidden `kernel()` bridge at the
    // pinned revision); move to solidcore-step's record writer once it takes meshes, layers and
    // a schema.
    let (opened, _) = ocs_solid::model::open(body).map_err(|_| "the body could not be opened")?;
    let model = step_export::brep_to_model(&opened.kernel()).map_err(|_| "the body could not be mapped to STEP")?;
    if model.bodies.is_empty() {
        Err("empty body")
    } else {
        Ok(model.bodies)
    }
}

/// A display mesh as planar faces: positions welded, coplanar neighbouring
/// triangles merged into one face bounded by its outline and holes. A closed
/// mesh is a solid, an open one a shell model. `None` when it has no
/// triangles.
fn mesh_body(mesh: &MeshModel, scale: f64) -> Option<m::Body> {
    struct Tri {
        v: [usize; 3],
        n: [f64; 3],
    }
    let verts = &mesh.verts;
    let normals = &mesh.normals;
    let idx = &mesh.indices;
    // Positions are stored as an f32 half plus a low residual so they stay
    // precise at survey coordinates; write the sum, as the renderer uses.
    let point = |i: usize| {
        let high = verts[i];
        let low = mesh.verts_low.get(i).copied().unwrap_or([0.0; 3]);
        [0, 1, 2].map(|k| (high[k] as f64 + low[k] as f64) * scale)
    };
    // Faces share vertices by position: the tessellator repeats a vertex per
    // face where the normal changes, and a seam's two copies differ in the
    // last bits, but the shell is one surface. Positions weld on a grid of
    // 1e-9 of the mesh's size.
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for i in 0..verts.len() {
        let p = point(i);
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let size = (hi[0] - lo[0]).hypot(hi[1] - lo[1]).hypot(hi[2] - lo[2]);
    let grid = if size.is_finite() && size > 0.0 {
        size * 1e-9
    } else {
        1.0
    };
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut by_position: HashMap<[i64; 3], usize> = HashMap::new();
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
        if [a, b, c].iter().flatten().any(|value| !value.is_finite()) {
            continue;
        }
        let v = [vertex(a), vertex(b), vertex(c)];
        if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
            continue;
        }
        // The face is placed on the triangle's own plane. A smoothed vertex
        // normal leans off it, so only a triangle whose cross product is zero
        // falls back to the normal the mesh gives it. Small triangles have a
        // tiny cross product but still a plane of their own.
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
        return None;
    }

    // One line edge per vertex pair, running from the lower index, and how
    // many faces use it.
    let mut edges: Vec<m::Edge> = Vec::new();
    let mut edge_of: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
    let mut faces: Vec<m::Face> = Vec::new();
    // A face bounded by `rings` (vertex rings, the outer one first) on the
    // plane through its first vertex with normal `n`.
    let mut add_face = |rings: &[Vec<usize>], n: [f64; 3]| {
        let mut loops = Vec::new();
        for ring in rings {
            let mut oriented = Vec::new();
            for k in 0..ring.len() {
                let (from, to) = (ring[k], ring[(k + 1) % ring.len()]);
                let key = (from.min(to), from.max(to));
                let edge = match edge_of.get_mut(&key) {
                    Some(entry) => {
                        entry.1 += 1;
                        entry.0
                    }
                    None => {
                        let (p, q) = (points[key.0], points[key.1]);
                        let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
                        let length = d[0].hypot(d[1]).hypot(d[2]).max(f64::EPSILON);
                        edges.push(m::Edge {
                            step_id: edges.len() as u64 + 1,
                            start: key.0,
                            end: key.1,
                            curve: m::Curve::Line {
                                origin: p,
                                dir: d.map(|value| value / length),
                                magnitude: length,
                            },
                            same_sense: true,
                            t_start: 0.0,
                            t_end: 1.0,
                        });
                        edge_of.insert(key, (edges.len() - 1, 1));
                        edges.len() - 1
                    }
                };
                oriented.push(m::OrientedEdge {
                    edge,
                    forward: from == key.0,
                });
            }
            loops.push(m::Loop {
                outer: loops.is_empty(),
                orientation: true,
                bound: m::Bound::Edges(oriented),
            });
        }
        // The reference direction: the first edge, made square to the normal
        // (a degenerate triangle's edge need not be).
        let [nx, ny, nz] = n;
        let outer = &rings[0];
        let (p, q) = (points[outer[0]], points[outer[1]]);
        let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
        let along = d[0] * nx + d[1] * ny + d[2] * nz;
        let r = [d[0] - along * nx, d[1] - along * ny, d[2] - along * nz];
        let r_len = r[0].hypot(r[1]).hypot(r[2]);
        let x = if r_len > 0.0 {
            r.map(|value| value / r_len)
        } else if nx.abs() < 0.9 {
            [0.0, -nz, ny]
        } else {
            [-nz, 0.0, nx]
        };
        let y = [ny * x[2] - nz * x[1], nz * x[0] - nx * x[2], nx * x[1] - ny * x[0]];
        faces.push(m::Face {
            step_id: faces.len() as u64 + 1,
            surface: m::Surface::Plane {
                frame: m::Frame {
                    origin: p,
                    x,
                    y,
                    z: n,
                },
            },
            same_sense: true,
            loops,
        });
    };

    // Coplanar neighbours are one face, as a modeller writes a box's six
    // faces rather than its twelve triangles: triangles that share an edge
    // and lie on one plane are grouped, and each group is written bounded by
    // its outline and holes.
    let mut by_edge: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
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
    let mut groups: Vec<Vec<usize>> = Vec::new();
    // Root -> index into `groups`, so grouping a large mesh stays linear.
    let mut group_of_root: HashMap<usize, usize> = HashMap::new();
    for t in 0..tris.len() {
        let r = root(&mut parent, t);
        match group_of_root.get(&r) {
            Some(&index) => groups[index].push(t),
            None => {
                group_of_root.insert(r, groups.len());
                groups.push(vec![t]);
            }
        }
    }
    for members in &groups {
        let n = tris[members[0]].n;
        match outline(members.iter().map(|&t| tris[t].v), &points, n) {
            Some(rings) => add_face(&rings, n),
            // An outline that does not close into rings: its triangles.
            None => {
                for &t in members {
                    add_face(&[tris[t].v.to_vec()], tris[t].n);
                }
            }
        }
    }

    let closed = edge_of.values().all(|(_, uses)| *uses == 2);
    Some(m::Body {
        step_id: 0,
        name: None,
        kind: if closed {
            m::BodyKind::ManifoldSolid
        } else {
            m::BodyKind::ShellModel
        },
        unit_mm: 1.0,
        uncertainty_mm: None,
        shells: vec![m::Shell {
            step_id: 1,
            closed,
            orientation: true,
            faces: (0..faces.len()).collect(),
        }],
        faces,
        edges,
        vertices: points
            .iter()
            .enumerate()
            .map(|(index, &point)| m::Vertex {
                step_id: index as u64 + 1,
                point,
            })
            .collect(),
    })
}

/// Web: no save dialog — the export continues with a virtual filename and
/// the generated bytes are delivered as a browser download.
#[cfg(target_arch = "wasm32")]
pub async fn pick_step_path_owned(stem: String) -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(format!("{stem}.step")))
}

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
#[cfg(test)]
mod tests {
    use super::{build_step, mesh_body, Schema, StepItem};
    use crate::scene::model::mesh_model::MeshModel;
    use ocs_solid::model::{self, Brep};

    /// The primitives the tests export, from Solidcore.
    mod make {
        use ocs_solid::model::{self, Brep};

        pub fn cuboid(origin: [f64; 3], size: [f64; 3]) -> Option<Brep> {
            model::make_box(origin, size).ok()
        }

        pub fn cylinder(base: [f64; 3], radius: f64, height: f64) -> Option<Brep> {
            model::make_cylinder(base, radius, height).ok()
        }

        pub fn cone(base: [f64; 3], radius: f64, height: f64) -> Option<Brep> {
            model::make_cone(base, radius, 0.0, height).ok()
        }

        pub fn frustum(base: [f64; 3], x_radius: f64, y_radius: f64, top: f64, height: f64) -> Option<Brep> {
            model::make_frustum(base, x_radius, y_radius, top, height).ok()
        }

        pub fn sphere(centre: [f64; 3], radius: f64) -> Option<Brep> {
            model::make_sphere(centre, radius).ok()
        }

        pub fn torus(centre: [f64; 3], major: f64, minor: f64) -> Option<Brep> {
            model::make_torus(centre, major, minor).ok()
        }
    }

    fn mesh_step(mesh: MeshModel) -> String {
        let item = StepItem {
            body: None,
            solid: true,
            mesh: Some(mesh),
            name: String::new(),
            layer: None,
            color: None,
        };
        build_step(&[item], 1.0, Schema::Ap214, "part").expect("step")
    }

    fn body_step(body: Brep, solid: bool, unit_mm: f64) -> String {
        let item = StepItem {
            body: Some(body),
            solid,
            mesh: None,
            name: "part".into(),
            layer: None,
            color: None,
        };
        build_step(&[item], unit_mm, Schema::Ap214, "part").expect("step")
    }

    /// The bodies OCS's own reader makes of a STEP text, unscaled.
    fn read_back(step: &str) -> Vec<Brep> {
        let model = crate::io::step_import::read(step.as_bytes()).expect("readable");
        let mut bodies: Vec<Brep> = model.direct.into_iter().map(|b| b.body).collect();
        for product in model.products {
            bodies.extend(product.bodies.into_iter().map(|b| b.body));
        }
        bodies
    }

    fn volume(body: &Brep) -> f64 {
        ocs_solid::model::mass_properties(body).expect("mass").volume
    }

    /// Exports a solid, reads it back through both readers and compares
    /// volumes: the faces must point out of the material for that to hold.
    fn round_trip(body: Brep, expected: f64, kinds: &[&str]) -> String {
        let step = body_step(body, true, 1.0);
        for kind in kinds {
            assert!(step.contains(kind), "no {kind}:\n{step}");
        }
        assert!(!step.contains("POLY_LOOP"), "{step}");
        let back = read_back(&step);
        assert_eq!(back.len(), 1, "{step}");
        let got = volume(&back[0]);
        assert!(
            (got - expected).abs() <= expected * 1e-3,
            "volume {got} != {expected}:\n{step}"
        );
        let neutral = step_export::import(step.as_bytes(), &step_export::Limits::default())
            .expect("the mapper reads it");
        assert_eq!(neutral.bodies.len(), 1, "{:?}", neutral.skipped);
        step
    }

    #[test]
    fn a_box_round_trips_as_an_exact_solid() {
        let step = round_trip(
            make::cuboid([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap(),
            6000.0,
            &["MANIFOLD_SOLID_BREP", "CLOSED_SHELL", "PLANE("],
        );
        assert_eq!(step.matches("ADVANCED_FACE").count(), 6);
        assert!(step.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN'))"));
    }

    #[test]
    fn a_cylinder_round_trips() {
        let expected = std::f64::consts::PI * 25.0 * 10.0;
        round_trip(
            make::cylinder([0.0, 0.0, 0.0], 5.0, 10.0).unwrap(),
            expected,
            &["CYLINDRICAL_SURFACE", "CIRCLE("],
        );
    }

    #[test]
    fn a_cone_round_trips() {
        let expected = std::f64::consts::PI * 16.0 * 9.0 / 3.0;
        round_trip(
            make::cone([1.0, 1.0, 0.0], 4.0, 9.0).unwrap(),
            expected,
            &["CONICAL_SURFACE"],
        );
    }

    #[test]
    fn a_frustum_round_trips() {
        let (r, top, h) = (6.0, 2.0, 5.0);
        let expected = std::f64::consts::PI * h / 3.0 * (r * r + r * top + top * top);
        round_trip(
            make::frustum([0.0, 0.0, 0.0], r, r, top, h).unwrap(),
            expected,
            &["CONICAL_SURFACE"],
        );
    }

    #[test]
    fn a_sphere_round_trips() {
        let expected = 4.0 / 3.0 * std::f64::consts::PI * 27.0;
        round_trip(
            make::sphere([2.0, 0.0, 0.0], 3.0).unwrap(),
            expected,
            &["SPHERICAL_SURFACE"],
        );
    }

    #[test]
    fn a_torus_round_trips() {
        let expected = 2.0 * std::f64::consts::PI.powi(2) * 6.0 * 1.5 * 1.5;
        round_trip(
            make::torus([0.0, 0.0, 0.0], 6.0, 1.5).unwrap(),
            expected,
            &["TOROIDAL_SURFACE"],
        );
    }

    #[test]
    fn a_spline_walled_cone_round_trips() {
        // An elliptical cone's wall is a spline surface.
        let (a, b, h) = (6.0, 4.0, 9.0);
        let expected = std::f64::consts::PI * a * b * h / 3.0;
        round_trip(
            make::frustum([0.0, 0.0, 0.0], a, b, 0.0, h).unwrap(),
            expected,
            &["B_SPLINE_SURFACE"],
        );
    }

    #[test]
    fn a_solid_with_a_void_keeps_it() {
        let outer = make::cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        let inner = make::cuboid([3.0, 3.0, 3.0], [4.0, 4.0, 4.0]).unwrap();
        let hollow = model::boolean(&outer, &inner, model::BooleanOp::Subtract)
            .expect("difference")
            .expect("something left");
        round_trip(
            hollow,
            1000.0 - 64.0,
            &["BREP_WITH_VOIDS", "ORIENTED_CLOSED_SHELL"],
        );
    }

    #[test]
    fn drawing_units_are_written_in_millimetres() {
        let body = make::cuboid([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let step = body_step(body, true, 25.4);
        let back = read_back(&step);
        let got = volume(&back[0]);
        assert!((got - 25.4f64.powi(3)).abs() < 1e-6 * got, "{got}");
    }

    #[test]
    fn a_sheet_is_a_shell_based_surface_model() {
        let square = make::cuboid([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap();
        let face = (0..square.faces.len() as u32).next().unwrap();
        let sheet = model::face_sheet(&square, &[face]).expect("sheet");
        let step = body_step(sheet, false, 1.0);
        assert!(step.contains("SHELL_BASED_SURFACE_MODEL"), "{step}");
        assert!(step.contains("OPEN_SHELL"), "{step}");
        assert!(step.contains("MANIFOLD_SURFACE_SHAPE_REPRESENTATION"), "{step}");
        assert_eq!(read_back(&step).len(), 1);
    }

    #[test]
    fn each_object_keeps_its_layer_and_colour() {
        let item = |x: f64, layer: &str, color: [f64; 3]| StepItem {
            body: make::cuboid([x, 0.0, 0.0], [1.0, 1.0, 1.0]),
            solid: true,
            mesh: None,
            name: layer.to_lowercase(),
            layer: Some(layer.into()),
            color: Some(color),
        };
        let items = [
            item(0.0, "A", [1.0, 0.0, 0.0]),
            item(5.0, "B", [0.0, 0.0, 1.0]),
            item(9.0, "A", [1.0, 0.0, 0.0]),
        ];
        let step = build_step(&items, 1.0, Schema::Ap242, "part").unwrap();
        assert!(step.contains("FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'))"));
        assert_eq!(step.matches("PRESENTATION_LAYER_ASSIGNMENT").count(), 2, "{step}");
        assert_eq!(step.matches("COLOUR_RGB").count(), 2, "{step}");
        assert_eq!(step.matches("STYLED_ITEM").count(), 3, "{step}");
        assert!(step.contains("MANIFOLD_SOLID_BREP('b'"), "{step}");
        let model = crate::io::step_import::read(step.as_bytes()).unwrap();
        let colours: Vec<_> = model
            .direct
            .iter()
            .chain(model.products.iter().flat_map(|p| p.bodies.iter()))
            .map(|b| b.colour)
            .collect();
        assert_eq!(
            colours,
            [Some([1.0, 0.0, 0.0]), Some([0.0, 0.0, 1.0]), Some([1.0, 0.0, 0.0])]
        );
    }

    #[test]
    fn a_body_that_cannot_be_written_falls_back_to_its_mesh() {
        let item = StepItem {
            body: Some(Brep {
                version: 1,
                tolerance: 1e-6,
                regions: Vec::new(),
                shells: Vec::new(),
                faces: Vec::new(),
                edges: Vec::new(),
                vertices: Vec::new(),
            }),
            solid: true,
            mesh: Some(cube()),
            name: String::new(),
            layer: None,
            color: None,
        };
        let step = build_step(&[item], 1.0, Schema::Ap203, "part").unwrap();
        assert!(step.contains("FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'))"));
        assert_eq!(step.matches("ADVANCED_FACE").count(), 6, "{step}");
    }

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
        let body = mesh_body(&triangle(verts, verts_low), 1.0).expect("body");
        let first = body.vertices[0].point;
        assert!((first[0] - 500000.123).abs() < 1e-6, "{first:?}");
        assert!((first[1] - 500010.456).abs() < 1e-6, "{first:?}");
    }

    /// Meshes without residuals (imported OBJ, legacy meshes) export as before.
    #[test]
    fn a_mesh_without_low_residuals_exports_its_positions() {
        let mesh = triangle(
            vec![[1.5, 2.0, 0.0], [2.5, 2.0, 0.0], [1.5, 3.0, 0.0]],
            Vec::new(),
        );
        let step = mesh_step(mesh);
        assert!(step.contains("CARTESIAN_POINT('',(1.5,2.0,0.))"), "{step}");
    }

    /// The normal of each face's plane.
    fn plane_normals(mesh: &MeshModel) -> Vec<[f64; 3]> {
        let body = mesh_body(mesh, 1.0).expect("body");
        body.faces
            .iter()
            .map(|face| match &face.surface {
                solidcore_record::step::model::Surface::Plane { frame } => frame.z,
                other => panic!("not a plane: {other:?}"),
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
        assert_eq!(plane_normals(&mesh), vec![[0.0, 0.0, 1.0]]);
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
        assert_eq!(plane_normals(&mesh), vec![[0.0, 0.0, 1.0]]);
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
        assert_eq!(plane_normals(&mesh), vec![[0.0, 1.0, 0.0]]);
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
        let step = mesh_step(cube());
        assert_eq!(step.matches("ADVANCED_FACE").count(), 6, "{step}");
        assert_eq!(step.matches("EDGE_CURVE").count(), 12);
        assert!(step.contains("CLOSED_SHELL") && step.contains("MANIFOLD_SOLID_BREP"));
        let back = read_back(&step);
        assert!((volume(&back[0]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_face_with_a_hole_keeps_both_bounds() {
        // A square with a square hole, as eight triangles in one plane.
        let p = |x: f32, y: f32| [x, y, 0.0];
        let verts = vec![
            p(0.0, 0.0),
            p(3.0, 0.0),
            p(3.0, 3.0),
            p(0.0, 3.0),
            p(1.0, 1.0),
            p(2.0, 1.0),
            p(2.0, 2.0),
            p(1.0, 2.0),
        ];
        let indices = vec![
            0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7,
        ];
        let mesh = MeshModel {
            indices,
            ..triangle(verts, Vec::new())
        };
        let step = mesh_step(mesh);
        assert_eq!(step.matches("ADVANCED_FACE").count(), 1, "{step}");
        assert_eq!(step.matches("FACE_OUTER_BOUND").count(), 1);
        assert_eq!(step.matches("=FACE_BOUND").count(), 1);
        assert!(step.contains("SHELL_BASED_SURFACE_MODEL"), "{step}");
    }
}
