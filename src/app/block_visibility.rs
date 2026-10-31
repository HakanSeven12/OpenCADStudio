use super::{Message, ModalKind, OpenCADStudio};
use crate::scene::block_visibility as visibility;
use crate::ui::window::block_visibility::{Action, NewVisibility, RibbonContext, State, Target};
use iced::Task;

impl OpenCADStudio {
    pub(super) fn block_visibility_context(&self) -> Option<RibbonContext> {
        let tab = self.tabs.get(self.active_tab)?;
        let definition = tab.active_block_edit_session()?.br_handle;
        let parameter = visibility::parameter(&tab.scene.document, definition)?;
        if parameter.states.is_empty() {
            return None;
        }
        let current = self.visibility_current(self.active_tab, definition);
        Some(RibbonContext {
            target: Target {
                document: tab.id,
                definition,
                parameter: parameter.handle,
            },
            names: parameter
                .states
                .iter()
                .map(|state| state.name.clone())
                .collect(),
            current: parameter
                .states
                .get(current)
                .map(|state| state.name.clone()),
            mode: self.block_visibility_mode,
        })
    }

    fn visibility_target_valid(&self, target: Target) -> bool {
        self.block_visibility_context()
            .is_some_and(|context| context.target == target)
    }

    pub(super) fn sync_block_visibility_display(&mut self) {
        for tab in &mut self.tabs {
            let definition = tab
                .active_block_edit_session()
                .map(|session| session.br_handle);
            // Resolve imported state flags once when an editor opens, not on
            // every view refresh. Subsequent state identity follows history.
            if let Some(definition) = definition {
                if !tab.scene.visibility_states.contains_key(&definition) {
                    if let Some(parameter) = visibility::parameter(&tab.scene.document, definition)
                    {
                        let current =
                            visibility::shown_state(&tab.scene.document, parameter.handle);
                        tab.scene.visibility_states.insert(definition, current);
                    }
                }
            }
            let editing = definition.is_some();
            let picking = tab
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.reveals_hidden_block_objects());
            tab.scene
                .set_block_visibility_reveal(editing && (self.block_visibility_mode || picking));
        }
    }

    pub(super) fn handle_block_visibility(&mut self, action: Action) -> Task<Message> {
        let i = self.active_tab;
        match action {
            Action::Open(target) => {
                if !self.visibility_target_valid(target) {
                    return Task::none();
                }
                // A ribbon click can interrupt a running authoring command.
                let cancellation =
                    if self.tabs[i].active_cmd.is_some() || self.pending_setvar.is_some() {
                        self.on_command_escape()
                    } else {
                        Task::none()
                    };
                self.sync_block_visibility_display();
                let original =
                    visibility::parameter(&self.tabs[i].scene.document, target.definition)
                        .unwrap()
                        .clone();
                let current = self
                    .visibility_current(i, target.definition)
                    .min(original.states.len().saturating_sub(1));
                let name = original
                    .states
                    .get(current)
                    .map(|state| state.name.clone())
                    .unwrap_or_default();
                self.block_visibility = Some(State {
                    target,
                    edited: original.clone(),
                    original,
                    current,
                    selected: current,
                    name,
                    new_visibility: NewVisibility::Keep,
                    error: None,
                    epoch: self.tabs[i].scene.geometry_epoch,
                });
                self.active_modal = Some(ModalKind::BlockVisibility);
                return cancellation;
            }
            Action::Current(target, name) => {
                if !self.visibility_target_valid(target) {
                    return Task::none();
                }
                let parameter =
                    visibility::parameter(&self.tabs[i].scene.document, target.definition).unwrap();
                let Some(index) = parameter.states.iter().position(|state| state.name == name)
                else {
                    return Task::none();
                };
                if self.visibility_current(i, target.definition) != index {
                    let cancellation =
                        if self.tabs[i].active_cmd.is_some() || self.pending_setvar.is_some() {
                            self.on_command_escape()
                        } else {
                            Task::none()
                        };
                    self.push_undo_snapshot(i, "Visibility state");
                    self.show_visibility_state(i, target.definition, target.parameter, index);
                    self.finish_pending_history(i);
                    return cancellation;
                }
            }
            Action::Mode(target) => {
                if self.visibility_target_valid(target) {
                    self.block_visibility_mode = !self.block_visibility_mode;
                    self.sync_block_visibility_display();
                }
            }
            Action::Apply => {
                let Some(state) = self.block_visibility.as_ref() else {
                    return Task::none();
                };
                if !self.visibility_target_valid(state.target)
                    || self.tabs[i].scene.geometry_epoch != state.epoch
                    || visibility::parameter(&self.tabs[i].scene.document, state.target.definition)
                        != Some(&state.original)
                {
                    self.block_visibility.as_mut().unwrap().error = Some(
                        crate::t!("The block changed. Cancel and reopen Visibility States.")
                            .into_owned(),
                    );
                    return Task::none();
                }
                let state = self.block_visibility.take().unwrap();
                if state.edited != state.original
                    || state.current != self.visibility_current(i, state.target.definition)
                {
                    self.push_undo_snapshot(i, "Visibility states");
                    visibility::replace_states(
                        &mut self.tabs[i].scene.document,
                        state.target.definition,
                        state.edited,
                    );
                    self.show_visibility_state(
                        i,
                        state.target.definition,
                        state.target.parameter,
                        state.current,
                    );
                    self.finish_pending_history(i);
                    self.refresh_properties();
                }
                self.active_modal = None;
            }
            Action::Help => {
                self.command_line.push_info(
                    crate::t!(
                        "A visibility state controls which objects a block shows. This dialog lists the states, marks the current one, and creates, renames, reorders or deletes them. OK applies the changes; Cancel discards them."
                    )
                    .as_ref(),
                );
                return Task::none();
            }
            action => {
                let Some(state) = self.block_visibility.as_mut() else {
                    return Task::none();
                };
                state.error = None;
                match action {
                    Action::Select(index) => state.select(index),
                    Action::Name(name) => state.name = name,
                    Action::NewVisibility(mode) => state.new_visibility = mode,
                    Action::New | Action::Rename => {
                        let rename = matches!(action, Action::Rename);
                        if let Some(error) = state.name_error(rename) {
                            state.error = Some(error);
                            return Task::none();
                        }
                        let name = state.name.trim().to_string();
                        if rename {
                            state.edited.states[state.selected].name = name;
                        } else {
                            let mut new = state.edited.states[state.current].clone();
                            new.name = name;
                            if state.new_visibility != NewVisibility::Keep {
                                // Newly controlled geometry was visible in all existing states.
                                let document = &self.tabs[i].scene.document;
                                let members = document
                                    .block_records
                                    .iter()
                                    .find(|record| record.handle == state.target.definition)
                                    .map(|record| record.entity_handles.clone())
                                    .unwrap_or_default();
                                for handle in members {
                                    if !state.edited.all_blocks.contains(&handle) {
                                        state.edited.all_blocks.push(handle);
                                        if document
                                            .get_entity(handle)
                                            .is_some_and(|entity| !entity.common().invisible)
                                        {
                                            for old in &mut state.edited.states {
                                                old.visible_blocks.push(handle);
                                            }
                                        }
                                    }
                                }
                                new.visible_blocks = if state.new_visibility == NewVisibility::Show
                                {
                                    state.edited.all_blocks.clone()
                                } else {
                                    Vec::new()
                                };
                            }
                            state.edited.states.push(new);
                            state.select(state.edited.states.len() - 1);
                        }
                    }
                    Action::Delete if state.edited.states.len() > 1 => {
                        let removed = state.selected;
                        state.edited.states.remove(removed);
                        if state.current == removed {
                            state.current = 0;
                        } else if state.current > removed {
                            state.current -= 1;
                        }
                        state.select(removed.min(state.edited.states.len() - 1));
                    }
                    Action::SetCurrent => state.current = state.selected,
                    Action::Move(up) => {
                        let other = if up {
                            state.selected.checked_sub(1)
                        } else {
                            Some(state.selected + 1)
                        };
                        if let Some(other) =
                            other.filter(|other| *other < state.edited.states.len())
                        {
                            state.edited.states.swap(state.selected, other);
                            if state.current == state.selected {
                                state.current = other;
                            } else if state.current == other {
                                state.current = state.selected;
                            }
                            state.select(other);
                        }
                    }
                    _ => {}
                }
            }
        }
        Task::none()
    }
}
