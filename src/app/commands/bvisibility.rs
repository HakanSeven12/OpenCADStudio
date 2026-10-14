//! BVSTATE, BVHIDE and BVSHOW: the visibility states of the block open in
//! the Block Editor (see `scene::block_visibility`).

use crate::app::{Message, OpenCADStudio};
use crate::scene::block_visibility as visibility;
use codec::{CadDocument, EntityType, Handle};
use iced::Task;

impl OpenCADStudio {
    /// The block open in the Block Editor, or an error naming `command`.
    fn visibility_block(&mut self, i: usize, command: &str) -> Option<Handle> {
        let block = self.tabs[i]
            .active_block_edit_session()
            .map(|session| session.br_handle);
        if block.is_none() {
            self.command_line.push_error(
                crate::tf!("{}: open the block in the Block Editor first (BEDIT).", command)
                    .as_ref(),
            );
        }
        block
    }

    /// The block's visibility parameter, made when it has none: its grip
    /// sits on the block's base point.
    fn visibility_parameter(&mut self, i: usize, block: Handle) -> Option<Handle> {
        let document = &mut self.tabs[i].scene.document;
        let base = document
            .block_records
            .iter()
            .find(|record| record.handle == block)
            .and_then(|record| match document.get_entity(record.block_entity_handle) {
                Some(EntityType::Block(begin)) => Some(begin.base_point),
                _ => None,
            })
            .unwrap_or(codec::types::Vector3::ZERO);
        visibility::create(document, block, base)
    }

    /// The state the block shows in the editor.
    fn visibility_current(&self, i: usize, block: Handle) -> usize {
        let scene = &self.tabs[i].scene;
        scene.visibility_states.get(&block).copied().unwrap_or_else(|| {
            visibility::parameter(&scene.document, block)
                .map_or(0, |parameter| visibility::shown_state(&scene.document, parameter.handle))
        })
    }

    fn show_visibility_state(&mut self, i: usize, block: Handle, parameter: Handle, state: usize) {
        visibility::apply_state(&mut self.tabs[i].scene.document, parameter, state);
        self.tabs[i].scene.visibility_states.insert(block, state);
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        if let Some(name) = visibility::state_names(&self.tabs[i].scene.document, parameter)
            .get(state)
        {
            self.command_line
                .push_output(crate::tf!("Current visibility state: {}", name).as_ref());
        }
    }

    pub(super) fn dispatch_bvisibility(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        use crate::command::CadCommand;
        use crate::modules::draw::modify::bvstate::{BvStateCommand, SEP};
        match cmd {
            "BVSTATE" => {
                let block = self.visibility_block(i, "BVSTATE")?;
                let document = &self.tabs[i].scene.document;
                let names = visibility::parameter(document, block)
                    .map(|parameter| visibility::state_names(document, parameter.handle))
                    .unwrap_or_else(|| vec!["VisibilityState0".to_string()]);
                let current = self.visibility_current(i, block);
                if let Some(name) = names.get(current) {
                    self.command_line
                        .push_output(crate::tf!("Current visibility state: {}", name).as_ref());
                }
                let command = BvStateCommand::new(names, current);
                self.command_line.push_info(&command.prompt());
                self.command_line.set_step_options(command.options());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            cmd if cmd.starts_with("_BVSTATE") => {
                let block = self.visibility_block(i, "BVSTATE")?;
                let fields: Vec<&str> = cmd.split(SEP).skip(1).collect();
                self.push_undo_snapshot(i, "BVSTATE");
                let parameter = self.visibility_parameter(i, block)?;
                let current = self.visibility_current(i, block);
                let document = &mut self.tabs[i].scene.document;
                let index = |document: &CadDocument, name: &str| {
                    visibility::state_names(document, parameter)
                        .iter()
                        .position(|candidate| candidate.eq_ignore_ascii_case(name))
                };
                let shown = match fields.as_slice() {
                    ["NEW", name] => visibility::add_state(document, parameter, name, current),
                    ["SET", name] => index(document, name),
                    ["RENAME", from, to] => {
                        if let Some(state) = index(document, from) {
                            visibility::rename_state(document, parameter, state, to);
                        }
                        Some(current)
                    }
                    ["DELETE", name] => index(document, name).map(|state| {
                        visibility::delete_state(document, parameter, state);
                        let count = visibility::state_names(document, parameter).len();
                        if current == state || current >= count {
                            0
                        } else if current > state {
                            current - 1
                        } else {
                            current
                        }
                    }),
                    _ => None,
                };
                self.show_visibility_state(i, block, parameter, shown.unwrap_or(current));
                self.tabs[i].active_cmd = None;
                Some(Task::none())
            }
            "BVHIDE" | "BVSHOW" => {
                let block = self.visibility_block(i, cmd)?;
                let hide = cmd == "BVHIDE";
                if !hide {
                    // Every object of the block shows while they are picked;
                    // the state is put back once the pick ends.
                    let document = &mut self.tabs[i].scene.document;
                    let members: Vec<Handle> = document
                        .block_records
                        .iter()
                        .find(|record| record.handle == block)
                        .map(|record| record.entity_handles.clone())
                        .unwrap_or_default();
                    for handle in members {
                        if let Some(entity) = document.get_entity_mut(handle) {
                            entity.common_mut().invisible = false;
                        }
                    }
                    self.tabs[i].scene.bump_geometry();
                }
                let pending = if hide { "_BVHIDE" } else { "_BVSHOW" };
                if !self.tabs[i].scene.selected.is_empty() {
                    return Some(self.dispatch_command(pending));
                }
                let prompt = if hide {
                    "Select objects to hide:"
                } else {
                    "Select objects to make visible:"
                };
                use crate::modules::draw::select::SelectObjectsCommand;
                let command = SelectObjectsCommand::with_prompt(cmd, pending, prompt);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            "_BVHIDE" | "_BVSHOW" => {
                let block = self.visibility_block(i, &cmd[1..])?;
                let handles: Vec<Handle> = self.tabs[i]
                    .scene
                    .selected_handles_in_order()
                    .into_iter()
                    .filter(|handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(*handle)
                            .is_some_and(|entity| entity.common().owner_handle == block)
                    })
                    .collect();
                self.push_undo_snapshot(i, &cmd[1..]);
                let parameter = self.visibility_parameter(i, block)?;
                let current = self.visibility_current(i, block);
                visibility::set_visible(
                    &mut self.tabs[i].scene.document,
                    parameter,
                    current,
                    &handles,
                    cmd == "_BVSHOW",
                );
                self.tabs[i].scene.deselect_all();
                self.show_visibility_state(i, block, parameter, current);
                self.tabs[i].active_cmd = None;
                Some(Task::none())
            }
            _ => None,
        }
    }
}
