use super::super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_blocks_settings(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::AliasEditorImported(result) => {
                // A picker can finish after the modal was dismissed.
                if self.active_modal == Some(super::super::super::ModalKind::Aliases) {
                    match result {
                        Some(Ok(bytes)) => self.import_alias_pgp(&bytes),
                        Some(Err(error)) => self.command_line.push_error(&format!("PGP: {error}")),
                        None => {}
                    }
                }
                Task::none()
            }
            Message::AliasEditorAdd => {
                if self.alias_pending_add {
                    // One draft at a time: keep the existing top draft.
                } else {
                    // The draft row goes to the top of the list so it is
                    // visible without scrolling.
                    self.alias_editor_rows
                        .insert(0, (String::new(), String::new()));
                    self.alias_pending_add = true;
                }
                Task::none()
            }
            Message::AliasEditorDraftAccept => {
                // The check button: finish the addition (keep the row in the
                // working table) without applying it. Only complete drafts
                // can be accepted — the button is disabled otherwise.
                self.finish_pending_alias_add();
                Task::none()
            }
            Message::AliasEditorDraftCancel => {
                // Esc backs out and abandons an unfinished draft.
                if self.alias_pending_add && !self.alias_editor_rows.is_empty() {
                    self.alias_editor_rows.remove(0);
                    self.alias_pending_add = false;
                }
                Task::none()
            }
            Message::AliasEditorRemove(idx) => {
                if idx < self.alias_editor_rows.len() {
                    self.alias_editor_rows.remove(idx);
                }
                if idx == 0 {
                    // Removing the draft row is the ✕ cancel path.
                    self.alias_pending_add = false;
                }
                Task::none()
            }
            Message::AliasEditorApply => {
                self.finish_alias_editor();
                Task::none()
            }
            Message::AliasEditorApplyExit => {
                self.finish_alias_editor();
                self.close_active_modal();
                Task::none()
            }
            Message::AliasEditorResetAsk => {
                self.alias_reset_confirm = true;
                Task::none()
            }
            Message::AliasEditorResetDeny => {
                self.alias_reset_confirm = false;
                Task::none()
            }
            Message::AliasEditorResetConfirm => {
                self.reset_aliases_to_defaults();
                self.command_line.push_info(
                    crate::tf!("{} alias(es) applied.", self.command_aliases.len()).as_ref(),
                );
                Task::none()
            }
            Message::AliasEditorCloseDiscard => {
                self.alias_close_confirm = false;
                self.close_active_modal();
                Task::none()
            }
            Message::AliasEditorCloseKeep => {
                self.alias_close_confirm = false;
                Task::none()
            }

            // ── Named Parameters (PARAMETERS) ─────────────────────────────────
            Message::NamedParametersOpen => {
                let i = self.active_tab;
                self.named_parameter_editor_rows = self.tabs[i]
                    .scene
                    .named_parameters()
                    .iter()
                    .map(|p| crate::ui::window::named_parameters::ParamEditorRow {
                        name: p.name.clone(),
                        formula: p.source.clone(),
                    })
                    .collect();
                self.active_modal = Some(super::super::super::ModalKind::NamedParameters);
                Task::none()
            }
            Message::BlockCycleSelect(index) => {
                if let Some(state) = self.block_cycle_order.as_mut() {
                    if index < state.rows.len() {
                        state.selected = Some(index);
                    }
                }
                Task::none()
            }
            Message::BlockCycleEnable(index, enabled) => {
                if let Some(state) = self.block_cycle_order.as_mut() {
                    if let Some(row) = state.rows.get_mut(index) {
                        row.enabled = enabled;
                        state.selected = Some(index);
                    }
                    state.error = None;
                }
                Task::none()
            }
            Message::BlockCycleMove(index, up) => {
                if let Some(state) = self.block_cycle_order.as_mut() {
                    let target = if up {
                        index.checked_sub(1)
                    } else {
                        index.checked_add(1)
                    };
                    if let Some(target) = target
                        .filter(|target| *target < state.rows.len() && index < state.rows.len())
                    {
                        state.rows.swap(index, target);
                        state.selected = state.selected.map(|selected| {
                            if selected == index {
                                target
                            } else if selected == target {
                                index
                            } else {
                                selected
                            }
                        });
                        state.error = None;
                    }
                }
                Task::none()
            }
            Message::BlockCycleApply => self.apply_block_cycle_order(),
            Message::BlockActionBarSelect(handle) => self.handle_action_bar(handle, false),
            Message::BlockActionBarEdit(handle) => self.handle_action_bar(handle, true),
            Message::BlockActionBarMove(handle, offset) => {
                self.move_action_bar(handle, offset);
                Task::none()
            }
            Message::BlockActionBarHide(handle) => {
                let i = self.active_tab;
                if self.action_bar_error().is_none()
                    && self.tabs[i].active_cmd.is_none()
                    && self.tabs[i]
                        .active_block_edit_session()
                        .is_some_and(|session| {
                            crate::scene::block_action_bars::groups(
                                &self.tabs[i].scene.document,
                                session.br_handle,
                            )
                            .iter()
                            .any(|group| group.parameter == handle)
                        })
                {
                    self.tabs[i].action_bars.hidden.insert(handle);
                }
                Task::none()
            }
            Message::VisualStylesCreaseInput(value) => {
                if let Some(crease) = &mut self.visual_styles_crease {
                    *crease = value;
                }
                Task::none()
            }
            Message::VisualStylesCreaseApply => {
                let Some(value) = self.visual_styles_crease.clone() else {
                    return Task::none();
                };
                let task =
                    self.run_one_command_line(&format!("SETVAR VSEDGESMOOTH {}", value.trim()));
                // A refused value goes back to the one in force.
                self.visual_styles_crease = Some(self.model_space.edge_smooth.to_string());
                task
            }
            Message::VisualStylesOccludedEdges(on) => {
                self.run_one_command_line(&format!("SETVAR VSOCCLUDEDEDGES {}", u8::from(on)))
            }
            Message::BlockEditorSettingsInput(index, value) => {
                if let Some(state) = &mut self.block_editor_settings {
                    if let Some(field) = state.values.get_mut(index) {
                        *field = value;
                    }
                    state.error = None;
                }
                Task::none()
            }
            Message::BlockConstraintColorInput(index, value) => {
                if let Some(state) = &mut self.block_editor_settings {
                    if let Some(color) = state.constraint_colors.get_mut(index) {
                        *color = value;
                    }
                    state.error = None;
                }
                Task::none()
            }
            Message::ToggleBlockConstraintStatus => self.dispatch_command("BCONSTATUSMODE TOGGLE"),
            Message::BlockEditorSettingsReset => {
                if self.block_editor_settings.is_some() {
                    self.block_editor_settings =
                        Some(crate::ui::window::block_editor_settings::State::new(
                            &super::super::super::settings::BlockEditorPreferences::default(),
                        ));
                }
                Task::none()
            }
            Message::BlockEditorSettingsApply => {
                if self.active_modal == Some(super::super::super::ModalKind::BlockEditorSettings) {
                    if let Some(state) = &mut self.block_editor_settings {
                        let mut staged = self.block_editor_preferences.clone();
                        for ((name, _), value) in super::super::super::settings::BLOCK_DISPLAY_VARIABLES
                            .iter()
                            .zip(&state.values)
                        {
                            if let Err(error) = staged.set_display_variable(name, value) {
                                state.error = Some(format!("{name}: {}", crate::t!(error)));
                                return Task::none();
                            }
                        }
                        for (index, value) in state.constraint_colors.iter().enumerate() {
                            let normalized = crate::ui::overlay::authoring::normalize_color(value);
                            let Some(color) =
                                normalized.filter(|value| value != "BYLAYER" && value != "BYBLOCK")
                            else {
                                state.error = Some(crate::t!("Constraint colors require an index from 1 to 255 or RGB:r,g,b.").into_owned());
                                return Task::none();
                            };
                            staged.constraint_colors[index] = color;
                        }
                        self.block_editor_preferences = staged;
                        self.sync_block_constraint_colors();
                        self.refresh_properties();
                        self.persist_settings_if_changed();
                        self.block_editor_settings = None;
                        self.close_active_modal();
                    }
                }
                Task::none()
            }
            Message::NamedParametersInput { idx, field, value } => {
                use crate::ui::window::named_parameters::ParamField;
                if let Some(row) = self.named_parameter_editor_rows.get_mut(idx) {
                    match field {
                        ParamField::Name => row.name = value,
                        ParamField::Formula => row.formula = value,
                    }
                }
                Task::none()
            }
            Message::NamedParametersAdd => {
                self.named_parameter_editor_rows
                    .push(crate::ui::window::named_parameters::ParamEditorRow::default());
                Task::none()
            }
            Message::NamedParametersRemove(idx) => {
                if idx < self.named_parameter_editor_rows.len() {
                    self.named_parameter_editor_rows.remove(idx);
                }
                Task::none()
            }
            Message::NamedParametersApply => {
                self.apply_named_parameter_editor_rows();
                Task::none()
            }

            // ── Parameters / Constraints sections embedded in the
            // Properties panel ──────────────────────────────────────────────
            Message::PropParamInput {
                index,
                field,
                value,
            } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(crate::ui::properties::FieldKey::Param(index, field), value);
                Task::none()
            }
            Message::PropParamCommit { index, field } => self.on_prop_param_commit(index, field),
            Message::PropParamDelete(index) => self.on_prop_param_delete(index),
            Message::PropParamAddNew => self.on_prop_param_add_new(),
            Message::PropConstraintDelete(id) => {
                self.delete_parametric_constraint(id);
                Task::none()
            }
            Message::PropConstraintLinkClick(handles) => {
                let i = self.active_tab;
                self.tabs[i].scene.deselect_all();
                for h in handles {
                    self.tabs[i].scene.select_entity(h, false);
                }
                self.refresh_properties();
                Task::none()
            }
            // ── Options / About windows ───────────────────────────────────
            Message::OptionsOpen => {
                self.options_open();
                Task::none()
            }
            Message::OptionsApply => {
                self.options_apply();
                Task::none()
            }
            Message::OptionsRestoreDefaults => {
                self.options_restore_defaults();
                Task::none()
            }
            Message::OptionsOk => {
                self.options_apply();
                self.options_forget();
                self.close_active_modal();
                Task::none()
            }
            Message::OptionsClose => {
                if !self.options_close_confirm && self.options_dirty() {
                    self.options_close_confirm = true;
                    return Task::none();
                }
                self.options_discard();
                self.close_active_modal();
                Task::none()
            }
            Message::OptionsCloseDiscard => {
                self.options_discard();
                self.close_active_modal();
                Task::none()
            }
            Message::OptionsCloseKeep => {
                self.options_close_confirm = false;
                Task::none()
            }

            Message::OptionsTabChanged(tab) => {
                self.options_tab = tab;
                Task::none()
            }

            #[cfg(not(target_arch = "wasm32"))]
            Message::GraphicsBackendChanged(choice) => {
                self.graphics_prefs.backend = choice;
                crate::gpu_backend::save_prefs(self.graphics_prefs);
                Task::none()
            }

            #[cfg(not(target_arch = "wasm32"))]
            Message::GraphicsCompatToggled(on) => {
                self.graphics_prefs.compat_renderer = on;
                crate::gpu_backend::save_prefs(self.graphics_prefs);
                Task::none()
            }

            #[cfg(not(target_arch = "wasm32"))]
            Message::GraphicsLegacyGlToggled(on) => {
                self.graphics_prefs.legacy_gl = on;
                crate::gpu_backend::save_prefs(self.graphics_prefs);
                Task::none()
            }

            Message::CursorSizeChanged(value) => {
                self.cursor_size = value.clamp(1, 100);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::PickBoxChanged(value) => {
                self.pick_box = value.clamp(0, 50);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::ApertureChanged(value) => {
                self.snapper.osnap_radius_px = (value as f32).clamp(
                    crate::snap::MIN_OSNAP_RADIUS_PX,
                    crate::snap::MAX_OSNAP_RADIUS_PX,
                );
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::DoubleClickBlockRefeditChanged(enabled) => {
                self.double_click_block_refedit = enabled;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::DoubleClickBlockAtteditChanged(enabled) => {
                self.double_click_block_attedit = enabled;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::CursorTypeChanged(value) => {
                self.cursor_type = value;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::LineweightDisplayScaleChanged(value) => {
                self.lineweight_display_scale = value.clamp(25, 200);
                let scale = self.lineweight_display_scale as f32 / 100.0;
                for tab in &mut self.tabs {
                    tab.scene.model_lineweight_scale = scale;
                }
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::CrosshairColorChanged(value) => {
                self.crosshair_color_input = value.clone();
                if value.trim().is_empty() {
                    self.crosshair_color = None;
                    self.persist_settings_if_changed();
                } else if let Some(rgb) = crate::app::config::parse_hex(&value) {
                    self.crosshair_color = Some(rgb);
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::QnewTemplateChanged(path) => {
                self.qnew_template = path;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::TransmittalLayoutChanged(layout) => {
                self.transmittal_layout = layout;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::StartTemplates(open) => {
                self.start_templates = open.then(|| self.template_folder_files());
                if self.start_templates.as_ref().is_some_and(Vec::is_empty) {
                    // No folder of templates: ask for one the usual way.
                    self.start_templates = None;
                    return self.dispatch_command("NEW");
                }
                Task::none()
            }
            Message::NewFromTemplate(Some(path)) => {
                self.start_templates = None;
                let task = self.update(Message::TabNew);
                let idx = self.active_tab;
                match self.apply_template(&path.to_string_lossy()) {
                    Ok(_) => {
                        self.tabs[idx].dirty = false;
                        self.refresh_layer_panel();
                        self.sync_ribbon_styles();
                    }
                    Err(error) => self
                        .command_line
                        .push_error(error["error"].as_str().unwrap_or("template")),
                }
                task
            }
            Message::NewFromTemplate(None) => Task::none(),
            Message::QnewTemplateBrowse => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .add_filter(
                            crate::t!("Drawing Template (*.dwt)").as_ref(),
                            &["dwt", "DWT", "dwg", "DWG", "dxf", "DXF"],
                        )
                        .pick_file()
                        .await
                        .map(|handle| crate::sys::handle_path(&handle).display().to_string())
                },
                |path| match path {
                    Some(path) => Message::QnewTemplateChanged(path),
                    None => Message::Noop,
                },
            ),
            Message::CommandTextColorChanged(value) => {
                self.command_text_color_input = value.clone();
                let color = if value.trim().is_empty() {
                    None
                } else if let Some(rgb) = crate::app::config::parse_hex(&value) {
                    Some(rgb)
                } else {
                    return Task::none();
                };
                self.command_text_color = color;
                crate::ui::command_line::set_text_color(color);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::SnapMarkerColorChanged(value) => {
                self.snap_marker_color_input = value.clone();
                if value.trim().is_empty() {
                    self.snap_marker_color = None;
                    self.persist_settings_if_changed();
                } else if let Some(rgb) = crate::app::config::parse_hex(&value) {
                    self.snap_marker_color = Some(rgb);
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::DefaultSaveFormatChanged(format) => {
                self.default_save_format = crate::io::canonical_save_format(&format).to_string();
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::OptionsThemeChanged(name) => {
                if self.ui_theme.name == "Custom" && name != "Custom" {
                    self.saved_custom_palette = Some(self.ui_theme.palette);
                }
                self.ui_theme.name = name;
                if let Some(theme) = crate::app::config::builtin_theme(&self.ui_theme.name) {
                    self.ui_theme.palette =
                        crate::app::config::UiThemePalette::from_iced(theme.seed());
                    self.theme_color_inputs = self.ui_theme.palette.hex_values();
                    self.active_theme = theme;
                } else {
                    self.ui_theme.name = "Custom".to_string();
                    if let Some(saved) = self.saved_custom_palette {
                        self.ui_theme.palette = saved;
                        self.theme_color_inputs = saved.hex_values();
                    }
                    self.active_theme = self.ui_theme.to_iced();
                }
                self.sync_model_space_theme(true);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::OptionsThemeColorChanged(index, value) => {
                if index >= self.theme_color_inputs.len() {
                    return Task::none();
                }
                self.theme_color_inputs[index] = value.clone();
                if self.ui_theme.palette.set_hex(index, &value) {
                    self.ui_theme.name = "Custom".to_string();
                    self.active_theme = self.ui_theme.to_iced();
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::ModelSpaceModeChanged(mode) => {
                self.model_space.mode = mode;
                self.sync_model_space_theme(true);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::ModelSpaceBgChanged(hex) => {
                self.model_bg_input = hex.clone();
                if hex.trim().is_empty() {
                    self.model_space.custom_bg = None;
                    self.model_space.mode = crate::app::config::ModelSpaceMode::MatchTheme;
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                } else if let Some(rgb) = crate::app::config::parse_hex(&hex) {
                    self.model_space.custom_bg = Some(rgb);
                    self.model_space.mode = crate::app::config::ModelSpaceMode::Custom;
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::PaperSpaceBgChanged(hex) => {
                self.paper_bg_input = hex.clone();
                if hex.trim().is_empty() {
                    self.model_space.custom_paper_bg = None;
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                } else if let Some(rgb) = crate::app::config::parse_hex(&hex) {
                    self.model_space.custom_paper_bg = Some(rgb);
                    self.sync_model_space_theme(true);
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::DeskSpaceBgChanged(hex) => {
                self.desk_bg_input = hex.clone();
                if hex.trim().is_empty() {
                    self.model_space.custom_desk_bg = None;
                    self.sync_model_space_theme(false);
                    self.persist_settings_if_changed();
                } else if let Some(rgb) = crate::app::config::parse_hex(&hex) {
                    self.model_space.custom_desk_bg = Some(rgb);
                    self.sync_model_space_theme(false);
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::GridOpacityChanged(opacity) => {
                self.model_space.grid_opacity = opacity.clamp(5, 100);
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionAreaToggled(enabled) => {
                self.model_space.selection_area = enabled;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionOpacityChanged(opacity) => {
                self.model_space.selection_opacity = opacity.clamp(0, 100);
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionWindowColorChanged(color) => {
                self.model_space.selection_window_color = color;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionCrossingColorChanged(color) => {
                self.model_space.selection_crossing_color = color;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionHighlightColorChanged(color) => {
                self.model_space.selection_highlight_color = color;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionHighlightOpacityChanged(opacity) => {
                self.model_space.selection_highlight_opacity = opacity.clamp(10, 100);
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionHighlightWidenChanged(widen) => {
                self.model_space.selection_highlight_widen = widen.min(6);
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::GripSizeChanged(size) => {
                self.model_space.grip_size = size.clamp(1, 25);
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SnapMarkerSizeChanged(size) => {
                self.model_space.snap_marker_size = size.clamp(1, 20);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::AutoSnapMagnetChanged(on) => {
                self.autosnap_magnet = on;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::GripColorChanged(color) => {
                self.model_space.grip_color = color;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::GripHotChanged(color) => {
                self.model_space.grip_hot = color;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::GripHoverChanged(color) => {
                self.model_space.grip_hover = color;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::RestoreModelSpaceDisplayDefaults => {
                self.model_space.mode = crate::app::config::ModelSpaceMode::MatchTheme;
                self.model_space.custom_bg = None;
                self.model_space.custom_paper_bg = None;
                self.model_space.custom_desk_bg = None;
                self.model_space.grid_opacity = 18;
                self.model_bg_input.clear();
                self.paper_bg_input.clear();
                self.desk_bg_input.clear();
                self.sync_model_space_theme(true);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::GripObjectLimitChanged(limit) => {
                self.grip_object_limit = limit.clamp(0, 32767);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionEffectToggled(enabled) => {
                self.model_space.selection_effect = enabled;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            // SELECTIONPREVIEW is a bitmask and the two checkboxes own one bit
            // each: 1 = rollover while idle, 2 = rollover during a command.
            Message::SelectionPreviewIdleToggled(enabled) => {
                self.model_space.selection_preview =
                    set_preview_bit(self.model_space.selection_preview, 1, enabled);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SelectionPreviewCommandToggled(enabled) => {
                self.model_space.selection_preview =
                    set_preview_bit(self.model_space.selection_preview, 2, enabled);
                self.persist_settings_if_changed();
                Task::none()
            }

            // "Use Shift to add" is the inverse of PICKADD.
            Message::ShiftToAddToggled(shift_to_add) => {
                self.pick_add = !shift_to_add;
                self.pick_add_retains = !shift_to_add;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::SaveTimeChanged(minutes) => {
                self.savetime_min = minutes.max(0);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::PageSetupImportFile(path) => self.on_page_setup_import_file(path),
            Message::PageSetupOnNewLayoutChanged(enabled) => {
                self.plot_dialog.page_setup_on_new_layout = enabled;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::BackupOnSaveChanged(enabled) => {
                self.backup_on_save = enabled;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::CheckMissingFontsChanged(enabled) => {
                self.check_missing_fonts = enabled;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::TextFillChanged(filled) => {
                crate::scene::text::sdf_atlas::set_textfill(filled);
                self.invalidate_text_everywhere();
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::ClipromptLinesChanged(lines) => {
                let lines = crate::app::settings::clamp_clipromptlines(lines);
                self.cliprompt_lines = lines;
                self.command_line
                    .set_cliprompt_lines(lines.clamp(0, 50) as u8);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::CommandLineOpacityChanged(percent) => {
                self.command_line.set_opacity_percent(percent);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::CommandLineFontChanged(family) => {
                let size = self.command_line.font_size();
                self.command_line.set_font(&family, size);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::CommandLineFontSizeChanged(size) => {
                let family = self.command_line.font_family().to_owned();
                self.command_line.set_font(&family, size);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::CommandLineFadeChanged(ms) => {
                let ms = crate::app::settings::clamp_commandline_fade_ms(ms);
                self.commandline_fade_ms = ms;
                self.command_line.set_commandline_fade_ms(ms.max(0) as u32);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::ZoomWheelReversedChanged(reversed) => {
                self.zoom_wheel_reversed = reversed;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::ToggleDynamicUcs => {
                self.dynamic_ucs = !self.dynamic_ucs;
                self.command_line.push_output(
                    if self.dynamic_ucs {
                        crate::t!("Dynamic UCS: on")
                    } else {
                        crate::t!("Dynamic UCS: off")
                    }
                    .as_ref(),
                );
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::NavigationStyleChanged(style) => {
                self.navigation_style = style;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::MenuBarChanged(shown) => {
                self.show_menu_bar = shown;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::TitleBar(msg) => self.on_title_bar(msg),

            Message::RibbonVisibleChanged(shown) => {
                self.ribbon_closed = !shown;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::RibbonCenteredChanged(centered) => {
                self.ribbon.set_centered(centered);
                self.save_config();
                Task::none()
            }

            Message::RibbonWheelTabsChanged(on) => {
                self.ribbon.set_wheel_switches_tabs(on);
                self.save_config();
                Task::none()
            }

            Message::HatchAssociativeChanged(associative) => {
                self.hatch_associative = associative;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::TouchpadPanChanged(enabled) => {
                self.touchpad_pan = enabled;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::KeypadDecimalPointChanged(enabled) => {
                crate::app::view::set_keypad_decimal_point(enabled);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::UiScaleChanged(percent) => {
                self.ui_scale_percent = crate::app::settings::clamp_ui_scale(percent);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::ZoomFactorChanged(factor) => {
                self.zoom_factor = crate::app::settings::clamp_zoom_factor(factor);
                // The field beside the slider reads the same value, so the
                // two never disagree about what the wheel is set to.
                self.zoom_factor_input = self.zoom_factor.to_string();
                self.persist_settings_if_changed();
                Task::none()
            }

            // Typed into freely; only committed when it parses, so clearing
            // the field to retype does not drop the wheel to its minimum.
            // This is the surface that reaches past the range the system
            // variable has.
            Message::ZoomFactorInputChanged(value) => {
                self.zoom_factor_input = value;
                if let Ok(factor) = self.zoom_factor_input.trim().parse::<i32>() {
                    self.zoom_factor = crate::app::settings::clamp_zoom_factor(factor);
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::RightClickModeChanged(mode) => {
                self.right_click_mode = mode;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::RightClickHoldMsChanged(ms) => {
                self.right_click_hold_ms = super::super::super::settings::clamp_right_click_hold_ms(ms);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::TextEditModeChanged(single) => {
                self.texteditmode = single;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::DimContinueModeChanged(inherit) => {
                self.dimension_continue_mode = i16::from(inherit);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::QdimSnapPriorityChanged(priority) => {
                self.quick_dimension_snap_priority = priority.min(1);
                self.persist_settings_if_changed();
                Task::none()
            }

            // The sign carries on/off and the magnitude the mode, so switching
            // off and back on has to return to the mode that was chosen — the
            // same convention the status-bar pill uses when it negates.
            Message::AnnoAutoScaleChanged(mode) => {
                self.annotation_auto_scale = if mode == 0 {
                    -self.annotation_auto_scale.abs().max(1)
                } else {
                    mode.clamp(1, 4)
                };
                self.persist_settings_if_changed();
                Task::none()
            }

            // Typed into freely; only committed when it parses, so clearing
            // the field to retype does not snap the crosshair back to zero.
            Message::SnapAngleInputChanged(value) => {
                self.snap_angle_input = value;
                if let Ok(angle) = self.snap_angle_input.trim().parse::<f32>() {
                    if angle.is_finite() {
                        self.snap_angle_deg = angle.rem_euclid(360.0);
                        self.persist_settings_if_changed();
                    }
                }
                Task::none()
            }

            Message::PolarIncrementChanged(deg) => {
                if deg.is_finite() && deg > 0.0 {
                    self.polar_increment_deg = deg;
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::ShowViewCubeChanged(show) => {
                self.show_viewcube = show;
                self.ribbon.set_viewcube(show);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::ShowUcsIconChanged(show) => {
                self.show_ucs_icon = show;
                self.ribbon.set_ucs_icon(show);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::UcsIconAtOriginChanged(at_origin) => {
                self.ucs_icon_at_origin = at_origin;
                self.persist_settings_if_changed();
                Task::none()
            }

            // ISOLINES is baked into solid meshes, so rebuild once on release.
            Message::IsolinesChanged(value) => {
                let value = value.max(0);
                let changed = self
                    .tabs
                    .get(self.active_tab)
                    .is_some_and(|tab| tab.scene.document.header.isolines != value);
                if changed {
                    self.set_drawing_var(|header| header.isolines = value);
                    self.isolines_awaiting_regen = true;
                }
                Task::none()
            }

            Message::IsolinesReleased => {
                if std::mem::take(&mut self.isolines_awaiting_regen) {
                    self.regenerate_meshes();
                }
                Task::none()
            }

            Message::DispSilhChanged(on) => {
                self.set_drawing_var(|header| header.display_silhouette = on);
                Task::none()
            }

            Message::SurfaceUChanged(value) => {
                self.set_drawing_var(|header| header.surface_u_density = value.clamp(0, 200));
                Task::none()
            }

            Message::SurfaceVChanged(value) => {
                self.set_drawing_var(|header| header.surface_v_density = value.clamp(0, 200));
                Task::none()
            }

            Message::SurfaceTypeChanged(value) => {
                self.set_drawing_var(|header| header.surface_type = value);
                Task::none()
            }

            Message::SolidHistChanged(record) => {
                self.set_drawing_var(|header| header.record_solid_history = record);
                Task::none()
            }

            Message::ShowHistChanged(mode) => {
                self.set_drawing_var(|header| header.show_solid_history = mode.clamp(0, 2));
                let i = self.active_tab;
                if let Some(tab) = self.tabs.get_mut(i) {
                    tab.scene.bump_geometry();
                }
                Task::none()
            }

            Message::SelectionCyclingChanged(on) => {
                self.toggle_selection_cycling(on);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::OpenFolder(path) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // The fonts folder exists only once something is put in it.
                    let _ = std::fs::create_dir_all(&path);
                    crate::sys::open_local_path(std::path::Path::new(&path), None)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = path;
                    Task::none()
                }
            }

            #[cfg(target_arch = "wasm32")]
            Message::ChooseFolder(_) | Message::FolderChosen(..) => Task::none(),
            #[cfg(not(target_arch = "wasm32"))]
            Message::ChooseFolder(folder) => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .pick_folder()
                        .await
                        .map(|handle| crate::sys::handle_path(&handle))
                },
                move |picked| match picked {
                    Some(path) => Message::FolderChosen(folder, Some(path)),
                    None => Message::Noop,
                },
            ),
            #[cfg(not(target_arch = "wasm32"))]
            Message::FolderChosen(folder, path) => {
                use crate::ui::window::options::MovableFolder;
                match folder {
                    MovableFolder::Fonts => crate::io::font_repo::set_fonts_folder(path),
                    MovableFolder::PlotStyles => {
                        crate::io::plot_style::set_plot_styles_folder(path)
                    }
                    MovableFolder::Support => crate::io::font_repo::set_support_folder(path),
                }
                self.save_config();
                Task::none()
            }

            Message::PickDragModeChanged(mode) => {
                self.pick_drag_mode = mode;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::PickAutoBitToggled(bit, checked) => {
                self.pick_auto = (self.pick_auto & !bit) | if checked { bit } else { 0 };
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::PickFirstToggled(enabled) => {
                self.set_pick_first(enabled);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::RestoreSelectionVisualDefaults => {
                self.model_space.selection_area = true;
                self.model_space.selection_opacity = 12;
                self.model_space.selection_window_color = 0;
                self.model_space.selection_crossing_color = 0;
                self.model_space.selection_highlight_color = 0;
                self.model_space.selection_effect = true;
                self.model_space.selection_preview = 3;
                self.model_space.selection_preview_limit = 20_000;
                self.model_space.grip_size = 5;
                self.model_space.grip_color = 0;
                self.model_space.grip_hot = 0;
                self.model_space.grip_hover = 0;
                // The button restores the *visual* defaults, which is why the
                // grip limit is here and PICKADD / PICKDRAG are not — those are
                // how selection behaves, not how it looks.
                self.grip_object_limit = crate::app::settings::DEFAULT_GRIP_OBJECT_LIMIT;
                self.sync_model_space_theme(false);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::FileAssocChanged(enabled) => {
                // The same two calls FILEASSOC makes, so the checkbox and the
                // command cannot leave the setting and the registration
                // disagreeing.
                self.file_assoc_enabled = enabled;
                self.persist_settings_if_changed();
                let outcome = if enabled {
                    crate::io::file_association::register_as_handler()
                } else {
                    crate::io::file_association::unregister_handler()
                };
                match outcome {
                    Ok(()) => {
                        let said = if enabled {
                            crate::t!("Registered as a .dwg/.dxf handler.")
                        } else {
                            crate::t!("No longer registered as a file handler.")
                        };
                        self.command_line.push_output(said.as_ref());
                    }
                    Err(why) => {
                        // The setting is what the user asked for; the
                        // registration is what the system allowed. Put the
                        // checkbox back rather than showing a state that is not
                        // true.
                        self.file_assoc_enabled = !enabled;
                        self.persist_settings_if_changed();
                        self.command_line
                            .push_error(crate::tf!("File association failed: {why}").as_ref());
                    }
                }
                Task::none()
            }
            Message::ShowConstraintValuesChanged(enabled) => {
                self.show_constraint_values = enabled;
                self.persist_settings_if_changed();
                // The no-selection Properties page caches this flag in its
                // On/Off row; rebuild it so the button shows the new state
                // and its next press sends the opposite value.
                self.refresh_properties();
                Task::none()
            }
            Message::LanguageChanged(language) => {
                if self.language == language {
                    return Task::none();
                }
                match crate::i18n::set_language(language) {
                    Ok(()) => {
                        self.language = language;
                        self.persist_settings_if_changed();
                        // Ribbon labels are built in the language they were
                        // made in; rebuild them so no restart is needed.
                        self.rebuild_ribbon_modules();
                        #[cfg(target_arch = "wasm32")]
                        {
                            let script = crate::scene::text::web_font::preload_language(
                                &crate::i18n::active_language_tag(),
                            );
                            return Task::batch([
                                Task::done(Message::PollWebFonts),
                                Task::done(Message::ApplyWebFont(script)),
                            ]);
                        }
                    }
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("Unable to change UI language: {error}").as_ref()),
                }
                Task::none()
            }

            Message::AboutOpen => {
                self.active_modal = Some(super::super::super::ModalKind::About);
                Task::none()
            }

            Message::GpuWarningOpen => {
                self.active_modal = Some(super::super::super::ModalKind::GpuWarning);
                Task::none()
            }
            Message::GpuWarningSilence => {
                self.gpu_warning_silenced = self.gpu_status.identity().unwrap_or_default();
                self.close_active_modal();
                Task::none()
            }

            // ✕ on the outdated extraction table window leaves every table
            // as it is; the event goes on.
            Message::CloseModal
                if self.active_modal == Some(super::super::super::ModalKind::ExtractionOutdated) =>
            {
                self.on_extraction_outdated(
                    crate::ui::window::extraction_outdated::OutdatedChoice::SkipAll,
                )
            }
            Message::CloseModal => {
                // Cancel or ✕ in the multiple-material window of a 3D Studio
                // import stops the import at the asked mesh; the meshes
                // before it are converted off the UI thread.
                if self.active_modal == Some(super::super::super::ModalKind::TdsMaterial) {
                    if let Some(state) = self.import_dialogs.tds_material.take() {
                        self.close_active_modal();
                        return self.stop_tds_import(state);
                    }
                }
                // The multileader style window's ✕ closes its topmost face.
                if self.active_modal == Some(super::super::super::ModalKind::MLeaderStyle)
                    && self.mleader_style_close_face()
                {
                    return Task::none();
                }
                if self.active_modal == Some(super::super::super::ModalKind::DimStyle)
                    && self.dim_style_close_face()
                {
                    return Task::none();
                }
                // The text style window's ✕ closes a card first, and asks
                // before dropping changes Apply has not written.
                if self.active_modal == Some(super::super::super::ModalKind::TextStyle)
                    && self.style_parent_stage.is_none()
                {
                    if self.textstyle_card.is_some() {
                        return self.on_text_style_card(
                            crate::ui::style::textstyle::CardMsg::NoticeClose,
                        );
                    }
                    if self.textstyle_modified() {
                        self.textstyle_card = Some(crate::ui::style::textstyle::Card::Modified(
                            crate::ui::style::textstyle::Pending::Close,
                        ));
                        return Task::none();
                    }
                }
                if self.active_modal == Some(super::super::super::ModalKind::BlockLookup) {
                    self.block_lookup = None;
                }
                if self.active_modal == Some(super::super::super::ModalKind::BlockPropertiesTable) {
                    self.block_properties_table = None;
                }
                if self.active_modal == Some(super::super::super::ModalKind::BlockVisibility) {
                    self.block_visibility = None;
                }
                // The Invalid Area dialog's ✕ undoes the boundary change, as its Cancel does.
                if self.active_modal == Some(super::super::super::ModalKind::CountInvalidArea) {
                    return self.update(Message::Count(
                        crate::ui::window::count_palette::CountMsg::InvalidUndo,
                    ));
                }
                if self.active_modal == Some(super::super::super::ModalKind::SheetSet) {
                    self.close_sheet_set_dialog();
                    return Task::none();
                }
                // The Background dialog returns to -VIEW's settings prompt.
                if self.active_modal == Some(super::super::super::ModalKind::ViewBackground) {
                    self.view_background = None;
                    self.close_active_modal();
                    let i = self.active_tab;
                    return self.run_dash_view("SETTINGS", i);
                }
                if self.active_modal == Some(super::super::super::ModalKind::ImageAdjust) {
                    self.image_adjust = None;
                }
                if self.active_modal == Some(super::super::super::ModalKind::PluginDialog) {
                    return self.update(Message::PluginDialogCancel);
                }
                if self.active_modal == Some(super::super::super::ModalKind::Field) {
                    self.close_field_dialog();
                    return Task::none();
                }
                // Closing the attribute manager is its Cancel: live changes
                // are taken back.
                if self.active_modal == Some(super::super::super::ModalKind::AttMan) {
                    return self.on_attman(crate::ui::window::attman_dialog::AttManMsg::Cancel);
                }
                if matches!(
                    self.active_modal,
                    Some(super::super::super::ModalKind::AttDef | super::super::super::ModalKind::AttDefEdit)
                ) {
                    self.attdef_dialog = None;
                    let editing = self.attdef_edit.take().is_some();
                    self.close_active_modal();
                    return if editing {
                        self.post_editor_closed(false)
                    } else {
                        Task::none()
                    };
                }
                if self.active_modal == Some(super::super::super::ModalKind::InsertAttributes) {
                    let i = self.active_tab;
                    // ATTEDIT's Cancel leaves the values as they were.
                    if self
                        .insert_attr_dialog
                        .as_ref()
                        .is_some_and(|d| d.edit.is_some())
                    {
                        self.insert_attr_dialog = None;
                        self.close_active_modal();
                        return Task::none();
                    }
                    if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.attreq_cancel_keeps_defaults())
                    {
                        return self.keep_insert_attr_defaults();
                    }
                    self.cancel_insert_attr_dialog();
                    self.close_active_modal();
                    return Task::none();
                }
                if self.active_modal == Some(super::super::super::ModalKind::RefEdit) {
                    self.refedit_dialog = None;
                }
                if self.active_modal == Some(super::super::super::ModalKind::PdfPassword) {
                    return self.cancel_pdf_password();
                }
                // Cancel returns to PDFSHXTEXT's selection prompt.
                if self.active_modal == Some(super::super::super::ModalKind::PdfShxSettings) {
                    self.pdf_shx_dialog = None;
                    self.close_active_modal();
                    let i = self.active_tab;
                    self.start_pdf_shx_gather(i);
                    return self.focus_cmd_input();
                }
                // Cancelling the section settings drops the waiting extraction.
                if self.active_modal == Some(super::super::super::ModalKind::PcSection) {
                    crate::modules::insert::pc_extract::drop_job();
                    self.pc_section = None;
                }
                if self.active_modal == Some(super::super::super::ModalKind::RecoveryPrompt) {
                    return self.update(Message::RecoveryDecline);
                }
                // The title-bar X, Escape, and MCP `close_modal` action mean
                // the same thing as Skip for this prompt. Record that choice
                // so reopening the drawing does not immediately nag again.
                if self.active_modal == Some(super::super::super::ModalKind::MissingFonts) {
                    return self.update(Message::MissingFontsDismiss);
                }
                // The Options window's × and Esc behave like its Close button.
                if self.active_modal == Some(super::super::super::ModalKind::Options) {
                    return self.update(Message::OptionsClose);
                }
                // Closing the crash-recovery prompt (✕ or Escape) DEFERS:
                // one stray keystroke must not delete the only recovery
                // copy, so the snapshot survives and is re-offered on the
                // next boot. Only the Discard button deletes.
                #[cfg(target_arch = "wasm32")]
                if self.active_modal == Some(super::super::super::ModalKind::AutosaveRestore) {
                    return self.update(Message::WebAutosaveDefer);
                }
                // Closing the shortcut editor with un-applied rows needs an
                // explicit discard; a second close attempt (or the overlay's
                // Discard button) goes through.
                if self.active_modal == Some(super::super::super::ModalKind::Shortcuts)
                    && !self.shortcut_close_confirm
                    && self.shortcut_editor_dirty()
                {
                    self.shortcut_close_confirm = true;
                    return Task::none();
                }
                self.shortcut_close_confirm = false;
                if self.active_modal == Some(super::super::super::ModalKind::Aliases)
                    && !self.alias_close_confirm
                    && self.alias_editor_dirty()
                {
                    self.alias_close_confirm = true;
                    return Task::none();
                }
                self.alias_close_confirm = false;
                if self.active_modal == Some(super::super::super::ModalKind::DraftingSettings)
                    && !self.drafting_settings_close_confirm
                    && self.drafting_settings_dirty()
                {
                    self.drafting_settings_close_confirm = true;
                    return Task::none();
                }
                self.drafting_settings_close_confirm = false;
                if self.active_modal == Some(super::super::super::ModalKind::AutoConstrainSettings) {
                    if let Some(saved) = self.auto_constrain_saved.take() {
                        self.auto_constrain_settings = saved;
                    }
                }
                let resume_open_queue = self.active_modal == Some(super::super::super::ModalKind::Recovery);
                self.close_active_modal();
                if resume_open_queue {
                    self.drain_pending_open()
                } else {
                    Task::none()
                }
            }
            Message::RecoveryClose => {
                self.close_active_modal();
                self.drain_pending_open()
            }
            Message::RecoveryAttempt => {
                let open_id = self.next_open_id();
                let Some(opening) = self.opening.as_mut() else {
                    self.close_active_modal();
                    return Task::none();
                };
                let model_bg = self
                    .default_bg_color
                    .unwrap_or_else(|| self.model_space.resolve_model_bg(&self.active_theme));
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(path) = opening.source_path.clone() {
                    let current_fingerprint =
                        crate::io::edit_lock::FileFingerprint::capture(&path).ok();
                    if current_fingerprint.as_ref() != opening.fingerprint.as_ref() {
                        let progress = std::sync::Arc::new(crate::io::OpenProgressState::new(
                            super::super::super::OPEN_PHASE_READING,
                        ));
                        opening.id = open_id;
                        opening.state = progress.clone();
                        opening.started = Instant::now();
                        opening.recovery_error = None;
                        opening.recovery_read_stats = None;
                        opening.fingerprint = current_fingerprint;
                        opening.size_bytes = std::fs::metadata(&path)
                            .map(|metadata| metadata.len())
                            .unwrap_or(0);
                        self.close_active_modal();
                        return Task::perform(
                            crate::io::open_path_with_phase(path, progress, model_bg),
                            move |result| Message::FileOpened(open_id, result),
                        );
                    }
                }
                let Some(initial_error) = opening.recovery_error.take() else {
                    self.close_active_modal();
                    return Task::none();
                };
                let initial_stats = opening.recovery_read_stats.take();
                let Some(path) = opening.source_path.clone() else {
                    // Nothing on disk to read again: the open fails with the
                    // strict read's error.
                    self.opening = None;
                    let e = initial_error;
                    self.command_line
                        .push_error(crate::tf!("Open failed: {e}").as_ref());
                    self.close_active_modal();
                    return self.drain_pending_open();
                };
                let progress = std::sync::Arc::new(crate::io::OpenProgressState::new(
                    super::super::super::OPEN_PHASE_READING,
                ));
                opening.id = open_id;
                opening.state = progress.clone();
                opening.started = Instant::now();
                #[cfg(target_arch = "wasm32")]
                let recovery_bytes = opening.recovery_bytes.take();
                self.close_active_modal();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(
                        crate::io::recover_path_with_phase(
                            path,
                            progress,
                            model_bg,
                            initial_error,
                            initial_stats,
                        ),
                        move |result| Message::FileOpened(open_id, result),
                    )
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = model_bg;
                    let Some(bytes) = recovery_bytes else {
                        self.opening = None;
                        return self.drain_pending_open();
                    };
                    Task::perform(
                        crate::io::recover_web_bytes(
                            path.to_string_lossy().into_owned(),
                            bytes,
                            progress,
                            initial_error,
                            initial_stats,
                        ),
                        move |outcome| Message::WebFileOpened(open_id, outcome),
                    )
                }
            }
            Message::RecoveryDecline => {
                let declined = self.opening.take();
                self.close_active_modal();
                if let Some(opening) = declined {
                    self.command_line
                        .push_info(crate::tf!("Recovery cancelled: \"{}\"", opening.name).as_ref());
                }
                self.drain_pending_open()
            }
            Message::RecoverySaveAs => {
                if !self.pending_opens.is_empty() {
                    self.close_active_modal();
                    return self.drain_pending_open();
                }
                let Some(tab_id) = self
                    .recovery_report
                    .as_ref()
                    .and_then(|report| report.tab_id)
                else {
                    self.close_active_modal();
                    return Task::none();
                };
                let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                    self.close_active_modal();
                    return Task::none();
                };
                self.close_active_modal();
                self.active_tab = i;
                self.open_save_dialog_window(i)
            }
            Message::RecoveryShowLog => {
                let Some(report) = self.recovery_report.as_ref() else {
                    return Task::none();
                };
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if let Some(path) = &report.log_path {
                        if let Err(error) = crate::sys::reveal_in_file_manager(path) {
                            self.command_line.push_error(
                                crate::tf!("Could not show recovery log: {error}").as_ref(),
                            );
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let name = report.suggested_download_name();
                    let body = report.log_text();
                    crate::sys::download_bytes(&name, body.as_bytes());
                }
                Task::none()
            }
            Message::AttrEditorOpen(handle) => {
                self.open_attribute_editor(handle);
                Task::none()
            }
            Message::AttrEditorTab(t) => {
                self.attr_editor_ui.tab = t;
                Task::none()
            }
            Message::AttrEditorSelect(idx) => {
                if idx < self.attr_editor_ui.rows.len() {
                    self.attr_editor_ui.selected = idx;
                }
                Task::none()
            }
            Message::AttrEditorInput { idx, value } => {
                if let Some(r) = self.attr_editor_ui.rows.get_mut(idx) {
                    r.value = value;
                    // Retyping a picked field drops it.
                    if r.field.take().is_some() {
                        r.field_changed = false;
                    }
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorTextStyle(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.text_style = s;
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorJustify(label) => {
                if let Some((h, v)) =
                    crate::ui::window::attribute_editor::justify_from_label(&label)
                {
                    if let Some(r) = self.attr_row_selected_mut() {
                        r.h_align = h;
                        r.v_align = v;
                    }
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorHeight(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.height = s;
                }
                self.attr_editor_ui.editing =
                    Some(crate::ui::window::attribute_editor::AttrField::Height);
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorRotation(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.rotation = s;
                }
                self.attr_editor_ui.editing =
                    Some(crate::ui::window::attribute_editor::AttrField::Rotation);
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorWidth(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.width_factor = s;
                }
                self.attr_editor_ui.editing =
                    Some(crate::ui::window::attribute_editor::AttrField::Width);
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorOblique(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.oblique = s;
                }
                self.attr_editor_ui.editing =
                    Some(crate::ui::window::attribute_editor::AttrField::Oblique);
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorBackwards(b) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.backwards = b;
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorUpsideDown(b) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.upside_down = b;
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorLayer(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.layer = s;
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorLinetype(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.linetype = if s == "ByLayer" { String::new() } else { s };
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorColor(label) => {
                if label == crate::ui::window::attribute_editor::SELECT_COLOR {
                    let current = self
                        .attr_editor_ui.rows
                        .get(self.attr_editor_ui.selected)
                        .map(|r| r.color)
                        .unwrap_or(document::types::Color::ByLayer);
                    return self.update(Message::OpenColorWindow(
                        crate::app::ColorPickTarget::AttrEditor,
                        current,
                    ));
                }
                if let Some(c) = crate::ui::window::attribute_editor::color_from_label(&label) {
                    if let Some(r) = self.attr_row_selected_mut() {
                        r.color = c;
                    }
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorLineweight(lw) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.line_weight = lw;
                }
                self.attr_editor_preview();
                Task::none()
            }
            Message::AttrEditorApply
            | Message::AttrEditorOk
            | Message::AttrEditorCancel
            | Message::AttrEditorHelp
            | Message::AttrEditorSelectBlock
            | Message::AttrEditorAnswer(_)
            | Message::AttrEditorDismissError
            | Message::AttrEditorEditValue
            | Message::AttrEditorInsertField
            | Message::AttrEditorBoundary(_)
            | Message::AttrEditorAnnotative(_)
            | Message::AttrEditorColorValue(_) => self.on_attr_editor_msg(msg),
            Message::ToolWindow(kind, inner) => {
                // Working in a dialog gives it the keyboard; its own layout
                // pass measuring it does not.
                if !matches!(*inner, Message::ModalContentResized(_)) {
                    self.keyboard_tool = Some(kind);
                }
                self.on_tool_window_msg(kind, *inner)
            }
            Message::ToolWindowTask(kind, inner) => self.on_tool_window_task(kind, *inner),
            Message::ToolWindowGrab(kind) => {
                self.keyboard_tool = Some(kind);
                self.on_tool_grab(kind, false)
            }
            Message::ToolWindowResizeGrab(kind) => self.on_tool_grab(kind, true),
            Message::ToolWindowDragMove(point) => self.on_tool_drag_move(point),
            Message::ToolWindowDragRelease => self.on_tool_drag_release(),
            Message::ToolWindowRaise(kind) => {
                self.keyboard_tool = Some(kind);
                self.raise_tool(kind)
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::OsWindowReturn(id) => self.return_os_window(id),
            #[cfg(not(target_arch = "wasm32"))]
            Message::OsWindowKey(id, key) => self.os_window_key(id, key),
            #[cfg(not(target_arch = "wasm32"))]
            Message::OsWindowResized(id, w, h) => {
                self.os_window_resized(id, w, h);
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::OsWindowMoved(id, at) => {
                self.os_window_moved(id, at);
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::OsWindowScale(id, scale) => self.os_window_scale(id, scale),
            #[cfg(target_arch = "wasm32")]
            Message::OsWindowReturn(_)
            | Message::OsWindowKey(..)
            | Message::OsWindowResized(..)
            | Message::OsWindowMoved(..)
            | Message::OsWindowScale(..) => Task::none(),
            Message::MTextToolbarGrab => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.toolbar_dragging = true;
                    ed.toolbar_drag_last = None;
                }
                Task::none()
            }
            Message::MTextToolbarMove(p) => {
                if let Some(ed) = self.mtext_editor.as_mut().filter(|ed| ed.toolbar_dragging) {
                    if let Some(last) = ed.toolbar_drag_last {
                        ed.toolbar_offset += p - last;
                    }
                    ed.toolbar_drag_last = Some(p);
                }
                Task::none()
            }
            Message::MTextToolbarRelease => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.toolbar_dragging = false;
                    ed.toolbar_drag_last = None;
                }
                Task::none()
            }
            Message::ModalGrab => {
                // Start a drag; the first ModalDragMove seeds the reference.
                self.modal_dragging = true;
                self.modal_drag_last = None;
                Task::none()
            }
            Message::ModalResizeGrab => {
                // Start a resize; the first ModalDragMove seeds the reference.
                self.modal_resizing = true;
                self.modal_drag_last = None;
                Task::none()
            }
            Message::ModalContentResized(size) => {
                if !size.width.is_finite()
                    || !size.height.is_finite()
                    || size.width <= 0.0
                    || size.height <= 0.0
                {
                    return Task::none();
                }
                let first_measurement = self.modal_content_size.replace(size).is_none();
                self.mark_startup_modal_shown();
                if first_measurement {
                    let initial_width = self.mtext_editor.as_ref().and_then(|editor| {
                        (editor.editing.is_none() && !editor.keep_width).then(|| {
                            (size.width - 2.0 * super::super::super::view::overlay::MTEXT_PREVIEW_PAD).max(80.0)
                                / editor.preview_scale()
                        })
                    });
                    if let (Some(editor), Some(width)) = (self.mtext_editor.as_mut(), initial_width)
                    {
                        editor.rect_width = f64::from(width.max(1e-6));
                        self.rebuild_mtext_preview();
                    }
                }
                Task::none()
            }
            Message::RibbonLayerFilterChanged(f) => {
                self.ribbon.layer_filter = f;
                Task::none()
            }
            Message::LayerShow(node) => {
                let i = self.active_tab;
                let panel = &mut self.tabs[i].layers;
                // A press on the tree is never the end of a row drag: one
                // released elsewhere must not file the layers here.
                panel.dragging = false;
                panel.drag_over = None;
                // A rename open on another node ends with the click.
                if panel.tree_rename.as_ref().is_some_and(|(n, _)| *n != node) {
                    panel.tree_rename = None;
                }
                panel.view = node;
                panel.tree_focus = true;
                panel.keys_active = true;
                if self.ribbon_layers_follow_filter {
                    self.sync_ribbon_layers();
                }
                Task::none()
            }
            Message::LayerTreeRenameEdit(typed) => {
                let i = self.active_tab;
                if let Some((_, name)) = self.tabs[i].layers.tree_rename.as_mut() {
                    *name = typed;
                }
                Task::none()
            }
            Message::LayerTreeRenameCommit => self.on_layer_tree_rename_commit(),
            Message::LayerTreeRenameCancel => {
                let i = self.active_tab;
                self.tabs[i].layers.tree_rename = None;
                Task::none()
            }
            Message::LayerNodeEye(node) => {
                let i = self.active_tab;
                let panel = &mut self.tabs[i].layers;
                let members = panel.node_layers(&node);
                let all_on = !members.is_empty() && members.iter().all(|m| panel.eye_on(m));
                panel
                    .eye_layers
                    .retain(|l| !members.iter().any(|m| m.eq_ignore_ascii_case(l)));
                if !all_on {
                    panel.eye_layers.extend(members);
                }
                panel.highlight_key = None;
                if panel.eye_layers.is_empty() {
                    panel.highlighted = 0;
                    self.tabs[i].scene.set_filter_preview_highlights(&[]);
                }
                self.refresh_layer_counts();
                Task::none()
            }
            Message::LayerSaveSearch => self.on_layer_save_search(),
            Message::LayerRibbonFollows(on) => {
                self.ribbon_layers_follow_filter = on;
                let i = self.active_tab;
                self.tabs[i].layers.ribbon_follows = on;
                self.sync_ribbon_layers();
                self.save_config();
                Task::none()
            }
            Message::LayerMoreMenu(open) => {
                let i = self.active_tab;
                self.tabs[i].layers.more_menu = open;
                Task::none()
            }
            Message::LayerMoreRun(msg) => {
                let i = self.active_tab;
                self.tabs[i].layers.more_menu = false;
                self.update(*msg)
            }
            Message::LayerSearchOpen(open) => {
                let i = self.active_tab;
                self.tabs[i].layers.search_open = open;
                Task::none()
            }
            Message::LayerDragEnd => {
                let i = self.active_tab;
                let panel = &mut self.tabs[i].layers;
                panel.dragging = false;
                panel.drag_over = None;
                Task::none()
            }
            Message::LayerDragOver(group) => {
                let i = self.active_tab;
                let panel = &mut self.tabs[i].layers;
                if panel.dragging {
                    panel.drag_over = group;
                }
                Task::none()
            }
            Message::LayerDrop(group) => {
                let i = self.active_tab;
                let panel = &mut self.tabs[i].layers;
                let dropped = panel.dragging;
                panel.dragging = false;
                panel.drag_over = None;
                if dropped {
                    let layers = panel.selected_names();
                    if !layers.is_empty() {
                        self.set_layer_group(i, &group, &layers, true);
                    }
                }
                Task::none()
            }
            Message::LayerTreeSection(section) => {
                let i = self.active_tab;
                self.tabs[i].layers.toggle_section(section);
                Task::none()
            }
            Message::LayerInvert(on) => {
                let i = self.active_tab;
                self.tabs[i].layers.invert = on;
                Task::none()
            }
            Message::LayerEye(name) => {
                let i = self.active_tab;
                let panel = &mut self.tabs[i].layers;
                if panel.eye_on(&name) {
                    panel.eye_layers.retain(|l| !l.eq_ignore_ascii_case(&name));
                } else {
                    panel.eye_layers.push(name);
                }
                panel.highlight_key = None;
                if panel.eye_layers.is_empty() {
                    panel.highlighted = 0;
                    self.tabs[i].scene.set_filter_preview_highlights(&[]);
                }
                self.refresh_layer_counts();
                Task::none()
            }
            Message::LayerFilterEdit(name) => self.on_layer_filter_edit(name),
            Message::LayerTreeAction(node, action) => self.on_layer_tree_action(node, action),
            Message::LayerGroupNew => {
                let i = self.active_tab;
                let name = self.next_layer_group_name(i);
                let layers = self.tabs[i].layers.selected_names();
                self.set_layer_group(i, &name, &layers, true);
                self.tabs[i].layers.set_group(Some(name));
                Task::none()
            }
            Message::LayerGroupAdd(name) => {
                let i = self.active_tab;
                let layers = self.tabs[i].layers.selected_names();
                self.set_layer_group(i, &name, &layers, true);
                Task::none()
            }
            Message::LayerGroupRemove => {
                let i = self.active_tab;
                if let Some(name) = self.tabs[i].layers.shown_group().cloned() {
                    let layers = self.tabs[i].layers.selected_names();
                    self.set_layer_group(i, &name, &layers, false);
                }
                Task::none()
            }
            Message::LayerGroupDelete => {
                let i = self.active_tab;
                if let Some(name) = self.tabs[i].layers.shown_group().cloned() {
                    self.tabs[i].layers.set_group(None);
                    self.delete_layer_group_named(i, &name);
                }
                Task::none()
            }
            Message::LayerManagerFilterChanged(f) => {
                let i = self.active_tab;
                self.tabs[i].layers.filter = f;
                Task::none()
            }
            Message::LayerColumnsShowAll => {
                crate::ui::window::layers::show_all_columns();
                self.save_config();
                Task::none()
            }
            Message::LayerColumnToggle(col) => {
                crate::ui::window::layers::toggle_column(col);
                self.save_config();
                Task::none()
            }
            Message::LayerColumnMove(col, past) => {
                crate::ui::window::layers::move_column(col, past);
                self.save_config();
                Task::none()
            }
            Message::LayerNameColGrab => {
                // Start a Name-column divider drag; the dock's pointer capture
                // feeds it (see `on_dock`).
                self.dock_drag = Some(crate::ui::dock::DockDrag::LayerColumn(None));
                self.dock_drag_last = None;
                Task::none()
            }
            Message::LayerColGrab(col) => {
                // Another column's divider: the same capture sizes it.
                self.dock_drag = Some(crate::ui::dock::DockDrag::LayerColumn(Some(col)));
                self.dock_drag_last = None;
                Task::none()
            }
            Message::ModalDragMove(p) => {
                if let Some(last) = self.modal_drag_last {
                    let (dx, dy) = (p.x - last.x, p.y - last.y);
                    if self.modal_resizing {
                        // The grip sits bottom-right, so dragging out grows the
                        // box. The delta is added to each dialog's natural size,
                        // so clamp it at zero — dragging in past the natural size
                        // does nothing (the natural size is the floor).
                        let nx = (self.modal_resize.x + dx).max(0.0);
                        let ny = (self.modal_resize.y + dy).max(0.0);
                        let (rx, ry) = (nx - self.modal_resize.x, ny - self.modal_resize.y);
                        self.modal_resize.x = nx;
                        self.modal_resize.y = ny;
                        // The box is centred, so shift the centre by half the
                        // growth to pin the top-left corner — the grip then
                        // tracks the cursor instead of drifting at half speed.
                        self.modal_offset.x += rx * 0.5;
                        self.modal_offset.y += ry * 0.5;
                    } else if self.modal_dragging {
                        self.modal_offset.x += dx;
                        self.modal_offset.y += dy;
                        // Clamp so the dialog stops at the window edge instead
                        // of being squeezed (the off-centre padding shrinks the
                        // dialog once it overlaps a border).
                        if let Some((cw, ch)) = self.modal_outer_size() {
                            let (ww, wh) = if self.mtext_editor.is_some() {
                                self.vp_size
                            } else {
                                self.win_size
                            };
                            let max_x = ((ww - cw) * 0.5).max(0.0);
                            let max_y = ((wh - ch) * 0.5).max(0.0);
                            self.modal_offset.x = self.modal_offset.x.clamp(-max_x, max_x);
                            self.modal_offset.y = self.modal_offset.y.clamp(-max_y, max_y);
                        }
                    }
                }
                if self.modal_dragging || self.modal_resizing {
                    self.modal_drag_last = Some(p);
                }
                Task::none()
            }
            Message::ModalDragRelease => {
                self.modal_dragging = false;
                self.modal_resizing = false;
                self.modal_drag_last = None;
                Task::none()
            }

            Message::AboutCopyInfo => {
                let info = crate::ui::window::about::report();
                crate::sys::clipboard_write(info)
            }

            // ── Plugin Manager window ─────────────────────────────────────
            Message::PluginManagerOpen => {
                #[cfg(target_arch = "wasm32")]
                {
                    self.active_modal = Some(super::super::super::ModalKind::PluginManager);
                    return Task::none();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Refresh the on-disk external-plugin list each time the manager
                    // opens so newly dropped-in packages show up.
                    self.external_plugins = crate::plugin::external::discover();
                    self.pending_plugin_approvals =
                        crate::plugin::external::collect_pending_approvals(&self.external_plugins);
                    self.refresh_plugin_tools();
                    self.marketplace_status.clear();
                    self.active_modal = Some(super::super::super::ModalKind::PluginManager);
                    // Fetch the curated registry and release lists for linked repos.
                    self.plugin_registry_loading = true;
                    self.plugin_registry_error = None;
                    self.plugin_registry_error_details_open = false;
                    let mut tasks = vec![self.fetch_registry_task()];
                    let release_repos: rustc_hash::FxHashSet<String> = self
                        .plugin_repos
                        .iter()
                        .cloned()
                        .chain(
                            self.external_plugins
                                .iter()
                                .filter_map(|plugin| plugin.repository.clone()),
                        )
                        .collect();
                    tasks.extend(
                        release_repos
                            .into_iter()
                            .map(|r| self.fetch_releases_task(r)),
                    );
                    if self.selected_plugin_repo.is_none() {
                        self.selected_plugin_repo = self
                            .external_plugins
                            .iter()
                            .find_map(|plugin| plugin.repository.clone())
                            .or_else(|| {
                                self.plugin_registry.first().map(|entry| entry.repo.clone())
                            })
                            .or_else(|| self.plugin_repos.first().cloned());
                    }
                    if let Some(repo) = self.selected_plugin_repo.clone() {
                        if !self.plugin_readmes.contains_key(&repo)
                            && self.plugin_readme_loading.insert(repo.clone())
                        {
                            tasks.push(self.fetch_plugin_readme_task(repo));
                        }
                    }
                    Task::batch(tasks)
                }
            }
            Message::PluginManagerClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::SetPluginEnabled(id, enabled) => {
                if enabled {
                    self.disabled_plugins.remove(&id);
                } else {
                    self.disabled_plugins.insert(id);
                }
                self.rebuild_ribbon_modules();
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::PluginRepoInput(s) => {
                self.plugin_repo_input = s;
                Task::none()
            }
            Message::PluginSearchInput(s) => {
                self.plugin_search_input = s;
                Task::none()
            }
            Message::PluginRepoAdd => {
                let Some(repo) =
                    crate::plugin::external::normalize_repository(&self.plugin_repo_input)
                else {
                    self.marketplace_status =
                        crate::t!("Enter a GitHub URL or repository in owner/repo format.")
                            .into_owned();
                    return Task::none();
                };
                if self.plugin_repos.contains(&repo)
                    || self.plugin_registry.iter().any(|entry| entry.repo == repo)
                {
                    self.marketplace_status =
                        crate::tf!("{repo} is already in the catalog.").into_owned();
                    self.selected_plugin_repo = Some(repo.clone());
                    if !self.plugin_readmes.contains_key(&repo)
                        && self.plugin_readme_loading.insert(repo.clone())
                    {
                        return self.fetch_plugin_readme_task(repo);
                    }
                    return Task::none();
                }
                if self
                    .external_plugins
                    .iter()
                    .any(|plugin| plugin.repository.as_deref() == Some(repo.as_str()))
                {
                    self.marketplace_status =
                        crate::tf!("{repo} is already installed.").into_owned();
                    self.selected_plugin_repo = Some(repo.clone());
                    if !self.plugin_readmes.contains_key(&repo)
                        && self.plugin_readme_loading.insert(repo.clone())
                    {
                        return self.fetch_plugin_readme_task(repo);
                    }
                    return Task::none();
                }
                self.plugin_repos.push(repo.clone());
                self.plugin_repo_input.clear();
                self.persist_settings_if_changed();
                self.marketplace_status = crate::tf!("Fetching releases for {repo}…").into_owned();
                self.selected_plugin_repo = Some(repo.clone());
                self.plugin_readmes.remove(&repo);
                self.plugin_readme_loading.insert(repo.clone());
                Task::batch(vec![
                    self.fetch_releases_task(repo.clone()),
                    self.fetch_plugin_readme_task(repo),
                ])
            }
            _ => Task::none(),
        }
    }
}
