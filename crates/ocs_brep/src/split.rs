//! Cutting topology apart without breaking it.
//!
//! Every modelling operation that adds detail ends here: a boolean splits the
//! faces its intersection curves cross, a fillet splits the edges it runs
//! along, and an imprint splits both. What they have in common is that the
//! result has to be as consistent as the input — a face split into two halves
//! that do not quite share their new edge is worse than one not split at all,
//! because it looks finished.
//!
//! So each operation here leaves [`Body::validate`] finding nothing, and the
//! tests check that rather than checking the pieces individually.
//!
//! # The part that is easy to forget
//!
//! An edge belongs to two faces. Splitting it changes the loop of the face
//! being worked on *and* the loop of the one on the other side, which is not
//! mentioned anywhere in the operation's own description and is exactly what
//! a first implementation misses. The far face's loop is then one coedge
//! short, its ring no longer closes, and the failure surfaces later as a
//! boolean that loses a wall.

use super::geometry::{Circle3, Curve3, Ellipse3};
use super::pcurve;
use super::topology::{
    Body, Coedge, CoedgeKey, Edge, EdgeKey, Face, FaceKey, Loop, LoopKey, Vertex, VertexKey,
};
use super::Provenance;
use ocs_geom::geom2d::{distance_to, intersect as cross, Tolerance, Transform};
use ocs_geom::space::Vec3;
use std::f64::consts::TAU;

/// Splits an edge at a parameter along its curve, returning the two halves.
///
/// The first keeps the original key, so anything already pointing at the edge
/// still names its first half. The second is new.
///
/// Every coedge that used the edge is split in step, in whichever face it
/// belongs to — including the face on the far side, whose loop is just as
/// much a part of this operation as the near one's.
///
/// `None` when the parameter is at or outside the edge's own span: there is
/// nothing to divide, and creating a zero-length piece would leave topology
/// no later operation can make sense of.
pub fn split_edge(body: &mut Body, edge: EdgeKey, parameter: f64) -> Option<(EdgeKey, EdgeKey)> {
    let original = body.edges.get(edge)?.clone();
    let span = original.end_parameter - original.start_parameter;
    if span == 0.0 {
        return None;
    }
    // Measured as a fraction of the span so the guard means the same thing on
    // an edge parameterised over a unit and one parameterised over a turn.
    let across = (parameter - original.start_parameter) / span;
    if !(1e-9..=1.0 - 1e-9).contains(&across) {
        return None;
    }

    // Resolve every pcurve before changing the shared topology.
    let point = body.curves.get(original.curve)?.point_at(parameter);
    let mut coedges = Vec::with_capacity(original.coedges.len());
    for key in &original.coedges {
        let existing = body.coedges.get(*key)?.clone();
        let (near_pcurve, far_pcurve) = match existing.pcurve.as_ref() {
            Some(curve) => {
                let guess = if existing.forward { across } else { 1.0 - across };
                let surface = body
                    .loops
                    .get(existing.owner)
                    .and_then(|ring| body.faces.get(ring.owner))
                    .and_then(|face| body.surfaces.get(face.surface));
                let at = surface.map_or(guess, |surface| pcurve_at(surface, curve, point, guess));
                let (first, second) = split_pcurve(curve, at)?;
                if existing.forward {
                    (Some(first), Some(second))
                } else {
                    (Some(second), Some(first))
                }
            }
            None => (None, None),
        };
        body.loops
            .get(existing.owner)?
            .coedges
            .iter()
            .position(|candidate| candidate == key)?;
        coedges.push((*key, existing, near_pcurve, far_pcurve));
    }

    let middle = body.vertices.insert(Vertex {
        point,
        provenance: Provenance::Synthesized,
    });

    let far = body.edges.insert(Edge {
        curve: original.curve,
        start_parameter: parameter,
        end_parameter: original.end_parameter,
        start: middle,
        end: original.end,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    {
        let near = body.edges.get_mut(edge)?;
        near.end_parameter = parameter;
        near.end = middle;
        near.provenance.soil();
    }

    for (coedge, existing, near_pcurve, far_pcurve) in coedges {
        let twin = body.coedges.insert(Coedge {
            edge: far,
            forward: existing.forward,
            pcurve: far_pcurve,
            owner: existing.owner,
            provenance: Provenance::Synthesized,
        });
        body.edges.get_mut(far)?.coedges.push(twin);

        let ring = body.loops.get_mut(existing.owner)?;
        let at = ring.coedges.iter().position(|key| *key == coedge)?;
        // A coedge running with the curve meets the near half first, so the
        // far half follows it. One running against the curve meets the far
        // half first, so the new coedge goes in ahead of it.
        ring.coedges.insert(if existing.forward { at + 1 } else { at }, twin);
        ring.provenance.soil();

        let face = ring.owner;
        if let Some(face) = body.faces.get_mut(face) {
            face.provenance.soil();
        }
        if let Some(coedge) = body.coedges.get_mut(coedge) {
            coedge.pcurve = near_pcurve;
            coedge.provenance.soil();
        }
    }

    Some((edge, far))
}

/// Where on a pcurve the surface passes through `point`. Its parameter need
/// not keep pace with its edge's — a circle rim on a spline surface runs at
/// the surface's own rate — so the edge's fraction is only where to start.
fn pcurve_at(
    surface: &super::geometry::Surface,
    pcurve: &ocs_geom::geom2d::Curve,
    point: [f64; 3],
    guess: f64,
) -> f64 {
    let gap = |t: f64| {
        let [u, v] = pcurve.point_at(t);
        Vec3::from(surface.point_at(u, v)).distance(Vec3::from(point))
    };
    const SAMPLES: usize = 64;
    let best = (0..=SAMPLES)
        .map(|step| step as f64 / SAMPLES as f64)
        .chain(std::iter::once(guess))
        .min_by(|a, b| gap(*a).total_cmp(&gap(*b)))
        .unwrap_or(guess);
    let step = 1.0 / SAMPLES as f64;
    let (mut low, mut high) = ((best - step).max(0.0), (best + step).min(1.0));
    for _ in 0..60 {
        let third = (high - low) / 3.0;
        if gap(low + third) < gap(high - third) {
            high -= third;
        } else {
            low += third;
        }
    }
    let found = 0.5 * (low + high);
    if gap(found) <= gap(guess) { found } else { guess }
}

fn split_pcurve(
    curve: &ocs_geom::geom2d::Curve,
    at: f64,
) -> Option<(ocs_geom::geom2d::Curve, ocs_geom::geom2d::Curve)> {
    use ocs_geom::geom2d::{Arc, Curve, EllipseArc, Line};
    Some(match curve {
        Curve::Line(line) => {
            let middle = [
                line.start[0] + (line.end[0] - line.start[0]) * at,
                line.start[1] + (line.end[1] - line.start[1]) * at,
            ];
            (
                Curve::Line(Line { start: line.start, end: middle }),
                Curve::Line(Line { start: middle, end: line.end }),
            )
        }
        Curve::Circle(circle) => {
            let middle = TAU * at;
            (
                Curve::Arc(Arc {
                    centre: circle.centre,
                    radius: circle.radius,
                    start_angle: 0.0,
                    end_angle: middle,
                }),
                Curve::Arc(Arc {
                    centre: circle.centre,
                    radius: circle.radius,
                    start_angle: middle,
                    end_angle: TAU,
                }),
            )
        }
        Curve::Arc(arc) => {
            let end = arc.start_angle + arc.sweep();
            let middle = arc.start_angle + arc.sweep() * at;
            (
                Curve::Arc(Arc {
                    end_angle: middle,
                    ..*arc
                }),
                Curve::Arc(Arc {
                    start_angle: middle,
                    end_angle: end,
                    ..*arc
                }),
            )
        }
        Curve::Ellipse(arc) => {
            let end = arc.start_parameter + arc.sweep();
            let middle = arc.start_parameter + arc.sweep() * at;
            (
                Curve::Ellipse(EllipseArc {
                    end_parameter: middle,
                    ..*arc
                }),
                Curve::Ellipse(EllipseArc {
                    start_parameter: middle,
                    end_parameter: end,
                    ..*arc
                }),
            )
        }
        Curve::Nurbs(curve) => {
            let (first, second) = curve.split_at(at)?;
            (Curve::Nurbs(first), Curve::Nurbs(second))
        }
        _ => return None,
    })
}

/// Splits an edge at the point on it nearest `point`.
///
/// The form a caller with an intersection result has: it knows where the
/// curves met, not what parameter that was.
pub fn split_edge_at(body: &mut Body, edge: EdgeKey, point: [f64; 3]) -> Option<(EdgeKey, EdgeKey)> {
    let parameter = {
        let node = body.edges.get(edge)?;
        parameter_in_span(
            body.curves.get(node.curve)?,
            point,
            node.start_parameter,
            node.end_parameter,
        )
    };
    split_edge(body, edge, parameter)
}


/// Cuts a face in two along `cutter`, returning both halves.
///
/// The first keeps the original key. Both lie on the same surface — a cut
/// divides a face without moving it — and both belong to the same shell.
///
/// Handles boundary crossings, closed interior cuts, and closed sections of
/// periodic faces. Returns `None` when the cut cannot be represented exactly.
/// Crossings are solved in the surface's parameter space.
pub fn split_face(
    body: &mut Body,
    face: FaceKey,
    cutter: &Curve3,
    tolerance: f64,
) -> Option<[FaceKey; 2]> {
    // Most tries cut nothing and leave the body as it was; it is copied
    // only once a cut starts changing it.
    let mut candidate = std::borrow::Cow::Borrowed(&*body);
    let result = split_face_in_place(&mut candidate, face, cutter, tolerance)?;
    let candidate = candidate.into_owned();
    let before = planar_area(body, face);
    // On a plane the halves share out the face's area. A cut bridging a hole
    // to the rim divides nothing and hands the one face back twice.
    if let (true, Some(whole), Some(kept), Some(made)) = (
        result[0] != result[1],
        before,
        planar_area(&candidate, result[0]),
        planar_area(&candidate, result[1]),
    ) {
        // Each half on the face's own side, too: a half walked inside out
        // carries the region negated, and the other half covers it twice.
        let scale = whole.abs() + kept.abs() + made.abs();
        if (whole - kept - made).abs() > 1e-3 * scale
            || kept * whole <= 0.0
            || made * whole <= 0.0
        {
            return None;
        }
    }
    // A cut along a circle the face is already bounded by — the equator of a
    // torus a plane runs through, met again on the next round — divides
    // nothing: it hands back a lens between two arcs of that one circle.
    // A disc cut from inside a face that had no edge on the circle is real.
    let on_circle = |body: &Body, face: FaceKey| {
        let coedges = body.face_coedges(face);
        let on = |coedge: &CoedgeKey| {
            body.coedges
                .get(*coedge)
                .and_then(|coedge| body.edges.get(coedge.edge))
                .and_then(|edge| body.curves.get(edge.curve))
                .is_some_and(|curve| same_circle(curve, cutter, tolerance))
        };
        (coedges.iter().any(on), !coedges.is_empty() && coedges.iter().all(on))
    };
    let lens = |face: FaceKey| on_circle(&candidate, face).1;
    if result[0] != result[1]
        && on_circle(body, face).0
        && (lens(result[0]) || lens(result[1]))
    {
        return None;
    }
    *body = candidate;
    Some(result)
}

/// A planar face's area, measured round its loops: holes, walked the other
/// way, take theirs off. `None` for a face on any other surface.
fn planar_area(body: &Body, face: FaceKey) -> Option<f64> {
    planar_measure(body, face).map(|(area, _)| area)
}

/// A planar face's area and the length round all its loops.
pub(super) fn planar_measure(body: &Body, face: FaceKey) -> Option<(f64, f64)> {
    let node = body.faces.get(face)?;
    let super::geometry::Surface::Plane(plane) = body.surfaces.get(node.surface)? else {
        return None;
    };
    let normal = Vec3::from(plane.normal()?) * if node.forward { 1.0 } else { -1.0 };
    let mut area = 0.0;
    let mut perimeter = 0.0;
    for ring in &node.loops {
        // Each edge's share of the area swept about the loop's first point,
        // `½ ∫ (p − first) × dp`: in closed form along a circle, whose
        // samples were most of a plate's cut when every hole was walked at
        // every cut; along a chain of chords elsewhere.
        let mut first = None;
        for coedge in &body.loops.get(*ring)?.coedges {
            let coedge = body.coedges.get(*coedge)?;
            let edge = body.edges.get(coedge.edge)?;
            let curve = body.curves.get(edge.curve)?;
            let (from, to) = match coedge.forward {
                true => (edge.start_parameter, edge.end_parameter),
                false => (edge.end_parameter, edge.start_parameter),
            };
            let first = *first.get_or_insert(Vec3::from(curve.point_at(from)));
            if let Curve3::Circle(circle) = curve {
                let x = Vec3::from(circle.plane.x_axis);
                let y = Vec3::from(circle.plane.y_axis);
                let centre = Vec3::from(circle.plane.origin);
                let chord = Vec3::from(curve.point_at(to)) - Vec3::from(curve.point_at(from));
                let swept = (centre - first).cross(chord)
                    + x.cross(y) * (circle.radius * circle.radius * (to - from));
                area += 0.5 * swept.dot(normal);
                perimeter += circle.radius * (to - from).abs();
                continue;
            }
            let steps = if matches!(curve, Curve3::Line(_)) { 1 } else { 256 };
            let mut previous = Vec3::from(curve.point_at(from));
            for step in 1..=steps {
                let next =
                    Vec3::from(curve.point_at(from + (to - from) * step as f64 / steps as f64));
                area += 0.5 * (previous - first).cross(next - previous).dot(normal);
                perimeter += previous.distance(next);
                previous = next;
            }
        }
    }
    Some((area, perimeter))
}

fn split_face_in_place(
    body: &mut std::borrow::Cow<'_, Body>,
    face: FaceKey,
    cutter: &Curve3,
    tolerance: f64,
) -> Option<[FaceKey; 2]> {
    let node = body.faces.get(face)?.clone();
    let surface = body.surfaces.get(node.surface)?.clone();
    let flat_cutter = pcurve::project(&surface, cutter, tolerance)?;
    if matches!(surface, super::geometry::Surface::Sphere(_)) {
        if let Some(result) =
            split_full_sphere(body.to_mut(), face, &node, &surface, cutter, &flat_cutter)
        {
            return Some(result);
        }
    }
    let boundary_parts = pcurve::face_boundary_parts(body, face, tolerance)?;
    let original_boundary: Vec<_> = boundary_parts
        .iter()
        .map(|(_, curve)| curve.clone())
        .collect();
    let periods = pcurve::periods(&surface);

    // Where the cut meets the boundary, as points in space.
    let mut landings: Vec<Landing> = Vec::new();
    // Two circles on one sphere cross where the line their planes share
    // pierces it. Solved there exactly rather than between the sampled
    // chains their parameter-space images are, which only come near.
    let sphere_circle = match (&surface, cutter) {
        (super::geometry::Surface::Sphere(sphere), Curve3::Circle(circle)) => {
            Some((sphere, circle))
        }
        _ => None,
    };
    // Boxes in (u, v), so a copy of the cutter a turn away, or an edge
    // nowhere near it, is passed over without the crossing search.
    let piece_boxes: Vec<Option<[[f64; 2]; 2]>> =
        boundary_parts.iter().map(|(_, piece)| uv_box(piece)).collect();
    // Whether a point is within the tolerance of the boundary, asked only of
    // the pieces whose boxes come that near: a plate's face with a hundred
    // holes measured every hole's image for every point.
    let near_boundary = |point: [f64; 2]| {
        original_boundary.iter().zip(&piece_boxes).any(|(edge, bounds)| {
            bounds.is_none_or(|[low, high]| {
                point[0] >= low[0] - tolerance
                    && point[0] <= high[0] + tolerance
                    && point[1] >= low[1] - tolerance
                    && point[1] <= high[1] + tolerance
            }) && distance_to(edge, point) <= tolerance
        })
    };
    let mut crossings_of = |shifted_cutter: Option<&ocs_geom::geom2d::Curve>| -> Option<()> {
        let cutter_box = shifted_cutter.and_then(uv_box);
        for (coedge, flat_edge) in &boundary_parts {
            let edge_key = body.coedges.get(*coedge)?.edge;
            let edge = body.edges.get(edge_key)?.clone();
            let curve = body.curves.get(edge.curve)?.clone();
            // An edge on the cutter's own curve is where an earlier cut
            // already ran; it has no crossing to give, and sampling two
            // copies of one curve against each other never settles.
            if same_circle(cutter, &curve, tolerance) || curve == *cutter {
                continue;
            }
            let points: Vec<[f64; 3]> = match (shifted_cutter, sphere_circle, &curve) {
                (None, Some((sphere, circle)), Curve3::Circle(rim)) => {
                    circles_meet_on_sphere(sphere, circle, rim, tolerance)
                }
                (Some(_), Some(_), Curve3::Circle(_)) | (None, _, _) => continue,
                (Some(shifted_cutter), _, _) => {
                    let index = boundary_parts.iter().position(|(key, _)| key == coedge)?;
                    if let (Some(one), Some(other)) = (cutter_box, piece_boxes[index]) {
                        if one[1][0] < other[0][0]
                            || other[1][0] < one[0][0]
                            || one[1][1] < other[0][1]
                            || other[1][1] < one[0][1]
                        {
                            continue;
                        }
                    }
                    // A sampled image crosses only to within its chords;
                    // the exact curves settle where.
                    let sampled = matches!(cutter, Curve3::Nurbs(_))
                        || matches!(shifted_cutter, ocs_geom::geom2d::Curve::Polyline(_))
                        || matches!(flat_edge, ocs_geom::geom2d::Curve::Polyline(_));
                    cross(shifted_cutter, flat_edge, Tolerance::new(tolerance))
                        .into_iter()
                        .map(|crossing| surface.point_at(crossing.point[0], crossing.point[1]))
                        .map(|point| match sampled {
                            true => onto_both(&curve, cutter, point, tolerance),
                            false => point,
                        })
                        .collect()
                }
            };
            for point in points {
                let along = parameter_in_span(
                    &curve,
                    point,
                    edge.start_parameter,
                    edge.end_parameter,
                );
                // The boundary pcurve may run past the edge it came from — a
                // straight edge projects to an infinite line, so a crossing
                // off the edge's own span is not on the boundary at all.
                let (low, high) = (
                    edge.start_parameter.min(edge.end_parameter),
                    edge.start_parameter.max(edge.end_parameter),
                );
                let slack = (high - low).abs() * 1e-9;
                // Just past an end is that end, a crossing exact to where
                // the corner was put only to within the tolerance.
                let at_end = |parameter: f64| {
                    Vec3::from(curve.point_at(parameter)).distance(Vec3::from(point)) <= tolerance
                };
                if (along < low - slack && !at_end(low)) || (along > high + slack && !at_end(high))
                {
                    continue;
                }
                if landings.iter().any(|seen| {
                    Vec3::from(seen.point).distance(Vec3::from(point)) <= tolerance
                        && (seen.edge != edge_key || seen.coedge == *coedge)
                })
                {
                    continue;
                }
                landings.push(Landing {
                    edge: edge_key,
                    coedge: *coedge,
                    point,
                });
            }
        }
        Some(())
    };
    crossings_of(None)?;
    for shifted_cutter in periodic_images(&flat_cutter, periods) {
        crossings_of(Some(&shifted_cutter))?;
    }
    // A spline that meets the boundary only by touching it — a fillet's
    // curve running into the wall it blends into, both on a third surface —
    // crosses nothing a walk can find, yet passes through a corner the
    // boundary already has. The corner is the landing.
    // An exact cutter passing through a corner lands there the same way:
    // crossings at the very ends of edges go unfound — a curve entering a
    // face at a corner two other cuts made, or between two pieces of itself
    // already cut.
    let spline_cutter = matches!(cutter, Curve3::Nurbs(_));
    let on_cutter = |coedge: CoedgeKey| {
        body.coedges
            .get(coedge)
            .and_then(|coedge| body.edges.get(coedge.edge))
            .and_then(|edge| body.curves.get(edge.curve))
            .is_some_and(|curve| same_circle(cutter, curve, tolerance) || curve == cutter)
    };
    let cut_before = boundary_parts.iter().any(|(coedge, _)| on_cutter(*coedge));
    // A closed cutter touching two walls of a corner of a plane has a half
    // inside the face either way round. The first cut took one; the other
    // still runs through the face from the ends of that cut, which are its
    // landings now. The rest of the turn is across from the cut's middle.
    let rest_inside = closed_period(cutter).is_some()
        && matches!(surface, super::geometry::Surface::Plane(_))
        && boundary_parts.iter().any(|(coedge, _)| {
            if !on_cutter(*coedge) {
                return false;
            }
            let Some(edge) = body.coedges.get(*coedge).and_then(|key| body.edges.get(key.edge))
            else {
                return false;
            };
            let Some(curve) = body.curves.get(edge.curve) else {
                return false;
            };
            let middle = curve.point_at(0.5 * (edge.start_parameter + edge.end_parameter));
            let period = closed_period(cutter).unwrap_or(0.0);
            let across = cutter.point_at(cutter.parameter_at(middle) + 0.5 * period);
            surface.parameters_at(across).is_some_and(|(u, v)| {
                pcurve::contains_parameter_facing(
                    &surface,
                    &original_boundary,
                    [u, v],
                    Tolerance::new(tolerance),
                    node.forward,
                ) && !near_boundary([u, v])
            })
        });
    let mut cut_ends: Vec<[f64; 3]> = Vec::new();
    for (coedge, _) in &boundary_parts {
        let Some((vertex, _)) = body.coedge_vertices(*coedge) else {
            continue;
        };
        // The corner where the cutter's own edge ends is where it was
        // cut before, not a crossing.
        // The one before it in its own loop: the parts run loop after loop.
        let previous = body.coedges.get(*coedge).and_then(|node| {
            let ring = &body.loops.get(node.owner)?.coedges;
            let at = ring.iter().position(|key| key == coedge)?;
            ring.get((at + ring.len() - 1) % ring.len()).copied()
        });
        if !spline_cutter
            && !rest_inside
            && (on_cutter(*coedge) || previous.is_some_and(on_cutter))
        {
            continue;
        }
        let Some(point) = body.vertices.get(vertex).map(|vertex| vertex.point) else {
            continue;
        };
        let gap = Vec3::from(cutter.point_at(cutter.parameter_at(point)))
            .distance(Vec3::from(point));
        let seen = landings
            .iter()
            .any(|landing| Vec3::from(landing.point).distance(Vec3::from(point)) <= tolerance);
        let near = if spline_cutter { tolerance * 10.0 } else { tolerance };
        // An end of the cut is on the cutter however far off it reads: a
        // corner where it ran tangent into a wall is placed only roughly.
        // A crossing found beside it, where the cutter ran tangent, is it.
        let cut_end = rest_inside && (on_cutter(*coedge) || previous.is_some_and(on_cutter));
        if cut_end {
            landings.retain(|landing| {
                Vec3::from(landing.point).distance(Vec3::from(point)) > tolerance
            });
            cut_ends.push(point);
        }
        if (gap <= near && !seen) || cut_end {
            let edge = body.coedges.get(*coedge)?.edge;
            landings.push(Landing { edge, coedge: *coedge, point });
        }
    }
    // A spline's fit: a thousandth of the spline, however short the stretch
    // being cut — two copies of one curve part a hair apart along all of it.
    let spline_size = match cutter {
        Curve3::Nurbs(spline) => {
            let (start, end) = spline.domain();
            let samples: Vec<Vec3> = (0..=16)
                .map(|step| {
                    Vec3::from(cutter.point_at(start + (end - start) * step as f64 / 16.0))
                })
                .collect();
            samples.iter().map(|point| point.distance(samples[0])).fold(0.0, f64::max)
        }
        _ => 0.0,
    };
    // A traced cutter crosses an edge where the exact curves meet only to
    // within its fit, and near a corner it crosses both edges there: two
    // landings a hair apart for one crossing at the corner.
    // A spline edge — a file's fitted intersection — is crossed by an exact
    // cutter only to within its own fit as well.
    let edge_fit = |edge: EdgeKey| -> f64 {
        let Some(node) = body.edges.get(edge) else {
            return 0.0;
        };
        let Some(curve @ Curve3::Nurbs(_)) = body.curves.get(node.curve) else {
            return 0.0;
        };
        let at = |step: usize| {
            let t = step as f64 / 16.0;
            Vec3::from(curve.point_at(node.start_parameter * (1.0 - t) + node.end_parameter * t))
        };
        let first = at(0);
        (1..=16).map(|step| at(step).distance(first)).fold(0.0, f64::max) * 1e-4
    };
    let snapping = spline_size > 0.0 || landings.iter().any(|landing| edge_fit(landing.edge) > 0.0);
    if snapping {
        // A landing a fit from a corner of its edge is that corner: cutting
        // there instead leaves an edge a fit long beside it.
        for landing in &mut landings {
            let fit = tolerance.max(spline_size * 1e-4).max(edge_fit(landing.edge));
            let corners = body
                .edges
                .get(landing.edge)
                .map(|edge| [edge.start, edge.end])
                .into_iter()
                .flatten()
                .filter_map(|vertex| body.vertices.get(vertex).map(|vertex| vertex.point));
            if let Some(corner) = corners
                .filter(|corner| Vec3::from(*corner).distance(Vec3::from(landing.point)) <= fit)
                .min_by(|a, b| {
                    let gap = |c: &[f64; 3]| Vec3::from(*c).distance(Vec3::from(landing.point));
                    gap(a).total_cmp(&gap(b))
                })
            {
                landing.point = corner;
            }
        }
        // Two landings snapped onto one corner are one landing — unless they
        // are the two uses of one seam edge, as when they were found.
        let mut kept: Vec<Landing> = Vec::with_capacity(landings.len());
        for landing in landings {
            let seen = |other: &Landing| {
                Vec3::from(other.point).distance(Vec3::from(landing.point)) <= tolerance
                    && (other.edge != landing.edge || other.coedge == landing.coedge)
            };
            if !kept.iter().any(seen) {
                kept.push(landing);
            }
        }
        landings = kept;
    }
    let fit_over = |from: f64, to: f64| {
        let chord = Vec3::from(cutter.point_at(from)).distance(Vec3::from(cutter.point_at(to)));
        tolerance.max(1e-3 * chord.max(spline_size))
    };
    let inside = |parameter: f64| {
        let (u, v) = surface.parameters_at(cutter.point_at(parameter))?;
        Some(periodic_points([u, v], periods).into_iter().any(|point| {
            pcurve::contains_parameter_facing(
                &surface,
                &original_boundary,
                point,
                Tolerance::new(tolerance),
                node.forward,
            )
        }))
    };
    let strictly_inside = |parameter: f64| {
        let (u, v) = surface.parameters_at(cutter.point_at(parameter))?;
        Some(periodic_points([u, v], periods).into_iter().any(|point| {
            pcurve::contains_parameter_facing(
                &surface,
                &original_boundary,
                point,
                Tolerance::new(tolerance),
                node.forward,
            ) && !near_boundary(point)
        }))
    };
    // Whether the stretch of the cutter between two parameters runs along the
    // boundary: on it halfway, and a quarter of the way from either end too.
    // A cut that only touches the boundary halfway — an arc grazing a
    // straight edge, where a round meets the wall it is tangent to — still
    // divides the face.
    let along = |from: f64, to: f64| {
        let on = |fraction: f64| {
            on_boundary(
                body,
                &boundary_parts,
                cutter,
                cutter.point_at(from + (to - from) * fraction),
                fit_over(from, to),
                tolerance,
            )
        };
        on(0.5) && (on(0.25) || on(0.75))
    };
    if matches!(surface, super::geometry::Surface::Plane(_)) {
        if let Some(period) = closed_period(cutter) {
            landings.retain(|landing| {
                // One end of a cut whose other half is still to make: along
                // the cut one way, into the face the other.
                if cut_ends.contains(&landing.point) {
                    return true;
                }
                let parameter = cutter.parameter_at(landing.point);
                // Far enough along to leave the containment test's own
                // tolerance behind: where the cutter crosses at a shallow
                // angle, a millionth of a turn stays within it either side
                // and a real crossing read as a touch.
                let probe = period * 1.0e-6;
                let speed = Vec3::from(cutter.point_at(parameter + probe))
                    .distance(Vec3::from(cutter.point_at(parameter)))
                    / probe;
                let step = probe.max(if speed > 0.0 { 50.0 * tolerance / speed } else { 0.0 });
                matches!(
                    (inside(parameter - step), inside(parameter + step)),
                    (Some(before), Some(after)) if before != after
                )
            });
        }
    }
    // A closed cutter touching the boundary at two places or more — a
    // fillet's circle against both walls of its corner — divides the face
    // there as surely as one crossing it, though no crossing was found.
    // A face cut along this curve before meets it at that cut's ends,
    // which are not touches.
    // A closed cutter crossing no edge is wholly inside the face or wholly
    // out of it, touching or not; out of it, nothing more needs asking.
    let mut touches = None;
    if landings.is_empty() {
        let period = closed_period(cutter)?;
        if inside(0.0) == Some(false) && inside(period * 0.5) == Some(false) {
            return None;
        }
        if !cut_before {
            let found = boundary_touches(body, &boundary_parts, cutter, period, tolerance);
            if found.len() >= 2 {
                landings = found;
            } else {
                touches = Some(found);
            }
        }
    }
    if landings.is_empty() {
        let period = closed_period(cutter)?;
        // A closed cutter that already is one of the face's edges was cut
        // before: the landings skip it as their own curve, and cutting
        // between the loops again peeled a sliver off the same band for ever
        // (#1563).
        let already_cut = boundary_parts.iter().any(|(coedge, _)| {
            body.coedges
                .get(*coedge)
                .and_then(|coedge| body.edges.get(coedge.edge))
                .and_then(|edge| body.curves.get(edge.curve))
                .is_some_and(|curve| same_circle(cutter, curve, tolerance) || curve == cutter)
        });
        if already_cut {
            return None;
        }
        // Loops the cutter closes round go with the island it cuts out. On a
        // band the cutter is straight and runs between the loops; on a plane
        // it is a closed curve, and an annulus cut by a circle between its
        // rims keeps one rim on each side (#1563's collar under a washer).
        let mut enclosed = Vec::new();
        // On a curved surface a closed cutter either runs once round it —
        // between the face's rims, cutting a band in two — or closes on
        // itself in `(u, v)`, an island like one on a plane (two cones
        // crossing side by side).
        let (head, tail) = (flat_cutter.point_at(0.0), flat_cutter.point_at(1.0));
        let travel = (tail[0] - head[0]).abs().max((tail[1] - head[1]).abs());
        let wraps = periods.iter().flatten().any(|period| (travel - period).abs() < period * 1e-3);
        if node.loops.len() > 1 {
            if (!matches!(surface, super::geometry::Surface::Plane(_)) && wraps)
                || matches!(flat_cutter, ocs_geom::geom2d::Curve::Line(_))
            {
                return split_closed_between_loops(
                    body.to_mut(),
                    face,
                    &node,
                    &surface,
                    cutter,
                    &flat_cutter,
                    &boundary_parts,
                    period,
                );
            }
            for ring in &node.loops {
                // An apex loop has no image to enclose; the island, not
                // running round the axis, cannot hold it.
                let Some(&first) = body.loops.get(*ring)?.coedges.first() else {
                    continue;
                };
                let boundary = &boundary_parts.iter().find(|(key, _)| *key == first)?.1;
                if ocs_geom::geom2d::contains(
                    std::slice::from_ref(&flat_cutter),
                    boundary.point_at(0.5),
                    Tolerance::new(tolerance),
                ) {
                    enclosed.push(*ring);
                }
            }
        }
        // An island lies inside the face all the way round. One reaching
        // out of it crosses the boundary — at corners the landings passed
        // over — and cut as an island it copied a piece of the face over
        // a neighbour that already had it.
        if (0..8).any(|index| inside(period * (index as f64 + 0.5) / 8.0) == Some(false)) {
            return None;
        }
        let start_parameter = (0..16)
            .map(|index| period * index as f64 / 16.0)
            .find(|parameter| strictly_inside(*parameter) == Some(true))?;
        let point = cutter.point_at(start_parameter);
        let (u, v) = surface.parameters_at(point)?;
        let strictly_inside = periodic_points([u, v], periods).into_iter().any(|point| {
            ocs_geom::geom2d::contains(
                &original_boundary,
                point,
                Tolerance::new(tolerance),
            ) && !near_boundary(point)
        });
        if !strictly_inside {
            return None;
        }

        // A closed cutter carries its own plane orientation. Surface/face
        // sense alone says nothing about which way increasing its parameter
        // winds: intersection circles can use the opposite plane normal.
        // The island follows the face and the new hole runs the other way.
        let alignment = match cutter {
            Curve3::Circle(Circle3 { plane, .. }) | Curve3::Ellipse(Ellipse3 { plane, .. }) => {
                Vec3::from(plane.normal()?).dot(Vec3::from(surface.normal_at(u, v)?))
            }
            Curve3::PlanarSpline { plane, curve } => {
                Vec3::from(plane.normal()?).dot(Vec3::from(surface.normal_at(u, v)?))
                    * ocs_geom::geom2d::Curve::Nurbs(curve.clone()).enclosed_area()
            }
            // A closed curve traced across a curved face has no plane of its
            // own; its image winds in (u, v), which turns the way the
            // surface's own normal does.
            Curve3::Nurbs(_) => flat_cutter.enclosed_area(),
            _ => return None,
        };
        if !alignment.is_finite() || alignment == 0.0 {
            return None;
        }
        // A plane face whose loops wind against its normal — as some files
        // write them — takes its island the way its loops actually run.
        let winds_back = planar_area(body, face).is_some_and(|area| area < 0.0);
        let island_forward = ((alignment > 0.0) == node.forward) != winds_back;

        // A cutter touching the boundary at one point — a fillet's circle
        // meeting the walls beside it — bounds no island: the face would
        // hang from the rest by that point. The touch becomes a corner the
        // outer loop passes through on its way round the cut.
        let touch = touches
            .unwrap_or_else(|| boundary_touches(body, &boundary_parts, cutter, period, tolerance))
            .pop();
        let body = body.to_mut();
        let (seam, start_parameter, attached) = match touch {
            Some(landing) => {
                let vertex = vertex_at(body, &landing, tolerance)?;
                let at = body.vertices.get(vertex)?.point;
                let ring = body.coedges.get(landing.coedge)?.owner;
                (vertex, cutter.parameter_at(at), Some(ring))
            }
            None => {
                let seam = body.vertices.insert(Vertex {
                    point,
                    provenance: Provenance::Synthesized,
                });
                (seam, start_parameter, None)
            }
        };
        let curve = body.curves.insert(cutter.clone());
        let cut = body.edges.insert(Edge {
            curve,
            start_parameter,
            end_parameter: start_parameter + period,
            start: seam,
            end: seam,
            coedges: Vec::new(),
            provenance: Provenance::Synthesized,
        });

        let hole_ring = match attached {
            Some(ring) => ring,
            None => body.loops.insert(Loop {
                coedges: Vec::new(),
                owner: face,
                provenance: Provenance::Synthesized,
            }),
        };
        let hole = body.coedges.insert(Coedge {
            edge: cut,
            forward: !island_forward,
            pcurve: None,
            owner: hole_ring,
            provenance: Provenance::Synthesized,
        });
        match attached {
            // Round the cut and back, between the two coedges meeting at
            // the touch.
            Some(ring) => {
                let at = body.loops.get(ring)?.coedges.iter().position(|coedge| {
                    body.coedge_vertices(*coedge).is_some_and(|(_, end)| end == seam)
                })?;
                body.loops.get_mut(ring)?.coedges.insert(at + 1, hole);
            }
            None => {
                body.loops.get_mut(hole_ring)?.coedges = vec![hole];
                body.faces.get_mut(face)?.loops.push(hole_ring);
            }
        }
        body.faces.get_mut(face)?.provenance.soil();

        let other = body.faces.insert(Face {
            surface: node.surface,
            forward: node.forward,
            loops: Vec::new(),
            owner: node.owner,
            provenance: Provenance::Synthesized,
        });
        let other_ring = body.loops.insert(Loop {
            coedges: Vec::new(),
            owner: other,
            provenance: Provenance::Synthesized,
        });
        let inner = body.coedges.insert(Coedge {
            edge: cut,
            forward: island_forward,
            pcurve: None,
            owner: other_ring,
            provenance: Provenance::Synthesized,
        });
        body.loops.get_mut(other_ring)?.coedges = vec![inner];
        body.faces.get_mut(face)?.loops.retain(|ring| !enclosed.contains(ring));
        for ring in &enclosed {
            body.loops.get_mut(*ring)?.owner = other;
        }
        body.faces.get_mut(other)?.loops =
            std::iter::once(other_ring).chain(enclosed.iter().copied()).collect();
        body.shells.get_mut(node.owner)?.faces.push(other);
        body.edges.get_mut(cut)?.coedges = vec![hole, inner];
        return Some([face, other]);
    }
    // The two uses of a seam edge each land the cutter where it crosses
    // the seam, at one point and one parameter, so which of the two a sort
    // puts first says nothing, and the two uses' images in the face can read
    // the same. A face lies to the left of each use walking along it, seen
    // from outside; a cut leaves from the use whose left it runs into. The
    // other joins it to the face's far side, and a cut that should take a
    // lens off one side of the seam then left the rest of the face wrapped
    // over the lens.
    let leftward = |landing: &Landing, into: f64, ahead: bool| -> f64 {
        let point = landing.point;
        let Some((u, v)) = surface.parameters_at(point) else {
            return f64::NEG_INFINITY;
        };
        let Some(normal) = surface.normal_at(u, v).and_then(|n| Vec3::from(n).normalize()) else {
            return f64::NEG_INFINITY;
        };
        let outward = if node.forward { normal } else { -normal };
        let departing = Vec3::from(cutter.tangent_at(into)) * if ahead { 1.0 } else { -1.0 };
        let Some(coedge) = body.coedges.get(landing.coedge) else {
            return f64::NEG_INFINITY;
        };
        let Some(curve) = body.edges.get(landing.edge).and_then(|e| body.curves.get(e.curve))
        else {
            return f64::NEG_INFINITY;
        };
        let walking = Vec3::from(curve.tangent_at(curve.parameter_at(point)))
            * if coedge.forward { 1.0 } else { -1.0 };
        walking.cross(departing).dot(outward)
    };
    let side = |all: &[Landing], landing: &Landing, into: f64, ahead: bool| -> Landing {
        all.iter()
            .filter(|other| {
                other.edge == landing.edge
                    && Vec3::from(other.point).distance(Vec3::from(landing.point)) <= tolerance
            })
            .max_by(|a, b| leftward(a, into, ahead).total_cmp(&leftward(b, into, ahead)))
            .map_or_else(|| landing.clone(), Landing::clone)
    };
    let mut selected_span = None;
    if landings.len() > 2 {
        // Split one interior span; the caller retries both resulting faces.
        landings.sort_by(|a, b| {
            cutter
                .parameter_at(a.point)
                .total_cmp(&cutter.parameter_at(b.point))
        });
        let period = closed_period(cutter);
        let count = landings.len();
        let pair_count = if period.is_some() { count } else { count - 1 };
        let pair = (0..pair_count).find_map(|index| {
            let next = (index + 1) % count;
            let first = cutter.parameter_at(landings[index].point);
            let mut second = cutter.parameter_at(landings[next].point);
            if next == 0 {
                second += period?;
            }
            // A span along an edge already there — a cut made on an earlier
            // pass — is not inside, however the walk of it reads.
            let middle = 0.5 * (first + second);
            let along_boundary = on_boundary(
                body,
                &boundary_parts,
                cutter,
                cutter.point_at(middle),
                fit_over(first, second),
                tolerance,
            );
            (strictly_inside(middle)? && !along_boundary)
                .then_some((
                    [landings[index].clone(), landings[next].clone()],
                    (first, second),
                ))
        });
        let (pair, span) = pair?;
        landings = vec![
            side(&landings, &pair[0], span.0, true),
            side(&landings, &pair[1], span.1, false),
        ];
        selected_span = Some(span);
    }
    if landings.len() != 2 {
        return None;
    }

    let same_vertex_landing = Vec3::from(landings[0].point)
        .distance(Vec3::from(landings[1].point))
        <= tolerance;
    let first_parameter = cutter.parameter_at(landings[0].point);
    let second_parameter = cutter.parameter_at(landings[1].point);
    let (low, high, low_parameter, high_parameter) =
        if first_parameter <= second_parameter {
            (0, 1, first_parameter, second_parameter)
        } else {
            (1, 0, second_parameter, first_parameter)
        };
    let (start_landing, end_landing, start_parameter, end_parameter) = match (
        selected_span,
        closed_period(cutter),
    ) {
        (Some((start, end)), _) => (0, 1, start, end),
        (None, Some(period)) if same_vertex_landing => {
            (low, high, low_parameter, low_parameter + period)
        }
        (None, Some(period)) => {
            // A half already cut along is boundary now, not inside.
            // A half that only grazes the boundary halfway is open where it
            // runs inside the face either side of that, not outside it.
            let open = |from: f64, to: f64| -> Option<bool> {
                let at = |fraction: f64| from + (to - from) * fraction;
                let grazing = on_boundary(
                    body,
                    &boundary_parts,
                    cutter,
                    cutter.point_at(at(0.5)),
                    fit_over(from, to),
                    tolerance,
                );
                let beside = !grazing || (inside(at(0.25))? && inside(at(0.75))?);
                Some(inside(at(0.5))? && !along(from, to) && beside)
            };
            let direct = open(low_parameter, high_parameter)?;
            let wrapped = open(high_parameter, low_parameter + period)?;
            match (direct, wrapped) {
                // A loop through two corners with both halves inside cuts
                // the face in three; one half now, the other on the retry.
                // The shorter half goes first, whichever way the ends' angles
                // read: an end at a half turn reads as either sign, and the
                // longer half first — a rounded corner's whole circle, not
                // its quarter — left a piece on both sides of the other body.
                (true, true) if low_parameter + period - high_parameter < high_parameter - low_parameter => {
                    (high, low, high_parameter, low_parameter + period)
                }
                (true, _) => (low, high, low_parameter, high_parameter),
                (false, true) => (high, low, high_parameter, low_parameter + period),
                (false, false) => return None,
            }
        }
        // Two landings on the boundary bound a stretch that may run through
        // a hole of the face rather than across it.
        // An open cutter from a seam corner back to it, landing on the
        // seam's two uses, runs once round the surface: a loop of a curve
        // that crossed itself there, cut free at the crossing. It divides
        // the band from the use it leaves into to the other.
        (None, None)
            if same_vertex_landing
                && landings[0].edge == landings[1].edge
                && landings[0].coedge != landings[1].coedge
                && matches!(cutter, Curve3::Nurbs(_)) =>
        {
            let Curve3::Nurbs(spline) = cutter else {
                return None;
            };
            let (from, to) = spline.domain();
            if !inside(0.5 * (from + to))? {
                return None;
            }
            let first_leaves =
                leftward(&landings[0], from, true) > leftward(&landings[1], from, true);
            if first_leaves { (0, 1, from, to) } else { (1, 0, from, to) }
        }
        (None, None) => {
            // An open cutter landing twice on one corner spans nothing.
            if same_vertex_landing || !inside(0.5 * (low_parameter + high_parameter))? {
                return None;
            }
            (low, high, low_parameter, high_parameter)
        }
    };

    // However the containment tests read, a cut cannot run far outside the
    // face it divides: one that does has gone the long way round a closed
    // cutter, across the rest of the surface.
    {
        let mut low = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut high = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (coedge, _) in &boundary_parts {
            let Some(edge) =
                body.coedges.get(*coedge).and_then(|coedge| body.edges.get(coedge.edge))
            else {
                continue;
            };
            let Some(curve) = body.curves.get(edge.curve) else {
                continue;
            };
            for step in 0..=8 {
                let t = edge.start_parameter
                    + (edge.end_parameter - edge.start_parameter) * step as f64 / 8.0;
                let p = Vec3::from(curve.point_at(t));
                low = Vec3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z));
                high = Vec3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z));
            }
        }
        // A cone face reaching its apex reaches past its edges.
        let apex_loop = node
            .loops
            .iter()
            .any(|ring| body.loops.get(*ring).is_some_and(|ring| ring.coedges.is_empty()));
        if let (super::geometry::Surface::Cone(cone), true) = (&surface, apex_loop) {
            let p = Vec3::from(surface.point_at(0.0, cone.radius / cone.half_angle.tan()));
            if p.is_finite() {
                low = Vec3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z));
                high = Vec3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z));
            }
        }
        if low.is_finite() && high.is_finite() {
            let pad = low.distance(high) + tolerance * 10.0;
            let outside = (0..=8).any(|step| {
                let t = start_parameter + (end_parameter - start_parameter) * step as f64 / 8.0;
                let p = Vec3::from(cutter.point_at(t));
                p.x < low.x - pad
                    || p.y < low.y - pad
                    || p.z < low.z - pad
                    || p.x > high.x + pad
                    || p.y > high.y + pad
                    || p.z > high.z + pad
            });
            if outside {
                return None;
            }
        }
    }

    // An open cut that ends where it starts encloses nothing — unless it
    // goes from one use of a seam to the other, once round the surface.
    let round_seam =
        landings[0].edge == landings[1].edge && landings[0].coedge != landings[1].coedge;
    if closed_period(cutter).is_none()
        && !round_seam
        && Vec3::from(cutter.point_at(start_parameter))
            .distance(Vec3::from(cutter.point_at(end_parameter)))
            <= tolerance
    {
        return None;
    }

    // A cut that runs along the boundary divides nothing. It happens
    // naturally the moment a face has already been cut: the new edge lies on
    // the cutter, its two ends are vertices, and the next pass finds the same
    // two landings and cuts again — the same face for ever. Asked inside
    // the cut rather than at the ends, since a genuine cut also touches the
    // boundary at both of those, and not halfway alone, since one may graze
    // the boundary there.
    // Asked of the edges' own curves too: a sampled boundary image sits off
    // its edge by more than the tolerance between samples.
    let on_image = |parameter: f64| -> Option<bool> {
        let (u, v) = surface.parameters_at(cutter.point_at(parameter))?;
        Some(periodic_points([u, v], periods).into_iter().any(|point| {
            near_boundary(point)
        }))
    };
    let span = end_parameter - start_parameter;
    let imaged = on_image(start_parameter + span * 0.5)?
        && (on_image(start_parameter + span * 0.25)? || on_image(start_parameter + span * 0.75)?);
    // A stretch cut before is an edge of the body already, between the same
    // two corners: a cutter touching the boundary where it was cut finds
    // those corners again and would peel the same sliver off for ever.
    let cut_already = || {
        let ends = [start_parameter, end_parameter, start_parameter + span * 0.5]
            .map(|at| Vec3::from(cutter.point_at(at)));
        body.edges.iter().any(|(_, edge)| {
            body.curves.get(edge.curve).is_some_and(|curve| curve == cutter)
                && [edge.start_parameter, edge.end_parameter].iter().all(|at| {
                    let point = Vec3::from(cutter.point_at(*at));
                    point.distance(ends[0]) <= tolerance || point.distance(ends[1]) <= tolerance
                })
                && Vec3::from(cutter.point_at(0.5 * (edge.start_parameter + edge.end_parameter)))
                    .distance(ends[2])
                    <= tolerance
        })
    };
    let traced = matches!(cutter, Curve3::Nurbs(spline) if spline.degree() > 1);
    if imaged || along(start_parameter, end_parameter) || (traced && cut_already()) {
        return None;
    }

    let ring_of = |landing: &Landing| body.coedges.get(landing.coedge).map(|coedge| coedge.owner);
    let (first_ring, second_ring) = (ring_of(&landings[0])?, ring_of(&landings[1])?);

    // Mutate the boundary only after proving this is a new cut.
    let body = body.to_mut();
    let first = vertex_at(body, &landings[0], tolerance)?;
    let second = vertex_at(body, &landings[1], tolerance)?;
    if first == second && !same_vertex_landing {
        return None;
    }
    // An edge of the face already joining the two corners the same way is
    // this cut made before — by a copy of the cutter a hair off, say — and
    // cutting again only peels a sliver off between the two.
    if first != second {
        let midway = Vec3::from(cutter.point_at(0.5 * (start_parameter + end_parameter)));
        let fit = fit_over(start_parameter, end_parameter);
        let repeated = body.face_coedges(face).iter().any(|coedge| {
            let Some(edge) =
                body.coedges.get(*coedge).and_then(|coedge| body.edges.get(coedge.edge))
            else {
                return false;
            };
            let joins = (edge.start == first && edge.end == second)
                || (edge.start == second && edge.end == first);
            joins
                && body.curves.get(edge.curve).is_some_and(|curve| {
                    let middle =
                        curve.point_at(0.5 * (edge.start_parameter + edge.end_parameter));
                    Vec3::from(middle).distance(midway) <= fit
                })
        });
        if repeated {
            return None;
        }
    }
    let ends = [first, second];
    let (start, end) = (ends[start_landing], ends[end_landing]);

    // The new edge, running along the cut between them.
    let curve = body.curves.insert(cutter.clone());
    let cut = body.edges.insert(Edge {
        curve,
        start_parameter,
        end_parameter,
        start,
        end,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    });

    // A cut from one loop to another divides nothing yet: it joins them, a
    // band's two rims or a hole and the outer boundary, into one loop that
    // runs round the first, along the cut, round the second and back. The
    // next cut across the face divides it as it would any one-loop face.
    if first_ring != second_ring {
        let from_corner = |body: &Body, ring: LoopKey, vertex: VertexKey| {
            let coedges = body.loops.get(ring)?.coedges.clone();
            let at = coedges.iter().position(|coedge| {
                body.coedge_vertices(*coedge).is_some_and(|(from, _)| from == vertex)
            })?;
            Some([&coedges[at..], &coedges[..at]].concat())
        };
        let around_first = from_corner(body, first_ring, first)?;
        let around_second = from_corner(body, second_ring, second)?;
        let across = body.coedges.insert(Coedge {
            edge: cut,
            forward: start == first,
            pcurve: None,
            owner: first_ring,
            provenance: Provenance::Synthesized,
        });
        let back = body.coedges.insert(Coedge {
            edge: cut,
            forward: start == second,
            pcurve: None,
            owner: first_ring,
            provenance: Provenance::Synthesized,
        });
        body.edges.get_mut(cut)?.coedges = vec![across, back];
        for coedge in &around_second {
            body.coedges.get_mut(*coedge)?.owner = first_ring;
        }
        let mut joined = around_first;
        joined.push(across);
        joined.extend(around_second);
        joined.push(back);
        let ring = body.loops.get_mut(first_ring)?;
        ring.coedges = joined;
        ring.provenance.soil();
        body.loops.remove(second_ring);
        let joined_face = body.faces.get_mut(face)?;
        joined_face.loops.retain(|ring| *ring != second_ring);
        joined_face.provenance.soil();
        return Some([face, face]);
    }
    let ring_key = first_ring;

    // The boundary, now split at both ends of the cut, divides into the two
    // arcs between them.
    let ring = body.loops.get(ring_key)?.coedges.clone();
    let begins_at = |body: &Body, coedge: CoedgeKey, vertex: VertexKey| {
        body.coedge_vertices(coedge)
            .is_some_and(|(from, _)| from == vertex)
    };
    // Where the cut leaves each of its ends, along the surface.
    let departing = |end: usize| -> Option<Vec3> {
        let (parameter, sign) =
            if end == start_landing { (start_parameter, 1.0) } else { (end_parameter, -1.0) };
        Some(Vec3::from(cutter.tangent_at(parameter)) * sign)
    };
    // A corner the ring passes more than once — where the cut meets a seam
    // and an earlier cut at one point, say — joins the cut at the pass
    // whose corner of the face the cut runs into: between the edge leaving
    // and the edge arriving there, turning left about the face's outward
    // normal.
    let passing = |vertex: VertexKey, toward: Vec3| -> Option<usize> {
        let starts: Vec<usize> =
            (0..ring.len()).filter(|index| begins_at(body, ring[*index], vertex)).collect();
        if starts.len() < 2 {
            return None;
        }
        let point = body.vertices.get(vertex)?.point;
        let (u, v) = surface.parameters_at(point)?;
        let normal = Vec3::from(surface.normal_at(u, v)?).normalize()?;
        let outward = if node.forward { normal } else { -normal };
        let walking = |coedge: CoedgeKey, leaving: bool| -> Option<Vec3> {
            let node = body.coedges.get(coedge)?;
            let edge = body.edges.get(node.edge)?;
            let curve = body.curves.get(edge.curve)?;
            let at =
                if node.forward == leaving { edge.start_parameter } else { edge.end_parameter };
            Some(Vec3::from(curve.tangent_at(at)) * if node.forward { 1.0 } else { -1.0 })
        };
        let flat = |direction: Vec3| direction - outward * direction.dot(outward);
        let angle = |from: Vec3, to: Vec3| {
            let (from, to) = (flat(from), flat(to));
            let turn = from.cross(to).dot(outward).atan2(from.dot(to));
            if turn < 0.0 { turn + std::f64::consts::TAU } else { turn }
        };
        starts.into_iter().find(|index| {
            let leaving = walking(ring[*index], true);
            let arriving = walking(ring[(*index + ring.len() - 1) % ring.len()], false);
            match (leaving, arriving) {
                (Some(leaving), Some(arriving)) => {
                    angle(leaving, toward) < angle(leaving, -arriving)
                }
                _ => false,
            }
        })
    };
    let landing_index = |landing: &Landing, vertex: VertexKey, end: usize| {
        if let Some(index) = departing(end).and_then(|toward| passing(vertex, toward)) {
            return Some(index);
        }
        let at = ring.iter().position(|coedge| *coedge == landing.coedge)?;
        if begins_at(body, ring[at], vertex) {
            return Some(at);
        }
        let next = (at + 1) % ring.len();
        if begins_at(body, ring[next], vertex) {
            return Some(next);
        }
        // Both ends landed on one edge, and splitting it at the first moved
        // the second onto a piece further round: a piece of the same use
        // of that edge, beside it in the ring. A seam's other use passes
        // the same corner on the face's far side, and joining the cut there
        // divided the face across rather than on the cut's own side.
        let curve_of = |coedge: CoedgeKey| {
            body.coedges.get(coedge).and_then(|node| body.edges.get(node.edge)).map(|e| e.curve)
        };
        let own = curve_of(ring[at]);
        let count = ring.len();
        for toward in [1, count - 1] {
            let mut index = at;
            for _ in 1..count {
                index = (index + toward) % count;
                if curve_of(ring[index]) != own {
                    break;
                }
                if begins_at(body, ring[index], vertex) {
                    return Some(index);
                }
            }
        }
        ring.iter().position(|coedge| begins_at(body, *coedge, vertex))
    };
    let at_first = landing_index(&landings[0], first, 0)?;
    let at_second = landing_index(&landings[1], second, 1)?;
    // Both ends placed by the corner they run into: the near arc, from the
    // first landing round to the second, closes along the cut from the
    // second back to the first.
    let placed = same_vertex_landing
        && [(first, 0), (second, 1)].iter().all(|(vertex, end)| {
            departing(*end).and_then(|toward| passing(*vertex, toward)).is_some()
        });
    // The stretch of the ring from one index round to the other, not
    // including where it stops.
    let arc = |from: usize, to: usize| -> Vec<CoedgeKey> {
        let count = ring.len();
        let mut out = Vec::new();
        let mut index = from;
        while index != to {
            out.push(ring[index]);
            index = (index + 1) % count;
        }
        out
    };
    let near = arc(at_first, at_second);
    let far = arc(at_second, at_first);
    if near.is_empty() || far.is_empty() {
        return None;
    }

    // Each arc is closed by the cut, traversed whichever way takes it back to
    // where the arc began. The two therefore run it opposite ways, which is
    // what makes the new edge a shared one rather than two coincident walls.
    let sense = |from: VertexKey| start == from;
    // A cut starting and ending at one vertex gives no ends to go by; the
    // near arc has to be on the face's side of the cut instead.
    let near_forward = if placed {
        start_landing == 1
    } else if same_vertex_landing {
        // Round a surface closed both ways the side test cannot tell the
        // halves apart; the cut has to close the near arc's walk in (u, v),
        // and only one way round does.
        let closing = || -> Option<bool> {
            if periods.iter().any(Option::is_none) {
                return None;
            }
            let parts = pcurve::face_boundary_parts(body, face, tolerance)?;
            let mut walked = [0.0, 0.0];
            for coedge in &near {
                let piece = &parts.iter().find(|(key, _)| key == coedge)?.1;
                let (from, to) = (piece.point_at(0.0), piece.point_at(1.0));
                walked = [walked[0] + to[0] - from[0], walked[1] + to[1] - from[1]];
            }
            let (from, to) = (flat_cutter.point_at(0.0), flat_cutter.point_at(1.0));
            let cut = [to[0] - from[0], to[1] - from[1]];
            let gap = |sign: f64| (walked[0] + sign * cut[0]).hypot(walked[1] + sign * cut[1]);
            let (forward, backward) = (gap(1.0), gap(-1.0));
            let turn = periods.iter().flatten().fold(f64::INFINITY, |least, p| least.min(*p));
            (forward.min(backward) < turn * 1e-3 && forward.max(backward) > turn * 0.5)
                .then_some(forward < backward)
        };
        match closing() {
            Some(forward) => forward,
            None => {
                near_side_forward(body, &surface, cutter, start_parameter, &near, node.forward)?
            }
        }
    } else {
        sense(second)
    };
    let near_closer = body.coedges.insert(Coedge {
        edge: cut,
        forward: near_forward,
        pcurve: None,
        owner: ring_key,
        provenance: Provenance::Synthesized,
    });
    let mut kept = near;
    kept.push(near_closer);
    {
        let ring = body.loops.get_mut(ring_key)?;
        ring.coedges = kept;
        ring.provenance.soil();
    }
    body.edges.get_mut(cut)?.coedges.push(near_closer);
    if let Some(face) = body.faces.get_mut(face) {
        face.provenance.soil();
    }

    // The far arc moves onto a new face on the same surface, in the same
    // shell.
    let other = body.faces.insert(Face {
        surface: node.surface,
        forward: node.forward,
        loops: Vec::new(),
        owner: node.owner,
        provenance: Provenance::Synthesized,
    });
    let other_ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: other,
        provenance: Provenance::Synthesized,
    });
    let far_closer = body.coedges.insert(Coedge {
        edge: cut,
        forward: if same_vertex_landing { !near_forward } else { sense(first) },
        pcurve: None,
        owner: other_ring,
        provenance: Provenance::Synthesized,
    });
    body.edges.get_mut(cut)?.coedges.push(far_closer);
    for coedge in &far {
        body.coedges.get_mut(*coedge)?.owner = other_ring;
    }
    let mut moved = far;
    moved.push(far_closer);
    body.loops.get_mut(other_ring)?.coedges = moved;
    body.faces.get_mut(other)?.loops = vec![other_ring];
    body.shells.get_mut(node.owner)?.faces.push(other);

    // The face's other loops — its holes, a band's far rim — go with
    // whichever half they lie in. A loop neither half's own boundary closes
    // round belongs to the half that wraps round the surface with it.
    // A cone's apex loop has no curve to stand on; a point just short of
    // the apex, on the face's side of it, stands for it.
    let apex_point = || -> Option<[f64; 2]> {
        let super::geometry::Surface::Cone(cone) = &surface else {
            return None;
        };
        let apex = cone.radius / cone.half_angle.tan();
        let samples: Vec<[f64; 2]> = boundary_parts
            .iter()
            .filter(|(coedge, _)| ring.contains(coedge))
            .map(|(_, piece)| piece.point_at(0.5))
            .collect();
        let count = samples.len() as f64;
        let u = samples.iter().map(|point| point[0]).sum::<f64>() / count;
        let v = samples.iter().map(|point| point[1]).sum::<f64>() / count;
        (apex.is_finite() && count > 0.0).then_some([u, apex - (apex - v) * 1e-3])
    };
    // A few points round each loop: one touching the cut — a hole the cut
    // runs tangent to — is in both halves and settles nothing.
    let extras: Vec<(LoopKey, Vec<[f64; 2]>)> = node
        .loops
        .iter()
        .filter(|ring| **ring != ring_key)
        .map(|ring| {
            let coedges = &body.loops.get(*ring)?.coedges;
            if coedges.is_empty() {
                return Some((*ring, vec![apex_point()?]));
            }
            let mut points = Vec::new();
            for coedge in coedges {
                if let Some((_, piece)) = boundary_parts.iter().find(|(key, _)| key == coedge) {
                    points.extend([0.5, 0.25, 0.75].map(|at| piece.point_at(at)));
                }
            }
            (!points.is_empty()).then_some((*ring, points))
        })
        .collect::<Option<_>>()?;
    if !extras.is_empty() {
        body.faces.get_mut(face)?.loops = vec![ring_key];
        let near_boundary = pcurve::face_boundary(body, face, tolerance)?;
        let far_boundary = pcurve::face_boundary(body, other, tolerance)?;
        let turn = periods[0].unwrap_or(f64::INFINITY);
        let wraps = |boundary: &[ocs_geom::geom2d::Curve]| {
            let (Some(first), Some(last)) = (boundary.first(), boundary.last()) else {
                return false;
            };
            let (start, end) = (first.point_at(0.0), last.point_at(1.0));
            (start[0] - end[0]).hypot(start[1] - end[1]) > turn * 0.5
        };
        let inside = |boundary: &[ocs_geom::geom2d::Curve], point: [f64; 2]| {
            let tolerance = Tolerance::new(tolerance);
            pcurve::contains_parameter_facing(&surface, boundary, point, tolerance, node.forward)
        };
        // Cut off a hole, the cap between the cut and the hole's edge winds
        // against the hole, and the rest still winds with it: that rest keeps
        // whatever neither half closes round.
        let winding = |boundary: &[ocs_geom::geom2d::Curve]| -> f64 {
            let points: Vec<[f64; 2]> = boundary
                .iter()
                .flat_map(|curve| (0..8).map(move |step| curve.point_at(step as f64 / 8.0)))
                .collect();
            (0..points.len())
                .map(|index| {
                    let (a, b) = (points[index], points[(index + 1) % points.len()]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum()
        };
        let original: Vec<ocs_geom::geom2d::Curve> = boundary_parts
            .iter()
            .filter(|(coedge, _)| ring.contains(coedge))
            .map(|(_, curve)| curve.clone())
            .collect();
        let far_keeps = if wraps(&near_boundary) != wraps(&far_boundary) {
            wraps(&far_boundary)
        } else {
            let sense = winding(&original).signum();
            winding(&far_boundary).signum() == sense && winding(&near_boundary).signum() != sense
        };
        let mut kept_loops = vec![ring_key];
        let mut moved_loops = vec![other_ring];
        for (ring, points) in extras {
            // The points that tell the halves apart vote.
            let mut votes = 0i32;
            for point in &points {
                match (inside(&near_boundary, *point), inside(&far_boundary, *point)) {
                    (false, true) => votes += 1,
                    (true, false) => votes -= 1,
                    _ => {}
                }
            }
            let to_far = match votes.signum() {
                1 => true,
                -1 => false,
                _ => far_keeps,
            };
            if to_far {
                body.loops.get_mut(ring)?.owner = other;
                moved_loops.push(ring);
            } else {
                kept_loops.push(ring);
            }
        }
        body.faces.get_mut(face)?.loops = kept_loops;
        body.faces.get_mut(other)?.loops = moved_loops;
    }

    Some([face, other])
}

/// Split an untrimmed one-face sphere without routing the cut through its
/// artificial pole-to-pole seam. The seam is only a parameterisation aid; a
/// real closed section leaves two spherical faces bounded by the section.
fn split_full_sphere(
    body: &mut Body,
    face: FaceKey,
    node: &Face,
    surface: &super::geometry::Surface,
    cutter: &Curve3,
    flat_cutter: &ocs_geom::geom2d::Curve,
) -> Option<[FaceKey; 2]> {
    let period = closed_period(cutter)?;
    if node.loops.len() != 1 {
        return None;
    }
    let old_loop = node.loops[0];
    let old_coedges = body.loops.get(old_loop)?.coedges.clone();
    if old_coedges.len() != 2 {
        return None;
    }
    let first = body.coedges.get(old_coedges[0])?;
    let second = body.coedges.get(old_coedges[1])?;
    if first.edge != second.edge || first.forward == second.forward {
        return None;
    }

    let (start_parameter, point, alignment) = (0..32).find_map(|index| {
        let parameter = period * index as f64 / 32.0;
        let point = cutter.point_at(parameter);
        let (u, v) = surface.parameters_at(point)?;
        let plane = match cutter {
            Curve3::Circle(circle) => &circle.plane,
            Curve3::Ellipse(ellipse) => &ellipse.plane,
            Curve3::PlanarSpline { plane, .. } => plane,
            _ => return None,
        };
        let alignment = Vec3::from(plane.normal()?)
            .dot(Vec3::from(surface.normal_at(u, v)?));
        (alignment.is_finite() && alignment != 0.0).then_some((parameter, point, alignment))
    })?;
    let first_forward = (alignment > 0.0) == node.forward;

    let old_edge = first.edge;
    for coedge in old_coedges {
        body.coedges.remove(coedge);
    }
    body.loops.remove(old_loop);
    body.edges.remove(old_edge);

    let seam = body.vertices.insert(Vertex {
        point,
        provenance: Provenance::Synthesized,
    });
    let curve = body.curves.insert(cutter.clone());
    let cut = body.edges.insert(Edge {
        curve,
        start_parameter,
        end_parameter: start_parameter + period,
        start: seam,
        end: seam,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    });

    let first_loop = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let first_pcurve = if first_forward {
        flat_cutter.clone()
    } else {
        reverse_closed_pcurve(flat_cutter)?
    };
    let first_coedge = body.coedges.insert(Coedge {
        edge: cut,
        forward: first_forward,
        pcurve: Some(first_pcurve),
        owner: first_loop,
        provenance: Provenance::Synthesized,
    });
    body.loops.get_mut(first_loop)?.coedges = vec![first_coedge];
    let kept = body.faces.get_mut(face)?;
    kept.loops = vec![first_loop];
    kept.provenance.soil();

    let other = body.faces.insert(Face {
        surface: node.surface,
        forward: node.forward,
        loops: Vec::new(),
        owner: node.owner,
        provenance: Provenance::Synthesized,
    });
    let other_loop = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: other,
        provenance: Provenance::Synthesized,
    });
    let other_forward = !first_forward;
    let other_pcurve = if other_forward {
        flat_cutter.clone()
    } else {
        reverse_closed_pcurve(flat_cutter)?
    };
    let other_coedge = body.coedges.insert(Coedge {
        edge: cut,
        forward: other_forward,
        pcurve: Some(other_pcurve),
        owner: other_loop,
        provenance: Provenance::Synthesized,
    });
    body.loops.get_mut(other_loop)?.coedges = vec![other_coedge];
    body.faces.get_mut(other)?.loops = vec![other_loop];
    body.shells.get_mut(node.owner)?.faces.push(other);
    body.edges.get_mut(cut)?.coedges = vec![first_coedge, other_coedge];
    body.validate().is_empty().then_some([face, other])
}

pub(super) fn reverse_closed_pcurve(curve: &ocs_geom::geom2d::Curve) -> Option<ocs_geom::geom2d::Curve> {
    use ocs_geom::geom2d::{Curve, Line, Polyline};
    Some(match curve {
        Curve::Line(line) => Curve::Line(Line {
            start: line.end,
            end: line.start,
        }),
        Curve::Polyline(polyline)
            if polyline
                .vertices
                .iter()
                .all(|vertex| vertex.bulge == 0.0) =>
        {
            let mut vertices = polyline.vertices.clone();
            vertices.reverse();
            Curve::Polyline(Polyline {
                vertices,
                closed: polyline.closed,
            })
        }
        Curve::Nurbs(curve) => Curve::Nurbs(curve.reversed()),
        _ => return None,
    })
}

/// Divides a periodic band along a closed section.
/// Which side of a cutter's image `point` lies on, positive to the left of
/// `direction`. A straight image is one cross product; a curved one running
/// round a `u` period is compared at `point`'s own `u`.
fn side_of_image(
    image: &[[f64; 2]],
    direction: Vec3,
    point: [f64; 2],
    period: Option<f64>,
) -> Option<f64> {
    let start = *image.first()?;
    if image.len() == 2 {
        return Some(direction.x * (point[1] - start[1]) - direction.y * (point[0] - start[0]));
    }
    let period = period?;
    let low = image.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let u = low + (point[0] - low).rem_euclid(period);
    let v = image
        .windows(2)
        .find_map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let inside = a[0].min(b[0]) <= u && u <= a[0].max(b[0]) && a[0] != b[0];
            inside.then(|| a[1] + (b[1] - a[1]) * (u - a[0]) / (b[0] - a[0]))
        })
        // At the image's own ends `u` can sit a rounding outside it.
        .or_else(|| {
            let nearest = image.iter().min_by(|a, b| (a[0] - u).abs().total_cmp(&(b[0] - u).abs()));
            nearest.map(|p| p[1])
        })?;
    Some(direction.x * (point[1] - v))
}

fn split_closed_between_loops(
    body: &mut Body,
    face: FaceKey,
    node: &Face,
    surface: &super::geometry::Surface,
    cutter: &Curve3,
    flat_cutter: &ocs_geom::geom2d::Curve,
    boundary_parts: &[(CoedgeKey, ocs_geom::geom2d::Curve)],
    period: f64,
) -> Option<[FaceKey; 2]> {
    // The cutter's image runs once round the band: straight along `u` for
    // a circle round a cylinder, a spline for a traced curve that wavers.
    // Which side of it a loop lies on is read against the image at the
    // loop's own `u`.
    let (start, end) = (flat_cutter.point_at(0.0), flat_cutter.point_at(1.0));
    let mut direction = Vec3::new(end[0] - start[0], end[1] - start[1], 0.0);
    if direction.length() <= f64::EPSILON {
        return None;
    }
    let image: Vec<[f64; 2]> = match flat_cutter {
        ocs_geom::geom2d::Curve::Line(line) => vec![line.start, line.end],
        _ => (0..=128).map(|step| flat_cutter.point_at(step as f64 / 128.0)).collect(),
    };
    // A circle round a cylinder's axis has a flat image running the way `u`
    // does, whichever way the circle itself turns. The cut's edge runs the
    // way the circle does, so that is the way read off here — a circle whose
    // plane faces down the axis otherwise left both rims of a band running
    // one way round.
    let (u0, v0) = surface.parameters_at(cutter.point_at(0.0))?;
    let (u1, v1) = surface.parameters_at(cutter.point_at(period * 1e-3))?;
    let wrapped = |delta: f64, period: Option<f64>| match period {
        Some(period) => delta - period * (delta / period).round(),
        None => delta,
    };
    let periods = pcurve::periods(surface);
    let running = Vec3::new(wrapped(u1 - u0, periods[0]), wrapped(v1 - v0, periods[1]), 0.0);
    if running.dot(direction) < 0.0 {
        direction = direction * -1.0;
    }

    let mut negative = Vec::new();
    let mut positive = Vec::new();
    // How far round each loop runs along the cut, in all: a rim a whole turn
    // one way or the other, a hole nothing.
    let mut negative_turn = 0.0;
    let mut positive_turn = 0.0;
    // A cone's apex is a loop with no edge: it has no point of its own to
    // place, but it can only be on the side the rims leave empty.
    let mut singular = Vec::new();
    for ring in &node.loops {
        let coedges = &body.loops.get(*ring)?.coedges;
        let Some(&first) = coedges.first() else {
            singular.push(*ring);
            continue;
        };
        let boundary = boundary_parts.iter().find(|(key, _)| *key == first)?.1.clone();
        let point = boundary.point_at(0.5);
        let turn: f64 = coedges
            .iter()
            .filter_map(|coedge| boundary_parts.iter().find(|(key, _)| key == coedge))
            .map(|(_, piece)| {
                let (from, to) = (piece.point_at(0.0), piece.point_at(1.0));
                Vec3::new(to[0] - from[0], to[1] - from[1], 0.0).dot(direction)
            })
            .sum();
        // A cut running round the other way — round a torus's tube rather
        // than its ring — is read with the two parameters exchanged.
        let side = if direction.y.abs() > direction.x.abs() {
            let swapped: Vec<[f64; 2]> = image.iter().map(|p| [p[1], p[0]]).collect();
            side_of_image(
                &swapped,
                Vec3::new(direction.y, direction.x, 0.0),
                [point[1], point[0]],
                pcurve::periods(surface)[1],
            )?
        } else {
            side_of_image(&image, direction, point, pcurve::periods(surface)[0])?
        };
        if side < 0.0 {
            negative.push(*ring);
            negative_turn += turn;
        } else if side > 0.0 {
            positive.push(*ring);
            positive_turn += turn;
        } else {
            return None;
        }
    }
    if !singular.is_empty() {
        match (negative.is_empty(), positive.is_empty()) {
            (true, false) => negative = singular,
            (false, true) => positive = singular,
            _ => return None,
        }
    }
    if negative.is_empty() || positive.is_empty() {
        return None;
    }
    // Each half is a band: its new rim runs against the rim it keeps. The
    // cut's edge runs the way `direction` does, so its use in the negative
    // half is forward exactly when that half's own rim runs the other way.
    // A half that is only an apex has no rim; its cut runs against the
    // other half's, as a rim of its own would.
    let negative_turn = if negative_turn == 0.0 { -positive_turn } else { negative_turn };
    if negative_turn * positive_turn > 0.0 || negative_turn == 0.0 {
        return None;
    }
    let (kept, moved, cut_forward) = (negative, positive, negative_turn < 0.0);

    let seam = body.vertices.insert(Vertex {
        point: cutter.point_at(0.0),
        provenance: Provenance::Synthesized,
    });
    let curve = body.curves.insert(cutter.clone());
    let cut = body.edges.insert(Edge {
        curve,
        start_parameter: 0.0,
        end_parameter: period,
        start: seam,
        end: seam,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    });

    let kept_ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let kept_coedge = body.coedges.insert(Coedge {
        edge: cut,
        forward: cut_forward,
        pcurve: None,
        owner: kept_ring,
        provenance: Provenance::Synthesized,
    });
    body.loops.get_mut(kept_ring)?.coedges = vec![kept_coedge];

    let other = body.faces.insert(Face {
        surface: node.surface,
        forward: node.forward,
        loops: Vec::new(),
        owner: node.owner,
        provenance: Provenance::Synthesized,
    });
    let moved_ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: other,
        provenance: Provenance::Synthesized,
    });
    let moved_coedge = body.coedges.insert(Coedge {
        edge: cut,
        forward: !cut_forward,
        pcurve: None,
        owner: moved_ring,
        provenance: Provenance::Synthesized,
    });
    body.loops.get_mut(moved_ring)?.coedges = vec![moved_coedge];
    body.edges.get_mut(cut)?.coedges = vec![kept_coedge, moved_coedge];

    for ring in &kept {
        body.loops.get_mut(*ring)?.owner = face;
    }
    for ring in &moved {
        body.loops.get_mut(*ring)?.owner = other;
    }
    let mut kept_loops = kept;
    kept_loops.push(kept_ring);
    let mut moved_loops = moved;
    moved_loops.push(moved_ring);
    let kept_face = body.faces.get_mut(face)?;
    kept_face.loops = kept_loops;
    kept_face.provenance.soil();
    body.faces.get_mut(other)?.loops = moved_loops;
    body.shells.get_mut(node.owner)?.faces.push(other);
    Some([face, other])
}

/// A crossing read off sampled parameter-space images, moved onto both
/// curves it is where: a spline's image is a fit off the spline, and a
/// corner left there is a fit off the edge the other body cuts along it.
fn onto_both(edge: &Curve3, cutter: &Curve3, point: [f64; 3], tolerance: f64) -> [f64; 3] {
    // Newton on edge(s) = cutter(t), in least squares: both run on one
    // surface, so where they cross the residual vanishes.
    let speed = |curve: &Curve3, at: f64| {
        let step = 1e-6 * at.abs().max(1.0);
        (Vec3::from(curve.point_at(at + step)) - Vec3::from(curve.point_at(at - step)))
            / (2.0 * step)
    };
    let (mut s, mut t) = (edge.parameter_at(point), cutter.parameter_at(point));
    let first_gap = Vec3::from(point).distance(Vec3::from(cutter.point_at(t)));
    for _ in 0..32 {
        let gap = Vec3::from(edge.point_at(s)) - Vec3::from(cutter.point_at(t));
        if gap.length() <= tolerance * 0.01 {
            break;
        }
        let (along, across) = (speed(edge, s), -speed(cutter, t));
        let (aa, ab, bb) = (along.dot(along), along.dot(across), across.dot(across));
        let det = aa * bb - ab * ab;
        if det.abs() <= f64::EPSILON * aa * bb {
            return point;
        }
        let (ra, rb) = (along.dot(gap), across.dot(gap));
        s -= (bb * ra - ab * rb) / det;
        t -= (aa * rb - ab * ra) / det;
    }
    let landed = edge.point_at(s);
    // A step that ran off to another crossing is no refinement. A traced
    // cutter passes the edge to within its fit, a little more than the
    // tolerance, still far nearer than the sampled crossing was.
    let gap = Vec3::from(landed).distance(Vec3::from(cutter.point_at(t)));
    if Vec3::from(landed).distance(Vec3::from(point)) > 0.1 * Vec3::from(point).length().max(1.0)
        || (gap > tolerance && gap * 100.0 > first_gap)
    {
        return point;
    }
    landed
}

fn closed_period(curve: &Curve3) -> Option<f64> {
    match curve {
        Curve3::Circle(_) | Curve3::Ellipse(_) => Some(TAU),
        Curve3::PlanarSpline { curve, .. } if curve.is_closed() => Some(1.0),
        Curve3::Nurbs(curve) if curve.periodicity() => {
            let (start, end) = curve.domain();
            (end > start).then_some(end - start)
        }
        _ => None,
    }
}

pub(super) fn periodic_images(
    curve: &ocs_geom::geom2d::Curve,
    periods: [Option<f64>; 2],
) -> Vec<ocs_geom::geom2d::Curve> {
    let turns = |period: Option<f64>| match period {
        Some(period) => (-2..=2).map(|turn| period * f64::from(turn)).collect(),
        None => vec![0.0],
    };
    let mut out = Vec::new();
    for u in turns(periods[0]) {
        for v in turns(periods[1]) {
            if let Some(moved) = curve.transformed(&Transform::translation([u, v])) {
                out.push(moved);
            }
        }
    }
    out
}

fn periodic_points(point: [f64; 2], periods: [Option<f64>; 2]) -> Vec<[f64; 2]> {
    let turns = |period: Option<f64>| match period {
        Some(period) => (-2..=2).map(|turn| period * f64::from(turn)).collect(),
        None => vec![0.0],
    };
    let mut out = Vec::new();
    for u in turns(periods[0]) {
        for v in turns(periods[1]) {
            out.push([point[0] + u, point[1] + v]);
        }
    }
    out
}

fn parameter_in_span(curve: &Curve3, point: [f64; 3], start: f64, end: f64) -> f64 {
    let parameter = curve.parameter_at(point);
    let Some(period) = closed_period(curve) else {
        return parameter;
    };
    let middle = 0.5 * (start + end);
    parameter + period * ((middle - parameter) / period).round()
}

/// Whether the near arc's closer runs with the cutter: true when the arc
/// lies on the left of the cut in the surface's parameter space, where a
/// forward face keeps its region.
fn near_side_forward(
    body: &Body,
    surface: &super::geometry::Surface,
    cutter: &Curve3,
    at: f64,
    near: &[CoedgeKey],
    face_forward: bool,
) -> Option<bool> {
    let period = pcurve::periods(surface)[0];
    let unwind = |value: f64, beside: f64| match period {
        Some(period) => value + period * ((beside - value) / period).round(),
        None => value,
    };
    let sample = body.coedges.get(*near.get(near.len() / 2)?)?;
    let edge = body.edges.get(sample.edge)?;
    let point = body
        .curves
        .get(edge.curve)?
        .point_at(0.5 * (edge.start_parameter + edge.end_parameter));
    let (u, v) = surface.parameters_at(point)?;
    // Read against the cutter where it passes the sample, not where it
    // starts: half a turn round, which way `u` runs from the start is a
    // coin toss, and it decided the side.
    let period_length = closed_period(cutter).unwrap_or(1.0);
    let (_, nearest) = (0..64)
        .filter_map(|step| {
            let t = at + period_length * step as f64 / 64.0;
            let (cu, cv) = surface.parameters_at(cutter.point_at(t))?;
            Some(((unwind(cu, u) - u).abs(), (t, cu, cv)))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let (t, u0, v0) = nearest;
    let (u1, v1) = surface.parameters_at(cutter.point_at(t + 1.0e-4))?;
    let along = [unwind(u1, u0) - u0, v1 - v0];
    let side = along[0] * (v - v0) - along[1] * (unwind(u, u0) - u0);
    (side != 0.0).then_some((side > 0.0) == face_forward)
}

/// Whether `point` lies on the edge under `coedge`, within its span.
fn on_edge(body: &Body, coedge: CoedgeKey, point: [f64; 3], tolerance: f64) -> Option<bool> {
    let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
    let curve = body.curves.get(edge.curve)?;
    let parameter = parameter_in_span(curve, point, edge.start_parameter, edge.end_parameter);
    let (low, high) = (
        edge.start_parameter.min(edge.end_parameter),
        edge.start_parameter.max(edge.end_parameter),
    );
    Some(
        parameter >= low
            && parameter <= high
            && Vec3::from(curve.point_at(parameter)).distance(Vec3::from(point)) <= tolerance,
    )
}

/// Whether a point of a cutter lies on the face's boundary already. A spline
/// on either side is held to its fit rather than the modelling tolerance: a
/// file's spline and the curve it stands for run a hair apart, and a cut
/// along one beside the other only peels off a sliver between them.
fn on_boundary(
    body: &Body,
    boundary_parts: &[(CoedgeKey, ocs_geom::geom2d::Curve)],
    cutter: &Curve3,
    point: [f64; 3],
    fit: f64,
    tolerance: f64,
) -> bool {
    boundary_parts.iter().any(|(coedge, _)| {
        let spline = matches!(cutter, Curve3::Nurbs(_))
            || body
                .coedges
                .get(*coedge)
                .and_then(|coedge| body.edges.get(coedge.edge))
                .and_then(|edge| body.curves.get(edge.curve))
                .is_some_and(|curve| matches!(curve, Curve3::Nurbs(_)));
        let near = if spline { fit } else { tolerance };
        on_edge(body, *coedge, point, near).unwrap_or(false)
    })
}

/// A box round a curve in (u, v), padded for what a sampling can miss;
/// `None` for one without ends.
fn uv_box(curve: &ocs_geom::geom2d::Curve) -> Option<[[f64; 2]; 2]> {
    if matches!(curve, ocs_geom::geom2d::Curve::XLine(_) | ocs_geom::geom2d::Curve::Ray(_)) {
        return None;
    }
    let points: Vec<[f64; 2]> = match curve {
        ocs_geom::geom2d::Curve::Polyline(polyline)
            if polyline.vertices.iter().all(|vertex| vertex.bulge == 0.0) =>
        {
            polyline.vertices.iter().map(|vertex| vertex.position).collect()
        }
        _ => (0..=32).map(|step| curve.point_at(step as f64 / 32.0)).collect(),
    };
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for point in &points {
        for axis in 0..2 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    if !low.iter().chain(&high).all(|value| value.is_finite()) {
        return None;
    }
    let pad = ((high[0] - low[0]).hypot(high[1] - low[1])) * 0.05 + 1e-9;
    Some([[low[0] - pad, low[1] - pad], [high[0] + pad, high[1] + pad]])
}

/// Whether two curves are one circle, whichever way round each runs.
pub(super) fn same_circle(one: &Curve3, other: &Curve3, tolerance: f64) -> bool {
    let (Curve3::Circle(one), Curve3::Circle(other)) = (one, other) else {
        return false;
    };
    let (Some(first), Some(second)) = (one.plane.normal(), other.plane.normal()) else {
        return false;
    };
    (one.radius - other.radius).abs() <= tolerance
        && Vec3::from(one.plane.origin).distance(Vec3::from(other.plane.origin)) <= tolerance
        && Vec3::from(first).is_parallel_to(Vec3::from(second), tolerance)
}

/// Where two circles lying on `sphere` cross: the points where the line
/// common to their planes meets it. None for circles on parallel planes,
/// which either coincide or never meet.
fn circles_meet_on_sphere(
    sphere: &super::geometry::Sphere,
    one: &super::geometry::Circle3,
    other: &super::geometry::Circle3,
    tolerance: f64,
) -> Vec<[f64; 3]> {
    let (Some(first), Some(second)) = (one.plane.normal(), other.plane.normal()) else {
        return Vec::new();
    };
    let (first, second) = (Vec3::from(first), Vec3::from(second));
    if first.cross(second).length() <= tolerance {
        return Vec::new();
    }
    let Some(along) = first.cross(second).normalize() else {
        return Vec::new();
    };
    let cosine = first.dot(second);
    let offsets = [
        first.dot(Vec3::from(one.plane.origin)),
        second.dot(Vec3::from(other.plane.origin)),
    ];
    let square = 1.0 - cosine * cosine;
    let base = first * ((offsets[0] - offsets[1] * cosine) / square)
        + second * ((offsets[1] - offsets[0] * cosine) / square);
    let from_centre = base - Vec3::from(sphere.frame.origin);
    let half = along.dot(from_centre);
    let discriminant = half * half - (from_centre.dot(from_centre) - sphere.radius * sphere.radius);
    if discriminant < -tolerance * sphere.radius {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let mut points = vec![(base + along * (-half - root)).to_array()];
    if root > tolerance {
        points.push((base + along * (-half + root)).to_array());
    }
    points
}

/// Where the cut met the boundary.
#[derive(Clone)]
struct Landing {
    edge: EdgeKey,
    coedge: CoedgeKey,
    point: [f64; 3],
}

/// Where a closed cutter lying in a face touches the face's boundary: per
/// edge, the boundary point nearest the cutter when that is within the
/// tolerance of it, one landing for each place touched.
fn boundary_touches(
    body: &Body,
    boundary_parts: &[(CoedgeKey, ocs_geom::geom2d::Curve)],
    cutter: &Curve3,
    period: f64,
    tolerance: f64,
) -> Vec<Landing> {
    let mut found: Vec<Landing> = Vec::new();
    // Boxes first: an edge the cutter's box does not reach it cannot touch,
    // and the search along the cutter is the costly part.
    let boxed = |points: &mut dyn Iterator<Item = [f64; 3]>| {
        points.fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(low, high), p| {
                (
                    std::array::from_fn(|i| low[i].min(p[i])),
                    std::array::from_fn(|i| high[i].max(p[i])),
                )
            },
        )
    };
    let reach = |(low, high): ([f64; 3], [f64; 3]), grow: f64| {
        (low.map(|v| v - grow), high.map(|v| v + grow))
    };
    let samples = 64;
    let cutter_box = boxed(&mut (0..=samples).map(|step| {
        cutter.point_at(period * step as f64 / samples as f64)
    }));
    let size = Vec3::from(cutter_box.0).distance(Vec3::from(cutter_box.1));
    // Chords cut inside a curve by up to its bend; a tenth of the size
    // covers that at 64 samples.
    let (cut_low, cut_high) = reach(cutter_box, size * 0.1 + tolerance);
    for (coedge, _) in boundary_parts {
        let Some(edge_key) = body.coedges.get(*coedge).map(|coedge| coedge.edge) else {
            continue;
        };
        let Some(edge) = body.edges.get(edge_key) else { continue };
        let Some(curve) = body.curves.get(edge.curve) else { continue };
        if same_circle(cutter, curve, tolerance) || curve == cutter {
            continue;
        }
        let (low, high) = (
            edge.start_parameter.min(edge.end_parameter),
            edge.start_parameter.max(edge.end_parameter),
        );
        let (edge_low, edge_high) = boxed(&mut (0..=16).map(|step| {
            curve.point_at(low + (high - low) * step as f64 / 16.0)
        }));
        let edge_size = Vec3::from(edge_low).distance(Vec3::from(edge_high));
        let (edge_low, edge_high) = reach((edge_low, edge_high), edge_size * 0.1);
        if (0..3).any(|i| edge_low[i] > cut_high[i] || edge_high[i] < cut_low[i]) {
            continue;
        }
        // Nearest point of the edge to a point of the cutter, and how far.
        let gap = |t: f64| {
            let point = cutter.point_at(t);
            let on = curve.point_at(curve.parameter_at(point).clamp(low, high));
            (Vec3::from(point).distance(Vec3::from(on)), on)
        };
        let steps = 32;
        let Some(coarse) = (0..steps)
            .map(|step| period * step as f64 / steps as f64)
            .min_by(|a, b| gap(*a).0.total_cmp(&gap(*b).0))
        else {
            continue;
        };
        let step = period / steps as f64;
        let (mut from, mut to) = (coarse - step, coarse + step);
        for _ in 0..60 {
            let (one, two) = (from + (to - from) / 3.0, to - (to - from) / 3.0);
            if gap(one).0 < gap(two).0 {
                to = two;
            } else {
                from = one;
            }
        }
        let (distance, on) = gap(0.5 * (from + to));
        let seen = found
            .iter()
            .any(|landing| Vec3::from(landing.point).distance(Vec3::from(on)) <= tolerance);
        if distance <= tolerance && !seen {
            found.push(Landing { edge: edge_key, coedge: *coedge, point: on });
        }
    }
    found
}

/// The vertex at a landing: an existing end of the edge when the cut runs
/// into a corner, otherwise a new one from splitting the edge there.
fn vertex_at(body: &mut Body, landing: &Landing, tolerance: f64) -> Option<VertexKey> {
    let curve_key = body.edges.get(landing.edge)?.curve;
    let curve = body.curves.get(curve_key)?.clone();
    let candidates: Vec<EdgeKey> = body
        .edges
        .iter()
        .filter_map(|(key, edge)| (edge.curve == curve_key).then_some(key))
        .collect();
    // A corner already there is the landing, whichever piece of the curve
    // it ends: splitting an earlier piece first put a second corner on it.
    for candidate in &candidates {
        let edge = body.edges.get(*candidate)?;
        for end in [edge.start, edge.end] {
            let point = body.vertices.get(end)?.point;
            if Vec3::from(point).distance(Vec3::from(landing.point)) <= tolerance {
                return Some(end);
            }
        }
    }
    for candidate in candidates {
        let edge = body.edges.get(candidate)?.clone();
        let parameter = parameter_in_span(
            &curve,
            landing.point,
            edge.start_parameter,
            edge.end_parameter,
        );
        let low = edge.start_parameter.min(edge.end_parameter);
        let high = edge.start_parameter.max(edge.end_parameter);
        if parameter > low && parameter < high {
            let (near, far) = split_edge(body, candidate, parameter)?;
            return shared_vertex(body, near, far);
        }
    }
    None
}

/// The vertex an edge split introduced, given the two halves.
pub fn shared_vertex(body: &Body, near: EdgeKey, far: EdgeKey) -> Option<VertexKey> {
    let near = body.edges.get(near)?;
    let far = body.edges.get(far)?;
    if near.end == far.start {
        return Some(near.end);
    }
    [near.start, near.end]
        .into_iter()
        .find(|key| *key == far.start || *key == far.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make::{cuboid, cylinder};
    use ocs_geom::space::Vec3;

    fn box_body() -> Body {
        cuboid([0.0, 0.0, 0.0], [2.0, 4.0, 6.0]).expect("a box")
    }

    #[test]
    fn splitting_an_edge_leaves_the_body_consistent() {
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        split_edge(&mut body, edge, 0.5).expect("a split at the middle");
        let flaws = body.validate();
        assert!(flaws.is_empty(), "{flaws:?}");
    }

    #[test]
    fn splitting_an_edge_adds_one_vertex_and_one_edge() {
        // Which leaves V − E + F where it was: the shape has not changed,
        // only how it is written down.
        let mut body = box_body();
        let before = (body.vertices.len(), body.edges.len(), body.faces.len());
        let characteristic = body.euler_characteristic();
        let edge = body.edges.keys().next().unwrap();
        split_edge(&mut body, edge, 0.5).unwrap();
        assert_eq!(body.vertices.len(), before.0 + 1);
        assert_eq!(body.edges.len(), before.1 + 1);
        assert_eq!(body.faces.len(), before.2);
        assert_eq!(body.euler_characteristic(), characteristic);
    }

    #[test]
    fn splitting_a_closed_edge_reports_the_new_vertex() {
        let mut body = cylinder([0.0; 3], 2.0, 4.0).unwrap();
        let edge = body
            .edge_keys()
            .find(|key| {
                let edge = body.edges.get(*key).unwrap();
                edge.start == edge.end && edge.end_parameter > edge.start_parameter
            })
            .unwrap();
        let expected = body
            .curves
            .get(body.edges.get(edge).unwrap().curve)
            .unwrap()
            .point_at(1.0);
        let (near, far) = split_edge(&mut body, edge, 1.0).unwrap();
        let vertex = shared_vertex(&body, near, far).unwrap();
        assert_eq!(body.vertices.get(vertex).unwrap().point, expected);
    }

    #[test]
    fn both_faces_that_used_the_edge_gain_a_coedge() {
        // The one that is easy to forget. Splitting only the near face's loop
        // leaves the far one a coedge short, its ring open, and the failure
        // turns up much later as a boolean losing a wall.
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        let faces: Vec<_> = body
            .edges
            .get(edge)
            .unwrap()
            .coedges
            .iter()
            .map(|c| {
                let owner = body.coedges.get(*c).unwrap().owner;
                body.loops.get(owner).unwrap().owner
            })
            .collect();
        assert_eq!(faces.len(), 2);
        let before: Vec<usize> = faces.iter().map(|f| body.face_coedges(*f).len()).collect();
        split_edge(&mut body, edge, 0.5).unwrap();
        for (face, was) in faces.iter().zip(before) {
            assert_eq!(body.face_coedges(*face).len(), was + 1, "face {face:?}");
        }
    }

    #[test]
    fn the_new_vertex_sits_on_the_curve_where_it_was_asked_for() {
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        let expected = {
            let node = body.edges.get(edge).unwrap();
            body.curves.get(node.curve).unwrap().point_at(0.25)
        };
        let (near, far) = split_edge(&mut body, edge, 0.25).unwrap();
        let vertex = shared_vertex(&body, near, far).unwrap();
        let point = body.vertices.get(vertex).unwrap().point;
        assert!(Vec3::from(point).distance(Vec3::from(expected)) < 1e-12);
        assert!(body.worst_vertex_gap() < 1e-12);
    }

    #[test]
    fn the_halves_run_the_same_way_the_whole_did() {
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        let (start, end) = {
            let node = body.edges.get(edge).unwrap();
            (node.start, node.end)
        };
        let (near, far) = split_edge(&mut body, edge, 0.5).unwrap();
        assert_eq!(body.edges.get(near).unwrap().start, start);
        assert_eq!(body.edges.get(far).unwrap().end, end);
        let middle = shared_vertex(&body, near, far).unwrap();
        assert_eq!(body.edges.get(near).unwrap().end, middle);
        assert_eq!(body.edges.get(far).unwrap().start, middle);
    }

    #[test]
    fn every_loop_still_closes_after_a_split() {
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        split_edge(&mut body, edge, 0.5).unwrap();
        for (key, ring) in body.loops.iter() {
            let count = ring.coedges.len();
            for index in 0..count {
                let (_, ends) = body.coedge_vertices(ring.coedges[index]).unwrap();
                let (begins, _) = body
                    .coedge_vertices(ring.coedges[(index + 1) % count])
                    .unwrap();
                assert_eq!(ends, begins, "loop {key:?} breaks after {index}");
            }
        }
    }

    #[test]
    fn a_backward_coedge_gets_its_new_piece_on_the_right_side() {
        // Every edge of a box is traversed forwards by one face and backwards
        // by the other, so this case is covered by any split — but only if
        // the insertion point depends on the sense. Putting the new coedge
        // after the old one in both cases breaks exactly the backward loop,
        // which is what the closure check above would catch.
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        let backward = body
            .edges
            .get(edge)
            .unwrap()
            .coedges
            .iter()
            .copied()
            .find(|c| !body.coedges.get(*c).unwrap().forward)
            .expect("one side runs against the curve");
        let ring = body.coedges.get(backward).unwrap().owner;
        let before = body.loops.get(ring).unwrap().coedges.clone();
        let at = before.iter().position(|c| *c == backward).unwrap();
        split_edge(&mut body, edge, 0.5).unwrap();
        let after = body.loops.get(ring).unwrap().coedges.clone();
        assert_eq!(after.len(), before.len() + 1);
        assert_eq!(after[at + 1], backward, "the new piece comes first");
    }

    #[test]
    fn splitting_twice_divides_an_edge_into_three() {
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        let (near, far) = split_edge(&mut body, edge, 0.5).unwrap();
        split_edge(&mut body, far, 0.75).unwrap();
        let flaws = body.validate();
        assert!(flaws.is_empty(), "{flaws:?}");
        assert_eq!(body.edges.len(), 14);
        assert_eq!(body.vertices.len(), 10);
        assert_eq!(body.euler_characteristic(), 2);
        assert!(body.edges.contains(near));
    }

    #[test]
    fn a_split_at_an_end_is_refused() {
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        assert!(split_edge(&mut body, edge, 0.0).is_none());
        assert!(split_edge(&mut body, edge, 1.0).is_none());
        assert!(split_edge(&mut body, edge, 1.5).is_none());
        assert!(split_edge(&mut body, edge, -0.5).is_none());
        assert!(body.validate().is_empty(), "nothing was changed");
        assert_eq!(body.edges.len(), 12);
    }

    #[test]
    fn splitting_by_a_point_lands_where_the_point_is() {
        let mut body = box_body();
        let edge = body.edges.keys().next().unwrap();
        let (start, end) = body.edge_endpoints(edge).unwrap();
        let middle = Vec3::from(start).lerp(Vec3::from(end), 0.3).to_array();
        let (near, far) = split_edge_at(&mut body, edge, middle).unwrap();
        let vertex = shared_vertex(&body, near, far).unwrap();
        let point = body.vertices.get(vertex).unwrap().point;
        assert!(Vec3::from(point).distance(Vec3::from(middle)) < 1e-12);
        assert!(body.validate().is_empty());
    }


    /// The face of the box lying on z = 0, and its surface.
    fn bottom_face(body: &Body) -> FaceKey {
        body.face_keys()
            .find(|key| {
                let face = body.faces.get(*key).unwrap();
                let crate::Surface::Plane(plane) =
                    body.surfaces.get(face.surface).unwrap()
                else {
                    unreachable!()
                };
                plane.origin[2].abs() < 1e-9 && plane.normal().unwrap()[2].abs() > 0.9
            })
            .expect("a box has a bottom")
    }

    /// A line across the box's bottom face at constant y, running in x.
    fn across_bottom(y: f64) -> Curve3 {
        Curve3::Line(crate::Line3 {
            origin: [-1.0, y, 0.0],
            direction: [1.0, 0.0, 0.0],
        })
    }

    #[test]
    fn cutting_a_face_leaves_two_and_the_body_consistent() {
        let mut body = box_body();
        let face = bottom_face(&body);
        let [kept, made] = split_face(&mut body, face, &across_bottom(2.0), 1e-9)
            .expect("a cut straight across");
        assert_eq!(kept, face);
        assert_ne!(made, face);
        assert_eq!(body.faces.len(), 7);
        let flaws = body.validate();
        assert!(flaws.is_empty(), "{flaws:?}");
    }

    #[test]
    fn a_cut_face_is_still_a_closed_solid() {
        // Two vertices, three edges and one face are added, which leaves
        // V − E + F alone: the shape has not changed, only its description.
        let mut body = box_body();
        let before = body.euler_characteristic();
        let face = bottom_face(&body);
        split_face(&mut body, face, &across_bottom(2.0), 1e-9).unwrap();
        assert_eq!(body.euler_characteristic(), before);
        assert_eq!(body.vertices.len(), 10);
        assert_eq!(body.edges.len(), 15);
        assert_eq!(body.faces.len(), 7);
    }

    #[test]
    fn both_halves_lie_on_the_surface_the_face_did() {
        let mut body = box_body();
        let face = bottom_face(&body);
        let surface = body.faces.get(face).unwrap().surface;
        let [kept, made] = split_face(&mut body, face, &across_bottom(2.0), 1e-9).unwrap();
        assert_eq!(body.faces.get(kept).unwrap().surface, surface);
        assert_eq!(body.faces.get(made).unwrap().surface, surface);
        // And in the same shell, so the solid is still one piece.
        assert_eq!(
            body.faces.get(kept).unwrap().owner,
            body.faces.get(made).unwrap().owner
        );
    }

    #[test]
    fn the_two_halves_share_the_new_edge_running_it_opposite_ways() {
        // What makes it a division rather than two coincident walls.
        let mut body = box_body();
        let face = bottom_face(&body);
        let before: Vec<_> = body.edges.keys().collect();
        split_face(&mut body, face, &across_bottom(2.0), 1e-9).unwrap();
        let cut = body
            .edges
            .keys()
            .find(|key| !before.contains(key) && body.edges.get(*key).unwrap().coedges.len() == 2)
            .expect("a new shared edge");
        let senses: Vec<bool> = body
            .edges
            .get(cut)
            .unwrap()
            .coedges
            .iter()
            .map(|c| body.coedges.get(*c).unwrap().forward)
            .collect();
        assert_ne!(senses[0], senses[1]);
    }

    #[test]
    fn the_halves_add_up_to_the_whole() {
        // Four coedges before; after the cut each half has three of the
        // original's pieces plus the cut, and the two edges it crossed have
        // each become two.
        let mut body = box_body();
        let face = bottom_face(&body);
        let [kept, made] = split_face(&mut body, face, &across_bottom(2.0), 1e-9).unwrap();
        assert_eq!(body.face_coedges(kept).len(), 4);
        assert_eq!(body.face_coedges(made).len(), 4);
    }

    #[test]
    fn a_cut_from_corner_to_corner_uses_the_corners_it_finds() {
        // The diagonal of the bottom face. Both ends land exactly on
        // existing vertices, so nothing is split and no vertex is added —
        // which a version that always split would get wrong by leaving two
        // zero-length edges behind.
        let mut body = cuboid([0.0; 3], [4.0, 4.0, 4.0]).unwrap();
        let face = bottom_face(&body);
        let diagonal = Curve3::Line(crate::Line3 {
            origin: [0.0, 0.0, 0.0],
            direction: [1.0, 1.0, 0.0],
        });
        let vertices = body.vertices.len();
        split_face(&mut body, face, &diagonal, 1e-9).expect("a diagonal cut");
        assert_eq!(body.vertices.len(), vertices, "no new corners were needed");
        assert_eq!(body.edges.len(), 13, "only the cut itself");
        let flaws = body.validate();
        assert!(flaws.is_empty(), "{flaws:?}");
    }

    #[test]
    fn a_cut_that_misses_the_face_does_nothing() {
        let mut body = box_body();
        let face = bottom_face(&body);
        let before = body.faces.len();
        // Well outside the box's footprint.
        assert!(split_face(&mut body, face, &across_bottom(99.0), 1e-9).is_none());
        assert_eq!(body.faces.len(), before);
        assert!(body.validate().is_empty());
    }

    #[test]
    fn a_cutter_off_the_surface_is_refused() {
        let mut body = box_body();
        let face = bottom_face(&body);
        let above = Curve3::Line(crate::Line3 {
            origin: [-1.0, 2.0, 1.0],
            direction: [1.0, 0.0, 0.0],
        });
        assert!(split_face(&mut body, face, &above, 1e-9).is_none());
        assert!(body.validate().is_empty());
    }

    #[test]
    fn cutting_twice_gives_three_pieces() {
        let mut body = cuboid([0.0; 3], [9.0, 9.0, 9.0]).unwrap();
        let face = bottom_face(&body);
        let [kept, _] = split_face(&mut body, face, &across_bottom(3.0), 1e-9).unwrap();
        // The half that still reaches y = 6 is cut again.
        let second = [kept]
            .into_iter()
            .chain(body.face_keys())
            .find(|key| split_face(&mut body.clone(), *key, &across_bottom(6.0), 1e-9).is_some())
            .expect("one half still spans y = 6");
        split_face(&mut body, second, &across_bottom(6.0), 1e-9).unwrap();
        assert_eq!(body.faces.len(), 8, "six sides, cut twice");
        let flaws = body.validate();
        assert!(flaws.is_empty(), "{flaws:?}");
        assert_eq!(body.euler_characteristic(), 2);
    }

    #[test]
    fn a_cut_at_survey_coordinates_works_the_same() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let mut body = cuboid(origin, [2.0, 4.0, 6.0]).unwrap();
        let face = body
            .face_keys()
            .find(|key| {
                let face = body.faces.get(*key).unwrap();
                let crate::Surface::Plane(plane) =
                    body.surfaces.get(face.surface).unwrap()
                else {
                    unreachable!()
                };
                (plane.origin[2] - origin[2]).abs() < 1e-6
                    && plane.normal().unwrap()[2].abs() > 0.9
            })
            .unwrap();
        let cutter = Curve3::Line(crate::Line3 {
            origin: [origin[0] - 1.0, origin[1] + 2.0, origin[2]],
            direction: [1.0, 0.0, 0.0],
        });
        split_face(&mut body, face, &cutter, 1e-6).expect("a cut at survey coordinates");
        let flaws = body.validate();
        assert!(flaws.is_empty(), "{flaws:?}");
        assert!(body.worst_vertex_gap() < 1e-6);
    }

    #[test]
    fn a_face_with_holes_is_not_guessed_at() {
        let mut body = box_body();
        let face = bottom_face(&body);
        // Give it a second loop, standing in for a hole.
        let extra = body.loops.insert(Loop {
            coedges: Vec::new(),
            owner: face,
            provenance: Provenance::Synthesized,
        });
        body.faces.get_mut(face).unwrap().loops.push(extra);
        assert!(split_face(&mut body, face, &across_bottom(2.0), 1e-9).is_none());
    }

    #[test]
    fn a_split_dirties_what_it_touched_and_leaves_the_rest_clean() {
        let mut body = box_body();
        for node in body.edges.values_mut() {
            node.provenance = Provenance::Clean(crate::SourceRef::new(0));
        }
        for node in body.faces.values_mut() {
            node.provenance = Provenance::Clean(crate::SourceRef::new(0));
        }
        let edge = body.edges.keys().next().unwrap();
        split_edge(&mut body, edge, 0.5).unwrap();
        let dirty_faces = body
            .faces
            .iter()
            .filter(|(_, f)| !f.provenance.is_reusable())
            .count();
        assert_eq!(dirty_faces, 2, "only the two the edge bounded");
        let clean_edges = body
            .edges
            .iter()
            .filter(|(_, e)| e.provenance.is_reusable())
            .count();
        assert_eq!(clean_edges, 11, "the other eleven are untouched");
    }
}
