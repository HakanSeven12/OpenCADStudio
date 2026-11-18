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

use ocs_brep::{
    Body, Curve3, EdgeKey, FaceKey, LoopKey, ShellKey, Surface, VertexKey,
};
use ocs_geom::space::{NurbsSurface3, Plane, Vec3};
use solidcore_record::step::model as m;
pub use step_export::Schema;

use crate::scene::model::mesh_model::MeshModel;

/// One object to export.
pub struct StepItem {
    /// The exact B-rep, when the object has one.
    pub body: Option<Body>,
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

fn scaled(point: [f64; 3], scale: f64) -> Lifted<[f64; 3]> {
    let out = point.map(|value| value * scale);
    if out.iter().all(|value| value.is_finite()) {
        Ok(out)
    } else {
        Err("non-finite coordinate")
    }
}

/// A plane's frame squared up: its normal, its x axis made square to it, and
/// y completing the right-handed set; the origin scaled.
fn frame(plane: &Plane, scale: f64) -> Lifted<m::Frame> {
    let z = Vec3::from(plane.normal().ok_or("degenerate frame")?);
    let x = Vec3::from(plane.x_axis);
    let x = (x - z * x.dot(z)).normalize().ok_or("degenerate frame")?;
    let y = z.cross(x);
    Ok(m::Frame {
        origin: scaled(plane.origin, scale)?,
        x: x.to_array(),
        y: y.to_array(),
        z: z.to_array(),
    })
}

/// Turns a frame half a turn about its z axis: the same circle with its
/// parameter moved by π, which is how a negative radius reads.
fn half_turn(frame: m::Frame) -> m::Frame {
    m::Frame {
        x: frame.x.map(|value| -value),
        y: frame.y.map(|value| -value),
        ..frame
    }
}

/// Distinct knots and their multiplicities.
fn knot_form(knots: &[f64]) -> (Vec<f64>, Vec<u32>) {
    let mut values: Vec<f64> = Vec::new();
    let mut mults: Vec<u32> = Vec::new();
    for &knot in knots {
        match (values.last(), mults.last_mut()) {
            (Some(&last), Some(mult)) if last == knot => *mult += 1,
            _ => {
                values.push(knot);
                mults.push(1);
            }
        }
    }
    (values, mults)
}

fn bspline_curve(
    degree: usize,
    points: Vec<[f64; 3]>,
    weights: Option<Vec<f64>>,
    knots: &[f64],
) -> Lifted<m::Curve> {
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err("non-finite control point");
    }
    let (knots, mults) = knot_form(knots);
    Ok(m::Curve::BSpline(m::BSplineCurve {
        degree: degree as u32,
        points,
        weights,
        knots,
        mults,
    }))
}

/// A curve in STEP's form, and the factor its parameter takes there (an
/// angle is in degrees).
fn curve(curve: &Curve3, scale: f64) -> Lifted<(m::Curve, f64)> {
    Ok(match curve {
        Curve3::Line(line) => {
            let direction = Vec3::from(line.direction);
            let dir = direction.normalize().ok_or("degenerate line")?;
            (
                m::Curve::Line {
                    origin: scaled(line.origin, scale)?,
                    dir: dir.to_array(),
                    magnitude: direction.length() * scale,
                },
                1.0,
            )
        }
        Curve3::Circle(circle) => {
            let frame = frame(&circle.plane, scale)?;
            let frame = if circle.radius < 0.0 {
                half_turn(frame)
            } else {
                frame
            };
            let radius = circle.radius.abs() * scale;
            (m::Curve::Circle { frame, radius }, 1f64.to_degrees())
        }
        Curve3::Ellipse(ellipse) => (
            m::Curve::Ellipse {
                frame: frame(&ellipse.plane, scale)?,
                semi1: ellipse.major_radius.abs() * scale,
                semi2: ellipse.minor_radius.abs() * scale,
            },
            1f64.to_degrees(),
        ),
        Curve3::PlanarSpline { plane, curve } => {
            let points = curve
                .control_points()
                .iter()
                .map(|&point| scaled(plane.point_at(point), scale))
                .collect::<Lifted<Vec<_>>>()?;
            let weights = curve.is_rational().then(|| curve.weights().to_vec());
            (
                bspline_curve(curve.degree(), points, weights, curve.knots())?,
                1.0,
            )
        }
        Curve3::Nurbs(curve) => {
            let points = curve
                .control_points()
                .iter()
                .map(|&point| scaled(point, scale))
                .collect::<Lifted<Vec<_>>>()?;
            let weights = curve.is_rational().then(|| curve.weights().to_vec());
            (
                bspline_curve(curve.degree(), points, weights, curve.knots())?,
                1.0,
            )
        }
    })
}

fn bspline_surface(surface: &NurbsSurface3, scale: f64) -> Lifted<m::Surface> {
    let points = surface
        .control_points()
        .iter()
        .map(|row| row.iter().map(|&point| scaled(point, scale)).collect())
        .collect::<Lifted<Vec<Vec<_>>>>()?;
    let rational = surface
        .weights()
        .iter()
        .flatten()
        .any(|weight| (weight - 1.0).abs() > 1e-12);
    let (u_degree, v_degree) = surface.degrees();
    let (u_knots, v_knots) = surface.knots();
    let (u_knots, u_mults) = knot_form(u_knots);
    let (v_knots, v_mults) = knot_form(v_knots);
    Ok(m::Surface::BSpline(m::BSplineSurface {
        u_degree: u_degree as u32,
        v_degree: v_degree as u32,
        points,
        weights: rational.then(|| surface.weights().to_vec()),
        u_knots,
        u_mults,
        v_knots,
        v_mults,
    }))
}

/// A surface in STEP's form. A spline whose `v` the kernel reads backwards
/// is written with its own net, so its normal is the other way round: the
/// flag says so.
fn surface(surface: &Surface, scale: f64) -> Lifted<(m::Surface, bool)> {
    Ok(match surface {
        Surface::Plane(plane) => (
            m::Surface::Plane {
                frame: frame(plane, scale)?,
            },
            false,
        ),
        Surface::Cylinder(cylinder) => (
            m::Surface::Cylinder {
                frame: frame(&cylinder.base, scale)?,
                radius: cylinder.radius.abs() * scale,
            },
            false,
        ),
        Surface::Cone(cone) => {
            // The kernel's radius shrinks along the axis by tan(half_angle);
            // STEP's grows by tan(semi_angle), from a radius that is not
            // negative. A cone narrowing along its axis is written about the
            // reversed axis, and one whose base radius is negative from its
            // apex.
            let tan = cone.half_angle.tan();
            let mut base = cone.base;
            let mut radius = cone.radius;
            if radius < 0.0 && tan != 0.0 {
                let normal = Vec3::from(base.normal().ok_or("degenerate frame")?);
                base.origin = (Vec3::from(base.origin) + normal * (radius / tan)).to_array();
                radius = 0.0;
            }
            let frame = frame(&base, scale)?;
            let frame = if cone.half_angle > 0.0 {
                m::Frame {
                    y: frame.y.map(|value| -value),
                    z: frame.z.map(|value| -value),
                    ..frame
                }
            } else {
                frame
            };
            (
                m::Surface::Cone {
                    frame,
                    radius: radius.abs() * scale,
                    semi_angle_deg: cone.half_angle.abs().to_degrees(),
                },
                false,
            )
        }
        Surface::Sphere(sphere) => (
            m::Surface::Sphere {
                frame: frame(&sphere.frame, scale)?,
                radius: sphere.radius.abs() * scale,
            },
            false,
        ),
        Surface::Torus(torus) => (
            m::Surface::Torus {
                frame: frame(&torus.frame, scale)?,
                major: torus.major_radius.abs() * scale,
                minor: torus.minor_radius.abs() * scale,
            },
            false,
        ),
        Surface::Nurbs(spline) => (bspline_surface(spline, scale)?, spline.v_reversed()),
        Surface::Offset(offset) => (
            bspline_surface(&offset.approximation, scale)?,
            offset.approximation.v_reversed(),
        ),
    })
}

/// STEP's normal of an analytic surface at a point on it: away from the
/// plane's front, the axis, the centre or the tube's core circle.
fn step_normal(surface: &m::Surface, point: [f64; 3]) -> Option<Vec3> {
    let radial = |frame: &m::Frame| {
        let offset = Vec3::from(point) - Vec3::from(frame.origin);
        let z = Vec3::from(frame.z);
        (offset - z * offset.dot(z)).normalize()
    };
    match surface {
        m::Surface::Plane { frame } => Some(Vec3::from(frame.z)),
        m::Surface::Cylinder { frame, .. } => radial(frame),
        m::Surface::Cone {
            frame,
            semi_angle_deg,
            ..
        } => {
            let angle = semi_angle_deg.to_radians();
            Some(radial(frame)? * angle.cos() - Vec3::from(frame.z) * angle.sin())
        }
        m::Surface::Sphere { frame, .. } => {
            (Vec3::from(point) - Vec3::from(frame.origin)).normalize()
        }
        m::Surface::Torus { frame, major, .. } => {
            let core = Vec3::from(frame.origin) + radial(frame)? * *major;
            (Vec3::from(point) - core).normalize()
        }
        _ => None,
    }
}

/// One lump of a body, gathered into a neutral body.
struct Lump<'a> {
    body: &'a Body,
    scale: f64,
    vertices: Vec<m::Vertex>,
    vertex_of: HashMap<VertexKey, usize>,
    edges: Vec<m::Edge>,
    edge_of: HashMap<EdgeKey, usize>,
    faces: Vec<m::Face>,
}

impl Lump<'_> {
    fn vertex(&mut self, key: VertexKey) -> Lifted<usize> {
        if let Some(&index) = self.vertex_of.get(&key) {
            return Ok(index);
        }
        let vertex = self.body.vertices.get(key).ok_or("missing vertex")?;
        let index = self.vertices.len();
        self.vertices.push(m::Vertex {
            step_id: index as u64 + 1,
            point: scaled(vertex.point, self.scale)?,
        });
        self.vertex_of.insert(key, index);
        Ok(index)
    }

    fn edge(&mut self, key: EdgeKey) -> Lifted<usize> {
        if let Some(&index) = self.edge_of.get(&key) {
            return Ok(index);
        }
        let edge = self.body.edges.get(key).ok_or("missing edge")?;
        let geometry = self.body.curves.get(edge.curve).ok_or("missing curve")?;
        let (curve, factor) = curve(geometry, self.scale)?;
        let start = self.vertex(edge.start)?;
        let end = self.vertex(edge.end)?;
        let index = self.edges.len();
        self.edges.push(m::Edge {
            step_id: index as u64 + 1,
            start,
            end,
            curve,
            same_sense: true,
            t_start: edge.start_parameter * factor,
            t_end: edge.end_parameter * factor,
        });
        self.edge_of.insert(key, index);
        Ok(index)
    }

    /// Points of a face's boundary in the middle of its edges, where its
    /// normal is compared.
    fn boundary_samples(&self, loops: &[LoopKey]) -> Vec<[f64; 3]> {
        let body = self.body;
        loops
            .iter()
            .filter_map(|&key| body.loops.get(key))
            .flat_map(|ring| ring.coedges.iter())
            .filter_map(|&coedge| {
                let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
                let curve = body.curves.get(edge.curve)?;
                Some(curve.point_at(0.5 * (edge.start_parameter + edge.end_parameter)))
            })
            .collect()
    }

    /// Adds a face; `reverse` turns it over (a void's faces, which STEP
    /// reverses again through the void shell's orientation).
    fn face(&mut self, key: FaceKey, reverse: bool) -> Lifted<usize> {
        let face = self.body.faces.get(key).ok_or("missing face")?;
        let geometry = self.body.surfaces.get(face.surface).ok_or("missing surface")?;
        let (written, mut flipped) = surface(geometry, self.scale)?;
        if !matches!(written, m::Surface::BSpline(_)) {
            // Which way the analytic surface's normal runs against STEP's,
            // seen at the first boundary point where both are clear.
            let samples = self.boundary_samples(&face.loops);
            flipped = samples
                .iter()
                .find_map(|&point| {
                    let (u, v) = geometry.parameters_at(point)?;
                    let kernel = Vec3::from(geometry.normal_at(u, v)?);
                    let step = step_normal(&written, scaled(point, self.scale).ok()?)?;
                    let agreement = kernel.dot(step);
                    (agreement.abs() > 0.2).then_some(agreement < 0.0)
                })
                .unwrap_or(false);
        }
        let mut loops = Vec::new();
        for &loop_key in &face.loops {
            let ring = self.body.loops.get(loop_key).ok_or("missing loop")?;
            let mut oriented = Vec::new();
            for &coedge_key in &ring.coedges {
                let coedge = self.body.coedges.get(coedge_key).ok_or("missing coedge")?;
                oriented.push(m::OrientedEdge {
                    edge: self.edge(coedge.edge)?,
                    forward: coedge.forward,
                });
            }
            if oriented.is_empty() {
                continue;
            }
            if reverse {
                oriented.reverse();
                for edge in &mut oriented {
                    edge.forward = !edge.forward;
                }
            }
            loops.push(m::Loop {
                outer: loops.is_empty(),
                orientation: true,
                bound: m::Bound::Edges(oriented),
            });
        }
        if loops.is_empty() {
            return Err("unbounded face");
        }
        let index = self.faces.len();
        self.faces.push(m::Face {
            step_id: index as u64 + 1,
            surface: written,
            same_sense: face.forward ^ flipped ^ reverse,
            loops,
        });
        Ok(index)
    }

    /// Whether every edge of a shell's faces is used twice.
    fn closed(&self, shell: ShellKey) -> bool {
        let body = self.body;
        body.shells.get(shell).is_some_and(|shell| {
            shell
                .faces
                .iter()
                .filter_map(|&face| body.faces.get(face))
                .flat_map(|face| face.loops.iter())
                .filter_map(|&ring| body.loops.get(ring))
                .flat_map(|ring| ring.coedges.iter())
                .all(|&coedge| {
                    body.coedges
                        .get(coedge)
                        .and_then(|coedge| body.edges.get(coedge.edge))
                        .is_some_and(|edge| edge.coedges.len() == 2)
                })
        })
    }
}

/// A kernel body as neutral bodies, one per lump: a solid when `solid` and
/// its shells close, otherwise a shell model.
fn body_models(body: &Body, scale: f64, solid: bool) -> Lifted<Vec<m::Body>> {
    let mut out = Vec::new();
    for &lump_key in &body.roots {
        let lump = body.lumps.get(lump_key).ok_or("missing lump")?;
        let mut gathered = Lump {
            body,
            scale,
            vertices: Vec::new(),
            vertex_of: HashMap::new(),
            edges: Vec::new(),
            edge_of: HashMap::new(),
            faces: Vec::new(),
        };
        let as_solid = solid && lump.shells.iter().all(|&shell| gathered.closed(shell));
        let mut shells = Vec::new();
        for (position, &shell_key) in lump.shells.iter().enumerate() {
            let shell = body.shells.get(shell_key).ok_or("missing shell")?;
            // A void's faces point out of the material, into the void; STEP
            // keeps its closed shell pointing out of the void and reverses
            // it through the oriented shell.
            let void = as_solid && position > 0;
            let faces = shell
                .faces
                .iter()
                .map(|&face| gathered.face(face, void))
                .collect::<Lifted<Vec<_>>>()?;
            if faces.is_empty() {
                continue;
            }
            shells.push(m::Shell {
                step_id: shells.len() as u64 + 1,
                closed: as_solid || gathered.closed(shell_key),
                orientation: !void,
                faces,
            });
        }
        if shells.is_empty() {
            continue;
        }
        let kind = match (as_solid, shells.len()) {
            (false, _) => m::BodyKind::ShellModel,
            (true, 1) => m::BodyKind::ManifoldSolid,
            (true, _) => m::BodyKind::SolidWithVoids,
        };
        out.push(m::Body {
            step_id: 0,
            name: None,
            kind,
            unit_mm: 1.0,
            uncertainty_mm: None,
            shells,
            faces: gathered.faces,
            edges: gathered.edges,
            vertices: gathered.vertices,
        });
    }
    if out.is_empty() {
        Err("empty body")
    } else {
        Ok(out)
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
    use ocs_brep::{make, Body};

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

    fn body_step(body: Body, solid: bool, unit_mm: f64) -> String {
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
    fn read_back(step: &str) -> Vec<Body> {
        let model = crate::io::step_import::read(step.as_bytes()).expect("readable");
        let mut bodies: Vec<Body> = model.direct.into_iter().map(|b| b.body).collect();
        for product in model.products {
            bodies.extend(product.bodies.into_iter().map(|b| b.body));
        }
        bodies
    }

    fn volume(body: &Body) -> f64 {
        ocs_brep::mass_properties(body).expect("mass").volume
    }

    /// Exports a solid, reads it back through both readers and compares
    /// volumes: the faces must point out of the material for that to hold.
    fn round_trip(body: Body, expected: f64, kinds: &[&str]) -> String {
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
        let hollow =
            ocs_brep::combine(outer, inner, ocs_brep::Operation::Difference, 1e-6)
                .expect("difference");
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
        let face = square.face_keys().next().unwrap();
        let sheet = ocs_brep::face_sheet(&square, &[face]).expect("sheet");
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
            body: Some(Body::new()),
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
