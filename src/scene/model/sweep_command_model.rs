//! SWEEP command data mapped to the shared kernel history reconstruction.

use codec::entities::{Surface, SurfaceData, SurfaceKind, SurfaceSweepOptions};
use codec::objects::{SolidHistoryNodeBase, SolidHistorySweep};
use codec::types::Vector3;
use codec::EntityType;
use kernel::brep::Body;

use crate::command::{ExtrudeMode, SweepOptions};
use super::sweep_model::{embedded_path, embedded_revolve_profile};

/// A 3D polyline profile or path as the reference records it: a wire body.
fn embedded_sweep_profile(entity: &EntityType) -> Option<(codec::entities::EmbeddedEntity, [f64; 16])> {
    match entity {
        EntityType::Region(region) => Some((codec::entities::EmbeddedEntity::Region(region.clone()), glam::DMat4::IDENTITY.to_cols_array())),
        EntityType::Polyline3D(value) => Some((polyline_wire(value)?, glam::DMat4::IDENTITY.to_cols_array())),
        _ => embedded_revolve_profile(entity),
    }
}

/// The sweep path as the reference records it: a 3D polyline as a wire
/// body, anything else as its embedded entity.
fn embedded_sweep_path(path: &EntityType) -> Option<codec::entities::EmbeddedEntity> {
    match path {
        EntityType::Polyline3D(value) => polyline_wire(value),
        _ => embedded_path(path),
    }
}

fn polyline_wire(value: &codec::entities::Polyline3D) -> Option<codec::entities::EmbeddedEntity> {
    let points = value.vertices.iter()
        .map(|vertex| [vertex.position.x, vertex.position.y, vertex.position.z])
        .collect::<Vec<_>>();
    let mut document = codec::entities::acis::SatDocument::new();
    kernel::acis::append_polyline_wire(&points, value.is_closed(), &mut document).ok()?;
    Some(codec::entities::EmbeddedEntity::Body {
        // The 3D polyline object type.
        type_code: 16,
        acis_data: codec::entities::AcisData::from_sat(&document.to_sat_string()),
    })
}

pub fn is_sweep_profile(entity: &EntityType) -> bool {
    embedded_sweep_profile(entity).is_some_and(|(profile, transform)| {
        kernel::acis::sweep_profile_geometry(&profile, transform).is_ok()
    }) || spatial_profile(entity).is_some()
}

/// The vertices of a 3D polyline profile that does not lie in one plane.
pub fn spatial_profile(entity: &EntityType) -> Option<(Vec<[f64; 3]>, bool)> {
    let EntityType::Polyline3D(_) = entity else { return None };
    let (profile, transform) = embedded_sweep_profile(entity)?;
    kernel::acis::sweep_spatial_profile(&profile, transform)
}

/// The sweep record of a spatial 3D polyline profile, which the reference
/// sweeps as a surface: the profile stays in its XY plane and each point's
/// height above it is carried along the path. Refused as the reference
/// refuses it: along a path with a corner (85036), and when the profile seen
/// from above crosses itself (85021); a side that is a point when seen from
/// above cannot be swept at all (`None` code).
pub fn spatial_sweep_record(profile: &EntityType, path: &EntityType, options: SweepOptions) -> Result<SolidHistorySweep, Option<u32>> {
    let (points, closed) = spatial_profile(profile).ok_or(None)?;
    let count = points.len();
    let sides = if closed { count } else { count - 1 };
    let plan = |index: usize| glam::DVec2::new(points[index % count][0], points[index % count][1]);
    if (0..sides).any(|index| plan(index).distance(plan(index + 1)) <= 1e-9) {
        return Err(None);
    }
    let cross = |a: glam::DVec2, b: glam::DVec2| a.x * b.y - a.y * b.x;
    for i in 0..sides {
        for j in i + 1..sides {
            // Neighbouring sides share a corner.
            if j == i + 1 || (closed && i == 0 && j == sides - 1) { continue; }
            let (p, r) = (plan(i), plan(i + 1) - plan(i));
            let (q, s) = (plan(j), plan(j + 1) - plan(j));
            let denominator = cross(r, s);
            if denominator.abs() <= 1e-12 { continue; }
            let t = cross(q - p, s) / denominator;
            let u = cross(q - p, r) / denominator;
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                return Err(Some(85021));
            }
        }
    }
    let (sweep_entity, _) = embedded_sweep_profile(profile).ok_or(None)?;
    let base_point = options.base_point.map(|point| point.to_array());
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = glam::DMat4::IDENTITY.to_cols_array();
    let record = SolidHistorySweep {
        base,
        operation_major: 1,
        sweep_entity: Some(sweep_entity),
        path_entity: Some(embedded_sweep_path(path).ok_or(None)?),
        scale_factor: options.scale,
        twist_angle: options.twist_angle,
        align_option: if options.align { 1 } else { 2 },
        has_align_start: true,
        flags_294_296: [base_point.is_some(), false, false],
        bank: options.bank,
        sweep_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
        path_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
        reference_point: base_point.map_or(Vector3::new(0.0, 0.0, 0.0), |p| Vector3::new(p[0], p[1], p[2])),
        miter_option: 2,
        ..SolidHistorySweep::default()
    };
    if kernel::acis::sweep_history_path_has_corner(&record).ok_or(None)? {
        return Err(Some(85036));
    }
    Ok(record)
}

/// The modeling error code the reference reports for a refused sweep.
pub fn sweep_refusal_code(refusal: kernel::brep::SweepRefusal) -> u32 {
    match refusal {
        kernel::brep::SweepRefusal::Scale => 5016,
        kernel::brep::SweepRefusal::Twist => 115065,
        kernel::brep::SweepRefusal::Bank => 115007,
    }
}

/// Why the reference refuses a path for a scaled sweep: scaling needs an
/// open path, and a spatial path must be smooth.
pub fn scaled_sweep_path_refusal(path: &EntityType) -> Option<&'static str> {
    let (closed, smooth) = match crate::entities::curve::entity_curve(path) {
        Some(planar) => (planar.curve.is_closed(), true),
        None => match path {
            EntityType::Polyline3D(value) => {
                let points = value.vertices.iter()
                    .map(|vertex| glam::DVec3::new(vertex.position.x, vertex.position.y, vertex.position.z))
                    .collect::<Vec<_>>();
                let directions = points.windows(2)
                    .filter_map(|pair| (pair[1] - pair[0]).try_normalize())
                    .collect::<Vec<_>>();
                let straight = directions.windows(2).all(|pair| pair[0].dot(pair[1]) >= 1.0 - 1e-9);
                (value.is_closed(), straight && !value.is_closed())
            }
            EntityType::Spline(value) => (value.flags.closed, value.degree > 1),
            _ => (false, true),
        },
    };
    if closed {
        Some("Cannot use scale option when path curve is closed.")
    } else if !smooth {
        Some("Path curve must be smooth when using scale option.")
    } else {
        None
    }
}

pub fn is_sweep_path(entity: &EntityType) -> bool {
    match embedded_path(entity) {
        Some(codec::entities::EmbeddedEntity::Spline(value)) => {
            value.degree > 0 && (value.control_points.len() > value.degree as usize
                || value.fit_points.len() >= 2)
        }
        Some(_) => crate::entities::curve::entity_curve(entity)
            .is_some_and(|curve| curve.curve.length().is_finite() && curve.curve.length() > 1e-9),
        None => false,
    }
}

/// All selected profiles use one base point, preserving their relative offsets.
pub fn sweep_selection_options(profiles: &[EntityType], mut options: SweepOptions) -> Option<SweepOptions> {
    if options.base_point.is_some() {
        return Some(options);
    }
    // Spatial profiles keep their own anchor.
    let profiles = profiles.iter().filter(|profile| spatial_profile(profile).is_none()).collect::<Vec<_>>();
    // A single profile takes its base from the path it is swept along
    // (`sweep_record`).
    // ponytail: several profiles share the group anchor even when the path
    // starts inside one of them; the reference rule for that case is unmeasured.
    if profiles.len() < 2 {
        return Some(options);
    }
    let geometry = profiles.iter().map(|profile| {
        let (entity, transform) = embedded_sweep_profile(profile)?;
        let (plane, wires, _) = kernel::acis::sweep_profile_geometry(&entity, transform).ok()?;
        Some((plane, wires))
    }).collect::<Option<Vec<_>>>()?;
    options.base_point = Some(glam::DVec3::from_array(
        kernel::brep::sweep_profile_group_base(&geometry)?,
    ));
    Some(options)
}

/// The path traversed the other way when it was picked nearer its end
/// (measured in plan, as the reference measures the pick), else `None`.
fn reversed_toward_pick(path: &EntityType, pick: glam::DVec3) -> Option<EntityType> {
    let (start, end) = path_ends(path)?;
    let flat = |p: glam::DVec3| p.truncate().distance(pick.truncate());
    if flat(end) >= flat(start) { return None; }
    match path {
        // An arc has one direction; reversed it is a one-segment polyline.
        EntityType::Arc(arc) => {
            let mut polyline = codec::LwPolyline::new();
            let sweep = (arc.end_angle - arc.start_angle).rem_euclid(std::f64::consts::TAU);
            let at = |angle: f64| codec::types::Vector2::new(arc.center.x + arc.radius * angle.cos(), arc.center.y + arc.radius * angle.sin());
            let mut first = codec::entities::LwVertex::new(at(arc.end_angle));
            first.bulge = -(sweep / 4.0).tan();
            polyline.vertices = vec![first, codec::entities::LwVertex::new(at(arc.start_angle))];
            polyline.normal = arc.normal;
            polyline.elevation = arc.center.z;
            Some(EntityType::LwPolyline(polyline))
        }
        _ => crate::modules::draw::modify::reverse::ReverseCommand::reversed(path),
    }
}

/// The start and end of an open path.
fn path_ends(path: &EntityType) -> Option<(glam::DVec3, glam::DVec3)> {
    Some(match path {
        EntityType::Polyline3D(value) if !value.is_closed() => {
            let p = |v: &codec::entities::Vertex3DPolyline| glam::DVec3::new(v.position.x, v.position.y, v.position.z);
            (p(value.vertices.first()?), p(value.vertices.last()?))
        }
        EntityType::Spline(value) if !value.flags.closed => {
            let points = if value.control_points.is_empty() { &value.fit_points } else { &value.control_points };
            let p = |v: &codec::types::Vector3| glam::DVec3::new(v.x, v.y, v.z);
            (p(points.first()?), p(points.last()?))
        }
        _ => {
            let planar = crate::entities::curve::entity_curve(path)?;
            if planar.curve.is_closed() { return None; }
            let at = |t: f64| glam::DVec3::from_array(planar.plane.point_at(planar.curve.point_at(t)));
            (at(0.0), at(1.0))
        }
    })
}

pub fn sweep_record(profile: &EntityType, path: &EntityType, options: SweepOptions) -> Option<SolidHistorySweep> {
    // Placed from the picked end; the record keeps the original path, and
    // the placed frame tells which end the sweep starts from.
    if let Some(reversed) = options.path_pick.and_then(|pick| reversed_toward_pick(path, pick)) {
        let mut record = sweep_record(profile, &reversed, SweepOptions { path_pick: None, ..options })?;
        record.path_entity = Some(embedded_sweep_path(path)?);
        return Some(record);
    }
    let (sweep_entity, sweep_entity_transform) = embedded_sweep_profile(profile)?;
    kernel::acis::sweep_profile_geometry(&sweep_entity, sweep_entity_transform).ok()?;
    let base_point = match options.base_point {
        Some(point) => point.to_array(),
        None => kernel::acis::sweep_default_base(&sweep_entity, sweep_entity_transform, &embedded_sweep_path(path)?).ok()?,
    };
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = glam::DMat4::IDENTITY.to_cols_array();
    let record = SolidHistorySweep {
        base,
        operation_major: 1,
        sweep_entity: Some(sweep_entity),
        path_entity: Some(embedded_sweep_path(path)?),
        scale_factor: options.scale,
        twist_angle: options.twist_angle,
        // 1 aligns the profile to the path; 2 only moves it to the path start.
        align_option: if options.align { 1 } else { 2 },
        has_align_start: true,
        bank: options.bank,
        sweep_entity_transform,
        path_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
        reference_point: Vector3::new(base_point[0], base_point[1], base_point[2]),
        ..SolidHistorySweep::default()
    };
    placed_sweep_record(profile, record)
}

/// The record the reference reads: the profile stored already placed at the
/// path start (base point, alignment and profile rotation applied) with flag
/// 295 set, and no reference point.
fn placed_sweep_record(profile: &EntityType, mut record: SolidHistorySweep) -> Option<SolidHistorySweep> {
    let (placed, _) = kernel::acis::sweep_history_placements(&record).ok()?;
    let embedded_to_world = glam::DMat4::from_cols(
        glam::DVec4::new(placed.x_axis[0], placed.x_axis[1], placed.x_axis[2], 0.0),
        glam::DVec4::new(placed.y_axis[0], placed.y_axis[1], placed.y_axis[2], 0.0),
        glam::DVec4::new(placed.z_axis[0], placed.z_axis[1], placed.z_axis[2], 0.0),
        glam::DVec4::new(placed.origin[0], placed.origin[1], placed.origin[2], 1.0),
    );
    // Source world geometry -> placed world geometry.
    let map = embedded_to_world * glam::DMat4::from_cols_array(&record.sweep_entity_transform).inverse();
    let m = map.to_cols_array_2d();
    let transform = codec::types::Transform::from_matrix(codec::types::Matrix4 {
        m: [
            [m[0][0], m[1][0], m[2][0], m[3][0]],
            [m[0][1], m[1][1], m[2][1], m[3][1]],
            [m[0][2], m[1][2], m[2][2], m[3][2]],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    // The record's matrices are the source profile frame (at the base point,
    // in the profile plane) and the frame it is placed in at the path start;
    // the reference re-places the profile from them when it re-evaluates.
    let (plane, _, _) = kernel::acis::sweep_profile_geometry(
        record.sweep_entity.as_ref()?,
        record.sweep_entity_transform,
    ).ok()?;
    let x_axis = glam::DVec3::from_array(plane.x_axis).try_normalize()?;
    let normal = glam::DVec3::from_array(plane.normal()?);
    let base = glam::DVec3::new(record.reference_point.x, record.reference_point.y, record.reference_point.z);
    let source_frame = glam::DMat4::from_cols(
        x_axis.extend(0.0),
        normal.cross(x_axis).extend(0.0),
        normal.extend(0.0),
        base.extend(1.0),
    );
    let mut moved = profile.clone();
    crate::scene::view::dispatch::apply_transform(&mut moved, &crate::command::EntityTransform::Affine(transform));
    let (sweep_entity, _) = embedded_sweep_profile(&moved)?;
    record.sweep_entity = Some(sweep_entity);
    record.sweep_entity_transform = source_frame.to_cols_array();
    record.path_entity_transform = (map * source_frame).to_cols_array();
    // Group 294 marks a profile only moved to the path, not turned onto it.
    record.flags_294_296 = [record.align_option != 1, true, true];
    // The reference records its default mitred joint.
    record.miter_option = 2;
    record.reference_point = Vector3::new(0.0, 0.0, 0.0);
    Some(record)
}

pub fn swept_with_options(profile: &EntityType, path: &EntityType, mode: ExtrudeMode, options: SweepOptions) -> Option<Body> {
    if spatial_profile(profile).is_some() {
        return kernel::acis::rebuild_sweep_with_mode(&spatial_sweep_record(profile, path, options).ok()?, true).ok();
    }
    let record = sweep_record(profile, path, options)?;
    kernel::acis::rebuild_sweep_with_mode(&record, mode == ExtrudeMode::Surface).ok()
}

/// The application name of the expressions a swept surface stays linked to.
pub const SWEEP_EXPRESSION_APP: &str = "OCS_SWEEP_EXPRESSION";

/// Links a swept surface to its scale and twist expressions (kept as the
/// surface's own data, under a registered application, so they are saved
/// with the drawing).
pub fn link_sweep_expressions(
    document: &mut codec::CadDocument,
    handle: codec::Handle,
    expressions: &[Option<String>; 2],
) {
    if handle.is_null() || expressions.iter().all(Option::is_none) { return; }
    let mut values = Vec::new();
    for (name, expression) in ["ScaleFactor", "TwistAngle"].iter().zip(expressions) {
        if let Some(expression) = expression {
            values.push(codec::xdata::XDataValue::String(name.to_string()));
            values.push(codec::xdata::XDataValue::String(expression.clone()));
        }
    }
    crate::scene::view::dispatch::set_entity_xdata(document, handle, SWEEP_EXPRESSION_APP, Some(values));
}

/// SURFACEASSOCIATIVITY: whether new surfaces stay associative to the
/// objects they were made from. A profile setting, so one value for every
/// drawing.
static SURFACE_ASSOCIATIVITY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn surface_associativity() -> bool {
    SURFACE_ASSOCIATIVITY.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_surface_associativity(on: bool) {
    SURFACE_ASSOCIATIVITY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The application name under which an associative swept surface keeps
/// the profile and path it was swept from.
pub const SWEEP_SOURCES_APP: &str = "OCS_SWEEP_SOURCES";

/// Marks a swept surface associative to the profile and path it was swept
/// from, with the options it is swept again with when they change; the
/// drawing's associative network for it is written on save.
pub fn link_sweep_sources(
    document: &mut codec::CadDocument,
    surface: codec::Handle,
    profile: codec::Handle,
    path: codec::Handle,
    options: &SweepOptions,
) {
    use codec::xdata::XDataValue;
    if surface.is_null() { return; }
    let point = |p: glam::DVec3| XDataValue::Point3D(Vector3::new(p.x, p.y, p.z));
    // The path keeps the direction it was swept in when it is edited.
    let reversed = options.path_pick
        .zip(document.get_entity(path))
        .is_some_and(|(pick, path)| reversed_toward_pick(path, pick).is_some());
    let mut values = vec![
        XDataValue::Handle(profile),
        XDataValue::Handle(path),
        XDataValue::Integer16(i16::from(options.align) | (i16::from(options.bank) << 1) | (i16::from(reversed) << 2)),
    ];
    if let Some(base) = options.base_point {
        values.push(XDataValue::String("BASE".to_string()));
        values.push(point(base));
    }
    crate::scene::view::dispatch::set_entity_xdata(document, surface, SWEEP_SOURCES_APP, Some(values));
}

/// The profile and path an associative swept surface was swept from.
pub fn sweep_sources(entity: &EntityType) -> Option<(codec::Handle, codec::Handle)> {
    let record = entity.common().extended_data.records().iter()
        .find(|record| record.application_name == SWEEP_SOURCES_APP)?;
    let mut handles = record.values.iter().filter_map(|value| match value {
        codec::xdata::XDataValue::Handle(handle) => Some(*handle),
        _ => None,
    });
    Some((handles.next()?, handles.next()?))
}

/// The surface swept again from its current profile and path, with the
/// options it was made with: (surface record, body).
pub fn resweep_from_sources(
    surface: &EntityType,
    profile: &EntityType,
    path: &EntityType,
) -> Option<(SolidHistorySweep, Body)> {
    use codec::xdata::XDataValue;
    let record = surface.common().extended_data.records().iter()
        .find(|record| record.application_name == SWEEP_SOURCES_APP)?;
    let EntityType::Surface(value) = surface else { return None };
    let SurfaceData::Swept { options: stored, .. } = &value.surface_data else { return None };
    let mut options = SweepOptions {
        scale: stored.scale_factor,
        twist_angle: stored.twist_angle,
        ..SweepOptions::default()
    };
    let mut values = record.values.iter();
    while let Some(value) = values.next() {
        match value {
            XDataValue::Integer16(flags) => {
                options.align = flags & 1 != 0;
                options.bank = flags & 2 != 0;
                // Swept from the end: picked there.
                if flags & 4 != 0 {
                    options.path_pick = path_ends(path).map(|(_, end)| end);
                }
            }
            XDataValue::String(key) if key == "BASE" => {
                if let Some(XDataValue::Point3D(p)) = values.next() {
                    options.base_point = Some(glam::DVec3::new(p.x, p.y, p.z));
                }
            }
            _ => {}
        }
    }
    let record = if spatial_profile(profile).is_some() {
        spatial_sweep_record(profile, path, options).ok()?
    } else {
        sweep_record(profile, path, options)?
    };
    let body = kernel::acis::rebuild_sweep_with_mode(&record, true).ok()?;
    Some((record, body))
}

/// An entity's curve as the reference stores it in an edge action
/// parameter: line segment (23: start, vector to the end), arc (11:
/// centre, normal, reference axis, radius, start and end angle, 0) or a
/// composite of those (47: count, then each part's type and values).
pub fn assoc_edge_curve(entity: &EntityType) -> Option<(i32, codec::objects::AssocSubcurveKind, Vec<codec::objects::AssocCurveValue>)> {
    use codec::objects::{AssocCurveValue as V, AssocSubcurveKind as K};
    use codec::types::Vector3 as P;
    let point = |p: glam::DVec3| V::Point(P::new(p.x, p.y, p.z));
    let segment = |a: glam::DVec3, b: glam::DVec3| vec![point(a), point(b - a)];
    // An arc about `normal` from `start` through `sweep` (signed, about the
    // normal) on a circle of `radius`; measured from the reference axis.
    let arc = |centre: glam::DVec3, normal: glam::DVec3, axis: glam::DVec3, radius: f64, start: f64, end: f64| vec![
        point(centre), point(normal), point(axis), V::Real(radius), V::Real(start), V::Real(end), V::Real(0.0),
    ];
    match entity {
        EntityType::Line(line) => {
            let (a, b) = (glam::DVec3::new(line.start.x, line.start.y, line.start.z), glam::DVec3::new(line.end.x, line.end.y, line.end.z));
            Some((23, K::LineSegment3d, segment(a, b)))
        }
        EntityType::Circle(circle) => {
            let normal = glam::DVec3::new(circle.normal.x, circle.normal.y, circle.normal.z).try_normalize()?;
            let axis = arbitrary_x(normal);
            Some((11, K::Arc, arc(glam::DVec3::new(circle.center.x, circle.center.y, circle.center.z), normal, axis, circle.radius, 0.0, std::f64::consts::TAU)))
        }
        EntityType::Arc(value) => {
            let normal = glam::DVec3::new(value.normal.x, value.normal.y, value.normal.z).try_normalize()?;
            let axis = arbitrary_x(normal);
            // The end past the start by the arc's sweep; an equal pair is a
            // full turn. (A loop adding TAU never ends on a huge angle.)
            let sweep = (value.end_angle - value.start_angle).rem_euclid(std::f64::consts::TAU);
            let end = value.start_angle + if sweep > 0.0 { sweep } else { std::f64::consts::TAU };
            Some((11, K::Arc, arc(glam::DVec3::new(value.center.x, value.center.y, value.center.z), normal, axis, value.radius, value.start_angle, end)))
        }
        EntityType::LwPolyline(polyline) => {
            let normal = glam::DVec3::new(polyline.normal.x, polyline.normal.y, polyline.normal.z).try_normalize()?;
            let axis_x = arbitrary_x(normal);
            let axis_y = normal.cross(axis_x);
            let at = |v: &codec::entities::LwVertex| axis_x * v.location.x + axis_y * v.location.y + normal * polyline.elevation;
            let count = polyline.vertices.len();
            let spans = if polyline.is_closed { count } else { count.saturating_sub(1) };
            if spans == 0 { return None; }
            let mut values = vec![V::Int(spans as i32)];
            for index in 0..spans {
                let (from, to) = (&polyline.vertices[index], &polyline.vertices[(index + 1) % count]);
                let (a, b) = (at(from), at(to));
                if from.bulge.abs() <= 1e-12 {
                    values.push(V::Int(23));
                    values.extend(segment(a, b));
                } else {
                    // The arc in the polyline's plane; a clockwise one turns about
                    // the opposite normal, from the same reference axis.
                    let bulge = from.bulge;
                    let chord = b - a;
                    let centre = a + chord * 0.5 + normal.cross(chord) * ((1.0 - bulge * bulge) / (4.0 * bulge));
                    let radius = chord.length() * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
                    let turn = normal * bulge.signum();
                    let across = turn.cross(axis_x);
                    let angle = |p: glam::DVec3| (p - centre).dot(across).atan2((p - centre).dot(axis_x));
                    let start = angle(a);
                    let end = start + 4.0 * bulge.atan().abs();
                    values.push(V::Int(11));
                    values.extend(arc(centre, turn, axis_x, radius, start, end));
                }
            }
            Some((47, K::None, values))
        }
        EntityType::Polyline3D(polyline) => {
            let points = polyline.vertices.iter().map(|v| glam::DVec3::new(v.position.x, v.position.y, v.position.z)).collect::<Vec<_>>();
            let spans = if polyline.is_closed() { points.len() } else { points.len().saturating_sub(1) };
            if spans == 0 { return None; }
            let mut values = vec![V::Int(spans as i32)];
            for index in 0..spans {
                values.push(V::Int(23));
                values.extend(segment(points[index], points[(index + 1) % points.len()]));
            }
            Some((47, K::None, values))
        }
        _ => None,
    }
}

/// The arbitrary-axis X direction of an entity normal.
fn arbitrary_x(normal: glam::DVec3) -> glam::DVec3 {
    let world = if normal.x.abs() < 1.0 / 64.0 && normal.y.abs() < 1.0 / 64.0 { glam::DVec3::Y } else { glam::DVec3::Z };
    world.cross(normal).normalize()
}

/// The scale and twist expressions a swept surface is linked to.
pub fn sweep_expressions(entity: &EntityType) -> Option<[Option<String>; 2]> {
    let record = entity.common().extended_data.records().iter()
        .find(|record| record.application_name == SWEEP_EXPRESSION_APP)?;
    let strings = record.values.iter().filter_map(|value| match value {
        codec::xdata::XDataValue::String(text) => Some(text.clone()),
        _ => None,
    }).collect::<Vec<_>>();
    let mut result = [None, None];
    for pair in strings.chunks(2) {
        if let [name, expression] = pair {
            match name.as_str() {
                "ScaleFactor" => result[0] = Some(expression.clone()),
                "TwistAngle" => result[1] = Some(expression.clone()),
                _ => {}
            }
        }
    }
    Some(result)
}

/// The sweep record a swept surface was made from.
pub fn surface_sweep_record(entity: &EntityType) -> Option<SolidHistorySweep> {
    let EntityType::Surface(surface) = entity else { return None };
    let SurfaceData::Swept { sweep_entity, path_entity, options, .. } = &surface.surface_data else { return None };
    let mut base = SolidHistoryNodeBase::new(1);
    base.transform = glam::DMat4::IDENTITY.to_cols_array();
    Some(SolidHistorySweep {
        base,
        operation_major: 1,
        sweep_entity: sweep_entity.clone(),
        path_entity: path_entity.clone(),
        draft_angle: options.draft_angle,
        scale_factor: options.scale_factor,
        twist_angle: options.twist_angle,
        align_angle: options.align_angle,
        align_option: options.sweep_alignment_flags as u8,
        has_align_start: true,
        align_start: options.align_start,
        bank: options.bank,
        sweep_entity_transform: options.sweep_entity_transform,
        path_entity_transform: options.path_entity_transform,
        reference_point: options.reference_vector,
        flags_294_296: [false, options.sweep_entity_transform_computed, options.path_entity_transform_computed],
        miter_option: 2,
        ..SolidHistorySweep::default()
    })
}

/// Preserve native construction parameters alongside the sheet's saved B-rep.
pub fn swept_surface_entity(record: &SolidHistorySweep) -> EntityType {
    let mut surface = Surface::new(SurfaceKind::Swept);
    if let Ok(point) = kernel::acis::sweep_history_reference_point(record) {
        surface.point_of_reference = Vector3::new(point[0], point[1], point[2]);
    }
    surface.surface_data = SurfaceData::Swept {
        class_version: 0,
        sweep_entity: record.sweep_entity.clone(),
        path_entity: record.path_entity.clone(),
        sweep_transform: glam::DMat4::IDENTITY.to_cols_array(),
        path_transform: glam::DMat4::IDENTITY.to_cols_array(),
        options: SurfaceSweepOptions {
            draft_angle: record.draft_angle,
            twist_angle: record.twist_angle,
            scale_factor: record.scale_factor,
            align_angle: record.align_angle,
            sweep_entity_transform: record.sweep_entity_transform,
            path_entity_transform: record.path_entity_transform,
            sweep_alignment_flags: record.align_option as i16,
            align_start: record.align_start,
            bank: record.bank,
            base_point_set: true,
            sweep_entity_transform_computed: record.flags_294_296[1],
            path_entity_transform_computed: record.flags_294_296[2],
            reference_vector: record.reference_point,
            ..SurfaceSweepOptions::default()
        },
    };
    EntityType::Surface(Box::new(surface))
}
