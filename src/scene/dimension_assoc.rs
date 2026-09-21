use acadrust::entities::Dimension;
use acadrust::objects::{
    AssocDimensionAssociation, AssocDimensionReference, AssociativeData, AssociativeObject,
    ObjectType,
};
use acadrust::types::{Handle, Transform, Vector3};
use acadrust::EntityType;
use cadkernel::geom2d::{
    angle_within_arc, arc as tessellate_arc, arc_span, closest_point, Arc as KernelArc, BulgeArc,
    Circle as KernelCircle, Curve as KernelCurve, DEFAULT_SEGMENTS_PER_RADIAN,
};
use cadkernel::space::Plane;
use std::f64::consts::TAU;

use crate::command::DimensionAssociationSource;

use super::dimension_assoc_chain::{self as chain, ChainError, FeatureContext, ReferenceChain};
use super::viewport_ref::{AcceptedSnap, MeasurementScale, ViewportFrame};
use super::{ChangeKind, Scene};

pub(crate) const POLYLINE_ARC_CENTER_MARKER: i32 = -4;
const POLYLINE_ARC_POINT_MARKER_BASE: i32 = -5;
pub(crate) const ARC_DIMENSION_POINT_MARKER: i32 = -1;

pub(crate) fn polyline_arc_point_marker(segment: i32) -> i32 {
    POLYLINE_ARC_POINT_MARKER_BASE - segment.max(0)
}

fn polyline_arc_segment_from_point_marker(marker: i32) -> Option<i32> {
    (marker <= POLYLINE_ARC_POINT_MARKER_BASE).then_some(POLYLINE_ARC_POINT_MARKER_BASE - marker)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RadialSourceGeometry {
    pub plane: Plane,
    pub center: [f64; 2],
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub limited: bool,
    pub marker: i32,
}

impl RadialSourceGeometry {
    pub(crate) fn center_world(self) -> Vector3 {
        vector3(self.plane.point_at(self.center))
    }

    pub(crate) fn point_at_angle(self, angle: f64) -> Vector3 {
        vector3(self.plane.point_at([
            self.center[0] + self.radius * angle.cos(),
            self.center[1] + self.radius * angle.sin(),
        ]))
    }

    pub(crate) fn angle_at(self, point: DPoint) -> f64 {
        let projected = self.plane.project(point).unwrap_or(self.center);
        (projected[1] - self.center[1]).atan2(projected[0] - self.center[0])
    }

    pub(crate) fn chord_at(self, point: DPoint) -> Vector3 {
        self.point_at_angle(self.angle_at(point))
    }

    pub(crate) fn opposite_chord_at(self, point: DPoint) -> Vector3 {
        self.point_at_angle(self.angle_at(point) + std::f64::consts::PI)
    }

    fn contains_angle(self, angle: f64) -> bool {
        !self.limited || angle_within_arc(angle, self.start_angle, self.end_angle)
    }

    fn curve(self) -> KernelCurve {
        if self.limited {
            KernelCurve::Arc(KernelArc {
                centre: self.center,
                radius: self.radius,
                start_angle: self.start_angle,
                end_angle: self.end_angle,
            })
        } else {
            KernelCurve::Circle(KernelCircle {
                centre: self.center,
                radius: self.radius,
            })
        }
    }

    fn distance_squared_to(self, point: DPoint) -> f64 {
        let Some(projected) = self.plane.project(point) else {
            return f64::INFINITY;
        };
        let planar = closest_point(&self.curve(), projected).distance;
        let world = self.plane.point_at(projected);
        let dx = world[0] - point[0];
        let dy = world[1] - point[1];
        let dz = world[2] - point[2];
        planar * planar + dx * dx + dy * dy + dz * dz
    }
}

type DPoint = [f64; 3];

/// The shared-contract vector (`glam`) as the codec's vector. `AcceptedSnap`
/// speaks `glam` because the snap engine does; everything stored in a
/// drawing speaks `acadrust`.
fn dvec3(point: glam::DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

fn vector3(point: DPoint) -> Vector3 {
    Vector3::new(point[0], point[1], point[2])
}

fn dpoint(point: Vector3) -> DPoint {
    [point.x, point.y, point.z]
}

fn radial_candidates(entity: &EntityType) -> Vec<RadialSourceGeometry> {
    let Some(planar) = crate::entities::curve::entity_curve(entity) else {
        return Vec::new();
    };
    match planar.curve {
        KernelCurve::Circle(circle) => vec![RadialSourceGeometry {
            plane: planar.plane,
            center: circle.centre,
            radius: circle.radius,
            start_angle: 0.0,
            end_angle: 0.0,
            limited: false,
            marker: 0,
        }],
        KernelCurve::Arc(arc) => vec![RadialSourceGeometry {
            plane: planar.plane,
            center: arc.centre,
            radius: arc.radius,
            start_angle: arc.start_angle,
            end_angle: arc.end_angle,
            limited: true,
            marker: 0,
        }],
        KernelCurve::Polyline(polyline) => (0..polyline.vertices.len())
            .filter_map(|index| {
                let arc = polyline.segment_arc(index)?;
                let (start_angle, end_angle) = if arc.sweep >= 0.0 {
                    (arc.start_angle, arc.end_angle)
                } else {
                    (arc.end_angle, arc.start_angle)
                };
                Some(RadialSourceGeometry {
                    plane: planar.plane,
                    center: arc.center,
                    radius: arc.radius,
                    start_angle,
                    end_angle,
                    limited: true,
                    marker: index as i32,
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn radial_source_at(
    entity: &EntityType,
    point: Vector3,
) -> Option<RadialSourceGeometry> {
    radial_candidates(entity)
        .into_iter()
        .min_by(|first, second| {
            first
                .distance_squared_to(dpoint(point))
                .total_cmp(&second.distance_squared_to(dpoint(point)))
        })
}

fn radial_source_for_marker(entity: &EntityType, marker: i32) -> Option<RadialSourceGeometry> {
    radial_candidates(entity)
        .into_iter()
        .find(|candidate| candidate.marker == marker)
}

fn radial_source_matching(
    entity: &EntityType,
    center: Vector3,
    radius: f64,
    chord: Vector3,
) -> Option<RadialSourceGeometry> {
    radial_candidates(entity)
        .into_iter()
        .min_by(|first, second| {
            let score = |candidate: &RadialSourceGeometry| {
                point_distance_squared(candidate.center_world(), center)
                    + (candidate.radius - radius).powi(2)
                    + candidate.distance_squared_to(dpoint(chord)) * 1e-6
            };
            score(first).total_cmp(&score(second))
        })
}

fn point_distance_squared(first: Vector3, second: Vector3) -> f64 {
    let dx = first.x - second.x;
    let dy = first.y - second.y;
    let dz = first.z - second.z;
    dx * dx + dy * dy + dz * dz
}

fn ocs_point(x: f64, y: f64, elevation: f64, normal: Vector3) -> Vector3 {
    let point = crate::scene::view::transform::ocs_point_to_wcs(
        (x, y, elevation),
        (normal.x, normal.y, normal.z),
    );
    Vector3::new(point.0, point.1, point.2)
}

fn circle_curve(circle: &acadrust::entities::Circle) -> KernelCurve {
    KernelCurve::Circle(KernelCircle {
        centre: [circle.center.x, circle.center.y],
        radius: circle.radius,
    })
}

fn bulge_center_world(
    first: [f64; 2],
    second: [f64; 2],
    bulge: f64,
    elevation: f64,
    normal: Vector3,
) -> Option<Vector3> {
    let center = BulgeArc::from_bulge(first, second, bulge)?.center;
    Some(ocs_point(center[0], center[1], elevation, normal))
}

fn next_segment_index(count: usize, closed: bool, segment: usize) -> Option<usize> {
    if segment + 1 < count {
        Some(segment + 1)
    } else if closed && segment < count {
        Some(0)
    } else {
        None
    }
}

fn polyline_arc_center(entity: &EntityType, segment: usize) -> Option<Vector3> {
    match entity {
        EntityType::LwPolyline(polyline) => {
            let count = polyline.vertices.len();
            let first = *polyline.vertices.get(segment)?;
            let second =
                *polyline
                    .vertices
                    .get(next_segment_index(count, polyline.is_closed, segment)?)?;
            bulge_center_world(
                [first.location.x, first.location.y],
                [second.location.x, second.location.y],
                first.bulge,
                polyline.elevation,
                polyline.normal,
            )
        }
        EntityType::Polyline2D(polyline) => {
            let count = polyline.vertices.len();
            let first = polyline.vertices.get(segment)?;
            let second =
                polyline
                    .vertices
                    .get(next_segment_index(count, polyline.is_closed(), segment)?)?;
            bulge_center_world(
                [first.location.x, first.location.y],
                [second.location.x, second.location.y],
                first.bulge,
                polyline.elevation,
                polyline.normal,
            )
        }
        _ => None,
    }
}

/// Ordered, named points for an entity (line start/end, polyline vertices,
/// ...), indexed by the same non-negative GsMarker convention
/// `AssocDimensionReference::main_gs_marker` uses. Promoted to
/// `pub(crate)` so the parametric solver can address constraint
/// endpoints the same way associative dimensions already address theirs,
/// rather than inventing a second sub-element scheme.
pub(crate) fn source_points(entity: &EntityType) -> Vec<Vector3> {
    match entity {
        EntityType::Line(line) => vec![line.start, line.end],
        EntityType::Ray(ray) => vec![ray.base_point],
        EntityType::Arc(arc) => vec![arc.start_point_wcs(), arc.end_point_wcs()],
        EntityType::Circle(_) => Vec::new(),
        EntityType::Spline(spline) => crate::entities::spline::nurbs3(spline)
            .map(|curve| {
                let (start, end) = curve.domain();
                [curve.point_at_knot(start), curve.point_at_knot(end)]
                    .into_iter()
                    .map(|point| Vector3::new(point[0], point[1], point[2]))
                    .collect()
            })
            .unwrap_or_default(),
        EntityType::LwPolyline(polyline) => polyline
            .vertices
            .iter()
            .map(|vertex| {
                ocs_point(
                    vertex.location.x,
                    vertex.location.y,
                    polyline.elevation,
                    polyline.normal,
                )
            })
            .collect(),
        EntityType::Polyline2D(polyline) => polyline
            .vertices
            .iter()
            .map(|vertex| {
                ocs_point(
                    vertex.location.x,
                    vertex.location.y,
                    polyline.elevation,
                    polyline.normal,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn source_reference(entity: &EntityType, point: Vector3) -> Option<(i32, f64)> {
    let curve_parameter = match entity {
        EntityType::Circle(circle) => {
            let center = circle.center_wcs();
            if point_distance_squared(center, point) <= 1e-16 {
                return Some((-3, 0.0));
            }
            let (axis_x, axis_y) = circle.axes_wcs();
            let offset = point - center;
            Some(offset.dot(&axis_y).atan2(offset.dot(&axis_x)))
        }
        EntityType::Arc(arc) => {
            let local = crate::scene::view::transform::wcs_point_to_ocs(
                (point.x, point.y, point.z),
                (arc.normal.x, arc.normal.y, arc.normal.z),
            );
            let dx = local.0 - arc.center.x;
            let dy = local.1 - arc.center.y;
            if dx * dx + dy * dy <= 1e-16 {
                return Some((-3, 0.0));
            }
            Some(dy.atan2(dx))
        }
        _ => None,
    };
    if let Some(parameter) = curve_parameter {
        return Some((-2, parameter));
    }
    source_marker(entity, point).map(|marker| (marker, 0.0))
}

fn source_marker(entity: &EntityType, point: Vector3) -> Option<i32> {
    source_points(entity)
        .into_iter()
        .enumerate()
        .min_by(|(_, first), (_, second)| {
            point_distance_squared(*first, point).total_cmp(&point_distance_squared(*second, point))
        })
        .map(|(index, _)| index as i32)
}

// ── Chain-aware reference resolution ──────────────────────────────────────

/// Transform an entity-local feature through its block path and optional viewport.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChainMap {
    pub transform: Transform,
    pub frame: Option<ViewportFrame>,
}

impl ChainMap {
    fn identity() -> Self {
        Self {
            transform: Transform::identity(),
            frame: None,
        }
    }

    /// Entity-local -> model.
    pub(crate) fn to_model(&self, point: Vector3) -> Vector3 {
        self.transform.apply(point)
    }

    /// Model -> the dimension's own space (paper through a viewport).
    pub(crate) fn model_to_space(&self, point: Vector3) -> Vector3 {
        match &self.frame {
            Some(frame) => {
                let mapped = frame.model_to_paper(glam::DVec3::new(point.x, point.y, point.z));
                Vector3::new(mapped.x, mapped.y, mapped.z)
            }
            None => point,
        }
    }

    /// The dimension's own space -> model.
    pub(crate) fn space_to_model(&self, point: Vector3) -> Vector3 {
        match &self.frame {
            Some(frame) => {
                let mapped = frame.paper_to_model(glam::DVec3::new(point.x, point.y, point.z));
                Vector3::new(mapped.x, mapped.y, mapped.z)
            }
            None => point,
        }
    }

    /// Entity-local -> the dimension's own space.
    pub(crate) fn to_space(&self, point: Vector3) -> Vector3 {
        self.model_to_space(self.to_model(point))
    }

    /// Entity-local -> the dimension's own space, for a direction (the linear
    /// part only: block rotation/scale, then the viewport's twist and scale).
    fn direction_to_space(&self, direction: Vector3) -> Vector3 {
        let direction = self.transform.apply_rotation(direction);
        match &self.frame {
            Some(frame) => {
                let xy = frame.model_to_paper_dir(glam::DVec2::new(direction.x, direction.y));
                Vector3::new(xy.x, xy.y, direction.z)
            }
            None => direction,
        }
    }

    /// Carry a plane into the dimension's space.
    ///
    /// A plane is an origin and two axes, and `Plane::point_at` is affine in
    /// them, so mapping the origin by the full transform and each axis by its
    /// linear part maps every point *on* the plane correctly — including under
    /// a non-uniformly scaled block, where the axes simply stop being
    /// orthonormal.
    fn map_plane(&self, plane: Plane) -> Plane {
        Plane::from_axes(
            dpoint(self.to_space(vector3(plane.origin))),
            dpoint(self.direction_to_space(vector3(plane.x_axis))),
            dpoint(self.direction_to_space(vector3(plane.y_axis))),
        )
    }

    /// Carry a radial source into the dimension's space.
    ///
    /// Only the plane moves: the centre, radius and angles are coordinates
    /// *within* the plane, so they stay as they are and every point the
    /// geometry produces comes out in the dimension's space already.
    fn map_radial(&self, radial: RadialSourceGeometry) -> Option<RadialSourceGeometry> {
        let plane = self.map_plane(radial.plane);
        let x = glam::DVec3::from_array(plane.x_axis);
        let y = glam::DVec3::from_array(plane.y_axis);
        let magnitude = x.length_squared().max(y.length_squared());
        if magnitude < 1e-24
            || (x.length_squared() - y.length_squared()).abs() > magnitude * 1e-9
            || x.dot(y).abs() > magnitude * 1e-9
        {
            return None;
        }
        Some(RadialSourceGeometry { plane, ..radial })
    }
}

/// One reference resolved all the way down the chain.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedReference {
    /// The feature point in model space, through the block-instance path.
    pub model: Vector3,
    /// The same point in the dimension's own space — projected onto the sheet
    /// when the chain goes through a viewport, otherwise equal to `model`.
    pub space: Vector3,
    /// The viewport the chain looks through, if any.
    pub viewport: Option<Handle>,
}

/// Why a reference did not produce a point. The reference's stored data is
/// never modified on a failure — a dimension whose source went away keeps its
/// last valid appearance, and an undo that restores the source restores the
/// link with no extra bookkeeping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceStatus {
    /// Resolved to a live feature point.
    Resolved,
    /// A handle in the chain is no longer in the document.
    Broken(Handle),
    /// The chain is intact but the feature could not be evaluated — an osnap
    /// type we do not support, or geometry that no longer has the feature.
    Unresolved,
}

fn chain_map(scene: &Scene, walked: &ReferenceChain) -> ChainMap {
    ChainMap {
        transform: walked.transform,
        frame: walked
            .viewport
            .and_then(|handle| scene.viewport_frame(handle)),
    }
}

/// How far `point` (entity-local) is from `entity`'s geometry. Used only to
/// choose between two readings of a stored osnap point, so an approximate
/// answer for exotic entities is fine.
fn hint_error(entity: &EntityType, point: Vector3) -> f64 {
    if let Some(distance) = source_distance_squared(entity, point) {
        if distance.is_finite() {
            return distance;
        }
    }
    if let Some(planar) = crate::entities::curve::entity_curve(entity) {
        if let Some(uv) = planar.plane.project(dpoint(point)) {
            let near = closest_point(&planar.curve, uv);
            let world = planar.plane.point_at(near.point);
            return point_distance_squared(vector3(world), point);
        }
    }
    f64::INFINITY
}

/// Interpret a stored point as a source-space or dimension-space hint.
/// Validate both interpretations against the geometry before using the hint
/// to disambiguate a feature; the stored point is never the resolved result.
fn local_hint(map: &ChainMap, entity: &EntityType, stored: Vector3) -> Option<Vector3> {
    let inverse = chain::invert(&map.transform)?;
    let mut candidates = vec![inverse.apply(stored)];
    if map.frame.is_some() {
        candidates.push(inverse.apply(map.space_to_model(stored)));
    }
    candidates
        .into_iter()
        .filter(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        .min_by(|first, second| hint_error(entity, *first).total_cmp(&hint_error(entity, *second)))
}

/// Resolve one reference through its full chain.
///
/// `from_space` is the *other* definition point of the same dimension, in the
/// dimension's own space. PERPENDICULAR and TANGENT have no meaning without
/// it, so the caller resolves the independent references first and comes back
/// for these.
pub fn resolve_reference_chain(
    scene: &Scene,
    reference: &AssocDimensionReference,
    from_space: Option<Vector3>,
) -> Result<ResolvedReference, ReferenceStatus> {
    let walked = match chain::walk_chain(&scene.document, &reference.xrefs) {
        Ok(walked) => walked,
        Err(ChainError::Missing(handle)) => return Err(ReferenceStatus::Broken(handle)),
        Err(ChainError::Empty | ChainError::Invalid) => return Err(ReferenceStatus::Unresolved),
    };
    let Some(entity) = scene.document.get_entity(walked.entity) else {
        return Err(ReferenceStatus::Broken(walked.entity));
    };
    let map = chain_map(scene, &walked);
    if walked.viewport.is_some() && map.frame.is_none() {
        return Err(ReferenceStatus::Unresolved);
    }
    let inverse = chain::invert(&map.transform).ok_or(ReferenceStatus::Unresolved)?;
    if matches!(
        reference.osnap_type,
        chain::osnap::INTERSEC | chain::osnap::APPARENT_INT
    ) {
        match chain::walk_chain(&scene.document, &reference.intersection_objects) {
            Err(ChainError::Missing(handle)) => return Err(ReferenceStatus::Broken(handle)),
            Err(_) => return Err(ReferenceStatus::Unresolved),
            Ok(_) => {}
        }
    }
    let legacy = reference.main_subent_type != 2
        && matches!(
            reference.osnap_type,
            chain::osnap::NONE | chain::osnap::END | chain::osnap::START_POINT
        );
    let local = if reference.osnap_type == chain::osnap::PERP && !walked.block_path.is_empty() {
        from_space
            .and_then(|from| {
                chain::perpendicular_in_model(entity, &map.transform, map.space_to_model(from))
            })
            .map(|point| inverse.apply(point))
    } else if legacy {
        resolve_local_by_marker(entity, reference)
    } else if chain::evaluates(reference.osnap_type) {
        let hint = if reference
            .osnap_point
            .x
            .abs()
            .max(reference.osnap_point.y.abs())
            .max(reference.osnap_point.z.abs())
            < 1e40
        {
            local_hint(&map, entity, reference.osnap_point)
        } else {
            None
        };
        let from = from_space.map(|point| inverse.apply(map.space_to_model(point)));
        chain::feature_point(
            &scene.document,
            entity,
            reference,
            FeatureContext { hint, from },
        )
    } else {
        None
    }
    .ok_or(ReferenceStatus::Unresolved)?;
    if !local.x.is_finite() || !local.y.is_finite() || !local.z.is_finite() {
        return Err(ReferenceStatus::Unresolved);
    }
    let model = map.to_model(local);
    Ok(ResolvedReference {
        model,
        space: map.model_to_space(model),
        viewport: walked.viewport,
    })
}

/// Resolve a marker/parameter in entity-local coordinates.
/// The caller applies the block and viewport transforms.
fn resolve_local_by_marker(
    entity: &EntityType,
    reference: &AssocDimensionReference,
) -> Option<Vector3> {
    if reference.main_gs_marker == ARC_DIMENSION_POINT_MARKER {
        let radial = radial_source_for_marker(entity, 0)?;
        let sweep = positive_sweep(radial.start_angle, radial.end_angle);
        let angle = radial.start_angle + sweep * reference.osnap_distance.clamp(0.0, 1.0);
        return Some(radial.point_at_angle(angle));
    }
    if let Some(segment) = polyline_arc_segment_from_point_marker(reference.main_gs_marker) {
        let radial = radial_source_for_marker(entity, segment)?;
        let sweep = positive_sweep(radial.start_angle, radial.end_angle);
        let angle = radial.start_angle + sweep * reference.osnap_distance.clamp(0.0, 1.0);
        return Some(radial.point_at_angle(angle));
    }
    if reference.main_gs_marker == POLYLINE_ARC_CENTER_MARKER {
        let segment = reference.osnap_distance.round().max(0.0) as usize;
        return polyline_arc_center(entity, segment);
    }
    if reference.main_gs_marker == -3 {
        return match entity {
            EntityType::Circle(circle) => Some(circle.center_wcs()),
            EntityType::Arc(arc) => Some(arc.center_wcs()),
            _ => None,
        };
    }
    if reference.main_gs_marker == -2 {
        return match entity {
            EntityType::Circle(circle) => Some(circle.point_at_angle_wcs(reference.osnap_distance)),
            EntityType::Arc(arc) => Some(arc.point_at_angle_wcs(reference.osnap_distance)),
            _ => None,
        };
    }
    if let EntityType::Circle(circle) = entity {
        let stored = crate::scene::view::transform::wcs_point_to_ocs(
            (
                reference.osnap_point.x,
                reference.osnap_point.y,
                reference.osnap_point.z,
            ),
            (circle.normal.x, circle.normal.y, circle.normal.z),
        );
        let curve = circle_curve(circle);
        let stored_parameter = curve.parameter_at([stored.0, stored.1]) * TAU;
        let parameter = if reference.osnap_distance.abs() > 1e-12 || stored_parameter.abs() <= 1e-12
        {
            reference.osnap_distance
        } else {
            stored_parameter
        };
        let parameter = if parameter.is_finite() {
            parameter
        } else {
            0.0
        };
        let point = curve.point_at(parameter / TAU);
        return Some(ocs_point(
            point[0],
            point[1],
            circle.center.z,
            circle.normal,
        ));
    }
    source_points(entity)
        .get(reference.main_gs_marker.max(0) as usize)
        .copied()
}

fn dimension_inference_points(dimension: &Dimension) -> Vec<Option<Vector3>> {
    match dimension {
        Dimension::Linear(linear) => vec![Some(linear.first_point), Some(linear.second_point)],
        Dimension::Aligned(aligned) => vec![Some(aligned.first_point), Some(aligned.second_point)],
        Dimension::Angular3Pt(angular) => vec![
            Some(angular.angle_vertex),
            Some(angular.first_point),
            Some(angular.second_point),
        ],
        Dimension::Angular2Ln(angular) => vec![
            Some(angular.first_point),
            Some(angular.second_point),
            Some(angular.angle_vertex),
            Some(angular.definition_point),
        ],
        Dimension::Ordinate(ordinate) => vec![Some(ordinate.feature_location)],
        _ => Vec::new(),
    }
}

fn source_distance_squared(entity: &EntityType, point: Vector3) -> Option<f64> {
    if let EntityType::Circle(circle) = entity {
        let point = crate::scene::view::transform::wcs_point_to_ocs(
            (point.x, point.y, point.z),
            (circle.normal.x, circle.normal.y, circle.normal.z),
        );
        let radial_error = closest_point(&circle_curve(circle), [point.0, point.1]).distance;
        let plane_error = point.2 - circle.center.z;
        return Some(radial_error * radial_error + plane_error * plane_error);
    }
    source_points(entity)
        .into_iter()
        .map(|candidate| point_distance_squared(candidate, point))
        .min_by(f64::total_cmp)
}

fn dimension_reference_points(dimension: &Dimension) -> Vec<Vector3> {
    match dimension {
        Dimension::Linear(linear) => vec![linear.first_point, linear.second_point],
        Dimension::Aligned(aligned) => vec![aligned.first_point, aligned.second_point],
        Dimension::Angular3Pt(angular) => vec![
            angular.angle_vertex,
            angular.first_point,
            angular.second_point,
        ],
        Dimension::Angular2Ln(angular) => vec![
            angular.first_point,
            angular.second_point,
            angular.angle_vertex,
            angular.definition_point,
        ],
        Dimension::Ordinate(ordinate) => vec![ordinate.feature_location],
        Dimension::Arc(arc) => vec![
            arc.center_point,
            arc.first_extension_point,
            arc.second_extension_point,
        ],
        _ => Vec::new(),
    }
}

fn positive_sweep(start: f64, end: f64) -> f64 {
    let raw = end - start;
    let mut sweep = raw.rem_euclid(TAU);
    if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
        sweep = TAU;
    }
    sweep
}

fn signed_angle_delta(value: f64) -> f64 {
    (value + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI
}

fn angle_about_plane(plane: Plane, center: Vector3, point: Vector3) -> f64 {
    let delta = [point.x - center.x, point.y - center.y, point.z - center.z];
    let dot = |axis: [f64; 3]| delta[0] * axis[0] + delta[1] * axis[1] + delta[2] * axis[2];
    dot(plane.y_axis).atan2(dot(plane.x_axis))
}

fn point_on_radial_circle(radial: RadialSourceGeometry, radius: f64, angle: f64) -> Vector3 {
    vector3(radial.plane.point_at([
        radial.center[0] + radius * angle.cos(),
        radial.center[1] + radius * angle.sin(),
    ]))
}

fn plane_from_normal(origin: Vector3, normal: Vector3) -> Plane {
    let (x_axis, y_axis) = crate::scene::view::transform::ocs_axes((normal.x, normal.y, normal.z));
    Plane::from_axes(
        [origin.x, origin.y, origin.z],
        [x_axis.0, x_axis.1, x_axis.2],
        [y_axis.0, y_axis.1, y_axis.2],
    )
}

fn remap_plane_offset(
    old_plane: Plane,
    new_plane: Plane,
    old_origin: Vector3,
    old_point: Vector3,
    new_origin: Vector3,
) -> Vector3 {
    let old_origin = old_plane.project(dpoint(old_origin)).unwrap_or([0.0; 2]);
    let old_point = old_plane.project(dpoint(old_point)).unwrap_or(old_origin);
    let new_origin_uv = new_plane.project(dpoint(new_origin)).unwrap_or([0.0; 2]);
    vector3(new_plane.point_at([
        new_origin_uv[0] + old_point[0] - old_origin[0],
        new_origin_uv[1] + old_point[1] - old_origin[1],
    ]))
}

/// Whether `dimension` still has at least one reference whose whole chain
/// resolves to a live entity.
///
/// The chain matters: an imported trans-space reference has the VIEWPORT as
/// its first handle, so "some handle in `xrefs` exists" was true even for a
/// reference whose actual source had been erased. Walking the chain asks the
/// question that was meant — is the geometry still there.
pub(crate) fn dimension_is_associative(
    document: &acadrust::CadDocument,
    dimension: Handle,
) -> bool {
    document.objects.values().any(|object| {
        let ObjectType::Associative(object) = object else {
            return false;
        };
        let AssociativeData::DimensionAssociation(association) = &object.data else {
            return false;
        };
        association.dimension == dimension
            && association.associativity != 0
            && association
                .references
                .iter()
                .flatten()
                .any(|reference| chain::walk_chain(document, &reference.xrefs).is_ok())
    })
}

pub(crate) fn constraint_from_associative_dimension(
    document: &acadrust::CadDocument,
    handle: Handle,
) -> Option<(
    super::parametric_constraints::ConstraintKind,
    Vec<super::parametric_constraints::ParametricRef>,
    super::named_parameters::DrivingValue,
)> {
    use super::named_parameters::DrivingValue;
    use super::parametric_constraints::{ConstraintKind, ParametricRef};

    let EntityType::Dimension(dimension) = document.get_entity(handle)? else {
        return None;
    };
    let association = document.objects.values().find_map(|object| {
        let ObjectType::Associative(object) = object else {
            return None;
        };
        let AssociativeData::DimensionAssociation(association) = &object.data else {
            return None;
        };
        (association.dimension == handle && association.associativity != 0).then_some(association)
    })?;
    if association.trans_space
        || association
            .references
            .iter()
            .flatten()
            .any(|r| r.xrefs.len() != 1)
    {
        return None;
    }
    let point = |index: usize| {
        let reference = association.references.get(index)?.first()?;
        Some(ParametricRef::point(
            *reference.xrefs.first()?,
            reference.main_gs_marker,
        ))
    };
    let whole = |index: usize| {
        let reference = association.references.get(index)?.first()?;
        Some(ParametricRef::whole(*reference.xrefs.first()?))
    };
    let measurement = DrivingValue::Literal(dimension.measurement());
    match dimension {
        Dimension::Aligned(_) => Some((
            ConstraintKind::Distance,
            vec![point(0)?, point(1)?],
            measurement,
        )),
        Dimension::Linear(linear) => {
            let direction = [linear.rotation.cos().abs(), linear.rotation.sin().abs()];
            let kind = if direction[1] <= 1e-9 {
                ConstraintKind::DistanceX
            } else if direction[0] <= 1e-9 {
                ConstraintKind::DistanceY
            } else {
                return None;
            };
            Some((kind, vec![point(0)?, point(1)?], measurement))
        }
        Dimension::Radius(_) | Dimension::LargeRadial(_) => {
            Some((ConstraintKind::Radius, vec![whole(0)?], measurement))
        }
        Dimension::Diameter(_) => Some((ConstraintKind::Diameter, vec![whole(0)?], measurement)),
        Dimension::Angular2Ln(_) => {
            let mut sources: Vec<Handle> = association
                .references
                .iter()
                .flatten()
                .filter_map(|reference| reference.xrefs.first().copied())
                .collect();
            sources.sort_unstable();
            sources.dedup();
            let [first, second] = sources.as_slice() else {
                return None;
            };
            Some((
                ConstraintKind::Angle,
                vec![ParametricRef::whole(*first), ParametricRef::whole(*second)],
                measurement,
            ))
        }
        Dimension::Arc(_) | Dimension::Angular3Pt(_) | Dimension::Ordinate(_) => None,
    }
}

pub(crate) fn radial_extension_points(
    document: &acadrust::CadDocument,
    dimension: Handle,
    gap: f64,
    extension: f64,
) -> Option<Vec<Vector3>> {
    let EntityType::Dimension(dimension_entity) = document.get_entity(dimension)? else {
        return None;
    };
    let target_point = match dimension_entity {
        Dimension::Radius(radius) => radius.definition_point,
        Dimension::Diameter(diameter) => diameter.angle_vertex,
        Dimension::LargeRadial(radial) => radial.chord_point,
        _ => return None,
    };
    let association = document.objects.values().find_map(|object| {
        let ObjectType::Associative(object) = object else {
            return None;
        };
        let AssociativeData::DimensionAssociation(association) = &object.data else {
            return None;
        };
        (association.dimension == dimension).then_some(association)
    })?;
    let reference = association.references[0].first()?;
    let source = document.get_entity(*reference.xrefs.first()?)?;
    let radial = radial_source_for_marker(source, reference.main_gs_marker)?;
    if !radial.limited || radial.radius <= 1e-12 {
        return None;
    }
    let target = radial.angle_at(dpoint(target_point));
    if radial.contains_angle(target) {
        return None;
    }

    let from_start = arc_span(target, radial.start_angle);
    let from_end = arc_span(radial.end_angle, target);
    let gap_angle = gap.max(0.0) / radial.radius;
    let extension_angle = extension.max(0.0) / radial.radius;
    let span = from_start.min(from_end);
    if span <= gap_angle + 1e-10 {
        return None;
    }
    let (start, end, reverse) = if from_start <= from_end {
        (
            target - extension_angle,
            radial.start_angle - gap_angle,
            true,
        )
    } else {
        (
            radial.end_angle + gap_angle,
            target + extension_angle,
            false,
        )
    };
    let mut points: Vec<_> = tessellate_arc(
        radial.center,
        radial.radius,
        start,
        end,
        0.0,
        DEFAULT_SEGMENTS_PER_RADIAN,
    )
    .into_iter()
    .map(|point| vector3(radial.plane.point_at([point[0], point[1]])))
    .collect();
    if reverse {
        points.reverse();
    }
    Some(points)
}

impl Scene {
    pub(crate) fn attach_dimension_association(
        &mut self,
        dimension: Handle,
        sources: Vec<Option<Handle>>,
    ) {
        self.attach_dimension_association_sources(
            dimension,
            sources
                .into_iter()
                .map(|source| source.map(DimensionAssociationSource::inferred))
                .collect(),
        );
    }

    pub(crate) fn attach_dimension_association_sources(
        &mut self,
        dimension: Handle,
        sources: Vec<Option<DimensionAssociationSource>>,
    ) {
        let Some(EntityType::Dimension(dimension_entity)) = self.document.get_entity(dimension)
        else {
            return;
        };
        let radial_data = match dimension_entity {
            Dimension::Radius(radius) => Some((
                radius.angle_vertex,
                radius.measurement(),
                radius.definition_point,
            )),
            Dimension::Diameter(diameter) => Some((
                diameter.center(),
                diameter.measurement() * 0.5,
                diameter.angle_vertex,
            )),
            Dimension::LargeRadial(radial) => Some((
                radial.definition_point,
                radial.measurement(),
                radial.chord_point,
            )),
            _ => None,
        };
        if let Some((center, measured_radius, chord)) = radial_data {
            let Some(source) = sources.iter().flatten().next().copied() else {
                return;
            };
            let Some(source_entity) = self.document.get_entity(source.handle) else {
                return;
            };
            let radial = source
                .marker
                .and_then(|marker| radial_source_for_marker(source_entity, marker))
                .or_else(|| radial_source_matching(source_entity, center, measured_radius, chord));
            let Some(radial) = radial else {
                return;
            };
            let angle = if source.marker.is_some() && source.parameter.is_finite() {
                source.parameter
            } else {
                radial.angle_at(dpoint(chord))
            };
            let reference = AssocDimensionReference {
                class_name: "AcDbOsnapPointRef".to_string(),
                osnap_type: 10,
                xrefs: vec![source.handle],
                main_subent_type: 1,
                main_gs_marker: radial.marker,
                osnap_distance: angle,
                osnap_point: chord,
                ..AssocDimensionReference::default()
            };
            self.store_dimension_association(
                dimension,
                [vec![reference], Vec::new(), Vec::new(), Vec::new()],
                1,
                sources,
            );
            return;
        }
        let source_data = dimension_reference_points(dimension_entity);
        if source_data.is_empty() {
            return;
        }
        let resolved: Vec<Option<(Handle, i32, f64, u8)>> = source_data
            .iter()
            .take(4)
            .enumerate()
            .map(|(index, point)| {
                let source = sources.get(index).copied().flatten()?;
                let entity = self.document.get_entity(source.handle)?;
                let (marker, parameter) = match source.marker {
                    Some(marker) => (marker, source.parameter),
                    None => source_reference(entity, *point)?,
                };
                let osnap_type = if matches!(entity, EntityType::Circle(_)) {
                    10
                } else {
                    1
                };
                Some((source.handle, marker, parameter, osnap_type))
            })
            .collect();
        if resolved.iter().all(Option::is_none) {
            return;
        }

        let reference =
            |source: Handle, marker: i32, parameter: f64, osnap_type: u8, point: Vector3| {
                AssocDimensionReference {
                    class_name: "AcDbOsnapPointRef".to_string(),
                    osnap_type,
                    xrefs: vec![source],
                    main_subent_type: 1,
                    main_gs_marker: marker,
                    osnap_distance: parameter,
                    osnap_point: point,
                    ..AssocDimensionReference::default()
                }
            };
        let mut references: [Vec<AssocDimensionReference>; 4] = std::array::from_fn(|_| Vec::new());
        let mut associativity = 0;
        for (index, resolved) in resolved.into_iter().enumerate() {
            if index >= references.len() {
                break;
            }
            if let Some((source, marker, parameter, osnap_type)) = resolved {
                associativity |= 1 << index;
                references[index].push(reference(
                    source,
                    marker,
                    parameter,
                    osnap_type,
                    source_data[index],
                ));
            }
        }

        self.store_dimension_association(dimension, references, associativity, sources);
    }

    fn store_dimension_association(
        &mut self,
        dimension: Handle,
        references: [Vec<AssocDimensionReference>; 4],
        associativity: i32,
        sources: Vec<Option<DimensionAssociationSource>>,
    ) {
        self.store_dimension_association_full(
            dimension,
            references,
            associativity,
            false,
            sources.into_iter().flatten().map(|s| s.handle).collect(),
        );
    }

    /// Create the DIMASSOC object for `dimension` and wire its reactors.
    ///
    /// `reactor_targets` is every handle the association depends on —
    /// including the viewport and each INSERT on a block path, not just the
    /// innermost entity — so a change to any link in the chain reaches
    /// [`Scene::refresh_associative_dimensions`] through the normal
    /// dependency index.
    ///
    /// The new object and every entity whose reactor list is touched are
    /// journalled into the open undo recording, so creating an associative
    /// dimension is a single undoable step rather than something that survives
    /// its own undo.
    fn store_dimension_association_full(
        &mut self,
        dimension: Handle,
        references: [Vec<AssocDimensionReference>; 4],
        associativity: i32,
        trans_space: bool,
        reactor_targets: Vec<Handle>,
    ) {
        let association_handle = self.document.allocate_handle();
        let mut object = AssociativeObject::new("DIMASSOC", "AcDbDimAssoc");
        object.handle = association_handle;
        object.reactors.push(dimension);
        object.data = AssociativeData::DimensionAssociation(AssocDimensionAssociation {
            associativity,
            trans_space,
            dimension,
            references,
            ..AssocDimensionAssociation::default()
        });
        // A brand-new object: its before-image is "absent", so undo erases it.
        self.record_undo_object_before(association_handle, None);
        self.document
            .objects
            .insert(association_handle, ObjectType::Associative(object));

        let mut reactor_targets = reactor_targets;
        reactor_targets.push(dimension);
        reactor_targets.retain(|handle| !handle.is_null());
        reactor_targets.sort_by_key(|handle| handle.value());
        reactor_targets.dedup();
        for handle in reactor_targets {
            if self
                .document
                .get_entity(handle)
                .is_some_and(|entity| !entity.common().reactors.contains(&association_handle))
            {
                if self.is_recording_undo() {
                    let before = self.document.get_entity_arc(handle);
                    self.record_undo_before(handle, before);
                }
                if let Some(entity) = self.document.get_entity_mut(handle) {
                    entity.common_mut().reactors.push(association_handle);
                }
            }
        }
    }

    /// Persist the association for a dimension whose points were picked
    /// through [`AcceptedSnap`]s, preserving each source and its owning space.
    ///
    /// `snaps` is positional: one slot per definition point, in the same order
    /// [`dimension_reference_points`] returns them (linear/aligned: first,
    /// second; angular: see that function). A `None` slot, or a snap that
    /// landed on empty space, leaves that slot non-associative.
    ///
    /// Each reference records the whole chain — viewport, block-instance path,
    /// entity — plus the osnap type and the sub-entity marker / parameter, so
    /// the feature survives an edit to the source rather than being
    /// re-inferred by proximity.
    pub fn attach_viewport_dimension_association(
        &mut self,
        dimension: Handle,
        snaps: &[Option<AcceptedSnap>],
    ) {
        let Some(EntityType::Dimension(dimension_entity)) = self.document.get_entity(dimension)
        else {
            return;
        };
        // Radial references identify a circle/arc and angle, rather than a vertex.
        let radial_dimension = matches!(
            dimension_entity,
            Dimension::Radius(_) | Dimension::Diameter(_) | Dimension::LargeRadial(_)
        );
        if radial_dimension {
            self.attach_viewport_radial_association(dimension, snaps);
            return;
        }
        let mut references: [Vec<AssocDimensionReference>; 4] = std::array::from_fn(|_| Vec::new());
        let mut associativity = 0;
        let mut trans_space = false;
        let mut reactor_targets = Vec::new();

        for (index, snap) in snaps.iter().enumerate().take(references.len()) {
            let Some(snap) = snap else { continue };
            let Some(source) = snap.source.as_ref() else {
                continue;
            };
            let entity_handle = source.source.handle;
            let Some(entity) = self.document.get_entity(entity_handle) else {
                continue;
            };
            // dimension -> [viewport] -> [insert ...] -> entity
            let mut xrefs = Vec::new();
            if let Some(viewport) = snap.viewport {
                xrefs.push(viewport);
            }
            xrefs.extend(source.block_path.iter().copied());
            xrefs.push(entity_handle);
            let Ok(walked) = chain::walk_chain(&self.document, &xrefs) else {
                continue;
            };
            let Some(inverse) = chain::invert(&walked.transform) else {
                continue;
            };
            let local = inverse.apply(dvec3(snap.model_point));
            let osnap_type = chain::osnap_type_for(source.snap_type);
            let marker_parameter = source
                .source
                .marker
                .map(|marker| (marker, source.source.parameter))
                .or_else(|| feature_reference(entity, local, source.snap_type));
            let Some((marker, parameter)) = marker_parameter else {
                continue;
            };
            let mut intersection_objects = Vec::new();
            if matches!(
                source.snap_type,
                crate::snap::SnapType::Intersection | crate::snap::SnapType::ApparentIntersection
            ) {
                let Some(other) = source.intersection.as_ref() else {
                    continue;
                };
                intersection_objects.extend(snap.viewport);
                intersection_objects.extend(other.block_path.iter().copied());
                intersection_objects.push(other.source.handle);
                if chain::walk_chain(&self.document, &intersection_objects).is_err() {
                    continue;
                }
                reactor_targets.extend(intersection_objects.iter().copied());
            }
            trans_space |= snap.viewport.is_some();
            reactor_targets.extend(xrefs.iter().copied());
            associativity |= 1 << index;
            references[index].push(AssocDimensionReference {
                class_name: "AcDbOsnapPointRef".to_string(),
                osnap_type,
                xrefs,
                intersection_objects,
                main_subent_type: 1,
                main_gs_marker: marker,
                osnap_distance: parameter,
                // Store the source's owning-space point: model coordinates for
                // viewport features, paper coordinates for direct sheet features.
                osnap_point: dvec3(snap.model_point),
                ..AssocDimensionReference::default()
            });
        }
        if associativity == 0 {
            return;
        }
        self.store_dimension_association_full(
            dimension,
            references,
            associativity,
            trans_space,
            reactor_targets,
        );
    }

    /// The radial half of [`Scene::attach_viewport_dimension_association`].
    ///
    /// The first snap that carries a source wins: a radius / diameter
    /// dimension measures one circle or one polyline arc segment, and the
    /// command only ever acquires one.
    fn attach_viewport_radial_association(
        &mut self,
        dimension: Handle,
        snaps: &[Option<AcceptedSnap>],
    ) {
        let Some(snap) = snaps.iter().flatten().find(|snap| snap.source.is_some()) else {
            return;
        };
        let source = snap.source.as_ref().expect("filtered above");
        let entity_handle = source.source.handle;
        let Some(entity) = self.document.get_entity(entity_handle) else {
            return;
        };
        let mut xrefs = Vec::new();
        xrefs.extend(snap.viewport);
        xrefs.extend(source.block_path.iter().copied());
        xrefs.push(entity_handle);
        let Ok(walked) = chain::walk_chain(&self.document, &xrefs) else {
            return;
        };
        let Some(inverse) = chain::invert(&walked.transform) else {
            return;
        };
        let local = inverse.apply(dvec3(snap.model_point));
        let radial = match source.source.marker {
            Some(marker) => radial_source_for_marker(entity, marker),
            None => radial_source_at(entity, local),
        };
        let Some(radial) = radial else { return };
        let angle = if source.source.marker.is_some() && source.source.parameter.is_finite() {
            source.source.parameter
        } else {
            radial.angle_at(dpoint(local))
        };
        let trans_space = snap.viewport.is_some();
        let reactor_targets = xrefs.clone();
        let reference = AssocDimensionReference {
            class_name: "AcDbOsnapPointRef".to_string(),
            osnap_type: 10,
            xrefs,
            main_subent_type: 1,
            main_gs_marker: radial.marker,
            osnap_distance: angle,
            osnap_point: dvec3(snap.model_point),
            ..AssocDimensionReference::default()
        };
        self.store_dimension_association_full(
            dimension,
            [vec![reference], Vec::new(), Vec::new(), Vec::new()],
            1,
            trans_space,
            reactor_targets,
        );
    }

    /// The definition points an association fills, slot by slot, in the same
    /// order [`Scene::attach_viewport_dimension_association`] consumes its
    /// `snaps` argument.
    ///
    /// Callers use it to size and sanity-check the slot array they build from
    /// the accepted snaps: slot `k` of that array must describe the feature
    /// sitting at `dimension_association_slot_points()[k]`.
    pub(crate) fn dimension_association_slot_points(&self, dimension: Handle) -> Vec<Vector3> {
        let Some(EntityType::Dimension(entity)) = self.document.get_entity(dimension) else {
            return Vec::new();
        };
        match entity {
            // A radial dimension's single reference describes the point where
            // the leader meets the circle.
            Dimension::Radius(radius) => vec![radius.definition_point],
            Dimension::Diameter(diameter) => vec![diameter.angle_vertex],
            Dimension::LargeRadial(radial) => vec![radial.chord_point],
            other => dimension_reference_points(other),
        }
    }

    /// Select one measurement viewport while allowing direct references to
    /// geometry owned by the dimension's paper layout. A model-space chain
    /// without its viewport cannot be interpreted as a paper-space anchor.
    fn association_measurement_frame(
        &self,
        association: &AssocDimensionAssociation,
    ) -> Result<Option<ViewportFrame>, ()> {
        let mut viewport = None;
        for reference in association.references.iter().flatten() {
            for handles in [&reference.xrefs, &reference.intersection_objects] {
                let Some(root) = handles.first() else {
                    continue;
                };
                if matches!(
                    self.document.get_entity(*root),
                    Some(EntityType::Viewport(_))
                ) {
                    if viewport.is_some_and(|previous| previous != *root) {
                        return Err(());
                    }
                    viewport = Some(*root);
                }
            }
        }
        let Some(viewport) = viewport else {
            return if association.trans_space {
                Err(())
            } else {
                Ok(None)
            };
        };
        let owner = self
            .document
            .get_entity(association.dimension)
            .ok_or(())?
            .common()
            .owner_handle;
        for reference in association.references.iter().flatten() {
            for handles in [&reference.xrefs, &reference.intersection_objects] {
                let Some(root) = handles.first() else {
                    continue;
                };
                if self
                    .document
                    .get_entity(*root)
                    .ok_or(())?
                    .common()
                    .owner_handle
                    != owner
                {
                    return Err(());
                }
            }
        }
        self.viewport_frame(viewport).map(Some).ok_or(())
    }

    /// Resolve each reference's current status without changing its saved data.
    pub fn dimension_association_status(&self, dimension: Handle) -> Vec<(usize, ReferenceStatus)> {
        let Some(association) = self.dimension_association(dimension) else {
            return Vec::new();
        };
        let measurement_supported = !association.trans_space
            || self.document.get_entity(dimension).is_some_and(|entity| {
                matches!(
                    entity,
                    EntityType::Dimension(Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_))
                ) || persisted_measurement_scale(&self.document, entity).is_some()
            });
        let measurement_supported = measurement_supported
            && self.document.get_entity(dimension).is_some_and(|entity| {
                if !matches!(
                    entity,
                    EntityType::Dimension(
                        Dimension::Radius(_) | Dimension::Diameter(_) | Dimension::LargeRadial(_)
                    )
                ) {
                    return true;
                }
                association.references[0]
                    .first()
                    .and_then(|reference| {
                        let walked = chain::walk_chain(&self.document, &reference.xrefs).ok()?;
                        let source = self.document.get_entity(walked.entity)?;
                        chain_map(self, &walked)
                            .map_radial(radial_source_for_marker(source, reference.main_gs_marker)?)
                    })
                    .is_some()
            });
        let measurement_supported = measurement_supported
            && association
                .references
                .iter()
                .flatten()
                .filter(|r| chain::needs_from_point(r.osnap_type))
                .count()
                <= 1;
        let measurement_supported =
            measurement_supported && self.association_measurement_frame(association).is_ok();
        let points = self.dimension_association_slot_points(dimension);
        association
            .references
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let reference = slot.first()?;
                Some((
                    index,
                    match resolve_reference_chain(
                        self,
                        reference,
                        points
                            .iter()
                            .enumerate()
                            .find_map(|(other, point)| (other != index).then_some(*point)),
                    ) {
                        Ok(_) if measurement_supported => ReferenceStatus::Resolved,
                        Ok(_) => ReferenceStatus::Unresolved,
                        Err(status) => status,
                    },
                ))
            })
            .collect()
    }

    /// Every entity `dimension` depends on: the viewport it looks through,
    /// each INSERT on the block path, and the source geometry itself — for
    /// each reference chain, plus the second chain of an intersection.
    ///
    /// Layer visibility and viewport clipping are deliberately not consulted:
    /// hiding a layer or clipping a viewport changes what a *new* snap may
    /// acquire, never what an existing association points at.
    pub fn dimension_association_sources(&self, dimension: Handle) -> Vec<Handle> {
        let Some(association) = self.dimension_association(dimension) else {
            return Vec::new();
        };
        let mut sources = Vec::new();
        for reference in association.references.iter().flatten() {
            for xrefs in [&reference.xrefs, &reference.intersection_objects] {
                let Ok(walked) = chain::walk_chain(&self.document, xrefs) else {
                    continue;
                };
                sources.extend(walked.viewport);
                sources.extend(walked.block_path.iter().copied());
                sources.push(walked.entity);
            }
        }
        sources.sort_by_key(|handle| handle.value());
        sources.dedup();
        sources
    }

    /// The DIMASSOC payload attached to `dimension`, if there is one.
    pub fn dimension_association(&self, dimension: Handle) -> Option<&AssocDimensionAssociation> {
        self.document.objects.values().find_map(|object| {
            let ObjectType::Associative(object) = object else {
                return None;
            };
            let AssociativeData::DimensionAssociation(association) = &object.data else {
                return None;
            };
            (association.dimension == dimension).then_some(association)
        })
    }

    /// Preserve an association only when the dimension and every source-chain
    /// handle were copied. Remap the complete chain to the new entities.
    pub(crate) fn copy_dimension_associations(
        &mut self,
        handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    ) {
        if handle_map.is_empty() {
            return;
        }
        let copies: Vec<_> = self
            .document
            .objects
            .values()
            .filter_map(|object| {
                let ObjectType::Associative(object) = object else {
                    return None;
                };
                let AssociativeData::DimensionAssociation(association) = &object.data else {
                    return None;
                };
                let dimension = *handle_map.get(&association.dimension)?;
                let complete = association.references.iter().flatten().all(|reference| {
                    reference
                        .xrefs
                        .iter()
                        .chain(reference.intersection_objects.iter())
                        .all(|handle| handle.is_null() || handle_map.contains_key(handle))
                });
                if !complete {
                    return None;
                }
                let remap = |handles: &[Handle]| -> Vec<Handle> {
                    handles
                        .iter()
                        .map(|handle| handle_map.get(handle).copied().unwrap_or(*handle))
                        .collect()
                };
                let references = std::array::from_fn(|index| {
                    association.references[index]
                        .iter()
                        .map(|reference| AssocDimensionReference {
                            xrefs: remap(&reference.xrefs),
                            intersection_objects: remap(&reference.intersection_objects),
                            ..reference.clone()
                        })
                        .collect::<Vec<_>>()
                });
                Some((
                    dimension,
                    references,
                    association.associativity,
                    association.trans_space,
                ))
            })
            .collect();
        for (dimension, references, associativity, trans_space) in copies {
            let reactor_targets: Vec<Handle> = {
                let references: &[Vec<AssocDimensionReference>; 4] = &references;
                references
                    .iter()
                    .flatten()
                    .flat_map(|reference| {
                        reference
                            .xrefs
                            .iter()
                            .chain(reference.intersection_objects.iter())
                            .copied()
                    })
                    .collect()
            };
            self.store_dimension_association_full(
                dimension,
                references,
                associativity,
                trans_space,
                reactor_targets,
            );
        }
    }

    /// Refresh dimensions when their viewport moves, pans, zooms, or twists.
    pub fn notify_viewport_changed(&mut self, viewport: Handle) {
        if matches!(
            self.document.get_entity(viewport),
            Some(EntityType::Viewport(_))
        ) {
            self.bump_entities(&[(viewport, ChangeKind::Modified)]);
        }
    }

    pub(crate) fn infer_dimension_sources(&self, dimension: Handle) -> Vec<Option<Handle>> {
        let Some(EntityType::Dimension(entity)) = self.document.get_entity(dimension) else {
            return Vec::new();
        };
        let radial_data = match entity {
            Dimension::Radius(radius) => Some((
                radius.angle_vertex,
                radius.measurement(),
                radius.definition_point,
            )),
            Dimension::Diameter(diameter) => Some((
                diameter.center(),
                diameter.measurement() * 0.5,
                diameter.angle_vertex,
            )),
            Dimension::LargeRadial(radial) => Some((
                radial.definition_point,
                radial.measurement(),
                radial.chord_point,
            )),
            _ => None,
        };
        if let Some((center, radius, chord)) = radial_data {
            let tolerance = radius.abs().max(1.0) * 1e-9;
            let source = self
                .document
                .entities()
                .filter(|candidate| candidate.common().handle != dimension)
                .filter_map(|candidate| {
                    let radial = radial_source_matching(candidate, center, radius, chord)?;
                    let center_error = point_distance_squared(radial.center_world(), center);
                    let radius_error = (radial.radius - radius).powi(2);
                    (center_error + radius_error <= tolerance * tolerance)
                        .then_some((center_error + radius_error, candidate.common().handle))
                })
                .min_by(|first, second| first.0.total_cmp(&second.0))
                .map(|(_, handle)| handle);
            return vec![source];
        }
        dimension_inference_points(entity)
            .into_iter()
            .map(|point| {
                point.and_then(|point| {
                    self.document
                        .entities()
                        .filter(|entity| entity.common().handle != dimension)
                        .filter_map(|entity| {
                            source_distance_squared(entity, point)
                                .map(|distance| (distance, entity.common().handle))
                        })
                        .filter(|(distance, _)| *distance <= 1e-16)
                        .min_by(|first, second| first.0.total_cmp(&second.0))
                        .map(|(_, handle)| handle)
                })
            })
            .collect()
    }

    pub(crate) fn refresh_associative_dimensions(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) -> Vec<(Handle, ChangeKind)> {
        let changed: rustc_hash::FxHashSet<_> = changes.iter().map(|(handle, _)| *handle).collect();
        if changed.is_empty() {
            return Vec::new();
        }
        let associations: Vec<_> = self
            .document
            .objects
            .values()
            .filter_map(|object| {
                let ObjectType::Associative(object) = object else {
                    return None;
                };
                let AssociativeData::DimensionAssociation(association) = &object.data else {
                    return None;
                };
                // Any link in a chain counts: the source entity, an INSERT on
                // the block path, or the viewport the reference looks through
                // (that last one is how a pan / zoom / twist of a layout
                // viewport reaches the dimensions drawn through it).
                association
                    .references
                    .iter()
                    .flatten()
                    .any(|reference| {
                        reference
                            .xrefs
                            .iter()
                            .chain(reference.intersection_objects.iter())
                            .any(|handle| changed.contains(handle))
                    })
                    .then(|| association.clone())
            })
            .collect();

        let mut refreshed = Vec::new();
        for association in associations {
            let Ok(frame) = self.association_measurement_frame(&association) else {
                continue;
            };
            // Dependent snaps need a resolved independent definition point.
            // Two such features require a coupled solve; old points are not
            // evidence of a solution after the geometry changes.
            if association
                .references
                .iter()
                .flatten()
                .filter(|r| chain::needs_from_point(r.osnap_type))
                .count()
                > 1
            {
                continue;
            }
            // Resolve independent references first, then perpendicular/tangent.
            let mut resolved: [Option<ResolvedReference>; 4] = std::array::from_fn(|index| {
                let reference = association.references[index].first()?;
                if chain::needs_from_point(reference.osnap_type) {
                    return None;
                }
                resolve_reference_chain(self, reference, None).ok()
            });
            let previous = self
                .document
                .get_entity(association.dimension)
                .and_then(|entity| match entity {
                    EntityType::Dimension(dimension) => Some(dimension_reference_points(dimension)),
                    _ => None,
                })
                .unwrap_or_default();
            for index in 0..resolved.len() {
                let Some(reference) = association.references[index].first() else {
                    continue;
                };
                if !chain::needs_from_point(reference.osnap_type) {
                    continue;
                }
                // The other point: freshly resolved where possible, else the
                // one the dimension is currently drawn with.
                let from = (0..resolved.len())
                    .filter(|other| *other != index)
                    .find_map(|other| {
                        resolved[other]
                            .map(|value| value.space)
                            .or_else(|| previous.get(other).copied())
                    });
                resolved[index] = resolve_reference_chain(self, reference, from).ok();
            }
            if association
                .references
                .iter()
                .enumerate()
                .any(|(index, slot)| !slot.is_empty() && resolved[index].is_none())
            {
                continue;
            }
            // Every point is already in paper space: direct sheet references
            // stay there, and model references are projected individually.
            // Only the measurement factor is shared by the whole dimension.
            let map = frame
                .map(|frame| ChainMap {
                    transform: Transform::identity(),
                    frame: Some(frame),
                })
                .unwrap_or_else(ChainMap::identity);
            let model_points: [Option<Vector3>; 4] = std::array::from_fn(|index| {
                resolved[index]
                    .filter(|value| value.viewport.is_some())
                    .map(|value| value.model)
            });
            let resolved: [Option<Vector3>; 4] =
                std::array::from_fn(|index| resolved[index].map(|value| value.space));

            let radial_source = association.references[0].first().and_then(|reference| {
                let walked = chain::walk_chain(&self.document, &reference.xrefs).ok()?;
                let entity = self.document.get_entity(walked.entity)?;
                let radial = radial_source_for_marker(entity, reference.main_gs_marker)?;
                // Mapped once, here: everything the match arms below build out
                // of it then lands in the dimension's own space.
                Some((
                    chain_map(self, &walked).map_radial(radial)?,
                    reference.osnap_distance,
                ))
            });
            let arc_source = association.references[0].first().and_then(|reference| {
                let walked = chain::walk_chain(&self.document, &reference.xrefs).ok()?;
                let entity = self.document.get_entity(walked.entity)?;
                let segment = match reference.main_gs_marker {
                    -3 => 0,
                    POLYLINE_ARC_CENTER_MARKER => reference.osnap_distance.round().max(0.0) as i32,
                    _ => return None,
                };
                let radial = radial_source_for_marker(entity, segment)?;
                chain_map(self, &walked).map_radial(radial)
            });
            if radial_source.is_none() && resolved.iter().all(Option::is_none) {
                continue;
            }
            let Some(before) = self.document.get_entity(association.dimension).cloned() else {
                continue;
            };
            let angular = matches!(
                &before,
                EntityType::Dimension(Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_))
            );
            let measurement_scale = if let Some(frame) = map.frame.filter(|_| !angular) {
                let Some(mut scale) = persisted_measurement_scale(&self.document, &before) else {
                    continue;
                };
                scale.viewport_compensation = frame.paper_to_model_length_factor();
                Some(scale)
            } else {
                None
            };
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(association.dimension);
                self.record_undo_before(association.dimension, before);
            }
            if measurement_scale.is_some() {
                self.ensure_app_id(MeasurementScale::APP_ID);
            }
            let Some(EntityType::Dimension(dimension)) =
                self.document.get_entity_mut(association.dimension)
            else {
                continue;
            };
            match dimension {
                Dimension::Linear(linear) => {
                    // The sheet annotation the user placed — the dimension
                    // line's standoff and, when they dragged it, the text — is
                    // held relative to the measured points. Carry it by the
                    // centroid shift so an edit to the source keeps the
                    // drafting the user did instead of collapsing it.
                    let carry = centroid_shift(
                        [Some(linear.first_point), Some(linear.second_point)],
                        [resolved[0], resolved[1]],
                    );
                    if let Some(point) = resolved[0] {
                        linear.first_point = point;
                    }
                    if let Some(point) = resolved[1] {
                        linear.second_point = point;
                    }
                    linear.definition_point = linear.definition_point + carry;
                    linear.base.definition_point = linear.definition_point;
                    if linear.base.text_user_positioned {
                        linear.base.text_middle_point = linear.base.text_middle_point + carry;
                        linear.base.insertion_point = linear.base.insertion_point + carry;
                    }
                }
                Dimension::Aligned(aligned) => {
                    let carry = centroid_shift(
                        [Some(aligned.first_point), Some(aligned.second_point)],
                        [resolved[0], resolved[1]],
                    );
                    if let Some(point) = resolved[0] {
                        aligned.first_point = point;
                    }
                    if let Some(point) = resolved[1] {
                        aligned.second_point = point;
                    }
                    aligned.definition_point = aligned.definition_point + carry;
                    aligned.base.definition_point = aligned.definition_point;
                    if aligned.base.text_user_positioned {
                        aligned.base.text_middle_point = aligned.base.text_middle_point + carry;
                        aligned.base.insertion_point = aligned.base.insertion_point + carry;
                    }
                }
                Dimension::Angular3Pt(angular) => {
                    if let Some(point) = resolved[0] {
                        angular.angle_vertex = point;
                    }
                    if let Some(point) = resolved[1] {
                        angular.first_point = point;
                    }
                    if let Some(point) = resolved[2] {
                        angular.second_point = point;
                    }
                    angular.base.definition_point = angular.definition_point;
                }
                Dimension::Angular2Ln(angular) => {
                    if let Some(point) = resolved[0] {
                        angular.first_point = point;
                    }
                    if let Some(point) = resolved[1] {
                        angular.second_point = point;
                    }
                    if let Some(point) = resolved[2] {
                        angular.angle_vertex = point;
                    }
                    if let Some(point) = resolved[3] {
                        angular.definition_point = point;
                    }
                    angular.base.definition_point = angular.dimension_arc;
                }
                Dimension::Radius(radius) => {
                    let Some((radial, angle)) = radial_source else {
                        continue;
                    };
                    let old_chord = radius.definition_point;
                    let new_center = radial.center_world();
                    let new_chord = radial.point_at_angle(angle);
                    let delta = Vector3::new(
                        new_chord.x - old_chord.x,
                        new_chord.y - old_chord.y,
                        new_chord.z - old_chord.z,
                    );
                    radius.angle_vertex = new_center;
                    radius.definition_point = new_chord;
                    radius.base.definition_point = new_chord;
                    if radius.base.text_user_positioned {
                        radius.base.text_middle_point = radius.base.text_middle_point + delta;
                        radius.base.insertion_point = radius.base.insertion_point + delta;
                    }
                    radius.base.actual_measurement = radius.measurement();
                }
                Dimension::Diameter(diameter) => {
                    let Some((radial, angle)) = radial_source else {
                        continue;
                    };
                    let old_chord = diameter.angle_vertex;
                    let new_chord = radial.point_at_angle(angle);
                    let new_far_chord = radial.point_at_angle(angle + std::f64::consts::PI);
                    let delta = Vector3::new(
                        new_chord.x - old_chord.x,
                        new_chord.y - old_chord.y,
                        new_chord.z - old_chord.z,
                    );
                    diameter.angle_vertex = new_chord;
                    diameter.definition_point = new_far_chord;
                    diameter.base.definition_point = new_far_chord;
                    if diameter.base.text_user_positioned {
                        diameter.base.text_middle_point = diameter.base.text_middle_point + delta;
                        diameter.base.insertion_point = diameter.base.insertion_point + delta;
                    }
                    diameter.base.actual_measurement = diameter.measurement();
                }
                Dimension::LargeRadial(radial) => {
                    let Some((source, angle)) = radial_source else {
                        continue;
                    };
                    let old_center = radial.definition_point;
                    let new_center = source.center_world();
                    let new_chord = source.point_at_angle(angle);
                    let center_delta = Vector3::new(
                        new_center.x - old_center.x,
                        new_center.y - old_center.y,
                        new_center.z - old_center.z,
                    );
                    radial.definition_point = new_center;
                    radial.base.definition_point = new_center;
                    radial.chord_point = new_chord;
                    if point_distance_squared(center_delta, Vector3::default()) > 1e-18 {
                        radial.override_center = radial.override_center + center_delta;
                        radial.jog_point = radial.jog_point + center_delta;
                        if radial.base.text_user_positioned {
                            radial.base.text_middle_point =
                                radial.base.text_middle_point + center_delta;
                            radial.base.insertion_point =
                                radial.base.insertion_point + center_delta;
                        }
                    }
                    radial.base.actual_measurement = radial.measurement();
                }
                Dimension::Ordinate(ordinate) => {
                    if let Some(feature) = resolved[0] {
                        ordinate.feature_location = feature;
                    }
                    ordinate.refresh_measurement();
                }
                Dimension::Arc(arc) => {
                    let Some(radial) = arc_source else {
                        continue;
                    };
                    let center = resolved[0].unwrap_or_else(|| radial.center_world());
                    let Some(first) = resolved[1] else {
                        continue;
                    };
                    let Some(second) = resolved[2] else {
                        continue;
                    };

                    let old_center = arc.center_point;
                    let old_definition = arc.definition_point;
                    let old_plane = plane_from_normal(old_center, arc.base.normal);
                    let old_source_radius = old_center.distance(&arc.first_extension_point);
                    let old_dim_radius = old_center.distance(&old_definition);
                    let radial_offset = old_dim_radius - old_source_radius;
                    let old_mid = arc.arc_start_parameter
                        + positive_sweep(arc.arc_start_parameter, arc.arc_end_parameter) * 0.5;
                    let old_definition_angle =
                        angle_about_plane(old_plane, old_center, old_definition);
                    let definition_angle_offset =
                        signed_angle_delta(old_definition_angle - old_mid);

                    let start = radial.angle_at(dpoint(first));
                    let end_at = radial.angle_at(dpoint(second));
                    let sweep = positive_sweep(start, end_at);
                    let end = start + sweep;
                    let new_radius = center.distance(&first);
                    if !new_radius.is_finite() || new_radius <= 1.0e-12 {
                        continue;
                    }
                    let dim_radius = (new_radius + radial_offset).max(1.0e-9);
                    let middle = start + sweep * 0.5;
                    let definition = point_on_radial_circle(
                        radial,
                        dim_radius,
                        middle + definition_angle_offset,
                    );
                    let text_middle = remap_plane_offset(
                        old_plane,
                        radial.plane,
                        old_definition,
                        arc.base.text_middle_point,
                        definition,
                    );
                    let insertion = remap_plane_offset(
                        old_plane,
                        radial.plane,
                        old_definition,
                        arc.base.insertion_point,
                        definition,
                    );
                    let second_leader = remap_plane_offset(
                        old_plane,
                        radial.plane,
                        old_definition,
                        arc.second_leader_point,
                        definition,
                    );

                    arc.center_point = center;
                    arc.first_extension_point = first;
                    arc.second_extension_point = second;
                    arc.arc_start_parameter = start;
                    arc.arc_end_parameter = end;
                    arc.definition_point = definition;
                    arc.base.definition_point = definition;
                    if let Some(normal) = radial.plane.normal() {
                        arc.base.normal = vector3(normal);
                    }
                    if arc.base.text_user_positioned {
                        arc.base.text_middle_point = text_middle;
                        arc.base.insertion_point = insertion;
                    } else {
                        arc.base.text_middle_point = definition;
                        arc.base.insertion_point = definition;
                    }
                    if arc.has_leader {
                        arc.first_leader_point = point_on_radial_circle(radial, dim_radius, middle);
                        arc.second_leader_point = second_leader;
                    }
                    arc.base.actual_measurement = arc.measurement();
                }
            }
            let measurement = match &*dimension {
                Dimension::Linear(linear) => linear_measurement(linear),
                _ => dimension.measurement(),
            };
            // Store the raw sheet measurement; the formatter applies DIMLFAC once.
            debug_assert!(
                map.frame.is_none()
                    || model_points
                        .iter()
                        .zip(resolved.iter())
                        .all(|(model, space)| match (model, space, map.frame.as_ref()) {
                            (Some(model), Some(space), Some(frame)) => {
                                let projected = frame
                                    .model_to_paper(glam::DVec3::new(model.x, model.y, model.z));
                                (projected.x - space.x).abs() < 1e-6
                                    && (projected.y - space.y).abs() < 1e-6
                            }
                            _ => true,
                        }),
                "resolved paper points must be the model points through the frame",
            );
            dimension.base_mut().actual_measurement = measurement;
            if map.frame.is_some() {
                if let EntityType::Dimension(old) = &before {
                    if old.base().text_user_positioned {
                        dimension.base_mut().text_middle_point = old.base().text_middle_point;
                        dimension.base_mut().insertion_point = old.base().insertion_point;
                    }
                }
            }
            let entity = self
                .document
                .get_entity_mut(association.dimension)
                .expect("dimension exists");
            if let Some(scale) = measurement_scale {
                scale.write_to_entity(entity);
            }
            if *entity == before {
                continue;
            }
            // Saved anonymous blocks bypass generated dimension rendering.
            // Invalidate only after a completely resolved, committed update;
            // the undo snapshot above retains the previous picture and data.
            if let EntityType::Dimension(dimension) = entity {
                dimension.base_mut().block_name.clear();
            }
            refreshed.push((association.dimension, ChangeKind::Modified));
        }
        refreshed
    }
    pub(crate) fn sync_diameter_association_angle(&mut self, dimension: Handle) {
        let chord = match self.document.get_entity(dimension) {
            Some(EntityType::Dimension(Dimension::Diameter(diameter))) => {
                diameter.angle_vertex
            }
            _ => return,
        };

        let association_handle = self.document.objects.iter().find_map(|(handle, object)| {
            let ObjectType::Associative(object) = object else {
                return None;
            };

            let AssociativeData::DimensionAssociation(association) = &object.data else {
                return None;
            };

            (association.dimension == dimension).then_some(*handle)
        });

        let Some(association_handle) = association_handle else {
            return;
        };

        // Resolve the radial source while the association is borrowed immutably.
        let angle = {
            let Some(ObjectType::Associative(object)) =
                self.document.objects.get(&association_handle)
            else {
                return;
            };

            let AssociativeData::DimensionAssociation(association) = &object.data else {
                return;
            };

            let Some(reference) = association.references[0].first() else {
                return;
            };

            let Ok(walked) = chain::walk_chain(&self.document, &reference.xrefs) else {
                return;
            };

            let Some(entity) = self.document.get_entity(walked.entity) else {
                return;
            };

            let Some(radial) = radial_source_for_marker(entity, reference.main_gs_marker) else {
                return;
            };

            let Some(radial) = chain_map(self, &walked).map_radial(radial) else {
                return;
            };

            radial.angle_at(dpoint(chord))
        };

        if self.is_recording_undo() {
            let before = self.document.objects.get(&association_handle).cloned();
            self.record_undo_object_before(association_handle, before);
        }

        let Some(ObjectType::Associative(object)) =
            self.document.objects.get_mut(&association_handle)
        else {
            return;
        };

        let AssociativeData::DimensionAssociation(association) = &mut object.data else {
            return;
        };

        let Some(reference) = association.references[0].first_mut() else {
            return;
        };

        reference.osnap_distance = angle;
        reference.osnap_point = chord;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{Circle, DimensionDiameter};

    #[test]
    fn diameter_angle_sync_records_the_association_for_undo() {
        let mut scene = Scene::new();
        let circle = scene.add_entity(EntityType::Circle(Circle::from_center_radius(
            Vector3::ZERO,
            5.0,
        )));
        let dimension = scene.add_entity(EntityType::Dimension(Dimension::Diameter(
            DimensionDiameter::new(
                Vector3::new(5.0, 0.0, 0.0),
                Vector3::new(-5.0, 0.0, 0.0),
            ),
        )));
        scene.attach_dimension_association(dimension, vec![Some(circle)]);
        let association = scene
            .document
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                ObjectType::Associative(object)
                    if matches!(
                        object.data,
                        AssociativeData::DimensionAssociation(ref association)
                            if association.dimension == dimension
                    ) => Some(*handle),
                _ => None,
            })
            .expect("diameter association");

        scene.begin_undo_recording();
        let Some(EntityType::Dimension(Dimension::Diameter(diameter))) =
            scene.document.get_entity_mut(dimension)
        else {
            panic!("diameter dimension");
        };
        diameter.angle_vertex = Vector3::new(0.0, 5.0, 0.0);
        scene.sync_diameter_association_angle(dimension);

        let recording = scene.take_undo_recording().expect("undo recording");
        let (_, objects, _, _) = recording.into_recorded_images();
        assert!(objects
            .iter()
            .any(|(handle, before)| *handle == association && before.is_some()));
    }
}

/// Average displacement of the dimension's resolved definition points.
fn centroid_shift(before: [Option<Vector3>; 2], after: [Option<Vector3>; 2]) -> Vector3 {
    let mut delta = Vector3::new(0.0, 0.0, 0.0);
    let mut count = 0.0;
    for index in 0..2 {
        if let (Some(old), Some(new)) = (before[index], after[index]) {
            delta = delta + (new - old);
            count += 1.0;
        }
    }
    if count == 0.0 {
        return Vector3::new(0.0, 0.0, 0.0);
    }
    Vector3::new(delta.x / count, delta.y / count, delta.z / count)
}

fn linear_measurement(linear: &acadrust::entities::DimensionLinear) -> f64 {
    let plane = plane_from_normal(linear.first_point, linear.base.normal);
    let first = plane
        .project(dpoint(linear.first_point))
        .unwrap_or([0.0; 2]);
    let second = plane.project(dpoint(linear.second_point)).unwrap_or(first);
    let axis = [linear.rotation.cos(), linear.rotation.sin()];
    ((second[0] - first[0]) * axis[0] + (second[1] - first[1]) * axis[1]).abs()
}

/// Encode the feature actually acquired, in source-local coordinates.
fn feature_reference(
    entity: &EntityType,
    point: Vector3,
    kind: crate::snap::SnapType,
) -> Option<(i32, f64)> {
    use crate::snap::SnapType as S;
    match kind {
        S::Node | S::Insertion => Some((0, 0.0)),
        S::Center if matches!(entity, EntityType::Ellipse(_)) => Some((-3, 0.0)),
        S::Endpoint | S::ObjectPick | S::Center => {
            if kind != S::Center {
                if let Some(index) = source_points(entity)
                    .iter()
                    .position(|candidate| point_distance_squared(*candidate, point) < 1e-12)
                {
                    return Some((index as i32, 0.0));
                }
            }
            if matches!(entity, EntityType::Circle(_) | EntityType::Arc(_)) {
                return source_reference(entity, point);
            }
            let planar = crate::entities::curve::entity_curve(entity)?;
            if let KernelCurve::Polyline(polyline) = &planar.curve {
                for segment in 0..polyline.vertices.len() {
                    if let Some(arc) = polyline.segment_arc(segment) {
                        let center = vector3(planar.plane.point_at(arc.center));
                        if point_distance_squared(center, point) < 1e-12 {
                            return Some((POLYLINE_ARC_CENTER_MARKER, segment as f64));
                        }
                    }
                }
            }
            None
        }
        S::Nearest | S::Extension => {
            if matches!(entity, EntityType::Circle(_) | EntityType::Arc(_)) {
                return source_reference(entity, point);
            }
            let planar = crate::entities::curve::entity_curve(entity)?;
            let near = closest_point(&planar.curve, planar.plane.project(dpoint(point))?);
            Some((0, near.t))
        }
        S::Midpoint => {
            let planar = crate::entities::curve::entity_curve(entity)?;
            if let KernelCurve::Polyline(_) = &planar.curve {
                let uv = planar.plane.project(dpoint(point))?;
                let segments = planar.curve.segments();
                let (index, _) = segments.iter().enumerate().min_by(|(_, a), (_, b)| {
                    let error = |c: &KernelCurve| {
                        let p = closest_point(c, uv).point;
                        (p[0] - uv[0]).powi(2) + (p[1] - uv[1]).powi(2)
                    };
                    error(a).total_cmp(&error(b))
                })?;
                Some((index as i32, 0.5))
            } else {
                Some((0, 0.5))
            }
        }
        S::Tangent => {
            if let EntityType::Spline(spline) = entity {
                return Some((
                    0,
                    crate::entities::spline::nurbs3(spline)?.parameter_at(dpoint(point)),
                ));
            }
            source_reference(entity, point)
        }
        S::Intersection | S::ApparentIntersection => Some((0, 0.0)),
        _ => source_reference(entity, point),
    }
}

fn persisted_measurement_scale(
    doc: &acadrust::CadDocument,
    entity: &EntityType,
) -> Option<MeasurementScale> {
    use crate::entities::dim_override;
    use acadrust::xdata::XDataValue;
    let EntityType::Dimension(dimension) = entity else {
        return None;
    };
    let data = &dimension.base().common.extended_data;
    let effective = dim_override::real(data, dim_override::DIMLFAC)
        .or_else(|| {
            doc.dim_styles
                .get(&dimension.base().style_name)
                .map(|s| s.dimlfac)
        })?
        .abs();
    let effective = if effective == 0.0 { 1.0 } else { effective };
    if !effective.is_finite() {
        return None;
    }
    if let Some(mut scale) = MeasurementScale::read(data) {
        // A Properties edit writes the persisted total DIMLFAC. Preserve that
        // new user choice when the next source/viewport update recomposes it.
        scale.user_lfac = effective / scale.viewport_compensation;
        return Some(scale);
    }
    let calculated = data
        .get_record("ACAD_DIMASSOC_CALC_DIMLFAC")?
        .values
        .iter()
        .find_map(|v| match v {
            XDataValue::Real(v) => Some(v.abs()),
            _ => None,
        })?;
    if !calculated.is_finite() || calculated <= 0.0 {
        return None;
    }
    Some(MeasurementScale {
        user_lfac: effective / calculated,
        viewport_compensation: calculated,
    })
}
