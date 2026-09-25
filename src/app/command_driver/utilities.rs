use super::*;

impl OpenCADStudio {
    pub(in crate::app) fn reject_locked_edit(&mut self, i: usize, handle: Handle) -> bool {
        let Some(layer) = self.tabs[i].scene.locked_layer_name(handle) else {
            return false;
        };
        self.command_line.push_info(
            crate::tf!("Object is on locked layer \"{layer}\" — unlock the layer to edit it.")
                .as_ref(),
        );
        true
    }

    pub(in crate::app) fn refresh_area_preview(&mut self, i: usize) {
        let hatches = self.tabs[i]
            .active_cmd
            .as_ref()
            .and_then(|command| command.hatch_preview_models());
        if let Some(hatches) = hatches {
            self.tabs[i].scene.set_command_preview_hatches(hatches);
            return;
        }
        let regions = self.tabs[i]
            .active_cmd
            .as_ref()
            .and_then(|command| command.area_preview_regions());
        if let Some(regions) = regions {
            self.tabs[i].scene.set_area_preview_regions(&regions);
        }
    }

    /// Point supplied by a bare Enter before LINE/PLINE's first click. Prefer
    /// the current endpoint of the most recently created path drawable in the
    /// active space. A loaded drawing has no runtime anchor, so recover its
    /// newest line/arc/polyline endpoint. An empty space starts at the active
    /// UCS origin (0,0,0 in user coordinates).
    fn default_draw_start(&self, i: usize) -> glam::DVec3 {
        let tab = &self.tabs[i];
        let endpoint = |entity: &codec::EntityType| {
            let last_grip = match entity {
                codec::EntityType::Line(line) => {
                    return Some(glam::DVec3::new(line.end.x, line.end.y, line.end.z));
                }
                codec::EntityType::Arc(_) => Some(2),
                codec::EntityType::LwPolyline(polyline) => {
                    polyline.vertices.len().checked_sub(1)
                }
                codec::EntityType::Polyline(polyline) => polyline.vertices.len().checked_sub(1),
                codec::EntityType::Polyline2D(polyline) => {
                    polyline.vertices.len().checked_sub(1)
                }
                codec::EntityType::Polyline3D(polyline) => {
                    polyline.vertices.len().checked_sub(1)
                }
                _ => None,
            }?;
            crate::scene::view::dispatch::grips(entity)
                .into_iter()
                .find(|grip| grip.id == last_grip)
                .map(|grip| grip.world)
        };

        if let Some(handle) = tab.last_draw_anchor {
            if tab.scene.entity_belongs_to_active_space(handle) {
                if let Some(point) = tab.scene.document.get_entity(handle).and_then(endpoint) {
                    return point;
                }
            }
        }

        let recovered = tab
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                let handle = entity.common().handle;
                tab.scene
                    .entity_belongs_to_active_space(handle)
                    .then(|| endpoint(entity).map(|point| (handle, point)))
                    .flatten()
            })
            .max_by_key(|(handle, _)| handle.value())
            .map(|(_, point)| point);

        recovered.unwrap_or_else(|| tab.ucs_origin_world())
    }

    /// Drop cursor-relative state that was computed in the drawing space being
    /// left. This is also used by MVIEW, whose command object survives its
    /// intentional paper/model round-trip while its old-space overlays cannot.
    pub(in crate::app) fn reset_space_interaction_state(&mut self) {
        let i = self.active_tab;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].snap_result = None;
        self.last_point = None;
        // Points collected in the space being left are meaningless in the new
        // one.
        self.clear_accepted_snaps();
        self.pending_click_snap = None;
        self.snapper.from_point = None;
        self.snapper.clear_tracking();
        self.otrack_active = None;
        self.otrack_cross = None;
        self.otrack_kind = None;
        self.axis_lock_dir = None;
        self.dyn_user_reshaped = false;
        self.dyn_coord_absolute = false;
        self.grip_hover = None;
        self.grip_popup = None;
        self.grip_pending = None;
        self.visibility_popup = None;
        self.hover_dwell = None;
        self.ucs_grip_drag = None;
        self.ucs_icon_selected = false;
        self.ucs_icon_hover = false;
        self.tabs[i].pan_mode = false;
        self.tabs[i].orbit_mode = false;
        self.tabs[i].zoom_dynamic_mode = false;
        let _ = self.on_viewport_exit();
    }

    pub(in crate::app) fn solve_grip_constraints(
        &mut self,
        i: usize,
        grip: &crate::scene::pick::grip::GripEdit,
    ) {
        let touched: Vec<_> = grip.targets.iter().map(|target| target.handle).collect();
        let connected = self.tabs[i].scene.parametric_connected_handles(
            self.tabs[i].current_parametric_scope(), &touched, true,
        );
        for handle in connected {
            if !self.grip_originals.iter().any(|(original, _)| *original == handle) {
                if let Some(entity) = self.tabs[i].scene.document.get_entity(handle).cloned() {
                    self.grip_originals.push((handle, entity));
                }
            }
            if !self.grip_preview_handles.contains(&handle) {
                self.grip_preview_handles.push(handle);
                if !self.tabs[i].scene.meshes.contains_key(&handle) {
                    self.tabs[i].scene.preview_hidden.insert(handle);
                }
            }
        }
        let driven_refs: Vec<_> = grip.targets.iter().flat_map(|target| {
            self.tabs[i].scene.document.get_entity(target.handle)
                .map(|entity| crate::scene::parametric_constraints::grip_solve_anchor_refs(
                    entity, target.handle, target.grip_id,
                )).unwrap_or_default()
        }).collect();
        let retain_size = self.constraint_solve_mode
            && !driven_refs.is_empty()
            && driven_refs.iter().all(|reference| reference.marker.is_some());
        let solved = self.tabs[i].scene.solve_parametric_constraints_preview(
            &touched,
            &driven_refs,
            retain_size,
            &self.grip_originals,
        );
        for (handle, entity) in solved {
            if let Some(slot) = self.tabs[i].scene.document.get_entity_mut(handle) {
                *slot = entity;
            }
        }
    }

    pub(in crate::app) fn capture_grip_history_originals(&mut self, i: usize, handles: &[Handle]) {
        if !self.grip_history_originals.is_empty() {
            return;
        }
        self.grip_history_originals = handles
            .iter()
            .filter_map(|&handle| {
                let objects = self.tabs[i].scene.solid_history_objects(handle);
                (!objects.is_empty()).then_some((handle, objects))
            })
            .collect();
    }

    pub(in crate::app) fn cancel_active_grip_edit(&mut self) -> bool {
        let i = self.active_tab;
        // Grip-menu toggles live only as long as the gesture.
        self.tabs[i].grip_copy = false;
        self.tabs[i].grip_base_pending = false;
        let had_grip = self.tabs[i].active_grip.take().is_some()
            || self.grip_add_provisional.is_some()
            || !self.grip_preview_handles.is_empty()
            || !self.grip_history_originals.is_empty();
        if !had_grip {
            return false;
        }

        // An Add-Leader arrow being placed is still provisional.
        if let Some((handle, grip_id)) = self.grip_add_provisional.take() {
            use crate::entities::traits::EntityTypeOps;
            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                entity.apply_grip_menu(
                    grip_id,
                    crate::scene::model::object::GripMenuAction::RemoveLeader,
                );
            }
            self.tabs[i]
                .scene
                .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
        }

        let handles = std::mem::take(&mut self.grip_preview_handles);
        let originals = std::mem::take(&mut self.grip_originals);
        let history_originals = std::mem::take(&mut self.grip_history_originals);
        let history_handles: rustc_hash::FxHashSet<_> = history_originals
            .iter()
            .map(|(handle, _)| *handle)
            .collect();
        for (_, objects) in history_originals {
            for (object_handle, object) in objects {
                self.tabs[i]
                    .scene
                    .document
                    .objects
                    .insert(object_handle, object);
            }
        }
        let mut changed_handles: rustc_hash::FxHashSet<_> = handles.iter().copied().collect();
        for (handle, original) in originals {
            changed_handles.insert(handle);
            if !history_handles.contains(&handle) {
                let current = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .and_then(crate::entities::solid3d::point_of_reference)
                    .map(|point| [point.x, point.y, point.z]);
                let target = crate::entities::solid3d::point_of_reference(&original)
                    .map(|point| [point.x, point.y, point.z]);
                if let (Some(current), Some(target)) = (current, target) {
                    self.tabs[i].scene.translate_solid_geometry(
                        handle,
                        [
                            target[0] - current[0],
                            target[1] - current[1],
                            target[2] - current[2],
                        ],
                    );
                }
            }
            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                *entity = original;
            }
        }
        for handle in history_handles {
            let restored = self.tabs[i]
                .scene
                .document
                .solid_history_operation(handle)
                .cloned()
                .is_some_and(|operation| {
                    self.tabs[i].scene.rebuild_solid_history(handle, operation)
                });
            if !restored {
                self.tabs[i].scene.reseed_derived_caches(handle);
            }
        }
        for &handle in &handles {
            self.tabs[i].scene.preview_hidden.remove(&handle);
        }
        let changes: Vec<_> = changed_handles
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities_after_parametric_solve(&changes);
        if let Some(dirty_before) = self.grip_dirty_before.take() {
            self.tabs[i].dirty = dirty_before;
        }

        self.grip_reference_wires.clear();
        self.grip_text_verts.clear();
        self.grip_text_slide = false;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].snap_result = None;
        self.refresh_selected_grips();
        self.refresh_properties();
        true
    }

    /// End an interactive command before its drawing coordinate context
    /// changes. This is a full interaction boundary, not only an `active_cmd`
    /// check: grip drags, suspended editor commands, snaps, tracking, dynamic
    /// input and pointer gestures all carry coordinates from the old space.
    pub(in crate::app) fn cancel_active_command_for_space_change(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let mut tasks = Vec::new();
        let mut cancellation_reported = false;

        if let Some(result) = self.tabs[i]
            .active_cmd
            .as_mut()
            .map(|command| command.on_space_change())
        {
            let (result, message_pending) = match result {
                CmdResult::Cancel => (CmdResult::CancelForSpaceChange, false),
                other => (other, true),
            };
            tasks.push(self.apply_cmd_result(result));

            // `on_space_change` must be terminal. Force a plain cancellation
            // if an external/plugin command violates that contract.
            if self.tabs[i].active_cmd.is_some() {
                tasks.push(self.apply_cmd_result(CmdResult::CancelForSpaceChange));
            } else if message_pending {
                self.command_line.push_info(
                    crate::t!("Command cancelled because the active drawing space changed.")
                        .as_ref(),
                );
            }
            cancellation_reported = true;
        }

        let grip_cancelled = self.cancel_active_grip_edit();
        let suspended_cancelled = self.tabs[i].suspended_cmd.take().is_some();
        let editor_cancelled = self.text_inline.is_some() || self.mtext_editor.is_some();
        if editor_cancelled {
            self.text_inline_cancel();
            self.mtext_cancel();
        }
        if !cancellation_reported && (grip_cancelled || suspended_cancelled || editor_cancelled) {
            self.command_line.push_info(
                crate::t!("Command cancelled because the active drawing space changed.").as_ref(),
            );
        }

        self.command_line.input.clear();
        self.command_line.autocomplete_cursor = None;
        self.command_line.close_history();
        self.reset_space_interaction_state();
        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }

    /// Apply LIMCHECK/PLIMCHECK to a point before an interactive command
    /// consumes it. LIMITS itself must be able to redefine a rectangle beyond
    /// the old boundary, so it is the sole bypass.
    pub(in crate::app) fn command_point_allowed(&mut self, i: usize, point: glam::DVec3) -> bool {
        let checks_limits = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.name() != "LIMITS")
            && self.tabs[i].scene.drawing_limit_check_enabled();
        if checks_limits && !self.tabs[i].scene.point_inside_drawing_limits(point) {
            self.command_line
                .push_error(crate::t!("Outside limits.").as_ref());
            return false;
        }
        true
    }

    /// Refresh screen-space feature picking immediately before point dispatch.
    pub(in crate::app) fn refresh_command_point_pick_context(&mut self, i: usize) {
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.wants_point_pick_context())
        {
            let scene = &self.tabs[i].scene;
            let size = scene.selection.borrow().vp_size;
            let edit = scene.viewport_edit_frame(size);
            let tile = edit
                .as_ref()
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| scene.active_model_tile_bounds(size.0, size.1));
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: tile.width,
                height: tile.height,
            };
            let context = if bounds.width > 0.0 && bounds.height > 0.0 {
                let (view, eye) = if let Some((camera, _)) = edit {
                    (camera.view_proj_rte(bounds), camera.eye())
                } else {
                    let camera = scene.camera.borrow();
                    (camera.view_proj_rte(bounds), camera.eye())
                };
                Some(crate::command::PointPickContext {
                    view,
                    eye,
                    bounds,
                    aperture_px: crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                })
            } else {
                None
            };
            if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                command.set_point_pick_context(context);
            }
        }
    }

    /// Drive the active command's step machine with one [`StepInput`], then
    /// apply the result. This is the single entry point every input source
    /// (command line, headless, dynamic input, plugin API, viewport) funnels
    /// through, so the routing from input → `on_*` method → `apply_cmd_result`
    /// lives in exactly one place. No-op when no command is active.
    pub(in crate::app) fn feed_command(&mut self, input: StepInput) -> Task<Message> {
        self.feed_command_consumed(input).0
    }

    /// Keeps only associative dimensions in a completed selection when the
    /// active command asks for that, deselecting the rest and reporting how
    /// many objects were dropped.
    fn filter_associative_dimension_selection(&mut self, input: StepInput) -> StepInput {
        let StepInput::SelectionComplete(handles) = input else {
            return input;
        };
        let i = self.active_tab;
        if !self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.selection_keeps_associative_dimensions())
        {
            return StepInput::SelectionComplete(handles);
        }
        let (kept, dropped): (Vec<Handle>, Vec<Handle>) = handles.into_iter().partition(|handle| {
            // A dimension that already drives a constraint is not one to
            // convert either.
            crate::scene::parametric_constraints::dynamic_dimension_constraint(
                &self.tabs[i].scene.parametric_constraints,
                *handle,
            )
            .is_none()
                && crate::scene::dimension_assoc::constraint_from_associative_dimension(
                    &self.tabs[i].scene.document,
                    *handle,
                )
                .is_some()
        });
        for handle in &dropped {
            self.tabs[i].scene.deselect_entity(*handle);
        }
        if !dropped.is_empty() {
            let count = dropped.len();
            self.command_line.push_info(
                if count == 1 {
                    crate::t!("1 was filtered out.")
                } else {
                    crate::tf!("{count} were filtered out.")
                }
                .as_ref(),
            );
            self.refresh_properties();
        }
        StepInput::SelectionComplete(kept)
    }

    /// Keeps only block references in a completed selection when the active
    /// command asks for that (XCLIP), deselecting the rest and saying how many
    /// were ineligible.
    fn filter_block_reference_selection(&mut self, input: StepInput) -> StepInput {
        let StepInput::SelectionComplete(handles) = input else {
            return input;
        };
        let i = self.active_tab;
        if !self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.selection_keeps_block_references())
        {
            return StepInput::SelectionComplete(handles);
        }
        let (kept, dropped): (Vec<Handle>, Vec<Handle>) = handles.into_iter().partition(|h| {
            matches!(
                self.tabs[i].scene.document.get_entity(*h),
                Some(codec::EntityType::Insert(_))
            )
        });
        for handle in &dropped {
            self.tabs[i].scene.deselect_entity(*handle);
        }
        if !dropped.is_empty() {
            let count = dropped.len();
            self.command_line.push_info(&if count == 1 {
                "1 was ineligible for clipping.".to_string()
            } else {
                format!("{count} were ineligible for clipping.")
            });
            self.refresh_properties();
        }
        StepInput::SelectionComplete(kept)
    }

    /// `feed_command`, also reporting whether the step actually took the input.
    /// Only a `Text` token can come back unclaimed (`on_text_input` returning
    /// `None`), which is what lets the caller read it as something else — a
    /// typed distance, say — instead of guessing beforehand.
    pub(in crate::app) fn feed_command_consumed(&mut self, input: StepInput) -> (Task<Message>, bool) {
        // A command that takes associative dimensions only keeps just those:
        // the rest leave the selection and the count of what went is reported,
        // as the reference does while gathering.
        let input = self.filter_associative_dimension_selection(input);
        let input = self.filter_block_reference_selection(input);
        // XCLIP / CLIP learn which chosen references carry a clip already.
        {
            let i = self.active_tab;
            let tab = &mut self.tabs[i];
            if let Some(command) = tab.active_cmd.as_mut() {
                let document = &tab.scene.document;
                command.inject_clipped(&|h| {
                    crate::scene::pick::xclip::filter_handle(document, h).is_some()
                });
            }
        }
        // Selection keywords (P / PREVIOUS, L / LAST) consume the token
        // before it reaches the command (#426).
        if let StepInput::Text(s) = &input {
            let kw_input = s.clone();
            if let Some(task) = self.try_selection_keyword(&kw_input) {
                return (task, true);
            }
        }
        let i = self.active_tab;
        let default_start = matches!(&input, StepInput::Enter)
            && self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.enter_accepts_default_start());
        let input = if default_start {
            StepInput::Point(self.default_draw_start(i))
        } else {
            input
        };
        if let StepInput::Point(point) = &input {
            if !self.command_point_allowed(i, *point) {
                return (Task::none(), true);
            }
            // Typed / dynamic-input / headless points carry no snap, but the
            // accepted-snap list must stay index-parallel with the points the
            // command collects. Interactive picks record themselves in
            // the click handler and never reach here.
            if !self.record_accepted_snap(i, None, None, *point) {
                return (Task::none(), true);
            }
        }
        if default_start {
            let StepInput::Point(point) = &input else {
                unreachable!("default command start must be a point");
            };
            self.last_point = Some(*point);
            self.dyn_user_reshaped = false;
            self.dyn_coord_absolute = false;
            self.sync_dyn_fields();
            self.reset_tracking_after_point();
            self.push_ucs_to_cmd(i);
        }
        if let StepInput::EntityPick(handle, point) = &input {
            if !self.dimension_acquisition_allowed(i, None) {
                return (Task::none(), true);
            }
            let solid_pick = matches!(
                self.tabs[i].scene.document.get_entity(*handle),
                Some(
                    codec::EntityType::Solid3D(_)
                        | codec::EntityType::Surface(_)
                        | codec::EntityType::Region(_)
                        | codec::EntityType::Body(_)
                )
            );
            let direction = if solid_pick {
                self.tabs[i]
                    .scene
                    .solid_planar_face_normal_at(*handle, *point)
            } else {
                self.tabs[i]
                    .scene
                    .document
                    .get_entity(*handle)
                    .and_then(crate::entities::curve::entity_curve)
                    .and_then(|curve| curve.plane.normal())
                    .map(glam::DVec3::from_array)
            };
            if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                command.set_entity_pick_direction(direction);
            }
            if self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.inject_before_entity_pick())
            {
                if let Some(entity) = self.tabs[i].scene.document.get_entity(*handle).cloned() {
                    if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                        command.inject_picked_entity(entity);
                    }
                }
            }
        }
        if let StepInput::SelectionComplete(handles) = &input {
            let exclude_locked = self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.selection_entities_exclude_locked());
            let entities = {
                let scene = &self.tabs[i].scene;
                handles
                    .iter()
                    .filter(|handle| !exclude_locked || !scene.is_layer_locked(**handle))
                    .filter_map(|handle| {
                        scene.document.get_entity(*handle).cloned().map(|entity| {
                            let surface_area = scene
                                .meshes
                                .get(handle)
                                .or_else(|| scene.block_meshes.get(handle))
                                .map(|mesh| mesh.metrics.surface_area);
                            SelectionEntity {
                                handle: *handle,
                                entity,
                                surface_area,
                            }
                        })
                    })
                    .collect()
            };
            if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                command.inject_selection_entities(entities);
            }
        }
        if matches!(&input, StepInput::Point(_)) {
            self.refresh_command_point_pick_context(i);
        }
        let ctrl = self.ctrl_down;
        let shift = self.shift_down;
        let picked_handle = if let StepInput::EntityPick(h, _) = &input {
            Some(*h)
        } else {
            None
        };
        let is_escape = matches!(&input, StepInput::Escape);
        let result: Option<CmdResult> = {
            let Some(cmd) = self.tabs[i].active_cmd.as_mut() else {
                return (Task::none(), false);
            };
            cmd.set_ctrl(ctrl);
            cmd.set_shift(shift);
            match input {
                StepInput::Point(p) => Some(cmd.on_point(p)),
                StepInput::Text(s) => cmd.on_text_input(&s),
                StepInput::EntityPick(h, p) => Some(cmd.on_entity_pick(h, p)),
                StepInput::StructurePick(h, p) => Some(cmd.on_structure_pick(h, p)),
                StepInput::SelectionComplete(hs) => Some(cmd.on_selection_complete(hs)),
                StepInput::Tangent(o, p) => Some(cmd.on_tangent_point(o, p)),
                StepInput::EditorClosed(c) => Some(cmd.on_editor_closed(c)),
                StepInput::Enter => Some(cmd.on_enter()),
                StepInput::Escape => Some(cmd.on_escape()),
            }
        };
        if let Some(handle) = picked_handle {
            self.record_dimension_entity_points(i, None, handle, Vec::new());
        }
        self.sync_dimension_snaps(i);
        if is_escape {
            self.tabs[i].pending_pause_tokens = None;
        }
        match result {
            Some(r) => (self.apply_cmd_result(r), true),
            None => (Task::none(), false),
        }
    }

    /// Process queued tokens buffered after a PAUSE (or `\`) until the queue is
    /// empty or another PAUSE / cancellation is encountered.
    pub(in crate::app) fn drain_pending_pause_tokens(&mut self, tab_idx: usize) -> Task<Message> {
        let Some(mut tokens) = self.tabs[tab_idx].pending_pause_tokens.take() else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        while !tokens.is_empty() {
            if self.tabs[tab_idx].active_cmd.is_none() {
                break;
            }
            let token = tokens.remove(0);
            let trimmed = token.trim();
            if trimmed.eq_ignore_ascii_case("PAUSE") || trimmed == "\\" {
                self.tabs[tab_idx].pending_pause_tokens = Some(tokens);
                break;
            }
            if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("ENTER")
                || trimmed.eq_ignore_ascii_case("RETURN")
                || trimmed == "\n"
            {
                tasks.push(self.feed_command(StepInput::Enter));
            } else if trimmed.eq_ignore_ascii_case("ESC")
                || trimmed.eq_ignore_ascii_case("ESCAPE")
                || trimmed.eq_ignore_ascii_case("CANCEL")
            {
                self.tabs[tab_idx].pending_pause_tokens = None;
                tasks.push(self.feed_command(StepInput::Escape));
                break;
            } else {
                tasks.push(self.feed_active_cmd(&token));
            }
        }
        Task::batch(tasks)
    }

    /// Run one whole command-line string. A single word or an inline-argument
    /// command (`PDMODE 3`, `LAYER Walls`, `UCS Z 90` pasted as one line)
    /// dispatches as-is; for a multi-token line whose first word starts an
    /// interactive tool (`LINE 0,0 10,10`) the first word starts the tool and the
    /// remaining tokens are fed as points / option keywords, then the command is
    /// terminated as if Enter were pressed. Shared by the GUI command line and
    /// the headless automation feeder so both behave identically.
    pub(in crate::app) fn run_command_line(&mut self, cmd: &str) -> Task<Message> {
        self.run_command_line_streaming(cmd, true)
    }

    /// `run_command_line`, with the trailing Enter left off when `finish` is
    /// false: the line is one instalment and the tool stays at its next prompt
    /// for the following one. Only a plugin streaming a command feeds it that
    /// way; the command line and the automation feeder always finish.
    pub(in crate::app) fn run_command_line_streaming(&mut self, cmd: &str, finish: bool) -> Task<Message> {
        let i = self.active_tab;
        self.command_line.unconsumed.clear();
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        if tokens.len() <= 1 {
            return self.dispatch_command(cmd);
        }
        // Plugin commands parse their own inline arguments from the whole line
        // (e.g. `HC_PIPE 2B 2C 1.25 0.013`), so offer the full command to plugin
        // dispatch first. A built-in interactive tool matches only its bare name
        // (`LINE`), so the full line is not a plugin command and falls through to
        // the first-word + fed-tokens path below. (#162)
        if !self.suppress_plugin_dispatch && crate::plugin::try_dispatch(self, i, cmd) {
            let toks: Vec<String> = tokens.iter().map(|s| s.to_string()).collect();
            return self.finish_active_command(&toks, finish);
        }
        // Bare XREF opens the External References palette, so probing the
        // verb alone would open it (and refresh every entry) before the
        // argument form `XREF Reload A` runs.
        if tokens[0].eq_ignore_ascii_case("BACKGROUND")
            || tokens[0].eq_ignore_ascii_case("COLORSCHEME")
            || tokens[0].eq_ignore_ascii_case("XREF")
        {
            return self.dispatch_command(cmd);
        }
        let start_task = self.dispatch_command(tokens[0]);
        if self.tabs[i].active_cmd.is_none() {
            // Not an interactive tool — an inline-argument command (`PDMODE 3`).
            return self.dispatch_command(cmd);
        }
        let toks: Vec<String> = tokens.iter().map(|s| s.to_string()).collect();
        let finish_task = self.finish_active_command(&toks, finish);
        Task::batch([start_task, finish_task])
    }

    /// Feed `tokens[1..]` to the active interactive command as points / option
    /// keywords, then terminate it as if Enter were pressed. No-op when no
    /// command is active.
    ///
    /// Two things happen when the tokens run out mid-way, both needed so a
    /// headless caller can tell what actually happened:
    ///
    /// * If the in-place text editor took over (the `TEXT` content step — the
    ///   command itself has already ended by then, `active_cmd` is `None`), the
    ///   remaining tokens are that text: they are typed into the editor and
    ///   committed, the same messages the control surface's `text_input` /
    ///   `text_commit` actions dispatch. Without this the tail of the line was
    ///   silently dropped and `TEXT 0,0 5 0 hi` created nothing at all.
    /// * Anything still left over was never claimed by any prompt; it is
    ///   recorded in [`CommandLine::unconsumed`] instead of vanishing.
    pub(in crate::app) fn finish_active_command(&mut self, tokens: &[String], finish: bool) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_none() {
            return Task::none();
        }
        self.last_point = None;
        // An editor that is *already* open belongs to someone else (an earlier
        // line that stopped at the content step). Only the editor this line
        // opens by feeding its own tokens may be handed the tail.
        let editor_was_open = self.text_inline.is_some();
        let mut tasks = Vec::new();
        // First token is the command verb itself; prompts start consuming after it.
        let mut consumed = 1;
        let mut paused = false;
        for (idx, tok) in tokens[1..].iter().enumerate() {
            if self.tabs[i].active_cmd.is_none() {
                break;
            }
            let trimmed = tok.trim();
            if trimmed.eq_ignore_ascii_case("PAUSE") || trimmed == "\\" {
                let mut remainder: Vec<String> = tokens[1 + idx + 1..].to_vec();
                // The line still ends with an Enter; it waits behind the pause
                // instead of being dropped with it.
                if finish {
                    remainder.push("ENTER".to_string());
                }
                self.tabs[i].pending_pause_tokens = Some(remainder);
                consumed = tokens.len();
                paused = true;
                break;
            }
            tasks.push(self.feed_active_cmd(tok));
            consumed += 1;
        }
        if !editor_was_open && self.text_inline.is_some() && consumed < tokens.len() {
            // Fill the editor the same way `Message::TextInlineInput` does, then
            // commit *synchronously* so the outcome is observable: dispatching
            // `TextInlineOk` as a task would hide whether the entity was really
            // created, and the tail token would be counted as consumed either
            // way — the caller would have no way to tell "text created" from
            // "text silently dropped".
            if let Some(editor) = self.text_inline.as_mut() {
                editor.value = tokens[consumed..].join(" ");
            }
            let committed = self.text_inline_commit();
            tasks.push(self.post_editor_closed(committed));
            if committed {
                consumed = tokens.len();
                // `TEXT` repeats by design: on a committed string it re-arms for
                // the next line (`open_next_line`), which re-opens the editor. A
                // batch line is a complete unit, and leaving that repeat armed
                // would make the *next* line get eaten as the next text position,
                // so end the command here, exactly as Esc does in the GUI.
                tasks.push(Task::done(Message::CommandEscape));
            }
            // On a failed commit the tokens stay unconsumed (assigned below), so
            // the caller still sees the text it passed in.
        }
        if consumed < tokens.len() {
            self.command_line.unconsumed = tokens[consumed..].to_vec();
        }
        if !paused && finish {
            tasks.push(self.feed_command(StepInput::Enter));
        }
        Task::batch(tasks)
    }

    /// Classify one typed token into a [`StepInput`] and route it through the
    /// shared [`Self::feed_command`]. An object-pick step takes a hex handle; a
    /// coordinate is parsed (and, like the GUI command line, interpreted in the
    /// active UCS); anything else is an option keyword / value. Used by both the
    /// GUI command line and headless automation.
    /// Standard selection keywords at a "Select objects:" prompt (#426):
    /// P / PREVIOUS re-selects the set the last command worked on, and
    /// L / LAST selects the most recently created object in the current
    /// space. Returns `Some` when the token was consumed as a keyword.
    /// Handled centrally so every gathering command gets them for free.
    pub(in crate::app) fn try_selection_keyword(&mut self, text: &str) -> Option<Task<Message>> {
        let i = self.active_tab;
        let gathering = self.tabs[i]
            .active_cmd
            .as_ref()
            .map_or(false, |c| c.is_selection_gathering());
        if !gathering {
            return None;
        }
        let kw = text.trim().to_ascii_uppercase();

        // Modes that arm the next gesture rather than selecting anything now.
        // Dragging normally decides window-vs-crossing from the direction the
        // corner travels; asking for one by name has to override that, and hold
        // the override until the box is finished. (#596)
        if let "W" | "WINDOW" | "C" | "CROSSING" = kw.as_str() {
            let crossing = matches!(kw.as_str(), "C" | "CROSSING");
            {
                let mut selection = self.tabs[i].scene.selection.borrow_mut();
                selection.box_crossing = crossing;
                selection.box_crossing_locked = true;
            }
            let hint = if crossing {
                crate::t!("Crossing: specify first corner.")
            } else {
                crate::t!("Window: specify first corner.")
            };
            self.command_line.push_info(hint.as_ref());
            return Some(Task::none());
        }

        // Whether a pick adds to the set or takes away from it, for the rest of
        // this selection. Mirrors PICKADD, which the pick paths already read.
        if let "R" | "REMOVE" | "A" | "ADD" = kw.as_str() {
            let adding = matches!(kw.as_str(), "A" | "ADD");
            self.select_remove_mode = !adding;
            let hint = if adding {
                crate::t!("Add mode: picks join the selection.")
            } else {
                crate::t!("Remove mode: picks leave the selection.")
            };
            self.command_line.push_info(hint.as_ref());
            return Some(Task::none());
        }

        let add: Vec<Handle> = match kw.as_str() {
            // Every selectable object of the current space.
            "ALL" => self.tabs[i]
                .scene
                .entity_wires()
                .iter()
                .filter_map(|w| crate::scene::Scene::handle_from_wire_name(&w.name))
                .collect(),
            "P" | "PREVIOUS" => self.tabs[i]
                .prev_selection
                .iter()
                .copied()
                .filter(|&h| self.tabs[i].scene.document.get_entity(h).is_some())
                .collect(),
            // Highest handle among the selectable wires of the current space —
            // handles are handed out monotonically, so that is the most
            // recently created object.
            "L" | "LAST" => self.tabs[i]
                .scene
                .entity_wires()
                .iter()
                .filter_map(|w| crate::scene::Scene::handle_from_wire_name(&w.name))
                .max_by_key(|h| h.value())
                .into_iter()
                .collect(),
            _ => return None,
        };
        if add.is_empty() {
            self.command_line.push_info(
                crate::t!(match kw.as_str() {
                    "P" | "PREVIOUS" => "No previous selection set.",
                    "ALL" => "Nothing to select.",
                    _ => "No last object.",
                })
                .as_ref(),
            );
            return Some(Task::none());
        }
        let count = add.len();
        for h in add {
            self.tabs[i].scene.select_entity(h, false);
        }
        self.command_line
            .push_info(crate::tf!("{count} object(s) added to selection.").as_ref());
        self.refresh_properties();
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        Some(self.feed_command(StepInput::SelectionComplete(handles)))
    }

    pub(in crate::app) fn feed_active_cmd(&mut self, token: &str) -> Task<Message> {
        let i = self.active_tab;
        // Object-pick step: the token is a handle (as returned by `query`).
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.needs_entity_pick())
        {
            // Option keywords take precedence over the handle reading —
            // "F"/"C"/"E" are valid hex, but during TRIM/EXTEND they are the
            // Fence/Crossing/Edge options (#336). Only an unconsumed token
            // falls through to the handle interpretation.
            let consumed = self.tabs[i]
                .active_cmd
                .as_mut()
                .and_then(|c| c.on_text_input(token));
            if let Some(r) = consumed {
                return self.apply_cmd_result(r);
            }
            let accepts_points = self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.entity_pick_accepts_points());
            if accepts_points {
                if let Some((coord, kind)) = crate::app::helpers::parse_coord(token) {
                    let ucs = self.tabs[i].active_ucs.clone();
                    let wcs = match (
                        matches!(kind, crate::app::helpers::CoordKind::Relative),
                        self.last_point,
                    ) {
                        (true, Some(base)) => {
                            base + match &ucs {
                                Some(value) => crate::app::helpers::ucs_rotate_vec(coord, value),
                                None => coord,
                            }
                        }
                        _ => match &ucs {
                            Some(value) => crate::app::helpers::ucs_to_wcs(coord, value),
                            None => coord,
                        },
                    };
                    if self.command_point_allowed(i, wcs) {
                        self.last_point = Some(wcs);
                        self.push_ucs_to_cmd(i);
                        // A typed coordinate at an object prompt is a pick at
                        // that point, as in the reference.
                        let picks_entity = self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .is_some_and(|command| command.typed_point_picks_entity());
                        if picks_entity {
                            let owner = self.tabs[i]
                                .current_parametric_scope()
                                .owner_handle(&self.tabs[i].scene.document);
                            let handle =
                                entity_at_typed_point(&self.tabs[i].scene.document, owner, wcs)
                                    .unwrap_or(Handle::NULL);
                            return self.feed_command(StepInput::EntityPick(handle, wcs));
                        }
                        return self.feed_command(StepInput::Point(wcs));
                    }
                    return Task::none();
                }
            }
            if let Ok(v) = u64::from_str_radix(token.trim_start_matches("0x"), 16) {
                let handle = Handle::new(v);
                let pt = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .map(|e| {
                        let bb = e.as_entity().bounding_box();
                        glam::Vec3::new(
                            ((bb.min.x + bb.max.x) * 0.5) as f32,
                            ((bb.min.y + bb.max.y) * 0.5) as f32,
                            0.0,
                        )
                    })
                    .unwrap_or(glam::Vec3::ZERO);
                return self.feed_command(StepInput::EntityPick(handle, pt.as_dvec3()));
            }
            return Task::none();
        }
        // Keywords, distances and points fed here (option buttons, the
        // context menu, scripts) join the Recent Input list like typed ones.
        self.command_line.record_recent_input(token);
        let is_mtp = token.trim_start_matches('_').eq_ignore_ascii_case("MTP")
            || token.trim_start_matches('_').eq_ignore_ascii_case("M2P");
        if is_mtp {
            let is_point_step = !self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.input_kind().wants_text())
                .unwrap_or(true)
                || self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.point_step_accepts_keywords())
                    .unwrap_or(false);
            let not_entity_pick = !self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_entity_pick())
                .unwrap_or(false);
            if is_point_step && not_entity_pick {
                self.start_mtp_modifier(i);
                return Task::none();
            }
        }
        if let Some((coord, kind)) = crate::app::helpers::parse_coord(token) {
            // Match the GUI command line: typed coordinates are in the active
            // UCS (relative offsets are rotated by the UCS axes), so a multi-
            // token `LINE 0,0 10,10` under a rotated UCS lands correctly.
            let ucs = self.tabs[i].active_ucs.clone();
            let wcs = match (
                matches!(kind, crate::app::helpers::CoordKind::Relative),
                self.last_point,
            ) {
                (true, Some(base)) => {
                    base + match &ucs {
                        Some(u) => crate::app::helpers::ucs_rotate_vec(coord, u),
                        None => coord,
                    }
                }
                _ => match &ucs {
                    Some(u) => crate::app::helpers::ucs_to_wcs(coord, u),
                    None => coord,
                },
            };
            if !self.command_point_allowed(i, wcs) {
                return Task::none();
            }
            self.last_point = Some(wcs);
            self.push_ucs_to_cmd(i);
            return self.feed_command(StepInput::Point(wcs));
        } else {
            // The step reads the token first: at a great many point prompts a
            // bare number is the step's own value, not a distance — ROTATE's
            // angle, SCALE's factor, CIRCLE's radius. Only a token no step
            // claims becomes direct distance entry, measured along the cursor
            // direction from the command's anchor.
            let (task, consumed) = self.feed_command_consumed(StepInput::Text(token.to_string()));
            if consumed {
                return task;
            }
            if let Some(distance) = self.try_direct_distance_entry(token) {
                return distance;
            }
            return task;
        }
    }
}
