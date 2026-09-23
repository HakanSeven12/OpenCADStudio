use super::{Message, OpenCADStudio};
use crate::command::{CmdResult, SelectionEntity, StepInput};
use acadrust::Handle;
use iced::Task;

/// `old` as a whole identifier in `source` becomes `new`.
fn replace_identifier(source: &str, old: &str, new: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        if !token.is_empty() {
            out.push_str(if token == old { new } else { token.as_str() });
            token.clear();
        }
    };
    for c in source.chars() {
        if c.is_alphanumeric() || c == '_' {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
            out.push(c);
        }
    }
    flush(&mut token, &mut out);
    out
}

impl OpenCADStudio {
    pub(super) fn reject_locked_edit(&mut self, i: usize, handle: Handle) -> bool {
        let Some(layer) = self.tabs[i].scene.locked_layer_name(handle) else {
            return false;
        };
        self.command_line.push_info(
            crate::tf!("Object is on locked layer \"{layer}\" — unlock the layer to edit it.")
                .as_ref(),
        );
        true
    }

    pub(super) fn refresh_area_preview(&mut self, i: usize) {
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
        let endpoint = |entity: &acadrust::EntityType| {
            let last_grip = match entity {
                acadrust::EntityType::Line(line) => {
                    return Some(glam::DVec3::new(line.end.x, line.end.y, line.end.z));
                }
                acadrust::EntityType::Arc(_) => Some(2),
                acadrust::EntityType::LwPolyline(polyline) => {
                    polyline.vertices.len().checked_sub(1)
                }
                acadrust::EntityType::Polyline(polyline) => polyline.vertices.len().checked_sub(1),
                acadrust::EntityType::Polyline2D(polyline) => {
                    polyline.vertices.len().checked_sub(1)
                }
                acadrust::EntityType::Polyline3D(polyline) => {
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
    pub(super) fn reset_space_interaction_state(&mut self) {
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

    pub(super) fn solve_grip_constraints(
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

    pub(super) fn capture_grip_history_originals(&mut self, i: usize, handles: &[Handle]) {
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

    pub(super) fn cancel_active_grip_edit(&mut self) -> bool {
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
    pub(super) fn cancel_active_command_for_space_change(&mut self) -> Task<Message> {
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
    pub(super) fn command_point_allowed(&mut self, i: usize, point: glam::DVec3) -> bool {
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
    pub(super) fn refresh_command_point_pick_context(&mut self, i: usize) {
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
    pub(super) fn feed_command(&mut self, input: StepInput) -> Task<Message> {
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

    /// `feed_command`, also reporting whether the step actually took the input.
    /// Only a `Text` token can come back unclaimed (`on_text_input` returning
    /// `None`), which is what lets the caller read it as something else — a
    /// typed distance, say — instead of guessing beforehand.
    pub(super) fn feed_command_consumed(&mut self, input: StepInput) -> (Task<Message>, bool) {
        // A command that takes associative dimensions only keeps just those:
        // the rest leave the selection and the count of what went is reported,
        // as the reference does while gathering.
        let input = self.filter_associative_dimension_selection(input);
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
                    acadrust::EntityType::Solid3D(_)
                        | acadrust::EntityType::Surface(_)
                        | acadrust::EntityType::Region(_)
                        | acadrust::EntityType::Body(_)
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
    pub(super) fn drain_pending_pause_tokens(&mut self, tab_idx: usize) -> Task<Message> {
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
    pub(super) fn run_command_line(&mut self, cmd: &str) -> Task<Message> {
        self.run_command_line_streaming(cmd, true)
    }

    /// `run_command_line`, with the trailing Enter left off when `finish` is
    /// false: the line is one instalment and the tool stays at its next prompt
    /// for the following one. Only a plugin streaming a command feeds it that
    /// way; the command line and the automation feeder always finish.
    pub(super) fn run_command_line_streaming(&mut self, cmd: &str, finish: bool) -> Task<Message> {
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
        if tokens[0].eq_ignore_ascii_case("BACKGROUND")
            || tokens[0].eq_ignore_ascii_case("COLORSCHEME")
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
    pub(super) fn finish_active_command(&mut self, tokens: &[String], finish: bool) -> Task<Message> {
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
    pub(super) fn try_selection_keyword(&mut self, text: &str) -> Option<Task<Message>> {
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

    pub(super) fn feed_active_cmd(&mut self, token: &str) -> Task<Message> {
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
                if let Some((coord, kind)) = super::helpers::parse_coord(token) {
                    let ucs = self.tabs[i].active_ucs.clone();
                    let wcs = match (
                        matches!(kind, super::helpers::CoordKind::Relative),
                        self.last_point,
                    ) {
                        (true, Some(base)) => {
                            base + match &ucs {
                                Some(value) => super::helpers::ucs_rotate_vec(coord, value),
                                None => coord,
                            }
                        }
                        _ => match &ucs {
                            Some(value) => super::helpers::ucs_to_wcs(coord, value),
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
        if let Some((coord, kind)) = super::helpers::parse_coord(token) {
            // Match the GUI command line: typed coordinates are in the active
            // UCS (relative offsets are rotated by the UCS axes), so a multi-
            // token `LINE 0,0 10,10` under a rotated UCS lands correctly.
            let ucs = self.tabs[i].active_ucs.clone();
            let wcs = match (
                matches!(kind, super::helpers::CoordKind::Relative),
                self.last_point,
            ) {
                (true, Some(base)) => {
                    base + match &ucs {
                        Some(u) => super::helpers::ucs_rotate_vec(coord, u),
                        None => coord,
                    }
                }
                _ => match &ucs {
                    Some(u) => super::helpers::ucs_to_wcs(coord, u),
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

    /// Applies one command result, then — when that result ended the active
    /// command — drops the selection. Editing tools (MOVE, COPY, ROTATE, …) and
    /// every other interactive command leave nothing selected once they finish,
    /// so a follow-up edit doesn't silently reuse the previous working set.
    ///
    /// `Relaunch`/`Dispatch` are excepted: they end the front-end command only
    /// to immediately start another and hand it a deliberate selection (the
    /// pick-first selector relaunching MOVE on the picked set works this way).
    /// Pure-selection commands (SELECTALL, QSELECT, …) run without an active
    /// command, so `was_active` is false and their selection is preserved.
    pub(super) fn apply_cmd_result(&mut self, mut result: CmdResult) -> Task<Message> {
        let i = self.active_tab;
        if let CmdResult::ReturnPoint(pt) = result {
            self.tabs[i].scene.clear_preview_wire();
            if let Some(mut suspended) = self.tabs[i].suspended_cmd.take() {
                self.last_point = Some(pt);
                let next_res = suspended.on_point(pt);
                self.tabs[i].active_cmd = Some(suspended);
                result = next_res;
            } else {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                return Task::none();
            }
        }
        let settings = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .and_then(|command| command.sketch_settings());
        if let Some((sketch_type, increment, tolerance)) = settings {
            let tab = &mut self.tabs[self.active_tab];
            let changed = tab.scene.document.header.sketch_type != sketch_type
                || (tab.scene.document.header.sketch_increment - increment).abs() > f64::EPSILON
                || (tab.scene.document.header.sketch_tolerance - tolerance).abs() > f64::EPSILON;
            if changed {
                crate::io::set_sketch_settings(
                    &mut tab.scene.document,
                    sketch_type,
                    increment,
                    tolerance,
                );
                tab.dirty = true;
            }
        }
        let mline_settings = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .and_then(|command| command.mline_settings());
        if let Some((scale, justification, style_name, style_handle)) = mline_settings {
            let tab = &mut self.tabs[self.active_tab];
            let header = &mut tab.scene.document.header;
            let style_handle = style_handle.unwrap_or(acadrust::Handle::NULL);
            let changed = (header.multiline_scale - scale).abs() > f64::EPSILON
                || header.multiline_justification != justification
                || !header.multiline_style.eq_ignore_ascii_case(&style_name)
                || header.current_multiline_style_handle != style_handle;
            if changed {
                header.multiline_scale = scale;
                header.multiline_justification = justification;
                header.multiline_style = style_name;
                header.current_multiline_style_handle = style_handle;
                tab.dirty = true;
            }
        }
        let was_active = self.tabs[self.active_tab].active_cmd.is_some();
        let preserve_selection = matches!(
            &result,
            CmdResult::Relaunch(..)
                | CmdResult::Dispatch(..)
                | CmdResult::SolidEdgeBlend { .. }
                | CmdResult::SolidSubtract { .. }
                | CmdResult::SliceEntities { .. }
                | CmdResult::SliceSurfaceEntities { .. }
        );
        let task = self.apply_cmd_result_inner(result);
        let i = self.active_tab;
        // A transparent zoom that just finished hands control back to the
        // command it interrupted before the "command ended" bookkeeping below.
        self.resume_transparent_parent(i);
        let preview_hidden = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|command| command.preview_hidden_handles().to_vec())
            .unwrap_or_default();
        self.tabs[i]
            .scene
            .set_command_preview_hidden(&preview_hidden);
        if was_active
            && !preserve_selection
            && self.tabs[i].active_cmd.is_none()
            && !self.tabs[i].scene.selected.is_empty()
        {
            // Remember the working set for the "Previous" selection keyword
            // (#426) before dropping it.
            self.tabs[i].prev_selection = self.tabs[i].scene.selected.iter().copied().collect();
            self.tabs[i].scene.deselect_all();
            self.refresh_properties();
        }
        // A command just ended (any terminal result) — if it was the draw
        // command an ADDSELECTED launched, revert the template-property override
        // so CLAYER / CECOLOR / … are left unchanged (#239). No-op otherwise.
        if was_active && self.tabs[i].active_cmd.is_none() {
            self.tabs[i].scene.set_hover_highlight(None);
            self.command_line.set_step_options(Vec::new());
            self.restore_add_selected_defaults();
            self.tabs[i].pending_pause_tokens = None;
        }
        if self.tabs[i].active_cmd.is_some() && self.tabs[i].pending_pause_tokens.is_some() {
            let drain_task = self.drain_pending_pause_tokens(i);
            Task::batch([task, drain_task])
        } else {
            task
        }
    }

    /// Resolve the cell under `click` on table `handle` (or any table in the drawing
    /// if `handle` is `Handle::NULL`), arm the cell indicator, and launch the cell
    /// editor unless the cell's content is locked. Shared by the double-click shortcut
    /// and the TABLEDIT point pick so both agree on grid coordinates and editor setup.
    pub(crate) fn begin_table_cell_edit(
        &mut self,
        i: usize,
        handle: acadrust::Handle,
        click: glam::DVec3,
    ) -> crate::modules::annotate::table_cmd::TableCellEditStart {
        use crate::modules::annotate::table_cmd::{table_cell_at, TableCellEditStart};
        let target = if handle != acadrust::Handle::NULL {
            self.tabs[i]
                .scene
                .document
                .get_entity(handle)
                .and_then(|entity| match entity {
                    acadrust::EntityType::Table(table) => Some((handle, table.clone())),
                    _ => None,
                })
        } else {
            // Find all tables whose grid contains `click`.
            // Iterating document.entities() visits entities in draw order;
            // taking the last match selects the table on top if they overlap.
            let mut matched = None;
            for entity in self.tabs[i].scene.document.entities() {
                if let acadrust::EntityType::Table(table) = entity {
                    let h = table.common.handle;
                    let style = table.table_style_handle.and_then(|style_handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .objects
                            .get(&style_handle)
                            .and_then(|object| match object {
                                acadrust::objects::ObjectType::TableStyle(style) => Some(style),
                                _ => None,
                            })
                    });
                    if table_cell_at(table, style, click).is_some() {
                        matched = Some((h, table.clone()));
                    }
                }
            }
            matched
        };
        let Some((handle, table)) = target else {
            return TableCellEditStart::NoCell;
        };
        let style = table.table_style_handle.and_then(|style_handle| {
            self.tabs[i]
                .scene
                .document
                .objects
                .get(&style_handle)
                .and_then(|object| match object {
                    acadrust::objects::ObjectType::TableStyle(style) => Some(style),
                    _ => None,
                })
        });
        let Some(hit) = table_cell_at(&table, style, click) else {
            return TableCellEditStart::NoCell;
        };
        // Arm the Properties cell indicator whether or not the cell turns
        // out to be editable (matches the double-click behavior).
        let index = hit.index(&table);
        self.tabs[i].properties.prop_vertex = index;
        self.tabs[i].properties.prop_vertex_indicator_active = true;
        crate::entities::table::set_prop_current_cell(index);
        self.refresh_properties();
        if hit.locked {
            return TableCellEditStart::LockedCell;
        }
        let command = crate::modules::annotate::table_cmd::TableCellEditCommand::new(
            handle, &table, hit.row, hit.column,
        );
        self.command_line
            .push_info(&crate::command::CadCommand::prompt(&command));
        self.tabs[i].active_cmd = Some(Box::new(command));
        TableCellEditStart::Started
    }

    /// Reports a refused command-line value and shows the prompt again.
    fn reprompt_active_command(&mut self, i: usize, message: &str) {
        self.command_line.push_error(message);
        if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|command| command.prompt()) {
            self.command_line.push_info(&prompt);
        }
    }

    /// Drops what removed dimensional constraints leave behind: their dynamic
    /// dimensions, and their parameters when no other constraint reads them.
    pub(crate) fn purge_dimensional_extras(
        &mut self,
        i: usize,
        dimensions: Vec<Handle>,
        parameters: Vec<String>,
    ) {
        if !dimensions.is_empty() {
            self.tabs[i].scene.erase_entities(&dimensions);
        }
        let unused: Vec<String> = parameters
            .into_iter()
            .filter(|name| {
                !self.tabs[i]
                    .scene
                    .parametric_constraints
                    .iter()
                    .flat_map(|set| set.constraints.iter())
                    .any(|constraint| {
                        matches!(
                            &constraint.driving_param,
                            Some(crate::scene::named_parameters::DrivingValue::Named(used))
                                if used == name
                        )
                    })
            })
            .collect();
        if !unused.is_empty() {
            self.tabs[i].scene.record_undo_named_parameters_before();
            for name in &unused {
                self.tabs[i].scene.named_parameters_mut().remove(name);
            }
        }
    }

    /// `Dynamic` or `Annotational`, the DCFORM setting.
    pub(crate) fn constraint_form_name(&self) -> &'static str {
        if self.constraint_form_annotational {
            "Annotational"
        } else {
            "Dynamic"
        }
    }

    /// The value prompt a double-clicked constraint dimension opens.
    pub(crate) fn dynamic_dimension_value_command(
        &self,
        i: usize,
        handle: Handle,
    ) -> Option<crate::modules::parametric::DimensionValueCommand> {
        use crate::scene::named_parameters::DrivingValue;
        let scene = &self.tabs[i].scene;
        for set in &scene.parametric_constraints {
            let Some((id, _)) = set
                .dimensions
                .iter()
                .find(|(_, dimension)| **dimension == handle)
            else {
                continue;
            };
            let constraint = set.get(*id)?;
            let Some(DrivingValue::Named(name)) = &constraint.driving_param else {
                return None;
            };
            let table = if set.local_parameters.is_empty() {
                scene.named_parameters()
            } else {
                &set.local_parameters
            };
            let current = table
                .get(name)
                .map(|parameter| parameter.source.trim().to_string())
                .unwrap_or_default();
            return Some(crate::modules::parametric::DimensionValueCommand::new(
                name.clone(),
                current,
            ));
        }
        None
    }

    /// Applies `expression` or `newname=expression` to parameter `name`:
    /// the dimension value prompt and -PARAMETERS Edit.
    pub(crate) fn apply_parameter_input(&mut self, i: usize, name: &str, input: &str) {
        let input = input.trim();
        let (target, expression) = match input.split_once('=') {
            Some((new_name, expression)) => {
                (new_name.trim().to_string(), expression.trim().to_string())
            }
            None => (name.to_string(), input.to_string()),
        };
        if expression.is_empty() {
            return;
        }
        if target != name {
            if let Err(error) = self.rename_parameter(i, name, &target) {
                self.command_line.push_error(&error);
                return;
            }
        }
        let mut table = self.tabs[i].scene.named_parameters().clone();
        if let Err(error) = table.set(&target, &expression) {
            self.command_line.push_error(&error.to_string());
            return;
        }
        if !table
            .resolve(&target)
            .is_ok_and(|value| value.is_finite())
        {
            self.command_line.push_error("Invalid expression.");
            return;
        }
        let pending = self.begin_undo(i, "Edit parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        self.resolve_named_parameter_edit(i, &target);
        self.tabs[i].scene.refresh_dynamic_dimension_texts();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    /// -PARAMETERS New.
    pub(crate) fn create_parameter(&mut self, i: usize, name: &str, expression: &str) {
        if self.tabs[i].scene.named_parameters().contains(name) {
            self.command_line
                .push_error(&format!("Parameter {name} already exists."));
            return;
        }
        let mut table = self.tabs[i].scene.named_parameters().clone();
        if let Err(error) = table.set(name, expression) {
            self.command_line.push_error(&error.to_string());
            return;
        }
        let pending = self.begin_undo(i, "New parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        self.tabs[i].scene.sync_native_parametric_graph();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    /// Renames a parameter everywhere: the table, the expressions that use
    /// it and the constraints it drives.
    pub(crate) fn rename_parameter(&mut self, i: usize, old: &str, new: &str) -> Result<(), String> {
        use crate::scene::named_parameters::{is_reserved_name, is_valid_name, DrivingValue};
        let table = self.tabs[i].scene.named_parameters().clone();
        let Some(source) = table.get(old).map(|parameter| parameter.source.clone()) else {
            return Err(format!("Parameter {old} not found."));
        };
        if !is_valid_name(new) || is_reserved_name(new) || table.contains(new) {
            return Err(format!("Invalid parameter name {new}."));
        }
        let mut renamed = table.clone();
        renamed.set(new, &source).map_err(|error| error.to_string())?;
        for parameter in table.iter() {
            if parameter.name == old {
                continue;
            }
            let rewritten = replace_identifier(&parameter.source, old, new);
            if rewritten != parameter.source {
                renamed
                    .set(&parameter.name, &rewritten)
                    .map_err(|error| error.to_string())?;
            }
        }
        renamed.remove(old);
        let pending = self.begin_undo(i, "Rename parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = renamed;
        let scopes: Vec<_> = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .map(|set| set.scope)
            .collect();
        for scope in scopes {
            let before = self.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .filter(|set| {
                    set.constraints.iter().any(|constraint| {
                        matches!(&constraint.driving_param, Some(DrivingValue::Named(name)) if name == old)
                    })
                })
                .cloned();
            let Some(before) = before else {
                continue;
            };
            self.tabs[i]
                .scene
                .record_undo_parametric_constraints_before(scope, before);
            let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
            for constraint in &mut set.constraints {
                if matches!(&constraint.driving_param, Some(DrivingValue::Named(name)) if name == old) {
                    constraint.driving_param = Some(DrivingValue::Named(new.to_string()));
                }
            }
        }
        self.tabs[i].scene.refresh_dynamic_dimension_texts();
        self.tabs[i].scene.sync_native_parametric_graph();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        Ok(())
    }

    /// -PARAMETERS Delete: a parameter no constraint drives with.
    pub(crate) fn delete_parameter(&mut self, i: usize, name: &str) {
        use crate::scene::named_parameters::DrivingValue;
        if !self.tabs[i].scene.named_parameters().contains(name) {
            self.command_line
                .push_error(&format!("Parameter {name} not found."));
            return;
        }
        let in_use = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .any(|constraint| {
                matches!(&constraint.driving_param, Some(DrivingValue::Named(used)) if used == name)
            });
        if in_use {
            self.command_line
                .push_error(&format!("Parameter {name} is used by a constraint."));
            return;
        }
        let pending = self.begin_undo(i, "Delete parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters_mut().remove(name);
        self.tabs[i].scene.sync_native_parametric_graph();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    fn remove_parametric_constraint(
        &mut self,
        id: crate::scene::parametric_constraints::ConstraintId,
        label: &'static str,
    ) -> bool {
        let i = self.active_tab;
        let scope = self.tabs[i].current_parametric_scope();
        let Some(set) = self.tabs[i].scene.parametric_constraint_set(scope) else {
            return false;
        };
        let Some(constraint) = set.get(id) else {
            return false;
        };
        let touched: Vec<Handle> = constraint.refs.iter().map(|r| r.entity).collect();
        let dimension = set.dimensions.get(&id).copied();
        let parameter = match &constraint.driving_param {
            Some(crate::scene::named_parameters::DrivingValue::Named(name)) => Some(name.clone()),
            _ => None,
        };
        let constraints_before = set.clone();
        let pending = self.begin_undo(i, label, touched.len(), dimension.is_none());
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        self.tabs[i]
            .scene
            .parametric_constraint_set_mut(scope)
            .remove(id);
        self.purge_dimensional_extras(
            i,
            dimension.into_iter().collect(),
            parameter.into_iter().collect(),
        );
        self.tabs[i].scene.bump_constraints_epoch();
        if self.tabs[i].scene.selected_constraint == Some(id) {
            self.tabs[i].scene.selected_constraint = None;
        }
        let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
            .into_iter()
            .map(|h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        true
    }

    /// Removes and re-solves the first conflicting constraint in the active scope.
    pub(super) fn resolve_one_parametric_conflict(&mut self) {
        let i = self.active_tab;
        let scope = self.tabs[i].current_parametric_scope();
        let Some(id) = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .and_then(|set| set.conflicts.first().map(|(id, _)| *id))
        else {
            return;
        };
        self.remove_parametric_constraint(id, "Remove conflicting constraint");
    }

    /// Removes one user-selected constraint from the active standard graph scope.
    pub(super) fn delete_parametric_constraint(
        &mut self,
        id: crate::scene::parametric_constraints::ConstraintId,
    ) {
        self.remove_parametric_constraint(id, "Delete constraint");
    }

    /// Rebuilds the active tab's `Scene::named_parameters` from the
    /// editor's working buffer and re-solves dependent constraints.
    pub(super) fn apply_named_parameter_editor_rows(&mut self) {
        let i = self.active_tab;
        // A name shared by more than one row can't be resolved by picking
        // one arbitrarily (`ParameterTable::set` would just let the last
        // one silently win) -- refuse every row sharing that name up front,
        // same as `named_parameters::preview`'s live check.
        let duplicates = crate::ui::window::named_parameters::duplicate_name_rows(
            &self.named_parameter_editor_rows,
        );
        let mut table = crate::scene::named_parameters::ParameterTable::new();
        let mut failed = 0usize;
        for (idx, row) in self.named_parameter_editor_rows.iter().enumerate() {
            let name = row.name.trim();
            if name.is_empty() || duplicates.contains(&idx) {
                continue;
            }
            if let Err(e) = table.set(name, row.formula.trim()) {
                self.command_line
                    .push_error(crate::tf!("Parameter '{}': {}", name, e).as_ref());
                failed += 1;
            }
        }
        if !duplicates.is_empty() {
            self.command_line.push_error(
                crate::tf!(
                    "{} row(s) skipped: duplicate parameter name.",
                    duplicates.len()
                )
                .as_ref(),
            );
            failed += duplicates.len();
        }
        let param_count = table.len();

        // The whole table just changed, not one isolated value, so there is
        // no cheaper "which handles actually moved" test worth doing here —
        // re-solve every scope with a constraint driven by *any* named
        // parameter (mirrors `solve_scope`'s own "full rebuild, not
        // incremental" philosophy).
        let touched: Vec<Handle> = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .filter(|c| {
                c.enabled
                    && matches!(
                        c.driving_param,
                        Some(crate::scene::named_parameters::DrivingValue::Named(_))
                    )
            })
            .flat_map(|c| c.refs.iter().map(|r| r.entity))
            .collect();

        // A changed value moves a dimensional constraint's second point; its
        // first point stays, as in the reference.
        let anchors: Vec<crate::scene::parametric_constraints::ParametricRef> = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .filter(|c| {
                c.enabled
                    && matches!(
                        c.driving_param,
                        Some(crate::scene::named_parameters::DrivingValue::Named(_))
                    )
            })
            .flat_map(|c| {
                crate::scene::parametric_constraints::dimensional_anchor_refs(
                    &self.tabs[i].scene.document,
                    &c.refs,
                )
            })
            .collect();
        let pending = self.begin_undo(i, "Apply named parameters", touched.len(), true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        self.tabs[i].dirty = true;
        if !touched.is_empty() {
            let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities_with_parametric_policy(
                &changes,
                &anchors,
                self.constraint_solve_mode,
            );
        } else {
            self.tabs[i].scene.sync_native_parametric_graph();
        }
        self.tabs[i].scene.refresh_dynamic_dimension_texts();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        if failed == 0 {
            self.command_line
                .push_info(crate::tf!("{} parameter(s) applied.", param_count).as_ref());
        }
    }

    /// Re-solves every entity touched by an enabled constraint driven by
    /// `name` — the targeted, single-row counterpart to
    /// `apply_named_parameter_editor_rows`'s "re-solve what it affects": an
    /// inline Properties-panel edit changes one parameter, not the whole
    /// table, so unlike that whole-table Apply this can cheaply scope the
    /// re-solve to just the entities that actually reference it.
    fn resolve_named_parameter_edit(&mut self, i: usize, name: &str) {
        let readers: Vec<&crate::scene::parametric_constraints::ParametricConstraint> = self
            .tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .filter(|c| {
                c.enabled
                    && matches!(&c.driving_param, Some(crate::scene::named_parameters::DrivingValue::Named(n)) if n == name)
            })
            .collect();
        let touched: Vec<Handle> = readers
            .iter()
            .flat_map(|c| c.refs.iter().map(|r| r.entity))
            .collect();
        // A changed value moves a dimensional constraint's second point; its
        // first point (and the line it measures perpendicular to) stays, as
        // in the reference.
        let anchors: Vec<crate::scene::parametric_constraints::ParametricRef> = readers
            .iter()
            .flat_map(|c| {
                crate::scene::parametric_constraints::dimensional_anchor_refs(
                    &self.tabs[i].scene.document,
                    &c.refs,
                )
            })
            .collect();
        self.tabs[i].dirty = true;
        if touched.is_empty() {
            self.tabs[i].scene.sync_native_parametric_graph();
            return;
        }
        let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
            .into_iter()
            .map(|h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i]
            .scene
            .bump_entities_with_parametric_policy(&changes, &anchors, false);
    }

    /// Commits one field of one Parameters-section row (Properties panel) —
    /// the per-row, commit-on-submit counterpart to
    /// `apply_named_parameter_editor_rows`'s whole-table Apply. A Formula
    /// edit is a plain redefine (`ParameterTable::set` on the existing
    /// name); a Name edit is a remove-then-set (there's no rename primitive
    /// — `ParameterTable` upserts by name), restoring the old name if the
    /// new one fails to validate so nothing is silently lost. Either way
    /// this only re-solves what actually depends on this one parameter —
    /// no "duplicate name" scan across the whole table is needed the way
    /// the buffered modal Apply needs one, since every row here is always
    /// already a real, distinct table entry.
    pub(super) fn on_prop_param_commit(
        &mut self,
        index: usize,
        field: crate::ui::window::named_parameters::ParamField,
    ) -> Task<Message> {
        use crate::ui::properties::FieldKey;
        use crate::ui::window::named_parameters::ParamField;
        let i = self.active_tab;
        let key = FieldKey::Param(index, field);
        self.tabs[i].properties.active_field = None;
        let Some(typed) = self.tabs[i].properties.edit_buf.remove(&key) else {
            return Task::none();
        };
        let typed = typed.trim().to_string();
        let Some(current) = self.tabs[i]
            .scene
            .named_parameters()
            .iter()
            .nth(index)
            .cloned()
        else {
            self.refresh_properties();
            return Task::none();
        };

        match field {
            ParamField::Formula if typed.is_empty() || typed == current.source => {
                self.refresh_properties();
                return Task::none();
            }
            ParamField::Name if typed.is_empty() || typed == current.name => {
                self.refresh_properties();
                return Task::none();
            }
            ParamField::Name if self.tabs[i].scene.named_parameters().contains(&typed) => {
                self.command_line
                    .push_error(crate::tf!("Parameter '{}' already exists.", typed).as_ref());
                self.refresh_properties();
                return Task::none();
            }
            _ => {}
        }

        let pending = self.begin_undo(i, "Edit named parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();

        let (result, resolve_name) = match field {
            ParamField::Formula => (
                self.tabs[i]
                    .scene
                    .named_parameters_mut()
                    .set(&current.name, &typed),
                current.name.clone(),
            ),
            ParamField::Name => {
                let description = self.tabs[i]
                    .scene
                    .named_parameters()
                    .description(&current.name)
                    .to_string();
                self.tabs[i]
                    .scene
                    .named_parameters_mut()
                    .remove(&current.name);
                let outcome = self.tabs[i]
                    .scene
                    .named_parameters_mut()
                    .set(&typed, &current.source);
                let kept = if outcome.is_err() {
                    let _ = self.tabs[i]
                        .scene
                        .named_parameters_mut()
                        .set(&current.name, &current.source);
                    current.name.as_str()
                } else {
                    typed.as_str()
                };
                self.tabs[i]
                    .scene
                    .named_parameters_mut()
                    .set_description(kept, &description);
                (outcome, typed.clone())
            }
        };

        if let Err(e) = result {
            self.tabs[i].scene.take_undo_recording();
            self.command_line
                .push_error(crate::tf!("Parameter '{}': {}", current.name, e).as_ref());
            self.refresh_properties();
            return Task::none();
        }

        if field == ParamField::Name {
            let affected: Vec<usize> = self.tabs[i]
                .scene
                .parametric_constraints
                .iter()
                .enumerate()
                .filter_map(|(index, set)| {
                    set.constraints
                        .iter()
                        .any(|constraint| {
                            matches!(&constraint.driving_param, Some(crate::scene::named_parameters::DrivingValue::Named(name)) if name == &current.name)
                        })
                        .then_some(index)
                })
                .collect();
            for set_index in affected {
                let scope = self.tabs[i].scene.parametric_constraints[set_index].scope;
                let before = self.tabs[i].scene.parametric_constraints[set_index].clone();
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, before);
                // A parameter rename rewrites the `Named` driving references
                // pointing at it — the glyph labels change with them.
                let mut renamed = false;
                for constraint in
                    &mut self.tabs[i].scene.parametric_constraints[set_index].constraints
                {
                    if let Some(crate::scene::named_parameters::DrivingValue::Named(name)) =
                        &mut constraint.driving_param
                    {
                        if name == &current.name {
                            *name = typed.clone();
                            renamed = true;
                        }
                    }
                }
                if renamed {
                    self.tabs[i].scene.bump_constraints_epoch();
                }
            }
        }
        self.resolve_named_parameter_edit(i, &resolve_name);
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        Task::none()
    }

    /// The dynamic dimension the Properties panel targets: its handle, scope,
    /// constraint id and parameter name.
    fn dynamic_dimension_target(
        &self,
        i: usize,
    ) -> Option<(
        Handle,
        crate::scene::parametric_constraints::ParametricScope,
        crate::scene::parametric_constraints::ConstraintId,
        String,
    )> {
        use crate::scene::named_parameters::DrivingValue;
        use crate::scene::parametric_constraints::dynamic_dimension_constraint;
        let handle = *self.property_target_handles(i).first()?;
        let (set, constraint) =
            dynamic_dimension_constraint(&self.tabs[i].scene.parametric_constraints, handle)?;
        let Some(DrivingValue::Named(name)) = &constraint.driving_param else {
            return None;
        };
        Some((handle, set.scope, constraint.id, name.clone()))
    }

    /// Commits a dynamic dimension's Description row into its parameter.
    pub(super) fn on_dynamic_dimension_description_commit(
        &mut self,
        field: &'static str,
    ) -> Task<Message> {
        use crate::ui::properties::FieldKey;
        let i = self.active_tab;
        self.tabs[i].properties.active_field = None;
        let Some(typed) = self.tabs[i]
            .properties
            .edit_buf
            .remove(&FieldKey::Geom(field))
        else {
            return Task::none();
        };
        let Some((_, _, _, name)) = self.dynamic_dimension_target(i) else {
            self.refresh_properties();
            return Task::none();
        };
        let pending = self.begin_undo(i, "Constraint description", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i]
            .scene
            .named_parameters_mut()
            .set_description(&name, &typed);
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        Task::none()
    }

    /// Applies a dynamic dimension's Constraint Form or Reference choice.
    pub(super) fn on_dynamic_dimension_choice(
        &mut self,
        field: &'static str,
        value: &str,
    ) -> Task<Message> {
        use crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER;
        let i = self.active_tab;
        let Some((handle, scope, id, _)) = self.dynamic_dimension_target(i) else {
            return Task::none();
        };
        match field {
            "dyn_constraint_form" => {
                // Annotational: an ordinary dimension on the current layer
                // that plots; Dynamic: the gray one on the constraints layer.
                let annotational = value.eq_ignore_ascii_case("Annotational");
                let active_layer = self.tabs[i].active_layer.clone();
                if !annotational {
                    self.tabs[i].scene.ensure_dynamic_dimension_layer();
                }
                self.apply_property_op(i, "Constraint form", &[handle], |app, handle| {
                    use crate::entities::dim_override as dov;
                    let Some(mut entity) = app.tabs[i].scene.document.get_entity(handle).cloned()
                    else {
                        return;
                    };
                    if annotational {
                        entity.as_entity_mut().set_layer(active_layer.clone());
                        entity.common_mut().color = acadrust::types::Color::ByLayer;
                        // The dynamic form's screen-size and horizontal-text
                        // overrides go; the style draws it.
                        for code in [
                            dov::DIMSCALE,
                            dov::DIMGAP,
                            dov::DIMEXO,
                            dov::DIMEXE,
                            dov::DIMASZ,
                            dov::DIMTAD,
                            dov::DIMTIH,
                            dov::DIMTOH,
                        ] {
                            dov::set_on_entity(&mut entity, code, None);
                        }
                    } else {
                        entity
                            .as_entity_mut()
                            .set_layer(DYNAMIC_DIMENSION_LAYER.to_string());
                        entity.common_mut().color = acadrust::types::Color::Rgb {
                            r: 103,
                            g: 109,
                            b: 118,
                        };
                    }
                    app.tabs[i].scene.update_entity(entity);
                });
                // The dynamic form is rescaled to the screen and shows the
                // trimmed value; the annotational one is never hidden.
                self.tabs[i].scene.refresh_dynamic_dimension_scales(true);
                self.tabs[i].scene.refresh_dynamic_dimension_texts();
                self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
            }
            "dyn_constraint_reference" => {
                // A reference constraint reads the geometry instead of
                // driving it; its text sits in parentheses.
                let reference =
                    value.eq_ignore_ascii_case("Yes") || value == crate::t!("Yes").as_ref();
                let Some(before) = self.tabs[i].scene.parametric_constraint_set(scope).cloned()
                else {
                    return Task::none();
                };
                let pending = self.begin_undo(i, "Constraint reference", 0, true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, before);
                self.tabs[i].scene.record_undo_named_parameters_before();
                let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
                if let Some(constraint) = set.constraints.iter_mut().find(|c| c.id == id) {
                    constraint.enabled = !reference;
                }
                // Toggling `enabled` changes the glyph placement set (and the
                // dynamic pills), so the memoised placements must miss next
                // frame — same guarantee `set_constraint_enabled` gives.
                self.tabs[i].scene.bump_constraints_epoch();
                self.tabs[i].scene.refresh_dynamic_dimension_texts();
                self.tabs[i].scene.sync_native_parametric_graph();
                self.tabs[i].dirty = true;
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
                self.refresh_properties();
            }
            _ => {}
        }
        Task::none()
    }

    /// Commits a dynamic dimension's Name or Expression row: the typed text
    /// goes to the Parameters-section row of the constraint's parameter.
    pub(super) fn on_dynamic_dimension_field_commit(
        &mut self,
        field: &'static str,
        param_field: crate::ui::window::named_parameters::ParamField,
    ) -> Task<Message> {
        use crate::scene::named_parameters::DrivingValue;
        use crate::scene::parametric_constraints::dynamic_dimension_constraint;
        use crate::ui::properties::FieldKey;
        let i = self.active_tab;
        self.tabs[i].properties.active_field = None;
        let Some(typed) = self.tabs[i]
            .properties
            .edit_buf
            .remove(&FieldKey::Geom(field))
        else {
            return Task::none();
        };
        let handles = self.property_target_handles(i);
        let name = handles.first().and_then(|handle| {
            let (_, constraint) =
                dynamic_dimension_constraint(&self.tabs[i].scene.parametric_constraints, *handle)?;
            match &constraint.driving_param {
                Some(DrivingValue::Named(name)) => Some(name.clone()),
                _ => None,
            }
        });
        let index = name.and_then(|name| {
            self.tabs[i]
                .scene
                .named_parameters()
                .iter()
                .position(|parameter| parameter.name == name)
        });
        let Some(index) = index else {
            self.refresh_properties();
            return Task::none();
        };
        self.tabs[i]
            .properties
            .edit_buf
            .insert(FieldKey::Param(index, param_field), typed);
        self.on_prop_param_commit(index, param_field)
    }

    /// Removes Parameters-section row `index` immediately. Any
    /// constraint that referenced it gets the same "undefined reference"
    /// resolve-failure treatment a formula's own bad reference already gets
    /// (`build_constraint`) — re-solving surfaces that rather than needing
    /// special-case handling here.
    pub(super) fn on_prop_param_delete(&mut self, index: usize) -> Task<Message> {
        let i = self.active_tab;
        let Some(name) = self.tabs[i]
            .scene
            .named_parameters()
            .iter()
            .nth(index)
            .map(|p| p.name.clone())
        else {
            return Task::none();
        };
        let pending = self.begin_undo(i, "Delete named parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters_mut().remove(&name);
        self.resolve_named_parameter_edit(i, &name);
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        Task::none()
    }

    /// The Parameters section's "+ Add parameter" row: appends a fresh,
    /// uniquely-named parameter (`param1`, `param2`, …) the user then
    /// renames/redefines inline via `on_prop_param_commit`. Defaults to `1`,
    /// not `0`: renaming this onto a name an existing Distance/Radius
    /// constraint already references (unusual, but reachable — define
    /// first, wire up the constraint's reference later) would otherwise
    /// briefly collapse that geometry to a zero-length/zero-radius
    /// degenerate state, which is a genuine numerical singularity for a
    /// point-to-point distance constraint to grow back out of (no defined
    /// direction to separate two exactly-coincident points from). `1` never
    /// hits that.
    pub(super) fn on_prop_param_add_new(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let mut n = 1usize;
        let name = loop {
            let candidate = format!("param{n}");
            if !self.tabs[i].scene.named_parameters().contains(&candidate) {
                break candidate;
            }
            n += 1;
        };
        let pending = self.begin_undo(i, "Add named parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        let _ = self.tabs[i].scene.named_parameters_mut().set(&name, "1");
        self.tabs[i].scene.sync_native_parametric_graph();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        Task::none()
    }

    /// Bring back the command a transparent zoom parked, once the zoom is
    /// over (no command active any more). No-op otherwise.
    pub(in crate::app) fn resume_transparent_parent(&mut self, i: usize) {
        if !self.tabs[i].transparent_resume {
            return;
        }
        if self.tabs[i].active_cmd.is_some() {
            // The transparent prompt is still up (or the parent was already
            // restored by a Cancel).
            if self.tabs[i].suspended_cmd.is_none() {
                self.tabs[i].transparent_resume = false;
            }
            return;
        }
        self.tabs[i].transparent_resume = false;
        let Some(parent) = self.tabs[i].suspended_cmd.take() else {
            return;
        };
        self.tabs[i].active_cmd = Some(parent);
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
        let opts = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.options())
            .unwrap_or_default();
        self.command_line.set_step_options(opts);
        self.refresh_active_cmd_preview(i);
    }

    pub(in crate::app) fn start_mtp_modifier(&mut self, i: usize) {
        let parent = self.tabs[i].active_cmd.take();
        self.tabs[i].suspended_cmd = parent;
        self.tabs[i].scene.clear_preview_wire();
        let cmd = crate::command::Mid2PointCommand::new();
        let prompt = crate::command::CadCommand::prompt(&cmd);
        self.tabs[i].active_cmd = Some(Box::new(cmd));
        self.command_line.push_info(&prompt);
        self.command_line.set_step_options(Vec::new());
        self.refresh_active_cmd_preview(i);
    }

    /// Apply a command replacement without deleting a one-to-one entity edit.
    /// Splits and entity-type conversions still allocate replacement identities.
    fn replace_command_entity(
        &mut self,
        tab: usize,
        handle: Handle,
        mut entities: Vec<acadrust::EntityType>,
    ) -> Vec<Handle> {
        let same_type = entities.len() == 1
            && self.tabs[tab]
                .scene
                .document
                .get_entity(handle)
                .is_some_and(|original| {
                    std::mem::discriminant(original) == std::mem::discriminant(&entities[0])
                });
        let handles = if same_type {
            let mut entity = entities.pop().expect("one replacement");
            entity.common_mut().handle = handle;
            // Scene::update_entity also retains the live owning block and
            // refreshes dependent render caches without erase notifications.
            if self.tabs[tab].scene.update_entity(entity) {
                vec![handle]
            } else {
                Vec::new()
            }
        } else {
            let owner = self.tabs[tab]
                .scene
                .document
                .get_entity(handle)
                .map(|entity| entity.common().owner_handle);
            self.tabs[tab].scene.erase_entities(&[handle]);
            entities
                .into_iter()
                .map(|mut entity| {
                    entity.common_mut().handle = Handle::NULL;
                    if let Some(owner) = owner {
                        entity.common_mut().owner_handle = owner;
                    }
                    self.tabs[tab].scene.add_entity(entity)
                })
                .collect()
        };
        for &updated in &handles {
            if matches!(
                self.tabs[tab].scene.document.get_entity(updated),
                Some(acadrust::EntityType::Dimension(_))
            ) {
                self.tabs[tab].scene.invalidate_dim_block_recorded(updated);
            }
        }
        handles
    }

    fn apply_pedit_result(
        &mut self,
        tab: usize,
        handle: Handle,
        op: crate::modules::draw::modify::pedit::PeditOp,
    ) -> Task<Message> {
        use crate::modules::draw::modify::pedit::{apply_pedit, convert_to_polyline, PeditOp};

        if !matches!(
            &op,
            PeditOp::Multiple(_, _) | PeditOp::JoinSelection(_, _, _)
        ) && self.reject_locked_edit(tab, handle)
        {
            return Task::none();
        }
        match &op {
            PeditOp::JoinSelection(handles, fuzz, kind) => {
                let mut available = handles
                    .iter()
                    .filter_map(|handle| {
                        if self.tabs[tab].scene.is_layer_locked(*handle) {
                            return None;
                        }
                        self.tabs[tab]
                            .scene
                            .document
                            .get_entity(*handle)
                            .cloned()
                            .map(|entity| (*handle, entity))
                    })
                    .collect::<Vec<_>>();
                let connector_distance =
                    crate::modules::draw::modify::pedit::selection_connector_distance(
                        &available, *fuzz,
                    );
                let mut changes = Vec::new();
                while !available.is_empty() {
                    let (source_handle, source) = available.remove(0);
                    let candidates = available
                        .iter()
                        .map(|(handle, entity)| (*handle, entity))
                        .collect::<Vec<_>>();
                    if let Some((mut result, consumed)) =
                        crate::modules::draw::modify::pedit::join_selection_extend(
                            &source,
                            &candidates,
                            *fuzz,
                            *kind,
                            connector_distance,
                        )
                    {
                        *result.common_mut() = source.common().clone();
                        available.retain(|(handle, _)| !consumed.contains(handle));
                        changes.push((source_handle, result, consumed));
                    }
                }
                if !changes.is_empty() {
                    self.push_undo_snapshot(tab, "PEDIT");
                    for (source, replacement, consumed) in changes {
                        self.tabs[tab].scene.erase_entities(&consumed);
                        self.tabs[tab].scene.update_entity(replacement.clone());
                        if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                            for old in consumed {
                                command.on_entity_replaced(old, &[source]);
                            }
                            command.inject_picked_entity(replacement);
                        }
                    }
                    if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                        command.on_pedit_applied();
                    }
                    self.tabs[tab].dirty = true;
                    self.refresh_properties();
                }
            }
            PeditOp::Multiple(handles, operation) => {
                let replacements: Vec<_> = handles
                    .iter()
                    .filter(|handle| !self.tabs[tab].scene.is_layer_locked(**handle))
                    .filter_map(|handle| {
                        let original = self.tabs[tab].scene.document.get_entity(*handle)?;
                        if matches!(operation.as_ref(), PeditOp::ConvertToPolyline) {
                            convert_to_polyline(original)
                                .map(|replacement| (*handle, replacement, true))
                        } else {
                            let mut replacement = original.clone();
                            apply_pedit(&mut replacement, operation).then_some((
                                *handle,
                                replacement,
                                false,
                            ))
                        }
                    })
                    .collect();
                if replacements.is_empty() {
                    return Task::none();
                }
                self.push_undo_snapshot(tab, "PEDIT");
                for (handle, replacement, converted) in replacements {
                    let updated_handle = if converted {
                        self.tabs[tab].scene.erase_entities(&[handle]);
                        let new_handle = self.tabs[tab].scene.add_entity(replacement);
                        if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                            command.on_entity_replaced(handle, &[new_handle]);
                        }
                        new_handle
                    } else {
                        if let Some(entity) = self.tabs[tab].scene.document.get_entity_mut(handle) {
                            *entity = replacement;
                        }
                        self.tabs[tab].scene.refresh_fill_model(handle);
                        self.tabs[tab]
                            .scene
                            .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                        handle
                    };
                    let updated = self.tabs[tab]
                        .scene
                        .document
                        .get_entity(updated_handle)
                        .cloned();
                    if let (Some(command), Some(entity)) =
                        (self.tabs[tab].active_cmd.as_mut(), updated)
                    {
                        command.inject_picked_entity(entity);
                    }
                }
                if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                    command.on_pedit_applied();
                }
                self.tabs[tab].dirty = true;
                self.refresh_properties();
            }
            PeditOp::VertexRange { first, last, split } => {
                let pieces = self.tabs[tab]
                    .scene
                    .document
                    .get_entity(handle)
                    .and_then(|entity| {
                        crate::modules::draw::modify::pedit::edit_vertex_range(
                            entity, *first, *last, *split,
                        )
                    });
                if let Some(mut pieces) = pieces {
                    self.push_undo_snapshot(tab, "PEDIT");
                    let source = pieces.remove(0);
                    self.tabs[tab].scene.update_entity(source);
                    for mut piece in pieces {
                        piece.common_mut().handle = Handle::NULL;
                        self.tabs[tab].scene.add_entity(piece);
                    }
                    let updated = self.tabs[tab].scene.document.get_entity(handle).cloned();
                    if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                        if let Some(entity) = updated {
                            command.inject_picked_entity(entity);
                        }
                        command.on_pedit_applied();
                    }
                    self.tabs[tab].dirty = true;
                    self.refresh_properties();
                } else {
                    self.command_line.push_error("PEDIT: invalid vertex range.");
                }
            }
            PeditOp::ConvertToPolyline => {
                let converted = self.tabs[tab]
                    .scene
                    .document
                    .get_entity(handle)
                    .and_then(convert_to_polyline);
                match converted {
                    Some(polyline) => {
                        return self
                            .apply_cmd_result(CmdResult::ReplaceEntity(handle, vec![polyline]));
                    }
                    None => self
                        .command_line
                        .push_error(crate::t!("PEDIT: cannot convert this entity.").as_ref()),
                }
            }
            _ => {
                self.push_undo_snapshot(tab, "PEDIT");
                let changed = self.tabs[tab]
                    .scene
                    .document
                    .get_entity_mut(handle)
                    .map(|entity| apply_pedit(entity, &op))
                    .unwrap_or(false);
                if changed {
                    let updated = self.tabs[tab].scene.document.get_entity(handle).cloned();
                    if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                        if let Some(entity) = updated {
                            command.inject_picked_entity(entity);
                        }
                        command.on_pedit_applied();
                    }
                    self.tabs[tab].dirty = true;
                    self.tabs[tab].scene.refresh_fill_model(handle);
                    self.tabs[tab]
                        .scene
                        .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    self.command_line
                        .push_output(crate::t!("PEDIT: applied.").as_ref());
                    self.refresh_properties();
                } else {
                    self.discard_last_undo_entry(tab);
                    self.command_line.push_error(
                        crate::t!("PEDIT: operation not applicable to this entity.").as_ref(),
                    );
                }
            }
        }
        if let Some(prompt) = self.tabs[tab]
            .active_cmd
            .as_ref()
            .map(|command| command.prompt())
        {
            self.command_line.push_info(&prompt);
        }
        Task::none()
    }

    fn apply_continuous_constraints(&mut self, i: usize, new_handles: &[acadrust::Handle]) {
        let supported_creation = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| {
                matches!(
                    command.name(),
                    "LINE"
                        | "PLINE"
                        | "POLYLINE"
                        | "RECTANG"
                        | "RECTANGLE"
                        | "POLYGON"
                        | "CIRCLE"
                        | "ARC"
                )
            });
        if supported_creation {
            self.apply_inferred_constraints(i, new_handles);
        }
    }

    pub(in crate::app) fn apply_inferred_constraints(
        &mut self,
        i: usize,
        changed_handles: &[acadrust::Handle],
    ) {
        if !self.constraint_infer || changed_handles.is_empty() {
            return;
        }
        let scope = self.tabs[i].current_parametric_scope();
        let owner = scope.owner_handle(&self.tabs[i].scene.document);
        let candidates = self.tabs[i]
            .scene
            .document
            .block_records
            .iter()
            .find(|record| record.handle == owner)
            .map(|record| record.entity_handles.clone())
            .unwrap_or_default();
        let inferred = self.tabs[i].scene.inferred_parametric_constraints(
            scope,
            &candidates,
            &self.auto_constrain_settings,
        );
        let inferred: Vec<_> = inferred
            .into_iter()
            .filter(|(_, refs)| {
                refs.iter()
                    .any(|reference| changed_handles.contains(&reference.entity))
            })
            .collect();
        if inferred.is_empty() {
            return;
        }
        let before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, before);
        let mut touched = Vec::new();
        for (kind, refs) in inferred {
            touched.extend(refs.iter().map(|reference| reference.entity));
            let id = self.tabs[i]
                .scene
                .parametric_constraint_set_mut(scope)
                .add(kind, refs, None);
            self.tabs[i].scene.note_parametric_constraint_applied(
                scope,
                id,
                self.constraint_bar_display,
            );
        }
        touched.sort_unstable_by_key(|handle| handle.value());
        touched.dedup();
        let changes: Vec<_> = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities_with_parametric_policy(
            &changes,
            &[],
            self.constraint_solve_mode,
        );
    }

    fn apply_cmd_result_inner(&mut self, result: CmdResult) -> Task<Message> {
        let i = self.active_tab;
        if let Some(bind) = self.tabs[i]
            .active_cmd
            .as_ref()
            .and_then(|command| command.nested_copy_bind_setting())
        {
            if self.ncopy_bind != bind {
                self.ncopy_bind = bind;
                self.persist_settings_if_changed();
            }
        }
        let preserve_commit_style = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.preserve_commit_style());
        let preserve_commit_layer = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.preserve_commit_layer());
        // Task produced by a command the arm dispatches; it must reach the
        // runtime or messages such as a chosen render mode are dropped.
        let mut dispatched = Task::none();
        match result {
            CmdResult::OpenAutoConstrainSettings => {
                self.auto_constrain_saved = Some(self.auto_constrain_settings.clone());
                self.auto_constrain_selected_row = 0;
                self.auto_constrain_distance_input =
                    format!("{}", self.auto_constrain_settings.distance_tolerance);
                self.auto_constrain_angle_input =
                    format!("{}", self.auto_constrain_settings.angle_tolerance_deg);
                self.active_modal = Some(super::ModalKind::AutoConstrainSettings);
            }
            CmdResult::NeedPoint => {
                // ATTEDIT finished its entity pick: hand the chosen block off to
                // the attribute editor dialog and end the command (open_attribute
                // _editor reports "no attributes" / "select a block" as needed).
                let attedit_handle = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|c| c.attedit_pending_handle());
                if let Some(ins_handle) = attedit_handle {
                    self.tabs[i].active_cmd = None;
                    self.command_line.set_step_options(Vec::new());
                    self.open_attribute_editor(ins_handle);
                    return Task::none();
                }
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
                let opts = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.options())
                    .unwrap_or_default();
                self.command_line.set_step_options(opts);
                if !self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|c| c.entity_pick_highlights_hover())
                {
                    self.tabs[i].scene.set_hover_highlight(None);
                }
                // The command may have advanced to a step with a different
                // dynamic-input shape (e.g. FILLET object-pick → radius entry).
                // Rebuild the fields now so the matching box appears immediately
                // and typed digits land in it rather than the command line,
                // instead of waiting for the next cursor move to resync.
                self.sync_dyn_fields();
                self.refresh_area_preview(i);
            }
            CmdResult::Preview(wire) => {
                self.tabs[i].scene.set_preview_wires(vec![wire]);
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }
            CmdResult::InterimWire(wire) => {
                self.tabs[i].scene.set_interim_wire(wire);
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }
            CmdResult::CommitEntity(entity) => {
                let source_handle = entity.common().handle;
                if !source_handle.is_null()
                    && self.tabs[i]
                        .scene
                        .document
                        .get_entity(source_handle)
                        .is_some()
                    && self.reject_locked_edit(i, source_handle)
                {
                    return Task::none();
                }
                // A line/arc drawn by a repeating command advances the ARC_CONT
                // continuation anchor, so ending one run and launching another
                // keeps continuing from the last segment (mirrors the
                // CommitAndExit arm). Non-continuable repeats (point/ray)
                // leave the anchor untouched. (#327)
                if matches!(
                    entity,
                    acadrust::EntityType::Line(_)
                        | acadrust::EntityType::Arc(_)
                        | acadrust::EntityType::LwPolyline(_)
                        | acadrust::EntityType::Polyline2D(_)
                ) {
                    self.update_cont_anchor(&entity);
                }
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                // Ordinary drawables, viewports and raster images use targeted
                // entity/object deltas. Block sentinels and novel layers retain
                // the structure snapshot fallback.
                let delta_safe = self.delta_add_safe(i, &entity);
                let pending = self.begin_undo(i, label, 1, delta_safe);
                let is_associative_dimension = matches!(
                    entity,
                    acadrust::EntityType::Dimension(
                        acadrust::entities::Dimension::Linear(_)
                            | acadrust::entities::Dimension::Aligned(_)
                    )
                );
                let association_enabled =
                    self.tabs[i].scene.document.header.dimension_associativity == 2;
                let committed = if preserve_commit_layer {
                    self.commit_entity_handle_preserve_layer(entity)
                } else {
                    self.commit_entity_handle(entity)
                };
                if is_associative_dimension && association_enabled {
                    if let Some(handle) = committed {
                        let sources = self.infer_dimension_sources_guarded(i, handle);
                        self.tabs[i]
                            .scene
                            .attach_dimension_association(handle, sources);
                    }
                }
                if let Some(handle) = committed {
                    self.apply_continuous_constraints(i, &[handle]);
                }
                self.tabs[i].dirty = true;
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitEntities(mut entities) => {
                let locked_source = entities.iter().find_map(|entity| {
                    let handle = entity.common().handle;
                    (!handle.is_null()
                        && self.tabs[i].scene.document.get_entity(handle).is_some()
                        && self.tabs[i].scene.is_layer_locked(handle))
                    .then_some(handle)
                });
                if let Some(handle) = locked_source {
                    self.reject_locked_edit(i, handle);
                    return Task::none();
                }
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let symbol_names = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.nested_copy_symbol_names())
                    .cloned();
                let delta_safe = symbol_names.is_none()
                    && entities.iter().all(|entity| self.delta_add_safe(i, entity));
                let pending = self.begin_undo(i, label, entities.len(), delta_safe);
                if let Some(names) = symbol_names {
                    let retained = self.tabs[i]
                        .scene
                        .document
                        .localize_nested_copy_symbols(&mut entities, &names);
                    if retained > 0 {
                        self.command_line.push_info(&format!("{} copied object(s) retain imported style references whose dependencies cannot be localized.", retained));
                    }
                    self.refresh_layer_panel();
                }
                let mut committed = Vec::new();
                for entity in entities {
                    let handle = if preserve_commit_style {
                        self.commit_entity_handle_preserve_style(entity)
                    } else if preserve_commit_layer {
                        self.commit_entity_handle_preserve_layer(entity)
                    } else {
                        self.commit_entity_handle(entity)
                    };
                    committed.extend(handle);
                }
                self.apply_continuous_constraints(i, &committed);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitEntitiesAndExit(mut entities) => {
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let symbol_names = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.nested_copy_symbol_names())
                    .cloned();
                let delta_safe = symbol_names.is_none()
                    && entities.iter().all(|entity| self.delta_add_safe(i, entity));
                let pending = self.begin_undo(i, label, entities.len(), delta_safe);
                if let Some(names) = symbol_names {
                    let retained = self.tabs[i]
                        .scene
                        .document
                        .localize_nested_copy_symbols(&mut entities, &names);
                    if retained > 0 {
                        self.command_line.push_info(&format!("{} copied object(s) retain imported style references whose dependencies cannot be localized.", retained));
                    }
                    self.refresh_layer_panel();
                }
                let mut committed = Vec::new();
                for entity in entities {
                    let handle = if preserve_commit_style {
                        self.commit_entity_handle_preserve_style(entity)
                    } else if preserve_commit_layer {
                        self.commit_entity_handle_preserve_layer(entity)
                    } else {
                        self.commit_entity_handle(entity)
                    };
                    committed.extend(handle);
                }
                self.apply_continuous_constraints(i, &committed);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::MviewCreate {
                viewport,
                preserve_view,
            } => {
                let saved_view = preserve_view.then(|| {
                    (
                        viewport.view_target.clone(),
                        viewport.view_direction.clone(),
                        viewport.view_center.clone(),
                        viewport.view_height,
                        viewport.custom_scale,
                        viewport.lens_length,
                        viewport.twist_angle,
                        viewport.status.perspective,
                    )
                });
                let label = self.history_label_from_active_cmd(i, "MVIEW");
                let pending = self.begin_undo(i, label, 1, true);
                let handle = self.commit_entity_handle(acadrust::EntityType::Viewport(viewport));
                if let (Some(handle), Some(saved)) = (handle, saved_view) {
                    if let Some(acadrust::EntityType::Viewport(viewport)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        viewport.view_target = saved.0;
                        viewport.view_direction = saved.1;
                        viewport.view_center = saved.2;
                        viewport.view_height = saved.3;
                        viewport.custom_scale = saved.4;
                        viewport.lens_length = saved.5;
                        viewport.twist_angle = saved.6;
                        viewport.status.perspective = saved.7;
                    }
                    self.tabs[i].scene.camera_generation += 1;
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                if let Some(pending) = pending {
                    self.commit_undo_delta(i, pending);
                }
            }
            CmdResult::MviewCreateClipped {
                boundary,
                boundary_handle,
            } => {
                if boundary.is_none() {
                    let scene = &self.tabs[i].scene;
                    let valid = scene.entity_belongs_to_current_layout(boundary_handle)
                        && scene
                            .document
                            .get_entity(boundary_handle)
                            .is_some_and(|entity| match entity {
                                acadrust::EntityType::Circle(_) => true,
                                acadrust::EntityType::Ellipse(ellipse) => ellipse.is_full(),
                                acadrust::EntityType::LwPolyline(polyline) => polyline.is_closed,
                                acadrust::EntityType::Polyline(polyline) => polyline.is_closed(),
                                acadrust::EntityType::Polyline2D(polyline) => polyline.is_closed(),
                                acadrust::EntityType::Polyline3D(polyline) => polyline.flags.closed,
                                _ => false,
                            });
                    if !valid {
                        self.command_line.push_error(
                            crate::t!("MVIEW Object: select a closed paper-space circle, ellipse, or polyline.").as_ref(),
                        );
                        if let Some(prompt) = self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .map(|command| command.prompt())
                        {
                            self.command_line.push_info(&prompt);
                        }
                        return Task::none();
                    }
                }

                let created_boundary = boundary.is_some();
                let touched = 2;
                let label = self.history_label_from_active_cmd(i, "MVIEW");
                let pending = self.begin_undo(i, label, touched, true);
                let clip_handle = match boundary {
                    Some(mut boundary) => {
                        // A non-rectangular viewport owns a helper boundary
                        // entity through `clip_boundary_handle`. Keep that
                        // helper in the document for DWG compatibility and
                        // stencil clipping, but do not expose it as a separate
                        // selectable polyline.
                        boundary.common_mut().invisible = true;
                        match self.commit_entity_handle(boundary) {
                            Some(handle) => handle,
                            None => {
                                self.tabs[i].active_cmd = None;
                                if let Some(pending) = pending {
                                    self.commit_undo_delta(i, pending);
                                }
                                return Task::none();
                            }
                        }
                    }
                    None => boundary_handle,
                };
                let polygon = self.tabs[i].scene.clip_boundary_polygon(clip_handle, 0.0);
                let bounds: Option<(f64, f64, f64, f64)> =
                    polygon.iter().fold(None, |bounds, point| {
                        if !point[0].is_finite() || !point[1].is_finite() {
                            return bounds;
                        }
                        Some(match bounds {
                            Some((min_x, min_y, max_x, max_y)) => (
                                min_x.min(point[0] as f64),
                                min_y.min(point[1] as f64),
                                max_x.max(point[0] as f64),
                                max_y.max(point[1] as f64),
                            ),
                            None => (
                                point[0] as f64,
                                point[1] as f64,
                                point[0] as f64,
                                point[1] as f64,
                            ),
                        })
                    });
                let Some((min_x, min_y, max_x, max_y)) = bounds else {
                    self.command_line.push_error(
                        crate::t!("MVIEW: the clipping boundary has no usable area.").as_ref(),
                    );
                    self.tabs[i].active_cmd = None;
                    if let Some(pending) = pending {
                        self.commit_undo_delta(i, pending);
                    }
                    return Task::none();
                };
                if max_x - min_x < 1e-6 || max_y - min_y < 1e-6 {
                    self.command_line.push_error(
                        crate::t!("MVIEW: the clipping boundary has no usable area.").as_ref(),
                    );
                    self.tabs[i].active_cmd = None;
                    if let Some(pending) = pending {
                        self.commit_undo_delta(i, pending);
                    }
                    return Task::none();
                }

                let mut viewport = acadrust::entities::Viewport::new();
                viewport.center = acadrust::types::Vector3::new(
                    (min_x + max_x) / 2.0,
                    (min_y + max_y) / 2.0,
                    0.0,
                );
                viewport.width = max_x - min_x;
                viewport.height = max_y - min_y;
                viewport.id = 2;
                viewport.clip_boundary_handle = clip_handle;
                let viewport_handle =
                    self.commit_entity_handle(acadrust::EntityType::Viewport(viewport));
                if let Some(viewport_handle) = viewport_handle {
                    if !created_boundary {
                        let before = self.tabs[i]
                            .scene
                            .document
                            .get_entity(clip_handle)
                            .cloned()
                            .map(std::sync::Arc::new);
                        self.tabs[i].scene.record_undo_before(clip_handle, before);
                    }
                    if let Some(boundary) = self.tabs[i].scene.document.get_entity_mut(clip_handle)
                    {
                        let common = boundary.common_mut();
                        common.invisible = true;
                        if !common.reactors.contains(&viewport_handle) {
                            common.reactors.push(viewport_handle);
                        }
                    }
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(clip_handle, crate::scene::ChangeKind::Modified)]);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                if let Some(pending) = pending {
                    self.commit_undo_delta(i, pending);
                }
            }
            CmdResult::WipeoutFromPolyline {
                handle,
                erase_source,
            } => {
                let wipeout = {
                    let scene = &self.tabs[i].scene;
                    scene
                        .entity_belongs_to_active_space(handle)
                        .then(|| scene.document.get_entity(handle))
                        .flatten()
                        .and_then(crate::modules::draw::draw::wipeout::wipeout_from_polyline)
                };
                if let Some(wipeout) = wipeout {
                    if erase_source {
                        return self.apply_cmd_result(CmdResult::ReplaceMany(
                            vec![(handle, Vec::new())],
                            vec![wipeout],
                        ));
                    }
                    return self.apply_cmd_result(CmdResult::CommitAndExit(wipeout));
                }
                self.command_line.push_error(
                    crate::t!("WIPEOUT Polyline: select a straight, closed, planar 2D polyline with at least 3 non-intersecting vertices.").as_ref(),
                );
                let command = crate::modules::draw::draw::wipeout::WipeoutCommand::new_polyline();
                self.command_line
                    .push_info(&crate::command::CadCommand::prompt(&command));
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            CmdResult::MviewSwitchLayout(layout) => {
                let task = self.on_layout_switch_preserving_command(layout);
                if let Some(prompt) = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.prompt())
                {
                    self.command_line.push_info(&prompt);
                }
                return task;
            }
            CmdResult::MviewCancelToLayout(layout) => {
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                return self.on_layout_switch(layout);
            }
            CmdResult::TransformSelected(mut handles, transform) => {
                handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                if handles.is_empty() {
                    self.tabs[i].active_cmd = None;
                    return Task::none();
                }
                if matches!(&transform, crate::command::EntityTransform::Translate(_)) {
                    let scope = self.tabs[i].current_parametric_scope();
                    handles = self.tabs[i]
                        .scene
                        .parametric_connected_handles(scope, &handles, false);
                    handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                }
                let label = self.history_label_from_active_cmd(i, "MOVE");
                // A translation carries its complete connected constraint
                // component so each member keeps its own dimensions. Other
                // transforms retain their selected-object behavior.
                let pending = self.begin_undo(i, label, handles.len(), true);
                self.tabs[i].scene.transform_entities(&handles, &transform);
                self.apply_inferred_constraints(i, &handles);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.refresh_properties();
                if let Some(p) = pending {
                    self.commit_undo_delta(i, p);
                }
            }
            CmdResult::CopySelected(mut handles, transform) => {
                handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                if handles.is_empty() {
                    return Task::none();
                }
                let label = self.history_label_from_active_cmd(i, "COPY");
                // Copying a dimension clones a *D block record, so gate delta on
                // the selection being dimension-free.
                let delta_safe = self.delta_copy_safe(i, &handles);
                let pending = self.begin_undo(i, label, handles.len(), delta_safe);
                let new_handles = self.tabs[i].scene.copy_entities(&handles, &transform);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.deselect_all();
                for h in new_handles {
                    self.tabs[i].scene.select_entity(h, false);
                }
                self.tabs[i].scene.clear_preview_wire();
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CopyToClipboard { handles, base } => {
                let count = self.copy_entities_to_clipboard(i, &handles, base);
                self.command_line
                    .push_info(crate::tf!("{} object(s) copied to clipboard.", count).as_ref());
                let prompt = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.prompt());
                if let Some(prompt) = prompt {
                    self.command_line.push_info(&prompt);
                }
                self.command_line.set_step_options(
                    self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|command| command.options())
                        .unwrap_or_default(),
                );
                if !self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.entity_pick_highlights_hover())
                {
                    self.tabs[i].scene.set_hover_highlight(None);
                }
                self.sync_dyn_fields();
                self.refresh_area_preview(i);
            }
            CmdResult::CommitAndExit(mut entity) => {
                // For XATTACH: ensure the xref block definition exists before
                // committing the INSERT entity that references it.
                // Extract path early to avoid borrow conflicts.
                let xattach_path: Option<String> = {
                    let tab = &self.tabs[i];
                    if let Some(cmd) = tab.active_cmd.as_ref() {
                        if cmd.name() == "XATTACH" {
                            cmd.xattach_path()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(path) = xattach_path {
                    // XREF-Task6: the host drawing path lives in the tab
                    // (`current_path`), not in `Scene`, so the self-attach
                    // guard runs here via the pure `is_self_attach` helper.
                    let host_file = self.tabs[i].current_path.clone();
                    let host_base = host_file
                        .as_deref()
                        .and_then(|p| p.parent())
                        .map(|p| p.to_path_buf());
                    if let Some(host) = host_file.as_deref() {
                        if crate::modules::insert::xattach::is_self_attach(
                            host,
                            &path,
                            host_base.as_deref(),
                        ) {
                            self.command_line.push_error(crate::t!("XATTACH: cannot attach the host drawing into itself.").as_ref());
                            self.tabs[i].scene.clear_preview_wire();
                            self.tabs[i].active_cmd = None;
                            self.tabs[i].snap_result = None;
                            self.restore_pre_cmd_tangent();
                            return Task::none();
                        }
                    }
                    let prepared_name = crate::modules::insert::xattach::prepare_xref_block(
                        &mut self.tabs[i].scene,
                        &path,
                        host_base.as_deref(),
                    );
                    // `prepare_xref_block` may suffix a colliding stem. The
                    // command was created before that collision was known, so
                    // retarget its pending INSERT to the actual new block.
                    if let acadrust::EntityType::Insert(insert) = &mut entity {
                        insert.block_name = prepared_name;
                    }
                    // Resolving the xref merged its layer / linetype tables
                    // into the document — mirror them into the Layers panel
                    // and ribbon dropdowns now, not on the next reopen (#407).
                    self.refresh_layer_panel();
                }
                let insert_block_name = match &entity {
                    acadrust::EntityType::Insert(ins) => Some(ins.block_name.clone()),
                    _ => None,
                };
                // Record where this draw ended so ARC_CONT can continue from it
                // (before `entity` is moved into commit_entity).
                self.update_cont_anchor(&entity);
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let delta_safe = self.delta_add_safe(i, &entity);
                let pending = self.begin_undo(i, label, 1, delta_safe);
                let is_associative_dimension = matches!(
                    entity,
                    acadrust::EntityType::Dimension(
                        acadrust::entities::Dimension::Linear(_)
                            | acadrust::entities::Dimension::Aligned(_)
                    )
                );
                let association_enabled =
                    self.tabs[i].scene.document.header.dimension_associativity == 2;
                let committed = self.commit_entity_handle(entity);
                if is_associative_dimension && association_enabled {
                    if let Some(handle) = committed {
                        let sources = self.infer_dimension_sources_guarded(i, handle);
                        self.tabs[i]
                            .scene
                            .attach_dimension_association(handle, sources);
                    }
                }
                if let Some(handle) = committed {
                    self.apply_continuous_constraints(i, &[handle]);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                if let Some(name) = insert_block_name {
                    self.record_block_insert(&name);
                }
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitDimension {
                mut entity,
                association,
                preserve_base_style,
                continue_command,
            } => {
                let label = self.history_label_from_active_cmd(i, "DIMENSION");
                let association_mode = self.tabs[i].scene.document.header.dimension_associativity;
                let single_source_dimension = matches!(
                    &entity,
                    acadrust::EntityType::Dimension(acadrust::entities::Dimension::Ordinate(_))
                );
                // A dimension placed on the sheet but measuring model geometry
                // through a viewport carries the compensation as a negative
                // DIMLFAC override.
                if !preserve_base_style {
                    crate::scene::creation_style::apply_current_creation_styles(
                        &self.tabs[i].scene.document,
                        &mut entity,
                    );
                }
                if !self.apply_viewport_dimension_measurement(i, &mut entity) {
                    return Task::none();
                }
                // Projected points cannot use direct paper-space source inference.
                let association_allowed = self.dimension_association_allowed(i);
                let inherited_dimension = if preserve_base_style {
                    match &entity {
                        acadrust::EntityType::Dimension(dimension) => Some((
                            dimension.base().common.layer.clone(),
                            dimension.base().style_name.clone(),
                        )),
                        _ => None,
                    }
                } else {
                    None
                };
                let pending = if association_mode == 0 {
                    let layer = self.tabs[i].active_layer.clone();
                    if layer != "0" || entity.as_entity().layer().is_empty() {
                        entity.as_entity_mut().set_layer(layer);
                    }
                    crate::scene::view::dispatch::apply_color(
                        &mut entity,
                        self.ribbon.active_color,
                    );
                    crate::scene::view::dispatch::apply_common_prop(
                        &mut entity,
                        "linetype",
                        &self.ribbon.active_linetype.clone(),
                    );
                    crate::scene::view::dispatch::apply_line_weight(
                        &mut entity,
                        self.ribbon.active_lineweight,
                    );
                    let celtscale = self.tabs[i]
                        .scene
                        .document
                        .header
                        .current_entity_linetype_scale;
                    if (celtscale - 1.0).abs() > 1e-9 && celtscale.abs() > 1e-9 {
                        entity.common_mut().linetype_scale = celtscale;
                    }
                    crate::scene::creation_style::apply_current_creation_styles(
                        &self.tabs[i].scene.document,
                        &mut entity,
                    );
                    if let (Some((layer, style_name)), acadrust::EntityType::Dimension(dimension)) =
                        (inherited_dimension, &mut entity)
                    {
                        dimension.base_mut().common.layer = layer;
                        dimension.base_mut().style_name = style_name;
                    }
                    let pieces = crate::modules::draw::modify::explode::explode_entity(
                        &entity,
                        &self.tabs[i].scene.document,
                    );
                    let delta_safe = pieces.iter().all(|piece| self.delta_add_safe(i, piece));
                    let pending = self.begin_undo(i, label, pieces.len(), delta_safe);
                    for piece in pieces {
                        self.tabs[i].scene.add_entity(piece);
                    }
                    pending
                } else {
                    let delta_safe = self.delta_add_safe(i, &entity);
                    let pending = self.begin_undo(i, label, 1, delta_safe);
                    if let Some(handle) =
                        self.commit_entity_handle_with_dimension_policy(entity, preserve_base_style)
                    {
                        if association_mode == 2 && association_allowed {
                            let mut changes = vec![(handle, crate::scene::ChangeKind::Modified)];
                            match association {
                                crate::command::DimensionAssociationInput::Infer(source) => {
                                    let sources: Vec<_> = source.map_or_else(
                                        || self.infer_dimension_sources_guarded(i, handle),
                                        |source| {
                                            if single_source_dimension {
                                                vec![Some(source)]
                                            } else {
                                                vec![Some(source), Some(source)]
                                            }
                                        },
                                    );
                                    self.tabs[i]
                                        .scene
                                        .attach_dimension_association(handle, sources.clone());
                                    changes.extend(sources.into_iter().flatten().map(|source| {
                                        (source, crate::scene::ChangeKind::Modified)
                                    }));
                                }
                                crate::command::DimensionAssociationInput::Explicit(sources) => {
                                    self.tabs[i].scene.attach_dimension_association_sources(
                                        handle,
                                        sources.clone(),
                                    );
                                    changes.extend(sources.into_iter().flatten().map(|source| {
                                        (source.handle, crate::scene::ChangeKind::Modified)
                                    }));
                                }
                            }
                            changes.sort_by_key(|(handle, _)| handle.value());
                            changes.dedup_by_key(|(handle, _)| handle.value());
                            self.tabs[i].scene.bump_entities(&changes);
                        } else if association_mode == 2 {
                            // Preserve the acquired viewport and source paths.
                            self.attach_viewport_dimension_association(i, handle);
                        }
                    }
                    pending
                };
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].snap_result = None;
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
                if continue_command {
                    if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|cmd| cmd.prompt()) {
                        self.command_line.push_info(&prompt);
                    }
                } else {
                    self.tabs[i].active_cmd = None;
                    self.restore_pre_cmd_tangent();
                }
            }
            CmdResult::CommitDimensionsAndExit(dimensions) => {
                let association_mode = self.tabs[i].scene.document.header.dimension_associativity;
                let mut made = 0;
                let pending = if association_mode == 0 {
                    let mut pieces = Vec::new();
                    for (mut entity, _) in dimensions {
                        let layer = self.tabs[i].active_layer.clone();
                        if layer != "0" || entity.as_entity().layer().is_empty() {
                            entity.as_entity_mut().set_layer(layer);
                        }
                        crate::scene::view::dispatch::apply_color(
                            &mut entity,
                            self.ribbon.active_color,
                        );
                        crate::scene::view::dispatch::apply_common_prop(
                            &mut entity,
                            "linetype",
                            &self.ribbon.active_linetype.clone(),
                        );
                        crate::scene::view::dispatch::apply_line_weight(
                            &mut entity,
                            self.ribbon.active_lineweight,
                        );
                        let celtscale = self.tabs[i]
                            .scene
                            .document
                            .header
                            .current_entity_linetype_scale;
                        if (celtscale - 1.0).abs() > 1e-9 && celtscale.abs() > 1e-9 {
                            entity.common_mut().linetype_scale = celtscale;
                        }
                        crate::scene::creation_style::apply_current_creation_styles(
                            &self.tabs[i].scene.document,
                            &mut entity,
                        );
                        let exploded = crate::modules::draw::modify::explode::explode_entity(
                            &entity,
                            &self.tabs[i].scene.document,
                        );
                        if !exploded.is_empty() {
                            made += 1;
                        }
                        pieces.extend(exploded);
                    }
                    let delta_safe = pieces.iter().all(|piece| self.delta_add_safe(i, piece));
                    let pending = self.begin_undo(i, "QDIM", pieces.len(), delta_safe);
                    for piece in pieces {
                        self.tabs[i].scene.add_entity(piece);
                    }
                    pending
                } else {
                    let delta_safe = dimensions
                        .iter()
                        .all(|(entity, _)| self.delta_add_safe(i, entity));
                    let pending = self.begin_undo(i, "QDIM", dimensions.len(), delta_safe);
                    for (entity, association) in dimensions {
                        let Some(handle) = self.commit_entity_handle(entity) else {
                            continue;
                        };
                        made += 1;
                        if association_mode != 2 {
                            continue;
                        }
                        let sources = match association {
                            crate::command::DimensionAssociationInput::Infer(source) => {
                                source.map_or_else(
                                    || {
                                        self.infer_dimension_sources_guarded(i, handle)
                                            .into_iter()
                                            .map(|source| {
                                                source.map(
                                                    crate::command::DimensionAssociationSource::inferred,
                                                )
                                            })
                                            .collect()
                                    },
                                    |source| {
                                        vec![Some(
                                            crate::command::DimensionAssociationSource::inferred(
                                                source,
                                            ),
                                        )]
                                    },
                                )
                            }
                            crate::command::DimensionAssociationInput::Explicit(sources) => {
                                sources
                            }
                        };
                        self.tabs[i]
                            .scene
                            .attach_dimension_association_sources(handle, sources.clone());
                        let mut changes = vec![(handle, crate::scene::ChangeKind::Modified)];
                        changes.extend(
                            sources
                                .into_iter()
                                .flatten()
                                .map(|source| (source.handle, crate::scene::ChangeKind::Modified)),
                        );
                        changes.sort_by_key(|(handle, _)| handle.value());
                        changes.dedup_by_key(|(handle, _)| handle.value());
                        self.tabs[i].scene.bump_entities(&changes);
                    }
                    pending
                };
                if made > 0 {
                    self.tabs[i].dirty = true;
                }
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.command_line
                    .push_output(crate::tf!("QDIM  {made} dimensions created.").as_ref());
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::SetQuickDimensionSnapPriority(priority) => {
                self.quick_dimension_snap_priority = priority.min(1);
                self.persist_settings_if_changed();
                let prompt = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.prompt());
                if let Some(prompt) = prompt {
                    self.command_line.push_info(&prompt);
                }
            }
            CmdResult::AlignMLeaders {
                mut handles,
                from,
                to,
            } => {
                handles.retain(|handle| {
                    !self.tabs[i].scene.is_layer_locked(*handle)
                        && matches!(
                            self.tabs[i].scene.document.get_entity(*handle),
                            Some(acadrust::EntityType::MultiLeader(_))
                        )
                });
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "MLEADERALIGN");
                    if apply_mleader_align(&mut self.tabs[i].scene, &handles, from, to) {
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::t!("MLEADERALIGN  Leaders aligned.").as_ref());
                    }
                }
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
            }
            CmdResult::CollectMLeaders { handles, point } => {
                let compatible = compatible_mleader_collect_handles(&self.tabs[i].scene, &handles);
                if compatible.len() >= 2 {
                    self.push_undo_snapshot(i, "MLEADERCOLLECT");
                    if apply_mleader_collect(&mut self.tabs[i].scene, &compatible, point) {
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::t!("MLEADERCOLLECT  Leaders collected.").as_ref());
                    }
                }
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
            }
            CmdResult::CommitSolid {
                entity,
                solid,
                history,
                erase_source,
            } => {
                let label = self.history_label_from_active_cmd(i, "SOLID");
                let erase_source = erase_source.filter(|handle| {
                    self.delete_objects != 0 && !self.tabs[i].scene.is_layer_locked(*handle)
                });
                let pending =
                    self.begin_undo(i, label, 1 + usize::from(erase_source.is_some()), true);
                let handle = self.add_solid_model(entity, *solid, history);
                if !handle.is_null() {
                    if let Some(source) = erase_source {
                        self.tabs[i].scene.erase_entities(&[source]);
                    }
                    self.tabs[i].dirty = true;
                    if let Some(pd) = pending {
                        self.commit_undo_delta(i, pd);
                    }
                }
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
            }
            CmdResult::CommitAndEditText(entity) => {
                let annotative_mleader = matches!(
                    &entity,
                    acadrust::EntityType::MultiLeader(ml) if ml.enable_annotation_scale
                );
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let delta_safe = self.delta_add_safe(i, &entity) && !annotative_mleader;
                let pending = self.begin_undo(i, label, 1, delta_safe);
                let handle = self.commit_entity_handle(entity);

                if annotative_mleader {
                    let scale_handle = self.tabs[i].scene.creation_annotation_scale_handle();
                    if let (Some(handle), Some(scale_handle)) = (handle, scale_handle) {
                        crate::scene::annotative::create_annotation_context(
                            &mut self.tabs[i].scene.document,
                            handle,
                            scale_handle,
                        );
                        self.tabs[i]
                            .scene
                            .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    }
                }

                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.ribbon.deactivate_tool();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
                if let Some(h) = handle {
                    return self.begin_text_edit(h);
                }
            }
            CmdResult::CommitManyAndEditText {
                entities,
                edit_index,
                open_editor,
            } => {
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let delta_safe = entities.iter().all(|entity| self.delta_add_safe(i, entity));
                let pending = self.begin_undo(i, label, entities.len(), delta_safe);
                let mut edit_handle = None;
                let mut leader_handle = None;
                for (idx, entity) in entities.into_iter().enumerate() {
                    let is_leader = matches!(entity, acadrust::EntityType::Leader(_));
                    let h = self.commit_entity_handle(entity);
                    if idx == edit_index {
                        edit_handle = h;
                    }
                    if is_leader {
                        leader_handle = h;
                    }
                }
                // Link the leader to its annotation so the pair edits as a unit
                // (double-click on the leader resolves to the text entity).
                if let (Some(lh), Some(ah)) = (leader_handle, edit_handle) {
                    let linked = if let Some(acadrust::EntityType::Leader(l)) =
                        self.tabs[i].scene.document.get_entity_mut(lh)
                    {
                        l.annotation_handle = ah;
                        true
                    } else {
                        false
                    };

                    if linked {
                        // The LEADER may already have received its annotation context while
                        // annotation_handle was still NULL. Refresh it now that the MTEXT link
                        // is known so the context represents the finished leader.
                        self.tabs[i].scene.sync_displayed_annotation_context(lh);

                        self.tabs[i]
                            .scene
                            .bump_entities(&[(lh, crate::scene::ChangeKind::Modified)]);
                    }
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.ribbon.deactivate_tool();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
                if open_editor {
                    if let Some(h) = edit_handle {
                        return self.begin_text_edit(h);
                    }
                }
            }
            CmdResult::CreateBlock {
                mut handles,
                name,
                base,
            } => {
                handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                if handles.is_empty() {
                    self.command_line
                        .push_info(crate::t!("No editable objects selected.").as_ref());
                    return Task::none();
                }
                self.push_undo_snapshot(i, "BLOCK");
                let ucs = self.tabs[i].ucs_xform();
                let world_to_block = ucs.to_ucs_transform_at(base);
                let block_to_world = ucs.to_wcs_transform_at(base);
                match self.tabs[i].scene.create_block_from_entities(
                    &handles,
                    &name,
                    &world_to_block,
                    &block_to_world,
                ) {
                    Ok(insert_handle) => {
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.deselect_all();
                        if !insert_handle.is_null() {
                            self.tabs[i].scene.select_entity(insert_handle, false);
                        }
                        self.tabs[i].scene.clear_preview_wire();
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        self.command_line
                            .push_output(crate::tf!("Block \"{name}\" created.").as_ref());
                        self.refresh_properties();
                    }
                    Err(err) => {
                        self.discard_last_undo_entry(i);
                        self.command_line.push_error(&err);
                        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                        if let Some(p) = prompt {
                            self.command_line.push_info(&p);
                        }
                    }
                }
            }
            CmdResult::CreateBlockWithOptions { mut options } => {
                options.handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                if options.handles.is_empty() {
                    self.command_line
                        .push_info(crate::t!("No editable objects selected.").as_ref());
                    return Task::none();
                }
                self.push_undo_snapshot(i, "BLOCK");
                let name = options.name.clone();
                match self.tabs[i].scene.create_block_with_options(*options) {
                    Ok(insert_handle) => {
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.deselect_all();
                        if !insert_handle.is_null() {
                            self.tabs[i].scene.select_entity(insert_handle, false);
                        }
                        self.tabs[i].scene.clear_preview_wire();
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        self.command_line
                            .push_output(crate::tf!("Block \"{name}\" created.").as_ref());
                        self.refresh_properties();
                        self.refresh_block_palette();
                    }
                    Err(err) => {
                        self.discard_last_undo_entry(i);
                        self.command_line.push_error(&err);
                    }
                }
            }
            CmdResult::CommitHatch(hatch) => {
                let label = self.history_label_from_active_cmd(i, "HATCH");
                let pending = self.begin_undo(i, label, 1, true);
                let layer = self.tabs[i].active_layer.clone();
                let new_handle = self.tabs[i].scene.add_hatch(hatch, Some(&layer), None);
                if !new_handle.is_null() {
                    self.tabs[i].scene.select_entity(new_handle, true);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitStyledHatch {
                hatch,
                color,
                transparency,
            } => {
                let label = self.history_label_from_active_cmd(i, "HATCH");
                let pending = self.begin_undo(i, label, 1, true);
                let layer = self.tabs[i].active_layer.clone();
                let new_handle =
                    self.tabs[i]
                        .scene
                        .add_hatch(hatch, Some(&layer), Some((color, transparency)));
                if !new_handle.is_null() {
                    self.tabs[i].scene.select_entity(new_handle, true);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitHatchWithBoundaries {
                mut hatch,
                boundaries,
                entity_style,
            } => {
                let label = self.history_label_from_active_cmd(i, "HATCH");
                let pending = self.begin_undo(i, label, boundaries.len() + 1, true);
                let mut sources = Vec::with_capacity(boundaries.len());
                for boundary in boundaries {
                    let handles = self
                        .commit_entity_handle(boundary)
                        .into_iter()
                        .collect::<Vec<_>>();
                    sources.push(handles);
                }
                if let Some(paths) = hatch.boundary_paths.as_mut() {
                    for (path, handles) in std::sync::Arc::make_mut(paths).iter_mut().zip(&sources)
                    {
                        path.boundary_handles = handles.clone();
                        path.flags.set_external(!handles.is_empty());
                    }
                }
                hatch.boundary_sources = Some(std::sync::Arc::new(sources));
                let layer = self.tabs[i].active_layer.clone();
                let new_handle = self.tabs[i]
                    .scene
                    .add_hatch(hatch, Some(&layer), entity_style);
                if !new_handle.is_null() {
                    self.tabs[i].scene.select_entity(new_handle, true);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitHatches {
                hatches,
                entity_style,
            } => {
                let label = self.history_label_from_active_cmd(i, "HATCH");
                let pending = self.begin_undo(i, label, hatches.len(), true);
                let layer = self.tabs[i].active_layer.clone();
                for hatch in hatches {
                    let new_handle =
                        self.tabs[i]
                            .scene
                            .add_hatch(hatch, Some(&layer), entity_style.clone());
                    if !new_handle.is_null() {
                        self.tabs[i].scene.select_entity(new_handle, true);
                    }
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::BatchCopy(mut handles, transforms) => {
                handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                if handles.is_empty() {
                    self.tabs[i].active_cmd = None;
                    return Task::none();
                }
                let label = self.history_label_from_active_cmd(i, "ARRAY");
                let count = transforms.len();
                // Same gate as COPY (dimension-free), sized by the total number
                // of copies the array will add.
                let delta_safe = self.delta_copy_safe(i, &handles);
                let pending = self.begin_undo(i, label.clone(), handles.len() * count, delta_safe);
                for t in &transforms {
                    self.tabs[i].scene.copy_entities(&handles, t);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                let noun = if count == 1 { "copy" } else { "copies" };
                self.command_line
                    .push_output(crate::tf!("{label}: {count} {noun} created.").as_ref());
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::ReplaceMany(replacements, additions) => {
                if let Some((handle, _)) = replacements
                    .iter()
                    .find(|(handle, _)| self.tabs[i].scene.is_layer_locked(*handle))
                {
                    self.reject_locked_edit(i, *handle);
                    self.tabs[i].active_cmd = None;
                    return Task::none();
                }
                let label = self.history_label_from_active_cmd(i, "FILLET");
                let was_catchment = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|c| c.name() == "SS_CATCHMENT");
                self.push_undo_snapshot(i, label);
                for (handle, entities) in replacements {
                    self.replace_command_entity(i, handle, entities);
                }
                for entity in additions {
                    self.tabs[i].scene.add_entity(entity);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                if was_catchment {
                    self.command_line
                        .push_info(crate::t!("Catchment tagged successfully.").as_ref());
                }
                self.refresh_properties();
            }
            CmdResult::ReplaceManyContinue(replacements) => {
                if let Some((handle, _)) = replacements
                    .iter()
                    .find(|(handle, _)| self.tabs[i].scene.is_layer_locked(*handle))
                {
                    self.reject_locked_edit(i, *handle);
                    return Task::none();
                }
                let label = self.history_label_from_active_cmd(i, "TRIM");
                self.push_undo_snapshot(i, label);
                for (handle, entities) in replacements {
                    let new_handles = self.replace_command_entity(i, handle, entities);
                    if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                        command.on_entity_replaced(handle, &new_handles);
                    }
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].snap_result = None;
                if let Some(prompt) = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.prompt())
                {
                    self.command_line.push_info(&prompt);
                }
                self.refresh_properties();
            }
            CmdResult::UpdateEntityAndFinish { handle, entity } => {
                if self.reject_locked_edit(i, handle) {
                    self.tabs[i].active_cmd = None;
                    return Task::none();
                }
                let label = self.history_label_from_active_cmd(i, "EDIT");
                self.push_undo_snapshot(i, label);
                let is_dimension = matches!(entity, acadrust::EntityType::Dimension(_));
                if let Some(current) = self.tabs[i].scene.document.get_entity_mut(handle) {
                    *current = entity;
                    if is_dimension {
                        self.tabs[i].scene.invalidate_dim_block_recorded(handle);
                    }
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    self.tabs[i].dirty = true;
                } else {
                    self.command_line.push_error(
                        crate::t!("The selected object is no longer available.").as_ref(),
                    );
                }
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.refresh_properties();
            }
            CmdResult::AddParametricConstraint {
                kind,
                refs,
                driving_param,
                label,
            } => {
                use crate::scene::parametric_constraints::ConstraintKind;
                // GCSMOOTH and FXCONSTRAINT re-prompt after a rejected pick.
                let keep_command = self.tabs[i].active_cmd.as_ref().is_some_and(|command| {
                    (kind == ConstraintKind::Smooth && command.name() == "GCSMOOTH")
                        || (kind == ConstraintKind::Fixed && command.name() == "FXCONSTRAINT")
                });
                if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
                    kind,
                    &refs,
                    driving_param.as_ref(),
                ) {
                    if !keep_command {
                        self.tabs[i].active_cmd = None;
                    }
                    self.tabs[i].snap_result = None;
                    self.command_line.push_error(message);
                    if keep_command {
                        if let Some(prompt) =
                            self.tabs[i].active_cmd.as_ref().map(|command| command.prompt())
                        {
                            self.command_line.push_info(&prompt);
                        }
                    }
                    return Task::none();
                }
                let scope = self.tabs[i].current_parametric_scope();
                if kind == ConstraintKind::Fixed
                    && self.tabs[i]
                        .scene
                        .parametric_constraint_set(scope)
                        .is_some_and(|set| {
                            set.constraints.iter().any(|existing| {
                                existing.enabled && existing.kind == kind && existing.refs == refs
                            })
                        })
                {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.command_line
                        .push_error("The constraint already exists on the selected objects.");
                    return Task::none();
                }
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let touched: Vec<Handle> = refs.iter().map(|r| r.entity).collect();
                let pending = self.begin_undo(i, label, touched.len(), true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                let retain_size = self.constraint_solve_mode && driving_param.is_none();
                let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
                    kind,
                    refs,
                    driving_param,
                );
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
                    .into_iter()
                    .map(|h| (h, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities_with_parametric_policy(
                    &changes,
                    &[],
                    retain_size,
                );
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line.push_output("Constraint applied.");
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::AddEqualConstraint {
                first,
                others,
                multiple,
                label,
            } => {
                use crate::modules::parametric::EqualConstraintCommand;
                use crate::scene::parametric_constraints::{
                    equal_size, equal_size_follower, ConstraintKind, EqualSize, ParametricRef,
                };

                let scope = self.tabs[i].current_parametric_scope();
                // Enter ends a Multiple flow with the reference's summary line.
                let finishing = multiple && others.is_empty();
                let mut followers: Vec<ParametricRef> = Vec::new();
                for other in others {
                    let refs = [first, other];
                    if other == first
                        || self
                            .tabs[i]
                            .scene
                            .validate_parametric_constraint(ConstraintKind::Equal, &refs, None)
                            .is_err()
                        || equal_size_follower(&self.tabs[i].scene.document, first, other)
                            .is_none()
                    {
                        // The reference names the kind the first object
                        // asks for.
                        let message = match equal_size(&self.tabs[i].scene.document, first) {
                            Some(EqualSize::Radius(_)) => EqualConstraintCommand::INVALID_RADIUS_OBJECT,
                            _ => EqualConstraintCommand::INVALID_LENGTH_OBJECT,
                        };
                        self.command_line.push_error(message);
                        continue;
                    }
                    let exists = self.tabs[i]
                        .scene
                        .parametric_constraint_set(scope)
                        .is_some_and(|set| {
                            set.constraints.iter().any(|existing| {
                                existing.enabled
                                    && existing.kind == ConstraintKind::Equal
                                    && (existing.refs == refs || existing.refs == [other, first])
                            })
                        });
                    if exists {
                        self.command_line
                            .push_error("The constraint already exists on the selected objects.");
                        continue;
                    }
                    followers.push(other);
                }
                // A refused second object is asked for again, as in the
                // reference; Multiple keeps asking anyway.
                let keep = (multiple && !finishing) || (!multiple && followers.is_empty());
                if keep {
                    if let Some(prompt) =
                        self.tabs[i].active_cmd.as_ref().map(|command| command.prompt())
                    {
                        self.command_line.push_info(&prompt);
                    }
                } else {
                    self.tabs[i].active_cmd = None;
                }
                self.tabs[i].snap_result = None;
                if finishing {
                    let summary = match equal_size(&self.tabs[i].scene.document, first) {
                        Some(EqualSize::Radius(_)) => "Radius of objects made equal",
                        _ => "Length of objects made equal",
                    };
                    self.command_line.push_output(summary);
                }
                if followers.is_empty() {
                    return Task::none();
                }
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let mut touched = vec![first.entity];
                for follower in &followers {
                    if !touched.contains(&follower.entity) {
                        touched.push(follower.entity);
                    }
                }
                let pending = self.begin_undo(i, label, touched.len(), true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                // The reference resizes the follower in place — its start (a
                // circle its center) and direction stay, only its length or
                // radius takes the first object's — so do that first and let
                // the relation then hold what already fits.
                for follower in &followers {
                    if let Some(resized) =
                        equal_size_follower(&self.tabs[i].scene.document, first, *follower)
                    {
                        self.tabs[i].scene.update_entity(resized);
                    }
                    let id = self.tabs[i]
                        .scene
                        .parametric_constraint_set_mut(scope)
                        .add(ConstraintKind::Equal, vec![first, *follower], None);
                    self.tabs[i].scene.note_parametric_constraint_applied(
                        scope,
                        id,
                        self.constraint_bar_display,
                    );
                }
                let changes = touched
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect::<Vec<_>>();
                self.tabs[i].scene.bump_entities_with_parametric_policy(
                    &changes,
                    &[],
                    self.constraint_solve_mode,
                );
                self.tabs[i].dirty = true;
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CheckConstraintPoint(pick) => {
                use crate::scene::parametric_constraints::{
                    nearest_parametric_point, nearest_parametric_point_on_entity, resolve_point,
                };
                let scope = self.tabs[i].current_parametric_scope();
                let resolved = {
                    let document = &self.tabs[i].scene.document;
                    let world =
                        acadrust::types::Vector3::new(pick.point.x, pick.point.y, pick.point.z);
                    let reference = match pick.handle {
                        Some(handle) => {
                            nearest_parametric_point_on_entity(document, scope, handle, world)
                        }
                        None => nearest_parametric_point(document, scope, world, None),
                    };
                    reference.map(|reference| {
                        let point = document
                            .get_entity(reference.entity)
                            .zip(reference.marker)
                            .and_then(|(entity, marker)| resolve_point(entity, marker))
                            .map_or(pick.point, |p| glam::DVec3::new(p.x, p.y, p.z));
                        (reference, point)
                    })
                };
                let Some((reference, point)) = resolved else {
                    return self.apply_cmd_result(CmdResult::ReportError(
                        crate::modules::parametric::DimConstraintCommand::NO_POINT.to_string(),
                    ));
                };
                let result = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .map(|command| command.accept_constraint_point(reference, point));
                return match result {
                    Some(result) => self.apply_cmd_result(result),
                    None => Task::none(),
                };
            }
            CmdResult::AddDimensionalConstraint {
                kind,
                first,
                second,
                first_point,
                second_point,
                location,
                axis,
                direction,
                name,
                expression,
                renamed,
                label,
            } => {
                use crate::modules::annotate::{aligned_dim, linear_dim};
                use crate::scene::named_parameters::{is_valid_name, DrivingValue};
                use crate::scene::parametric_constraints::{
                    distance_direction_type, dynamic_dimension_text, ConstraintKind,
                };

                let scope = self.tabs[i].current_parametric_scope();
                let mut refs = vec![first, second];
                refs.extend(direction);
                // The same measurement between the same references is
                // refused after the value, and the command ends.
                let duplicate = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .is_some_and(|set| {
                        set.constraints.iter().any(|c| {
                            c.enabled
                                && c.kind == kind
                                && c.refs.len() == refs.len()
                                && c.refs[..2].contains(&first)
                                && c.refs[..2].contains(&second)
                                && c.refs.get(2) == refs.get(2)
                        })
                    });
                if duplicate {
                    self.command_line
                        .push_output("The constraint already exists on the selected objects.");
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    return Task::none();
                }
                // A refused value keeps the prompt open, as the reference's
                // editor does. A directed distance validates as the plain
                // two-point distance it also is.
                let (validate_kind, validate_refs) = if kind == ConstraintKind::DistanceDirected {
                    (ConstraintKind::Distance, &refs[..2])
                } else {
                    (kind, &refs[..])
                };
                if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
                    validate_kind,
                    validate_refs,
                    Some(&DrivingValue::Literal(1.0)),
                ) {
                    self.reprompt_active_command(i, message);
                    return Task::none();
                }
                let anchors = crate::scene::parametric_constraints::dimensional_anchor_refs(
                    &self.tabs[i].scene.document,
                    &refs,
                );
                let name = name.trim().to_string();
                if !is_valid_name(&name) {
                    self.command_line.push_error(
                        "Only alphanumeric names, starting with alpha characters, are allowed.",
                    );
                    self.reprompt_active_command(i, &format!("Invalid parameter name: {name}."));
                    return Task::none();
                }
                if renamed && self.tabs[i].scene.named_parameters().contains(&name) {
                    self.reprompt_active_command(i, "Parameter with this name already exists.");
                    return Task::none();
                }
                let mut table = self.tabs[i].scene.named_parameters().clone();
                if let Err(error) = table.set(&name, &expression) {
                    self.reprompt_active_command(i, &error.to_string());
                    return Task::none();
                }
                let value = match table.resolve(&name) {
                    Ok(value) if value.is_finite() => value,
                    _ => {
                        self.command_line.push_error("Invalid expression.");
                        self.reprompt_active_command(
                            i,
                            "The parameter is used in an expression which results in an invalid value for a dimensional constraint.",
                        );
                        return Task::none();
                    }
                };
                // DCFORM Annotational: an ordinary plotted dimension on the
                // current layer that shows the value at dimension precision.
                let annotational = self.constraint_form_annotational;
                let text = dynamic_dimension_text(
                    &name,
                    value,
                    self.tabs[i].scene.constraint_name_format,
                    false,
                    Some(&expression),
                    annotational,
                    None,
                );
                let mut entity = if kind == ConstraintKind::Distance {
                    aligned_dim::aligned_dimension_entity(
                        first_point,
                        second_point,
                        location,
                        Some(text),
                    )
                } else {
                    linear_dim::linear_dimension_entity(
                        first_point,
                        second_point,
                        location,
                        axis,
                        Some(text),
                    )
                };
                crate::scene::creation_style::apply_current_creation_styles(
                    &self.tabs[i].scene.document,
                    &mut entity,
                );
                if annotational {
                    entity
                        .as_entity_mut()
                        .set_layer(self.tabs[i].active_layer.clone());
                } else {
                    // A dynamic dimension lives on the reference's constraints
                    // layer, hidden from the layer lists and never plotted, and
                    // draws in the constraint gray, not the current color.
                    self.tabs[i].scene.ensure_dynamic_dimension_layer();
                    entity.as_entity_mut().set_layer(
                        crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER.to_string(),
                    );
                    entity.common_mut().color = acadrust::types::Color::Rgb {
                        r: 103,
                        g: 109,
                        b: 118,
                    };
                }
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let mut touched = vec![first.entity];
                for reference in [Some(second), direction].into_iter().flatten() {
                    if !touched.contains(&reference.entity) {
                        touched.push(reference.entity);
                    }
                }
                let pending = self.begin_undo(i, label, touched.len() + 1, false);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                self.tabs[i].scene.record_undo_named_parameters_before();
                self.tabs[i].scene.named_parameters = table;
                let dimension = self.tabs[i].scene.add_entity(entity);
                let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
                let id = set.add(kind, refs, Some(DrivingValue::Named(name)));
                if direction.is_some() {
                    if let Some(constraint) = set.constraints.iter_mut().find(|c| c.id == id) {
                        constraint.distance_direction_type =
                            distance_direction_type::PERPENDICULAR_TO_LINE;
                    }
                }
                set.dimensions.insert(id, dimension);
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                self.tabs[i].scene.attach_dimension_association(
                    dimension,
                    vec![Some(first.entity), Some(second.entity)],
                );
                let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                // The first point (and a line it belongs to that the distance
                // runs perpendicular to) stays; a value other than the
                // measured one moves the second, as in the reference. Not
                // `retain_size`: the typed value is the new length.
                self.tabs[i]
                    .scene
                    .bump_entities_with_parametric_policy(&changes, &anchors, false);
                // DYNCONSTRAINTDISPLAY 0 also keeps a new dynamic dimension off screen.
                self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
                self.tabs[i].scene.refresh_dynamic_dimension_scales(true);
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CancelWithMessage(message) => {
                self.command_line.push_error(&message);
                return self.apply_cmd_result(CmdResult::Cancel);
            }
            CmdResult::AddRadialConstraint {
                circle,
                center,
                radius,
                location,
                diameter,
                name,
                expression,
                renamed,
            } => {
                use crate::modules::annotate::{diameter_dim, radius_dim};
                use crate::scene::named_parameters::{is_valid_name, DrivingValue};
                use crate::scene::parametric_constraints::{
                    dynamic_dimension_text, ConstraintKind, ParametricRef,
                };

                let scope = self.tabs[i].current_parametric_scope();
                let kind = if diameter {
                    ConstraintKind::Diameter
                } else {
                    ConstraintKind::Radius
                };
                let refs = vec![circle];
                // A circle already carrying a radius or diameter constraint
                // is refused after the value, as the reference does.
                let duplicate = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .is_some_and(|set| {
                        set.constraints.iter().any(|c| {
                            c.enabled
                                && matches!(
                                    c.kind,
                                    ConstraintKind::Radius | ConstraintKind::Diameter
                                )
                                && c.refs.iter().any(|r| r.entity == circle.entity)
                        })
                    });
                if duplicate {
                    self.command_line
                        .push_output("The constraint already exists on the selected objects.");
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    return Task::none();
                }
                if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
                    kind,
                    &refs,
                    Some(&DrivingValue::Literal(1.0)),
                ) {
                    self.reprompt_active_command(i, message);
                    return Task::none();
                }
                let name = name.trim().to_string();
                if !is_valid_name(&name) {
                    self.command_line.push_error(
                        "Only alphanumeric names, starting with alpha characters, are allowed.",
                    );
                    self.reprompt_active_command(i, &format!("Invalid parameter name: {name}."));
                    return Task::none();
                }
                if renamed && self.tabs[i].scene.named_parameters().contains(&name) {
                    self.reprompt_active_command(i, "Parameter with this name already exists.");
                    return Task::none();
                }
                let mut table = self.tabs[i].scene.named_parameters().clone();
                if let Err(error) = table.set(&name, &expression) {
                    self.reprompt_active_command(i, &error.to_string());
                    return Task::none();
                }
                // A circle has no zero or negative size: the value is
                // refused and the prompt comes back.
                let value = match table.resolve(&name) {
                    Ok(value) if value.is_finite() && value > 0.0 => value,
                    _ => {
                        self.command_line.push_error("Invalid expression.");
                        self.reprompt_active_command(
                            i,
                            "The parameter is used in an expression which results in an invalid value for a dimensional constraint.",
                        );
                        return Task::none();
                    }
                };
                let annotational = self.constraint_form_annotational;
                let text = dynamic_dimension_text(
                    &name,
                    value,
                    self.tabs[i].scene.constraint_name_format,
                    false,
                    Some(&expression),
                    annotational,
                    None,
                );
                let entity = if diameter {
                    diameter_dim::diameter_constraint_entity(center, radius, location, Some(text))
                } else {
                    radius_dim::radius_constraint_entity(center, radius, location, Some(text))
                };
                let Some(mut entity) = entity else {
                    self.reprompt_active_command(i, "No valid constraint point found.");
                    return Task::none();
                };
                crate::scene::creation_style::apply_current_creation_styles(
                    &self.tabs[i].scene.document,
                    &mut entity,
                );
                if annotational {
                    entity
                        .as_entity_mut()
                        .set_layer(self.tabs[i].active_layer.clone());
                } else {
                    self.tabs[i].scene.ensure_dynamic_dimension_layer();
                    entity.as_entity_mut().set_layer(
                        crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER.to_string(),
                    );
                    entity.common_mut().color = acadrust::types::Color::Rgb {
                        r: 103,
                        g: 109,
                        b: 118,
                    };
                }
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                // The centre stays where it is; the value moves the rim.
                let anchors = vec![ParametricRef::center(circle.entity)];
                let pending = self.begin_undo(
                    i,
                    if diameter {
                        "Diameter constraint"
                    } else {
                        "Radius constraint"
                    },
                    2,
                    false,
                );
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                self.tabs[i].scene.record_undo_named_parameters_before();
                self.tabs[i].scene.named_parameters = table;
                let dimension = self.tabs[i].scene.add_entity(entity);
                let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
                let id = set.add(kind, refs, Some(DrivingValue::Named(name)));
                set.dimensions.insert(id, dimension);
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                self.tabs[i]
                    .scene
                    .attach_dimension_association(dimension, vec![Some(circle.entity)]);
                let changes = vec![(circle.entity, crate::scene::ChangeKind::Modified)];
                self.tabs[i]
                    .scene
                    .bump_entities_with_parametric_policy(&changes, &anchors, false);
                self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
                self.tabs[i].scene.refresh_dynamic_dimension_scales(true);
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::AddAngularConstraint {
                refs,
                points,
                location,
                sector,
                name,
                expression,
                renamed,
            } => {
                use crate::modules::annotate::angular_dim;
                use crate::scene::named_parameters::{is_valid_name, DrivingValue};
                use crate::scene::parametric_constraints::{
                    angle_decimals, dynamic_dimension_text, ConstraintKind, ParametricRef,
                };

                let scope = self.tabs[i].current_parametric_scope();
                let kind = if refs.len() == 2 {
                    ConstraintKind::Angle
                } else {
                    ConstraintKind::Angle3Point
                };
                // The same angle between the same sides is refused after the
                // value (whichever sector), and the command ends.
                let duplicate = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .is_some_and(|set| {
                        set.constraints.iter().any(|c| {
                            c.enabled
                                && c.kind == kind
                                && c.refs.len() == refs.len()
                                && if kind == ConstraintKind::Angle {
                                    c.refs.contains(&refs[0]) && c.refs.contains(&refs[1])
                                } else {
                                    c.refs == refs
                                }
                        })
                    });
                if duplicate {
                    self.command_line
                        .push_output("The constraint already exists on the selected objects.");
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    return Task::none();
                }
                if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
                    kind,
                    &refs,
                    Some(&DrivingValue::Literal(1.0)),
                ) {
                    self.reprompt_active_command(i, message);
                    return Task::none();
                }
                let name = name.trim().to_string();
                if !is_valid_name(&name) {
                    self.command_line.push_error(
                        "Only alphanumeric names, starting with alpha characters, are allowed.",
                    );
                    self.reprompt_active_command(i, &format!("Invalid parameter name: {name}."));
                    return Task::none();
                }
                if renamed && self.tabs[i].scene.named_parameters().contains(&name) {
                    self.reprompt_active_command(i, "Parameter with this name already exists.");
                    return Task::none();
                }
                let mut table = self.tabs[i].scene.named_parameters().clone();
                if let Err(error) = table.set(&name, &expression) {
                    self.reprompt_active_command(i, &error.to_string());
                    return Task::none();
                }
                let value = match table.resolve(&name) {
                    Ok(value) if value.is_finite() => value,
                    _ => {
                        self.command_line.push_error("Invalid expression.");
                        self.reprompt_active_command(
                            i,
                            "The parameter is used in an expression which results in an invalid value for a dimensional constraint.",
                        );
                        return Task::none();
                    }
                };
                let annotational = self.constraint_form_annotational;
                let decimals = angle_decimals(&self.tabs[i].scene.document, None);
                let text = dynamic_dimension_text(
                    &name,
                    value,
                    self.tabs[i].scene.constraint_name_format,
                    false,
                    Some(&expression),
                    annotational,
                    Some(decimals),
                );
                let entity = if kind == ConstraintKind::Angle {
                    angular_dim::angular_two_line_entity(
                        points[0], points[1], points[2], points[3], location, Some(text),
                    )
                } else {
                    angular_dim::angular_three_point_entity(
                        points[0], points[1], points[2], location, Some(text),
                    )
                };
                let Some(mut entity) = entity else {
                    self.reprompt_active_command(i, "No valid constraint point found.");
                    return Task::none();
                };
                crate::scene::creation_style::apply_current_creation_styles(
                    &self.tabs[i].scene.document,
                    &mut entity,
                );
                if annotational {
                    entity
                        .as_entity_mut()
                        .set_layer(self.tabs[i].active_layer.clone());
                } else {
                    self.tabs[i].scene.ensure_dynamic_dimension_layer();
                    entity.as_entity_mut().set_layer(
                        crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER.to_string(),
                    );
                    entity.common_mut().color = acadrust::types::Color::Rgb {
                        r: 103,
                        g: 109,
                        b: 118,
                    };
                }
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let mut touched: Vec<Handle> = Vec::new();
                for reference in &refs {
                    if !touched.contains(&reference.entity) {
                        touched.push(reference.entity);
                    }
                }
                // The first side (or the vertex and first point) stays; a
                // value other than the measured one turns the other side.
                let anchors: Vec<ParametricRef> =
                    crate::scene::parametric_constraints::dimensional_anchor_refs(
                        &self.tabs[i].scene.document,
                        &refs,
                    );
                let association = if kind == ConstraintKind::Angle {
                    vec![
                        Some(refs[0].entity),
                        Some(refs[0].entity),
                        Some(refs[1].entity),
                        Some(refs[1].entity),
                    ]
                } else {
                    vec![
                        Some(refs[1].entity),
                        Some(refs[0].entity),
                        Some(refs[2].entity),
                    ]
                };
                let pending = self.begin_undo(i, "Angular constraint", touched.len() + 1, false);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                self.tabs[i].scene.record_undo_named_parameters_before();
                self.tabs[i].scene.named_parameters = table;
                let dimension = self.tabs[i].scene.add_entity(entity);
                let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
                let id = set.add(kind, refs, Some(DrivingValue::Named(name)));
                if let Some(constraint) = set.constraints.iter_mut().find(|c| c.id == id) {
                    constraint.angle_sector = sector;
                }
                set.dimensions.insert(id, dimension);
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                self.tabs[i]
                    .scene
                    .attach_dimension_association(dimension, association);
                let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i]
                    .scene
                    .bump_entities_with_parametric_policy(&changes, &anchors, false);
                self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
                self.tabs[i].scene.refresh_dynamic_dimension_scales(true);
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::MakeParallel {
                first_line,
                first_ends,
                first_pick,
                second_line,
                second_ends,
            } => {
                use crate::scene::parametric_constraints::{
                    resolve_point, two_lines_placed_entity, ConstraintKind,
                };
                let scope = self.tabs[i].current_parametric_scope();
                let refs = [first_line, second_line];
                if let Err(message) =
                    self.tabs[i]
                        .scene
                        .validate_parametric_constraint(ConstraintKind::Parallel, &refs, None)
                {
                    return self.apply_cmd_result(CmdResult::ReportError(message.to_string()));
                }
                // The reference's placement of the second line, written
                // before the Parallel constraint holds it there. Built out of
                // line and boxed: this arm's locals would otherwise sit in
                // `apply_cmd_result`'s frame, which is on every click's stack.
                let placed_entity = two_lines_placed_entity(
                    &self.tabs[i].scene.document,
                    first_ends,
                    first_pick,
                    second_line,
                    second_ends,
                );
                let already = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .is_some_and(|set| {
                        set.constraints.iter().any(|c| {
                            c.enabled
                                && c.kind == ConstraintKind::Parallel
                                && c.refs.contains(&first_line)
                                && c.refs.contains(&second_line)
                        })
                    });
                if !already || placed_entity.is_some() {
                    let constraints_before = self.tabs[i]
                        .scene
                        .parametric_constraint_set(scope)
                        .cloned()
                        .unwrap_or_else(|| {
                            crate::scene::parametric_constraints::ParametricConstraintSet::new(
                                scope,
                            )
                        });
                    let pending = self.begin_undo(i, "Parallel constraint", 1, true);
                    if let Some(entity) = placed_entity {
                        self.tabs[i].scene.replace_entity_recorded(entity);
                    }
                    self.tabs[i]
                        .scene
                        .record_undo_parametric_constraints_before(scope, constraints_before);
                    if !already {
                        let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
                        let id = set.add(ConstraintKind::Parallel, refs.to_vec(), None);
                        self.tabs[i].scene.note_parametric_constraint_applied(
                            scope,
                            id,
                            self.constraint_bar_display,
                        );
                    }
                    // Both lines' ends hold the placement; the solve only
                    // settles whatever else is tied to them.
                    let mut pins = first_ends.to_vec();
                    pins.extend(second_ends);
                    self.tabs[i].scene.bump_entities_with_parametric_policy(
                        &[(second_line.entity, crate::scene::ChangeKind::Modified)],
                        &pins,
                        self.constraint_solve_mode,
                    );
                    self.tabs[i].dirty = true;
                    if let Some(pd) = pending {
                        self.commit_undo_delta(i, pd);
                    }
                }
                let ends = {
                    let document = &self.tabs[i].scene.document;
                    document.get_entity(second_line.entity).and_then(|entity| {
                        let world = |reference: crate::scene::parametric_constraints::ParametricRef| {
                            let point = resolve_point(entity, reference.marker?)?;
                            Some((reference, glam::DVec3::new(point.x, point.y, point.z)))
                        };
                        Some([world(second_ends[0])?, world(second_ends[1])?])
                    })
                };
                let Some(ends) = ends else {
                    return self.apply_cmd_result(CmdResult::ReportError(
                        crate::modules::parametric::DimConstraintCommand::NO_OBJECT.to_string(),
                    ));
                };
                let result = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .map(|command| command.accept_parallel_line(ends));
                return match result {
                    Some(result) => self.apply_cmd_result(result),
                    None => Task::none(),
                };
            }
            CmdResult::AddFixedConstraint(pick) => {
                use crate::modules::parametric::FixConstraintCommand;
                use crate::scene::parametric_constraints::{
                    nearest_parametric_point, nearest_parametric_point_on_entity,
                    parametric_curve_ref_for_pick, ConstraintKind,
                };

                let scope = self.tabs[i].current_parametric_scope();
                let document = &self.tabs[i].scene.document;
                let world =
                    acadrust::types::Vector3::new(pick.point.x, pick.point.y, pick.point.z);
                let resolved = match (pick.whole_curve, pick.handle) {
                    (true, Some(handle)) => {
                        parametric_curve_ref_for_pick(document, scope, handle, world)
                            .ok_or(FixConstraintCommand::INVALID_OBJECT)
                    }
                    (true, None) => Err(FixConstraintCommand::NO_OBJECT),
                    (false, Some(handle)) => {
                        nearest_parametric_point_on_entity(document, scope, handle, world)
                            .ok_or(FixConstraintCommand::INVALID_OBJECT)
                    }
                    (false, None) => nearest_parametric_point(document, scope, world, None)
                        .ok_or(FixConstraintCommand::NO_POINT),
                };
                // An off-plane or 3D curve is "not a valid object" to the
                // reference, not a solver limitation.
                let resolved = resolved.and_then(|reference| {
                    let supported = !pick.whole_curve
                        || self.tabs[i]
                            .scene
                            .validate_parametric_constraint(
                                ConstraintKind::Fixed,
                                &[reference],
                                None,
                            )
                            .is_ok();
                    supported
                        .then_some(reference)
                        .ok_or(FixConstraintCommand::INVALID_OBJECT)
                });
                return match resolved {
                    Ok(reference) => self.apply_cmd_result(CmdResult::AddParametricConstraint {
                        kind: ConstraintKind::Fixed,
                        refs: vec![reference],
                        driving_param: None,
                        label: "Fixed constraint",
                    }),
                    // A miss re-prompts: `ReportError` keeps the command.
                    Err(message) => {
                        self.apply_cmd_result(CmdResult::ReportError(message.to_string()))
                    }
                };
            }
            CmdResult::CheckHorizontalPoint { kind, pick } => {
                use crate::modules::parametric::HorizontalConstraintCommand;
                use crate::scene::parametric_constraints::nearest_parametric_point;

                // The reference rejects a missed first point right away and
                // asks for it again; a hit moves on to the second point.
                let scope = self.tabs[i].current_parametric_scope();
                let found = nearest_parametric_point(
                    &self.tabs[i].scene.document,
                    scope,
                    acadrust::types::Vector3::new(pick.point.x, pick.point.y, pick.point.z),
                    None,
                )
                .is_some();
                if !found {
                    self.command_line
                        .push_error("No valid constraint point found.");
                }
                let command = HorizontalConstraintCommand::resume(kind, found.then_some(pick));
                self.command_line
                    .push_info(&crate::command::CadCommand::prompt(&command));
                self.tabs[i].active_cmd = Some(Box::new(command));
                self.tabs[i].snap_result = None;
                return Task::none();
            }
            CmdResult::AddHorizontalConstraint {
                kind,
                selection,
                direction,
                label,
            } => {
                use crate::command::{EntityTransform, HorizontalConstraintSelection};
                use crate::modules::parametric::HorizontalConstraintCommand;
                use crate::scene::parametric_constraints::{
                    nearest_parametric_point, nearest_parametric_point_on_entity, resolve_point,
                    ConstraintKind, DirectionalAxis, ParametricRef,
                };

                let vertical = kind == ConstraintKind::Vertical;
                let axis = if vertical { "Vertical" } else { "Horizontal" };
                let scope = self.tabs[i].current_parametric_scope();
                let to_world = |point: glam::DVec3| {
                    acadrust::types::Vector3::new(point.x, point.y, point.z)
                };
                let (refs, initial_fixed) = match selection {
                    HorizontalConstraintSelection::Reference(reference) => {
                        // The reference turns the object about its first
                        // vertex: a line's start, the picked polyline
                        // segment's first vertex, a text's insertion point,
                        // an ellipse's center.
                        let initial_fixed = match reference.directional_axis() {
                            Some(DirectionalAxis::EllipseMajor | DirectionalAxis::EllipseMinor) => {
                                vec![ParametricRef::center(reference.entity)]
                            }
                            Some(DirectionalAxis::TextBaseline) => {
                                vec![ParametricRef::point(reference.entity, 0)]
                            }
                            None => vec![ParametricRef::point(
                                reference.entity,
                                reference.segment_index().map_or(0, |index| index as i32),
                            )],
                        };
                        (vec![reference], initial_fixed)
                    }
                    HorizontalConstraintSelection::Points(first, second) => {
                        let resolve = |pick: crate::command::CoincidentPick| {
                            if let Some(handle) = pick.handle {
                                nearest_parametric_point_on_entity(
                                    &self.tabs[i].scene.document,
                                    scope,
                                    handle,
                                    to_world(pick.point),
                                )
                            } else {
                                nearest_parametric_point(
                                    &self.tabs[i].scene.document,
                                    scope,
                                    to_world(pick.point),
                                    None,
                                )
                            }
                        };
                        // A miss asks for that point again, keeping a good
                        // first pick; the same point twice asks for another
                        // second point — the reference's own re-prompts.
                        let (first_ref, second_ref) = match (resolve(first), resolve(second)) {
                            (Some(first_ref), Some(second_ref)) if first_ref != second_ref => {
                                (first_ref, second_ref)
                            }
                            (first_ref, second_ref) => {
                                let (message, keep_first) = if first_ref.is_none() {
                                    ("No valid constraint point found.", None)
                                } else if second_ref.is_none() {
                                    ("No valid constraint point found.", Some(first))
                                } else {
                                    (
                                        "The object or point is already selected.  Select a different object or constraint point.",
                                        Some(first),
                                    )
                                };
                                let command = HorizontalConstraintCommand::resume(kind, keep_first);
                                self.command_line.push_error(message);
                                self.command_line
                                    .push_info(&crate::command::CadCommand::prompt(&command));
                                self.tabs[i].active_cmd = Some(Box::new(command));
                                self.tabs[i].snap_result = None;
                                return Task::none();
                            }
                        };
                        (vec![first_ref, second_ref], vec![first_ref])
                    }
                };
                let axis_length = direction.x.hypot(direction.y);
                if axis_length <= 1.0e-12 {
                    self.command_line.push_error(&format!(
                        "{axis}: the current UCS {} axis is not supported.",
                        if vertical { "Y" } else { "X" }
                    ));
                    return Task::none();
                }
                let direction = acadrust::types::Vector3::new(
                    direction.x / axis_length,
                    direction.y / axis_length,
                    0.0,
                );
                if let Err(message) =
                    self.tabs[i].scene.validate_parametric_constraint(kind, &refs, None)
                {
                    self.command_line.push_error(message);
                    return Task::none();
                }
                if self
                    .tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .is_some_and(|set| set.contains_axis_constraint(kind, &refs, direction))
                {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.command_line
                        .push_error("The constraint already exists on the selected objects.");
                    return Task::none();
                }
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let mut touched = Vec::new();
                for reference in &refs {
                    if !touched.contains(&reference.entity) {
                        touched.push(reference.entity);
                    }
                }
                let pending = self.begin_undo(i, label, touched.len(), true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                // Two points on different objects: the reference slides the
                // second object rigidly onto the axis through the first
                // point instead of re-solving its shape, so move it first
                // and let the constraint then hold what already fits.
                if let [first_ref, second_ref] = refs.as_slice() {
                    if first_ref.entity != second_ref.entity {
                        let position = |reference: &ParametricRef| {
                            self.tabs[i]
                                .scene
                                .document
                                .get_entity(reference.entity)
                                .zip(reference.marker)
                                .and_then(|(entity, marker)| resolve_point(entity, marker))
                        };
                        if let (Some(first_point), Some(second_point)) =
                            (position(first_ref), position(second_ref))
                        {
                            let normal = (-direction.y, direction.x);
                            let offset = (second_point.x - first_point.x) * normal.0
                                + (second_point.y - first_point.y) * normal.1;
                            if offset.abs() > 1.0e-9 {
                                self.tabs[i].scene.transform_entities(
                                    &[second_ref.entity],
                                    &EntityTransform::Translate(glam::DVec3::new(
                                        -offset * normal.0,
                                        -offset * normal.1,
                                        0.0,
                                    )),
                                );
                            }
                        }
                    }
                }
                // The solver cannot start from an axis lying exactly across
                // the datum (its equations are singular there), so turn the
                // object onto the axis about its anchor first — the
                // reference turns it about that anchor too — and let the
                // constraint hold what already fits.
                if let Some((handle, anchor, end, vertex)) =
                    crate::scene::parametric_constraints::axis_alignment_target(
                        &self.tabs[i].scene.document,
                        &refs,
                    )
                {
                    let axis = (end.x - anchor.x, end.y - anchor.y);
                    let length = axis.0.hypot(axis.1);
                    let target = if axis.0 * direction.x + axis.1 * direction.y >= 0.0 {
                        (direction.x, direction.y)
                    } else {
                        (-direction.x, -direction.y)
                    };
                    let angle = (axis.0 * target.1 - axis.1 * target.0)
                        .atan2(axis.0 * target.0 + axis.1 * target.1);
                    if length > 1.0e-9 && angle.abs() > 1.0e-9 {
                        match vertex {
                            Some(index) => {
                                let moved =
                                    self.tabs[i].scene.document.get_entity(handle).cloned();
                                if let Some(mut entity) = moved {
                                    if crate::scene::parametric_constraints::set_polyline_vertex(
                                        &mut entity,
                                        index,
                                        anchor.x + length * target.0,
                                        anchor.y + length * target.1,
                                    ) {
                                        self.tabs[i].scene.update_entity(entity);
                                    }
                                }
                            }
                            None => self.tabs[i].scene.transform_entities(
                                &[handle],
                                &EntityTransform::Rotate {
                                    center: glam::DVec3::new(anchor.x, anchor.y, anchor.z),
                                    axis: glam::DVec3::Z,
                                    angle_rad: angle,
                                },
                            ),
                        }
                    }
                }
                let id = self.tabs[i]
                    .scene
                    .parametric_constraint_set_mut(scope)
                    .add_axis_constraint(kind, refs, direction);
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                let changes = touched
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect::<Vec<_>>();
                self.tabs[i].scene.bump_entities_with_parametric_policy(
                    &changes,
                    &initial_fixed,
                    self.constraint_solve_mode,
                );
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line
                    .push_output(&format!("{axis} constraint applied."));
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::AddSymmetricConstraint {
                selection,
                axis,
                label,
            } => {
                use crate::command::SymmetricConstraintSelection;
                use crate::scene::parametric_constraints::ConstraintKind;

                let scope = self.tabs[i].current_parametric_scope();
                let to_world = |point: glam::DVec3| {
                    acadrust::types::Vector3::new(point.x, point.y, point.z)
                };
                let resolve_point = |pick: crate::command::CoincidentPick| {
                    if let Some(handle) = pick.handle {
                        crate::scene::parametric_constraints::nearest_parametric_point_on_entity(
                            &self.tabs[i].scene.document,
                            scope,
                            handle,
                            to_world(pick.point),
                        )
                    } else {
                        crate::scene::parametric_constraints::nearest_parametric_point(
                            &self.tabs[i].scene.document,
                            scope,
                            to_world(pick.point),
                            None,
                        )
                    }
                };
                let (first, second) = match selection {
                    SymmetricConstraintSelection::Objects(first, second) => (first, second),
                    SymmetricConstraintSelection::Points(first, second) => {
                        let (Some(first), Some(second)) =
                            (resolve_point(first), resolve_point(second))
                        else {
                            self.command_line.push_error(
                                "Symmetric: select supported endpoints, centers, midpoints, or vertices.",
                            );
                            return Task::none();
                        };
                        (first, second)
                    }
                };
                if first == second
                    || axis.entity == first.entity
                    || axis.entity == second.entity
                {
                    self.command_line
                        .push_error("Symmetric: select two different references and a separate line axis.");
                    return Task::none();
                }
                let refs = vec![first, second, axis];
                if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
                    ConstraintKind::Symmetric,
                    &refs,
                    None,
                ) {
                    self.command_line.push_error(message);
                    return Task::none();
                }
                let duplicate = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .is_some_and(|set| {
                        set.constraints.iter().any(|constraint| {
                            constraint.enabled
                                && constraint.kind == ConstraintKind::Symmetric
                                && matches!(constraint.refs.as_slice(), [a, b, m]
                                    if *m == axis
                                        && ((*a == first && *b == second)
                                            || (*a == second && *b == first)))
                        })
                    });
                if duplicate {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.command_line
                        .push_error("The constraint already exists on the selected objects.");
                    return Task::none();
                }
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let mut touched = Vec::new();
                for reference in &refs {
                    if !touched.contains(&reference.entity) {
                        touched.push(reference.entity);
                    }
                }
                let pending = self.begin_undo(i, label, touched.len(), true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
                    ConstraintKind::Symmetric,
                    refs,
                    None,
                );
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                let changes = touched
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect::<Vec<_>>();
                self.tabs[i].scene.bump_entities_with_initial_parametric_policy(
                    &changes,
                    &[first, axis],
                    false,
                );
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line
                    .push_output("Symmetric constraint applied.");
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::AddPerpendicularConstraint {
                first,
                second,
                first_fixed,
                second_start,
                label,
            } => {
                use crate::scene::parametric_constraints::ConstraintKind;

                let refs = vec![first, second];
                if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
                    ConstraintKind::Perpendicular,
                    &refs,
                    None,
                ) {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.command_line.push_error(message);
                    return Task::none();
                }
                let scope = self.tabs[i].current_parametric_scope();
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let touched = if first.entity == second.entity {
                    vec![first.entity]
                } else {
                    vec![first.entity, second.entity]
                };
                let pending = self.begin_undo(i, label, touched.len(), true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
                    ConstraintKind::Perpendicular,
                    refs,
                    None,
                );
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                let changes = touched
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect::<Vec<_>>();
                self.tabs[i].scene.bump_entities_with_initial_parametric_policy(
                    &changes,
                    &[first_fixed, second_start],
                    true,
                );
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line.push_output("Constraint applied.");
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::AddTangentConstraint {
                first,
                second,
                label,
            } => {
                use crate::scene::parametric_constraints::ConstraintKind;

                let refs = vec![first, second];
                if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
                    ConstraintKind::Tangent,
                    &refs,
                    None,
                ) {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.command_line.push_error(message);
                    return Task::none();
                }
                let scope = self.tabs[i].current_parametric_scope();
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let touched = if first.entity == second.entity {
                    vec![first.entity]
                } else {
                    vec![first.entity, second.entity]
                };
                let pending = self.begin_undo(i, label, touched.len(), true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
                    ConstraintKind::Tangent,
                    refs,
                    None,
                );
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                let changes = touched
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect::<Vec<_>>();
                self.tabs[i].scene.bump_entities_with_initial_parametric_policy(
                    &changes,
                    &[first],
                    true,
                );
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line.push_output("Constraint applied.");
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::AddConcentricConstraint {
                first,
                second,
                label,
            } => {
                use crate::scene::parametric_constraints::ConstraintKind;

                let refs = vec![first, second];
                if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
                    ConstraintKind::Concentric,
                    &refs,
                    None,
                ) {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.command_line.push_error(message);
                    return Task::none();
                }
                let scope = self.tabs[i].current_parametric_scope();
                let duplicate = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .is_some_and(|set| {
                        set.constraints.iter().any(|constraint| {
                            constraint.enabled
                                && constraint.kind == ConstraintKind::Concentric
                                && matches!(constraint.refs.as_slice(), [a, b]
                                    if (*a == first && *b == second)
                                        || (*a == second && *b == first))
                        })
                    });
                if duplicate {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.command_line
                        .push_output("The Concentric constraint already exists.");
                    return Task::none();
                }
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let touched = if first.entity == second.entity {
                    vec![first.entity]
                } else {
                    vec![first.entity, second.entity]
                };
                let pending = self.begin_undo(i, label, touched.len(), true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
                    ConstraintKind::Concentric,
                    refs,
                    None,
                );
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                let changes = touched
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect::<Vec<_>>();
                self.tabs[i].scene.bump_entities_with_initial_parametric_policy(
                    &changes,
                    &[first],
                    true,
                );
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line.push_output("Constraint applied.");
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::AddCoincidentConstraint {
                first,
                second,
                multiple,
                label,
            } => {
                let scope = self.tabs[i].current_parametric_scope();
                let to_world = |p: glam::DVec3| acadrust::types::Vector3::new(p.x, p.y, p.z);
                let resolve = |pick: crate::command::CoincidentPick,
                               exclude: Option<Handle>| {
                    if pick.whole_curve {
                        pick.handle.and_then(|handle| {
                            crate::scene::parametric_constraints::parametric_curve_ref_for_pick(
                                &self.tabs[i].scene.document,
                                scope,
                                handle,
                                to_world(pick.point),
                            )
                        })
                    } else if let Some(handle) = pick.handle {
                        crate::scene::parametric_constraints::nearest_parametric_point_on_entity(
                            &self.tabs[i].scene.document,
                            scope,
                            handle,
                            to_world(pick.point),
                        )
                    } else {
                        crate::scene::parametric_constraints::nearest_parametric_point(
                            &self.tabs[i].scene.document,
                            scope,
                            to_world(pick.point),
                            exclude,
                        )
                    }
                };
                let resolved_first = resolve(first, None);
                let resolved_second = resolved_first
                    .and_then(|reference| resolve(second, Some(reference.entity)));
                let (Some(first_ref), Some(second_ref)) = (resolved_first, resolved_second) else {
                    self.command_line.push_error(
                        "Coincident: select a supported endpoint, center, midpoint, vertex, or curve.",
                    );
                    return Task::none();
                };
                let (kind, refs) = match (first.whole_curve, second.whole_curve) {
                    (false, false) if first_ref != second_ref => (
                        crate::scene::parametric_constraints::ConstraintKind::Coincident,
                        vec![first_ref, second_ref],
                    ),
                    (true, false) => (
                        crate::scene::parametric_constraints::ConstraintKind::PointOnCurve,
                        vec![second_ref, first_ref],
                    ),
                    (false, true) => (
                        crate::scene::parametric_constraints::ConstraintKind::PointOnCurve,
                        vec![first_ref, second_ref],
                    ),
                    _ => {
                        self.command_line
                            .push_error("Coincident: select one point and one curve, or two points.");
                        return Task::none();
                    }
                };
                if let Err(error) = self.tabs[i]
                    .scene
                    .validate_parametric_constraint(kind, &refs, None)
                {
                    self.command_line.push_error(error);
                    return Task::none();
                }
                let touched: Vec<_> = refs.iter().map(|reference| reference.entity).collect();
                let constraints_before = self.tabs[i]
                    .scene
                    .parametric_constraint_set(scope)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
                    });
                let pending = self.begin_undo(i, label, touched.len(), true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, constraints_before);
                let id = self.tabs[i]
                    .scene
                    .parametric_constraint_set_mut(scope)
                    .add(kind, refs, None);
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
                let changes: Vec<_> = touched
                    .iter()
                    .copied()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities_with_parametric_policy(
                    &changes,
                    &[first_ref],
                    self.constraint_solve_mode,
                );
                self.tabs[i].dirty = true;
                self.tabs[i].snap_result = None;
                if !multiple {
                    self.tabs[i].active_cmd = None;
                }
                self.command_line.push_output("Coincident constraint applied.");
                if multiple {
                    if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|cmd| cmd.prompt()) {
                        self.command_line.push_info(&prompt);
                    }
                    self.command_line.set_step_options(
                        self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .map(|cmd| cmd.options())
                            .unwrap_or_default(),
                    );
                }
                self.refresh_properties();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::AddAutoCoincidentConstraints { handles } => {
                use crate::scene::parametric_constraints::ConstraintKind;
                let scope = self.tabs[i].current_parametric_scope();
                let inferred = self.tabs[i]
                    .scene
                    .inferred_coincident_constraints(scope, &handles);
                let count = inferred.len();
                if count > 0 {
                    let constraints_before = self.tabs[i]
                        .scene
                        .parametric_constraint_set(scope)
                        .cloned()
                        .unwrap_or_else(|| {
                            crate::scene::parametric_constraints::ParametricConstraintSet::new(
                                scope,
                            )
                        });
                    let pending = self.begin_undo(i, "Coincident auto constrain", handles.len(), true);
                    self.tabs[i]
                        .scene
                        .record_undo_parametric_constraints_before(scope, constraints_before);
                    for refs in inferred {
                        let id = self.tabs[i]
                            .scene
                            .parametric_constraint_set_mut(scope)
                            .add(ConstraintKind::Coincident, refs, None);
                        self.tabs[i].scene.note_parametric_constraint_applied(
                            scope,
                            id,
                            self.constraint_bar_display,
                        );
                    }
                    let changes: Vec<_> = handles
                        .iter()
                        .copied()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    if let Some(pd) = pending {
                        self.commit_undo_delta(i, pd);
                    }
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line
                    .push_output(format!("{} coincident constraint(s) applied.", count).as_str());
            }
            CmdResult::AddPointOnEntityConstraint {
                point,
                target,
                kind,
                label,
            } => {
                let scope = self.tabs[i].current_parametric_scope();
                let to_world = |p: glam::DVec3| acadrust::types::Vector3::new(p.x, p.y, p.z);
                let resolved = crate::scene::parametric_constraints::nearest_parametric_point(
                    &self.tabs[i].scene.document,
                    scope,
                    to_world(point),
                    Some(target),
                );
                match resolved {
                    Some(point_ref) => {
                        use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef};
                        // `CenterPoint` addresses its whole-entity side via
                        // the center marker (same solve as Coincident /
                        // Concentric — `ConstraintKind::CenterPoint`'s own
                        // doc comment); `Midpoint`/`PointOnCurve` address it
                        // as a whole entity.
                        let target_ref = if kind == ConstraintKind::CenterPoint {
                            ParametricRef::center(target)
                        } else {
                            ParametricRef::whole(target)
                        };
                        return self.apply_cmd_result(CmdResult::AddParametricConstraint {
                            kind,
                            refs: vec![point_ref, target_ref],
                            driving_param: None,
                            label,
                        });
                    }
                    None => {
                        self.command_line.push_error(
                            "Pick didn't land on a point (endpoint, center, or vertex) — enable an Endpoint/Center object snap and try again.",
                        );
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                    }
                }
            }
            CmdResult::AddEqualDistanceConstraint { points, label } => {
                let scope = self.tabs[i].current_parametric_scope();
                let to_world = |p: glam::DVec3| acadrust::types::Vector3::new(p.x, p.y, p.z);
                let mut refs = Vec::with_capacity(4);
                for p in points {
                    let Some(r) = crate::scene::parametric_constraints::nearest_parametric_point(
                        &self.tabs[i].scene.document,
                        scope,
                        to_world(p),
                        None,
                    ) else {
                        refs.clear();
                        break;
                    };
                    refs.push(r);
                }
                if refs.len() == 4 {
                    return self.apply_cmd_result(CmdResult::AddParametricConstraint {
                        kind: crate::scene::parametric_constraints::ConstraintKind::EqualDistance,
                        refs,
                        driving_param: None,
                        label,
                    });
                }
                self.command_line.push_error(
                    "Equal Distance: a pick didn't land on a point (endpoint, center, or vertex) — enable an Endpoint/Center object snap and try again.",
                );
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
            }
            CmdResult::ReassociateCenterMark {
                target,
                source,
                point,
            } => {
                if self.reject_locked_edit(i, target) {
                    return Task::none();
                }
                self.push_undo_snapshot(i, "CENTERREASSOCIATE");
                if self.tabs[i]
                    .scene
                    .reassociate_center_mark(target, source, point)
                {
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(
                        crate::t!("CENTERREASSOCIATE: center mark associated.").as_ref(),
                    );
                } else {
                    self.command_line.push_error(
                        crate::t!("CENTERREASSOCIATE: the selected source is not circular.")
                            .as_ref(),
                    );
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.refresh_properties();
            }
            CmdResult::EditDimensionBreak {
                dimensions,
                operation,
            } => {
                self.push_undo_snapshot(i, "DIMBREAK");
                let result = apply_dimbreak(&mut self.tabs[i].scene, &dimensions, operation);
                if result.changed.is_empty() {
                    self.command_line.push_info(&result.message);
                } else {
                    for handle in &result.changed {
                        self.tabs[i].scene.invalidate_dim_block_recorded(*handle);
                    }
                    let changes = result
                        .changed
                        .iter()
                        .copied()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect::<Vec<_>>();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(&result.message);
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
            }
            CmdResult::EditDimensionJog { dimension, point } => {
                let valid = matches!(
                    self.tabs[i].scene.document.get_entity(dimension),
                    Some(acadrust::EntityType::Dimension(
                        acadrust::entities::Dimension::Linear(_)
                            | acadrust::entities::Dimension::Aligned(_)
                    ))
                );
                if !valid {
                    self.command_line.push_error(
                        crate::t!("DIMJOGLINE: select a linear or aligned dimension.").as_ref(),
                    );
                } else if !self.reject_locked_edit(i, dimension) {
                    self.push_undo_snapshot(i, "DIMJOGLINE");
                    let values = point.map(|point| {
                        use acadrust::xdata::XDataValue;
                        vec![
                            XDataValue::Integer16(387),
                            XDataValue::Integer16(3),
                            XDataValue::Integer16(389),
                            XDataValue::Point3D(acadrust::types::Vector3::new(
                                point.x, point.y, point.z,
                            )),
                        ]
                    });
                    crate::scene::view::dispatch::set_entity_xdata(
                        &mut self.tabs[i].scene.document,
                        dimension,
                        "ACAD_DSTYLE_DIMJAG_POSITION",
                        values,
                    );
                    self.tabs[i].scene.invalidate_dim_block_recorded(dimension);
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(dimension, crate::scene::ChangeKind::Modified)]);
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(
                        if point.is_some() {
                            crate::t!("DIMJOGLINE: jog added.")
                        } else {
                            crate::t!("DIMJOGLINE: jog removed.")
                        }
                        .as_ref(),
                    );
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
            }
            CmdResult::SpaceDimensions {
                base,
                others,
                spacing,
            } => {
                self.push_undo_snapshot(i, "DIMSPACE");
                if apply_dimspace(&mut self.tabs[i].scene, base, &others, spacing) {
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::t!("DIMSPACE  Spacing adjusted.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
            }
            CmdResult::ReplaceEntity(handle, new_entities) => {
                if self.reject_locked_edit(i, handle) {
                    return Task::none();
                }
                // Detect SPLINEDIT sentinel: a single XLine with a magic layer name.
                if new_entities.len() == 1 {
                    if let acadrust::EntityType::XLine(ref xl) = new_entities[0] {
                        let op = xl.common.layer.clone();
                        if op.starts_with("__SPLINEDIT_") {
                            let label = self.history_label_from_active_cmd(i, "SPLINEDIT");
                            self.push_undo_snapshot(i, label);
                            crate::modules::draw::modify::splinedit::apply_spline_op(
                                &mut self.tabs[i].scene.document,
                                handle,
                                &op,
                            );
                            self.tabs[i].dirty = true;
                            let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                            if let Some(p) = prompt {
                                self.command_line.push_info(&p);
                            }
                            return Task::none();
                        }
                    }
                }
                let label = self.history_label_from_active_cmd(i, "TRIM");
                self.push_undo_snapshot(i, label);
                self.tabs[i].scene.erase_entities(&[handle]);
                let new_handles: Vec<acadrust::Handle> = new_entities
                    .into_iter()
                    .map(|e| self.tabs[i].scene.add_entity(e))
                    .collect();
                // Rebuild replaced dimensions from edited data.
                for &nh in &new_handles {
                    if matches!(
                        self.tabs[i].scene.document.get_entity(nh),
                        Some(acadrust::EntityType::Dimension(_))
                    ) {
                        self.tabs[i].scene.invalidate_dim_block_recorded(nh);
                    }
                }
                let pedit_entities = if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.name() == "PEDIT")
                {
                    new_handles
                        .iter()
                        .filter_map(|new| self.tabs[i].scene.document.get_entity(*new).cloned())
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if let Some(cmd) = &mut self.tabs[i].active_cmd {
                    cmd.on_entity_replaced(handle, &new_handles);
                    for entity in pedit_entities {
                        cmd.inject_picked_entity(entity);
                    }
                }
                self.tabs[i].dirty = true;
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }
            CmdResult::AttreqNeeded { block_name } => {
                // Collect the full AttributeDefinitions owned by this block
                // record so each created attribute keeps its geometry (#255).
                let attdefs: Vec<acadrust::entities::AttributeDefinition> = {
                    let doc = &self.tabs[i].scene.document;
                    if let Some(br) = doc.block_records.get(&block_name) {
                        br.entity_handles
                            .iter()
                            .filter_map(|&h| {
                                if let Some(acadrust::EntityType::AttributeDefinition(ad)) =
                                    doc.get_entity(h)
                                {
                                    Some(ad.clone())
                                } else {
                                    None
                                }
                            })
                            .collect()
                    } else {
                        vec![]
                    }
                };

                if attdefs.is_empty() {
                    // No attribute definitions — commit the INSERT directly.
                    let entity = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .and_then(|c| c.attreq_take_insert());
                    if let Some(entity) = entity {
                        let insert_name = match &entity {
                            acadrust::EntityType::Insert(ins) => Some(ins.block_name.clone()),
                            _ => None,
                        };
                        let label = self.history_label_from_active_cmd(i, "INSERT");
                        let delta_safe = self.delta_add_safe(i, &entity);
                        let pending = self.begin_undo(i, label, 1, delta_safe);
                        self.commit_entity(entity);
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.clear_preview_wire();
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        if let Some(n) = insert_name {
                            self.record_block_insert(&n);
                        }
                        self.restore_pre_cmd_tangent();
                        if let Some(pd) = pending {
                            self.commit_undo_delta(i, pd);
                        }
                    }
                } else {
                    let completed = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .and_then(|cmd| cmd.attreq_set_attdefs(attdefs));
                    if let Some(entity) = completed {
                        return self.apply_cmd_result(CmdResult::CommitAndExit(entity));
                    }
                    let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                    if let Some(p) = prompt {
                        self.command_line.push_info(&p);
                    }
                }
            }
            CmdResult::CommitLiveEntity(entity) => {
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let delta_safe = self.delta_add_safe(i, &entity);
                let pending = self.begin_undo(i, label, 1, delta_safe);
                let handle = self.commit_entity_handle(entity);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
                if let Some(h) = handle {
                    // Keep the live document Arc unique while later vertices
                    // replace the geometry in place. The final Arc is captured
                    // by UpdateLiveEntity when the command completes.
                    self.defer_live_entity_history_after(i, h);
                    if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                        cmd.set_live_handle(h);
                    }
                }
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }
            CmdResult::UpdateLiveEntity {
                handle,
                entity,
                finish,
            } => {
                let tracks_draw_anchor = matches!(
                    &entity,
                    acadrust::EntityType::Line(_)
                        | acadrust::EntityType::Arc(_)
                        | acadrust::EntityType::LwPolyline(_)
                        | acadrust::EntityType::Polyline(_)
                        | acadrust::EntityType::Polyline2D(_)
                        | acadrust::EntityType::Polyline3D(_)
                );
                // Replace the live entity's geometry in place, preserving its
                // handle and layer (the fresh entity from the command carries
                // defaults — a NULL handle would desync it from the document
                // map key and drop it from rendering / hit-test). No undo
                // snapshot — the create already pushed one, so the whole object
                // reverts as a unit.
                if let Some(old) = self.tabs[i].scene.document.get_entity_mut(handle) {
                    // The initial live commit assigns the handle, owning block,
                    // current layer and common display properties.  Geometry
                    // refreshes must retain all of that identity; replacing only
                    // the handle and layer reset owner_handle to NULL and made
                    // the completed entity unavailable to scoped operations such
                    // as parametric constraints.
                    let common = old.common().clone();
                    let mut new = entity;
                    *new.common_mut() = common;
                    *old = new;
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    self.tabs[i].dirty = true;
                }
                if tracks_draw_anchor {
                    self.tabs[i].last_draw_anchor = Some(handle);
                }
                if finish {
                    self.finish_live_entity_history(i, handle);
                    self.tabs[i].scene.clear_preview_wire();
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.restore_pre_cmd_tangent();
                } else {
                    let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                    if let Some(p) = prompt {
                        self.command_line.push_info(&p);
                    }
                }
            }
            CmdResult::FinalizeLiveEntity(handle) => {
                // The live geometry already matches the command's committed
                // vertices. Close the deferred history image and UI state
                // without publishing a redundant Modified delta.
                self.finish_live_entity_history(i, handle);
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
            }
            CmdResult::RemoveLiveEntity(handle) => {
                // The command backed off below a valid entity (PLINE Undo at
                // one remaining vertex): take the live entity out of the
                // document, drop its provisional history entry and keep
                // prompting. A later second point creates one fresh entry.
                self.tabs[i].scene.erase_entities(&[handle]);
                if self.tabs[i]
                    .last_draw_anchor
                    .is_some_and(|anchor_handle| anchor_handle == handle)
                {
                    self.tabs[i].last_draw_anchor = None;
                }
                self.discard_last_undo_entry(i);
                self.tabs[i].dirty = true;
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }
            cancel @ (CmdResult::Cancel | CmdResult::CancelForSpaceChange) => {
                let space_changed = matches!(cancel, CmdResult::CancelForSpaceChange);
                self.tabs[i].scene.clear_preview_wire();
                if !space_changed && self.tabs[i].suspended_cmd.is_some() {
                    self.tabs[i].active_cmd = self.tabs[i].suspended_cmd.take();
                    let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                    if let Some(p) = prompt {
                        self.command_line.push_info(&p);
                    }
                    let opts = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.options())
                        .unwrap_or_default();
                    self.command_line.set_step_options(opts);
                    self.refresh_active_cmd_preview(i);
                } else {
                    self.tabs[i].suspended_cmd = None;
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.restore_pre_cmd_tangent();
                    self.command_line.push_info(
                        crate::t!(if space_changed {
                            "Command cancelled because the active drawing space changed."
                        } else {
                            "Command cancelled."
                        })
                        .as_ref(),
                    );
                }
            }
            CmdResult::SelectByPath {
                path,
                closed,
                crossing,
            } => {
                // The command picked the path; the hit test lives here, where
                // the camera and the drawing's geometry are. Everything after
                // matches what a lasso does, Remove included.
                let canvas = self.tabs[i].scene.selection.borrow().vp_size;
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: canvas.0,
                    height: canvas.1,
                };
                let edit_cam = self.tabs[i]
                    .scene
                    .viewport_edit_frame(canvas)
                    .map(|(cam, _)| cam);
                let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
                let screen: Vec<iced::Point> = path
                    .iter()
                    .map(|p| {
                        crate::scene::pick::hit_test::world_to_screen(
                            glam::DVec3::new(p[0], p[1], 0.0),
                            view_rot,
                            eye,
                            bounds,
                        )
                    })
                    .collect();
                let handles = self.tabs[i].scene.path_hit_handles(
                    &screen,
                    crossing,
                    !closed,
                    all_wires,
                    view_rot,
                    eye,
                    bounds,
                    |point| self.cursor_model_point(i, &edit_cam, point, bounds),
                );
                if self.select_remove_mode {
                    for h in &handles {
                        self.tabs[i].scene.deselect_entity(*h);
                    }
                } else {
                    for h in &handles {
                        self.tabs[i].scene.select_entity(*h, false);
                    }
                    self.tabs[i].scene.expand_selection_for_groups(&handles);
                }
                self.refresh_properties();
                let selected: Vec<Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                let count = handles.len();
                self.command_line
                    .push_info(crate::tf!("{count} object(s) found.").as_ref());
                return self.feed_command(StepInput::SelectionComplete(selected));
            }
            CmdResult::Relaunch(cmd, handles) => {
                self.tabs[i].scene.deselect_all();
                for h in &handles {
                    self.tabs[i].scene.select_entity(*h, false);
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                dispatched = self.dispatch_command(&cmd);
            }
            CmdResult::Dispatch(cmd) => {
                // End this interactive front-end, then run the assembled command
                // through the normal dispatcher. Selection is left untouched.
                // The option picked in DIMCONSTRAINT becomes its next default;
                // the same command run on its own leaves the default alone.
                let from_menu = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.name() == "DIMCONSTRAINT");
                if from_menu {
                    match cmd.as_str() {
                        "DCRADIUS" => self.dim_constraint_last = "Radius",
                        "DCDIAMETER" => self.dim_constraint_last = "Diameter",
                        "DCCONVERT" => self.dim_constraint_last = "Convert",
                        _ => {}
                    }
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                dispatched = self.dispatch_command(&cmd);
            }
            CmdResult::EditTableCell { handle, point } => {
                // TABLEDIT's pick: end the pick phase and hand (table, point)
                // to the shared cell-edit launcher. A miss or a locked cell
                // re-prompts until a valid table cell is selected.
                use crate::modules::annotate::table_cmd::{table_cell_at, TableCellEditStart};
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();

                if handle == acadrust::Handle::NULL && self.selection_cycling {
                    let mut candidates = Vec::new();
                    for entity in self.tabs[i].scene.document.entities() {
                        if let acadrust::EntityType::Table(table) = entity {
                            let h = table.common.handle;
                            let style = table.table_style_handle.and_then(|style_handle| {
                                self.tabs[i]
                                    .scene
                                    .document
                                    .objects
                                    .get(&style_handle)
                                    .and_then(|object| match object {
                                        acadrust::objects::ObjectType::TableStyle(style) => {
                                            Some(style)
                                        }
                                        _ => None,
                                    })
                            });
                            if table_cell_at(table, style, point).is_some() {
                                candidates.push(h);
                            }
                        }
                    }
                    if candidates.len() >= 2 {
                        self.cycle_candidates = Some((self.quick_properties_anchor, candidates));
                        return Task::none();
                    }
                }

                match self.begin_table_cell_edit(i, handle, point) {
                    TableCellEditStart::Started => {}
                    TableCellEditStart::LockedCell => {
                        self.command_line
                            .push_error(crate::t!("The cell is content locked.").as_ref());
                        let _ = self.dispatch_command("TABLEDIT");
                    }
                    TableCellEditStart::NoCell => {
                        self.command_line
                            .push_error(crate::t!("No editable table cell picked.").as_ref());
                        let _ = self.dispatch_command("TABLEDIT");
                    }
                }
            }
            CmdResult::MatchEntityLayer { dest, src } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                let src_layer = self.tabs[i]
                    .scene
                    .document
                    .get_entity(src)
                    .map(|e| e.common().layer.clone());
                let dest: Vec<_> = dest
                    .into_iter()
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if dest.is_empty() {
                    self.command_line
                        .push_info(crate::t!("No editable objects selected.").as_ref());
                } else if let Some(layer) = src_layer {
                    self.push_undo_snapshot(i, "LAYMATCH");
                    for h in &dest {
                        if let Some(e) = self.tabs[i].scene.document.get_entity_mut(*h) {
                            e.as_entity_mut().set_layer(layer.clone());
                        }
                    }
                    // New layer changes the baked by-layer colour/linetype/
                    // lineweight — re-tessellate the moved entities so they
                    // repaint immediately (issue #231 class).
                    self.invalidate_property_targets(i, &dest);
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_info(crate::tf!("Layer matched to \"{layer}\".").as_ref());
                    self.sync_ribbon_layers();
                } else {
                    self.command_line
                        .push_error(crate::t!("Source object not found.").as_ref());
                }
            }
            CmdResult::MatchProperties { mut dest, src } => {
                dest.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                if dest.is_empty() {
                    return Task::none();
                }
                // The command stays active after each apply so more targets
                // can keep being picked; Enter / Esc ends it (#362).
                let src_clone = self.tabs[i].scene.document.get_entity(src).cloned();
                let src_common = src_clone.as_ref().map(|e| e.common().clone());
                let thickness = src_clone
                    .as_ref()
                    .and_then(crate::scene::view::dispatch::entity_thickness);
                // This application record is the hatch background colour.
                // Keep the outer Option to distinguish "source is not a
                // hatch" from "source hatch has no background".
                let hatch_background_xdata: Option<Option<Vec<acadrust::xdata::XDataValue>>> =
                    match src_clone.as_ref() {
                        Some(acadrust::EntityType::Hatch(h)) => Some(
                            h.common
                                .extended_data
                                .get_record("HATCHBACKGROUNDCOLOR")
                                .map(|r| r.values.clone()),
                        ),
                        _ => None,
                    };
                // Dimension-style overrides ride the ACAD record, identified
                // by a leading DSTYLE string. Matching replicates that payload
                // (or clears the destination when the source has none).
                let dstyle_xdata: Option<Vec<(i16, acadrust::xdata::XDataValue)>> = src_clone
                    .as_ref()
                    .filter(|e| {
                        matches!(
                            e,
                            acadrust::EntityType::Dimension(_) | acadrust::EntityType::Leader(_)
                        )
                    })
                    .map(|e| crate::entities::dim_override::pairs(&e.common().extended_data));

                if let Some(common) = src_common {
                    self.apply_property_op(i, "MATCHPROP", &dest, |app, handle| {
                        let mut is_dim = false;
                        let mut is_hatch = false;
                        if let Some(e) = app.tabs[i].scene.document.get_entity_mut(handle) {
                            e.as_entity_mut().set_layer(common.layer.clone());
                            crate::scene::view::dispatch::apply_color(e, common.color);
                            crate::scene::view::dispatch::apply_line_weight(e, common.line_weight);
                            {
                                let dst_common = e.common_mut();
                                dst_common.linetype = common.linetype.clone();
                                dst_common.linetype_handle = common.linetype_handle;
                                dst_common.linetype_scale = common.linetype_scale;
                                dst_common.transparency = common.transparency.clone();
                                dst_common.color_name = common.color_name.clone();
                                dst_common.color_book_handle = common.color_book_handle;
                                dst_common.full_visual_style_handle =
                                    common.full_visual_style_handle;
                                dst_common.face_visual_style_handle =
                                    common.face_visual_style_handle;
                                dst_common.edge_visual_style_handle =
                                    common.edge_visual_style_handle;
                                dst_common.material_flags = common.material_flags;
                                dst_common.material_handle = common.material_handle;
                                dst_common.shadow_flags = common.shadow_flags;
                                dst_common.plotstyle_flags = common.plotstyle_flags;
                                dst_common.plotstyle_handle = common.plotstyle_handle;
                            }
                            if let Some(value) = thickness {
                                crate::scene::view::dispatch::set_entity_thickness(e, value);
                            }
                            if let Some(se) = &src_clone {
                                is_dim = matches!(e, acadrust::EntityType::Dimension(_));
                                is_hatch = matches!(e, acadrust::EntityType::Hatch(_));
                                match_special_props(se, e);
                            }
                        }
                        if is_hatch {
                            if let Some(values) = &hatch_background_xdata {
                                crate::scene::view::dispatch::set_entity_xdata(
                                    &mut app.tabs[i].scene.document,
                                    handle,
                                    "HATCHBACKGROUNDCOLOR",
                                    values.clone(),
                                );
                            }
                        }
                        // Dim-style overrides follow the style for dimension /
                        // leader destinations — through set_entity_xdata so no
                        // stale raw record survives.
                        if dstyle_xdata.is_some()
                            || matches!(
                                app.tabs[i].scene.document.get_entity(handle),
                                Some(
                                    acadrust::EntityType::Dimension(_)
                                        | acadrust::EntityType::Leader(_)
                                )
                            )
                        {
                            if matches!(
                                app.tabs[i].scene.document.get_entity(handle),
                                Some(
                                    acadrust::EntityType::Dimension(_)
                                        | acadrust::EntityType::Leader(_)
                                )
                            ) && matches!(
                                src_clone,
                                Some(
                                    acadrust::EntityType::Dimension(_)
                                        | acadrust::EntityType::Leader(_)
                                )
                            ) {
                                crate::entities::dim_override::replace(
                                    &mut app.tabs[i].scene.document,
                                    handle,
                                    dstyle_xdata.clone().unwrap_or_default(),
                                );
                            }
                        }
                        // A restyled dimension renders from its baked *D block —
                        // drop the stale block so the new style shows (#398).
                        if is_dim {
                            app.tabs[i].scene.invalidate_dim_block_recorded(handle);
                        }
                        // Hatch fills render from a prebuilt model (#415).
                        app.tabs[i].scene.refresh_fill_model(handle);
                    });
                    // Color / linetype / lineweight are baked into the cached
                    // wires at tessellation time; re-tessellate only the matched
                    // objects instead of rebuilding a large drawing.
                    let changes: Vec<_> = dest
                        .iter()
                        .copied()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.command_line.push_info(
                        crate::tf!("Properties matched to {} object(s).", dest.len()).as_ref(),
                    );
                    // Clear the consumed target selection and keep prompting.
                    self.tabs[i].scene.deselect_all();
                    if let Some(cmd) = &self.tabs[i].active_cmd {
                        self.command_line.push_info(&cmd.prompt());
                    }
                } else {
                    self.command_line
                        .push_error(crate::t!("Source object not found.").as_ref());
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.tabs[i].scene.clear_preview_wire();
                }
            }
            CmdResult::PasteClipboard { base_pt } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                if self.clipboard.is_empty() {
                    self.command_line
                        .push_error(crate::t!("Clipboard is empty.").as_ref());
                } else {
                    let delta = base_pt - self.clipboard_base;
                    let translate = crate::command::EntityTransform::Translate(delta);
                    self.push_undo_snapshot(i, "PASTECLIP");
                    let count = self.clipboard.len();
                    let by_index = self.finalize_paste(i, Some(translate));
                    self.tabs[i].scene.deselect_all();
                    for h in by_index.iter().copied().filter(|h| !h.is_null()) {
                        self.tabs[i].scene.select_entity(h, false);
                    }
                    self.tabs[i].dirty = true;
                    // Surface any layers the paste brought in (cross-drawing)
                    // in the layer manager and the layer dropdown.
                    self.refresh_layer_panel();
                    self.refresh_properties();
                    self.command_line
                        .push_info(crate::tf!("{count} object(s) pasted.").as_ref());
                }
            }
            CmdResult::CreateGroup { mut handles, name } => {
                handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                if handles.is_empty() {
                    return Task::none();
                }
                let undo = self.begin_group_undo(i, "GROUP");
                self.tabs[i].scene.create_group(name.clone(), handles);
                self.tabs[i].dirty = true;
                self.commit_group_undo(i, undo);
                self.command_line
                    .push_info(crate::tf!("Group \"{}\" created.", name).as_ref());
            }
            CmdResult::DeleteGroups { mut handles } => {
                handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                if handles.is_empty() {
                    return Task::none();
                }
                let undo = self.begin_group_undo(i, "UNGROUP");
                let count = self.tabs[i].scene.delete_groups_containing(&handles);
                self.tabs[i].dirty = true;
                self.commit_group_undo(i, undo);
                if count > 0 {
                    self.command_line
                        .push_info(crate::tf!("{} group(s) dissolved.", count).as_ref());
                } else {
                    self.command_line
                        .push_info(crate::t!("No groups found for selected objects.").as_ref());
                }
            }
            CmdResult::VpLayerUpdate {
                vp_handle,
                freeze,
                thaw,
            } => {
                // Resolve layer names → handles, then update frozen_layers on the viewport(s).
                // vp_handle == Handle::NULL means "apply to all viewports in current layout".
                let freeze_handles: Vec<Handle> = freeze
                    .iter()
                    .filter_map(|name| {
                        self.tabs[i]
                            .scene
                            .document
                            .layers
                            .iter()
                            .find(|l| l.name.eq_ignore_ascii_case(name))
                            .map(|l| l.handle)
                    })
                    .collect();
                let thaw_handles: Vec<Handle> = thaw
                    .iter()
                    .filter_map(|name| {
                        self.tabs[i]
                            .scene
                            .document
                            .layers
                            .iter()
                            .find(|l| l.name.eq_ignore_ascii_case(name))
                            .map(|l| l.handle)
                    })
                    .collect();

                let mut frozen_count = 0usize;
                let mut thawed_count = 0usize;

                // Collect target viewport handles
                let target_handles: Vec<Handle> = if vp_handle == acadrust::Handle::NULL {
                    // All viewports in current layout block
                    let block_handle = self.tabs[i].scene.current_layout_block_handle_pub();
                    self.tabs[i]
                        .scene
                        .document
                        .entities()
                        .filter(|e| {
                            e.common().owner_handle == block_handle
                                && matches!(e, acadrust::EntityType::Viewport(_))
                        })
                        .map(|e| e.common().handle)
                        .collect()
                } else {
                    vec![vp_handle]
                };

                for &target_handle in &target_handles {
                    if let Some(acadrust::EntityType::Viewport(vp)) =
                        self.tabs[i].scene.document.get_entity_mut(target_handle)
                    {
                        for h in &freeze_handles {
                            if !vp.frozen_layers.contains(h) {
                                vp.frozen_layers.push(*h);
                                frozen_count += 1;
                            }
                        }
                        for h in &thaw_handles {
                            let before = vp.frozen_layers.len();
                            vp.frozen_layers.retain(|fh| fh != h);
                            if vp.frozen_layers.len() < before {
                                thawed_count += 1;
                            }
                        }
                    }
                }

                if frozen_count > 0 || thawed_count > 0 {
                    self.push_undo_snapshot(i, "VPLAYER");
                    self.tabs[i].dirty = true;
                    if frozen_count > 0 {
                        self.command_line.push_info(
                            crate::tf!("VPLAYER: {frozen_count} layer(s) frozen in viewport.")
                                .as_ref(),
                        );
                    }
                    if thawed_count > 0 {
                        self.command_line.push_info(
                            crate::tf!("VPLAYER: {thawed_count} layer(s) thawed in viewport.")
                                .as_ref(),
                        );
                    }
                    // Sync layer panel so VP freeze columns update immediately.
                    let doc_layers = self.tabs[i].scene.document.layers.clone();
                    let vp_info = self.tabs[i].scene.viewport_list();
                    self.tabs[i]
                        .layers
                        .sync_with_viewports(&doc_layers, vp_info);
                }

                // Show updated prompt (command stays active for more operations).
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }

            CmdResult::ZoomToWindow { p1, p2 } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].scene.remember_current_view();
                self.tabs[i]
                    .scene
                    .zoom_to_window(p1.as_vec3(), p2.as_vec3());
                self.command_line
                    .push_output(crate::t!("Zoom Window").as_ref());
            }
            CmdResult::Measurement(msg) => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                self.command_line.push_output(&msg);
            }
            CmdResult::ReportMeasurement(msg) => {
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.refresh_area_preview(i);
                self.command_line.push_output(&msg);
                if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
                    self.command_line.push_info(&prompt);
                }
            }
            CmdResult::ReportError(msg) => {
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.command_line.push_error(&msg);
                if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
                    self.command_line.push_info(&prompt);
                }
            }
            CmdResult::ReportMeasurementAndDeselect(msg) => {
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.clear_preview_wire();
                self.refresh_area_preview(i);
                self.refresh_properties();
                self.command_line.push_output(&msg);
                if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
                    self.command_line.push_info(&prompt);
                }
            }
            CmdResult::DeselectAndContinue => {
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.clear_preview_wire();
                self.refresh_area_preview(i);
                self.refresh_properties();
                if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
                    self.command_line.push_info(&prompt);
                }
            }
            CmdResult::AlignSelected {
                mut handles,
                src1,
                dst1,
                angle_rad,
                scale,
            } => {
                handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                if handles.is_empty() {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.tabs[i].scene.clear_preview_wire();
                    self.restore_pre_cmd_tangent();
                } else {
                    let label = self.history_label_from_active_cmd(i, "ALIGN");
                    // A chain of transform_entities on the same handles — the
                    // recording keeps each entity's first (pre-align) image, so
                    // it's delta-safe like a single transform.
                    let pending = self.begin_undo(i, label, handles.len(), true);
                    // Step 1: translate so src1 is at origin
                    self.tabs[i].scene.transform_entities(
                        &handles,
                        &crate::command::EntityTransform::Translate(-src1),
                    );
                    // Step 2: uniform scale (only when != 1)
                    if (scale - 1.0).abs() > 1e-4 {
                        self.tabs[i].scene.transform_entities(
                            &handles,
                            &crate::command::EntityTransform::Scale {
                                center: glam::DVec3::ZERO,
                                factor: scale,
                            },
                        );
                    }
                    // Step 3: rotate in the XY plane by angle_rad
                    if angle_rad.abs() > 1e-4 {
                        self.tabs[i].scene.transform_entities(
                            &handles,
                            &crate::command::EntityTransform::Rotate {
                                center: glam::DVec3::ZERO,
                                axis: glam::DVec3::Z,
                                angle_rad,
                            },
                        );
                    }
                    // Step 4: translate to dst1
                    self.tabs[i].scene.transform_entities(
                        &handles,
                        &crate::command::EntityTransform::Translate(dst1),
                    );
                    self.tabs[i].dirty = true;
                    self.tabs[i].scene.deselect_all();
                    for h in &handles {
                        self.tabs[i].scene.select_entity(*h, false);
                    }
                    self.tabs[i].scene.clear_preview_wire();
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.restore_pre_cmd_tangent();
                    self.command_line
                        .push_output(crate::t!("ALIGN: applied.").as_ref());
                    self.refresh_properties();
                    if let Some(pd) = pending {
                        self.commit_undo_delta(i, pd);
                    }
                }
            }
            CmdResult::LengthenEntity {
                handle,
                pick_pt,
                mode,
            } => {
                if self.reject_locked_edit(i, handle) {
                    return Task::none();
                }
                use crate::modules::draw::modify::lengthen::lengthen_entity;
                let result = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .and_then(|e| lengthen_entity(e, pick_pt.as_vec3(), &mode));
                match result {
                    Some(new_entity) => {
                        let label = self.history_label_from_active_cmd(i, "LENGTHEN");
                        self.push_undo_snapshot(i, label);
                        self.tabs[i].scene.erase_entities(&[handle]);
                        self.tabs[i].scene.add_entity(new_entity);
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::t!("LENGTHEN: applied.").as_ref());
                        self.refresh_properties();
                    }
                    None => {
                        self.command_line
                            .push_error(crate::t!("LENGTHEN: entity type not supported.").as_ref());
                    }
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::DivideEntity { handle, n, marker } => {
                use crate::modules::draw::inquiry::divide::divide_entity;
                let pts = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .map(|e| divide_entity(e, n, marker.as_ref()))
                    .unwrap_or_default();
                let count = pts.len();
                if count > 0 {
                    self.push_undo_snapshot(i, "DIVIDE");
                    let layer = self.tabs[i].active_layer.clone();
                    for mut p in pts {
                        p.as_entity_mut().set_layer(layer.clone());
                        self.tabs[i].scene.add_entity(p);
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("DIVIDE: {count} marker(s) placed.").as_ref());
                } else {
                    self.command_line.push_error(
                        crate::t!("DIVIDE: entity type not supported or N < 2.").as_ref(),
                    );
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::MeasureEntity {
                handle,
                segment_length,
                pick_point,
                marker,
            } => {
                use crate::modules::draw::inquiry::divide::measure_entity;
                let pts = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .map(|e| measure_entity(e, segment_length, pick_point, marker.as_ref()))
                    .unwrap_or_default();
                let count = pts.len();
                if count > 0 {
                    self.push_undo_snapshot(i, "MEASURE");
                    let layer = self.tabs[i].active_layer.clone();
                    for mut p in pts {
                        p.as_entity_mut().set_layer(layer.clone());
                        self.tabs[i].scene.add_entity(p);
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("MEASURE: {count} marker(s) placed.").as_ref());
                } else {
                    self.command_line
                        .push_output(crate::t!("MEASURE: 0 markers placed.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::PeditOp { handle, op } => return self.apply_pedit_result(i, handle, op),
            CmdResult::JoinToSource { source, handles } => {
                if self.reject_locked_edit(i, source) {
                    return Task::none();
                }
                let joined = self.tabs[i]
                    .scene
                    .document
                    .get_entity(source)
                    .and_then(|entity| {
                        let candidates: Vec<_> = handles
                            .iter()
                            .filter(|handle| {
                                **handle != source && !self.tabs[i].scene.is_layer_locked(**handle)
                            })
                            .filter_map(|handle| {
                                self.tabs[i]
                                    .scene
                                    .document
                                    .get_entity(*handle)
                                    .map(|entity| (*handle, entity))
                            })
                            .collect();
                        crate::modules::draw::modify::join::join_to_source(entity, &candidates)
                    });
                if let Some((replacement, consumed)) = joined {
                    self.push_undo_snapshot(i, "JOIN");
                    if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(source) {
                        *entity = replacement;
                    }
                    self.tabs[i].scene.erase_entities(&consumed);
                    self.tabs[i].scene.refresh_fill_model(source);
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(source, crate::scene::ChangeKind::Modified)]);
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(&format!(
                        "JOIN: {} objects joined to source.",
                        consumed.len()
                    ));
                    self.refresh_properties();
                } else {
                    self.command_line
                        .push_info("JOIN: no compatible objects joined to source.");
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::JoinEntities(handles) => {
                if let Some(handle) = handles
                    .iter()
                    .find(|handle| self.tabs[i].scene.is_layer_locked(**handle))
                {
                    self.reject_locked_edit(i, *handle);
                    return Task::none();
                }
                use crate::modules::draw::modify::join::join_entities;
                let pairs: Vec<_> = handles
                    .iter()
                    .filter_map(|&h| self.tabs[i].scene.document.get_entity(h).map(|e| (h, e)))
                    .collect();
                match join_entities(&pairs) {
                    Some((to_remove, merged)) => {
                        let label = self.history_label_from_active_cmd(i, "JOIN");
                        self.push_undo_snapshot(i, label);
                        self.tabs[i].scene.erase_entities(&to_remove);
                        let count_in = to_remove.len();
                        let count_out = merged.len();
                        for e in merged {
                            self.tabs[i].scene.add_entity(e);
                        }
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.clear_preview_wire();
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        self.restore_pre_cmd_tangent();
                        self.command_line.push_output(
                            crate::tf!("JOIN: {count_in} object(s) joined into {count_out}.")
                                .as_ref(),
                        );
                        self.refresh_properties();
                    }
                    None => {
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.restore_pre_cmd_tangent();
                        self.command_line.push_error(
                            crate::t!("JOIN: objects don't form a single connected chain, or contain an unsupported type / tilted arc.").as_ref(),
                        );
                    }
                }
            }
            CmdResult::BreakEntity { handle, p1, p2 } => {
                if self.reject_locked_edit(i, handle) {
                    return Task::none();
                }
                use crate::modules::draw::modify::break_cmd::break_entity;
                let replacement = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .and_then(|e| break_entity(e, p1, p2));
                match replacement {
                    Some(frags) => {
                        let unchanged = frags.len() == 1
                            && self.tabs[i].scene.document.get_entity(handle).is_some_and(
                                |original| {
                                    let mut fragment = frags[0].clone();
                                    fragment.common_mut().handle = handle;
                                    &fragment == original
                                },
                            );
                        if unchanged {
                            self.tabs[i].active_cmd = None;
                            self.tabs[i].snap_result = None;
                            self.tabs[i].scene.clear_preview_wire();
                            self.restore_pre_cmd_tangent();
                            self.command_line.push_output("BREAK: no geometry changed.");
                            return Task::none();
                        }
                        let label = self.history_label_from_active_cmd(i, "BREAK");
                        self.push_undo_snapshot(i, label);
                        let count = frags.len();
                        let owner = self.tabs[i]
                            .scene
                            .document
                            .get_entity(handle)
                            .map(|entity| entity.common().owner_handle);
                        let mut fragments = frags.into_iter();
                        if let Some(mut first) = fragments.next() {
                            first.common_mut().handle = handle;
                            self.tabs[i].scene.update_entity(first);
                            for mut fragment in fragments {
                                fragment.common_mut().handle = Handle::NULL;
                                if let Some(owner) = owner {
                                    fragment.common_mut().owner_handle = owner;
                                }
                                self.tabs[i].scene.add_entity(fragment);
                            }
                        } else {
                            self.tabs[i].scene.erase_entities(&[handle]);
                        }
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.clear_preview_wire();
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        self.restore_pre_cmd_tangent();
                        self.command_line
                            .push_output(crate::tf!("BREAK: {} fragment(s).", count).as_ref());
                        self.refresh_properties();
                    }
                    None => {
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.restore_pre_cmd_tangent();
                        self.command_line
                            .push_error(crate::t!("BREAK: entity type not supported.").as_ref());
                    }
                }
            }
            CmdResult::SetPlotWindow { p1, p2 } => {
                let layout_name = self.tabs[i].scene.current_layout.clone();
                if layout_name == "Model" {
                    // Model space: remember the window (world X/Y) for the plot dialog.
                    let x0 = p1.x.min(p2.x);
                    let y0 = p1.y.min(p2.y);
                    let x1 = p1.x.max(p2.x);
                    let y1 = p1.y.max(p2.y);
                    self.plot_window = Some((x0, y0, x1, y1));
                    self.plot_dialog.window = self.plot_window;
                    self.command_line.push_output(
                        crate::tf!("Plot window: {x0:.2},{y0:.2} to {x1:.2},{y1:.2}").as_ref(),
                    );
                    // Pick window closed the plot dialog so the viewport could
                    // receive the two clicks — bring the dialog back with the
                    // window now active.
                    self.plot_dialog.area = "Window".to_string();
                    // Remember the pick immediately, like the printer choice.
                    self.save_config();
                    self.active_modal = Some(super::ModalKind::Plot);
                } else {
                    // PLOTWINDOW always describes the plotted layout. In MSPACE
                    // the command points are model coordinates, so map them back
                    // through the active floating viewport first.
                    let p1 = self.tabs[i].scene.model_to_paper(p1);
                    let p2 = self.tabs[i].scene.model_to_paper(p2);
                    let x1 = p1.x.min(p2.x);
                    let y1 = p1.y.min(p2.y);
                    let x2 = p1.x.max(p2.x);
                    let y2 = p1.y.max(p2.y);
                    self.plot_window = Some((x1, y1, x2, y2));
                    self.plot_dialog.window = self.plot_window;
                    self.command_line.push_output(
                        crate::tf!("Plot window: {x1:.2},{y1:.2} to {x2:.2},{y2:.2}").as_ref(),
                    );
                    self.plot_dialog.area = "Window".to_string();
                    self.save_config();
                    self.active_modal = Some(super::ModalKind::Plot);
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::QuickPrint(handles) => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                return self.on_quick_print_handles(handles);
            }
            CmdResult::StretchWindow {
                mut handles,
                windows,
            } => {
                // Accumulate every entity touched by any crossing window. Keep STRETCH
                // in its selection stage; Enter is what advances to the base point.
                {
                    let scene = &self.tabs[i].scene;

                    for (win_min, win_max) in &windows {
                        handles.extend(
                            scene
                                .interaction_handles_in_world_aabb([
                                    win_min.x, win_min.y, win_max.x, win_max.y,
                                ])
                                .into_iter()
                                .filter(|&handle| !scene.is_layer_locked(handle)),
                        );
                    }
                }

                handles.sort_unstable_by_key(|handle| handle.value());
                handles.dedup();

                if handles.is_empty() {
                    self.command_line
                        .push_output(crate::t!("STRETCH: nothing crosses the window.").as_ref());
                } else {
                    // Visual feedback must only highlight entities that actually have a
                    // stretchable point inside one of the crossing windows. `handles` can
                    // contain the whole candidate/preselection set, while `windows` decides
                    // which vertices will really move.
                    let point_inside = |point: glam::DVec3| {
                        const EPS: f64 = 1.0e-9;

                        windows.iter().any(|(win_min, win_max)| {
                            point.x >= win_min.x - EPS
                                && point.x <= win_max.x + EPS
                                && point.y >= win_min.y - EPS
                                && point.y <= win_max.y + EPS
                                && point.z >= win_min.z - EPS
                                && point.z <= win_max.z + EPS
                        })
                    };

                    let mut visual_handles = Vec::new();

                    for &handle in &handles {
                        let wires = self.tabs[i].scene.wire_models_for(&[handle]);

                        let affected = wires.iter().any(|wire| {
                            // LINE / LWPOLYLINE / other vertex-based entities.
                            let vertex_hit = wire
                                .key_vertices
                                .iter()
                                .any(|point| point_inside(glam::DVec3::from_array(*point)));

                            if vertex_hit {
                                return true;
                            }

                            // Whole-object STRETCH cases such as circles/arcs/inserts/points:
                            // use only their meaningful anchor snap, not quadrants or
                            // tessellated display points.
                            wire.snap_pts.iter().any(|(point, hint)| {
                                matches!(
                                    hint,
                                    crate::scene::model::wire_model::SnapHint::Center
                                        | crate::scene::model::wire_model::SnapHint::Insertion
                                        | crate::scene::model::wire_model::SnapHint::Node
                                ) && point_inside(*point)
                            })
                        });

                        if affected {
                            visual_handles.push(handle);
                        }
                    }

                    self.tabs[i].scene.deselect_all();
                    self.tabs[i].scene.select_entities(&visual_handles);
                    self.refresh_selected_grips();
                    // STRETCH temporary grips: keep only the actual vertices/endpoints
                    // that lie inside one of the accumulated crossing windows.
                    //
                    // Mid-segment grips are deliberately excluded: STRETCH moves vertices,
                    // not the midpoint affordances used by normal grip editing.
                    let mut stretch_grips = Vec::new();
                    let mut stretch_grip_handles = Vec::new();

                    for (handle, grip) in std::mem::take(&mut self.tabs[i].selected_grip_handles)
                        .into_iter()
                        .zip(std::mem::take(&mut self.tabs[i].selected_grips))
                    {
                        if !grip.is_midpoint && point_inside(grip.world) {
                            stretch_grip_handles.push(handle);
                            stretch_grips.push(grip);
                        }
                    }

                    self.tabs[i].selected_grip_handles = stretch_grip_handles;
                    self.tabs[i].selected_grips = stretch_grips;
                }

                use crate::command::CadCommand;
                use crate::modules::draw::modify::stretch::StretchCommand;

                let wires = self.tabs[i].scene.wire_models_for(&handles);

                let cmd = StretchCommand::with_windows(handles, wires, windows);

                self.command_line.push_info(&CadCommand::prompt(&cmd));
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }
            CmdResult::StretchEntities {
                mut handles,
                windows,
                delta,
            } => {
                handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                if handles.is_empty() {
                    self.tabs[i].active_cmd = None;
                    return Task::none();
                }
                let structural = handles.iter().any(|handle| {
                    matches!(
                        self.tabs[i].scene.document.get_entity(*handle),
                        Some(acadrust::EntityType::Dimension(_))
                    )
                });
                let pending = self.begin_undo(i, "STRETCH", handles.len(), !structural);
                let mut count = 0usize;
                let mut changed_handles = Vec::new();
                let mut driven_refs = Vec::new();

                // Helper: is DXF point (x, y) inside the world-space window?
                // Drawing plane is world XY (= DXF XY).
                let in_win = |x: f64, y: f64| -> bool {
                    windows.iter().any(|(win_min, win_max)| {
                        x >= win_min.x && x <= win_max.x && y >= win_min.y && y <= win_max.y
                    })
                };

                let dx = delta.x as f64;
                let dy = delta.y as f64; // drawing plane is world XY
                let dz = delta.z as f64;

                // Dimensions whose points moved — their baked *D block is
                // stale afterwards and must be dropped (see #398 / #372).
                let mut stretched_dims: Vec<acadrust::Handle> = Vec::new();
                for handle in &handles {
                    let before = self.tabs[i].scene.document.get_entity_arc(*handle);
                    if let Some(entity) = before.as_deref() {
                        use crate::scene::parametric_constraints::ParametricRef;
                        match entity {
                            acadrust::EntityType::Line(line) => {
                                if in_win(line.start.x, line.start.y) {
                                    driven_refs.push(ParametricRef::point(*handle, 0));
                                }
                                if in_win(line.end.x, line.end.y) {
                                    driven_refs.push(ParametricRef::point(*handle, 1));
                                }
                            }
                            acadrust::EntityType::LwPolyline(polyline) => {
                                if let Some(world) =
                                    crate::entities::curve::lwpolyline_world_xy(polyline)
                                {
                                    for (index, vertex) in world.vertices.iter().enumerate() {
                                        if in_win(vertex.location.x, vertex.location.y) {
                                            driven_refs.push(ParametricRef::point(
                                                *handle,
                                                index as i32,
                                            ));
                                        }
                                    }
                                }
                            }
                            acadrust::EntityType::Polyline2D(polyline) => {
                                for (index, vertex) in polyline.vertices.iter().enumerate() {
                                    if in_win(vertex.location.x, vertex.location.y) {
                                        driven_refs.push(ParametricRef::point(
                                            *handle,
                                            index as i32,
                                        ));
                                    }
                                }
                            }
                            acadrust::EntityType::Circle(circle)
                                if in_win(circle.center.x, circle.center.y) =>
                            {
                                driven_refs.push(ParametricRef::center(*handle));
                            }
                            acadrust::EntityType::Arc(arc)
                                if in_win(arc.center.x, arc.center.y) =>
                            {
                                driven_refs.push(ParametricRef::center(*handle));
                            }
                            _ => {}
                        }
                    }
                    let Some(entity) = self.tabs[i].scene.document.get_entity_mut(*handle) else {
                        continue;
                    };
                    let mut stretched = false;
                    match entity {
                        acadrust::EntityType::Line(l) => {
                            let s_in = in_win(l.start.x, l.start.y);
                            let e_in = in_win(l.end.x, l.end.y);
                            if s_in {
                                l.start.x += dx;
                                l.start.y += dy;
                                l.start.z += dz;
                                stretched = true;
                            }
                            if e_in {
                                l.end.x += dx;
                                l.end.y += dy;
                                l.end.z += dz;
                                stretched = true;
                            }
                        }
                        acadrust::EntityType::LwPolyline(p) => {
                            let Some(mut world) = crate::entities::curve::lwpolyline_world_xy(p)
                            else {
                                continue;
                            };
                            for v in &mut world.vertices {
                                if in_win(v.location.x, v.location.y) {
                                    v.location.x += dx;
                                    v.location.y += dy;
                                    stretched = true;
                                }
                            }
                            if stretched {
                                *p = world;
                            }
                        }
                        acadrust::EntityType::Polyline2D(p) => {
                            for v in &mut p.vertices {
                                if in_win(v.location.x, v.location.y) {
                                    v.location.x += dx;
                                    v.location.y += dy;
                                    stretched = true;
                                }
                            }
                        }
                        acadrust::EntityType::Polyline(p) => {
                            for v in &mut p.vertices {
                                if in_win(v.location.x, v.location.z) {
                                    v.location.x += dx;
                                    v.location.z += dy;
                                    stretched = true;
                                }
                            }
                        }
                        acadrust::EntityType::Arc(a) => {
                            if in_win(a.center.x, a.center.y) {
                                a.center.x += dx;
                                a.center.y += dy;
                                a.center.z += dz;
                                stretched = true;
                            }
                        }
                        acadrust::EntityType::Circle(c) => {
                            if in_win(c.center.x, c.center.y) {
                                c.center.x += dx;
                                c.center.y += dy;
                                c.center.z += dz;
                                stretched = true;
                            }
                        }
                        acadrust::EntityType::Ellipse(e) => {
                            if in_win(e.center.x, e.center.y) {
                                e.center.x += dx;
                                e.center.y += dy;
                                e.center.z += dz;
                                stretched = true;
                            }
                        }
                        acadrust::EntityType::Insert(ins) => {
                            if in_win(ins.insert_point.x, ins.insert_point.y) {
                                ins.insert_point.x += dx;
                                ins.insert_point.y += dy;
                                ins.insert_point.z += dz;
                                stretched = true;
                            }
                        }
                        acadrust::EntityType::Text(t) => {
                            if in_win(t.insertion_point.x, t.insertion_point.y) {
                                t.insertion_point.x += dx;
                                t.insertion_point.y += dy;
                                t.insertion_point.z += dz;
                                stretched = true;
                            }
                        }
                        acadrust::EntityType::MText(t) => {
                            if in_win(t.insertion_point.x, t.insertion_point.y) {
                                t.insertion_point.x += dx;
                                t.insertion_point.y += dy;
                                t.insertion_point.z += dz;
                                stretched = true;
                            }
                        }
                        acadrust::EntityType::Viewport(vp) => {
                            stretched = windows.iter().any(|(win_min, win_max)| {
                                crate::entities::viewport::stretch(vp, *win_min, *win_max, delta)
                            });
                        }
                        acadrust::EntityType::Dimension(dim) => {
                            use acadrust::entities::Dimension;
                            // Move every definition point that falls inside the
                            // window — the same points the grips expose — then
                            // refresh the stored measurement so the value tracks
                            // the stretched geometry.
                            let mv = |p: &mut acadrust::types::Vector3| {
                                if in_win(p.x, p.y) {
                                    p.x += dx;
                                    p.y += dy;
                                    p.z += dz;
                                    true
                                } else {
                                    false
                                }
                            };
                            match dim {
                                Dimension::Linear(d) => {
                                    stretched |= mv(&mut d.first_point);
                                    stretched |= mv(&mut d.second_point);
                                    stretched |= mv(&mut d.definition_point);
                                }
                                Dimension::Aligned(d) => {
                                    stretched |= mv(&mut d.first_point);
                                    stretched |= mv(&mut d.second_point);
                                    stretched |= mv(&mut d.definition_point);
                                }
                                Dimension::Radius(d) => {
                                    stretched |= mv(&mut d.angle_vertex);
                                    stretched |= mv(&mut d.definition_point);
                                }
                                Dimension::Diameter(d) => {
                                    stretched |= mv(&mut d.angle_vertex);
                                    stretched |= mv(&mut d.definition_point);
                                }
                                Dimension::Angular2Ln(d) => {
                                    stretched |= mv(&mut d.angle_vertex);
                                    stretched |= mv(&mut d.first_point);
                                    stretched |= mv(&mut d.second_point);
                                    stretched |= mv(&mut d.definition_point);
                                }
                                Dimension::Angular3Pt(d) => {
                                    stretched |= mv(&mut d.angle_vertex);
                                    stretched |= mv(&mut d.first_point);
                                    stretched |= mv(&mut d.second_point);
                                    stretched |= mv(&mut d.definition_point);
                                }
                                Dimension::Ordinate(d) => {
                                    stretched |= mv(&mut d.feature_location);
                                    stretched |= mv(&mut d.leader_endpoint);
                                }
                                Dimension::Arc(d) => {
                                    stretched |= mv(&mut d.definition_point);
                                    stretched |= mv(&mut d.first_extension_point);
                                    stretched |= mv(&mut d.second_extension_point);
                                    stretched |= mv(&mut d.center_point);
                                    if d.has_leader {
                                        stretched |= mv(&mut d.first_leader_point);
                                        stretched |= mv(&mut d.second_leader_point);
                                    }
                                }
                                Dimension::LargeRadial(d) => {
                                    stretched |= mv(&mut d.definition_point);
                                    stretched |= mv(&mut d.chord_point);
                                    stretched |= mv(&mut d.override_center);
                                    stretched |= mv(&mut d.jog_point);
                                }
                            }
                            // Pinned text follows too; the zero sentinel means
                            // "auto placement" and must not be captured by a
                            // window that happens to cover the origin.
                            let t = dim.base().text_middle_point;
                            if t.x * t.x + t.y * t.y + t.z * t.z > 1e-16 {
                                let mut t = t;
                                if mv(&mut t) {
                                    dim.base_mut().text_middle_point = t;
                                    stretched = true;
                                }
                            }
                            if stretched {
                                dim.base_mut().actual_measurement = dim.measurement();
                                stretched_dims.push(*handle);
                            }
                        }
                        _ => {
                            // Generic: move entire entity (treat as block-level)
                            stretched = false; // skip generic types
                        }
                    }
                    if stretched {
                        if let Some(before) = before {
                            self.tabs[i].scene.record_undo_before(*handle, Some(before));
                        }
                        self.tabs[i].scene.mark_entity_dirty(*handle);
                        changed_handles.push(*handle);
                        count += 1;
                    }
                }
                // A stretched dimension renders through its baked *D block when
                // one exists (file roundtrip) — drop it so tessellation falls
                // back to the live points and the next save re-bakes. (#372)
                for h in stretched_dims {
                    self.tabs[i].scene.invalidate_dim_block_recorded(h);
                }

                // Geometry was edited in place via get_entity_mut, which the
                // scene's tessellation cache doesn't observe. Re-tessellate the
                // moved entities so the viewport reflects the stretch right away
                // instead of only on the next unrelated redraw. See #95.
                if count > 0 {
                    let changes: Vec<_> = changed_handles
                        .iter()
                        .map(|&handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities_with_parametric_policy(
                        &changes,
                        &driven_refs,
                        self.constraint_solve_mode && !driven_refs.is_empty(),
                    );
                    self.apply_inferred_constraints(i, &changed_handles);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                self.command_line
                    .push_output(crate::tf!("STRETCH: {count} entity(ies) stretched.").as_ref());
                self.refresh_properties();
                if let Some(pending) = pending {
                    self.commit_undo_delta(i, pending);
                }
            }
            // ── EXTRUDE ────────────────────────────────────────────────────
            CmdResult::ExtrudeEntities {
                handles,
                extent,
                mode,
                taper_angle,
                color: _,
            } => {
                let delete_sources = self.delete_objects != 0;
                if handles.is_empty()
                    || handles
                        .iter()
                        .any(|handle| self.reject_locked_edit(i, *handle))
                {
                    self.tabs[i].active_cmd = None;
                    return Task::none();
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
                    return Task::none();
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
            }

            CmdResult::ThickenEntities { handles, distance } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();

                if handles.is_empty() {
                    self.command_line
                        .push_output(crate::t!("No surfaces selected.").as_ref());
                    return Task::none();
                }
                if !distance.is_finite() {
                    self.command_line
                        .push_error(crate::t!("Requires numeric distance or two points.").as_ref());
                    return Task::none();
                }
                if distance.abs() <= f64::EPSILON {
                    return Task::none();
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
            }

            CmdResult::PresspullPick {
                handle,
                point,
                offset,
                multiple: _,
            } => {
                self.presspull_pick(handle, point, offset);
            }
            CmdResult::PresspullApply {
                targets,
                distance,
                color: _,
            } => {
                self.presspull_apply(targets, distance);
            }

            // ── REVOLVE ────────────────────────────────────────────────────
            CmdResult::RevolveEntities {
                handles,
                axis_start,
                axis_end,
                angle,
                start_angle,
                mode,
                color: _,
            } => {
                if handles.is_empty() {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.tabs[i].scene.clear_preview_wire();
                    self.restore_pre_cmd_tangent();
                    return Task::none();
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
            }
            // ── SWEEP ─────────────────────────────────────────────────────
            CmdResult::SweepEntities {
                handles,
                path_handle,
                mode,
                options,
                color: _,
            } => {
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

            // ── LOFT ──────────────────────────────────────────────────────
            CmdResult::LoftEntities {
                sections,
                guides,
                path,
                mode,
                options,
                color: _,
            } => {
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
                            return Task::none();
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
                            return Task::none();
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
                        return Task::none();
                    }
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                self.refresh_properties();
            }

            CmdResult::SolidEdgeBlend {
                handle,
                edges,
                base_face,
                value,
                other_value,
                fillet,
            } => {
                let task =
                    self.solid_edge_blend(handle, &edges, base_face, value, other_value, fillet);
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                return task;
            }

            CmdResult::SolidShell {
                handle,
                actions,
                distance,
            } => {
                let task = self.solid_shell(handle, &actions, distance);
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                return task;
            }

            CmdResult::SolidSubtract {
                bases,
                cutters,
                convert_meshes,
            } => {
                let task = self.solid_subtract(&bases, &cutters, convert_meshes);
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                return task;
            }

            CmdResult::SliceEntities {
                targets,
                plane,
                keep_point,
            } => {
                let task = self.solid_slice(&targets, plane, keep_point);
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                return task;
            }

            CmdResult::SliceSurfaceEntities {
                targets,
                cutter,
                keep_point,
            } => {
                let task = self.solid_slice_surface(&targets, *cutter, keep_point);
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                return task;
            }

            CmdResult::HatcheditApply {
                handle,
                name,
                scale,
                angle,
                operation,
            } => {
                if self.reject_locked_edit(i, handle) {
                    return Task::none();
                }
                if !matches!(
                    self.tabs[i].scene.document.get_entity(handle),
                    Some(acadrust::EntityType::Hatch(_))
                ) {
                    self.command_line
                        .push_error(crate::t!("HATCHEDIT: hatch entity not found.").as_ref());
                } else {
                    use crate::command::{CadCommand, HatchEditOperation};
                    if matches!(&operation, HatchEditOperation::BeginAssociate) {
                        let associative = matches!(self.tabs[i].scene.document.get_entity(handle),Some(acadrust::EntityType::Hatch(h)) if h.is_associative);
                        if associative {
                            self.command_line
                                .push_info("HATCHEDIT: hatch is already associative.");
                            self.tabs[i].active_cmd = None;
                        } else {
                            let plane = match self.tabs[i].scene.document.get_entity(handle) {
                                Some(acadrust::EntityType::Hatch(h)) => {
                                    let storage =
                                        crate::entities::curve::ocs_plane(h.normal, h.elevation);
                                    crate::command::WorkingPlane::new(
                                        glam::DVec3::from_array(storage.origin),
                                        glam::DVec3::from_array(storage.x_axis),
                                        glam::DVec3::from_array(storage.y_axis),
                                    )
                                }
                                _ => self.tabs[i].ucs_xform().working_plane(),
                            };
                            let mut sources =
                                self.tabs[i].scene.boundary_sources_on_plane(plane, 1e-6);
                            sources.remove(&handle);
                            let command=crate::modules::draw::draw::hatchedit::HatcheditCommand::for_association(handle,name,scale,angle,plane,sources);
                            self.tabs[i].scene.deselect_all();
                            self.command_line.push_info(&command.prompt());
                            self.tabs[i].active_cmd = Some(Box::new(command));
                        }
                        return Task::none();
                    }
                    if let HatchEditOperation::DrawOrderBoundary { above } = &operation {
                        let references: Vec<_> =
                            match self.tabs[i].scene.document.get_entity(handle) {
                                Some(acadrust::EntityType::Hatch(h)) => h
                                    .paths
                                    .iter()
                                    .flat_map(|p| p.boundary_handles.iter())
                                    .filter(|h| {
                                        self.tabs[i].scene.document.get_entity(**h).is_some()
                                    })
                                    .map(|h| format!("{:X}", h.value()))
                                    .collect(),
                                _ => Vec::new(),
                            };
                        self.tabs[i].active_cmd = None;
                        if references.is_empty() {
                            self.command_line
                                .push_info("HATCHEDIT: no associated boundary objects.");
                            return Task::none();
                        }
                        self.tabs[i].scene.deselect_all();
                        self.tabs[i].scene.select_entity(handle, false);
                        let command = format!(
                            "DRAWORDER {} {}",
                            if *above { "ABOVE" } else { "UNDER" },
                            references.join(" ")
                        );
                        return self.dispatch_view(&command, i).unwrap_or_else(Task::none);
                    }
                    if matches!(
                        &operation,
                        HatchEditOperation::DrawOrderFront | HatchEditOperation::DrawOrderBack
                    ) {
                        let command = if matches!(&operation, HatchEditOperation::DrawOrderFront) {
                            "DRAWORDER FRONT"
                        } else {
                            "DRAWORDER BACK"
                        };
                        self.tabs[i].scene.deselect_all();
                        self.tabs[i].scene.select_entity(handle, false);
                        self.tabs[i].active_cmd = None;
                        return self.dispatch_view(command, i).unwrap_or_else(Task::none);
                    }
                    self.push_undo_snapshot(i, "HATCHEDIT");
                    match operation {
                        HatchEditOperation::Appearance {
                            color,
                            layer,
                            transparency,
                        } => {
                            let layer = layer.map(|name| {
                                if name == "." {
                                    self.tabs[i].active_layer.clone()
                                } else {
                                    name
                                }
                            });
                            if layer.as_ref().is_some_and(|name| {
                                self.tabs[i].scene.document.layers.get(name).is_none()
                            }) {
                                self.discard_last_undo_entry(i);
                                self.command_line.push_error("HATCHEDIT: layer not found.");
                                return Task::none();
                            }
                            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                let common = entity.common_mut();
                                if let Some(value) = color {
                                    common.color = value;
                                    common.color_name = None;
                                    common.color_book_handle = None;
                                }
                                if let Some(value) = layer {
                                    common.layer = value;
                                }
                                if let Some(value) = transparency {
                                    common.transparency = value;
                                }
                            }
                            self.tabs[i]
                                .scene
                                .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                            self.refresh_properties();
                        }
                        HatchEditOperation::Update {
                            origin,
                            store_origin,
                            disassociate,
                            style,
                            annotative,
                        } => {
                            if store_origin {
                                if let Some((x, y)) = origin {
                                    if !self.tabs[i].scene.document.set_hatch_origin([x, y]) {
                                        self.discard_last_undo_entry(i);
                                        self.command_line
                                            .push_error("HATCHEDIT: cannot store default origin.");
                                        return Task::none();
                                    }
                                }
                            }
                            if let Some(acadrust::EntityType::Hatch(hatch)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                if !name.is_empty() && name != hatch.pattern.name {
                                    if let Some(entry) =
                                        crate::scene::model::hatch_patterns::find(&name)
                                    {
                                        let mut pattern =
                                            crate::scene::model::hatch_patterns::build_dxf_pattern(
                                                entry,
                                            );
                                        crate::entities::hatch::scale_pattern_geometry(
                                            &mut pattern,
                                            scale.max(1.0e-6) as f64,
                                        );
                                        crate::entities::hatch::rotate_pattern_geometry(
                                            &mut pattern,
                                            (angle as f64).to_radians(),
                                        );
                                        let origin = hatch.pattern_origin();
                                        crate::entities::hatch::translate_pattern_geometry(
                                            &mut pattern,
                                            origin.x,
                                            origin.y,
                                        );
                                        hatch.pattern = pattern;
                                        hatch.is_solid = matches!(
                                            entry.gpu,
                                            crate::scene::model::hatch_model::HatchPattern::Solid
                                        );
                                        hatch.pattern_type =
                                            acadrust::entities::HatchPatternType::Predefined;
                                        hatch.gradient_color.enabled = false;
                                    }
                                } else {
                                    let requested_scale = scale.max(1.0e-6) as f64;
                                    if hatch.pattern_scale > 1.0e-12 {
                                        let factor = requested_scale / hatch.pattern_scale;
                                        hatch.scale_pattern_about_origin(factor);
                                    }
                                    let requested_angle = (angle as f64).to_radians();
                                    let delta = requested_angle - hatch.pattern_angle;
                                    hatch.rotate_pattern_about_origin(delta);
                                }
                                hatch.pattern_scale = scale.max(1.0e-6) as f64;
                                hatch.pattern_angle = (angle as f64).to_radians();
                                if let Some((x, y)) = origin {
                                    hatch.set_pattern_origin(acadrust::types::Vector2::new(x, y));
                                }
                                if disassociate {
                                    for path in &mut hatch.paths {
                                        path.boundary_handles.clear();
                                        path.flags.set_external(false);
                                    }
                                    hatch.is_associative = false;
                                }
                                if let Some(style) = style {
                                    hatch.style = style;
                                }
                            }
                            if let Some(value) = annotative {
                                crate::scene::annotative::set_entity_annotative(
                                    &mut self.tabs[i].scene.document,
                                    handle,
                                    value,
                                );
                                if value {
                                    if let Some(scale_handle) =
                                        self.tabs[i].scene.creation_annotation_scale_handle()
                                    {
                                        crate::scene::annotative::create_annotation_context(
                                            &mut self.tabs[i].scene.document,
                                            handle,
                                            scale_handle,
                                        );
                                    }
                                }
                            }
                            self.tabs[i]
                                .scene
                                .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                        }
                        HatchEditOperation::AddBoundaries(handles) => {
                            self.tabs[i]
                                .scene
                                .edit_hatch_boundary_handles(handle, &handles, true);
                        }
                        HatchEditOperation::RemoveBoundaries(handles) => {
                            self.tabs[i]
                                .scene
                                .edit_hatch_boundary_handles(handle, &handles, false);
                        }
                        HatchEditOperation::AssociatePaths(paths) => {
                            if paths.is_empty()
                                || paths.iter().any(|path| {
                                    path.boundary_handles.is_empty()
                                        || path.boundary_handles.iter().any(|source| {
                                            self.tabs[i]
                                                .scene
                                                .document
                                                .get_entity(*source)
                                                .is_none()
                                        })
                                })
                            {
                                self.discard_last_undo_entry(i);
                                self.command_line.push_error(
                                    "HATCHEDIT: associated boundary is no longer available.",
                                );
                                return Task::none();
                            }
                            self.tabs[i].scene.replace_hatch_association(handle, paths);
                        }
                        HatchEditOperation::RecreateBoundary { associate, region } => {
                            let source = self.tabs[i].scene.document.get_entity(handle).cloned();
                            if let Some(acadrust::EntityType::Hatch(source)) = source {
                                let storage = crate::entities::curve::ocs_plane(
                                    source.normal,
                                    source.elevation,
                                );
                                let plane = crate::command::WorkingPlane::new(
                                    glam::DVec3::from_array(storage.origin),
                                    glam::DVec3::from_array(storage.x_axis),
                                    glam::DVec3::from_array(storage.y_axis),
                                );
                                let (entities, path_groups) = if region {
                                    let loops = source
                                        .paths
                                        .iter()
                                        .map(|path| {
                                            path.edges
                                                .iter()
                                                .map(crate::entities::hatch::edge_curve)
                                                .collect::<Option<Vec<_>>>()
                                        })
                                        .collect::<Option<Vec<_>>>();
                                    let Some(loops) = loops.filter(|loops| {
                                        !loops.is_empty()
                                            && loops.iter().all(|ring| !ring.is_empty())
                                    }) else {
                                        self.discard_last_undo_entry(i);
                                        self.command_line.push_error(
                                            "HATCHEDIT: boundary cannot form a region.",
                                        );
                                        return Task::none();
                                    };
                                    let contained = loops
                                        .iter()
                                        .enumerate()
                                        .map(|(inner, ring)| {
                                            let seed = ring[0].point_at(0.0);
                                            loops
                                                .iter()
                                                .enumerate()
                                                .map(|(outer, boundary)| {
                                                    inner != outer
                                                        && cadkernel::geom2d::contains(
                                                            boundary,
                                                            seed,
                                                            cadkernel::geom2d::Tolerance::new(1e-6),
                                                        )
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .collect::<Vec<_>>();
                                    let depths = contained
                                        .iter()
                                        .map(|row| row.iter().filter(|inside| **inside).count())
                                        .collect::<Vec<_>>();
                                    let groups = depths
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, depth)| **depth % 2 == 0)
                                        .map(|(outer, depth)| {
                                            let mut indices = vec![outer];
                                            indices.extend((0..loops.len()).filter(|inner| {
                                                depths[*inner] == *depth + 1
                                                    && contained[*inner][outer]
                                            }));
                                            indices
                                        })
                                        .collect::<Vec<_>>();
                                    let entities = groups
                                        .iter()
                                        .map(|indices| {
                                            let profiles = indices
                                                .iter()
                                                .map(|index| loops[*index].clone())
                                                .collect::<Vec<_>>();
                                            crate::scene::model::presspull_model::region_from_loops(
                                                &profiles, plane,
                                            )
                                        })
                                        .collect::<Option<Vec<_>>>();
                                    let Some(entities) = entities else {
                                        self.discard_last_undo_entry(i);
                                        self.command_line.push_error(
                                            "HATCHEDIT: boundary cannot form a region.",
                                        );
                                        return Task::none();
                                    };
                                    (entities, groups)
                                } else {
                                    let rings = crate::scene::hatch_boundary_rings(&source);
                                    if rings.len() != source.paths.len() {
                                        self.discard_last_undo_entry(i);
                                        self.command_line.push_error(
                                            "HATCHEDIT: boundary cannot form a polyline.",
                                        );
                                        return Task::none();
                                    }
                                    let entities = crate::scene::boundary_entities(&rings, plane);
                                    if entities.len() != source.paths.len() {
                                        self.discard_last_undo_entry(i);
                                        self.command_line.push_error(
                                            "HATCHEDIT: boundary cannot form a polyline.",
                                        );
                                        return Task::none();
                                    }
                                    let groups =
                                        (0..entities.len()).map(|index| vec![index]).collect();
                                    (entities, groups)
                                };
                                let mut path_handles = vec![None; source.paths.len()];
                                for (entity, paths) in entities.into_iter().zip(path_groups) {
                                    if let Some(boundary) = self.commit_entity_handle(entity) {
                                        for path in paths {
                                            if let Some(slot) = path_handles.get_mut(path) {
                                                *slot = Some(boundary);
                                            }
                                        }
                                    }
                                }
                                if associate {
                                    if let Some(acadrust::EntityType::Hatch(hatch)) =
                                        self.tabs[i].scene.document.get_entity_mut(handle)
                                    {
                                        for (path, boundary) in
                                            hatch.paths.iter_mut().zip(path_handles.iter().copied())
                                        {
                                            if let Some(boundary) = boundary {
                                                path.boundary_handles = vec![boundary];
                                                path.flags.set_external(true);
                                            }
                                        }
                                        hatch.is_associative =
                                            path_handles.iter().all(Option::is_some);
                                    }
                                }
                                self.tabs[i]
                                    .scene
                                    .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                            }
                        }
                        HatchEditOperation::Separate => {
                            let source = self.tabs[i].scene.document.get_entity(handle).cloned();
                            if let Some(acadrust::EntityType::Hatch(hatch)) = source {
                                let groups = crate::scene::separated_hatch_path_groups(&hatch);
                                if groups.len() > 1 {
                                    for paths in groups {
                                        let mut separated = hatch.clone();
                                        separated.common.handle = acadrust::Handle::NULL;
                                        separated.paths = paths;
                                        separated.is_associative = separated
                                            .paths
                                            .iter()
                                            .any(|path| !path.boundary_handles.is_empty());
                                        self.tabs[i]
                                            .scene
                                            .add_entity(acadrust::EntityType::Hatch(separated));
                                    }
                                    self.tabs[i].scene.erase_entities(&[handle]);
                                }
                            }
                        }
                        HatchEditOperation::DrawOrderFront
                        | HatchEditOperation::DrawOrderBack
                        | HatchEditOperation::DrawOrderBoundary { .. }
                        | HatchEditOperation::BeginAssociate => unreachable!(),
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::t!("HATCHEDIT: hatch updated.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::OpenMTextEditor {
                pos,
                handle,
                initial,
                height,
                template,
            } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.open_mtext_editor(pos, handle, &initial, height, template.map(|m| *m));
            }
            CmdResult::SuspendForMTextInput {
                pos,
                initial,
                height,
            } => {
                self.tabs[i].suspended_cmd = self.tabs[i].active_cmd.take();
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                self.command_mtext_input = true;
                self.pending_command_editor_text = None;
                self.open_mtext_editor(pos, None, &initial, height, None);
            }
            CmdResult::OpenTextEditor {
                pos,
                handle,
                initial,
                height,
            } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.open_text_inline(
                    pos,
                    handle,
                    &initial,
                    height,
                    super::text_inline::TextEntityField::Text,
                    None,
                );
            }
            CmdResult::SuspendForTextInput { pos, entity } => {
                self.tabs[i].suspended_cmd = self.tabs[i].active_cmd.take();
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                let height = entity.height;
                self.open_text_inline(
                    pos,
                    None,
                    "",
                    height,
                    super::text_inline::TextEntityField::Text,
                    Some(entity),
                );
            }
            CmdResult::EditTextEntity { handle } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                self.ribbon.deactivate_tool();
                return self.begin_text_edit(handle);
            }
            CmdResult::SuspendForTextEdit { handle } => {
                let is_editable =
                    crate::app::text_inline::can_edit_text(handle, &self.tabs[i].scene.document);
                if !is_editable {
                    self.command_line
                        .push_error(crate::t!("TEXTEDIT: selected entity is not text.").as_ref());
                    let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                    if let Some(p) = prompt {
                        self.command_line.push_info(&p);
                    }
                    return Task::none();
                }
                let cmd = self.tabs[i].active_cmd.take();
                self.tabs[i].suspended_cmd = cmd;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                self.ribbon.deactivate_tool();
                return self.begin_text_edit(handle);
            }
            CmdResult::UndoDocument => {
                let active = self.tabs[i].active_cmd.take();
                self.undo_active_tab();
                self.tabs[i].active_cmd = active;
                if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.name() == "PEDIT")
                {
                    let entities: Vec<_> =
                        self.tabs[i].scene.document.entities().cloned().collect();
                    if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                        for entity in entities {
                            command.inject_picked_entity(entity);
                        }
                    }
                }
                {
                    let tab = &mut self.tabs[i];
                    if let Some(command) = tab.active_cmd.as_mut() {
                        command.on_document_undone(&tab.scene.document);
                    }
                }
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }
            CmdResult::SetTexteditMode(val) => {
                self.texteditmode = val;
                let display_val = if val { 1 } else { 0 };
                self.command_line
                    .push_output(crate::tf!("TEXTEDITMODE set to {display_val}").as_ref());
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
            }
            CmdResult::EditDimensions {
                mut handles,
                operation,
            } => {
                handles.retain(|handle| {
                    !self.tabs[i].scene.is_layer_locked(*handle)
                        && matches!(
                            self.tabs[i].scene.document.get_entity(*handle),
                            Some(acadrust::EntityType::Dimension(_))
                        )
                });
                if handles.is_empty() {
                    self.command_line.push_error(
                        crate::t!("DIMEDIT: no editable dimensions were selected.").as_ref(),
                    );
                } else {
                    self.push_undo_snapshot(i, "DIMEDIT");
                    for handle in &handles {
                        if let Some(acadrust::EntityType::Dimension(dimension)) =
                            self.tabs[i].scene.document.get_entity_mut(*handle)
                        {
                            use crate::command::DimensionEditOperation as Operation;
                            match &operation {
                                Operation::Home => {
                                    let base = dimension.base_mut();
                                    base.text_middle_point = acadrust::types::Vector3::ZERO;
                                    base.insertion_point = acadrust::types::Vector3::ZERO;
                                    base.text_user_positioned = false;
                                    base.text_rotation = 0.0;
                                }
                                Operation::NewText(text) => {
                                    dimension.base_mut().set_text_override(
                                        (!text.is_empty()).then_some(text.clone()),
                                    );
                                }
                                Operation::Rotate(degrees) => {
                                    dimension.base_mut().text_rotation = degrees.to_radians();
                                }
                                Operation::Oblique(degrees) => match dimension {
                                    acadrust::entities::Dimension::Linear(value) => {
                                        value.ext_line_rotation = degrees.to_radians();
                                    }
                                    acadrust::entities::Dimension::Aligned(value) => {
                                        value.ext_line_rotation = degrees.to_radians();
                                    }
                                    _ => {}
                                },
                            }
                        }
                        self.tabs[i].scene.invalidate_dim_block_recorded(*handle);
                    }
                    let changes: Vec<_> = handles
                        .iter()
                        .copied()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(
                        crate::tf!("DIMEDIT: updated {} dimension(s).", handles.len()).as_ref(),
                    );
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::DdeditEntity { handle, new_text } => {
                if self.reject_locked_edit(i, handle) {
                    self.tabs[i].active_cmd = None;
                    return Task::none();
                }
                self.push_undo_snapshot(i, "DDEDIT");
                let mut updated = false;
                let mut is_dim = false;
                if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                    match entity {
                        acadrust::EntityType::Text(t) => {
                            t.value = new_text;
                            updated = true;
                        }
                        acadrust::EntityType::MText(t) => {
                            t.value = new_text;
                            updated = true;
                        }
                        acadrust::EntityType::AttributeDefinition(a) => {
                            a.default_value = new_text;
                            updated = true;
                        }
                        acadrust::EntityType::AttributeEntity(a) => {
                            a.set_value(new_text);
                            updated = true;
                        }
                        acadrust::EntityType::Dimension(d) => {
                            // Empty string resets to auto-measured value; otherwise set override.
                            let base = d.base_mut();
                            base.text = new_text;
                            updated = true;
                            is_dim = true;
                        }
                        _ => {}
                    }
                }
                if is_dim {
                    // The edited override changed the dimension text; drop its
                    // stale *D block so save re-bakes it. (#181)
                    self.tabs[i].scene.invalidate_dim_block_recorded(handle);
                }
                if updated {
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::t!("DDEDIT: text updated.").as_ref());
                } else {
                    self.discard_last_undo_entry(i);
                    self.command_line
                        .push_error(crate::t!("DDEDIT: entity type not supported.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::ReturnPoint(_) => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
        }
        // When no command is running the ribbon tool button still has to
        // visually deactivate. Keyboard focus is assigned below to whichever
        // editor currently owns typed input.
        if self.tabs[i].active_cmd.is_none() {
            self.ribbon.deactivate_tool();
        }
        // The rich text canvas owns keyboard editing itself. Leaving the
        // hidden command input focused would make it consume Left/Right before
        // the editor can handle them.
        let focus = if self.mtext_editor.is_some() {
            self.unfocus_widgets()
        } else if self.text_inline.is_some() {
            // The in-place TEXT editor needs keyboard focus on its own field.
            iced::widget::operation::focus(iced::widget::Id::new(super::view::TEXT_INLINE_ID))
        } else {
            self.focus_cmd_input()
        };
        Task::batch([dispatched, focus])
    }

    /// Restore the tangent-snap / ortho state that was in effect before the command started.
    /// Recreate clipboard-dependency records (layer / linetype / text + dim
    /// style) in tab `i`'s document for any the copied entities reference but
    /// this drawing doesn't already have. Each recreated record gets a fresh
    /// handle from the target document so it can't collide with an existing
    /// one. No-op for same-document pastes (the records already exist). (#129)
    pub(super) fn merge_dependencies(&mut self, i: usize, deps: &crate::app::ClipboardDeps) {
        use acadrust::TableEntry;
        if deps.is_empty() {
            return;
        }
        let doc = &mut self.tabs[i].scene.document;
        for rec in &deps.layers {
            if !doc.layers.contains(rec.name()) {
                let mut r = rec.clone();
                r.set_handle(doc.allocate_handle());
                let _ = doc.layers.add(r);
            }
        }
        for rec in &deps.linetypes {
            if !doc.line_types.contains(rec.name()) {
                let mut r = rec.clone();
                r.set_handle(doc.allocate_handle());
                let _ = doc.line_types.add(r);
            }
        }
        for rec in &deps.text_styles {
            if !doc.text_styles.contains(rec.name()) {
                let mut r = rec.clone();
                r.set_handle(doc.allocate_handle());
                let _ = doc.text_styles.add(r);
            }
        }
        for rec in &deps.dim_styles {
            if !doc.dim_styles.contains(rec.name()) {
                let mut r = rec.clone();
                r.set_handle(doc.allocate_handle());
                let _ = doc.dim_styles.add(r);
            }
        }
    }

    pub(super) fn merge_clipboard_deps(&mut self, i: usize) {
        let deps = self.clipboard_deps.clone();
        self.merge_dependencies(i, &deps);
    }

    /// Recreate any block definition the pasted INSERTs reference but tab
    /// `i`'s document lacks (cross-drawing paste), so the block reference
    /// renders its geometry instead of nothing. No-op for same-document
    /// pastes. (#135)
    pub(super) fn merge_clipboard_blocks(&mut self, i: usize) {
        if self.clipboard_deps.blocks.is_empty() {
            return;
        }
        let blocks = self.clipboard_deps.blocks.clone();
        for def in blocks {
            if self.tabs[i]
                .scene
                .document
                .block_records
                .get(&def.name)
                .is_some()
            {
                continue;
            }
            self.tabs[i]
                .scene
                .define_block_raw(&def.name, def.base_point, def.entities);
        }
    }

    /// Shared paste finalize for every paste path (PASTECLIP, PASTEORIG):
    /// recreate the clipboard's dependency records and block definitions, add
    /// each entity with fresh handles (optionally transformed), recreate each
    /// entity's xdictionary graph (XCLIP filters etc.), and tessellate pasted
    /// solids. Returns the new handles, index-aligned with the clipboard
    /// (NULL where an add failed). Keeping this in one place means a new
    /// cross-drawing concern is wired once, not re-implemented per command.
    pub(super) fn finalize_paste(
        &mut self,
        i: usize,
        translate: Option<crate::command::EntityTransform>,
    ) -> Vec<Handle> {
        self.merge_clipboard_deps(i);
        self.merge_clipboard_blocks(i);
        let by_index: Vec<Handle> = self
            .clipboard
            .clone()
            .into_iter()
            .map(|mut entity| {
                if let Some(t) = &translate {
                    crate::scene::view::dispatch::apply_transform(&mut entity, t);
                }
                // A dimension draws from its baked `*D` block (baked in WCS), so
                // give the paste its own transformed copy of that block and
                // re-point it — otherwise the pasted dimension renders at the
                // source location instead of the paste point. The block was
                // snapshotted into the clipboard at copy time, so this works
                // cross-drawing too. Mirrors the in-drawing copy. (#290, #161)
                if let acadrust::EntityType::Dimension(d) = &entity {
                    let bn = d.base().block_name.clone();
                    if !bn.trim().is_empty() {
                        let subs = self
                            .clipboard_deps
                            .dim_blocks
                            .iter()
                            .find(|b| b.name.eq_ignore_ascii_case(&bn))
                            .map(|def| def.entities.clone());
                        if let Some(subs) = subs {
                            let bt = translate.clone().unwrap_or(
                                crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
                            );
                            if let Some(new_bn) =
                                self.tabs[i].scene.define_transformed_block(&subs, &bt)
                            {
                                if let acadrust::EntityType::Dimension(d) = &mut entity {
                                    d.base_mut().block_name = new_bn;
                                }
                            }
                        }
                    }
                }
                self.tabs[i].scene.add_entity_clone(entity)
            })
            .collect();
        let annotation_delta = match translate {
            Some(crate::command::EntityTransform::Translate(delta)) => delta,
            _ => glam::DVec3::ZERO,
        };
        self.merge_clipboard_ext_objects(i, &by_index, annotation_delta);
        // Source handles stored in the clipboard map one-to-one to the freshly
        // pasted handles. Use that map to reconnect LEADER -> copied annotation.
        let mut handle_map = rustc_hash::FxHashMap::default();

        for (source, &copied) in self.clipboard.iter().zip(by_index.iter()) {
            if !copied.is_null() {
                handle_map.insert(source.common().handle, copied);
            }
        }

        let leader_links: Vec<(Handle, Handle)> = self
            .clipboard
            .iter()
            .filter_map(|source| {
                let acadrust::EntityType::Leader(leader) = source else {
                    return None;
                };

                let copied_leader = handle_map.get(&source.common().handle).copied()?;
                let copied_annotation = handle_map
                    .get(&leader.annotation_handle)
                    .copied()
                    .unwrap_or(Handle::NULL);

                Some((copied_leader, copied_annotation))
            })
            .collect();

        for (leader_handle, annotation_handle) in leader_links {
            if let Some(acadrust::EntityType::Leader(leader)) =
                self.tabs[i].scene.document.get_entity_mut(leader_handle)
            {
                leader.annotation_handle = annotation_handle;
            }

            let _ = self.tabs[i]
                .scene
                .sync_displayed_annotation_context(leader_handle);
        }
        // Recreate any group whose whole membership was copied, so a pasted
        // group stays grouped — cross-drawing too, since the groups were
        // snapshotted into the clipboard at copy time. `by_index` is aligned
        // with `self.clipboard`, so the source handle of each pasted entity maps
        // its clipboard clone to its new handle. Same shared `recreate_groups`
        // the in-drawing COPY path uses. (#440)
        if !self.clipboard_deps.groups.is_empty() {
            let groups = self.clipboard_deps.groups.clone();
            self.tabs[i].scene.recreate_groups(groups, &handle_map);
        }
        // Constraints wholly within the pasted selection follow it.
        self.tabs[i]
            .scene
            .duplicate_parametric_constraints_for(&handle_map);
        // Incremental: `add_entity` already tessellated every pasted top-level
        // solid, and existing document solids are still cached — so only newly
        // introduced block-definition solids need building. The full rebuild
        // would clear and re-tessellate the entire document (every solid in the
        // drawing) on each paste, which is what made a large paste stall.
        self.tabs[i].scene.populate_missing_meshes_from_document();
        by_index
    }

    /// Recreate the extension-dictionary object graph (XCLIP spatial filters,
    /// attached XRecords, …) captured for each copied entity, cloning every
    /// object into this document with fresh handles, remapping all internal
    /// references, and re-pointing the pasted entity's `xdictionary_handle` at
    /// the new root. `by_index` is the paste's new entity handles, aligned with
    /// the clipboard order (NULL where the add failed). No-op without captures.
    pub(super) fn merge_clipboard_ext_objects(
        &mut self,
        i: usize,
        by_index: &[Handle],
        annotation_delta: glam::DVec3,
    ) {
        if self.clipboard_deps.ext_objects.is_empty() {
            return;
        }
        let captures = self.clipboard_deps.ext_objects.clone();
        let doc = &mut self.tabs[i].scene.document;
        for cap in &captures {
            let Some(&new_entity) = by_index.get(cap.entity_index) else {
                continue;
            };
            if new_entity.is_null() {
                continue;
            }
            if let Some(new_root) = recreate_ext_subtree(doc, cap, Some(new_entity)) {
                if let Some(e) = doc.get_entity_mut(new_entity) {
                    e.common_mut().xdictionary_handle = Some(new_root);
                }
                crate::scene::annotative::translate_annotation_contexts(
                    doc,
                    new_entity,
                    annotation_delta,
                );
            }
        }
        // The wires were tessellated before the filters existed; refresh only
        // the freshly-pasted entities whose clip object graph was attached.
        let changes: Vec<_> = by_index
            .iter()
            .copied()
            .filter(|handle| !handle.is_null())
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
    }

    /// Recreate the captured xdictionary subtrees in this document (fresh
    /// handles, remapped references) WITHOUT an added host entity, returning
    /// `entity_index → new xdictionary root`. Used by PASTEBLOCK, which folds
    /// the clipboard into a new block definition: the caller stamps each new
    /// root onto the matching entity's `xdictionary_handle` before defining the
    /// block, so the block's nested insert keeps its XCLIP filter.
    pub(super) fn recreate_clipboard_ext_roots(
        &mut self,
        i: usize,
    ) -> std::collections::HashMap<usize, Handle> {
        let mut out = std::collections::HashMap::new();
        if self.clipboard_deps.ext_objects.is_empty() {
            return out;
        }
        let captures = self.clipboard_deps.ext_objects.clone();
        let doc = &mut self.tabs[i].scene.document;
        for cap in &captures {
            if let Some(new_root) = recreate_ext_subtree(doc, cap, None) {
                out.insert(cap.entity_index, new_root);
            }
        }
        out
    }

    fn restore_pre_cmd_tangent(&mut self) {
        if let Some(was_on) = self.pre_cmd_tangent.take() {
            if !was_on {
                self.snapper.enabled.remove(&crate::snap::SnapType::Tangent);
            }
        }
        if self.rect_suppressed_ortho {
            self.rect_suppressed_ortho = false;
            self.ortho_mode = true;
            self.polar_mode = false;
        }
    }
}

/// Clone one captured xdictionary subtree into `doc` with fresh handles,
/// remapping every internal reference (and the owning entity, when known),
/// returning the new root handle. `allocate_handle` advances the document's
/// handle counter — `next_handle()` only peeks, so reusing it would hand every
/// object the same handle and collapse the dictionary chain.
/// The entity a typed coordinate lands on while an object is asked for:
/// the nearest planar curve of the edited space within a small share of
/// that space's extent, the way a pick box takes the object under a click.
fn entity_at_typed_point(
    document: &acadrust::CadDocument,
    owner: Handle,
    point: glam::DVec3,
) -> Option<Handle> {
    let mut extent = 0.0f64;
    let mut nearest: Option<(f64, Handle)> = None;
    for entity in document.entities() {
        let common = entity.common();
        if common.owner_handle != owner {
            continue;
        }
        let bounds = entity.as_entity().bounding_box();
        extent = extent
            .max((bounds.max.x - bounds.min.x).abs())
            .max((bounds.max.y - bounds.min.y).abs());
        let distance = match crate::scene::viewport_dimension_pick::planar_pick_distance(entity, point) {
            Some(distance) => distance,
            // A text or an ellipse answers by its extent, the way a click
            // inside it picks it.
            None if matches!(
                entity,
                acadrust::EntityType::Text(_)
                    | acadrust::EntityType::MText(_)
                    | acadrust::EntityType::Ellipse(_)
            ) =>
            {
                let dx = (bounds.min.x - point.x).max(point.x - bounds.max.x).max(0.0);
                let dy = (bounds.min.y - point.y).max(point.y - bounds.max.y).max(0.0);
                dx.hypot(dy)
            }
            None => continue,
        };
        if nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, common.handle));
        }
    }
    let tolerance = (extent * 0.002).max(1.0e-9);
    nearest
        .filter(|(distance, _)| *distance <= tolerance)
        .map(|(_, handle)| handle)
}

fn recreate_ext_subtree(
    doc: &mut acadrust::CadDocument,
    cap: &crate::app::ClipExtObjects,
    entity_handle: Option<Handle>,
) -> Option<Handle> {
    use std::collections::HashMap;
    let mut remap: HashMap<Handle, Handle> = HashMap::new();
    if let Some(eh) = entity_handle {
        remap.insert(cap.src_entity_handle, eh);
    }
    for (old, scale) in &cap.annotation_scales {
        let target = crate::scene::annotative::ensure_scale_object(doc, scale);
        remap.insert(*old, target);
    }
    for (old, _) in &cap.objects {
        remap.insert(*old, doc.allocate_handle());
    }
    for (old, obj) in &cap.objects {
        let mut obj = obj.clone();
        let new_h = remap[old];
        remap_object(&mut obj, new_h, &remap);
        doc.objects.insert(new_h, obj);
    }
    remap.get(&cap.root).copied()
}

/// Replace references to a clipboard entity inside one recreated extension
/// dictionary graph after its final block-owned handle becomes known.
pub(crate) fn remap_ext_subtree_reference(
    doc: &mut acadrust::CadDocument,
    root: Handle,
    source_entity: Handle,
    target_entity: Handle,
) {
    use acadrust::objects::ObjectType;
    use rustc_hash::FxHashSet;
    use std::collections::HashMap;

    let remap = HashMap::from([(source_entity, target_entity)]);
    let mut seen = FxHashSet::default();
    let mut pending = vec![root];
    while let Some(handle) = pending.pop() {
        if handle.is_null() || !seen.insert(handle) {
            continue;
        }
        let children = match doc.objects.get(&handle) {
            Some(ObjectType::Dictionary(dictionary)) => {
                let mut children: Vec<_> =
                    dictionary.entries.iter().map(|(_, child)| *child).collect();
                if let Some(extension) = dictionary.xdictionary_handle {
                    children.push(extension);
                }
                children
            }
            Some(ObjectType::DictionaryWithDefault(dictionary)) => {
                let mut children: Vec<_> =
                    dictionary.entries.iter().map(|(_, child)| *child).collect();
                children.push(dictionary.default_handle);
                children
            }
            _ => Vec::new(),
        };
        pending.extend(children);
        if let Some(mut object) = doc.objects.remove(&handle) {
            remap_object(&mut object, handle, &remap);
            doc.objects.insert(handle, object);
        }
    }
}

/// Rewrite a cloned extension-dictionary object onto fresh handles: set its own
/// handle to `new_handle` and remap its owner and any handle references it holds
/// through `remap` (a handle still in the source space stays unchanged, which is
/// correct for cross-references that point outside the captured subtree).
fn remap_object(
    obj: &mut acadrust::objects::ObjectType,
    new_handle: acadrust::Handle,
    remap: &std::collections::HashMap<acadrust::Handle, acadrust::Handle>,
) {
    use acadrust::objects::ObjectType;
    let map = |h: acadrust::Handle| remap.get(&h).copied().unwrap_or(h);
    match obj {
        ObjectType::Dictionary(d) => {
            d.handle = new_handle;
            d.owner = map(d.owner);
            for (_, h) in d.entries.iter_mut() {
                *h = map(*h);
            }
            if let Some(x) = d.xdictionary_handle.as_mut() {
                *x = map(*x);
            }
            for r in d.reactors.iter_mut() {
                *r = map(*r);
            }
        }
        ObjectType::DictionaryWithDefault(d) => {
            d.handle = new_handle;
            d.owner = map(d.owner);
            for (_, h) in d.entries.iter_mut() {
                *h = map(*h);
            }
            d.default_handle = map(d.default_handle);
        }
        ObjectType::DictionaryVariable(v) => {
            v.handle = new_handle;
            v.owner_handle = map(v.owner_handle);
        }
        ObjectType::SpatialFilter(s) => {
            s.handle = new_handle;
            s.owner = map(s.owner);
        }
        ObjectType::XRecord(x) => {
            x.handle = new_handle;
            x.owner = map(x.owner);
            for entry in &mut x.entries {
                if let acadrust::objects::XRecordValue::Handle(handle) = &mut entry.value {
                    *handle = map(*handle);
                }
            }
        }
        ObjectType::Group(g) => {
            g.handle = new_handle;
            g.owner = map(g.owner);
            for h in g.entities.iter_mut() {
                *h = map(*h);
            }
        }
        ObjectType::ObjectContextData(context) => {
            context.handle = new_handle;
            context.owner_handle = map(context.owner_handle);
            for reactor in &mut context.reactors {
                *reactor = map(*reactor);
            }
            if let Some(dictionary) = &mut context.xdictionary_handle {
                *dictionary = map(*dictionary);
            }
            context.scale = map(context.scale);
            match &mut context.kind {
                acadrust::objects::ObjectContextKind::Dim(dimension) => {
                    dimension.block = map(dimension.block);
                }
                acadrust::objects::ObjectContextKind::HatchView(hatch) => {
                    hatch.view = map(hatch.view);
                }
                acadrust::objects::ObjectContextKind::MTextAttribute(attribute) => {
                    if let Some(embedded) = &mut attribute.context {
                        embedded.owner_handle = map(embedded.owner_handle);
                        for reactor in &mut embedded.reactors {
                            *reactor = map(*reactor);
                        }
                        if let Some(dictionary) = &mut embedded.xdictionary_handle {
                            *dictionary = map(*dictionary);
                        }
                        embedded.scale = map(embedded.scale);
                    }
                }
                acadrust::objects::ObjectContextKind::MLeader(mleader) => {
                    if let Some(handle) = &mut mleader.text_style_handle {
                        *handle = map(*handle);
                    }
                    if let Some(handle) = &mut mleader.block_content_handle {
                        *handle = map(*handle);
                    }
                    if let Some(handle) = &mut mleader.scale_handle {
                        *handle = map(*handle);
                    }
                    for root in &mut mleader.leader_roots {
                        for line in &mut root.lines {
                            if let Some(handle) = &mut line.line_type_handle {
                                *handle = map(*handle);
                            }
                            if let Some(handle) = &mut line.arrowhead_handle {
                                *handle = map(*handle);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // Other leaf object kinds don't appear in an entity xdictionary; if one
        // does, it's inserted with the fresh handle below via the caller's key,
        // but its internal owner is left as-is (best effort).
        _ => {}
    }
}

// ── DIMSPACE helper ───────────────────────────────────────────────────────────

struct DimBreakResult {
    changed: Vec<Handle>,
    message: String,
}

fn wire_segments(
    models: &[crate::scene::model::wire_model::WireModel],
) -> Vec<(glam::DVec3, glam::DVec3)> {
    let mut output = Vec::new();
    for model in models {
        for run in model.points.split(|point| !point[0].is_finite()) {
            for pair in run.windows(2) {
                let first =
                    glam::DVec3::new(pair[0][0] as f64, pair[0][1] as f64, pair[0][2] as f64);
                let second =
                    glam::DVec3::new(pair[1][0] as f64, pair[1][1] as f64, pair[1][2] as f64);
                if first.is_finite()
                    && second.is_finite()
                    && first.distance_squared(second) > 1.0e-18
                {
                    output.push((first, second));
                }
            }
        }
    }
    output
}

fn segment_intersection_xy(
    first: (glam::DVec3, glam::DVec3),
    second: (glam::DVec3, glam::DVec3),
) -> Option<(glam::DVec3, glam::DVec3)> {
    let gap = cadkernel::space::segment_break_gap_xy(
        [first.0.to_array(), first.1.to_array()],
        [second.0.to_array(), second.1.to_array()],
        0.25,
    )?;
    Some((
        glam::DVec3::from_array(gap[0]),
        glam::DVec3::from_array(gap[1]),
    ))
}

fn break_object_handle(document: &acadrust::CadDocument, dimension: Handle) -> Option<Handle> {
    document.objects.iter().find_map(|(handle, object)| {
        let acadrust::objects::ObjectType::DataObject(object) = object else {
            return None;
        };
        let acadrust::objects::DataObjectData::BreakData(data) = &object.data else {
            return None;
        };
        (data.dimension_reference == dimension).then_some(*handle)
    })
}

fn remove_dimension_break_data(document: &mut acadrust::CadDocument, dimension: Handle) -> bool {
    let handles: Vec<_> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            acadrust::objects::ObjectType::DataObject(object) => match &object.data {
                acadrust::objects::DataObjectData::BreakData(data)
                    if data.dimension_reference == dimension =>
                {
                    Some(*handle)
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    if handles.is_empty() {
        return false;
    }
    for handle in &handles {
        document.objects.remove(handle);
    }
    if let Some(dictionary_handle) = document.extension_dictionary_handle(dimension) {
        if let Some(acadrust::objects::ObjectType::Dictionary(dictionary)) =
            document.objects.get_mut(&dictionary_handle)
        {
            dictionary.entries.retain(|(name, handle)| {
                !name.eq_ignore_ascii_case("ACAD_BREAKDATA") && !handles.contains(handle)
            });
            dictionary
                .hard_owner_entries
                .retain(|name| !name.eq_ignore_ascii_case("ACAD_BREAKDATA"));
        }
    }
    true
}

fn write_dimension_break_data(
    document: &mut acadrust::CadDocument,
    dimension: Handle,
    reserved_reference: Handle,
    references: Vec<acadrust::objects::BreakPointReference>,
) -> bool {
    use acadrust::objects::{BreakData, DataObject, DataObjectData, Dictionary, ObjectType};

    if references.is_empty() {
        return remove_dimension_break_data(document, dimension);
    }
    if let Some(handle) = break_object_handle(document, dimension) {
        if let Some(ObjectType::DataObject(object)) = document.objects.get_mut(&handle) {
            object.data = DataObjectData::BreakData(BreakData {
                version: 0,
                dimension_reference: dimension,
                reserved_reference,
                point_references: references,
            });
            return true;
        }
    }

    let dictionary_handle = match document.extension_dictionary_handle(dimension) {
        Some(handle) => handle,
        None => {
            let handle = document.allocate_handle();
            let mut dictionary = Dictionary::new();
            dictionary.handle = handle;
            dictionary.owner = dimension;
            dictionary.hard_owner = true;
            document
                .objects
                .insert(handle, ObjectType::Dictionary(dictionary));
            if let Some(entity) = document.get_entity_mut(dimension) {
                entity.common_mut().xdictionary_handle = Some(handle);
            }
            handle
        }
    };

    let object_handle = document.allocate_handle();
    let mut object = DataObject::new(DataObjectData::BreakData(BreakData {
        version: 0,
        dimension_reference: dimension,
        reserved_reference,
        point_references: references,
    }));
    object.handle = object_handle;
    object.owner = dictionary_handle;
    document
        .objects
        .insert(object_handle, ObjectType::DataObject(object));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary_handle) {
        dictionary
            .entries
            .retain(|(name, _)| !name.eq_ignore_ascii_case("ACAD_BREAKDATA"));
        dictionary.add_entry("ACAD_BREAKDATA", object_handle);
        dictionary.set_entry_hard_owner("ACAD_BREAKDATA", true);
    }
    true
}

fn break_reference(
    identifier: i32,
    reference_type: i32,
    first: glam::DVec3,
    second: glam::DVec3,
) -> acadrust::objects::BreakPointReference {
    acadrust::objects::BreakPointReference {
        version: 0,
        reserved: 0,
        reference_type,
        flags: 0,
        identifier,
        first_point: acadrust::types::Vector3::new(first.x, first.y, first.z),
        second_point: acadrust::types::Vector3::new(second.x, second.y, second.z),
        trailing_version: 0,
    }
}

fn apply_dimbreak(
    scene: &mut crate::scene::Scene,
    requested: &[Handle],
    operation: crate::command::DimensionBreakOperation,
) -> DimBreakResult {
    use crate::command::DimensionBreakOperation;

    let dimensions: Vec<_> = requested
        .iter()
        .copied()
        .filter(|handle| {
            matches!(
                scene.document.get_entity(*handle),
                Some(acadrust::EntityType::Dimension(_))
            ) && scene.locked_layer_name(*handle).is_none()
        })
        .collect();
    if dimensions.is_empty() {
        return DimBreakResult {
            changed: Vec::new(),
            message: crate::t!("DIMBREAK: no editable dimensions were selected.").into_owned(),
        };
    }

    if matches!(operation, DimensionBreakOperation::Remove) {
        let changed: Vec<_> = dimensions
            .into_iter()
            .filter(|handle| remove_dimension_break_data(&mut scene.document, *handle))
            .collect();
        return DimBreakResult {
            message: crate::tf!(
                "DIMBREAK: removed breaks from {} dimension(s).",
                changed.len()
            )
            .into_owned(),
            changed,
        };
    }

    let crossing_handles: Vec<Handle> = match operation {
        DimensionBreakOperation::Object(handle) => vec![handle],
        DimensionBreakOperation::Auto => scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .filter(|handle| {
                !dimensions.contains(handle) && scene.entity_belongs_to_active_space(*handle)
            })
            .collect(),
        DimensionBreakOperation::Manual(_, _) | DimensionBreakOperation::Remove => Vec::new(),
    };
    let crossing_segments: Vec<_> = crossing_handles
        .iter()
        .flat_map(|handle| wire_segments(&scene.wire_models_for(&[*handle])))
        .collect();

    let mut pending = Vec::new();
    for dimension in &dimensions {
        let existing: Vec<_> = break_object_handle(&scene.document, *dimension)
            .and_then(|handle| scene.document.objects.get(&handle))
            .and_then(|object| match object {
                acadrust::objects::ObjectType::DataObject(object) => match &object.data {
                    acadrust::objects::DataObjectData::BreakData(data) => {
                        Some(data.point_references.clone())
                    }
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_default();
        let mut references = if matches!(operation, DimensionBreakOperation::Manual(_, _)) {
            existing
        } else {
            existing
                .into_iter()
                .filter(|reference| reference.reference_type == 2)
                .collect()
        };
        match operation {
            DimensionBreakOperation::Manual(first, second) => {
                references.push(break_reference(references.len() as i32, 2, first, second));
            }
            DimensionBreakOperation::Auto | DimensionBreakOperation::Object(_) => {
                let dimension_segments = wire_segments(&scene.wire_models_for(&[*dimension]));
                let mut intersections = Vec::new();
                for dim_segment in &dimension_segments {
                    for crossing_segment in &crossing_segments {
                        if let Some(points) =
                            segment_intersection_xy(*dim_segment, *crossing_segment)
                        {
                            let center = (points.0 + points.1) * 0.5;
                            if intersections.iter().all(
                                |(first, second): &(glam::DVec3, glam::DVec3)| {
                                    center.distance_squared((*first + *second) * 0.5) > 1.0e-8
                                },
                            ) {
                                intersections.push(points);
                            }
                        }
                    }
                }
                let start = references.len() as i32;
                references.extend(
                    intersections
                        .into_iter()
                        .enumerate()
                        .map(|(index, points)| {
                            break_reference(start + index as i32, 1, points.0, points.1)
                        }),
                );
            }
            DimensionBreakOperation::Remove => {}
        }
        pending.push((*dimension, references));
    }

    let reserved = match operation {
        DimensionBreakOperation::Object(handle) => handle,
        _ => Handle::NULL,
    };
    let changed: Vec<_> = pending
        .into_iter()
        .filter_map(|(dimension, references)| {
            write_dimension_break_data(&mut scene.document, dimension, reserved, references)
                .then_some(dimension)
        })
        .collect();
    DimBreakResult {
        message: if changed.is_empty() {
            crate::t!("DIMBREAK: no intersections were found.").into_owned()
        } else {
            crate::tf!("DIMBREAK: updated {} dimension(s).", changed.len()).into_owned()
        },
        changed,
    }
}

fn apply_dimspace(
    scene: &mut crate::scene::Scene,
    base_h: Handle,
    others: &[Handle],
    requested_spacing: Option<f64>,
) -> bool {
    use acadrust::entities::Dimension;
    let (axis, normal, definition, auto_spacing) = match scene.document.get_entity(base_h) {
        Some(acadrust::EntityType::Dimension(dimension @ Dimension::Linear(d))) => {
            let spacing = dimension_auto_spacing(
                &scene.document,
                dimension,
                scene.creation_annotation_multiplier(),
            );
            (
                [d.rotation.cos(), d.rotation.sin(), 0.0],
                [d.base.normal.x, d.base.normal.y, d.base.normal.z],
                [
                    d.definition_point.x,
                    d.definition_point.y,
                    d.definition_point.z,
                ],
                spacing,
            )
        }
        Some(acadrust::EntityType::Dimension(dimension @ Dimension::Aligned(d))) => {
            let spacing = dimension_auto_spacing(
                &scene.document,
                dimension,
                scene.creation_annotation_multiplier(),
            );
            (
                [
                    d.second_point.x - d.first_point.x,
                    d.second_point.y - d.first_point.y,
                    d.second_point.z - d.first_point.z,
                ],
                [d.base.normal.x, d.base.normal.y, d.base.normal.z],
                [
                    d.definition_point.x,
                    d.definition_point.y,
                    d.definition_point.z,
                ],
                spacing,
            )
        }
        _ => return false,
    };
    let Some(frame) = cadkernel::space::dimension_spacing_frame(axis, normal, definition) else {
        return false;
    };

    let effective_spacing = requested_spacing.unwrap_or(auto_spacing);
    let mut changes = Vec::new();
    for (idx, &h) in others.iter().enumerate() {
        let target = frame.coordinate + effective_spacing * (idx + 1) as f64;
        let mut changed = false;
        if let Some(acadrust::EntityType::Dimension(dim)) = scene.document.get_entity_mut(h) {
            let slide = |p: &mut acadrust::types::Vector3| {
                let point =
                    cadkernel::space::move_to_dimension_spacing([p.x, p.y, p.z], frame, target);
                *p = acadrust::types::Vector3::new(point[0], point[1], point[2]);
            };
            match dim {
                Dimension::Linear(d) => {
                    slide(&mut d.definition_point);
                    d.base.definition_point = d.definition_point;
                    changed = true;
                }
                Dimension::Aligned(d) => {
                    slide(&mut d.definition_point);
                    d.base.definition_point = d.definition_point;
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            // The dimension line moved, so its baked *D block is stale — drop it
            // so the next save re-bakes it. (#181)
            scene.invalidate_dim_block_recorded(h);
            changes.push((h, crate::scene::ChangeKind::Modified));
        }
    }
    if !changes.is_empty() {
        scene.bump_entities(&changes);
    }
    !changes.is_empty()
}

fn dimension_auto_spacing(
    document: &acadrust::CadDocument,
    dimension: &acadrust::entities::Dimension,
    annotation_multiplier: f64,
) -> f64 {
    let style = document
        .dim_styles
        .iter()
        .find(|style| {
            style
                .name
                .eq_ignore_ascii_case(&dimension.base().style_name)
        })
        .map(|style| {
            crate::entities::dimension::resolved_dimension_style(style, dimension, document)
        });
    style
        .map(|style| {
            let scale = if style.dimscale > 1.0e-9 {
                style.dimscale
            } else {
                annotation_multiplier.max(1.0e-9)
            };
            (style.dimdli.abs() * scale).max(1.0e-6)
        })
        .unwrap_or(3.75)
}

fn apply_mleader_align(
    scene: &mut crate::scene::Scene,
    handles: &[acadrust::Handle],
    from: glam::DVec3,
    to: glam::DVec3,
) -> bool {
    use cadkernel::geom2d::{closest_point, Curve, Vec2, XLine};

    let Some(direction) = Vec2::new(to.x - from.x, to.y - from.y).normalize() else {
        return false;
    };
    let line = Curve::XLine(XLine {
        base: [from.x, from.y],
        direction: direction.into(),
    });
    let mut changed = Vec::new();
    for &handle in handles {
        if let Some(acadrust::EntityType::MultiLeader(ml)) = scene.document.get_entity_mut(handle) {
            let old = ml.context.content_base_point;
            let projected = closest_point(&line, [old.x, old.y]).point;
            let new_x = projected[0];
            let new_y = projected[1];
            let shift_x = new_x - old.x;
            let shift_y = new_y - old.y;
            if shift_x.abs() <= 1.0e-12 && shift_y.abs() <= 1.0e-12 {
                continue;
            }
            ml.context.content_base_point.x = new_x;
            ml.context.content_base_point.y = new_y;
            ml.context.text_location.x += shift_x;
            ml.context.text_location.y += shift_y;
            ml.context.block_content_location.x += shift_x;
            ml.context.block_content_location.y += shift_y;
            for root in &mut ml.context.leader_roots {
                root.connection_point.x += shift_x;
                root.connection_point.y += shift_y;
            }
            changed.push((handle, crate::scene::ChangeKind::Modified));
        }
    }
    if !changed.is_empty() {
        scene.bump_entities(&changed);
    }
    !changed.is_empty()
}

fn compatible_mleader_collect_handles(
    scene: &crate::scene::Scene,
    handles: &[acadrust::Handle],
) -> Vec<acadrust::Handle> {
    let Some((base_block, base_style)) = handles.iter().find_map(|handle| {
        if scene.is_layer_locked(*handle) {
            return None;
        }
        let acadrust::EntityType::MultiLeader(leader) = scene.document.get_entity(*handle)? else {
            return None;
        };
        (leader.content_type == acadrust::entities::LeaderContentType::Block)
            .then_some((leader.block_content_handle, leader.style_handle))
    }) else {
        return Vec::new();
    };
    let mut compatible = Vec::new();
    for handle in handles.iter().copied() {
        if compatible.contains(&handle) {
            continue;
        }
        if !scene.is_layer_locked(handle)
            && matches!(
                scene.document.get_entity(handle),
                Some(acadrust::EntityType::MultiLeader(leader))
                    if leader.content_type == acadrust::entities::LeaderContentType::Block
                        && leader.block_content_handle == base_block
                        && leader.style_handle == base_style
            )
        {
            compatible.push(handle);
        }
    }
    compatible
}

fn apply_mleader_collect(
    scene: &mut crate::scene::Scene,
    handles: &[acadrust::Handle],
    point: glam::DVec3,
) -> bool {
    if handles.len() < 2 {
        return false;
    }
    let px = point.x;
    let py = point.y;
    let Some(acadrust::EntityType::MultiLeader(base)) = scene.document.get_entity(handles[0])
    else {
        return false;
    };
    let base_block = base.block_content_handle;
    let base_style = base.style_handle;

    let mut extra_roots: Vec<acadrust::entities::LeaderRoot> = Vec::new();
    let mut merged_handles = Vec::new();
    for &h in &handles[1..] {
        if let Some(acadrust::EntityType::MultiLeader(ml)) = scene.document.get_entity(h) {
            if ml.content_type == acadrust::entities::LeaderContentType::Block
                && ml.block_content_handle == base_block
                && ml.style_handle == base_style
            {
                let shift_x = px - ml.context.content_base_point.x;
                let shift_y = py - ml.context.content_base_point.y;
                extra_roots.extend(ml.context.leader_roots.iter().cloned().map(|mut root| {
                    root.connection_point.x += shift_x;
                    root.connection_point.y += shift_y;
                    root
                }));
                merged_handles.push(h);
            }
        }
    }
    if merged_handles.is_empty() {
        return false;
    }

    if let Some(acadrust::EntityType::MultiLeader(ml)) = scene.document.get_entity_mut(handles[0]) {
        let shift_x = px - ml.context.content_base_point.x;
        let shift_y = py - ml.context.content_base_point.y;
        for root in &mut ml.context.leader_roots {
            root.connection_point.x += shift_x;
            root.connection_point.y += shift_y;
        }
        ml.context.content_base_point.x = px;
        ml.context.content_base_point.y = py;
        ml.context.block_content_location.x = px;
        ml.context.block_content_location.y = py;
        ml.context.text_location.x = px;
        ml.context.text_location.y = py;
        for root in extra_roots {
            ml.context.leader_roots.push(root);
        }
    }

    scene.erase_entities(&merged_handles);
    scene.bump_entities(&[(handles[0], crate::scene::ChangeKind::Modified)]);
    true
}

/// MATCHPROP special properties (#281): copy every STYLE-affecting field from
/// `src` to `dst` when the destination supports it — never content (text
/// strings, block names) or placement (positions, rotations of the object
/// itself). Text formatting crosses TEXT ↔ MTEXT; the dimension style crosses
/// Dimension / Leader / Tolerance.
fn match_special_props(src: &acadrust::EntityType, dst: &mut acadrust::EntityType) {
    use acadrust::EntityType as E;

    // Text-ish source formatting.
    let text_fmt = match src {
        E::Text(t) => Some((
            t.style.clone(),
            t.height,
            Some(t.width_factor),
            Some(t.oblique_angle),
        )),
        E::MText(m) => Some((m.style.clone(), m.height, None, None)),
        E::AttributeDefinition(a) => Some((
            a.text_style.clone(),
            a.height,
            Some(a.width_factor),
            Some(a.oblique_angle),
        )),
        E::AttributeEntity(a) => Some((
            a.text_style.clone(),
            a.height,
            Some(a.width_factor),
            Some(a.oblique_angle),
        )),
        _ => None,
    };
    if let Some((style, height, wf, ob)) = text_fmt {
        match dst {
            E::Text(t) => {
                t.style = style.clone();
                t.height = height;
                if let Some(value) = wf {
                    t.width_factor = value;
                }
                if let Some(value) = ob {
                    t.oblique_angle = value;
                }
            }
            E::MText(m) => {
                m.style = style.clone();
                m.height = height;
            }
            E::AttributeDefinition(a) => {
                a.text_style = style.clone();
                a.height = height;
                if let Some(value) = wf {
                    a.width_factor = value;
                }
                if let Some(value) = ob {
                    a.oblique_angle = value;
                }
            }
            E::AttributeEntity(a) => {
                a.text_style = style;
                a.height = height;
                if let Some(value) = wf {
                    a.width_factor = value;
                }
                if let Some(value) = ob {
                    a.oblique_angle = value;
                }
            }
            _ => {}
        }
    }
    // MText-only extras (line spacing, background fill).
    if let (E::MText(sm), E::MText(dm)) = (src, dst as &mut E) {
        dm.drawing_direction = sm.drawing_direction;
        dm.line_spacing_factor = sm.line_spacing_factor;
        dm.line_spacing_style = sm.line_spacing_style;
        dm.background_fill_flags = sm.background_fill_flags;
        dm.background_scale = sm.background_scale;
        dm.background_color = sm.background_color;
        dm.background_transparency = sm.background_transparency;
    }

    // Dimension style name crosses the three dim-styled families.
    let dim_style = match src {
        E::Dimension(d) => Some(d.base().style_name.clone()),
        E::Leader(l) => Some(l.dimension_style.clone()),
        E::Tolerance(t) => Some(t.dimension_style_name.clone()),
        _ => None,
    };
    if let Some(ds) = dim_style {
        match dst {
            E::Dimension(d) => d.base_mut().style_name = ds,
            E::Leader(l) => l.dimension_style = ds,
            E::Tolerance(t) => t.dimension_style_name = ds,
            _ => {}
        }
    }

    // Hatch pattern / gradient — everything but the boundary.
    if let (E::Hatch(sh), E::Hatch(dh)) = (src, dst as &mut E) {
        dh.pattern = sh.pattern.clone();
        dh.pattern_type = sh.pattern_type;
        dh.pattern_angle = sh.pattern_angle;
        dh.pattern_scale = sh.pattern_scale;
        dh.is_solid = sh.is_solid;
        dh.is_double = sh.is_double;
        dh.style = sh.style;
        dh.gradient_color = sh.gradient_color.clone();
    }

    // Polyline display style crosses lightweight and legacy 2D polylines.
    // Per-vertex/tapered widths are resampled over the destination vertices;
    // they must not be flattened into the source's constant-width field.
    if let Some(style) = PolylineMatchStyle::from_entity(src) {
        style.apply_to(dst);
    }

    if let (E::Leader(sl), E::Leader(dl)) = (src, dst as &mut E) {
        dl.arrow_enabled = sl.arrow_enabled;
        dl.path_type = sl.path_type;
        dl.hookline_direction = sl.hookline_direction;
        dl.hookline_enabled = sl.hookline_enabled;
        dl.override_color = sl.override_color;
        dl.dimension_gap = sl.dimension_gap;
        dl.arrowhead_type = sl.arrowhead_type;
        dl.arrow_size = sl.arrow_size;
        dl.byblock_color = sl.byblock_color;
    }
    if let (E::Tolerance(st), E::Tolerance(dt)) = (src, dst as &mut E) {
        dt.dimension_style_handle = st.dimension_style_handle;
        dt.text_height = st.text_height;
        dt.dimension_gap = st.dimension_gap;
    }

    // Paper-space geometry and view position stay; viewport display/plot
    // styling and effective scale follow the source.
    if let (E::Viewport(sv), E::Viewport(dv)) = (src, dst as &mut E) {
        let scale = crate::scene::vp_effective_scale(sv.custom_scale, sv.view_height, sv.height);
        dv.custom_scale = scale;
        if scale.abs() > 1e-9 {
            dv.view_height = dv.height / scale;
        }
        dv.status.locked = sv.status.locked;
        dv.status.hide_plot = sv.status.hide_plot;
        dv.render_mode = sv.render_mode;
        dv.style_sheet = sv.style_sheet.clone();
        dv.shade_plot_mode = sv.shade_plot_mode;
        dv.background_handle = sv.background_handle;
        dv.shade_plot_handle = sv.shade_plot_handle;
        dv.visual_style_handle = sv.visual_style_handle;
        dv.default_lighting = sv.default_lighting;
        dv.default_lighting_type = sv.default_lighting_type;
        dv.brightness = sv.brightness;
        dv.contrast = sv.contrast;
        dv.ambient_color = sv.ambient_color;
    }

    if let (E::Table(st), E::Table(dt)) = (src, dst as &mut E) {
        dt.table_style_handle = st.table_style_handle;
        dt.base_style = st.base_style.clone();
        dt.override_flag = st.override_flag;
        dt.override_border_color = st.override_border_color;
        dt.override_border_line_weight = st.override_border_line_weight;
        dt.override_border_visibility = st.override_border_visibility;
        dt.legacy_style_override = st.legacy_style_override.clone();
        dt.legacy_border_colors = st.legacy_border_colors.clone();
        dt.legacy_border_line_weights = st.legacy_border_line_weights.clone();
        dt.legacy_border_visibility = st.legacy_border_visibility.clone();
    }

    if let (E::MLine(sm), E::MLine(dm)) = (src, dst as &mut E) {
        dm.style_handle = sm.style_handle;
        dm.style_name = sm.style_name.clone();
        dm.style_element_count = sm.style_element_count;
        dm.justification = sm.justification;
        dm.scale_factor = sm.scale_factor;
        // Stored segment offsets bake the old style into each vertex. Empty
        // data deliberately selects the renderer's style-derived fallback.
        for vertex in &mut dm.vertices {
            vertex.segments.clear();
        }
    }

    // MultiLeader style + every style-affecting override; content and
    // geometry (leader points, text, block handle) stay.
    if let (E::MultiLeader(sm), E::MultiLeader(dm)) = (src, dst as &mut E) {
        dm.style_handle = sm.style_handle;
        dm.path_type = sm.path_type;
        dm.line_color = sm.line_color;
        dm.line_type_handle = sm.line_type_handle;
        dm.line_weight = sm.line_weight;
        dm.enable_landing = sm.enable_landing;
        dm.enable_dogleg = sm.enable_dogleg;
        dm.dogleg_length = sm.dogleg_length;
        dm.arrowhead_handle = sm.arrowhead_handle;
        dm.arrowhead_size = sm.arrowhead_size;
        dm.text_style_handle = sm.text_style_handle;
        dm.text_color = sm.text_color;
        dm.text_frame = sm.text_frame;
        dm.text_height = sm.text_height;
        dm.text_left_attachment = sm.text_left_attachment;
        dm.text_right_attachment = sm.text_right_attachment;
        dm.text_top_attachment = sm.text_top_attachment;
        dm.text_bottom_attachment = sm.text_bottom_attachment;
        dm.text_attachment_direction = sm.text_attachment_direction;
        dm.text_attachment_point = sm.text_attachment_point;
        dm.text_alignment = sm.text_alignment;
        dm.text_angle_type = sm.text_angle_type;
        dm.text_direction_negative = sm.text_direction_negative;
        dm.text_align_in_ipe = sm.text_align_in_ipe;
        dm.block_content_color = sm.block_content_color;
        dm.block_connection_type = sm.block_connection_type;
        dm.block_scale = sm.block_scale;
        dm.scale_factor = sm.scale_factor;
        dm.property_override_flags = sm.property_override_flags;
        dm.enable_annotation_scale = sm.enable_annotation_scale;
        dm.extend_leader_to_text = sm.extend_leader_to_text;
        dm.arrowhead_overrides = sm.arrowhead_overrides.clone();

        let sc = &sm.context;
        let dc = &mut dm.context;
        dc.scale_factor = sc.scale_factor;
        dc.text_height = sc.text_height;
        dc.text_width = sc.text_width;
        dc.text_boundary_height = sc.text_boundary_height;
        dc.line_spacing_factor = sc.line_spacing_factor;
        dc.line_spacing_style = sc.line_spacing_style;
        dc.text_color = sc.text_color;
        dc.text_attachment_point = sc.text_attachment_point;
        dc.text_flow_direction = sc.text_flow_direction;
        dc.text_alignment = sc.text_alignment;
        dc.text_left_attachment = sc.text_left_attachment;
        dc.text_right_attachment = sc.text_right_attachment;
        dc.text_top_attachment = sc.text_top_attachment;
        dc.text_bottom_attachment = sc.text_bottom_attachment;
        dc.text_height_automatic = sc.text_height_automatic;
        dc.word_break = sc.word_break;
        dc.text_style_handle = sc.text_style_handle;
        dc.block_content_scale = sc.block_content_scale;
        dc.block_content_color = sc.block_content_color;
        dc.block_connection_type = sc.block_connection_type;
        dc.column_type = sc.column_type;
        dc.column_width = sc.column_width;
        dc.column_gutter = sc.column_gutter;
        dc.column_flow_reversed = sc.column_flow_reversed;
        dc.column_sizes = sc.column_sizes.clone();
        dc.background_fill_enabled = sc.background_fill_enabled;
        dc.background_mask_fill_on = sc.background_mask_fill_on;
        dc.background_fill_color = sc.background_fill_color;
        dc.background_scale_factor = sc.background_scale_factor;
        dc.background_transparency = sc.background_transparency;
        dc.arrowhead_size = sc.arrowhead_size;
        dc.landing_gap = sc.landing_gap;
        dc.scale_handle = sc.scale_handle;
    }

    // External-reference identity, placement and clip geometry stay. Only
    // display controls/appearance are matched.
    if let (E::RasterImage(si), E::RasterImage(di)) = (src, dst as &mut E) {
        di.flags = si.flags;
        di.clipping_enabled = si.clipping_enabled;
        di.brightness = si.brightness;
        di.contrast = si.contrast;
        di.fade = si.fade;
        di.clip_boundary.clip_mode = si.clip_boundary.clip_mode;
    }
    if let (E::Wipeout(sw), E::Wipeout(dw)) = (src, dst as &mut E) {
        dw.flags = sw.flags;
        dw.clipping_enabled = sw.clipping_enabled;
        dw.brightness = sw.brightness;
        dw.contrast = sw.contrast;
        dw.fade = sw.fade;
        dw.clip_mode = sw.clip_mode;
    }
    if let (E::Underlay(su), E::Underlay(du)) = (src, dst as &mut E) {
        du.flags = su.flags;
        du.contrast = su.contrast;
        du.fade = su.fade;
        du.clip_inverted = su.clip_inverted;
    }
}

struct PolylineMatchStyle {
    plinegen: bool,
    widths: Vec<(f64, f64)>,
}

impl PolylineMatchStyle {
    fn from_entity(entity: &acadrust::EntityType) -> Option<Self> {
        use acadrust::EntityType as E;

        match entity {
            E::LwPolyline(poly) => {
                let widths = if poly.vertices.is_empty() {
                    vec![(poly.constant_width, poly.constant_width)]
                } else {
                    poly.vertices
                        .iter()
                        .map(|v| {
                            (
                                width_or_default(v.start_width, poly.constant_width),
                                width_or_default(v.end_width, poly.constant_width),
                            )
                        })
                        .collect()
                };
                Some(Self {
                    plinegen: poly.plinegen,
                    widths,
                })
            }
            E::Polyline2D(poly) => {
                let widths = if poly.vertices.is_empty() {
                    vec![(poly.start_width, poly.end_width)]
                } else {
                    poly.vertices
                        .iter()
                        .map(|v| {
                            (
                                width_or_default(v.start_width, poly.start_width),
                                width_or_default(v.end_width, poly.end_width),
                            )
                        })
                        .collect()
                };
                Some(Self {
                    plinegen: poly.flags.bits()
                        & acadrust::entities::PolylineFlags::LINETYPE_CONTINUOUS.bits()
                        != 0,
                    widths,
                })
            }
            _ => None,
        }
    }

    fn apply_to(&self, entity: &mut acadrust::EntityType) {
        use acadrust::EntityType as E;

        match entity {
            E::LwPolyline(poly) => {
                poly.plinegen = self.plinegen;
                let sampled = resample_widths(&self.widths, poly.vertices.len());
                let first = sampled.first().copied().unwrap_or((0.0, 0.0));
                let can_use_constant = widths_are_same(&sampled) && nearly_equal(first.0, first.1);
                poly.constant_width = if can_use_constant { first.0 } else { 0.0 };
                for (vertex, (start, end)) in poly.vertices.iter_mut().zip(sampled) {
                    if can_use_constant {
                        vertex.start_width = 0.0;
                        vertex.end_width = 0.0;
                    } else {
                        vertex.start_width = start;
                        vertex.end_width = end;
                    }
                }
            }
            E::Polyline2D(poly) => {
                let mut bits = poly.flags.bits();
                let flag = acadrust::entities::PolylineFlags::LINETYPE_CONTINUOUS.bits();
                if self.plinegen {
                    bits |= flag;
                } else {
                    bits &= !flag;
                }
                poly.flags = acadrust::entities::PolylineFlags::from_bits(bits);
                let sampled = resample_widths(&self.widths, poly.vertices.len());
                let first = sampled.first().copied().unwrap_or((0.0, 0.0));
                let can_use_defaults = widths_are_same(&sampled);
                if can_use_defaults {
                    poly.start_width = first.0;
                    poly.end_width = first.1;
                } else {
                    poly.start_width = 0.0;
                    poly.end_width = 0.0;
                }
                for (vertex, (start, end)) in poly.vertices.iter_mut().zip(sampled) {
                    if can_use_defaults {
                        vertex.start_width = 0.0;
                        vertex.end_width = 0.0;
                    } else {
                        vertex.start_width = start;
                        vertex.end_width = end;
                    }
                }
            }
            _ => {}
        }
    }
}

fn width_or_default(value: f64, default: f64) -> f64 {
    if value.abs() <= 1e-12 {
        default
    } else {
        value
    }
}

fn nearly_equal(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}

fn widths_are_same(widths: &[(f64, f64)]) -> bool {
    let Some(first) = widths.first() else {
        return true;
    };
    widths
        .iter()
        .all(|value| nearly_equal(value.0, first.0) && nearly_equal(value.1, first.1))
}

fn resample_widths(source: &[(f64, f64)], count: usize) -> Vec<(f64, f64)> {
    if count == 0 || source.is_empty() {
        return Vec::new();
    }
    if count == 1 || source.len() == 1 {
        return vec![source[0]; count];
    }
    (0..count)
        .map(|index| {
            let source_index = index.saturating_mul(source.len() - 1) / count.saturating_sub(1);
            source[source_index]
        })
        .collect()
}

#[cfg(test)]
mod command_replacement_tests {
    use super::*;
    use acadrust::entities::{Circle, Line};
    use acadrust::types::Vector3;

    #[test]
    fn one_to_one_edits_keep_identity_but_type_changes_do_not() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let handle = app.tabs[tab]
            .scene
            .add_entity(acadrust::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )));
        let owner = app.tabs[tab]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .owner_handle;

        let kept = app.replace_command_entity(
            tab,
            handle,
            vec![acadrust::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(3.0, 0.0, 0.0),
            ))],
        );
        assert_eq!(kept, vec![handle]);
        let edited = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        assert_eq!(edited.common().owner_handle, owner);
        assert!(matches!(edited, acadrust::EntityType::Line(line) if line.end.x == 3.0));

        let allocated = app.replace_command_entity(
            tab,
            handle,
            vec![acadrust::EntityType::Circle(Circle::from_coords(
                0.0, 0.0, 0.0, 2.0,
            ))],
        );
        assert_eq!(allocated.len(), 1);
        assert_ne!(allocated[0], handle);
        assert!(app.tabs[tab].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn live_geometry_updates_keep_document_identity_and_display_properties() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let handle = app.tabs[tab]
            .scene
            .add_entity(acadrust::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )));
        let expected = {
            let entity = app.tabs[tab].scene.document.get_entity_mut(handle).unwrap();
            entity.common_mut().layer = "LIVE".to_string();
            entity.common_mut().invisible = true;
            entity.common_mut().linetype_scale = 2.5;
            entity.common().clone()
        };

        let _ = app.apply_cmd_result(CmdResult::UpdateLiveEntity {
            handle,
            entity: acadrust::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(4.0, 0.0, 0.0),
            )),
            finish: false,
        });

        let updated = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        assert_eq!(updated.common().handle, expected.handle);
        assert_eq!(updated.common().owner_handle, expected.owner_handle);
        assert_eq!(updated.common().layer, expected.layer);
        assert_eq!(updated.common().invisible, expected.invisible);
        assert_eq!(updated.common().linetype_scale, expected.linetype_scale);
        assert!(matches!(updated, acadrust::EntityType::Line(line) if line.end.x == 4.0));
    }
}

#[cfg(test)]
mod parametric_constraint_undo_tests {
    use super::*;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
        app.tabs[app.active_tab]
            .scene
            .add_entity(acadrust::EntityType::Line(
                acadrust::entities::Line::from_points(
                    acadrust::types::Vector3::new(x1, y1, 0.0),
                    acadrust::types::Vector3::new(x2, y2, 0.0),
                ),
            ))
    }

    fn line_angle_deg(app: &OpenCADStudio, handle: Handle) -> f64 {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(l)) => (l.end.y - l.start.y)
                .atan2(l.end.x - l.start.x)
                .to_degrees(),
            other => panic!("expected a Line, got {other:?}"),
        }
    }

    /// Constraint state and solved geometry share one undo/redo transaction.
    #[test]
    fn adding_a_constraint_is_undoable_and_redoable_atomically() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 3.0);
        let original_angle = line_angle_deg(&app, line);
        assert!((original_angle - 16.699244).abs() < 1e-3);

        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(line)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        assert!(
            line_angle_deg(&app, line).abs() < 1e-6,
            "constraint should have leveled the line"
        );
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1
        );

        app.undo_steps(1);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map(|s| s.constraints.len())
                .unwrap_or(0),
            0,
            "undo should remove the constraint record"
        );
        assert!(
            (line_angle_deg(&app, line) - original_angle).abs() < 1e-3,
            "undo should restore the pre-constraint angle"
        );

        app.redo_steps(1);
        assert!(
            line_angle_deg(&app, line).abs() < 1e-6,
            "redo should re-level the line"
        );
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1,
            "redo should restore the constraint record"
        );
    }

    #[test]
    fn perpendicular_initial_solve_rotates_parallel_second_line_in_kernel() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let first = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let second = add_line(&mut app, 20.0, 0.0, 30.0, 0.0);

        let _ = app.apply_cmd_result(CmdResult::AddPerpendicularConstraint {
            first: ParametricRef::whole(first),
            second: ParametricRef::whole(second),
            first_fixed: ParametricRef::whole(first),
            second_start: ParametricRef::point(second, 0),
            label: "Perpendicular constraint",
        });

        let line = |handle| match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(line)) => line.clone(),
            other => panic!("expected a Line, got {other:?}"),
        };
        let fixed = line(first);
        let moving = line(second);
        assert_eq!(fixed.start, acadrust::types::Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(fixed.end, acadrust::types::Vector3::new(10.0, 0.0, 0.0));
        assert!((moving.start - acadrust::types::Vector3::new(20.0, 0.0, 0.0)).length() < 1.0e-9);
        assert!((moving.length() - 10.0).abs() < 1.0e-7);
        assert!(
            (fixed.end - fixed.start)
                .dot(&(moving.end - moving.start))
                .abs()
                < 1.0e-7,
            "fixed={fixed:?}, moving={moving:?}"
        );
    }

    #[test]
    fn horizontal_initial_solve_uses_the_captured_axis_in_kernel() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let handle = add_line(&mut app, 0.0, 0.0, 5.0, 2.0);
        let (original_start, original_end, original_length) = match app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
        {
            Some(acadrust::EntityType::Line(line)) => (line.start, line.end, line.length()),
            other => panic!("expected a Line, got {other:?}"),
        };
        let direction = acadrust::types::Vector3::new(3.0, 4.0, 0.0).normalize();

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Reference(
                ParametricRef::whole(handle),
            ),
            direction,
            label: "Horizontal constraint",
        });

        let line = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(line)) => line,
            other => panic!("expected a Line, got {other:?}"),
        };
        let solved = (line.end - line.start).normalize();
        assert!(solved.cross(&direction).length() < 1.0e-7);
        assert!((line.length() - original_length).abs() < 1.0e-7);
        let constraint = &app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap()
            .constraints[0];
        assert_eq!(constraint.axis_direction, Some(direction));

        app.undo_steps(1);
        let line = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(line)) => line,
            other => panic!("expected a Line after undo, got {other:?}"),
        };
        assert_eq!((line.start, line.end), (original_start, original_end));
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map(|set| set.constraints.len())
                .unwrap_or(0),
            0
        );

        app.redo_steps(1);
        let line = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(line)) => line,
            other => panic!("expected a Line after redo, got {other:?}"),
        };
        assert!((line.end - line.start).normalize().cross(&direction).length() < 1.0e-7);
    }

    #[test]
    fn horizontal_two_point_solve_keeps_the_first_point_fixed() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let first = add_line(&mut app, 0.0, 0.0, 0.0, 5.0);
        let second = add_line(&mut app, 8.0, 3.0, 8.0, 7.0);
        let pick = |handle, point| crate::command::CoincidentPick {
            handle: Some(handle),
            point,
            whole_curve: false,
        };

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Points(
                pick(first, glam::DVec3::ZERO),
                pick(second, glam::DVec3::new(8.0, 3.0, 0.0)),
            ),
            direction: acadrust::types::Vector3::UNIT_X,
            label: "Horizontal constraint",
        });

        let line = |handle| match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(line)) => line,
            other => panic!("expected a Line, got {other:?}"),
        };
        assert_eq!(line(first).start, acadrust::types::Vector3::ZERO);
        assert!(line(second).start.y.abs() < 1.0e-8);
    }

    #[test]
    fn horizontal_minor_axis_rotates_an_ellipse_around_its_center() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let mut ellipse = acadrust::entities::Ellipse::default();
        ellipse.center = acadrust::types::Vector3::new(3.0, 7.0, 0.0);
        ellipse.major_axis = acadrust::types::Vector3::new(3.0, 4.0, 0.0);
        ellipse.minor_axis_ratio = 0.4;
        let handle = app.tabs[app.active_tab]
            .scene
            .add_entity(acadrust::EntityType::Ellipse(ellipse));

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Reference(
                ParametricRef::ellipse_minor_axis(handle),
            ),
            direction: acadrust::types::Vector3::UNIT_X,
            label: "Horizontal constraint",
        });

        let ellipse = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Ellipse(ellipse)) => ellipse,
            other => panic!("expected an Ellipse, got {other:?}"),
        };
        assert!((ellipse.center - acadrust::types::Vector3::new(3.0, 7.0, 0.0)).length() < 1.0e-9);
        assert!(ellipse.major_axis.x.abs() < 1.0e-7);
        assert!((ellipse.major_axis.length() - 5.0).abs() < 1.0e-7);
        assert!((ellipse.minor_axis_ratio - 0.4).abs() < 1.0e-9);
    }

    /// Drawing commands must not create parametric constraints implicitly.
    #[test]
    fn drawing_a_line_onto_an_existing_endpoint_does_not_add_a_constraint() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _existing = add_line(&mut app, 0.0, 0.0, 5.0, 0.0);

        let new_line = acadrust::EntityType::Line(acadrust::entities::Line::from_points(
            acadrust::types::Vector3::new(5.0, 0.0, 0.0), // exactly on `existing`'s end point
            acadrust::types::Vector3::new(10.0, 0.0, 0.0),
        ));
        app.tabs[app.active_tab].active_cmd = Some(Box::new(
            crate::modules::draw::draw::line::LineCommand::new(),
        ));
        let _ = app.apply_cmd_result(CmdResult::CommitEntity(new_line));

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map(|s| s.constraints.len())
                .unwrap_or(0),
            0,
            "a drawing command must not add an implicit constraint"
        );
    }

    /// Conflict resolution removes one flagged constraint and is undoable.
    #[test]
    fn resolve_one_parametric_conflict_removes_a_flagged_constraint_and_is_undoable() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 3.0);

        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(line)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(line)],
            driving_param: None,
            label: "Horizontal constraint",
        }); // exact duplicate — flagged redundant once bumped
        app.tabs[app.active_tab]
            .scene
            .bump_entities(&[(line, crate::scene::ChangeKind::Modified)]);

        let set = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap();
        assert_eq!(set.constraints.len(), 2);
        assert_eq!(
            set.conflicts.len(),
            1,
            "the duplicate should have been flagged"
        );

        app.resolve_one_parametric_conflict();
        let set = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap();
        assert_eq!(
            set.constraints.len(),
            1,
            "one of the two duplicates should have been removed"
        );

        app.undo_steps(1);
        let set = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap();
        assert_eq!(
            set.constraints.len(),
            2,
            "undo should restore the removed constraint"
        );
    }

    #[test]
    fn deleting_one_parametric_constraint_is_undoable() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 3.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(line)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        let id = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap()
            .constraints[0]
            .id;
        app.tabs[app.active_tab].scene.selected_constraint = Some(id);

        app.delete_parametric_constraint(id);
        assert!(app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap()
            .constraints
            .is_empty());
        assert_eq!(app.tabs[app.active_tab].scene.selected_constraint, None);

        app.undo_steps(1);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1
        );
    }

    /// `named_parameters_design.md` stage 4: applying the PARAMETERS editor's
    /// working buffer both defines the parameter and re-solves whatever
    /// constraint already references it by name — the entire point of a
    /// *named* parameter, not just data plumbing.
    #[test]
    fn applying_named_parameter_rows_defines_and_resolves_referencing_constraints() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);

        // The constraint is added *before* the parameter exists -- allowed,
        // same as `ParameterTable::set`'s own forward-reference tolerance;
        // it just doesn't resolve to anything until the parameter is defined.
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Distance,
            refs: vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            driving_param: Some(crate::scene::named_parameters::DrivingValue::Named(
                "target_len".to_string(),
            )),
            label: "Distance constraint",
        });

        app.named_parameter_editor_rows =
            vec![crate::ui::window::named_parameters::ParamEditorRow {
                name: "target_len".to_string(),
                formula: "8".to_string(),
            }];
        app.apply_named_parameter_editor_rows();

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("target_len"),
            Ok(8.0)
        );
        let (start, end) = match app.tabs[app.active_tab].scene.document.get_entity(line) {
            Some(acadrust::EntityType::Line(l)) => (l.start, l.end),
            other => panic!("expected a Line, got {other:?}"),
        };
        let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
        assert!(
            (len - 8.0).abs() < 1e-6,
            "length should track the applied parameter, got {len}"
        );

        // Redefining the parameter and re-applying should ripple again.
        app.named_parameter_editor_rows[0].formula = "3".to_string();
        app.apply_named_parameter_editor_rows();
        let (start, end) = match app.tabs[app.active_tab].scene.document.get_entity(line) {
            Some(acadrust::EntityType::Line(l)) => (l.start, l.end),
            other => panic!("expected a Line, got {other:?}"),
        };
        let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
        assert!(
            (len - 3.0).abs() < 1e-6,
            "length should track the redefined parameter, got {len}"
        );

        app.undo_steps(1);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("target_len"),
            Ok(8.0)
        );
        let length_after_undo = match app.tabs[app.active_tab].scene.document.get_entity(line) {
            Some(acadrust::EntityType::Line(line)) => line.length(),
            other => panic!("expected a Line, got {other:?}"),
        };
        assert!((length_after_undo - 8.0).abs() < 1e-6);

        app.redo_steps(1);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("target_len"),
            Ok(3.0)
        );
    }

    /// The no-selection page must expose the active drawing's named parameters.
    #[test]
    fn no_selection_properties_exposes_named_parameters() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.tabs[app.active_tab]
            .scene
            .named_parameters_mut()
            .set("width", "12")
            .unwrap();

        app.refresh_properties();

        assert!(app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .any(|property| matches!(
                &property.value,
                crate::scene::model::object::PropValue::ParamRow { name, .. }
                    if name == "width"
            )));
        assert!(app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .any(|property| matches!(
                property.value,
                crate::scene::model::object::PropValue::ParamsVisibilityToggle(_)
            )));
    }

    /// End-to-end through the actual `Message` handlers a Properties-panel
    /// click/keystroke would fire — not `apply_named_parameter_editor_rows`
    /// directly — covering the whole Parameters-section lifecycle: add,
    /// rename, redefine, and confirm a referencing constraint re-solves
    /// after each commit (not just after a whole-table Apply, since this
    /// path commits per field).
    #[test]
    fn properties_panel_parameter_row_add_rename_redefine_and_delete() {
        use crate::ui::window::named_parameters::ParamField;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Distance,
            refs: vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            driving_param: Some(crate::scene::named_parameters::DrivingValue::Named(
                "len".to_string(),
            )),
            label: "Distance constraint",
        });

        // Add: the first row is auto-named "param1".
        let _ = app.update(Message::PropParamAddNew);
        assert!(app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .contains("param1"));
        let index = app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .iter()
            .position(|p| p.name == "param1")
            .unwrap();

        // Rename param1 -> len (commit-on-submit, not per keystroke: input
        // alone must not touch the table yet).
        let _ = app.update(Message::PropParamInput {
            index,
            field: ParamField::Name,
            value: "len".to_string(),
        });
        assert!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .contains("param1"),
            "typing alone must not commit"
        );
        let _ = app.update(Message::PropParamCommit {
            index,
            field: ParamField::Name,
        });
        assert!(!app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .contains("param1"));
        assert!(app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .contains("len"));

        // Redefine its formula to 8 and confirm the Distance constraint
        // (already referencing "len" by name, defined before this) re-solves.
        let _ = app.update(Message::PropParamInput {
            index,
            field: ParamField::Formula,
            value: "8".to_string(),
        });
        let _ = app.update(Message::PropParamCommit {
            index,
            field: ParamField::Formula,
        });
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("len"),
            Ok(8.0)
        );
        let (start, end) = match app.tabs[app.active_tab].scene.document.get_entity(line) {
            Some(acadrust::EntityType::Line(l)) => (l.start, l.end),
            other => panic!("expected a Line, got {other:?}"),
        };
        let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
        assert!(
            (len - 8.0).abs() < 1e-6,
            "length should track the redefined parameter, got {len}"
        );

        // Delete: the row is gone from the table.
        let _ = app.update(Message::PropParamDelete(index));
        assert!(!app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .contains("len"));
    }

    /// A Constraints-section row click selects every entity the constraint
    /// references, replacing whatever was selected before.
    #[test]
    fn properties_panel_constraint_link_click_selects_its_entities() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let a = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let b = add_line(&mut app, 0.0, 5.0, 10.0, 5.0);
        let unrelated = add_line(&mut app, 20.0, 20.0, 30.0, 20.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Parallel,
            refs: vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            driving_param: None,
            label: "Parallel constraint",
        });
        app.tabs[app.active_tab]
            .scene
            .select_entity(unrelated, true);

        let _ = app.update(Message::PropConstraintLinkClick(vec![a, b]));

        let selected: std::collections::HashSet<Handle> = app.tabs[app.active_tab]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        assert_eq!(selected, std::collections::HashSet::from([a, b]));
    }

    /// A row that fails to validate (here, a formula that doesn't parse)
    /// must not silently vanish or corrupt the table: it's reported as an
    /// error and simply isn't added, while a valid row alongside it still
    /// commits — the same isolate-the-failure treatment
    /// `ParameterTable::resolve_all` already gives a bad reference.
    #[test]
    fn applying_a_row_with_a_malformed_formula_reports_an_error_and_keeps_the_good_row() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        app.named_parameter_editor_rows = vec![
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "good".to_string(),
                formula: "10".to_string(),
            },
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "bad".to_string(),
                formula: "1 +".to_string(),
            },
        ];
        app.apply_named_parameter_editor_rows();

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("good"),
            Ok(10.0)
        );
        assert!(
            !app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .contains("bad"),
            "a malformed row must not be added"
        );
        assert!(
            app.command_line
                .history
                .iter()
                .any(|e| e.kind == crate::ui::command_line::EntryKind::Error),
            "a malformed row must be reported as an error, not silently dropped"
        );
    }

    /// Two rows typed with the same name must not silently collapse into
    /// "whichever `set` call happens last wins" — both are refused, and an
    /// unrelated row still commits normally.
    #[test]
    fn applying_rows_with_a_duplicate_name_refuses_both_and_keeps_the_unrelated_row() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        app.named_parameter_editor_rows = vec![
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "x".to_string(),
                formula: "1".to_string(),
            },
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "x".to_string(),
                formula: "2".to_string(),
            },
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "y".to_string(),
                formula: "3".to_string(),
            },
        ];
        app.apply_named_parameter_editor_rows();

        assert!(
            !app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .contains("x"),
            "a duplicated name must not be added at all"
        );
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("y"),
            Ok(3.0)
        );
        assert!(app
            .command_line
            .history
            .iter()
            .any(|e| e.kind == crate::ui::command_line::EntryKind::Error));
    }

    /// Two endpoint picks resolve to a Coincident constraint.
    #[test]
    fn coincident_command_resolves_two_picked_points_into_a_constraint() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let a = add_line(&mut app, 0.0, 0.0, 5.0, 0.0);
        let b = add_line(&mut app, 20.0, 0.0, 20.0, 5.0); // deliberately far from `a`, so the two picks are unambiguous

        let _ = app.apply_cmd_result(CmdResult::AddCoincidentConstraint {
            first: crate::command::CoincidentPick {
                handle: Some(a),
                point: glam::DVec3::new(5.0, 0.0, 0.0),
                whole_curve: false,
            },
            second: crate::command::CoincidentPick {
                handle: Some(b),
                point: glam::DVec3::new(20.0, 0.0, 0.0),
                whole_curve: false,
            },
            multiple: false,
            label: "Coincident constraint",
        });

        let set = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .expect("a constraint set should exist");
        assert_eq!(set.constraints.len(), 1);
        let c = &set.constraints[0];
        assert_eq!(c.kind, ConstraintKind::Coincident);
        let entities: Vec<Handle> = c.refs.iter().map(|r| r.entity).collect();
        assert!(
            entities.contains(&a) && entities.contains(&b),
            "the constraint should reference both lines"
        );
        let Some(acadrust::EntityType::Line(first)) =
            app.tabs[app.active_tab].scene.document.get_entity(a)
        else {
            panic!("expected first line");
        };
        let Some(acadrust::EntityType::Line(second)) =
            app.tabs[app.active_tab].scene.document.get_entity(b)
        else {
            panic!("expected second line");
        };
        assert!(
            (first.end - acadrust::types::Vector3::new(5.0, 0.0, 0.0)).length() < 1.0e-7
        );
        assert!((second.start - first.end).length() < 1.0e-7);
        assert!((second.length() - 5.0).abs() < 1.0e-7);
    }

    /// A pick that doesn't land near any real point (no object snap match)
    /// must not add a constraint — it should report an error and leave the
    /// scope untouched rather than silently constraining the wrong thing.
    #[test]
    fn coincident_command_reports_an_error_when_a_pick_misses() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _a = add_line(&mut app, 0.0, 0.0, 5.0, 0.0);

        let _ = app.apply_cmd_result(CmdResult::AddCoincidentConstraint {
            first: crate::command::CoincidentPick {
                handle: Some(_a),
                point: glam::DVec3::new(5.0, 0.0, 0.0),
                whole_curve: false,
            },
            second: crate::command::CoincidentPick {
                handle: None,
                point: glam::DVec3::new(500.0, 500.0, 0.0),
                whole_curve: false,
            },
            multiple: false,
            label: "Coincident constraint",
        });

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map(|s| s.constraints.len())
                .unwrap_or(0),
            0,
            "a missed pick must not add a constraint"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod delobj_tests {
    use super::*;
    use acadrust::types::Vector3;

    fn sweep_source_presence(value: i16, mode: crate::command::ExtrudeMode) -> (bool, bool) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut circle = acadrust::Circle::new();
        circle.radius = 2.0;
        let profile = app.tabs[i]
            .scene
            .add_entity(acadrust::EntityType::Circle(circle));
        let path =
            app.tabs[i]
                .scene
                .add_entity(acadrust::EntityType::Line(acadrust::Line::from_points(
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 5.0),
                )));
        app.delete_objects = value;

        let _ = app.apply_cmd_result(CmdResult::SweepEntities {
            handles: vec![profile],
            path_handle: path,
            mode,
            options: crate::command::SweepOptions::default(),
            color: [1.0; 4],
        });

        let created = app.tabs[i]
            .scene
            .document
            .entities()
            .any(|entity| match mode {
                crate::command::ExtrudeMode::Solid => {
                    matches!(entity, acadrust::EntityType::Solid3D(_))
                }
                crate::command::ExtrudeMode::Surface => {
                    matches!(entity, acadrust::EntityType::Surface(_))
                }
            });
        assert!(created, "SWEEP must produce the requested result");
        (
            app.tabs[i].scene.document.get_entity(profile).is_some(),
            app.tabs[i].scene.document.get_entity(path).is_some(),
        )
    }

    #[test]
    fn sweep_applies_all_four_source_deletion_modes() {
        let solid_expectations = [(true, true), (false, true), (false, false), (false, false)];
        for (value, expected) in solid_expectations.into_iter().enumerate() {
            assert_eq!(
                sweep_source_presence(value as i16, crate::command::ExtrudeMode::Solid),
                expected,
            );
        }
        assert_eq!(
            sweep_source_presence(2, crate::command::ExtrudeMode::Surface),
            (false, false),
        );
        assert_eq!(
            sweep_source_presence(3, crate::command::ExtrudeMode::Surface),
            (false, true),
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod thicken_tests {
    use super::*;
    use acadrust::entities::{Surface, SurfaceKind};
    use cadkernel::geom2d::{Circle, Curve, NurbsCurve};
    use cadkernel::space::{Parameterization, Plane};

    #[test]
    fn thicken_preserves_sources_and_round_trips_results_with_one_undo() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let body = cadkernel::brep::planar_region(
            Plane::XY,
            &[vec![Curve::Circle(Circle {
                centre: [0.0; 2],
                radius: 3.0,
            })]],
        )
        .unwrap();
        let source = app.add_surface_model(
            acadrust::EntityType::Surface(Surface::new(SurfaceKind::Plane)),
            body,
        );
        assert!(!source.is_null());
        let i = app.active_tab;
        let invalid = app.tabs[i]
            .scene
            .add_entity(acadrust::EntityType::Surface(Surface::new(
                SurfaceKind::Generic,
            )));
        let original =
            serde_json::to_value(app.tabs[i].scene.document.get_entity(source).unwrap()).unwrap();
        let before = app.tabs[i].history.undo_stack.len();
        let _ = app.apply_cmd_result(CmdResult::ThickenEntities {
            handles: vec![source],
            distance: 0.0,
        });
        assert_eq!(app.tabs[i].history.undo_stack.len(), before);
        let _ = app.apply_cmd_result(CmdResult::ThickenEntities {
            handles: vec![invalid, source],
            distance: 2.0,
        });
        assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
        let result = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| {
                matches!(entity, acadrust::EntityType::Solid3D(_)).then_some(entity.common().handle)
            })
            .unwrap();
        assert!(app.tabs[i]
            .scene
            .document
            .solid_history_operation(result)
            .is_some());
        assert_eq!(
            serde_json::to_value(app.tabs[i].scene.document.get_entity(source).unwrap()).unwrap(),
            original
        );
        let expected =
            cadkernel::brep::analytic_mass_properties(&app.tabs[i].scene.solid_models[&result])
                .unwrap()
                .volume;
        let bytes = crate::io::save_to_bytes(
            &app.tabs[i].scene.document,
            "dwg",
            app.tabs[i].scene.document.version,
        )
        .unwrap();
        let document = crate::io::load_bytes("thicken.dwg", bytes).unwrap();
        let mut restored = crate::scene::Scene::new();
        restored.document = document;
        restored.restore_solid_models(&[result]);
        let actual = cadkernel::brep::analytic_mass_properties(&restored.solid_models[&result])
            .unwrap()
            .volume;
        assert!((expected - actual).abs() < 1e-8);
        app.undo_active_tab();
        assert!(app.tabs[i].scene.document.get_entity(result).is_none());
        assert!(app.tabs[i]
            .scene
            .document
            .solid_history_operation(result)
            .is_none());
        assert!(app.tabs[i].scene.document.get_entity(source).is_some());
        assert!(app.tabs[i].scene.document.get_entity(invalid).is_some());
        app.redo_active_tab();
        assert!(app.tabs[i].scene.document.get_entity(result).is_some());
        assert!(app.tabs[i]
            .scene
            .document
            .solid_history_operation(result)
            .is_some());
        assert_eq!(
            serde_json::to_value(app.tabs[i].scene.document.get_entity(source).unwrap()).unwrap(),
            original
        );
    }

    #[test]
    fn thicken_creates_a_solid_from_a_free_form_surface() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let profile = NurbsCurve::interpolate(
            &[[0.0, 0.0], [0.7, 0.15], [1.3, -0.1], [2.0, 0.0]],
            None,
            None,
            Parameterization::Chord,
        )
        .unwrap();
        let body =
            cadkernel::brep::extrude_surface(Plane::XY, &[Curve::Nurbs(profile)], [0.0, 0.0, 2.0])
                .unwrap();
        let source = app.add_surface_model(
            acadrust::EntityType::Surface(Surface::new(SurfaceKind::Generic)),
            body,
        );
        let i = app.active_tab;

        let _ = app.apply_cmd_result(CmdResult::ThickenEntities {
            handles: vec![source],
            distance: 0.15,
        });

        let solids = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                matches!(entity, acadrust::EntityType::Solid3D(_)).then_some(entity.common().handle)
            })
            .collect::<Vec<_>>();
        assert_eq!(solids.len(), 1);
        let solid = &app.tabs[i].scene.solid_models[&solids[0]];
        assert!(solid.validate().is_empty());
        assert!(solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
        assert!(app.tabs[i]
            .scene
            .document
            .solid_history_operation(solids[0])
            .is_some());
        assert!(app.tabs[i].scene.document.get_entity(source).is_some());
    }
}

#[cfg(test)]
mod dispatched_command_task_tests {
    use super::*;
    use crate::command::CmdResult;
    use acadrust::entities::ViewportRenderMode as Mode;

    /// `CmdResult::Dispatch` and `CmdResult::Relaunch` run the assembled line
    /// through `dispatch_command`, and the Task it returns must reach the
    /// runtime: the render mode a VSCURRENT keyword picker chooses is applied
    /// by a message that Task carries.
    #[test]
    fn dispatch_and_relaunch_keep_the_task_the_command_returns() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        app.tabs[i].render_mode = Mode::Wireframe2D;
        let task = app.apply_cmd_result(CmdResult::Dispatch("VSCURRENT GOURAUDSHADED".into()));
        app.drive_headless_task(task).unwrap();
        assert_eq!(
            app.tabs[i].render_mode,
            Mode::GouraudShaded,
            "Dispatch dropped the dispatched command's task"
        );

        app.tabs[i].render_mode = Mode::Wireframe2D;
        let task = app.apply_cmd_result(CmdResult::Relaunch(
            "VSCURRENT GOURAUDSHADED".into(),
            Vec::new(),
        ));
        app.drive_headless_task(task).unwrap();
        assert_eq!(
            app.tabs[i].render_mode,
            Mode::GouraudShaded,
            "Relaunch dropped the dispatched command's task"
        );
    }
}
