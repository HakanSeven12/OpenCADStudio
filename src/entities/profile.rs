//! Planar profiles: the loops a region, a closed curve or an embedded
//! entity bounds, in its own plane, as extrusions, sweeps, revolves and mass
//! properties read them.

use document::entities::{EmbeddedEntity, LwPolyline, LwVertex, Spline};
use document::types::{Vector2, Vector3};
use document::EntityType;
use ocs_geom::geom2d::{Arc, Curve, EllipseArc};
use ocs_geom::space::{PlanarCurve, Plane, Vec3};
use crate::entities::curve::entity_curve;

/// The arbitrary-axis X direction of an entity normal.
pub(crate) fn arbitrary_x(normal: glam::DVec3) -> glam::DVec3 {
    let world = if normal.x.abs() < 1.0 / 64.0 && normal.y.abs() < 1.0 / 64.0 {
        glam::DVec3::Y
    } else {
        glam::DVec3::Z
    };
    world.cross(normal).normalize()
}

/// A drawn profile, as the kernel wants it: the plane it lies in and the
/// chain of pieces closing a loop in that plane.
pub struct Profile {
    pub plane: Plane,
    pub pieces: Vec<Curve>,
}

/// An entity-level planar curve suitable for either a solid profile or an
/// open surface profile.
pub fn extrusion_profile_of(entity: &EntityType) -> Option<(Profile, bool)> {
    if let Some(profile) = region_loop(entity) {
        return Some((profile, true));
    }
    let planar = entity_curve(entity).or_else(|| planar_polygon_entity(entity))?;
    let closed = planar.curve.is_closed();
    let pieces = profile_pieces(&planar.curve, closed)?;
    (!pieces.is_empty()).then_some((
        Profile {
            plane: planar.plane,
            pieces,
        },
        closed,
    ))
}

/// A region of one boundary loop as that loop, read from the region's exact
/// edges (its display wires hold one wire per edge), a lone closed edge split
/// as a closed curve is.
pub(crate) fn region_loop(entity: &EntityType) -> Option<Profile> {
    let EntityType::Region(region) = entity else {
        return None;
    };
    if !region.acis_data.has_data() {
        return None;
    }
    let (plane, loops, _) = profile_geometry(entity)?;
    let [boundary] = loops.as_slice() else {
        return None;
    };
    let single = boundary.len() == 1;
    let pieces = boundary
        .iter()
        .map(|piece| profile_pieces(piece, single))
        .collect::<Option<Vec<_>>>()?
        .concat();
    Some(Profile { plane, pieces })
}

/// Whether a region has more than one boundary loop.
pub(crate) fn region_with_holes(entity: &EntityType) -> bool {
    matches!(entity, EntityType::Region(_))
        && region_profiles(entity).is_some_and(|(_, loops)| loops.len() > 1)
}

pub(crate) fn profile_pieces(curve: &Curve, closed: bool) -> Option<Vec<Curve>> {
    let pieces = match curve {
        Curve::Polyline(_) => curve.segments(),
        Curve::Circle(circle) => quarters(circle.centre, circle.radius),
        Curve::Arc(arc) if closed => split_arc(*arc),
        Curve::Arc(arc) => vec![Curve::Arc(*arc)],
        Curve::Ellipse(arc) if closed => split_ellipse(*arc),
        Curve::Ellipse(arc) => vec![Curve::Ellipse(*arc)],
        Curve::Line(line) => vec![Curve::Line(*line)],
        Curve::Nurbs(nurbs) if closed => (0..4)
            .map(|part| {
                nurbs
                    .trimmed(part as f64 / 4.0, (part + 1) as f64 / 4.0)
                    .map(Curve::Nurbs)
            })
            .collect::<Option<Vec<_>>>()?,
        Curve::Nurbs(nurbs) => vec![Curve::Nurbs(nurbs.clone())],
        _ => return None,
    };
    Some(pieces)
}

pub(crate) fn planar_polygon_entity(entity: &EntityType) -> Option<PlanarCurve> {
    let (points, closed) = match entity {
        EntityType::Polyline3D(value) => (
            value
                .vertices
                .iter()
                .map(|vertex| [vertex.position.x, vertex.position.y, vertex.position.z])
                .collect::<Vec<_>>(),
            value.is_closed(),
        ),
        EntityType::Face3D(value) => (
            value
                .corners()
                .into_iter()
                .map(|point| [point.x, point.y, point.z])
                .collect::<Vec<_>>(),
            true,
        ),
        EntityType::Solid(value) => (
            value
                .boundary_corners()
                .into_iter()
                .map(|point| [point.x, point.y, point.z])
                .collect::<Vec<_>>(),
            true,
        ),
        EntityType::Region(value) => (
            value
                .wires
                .first()?
                .points
                .iter()
                .map(|point| [point.x, point.y, point.z])
                .collect::<Vec<_>>(),
            true,
        ),
        _ => return None,
    };
    let mut points = points;
    if closed && points.len() > 2 && points.first() == points.last() {
        points.pop();
    }
    if points.len() < if closed { 3 } else { 2 } {
        return None;
    }
    let origin = Vec3::from(points[0]);
    let first = Vec3::from(points[1]) - origin;
    let normal = points[2..]
        .iter()
        .find_map(|point| first.cross(Vec3::from(*point) - origin).normalize())
        .or_else(|| (!closed).then(|| first.cross(Vec3::Z).normalize().unwrap_or(Vec3::Y)))?;
    let plane = Plane::orthonormal(points[0], first.to_array(), normal.to_array())?;
    let scale = points
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0_f64, f64::max);
    if points
        .iter()
        .any(|point| !plane.contains(*point, scale * 1e-9))
    {
        return None;
    }
    let vertices = points
        .iter()
        .map(|point| {
            plane
                .project(*point)
                .map(|position| ocs_geom::geom2d::PolylineVertex {
                    position,
                    bulge: 0.0,
                })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(PlanarCurve::new(
        plane,
        Curve::Polyline(ocs_geom::geom2d::Polyline { vertices, closed }),
    ))
}

/// A region's boundary loops: from its modeler edges when it has them (its
/// display wires then hold one wire per edge), else from its wires.
pub(crate) fn region_profiles(entity: &EntityType) -> Option<(Plane, Vec<Vec<Curve>>)> {
    let EntityType::Region(region) = entity else {
        return None;
    };
    if region.acis_data.has_data() {
        let (plane, loops, _) = profile_geometry(entity)?;
        return (!loops.is_empty()).then_some((plane, loops));
    }
    let plane = planar_polygon_entity(entity)?.plane;
    let mut profiles = Vec::with_capacity(region.wires.len());
    for wire in &region.wires {
        let mut points = wire
            .points
            .iter()
            .map(|point| [point.x, point.y, point.z])
            .collect::<Vec<_>>();
        if points.len() > 2 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < 3 {
            return None;
        }
        let scale = points
            .iter()
            .flatten()
            .map(|value| value.abs())
            .fold(1.0_f64, f64::max);
        if points
            .iter()
            .any(|point| !plane.contains(*point, scale * 1e-9))
        {
            return None;
        }
        let vertices = points
            .iter()
            .map(|point| {
                plane
                    .project(*point)
                    .map(|position| ocs_geom::geom2d::PolylineVertex {
                        position,
                        bulge: 0.0,
                    })
            })
            .collect::<Option<Vec<_>>>()?;
        profiles.push(
            Curve::Polyline(ocs_geom::geom2d::Polyline {
                vertices,
                closed: true,
            })
            .segments(),
        );
    }
    (!profiles.is_empty()).then_some((plane, profiles))
}

/// A circle as four arcs.
pub(crate) fn quarters(centre: [f64; 2], radius: f64) -> Vec<Curve> {
    use std::f64::consts::FRAC_PI_2;
    (0..4)
        .map(|quarter| {
            let start = FRAC_PI_2 * quarter as f64;
            Curve::Arc(Arc {
                centre,
                radius,
                start_angle: start,
                end_angle: start + FRAC_PI_2,
            })
        })
        .collect()
}

pub(crate) fn split_arc(arc: Arc) -> Vec<Curve> {
    let start = arc.start_angle;
    let step = arc.sweep() / 4.0;
    (0..4)
        .map(|part| {
            Curve::Arc(Arc {
                start_angle: start + step * part as f64,
                end_angle: start + step * (part + 1) as f64,
                ..arc
            })
        })
        .collect()
}

pub(crate) fn split_ellipse(arc: EllipseArc) -> Vec<Curve> {
    let start = arc.start_parameter;
    let step = arc.sweep() / 4.0;
    (0..4)
        .map(|part| {
            Curve::Ellipse(EllipseArc {
                ellipse: arc.ellipse,
                start_parameter: start + step * part as f64,
                end_parameter: start + step * (part + 1) as f64,
            })
        })
        .collect()
}

pub fn embedded_path(entity: &EntityType) -> Option<EmbeddedEntity> {
    match entity {
        EntityType::Line(value) => Some(EmbeddedEntity::Line(value.clone())),
        EntityType::Arc(value) => Some(EmbeddedEntity::Arc(value.clone())),
        EntityType::Circle(value) => Some(EmbeddedEntity::Circle(value.clone())),
        EntityType::Ellipse(value) => Some(EmbeddedEntity::Ellipse(value.clone())),
        EntityType::LwPolyline(value) => Some(EmbeddedEntity::LwPolyline(value.clone())),
        EntityType::Polyline2D(value) => {
            let mut polyline = LwPolyline::new();
            polyline.elevation = value.elevation;
            polyline.normal = value.normal;
            polyline.is_closed = value.is_closed();
            polyline.vertices = value
                .vertices
                .iter()
                .map(|vertex| {
                    let mut converted =
                        LwVertex::new(Vector2::new(vertex.location.x, vertex.location.y));
                    converted.bulge = vertex.bulge;
                    converted
                })
                .collect();
            Some(EmbeddedEntity::LwPolyline(polyline))
        }
        EntityType::Spline(value) => Some(EmbeddedEntity::Spline(value.clone())),
        EntityType::Helix(value) => Some(EmbeddedEntity::Spline(value.spline.clone())),
        EntityType::Polyline3D(value) if value.vertices.len() >= 2 => {
            let mut points = value
                .vertices
                .iter()
                .map(|vertex| vertex.position)
                .collect::<Vec<_>>();
            if value.is_closed() && points.first() != points.last() {
                points.push(*points.first()?);
            }
            let mut spline = Spline::from_control_points(1, points);
            spline.flags.linear = true;
            spline.flags.planar = false;
            spline.flags.closed = value.is_closed();
            Some(EmbeddedEntity::Spline(spline))
        }
        _ => None,
    }
}

/// A region of one boundary loop as the curve the reference records for it
/// as a profile: a circle (one edge, or arcs of one circle making a whole
/// turn) or whole ellipse as itself, a loop of lines and arcs as a closed
/// polyline in its own coordinate system about the region's normal, read
/// from the region's modeler edges.
pub fn region_boundary_entity(entity: &EntityType) -> Option<(EmbeddedEntity, [f64; 16])> {
    let EntityType::Region(region) = entity else {
        return None;
    };
    if !region.acis_data.has_data() {
        return None;
    }
    let (plane, loops, _) = profile_geometry(entity)?;
    let [boundary] = loops.as_slice() else {
        return None;
    };
    let world = |v: [f64; 3]| Vector3::new(v[0], v[1], v[2]);
    let normal = world(plane.normal()?);
    let identity = glam::DMat4::IDENTITY.to_cols_array();
    match boundary.as_slice() {
        [Curve::Circle(circle)] => {
            let mut value = document::entities::Circle::new();
            value.center = world(plane.point_at(circle.centre));
            value.radius = circle.radius;
            value.normal = normal;
            return Some((EmbeddedEntity::Circle(value), identity));
        }
        [Curve::Arc(first), rest @ ..]
            if rest.iter().all(|piece| matches!(piece, Curve::Arc(arc)
                if (arc.centre[0] - first.centre[0]).hypot(arc.centre[1] - first.centre[1]) <= 1e-9 * (1.0 + first.radius)
                    && (arc.radius - first.radius).abs() <= 1e-9 * (1.0 + first.radius)))
                && (boundary.iter().map(|piece| match piece { Curve::Arc(arc) => arc.sweep(), _ => 0.0 }).sum::<f64>()
                    - std::f64::consts::TAU).abs() <= 1e-9 =>
        {
            let mut value = document::entities::Circle::new();
            value.center = world(plane.point_at(first.centre));
            value.radius = first.radius;
            value.normal = normal;
            return Some((EmbeddedEntity::Circle(value), identity));
        }
        [Curve::Ellipse(arc)] if (arc.sweep() - std::f64::consts::TAU).abs() <= 1e-9 => {
            let ellipse = arc.ellipse;
            let centre = Vec3::from(plane.point_at(ellipse.centre));
            let tip = Vec3::from(plane.point_at([
                ellipse.centre[0] + ellipse.major_axis[0] * ellipse.major_radius,
                ellipse.centre[1] + ellipse.major_axis[1] * ellipse.major_radius,
            ]));
            let mut value = document::entities::Ellipse::new();
            value.center = world(centre.to_array());
            value.major_axis = world((tip - centre).to_array());
            value.minor_axis_ratio = ellipse.minor_radius / ellipse.major_radius;
            value.start_parameter = arc.start_parameter;
            value.end_parameter = arc.start_parameter + std::f64::consts::TAU;
            value.normal = normal;
            return Some((EmbeddedEntity::Ellipse(value), identity));
        }
        _ => {}
    }
    // Walk the loop head to tail; an arc run against its direction turns
    // clockwise (negative bulge).
    let close = |a: [f64; 2], b: [f64; 2]| {
        (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-9 * (1.0 + a[0].abs().max(a[1].abs()))
    };
    let ends = |piece: &Curve| match piece {
        Curve::Line(line) => Some((line.start, line.end, 0.0)),
        Curve::Arc(arc) => {
            let at = |angle: f64| {
                [
                    arc.centre[0] + arc.radius * angle.cos(),
                    arc.centre[1] + arc.radius * angle.sin(),
                ]
            };
            Some((
                at(arc.start_angle),
                at(arc.end_angle),
                (arc.sweep() / 4.0).tan(),
            ))
        }
        _ => None,
    };
    let (first_start, first_end, _) = ends(boundary.first()?)?;
    let second = boundary.get(1).and_then(ends);
    let mut at = if second
        .is_some_and(|(start, end, _)| close(first_start, start) || close(first_start, end))
    {
        first_end
    } else {
        first_start
    };
    // The polyline in its object coordinates: the arbitrary axes of the normal.
    let axis_normal = glam::DVec3::new(normal.x, normal.y, normal.z);
    let axis_x = crate::entities::profile::arbitrary_x(axis_normal);
    let axis_y = axis_normal.cross(axis_x);
    let mut polyline = LwPolyline::new();
    polyline.normal = normal;
    polyline.elevation = glam::DVec3::from_array(plane.origin).dot(axis_normal);
    for piece in boundary {
        let (start, end, bulge) = ends(piece)?;
        let (from, to, bulge) = if close(start, at) {
            (start, end, bulge)
        } else if close(end, at) {
            (end, start, -bulge)
        } else {
            return None;
        };
        let point = glam::DVec3::from_array(plane.point_at(from));
        let mut vertex = LwVertex::new(Vector2::new(point.dot(axis_x), point.dot(axis_y)));
        vertex.bulge = bulge;
        polyline.vertices.push(vertex);
        at = to;
    }
    polyline.is_closed = true;
    Some((EmbeddedEntity::LwPolyline(polyline), identity))
}

pub(crate) fn embedded_planar_entity(entity: &EntityType) -> Option<(EmbeddedEntity, [f64; 16])> {
    if region_with_holes(entity) {
        return None;
    }
    if let Some(found) = region_boundary_entity(entity) {
        return Some(found);
    }
    if !matches!(entity, EntityType::Polyline3D(_)) {
        if let Some(embedded) = embedded_path(entity) {
            return Some((embedded, glam::DMat4::IDENTITY.to_cols_array()));
        }
    }
    let (profile, closed) = extrusion_profile_of(entity)?;
    let mut polyline = LwPolyline::new();
    for piece in &profile.pieces {
        let Curve::Line(line) = piece else {
            return None;
        };
        polyline
            .vertices
            .push(LwVertex::new(Vector2::new(line.start[0], line.start[1])));
    }
    if !closed {
        let Curve::Line(last) = profile.pieces.last()? else {
            return None;
        };
        polyline
            .vertices
            .push(LwVertex::new(Vector2::new(last.end[0], last.end[1])));
    }
    polyline.is_closed = closed;
    let normal = profile.plane.normal()?;
    let transform = glam::DMat4::from_cols(
        glam::DVec4::new(
            profile.plane.x_axis[0],
            profile.plane.x_axis[1],
            profile.plane.x_axis[2],
            0.0,
        ),
        glam::DVec4::new(
            profile.plane.y_axis[0],
            profile.plane.y_axis[1],
            profile.plane.y_axis[2],
            0.0,
        ),
        glam::DVec4::new(normal[0], normal[1], normal[2], 0.0),
        glam::DVec4::new(
            profile.plane.origin[0],
            profile.plane.origin[1],
            profile.plane.origin[2],
            1.0,
        ),
    )
    .to_cols_array();
    Some((EmbeddedEntity::LwPolyline(polyline), transform))
}

pub fn embedded_revolve_profile(entity: &EntityType) -> Option<(EmbeddedEntity, [f64; 16])> {
    embedded_planar_entity(entity)
}

/// Extract exact embedded REGION loops before falling back to polygonal entities.
/// In particular, preview and final extrusion must not approximate REGION arcs
/// from their display wires or discard holes.
pub fn profile_geometry(entity: &EntityType) -> Option<(Plane, Vec<Vec<Curve>>, bool)> {
    if matches!(entity, EntityType::Ray(_) | EntityType::XLine(_)) {
        return None;
    }
    let embedded = match entity {
        EntityType::Region(region) => Some((
            EmbeddedEntity::Region(region.clone()),
            glam::DMat4::IDENTITY.to_cols_array(),
        )),
        _ => embedded_revolve_profile(entity),
    };
    if let Some((entity, transform)) = embedded {
        if let Ok(geometry) = ocs_solid::sweep_profile_geometry(&entity, transform) {
            return Some(geometry);
        }
        // Exact modeler data is authoritative, not a hint to replace by chords.
        if matches!(entity, EmbeddedEntity::Region(ref region) if region.acis_data.has_data()) {
            return None;
        }
    }
    let (profile, closed) = extrusion_profile_of(entity)?;
    Some((profile.plane, vec![profile.pieces], closed))
}
