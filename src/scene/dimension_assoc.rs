use acadrust::entities::Dimension;
use acadrust::objects::{
    AssocDimensionAssociation, AssocDimensionReference, AssociativeData,
    AssociativeObject, ObjectType,
};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;

use crate::command::DimensionAssociationSource;

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

fn bulge_center_world(
    first: [f64; 2],
    second: [f64; 2],
    bulge: f64,
    elevation: f64,
    normal: Vector3,
) -> Option<Vector3> {
    if bulge.abs() <= 1.0e-12 {
        return None;
    }
    let dx = second[0] - first[0];
    let dy = second[1] - first[1];
    let chord = dx.hypot(dy);
    if chord <= 1.0e-12 {
        return None;
    }
    let midpoint = [(first[0] + second[0]) * 0.5, (first[1] + second[1]) * 0.5];
    let offset = chord * (1.0 - bulge * bulge) / (4.0 * bulge);
    Some(ocs_point(
        midpoint[0] - dy / chord * offset,
        midpoint[1] + dx / chord * offset,
        elevation,
        normal,
    ))
}

fn polyline_arc_center(entity: &EntityType, segment: usize) -> Option<Vector3> {
    match entity {
        EntityType::LwPolyline(polyline) => {
            let count = polyline.vertices.len();
            let first = *polyline.vertices.get(segment)?;
            let second = *polyline.vertices.get((segment + 1) % count)?;
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
            let second = polyline.vertices.get((segment + 1) % count)?;
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
    if reference.main_gs_marker == -4 {
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
    source_points(entity)
        .get(reference.main_gs_marker.max(0) as usize)
        .copied()
}

fn dimension_reference_points(dimension: &Dimension) -> Vec<Vector3> {
    match dimension {
        Dimension::Linear(linear) => vec![linear.first_point, linear.second_point],
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
        _ => Vec::new(),
    }
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
        let source_data = dimension_reference_points(dimension_entity);
        if source_data.is_empty() {
            return;
        }
        let resolved: Vec<Option<(Handle, i32, f64)>> = source_data
            .iter()
            .enumerate()
            .map(|(index, point)| {
            let source = sources.get(index).copied().flatten()?;
            let (marker, parameter) = match source.marker {
                Some(marker) => (marker, source.parameter),
                None => {
                    let entity = self.document.get_entity(source.handle)?;
                    source_reference(entity, *point)?
                }
            };
            Some((source.handle, marker, parameter))
        })
            .collect();
        if resolved.iter().all(Option::is_none) {
            return;
        }

        let reference = |source: Handle, marker: i32, parameter: f64, point: Vector3| AssocDimensionReference {
            class_name: "AcDbOsnapPointRef".to_string(),
            osnap_type: 1,
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
            if let Some((source, marker, parameter)) = resolved {
                associativity |= 1 << index;
                references[index].push(reference(source, marker, parameter, source_data[index]));
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

    pub(crate) fn attach_linear_dimension_association(
        &mut self,
        dimension: Handle,
        sources: [Option<Handle>; 2],
    ) {
        self.attach_dimension_association(dimension, sources.into_iter().collect());
    }

    pub(crate) fn infer_linear_dimension_sources(
        &self,
        dimension: Handle,
    ) -> [Option<Handle>; 2] {
        let Some(EntityType::Dimension(Dimension::Linear(linear))) =
            self.document.get_entity(dimension)
        else {
            return [None, None];
        };
        [linear.first_point, linear.second_point].map(|point| {
            self.document
                .entities()
                .filter(|entity| entity.common().handle != dimension)
                .filter_map(|entity| {
                    source_points(entity)
                        .into_iter()
                        .map(|candidate| point_distance_squared(candidate, point))
                        .min_by(f64::total_cmp)
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
            let resolved: [Option<Vector3>; 4] = std::array::from_fn(|index| {
                association.references[index]
                    .first()
                    .and_then(|reference| resolve_reference(self, reference))
            });
            if resolved.iter().all(Option::is_none) {
                continue;
            }
            if let Some(EntityType::Dimension(dimension)) =
                self.document.get_entity_mut(association.dimension)
            {
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
                    _ => continue,
                }
                dimension.base_mut().actual_measurement = dimension.measurement();
                refreshed.push((association.dimension, ChangeKind::Modified));
            }
        }
        refreshed
    }
}
