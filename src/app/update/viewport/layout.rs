use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(crate) fn on_layout_switch(&mut self, name: String) -> Task<Message> {
        if self.tabs[self.active_tab].active_block_edit.is_some() {
            return self.request_block_edit_close(
                self.active_tab,
                crate::app::block_editor::BlockEditContinuation::Layout(name),
            );
        }
        self.on_layout_switch_inner(name, false, true)
    }

    /// Automation activation changes only view state. Interactive first-open
    /// initialization belongs to the undoable create/page-setup operations.
    pub(crate) fn on_layout_switch_view_only(&mut self, name: String) -> Task<Message> {
        self.on_layout_switch_inner(name, false, false)
    }

    pub(crate) fn on_block_edit_switch(&mut self, name: String) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            return Task::none();
        }
        let Some(target_index) = self.tabs[i]
            .block_edits
            .iter()
            .position(|session| session.block_name == name)
        else {
            self.command_line
                .push_error(crate::tf!("BEDIT: block tab \"{name}\" is not open.").as_ref());
            return Task::none();
        };
        if self.tabs[i].active_block_edit == Some(target_index) {
            return Task::none();
        }

        let cancel_task = self.cancel_active_command_for_space_change();
        let current_camera = self.tabs[i].scene.camera.borrow().clone();
        let current_ucs = self.tabs[i].active_ucs.clone();
        if let Some(active_index) = self.tabs[i].active_block_edit {
            if let Some(session) = self.tabs[i].block_edits.get_mut(active_index) {
                session.editor_camera = current_camera;
                session.editor_ucs = current_ucs;
            }
        } else {
            self.tabs[i].scene.sync_camera_to_document();
        }

        let (br_handle, editor_camera) = {
            let session = &self.tabs[i].block_edits[target_index];
            (session.br_handle, session.editor_camera.clone())
        };
        self.layout_rename_state = None;
        self.tabs[i].scene.active_viewport = None;
        self.tabs[i].scene.set_current_layout("Model".to_string());
        self.tabs[i].scene.block_edit_block = Some(br_handle);
        self.reset_block_constraint_display(i);
        self.tabs[i].active_block_edit = Some(target_index);
        *self.tabs[i].scene.camera.borrow_mut() = editor_camera;
        self.tabs[i].scene.camera_generation += 1;
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        self.tabs[i].scene.deselect_all();
        self.tabs[i].active_grip = None;
        self.grip_hover = None;
        self.grip_popup = None;
        self.visibility_popup = None;
        self.table_popup = None;
        self.tabs[i].refresh_active_ucs();
        self.tabs[i].scene.bump_geometry_no_blocks();
        self.refresh_properties();
        self.adopt_view_display(i);
        self.sync_dyn_fields();
        cancel_task
    }

    /// MVIEW's "Insert view > New" flow deliberately visits Model space to
    /// define a view and then returns to its paper layout. This is the only
    /// layout transition allowed to preserve an active command.
    pub(crate) fn on_layout_switch_preserving_command(&mut self, name: String) -> Task<Message> {
        if self.tabs[self.active_tab].active_block_edit.is_some() {
            return self.request_block_edit_close(
                self.active_tab,
                crate::app::block_editor::BlockEditContinuation::Layout(name),
            );
        }
        self.on_layout_switch_inner(name, true, true)
    }

    pub(in crate::app) fn on_layout_switch_inner(
        &mut self,
        name: String,
        preserve_active_command: bool,
        initialize_layout: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            self.command_line
                .push_info(crate::t!("Open or create a drawing to switch layouts.").as_ref());
            return Task::none();
        }
        let perf = crate::perf::enabled();
        let perf_total = Instant::now();
        let leaving_block_edit = self.tabs[i].active_block_edit.is_some();
        let perf_from = self.tabs[i]
            .active_block_edit_session()
            .map(|session| session.block_name.clone())
            .unwrap_or_else(|| self.tabs[i].scene.current_layout.clone());
        let context_changed = leaving_block_edit
            || self.tabs[i].scene.current_layout != name
            || self.tabs[i].scene.active_viewport.is_some();
        let preserve_active_command = preserve_active_command
            && self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| matches!(command.name(), "MVIEW" | "MVSETUP"));
        if context_changed && preserve_active_command {
            // MVIEW and MVSETUP keep their step data, but all host-owned
            // cursor/snap/dynamic-input state belongs to the old space.
            self.reset_space_interaction_state();
        }
        let cancel_task = if context_changed && !preserve_active_command {
            self.cancel_active_command_for_space_change()
        } else {
            Task::none()
        };
        if let Some(active_index) = self.tabs[i].active_block_edit.take() {
            let camera = self.tabs[i].scene.camera.borrow().clone();
            let editor_ucs = self.tabs[i].active_ucs.clone();
            let (return_layout, return_camera) = {
                let session = &mut self.tabs[i].block_edits[active_index];
                session.editor_camera = camera;
                session.editor_ucs = editor_ucs;
                (session.return_layout.clone(), session.return_camera.clone())
            };
            let return_layout = if self.tabs[i].scene.layout_names().contains(&return_layout) {
                return_layout
            } else {
                "Model".to_string()
            };
            self.tabs[i].scene.block_edit_block = None;
            self.tabs[i].scene.set_current_layout(return_layout);
            *self.tabs[i].scene.camera.borrow_mut() = return_camera;
            self.tabs[i].scene.camera_generation += 1;
            self.tabs[i].scene.bump_geometry_no_blocks();
        }
        let going_to_paper = name != "Model";
        // Persist the camera of the layout we're leaving BEFORE switching
        // so returning to it restores where the user left off (the
        // periodic sync only fires on a tick, which may not have run
        // since the last pan/zoom).
        let perf_phase = Instant::now();
        self.tabs[i].scene.sync_camera_to_document();
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        let sync_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // Cancel any pending rename and active viewport when switching.
        self.layout_rename_state = None;
        self.tabs[i].scene.active_viewport = None;
        let perf_phase = Instant::now();
        self.tabs[i].scene.set_current_layout(name.clone());
        self.tabs[i].scene.deselect_all();
        let switch_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // UCS follows the active model, layout, or floating viewport pane.
        let perf_phase = Instant::now();
        self.tabs[i].refresh_active_ucs();
        if initialize_layout {
            self.tabs[i].scene.restore_saved_camera();
        } else {
            self.tabs[i].scene.restore_saved_camera_view_only();
        }
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        // `deselect_all` invalidates the scene highlight, but grips and the
        // Properties panel are separate caches owned by the app.
        self.refresh_properties();
        // Grid/snap are per-view: load the layout we just entered (its
        // sheet viewport in paper space, the model tile in model space)
        // so model and each layout keep independent grid state.
        self.adopt_view_display(i);
        let restore_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // Paper-space tools live in the right-edge side toolbar now, so
        // entering/leaving a layout no longer hijacks the ribbon tab.
        let _ = going_to_paper;
        // Refresh VP freeze columns for the new layout.
        let perf_phase = Instant::now();
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        // A walk in flight belongs to the space on screen, so it moves with
        // the layout instead of silently hiding a drawing the user left.
        self.walk_space_changed();
        let layers_ms = perf_phase.elapsed().as_secs_f64() * 1000.0;
        // Give the layout rebuild a visible notice first.
        self.layout_settling = true;
        if perf {
            crate::perf_record!(
                "[perf] layout-switch from={} to={} total={:.2}ms sync={:.2}ms switch={:.2}ms restore={:.2}ms layers={:.2}ms epoch={}",
                perf_from,
                name,
                perf_total.elapsed().as_secs_f64() * 1000.0,
                sync_ms,
                switch_ms,
                restore_ms,
                layers_ms,
                self.tabs[i].scene.geometry_epoch,
            );
        }
        if context_changed {
            self.sync_dyn_fields();
        }
        cancel_task
    }

    pub(in crate::app) fn on_layout_create(&mut self) -> Task<Message> {
        self.create_layout_named(None)
    }

    /// The next free `LayoutN` name, counting from the number of layouts.
    pub(in crate::app) fn next_layout_name(existing: &[String]) -> String {
        (existing.len()..)
            .map(|n| format!("Layout{n}"))
            .find(|candidate| !existing.contains(candidate))
            .unwrap_or_default()
    }

    /// A new layout called `name`, or the next free `LayoutN`.
    pub(in crate::app) fn create_layout_named(&mut self, name: Option<String>) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            self.command_line
                .push_info(crate::t!("Open or create a drawing to add a layout.").as_ref());
            return Task::none();
        }
        // Find a unique name (e.g. Layout2, Layout3, ...).
        let existing = self.tabs[i].scene.layout_names();
        if let Some(name) = name.as_ref().filter(|name| {
            existing
                .iter()
                .any(|layout| layout.eq_ignore_ascii_case(name))
        }) {
            self.command_line
                .push_error(crate::tf!("\"{}\" name already in use", name).as_ref());
            return Task::none();
        }
        let cancel_task = self.cancel_active_command_for_space_change();
        let new_name = name.unwrap_or_else(|| Self::next_layout_name(&existing));
        self.push_undo_snapshot(i, "LAYOUT");
        match self.tabs[i].scene.add_layout(&new_name) {
            Ok(_) => {
                let layout_flags = i16::from(
                    self.tabs[i]
                        .scene
                        .document
                        .header
                        .paper_space_linetype_scaling,
                ) | (i16::from(
                    self.tabs[i].scene.document.header.paper_space_limit_check,
                ) << 1);
                let plot_style = self
                    .active_plot_style
                    .as_ref()
                    .map(|style| style.name.clone())
                    .unwrap_or_default();
                self.tabs[i].scene.new_layout_plot_style = plot_style.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let document::objects::ObjectType::Layout(l) = obj {
                        if l.name == new_name {
                            l.flags = layout_flags;
                            crate::scene::apply_default_page_setup(l, &plot_style);
                            break;
                        }
                    }
                }
                // Safety net — `add_layout` already creates the overall
                // sheet viewport; this covers any path that doesn't.
                self.tabs[i].scene.ensure_sheet_viewport(&new_name);
                let switch_task = self.on_layout_switch(new_name.clone());
                self.tabs[i].scene.fit_all();
                // Opened, it already has its first viewport unless
                // LAYOUTCREATEVIEWPORT is off.
                if crate::scene::layout_create_viewport() {
                    let name = new_name.clone();
                    self.command_line
                        .push_output(crate::tf!("Layout '{name}' created.").as_ref());
                } else {
                    self.command_line.push_output(
                        crate::tf!("Layout \"{new_name}\" created — use MVIEW to add a viewport")
                            .as_ref(),
                    );
                }
                // The option that greets every new layout with its page setup.
                let switch_task = if self.plot_dialog.page_setup_on_new_layout {
                    Task::batch([switch_task, self.on_plot_dialog_open()])
                } else {
                    switch_task
                };
                self.tabs[i].dirty = true;
                return Task::batch([cancel_task, switch_task]);
            }
            Err(e) => self
                .command_line
                .push_error(crate::tf!("Failed to create layout: {e}").as_ref()),
        }
        cancel_task
    }

    /// Copy layout `source` as "source (2)": its page setup and every
    /// paper-space object, viewports included. The copy opens.
    pub(in crate::app) fn on_layout_copy(&mut self, source: String) -> Task<Message> {
        self.copy_layout_named(source, None)
    }

    /// Copy layout `source` as `name`, or as the next free "source (n)".
    pub(in crate::app) fn copy_layout_named(
        &mut self,
        source: String,
        name: Option<String>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start || source == "Model" {
            return Task::none();
        }
        let existing = self.tabs[i].scene.layout_names();
        if let Some(name) = name.as_ref().filter(|name| existing.contains(name)) {
            self.command_line
                .push_error(crate::tf!("\"{}\" name already in use", name).as_ref());
            return Task::none();
        }
        let Some(name) = name.or_else(|| {
            (2..)
                .map(|n| format!("{source} ({n})"))
                .find(|candidate| !existing.contains(candidate))
        }) else {
            return Task::none();
        };
        let cancel_task = self.cancel_active_command_for_space_change();
        self.push_undo_snapshot(i, "LAYOUT");
        let scene = &mut self.tabs[i].scene;
        if let Err(e) = scene.add_layout(&name) {
            self.command_line
                .push_error(crate::tf!("Failed to create layout: {e}").as_ref());
            return cancel_task;
        }
        scene.copy_layout_page(&source, &name);
        let handles = scene.layout_paper_entities(&source);
        let (entities, deps) = crate::app::copy_to_clipboard_kernel(&scene.document, &handles);
        crate::app::paste_entities_kernel(
            &mut scene.document,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Layout(&name),
        );
        scene.populate_meshes_from_document();
        scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.command_line
            .push_output(crate::tf!("Layout \"{source}\" copied as \"{name}\"").as_ref());
        let switch_task = self.on_layout_switch(name);
        Task::batch([cancel_task, switch_task])
    }

    pub(in crate::app) fn on_layout_rename_commit(&mut self) -> Task<Message> {
        if let Some((orig, new_name)) = self.layout_rename_state.take() {
            let new_name = new_name.trim().to_string();
            if !new_name.is_empty() && new_name != orig {
                let i = self.active_tab;
                // BEDIT block tab: renaming it renames the BLOCK itself —
                // its record, marker and every INSERT reference. (#261)
                let is_block_tab = self.tabs[i]
                    .active_block_edit_session()
                    .is_some_and(|session| session.block_name == orig);
                if is_block_tab {
                    if self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .get(&new_name)
                        .is_some()
                    {
                        self.command_line.push_error(
                            crate::tf!("\"{}\" name already in use", new_name).as_ref(),
                        );
                    } else {
                        self.push_undo_snapshot(i, "BLOCK RENAME");
                        if self.tabs[i].scene.rename_block(&orig, &new_name) {
                            if let Some(session) = self.tabs[i].active_block_edit_session_mut() {
                                session.block_name = new_name.clone();
                            }
                            for session in &mut self.tabs[i].block_edits {
                                if session.return_block.as_deref() == Some(orig.as_str()) {
                                    session.return_block = Some(new_name.clone());
                                }
                            }
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(
                                crate::tf!("Block \"{orig}\" → \"{new_name}\"").as_ref(),
                            );
                        } else {
                            self.command_line.push_error(
                                crate::tf!("Could not rename block \"{orig}\"").as_ref(),
                            );
                        }
                    }
                    return Task::none();
                }
                let exists = self.tabs[i].scene.layout_names().contains(&new_name);
                if exists {
                    self.command_line
                        .push_error(crate::tf!("\"{}\" name already in use", new_name).as_ref());
                } else {
                    self.push_undo_snapshot(i, "LAYOUT RENAME");
                    self.tabs[i].scene.rename_layout(&orig, &new_name);
                    if self.tabs[i].scene.current_layout == orig {
                        self.tabs[i].scene.set_current_layout(new_name.clone());
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("Layout \"{orig}\" → \"{new_name}\"").as_ref());
                }
            }
        }
        Task::none()
    }

    /// Commit the active grip edit at its current (already applied) position:
    /// one undoable group per gesture, every edited entity back into the
    /// resident tessellation. With the grip-menu Copy toggle on, the
    /// originals are put back and the moved shapes are added as new
    /// entities, and the grip stays hot for the next copy.
    pub(in crate::app) fn commit_active_grip_edit(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(grip) = self.tabs[i].active_grip.clone() else {
            return Task::none();
        };
        if grip.last_world == grip.origin_world {
            // Nothing placed yet — the grip simply stays hot.
            return Task::none();
        }
        if self.tabs[i].grip_copy {
            return self.commit_active_grip_edit_as_copy(grip);
        }
        self.tabs[i].grip_base_pending = false;
        if matches!(
            grip.mode,
            GripEditMode::Lengthen
                | GripEditMode::Radius
                | GripEditMode::ArcLength
                | GripEditMode::RectangleWidth
                | GripEditMode::RectangleHeight
                | GripEditMode::MoveParallel
        ) {
            self.grip_pending = None;
            self.command_line.input.clear();
        }
        let added_vertex_focus = grip.targets.iter().find_map(|target| {
            let original = self
                .grip_originals
                .iter()
                .find(|(handle, _)| *handle == target.handle)
                .map(|(_, entity)| entity)?;
            let current = self.tabs[i].scene.document.get_entity(target.handle)?;
            is_added_polyline_vertex(original, current, target.grip_id).then_some(target.grip_id)
        });
        // Keep originals available until every history shape has a valid
        // final display. A rejected rebuild cancels the entire gesture.
        let history_handles: Vec<_> = self
            .grip_preview_handles
            .iter()
            .copied()
            .filter(|handle| {
                self.tabs[i]
                    .scene
                    .document
                    .solid_history_operation(*handle)
                    .is_some()
            })
            .collect();
        for handle in history_handles {
            if !self.tabs[i].scene.finalize_solid_history(handle) {
                self.cancel_active_grip_edit();
                self.command_line.push_error(crate::t!("The edited shape could not be displayed; the original geometry was restored.").as_ref());
                self.refresh_properties();
                return Task::none();
            }
        }
        self.tabs[i].active_grip = None;
        // Commit the grip drag as one undoable group, then put every
        // edited entity back into the resident tessellation.
        let handles = std::mem::take(&mut self.grip_preview_handles);
        let originals = std::mem::take(&mut self.grip_originals);
        let history_originals = std::mem::take(&mut self.grip_history_originals);
        let dirty_before = self.grip_dirty_before.take().unwrap_or(self.tabs[i].dirty);
        if !handles.is_empty() {
            self.grip_reference_wires.clear();
            self.grip_text_verts = Vec::new();
            self.grip_text_slide = false;
            for &handle in &handles {
                self.tabs[i].scene.preview_hidden.remove(&handle);
            }
            self.tabs[i].scene.clear_preview_wire();
            let changes: Vec<_> = handles
                .into_iter()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            let rebuilds_table = changes.iter().any(|(handle, _)| {
                matches!(self.tabs[i].scene.document.get_entity(*handle),
                    Some(document::EntityType::Table(table)) if table.block_record_handle.is_none())
            });
            if rebuilds_table {
                // Picture rebuilds replace block members, possibly adding a
                // private block. Record structure and removed/added entities,
                // starting with the drag originals, as one history entry.
                let mut edited = Vec::new();
                for (handle, original) in &originals {
                    if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(*handle) {
                        edited.push((*handle, std::mem::replace(entity, original.clone())));
                    }
                }
                let mut edited_objects = Vec::new();
                for (handle, original) in history_originals.iter().flat_map(|(_, objects)| objects)
                {
                    if let Some(object) = self.tabs[i].scene.document.objects.get_mut(handle) {
                        edited_objects.push((*handle, std::mem::replace(object, original.clone())));
                    }
                }
                self.tabs[i].dirty = dirty_before;
                self.push_undo_snapshot(i, "GRIP");
                for (handle, entity) in edited {
                    *self.tabs[i].scene.document.get_entity_mut(handle).unwrap() = entity;
                }
                for (handle, object) in edited_objects {
                    self.tabs[i].scene.document.objects.insert(handle, object);
                }
                // A fill copies formulas and fields as fields, inside the
                // same history entry.
                if let Some((table, pairs)) = self.tabs[i].scene.table_fill_fields.take() {
                    let filled = grip.targets.iter().any(|target| {
                        target.handle == table
                            && target.grip_id == crate::entities::table::CELL_GRIP_FILL
                    });
                    if filled {
                        crate::entities::table::commit_fill_fields(
                            &mut self.tabs[i].scene.document,
                            table,
                            &pairs,
                        );
                    }
                }
            }
            // Dimensions that follow the edited shapes update here; record
            // them so the one undo also brings back their saved pictures.
            self.tabs[i].scene.begin_undo_recording();
            self.tabs[i]
                .scene
                .bump_entities_after_parametric_solve(&changes);
            let (followers, follower_objects, ..) = self.tabs[i]
                .scene
                .take_undo_recording()
                .map(|recording| recording.into_recorded_images())
                .unwrap_or_default();
            // A grip that detaches a centre object says so, as moving it does.
            for (handle, original) in &originals {
                let document::EntityType::Line(line) = original else {
                    continue;
                };
                let data = &line.common.extended_data;
                let was_associated = document::entities::CenterMarkAssociation::read(data)
                    .map(|association| association.associated)
                    .or_else(|| {
                        document::entities::CenterLineAssociation::read(data)
                            .map(|association| association.associated)
                    });
                if was_associated == Some(true) {
                    if let Some((mark, false)) = self.tabs[i].scene.center_object_state(*handle) {
                        self.command_line.push_output(&if mark {
                            crate::tr!("center-cmd", "mark-disassociated")
                        } else {
                            crate::tr!("center-cmd", "line-disassociated")
                        });
                    }
                }
            }
            if !originals.is_empty() {
                let mut before: Vec<_> = originals
                    .into_iter()
                    .map(|(handle, entity)| (handle, std::sync::Arc::new(entity)))
                    .collect();
                for (handle, entity) in followers {
                    if let Some(entity) = entity {
                        if !before.iter().any(|(known, _)| *known == handle) {
                            before.push((handle, entity));
                        }
                    }
                }
                let mut object_before: Vec<_> = history_originals
                    .into_iter()
                    .flat_map(|(_, objects)| objects)
                    .collect();
                for (handle, object) in follower_objects {
                    if let Some(object) = object {
                        if !object_before.iter().any(|(known, _)| *known == handle) {
                            object_before.push((handle, object));
                        }
                    }
                }
                if rebuilds_table {
                    self.tabs[i].dirty = true;
                    self.finish_pending_history(i);
                } else {
                    self.push_entity_group_history(i, "GRIP", before, object_before, dirty_before);
                }
                self.tabs[i].dirty = true;
            }
        }
        // Placement confirmed — keep the just-added leader.
        self.grip_add_provisional = None;
        self.grip_ctrl_cycle = None;
        self.tabs[i].snap_result = None;
        if let Some(vertex_id) = added_vertex_focus {
            self.tabs[i].properties.prop_vertex = vertex_id;
            self.tabs[i].properties.prop_vertex_indicator_active = true;
            crate::scene::view::dispatch::set_prop_current_vertex(vertex_id);
        }
        self.refresh_properties();
        Task::none()
    }

    /// Grip-menu Copy: leave the originals where they were and add the
    /// edited shapes as new entities, then re-arm the same grip at its
    /// origin so the next placement makes another copy (grip Copy, as in commercial solutions).
    pub(in crate::app) fn commit_active_grip_edit_as_copy(&mut self, grip: GripEdit) -> Task<Message> {
        let i = self.active_tab;
        let handles = std::mem::take(&mut self.grip_preview_handles);
        let originals = std::mem::take(&mut self.grip_originals);
        let context_originals = std::mem::take(&mut self.grip_history_originals);
        self.grip_dirty_before = None;
        self.grip_add_provisional = None;
        self.grip_reference_wires.clear();
        self.grip_text_verts = Vec::new();
        self.grip_text_slide = false;
        for &handle in &handles {
            self.tabs[i].scene.preview_hidden.remove(&handle);
        }
        self.tabs[i].scene.clear_preview_wire();
        if !originals.is_empty() {
            self.push_undo_snapshot(i, "GRIP");
            // Copy while the moved placement and its annotation leaves are
            // still live. The normal COPY path clones each attribute's own
            // context dictionary onto fresh handles.
            let source_handles: Vec<_> = originals.iter().map(|(handle, _)| *handle).collect();
            self.tabs[i].scene.copy_entities(
                &source_handles,
                &crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
            );
            for (_, objects) in context_originals {
                for (handle, object) in objects {
                    self.tabs[i].scene.document.objects.insert(handle, object);
                }
            }
            let mut restored = Vec::with_capacity(originals.len());
            for (handle, original) in &originals {
                if self.tabs[i].scene.update_entity(original.clone()) {
                    restored.push((*handle, crate::scene::ChangeKind::Modified));
                }
            }
            self.tabs[i]
                .scene
                .bump_entities_after_parametric_solve(&restored);
            self.finish_pending_history(i);
            self.tabs[i].dirty = true;
        }
        self.tabs[i].snap_result = None;
        // Re-arm: the grip sits at its origin again over the restored shape.
        let mut again = grip;
        let delta = again.last_world - again.origin_world;
        again.last_world = again.origin_world;
        for target in &mut again.targets {
            target.last_world -= delta;
        }
        self.tabs[i].active_grip = Some(again);
        self.refresh_properties();
        Task::none()
    }
}
