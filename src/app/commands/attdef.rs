//! ATTDEF (dialog), -ATTDEF (command line), AFLAGS, and the Edit Attribute
//! Definition dialog opened by double-click / TEXTEDIT / DDEDIT.

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::draw::draw::attdef::{
    self as attdef, AttdefCommand, AttdefPickCommand, AttdefPlaceCommand, AttdefSpec, Justify,
};
use crate::ui::window::attdef_dialog::{
    tag_error, AttdefDialogMsg, AttdefDialogState, AttdefEditState,
};
use iced::Task;

fn short(v: f64) -> String {
    let t = format!("{v:.4}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl OpenCADStudio {
    pub(super) fn dispatch_attdef(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        if matches!(cmd, "ATTDEF" | "-ATTDEF") {
            // Placement measures the tag with its style's font.
            crate::entities::common::set_fixed_text_heights(&self.tabs[i].scene.document);
        }
        match cmd {
            "ATTDEF" => {
                self.open_attdef_dialog(i);
                Some(Task::none())
            }
            "-ATTDEF" => {
                let defaults =
                    crate::scene::creation_style::current_text_defaults(&self.tabs[i].scene.document);
                let styles: Vec<_> = self.tabs[i].scene.document.text_styles.iter().cloned().collect();
                let command = AttdefCommand::new(
                    "-ATTDEF",
                    defaults.style_name,
                    defaults.height,
                    defaults.width_factor,
                    defaults.oblique_angle,
                    styles,
                );
                self.reset_command_start_state(i);
                self.command_line.push_output(&command.intro());
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                self.push_ucs_to_cmd(i);
                Some(Task::none())
            }
            "ATTMODE0" => self.dispatch_draw("ATTDISP OFF", i),
            "ATTMODE1" => self.dispatch_draw("ATTDISP NORMAL", i),
            "ATTMODE2" => self.dispatch_draw("ATTDISP ON", i),
            "_ATTDEF_VALUE" => {
                let value = attdef::session().editor_value.take();
                if let Some(state) = self.attdef_dialog.as_mut() {
                    if let Some(value) = value {
                        state.default = value;
                        state.field = None;
                    }
                    self.active_modal = Some(crate::app::ModalKind::AttDef);
                }
                Some(Task::none())
            }
            cmd if cmd == "_ATTDEF_PICKED" || cmd.starts_with("_ATTDEF_PICKED ") => {
                let mut parts = cmd.split_whitespace().skip(1);
                if let (Some(kind), Some(value), Some(state)) =
                    (parts.next(), parts.next(), self.attdef_dialog.as_mut())
                {
                    match kind {
                        "H" => state.height = value.to_string(),
                        "R" => state.rotation = value.to_string(),
                        _ => state.width = value.to_string(),
                    }
                }
                if self.attdef_dialog.is_some() {
                    self.active_modal = Some(crate::app::ModalKind::AttDef);
                }
                Some(Task::none())
            }
            _ => self.dispatch_aflags(cmd),
        }
    }

    /// AFLAGS: the session's attribute modes (0–63, default 16).
    fn dispatch_aflags(&mut self, cmd: &str) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let mut parts = rest.splitn(2, char::is_whitespace);
        if !parts.next().unwrap_or("").eq_ignore_ascii_case("AFLAGS") {
            return None;
        }
        let current = attdef::session().aflags;
        match parts.next().map(str::trim).filter(|v| !v.is_empty()) {
            None => {
                self.command_line
                    .push_output(&format!("Enter new value for AFLAGS <{current}>:"));
                self.pending_setvar = Some("AFLAGS".to_string());
            }
            Some(v) => match v.parse::<u8>().ok().filter(|v| *v <= 63) {
                Some(v) => attdef::session().aflags = v,
                None => {
                    self.command_line.push_error("Requires an integer between 0 and 63.");
                    self.command_line
                        .push_output(&format!("Enter new value for AFLAGS <{current}>:"));
                    self.pending_setvar = Some("AFLAGS".to_string());
                }
            },
        }
        Some(Task::none())
    }

    fn open_attdef_dialog(&mut self, i: usize) {
        let document = &self.tabs[i].scene.document;
        let defaults = crate::scene::creation_style::current_text_defaults(document);
        let mut styles: Vec<String> = document
            .text_styles
            .iter()
            .filter(|s| !s.name.is_empty())
            .map(|s| s.name.clone())
            .collect();
        styles.sort_by_key(|s| s.to_lowercase());
        let session = attdef::session();
        self.attdef_dialog = Some(AttdefDialogState {
            aflags: session.aflags,
            annotative: session.annotative,
            tag: String::new(),
            prompt: String::new(),
            default: String::new(),
            justify: Justify::Left,
            style: defaults.style_name,
            styles,
            height: short(defaults.height),
            rotation: "0".into(),
            width: "0".into(),
            on_screen: session.on_screen,
            coords: ["0".into(), "0".into(), "0".into()],
            align_below: false,
            can_align_below: session.last.is_some(),
            error: None,
            field: None,
        });
        drop(session);
        self.tabs[i].active_cmd = None;
        self.active_modal = Some(crate::app::ModalKind::AttDef);
    }

    /// The Edit Attribute Definition dialog for `handle`.
    pub(in crate::app) fn open_attdef_edit(&mut self, handle: codec::Handle) {
        let i = self.active_tab;
        let Some(codec::EntityType::AttributeDefinition(a)) = self.tabs[i].scene.document.get_entity(handle)
        else {
            return;
        };
        self.attdef_edit = Some(AttdefEditState {
            handle,
            tag: a.tag.clone(),
            prompt: a.prompt.clone(),
            default: a.default_value.clone(),
            constant: a.flags.constant,
            error: None,
            field: None,
            field_changed: false,
        });
        self.active_modal = Some(crate::app::ModalKind::AttDefEdit);
    }

    pub(in crate::app) fn on_attdef_dialog(&mut self, m: AttdefDialogMsg) -> Task<Message> {
        let i = self.active_tab;
        if let Some(edit) = self.attdef_edit.as_mut() {
            match m {
                AttdefDialogMsg::EditTag(v) => edit.tag = v,
                AttdefDialogMsg::EditPrompt(v) => edit.prompt = v,
                AttdefDialogMsg::EditDefault(v) => {
                    edit.default = v;
                    edit.field = None;
                    edit.field_changed = true;
                }
                AttdefDialogMsg::EditInsertField => {
                    self.active_modal = None;
                    self.open_field_dialog(crate::ui::window::field_dialog::FieldTarget::AttdefEdit);
                }
                AttdefDialogMsg::Help => self.command_line.push_info(
                    crate::t!("Changes the tag, prompt and default value of an attribute definition.")
                        .as_ref(),
                ),
                AttdefDialogMsg::EditOk => return self.attdef_edit_ok(i),
                AttdefDialogMsg::DismissError => edit.error = None,
                _ => {}
            }
            return Task::none();
        }
        let Some(state) = self.attdef_dialog.as_mut() else {
            return Task::none();
        };
        match m {
            AttdefDialogMsg::Mode(bit, on) => {
                if on {
                    state.aflags |= bit;
                } else {
                    state.aflags &= !bit;
                }
            }
            AttdefDialogMsg::Annotative(v) => state.annotative = v,
            AttdefDialogMsg::Tag(v) => state.tag = v,
            AttdefDialogMsg::Prompt(v) => state.prompt = v,
            AttdefDialogMsg::Default(v) => {
                state.default = v;
                state.field = None;
            }
            AttdefDialogMsg::InsertField => {
                self.active_modal = None;
                self.open_field_dialog(crate::ui::window::field_dialog::FieldTarget::AttdefDefault);
                return Task::none();
            }
            AttdefDialogMsg::Justify(c) => state.justify = c.0,
            AttdefDialogMsg::Style(v) => state.style = v,
            AttdefDialogMsg::Height(v) => state.height = v,
            AttdefDialogMsg::Rotation(v) => state.rotation = v,
            AttdefDialogMsg::Width(v) => state.width = v,
            AttdefDialogMsg::OnScreen(v) => state.on_screen = v,
            AttdefDialogMsg::Coord(k, v) => state.coords[k] = v,
            AttdefDialogMsg::AlignBelow(v) => state.align_below = v,
            AttdefDialogMsg::PickHeight | AttdefDialogMsg::PickRotation | AttdefDialogMsg::PickWidth => {
                let (kind, current) = match m {
                    AttdefDialogMsg::PickHeight => ("H", state.height.clone()),
                    AttdefDialogMsg::PickRotation => ("R", state.rotation.clone()),
                    _ => ("W", state.width.clone()),
                };
                // Keep the dialog's fields; it reopens when the value is in.
                self.active_modal = None;
                let command = AttdefPickCommand::new(kind, current);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                self.push_ucs_to_cmd(i);
                return self.focus_cmd_input();
            }
            AttdefDialogMsg::Help => self.command_line.push_info(
                crate::t!("Defines an attribute: a tag, prompt and default value placed as text in a block definition.")
                    .as_ref(),
            ),
            AttdefDialogMsg::Ok => return self.attdef_dialog_ok(i),
            AttdefDialogMsg::DismissError => state.error = None,
            AttdefDialogMsg::EditValue => {
                // The dialog waits while the multi-line editor collects the value.
                let initial = state.default.clone();
                let height = state.height.trim().parse::<f64>().ok().filter(|h| *h > 0.0).unwrap_or(2.5);
                self.active_modal = None;
                attdef::session().editor_value = None;
                self.tabs[i].active_cmd = Some(Box::new(attdef::AttdefValueCommand));
                let pos = self.tabs[i].scene.camera.borrow().target;
                return self.apply_cmd_result(crate::command::CmdResult::SuspendForMTextInput {
                    pos,
                    initial,
                    height,
                });
            }
            _ => {}
        }
        Task::none()
    }

    /// Show a validation message in the dialog; it stays open.
    fn attdef_dialog_error(&mut self, error: &str) -> Task<Message> {
        if let Some(state) = self.attdef_dialog.as_mut() {
            state.error = Some(crate::t!(error).into_owned());
        }
        Task::none()
    }

    fn attdef_dialog_ok(&mut self, i: usize) -> Task<Message> {
        let Some(state) = self.attdef_dialog.clone() else {
            return Task::none();
        };
        if let Some(error) = tag_error(&state.tag) {
            return self.attdef_dialog_error(error);
        }
        let number = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
        let height = match number(&state.height) {
            Some(v) if v > 0.0 => v,
            _ => {
                return self.attdef_dialog_error("Value must be positive and nonzero.");
            }
        };
        let Some(rotation) = number(&state.rotation) else {
            return self.attdef_dialog_error("Requires valid numeric angle or second point.");
        };
        let width = number(&state.width).unwrap_or(0.0).max(0.0);
        let defaults =
            crate::scene::creation_style::current_text_defaults(&self.tabs[i].scene.document);
        let spec = AttdefSpec {
            aflags: state.aflags,
            annotative: state.annotative,
            tag: state.tag.clone(),
            prompt: state.prompt.clone(),
            value: state.default.clone(),
            justify: state.justify,
            style: state.style.clone(),
            height,
            rotation: rotation.to_radians(),
            width_factor: defaults.width_factor,
            oblique_angle: defaults.oblique_angle,
            boundary_width: width,
            line_spacing: 1.0,
            field: state.field.clone(),
        };
        self.attdef_dialog = None;
        self.close_active_modal();
        {
            let mut session = attdef::session();
            session.aflags = spec.aflags;
            session.annotative = spec.annotative;
            session.on_screen = state.on_screen;
        }
        // The choice is kept with the user settings, as the reference keeps it.
        self.persist_settings_if_changed();
        self.reset_command_start_state(i);
        if state.align_below {
            let previous = attdef::session().last.clone();
            if let Some(previous) = previous {
                let entity = attdef::build_below(&spec, &previous);
                self.tabs[i].active_cmd = Some(Box::new(AttdefPlaceCommand::new(spec.clone())));
                attdef::remember(&spec, &entity);
                return self.apply_cmd_result(crate::command::CmdResult::CommitAndExit(entity));
            }
        }
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        let mut command = AttdefPlaceCommand::new(spec);
        command.set_working_plane(plane);
        if state.on_screen {
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return self.focus_cmd_input();
        }
        let local = glam::DVec3::new(
            number(&state.coords[0]).unwrap_or(0.0),
            number(&state.coords[1]).unwrap_or(0.0),
            number(&state.coords[2]).unwrap_or(0.0),
        );
        let result = command.on_point(plane.to_world(local));
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.apply_cmd_result(result)
    }

    fn attdef_edit_ok(&mut self, i: usize) -> Task<Message> {
        let Some(state) = self.attdef_edit.clone() else {
            return Task::none();
        };
        if let Some(error) = tag_error(&state.tag) {
            if let Some(edit) = self.attdef_edit.as_mut() {
                edit.error = Some(crate::t!(error).into_owned());
            }
            return Task::none();
        }
        self.attdef_edit = None;
        self.close_active_modal();
        let changed = matches!(
            self.tabs[i].scene.document.get_entity(state.handle),
            Some(codec::EntityType::AttributeDefinition(a))
                if a.tag != attdef::normalize_tag(&state.tag) || a.prompt != state.prompt || a.default_value != state.default
        ) || state.field_changed;
        if changed && !self.reject_locked_edit(i, state.handle) {
            self.push_undo_snapshot(i, "DDEDIT");
            if let Some(codec::EntityType::AttributeDefinition(a)) =
                self.tabs[i].scene.document.get_entity_mut(state.handle)
            {
                a.tag = attdef::normalize_tag(&state.tag);
                if !a.flags.constant {
                    a.prompt = state.prompt.clone();
                }
                a.default_value = state.default.clone();
            }
            if state.field_changed {
                crate::entities::field::set_text_field(
                    &mut self.tabs[i].scene.document,
                    state.handle,
                    state.field.clone(),
                    &state.default,
                );
            }
            self.tabs[i]
                .scene
                .bump_entities(&[(state.handle, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
            self.refresh_properties();
        }
        self.post_editor_closed(changed)
    }
}
