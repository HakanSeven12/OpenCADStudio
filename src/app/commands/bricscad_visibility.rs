//! VISIBILITYSTATES: the visibility states of a BricsCAD parametric block, in
//! model space of the block's own drawing (see
//! `scene::bricscad_parameters::author`).

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::draw::modify::visibilitystates::{VisibilityStatesCommand, SEP};
use crate::scene::bricscad_parameters::{self as bricscad, Action};
use document::Handle;
use iced::Task;

/// Marks the step that runs after the objects are picked.
const PICKED: &str = "PICKED";

impl OpenCADStudio {
    /// Model space, the block of a BricsCAD block drawing. A block definition
    /// is left alone: the anonymous copies its references show would keep the
    /// states it had.
    fn visibility_states_owner(&mut self, i: usize) -> Option<Handle> {
        if self.tabs[i].active_block_edit_session().is_none()
            && self.tabs[i].scene.current_layout == "Model"
        {
            return Some(self.tabs[i].scene.document.header.model_space_block_handle);
        }
        self.command_line.push_error(
            crate::t!(
                "VISIBILITYSTATES works in model space: open the drawing of the BricsCAD block."
            )
            .as_ref(),
        );
        None
    }

    pub(super) fn dispatch_bricscad_visibility(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        if cmd == "VISIBILITYSTATES" {
            let Some(owner) = self.visibility_states_owner(i) else {
                return Some(Task::none());
            };
            let parameters = bricscad::parameter_states(&self.tabs[i].scene.document, owner);
            let command = VisibilityStatesCommand::new(parameters);
            self.command_line.push_info(&command.prompt());
            self.command_line.set_step_options(command.options());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Some(Task::none());
        }
        let fields = cmd.strip_prefix("_VISIBILITYSTATES")?;
        let fields: Vec<&str> = fields.strip_prefix(SEP)?.split(SEP).collect();
        let (parameter, state, action, picked) = match fields.as_slice() {
            [parameter, state, action] => (*parameter, *state, *action, false),
            [parameter, state, action, PICKED] => (*parameter, *state, *action, true),
            _ => return Some(Task::none()),
        };
        let action = match action {
            "ADD" => Action::Add,
            "REMOVE" => Action::Remove,
            "VISIBLE" => Action::Visible,
            "INVISIBLE" => Action::Invisible,
            "SELECT" => Action::Select,
            _ => return Some(Task::none()),
        };
        if action != Action::Select && !picked && self.tabs[i].scene.selected.is_empty() {
            use crate::modules::draw::select::SelectObjectsCommand;
            let pending = format!("{cmd}{SEP}{PICKED}");
            let command =
                SelectObjectsCommand::with_prompt("VISIBILITYSTATES", &pending, "Select objects:");
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Some(Task::none());
        }
        self.tabs[i].active_cmd = None;
        let Some(owner) = self.visibility_states_owner(i) else {
            return Some(Task::none());
        };
        let objects = if action == Action::Select {
            Vec::new()
        } else {
            self.tabs[i].scene.selected_handles_in_order()
        };
        // The edit runs on a copy, so a refusal leaves the drawing as it was;
        // what the editor holds goes into the drawing first.
        self.tabs[i].scene.sync_native_parametric_graph();
        let mut scene = crate::scene::Scene::new();
        scene.document = self.tabs[i].scene.document.clone();
        scene.load_named_parameters_from_document();
        scene.load_parametric_constraints_from_document();
        match scene.edit_bricscad_visibility(owner, parameter, state, action, &objects) {
            Ok(()) => {
                self.push_undo_snapshot(i, "VISIBILITYSTATES");
                self.apply_action_document_and_bump(i, scene.document);
                self.tabs[i].scene.load_named_parameters_from_document();
                self.tabs[i]
                    .scene
                    .load_parametric_constraints_from_document();
                self.tabs[i].dirty = true;
                self.finish_pending_history(i);
                if action != Action::Select {
                    self.tabs[i].scene.deselect_all();
                }
                let shown = bricscad::parameter_states(&self.tabs[i].scene.document, owner)
                    .into_iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(parameter))
                    .and_then(|_| {
                        bricscad::current_state(&self.tabs[i].scene.document, owner, parameter)
                    });
                if let Some(shown) = shown {
                    self.command_line
                        .push_output(crate::tf!("Current visibility state: {}", shown).as_ref());
                }
                self.refresh_properties();
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Some(Task::none())
    }
}
