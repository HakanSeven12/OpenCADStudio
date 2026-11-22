// Plugin interactive-command adapters and preview conversion.

use super::*;

fn plugin_input_kind_to_host(
    kind: ocs_plugin_api::host::InteractiveInputKind,
) -> crate::command::InputKind {
    match kind {
        ocs_plugin_api::host::InteractiveInputKind::Point => crate::command::InputKind::Point,
        ocs_plugin_api::host::InteractiveInputKind::SingleToken => {
            crate::command::InputKind::SingleToken
        }
        ocs_plugin_api::host::InteractiveInputKind::FreeText => {
            crate::command::InputKind::FreeText
        }
    }
}

/// Bridges a plugin's [`InteractiveCommand`](ocs_plugin_api::host::InteractiveCommand)
/// to the host's internal `CadCommand`, so a plugin tool drives the host's
/// point-collection flow (viewport clicks or `--serve` coordinates) just like a
/// built-in tool.
pub(super) struct PluginInteractiveAdapter {
    pub(super) inner: Box<dyn ocs_plugin_api::host::InteractiveCommand>,
}

impl crate::command::CadCommand for PluginInteractiveAdapter {
    fn name(&self) -> &'static str {
        "PLUGIN"
    }
    fn preserve_commit_style(&self) -> bool {
        true
    }
    fn preserve_commit_layer(&self) -> bool {
        true
    }
    // Every call into the plugin runs under a panic guard (#145): a buggy plugin
    // that panics mid-command leaves the host running — the command just ends.
    fn prompt(&self) -> String {
        crate::plugin::guard("InteractiveCommand::prompt", || self.inner.prompt())
            .unwrap_or_default()
    }
    fn on_point(&mut self, pt: glam::DVec3) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_point", || {
            self.inner.on_point([pt.x, pt.y, pt.z])
        })
        .map(plugin_step_to_result)
        .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn on_enter(&mut self) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_enter", || self.inner.on_enter())
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn on_undo_step(&mut self) -> Option<crate::command::CmdResult> {
        crate::plugin::guard("InteractiveCommand::on_undo_step", || {
            self.inner.on_undo_step()
        })
        .flatten()
        .map(plugin_step_to_result)
    }
    fn needs_entity_pick(&self) -> bool {
        crate::plugin::guard("InteractiveCommand::needs_object_pick", || {
            self.inner.needs_object_pick()
        })
        .unwrap_or(false)
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: glam::DVec3) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_object_pick", || {
            self.inner.on_object_pick(handle, [pt.x, pt.y, pt.z])
        })
        .map(plugin_step_to_result)
        .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn on_preview_wires(
        &mut self,
        pt: glam::DVec3,
    ) -> Vec<crate::scene::model::wire_model::WireModel> {
        let raw_wires = crate::plugin::guard("InteractiveCommand::on_cursor_move", || {
            self.inner.on_cursor_move([pt.x, pt.y, pt.z])
        })
        .unwrap_or_default();
        convert_preview_wires(raw_wires)
    }
    fn is_selection_gathering(&self) -> bool {
        crate::plugin::guard("InteractiveCommand::is_selection_gathering", || {
            self.inner.is_selection_gathering()
        })
        .unwrap_or(false)
    }
    fn accepts_drag_selection(&self) -> bool {
        crate::plugin::guard("InteractiveCommand::accepts_drag_selection", || {
            self.inner.accepts_drag_selection()
        })
        .unwrap_or(false)
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_selection_complete", || {
            self.inner.on_selection_complete(handles)
        })
        .map(plugin_step_to_result)
        .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn selection_rejects_locked_layers(&self) -> bool {
        crate::plugin::guard(
            "InteractiveCommand::selection_rejects_locked_layers",
            || self.inner.selection_rejects_locked_layers(),
        )
        .unwrap_or(false)
    }
    fn entity_pick_highlights_hover(&self) -> bool {
        self.needs_entity_pick() || self.is_selection_gathering()
    }
    fn modal_dialog(&self) -> Option<ocs_plugin_api::dialog::DialogDef> {
        crate::plugin::guard("InteractiveCommand::modal_dialog", || {
            self.inner.modal_dialog()
        })
        .unwrap_or(None)
    }
    fn on_dialog_response(
        &mut self,
        response: ocs_plugin_api::dialog::DialogResponse,
    ) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_dialog_response", || {
            self.inner.on_dialog_response(response)
        })
        .map(plugin_step_to_result)
        .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn on_dialog_action(
        &mut self,
        event: ocs_plugin_api::dialog::DialogEvent,
    ) -> ocs_plugin_api::dialog::DialogActionResponse {
        crate::plugin::guard("InteractiveCommand::on_dialog_action", || {
            self.inner.on_dialog_action(event)
        })
        .unwrap_or(ocs_plugin_api::dialog::DialogActionResponse::None)
    }
    fn take_pending_panel_updates(
        &mut self,
    ) -> Vec<(
        String,
        std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>,
    )> {
        crate::plugin::guard("InteractiveCommand::take_pending_panel_updates", || {
            self.inner.take_pending_panel_updates()
        })
        .unwrap_or_default()
    }
    fn input_kind(&self) -> crate::command::InputKind {
        crate::plugin::guard("InteractiveCommand::input_kind", || self.inner.input_kind())
            .map(plugin_input_kind_to_host)
            .unwrap_or_default()
    }
    fn on_text_input(&mut self, text: &str) -> Option<crate::command::CmdResult> {
        if !self.input_kind().wants_text() {
            return None;
        }
        Some(
            crate::plugin::guard("InteractiveCommand::on_text_input", || {
                self.inner.on_text_input(text)
            })
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel),
        )
    }
}

/// Bridges an out-of-process plugin's interactive command to the host's
/// `CadCommand`. Events are sent over IPC and the returned `CommandStep` is
/// translated into a `CmdResult`. Prompt and object-pick mode are cached and
/// refreshed after each event.
pub(crate) struct PluginProcessInteractiveAdapter {
    pub process: std::sync::Arc<ocs_plugin_api::process::PluginProcess>,
    pub command_id: u64,
    prompt: Option<String>,
    input_kind: ocs_plugin_api::host::InteractiveInputKind,
    needs_entity_pick: Option<bool>,
    is_selection_gathering: Option<bool>,
    accepts_drag_selection: Option<bool>,
    selection_rejects_locked_layers: Option<bool>,
    is_done: bool,
}

impl PluginProcessInteractiveAdapter {
    pub(crate) fn new(
        process: std::sync::Arc<ocs_plugin_api::process::PluginProcess>,
        command_id: u64,
    ) -> Self {
        let prompt = process.get_prompt(command_id).ok();
        let input_kind = process.get_input_kind(command_id).unwrap_or_default();
        let needs_entity_pick = process.needs_entity_pick(command_id).ok();
        let is_selection_gathering = process.is_selection_gathering(command_id).ok();
        let accepts_drag_selection = process.accepts_drag_selection(command_id).ok();
        let selection_rejects_locked_layers =
            process.selection_rejects_locked_layers(command_id).ok();
        Self {
            process,
            command_id,
            prompt,
            input_kind,
            needs_entity_pick,
            is_selection_gathering,
            accepts_drag_selection,
            selection_rejects_locked_layers,
            is_done: false,
        }
    }

    fn refresh(&mut self) {
        self.prompt = self.process.get_prompt(self.command_id).ok();
        self.input_kind = self.process.get_input_kind(self.command_id).unwrap_or_default();
        self.needs_entity_pick = self.process.needs_entity_pick(self.command_id).ok();
        self.is_selection_gathering = self.process.is_selection_gathering(self.command_id).ok();
        self.accepts_drag_selection = self.process.accepts_drag_selection(self.command_id).ok();
        self.selection_rejects_locked_layers = self
            .process
            .selection_rejects_locked_layers(self.command_id)
            .ok();
    }

    fn cancel(&mut self) {
        if !self.is_done {
            self.is_done = true;
            use ocs_plugin_api::ipc::protocol::InteractiveEvent;
            let _ = self
                .process
                .interactive_event(self.command_id, InteractiveEvent::Cancel);
        }
    }
}

impl Drop for PluginProcessInteractiveAdapter {
    fn drop(&mut self) {
        // Tell the command it was cancelled before the runner forgets it:
        // a plugin waiting on a step would otherwise never be released.
        self.cancel();
        let _ = self.process.drop_interactive(self.command_id);
    }
}

impl crate::command::CadCommand for PluginProcessInteractiveAdapter {
    fn name(&self) -> &'static str {
        "PLUGIN"
    }
    fn preserve_commit_style(&self) -> bool {
        true
    }
    fn preserve_commit_layer(&self) -> bool {
        true
    }
    fn prompt(&self) -> String {
        self.prompt.clone().unwrap_or_default()
    }
    fn on_point(&mut self, pt: glam::DVec3) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(self.command_id, InteractiveEvent::Point([pt.x, pt.y, pt.z]))
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(
            result,
            crate::command::CmdResult::CommitAndExit(_)
                | crate::command::CmdResult::CommitEntitiesAndExit(_)
                | crate::command::CmdResult::Done
                | crate::command::CmdResult::Script(_)
                | crate::command::CmdResult::Cancel
        ) {
            self.is_done = true;
        } else {
            self.refresh();
        }
        result
    }
    fn on_enter(&mut self) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(self.command_id, InteractiveEvent::Enter)
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(
            result,
            crate::command::CmdResult::CommitAndExit(_)
                | crate::command::CmdResult::CommitEntitiesAndExit(_)
                | crate::command::CmdResult::Done
                | crate::command::CmdResult::Script(_)
                | crate::command::CmdResult::Cancel
        ) {
            self.is_done = true;
        } else {
            self.refresh();
        }
        result
    }
    fn on_undo_step(&mut self) -> Option<crate::command::CmdResult> {
        if self.is_done {
            return None;
        }
        // `None` (the command declines, the plugin predates API v24, or the
        // call failed) leaves Ctrl+Z to the document undo.
        let step = self.process.undo_step(self.command_id).ok().flatten()?;
        let result = plugin_step_to_result(step);
        if matches!(
            result,
            crate::command::CmdResult::CommitAndExit(_)
                | crate::command::CmdResult::CommitEntitiesAndExit(_)
                | crate::command::CmdResult::Done
                | crate::command::CmdResult::Script(_)
                | crate::command::CmdResult::Cancel
        ) {
            self.is_done = true;
        } else {
            self.refresh();
        }
        Some(result)
    }
    fn on_escape(&mut self) -> crate::command::CmdResult {
        self.cancel();
        crate::command::CmdResult::Cancel
    }
    fn needs_entity_pick(&self) -> bool {
        self.needs_entity_pick.unwrap_or(false)
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: glam::DVec3) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(
                self.command_id,
                InteractiveEvent::ObjectPick {
                    handle,
                    pt: [pt.x, pt.y, pt.z],
                },
            )
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(
            result,
            crate::command::CmdResult::CommitAndExit(_)
                | crate::command::CmdResult::CommitEntitiesAndExit(_)
                | crate::command::CmdResult::Done
                | crate::command::CmdResult::Script(_)
                | crate::command::CmdResult::Cancel
        ) {
            self.is_done = true;
        } else {
            self.refresh();
        }
        result
    }
    fn on_preview_wires(
        &mut self,
        pt: glam::DVec3,
    ) -> Vec<crate::scene::model::wire_model::WireModel> {
        let raw_wires = self
            .process
            .on_cursor_move(self.command_id, [pt.x, pt.y, pt.z])
            .unwrap_or_default();
        convert_preview_wires(raw_wires)
    }
    fn is_selection_gathering(&self) -> bool {
        self.is_selection_gathering.unwrap_or(false)
    }
    fn accepts_drag_selection(&self) -> bool {
        self.accepts_drag_selection.unwrap_or(false)
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(
                self.command_id,
                InteractiveEvent::SelectionComplete(handles),
            )
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(
            result,
            crate::command::CmdResult::CommitAndExit(_)
                | crate::command::CmdResult::CommitEntitiesAndExit(_)
                | crate::command::CmdResult::Done
                | crate::command::CmdResult::Script(_)
                | crate::command::CmdResult::Cancel
        ) {
            self.is_done = true;
        } else {
            self.refresh();
        }
        result
    }
    fn entity_pick_highlights_hover(&self) -> bool {
        self.needs_entity_pick.unwrap_or(false) || self.is_selection_gathering.unwrap_or(false)
    }
    fn selection_rejects_locked_layers(&self) -> bool {
        self.selection_rejects_locked_layers.unwrap_or(false)
    }
    fn modal_dialog(&self) -> Option<ocs_plugin_api::dialog::DialogDef> {
        self.process
            .get_modal_dialog(self.command_id)
            .ok()
            .flatten()
    }
    fn on_dialog_action(
        &mut self,
        event: ocs_plugin_api::dialog::DialogEvent,
    ) -> ocs_plugin_api::dialog::DialogActionResponse {
        // A runner without `DialogAction` answers with an error; the host
        // then falls back to the panel-action notification.
        self.process
            .dialog_action(self.command_id, event)
            .unwrap_or(ocs_plugin_api::dialog::DialogActionResponse::None)
    }
    fn on_dialog_response(
        &mut self,
        response: ocs_plugin_api::dialog::DialogResponse,
    ) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(self.command_id, InteractiveEvent::DialogResponse(response))
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(
            result,
            crate::command::CmdResult::CommitAndExit(_)
                | crate::command::CmdResult::CommitEntitiesAndExit(_)
                | crate::command::CmdResult::Done
                | crate::command::CmdResult::Script(_)
                | crate::command::CmdResult::Cancel
        ) {
            self.is_done = true;
        } else {
            self.refresh();
        }
        result
    }
    fn input_kind(&self) -> crate::command::InputKind {
        plugin_input_kind_to_host(self.input_kind)
    }
    fn on_text_input(&mut self, text: &str) -> Option<crate::command::CmdResult> {
        if !self.input_kind().wants_text() {
            return None;
        }
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(self.command_id, InteractiveEvent::TextInput(text.to_string()))
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(
            result,
            crate::command::CmdResult::CommitAndExit(_)
                | crate::command::CmdResult::CommitEntitiesAndExit(_)
                | crate::command::CmdResult::Done
                | crate::command::CmdResult::Script(_)
                | crate::command::CmdResult::Cancel
        ) {
            self.is_done = true;
        } else {
            self.refresh();
        }
        Some(result)
    }
    fn owner_plugin_id(&self) -> Option<&str> {
        Some(self.process.id())
    }
    fn plugin_command_id(&self) -> Option<u64> {
        Some(self.command_id)
    }
}

/// Convert plugin-declared preview wires to scene wire models. Shared by the
/// interactive-command previews and the custom-grip drag previews (API v11).
pub(in crate::app) fn convert_preview_wires(
    raw_wires: Vec<ocs_plugin_api::host::PreviewWire>,
) -> Vec<crate::scene::model::wire_model::WireModel> {
    use crate::scene::model::wire_model::{TangentGeom, WireModel};
    use ocs_plugin_api::host::PreviewWire;

    raw_wires
        .into_iter()
        .filter_map(|w| match w {
            PreviewWire::Polyline { points, color } => {
                if points.len() < 2 {
                    return None;
                }
                let c = color.unwrap_or(WireModel::CYAN);
                Some(WireModel::solid_f64("rubber_band".into(), points, c, false))
            }
            PreviewWire::Circle {
                center,
                radius,
                color,
            } => {
                if radius <= 0.0 || !radius.is_finite() {
                    return None;
                }
                let c = color.unwrap_or(WireModel::CYAN);
                // Two opposite diameter points ensure OCS's viewport frame-cache
                // (render_signature) detects cursor motion and triggers 60-120 FPS
                // redraws without requiring polar snap or artificial guide lines.
                let points = vec![
                    [center[0] - radius, center[1], center[2]],
                    [center[0] + radius, center[1], center[2]],
                ];
                let mut wire = WireModel::solid_f64("rubber_band".into(), points, c, false);
                wire.tangent_geoms.push(TangentGeom::PlanarCircle {
                    center,
                    axis_x: [1.0, 0.0, 0.0],
                    axis_y: [0.0, 1.0, 0.0],
                    radius,
                });
                Some(wire)
            }
            PreviewWire::Arc {
                center,
                radius,
                start_angle_rad,
                end_angle_rad,
                color,
            } => {
                if radius <= 0.0 || !radius.is_finite() {
                    return None;
                }
                let c = color.unwrap_or(WireModel::CYAN);
                // Start and end points along the circumference ensure OCS's viewport
                // frame-cache (render_signature) detects cursor motion and triggers
                // 60-120 FPS redraws without requiring polar snap or artificial guide lines.
                let p_start = [
                    center[0] + radius * start_angle_rad.cos(),
                    center[1] + radius * start_angle_rad.sin(),
                    center[2],
                ];
                let p_end = [
                    center[0] + radius * end_angle_rad.cos(),
                    center[1] + radius * end_angle_rad.sin(),
                    center[2],
                ];
                let points = vec![p_start, p_end];
                let mut wire = WireModel::solid_f64("rubber_band".into(), points, c, false);
                wire.tangent_geoms.push(TangentGeom::Arc {
                    center,
                    axis_x: [1.0, 0.0, 0.0],
                    axis_y: [0.0, 1.0, 0.0],
                    radius,
                    start_angle: start_angle_rad,
                    end_angle: end_angle_rad,
                });
                Some(wire)
            }
        })
        .collect()
}

fn plugin_step_to_result(step: ocs_plugin_api::host::CommandStep) -> crate::command::CmdResult {
    use crate::command::CmdResult;
    use ocs_plugin_api::host::CommandStep;
    match step {
        CommandStep::NeedPoint => CmdResult::NeedPoint,
        CommandStep::Commit(e) => CmdResult::CommitEntity(e),
        CommandStep::CommitMany(es) => CmdResult::CommitEntities(es),
        CommandStep::CommitAndEnd(e) => CmdResult::CommitAndExit(e),
        CommandStep::CommitManyAndEnd(es) => CmdResult::CommitEntitiesAndExit(es),
        CommandStep::Done => CmdResult::Done,
        CommandStep::Dispatch(script) => CmdResult::Script(script),
        CommandStep::Cancel => CmdResult::Cancel,
    }
}
