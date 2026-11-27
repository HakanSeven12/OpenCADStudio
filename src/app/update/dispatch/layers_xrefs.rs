use super::super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_layers_xrefs(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::LayerTranslatorForceByLayer(value) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.force_bylayer = value;
                }
                Task::none()
            }
            Message::LayerTranslatorWriteLog(value) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.write_log = value;
                }
                Task::none()
            }
            Message::LayerTranslatorSaveMappings => Task::perform(
                crate::io::pick_layer_mapping_path(true),
                |path| match path {
                    Some(path) => Message::LayerTranslatorMappingsPath(path, true),
                    None => Message::Noop,
                },
            ),
            Message::LayerTranslatorLoadMappings => Task::perform(
                crate::io::pick_layer_mapping_path(false),
                |path| match path {
                    Some(path) => Message::LayerTranslatorMappingsPath(path, false),
                    None => Message::Noop,
                },
            ),
            Message::LayerTranslatorMappingsPath(path, save) => {
                self.layer_translator_mappings_file(&path, save)
            }
            Message::LayerFileRead(done) => self.finish_layer_file_read(*done),
            Message::LayerTranslatorTranslate => {
                use crate::modules::draw::layers::laytrans;
                let Some(state) = self.layer_translator.take() else {
                    return Task::none();
                };
                self.active_modal = None;
                let i = self.active_tab;
                let current = self.tabs[i].active_layer.clone();
                self.push_undo_snapshot(i, "LAYTRANS");
                let report = laytrans::translate(
                    &mut self.tabs[i].scene,
                    &state.mappings,
                    &state.targets,
                    &current,
                    laytrans::Options {
                        force_bylayer: state.force_bylayer,
                    },
                );
                let write_log = state.write_log;
                let task = self.finish_layer_translation(i, report);
                if write_log {
                    self.write_layer_translation_log(i);
                }
                task
            }
            Message::LayerStateManagerSelect(name) => {
                self.load_layer_state_editor(Some(name));
                Task::none()
            }
            Message::LayerStateManagerNew => {
                self.load_layer_state_editor(None);
                Task::none()
            }
            Message::LayerStateManagerFilter(value) => {
                self.layer_state_filter = value;
                Task::none()
            }
            Message::LayerStateManagerName(value) => {
                self.layer_state_name_buf = value;
                Task::none()
            }
            Message::LayerStateManagerDescription(value) => {
                self.layer_state_description_buf = value;
                Task::none()
            }
            Message::LayerStateManagerSave => {
                let i = self.active_tab;
                let name = self.layer_state_name_buf.trim().to_string();
                if name.is_empty() {
                    self.command_line
                        .push_error(crate::t!("Layer state name cannot be empty.").as_ref());
                    return Task::none();
                }
                let old_name = self.layer_state_selected.clone();
                let duplicate =
                    self.tabs[i]
                        .scene
                        .document
                        .layer_states()
                        .into_iter()
                        .any(|state| {
                            state.name.eq_ignore_ascii_case(&name)
                                && old_name
                                    .as_deref()
                                    .is_none_or(|old| !state.name.eq_ignore_ascii_case(old))
                        });
                if duplicate {
                    self.command_line
                        .push_error(crate::tf!("Layer state \"{name}\" already exists.").as_ref());
                    return Task::none();
                }

                self.push_undo_snapshot(i, "LAYERSTATE SAVE");
                if let Some(old_name) = old_name.as_deref() {
                    if !old_name.eq_ignore_ascii_case(&name) {
                        self.tabs[i]
                            .scene
                            .document
                            .rename_layer_state(old_name, &name);
                    }
                }
                self.tabs[i]
                    .scene
                    .document
                    .capture_layer_state(&name, self.layer_state_description_buf.trim());
                self.tabs[i].dirty = true;
                self.layer_state_selected = Some(name.clone());
                self.layer_state_name_buf = name.clone();
                self.command_line.push_output(
                    crate::tf!("LAYERSTATE: saved \"{name}\" in the drawing.").as_ref(),
                );
                Task::none()
            }
            Message::RibbonLayerStateRestore(name) => {
                self.ribbon.close_dropdown();
                self.layer_state_selected = Some(name);
                self.update(Message::LayerStateManagerRestore)
            }
            Message::LayerStateManagerRestore => {
                let i = self.active_tab;
                let Some(name) = self.layer_state_selected.clone() else {
                    return Task::none();
                };
                let layer_names: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .map(|layer| layer.name.clone())
                    .collect();
                self.push_undo_snapshot(i, "LAYERSTATE RESTORE");
                let restored = self.restore_layer_state_here(i, &name);
                let active = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_layer_name
                    .clone();
                self.tabs[i].active_layer = active;
                self.tabs[i]
                    .scene
                    .invalidate_layer_dependencies(&layer_names);
                self.tabs[i].dirty = true;
                self.refresh_layer_panel();
                self.command_line.push_output(
                    crate::tf!("LAYERSTATE: restored \"{name}\" ({restored} layer(s)).").as_ref(),
                );
                Task::none()
            }
            Message::LayerStateManagerDelete => {
                let i = self.active_tab;
                let Some(name) = self.layer_state_selected.clone() else {
                    return Task::none();
                };
                self.push_undo_snapshot(i, "LAYERSTATE DELETE");
                if self.tabs[i].scene.document.delete_layer_state(&name) {
                    self.tabs[i].dirty = true;
                    let mut names: Vec<String> = self.tabs[i]
                        .scene
                        .document
                        .layer_states()
                        .into_iter()
                        .map(|state| state.name)
                        .collect();
                    names.sort_by_key(|name| name.to_lowercase());
                    self.load_layer_state_editor(names.into_iter().next());
                    self.command_line
                        .push_output(crate::tf!("LAYERSTATE: deleted \"{name}\".").as_ref());
                }
                Task::none()
            }
            Message::LayerStateManagerEdit => {
                let i = self.active_tab;
                let Some(name) = self.layer_state_selected.clone() else {
                    return Task::none();
                };
                let Some(state) = self.tabs[i].scene.document.layer_state(&name) else {
                    self.command_line
                        .push_error(crate::tf!("Layer state \"{name}\" was not found.").as_ref());
                    return Task::none();
                };
                self.layer_state_edit_draft = Some(state);
                self.layer_state_edit_filter.clear();
                self.layer_state_edit_color_open = None;
                self.active_modal = Some(super::super::super::ModalKind::LayerStateEditor);
                Task::none()
            }
            Message::LayerStateEditorMaskToggle(property) => {
                let flag = match property {
                    super::super::super::LayerStateProperty::On => document::LayerStateMask::ON,
                    super::super::super::LayerStateProperty::Frozen => document::LayerStateMask::FROZEN,
                    super::super::super::LayerStateProperty::Locked => document::LayerStateMask::LOCKED,
                    super::super::super::LayerStateProperty::Plot => document::LayerStateMask::PLOT,
                    super::super::super::LayerStateProperty::NewViewport => {
                        document::LayerStateMask::NEW_VIEWPORT
                    }
                    super::super::super::LayerStateProperty::Color => document::LayerStateMask::COLOR,
                    super::super::super::LayerStateProperty::LineType => document::LayerStateMask::LINE_TYPE,
                    super::super::super::LayerStateProperty::LineWeight => document::LayerStateMask::LINE_WEIGHT,
                    super::super::super::LayerStateProperty::PlotStyle => document::LayerStateMask::PLOT_STYLE,
                    super::super::super::LayerStateProperty::Transparency => {
                        document::LayerStateMask::TRANSPARENCY
                    }
                };
                if let Some(state) = self.layer_state_edit_draft.as_mut() {
                    state.mask =
                        document::LayerStateMask::from_bits(state.mask.bits() ^ flag.bits());
                }
                Task::none()
            }
            Message::LayerStateEditorLayerFlagToggle(index, flag) => {
                let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                else {
                    return Task::none();
                };
                match flag {
                    super::super::super::LayerStateLayerFlag::On => layer.off = !layer.off,
                    super::super::super::LayerStateLayerFlag::Frozen => layer.frozen = !layer.frozen,
                    super::super::super::LayerStateLayerFlag::Locked => layer.locked = !layer.locked,
                    super::super::super::LayerStateLayerFlag::Plot => layer.plottable = !layer.plottable,
                    super::super::super::LayerStateLayerFlag::NewViewport => {
                        layer.new_viewport_frozen = !layer.new_viewport_frozen
                    }
                }
                Task::none()
            }
            Message::LayerStateEditorLayerColorToggle(index) => {
                self.layer_state_edit_color_open =
                    if self.layer_state_edit_color_open == Some(index) {
                        None
                    } else {
                        Some(index)
                    };
                Task::none()
            }
            Message::LayerStateEditorLayerColor(index, color) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.color = color;
                }
                self.layer_state_edit_color_open = None;
                Task::none()
            }
            Message::LayerStateEditorLayerLinetype(index, value) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.line_type = value;
                }
                Task::none()
            }
            Message::LayerStateEditorLayerLineweight(index, value) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.line_weight = value;
                }
                Task::none()
            }
            Message::LayerStateEditorLayerPlotStyle(index, value) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.plot_style = value;
                }
                Task::none()
            }
            Message::LayerStateEditorLayerTransparency(index, value) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.transparency = value;
                }
                Task::none()
            }
            Message::LayerStateEditorName(value) => {
                if let Some(state) = self.layer_state_edit_draft.as_mut() {
                    state.name = value;
                }
                Task::none()
            }
            Message::LayerStateEditorDescription(value) => {
                if let Some(state) = self.layer_state_edit_draft.as_mut() {
                    state.description = value;
                }
                Task::none()
            }
            Message::LayerStateEditorCurrentLayer(value) => {
                if let Some(state) = self.layer_state_edit_draft.as_mut() {
                    state.current_layer = value;
                }
                Task::none()
            }
            Message::LayerStateEditorFilter(value) => {
                self.layer_state_edit_filter = value;
                Task::none()
            }
            Message::LayerStateEditorSave => {
                let i = self.active_tab;
                let Some(draft) = self.layer_state_edit_draft.as_ref() else {
                    return Task::none();
                };
                let name = draft.name.trim().to_string();
                if name.is_empty() {
                    self.command_line
                        .push_error(crate::t!("Layer state name cannot be empty.").as_ref());
                    return Task::none();
                }
                let old_name = self.layer_state_selected.clone();
                let duplicate =
                    self.tabs[i]
                        .scene
                        .document
                        .layer_states()
                        .into_iter()
                        .any(|state| {
                            state.name.eq_ignore_ascii_case(&name)
                                && old_name
                                    .as_deref()
                                    .is_none_or(|old| !state.name.eq_ignore_ascii_case(old))
                        });
                if duplicate {
                    self.command_line
                        .push_error(crate::tf!("Layer state \"{name}\" already exists.").as_ref());
                    return Task::none();
                }
                let Some(state) = self.layer_state_edit_draft.take() else {
                    return Task::none();
                };
                let mut state = state;
                state.name.clone_from(&name);
                state.description = state.description.trim().to_string();
                let description = state.description.clone();
                let linetypes: Vec<String> = state
                    .layers
                    .iter()
                    .map(|layer| layer.line_type.clone())
                    .collect();
                self.push_undo_snapshot(i, "LAYERSTATE EDIT");
                for lt in &linetypes {
                    let _ = self.ensure_tab_catalog_linetype(i, lt);
                }
                if let Some(old_name) = old_name.as_deref() {
                    if !old_name.eq_ignore_ascii_case(&name) {
                        self.tabs[i]
                            .scene
                            .document
                            .rename_layer_state(old_name, &name);
                    }
                }
                self.tabs[i].scene.document.store_layer_state(state);
                self.tabs[i].dirty = true;
                self.layer_state_selected = Some(name.clone());
                self.layer_state_name_buf = name.clone();
                self.layer_state_description_buf = description;
                self.layer_state_edit_filter.clear();
                self.layer_state_edit_color_open = None;
                self.active_modal = Some(super::super::super::ModalKind::LayerStateManager);
                self.command_line
                    .push_output(crate::tf!("LAYERSTATE: updated \"{name}\".").as_ref());
                Task::none()
            }
            Message::LayerStateEditorCancel => {
                self.layer_state_edit_draft = None;
                self.layer_state_edit_filter.clear();
                self.layer_state_edit_color_open = None;
                self.active_modal = Some(super::super::super::ModalKind::LayerStateManager);
                Task::none()
            }

            Message::WindowCloseRequested(id) => {
                if self.main_window == Some(id) {
                    // A plain quit after a cancelled Restart quits only.
                    #[cfg(not(target_arch = "wasm32"))]
                    super::super::super::RESTART_AFTER_EXIT.store(false, std::sync::atomic::Ordering::Relaxed);
                    return self.request_quit_with_block_edits();
                }
                #[cfg(not(target_arch = "wasm32"))]
                return self.os_window_close_requested(id);
                #[cfg(target_arch = "wasm32")]
                Task::none()
            }

            Message::OsWindowClosed(id) => {
                // Closing the main window exits; a window of its own (a tool
                // dialog or pallet) only goes away.
                if self.main_window == Some(id) {
                    return self.exit_app();
                }
                #[cfg(not(target_arch = "wasm32"))]
                self.os_window_closed(id);
                Task::none()
            }

            // ── Layer panel messages ───────────────────────────────────────
            Message::LayerToggleVisible(name) => {
                let i = self.active_tab;
                let Some(idx) = self.layer_panel_index(i, &name) else {
                    return Task::none();
                };
                // New state = toggle of the clicked row, applied to every target
                // (the whole selection when the clicked row is part of it) (#236).
                let on = self.tabs[i].layers.layers.get(idx).map(|l| !l.visible);
                let targets = self.layer_row_action_targets(i, idx);
                if let Some(on) = on {
                    if !targets.is_empty() {
                        let undo = self.begin_layer_undo(i, "LAYER OFF/ON", &targets);
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                dl.flags.off = !on;
                            }
                            if let Some(pl) = self.tabs[i]
                                .layers
                                .layers
                                .iter_mut()
                                .find(|l| &l.name == name)
                            {
                                pl.visible = on;
                            }
                        }
                        self.tabs[i].scene.invalidate_layer_visibility(&targets);
                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);
                        self.command_line.push_output(
                            crate::tf!(
                                "{} layer(s) turned {}",
                                targets.len(),
                                if on { "on" } else { "off" }
                            )
                            .as_ref(),
                        );
                        self.sync_ribbon_layers();
                    }
                }
                Task::none()
            }

            Message::LayerSort(col) => {
                let i = self.active_tab;
                self.tabs[i].layers.sort_by(col);
                // Keep the ribbon dropdown's order (and its toggle indices) in
                // step with the re-sorted manager table.
                self.sync_ribbon_layers();
                Task::none()
            }

            Message::LayerToggleLock(name) => {
                let i = self.active_tab;
                let Some(idx) = self.layer_panel_index(i, &name) else {
                    return Task::none();
                };
                let locked = self.tabs[i].layers.layers.get(idx).map(|l| !l.locked);
                let targets = self.layer_row_action_targets(i, idx);
                if let Some(locked) = locked {
                    if !targets.is_empty() {
                        let undo = self.begin_layer_undo(i, "LAYER LOCK/UNLOCK", &targets);
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                dl.flags.locked = locked;
                            }
                            if let Some(pl) = self.tabs[i]
                                .layers
                                .layers
                                .iter_mut()
                                .find(|l| &l.name == name)
                            {
                                pl.locked = locked;
                            }
                        }
                        // Lock state changes the display colour in the Model
                        // view, so invalidate all render dependencies as well
                        // as the editability state.
                        self.tabs[i].scene.invalidate_layer_dependencies(&targets);
                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);
                        self.command_line.push_output(
                            crate::tf!(
                                "{} layer(s) {}",
                                targets.len(),
                                if locked { "locked" } else { "unlocked" }
                            )
                            .as_ref(),
                        );
                        self.sync_ribbon_layers();
                        self.refresh_properties();
                    }
                }
                Task::none()
            }

            Message::LayerToggleFreeze(name) => {
                let i = self.active_tab;
                let Some(idx) = self.layer_panel_index(i, &name) else {
                    return Task::none();
                };
                let frozen = self.tabs[i].layers.layers.get(idx).map(|l| !l.frozen);
                let mut targets = self.layer_row_action_targets(i, idx);
                // The current layer can't be frozen; thawing it is fine.
                if frozen == Some(true) {
                    let current = self.tabs[i]
                        .scene
                        .document
                        .header
                        .current_layer_name
                        .clone();
                    let before = targets.len();
                    targets.retain(|name| !name.eq_ignore_ascii_case(&current));
                    if targets.len() < before {
                        self.command_line.push_info(
                            crate::t!("The current layer can't be frozen — skipped.").as_ref(),
                        );
                    }
                }
                if let Some(frozen) = frozen {
                    if !targets.is_empty() {
                        let undo = self.begin_layer_undo(i, "LAYER FREEZE", &targets);
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                if frozen {
                                    dl.freeze();
                                } else {
                                    dl.thaw();
                                }
                            }
                            if let Some(pl) = self.tabs[i]
                                .layers
                                .layers
                                .iter_mut()
                                .find(|l| &l.name == name)
                            {
                                pl.frozen = frozen;
                            }
                        }
                        self.tabs[i].scene.invalidate_layer_visibility(&targets);
                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);
                        self.command_line.push_output(
                            crate::tf!(
                                "{} layer(s) {}",
                                targets.len(),
                                if frozen { "frozen" } else { "thawed" }
                            )
                            .as_ref(),
                        );
                        self.sync_ribbon_layers();
                    }
                }
                Task::none()
            }
            Message::LayerTogglePlot(name) => {
                let i = self.active_tab;
                let Some(idx) = self.layer_panel_index(i, &name) else {
                    return Task::none();
                };

                let plottable = self.tabs[i]
                    .layers
                    .layers
                    .get(idx)
                    .map(|layer| !layer.plottable);

                let targets = self.layer_row_action_targets(i, idx);

                if let Some(plottable) = plottable {
                    if !targets.is_empty() {
                        let undo = self.begin_layer_undo(i, "LAYER PLOT/NOPLOT", &targets);

                        for name in &targets {
                            if let Some(layer) = self.tabs[i].scene.document.layers.get_mut(name) {
                                layer.is_plottable = plottable;
                            }

                            if let Some(layer) = self.tabs[i]
                                .layers
                                .layers
                                .iter_mut()
                                .find(|layer| &layer.name == name)
                            {
                                layer.plottable = plottable;
                            }
                        }

                        // Like the other toggles, no re-sort: the row stays
                        // under the pointer.
                        self.tabs[i].scene.invalidate_layer_dependencies(&targets);

                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);

                        self.command_line.push_output(
                            crate::tf!(
                                "{} layer(s) set to {}",
                                targets.len(),
                                if plottable { "Plot" } else { "No Plot" }
                            )
                            .as_ref(),
                        );
                    }
                }

                Task::none()
            }

            Message::LayerToggleVpFreeze(name, vp_col_idx) => {
                let Some(idx) = self.layer_panel_index(self.active_tab, &name) else {
                    return Task::none();
                };
                self.on_layer_toggle_vp_freeze(idx, vp_col_idx)
            }

            Message::LayerNew => self.on_layer_new(),

            Message::LayerImport(msg) => self.on_layer_import(msg),

            Message::LayerNewVpFrozen => self.on_layer_new_with(true),

            Message::LayerMenu(name, action) => self.on_layer_menu(name, action),

            Message::LayerDelete => self.on_layer_delete(),

            Message::LayerDeleteConfirm => self.on_layer_delete_confirm(),

            Message::LayerSetCurrent => self.on_layer_set_current(),

            Message::LayerSelect(name) => {
                let i = self.active_tab;
                let Some(idx) = self.layer_panel_index(i, &name) else {
                    return Task::none();
                };
                if self.tabs[i].layers.editing.is_some() {
                    return Task::done(Message::LayerRenameCommit);
                }
                let (shift, ctrl) = (self.shift_down, self.ctrl_down);
                let panel = &mut self.tabs[i].layers;
                panel.tree_rename = None;
                // The press may become a drag onto a group in the tree.
                panel.dragging = true;
                panel.tree_focus = false;
                if ctrl {
                    // Ctrl/Cmd-click toggles this row in the selection.
                    if let Some(pos) = panel.selected_multi.iter().position(|&x| x == idx) {
                        panel.selected_multi.remove(pos);
                    } else {
                        panel.selected_multi.push(idx);
                    }
                } else if shift {
                    // Shift-click selects the range from the anchor to here —
                    // visible rows only, so filtered-out rows between them
                    // cannot join bulk edits/deletes (#50).
                    let anchor = panel.selected.unwrap_or(idx);
                    let (lo, hi) = (anchor.min(idx), anchor.max(idx));
                    let visible: std::collections::HashSet<usize> =
                        panel.visible_indices().into_iter().collect();
                    panel.selected_multi = (lo..=hi).filter(|r| visible.contains(r)).collect();
                } else if !panel.selected_multi.contains(&idx) {
                    // Plain click collapses to this row — but NOT when it is
                    // already part of a multi-selection, so clicking a property
                    // combo (linetype / lineweight) on one of the selected rows
                    // keeps the selection and the edit stays bulk (#236).
                    panel.selected_multi = vec![idx];
                }
                panel.selected = Some(idx);
                panel.keys_active = true;
                Task::none()
            }

            Message::LayerRenameStart(name) => {
                // Ctrl/Shift on a name extends the selection, as on the rest
                // of the row; it does not start a rename (#2610).
                if self.ctrl_down || self.shift_down {
                    return self.update(Message::LayerSelect(name));
                }
                let i = self.active_tab;
                let Some(idx) = self.layer_panel_index(i, &name) else {
                    return Task::none();
                };
                self.tabs[i].layers.selected = Some(idx);
                self.tabs[i].layers.selected_multi = vec![idx];
                self.tabs[i].layers.edit_buf = name.clone();
                self.tabs[i].layers.editing = Some(name);
                focus_layer_name_field()
            }

            Message::LayerRenameEdit(s) => {
                let i = self.active_tab;
                self.tabs[i].layers.edit_buf = s;
                Task::none()
            }

            Message::LayerRenameCommit => self.on_layer_rename_commit(),

            // A click moved focus off the name field: keep what was typed, as
            // Enter would. An untouched name stays open (the field may not have
            // taken focus yet on the click that opened it).
            Message::LayerRenameFocus(focused) => {
                let layers = &self.tabs[self.active_tab].layers;
                let typed = layers
                    .editing
                    .as_ref()
                    .is_some_and(|name| *name != layers.edit_buf);
                if typed && focused != Some(crate::ui::window::layers::rename_input_id()) {
                    return self.commit_layer_rename(false);
                }
                Task::none()
            }

            Message::LayerColorPickerToggle(name) => {
                let i = self.active_tab;
                let Some(idx) = self.layer_panel_index(i, &name) else {
                    return Task::none();
                };
                let panel = &mut self.tabs[i].layers;
                if panel.color_picker_row.as_deref() == Some(name.as_str()) {
                    panel.color_picker_row = None;
                    panel.color_full_palette = false;
                } else {
                    panel.color_picker_row = Some(name);
                    panel.color_full_palette = false;
                    panel.selected = Some(idx);
                    // Opening the swatch on a row outside the current
                    // multi-selection narrows to just that row; on a selected
                    // row it keeps the multi-selection so the pick applies to all.
                    if !panel.selected_multi.contains(&idx) {
                        panel.selected_multi = vec![idx];
                    }
                }
                Task::none()
            }

            Message::LayerColorMorePalette => {
                let i = self.active_tab;
                self.tabs[i].layers.color_full_palette = !self.tabs[i].layers.color_full_palette;
                Task::none()
            }

            Message::LayerColorSet(color) => self.set_layer_color(color, None),

            Message::LayerLinetypeSet(lt) => {
                let i = self.active_tab;
                let names = self.selected_layer_names(i);
                if !names.is_empty() {
                    let needs_catalog = !self.tabs[i]
                        .scene
                        .document
                        .line_types
                        .iter()
                        .any(|entry| entry.name.eq_ignore_ascii_case(&lt));
                    if needs_catalog {
                        self.push_undo_snapshot(i, "LAYER LINETYPE");
                        let _ = self.ensure_tab_catalog_linetype(i, &lt);
                        for name in &names {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                dl.line_type = lt.clone();
                            }
                        }
                        for pl in self.tabs[i].layers.layers.iter_mut() {
                            if names.contains(&pl.name) {
                                pl.linetype = lt.clone();
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.invalidate_layer_dependencies(&names);
                    } else {
                        let undo = self.begin_layer_undo(i, "LAYER LINETYPE", &names);
                        for name in &names {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                dl.line_type = lt.clone();
                            }
                        }
                        for pl in self.tabs[i].layers.layers.iter_mut() {
                            if names.contains(&pl.name) {
                                pl.linetype = lt.clone();
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);
                        // Linetype is baked into the cached wires; repaint.
                        self.tabs[i].scene.invalidate_layer_dependencies(&names);
                    }
                }
                Task::none()
            }

            Message::LayerLineweightSet(lw) => {
                let i = self.active_tab;
                let names = self.selected_layer_names(i);
                if !names.is_empty() {
                    let undo = self.begin_layer_undo(i, "LAYER LINEWEIGHT", &names);
                    for name in &names {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.line_weight = lw;
                        }
                    }
                    for pl in self.tabs[i].layers.layers.iter_mut() {
                        if names.contains(&pl.name) {
                            pl.lineweight = lw;
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.commit_layer_undo(i, undo);
                    // Lineweight is baked into the cached wires; repaint.
                    self.tabs[i].scene.invalidate_layer_dependencies(&names);
                }
                Task::none()
            }

            Message::LayerTransparencyEdit(name, s) => {
                let i = self.active_tab;
                if self.layer_panel_index(i, &name).is_none() {
                    return Task::none();
                }
                // Another row's typed value is applied before this one starts.
                let other = self.tabs[i]
                    .layers
                    .transparency_edit
                    .as_ref()
                    .is_some_and(|(editing, _)| *editing != name);
                let task = if other {
                    self.on_layer_transparency_commit()
                } else {
                    Task::none()
                };
                self.tabs[i].layers.transparency_edit = Some((name, s));
                task
            }

            Message::LayerTransparencyCommit => self.on_layer_transparency_commit(),

            Message::LayerDescriptionEdit(name, s) => {
                let i = self.active_tab;
                if self.layer_panel_index(i, &name).is_none() {
                    return Task::none();
                }
                let other = self.tabs[i]
                    .layers
                    .description_edit
                    .as_ref()
                    .is_some_and(|(editing, _)| *editing != name);
                let task = if other {
                    self.on_layer_description_commit()
                } else {
                    Task::none()
                };
                self.tabs[i].layers.description_edit = Some((name, s));
                task
            }

            Message::LayerDescriptionCommit => self.on_layer_description_commit(),

            Message::LayerDescriptionFocus(focused) => {
                if focused != Some(crate::ui::window::layers::description_input_id()) {
                    return self.on_layer_description_commit();
                }
                Task::none()
            }

            // A click moved focus off the transparency field: apply what was
            // typed, as Enter would.
            Message::LayerTransparencyFocus(focused) => {
                if focused != Some(crate::ui::window::layers::transparency_input_id()) {
                    return self.on_layer_transparency_commit();
                }
                Task::none()
            }

            // ── Cursor / viewport messages ─────────────────────────────────
            Message::CursorMoved(p, viewport) => self.on_cursor_moved(p, viewport),

            // A multi-finger gesture owns the pointer while it runs.
            Message::ViewportMove(_)
            | Message::ViewportLeftPressAt(_)
            | Message::ViewportLeftPress
            | Message::ViewportLeftRelease
            | Message::PaneMove(..)
            | Message::PanePress(_)
            | Message::PanePressAt(..)
            | Message::PaneRelease(_)
                if self.touch.active() =>
            {
                Task::none()
            }

            // The navigation style may take a left or right press (and its
            // release) for a view drag; otherwise it runs its normal path.
            message @ (Message::ViewportLeftPressAt(_)
            | Message::ViewportLeftPress
            | Message::PanePress(_)
            | Message::PanePressAt(..))
                if !self.nav_replay =>
            {
                match self.nav_press(true, &message) {
                    Some(task) => task,
                    None => self.update_navigation_passthrough(message),
                }
            }
            message @ (Message::ViewportLeftRelease | Message::PaneRelease(_))
                if !self.nav_replay =>
            {
                match self.nav_release(true) {
                    Some(task) => task,
                    None => self.update_navigation_passthrough(message),
                }
            }
            Message::ViewportRightPress if !self.nav_replay => {
                match self.nav_press(false, &Message::ViewportRightPress) {
                    Some(task) => task,
                    None => self.update_navigation_passthrough(Message::ViewportRightPress),
                }
            }
            Message::ViewportRightRelease if !self.nav_replay => match self.nav_release(false) {
                Some(task) => task,
                None => self.update_navigation_passthrough(Message::ViewportRightRelease),
            },

            Message::ViewportMove(p) => self.on_pointer_move_coalesced(p),

            Message::ViewportExit => self.on_viewport_exit(),

            // ── Per-pane Model viewport ───────────────────────────────────
            Message::PaneResized(ev) => self.on_pane_resized(ev),
            Message::PaneClicked(pane) => self.on_pane_clicked(pane),
            Message::PaneDragged(ev) => self.on_pane_dragged(ev),
            Message::PaneMove(idx, local) => {
                if self.color_picker.is_some() {
                    return Task::none();
                }
                let p = self.pane_canvas_point(idx, local);
                // While dragging a pane, just track the cursor (no focus swap or
                // snap) so the drop target reads cleanly.
                if self.pane_move_from.is_some() {
                    let mut sel = self.tabs[self.active_tab].scene.selection.borrow_mut();
                    sel.input.last_move_pos = Some(p);
                    sel.input.pointer_away = false;
                    return Task::none();
                }
                self.focus_model_pane(idx);
                self.on_pointer_move_coalesced(p)
            }
            Message::ViewportHighlightToggle => {
                let on = !self.model_space.viewport_hover_highlight;
                self.model_space.viewport_hover_highlight = on;
                self.command_line
                    .push_info(&format!("VPHIGHLIGHT = {}", on as u8));
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ViewportHighlightOpacity(op) => {
                let op = op.min(100);
                if op != self.model_space.viewport_hover_opacity {
                    self.model_space.viewport_hover_opacity = op;
                    // No echo: a wheel spin is many steps, and the chip's
                    // tooltip and the outline itself show the value.
                    self.persist_settings_if_changed();
                }
                Task::none()
            }
            Message::ViewportOutlineSwitchHover(on) => {
                self.outline_switch_hover = on;
                Task::none()
            }
            Message::PaneMoveStart => {
                let i = self.active_tab;
                self.pane_move_from = Some(self.tabs[i].scene.active_model_tile.get());
                Task::none()
            }
            Message::PanePressAt(idx, local) => {
                self.pane_move_from = None;
                self.focus_model_pane(idx);
                let position = self.pane_canvas_point(idx, local);
                self.update(Message::ViewportLeftPressAt(position))
            }
            Message::PanePress(idx) => {
                // A click-away into the drawing area re-syncs the active-row
                // highlight against real focus before the pick runs.
                // The press takes the focus from a Properties field: what was typed
                // there is applied before the pick can change the selection.
                let applied = self.commit_active_prop_field();
                let sweep = Task::batch([applied, self.sync_active_field_if_any()]);
                // A fresh press ends any stale (un-dropped) pane move.
                self.pane_move_from = None;
                self.focus_model_pane(idx);
                Task::batch(vec![sweep, self.on_viewport_left_press()])
            }
            Message::PaneRelease(idx) => {
                // Finishing a pane-move drag: swap the source pane with the one
                // released over, instead of the normal release handling.
                if let Some(from) = self.pane_move_from.take() {
                    let i = self.active_tab;
                    self.tabs[i].scene.swap_model_panes(from, idx);
                    self.tabs[i].scene.camera_generation += 1;
                    return Task::none();
                }
                self.focus_model_pane(idx);
                self.on_viewport_left_release()
            }
            Message::PaneRightPress(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportRightPress)
            }
            Message::PaneRightRelease(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportRightRelease)
            }
            Message::PaneMiddlePress(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportMiddlePress)
            }
            Message::PaneMiddleRelease(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportMiddleRelease)
            }
            Message::PaneScroll(idx, d) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportScroll(d))
            }

            Message::ViewportLeftPressAt(position) => {
                // Use the pointer location captured by the actual button
                // event. The last hover can lag behind fast cursor motion,
                // particularly while expensive OSNAP/OTRACK previews run.
                self.snap_dwell = None;
                // Clicking the drawing ends the walk list's claim on the arrow
                // keys; the pallet stays open and keeps its selection.
                self.layer_walk_lost_focus();
                let tab = &self.tabs[self.active_tab];
                // Ordinary selection does not consume a snapped point. Running
                // ViewportMove here repeated the full snap/hover query at click
                // time, even though the selection handlers pick from the exact
                // screen position themselves. Keep command picks on the full
                // path unless they use the existing lightweight point-pick path.
                let plain_selection_press = tab.active_cmd.is_none()
                    && tab.active_grip.is_none()
                    && !tab.orbit_mode
                    && !tab.pan_mode
                    && !tab.zoom_dynamic_mode;
                // A point command resolves snapping and Dynamic UCS from this
                // exact press in on_viewport_left_release. Running a full move
                // pass first would repeat the dense-scene snap before the click.
                let press_move_started = crate::perf::enabled().then(Instant::now);
                let (moved, cursor_path) = if plain_selection_press {
                    self.set_cursor_for_press(position);
                    (Task::none(), "selection-projection")
                } else if self.command_picks_on_press()
                    && tab.active_cmd.as_ref().is_some_and(|command| {
                        !command.needs_entity_pick() && !command.needs_structure_point_pick()
                    })
                    && tab.active_grip.is_none()
                    && !tab.orbit_mode
                    && !tab.pan_mode
                    && !tab.zoom_dynamic_mode
                {
                    self.set_cursor_for_press(position);
                    (Task::none(), "point-projection")
                } else {
                    (self.on_viewport_move(position), "full-move")
                };
                let press_move_ms = crate::perf::elapsed_ms(press_move_started);
                if press_move_ms >= 1.0 {
                    crate::perf_record!(
                        "[perf] pointer-press-cursor path={cursor_path} total={press_move_ms:.1}ms"
                    );
                }
                self.graphic_attributes.open_menu = None;
                // The press takes the focus from a Properties field: what was typed
                // there is applied before the pick can change the selection.
                let applied = self.commit_active_prop_field();
                let sweep = Task::batch([applied, self.sync_active_field_if_any()]);
                Task::batch(vec![moved, sweep, self.on_viewport_left_press()])
            }

            Message::ViewportLeftPress => {
                self.graphic_attributes.open_menu = None;
                // The press takes the focus from a Properties field: what was typed
                // there is applied before the pick can change the selection.
                let applied = self.commit_active_prop_field();
                let sweep = Task::batch([applied, self.sync_active_field_if_any()]);
                Task::batch(vec![sweep, self.on_viewport_left_press()])
            }

            Message::ViewportLeftRelease => self.on_viewport_left_release(),

            Message::ViewportRightPress => {
                self.graphic_attributes.open_menu = None;
                let i = self.active_tab;
                self.tabs[i].measurement.dismiss_result();
                self.ribbon.close_dropdown();
                // Shift+RMB: the one-shot snap override menu at the cursor —
                // pick a snap for just the next point, then it expires (#337).
                if self.shift_down {
                    let pos = self.tabs[i].scene.selection.borrow().input.last_move_pos;
                    if let Some(p) = pos {
                        self.snap_override_popup = Some(p);
                    }
                    return Task::none();
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                let Some(p) = sel.input.last_move_pos else {
                    return Task::none();
                };
                sel.menu.close();
                sel.input.right_down = true;
                sel.input.right_press_pos = Some(p);
                sel.input.right_press_time = Some(iced::time::Instant::now());
                sel.input.right_last_pos = Some(p);
                sel.input.right_dragging = false;
                Task::none()
            }

            Message::ViewportRightRelease => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                let Some(click_pos) = sel.input.last_move_pos else {
                    return Task::none();
                };
                if !sel.input.right_down {
                    return Task::none();
                }
                let was_click = !sel.input.right_dragging;
                // How long the button was held, for the time-sensitive mode.
                let held_ms = sel
                    .input
                    .right_press_time
                    .map_or(0, |t| t.elapsed().as_millis() as i32);
                sel.input.right_down = false;
                sel.input.right_press_pos = None;
                sel.input.right_press_time = None;
                sel.input.right_last_pos = None;
                sel.input.right_dragging = false;
                sel.orbit.pivot = None;
                if !was_click {
                    return Task::none();
                }
                // A command name (or option keyword / value) typed into the
                // command line but not yet entered runs on right-click, exactly
                // as pressing Enter would. Route through CommandFinalize — the
                // canonical Enter action — so the same MText / grip-popup guards
                // apply and a non-empty line is forwarded to the submit path.
                // Without this the typed text would be swallowed by the context
                // menu (when idle) or the Enter cycle. Every other right-click
                // behaviour below is unchanged and only applies when the command
                // line is empty. Pending text always runs and resets the Enter
                // cycle so the next right-click acts as Enter again.
                if !self.command_line.input.trim().is_empty() {
                    sel.input.right_click_entered = false;
                    drop(sel);
                    return self.update(Message::CommandFinalize);
                }
                // The Quad cursor menu, when it exists, owns the right-click
                // like commercial solutions do (docs/quad-menu.md §3.4): the
                // hovered default command launches, the Selection quad
                // replaces the legacy menu on a selection, the No Selection
                // quad appears in the void — and the legacy path below
                // answers everything else (a command running keeps it
                // entirely).
                drop(sel);
                if self.quad && self.tabs[i].active_cmd.is_none() {
                    // The takeover needs the press duration the time-sensitive
                    // modes use below: a quick click launches, a hold opens
                    // the No Selection quad (docs/quad-menu.md §3.4).
                    if let Some(task) = self.quad_right_click_takeover(click_pos, held_ms) {
                        return task;
                    }
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                // A right-click. What it does is the user's choice (Options →
                // User Preferences, SHORTCUTMENU in commercial solutions):
                //  • Shortcut menu — always open the context menu, whose
                //    default row (Enter / Repeat) sits under the pointer.
                //  • Time-sensitive — a quick click is Enter while a command
                //    runs (repeat the last command when idle); a held click
                //    opens the menu.
                //  • Enter first — while a command is active the first
                //    right-click acts as Enter and a second consecutive one
                //    opens the menu; idle always opens the menu. Any other
                //    interaction — a left-click pick or a new command — resets
                //    that cycle so the next right-click is Enter again.
                let has_cmd = self.tabs[i].active_cmd.is_some();
                let open_menu = match self.right_click_mode {
                    super::super::super::settings::RightClickMode::ShortcutMenu => true,
                    super::super::super::settings::RightClickMode::TimeSensitive => {
                        held_ms >= self.right_click_hold_ms
                    }
                    super::super::super::settings::RightClickMode::EnterFirst => {
                        !(has_cmd && !sel.input.right_click_entered)
                    }
                };
                if !open_menu {
                    sel.input.right_click_entered = true;
                    drop(sel);
                    // CommandFinalize is Enter during a command and "repeat
                    // the last command" when idle — exactly the quick
                    // right-click.
                    return self.update(Message::CommandFinalize);
                }
                sel.input.right_click_entered = false;
                self.menu_bar_popup = None;
                // A menu closed by a click elsewhere left its editor behind:
                // a new right-click opens the menu itself (#2619).
                self.context_menu_editing = None;
                sel.menu.open(click_pos);
                drop(sel);
                // Take the keyboard away from the command-line field so keys
                // reach the menu through the global subscription; the field
                // is re-focused when the menu closes.
                self.unfocus_widgets()
            }

            Message::ViewportMiddlePress => {
                self.graphic_attributes.open_menu = None;
                let i = self.active_tab;
                self.nav_resync_held();
                self.nav_held.middle = true;
                self.nav_drag_by_button = false;
                self.nav_middle_press = self.tabs[i].scene.selection.borrow().input.last_move_pos;
                self.on_viewport_middle_press()
            }

            Message::ViewportMiddleRelease => {
                self.nav_held.middle = false;
                self.nav_zoom_latched = false;
                let i = self.active_tab;
                // FreeCAD CAD: once the right button is down the orbit goes
                // on without the middle one.
                if self.nav_claimed.right
                    && self
                        .navigation_style
                        .action(
                            self.nav_held,
                            self.shift_down,
                            self.ctrl_down,
                            self.alt_down,
                            true,
                        )
                        .is_some()
                {
                    self.nav_drag_by_button = true;
                    return Task::none();
                }
                let at = self.tabs[i].scene.selection.borrow().input.last_move_pos;
                let click = matches!(
                    (self.nav_middle_press.take(), at),
                    (Some(from), Some(at)) if (at.x - from.x).hypot(at.y - from.y) < 3.0
                );
                if click && self.navigation_style.middle_click_recenters() {
                    if let Some(at) = at {
                        self.nav_recenter_at(i, at);
                    }
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.input.middle_down = false;
                sel.input.middle_last_pos = None;
                // End of a Shift+MMB orbit — drop the captured pivot so the next
                // gesture recomputes it against the current selection. (#229)
                sel.orbit.pivot = None;
                drop(sel);
                self.arm_hover_after_navigation(i);
                self.refresh_pointer_after_navigation(i)
            }

            Message::ViewportScroll(delta) => self.on_viewport_scroll(delta),

            Message::ViewportClick(viewport) => self.on_viewport_click(viewport),

            // The ribbon knows the window's width from the start, not only
            // after the first resize.
            Message::WindowOpened(id, w, h) => {
                if Some(id) == self.main_window {
                    let resized = self.update(Message::WindowResized(w, h));
                    // The drawn maximize button answers as the system's own,
                    // for the Snap Layouts flyout.
                    #[cfg(target_os = "windows")]
                    if self.draws_title_bar() {
                        use iced::window::raw_window_handle::RawWindowHandle;
                        let snap = iced::window::run(id, |window| {
                            if let Ok(handle) = window.window_handle() {
                                if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                                    // Without it the app draws the edges
                                    // and the button stays its own.
                                    if !crate::ui::title_bar::windows::install(handle.hwnd.get()) {
                                        log::warn!("title bar: subclassing the main window failed");
                                    }
                                }
                            }
                        })
                        .discard();
                        return Task::batch([resized, snap]);
                    }
                    return resized;
                }
                Task::none()
            }
            Message::WindowResized(w, h) => {
                self.vp_size = ((w - 440.0).max(200.0), h);
                self.win_size = (w, h);
                self.ribbon.set_window_size((w, h));
                // A resize may have maximized or restored the window (a
                // double click, a snap, the system menu): the title bar's
                // maximize button follows.
                match self.main_window {
                    Some(id) if self.draws_title_bar() => {
                        iced::window::is_maximized(id).map(|max| {
                            Message::TitleBar(crate::ui::title_bar::TitleBarMsg::Maximized(max))
                        })
                    }
                    _ => Task::none(),
                }
            }

            Message::ViewCubeSnap(region) => self.on_view_cube_snap(region),
            Message::ViewCubeSnapWorld(region) => self.on_view_cube_snap_world(region),

            Message::ViewCubeHome => {
                let i = self.active_tab;
                self.clear_navigation_hover(i);
                self.tabs[i].scene.remember_current_view();
                let r_ucs = self.tabs[i].scene.viewcube_ucs_mat();
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.home_view(r_ucs));
                } else {
                    self.tabs[i].scene.camera.borrow_mut().home_view(r_ucs);
                }
                self.tabs[i].scene.camera_generation += 1;
                self.command_line
                    .push_output(crate::t!("View: Home").as_ref());
                Task::none()
            }

            Message::ViewCubeRoll(cw) => {
                let i = self.active_tab;
                self.clear_navigation_hover(i);
                self.tabs[i].scene.remember_current_view();
                let ang = if cw {
                    std::f32::consts::FRAC_PI_2
                } else {
                    -std::f32::consts::FRAC_PI_2
                };
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.roll_by(ang));
                } else {
                    self.tabs[i].scene.camera.borrow_mut().roll_by(ang);
                }
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::ViewCubeNudge(dir) => {
                use crate::scene::NudgeDir;
                let (horizontal, positive) = match dir {
                    NudgeDir::Up => (false, false),
                    NudgeDir::Down => (false, true),
                    NudgeDir::Left => (true, false),
                    NudgeDir::Right => (true, true),
                };
                let i = self.active_tab;
                self.clear_navigation_hover(i);
                self.tabs[i].scene.remember_current_view();
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.nudge_90(horizontal, positive));
                } else {
                    self.tabs[i]
                        .scene
                        .camera
                        .borrow_mut()
                        .nudge_90(horizontal, positive);
                }
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::SetViewcubeUcs(name) => {
                let i = self.active_tab;
                let mut changed = false;
                if name.is_empty() || name == "WCS" {
                    self.tabs[i].active_ucs = None;
                    self.command_line
                        .push_output(crate::t!("UCS: World").as_ref());
                    changed = true;
                } else if let Some(named) = self.tabs[i].scene.document.ucss.get(&name).cloned() {
                    self.tabs[i].active_ucs = Some(named);
                    self.command_line
                        .push_output(crate::tf!("UCS: {}", name).as_ref());
                    changed = true;
                }
                if changed {
                    self.commit_active_ucs_change(i, "UCS");
                    self.tabs[i].scene.camera_generation += 1;
                }
                Task::none()
            }

            Message::PluginFrameDrawn => {
                self.on_plugin_frame_drawn();
                Task::none()
            }
            Message::PluginFrameResume => self.resume_plugin_frame(),
            Message::PluginRunWake => self.on_plugin_run_wake(),
            Message::PluginRunCancel => {
                self.cancel_plugin_run();
                Task::none()
            }
            Message::LayoutSettled => {
                // Restore the scene after the notice redraw.
                self.layout_settling = false;
                Task::none()
            }
            Message::GripDwellTick => {
                let i = self.active_tab;
                // Reuse the move-time logic — `p` is the last cursor
                // position the viewport saw, which is also what the
                // hover state was last set with.
                let p = self.tabs[i]
                    .scene
                    .selection
                    .borrow()
                    .input
                    .last_move_pos
                    .unwrap_or(self.cursor_pos);
                self.update_grip_hover(i, p);
                Task::none()
            }

            Message::HoverDwellTick => self.on_hover_dwell_tick(),
            Message::MoveCoalesceTick => self.on_move_coalesce_tick(),
            Message::BoxPreviewTick => self.on_box_preview_tick(),
            Message::PropertiesRefreshTick => self.on_properties_refresh_tick(),

            Message::SnapDwellTick => self.on_snap_dwell_tick(),

            Message::InteractionIndexReady {
                tab_id,
                epoch,
                source,
                wires,
                index,
                build_ms,
            } => {
                if self.active_interaction_index == Some((tab_id, epoch, source)) {
                    self.active_interaction_index = None;
                }
                let installed = self
                    .tabs
                    .iter()
                    .position(|tab| tab.id == tab_id)
                    .is_some_and(|i| {
                        self.tabs[i]
                            .scene
                            .install_prepared_interaction_index(epoch, source, wires, index)
                    });
                if crate::perf::enabled() {
                    crate::perf_record!(
                        "[perf] interaction-index-bg {:>7.1}ms installed={installed}",
                        build_ms,
                    );
                }
                while let Some((
                    queued_tab,
                    queued_epoch,
                    queued_source,
                    queued_wires,
                    screen_height,
                )) = self.queued_interaction_indices.pop_front()
                {
                    let Some(i) = self.tabs.iter().position(|tab| tab.id == queued_tab) else {
                        continue;
                    };
                    let stale = self.tabs[i].scene.geometry_epoch != queued_epoch
                        || std::sync::Arc::as_ptr(&queued_wires) as usize != queued_source;
                    let (wires, screen_height) = if stale {
                        (
                            self.tabs[i].scene.hit_test_wires(),
                            self.tabs[i].scene.selection.borrow().view.vp_size.1,
                        )
                    } else {
                        (queued_wires, screen_height)
                    };
                    if let Some(task) = self.prepare_interaction_index_task(i, wires, screen_height)
                    {
                        return task;
                    }
                }
                if self.active_interaction_index.is_none() && !self.tabs.is_empty() {
                    let i = self.active_tab.min(self.tabs.len() - 1);
                    let wires = self.tabs[i].scene.hit_test_wires();
                    let screen_height = self.tabs[i].scene.selection.borrow().view.vp_size.1;
                    self.prepare_interaction_index_task(i, wires, screen_height)
                        .unwrap_or_else(Task::none)
                } else {
                    Task::none()
                }
            }

            Message::VisibilityPick(idx) => {
                if let Some(popup) = self.visibility_popup.take() {
                    self.apply_visibility_state(popup.insert_handle, idx);
                }
                Task::none()
            }
            Message::TableRowPick(idx) => {
                if let Some(popup) = self.table_popup.take() {
                    self.apply_table_row(popup.insert_handle, idx);
                }
                Task::none()
            }
            Message::TestVisibilityPick(idx) => {
                let i = self.active_tab;
                if let Some(insert) = self.tabs[i].block_test.as_ref().map(|test| test.insert) {
                    self.apply_visibility_state(insert, idx);
                    self.finish_pending_history(i);
                    self.refresh_properties();
                }
                Task::none()
            }
            Message::TestLookupPick(idx) => {
                let i = self.active_tab;
                let Some((insert, baseline)) = self.tabs[i]
                    .block_test
                    .as_ref()
                    .map(|test| (test.insert, test.baseline.clone()))
                else {
                    return Task::none();
                };
                if self.reject_locked_edit(i, insert) {
                    return Task::none();
                }
                let Some(value) = self
                    .test_block_widgets(i)
                    .and_then(|widgets| widgets.lookup)
                    .and_then(|widget| widget.items.get(idx).cloned())
                else {
                    return Task::none();
                };
                let result = crate::scene::block_lookup::evaluate_test_choice(
                    &self.tabs[i].scene.document,
                    baseline.as_ref(),
                    insert,
                    &value,
                );
                match result {
                    Ok(Some(staged)) => {
                        self.push_undo_snapshot(i, "Lookup value");
                        self.apply_action_document_and_bump(i, staged);
                        self.tabs[i].dirty = true;
                        self.finish_pending_history(i);
                        self.refresh_properties();
                    }
                    Ok(None) => {}
                    Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
                }
                Task::none()
            }
            Message::GripMenuPick(idx) => self.on_grip_menu_pick(idx),

            // ── Snap / mode toggles ───────────────────────────────────────
            Message::ToggleSnapEnabled => {
                self.snapper.toggle_global();
                self.sync_vport_display(self.active_tab);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ToggleSnap3dEnabled => {
                self.snapper.toggle_snap3d();
                self.sync_vport_display(self.active_tab);
                Task::none()
            }
            Message::ToggleGridSnap => {
                self.snapper.toggle_grid_snap();
                self.sync_vport_display(self.active_tab);
                let snap_on = self.snapper.grid_snap();
                self.show_toggle_in_drafting_settings(|state| state.snap_on = snap_on);
                Task::none()
            }
            Message::ToggleIsometricDrafting => {
                self.isometric_drafting = !self.isometric_drafting;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::SetIsoPlane(plane) => {
                self.isometric_drafting = true;
                self.iso_plane = plane;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::CycleIsoPlane => {
                if self.isometric_drafting {
                    self.iso_plane = self.iso_plane.next();
                } else {
                    self.isometric_drafting = true;
                }
                self.command_line.push_output(
                    crate::tf!("Isometric plane: {}.", self.iso_plane.label()).as_ref(),
                );
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ResetDraftingRotation => {
                self.snap_angle_deg = 0.0;
                let i = self.active_tab;
                if self.tabs[i].active_ucs.is_some() {
                    self.tabs[i].active_ucs = None;
                    self.commit_active_ucs_change(i, "UCS");
                    self.tabs[i].scene.camera_generation += 1;
                }
                self.command_line
                    .push_output(crate::t!("Drafting rotation reset to World at 0°.").as_ref());
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ToggleGrid => {
                self.show_grid ^= true;
                self.sync_vport_display(self.active_tab);
                let grid_on = self.show_grid;
                self.show_toggle_in_drafting_settings(|state| state.grid_on = grid_on);
                Task::none()
            }
            Message::ToggleOrtho => {
                self.ortho_mode ^= true;
                if self.ortho_mode {
                    self.polar_mode = false;
                }
                // If the user manually toggles ortho during a command that
                // suppressed it (e.g. RECTANG), the toggle is permanent —
                // don't restore the pre-command state when the command ends.
                self.rect_suppressed_ortho = false;
                Task::none()
            }
            Message::ToggleLineweightDisplay => {
                let i = self.active_tab;
                if i < self.tabs.len() {
                    let h = &mut self.tabs[i].scene.document.header;
                    h.lineweight_display = !h.lineweight_display;
                    // No retessellate — the wire shader reads the flag from uniforms.
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }
            Message::CycleCoordsMode => {
                // $COORDS 0 (static) → 1 (live absolute) → 2 (polar) → 0.
                let i = self.active_tab;
                if i < self.tabs.len() {
                    let mode = {
                        let h = &mut self.tabs[i].scene.document.header;
                        h.coords_mode = (h.coords_mode + 1).rem_euclid(3);
                        h.coords_mode
                    };
                    self.tabs[i].dirty = true;
                    let label = match mode {
                        0 => "static",
                        2 => "polar",
                        _ => "live",
                    };
                    self.command_line
                        .push_output(crate::tf!("COORDS = {mode} ({label})").as_ref());
                }
                Task::none()
            }
            Message::ResolveOneParametricConflict => {
                self.resolve_one_parametric_conflict();
                Task::none()
            }
            Message::TogglePolar => {
                self.polar_mode ^= true;
                if self.polar_mode {
                    self.ortho_mode = false;
                }
                Task::none()
            }
            Message::ToggleDynInput => {
                self.set_dyn_input(!self.dyn_input);
                Task::none()
            }
            Message::ToggleViewCube => {
                self.show_viewcube ^= true;
                self.ribbon.set_viewcube(self.show_viewcube);
                Task::none()
            }
            Message::ToggleProperties => {
                let id = crate::ui::dock::PanelId::Properties;
                // Open but tucked away (an auto-hiding edge, another group
                // shown): the shortcut brings it into view (#2251).
                if self.dock_panel_visible(id) && !self.dock_panel_on_screen(id) {
                    self.dock_reveal(id);
                    return Task::none();
                }
                let open = !self.dock_panel_visible(id);
                // Hiding the panel takes its lists with it; a flag left behind
                // would keep the keyboard guard swallowing keys.
                self.tabs[self.active_tab].properties.open_block_picker = None;
                self.tabs[self.active_tab].properties.open_choice = None;
                if self.clean_screen {
                    // Shown over clean screen without changing the panel's
                    // normal state.
                    self.clean_screen_properties = open;
                    if !open {
                        return Task::none();
                    }
                }
                self.dock_set_open(id, open)
            }
            Message::ToggleGraphicAttributes => {
                let id = crate::ui::dock::PanelId::GraphicAttributes;
                let open = !self.show_graphic_attributes;
                if open {
                    self.dock.dock_below_properties(id);
                }
                self.dock_set_open(id, open)
            }
            Message::ToggleFileTabs => {
                self.show_file_tabs ^= true;
                self.ribbon.set_file_tabs(self.show_file_tabs);
                self.save_config();
                Task::none()
            }
            Message::ToggleLayoutTabs => {
                self.show_layout_tabs ^= true;
                self.ribbon.set_layout_tabs(self.show_layout_tabs);
                self.save_config();
                Task::none()
            }
            Message::ToggleOTrack => {
                self.snapper.otrack_enabled ^= true;
                if !self.snapper.otrack_enabled {
                    self.snapper.clear_tracking();
                    self.otrack_active = None;
                    self.otrack_cross = None;
                    self.otrack_kind = None;
                }
                Task::none()
            }
            Message::SetPolarAngle(deg) => {
                self.polar_increment_deg = deg;
                self.polar_mode = true;
                self.ortho_mode = false;
                self.polar_popup_open = false;
                Task::none()
            }
            Message::TogglePolarPopup => {
                // MenuBar owns its open state. Reset only the transient field
                // whenever the caret starts a fresh interaction.
                self.polar_custom_input.clear();
                Task::none()
            }
            Message::ClosePolarPopup => {
                self.polar_popup_open = false;
                Task::none()
            }
            Message::PolarCustomInput(s) => {
                self.polar_custom_input = s;
                Task::none()
            }
            Message::SubmitPolarCustom => {
                // Accept any positive angle up to a full turn; ignore garbage.
                if let Ok(v) = self.polar_custom_input.trim().parse::<f32>() {
                    if v > 0.0 && v <= 360.0 {
                        self.polar_increment_deg = v;
                        self.polar_mode = true;
                        self.ortho_mode = false;
                    }
                }
                self.polar_custom_input.clear();
                self.polar_popup_open = false;
                Task::none()
            }
            Message::SetAnnotationScale(scale) => {
                self.scale_ui.popup_open = false;
                let auto_scale = self.annotation_auto_scale;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let previous = tab.scene.displayed_annotation_scale_handle();
                    if let Some(handle) = tab.scene.set_annotation_scale_named(&scale) {
                        if auto_scale > 0 {
                            tab.scene.add_annotation_scale_to_objects(
                                handle,
                                previous,
                                auto_scale as u8,
                            );
                        }
                        tab.dirty = true;
                    }
                }
                Task::none()
            }
            Message::SetViewportScale(scale) => {
                self.scale_ui.popup_open = false;
                let auto_scale = self.annotation_auto_scale;
                // Say why nothing changed instead of leaving the list silent
                // (#2626).
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    match tab.scene.scale_target_viewport() {
                        None => {
                            self.command_line.push_error(
                                crate::t!(
                                    "Select a viewport or double-click into one to set its scale."
                                )
                                .as_ref(),
                            );
                            return Task::none();
                        }
                        Some(viewport) if tab.scene.viewport_display_locked(viewport) => {
                            self.command_line.push_error(
                                crate::t!("The viewport's display is locked; unlock it to change its scale.")
                                    .as_ref(),
                            );
                            return Task::none();
                        }
                        Some(_) => {}
                    }
                }
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let previous = tab.scene.displayed_annotation_scale_handle();
                    if let Some(handle) = tab.scene.set_viewport_scale_named(&scale) {
                        if auto_scale > 0 {
                            tab.scene.add_annotation_scale_to_objects(
                                handle,
                                previous,
                                auto_scale as u8,
                            );
                        }
                        tab.dirty = true;
                    }
                }
                Task::none()
            }
            Message::ToggleAnnotationVisibility => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let value = !tab.scene.annotation_all_visible();
                    tab.scene.set_annotation_all_visible(value);
                    tab.dirty = true;
                }
                Task::none()
            }
            Message::ToggleAnnotationAutoAdd => {
                self.annotation_auto_scale = match self.annotation_auto_scale {
                    0 => 4,
                    value => -value,
                };
                Task::none()
            }
            Message::SyncViewportAnnotationScale => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.scene.sync_viewport_annotation_scale() {
                        tab.dirty = true;
                    }
                }
                Task::none()
            }
            Message::ToggleViewportLock => {
                let i = self.active_tab;
                let scene = &self.tabs[i].scene;
                let lock = scene.viewports_display_locked() == Some(false);
                let handles = scene.lockable_viewports();
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if let Some(document::EntityType::Viewport(vp)) =
                        app.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        vp.status.locked = lock;
                    }
                });
                Task::none()
            }
            Message::ToggleScalePopup => {
                self.scale_ui.popup_open ^= true;
                Task::none()
            }
            Message::CloseScalePopup => {
                self.scale_ui.popup_open = false;
                Task::none()
            }
            Message::ScaleManagerOpen => {
                let i = self.active_tab;
                self.scale_ui.popup_open = false;
                // Snapshot so New / Copy / Delete / edits revert if closed
                // without Apply.
                self.scale_stage_begin();
                self.scale_ui.reset_asking = false;
                self.scale_ui.delete_asking = false;
                self.scale_ui.edit_adding = false;
                // Fallback scales are virtual (no real objects), so they can't
                // be edited or renamed. Materialise the standard set into real
                // staged objects so the manager behaves like a drawing with its
                // own list; the stage reverts them on close unless applied.
                if self.tabs[i].scene.ensure_real_scale_list() {
                    self.scale_stage_materialized();
                }
                self.scale_ui.rename = None;
                let cur = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_annotation_scale
                    .clone();
                // Select the current scale, or the first one if it isn't listed.
                if self.tabs[i].scene.scale_paper_drawing(&cur).is_some() {
                    self.load_scale_editor(&cur);
                } else if let Some((first, _, _)) =
                    self.tabs[i].scene.scale_list().into_iter().next()
                {
                    self.load_scale_editor(&first);
                }
                self.scale_ui.multi = vec![self.scale_ui.selected.clone()];
                self.active_modal = Some(crate::app::ModalKind::ScaleManager);
                Task::none()
            }
            Message::AnnoObjectScaleOpen => {
                // The dialog edits the selected objects' per-scale memberships.
                let i = self.active_tab;
                let handles: Vec<_> = self
                    .property_target_handles(i)
                    .into_iter()
                    .filter(|handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(*handle)
                            .is_some_and(crate::scene::annotative::supports_annotation_context)
                    })
                    .collect();
                if handles.is_empty() {
                    self.command_line.push_info(
                        crate::t!("The selected object does not support annotation scales.")
                            .as_ref(),
                    );
                } else {
                    self.anno_object_scale_target = Some(handles[0]);
                    self.anno_object_scale_extra = handles[1..].to_vec();
                    let document = &self.tabs[i].scene.document;
                    self.anno_object_scale = crate::ui::style::anno_object_scale::State {
                        staged: handles
                            .iter()
                            .map(|handle| {
                                let names = crate::scene::annotative::object_scale_memberships(
                                    document, *handle,
                                )
                                .into_iter()
                                .map(|(name, _)| name)
                                .collect();
                                (*handle, names)
                            })
                            .collect(),
                        order: self.tabs[i].scene.scale_names_in_list_order(),
                        ..Default::default()
                    };
                    self.active_modal = Some(crate::app::ModalKind::AnnoObjectScale);
                }
                Task::none()
            }
            Message::AnnoObjectScaleCommon(common) => {
                self.anno_object_scale.common_only = common;
                // The shared list opens on its first scale when none of the
                // selected ones is in it.
                if common && self.anno_object_scale.selected_listed().is_empty() {
                    self.anno_object_scale.selected = self
                        .anno_object_scale
                        .listed()
                        .into_iter()
                        .take(1)
                        .collect();
                }
                Task::none()
            }
            Message::AnnoObjectScaleSelect(name) => {
                let (ctrl, shift) = (self.ctrl_down, self.shift_down);
                self.anno_object_scale.click(name, ctrl, shift);
                Task::none()
            }
            Message::AnnoObjectScaleAdd => {
                self.anno_object_scale.adding = Some(Vec::new());
                Task::none()
            }
            Message::AnnoObjectScaleAddToggle(name) => {
                if let Some(ticked) = self.anno_object_scale.adding.as_mut() {
                    if let Some(at) = ticked.iter().position(|t| t.eq_ignore_ascii_case(&name)) {
                        ticked.remove(at);
                    } else {
                        ticked.push(name);
                    }
                }
                Task::none()
            }
            Message::AnnoObjectScaleAddCancel => {
                self.anno_object_scale.adding = None;
                Task::none()
            }
            Message::AnnoObjectScaleAddOk => {
                // The ticked scales go to every edited object lacking them,
                // in the drawing's list order.
                let ticked = self.anno_object_scale.adding.take().unwrap_or_default();
                let order = self.tabs[self.active_tab].scene.scale_names_in_list_order();
                for name in order
                    .iter()
                    .filter(|name| ticked.iter().any(|t| t.eq_ignore_ascii_case(name)))
                {
                    for (_, scales) in &mut self.anno_object_scale.staged {
                        if !scales.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                            scales.push(name.clone());
                        }
                    }
                }
                Task::none()
            }
            Message::AnnoObjectScaleDelete => {
                if self.anno_object_scale.can_delete() {
                    self.anno_object_scale.delete_picked();
                }
                Task::none()
            }
            Message::AnnoObjectScaleOk => {
                self.apply_anno_object_scale();
                self.close_active_modal();
                Task::none()
            }
            Message::PropHyperlinkOpen => {
                let i = self.active_tab;
                self.hyperlink_editor_block = None;
                let handles = self.property_target_handles(i);
                if handles.is_empty() && self.open_block_property_hyperlink(i) {
                    return Task::none();
                }
                if handles.is_empty() {
                    return Task::none();
                }
                self.open_hyperlink_dialog(i, handles);
                Task::none()
            }
            Message::HyperlinkUrlChanged(value) => {
                self.hyperlink_editor_url = value;
                self.hyperlink_editor_dirty = true;
                Task::none()
            }
            Message::HyperlinkDescriptionChanged(value) => {
                self.hyperlink_editor_description = value;
                self.hyperlink_editor_dirty = true;
                Task::none()
            }
            Message::HyperlinkApply => {
                if !self.hyperlink_editor_dirty {
                    self.close_active_modal();
                    return Task::none();
                }
                let url = self.hyperlink_editor_url.trim().to_owned();
                if self.hyperlink_editor_mixed
                    && url.is_empty()
                    && !self.hyperlink_options.view_target
                {
                    self.command_line.push_info(
                        crate::t!("Enter a URL, or use Remove to clear all hyperlinks.").as_ref(),
                    );
                    return Task::none();
                }
                let description = self.hyperlink_editor_description.trim().to_owned();
                if self.hyperlink_editor_block.is_some() {
                    return self.apply_block_property_hyperlink(&url, &description);
                }
                if let Some(block_def) = self.block_definition.as_mut() {
                    block_def.hyperlink_url = url;
                    block_def.hyperlink_desc = description;
                    self.active_modal = Some(crate::app::ModalKind::BlockDefinition);
                    return Task::none();
                }
                let link = self.hyperlink_from_dialog();
                let i = self.active_tab;
                let handles = self.hyperlink_editor_handles.clone();
                self.apply_property_op(i, "HYPERLINK", &handles, |app, handle| {
                    let document = &mut app.tabs[i].scene.document;
                    if link.is_empty() {
                        crate::scene::hyperlink::set(document, handle, None);
                    } else {
                        crate::scene::hyperlink::set(document, handle, Some(&link));
                    }
                });
                self.close_active_modal();
                Task::none()
            }
            Message::HyperlinkRemove => {
                if self.hyperlink_editor_block.is_some() {
                    return self.apply_block_property_hyperlink("", "");
                }
                if let Some(block_def) = self.block_definition.as_mut() {
                    block_def.hyperlink_url.clear();
                    block_def.hyperlink_desc.clear();
                    self.active_modal = Some(crate::app::ModalKind::BlockDefinition);
                    return Task::none();
                }
                let i = self.active_tab;
                let handles = self.hyperlink_editor_handles.clone();
                self.apply_property_op(i, "HYPERLINK", &handles, |app, handle| {
                    crate::scene::view::dispatch::set_entity_xdata(
                        &mut app.tabs[i].scene.document,
                        handle,
                        "PE_URL",
                        None,
                    );
                });
                self.close_active_modal();
                Task::none()
            }
            Message::HyperlinkCancel => {
                if self.block_definition.is_some() {
                    self.active_modal = Some(crate::app::ModalKind::BlockDefinition);
                    return Task::none();
                }
                self.close_active_modal();
                Task::none()
            }
            Message::AnnoObjectScaleToggle(name) => {
                let i = self.active_tab;
                let Some(entity) = self.anno_object_scale_target else {
                    self.command_line.push_info(
                        crate::t!("Select one object first, then run OBJECTSCALE.").as_ref(),
                    );
                    return Task::none();
                };
                if self.tabs[i].scene.is_layer_locked(entity) {
                    self.command_line
                        .push_info(crate::t!("The object is on a locked layer.").as_ref());
                    return Task::none();
                }
                if let Some(sh) = self.tabs[i].scene.scale_handle_ensuring(&name) {
                    self.push_undo_snapshot(i, "OBJECTSCALE");
                    let mut targets = vec![entity];
                    targets.extend(self.anno_object_scale_extra.iter().copied());
                    targets.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
                    let doc = &mut self.tabs[i].scene.document;
                    let has = |doc: &document::Document, handle| {
                        crate::scene::annotative::object_scale_memberships(doc, handle)
                            .iter()
                            .any(|(_, h)| *h == sh)
                    };
                    // A scale every object has is taken off them all (none
                    // loses its last scale); otherwise the others get it.
                    let all = targets.iter().all(|handle| has(doc, *handle));
                    for handle in &targets {
                        if all {
                            if crate::scene::annotative::object_scale_memberships(doc, *handle)
                                .len()
                                > 1
                            {
                                crate::scene::annotative::remove_annotation_context_for_scale(
                                    doc, *handle, sh,
                                );
                            }
                        } else if !has(doc, *handle) {
                            crate::scene::annotative::create_annotation_context(doc, *handle, sh);
                        }
                    }
                    self.tabs[i].dirty = true;
                    let changes: Vec<_> = targets
                        .iter()
                        .map(|handle| (*handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.invalidate_property_targets(i, &targets);
                    self.refresh_properties();
                } else {
                    self.command_line
                        .push_info(crate::tf!("Unknown annotation scale '{name}'.").as_ref());
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
