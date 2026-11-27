// Exact profile sweeps stored as kernel B-reps and ACIS.

use document::entities::{EmbeddedEntity, LwPolyline, LwVertex};
use document::objects::{
    SolidHistoryLoft, SolidHistoryNodeBase, SolidHistoryOperation, SolidHistoryRevolve,
    SolidHistorySweep,
};
use document::types::{Vector2, Vector3};
use document::EntityType;
use ocs_solid::model::Brep;
use ocs_geom::geom2d::{Curve, Line};
use ocs_geom::space::{Plane, Vec3};

pub use super::sweep_command_model::{
    assoc_edge_curve, assoc_source_supported, is_sweep_path, is_sweep_profile,
    link_sweep_expressions, link_sweep_sources, region_edge_curves, resweep_from_sources,
    scaled_sweep_path_refusal, set_surface_associativity, spatial_profile, spatial_sweep_record,
    surface_associativity, surface_sweep_record, sweep_expressions, sweep_record,
    sweep_refusal_code, sweep_selection_options, sweep_sources, swept_surface_entity,
    swept_with_options, AssocCurve, SWEEP_SOURCES_APP,
};
use crate::entities::curve::entity_curve;
use glam::DVec3;
pub use crate::entities::profile::{
    embedded_path, embedded_revolve_profile, extrusion_profile_of, region_boundary_entity, Profile,
};
pub(crate) use crate::entities::profile::{
    embedded_planar_entity, planar_polygon_entity, profile_pieces, quarters, region_loop,
    region_profiles, region_with_holes,
};

/// The closed profile an entity describes, or `None` when it does not
/// describe one.
///
/// A single closed curve — a circle, an ellipse, a closed polyline — is what
/// a sweep needs. The kernel wants it as a chain of at least three pieces,
/// which a polyline already is and which a circle is not, so a round profile
/// is handed over as the arcs it is made of rather than as one curve with
/// nowhere for the sweep to start.
pub fn profile_of(entity: &EntityType) -> Option<Profile> {
    if let Some(profile) = region_loop(entity) {
        return (profile.pieces.len() >= 3).then_some(profile);
    }
    let planar = entity_curve(entity).or_else(|| planar_polygon_entity(entity))?;
    if !planar.curve.is_closed() {
        return None;
    }
    let pieces = profile_pieces(&planar.curve, true)?;
    (pieces.len() >= 3).then_some(Profile {
        plane: planar.plane,
        pieces,
    })
}

/// The axis a height extrudes `entity` along: its profile plane's normal,
/// except that a line holds no plane of its own, so one lying in the UCS XY
/// plane goes along the UCS Z axis, the plane it was drawn in.
pub fn extrusion_axis(entity: &EntityType, ucs_z: [f64; 3]) -> Option<[f64; 3]> {
    if let EntityType::Line(line) = entity {
        let along = [
            line.end.x - line.start.x,
            line.end.y - line.start.y,
            line.end.z - line.start.z,
        ];
        let length = along.iter().map(|v| v * v).sum::<f64>().sqrt();
        let dot = along.iter().zip(ucs_z).map(|(a, z)| a * z).sum::<f64>();
        if length > 0.0 && (dot / length).abs() < 1e-6 {
            return Some(ucs_z);
        }
    }
    entity_curve(entity)
        .and_then(|curve| curve.plane.normal())
        .or_else(|| extrusion_profile_of(entity)?.0.plane.normal())
}

/// EXTRUDE: drag the profile `height` along its own plane's normal.
///
/// `None` for a profile that does not close, encloses nothing, or holds a
/// piece with no analytic side wall.
pub fn extruded(entity: &EntityType, height: f64) -> Option<Brep> {
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
) -> Option<Brep> {
    if let Some((plane, profiles)) =
        region_profiles(entity).filter(|(_, profiles)| profiles.len() > 1)
    {
        if taper_angle.abs() > 1e-12 {
            return None;
        }
        return ocs_solid::extrude_region(&plane, &profiles, direction, 0.0, false).ok();
    }
    let profile = profile_of(entity)?;
    ocs_solid::extrude_region(&profile.plane, &[profile.pieces], direction, taper_angle, false).ok()
}

pub fn extruded_surface(
    entity: &EntityType,
    direction: [f64; 3],
    taper_angle: f64,
) -> Option<Brep> {
    if region_with_holes(entity) {
        return None;
    }
    let (profile, _) = extrusion_profile_of(entity)?;
    ocs_solid::extrude_region(&profile.plane, &[profile.pieces], direction, taper_angle, true).ok()
}

pub fn extruded_along_path(
    entity: &EntityType,
    path: &EntityType,
    taper_angle: f64,
) -> Option<Brep> {
    if region_with_holes(entity) {
        if taper_angle.abs() > 1e-12 {
            return None;
        }
        return extruded_direction(entity, straight_path_direction(path)?, 0.0);
    }
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
            return ocs_solid::extrude_region(
                &profile.plane,
                &[profile.pieces],
                (Vec3::from(points[1]) - Vec3::from(points[0])).to_array(),
                taper_angle,
                false,
            )
            .ok();
        }
        // The path moves to the profile, which keeps its own orientation.
        let shift = profile_center(&profile)? - Vec3::from(*points.first()?);
        let points = points.iter().map(|point| (Vec3::from(*point) + shift).to_array()).collect::<Vec<_>>();
        return ocs_solid::sweep::sweep_path(
            profile.plane,
            &[profile.pieces],
            ocs_solid::sweep::SweepPath::Polyline3d { points: &points, closed: false },
            ocs_solid::sweep::SweepOptions { align: false, base_point: Some(points[0]), ..Default::default() },
        )
        .ok();
    }
    if taper_angle.abs() <= 1e-12 {
        return swept(entity, path);
    }
    let curve = entity_curve(path)?;
    let Curve::Line(line) = curve.curve else {
        return None;
    };
    let direction =
        Vec3::from(curve.plane.point_at(line.end)) - Vec3::from(curve.plane.point_at(line.start));
    ocs_solid::extrude_region(&profile.plane, &[profile.pieces], direction.to_array(), taper_angle, false).ok()
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
        (Vec3::from(curve.plane.point_at(line.end)) - Vec3::from(curve.plane.point_at(line.start)))
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
    start_angle: f64,
) -> Option<Brep> {
    let axis = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let body = if let Some((plane, profiles)) =
        region_profiles(entity).filter(|(_, profiles)| profiles.len() > 1)
    {
        axis_in_plane(&plane, from, axis).then_some(())?;
        revolve_region(plane, &profiles, from, axis, angle, true)?
    } else {
        let profile = profile_of(entity)?;
        axis_in_plane(&profile.plane, from, axis).then_some(())?;
        ocs_solid::revolve(&profile.plane, &profile.pieces, from, axis, angle, true).ok()?
    };
    turn_revolved_body(body, from, axis, start_angle)
}

/// REVOLVE sheet mode for an open or closed planar profile.
pub fn revolved_surface(
    entity: &EntityType,
    from: [f64; 3],
    to: [f64; 3],
    angle: f64,
    start_angle: f64,
) -> Option<Brep> {
    let axis = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let body = if let Some((plane, profiles)) =
        region_profiles(entity).filter(|(_, profiles)| profiles.len() > 1)
    {
        revolve_region(plane, &profiles, from, axis, angle, false)?
    } else {
        let (profile, _) = extrusion_profile_of(entity)?;
        ocs_solid::revolve(&profile.plane, &profile.pieces, from, axis, angle, false).ok()?
    };
    turn_revolved_body(body, from, axis, start_angle)
}

/// Whether the axis through `origin` along `axis` lies in `plane`: one off it sweeps the
/// profile into surfaces with no closed form, which are refused rather than approximated.
fn axis_in_plane(plane: &Plane, origin: [f64; 3], axis: [f64; 3]) -> bool {
    let (Some(normal), Some(axis)) = (plane.normal(), Vec3::from(axis).normalize()) else {
        return false;
    };
    let size = Vec3::from(origin).length().max(1.0);
    axis.dot(Vec3::from(normal)).abs() <= 1e-9 && plane.distance_to(origin).is_some_and(|gap| gap.abs() <= 1e-9 * size)
}

/// A region's outer loop revolved less its revolved holes (`solid`), or each loop's
/// revolved sheet.
fn revolve_region(
    plane: Plane,
    loops: &[Vec<Curve>],
    origin: [f64; 3],
    axis: [f64; 3],
    angle: f64,
    solid: bool,
) -> Option<Brep> {
    let bodies = loops
        .iter()
        .map(|ring| ocs_solid::revolve(&plane, ring, origin, axis, angle, solid).ok())
        .collect::<Option<Vec<_>>>()?;
    if solid {
        ocs_solid::model::subtract_all(bodies.first()?, &bodies[1..]).ok()?
    } else {
        ocs_solid::model::sewn(&bodies.iter().collect::<Vec<_>>(), 1e-9).ok()
    }
}

fn turn_revolved_body(body: Brep, pivot: [f64; 3], axis: [f64; 3], angle: f64) -> Option<Brep> {
    if !angle.is_finite() {
        return None;
    }
    if angle.abs() <= 1e-12 {
        return Some(body);
    }
    let axis = Vec3::from(axis).normalize()?;
    let rotate = |vector: Vec3| {
        let (sin, cos) = angle.sin_cos();
        vector * cos + axis.cross(vector) * sin + axis * axis.dot(vector) * (1.0 - cos)
    };
    let pivot = Vec3::from(pivot);
    let placement = ocs_solid::model::Placement {
        x_axis: rotate(Vec3::X).to_array(),
        y_axis: rotate(Vec3::Y).to_array(),
        z_axis: rotate(Vec3::Z).to_array(),
        origin: (pivot - rotate(pivot)).to_array(),
    };
    ocs_solid::model::placed(&body, &placement).ok()
}

fn profile_center(profile: &Profile) -> Option<Vec3> {
    let count = profile.pieces.len();
    let sum = profile.pieces.iter().try_fold(Vec3::ZERO, |sum, piece| {
        let point = Vec3::from(profile.plane.point_at(piece.point_at(0.0)));
        point.is_finite().then_some(sum + point)
    })?;
    (count > 0).then_some(sum / count as f64)
}

/// Stores the source profile and canonical axis required to rebuild and edit
/// a revolved solid.
pub fn revolve_history(
    entity: &EntityType,
    from: [f64; 3],
    to: [f64; 3],
    angle: f64,
    start_angle: f64,
) -> Option<SolidHistoryOperation> {
    if !angle.is_finite()
        || angle.abs() <= 1e-12
        || !start_angle.is_finite()
        || region_with_holes(entity)
    {
        return None;
    }
    let profile = profile_of(entity)?;
    let from = Vec3::from(from);
    let direction = (Vec3::from(to) - from).normalize()?;
    let center = profile_center(&profile)?;
    let axis_point = from + direction * (center - from).dot(direction);
    let (sweep_entity, transform) = embedded_planar_entity(entity)?;
    let transform = glam::DMat4::from_cols_array(&transform);
    let inverse = transform.inverse();
    if !transform.is_finite() || !inverse.is_finite() || transform.determinant().abs() <= 1e-12 {
        return None;
    }
    let axis_point = inverse.transform_point3(glam::DVec3::from_array(axis_point.to_array()));
    let direction = inverse
        .transform_vector3(glam::DVec3::from_array(direction.to_array()))
        .try_normalize()?;
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = transform.to_cols_array();
    Some(SolidHistoryOperation::Revolve(SolidHistoryRevolve {
        base,
        operation_major: 1,
        axis_point: Vector3::new(axis_point.x, axis_point.y, axis_point.z),
        direction: Vector3::new(direction.x, direction.y, direction.z),
        revolve_angle: angle,
        start_angle,
        flag_290: true,
        sweep_entity: Some(sweep_entity),
        ..SolidHistoryRevolve::default()
    }))
}

/// SWEEP as an exact B-rep along straight and circular path pieces.
pub fn swept(profile: &EntityType, path: &EntityType) -> Option<Brep> {
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
                .map(|pair| {
                    Curve::Line(Line {
                        start: pair[0],
                        end: pair[1],
                    })
                })
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
    let path = ocs_solid::sweep::SweepPath::Planar { plane: path.plane, curves: &pieces };
    let start = ocs_solid::sweep::sweep_path_start(path)?;
    ocs_solid::sweep::sweep_path(
        profile.plane,
        &[profile.pieces],
        path,
        ocs_solid::sweep::SweepOptions { align: false, base_point: Some(start), ..Default::default() },
    )
    .ok()
}

/// LOFT as an exact B-rep through compatible closed sections.
pub fn lofted(profiles: &[EntityType]) -> Option<Brep> {
    if let Some(body) = circular_loft(profiles) {
        return Some(body);
    }
    let sections = profiles
        .iter()
        .map(|entity| {
            let profile = profile_of(entity)?;
            Some(ocs_solid::LoftSection::Profile {
                plane: profile.plane,
                wires: vec![profile.pieces],
                closed: true,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    ocs_solid::loft_sections(&sections, &[], false, false, false).ok()
}

/// Stores the source sections required to rebuild and edit a loft.
pub fn loft_history(profiles: &[EntityType]) -> Option<SolidHistoryOperation> {
    let cross_sections = profiles
        .iter()
        .map(embedded_path)
        .collect::<Option<Vec<_>>>()?;
    if cross_sections.len() < 2 {
        return None;
    }
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = glam::DMat4::IDENTITY.to_cols_array();
    Some(SolidHistoryOperation::Loft(SolidHistoryLoft {
        base,
        operation_major: 1,
        cross_sections,
        guides: Vec::new(),
        ..SolidHistoryLoft::default()
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolysolidJustification {
    Left,
    Center,
    Right,
}

/// Extrusion records set group 295 (see `extrusion_history`): their embedded
/// entities stand where they are and the matrices only name their frames. A
/// planar fallback profile stored in its plane's local frame is therefore
/// re-expressed in its own OCS.
fn embedded_in_place(entity: EmbeddedEntity, transform: &[f64; 16]) -> EmbeddedEntity {
    let frame = glam::DMat4::from_cols_array(transform);
    let EmbeddedEntity::LwPolyline(mut polyline) = entity else {
        return entity;
    };
    if frame == glam::DMat4::IDENTITY {
        return EmbeddedEntity::LwPolyline(polyline);
    }
    let normal = frame.z_axis.truncate().normalize();
    let (x, y) = crate::scene::view::transform::ocs_axes((normal.x, normal.y, normal.z));
    let x_axis = glam::DVec3::new(x.0, x.1, x.2);
    let y_axis = glam::DVec3::new(y.0, y.1, y.2);
    for vertex in &mut polyline.vertices {
        let local = glam::DVec3::new(vertex.location.x, vertex.location.y, 0.0);
        let world = frame.transform_point3(local);
        vertex.location = Vector2::new(world.dot(x_axis), world.dot(y_axis));
        polyline.elevation = world.dot(normal);
    }
    polyline.normal = Vector3::new(normal.x, normal.y, normal.z);
    EmbeddedEntity::LwPolyline(polyline)
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
    let sweep_entity = embedded_in_place(sweep_entity, &sweep_entity_transform);
    let path_entity = path_entity.map(|entity| embedded_in_place(entity, &path_entity_transform));
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
        reference_point: Vector3::new(reference_point[0], reference_point[1], reference_point[2]),
        // Group 295: the profile frame is given, as the reference records
        // it. Left to be computed, the reference cannot open a drawing with
        // a tapered extrusion (ErrorStatus 53).
        flags_294_296: [false, true, false],
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
        reference_point: Vector3::new(reference_point[0], reference_point[1], reference_point[2]),
        ..SolidHistorySweep::default()
    }))
}

/// Builds a history-backed wall solid along a supported planar curve.
pub fn polysolid(
    entity: &EntityType,
    width: f64,
    height: f64,
    justification: PolysolidJustification,
) -> Option<(Brep, SolidHistoryOperation)> {
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
    let profile_plane = crate::entities::transform::WorkingPlane::new(
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
    let body = ocs_solid::rebuild_body(&operation).ok()?;
    Some((body, operation))
}

fn circular_loft(profiles: &[EntityType]) -> Option<Brep> {
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
        if !radius.is_finite() || *radius <= tolerance || normal.dot(direction).abs() < 1.0 - 1e-9 {
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
    let profile_plane =
        Plane::from_axes(first.0.to_array(), radial.to_array(), direction.to_array());
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
    ocs_solid::revolve(
        &profile_plane,
        &profile,
        first.0.to_array(),
        direction.to_array(),
        std::f64::consts::TAU,
        true,
    )
    .ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtrudeMode {
    Solid,
    Surface,
}

/// Construction options shared by SWEEP creation and its live preview.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepOptions {
    pub align: bool,
    pub bank: bool,
    pub base_point: Option<glam::DVec3>,
    pub scale: f64,
    /// Total twist along the path, in radians.
    pub twist_angle: f64,
    /// Where the path was picked: an open path is swept from its end nearest
    /// this point (seen from above), as the reference does.
    pub path_pick: Option<glam::DVec3>,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            align: true,
            bank: false,
            base_point: None,
            scale: 1.0,
            twist_angle: 0.0,
            path_pick: None,
        }
    }
}

pub fn empty_extruded_surface(direction: DVec3, taper_angle: f64) -> EntityType {
    use document::entities::{Surface, SurfaceData, SurfaceKind};
    use document::types::Vector3;

    let mut surface = Surface::new(SurfaceKind::Extruded);
    surface.u_isolines = 6;
    surface.v_isolines = 6;
    if let SurfaceData::Extruded {
        options,
        sweep_vector,
        ..
    } = &mut surface.surface_data
    {
        options.draft_angle = taper_angle;
        options.is_solid = false;
        *sweep_vector = Vector3::new(direction.x, direction.y, direction.z);
    }
    EntityType::Surface(Box::new(surface))
}

pub fn empty_revolved_surface(
    profile: &EntityType,
    axis_start: DVec3,
    axis_end: DVec3,
    angle: f64,
    start_angle: f64,
) -> EntityType {
    use document::entities::{Surface, SurfaceData, SurfaceKind};
    use document::types::Vector3;

    let mut surface = Surface::new(SurfaceKind::Revolved);
    let axis = (axis_end - axis_start).normalize_or(DVec3::Z);
    surface.point_of_reference = Vector3::new(axis_start.x, axis_start.y, axis_start.z);
    if let SurfaceData::Revolved {
        revolve_entity,
        axis_point,
        axis_vector,
        revolve_angle,
        start_angle: stored_start,
        entity_transform,
        solid,
        ..
    } = &mut surface.surface_data
    {
        if let Some((embedded, transform)) =
            embedded_revolve_profile(profile)
        {
            *revolve_entity = Some(embedded);
            *entity_transform = transform;
        }
        *axis_point = Vector3::new(axis_start.x, axis_start.y, axis_start.z);
        *axis_vector = Vector3::new(axis.x, axis.y, axis.z);
        *revolve_angle = angle;
        *stored_start = start_angle;
        *solid = false;
    }
    EntityType::Surface(Box::new(surface))
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::{Circle, LwPolyline, LwVertex};
    use document::types::{Vector2, Vector3};

    fn square(size: f64) -> EntityType {
        let mut polyline = LwPolyline::new();
        for corner in [[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]] {
            polyline.add_vertex(LwVertex::new(Vector2::new(corner[0], corner[1])));
        }
        polyline.is_closed = true;
        EntityType::LwPolyline(polyline)
    }

    fn volume(body: &Brep) -> f64 {
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
            0.0,
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
            std::f64::consts::TAU,
            0.0,
        )
        .is_none());
    }
}
