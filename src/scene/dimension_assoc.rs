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
use std::f64::consts::TAU;

use super::{ChangeKind, Scene};

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

fn dimension_points(dimension: &Dimension) -> Option<[Option<Vector3>; 2]> {
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
        let Some(source_data) = dimension_points(entity) else {
            return;
        };
        let resolved: [Option<(Handle, i32, f64, u8)>; 2] = std::array::from_fn(|index| {
            let point = source_data[index]?;
            let source = sources[index]?;
            let entity = self.document.get_entity(source)?;
            source_marker(entity, point).map(|marker| {
                (
                    source,
                    marker,
                    source_parameter(entity, point),
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
                    source_data[index].expect("resolved dimension point"),
                ));
            }
        }

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
        let Some(points) = dimension_points(entity) else {
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
            let first = association.references[0]
                .first()
                .and_then(|reference| resolve_reference(self, reference));
            let second = association.references[1]
                .first()
                .and_then(|reference| resolve_reference(self, reference));
            if first.is_none() && second.is_none() {
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
                Dimension::Ordinate(ordinate) => {
                    if let Some(feature) = first {
                        ordinate.feature_location = feature;
                    }
                    ordinate.refresh_measurement();
                }
                _ => continue,
            }
            refreshed.push((association.dimension, ChangeKind::Modified));
        }
        refreshed
    }
}
