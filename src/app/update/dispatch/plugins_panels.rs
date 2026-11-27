use super::super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_plugins_panels(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::PanelTableCellDraft {
                owner,
                panel,
                row,
                column,
                text,
            } => {
                // Host-local draft state; the stored table is untouched.
                self.panel_table_ui
                    .entry(panel.clone())
                    .or_default()
                    .set_draft(row, column, text);
                let _ = owner;
                Task::none()
            }
            Message::PanelTableCellSelected { owner, panel, row, column } => {
                self.panel_table_ui
                    .entry(panel.clone())
                    .or_default()
                    .select((row, column));
                let _ = owner;
                Task::none()
            }
            Message::PanelTableCellCommitted {
                owner,
                panel,
                row,
                column,
            } => {
                // Typed-commit rule: the edited value takes the existing
                // cell's type; a failed parse keeps the draft. A commit
                // notifies the plugin (best-effort, owner-targeted) — the
                // plugin stays the data authority and canonicalizes with
                // its own table patch.
                let stored = self
                    .panel_surfaces
                    .table(&owner, &panel)
                    .and_then(|surface| {
                        let index = surface.columns.iter().position(|c| c.id == column)?;
                        surface
                            .rows
                            .get(&row)
                            .and_then(|cells| cells.get(index))
                            .cloned()
                    });
                let value = self
                    .panel_table_ui
                    .entry(panel.clone())
                    .or_default()
                    .commit_draft(row, column, stored.as_ref());
                if let Ok(value) = value {
                    let routed = self
                        .panel_event_router
                        .on_cell_edited(&panel, row, column, value);
                    #[cfg(not(target_arch = "wasm32"))]
                    crate::plugin::v4_support::publish_surface_event_to_plugin(
                        &owner, routed.panel_id, routed.sequence, routed.event,
                    );
                    #[cfg(target_arch = "wasm32")]
                    let _ = routed;
                }
                Task::none()
            }

            Message::PluginDialogTextChange(id, val) => {
                // Each edit reaches the plugin as a "change", as a panel's
                // text field and a dialog's file field already report it,
                // so it can follow the input (filter a list, check a name).
                // A Number field reports its text as typed, partial or not.
                if let Some(state) = &mut self.plugin_dialog_state {
                    state.text_values.insert(id.clone(), val.clone());
                    return self.dispatch_plugin_dialog_action(
                        id,
                        "change".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::Text(val)),
                    );
                }
                Task::none()
            }
            Message::PluginDialogBoolToggle(id, val) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    state.bool_values.insert(id.clone(), val);
                    return self.dispatch_plugin_dialog_action(
                        id,
                        "toggle".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::Bool(val)),
                    );
                }
                Task::none()
            }
            Message::PluginDialogChoiceSelect(id, idx) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    state.choice_values.insert(id.clone(), idx);
                    let opt_text = state.choice_option_text(&id, idx);
                    let val = ocs_plugin_api::dialog::DialogValue::Choice {
                        index: idx,
                        text: opt_text,
                    };
                    return self.dispatch_plugin_dialog_action(id, "change".to_string(), Some(val));
                }
                Task::none()
            }
            Message::PluginDialogColorChange(id, rgb) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    state.color_values.insert(id.clone(), rgb);
                    return self.dispatch_plugin_dialog_action(
                        id,
                        "change".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::Color(rgb)),
                    );
                }
                Task::none()
            }
            Message::PluginDialogTableCellChange(id, row, col, val) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    if let Some(rows) = state.table_values.get_mut(&id) {
                        if let Some(row_data) = rows.get_mut(row) {
                            if let Some(cell) = row_data.get_mut(col) {
                                *cell = val;
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::PluginDialogTableAddRow(id) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    let col_count = state.find_table_col_count(&id);
                    if let Some(rows) = state.table_values.get_mut(&id) {
                        rows.push(vec![String::new(); col_count]);
                    }
                    return self.dispatch_plugin_dialog_action(id, "add_row".to_string(), None);
                }
                Task::none()
            }
            Message::PluginDialogTableRemoveRow(id, row_idx) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    if let Some(rows) = state.table_values.get_mut(&id) {
                        if row_idx < rows.len() {
                            rows.remove(row_idx);
                        }
                    }
                    return self.dispatch_plugin_dialog_action(
                        id,
                        "remove_row".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::Number(row_idx as f64)),
                    );
                }
                Task::none()
            }
            Message::PluginDialogSectionToggle(id) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    if state.collapsed_sections.contains(&id) {
                        state.collapsed_sections.remove(&id);
                    } else {
                        state.collapsed_sections.insert(id);
                    }
                }
                Task::none()
            }
            Message::PluginDialogFilePick(id) => {
                use crate::ui::window::plugin_dialog::{pick_path, pick_target, PickTarget};
                let (target, start) = match &self.plugin_dialog_state {
                    Some(state) => (
                        state
                            .def
                            .rows
                            .iter()
                            .find_map(|r| pick_target(&r.controls, &id)),
                        state.file_values.get(&id).map(std::path::PathBuf::from),
                    ),
                    None => (None, None),
                };
                let target = target.unwrap_or(PickTarget::File {
                    save: false,
                    extensions: Vec::new(),
                });
                let id_clone = id.clone();
                iced::Task::perform(pick_path(target, start), move |path| {
                    Message::PluginDialogFilePicked(id_clone, path)
                })
            }
            Message::PluginDialogFilePicked(id, path) => {
                if let Some(p) = path {
                    let path_str = p.to_string_lossy().to_string();
                    if let Some(state) = &mut self.plugin_dialog_state {
                        state.file_values.insert(id.clone(), path_str.clone());
                    }
                    return self.dispatch_plugin_dialog_action(
                        id,
                        "pick".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::FilePath(path_str)),
                    );
                }
                Task::none()
            }
            Message::PluginDialogDate(id, msg) => {
                let Some(state) = &mut self.plugin_dialog_state else {
                    return Task::none();
                };
                let dismissed = msg == crate::ui::window::date_field::DateMsg::Close
                    && state
                        .open_calendar
                        .as_ref()
                        .is_some_and(|(o, _, _)| *o == id);
                if dismissed {
                    self.calendar_dismissed = Some(iced::time::Instant::now());
                }
                let Some(state) = &mut self.plugin_dialog_state else {
                    return Task::none();
                };
                let controls: Vec<_> = state
                    .def
                    .rows
                    .iter()
                    .flat_map(|r| r.controls.iter().cloned())
                    .collect();
                match crate::ui::window::date_field::step(
                    &mut state.open_calendar,
                    &mut state.text_values,
                    &controls,
                    &id,
                    msg,
                ) {
                    Some(picked) => self.dispatch_plugin_dialog_action(
                        id,
                        "pick".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::Text(picked)),
                    ),
                    None => Task::none(),
                }
            }
            Message::PluginDialogFileTextChange(id, val) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    state.file_values.insert(id.clone(), val.clone());
                    return self.dispatch_plugin_dialog_action(
                        id,
                        "change".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::FilePath(val)),
                    );
                }
                Task::none()
            }
            Message::PluginDialogButtonClick(id, action) => {
                let tab_idx = self.dialog_target_tab_index();
                let action_for_event = action.clone().unwrap_or_else(|| "click".to_string());
                let (action_resp, panel_updates, dialog_meta) =
                    if let Some(state) = &self.plugin_dialog_state {
                        let current_values = state.collect_response_map();
                        let dialog_title = state.def.title.clone();
                        let owner_plugin_id = state.plugin_id.clone();
                        let owner_command_id = state.command_id;
                        let event = ocs_plugin_api::dialog::DialogEvent {
                            control_id: id.clone(),
                            action: action_for_event,
                            value: None,
                            current_values: current_values.clone(),
                        };
                        if let Some(cmd) = self
                            .tabs
                            .get_mut(tab_idx)
                            .and_then(|t| t.active_cmd.as_mut())
                        {
                            let resp = cmd.on_dialog_action(event);
                            let updates = cmd.take_pending_panel_updates();
                            (
                                Some(resp),
                                updates,
                                Some((
                                    dialog_title,
                                    owner_plugin_id,
                                    owner_command_id,
                                    current_values,
                                )),
                            )
                        } else {
                            (
                                None,
                                Vec::new(),
                                Some((
                                    dialog_title,
                                    owner_plugin_id,
                                    owner_command_id,
                                    current_values,
                                )),
                            )
                        }
                    } else {
                        (None, Vec::new(), None)
                    };

                let had_pending_updates = !panel_updates.is_empty();
                self.apply_panel_or_dialog_updates(panel_updates);

                match action_resp {
                    Some(ocs_plugin_api::dialog::DialogActionResponse::UpdateValues(values)) => {
                        if let Some(state) = &mut self.plugin_dialog_state {
                            state.update_values(values);
                        }
                        return Task::none();
                    }
                    Some(ocs_plugin_api::dialog::DialogActionResponse::ReplaceDialog(new_def)) => {
                        if let Some(state) = &mut self.plugin_dialog_state {
                            let tab_id = state.tab_id;
                            let plugin_id = state.plugin_id.clone();
                            let command_id = state.command_id;
                            *state =
                                crate::ui::window::plugin_dialog::PluginDialogState::new(*new_def)
                                    .with_owner(tab_id, plugin_id, command_id);
                        }
                        return Task::none();
                    }
                    Some(ocs_plugin_api::dialog::DialogActionResponse::Submit) => {
                        return self.on_plugin_dialog_submit();
                    }
                    Some(ocs_plugin_api::dialog::DialogActionResponse::Close) => {
                        return self.on_plugin_dialog_cancel();
                    }
                    Some(ocs_plugin_api::dialog::DialogActionResponse::None) | None => {
                        if had_pending_updates {
                            return Task::none();
                        }
                    }
                }

                // Fall back to the panel button click path when on_dialog_action returns None.
                let action_str = action.unwrap_or_default();
                let lower = action_str.to_ascii_lowercase();
                if lower == "close" || lower == "hide" {
                    return self.on_plugin_dialog_cancel();
                } else if lower == "clear" {
                    if let Some(state) = &mut self.plugin_dialog_state {
                        for rows in state.table_values.values_mut() {
                            rows.clear();
                        }
                    }
                    if let Some((dialog_title, owner_plugin_id, owner_command_id, current_values)) =
                        &dialog_meta
                    {
                        crate::plugin::v4_support::publish_panel_action_to_plugin(
                            owner_plugin_id.as_deref(),
                            *owner_command_id,
                            dialog_title.clone(),
                            id,
                            "clear".to_string(),
                            Some(ocs_plugin_api::dialog::DialogValue::Fields(
                                current_values.clone(),
                            )),
                        );
                    }
                    return Task::none();
                } else if lower == "reset" {
                    if let Some(state) = &mut self.plugin_dialog_state {
                        state.reset_values();
                    }
                    if let Some((dialog_title, owner_plugin_id, owner_command_id, current_values)) =
                        &dialog_meta
                    {
                        crate::plugin::v4_support::publish_panel_action_to_plugin(
                            owner_plugin_id.as_deref(),
                            *owner_command_id,
                            dialog_title.clone(),
                            id,
                            "reset".to_string(),
                            Some(ocs_plugin_api::dialog::DialogValue::Fields(
                                current_values.clone(),
                            )),
                        );
                    }
                    return Task::none();
                }

                // A button whose action names a command runs it, as panel
                // buttons do; a plain button is a click event for the plugin.
                if !action_str.is_empty() {
                    return self.dispatch_command(&action_str);
                }
                if let Some((dialog_title, owner_plugin_id, owner_command_id, current_values)) =
                    &dialog_meta
                {
                    crate::plugin::v4_support::publish_panel_action_to_plugin(
                        owner_plugin_id.as_deref(),
                        *owner_command_id,
                        dialog_title.clone(),
                        id,
                        "click".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::Fields(
                            current_values.clone(),
                        )),
                    );
                }
                Task::none()
            }
            Message::PluginDialogControlEvent(id, action, val) => {
                self.dispatch_plugin_dialog_action(id, action, val)
            }
            Message::PluginDialogSubmit => self.on_plugin_dialog_submit(),
            Message::PluginDialogCancel => self.on_plugin_dialog_cancel(),

            Message::PluginPanelButtonClick(panel_id, control_id, action) => {
                let action_str = action.unwrap_or_default();
                let lower = action_str.to_ascii_lowercase();

                // 1. Built-in keywords
                if lower == "close" || lower == "hide" {
                    if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                        panel.visible = false;
                    }
                    return Task::none();
                } else if lower == "clear" {
                    if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                        for rows in panel.table_values.values_mut() {
                            rows.clear();
                        }
                    }
                    crate::plugin::v4_support::publish_panel_action(
                        panel_id,
                        control_id,
                        "clear".to_string(),
                        None,
                    );
                    return Task::none();
                } else if lower == "reset" {
                    if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                        let def = panel.def.clone();
                        let owner = panel.owner_plugin_id.clone();
                        *panel = crate::ui::window::plugin_panel::PluginPanelState::from_def(def);
                        panel.owner_plugin_id = owner;
                    }
                    crate::plugin::v4_support::publish_panel_action(
                        panel_id,
                        control_id,
                        "reset".to_string(),
                        None,
                    );
                    return Task::none();
                }

                // 2. Direct CAD command
                if !action_str.is_empty() {
                    return self.dispatch_command(&action_str);
                }

                // 3. Otherwise broadcast as a button click event to plugin
                crate::plugin::v4_support::publish_panel_action(
                    panel_id,
                    control_id,
                    "click".to_string(),
                    None,
                );
                Task::none()
            }
            Message::PluginPanelTextChange(panel_id, control_id, val) => {
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    panel.text_values.insert(control_id.clone(), val.clone());
                }
                crate::plugin::v4_support::publish_panel_action(
                    panel_id,
                    control_id,
                    "change".to_string(),
                    Some(ocs_plugin_api::dialog::DialogValue::Text(val)),
                );
                Task::none()
            }
            Message::PluginPanelBoolToggle(panel_id, control_id, checked) => {
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    panel.bool_values.insert(control_id.clone(), checked);
                }
                crate::plugin::v4_support::publish_panel_action(
                    panel_id,
                    control_id,
                    "change".to_string(),
                    Some(ocs_plugin_api::dialog::DialogValue::Bool(checked)),
                );
                Task::none()
            }
            Message::PluginPanelChoiceSelect(panel_id, control_id, index, text) => {
                let choice_text = if !text.is_empty() {
                    text
                } else {
                    self.plugin_panels
                        .get(&panel_id)
                        .map(|p| p.choice_option_text(&control_id, index))
                        .unwrap_or_default()
                };
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    panel.choice_values.insert(control_id.clone(), index);
                }
                crate::plugin::v4_support::publish_panel_action(
                    panel_id,
                    control_id,
                    "change".to_string(),
                    Some(ocs_plugin_api::dialog::DialogValue::Choice {
                        index,
                        text: choice_text,
                    }),
                );
                Task::none()
            }
            Message::PluginPanelSectionToggle(panel_id, section_id) => {
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    if panel.collapsed_sections.contains(&section_id) {
                        panel.collapsed_sections.remove(&section_id);
                    } else {
                        panel.collapsed_sections.insert(section_id);
                    }
                }
                Task::none()
            }
            Message::PluginPanelTableCellChange(panel_id, table_id, row, col, val) => {
                let mut current_rows = Vec::new();
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    if let Some(rows) = panel.table_values.get_mut(&table_id) {
                        if let Some(r) = rows.get_mut(row) {
                            if let Some(c) = r.get_mut(col) {
                                *c = val.clone();
                            }
                        }
                        current_rows = rows.clone();
                    }
                }
                crate::plugin::v4_support::publish_panel_action(
                    panel_id,
                    table_id,
                    "cell_change".to_string(),
                    Some(ocs_plugin_api::dialog::DialogValue::Table(current_rows)),
                );
                Task::none()
            }
            Message::PluginPanelTableAddRow(panel_id, table_id) => {
                let mut current_rows = Vec::new();
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    fn find_table_cols(
                        ctrls: &[ocs_plugin_api::dialog::DialogControl],
                        target: &str,
                    ) -> Option<usize> {
                        for c in ctrls {
                            match c {
                                ocs_plugin_api::dialog::DialogControl::Table {
                                    id,
                                    headers,
                                    ..
                                } if id == target => {
                                    return Some(headers.len());
                                }
                                ocs_plugin_api::dialog::DialogControl::Fields {
                                    controls, ..
                                }
                                | ocs_plugin_api::dialog::DialogControl::Row { controls, .. }
                                | ocs_plugin_api::dialog::DialogControl::Column {
                                    controls, ..
                                } => {
                                    if let Some(res) = find_table_cols(controls, target) {
                                        return Some(res);
                                    }
                                }
                                _ => {}
                            }
                        }
                        None
                    }
                    let col_count = find_table_cols(&panel.def.controls, &table_id).unwrap_or(2);
                    let rows = panel.table_values.entry(table_id.clone()).or_default();
                    rows.push(vec![String::new(); col_count]);
                    current_rows = rows.clone();
                }
                crate::plugin::v4_support::publish_panel_action(
                    panel_id,
                    table_id,
                    "add_row".to_string(),
                    Some(ocs_plugin_api::dialog::DialogValue::Table(current_rows)),
                );
                Task::none()
            }
            Message::PluginPanelTableRemoveRow(panel_id, table_id, row_idx) => {
                let mut current_rows = Vec::new();
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    if let Some(rows) = panel.table_values.get_mut(&table_id) {
                        if row_idx < rows.len() {
                            rows.remove(row_idx);
                        }
                        current_rows = rows.clone();
                    }
                }
                crate::plugin::v4_support::publish_panel_action(
                    panel_id,
                    table_id,
                    "remove_row".to_string(),
                    Some(ocs_plugin_api::dialog::DialogValue::Table(current_rows)),
                );
                Task::none()
            }
            Message::PluginPanelFilePick(panel_id, control_id) => {
                use crate::ui::window::plugin_dialog::{pick_path, pick_target, PickTarget};
                // A panel's file field opens files without a filter, as
                // before; its folder field the folder dialog.
                let panel = self.plugin_panels.get(&panel_id);
                let folder = panel.is_some_and(|p| {
                    pick_target(&p.def.controls, &control_id) == Some(PickTarget::Folder)
                });
                let target = if folder {
                    PickTarget::Folder
                } else {
                    PickTarget::File {
                        save: false,
                        extensions: Vec::new(),
                    }
                };
                let start = panel
                    .and_then(|p| p.file_values.get(&control_id))
                    .map(std::path::PathBuf::from);
                let pid = panel_id.clone();
                let cid = control_id.clone();
                iced::Task::perform(pick_path(target, start), move |path| {
                    Message::PluginPanelFilePicked(pid, cid, path)
                })
            }
            Message::PluginPanelDate(panel_id, control_id, msg) => {
                let Some(panel) = self.plugin_panels.get_mut(&panel_id) else {
                    return Task::none();
                };
                if msg == crate::ui::window::date_field::DateMsg::Close
                    && panel
                        .open_calendar
                        .as_ref()
                        .is_some_and(|(o, _, _)| *o == control_id)
                {
                    self.calendar_dismissed = Some(iced::time::Instant::now());
                }
                let Some(panel) = self.plugin_panels.get_mut(&panel_id) else {
                    return Task::none();
                };
                let controls = panel.def.controls.clone();
                if let Some(picked) = crate::ui::window::date_field::step(
                    &mut panel.open_calendar,
                    &mut panel.text_values,
                    &controls,
                    &control_id,
                    msg,
                ) {
                    crate::plugin::v4_support::publish_panel_action(
                        panel_id,
                        control_id,
                        "pick".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::Text(picked)),
                    );
                }
                Task::none()
            }
            Message::PluginPanelFilePicked(panel_id, control_id, path) => {
                if let Some(p) = path {
                    let s = p.to_string_lossy().to_string();
                    if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                        panel.file_values.insert(control_id.clone(), s.clone());
                    }
                    crate::plugin::v4_support::publish_panel_action(
                        panel_id,
                        control_id,
                        "file_picked".to_string(),
                        Some(ocs_plugin_api::dialog::DialogValue::FilePath(s)),
                    );
                }
                Task::none()
            }
            Message::PluginPanelFileTextChange(panel_id, control_id, val) => {
                if let Some(panel) = self.plugin_panels.get_mut(&panel_id) {
                    panel.file_values.insert(control_id.clone(), val.clone());
                }
                crate::plugin::v4_support::publish_panel_action(
                    panel_id,
                    control_id,
                    "change".to_string(),
                    Some(ocs_plugin_api::dialog::DialogValue::FilePath(val)),
                );
                Task::none()
            }

            // ── Document tabs ─────────────────────────────────────────────
            Message::TabNew => {
                if self.mtext_editor.is_some() {
                    self.mtext_cancel();
                }
                self.close_table_cell_editor();
                // Preserve the outgoing drawing's Ortho / running OSNAP.
                self.stamp_header_sysvars(self.active_tab);
                // The quad does not survive the switch to the fresh
                // drawing either (same TabSwitch rule): its window state
                // is per-tab while the panels are app-global.
                self.close_quad();
                self.tab_counter += 1;
                let new_tab = super::super::super::tab_document::DocumentTab::new_drawing(self.tab_counter);
                self.tabs.push(new_tab);
                self.active_tab = self.tabs.len() - 1;
                let idx = self.active_tab;
                // A fresh drawing inherits the app's current Ortho / OSNAP so
                // creating one doesn't silently reset them (its header default is
                // 0 = no snaps); saving then persists them into the file.
                self.stamp_header_sysvars(idx);
                self.apply_display_defaults(idx);
                self.sync_ribbon_layers();
                self.sync_ribbon_styles();
                // #21: reset ribbon Color / Linetype / Lineweight to the
                // fresh tab's defaults (ByLayer) instead of inheriting the
                // previous tab's last selection.
                self.sync_ribbon_from_selection();
                // A fresh drawing starts with grid/snap off (its tile defaults).
                self.adopt_view_display(self.active_tab);
                // QNEW: start from the default template when one is set (#1113).
                let template = self.qnew_template.trim().to_string();
                if !template.is_empty() {
                    match self.apply_template(&template) {
                        Ok(_) => {
                            self.tabs[idx].dirty = false;
                            self.refresh_layer_panel();
                            self.sync_ribbon_styles();
                        }
                        Err(error) => self
                            .command_line
                            .push_error(error["error"].as_str().unwrap_or("template")),
                    }
                }
                Task::none()
            }

            Message::TabSwitch(idx) => {
                self.doc_tab_layouts = None;
                if idx != self.active_tab {
                    self.close_table_cell_editor();
                    self.release_pointer_navigation();
                }
                if self.dialog_open(super::super::super::ModalKind::Recovery) {
                    return Task::none();
                }
                if idx < self.tabs.len() && idx != self.active_tab && self.mtext_editor.is_some() {
                    self.mtext_cancel();
                }
                self.layout_list_open = false;
                self.layout_rename_state = None;
                for tab in &mut self.tabs {
                    tab.properties.open_block_picker = None;
                    tab.properties.open_choice = None;
                }
                self.block_picker_dismissed = false;
                if idx < self.tabs.len() {
                    if idx != self.active_tab {
                        // The attribute editor is tab-scoped; leaving its tab
                        // drops it (its handle is that document's, not this one's).
                        self.cancel_attr_editor();
                        // Tool dialogs working on the outgoing drawing close
                        // with it; program-wide ones (Options, …) stay.
                        self.close_document_tools(None);
                        // The quad is cursor UI: like the legacy menu on the
                        // disrupted transitions, it does not ride a tab
                        // switch (its per-tab window state rides
                        // app-global panels, so leaving it open would let a
                        // pick re-target the outgoing drawing's handles).
                        self.close_quad();
                        // The Block Properties Table list is tab-scoped for the
                        // same reason: it names an insert of the outgoing drawing.
                        self.table_popup = None;
                        // Persist the outgoing drawing's Ortho / running OSNAP
                        // before leaving it, so switching back restores them.
                        let prev = self.active_tab;
                        self.stamp_header_sysvars(prev);
                    }
                    self.active_tab = idx;
                    if self.tabs[idx].is_start {
                        self.ribbon.close_dropdown();
                    }
                    if self.tabs[idx].is_start
                        && matches!(
                            self.active_modal,
                            Some(
                                super::super::super::ModalKind::LayoutManager
                                    | super::super::super::ModalKind::LayerStateManager
                                    | super::super::super::ModalKind::LayerStateEditor
                            )
                        )
                    {
                        self.close_active_modal();
                    } else if self.active_modal == Some(super::super::super::ModalKind::LayerStateEditor) {
                        self.close_active_modal();
                    } else if self.active_modal == Some(super::super::super::ModalKind::LayerStateManager) {
                        let mut names: Vec<String> = self.tabs[idx]
                            .scene
                            .document
                            .layer_states()
                            .into_iter()
                            .map(|state| state.name)
                            .collect();
                        names.sort_by_key(|name| name.to_lowercase());
                        self.load_layer_state_editor(names.into_iter().next());
                    }
                    self.sync_ribbon_layers();
                    self.sync_ribbon_styles();
                    // #21: also re-seed ribbon Color / Linetype / Lineweight
                    // from the newly active tab so they reflect that doc's
                    // CECOLOR / CELTYPE / CELWEIGHT (or its current selection
                    // if there is one), not the prior tab's choice.
                    self.sync_ribbon_from_selection();
                    // Grid/snap follow the newly active drawing's viewport.
                    self.adopt_view_display(idx);
                    // Ortho / running OSNAP follow the newly active drawing.
                    self.adopt_header_sysvars(idx);
                    // Shared CJK ideographs follow the newly active drawing's
                    // language; re-tessellate if it differs from the last. (#141)
                    if crate::scene::text::web_font::set_cjk_lang_from_codepage(
                        &self.tabs[idx].scene.document.header.code_page,
                    ) {
                        crate::scene::text::ttf_glyph::clear_fallback_cache();
                        self.tabs[idx].scene.invalidate_text_geometry_dependencies();
                    }
                }
                Task::none()
            }

            Message::DocTabHover(index) => {
                let index = index.filter(|&idx| idx < self.tabs.len());
                if index == self.hovered_doc_tab {
                    return Task::none();
                }
                self.hovered_doc_tab = index;
                self.doc_tab_hover_gen = self.doc_tab_hover_gen.wrapping_add(1);
                // Another tab's rest closes the open list; leaving for the
                // list itself keeps it.
                if index.is_some() && index != self.doc_tab_layouts {
                    self.doc_tab_layouts = None;
                }
                match index {
                    // Not while a menu is open.
                    Some(idx) if !self.doc_tab_menu => {
                        let gen = self.doc_tab_hover_gen;
                        let delay = std::time::Duration::from_millis(
                            crate::ui::style::kit_metrics::TAB_HOVER_MS,
                        );
                        Task::perform(rest(delay), move |_| {
                            Message::DocTabLayoutsSettled(idx, gen)
                        })
                    }
                    _ => Task::none(),
                }
            }

            Message::DocTabLayoutsSettled(idx, gen) => {
                if gen == self.doc_tab_hover_gen
                    && self.hovered_doc_tab == Some(idx)
                    && !self.doc_tab_menu
                {
                    self.doc_tab_layouts = Some(idx);
                }
                Task::none()
            }

            Message::DocTabLayoutsClose => {
                self.doc_tab_layouts = None;
                Task::none()
            }

            Message::DocTabLayout(idx, layout) => {
                self.doc_tab_layouts = None;
                if idx >= self.tabs.len() {
                    return Task::none();
                }
                let switch = if idx == self.active_tab {
                    Task::none()
                } else {
                    self.update(Message::TabSwitch(idx))
                };
                // The switch can be refused (a recovery prompt is open); the
                // layout belongs to that tab, not to the one still active.
                if self.active_tab != idx {
                    return switch;
                }
                let layout = if self.tabs[self.active_tab].scene.current_layout == layout {
                    Task::none()
                } else {
                    self.update(Message::LayoutSwitch(layout))
                };
                Task::batch([switch, layout])
            }

            Message::DocTabsAboveDocks(on) => {
                self.doc_tab_menu = false;
                self.file_tabs_above_docks = on;
                self.save_config();
                Task::none()
            }

            Message::DocTabMenu(open, pick) => {
                self.doc_tab_menu = open;
                self.doc_tab_layouts = None;
                match pick {
                    Some(idx) if idx < self.tabs.len() && idx != self.active_tab => {
                        self.update(Message::TabSwitch(idx))
                    }
                    _ => Task::none(),
                }
            }

            Message::TabReorder { from, to, after } => {
                // The layouts list belongs to a tab index the move shifts.
                self.doc_tab_layouts = None;
                let Some(insertion) = reorder_insertion_index(from, to, after, self.tabs.len())
                else {
                    return Task::none();
                };
                if self.tabs.get(from).is_some_and(|tab| tab.is_start)
                    || self.tabs.get(to).is_some_and(|tab| tab.is_start)
                {
                    return Task::none();
                }

                let active_id = self.tabs[self.active_tab].id;
                let moved = self.tabs.remove(from);
                self.tabs.insert(insertion, moved);
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == active_id) {
                    self.active_tab = index;
                }
                self.hovered_doc_tab = None;
                Task::none()
            }

            // API v11: keep the active custom-grip drag's coalesced move
            // channel flowing without ever blocking the app loop on plugin
            // latency.
            #[cfg(not(target_arch = "wasm32"))]
            Message::PluginGripPump => self.pump_plugin_grips(),

            Message::TabClose(tab_id) => {
                if self
                    .table_cell_editor
                    .as_ref()
                    .is_some_and(|cell| cell.tab_id == tab_id)
                {
                    self.close_table_cell_editor();
                }
                self.hovered_doc_tab = None;
                self.doc_tab_layouts = None;
                self.release_pointer_navigation();
                // Resolve the stable id when the message is handled, not when
                // it was built: an earlier close in the same batch may have
                // shifted or removed the tab a captured index referred to.
                match self.tabs.iter().position(|tab| tab.id == tab_id) {
                    Some(idx) => self.on_tab_close(idx),
                    // Already closed by an earlier message — nothing to do.
                    None => Task::none(),
                }
            }

            Message::DocTabSaveAll => self.dispatch_command("SAVEALL"),
            Message::RetryWindowView(window_id) => {
                self.failed_views
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .retain(|failed| *failed != window_id);
                Task::none()
            }

            // Both close-* variants resolve ids against the live tab list at
            // handling time (like `TabClose` above), so a batch that closed
            // tabs earlier cannot leave them stale.
            Message::DocTabCloseAll => {
                let ids = self
                    .tabs
                    .iter()
                    .filter(|tab| !tab.is_start)
                    .map(|tab| tab.id)
                    .collect();
                self.begin_tab_close_queue(ids)
            }

            Message::DocTabCloseOthers(idx) => {
                let Some(keep_id) = self.tabs.get(idx).filter(|tab| !tab.is_start).map(|t| t.id)
                else {
                    return Task::none();
                };
                let switch = self.update(Message::TabSwitch(idx));
                let ids = self
                    .tabs
                    .iter()
                    .filter(|tab| !tab.is_start && tab.id != keep_id)
                    .map(|tab| tab.id)
                    .collect();
                Task::batch([switch, self.begin_tab_close_queue(ids)])
            }

            Message::DocTabCopyFullPath(idx) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let Some(path) = self.tabs.get(idx).and_then(|tab| tab.current_path.clone())
                    else {
                        self.command_line.push_error(
                            crate::t!("Save the drawing before copying its file path.").as_ref(),
                        );
                        return Task::none();
                    };
                    let full_path = path.canonicalize().unwrap_or_else(|_| {
                        if path.is_absolute() {
                            path
                        } else {
                            std::env::current_dir()
                                .map(|dir| dir.join(&path))
                                .unwrap_or(path)
                        }
                    });
                    self.command_line
                        .push_output(crate::tf!("Copied path: {}", full_path.display()).as_ref());
                    iced::clipboard::write(full_path.to_string_lossy().into_owned()).discard()
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = idx;
                    self.command_line.push_error(
                        crate::t!("Full file paths are unavailable in the web application.")
                            .as_ref(),
                    );
                    Task::none()
                }
            }

            Message::DocTabOpenFileLocation(idx) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let Some(path) = self.tabs.get(idx).and_then(|tab| tab.current_path.clone())
                    else {
                        self.command_line.push_error(
                            crate::t!("Save the drawing before opening its file location.")
                                .as_ref(),
                        );
                        return Task::none();
                    };
                    match crate::sys::reveal_in_file_manager(&path) {
                        Ok(()) => self.command_line.push_output(
                            crate::tf!("Opened file location: {}", path.display()).as_ref(),
                        ),
                        Err(error) => self.command_line.push_error(
                            crate::tf!("Could not open file location: {error}").as_ref(),
                        ),
                    }
                    Task::none()
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = idx;
                    self.command_line.push_error(
                        crate::t!("File locations are unavailable in the web application.")
                            .as_ref(),
                    );
                    Task::none()
                }
            }

            Message::CommandInput(s) => {
                // Space submits (acts like Enter) so a command advances
                // token-by-token, matching CAD convention. A leading `>` switches
                // to literal-space mode so an argument containing spaces (a text
                // string, a path, `UCS Z 90` as one line) can be typed; the `>`
                // is stripped on submit. (Unfocused Space repeats the last
                // command via CommandSpace.)
                // Preserve command-entry input exactly. Active single-token
                // prompt keywords keep their established uppercase behavior;
                // `CommandLine::submit` uppercases only the command verb.
                let text_with_spaces = self.is_free_text_active();
                // Pasted rows (a column of points copied from a spreadsheet)
                // are entries one after another, and a tab separates X from
                // Y: the same as spaces and commas typed here.
                let s = if text_with_spaces || !s.contains(['\r', '\n', '\t']) {
                    s
                } else {
                    s.replace("\r\n", " ")
                        .replace(['\r', '\n'], " ")
                        .replace('\t', ",")
                };
                let active_prompt = self.tabs[self.active_tab].active_cmd.is_some();
                let literal = self.command_line.literal_spaces || s.starts_with('>');
                let s = if active_prompt && !text_with_spaces && !literal {
                    s.to_uppercase()
                } else {
                    s
                };
                // A space submits (acts like Enter). The whole value is handed to
                // the submit path, which tokenises multi-token lines — so a typed
                // token, a pasted `LINE 0,0 10,10`, or API-fed text all run their
                // spaces as step separators. A leading `>` keeps spaces literal,
                // as does an active free-form text prompt.
                let sweep = self.sync_active_field_if_any();
                if !text_with_spaces
                    && !self.command_line.literal_spaces
                    && !s.starts_with('>')
                    && s.contains(' ')
                {
                    self.command_line.input = s;
                    return Task::batch(vec![sweep, self.on_command_submit()]);
                }
                let live_input = s.clone();
                self.command_line.input = live_input.clone();
                let i = self.active_tab;
                if self.dyn_input
                    && self.tabs[i]
                        .active_grip
                        .as_ref()
                        .is_some_and(|grip| grip.mode.uses_scalar_dynamic_input())
                    && !self.tabs[i].dyn_fields.is_empty()
                {
                    let a = self.tabs[i]
                        .dyn_active
                        .min(self.tabs[i].dyn_fields.len() - 1);
                    self.tabs[i].dyn_fields[a].buffer =
                        (!live_input.is_empty()).then_some(live_input.clone());
                }
                self.command_line.autocomplete_cursor = None;
                self.command_line.cancel_history_navigation();
                // Live incremental search for INSERT/MINSERT: update picker on each keystroke
                // without requiring Enter. Performance-first: uses upper/lower caches
                // and partial sort (O(k)) so per-keystroke is <0.2ms even for 10k blocks.
                // Suggestion count is in direct relation to CLIPROMPTLINES (picker limit).
                {
                    let i = self.active_tab;
                    // Capture prompt/options while holding cmd borrow, then release before
                    // borrowing command_line to satisfy borrow checker.
                    let (should_update, opts, prompt, suggestions) =
                        if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                            let changed = cmd.on_live_input(&live_input);
                            let suggestions = cmd.text_suggestions(&live_input);
                            if changed {
                                (true, cmd.options(), cmd.prompt(), suggestions)
                            } else {
                                (false, Vec::new(), String::new(), suggestions)
                            }
                        } else {
                            (false, Vec::new(), String::new(), Vec::new())
                        };

                    self.command_line.set_contextual_suggestions(suggestions);
                    self.refresh_content_suggestions();

                    if should_update {
                        self.command_line.set_step_options(opts);
                        // Update pinned prompt text live without pushing new history entry
                        // (avoids flooding history with one entry per keystroke).
                        if let Some(last) = self.command_line.history.last_mut() {
                            if last.pinned {
                                last.text = prompt;
                            }
                        }
                    }
                }
                Task::batch(vec![sweep, Task::none()])
            }

            Message::CommandAppendChar(s) => self.on_command_append_char(s),
            Message::HeldOverrideRelease => {
                self.release_held_override();
                Task::none()
            }

            Message::CommandBackspace => self.on_command_backspace(),

            Message::DynTabNext if self.grip_popup.is_some() => {
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = (popup.selected + 1) % popup.items.len();
                    }
                }
                Task::none()
            }

            Message::DynTabNext => {
                if self.table_cell_editor.is_some() {
                    return self.table_editor_step(0, 1, true);
                }
                if let Some(task) = self.table_cell_tab(true) {
                    return task;
                }
                if self.command_line.has_contextual_suggestions()
                    && self.command_line.autocomplete_next()
                {
                    return Task::none();
                }
                let (cycled, moved) = self.cycle_object_snap();
                if cycled {
                    return Task::batch([moved, self.focus_cmd_input()]);
                }

                let i = self.active_tab;
                let n = self.tabs[i].dyn_fields.len();
                if n > 0 {
                    self.tabs[i].dyn_active = (self.tabs[i].dyn_active + 1) % n;
                    // TAB locks the value just typed — reshape the rubber-band
                    // to the constrained point now (#356).
                    self.refresh_active_cmd_preview(i);
                }
                Task::batch([moved, self.focus_cmd_input()])
            }

            Message::DynTabPrev => {
                if self.table_cell_editor.is_some() {
                    return self.table_editor_step(0, -1, true);
                }
                if let Some(task) = self.table_cell_tab(false) {
                    return task;
                }
                if self.command_line.has_contextual_suggestions() {
                    self.command_line.autocomplete_prev();
                    return self.focus_cmd_input();
                }
                let moved = self.cycle_object_snap_back();
                Task::batch([moved, self.focus_cmd_input()])
            }

            Message::SplitModelViewport(horizontal) => {
                let i = self.active_tab;
                self.tabs[i].scene.split_active_pane(horizontal);
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::CloseModelViewport => {
                let i = self.active_tab;
                self.tabs[i].scene.close_active_pane();
                self.tabs[i].scene.camera_generation += 1;
                self.sync_render_mode_to_active_tile(i);
                self.adopt_view_display(i);
                Task::none()
            }

            Message::CommandHistoryPrev => {
                if self.tabs[self.active_tab]
                    .properties
                    .hatch_pattern_picker_open
                    || self.graphic_attributes.hatch_editor.is_some()
                {
                    return self.update(Message::PropHatchPatternNavigate(-2));
                }
                // Grip popup wins first — arrow keys walk its items.
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = if popup.selected == 0 {
                            popup.items.len() - 1
                        } else {
                            popup.selected - 1
                        };
                    }
                    return Task::none();
                }
                let i = self.active_tab;
                if !self.command_line.history_navigation_active()
                    && !self.box_corner_prompt_pending()
                    && (self.tabs[i].active_cmd.is_none()
                        || self.command_line.has_contextual_suggestions())
                    && self.command_line.autocomplete_prev()
                {
                    return Task::none();
                }
                self.command_line.history_prev();
                iced::widget::operation::move_cursor_to_end(iced::widget::Id::new(
                    crate::ui::command_line::CMD_INPUT_ID,
                ))
            }

            Message::CommandHistoryNext => {
                if self.tabs[self.active_tab]
                    .properties
                    .hatch_pattern_picker_open
                    || self.graphic_attributes.hatch_editor.is_some()
                {
                    return self.update(Message::PropHatchPatternNavigate(2));
                }
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = (popup.selected + 1) % popup.items.len();
                    }
                    return Task::none();
                }
                let i = self.active_tab;
                if !self.command_line.history_navigation_active()
                    && !self.box_corner_prompt_pending()
                    && (self.tabs[i].active_cmd.is_none()
                        || self.command_line.has_contextual_suggestions())
                    && self.command_line.autocomplete_next()
                {
                    return Task::none();
                }
                self.command_line.history_next();
                iced::widget::operation::move_cursor_to_end(iced::widget::Id::new(
                    crate::ui::command_line::CMD_INPUT_ID,
                ))
            }

            Message::CommandLineSpaceProbe => iced::widget::operation::is_focused(
                iced::widget::Id::new(crate::ui::command_line::CMD_INPUT_ID),
            )
            .map(|focused| {
                if focused {
                    Message::CommandLineSpaceResolved
                } else {
                    Message::Noop
                }
            }),

            Message::CommandLineSpaceResolved => {
                if self.command_line.input.ends_with(' ') {
                    self.command_line.input.pop();
                }
                self.update(Message::ShortcutPressed("SPACE".to_string()))
            }

            Message::CommandLineArrowProbe {
                direction,
                extend_selection,
            } => iced::widget::operation::is_focused(iced::widget::Id::new(
                crate::ui::command_line::CMD_INPUT_ID,
            ))
            .map(move |focused| Message::CommandLineArrowResolved {
                direction,
                focused,
                extend_selection,
            }),

            Message::CommandLineArrowResolved {
                direction,
                focused,
                extend_selection,
            } => {
                if !focused {
                    // Not the command line: the walk list keeps the arrows while
                    // a row has the selection, so ↑/↓ step through layers.
                    if self.layer_walk_owns_arrows() {
                        match direction {
                            ArrowKey::Up => {
                                return self.on_layer_walk(
                                    crate::ui::window::layer_walk::LayerWalkMsg::KeyNav {
                                        up: true,
                                    },
                                )
                            }
                            ArrowKey::Down => {
                                return self.on_layer_walk(
                                    crate::ui::window::layer_walk::LayerWalkMsg::KeyNav {
                                        up: false,
                                    },
                                )
                            }
                            ArrowKey::Left | ArrowKey::Right => {}
                        }
                    }
                    // …or maybe the QuickCalc input, whose ↑/↓ step through
                    // its history.
                    let up = matches!(direction, ArrowKey::Up);
                    if !self.quick_calc.show || !matches!(direction, ArrowKey::Up | ArrowKey::Down)
                    {
                        return Task::none();
                    }
                    return iced::widget::operation::is_focused(iced::widget::Id::new(
                        crate::ui::window::quick_calc::INPUT_ID,
                    ))
                    .then(move |focused| {
                        if focused {
                            Task::done(Message::QuickCalc(
                                crate::ui::window::quick_calc::QuickCalcMsg::HistoryStep(up),
                            ))
                        } else {
                            Task::none()
                        }
                    });
                }
                // The editor inherits arrows captured by the focused command line.
                if self.mtext_editor.is_some() {
                    match direction {
                        ArrowKey::Up => self.mtext_caret_move_vertical(1, extend_selection),
                        ArrowKey::Down => self.mtext_caret_move_vertical(-1, extend_selection),
                        ArrowKey::Left | ArrowKey::Right => {}
                    }
                    return Task::none();
                }
                match direction {
                    ArrowKey::Up => self.update(Message::CommandHistoryPrev),
                    ArrowKey::Down => self.update(Message::CommandHistoryNext),
                    ArrowKey::Left | ArrowKey::Right => Task::none(),
                }
            }

            Message::ArrowKeyPressed {
                direction,
                shortcut,
                extend_selection,
            } => {
                // ↑/↓ step through the command search's results.
                if self.ribbon.search_results_open()
                    && matches!(direction, ArrowKey::Up | ArrowKey::Down)
                {
                    let down = matches!(direction, ArrowKey::Down);
                    let step = crate::ui::ribbon::SearchMsg::Move(down);
                    return self.update(Message::RibbonSearch(step));
                }
                if self.table_cell_editor.is_some() && !extend_selection {
                    let ed = self.mtext_editor.as_ref().unwrap();
                    match direction {
                        ArrowKey::Up => return self.table_editor_step(-1, 0, false),
                        ArrowKey::Down => return self.table_editor_step(1, 0, false),
                        ArrowKey::Left if ed.caret == 0 => {
                            return self.table_editor_step(0, -1, false)
                        }
                        ArrowKey::Right if ed.caret == self.mtext_vis_count() => {
                            return self.table_editor_step(0, 1, false)
                        }
                        _ => {}
                    }
                }
                if self.mtext_editor.is_some() {
                    match direction {
                        ArrowKey::Left => self.mtext_caret_move(-1, extend_selection),
                        ArrowKey::Right => self.mtext_caret_move(1, extend_selection),
                        ArrowKey::Up => self.mtext_caret_move_vertical(1, extend_selection),
                        ArrowKey::Down => self.mtext_caret_move_vertical(-1, extend_selection),
                    }
                    Task::none()
                } else {
                    match direction {
                        ArrowKey::Up => self.update(Message::CommandHistoryPrev),
                        ArrowKey::Down => self.update(Message::CommandHistoryNext),
                        ArrowKey::Left | ArrowKey::Right => self.run_shortcut(&shortcut),
                    }
                }
            }

            Message::CommandLiteralToggle => {
                self.command_line.literal_spaces = !self.command_line.literal_spaces;
                self.save_config();
                self.focus_cmd_input()
            }

            Message::CommandHistoryHover(hovered) => {
                self.command_line.set_history_hover(hovered);
                Task::none()
            }

            Message::CommandHistoryToggle => {
                self.command_line.toggle_history();
                if !self.command_line.history_open {
                    self.command_history_resizing = false;
                    self.command_history_drag_last = None;
                }
                self.sync_open_command_history();
                Task::none()
            }

            Message::CommandLineMoveGrab => {
                self.command_line_moving = true;
                self.command_history_drag_last = None;
                Task::none()
            }

            Message::CommandLineMoveMove(point) => {
                if self.command_line_moving {
                    if let Some(last) = self.command_history_drag_last {
                        self.command_line.float_offset =
                            self.command_line.float_offset + (point - last);
                    }
                    self.command_history_drag_last = Some(point);
                }
                Task::none()
            }

            Message::CommandLineMoveRelease => {
                self.command_line_moving = false;
                self.command_history_drag_last = None;
                Task::none()
            }

            Message::CommandLineDock => {
                self.command_line_moving = false;
                self.command_line.float_offset = iced::Vector::ZERO;
                Task::none()
            }

            Message::CommandHistoryResizeGrab => {
                if self.command_line.history_open {
                    self.command_history_resizing = true;
                    self.command_history_drag_last = None;
                }
                Task::none()
            }

            Message::CommandHistoryResizeMove(point) => {
                if self.command_history_resizing {
                    if let Some(last) = self.command_history_drag_last {
                        let dy = point.y - last.y;
                        let max_height =
                            crate::ui::command_line::history_max_height(self.win_size.1);
                        self.command_line.history_height = (self.command_line.history_height - dy)
                            .clamp(crate::ui::command_line::HISTORY_HEIGHT_MIN, max_height);
                    }
                    self.command_history_drag_last = Some(point);
                }
                Task::none()
            }

            Message::CommandHistoryResizeRelease => {
                let changed = self.command_history_resizing;
                self.command_history_resizing = false;
                self.command_history_drag_last = None;
                if changed {
                    self.save_config();
                }
                Task::none()
            }

            Message::CommandHistoryHeightReset => {
                self.command_line.history_height = crate::ui::command_line::HISTORY_HEIGHT_DEFAULT;
                self.save_config();
                Task::none()
            }

            Message::CommandHistoryCopy => {
                let text = self.command_line.history_plain_text();
                if text.is_empty() {
                    return Task::none();
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let promise = crate::sys::copy_history_text(
                        &text,
                        crate::t!("Clipboard access is unavailable. Press Ctrl+C or Command+C to copy the selected history.").as_ref(),
                        crate::t!("Close").as_ref(),
                    );
                    Task::perform(
                        async move {
                            wasm_bindgen_futures::JsFuture::from(promise)
                                .await
                                .ok()
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false)
                        },
                        Message::CommandHistoryCopied,
                    )
                }
                #[cfg(not(target_arch = "wasm32"))]
                iced::clipboard::write(text).discard()
            }

            #[cfg(target_arch = "wasm32")]
            Message::CommandHistoryCopied(copied) => {
                if copied {
                    self.command_line.push_info(crate::t!("Copied").as_ref());
                } else {
                    self.command_line.push_error(crate::t!("Clipboard access is unavailable. Press Ctrl+C or Command+C to copy the selected history.").as_ref());
                }
                Task::none()
            }

            Message::CommandHistoryClear => {
                self.command_line.clear_history();
                self.history_content = iced::widget::text_editor::Content::new();
                Task::none()
            }

            Message::PerfCopy => {
                let text = crate::perf::snapshot_text();
                if text.is_empty() {
                    Task::none()
                } else {
                    crate::sys::clipboard_write(text)
                }
            }

            Message::PerfClear => {
                crate::perf::clear();
                Task::none()
            }

            Message::CommandHistoryEdit(action) => {
                // Read-only: drop edits, keep selection/cursor actions, and
                // route wheel input to the outer scrollbar. The text editor
                // remains one selectable buffer while the scrollbar stays
                // visible and draggable.
                if let iced::widget::text_editor::Action::Scroll { lines } = action {
                    iced::widget::operation::scroll_by(
                        iced::widget::Id::new(crate::ui::command_line::HISTORY_SCROLL_ID),
                        iced::widget::scrollable::AbsoluteOffset {
                            x: 0.0,
                            y: lines as f32 * 14.0,
                        },
                    )
                } else {
                    if !action.is_edit() {
                        self.history_content.perform(action);
                    }
                    Task::none()
                }
            }

            Message::CommandSuggestionPick(value) => {
                self.command_line.close_history();

                if self.tabs[self.active_tab].active_cmd.is_some()
                    && self.command_line.has_contextual_suggestions()
                {
                    self.command_line.input = value;
                    self.command_line.autocomplete_cursor = None;
                    return self.on_command_submit();
                }

                self.command_line.input.clear();
                self.command_line.autocomplete_cursor = None;
                self.dispatch_command(&value)
            }

            Message::ContentSuggestionPick(item) => {
                self.command_line.close_history();
                self.run_content_suggestion(item)
            }

            Message::CommandOptionPick(kw) => {
                // Clicking an option button feeds its keyword to the active
                // command through the same path as typed text; an empty keyword
                // finishes the step like Enter. (#304)
                self.command_line.input.clear();
                self.command_line.close_history();
                if self.awaiting_fill_close_answer() {
                    return self.on_graphic_fill_close_input(&kw);
                }
                if kw.is_empty() {
                    return self.feed_command(crate::command::StepInput::Enter);
                }
                // A path picked at a clicked box starts from the clicked
                // point, as the typed word does.
                if self.box_corner_prompt_pending() {
                    self.command_line.input = kw.clone();
                    if let Some(task) = self.try_box_path_keyword() {
                        return task;
                    }
                    self.command_line.input.clear();
                }
                self.feed_active_cmd(&kw)
            }

            Message::CommandSubmit => self.on_command_submit(),

            Message::CommandSpace => {
                // What Space means is decided in one place: the MText preview
                // and free-form text prompts take it as a literal character,
                // everything else finalises the active command like Enter.
                match self.text_entry_mode() {
                    TextEntryMode::MTextPreview => {
                        self.mtext_type(" ");
                        Task::none()
                    }
                    // A free-form text prompt (table cell content, TEXT /
                    // MTEXT bodies) keeps Space in the buffer as a literal
                    // character.
                    TextEntryMode::FreeText => {
                        self.command_line.input.push(' ');
                        Task::none()
                    }
                    TextEntryMode::Command => {
                        // A leading `>` (or the persistent `>` toggle) puts
                        // the command line in "literal space" mode so the user
                        // can type arguments that contain spaces. The typed
                        // `>` is stripped on submit.
                        if self.command_line.literal_spaces
                            || self.command_line.input.starts_with('>')
                        {
                            self.command_line.input.push(' ');
                            return Task::none();
                        }
                        self.update(Message::CommandFinalize)
                    }
                }
            }
            Message::CommandFinalize => self.on_command_finalize(),

            Message::CommandEscape => {
                let panel = &mut self.tabs[self.active_tab].properties;
                if panel.hatch_pattern_picker_open {
                    panel.hatch_pattern_picker_open = false;
                    panel.hatch_pattern_search.clear();
                    panel.hatch_pattern_focus = 0;
                    Task::none()
                } else {
                    self.on_command_escape()
                }
            }

            Message::Command(cmd) => {
                // A command picked from the application menu closes it.
                self.ribbon.close_app_menu();
                // Close viewport context menu if open.
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().menu.close();
                // Any command also dismisses the Isolate action menu.
                self.isolate_popup_open = false;
                // "Pick window" (PLOTWINDOW) from Page Setup needs the backdrop
                // gone so the viewport pick lands; every other command leaves an
                // open modal (and its staged edits) untouched.
                if cmd.trim().eq_ignore_ascii_case("PLOTWINDOW")
                    || cmd.trim().eq_ignore_ascii_case("PW")
                {
                    self.close_active_modal();
                }
                self.dispatch_command(&cmd)
            }

            Message::ScriptLine(line) => self.feed_script_line(&line),
            Message::ScriptPump => self.pump_script(),

            // Web SCRIPT: the picked .scr file arrived — run it through the
            // same runner the native path feeds.
            #[cfg(target_arch = "wasm32")]
            Message::ScriptFilePicked(Some((name, text))) => self.run_script(&name, &text),
            #[cfg(target_arch = "wasm32")]
            Message::ScriptFilePicked(None) => Task::none(),

            // Web DATALINK: the picked .csv's bytes arrived — register them
            // as the link's in-session source, then run the same table-insert
            // path the native typed-path command feeds.
            #[cfg(target_arch = "wasm32")]
            Message::DataLinkCsvPicked(picked) => self.on_datalink_csv_picked(picked),
            Message::DataLinkCsvRead(read, result) => self.finish_data_link_csv_read(&read, result),

            // Web CENTERLTYPEFILE: the picked .lin/.dnl file's text arrived —
            // load its linetypes, then record the setting under the file's
            // name (the native command reads the typed path inside
            // `set_centerline_setting` instead).
            #[cfg(target_arch = "wasm32")]
            Message::CenterlineLinetypeFilePicked(Some((name, source))) => {
                let i = self.active_tab;
                crate::io::linetypes::populate_document_from_source(
                    &mut self.tabs[i].scene.document,
                    &source,
                    None,
                );
                self.tabs[i]
                    .scene
                    .set_centerline_setting("CENTERLTYPEFILE", &name);
                self.tabs[i].dirty = true;
                Task::none()
            }
            #[cfg(target_arch = "wasm32")]
            Message::CenterlineLinetypeFilePicked(None) => Task::none(),

            Message::ToggleLayers => {
                if self.show_layers {
                    // In a window of its own it may be behind the main one,
                    // the only place this can be pressed from: bring it up.
                    if let Some(raise) = self.raise_pallet_window(crate::ui::dock::PanelId::Layers)
                    {
                        return raise;
                    }
                    return self.on_dock(crate::ui::dock::DockMsg::Close(
                        crate::ui::dock::PanelId::Layers,
                    ));
                }
                self.pallet_as_window(crate::ui::dock::PanelId::Layers);
                self.open_layers_pallet();
                Task::none()
            }

            Message::ToggleXrefManager => {
                if !self.show_external_references {
                    self.pallet_as_window(crate::ui::dock::PanelId::ExternalReferences);
                }
                self.show_external_references ^= true;
                if self.show_external_references {
                    self.dock_open_at_default(crate::ui::dock::PanelId::ExternalReferences);
                    self.refresh_xref_manager();
                }
                Task::none()
            }
            Message::XrefManagerRefresh => {
                self.refresh_xref_manager();
                Task::none()
            }
            Message::XrefManagerSelect(id) => {
                use crate::ui::window::xref_manager::SelectExtend;
                // Normal GUI list behavior off the globally tracked
                // modifiers (same source as the layer list): plain click
                // selects one row, Ctrl/Cmd toggles, Shift extends a range.
                let extend = if self.ctrl_down {
                    SelectExtend::Toggle
                } else if self.shift_down {
                    SelectExtend::Range
                } else {
                    SelectExtend::Single
                };
                self.xref_manager.click_select(&id, extend);
                self.xref_manager.keys_active = true;
                Task::none()
            }
            Message::XrefRowRightClick(id) => {
                self.xref_manager.row_change_path_open = false;
                self.xref_manager.right_click_select(&id);
                self.xref_manager.keys_active = true;
                Task::none()
            }
            Message::XrefManagerToggleTree => {
                self.xref_manager.toggle_tree();
                Task::none()
            }
            Message::XrefColGrab(i) => {
                // Start a table-column divider drag; moves arrive via the
                // header's mouse_area (XrefColMove), like the Layers
                // Name-column divider rides ModalDragMove.
                self.xref_col_drag = Some(i);
                self.xref_col_last = None;
                Task::none()
            }
            Message::XrefColMove(p) => {
                if let Some(i) = self.xref_col_drag {
                    if let Some(last) = self.xref_col_last {
                        self.xref_manager.drag_col_by(i, p.x - last.x);
                    }
                    self.xref_col_last = Some(p);
                }
                Task::none()
            }
            Message::XrefColRelease => {
                self.xref_col_drag = None;
                self.xref_col_last = None;
                Task::none()
            }
            Message::XrefSplitGrab => {
                self.xref_split_drag = true;
                self.xref_col_last = None;
                Task::none()
            }
            Message::XrefSplitMove(p) => {
                if self.xref_split_drag {
                    if let Some(last) = self.xref_col_last {
                        self.xref_manager.drag_table_by(p.y - last.y);
                    }
                    self.xref_col_last = Some(p);
                }
                Task::none()
            }
            Message::XrefSplitRelease => {
                self.xref_split_drag = false;
                self.xref_col_last = None;
                Task::none()
            }
            Message::XrefManagerTogglePreview => {
                self.xref_manager.show_preview ^= true;
                Task::none()
            }
            Message::XrefPreviewRead(generation, key, preview) => {
                self.xref_manager.preview_read(generation, key, preview);
                Task::none()
            }
            Message::XrefManagerAttachMenu => {
                self.xref_manager.attach_open ^= true;
                self.xref_manager.refresh_open = false;
                self.xref_manager.path_open = false;
                Task::none()
            }
            Message::XrefManagerRefreshMenu => {
                self.xref_manager.refresh_open ^= true;
                self.xref_manager.attach_open = false;
                self.xref_manager.path_open = false;
                Task::none()
            }
            Message::XrefManagerPathMenu => {
                self.xref_manager.path_open ^= true;
                self.xref_manager.attach_open = false;
                self.xref_manager.refresh_open = false;
                Task::none()
            }
            Message::XrefHelpOpen => {
                self.active_modal = Some(super::super::super::ModalKind::XrefHelp);
                Task::none()
            }
            Message::XrefManagerDismissMenus => {
                self.xref_manager.attach_open = false;
                self.xref_manager.refresh_open = false;
                self.xref_manager.path_open = false;
                self.xref_manager.row_change_path_open = false;
                Task::none()
            }
            Message::XrefPathPick => Task::perform(
                async {
                    let handle = crate::sys::file_dialog()
                        .set_title(crate::t!("Select New Path").as_ref())
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(crate::sys::handle_path(&h)),
                        None => Err("Cancelled".to_string()),
                    }
                },
                Message::XrefPathPickResult,
            ),
            Message::XrefPathPickResult(Ok(path)) => {
                // Select New Path applies the picked file to the single
                // direct anchor entry (menu gates the rest).
                let i = self.active_tab;
                let anchor = self.xref_manager.anchor.and_then(|a| {
                    self.xref_manager
                        .entries
                        .get(a)
                        .filter(|_| !self.xref_manager.nested.contains(&a))
                        .map(|e| (e.key, e.name.clone()))
                });
                let Some((key, _)) = anchor else {
                    return Task::none();
                };
                let new_raw = path.to_string_lossy().into_owned();
                self.push_undo_snapshot(i, "XREF-PATH");
                match crate::io::xref::set_ref_path(&mut self.tabs[i].scene.document, key, &new_raw)
                {
                    Ok(name) => {
                        self.command_line.push_output(
                            crate::tf!("XREF: Path set for \"{}\" — Reload to apply.", name)
                                .as_ref(),
                        );
                        self.post_ref_op(i);
                    }
                    Err(msg) => self.command_line.push_error(msg.as_str()),
                }
                self.refresh_xref_manager();
                Task::none()
            }
            Message::XrefPathPickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line
                        .push_error(crate::tf!("XREF: {e}").as_ref());
                }
                Task::none()
            }
            Message::XrefManagerReloadAll => {
                self.xref_manager_reload_all();
                Task::none()
            }
            Message::XrefManagerToggleExpand(key) => {
                self.xref_manager.toggle_expand(key);
                Task::none()
            }
            Message::XrefManagerOp(op) => {
                self.xref_manager_op(op);
                Task::none()
            }
            Message::XrefRowOp(id, op) => {
                // Open is not a palette op — it navigates to the file.
                if op == crate::ui::window::xref_manager::XrefPaletteOp::Open {
                    self.xref_manager.right_click_select(&id);
                    self.xref_manager.row_change_path_open = false;
                    if let Some(index) = self.xref_manager.row_index(&id) {
                        if let Some(entry) = self.xref_manager.entries.get(index) {
                            if let Some(found) = entry.found_at.clone() {
                                let is_dwg = found.to_ascii_lowercase().ends_with(".dwg")
                                    || found.to_ascii_lowercase().ends_with(".dxf");
                                self.command_line.push_output(
                                    crate::tf!("XOPEN: opening \"{}\".", found).as_ref(),
                                );
                                if is_dwg {
                                    return self.update(Message::OpenRecent(
                                        std::path::PathBuf::from(found),
                                    ));
                                } else {
                                    #[cfg(not(target_arch = "wasm32"))]
                                    let _ = open::that_detached(&found);
                                }
                            } else {
                                self.command_line.push_error(
                                    crate::t!("File not found — check the saved path.").as_ref(),
                                );
                            }
                        }
                    }
                    return Task::none();
                }
                // Attach prompts the file dialog to pick a drawing to attach.
                if matches!(op, crate::ui::window::xref_manager::XrefPaletteOp::Attach) {
                    self.xref_manager.right_click_select(&id);
                    self.xref_manager.row_change_path_open = false;
                    self.command_line.push_output(crate::t!("XATTACH").as_ref());
                    return self.update(Message::XAttachPick);
                }
                // Row-scoped op: select the row first, then run the op.
                self.xref_manager.right_click_select(&id);
                self.xref_manager.row_change_path_open = false;
                self.xref_manager_op(op);
                Task::none()
            }
            Message::XrefRowPathPick(id) => {
                self.xref_manager.right_click_select(&id);
                self.xref_manager.row_change_path_open = false;
                self.update(Message::XrefPathPick)
            }
            Message::XrefRowFindReplacePrompt(id) => {
                self.xref_manager.right_click_select(&id);
                self.xref_manager.row_change_path_open = false;
                let prefill = if let Some(index) = self.xref_manager.row_index(&id) {
                    self.xref_manager.entries.get(index).map_or_else(
                        || "XREF Path Find ".to_string(),
                        |entry| {
                            let saved = &entry.saved_path;
                            if let Some(parent) = std::path::Path::new(saved)
                                .parent()
                                .and_then(|p| p.to_str())
                            {
                                if !parent.is_empty() {
                                    format!("XREF Path Find \"{}\" ", parent)
                                } else {
                                    "XREF Path Find ".to_string()
                                }
                            } else {
                                "XREF Path Find ".to_string()
                            }
                        },
                    )
                } else {
                    "XREF Path Find ".to_string()
                };
                self.command_line.input = prefill;
                self.command_line.autocomplete_cursor = None;
                self.command_line.cancel_history_navigation();
                self.command_line.push_info(
                    crate::t!("Specify replacement path: XREF Path Find <old> <new>").as_ref(),
                );
                self.focus_cmd_input()
            }
            Message::XrefRowChangePathEnter => {
                self.xref_manager.row_change_path_open = true;
                Task::none()
            }
            Message::XrefRowChangePathLeave => {
                self.xref_manager.row_change_path_open = false;
                Task::none()
            }
            Message::XrefFindReplacePrompt => {
                // No fields in the panel: prefill the command line so the
                // existing `XREF Path Find <old> <new>` parsing runs it.
                self.command_line.input = "XREF Path Find ".to_string();
                self.command_line.autocomplete_cursor = None;
                self.command_line.cancel_history_navigation();
                self.focus_cmd_input()
            }

            Message::LayerStateManagerOpen => {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                if self.tabs[i].is_start {
                    self.command_line.push_info(
                        crate::t!("Open or create a drawing to manage layer states.").as_ref(),
                    );
                    return Task::none();
                }
                let mut names: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .layer_states()
                    .into_iter()
                    .map(|state| state.name)
                    .collect();
                names.sort_by_key(|name| name.to_lowercase());
                self.load_layer_state_editor(names.into_iter().next());
                self.active_modal = Some(super::super::super::ModalKind::LayerStateManager);
                Task::none()
            }
            // ── Layer Translator (#624) ──────────────────────────────────
            Message::LayerTranslatorLoad => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(crate::io::pick_layer_standard_path(), |path| match path {
                        Some(path) => Message::LayerTranslatorLoaded(path),
                        None => Message::Noop,
                    })
                }
                // Web: read the picked file's bytes and parse them on the
                // worker right away — a name-only path has nothing behind it
                // for `load_targets` to read later.
                #[cfg(target_arch = "wasm32")]
                {
                    Task::perform(
                        async {
                            crate::io::pick_and_parse_web(
                                crate::t!("Load layer standard").as_ref(),
                                crate::t!("Drawings and standards").as_ref(),
                                &["dwg", "dws", "dwt", "dxf", "DWG", "DWS", "DWT", "DXF"],
                            )
                            .await
                            .and_then(|picked| {
                                picked.map_or(Ok(None), |(name, document)| {
                                    crate::modules::draw::layers::laytrans::targets_from_document(
                                        &document,
                                    )
                                    .map(|targets| Some((name, targets)))
                                })
                            })
                        },
                        |loaded| Message::LayerTranslatorLoadedWeb(loaded),
                    )
                }
            }
            #[cfg(target_arch = "wasm32")]
            Message::LayerTranslatorLoadedWeb(loaded) => {
                match loaded {
                    Ok(Some((source_file, targets))) => {
                        let state = self.layer_translator.get_or_insert_with(Default::default);
                        state.source_file = source_file;
                        state.targets = targets;
                        // A target set that no longer contains a mapped name
                        // would translate onto nothing.
                        let names: Vec<String> =
                            state.targets.iter().map(|t| t.name.clone()).collect();
                        state
                            .mappings
                            .retain(|m| names.iter().any(|n| n.eq_ignore_ascii_case(&m.to)));
                        state.selected_to = None;
                    }
                    Err(why) => self
                        .command_line
                        .push_error(crate::tf!("LAYTRANS: {why}.").as_ref()),
                    Ok(None) => {}
                }
                Task::none()
            }
            Message::ShortcutsFileRead(path, rows) => {
                self.finish_shortcuts_import(&path, rows);
                Task::none()
            }
            Message::LayerTranslatorLoaded(path) => {
                use crate::app::commands::layer_file_reads::{LayerFileJob, LayerFiles};
                let files = LayerFiles {
                    standards: Some(path),
                    ..LayerFiles::default()
                };
                self.read_layer_files(self.active_tab, LayerFileJob::WindowStandards, files)
            }
            Message::LayerTranslatorSelectFrom(name) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.selected_from = Some(name);
                }
                Task::none()
            }
            Message::LayerTranslatorSelectTo(name) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.selected_to = Some(name);
                }
                Task::none()
            }
            Message::LayerTranslatorMap => {
                use crate::modules::draw::layers::laytrans::Mapping;
                if let Some(state) = self.layer_translator.as_mut() {
                    if let (Some(from), Some(to)) =
                        (state.selected_from.take(), state.selected_to.clone())
                    {
                        state
                            .mappings
                            .retain(|m| !m.from.eq_ignore_ascii_case(&from));
                        state.mappings.push(Mapping { from, to });
                    }
                }
                Task::none()
            }
            Message::LayerTranslatorMapSame => {
                use crate::modules::draw::layers::laytrans;
                let i = self.active_tab;
                let current = self.tabs[i].active_layer.clone();
                let sources = laytrans::source_layers(&self.tabs[i].scene, &current);
                if let Some(state) = self.layer_translator.as_mut() {
                    for mapping in laytrans::map_same(&sources, &state.targets) {
                        if !state
                            .mappings
                            .iter()
                            .any(|m| m.from.eq_ignore_ascii_case(&mapping.from))
                        {
                            state.mappings.push(mapping);
                        }
                    }
                }
                Task::none()
            }
            Message::LayerTranslatorUnmap(from) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.mappings.retain(|m| m.from != from);
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
