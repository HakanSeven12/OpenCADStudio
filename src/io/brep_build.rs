//! Assembles a B-rep record from the topology an exchange file spells out:
//! vertices, edges along their curves, faces bounded by loops of those
//! edges, shells of faces. The IGES reader uses it.
//!
//! The builder keeps the vertices and edges it is given and hands them, with
//! the faces of a body, to Solidcore's assembler each time a body is asked
//! for, so a body the assembler refuses as a solid can be asked for again as
//! a surface model.

use std::f64::consts::TAU;

use ocs_geom::space::{NurbsCurve3, NurbsSurface3};
use ocs_solid::model::{r, Brep};
use solidcore_step::assembly::{Assembly, Bound, Kind};

static TRIMS: ocs_solid::geometry::Traced = ocs_solid::geometry::Traced;

/// The curve an edge runs along, as the file gives it.
#[derive(Debug, Clone)]
pub enum EdgeCurve {
    /// The line through both points.
    Line { start: [f64; 3], end: [f64; 3] },
    /// The circle round the frame's z axis, its angle measured from x.
    Circle { frame: r::Frame, radius: f64 },
    Nurbs(NurbsCurve3),
}

/// A face as the file gives it: its surface, whether its side is the
/// surface's natural normal, and its bounds, relative to that normal (seen
/// from it an outer loop runs counter-clockwise).
pub struct FaceSpec {
    pub surface: r::Surface,
    pub forward: bool,
    pub loops: Vec<Bound>,
}

#[derive(Default)]
pub struct Builder {
    vertices: Vec<[f64; 3]>,
    /// Each edge's curve, its interval on it, and its vertices at the
    /// interval's start and end.
    edges: Vec<(r::Curve, [f64; 2], [u32; 2])>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertex(&mut self, point: [f64; 3]) -> u32 {
        self.vertices.push(point);
        (self.vertices.len() - 1) as u32
    }

    /// An edge along `curve` from `start` to `end`, and whether it runs that
    /// way: a curve that runs from `end` to `start` gives an edge running so.
    /// A circle runs counter-clockwise round its frame's z axis from `start`
    /// to `end`; an edge whose ends are one vertex is a whole circle, or a
    /// whole spline.
    pub fn edge(&mut self, curve: EdgeCurve, start: u32, end: u32) -> Option<(u32, bool)> {
        let a = *self.vertices.get(start as usize)?;
        let b = *self.vertices.get(end as usize)?;
        let closed = start == end;
        let (curve, t0, t1) = match curve {
            EdgeCurve::Line { start: s, end: e } => {
                let d = sub(e, s);
                let at = |p| dot(sub(p, s), d) / dot(d, d).max(f64::MIN_POSITIVE);
                (r::Curve::Line { start: s, end: e }, at(a), at(b))
            }
            EdgeCurve::Circle { frame, radius } => {
                let angle = |p| {
                    let q = sub(p, frame.origin);
                    dot(q, frame.y_axis)
                        .atan2(dot(q, frame.x_axis))
                        .rem_euclid(TAU)
                };
                let t0 = angle(a);
                let mut t1 = angle(b);
                if closed {
                    t1 = t0 + TAU;
                } else {
                    while t1 <= t0 + 1e-12 {
                        t1 += TAU;
                    }
                }
                (r::Curve::Circle { frame, radius }, t0, t1)
            }
            EdgeCurve::Nurbs(nurbs) => {
                // ponytail: a periodic spline's edge across its seam is read
                // as running back the other way; split it there if one shows.
                let (t0, t1) = if closed {
                    nurbs.domain()
                } else {
                    (nurbs.parameter_at(a), nurbs.parameter_at(b))
                };
                (spline_curve(&nurbs)?, t0, t1)
            }
        };
        let (interval, ends, along) = if t0 <= t1 {
            ([t0, t1], [start, end], true)
        } else {
            ([t1, t0], [end, start], false)
        };
        if !(interval[1] > interval[0]) || !interval[0].is_finite() || !interval[1].is_finite() {
            return None;
        }
        self.edges.push((curve, interval, ends));
        Some(((self.edges.len() - 1) as u32, along))
    }

    /// Turns a loop to run counter-clockwise round the surface's natural
    /// normal (an outer one) or clockwise (a hole), judged by the area it
    /// encloses in the surface's parameters.
    pub fn orient(&self, surface: &r::Surface, ring: &mut [(u32, bool)], outer: bool) {
        let mut points = Vec::new();
        for (edge, forward) in ring.iter() {
            let Some((curve, interval, _)) = self.edges.get(*edge as usize) else {
                return;
            };
            let Some(mut piece) = ocs_solid::geometry::points_on(curve, *interval, 8) else {
                return;
            };
            if !forward {
                piece.reverse();
            }
            piece.pop();
            points.extend(piece);
        }
        let Some(uv) = parameters_on(surface, &points) else {
            return;
        };
        let area: f64 = (0..uv.len())
            .map(|k| {
                let (a, b) = (uv[k], uv[(k + 1) % uv.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum();
        if (area > 0.0) != outer {
            ring.reverse();
            for (_, forward) in ring.iter_mut() {
                *forward = !*forward;
            }
        }
    }

    /// The body of `shells` (the outer one first, then voids; or the shells
    /// of a surface model), each a list of faces, made as `kind` at
    /// `tolerance`.
    pub fn body(&self, shells: &[Vec<FaceSpec>], kind: Kind, tolerance: f64) -> Result<Brep, String> {
        let mut assembly = Assembly::new(tolerance, Some(&TRIMS));
        for point in &self.vertices {
            assembly.add_vertex(*point)?;
        }
        // The assembler turns an edge whose vertices lie nearer the other
        // ends of its interval; its uses follow.
        let mut along = Vec::with_capacity(self.edges.len());
        for (k, (curve, interval, ends)) in self.edges.iter().enumerate() {
            along.push(assembly.add_edge(k as u64, curve.clone(), *interval, *ends)?.1);
        }
        let mut used = Vec::with_capacity(shells.len());
        let mut origin = 0u64;
        for faces in shells {
            let mut shell = Vec::with_capacity(faces.len());
            for spec in faces {
                let bounds = spec
                    .loops
                    .iter()
                    .map(|bound| match bound {
                        Bound::Edges(uses) => Bound::Edges(
                            uses.iter()
                                .map(|(e, f)| (*e, *f == along.get(*e as usize).copied().unwrap_or(true)))
                                .collect(),
                        ),
                        other => other.clone(),
                    })
                    .collect();
                let face = assembly.add_face(origin, spec.surface.clone(), bounds, None, spec.forward)?;
                origin += 1;
                shell.push((face, spec.forward));
            }
            used.push(shell);
        }
        Ok(assembly.finish(0, String::new(), used, kind)?.brep)
    }
}

/// A spline curve as a record curve.
pub fn spline_curve(nurbs: &NurbsCurve3) -> Option<r::Curve> {
    let weights = nurbs.is_rational().then(|| nurbs.weights());
    solidcore_step::assembly::spline_curve(
        nurbs.degree(),
        nurbs.knots().to_vec(),
        nurbs.control_points(),
        weights,
    )
    .ok()
}

/// A spline surface as a record surface.
pub fn spline_surface(nurbs: &NurbsSurface3) -> Option<r::Surface> {
    let (du, dv) = nurbs.degrees();
    let (uk, vk) = nurbs.knots();
    let weights = nurbs.weights();
    let first = *weights.first()?.first()?;
    let rational = weights.iter().flatten().any(|w| (w - first).abs() > 1e-12);
    solidcore_step::assembly::spline_surface(
        [du, dv],
        [uk.to_vec(), vk.to_vec()],
        nurbs.control_points(),
        rational.then_some(weights),
    )
    .ok()
}

/// The parameters of each of `points` on `surface`, each read near the one
/// before and kept on its turn of a surface that closes on itself.
fn parameters_on(surface: &r::Surface, points: &[[f64; 3]]) -> Option<Vec<[f64; 2]>> {
    let converted = solidcore_convert::surface(surface).ok()?;
    let eval = solidcore::eval::SurfaceEval::new(&converted).ok()?;
    let period = eval.domain().period;
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    let mut near = None;
    for p in points {
        let found = eval.project(solidcore_convert::vec3((*p).into()), near, f64::INFINITY)?;
        near = Some(found);
        let mut uv = found;
        if let Some(last) = out.last() {
            for d in 0..2 {
                if let Some(per) = period[d].filter(|per| per.is_finite() && *per > 0.0) {
                    uv[d] += per * ((last[d] - uv[d]) / per).round();
                }
            }
        }
        out.push(uv);
    }
    Some(out)
}

/// Whether `surface`'s natural normal runs the way `net`'s does, the spline
/// it stands for.
pub fn agrees(surface: &r::Surface, net: &NurbsSurface3) -> bool {
    if matches!(surface, r::Surface::Spline { .. } | r::Surface::RationalSpline { .. }) {
        return true;
    }
    let ((u0, u1), (v0, v1)) = net.domain();
    let (u, v) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
    let natural = || {
        let converted = solidcore_convert::surface(surface).ok()?;
        let eval = solidcore::eval::SurfaceEval::new(&converted).ok()?;
        let p = net.point_at_knot(u, v);
        let [s, t] = eval.project(solidcore_convert::vec3(p.into()), None, f64::INFINITY)?;
        let (_, su, sv) = eval.derivs(s, t)?;
        let [su, sv] = [su, sv].map(|w| {
            let w = solidcore_convert::record_vec3(w);
            [w.x, w.y, w.z]
        });
        Some(cross(su, sv))
    };
    match (net.normal_at_knot(u, v), natural()) {
        (Some(a), Some(b)) => dot(a, b) >= 0.0,
        _ => true,
    }
}
