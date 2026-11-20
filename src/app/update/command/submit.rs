use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn on_command_submit(&mut self) -> Task<Message> {
        if self.table_cell_editor.is_some() {
            if self.field_dialog.is_some() && self.tabs[self.active_tab].active_cmd.is_some() {
                return self.feed_command(crate::command::StepInput::Enter);
            }
            return self.table_editor_step(1, 0, false);
        }
        // Submitting a command implicitly dismisses the history
        // dropdown so the dispatched command's new prompt is
        // immediately visible on the overlay.
        self.command_line.close_history();
        // A table cell's editor: Enter writes the cell and moves down.
        if let Some(task) = self.table_cell_enter() {
            return task;
        }
        if self.awaiting_fill_close_answer() {
            let input = std::mem::take(&mut self.command_line.input);
            return self.on_graphic_fill_close_input(&input);
        }
        // A leading `>` was only a "literal spaces" typing hint (see
        // CommandSpace) — drop it before the input is interpreted.
        // Free-form text prompts keep it: there it is content, not a
        // hint, so a cell value like `>Note` commits verbatim.
        if !self.is_free_text_active() && self.command_line.input.starts_with('>') {
            self.command_line.input.remove(0);
        }
        // Fence / WPolygon / CPolygon typed while a clicked box waits for
        // its opposite corner turn the box into that path.
        if let Some(task) = self.try_box_path_keyword() {
            return task;
        }
        // Grip-menu value prompt — consume the typed number and
        // route it through `apply_grip_menu_value`.
        // Interactive grip prompts are valid only while their matching
        // grip edit is alive. Do not let a stale Radius / Lengthen /
        // Arc Length prompt consume Enter or coordinates from a later
        // drawing command.
        let stale_interactive_grip_prompt = self.grip_pending.as_ref().is_some_and(|pending| {
            let expected_mode = match pending.action {
                crate::scene::model::object::GripMenuAction::Lengthen => {
                    Some(GripEditMode::Lengthen)
                }
                crate::scene::model::object::GripMenuAction::Radius => Some(GripEditMode::Radius),
                crate::scene::model::object::GripMenuAction::ArcLength => {
                    Some(GripEditMode::ArcLength)
                }
                crate::scene::model::object::GripMenuAction::RectangleWidth => {
                    Some(GripEditMode::RectangleWidth)
                }
                crate::scene::model::object::GripMenuAction::RectangleHeight => {
                    Some(GripEditMode::RectangleHeight)
                }
                crate::scene::model::object::GripMenuAction::MoveParallel => {
                    Some(GripEditMode::MoveParallel)
                }
                _ => None,
            };
            expected_mode.is_some_and(|mode| {
                !self.tabs[self.active_tab]
                    .active_grip
                    .as_ref()
                    .is_some_and(|grip| {
                        grip.mode == mode
                            && grip.handle == pending.handle
                            && grip.grip_id == pending.grip_id
                    })
            })
        });
        if stale_interactive_grip_prompt {
            self.grip_pending = None;
        }
        if let Some(pending) = self.grip_pending.take() {
            let i = self.active_tab;
            if self.reject_locked_edit(i, pending.handle) {
                self.cancel_active_grip_edit();
                return Task::none();
            }
            // Dynamic Input keeps numeric typing in its focused field.
            // Fall back to the command-line buffer so the established
            // prompt workflow remains unchanged when DYN is disabled.
            let dyn_value = self.tabs[i]
                .dyn_fields
                .iter()
                .find_map(|field| field.buffer.as_deref());
            let entered = dyn_value.unwrap_or(self.command_line.input.trim());
            let raw = crate::app::expr_eval::eval_to_string(entered);
            self.command_line.input.clear();
            let Ok(v) = raw.parse::<f64>() else {
                self.command_line.push_error(
                    crate::tf!("{}: expected a number, got \"{raw}\"", pending.label).as_ref(),
                );
                self.grip_pending = Some(pending);
                return self.focus_cmd_input();
            };
            let interactive_value_grip = self.tabs[i].active_grip.as_ref().is_some_and(|grip| {
                matches!(
                    (grip.mode, pending.action),
                    (
                        GripEditMode::Lengthen,
                        crate::scene::model::object::GripMenuAction::Lengthen,
                    ) | (
                        GripEditMode::Radius,
                        crate::scene::model::object::GripMenuAction::Radius,
                    ) | (
                        GripEditMode::ArcLength,
                        crate::scene::model::object::GripMenuAction::ArcLength,
                    ) | (
                        GripEditMode::RectangleWidth,
                        crate::scene::model::object::GripMenuAction::RectangleWidth,
                    ) | (
                        GripEditMode::RectangleHeight,
                        crate::scene::model::object::GripMenuAction::RectangleHeight,
                    ) | (
                        GripEditMode::MoveParallel,
                        crate::scene::model::object::GripMenuAction::MoveParallel,
                    )
                ) && grip.handle == pending.handle
                    && grip.grip_id == pending.grip_id
            });
            if interactive_value_grip {
                self.cancel_active_grip_edit();
            }
            use crate::entities::traits::EntityTypeOps;
            self.push_undo_snapshot(i, pending.label);
            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(pending.handle) {
                entity.apply_grip_menu_value(pending.grip_id, pending.action, v);
            }
            // A typed grip-menu value reshapes dimensions too — drop a
            // stale baked *D block (no-op for non-dims). (#398)
            self.tabs[i]
                .scene
                .invalidate_dim_block_recorded(pending.handle);
            self.tabs[i]
                .scene
                .bump_entities(&[(pending.handle, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
            self.refresh_selected_grips();
            self.refresh_properties();
            return Task::none();
        }

        // Enter settles a custom grip at its current point. In particular it
        // must never reach the empty-submit repeat-last-command shortcut.
        #[cfg(not(target_arch = "wasm32"))]
        if self.tabs[self.active_tab].active_plugin_grip.is_some() {
            return self.settle_plugin_grip_from_enter();
        }

        // Numeric entry while a normal grip stretch is active.
        //
        // With Dynamic Input enabled, typed values live in the shared
        // Distance / Angle fields. Resolve those fields into an exact
        // world point. Without DYN input, preserve the existing direct-
        // distance command-line behaviour.
        {
            let i = self.active_tab;

            // MTP / M2P while a grip is hot (no command runs a grip edit):
            // pick the two points, and their midpoint places the grip.
            if self.grip_point_prompt(i) {
                let raw = self.command_line.input.trim().trim_start_matches('_');
                if raw.eq_ignore_ascii_case("MTP") || raw.eq_ignore_ascii_case("M2P") {
                    self.command_line.input.clear();
                    self.start_mtp_modifier(i);
                    return self.focus_cmd_input();
                }
            }
            let point_override = self.grip_point_override.take();

            if let Some(grip) = self.tabs[i]
                .active_grip
                .clone()
                .filter(|_| self.tabs[i].active_cmd.is_none())
            {
                if matches!(
                    grip.mode,
                    GripEditMode::Stretch | GripEditMode::RectangleResize
                ) {
                    let dyn_locked = self.tabs[i]
                        .dyn_fields
                        .iter()
                        .any(|field| field.buffer.is_some());

                    let target = if let Some(point) = point_override {
                        Some(point)
                    } else if matches!(grip.mode, GripEditMode::RectangleResize) {
                        grip.rectangle_frame
                            .map(|(opposite, width_axis, height_axis)| {
                                let cursor_delta = self.tabs[i].last_cursor_world - opposite;
                                let mut width = cursor_delta.dot(width_axis);
                                let mut height = cursor_delta.dot(height_axis);
                                for field in &self.tabs[i].dyn_fields {
                                    let value = field.buffer.as_ref().and_then(|buffer| {
                                        crate::app::expr_eval::eval_number(buffer)
                                    });
                                    if let Some(value) = value {
                                        match field.role {
                                            crate::command::DynRole::Width => {
                                                width = value.abs().copysign(width);
                                            }
                                            crate::command::DynRole::Height => {
                                                height = value.abs().copysign(height);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                opposite + width_axis * width + height_axis * height
                            })
                    } else if dyn_locked {
                        // Distance only:
                        //   keep the cursor's current direction.
                        //
                        // Distance + Angle:
                        //   resolve both typed values from the grip's
                        //   original position (`dyn_anchor`).
                        self.dyn_resolve_point()
                    } else {
                        // Legacy command-line direct-distance entry.
                        let text =
                            crate::app::expr_eval::eval_to_string(self.command_line.input.trim());

                        crate::app::expr_eval::eval_number(text.trim()).map(|dist| {
                            let cursor = self.tabs[i].last_cursor_world;

                            // If the cursor is following OTRACK or Extension,
                            // preserve the existing reference-ray behaviour.
                            if let Some((base, dir)) = self.active_distance_ray(i) {
                                base + dir * dist
                            } else {
                                let direction = cursor - grip.origin_world;
                                let len = direction.length();

                                if len <= 1e-12 {
                                    grip.origin_world
                                } else {
                                    grip.origin_world + direction / len * dist
                                }
                            }
                        })
                    };

                    if let Some(target) = target {
                        // Typed Dynamic Input can commit a grip before the mouse has moved.
                        //
                        // Normally the first ViewportMove initializes the grip preview handles and
                        // snapshots the original entities. Without that move the document changes,
                        // so the refreshed grips move, but the resident wire tessellation is never
                        // invalidated by the normal grip-finalization path.
                        //
                        // Seed the same bookkeeping here before modifying the entities.
                        if self.grip_preview_handles.is_empty() {
                            let mut seen_handles = rustc_hash::FxHashSet::default();

                            let edited_handles: Vec<_> = grip
                                .targets
                                .iter()
                                .map(|target| target.handle)
                                .filter(|handle| seen_handles.insert(*handle))
                                .collect();

                            if self.grip_dirty_before.is_none() {
                                self.grip_dirty_before = Some(self.tabs[i].dirty);
                            }

                            if self.grip_originals.is_empty() {
                                self.grip_originals = edited_handles
                                    .iter()
                                    .filter_map(|&handle| {
                                        self.tabs[i]
                                            .scene
                                            .document
                                            .get_entity(handle)
                                            .cloned()
                                            .map(|entity| (handle, entity))
                                    })
                                    .collect();
                            }

                            self.capture_grip_history_originals(i, &edited_handles);

                            self.grip_preview_handles = edited_handles;
                        }
                        let delta = target - grip.last_world;

                        let actions: Vec<_> = if let Some((opposite, width_axis, height_axis)) =
                            grip.rectangle_frame
                        {
                            let opposite_id = (grip.grip_id + 2) % 4;
                            let mut edits = Vec::with_capacity(3);
                            for adjacent_id in [(opposite_id + 1) % 4, (opposite_id + 3) % 4] {
                                if let Some(original) = self.tabs[i]
                                    .selected_grip_handles
                                    .iter()
                                    .zip(self.tabs[i].selected_grips.iter())
                                    .find(|(owner, candidate)| {
                                        **owner == grip.handle && candidate.id == adjacent_id
                                    })
                                    .map(|(_, candidate)| candidate.world)
                                {
                                    let d = original - opposite;
                                    let axis =
                                        if d.dot(width_axis).abs() >= d.dot(height_axis).abs() {
                                            width_axis
                                        } else {
                                            height_axis
                                        };
                                    edits.push((
                                        grip.handle,
                                        adjacent_id,
                                        GripApply::Absolute(
                                            opposite + axis * (target - opposite).dot(axis),
                                        ),
                                    ));
                                }
                            }
                            edits.push((grip.handle, grip.grip_id, GripApply::Absolute(target)));
                            edits
                        } else {
                            grip.targets
                                .iter()
                                .map(|target_grip| {
                                    let apply = if target_grip.is_translate {
                                        GripApply::Translate(delta)
                                    } else {
                                        GripApply::Absolute(target_grip.last_world + delta)
                                    };

                                    (target_grip.handle, target_grip.grip_id, apply)
                                })
                                .collect()
                        };

                        for (handle, grip_id, apply) in actions {
                            self.tabs[i].scene.apply_grip(handle, grip_id, apply);
                        }
                        self.solve_grip_constraints(i, &grip);

                        // Keep GripEdit synchronized so the normal commit
                        // path records the exact final position.
                        if let Some(active) = self.tabs[i].active_grip.as_mut() {
                            active.last_world = target;

                            for target_grip in &mut active.targets {
                                target_grip.last_world += delta;
                            }
                        }

                        self.tabs[i].last_cursor_world = target;
                        self.command_line.input.clear();

                        // Consume the typed Dynamic Input values.
                        for field in &mut self.tabs[i].dyn_fields {
                            field.buffer = None;
                        }
                        self.tabs[i].dyn_active = 0;
                        self.dyn_user_reshaped = false;
                        self.dyn_coord_absolute = self.dyn_pointer_absolute;

                        self.tabs[i].dirty = true;

                        // Reuse the existing grip finalization path:
                        // undo grouping, preview restoration, cleanup, etc.
                        let task = self.on_viewport_left_release();

                        // active_grip is now gone, so remove the temporary
                        // Distance / Angle fields as well.
                        self.sync_dyn_fields();

                        // A typed grip commit bypasses the normal viewport point-release
                        // tracking cleanup. Drop the consumed OTRACK / Extension guide now
                        // so no stale tracking ray remains visible after the grip edit.
                        self.reset_tracking_after_point();

                        return task;
                    }
                }
            }
        }
        // Interactive VPORTS: the entry after a bare `VPORTS` is the
        // tiled configuration. Empty input defaults to SINGLE.
        if self.awaiting_vports {
            self.awaiting_vports = false;
            let cfg = self.command_line.input.trim().to_string();
            self.command_line.input.clear();
            let cfg = if cfg.is_empty() {
                "SINGLE".to_string()
            } else {
                cfg
            };
            return self.dispatch_command(&format!("VPORTS {cfg}"));
        }
        // Interactive system-variable value: after a bare `MIRRTEXT`
        // (etc.) the next entry is the new value; empty Enter keeps the
        // current setting.
        if let Some(name) = self.pending_setvar.take() {
            let val = self.command_line.input.trim().to_string();
            self.command_line.input.clear();
            if val.is_empty() {
                return Task::none();
            }
            self.command_line.answer_last_prompt(&val);
            return self.dispatch_command(&format!("SETVAR {name} {val}"));
        }
        let i_tab = self.active_tab;
        if self.tabs[i_tab].active_cmd.is_none() {
            if let Some(command) = self.command_line.selected_suggestion() {
                self.command_line.input.clear();
                self.command_line.autocomplete_cursor = None;
                self.command_line.push_command(&command);
                return self.dispatch_command(&command);
            }
        } else if self.command_line.has_contextual_suggestions() {
            // During an active command Enter accepts the highlighted
            // contextual value instead of the user's partial search.
            if let Some(value) = self.command_line.selected_suggestion() {
                self.command_line.input = value;
            }
        }
        let i = self.active_tab;
        // A whole multi-token command line (`UCS Z 90`, `LINE 0,0
        // 10,10`, `PDMODE 3`) — typable now that Space is literal — is
        // processed as one unit: feed the tokens to a running command,
        // or start a new one through the shared runner that the headless
        // automation feeder uses too.
        {
            // Skip token-splitting when the active command collects
            // free-form text with spaces (TEXT / MTEXT / a name) — it
            // wants the whole line as one input.
            let wants_spaces = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.is_free_text_step())
                .unwrap_or(false);
            let raw = self.command_line.input.clone();
            if !raw.trim().starts_with('(') {
                let toks: Vec<String> = raw.split_whitespace().map(String::from).collect();
                if toks.len() > 1 && !wants_spaces {
                    self.command_line.input.clear();
                    if self.tabs[i].active_cmd.is_some() {
                        let mut tasks = Vec::new();
                        for tok in &toks {
                            if self.tabs[i].active_cmd.is_none() {
                                break;
                            }
                            tasks.push(self.feed_active_cmd(tok));
                        }
                        return Task::batch(tasks);
                    }
                    // Echo the line like a single word; a variable answered on
                    // it is restated to its name when its prompt takes the value.
                    self.command_line.push_command(raw.trim());
                    return self.run_command_line(&raw);
                }
            }
        }
        // With the command line empty, a typed dynamic-input value
        // commits as a point pick instead of an empty submit.
        if self.tabs[i].active_cmd.is_some() && self.command_line.input.trim().is_empty() {
            if let Some(task) = self.try_dyn_commit() {
                return task;
            }
        }
        if self.tabs[i].active_cmd.is_some() {
            // Free-form text is the content itself: no expression
            // evaluation, or a cell value like `5*2` would commit as
            // `10`. Single-token prompts keep the calculator behavior.
            let free_text = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.is_free_text_step())
                .unwrap_or(false);
            let raw = self.command_line.input.trim().to_string();
            if raw.trim_start_matches('_').eq_ignore_ascii_case("FROM")
                && self.point_modifier_step(i)
            {
                self.command_line.input.clear();
                self.start_from_modifier(i);
                return self.focus_cmd_input();
            }
            let is_mtp = raw.trim_start_matches('_').eq_ignore_ascii_case("MTP")
                || raw.trim_start_matches('_').eq_ignore_ascii_case("M2P");
            if is_mtp && self.point_modifier_step(i) {
                self.command_line.input.clear();
                self.start_mtp_modifier(i);
                return self.focus_cmd_input();
            }
            if let Some(kind) = self.typed_snap_override(i, &raw) {
                self.command_line.input.clear();
                self.apply_snap_override(kind);
                return self.focus_cmd_input();
            }
            let text = if free_text {
                raw
            } else {
                crate::app::expr_eval::eval_to_string(&raw)
            };
            self.command_line.input.clear();
            // Remember the token for the context menu's Recent Input
            // list (prose steps excluded: a table cell or text body
            // is not a reusable value).
            let is_prose = self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|c| c.input_kind().is_free_text());
            if !is_prose {
                self.command_line.record_recent_input(&text);
            }

            // Offer the typed text to the command's option handler
            // first (keywords like PLINE's A/L/C, a radius, …). If it
            // consumes the text we're done; if it returns None the
            // text falls through to the Enter / coordinate handling
            // below, so a bare Enter still terminates and typed points
            // still work when dynamic input is off. See #97.
            // Selection keywords (P / L) at a Select objects prompt
            // consume the token before any other interpretation (#426).
            // A typed window waiting for its opposite corner reads this
            // entry first, Enter included.
            if self.typed_window_corner.is_some() {
                if let Some(task) = self.typed_object_pick(&text) {
                    return task;
                }
            }
            if let Some(task) = self.try_selection_keyword(&text) {
                return task;
            }
            // Under a typed `<angle` a bare number is the distance along it,
            // before the step reads it as a value of its own.
            if self.angle_override.is_some() {
                if let Some(task) = self.try_direct_distance_entry(&text) {
                    return task;
                }
            }
            // A point prompt that also takes keywords reads a typed
            // coordinate as the point: handing `300,50` to the step's text
            // reader would turn the comma into a decimal mark (300.5).
            // A coordinate under a typed object snap is a point at any step
            // that takes one, so the snap resolves it.
            let coordinate_at_point_step = parse_coord(&text).is_some()
                && (self.snapper.pending_override().is_some()
                    || self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.point_step_accepts_keywords()));
            if !coordinate_at_point_step
                && self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.input_kind().wants_text())
                    .unwrap_or(false)
            {
                // An empty submit must not be handed to on_text_input —
                // a text step would commit the empty string (wiping a
                // table cell, clearing an attribute). It falls through
                // to the Enter handling below, which ends the step the
                // same way a bare Enter does: unchanged.
                if !text.is_empty() {
                    if let Some(task) = self.offer_typed_text(i, &text) {
                        return task;
                    }
                }
            }

            if text.is_empty() {
                return self.feed_command(crate::command::StepInput::Enter);
            }

            if self.try_angle_override(&text) {
                return Task::none();
            }

            // OTRACK and Extension both allow a bare scalar to act as a
            // distance measured along the active reference ray.
            if self.active_distance_ray(i).is_some() {
                if let Some(task) = self.try_direct_distance_entry(&text) {
                    return task;
                }
            }

            if let Some(task) = self.typed_object_pick(&text) {
                return task;
            }
            if let Some((coord, kind)) = parse_coord(&text) {
                // Command-line coordinates are absolute by default,
                // independent of DYN: `@` forces relative, `#` forces
                // absolute, and a bare value is absolute. (Relative-by-
                // default lives in the DYN tooltip path — see
                // `dyn_resolve_point`; command-line coordinates stay
                // absolute regardless of the DYN setting.)
                let want_relative = matches!(kind, CoordKind::Relative);
                let ucs = self.tabs[i].active_ucs.clone();
                let wcs_pt = match (want_relative, self.relative_input_base(i)) {
                    (true, Some(base)) => {
                        // Offset from the last point, rotated by the
                        // UCS axes (no origin translation).
                        let offset = match &ucs {
                            Some(u) => ucs_rotate_vec(coord, u),
                            None => coord,
                        };
                        base + offset
                    }
                    _ => {
                        // Absolute: typed coordinates are in active UCS.
                        match &ucs {
                            Some(u) => ucs_to_wcs(coord, u),
                            None => coord,
                        }
                    }
                };
                // An object snap override typed before the point snaps it.
                let (wcs_pt, snapped) = match self.typed_override_snap(i, wcs_pt) {
                    Some(Err(())) => return self.focus_cmd_input(),
                    Some(Ok(Some(hit))) => (hit.world, Some(hit)),
                    Some(Ok(None)) | None => (wcs_pt, None),
                };
                if !self.command_point_allowed(i, wcs_pt) {
                    return Task::none();
                }
                // The accepted-snap list stays index-parallel with the
                // points the command collects, as on the feed path.
                if !self.record_accepted_snap(i, snapped, None, wcs_pt) {
                    return Task::none();
                }
                self.last_point = Some(wcs_pt);
                self.dyn_user_reshaped = false;
                self.dyn_coord_absolute = self.dyn_pointer_absolute;
                self.sync_dyn_fields();
                self.reset_tracking_after_point();
                self.snapper.clear_override();
                self.push_ucs_to_cmd(i);
                let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_point(wcs_pt));
                if let Some(r) = result {
                    let task = self.apply_cmd_result(r);
                    // The rubber-band preview that the command
                    // last published reflects the *previous*
                    // last_point — a typed coordinate doesn't
                    // fire a mouse-move, so re-run the preview
                    // hook now using the current cursor world
                    // pos so the next segment immediately starts
                    // from the just-committed point. See #32.
                    self.refresh_active_cmd_preview(i);
                    return task;
                }
                return Task::none();
            }

            if let Some(task) = self.offer_typed_text(i, &text) {
                return task;
            }

            // Direct distance entry: if an anchor exists and a scalar distance was entered,
            // project along the active reference ray (if any) or the current cursor direction
            // in the active UCS plane.
            if let Some(task) = self.try_direct_distance_entry(&text) {
                return task;
            }

            // A point prompt refuses other text as the reference does, with
            // a plain line before asking again.
            if let Some(command) = self.tabs[i].active_cmd.as_deref() {
                let line = crate::command::point_rejection_line(command);
                self.command_line.push_rejection(&line);
            }
            return self.focus_cmd_input();
        }
        if let Some(cmd) = self.command_line.submit() {
            return self.dispatch_command_or_suggest(&cmd);
        }
        // Empty Enter with a pending client user_select request is the
        // person's answer: hand the picked set back to the caller
        // before the repeat-last-command shortcut can fire.
        if self.control.user_select.is_some() {
            self.resolve_user_select(true);
            return Task::none();
        }
        // Empty Enter / Space with no active command repeats the
        // last dispatched command — same shortcut `CommandFinalize`
        // already implements, mirrored here so the trailing-space
        // submit path goes through it too.
        if let Some(cmd) = self.tabs[i].last_cmd.clone() {
            return self.dispatch_command(&cmd);
        }
        Task::none()
    }

}
