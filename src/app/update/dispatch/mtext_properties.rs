use super::super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_mtext_properties(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::MTextSelTo(off) => {
                let off = off.min(self.mtext_vis_count());
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.typing_format = Default::default();
                    let a = ed.sel_anchor;
                    ed.sel = Some((a.min(off), a.max(off)));
                    ed.caret = off;
                    ed.caret_blink_on = true;
                }
                Task::none()
            }
            Message::MTextFieldSubmit => self.unfocus_widgets(),
            Message::MTextSelEnd => {
                self.mtext_match_apply();
                Task::none()
            }
            Message::MTextCaretMove(d) => {
                if self.tabs[self.active_tab]
                    .properties
                    .hatch_pattern_picker_open
                    || self.graphic_attributes.hatch_editor.is_some()
                {
                    return self.update(Message::PropHatchPatternNavigate(d as i8));
                }
                self.mtext_caret_move(d, false);
                Task::none()
            }
            Message::MTextCaretBlink => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.caret_blink_on = !ed.caret_blink_on;
                }
                Task::none()
            }
            Message::MTextOk => {
                let committed = self.mtext_commit();
                // A draft that could not be saved stays open with its message.
                if self.mtext_editor.is_some() {
                    return Task::none();
                }
                self.post_editor_closed(committed)
            }
            Message::MTextApply => {
                self.mtext_apply();
                Task::none()
            }
            Message::MTextCancel => {
                self.mtext_cancel();
                self.post_editor_closed(false)
            }

            Message::TextInlineInput(s) => {
                if let Some(ed) = self.text_inline.as_mut() {
                    ed.value = s;
                }
                Task::none()
            }

            // Ctrl+V. The MText editor and (on the web) the TEXT editor read the
            // system clipboard asynchronously — the only paste path that works
            // in the browser, where the synchronous clipboard the iced
            // text_input expects is empty. With no editor open, drawing objects
            // take priority and system text is used when that clipboard is empty.
            Message::PasteShortcut => self.on_paste_shortcut(),

            // A focused text input pastes text natively: another field keeps
            // Ctrl+V to itself, and the command line must not get the text a
            // second time. Copied objects still paste over the command line.
            // The paste shortcut already places the drawing's own copy; only
            // markup from another program is read back to come in as a picture.
            Message::CommandPasteMarkup => {
                if self.clipboard.is_empty() {
                    self.read_system_clipboard_for_paste()
                } else {
                    Task::none()
                }
            }

            Message::PasteShortcutResolved(focus) => {
                use super::super::super::PasteFocus;
                if focus == PasteFocus::Field {
                    Task::none()
                } else if self.mtext_editor.is_none()
                    && self.tabs[self.active_tab].scene.table_cells.is_some()
                {
                    // Selected cells take the clipboard text; the command
                    // line's own paste of it is undone.
                    if focus == PasteFocus::CommandLine {
                        self.command_line.input.clear();
                    }
                    #[cfg(target_arch = "wasm32")]
                    return Task::perform(
                        crate::sys::read_clipboard_text(),
                        Message::TableCellsPaste,
                    );
                    #[cfg(not(target_arch = "wasm32"))]
                    iced::clipboard::read_text().map(|result| {
                        Message::TableCellsPaste(result.ok().map(|text| (*text).clone()))
                    })
                } else if !self.clipboard.is_empty() {
                    self.update(Message::Command("PASTECLIP".to_string()))
                } else if focus == PasteFocus::CommandLine
                    && !(self.dyn_capturing()
                        && self.text_inline.is_none()
                        && !self.mtext_editor.as_ref().is_some_and(|e| e.show_preview))
                {
                    // The command line pasted it natively. While dynamic input
                    // holds the keystrokes the field is read-only, so the
                    // text is read here and typed in as rows. (#1883)
                    Task::none()
                } else {
                    self.read_system_clipboard_for_paste()
                }
            }

            Message::PasteAsHyperlink(text) => self.paste_as_hyperlink(text),

            Message::PasteSpecial(text, picture, link) => {
                let i = self.active_tab;
                let waiting = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.name() == "PASTESPEC");
                if !waiting {
                    return Task::none();
                }
                let text = text.filter(|text| !text.trim().is_empty());
                let nothing = text.is_none() && picture.is_none() && link.is_none();
                if nothing && self.clipboard.is_empty() {
                    // Nothing to paste in any format.
                    self.tabs[i].active_cmd = None;
                    self.command_line
                        .push_error(crate::tr!("clipboard", "no-supported-content").as_ref());
                    return Task::none();
                }
                // Every format is known now: ask for one of them.
                let command = self.paste_special_command(i, text.as_deref(), link, picture);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(command);
                self.push_ucs_to_cmd(i);
                Task::none()
            }

            Message::SystemClipboardPaste(result) => {
                use super::super::super::SystemClipboardText as Text;

                match result {
                    // SVG markup (from another program, or another OCS)
                    // comes in as drawing, placed by the insertion prompts.
                    Text::Text(text)
                        if text.trim_start().starts_with('<') && text.contains("<svg") =>
                    {
                        self.paste_svg_text(self.active_tab, text)
                    }
                    Text::Text(text) => {
                        // As if typed: rows submit one after another.
                        let typed = format!("{}{text}", self.command_line.input);
                        Task::batch([
                            self.update(Message::CommandInput(typed)),
                            self.focus_cmd_input(),
                        ])
                    }
                    Text::EmptyOrUnsupported => {
                        // No text — the clipboard may still hold an IMAGE (a
                        // pasted screenshot). Read it and attach it the way the
                        // IMAGE command's pick does; only when that also finds
                        // nothing does the unsupported-content error surface.
                        self.read_system_clipboard_image()
                    }
                    Text::Unavailable => {
                        self.command_line
                            .push_error(crate::tr!("clipboard", "unavailable").as_ref());
                        Task::none()
                    }
                    Text::Occupied => {
                        self.command_line
                            .push_error(crate::tr!("clipboard", "occupied").as_ref());
                        Task::none()
                    }
                    Text::ConversionFailed => {
                        self.command_line
                            .push_error(crate::tr!("clipboard", "conversion-failed").as_ref());
                        Task::none()
                    }
                }
            }

            // The clipboard-image fallback of a text-less paste either
            // attaches the image or falls back to the same error the text
            // path reports when nothing readable was on the clipboard.
            Message::SystemClipboardPasteImage(image) => match image {
                Some((name, bytes)) => self.paste_clipboard_image(name, bytes),
                None => {
                    self.command_line
                        .push_error(crate::tr!("clipboard", "no-supported-content").as_ref());
                    Task::none()
                }
            },

            Message::SelectAllShortcut => {
                let i = self.active_tab;
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    // Ctrl+A in the MText editor selects all of its text.
                    self.mtext_select_all();
                    Task::none()
                } else if self.dock_panel_visible(crate::ui::dock::PanelId::Layers)
                    && self.tabs[i].layers.selected.is_some()
                {
                    // Select every row in the Layer Manager (#236) while the
                    // user is working in it (a layer row is selected).
                    let n = self.tabs[i].layers.layers.len();
                    self.tabs[i].layers.selected_multi = (0..n).collect();
                    self.tabs[i].layers.selected = (n > 0).then_some(0);
                    Task::none()
                } else {
                    self.dispatch_command("SELECTALL")
                }
            }
            Message::FindReplaceOpen => self.open_find_replace(),
            Message::Find(message) => self.on_find_message(message),
            Message::OverkillSettingsChanged(settings) => {
                if let Some(state) = &mut self.overkill_dialog {
                    state.settings = settings;
                }
                Task::none()
            }
            Message::OverkillToleranceChanged(value) => {
                if let Some(state) = &mut self.overkill_dialog {
                    state.tolerance = value;
                }
                Task::none()
            }
            Message::OverkillOk => {
                let Some(settings) = self.overkill_dialog.take().and_then(|s| s.parsed()) else {
                    return Task::none();
                };
                self.close_active_modal();
                self.dispatch_command(&settings.apply_command())
            }
            Message::MTextPasteClip(text) => {
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    // CR/LF arrive as line breaks; MText keeps "\n", drop "\r".
                    let text = text.replace('\r', "");
                    match self.mtext_clipboard.clone() {
                        // Copied in this editor: paste it with its formatting.
                        Some((plain, fragment)) if plain.replace('\r', "") == text => {
                            self.raw_paste_fragment(&fragment)
                        }
                        _ => self.mtext_type(&text),
                    }
                    self.rebuild_mtext_preview();
                }
                Task::none()
            }
            Message::TextInlinePasteClip(text) => {
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    // Single-line field: collapse newlines, append at the end.
                    let flat = text.replace(['\r', '\n'], " ");
                    if let Some(ed) = self.text_inline.as_mut() {
                        ed.value.push_str(&flat);
                    }
                }
                Task::none()
            }
            Message::TextInlineOk => {
                let committed = self.text_inline_commit();
                self.post_editor_closed(committed)
            }

            Message::ContextMenuPick(action) => self.on_context_menu_pick(action),
            Message::ContextMenuSubmenuToggle(id) => self.on_context_menu_submenu_toggle(id),
            Message::ContextMenuNavigate(nav) => self.on_context_menu_navigate(nav),

            // Quad cursor menu interactions and display options
            // (docs/quad-menu.md); the glue lives next to the context-menu
            // router it mirrors.
            Message::Quad(quad) => self.on_quad_msg(quad),
            Message::QuadOption(option) => self.on_quad_option(option),

            Message::DrawOrderPickRef(above) => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().menu.close();
                let to_move: Vec<_> = self.tabs[i].scene.selected.iter().cloned().collect();
                if to_move.is_empty() {
                    self.command_line
                        .push_error(crate::t!("DRAWORDER: select entities first.").as_ref());
                } else {
                    use crate::command::CadCommand;
                    let cmd = super::super::super::commands::DrawOrderCommand::for_reference_pick(to_move, above);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
                Task::none()
            }

            Message::SelectSimilar => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().menu.close();
                let added = self.tabs[i].scene.select_similar(self.select_similar_mode);
                self.command_line
                    .push_output(crate::tf!("Select Similar: {} added.", added).as_ref());
                self.refresh_properties();
                Task::none()
            }

            Message::InvertSelection => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().menu.close();
                let count = self.tabs[i].scene.invert_selection();
                self.command_line.push_output(
                    crate::tf!("Invert Selection: {} object(s) selected.", count).as_ref(),
                );
                self.refresh_properties();
                Task::none()
            }

            Message::QSelectOpen => self.on_qselect_open(),

            Message::QSelectAdvanced => self.qselect_advanced(),

            Message::FilterDialog(msg) => self.on_filter_message(msg),

            Message::QSelectClose => {
                self.qselect_clear_preview();
                if self.transparent_select.is_some() {
                    if let Some(state) = self.qselect.take() {
                        self.qselect_settings = Some((&state).into());
                    }
                    self.reset_modal_geometry();
                    return self.finish_transparent_select(None);
                }
                if let Some(state) = self.qselect.take() {
                    self.qselect_settings = Some((&state).into());
                }
                self.reset_modal_geometry();
                self.return_to_block_dialog(false);
                Task::none()
            }

            Message::QSelectSetScope(scope) => {
                let i = self.active_tab;
                let available_types = self.tabs[i].scene.qselect_type_counts(scope);
                let type_filter = self.qselect.as_ref().and_then(|state| {
                    state
                        .type_filter
                        .as_ref()
                        .filter(|selected| {
                            available_types.iter().any(|(item, _)| item == *selected)
                        })
                        .cloned()
                });
                let available_properties = self.tabs[i]
                    .scene
                    .qselect_properties(type_filter.as_deref(), scope);
                let candidate_count = self.tabs[i].scene.qselect_candidate_count(scope);
                if let Some(state) = self.qselect.as_mut() {
                    state.scope = scope;
                    state.available_types = available_types;
                    state.available_properties = available_properties;
                    state.candidate_count = candidate_count;
                    state.type_filter = type_filter;
                    state.property = state.property.as_ref().and_then(|selected| {
                        state
                            .available_properties
                            .iter()
                            .find(|available| available.field == selected.field)
                            .cloned()
                    });
                    state.value.clear();
                    state.error = None;
                    state.normalize_operator();
                }
                self.qselect_refresh_values();
                self.qselect_refresh_live();
                Task::none()
            }

            Message::QSelectSetType(t) => {
                let i = self.active_tab;
                let scope = self
                    .qselect
                    .as_ref()
                    .map_or(crate::app::QSelectScope::CurrentSpace, |state| state.scope);
                let properties = self.tabs[i].scene.qselect_properties(t.as_deref(), scope);
                if let Some(state) = self.qselect.as_mut() {
                    let kept_property = state.property.as_ref().and_then(|selected| {
                        properties
                            .iter()
                            .find(|available| available.field == selected.field)
                            .cloned()
                    });
                    state.type_filter = t;
                    state.available_properties = properties;
                    state.property = kept_property;
                    state.value.clear();
                    state.error = None;
                    state.normalize_operator();
                }
                self.qselect_refresh_values();
                self.qselect_refresh_live();
                Task::none()
            }

            Message::QSelectSetProperty(p) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.property = p;
                    state.value.clear();
                    state.error = None;
                    state.normalize_operator();
                }
                self.qselect_refresh_values();
                self.qselect_refresh_live();
                Task::none()
            }

            Message::QSelectSetOperator(op) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.operator = op;
                    state.error = None;
                }
                self.qselect_refresh_live();
                Task::none()
            }

            Message::QSelectSetValue(v) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.value = v;
                    state.error = None;
                }
                self.qselect_refresh_live();
                Task::none()
            }

            Message::QSelectSetMode(mode) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.mode = mode;
                    state.error = None;
                }
                self.qselect_refresh_live();
                Task::none()
            }

            Message::QSelectSetResult(result) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.result = result;
                }
                Task::none()
            }

            Message::QSelectSetShow(show) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.show = show;
                }
                self.qselect_refresh_live();
                Task::none()
            }

            Message::QSelectSampleToggle(index) => {
                if let Some(row) = self
                    .qselect
                    .as_mut()
                    .and_then(|state| state.sample.as_mut())
                    .and_then(|sample| sample.rows.get_mut(index))
                {
                    row.1 = !row.1;
                }
                Task::none()
            }

            Message::QSelectSampleAdd => {
                self.on_qselect_sample_add();
                Task::none()
            }

            Message::QSelectSampleCancel => {
                if let Some(state) = self.qselect.as_mut() {
                    state.sample = None;
                }
                Task::none()
            }

            Message::QSelectPickObject => self.on_qselect_pick_object(),
            Message::QSelectWindow => self.scope_window_pick(true),
            Message::QSelectAddCondition => {
                if let Some(state) = self.qselect.as_mut() {
                    let condition = state.draft_condition();
                    if let Some(error) = condition.validation_error() {
                        state.error = Some(error);
                        return Task::none();
                    }
                    if let Some(index) = state.editing_condition.take() {
                        if let Some(existing) = state.conditions.get_mut(index) {
                            *existing = condition;
                        }
                    } else {
                        state.conditions.push(condition);
                    }
                    state.error = None;
                }
                self.qselect_refresh_live();
                Task::none()
            }
            Message::QSelectEditCondition(index) => {
                let condition = self
                    .qselect
                    .as_ref()
                    .and_then(|state| state.conditions.get(index).cloned());
                if let Some(condition) = condition {
                    let scope = self.qselect.as_ref().unwrap().scope;
                    let properties = self.tabs[self.active_tab]
                        .scene
                        .qselect_properties(condition.type_filter.as_deref(), scope);
                    if let Some(state) = self.qselect.as_mut() {
                        state.available_properties = properties;
                        state.type_filter = condition.type_filter;
                        state.property = condition.property;
                        state.operator = condition.operator;
                        state.value = condition.value;
                        state.editing_condition = Some(index);
                        state.error = None;
                    }
                    self.qselect_refresh_values();
                }
                Task::none()
            }
            Message::QSelectRemoveCondition(index) => {
                if let Some(state) = self.qselect.as_mut() {
                    if index < state.conditions.len() {
                        state.conditions.remove(index);
                        state.editing_condition = match state.editing_condition {
                            Some(editing) if editing == index => None,
                            Some(editing) if editing > index => Some(editing - 1),
                            other => other,
                        };
                    }
                    state.error = None;
                }
                self.qselect_refresh_live();
                Task::none()
            }
            Message::QSelectClearConditions => {
                if let Some(state) = self.qselect.as_mut() {
                    state.conditions.clear();
                    state.editing_condition = None;
                    state.error = None;
                }
                self.qselect_refresh_live();
                Task::none()
            }
            Message::QSelectCancelEdit => {
                if let Some(state) = self.qselect.as_mut() {
                    state.editing_condition = None;
                    state.error = None;
                }
                Task::none()
            }
            Message::QSelectApply => {
                let validation_error = self.qselect.as_ref().and_then(|state| {
                    let candidate_count = self.tabs[self.active_tab]
                        .scene
                        .qselect_candidate_count(state.scope);
                    state.validation_error(candidate_count)
                });
                if let Some(error) = validation_error {
                    if let Some(state) = self.qselect.as_mut() {
                        state.error = Some(error);
                    }
                    return Task::none();
                }
                self.qselect_clear_preview();
                let Some(state) = self.qselect.take() else {
                    return Task::none();
                };
                self.qselect_settings = Some((&state).into());
                self.reset_modal_geometry();
                let i = self.active_tab;
                let conditions = if state.conditions.is_empty() {
                    vec![state.draft_condition()]
                } else {
                    state.conditions.clone()
                };
                let matched = self.tabs[i].scene.qselect_conditions(
                    state.scope,
                    &conditions,
                    state.mode,
                    state.result,
                );
                if self.transparent_select.is_some() {
                    return self.finish_transparent_select(Some(matched));
                }
                self.command_line
                    .push_output(crate::tf!("QSELECT: {} object(s) selected.", matched).as_ref());
                self.refresh_properties();
                if self.return_to_block_dialog(true) {
                    return Task::none();
                }
                // FILTER may be opened from a still-focused command input.
                // Release text focus so Delete and clipboard shortcuts act
                // on the resulting pick-first selection, not on that input.
                self.unfocus_widgets()
            }

            // ── Properties panel messages ─────────────────────────────────
            Message::PropSelectionGroupChanged(group) => {
                self.tabs[self.active_tab].properties.selected_group = Some(group);
                self.refresh_properties();
                Task::none()
            }

            Message::RibbonLayerChanged(layer) => self.on_ribbon_layer_changed(layer),

            Message::RibbonColorChanged(color) => self.on_ribbon_color_changed(color),
            Message::RibbonLayerColorChanged { layer, color } => {
                self.set_ribbon_layer_color(&layer, color, None)
            }
            Message::RibbonColorPaletteToggle => {
                self.ribbon.prop_color_palette_open ^= true;
                Task::none()
            }
            Message::RibbonLinetypeChanged(lt) => self.on_ribbon_linetype_changed(lt),
            Message::RibbonLineweightChanged(lw) => self.on_ribbon_lineweight_changed(lw),

            Message::RibbonStyleChanged { key, name } => self.on_ribbon_style_changed(key, name),

            Message::PropLayerChanged(layer) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    let task = self.on_ribbon_layer_changed(layer);
                    self.refresh_properties();
                    return task;
                }
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    let document = &mut app.tabs[i].scene.document;
                    let layer_handle = document
                        .layers
                        .get(&layer)
                        .map(|entry| entry.handle)
                        .filter(|handle| !handle.is_null());
                    if let Some(entity) = document.get_entity_mut(handle) {
                        crate::scene::view::dispatch::apply_common_prop(entity, "layer", &layer);
                        if entity.common().layer == layer {
                            entity.common_mut().layer_handle = layer_handle;
                        }
                    }
                });
                Task::none()
            }

            Message::PropColorChanged(color) => self.set_property_color(color, None),

            Message::PropLwChanged(lw) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    let task = self.on_ribbon_lineweight_changed(lw);
                    self.refresh_properties();
                    return task;
                }
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                        crate::scene::view::dispatch::apply_line_weight(entity, lw);
                    }
                });
                Task::none()
            }

            Message::PropFieldLwChanged { field, value } => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if field == "tbl_cell_border_lw" {
                    let cells = self.picked_cell_positions(i);
                    self.apply_property_op(i, "TABLE CELL BORDER", &handles, |app, handle| {
                        use crate::entities::table::TableCellBordersExt;
                        if let Some(document::EntityType::Table(table)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            for &(row, column) in &cells {
                                table.edit_cell_borders(row, column, |border| {
                                    border.line_weight = value;
                                    document::entities::table::BorderPropertyFlags::LINE_WEIGHT
                                });
                            }
                            table.block_record_handle = None;
                        }
                    });
                    return Task::none();
                }
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if let Some(document::EntityType::MultiLeader(leader)) =
                        app.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        if field == "line_weight" {
                            leader.line_weight = value;
                            leader.property_override_flags.insert(
                                document::entities::MultiLeaderPropertyOverrideFlags::LEADER_LINE_WEIGHT,
                            );
                            for root in &mut leader.context.leader_roots {
                                for line in &mut root.lines {
                                    line.line_weight = value;
                                    line.override_flags.insert(
                                        document::entities::LeaderLinePropertyOverrideFlags::LINE_WEIGHT,
                                    );
                                }
                            }
                        }
                    }
                });
                Task::none()
            }

            Message::PropLinetypeChanged(lt) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    let task = self.on_ribbon_linetype_changed(lt);
                    self.refresh_properties();
                    return task;
                }
                let mut catalog_checked = false;
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if !catalog_checked {
                        catalog_checked = true;
                        let _ = app.ensure_tab_catalog_linetype(i, &lt);
                    }
                    if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                        crate::scene::view::dispatch::apply_common_prop(entity, "linetype", &lt);
                    }
                });
                Task::none()
            }

            Message::PropHatchPatternChanged(name) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                panel.hatch_pattern_picker_open = false;
                panel.hatch_pattern_search.clear();
                self.on_prop_hatch_pattern_changed(name)
            }

            Message::PropHatchPatternPickerToggle(current) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                let opening = !panel.hatch_pattern_picker_open;
                if opening {
                    panel.close_popups();
                    panel.hatch_pattern_focus = crate::ui::properties::filtered_hatch_patterns("")
                        .iter()
                        .position(|entry| entry.name.eq_ignore_ascii_case(&current))
                        .unwrap_or(0);
                }
                panel.hatch_pattern_picker_open = opening;
                if opening {
                    self.block_picker_dismissed = false;
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        "hatch-pattern-search",
                    ));
                }
                panel.hatch_pattern_search.clear();
                panel.hatch_pattern_focus = 0;
                Task::none()
            }

            Message::PropHatchPatternSearchChanged(search) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                panel.hatch_pattern_search = search;
                panel.hatch_pattern_focus = 0;
                Task::none()
            }

            Message::PropHatchPatternFocus(index) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                let len =
                    crate::ui::properties::filtered_hatch_patterns(&panel.hatch_pattern_search)
                        .len();
                if index < len {
                    panel.hatch_pattern_focus = index;
                }
                Task::none()
            }

            Message::PropHatchPatternNavigate(delta) => {
                if let Some(editor) = self.graphic_attributes.hatch_editor.as_mut() {
                    let len = crate::ui::properties::filtered_hatch_patterns(&editor.search).len();
                    if len > 0 {
                        editor.focus = (editor.focus as isize + delta as isize)
                            .rem_euclid(len as isize)
                            as usize;
                    }
                    return Task::none();
                }
                let panel = &mut self.tabs[self.active_tab].properties;
                let len =
                    crate::ui::properties::filtered_hatch_patterns(&panel.hatch_pattern_search)
                        .len();
                if len > 0 {
                    panel.hatch_pattern_focus =
                        (panel.hatch_pattern_focus as isize + delta as isize)
                            .rem_euclid(len as isize) as usize;
                }
                Task::none()
            }

            Message::PropHatchPatternConfirm => {
                let panel = &self.tabs[self.active_tab].properties;
                let name =
                    crate::ui::properties::filtered_hatch_patterns(&panel.hatch_pattern_search)
                        .get(panel.hatch_pattern_focus)
                        .map(|entry| entry.name.clone());
                if let Some(name) = name {
                    self.update(Message::PropHatchPatternChanged(name))
                } else {
                    Task::none()
                }
            }

            Message::PropBoolToggle(field) => {
                if field == "authoring_show_properties" {
                    return self.commit_authoring_metadata(field);
                }
                if field == "authoring_action_base_dependent" {
                    return self.commit_authoring_action_base(field);
                }
                if field == "authoring_chain_actions" {
                    return self.toggle_authoring_chain_actions();
                }
                if field.starts_with("authoring_set_") && field.ends_with("_enabled") {
                    return self.toggle_authoring_value_set_flag(field);
                }
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if crate::app::properties::field_moves_geometry(field)
                    && self.any_entity_is_fixed(i, &handles)
                {
                    self.command_line
                        .push_error(crate::app::properties::HELD_BY_FIXED);
                    return Task::none();
                }
                if !handles.is_empty() {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        match field {
                            // Per-object annotative toggle: MTEXT/MULTILEADER carry
                            // a native flag; single-line TEXT is annotative purely
                            // by the presence of a per-object context. A doc-aware
                            // toggle so turning it on synthesizes a real per-scale
                            // representation and turning it off removes it (not just
                            // the flag).
                            "is_annotative" | "enable_annotation_scale" | "annotative_ctx" => {
                                let doc = &app.tabs[i].scene.document;
                                let cur = match doc.get_entity(handle) {
                                    Some(document::EntityType::MText(t)) => t.is_annotative,
                                    Some(document::EntityType::MultiLeader(m)) => {
                                        m.enable_annotation_scale
                                    }
                                    // TEXT (and any other context-only type): its
                                    // annotative state is whether a context exists.
                                    Some(e) => crate::scene::annotative::is_annotative(doc, e),
                                    None => return,
                                };
                                crate::scene::annotative::set_entity_annotative(
                                    &mut app.tabs[i].scene.document,
                                    handle,
                                    !cur,
                                );
                                // Turning it on also gives the object a real
                                // per-scale representation at the current
                                // annotation scale (not just the native flag),
                                // so it interoperates as a genuine annotative
                                // object. Off is handled inside set_entity_*.
                                if !cur {
                                    if let Some(sh) =
                                        app.tabs[i].scene.creation_annotation_scale_handle()
                                    {
                                        crate::scene::annotative::create_annotation_context(
                                            &mut app.tabs[i].scene.document,
                                            handle,
                                            sh,
                                        );
                                    }
                                }
                            }
                            "invisible" => {
                                if let Some(entity) =
                                    app.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    crate::scene::view::dispatch::toggle_invisible(entity);
                                }
                            }
                            // Uniform-scale checkbox on a block reference
                            // (#427): checking collapses Y/Z onto X; unchecking
                            // only switches the panel to per-axis rows.
                            "ins_uniform" => {
                                let scales = match app.tabs[i].scene.document.get_entity(handle) {
                                    Some(document::EntityType::Insert(ins)) => {
                                        Some((ins.x_scale(), ins.y_scale(), ins.z_scale()))
                                    }
                                    _ => None,
                                };
                                let Some((sx, sy, sz)) = scales else { return };
                                let eq = (sx - sy).abs() < 1e-12 && (sx - sz).abs() < 1e-12;
                                let checked =
                                    eq && !app.tabs[i].props_asym_scale.contains(&handle.value());
                                if checked {
                                    app.tabs[i].props_asym_scale.insert(handle.value());
                                } else {
                                    app.tabs[i].props_asym_scale.remove(&handle.value());
                                    if let Some(document::EntityType::Insert(ins)) =
                                        app.tabs[i].scene.document.get_entity_mut(handle)
                                    {
                                        ins.set_y_scale(sx);
                                        ins.set_z_scale(sx);
                                    }
                                }
                            }
                            "tbl_title_suppressed" | "tbl_header_suppressed" => {
                                let next = {
                                    let document = &app.tabs[i].scene.document;
                                    let Some(document::EntityType::Table(table)) =
                                        document.get_entity(handle)
                                    else {
                                        return;
                                    };
                                    let table_style =
                                        table.table_style_handle.and_then(|style_handle| {
                                            document.objects.get(&style_handle).and_then(|object| {
                                                match object {
                                                    document::objects::ObjectType::TableStyle(
                                                        style,
                                                    ) => Some(style),
                                                    _ => None,
                                                }
                                            })
                                        });
                                    let current = if field == "tbl_title_suppressed" {
                                        crate::entities::table::resolved_title_suppressed(
                                            table,
                                            table_style,
                                        )
                                    } else {
                                        crate::entities::table::resolved_header_suppressed(
                                            table,
                                            table_style,
                                        )
                                    };
                                    !current
                                };
                                if let Some(entity) =
                                    app.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    crate::scene::view::dispatch::apply_geom_prop(
                                        entity,
                                        field,
                                        if next { "true" } else { "false" },
                                    );
                                }
                            }
                            _ => {
                                if let Some(entity) =
                                    app.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    crate::scene::view::dispatch::apply_geom_prop(
                                        entity, field, "toggle",
                                    );
                                }
                            }
                        }
                        if field.starts_with("tbl_") {
                            if let Some(document::EntityType::Table(table)) =
                                app.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                table.block_record_handle = None;
                            }
                        }
                    });
                }
                Task::none()
            }

            Message::PropVertexStep(delta) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                // Navigate the active vertex or table cell.
                let n = if handles.len() == 1 {
                    match self.tabs[i].scene.document.get_entity(handles[0]) {
                        Some(document::EntityType::LwPolyline(p)) => p.vertices.len(),
                        Some(document::EntityType::Polyline2D(p)) => {
                            crate::entities::polyline::polyline2d_frame_indices(p).len()
                        }
                        Some(document::EntityType::PolygonMesh(p)) => p.vertices.len(),
                        Some(document::EntityType::Face3D(face)) => {
                            if face.is_triangle() {
                                3
                            } else {
                                4
                            }
                        }
                        Some(document::EntityType::Polyline3D(p)) => {
                            crate::entities::polyline::polyline3d_control_vertex_count(p)
                        }
                        Some(document::EntityType::Table(table)) => {
                            table.row_count().saturating_mul(table.column_count())
                        }
                        _ => 0,
                    }
                } else {
                    0
                };
                if n > 0 {
                    let cur = self.tabs[i].properties.prop_vertex.min(n - 1) as i64;
                    // Wrap around so ◀ from the first vertex lands on the last.
                    let next = (cur + delta as i64).rem_euclid(n as i64) as usize;
                    self.tabs[i].properties.prop_vertex = next;
                    self.tabs[i].properties.prop_vertex_indicator_active = next != cur as usize;
                    crate::entities::table::set_prop_current_cell(next);
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropGeomChoiceChanged { field, value } => {
                // Picking a row closes the list it came from, whichever panel
                // rendered it.
                for tab in &mut self.tabs {
                    if tab
                        .properties
                        .open_choice
                        .is_some_and(|(open, _)| open == field)
                    {
                        tab.properties.open_choice = None;
                    }
                }
                self.on_prop_geom_choice_changed(field, value)
            }

            Message::PropChoiceToggle { field, quick } => {
                let panel = &mut self.tabs[self.active_tab].properties;
                let opening = panel.open_choice != Some((field, quick));
                if opening {
                    // One list at a time: another list left open behind this
                    // row would stack two overlays.
                    panel.close_popups();
                }
                panel.open_choice = if opening { Some((field, quick)) } else { None };
                self.block_picker_dismissed = false;
                Task::none()
            }

            Message::PropChoiceDismiss { field, quick } => {
                for tab in &mut self.tabs {
                    if tab
                        .properties
                        .open_choice
                        .is_some_and(|open| open == (field, quick))
                    {
                        tab.properties.open_choice = None;
                    }
                }
                self.block_picker_dismissed = true;
                Task::none()
            }

            Message::PropGeomInput { field, value } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(crate::ui::properties::FieldKey::Geom(field), value);
                Task::none()
            }

            Message::PropGeomCommit(field) => self.on_prop_geom_commit(field),
            Message::PropBlockInput { key, value } => {
                if self.block_property_target_is_current(key) {
                    self.tabs[self.active_tab]
                        .properties
                        .edit_buf
                        .insert(crate::ui::properties::FieldKey::BlockParameter(key), value);
                }
                Task::none()
            }
            Message::PropBlockCommit(key) => self.commit_block_parameter_property(key, None),
            Message::PropBlockChoice { key, index } => {
                for tab in &mut self.tabs {
                    if tab
                        .properties
                        .open_block_picker
                        .is_some_and(|(open, _)| open == key)
                    {
                        tab.properties.open_block_picker = None;
                    }
                }
                self.commit_block_parameter_property(key, Some(index))
            }
            Message::PropBlockPickerToggle { key, quick } => {
                let panel = &mut self.tabs[self.active_tab].properties;
                if panel.open_block_picker == Some((key, quick)) {
                    panel.open_block_picker = None;
                } else {
                    panel.close_popups();
                    panel.open_block_picker = Some((key, quick));
                }
                self.block_picker_dismissed = false;
                Task::none()
            }
            Message::PropBlockPickerDismiss { key, quick } => {
                for tab in &mut self.tabs {
                    if tab
                        .properties
                        .open_block_picker
                        .is_some_and(|open| open == (key, quick))
                    {
                        tab.properties.open_block_picker = None;
                    }
                }
                self.block_picker_dismissed = true;
                Task::none()
            }

            Message::PropGroupToggle(key) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                // A collapsed group can take the open list's row off screen.
                panel.open_block_picker = None;
                panel.open_choice = None;
                if !panel.expanded_groups.remove(&key) {
                    panel.expanded_groups.insert(key);
                }
                Task::none()
            }

            Message::PropRowSelect(key) => {
                let i = self.active_tab;
                self.tabs[i].properties.selected_row = Some(key);
                Task::none()
            }

            Message::PropSectionToggle(title) => {
                let sections = &mut self.collapsed_property_sections;
                if !sections.remove(&title) {
                    sections.insert(title);
                }
                // Keep already-built panels in other open documents in sync,
                // rather than waiting for each one to rebuild on selection.
                let sections = self.collapsed_property_sections.clone();
                for tab in &mut self.tabs {
                    tab.properties.collapsed_sections = sections.clone();
                    tab.properties.open_block_picker = None;
                    tab.properties.open_choice = None;
                }
                Task::none()
            }

            Message::PropEditChoiceToggle(field) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                let opening = panel.edit_choice_open != Some(field);
                if opening {
                    panel.close_popups();
                }
                panel.edit_choice_open = opening.then_some(field);
                if opening {
                    self.block_picker_dismissed = false;
                    return iced::widget::operation::focus(
                        crate::ui::properties::prop_geom_field_id(field),
                    );
                }
                Task::none()
            }

            Message::PropAttrInput { tag, value } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(crate::ui::properties::attr_edit_key(&tag), value);
                Task::none()
            }

            Message::PropAttrCommit(tag) => self.on_prop_attr_commit(tag),

            Message::PropPluginParamInput { name, value } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(crate::ui::properties::plugin_param_edit_key(&name), value);
                Task::none()
            }

            Message::PropPluginParamCommit(name) => self.on_prop_plugin_param_commit(name),

            Message::PropPointerPressed => {
                // A press anywhere ends typing in QuickCalc; its own
                // buttons set it again after their release.
                self.quick_calc.typing = false;
                let mut rename_blur = if self.tabs[self.active_tab].layers.editing.is_some() {
                    crate::ui::properties::focused_widget_task(Message::LayerRenameFocus)
                } else {
                    Task::none()
                };
                if self.tabs[self.active_tab]
                    .layers
                    .transparency_edit
                    .is_some()
                {
                    rename_blur = Task::batch([
                        rename_blur,
                        crate::ui::properties::focused_widget_task(Message::LayerTransparencyFocus),
                    ]);
                }
                if self.tabs[self.active_tab].layers.description_edit.is_some() {
                    rename_blur = Task::batch([
                        rename_blur,
                        crate::ui::properties::focused_widget_task(Message::LayerDescriptionFocus),
                    ]);
                }
                if self.style_rename.is_some() {
                    rename_blur = Task::batch([
                        rename_blur,
                        crate::ui::properties::focused_widget_task(Message::StyleRenameFocus),
                    ]);
                }
                if self.tabs[self.active_tab]
                    .parameters_panel
                    .editing
                    .is_some()
                {
                    rename_blur = Task::batch([
                        rename_blur,
                        crate::ui::properties::focused_widget_task(Message::ParametersManagerFocus),
                    ]);
                }
                if !self.dock_panel_visible(crate::ui::dock::PanelId::Properties)
                    && !self.dock_panel_visible(crate::ui::dock::PanelId::GraphicAttributes)
                {
                    return rename_blur;
                }
                Task::batch([crate::ui::properties::sync_active_field_task(), rename_blur])
            }

            Message::PropSyncActive(focused) => {
                // The field the focus left keeps what was typed into it.
                let left = self.tabs[self.active_tab]
                    .properties
                    .active_field
                    .clone()
                    .filter(|key| {
                        !crate::ui::properties::active_key_focused(Some(key), focused.as_ref())
                    });
                let commit = match left {
                    Some(key) => self.commit_prop_field(key),
                    None => Task::none(),
                };
                // The Graphic Attributes linetype scale field behaves like a
                // Properties field: the first click selects its value.
                let scale_field = iced::widget::Id::new(
                    crate::ui::window::graphic_attributes::LINETYPE_SCALE_FIELD,
                );
                if focused.as_ref() == Some(&scale_field) {
                    if !std::mem::replace(&mut self.graphic_attributes.linetype_scale_focused, true)
                    {
                        return Task::batch([
                            commit,
                            iced::widget::operation::select_all(scale_field),
                        ]);
                    }
                    return commit;
                }
                self.graphic_attributes.linetype_scale_focused = false;
                self.graphic_attributes.linetype_scale_input = None;
                let panel = &mut self.tabs[self.active_tab].properties;
                if let Some(id) = focused.as_ref() {
                    if let Some(key) = panel.prop_field_key_for_id(id) {
                        let changed = panel.active_field.as_ref() != Some(&key);
                        panel.active_field = Some(key);
                        // Only select the whole value when focus landed on a
                        // field that wasn't already the active one; re-focusing
                        // the same field (or sweeping after a click elsewhere
                        // left its focus in place) must keep caret placement.
                        if changed {
                            return Task::batch([
                                commit,
                                iced::widget::operation::select_all(id.clone()),
                            ]);
                        }
                        return commit;
                    }
                }
                if !crate::ui::properties::active_key_focused(
                    panel.active_field.as_ref(),
                    focused.as_ref(),
                ) {
                    panel.active_field = None;
                }
                commit
            }

            Message::PropPickerSurface(quick, message) => {
                let task = self.update(*message);
                let i = self.active_tab;
                self.tabs[i].properties.picker_in_quick = quick;
                task
            }

            Message::PropColorPickerToggle => {
                let panel = &mut self.tabs[self.active_tab].properties;
                let opening = !panel.color_picker_open;
                if opening {
                    panel.close_popups();
                }
                panel.color_picker_open = opening;
                if opening {
                    self.block_picker_dismissed = false;
                }
                Task::none()
            }

            Message::PropBgColorPickerToggle => {
                let panel = &mut self.tabs[self.active_tab].properties;
                let opening = !panel.bg_color_picker_open;
                if opening {
                    panel.close_popups();
                }
                panel.bg_color_picker_open = opening;
                if opening {
                    self.block_picker_dismissed = false;
                }
                Task::none()
            }

            Message::PropBgColorChanged(color) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        match app.tabs[i].scene.document.get_entity_mut(handle) {
                            // None turns the background mask off.
                            Some(document::EntityType::MText(m))
                                if color == document::types::Color::None =>
                            {
                                m.background_fill_flags &= !0x03;
                            }
                            Some(document::EntityType::MText(m)) => {
                                m.background_color = color;
                                // Picking a colour turns the background on in Fill
                                // mode (specific colour), preserving the frame bit.
                                m.background_fill_flags = (m.background_fill_flags & !0x02) | 0x01;
                            }
                            Some(document::EntityType::Hatch(h)) => {
                                if color == document::types::Color::None {
                                    crate::entities::hatch::clear_background_color(h);
                                } else {
                                    crate::entities::hatch::set_background_color(h, &color);
                                }
                            }
                            _ => {}
                        }
                    });
                    self.tabs[i].properties.bg_color_picker_open = false;
                }
                Task::none()
            }

            Message::PropColorFieldToggle(field) => {
                let i = self.active_tab;
                let p = &mut self.tabs[i].properties;
                if p.open_color_field.as_deref() == Some(field.as_str()) {
                    p.open_color_field = None;
                } else {
                    p.close_popups();
                    p.open_color_field = Some(field);
                    self.block_picker_dismissed = false;
                }
                Task::none()
            }

            Message::PropColorFieldChanged { field, color } => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if field == "indicator_fill_color" {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if let Some(document::EntityType::Extended(extended)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            if let document::entities::ExtendedEntityData::SectionObject(data) =
                                &mut extended.data
                            {
                                data.indicator_color = color;
                            }
                        }
                    });
                    self.tabs[i].properties.open_color_field = None;
                    return Task::none();
                }
                // Dim-line colour override (Leader / Dimension): write it as an
                // ACAD_DSTYLE code-176 override (an ACI index) so it round-trips
                // through DWG and DXF. RGB picks collapse to the nearest ACI, in
                // line with the rest of the dim-colour stack (index-only through
                // the file layer). Guarded to leaders / dimensions so a mixed
                // selection can't stamp the override onto other entities.
                if matches!(
                    field.as_str(),
                    "dim_line_color"
                        | "dim_ext_line_color"
                        | "dim_text_color"
                        | "dim_text_fill_color"
                ) {
                    let fill_mode = (field == "dim_text_fill_color").then_some(match color {
                        document::types::Color::None => 0,
                        document::types::Color::ByBlock => 1,
                        _ => 2,
                    });
                    let aci = color.approximate_index();
                    let code = match field.as_str() {
                        "dim_ext_line_color" => crate::entities::dim_override::DIMCLRE,
                        "dim_text_color" => crate::entities::dim_override::DIMCLRT,
                        "dim_text_fill_color" => crate::entities::dim_override::DIMTFILLCLR,
                        _ => crate::entities::dim_override::DIMCLRD,
                    };
                    let targets: Vec<document::Handle> = handles
                        .iter()
                        .copied()
                        .filter(|&h| {
                            matches!(
                                self.tabs[i].scene.document.get_entity(h),
                                Some(document::EntityType::Leader(_))
                                    | Some(document::EntityType::Dimension(_))
                            )
                        })
                        .collect();
                    if !targets.is_empty() {
                        self.apply_property_op(i, "CHPROP", &targets, |app, handle| {
                            if field == "dim_text_fill_color" {
                                crate::entities::dim_override::set(
                                    &mut app.tabs[i].scene.document,
                                    handle,
                                    crate::entities::dim_override::DIMTFILL,
                                    Some(document::xdata::XDataValue::Integer16(
                                        fill_mode.unwrap_or(2),
                                    )),
                                );
                                if fill_mode != Some(2) {
                                    return;
                                }
                            }
                            crate::entities::dim_override::set(
                                &mut app.tabs[i].scene.document,
                                handle,
                                code,
                                Some(document::xdata::XDataValue::Integer16(aci)),
                            );
                        });
                        self.tabs[i].properties.open_color_field = None;
                    }
                    return Task::none();
                }
                if matches!(
                    field.as_str(),
                    "line_color" | "text_color" | "block_content_color" | "background_fill_color"
                ) {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                            if let Some(document::EntityType::MultiLeader(leader)) =
                                app.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                match field.as_str() {
                                    "line_color" => {
                                        leader.line_color = color;
                                        leader.property_override_flags.insert(
                                            document::entities::MultiLeaderPropertyOverrideFlags::LINE_COLOR,
                                        );
                                        for root in &mut leader.context.leader_roots {
                                            for line in &mut root.lines {
                                                line.line_color = color;
                                                line.override_flags.insert(
                                                    document::entities::LeaderLinePropertyOverrideFlags::LINE_COLOR,
                                                );
                                            }
                                        }
                                    }
                                    "text_color" => {
                                        leader.text_color = color;
                                        leader.context.text_color = color;
                                        leader.property_override_flags.insert(
                                            document::entities::MultiLeaderPropertyOverrideFlags::TEXT_COLOR,
                                        );
                                    }
                                    "block_content_color" => {
                                        leader.block_content_color = color;
                                        leader.context.block_content_color = color;
                                        leader.property_override_flags.insert(
                                            document::entities::MultiLeaderPropertyOverrideFlags::BLOCK_CONTENT_COLOR,
                                        );
                                    }
                                    "background_fill_color" => {
                                        leader.context.background_fill_color = color;
                                    }
                                    _ => {}
                                }
                            }
                    });
                    self.tabs[i].properties.open_color_field = None;
                    return Task::none();
                }
                if matches!(
                    field.as_str(),
                    "tbl_cell_content_color" | "tbl_cell_background_color"
                ) {
                    let cells = self.picked_cell_positions(i);
                    if !handles.is_empty() {
                        self.apply_property_op(i, "TABLE CELL COLOR", &handles, |app, handle| {
                            let Some(document::EntityType::Table(table)) =
                                app.tabs[i].scene.document.get_entity_mut(handle)
                            else {
                                return;
                            };
                            for &(row, column) in &cells {
                            if let Some(cell) = table.cell_mut(row, column) {
                                use document::entities::table::CellStateFlags;
                                if cell.state.intersects(
                                    CellStateFlags::FORMAT_LOCKED
                                        | CellStateFlags::FORMAT_READ_ONLY,
                                ) {
                                    continue;
                                }
                                let style = cell.style.get_or_insert_with(Default::default);
                                if field == "tbl_cell_content_color" {
                                    style.content_color = color;
                                    style.property_flags.insert(
                                        document::entities::table::CellStylePropertyFlags::CONTENT_COLOR,
                                    );
                                } else {
                                    style.background_color = color;
                                    style.fill_enabled = true;
                                    style.property_flags.insert(
                                        document::entities::table::CellStylePropertyFlags::BACKGROUND_COLOR,
                                    );
                                }
                            }
                            }
                            table.block_record_handle = None;
                        });
                        self.tabs[i].properties.open_color_field = None;
                    }
                    return Task::none();
                }
                if !handles.is_empty() {
                    let idx = if field == "gradient_color_2" { 1 } else { 0 };
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if let Some(document::EntityType::Hatch(h)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            while h.gradient_color.colors.len() <= idx {
                                let value = if h.gradient_color.colors.is_empty() {
                                    0.0
                                } else {
                                    1.0
                                };
                                h.gradient_color.colors.push(
                                    document::entities::hatch::GradientColorEntry {
                                        value,
                                        color: document::types::Color::Index(7),
                                        ..Default::default()
                                    },
                                );
                            }
                            h.gradient_color.colors[idx].color = color;
                        }
                    });
                    // Rebuild hatch seeds so the gradient fill picks up the new
                    // colour (synced_hatch_models only patches the main colour).
                    self.tabs[i].scene.populate_hatches_from_document();
                    self.tabs[i].properties.open_color_field = None;
                }
                Task::none()
            }

            Message::PropColorPickerClose => {
                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open = false;
                self.tabs[i].properties.hatch_pattern_picker_open = false;
                self.tabs[i].properties.hatch_pattern_search.clear();
                Task::none()
            }

            Message::LayoutSwitch(name) => {
                self.layout_list_open = false;
                self.on_layout_switch(name)
            }

            Message::BlockEditSwitch(name) => {
                self.layout_list_open = false;
                self.on_block_edit_switch(name)
            }

            Message::LayoutReorder { from, to, after } => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    return Task::none();
                }
                let mut paper: Vec<String> = self.tabs[i]
                    .scene
                    .layout_names()
                    .into_iter()
                    .skip(1)
                    .collect();
                let Some(from_index) = paper.iter().position(|name| name == &from) else {
                    return Task::none();
                };
                let Some(to_index) = paper.iter().position(|name| name == &to) else {
                    return Task::none();
                };
                let Some(insertion) =
                    reorder_insertion_index(from_index, to_index, after, paper.len())
                else {
                    return Task::none();
                };

                let moved = paper.remove(from_index);
                paper.insert(insertion, moved);
                self.push_undo_snapshot(i, "LAYOUT REORDER");
                self.tabs[i].scene.set_layout_tab_order(&paper);
                self.tabs[i].dirty = true;
                Task::none()
            }

            Message::LayoutCreate => self.on_layout_create(),

            Message::LayoutCopy(name) => self.on_layout_copy(name),

            Message::LayoutPageSetup(name) => {
                let switch_task = self.on_layout_switch(name);
                Task::batch([switch_task, self.on_plot_dialog_open()])
            }

            Message::LayoutPlot(name) => {
                let switch_task = self.on_layout_switch(name);
                Task::batch([switch_task, Task::done(Message::Command("PLOT".into()))])
            }

            Message::LayoutDelete(name) => {
                let i = self.active_tab;
                let deleting_current = self.tabs[i].scene.current_layout == name;
                let cancel_task = if deleting_current {
                    self.cancel_active_command_for_space_change()
                } else {
                    Task::none()
                };
                self.push_undo_snapshot(i, "LAYOUT DEL");
                let switch_task = if deleting_current {
                    self.on_layout_switch("Model".to_string())
                } else {
                    Task::none()
                };
                if self.tabs[i].scene.delete_layout(&name) {
                    self.layout_rename_state = None;
                    self.command_line
                        .push_output(crate::tf!("Layout \"{name}\" silindi").as_ref());
                    self.tabs[i].dirty = true;
                }
                Task::batch([cancel_task, switch_task])
            }

            Message::LayoutRenameStart(name) => {
                if name != "Model" {
                    self.layout_rename_state = Some((name.clone(), name));
                    // Focus the inline field so the user types into it
                    // directly instead of the command line (issue #86).
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        crate::ui::statusbar::LAYOUT_RENAME_INPUT_ID,
                    ));
                }
                Task::none()
            }

            Message::LayoutRenameEdit(val) => {
                if let Some((orig, _)) = &self.layout_rename_state {
                    let orig = orig.clone();
                    self.layout_rename_state = Some((orig, val));
                }
                Task::none()
            }

            Message::LayoutRenameCommit => self.on_layout_rename_commit(),

            Message::LayoutRenameCancel => {
                self.layout_rename_state = None;
                Task::none()
            }

            // ── Layout Manager Panel ──────────────────────────────────────────
            Message::LayoutManagerOpen => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    self.command_line.push_info(
                        crate::t!("Open or create a drawing to manage layouts.").as_ref(),
                    );
                    return Task::none();
                }
                let current = self.tabs[i].scene.current_layout.clone();
                self.layout_manager_selected = current.clone();
                self.layout_manager_rename_buf = if current == "Model" {
                    String::new()
                } else {
                    current
                };
                self.active_modal = Some(super::super::super::ModalKind::LayoutManager);
                Task::none()
            }
            Message::LayoutManagerClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::LayoutManagerSelect(name) => {
                self.layout_manager_rename_buf = if name == "Model" {
                    String::new()
                } else {
                    name.clone()
                };
                self.layout_manager_selected = name;
                Task::none()
            }
            Message::LayoutManagerRenameBuf(s) => {
                self.layout_manager_rename_buf = s;
                Task::none()
            }
            Message::LayoutManagerRenameCommit => {
                let i = self.active_tab;
                let old_name = self.layout_manager_selected.clone();
                let new_name = self.layout_manager_rename_buf.trim().to_string();
                if old_name == "Model" {
                    self.command_line
                        .push_error(crate::t!("Cannot rename the Model layout.").as_ref());
                } else if new_name.is_empty() {
                    self.command_line
                        .push_error(crate::t!("Layout name cannot be empty.").as_ref());
                } else if new_name == old_name {
                    // no-op
                } else {
                    self.push_undo_snapshot(i, "LAYOUT RENAME");
                    self.tabs[i].scene.rename_layout(&old_name, &new_name);
                    if self.tabs[i].scene.current_layout == old_name {
                        self.tabs[i].scene.set_current_layout(new_name.clone());
                    }
                    self.layout_manager_selected = new_name.clone();
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(
                        crate::tf!("Layout renamed: '{old_name}' → '{new_name}'").as_ref(),
                    );
                }
                Task::none()
            }
            Message::LayoutManagerNew => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    self.command_line
                        .push_info(crate::t!("Open or create a drawing to add a layout.").as_ref());
                    return Task::none();
                }
                let existing = self.tabs[i].scene.layout_names();
                let n = (1usize..)
                    .find(|n| !existing.contains(&format!("Layout{n}")))
                    .unwrap_or(1);
                let name = format!("Layout{n}");
                self.push_undo_snapshot(i, "LAYOUT NEW");
                match self.tabs[i].scene.add_layout(&name) {
                    Ok(_) => {
                        self.tabs[i].dirty = true;
                        self.layout_manager_selected = name.clone();
                        self.layout_manager_rename_buf = name.clone();
                        self.command_line
                            .push_output(crate::tf!("Layout '{name}' created.").as_ref());
                    }
                    Err(e) => self
                        .command_line
                        .push_error(crate::tf!("LAYOUT: {e}").as_ref()),
                }
                Task::none()
            }
            Message::LayoutManagerDelete => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    self.command_line
                        .push_error(crate::t!("Cannot delete the Model layout.").as_ref());
                    Task::none()
                } else {
                    let deleting_current = self.tabs[i].scene.current_layout == name;
                    let cancel_task = if deleting_current {
                        self.cancel_active_command_for_space_change()
                    } else {
                        Task::none()
                    };
                    self.push_undo_snapshot(i, "LAYOUT DELETE");
                    let switch_task = if deleting_current {
                        self.on_layout_switch("Model".to_string())
                    } else {
                        Task::none()
                    };
                    self.tabs[i].scene.delete_layout(&name);
                    self.tabs[i].dirty = true;
                    self.layout_manager_selected = "Model".to_string();
                    self.layout_manager_rename_buf = String::new();
                    self.command_line
                        .push_output(crate::tf!("Layout '{name}' deleted.").as_ref());
                    Task::batch([cancel_task, switch_task])
                }
            }
            Message::LayoutManagerMoveLeft => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    return Task::none();
                }
                let names = self.tabs[i].scene.layout_names();
                // Find position among paper layouts only.
                let paper: Vec<&str> = names.iter().skip(1).map(|s| s.as_str()).collect();
                if let Some(pos) = paper.iter().position(|&n| n == name) {
                    if pos > 0 {
                        self.push_undo_snapshot(i, "LAYOUT REORDER");
                        self.tabs[i].scene.swap_layout_order(&name, paper[pos - 1]);
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }
            Message::LayoutManagerMoveRight => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    return Task::none();
                }
                let names = self.tabs[i].scene.layout_names();
                let paper: Vec<&str> = names.iter().skip(1).map(|s| s.as_str()).collect();
                if let Some(pos) = paper.iter().position(|&n| n == name) {
                    if pos + 1 < paper.len() {
                        self.push_undo_snapshot(i, "LAYOUT REORDER");
                        self.tabs[i].scene.swap_layout_order(&name, paper[pos + 1]);
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }
            Message::LayoutManagerSetCurrent => {
                let name = self.layout_manager_selected.clone();
                let task = self.on_layout_switch(name.clone());
                if self.tabs[self.active_tab].scene.current_layout == name {
                    self.command_line
                        .push_output(crate::tf!("Switched to layout '{name}'.").as_ref());
                }
                task
            }

            Message::SetTheme(theme) => {
                if self.ui_theme.name == "Custom" {
                    self.saved_custom_palette = Some(self.ui_theme.palette);
                }
                self.ui_theme.name = theme.to_string();
                self.ui_theme.palette = crate::app::config::UiThemePalette::from_iced(theme.seed());
                self.theme_color_inputs = self.ui_theme.palette.hex_values();
                self.active_theme = theme;
                self.sync_model_space_theme(true);
                self.persist_settings_if_changed();
                Task::none()
            }

            // ── Keyboard Shortcuts Panel ──────────────────────────────────────
            Message::ShortcutsPanelOpen => {
                let mut rows: Vec<(String, String)> = self
                    .shortcut_bindings
                    .iter()
                    .map(|(key, command)| (key.clone(), command.clone()))
                    .collect();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                self.shortcut_editor_rows = rows;
                self.shortcut_filter.clear();
                self.shortcut_capture_row = None;
                self.shortcut_pending_add = false;
                self.shortcut_suggest = None;
                self.shortcut_reset_confirm = false;
                self.shortcut_close_confirm = false;
                self.active_modal = Some(super::super::super::ModalKind::Shortcuts);
                Task::none()
            }
            Message::ShortcutEditorFilter(value) => {
                self.shortcut_filter = value;
                Task::none()
            }
            Message::ShortcutsPanelClose => {
                if !self.shortcut_close_confirm && self.shortcut_editor_dirty() {
                    self.shortcut_close_confirm = true;
                    return Task::none();
                }
                self.shortcut_close_confirm = false;
                self.close_active_modal();
                Task::none()
            }
            Message::ShortcutEditorCloseDiscard => {
                self.shortcut_close_confirm = false;
                self.close_active_modal();
                Task::none()
            }
            Message::ShortcutEditorCloseKeep => {
                self.shortcut_close_confirm = false;
                Task::none()
            }
            Message::ShortcutEditorInput { idx, field, value } => {
                use crate::ui::window::shortcuts::ShortcutField;
                if let Some(row) = self.shortcut_editor_rows.get_mut(idx) {
                    match field {
                        ShortcutField::Key => row.0 = value.to_uppercase(),
                        ShortcutField::Command => {
                            row.1 = value.to_uppercase();
                            // Typing opens this row's suggestion list.
                            self.shortcut_suggest = Some((idx, row.1.clone()));
                        }
                    }
                }
                // Typing never finishes the addition — only the draft row's
                // check button does, so no half-visible "ghost" row appears.
                Task::none()
            }
            Message::ShortcutEditorSuggestionPick { idx, command } => {
                if let Some(row) = self.shortcut_editor_rows.get_mut(idx) {
                    row.1 = command;
                }
                self.shortcut_suggest = None;
                // The box keeps the caret where the typed prefix ended; put
                // it after the picked name (and keep the box focused after a
                // mouse pick) so typing continues at the end.
                let id = crate::ui::window::shortcuts::command_input_id(idx);
                Task::batch([
                    iced::widget::operation::focus(id.clone()),
                    iced::widget::operation::move_cursor_to_end(id),
                ])
            }
            Message::ShortcutEditorSuggestionDismiss => {
                self.shortcut_suggest = None;
                Task::none()
            }
            Message::ShortcutEditorAdd => {
                self.shortcut_suggest = None;
                if self.shortcut_pending_add {
                    // One draft at a time: re-arm its Key cell instead.
                    self.shortcut_capture_row = Some(0);
                } else {
                    // The draft row goes to the top of the list so it is
                    // visible without scrolling, with its Key cell armed.
                    self.shortcut_editor_rows
                        .insert(0, (String::new(), String::new()));
                    self.shortcut_pending_add = true;
                    self.shortcut_capture_row = Some(0);
                }
                Task::none()
            }
            Message::ShortcutCaptureStart(idx) => {
                // Clicking the armed cell again cancels capture.
                self.shortcut_capture_row = match self.shortcut_capture_row {
                    Some(armed) if armed == idx => None,
                    _ => Some(idx),
                };
                Task::none()
            }
            Message::ShortcutCaptureKey(raw) => {
                if let Some(idx) = self.shortcut_capture_row.take() {
                    let key = crate::app::shortcuts::normalize_key(&raw);
                    if !key.is_empty() && self.shortcut_editor_rows.get_mut(idx).is_some() {
                        self.shortcut_editor_rows[idx].0 = key;
                    }
                }
                Task::none()
            }
            Message::ShortcutCaptureClear => {
                self.shortcut_capture_row = None;
                Task::none()
            }
            Message::ShortcutCaptureCancel => {
                // Esc backs out of capture and abandons an unfinished draft.
                self.shortcut_capture_row = None;
                self.shortcut_suggest = None;
                if self.shortcut_pending_add {
                    self.shortcut_editor_rows.remove(0);
                    self.shortcut_pending_add = false;
                }
                Task::none()
            }
            Message::ShortcutEditorRemove(idx) => {
                self.shortcut_suggest = None;
                if idx < self.shortcut_editor_rows.len() {
                    self.shortcut_editor_rows.remove(idx);
                }
                if idx == 0 {
                    // Removing the draft row is the ✕ cancel path.
                    self.shortcut_pending_add = false;
                }
                // Armed indices shift when a row above them disappears.
                self.shortcut_capture_row = match self.shortcut_capture_row {
                    Some(armed) if armed == idx => None,
                    Some(armed) if armed > idx => Some(armed - 1),
                    other => other,
                };
                Task::none()
            }
            Message::ShortcutEditorApply => {
                self.finish_shortcut_editor();
                Task::none()
            }
            Message::ShortcutEditorApplyExit => {
                self.finish_shortcut_editor();
                self.close_active_modal();
                Task::none()
            }
            Message::ShortcutEditorDraftAccept => {
                // The check button: finish the addition (keep the row in the
                // working table) without applying it. Only complete drafts
                // can be accepted — the button is disabled otherwise.
                self.finish_pending_add();
                Task::none()
            }
            Message::ShortcutEditorImport => Task::perform(
                async {
                    let handle = rfd::AsyncFileDialog::new()
                        .add_filter(
                            crate::t!("Shortcut lists (*.xlsx, *.csv, *.txt)").as_ref(),
                            &["xlsx", "csv", "txt"],
                        )
                        .pick_file()
                        .await?;
                    #[cfg(not(target_arch = "wasm32"))]
                    let bytes = std::fs::read(handle.path()).map_err(|e| e.to_string());
                    #[cfg(target_arch = "wasm32")]
                    let bytes: Result<Vec<u8>, String> = Ok(handle.read().await);
                    Some(bytes)
                },
                Message::ShortcutEditorImported,
            ),
            Message::ShortcutEditorImported(result) => {
                // A picker can finish after the modal was dismissed.
                if self.dialog_open(super::super::super::ModalKind::Shortcuts) {
                    match result {
                        Some(Ok(bytes)) => self.import_shortcut_rows(&bytes),
                        Some(Err(e)) => self.command_line.push_error(
                            crate::tf!("Shortcuts: cannot read the file: {e}").as_ref(),
                        ),
                        None => {}
                    }
                }
                Task::none()
            }
            Message::ShortcutEditorResetAsk => {
                self.shortcut_reset_confirm = true;
                Task::none()
            }
            Message::ShortcutEditorResetDeny => {
                self.shortcut_reset_confirm = false;
                Task::none()
            }
            Message::ShortcutEditorResetConfirm => {
                self.reset_shortcuts_to_defaults();
                self.command_line.push_info(
                    crate::tf!("{} shortcut(s) applied.", self.shortcut_bindings.len()).as_ref(),
                );
                Task::none()
            }
            Message::ShortcutPressed(key) => {
                if key == "SPACE" && self.cycle_lasso_mode() {
                    return Task::none();
                }
                if key == "ESCAPE" && self.cancel_layer_manager_edit() {
                    return Task::none();
                }
                if self.mtext_navigation_key(&key) {
                    return Task::none();
                }
                if let Some(task) = self.mtext_editing_key(&key) {
                    return task;
                }
                if self.table_cell_line_break(&key) {
                    return Task::none();
                }
                if let Some(task) = self.layer_manager_key(&key) {
                    return task;
                }
                if let Some(task) = self.xref_manager_key(&key) {
                    return task;
                }
                if let Some(task) = self.quick_calc_shortcut(&key) {
                    return task;
                }
                self.run_shortcut(&key)
            }

            // ── Command Alias Editor (ALIASEDIT) ──────────────────────────────
            Message::AliasEditorOpen => {
                // Seed the working buffer from the current table, sorted by alias
                // so the list is stable and diffable.
                let mut rows: Vec<(String, String)> = self
                    .command_aliases
                    .iter()
                    .map(|(a, c)| (a.clone(), c.clone()))
                    .collect();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                self.alias_editor_rows = rows;
                self.alias_import_report.clear();
                self.alias_pending_add = false;
                self.alias_reset_confirm = false;
                self.alias_close_confirm = false;
                self.active_modal = Some(super::super::super::ModalKind::Aliases);
                Task::none()
            }
            Message::AliasEditorInput { idx, field, value } => {
                use crate::ui::window::alias_editor::AliasField;
                // Aliases and commands are stored uppercase; uppercasing as the
                // user types keeps display and the committed table consistent.
                let value = value.to_uppercase();
                if let Some(rowdata) = self.alias_editor_rows.get_mut(idx) {
                    match field {
                        AliasField::Alias => rowdata.0 = value,
                        AliasField::Command => rowdata.1 = value,
                    }
                }
                // Typing never finishes the addition — only the draft row's
                // check button does, so no half-visible "ghost" row appears.
                Task::none()
            }
            Message::HatchPanel(msg) => self.on_hatch_panel(msg),
            Message::RibbonCustomizeOpen => {
                let selected = (!self.ribbon.custom_panels().is_empty()).then_some(0);
                self.ribbon_customize = Some(crate::ui::window::ribbon_customize::State {
                    selected,
                    ..Default::default()
                });
                self.active_modal = Some(crate::app::ModalKind::RibbonCustomize);
                Task::none()
            }
            Message::RibbonCustomize(msg) => self.on_ribbon_customize(msg),
            Message::ContextMenuEdit(msg) => self.on_context_menu_edit(msg),
            Message::KeyListExport(aliases) => {
                let (header, rows, name) = if aliases {
                    (
                        [crate::t!("Alias"), crate::t!("Command")],
                        &self.alias_editor_rows,
                        "aliases",
                    )
                } else {
                    (
                        [crate::t!("Key"), crate::t!("Command")],
                        &self.shortcut_editor_rows,
                        "shortcuts",
                    )
                };
                let table: Vec<Vec<String>> =
                    std::iter::once(header.map(|h| h.into_owned()).to_vec())
                        .chain(rows.iter().map(|(a, b)| vec![a.clone(), b.clone()]))
                        .collect();
                #[cfg(target_arch = "wasm32")]
                {
                    // The web build downloads it as a workbook.
                    let path = std::path::PathBuf::from(format!("{name}.xlsx"));
                    return Task::done(Message::KeyListExportTo(Some(path), table));
                }
                #[cfg(not(target_arch = "wasm32"))]
                Task::perform(
                    async move {
                        let path = crate::sys::file_dialog()
                            .set_file_name(format!("{name}.xlsx"))
                            .add_filter("Excel (*.xlsx)", &["xlsx"])
                            .add_filter("CSV (*.csv)", &["csv"])
                            .add_filter("Text (*.txt)", &["txt"])
                            .save_file()
                            .await
                            .map(|handle| crate::sys::handle_path(&handle));
                        (path, table)
                    },
                    |(path, table)| Message::KeyListExportTo(path, table),
                )
            }
            Message::KeyListExportTo(path, table) => {
                if let Some(path) = path {
                    match crate::app::annotation_data::write_extraction_file(&path, &table) {
                        Ok(()) => self
                            .command_line
                            .push_info(crate::tf!("Exported: {}", path.display()).as_ref()),
                        Err(error) => self.command_line.push_error(&error),
                    }
                }
                Task::none()
            }
            Message::AliasEditorImport => Task::perform(
                async {
                    let handle = rfd::AsyncFileDialog::new()
                        .add_filter(crate::t!("Command aliases (*.pgp)").as_ref(), &["pgp"])
                        .add_filter("Excel (*.xlsx)", &["xlsx"])
                        .pick_file()
                        .await?;
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        Some(std::fs::read(handle.path()).map_err(|e| e.to_string()))
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        Some(Ok(handle.read().await))
                    }
                },
                Message::AliasEditorImported,
            ),
            _ => Task::none(),
        }
    }
}
