use crate::app::{Message, OpenCADStudio};
use iced::Task;

const NAMES: &[&str] = &[
    "APSTATE",
    "BLOCKEDITOR",
    "BLOCKTESTWINDOW",
    "BLOCKEDITLOCK",
    "BACTIONCOLOR",
    "BPARAMETERCOLOR",
    "BGRIPOBJCOLOR",
    "BGRIPOBJSIZE",
    "BPARAMETERSIZE",
    "BPARAMETERFONT",
    "BPTEXTHORIZONTAL",
    "BDEPENDENCYHIGHLIGHT",
    "BTMARKDISPLAY",
    "GRIPDYNCOLOR",
    "GRIPTIPS",
    "BACTIONBARMODE",
    "BCONSTATUSMODE",
];

inventory::submit!(crate::command::CommandRegistration { names: NAMES });
inventory::submit!(crate::command::CommandRegistration {
    names: &["BESETTINGS"]
});

pub(super) fn setvar_listing() -> String {
    format!("SETVAR (block editor): {}", NAMES.join(" "))
}

impl OpenCADStudio {
    pub(in crate::app) fn block_editor_session_variable(&self, name: &str, i: usize) -> Option<u8> {
        let tab = self.tabs.get(i)?;
        Some(match name {
            // The palettes are a pane, so the answer is the pane the screen
            // renders: this one reads the active tab, not `i`.
            "APSTATE" => {
                u8::from(self.dock_panel_visible(crate::ui::dock::PanelId::AuthoringPalettes))
            }
            "BLOCKEDITOR" => u8::from(tab.active_block_edit_session().is_some()),
            "BLOCKTESTWINDOW" => u8::from(tab.block_test.is_some()),
            "BLOCKEDITLOCK" => u8::from(self.block_editor_preferences.locked),
            "BCONSTATUSMODE" => u8::from(tab.scene.block_constraint_status_mode),
            _ => return None,
        })
    }

    pub(super) fn dispatch_block_editor_vars(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        let command = cmd.trim_start_matches(['_', '.']);
        if command == "BESETTINGS" {
            if self.tabs[i].active_block_edit_session().is_none() {
                self.command_line.push_error(
                    crate::t!("BESETTINGS requires an active Block Editor session.").as_ref(),
                );
            } else {
                self.block_editor_settings =
                    Some(crate::ui::window::block_editor_settings::State::new(
                        &self.block_editor_preferences,
                    ));
                self.active_modal = Some(crate::app::ModalKind::BlockEditorSettings);
                self.modal_resize = iced::Vector::ZERO;
            }
            return Some(Task::none());
        }
        let rest = command.strip_prefix("SETVAR ").unwrap_or(command).trim();
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next()?.to_ascii_uppercase();
        if !NAMES.contains(&name.as_str()) {
            return None;
        }
        let value = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if name == "BCONSTATUSMODE" { return Some(self.dispatch_constraint_status_mode(i, value)); }
        if let Some(current) = self.block_editor_preferences.display_variable(&name) {
            if let Some(value) = value {
                match self
                    .block_editor_preferences
                    .set_display_variable(&name, value)
                {
                    Ok(()) => {
                        if self.pending_setvar.as_deref() == Some(name.as_str()) {
                            self.pending_setvar = None;
                        }
                        if name == "BACTIONBARMODE" { self.refresh_properties(); }
                        self.persist_settings_if_changed();
                        self.command_line.push_output(&format!(
                            "{name} = {}",
                            self.block_editor_preferences
                                .display_variable(&name)
                                .unwrap()
                        ));
                    }
                    Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
                }
            } else {
                self.command_line
                    .push_output(&format!("Enter new value for {name} <{current}>:"));
                self.pending_setvar = Some(name);
            }
            return Some(Task::none());
        }
        let current = self.block_editor_session_variable(&name, i).unwrap_or(0);
        if name != "BLOCKEDITLOCK" {
            if value.is_some() {
                self.command_line
                    .push_error(crate::tf!("{} is read only.", name).as_ref());
            } else {
                self.command_line
                    .push_output(&format!("{name} = {current} (read only)"));
            }
        } else if let Some(value) = value {
            match value {
                "0" | "1" => {
                    self.block_editor_preferences.locked = value == "1";
                    if self.pending_setvar.as_deref() == Some(name.as_str()) {
                        self.pending_setvar = None;
                    }
                    self.persist_settings_if_changed();
                    self.command_line.push_output(&format!("{name} = {value}"));
                }
                _ => self
                    .command_line
                    .push_error(crate::t!("BLOCKEDITLOCK requires 0 or 1.").as_ref()),
            }
        } else {
            self.command_line
                .push_output(&format!("Enter new value for {name} <{current}>:"));
            self.pending_setvar = Some(name);
        }
        Some(Task::none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::{entities::Insert, types::Vector3, EntityType, Handle};

    fn fixture() -> (OpenCADStudio, Handle, Handle) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .define_block_raw("STATE", Vector3::ZERO, vec![]);
        let definition = app.tabs[i]
            .scene
            .document
            .block_records
            .get("STATE")
            .unwrap()
            .handle;
        let insert = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new("STATE", Vector3::ZERO)))
            .unwrap();
        (app, definition, insert)
    }

    #[test]
    fn block_editor_session_variables_follow_editor_palette_and_test_window_lifecycles() {
        let (mut app, _, reference) = fixture();
        let source = app.active_tab;
        for name in ["APSTATE", "BLOCKEDITOR", "BLOCKTESTWINDOW"] {
            let _ = app.dispatch_command(&format!("SETVAR {name}"));
            assert!(app
                .command_line
                .history
                .last()
                .unwrap()
                .text
                .contains(&format!("{name} = 0")));
        }
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
        assert_eq!(
            app.block_editor_session_variable("BLOCKEDITOR", source),
            Some(1)
        );
        let _ = app.dispatch_command("BAUTHORPALETTE");
        assert_eq!(
            app.block_editor_session_variable("APSTATE", source),
            Some(1)
        );
        let before = app.tabs[source].scene.document.clone();
        let depth = app.tabs[source].history.undo_stack.len();
        for name in ["APSTATE", "BLOCKEDITOR", "BLOCKTESTWINDOW"] {
            let _ = app.dispatch_command(&format!("SETVAR {name} 0"));
            assert!(app
                .command_line
                .last_error
                .as_ref()
                .unwrap()
                .contains("read only"));
        }
        assert_eq!(app.tabs[source].scene.document, before);
        assert_eq!(app.tabs[source].history.undo_stack.len(), depth);
        let _ = app.dispatch_command("BTESTBLOCK");
        let test = app.active_tab;
        assert_ne!(test, source);
        assert_eq!(
            app.block_editor_session_variable("BLOCKEDITOR", test),
            Some(0)
        );
        assert_eq!(app.block_editor_session_variable("APSTATE", test), Some(0));
        assert_eq!(
            app.block_editor_session_variable("BLOCKTESTWINDOW", test),
            Some(1)
        );
        let _ = app.dispatch_command("BCLOSETESTBLOCK");
        assert_eq!(app.active_tab, source);
        app.close_block_edit(source, false);
        assert_eq!(
            app.block_editor_session_variable("BLOCKEDITOR", source),
            Some(0)
        );
        assert_eq!(
            app.block_editor_session_variable("APSTATE", source),
            Some(0)
        );
    }

    /// The action-bar mode is one of the block editor's own variables: the bare
    /// form answers with the value it holds, and that value decides whether a bar
    /// is displayed at all.
    #[test]
    fn the_action_bar_mode_variable_reads_back_and_gates_the_bars() {
        let (mut app, _, reference) = fixture();
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
        let _ = app.run_command_line("BPARAMETER LOOKUP 3,3");
        let bounds =
            iced::Rectangle::new(iced::Point::new(0.0, 0.0), iced::Size::new(800.0, 600.0));
        let last = |app: &OpenCADStudio| app.command_line.history.last().unwrap().text.clone();
        assert!(
            !app.action_bar_layout(bounds).is_empty(),
            "a parameter that carries a bar shows it while the mode is on"
        );

        let _ = app.dispatch_command("SETVAR BACTIONBARMODE");
        assert!(
            last(&app).starts_with("Enter new value for BACTIONBARMODE <"),
            "the bare form shows the value it holds: {}",
            last(&app)
        );
        assert!(
            last(&app).contains("<1>"),
            "and the value the variable holds at that point is one"
        );

        let _ = app.dispatch_command("SETVAR BACTIONBARMODE 0");
        assert!(
            last(&app).contains("BACTIONBARMODE = 0"),
            "the write answers with the new value"
        );
        assert!(
            app.action_bar_layout(bounds).is_empty(),
            "with the mode off no bar is laid out"
        );
        assert_eq!(
            app.action_bar_error(),
            Some("Set BACTIONBARMODE to 1 to display action bars."),
            "and the bar command says why"
        );

        let _ = app.dispatch_command("SETVAR BACTIONBARMODE");
        assert!(
            last(&app).contains("<0>"),
            "the read now reports the value the write set"
        );

        let _ = app.dispatch_command("BACTIONBAR");
        assert!(
            app.command_line
                .last_error
                .as_ref()
                .is_some_and(|error| error.contains("Set BACTIONBARMODE to 1")),
            "the bar command refused off the command line prints that reason too"
        );
        assert!(
            app.tabs[app.active_tab].active_cmd.is_none(),
            "and the refused bar command leaves nothing running"
        );

        let _ = app.dispatch_command("SETVAR BACTIONBARMODE 1");
        assert_eq!(
            app.action_bar_error(),
            None,
            "with the mode back on the refusal is gone"
        );
        assert!(
            !app.action_bar_layout(bounds).is_empty(),
            "and the bar is laid out again"
        );
        let _ = app.dispatch_command("BEDIT_DISCARD");
    }

    #[test]
    fn the_authoring_palette_variable_reports_the_pane_the_screen_shows() {
        use crate::ui::dock::PanelId;
        let mut bare = OpenCADStudio::new_for_test();
        bare.show_authoring_palettes = true;
        assert!(
            bare.tabs[bare.active_tab].is_start,
            "the app opens on the start screen, which carries no panels"
        );
        assert_eq!(
            bare.block_editor_session_variable("APSTATE", bare.active_tab),
            Some(0),
            "so a palette the toggle wants is still not on the screen"
        );
        let (mut app, _, reference) = fixture();
        let i = app.active_tab;
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
        let _ = app.dispatch_command("BAUTHORPALETTE");
        assert!(app.dock_panel_visible(PanelId::AuthoringPalettes));
        assert_eq!(
            app.block_editor_session_variable("APSTATE", i),
            Some(1),
            "the palettes are open, so the variable says so"
        );
        let _ = app.update(Message::ToggleCleanScreen);
        assert!(app.clean_screen, "the viewport is cleared of panels");
        assert!(
            app.show_authoring_palettes,
            "while the flag the toggle command sets stays set"
        );
        assert!(
            !app.dock_panel_visible(PanelId::AuthoringPalettes),
            "and none of the palettes is rendered"
        );
        assert_eq!(
            app.block_editor_session_variable("APSTATE", i),
            Some(0),
            "so the variable cannot report palettes the screen is not showing"
        );
        let _ = app.update(Message::ToggleCleanScreen);
        assert_eq!(
            app.block_editor_session_variable("APSTATE", i),
            Some(1),
            "and it comes back with the pane"
        );
        let _ = app.dispatch_command("BEDIT_DISCARD");
    }

    #[test]
    fn block_editor_lock_refuses_every_entry_without_canceling_a_running_command() {
        let (mut app, definition, reference) = fixture();
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let _ = app.dispatch_command("LINE");
        let _ = app.dispatch_command("SETVAR BLOCKEDITLOCK 1");
        for command in [
            "BEDIT".to_owned(),
            format!("BEDIT_BEGIN:{}", reference.value()),
            "BEDIT_OPEN:STATE".into(),
            "BEDIT_NEW:REFUSED".into(),
            "._BEDIT".into(),
        ] {
            let _ = app.dispatch_command(&command);
            assert!(app.tabs[i].active_block_edit_session().is_none());
            assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
            assert_eq!(app.tabs[i].scene.document, before);
        }
        app.open_bedit_dialog(i);
        assert!(app.active_modal.is_none());
        app.start_block_edit_session(i, "STATE".into(), definition, None);
        assert!(app.tabs[i].active_block_edit_session().is_none());
        let _ = app.dispatch_command("SETVAR BLOCKEDITLOCK 2");
        assert!(app.block_editor_preferences.locked);
        let _ = app.dispatch_command("SETVAR BLOCKEDITLOCK 0");
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
        assert!(app.tabs[i].active_block_edit_session().is_some());
    }

    #[test]
    fn block_editor_lock_persists_but_derived_session_variables_do_not() {
        let (mut app, _, _) = fixture();
        let i = app.active_tab;
        let _ = app.dispatch_command("BLOCKEDITLOCK");
        assert_eq!(app.pending_setvar.as_deref(), Some("BLOCKEDITLOCK"));
        let _ = app.dispatch_command("SETVAR BLOCKEDITLOCK 1");
        assert!(app.pending_setvar.is_none());
        let saved = serde_json::to_value(app.current_settings()).unwrap();
        assert_eq!(saved["block_editor"]["locked"], true);
        assert!(saved["block_editor"].get("BLOCKEDITOR").is_none());
        let loaded = serde_json::from_value(saved).unwrap();
        let mut restored = OpenCADStudio::new_for_test();
        restored.apply_settings(&loaded);
        assert!(restored.block_editor_preferences.locked);
        let defaults: crate::app::settings::UserSettings = serde_json::from_str("{}").unwrap();
        assert!(!defaults.block_editor.locked);
        let _ = app.dispatch_command("LINE");
        let _ = app.dispatch_command("._SETVAR APSTATE");
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }

    #[test]
    fn block_editor_display_variables_validate_persist_and_preserve_command_and_document() {
        let (mut app, _, _) = fixture();
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let _ = app.dispatch_command("LINE");
        for (name, value, expected) in [
            ("BACTIONCOLOR", "rgb:1,2,3", "RGB:1,2,3"),
            ("BPARAMETERCOLOR", "bylayer", "BYLAYER"),
            ("BGRIPOBJCOLOR", "255", "255"),
            ("BGRIPOBJSIZE", "255", "255"),
            ("BPARAMETERSIZE", "1", "1"),
            ("BPARAMETERFONT", "Arial Unicode MS", "Arial Unicode MS"),
            ("BPTEXTHORIZONTAL", "0", "0"),
            ("BDEPENDENCYHIGHLIGHT", "0", "0"),
            ("BTMARKDISPLAY", "0", "0"),
            ("GRIPDYNCOLOR", "255", "255"),
            ("GRIPTIPS", "0", "0"),
        ] {
            let _ = app.dispatch_command(&format!("SETVAR {name} {value}"));
            assert_eq!(
                app.block_editor_preferences
                    .display_variable(name)
                    .as_deref(),
                Some(expected)
            );
            assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
        }
        let valid = app.block_editor_preferences.clone();
        for command in [
            "BGRIPOBJSIZE 0",
            "BPARAMETERSIZE 256",
            "BACTIONCOLOR RGB:1,2,999",
            "BGRIPOBJCOLOR -1",
            "BPTEXTHORIZONTAL 2",
            "BDEPENDENCYHIGHLIGHT 2",
            "BTMARKDISPLAY -1",
            "GRIPDYNCOLOR 0",
            "GRIPDYNCOLOR 256",
            "GRIPTIPS 5",
            "BPARAMETERFONT \"\"",
        ] {
            let _ = app.dispatch_command(command);
            assert_eq!(app.block_editor_preferences, valid, "{command}");
        }
        let saved = serde_json::to_value(app.current_settings()).unwrap();
        let mut restored = OpenCADStudio::new_for_test();
        restored.apply_settings(&serde_json::from_value(saved).unwrap());
        assert_eq!(restored.block_editor_preferences, valid);
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    }

    #[test]
    fn block_editor_settings_apply_is_atomic_and_cancel_discards_staged_changes() {
        let (mut app, _, reference) = fixture();
        let _ = app.dispatch_command("LINE");
        let _ = app.dispatch_command("BESETTINGS");
        assert!(app.active_modal.is_none());
        assert_eq!(
            app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
            "LINE"
        );
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
        let baseline = app.block_editor_preferences.clone();
        let _ = app.dispatch_command("BESETTINGS");
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::BlockEditorSettings)
        );
        let _ = app.update(Message::BlockEditorSettingsInput(0, "RGB:12,34,56".into()));
        let _ = app.update(Message::BlockEditorSettingsInput(3, "0".into()));
        let _ = app.update(Message::BlockEditorSettingsApply);
        assert_eq!(app.block_editor_preferences, baseline);
        assert!(app.block_editor_settings.as_ref().unwrap().error.is_some());
        let _ = app.update(Message::BlockEditorSettingsInput(3, "32".into()));
        let _ = app.update(Message::BlockEditorSettingsApply);
        assert_eq!(app.block_editor_preferences.action_color, "RGB:12,34,56");
        assert_eq!(app.block_editor_preferences.grip_size, 32);
        assert!(app.active_modal.is_none());
        let applied = app.block_editor_preferences.clone();
        let _ = app.dispatch_command("BESETTINGS");
        let _ = app.update(Message::BlockEditorSettingsReset);
        assert_eq!(app.block_editor_preferences, applied);
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.block_editor_preferences, applied);
        assert!(app.block_editor_settings.is_none());
        let _ = app.dispatch_command("BESETTINGS");
        let _ = app.update(Message::BlockEditorSettingsReset);
        let _ = app.update(Message::BlockEditorSettingsApply);
        assert_eq!(app.block_editor_preferences, baseline);
    }
}
