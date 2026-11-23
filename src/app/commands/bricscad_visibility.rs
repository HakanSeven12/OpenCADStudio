//! VISIBILITYSTATES and its palette (VISIBILITYSTATESPANELOPEN): the
//! visibility states of a BricsCAD parametric block, in model space of the
//! block's own drawing (see `scene::bricscad_parameters::author`).

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::draw::modify::visibilitystates::{VisibilityStatesCommand, SEP};
use crate::scene::bricscad_parameters::{self as bricscad, Action};
use crate::ui::window::bricscad_visibility::{Model, VisMsg};
use document::Handle;
use iced::Task;
use std::rc::Rc;

/// Marks the step that runs after the objects are picked.
const PICKED: &str = "PICKED";

impl OpenCADStudio {
    /// Model space, the block of a BricsCAD block drawing. A block definition
    /// is left alone: the anonymous copies its references show would keep the
    /// states it had.
    fn visibility_states_owner(&mut self, i: usize) -> Option<Handle> {
        if let Some(owner) = Self::model_space_owner(&self.tabs[i]) {
            return Some(owner);
        }
        self.command_line.push_error(
            crate::t!(
                "VISIBILITYSTATES works in model space: open the drawing of the BricsCAD block."
            )
            .as_ref(),
        );
        None
    }

    fn model_space_owner(tab: &crate::app::DocumentTab) -> Option<Handle> {
        (tab.active_block_edit_session().is_none() && tab.scene.current_layout == "Model")
            .then_some(tab.scene.document.header.model_space_block_handle)
    }

    pub(super) fn dispatch_bricscad_visibility(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        match cmd {
            "VISIBILITYSTATESPANELOPEN" => {
                self.set_bricscad_visibility_panel(true);
                return Some(Task::none());
            }
            "VISIBILITYSTATESPANELCLOSE" => {
                self.set_bricscad_visibility_panel(false);
                return Some(Task::none());
            }
            _ => {}
        }
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
            self.pick_for_bricscad_visibility(i, cmd);
            return Some(Task::none());
        }
        self.tabs[i].active_cmd = None;
        let objects = if action == Action::Select {
            Vec::new()
        } else {
            self.tabs[i].scene.selected_handles_in_order()
        };
        self.apply_bricscad_visibility(i, parameter, state, action, &objects);
        Some(Task::none())
    }

    /// Ask for the objects of `cmd` (`_VISIBILITYSTATES` with its fields).
    fn pick_for_bricscad_visibility(&mut self, i: usize, cmd: &str) {
        use crate::modules::draw::select::SelectObjectsCommand;
        self.tabs[i].scene.deselect_all();
        let pending = format!("{cmd}{SEP}{PICKED}");
        let command =
            SelectObjectsCommand::with_prompt("VISIBILITYSTATES", &pending, "Select objects:");
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
    }

    /// One VISIBILITYSTATES step on model space of tab `i`, one undo step;
    /// whether it was done.
    fn apply_bricscad_visibility(
        &mut self,
        i: usize,
        parameter: &str,
        state: &str,
        action: Action,
        objects: &[Handle],
    ) -> bool {
        let Some(owner) = self.visibility_states_owner(i) else {
            return false;
        };
        // The edit runs on a copy, so a refusal leaves the drawing as it was;
        // what the editor holds goes into the drawing first.
        self.tabs[i].scene.sync_native_parametric_graph();
        let mut scene = crate::scene::Scene::new();
        scene.document = self.tabs[i].scene.document.clone();
        scene.load_named_parameters_from_document();
        scene.load_parametric_constraints_from_document();
        match scene.edit_bricscad_visibility(owner, parameter, state, action, objects) {
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
                true
            }
            Err(error) => {
                self.command_line.push_error(crate::t!(error).as_ref());
                false
            }
        }
    }

    pub(in crate::app) fn set_bricscad_visibility_panel(&mut self, open: bool) {
        let id = crate::ui::dock::PanelId::BricscadVisibility;
        self.bricscad_visibility.show = open;
        if open {
            self.dock_open_at_default(id);
        } else {
            self.dock_unpeek(id);
        }
    }

    /// What the palette shows of `tab`: model space's parameters, read once
    /// per change of the drawing; `None` outside model space.
    pub(in crate::app) fn bricscad_visibility_model(
        &self,
        tab: &crate::app::DocumentTab,
    ) -> Option<Rc<Model>> {
        Self::model_space_owner(tab)?;
        let key = (
            tab.id,
            tab.edit_revision,
            crate::i18n::language_generation(),
        );
        let mut cache = self.bricscad_visibility.cache.borrow_mut();
        match cache.as_ref() {
            Some((at, model)) if *at == key => Some(model.clone()),
            _ => {
                let model = Rc::new(crate::ui::window::bricscad_visibility::read(
                    &tab.scene.document,
                ));
                *cache = Some((key, model.clone()));
                Some(model)
            }
        }
    }

    pub(in crate::app) fn on_bricscad_visibility(&mut self, message: VisMsg) -> Task<Message> {
        let i = self.active_tab;
        let panel = &mut self.bricscad_visibility;
        match message {
            VisMsg::Select(parameter, state) => {
                self.apply_bricscad_visibility(i, &parameter, &state, Action::Select, &[]);
            }
            VisMsg::Open(parameter, state) => {
                panel.page = Some((parameter, state));
                panel.new_state = None;
                panel.new_parameter = None;
            }
            VisMsg::Move(parameter, state, up) => {
                let action = if up { Action::MoveUp } else { Action::MoveDown };
                self.apply_bricscad_visibility(i, &parameter, &state, action, &[]);
            }
            VisMsg::Back => panel.page = None,
            VisMsg::Fold(parameter) => {
                let key = parameter.to_uppercase();
                if let Some(k) = panel.folded.iter().position(|p| *p == key) {
                    panel.folded.remove(k);
                } else {
                    panel.folded.push(key);
                }
            }
            VisMsg::NewParameter => {
                panel.new_parameter = Some(Default::default());
                panel.new_state = None;
            }
            VisMsg::NewParameterName(name) => {
                if let Some((n, _)) = panel.new_parameter.as_mut() {
                    *n = name;
                }
            }
            VisMsg::NewParameterState(state) => {
                if let Some((_, s)) = panel.new_parameter.as_mut() {
                    *s = state;
                }
            }
            VisMsg::NewParameterPick => {
                let Some((name, state)) = panel.new_parameter.take() else {
                    return Task::none();
                };
                let (name, state) = (name.trim().to_string(), state.trim().to_string());
                if name.is_empty() || state.is_empty() {
                    panel.new_parameter = Some((name, state));
                    return Task::none();
                }
                panel.page = Some((name.clone(), state.clone()));
                if self.visibility_states_owner(i).is_some() {
                    self.pick_for_bricscad_visibility(
                        i,
                        &format!("_VISIBILITYSTATES{SEP}{name}{SEP}{state}{SEP}ADD"),
                    );
                }
            }
            VisMsg::NewState(parameter) => {
                panel.new_state = Some((parameter, String::new()));
                panel.new_parameter = None;
            }
            VisMsg::NewStateName(name) => {
                if let Some((_, n)) = panel.new_state.as_mut() {
                    *n = name;
                }
            }
            VisMsg::NewStateCommit => {
                let Some((parameter, name)) = panel.new_state.take() else {
                    return Task::none();
                };
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                if self.apply_bricscad_visibility(i, &parameter, &name, Action::NewState, &[]) {
                    self.bricscad_visibility.page = Some((parameter, name));
                } else {
                    self.bricscad_visibility.new_state = Some((parameter, name));
                }
            }
            VisMsg::Cancel => {
                panel.new_parameter = None;
                panel.new_state = None;
            }
            VisMsg::Show(entity, shown) => {
                if let Some((parameter, state)) = panel.page.clone() {
                    let action = if shown {
                        Action::Visible
                    } else {
                        Action::Invisible
                    };
                    self.apply_bricscad_visibility(i, &parameter, &state, action, &[entity]);
                }
            }
            VisMsg::Remove(entity) => {
                if let Some((parameter, state)) = panel.page.clone() {
                    self.apply_bricscad_visibility(
                        i,
                        &parameter,
                        &state,
                        Action::Remove,
                        &[entity],
                    );
                }
            }
            VisMsg::AddObjects => {
                if let Some((parameter, state)) = panel.page.clone() {
                    if self.visibility_states_owner(i).is_some() {
                        self.pick_for_bricscad_visibility(
                            i,
                            &format!("_VISIBILITYSTATES{SEP}{parameter}{SEP}{state}{SEP}ADD"),
                        );
                    }
                }
            }
            VisMsg::Highlight(entity) => {
                if self.tabs[i].scene.document.get_entity(entity).is_some() {
                    self.tabs[i].scene.deselect_all();
                    self.tabs[i].scene.select_entity(entity, true);
                    self.refresh_properties();
                }
            }
        }
        Task::none()
    }
}
