//! This crate's [`Body`] as Solidcore's record (schema 4, [`solidcore_record::brep`]) and
//! back: the one door between the model OpenCADStudio edits and the kernel that operates on it.
//!
//! Outward, a body goes through Solidcore's own assembler ([`Assembly`]), the stage its STEP
//! reader builds records with: each vertex, each edge on its curve and each face on its surface
//! is added as it is, and the assembler makes everything a record needs that this model does not
//! keep (seams, every use's pcurve, the face domains, tolerances, regions and canonical order).
//! Inward, the record is canonical already and maps across field by field.
//!
//! The two models agree on what every curve and surface parameter means (an angle about a
//! frame's x axis, a height along its z, knots as given), with two exceptions handled here: a
//! line's record parameter runs 0 → 1 between its ends, and a cone's half angle has the
//! opposite sign (this model's cone narrows along its axis for a positive angle).
//!
//! Loops: this model runs a loop counter-clockwise about its face's outward normal, the record
//! about its surface's natural normal; a face whose normal is against its surface's has its
//! loops run backwards on the way across.

use std::collections::HashMap;
use std::f64::consts::TAU;

use ocs_geom::space::{NurbsCurve3, NurbsSurface3, Plane, Vec3};
use solidcore_ops::pcurve::{pcurves_within, EdgeUse, TrimFace, Trims};
use solidcore_record::brep as r;
use solidcore_step::assembly::{Assembly, Bound, Kind};

use crate::{
    Body, Circle3, Coedge, CoedgeKey, Cone, Curve3, Cylinder, Edge, EdgeKey, Ellipse3, Face,
    FaceKey, Line3, Loop, Lump, Provenance, Shell, Sphere, Surface, Torus, Vertex, VertexKey,
};

/// Which entity of the body each record face, edge and vertex is, by record index.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Correspondence {
    /// `faces[i]` is record face `i`.
    pub faces: Vec<FaceKey>,
    /// `edges[i]` is record edge `i`, when the body has an edge there (an assembler's seam on a
    /// periodic face has none).
    pub edges: Vec<Option<EdgeKey>>,
    /// `vertices[i]` is record vertex `i`, when the body has one there.
    pub vertices: Vec<Option<VertexKey>>,
}

impl Correspondence {
    /// The record index of `face`.
    pub fn face_index(&self, face: FaceKey) -> Option<u32> {
        self.faces.iter().position(|f| *f == face).map(|i| i as u32)
    }

    /// The record index of `edge`.
    pub fn edge_index(&self, edge: EdgeKey) -> Option<u32> {
        self.edges.iter().position(|e| *e == Some(edge)).map(|i| i as u32)
    }
}

/// Makes the pcurves the assembler has no closed form for, with Solidcore's own projection
/// ([`pcurves_within`]).
struct KernelTrims;

impl Trims for KernelTrims {
    fn trims(&self, face: &TrimFace<'_>) -> Result<Vec<Vec<r::Curve2>>, String> {
        let surface = face.nurbs.unwrap_or(face.surface);
        let mut loops = Vec::with_capacity(face.loops.len());
        for l in &face.loops {
            let mut uses = Vec::with_capacity(l.len());
            for u in l {
                let e = face.edges.get(u.edge as usize).ok_or("an edge outside the face")?;
                uses.push(EdgeUse {
                    edge: u.edge,
                    curve: e.nurbs.unwrap_or(e.curve),
                    interval: e.interval,
                    forward: u.forward,
                    tolerance: e.tolerance,
                    given: u.pcurve.as_ref(),
                });
            }
            loops.push(uses);
        }
        match pcurves_within(surface, &loops, face.tolerance, Some(face.domain)) {
            Ok(found) => Ok(found),
            // No closed form (a spline edge on a spline face): each missing one traced.
            Err(e) => traced_trims(face).ok_or_else(|| format!("pcurves: {e:?}")),
        }
    }
}

/// A face's missing pcurves traced: each edge sampled along its interval and every sample read
/// back into the face's (u, v), joined by straight spans on the edge's own parameter (so each
/// sample sits at its edge parameter exactly). `None` where a point does not read back.
fn traced_trims(face: &TrimFace<'_>) -> Option<Vec<Vec<r::Curve2>>> {
    // The surface the face's (u, v) are on, and how to read a point on it.
    enum On {
        Spline(NurbsSurface3),
        Plane { origin: Vec3, u: Vec3, v: Vec3 },
        Analytic(Surface, f64),
    }
    let surface = face.nurbs.unwrap_or(face.surface);
    let on = match surface {
        r::Surface::Spline { .. } | r::Surface::RationalSpline { .. } => On::Spline(spline_surface(surface)?),
        r::Surface::Plane { origin, u_axis, v_axis } => On::Plane {
            origin: Vec3::from(*origin),
            u: Vec3::from(*u_axis),
            v: Vec3::from(*v_axis),
        },
        r::Surface::Torus { tube_start, .. } => On::Analytic(label_surface(surface)?, *tube_start),
        _ => On::Analytic(label_surface(surface)?, 0.0),
    };
    let read = |p: [f64; 3]| -> Option<[f64; 2]> {
        match &on {
            On::Spline(n) => n.parameters_at(p).map(|(u, v)| [u, v]),
            On::Plane { origin, u, v } => {
                // (p - o) = a u + b v, solved by least squares.
                let d = Vec3::from(p) - *origin;
                let (uu, uv, vv) = (u.dot(*u), u.dot(*v), v.dot(*v));
                let det = uu * vv - uv * uv;
                (det.abs() > 1e-300).then(|| {
                    let (du, dv) = (d.dot(*u), d.dot(*v));
                    [(du * vv - dv * uv) / det, (dv * uu - du * uv) / det]
                })
            }
            On::Analytic(s, shift) => surface_parameters(s, p).map(|uv| [uv[0], uv[1] - shift]),
        }
    };
    let periodic = match &on {
        On::Analytic(s, _) => periodic_directions(s),
        _ => [false, false],
    };
    const N: usize = 48;
    let mut out = Vec::with_capacity(face.loops.len());
    for l in &face.loops {
        let mut pcs = Vec::with_capacity(l.len());
        for u in l {
            if let Some(given) = &u.pcurve {
                pcs.push(given.clone());
                continue;
            }
            let e = face.edges.get(u.edge as usize)?;
            let [t0, t1] = e.interval;
            let (curve, _) = model_curve(e.nurbs.unwrap_or(e.curve), e.interval)?;
            let ts: Vec<f64> = (0..=N).map(|i| t0 + (t1 - t0) * i as f64 / N as f64).collect();
            let mut points: Vec<[f64; 2]> = ts.iter().map(|t| read(curve.point_at(*t))).collect::<Option<_>>()?;
            for d in 0..2 {
                if periodic[d] {
                    let mut values: Vec<f64> = points.iter().map(|p| p[d]).collect();
                    unwrap(&mut values);
                    // Onto the turn the face's domain is on.
                    let [lo, hi] = face.domain[d];
                    let mid = 0.5 * (values[0] + values[values.len() - 1]);
                    let shift = TAU * ((0.5 * (lo + hi) - mid) / TAU).round();
                    for (p, v) in points.iter_mut().zip(values) {
                        p[d] = v + shift;
                    }
                }
            }
            let mut knots = vec![t0];
            knots.extend(ts.iter().copied());
            knots.push(t1);
            pcs.push(r::Curve2::Spline { knots, points });
        }
        out.push(pcs);
    }
    Some(out)
}

fn frame_of(plane: &Plane) -> r::Frame {
    let unit = |v: Vec3| v.normalize().unwrap_or(v);
    let x = unit(Vec3::from(plane.x_axis));
    let y = Vec3::from(plane.y_axis);
    // Orthonormal: the y axis with any x part taken out.
    let y = unit(y - x * y.dot(x));
    r::Frame {
        origin: plane.origin,
        x_axis: x.to_array(),
        y_axis: y.to_array(),
    }
}

fn plane_of(frame: &r::Frame) -> Option<Plane> {
    let x = Vec3::from(frame.x_axis);
    let y = Vec3::from(frame.y_axis);
    Plane::orthonormal(frame.origin, x.to_array(), x.cross(y).to_array())
}

pub(crate) fn record_spline(curve: &NurbsCurve3) -> r::Curve {
    let knots = curve.knots().to_vec();
    let weights = curve.weights();
    if weights.iter().all(|w| (*w - 1.0).abs() < 1e-15) {
        r::Curve::Spline {
            knots,
            points: curve.control_points().to_vec(),
        }
    } else {
        r::Curve::RationalSpline {
            knots,
            homogeneous: curve
                .control_points()
                .iter()
                .zip(weights)
                .map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w])
                .collect(),
        }
    }
}

/// An edge's curve as the record takes it, with its interval there.
fn record_curve(curve: &Curve3, interval: [f64; 2]) -> Option<(r::Curve, [f64; 2])> {
    Some(match curve {
        Curve3::Line(_) => (
            r::Curve::Line {
                start: curve.point_at(interval[0]),
                end: curve.point_at(interval[1]),
            },
            [0.0, 1.0],
        ),
        Curve3::Circle(c) => (
            r::Curve::Circle {
                frame: frame_of(&c.plane),
                radius: c.radius,
            },
            interval,
        ),
        Curve3::Ellipse(e) => (
            r::Curve::Ellipse {
                frame: frame_of(&e.plane),
                radii: [e.major_radius, e.minor_radius],
            },
            interval,
        ),
        Curve3::PlanarSpline { plane, curve } => {
            let lifted = NurbsCurve3::new(
                curve.degree(),
                curve.control_points().iter().map(|p| plane.point_at(*p)).collect(),
                curve.knots().to_vec(),
                Some(curve.weights().to_vec()),
            )?;
            (record_spline(&lifted), interval)
        }
        Curve3::Nurbs(n) => (record_spline(n), interval),
    })
}

pub(crate) fn record_spline_surface(s: &NurbsSurface3) -> r::Surface {
    let (u_knots, v_knots) = s.knots();
    let (mut u_knots, mut v_knots) = (u_knots.to_vec(), v_knots.to_vec());
    let mut points: Vec<Vec<[f64; 3]>> = s.control_points().to_vec();
    let mut weights: Vec<Vec<f64>> = s.weights().to_vec();
    if s.v_reversed() {
        // v read backwards: the knots mirrored about their range, each row's points reversed.
        let (lo, hi) = (v_knots[0], v_knots[v_knots.len() - 1]);
        v_knots = v_knots.iter().rev().map(|k| lo + hi - k).collect();
        points.iter_mut().for_each(|row| row.reverse());
        weights.iter_mut().for_each(|row| row.reverse());
    }
    let _ = &mut u_knots;
    if weights.iter().flatten().all(|w| (*w - 1.0).abs() < 1e-15) {
        r::Surface::Spline {
            u_knots,
            v_knots,
            points,
        }
    } else {
        r::Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous: points
                .iter()
                .zip(&weights)
                .map(|(row, ws)| {
                    row.iter()
                        .zip(ws)
                        .map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w])
                        .collect()
                })
                .collect(),
        }
    }
}

fn record_surface(surface: &Surface) -> r::Surface {
    match surface {
        Surface::Plane(p) => {
            let f = frame_of(p);
            r::Surface::Plane {
                origin: f.origin,
                u_axis: f.x_axis,
                v_axis: f.y_axis,
            }
        }
        Surface::Cylinder(c) => r::Surface::Cylinder {
            frame: frame_of(&c.base),
            radius: c.radius,
        },
        Surface::Cone(c) => r::Surface::Cone {
            frame: frame_of(&c.base),
            radius_at_origin: c.radius,
            half_angle: -c.half_angle,
        },
        Surface::Sphere(s) => r::Surface::Sphere {
            frame: frame_of(&s.frame),
            radius: s.radius,
        },
        Surface::Torus(t) => r::Surface::Torus {
            frame: frame_of(&t.frame),
            major_radius: t.major_radius,
            minor_radius: t.minor_radius,
            tube_start: 0.0,
        },
        Surface::Nurbs(n) => record_spline_surface(n),
        Surface::Offset(o) => record_spline_surface(&o.approximation),
    }
}

/// `body` as Solidcore's record, and which of its entities each record entity is.
///
/// # Errors
/// What the assembler refused, in its words, or a body this model holds that the record has no
/// place for (a body of several solids with voids).
pub fn to_record(body: &Body) -> Result<(r::Brep, Correspondence), String> {
    let tolerance = crate::operation_tolerance(&[body]).clamp(1e-7, 1e-3);
    let trims = KernelTrims;
    let mut asm = Assembly::new(tolerance, Some(&trims));

    let mut vertex_index: HashMap<VertexKey, u32> = HashMap::new();
    for (key, vertex) in body.vertices.iter() {
        vertex_index.insert(key, asm.add_vertex(vertex.point)?);
    }

    // Each edge, and whether the record runs it against this model's direction.
    let mut edge_index: HashMap<EdgeKey, (u32, bool)> = HashMap::new();
    let mut edge_midpoints: Vec<(EdgeKey, [f64; 3])> = Vec::new();
    for (n, (key, edge)) in body.edges.iter().enumerate() {
        let curve = body.curves.get(edge.curve).ok_or("an edge without its curve")?;
        let (mut t0, mut t1) = (edge.start_parameter, edge.end_parameter);
        let (mut start, mut end) = (edge.start, edge.end);
        let mut against = false;
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
            std::mem::swap(&mut start, &mut end);
            against = true;
        }
        let (record, interval) =
            record_curve(curve, [t0, t1]).ok_or("an edge curve the record cannot hold")?;
        let ends = [
            *vertex_index.get(&start).ok_or("an edge without its start")?,
            *vertex_index.get(&end).ok_or("an edge without its end")?,
        ];
        // The assembler takes the edge as given unless its vertices sit the other way round.
        let (index, as_given) = asm.add_edge(n as u64, record, interval, ends)?;
        edge_index.insert(key, (index, against ^ !as_given));
        edge_midpoints.push((key, curve.point_at(0.5 * (t0 + t1))));
    }

    // Faces shell by shell, so a shell's faces are listed together.
    let mut shells: Vec<Vec<(u32, bool)>> = Vec::new();
    let mut origin_face: Vec<FaceKey> = Vec::new();
    let mut closed_shells = 0usize;
    let mut open_shells = 0usize;
    let mut voids = false;
    for &lump_key in &body.roots {
        let lump = body.lumps.get(lump_key).ok_or("a root without its lump")?;
        for (s, &shell_key) in lump.shells.iter().enumerate() {
            let shell = body.shells.get(shell_key).ok_or("a lump without its shell")?;
            let mut faces = Vec::with_capacity(shell.faces.len());
            for &face_key in &shell.faces {
                let face = body.faces.get(face_key).ok_or("a shell without its face")?;
                let surface = body.surfaces.get(face.surface).ok_or("a face without its surface")?;
                let mut bounds = Vec::with_capacity(face.loops.len());
                for &loop_key in &face.loops {
                    let lp = body.loops.get(loop_key).ok_or("a face without its loop")?;
                    let mut uses = Vec::with_capacity(lp.coedges.len());
                    for &coedge_key in &lp.coedges {
                        let coedge =
                            body.coedges.get(coedge_key).ok_or("a loop without its coedge")?;
                        let &(index, against) =
                            edge_index.get(&coedge.edge).ok_or("a coedge without its edge")?;
                        uses.push((index, coedge.forward ^ against));
                    }
                    if !face.forward {
                        // About the surface's normal, the loop runs the other way.
                        uses.reverse();
                        uses.iter_mut().for_each(|u| u.1 = !u.1);
                    }
                    bounds.push(Bound::Edges(uses));
                }
                let origin = origin_face.len() as u64;
                origin_face.push(face_key);
                let index = asm.add_face(origin, record_surface(surface), bounds, None, face.forward)?;
                // The shell uses the face on its side: the record's side of it.
                faces.push((index, face.forward));
            }
            if shell_is_closed(body, shell) {
                closed_shells += 1;
                voids |= s > 0;
            } else {
                open_shells += 1;
            }
            shells.push(faces);
        }
    }
    let kind = if open_shells == 0 && closed_shells == 1 {
        Kind::Solid
    } else if open_shells == 0 && body.roots.len() == 1 && voids {
        Kind::Voids
    } else if voids {
        return Err("several solids with voids in one body".into());
    } else {
        Kind::Model
    };
    let solid = asm.finish(0, String::new(), shells, kind)?;

    let faces = solid
        .face_origin
        .iter()
        .map(|&o| origin_face.get(o as usize).copied().ok_or("a face of no origin"))
        .collect::<Result<Vec<_>, _>>()?;
    // Edges by where they run: the assembler may have merged, turned or added (seams) them.
    let edges = solid
        .brep
        .edges
        .iter()
        .map(|e| {
            let mid = record_edge_point(e, 0.5)?;
            edge_midpoints
                .iter()
                .map(|(k, p)| (*k, Vec3::from(*p).distance(Vec3::from(mid))))
                .filter(|(_, d)| *d <= 1e3 * tolerance)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, _)| k)
        })
        .collect();
    let vertices = solid
        .brep
        .vertices
        .iter()
        .map(|v| {
            body.vertices
                .iter()
                .map(|(k, x)| (k, Vec3::from(x.point).distance(Vec3::from(v.point))))
                .filter(|(_, d)| *d <= 1e3 * tolerance)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, _)| k)
        })
        .collect();
    Ok((
        solid.brep,
        Correspondence {
            faces,
            edges,
            vertices,
        },
    ))
}

/// Whether every edge of `shell` is used twice in it, or once twice over (a seam).
fn shell_is_closed(body: &Body, shell: &Shell) -> bool {
    let mut uses: HashMap<EdgeKey, usize> = HashMap::new();
    for &face_key in &shell.faces {
        let Some(face) = body.faces.get(face_key) else {
            return false;
        };
        for &loop_key in &face.loops {
            let Some(lp) = body.loops.get(loop_key) else {
                return false;
            };
            for &coedge_key in &lp.coedges {
                if let Some(c) = body.coedges.get(coedge_key) {
                    *uses.entry(c.edge).or_default() += 1;
                }
            }
        }
    }
    !uses.is_empty() && uses.values().all(|n| n % 2 == 0)
}

/// A record curve in this model's terms over `interval`, and its interval there.
pub(crate) fn model_curve(curve: &r::Curve, interval: [f64; 2]) -> Option<(Curve3, [f64; 2])> {
    Some(match curve {
        r::Curve::Line { start, end } => (
            Curve3::Line(Line3 {
                origin: *start,
                direction: (Vec3::from(*end) - Vec3::from(*start)).to_array(),
            }),
            interval,
        ),
        r::Curve::Circle { frame, radius } => (
            Curve3::Circle(Circle3 {
                plane: plane_of(frame)?,
                radius: *radius,
            }),
            interval,
        ),
        r::Curve::Ellipse { frame, radii } => (
            Curve3::Ellipse(Ellipse3 {
                plane: plane_of(frame)?,
                major_radius: radii[0],
                minor_radius: radii[1],
            }),
            interval,
        ),
        r::Curve::Polyline { points } => {
            let n = points.len();
            let mut knots = vec![0.0];
            knots.extend((0..n).map(|i| i as f64));
            knots.push((n - 1) as f64);
            (
                Curve3::Nurbs(NurbsCurve3::new(1, points.clone(), knots, None)?),
                interval,
            )
        }
        r::Curve::Spline { knots, points } => {
            let degree = knots.len().checked_sub(points.len() + 1)?;
            (
                Curve3::Nurbs(periodic(
                    NurbsCurve3::new(degree, points.clone(), knots.clone(), None)?,
                    knots,
                    points.len(),
                )),
                interval,
            )
        }
        r::Curve::RationalSpline { knots, homogeneous } => {
            let degree = knots.len().checked_sub(homogeneous.len() + 1)?;
            let (points, weights): (Vec<[f64; 3]>, Vec<f64>) = homogeneous
                .iter()
                .map(|h| ([h[0] / h[3], h[1] / h[3], h[2] / h[3]], h[3]))
                .unzip();
            let n = points.len();
            (
                Curve3::Nurbs(periodic(
                    NurbsCurve3::new(degree, points, knots.clone(), Some(weights))?,
                    knots,
                    n,
                )),
                interval,
            )
        }
        r::Curve::SurfaceIntersection { leader, .. } => return model_curve(leader, interval),
    })
}

fn periodic(curve: NurbsCurve3, knots: &[f64], count: usize) -> NurbsCurve3 {
    let closed = matches!(r::knot_domain(knots, count), r::Natural::Periodic);
    curve.with_periodicity(closed)
}

pub(crate) fn spline_surface(s: &r::Surface) -> Option<NurbsSurface3> {
    match s {
        r::Surface::Spline {
            u_knots,
            v_knots,
            points,
        } => {
            let ud = u_knots.len().checked_sub(points.len() + 1)?;
            let vd = v_knots.len().checked_sub(points.first()?.len() + 1)?;
            NurbsSurface3::new(ud, vd, points.clone(), u_knots.clone(), v_knots.clone(), None)
        }
        r::Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => {
            let ud = u_knots.len().checked_sub(homogeneous.len() + 1)?;
            let vd = v_knots.len().checked_sub(homogeneous.first()?.len() + 1)?;
            let points = homogeneous
                .iter()
                .map(|row| row.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).collect())
                .collect();
            let weights = homogeneous
                .iter()
                .map(|row| row.iter().map(|h| h[3]).collect())
                .collect();
            NurbsSurface3::new(ud, vd, points, u_knots.clone(), v_knots.clone(), Some(weights))
        }
        _ => None,
    }
}

/// A record face's surface in this model's terms: its label where this model has the kind, its
/// NURBS form otherwise.
fn model_surface(face: &r::Face) -> Option<Surface> {
    let net = || face.nurbs.as_ref().and_then(spline_surface).map(Surface::Nurbs);
    Some(match &face.surface {
        r::Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } => {
            let x = Vec3::from(*u_axis);
            Surface::Plane(Plane::orthonormal(*origin, x.to_array(), x.cross(Vec3::from(*v_axis)).to_array())?)
        }
        r::Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let o = Vec3::from(*origin);
            let x = Vec3::from(*u_point) - o;
            let y = Vec3::from(*v_point) - o;
            Surface::Plane(Plane::orthonormal(*origin, x.to_array(), x.cross(y).to_array())?)
        }
        r::Surface::Cylinder { frame, radius } => Surface::Cylinder(Cylinder {
            base: plane_of(frame)?,
            radius: *radius,
        }),
        r::Surface::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => Surface::Cone(Cone {
            base: plane_of(frame)?,
            radius: *radius_at_origin,
            half_angle: -*half_angle,
        }),
        r::Surface::Sphere { frame, radius } => Surface::Sphere(Sphere {
            frame: plane_of(frame)?,
            radius: *radius,
        }),
        r::Surface::Torus {
            frame,
            major_radius,
            minor_radius,
            ..
        } => Surface::Torus(Torus {
            frame: plane_of(frame)?,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        }),
        r::Surface::Spline { .. } | r::Surface::RationalSpline { .. } => {
            exact_or_spline(spline_surface(&face.surface)?)
        }
        r::Surface::BallPatch { .. }
        | r::Surface::PlacedRevolution { .. }
        | r::Surface::Blend { .. } => match net()? {
            Surface::Nurbs(n) => exact_or_spline(n),
            other => other,
        },
    })
}

/// A spline that is exactly a plane, cylinder, cone, sphere or torus (as Solidcore makes offsets
/// and rounds of analytic faces), as that surface; otherwise the spline.
fn exact_or_spline(spline: NurbsSurface3) -> Surface {
    match crate::analytic_surface(&spline) {
        Some((surface, _)) => surface,
        None => Surface::Nurbs(spline),
    }
}

/// The point at fraction `s` of a record edge's interval, on the curve its interval is on.
fn record_edge_point(edge: &r::Edge, s: f64) -> Option<[f64; 3]> {
    let [t0, t1] = edge.interval;
    let t = t0 + s * (t1 - t0);
    let (curve, _) = model_curve(edge.nurbs.as_ref().unwrap_or(&edge.curve), edge.interval)?;
    Some(curve.point_at(t))
}

/// A record surface label in this model's terms, where this model has the kind.
fn label_surface(surface: &r::Surface) -> Option<Surface> {
    let face = r::Face {
        surface: surface.clone(),
        nurbs: None,
        domain: [[0.0, 1.0], [0.0, 1.0]],
        loops: Vec::new(),
    };
    model_surface(&face)
}

/// The interval on `curve` (a line, circle or ellipse of this model) from `start` through
/// `middle` to `end`; a closed edge (`closed`) makes a whole turn.
fn interval_through(curve: &Curve3, start: [f64; 3], middle: [f64; 3], end: [f64; 3], closed: bool) -> Option<(Curve3, [f64; 2])> {
    Some(match curve {
        Curve3::Line(_) => (
            Curve3::Line(Line3 {
                origin: start,
                direction: (Vec3::from(end) - Vec3::from(start)).to_array(),
            }),
            [0.0, 1.0],
        ),
        Curve3::Circle(Circle3 { plane, .. }) | Curve3::Ellipse(Ellipse3 { plane, .. }) => {
            let (a, b) = match curve {
                Curve3::Ellipse(e) => (e.major_radius, e.minor_radius),
                Curve3::Circle(c) => (c.radius, c.radius),
                _ => return None,
            };
            let angle = |p: [f64; 3]| -> Option<f64> {
                let uv = plane.project(p)?;
                Some((uv[1] / b).atan2(uv[0] / a))
            };
            let t0 = angle(start)?;
            let mut tm = angle(middle)?;
            while tm <= t0 {
                tm += TAU;
            }
            let mut t1 = angle(end)?;
            while t1 < tm {
                t1 += TAU;
            }
            if closed {
                t1 = t0 + TAU;
            }
            (curve.clone(), [t0, t1])
        }
        Curve3::Nurbs(n) => {
            let (t0, t1) = (n.parameter_at(start), n.parameter_at(end));
            (curve.clone(), [t0.min(t1), t0.max(t1)])
        }
        Curve3::PlanarSpline { .. } => return None,
    })
}

/// A record edge in this model's terms: its curve as labelled, and the interval on it from its
/// start vertex to its end vertex. An edge on a NURBS form keeps its label (a line, a circle,
/// an ellipse) with the interval read off that label; a traced intersection of two surfaces this
/// model meets in closed form is that curve; any other is the spline through its trace.
fn model_edge(edge: &r::Edge, vertices: &[r::Vertex]) -> Option<(Curve3, [f64; 2])> {
    let start = vertices.get(edge.vertices[0] as usize)?.point;
    let end = vertices.get(edge.vertices[1] as usize)?.point;
    let closed = edge.vertices[0] == edge.vertices[1];
    if let r::Curve::SurfaceIntersection { surfaces, leader, .. } = &edge.curve {
        let middle = record_edge_point(edge, 0.5)?;
        if let (Some(a), Some(b)) = (label_surface(&surfaces[0]), label_surface(&surfaces[1])) {
            if let crate::Meeting::Curves(curves) = crate::intersect_surfaces(&a, &b, 1e-7) {
                // The closed-form curve the trace runs along.
                let near = |c: &Curve3| -> f64 {
                    [start, middle, end]
                        .iter()
                        .map(|p| {
                            let probe = match c {
                                Curve3::Line(l) => {
                                    let d = Vec3::from(l.direction);
                                    let t = (Vec3::from(*p) - Vec3::from(l.origin)).dot(d) / d.dot(d).max(1e-300);
                                    c.point_at(t)
                                }
                                Curve3::Circle(Circle3 { plane, .. }) | Curve3::Ellipse(Ellipse3 { plane, .. }) => {
                                    let uv = plane.project(*p).unwrap_or([0.0, 0.0]);
                                    let (ra, rb) = match c {
                                        Curve3::Ellipse(e) => (e.major_radius, e.minor_radius),
                                        Curve3::Circle(k) => (k.radius, k.radius),
                                        _ => (1.0, 1.0),
                                    };
                                    c.point_at((uv[1] / rb).atan2(uv[0] / ra))
                                }
                                Curve3::Nurbs(n) => n.point_at_knot(n.parameter_at(*p)),
                                Curve3::PlanarSpline { .. } => [f64::INFINITY; 3],
                            };
                            Vec3::from(probe).distance(Vec3::from(*p))
                        })
                        .fold(0.0, f64::max)
                };
                if let Some(best) = curves.iter().min_by(|x, y| near(x).total_cmp(&near(y))) {
                    if near(best) <= 1e-4 * (1.0 + Vec3::from(start).length()) {
                        if let Some(found) = interval_through(best, start, middle, end, closed) {
                            return Some(found);
                        }
                    }
                }
            }
        }
        // No closed form: the cubic through the trace.
        let [t0, t1] = edge.interval;
        let (trace, _) = model_curve(leader, edge.interval)?;
        let points: Vec<[f64; 3]> = (0..=64).map(|i| trace.point_at(t0 + (t1 - t0) * i as f64 / 64.0)).collect();
        let mut points = points;
        points[0] = start;
        if let Some(last) = points.last_mut() {
            *last = end;
        }
        let fitted = NurbsCurve3::interpolate_fit(&points, None, None, ocs_geom::space::Parameterization::Chord)?;
        let (a, b) = fitted.domain();
        return Some((Curve3::Nurbs(fitted), [a, b]));
    }
    let Some(_) = &edge.nurbs else {
        let (curve, interval) = model_curve(&edge.curve, edge.interval)?;
        // A spline that is exactly a line or a circle (Solidcore makes its conics so) is one.
        if let Curve3::Nurbs(n) = &curve {
            if let Some(exact) = crate::analytic_curve(n) {
                let middle = curve.point_at(0.5 * (interval[0] + interval[1]));
                if let Some(found) = interval_through(&exact, start, middle, end, closed) {
                    return Some(found);
                }
            }
        }
        return Some((curve, interval));
    };
    let (curve, _) = model_curve(&edge.curve, edge.interval)?;
    let middle = record_edge_point(edge, 0.5)?;
    match curve {
        Curve3::Line(_) | Curve3::Circle(_) | Curve3::Ellipse(_) => interval_through(&curve, start, middle, end, closed),
        _ => model_curve(edge.nurbs.as_ref()?, edge.interval),
    }
}

/// A record face's (u, v) read in this model's surface parameters: through the face's NURBS
/// form and back onto this model's surface where the face is on a form, as given otherwise.
struct FaceParams<'a> {
    label: &'a r::Surface,
    form: Option<NurbsSurface3>,
    surface: Surface,
    /// Along each periodic direction read through the form: the form parameter and the angle
    /// it reaches, unwrapped across the face's domain.
    tables: [Option<Vec<(f64, f64)>>; 2],
}

/// `point` on `surface` in its parameters, angles in (-π, π].
fn surface_parameters(surface: &Surface, point: [f64; 3]) -> Option<[f64; 2]> {
    let local = |plane: &Plane| -> Option<(f64, f64, f64)> {
        let d = Vec3::from(point) - Vec3::from(plane.origin);
        let n = Vec3::from(plane.normal()?);
        Some((d.dot(Vec3::from(plane.x_axis)), d.dot(Vec3::from(plane.y_axis)), d.dot(n)))
    };
    Some(match surface {
        Surface::Plane(p) => p.project(point)?,
        Surface::Cylinder(c) => {
            let (x, y, z) = local(&c.base)?;
            [y.atan2(x), z]
        }
        Surface::Cone(c) => {
            let (x, y, z) = local(&c.base)?;
            [y.atan2(x), z]
        }
        Surface::Sphere(s) => {
            let (x, y, z) = local(&s.frame)?;
            [y.atan2(x), (z / s.radius).clamp(-1.0, 1.0).asin()]
        }
        Surface::Torus(t) => {
            let (x, y, z) = local(&t.frame)?;
            [y.atan2(x), z.atan2(x.hypot(y) - t.major_radius)]
        }
        Surface::Nurbs(_) | Surface::Offset(_) => return None,
    })
}

fn periodic_directions(surface: &Surface) -> [bool; 2] {
    match surface {
        Surface::Cylinder(_) | Surface::Cone(_) | Surface::Sphere(_) => [true, false],
        Surface::Torus(_) => [true, true],
        _ => [false, false],
    }
}

fn unwrap(values: &mut [f64]) {
    for i in 1..values.len() {
        let step = values[i] - values[i - 1];
        values[i] -= TAU * (step / TAU).round();
    }
}

impl<'a> FaceParams<'a> {
    fn new(face: &'a r::Face, surface: &Surface) -> Self {
        let on_form = !matches!(surface, Surface::Nurbs(_) | Surface::Offset(_));
        // The spline the face's (u, v) are on, where this model keeps another surface: the
        // face's NURBS form, or its spline label recognised as an analytic surface.
        let form = if on_form {
            face.nurbs.as_ref().or(match &face.surface {
                r::Surface::Spline { .. } | r::Surface::RationalSpline { .. } => Some(&face.surface),
                _ => None,
            })
            .and_then(spline_surface)
        } else {
            None
        };
        let mut out = Self {
            label: &face.surface,
            form,
            surface: surface.clone(),
            tables: [None, None],
        };
        if let Some(form) = &out.form {
            let periodic = periodic_directions(surface);
            let [[u0, u1], [v0, v1]] = face.domain;
            for d in 0..2 {
                if !periodic[d] {
                    continue;
                }
                let samples: Vec<f64> = (0..=32)
                    .map(|i| if d == 0 { u0 + (u1 - u0) * i as f64 / 32.0 } else { v0 + (v1 - v0) * i as f64 / 32.0 })
                    .collect();
                let mut angles: Vec<f64> = samples
                    .iter()
                    .filter_map(|s| {
                        let uv = if d == 0 { [*s, 0.5 * (v0 + v1)] } else { [0.5 * (u0 + u1), *s] };
                        surface_parameters(surface, form.point_at_knot(uv[0], uv[1])).map(|p| p[d])
                    })
                    .collect();
                if angles.len() == samples.len() {
                    unwrap(&mut angles);
                    out.tables[d] = Some(samples.into_iter().zip(angles).collect());
                }
            }
        }
        out
    }

    /// Whether the face's NURBS form, where it is on one, has its natural normal where this
    /// model's surface has its own; `None` where it cannot say.
    fn form_agrees_with_surface(&self) -> Option<bool> {
        let form = self.form.as_ref()?;
        let ((u0, u1), (v0, v1)) = form.domain();
        // A point inside, off any pole or apex the middle might sit on.
        let (u, v) = (u0 + 0.37 * (u1 - u0), v0 + 0.41 * (v1 - v0));
        let normal = Vec3::from(form.normal_at_knot(u, v)?);
        let uv = surface_parameters(&self.surface, form.point_at_knot(u, v))?;
        let own = Vec3::from(self.surface.normal_at(uv[0], uv[1])?);
        Some(normal.dot(own) >= 0.0)
    }

    /// Record `(u, v)` as this model's, angles as their principal values.
    fn principal(&self, uv: [f64; 2]) -> Option<[f64; 2]> {
        if let Some(form) = &self.form {
            return surface_parameters(&self.surface, form.point_at_knot(uv[0], uv[1]));
        }
        Some(match self.label {
            r::Surface::Plane { origin, u_axis, v_axis } => {
                let p = Vec3::from(*origin) + Vec3::from(*u_axis) * uv[0] + Vec3::from(*v_axis) * uv[1];
                surface_parameters(&self.surface, p.to_array())?
            }
            r::Surface::PlaneThrough { origin, u_point, v_point } => {
                let o = Vec3::from(*origin);
                let p = o + (Vec3::from(*u_point) - o) * uv[0] + (Vec3::from(*v_point) - o) * uv[1];
                surface_parameters(&self.surface, p.to_array())?
            }
            r::Surface::Torus { tube_start, .. } => [uv[0], uv[1] + tube_start],
            _ => uv,
        })
    }

    /// The angle the form parameter `s` reaches along direction `d`, from the face's table.
    fn expected(&self, d: usize, s: f64) -> Option<f64> {
        let table = self.tables[d].as_ref()?;
        let i = table.partition_point(|(x, _)| *x < s).clamp(1, table.len() - 1);
        let ((x0, a0), (x1, a1)) = (table[i - 1], table[i]);
        let f = if (x1 - x0).abs() > 0.0 { (s - x0) / (x1 - x0) } else { 0.0 };
        Some(a0 + f * (a1 - a0))
    }

    /// A use's pcurve, sampled over its edge's interval, as a polyline in this model's surface
    /// parameters: each periodic angle continuous along it and on the turn the face's form puts
    /// it (so a seam's two uses land a turn apart).
    fn pcurve(&self, pcurve: &r::Curve2, interval: [f64; 2]) -> Option<ocs_geom::geom2d::Curve> {
        use ocs_geom::geom2d::{Curve, NurbsCurve, Polyline, PolylineVertex};
        let curve = match pcurve {
            r::Curve2::Spline { knots, points } => {
                NurbsCurve::new(knots.len().checked_sub(points.len() + 1)?, points.clone(), knots.clone(), None)?
            }
            r::Curve2::RationalSpline { knots, homogeneous } => NurbsCurve::new(
                knots.len().checked_sub(homogeneous.len() + 1)?,
                homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
                knots.clone(),
                Some(homogeneous.iter().map(|h| h[2]).collect()),
            )?,
        };
        // Exact where the record's (u, v) are this model's: the same spline, its points read
        // across.
        if self.form.is_none() && matches!(self.surface, Surface::Nurbs(_)) {
            return Some(Curve::Nurbs(curve));
        }
        const N: usize = 64;
        let record: Vec<[f64; 2]> = (0..=N)
            .map(|i| curve.point_at_knot(interval[0] + (interval[1] - interval[0]) * i as f64 / N as f64))
            .collect();
        let mut points: Vec<[f64; 2]> = record.iter().map(|uv| self.principal(*uv)).collect::<Option<_>>()?;
        if self.form.is_some() {
            if let Surface::Sphere(_) = self.surface {
                // At a pole the longitude is the meridian's that reaches it.
                let pole = |p: &[f64; 2]| (p[1].abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-7;
                for i in 0..points.len() {
                    if pole(&points[i]) {
                        let near = (1..points.len())
                            .flat_map(|k| [i.checked_sub(k), Some(i + k)])
                            .flatten()
                            .find(|&j| j < points.len() && !pole(&points[j]));
                        if let Some(j) = near {
                            points[i][0] = points[j][0];
                        }
                    }
                }
            }
            for d in 0..2 {
                if self.tables[d].is_none() {
                    continue;
                }
                let mut values: Vec<f64> = points.iter().map(|p| p[d]).collect();
                unwrap(&mut values);
                let n = values.len() as f64;
                let mean: f64 = values.iter().sum::<f64>() / n;
                let want: f64 = record.iter().filter_map(|uv| self.expected(d, uv[d])).sum::<f64>() / n;
                let shift = TAU * ((want - mean) / TAU).round();
                for (p, v) in points.iter_mut().zip(values) {
                    p[d] = v + shift;
                }
            }
        }
        let mut polyline = Polyline::new();
        polyline.vertices = points
            .into_iter()
            .map(|position| PolylineVertex { position, bulge: 0.0 })
            .collect();
        Some(Curve::Polyline(polyline))
    }
}

fn reversed_pcurve(curve: ocs_geom::geom2d::Curve) -> ocs_geom::geom2d::Curve {
    use ocs_geom::geom2d::Curve;
    match curve {
        Curve::Nurbs(n) => Curve::Nurbs(n.reversed()),
        Curve::Polyline(mut p) => {
            p.vertices.reverse();
            Curve::Polyline(p)
        }
        other => other,
    }
}

/// A record as a body of this model, and which entity each record entity became.
///
/// # Errors
/// A record entity this model has no form for, named.
pub fn from_record(record: &r::Brep) -> Result<(Body, Correspondence), String> {
    let mut body = Body::new();
    let vertices: Vec<VertexKey> = record
        .vertices
        .iter()
        .map(|v| {
            body.vertices.insert(Vertex {
                point: v.point,
                provenance: Provenance::Synthesized,
            })
        })
        .collect();
    let mut edges = Vec::with_capacity(record.edges.len());
    for e in &record.edges {
        let (curve, interval) =
            model_edge(e, &record.vertices).ok_or("an edge curve with no form here")?;
        let curve = body.curves.insert(curve);
        let start = *vertices.get(e.vertices[0] as usize).ok_or("an edge's start")?;
        let end = *vertices.get(e.vertices[1] as usize).ok_or("an edge's end")?;
        edges.push(body.edges.insert(Edge {
            curve,
            start_parameter: interval[0],
            end_parameter: interval[1],
            start,
            end,
            coedges: Vec::new(),
            provenance: Provenance::Synthesized,
        }));
    }

    // Each face's side, from the shell that uses it.
    let mut side = vec![true; record.faces.len()];
    for s in &record.shells {
        if let r::Shell::Faces { faces, .. } = s {
            for f in faces {
                if let Some(x) = side.get_mut(f.face as usize) {
                    *x = f.forward;
                }
            }
        }
    }

    // Faces are made before the shells that own them: they wait on a placeholder shell, in a
    // placeholder lump, both taken away once every face has its own.
    let placeholder_lump = body.lumps.insert(Lump {
        shells: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let placeholder = body.shells.insert(Shell {
        faces: Vec::new(),
        owner: placeholder_lump,
        provenance: Provenance::Synthesized,
    });
    let mut faces = Vec::with_capacity(record.faces.len());
    for (fi, f) in record.faces.iter().enumerate() {
        let surface = model_surface(f).ok_or("a face surface with no form here")?;
        let params = FaceParams::new(f, &surface);
        // The record's side is about the natural normal of the surface the face's (u, v) are on:
        // its NURBS form where it has one. This model's is about the surface it keeps.
        let agrees = params.form_agrees_with_surface().unwrap_or(true);
        let surface = body.surfaces.insert(surface);
        let forward = side[fi];
        let face = body.faces.insert(Face {
            surface,
            forward: forward == agrees,
            loops: Vec::new(),
            owner: placeholder,
            provenance: Provenance::Synthesized,
        });
        for l in &f.loops {
            let r::Loop::Edges { edges: uses } = l else {
                continue;
            };
            let lp = body.loops.insert(Loop {
                coedges: Vec::new(),
                owner: face,
                provenance: Provenance::Synthesized,
            });
            // A use's pcurve is carried where this model cannot find it from the edge alone: a
            // seam's two uses on one face (which side is which), and every use on a spline,
            // whose (u, v) are the record's. Elsewhere this model projects its own, exactly.
            let seam = |edge: u32| uses.iter().filter(|u| u.edge == edge).count() > 1;
            let spline = matches!(model_surface(f), Some(Surface::Nurbs(_)));
            let mut ordered: Vec<(u32, bool, Option<ocs_geom::geom2d::Curve>)> = uses
                .iter()
                .map(|u| {
                    let interval = record.edges.get(u.edge as usize).map(|e| e.interval);
                    // Along the edge for now; turned to run along its use below.
                    let pcurve = if seam(u.edge) || spline {
                        interval.and_then(|i| params.pcurve(&u.pcurve, i))
                    } else {
                        None
                    };
                    (u.edge, u.forward, pcurve)
                })
                .collect();
            if !forward {
                ordered.reverse();
                ordered.iter_mut().for_each(|u| u.1 = !u.1);
            }
            let mut coedges: Vec<CoedgeKey> = Vec::with_capacity(ordered.len());
            for (edge, along, pcurve) in ordered {
                let edge = *edges.get(edge as usize).ok_or("a use's edge")?;
                // This model runs a pcurve along its use.
                let pcurve = pcurve.map(|c| if along { c } else { reversed_pcurve(c) });
                let c = body.coedges.insert(Coedge {
                    edge,
                    forward: along,
                    pcurve,
                    owner: lp,
                    provenance: Provenance::Synthesized,
                });
                if let Some(e) = body.edges.get_mut(edge) {
                    e.coedges.push(c);
                }
                coedges.push(c);
            }
            if let Some(x) = body.loops.get_mut(lp) {
                x.coedges = coedges;
            }
            if let Some(x) = body.faces.get_mut(face) {
                x.loops.push(lp);
            }
        }
        faces.push(face);
    }

    // Shells and the lumps they bound: a bounded material region is a lump of its shells; an
    // open shell, which no region is bounded by, is a lump of its own.
    let mut shell_keys: HashMap<u32, crate::ShellKey> = HashMap::new();
    let mut shell_of = |body: &mut Body, s: u32, lump: crate::LumpKey| -> Result<crate::ShellKey, String> {
        if let Some(k) = shell_keys.get(&s) {
            return Ok(*k);
        }
        let r::Shell::Faces { faces: uses, .. } =
            record.shells.get(s as usize).ok_or("a region's shell")?
        else {
            return Err("a wire or vertex shell".into());
        };
        let k = body.shells.insert(Shell {
            faces: Vec::new(),
            owner: lump,
            provenance: Provenance::Synthesized,
        });
        let list: Vec<FaceKey> = uses
            .iter()
            .map(|u| faces.get(u.face as usize).copied().ok_or("a shell's face"))
            .collect::<Result<_, _>>()?;
        for &f in &list {
            if let Some(x) = body.faces.get_mut(f) {
                x.owner = k;
            }
        }
        if let Some(x) = body.shells.get_mut(k) {
            x.faces = list;
        }
        shell_keys.insert(s, k);
        Ok(k)
    };
    let mut listed_by_bounded: Vec<bool> = vec![false; record.shells.len()];
    for region in record.regions.iter().skip(1) {
        for &s in &region.shells {
            if let Some(x) = listed_by_bounded.get_mut(s as usize) {
                *x = true;
            }
        }
    }
    for region in record.regions.iter().skip(1).filter(|r| !r.void) {
        let lump = body.lumps.insert(Lump {
            shells: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        let mut shells = Vec::with_capacity(region.shells.len());
        for &s in &region.shells {
            if matches!(record.shells.get(s as usize), Some(r::Shell::Faces { .. })) {
                shells.push(shell_of(&mut body, s, lump)?);
            }
        }
        if let Some(x) = body.lumps.get_mut(lump) {
            x.shells = shells;
        }
        body.roots.push(lump);
    }
    if let Some(infinite) = record.regions.first() {
        for &s in &infinite.shells {
            let open = !listed_by_bounded.get(s as usize).copied().unwrap_or(false);
            if open && matches!(record.shells.get(s as usize), Some(r::Shell::Faces { .. })) {
                let lump = body.lumps.insert(Lump {
                    shells: Vec::new(),
                    provenance: Provenance::Synthesized,
                });
                let k = shell_of(&mut body, s, lump)?;
                if let Some(x) = body.lumps.get_mut(lump) {
                    x.shells = vec![k];
                }
                body.roots.push(lump);
            }
        }
    }

    body.shells.remove(placeholder);
    body.lumps.remove(placeholder_lump);
    Ok((
        body,
        Correspondence {
            faces,
            edges: edges.into_iter().map(Some).collect(),
            vertices: vertices.into_iter().map(Some).collect(),
        },
    ))
}

/// `body` as a Solidcore body ready for its operations, and which entity each of the
/// operation's record indices names.
///
/// # Errors
/// As [`to_record`], or what Solidcore refuses of the record.
pub fn to_solidcore(
    body: &Body,
) -> Result<(solidcore_ops::Body, solidcore_ops::Order, Correspondence), String> {
    let (record, map) = to_record(body)?;
    let (b, order) = solidcore_ops::Body::from_record(&record, true)
        .map_err(|e| format!("solidcore refused the body: {e:?}"))?;
    Ok((b, order, map))
}

/// A Solidcore body as this model's.
///
/// # Errors
/// What Solidcore or [`from_record`] refuse.
pub fn from_solidcore(body: &solidcore_ops::Body) -> Result<(Body, Correspondence), String> {
    let (record, _) = body.to_record().map_err(|e| format!("solidcore record: {e:?}"))?;
    from_record(&record)
}

/// Runs `f` on Solidcore bodies made of `bodies` and brings its result back; `Ok(None)` from
/// `f` (nothing left, as an intersection of solids apart) is an empty body.
fn through<const N: usize>(
    bodies: [&Body; N],
    f: impl FnOnce([solidcore_ops::Body; N]) -> solidcore_ops::Result<Option<solidcore_ops::Body>>,
) -> Result<Body, String> {
    let mut made = Vec::with_capacity(N);
    for body in bodies {
        made.push(to_solidcore(body)?.0);
    }
    let made: [solidcore_ops::Body; N] =
        made.try_into().map_err(|_| "operand count".to_string())?;
    match f(made).map_err(|e| format!("solidcore: {e:?}"))? {
        Some(result) => Ok(from_solidcore(&result)?.0),
        None => Ok(Body::new()),
    }
}

/// `a` combined with `b` by Solidcore's Boolean.
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn boolean(a: &Body, b: &Body, op: solidcore_ops::BooleanOp) -> Result<Body, String> {
    through([a, b], |[a, b]| solidcore_ops::boolean::boolean(a, b, op))
}

/// The record indices of `edges`.
fn edge_indices(map: &Correspondence, edges: &[EdgeKey]) -> Result<Vec<usize>, String> {
    edges
        .iter()
        .map(|e| map.edge_index(*e).map(|i| i as usize).ok_or_else(|| "an edge the record lost".to_string()))
        .collect()
}

/// `edges` rounded at `radius` by Solidcore's rolling ball, their corners filled.
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn fillet(body: &Body, edges: &[EdgeKey], radius: f64) -> Result<Body, String> {
    use solidcore_ops::blend::{blend_ordered, Blend, BlendOptions, Corners};
    let (sc, order, map) = to_solidcore(body)?;
    let edges = edge_indices(&map, edges)?;
    blend_ordered(&sc, &order, Blend::Round, &edges, &[radius], &[], Corners::Filled, BlendOptions::default())
        .map_err(|e| format!("solidcore fillet: {e:?}"))?;
    Ok(from_solidcore(&sc)?.0)
}

/// `edges` chamfered `base` back along `base_face` and `other` along each edge's other face.
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn chamfer(body: &Body, edges: &[EdgeKey], base_face: FaceKey, base: f64, other: f64) -> Result<Body, String> {
    use solidcore_ops::blend::{chamfer_sides_ordered, ChamferSecond};
    let (sc, order, map) = to_solidcore(body)?;
    let edges = edge_indices(&map, edges)?;
    let face = map.face_index(base_face).ok_or("a face the record lost")? as usize;
    let along = vec![face; edges.len()];
    chamfer_sides_ordered(&sc, &order, &edges, &along, base, ChamferSecond::Distance(other))
        .map_err(|e| format!("solidcore chamfer: {e:?}"))?;
    Ok(from_solidcore(&sc)?.0)
}

/// The record indices of `faces`.
fn face_indices(map: &Correspondence, faces: &[FaceKey]) -> Result<Vec<usize>, String> {
    faces
        .iter()
        .map(|f| map.face_index(*f).map(|i| i as usize).ok_or_else(|| "a face the record lost".to_string()))
        .collect()
}

/// Runs an operation that edits a Solidcore body in place, and brings it back.
fn edited(
    body: &Body,
    f: impl FnOnce(&solidcore_ops::Body, &solidcore_ops::Order, &Correspondence) -> Result<(), String>,
) -> Result<Body, String> {
    let (sc, order, map) = to_solidcore(body)?;
    f(&sc, &order, &map)?;
    Ok(from_solidcore(&sc)?.0)
}

/// `body` hollowed into a wall `thickness` thick (negative: inward), opened at `open`.
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn shell(body: &Body, open: &[FaceKey], thickness: f64) -> Result<Body, String> {
    edited(body, |sc, order, map| {
        let open = face_indices(map, open)?;
        solidcore_ops::offset::shell(sc, order, &open, thickness).map_err(|e| format!("solidcore shell: {e:?}"))
    })
}

/// The sheet `body` thickened by `distance` along its faces' normals into a solid.
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn thicken(body: &Body, distance: f64) -> Result<Body, String> {
    edited(body, |sc, _, _| {
        solidcore_ops::offset::thicken(sc, distance, false).map_err(|e| format!("solidcore thicken: {e:?}"))
    })
}

/// `faces` moved `distance` along their outward normals, their neighbours following.
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn offset_faces(body: &Body, faces: &[FaceKey], distance: f64) -> Result<Body, String> {
    edited(body, |sc, order, map| {
        let faces = face_indices(map, faces)?;
        solidcore_ops::offset::offset_faces(sc, order, &faces, distance)
            .map_err(|e| format!("solidcore offset faces: {e:?}"))
    })
}

/// `faces` drafted `angle` about the neutral plane through `base` across `neutral`, pulled
/// along `pull`.
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn draft(
    body: &Body,
    faces: &[FaceKey],
    base: [f64; 3],
    neutral: [f64; 3],
    pull: [f64; 3],
    angle: f64,
) -> Result<Body, String> {
    edited(body, |sc, order, map| {
        let faces = face_indices(map, faces)?;
        solidcore_ops::offset::draft(sc, order, &faces, base, neutral, pull, angle)
            .map_err(|e| format!("solidcore draft: {e:?}"))
    })
}

/// Two Solidcore parts brought back.
fn parts(found: Option<(solidcore_ops::Body, solidcore_ops::Body)>) -> Result<Option<(Body, Body)>, String> {
    match found {
        Some((behind, front)) => Ok(Some((from_solidcore(&behind)?.0, from_solidcore(&front)?.0))),
        None => Ok(None),
    }
}

/// The solid `body` cut by `plane` into the part behind its normal and the part in front;
/// `reach` is how far the plane runs from its origin each way (beyond the body).
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn split_by_plane(body: &Body, plane: &Plane, reach: f64) -> Result<Option<(Body, Body)>, String> {
    let f = frame_of(plane);
    let tool = solidcore_ops::split::Tool {
        surface: r::Surface::Plane { origin: f.origin, u_axis: f.x_axis, v_axis: f.y_axis },
        domain: [[-reach, reach], [-reach, reach]],
    };
    let (sc, _, _) = to_solidcore(body)?;
    parts(solidcore_ops::split::split_body(&sc, &tool).map_err(|e| format!("solidcore split: {e:?}"))?)
}

/// The solid `body` cut by the sheet `cutter` into the part behind its face `face`'s normal and
/// the part in front.
///
/// # Errors
/// What the bridge or Solidcore refuse, in their words.
pub fn split_by_sheet(body: &Body, cutter: &Body, face: FaceKey) -> Result<Option<(Body, Body)>, String> {
    let (sc, _, _) = to_solidcore(body)?;
    let (sheet, sheet_order, map) = to_solidcore(cutter)?;
    let face = map.face_index(face).ok_or("a face the record lost")? as usize;
    parts(
        solidcore_ops::split::split_body_by_sheet(&sc, &sheet, &sheet_order, face)
            .map_err(|e| format!("solidcore split: {e:?}"))?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(body: &Body) -> f64 {
        crate::mass_properties(body).expect("mass").volume
    }

    fn round_trip(body: Body) {
        let v = volume(&body);
        let (record, map) = to_record(&body).expect("to record");
        assert_eq!(map.faces.len(), record.faces.len());
        let (sc, _) = solidcore_ops::Body::from_record(&record, true).expect("solidcore takes it");
        let sv = sc.volume(1e-6).expect("solidcore volume");
        assert!((sv - v).abs() <= 1e-4 * v.abs().max(1.0), "solidcore {sv} vs {v}");
        let (back, _) = from_solidcore(&sc).expect("back");
        let bv = volume(&back);
        assert!((bv - v).abs() <= 1e-3 * v.abs().max(1.0), "back {bv} vs {v}");
    }

    #[test]
    fn primitives_cross_both_ways_with_their_volume() {
        round_trip(crate::make::cuboid([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]).unwrap());
        round_trip(crate::make::cylinder([0.0, 0.0, 0.0], 2.0, 5.0).unwrap());
        round_trip(crate::make::sphere([1.0, 1.0, 1.0], 3.0).unwrap());
        round_trip(crate::make::cone([0.0, 0.0, 0.0], 2.0, 4.0).unwrap());
        round_trip(crate::make::torus([0.0, 0.0, 0.0], 5.0, 1.0).unwrap());
    }

    #[test]
    fn booleans_go_through_solidcore() {
        let block = crate::make::cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        let pin = crate::make::cylinder([5.0, 5.0, -1.0], 2.0, 12.0).unwrap();
        let hole = boolean(&block, &pin, solidcore_ops::BooleanOp::Subtract).expect("subtract");
        let want = 1000.0 - std::f64::consts::PI * 4.0 * 10.0;
        assert!((volume(&hole) - want).abs() < 1e-3 * want, "{} vs {want}", volume(&hole));
        let block_edges: Vec<EdgeKey> = block.edges.keys().take(1).collect();
        let rounded = fillet(&block, &block_edges, 1.0).expect("fillet");
        let want = 1000.0 - 10.0 * (1.0 - std::f64::consts::PI / 4.0);
        assert!((volume(&rounded) - want).abs() < 1e-3 * want, "{} vs {want}", volume(&rounded));
        let both = boolean(&block, &pin, solidcore_ops::BooleanOp::Union).expect("union");
        let want = 1000.0 + std::f64::consts::PI * 4.0 * 2.0;
        assert!((volume(&both) - want).abs() < 1e-3 * want, "{} vs {want}", volume(&both));
    }
}


