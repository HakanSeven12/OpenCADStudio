//! PRESSPULL scene integration. Resolve first; build and validate the complete
//! batch before recording undo or changing any persistent entity.

use acadrust::entities::{AcisData, EmbeddedEntity, Solid3D, Surface, SurfaceKind, Wire};
use acadrust::objects::{DynamicBlockData, ObjectType, SolidHistoryNodeBase, SolidHistoryOperation, SolidHistorySweep};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use cadkernel::brep::{self, Body, PlanarFaceProfile, PresspullMode};
use glam::DVec3;
use crate::scene::model::{solid_history, sweep_model};
use crate::scene::model::mesh_model::MeshLodSet;
use crate::scene::model::presspull_model::{self, PresspullTarget, PresspullTargetKind};

struct PreparedPresspull {
    owner: Option<Handle>,
    entity: EntityType,
    body: Body,
    history: SolidHistoryOperation,
    display: (MeshLodSet, Vec<Wire>, [f64; 3]),
}

fn extrusion_history(entity: &EntityType, direction: DVec3, anchor: DVec3) -> Option<SolidHistoryOperation> {
    if let EntityType::Region(region) = entity {
        let mut base = SolidHistoryNodeBase::new(1);
        base.transform = glam::DMat4::IDENTITY.to_cols_array();
        return Some(SolidHistoryOperation::Extrusion(SolidHistorySweep {
            base,
            operation_major: 1,
            sweep_entity: Some(EmbeddedEntity::Region(region.clone())),
            direction: Vector3::new(direction.x, direction.y, direction.z),
            end_draft_distance: direction.length(),
            scale_factor: 1.0,
            sweep_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
            path_entity_transform: glam::DMat4::IDENTITY.to_cols_array(),
            reference_point: Vector3::new(anchor.x, anchor.y, anchor.z),
            ..SolidHistorySweep::default()
        }));
    }
    sweep_model::extrusion_history(entity, None, direction.to_array(), 0.0, anchor.to_array())
}

impl super::OpenCADStudio {
    fn restore_presspull_bodies(&mut self) {
        let scene = &mut self.tabs[self.active_tab].scene;
        let handles = scene.document.entities().filter_map(|entity| {
            matches!(entity, EntityType::Solid3D(_)).then_some(entity.common().handle)
        }).collect::<Vec<_>>();
        scene.restore_solid_models(&handles);
    }

    pub(super) fn presspull_preselection(&mut self) -> Vec<PresspullTarget> {
        self.restore_presspull_bodies();
        let tab = &self.tabs[self.active_tab];
        let plane = tab.ucs_xform().working_plane();
        tab.scene.selected_handles_in_order().into_iter().filter_map(|handle| {
            let entity = tab.scene.document.get_entity(handle)?;
            let (profile_plane, loops, _) = presspull_model::profile_geometry(entity)?;
            let first = loops.first()?.first()?.point_at(0.0);
            let anchor = DVec3::from_array(profile_plane.point_at(first));
            presspull_model::resolve_target(&tab.scene, Some(handle), anchor, plane, false).ok()
        }).collect()
    }

    pub(super) fn presspull_pick(&mut self, handle: Option<Handle>, point: DVec3, offset: bool) {
        self.restore_presspull_bodies();
        let i = self.active_tab;
        let target = presspull_model::resolve_target(
            &self.tabs[i].scene, handle, point, self.tabs[i].ucs_xform().working_plane(), offset,
        );
        match target {
            Ok(target) => {
                if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                    command.on_presspull_target(target);
                }
                self.tabs[i].scene.set_hover_highlight(None);
            }
            Err(error) => self.command_line.push_error(&error),
        }
        self.refresh_presspull_prompt();
    }

    fn refresh_presspull_prompt(&mut self) {
        if let Some(command) = self.tabs[self.active_tab].active_cmd.as_ref() {
            self.command_line.push_info(&command.prompt());
            self.command_line.set_step_options(command.options());
        }
        self.sync_dyn_fields();
    }

    fn prepare_presspull(&self, targets: &[PresspullTarget], distance: f64) -> Result<Vec<PreparedPresspull>, String> {
        if targets.is_empty() || !distance.is_finite() || distance.abs() <= 1e-9 {
            return Err("PRESSPULL: enter a finite non-zero height.".into());
        }
        let scene = &self.tabs[self.active_tab].scene;
        let mut results: Vec<(Option<Handle>, EntityType, Body, SolidHistoryOperation)> = Vec::new();
        for target in targets {
            let owner = match &target.kind {
                PresspullTargetKind::Profile { source, entity, owner } => {
                    if let Some(source) = source {
                        if scene.is_layer_locked(*source) || scene.document.get_entity(*source) != Some(entity) {
                            return Err("PRESSPULL: a selected source changed. Select it again.".into());
                        }
                    }
                    *owner
                }
                PresspullTargetKind::Face { handle, .. } => Some(*handle),
            };
            if owner.is_some_and(|handle| scene.is_layer_locked(handle)) {
                return Err("PRESSPULL: the solid is on a locked layer.".into());
            }
            let previous = owner.and_then(|handle| results.iter().position(|result| result.0 == Some(handle)));
            let current = previous.map(|index| &results[index].2)
                .or_else(|| owner.and_then(|handle| scene.solid_models.get(&handle)));
            let (body, history, mut entity) = match &target.kind {
                PresspullTargetKind::Profile { entity, .. } => {
                    let (plane, loops, closed) = presspull_model::profile_geometry(entity)
                        .ok_or("PRESSPULL: the profile could not be recovered exactly.")?;
                    if owner.is_some() {
                        let current = current.ok_or("PRESSPULL: the original solid could not be restored.")?;
                        if !closed {
                            return Err("PRESSPULL: an attached region must be closed.".into());
                        }
                        let profile = PlanarFaceProfile { plane, loops, outward: target.direction.to_array() };
                        let body = brep::presspull_region(current, &profile, distance)
                            .ok_or("PRESSPULL: the region could not be added to or cut from the solid.")?;
                        let history = solid_history::brep_op(&body);
                        (body, history, scene.document.get_entity(owner.unwrap()).cloned()
                            .ok_or("PRESSPULL: the original solid no longer exists.")?)
                    } else {
                        let direction = target.direction * distance;
                        let body = presspull_model::extrusion_body(entity, direction.to_array())
                            .ok_or("PRESSPULL: the profile could not be extruded with this height.")?;
                        let history = extrusion_history(entity, direction, target.anchor)
                            .ok_or("PRESSPULL: the editable extrusion profile could not be retained.")?;
                        let entity = if closed {
                            EntityType::Solid3D(Solid3D::new())
                        } else {
                            let mut surface = Surface::new(SurfaceKind::Extruded);
                            if let SolidHistoryOperation::Extrusion(record) = &history {
                                surface.surface_data = solid_history::extrusion_surface_data(record)
                                    .ok_or("PRESSPULL: the surface parameters could not be retained.")?;
                            }
                            EntityType::Surface(surface)
                        };
                        (body, history, entity)
                    }
                }
                PresspullTargetKind::Face { face, offset, .. } => {
                    let current = current.ok_or("PRESSPULL: the original solid could not be restored.")?;
                    // Earlier targets in the same batch may have rebuilt face keys.
                    let face = if previous.is_some() {
                        brep::planar_face_at_point(current, target.anchor.to_array(), 1e-6)
                            .ok_or("PRESSPULL: an earlier selected region changed this face.")?
                    } else { *face };
                    let body = brep::presspull_face(current, face, distance,
                        if *offset { PresspullMode::Offset } else { PresspullMode::Extrude })
                        .ok_or("PRESSPULL: the requested face operation would invalidate the solid.")?;
                    let history = solid_history::brep_op(&body);
                    (body, history, scene.document.get_entity(owner.unwrap()).cloned()
                        .ok_or("PRESSPULL: the original solid no longer exists.")?)
                }
            };
            let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                .ok_or("PRESSPULL: the result could not be encoded. The original objects were retained.")?;
            if owner.is_none() { entity.common_mut().plotstyle_flags = 2; }
            match &mut entity {
                EntityType::Solid3D(solid) => {
                    solid.history_handle = None;
                    solid.silhouettes.clear();
                    solid.set_sat_document(&sat);
                }
                EntityType::Surface(surface) => {
                    surface.history_handle = None;
                    surface.silhouettes.clear();
                    surface.acis_data = AcisData::from_sat(&sat.to_sat_string());
                }
                _ => return Err("PRESSPULL: unsupported result object.".into()),
            }
            let result = (owner, entity, body, history);
            if let Some(index) = previous { results[index] = result; } else { results.push(result); }
        }
        results.into_iter().map(|(owner, entity, body, history)| {
            let display = scene.prepare_solid_model_display(owner.unwrap_or(Handle::NULL), &body)
                .filter(|display| display.0.complete && display.0.lods.iter().any(|mesh| !mesh.indices.is_empty()))
                .ok_or("PRESSPULL: the complete result could not be displayed. The original objects were retained.")?;
            Ok(PreparedPresspull { owner, entity, body, history, display })
        }).collect()
    }

    pub(super) fn presspull_apply(&mut self, targets: Vec<PresspullTarget>, distance: f64) {
        let i = self.active_tab;
        let prepared = match self.prepare_presspull(&targets, distance) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.command_line.push_error(&error);
                if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                    command.on_presspull_applied(false);
                }
                self.refresh_presspull_prompt();
                return;
            }
        };
        let pending = self.begin_undo(i, "PRESSPULL", prepared.len(), true);
        let mut handles = Vec::new();
        // All fallible geometry, persistence and display work finished above.
        // No source curve is deleted by PRESSPULL.
        for result in prepared {
            let handle = if let Some(handle) = result.owner {
                self.tabs[i].scene.delete_solid_history(handle);
                self.tabs[i].scene.update_entity(result.entity);
                handle
            } else {
                self.commit_entity_handle(result.entity).expect("validated solid/surface insertion")
            };
            self.tabs[i].scene.create_solid_history(handle, result.history);
            // PRESSPULL keeps editable construction parameters without enabling
            // recording of subsequent solid operations by default.
            if result.owner.is_none() {
                let document = &mut self.tabs[i].scene.document;
                if let Some(graph) = document.solid_history_graph(handle) {
                    if let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&graph.root) {
                        if let DynamicBlockData::SolidHistory(history) = &mut object.data {
                            history.record_history = false;
                        }
                    }
                }
            }
            self.tabs[i].scene.register_prepared_solid_model(handle, result.body, result.display);
            handles.push(handle);
        }
        self.tabs[i].scene.deselect_all();
        for handle in &handles { self.tabs[i].scene.select_entity(*handle, false); }
        self.tabs[i].dirty = true;
        if let Some(pending) = pending { self.commit_undo_delta(i, pending); }
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].snap_result = None;
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.on_presspull_applied(true);
        }
        self.command_line.push_output(crate::tf!("PRESSPULL: %{count} object(s) updated.", count = handles.len()).as_ref());
        self.refresh_properties();
        self.refresh_presspull_prompt();
    }
}
