use acadrust::entities::Dimension;
use acadrust::objects::{
    AssocDimensionAssociation, AssocDimensionReference, AssociativeData,
    AssociativeObject, ObjectType,
};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use cadkernel::geom2d::{
    closest_point, Circle as KernelCircle, Curve as KernelCurve,
};
use cadkernel::space::Plane;
use std::f64::consts::TAU;

use super::{ChangeKind, Scene};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RadialSourceGeometry {
    pub plane: Plane,
    pub center: [f64; 2],
    pub radius: f64,
    pub start_angle: f64,
    pub sweep: f64,
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
        if !self.limited {
            return true;
        }
        signed_progress(self.start_angle, angle, self.sweep)
            .is_some_and(|progress| progress <= self.sweep.abs() + 1e-10)
    }

    fn distance_squared_to(self, point: DPoint) -> f64 {
        let Some(projected) = self.plane.project(point) else {
            return f64::INFINITY;
        };
        let angle = (projected[1] - self.center[1]).atan2(projected[0] - self.center[0]);
        let radial = ((projected[0] - self.center[0]).hypot(projected[1] - self.center[1])
            - self.radius)
            .abs();
        let candidate = if self.contains_angle(angle) {
            [
                self.center[0] + self.radius * angle.cos(),
                self.center[1] + self.radius * angle.sin(),
            ]
        } else {
            let start = [
                self.center[0] + self.radius * self.start_angle.cos(),
                self.center[1] + self.radius * self.start_angle.sin(),
            ];
            let end_angle = self.start_angle + self.sweep;
            let end = [
                self.center[0] + self.radius * end_angle.cos(),
                self.center[1] + self.radius * end_angle.sin(),
            ];
            if squared_2d(start, projected) <= squared_2d(end, projected) {
                start
            } else {
                end
            }
        };
        let planar = if self.contains_angle(angle) {
            radial * radial
        } else {
            squared_2d(candidate, projected)
        };
        let world = self.plane.point_at(projected);
        let dx = world[0] - point[0];
        let dy = world[1] - point[1];
        let dz = world[2] - point[2];
        planar + dx * dx + dy * dy + dz * dz
    }
}

type DPoint = [f64; 3];

fn vector3(point: DPoint) -> Vector3 {
    Vector3::new(point[0], point[1], point[2])
}

fn dpoint(point: Vector3) -> DPoint {
    [point.x, point.y, point.z]
}

fn squared_2d(first: [f64; 2], second: [f64; 2]) -> f64 {
    let dx = first[0] - second[0];
    let dy = first[1] - second[1];
    dx * dx + dy * dy
}

fn signed_progress(start: f64, angle: f64, sweep: f64) -> Option<f64> {
    if !start.is_finite() || !angle.is_finite() || !sweep.is_finite() || sweep.abs() <= 1e-12 {
        return None;
    }
    Some(if sweep > 0.0 {
        (angle - start).rem_euclid(TAU)
    } else {
        (start - angle).rem_euclid(TAU)
    })
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
            sweep: TAU,
            limited: false,
            marker: 0,
        }],
        KernelCurve::Arc(arc) => vec![RadialSourceGeometry {
            plane: planar.plane,
            center: arc.centre,
            radius: arc.radius,
            start_angle: arc.start_angle,
            sweep: arc.sweep(),
            limited: true,
            marker: 0,
        }],
        KernelCurve::Polyline(polyline) => (0..polyline.vertices.len())
            .filter_map(|index| {
                let arc = polyline.segment_arc(index)?;
                Some(RadialSourceGeometry {
                    plane: planar.plane,
                    center: arc.center,
                    radius: arc.radius,
                    start_angle: arc.start_angle,
                    sweep: arc.sweep,
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

fn source_points(entity: &EntityType) -> Vec<Vector3> {
    match entity {
        EntityType::Line(line) => vec![line.start, line.end],
        EntityType::Arc(arc) => vec![arc.start_point_wcs(), arc.end_point_wcs()],
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

fn source_marker(entity: &EntityType, point: Vector3) -> Option<i32> {
    if matches!(entity, EntityType::Circle(_)) {
        return Some(0);
    }
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

fn dimension_points(dimension: &Dimension) -> Option<[Vector3; 2]> {
    match dimension {
        Dimension::Linear(linear) => Some([linear.first_point, linear.second_point]),
        Dimension::Aligned(aligned) => Some([aligned.first_point, aligned.second_point]),
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

fn source_parameter(entity: &EntityType, point: Vector3) -> f64 {
    let EntityType::Circle(circle) = entity else {
        return 0.0;
    };
    let point = crate::scene::view::transform::wcs_point_to_ocs(
        (point.x, point.y, point.z),
        (circle.normal.x, circle.normal.y, circle.normal.z),
    );
    circle_curve(circle).parameter_at([point.0, point.1]) * TAU
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
    let EntityType::Dimension(Dimension::Diameter(diameter)) = document.get_entity(dimension)?
    else {
        return None;
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
    let target = radial.angle_at(dpoint(diameter.angle_vertex));
    if radial.contains_angle(target) {
        return None;
    }

    let end = radial.start_angle + radial.sweep;
    let direction = radial.sweep.signum();
    let from_start = if direction > 0.0 {
        (radial.start_angle - target).rem_euclid(TAU)
    } else {
        (target - radial.start_angle).rem_euclid(TAU)
    };
    let from_end = if direction > 0.0 {
        (target - end).rem_euclid(TAU)
    } else {
        (end - target).rem_euclid(TAU)
    };
    let (origin, extension_direction, span) = if from_start <= from_end {
        (radial.start_angle, -direction, from_start)
    } else {
        (end, direction, from_end)
    };
    let gap_angle = gap.max(0.0) / radial.radius;
    if span <= gap_angle + 1e-10 {
        return None;
    }
    let extension_angle = extension.max(0.0) / radial.radius;
    let visible_span = span - gap_angle + extension_angle;
    let first_angle = origin + extension_direction * gap_angle;
    let segments = (visible_span / (5.0_f64.to_radians())).ceil().max(1.0) as usize;
    Some(
        (0..=segments)
            .map(|index| {
                let fraction = index as f64 / segments as f64;
                radial.point_at_angle(first_angle + extension_direction * visible_span * fraction)
            })
            .collect(),
    )
}

impl Scene {
    pub(crate) fn attach_dimension_association(
        &mut self,
        dimension: Handle,
        sources: [Option<Handle>; 2],
    ) {
        let Some(EntityType::Dimension(entity)) = self.document.get_entity(dimension)
        else {
            return;
        };
        if let Dimension::Diameter(diameter) = entity {
            let Some(source) = sources.into_iter().flatten().next() else {
                return;
            };
            let Some(source_entity) = self.document.get_entity(source) else {
                return;
            };
            let center = diameter.center();
            let radius = diameter.measurement() * 0.5;
            let Some(radial) = radial_source_matching(
                source_entity,
                center,
                radius,
                diameter.angle_vertex,
            ) else {
                return;
            };
            let angle = radial.angle_at(dpoint(diameter.angle_vertex));
            let reference = AssocDimensionReference {
                class_name: "AcDbOsnapPointRef".to_string(),
                osnap_type: 10,
                xrefs: vec![source],
                main_subent_type: 1,
                main_gs_marker: radial.marker,
                osnap_distance: angle,
                osnap_point: diameter.angle_vertex,
                ..AssocDimensionReference::default()
            };
            self.store_dimension_association(
                dimension,
                [vec![reference], Vec::new(), Vec::new(), Vec::new()],
                1,
                [Some(source), None],
            );
            return;
        }
        let Some(source_data) = dimension_points(entity) else {
            return;
        };
        let resolved: [Option<(Handle, i32, f64, u8)>; 2] = std::array::from_fn(|index| {
            let source = sources[index]?;
            let entity = self.document.get_entity(source)?;
            source_marker(entity, source_data[index]).map(|marker| {
                (
                    source,
                    marker,
                    source_parameter(entity, source_data[index]),
                    if matches!(entity, EntityType::Circle(_)) {
                        10
                    } else {
                        1
                    },
                )
            })
        });
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
        sources: [Option<Handle>; 2],
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
        reactor_targets.extend(sources.into_iter().flatten());
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
        if let Dimension::Diameter(diameter) = entity {
            let measurement = diameter.measurement();
            let radius = measurement * 0.5;
            let center = diameter.center();
            let tolerance = measurement.abs().max(1.0) * 1e-9;
            let source = self
                .document
                .entities()
                .filter(|candidate| candidate.common().handle != dimension)
                .filter_map(|candidate| {
                    let radial = radial_source_matching(
                        candidate,
                        center,
                        radius,
                        diameter.angle_vertex,
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
        let Some(points) = dimension_points(entity) else {
            return [None, None];
        };
        points.map(|point| {
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
            let radial_source = association.references[0].first().and_then(|reference| {
                let source = *reference.xrefs.first()?;
                let entity = self.document.get_entity(source)?;
                let radial = radial_source_for_marker(entity, reference.main_gs_marker)?;
                Some((radial, reference.osnap_distance))
            });
            let first = association.references[0]
                .first()
                .and_then(|reference| resolve_reference(self, reference));
            let second = association.references[1]
                .first()
                .and_then(|reference| resolve_reference(self, reference));
            if first.is_none() && second.is_none() && radial_source.is_none() {
                continue;
            }
            let Some(EntityType::Dimension(dimension)) =
                self.document.get_entity_mut(association.dimension)
            else {
                continue;
            };
            match dimension {
                Dimension::Linear(linear) => {
                    if let Some(first) = first {
                        linear.first_point = first;
                    }
                    if let Some(second) = second {
                        linear.second_point = second;
                    }
                    linear.base.actual_measurement = linear.measurement();
                    linear.base.definition_point = linear.definition_point;
                }
                Dimension::Aligned(aligned) => {
                    if let Some(first) = first {
                        aligned.first_point = first;
                    }
                    if let Some(second) = second {
                        aligned.second_point = second;
                    }
                    aligned.base.actual_measurement = aligned.measurement();
                    aligned.base.definition_point = aligned.definition_point;
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
                    diameter.base.text_middle_point = diameter.base.text_middle_point + delta;
                    diameter.base.insertion_point = diameter.base.insertion_point + delta;
                    diameter.base.actual_measurement = diameter.measurement();
                }
                _ => continue,
            }
            refreshed.push((association.dimension, ChangeKind::Modified));
        }
        refreshed
    }
}
