//! Resolve a dimension feature through `[viewport?, insert..., entity]`.
//! `xrefs` stores the primary chain; `intersection_objects` stores the second
//! intersection source. Features are evaluated in entity-local coordinates and
//! transformed through their block instances. The caller projects viewport
//! references onto the sheet. All arithmetic uses `f64`.

use acadrust::objects::AssocDimensionReference;
use acadrust::types::{Handle, Matrix4, Transform, Vector3};
use acadrust::{CadDocument, EntityType};
use cadkernel::geom2d::{closest_point, Curve as KernelCurve};
use cadkernel::space::Plane;

use crate::entities::curve::entity_curve;

/// `AcDb::OsnapMode`, the value stored in
/// [`AssocDimensionReference::osnap_type`].
///
/// These are ObjectARX codes, independent of [`crate::snap::SnapType`].
#[allow(dead_code)] // Include codes preserved from unsupported imported modes.
pub(crate) mod osnap {
    pub const NONE: u8 = 0;
    pub const END: u8 = 1;
    pub const MID: u8 = 2;
    pub const CEN: u8 = 3;
    pub const NODE: u8 = 4;
    pub const QUAD: u8 = 5;
    pub const INTERSEC: u8 = 6;
    pub const INS: u8 = 7;
    pub const PERP: u8 = 8;
    pub const TAN: u8 = 9;
    pub const NEAR: u8 = 10;
    pub const APPARENT_INT: u8 = 11;
    pub const PARA: u8 = 12;
    pub const START_POINT: u8 = 13;
}

/// Translate a live snap mode into the `AcDb::OsnapMode` we persist.
///
/// Modes with no ObjectARX equivalent (grid, object pick, 3D solid snaps) map
/// to [`osnap::NONE`]; a reference carrying `NONE` resolves by
/// marker/parameter alone.
pub(crate) fn osnap_type_for(snap: crate::snap::SnapType) -> u8 {
    use crate::snap::SnapType as S;
    match snap {
        S::Endpoint => osnap::END,
        S::Midpoint => osnap::MID,
        S::Center => osnap::CEN,
        S::Node => osnap::NODE,
        S::Quadrant => osnap::QUAD,
        S::Intersection => osnap::INTERSEC,
        S::Insertion => osnap::INS,
        S::Perpendicular => osnap::PERP,
        S::Tangent => osnap::TAN,
        S::Nearest | S::Extension => osnap::NEAR,
        S::ApparentIntersection => osnap::APPARENT_INT,
        S::Parallel => osnap::PARA,
        S::Grid | S::ObjectPick | S::Vertex | S::EdgeMidpoint | S::FaceCenter | S::Knot | S::FacePerpendicular | S::NearestFace => {
            osnap::NONE
        }
    }
}

/// A resolved `xrefs` chain.
#[derive(Clone, Debug)]
pub(crate) struct ReferenceChain {
    /// The layout viewport the reference is seen through, when the chain
    /// starts with one. `None` for a plain model-space reference.
    pub viewport: Option<Handle>,
    /// INSERT handles from the outermost instance down to the instance that
    /// directly contains [`Self::entity`]. Empty for top-level geometry.
    pub block_path: Vec<Handle>,
    /// The innermost entity — the one that owns the feature.
    pub entity: Handle,
    /// Block-path transform: entity-local coordinates -> model coordinates.
    /// Identity when `block_path` is empty.
    pub transform: Transform,
}

/// Why a chain could not be walked. The data is never discarded on a failure:
/// the caller keeps the reference exactly as read and leaves the dimension
/// showing its last valid appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChainError {
    /// `xrefs` was empty — nothing to resolve.
    Empty,
    Invalid,
    /// A handle in the chain names an object that is not in the document
    /// (erased source; restored by an undo).
    Missing(Handle),
}

/// Walk `xrefs` into a [`ReferenceChain`].
///
/// The chain is read positionally rather than by a stored kind, because that
/// is all the file gives us: a leading VIEWPORT is the trans-space hop, any
/// INSERTs after it are the block-instance path, and the first handle that is
/// neither is the geometry. A chain that is nothing but INSERTs (dimensioning
/// a block reference's insertion point) ends on the innermost INSERT.
pub(crate) fn walk_chain(
    document: &CadDocument,
    xrefs: &[Handle],
) -> Result<ReferenceChain, ChainError> {
    if xrefs.is_empty() {
        return Err(ChainError::Empty);
    }
    let mut viewport = None;
    let mut block_path = Vec::new();
    let mut transform = Transform::identity();
    let mut owner = None;
    for (index, &handle) in xrefs.iter().enumerate() {
        if handle.is_null() {
            return Err(ChainError::Invalid);
        }
        let node = document
            .get_entity(handle)
            .ok_or(ChainError::Missing(handle))?;
        if owner.is_some_and(|owner| node.common().owner_handle != owner) {
            return Err(ChainError::Invalid);
        }
        if index == 0 && matches!(node, EntityType::Viewport(_)) {
            viewport = Some(handle);
            owner = document.block_records.get("*Model_Space").map(|b| b.handle);
            continue;
        }
        if index == xrefs.len() - 1 {
            return Ok(ReferenceChain {
                viewport,
                block_path,
                entity: handle,
                transform,
            });
        }
        let EntityType::Insert(insert) = node else {
            return Err(ChainError::Invalid);
        };
        // Arrays need an instance index that this reference format does not carry.
        if insert.row_count > 1
            || insert.column_count > 1
            || block_path.len() >= 8
            || block_path.contains(&handle)
        {
            return Err(ChainError::Invalid);
        }
        let block = document
            .block_records
            .get(&insert.block_name)
            .ok_or(ChainError::Invalid)?;
        owner = Some(block.handle);
        block_path.push(handle);
        transform = transform.compose(&crate::scene::render_graph::insert_transform(
            document, insert,
        ));
    }
    Err(ChainError::Empty)
}

/// Invert an affine block transform, rejecting singular or nonfinite matrices.
pub(crate) fn invert(transform: &Transform) -> Option<Transform> {
    let m = &transform.matrix.m;
    let matrix = glam::DMat4::from_cols_array_2d(&std::array::from_fn(|column| {
        std::array::from_fn(|row| m[row][column])
    }));
    if !matrix.is_finite() || matrix.determinant().abs() < 1e-18 {
        return None;
    }
    let inverse = matrix.inverse();
    if !inverse.is_finite() {
        return None;
    }
    let columns = inverse.to_cols_array_2d();
    Some(Transform::from_matrix(Matrix4 {
        m: std::array::from_fn(|row| std::array::from_fn(|column| columns[column][row])),
    }))
}

// ── Feature evaluation ────────────────────────────────────────────────────

fn vector3(point: [f64; 3]) -> Vector3 {
    Vector3::new(point[0], point[1], point[2])
}

fn dpoint(point: Vector3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn distance_squared(a: Vector3, b: Vector3) -> f64 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    dx * dx + dy * dy + dz * dz
}

fn lift(plane: &Plane, uv: [f64; 2]) -> Vector3 {
    vector3(plane.point_at(uv))
}

/// Everything a feature evaluation may need beyond the reference itself.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FeatureContext {
    /// The stored osnap point, already expressed in *entity-local*
    /// coordinates. Used as the disambiguating hint for every feature that has
    /// more than one candidate (quadrant, intersection, tangent, nearest).
    pub hint: Option<Vector3>,
    /// The point the feature is measured *from*, in entity-local coordinates.
    /// PERPENDICULAR and TANGENT are two-point features: the foot / touch
    /// point only exists relative to an external point, which for a dimension
    /// is the other definition point. Tangency requires this point.
    pub from: Option<Vector3>,
}

/// Evaluate the osnap feature `reference` names on `entity`, in the entity's
/// own coordinates.
///
/// Returns `None` — never a guess — when the osnap type is one we do not
/// evaluate, or when the geometry no longer supports it. The caller keeps the
/// reference data intact and leaves the dimension where it was.
pub(crate) fn feature_point(
    document: &CadDocument,
    entity: &EntityType,
    reference: &AssocDimensionReference,
    context: FeatureContext,
) -> Option<Vector3> {
    match reference.osnap_type {
        osnap::END | osnap::START_POINT if reference.main_subent_type == 2 => {
            imported_endpoint(entity, reference, context.hint)
        }
        osnap::CEN => center_point(entity, reference),
        osnap::MID => mid_point(entity, reference),
        osnap::QUAD => quadrant_point(entity, reference, context.hint?),
        osnap::NODE | osnap::INS => node_point(entity),
        osnap::INTERSEC | osnap::APPARENT_INT => intersection_point(
            document,
            entity,
            reference,
            context.hint?,
            reference.osnap_type == osnap::APPARENT_INT,
        ),
        osnap::PERP => perpendicular_point(entity, context.from.or(context.hint)?),
        osnap::TAN => tangent_point(entity, reference, context),
        osnap::NEAR => nearest_point(entity, reference, context.hint),
        // END / START_POINT / PARA / NONE all address a named vertex or a
        // stored curve parameter, which is what the legacy marker convention
        // already encodes. The caller keeps that path.
        _ => None,
    }
}

/// Whether [`feature_point`] knows how to evaluate this osnap type. A type we
/// do not handle keeps its stored data and stays unresolved.
pub(crate) fn evaluates(osnap_type: u8) -> bool {
    matches!(
        osnap_type,
        osnap::END
            | osnap::START_POINT
            | osnap::CEN
            | osnap::MID
            | osnap::QUAD
            | osnap::NODE
            | osnap::INS
            | osnap::INTERSEC
            | osnap::APPARENT_INT
            | osnap::PERP
            | osnap::TAN
            | osnap::NEAR
    )
}

/// Whether this osnap type needs the *other* definition point to evaluate.
pub(crate) fn needs_from_point(osnap_type: u8) -> bool {
    matches!(osnap_type, osnap::PERP | osnap::TAN)
}

fn center_point(entity: &EntityType, reference: &AssocDimensionReference) -> Option<Vector3> {
    match entity {
        EntityType::Circle(circle) => Some(circle.center_wcs()),
        EntityType::Arc(arc) => Some(arc.center_wcs()),
        EntityType::Ellipse(ellipse) => Some(ellipse.center),
        _ => {
            // A polyline's bulge segment: `main_gs_marker` names the segment.
            let planar = entity_curve(entity)?;
            let KernelCurve::Polyline(polyline) = &planar.curve else {
                return None;
            };
            let segment = if reference.main_subent_type == 1
                && reference.main_gs_marker == super::dimension_assoc::POLYLINE_ARC_CENTER_MARKER
            {
                reference.osnap_distance.round().max(0.0) as usize
            } else {
                segment_index(reference)?
            };
            let arc = polyline.segment_arc(segment)?;
            Some(lift(&planar.plane, arc.center))
        }
    }
}

fn mid_point(entity: &EntityType, reference: &AssocDimensionReference) -> Option<Vector3> {
    let planar = entity_curve(entity)?;
    match &planar.curve {
        // For a polyline the marker names which segment's midpoint it is;
        // without one, the midpoint of the whole run.
        KernelCurve::Polyline(_) if reference.main_gs_marker >= 0 => {
            let segments = planar.curve.segments();
            let segment = segments.get(segment_index(reference)?)?;
            Some(lift(&planar.plane, segment.point_at(0.5)))
        }
        _ => Some(lift(&planar.plane, planar.curve.point_at(0.5))),
    }
}

fn quadrant_point(
    entity: &EntityType,
    reference: &AssocDimensionReference,
    hint: Vector3,
) -> Option<Vector3> {
    let planar = entity_curve(entity)?;
    let (centre, radius) = match &planar.curve {
        KernelCurve::Circle(circle) => (circle.centre, circle.radius),
        KernelCurve::Arc(arc) => (arc.centre, arc.radius),
        _ => return None,
    };
    let valid_angle = |angle: f64| match &planar.curve {
        KernelCurve::Arc(arc) => {
            cadkernel::geom2d::angle_within_arc(angle, arc.start_angle, arc.end_angle)
        }
        _ => true,
    };
    let at_angle = |angle: f64| {
        lift(
            &planar.plane,
            KernelCurve::Circle(cadkernel::geom2d::Circle { centre, radius })
                .point_at(angle.rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU),
        )
    };
    // New references retain the chosen source-local angle, so translation
    // cannot silently select another quadrant using the old world point.
    if reference.main_subent_type == 1 && reference.main_gs_marker == -2 {
        let angle = reference.osnap_distance;
        return (angle.is_finite() && valid_angle(angle)).then(|| at_angle(angle));
    }
    // An imported point can identify an unchanged quadrant, but cannot prove
    // which quadrant was intended after it moves away from the stored hint.
    (0..4)
        .map(|index| std::f64::consts::FRAC_PI_2 * index as f64)
        .filter(|angle| valid_angle(*angle))
        .map(at_angle)
        .find(|candidate| distance_squared(*candidate, hint) < 1e-12)
}

fn node_point(entity: &EntityType) -> Option<Vector3> {
    match entity {
        EntityType::Point(point) => Some(point.location),
        EntityType::Insert(insert) => Some(insert.insert_point),
        EntityType::Text(text) => Some(text.insertion_point),
        EntityType::MText(text) => Some(text.insertion_point),
        EntityType::Shape(shape) => Some(shape.insertion_point),
        _ => None,
    }
}

fn perpendicular_point(entity: &EntityType, from: Vector3) -> Option<Vector3> {
    let planar = entity_curve(entity)?;
    let uv = planar.plane.project(dpoint(from))?;
    cadkernel::geom2d::snap::perpendicular_from(&planar.curve, uv)
        .into_iter()
        .map(|candidate| lift(&planar.plane, candidate.point))
        .min_by(|a, b| distance_squared(*a, from).total_cmp(&distance_squared(*b, from)))
}

/// Perpendicularity must be evaluated after a block's nonuniform scale.
pub(crate) fn perpendicular_in_model(
    entity: &EntityType,
    transform: &Transform,
    from: Vector3,
) -> Option<Vector3> {
    use glam::DVec3;
    let source = entity_curve(entity)?;
    let origin = transform.apply(vector3(source.plane.origin));
    let x = DVec3::from_array(dpoint(
        transform.apply_rotation(vector3(source.plane.x_axis)),
    ));
    let y = DVec3::from_array(dpoint(
        transform.apply_rotation(vector3(source.plane.y_axis)),
    ));
    let axis_x = x.try_normalize()?;
    let normal = x.cross(y).try_normalize()?;
    let axis_y = normal.cross(axis_x);
    let plane = Plane::from_axes(dpoint(origin), axis_x.to_array(), axis_y.to_array());
    let curve = source.curve.transformed(&cadkernel::geom2d::Transform {
        origin: [0.0, 0.0].into(),
        x_axis: [x.dot(axis_x), x.dot(axis_y)].into(),
        y_axis: [y.dot(axis_x), y.dot(axis_y)].into(),
    })?;
    let uv = plane.project(dpoint(from))?;
    cadkernel::geom2d::snap::perpendicular_from(&curve, uv)
        .into_iter()
        .map(|candidate| lift(&plane, candidate.point))
        .min_by(|a, b| distance_squared(*a, from).total_cmp(&distance_squared(*b, from)))
}

fn nearest_point(
    entity: &EntityType,
    reference: &AssocDimensionReference,
    hint: Option<Vector3>,
) -> Option<Vector3> {
    let planar = entity_curve(entity)?;
    // Circles and arcs store an angle; other curves use a normalized parameter.
    match &planar.curve {
        KernelCurve::Circle(_) | KernelCurve::Arc(_) => match entity {
            EntityType::Circle(circle) => Some(circle.point_at_angle_wcs(reference.osnap_distance)),
            EntityType::Arc(arc) => Some(arc.point_at_angle_wcs(reference.osnap_distance)),
            _ => None,
        },
        _ => {
            let parameter = reference.osnap_distance;
            if parameter.is_finite() && (0.0..=1.0).contains(&parameter) {
                return Some(lift(&planar.plane, planar.curve.point_at(parameter)));
            }
            // No usable parameter: fall back to the point on the curve nearest
            // the stored osnap point, which is what NEAR means anyway.
            let uv = planar.plane.project(dpoint(hint?))?;
            Some(lift(&planar.plane, closest_point(&planar.curve, uv).point))
        }
    }
}

// ── Tangent ───────────────────────────────────────────────────────────────

/// Tangent point for the line through `context.from`.
fn tangent_point(
    entity: &EntityType,
    reference: &AssocDimensionReference,
    context: FeatureContext,
) -> Option<Vector3> {
    if let EntityType::Spline(spline) = entity {
        return spline_tangent_point(spline, reference, context);
    }
    let planar = entity_curve(entity)?;
    let from = context.from?;
    let uv = planar.plane.project(dpoint(from))?;
    cadkernel::geom2d::snap::tangent_from(&planar.curve, uv)
        .into_iter()
        .map(|candidate| lift(&planar.plane, candidate.point))
        .min_by(|a, b| {
            distance_squared(*a, context.hint.unwrap_or(from))
                .total_cmp(&distance_squared(*b, context.hint.unwrap_or(from)))
        })
}

fn spline_tangent_point(
    spline: &acadrust::entities::Spline,
    reference: &AssocDimensionReference,
    context: FeatureContext,
) -> Option<Vector3> {
    let curve = crate::entities::spline::nurbs3(spline)?;
    let (start, end) = curve.domain();
    let stored = reference.osnap_distance;
    let u = if stored.is_finite() && stored > start - 1e-9 && stored < end + 1e-9 {
        stored.clamp(start, end)
    } else {
        let hint = context.hint.or(context.from)?;
        curve.parameter_at(dpoint(hint)).clamp(start, end)
    };
    curve
        .tangent_from_near(dpoint(context.from?), u)
        .map(vector3)
}

// ── Intersection ──────────────────────────────────────────────────────────

/// Intersection of `entity` with the reference's second object chain.
///
/// Transform both curves into the first entity's local plane and use the
/// kernel's exact/converged intersection solver. Apparent intersections may
/// project noncoplanar curves; only straight lines support an extended crossing.
fn intersection_point(
    document: &CadDocument,
    entity: &EntityType,
    reference: &AssocDimensionReference,
    hint: Vector3,
    apparent: bool,
) -> Option<Vector3> {
    let main = walk_chain(document, &reference.xrefs).ok()?;
    let other = walk_chain(document, &reference.intersection_objects).ok()?;
    if main.viewport != other.viewport {
        return None;
    }
    let inverse = invert(&main.transform)?;
    let first = entity_curve(entity)?;
    let second = entity_curve(document.get_entity(other.entity)?)?;
    let map = |uv| {
        let local = inverse.apply(other.transform.apply(lift(&second.plane, uv)));
        let projected = first.plane.project(dpoint(local))?;
        if !apparent && distance_squared(local, lift(&first.plane, projected)) > 1e-12 {
            return None;
        }
        Some(projected)
    };
    let origin = glam::DVec2::from_array(map([0.0, 0.0])?);
    let x = glam::DVec2::from_array(map([1.0, 0.0])?) - origin;
    let y = glam::DVec2::from_array(map([0.0, 1.0])?) - origin;
    let second = second.curve.transformed(&cadkernel::geom2d::Transform {
        origin: origin.to_array().into(),
        x_axis: x.to_array().into(),
        y_axis: y.to_array().into(),
    })?;
    let hint_uv = first.plane.project(dpoint(hint))?;
    let mut points: Vec<_> = cadkernel::geom2d::intersect(
        &first.curve,
        &second,
        cadkernel::geom2d::Tolerance::new(1e-9),
    )
    .into_iter()
    .map(|hit| hit.point)
    .collect();
    if apparent && points.is_empty() {
        // Extended apparent intersections are meaningful for straight lines.
        // Curved geometry must actually cross after projection.
        if let (KernelCurve::Line(a), KernelCurve::Line(b)) = (&first.curve, &second) {
            let extend = |line: &cadkernel::geom2d::Line| {
                KernelCurve::XLine(cadkernel::geom2d::XLine {
                    base: line.start,
                    direction: [line.end[0] - line.start[0], line.end[1] - line.start[1]],
                })
            };
            points.extend(
                cadkernel::geom2d::intersect(
                    &extend(a),
                    &extend(b),
                    cadkernel::geom2d::Tolerance::new(1e-9),
                )
                .into_iter()
                .map(|hit| hit.point),
            );
        }
    }
    points
        .into_iter()
        .min_by(|a, b| {
            let error = |p: &[f64; 2]| (p[0] - hint_uv[0]).powi(2) + (p[1] - hint_uv[1]).powi(2);
            error(a).total_cmp(&error(b))
        })
        .map(|point| lift(&first.plane, point))
}

/// Internal markers are zero-based; imported edge markers are one-based.
fn segment_index(reference: &AssocDimensionReference) -> Option<usize> {
    let marker = if reference.main_subent_type == 2 {
        reference.main_gs_marker.checked_sub(1)?
    } else {
        reference.main_gs_marker
    };
    usize::try_from(marker).ok()
}

fn imported_endpoint(
    entity: &EntityType,
    reference: &AssocDimensionReference,
    hint: Option<Vector3>,
) -> Option<Vector3> {
    let planar = entity_curve(entity)?;
    let t = match reference.osnap_distance {
        v if v.abs() < 1e-9 => 0.0,
        v if (v - 1.0).abs() < 1e-9 => 1.0,
        _ => return None,
    };
    let point = match &planar.curve {
        KernelCurve::Polyline(_) => {
            let segment = segment_index(reference)?;
            let point = lift(
                &planar.plane,
                planar.curve.segments().get(segment)?.point_at(t),
            );
            // Exporters can leave stale GS markers after editing a polyline.
            // Beyond the unambiguous first-edge convention, require a real
            // stored snap point to validate this imported endpoint. Zero and
            // 2e50 sentinel points do not establish feature identity.
            if segment > 0 && !hint.is_some_and(|hint| distance_squared(point, hint) < 1e-10) {
                return None;
            }
            point
        }
        KernelCurve::Line(_) | KernelCurve::Arc(_) | KernelCurve::Nurbs(_) => {
            lift(&planar.plane, planar.curve.point_at(t))
        }
        _ => return None,
    };
    Some(point)
}
