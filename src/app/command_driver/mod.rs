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
//! - `property_match.rs` — MATCHPROP/LAYMATCH + `match_special_props`.
//! - `entity_commit.rs` — entity/dimension/hatch/block commit arms.
//! - `solid3d.rs` — extrude/thicken/presspull/revolve/sweep/loft/shell/boolean/slice.
//! - `viewport.rs` — MVIEW/layout/plot/zoom/print arms.
//! - `transform.rs` — MOVE/COPY/ALIGN/STRETCH arms.
//! - `clipboard.rs` — COPYCLIP/PASTECLIP/PASTEORIG/PASTEBLOCK arms,
//!   clipboard merge + xdict helpers; re-exports
//!   `remap_ext_subtree_reference` for `app::commands::blocks`.
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
use acadrust::Handle;
use iced::Task;

mod utilities;
mod parameters;
mod tests;
mod property_match;
mod entity_commit;
mod solid3d;
mod viewport;
mod transform;
mod clipboard;
mod modify;
mod text_edit;
mod constraint;
mod dim_edit;

pub(crate) use clipboard::remap_ext_subtree_reference;

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
            } => {
                if let Some(task) = self.handle_mview_create_clipped(boundary, boundary_handle) {
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
                if let Some(task) = self.handle_commit_and_exit(entity) {
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
            CmdResult::AlignMLeaders { handles, from, to } => {
                self.handle_align_mleaders(handles, from, to);
            }
            CmdResult::CollectMLeaders { handles, point } => {
                self.handle_collect_mleaders(handles, point);
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
            CmdResult::UpdateEntityAndFinish { handle, entity } => {
                if let Some(task) = self.handle_update_entity_and_finish(handle, entity) {
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
            CmdResult::EditDimensionBreak {
                dimensions,
                operation,
            } => {
                self.handle_edit_dimension_break(dimensions, operation);
            }
            CmdResult::EditDimensionJog { dimension, point } => {
                self.handle_edit_dimension_jog(dimension, point);
            }
            CmdResult::SpaceDimensions {
                base,
                others,
                spacing,
            } => {
                self.handle_space_dimensions(base, others, spacing);
            }
            CmdResult::ReplaceEntity(handle, new_entities) => {
                if let Some(task) = self.handle_replace_entity(handle, new_entities) {
                    return task;
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
            CmdResult::LengthenEntity { handle, pick_pt, mode } => {
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
            CmdResult::StretchWindow { handles, windows } => {
                self.handle_stretch_window(handles, windows);
            }
            CmdResult::StretchEntities { handles, windows, delta } => {
                if let Some(task) = self.handle_stretch_entities(handles, windows, delta) {
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
            CmdResult::PresspullApply { targets, distance, color: _ } => {
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

            result @ CmdResult::SolidEdgeBlend { .. } => return self.handle_solid_edge_blend(result),

            CmdResult::SolidShell { handle, actions, distance } => return self.handle_solid_shell(handle, actions, distance),

            CmdResult::SolidSubtract { bases, cutters, convert_meshes } => return self.handle_solid_subtract(bases, cutters, convert_meshes),

            CmdResult::SliceEntities { targets, plane, keep_point } => return self.handle_slice_entities(targets, plane, keep_point),

            CmdResult::SliceSurfaceEntities { targets, cutter, keep_point } => return self.handle_slice_surface_entities(targets, cutter, keep_point),

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

    fn handle_cancel(&mut self, cancel: CmdResult) {
        let i = self.active_tab;
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

    fn handle_cancel_with_message(&mut self, message: String) -> Task<Message> {
        self.command_line.push_error(&message);
        self.apply_cmd_result(CmdResult::Cancel)
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
        self.feed_command(StepInput::SelectionComplete(selected))
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
        self.dispatch_command(&cmd)
    }

    fn handle_measurement(&mut self, msg: String) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.command_line.push_output(&msg);
    }

    fn handle_report_measurement(&mut self, msg: String) {
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
                                acadrust::objects::ObjectType::TableStyle(style) => Some(style),
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
        from: glam::DVec3,
        to: glam::DVec3,
    ) {
        let i = self.active_tab;
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

    fn handle_collect_mleaders(&mut self, handles: Vec<Handle>, point: glam::DVec3) {
        let i = self.active_tab;
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


// ── DIMSPACE helper ───────────────────────────────────────────────────────────


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


