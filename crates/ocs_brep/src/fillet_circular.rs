//! Exact circular-edge blends of complete coaxial analytic solids.

use super::blend::FilletError;
use super::{Body, Curve3, EdgeKey, Surface};
use ocs_geom::geom2d::{fillet_between_rays, intersect, Arc, Curve, Line, Tolerance, Vec2};
use ocs_geom::space::{Plane, Vec3};
use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};

/// Recognizes a closed meridian made from planes, cylinders, cones and tori.
/// An unrecognized body stays available to the other blend solvers.
pub(super) fn fillet_circular(
    body: &Body,
    selected: &[EdgeKey],
    radius: f64,
) -> Option<Result<Body, FilletError>> {
    let first = body.edges.get(*selected.first()?)?;
    let Curve3::Circle(circle) = body.curves.get(first.curve)? else { return None };
    let origin = Vec3::from(circle.plane.origin);
    let axis = Vec3::from(circle.plane.normal()?);
    let radial = Vec3::from(circle.plane.x_axis).normalize()?;
    let plane = Plane::orthonormal(origin.to_array(), radial.to_array(), radial.cross(axis).to_array())?;
    let tolerance = super::operation_tolerance(&[body]);
    if body.roots.len() != 1 || body.shells.len() != 1 || body.lumps.len() != 1 {
        return None;
    }

    let mut points = Vec::<[f64; 2]>::new();
    let mut circles = HashMap::new();
    for (key, edge) in body.edges.iter() {
        if edge.coedges.len() != 2 { return None; }
        let owners = edge.coedges.iter().map(|key| {
            body.coedges.get(*key).and_then(|coedge| body.loops.get(coedge.owner)).map(|ring| ring.owner)
        }).collect::<Option<Vec<_>>>()?;
        if owners[0] == owners[1] {
            // Line generators and circular torus meridians are periodic seams.
            if matches!(body.curves.get(edge.curve)?, Curve3::Line(_) | Curve3::Circle(_)) { continue; }
            return None;
        }
        match body.curves.get(edge.curve)? {
            Curve3::Circle(value) => {
                if (edge.end_parameter - edge.start_parameter).abs() < TAU - 1e-8
                    || (edge.end_parameter - edge.start_parameter).abs() > TAU + 1e-8
                    || Vec3::from(value.plane.normal()?).dot(axis).abs() < 1.0 - 1e-9
                { return None; }
                let delta = Vec3::from(value.plane.origin) - origin;
                let height = delta.dot(axis);
                if (delta - axis * height).length() > tolerance { return None; }
                circles.insert(key, points.len());
                points.push([value.radius, height]);
            }
            _ => return None,
        }
    }
    if selected.iter().any(|key| !circles.contains_key(key)) { return None; }
    let mut links = Vec::<(usize, usize)>::new();
    let mut preserved = HashMap::new();
    let mut axis_nodes = Vec::new();
    for (face_key, face) in body.faces.iter() {
        let surface = body.surfaces.get(face.surface)?;
        let surface_frame = match surface {
            Surface::Plane(value) => value,
            Surface::Cylinder(value) => &value.base,
            Surface::Cone(value) => &value.base,
            Surface::Torus(value) => &value.frame,
            _ => return None,
        };
        if Vec3::from(surface_frame.normal()?).dot(axis).abs() < 1.0 - 1e-9 { return None; }
        if !matches!(surface, Surface::Plane(_)) {
            let delta = Vec3::from(surface_frame.origin) - origin;
            if (delta - axis * delta.dot(axis)).length() > tolerance { return None; }
        }
        let mut nodes = body.face_coedges(face_key).into_iter().filter_map(|coedge| {
            circles.get(&body.coedges.get(coedge)?.edge).copied()
        }).collect::<Vec<_>>();
        nodes.sort_unstable();
        nodes.dedup();
        if nodes.len() == 1 {
            let height = match surface {
                Surface::Plane(_) => points[nodes[0]][1],
                Surface::Cone(cone) if cone.half_angle.tan().abs() > 1e-12 => {
                    let local_axis = Vec3::from(cone.base.normal()?);
                    let apex = Vec3::from(cone.base.origin) + local_axis * (cone.radius / cone.half_angle.tan());
                    let height = (apex - origin).dot(axis);
                    if !body.face_coedges(face_key).into_iter().any(|coedge| {
                        let Some(edge) = body.coedges.get(coedge).and_then(|value| body.edges.get(value.edge)) else { return false };
                        [edge.start, edge.end].iter().any(|key| body.vertices.get(*key).is_some_and(|vertex| Vec3::from(vertex.point).distance(apex) <= tolerance))
                    }) { return None; }
                    height
                }
                _ => return None,
            };
            nodes.push(points.len());
            axis_nodes.push(points.len());
            points.push([0.0, height]);
        }
        if nodes.len() != 2 { return None; }
        let meridian = if let Surface::Torus(torus) = surface {
            if torus.major_radius <= tolerance { return None; }
            let centre = [torus.major_radius, (Vec3::from(torus.frame.origin) - origin).dot(axis)];
            let angle = |node: usize| (points[node][1] - centre[1]).atan2(points[node][0] - centre[0]);
            let mut arc = Arc { centre, radius: torus.minor_radius, start_angle: angle(nodes[0]), end_angle: angle(nodes[1]) };
            let seam = body.face_coedges(face_key).into_iter().find_map(|coedge| {
                let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
                if circles.contains_key(&body.coedges.get(coedge)?.edge) { return None; }
                let curve = body.curves.get(edge.curve)?;
                matches!(curve, Curve3::Circle(_)).then(|| curve.point_at((edge.start_parameter + edge.end_parameter) * 0.5))
            })?;
            let delta = Vec3::from(seam) - origin;
            let height = delta.dot(axis);
            let middle = [(delta - axis * height).length(), height];
            let middle_angle = (middle[1] - centre[1]).atan2(middle[0] - centre[0]);
            if (middle_angle - arc.start_angle).rem_euclid(TAU) > arc.sweep() + 1e-9 {
                std::mem::swap(&mut arc.start_angle, &mut arc.end_angle);
            }
            if (PI - arc.start_angle).rem_euclid(TAU) <= arc.sweep()
                && arc.centre[0] - arc.radius <= tolerance { return None; }
            let curve = Curve::Arc(arc);
            if Vec2::from(curve.point_at(curve.parameter_at(middle))).distance(Vec2::from(middle)) > tolerance { return None; }
            preserved.insert((nodes[0].min(nodes[1]), nodes[0].max(nodes[1])), arc);
            curve
        } else {
            Curve::Line(Line { start: points[nodes[0]], end: points[nodes[1]] })
        };
        // Establish that the reconstructed meridian belongs to this surface.
        for t in [0.0, 0.5, 1.0] {
            let world = plane.point_at(meridian.point_at(t));
            let (u, v) = surface.parameters_at(world)?;
            if Vec3::from(surface.point_at(u, v)).distance(Vec3::from(world)) > tolerance { return None; }
        }
        links.push((nodes[0], nodes[1]));
    }
    match axis_nodes.as_slice() {
        [] => {}
        [first, second] => links.push((*first, *second)),
        _ => return None,
    }
    let mut graph = vec![Vec::new(); points.len()];
    for (a, b) in links {
        graph[a].push(b);
        graph[b].push(a);
    }
    if graph.iter().any(|neighbors| neighbors.len() != 2) { return None; }
    let mut order = vec![0];
    let mut previous = usize::MAX;
    let mut current = 0;
    loop {
        let next = *graph[current].iter().find(|node| **node != previous)?;
        if next == 0 { break; }
        if order.contains(&next) { return None; }
        order.push(next);
        previous = current;
        current = next;
    }
    if order.len() != points.len() { return None; }
    let selected_nodes = selected.iter().map(|key| circles[key]).collect::<HashSet<_>>();
    let originals = (0..order.len()).map(|i| {
        let a = order[i];
        let b = order[(i + 1) % order.len()];
        preserved.get(&(a.min(b), a.max(b))).copied()
    }).collect::<Vec<_>>();
    Some(round_meridian(&points, &order, &selected_nodes, radius, plane, origin, axis, tolerance, &originals))
}

fn round_meridian(
    points: &[[f64; 2]],
    order: &[usize],
    selected: &HashSet<usize>,
    radius: f64,
    plane: Plane,
    origin: Vec3,
    axis: Vec3,
    tolerance: f64,
    originals: &[Option<Arc>],
) -> Result<Body, FilletError> {
    let profile = round_profile(points, order, selected, radius, tolerance, true, originals)?;
    let result = super::revolve(plane, &profile, origin.to_array(), axis.to_array(), TAU)
        .ok_or(FilletError::InvalidResult)?;
    if !result.validate().is_empty() || result.worst_vertex_gap() > tolerance {
        return Err(FilletError::InvalidResult);
    }
    Ok(result)
}

pub(super) fn round_profile(
    points: &[[f64; 2]],
    order: &[usize],
    selected: &HashSet<usize>,
    radius: f64,
    tolerance: f64,
    avoid_axis: bool,
    originals: &[Option<Arc>],
) -> Result<Vec<Curve>, FilletError> {
    let count = order.len();
    let mut incoming = order.iter().map(|node| points[*node]).collect::<Vec<_>>();
    let mut outgoing = incoming.clone();
    let mut arcs = vec![None; count];
    for i in 0..count {
        if !selected.contains(&order[i]) { continue; }
        if originals.get(i).is_some_and(Option::is_some)
            || originals.get((i + count - 1) % count).is_some_and(Option::is_some)
        { return Err(FilletError::UnsupportedExistingFillet); }
        let apex = Vec2::from(points[order[i]]);
        let before = Vec2::from(points[order[(i + count - 1) % count]]) - apex;
        let after = Vec2::from(points[order[(i + 1) % count]]) - apex;
        let fillet = fillet_between_rays(apex.to_array(), before.normalize().ok_or(FilletError::InvalidResult)?.to_array(), after.normalize().ok_or(FilletError::InvalidResult)?.to_array(), radius)
            .ok_or(FilletError::RadiusTooLargeOrInteracting)?;
        if Vec2::from(fillet.tangent1).distance(apex) >= before.length() - tolerance
            || Vec2::from(fillet.tangent2).distance(apex) >= after.length() - tolerance
        { return Err(FilletError::RadiusTooLargeOrInteracting); }
        let arc = Arc { centre: fillet.centre, radius, start_angle: fillet.start_angle, end_angle: fillet.end_angle };
        // A blend crossing the axis requires a singular-surface solver.
        let minimum_radius = if (PI - arc.start_angle).rem_euclid(TAU) <= arc.sweep() {
            arc.centre[0] - radius
        } else { fillet.tangent1[0].min(fillet.tangent2[0]) };
        if avoid_axis && minimum_radius <= tolerance { return Err(FilletError::RadiusTooLargeOrInteracting); }
        incoming[i] = fillet.tangent1;
        outgoing[i] = fillet.tangent2;
        arcs[i] = Some(arc);
    }
    let mut profile = Vec::new();
    for i in 0..count {
        if let Some(arc) = arcs[i] { profile.push(Curve::Arc(arc)); }
        if let Some(Some(arc)) = originals.get(i) {
            profile.push(Curve::Arc(*arc));
            continue;
        }
        let next = (i + 1) % count;
        let original = Vec2::from(points[order[next]]) - Vec2::from(points[order[i]]);
        let remaining = Vec2::from(incoming[next]) - Vec2::from(outgoing[i]);
        if remaining.dot(original) <= tolerance * original.length() {
            return Err(FilletError::RadiusTooLargeOrInteracting);
        }
        profile.push(Curve::Line(Line { start: outgoing[i], end: incoming[next] }));
    }
    for a in 0..profile.len() {
        for b in a + 1..profile.len() {
            let adjacent = b == a + 1 || (a == 0 && b + 1 == profile.len());
            for hit in intersect(&profile[a], &profile[b], Tolerance::new(tolerance)) {
                let endpoint = |curve: &Curve| [0.0, 1.0].iter().any(|t| Vec2::from(curve.point_at(*t)).distance(Vec2::from(hit.point)) <= tolerance);
                if !adjacent || !endpoint(&profile[a]) || !endpoint(&profile[b]) {
                    return Err(FilletError::RadiusTooLargeOrInteracting);
                }
            }
        }
    }
    Ok(profile)
}
