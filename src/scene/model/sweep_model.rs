// Exact profile sweeps stored as kernel B-reps and ACIS.

use cadkernel::brep::{self, Body};
use cadkernel::geom2d::{Arc, Curve, EllipseArc, Line};
use cadkernel::space::{PlanarCurve, Plane, Vec3};
use acadrust::entities::{EmbeddedEntity, LwPolyline, LwVertex, Spline};
use acadrust::objects::{
    SolidHistoryNodeBase, SolidHistoryOperation, SolidHistorySweep,
};
use acadrust::types::{Vector2, Vector3};
use acadrust::EntityType;

use crate::entities::curve::entity_curve;

/// A drawn profile, as the kernel wants it: the plane it lies in and the
/// chain of pieces closing a loop in that plane.
pub struct Profile {
    pub plane: Plane,
    pub pieces: Vec<Curve>,
}

/// The closed profile an entity describes, or `None` when it does not
/// describe one.
///
/// A single closed curve — a circle, an ellipse, a closed polyline — is what
/// a sweep needs. The kernel wants it as a chain of at least three pieces,
/// which a polyline already is and which a circle is not, so a round profile
/// is handed over as the arcs it is made of rather than as one curve with
/// nowhere for the sweep to start.
pub fn profile_of(entity: &EntityType) -> Option<Profile> {
    let planar: PlanarCurve = entity_curve(entity)?;
    if !planar.curve.is_closed() {
        return None;
    }
    let pieces = profile_pieces(&planar.curve, true)?;
    (pieces.len() >= 3).then_some(Profile {
        plane: planar.plane,
        pieces,
    })
}

/// An entity-level planar curve suitable for either a solid profile or an
/// open surface profile.
pub fn extrusion_profile_of(entity: &EntityType) -> Option<(Profile, bool)> {
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

fn profile_pieces(curve: &Curve, closed: bool) -> Option<Vec<Curve>> {
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

fn planar_polygon_entity(entity: &EntityType) -> Option<PlanarCurve> {
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
        .or_else(|| {
            (!closed).then(|| first.cross(Vec3::Z).normalize().unwrap_or(Vec3::Y))
        })?;
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
            plane.project(*point).map(|position| cadkernel::geom2d::PolylineVertex {
                position,
                bulge: 0.0,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(PlanarCurve::new(
        plane,
        Curve::Polyline(cadkernel::geom2d::Polyline { vertices, closed }),
    ))
}

/// A circle as four arcs.
fn quarters(centre: [f64; 2], radius: f64) -> Vec<Curve> {
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

fn split_arc(arc: Arc) -> Vec<Curve> {
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

fn split_ellipse(arc: EllipseArc) -> Vec<Curve> {
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

/// EXTRUDE: drag the profile `height` along its own plane's normal.
///
/// `None` for a profile that does not close, encloses nothing, or holds a
/// piece with no analytic side wall.
pub fn extruded(entity: &EntityType, height: f64) -> Option<Body> {
    let profile = profile_of(entity)?;
    let normal = profile.plane.normal()?;
    extruded_direction(
        entity,
        [normal[0] * height, normal[1] * height, normal[2] * height],
        0.0,
    )
}

pub fn extruded_direction(
    entity: &EntityType,
    direction: [f64; 3],
    taper_angle: f64,
) -> Option<Body> {
    let profile = profile_of(entity)?;
    brep::extrude_tapered(profile.plane, &profile.pieces, direction, taper_angle)
}

pub fn extruded_surface(
    entity: &EntityType,
    direction: [f64; 3],
    taper_angle: f64,
) -> Option<Body> {
    let (profile, closed) = extrusion_profile_of(entity)?;
    let _ = closed;
    brep::extrude_surface_tapered(
        profile.plane,
        &profile.pieces,
        direction,
        taper_angle,
    )
}

pub fn extruded_along_path(
    entity: &EntityType,
    path: &EntityType,
    taper_angle: f64,
) -> Option<Body> {
    let profile = profile_of(entity)?;
    if let EntityType::Polyline3D(path) = path {
        let points = path
            .vertices
            .iter()
            .map(|vertex| [vertex.position.x, vertex.position.y, vertex.position.z])
            .collect::<Vec<_>>();
        if taper_angle.abs() > 1e-12 {
            if points.len() != 2 {
                return None;
            }
            return brep::extrude_tapered(
                profile.plane,
                &profile.pieces,
                (Vec3::from(points[1]) - Vec3::from(points[0])).to_array(),
                taper_angle,
            );
        }
        return brep::sweep_along_polyline3d(profile.plane, &profile.pieces, &points);
    }
    if taper_angle.abs() <= 1e-12 {
        return swept(entity, path);
    }
    let curve = entity_curve(path)?;
    let Curve::Line(line) = curve.curve else {
        return None;
    };
    let direction = Vec3::from(curve.plane.point_at(line.end))
        - Vec3::from(curve.plane.point_at(line.start));
    brep::extrude_tapered(
        profile.plane,
        &profile.pieces,
        direction.to_array(),
        taper_angle,
    )
}

pub fn straight_path_direction(path: &EntityType) -> Option<[f64; 3]> {
    if let EntityType::Polyline3D(path) = path {
        if path.vertices.len() != 2 {
            return None;
        }
        let start = &path.vertices[0].position;
        let end = &path.vertices[1].position;
        return Some([end.x - start.x, end.y - start.y, end.z - start.z]);
    }
    let curve = entity_curve(path)?;
    let Curve::Line(line) = curve.curve else {
        return None;
    };
    Some(
        (Vec3::from(curve.plane.point_at(line.end))
            - Vec3::from(curve.plane.point_at(line.start)))
        .to_array(),
    )
}

/// Signed distance of a drag along a profile's normal.
pub fn projected_drag(entity: &EntityType, from: glam::DVec3, to: glam::DVec3) -> Option<f64> {
    let normal = entity_curve(entity)?.plane.normal()?;
    let distance = (to - from).dot(glam::DVec3::from_array(normal));
    (distance.is_finite() && distance.abs() > 1e-6).then_some(distance)
}

/// REVOLVE: turn the profile about the axis from `from` to `to` by `angle`
/// radians.
///
/// The axis has to lie in the profile's plane — a profile and an axis that do
/// not share one sweep into surfaces with no analytic form, and the kernel
/// refuses rather than approximating them.
pub fn revolved(
    entity: &EntityType,
    from: [f64; 3],
    to: [f64; 3],
    angle: f64,
) -> Option<Body> {
    let profile = profile_of(entity)?;
    brep::revolve(
        profile.plane,
        &profile.pieces,
        from,
        [to[0] - from[0], to[1] - from[1], to[2] - from[2]],
        angle,
    )
}

/// SWEEP as an exact B-rep along straight and circular path pieces.
pub fn swept(profile: &EntityType, path: &EntityType) -> Option<Body> {
    let profile = profile_of(profile)?;
    let mut path = entity_curve(path)?;
    let pieces = match &path.curve {
        Curve::Line(line) => vec![Curve::Line(*line)],
        Curve::Arc(arc) => vec![Curve::Arc(*arc)],
        Curve::Circle(circle) => quarters(circle.centre, circle.radius),
        Curve::Polyline(_) => path.curve.segments(),
        Curve::Ellipse(_) | Curve::Nurbs(_) => {
            let scale = path
                .curve
                .point_at(0.0)
                .into_iter()
                .chain(path.curve.point_at(1.0))
                .map(f64::abs)
                .fold(1.0_f64, f64::max);
            path.curve
                .tessellate_within(scale * 1e-4)
                .windows(2)
                .filter(|pair| pair[0] != pair[1])
                .map(|pair| Curve::Line(Line {
                    start: pair[0],
                    end: pair[1],
                }))
                .collect()
        }
        _ => return None,
    };
    let profile_center = profile
        .pieces
        .iter()
        .map(|piece| Vec3::from(profile.plane.point_at(piece.point_at(0.0))))
        .fold(Vec3::ZERO, |sum, point| sum + point)
        / profile.pieces.len() as f64;
    let path_start = Vec3::from(path.plane.point_at(path.curve.point_at(0.0)));
    path.plane.origin = (Vec3::from(path.plane.origin) + profile_center - path_start).to_array();
    brep::sweep_along(profile.plane, &profile.pieces, path.plane, &pieces)
}

/// LOFT as an exact B-rep through compatible closed sections.
pub fn lofted(profiles: &[EntityType]) -> Option<Body> {
    if let Some(body) = circular_loft(profiles) {
        return Some(body);
    }
    let sections = profiles
        .iter()
        .map(|entity| {
            let profile = profile_of(entity)?;
            Some((profile.plane, profile.pieces))
        })
        .collect::<Option<Vec<_>>>()?;
    brep::loft(&sections)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolysolidJustification {
    Left,
    Center,
    Right,
}

pub fn embedded_path(entity: &EntityType) -> Option<EmbeddedEntity> {
    match entity {
        EntityType::Line(value) => Some(EmbeddedEntity::Line(value.clone())),
        EntityType::Arc(value) => Some(EmbeddedEntity::Arc(value.clone())),
        EntityType::Circle(value) => Some(EmbeddedEntity::Circle(value.clone())),
        EntityType::Ellipse(value) => Some(EmbeddedEntity::Ellipse(value.clone())),
        EntityType::LwPolyline(value) => Some(EmbeddedEntity::LwPolyline(value.clone())),
        EntityType::Spline(value) => Some(EmbeddedEntity::Spline(value.clone())),
        EntityType::Polyline3D(value) if !value.is_closed() && value.vertices.len() >= 2 => {
            let mut spline = Spline::from_control_points(
                1,
                value
                    .vertices
                    .iter()
                    .map(|vertex| vertex.position)
                    .collect(),
            );
            spline.flags.linear = true;
            spline.flags.planar = false;
            Some(EmbeddedEntity::Spline(spline))
        }
        _ => None,
    }
}

fn embedded_planar_entity(entity: &EntityType) -> Option<(EmbeddedEntity, [f64; 16])> {
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

pub fn extrusion_history(
    profile: &EntityType,
    path: Option<&EntityType>,
    direction: [f64; 3],
    taper_angle: f64,
    reference_point: [f64; 3],
) -> Option<SolidHistoryOperation> {
    let signed_height = Vec3::from(direction).length();
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = glam::DMat4::IDENTITY.to_cols_array();
    let (sweep_entity, sweep_entity_transform) = embedded_planar_entity(profile)?;
    let (path_entity, path_entity_transform) = match path {
        Some(path) => {
            let (entity, transform) = embedded_planar_entity(path)?;
            (Some(entity), transform)
        }
        None => (None, glam::DMat4::IDENTITY.to_cols_array()),
    };
    Some(SolidHistoryOperation::Extrusion(SolidHistorySweep {
        base,
        operation_major: 1,
        direction: Vector3::new(direction[0], direction[1], direction[2]),
        sweep_entity: Some(sweep_entity),
        path_entity,
        draft_angle: taper_angle,
        start_draft_distance: 0.0,
        end_draft_distance: signed_height,
        scale_factor: 1.0,
        sweep_entity_transform,
        path_entity_transform,
        reference_point: Vector3::new(
            reference_point[0],
            reference_point[1],
            reference_point[2],
        ),
        ..SolidHistorySweep::default()
    }))
}

pub fn sweep_history(
    profile: &EntityType,
    path: &EntityType,
    taper_angle: f64,
    reference_point: [f64; 3],
) -> Option<SolidHistoryOperation> {
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = glam::DMat4::IDENTITY.to_cols_array();
    let (sweep_entity, sweep_entity_transform) = embedded_planar_entity(profile)?;
    Some(SolidHistoryOperation::Sweep(SolidHistorySweep {
        base,
        operation_major: 1,
        sweep_entity: Some(sweep_entity),
        path_entity: Some(embedded_path(path)?),
        draft_angle: taper_angle,
        scale_factor: 1.0,
        sweep_entity_transform,
        path_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
        reference_point: Vector3::new(
            reference_point[0],
            reference_point[1],
            reference_point[2],
        ),
        ..SolidHistorySweep::default()
    }))
}

/// Builds a history-backed wall solid along a supported planar curve.
pub fn polysolid(
    entity: &EntityType,
    width: f64,
    height: f64,
    justification: PolysolidJustification,
) -> Option<(Body, SolidHistoryOperation)> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height.abs() <= 1e-12 {
        return None;
    }
    let planar = entity_curve(entity)?;
    let start_local = planar.curve.point_at(0.0);
    let tangent_local = Vec3::from([
        planar.curve.tangent_at(0.0)[0],
        planar.curve.tangent_at(0.0)[1],
        0.0,
    ])
    .normalize()?;
    let start = Vec3::from(planar.plane.point_at(start_local));
    let tangent = (Vec3::from(planar.plane.x_axis) * tangent_local.x
        + Vec3::from(planar.plane.y_axis) * tangent_local.y)
        .normalize()?;
    let normal = Vec3::from(planar.plane.normal()?).normalize()?;
    let side = normal.cross(tangent).normalize()?;
    let offset = match justification {
        PolysolidJustification::Left => 0.0,
        PolysolidJustification::Center => -width * 0.5,
        PolysolidJustification::Right => -width,
    };
    let profile_origin = start + side * offset;
    let profile_plane = crate::command::WorkingPlane::new(
        glam::DVec3::from_array(profile_origin.to_array()),
        glam::DVec3::from_array(side.to_array()),
        glam::DVec3::from_array(normal.to_array()),
    );
    let mut profile = LwPolyline::new();
    profile.vertices = [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]]
        .into_iter()
        .map(|point| LwVertex::new(Vector2::new(point[0], point[1])))
        .collect();
    profile.is_closed = true;
    let EntityType::LwPolyline(profile) =
        profile_plane.place_entity(EntityType::LwPolyline(profile))
    else {
        return None;
    };
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = glam::DMat4::IDENTITY.to_cols_array();
    let operation = SolidHistoryOperation::Sweep(SolidHistorySweep {
        base,
        operation_major: 1,
        direction: Vector3::new(normal.x * height, normal.y * height, normal.z * height),
        sweep_entity: Some(EmbeddedEntity::LwPolyline(profile)),
        path_entity: Some(embedded_path(entity)?),
        scale_factor: 1.0,
        sweep_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
        path_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
        reference_point: Vector3::new(start.x, start.y, start.z),
        ..SolidHistorySweep::default()
    });
    let body = cadkernel::acis::rebuild_body(&operation).ok()?;
    Some((body, operation))
}

fn circular_loft(profiles: &[EntityType]) -> Option<Body> {
    if profiles.len() < 2 {
        return None;
    }
    let sections = profiles
        .iter()
        .map(|entity| {
            let planar = entity_curve(entity)?;
            let Curve::Circle(circle) = planar.curve else {
                return None;
            };
            Some((
                Vec3::from(planar.plane.point_at(circle.centre)),
                circle.radius,
                Vec3::from(planar.plane.normal()?),
                Vec3::from(planar.plane.x_axis),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let first = sections.first()?;
    let direction = (sections.last()?.0 - first.0).normalize()?;
    let scale = sections
        .iter()
        .map(|(centre, radius, _, _)| centre.length().max(*radius))
        .fold(1.0_f64, f64::max);
    let tolerance = scale * 1e-9;
    let mut heights = Vec::with_capacity(sections.len());
    for (centre, radius, normal, _) in &sections {
        if !radius.is_finite()
            || *radius <= tolerance
            || normal.dot(direction).abs() < 1.0 - 1e-9
        {
            return None;
        }
        let offset = *centre - first.0;
        if (offset - direction * offset.dot(direction)).length() > tolerance {
            return None;
        }
        heights.push(offset.dot(direction));
    }
    if heights
        .windows(2)
        .any(|pair| pair[1] <= pair[0] + tolerance)
    {
        return None;
    }
    let radial_seed = first.3 - direction * first.3.dot(direction);
    let radial = radial_seed.normalize()?;
    let profile_plane = Plane::from_axes(first.0.to_array(), radial.to_array(), direction.to_array());
    let mut points = Vec::with_capacity(sections.len() + 2);
    points.push([0.0, 0.0]);
    points.extend(
        sections
            .iter()
            .zip(&heights)
            .map(|((_, radius, _, _), height)| [*radius, *height]),
    );
    points.push([0.0, *heights.last()?]);
    let profile = (0..points.len())
        .map(|index| {
            Curve::Line(Line {
                start: points[index],
                end: points[(index + 1) % points.len()],
            })
        })
        .collect::<Vec<_>>();
    brep::revolve(
        profile_plane,
        &profile,
        first.0.to_array(),
        direction.to_array(),
        std::f64::consts::TAU,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{Circle, LwPolyline, LwVertex};
    use acadrust::types::{Vector2, Vector3};

    fn square(size: f64) -> EntityType {
        let mut polyline = LwPolyline::new();
        for corner in [[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]] {
            polyline.add_vertex(LwVertex::new(Vector2::new(corner[0], corner[1])));
        }
        polyline.is_closed = true;
        EntityType::LwPolyline(polyline)
    }

    fn volume(body: &Body) -> f64 {
        crate::scene::model::solid_model::volume(body)
    }

    #[test]
    fn a_closed_polyline_extrudes_into_a_prism() {
        let solid = extruded(&square(10.0), 4.0).expect("a prism");
        assert!((volume(&solid) - 400.0).abs() < 1e-6, "{}", volume(&solid));
    }

    #[test]
    fn a_circle_extrudes_into_a_cylinder() {
        // Cut into quarter arcs on the way, so the wall is four cylinder
        // patches rather than a run of flat chords — the volume says which.
        let mut circle = Circle::new();
        circle.center = Vector3::new(0.0, 0.0, 0.0);
        circle.radius = 5.0;
        let solid = extruded(&EntityType::Circle(circle), 3.0).expect("a cylinder");
        let expected = std::f64::consts::PI * 25.0 * 3.0;
        let got = volume(&solid);
        assert!(got > 0.98 * expected, "{got} vs {expected}");
        assert!(got <= expected * 1.000_001, "{got} vs {expected}");
    }

    #[test]
    fn a_square_revolved_about_its_own_edge_is_a_tube() {
        // The axis runs up the square's left side, so what it sweeps is a
        // solid cylinder of radius ten and height ten.
        let solid = revolved(
            &square(10.0),
            [0.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            std::f64::consts::TAU,
        );
        // The profile is in the XY plane and the axis lies in it, so this is
        // a revolution about the y axis: radius ten, height ten.
        let solid = solid.expect("a cylinder");
        let expected = std::f64::consts::PI * 100.0 * 10.0;
        let got = volume(&solid);
        assert!(got > 0.98 * expected, "{got} vs {expected}");
    }

    #[test]
    fn an_open_profile_is_refused() {
        let mut polyline = LwPolyline::new();
        for corner in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]] {
            polyline.add_vertex(LwVertex::new(Vector2::new(corner[0], corner[1])));
        }
        polyline.is_closed = false;
        assert!(extruded(&EntityType::LwPolyline(polyline), 4.0).is_none());
    }

    #[test]
    fn an_axis_off_the_profiles_plane_is_refused() {
        assert!(revolved(
            &square(10.0),
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            std::f64::consts::TAU
        )
        .is_none());
    }
}
