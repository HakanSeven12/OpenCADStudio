//! A space curve seen in a surface's own coordinates.
//!
//! Splitting a face is a two-dimensional problem. The face is a region of its
//! surface's `(u, v)` space bounded by loops; the curve that cuts it is
//! another set of `(u, v)` points; and what has to happen — where they cross,
//! which pieces the cut leaves, which side of the cut a piece is on — is
//! exactly what [`geom2d`](ocs_geom::geom2d) answers. So the last step before a
//! boolean can do anything is bringing the curve down into that space.
//!
//! ACIS calls the result a pcurve and stores one on every coedge, for the
//! same reason.
//!
//! # Parameters do not carry across
//!
//! The result traces the same *points* as the space curve, not the same
//! parameter values: a circle in space is parameterised by angle here and by
//! a fraction of a turn in [`geom2d`]. A caller needing the other curve's
//! parameter goes through the point, which both kinds invert exactly.
//! Inventing a mapping type to carry the difference would put a conversion in
//! every signature to save one call at a handful of sites.
//!
//! # Where the projection does not exist
//!
//! On a cylinder, a slanted plane's section is a sine wave in `(u, v)` — no
//! [`Curve`] variant is one, and calling it a spline would be an
//! approximation dressed as a fact. Those cases answer `None`, on the same
//! principle as [`Meeting::Unknown`](super::Meeting): a caller told nothing
//! can refuse, a caller told something wrong cannot.

use super::geometry::{Curve3, Surface};
use ocs_geom::geom2d::{
    Arc, Circle, Curve, Ellipse, EllipseArc, Line, Polyline, PolylineVertex, XLine,
};
use ocs_geom::space::Vec3;
use std::f64::consts::TAU;

/// The curve `curve` traces in `surface`'s parameter space.
///
/// `None` when the curve does not lie on the surface, and when the shape it
/// traces there is not one this kernel can write down.
pub fn project(surface: &Surface, curve: &Curve3, tolerance: f64) -> Option<Curve> {
    // A boolean projects one cutter onto one surface over and over — on
    // every face it tries, every round — and off a plane the image is
    // sampled or inverted point by point. The last few are kept: a curve
    // and a surface equal to ones seen give the image found then.
    type Projection = (Surface, Curve3, u64, Option<Curve>);
    thread_local! {
        static SEEN: std::cell::RefCell<Vec<Projection>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    if matches!(surface, Surface::Plane(_)) && !matches!(curve, Curve3::Nurbs(_)) {
        return project_afresh(surface, curve, tolerance);
    }
    let bits = tolerance.to_bits();
    let known = SEEN.with(|seen| {
        seen.borrow()
            .iter()
            .find(|(s, c, t, _)| *t == bits && s == surface && c == curve)
            .map(|(_, _, _, image)| image.clone())
    });
    if let Some(image) = known {
        return image;
    }
    let image = project_afresh(surface, curve, tolerance);
    SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        // ponytail: a short list searched in turn; a map keyed by a hash if
        // booleans ever carry hundreds of cutters.
        if seen.len() >= 32 {
            seen.remove(0);
        }
        seen.push((surface.clone(), curve.clone(), bits, image.clone()));
    });
    image
}

fn project_afresh(surface: &Surface, curve: &Curve3, tolerance: f64) -> Option<Curve> {
    // A curve that is not on the surface has no image in its parameter
    // space, and projecting it anyway would produce a plausible shape in the
    // wrong place. Sampled rather than reasoned about per pair: the check is
    // the same for every combination and cheap next to what follows.
    if !lies_on(surface, curve, tolerance) {
        return None;
    }
    // A spline edge — an intersection a file saved as one — has no closed
    // image on an analytic surface, and without one the face had no
    // boundary at all: no point could be placed in or out of a solid with a
    // hex socket, so every boolean against it was refused (#1563). Its
    // image is walked instead, which `trim_to` redoes over the edge's own
    // span, the same way a general circle on a sphere is kept.
    if let Curve3::Nurbs(spline) = curve {
        // In a plane it is exact: the plane's coordinates are an affine
        // map, which a spline follows by its control points alone. A walk
        // there turns a curved edge into a chain no wall can be laid along.
        if let Surface::Plane(plane) = surface {
            if let Some(image) = planar_image(plane, spline, tolerance) {
                return Some(image);
            }
        }
        return sampled_image(surface, curve);
    }
    match surface {
        Surface::Plane(plane) => match curve {
            Curve3::Line(line) => Some(Curve::XLine(XLine {
                base: plane.project(line.origin)?,
                direction: plane.project_vector(line.direction)?,
            })),
            Curve3::Circle(circle) => Some(Curve::Circle(Circle {
                centre: plane.project(circle.plane.origin)?,
                radius: circle.radius,
            })),
            Curve3::Ellipse(ellipse) => {
                let major = plane.project_vector(ellipse.plane.x_axis)?;
                let length = major[0].hypot(major[1]);
                if length <= tolerance {
                    return None;
                }
                Some(Curve::Ellipse(EllipseArc {
                    ellipse: Ellipse {
                        centre: plane.project(ellipse.plane.origin)?,
                        major_radius: ellipse.major_radius,
                        minor_radius: ellipse.minor_radius,
                        major_axis: [major[0] / length, major[1] / length],
                    },
                    start_parameter: 0.0,
                    end_parameter: TAU,
                }))
            }
            // Already a plane curve; it only has to be re-expressed in this
            // plane's coordinates rather than its own.
            Curve3::PlanarSpline { curve, .. } => Some(Curve::Nurbs(curve.clone())),
            Curve3::Nurbs(_) => None,
        },

        // On a cylinder `u` runs round and `v` along the axis, so the two
        // curves that stay straight there are the ones aligned with it: a
        // circle at one height, and a generator. A slanted section is a sine
        // wave, kept as a sampled chain.
        Surface::Cylinder(cylinder) => match curve {
            Curve3::Circle(circle) => {
                let axis = Vec3::from(cylinder.base.normal()?);
                let plane_normal = Vec3::from(circle.plane.normal()?);
                if !plane_normal.is_parallel_to(axis, tolerance) {
                    return None;
                }
                let height = (Vec3::from(circle.plane.origin)
                    - Vec3::from(cylinder.base.origin))
                .dot(axis);
                Some(band_at(height))
            }
            Curve3::Line(line) => {
                let axis = Vec3::from(cylinder.base.normal()?);
                if !Vec3::from(line.direction).is_parallel_to(axis, tolerance) {
                    return None;
                }
                Some(generator_at(angle_about(
                    &cylinder.base,
                    line.origin,
                )?))
            }
            _ => sampled_closed_conic(surface, curve),
        },

        // The same two shapes on a cone, with `v` measured along the axis as
        // it is for a cylinder.
        Surface::Cone(cone) => match curve {
            Curve3::Circle(circle) => {
                let axis = Vec3::from(cone.base.normal()?);
                let plane_normal = Vec3::from(circle.plane.normal()?);
                if !plane_normal.is_parallel_to(axis, tolerance) {
                    return None;
                }
                let height =
                    (Vec3::from(circle.plane.origin) - Vec3::from(cone.base.origin)).dot(axis);
                Some(band_at(height))
            }
            Curve3::Line(line) => {
                // A generator leans by the half-angle, so it is not parallel
                // to the axis; what identifies it is that it keeps one angle
                // all the way along.
                let start = angle_about(&cone.base, line.origin)?;
                let further = angle_about(
                    &cone.base,
                    (Vec3::from(line.origin) + Vec3::from(line.direction)).to_array(),
                )?;
                let turn = (further - start).abs();
                if turn.min(TAU - turn) > tolerance {
                    return None;
                }
                Some(generator_at(start))
            }
            _ => sampled_closed_conic(surface, curve),
        },

        // A torus closes both ways, so both families of circles on it are
        // straight in `(u, v)`: the parallels, which run round the ring at
        // one place on the tube, and the meridians, which run round the tube
        // at one place on the ring. Between them they are every edge a
        // revolution puts on one. Anything else — a circle cutting across
        // both — is a quartic's section, kept as a sampled chain.
        Surface::Torus(torus) => match curve {
            Curve3::Circle(circle) => {
                let axis = Vec3::from(torus.frame.normal()?);
                let plane_normal = Vec3::from(circle.plane.normal()?);
                let offset = Vec3::from(circle.plane.origin) - Vec3::from(torus.frame.origin);
                if plane_normal.is_parallel_to(axis, tolerance) {
                    // A parallel. Where it sits round the tube is fixed by
                    // how far out and how high it is — read the way
                    // `parameters_at` reads it, through the minor radius's
                    // sign: a torus stored with a negative one (a concave
                    // fillet) otherwise put every parallel half a turn round
                    // the tube, where a cut that missed the face fell inside
                    // it (#1563).
                    let sign = torus.minor_radius.signum();
                    Some(band_at(
                        (offset.dot(axis) * sign)
                            .atan2((circle.radius - torus.major_radius) * sign),
                    ))
                } else if plane_normal.dot(axis).abs() <= tolerance {
                    Some(meridian_at(angle_about(&torus.frame, circle.plane.origin)?))
                } else {
                    sampled_closed_conic(surface, curve)
                }
            }
            _ => sampled_closed_conic(surface, curve),
        },

        // A latitude is straight in a sphere's `(u, v)` space. This is the
        // exact pcurve needed when a plane perpendicular to the sphere axis
        // cuts it, including the equator used by coaxial round solids. Other
        // circles can cross the longitude seam or a pole and need a general
        // spherical pcurve representation rather than being guessed here.
        Surface::Sphere(sphere) => match curve {
            Curve3::Circle(circle) => {
                let axis = Vec3::from(sphere.frame.normal()?);
                let plane_normal = Vec3::from(circle.plane.normal()?);
                if plane_normal.is_parallel_to(axis, tolerance) {
                    let height = (Vec3::from(circle.plane.origin)
                        - Vec3::from(sphere.frame.origin))
                    .dot(axis);
                    if height.abs() > sphere.radius + tolerance {
                        return None;
                    }
                    return Some(band_at(
                        (height / sphere.radius).clamp(-1.0, 1.0).asin(),
                    ));
                }
                let centred = Vec3::from(circle.plane.origin)
                    .distance(Vec3::from(sphere.frame.origin))
                    <= tolerance;
                let meridian = plane_normal.dot(axis).abs() <= tolerance
                    && centred
                    && (circle.radius - sphere.radius).abs() <= tolerance;
                if meridian {
                    return Some(generator_at(angle_about(
                        &sphere.frame,
                        curve.point_at(0.0),
                    )?));
                }
                sampled_closed_conic(surface, curve)
            }
            _ => None,
        },
        Surface::Nurbs(_) | Surface::Offset(_) => None,
    }
}

/// A spline edge's image in an analytic surface's parameters, walked over
/// the spline's whole domain and unwound across any seam.
/// A spline lying in `plane`, written in its coordinates: control points
/// carried over, knots and weights kept. `None` where the result strays off
/// the spline by more than `tolerance` — a fitted curve only near the plane.
fn planar_image(
    plane: &ocs_geom::space::Plane,
    spline: &ocs_geom::space::NurbsCurve3,
    tolerance: f64,
) -> Option<Curve> {
    // A closed spline's edges run on round its seam, past where one copy
    // of it ends; its image is walked instead, which wraps as it goes.
    if spline.periodicity() {
        return None;
    }
    let points = spline
        .control_points()
        .iter()
        .map(|point| plane.project(*point))
        .collect::<Option<Vec<_>>>()?;
    let flat = ocs_geom::geom2d::NurbsCurve::new(
        spline.degree(),
        points,
        spline.knots().to_vec(),
        Some(spline.weights().to_vec()),
    )?;
    let (start, end) = spline.domain();
    let (low, high) = flat.domain();
    if (low - start).abs() > 1e-12 * (1.0 + start.abs())
        || (high - end).abs() > 1e-12 * (1.0 + end.abs())
    {
        return None;
    }
    (0..=32)
        .all(|step| {
            let t = step as f64 / 32.0;
            let there = spline.point_at_knot(start + (end - start) * t);
            Vec3::from(plane.point_at(flat.point_at(t))).distance(Vec3::from(there)) <= tolerance
        })
        .then_some(Curve::Nurbs(flat))
}

fn sampled_image(surface: &Surface, curve: &Curve3) -> Option<Curve> {
    const SAMPLES: usize = 64;
    let Curve3::Nurbs(spline) = curve else {
        return None;
    };
    let (start, end) = spline.domain();
    let periods = periods(surface);
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(SAMPLES + 1);
    for index in 0..=SAMPLES {
        let t = start + (end - start) * index as f64 / SAMPLES as f64;
        let (u, v) = surface.parameters_at(curve.point_at(t))?;
        let mut here = [u, v];
        if let Some(previous) = points.last() {
            for axis in 0..2 {
                if let Some(period) = periods[axis] {
                    here[axis] = unwound(here[axis], previous[axis], period);
                }
            }
        }
        points.push(here);
    }
    Some(Curve::Polyline(Polyline {
        vertices: points.into_iter().map(PolylineVertex::straight).collect(),
        closed: false,
    }))
}

/// A closed conic with no closed-form image — a general circle on a
/// sphere, a slanted section of a cylinder (a sine wave), a conic across a
/// cone or a torus. Its pcurve is a dense parameter-space chain through
/// exact samples while the analytic curve stays the space edge.
fn sampled_closed_conic(surface: &Surface, curve: &Curve3) -> Option<Curve> {
    const SAMPLES: usize = 96;
    if !matches!(curve, Curve3::Circle(_) | Curve3::Ellipse(_)) {
        return None;
    }
    let periods = periods(surface);
    let unwind = |here: [f64; 2], last: [f64; 2]| -> [f64; 2] {
        std::array::from_fn(|axis| match periods[axis] {
            Some(period) => unwound(here[axis], last[axis], period),
            None => here[axis],
        })
    };
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(SAMPLES + 1);
    for index in 0..SAMPLES {
        let parameter = TAU * index as f64 / SAMPLES as f64;
        let (u, v) = surface.parameters_at(curve.point_at(parameter))?;
        let here = points.last().map_or([u, v], |last| unwind([u, v], *last));
        points.push(here);
    }
    let (u, v) = surface.parameters_at(curve.point_at(TAU))?;
    let last = unwind([u, v], *points.last()?);
    let first = points[0];
    // A curve round the axis comes back a turn along, like a band of
    // latitude: one turn, left open, which its periodic images continue.
    let mut closed = true;
    for axis in 0..2 {
        let gap = (last[axis] - first[axis]).abs();
        if gap <= 1.0e-6 {
            continue;
        }
        match periods[axis] {
            Some(period) if (gap - period).abs() <= 1.0e-6 => closed = false,
            _ => return None,
        }
    }
    if !closed {
        points.push(last);
    }
    Some(Curve::Polyline(Polyline {
        vertices: points
            .into_iter()
            .map(PolylineVertex::straight)
            .collect(),
        closed,
    }))
}

/// A face's loops as curves in its surface's parameter space, each trimmed
/// to the edge it came from and oriented the way its loop runs.
///
/// The trimming is the part that matters for containment. A straight edge
/// projects to an infinite line, and a boundary made of infinite lines
/// encloses nothing and reports every point as lying on it. A *circular*
/// edge is the same problem wearing a bounded shape: half an arc on a plane
/// projects to the whole circle it lies on, and a face bounded by whole
/// circles where it meant arcs covers a region nobody asked for.
///
/// The orientation matters for anything walking the ring. An edge has one
/// direction and its two coedges disagree about it, so a boundary built from
/// the edges runs backwards wherever the loop does — half the time on any
/// closed solid. A caller chaining the pieces then gets a ring in no order at
/// all.
///
/// `None` when any edge's projection has no closed form: a boundary with a
/// piece missing is worse than none, since a caller would take the rest for
/// the whole.
pub fn face_boundary(
    body: &super::topology::Body,
    face: super::topology::FaceKey,
    tolerance: f64,
) -> Option<Vec<Curve>> {
    Some(
        face_boundary_parts(body, face, tolerance)?
            .into_iter()
            .map(|(_, curve)| curve)
            .collect(),
    )
}

pub fn face_boundary_parts(
    body: &super::topology::Body,
    face: super::topology::FaceKey,
    tolerance: f64,
) -> Option<Vec<(super::topology::CoedgeKey, Curve)>> {
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    let mut out = Vec::new();
    for ring in &node.loops {
        let mut pieces = Vec::new();
        for coedge in &body.loops.get(*ring)?.coedges {
            let coedge_node = body.coedges.get(*coedge)?;
            if let Some(curve) = &coedge_node.pcurve {
                pieces.push((*coedge, curve.clone()));
                continue;
            }
            let edge_key = coedge_node.edge;
            let edge = body.edges.get(edge_key)?;
            let curve = body.curves.get(edge.curve)?;
            // A curve with no written image — anything on a spline patch, a
            // slanted section of a cylinder — still bounds the face: the walk
            // `trim_to` takes over the edge's own span is its image.
            let whole = (edge.start_parameter, edge.end_parameter);
            let flat = match project(surface, curve, tolerance) {
                Some(flat) => flat,
                None if span_on(surface, curve, whole, tolerance) => Curve::Polyline(Polyline {
                    vertices: Vec::new(),
                    closed: false,
                }),
                None => return None,
            };
            // The loop's own direction, not the edge's.
            let forward = coedge_node.forward;
            let span = if forward {
                (edge.start_parameter, edge.end_parameter)
            } else {
                (edge.end_parameter, edge.start_parameter)
            };
            pieces.push((*coedge, trim_to(surface, curve, span, flat)?));
        }
        let mut curves: Vec<Curve> = pieces.iter().map(|(_, curve)| curve.clone()).collect();
        chain_round(&mut curves, periods(surface));
        for ((_, curve), moved) in pieces.iter_mut().zip(curves) {
            *curve = moved;
        }
        out.extend(pieces);
    }
    Some(out)
}

/// Slides each piece of a loop by whole turns so the ring joins up.
///
/// On a surface closed in `u`, a point does not have one parameter — it has
/// one every turn. A seam is the case that matters: the face runs the whole
/// way round and meets itself there, so its two coedges are the *same* edge
/// and project to the same `u`, when what bounds the face is that edge at
/// zero and again at a full turn. Read literally, the ring collapses to a
/// line and the face cannot be filled at all — which is why a cylinder's
/// wall silently produced no triangles before this.
///
/// ACIS keeps a pcurve on every coedge and so has the answer stored. Until
/// this kernel does too, the ring itself says which turn each piece belongs
/// on: the one that continues from where the last piece ended.
fn chain_round(pieces: &mut [Curve], periods: [Option<f64>; 2]) {
    if periods == [None, None] || pieces.len() < 2 {
        return;
    }
    let mut head = pieces[0].point_at(1.0);
    let mut behind = [pieces[0].point_at(0.0), head, pieces[0].point_at(0.5)];
    for piece in pieces.iter_mut().skip(1) {
        let (start, end, middle) = (piece.point_at(0.0), piece.point_at(1.0), piece.point_at(0.5));
        let mut best = (f64::INFINITY, [0.0, 0.0]);
        for across in turns(periods[0]) {
            for along in turns(periods[1]) {
                let shift = [across, along];
                // A seam is one edge used twice by the same loop, and its two
                // uses are a whole turn apart — that is what makes the face a
                // rectangle in `(u, v)` rather than a slit. Placing the second
                // on top of the first is the nearest fit and always the wrong
                // one: it retraces the piece before it and the ring encloses
                // nothing, which is how a cone's wall came to be missing from
                // every mesh.
                //
                // Only the same path counts: two arcs between one pair of
                // corners — a sliver cut from a sphere — share their ends too.
                let moved = [
                    [start[0] + shift[0], start[1] + shift[1]],
                    [end[0] + shift[0], end[1] + shift[1]],
                    [middle[0] + shift[0], middle[1] + shift[1]],
                ];
                if near(moved[0], behind[1]) && near(moved[1], behind[0]) && near(moved[2], behind[2]) {
                    continue;
                }
                let gap = (moved[0][0] - head[0]).hypot(moved[0][1] - head[1]);
                if gap < best.0 {
                    best = (gap, shift);
                }
            }
        }
        if best.1 != [0.0, 0.0] {
            slide(piece, best.1);
        }
        behind = [piece.point_at(0.0), piece.point_at(1.0), piece.point_at(0.5)];
        head = behind[1];
    }
}

/// Whether two parameter-space points are the same place, at the scale
/// whole turns are compared on.
fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9
}

/// The whole turns worth trying on one axis: none at all when it does not
/// wrap, and a turn either way when it does.
fn turns(period: Option<f64>) -> impl Iterator<Item = f64> {
    let period = period.filter(|period| period.is_finite() && *period > 0.0);
    let span = if period.is_some() { 2 } else { 0 };
    (-span..=span).map(move |turn| period.unwrap_or(0.0) * f64::from(turn))
}

/// Moves a projected piece by whole turns. Only the straight and sampled
/// kinds ever need it: a circle or an ellipse in parameter space comes from a
/// plane, and a plane does not wrap.
fn slide(piece: &mut Curve, shift: [f64; 2]) {
    match piece {
        Curve::Line(line) => {
            for axis in 0..2 {
                line.start[axis] += shift[axis];
                line.end[axis] += shift[axis];
            }
        }
        Curve::Polyline(polyline) => {
            for vertex in &mut polyline.vertices {
                for axis in 0..2 {
                    vertex.position[axis] += shift[axis];
                }
            }
        }
        _ => {}
    }
}

/// How many places along an edge are looked at to find where its projection
/// begins and ends. Three would do for the shapes here; more costs nothing
/// and keeps the unwrapping below honest when an edge covers most of a turn.
const WALK: usize = 8;
/// Steps per whole turn when a sampled projection is walked.
const SAMPLED_WALK: usize = 256;

/// The part of a projected boundary curve the edge actually covers.
///
/// Worked out by walking the space curve over the edge's own parameter range
/// and projecting as it goes, rather than from the two endpoints alone. The
/// endpoints do not say enough: an arc covering three quarters of a turn has
/// the same pair of ends as the quarter left over, and on a closed surface
/// they do not even say which way round the edge went.
fn trim_to(
    surface: &Surface,
    curve: &Curve3,
    span: (f64, f64),
    flat: Curve,
) -> Option<Curve> {
    // A parameter that wraps reads a walk across the seam as a jump
    // backwards unless it is unwound. One that does not wrap must be left
    // alone: unwinding a plane's coordinates would move the curve.
    let periods = periods(surface);
    // A sampled projection is kept as the walk itself, so it walks finely:
    // evenly round the turn, so a short arc is not sampled like a whole one.
    let steps = if matches!(flat, Curve::Polyline(_)) {
        ((span.1 - span.0).abs() / TAU * SAMPLED_WALK as f64).ceil().clamp(16.0, SAMPLED_WALK as f64) as usize
    } else {
        WALK
    };
    let mut raw: Vec<[f64; 2]> = Vec::with_capacity(steps + 1);
    for step in 0..=steps {
        let t = span.0 + (span.1 - span.0) * step as f64 / steps as f64;
        let point = curve.point_at(t);
        if let Some((u, v)) = surface.parameters_at(point) {
            raw.push([u, v]);
            continue;
        }
        let Surface::Sphere(sphere) = surface else {
            return None;
        };
        let local = sphere.frame.project(point)?;
        let axis = Vec3::from(sphere.frame.normal()?);
        let height = (Vec3::from(point) - Vec3::from(sphere.frame.origin)).dot(axis);
        if local[0].hypot(local[1]) > f64::EPSILON * sphere.radius.max(1.0) {
            return None;
        }
        // Longitude is singular at a pole. Keep it unset until a neighbouring
        // non-pole sample supplies the meridian this edge approaches on.
        raw.push([f64::NAN, height.atan2(0.0)]);
    }
    for index in 0..raw.len() {
        if raw[index][0].is_finite() {
            continue;
        }
        let longitude = (1..raw.len()).find_map(|distance| {
            index
                .checked_sub(distance)
                .and_then(|near| raw[near][0].is_finite().then_some(raw[near][0]))
                .or_else(|| {
                    raw.get(index + distance)
                        .and_then(|near| near[0].is_finite().then_some(near[0]))
                })
        })?;
        raw[index][0] = longitude;
    }

    let mut walk: Vec<[f64; 2]> = Vec::with_capacity(steps + 1);
    let mut last: Option<[f64; 2]> = None;
    for mut here in raw {
        if let Some(previous) = last {
            for axis in 0..2 {
                if let Some(period) = periods[axis] {
                    here[axis] = unwound(here[axis], previous[axis], period);
                }
            }
        }
        last = Some(here);
        walk.push(here);
    }
    let (first, final_point) = (walk[0], walk[steps]);

    // An exact image of a space spline: the edge's stretch of it, cut out
    // by knot parameter as the edge's own span runs.
    if let (Curve3::Nurbs(spline), Curve::Nurbs(image)) = (curve, &flat) {
        let (start, end) = spline.domain();
        // A span's ends sit on the domain's only to rounding.
        let at = |knot: f64| {
            let t = (knot - start) / (end - start);
            if (-1e-9..=1.0 + 1e-9).contains(&t) { t.clamp(0.0, 1.0) } else { t }
        };
        if let Some(piece) = image.trimmed(at(span.0), at(span.1)) {
            return Some(Curve::Nurbs(piece));
        }
        let steps = SAMPLED_WALK;
        let walk = (0..=steps)
            .map(|step| {
                let t = span.0 + (span.1 - span.0) * step as f64 / steps as f64;
                surface.parameters_at(curve.point_at(t)).map(|(u, v)| [u, v])
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(Curve::Polyline(Polyline {
            vertices: walk.into_iter().map(PolylineVertex::straight).collect(),
            closed: false,
        }));
    }
    Some(match flat {
        // The kinds that run past their edge become the segment between the
        // two ends. A straight edge's projection is straight, so nothing is
        // lost by saying so — and a band round a cylinder is straight in
        // `(u, v)` too.
        Curve::XLine(_) | Curve::Ray(_) | Curve::Line(_) => Curve::Line(Line {
            start: first,
            end: final_point,
        }),
        Curve::Circle(circle) => {
            let (from, to) = swept(&walk, circle.centre, |point, centre| {
                (point[1] - centre[1]).atan2(point[0] - centre[0])
            });
            let positive = if (to - from).abs() >= TAU - 1e-9 {
                Curve::Circle(circle)
            } else {
                Curve::Arc(Arc { centre: circle.centre, radius: circle.radius,
                    start_angle: from.min(to), end_angle: from.max(to) })
            };
            // Circle and Arc represent positive traversal only. Retain a
            // clockwise coedge as an exact reversed rational conic instead.
            if to < from {
                Curve::Nurbs(super::nurbs_builder::RationalCurve2::from_curve(&positive)?.reversed().curve()?)
            } else { positive }
        }
        Curve::Ellipse(whole) => {
            let (from, to) = swept(&walk, [0.0, 0.0], |point, _| {
                Curve::Ellipse(whole).parameter_at(*point) * TAU
            });
            let positive = if (to - from).abs() >= TAU - 1e-9 {
                Curve::Ellipse(whole)
            } else {
                Curve::Ellipse(EllipseArc { ellipse: whole.ellipse,
                    start_parameter: from.min(to), end_parameter: from.max(to) })
            };
            if to < from {
                Curve::Nurbs(super::nurbs_builder::RationalCurve2::from_curve(&positive)?.reversed().curve()?)
            } else { positive }
        }
        // A sampled circle covers the whole turn; the edge is the walk.
        Curve::Polyline(_) => Curve::Polyline(Polyline {
            vertices: walk.into_iter().map(PolylineVertex::straight).collect(),
            closed: false,
        }),
        // A spline's projection already spans exactly its own edge.
        other => other,
    })
}

/// Where a walk round a closed curve begins and ends, unwound so the two
/// bound the part actually covered, retaining direction even for full turns.
fn swept(
    walk: &[[f64; 2]],
    centre: [f64; 2],
    angle_of: impl Fn(&[f64; 2], [f64; 2]) -> f64,
) -> (f64, f64) {
    let mut angles = Vec::with_capacity(walk.len());
    let mut last: Option<f64> = None;
    for point in walk {
        let mut angle = angle_of(point, centre);
        if let Some(previous) = last {
            angle = unwound(angle, previous, TAU);
        }
        last = Some(angle);
        angles.push(angle);
    }
    let (from, to) = (angles[0], angles[angles.len() - 1]);
    (from, to)
}

/// `angle` moved by whole turns to sit within half a turn of `previous`.
fn unwound(value: f64, previous: f64, period: f64) -> f64 {
    value + period * ((previous - value) / period).round()
}

/// A line of constant `v`, spanning one full turn of `u`.
///
/// Bounded rather than infinite because `u` on a closed surface is periodic:
/// past a turn it repeats, and a curve that ran on forever would report every
/// crossing an unbounded number of times.
fn band_at(height: f64) -> Curve {
    Curve::Line(Line {
        start: [0.0, height],
        end: [TAU, height],
    })
}

/// The same the other way: constant `u`, a full turn of `v`. Only a torus
/// has one, since it is the only surface here that closes both ways.
fn meridian_at(angle: f64) -> Curve {
    Curve::Line(Line {
        start: [angle, 0.0],
        end: [angle, TAU],
    })
}

/// Which of a surface's parameters wrap.
///
/// `u` runs round every closed surface here; `v` only runs round a torus. A
/// plane's do not wrap at all, and unwinding one there would move the curve
/// rather than place it.
pub(crate) fn periods(surface: &Surface) -> [Option<f64>; 2] {
    match surface {
        Surface::Plane(_) => [None, None],
        Surface::Cylinder(_) | Surface::Cone(_) | Surface::Sphere(_) => [Some(TAU), None],
        Surface::Torus(_) => [Some(TAU), Some(TAU)],
        Surface::Nurbs(surface) | Surface::Offset(crate::OffsetSurface { approximation: surface, .. }) => {
            let ((u0, u1), (v0, v1)) = surface.domain();
            let periodic = surface.periodicity();
            [
                periodic[0].then_some(u1 - u0),
                periodic[1].then_some(v1 - v0),
            ]
        }
    }
}

pub(crate) fn contains_parameter(
    surface: &Surface,
    boundary: &[Curve],
    point: [f64; 2],
    tolerance: ocs_geom::geom2d::Tolerance,
) -> bool {
    let periods = periods(surface);
    let closed = apex_closed(surface, boundary, tolerance);
    let boundary = closed.as_deref().unwrap_or(boundary);
    // A band's rims are not polygons, and a polygon test over them reads a
    // point in one of the band's holes as inside it; counted along `v` the
    // holes and rims all answer alike.
    if let Some(inside) = band_parity(periods, boundary, point, tolerance) {
        return inside;
    }
    let turns = |period: Option<f64>| match period {
        Some(period) => vec![-period, 0.0, period],
        None => vec![0.0],
    };
    // A turn landing off the boundary's box is neither in it nor on it;
    // on a torus that is most of the nine, each a whole polygon test.
    let reach = boundary_box(boundary);
    let near = |point: [f64; 2]| {
        reach.is_none_or(|(low, high)| {
            (0..2).all(|axis| {
                point[axis] >= low[axis] - tolerance.linear()
                    && point[axis] <= high[axis] + tolerance.linear()
            })
        })
    };
    let enclosed = turns(periods[0]).into_iter().any(|u| {
        turns(periods[1]).into_iter().any(|v| {
            let shifted = [point[0] + u, point[1] + v];
            near(shifted) && ocs_geom::geom2d::contains(boundary, shifted, tolerance)
        })
    });
    if enclosed {
        return true;
    }
    // Two full-turn loops bound a band without forming a plane polygon.
    periodic_band_levels(surface, boundary, tolerance.linear()).is_some_and(|levels| {
        point[1] >= levels[0] - tolerance.linear()
            && point[1] <= levels[1] + tolerance.linear()
    })
}

/// A box holding every piece of `boundary`: exact for the analytic kinds,
/// a spline's control points for one, which hold it. `None` when a piece
/// runs on without end.
fn boundary_box(boundary: &[Curve]) -> Option<([f64; 2], [f64; 2])> {
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for curve in boundary {
        let (from, to) = match curve {
            Curve::Nurbs(spline) => {
                spline.control_points().iter().fold(
                    ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
                    |(from, to), point| {
                        (
                            [from[0].min(point[0]), from[1].min(point[1])],
                            [to[0].max(point[0]), to[1].max(point[1])],
                        )
                    },
                )
            }
            other => ocs_geom::geom2d::analytic_curve_bounds(std::slice::from_ref(other))?,
        };
        for axis in 0..2 {
            low[axis] = low[axis].min(from[axis]);
            high[axis] = high[axis].max(to[axis]);
        }
    }
    low.iter().chain(&high).all(|value| value.is_finite()).then_some((low, high))
}

/// [`contains_parameter`] for a face whose loops run as `forward` says: a
/// band round a torus's tube — rims wrapping round `v`, the face running
/// between them along `u` — is one of the two stretches of `u` the rims
/// part, and only which way the rims run tells which.
pub(crate) fn contains_parameter_facing(
    surface: &Surface,
    boundary: &[Curve],
    point: [f64; 2],
    tolerance: ocs_geom::geom2d::Tolerance,
    forward: bool,
) -> bool {
    tube_band_side(periods(surface), boundary, point, tolerance, forward)
        .unwrap_or_else(|| contains_parameter(surface, boundary, point, tolerance))
}

/// Inside a band round a torus's tube: the boundary first met going on
/// along `u` keeps the face on its left in a forward face, so the point is
/// inside when that boundary runs up `v` there. `None` for any other face.
fn tube_band_side(
    periods: [Option<f64>; 2],
    boundary: &[Curve],
    point: [f64; 2],
    tolerance: ocs_geom::geom2d::Tolerance,
    forward: bool,
) -> Option<bool> {
    let [Some(u_period), Some(v_period)] = periods else {
        return None;
    };
    // Loops end where the next piece does not take up; a rim's ends are a
    // turn of `v` apart.
    let joined =
        |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= u_period * 1e-3;
    let mut rims = 0;
    let mut loop_start = 0;
    for (index, curve) in boundary.iter().enumerate() {
        let end = curve.point_at(1.0);
        if boundary.get(index + 1).is_some_and(|next| joined(next.point_at(0.0), end)) {
            continue;
        }
        let start = boundary[loop_start].point_at(0.0);
        let (du, dv) = ((end[0] - start[0]).abs(), (end[1] - start[1]).abs());
        if (dv - v_period).abs() <= v_period * 1e-3 && du <= u_period * 1e-3 {
            rims += 1;
        } else if !joined(start, end) {
            return None;
        }
        loop_start = index + 1;
    }
    if rims < 2 {
        return None;
    }
    let ray = Curve::Line(Line { start: point, end: [point[0] + u_period, point[1]] });
    let mut nearest: Option<(f64, bool)> = None;
    for curve in boundary {
        // A piece's box from samples, grown for what bulges between them:
        // only the turns that bring it across the ray are tried.
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for sample in (0..=16).map(|step| curve.point_at(step as f64 / 16.0)) {
            for axis in 0..2 {
                low[axis] = low[axis].min(sample[axis]);
                high[axis] = high[axis].max(sample[axis]);
            }
        }
        let pad = (high[0] - low[0]).hypot(high[1] - low[1]) * 0.05 + u_period * 1e-3;
        for u_turn in -2..=2 {
            let shift_u = u_turn as f64 * u_period;
            if high[0] + shift_u < point[0] - pad || low[0] + shift_u > point[0] + u_period + pad {
                continue;
            }
            // The point may come a turn or two of `v` away itself — the
            // callers try its periodic images — and a rim missing a copy
            // there lets one farther along answer for it.
            for v_turn in -3..=3 {
                let shift = [shift_u, v_turn as f64 * v_period];
                if high[1] + shift[1] < point[1] - pad || low[1] + shift[1] > point[1] + pad {
                    continue;
                }
                let moved = |at: f64| {
                    let p = curve.point_at(at.clamp(0.0, 1.0));
                    [p[0] + shift[0], p[1] + shift[1]]
                };
                let by = ocs_geom::geom2d::Transform::translation(shift);
                let Some(image) = curve.transformed(&by) else { continue };
                for crossing in ocs_geom::geom2d::intersect(&ray, &image, tolerance) {
                    let along = crossing.point[0] - point[0];
                    if along <= tolerance.linear() {
                        continue;
                    }
                    let step = 1e-4;
                    let up = moved(crossing.t_b + step)[1] - moved(crossing.t_b - step)[1];
                    if up == 0.0 {
                        return None;
                    }
                    if nearest.is_none_or(|(seen, _)| along < seen) {
                        nearest = Some((along, up > 0.0));
                    }
                }
            }
        }
    }
    nearest.map(|(_, up)| up == forward)
}

/// A cone face running up to its apex along a seam has no edge there: in
/// `(u, v)` its loop stops at one end of the apex line and goes on from the
/// other, an open chain no polygon test reads. The apex line closes it.
/// `None` when nothing needed closing.
pub(super) fn apex_closed(
    surface: &Surface,
    boundary: &[Curve],
    tolerance: ocs_geom::geom2d::Tolerance,
) -> Option<Vec<Curve>> {
    // The latitudes where the surface closes to a point: a cone's apex, a
    // sphere's poles — a sphere face reaching a pole stops there the same
    // way.
    let apexes: Vec<f64> = match surface {
        Surface::Cone(cone) => vec![cone.radius / cone.half_angle.tan()],
        Surface::Sphere(_) => vec![std::f64::consts::FRAC_PI_2, -std::f64::consts::FRAC_PI_2],
        _ => return None,
    };
    if apexes.iter().any(|apex| !apex.is_finite()) {
        return None;
    }
    let slack = |apex: f64| tolerance.linear().max(apex.abs() * 1e-9);
    let pole = |point: [f64; 2]| {
        apexes.iter().position(|apex| (point[1] - apex).abs() <= slack(*apex))
    };
    let same_apex = |a: [f64; 2], b: [f64; 2]| pole(a).is_some() && pole(a) == pole(b);
    let joined = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= TAU * 1e-3;
    let line = |start: [f64; 2], end: [f64; 2]| Curve::Line(Line { start, end });
    let mut out = Vec::with_capacity(boundary.len() + 3);
    let mut changed = false;
    let mut loop_start = 0;
    // Where a rim running once round ends, and from where it started.
    let mut wrapping = Vec::new();
    for (index, curve) in boundary.iter().enumerate() {
        out.push(curve.clone());
        let end = curve.point_at(1.0);
        match boundary.get(index + 1).map(|next| next.point_at(0.0)) {
            Some(next) if joined(next, end) => continue,
            Some(next) if same_apex(end, next) => {
                out.push(line(end, next));
                changed = true;
                continue;
            }
            _ => {}
        }
        let start = boundary[loop_start].point_at(0.0);
        if !joined(start, end) && same_apex(start, end) {
            out.push(line(end, start));
            changed = true;
        } else if ((end[0] - start[0]).abs() - TAU).abs() <= TAU * 1e-3
            && (end[1] - start[1]).abs() <= TAU * 1e-3
        {
            wrapping.push((out.len(), start, end));
        }
        loop_start = index + 1;
    }
    // A face with one rim round the cone and nothing else — its apex loop
    // has no curve — is everything from the rim up to the apex.
    if let ([(at, start, end)], [apex]) = (&wrapping[..], &apexes[..]) {
        let (at, start, end, apex) = (*at, *start, *end, *apex);
        let (over_end, over_start) = ([end[0], apex], [start[0], apex]);
        let closing = [line(end, over_end), line(over_end, over_start), line(over_start, start)];
        out.splice(at..at, closing);
        changed = true;
    }
    changed.then_some(out)
}

/// Whether a point is inside a band — a face whose rims wrap round the
/// surface rather than close, holes and notches and all. A line from the
/// point along `v`, past every rim, crosses the boundary an odd number of
/// times from inside; the boundary covers one turn, so the line is tried at
/// every turn it spans. `None` for a face that is not a band.
fn band_parity(
    periods: [Option<f64>; 2],
    boundary: &[Curve],
    point: [f64; 2],
    tolerance: ocs_geom::geom2d::Tolerance,
) -> Option<bool> {
    // A torus closes along `v` as well; a band round it that stays within
    // one turn of `v` is a band all the same.
    let ([Some(period), v_period], false) = (periods, boundary.is_empty()) else {
        return None;
    };
    // A band has its rims wrap round, each a turn along, in pairs; with no
    // wrapping loop the face is the polygon test's. One wrapping loop alone
    // — a cone's rim, its face running up to the apex — bounds a side only
    // the apex decides, not a band.
    let near =
        |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= tolerance.linear();
    // The boundary lists each loop's pieces in order; a loop ends where the
    // next piece does not take up from it. Pieces walked from a spline meet
    // only to the walk's accuracy, and a wrap leaves a whole turn between
    // the loop's two ends.
    let joined =
        |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= period * 1e-3;
    let mut wrapping = 0;
    let mut loop_start = 0;
    for (index, curve) in boundary.iter().enumerate() {
        let end = curve.point_at(1.0);
        let continues = boundary
            .get(index + 1)
            .is_some_and(|next| joined(next.point_at(0.0), end));
        if !continues {
            if !joined(boundary[loop_start].point_at(0.0), end) {
                wrapping += 1;
            }
            loop_start = index + 1;
        }
    }
    if wrapping < 2 || wrapping % 2 == 1 {
        return None;
    }
    let samples: Vec<[f64; 2]> = boundary
        .iter()
        .flat_map(|curve| (0..=16).map(move |step| curve.point_at(step as f64 / 16.0)))
        .collect();
    let (low, high, top) = samples.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        |(low, high, top), sample| {
            (low.min(sample[0]), high.max(sample[0]), top.max(sample[1]))
        },
    );
    let mut point = point;
    if let Some(v_period) = v_period {
        let bottom = samples.iter().map(|sample| sample[1]).fold(f64::INFINITY, f64::min);
        if top - bottom >= v_period * (1.0 - 1e-6) {
            return None;
        }
        // The point's turn of `v` nearest the band's.
        let middle = 0.5 * (bottom + top);
        point[1] += v_period * ((middle - point[1]) / v_period).round();
    }
    if point[1] >= top {
        return Some(false);
    }
    let top = top + (top - point[1]) + 1.0;
    let first = ((low - point[0]) / period).floor() as i64 - 1;
    let last = ((high - point[0]) / period).ceil() as i64 + 1;
    // Each piece's box, so the line is only tried against pieces it can meet.
    let boxes: Vec<[[f64; 2]; 2]> = boundary
        .iter()
        .map(|curve| {
            let points: Vec<[f64; 2]> = match curve {
                Curve::Polyline(polyline)
                    if polyline.vertices.iter().all(|vertex| vertex.bulge == 0.0) =>
                {
                    polyline.vertices.iter().map(|vertex| vertex.position).collect()
                }
                _ => (0..=16).map(|step| curve.point_at(step as f64 / 16.0)).collect(),
            };
            let mut low = [f64::INFINITY; 2];
            let mut high = [f64::NEG_INFINITY; 2];
            for point in &points {
                for axis in 0..2 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
            let pad = (high[0] - low[0]).hypot(high[1] - low[1]) * 0.05 + period * 1e-3;
            [[low[0] - pad, low[1] - pad], [high[0] + pad, high[1] + pad]]
        })
        .collect();
    let mut crossings: Vec<[f64; 2]> = Vec::new();
    for turn in first..=last {
        let u = point[0] + turn as f64 * period;
        // Slanted a little, so the line never runs along an edge of
        // constant `u` — a generator, a slot's side.
        let ray = Curve::Line(Line {
            start: [u, point[1]],
            end: [u + period * 1e-3, top],
        });
        for (curve, bounds) in boundary.iter().zip(&boxes) {
            if u < bounds[0][0] || u > bounds[1][0] || point[1] > bounds[1][1] {
                continue;
            }
            for crossing in ocs_geom::geom2d::intersect(&ray, curve, tolerance) {
                // Where two boundary pieces meet, both report the joint —
                // a turn apart, where a wrapping loop comes round.
                let turned = [crossing.point[0].rem_euclid(period), crossing.point[1]];
                if !crossings.iter().any(|seen| {
                    near(*seen, turned)
                        || near([seen[0] + period, seen[1]], turned)
                        || near([seen[0] - period, seen[1]], turned)
                }) {
                    crossings.push(turned);
                }
            }
        }
    }
    Some(crossings.len() % 2 == 1)
}

/// An interior point between two full-turn boundary loops.
pub(crate) fn periodic_band_point(
    surface: &Surface,
    boundary: &[Curve],
    tolerance: f64,
) -> Option<[f64; 3]> {
    let levels = periodic_band_levels(surface, boundary, tolerance)?;
    let u = boundary.first()?.point_at(0.5)[0];
    Some(surface.point_at(u, 0.5 * (levels[0] + levels[1])))
}

fn periodic_band_levels(
    surface: &Surface,
    boundary: &[Curve],
    tolerance: f64,
) -> Option<[f64; 2]> {
    let period = periods(surface)[0]?;
    let mut levels: Vec<(f64, f64)> = Vec::new();
    for curve in boundary {
        // Any curve that holds one `v` — a line, or a spline rim on a
        // spline band.
        let (start, end) = (curve.point_at(0.0), curve.point_at(1.0));
        if (1..8).any(|step| (curve.point_at(step as f64 / 8.0)[1] - start[1]).abs() > tolerance)
            || (end[1] - start[1]).abs() > tolerance
        {
            return None;
        }
        let level = 0.5 * (start[1] + end[1]);
        let span = (end[0] - start[0]).abs();
        if let Some((_, covered)) = levels
            .iter_mut()
            .find(|(existing, _)| (*existing - level).abs() <= tolerance)
        {
            *covered += span;
        } else {
            levels.push((level, span));
        }
    }
    // Coverage is an angle, the tolerance a length: on a small cylinder the
    // pieces of a cut rim meet a few nano-radians apart and still close.
    let slack = tolerance.max(period * 1e-6);
    if levels.len() != 2
        || levels
            .iter()
            .any(|(_, covered)| *covered < period - slack)
    {
        return None;
    }
    levels.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some([levels[0].0, levels[1].0])
}

/// A line of constant `u`, unbounded in `v` — a generator, which the face's
/// own extent trims.
fn generator_at(angle: f64) -> Curve {
    Curve::XLine(XLine {
        base: [angle, 0.0],
        direction: [0.0, 1.0],
    })
}

/// Where `point` sits around a frame's axis, in radians from its x axis.
fn angle_about(frame: &ocs_geom::space::Plane, point: [f64; 3]) -> Option<f64> {
    let local = frame.project(point)?;
    Some(local[1].atan2(local[0]))
}

/// Whether every sampled point of the curve is on the surface.
fn lies_on(surface: &Surface, curve: &Curve3, tolerance: f64) -> bool {
    // Spread over a range wide enough to catch a curve that touches the
    // surface but does not follow it — a line crossing a sphere is on it at
    // two parameters and nowhere else.
    const SAMPLES: usize = 9;
    let points: Vec<[f64; 3]> = (0..SAMPLES)
        .map(|index| {
            let t = match curve {
                Curve3::Line(_) => -2.0 + 4.0 * index as f64 / (SAMPLES - 1) as f64,
                Curve3::PlanarSpline { .. } => index as f64 / (SAMPLES - 1) as f64,
                Curve3::Nurbs(spline) => {
                    let (start, end) = spline.domain();
                    start + (end - start) * index as f64 / (SAMPLES - 1) as f64
                }
                _ => TAU * index as f64 / SAMPLES as f64,
            };
            curve.point_at(t)
        })
        .collect();
    let allowance = fit_allowance(surface, curve, &points, tolerance);
    points.into_iter().all(|point| surface.distance_to(point).abs() <= allowance)
}

/// How far a curve may stand off a surface and still be on it. A spline in a
/// file is a fit to the true intersection or surface, within the fit's own
/// tolerance — up to a thousandth of the curve, many times the
/// modelling tolerance — so a spline on either side is held to that.
fn fit_allowance(
    surface: &Surface,
    curve: &Curve3,
    points: &[[f64; 3]],
    tolerance: f64,
) -> f64 {
    if !matches!(curve, Curve3::Nurbs(_)) && !surface.is_spline() {
        return tolerance;
    }
    let first = Vec3::from(points[0]);
    let extent = points
        .iter()
        .map(|point| Vec3::from(*point).distance(first))
        .fold(0.0, f64::max);
    tolerance.max(extent * 1e-3)
}

/// Whether an edge's own span lies on a surface. A spline patch ends where
/// the face does, so the curve beyond the edge's ends is no evidence.
fn span_on(surface: &Surface, curve: &Curve3, span: (f64, f64), tolerance: f64) -> bool {
    const SAMPLES: usize = 9;
    let points: Vec<[f64; 3]> = (0..SAMPLES)
        .map(|index| {
            curve.point_at(span.0 + (span.1 - span.0) * index as f64 / (SAMPLES - 1) as f64)
        })
        .collect();
    let allowance = fit_allowance(surface, curve, &points, tolerance);
    points.into_iter().all(|point| surface.distance_to(point).abs() <= allowance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Circle3, Cone, Cylinder, Ellipse3, Line3, Sphere};
    use ocs_geom::geom2d::{NurbsCurve, Vec2};
    use ocs_geom::space::Plane;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    const TOL: f64 = 1e-9;

    fn plane_at(origin: [f64; 3], normal: [f64; 3]) -> Plane {
        let seed = if normal[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        Plane::orthonormal(origin, seed, normal).unwrap()
    }

    /// Every point of the projected curve, lifted back through the surface,
    /// must land on the space curve. That round trip is the whole contract.
    fn assert_round_trips(surface: &Surface, curve: &Curve3, flat: &Curve, span: (f64, f64)) {
        for index in 0..=10 {
            let t = span.0 + (span.1 - span.0) * index as f64 / 10.0;
            let uv = flat.point_at(t);
            let lifted = surface.point_at(uv[0], uv[1]);
            // The lifted point has to be *on* the space curve, which is
            // asked by inverting the curve and evaluating it back.
            let back = curve.point_at(curve.parameter_at(lifted));
            assert!(
                Vec3::from(lifted).distance(Vec3::from(back)) < 1e-6,
                "t={t}: {lifted:?} is not on the curve ({back:?})"
            );
        }
    }

    #[test]
    fn a_line_on_a_plane_projects_to_a_line() {
        let plane = plane_at([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]);
        let surface = Surface::Plane(plane);
        let curve = Curve3::Line(Line3 {
            origin: [1.0, 2.0, 5.0],
            direction: [3.0, 4.0, 0.0],
        });
        let flat = project(&surface, &curve, TOL).expect("a line on its own plane");
        assert_round_trips(&surface, &curve, &flat, (-2.0, 2.0));
        // And the parameter happens to carry across for a straight curve,
        // since both are `base + t · direction`.
        assert_eq!(flat.point_at(1.0), [4.0, 6.0]);
    }

    #[test]
    fn a_circle_on_a_plane_projects_to_a_circle() {
        let plane = plane_at([0.0; 3], [0.0, 0.0, 1.0]);
        let surface = Surface::Plane(plane);
        let curve = Curve3::Circle(Circle3 {
            plane: plane_at([2.0, 3.0, 0.0], [0.0, 0.0, 1.0]),
            radius: 4.0,
        });
        let flat = project(&surface, &curve, TOL).expect("a circle on its own plane");
        let Curve::Circle(circle) = &flat else {
            panic!("expected a circle, got {flat:?}");
        };
        assert!((circle.radius - 4.0).abs() < 1e-12);
        assert_round_trips(&surface, &curve, &flat, (0.0, 1.0));
    }

    #[test]
    fn an_ellipse_on_a_plane_keeps_both_its_radii_and_its_direction() {
        let plane = plane_at([0.0; 3], [0.0, 0.0, 1.0]);
        let surface = Surface::Plane(plane);
        // Major axis along +Y, so a projection that assumed +X would be a
        // quarter turn out.
        let frame = Plane::from_axes([1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]);
        let curve = Curve3::Ellipse(Ellipse3 {
            plane: frame,
            major_radius: 6.0,
            minor_radius: 2.0,
        });
        let flat = project(&surface, &curve, TOL).expect("an ellipse on its own plane");
        let Curve::Ellipse(arc) = &flat else {
            panic!("expected an ellipse, got {flat:?}");
        };
        assert!((arc.ellipse.major_radius - 6.0).abs() < 1e-12);
        assert!((arc.ellipse.major_axis[1] - 1.0).abs() < 1e-12, "{:?}", arc.ellipse.major_axis);
        assert_round_trips(&surface, &curve, &flat, (0.0, 1.0));
    }

    #[test]
    fn a_curve_off_the_surface_has_no_projection() {
        // The failure this guards: projecting it anyway gives a shape of the
        // right kind sitting in the wrong place, which validates and is
        // wrong.
        let surface = Surface::Plane(plane_at([0.0; 3], [0.0, 0.0, 1.0]));
        let above = Curve3::Line(Line3 {
            origin: [0.0, 0.0, 4.0],
            direction: [1.0, 0.0, 0.0],
        });
        assert!(project(&surface, &above, TOL).is_none());
        // One that merely crosses the plane is not on it either.
        let through = Curve3::Line(Line3 {
            origin: [0.0, 0.0, -1.0],
            direction: [0.0, 0.0, 1.0],
        });
        assert!(project(&surface, &through, TOL).is_none());
    }

    #[test]
    fn a_circle_round_a_cylinder_becomes_a_straight_band() {
        // The point of parameter space: a circle is a straight line there.
        let cylinder = Cylinder {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 3.0,
        };
        let surface = Surface::Cylinder(cylinder);
        let curve = Curve3::Circle(Circle3 {
            plane: plane_at([0.0, 0.0, 7.0], [0.0, 0.0, 1.0]),
            radius: 3.0,
        });
        let flat = project(&surface, &curve, TOL).expect("a circle round its own cylinder");
        let Curve::Line(line) = &flat else {
            panic!("expected a line in (u, v), got {flat:?}");
        };
        assert!((line.start[1] - 7.0).abs() < 1e-12, "the height is v");
        assert!((line.end[0] - TAU).abs() < 1e-12, "one full turn of u");
        assert_round_trips(&surface, &curve, &flat, (0.0, 1.0));
    }

    #[test]
    fn a_generator_on_a_cylinder_becomes_a_line_of_constant_angle() {
        let cylinder = Cylinder {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 3.0,
        };
        let surface = Surface::Cylinder(cylinder);
        // The generator at ninety degrees round.
        let curve = Curve3::Line(Line3 {
            origin: [0.0, 3.0, 0.0],
            direction: [0.0, 0.0, 1.0],
        });
        let flat = project(&surface, &curve, TOL).expect("a generator on its own cylinder");
        let Curve::XLine(line) = &flat else {
            panic!("expected an infinite line, got {flat:?}");
        };
        assert!((line.base[0] - FRAC_PI_2).abs() < 1e-9, "{:?}", line.base);
        assert_round_trips(&surface, &curve, &flat, (-3.0, 3.0));
    }

    #[test]
    fn a_slanted_section_of_a_cylinder_has_no_written_form() {
        // It is a sine wave in (u, v). Calling it a spline would be an
        // approximation presented as a fact.
        let surface = Surface::Cylinder(Cylinder {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 3.0,
        });
        let slanted = Curve3::Ellipse(Ellipse3 {
            plane: plane_at([0.0; 3], [0.0, 1.0, 1.0]),
            major_radius: 3.0 * std::f64::consts::SQRT_2,
            minor_radius: 3.0,
        });
        assert!(project(&surface, &slanted, TOL).is_none());
    }

    #[test]
    fn a_circle_round_a_cone_becomes_a_band_at_its_height() {
        let cone = Cone {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 10.0,
            half_angle: FRAC_PI_4,
        };
        let surface = Surface::Cone(cone);
        let curve = Curve3::Circle(Circle3 {
            plane: plane_at([0.0, 0.0, 4.0], [0.0, 0.0, 1.0]),
            radius: 6.0,
        });
        let flat = project(&surface, &curve, TOL).expect("a circle round its own cone");
        let Curve::Line(line) = &flat else {
            panic!("expected a line, got {flat:?}");
        };
        assert!((line.start[1] - 4.0).abs() < 1e-12);
        assert_round_trips(&surface, &curve, &flat, (0.0, 1.0));
    }

    #[test]
    fn a_generator_on_a_cone_leans_and_is_still_one_angle() {
        // Unlike a cylinder's, a cone's generator is not parallel to the
        // axis, so a parallel test would reject it. What identifies it is
        // holding one angle the whole way.
        let cone = Cone {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 10.0,
            half_angle: FRAC_PI_4,
        };
        let surface = Surface::Cone(cone);
        let curve = Curve3::Line(Line3 {
            origin: [10.0, 0.0, 0.0],
            direction: [-1.0, 0.0, 1.0],
        });
        let flat = project(&surface, &curve, 1e-6).expect("a generator on its own cone");
        let Curve::XLine(line) = &flat else {
            panic!("expected an infinite line, got {flat:?}");
        };
        assert!(line.base[0].abs() < 1e-9, "at angle zero: {:?}", line.base);
    }

    #[test]
    fn a_spline_on_its_own_plane_carries_over_whole() {
        let frame = plane_at([0.0; 3], [0.0, 0.0, 1.0]);
        let surface = Surface::Plane(frame);
        let curve = Curve3::PlanarSpline {
            plane: frame,
            curve: NurbsCurve::new(
                3,
                vec![[0.0, 0.0], [1.0, 5.0], [4.0, 5.0], [5.0, 0.0]],
                Vec::new(),
                None,
            )
            .unwrap(),
        };
        let flat = project(&surface, &curve, TOL).expect("a spline on its own plane");
        assert!(matches!(flat, Curve::Nurbs(_)));
        assert_round_trips(&surface, &curve, &flat, (0.0, 1.0));
    }

    #[test]
    fn a_spheres_equator_becomes_a_straight_band() {
        let surface = Surface::Sphere(Sphere {
            frame: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let equator = Curve3::Circle(Circle3 {
            plane: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let flat = project(&surface, &equator, TOL).expect("the equator on its own sphere");
        let Curve::Line(line) = &flat else {
            panic!("expected a line in (u, v), got {flat:?}");
        };
        assert!(line.start[1].abs() < 1e-12, "the latitude is zero");
        assert!((line.end[0] - TAU).abs() < 1e-12, "one full longitude turn");
        assert_round_trips(&surface, &equator, &flat, (0.0, 1.0));
    }

    #[test]
    fn a_face_of_a_box_projects_its_own_boundary() {
        // The case the boolean will actually meet first: a planar face and
        // the straight edges that bound it.
        let body = crate::make::cuboid([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
        let mut projected = 0;
        for face in body.face_keys() {
            let surface = body
                .surfaces
                .get(body.faces.get(face).unwrap().surface)
                .unwrap();
            for coedge in body.face_coedges(face) {
                let edge = body.edges.get(body.coedges.get(coedge).unwrap().edge).unwrap();
                let curve = body.curves.get(edge.curve).unwrap();
                let flat = project(surface, curve, 1e-9)
                    .expect("a box's edge lies on the faces it bounds");
                assert_round_trips(surface, curve, &flat, (0.0, 1.0));
                projected += 1;
            }
        }
        assert_eq!(projected, 24, "four edges on each of six faces");
    }

    #[test]
    fn survey_coordinates_project_to_the_same_place() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let plane = plane_at(origin, [0.0, 0.0, 1.0]);
        let surface = Surface::Plane(plane);
        let curve = Curve3::Circle(Circle3 {
            plane: plane_at([origin[0] + 1.0, origin[1] + 2.0, origin[2]], [0.0, 0.0, 1.0]),
            radius: 4.0,
        });
        let flat = project(&surface, &curve, 1e-6).expect("still on its plane");
        let Curve::Circle(circle) = &flat else {
            unreachable!()
        };
        assert!((circle.centre[0] - 1.0).abs() < 1e-6, "{:?}", circle.centre);
        assert!((circle.radius - 4.0).abs() < 1e-9);
    }

    #[test]
    fn clockwise_circle_spans_keep_their_direction() {
        let plane = plane_at([0.0; 3], [0.0, 0.0, 1.0]);
        let surface = Surface::Plane(plane);
        let curve = Curve3::Circle(Circle3 {
            plane,
            radius: 2.0,
        });
        let flat = Curve::Circle(ocs_geom::geom2d::Circle {
            centre: [0.0, 0.0],
            radius: 2.0,
        });

        let half = trim_to(&surface, &curve, (std::f64::consts::PI, 0.0), flat.clone())
            .expect("a clockwise half circle has an exact projection");
        assert!(matches!(half, Curve::Nurbs(_)));
        assert!(Vec2::from(half.point_at(0.0)).distance(Vec2::from([-2.0, 0.0])) < TOL);
        assert!(Vec2::from(half.point_at(0.5)).distance(Vec2::from([0.0, 2.0])) < TOL);
        assert!(Vec2::from(half.point_at(1.0)).distance(Vec2::from([2.0, 0.0])) < TOL);

        let whole = trim_to(&surface, &curve, (0.0, -TAU), flat)
            .expect("a clockwise full circle has an exact projection");
        assert!(matches!(whole, Curve::Nurbs(_)));
        assert!(Vec2::from(whole.point_at(0.25)).distance(Vec2::from([0.0, -2.0])) < TOL);
    }

    #[test]
    fn a_band_round_a_torus_tube_is_the_stretch_its_rims_keep() {
        // Rims round the tube at u = -π (going up v) and u = 2.4 (going
        // down): the forward face is u from 2.4 on to π, not back to -π.
        let line = |start, end| Curve::Line(Line { start, end });
        let boundary = [line([-PI, -1.0], [-PI, TAU - 1.0]), line([2.4, 0.0], [2.4, -TAU])];
        let periods = [Some(TAU), Some(TAU)];
        let tolerance = ocs_geom::geom2d::Tolerance::new(1e-9);
        let side =
            |u: f64, forward| tube_band_side(periods, &boundary, [u, 0.3], tolerance, forward);
        assert_eq!(side(1.6, true), Some(false));
        assert_eq!(side(2.9, true), Some(true));
        assert_eq!(side(1.6, false), Some(true));
    }
}
