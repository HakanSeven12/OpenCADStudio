// Kernel B-rep solid modelling and exact ACIS persistence.

use acadrust::{
    entities::{
        AcisData, EntityCommon, Region, Solid3D, Surface, SurfaceData, SurfaceKind, Wire,
    },
    objects::SolidHistoryOperation,
    EntityType, Handle,
};
use cadkernel::brep::{Body, EdgeKey, FaceKey};
use iced::Task;
use std::collections::{HashMap, HashSet};

use super::Message;
use crate::modules::model::boolean_cmd::BoolOp;
use crate::scene::model::solid_history;
use crate::scene::model::solid_model::{self, Bool};

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnionEntityKind {
    Solid,
    Region,
    Surface,
}

impl UnionEntityKind {
    fn from_entity(entity: &EntityType) -> Option<Self> {
        Some(match entity {
            EntityType::Solid3D(_) => Self::Solid,
            EntityType::Region(_) => Self::Region,
            EntityType::Surface(_) => Self::Surface,
            _ => return None,
        })
    }
}

fn subtract_entity_kind(entity: &EntityType, convert_meshes: bool) -> Option<UnionEntityKind> {
    UnionEntityKind::from_entity(entity).or_else(|| {
        (convert_meshes
            && matches!(
                entity,
                EntityType::Mesh(_) | EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_)
            ))
        .then_some(UnionEntityKind::Solid)
    })
}

struct UnionGroup {
    kind: UnionEntityKind,
    handles: Vec<Handle>,
}

struct PreparedUnion {
    kind: UnionEntityKind,
    handles: Vec<Handle>,
    body: Body,
    common: EntityCommon,
}

struct SubtractGroup {
    kind: UnionEntityKind,
    plane: Option<cadkernel::space::Plane>,
    bases: Vec<Handle>,
    cutters: Vec<Handle>,
}

type PreparedSolidDisplay = (
    crate::scene::model::mesh_model::MeshLodSet,
    Vec<Wire>,
    [f64; 3],
);

struct PreparedSubtract {
    bases: Vec<Handle>,
    cutters: Vec<Handle>,
    result: Option<(Handle, EntityType, Body, PreparedSolidDisplay)>,
}

struct PreparedSlice {
    retained: Handle,
    entity: EntityType,
    body: Body,
    display: PreparedSolidDisplay,
    extra: Option<(EntityType, Body)>,
}

enum ModelSliceTool {
    Plane(cadkernel::space::Plane),
    Surface(Body),
}

impl ModelSliceTool {
    fn side(&self, point: [f64; 3]) -> Option<f64> {
        match self {
            Self::Plane(plane) => plane.distance_to(point),
            Self::Surface(surface) => cadkernel::brep::surface_side(surface, point),
        }
    }

    fn split(&self, body: &Body) -> Result<Option<cadkernel::brep::PlaneSlice>, cadkernel::brep::Snag> {
        match self {
            Self::Plane(plane) => cadkernel::brep::slice_by_plane(body, *plane),
            Self::Surface(surface) => cadkernel::brep::slice_by_surface(body, surface),
        }
    }
}

struct IntersectGroup {
    kind: UnionEntityKind,
    plane: Option<cadkernel::space::Plane>,
    handles: Vec<Handle>,
}

enum PreparedIntersectOutcome {
    Replace {
        retained: Handle,
        entity: EntityType,
        body: Body,
        display: PreparedSolidDisplay,
    },
    Consume,
}

struct PreparedIntersect {
    handles: Vec<Handle>,
    outcome: PreparedIntersectOutcome,
}

fn inherited_common(source: &EntityCommon) -> EntityCommon {
    let mut result = EntityCommon::new();
    result.layer = source.layer.clone();
    result.color = source.color;
    result.line_weight = source.line_weight;
    result.linetype = source.linetype.clone();
    result.linetype_handle = source.linetype_handle;
    result.linetype_scale = source.linetype_scale;
    result.transparency = source.transparency;
    result.color_name = source.color_name.clone();
    result.invisible = source.invisible;
    result.color_book_handle = source.color_book_handle;
    result.full_visual_style_handle = source.full_visual_style_handle;
    result.face_visual_style_handle = source.face_visual_style_handle;
    result.edge_visual_style_handle = source.edge_visual_style_handle;
    result.material_flags = source.material_flags;
    result.material_handle = source.material_handle;
    result.shadow_flags = source.shadow_flags;
    result.plotstyle_flags = source.plotstyle_flags;
    result.plotstyle_handle = source.plotstyle_handle;
    result
}

fn union_bodies(
    kind: UnionEntityKind,
    bodies: Vec<Body>,
) -> Result<Body, cadkernel::brep::Snag> {
    if kind == UnionEntityKind::Solid {
        let mut operands = bodies.into_iter();
        let mut result = operands
            .next()
            .ok_or(cadkernel::brep::Snag::CutRefused)?;
        for operand in operands {
            result = solid_model::boolean_result(Bool::Union, &result, &operand)?;
        }
        return Ok(result);
    }
    let references = bodies.iter().collect::<Vec<_>>();
    let tolerance = cadkernel::brep::operation_tolerance(&references);
    cadkernel::brep::union_planar_regions(&bodies, tolerance)
}

fn planar_body_plane(body: &Body) -> Option<cadkernel::space::Plane> {
    let mut faces = body.face_keys();
    let first = cadkernel::brep::planar_face_profile(body, faces.next()?)?.plane;
    let first_normal = glam::DVec3::from_array(first.normal()?);
    let tolerance = cadkernel::brep::operation_tolerance(&[body]);
    faces
        .all(|face| {
            let Some(profile) = cadkernel::brep::planar_face_profile(body, face) else {
                return false;
            };
            let Some(normal) = profile.plane.normal() else {
                return false;
            };
            first_normal.dot(glam::DVec3::from_array(normal)).abs() >= 1.0 - 1e-9
                && first
                    .distance_to(profile.plane.origin)
                    .is_some_and(|distance| distance.abs() <= tolerance * 4.0)
        })
        .then_some(first)
}

fn coplanar_bodies(plane: cadkernel::space::Plane, body: &Body) -> bool {
    let Some(other) = planar_body_plane(body) else {
        return false;
    };
    let Some(one_normal) = plane.normal().map(glam::DVec3::from_array) else {
        return false;
    };
    let Some(other_normal) = other.normal().map(glam::DVec3::from_array) else {
        return false;
    };
    let tolerance = cadkernel::brep::operation_tolerance(&[body]);
    one_normal.dot(other_normal).abs() >= 1.0 - 1e-9
        && plane
            .distance_to(other.origin)
            .is_some_and(|distance| distance.abs() <= tolerance * 4.0)
}

fn subtract_bodies(
    kind: UnionEntityKind,
    plane: Option<cadkernel::space::Plane>,
    bases: Vec<Body>,
    cutters: Vec<Body>,
) -> Result<Body, cadkernel::brep::Snag> {
    if kind != UnionEntityKind::Solid && plane.is_none() {
        return Err(cadkernel::brep::Snag::NoClosedForm);
    }
    if kind != UnionEntityKind::Solid && plane.is_some() {
        let references = bases.iter().chain(&cutters).collect::<Vec<_>>();
        let tolerance = cadkernel::brep::operation_tolerance(&references);
        return cadkernel::brep::subtract_planar_regions(&bases, &cutters, tolerance);
    }

    let mut bases = bases.into_iter();
    let mut result = bases
        .next()
        .ok_or(cadkernel::brep::Snag::CutRefused)?;
    for base in bases {
        result = solid_model::boolean_result(Bool::Union, &result, &base)?;
    }
    for cutter in cutters {
        result = solid_model::boolean_result(Bool::Subtract, &result, &cutter)?;
    }
    Ok(result)
}

fn entity_with_boolean_body(mut source: EntityType, body: &Body) -> Option<EntityType> {
    let document = crate::scene::convert::acis_export::solid_to_sat(body)?;
    let wires = solid_model::edge_wires(body);
    if matches!(
        &source,
        EntityType::Mesh(_) | EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_)
    ) {
        let mut solid = Solid3D::new();
        solid.common = source.common().clone();
        source = EntityType::Solid3D(solid);
    }
    match &mut source {
        EntityType::Solid3D(entity) => {
            entity.wires = wires;
            entity.silhouettes.clear();
            entity.history_handle = None;
            entity.set_sat_document(&document);
        }
        EntityType::Region(entity) => {
            entity.wires = wires;
            entity.silhouettes.clear();
            entity.history_handle = None;
            entity.set_sat_document(&document);
        }
        EntityType::Surface(entity) => {
            entity.wires = wires;
            entity.silhouettes.clear();
            entity.history_handle = None;
            if entity.kind != SurfaceKind::Plane {
                entity.kind = SurfaceKind::Generic;
                entity.surface_data = SurfaceData::Generic;
            }
            entity.acis_data = AcisData::from_sat(&document.to_sat_string());
        }
        _ => return None,
    }
    Some(source)
}

fn entity_with_history_body(mut source: EntityType, body: &Body) -> Option<EntityType> {
    let document = crate::scene::convert::acis_export::solid_to_sat(body)?;
    let wires = solid_model::edge_wires(body);
    match &mut source {
        EntityType::Solid3D(entity) => {
            entity.wires = wires;
            entity.silhouettes.clear();
            entity.set_sat_document(&document);
        }
        EntityType::Surface(entity) => {
            entity.wires = wires;
            entity.silhouettes.clear();
            if entity.kind != SurfaceKind::Plane {
                entity.kind = SurfaceKind::Generic;
                entity.surface_data = SurfaceData::Generic;
            }
            entity.acis_data = AcisData::from_sat(&document.to_sat_string());
        }
        _ => return None,
    }
    Some(source)
}

enum IntersectBodyOutcome {
    Area(Body),
    Touching,
    Disjoint,
}

fn intersect_bodies(
    kind: UnionEntityKind,
    bodies: Vec<Body>,
) -> Result<IntersectBodyOutcome, cadkernel::brep::Snag> {
    if kind == UnionEntityKind::Solid {
        let mut operands = bodies.into_iter();
        let mut result = operands
            .next()
            .ok_or(cadkernel::brep::Snag::CutRefused)?;
        for operand in operands {
            result = solid_model::boolean_result(Bool::Intersect, &result, &operand)?;
            if result.faces.is_empty() {
                return Ok(IntersectBodyOutcome::Disjoint);
            }
        }
        return Ok(IntersectBodyOutcome::Area(result));
    }

    let references = bodies.iter().collect::<Vec<_>>();
    let tolerance = cadkernel::brep::operation_tolerance(&references);
    Ok(match cadkernel::brep::intersect_planar_regions(&bodies, tolerance)? {
        cadkernel::brep::PlanarIntersection::Area(body) => IntersectBodyOutcome::Area(body),
        cadkernel::brep::PlanarIntersection::Touching => IntersectBodyOutcome::Touching,
        cadkernel::brep::PlanarIntersection::Disjoint => IntersectBodyOutcome::Disjoint,
    })
}

impl super::OpenCADStudio {
    /// Add a solid and register its persistent B-rep.
    pub(super) fn add_solid_model(
        &mut self,
        entity: EntityType,
        solid: Body,
        history: SolidHistoryOperation,
    ) -> Handle {
        self.add_solid_model_inner(entity, solid, history, true)
    }

    fn add_solid_model_preserving_style(
        &mut self,
        entity: EntityType,
        solid: Body,
        history: SolidHistoryOperation,
    ) -> Handle {
        self.add_solid_model_inner(entity, solid, history, false)
    }

    fn add_solid_model_inner(
        &mut self,
        mut entity: EntityType,
        solid: Body,
        history: SolidHistoryOperation,
        apply_creation_style: bool,
    ) -> Handle {
        let i = self.active_tab;
        let EntityType::Solid3D(inner) = &mut entity else {
            return Handle::NULL;
        };
        if apply_creation_style {
            inner.common.plotstyle_flags = 2;
        }
        inner.wires = solid_model::edge_wires(&solid);
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&solid)
        else {
            self.command_line
                .push_error(crate::t!("The solid could not be encoded as ACIS.").as_ref());
            return Handle::NULL;
        };
        inner.set_sat_document(&document);
        let handle = if apply_creation_style {
            self.commit_entity_handle(entity)
        } else {
            self.commit_entity_handle_preserve_style(entity)
        };
        let Some(handle) = handle else {
            return Handle::NULL;
        };
        let require_complete = matches!(history, SolidHistoryOperation::Loft(_));
        self.tabs[i].scene.create_solid_history(handle, history);
        if !self.tabs[i].scene.register_solid_model(handle, solid)
            || (require_complete && self.tabs[i].scene.meshes.get(&handle).is_none_or(|mesh| !mesh.complete)) {
            // A command result is not successful until it has renderable
            // geometry. Roll the new entity back so DELOBJ cannot consume a
            // visible source in exchange for an invisible result.
            self.tabs[i].scene.rollback_new_entities(&[handle]);
            return Handle::NULL;
        }
        handle
    }

    /// Add an open sheet body as a persistent Surface entity and register its
    /// exact B-rep for shaded and wireframe display.
    pub(super) fn add_surface_model(
        &mut self,
        entity: EntityType,
        surface: Body,
    ) -> Handle {
        self.add_surface_model_inner(entity, surface, true)
    }

    fn add_surface_model_preserving_style(
        &mut self,
        entity: EntityType,
        surface: Body,
    ) -> Handle {
        self.add_surface_model_inner(entity, surface, false)
    }

    fn add_surface_model_inner(
        &mut self,
        mut entity: EntityType,
        surface: Body,
        apply_creation_style: bool,
    ) -> Handle {
        let i = self.active_tab;
        let EntityType::Surface(inner) = &mut entity else {
            return Handle::NULL;
        };
        if apply_creation_style {
            inner.common.plotstyle_flags = 2;
        }
        inner.wires = solid_model::edge_wires(&surface);
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&surface)
        else {
            self.command_line
                .push_error(crate::t!("The surface could not be encoded as ACIS.").as_ref());
            return Handle::NULL;
        };
        inner.acis_data = acadrust::entities::AcisData::from_sat(&document.to_sat_string());
        let handle = if apply_creation_style {
            self.commit_entity_handle(entity)
        } else {
            self.commit_entity_handle_preserve_style(entity)
        };
        let Some(handle) = handle else {
            return Handle::NULL;
        };
        let require_complete = self.tabs[i].scene.document.get_entity(handle).is_some_and(|entity|
            matches!(entity, EntityType::Surface(value) if value.kind == acadrust::entities::SurfaceKind::Lofted));
        if !self.tabs[i].scene.register_solid_model(handle, surface)
            || (require_complete && self.tabs[i].scene.meshes.get(&handle).is_none_or(|mesh| !mesh.complete)) {
            self.tabs[i].scene.rollback_new_entities(&[handle]);
            return Handle::NULL;
        }
        handle
    }

    pub(super) fn add_region_model(&mut self, region: Region, body: Body) -> Handle {
        self.add_region_model_inner(region, body, false)
    }

    fn add_region_model_preserving_style(&mut self, region: Region, body: Body) -> Handle {
        self.add_region_model_inner(region, body, true)
    }

    fn add_region_model_inner(
        &mut self,
        mut region: Region,
        body: Body,
        preserve_style: bool,
    ) -> Handle {
        let i = self.active_tab;
        region.wires = solid_model::edge_wires(&body);
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&body) else {
            self.command_line
                .push_error(crate::t!("The region could not be encoded as ACIS.").as_ref());
            return Handle::NULL;
        };
        region.set_sat_document(&document);
        let entity = EntityType::Region(region);
        let handle = if preserve_style {
            self.commit_entity_handle_preserve_style(entity)
        } else {
            self.commit_entity_handle(entity)
        };
        let Some(handle) = handle else {
            return Handle::NULL;
        };
        if !self.tabs[i].scene.register_solid_model(handle, body) {
            self.tabs[i].scene.rollback_new_entities(&[handle]);
            return Handle::NULL;
        }
        handle
    }

    fn selected_solid_handles(&mut self) -> Vec<Handle> {
        let i = self.active_tab;
        let mut handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected_handles_in_order()
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .filter(|handle| {
                matches!(
                    self.tabs[i].scene.document.get_entity(*handle),
                    Some(EntityType::Solid3D(_))
                )
            })
            .collect();
        self.tabs[i].scene.restore_solid_models(&handles);
        handles.retain(|handle| self.tabs[i].scene.solid_models.contains_key(handle));
        handles
    }

    fn selected_union_groups(&self) -> Vec<UnionGroup> {
        let scene = &self.tabs[self.active_tab].scene;
        let mut groups: Vec<UnionGroup> = Vec::new();
        for handle in scene.selected_handles_in_order() {
            if scene.is_layer_locked(handle) {
                continue;
            }
            let Some(kind) = scene
                .document
                .get_entity(handle)
                .and_then(UnionEntityKind::from_entity)
            else {
                continue;
            };
            if let Some(group) = groups.iter_mut().find(|group| group.kind == kind) {
                group.handles.push(handle);
            } else {
                groups.push(UnionGroup {
                    kind,
                    handles: vec![handle],
                });
            }
        }
        groups
    }

    pub(super) fn union_ready(&self) -> bool {
        self.selected_union_groups()
            .iter()
            .any(|group| group.handles.len() >= 2)
    }

    pub(super) fn intersect_ready(&self) -> bool {
        self.union_ready()
    }

    fn selected_intersect_handles(&self) -> Vec<Handle> {
        let scene = &self.tabs[self.active_tab].scene;
        scene
            .selected_handles_in_order()
            .into_iter()
            .filter(|handle| !scene.is_layer_locked(*handle))
            .filter(|handle| {
                scene
                    .document
                    .get_entity(*handle)
                    .and_then(UnionEntityKind::from_entity)
                    .is_some()
            })
            .collect()
    }

    fn intersect_groups(
        &self,
        handles: &[Handle],
        bodies: &HashMap<Handle, Body>,
    ) -> Vec<IntersectGroup> {
        let scene = &self.tabs[self.active_tab].scene;
        let mut groups = Vec::<IntersectGroup>::new();
        for handle in handles {
            let Some(kind) = scene
                .document
                .get_entity(*handle)
                .and_then(UnionEntityKind::from_entity)
            else {
                continue;
            };
            let plane = (kind != UnionEntityKind::Solid)
                .then(|| planar_body_plane(&bodies[handle]))
                .flatten();
            if let Some(group) = groups.iter_mut().find(|group| {
                group.kind == kind
                    && if kind == UnionEntityKind::Solid {
                        true
                    } else {
                        match (group.plane, plane) {
                            (Some(group_plane), Some(_)) => {
                                coplanar_bodies(group_plane, &bodies[handle])
                            }
                            (None, None) => true,
                            _ => false,
                        }
                    }
            }) {
                group.handles.push(*handle);
            } else {
                groups.push(IntersectGroup {
                    kind,
                    plane,
                    handles: vec![*handle],
                });
            }
        }
        groups.retain(|group| group.handles.len() >= 2);
        groups
    }

    fn replace_solid_body_with_fillet(
        &mut self,
        handle: Handle,
        source: &Body,
        result: Body,
        edges: &[EdgeKey],
        radius: f64,
    ) -> bool {
        let i = self.active_tab;
        let Some(display) = self.tabs[i]
            .scene
            .prepare_solid_model_display(handle, &result)
            .filter(|display| display.0.complete)
        else {
            self.command_line.push_error(crate::t!("The result could not be displayed completely. The original edge body was retained.").as_ref());
            return false;
        };
        let ordered = source.edge_keys().collect::<Vec<_>>();
        let Some(history_edges) = edges
            .iter()
            .map(|edge| {
                ordered
                    .iter()
                    .position(|candidate| candidate == edge)
                    .and_then(|ordinal| i32::try_from(ordinal).ok())
            })
            .collect::<Option<Vec<_>>>()
        else {
            self.command_line
                .push_error(crate::t!("A selected edge no longer belongs to the body.").as_ref());
            return false;
        };

        self.push_undo_snapshot(i, "FILLETEDGE");
        if self.tabs[i].scene.document.solid_history_graph(handle).is_none()
            && !self.tabs[i]
                .scene
                .create_solid_history(handle, solid_history::brep_op(source))
        {
            self.command_line
                .push_error(crate::t!("The edge body history could not be created.").as_ref());
            return false;
        }
        if !self.tabs[i].scene.append_solid_history(
            handle,
            solid_history::fillet_op(history_edges, radius),
        ) {
            self.command_line
                .push_error(crate::t!("The fillet history could not be recorded.").as_ref());
            return false;
        }
        let _ = self.tabs[i].scene.apply_solid_history_choice(
            handle,
            solid_history::PROP_HISTORY,
            "Record",
        );
        let Some(entity) = self.tabs[i].scene.document.get_entity(handle).cloned() else {
            return false;
        };
        let Some(entity) = entity_with_history_body(entity, &result) else {
            return false;
        };
        if !self.tabs[i].scene.update_entity(entity) {
            self.command_line
                .push_error(crate::t!("The edge body could not be updated.").as_ref());
            return false;
        }
        self.tabs[i]
            .scene
            .register_prepared_solid_model(handle, result, display);
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.select_entity(handle, false);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        true
    }

    fn replace_edge_body_with_chamfer(
        &mut self,
        handle: Handle,
        source: &Body,
        result: Body,
        edges: &[EdgeKey],
        base_face: FaceKey,
        base_distance: f64,
        other_distance: f64,
    ) -> bool {
        let i = self.active_tab;
        let Some(display) = self.tabs[i]
            .scene
            .prepare_solid_model_display(handle, &result)
            .filter(|display| display.0.complete)
        else {
            self.command_line.push_error(crate::t!("The result could not be displayed completely. The original edge body was retained.").as_ref());
            return false;
        };
        let ordered_edges = source.edge_keys().collect::<Vec<_>>();
        let Some(history_edges) = edges
            .iter()
            .map(|edge| {
                ordered_edges
                    .iter()
                    .position(|candidate| candidate == edge)
                    .and_then(|ordinal| i32::try_from(ordinal).ok())
            })
            .collect::<Option<Vec<_>>>()
        else {
            self.command_line
                .push_error(crate::t!("A selected edge no longer belongs to the body.").as_ref());
            return false;
        };
        let Some(history_face) = source
            .face_keys()
            .position(|candidate| candidate == base_face)
            .and_then(|ordinal| i32::try_from(ordinal).ok())
        else {
            self.command_line
                .push_error(crate::t!("The selected base face no longer belongs to the body.").as_ref());
            return false;
        };

        self.push_undo_snapshot(i, "CHAMFEREDGE");
        if self.tabs[i].scene.document.solid_history_graph(handle).is_none()
            && !self.tabs[i]
                .scene
                .create_solid_history(handle, solid_history::brep_op(source))
        {
            self.command_line
                .push_error(crate::t!("The edge body history could not be created.").as_ref());
            return false;
        }
        if !self.tabs[i].scene.append_solid_history(
            handle,
            solid_history::chamfer_op(
                history_edges,
                history_face,
                base_distance,
                other_distance,
            ),
        ) {
            self.command_line
                .push_error(crate::t!("The chamfer history could not be recorded.").as_ref());
            return false;
        }
        let _ = self.tabs[i].scene.apply_solid_history_choice(
            handle,
            solid_history::PROP_HISTORY,
            "Record",
        );
        let Some(entity) = self.tabs[i].scene.document.get_entity(handle).cloned() else {
            return false;
        };
        let Some(entity) = entity_with_history_body(entity, &result) else {
            return false;
        };
        if !self.tabs[i].scene.update_entity(entity) {
            self.command_line
                .push_error(crate::t!("The edge body could not be updated.").as_ref());
            return false;
        }
        self.tabs[i]
            .scene
            .register_prepared_solid_model(handle, result, display);
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.select_entity(handle, false);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        true
    }

    pub(super) fn solid_edge_blend(
        &mut self,
        handle: Handle,
        edges: &[EdgeKey],
        base_face: Option<FaceKey>,
        value: f64,
        other_value: f64,
        fillet: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Task::none();
        }
        if !matches!(self.tabs[i].scene.document.get_entity(handle), Some(
            EntityType::Solid3D(_) | EntityType::Surface(_)
        )) {
            self.command_line
                .push_error(crate::t!("Select a 3D solid or surface edge.").as_ref());
            return Task::none();
        }
        self.tabs[i].scene.restore_solid_models(&[handle]);
        let Some(body) = self.tabs[i].scene.solid_models.get(&handle).cloned() else {
            self.command_line
                .push_error(crate::t!("The solid geometry could not be restored.").as_ref());
            return Task::none();
        };
        let result = if fillet {
            match cadkernel::brep::fillet_edges(&body, edges, value) {
                Ok(result) => Some(result),
                Err(error) => {
                    self.command_line
                        .push_error(&format!("FILLETEDGE: {error}"));
                    return Task::none();
                }
            }
        } else {
            let Some(base_face) = base_face else {
                self.command_line
                    .push_error(crate::t!("CHAMFEREDGE: select a base face edge.").as_ref());
                return Task::none();
            };
            match cadkernel::brep::chamfer_edges(
                &body,
                edges,
                base_face,
                value,
                other_value,
            ) {
                Ok(result) => Some(result),
                Err(error) => {
                    self.command_line
                        .push_error(&format!("CHAMFEREDGE: {error}"));
                    return Task::none();
                }
            }
        };
        let Some(result) = result else {
            self.command_line.push_error(
                crate::t!("Edge operation failed. Use a convex planar solid and a smaller value.")
                    .as_ref(),
            );
            return Task::none();
        };
        let label = if fillet { "FILLETEDGE" } else { "CHAMFEREDGE" };
        let updated = if fillet {
            self.replace_solid_body_with_fillet(handle, &body, result, edges, value)
        } else {
            self.replace_edge_body_with_chamfer(
                handle,
                &body,
                result,
                edges,
                base_face.expect("checked chamfer face"),
                value,
                other_value,
            )
        };
        if updated {
            if !fillet {
                self.tabs[i].scene.document.header.chamfer_distance_a = value;
                self.tabs[i].scene.document.header.chamfer_distance_b = other_value;
            }
            self.command_line
                .push_output(crate::tf!("{label}: solid updated.").as_ref());
        }
        Task::none()
    }

    pub(super) fn solid_shell(
        &mut self,
        handle: Handle,
        actions: &[crate::command::ShellFaceAction],
        distance: f64,
    ) -> Task<Message> {
        use crate::command::ShellFaceAction;

        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Task::none();
        }
        if !matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(EntityType::Solid3D(_))
        ) {
            self.command_line
                .push_error(crate::t!("Select a 3D solid.").as_ref());
            return Task::none();
        }
        if !distance.is_finite() || distance.abs() <= f64::EPSILON {
            self.command_line
                .push_error(crate::t!("The shell offset distance must be nonzero.").as_ref());
            return Task::none();
        }

        self.tabs[i].scene.restore_solid_models(&[handle]);
        let Some(body) = self.tabs[i].scene.solid_models.get(&handle).cloned() else {
            self.command_line
                .push_error(crate::t!("The solid geometry could not be restored.").as_ref());
            return Task::none();
        };

        let all_faces = body.face_keys().collect::<Vec<_>>();
        let mut removed = HashSet::new();
        for action in actions {
            match *action {
                ShellFaceAction::Remove(point) => {
                    let Some(face) = cadkernel::brep::shell_face_at_point(&body, point.to_array())
                    else {
                        self.command_line.push_error(
                            crate::t!("A selected face could not be resolved.").as_ref(),
                        );
                        return Task::none();
                    };
                    removed.insert(face);
                }
                ShellFaceAction::Add(point) => {
                    let Some(face) = cadkernel::brep::shell_face_at_point(&body, point.to_array())
                    else {
                        self.command_line.push_error(
                            crate::t!("A selected face could not be resolved.").as_ref(),
                        );
                        return Task::none();
                    };
                    removed.remove(&face);
                }
                ShellFaceAction::RemoveAll => removed.extend(all_faces.iter().copied()),
                ShellFaceAction::AddAll => removed.clear(),
            }
        }
        let removed = removed.into_iter().collect::<Vec<_>>();

        let result = match cadkernel::brep::shell(&body, &removed, distance) {
            Ok(result) => result,
            Err(cadkernel::brep::ShellError::InvalidDistance) => {
                self.command_line
                    .push_error(crate::t!("The shell offset distance must be nonzero.").as_ref());
                return Task::none();
            }
            Err(cadkernel::brep::ShellError::UnsupportedSolid) => {
                self.command_line.push_error(
                    crate::t!("SHELL supports rectangular boxes, circular cylinders, and spheres.")
                        .as_ref(),
                );
                return Task::none();
            }
            Err(cadkernel::brep::ShellError::UnknownFace) => {
                self.command_line
                    .push_error(crate::t!("A selected face does not belong to the solid.").as_ref());
                return Task::none();
            }
            Err(cadkernel::brep::ShellError::NoMaterial) => {
                self.command_line.push_error(
                    crate::t!("The offset removes all material. Use a smaller distance or add a face back.")
                        .as_ref(),
                );
                return Task::none();
            }
            Err(cadkernel::brep::ShellError::Kernel(_)) => {
                self.command_line.push_error(
                    crate::t!("The shell could not be constructed from the selected solid and distance.")
                        .as_ref(),
                );
                return Task::none();
            }
        };

        let Some(source) = self.tabs[i].scene.document.get_entity(handle).cloned() else {
            return Task::none();
        };
        let Some(entity) = entity_with_boolean_body(source, &result) else {
            self.command_line
                .push_error(crate::t!("The result could not be encoded as ACIS.").as_ref());
            return Task::none();
        };
        let Some(display) = self.tabs[i]
            .scene
            .prepare_solid_model_display(handle, &result)
            .filter(|display| display.0.complete)
        else {
            self.command_line.push_error(crate::t!("The result could not be displayed completely. The original solid was retained.").as_ref());
            return Task::none();
        };

        self.push_undo_snapshot(i, "SHELL");
        self.tabs[i].scene.delete_solid_history(handle);
        if !self.tabs[i].scene.update_entity(entity) {
            self.command_line
                .push_error(crate::t!("The solid could not be updated.").as_ref());
            return Task::none();
        }
        self.tabs[i]
            .scene
            .create_solid_history(handle, solid_history::brep_op(&result));
        self.tabs[i]
            .scene
            .register_prepared_solid_model(handle, result, display);
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.select_entity(handle, false);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::t!("SHELL: solid updated.").as_ref());
        Task::none()
    }

    /// Intersect compatible selected solids, Regions, and coplanar Surfaces.
    pub(super) fn solid_intersect(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let mut handles = self.selected_intersect_handles();
        self.tabs[i].scene.restore_solid_models(&handles);
        handles.retain(|handle| self.tabs[i].scene.solid_models.contains_key(handle));
        let bodies = handles
            .iter()
            .filter_map(|handle| {
                self.tabs[i]
                    .scene
                    .solid_models
                    .get(handle)
                    .cloned()
                    .map(|body| (*handle, body))
            })
            .collect::<HashMap<_, _>>();
        let groups = self.intersect_groups(&handles, &bodies);
        if groups.is_empty() {
            self.command_line.push_error(
                crate::t!("INTERSECT: select at least two solids, Regions, or coplanar Surfaces of a compatible type.")
                    .as_ref(),
            );
            return Task::none();
        }

        let mut prepared = Vec::new();
        let mut retained_without_change = 0usize;
        for group in groups {
            let operands = group
                .handles
                .iter()
                .map(|handle| bodies[handle].clone())
                .collect::<Vec<_>>();
            let outcome = match intersect_bodies(group.kind, operands) {
                Ok(outcome) => outcome,
                Err(cadkernel::brep::Snag::NoClosedForm) => {
                    self.command_line.push_error(
                        crate::t!("INTERSECT: the selected objects do not have a supported coplanar surface intersection.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::Coincident) => {
                    self.command_line.push_error(
                        crate::t!("INTERSECT: the selected coincident geometry is ambiguous.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::CutRefused) => {
                    self.command_line.push_error(
                        crate::t!("INTERSECT: the selected topology could not be intersected safely.")
                            .as_ref(),
                    );
                    return Task::none();
                }
            };

            match outcome {
                IntersectBodyOutcome::Area(body) => {
                    let retained = group.handles[0];
                    let Some(source) = self.tabs[i].scene.document.get_entity(retained).cloned()
                    else {
                        return Task::none();
                    };
                    let Some(entity) = entity_with_boolean_body(source, &body) else {
                        self.command_line.push_error(
                            crate::t!("The INTERSECT result could not be encoded as ACIS.")
                                .as_ref(),
                        );
                        return Task::none();
                    };
                    let Some(display) = self.tabs[i]
                        .scene
                        .prepare_solid_model_display(retained, &body)
                        .filter(|display| display.0.complete)
                    else {
                        self.command_line.push_error(
                            crate::t!("The INTERSECT result could not be displayed completely. The original objects were retained.")
                                .as_ref(),
                        );
                        return Task::none();
                    };
                    prepared.push(PreparedIntersect {
                        handles: group.handles,
                        outcome: PreparedIntersectOutcome::Replace {
                            retained,
                            entity,
                            body,
                            display,
                        },
                    });
                }
                IntersectBodyOutcome::Touching
                    if group.kind == UnionEntityKind::Region
                        || group.kind == UnionEntityKind::Surface =>
                {
                    prepared.push(PreparedIntersect {
                        handles: group.handles,
                        outcome: PreparedIntersectOutcome::Consume,
                    });
                }
                IntersectBodyOutcome::Disjoint if group.kind == UnionEntityKind::Region => {
                    prepared.push(PreparedIntersect {
                        handles: group.handles,
                        outcome: PreparedIntersectOutcome::Consume,
                    });
                }
                IntersectBodyOutcome::Touching | IntersectBodyOutcome::Disjoint => {
                    retained_without_change += group.handles.len();
                }
            }
        }

        if prepared.is_empty() {
            self.command_line.push_output(
                crate::tf!(
                    "INTERSECT: no positive-area common result; %{count} original object(s) retained.",
                    count = retained_without_change
                )
                .as_ref(),
            );
            return Task::none();
        }

        let record_history = self.tabs[i].scene.document.header.record_solid_history;
        self.push_undo_snapshot(i, "INTERSECT");
        let mut results = Vec::new();
        let mut consumed = Vec::new();
        for group in prepared {
            match group.outcome {
                PreparedIntersectOutcome::Replace {
                    retained,
                    entity,
                    body,
                    display,
                } => {
                    self.tabs[i].scene.delete_solid_history(retained);
                    if !self.tabs[i].scene.update_entity(entity) {
                        self.command_line.push_error(
                            crate::t!("INTERSECT: the retained object could not be updated.")
                                .as_ref(),
                        );
                        return Task::none();
                    }
                    if record_history
                        && matches!(
                            self.tabs[i].scene.document.get_entity(retained),
                            Some(EntityType::Solid3D(_))
                        )
                    {
                        let history = solid_history::brep_op(&body);
                        self.tabs[i].scene.create_solid_history(retained, history);
                    }
                    self.tabs[i]
                        .scene
                        .register_prepared_solid_model(retained, body, display);
                    results.push(retained);
                    consumed.extend(
                        group
                            .handles
                            .into_iter()
                            .filter(|handle| *handle != retained),
                    );
                }
                PreparedIntersectOutcome::Consume => consumed.extend(group.handles),
            }
        }
        self.tabs[i].scene.erase_entities(&consumed);
        self.tabs[i].scene.deselect_all();
        for handle in &results {
            self.tabs[i].scene.select_entity(*handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(
            crate::tf!(
                "INTERSECT: created %{count} result object(s).",
                count = results.len()
            )
            .as_ref(),
        );
        Task::none()
    }

    /// Run a boolean over the selected solids in selection order.
    pub(super) fn solid_boolean(&mut self, op: BoolOp) -> Task<Message> {
        match op {
            BoolOp::Union => return self.union_selected_entities(),
            BoolOp::Intersect => return self.solid_intersect(),
            BoolOp::Subtract => {}
        }
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() < 2 {
            self.command_line
                .push_error(crate::t!("Boolean: select at least two solids.").as_ref());
            return Task::none();
        }
        let kind = match op {
            BoolOp::Union => Bool::Union,
            BoolOp::Subtract => Bool::Subtract,
            BoolOp::Intersect => Bool::Intersect,
        };
        let mut operands = handles
            .iter()
            .filter_map(|handle| self.tabs[i].scene.solid_models.get(handle).cloned());
        let mut result = operands.next().expect("at least two restored solids");
        for operand in operands {
            let Some(combined) = solid_model::boolean(kind, &result, &operand) else {
                self.command_line.push_error(
                    crate::t!("Boolean failed while combining the selected solids.").as_ref(),
                );
                return Task::none();
            };
            result = combined;
        }
        if crate::scene::convert::acis_export::solid_to_sat(&result).is_none() {
            self.command_line
                .push_error(crate::t!("The boolean result could not be encoded as ACIS.").as_ref());
            return Task::none();
        }

        self.push_undo_snapshot(i, "BOOLEAN");
        self.tabs[i].scene.erase_entities(&handles);
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&result);
        let history = solid_history::brep_op(&result);
        let handle = self.add_solid_model(EntityType::Solid3D(s3d), result, history);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        Task::none()
    }

    fn union_selected_entities(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let mut groups = self.selected_union_groups();
        let handles = groups
            .iter()
            .flat_map(|group| group.handles.iter().copied())
            .collect::<Vec<_>>();
        self.tabs[i].scene.restore_solid_models(&handles);
        for group in &mut groups {
            group
                .handles
                .retain(|handle| self.tabs[i].scene.solid_models.contains_key(handle));
        }
        groups.retain(|group| group.handles.len() >= 2);
        if groups.is_empty() {
            self.command_line.push_error(
                crate::t!("UNION: select at least two solids, Regions, or planar Surfaces of the same type.")
                    .as_ref(),
            );
            return Task::none();
        }

        let mut prepared = Vec::with_capacity(groups.len());
        for group in groups {
            let bodies = group
                .handles
                .iter()
                .filter_map(|handle| self.tabs[i].scene.solid_models.get(handle).cloned())
                .collect::<Vec<_>>();
            let result = match union_bodies(group.kind, bodies) {
                Ok(result) => result,
                Err(cadkernel::brep::Snag::NoClosedForm) => {
                    self.command_line.push_error(
                        crate::t!("UNION: the selected geometry includes an unsupported surface intersection.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::Coincident) => {
                    self.command_line.push_error(
                        crate::t!("UNION: the selected coincident geometry is ambiguous.").as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::CutRefused) => {
                    self.command_line.push_error(
                        crate::t!("UNION: the selected topology could not be closed safely.").as_ref(),
                    );
                    return Task::none();
                }
            };
            if crate::scene::convert::acis_export::solid_to_sat(&result).is_none() {
                self.command_line.push_error(
                    crate::t!("The UNION result could not be encoded as ACIS.").as_ref(),
                );
                return Task::none();
            }
            let Some(source) = group
                .handles
                .first()
                .and_then(|handle| self.tabs[i].scene.document.get_entity(*handle))
            else {
                return Task::none();
            };
            prepared.push(PreparedUnion {
                kind: group.kind,
                handles: group.handles,
                body: result,
                common: inherited_common(source.common()),
            });
        }

        let consumed = prepared
            .iter()
            .flat_map(|group| group.handles.iter().copied())
            .collect::<Vec<_>>();
        self.push_undo_snapshot(i, "UNION");
        let mut created = Vec::with_capacity(prepared.len());
        for group in prepared {
            let handle = match group.kind {
                UnionEntityKind::Solid => {
                    let history = solid_history::brep_op(&group.body);
                    let mut entity = Solid3D::new();
                    entity.common = group.common;
                    self.add_solid_model_preserving_style(
                        EntityType::Solid3D(entity),
                        group.body,
                        history,
                    )
                }
                UnionEntityKind::Region => {
                    let mut entity = Region::new();
                    entity.common = group.common;
                    self.add_region_model_preserving_style(entity, group.body)
                }
                UnionEntityKind::Surface => {
                    let mut entity = Surface::new(SurfaceKind::Generic);
                    entity.common = group.common;
                    self.add_surface_model_preserving_style(
                        EntityType::Surface(entity),
                        group.body,
                    )
                }
            };
            if handle.is_null() {
                self.tabs[i].scene.rollback_new_entities(&created);
                self.discard_last_undo_entry(i);
                return Task::none();
            }
            created.push(handle);
        }

        self.tabs[i].scene.erase_entities(&consumed);
        self.tabs[i].scene.deselect_all();
        for handle in &created {
            self.tabs[i].scene.select_entity(*handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(
            crate::tf!("UNION: created %{count} result object(s).", count = created.len()).as_ref(),
        );
        Task::none()
    }

    pub(super) fn solid_subtract(
        &mut self,
        bases: &[Handle],
        cutters: &[Handle],
        convert_meshes: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        let valid_operand = |handle: &Handle| {
            !self.tabs[i].scene.is_layer_locked(*handle)
                && self.tabs[i]
                    .scene
                    .document
                    .get_entity(*handle)
                    .and_then(|entity| subtract_entity_kind(entity, convert_meshes))
                    .is_some()
        };
        let mut base_handles: Vec<_> = bases.iter().copied().filter(valid_operand).collect();
        let mut cutter_handles: Vec<_> = cutters.iter().copied().filter(valid_operand).collect();
        cutter_handles.retain(|handle| !base_handles.contains(handle));

        let mut operands = base_handles.clone();
        operands.extend(cutter_handles.iter().copied());
        self.tabs[i].scene.restore_solid_models(&operands);
        let mut operand_bodies = std::collections::HashMap::<Handle, Body>::new();
        for handle in &operands {
            if let Some(body) = self.tabs[i].scene.solid_models.get(handle).cloned() {
                operand_bodies.insert(*handle, body);
                continue;
            }
            let Some(entity) = self.tabs[i].scene.document.get_entity(*handle) else {
                continue;
            };
            if convert_meshes
                && matches!(
                    entity,
                    EntityType::Mesh(_) | EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_)
                )
            {
                let Some(body) = crate::entities::mesh::closed_mesh_body(entity) else {
                    self.command_line.push_error(
                        crate::t!("SUBTRACT: a selected Mesh is not a supported closed single-shell mesh; no object was changed.")
                            .as_ref(),
                    );
                    return Task::none();
                };
                operand_bodies.insert(*handle, body);
            }
        }
        base_handles.retain(|handle| operand_bodies.contains_key(handle));
        cutter_handles.retain(|handle| operand_bodies.contains_key(handle));
        if base_handles.is_empty() || cutter_handles.is_empty() {
            self.command_line.push_error(
                crate::t!("SUBTRACT: select at least one base and one cutter Solid, Region, or Surface.")
                    .as_ref(),
            );
            return Task::none();
        }

        let kind_for = |handle: Handle| {
            self.tabs[i]
                .scene
                .document
                .get_entity(handle)
                .and_then(|entity| subtract_entity_kind(entity, convert_meshes))
        };
        let mut groups = Vec::<SubtractGroup>::new();
        for kind in [
            UnionEntityKind::Solid,
            UnionEntityKind::Region,
            UnionEntityKind::Surface,
        ] {
            let kind_bases = base_handles
                .iter()
                .copied()
                .filter(|handle| kind_for(*handle) == Some(kind))
                .collect::<Vec<_>>();
            if kind_bases.is_empty() {
                continue;
            }
            if kind == UnionEntityKind::Solid {
                let kind_cutters = cutter_handles
                    .iter()
                    .copied()
                    .filter(|handle| kind_for(*handle) == Some(kind))
                    .collect::<Vec<_>>();
                if !kind_cutters.is_empty() {
                    groups.push(SubtractGroup {
                        kind,
                        plane: None,
                        bases: kind_bases,
                        cutters: kind_cutters,
                    });
                }
                continue;
            }

            for handle in kind_bases {
                let body = &operand_bodies[&handle];
                let plane = planar_body_plane(body);
                if let Some(group) = groups.iter_mut().find(|group| {
                    group.kind == kind
                        && match (group.plane, plane) {
                            (Some(group_plane), Some(_)) => coplanar_bodies(group_plane, body),
                            (None, None) => true,
                            _ => false,
                        }
                }) {
                    group.bases.push(handle);
                } else {
                    groups.push(SubtractGroup {
                        kind,
                        plane,
                        bases: vec![handle],
                        cutters: Vec::new(),
                    });
                }
            }
            for handle in cutter_handles
                .iter()
                .copied()
                .filter(|handle| kind_for(*handle) == Some(kind))
            {
                let body = &operand_bodies[&handle];
                let plane = planar_body_plane(body);
                if let Some(group) = groups.iter_mut().find(|group| {
                    group.kind == kind
                        && match (group.plane, plane) {
                            (Some(group_plane), Some(_)) => coplanar_bodies(group_plane, body),
                            (None, None) => true,
                            _ => false,
                        }
                }) {
                    group.cutters.push(handle);
                }
            }
        }
        groups.retain(|group| !group.bases.is_empty() && !group.cutters.is_empty());
        if groups.is_empty() {
            self.command_line.push_error(
                crate::t!("SUBTRACT: no compatible base and cutter types or planes were selected.")
                    .as_ref(),
            );
            return Task::none();
        }

        let mut prepared = Vec::with_capacity(groups.len());
        for group in groups {
            let base_bodies = group
                .bases
                .iter()
                .map(|handle| operand_bodies[handle].clone())
                .collect::<Vec<_>>();
            let cutter_bodies = group
                .cutters
                .iter()
                .map(|handle| operand_bodies[handle].clone())
                .collect::<Vec<_>>();
            let result = match subtract_bodies(group.kind, group.plane, base_bodies, cutter_bodies) {
                Ok(result) => result,
                Err(cadkernel::brep::Snag::NoClosedForm) => {
                    self.command_line.push_error(
                        crate::t!("SUBTRACT: the selected geometry includes an unsupported surface intersection.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::Coincident) => {
                    self.command_line.push_error(
                        crate::t!("SUBTRACT: the selected coincident geometry is ambiguous.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::CutRefused) => {
                    self.command_line.push_error(
                        crate::t!("SUBTRACT: the selected topology could not be cut safely.")
                            .as_ref(),
                    );
                    return Task::none();
                }
            };
            let staged_result = if result.faces.is_empty() {
                None
            } else {
                let retained = group.bases[0];
                let Some(source) = self.tabs[i].scene.document.get_entity(retained).cloned() else {
                    return Task::none();
                };
                let Some(entity) = entity_with_boolean_body(source, &result) else {
                    self.command_line.push_error(
                        crate::t!("The SUBTRACT result could not be encoded as ACIS.").as_ref(),
                    );
                    return Task::none();
                };
                let Some(display) = self.tabs[i]
                    .scene
                    .prepare_solid_model_display(retained, &result)
                    .filter(|display| display.0.complete)
                else {
                    self.command_line.push_error(
                        crate::t!("The SUBTRACT result could not be displayed completely. The original objects were retained.")
                            .as_ref(),
                    );
                    return Task::none();
                };
                Some((retained, entity, result, display))
            };
            prepared.push(PreparedSubtract {
                bases: group.bases,
                cutters: group.cutters,
                result: staged_result,
            });
        }

        let record_history = self.tabs[i].scene.document.header.record_solid_history;
        self.push_undo_snapshot(i, "SUBTRACT");
        let mut retained = Vec::new();
        let mut consumed = Vec::new();
        for group in prepared {
            if let Some((handle, entity, body, display)) = group.result {
                self.tabs[i].scene.delete_solid_history(handle);
                if !self.tabs[i].scene.update_entity(entity) {
                    self.command_line.push_error(
                        crate::t!("SUBTRACT: the retained base object could not be updated.").as_ref(),
                    );
                    return Task::none();
                }
                if record_history
                    && matches!(
                        self.tabs[i].scene.document.get_entity(handle),
                        Some(EntityType::Solid3D(_))
                    )
                {
                    let history = solid_history::brep_op(&body);
                    self.tabs[i].scene.create_solid_history(handle, history);
                }
                self.tabs[i]
                    .scene
                    .register_prepared_solid_model(handle, body, display);
                retained.push(handle);
                consumed.extend(group.bases.into_iter().skip(1));
            } else {
                consumed.extend(group.bases);
            }
            consumed.extend(group.cutters);
        }
        self.tabs[i].scene.erase_entities(&consumed);
        self.tabs[i].scene.deselect_all();
        for handle in &retained {
            self.tabs[i].scene.select_entity(*handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(
            crate::tf!(
                "SUBTRACT: created %{count} result object(s).",
                count = retained.len()
            )
            .as_ref(),
        );
        Task::none()
    }

    /// Split every selected solid or surface by an arbitrary plane.  All
    /// geometry, persistence records, and displays are prepared before the
    /// first source entity is changed.
    pub(super) fn solid_slice(
        &mut self,
        requested: &[Handle],
        plane: cadkernel::space::Plane,
        keep_point: Option<glam::DVec3>,
    ) -> Task<Message> {
        self.solid_slice_with_tool(requested, ModelSliceTool::Plane(plane), keep_point)
    }

    pub(super) fn solid_slice_surface(
        &mut self,
        requested: &[Handle],
        cutter: Body,
        keep_point: Option<glam::DVec3>,
    ) -> Task<Message> {
        self.solid_slice_with_tool(requested, ModelSliceTool::Surface(cutter), keep_point)
    }

    fn solid_slice_with_tool(
        &mut self,
        requested: &[Handle],
        tool: ModelSliceTool,
        keep_point: Option<glam::DVec3>,
    ) -> Task<Message> {
        let i = self.active_tab;
        let mut handles = requested
            .iter()
            .copied()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .filter(|handle| {
                matches!(
                    self.tabs[i].scene.document.get_entity(*handle),
                    Some(EntityType::Solid3D(_) | EntityType::Surface(_))
                )
            })
            .collect::<Vec<_>>();
        handles.sort_unstable_by_key(|handle| handle.value());
        handles.dedup();
        self.tabs[i].scene.restore_solid_models(&handles);
        handles.retain(|handle| self.tabs[i].scene.solid_models.contains_key(handle));
        if handles.is_empty() {
            self.command_line.push_error(
                crate::t!("SLICE: select at least one solid or surface.").as_ref(),
            );
            return Task::none();
        }

        let keep_positive = if let Some(point) = keep_point {
            let tolerance = cadkernel::brep::operation_tolerance(
                &handles
                    .iter()
                    .map(|handle| &self.tabs[i].scene.solid_models[handle])
                    .collect::<Vec<_>>(),
            );
            let Some(distance) = tool.side(point.to_array()) else {
                self.command_line
                    .push_error(crate::t!("SLICE: the cutting plane is invalid.").as_ref());
                return Task::none();
            };
            if distance.abs() <= tolerance {
                self.command_line.push_error(
                    crate::t!("SLICE: choose a point away from the cutting plane.").as_ref(),
                );
                return Task::none();
            }
            Some(distance > 0.0)
        } else {
            None
        };

        let mut prepared = Vec::with_capacity(handles.len());
        for handle in &handles {
            let body = &self.tabs[i].scene.solid_models[handle];
            let result = match tool.split(body) {
                Ok(Some(result)) => result,
                Ok(None) => {
                    self.command_line.push_error(
                        crate::t!("SLICE: the cutting plane does not cross every selected object; no object was changed.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::NoClosedForm) => {
                    self.command_line.push_error(
                        crate::t!("SLICE: the selected geometry has an unsupported surface intersection; no object was changed.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::Coincident) => {
                    self.command_line.push_error(
                        crate::t!("SLICE: the cutting plane is coincident with source geometry; no object was changed.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                Err(cadkernel::brep::Snag::CutRefused) => {
                    self.command_line.push_error(
                        crate::t!("SLICE: the selected topology could not be split safely; no object was changed.")
                            .as_ref(),
                    );
                    return Task::none();
                }
            };
            let (body, extra_body) = match keep_positive {
                Some(true) => (result.positive, None),
                Some(false) => (result.negative, None),
                None => (result.negative, Some(result.positive)),
            };
            let Some(source) = self.tabs[i].scene.document.get_entity(*handle).cloned() else {
                return Task::none();
            };
            let Some(entity) = entity_with_boolean_body(source.clone(), &body) else {
                self.command_line.push_error(
                    crate::t!("SLICE: a result could not be encoded; no object was changed.")
                        .as_ref(),
                );
                return Task::none();
            };
            let Some(display) = self.tabs[i]
                .scene
                .prepare_solid_model_display(*handle, &body)
                .filter(|display| display.0.complete)
            else {
                self.command_line.push_error(
                    crate::t!("SLICE: a result could not be displayed completely; no object was changed.")
                        .as_ref(),
                );
                return Task::none();
            };
            let extra = match extra_body {
                Some(extra_body) => {
                    let mut extra_entity = match entity_with_boolean_body(source, &extra_body) {
                        Some(entity) => entity,
                        None => {
                            self.command_line.push_error(
                                crate::t!("SLICE: a result could not be encoded; no object was changed.")
                                    .as_ref(),
                            );
                            return Task::none();
                        }
                    };
                    extra_entity.common_mut().handle = Handle::NULL;
                    Some((extra_entity, extra_body))
                }
                None => None,
            };
            prepared.push(PreparedSlice {
                retained: *handle,
                entity,
                body,
                display,
                extra,
            });
        }

        self.push_undo_snapshot(i, "SLICE");
        let mut created = Vec::new();
        for item in &prepared {
            let Some((entity, body)) = &item.extra else {
                continue;
            };
            let handle = match entity {
                EntityType::Solid3D(_) => self.add_solid_model_preserving_style(
                    entity.clone(),
                    body.clone(),
                    solid_history::brep_op(body),
                ),
                EntityType::Surface(_) => {
                    self.add_surface_model_preserving_style(entity.clone(), body.clone())
                }
                _ => Handle::NULL,
            };
            if handle.is_null() {
                self.tabs[i].scene.rollback_new_entities(&created);
                self.discard_last_undo_entry(i);
                self.command_line.push_error(
                    crate::t!("SLICE: a result could not be committed; all source objects were retained.")
                        .as_ref(),
                );
                return Task::none();
            }
            created.push(handle);
        }

        let record_history = self.tabs[i].scene.document.header.record_solid_history;
        let mut retained = Vec::with_capacity(prepared.len());
        for item in prepared {
            self.tabs[i].scene.delete_solid_history(item.retained);
            if !self.tabs[i].scene.update_entity(item.entity) {
                self.command_line.push_error(
                    crate::t!("SLICE: a retained result could not be updated.").as_ref(),
                );
                return Task::none();
            }
            if record_history
                && matches!(
                    self.tabs[i].scene.document.get_entity(item.retained),
                    Some(EntityType::Solid3D(_))
                )
            {
                self.tabs[i]
                    .scene
                    .create_solid_history(item.retained, solid_history::brep_op(&item.body));
            }
            self.tabs[i].scene.register_prepared_solid_model(
                item.retained,
                item.body,
                item.display,
            );
            retained.push(item.retained);
        }

        retained.extend(created);
        self.tabs[i].scene.deselect_all();
        for handle in &retained {
            self.tabs[i].scene.select_entity(*handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(
            crate::tf!(
                "SLICE: created %{count} result object(s).",
                count = retained.len()
            )
            .as_ref(),
        );
        Task::none()
    }

    pub(super) fn slice_selected(
        &mut self,
        plane: cadkernel::space::Plane,
        keep_point: Option<glam::DVec3>,
    ) -> Task<Message> {
        let handles = self.tabs[self.active_tab].scene.selected_handles_in_order();
        self.solid_slice(&handles, plane, keep_point)
    }

    /// INTERFERE — create a solid from the overlap of the two selected solids,
    /// leaving the originals in place (a non-destructive boolean intersect).
    pub(super) fn solid_interfere(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 2 {
            self.command_line
                .push_error(crate::t!("INTERFERE: select exactly two solids created this session.").as_ref());
            return Task::none();
        }
        let a = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let b = self.tabs[i].scene.solid_models[&handles[1]].clone();
        match solid_model::boolean(Bool::Intersect, &a, &b) {
            Some(result) => {
                self.push_undo_snapshot(i, "INTERFERE");
                // Keep both originals; add the interference solid.
                let mut s3d = Solid3D::new();
                s3d.wires = solid_model::edge_wires(&result);
                let history = solid_history::brep_op(&result);
                self.add_solid_model(EntityType::Solid3D(s3d), result, history);
                self.tabs[i].dirty = true;
                self.refresh_properties();
                self.command_line
                    .push_output(crate::t!("INTERFERE: created an interference solid from the overlap.").as_ref());
            }
            None => self
                .command_line
                .push_output(crate::t!("INTERFERE: the selected solids do not overlap.").as_ref()),
        }
        Task::none()
    }

    /// 3DROTATE — rotate the one selected solid about the X/Y/Z axis (0/1/2)
    /// through its centre by `angle_deg` degrees. Rotation preserves the solid's
    /// orientation, so it reuses the cached B-rep directly.
    pub(super) fn solid_rotate3d(&mut self, axis: usize, angle_deg: f64) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("3DROTATE: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let Some(middle) = solid_model::centre(&solid) else {
            self.command_line
                .push_error(crate::t!("3DROTATE: could not determine the solid's extent.").as_ref());
            return Task::none();
        };
        let Some(rotated) =
            solid_model::turned(&solid, axis, angle_deg.to_radians(), middle)
        else {
            self.command_line
                .push_error(crate::t!("3DROTATE: could not turn the solid.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "3DROTATE");
        self.tabs[i].scene.erase_entities(&handles);
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&rotated);
        let history = solid_history::brep_op(&rotated);
        let handle = self.add_solid_model(EntityType::Solid3D(s3d), rotated, history);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(crate::tf!(
            "3DROTATE: rotated {angle_deg}° about the {} axis.",
            ["X", "Y", "Z"][axis]
        ).as_ref());
        Task::none()
    }

    /// 3DMIRROR — add a mirrored copy of the one selected solid across the plane
    /// perpendicular to the X/Y/Z axis (0/1/2) through its centre, keeping the
    /// original. A reflection loses handedness, so the kernel reverses every
    /// loop on the way through; without that the copy lights black.
    pub(super) fn solid_mirror3d(&mut self, axis: usize) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("3DMIRROR: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let Some(middle) = solid_model::centre(&solid) else {
            self.command_line
                .push_error(crate::t!("3DMIRROR: could not determine the solid's extent.").as_ref());
            return Task::none();
        };
        let Some(reflected) = solid_model::mirrored(&solid, axis, middle) else {
            self.command_line
                .push_error(crate::t!("3DMIRROR: could not mirror the solid.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "3DMIRROR");
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&reflected);
        let history = solid_history::brep_op(&reflected);
        let h = self.add_solid_model(EntityType::Solid3D(s3d), reflected, history);
        self.tabs[i].scene.deselect_all();
        if !h.is_null() {
            self.tabs[i].scene.select_entity(h, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(crate::tf!(
            "3DMIRROR: added a mirror across the {} plane.",
            ["X", "Y", "Z"][axis]
        ).as_ref());
        Task::none()
    }

    /// 3DALIGN — move/rotate the one selected solid so its three source points
    /// land on the three destination points. The frame-to-frame transform is
    /// computed in glam (`M = D · S⁻¹`); both frames are right-handed, so the
    /// result is a pure rotation and translation and the kernel accepts it.
    pub(super) fn solid_align3d(
        &mut self,
        src: [[f64; 3]; 3],
        dst: [[f64; 3]; 3],
    ) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("3DALIGN: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        // Build a right-handed frame (origin + orthonormal axes) from 3 points.
        let frame = |p: [[f64; 3]; 3]| -> Option<glam::DMat4> {
            let p1 = glam::DVec3::from_array(p[0]);
            let p2 = glam::DVec3::from_array(p[1]);
            let p3 = glam::DVec3::from_array(p[2]);
            let x = (p2 - p1).normalize_or_zero();
            let z = (p2 - p1).cross(p3 - p1).normalize_or_zero();
            if x.length_squared() < 1e-12 || z.length_squared() < 1e-12 {
                return None; // coincident or collinear points
            }
            let y = z.cross(x);
            Some(glam::DMat4::from_cols(
                x.extend(0.0),
                y.extend(0.0),
                z.extend(0.0),
                p1.extend(1.0),
            ))
        };
        let (Some(s), Some(d)) = (frame(src), frame(dst)) else {
            self.command_line
                .push_error(crate::t!("3DALIGN: each point triple must be non-coincident and non-collinear.").as_ref());
            return Task::none();
        };
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let Some(aligned) = solid_model::by_matrix(&solid, (d * s.inverse()).to_cols_array())
        else {
            self.command_line
                .push_error(crate::t!("3DALIGN: could not align the solid.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "3DALIGN");
        self.tabs[i].scene.erase_entities(&handles);
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&aligned);
        let history = solid_history::brep_op(&aligned);
        let h = self.add_solid_model(EntityType::Solid3D(s3d), aligned, history);
        self.tabs[i].scene.deselect_all();
        if !h.is_null() {
            self.tabs[i].scene.select_entity(h, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::t!("3DALIGN: aligned the solid to the destination points.").as_ref());
        Task::none()
    }

    /// SECTION — draw the cross-section outline where an axis-aligned plane
    /// (X/Y/Z = `axis` at `value`) cuts the one selected solid, as Line
    /// entities.
    pub(super) fn solid_section(&mut self, axis: usize, value: f64) -> Task<Message> {
        use acadrust::types::Vector3;
        use acadrust::Line;

        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("SECTION: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let Some((min, max)) = solid_model::extent(&solid) else {
            self.command_line
                .push_error(crate::t!("SECTION: could not determine the solid's extent.").as_ref());
            return Task::none();
        };
        // The plane has to actually reach the solid to cut it.
        if value < min[axis] || value > max[axis] {
            self.command_line
                .push_output(crate::t!("SECTION: the plane does not cross the solid.").as_ref());
            return Task::none();
        }
        let segs = solid_model::section(&solid, axis, value);
        if segs.is_empty() {
            self.command_line
                .push_output(crate::t!("SECTION: the plane does not cross the solid.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, "SECTION");
        for (p1, p2) in &segs {
            let line = Line::from_points(
                Vector3::new(p1[0], p1[1], p1[2]),
                Vector3::new(p2[0], p2[1], p2[2]),
            );
            self.tabs[i].scene.add_entity(EntityType::Line(line));
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(crate::tf!(
            "SECTION: created {} section line(s) at {}={value}.",
            segs.len(),
            ["X", "Y", "Z"][axis]
        ).as_ref());
        Task::none()
    }

    /// PYRAMID — create an `n`-sided pyramid: a regular polygon base of the
    /// given circumradius with its apex at `height`.
    ///
    /// A real B-rep rather than a bag of faces, so it joins the boolean tools
    /// and the exact-geometry save path like every other primitive.
    pub(super) fn solid_pyramid(
        &mut self,
        radius: f64,
        height: f64,
        sides: usize,
    ) -> Task<Message> {
        use crate::modules::insert::solid3d_cmds::empty_solid3d;

        let i = self.active_tab;
        let n = sides.max(3);
        let Some(solid) = solid_model::pyramid_solid([0.0; 3], radius, height, n) else {
            self.command_line
                .push_error(crate::t!("PYRAMID: radius and height must be positive.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "PYRAMID");
        let mut entity = empty_solid3d();
        if let EntityType::Solid3D(inner) = &mut entity {
            inner.wires = solid_model::edge_wires(&solid);
        }
        let history = solid_history::pyramid_op(
            glam::DMat4::IDENTITY.to_cols_array(),
            radius,
            0.0,
            height,
            n,
            true,
        );
        let handle = self.add_solid_model(entity, solid, history);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
            self.tabs[i].dirty = true;
            self.refresh_properties();
            self.command_line.push_output(crate::tf!(
                "PYRAMID: created a {n}-sided pyramid (radius {radius}, height {height})."
            ).as_ref());
        }
        Task::none()
    }

    /// SPLINEFIT — replace the selected polyline with a cubic spline that passes
    /// through its vertices. Control points come from the Catmull-Rom → cubic
    /// Bézier formula (the curve provably interpolates each vertex), with a
    /// clamped piecewise-Bézier knot vector the spline renderer reads directly.
    pub(super) fn fit_spline(&mut self) -> Task<Message> {
        use acadrust::entities::Spline;
        use acadrust::types::Vector3;

        let i = self.active_tab;
        let found: Option<(Handle, Vec<[f64; 3]>)> = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .filter(|(h, _)| !self.tabs[i].scene.is_layer_locked(*h))
            .find_map(|(h, e)| {
                let EntityType::LwPolyline(_) = e else {
                    return None;
                };
                let curve = crate::entities::curve::entity_curve(e)?;
                let cadkernel::geom2d::Curve::Polyline(polyline) = &curve.curve else {
                    return None;
                };
                Some((
                    *h,
                    polyline
                        .vertices
                        .iter()
                        .map(|vertex| curve.plane.point_at(vertex.position))
                        .collect(),
                ))
            });
        let Some((handle, fit)) = found else {
            self.command_line
                .push_error(crate::t!("SPLINEFIT: select a polyline to fit a spline through.").as_ref());
            return Task::none();
        };
        if fit.len() < 3 {
            self.command_line
                .push_error(crate::t!("SPLINEFIT: need at least 3 points.").as_ref());
            return Task::none();
        }
        let n = fit.len();
        let m = n - 1; // Bézier segments
        let p = |k: usize| glam::DVec3::new(fit[k][0], fit[k][1], fit[k][2]);
        // Catmull-Rom → cubic Bézier control points: [P0, b1,b2,P1, b1,b2,P2, …].
        let mut ctrl: Vec<Vector3> = Vec::with_capacity(3 * m + 1);
        ctrl.push(Vector3::new(fit[0][0], fit[0][1], fit[0][2]));
        for seg in 0..m {
            let p0 = p(seg);
            let p1 = p(seg + 1);
            let prev = if seg > 0 { p(seg - 1) } else { p0 };
            let next = if seg + 2 <= m { p(seg + 2) } else { p1 };
            let b1 = p0 + (p1 - prev) / 6.0;
            let b2 = p1 - (next - p0) / 6.0;
            ctrl.push(Vector3::new(b1.x, b1.y, b1.z));
            ctrl.push(Vector3::new(b2.x, b2.y, b2.z));
            ctrl.push(Vector3::new(p1.x, p1.y, p1.z));
        }
        // Clamped piecewise-Bézier knots (degree 3): len == ctrl.len()+degree+1.
        let mut knots: Vec<f64> = vec![0.0; 4];
        for s in 1..m {
            knots.extend_from_slice(&[s as f64, s as f64, s as f64]);
        }
        knots.extend_from_slice(&[m as f64; 4]);
        let mut spl = Spline::new();
        spl.degree = 3;
        spl.control_points = ctrl;
        spl.knots = knots;
        spl.fit_points = fit
            .iter()
            .map(|q| Vector3::new(q[0], q[1], q[2]))
            .collect();
        // flags.rational defaults to false (non-rational) — exactly what we want.
        self.push_undo_snapshot(i, "SPLINEFIT");
        self.tabs[i].scene.erase_entities(&[handle]);
        self.tabs[i].scene.add_entity(EntityType::Spline(spl));
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::tf!("SPLINEFIT: fit a spline through {n} points.").as_ref());
        Task::none()
    }

    /// FLATSHOT — project the selected solid's edges onto the XY plane (Z=0) as
    /// Line entities, giving a flattened 2D shot of the model. Reuses the cached
    /// solid's edge wires (the same source SECTION uses).
    pub(super) fn solid_flatshot(&mut self) -> Task<Message> {
        use acadrust::types::Vector3;
        use acadrust::Line;
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.is_empty() {
            self.command_line
                .push_error(crate::t!("FLATSHOT: select a solid created this session.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, "FLATSHOT");
        let mut n = 0usize;
        for h in &handles {
            let solid = self.tabs[i].scene.solid_models[h].clone();
            for w in solid_model::edge_wires(&solid) {
                for seg in w.points.windows(2) {
                    let line = Line::from_points(
                        Vector3::new(seg[0].x, seg[0].y, 0.0),
                        Vector3::new(seg[1].x, seg[1].y, 0.0),
                    );
                    self.tabs[i].scene.add_entity(EntityType::Line(line));
                    n += 1;
                }
            }
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::tf!("FLATSHOT: created {n} projected edge(s) at Z=0.").as_ref());
        Task::none()
    }

    /// CONVTOSURFACE — convert the selected solid(s) into Surface entities,
    /// carrying the solid's edge wires (reuses the cached B-rep's edge wires).
    pub(super) fn solid_convtosurface(&mut self) -> Task<Message> {
        use acadrust::entities::{Surface, SurfaceKind, Wire as AWire};
        use acadrust::types::Vector3;
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.is_empty() {
            self.command_line
                .push_error(crate::t!("CONVTOSURFACE: select a solid created this session.").as_ref());
            return Task::none();
        }
        let mut surfaces: Vec<Surface> = Vec::new();
        for h in &handles {
            let solid = self.tabs[i].scene.solid_models[h].clone();
            let awires: Vec<AWire> = solid_model::edge_wires(&solid)
                .into_iter()
                .map(|w| {
                    let mut aw = AWire::new();
                    aw.points = w
                        .points
                        .iter()
                        .map(|p| Vector3::new(p.x, p.y, p.z))
                        .collect();
                    aw
                })
                .collect();
            let mut surf = Surface::new(SurfaceKind::Generic);
            surf.wires = awires;
            surf.common.layer = self.tabs[i].active_layer.clone();
            surfaces.push(surf);
        }
        self.push_undo_snapshot(i, "CONVTOSURFACE");
        self.tabs[i].scene.erase_entities(&handles);
        let n = surfaces.len();
        for surf in surfaces {
            self.tabs[i].scene.add_entity(EntityType::Surface(surf));
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::tf!("CONVTOSURFACE: converted {n} solid(s) to surface(s).").as_ref());
        Task::none()
    }
}
