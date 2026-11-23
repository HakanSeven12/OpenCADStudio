//! `command` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{parse_coord, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis, CoordKind};
use crate::app::TextEntryMode;
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugin::v4_support;
use crate::scene::model::object::{GripApply, PropValue};
use crate::scene::pick::grip::{
    find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit, GripEditMode,
};
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::window::attribute_editor::{AttrRow, AttrTab};
use crate::ui::PropertiesPanel;
use document::types::Color as AcadColor;
use document::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};

/// Return `(new vertex grip id, old vertex count)` for polyline Add Vertex.
/// LWPolyline segment grips live after the vertex grips, so translate their id

mod tabs;
mod submit;
mod layer_manager;
mod grip_edit;
mod clipboard;
mod qselect_cmd;
mod properties;

pub(crate) use self::tabs::*;
pub(crate) use self::submit::*;
pub(crate) use self::layer_manager::*;
pub(crate) use self::grip_edit::*;
pub(crate) use self::clipboard::*;
pub(crate) use self::qselect_cmd::*;
pub(crate) use self::properties::*;

impl OpenCADStudio {
    pub(super) fn on_command_append_char(&mut self, s: String) -> Task<Message> {
        if let Some(task) = self.shift_glyph(&s) {
            return task;
        }
        // While the MText preview is up, typed glyphs edit it directly.
        if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
            if s.chars().all(|c| !c.is_control()) {
                self.mtext_type(&s);
            }
            return Task::none();
        }
        // A dialog owns the keyboard: typing while one is open (an open list
        // in it included) must not land in the command line behind it.
        if self.active_modal.is_some()
            || self.qselect.is_some()
            || self.filter_dialog.is_some()
            || self.layer_import.is_some()
        {
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
            // In dynamic point entry, `@` and `#` are coordinate-mode
            // switches rather than command-line text. Consuming them
            // here keeps the prefix out of the command buffer while
            // the following digits continue into the dynamic fields.
            if matches!(s.as_str(), "@" | "#")
                && self.command_line.input.is_empty()
                && self.dyn_input
                && self.dyn_has_coordinate_fields(i)
            {
                self.dyn_set_coordinate_mode(s == "#");
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
                        || matches!(
                            c,
                            '.' | '-' | '+' | '*' | '/' | '^' | '%' | '(' | ')' | '\'' | '"'
                        )
                });
            if dyn_field_char
                && self.command_line.input.is_empty()
                && self.dyn_input
                && !self.tabs[i].dyn_fields.is_empty()
            {
                let a = self.tabs[i]
                    .dyn_active
                    .min(self.tabs[i].dyn_fields.len() - 1);
                self.tabs[i].dyn_fields[a]
                    .buffer
                    .get_or_insert_with(String::new)
                    .push_str(&s);
                if self.tabs[i]
                    .active_grip
                    .as_ref()
                    .is_some_and(|grip| grip.mode.uses_scalar_dynamic_input())
                {
                    self.command_line.input.push_str(&s);
                }
            } else {
                // A letter after digits typed into a field makes the whole
                // entry an option keyword such as `2P` or `3P` (#2312).
                if s.starts_with(|c: char| c.is_alphabetic())
                    && self.command_line.input.is_empty()
                    && self.dyn_input
                    && !self.tabs[i].dyn_fields.is_empty()
                {
                    let a = self.tabs[i]
                        .dyn_active
                        .min(self.tabs[i].dyn_fields.len() - 1);
                    if let Some(typed) = self.tabs[i].dyn_fields[a].buffer.take() {
                        self.command_line.input = typed;
                    }
                }
                // Preserve case while entering a command or literal/free-text
                // value. Active single-token prompt keywords keep their
                // established uppercase normalization.
                let preserve_case = self.tabs[i].active_cmd.is_none()
                    || self.is_free_text_active()
                    || self.command_line.literal_spaces
                    || self.command_line.input.starts_with('>')
                    || s.starts_with('>');
                if preserve_case {
                    self.command_line.input.push_str(&s);
                } else {
                    self.command_line.input.push_str(&s.to_uppercase());
                }
                self.command_line.cancel_history_navigation();
                // Live incremental search for INSERT/MINSERT (see CommandInput)
                let live = self.command_line.input.clone();
                let i = self.active_tab;
                let (should_update, opts, prompt, suggestions) =
                    if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                        let changed = cmd.on_live_input(&live);
                        let suggestions = cmd.text_suggestions(&live);
                        if changed {
                            (true, cmd.options(), cmd.prompt(), suggestions)
                        } else {
                            (false, Vec::new(), String::new(), suggestions)
                        }
                    } else {
                        (false, Vec::new(), String::new(), Vec::new())
                    };

                self.command_line.set_contextual_suggestions(suggestions);

                if should_update {
                    self.command_line.set_step_options(opts);
                    if let Some(last) = self.command_line.history.last_mut() {
                        if last.pinned {
                            last.text = prompt;
                        }
                    }
                }
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
        // A Mac keyboard's Delete key sends Backspace: with nothing typed and
        // no command running it erases the selection, as Delete does.
        if cfg!(target_os = "macos")
            && self.command_line.input.is_empty()
            && self.tabs[i].active_cmd.is_none()
            && !self.tabs[i].scene.selected.is_empty()
        {
            return Task::done(Message::DeleteSelected);
        }
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
                if self.tabs[i]
                    .active_grip
                    .as_ref()
                    .is_some_and(|grip| grip.mode.uses_scalar_dynamic_input())
                {
                    self.command_line.input.pop();
                }
                return self.focus_cmd_input();
            }
        }
        self.command_line.input.pop();
        self.command_line.autocomplete_cursor = None;
        self.command_line.cancel_history_navigation();
        // Live incremental search for INSERT/MINSERT
        {
            let live = self.command_line.input.clone();
            let i = self.active_tab;
            let (should_update, opts, prompt) = if let Some(cmd) = self.tabs[i].active_cmd.as_mut()
            {
                if cmd.on_live_input(&live) {
                    (true, cmd.options(), cmd.prompt())
                } else {
                    (false, Vec::new(), String::new())
                }
            } else {
                (false, Vec::new(), String::new())
            };
            if should_update {
                self.command_line.set_step_options(opts);
                if let Some(last) = self.command_line.history.last_mut() {
                    if last.pinned {
                        last.text = prompt;
                    }
                }
            }
        }
        self.focus_cmd_input()
    }


    /// Hand typed command-line text to the active command's step. A step
    /// that took it without a result (a count, a toggle) still moved its
    /// prompt on: that is taken too, or the same text was read again by the
    /// next step — ARRAYPOLAR's item count became its angle (#1571).
    fn offer_typed_text(&mut self, i: usize, text: &str) -> Option<Task<Message>> {
        self.push_ucs_to_cmd(i);
        let command = self.tabs[i].active_cmd.as_mut()?;
        let before = crate::command::step_signature(command.as_ref());
        if let Some(result) = command.on_text_input(text) {
            return Some(self.apply_cmd_result(result));
        }
        if crate::command::step_signature(command.as_ref()) == before {
            return None;
        }
        self.refresh_active_cmd_preview(i);
        Some(self.focus_cmd_input())
    }

    /// Attempt direct distance entry: if an active command is expecting a point,
    /// has an established anchor, and `text` parses as a scalar distance,
    /// project along the active reference ray (if any) or the current cursor
    /// direction in the active UCS plane.
    /// `<angle` at a point prompt locks the next point's direction from the
    /// last point; a bare distance then runs along it. True when consumed.
    pub(in crate::app) fn try_angle_override(&mut self, text: &str) -> bool {
        let (Some(angle), Some(_)) = (text.trim().strip_prefix('<'), self.last_point) else {
            return false;
        };
        let Some(angle) = crate::entities::common::parse_typed_direction(angle) else {
            return false;
        };
        let local = glam::DVec3::new(angle.cos(), angle.sin(), 0.0);
        self.angle_override = Some(match &self.tabs[self.active_tab].active_ucs {
            Some(ucs) => ucs_rotate_vec(local, ucs),
            None => local,
        });
        let shown = crate::entities::common::format_direction(angle);
        self.command_line
            .push_output(crate::tf!("Angle Override: {shown}").as_ref());
        true
    }

    pub(in crate::app) fn try_direct_distance_entry(
        &mut self,
        text: &str,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.needs_entity_pick())
        {
            return None;
        }

        let dist = crate::entities::common::parse_length(text.trim())
            .or_else(|| crate::app::expr_eval::eval_number(text.trim()))?;

        // A tracking ray carries its own base, so it needs no anchor: that is
        // how OTRACK and Extension accepted a bare distance for the *first*
        // point of a command, before any anchor exists.
        let distance_ray = self.active_distance_ray(i);
        let pt = if let Some((base, dir)) = distance_ray {
            base + dir * dist
        } else {
            let anchor = self.tabs[i]
                .active_cmd
                .as_ref()
                .and_then(|c| c.resolved_anchor())
                .or(self.tabs[i].dyn_anchor)
                .or(self.last_point)?;
            let w = self.tabs[i].last_cursor_world;
            let xf = self.tabs[i].ucs_xform();
            let d_ucs = xf.vec_to_ucs(w - anchor);
            let dx = d_ucs.x;
            let dy = d_ucs.y;
            // In the plane unless the cursor is tracked straight up or down
            // (#2796), where what is left in the plane is only noise.
            let in_plane = (dx * dx + dy * dy).sqrt();
            let dir_ucs = if in_plane > 1e-6 && in_plane > d_ucs.z.abs() * 1e-6 {
                glam::DVec3::new(dx, dy, 0.0).normalize()
            } else if d_ucs.length_squared() > 1e-12 {
                d_ucs.normalize()
            } else {
                glam::DVec3::X
            };
            anchor + xf.vec_to_wcs(dir_ucs * dist)
        };

        if !self.command_point_allowed(i, pt) {
            return Some(Task::none());
        }

        self.last_point = Some(pt);
        self.dyn_user_reshaped = false;
        self.dyn_coord_absolute = self.dyn_pointer_absolute;
        self.sync_dyn_fields();
        self.reset_tracking_after_point();
        self.snapper.clear_override();
        if let Some((_, dir)) = distance_ray {
            self.snapper.remember_distance_reference(dir);
        }
        self.push_ucs_to_cmd(i);

        let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_point(pt))?;
        let task = self.apply_cmd_result(result);
        self.refresh_active_cmd_preview(i);
        Some(task)
    }

    /// Shared Enter commit for a hot custom grip. Typed Dynamic Input and the
    /// native direct-distance form both become one ordinary drag Move before
    /// Settle, so the plugin sees the same lifecycle as a pointer placement.
    #[cfg(not(target_arch = "wasm32"))]
    fn settle_plugin_grip_from_enter(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(active) = self.tabs[i].active_plugin_grip.clone() else {
            return Task::none();
        };
        let dyn_locked =
            self.dyn_input && self.tabs[i].dyn_fields.iter().any(|field| field.locked());
        let target = if dyn_locked {
            self.dyn_resolve_point()
        } else {
            // Mirror native grip Stretch's direct-distance entry, including
            // its tracking-ray behavior. No custom-grip scalar/radius mode is
            // introduced here: this is only a displacement from the origin.
            let text = crate::app::expr_eval::eval_to_string(self.command_line.input.trim());
            crate::app::expr_eval::eval_number(text.trim()).map(|dist| {
                if let Some((base, dir)) = self.active_distance_ray(i) {
                    base + dir * dist
                } else {
                    let direction = self.tabs[i].last_cursor_world - active.origin_world;
                    let len = direction.length();
                    if len <= 1e-12 {
                        active.origin_world
                    } else {
                        active.origin_world + direction / len * dist
                    }
                }
            })
        };
        self.command_line.input.clear();

        let mut move_task = Task::none();
        if let Some(mut point) = target {
            // Typed input must not bypass the declaration's hard axis.
            if let Some(axis) = active.axis.and_then(|axis| axis.try_normalize()) {
                point = active.origin_world + axis * (point - active.origin_world).dot(axis);
            }
            move_task = self.move_plugin_grip_drag(i, point);
        }
        let settle_task = self.settle_plugin_grip_drag();
        self.sync_dyn_fields();
        Task::batch([move_task, settle_task])
    }

    /// The active command's current step collects free-form prose from the
    /// command line. Single decision point for the routes that type into
    /// the buffer (CommandInput, Shift+Enter, the view's on_submit wiring).
    pub(crate) fn is_free_text_active(&self) -> bool {
        self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.is_free_text_step())
    }

    /// What Space / Enter currently mean. Previously every key handler
    /// re-derived this from the MText editor and the active command, and
    /// the MText preview case was known only to `CommandSpace` — so Enter
    /// behaved differently depending on which route carried the key.
    pub(crate) fn text_entry_mode(&self) -> crate::app::TextEntryMode {
        if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
            return TextEntryMode::MTextPreview;
        }
        if self.is_free_text_active() {
            return TextEntryMode::FreeText;
        }
        TextEntryMode::Command
    }

    pub(super) fn on_command_finalize(&mut self) -> Task<Message> {
        if self.table_cell_editor.is_some() {
            if self.field_dialog.is_some() && self.tabs[self.active_tab].active_cmd.is_some() {
                return self.feed_command(crate::command::StepInput::Enter);
            }
            return self.table_editor_step(1, 0, false);
        }
        // In the MText preview, Enter inserts a line break. The
        // free-text prompt case falls through deliberately: for it
        // Enter *finishes* the edit (Shift+Enter breaks the line,
        // handled by the SHIFT+ENTER shortcut route).
        if self.text_entry_mode() == TextEntryMode::MTextPreview {
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
        if self.awaiting_fill_close_answer() {
            return self.on_graphic_fill_close_input("");
        }
        // A grip edit is not an active CAD command, but its Dynamic Input
        // fields use the same keyboard path. Route Enter through CommandSubmit,
        // whose grip branch resolves Distance / Angle and finalizes the edit.
        let i = self.active_tab;
        #[cfg(not(target_arch = "wasm32"))]
        if self.tabs[i].active_plugin_grip.is_some() && self.tabs[i].active_cmd.is_none() {
            return self.settle_plugin_grip_from_enter();
        }
        let grip_dyn_locked = self.tabs[i].active_grip.is_some()
            && self.dyn_input
            && self.tabs[i].dyn_fields.iter().any(|field| field.locked());

        if grip_dyn_locked {
            return self.update(Message::CommandSubmit);
        }
        // Enter while a grip is hot places it where it is (or keeps
        // it hot when it has not moved) — it must not fall through to
        // "repeat the last command", which would discard the edit.
        if self.tabs[i].active_grip.is_some() && self.tabs[i].active_cmd.is_none() {
            return self.commit_active_grip_edit();
        }

        // Normal command Dynamic Input commit.
        if let Some(task) = self.try_dyn_commit() {
            return task;
        }

        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_some() {
            self.feed_command(crate::command::StepInput::Enter)
        } else if self.control.user_select.is_some() {
            // Enter with no command active answers a pending client
            // user_select request — same contract as the command-line
            // empty-submit path above.
            self.resolve_user_select(true);
            Task::none()
        } else if let Some(cmd) = self.tabs[i].last_cmd.clone() {
            self.dispatch_command(&cmd)
        } else {
            Task::none()
        }
    }

    pub(in crate::app) fn on_command_escape(&mut self) -> Task<Message> {
        self.table_edit_command_line = false;
        self.tabs[self.active_tab].multiple_cmd = None;
        self.tabs[self.active_tab].measurement.clear();
        // Escape also closes the overlapping-objects list.
        if self.cycle_candidates.take().is_some() {
            self.tabs[self.active_tab].scene.set_hover_highlight(None);
        }
        // Escape answers a pending client getpoint request first: no point.
        if self.control.get_point.is_some() {
            self.resolve_get_point(None);
            return Task::none();
        }
        // Escape answers a pending client user_select request before anything
        // else — the person on the screen is who the request is waiting for.
        if self.control.user_select.is_some() {
            self.resolve_user_select(false);
            return Task::none();
        }
        if self.ribbon.escape_search()
            || self.ribbon.close_menu()
            || self.ribbon.close_app_menu()
            || self.ribbon.escape_extension()
            || self.ribbon.close_popup()
        {
            return Task::none();
        }
        if self.awaiting_fill_close_answer() {
            self.graphic_attributes.pending_fill_close = None;
            self.command_line.push_info(crate::t!("*Cancel*").as_ref());
            return Task::none();
        }
        // Esc drops an unconsumed one-shot snap override and closes
        // its menu (#337). Falls through — Esc keeps its usual effect.
        self.snap_override_popup = None;
        self.snapper.clear_override();
        // Clear the deferred snap dwell — the command is being cancelled.
        self.snap_dwell = None;
        if self.table_cell_editor.is_some()
            && self.field_dialog.is_some()
            && self.tabs[self.active_tab].active_cmd.is_some()
        {
            return self.feed_command(crate::command::StepInput::Escape);
        }
        // Open MText editor swallows Escape (cancel without committing). An
        // open menu or the colour list closes first, one per Escape.
        if let Some(ed) = self.mtext_editor.as_mut() {
            if ed.menu.take().is_some()
                || std::mem::take(&mut ed.color_picker_open)
                || ed.matching.take().is_some()
            {
                return Task::none();
            }
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
            self.command_line.push_info(crate::t!("*Cancel*").as_ref());
            return Task::none();
        }
        // UCS icon: Esc ends any grip drag and clears the selection
        // (only when no command owns Escape).
        if self.tabs[self.active_tab].active_cmd.is_none()
            && (self.ucs_grip_drag.is_some() || self.ucs_icon_selected)
        {
            let i = self.active_tab;
            // A UCS grip drag is live-only until mouse release. Reload
            // the persisted pane UCS so Escape really cancels it.
            if self.ucs_grip_drag.take().is_some() {
                self.tabs[i].refresh_active_ucs();
                self.tabs[i].scene.camera_generation += 1;
            }
            self.ucs_icon_selected = false;
            self.ucs_icon_hover = false;
            self.tabs[i].snap_result = None;
            return Task::none();
        }
        // Leave an interactive navigation mode and end its in-flight
        // drag. Orbit exits silently; PAN keeps its existing message.
        if self.tabs[self.active_tab].pan_mode
            || self.tabs[self.active_tab].orbit_mode
            || self.tabs[self.active_tab].zoom_dynamic_mode
        {
            let i = self.active_tab;
            let was_pan = self.tabs[i].pan_mode;
            self.tabs[i].pan_mode = false;
            self.tabs[i].orbit_mode = false;
            self.tabs[i].zoom_dynamic_mode = false;
            {
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.input.middle_down = false;
                sel.input.middle_last_pos = None;
                sel.orbit.pivot = None;
                sel.gesture.box_anchor = None;
                sel.gesture.box_anchor_world = None;
                sel.gesture.box_current = None;
                sel.gesture.box_crossing_locked = false;
            }
            if was_pan {
                self.command_line
                    .push_output(crate::t!("PAN ended.").as_ref());
            }
            self.ribbon.deactivate_tool();
            return Task::none();
        }
        // Grip popup intercepts Escape — dismisses the menu
        // without doing anything else.
        if self.grip_popup.take().is_some() {
            self.grip_hover = None;
            return Task::none();
        }
        if self.table_popup.take().is_some() {
            return Task::none();
        }
        if self.visibility_popup.take().is_some() {
            self.table_popup = None;
            return Task::none();
        }
        if self.tabs[self.active_tab].active_grip.is_some() {
            self.grip_pending = None;
            self.command_line.input.clear();
            if self.cancel_active_grip_edit() {
                return Task::none();
            }
        }
        if self.grip_pending.take().is_some() {
            self.command_line.input.clear();
            return Task::none();
        }
        // A hot grip (click-move-click placement in progress) rolls
        // back to its pre-drag image on Escape.
        if self.cancel_active_grip_edit() {
            self.sync_dyn_fields();
            return Task::none();
        }
        // Cancel layout rename first, then fall through.
        let i_e = self.active_tab;
        // Quick Select and FILTER close as their Cancel does: the preview
        // goes and a parked 'QSELECT / 'FILTER command comes back.
        if self.qselect.is_some() {
            return self.update(Message::QSelectClose);
        }
        if self.filter_dialog.is_some() {
            return self.on_filter_message(crate::app::filter_dialog::FilterMsg::Close);
        }
        {
            let mut sel = self.tabs[i_e].scene.selection.borrow_mut();
            if sel.menu.open_at.is_some() {
                sel.menu.close();
                return Task::none();
            }
        }
        if self.layout_rename_state.take().is_some() {
            return Task::none();
        }
        // A palette cell holding typed text reverts first: the value goes
        // back to the model's and the row keeps its focus and its
        // reference selected. A second Esc then escalates as usual.
        if self.tabs[i_e].active_cmd.is_none() {
            let panel = &mut self.tabs[i_e].properties;
            if let Some(key) = panel.active_field.clone() {
                if panel.edit_buf.remove(&key).is_some() {
                    return Task::none();
                }
            }
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
                let task = self.apply_cmd_result(r);
                // Ending a command refocuses the command line; Esc that ended
                // it gives the keyboard back to the drawing, so a Shift+key
                // shortcut runs at once. A command that keeps running on Esc
                // (a multi-stage command's first Esc) keeps the focus.
                if self.tabs[self.active_tab].active_cmd.is_none() {
                    return task.chain(self.unfocus_widgets());
                }
                return task;
            }
        } else {
            // In a viewport's model space Esc deselects too and the viewport
            // stays active: PSPACE, or a double-click outside it, leaves it.
            self.tabs[i].selected_authoring = None;
            self.tabs[i].scene.deselect_all();
            self.tabs[i].last_crossing_window = None;
            self.refresh_properties();
            self.tabs[i].scene.set_box_hover_highlights(&[]);
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.gesture.box_anchor = None;
            // Also drop the world-space anchor, or the next pan/zoom
            // re-projects it back into `box_anchor` and the cancelled
            // marquee springs back to life (reproject_box_anchor).
            sel.gesture.box_anchor_world = None;
            sel.gesture.box_current = None;
            sel.gesture.box_crossing = false;
        }
        Task::none()
    }

}

/// Why `new` can't be the name of layer `old`, or `None` when the rename can
/// go ahead. The same rules the document applies, said in the user's words.
pub(super) fn layer_rename_problem(
    document: &document::Document,
    old: &str,
    new: &str,
) -> Option<String> {
    if old == "0" || old.eq_ignore_ascii_case("Defpoints") || old.contains('|') {
        return Some(crate::tf!("Layer \"{old}\" can't be renamed.").into_owned());
    }
    if !crate::scene::valid_block_name(new) {
        return Some(
            crate::tf!("\"{new}\" isn't a valid layer name (no < > / \\ \" : ; ? * | , = `).")
                .into_owned(),
        );
    }
    // A case-only change finds the layer itself, which is fine.
    if document
        .layers
        .get(new)
        .is_some_and(|taken| taken.name != old)
    {
        return Some(crate::tf!("A layer named \"{new}\" already exists.").into_owned());
    }
    None
}

/// Layers the Layer manager never deletes; the rule lives with the table.
pub(super) fn layer_is_protected(name: &str, current: &str) -> bool {
    crate::ui::window::layers::layer_is_protected(name, current)
}

/// The inputs of `refresh_layer_tree`, folded into one value: the layers the
/// selected objects sit on, the layer fields the filter engine reads, the
/// filters the drawing keeps and the ribbon setting. Entity geometry and the
/// selection handles themselves are not inputs — the tree shows layers — so an
/// edit or a selection change that leaves those layers alone must not rebuild it.
fn layer_tree_key(scene: &Scene, ribbon_follows: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ribbon_follows.hash(&mut hasher);
    // The tree lists the distinct lowercased layers of the selected objects, so
    // the key folds that set: one more object on a layer already listed changes
    // nothing, while the set itself hashes in order like the rebuilt row does.
    let mut selected: Vec<String> = scene
        .selected
        .iter()
        .filter_map(|handle| scene.document.get_entity(*handle))
        .map(|entity| entity.common().layer.to_lowercase())
        .collect();
    selected.sort();
    selected.dedup();
    selected.hash(&mut hasher);
    for layer in scene.document.layers.iter() {
        layer.name.hash(&mut hasher);
        layer.description.hash(&mut hasher);
        layer.color.index().hash(&mut hasher);
        layer.line_type.hash(&mut hasher);
        layer.is_off().hash(&mut hasher);
        layer.is_frozen().hash(&mut hasher);
        layer.is_locked().hash(&mut hasher);
        layer.is_plottable.hash(&mut hasher);
        layer.flags.xref_dependent.hash(&mut hasher);
    }
    super::super::layer_filter_store::filter_table_fingerprint(&scene.document).hash(&mut hasher);
    hasher.finish()
}

// Test-only tally of how often `refresh_layer_tree` entered its expensive
// rebuild (reload the drawing's filters + walk them over every layer). The
// idle-refresh test asserts it does not move when nothing changed.
#[cfg(test)]
std::thread_local! {
    static TREE_EXPENSIVE_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod grip_ctrl_cycle_tests;
#[cfg(test)]
mod layer_name_target_tests;
#[cfg(test)]
mod layer_refresh_tests;
#[cfg(test)]
mod layer_rename_tests;
#[cfg(test)]
mod plot_style_ctb_guard_tests;
#[cfg(test)]
mod tab_close_tests;
#[cfg(test)]
mod tablestyle_buffer_tests;
#[cfg(test)]
mod tablestyle_ribbon_tests;
#[cfg(test)]
mod transparency_property_tests;
