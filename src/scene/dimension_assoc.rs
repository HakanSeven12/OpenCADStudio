use acadrust::entities::Dimension;
use acadrust::objects::{
    AssocDimensionAssociation, AssocDimensionReference, AssociativeData,
    AssociativeObject, ObjectType,
};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use cadkernel::geom2d::{
    angle_within_arc, arc as tessellate_arc, arc_span, closest_point, Arc as KernelArc,
    BulgeArc, Circle as KernelCircle, Curve as KernelCurve, DEFAULT_SEGMENTS_PER_RADIAN,
};
use cadkernel::space::Plane;
use std::f64::consts::TAU;

use crate::command::DimensionAssociationSource;

use super::{ChangeKind, Scene};

pub(crate) const POLYLINE_ARC_CENTER_MARKER: i32 = -4;
const POLYLINE_ARC_POINT_MARKER_BASE: i32 = -5;

pub(crate) fn polyline_arc_point_marker(segment: i32) -> i32 {
    POLYLINE_ARC_POINT_MARKER_BASE - segment.max(0)
}

fn polyline_arc_segment_from_point_marker(marker: i32) -> Option<i32> {
    (marker <= POLYLINE_ARC_POINT_MARKER_BASE)
        .then_some(POLYLINE_ARC_POINT_MARKER_BASE - marker)
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
    radial_candidates(entity).into_iter().min_by(|first, second| {
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
    radial_candidates(entity).into_iter().min_by(|first, second| {
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
            let second = *polyline
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
            let second = polyline
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

fn source_points(entity: &EntityType) -> Vec<Vector3> {
    match entity {
        EntityType::Line(line) => vec![line.start, line.end],
        EntityType::Arc(arc) => vec![arc.start_point_wcs(), arc.end_point_wcs()],
        EntityType::Circle(_) => Vec::new(),
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
            point_distance_squared(*first, point)
                .total_cmp(&point_distance_squared(*second, point))
        })
        .map(|(index, _)| index as i32)
}

fn resolve_reference(scene: &Scene, reference: &AssocDimensionReference) -> Option<Vector3> {
    let source = *reference.xrefs.first()?;
    let entity = scene.document.get_entity(source)?;
    if let Some(segment) =
        polyline_arc_segment_from_point_marker(reference.main_gs_marker)
    {
        let radial = radial_source_for_marker(entity, segment)?;
        return Some(radial.point_at_angle(reference.osnap_distance));
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
            EntityType::Circle(circle) => {
                Some(circle.point_at_angle_wcs(reference.osnap_distance))
            }
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
        let parameter = if reference.osnap_distance.abs() > 1e-12
            || stored_parameter.abs() <= 1e-12
        {
            reference.osnap_distance
        } else {
            stored_parameter
        };
        let parameter = if parameter.is_finite() { parameter } else { 0.0 };
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

fn dimension_inference_points(dimension: &Dimension) -> Option<[Option<Vector3>; 2]> {
    match dimension {
        Dimension::Linear(linear) => Some([Some(linear.first_point), Some(linear.second_point)]),
        Dimension::Aligned(aligned) => Some([Some(aligned.first_point), Some(aligned.second_point)]),
        Dimension::Ordinate(ordinate) => Some([Some(ordinate.feature_location), None]),
        _ => None,
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

fn angle_about_plane(
    plane: Plane,
    center: Vector3,
    point: Vector3,
) -> f64 {
    let delta = [
        point.x - center.x,
        point.y - center.y,
        point.z - center.z,
    ];
    let dot = |axis: [f64; 3]| {
        delta[0] * axis[0] + delta[1] * axis[1] + delta[2] * axis[2]
    };
    dot(plane.y_axis).atan2(dot(plane.x_axis))
}

fn point_on_radial_circle(
    radial: RadialSourceGeometry,
    radius: f64,
    angle: f64,
) -> Vector3 {
    vector3(radial.plane.point_at([
        radial.center[0] + radius * angle.cos(),
        radial.center[1] + radius * angle.sin(),
    ]))
}

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
            && association.references.iter().flatten().any(|reference| {
                reference
                    .xrefs
                    .iter()
                    .any(|source| document.get_entity(*source).is_some())
            })
    })
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
        let Some(EntityType::Dimension(dimension_entity)) =
            self.document.get_entity(dimension)
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

        let reference = |source: Handle,
                         marker: i32,
                         parameter: f64,
                         osnap_type: u8,
                         point: Vector3| AssocDimensionReference {
            class_name: "AcDbOsnapPointRef".to_string(),
            osnap_type,
            xrefs: vec![source],
            main_subent_type: 1,
            main_gs_marker: marker,
            osnap_distance: parameter,
            osnap_point: point,
            ..AssocDimensionReference::default()
        };
        let mut references: [Vec<AssocDimensionReference>; 4] =
            std::array::from_fn(|_| Vec::new());
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
        let association_handle = self.document.allocate_handle();
        let mut object = AssociativeObject::new("DIMASSOC", "AcDbDimAssoc");
        object.handle = association_handle;
        object.reactors.push(dimension);
        object.data = AssociativeData::DimensionAssociation(AssocDimensionAssociation {
            associativity,
            dimension,
            references,
            ..AssocDimensionAssociation::default()
        });
        self.document
            .objects
            .insert(association_handle, ObjectType::Associative(object));

        let mut reactor_targets = vec![dimension];
        reactor_targets.extend(sources.into_iter().flatten().map(|source| source.handle));
        reactor_targets.sort_by_key(|handle| handle.value());
        reactor_targets.dedup();
        for handle in reactor_targets {
            if let Some(entity) = self.document.get_entity_mut(handle) {
                if !entity.common().reactors.contains(&association_handle) {
                    entity.common_mut().reactors.push(association_handle);
                }
            }
        }
    }

    pub(crate) fn infer_dimension_sources(
        &self,
        dimension: Handle,
    ) -> [Option<Handle>; 2] {
        let Some(EntityType::Dimension(entity)) = self.document.get_entity(dimension)
        else {
            return [None, None];
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
            _ => None,
        };
        if let Some((center, radius, chord)) = radial_data {
            let tolerance = radius.abs().max(1.0) * 1e-9;
            let source = self
                .document
                .entities()
                .filter(|candidate| candidate.common().handle != dimension)
                .filter_map(|candidate| {
                    let radial = radial_source_matching(
                        candidate,
                        center,
                        radius,
                        chord,
                    )?;
                    let center_error = point_distance_squared(radial.center_world(), center);
                    let radius_error = (radial.radius - radius).powi(2);
                    (center_error + radius_error <= tolerance * tolerance).then_some((
                        center_error + radius_error,
                        candidate.common().handle,
                    ))
                })
                .min_by(|first, second| first.0.total_cmp(&second.0))
                .map(|(_, handle)| handle);
            return [source, None];
        }
        let Some(points) = dimension_inference_points(entity) else {
            return [None, None];
        };
        points.map(|point| {
            point.and_then(|point| self.document
                .entities()
                .filter(|entity| entity.common().handle != dimension)
                .filter_map(|entity| {
                    source_distance_squared(entity, point)
                        .map(|distance| (distance, entity.common().handle))
                })
                .filter(|(distance, _)| *distance <= 1e-16)
                .min_by(|first, second| first.0.total_cmp(&second.0))
                .map(|(_, handle)| handle))
        })
    }

    pub(crate) fn refresh_associative_dimensions(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) -> Vec<(Handle, ChangeKind)> {
        let changed: rustc_hash::FxHashSet<_> =
            changes.iter().map(|(handle, _)| *handle).collect();
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
                association
                    .references
                    .iter()
                    .flatten()
                    .any(|reference| reference.xrefs.iter().any(|handle| changed.contains(handle)))
                    .then_some(association.clone())
            })
            .collect();

        let mut refreshed = Vec::new();
        for association in associations {
            let resolved: [Option<Vector3>; 4] = std::array::from_fn(|index| {
                association.references[index]
                    .first()
                    .and_then(|reference| resolve_reference(self, reference))
            });
            let radial_source = association.references[0].first().and_then(|reference| {
                let source = *reference.xrefs.first()?;
                let entity = self.document.get_entity(source)?;
                let radial = radial_source_for_marker(entity, reference.main_gs_marker)?;
                Some((radial, reference.osnap_distance))
            });
            let arc_source = association.references[0].first().and_then(|reference| {
                let source = *reference.xrefs.first()?;
                let entity = self.document.get_entity(source)?;
                let segment = match reference.main_gs_marker {
                    -3 => 0,
                    POLYLINE_ARC_CENTER_MARKER => {
                        reference.osnap_distance.round().max(0.0) as i32
                    }
                    _ => return None,
                };
                radial_source_for_marker(entity, segment)
            });
            if radial_source.is_none() && resolved.iter().all(Option::is_none) {
                continue;
            }
            let Some(EntityType::Dimension(dimension)) =
                self.document.get_entity_mut(association.dimension)
            else {
                continue;
            };
            match dimension {
                Dimension::Linear(linear) => {
                    if let Some(point) = resolved[0] {
                        linear.first_point = point;
                    }
                    if let Some(point) = resolved[1] {
                        linear.second_point = point;
                    }
                    linear.base.definition_point = linear.definition_point;
                }
                Dimension::Aligned(aligned) => {
                    if let Some(point) = resolved[0] {
                        aligned.first_point = point;
                    }
                    if let Some(point) = resolved[1] {
                        aligned.second_point = point;
                    }
                    aligned.base.definition_point = aligned.definition_point;
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
                    angular.base.definition_point = angular.definition_point;
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
                    let old_source_radius = old_center.distance(&arc.first_extension_point);
                    let old_dim_radius = old_center.distance(&old_definition);
                    let radial_offset = old_dim_radius - old_source_radius;
                    let old_mid = arc.arc_start_parameter
                        + positive_sweep(
                            arc.arc_start_parameter,
                            arc.arc_end_parameter,
                        ) * 0.5;
                    let old_definition_angle =
                        angle_about_plane(radial.plane, old_center, old_definition);
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
                    let delta = definition - old_definition;

                    arc.center_point = center;
                    arc.first_extension_point = first;
                    arc.second_extension_point = second;
                    arc.arc_start_parameter = start;
                    arc.arc_end_parameter = end;
                    arc.definition_point = definition;
                    arc.base.definition_point = definition;
                    if arc.base.text_user_positioned {
                        arc.base.text_middle_point = arc.base.text_middle_point + delta;
                        arc.base.insertion_point = arc.base.insertion_point + delta;
                    } else {
                        arc.base.text_middle_point = definition;
                        arc.base.insertion_point = definition;
                    }
                    if arc.has_leader {
                        arc.first_leader_point =
                            point_on_radial_circle(radial, dim_radius, middle);
                        arc.second_leader_point = arc.second_leader_point + delta;
                    }
                    arc.base.actual_measurement = arc.measurement();
                }
                _ => continue,
            }
            dimension.base_mut().actual_measurement = dimension.measurement();
            refreshed.push((association.dimension, ChangeKind::Modified));
        }
        refreshed
    }
}
