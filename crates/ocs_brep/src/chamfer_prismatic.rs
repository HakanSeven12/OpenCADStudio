//! Exact longitudinal chamfers of constant analytic prisms.

use super::{Body, ChamferError, Curve3, EdgeKey, FaceKey, Surface};
use ocs_geom::geom2d::{Curve, Vec2};
use ocs_geom::space::Vec3;
use std::collections::{HashMap, HashSet};

pub(super) fn chamfer_prismatic(
    body: &Body,
    selected: &[EdgeKey],
    base_face: FaceKey,
    base_distance: f64,
    other_distance: f64,
) -> Option<Result<Body, ChamferError>> {
    let first = body.edges.get(*selected.first()?)?;
    if !matches!(body.curves.get(first.curve)?, Curve3::Line(_)) {
        return None;
    }
    let axis = (Vec3::from(body.vertices.get(first.end)?.point)
        - Vec3::from(body.vertices.get(first.start)?.point))
    .normalize()?;
    let tolerance = super::operation_tolerance(&[body]);
    if body.roots.len() != 1
        || body.lumps.len() != 1
        || body.shells.len() != 1
        || body.faces.iter().any(|(_, face)| face.loops.len() != 1)
        || body.edges.iter().any(|(_, edge)| {
            edge.coedges.len() != 2
                || !matches!(
                    body.curves.get(edge.curve),
                    Some(Curve3::Line(_) | Curve3::Circle(_))
                )
        })
    {
        return None;
    }

    let mut caps = Vec::new();
    for (key, face) in body.faces.iter() {
        match body.surfaces.get(face.surface)? {
            Surface::Plane(plane) => {
                let dot = Vec3::from(plane.normal()?).dot(axis).abs();
                if dot > 1.0 - 1.0e-9 {
                    caps.push((key, *plane));
                } else if dot > 1.0e-9 {
                    return None;
                }
            }
            Surface::Cylinder(cylinder)
                if Vec3::from(cylinder.base.normal()?).dot(axis).abs() > 1.0 - 1.0e-9 => {}
            _ => return None,
        }
    }
    if caps.len() != 2 {
        return None;
    }
    caps.sort_by(|first, second| {
        Vec3::from(first.1.origin)
            .dot(axis)
            .total_cmp(&Vec3::from(second.1.origin).dot(axis))
    });
    let (bottom, plane) = caps[0];
    let height = (Vec3::from(caps[1].1.origin) - Vec3::from(plane.origin)).dot(axis);
    if height <= tolerance {
        return None;
    }
    let shift = axis * height;

    let mut vertices = Vec::new();
    let mut bottom_edges = Vec::new();
    for coedge in body.face_coedges(bottom) {
        let coedge = body.coedges.get(coedge)?;
        let edge = body.edges.get(coedge.edge)?;
        vertices.push(if coedge.forward { edge.start } else { edge.end });
        bottom_edges.push(coedge.edge);
    }
    let count = vertices.len();
    if count < 3
        || body.vertices.len() != count * 2
        || body.edges.len() != count * 3
        || body.faces.len() != count + 2
    {
        return None;
    }
    let positions = vertices
        .iter()
        .map(|key| body.vertices.get(*key).map(|vertex| Vec3::from(vertex.point)))
        .collect::<Option<Vec<_>>>()?;
    let points = positions
        .iter()
        .map(|point| plane.project(point.to_array()))
        .collect::<Option<Vec<_>>>()?;
    let cap_profile = super::planar_face_profile(body, bottom)?;
    let profile = cap_profile.loops.first()?;
    if profile.len() != count {
        return None;
    }
    for index in 0..count {
        let next = (index + 1) % count;
        if Vec2::from(profile[index].point_at(0.0)).distance(Vec2::from(points[index]))
            > tolerance
            || Vec2::from(profile[index].point_at(1.0)).distance(Vec2::from(points[next]))
                > tolerance
        {
            return None;
        }
        if !matches!(profile[index], Curve::Line(_) | Curve::Arc(_)) {
            return None;
        }
    }

    let mut top_vertices = Vec::new();
    for point in &positions {
        let matches = body
            .vertices
            .iter()
            .filter(|(_, vertex)| Vec3::from(vertex.point).distance(*point + shift) <= tolerance)
            .map(|(key, _)| key)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return None;
        }
        top_vertices.push(matches[0]);
    }
    let top_boundary = body
        .face_coedges(caps[1].0)
        .into_iter()
        .map(|key| body.coedges.get(key).map(|coedge| coedge.edge))
        .collect::<Option<HashSet<_>>>()?;
    if top_boundary.len() != count {
        return None;
    }
    let mut top_edges = Vec::new();
    for index in 0..count {
        let next = (index + 1) % count;
        let candidates = top_boundary
            .iter()
            .filter(|key| {
                body.edges.get(**key).is_some_and(|edge| {
                    (edge.start == top_vertices[index] && edge.end == top_vertices[next])
                        || (edge.end == top_vertices[index]
                            && edge.start == top_vertices[next])
                })
            })
            .copied()
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return None;
        }
        let top = candidates[0];
        if !translated_edge(body, bottom_edges[index], top, shift, tolerance)? {
            return None;
        }
        top_edges.push(top);
    }

    let mut side_faces = vec![None; count];
    for (key, face) in body.faces.iter() {
        if key == bottom || key == caps[1].0 {
            continue;
        }
        let keys = body
            .face_coedges(key)
            .into_iter()
            .filter_map(|coedge| {
                let value = body.coedges.get(coedge)?;
                let edge = body.edges.get(value.edge)?;
                Some(if value.forward { edge.start } else { edge.end })
            })
            .collect::<HashSet<_>>();
        if keys.len() != 4 {
            return None;
        }
        let index = (0..count).find(|index| {
            let next = (*index + 1) % count;
            [
                vertices[*index],
                vertices[next],
                top_vertices[*index],
                top_vertices[next],
            ]
            .iter()
            .all(|vertex| keys.contains(vertex))
        })?;
        if side_faces[index].replace(key).is_some() {
            return None;
        }
        let side_edges = body
            .face_coedges(key)
            .into_iter()
            .map(|key| body.coedges.get(key).map(|coedge| coedge.edge))
            .collect::<Option<Vec<_>>>()?;
        if side_edges.len() != 4
            || !side_edges.contains(&bottom_edges[index])
            || !side_edges.contains(&top_edges[index])
        {
            return None;
        }
        let surface = body.surfaces.get(face.surface)?;
        for edge_key in side_edges {
            let edge = body.edges.get(edge_key)?;
            let curve = body.curves.get(edge.curve)?;
            if edge_key != bottom_edges[index] && edge_key != top_edges[index] {
                if !matches!(curve, Curve3::Line(_)) {
                    return None;
                }
                let span = Vec3::from(body.vertices.get(edge.end)?.point)
                    - Vec3::from(body.vertices.get(edge.start)?.point);
                if span.distance(shift).min(span.distance(-shift)) > tolerance {
                    return None;
                }
            }
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let point = curve.point_at(
                    edge.start_parameter
                        + fraction * (edge.end_parameter - edge.start_parameter),
                );
                let distance = surface.distance_to(point);
                if !distance.is_finite() || distance.abs() > tolerance {
                    return None;
                }
            }
        }
    }
    if side_faces.iter().any(Option::is_none) {
        return None;
    }

    let mut selected_nodes = HashMap::new();
    for key in selected {
        let edge = body.edges.get(*key)?;
        let index = (0..count).find(|index| {
            (edge.start == vertices[*index] && edge.end == top_vertices[*index])
                || (edge.end == vertices[*index] && edge.start == top_vertices[*index])
        })?;
        selected_nodes.insert(index, *key);
    }
    let result = super::chamfer_profile::chamfer_profile(
        profile,
        &points,
        &selected_nodes,
        &side_faces,
        base_face,
        base_distance,
        other_distance,
        tolerance,
        false,
    );
    Some(result.and_then(|profile| {
        let result = super::extrude(plane, &profile, shift.to_array())
            .ok_or(ChamferError::InvalidResult)?;
        if !result.validate().is_empty() || result.worst_vertex_gap() > tolerance {
            return Err(ChamferError::InvalidResult);
        }
        Ok(result)
    }))
}

fn translated_edge(
    body: &Body,
    bottom: EdgeKey,
    top: EdgeKey,
    shift: Vec3,
    tolerance: f64,
) -> Option<bool> {
    let bottom = body.edges.get(bottom)?;
    let top = body.edges.get(top)?;
    let first = body.curves.get(bottom.curve)?;
    let second = body.curves.get(top.curve)?;
    match (first, second) {
        (Curve3::Line(_), Curve3::Line(_)) => {}
        (Curve3::Circle(first), Curve3::Circle(second)) => {
            if (first.radius - second.radius).abs() > tolerance
                || (bottom.end_parameter - bottom.start_parameter).abs()
                    >= std::f64::consts::TAU
                || (Vec3::from(first.plane.origin) + shift)
                    .distance(Vec3::from(second.plane.origin))
                    > tolerance
                || Vec3::from(first.plane.normal()?)
                    .dot(Vec3::from(second.plane.normal()?))
                    .abs()
                    < 1.0 - 1.0e-9
                || ((bottom.end_parameter - bottom.start_parameter).abs()
                    - (top.end_parameter - top.start_parameter).abs())
                .abs()
                    * first.radius
                    > tolerance
            {
                return Some(false);
            }
        }
        _ => return Some(false),
    }
    let from = Vec3::from(first.point_at(bottom.start_parameter)) + shift;
    let reversed = from.distance(Vec3::from(second.point_at(top.end_parameter)))
        < from.distance(Vec3::from(second.point_at(top.start_parameter)));
    Some([0.0, 0.25, 0.5, 0.75, 1.0].iter().all(|fraction| {
        let parameter = if reversed { 1.0 - fraction } else { *fraction };
        let first_point = Vec3::from(first.point_at(
            bottom.start_parameter
                + fraction * (bottom.end_parameter - bottom.start_parameter),
        )) + shift;
        let second_point = Vec3::from(second.point_at(
            top.start_parameter + parameter * (top.end_parameter - top.start_parameter),
        ));
        first_point.distance(second_point) <= tolerance
    }))
}
