use super::{Message, OpenCADStudio};
use crate::command::{CadCommand, CmdResult};
use crate::modules::block_editor::construction::{ConstructionCommand, Operation};
use iced::Task;

impl OpenCADStudio {
    pub(in crate::app) fn start_block_construction(&mut self, args: &str) -> Task<Message> {
        let i = self.active_tab;
        let Some(definition) = self.tabs[i]
            .active_block_edit_session()
            .map(|s| s.br_handle)
        else {
            self.command_line.push_error(
                crate::t!("BCONSTRUCTION is only available in the Block Editor.").as_ref(),
            );
            return Task::none();
        };
        let operation = if args.is_empty() {
            None
        } else {
            let Some(operation) = Operation::parse(args) else {
                self.command_line
                    .push_error(crate::t!("Choose Convert, Revert, Show or Hide.").as_ref());
                return Task::none();
            };
            Some(operation)
        };
        let handles = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .map(|(h, _)| *h)
            .collect::<Vec<_>>();
        let command = ConstructionCommand {
            document: self.tabs[i].id,
            definition,
            handles,
            operation,
        };
        if let Some(operation) = operation {
            if matches!(operation, Operation::Show | Operation::Hide) || !command.handles.is_empty()
            {
                return self.set_block_construction(CmdResult::SetBlockConstruction {
                    document: command.document,
                    definition,
                    handles: command.handles,
                    operation,
                });
            }
        }
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    pub(in crate::app) fn set_block_construction(&mut self, result: CmdResult) -> Task<Message> {
        let CmdResult::SetBlockConstruction {
            document,
            definition,
            handles,
            operation,
        } = result
        else {
            return Task::none();
        };
        let i = self.active_tab;
        if self.tabs[i].id != document
            || self.tabs[i]
                .active_block_edit_session()
                .map(|s| s.br_handle)
                != Some(definition)
        {
            self.command_line.push_error(
                crate::t!("The construction geometry's Block Editor session is no longer active.")
                    .as_ref(),
            );
            return Task::none();
        }
        if matches!(operation, Operation::Show | Operation::Hide) {
            self.tabs[i]
                .scene
                .set_construction_shown(definition, operation == Operation::Show);
            self.tabs[i].selected_handle = None;
            self.tabs[i].selected_grips.clear();
            self.tabs[i].selected_grip_handles.clear();
            self.tabs[i].active_cmd = None;
            self.refresh_properties();
            return Task::none();
        }
        let mut staged = self.tabs[i].scene.document.clone();
        match crate::scene::block_construction::set(
            &mut staged,
            definition,
            &handles,
            operation == Operation::Convert,
            &self.tabs[i].active_layer,
        ) {
            Ok(0) => {
                self.tabs[i].active_cmd = None;
            }
            Ok(count) => {
                if let Some(parameter) =
                    crate::scene::block_visibility::parameter(&staged, definition)
                {
                    let parameter = parameter.handle;
                    let state = self.tabs[i]
                        .scene
                        .visibility_states
                        .get(&definition)
                        .copied()
                        .unwrap_or(0);
                    crate::scene::block_visibility::apply_state(&mut staged, parameter, state);
                }
                let staged =
                    match crate::scene::block_actions::instance::refresh(&staged, definition) {
                        Ok(staged) => staged,
                        Err(error) => {
                            self.command_line.push_error(crate::t!(error).as_ref());
                            return Task::none();
                        }
                    };
                self.push_undo_snapshot(
                    i,
                    super::document::HistoryLabel::Message("Edit construction geometry"),
                );
                self.apply_action_document(i, staged);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].active_cmd = None;
                self.finish_pending_history(i);
                self.refresh_properties();
                self.command_line
                    .push_output(crate::tf!("{} geometry objects changed.", count).as_ref());
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }
}
