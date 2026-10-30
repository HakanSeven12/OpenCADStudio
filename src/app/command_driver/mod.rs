//! Command-result driver: routes [`CmdResult`]s from interactive commands into host state.
//!
//! # Router (no wildcard)
//!
//! `apply_cmd_result_inner` is a thin exhaustive `match` over [`CmdResult`]:
//! every variant has an explicit arm and there is intentionally NO wildcard arm,
//! so adding a variant fails to compile until it is routed here.
//!
//! # Handler signature rule (T5)
//!
//! Router arms stay thin: each forwards to a `handle_*` method on `OpenCADStudio`.
//! - Payloads with **3 or fewer fields** are destructured at the router and passed
//!   as individual arguments.
//! - **Wide payloads** (more than 3 fields) are passed whole via
//!   `result @ Variant { .. }` and destructured inside the handler with `let-else`.
//! - **Or-pattern** arms (e.g. `Cancel | CancelForSpaceChange`) pass the enum itself
//!   so the handler can tell the cases apart with `matches!`.
//! - Return type follows the arm's control flow so the router's post-match focus
//!   bookkeeping runs exactly when it used to: `()` when the arm falls through,
//!   `Task<Message>` when every path diverges with `return`, and
//!   `Option<Task<Message>>` (`Some` = return, `None` = fall through) when the arm
//!   mixes both. `Relaunch`/`Dispatch` instead produce the `dispatched` task that
//!   the router assigns.
//!
//! Handler bodies are verbatim 1:1 moves of the original match arms (plus the
//! mechanical `let i = self.active_tab;` prologue).
//!
//! # Adding a variant (T1 exhaustiveness guide)
//!
//! There is intentionally NO wildcard arm in the router, so adding a
//! [`CmdResult`] variant fails to compile until it is routed here. To add one:
//! 1. add an explicit arm in `apply_cmd_result_inner` following the T5 rule;
//! 2. put the `handle_*` body in the owning file below (extend the `impl`
//!    block there — never inline a body in the router);
//! 3. if no file owns the new concern, extend this list and document the new
//!    file here.
//!
//! # Router-only exception (T6)
//!
//! Only the router (`apply_cmd_result` / `apply_cmd_result_inner`), flow and
//! lifecycle arms, group/table arms, mleader arms, and helpers with callers in
//! more than one domain file live in `mod.rs`: `merge_dependencies` (also
//! called from `app::update::dialog`), `restore_pre_cmd_tangent` (called from
//! every domain file), and `entity_at_typed_point` (called from
//! `utilities.rs`). Everything else lives in its domain file.
//!
//! # File ownership map
//!
//! - `utilities.rs` — generic pre-match helpers.
//! - `parameters.rs` — parametric parameters, named-parameter editor rows,
//!   dynamic dimensions, and the constraint-inference entry points
//!   (`apply_continuous_constraints`, `apply_inferred_constraints`) shared by
//!   the `constraint.rs` arms.
//! - `property_match.rs` — MATCHPROP/LAYMATCH arms (special-prop kernels live
//!   in `crate::entities::match_props`).
//! - `entity_commit.rs` — entity/dimension/hatch/block commit arms.
//! - `solid3d.rs` — extrude/thicken/presspull/revolve/sweep/loft/shell/boolean/slice.
//! - `viewport.rs` — MVIEW/layout/plot/zoom/print arms.
//! - `transform.rs` — MOVE/COPY/ALIGN/STRETCH arms.
//! - `clipboard.rs` — COPYCLIP/PASTECLIP/PASTEORIG/PASTEBLOCK arms,
//!   clipboard merge + xdict helpers; re-exports
//!   `remap_ext_subtree_reference` and `copy_to_clipboard_kernel`
//!   for `app::commands::blocks`.
//! - `modify.rs` — LENGTHEN/DIVIDE/MEASURE/PEDIT/JOIN/BREAK/HATCHEDIT arms +
//!   `apply_pedit_result`. (Entity BREAK itself delegates to
//!   `break_cmd::break_entity`; the DIMBREAK free helpers live in `dim_edit`.)
//! - `text_edit.rs` — MTEXT/TEXT edit-session arms (incl. DDEDIT).
//! - `constraint.rs` — parametric/geometric constraint arms (moved LAST:
//!   active development area).
//! - `dim_edit.rs` — DIMBREAK/DIMJOGLINE/DIMSPACE arms + `apply_dimspace`,
//!   `dimension_auto_spacing`, and the dimbreak free helpers
//!   (`DimBreakResult`, `wire_segments`, `segment_intersection_xy`,
//!   `break_object_handle`, `remove/write_dimension_break_data`,
//!   `break_reference`, `apply_dimbreak`).
//! - mleader arms + `apply_mleader_align`/`collect` helpers live in `mod.rs`
//!   (router-only exception).

use super::{Message, OpenCADStudio};
use crate::command::{CmdResult, SelectionEntity, StepInput};
use codec::Handle;
use iced::Task;

pub(super) mod clipboard;
mod constraint;
mod dim_edit;
mod entity_commit;
mod modify;
mod parameters;
mod property_match;
mod solid3d;
mod tests;
mod autolisp_tests;
mod text_edit;
mod transform;
mod utilities;
mod viewport;

pub(crate) use clipboard::{
    copy_to_clipboard_kernel, define_block_raw_doc, paste_entities_kernel,
    remap_ext_subtree_reference, remap_object, PasteSpace,
};

impl OpenCADStudio {
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
        let panel_updates = self.tabs[i]
            .active_cmd
            .as_mut()
            .map(|c| c.take_pending_panel_updates())
            .unwrap_or_default();
        for (panel_id, values) in panel_updates {
            if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                panel.update_values(values);
            }
        }
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
        let gap_tolerance = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .and_then(|command| command.hatch_gap_tolerance());
        if let Some(gap_tolerance) = gap_tolerance {
            if gap_tolerance != self.hatch_gap_tolerance {
                self.hatch_gap_tolerance = gap_tolerance;
                self.persist_settings_if_changed();
            }
        }
        let variable = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .and_then(|command| command.drawing_variable_update());
        if let Some((name, value)) = variable {
            let tab = &mut self.tabs[self.active_tab];
            if crate::io::drawing_variable(&tab.scene.document, name).as_deref() != Some(value.as_str()) {
                crate::io::set_drawing_variable(&mut tab.scene.document, name, &value);
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
            let style_handle = style_handle.unwrap_or(codec::Handle::NULL);
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
                | CmdResult::Script(..)
                | CmdResult::QSelectSample { .. }
                | CmdResult::SolidEdgeBlend { .. }
                | CmdResult::SolidSubtract { .. }
                | CmdResult::SliceEntities { .. }
                | CmdResult::SliceSurfaceEntities { .. }
        );
        let task = self.apply_cmd_result_inner(result);
        let i = self.active_tab;
        // A typed `<angle` does not outlive the command it was typed in.
        if self.tabs[i].active_cmd.is_none() {
            self.angle_override = None;
        }
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
            self.command_line.clear_contextual_suggestions();
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
        handle: codec::Handle,
        click: glam::DVec3,
    ) -> crate::modules::annotate::table_cmd::TableCellEditStart {
        use crate::modules::annotate::table_cmd::{table_cell_at, TableCellEditStart};
        let target = if handle != codec::Handle::NULL {
            self.tabs[i]
                .scene
                .document
                .get_entity(handle)
                .and_then(|entity| match entity {
                    codec::EntityType::Table(table) => Some((handle, table.clone())),
                    _ => None,
                })
        } else {
            // Find all tables whose grid contains `click`.
            // Iterating document.entities() visits entities in draw order;
            // taking the last match selects the table on top if they overlap.
            let mut matched = None;
            for entity in self.tabs[i].scene.document.entities() {
                if let codec::EntityType::Table(table) = entity {
                    let h = table.common.handle;
                    let style = table.table_style_handle.and_then(|style_handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .objects
                            .get(&style_handle)
                            .and_then(|object| match object {
                                codec::objects::ObjectType::TableStyle(style) => Some(style),
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
                    codec::objects::ObjectType::TableStyle(style) => Some(style),
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
        if let Some(prompt) = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|command| command.prompt())
        {
            self.command_line.push_info(&prompt);
        }
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
        if self.tabs[i].suspended_cmd.is_some() {
            return;
        }
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

    pub(in crate::app) fn start_from_modifier(&mut self, i: usize) {
        // There is only one parking slot; never overwrite an existing parent.
        if self.tabs[i].suspended_cmd.is_some() {
            return;
        }
        let parent = self.tabs[i].active_cmd.take();
        self.tabs[i].suspended_cmd = parent;
        self.tabs[i].scene.clear_preview_wire();
        let cmd = crate::command::FromPointCommand::new(self.last_point);
        let prompt = crate::command::CadCommand::prompt(&cmd);
        self.tabs[i].active_cmd = Some(Box::new(cmd));
        self.command_line.push_info(&prompt);
        self.command_line.set_step_options(Vec::new());
        self.sync_dyn_fields();
        self.refresh_active_cmd_preview(i);
    }

    pub(in crate::app) fn relative_input_base(&self, i: usize) -> Option<glam::DVec3> {
        self.tabs[i]
            .active_cmd
            .as_ref()
            .filter(|c| c.name() == "FROM")
            .and_then(|c| c.resolved_anchor())
            .or(self.last_point)
    }

    /// Apply a command replacement without deleting a one-to-one entity edit.
    /// Splits and entity-type conversions still allocate replacement identities.
    fn replace_command_entity(
        &mut self,
        tab: usize,
        handle: Handle,
        mut entities: Vec<codec::EntityType>,
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
            let pieces: Vec<Handle> = entities
                .into_iter()
                .map(|mut entity| {
                    entity.common_mut().handle = Handle::NULL;
                    if let Some(owner) = owner {
                        entity.common_mut().owner_handle = owner;
                    }
                    self.tabs[tab].scene.add_entity(entity)
                })
                .collect();
            // Hand the hatch associations over before the original goes.
            self.tabs[tab].scene.split_hatch_source(handle, &pieces);
            self.tabs[tab].scene.erase_entities(&[handle]);
            pieces
        };
        for &updated in &handles {
            if matches!(
                self.tabs[tab].scene.document.get_entity(updated),
                Some(codec::EntityType::Dimension(_))
            ) {
                self.tabs[tab].scene.invalidate_dim_block_recorded(updated);
            }
        }
        handles
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
            CmdResult::QSelectSample {
                state,
                tab_id,
                handle,
            } => {
                self.finish_qselect_sample(*state, tab_id, handle);
            }
            CmdResult::QuickCalcPick(value) => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                dispatched = Task::done(Message::QuickCalc(
                    crate::ui::window::quick_calc::QuickCalcMsg::Picked(value),
                ));
            }
            CmdResult::OpenTxt2MtxtSettings => self.open_txt2mtxt_settings(),
            CmdResult::OpenAutoConstrainSettings => {
                self.handle_open_auto_constrain_settings();
            }
            CmdResult::NeedPoint => {
                if let Some(task) = self.handle_need_point() {
                    return task;
                }
            }
            CmdResult::Preview(wire) => {
                self.handle_preview(wire);
            }
            CmdResult::InterimWire(wire) => {
                self.handle_interim_wire(wire);
            }
            CmdResult::CommitEntity(entity) => {
                if let Some(task) = self.handle_commit_entity(entity, preserve_commit_layer) {
                    return task;
                }
            }
            CmdResult::CommitEntities(entities) => {
                if let Some(task) = self.handle_commit_entities(
                    entities,
                    preserve_commit_style,
                    preserve_commit_layer,
                ) {
                    return task;
                }
            }
            CmdResult::CommitEntitiesAndExit(entities) => {
                self.handle_commit_entities_and_exit(
                    entities,
                    preserve_commit_style,
                    preserve_commit_layer,
                );
            }
            CmdResult::MviewCreate {
                viewport,
                preserve_view,
            } => {
                self.handle_mview_create(viewport, preserve_view);
            }
            CmdResult::MviewCreateClipped {
                boundary,
                boundary_handle,
                target,
            } => {
                if !target.is_null() {
                    self.handle_vpclip(target, boundary, boundary_handle);
                } else if let Some(task) =
                    self.handle_mview_create_clipped(boundary, boundary_handle)
                {
                    return task;
                }
            }
            CmdResult::WipeoutFromPolyline {
                handle,
                erase_source,
            } => {
                if let Some(task) = self.handle_wipeout_from_polyline(handle, erase_source) {
                    return task;
                }
            }
            CmdResult::MviewSwitchLayout(layout) => return self.handle_mview_switch_layout(layout),
            CmdResult::MviewCancelToLayout(layout) => {
                return self.handle_mview_cancel_to_layout(layout)
            }
            CmdResult::TransformSelected(handles, transform) => {
                if let Some(task) = self.handle_transform_selected(handles, transform) {
                    return task;
                }
            }
            CmdResult::CopySelected(handles, transform) => {
                if let Some(task) = self.handle_copy_selected(handles, transform) {
                    return task;
                }
            }
            CmdResult::CopyToClipboard { handles, base } => {
                self.handle_copy_to_clipboard(handles, base);
            }
            CmdResult::CommitAndExit(entity) => {
                if let Some(task) = self.handle_commit_and_exit(entity, preserve_commit_layer) {
                    return task;
                }
            }
            result @ CmdResult::CommitDimension { .. } => {
                if let Some(task) = self.handle_commit_dimension(result) {
                    return task;
                }
            }
            CmdResult::CommitDimensionsAndExit(dimensions) => {
                self.handle_commit_dimensions_and_exit(dimensions);
            }
            CmdResult::SetQuickDimensionSnapPriority(priority) => {
                self.handle_set_quick_dimension_snap_priority(priority);
            }
            CmdResult::AlignMLeaders {
                handles,
                reference,
                mode,
                spacing,
                from,
                to,
            } => {
                self.handle_align_mleaders(handles, reference, mode, spacing, from, to);
            }
            CmdResult::CollectMLeaders {
                handles,
                point,
                layout,
            } => {
                self.handle_collect_mleaders(handles, point, layout);
            }
            result @ CmdResult::CommitSolid { .. } => {
                self.handle_commit_solid(result);
            }
            CmdResult::CommitAndEditText(entity) => {
                if let Some(task) = self.handle_commit_and_edit_text(entity) {
                    return task;
                }
            }
            CmdResult::CommitManyAndEditText {
                entities,
                edit_index,
                open_editor,
            } => {
                if let Some(task) =
                    self.handle_commit_many_and_edit_text(entities, edit_index, open_editor)
                {
                    return task;
                }
            }
            CmdResult::CreateBlock {
                handles,
                name,
                base,
            } => {
                if let Some(task) = self.handle_create_block(handles, name, base) {
                    return task;
                }
            }
            result @ CmdResult::SetBlockParameterGrips { .. } => {
                return self.set_block_parameter_grips(result);
            }
            result @ CmdResult::SetBlockConstruction { .. } => {
                return self.set_block_construction(result);
            }
            result @ CmdResult::EditBlockAuthoring { .. } => {
                return self.handle_authoring_edit(result);
            }
            CmdResult::SetBlockActionBars { document,definition,mode,parameters } => {
                return self.set_action_bars(document,definition,mode,parameters);
            }
            result @ CmdResult::AlignBlockReference { .. } => { return self.handle_block_alignment(result); }
            result @ (CmdResult::PlaceBlockAction { .. }
            | CmdResult::EditBlockActionSet { .. }
            | CmdResult::AssociateBlockAction { .. }
            | CmdResult::EvaluateBlockParameter { .. }) => {
                return self.handle_block_action_result(result);
            }
            result @ CmdResult::PlaceBlockSingleParameter { .. } => {
                return self.handle_place_block_single_parameter(result);
            }
            result @ CmdResult::PlaceBlockOrientationParameter { .. } => {
                return self.handle_place_block_orientation_parameter(result);
            }
            result @ CmdResult::PlaceBlockDistanceParameter { .. } => {
                return self.handle_place_block_distance_parameter(result);
            }
            result @ CmdResult::PlaceBlockPointParameter { .. } => {
                return self.handle_place_block_point_parameter(result);
            }
            CmdResult::PlaceBlockVisibilityParameter {
                document_id,
                definition,
                location,
            } => {
                return self.handle_place_block_visibility_parameter(
                    document_id,
                    definition,
                    location,
                );
            }
            CmdResult::CreateBlockWithOptions { options } => {
                if let Some(task) = self.handle_create_block_with_options(options) {
                    return task;
                }
            }
            CmdResult::CommitHatch(hatch) => {
                self.handle_commit_hatch(hatch);
            }
            CmdResult::CommitStyledHatch {
                hatch,
                color,
                transparency,
            } => {
                self.handle_commit_styled_hatch(hatch, color, transparency);
            }
            CmdResult::CommitHatchWithBoundaries {
                hatch,
                boundaries,
                entity_style,
            } => {
                self.handle_commit_hatch_with_boundaries(hatch, boundaries, entity_style);
            }
            CmdResult::CommitHatches {
                hatches,
                entity_style,
            } => {
                self.handle_commit_hatches(hatches, entity_style);
            }
            CmdResult::BatchCopy(handles, transforms) => {
                if let Some(task) = self.handle_batch_copy(handles, transforms) {
                    return task;
                }
            }
            CmdResult::AssocArray { handles, spec } => {
                self.handle_assoc_array(handles, spec);
            }
            CmdResult::ReplaceMany(replacements, additions) => {
                if let Some(task) = self.handle_replace_many(replacements, additions) {
                    return task;
                }
            }
            CmdResult::ReplaceManyContinue(replacements) => {
                if let Some(task) = self.handle_replace_many_continue(replacements) {
                    return task;
                }
            }
            CmdResult::XClip { inserts, action } => {
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.apply_xclip(i, inserts, action);
            }
            CmdResult::OpenPdfImportSettings => self.open_pdf_import_settings(),
            CmdResult::PdfPassword {
                path,
                input,
                stage,
                remembered,
                attach,
            } => return self.handle_pdf_password(path, input, stage, remembered, attach),
            CmdResult::DgnImport(request) => {
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                return self.run_dgn_import(i, request);
            }
            CmdResult::PdfImportFile(import) => {
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.run_pdf_import(
                    i,
                    crate::app::commands::pdf_import::PdfImportSource::File(import),
                );
            }
            CmdResult::AttachPointCloud { placement, message } => {
                let label = self.history_label_from_active_cmd(i, "POINTCLOUDATTACH");
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line.push_output(&message);
                self.attach_point_cloud(i, label, placement);
                self.restore_pre_cmd_tangent();
            }
            CmdResult::AttachPdfPages { kind, path, pages } => {
                let label = self.history_label_from_active_cmd(i, "PDFATTACH");
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.attach_pdf_pages(i, label, kind, &path, pages);
                self.restore_pre_cmd_tangent();
            }
            CmdResult::PdfImport(request) => {
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.run_pdf_import(
                    i,
                    crate::app::commands::pdf_import::PdfImportSource::Underlay(request),
                );
            }
            CmdResult::UpdateEntityAndFinish { handle, entity } => {
                if let Some(task) = self.handle_update_entity_and_finish(handle, entity) {
                    return task;
                }
            }
            CmdResult::UpdateEntityAndReport {
                handle,
                entity,
                message,
            } => {
                let task = self.handle_update_entity_and_finish(handle, entity);
                self.command_line.push_output(&message);
                if let Some(task) = task {
                    return task;
                }
            }
            result @ CmdResult::AddParametricConstraint { .. } => {
                if let Some(task) = self.handle_add_parametric_constraint(result) {
                    return task;
                }
            }
            result @ CmdResult::AddEqualConstraint { .. } => {
                if let Some(task) = self.handle_add_equal_constraint(result) {
                    return task;
                }
            }
            CmdResult::CheckConstraintPoint(pick) => {
                return self.handle_check_constraint_point(pick)
            }
            result @ CmdResult::AddDimensionalConstraint { .. } => {
                if let Some(task) = self.handle_add_dimensional_constraint(result) {
                    return task;
                }
            }
            CmdResult::CancelWithMessage(message) => {
                return self.handle_cancel_with_message(message)
            }
            result @ CmdResult::AddRadialConstraint { .. } => {
                if let Some(task) = self.handle_add_radial_constraint(result) {
                    return task;
                }
            }
            result @ CmdResult::AddAngularConstraint { .. } => {
                if let Some(task) = self.handle_add_angular_constraint(result) {
                    return task;
                }
            }
            result @ CmdResult::MakeParallel { .. } => return self.handle_make_parallel(result),
            CmdResult::AddFixedConstraint(pick) => return self.handle_add_fixed_constraint(pick),
            CmdResult::CheckHorizontalPoint { kind, pick } => {
                return self.handle_check_horizontal_point(kind, pick)
            }
            result @ CmdResult::AddHorizontalConstraint { .. } => {
                if let Some(task) = self.handle_add_horizontal_constraint(result) {
                    return task;
                }
            }
            CmdResult::AddSymmetricConstraint {
                selection,
                axis,
                label,
            } => {
                if let Some(task) = self.handle_add_symmetric_constraint(selection, axis, label) {
                    return task;
                }
            }
            result @ CmdResult::AddPerpendicularConstraint { .. } => {
                if let Some(task) = self.handle_add_perpendicular_constraint(result) {
                    return task;
                }
            }
            CmdResult::AddTangentConstraint {
                first,
                second,
                label,
            } => {
                if let Some(task) = self.handle_add_tangent_constraint(first, second, label) {
                    return task;
                }
            }
            CmdResult::AddConcentricConstraint {
                first,
                second,
                label,
            } => {
                if let Some(task) = self.handle_add_concentric_constraint(first, second, label) {
                    return task;
                }
            }
            result @ CmdResult::AddCoincidentConstraint { .. } => {
                if let Some(task) = self.handle_add_coincident_constraint(result) {
                    return task;
                }
            }
            CmdResult::AddAutoCoincidentConstraints { handles } => {
                self.handle_add_auto_coincident_constraints(handles);
            }
            result @ CmdResult::AddPointOnEntityConstraint { .. } => {
                if let Some(task) = self.handle_add_point_on_entity_constraint(result) {
                    return task;
                }
            }
            CmdResult::AddEqualDistanceConstraint { points, label } => {
                if let Some(task) = self.handle_add_equal_distance_constraint(points, label) {
                    return task;
                }
            }
            CmdResult::ReassociateCenterMark {
                target,
                source,
                point,
            } => {
                if let Some(task) = self.handle_reassociate_center_mark(target, source, point) {
                    return task;
                }
            }
            CmdResult::ReassociateCenterLine {
                target,
                first,
                second,
            } => {
                if let Some(task) = self.handle_reassociate_center_line(target, first, second) {
                    return task;
                }
            }
            CmdResult::EditDimensionBreak {
                dimensions,
                operation,
            } => {
                self.handle_edit_dimension_break(dimensions, operation);
            }
            CmdResult::EditDimensionJog { dimension, point } => {
                self.handle_edit_dimension_jog(dimension, point);
            }
            CmdResult::ReassociateDimension {
                dimension,
                target,
                last,
            } => {
                self.handle_reassociate_dimension(dimension, target, last);
            }
            CmdResult::SpaceDimensions {
                base,
                others,
                spacing,
            } => {
                self.handle_space_dimensions(base, others, spacing);
            }
            CmdResult::DistributeDimensions {
                base,
                dimensions,
                offset,
            } => {
                self.handle_distribute_dimensions(base, dimensions, offset);
            }
            CmdResult::ReplaceEntity(handle, new_entities) => {
                if let Some(task) = self.handle_replace_entity(handle, new_entities) {
                    return task;
                }
            }
            CmdResult::CommitExplodedInsert { insert, keep_going } => {
                let i = self.active_tab;
                if let codec::EntityType::Insert(ins) = &insert {
                    let name = ins.block_name.clone();
                    self.record_block_insert(&name);
                }
                let pieces = crate::modules::draw::modify::explode::explode_entity(
                    &insert,
                    &self.tabs[i].scene.document,
                );
                if keep_going {
                    if let Some(task) = self.handle_commit_entities(
                        pieces,
                        preserve_commit_style,
                        preserve_commit_layer,
                    ) {
                        return task;
                    }
                } else {
                    self.handle_commit_entities_and_exit(
                        pieces,
                        preserve_commit_style,
                        preserve_commit_layer,
                    );
                }
            }
            CmdResult::AttreqNeeded { block_name } => {
                if let Some(task) = self.handle_attreq_needed(block_name) {
                    return task;
                }
            }
            CmdResult::CommitLiveEntity(entity) => {
                self.handle_commit_live_entity(entity);
            }
            CmdResult::UpdateLiveEntity {
                handle,
                entity,
                finish,
            } => {
                self.handle_update_live_entity(handle, entity, finish);
            }
            CmdResult::FinalizeLiveEntity(handle) => {
                self.handle_finalize_live_entity(handle);
            }
            CmdResult::RemoveLiveEntity(handle) => {
                self.handle_remove_live_entity(handle);
            }
            CmdResult::Done => {
                self.handle_done();
            }
            cancel @ (CmdResult::Cancel | CmdResult::CancelForSpaceChange) => {
                self.handle_cancel(cancel);
            }
            CmdResult::SelectByPath {
                path,
                closed,
                crossing,
            } => return self.handle_select_by_path(path, closed, crossing),
            CmdResult::Relaunch(cmd, handles) => {
                dispatched = self.handle_relaunch(cmd, handles);
            }
            CmdResult::Dispatch(cmd) => {
                dispatched = self.handle_dispatch(cmd);
            }
            CmdResult::Script(script) => {
                dispatched = self.handle_script(script);
            }
            CmdResult::EditTableCell { handle, point } => {
                if let Some(task) = self.handle_edit_table_cell(handle, point) {
                    return task;
                }
            }
            CmdResult::MatchEntityLayer { dest, src } => {
                self.handle_match_entity_layer(dest, src);
            }
            CmdResult::MatchProperties { dest, src } => {
                if let Some(task) = self.handle_match_properties(dest, src) {
                    return task;
                }
            }
            CmdResult::PasteClipboard { base_pt } => {
                self.handle_paste_clipboard(base_pt);
            }
            CmdResult::CreateGroup { handles, name } => {
                if let Some(task) = self.handle_create_group(handles, name) {
                    return task;
                }
            }
            CmdResult::DeleteGroups { handles } => {
                if let Some(task) = self.handle_delete_groups(handles) {
                    return task;
                }
            }
            CmdResult::VpLayerUpdate {
                vp_handle,
                freeze,
                thaw,
            } => {
                self.handle_vp_layer_update(vp_handle, freeze, thaw);
            }

            CmdResult::ZoomToWindow { p1, p2 } => {
                self.handle_zoom_to_window(p1, p2);
            }
            CmdResult::Show { p1, p2 } => {
                let i = self.active_tab;
                self.tabs[i].scene.remember_current_view();
                self.tabs[i]
                    .scene
                    .zoom_to_window(p1.as_vec3(), p2.as_vec3());
            }
            CmdResult::CommitAndZoom { entity, p1, p2 } => {
                self.handle_commit_entities_and_exit(vec![entity], false, false);
                self.handle_zoom_to_window(p1, p2);
            }
            CmdResult::ScaleViewports { handles, factor } => {
                self.handle_scale_viewports(handles, factor);
            }
            CmdResult::TwistView(twist) => {
                self.handle_twist_view(twist);
            }
            CmdResult::Measurement(msg) => {
                self.handle_measurement(msg);
            }
            CmdResult::ReportMeasurement(msg) => {
                self.handle_report_measurement(msg);
            }
            CmdResult::ReportError(msg) => {
                self.handle_report_error(msg);
            }
            CmdResult::ReportMeasurementAndDeselect(msg) => {
                self.handle_report_measurement_and_deselect(msg);
            }
            CmdResult::DeselectAndContinue => {
                self.handle_deselect_and_continue();
            }
            result @ CmdResult::AlignSelected { .. } => {
                self.handle_align_selected(result);
            }
            CmdResult::LengthenEntity {
                handle,
                pick_pt,
                mode,
            } => {
                if let Some(task) = self.handle_lengthen_entity(handle, pick_pt, mode) {
                    return task;
                }
            }
            CmdResult::DivideEntity { handle, n, marker } => {
                self.handle_divide_entity(handle, n, marker);
            }
            result @ CmdResult::MeasureEntity { .. } => {
                self.handle_measure_entity(result);
            }
            CmdResult::PeditOp { handle, op } => return self.handle_pedit_op(handle, op),
            CmdResult::JoinToSource { source, handles } => {
                if let Some(task) = self.handle_join_to_source(source, handles) {
                    return task;
                }
            }
            CmdResult::JoinEntities(handles) => {
                if let Some(task) = self.handle_join_entities(handles) {
                    return task;
                }
            }
            CmdResult::BreakEntity { handle, p1, p2 } => {
                if let Some(task) = self.handle_break_entity(handle, p1, p2) {
                    return task;
                }
            }
            CmdResult::SetPlotWindow { p1, p2 } => {
                self.handle_set_plot_window(p1, p2);
            }
            CmdResult::QuickPrint(handles) => return self.handle_quick_print(handles),
            CmdResult::StretchWindow {
                handles,
                windows,
                excluded_points,
                subtract_window,
            } => {
                self.handle_stretch_window(handles, windows, excluded_points, subtract_window);
            }
            CmdResult::StretchEntities {
                handles,
                windows,
                excluded_points,
                delta,
            } => {
                if let Some(task) =
                    self.handle_stretch_entities(handles, windows, excluded_points, delta)
                {
                    return task;
                }
            }
            // ── EXTRUDE ────────────────────────────────────────────────────
            result @ CmdResult::ExtrudeEntities { .. } => {
                if let Some(task) = self.handle_extrude_entities(result) {
                    return task;
                }
            }

            CmdResult::ThickenEntities { handles, distance } => {
                if let Some(task) = self.handle_thicken_entities(handles, distance) {
                    return task;
                }
            }

            result @ CmdResult::PresspullPick { .. } => {
                self.handle_presspull_pick(result);
            }
            CmdResult::PresspullApply {
                targets,
                distance,
                color: _,
            } => {
                self.handle_presspull_apply(targets, distance);
            }

            // ── REVOLVE ────────────────────────────────────────────────────
            result @ CmdResult::RevolveEntities { .. } => {
                if let Some(task) = self.handle_revolve_entities(result) {
                    return task;
                }
            }
            // ── SWEEP ─────────────────────────────────────────────────────
            result @ CmdResult::SweepEntities { .. } => {
                self.handle_sweep_entities(result);
            }

            // ── LOFT ──────────────────────────────────────────────────────
            result @ CmdResult::LoftEntities { .. } => {
                if let Some(task) = self.handle_loft_entities(result) {
                    return task;
                }
            }

            result @ CmdResult::SolidEdgeBlend { .. } => {
                return self.handle_solid_edge_blend(result)
            }

            CmdResult::SolidShell {
                handle,
                actions,
                distance,
            } => return self.handle_solid_shell(handle, actions, distance),

            CmdResult::SolidSubtract {
                bases,
                cutters,
                convert_meshes,
            } => return self.handle_solid_subtract(bases, cutters, convert_meshes),

            CmdResult::SliceEntities {
                targets,
                plane,
                keep_point,
            } => return self.handle_slice_entities(targets, plane, keep_point),

            CmdResult::SliceSurfaceEntities {
                targets,
                cutter,
                keep_point,
            } => return self.handle_slice_surface_entities(targets, cutter, keep_point),

            result @ CmdResult::HatcheditApply { .. } => {
                if let Some(task) = self.handle_hatchedit_apply(result) {
                    return task;
                }
            }
            result @ CmdResult::OpenMTextEditor { .. } => {
                self.handle_open_mtext_editor(result);
            }
            CmdResult::SuspendForMTextInput {
                pos,
                initial,
                height,
            } => {
                self.handle_suspend_for_mtext_input(pos, initial, height);
            }
            result @ CmdResult::OpenTextEditor { .. } => {
                self.handle_open_text_editor(result);
            }
            CmdResult::SuspendForTextInput { pos, entity } => {
                self.handle_suspend_for_text_input(pos, entity);
            }
            CmdResult::EditTextEntity { handle } => return self.handle_edit_text_entity(handle),
            CmdResult::SuspendForTextEdit { handle } => {
                return self.handle_suspend_for_text_edit(handle)
            }
            CmdResult::UndoDocument => {
                self.handle_undo_document();
            }
            CmdResult::SetTexteditMode(val) => {
                self.handle_set_textedit_mode(val);
            }
            CmdResult::EditDimensions { handles, operation } => {
                self.handle_edit_dimensions(handles, operation);
            }
            CmdResult::DdeditEntity { handle, new_text } => {
                if let Some(task) = self.handle_ddedit_entity(handle, new_text) {
                    return task;
                }
            }
            CmdResult::ReturnPoint(_) => {
                self.handle_return_point();
            }
        }
        // When no command is running the ribbon tool button still has to
        // visually deactivate. Keyboard focus is assigned below to whichever
        // editor currently owns typed input. Re-read the active tab: a
        // dispatched command (CLOSE, an SDI open) may have closed tab `i`.
        if self.tabs[self.active_tab].active_cmd.is_none() {
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
        crate::ui::ribbon::keytip_note_command_finished();
        Task::batch([dispatched, focus])
    }

    // --- C2a handlers: flow / lifecycle (thin-router targets, bodies moved verbatim) ---

    fn handle_need_point(&mut self) -> Option<Task<Message>> {
        let i = self.active_tab;
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
            return Some(Task::none());
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
        None
    }

    fn handle_preview(&mut self, wire: crate::scene::model::wire_model::WireModel) {
        let i = self.active_tab;
        self.tabs[i].scene.set_preview_wires(vec![wire]);
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
    }

    fn handle_interim_wire(&mut self, wire: crate::scene::model::wire_model::WireModel) {
        let i = self.active_tab;
        self.tabs[i].scene.set_interim_wire(wire);
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
    }

    fn handle_done(&mut self) {
        crate::ui::ribbon::keytip_note_command_finished();
        let i = self.active_tab;
        self.tabs[i].scene.clear_preview_wire();
        if self.tabs[i].suspended_cmd.is_some() {
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
        }
    }

    fn handle_cancel(&mut self, cancel: CmdResult) {
        crate::ui::ribbon::keytip_note_command_finished();
        let i = self.active_tab;
        self.tabs[i].pending_parameter_set_follow_up = None;
        self.tabs[i].pending_xy_box_set = None;
        self.tabs[i].pending_action_pair = None;
        self.tabs[i].measurement.clear();
        let space_changed = matches!(cancel, CmdResult::CancelForSpaceChange);
        self.tabs[i].scene.clear_preview_wire();
        if !space_changed && self.tabs[i].suspended_cmd.is_some() {
            if let Some(point) = self.tabs[i]
                .active_cmd
                .as_ref()
                .and_then(|c| c.modifier_restore_point())
            {
                self.last_point = point;
                self.reset_tracking_after_point();
            }
            self.tabs[i].active_cmd = self.tabs[i].suspended_cmd.take();
            self.sync_dyn_fields();
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

    fn handle_cancel_with_message(&mut self, message: String) -> Task<Message> {
        for line in message.lines() {
            self.command_line.push_error(line);
        }
        self.apply_cmd_result(CmdResult::Cancel)
    }

    /// A window between two typed corners, run through the same box test
    /// as a dragged one: a box upright on the screen (so in a twisted or 3D
    /// view its sides follow the screen), window or crossing by the drag's
    /// rule, and the same filters.
    pub(super) fn select_typed_window(
        &mut self,
        first: glam::DVec3,
        second: glam::DVec3,
    ) -> Task<Message> {
        let i = self.active_tab;
        let canvas = self.tabs[i].scene.selection.borrow().view.vp_size;
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
        let (view_rot, eye, _) = self.pick_view(i, &edit_cam, bounds);
        let a = crate::scene::pick::hit_test::world_to_screen(first, view_rot, eye, bounds);
        let p = crate::scene::pick::hit_test::world_to_screen(second, view_rot, eye, bounds);
        let crossing = {
            // A Window / Crossing keyword holds for this one box, as for a
            // dragged one.
            let mut selection = self.tabs[i].scene.selection.borrow_mut();
            let gesture = &mut selection.gesture;
            let crossing = if gesture.box_crossing_locked {
                gesture.box_crossing
            } else {
                crate::scene::selection_box_crossing(a, p)
            };
            gesture.box_crossing = false;
            gesture.box_crossing_locked = false;
            crossing
        };
        let (mut handles, _) = self.box_selection_handles(i, &edit_cam, bounds, a, p, crossing);
        handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
        self.filter_handles_reject_locked(i, &mut handles);
        self.last_pick = Some((handles.clone(), self.select_remove_mode));
        if self.select_remove_mode {
            self.tabs[i].scene.deselect_entities(&handles);
        } else {
            self.tabs[i].scene.select_entities(&handles);
            self.tabs[i].scene.expand_selection_for_groups(&handles);
        }
        self.refresh_properties();
        let selected: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        self.feed_command(StepInput::SelectionComplete(selected))
    }

    fn handle_select_by_path(
        &mut self,
        path: Vec<[f64; 2]>,
        closed: bool,
        crossing: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        // The command picked the path; the hit test lives here, where
        // the camera and the drawing's geometry are. Everything after
        // matches what a lasso does, Remove included.
        let canvas = self.tabs[i].scene.selection.borrow().view.vp_size;
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
        self.last_pick = Some((handles.clone(), self.select_remove_mode));
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
        self.feed_command(StepInput::SelectionComplete(selected))
    }

    #[inline(never)]
    fn handle_pdf_password(
        &mut self,
        path: String,
        input: String,
        stage: crate::scene::model::pdf_password::Stage,
        remembered: bool,
        attach: bool,
    ) -> Task<Message> {
        use crate::ui::window::pdf_password_dialog::PdfPasswordThen;
        let then = PdfPasswordThen::Command { input, attach };
        self.open_pdf_password(&path, stage, remembered, then)
    }

    fn handle_relaunch(&mut self, cmd: String, handles: Vec<Handle>) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.deselect_all();
        for h in &handles {
            self.tabs[i].scene.select_entity(*h, false);
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.dispatch_command(&cmd)
    }

    fn handle_dispatch(&mut self, cmd: String) -> Task<Message> {
        let i = self.active_tab;
        // End this interactive front-end, then run the assembled command
        // through the normal dispatcher. Selection is left untouched.
        // The option picked in DIMCONSTRAINT becomes its next default;
        // the same command run on its own leaves the default alone.
        // DIM's Offset sets the drawing's DIMDLI; the series goes on.
        if let Some(value) = cmd
            .strip_prefix("_DIM_SET_DIMDLI ")
            .and_then(|value| value.trim().parse::<f64>().ok())
        {
            self.tabs[i].scene.document.header.dim_line_increment = value;
            self.tabs[i].dirty = true;
            if let Some(command) = self.tabs[i].active_cmd.as_ref() {
                let (prompt, options) = (command.prompt(), command.options());
                self.command_line.push_info(&prompt);
                self.command_line.set_step_options(options);
            }
            return Task::none();
        }
        // DIM's answer about the dimensions it placed one over; DIM (or its
        // continued series) goes on.
        if let Some(rest) = cmd.strip_prefix("_DIM_OVERLAP ") {
            let mut parts = rest.split_whitespace();
            let choice = parts.next().and_then(|v| v.chars().next()).unwrap_or('N');
            let new = parts
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .map(codec::Handle::new);
            let others: Vec<codec::Handle> = parts
                .next()
                .unwrap_or("")
                .split(',')
                .filter_map(|v| v.parse::<u64>().ok())
                .map(codec::Handle::new)
                .collect();
            if let (Some(new), false) = (new, others.is_empty() || choice == 'N') {
                let dimdli = crate::scene::creation_style::drawing_dimdli(
                    &self.tabs[i].scene.document,
                );
                self.push_undo_snapshot(i, "DIM");
                dim_edit::apply_dim_overlap(&mut self.tabs[i].scene, choice, new, &others, dimdli);
                self.tabs[i].dirty = true;
            }
            if let Some(command) = self.tabs[i].active_cmd.as_ref() {
                let (prompt, options) = (command.prompt(), command.options());
                self.command_line.push_info(&prompt);
                self.command_line.set_step_options(options);
            }
            return Task::none();
        }
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
        // The command handing off is already in the Repeat menu and ↑/↓
        // recall; the internal line it hands to (`BLOCK_POINT_PICKED x y z`)
        // must not land there too (#2170).
        let recent = self.command_line.recent_commands.clone();
        let recall = self.command_line.cmd_recall.clone();
        let task = self.dispatch_command(&cmd);
        self.command_line.recent_commands = recent;
        self.command_line.cmd_recall = recall;
        task
    }

    fn handle_script(&mut self, script: String) -> Task<Message> {
        let i = self.active_tab;
        // A plugin command's script starts from an empty selection.
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.name() == "PLUGIN")
        {
            self.tabs[i].scene.deselect_all();
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        // Each line answers the prompts the previous one left open.
        for line in script.lines() {
            let line = line.trim();
            if !line.is_empty() {
                let task = self.run_command_line_streaming(line, true);
                let _ = self.drive_headless_task(task);
            }
        }
        Task::none()
    }

    fn handle_measurement(&mut self, msg: String) {
        self.remember_measurement(&msg);
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        if !msg.is_empty() {
            self.command_line.push_output(&msg);
        }
    }

    fn handle_report_measurement(&mut self, msg: String) {
        self.remember_measurement(&msg);
        let i = self.active_tab;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.refresh_area_preview(i);
        self.command_line.push_output(&msg);
        if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
            self.command_line.push_info(&prompt);
        }
    }

    fn handle_report_error(&mut self, msg: String) {
        let i = self.active_tab;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.command_line.push_error(&msg);
        if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
            self.command_line.push_info(&prompt);
        }
    }

    fn handle_report_measurement_and_deselect(&mut self, msg: String) {
        self.remember_measurement(&msg);
        let i = self.active_tab;
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

    fn handle_deselect_and_continue(&mut self) {
        let i = self.active_tab;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.clear_preview_wire();
        self.refresh_area_preview(i);
        self.refresh_properties();
        if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
            self.command_line.push_info(&prompt);
        }
    }

    fn handle_undo_document(&mut self) {
        let i = self.active_tab;
        let active = self.tabs[i].active_cmd.take();
        self.undo_active_tab();
        self.tabs[i].active_cmd = active;
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.name() == "PEDIT")
        {
            let entities: Vec<_> = self.tabs[i].scene.document.entities().cloned().collect();
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

    fn handle_return_point(&mut self) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
    }
    // --- C2a handlers: group / table (thin-router targets, bodies moved verbatim) ---

    fn handle_create_group(
        &mut self,
        mut handles: Vec<Handle>,
        name: String,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        if handles.is_empty() {
            return Some(Task::none());
        }
        let undo = self.begin_group_undo(i, "GROUP");
        self.tabs[i].scene.create_group(name.clone(), handles);
        self.tabs[i].dirty = true;
        self.commit_group_undo(i, undo);
        self.command_line
            .push_info(crate::tf!("Group \"{}\" created.", name).as_ref());
        None
    }

    fn handle_delete_groups(&mut self, mut handles: Vec<Handle>) -> Option<Task<Message>> {
        let i = self.active_tab;
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        if handles.is_empty() {
            return Some(Task::none());
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
        None
    }

    fn handle_edit_table_cell(
        &mut self,
        handle: Handle,
        point: glam::DVec3,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        // TABLEDIT's pick: end the pick phase and hand (table, point)
        // to the shared cell-edit launcher. A miss or a locked cell
        // re-prompts until a valid table cell is selected.
        use crate::modules::annotate::table_cmd::{table_cell_at, TableCellEditStart};
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();

        if handle == codec::Handle::NULL && self.selection_cycling {
            let mut candidates = Vec::new();
            for entity in self.tabs[i].scene.document.entities() {
                if let codec::EntityType::Table(table) = entity {
                    let h = table.common.handle;
                    let style = table.table_style_handle.and_then(|style_handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .objects
                            .get(&style_handle)
                            .and_then(|object| match object {
                                codec::objects::ObjectType::TableStyle(style) => Some(style),
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
                return Some(Task::none());
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
        None
    }
    // --- C2a handlers: mleader (thin-router targets, bodies moved verbatim) ---

    fn handle_set_quick_dimension_snap_priority(&mut self, priority: u8) {
        let i = self.active_tab;
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

    fn handle_align_mleaders(
        &mut self,
        mut handles: Vec<Handle>,
        reference: Option<Handle>,
        mode: crate::modules::annotate::mleader_edit::MLeaderAlignMode,
        spacing: f64,
        from: glam::DVec3,
        to: glam::DVec3,
    ) {
        let i = self.active_tab;
        handles.retain(|handle| {
            !self.tabs[i].scene.is_layer_locked(*handle)
                && matches!(
                    self.tabs[i].scene.document.get_entity(*handle),
                    Some(codec::EntityType::MultiLeader(_))
                )
        });
        if !handles.is_empty() {
            self.push_undo_snapshot(i, "MLEADERALIGN");
            let request = MLeaderAlign {
                reference,
                mode,
                spacing,
                from,
                to,
            };
            if apply_mleader_align(&mut self.tabs[i].scene, &handles, &request) {
                self.tabs[i].dirty = true;
            }
        }
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
    }

    fn handle_collect_mleaders(
        &mut self,
        handles: Vec<Handle>,
        point: glam::DVec3,
        layout: crate::modules::annotate::mleader_edit::MLeaderCollectLayout,
    ) {
        let i = self.active_tab;
        let collected: Vec<Handle> = handles
            .into_iter()
            .filter(|handle| {
                !self.tabs[i].scene.is_layer_locked(*handle)
                    && matches!(
                        self.tabs[i].scene.document.get_entity(*handle),
                        Some(codec::EntityType::MultiLeader(leader))
                            if leader.content_type == codec::entities::LeaderContentType::Block
                    )
            })
            .collect();
        if !collected.is_empty() {
            self.push_undo_snapshot(i, "MLEADERCOLLECT");
            if apply_mleader_collect(&mut self.tabs[i].scene, &collected, point, layout) {
                self.tabs[i].dirty = true;
            }
        }
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
    }
    // --- C2a handlers: property_match (thin-router targets, bodies moved verbatim) ---

    // --- C2a handlers: viewport (thin-router targets, bodies moved verbatim) ---

    // --- C2a handlers: text_edit (thin-router targets, bodies moved verbatim) ---

    // --- C2b handlers: entity_commit (thin-router targets, bodies moved verbatim) ---

    // --- C2b handlers: clipboard (thin-router targets, bodies moved verbatim) ---

    /// Restore the tangent-snap / ortho state that was in effect before the command started.
    /// Recreate clipboard-dependency records (layer / linetype / text + dim
    /// style) in tab `i`'s document for any the copied entities reference but
    /// this drawing doesn't already have. Each recreated record gets a fresh
    /// handle from the target document so it can't collide with an existing
    /// one. No-op for same-document pastes (the records already exist). (#129)
    pub(super) fn merge_dependencies(&mut self, i: usize, deps: &crate::app::ClipboardDeps) {
        if deps.is_empty() {
            return;
        }
        // Single shared copy — see `clipboard::merge_table_records`.
        clipboard::merge_table_records(&mut self.tabs[i].scene.document, deps);
    }

    // --- C2d handlers: constraint (thin-router targets, bodies moved verbatim) ---

    // --- C2d handlers: dim_edit (thin-router targets, bodies moved verbatim) ---

    pub(super) fn restore_pre_cmd_tangent(&mut self) {
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
    document: &codec::Document,
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
        let distance =
            match crate::scene::viewport_dimension_pick::planar_pick_distance(entity, point) {
                Some(distance) => distance,
                // A multileader answers by its leader lines and landings.
                None if matches!(entity, codec::EntityType::MultiLeader(_)) => match entity {
                    codec::EntityType::MultiLeader(ml) => {
                        crate::modules::annotate::mleader_edit::mleader_pick_distance(
                            ml,
                            codec::types::Vector3::new(point.x, point.y, point.z),
                        )
                    }
                    _ => continue,
                },
                // A text or an ellipse answers by its extent, the way a click
                // inside it picks it.
                None if matches!(
                    entity,
                    codec::EntityType::Text(_)
                        | codec::EntityType::MText(_)
                        | codec::EntityType::Ellipse(_)
                ) =>
                {
                    let dx = (bounds.min.x - point.x)
                        .max(point.x - bounds.max.x)
                        .max(0.0);
                    let dy = (bounds.min.y - point.y)
                        .max(point.y - bounds.max.y)
                        .max(0.0);
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

// ── DIMSPACE helper ───────────────────────────────────────────────────────────

/// One MLEADERALIGN run, as the command gathered it.
struct MLeaderAlign {
    reference: Option<codec::Handle>,
    mode: crate::modules::annotate::mleader_edit::MLeaderAlignMode,
    spacing: f64,
    from: glam::DVec3,
    to: glam::DVec3,
}

/// Where a multileader's content meets its leader: the end of its first
/// landing, else the stored content base point.
fn mleader_base(ml: &codec::entities::MultiLeader) -> glam::DVec2 {
    let ctx = &ml.context;
    if let Some(root) = ctx.leader_roots.first() {
        let d = &root.direction;
        let len = (d.x * d.x + d.y * d.y).sqrt();
        if len > 1.0e-12 && ml.enable_landing && ml.enable_dogleg {
            let reach = root.landing_distance.max(0.0) / len;
            return glam::DVec2::new(
                root.connection_point.x + d.x * reach,
                root.connection_point.y + d.y * reach,
            );
        }
        if ctx.content_base_point == codec::types::Vector3::ZERO {
            return glam::DVec2::new(root.connection_point.x, root.connection_point.y);
        }
    }
    glam::DVec2::new(ctx.content_base_point.x, ctx.content_base_point.y)
}

/// Move a multileader's content (and the landings that hold it) by `shift`;
/// the arrowheads stay where they point.
fn shift_mleader_content(ml: &mut codec::entities::MultiLeader, shift: glam::DVec2) {
    let ctx = &mut ml.context;
    for point in [
        &mut ctx.content_base_point,
        &mut ctx.text_location,
        &mut ctx.block_content_location,
    ] {
        point.x += shift.x;
        point.y += shift.y;
    }
    for root in &mut ctx.leader_roots {
        root.connection_point.x += shift.x;
        root.connection_point.y += shift.y;
    }
}

/// Size of a multileader's content measured along `dir`.
fn mleader_extent_along(ml: &codec::entities::MultiLeader, dir: glam::DVec2) -> f64 {
    let ctx = &ml.context;
    let height = match ml.content_type {
        codec::entities::LeaderContentType::MText => {
            let lines = ctx.text_string.split("\\P").count().max(1) as f64;
            let h = if ctx.text_height > 0.0 { ctx.text_height } else { ml.text_height };
            h * (1.0 + (lines - 1.0) * 5.0 / 3.0 * ctx.line_spacing_factor.max(0.0))
        }
        _ => ctx.block_content_scale.y.abs().max(1.0e-9),
    };
    let width = ctx.text_width.max(0.0);
    let td = &ctx.text_direction;
    let (ux, uy) = {
        let l = (td.x * td.x + td.y * td.y).sqrt();
        if l > 1.0e-9 { (td.x / l, td.y / l) } else { (1.0, 0.0) }
    };
    width * (dir.x * ux + dir.y * uy).abs() + height * (-dir.x * uy + dir.y * ux).abs()
}

fn apply_mleader_align(
    scene: &mut crate::scene::Scene,
    handles: &[codec::Handle],
    request: &MLeaderAlign,
) -> bool {
    use crate::modules::annotate::mleader_edit::MLeaderAlignMode as Mode;
    let leader = |scene: &crate::scene::Scene, h: codec::Handle| match scene.document.get_entity(h) {
        Some(codec::EntityType::MultiLeader(ml)) => Some((**ml).clone()),
        _ => None,
    };
    let mut moves: Vec<(codec::Handle, glam::DVec2)> = Vec::new();
    match request.mode {
        Mode::Distribute => {
            let a = glam::DVec2::new(request.from.x, request.from.y);
            let b = glam::DVec2::new(request.to.x, request.to.y);
            let span = b - a;
            if span.length_squared() <= 1.0e-18 {
                return false;
            }
            let mut order: Vec<(f64, codec::Handle, glam::DVec2)> = handles
                .iter()
                .filter_map(|&h| leader(scene, h).map(|ml| (h, mleader_base(&ml))))
                .map(|(h, base)| ((base - a).dot(span), h, base))
                .collect();
            order.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
            let last = order.len().saturating_sub(1).max(1) as f64;
            for (k, (_, h, base)) in order.into_iter().enumerate() {
                let target = a + span * (k as f64 / last);
                moves.push((h, target - base));
            }
        }
        Mode::Parallel => {
            let Some(reference) = request.reference.and_then(|h| leader(scene, h)) else {
                return false;
            };
            let segment = |ml: &codec::entities::MultiLeader| {
                let root = ml.context.leader_roots.first()?;
                let line = root.lines.first()?;
                let start = *line.points.last()?;
                let end = root.connection_point;
                Some((
                    glam::DVec2::new(start.x, start.y),
                    glam::DVec2::new(end.x, end.y),
                ))
            };
            let Some((ra, rb)) = segment(&reference) else {
                return false;
            };
            let Some(dir) = (rb - ra).try_normalize() else {
                return false;
            };
            for &h in handles {
                if Some(h) == request.reference {
                    continue;
                }
                let Some(ml) = leader(scene, h) else { continue };
                let Some((a, b)) = segment(&ml) else { continue };
                let target = a + dir * (b - a).length();
                moves.push((h, target - b));
            }
        }
        Mode::UseCurrent | Mode::Spacing => {
            let Some(reference) = request.reference.and_then(|h| leader(scene, h)) else {
                return false;
            };
            let origin = mleader_base(&reference);
            let Some(dir) = (glam::DVec2::new(request.to.x, request.to.y) - origin).try_normalize()
            else {
                return false;
            };
            let mut along: Vec<(f64, codec::Handle, glam::DVec2, f64)> = handles
                .iter()
                .filter(|&&h| Some(h) != request.reference)
                .filter_map(|&h| leader(scene, h).map(|ml| (h, ml)))
                .map(|(h, ml)| {
                    let base = mleader_base(&ml);
                    ((base - origin).dot(dir), h, base, mleader_extent_along(&ml, dir))
                })
                .collect();
            if request.mode == Mode::UseCurrent {
                for (t, h, base, _) in along {
                    moves.push((h, origin + dir * t - base));
                }
            } else {
                along.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
                let reference_extent = mleader_extent_along(&reference, dir);
                // Ahead of the reference, then behind it, each content one
                // spacing clear of the one before.
                let mut position = 0.0;
                let mut previous = reference_extent;
                for (_, h, base, extent) in along.iter().filter(|e| e.0 >= 0.0) {
                    position += previous + request.spacing;
                    previous = *extent;
                    moves.push((*h, origin + dir * position - *base));
                }
                let mut position = 0.0;
                for (_, h, base, extent) in along.iter().rev().filter(|e| e.0 < 0.0) {
                    position -= extent + request.spacing;
                    moves.push((*h, origin + dir * position - *base));
                }
            }
        }
    }
    let mut changed = Vec::new();
    for (handle, shift) in moves {
        if shift.length_squared() <= 1.0e-24 {
            continue;
        }
        if let Some(codec::EntityType::MultiLeader(ml)) = scene.document.get_entity_mut(handle) {
            shift_mleader_content(ml, shift);
            changed.push((handle, crate::scene::ChangeKind::Modified));
        }
    }
    if !changed.is_empty() {
        scene.bump_entities(&changed);
    }
    !changed.is_empty()
}

/// Lay block multileaders out from `point` in a row, a column or wrapped
/// rows; the last one selected lends its leader to the collection, the rest
/// lose theirs.
fn apply_mleader_collect(
    scene: &mut crate::scene::Scene,
    handles: &[codec::Handle],
    point: glam::DVec3,
    layout: crate::modules::annotate::mleader_edit::MLeaderCollectLayout,
) -> bool {
    use crate::modules::annotate::mleader_edit::MLeaderCollectLayout as Layout;
    let sizes: Vec<(codec::Handle, glam::DVec2)> = handles
        .iter()
        .filter_map(|&h| {
            let codec::EntityType::MultiLeader(ml) = scene.document.get_entity(h)? else {
                return None;
            };
            let name = ml
                .block_content_handle
                .and_then(|bh| scene.document.block_records.iter().find(|b| b.handle == bh))
                .map(|b| b.name.clone())?;
            let bounds = crate::app::helpers::block_definition_bounds(&scene.document, &name)?;
            let scale = ml.context.block_content_scale;
            Some((
                h,
                glam::DVec2::new(
                    (bounds.max.x - bounds.min.x) * scale.x.abs(),
                    (bounds.max.y - bounds.min.y) * scale.y.abs(),
                ),
            ))
        })
        .collect();
    if sizes.is_empty() {
        return false;
    }
    let leader_source = *handles.last().unwrap_or(&sizes[0].0);
    let arrow_lines = match scene.document.get_entity(leader_source) {
        Some(codec::EntityType::MultiLeader(ml)) => ml
            .context
            .leader_roots
            .first()
            .map(|root| (root.lines.clone(), root.direction, root.landing_distance)),
        _ => None,
    };
    // Content centres, from the picked point.
    let mut centres = Vec::new();
    let (mut x, mut y, mut row_height) = (point.x, point.y, 0.0f64);
    for (index, (_, size)) in sizes.iter().enumerate() {
        match layout {
            Layout::Vertical => {
                if index > 0 {
                    y += sizes[index - 1].1.y * 0.5 + size.y * 0.5;
                }
                centres.push(glam::DVec2::new(point.x, y));
            }
            Layout::Horizontal | Layout::Wrap(_) | Layout::WrapNumber(_) => {
                if index > 0 {
                    x += sizes[index - 1].1.x * 0.5 + size.x * 0.5;
                }
                let wrap = match layout {
                    Layout::Wrap(width) => width > 0.0 && x + size.x * 0.5 - point.x > width,
                    Layout::WrapNumber(count) => index % count.max(1) as usize == 0,
                    _ => false,
                };
                if wrap && index > 0 {
                    x = point.x;
                    y -= row_height;
                    row_height = 0.0;
                }
                row_height = row_height.max(size.y);
                centres.push(glam::DVec2::new(x, y));
            }
        }
    }
    // The leader joins the first content in a row and the top one in a column.
    let carrier = match layout {
        Layout::Vertical => sizes.len() - 1,
        _ => 0,
    };
    let mut changed = Vec::new();
    for (index, ((handle, _), centre)) in sizes.iter().zip(&centres).enumerate() {
        let Some(codec::EntityType::MultiLeader(ml)) = scene.document.get_entity_mut(*handle)
        else {
            continue;
        };
        let old = ml.context.block_content_location;
        let shift = glam::DVec2::new(centre.x - old.x, centre.y - old.y);
        shift_mleader_content(ml, shift);
        if index == carrier {
            if let Some((lines, direction, landing)) = arrow_lines.clone() {
                let d = glam::DVec2::new(direction.x, direction.y)
                    .try_normalize()
                    .unwrap_or(glam::DVec2::X);
                let base = glam::DVec2::new(
                    ml.context.content_base_point.x,
                    ml.context.content_base_point.y,
                );
                let connection = base - d * landing;
                ml.context.leader_roots.truncate(1);
                if let Some(root) = ml.context.leader_roots.first_mut() {
                    root.lines = lines;
                    root.connection_point.x = connection.x;
                    root.connection_point.y = connection.y;
                }
            }
        } else {
            ml.context.leader_roots.clear();
        }
        changed.push((*handle, crate::scene::ChangeKind::Modified));
    }
    scene.bump_entities(&changed);
    true
}
