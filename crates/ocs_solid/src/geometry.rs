//! What reading and writing records needs of Solidcore: points on record curves, parameters on
//! record surfaces, a body moved, and the pcurves the assembler has no closed form for.

use std::f64::consts::TAU;

use solidcore::eval as k;
use solidcore_ops::pcurve::{pcurves_within, EdgeUse, TrimFace, Trims};
use solidcore_record::brep as r;

pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

pub(crate) fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = length(a);
    (l.is_finite() && l > 1e-300).then(|| scale(a, 1.0 / l))
}

/// The orthonormal frame at `origin` with its x axis along `x` (made square to `z`) and its
/// z axis along `z`.
pub fn frame(origin: [f64; 3], x: [f64; 3], z: [f64; 3]) -> Option<r::Frame> {
    // Square to the rounding Solidcore's planes and frames hold their axes to: a second pass of
    // each projection takes off what the first left.
    let z = unit(z)?;
    let mut x = unit(sub(x, scale(z, dot(x, z))))?;
    x = unit(sub(x, scale(z, dot(x, z))))?;
    let mut y = unit(cross(z, x))?;
    y = unit(sub(y, scale(x, dot(x, y))))?;
    Some(r::Frame {
        origin,
        x_axis: x,
        y_axis: y,
    })
}

fn point_of(v: solidcore::Vec3) -> [f64; 3] {
    let v = solidcore_convert::record_vec3(v);
    [v.x, v.y, v.z]
}

/// The point of `curve` at `t`.
pub fn point_on(curve: &r::Curve, t: f64) -> Option<[f64; 3]> {
    let curve = solidcore_convert::curve(curve).ok()?;
    k::CurveEval::new(&curve).ok()?.point(t).map(point_of)
}

/// `curve`'s points at `count + 1` even steps over `interval`.
pub fn points_on(curve: &r::Curve, interval: [f64; 2], count: usize) -> Option<Vec<[f64; 3]>> {
    let converted = solidcore_convert::curve(curve).ok()?;
    let eval = k::CurveEval::new(&converted).ok()?;
    (0..=count)
        .map(|i| {
            let t = interval[0] + (interval[1] - interval[0]) * i as f64 / count as f64;
            eval.point(t).map(point_of)
        })
        .collect()
}

/// The parameter of `curve` nearest `p`.
pub fn parameter_on(curve: &r::Curve, p: [f64; 3]) -> Option<f64> {
    let tolerance = 1e-9 * (1.0 + length(p));
    solidcore_ops::curve::closest_point(curve, p, tolerance)
        .ok()
        .flatten()
        .map(|found| found.parameter)
}

/// `brep` moved by Solidcore's affine `matrix` (rows the images of x, y and z, then the
/// translation), and for each of its faces the face of `brep` it was.
pub fn transformed(brep: &r::Brep, matrix: &[[f64; 4]; 4]) -> Option<(r::Brep, Vec<u32>)> {
    let (body, before) = solidcore_ops::Body::from_record(brep, true).ok()?;
    body.transform(matrix).ok()?;
    let (record, after) = body.to_record().ok()?;
    let faces = after
        .faces
        .iter()
        .map(|face| before.faces.iter().position(|f| f == face).map(|i| i as u32))
        .collect::<Option<Vec<_>>>()?;
    Some((record, faces))
}

/// Makes the pcurves the assembler has no closed form for: Solidcore's projection
/// ([`pcurves_within`]), and where that finds none, each edge traced onto the surface.
pub struct Traced;

impl Trims for Traced {
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
            Err(e) => traced(face).ok_or_else(|| format!("pcurves: {e:?}")),
        }
    }
}

/// A face's missing pcurves traced: each edge sampled along its interval, every sample read
/// back into the face's (u, v) near the one before, joined by straight spans on the edge's own
/// parameter.
fn traced(face: &TrimFace<'_>) -> Option<Vec<Vec<r::Curve2>>> {
    let surface = solidcore_convert::surface(face.nurbs.unwrap_or(face.surface)).ok()?;
    let eval = k::SurfaceEval::new(&surface).ok()?;
    let period = eval.domain().period;
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
            let points = points_on(e.nurbs.unwrap_or(e.curve), e.interval, N)?;
            // Read back as far off the surface as the edge may lie (a file's spline edge on a
            // spline face is often a fit of it), and no farther than its own span allows.
            let span = points.windows(2).map(|w| length(sub(w[1], w[0]))).sum::<f64>();
            let accept = (face.tolerance.max(e.tolerance) * 1e3).min(span * 1e-2).max(face.tolerance * 10.0);
            let mut near = None;
            let mut uv: Vec<[f64; 2]> = Vec::with_capacity(points.len());
            for p in points {
                let found = eval.project(solidcore_convert::vec3(p.into()), near, accept)?;
                near = Some(found);
                uv.push(found);
            }
            // Onto the turn the face's domain is on.
            for (d, per) in period.iter().enumerate() {
                let Some(per) = per.filter(|p| p.is_finite() && *p > 0.0) else {
                    continue;
                };
                let [lo, hi] = face.domain[d];
                let mid = 0.5 * (uv[0][d] + uv[uv.len() - 1][d]);
                let shift = per * ((0.5 * (lo + hi) - mid) / per).round();
                for p in &mut uv {
                    p[d] += shift;
                }
            }
            // The ends exactly the interval's, so the knots run in order.
            let ts: Vec<f64> = (0..=N)
                .map(|i| match i {
                    0 => t0,
                    _ if i == N => t1,
                    _ => t0 + (t1 - t0) * i as f64 / N as f64,
                })
                .collect();
            let mut knots = vec![t0];
            knots.extend(ts.iter().copied());
            knots.push(t1);
            pcs.push(r::Curve2::Spline { knots, points: uv });
        }
        out.push(pcs);
    }
    Some(out)
}

/// An angle brought within half a turn of `near`.
pub(crate) fn turn_near(angle: f64, near: f64) -> f64 {
    angle + TAU * ((near - angle) / TAU).round()
}

/// `brep` moved by the rigid affine `matrix` (rows the images of x, y and z, then the
/// translation), its own geometry moved: every parameter as it was, every frame and plane square
/// to the rounding Solidcore holds them to. `None` for a map that is not rigid and proper (a
/// scale, a reflection) or a surface kind not moved here.
pub fn rigidly_moved(brep: &r::Brep, matrix: &[[f64; 4]; 4]) -> Option<r::Brep> {
    let rows = [0, 1, 2].map(|i| [matrix[i][0], matrix[i][1], matrix[i][2]]);
    // A file's rotation is square to its own rounding only: one within a millionth of it is
    // taken as the rotation it stands for, made square.
    let rigid = (0..3).all(|i| (dot(rows[i], rows[i]) - 1.0).abs() < 1e-6)
        && (0..3).all(|i| (0..i).all(|j| dot(rows[i], rows[j]).abs() < 1e-6))
        && dot(cross(rows[0], rows[1]), rows[2]) > 0.0;
    if !rigid {
        return None;
    }
    let squared = frame([0.0; 3], rows[0], cross(rows[0], rows[1]))?;
    let rows = [squared.x_axis, squared.y_axis, cross(squared.x_axis, squared.y_axis)];
    let mover = Mover { rows, t: [matrix[3][0], matrix[3][1], matrix[3][2]] };
    let mut out = brep.clone();
    for v in &mut out.vertices {
        v.point = mover.point(v.point);
    }
    for e in &mut out.edges {
        e.curve = mover.curve(&e.curve)?;
        if let Some(n) = &e.nurbs {
            e.nurbs = Some(mover.curve(n)?);
        }
    }
    for f in &mut out.faces {
        f.surface = mover.surface(&f.surface)?;
        if let Some(n) = &f.nurbs {
            f.nurbs = Some(mover.surface(n)?);
        }
    }
    Some(out)
}

struct Mover {
    rows: [[f64; 3]; 3],
    t: [f64; 3],
}

impl Mover {
    fn vector(&self, v: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|k| v[0] * self.rows[0][k] + v[1] * self.rows[1][k] + v[2] * self.rows[2][k])
    }

    fn point(&self, p: [f64; 3]) -> [f64; 3] {
        add(self.vector(p), self.t)
    }

    fn homogeneous(&self, h: [f64; 4]) -> [f64; 4] {
        let p = self.vector([h[0], h[1], h[2]]);
        [p[0] + self.t[0] * h[3], p[1] + self.t[1] * h[3], p[2] + self.t[2] * h[3], h[3]]
    }

    fn frame(&self, f: &r::Frame) -> Option<r::Frame> {
        let x = self.vector(f.x_axis);
        let z = self.vector(cross(f.x_axis, f.y_axis));
        frame(self.point(f.origin), x, z)
    }

    fn curve(&self, c: &r::Curve) -> Option<r::Curve> {
        Some(match c {
            r::Curve::Line { start, end } => r::Curve::Line { start: self.point(*start), end: self.point(*end) },
            r::Curve::Circle { frame, radius } => r::Curve::Circle { frame: self.frame(frame)?, radius: *radius },
            r::Curve::Ellipse { frame, radii } => r::Curve::Ellipse { frame: self.frame(frame)?, radii: *radii },
            r::Curve::Polyline { points } => r::Curve::Polyline { points: points.iter().map(|p| self.point(*p)).collect() },
            r::Curve::Spline { knots, points } => r::Curve::Spline { knots: knots.clone(), points: points.iter().map(|p| self.point(*p)).collect() },
            r::Curve::RationalSpline { knots, homogeneous } => r::Curve::RationalSpline {
                knots: knots.clone(),
                homogeneous: homogeneous.iter().map(|h| self.homogeneous(*h)).collect(),
            },
            r::Curve::SurfaceIntersection { surfaces, leader, tolerance } => r::Curve::SurfaceIntersection {
                surfaces: [Box::new(self.surface(&surfaces[0])?), Box::new(self.surface(&surfaces[1])?)],
                leader: Box::new(self.curve(leader)?),
                tolerance: *tolerance,
            },
        })
    }

    fn surface(&self, s: &r::Surface) -> Option<r::Surface> {
        Some(match s {
            r::Surface::Plane { origin, u_axis, v_axis } => {
                let f = frame(self.point(*origin), self.vector(*u_axis), self.vector(cross(*u_axis, *v_axis)))?;
                // The axes' own lengths kept, so the plane's parameters are as they were.
                r::Surface::Plane { origin: f.origin, u_axis: scale(f.x_axis, length(*u_axis)), v_axis: scale(f.y_axis, length(*v_axis)) }
            }
            r::Surface::PlaneThrough { origin, u_point, v_point } => r::Surface::PlaneThrough {
                origin: self.point(*origin),
                u_point: self.point(*u_point),
                v_point: self.point(*v_point),
            },
            r::Surface::Cylinder { frame, radius } => r::Surface::Cylinder { frame: self.frame(frame)?, radius: *radius },
            r::Surface::Cone { frame, radius_at_origin, half_angle } => r::Surface::Cone {
                frame: self.frame(frame)?,
                radius_at_origin: *radius_at_origin,
                half_angle: *half_angle,
            },
            r::Surface::Sphere { frame, radius } => r::Surface::Sphere { frame: self.frame(frame)?, radius: *radius },
            r::Surface::Torus { frame, major_radius, minor_radius, tube_start } => r::Surface::Torus {
                frame: self.frame(frame)?,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
                tube_start: *tube_start,
            },
            r::Surface::BallPatch { center, radius } => r::Surface::BallPatch { center: self.point(*center), radius: *radius },
            r::Surface::Spline { u_knots, v_knots, points } => r::Surface::Spline {
                u_knots: u_knots.clone(),
                v_knots: v_knots.clone(),
                points: points.iter().map(|row| row.iter().map(|p| self.point(*p)).collect()).collect(),
            },
            r::Surface::RationalSpline { u_knots, v_knots, homogeneous } => r::Surface::RationalSpline {
                u_knots: u_knots.clone(),
                v_knots: v_knots.clone(),
                homogeneous: homogeneous.iter().map(|row| row.iter().map(|h| self.homogeneous(*h)).collect()).collect(),
            },
            r::Surface::PlacedRevolution { .. } | r::Surface::Blend { .. } => return None,
        })
    }
}

/// Where a body's records are placed in the world: a similarity (a rotation, perhaps a
/// reflection, a uniform scale, a translation), applied to every point, direction and length
/// as it is read, so the assembler builds the placed body.
#[derive(Clone, Copy)]
pub(crate) struct Place {
    /// Rows: the images of x, y and z (with the scale).
    pub(crate) rows: [[f64; 3]; 3],
    pub(crate) translation: [f64; 3],
    pub(crate) scale: f64,
    pub(crate) mirror: bool,
}

impl Place {
    pub(crate) const NONE: Place = Place {
        rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation: [0.0; 3],
        scale: 1.0,
        mirror: false,
    };

    /// The placement of an affine that is a similarity to a millionth; `None` for a shear or a
    /// scale that differs by axis.
    pub(crate) fn of(matrix: &[[f64; 4]; 4]) -> Option<Place> {
        let rows = [0, 1, 2].map(|i| [matrix[i][0], matrix[i][1], matrix[i][2]]);
        let lengths = rows.map(length);
        let scale = lengths[0];
        if !(scale.is_finite() && scale > 0.0)
            || lengths.iter().any(|l| (l - scale).abs() > 1e-6 * scale)
            || (0..3).any(|i| (0..i).any(|j| dot(rows[i], rows[j]).abs() > 1e-6 * scale * scale))
        {
            return None;
        }
        Some(Place {
            rows,
            translation: [matrix[3][0], matrix[3][1], matrix[3][2]],
            scale,
            mirror: dot(cross(rows[0], rows[1]), rows[2]) < 0.0,
        })
    }

    pub(crate) fn vector(&self, v: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|k| v[0] * self.rows[0][k] + v[1] * self.rows[1][k] + v[2] * self.rows[2][k])
    }

    pub(crate) fn point(&self, p: [f64; 3]) -> [f64; 3] {
        add(self.vector(p), self.translation)
    }

    pub(crate) fn homogeneous(&self, h: [f64; 4]) -> [f64; 4] {
        let p = self.vector([h[0], h[1], h[2]]);
        [0, 1, 2, 3].map(|k| if k < 3 { p[k] + self.translation[k] * h[3] } else { h[3] })
    }

    /// A frame placed: its origin and axes moved, made square and right-handed about the moved
    /// z axis.
    pub(crate) fn frame(&self, f: &r::Frame) -> Option<r::Frame> {
        frame(self.point(f.origin), self.vector(f.x_axis), self.vector(cross(f.x_axis, f.y_axis)))
    }

    /// A curve placed. A reflected circle or ellipse runs the other way round its moved axis,
    /// so its parameter is negated.
    pub(crate) fn curve(&self, c: r::Curve, interval: [f64; 2]) -> Option<(r::Curve, [f64; 2])> {
        let turned = if self.mirror { [-interval[1], -interval[0]] } else { interval };
        Some(match c {
            r::Curve::Line { start, end } => (r::Curve::Line { start: self.point(start), end: self.point(end) }, interval),
            r::Curve::Circle { frame, radius } => (r::Curve::Circle { frame: self.frame(&frame)?, radius: radius * self.scale }, turned),
            r::Curve::Ellipse { frame, radii } => (r::Curve::Ellipse { frame: self.frame(&frame)?, radii: radii.map(|x| x * self.scale) }, turned),
            r::Curve::Spline { knots, points } => (r::Curve::Spline { knots, points: points.iter().map(|p| self.point(*p)).collect() }, interval),
            r::Curve::RationalSpline { knots, homogeneous } => (
                r::Curve::RationalSpline { knots, homogeneous: homogeneous.iter().map(|h| self.homogeneous(*h)).collect() },
                interval,
            ),
            _ => return None,
        })
    }

    /// A surface placed.
    pub(crate) fn surface(&self, s: r::Surface) -> Option<r::Surface> {
        Some(match s {
            r::Surface::Plane { origin, u_axis, v_axis } => {
                let f = frame(self.point(origin), self.vector(u_axis), self.vector(cross(u_axis, v_axis)))?;
                r::Surface::Plane { origin: f.origin, u_axis: f.x_axis, v_axis: f.y_axis }
            }
            r::Surface::Cylinder { frame, radius } => r::Surface::Cylinder { frame: self.frame(&frame)?, radius: radius * self.scale },
            r::Surface::Cone { frame, radius_at_origin, half_angle } => r::Surface::Cone {
                frame: self.frame(&frame)?,
                radius_at_origin: radius_at_origin * self.scale,
                half_angle,
            },
            r::Surface::Sphere { frame, radius } => r::Surface::Sphere { frame: self.frame(&frame)?, radius: radius * self.scale },
            r::Surface::Torus { frame, major_radius, minor_radius, tube_start } => r::Surface::Torus {
                frame: self.frame(&frame)?,
                major_radius: major_radius * self.scale,
                minor_radius: minor_radius * self.scale,
                tube_start,
            },
            r::Surface::Spline { u_knots, v_knots, points } => r::Surface::Spline {
                u_knots,
                v_knots,
                points: points.iter().map(|row| row.iter().map(|p| self.point(*p)).collect()).collect(),
            },
            r::Surface::RationalSpline { u_knots, v_knots, homogeneous } => r::Surface::RationalSpline {
                u_knots,
                v_knots,
                homogeneous: homogeneous.iter().map(|row| row.iter().map(|h| self.homogeneous(*h)).collect()).collect(),
            },
            _ => return None,
        })
    }
}

/// Whether the natural normal of the surface a face's (u, v) are on (its NURBS form, when it
/// has one) is its surface's own.
pub(crate) fn form_agrees(face: &r::Face) -> bool {
    let Some(form) = &face.nurbs else {
        return true;
    };
    let normal = |surface: &r::Surface, uv: Option<[f64; 2]>, at: Option<[f64; 3]>| -> Option<([f64; 3], [f64; 3])> {
        let converted = solidcore_convert::surface(surface).ok()?;
        let eval = k::SurfaceEval::new(&converted).ok()?;
        let uv = match (uv, at) {
            (Some(uv), _) => uv,
            (None, Some(p)) => eval.project(solidcore_convert::vec3(p.into()), None, 1e-3 * (1.0 + length(p)))?,
            _ => return None,
        };
        let (p, su, sv) = eval.derivs(uv[0], uv[1])?;
        let [p, su, sv] = [p, su, sv].map(point_of);
        Some((p, cross(su, sv)))
    };
    let mid = face.domain.map(|[a, b]| 0.5 * (a + b));
    let Some((p, on_form)) = normal(form, Some(mid), None) else {
        return true;
    };
    let Some((_, on_surface)) = normal(&face.surface, None, Some(p)) else {
        return true;
    };
    dot(on_form, on_surface) >= 0.0
}

/// `brep` placed by the similarity `matrix` (rows the images of x, y and z, then the
/// translation): its vertices, edges and faces read again through Solidcore's assembler with
/// every point, curve and surface placed, so its pcurves are made for the placed geometry.
/// `None` for a map that is not a similarity, or a record the assembler refuses.
pub fn reassembled(brep: &r::Brep, matrix: &[[f64; 4]; 4]) -> Option<r::Brep> {
    let place = Place::of(matrix)?;
    reassemble(brep, &place, |_, face| {
        // On its own surface where the placement can carry it, else on its NURBS form.
        match place.surface(face.surface.clone()) {
            Some(surface) => Some((surface, form_agrees(face))),
            None => Some((place.surface(face.parameters().clone())?, true)),
        }
    })
}

/// A spline that runs along a circle or an ellipse over `interval`, as that conic: its frame's
/// x along the ellipse's first axis (toward the spline's start on a circle), turning the way
/// the spline runs.
pub fn conic_of(curve: &r::Curve, interval: [f64; 2]) -> Option<r::Curve> {
    if !matches!(curve, r::Curve::Spline { .. } | r::Curve::RationalSpline { .. }) {
        return None;
    }
    let points = points_on(curve, interval, 16)?;
    let (a, b, c) = (points[0], points[5], points[11]);
    let (ab, ac) = (sub(b, a), sub(c, a));
    let n = cross(ab, ac);
    let nn = dot(n, n);
    // Points in a line (or nearly) are no conic's.
    if !(nn.is_finite() && nn.sqrt() > 1e-6 * length(ab) * length(ac)) {
        return None;
    }
    let normal = unit(n)?;
    let size = points.iter().map(|p| length(*p)).fold(1.0, f64::max);
    let tolerance = 1e-9 * size;
    if points.iter().any(|p| dot(sub(*p, a), normal).abs() > tolerance) {
        return None;
    }
    let centre = add(a, scale(add(scale(cross(n, ab), dot(ac, ac)), scale(cross(ac, n), dot(ab, ab))), 0.5 / nn));
    let radius = length(sub(a, centre));
    if points.iter().all(|p| (length(sub(*p, centre)) - radius).abs() <= tolerance) {
        let x = unit(sub(a, centre))?;
        let frame = r::Frame { origin: centre, x_axis: x, y_axis: cross(normal, x) };
        return Some(r::Curve::Circle { frame, radius });
    }
    ellipse_through(&points, normal, tolerance)
}

/// The ellipse in the plane across `normal` that `points` lie on, if they do.
fn ellipse_through(points: &[[f64; 3]], normal: [f64; 3], tolerance: f64) -> Option<r::Curve> {
    let origin = points[0];
    let u = unit(sub(points[8], origin))?;
    let v = cross(normal, u);
    let local: Vec<[f64; 2]> = points.iter().map(|p| [dot(sub(*p, origin), u), dot(sub(*p, origin), v)]).collect();
    // A x² + B xy + C y² + D x + E y = 1 through five of them, about their middle (where no
    // conic through them passes).
    let mid = [0, 1].map(|k| local.iter().map(|p| p[k]).sum::<f64>() / local.len() as f64);
    let at = |i: usize| [local[i][0] - mid[0], local[i][1] - mid[1]];
    let mut m: Vec<[f64; 6]> = [0, 3, 6, 10, 13]
        .iter()
        .map(|&i| {
            let [x, y] = at(i);
            [x * x, x * y, y * y, x, y, 1.0]
        })
        .collect();
    for col in 0..5 {
        let pivot = (col..5).max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))?;
        m.swap(col, pivot);
        if m[col][col].abs() < 1e-300 {
            return None;
        }
        for row in 0..5 {
            if row != col {
                let f = m[row][col] / m[col][col];
                for k in col..6 {
                    m[row][k] -= f * m[col][k];
                }
            }
        }
    }
    let [qa, qb, qc, qd, qe] = [0, 1, 2, 3, 4].map(|i| m[i][5] / m[i][i]);
    // An ellipse: 4AC − B² > 0. Its centre, where the gradient vanishes.
    let det = 4.0 * qa * qc - qb * qb;
    if !(det > 0.0) {
        return None;
    }
    let cx = (qb * qe - 2.0 * qc * qd) / det;
    let cy = (qb * qd - 2.0 * qa * qe) / det;
    let k = 1.0 - (qa * cx * cx + qb * cx * cy + qc * cy * cy + qd * cx + qe * cy);
    // Principal axes of [[A, B/2], [B/2, C]].
    let theta = 0.5 * qb.atan2(qa - qc);
    let (sin, cos) = theta.sin_cos();
    let l1 = qa * cos * cos + qb * sin * cos + qc * sin * sin;
    let l2 = qa * sin * sin - qb * sin * cos + qc * cos * cos;
    if !(l1 > 0.0 && l2 > 0.0 && k > 0.0) {
        return None;
    }
    let (r1, r2) = ((k / l1).sqrt(), (k / l2).sqrt());
    let centre = add(origin, add(scale(u, cx + mid[0]), scale(v, cy + mid[1])));
    let x = add(scale(u, cos), scale(v, sin));
    let frame = r::Frame { origin: centre, x_axis: x, y_axis: cross(normal, x) };
    let ellipse = r::Curve::Ellipse { frame: frame.clone(), radii: [r1, r2] };
    // Every sample on it: its point at its own angle.
    let on = |p: &[f64; 3]| {
        let d = sub(*p, frame.origin);
        let t = (dot(d, frame.y_axis) / r2).atan2(dot(d, frame.x_axis) / r1);
        let q = add(frame.origin, add(scale(frame.x_axis, r1 * t.cos()), scale(frame.y_axis, r2 * t.sin())));
        length(sub(*p, q)) <= tolerance
    };
    points.iter().all(on).then_some(ellipse)
}

/// A spline surface that is a cylinder, cone or sphere over `domain`, as that surface (its frame
/// any about its axis: a face keeps the spline as the form it is parameterised on).
// ponytail: tori are left splines; recognise them when a command makes them often.
pub fn quadric_of(surface: &r::Surface, domain: [[f64; 2]; 2]) -> Option<r::Surface> {
    if !matches!(surface, r::Surface::Spline { .. } | r::Surface::RationalSpline { .. }) {
        return None;
    }
    let on = OnSurface::of(surface)?;
    const N: usize = 6;
    let mut samples = Vec::with_capacity(N * N);
    for i in 0..N {
        for j in 0..N {
            let uv = [
                domain[0][0] + (domain[0][1] - domain[0][0]) * (i as f64 + 0.5) / N as f64,
                domain[1][0] + (domain[1][1] - domain[1][0]) * (j as f64 + 0.5) / N as f64,
            ];
            samples.push((on.point(uv)?, on.normal(uv)?));
        }
    }
    let size = samples.iter().map(|(p, _)| length(*p)).fold(1.0, f64::max);
    let tolerance = 1e-8 * size;
    // The two normals furthest apart, and where their lines pass closest: on the axis of a
    // surface of revolution, at the centre of a sphere.
    let (mut best, mut pair) = (0.0, (0, 1));
    for a in 0..samples.len() {
        for b in a + 1..samples.len() {
            let c = length(cross(samples[a].1, samples[b].1));
            if c > best {
                (best, pair) = (c, (a, b));
            }
        }
    }
    if best < 1e-3 {
        return None;
    }
    let ((p1, n1), (p2, n2)) = (samples[pair.0], samples[pair.1]);
    let meet = {
        // Closest points of p1 + s n1 and p2 + t n2.
        let w = sub(p1, p2);
        let (b, d, e) = (dot(n1, n2), dot(n1, w), dot(n2, w));
        let den = 1.0 - b * b;
        let (s, t) = ((b * e - d) / den, (e - b * d) / den);
        scale(add(add(p1, scale(n1, s)), add(p2, scale(n2, t))), 0.5)
    };
    // A sphere: every point as far from that centre, along its normal.
    let radius = length(sub(p1, meet));
    if samples.iter().all(|(p, n)| {
        let d = sub(*p, meet);
        (length(d) - radius).abs() <= tolerance && length(cross(d, *n)) <= tolerance
    }) {
        let x = unit(sub(p1, meet))?;
        let y = unit(cross(cross(x, n2), x)).or_else(|| unit(cross(x, [0.0, 0.0, 1.0]))).or_else(|| unit(cross(x, [0.0, 1.0, 0.0])))?;
        let f = frame(meet, x, cross(x, y))?;
        return Some(r::Surface::Sphere { frame: f, radius });
    }
    // A cylinder or cone: its normals keep one angle to the axis, so their differences are
    // square to it.
    let (mut best, mut axis) = (0.0, None);
    for a in 0..samples.len() {
        for b in a + 1..samples.len() {
            let d1 = sub(samples[a].1, samples[0].1);
            let d2 = sub(samples[b].1, samples[0].1);
            let c = cross(d1, d2);
            if length(c) > best {
                (best, axis) = (length(c), unit(c));
            }
        }
    }
    let axis = axis.filter(|_| best > 1e-6)?;
    let slope = dot(samples[0].1, axis);
    if samples.iter().any(|(_, n)| (dot(*n, axis) - slope).abs() > 1e-8) {
        return None;
    }
    // Every normal line crosses the axis: in the plane square to it, where two of them meet.
    let flat = |v: [f64; 3]| sub(v, scale(axis, dot(v, axis)));
    let meet = {
        let (q1, m1) = (flat(p1), unit(flat(n1))?);
        let (q2, m2) = (flat(p2), unit(flat(n2))?);
        let w = sub(q1, q2);
        let (b, d, e) = (dot(m1, m2), dot(m1, w), dot(m2, w));
        let den = 1.0 - b * b;
        if den < 1e-12 {
            return None;
        }
        let s = (b * e - d) / den;
        add(q1, scale(m1, s))
    };
    // Heights along the axis from the meeting point, and distances from it.
    let polar = |p: [f64; 3]| {
        let d = sub(p, meet);
        let h = dot(d, axis);
        (h, length(sub(d, scale(axis, h))))
    };
    let (h1, _) = polar(p1);
    // The radius's slope from the lowest sample to the highest.
    let polars: Vec<(f64, f64)> = samples.iter().map(|(p, _)| polar(*p)).collect();
    let (hl, rl) = polars.iter().copied().min_by(|a, b| a.0.total_cmp(&b.0))?;
    let (hh, rh) = polars.iter().copied().max_by(|a, b| a.0.total_cmp(&b.0))?;
    let (tan, r0) = if hh - hl > tolerance {
        let tan = (rh - rl) / (hh - hl);
        (tan, rl - tan * hl)
    } else {
        return None;
    };
    if samples.iter().any(|(p, _)| {
        let (h, rho) = polar(*p);
        (rho - (r0 + tan * h)).abs() > tolerance
    }) {
        return None;
    }
    let x = unit(sub(sub(p1, meet), scale(axis, h1)))?;
    let f = frame(meet, x, axis)?;
    Some(if tan.abs() <= 1e-12 {
        r::Surface::Cylinder { frame: f, radius: r0 }
    } else {
        r::Surface::Cone { frame: f, radius_at_origin: r0, half_angle: tan.atan() }
    })
}

/// An edge's curve as its label names it (a line, circle or ellipse) and the edge's interval
/// on that curve, read off its vertices where the edge is parameterised on a NURBS form.
pub fn labelled(brep: &r::Brep, edge: &r::Edge) -> Option<(r::Curve, [f64; 2])> {
    if edge.nurbs.is_none() {
        return Some((edge.curve.clone(), edge.interval));
    }
    let [start, end] = edge.vertices.map(|v| brep.vertices.get(v as usize).map(|v| v.point));
    let (start, end) = (start?, end?);
    let interval = match &edge.curve {
        r::Curve::Line { start: s, end: e } => {
            let d = sub(*e, *s);
            let at = |p: [f64; 3]| dot(sub(p, *s), d) / dot(d, d);
            [at(start), at(end)]
        }
        r::Curve::Circle { frame, radius } => turn(frame, [*radius; 2], start, end, edge.vertices[0] == edge.vertices[1]),
        r::Curve::Ellipse { frame, radii } => turn(frame, *radii, start, end, edge.vertices[0] == edge.vertices[1]),
        _ => return Some((edge.parameters().clone(), edge.interval)),
    };
    Some((edge.curve.clone(), interval))
}

/// The angles from `start` to `end` anticlockwise on the ellipse of `radii` in `frame`; a whole
/// turn when `closed`.
fn turn(frame: &r::Frame, radii: [f64; 2], start: [f64; 3], end: [f64; 3], closed: bool) -> [f64; 2] {
    let angle = |p: [f64; 3]| {
        let d = sub(p, frame.origin);
        (dot(d, frame.y_axis) / radii[1]).atan2(dot(d, frame.x_axis) / radii[0])
    };
    let low = angle(start);
    let mut high = if closed { low + std::f64::consts::TAU } else { angle(end) };
    while high <= low + 1e-12 {
        high += std::f64::consts::TAU;
    }
    [low, high]
}

/// The affine that moves nothing.
pub const IDENTITY: [[f64; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// `brep` with every face on its exact NURBS form, its edges kept and its pcurves made again.
pub fn on_nurbs_forms(brep: &r::Brep) -> Option<r::Brep> {
    let place = Place::of(&IDENTITY)?;
    reassemble(brep, &place, |index, face| {
        if face.nurbs.is_some() || matches!(face.surface, r::Surface::Spline { .. } | r::Surface::RationalSpline { .. }) {
            return Some((face.parameters().clone(), form_agrees(face)));
        }
        // The exact form over the face's box: its sheet's control net made editable.
        let sheet = crate::model::face_sheet(brep, &[index as u32])?;
        let editable = solidcore_ops::surface_cvs::move_cvs(&sheet, &[]).ok()?;
        let spline = editable.faces.first()?.surface.clone();
        let mut probe = face.clone();
        probe.nurbs = Some(spline.clone());
        Some((spline, form_agrees(&probe)))
    })
}

/// `brep`'s vertices, edges and faces read again through Solidcore's assembler, placed by
/// `place`, each face on the surface `surface_of` gives (with whether its natural normal runs
/// the way the record surface's does).
fn reassemble(brep: &r::Brep, place: &Place, surface_of: impl Fn(usize, &r::Face) -> Option<(r::Surface, bool)>) -> Option<r::Brep> {
    static TRACED: Traced = Traced;
    let mut assembly = solidcore_step::assembly::Assembly::new(brep.tolerance.max(1e-6) * place.scale, Some(&TRACED));
    let vertices: Vec<u32> = brep
        .vertices
        .iter()
        .map(|v| assembly.add_vertex(place.point(v.point)).ok())
        .collect::<Option<_>>()?;
    // Each assembled edge, and whether it runs the way the record's did.
    let mut edges: Vec<(u32, bool)> = Vec::with_capacity(brep.edges.len());
    for (index, edge) in brep.edges.iter().enumerate() {
        let (curve, interval) = place.curve(edge.parameters().clone(), edge.interval)?;
        let ends = [vertices[edge.vertices[0] as usize], vertices[edge.vertices[1] as usize]];
        // A reflected circle runs the other way: its interval was negated, so its ends swap.
        let turned = place.mirror && matches!(curve, r::Curve::Circle { .. } | r::Curve::Ellipse { .. });
        let given = if turned { [ends[1], ends[0]] } else { ends };
        let (added, as_given) = assembly.add_edge(index as u64, curve, interval, given).ok()?;
        let first = if as_given { given[0] } else { given[1] };
        let same = if ends[0] == ends[1] { !turned } else { first == ends[0] };
        edges.push((added, same));
    }
    let mut sides = Vec::with_capacity(brep.faces.len());
    let record_sides = crate::model::face_sides(brep);
    for (index, face) in brep.faces.iter().enumerate() {
        let (surface, agrees) = surface_of(index, face)?;
        let spline = matches!(surface, r::Surface::Spline { .. } | r::Surface::RationalSpline { .. });
        // A reflection turns a spline's natural normal, not a rebuilt analytic frame's.
        let flips_natural = place.mirror && spline;
        let side = (record_sides[index] == agrees) != flips_natural;
        // Loops run counter-clockwise about the natural normal the record's (u, v) are on;
        // that normal turned, or the loops reflected, they run the other way.
        let reverse = !agrees != (place.mirror && !spline);
        let bounds = face
            .loops
            .iter()
            .map(|ring| match ring {
                r::Loop::Edges { edges: uses } => {
                    let mapped: Vec<(u32, bool)> = uses
                        .iter()
                        .map(|u| {
                            let (edge, same) = edges[u.edge as usize];
                            (edge, u.forward == same)
                        })
                        .collect();
                    solidcore_step::assembly::Bound::Edges(if reverse {
                        mapped.iter().rev().map(|(e, f)| (*e, !f)).collect()
                    } else {
                        mapped
                    })
                }
                r::Loop::Vertex { vertex } => solidcore_step::assembly::Bound::Vertex(vertices[*vertex as usize]),
            })
            .collect();
        assembly.add_face(index as u64, surface, bounds, None, side).ok()?;
        sides.push(side);
    }
    let shells: Vec<Vec<(u32, bool)>> = brep
        .shells
        .iter()
        .filter_map(|shell| match shell {
            r::Shell::Faces { faces, .. } => Some(faces.iter().map(|f| (f.face, sides[f.face as usize])).collect()),
            _ => None,
        })
        .collect();
    let material: Vec<&r::Region> = brep.regions.iter().skip(1).filter(|region| !region.void).collect();
    let kind = match material[..] {
        [region] if region.shells.len() == 1 && shells.len() == 1 => solidcore_step::assembly::Kind::Solid,
        [region] if region.shells.len() == shells.len() => solidcore_step::assembly::Kind::Voids,
        _ => solidcore_step::assembly::Kind::Model,
    };
    assembly.finish(0, String::new(), shells, kind).ok().map(|solid| solid.brep)
}

/// A record surface evaluated: its points, natural normals, and the parameters of a point.
pub struct OnSurface(k::SurfaceEval);

impl OnSurface {
    pub fn of(surface: &r::Surface) -> Option<Self> {
        let converted = solidcore_convert::surface(surface).ok()?;
        Some(Self(k::SurfaceEval::new(&converted).ok()?))
    }

    /// The parameters of the surface's point nearest `p`.
    pub fn uv(&self, p: [f64; 3]) -> Option<[f64; 2]> {
        self.0.project(solidcore_convert::vec3(p.into()), None, f64::INFINITY)
    }

    pub fn point(&self, uv: [f64; 2]) -> Option<[f64; 3]> {
        self.0.point(uv).map(point_of)
    }

    /// The unit natural normal (∂u × ∂v) at `uv`.
    pub fn normal(&self, uv: [f64; 2]) -> Option<[f64; 3]> {
        let (_, su, sv) = self.0.derivs(uv[0], uv[1])?;
        unit(cross(point_of(su), point_of(sv)))
    }
}

/// An axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Lowest corner.
    pub min: [f64; 3],
    /// Highest corner.
    pub max: [f64; 3],
}

impl Aabb {
    /// The box around `points`; `None` for none.
    pub fn around(points: impl IntoIterator<Item = [f64; 3]>) -> Option<Aabb> {
        let mut points = points.into_iter();
        let first = points.next()?;
        Some(points.fold(Aabb { min: first, max: first }, |b, p| Aabb {
            min: [0, 1, 2].map(|k| b.min[k].min(p[k])),
            max: [0, 1, 2].map(|k| b.max[k].max(p[k])),
        }))
    }
}
