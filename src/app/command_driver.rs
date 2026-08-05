use super::{Message, OpenCADStudio};
use crate::command::{CmdResult, SelectionEntity, StepInput};
use acadrust::Handle;
use iced::Task;

impl OpenCADStudio {
    fn refresh_area_preview(&mut self, i: usize) {
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
                    return Some(glam::DVec3::new(
                        line.end.x,
                        line.end.y,
                        line.end.z,
                    ));
                }
                acadrust::EntityType::Arc(_) => Some(2),
                acadrust::EntityType::LwPolyline(polyline) => {
                    polyline.vertices.len().checked_sub(1)
                }
                acadrust::EntityType::Polyline(polyline) => {
                    polyline.vertices.len().checked_sub(1)
                }
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
        self.snapper.from_point = None;
        self.snapper.clear_tracking();
        self.otrack_active = None;
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
        let _ = self.on_viewport_exit();
    }

    /// Roll a hot grip back to its pre-drag image and remove every grip-owned
    /// overlay. Shared by Escape and drawing-space transitions.
    pub(super) fn cancel_active_grip_edit(&mut self) -> bool {
        let i = self.active_tab;
        let had_grip = self.tabs[i].active_grip.take().is_some()
            || self.grip_add_provisional.is_some()
            || !self.grip_preview_handles.is_empty();
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
        let mut changed_handles: rustc_hash::FxHashSet<_> = handles.iter().copied().collect();
        for (handle, original) in originals {
            changed_handles.insert(handle);
            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                *entity = original;
            }
        }
        for &handle in &handles {
            self.tabs[i].scene.preview_hidden.remove(&handle);
        }
        let changes: Vec<_> = changed_handles
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        if let Some(dirty_before) = self.grip_dirty_before.take() {
            self.tabs[i].dirty = dirty_before;
        }

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
                self.command_line
                    .push_info(crate::t!("Command cancelled because the active drawing space changed.").as_ref());
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
            self.command_line
                .push_info(crate::t!("Command cancelled because the active drawing space changed.").as_ref());
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
            self.command_line.push_error(crate::t!("Outside limits.").as_ref());
            return false;
        }
        true
    }

    /// Drive the active command's step machine with one [`StepInput`], then
    /// apply the result. This is the single entry point every input source
    /// (command line, headless, dynamic input, plugin API, viewport) funnels
    /// through, so the routing from input → `on_*` method → `apply_cmd_result`
    /// lives in exactly one place. No-op when no command is active.
    pub(super) fn feed_command(&mut self, input: StepInput) -> Task<Message> {
        // Selection keywords (P / PREVIOUS, L / LAST) consume the token
        // before it reaches the command (#426).
        if let StepInput::Text(s) = &input {
            let kw_input = s.clone();
            if let Some(task) = self.try_selection_keyword(&kw_input) {
                return task;
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
                return Task::none();
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
        if let StepInput::SelectionComplete(handles) = &input {
            let entities = {
                let scene = &self.tabs[i].scene;
                handles
                    .iter()
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
        let ctrl = self.ctrl_down;
        let shift = self.shift_down;
        let result: Option<CmdResult> = {
            let Some(cmd) = self.tabs[i].active_cmd.as_mut() else {
                return Task::none();
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
        match result {
            Some(r) => self.apply_cmd_result(r),
            None => Task::none(),
        }
    }

    /// Run one whole command-line string. A single word or an inline-argument
    /// command (`PDMODE 3`, `LAYER Walls`, `UCS Z 90` pasted as one line)
    /// dispatches as-is; for a multi-token line whose first word starts an
    /// interactive tool (`LINE 0,0 10,10`) the first word starts the tool and the
    /// remaining tokens are fed as points / option keywords, then the command is
    /// terminated as if Enter were pressed. Shared by the GUI command line and
    /// the headless automation feeder so both behave identically.
    pub(super) fn run_command_line(&mut self, cmd: &str) -> Task<Message> {
        let i = self.active_tab;
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        if tokens.len() <= 1 {
            return self.dispatch_command(cmd);
        }
        // Plugin commands parse their own inline arguments from the whole line
        // (e.g. `HC_PIPE 2B 2C 1.25 0.013`), so offer the full command to plugin
        // dispatch first. A built-in interactive tool matches only its bare name
        // (`LINE`), so the full line is not a plugin command and falls through to
        // the first-word + fed-tokens path below. (#162)
        if crate::plugin::try_dispatch(self, i, cmd) {
            let toks: Vec<String> = tokens.iter().map(|s| s.to_string()).collect();
            self.finish_active_command(&toks);
            return Task::none();
        }
        let _ = self.dispatch_command(tokens[0]);
        if self.tabs[i].active_cmd.is_none() {
            // Not an interactive tool — an inline-argument command (`PDMODE 3`).
            return self.dispatch_command(cmd);
        }
        let toks: Vec<String> = tokens.iter().map(|s| s.to_string()).collect();
        self.finish_active_command(&toks);
        Task::none()
    }

    /// Feed `tokens[1..]` to the active interactive command as points / option
    /// keywords, then terminate it as if Enter were pressed. No-op when no
    /// command is active.
    pub(super) fn finish_active_command(&mut self, tokens: &[String]) {
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_none() {
            return;
        }
        self.last_point = None;
        for tok in &tokens[1..] {
            if self.tabs[i].active_cmd.is_none() {
                break;
            }
            self.feed_active_cmd(tok);
        }
        let _ = self.feed_command(StepInput::Enter);
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
        let add: Vec<Handle> = match kw.as_str() {
            "P" | "PREVIOUS" => self.tabs[i]
                .prev_selection
                .iter()
                .copied()
                .filter(|&h| {
                    self.tabs[i].scene.document.get_entity(h).is_some()
                        && !self.tabs[i].scene.is_layer_locked(h)
                })
                .collect(),
            // Highest handle among the selectable wires of the current space —
            // handles are handed out monotonically, so that is the most
            // recently created object.
            "L" | "LAST" => self.tabs[i]
                .scene
                .entity_wires()
                .iter()
                .filter_map(|w| crate::scene::Scene::handle_from_wire_name(&w.name))
                .filter(|&h| !self.tabs[i].scene.is_layer_locked(h))
                .max_by_key(|h| h.value())
                .into_iter()
                .collect(),
            _ => return None,
        };
        if add.is_empty() {
            self.command_line.push_info(match kw.as_str() {
                "P" | "PREVIOUS" => "No previous selection set.",
                _ => "No last object.",
            });
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

    pub(super) fn feed_active_cmd(&mut self, token: &str) {
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
                let _ = self.apply_cmd_result(r);
                return;
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
                let _ = self.feed_command(StepInput::EntityPick(handle, pt.as_dvec3()));
            }
            return;
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
                return;
            }
            self.last_point = Some(wcs);
            self.push_ucs_to_cmd(i);
            let _ = self.feed_command(StepInput::Point(wcs));
        } else {
            let _ = self.feed_command(StepInput::Text(token.to_string()));
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
    pub(super) fn apply_cmd_result(&mut self, result: CmdResult) -> Task<Message> {
        let was_active = self.tabs[self.active_tab].active_cmd.is_some();
        let preserve_selection =
            matches!(result, CmdResult::Relaunch(..) | CmdResult::Dispatch(..));
        let task = self.apply_cmd_result_inner(result);
        let i = self.active_tab;
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
            self.restore_add_selected_defaults();
        }
        task
    }

    fn apply_cmd_result_inner(&mut self, result: CmdResult) -> Task<Message> {
        let i = self.active_tab;
        match result {
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
                    self.open_attribute_editor(ins_handle);
                    return Task::none();
                }
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
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
                // A line/arc drawn by a repeating command advances the ARC_CONT
                // continuation anchor, so ending one run and launching another
                // keeps continuing from the last segment (mirrors the
                // CommitAndExit arm). Non-continuable repeats (point/ray)
                // leave the anchor untouched. (#327)
                if matches!(
                    entity,
                    acadrust::EntityType::Line(_) | acadrust::EntityType::Arc(_)
                ) {
                    self.update_cont_anchor(&entity);
                }
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                // Ordinary drawables, viewports and raster images use targeted
                // entity/object deltas. Block sentinels and novel layers retain
                // the structure snapshot fallback.
                let delta_safe = self.delta_add_safe(i, &entity);
                let pending = self.begin_undo(i, label, 1, delta_safe);
                self.commit_entity(entity);
                self.tabs[i].dirty = true;
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitEntities(entities) => {
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let delta_safe = entities
                    .iter()
                    .all(|entity| self.delta_add_safe(i, entity));
                let pending = self.begin_undo(i, label, entities.len(), delta_safe);
                for entity in entities {
                    self.commit_entity(entity);
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
                let handle = self.commit_entity_handle(
                    acadrust::EntityType::Viewport(viewport),
                );
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
                    let valid = scene
                        .entity_belongs_to_current_layout(boundary_handle)
                        && scene
                        .document
                        .get_entity(boundary_handle)
                        .is_some_and(|entity| {
                            match entity {
                                acadrust::EntityType::Circle(_) => true,
                                acadrust::EntityType::Ellipse(ellipse) => ellipse.is_full(),
                                acadrust::EntityType::LwPolyline(polyline) => {
                                    polyline.is_closed
                                }
                                acadrust::EntityType::Polyline(polyline) => {
                                    polyline.is_closed()
                                }
                                acadrust::EntityType::Polyline2D(polyline) => {
                                    polyline.is_closed()
                                }
                                acadrust::EntityType::Polyline3D(polyline) => {
                                    polyline.flags.closed
                                }
                                _ => false,
                            }
                        });
                    if !valid {
                        self.command_line.push_error(
                            "MVIEW Object: select a closed paper-space circle, ellipse, or polyline.",
                        );
                        if let Some(prompt) =
                            self.tabs[i].active_cmd.as_ref().map(|command| command.prompt())
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
                let polygon = self.tabs[i]
                    .scene
                    .clip_boundary_polygon(clip_handle, 0.0);
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
                    self.command_line
                        .push_error(crate::t!("MVIEW: the clipping boundary has no usable area.").as_ref());
                    self.tabs[i].active_cmd = None;
                    if let Some(pending) = pending {
                        self.commit_undo_delta(i, pending);
                    }
                    return Task::none();
                };
                if max_x - min_x < 1e-6 || max_y - min_y < 1e-6 {
                    self.command_line
                        .push_error(crate::t!("MVIEW: the clipping boundary has no usable area.").as_ref());
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
                let viewport_handle = self.commit_entity_handle(
                    acadrust::EntityType::Viewport(viewport),
                );
                if let Some(viewport_handle) = viewport_handle {
                    if !created_boundary {
                        let before = self.tabs[i]
                            .scene
                            .document
                            .get_entity(clip_handle)
                            .cloned()
                            .map(std::sync::Arc::new);
                        self.tabs[i]
                            .scene
                            .record_undo_before(clip_handle, before);
                    }
                    if let Some(boundary) =
                        self.tabs[i].scene.document.get_entity_mut(clip_handle)
                    {
                        let common = boundary.common_mut();
                        common.invisible = true;
                        if !common.reactors.contains(&viewport_handle) {
                            common.reactors.push(viewport_handle);
                        }
                    }
                    self.tabs[i].scene.bump_entities(&[(
                        clip_handle,
                        crate::scene::ChangeKind::Modified,
                    )]);
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
            CmdResult::WipeoutFromPolyline(handle) => {
                let wipeout = {
                    let scene = &self.tabs[i].scene;
                    scene
                        .entity_belongs_to_active_space(handle)
                        .then(|| scene.document.get_entity(handle))
                        .flatten()
                        .and_then(
                            crate::modules::draw::draw::wipeout::wipeout_from_polyline,
                        )
                };
                if let Some(wipeout) = wipeout {
                    return self.apply_cmd_result(CmdResult::CommitAndExit(wipeout));
                }
                self.command_line.push_error(
                    "WIPEOUT Polyline: select a closed planar polyline with at least 3 vertices.",
                );
                if let Some(prompt) =
                    self.tabs[i].active_cmd.as_ref().map(|command| command.prompt())
                {
                    self.command_line.push_info(&prompt);
                }
            }
            CmdResult::MviewSwitchLayout(layout) => {
                let task = self.on_layout_switch_preserving_command(layout);
                if let Some(prompt) =
                    self.tabs[i].active_cmd.as_ref().map(|command| command.prompt())
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
            CmdResult::TransformSelected(handles, transform) => {
                let label = self.history_label_from_active_cmd(i, "MOVE");
                // A move/rotate/scale/mirror mutates only the selected entities
                // (and their baked dimension sub-entities) through
                // transform_entities — always delta-safe.
                let pending = self.begin_undo(i, label, handles.len(), true);
                self.tabs[i].scene.transform_entities(&handles, &transform);
                // ACIS solids render from a cached mesh, so a move/rotate/
                // scale/mirror needs the mesh re-tessellated from the now-moved
                // body — wire re-tessellation alone leaves the solid drawn at
                // its old spot. (#135)
                if self.tabs[i].scene.any_solid(&handles) {
                    self.tabs[i].scene.refresh_meshes_for_handles(&handles);
                }
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
            CmdResult::CopySelected(handles, transform) => {
                let label = self.history_label_from_active_cmd(i, "COPY");
                // Copying a dimension clones a *D block record, so gate delta on
                // the selection being dimension-free.
                let delta_safe = self.delta_copy_safe(i, &handles);
                let pending = self.begin_undo(i, label, handles.len(), delta_safe);
                let new_handles = self.tabs[i].scene.copy_entities(&handles, &transform);
                if self.tabs[i].scene.any_solid(&new_handles) {
                    self.tabs[i].scene.refresh_meshes_for_handles(&new_handles);
                }
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
            CmdResult::CommitAndExit(entity) => {
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
                    crate::modules::insert::xattach::prepare_xref_block(
                        &mut self.tabs[i].scene,
                        &path,
                    );
                    // Resolving the xref merged its layer / linetype tables
                    // into the document — mirror them into the Layers panel
                    // and ribbon dropdowns now, not on the next reopen (#407).
                    self.refresh_layer_panel();
                }
                // Record where this draw ended so ARC_CONT can continue from it
                // (before `entity` is moved into commit_entity).
                self.update_cont_anchor(&entity);
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let delta_safe = self.delta_add_safe(i, &entity);
                let pending = self.begin_undo(i, label, 1, delta_safe);
                self.commit_entity(entity);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitSolid { entity, solid } => {
                let label = self.history_label_from_active_cmd(i, "SOLID");
                let pending = self.begin_undo(i, label, 1, true);
                self.add_solid_model(entity, *solid);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
            CmdResult::CommitAndEditText(entity) => {
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let delta_safe = self.delta_add_safe(i, &entity);
                let pending = self.begin_undo(i, label, 1, delta_safe);
                let handle = self.commit_entity_handle(entity);
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
            } => {
                let label = self.history_label_from_active_cmd(i, "ENTITY");
                let delta_safe = entities
                    .iter()
                    .all(|entity| self.delta_add_safe(i, entity));
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
                    if let Some(acadrust::EntityType::Leader(l)) =
                        self.tabs[i].scene.document.get_entity_mut(lh)
                    {
                        l.annotation_handle = ah;
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
                if let Some(h) = edit_handle {
                    return self.begin_text_edit(h);
                }
            }
            CmdResult::CreateBlock {
                handles,
                name,
                base,
            } => {
                self.push_undo_snapshot(i, "BLOCK");
                let ucs = self.tabs[i].ucs_xform();
                let world_to_block = ucs.to_ucs_transform_at(base);
                let block_to_world = ucs.to_wcs_transform_at(base);
                match self.tabs[i]
                    .scene
                    .create_block_from_entities(
                        &handles,
                        &name,
                        &world_to_block,
                        &block_to_world,
                    )
                {
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
            CmdResult::CommitHatch(hatch) => {
                let label = self.history_label_from_active_cmd(i, "HATCH");
                let pending = self.begin_undo(i, label, 1, true);
                let new_handle = self.tabs[i].scene.add_hatch(hatch);
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
            CmdResult::BatchCopy(handles, transforms) => {
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
                let label = self.history_label_from_active_cmd(i, "FILLET");
                let was_catchment = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|c| c.name() == "SS_CATCHMENT");
                self.push_undo_snapshot(i, label);
                for (handle, entities) in replacements {
                    self.tabs[i].scene.erase_entities(&[handle]);
                    for entity in entities {
                        let nh = self.tabs[i].scene.add_entity(entity);
                        // Drop a stale *D block on any replaced dimension (#181).
                        if matches!(
                            self.tabs[i].scene.document.get_entity(nh),
                            Some(acadrust::EntityType::Dimension(_))
                        ) {
                            self.tabs[i].scene.invalidate_dim_block_recorded(nh);
                        }
                    }
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
                let label = self.history_label_from_active_cmd(i, "TRIM");
                self.push_undo_snapshot(i, label);
                for (handle, entities) in replacements {
                    self.tabs[i].scene.erase_entities(&[handle]);
                    let new_handles: Vec<Handle> = entities
                        .into_iter()
                        .map(|entity| self.tabs[i].scene.add_entity(entity))
                        .collect();
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
            CmdResult::ReplaceEntity(handle, new_entities) => {
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
                // Detect DIMBREAK sentinel.
                if new_entities.len() == 1 {
                    if let acadrust::EntityType::XLine(ref xl) = new_entities[0] {
                        let layer = xl.common.layer.clone();
                        if layer.starts_with("__DIMBREAK__")
                            || layer.starts_with("__DIMBREAK_AUTO__")
                        {
                            // DIMBREAK needs a break-gap field on the dimension
                            // model (not yet present) to store and render the gap.
                            // Report honestly rather than claiming success while
                            // changing nothing. (#181 / DIM-020)
                            self.command_line
                                .push_info(crate::t!("DIMBREAK: not yet implemented — nothing changed.").as_ref());
                            self.tabs[i].active_cmd = None;
                            self.tabs[i].snap_result = None;
                            return Task::none();
                        }
                        if layer.starts_with("__DIMSPACE__") {
                            if let Some(encoded) = layer.strip_prefix("__DIMSPACE__") {
                                apply_dimspace(&mut self.tabs[i].scene, encoded);
                            }
                            self.push_undo_snapshot(i, "DIMSPACE");
                            self.command_line.push_output(crate::t!("DIMSPACE  Spacing adjusted.").as_ref());
                            self.tabs[i].dirty = true;
                            self.tabs[i].active_cmd = None;
                            self.tabs[i].snap_result = None;
                            return Task::none();
                        }
                        if layer.starts_with("__DIMJOG__") {
                            // DIMJOGLINE needs a jog-point field on the dimension
                            // model (not yet present) to store and render the jog.
                            // Report honestly rather than faking success. (DIM-019)
                            self.command_line
                                .push_info(crate::t!("DIMJOGLINE: not yet implemented — nothing changed.").as_ref());
                            self.tabs[i].active_cmd = None;
                            self.tabs[i].snap_result = None;
                            return Task::none();
                        }
                        if layer.starts_with("__MLEADERALIGN__") {
                            if let Some(encoded) = layer.strip_prefix("__MLEADERALIGN__") {
                                apply_mleader_align(&mut self.tabs[i].scene, encoded);
                            }
                            self.push_undo_snapshot(i, "MLEADERALIGN");
                            self.command_line
                                .push_output(crate::t!("MLEADERALIGN  Leaders aligned.").as_ref());
                            self.tabs[i].dirty = true;
                            self.tabs[i].active_cmd = None;
                            self.tabs[i].snap_result = None;
                            return Task::none();
                        }
                        if layer.starts_with("__MLEADERCOLLECT__") {
                            if let Some(encoded) = layer.strip_prefix("__MLEADERCOLLECT__") {
                                apply_mleader_collect(&mut self.tabs[i].scene, encoded);
                            }
                            self.push_undo_snapshot(i, "MLEADERCOLLECT");
                            self.command_line
                                .push_output(crate::t!("MLEADERCOLLECT  Leaders collected.").as_ref());
                            self.tabs[i].dirty = true;
                            self.tabs[i].active_cmd = None;
                            self.tabs[i].snap_result = None;
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
                // A replaced dimension carries edited geometry/text but still
                // names its old *D block; drop that stale block so the next save
                // re-bakes it — otherwise BricsCAD/ODA draw the pre-edit
                // graphics while OCS shows the edit. (#181)
                for &nh in &new_handles {
                    if matches!(
                        self.tabs[i].scene.document.get_entity(nh),
                        Some(acadrust::EntityType::Dimension(_))
                    ) {
                        self.tabs[i].scene.invalidate_dim_block_recorded(nh);
                    }
                }
                if let Some(cmd) = &mut self.tabs[i].active_cmd {
                    cmd.on_entity_replaced(handle, &new_handles);
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
                        let label = self.history_label_from_active_cmd(i, "INSERT");
                        let delta_safe = self.delta_add_safe(i, &entity);
                        let pending = self.begin_undo(i, label, 1, delta_safe);
                        self.commit_entity(entity);
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.clear_preview_wire();
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        self.restore_pre_cmd_tangent();
                        if let Some(pd) = pending {
                            self.commit_undo_delta(i, pd);
                        }
                    }
                } else {
                    // Inject attdefs so the command enters attr-filling mode.
                    if let Some(cmd) = &mut self.tabs[i].active_cmd {
                        cmd.attreq_set_attdefs(attdefs);
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
                    let old_handle = old.as_entity().handle();
                    let layer = old.as_entity().layer().to_string();
                    let mut new = entity;
                    new.as_entity_mut().set_handle(old_handle);
                    new.as_entity_mut().set_layer(layer);
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
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.command_line.push_info(if space_changed {
                    "Command cancelled because the active drawing space changed."
                } else {
                    "Command cancelled."
                });
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
                let _ = self.dispatch_command(&cmd);
            }
            CmdResult::Dispatch(cmd) => {
                // End this interactive front-end, then run the assembled command
                // through the normal dispatcher. Selection is left untouched.
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                let _ = self.dispatch_command(&cmd);
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
                if let Some(layer) = src_layer {
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
                    self.command_line.push_error(crate::t!("Source object not found.").as_ref());
                }
            }
            CmdResult::MatchProperties { dest, src } => {
                // The command stays active after each apply so more targets
                // can keep being picked; Enter / Esc ends it (#362).
                // Special (type-specific) properties travel like AutoCAD's
                // Special Properties: each is captured from the source when it
                // carries it and applied only to destinations that support it
                // (#281). Text formatting crosses TEXT ↔ MTEXT (#361); the dim
                // style crosses Dimension / Leader / Tolerance.
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
                    self.push_undo_snapshot(i, "MATCHPROP");
                    for h in &dest {
                        let mut is_dim = false;
                        let mut is_hatch = false;
                        if let Some(e) = self.tabs[i].scene.document.get_entity_mut(*h) {
                            e.as_entity_mut().set_layer(common.layer.clone());
                            crate::scene::view::dispatch::apply_color(e, common.color);
                            crate::scene::view::dispatch::apply_line_weight(e, common.line_weight);
                            {
                                let dst_common = e.common_mut();
                                dst_common.linetype = common.linetype.clone();
                                dst_common.linetype_handle = common.linetype_handle;
                                dst_common.linetype_scale = common.linetype_scale;
                                dst_common.transparency = common.transparency.clone();
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
                                    &mut self.tabs[i].scene.document,
                                    *h,
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
                                self.tabs[i].scene.document.get_entity(*h),
                                Some(
                                    acadrust::EntityType::Dimension(_)
                                        | acadrust::EntityType::Leader(_)
                                )
                            )
                        {
                            if matches!(
                                self.tabs[i].scene.document.get_entity(*h),
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
                                    &mut self.tabs[i].scene.document,
                                    *h,
                                    dstyle_xdata.clone().unwrap_or_default(),
                                );
                            }
                        }
                        // A restyled dimension renders from its baked *D block —
                        // drop the stale block so the new style shows (#398).
                        if is_dim {
                            self.tabs[i].scene.invalidate_dim_block_recorded(*h);
                        }
                        // Hatch fills render from a prebuilt model (#415).
                        self.tabs[i].scene.refresh_fill_model(*h);
                    }
                    self.tabs[i].dirty = true;
                    // Color / linetype / lineweight are baked into the cached
                    // wires at tessellation time; re-tessellate only the matched
                    // objects instead of rebuilding a large drawing.
                    let changes: Vec<_> = dest
                        .iter()
                        .copied()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.refresh_properties();
                    self.command_line
                        .push_info(crate::tf!("Properties matched to {} object(s).", dest.len()).as_ref());
                    // Clear the consumed target selection and keep prompting.
                    self.tabs[i].scene.deselect_all();
                    if let Some(cmd) = &self.tabs[i].active_cmd {
                        self.command_line.push_info(&cmd.prompt());
                    }
                } else {
                    self.command_line.push_error(crate::t!("Source object not found.").as_ref());
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
                    self.command_line.push_error(crate::t!("Clipboard is empty.").as_ref());
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
            CmdResult::CreateGroup { handles, name } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                let undo = self.begin_group_undo(i, "GROUP");
                self.tabs[i].scene.create_group(name.clone(), handles);
                self.tabs[i].dirty = true;
                self.commit_group_undo(i, undo);
                self.command_line
                    .push_info(crate::tf!("Group \"{}\" created.", name).as_ref());
            }
            CmdResult::DeleteGroups { handles } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
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
                        self.command_line.push_info(crate::tf!(
                            "VPLAYER: {frozen_count} layer(s) frozen in viewport."
                        ).as_ref());
                    }
                    if thawed_count > 0 {
                        self.command_line.push_info(crate::tf!(
                            "VPLAYER: {thawed_count} layer(s) thawed in viewport."
                        ).as_ref());
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
                self.tabs[i]
                    .scene
                    .zoom_to_window(p1.as_vec3(), p2.as_vec3());
                self.command_line.push_output(crate::t!("Zoom Window").as_ref());
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
                handles,
                src1,
                dst1,
                angle_rad,
                scale,
            } => {
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
                    self.command_line.push_output(crate::t!("ALIGN: applied.").as_ref());
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
                        self.command_line.push_output(crate::t!("LENGTHEN: applied.").as_ref());
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
            CmdResult::DivideEntity { handle, n } => {
                use crate::modules::draw::inquiry::divide::divide_entity;
                let pts = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .map(|e| divide_entity(e, n))
                    .unwrap_or_default();
                let count = pts.len();
                if count > 0 {
                    self.push_undo_snapshot(i, "DIVIDE");
                    for p in pts {
                        self.tabs[i].scene.add_entity(p);
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("DIVIDE: {count} point(s) placed.").as_ref());
                } else {
                    self.command_line
                        .push_error(crate::t!("DIVIDE: entity type not supported or N < 2.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::MeasureEntity {
                handle,
                segment_length,
            } => {
                use crate::modules::draw::inquiry::divide::measure_entity;
                let pts = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .map(|e| measure_entity(e, segment_length))
                    .unwrap_or_default();
                let count = pts.len();
                if count > 0 {
                    self.push_undo_snapshot(i, "MEASURE");
                    for p in pts {
                        self.tabs[i].scene.add_entity(p);
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("MEASURE: {count} point(s) placed.").as_ref());
                } else {
                    self.command_line
                        .push_error(crate::t!("MEASURE: entity type not supported or distance too large.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }
            CmdResult::PeditOp { handle, op } => {
                use crate::modules::draw::modify::pedit::{
                    apply_pedit, convert_to_polyline, PeditOp,
                };
                match &op {
                    // The convert replaces the entity (new handle).
                    PeditOp::ConvertToPolyline => {
                        let converted = self.tabs[i]
                            .scene
                            .document
                            .get_entity(handle)
                            .and_then(convert_to_polyline);
                        match converted {
                            Some(pl) => {
                                return self
                                    .apply_cmd_result(CmdResult::ReplaceEntity(handle, vec![pl]));
                            }
                            None => self
                                .command_line
                                .push_error(crate::t!("PEDIT: cannot convert this entity.").as_ref()),
                        }
                    }
                    _ => {
                        // Snapshot BEFORE the mutation — an after-the-fact
                        // snapshot records the changed state and undo no-ops.
                        self.push_undo_snapshot(i, "PEDIT");
                        let changed = self.tabs[i]
                            .scene
                            .document
                            .get_entity_mut(handle)
                            .map(|e| apply_pedit(e, &op))
                            .unwrap_or(false);
                        if changed {
                            self.tabs[i].dirty = true;
                            // Repaint immediately (wide-band fill included).
                            self.tabs[i].scene.refresh_fill_model(handle);
                            self.tabs[i]
                                .scene
                                .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                            self.command_line.push_output(crate::t!("PEDIT: applied.").as_ref());
                            self.refresh_properties();
                        } else {
                            self.discard_last_undo_entry(i);
                            self.command_line
                                .push_error(crate::t!("PEDIT: operation not applicable to this entity.").as_ref());
                        }
                    }
                }
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }
            CmdResult::JoinEntities(handles) => {
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
                        self.command_line.push_output(crate::tf!(
                            "JOIN: {count_in} object(s) joined into {count_out}."
                        ).as_ref());
                        self.refresh_properties();
                    }
                    None => {
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].snap_result = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.restore_pre_cmd_tangent();
                        self.command_line.push_error(
                            "JOIN: objects don't form a single connected chain, or contain an unsupported type / tilted arc.",
                        );
                    }
                }
            }
            CmdResult::BreakEntity { handle, p1, p2 } => {
                use crate::modules::draw::modify::break_cmd::break_entity;
                let replacement = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .and_then(|e| break_entity(e, p1, p2));
                match replacement {
                    Some(frags) => {
                        let label = self.history_label_from_active_cmd(i, "BREAK");
                        self.push_undo_snapshot(i, label);
                        self.tabs[i].scene.erase_entities(&[handle]);
                        let count = frags.len();
                        for e in frags {
                            self.tabs[i].scene.add_entity(e);
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
                    self.command_line
                        .push_output(crate::tf!("Plot window: {x0:.2},{y0:.2} to {x1:.2},{y1:.2}").as_ref());
                    // Pick window closed the plot dialog so the viewport could
                    // receive the two clicks — bring the dialog back with the
                    // window now active.
                    self.plot_dialog.area = "Window".to_string();
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
                    self.command_line.push_output(crate::tf!(
                        "Plot window: {x1:.2},{y1:.2} to {x2:.2},{y2:.2}"
                    ).as_ref());
                    self.plot_dialog.area = "Window".to_string();
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
            CmdResult::StretchWindow { win_min, win_max } => {
                // Implicit STRETCH selection (#338): the crossing window drawn
                // with no prior selection picks the objects itself. Entities
                // whose world AABB touches the window are handed back to the
                // command at the base-point step — over-selection is harmless,
                // since only points INSIDE the window move anyway.
                let mut handles: Vec<Handle> = Vec::new();
                {
                    let scene = &self.tabs[i].scene;
                    handles.extend(
                        scene
                            .interaction_handles_in_world_aabb([
                                win_min.x, win_min.y, win_max.x, win_max.y,
                            ])
                            .into_iter()
                            .filter(|&h| !scene.is_layer_locked(h)),
                    );
                }
                if handles.is_empty() {
                    self.command_line
                        .push_output(crate::t!("STRETCH: nothing crosses the window.").as_ref());
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.tabs[i].scene.clear_preview_wire();
                    self.restore_pre_cmd_tangent();
                } else {
                    use crate::command::CadCommand;
                    use crate::modules::draw::modify::stretch::StretchCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let cmd = StretchCommand::with_window(handles, wires, win_min, win_max);
                    self.command_line.push_info(&CadCommand::prompt(&cmd));
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }
            CmdResult::StretchEntities {
                handles,
                win_min,
                win_max,
                delta,
            } => {
                let structural = handles.iter().any(|handle| {
                    matches!(
                        self.tabs[i].scene.document.get_entity(*handle),
                        Some(acadrust::EntityType::Dimension(_))
                    )
                });
                let pending = self.begin_undo(i, "STRETCH", handles.len(), !structural);
                let mut count = 0usize;
                let mut changed_handles = Vec::new();

                // Helper: is DXF point (x, y) inside the world-space window?
                // Drawing plane is world XY (= DXF XY).
                let in_win = |x: f64, y: f64| -> bool {
                    x >= win_min.x && x <= win_max.x && y >= win_min.y && y <= win_max.y
                };

                let dx = delta.x as f64;
                let dy = delta.y as f64; // drawing plane is world XY
                let dz = delta.z as f64;

                // Dimensions whose points moved — their baked *D block is
                // stale afterwards and must be dropped (see #398 / #372).
                let mut stretched_dims: Vec<acadrust::Handle> = Vec::new();
                for handle in &handles {
                    let before = self.tabs[i].scene.document.get_entity_arc(*handle);
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
                            for v in &mut p.vertices {
                                if in_win(v.location.x, v.location.y) {
                                    v.location.x += dx;
                                    v.location.y += dy;
                                    stretched = true;
                                }
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
                            stretched =
                                crate::entities::viewport::stretch(vp, win_min, win_max, delta);
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
                                    stretched |= mv(&mut d.definition_point);
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
                    self.tabs[i].scene.bump_entities(&changes);
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
            // ── Solid3D creation (BOX / SPHERE / CYLINDER) ────────────────
            CmdResult::CommitSolid3D { mesh_fn } => {
                use crate::modules::insert::solid3d_cmds::empty_solid3d;
                let pending = self.begin_undo(i, "SOLID3D", 1, true);
                let entity = empty_solid3d();
                let handle = self.tabs[i].scene.add_entity(entity);
                if !handle.is_null() {
                    let name = format!("{}", handle.value());
                    let color = [0.6f32, 0.6, 0.8, 1.0]; // default colour; command embedded it
                    let _ = color; // color is captured inside mesh_fn
                    if let Some(mesh) = mesh_fn(name) {
                        self.tabs[i]
                            .scene
                            .meshes
                            .insert(handle, crate::scene::MeshLodSet::from_single(mesh));
                    }
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(crate::t!("Solid created.").as_ref());
                }
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }

            // ── EXTRUDE ────────────────────────────────────────────────────
            CmdResult::ExtrudeEntity {
                handle,
                height,
                color,
            } => {
                use crate::entities::traits::EntityTypeOps;
                use crate::modules::insert::solid3d_cmds::empty_solid3d;
                use crate::scene::convert::acad_to_truck::TruckObject;
                use crate::scene::convert::truck_tess;
                use truck_modeling::builder;
                use truck_modeling::Vector3 as TruckVec3;

                let entity_opt = self.tabs[i].scene.document.get_entity(handle).cloned();
                if let Some(entity) = entity_opt {
                    let truck_entity = entity.to_truck_entity(&self.tabs[i].scene.document);
                    let result = truck_entity.and_then(|te| {
                        match te.object {
                            TruckObject::Contour(wire) => {
                                // Attach a planar face to the wire profile, then sweep.
                                let face = builder::try_attach_plane(&[wire]).ok()?;
                                // tsweep(Face) → Solid
                                let solid =
                                    builder::tsweep(&face, TruckVec3::new(0.0, 0.0, height as f64));
                                match truck_tess::tessellate_solid(&solid) {
                                    truck_tess::TruckTessResult::Mesh {
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                    } => Some((
                                        crate::scene::model::mesh_model::MeshModel {
                                            name: String::new(),
                                            verts,
                                            verts_low,
                                            normals,
                                            indices,
                                            triangle_material_handles: Vec::new(),
                                            triangle_colors: Vec::new(),
                                            color,
                                            selected: false,
                                        },
                                        solid,
                                    )),
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    });
                    if let Some((mut mesh, solid)) = result {
                        let pending = self.begin_undo(i, "EXTRUDE", 1, true);
                        let new_entity = empty_solid3d();
                        let new_handle = self.tabs[i].scene.add_entity(new_entity);
                        mesh.name = format!("{}", new_handle.value());
                        self.tabs[i]
                            .scene
                            .meshes
                            .insert(new_handle, crate::scene::MeshLodSet::from_single(mesh));
                        // Keep the truck B-rep so the save path can export exact
                        // ACIS geometry (else the solid is dropped by other CAD apps).
                        self.tabs[i].scene.solid_models.insert(new_handle, solid);
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::t!("EXTRUDE: solid created.").as_ref());
                        if let Some(pd) = pending {
                            self.commit_undo_delta(i, pd);
                        }
                    } else {
                        self.command_line.push_error(crate::t!("EXTRUDE: could not build profile. Select a closed 2D entity (Circle, LwPolyline, etc.).").as_ref());
                    }
                } else {
                    self.command_line.push_error(crate::t!("EXTRUDE: entity not found.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }

            // ── REVOLVE ────────────────────────────────────────────────────
            CmdResult::RevolveEntity {
                handle,
                axis_start,
                axis_end,
                angle_deg,
                color,
            } => {
                use crate::entities::traits::EntityTypeOps;
                use crate::modules::insert::solid3d_cmds::empty_solid3d;
                use crate::scene::convert::acad_to_truck::TruckObject;
                use crate::scene::convert::truck_tess;
                use truck_modeling::builder;
                use truck_modeling::{Point3, Rad, Vector3 as TruckVec3};

                let entity_opt = self.tabs[i].scene.document.get_entity(handle).cloned();
                if let Some(entity) = entity_opt {
                    let truck_entity = entity.to_truck_entity(&self.tabs[i].scene.document);
                    let result = truck_entity.and_then(|te| {
                        let wire: Option<truck_modeling::Wire> = match te.object {
                            TruckObject::Contour(w) => Some(w),
                            TruckObject::Curve(e) => Some(std::iter::once(e).collect()),
                            _ => None,
                        };
                        let wire = wire?;
                        let origin = Point3::new(
                            axis_start.x as f64,
                            axis_start.y as f64,
                            axis_start.z as f64,
                        );
                        let dir = (axis_end - axis_start).normalize();
                        let axis = TruckVec3::new(dir.x as f64, dir.y as f64, dir.z as f64);
                        let shell = builder::rsweep(
                            &wire,
                            origin,
                            axis,
                            Rad(angle_deg.to_radians() as f64),
                        );
                        match truck_tess::tessellate_shell(&shell) {
                            truck_tess::TruckTessResult::Mesh {
                                verts,
                                verts_low,
                                normals,
                                indices,
                            } => Some(crate::scene::model::mesh_model::MeshModel {
                                name: String::new(),
                                verts,
                                verts_low,
                                normals,
                                indices,
                                triangle_material_handles: Vec::new(),
                                triangle_colors: Vec::new(),
                                color,
                                selected: false,
                            }),
                            _ => None,
                        }
                    });
                    if let Some(mut mesh) = result {
                        let pending = self.begin_undo(i, "REVOLVE", 1, true);
                        let new_entity = empty_solid3d();
                        let new_handle = self.tabs[i].scene.add_entity(new_entity);
                        mesh.name = format!("{}", new_handle.value());
                        self.tabs[i]
                            .scene
                            .meshes
                            .insert(new_handle, crate::scene::MeshLodSet::from_single(mesh));
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("REVOLVE: solid created ({:.0}°).", angle_deg).as_ref());
                        if let Some(pd) = pending {
                            self.commit_undo_delta(i, pd);
                        }
                    } else {
                        self.command_line
                            .push_error(crate::t!("REVOLVE: could not revolve profile.").as_ref());
                    }
                } else {
                    self.command_line.push_error(crate::t!("REVOLVE: entity not found.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }

            // ── SWEEP ──────────────────────────────────────────────────────
            CmdResult::SweepEntity {
                profile_handle,
                path_handle,
                color,
            } => {
                use crate::entities::traits::EntityTypeOps;
                use crate::modules::insert::solid3d_cmds::empty_solid3d;
                use crate::scene::convert::acad_to_truck::TruckObject;
                use crate::scene::convert::truck_tess;
                use truck_modeling::builder;
                use truck_modeling::Vector3 as TruckVec3;

                let profile_ent = self.tabs[i]
                    .scene
                    .document
                    .get_entity(profile_handle)
                    .cloned();
                let path_ent = self.tabs[i].scene.document.get_entity(path_handle).cloned();

                let result = profile_ent.zip(path_ent).and_then(|(prof_e, path_e)| {
                    let prof_truck = prof_e.to_truck_entity(&self.tabs[i].scene.document)?;
                    let path_truck = path_e.to_truck_entity(&self.tabs[i].scene.document)?;

                    // Profile must be a wire (closed or open).
                    let profile_wire: truck_modeling::Wire = match prof_truck.object {
                        TruckObject::Contour(w) => w,
                        TruckObject::Curve(e) => std::iter::once(e).collect(),
                        _ => return None,
                    };

                    // Path determines the sweep operation.
                    let mesh = match path_truck.object {
                        // Linear path: translate profile along the line direction.
                        TruckObject::Curve(edge) => {
                            let p_start = edge.front().point();
                            let p_end = edge.back().point();
                            let dir = TruckVec3::new(
                                p_end.x - p_start.x,
                                p_end.y - p_start.y,
                                p_end.z - p_start.z,
                            );
                            // Try to build a face from the profile; if it's a closed
                            // wire we get a Solid, otherwise a Shell.
                            if let Ok(face) = builder::try_attach_plane(&[profile_wire.clone()]) {
                                let solid = builder::tsweep(&face, dir);
                                match truck_tess::tessellate_solid(&solid) {
                                    truck_tess::TruckTessResult::Mesh {
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                    } => Some(crate::scene::model::mesh_model::MeshModel {
                                        name: String::new(),
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                        triangle_material_handles: Vec::new(),
                                        triangle_colors: Vec::new(),
                                        color,
                                        selected: false,
                                    }),
                                    _ => None,
                                }
                            } else {
                                let shell = builder::tsweep(&profile_wire, dir);
                                match truck_tess::tessellate_shell(&shell) {
                                    truck_tess::TruckTessResult::Mesh {
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                    } => Some(crate::scene::model::mesh_model::MeshModel {
                                        name: String::new(),
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                        triangle_material_handles: Vec::new(),
                                        triangle_colors: Vec::new(),
                                        color,
                                        selected: false,
                                    }),
                                    _ => None,
                                }
                            }
                        }

                        // Contour path (polyline): sweep along the polyline using the
                        // first edge's direction as approximation (multi-segment sweep
                        // requires NURBS deformation — not supported here).
                        TruckObject::Contour(path_wire) => {
                            // Use start→end of the whole wire as translation vector.
                            let p_start = path_wire.front_vertex()?.point();
                            let p_end = path_wire.back_vertex()?.point();
                            let dir = TruckVec3::new(
                                p_end.x - p_start.x,
                                p_end.y - p_start.y,
                                p_end.z - p_start.z,
                            );
                            if let Ok(face) = builder::try_attach_plane(&[profile_wire.clone()]) {
                                let solid = builder::tsweep(&face, dir);
                                match truck_tess::tessellate_solid(&solid) {
                                    truck_tess::TruckTessResult::Mesh {
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                    } => Some(crate::scene::model::mesh_model::MeshModel {
                                        name: String::new(),
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                        triangle_material_handles: Vec::new(),
                                        triangle_colors: Vec::new(),
                                        color,
                                        selected: false,
                                    }),
                                    _ => None,
                                }
                            } else {
                                let shell = builder::tsweep(&profile_wire, dir);
                                match truck_tess::tessellate_shell(&shell) {
                                    truck_tess::TruckTessResult::Mesh {
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                    } => Some(crate::scene::model::mesh_model::MeshModel {
                                        name: String::new(),
                                        verts,
                                        verts_low,
                                        normals,
                                        indices,
                                        triangle_material_handles: Vec::new(),
                                        triangle_colors: Vec::new(),
                                        color,
                                        selected: false,
                                    }),
                                    _ => None,
                                }
                            }
                        }

                        _ => None,
                    };
                    mesh
                });

                if let Some(mut mesh) = result {
                    let pending = self.begin_undo(i, "SWEEP", 1, true);
                    let new_entity = empty_solid3d();
                    let new_handle = self.tabs[i].scene.add_entity(new_entity);
                    mesh.name = format!("{}", new_handle.value());
                    self.tabs[i]
                        .scene
                        .meshes
                        .insert(new_handle, crate::scene::MeshLodSet::from_single(mesh));
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(crate::t!("SWEEP: solid created.").as_ref());
                    if let Some(pd) = pending {
                        self.commit_undo_delta(i, pd);
                    }
                } else {
                    self.command_line.push_error(crate::t!("SWEEP: could not sweep profile along path. Use a closed 2D profile and a Line or Polyline path.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }

            // ── LOFT ───────────────────────────────────────────────────────
            CmdResult::LoftEntities { handles, color } => {
                use crate::entities::traits::EntityTypeOps;
                use crate::modules::insert::solid3d_cmds::empty_solid3d;
                use crate::scene::convert::acad_to_truck::TruckObject;
                use crate::scene::convert::truck_tess;
                use truck_modeling::builder;

                // Collect wires from each profile.
                let mut wires: Vec<truck_modeling::Wire> = Vec::new();
                for h in &handles {
                    if let Some(ent) = self.tabs[i].scene.document.get_entity(*h).cloned() {
                        if let Some(te) = ent.to_truck_entity(&self.tabs[i].scene.document) {
                            let wire = match te.object {
                                TruckObject::Contour(w) => Some(w),
                                TruckObject::Curve(e) => Some(std::iter::once(e).collect()),
                                _ => None,
                            };
                            if let Some(w) = wire {
                                wires.push(w);
                            }
                        }
                    }
                }

                let result: Option<crate::scene::model::mesh_model::MeshModel> = (|| {
                    if wires.len() < 2 {
                        return None;
                    }

                    // Build ruled shells between consecutive profile pairs.
                    let mut all_faces: Vec<truck_modeling::Face> = Vec::new();

                    for pair in wires.windows(2) {
                        let shell = builder::try_wire_homotopy(&pair[0], &pair[1]).ok()?;
                        for face in shell.into_iter() {
                            all_faces.push(face);
                        }
                    }

                    // Cap the first and last profiles if they are closed.
                    if let Ok(cap) = builder::try_attach_plane(&[wires.first()?.clone()]) {
                        all_faces.push(cap);
                    }
                    if let Ok(cap) = builder::try_attach_plane(&[wires.last()?.clone()]) {
                        all_faces.push(cap);
                    }

                    let shell = truck_modeling::Shell::from(all_faces);
                    match truck_tess::tessellate_shell(&shell) {
                        truck_tess::TruckTessResult::Mesh {
                            verts,
                            verts_low,
                            normals,
                            indices,
                        } => Some(crate::scene::model::mesh_model::MeshModel {
                            name: String::new(),
                            verts,
                            verts_low,
                            normals,
                            indices,
                            triangle_material_handles: Vec::new(),
                            triangle_colors: Vec::new(),
                            color,
                            selected: false,
                        }),
                        _ => None,
                    }
                })(
                );

                if let Some(mut mesh) = result {
                    let pending = self.begin_undo(i, "LOFT", 1, true);
                    let new_entity = empty_solid3d();
                    let new_handle = self.tabs[i].scene.add_entity(new_entity);
                    mesh.name = format!("{}", new_handle.value());
                    self.tabs[i]
                        .scene
                        .meshes
                        .insert(new_handle, crate::scene::MeshLodSet::from_single(mesh));
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(crate::tf!(
                        "LOFT: solid created from {} profiles.",
                        handles.len()
                    ).as_ref());
                    if let Some(pd) = pending {
                        self.commit_undo_delta(i, pd);
                    }
                } else {
                    self.command_line.push_error(crate::t!("LOFT: could not loft profiles. Ensure sections have the same edge count and are compatible.").as_ref());
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
            }

            CmdResult::HatcheditApply {
                handle,
                name,
                scale,
                angle,
            } => {
                if let Some(mut model) = self.tabs[i].scene.hatches.get(&handle).cloned() {
                    // Update model fields
                    if !name.is_empty() {
                        use crate::scene::model::hatch_model::HatchPattern;
                        use crate::scene::model::hatch_patterns;
                        model.name = name.clone();
                        if name.to_uppercase() == "SOLID" {
                            model.pattern = HatchPattern::Solid;
                        } else if let Some(entry) = hatch_patterns::find(&name) {
                            model.pattern = entry.gpu.clone();
                        }
                        // If not found in catalog, keep existing pattern type
                    }
                    model.scale = scale;
                    model.angle_offset = angle;

                    self.push_undo_snapshot(i, "HATCHEDIT");
                    // Remove old hatch (entity + GPU model)
                    self.tabs[i].scene.erase_entities(&[handle]);
                    // Re-add with updated model
                    self.tabs[i].scene.add_hatch(model);
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(crate::t!("HATCHEDIT: hatch updated.").as_ref());
                } else {
                    self.command_line
                        .push_error(crate::t!("HATCHEDIT: hatch entity not found.").as_ref());
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
            } => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.open_mtext_editor(pos, handle, &initial, height);
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
            CmdResult::DdeditEntity { handle, new_text } => {
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
                    self.command_line.push_output(crate::t!("DDEDIT: text updated.").as_ref());
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
        if self.mtext_editor.is_some() {
            return self.unfocus_widgets();
        }
        // The in-place TEXT editor needs keyboard focus on its own field.
        if self.text_inline.is_some() {
            return iced::widget::operation::focus(iced::widget::Id::new(
                super::view::TEXT_INLINE_ID,
            ));
        }
        self.focus_cmd_input()
    }

    /// Restore the tangent-snap / ortho state that was in effect before the command started.
    /// Recreate clipboard-dependency records (layer / linetype / text + dim
    /// style) in tab `i`'s document for any the copied entities reference but
    /// this drawing doesn't already have. Each recreated record gets a fresh
    /// handle from the target document so it can't collide with an existing
    /// one. No-op for same-document pastes (the records already exist). (#129)
    pub(super) fn merge_dependencies(
        &mut self,
        i: usize,
        deps: &crate::app::ClipboardDeps,
    ) {
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
        // Recreate any group whose whole membership was copied, so a pasted
        // group stays grouped — cross-drawing too, since the groups were
        // snapshotted into the clipboard at copy time. `by_index` is aligned
        // with `self.clipboard`, so the source handle of each pasted entity maps
        // its clipboard clone to its new handle. Same shared `recreate_groups`
        // the in-drawing COPY path uses. (#440)
        if !self.clipboard_deps.groups.is_empty() {
            let mut handle_map = rustc_hash::FxHashMap::default();
            for (src, &new) in self.clipboard.iter().zip(by_index.iter()) {
                if !new.is_null() {
                    handle_map.insert(src.common().handle, new);
                }
            }
            let groups = self.clipboard_deps.groups.clone();
            self.tabs[i].scene.recreate_groups(groups, &handle_map);
        }
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

/// Parse `base_val,h1;h2;...;hN,spacing` and adjust parallel dimension positions.
fn apply_dimspace(scene: &mut crate::scene::Scene, encoded: &str) {
    // Format: "<base_handle>,<h1>;<h2>;...;<hN>,<spacing>"
    let parts: Vec<&str> = encoded.splitn(3, ',').collect();
    if parts.len() < 3 {
        return;
    }
    let base_val: u64 = parts[0].parse().unwrap_or(0);
    let other_vals: Vec<u64> = parts[1]
        .split(';')
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();
    let spacing: f64 = parts[2].parse().unwrap_or(0.0);

    use acadrust::entities::Dimension;
    let base_h = acadrust::Handle::from(base_val);
    // Base dim: the perpendicular direction (from its rotation / axis) and the
    // dim line's perpendicular coordinate. Spacing steps each parallel dim along
    // this perp IN THE DRAWING PLANE — offsetting Z had no effect on the dim
    // line, which is computed from def·perp with perp.z = 0. (#181 / DIM-021)
    let (perp, base_coord) = match scene.document.get_entity(base_h) {
        Some(acadrust::EntityType::Dimension(Dimension::Linear(d))) => {
            let (s, c) = d.rotation.sin_cos();
            let perp = (-s, c);
            let dp = d.definition_point;
            (perp, dp.x * perp.0 + dp.y * perp.1)
        }
        Some(acadrust::EntityType::Dimension(Dimension::Aligned(d))) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt().max(1e-12);
            let perp = (-dy / len, dx / len);
            let dp = d.definition_point;
            (perp, dp.x * perp.0 + dp.y * perp.1)
        }
        _ => return,
    };

    let effective_spacing = if spacing <= 0.0 { 10.0 } else { spacing };
    let mut changes = Vec::new();
    for (idx, &hv) in other_vals.iter().enumerate() {
        let h = acadrust::Handle::from(hv);
        let target = base_coord + effective_spacing * (idx + 1) as f64;
        let mut changed = false;
        if let Some(acadrust::EntityType::Dimension(dim)) = scene.document.get_entity_mut(h) {
            // Slide this dim's definition point along perp so its perpendicular
            // coordinate equals `target`; update both the struct field (render)
            // and base (save).
            let slide = |p: &mut acadrust::types::Vector3| {
                let cur = p.x * perp.0 + p.y * perp.1;
                let delta = target - cur;
                p.x += perp.0 * delta;
                p.y += perp.1 * delta;
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
}

// ── MLEADERALIGN helper ───────────────────────────────────────────────────────

/// Parse `h1,h2,...;fx,fz;tx,tz` and align multileader content points along the direction.
fn apply_mleader_align(scene: &mut crate::scene::Scene, encoded: &str) {
    // Format: "<h1>,<h2>,...;<fx>,<fz>;<tx>,<tz>"
    let parts: Vec<&str> = encoded.splitn(3, ';').collect();
    if parts.len() < 3 {
        return;
    }
    let handles: Vec<acadrust::Handle> = parts[0]
        .split(',')
        .filter_map(|s| s.parse::<u64>().ok().map(acadrust::Handle::from))
        .collect();
    let from_parts: Vec<f64> = parts[1].split(',').filter_map(|s| s.parse().ok()).collect();
    let to_parts: Vec<f64> = parts[2].split(',').filter_map(|s| s.parse().ok()).collect();
    if from_parts.len() < 2 || to_parts.len() < 2 || handles.is_empty() {
        return;
    }

    let fx = from_parts[0];
    let fz = from_parts[1];
    let tx = to_parts[0];
    let tz = to_parts[1];
    let dx = tx - fx;
    let dz = tz - fz;
    let len = (dx * dx + dz * dz).sqrt();
    if len < 1e-9 {
        return;
    }

    // Project each multileader's content point onto the alignment line, then
    // snap it to the line (preserve perpendicular offset from line is discarded;
    // align along direction through `from`).
    for h in handles {
        if let Some(acadrust::EntityType::MultiLeader(ml)) = scene.document.get_entity_mut(h) {
            let cp = &mut ml.context.content_base_point;
            // Project onto line from_pt + t * dir: keep t component, set perpendicular = 0
            let rel_x = cp.x - fx;
            let rel_z = cp.z - fz;
            let t = (rel_x * (dx / len) + rel_z * (dz / len)) / len;
            let t = t.clamp(0.0, 1.0);
            cp.x = fx + t * dx;
            cp.z = fz + t * dz;
        }
    }
}

// ── MLEADERCOLLECT helper ─────────────────────────────────────────────────────

/// Parse `h1,h2,...;px,pz` — merge all selected multileaders into the first one at position.
fn apply_mleader_collect(scene: &mut crate::scene::Scene, encoded: &str) {
    let parts: Vec<&str> = encoded.splitn(2, ';').collect();
    if parts.len() < 2 {
        return;
    }
    let handles: Vec<acadrust::Handle> = parts[0]
        .split(',')
        .filter_map(|s| s.parse::<u64>().ok().map(acadrust::Handle::from))
        .collect();
    let pos_parts: Vec<f64> = parts[1].split(',').filter_map(|s| s.parse().ok()).collect();
    if handles.len() < 2 || pos_parts.len() < 2 {
        return;
    }

    let px = pos_parts[0];
    let pz = pos_parts[1];

    // Collect all leader roots from the secondary multileaders.
    let mut extra_roots: Vec<acadrust::entities::LeaderRoot> = Vec::new();
    for &h in &handles[1..] {
        if let Some(acadrust::EntityType::MultiLeader(ml)) = scene.document.get_entity(h) {
            extra_roots.extend(ml.context.leader_roots.iter().cloned());
        }
    }

    // Add collected roots to the first multileader and move its content point.
    if let Some(acadrust::EntityType::MultiLeader(ml)) = scene.document.get_entity_mut(handles[0]) {
        ml.context.content_base_point.x = px;
        ml.context.content_base_point.z = pz;
        for root in extra_roots {
            ml.context.leader_roots.push(root);
        }
    }

    // Erase the secondary multileaders.
    scene.erase_entities(&handles[1..]);
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
