use super::*;

impl OpenCADStudio {
    pub(super) fn handle_extrude_entities(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::ExtrudeEntities {
            handles,
            extent,
            mode,
            taper_angle,
            color: _,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        let delete_sources = self.delete_objects != 0;
        if handles.is_empty()
            || handles
                .iter()
                .any(|handle| self.reject_locked_edit(i, *handle))
        {
            self.tabs[i].active_cmd = None;
            return Some(Task::none());
        }
        use crate::command::{ExtrudeExtent, ExtrudeMode};
        use crate::modules::insert::solid3d_cmds::{empty_extruded_surface, empty_solid3d};
        use crate::scene::model::sweep_model;
        let path = match extent {
            ExtrudeExtent::Path(handle) => self.tabs[i]
                .scene
                .document
                .get_entity(handle)
                .cloned()
                .map(|entity| (handle, entity)),
            _ => None,
        };
        if matches!(extent, ExtrudeExtent::Path(_)) && path.is_none() {
            self.command_line
                .push_error(crate::t!("EXTRUDE: path entity not found.").as_ref());
            self.tabs[i].active_cmd = None;
            return Some(Task::none());
        }
        let pending = self.begin_undo(i, "EXTRUDE", handles.len(), true);
        let mut created_handles = Vec::new();
        let mut consumed = Vec::new();
        let mut failed = 0usize;
        for handle in &handles {
            let Some(entity) = self.tabs[i].scene.document.get_entity(*handle).cloned()
            else {
                failed += 1;
                continue;
            };
            let Some((profile, closed)) = sweep_model::extrusion_profile_of(&entity) else {
                failed += 1;
                continue;
            };
            let direction = match extent {
                ExtrudeExtent::Height(height) => profile.plane.normal().map(|normal| {
                    glam::DVec3::new(
                        normal[0] * height,
                        normal[1] * height,
                        normal[2] * height,
                    )
                }),
                ExtrudeExtent::Direction(direction) => Some(direction),
                ExtrudeExtent::Path(_) => None,
            };
            let path_direction = path
                .as_ref()
                .and_then(|(_, path)| sweep_model::straight_path_direction(path))
                .map(glam::DVec3::from_array);
            // The creation mode controls closed profiles. Open curves
            // always create sheet surfaces.
            let creates_surface = matches!(mode, ExtrudeMode::Surface) || !closed;
            let body = match (creates_surface, &path, direction) {
                (false, Some((_, path)), _) => {
                    sweep_model::extruded_along_path(&entity, path, taper_angle)
                }
                (false, None, Some(direction)) => sweep_model::extruded_direction(
                    &entity,
                    direction.to_array(),
                    taper_angle,
                ),
                (true, None, Some(direction)) => sweep_model::extruded_surface(
                    &entity,
                    direction.to_array(),
                    taper_angle,
                ),
                (true, Some(_), _) => path_direction.and_then(|direction| {
                    sweep_model::extruded_surface(
                        &entity,
                        direction.to_array(),
                        taper_angle,
                    )
                }),
                _ => None,
            };
            let Some(body) = body else {
                failed += 1;
                continue;
            };
            let created = if creates_surface {
                let direction = direction.or(path_direction).unwrap_or(glam::DVec3::ZERO);
                let history = path
                    .is_none()
                    .then(|| {
                        sweep_model::extrusion_history(
                            &entity,
                            None,
                            direction.to_array(),
                            taper_angle,
                            profile.plane.origin,
                        )
                    })
                    .flatten();
                let mut surface = empty_extruded_surface(direction, taper_angle);
                if let (
                    Some(acadrust::objects::SolidHistoryOperation::Extrusion(value)),
                    acadrust::EntityType::Surface(entity),
                ) = (&history, &mut surface)
                {
                    if let Some(data) =
                        crate::scene::model::solid_history::extrusion_surface_data(value)
                    {
                        entity.surface_data = data;
                    }
                }
                match history {
                    Some(history) => {
                        self.add_surface_model_with_history(surface, body, history)
                    }
                    None => self.add_surface_model(surface, body),
                }
            } else {
                let direction = direction.unwrap_or(glam::DVec3::ZERO);
                let history = path
                    .as_ref()
                    .and_then(|(_, path)| {
                        sweep_model::sweep_history(
                            &entity,
                            path,
                            taper_angle,
                            profile.plane.origin,
                        )
                    })
                    .or_else(|| {
                        sweep_model::extrusion_history(
                            &entity,
                            None,
                            direction.to_array(),
                            taper_angle,
                            profile.plane.origin,
                        )
                    })
                    .unwrap_or_else(|| crate::scene::model::solid_history::brep_op(&body));
                self.add_solid_model(empty_solid3d(), body, history)
            };
            if created.is_null() {
                failed += 1;
            } else {
                created_handles.push(created);
                if delete_sources {
                    consumed.push(*handle);
                }
            }
        }
        if delete_sources && !created_handles.is_empty() {
            consumed.sort_unstable_by_key(|handle| handle.value());
            consumed.dedup();
            self.tabs[i].scene.erase_entities(&consumed);
        }
        if !created_handles.is_empty() {
            self.tabs[i].scene.deselect_all();
            for handle in &created_handles {
                self.tabs[i].scene.select_entity(*handle, true);
            }
            self.tabs[i].dirty = true;
            self.command_line.push_output(
                crate::tf!(
                    "EXTRUDE: created %{created} object(s); %{failed} source(s) could not be extruded.",
                    created = created_handles.len(),
                    failed = failed
                )
                .as_ref(),
            );
            if let Some(pending) = pending {
                self.commit_undo_delta(i, pending);
            }
        } else {
            self.command_line.push_error(crate::t!("EXTRUDE: no selected object could be extruded with the requested options.").as_ref());
            if let Some(pending) = pending {
                self.commit_undo_delta(i, pending);
            }
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
        None
    }

    pub(super) fn handle_thicken_entities(&mut self, handles: Vec<Handle>, distance: f64) -> Option<Task<Message>> {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();

        if handles.is_empty() {
            self.command_line
                .push_output(crate::t!("No surfaces selected.").as_ref());
            return Some(Task::none());
        }
        if !distance.is_finite() {
            self.command_line
                .push_error(crate::t!("Requires numeric distance or two points.").as_ref());
            return Some(Task::none());
        }
        if distance.abs() <= f64::EPSILON {
            return Some(Task::none());
        }

        use crate::modules::insert::solid3d_cmds::empty_solid3d;
        let pending = self.begin_undo(i, "THICKEN", handles.len(), true);
        let mut created = 0usize;
        let mut failed = 0usize;
        let mut self_intersections = 0usize;
        for handle in handles {
            let Some(acadrust::EntityType::Surface(surface)) =
                self.tabs[i].scene.document.get_entity(handle)
            else {
                failed += 1;
                continue;
            };
            let kernel_distance =
                if surface.kind == acadrust::entities::SurfaceKind::Revolved {
                    -distance
                } else {
                    distance
                };
            let body = self.tabs[i]
                .scene
                .solid_models
                .get(&handle)
                .cloned()
                .or_else(|| {
                    crate::scene::convert::solid3d_tess::kernel_surface_body(surface)
                });
            let Some(body) = body else {
                failed += 1;
                continue;
            };
            let solid = match cadkernel::brep::thicken(&body, kernel_distance) {
                Ok(solid) => solid,
                Err(cadkernel::brep::ThickenError::SelfIntersection) => {
                    failed += 1;
                    self_intersections += 1;
                    continue;
                }
                Err(_) => {
                    failed += 1;
                    continue;
                }
            };
            let history = crate::scene::model::solid_history::brep_op(&solid);
            if self
                .add_solid_model(empty_solid3d(), solid, history)
                .is_null()
            {
                failed += 1;
            } else {
                created += 1;
            }
        }

        if created > 0 {
            self.tabs[i].dirty = true;
        }
        if self_intersections > 0 {
            self.command_line.push_error(&crate::tf!(
                "{} surface(s) cannot be thickened because the offset intersects itself.",
                self_intersections
            ));
        }
        if failed > 0 {
            self.command_line.push_output(&crate::tf!(
                "THICKEN: created {} solid(s); {} surface(s) could not be thickened exactly.",
                created, failed
            ));
        }
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        self.refresh_properties();
        None
    }

    pub(super) fn handle_presspull_pick(&mut self, result: CmdResult) {
        let CmdResult::PresspullPick {
            handle,
            point,
            offset,
            multiple: _,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        self.presspull_pick(handle, point, offset);
    }

    pub(super) fn handle_presspull_apply(&mut self, targets: Vec<crate::scene::model::presspull_model::PresspullTarget>, distance: f64) {
        self.presspull_apply(targets, distance);
    }

    pub(super) fn handle_revolve_entities(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::RevolveEntities {
            handles,
            axis_start,
            axis_end,
            angle,
            start_angle,
            mode,
            color: _,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        if handles.is_empty() {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.tabs[i].scene.clear_preview_wire();
            self.restore_pre_cmd_tangent();
            return Some(Task::none());
        }
        let delete_sources = self.delete_objects != 0;
        use crate::command::ExtrudeMode;
        use crate::modules::insert::solid3d_cmds::{empty_revolved_surface, empty_solid3d};
        use crate::scene::model::sweep_model;
        let from = axis_start.to_array();
        let to = axis_end.to_array();
        let mut editable_handles = Vec::with_capacity(handles.len());
        let mut failed = 0usize;
        for handle in handles {
            if self.reject_locked_edit(i, handle) {
                failed += 1;
            } else {
                editable_handles.push(handle);
            }
        }
        let pending = if editable_handles.is_empty() {
            None
        } else {
            self.begin_undo(i, "REVOLVE", editable_handles.len(), true)
        };
        let mut created_handles = Vec::new();
        let mut consumed = Vec::new();
        for handle in &editable_handles {
            let Some(entity) = self.tabs[i].scene.document.get_entity(*handle).cloned()
            else {
                failed += 1;
                continue;
            };
            let Some((_, closed)) = sweep_model::extrusion_profile_of(&entity) else {
                failed += 1;
                continue;
            };
            let creates_surface = mode == ExtrudeMode::Surface || !closed;
            let created = if creates_surface {
                let Some(body) =
                    sweep_model::revolved_surface(&entity, from, to, angle, start_angle)
                else {
                    failed += 1;
                    continue;
                };
                self.add_surface_model(
                    empty_revolved_surface(
                        &entity,
                        axis_start,
                        axis_end,
                        angle,
                        start_angle,
                    ),
                    body,
                )
            } else {
                let result =
                    sweep_model::revolve_history(&entity, from, to, angle, start_angle)
                        .and_then(|history| {
                            cadkernel::acis::rebuild_body(&history)
                                .ok()
                                .map(|solid| (solid, history))
                        })
                        .or_else(|| {
                            sweep_model::revolved(&entity, from, to, angle, start_angle)
                                .map(|solid| {
                                    let history =
                                        crate::scene::model::solid_history::brep_op(&solid);
                                    (solid, history)
                                })
                        });
                let Some((solid, history)) = result else {
                    failed += 1;
                    continue;
                };
                self.add_solid_model(empty_solid3d(), solid, history)
            };
            if created.is_null() {
                failed += 1;
            } else {
                created_handles.push(created);
                if delete_sources {
                    consumed.push(*handle);
                }
            }
        }
        if delete_sources && !created_handles.is_empty() {
            consumed.sort_unstable_by_key(|handle| handle.value());
            consumed.dedup();
            self.tabs[i].scene.erase_entities(&consumed);
        }
        if !created_handles.is_empty() {
            self.tabs[i].scene.deselect_all();
            for handle in &created_handles {
                self.tabs[i].scene.select_entity(*handle, true);
            }
            self.tabs[i].dirty = true;
            self.command_line.push_output(
                crate::tf!(
                    "REVOLVE: created %{created} object(s); %{failed} source(s) could not be revolved.",
                    created = created_handles.len(),
                    failed = failed
                )
                .as_ref(),
            );
        } else {
            self.command_line.push_error(
                crate::t!("REVOLVE: no selected object could be revolved with the requested options.")
                    .as_ref(),
            );
        }
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
        None
    }

    pub(super) fn handle_sweep_entities(&mut self, result: CmdResult) {
        let i = self.active_tab;
        let CmdResult::SweepEntities {
            handles,
            path_handle,
            mode,
            options,
            color: _,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::command::ExtrudeMode;
        use crate::modules::insert::solid3d_cmds::empty_solid3d;
        use crate::scene::model::sweep_model;
        let delete_objects = self.delete_objects;
        let delete_profiles = delete_objects != 0;
        let path = self.tabs[i].scene.document.get_entity(path_handle).cloned();
        let mut profiles = Vec::new();
        let mut failed = 0usize;
        for handle in handles {
            if handle == path_handle || self.reject_locked_edit(i, handle) {
                failed += 1;
                continue;
            }
            if let Some(entity) = self.tabs[i].scene.document.get_entity(handle).cloned() {
                profiles.push((handle, entity));
            } else {
                failed += 1;
            }
        }
        let selection = profiles
            .iter()
            .map(|(_, entity)| entity.clone())
            .collect::<Vec<_>>();
        let options = sweep_model::sweep_selection_options(&selection, options);
        let pending = if profiles.is_empty() {
            None
        } else {
            self.begin_undo(i, "SWEEP", profiles.len(), true)
        };
        let mut created_handles = Vec::new();
        let mut consumed = Vec::new();
        for (handle, profile) in profiles {
            let result = path.as_ref().zip(options).and_then(|(path, options)| {
                let record = sweep_model::sweep_record(&profile, path, options)?;
                let (_, _, closed) = cadkernel::acis::sweep_profile_geometry(
                    record.sweep_entity.as_ref()?,
                    record.sweep_entity_transform,
                )
                .ok()?;
                let surface = mode == ExtrudeMode::Surface || !closed;
                let body =
                    cadkernel::acis::rebuild_sweep_with_mode(&record, surface).ok()?;
                Some((body, record, surface))
            });
            let Some((body, record, surface)) = result else {
                failed += 1;
                continue;
            };
            let deletes_path =
                crate::app::delobj_deletes_auxiliary(delete_objects, surface);
            if deletes_path && self.tabs[i].scene.is_layer_locked(path_handle) {
                failed += 1;
                continue;
            }
            let created = if surface {
                self.add_surface_model(sweep_model::swept_surface_entity(&record), body)
            } else {
                self.add_solid_model(
                    empty_solid3d(),
                    body,
                    acadrust::objects::SolidHistoryOperation::Sweep(record),
                )
            };
            if created.is_null() {
                failed += 1;
            } else {
                created_handles.push(created);
                if delete_profiles {
                    consumed.push(handle);
                }
                if deletes_path {
                    consumed.push(path_handle);
                }
            }
        }
        if !created_handles.is_empty() {
            consumed.sort_unstable_by_key(|handle| handle.value());
            consumed.dedup();
            self.tabs[i].scene.erase_entities(&consumed);
            self.tabs[i].scene.deselect_all();
            for handle in &created_handles {
                self.tabs[i].scene.select_entity(*handle, true);
            }
            self.tabs[i].dirty = true;
            self.command_line.push_output(crate::tf!(
                "SWEEP: created {created} object(s); {failed} source(s) could not be swept.",
                created = created_handles.len(), failed = failed
            ).as_ref());
        } else {
            self.command_line.push_error(
                crate::t!("SWEEP: could not sweep the profile along the path.").as_ref(),
            );
        }
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
    }

    pub(super) fn handle_loft_entities(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::LoftEntities {
            sections,
            guides,
            path,
            mode,
            options,
            color: _,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::command::LoftSectionSelection;
        use crate::modules::insert::solid3d_cmds::empty_solid3d;
        use crate::scene::model::loft_command_model;
        let mut sources = sections
            .iter()
            .flat_map(|section| match section {
                LoftSectionSelection::Entity(handle) => vec![*handle],
                LoftSectionSelection::Join(handles) => handles.clone(),
                LoftSectionSelection::Point(_) => Vec::new(),
            })
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|handle| handle.value());
        sources.dedup();
        let delete_objects = self.delete_objects;
        let delete_sections = delete_objects != 0;
        let section_locked = delete_sections
            && sources
                .iter()
                .any(|handle| self.tabs[i].scene.is_layer_locked(*handle));
        let available = sources
            .iter()
            .chain(guides.iter())
            .copied()
            .chain(path)
            .filter_map(|handle| {
                self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .cloned()
                    .map(|entity| (handle, entity))
            })
            .collect::<Vec<_>>();
        let result = if section_locked {
            Err("LOFT: a source is on a locked layer; disable source deletion or unlock it.".to_string())
        } else {
            loft_command_model::record(&sections, &guides, path, &available, mode, options)
                .and_then(|record| {
                    cadkernel::acis::rebuild_loft_with_options(&record)
                        .map(|body| (body, record))
                })
        };
        match result {
            Ok((body, record)) => {
                let surface = record
                    .parameters
                    .as_ref()
                    .is_some_and(|settings| settings.surface);
                let delete_auxiliary =
                    crate::app::delobj_deletes_auxiliary(delete_objects, surface);
                if delete_auxiliary
                    && guides
                        .iter()
                        .copied()
                        .chain(path)
                        .any(|handle| self.tabs[i].scene.is_layer_locked(handle))
                {
                    self.command_line.push_error(
                        "LOFT: a source is on a locked layer; disable source deletion or unlock it.",
                    );
                    return Some(Task::none());
                }
                let dirty_before = self.tabs[i].dirty;
                let pending = self.begin_undo(i, "LOFT", 1, true);
                let created = if surface {
                    let handle = self.add_surface_model(
                        loft_command_model::surface_entity(&record),
                        body,
                    );
                    if !handle.is_null() {
                        self.tabs[i].scene.create_solid_history(
                            handle,
                            acadrust::objects::SolidHistoryOperation::Loft(record),
                        );
                    }
                    handle
                } else {
                    self.add_solid_model(
                        empty_solid3d(),
                        body,
                        acadrust::objects::SolidHistoryOperation::Loft(record),
                    )
                };
                if created.is_null() {
                    // The creation helper already removed its provisional
                    // result. Discard its empty undo recording as well.
                    self.tabs[i].scene.take_undo_recording();
                    self.tabs[i].dirty = dirty_before;
                    self.command_line.push_error(crate::t!("LOFT could not create a complete display. The source sections were preserved.").as_ref());
                    return Some(Task::none());
                } else {
                    let mut consumed = if delete_sections {
                        sources.clone()
                    } else {
                        Vec::new()
                    };
                    if delete_auxiliary {
                        consumed.extend(guides.iter().copied());
                        consumed.extend(path);
                    }
                    consumed.sort_unstable_by_key(|handle| handle.value());
                    consumed.dedup();
                    self.tabs[i].scene.erase_entities(&consumed);
                    self.tabs[i].scene.deselect_all();
                    self.tabs[i].scene.select_entity(created, true);
                    self.tabs[i].dirty = true;
                    let message = if surface {
                        crate::t!("LOFT: surface created.")
                    } else {
                        crate::t!("LOFT: solid created.")
                    };
                    self.command_line.push_output(message.as_ref());
                }
                if let Some(pending) = pending {
                    self.commit_undo_delta(i, pending);
                }
            }
            Err(error) => {
                self.command_line.push_error(&error);
                return Some(Task::none());
            }
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
        None
    }

    pub(super) fn handle_solid_edge_blend(&mut self, result: CmdResult) -> Task<Message> {
        let i = self.active_tab;
        let CmdResult::SolidEdgeBlend {
            handle,
            edges,
            base_face,
            value,
            other_value,
            fillet,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        let task =
            self.solid_edge_blend(handle, &edges, base_face, value, other_value, fillet);
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        task
    }

    pub(super) fn handle_solid_shell(&mut self, handle: Handle, actions: Vec<crate::command::ShellFaceAction>, distance: f64) -> Task<Message> {
        let i = self.active_tab;
        let task = self.solid_shell(handle, &actions, distance);
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        task
    }

    pub(super) fn handle_solid_subtract(&mut self, bases: Vec<Handle>, cutters: Vec<Handle>, convert_meshes: bool) -> Task<Message> {
        let i = self.active_tab;
        let task = self.solid_subtract(&bases, &cutters, convert_meshes);
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        task
    }

    pub(super) fn handle_slice_entities(&mut self, targets: Vec<Handle>, plane: cadkernel::space::Plane, keep_point: Option<glam::DVec3>) -> Task<Message> {
        let i = self.active_tab;
        let task = self.solid_slice(&targets, plane, keep_point);
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        task
    }

    pub(super) fn handle_slice_surface_entities(&mut self, targets: Vec<Handle>, cutter: Box<cadkernel::brep::Body>, keep_point: Option<glam::DVec3>) -> Task<Message> {
        let i = self.active_tab;
        let task = self.solid_slice_surface(&targets, *cutter, keep_point);
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        task
    }
}
