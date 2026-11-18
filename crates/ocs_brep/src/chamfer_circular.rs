//! Exact chamfers of circular edges on complete coaxial analytic solids.

use super::{Body, ChamferError, Curve3, EdgeKey, FaceKey, Surface};
use ocs_geom::geom2d::{Arc, Curve, Line, Vec2};
use ocs_geom::space::{Plane, Vec3};
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

pub(super) fn chamfer_circular(
    body: &Body,
    selected: &[EdgeKey],
    base_face: FaceKey,
    base_distance: f64,
    other_distance: f64,
) -> Option<Result<Body, ChamferError>> {
    let first = body.edges.get(*selected.first()?)?;
    let Curve3::Circle(circle) = body.curves.get(first.curve)? else {
        return None;
    };
    let origin = Vec3::from(circle.plane.origin);
    let axis = Vec3::from(circle.plane.normal()?);
    let radial = Vec3::from(circle.plane.x_axis).normalize()?;
    let plane = Plane::orthonormal(
        origin.to_array(),
        radial.to_array(),
        radial.cross(axis).to_array(),
    )?;
    let tolerance = super::operation_tolerance(&[body]);
    if body.roots.len() != 1 || body.shells.len() != 1 || body.lumps.len() != 1 {
        return None;
    }

    let mut points = Vec::<[f64; 2]>::new();
    let mut circles = HashMap::new();
    for (key, edge) in body.edges.iter() {
        if edge.coedges.len() != 2 {
            return None;
        }
        let owners = edge
            .coedges
            .iter()
            .map(|key| {
                body.coedges
                    .get(*key)
                    .and_then(|coedge| body.loops.get(coedge.owner))
                    .map(|ring| ring.owner)
            })
            .collect::<Option<Vec<_>>>()?;
        if owners[0] == owners[1] {
            if matches!(body.curves.get(edge.curve)?, Curve3::Line(_) | Curve3::Circle(_)) {
                continue;
            }
            return None;
        }
        let Curve3::Circle(value) = body.curves.get(edge.curve)? else {
            return None;
        };
        let span = (edge.end_parameter - edge.start_parameter).abs();
        if (span - TAU).abs() > 1.0e-8
            || Vec3::from(value.plane.normal()?).dot(axis).abs() < 1.0 - 1.0e-9
        {
            return None;
        }
        let delta = Vec3::from(value.plane.origin) - origin;
        let height = delta.dot(axis);
        if (delta - axis * height).length() > tolerance {
            return None;
        }
        circles.insert(key, points.len());
        points.push([value.radius, height]);
    }
    if selected.iter().any(|key| !circles.contains_key(key)) {
        return None;
    }

    let mut links = Vec::<(usize, usize, Option<FaceKey>, Curve)>::new();
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
        if Vec3::from(surface_frame.normal()?).dot(axis).abs() < 1.0 - 1.0e-9 {
            return None;
        }
        if !matches!(surface, Surface::Plane(_)) {
            let delta = Vec3::from(surface_frame.origin) - origin;
            if (delta - axis * delta.dot(axis)).length() > tolerance {
                return None;
            }
        }
        let mut nodes = body
            .face_coedges(face_key)
            .into_iter()
            .filter_map(|coedge| circles.get(&body.coedges.get(coedge)?.edge).copied())
            .collect::<Vec<_>>();
        nodes.sort_unstable();
        nodes.dedup();
        if nodes.len() == 1 {
            let height = match surface {
                Surface::Plane(_) => points[nodes[0]][1],
                Surface::Cone(cone) if cone.half_angle.tan().abs() > 1.0e-12 => {
                    let local_axis = Vec3::from(cone.base.normal()?);
                    let apex = Vec3::from(cone.base.origin)
                        + local_axis * (cone.radius / cone.half_angle.tan());
                    let height = (apex - origin).dot(axis);
                    if !body.face_coedges(face_key).into_iter().any(|coedge| {
                        let Some(edge) = body
                            .coedges
                            .get(coedge)
                            .and_then(|value| body.edges.get(value.edge))
                        else {
                            return false;
                        };
                        [edge.start, edge.end].iter().any(|key| {
                            body.vertices.get(*key).is_some_and(|vertex| {
                                Vec3::from(vertex.point).distance(apex) <= tolerance
                            })
                        })
                    }) {
                        return None;
                    }
                    height
                }
                _ => return None,
            };
            nodes.push(points.len());
            axis_nodes.push(points.len());
            points.push([0.0, height]);
        }
        if nodes.len() != 2 {
            return None;
        }
        let meridian = if let Surface::Torus(torus) = surface {
            if torus.major_radius <= tolerance {
                return None;
            }
            let centre = [
                torus.major_radius,
                (Vec3::from(torus.frame.origin) - origin).dot(axis),
            ];
            let angle = |node: usize| {
                (points[node][1] - centre[1]).atan2(points[node][0] - centre[0])
            };
            let mut arc = Arc {
                centre,
                radius: torus.minor_radius,
                start_angle: angle(nodes[0]),
                end_angle: angle(nodes[1]),
            };
            let seam = body.face_coedges(face_key).into_iter().find_map(|coedge| {
                let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
                if circles.contains_key(&body.coedges.get(coedge)?.edge) {
                    return None;
                }
                let curve = body.curves.get(edge.curve)?;
                matches!(curve, Curve3::Circle(_)).then(|| {
                    curve.point_at((edge.start_parameter + edge.end_parameter) * 0.5)
                })
            })?;
            let delta = Vec3::from(seam) - origin;
            let height = delta.dot(axis);
            let middle = [(delta - axis * height).length(), height];
            let middle_angle =
                (middle[1] - centre[1]).atan2(middle[0] - centre[0]);
            if (middle_angle - arc.start_angle).rem_euclid(TAU) > arc.sweep() + 1.0e-9 {
                std::mem::swap(&mut arc.start_angle, &mut arc.end_angle);
            }
            if (PI - arc.start_angle).rem_euclid(TAU) <= arc.sweep()
                && arc.centre[0] - arc.radius <= tolerance
            {
                return None;
            }
            let curve = Curve::Arc(arc);
            if Vec2::from(curve.point_at(curve.parameter_at(middle)))
                .distance(Vec2::from(middle))
                > tolerance
            {
                return None;
            }
            curve
        } else {
            Curve::Line(Line {
                start: points[nodes[0]],
                end: points[nodes[1]],
            })
        };
        for parameter in [0.0, 0.5, 1.0] {
            let world = plane.point_at(meridian.point_at(parameter));
            let (u, v) = surface.parameters_at(world)?;
            if Vec3::from(surface.point_at(u, v)).distance(Vec3::from(world)) > tolerance {
                return None;
            }
        }
        links.push((nodes[0], nodes[1], Some(face_key), meridian));
    }
    match axis_nodes.as_slice() {
        [] => {}
        [first, second] => links.push((
            *first,
            *second,
            None,
            Curve::Line(Line {
                start: points[*first],
                end: points[*second],
            }),
        )),
        _ => return None,
    }

    let mut graph = vec![Vec::<usize>::new(); points.len()];
    for (first, second, _, _) in &links {
        graph[*first].push(*second);
        graph[*second].push(*first);
    }
    if graph.iter().any(|neighbors| neighbors.len() != 2) {
        return None;
    }
    let mut order = vec![0];
    let mut previous = usize::MAX;
    let mut current = 0;
    loop {
        let next = *graph[current].iter().find(|node| **node != previous)?;
        if next == 0 {
            break;
        }
        if order.contains(&next) {
            return None;
        }
        order.push(next);
        previous = current;
        current = next;
    }
    if order.len() != points.len() {
        return None;
    }

    let ordered = ordered_profile(&order, &points, &links, tolerance).or_else(|| {
        let reversed = std::iter::once(order[0])
            .chain(order[1..].iter().rev().copied())
            .collect::<Vec<_>>();
        ordered_profile(&reversed, &points, &links, tolerance)
    })?;
    let (order, ordered_points, segments, segment_faces) = ordered;
    let positions = order
        .iter()
        .enumerate()
        .map(|(position, node)| (*node, position))
        .collect::<HashMap<_, _>>();
    let selected_nodes = selected
        .iter()
        .map(|edge| Some((*positions.get(&circles[edge])?, *edge)))
        .collect::<Option<HashMap<_, _>>>()?;
    let profile = super::chamfer_profile::chamfer_profile(
        &segments,
        &ordered_points,
        &selected_nodes,
        &segment_faces,
        base_face,
        base_distance,
        other_distance,
        tolerance,
        true,
    );
    Some(profile.and_then(|profile| {
        let result = super::revolve(plane, &profile, origin.to_array(), axis.to_array(), TAU)
            .ok_or(ChamferError::InvalidResult)?;
        if !result.validate().is_empty() || result.worst_vertex_gap() > tolerance {
            return Err(ChamferError::InvalidResult);
        }
        Ok(result)
    }))
}

type OrderedProfile = (Vec<usize>, Vec<[f64; 2]>, Vec<Curve>, Vec<Option<FaceKey>>);

fn ordered_profile(
    order: &[usize],
    points: &[[f64; 2]],
    links: &[(usize, usize, Option<FaceKey>, Curve)],
    tolerance: f64,
) -> Option<OrderedProfile> {
    let mut ordered_points = Vec::with_capacity(order.len());
    let mut segments = Vec::with_capacity(order.len());
    let mut segment_faces = Vec::with_capacity(order.len());
    for index in 0..order.len() {
        let node = order[index];
        let next = order[(index + 1) % order.len()];
        let (_, _, face, curve) = links.iter().find(|(first, second, _, _)| {
            (*first == node && *second == next) || (*first == next && *second == node)
        })?;
        let curve = match curve {
            Curve::Line(_) => Curve::Line(Line {
                start: points[node],
                end: points[next],
            }),
            Curve::Arc(_) => {
                if Vec2::from(curve.point_at(0.0)).distance(Vec2::from(points[node])) > tolerance
                    || Vec2::from(curve.point_at(1.0)).distance(Vec2::from(points[next]))
                        > tolerance
                {
                    return None;
                }
                curve.clone()
            }
            _ => return None,
        };
        ordered_points.push(points[node]);
        segments.push(curve);
        segment_faces.push(*face);
    }
    Some((order.to_vec(), ordered_points, segments, segment_faces))
}
