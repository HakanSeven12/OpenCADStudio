//! `command` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    ortho_constrain, parse_coord, polar_constrain_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
    CoordKind,
};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::model::object::GripApply;
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use crate::ui::window::attribute_editor::{AttrRow, AttrTab};
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};


impl OpenCADStudio {
pub(super) fn begin_tab_close_queue(&mut self, tab_ids: Vec<u64>) -> Task<Message> {
                self.pending_tab_closes.clear();
                self.pending_tab_closes.extend(tab_ids);
                self.continue_tab_close_queue()
    }

    /// Close queued drawings until a dirty tab requires confirmation. Queue
    /// entries use stable document ids because removing an earlier tab changes
    /// every later vector index.
    pub(super) fn continue_tab_close_queue(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        while let Some(tab_id) = self.pending_tab_closes.pop_front() {
            let Some(idx) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                continue;
            };
            if self.tabs[idx].is_start {
                continue;
            }
            if self.tabs[idx].dirty {
                self.pending_close = Some(crate::app::PendingClose::Tab(idx));
                tasks.push(self.open_unsaved_dialog_window());
                break;
            }
            tasks.push(self.on_tab_close(idx));
        }
        Task::batch(tasks)
    }

pub(super) fn on_tab_close(&mut self, idx: usize) -> Task<Message> {
                // Start tab is fixed — close requests on it are no-ops.
                if self.tabs.get(idx).map_or(false, |t| t.is_start) {
                    return Task::none();
                }
                // Closing a tab shifts indices / the active tab. The attribute
                // editor holds a document-local handle into one tab, so drop it
                // now rather than risk it applying to a different tab's document.
                self.cancel_attr_editor();
                if self.tabs.get(idx).map_or(false, |t| t.dirty) {
                    self.pending_close = Some(crate::app::PendingClose::Tab(idx));
                    return self.open_unsaved_dialog_window();
                }
                // This tab is closing for good — drop its autosave recovery copy.
                #[cfg(not(target_arch = "wasm32"))]
                let _ = std::fs::remove_file(self.autosave_target(idx));
                // Only-tab case: when the lone non-start tab closes, fall
                // back to the Start tab if it exists; otherwise spawn a
                // fresh blank drawing (legacy behaviour).
                if self.tabs.len() == 1 {
                    self.tab_counter += 1;
                    self.tabs[0] = crate::app::document::DocumentTab::new_drawing(self.tab_counter);
                    self.active_tab = 0;
                    self.apply_bg_default(0);
                } else {
                    self.tabs.remove(idx);
                    if self.active_tab >= self.tabs.len() {
                        self.active_tab = self.tabs.len() - 1;
                    }
                }
                // The active tab is now either a brand-new blank or a
                // different existing tab; in both cases the ribbon needs
                // to track that doc's defaults / selection. #21.
                self.sync_ribbon_layers();
                self.sync_ribbon_styles();
                self.sync_ribbon_from_selection();
                Task::none()
    }

    pub(super) fn on_command_append_char(&mut self, s: String) -> Task<Message> {
                // While the MText preview is up, typed glyphs edit it directly.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    if s.chars().all(|c| !c.is_control()) {
                        self.mtext_type(&s);
                    }
                    return Task::none();
                }
                // Filter out control characters — only push the typed
                // glyph(s). `Tab`, etc. arrive as Named keys, not here.
                if s.chars().all(|c| !c.is_control()) {
                    let i = self.active_tab;
                    // `,` is the coordinate separator in dynamic input,
                    // not a decimal point: typing it locks the current
                    // field's buffer and advances to the next coordinate,
                    // reshaping the field set when going polar → cartesian
                    // (Distance → X, Y) or 2-D → 3-D (X, Y → X, Y, Z).
                    // See #35.
                    if s == "," && self.dyn_input && !self.tabs[i].dyn_fields.is_empty() {
                        self.dyn_comma_advance();
                        self.command_line.autocomplete_cursor = None;
                        self.refresh_active_cmd_preview(i);
                        return self.focus_cmd_input();
                    }
                    // While dynamic input is showing fields, numeric and
                    // expression glyphs edit the focused field instead of
                    // the command line. Letters still go to the command line
                    // so command-option keywords keep working.
                    let dyn_field_char = !s.is_empty()
                        && s.chars().all(|c| {
                            c.is_ascii_digit()
                                || matches!(c, '.' | '-' | '+' | '*' | '/' | '^' | '%' | '(' | ')')
                        });
                    if dyn_field_char && self.dyn_input && !self.tabs[i].dyn_fields.is_empty() {
                        let a = self.tabs[i]
                            .dyn_active
                            .min(self.tabs[i].dyn_fields.len() - 1);
                        self.tabs[i].dyn_fields[a]
                            .buffer
                            .get_or_insert_with(String::new)
                            .push_str(&s);
                    } else {
                        // Command-line entry is shown uppercase.
                        self.command_line.input.push_str(&s.to_uppercase());
                    }
                }
                self.command_line.autocomplete_cursor = None;
                self.focus_cmd_input()
    }

    pub(super) fn on_command_backspace(&mut self) -> Task<Message> {
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    self.mtext_backspace();
                    return Task::none();
                }
                let i = self.active_tab;
                // Backspace edits the focused dynamic-input field first;
                // emptying it unlocks the field (back to cursor tracking).
                if self.dyn_input && !self.tabs[i].dyn_fields.is_empty() {
                    let a = self.tabs[i]
                        .dyn_active
                        .min(self.tabs[i].dyn_fields.len() - 1);
                    if let Some(buf) = self.tabs[i].dyn_fields[a].buffer.as_mut() {
                        buf.pop();
                        if buf.is_empty() {
                            self.tabs[i].dyn_fields[a].buffer = None;
                        }
                        return self.focus_cmd_input();
                    }
                }
                self.command_line.input.pop();
                self.command_line.autocomplete_cursor = None;
                self.focus_cmd_input()
    }

    pub(super) fn on_command_submit(&mut self) -> Task<Message> {
                // Submitting a command implicitly dismisses the history
                // dropdown so the dispatched command's new prompt is
                // immediately visible on the overlay.
                self.command_line.close_history();
                // A leading `>` was only a "literal spaces" typing hint (see
                // CommandSpace) — drop it before the input is interpreted.
                if self.command_line.input.starts_with('>') {
                    self.command_line.input.remove(0);
                }
                // Grip-menu value prompt — consume the typed number and
                // route it through `apply_grip_menu_value`.
                if let Some(pending) = self.grip_pending.take() {
                    let raw = crate::app::expr_eval::eval_to_string(self.command_line.input.trim());
                    self.command_line.input.clear();
                    let Ok(v) = raw.parse::<f64>() else {
                        self.command_line.push_error(&format!(
                            "{}: expected a number, got \"{raw}\"",
                            pending.label
                        ));
                        return Task::none();
                    };
                    let i = self.active_tab;
                    use crate::entities::traits::EntityTypeOps;
                    self.push_undo_snapshot(i, pending.label);
                    if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(pending.handle)
                    {
                        entity.apply_grip_menu_value(pending.grip_id, pending.action, v);
                    }
                    // A typed grip-menu value reshapes dimensions too — drop a
                    // stale baked *D block (no-op for non-dims). (#398)
                    self.tabs[i]
                        .scene
                        .invalidate_dim_block_recorded(pending.handle);
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                    self.refresh_selected_grips();
                    self.refresh_properties();
                    return Task::none();
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
                    return self.dispatch_command(&format!("SETVAR {name} {val}"));
                }
                // If the user navigated the autocomplete list with the
                // arrow keys, Enter dispatches the highlighted command
                // rather than the partial text actually in the buffer.
                let i_tab = self.active_tab;
                if self.tabs[i_tab].active_cmd.is_none() {
                    if let Some(cmd) = self.command_line.selected_suggestion() {
                        self.command_line.input.clear();
                        self.command_line.autocomplete_cursor = None;
                        return self.dispatch_command(&cmd);
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
                        .map(|c| c.wants_text_input() && c.wants_text_with_spaces())
                        .unwrap_or(false);
                    let raw = self.command_line.input.clone();
                    let toks: Vec<String> = raw.split_whitespace().map(String::from).collect();
                    if toks.len() > 1 && !wants_spaces {
                        self.command_line.input.clear();
                        if self.tabs[i].active_cmd.is_some() {
                            for tok in &toks {
                                if self.tabs[i].active_cmd.is_none() {
                                    break;
                                }
                                self.feed_active_cmd(tok);
                            }
                            return Task::none();
                        }
                        return self.run_command_line(&raw);
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
                    let text = crate::app::expr_eval::eval_to_string(self.command_line.input.trim());
                    self.command_line.input.clear();

                    // Offer the typed text to the command's option handler
                    // first (keywords like PLINE's A/L/C, a radius, …). If it
                    // consumes the text we're done; if it returns None the
                    // text falls through to the Enter / coordinate handling
                    // below, so a bare Enter still terminates and typed points
                    // still work when dynamic input is off. See #97.
                    // Selection keywords (P / L) at a Select objects prompt
                    // consume the token before any other interpretation (#426).
                    if let Some(task) = self.try_selection_keyword(&text) {
                        return task;
                    }
                    if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.wants_text_input())
                        .unwrap_or(false)
                    {
                        self.push_ucs_to_cmd(i);
                        if let Some(result) = self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .and_then(|c| c.on_text_input(&text))
                        {
                            return self.apply_cmd_result(result);
                        }
                    }

                    if text.is_empty() {
                        let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_enter());
                        if let Some(r) = result {
                            return self.apply_cmd_result(r);
                        }
                        return Task::none();
                    }

                    // OTRACK: while aligned to a tracking ray, a bare distance
                    // places the point along the ray from the tracking point
                    // (issue #69).
                    if let Some((base, dir)) = self.otrack_active {
                        if let Some(dist) = crate::app::expr_eval::eval_number(text.trim()) {
                            let pt = base + dir * dist;
                            if !self.command_point_allowed(i, pt) {
                                return Task::none();
                            }
                            self.last_point = Some(pt);
                            self.dyn_user_reshaped = false;
                            self.sync_dyn_fields();
                            self.reset_tracking_after_point();
                            self.push_ucs_to_cmd(i);
                            let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_point(pt));
                            if let Some(r) = result {
                                let task = self.apply_cmd_result(r);
                                self.refresh_active_cmd_preview(i);
                                return task;
                            }
                            return Task::none();
                        }
                    }

                    if let Some((coord, kind)) = parse_coord(&text) {
                        // Command-line coordinates are absolute by default,
                        // independent of DYN: `@` forces relative, `#` forces
                        // absolute, and a bare value is absolute. (Relative-by-
                        // default lives in the DYN tooltip path — see
                        // `dyn_resolve_point` — matching AutoCAD, where the
                        // command line stays absolute regardless of DYN.)
                        let want_relative = matches!(kind, CoordKind::Relative);
                        let ucs = self.tabs[i].active_ucs.clone();
                        let wcs_pt = match (want_relative, self.last_point) {
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
                        if !self.command_point_allowed(i, wcs_pt) {
                            return Task::none();
                        }
                        self.last_point = Some(wcs_pt);
                        self.dyn_user_reshaped = false;
                        self.sync_dyn_fields();
                        self.reset_tracking_after_point();
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

                    self.push_ucs_to_cmd(i);
                    if let Some(result) = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .and_then(|c| c.on_text_input(&text))
                    {
                        return self.apply_cmd_result(result);
                    }

                    self.command_line.push_error(&format!(
                        "Expected coordinates (x,y) or a number, got: \"{text}\""
                    ));
                    return self.focus_cmd_input();
                }
                if let Some(cmd) = self.command_line.submit() {
                    return self.dispatch_command_or_suggest(&cmd);
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

    pub(super) fn on_command_finalize(&mut self) -> Task<Message> {
                // In the MText preview, Enter inserts a line break.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    self.mtext_type("\n");
                    return Task::none();
                }
                // Grip popup open → Enter commits the highlighted item.
                if self.grip_popup.is_some() {
                    let idx = self.grip_popup.as_ref().map(|p| p.selected).unwrap_or(0);
                    return Task::done(Message::GripMenuPick(idx));
                }
                // Any typed command-line text must be submitted rather than
                // finalising. The focused-input Enter routes through
                // CommandSubmit, but when the field isn't focused — e.g. at
                // startup before the window grabs focus, or a command started
                // from the ribbon — its Enter arrives here. Forward a non-empty
                // buffer to the same submit path so typing a command name (or
                // an option keyword like "R") and pressing Enter works without
                // first clicking into the command line (issue #99).
                if !self.command_line.input.trim().is_empty() {
                    return self.update(Message::CommandSubmit);
                }
                // A typed dynamic-input value commits as a point pick
                // before the plain-Enter (on_enter) path runs.
                if let Some(task) = self.try_dyn_commit() {
                    return task;
                }
                let i = self.active_tab;
                if self.tabs[i].active_cmd.is_some() {
                    let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_enter());
                    if let Some(r) = result {
                        return self.apply_cmd_result(r);
                    }
                    Task::none()
                } else if let Some(cmd) = self.tabs[i].last_cmd.clone() {
                    self.dispatch_command(&cmd)
                } else {
                    Task::none()
                }
    }

    pub(super) fn on_command_escape(&mut self) -> Task<Message> {
                // Esc drops an unconsumed one-shot snap override and closes
                // its menu (#337). Falls through — Esc keeps its usual effect.
                self.snap_override_popup = None;
                self.snapper.clear_override();
                // Open MText editor swallows Escape (cancel without committing).
                if self.mtext_editor.is_some() {
                    self.mtext_cancel();
                    return self.post_editor_closed(false);
                }
                // The in-place TEXT editor likewise cancels on Escape.
                if self.text_inline.is_some() {
                    self.text_inline_cancel();
                    return self.post_editor_closed(false);
                }
                // Esc cancels an armed pane move.
                if self.pane_move_from.take().is_some() {
                    return Task::none();
                }
                // Esc cancels a pending system-variable value prompt.
                if self.pending_setvar.take().is_some() {
                    self.command_line.push_info("*Cancel*");
                    return Task::none();
                }
                // UCS icon: Esc ends any grip drag and clears the selection
                // (only when no command owns Escape).
                if self.tabs[self.active_tab].active_cmd.is_none()
                    && (self.ucs_grip_drag.is_some() || self.ucs_icon_selected)
                {
                    let i = self.active_tab;
                    self.ucs_grip_drag = None;
                    self.ucs_icon_selected = false;
                    self.ucs_icon_hover = false;
                    self.tabs[i].snap_result = None;
                    return Task::none();
                }
                // Leave interactive PAN mode (and end any in-flight pan drag).
                if self.tabs[self.active_tab].pan_mode {
                    let i = self.active_tab;
                    self.tabs[i].pan_mode = false;
                    {
                        let mut sel = self.tabs[i].scene.selection.borrow_mut();
                        sel.middle_down = false;
                        sel.middle_last_pos = None;
                    }
                    self.command_line.push_output("PAN ended.");
                    return Task::none();
                }
                // Grip popup intercepts Escape — dismisses the menu
                // without doing anything else.
                if self.grip_popup.take().is_some() {
                    self.grip_hover = None;
                    return Task::none();
                }
                if self.visibility_popup.take().is_some() {
                    return Task::none();
                }
                if self.grip_pending.take().is_some() {
                    self.command_line.input.clear();
                    return Task::none();
                }
                // A hot grip (click-move-click placement in progress) ends on
                // Escape, leaving the entity at its last previewed position.
                if self.tabs[self.active_tab].active_grip.take().is_some() {
                    // An Add-Leader arrow being placed: Esc removes it again.
                    if let Some((h, gid)) = self.grip_add_provisional.take() {
                        let i = self.active_tab;
                        use crate::entities::traits::EntityTypeOps;
                        if let Some(e) = self.tabs[i].scene.document.get_entity_mut(h) {
                            e.apply_grip_menu(
                                gid,
                                crate::scene::model::object::GripMenuAction::RemoveLeader,
                            );
                        }
                        self.tabs[i].scene.bump_geometry();
                        self.refresh_selected_grips();
                    }
                    // Cancel an in-progress grip drag: restore the edited
                    // entity from its pre-drag backup, un-hide it, re-tessellate
                    // once, and drop the preview.
                    if let Some(h) = self.grip_preview_handle.take() {
                        let i = self.active_tab;
                        self.grip_text_verts = Vec::new();
                        self.grip_text_slide = false;
                        if let Some(orig) = self.grip_original.take() {
                            if let Some(e) = self.tabs[i].scene.document.get_entity_mut(h) {
                                *e = orig;
                            }
                        }
                        self.tabs[i].scene.preview_hidden.remove(&h);
                        self.tabs[i].scene.clear_preview_wire();
                        // Geometry restored to the backup — re-tessellate just it.
                        self.tabs[i]
                            .scene
                            .bump_entities(&[(h, crate::scene::ChangeKind::Modified)]);
                        self.refresh_selected_grips();
                    }
                    self.tabs[self.active_tab].snap_result = None;
                    self.refresh_properties();
                    return Task::none();
                }
                // Cancel layout rename first, then fall through.
                let i_e = self.active_tab;
                if self.qselect.take().is_some() {
                    return Task::none();
                }
                {
                    let mut sel = self.tabs[i_e].scene.selection.borrow_mut();
                    if sel.context_menu.is_some() {
                        sel.context_menu = None;
                        return Task::none();
                    }
                }
                if self.layout_rename_state.take().is_some() {
                    return Task::none();
                }
                // Typed text on the command line cancels first — one
                // Esc empties the buffer, a second Esc then escalates
                // to whatever the current mode would otherwise do
                // (cancel command / exit viewport / deselect).
                if !self.command_line.input.is_empty() {
                    self.command_line.input.clear();
                    self.command_line.autocomplete_cursor = None;
                    self.command_line.close_history();
                    return Task::none();
                }
                let i = self.active_tab;
                if self.tabs[i].active_cmd.is_some() {
                    let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_escape());
                    if let Some(r) = result {
                        return self.apply_cmd_result(r);
                    }
                } else if self.tabs[i].scene.active_viewport.is_some() {
                    // ESC while in MSPACE → exit back to paper space.
                    return Task::done(Message::ExitViewport);
                } else {
                    self.tabs[i].scene.deselect_all();
                    self.refresh_properties();
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    sel.box_anchor = None;
                    // Also drop the world-space anchor, or the next pan/zoom
                    // re-projects it back into `box_anchor` and the cancelled
                    // marquee springs back to life (reproject_box_anchor).
                    sel.box_anchor_world = None;
                    sel.box_current = None;
                    sel.box_crossing = false;
                }
                Task::none()
    }

    pub(super) fn on_layer_toggle_vp_freeze(&mut self, layer_idx: usize, vp_col_idx: usize) -> Task<Message> {
                let i = self.active_tab;
                let vp_handle = self.tabs[i]
                    .layers
                    .vp_cols
                    .get(vp_col_idx)
                    .map(|c| c.handle);
                let layer_name = self.tabs[i]
                    .layers
                    .layers
                    .get(layer_idx)
                    .map(|l| l.name.clone());

                if let (Some(vp_handle), Some(layer_name)) = (vp_handle, layer_name) {
                    // Get the layer handle from the document
                    if let Some(doc_layer) = self.tabs[i].scene.document.layers.get(&layer_name) {
                        let layer_handle = doc_layer.handle;
                        self.push_undo_snapshot(i, "VPLAYER");

                        // Toggle frozen_layers on the viewport entity
                        for e in self.tabs[i].scene.document.entities_mut() {
                            if let acadrust::EntityType::Viewport(vp) = e {
                                if vp.common.handle == vp_handle {
                                    if vp.frozen_layers.contains(&layer_handle) {
                                        vp.frozen_layers.retain(|h| h != &layer_handle);
                                    } else {
                                        vp.frozen_layers.push(layer_handle);
                                    }
                                    break;
                                }
                            }
                        }

                        // Re-sync layer panel with updated VP info
                        let vp_info = self.tabs[i].scene.viewport_list();
                        let doc_layers = self.tabs[i].scene.document.layers.clone();
                        self.tabs[i]
                            .layers
                            .sync_with_viewports(&doc_layers, vp_info);
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
    }

    pub(super) fn on_layer_new(&mut self) -> Task<Message> {
                let i = self.active_tab;
                let mut n = 1;
                let new_name = loop {
                    let candidate = format!("Layer{}", n);
                    if !self.tabs[i].scene.document.layers.contains(&candidate) {
                        break candidate;
                    }
                    n += 1;
                };
                self.push_undo_snapshot(i, "LAYER NEW");
                use acadrust::tables::layer::Layer as DocLayer;
                // A layer needs a real handle or it is dropped on a DWG save
                // (the format is handle-based; issue #67).
                let mut dl = DocLayer::new(&new_name);
                // `allocate_handle` advances the seed so the layer gets a
                // unique handle; the non-advancing `next_handle` getter hands
                // out the same value twice and the later object overwrites it.
                dl.handle = self.tabs[i].scene.document.allocate_handle();
                let _ = self.tabs[i].scene.document.layers.add(dl);
                self.tabs[i].dirty = true;
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                let vp_info = self.tabs[i].scene.viewport_list();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                let new_idx = self.tabs[i]
                    .layers
                    .layers
                    .iter()
                    .position(|l| l.name == new_name);
                if let Some(idx) = new_idx {
                    self.tabs[i].layers.selected = Some(idx);
                    self.tabs[i].layers.selected_multi = vec![idx];
                    self.tabs[i].layers.editing = Some(idx);
                    self.tabs[i].layers.edit_buf = new_name.clone();
                }
                self.sync_ribbon_layers();
                // With alphabetical ordering (#270) the new layer can land
                // anywhere in a long list; scroll its row into view so the
                // rename prompt is visible (#271). Offset is layout-independent
                // (idx × fixed row height), so it holds even before the freshly
                // added row is measured.
                match new_idx {
                    Some(idx) => iced::widget::operation::scroll_to(
                        iced::advanced::widget::Id::new(
                            crate::ui::window::layers::LAYER_TABLE_SCROLL_ID,
                        ),
                        iced::widget::scrollable::AbsoluteOffset {
                            x: 0.0,
                            y: idx as f32 * crate::ui::ROW_H,
                        },
                    ),
                    None => Task::none(),
                }
    }

    pub(super) fn on_layer_delete(&mut self) -> Task<Message> {
                let i = self.active_tab;
                // Every selected layer (multi-select), or the anchor if none.
                let mut names = self.selected_layer_names(i);
                if names.is_empty() {
                    return Task::none();
                }
                // Layer "0" and the current layer can't be deleted — drop them
                // from the batch and note it.
                let current = self.tabs[i].scene.document.header.current_layer_name.clone();
                let before = names.len();
                names.retain(|n| n != "0" && *n != current);
                if names.len() < before {
                    self.command_line
                        .push_info("Layer \"0\" and the current layer can't be deleted — skipped.");
                }
                if names.is_empty() {
                    return Task::none();
                }
                // Total objects across the layers. Empty → delete straight away;
                // non-empty → warn first (deleting also removes those objects).
                let count = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter(|e| names.contains(&e.common().layer))
                    .count();
                if count == 0 {
                    self.push_undo_snapshot(i, "LAYER DELETE");
                    for name in &names {
                        self.tabs[i].scene.document.layers.remove(name);
                    }
                    self.tabs[i].dirty = true;
                    self.sync_layer_panel(i);
                } else {
                    self.layer_delete_pending = Some((names, count));
                    self.active_modal = Some(crate::app::ModalKind::LayerDeleteWarning);
                    self.modal_offset = iced::Vector::ZERO;
                    self.modal_resize = iced::Vector::ZERO;
                }
                Task::none()
    }

    /// User confirmed deleting non-empty layer(s): erase every object on them,
    /// then remove the layer records.
    pub(super) fn on_layer_delete_confirm(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.active_modal = None;
        self.modal_offset = iced::Vector::ZERO;
        self.modal_resize = iced::Vector::ZERO;
        let Some((names, _)) = self.layer_delete_pending.take() else {
            return Task::none();
        };
        self.push_undo_snapshot(i, "LAYER DELETE");
        let handles: Vec<acadrust::Handle> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| names.contains(&e.common().layer))
            .map(|e| e.common().handle)
            .collect();
        // Remove the layer records first so the lock guard in `erase_entities`
        // (which looks the layer up by name) can't block the objects.
        for name in &names {
            self.tabs[i].scene.document.layers.remove(name);
        }
        self.tabs[i].scene.erase_entities(&handles);
        self.tabs[i].dirty = true;
        self.sync_layer_panel(i);
        self.refresh_properties();
        Task::none()
    }

    /// Layers a Layer-manager row toggle (visibility / lock / freeze /
    /// transparency) should affect: the whole multi-selection when the clicked
    /// row is part of it, otherwise just the clicked row — so those toggles work
    /// in bulk like the color / linetype / lineweight edits already do (#236).
    pub(super) fn layer_row_action_targets(&self, i: usize, idx: usize) -> Vec<String> {
        let Some(clicked) = self.tabs[i].layers.layers.get(idx).map(|l| l.name.clone()) else {
            return Vec::new();
        };
        let selected = self.selected_layer_names(i);
        if selected.iter().any(|n| n == &clicked) {
            selected
        } else {
            vec![clicked]
        }
    }

    /// Layer names for the current Layer-manager selection — every row in the
    /// multi-selection, or the anchor row when the multi-set is empty. Bulk
    /// property edits and deletion act on these.
    pub(super) fn selected_layer_names(&self, i: usize) -> Vec<String> {
        let panel = &self.tabs[i].layers;
        let idxs: Vec<usize> = if panel.selected_multi.is_empty() {
            panel.selected.into_iter().collect()
        } else {
            panel.selected_multi.clone()
        };
        idxs.iter()
            .filter_map(|&x| panel.layers.get(x).map(|l| l.name.clone()))
            .collect()
    }

    /// Rebuild the Layer-manager panel + ribbon after a layer-table change.
    fn sync_layer_panel(&mut self, i: usize) {
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i].layers.sync_with_viewports(&doc_layers, vp_info);
        self.tabs[i].layers.selected = None;
        self.tabs[i].layers.selected_multi.clear();
        self.sync_ribbon_layers();
    }

    pub(super) fn on_layer_set_current(&mut self) -> Task<Message> {
                let i = self.active_tab;
                if let Some(idx) = self.tabs[i].layers.selected {
                    if let Some(layer) = self.tabs[i].layers.layers.get(idx) {
                        let name = layer.name.clone();
                        // Mirror the change into the document header (CLAYER) too,
                        // not just the per-tab default. Otherwise the no-selection
                        // ribbon refresh (e.g. after Esc) re-reads the stale header
                        // layer and the dropdown snaps back to it. See #93.
                        let handle = self.tabs[i]
                            .scene
                            .document
                            .layers
                            .get(&name)
                            .map(|l| l.handle)
                            .unwrap_or(acadrust::types::Handle::NULL);
                        self.tabs[i].scene.document.header.current_layer_name = name.clone();
                        self.tabs[i].scene.document.header.current_layer_handle = handle;
                        self.tabs[i].active_layer = name.clone();
                        self.tabs[i].layers.current_layer = name.clone();
                        self.tabs[i].dirty = true;
                        self.ribbon.active_layer = name;
                    }
                }
                Task::none()
    }

    pub(super) fn on_layer_rename_commit(&mut self) -> Task<Message> {
                let i = self.active_tab;
                let editing_idx = self.tabs[i].layers.editing.take();
                if let Some(idx) = editing_idx {
                    let new_name = self.tabs[i].layers.edit_buf.trim().to_string();
                    let old_name = self.tabs[i]
                        .layers
                        .layers
                        .get(idx)
                        .map(|l| l.name.clone())
                        .unwrap_or_default();
                    if !new_name.is_empty()
                        && new_name != old_name
                        && !self.tabs[i].scene.document.layers.contains(&new_name)
                    {
                        self.push_undo_snapshot(i, "LAYER RENAME");
                        // Keep the whole record (handle, color, linetype,
                        // lineweight, flags) and only change the name, so the
                        // renamed layer still has a valid handle and survives a
                        // DWG save (issue #67).
                        if let Some(mut nl) =
                            self.tabs[i].scene.document.layers.get(&old_name).cloned()
                        {
                            nl.name = new_name.clone();
                            if !nl.handle.is_valid() {
                                nl.handle = self.tabs[i].scene.document.allocate_handle();
                            }
                            let _ = self.tabs[i].scene.document.layers.add(nl);
                        }
                        self.tabs[i].scene.document.layers.remove(&old_name);
                        for e in self.tabs[i].scene.document.entities_mut() {
                            if e.as_entity().layer() == old_name {
                                e.as_entity_mut().set_layer(new_name.clone());
                            }
                        }
                        self.tabs[i].dirty = true;
                    }
                    let doc_layers = self.tabs[i].scene.document.layers.clone();
                    let vp_info = self.tabs[i].scene.viewport_list();
                    self.tabs[i]
                        .layers
                        .sync_with_viewports(&doc_layers, vp_info);
                    self.tabs[i].layers.edit_buf.clear();
                    self.sync_ribbon_layers();
                }
                Task::none()
    }

    pub(super) fn on_grip_menu_pick(&mut self, idx: usize) -> Task<Message> {
                let i = self.active_tab;
                let Some(popup) = self.grip_popup.take() else {
                    return Task::none();
                };
                self.grip_hover = None;
                let Some(item) = popup.items.get(idx).cloned() else {
                    return Task::none();
                };
                use crate::entities::traits::EntityTypeOps;
                use crate::scene::model::object::GripMenuAction;
                if matches!(
                    item.action,
                    GripMenuAction::Stretch
                        | GripMenuAction::MoveWithLeader
                        | GripMenuAction::MoveIndependent
                ) {
                    // Stretch / Move = grab this grip. Engage it so the next
                    // click places it (click-move-click) — same as picking the
                    // grip directly in the viewport. Without this the menu just
                    // closed and the grip never became hot (issue #48).
                    if let Some(g) = self.tabs[i]
                        .selected_grips
                        .iter()
                        .find(|g| g.id == popup.grip_id)
                    {
                        // "Move with Leader" drags the whole multileader; the
                        // others move just the picked grip.
                        let (grip_id, is_translate) =
                            if matches!(item.action, GripMenuAction::MoveWithLeader) {
                                (crate::entities::multileader::MOVE_ALL_GRIP, true)
                            } else {
                                (popup.grip_id, g.is_midpoint)
                            };
                        self.tabs[i].active_grip = Some(GripEdit {
                            handle: popup.handle,
                            grip_id,
                            is_translate,
                            origin_world: g.world,
                            last_world: g.world,
                        });
                    }
                    return Task::none();
                }
                // Actions that need a follow-up number stash a pending
                // state + prompt; the next typed value drives
                // `apply_grip_menu_value`.
                let prompt = self.tabs[i]
                    .scene
                    .document
                    .get_entity(popup.handle)
                    .and_then(|e| e.grip_menu_value_prompt(popup.grip_id, item.action));
                if let Some(label) = prompt {
                    self.grip_pending = Some(crate::app::GripPendingValue {
                        handle: popup.handle,
                        grip_id: popup.grip_id,
                        action: item.action,
                        label,
                    });
                    self.command_line.push_info(&format!("{label}:"));
                    return self.focus_cmd_input();
                }
                // Break at vertex replaces the entity with the split pieces —
                // structural, so it can't ride the in-place apply below.
                if matches!(item.action, GripMenuAction::BreakVertex) {
                    let pieces = match self.tabs[i].scene.document.get_entity(popup.handle) {
                        Some(acadrust::EntityType::LwPolyline(p)) => {
                            crate::entities::lwpolyline::break_at_vertex(p, popup.grip_id)
                        }
                        _ => None,
                    };
                    match pieces {
                        Some(pieces) => {
                            self.push_undo_snapshot(i, "BREAK");
                            self.tabs[i].scene.erase_entities(&[popup.handle]);
                            for e in pieces {
                                self.tabs[i].scene.add_entity(e);
                            }
                            self.tabs[i].dirty = true;
                            self.refresh_selected_grips();
                            self.refresh_properties();
                            self.command_line.push_output("Polyline broken at vertex.");
                        }
                        None => self
                            .command_line
                            .push_error("Cannot break at this vertex."),
                    }
                    return Task::none();
                }
                // One-shot action — apply immediately.
                self.push_undo_snapshot(i, item.label);
                // For Add Leader, the new arrow becomes the last grip; remember
                // its id so we can grab it for placement right after.
                let add_leader_gid = if matches!(item.action, GripMenuAction::AddLeader) {
                    self.tabs[i]
                        .scene
                        .document
                        .get_entity(popup.handle)
                        .and_then(|e| match e {
                            acadrust::EntityType::MultiLeader(ml) => Some(
                                ml.context
                                    .leader_roots
                                    .iter()
                                    .flat_map(|r| r.lines.iter())
                                    .map(|l| l.points.len())
                                    .sum::<usize>(),
                            ),
                            _ => None,
                        })
                } else {
                    None
                };
                if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(popup.handle) {
                    entity.apply_grip_menu(popup.grip_id, item.action);
                }
                // Menu actions reshape dimensions too — drop a stale baked *D
                // block so the edit is visible (no-op for non-dims). (#398)
                self.tabs[i]
                    .scene
                    .invalidate_dim_block_recorded(popup.handle);
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].dirty = true;
                self.refresh_selected_grips();
                self.refresh_properties();
                // Grab the new arrow so it follows the cursor (click places it,
                // Esc removes it).
                if let Some(new_gid) = add_leader_gid {
                    if let Some(g) = self.tabs[i].selected_grips.iter().find(|g| g.id == new_gid) {
                        self.tabs[i].active_grip = Some(GripEdit {
                            handle: popup.handle,
                            grip_id: new_gid,
                            is_translate: false,
                            origin_world: g.world,
                            last_world: g.world,
                        });
                        self.grip_add_provisional = Some((popup.handle, new_gid));
                    }
                }
                // Convert to Arc enters placement: the segment grip goes hot
                // in Absolute mode, so the arc re-fits through the cursor as
                // it moves and the next click seats it (#339).
                if matches!(item.action, GripMenuAction::ConvertToArc) {
                    if let Some(g) = self.tabs[i]
                        .selected_grips
                        .iter()
                        .find(|g| g.id == popup.grip_id)
                    {
                        self.tabs[i].active_grip = Some(GripEdit {
                            handle: popup.handle,
                            grip_id: popup.grip_id,
                            is_translate: false,
                            origin_world: g.world,
                            last_world: g.world,
                        });
                    }
                }
                Task::none()
    }

    pub(super) fn on_paste_shortcut(&mut self) -> Task<Message> {
                if self.mtext_editor.is_some() {
                    // Web reads via the browser's async Clipboard API (iced's
                    // sync clipboard read returns nothing there); native uses
                    // iced's clipboard.
                    #[cfg(target_arch = "wasm32")]
                    return Task::perform(
                        crate::sys::read_clipboard_text(),
                        Message::MTextPasteClip,
                    );
                    #[cfg(not(target_arch = "wasm32"))]
                    return iced::clipboard::read().map(Message::MTextPasteClip);
                }
                if self.text_inline.is_some() {
                    // Web: the iced text_input can't reach the async clipboard,
                    // so paste it ourselves. Native: the focused text_input
                    // already handled Ctrl+V — doing it here would duplicate.
                    #[cfg(target_arch = "wasm32")]
                    return Task::perform(
                        crate::sys::read_clipboard_text(),
                        Message::TextInlinePasteClip,
                    );
                    #[cfg(not(target_arch = "wasm32"))]
                    return Task::none();
                }
                Task::done(Message::Command("PASTECLIP".to_string()))
    }

    pub(super) fn on_qselect_open(&mut self) -> Task<Message> {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                // Seed the type filter from the first selected entity so a
                // right-click → Quick Select on a known object opens the
                // panel pre-tuned to that entity's type. Property defaults
                // to "(Any property)" so the user immediately picks what
                // they want to compare.
                let mut type_filter: Option<String> = None;
                if let Some(&h) = self.tabs[i].scene.selected.iter().next() {
                    if let Some(e) = self.tabs[i].scene.document.get_entity(h) {
                        use crate::entities::traits::entity_type_name;
                        type_filter = Some(entity_type_name(e).to_string());
                    }
                }
                self.qselect = Some(crate::app::QSelectState {
                    type_filter,
                    property: None,
                    operator: crate::app::QSelectOp::Eq,
                    value: String::new(),
                    append: false,
                });
                Task::none()
    }

    pub(super) fn on_ribbon_layer_changed(&mut self, layer: String) -> Task<Message> {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    // No selection — change the creation default. Persist
                    // into the tab's header (CLAYER) so it survives a tab
                    // switch and rides the next save. #21.
                    let handle = self.tabs[i]
                        .scene
                        .document
                        .layers
                        .get(&layer)
                        .map(|l| l.handle)
                        .unwrap_or(acadrust::types::Handle::NULL);
                    self.tabs[i].scene.document.header.current_layer_name = layer.clone();
                    self.tabs[i].scene.document.header.current_layer_handle = handle;
                    self.tabs[i].active_layer = layer.clone();
                    self.tabs[i].layers.current_layer = layer.clone();
                    self.tabs[i].dirty = true;
                    self.ribbon.active_layer = layer;
                } else {
                    // Apply to the selection; leave the creation default alone
                    // ("Make current" is a separate action).
                    self.push_undo_snapshot(i, "CHPROP");
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_common_prop(entity, "layer", &layer);
                        }
                    }
                    // Layer drives by-layer colour/linetype/lineweight, which are
                    // baked into the cached wire geometry — re-tessellate so the
                    // change shows immediately (issue #231 class).
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty = true;
                    self.ribbon.active_layer = layer;
                    self.refresh_properties();
                }
                Task::none()
    }

    pub(super) fn on_ribbon_color_changed(&mut self, color: AcadColor) -> Task<Message> {
                let i = self.active_tab;
                self.ribbon.prop_color_palette_open = false;
                self.ribbon.close_dropdown();
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    // Persist the new default into the tab's header so it
                    // round-trips through tab switches and writes back on
                    // save (CECOLOR). #21.
                    self.tabs[i].scene.document.header.current_entity_color = color;
                    self.tabs[i].dirty = true;
                    self.ribbon.active_color = color;
                } else {
                    self.push_undo_snapshot(i, "CHPROP");
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_color(entity, color);
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty = true;
                    self.ribbon.active_color = color;
                    self.refresh_properties();
                }
                Task::none()
    }

    pub(super) fn on_ribbon_linetype_changed(&mut self, lt: String) -> Task<Message> {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    // Persist into the tab's header (CELTYPE). Resolve to a
                    // handle when the name matches a line_types entry so the
                    // handle-based lookup stays in sync. #21.
                    let handle = self.tabs[i]
                        .scene
                        .document
                        .line_types
                        .iter()
                        .find(|x| x.name.eq_ignore_ascii_case(&lt))
                        .map(|x| x.handle)
                        .unwrap_or(acadrust::types::Handle::NULL);
                    self.tabs[i].scene.document.header.current_linetype_name = lt.clone();
                    self.tabs[i].scene.document.header.current_linetype_handle = handle;
                    self.tabs[i].dirty = true;
                    self.ribbon.active_linetype = lt;
                } else {
                    self.push_undo_snapshot(i, "CHPROP");
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_common_prop(entity, "linetype", &lt);
                        }
                    }
                    // Linetype is baked into the cached wire geometry —
                    // re-tessellate so the dashed/solid look updates immediately
                    // (issue #231 class).
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty = true;
                    self.ribbon.active_linetype = lt;
                    self.refresh_properties();
                }
                Task::none()
    }

    pub(super) fn on_ribbon_style_changed(&mut self, key: crate::modules::StyleKey, name: String) -> Task<Message> {
                use crate::modules::StyleKey;
                self.ribbon.close_dropdown();
                match key {
                    StyleKey::TextStyle => {
                        self.ribbon.active_text_style = name.clone();
                        let i = self.active_tab;
                        let found = self.tabs[i]
                            .scene
                            .document
                            .text_styles
                            .iter()
                            .find(|s| s.name == name)
                            .map(|ts| ts.handle);
                        if let Some(h) = found {
                            self.tabs[i].scene.document.header.current_text_style_handle = h;
                            self.tabs[i].scene.document.header.current_text_style_name = name;
                        }
                    }
                    StyleKey::DimStyle => {
                        self.ribbon.active_dim_style = name.clone();
                        let i = self.active_tab;
                        let found = self.tabs[i]
                            .scene
                            .document
                            .dim_styles
                            .get(&name)
                            .map(|ds| ds.handle);
                        if let Some(h) = found {
                            self.tabs[i].scene.document.header.current_dimstyle_handle = h;
                            self.tabs[i].scene.document.header.current_dimstyle_name = name;
                        }
                    }
                    StyleKey::MLeaderStyle => {
                        self.ribbon.active_mleader_style = name.clone();
                        let i = self.active_tab;
                        self.tabs[i].active_mleader_style = name;
                    }
                    StyleKey::TableStyle => {
                        self.ribbon.active_table_style = name;
                    }
                }
                Task::none()
    }

    pub(super) fn on_prop_hatch_pattern_changed(&mut self, name: String) -> Task<Message> {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    use crate::scene::model::hatch_patterns;
                    if let Some(entry) = hatch_patterns::find(&name) {
                        self.push_undo_snapshot(i, "HATCHEDIT");
                        for &handle in &handles {
                            if let Some(acadrust::EntityType::Hatch(dxf)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                let old_origin = dxf.pattern.lines.first().map(|line| {
                                    (line.base_point.x, line.base_point.y)
                                });
                                let mut pattern = hatch_patterns::build_dxf_pattern(entry);
                                // Stored pattern lines are final world-space
                                // geometry. Preserve the selected hatch's scale,
                                // angle and origin when replacing the catalog
                                // pattern.
                                crate::entities::hatch::scale_pattern_geometry(
                                    &mut pattern,
                                    dxf.pattern_scale,
                                );
                                crate::entities::hatch::rotate_pattern_geometry(
                                    &mut pattern,
                                    dxf.pattern_angle,
                                );
                                if let (Some((old_x, old_y)), Some(new_origin)) =
                                    (old_origin, pattern.lines.first())
                                {
                                    let dx = old_x - new_origin.base_point.x;
                                    let dy = old_y - new_origin.base_point.y;
                                    crate::entities::hatch::translate_pattern_geometry(
                                        &mut pattern,
                                        dx,
                                        dy,
                                    );
                                }
                                dxf.pattern = pattern;
                                dxf.is_solid = matches!(
                                    entry.gpu,
                                    crate::scene::model::hatch_model::HatchPattern::Solid
                                );
                            }
                            if let Some(model) = self.tabs[i].scene.hatches.get_mut(&handle) {
                                model.pattern = entry.gpu.clone();
                                model.name = name.clone();
                            }
                        }
                        self.invalidate_property_targets(i, &handles);
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                    }
                }
                Task::none()
    }

    pub(super) fn on_prop_geom_choice_changed(&mut self, field: &'static str, value: String) -> Task<Message> {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    if field == "vp_ucs_name" {
                        // Resolve UCS name → cloned data, then mutate viewports.
                        let ucs_data = self.tabs[i]
                            .scene
                            .document
                            .ucss
                            .iter()
                            .find(|u| u.name == value)
                            .cloned();
                        if let Some(ucs) = ucs_data {
                            for handle in &handles {
                                if let Some(acadrust::EntityType::Viewport(vp)) =
                                    self.tabs[i].scene.document.get_entity_mut(*handle)
                                {
                                    vp.ucs_handle = ucs.handle;
                                    vp.ucs_origin = ucs.origin.clone();
                                    vp.ucs_x_axis = ucs.x_axis.clone();
                                    vp.ucs_y_axis = ucs.y_axis.clone();
                                }
                            }
                        }
                    } else if field == "vp_named_view" {
                        // Assign a named view to viewport(s): copy camera parameters.
                        let view_data = self.tabs[i]
                            .scene
                            .document
                            .views
                            .iter()
                            .find(|v| v.name == value)
                            .cloned();
                        if let Some(view) = view_data {
                            for handle in &handles {
                                if let Some(acadrust::EntityType::Viewport(vp)) =
                                    self.tabs[i].scene.document.get_entity_mut(*handle)
                                {
                                    vp.view_target = view.target.clone();
                                    vp.view_direction = view.direction.clone();
                                    if view.height > 0.0 {
                                        vp.view_height = view.height;
                                    }
                                }
                            }
                            self.tabs[i].scene.camera_generation += 1;
                        }
                    } else if matches!(
                        field,
                        "mleader_style"
                            | "text_style_handle"
                            | "arrowhead_handle"
                            | "line_type_handle"
                    ) {
                        // Resolve a picked name back to the handle the MLEADER
                        // stores. The style/text-style rows keep their existing
                        // handle on a failed lookup; the arrowhead/linetype rows
                        // take the resolved value directly (None = the default
                        // "Closed filled" / "ByBlock" option).
                        let doc = &self.tabs[i].scene.document;
                        let resolved: Option<acadrust::Handle> = match field {
                            "mleader_style" => doc.objects.iter().find_map(|(h, o)| match o {
                                acadrust::objects::ObjectType::MultiLeaderStyle(s)
                                    if s.name == value =>
                                {
                                    Some(*h)
                                }
                                _ => None,
                            }),
                            "text_style_handle" => doc
                                .text_styles
                                .iter()
                                .find(|s| s.name == value)
                                .map(|s| s.handle),
                            "arrowhead_handle" => {
                                if value == "Closed filled" {
                                    None
                                } else {
                                    doc.block_records
                                        .iter()
                                        .find(|b| b.name == value)
                                        .map(|b| b.handle)
                                }
                            }
                            "line_type_handle" => {
                                if value == "ByBlock" {
                                    None
                                } else {
                                    doc.line_types
                                        .iter()
                                        .find(|l| l.name == value)
                                        .map(|l| l.handle)
                                }
                            }
                            _ => None,
                        };
                        for &handle in &handles {
                            if self.tabs[i].scene.is_layer_locked(handle) {
                                continue;
                            }
                            if let Some(acadrust::EntityType::MultiLeader(ml)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                match field {
                                    "mleader_style" => {
                                        if let Some(h) = resolved {
                                            ml.style_handle = Some(h);
                                        }
                                    }
                                    "text_style_handle" => {
                                        if let Some(h) = resolved {
                                            ml.text_style_handle = Some(h);
                                        }
                                    }
                                    "arrowhead_handle" => ml.arrowhead_handle = resolved,
                                    "line_type_handle" => ml.line_type_handle = resolved,
                                    _ => {}
                                }
                            }
                        }
                    } else if matches!(field, "arrow_block" | "dim_line_lw" | "text_pos_vert") {
                        // Leader dim-var overrides picked from a dropdown. The
                        // arrow block resolves a friendly arrowhead name to a
                        // block handle ("Closed filled" clears the override →
                        // reverts to the style); the other two map a label to
                        // the DIMLWD / DIMTAD enum value.
                        use crate::entities::dim_override as dov;
                        use acadrust::xdata::XDataValue;
                        let arrow_h: Option<acadrust::Handle> =
                            if field == "arrow_block" && value != "Closed filled" {
                                self.tabs[i]
                                    .scene
                                    .document
                                    .block_records
                                    .iter()
                                    .find(|b| {
                                        crate::app::properties::arrowhead_label(&b.name) == value
                                    })
                                    .map(|b| b.handle)
                            } else {
                                None
                            };
                        for &handle in &handles {
                            if self.tabs[i].scene.is_layer_locked(handle) {
                                continue;
                            }
                            // Only leaders carry these rows; skip anything else so
                            // a multi-type selection can't get stray overrides.
                            if !matches!(
                                self.tabs[i].scene.document.get_entity(handle),
                                Some(acadrust::EntityType::Leader(_))
                            ) {
                                continue;
                            }
                            let doc = &mut self.tabs[i].scene.document;
                            match field {
                                "dim_line_lw" => dov::set(
                                    doc,
                                    handle,
                                    dov::DIMLWD,
                                    Some(XDataValue::Integer16(
                                        crate::app::properties::dim_lineweight_from_label(&value),
                                    )),
                                ),
                                "text_pos_vert" => dov::set(
                                    doc,
                                    handle,
                                    dov::DIMTAD,
                                    Some(XDataValue::Integer16(
                                        crate::app::properties::dimtad_from_label(&value),
                                    )),
                                ),
                                "arrow_block" => {
                                    // "Closed filled" is an explicit override to
                                    // the null-handle default arrow (not a clear),
                                    // so the pick sticks even when the style's
                                    // arrow differs.
                                    if value == "Closed filled" {
                                        dov::set(
                                            doc,
                                            handle,
                                            dov::DIMLDRBLK,
                                            Some(XDataValue::Handle(acadrust::Handle::NULL)),
                                        );
                                    } else if let Some(h) = arrow_h {
                                        dov::set(
                                            doc,
                                            handle,
                                            dov::DIMLDRBLK,
                                            Some(XDataValue::Handle(h)),
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if field == "block" {
                        // Name dropdown on a block reference: re-point the
                        // selected inserts to the picked definition. A stale
                        // typed value in the text buffer would mask the pick,
                        // so drop it; the pick also closes the list.
                        self.tabs[i].properties.edit_buf.remove("block");
                        self.tabs[i].properties.edit_choice_open = false;
                        let canon = self.tabs[i]
                            .scene
                            .document
                            .block_records
                            .get(&value)
                            .map(|br| br.name.clone());
                        if let Some(canon) = canon {
                            let mut changed = false;
                            for &handle in &handles {
                                if self.tabs[i].scene.is_layer_locked(handle) {
                                    continue;
                                }
                                if let Some(acadrust::EntityType::Insert(ins)) =
                                    self.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    if ins.block_name != canon {
                                        ins.block_name = canon.clone();
                                        changed = true;
                                    }
                                }
                            }
                            if changed {
                                // The picked block may not be in the defn cache
                                // yet — rebuild block definitions too.
                                self.tabs[i].scene.bump_geometry();
                            }
                        }
                    } else if field == "plot_style" {
                        // Named plot-style pick: ByLayer / ByBlock clear the
                        // handle; a named style resolves through the drawing's
                        // ACAD_PLOTSTYLENAME dictionary to its placeholder handle.
                        let dict_h =
                            self.tabs[i].scene.document.header.acad_plotstylename_dict_handle;
                        let ph: Option<acadrust::Handle> =
                            crate::scene::annotative::as_dict(&self.tabs[i].scene.document, dict_h)
                                .and_then(|d| {
                                    d.entries
                                        .iter()
                                        .find(|(n, _)| *n == value)
                                        .map(|(_, h)| *h)
                                });
                        for &handle in &handles {
                            if self.tabs[i].scene.is_layer_locked(handle) {
                                continue;
                            }
                            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                let common = entity.common_mut();
                                match value.as_str() {
                                    "ByLayer" => {
                                        common.plotstyle_flags = 0;
                                        common.plotstyle_handle = None;
                                    }
                                    "ByBlock" => {
                                        common.plotstyle_flags = 1;
                                        common.plotstyle_handle = None;
                                    }
                                    _ => {
                                        if let Some(h) = ph {
                                            common.plotstyle_flags = 3;
                                            common.plotstyle_handle = Some(h);
                                        }
                                    }
                                }
                            }
                        }
                    } else if field == "material" {
                        // Material source: ByLayer / ByBlock clear the handle; a
                        // named material sets flag 3 + its handle (resolved here
                        // because the update loop holds the document).
                        let mat_handle: Option<acadrust::Handle> = self.tabs[i]
                            .scene
                            .document
                            .objects
                            .iter()
                            .find_map(|(h, o)| match o {
                                acadrust::objects::ObjectType::Material(m) if m.name == value => {
                                    Some(*h)
                                }
                                _ => None,
                            });
                        for &handle in &handles {
                            if self.tabs[i].scene.is_layer_locked(handle) {
                                continue;
                            }
                            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                let common = entity.common_mut();
                                match value.as_str() {
                                    "ByLayer" => {
                                        common.material_flags = 0;
                                        common.material_handle = None;
                                    }
                                    "ByBlock" => {
                                        common.material_flags = 1;
                                        common.material_handle = None;
                                    }
                                    _ => {
                                        if let Some(h) = mat_handle {
                                            common.material_flags = 3;
                                            common.material_handle = Some(h);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        for &handle in &handles {
                            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                crate::scene::view::dispatch::apply_geom_prop(entity, field, &value);
                            }
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
    }

    pub(super) fn on_prop_geom_commit(&mut self, field: &'static str) -> Task<Message> {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    if let Some(raw_val) = self.tabs[i].properties.edit_buf.remove(field) {
                        // Block names are free-form text — a name like "10-5"
                        // must not be arithmetic-evaluated.
                        let val = if field == "block" {
                            raw_val
                        } else {
                            crate::app::expr_eval::eval_to_string(&raw_val)
                        };
                        self.push_undo_snapshot(i, "CHPROP");
                        if field == "block" {
                            // Name row on a block reference: an existing name
                            // re-points the selected inserts; a new one renames
                            // the definition they share. Commit closes the list.
                            self.tabs[i].properties.edit_choice_open = false;
                            self.apply_block_name_commit(i, &handles, val.trim());
                        } else if field == "frozen_layers" {
                            // Resolve layer names → handles, then apply to viewports.
                            let layer_handles: Vec<acadrust::Handle> = val
                                .split(',')
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
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
                            for &handle in &handles {
                                if let Some(acadrust::EntityType::Viewport(vp)) =
                                    self.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    vp.frozen_layers = layer_handles.clone();
                                }
                            }
                        } else {
                            // Per-vertex geometry edits (vertex_x/y, widths) target
                            // the vertex the Current Vertex stepper is on. (polyline)
                            crate::scene::view::dispatch::set_prop_current_vertex(
                                self.tabs[i].properties.prop_vertex,
                            );
                            for &handle in &handles {
                                // Skip objects on a locked layer.
                                if self.tabs[i].scene.is_layer_locked(handle) {
                                    continue;
                                }
                                match field {
                                    "hyperlink" => {
                                        // Stored in the standard PE_URL XDATA
                                        // record; an empty value clears it.
                                        let vals = if val.trim().is_empty() {
                                            None
                                        } else {
                                            Some(vec![acadrust::xdata::XDataValue::String(
                                                val.clone(),
                                            )])
                                        };
                                        crate::scene::view::dispatch::set_entity_xdata(
                                            &mut self.tabs[i].scene.document,
                                            handle,
                                            "PE_URL",
                                            vals,
                                        );
                                    }
                                    "arrow_size" | "text_offset" | "dim_scale_overall" => {
                                        // Real-valued leader dim-var overrides
                                        // (ACAD_DSTYLE). A blank value clears the
                                        // override → reverts to the style; an
                                        // unparseable entry is ignored so a typo
                                        // can't silently wipe a stored override.
                                        use crate::entities::dim_override as dov;
                                        let code = match field {
                                            "arrow_size" => dov::DIMASZ,
                                            "text_offset" => dov::DIMGAP,
                                            _ => dov::DIMSCALE,
                                        };
                                        let trimmed = val.trim();
                                        if trimmed.is_empty() {
                                            dov::set(
                                                &mut self.tabs[i].scene.document,
                                                handle,
                                                code,
                                                None,
                                            );
                                        } else if let Ok(n) = trimmed.parse::<f64>() {
                                            dov::set(
                                                &mut self.tabs[i].scene.document,
                                                handle,
                                                code,
                                                Some(acadrust::xdata::XDataValue::Real(n)),
                                            );
                                        }
                                    }
                                    "linetype_scale" | "transparency" | "thickness" => {
                                        // Thickness (DXF 39 extrusion) is a General-
                                        // group common prop, not a geometry-group one:
                                        // route it to apply_common_prop (which handles
                                        // it via set_entity_thickness). apply_geom_prop
                                        // ignores it, so the edit would otherwise be a
                                        // silent no-op and revert to the old value.
                                        if let Some(entity) =
                                            self.tabs[i].scene.document.get_entity_mut(handle)
                                        {
                                            crate::scene::view::dispatch::apply_common_prop(
                                                entity, field, &val,
                                            );
                                        }
                                    }
                                    _ => {
                                        if let Some(entity) =
                                            self.tabs[i].scene.document.get_entity_mut(handle)
                                        {
                                            crate::scene::view::dispatch::apply_geom_prop(
                                                entity, field, &val,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        self.invalidate_property_targets(i, &handles);
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                    }
                }
                Task::none()
    }

    /// Properties-panel Name commit for block references. A `new` matching an
    /// existing regular block re-points the selected inserts to it; otherwise
    /// the definition the (single-block) selection references is renamed and
    /// every insert of it follows. Anonymous/xref definitions never get here —
    /// their Name row stays read-only.
    fn apply_block_name_commit(&mut self, i: usize, handles: &[acadrust::Handle], new: &str) {
        if new.is_empty() {
            return;
        }
        // Existing target → swap the selected references to it. Anonymous,
        // xref and xref-dependent definitions are not valid targets.
        let target = self.tabs[i].scene.document.block_records.get(new).map(|br| {
            (
                br.name.clone(),
                br.is_anonymous() || br.flags.is_xref || br.name.contains('|'),
            )
        });
        if let Some((canon, protected)) = target {
            if protected {
                return;
            }
            let mut changed = false;
            for &handle in handles {
                if self.tabs[i].scene.is_layer_locked(handle) {
                    continue;
                }
                if let Some(acadrust::EntityType::Insert(ins)) =
                    self.tabs[i].scene.document.get_entity_mut(handle)
                {
                    if ins.block_name != canon {
                        ins.block_name = canon.clone();
                        changed = true;
                    }
                }
            }
            if changed {
                self.tabs[i].scene.bump_geometry();
            }
            return;
        }
        // New name → rename. Only when every selected insert references the
        // same definition; a mixed selection makes the rename target ambiguous.
        let mut old: Option<String> = None;
        for &handle in handles {
            if let Some(acadrust::EntityType::Insert(ins)) =
                self.tabs[i].scene.document.get_entity(handle)
            {
                match &old {
                    None => old = Some(ins.block_name.clone()),
                    Some(o) if o.eq_ignore_ascii_case(&ins.block_name) => {}
                    _ => return,
                }
            }
        }
        if let Some(old) = old {
            self.tabs[i].scene.rename_block(&old, new);
        }
    }

    /// Commit an edited block-attribute value from the Properties panel.
    /// Writes the new value into the matching attribute (by tag) of every
    /// selected INSERT, then re-tessellates so the attribute text repaints.
    /// The value is stored verbatim (attribute values are free-form text, so
    /// it is not run through the expression evaluator like numeric fields).
    pub(super) fn on_prop_attr_commit(&mut self, tag: String) -> Task<Message> {
        let i = self.active_tab;
        let key = crate::ui::properties::attr_edit_key(&tag);
        let handles = self.property_target_handles(i);
        if handles.is_empty() {
            return Task::none();
        }
        let Some(val) = self.tabs[i].properties.edit_buf.remove(&key) else {
            return Task::none();
        };
        self.push_undo_snapshot(i, "ATTEDIT");
        for &handle in &handles {
            if let Some(acadrust::EntityType::Insert(ins)) =
                self.tabs[i].scene.document.get_entity_mut(handle)
            {
                if let Some(attr) = ins.attributes.iter_mut().find(|a| a.tag == tag) {
                    attr.set_value(val.clone());
                }
            }
        }
        self.invalidate_property_targets(i, &handles);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        Task::none()
    }

    /// Open the attribute editor dialog for the given INSERT, loading a working
    /// copy of its attribute values. No-op (with a hint) when the block has no
    /// attributes. Entry points: double-clicking such a block, or ATTEDIT.
    pub(crate) fn open_attribute_editor(&mut self, handle: acadrust::Handle) {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        // Ok((block, rows)) to open; Err(msg) to report and stay closed. The
        // borrow of `doc` ends with this match, before any `self` mutation.
        let result = match doc.get_entity(handle) {
            Some(acadrust::EntityType::Insert(ins)) if !ins.attributes.is_empty() => {
                // The prompt text lives on the block's ATTDEFs, not on the
                // attribute instances — map tag → prompt from the definition.
                let prompts = block_attr_prompts(doc, &ins.block_name);
                let rows = ins
                    .attributes
                    .iter()
                    .map(|a| attr_row_from_entity(a, &prompts))
                    .collect::<Vec<_>>();
                Ok((ins.block_name.clone(), rows))
            }
            Some(acadrust::EntityType::Insert(_)) => {
                Err("ATTEDIT  This block has no attributes.")
            }
            _ => Err("ATTEDIT  Select a block with attributes."),
        };
        match result {
            Ok((block, rows)) => {
                self.attr_editor_block = block;
                self.attr_editor_rows = rows;
                self.attr_editor_selected = 0;
                self.attr_editor_tab = AttrTab::Attribute;
                self.attr_editor_handle = Some(handle);
                self.active_modal = Some(crate::app::ModalKind::AttributeEditor);
                self.modal_offset = iced::Vector::ZERO;
                self.modal_resize = iced::Vector::ZERO;
            }
            Err(msg) => self.command_line.push_error(msg),
        }
    }

    /// The currently-selected editor row, for the Text Options / Properties
    /// edit handlers (which all act on that one attribute).
    pub(super) fn attr_row_selected_mut(&mut self) -> Option<&mut AttrRow> {
        let sel = self.attr_editor_selected;
        self.attr_editor_rows.get_mut(sel)
    }

    /// Close the attribute editor without applying, if it is open. Used when
    /// the tab it belongs to closes or the active tab switches — its handle is
    /// document-local and must not outlive its tab.
    pub(super) fn cancel_attr_editor(&mut self) {
        if self.active_modal == Some(crate::app::ModalKind::AttributeEditor) {
            self.active_modal = None;
            self.modal_offset = iced::Vector::ZERO;
            self.modal_resize = iced::Vector::ZERO;
        }
        self.attr_editor_handle = None;
        self.attr_editor_block.clear();
        self.attr_editor_rows.clear();
        self.attr_editor_selected = 0;
        self.attr_editor_tab = AttrTab::Attribute;
    }

    /// Commit every edited attribute (value, text options, common properties)
    /// back to the block, keeping the dialog open (Apply). Closing is the frame
    /// ✕ (`CloseModal`), which discards any un-applied edits — matching the
    /// other modal windows. Applied positionally so blocks with duplicate tags
    /// stay correct; guarded on block name + count so an edit can never land on
    /// a different entity if the selection changed under the open dialog.
    pub(super) fn on_attr_editor_apply(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(handle) = self.attr_editor_handle else {
            return Task::none();
        };
        // The block's layer may have been locked while the editor was open —
        // refuse to write attributes to a locked-layer block.
        if let Some(layer) = self.tabs[i].scene.locked_layer_name(handle) {
            self.command_line.push_info(&format!(
                "Object is on locked layer \"{layer}\" — unlock the layer to edit its attributes."
            ));
            return Task::none();
        }
        // Snapshot the working copy so the document can be mutated while the
        // dialog's rows stay put (it remains open for further edits).
        let rows = self.attr_editor_rows.clone();
        let block = self.attr_editor_block.clone();

        self.push_undo_snapshot(i, "ATTEDIT");
        let mut changed = false;
        if let Some(acadrust::EntityType::Insert(ins)) =
            self.tabs[i].scene.document.get_entity_mut(handle)
        {
            if ins.block_name == block && rows.len() == ins.attributes.len() {
                for (attr, row) in ins.attributes.iter_mut().zip(rows.iter()) {
                    changed |= apply_attr_row(attr, row);
                }
            }
        }
        if changed {
            self.invalidate_property_targets(i, &[handle]);
            self.tabs[i].dirty = true;
        } else {
            // Nothing changed — drop the snapshot pushed a moment ago.
            self.discard_last_undo_entry(i);
        }
        self.refresh_properties();
        Task::none()
    }
}

/// Build the editor's working copy of one attribute. Angles are shown in
/// degrees; numeric fields become strings so the user can type freely.
fn attr_row_from_entity(
    a: &acadrust::entities::AttributeEntity,
    prompts: &rustc_hash::FxHashMap<String, String>,
) -> AttrRow {
    let fmt = |v: f64| format!("{v}");
    AttrRow {
        tag: a.tag.clone(),
        prompt: prompts.get(&a.tag).cloned().unwrap_or_default(),
        value: a.get_value().to_string(),
        text_style: a.text_style.clone(),
        height: fmt(a.height),
        rotation: fmt(a.rotation.to_degrees()),
        width_factor: fmt(a.width_factor),
        oblique: fmt(a.oblique_angle.to_degrees()),
        h_align: a.horizontal_alignment,
        v_align: a.vertical_alignment,
        backwards: a.text_generation_flags & 0x2 != 0,
        upside_down: a.text_generation_flags & 0x4 != 0,
        layer: a.common.layer.clone(),
        color: a.common.color,
        linetype: a.common.linetype.clone(),
        line_weight: a.common.line_weight,
    }
}

/// Write one edited row back onto its attribute; returns whether anything
/// changed. Numeric fields are parsed (angles from degrees); an unparseable
/// field is left as-is.
fn apply_attr_row(a: &mut acadrust::entities::AttributeEntity, row: &AttrRow) -> bool {
    let mut ch = false;
    if a.get_value() != row.value {
        a.set_value(row.value.clone());
        ch = true;
    }
    if a.text_style != row.text_style {
        a.text_style = row.text_style.clone();
        ch = true;
    }
    if let Ok(h) = row.height.trim().parse::<f64>() {
        if a.height != h {
            a.height = h;
            ch = true;
        }
    }
    if let Ok(deg) = row.rotation.trim().parse::<f64>() {
        let rad = deg.to_radians();
        if (a.rotation - rad).abs() > 1e-12 {
            a.rotation = rad;
            ch = true;
        }
    }
    if let Ok(w) = row.width_factor.trim().parse::<f64>() {
        if a.width_factor != w {
            a.width_factor = w;
            ch = true;
        }
    }
    if let Ok(deg) = row.oblique.trim().parse::<f64>() {
        let rad = deg.to_radians();
        if (a.oblique_angle - rad).abs() > 1e-12 {
            a.oblique_angle = rad;
            ch = true;
        }
    }
    if a.horizontal_alignment != row.h_align {
        a.horizontal_alignment = row.h_align;
        ch = true;
    }
    if a.vertical_alignment != row.v_align {
        a.vertical_alignment = row.v_align;
        ch = true;
    }
    // Text generation flags: bit 0x2 = backwards, bit 0x4 = upside down.
    let new_flags = (a.text_generation_flags & !0x6)
        | if row.backwards { 0x2 } else { 0 }
        | if row.upside_down { 0x4 } else { 0 };
    if new_flags != a.text_generation_flags {
        a.text_generation_flags = new_flags;
        ch = true;
    }
    if a.common.layer != row.layer {
        a.common.layer = row.layer.clone();
        ch = true;
    }
    if a.common.color != row.color {
        a.common.color = row.color;
        ch = true;
    }
    if a.common.linetype != row.linetype {
        a.common.linetype = row.linetype.clone();
        ch = true;
    }
    if a.common.line_weight != row.line_weight {
        a.common.line_weight = row.line_weight;
        ch = true;
    }
    ch
}

/// Map each attribute tag to the prompt text declared on the block's matching
/// ATTDEF. Attribute instances (ATTRIB) carry only tag + value; the prompt is
/// defined once on the block definition. Tags with no definition are absent.
fn block_attr_prompts(
    doc: &acadrust::CadDocument,
    block_name: &str,
) -> rustc_hash::FxHashMap<String, String> {
    let mut map = rustc_hash::FxHashMap::default();
    if let Some(br) = doc.block_records.get(block_name) {
        for &eh in &br.entity_handles {
            if let Some(acadrust::EntityType::AttributeDefinition(ad)) = doc.get_entity(eh) {
                map.insert(ad.tag.clone(), ad.prompt.clone());
            }
        }
    }
    map
}
