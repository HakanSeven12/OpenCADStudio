use super::super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_styles_plot(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::PluginRepoRemove(repo) => {
                self.plugin_repos.retain(|r| r != &repo);
                self.repo_release_tags.remove(&repo);
                self.repo_selected_tag.remove(&repo);
                if self.selected_plugin_repo.as_deref() == Some(repo.as_str())
                    && !self.plugin_registry.iter().any(|entry| entry.repo == repo)
                {
                    self.selected_plugin_repo =
                        self.plugin_registry.first().map(|entry| entry.repo.clone());
                }
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::PluginRegistryFetched(Ok(entries)) => {
                self.plugin_registry_loading = false;
                self.plugin_registry_error = None;
                self.plugin_registry_error_details_open = false;
                // Fetch releases for every curated repo so the dropdowns fill in.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if self.selected_plugin_repo.is_none() {
                        self.selected_plugin_repo = self
                            .external_plugins
                            .iter()
                            .find_map(|plugin| {
                                plugin.repository.clone().or_else(|| {
                                    entries
                                        .iter()
                                        .find(|entry| entry.name.eq_ignore_ascii_case(&plugin.name))
                                        .map(|entry| entry.repo.clone())
                                })
                            })
                            .or_else(|| entries.first().map(|entry| entry.repo.clone()));
                    }
                    let mut tasks: Vec<_> = entries
                        .iter()
                        .map(|e| self.fetch_releases_task(e.repo.clone()))
                        .collect();
                    self.plugin_registry = entries;
                    if let Some(repo) = self.selected_plugin_repo.clone() {
                        if !self.plugin_readmes.contains_key(&repo)
                            && self.plugin_readme_loading.insert(repo.clone())
                        {
                            tasks.push(self.fetch_plugin_readme_task(repo));
                        }
                    }
                    Task::batch(tasks)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    self.plugin_registry = entries;
                    Task::none()
                }
            }
            Message::PluginRegistryFetched(Err(e)) => {
                self.plugin_registry_loading = false;
                self.plugin_registry_error = Some(e);
                self.plugin_registry_error_details_open = false;
                Task::none()
            }
            Message::PluginRegistryRetry => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.plugin_registry_loading = true;
                    self.plugin_registry_error = None;
                    self.plugin_registry_error_details_open = false;
                    self.fetch_registry_task()
                }
                #[cfg(target_arch = "wasm32")]
                Task::none()
            }
            Message::PluginRegistryErrorDetailsToggle => {
                if self.plugin_registry_error.is_some() {
                    self.plugin_registry_error_details_open =
                        !self.plugin_registry_error_details_open;
                }
                Task::none()
            }
            Message::PluginRegistryCopyDiagnostics => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(error) = &self.plugin_registry_error {
                    return iced::clipboard::write(format!(
                        "Open CAD Studio v{}\nOS: {}\nArchitecture: {}\nRegistry: {}\nError: {}",
                        env!("OCS_FULL_VERSION"),
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                        crate::plugin::marketplace::REGISTRY_URL,
                        error,
                    ))
                    .discard();
                }
                Task::none()
            }
            Message::PatronsFetched(Ok(names)) => {
                // Merge the hand-maintained supporters and rank everyone by
                // amount (also sorts the web list, which arrives unsorted).
                self.patrons = crate::patreon::merge_manual(names);
                Task::none()
            }
            // No token / offline: still show any hand-maintained supporters
            // (Start page shows a "Support on Patreon" prompt when empty).
            Message::PatronsFetched(Err(_)) => {
                self.patrons = crate::patreon::merge_manual(Vec::new());
                Task::none()
            }
            Message::VideosFetched(Ok(videos)) => {
                self.videos_loading = false;
                self.set_videos(videos);
                Task::none()
            }
            // Offline / markup change: keep whatever the on-disk cache seeded.
            Message::VideosFetched(Err(_)) => {
                self.videos_loading = false;
                Task::none()
            }
            Message::DiscussionsFetched(Ok(discussions)) => {
                self.discussions_loading = false;
                self.discussions = discussions;
                Task::none()
            }
            // Offline: keep the native cache (web leaves the panel empty).
            Message::DiscussionsFetched(Err(_)) => {
                self.discussions_loading = false;
                Task::none()
            }
            Message::RecentThumbsLoaded(thumbs) => {
                for (path, handle) in thumbs {
                    self.recent_thumbs.insert(path, handle);
                }
                Task::none()
            }
            Message::PluginReleasesFetched(repo, Ok(releases)) => {
                self.plugin_releases_loading.remove(&repo);
                if let Some(first) = releases.first() {
                    self.repo_selected_tag
                        .entry(repo.clone())
                        .or_insert_with(|| first.tag.clone());
                }
                if self.marketplace_status
                    == crate::tf!("Fetching releases for {repo}…").into_owned()
                {
                    self.marketplace_status = crate::tf!(
                        "Repository added. {} installable release(s) found.",
                        releases.len()
                    )
                    .into_owned();
                }
                self.repo_release_tags.insert(repo, releases);
                Task::none()
            }
            Message::PluginReleasesFetched(repo, Err(e)) => {
                self.plugin_releases_loading.remove(&repo);
                self.marketplace_status = format!("{repo}: {e}");
                Task::none()
            }
            Message::PluginReleaseSelect(repo, tag) => {
                self.repo_selected_tag.insert(repo, tag);
                Task::none()
            }
            Message::PluginReadmeSelect(repo) => {
                self.selected_plugin_repo = Some(repo.clone());
                if self.plugin_readme_loading.contains(&repo) {
                    return Task::none();
                }
                if matches!(self.plugin_readmes.get(&repo), Some(Ok(_))) {
                    return Task::none();
                }
                // A second click on an error state acts as retry.
                self.plugin_readmes.remove(&repo);
                self.plugin_readme_loading.insert(repo.clone());
                self.fetch_plugin_readme_task(repo)
            }
            Message::PluginReadmeFetched(repo, result) => {
                self.plugin_readme_loading.remove(&repo);
                self.plugin_readmes.insert(
                    repo,
                    result.map(|source| iced::widget::markdown::Content::parse(&source)),
                );
                Task::none()
            }
            Message::PluginInstall(repo) => {
                let Some(tag) = self.repo_selected_tag.get(&repo).cloned() else {
                    return Task::none();
                };
                if !self.plugin_installing.insert(repo.clone()) {
                    return Task::none();
                }
                self.plugin_installed_now.insert(repo.clone(), true);
                self.marketplace_status = crate::tf!("Installing {repo} {tag}…").into_owned();
                self.install_task(repo, tag)
            }
            Message::PluginUpdate(repo, tag) => {
                if !self.plugin_installing.insert(repo.clone()) {
                    return Task::none();
                }
                self.plugin_installed_now.insert(repo.clone(), false);
                self.marketplace_status = crate::tf!("Updating {repo} to {tag}…").into_owned();
                self.install_task(repo, tag)
            }
            Message::PluginInstalled(repo, Ok(id)) => {
                self.plugin_installing.remove(&repo);
                self.marketplace_status =
                    crate::tf!("Installed '{id}'. Restart to load it.").into_owned();
                for id in id.split(", ") {
                    self.plugin_load_errors.remove(id);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.external_plugins = crate::plugin::external::discover();
                    self.pending_plugin_approvals =
                        crate::plugin::external::collect_pending_approvals(&self.external_plugins);
                }
                Task::none()
            }
            Message::PluginInstalled(repo, Err(e)) => {
                self.plugin_installing.remove(&repo);
                self.plugin_installed_now.remove(&repo);
                self.marketplace_status = crate::tf!("Install failed: {e}").into_owned();
                Task::none()
            }
            Message::PluginRestart => {
                #[cfg(not(target_arch = "wasm32"))]
                super::super::super::RESTART_AFTER_EXIT.store(true, std::sync::atomic::Ordering::Relaxed);
                self.request_quit_with_block_edits()
            }
            Message::PluginUninstall(id) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Stop the plugin runner first so Windows releases the DLL
                    // and allows the package directory to be deleted.
                    if !crate::plugin::external::remove_plugin(&id) {
                        self.marketplace_status =
                            crate::tf!("Uninstall failed: plugin '{id}' did not stop in time")
                                .into_owned();
                        return Task::none();
                    }
                    match crate::plugin::external::uninstall(&id) {
                        Ok(()) => {
                            self.marketplace_status =
                                crate::tf!("Uninstalled '{id}'.").into_owned();
                            self.plugin_load_errors.remove(&id);
                            self.loaded_plugin_ids.remove(&id);
                            self.pending_plugin_approvals.retain(|a| a.id != id);
                            self.rebuild_ribbon_modules();
                            self.external_plugins = crate::plugin::external::discover();
                        }
                        Err(e) => {
                            self.marketplace_status =
                                crate::tf!("Uninstall failed: {e}").into_owned();
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                let _ = id;
                Task::none()
            }
            Message::PluginApprove(id) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.external_plugins = crate::plugin::external::discover();
                    if let Some(plugin) = self
                        .external_plugins
                        .iter()
                        .find(|plugin| plugin.id == id)
                        .cloned()
                    {
                        match crate::plugin::external::approve_plugin(&plugin).and_then(|_| {
                            crate::plugin::external::load_single_approved(self, &plugin)
                        }) {
                            Ok(loaded_id) => {
                                self.plugin_load_errors.remove(&id);
                                self.plugin_load_errors.remove(&loaded_id);
                                self.loaded_plugin_ids =
                                    crate::plugin::external::loaded_ids().into_iter().collect();
                                self.refresh_plugin_tools();
                                self.rebuild_ribbon_modules();
                                self.marketplace_status =
                                    crate::tf!("Trusted and loaded '{loaded_id}'.").into_owned();
                            }
                            Err(err) => {
                                self.plugin_load_errors.insert(id.clone(), err.clone());
                                self.marketplace_status =
                                    crate::tf!("Approval failed for '{id}': {err}").into_owned();
                            }
                        }
                    }
                    self.pending_plugin_approvals =
                        crate::plugin::external::collect_pending_approvals(&self.external_plugins);
                    if self.active_modal == Some(super::super::super::ModalKind::PluginConsent)
                        && self.pending_plugin_approvals.is_empty()
                    {
                        self.close_active_modal();
                    }
                }
                #[cfg(target_arch = "wasm32")]
                let _ = id;
                Task::none()
            }
            Message::PluginConsentDismiss => {
                self.close_active_modal();
                Task::none()
            }
            Message::PointStyleSetMode(mode) => {
                self.set_point_mode_bits(!0, mode);
                Task::none()
            }
            Message::PointStyleSizeRelative(relative) => {
                self.point_size_relative = relative;
                Task::none()
            }
            Message::PointStyleSizeInput(s) => {
                self.point_size_buf = s;
                Task::none()
            }
            Message::PointStyleApplySize => {
                self.validate_point_size_input();
                Task::none()
            }
            Message::PointStyleOk => {
                if self.commit_point_style_dialog() {
                    self.close_active_modal();
                }
                Task::none()
            }

            Message::EnterViewport(handle) => {
                let i = self.active_tab;
                let context_changed = self.tabs[i].scene.active_viewport != Some(handle);
                // Ctrl+R and CHSPACE's target choice move within the command.
                let keep_command = std::mem::take(&mut self.viewport_switch_keeps_command)
                    || self.chspace_pending.is_some();
                let cancel_task = if context_changed && !keep_command {
                    self.cancel_active_command_for_space_change()
                } else {
                    Task::none()
                };
                let perf = crate::perf::enabled();
                let total = Instant::now();
                if context_changed {
                    self.tabs[i].scene.clear_preview_wire();
                }
                // Clear paper-space selection before entering model space.
                if !keep_command {
                    self.tabs[i].scene.deselect_all();
                }
                self.tabs[i].scene.active_viewport = Some(handle);
                // Fold a stale UTM saved view onto the effective (auto-fit)
                // centre so pan/zoom, paper↔model and the display all agree —
                // otherwise the camera auto-fits to the model while the cursor
                // math stays at the origin, jittering as pan toggles the two.
                let phase = Instant::now();
                self.tabs[i].scene.normalize_active_viewport_view();
                let normalize_ms = phase.elapsed().as_secs_f64() * 1000.0;
                // Grid/snap follow the entered viewport.
                let phase = Instant::now();
                self.adopt_view_display(i);
                let display_ms = phase.elapsed().as_secs_f64() * 1000.0;
                // Adopt the entered viewport's own per-viewport UCS.
                let phase = Instant::now();
                self.tabs[i].refresh_active_ucs();
                let ucs_ms = phase.elapsed().as_secs_f64() * 1000.0;
                let phase = Instant::now();
                self.refresh_properties();
                let properties_ms = phase.elapsed().as_secs_f64() * 1000.0;
                // The entered viewport is now the space being walked.
                self.walk_space_changed();
                self.command_line.push_output(crate::t!("MSPACE").as_ref());
                if perf {
                    crate::perf_record!(
                        "[perf] viewport-enter total={:.2}ms normalize={:.2}ms display={:.2}ms ucs={:.2}ms properties={:.2}ms handle={}",
                        total.elapsed().as_secs_f64() * 1000.0,
                        normalize_ms,
                        display_ms,
                        ucs_ms,
                        properties_ms,
                        handle.value(),
                    );
                }
                if context_changed {
                    self.sync_dyn_fields();
                }
                cancel_task
            }

            Message::ExitViewport => {
                let i = self.active_tab;
                let context_changed = self.tabs[i].scene.active_viewport.is_some();
                let keep_command = std::mem::take(&mut self.viewport_switch_keeps_command);
                let cancel_task = if context_changed && !keep_command {
                    self.cancel_active_command_for_space_change()
                } else {
                    Task::none()
                };
                if context_changed {
                    self.tabs[i].scene.clear_preview_wire();
                }
                // Clear model-space selection before returning to paper space.
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.active_viewport = None;
                // Grid/snap return to the paper sheet's own state.
                self.adopt_view_display(i);
                // Paper space has no UCS — drop the viewport's UCS.
                self.tabs[i].refresh_active_ucs();
                self.refresh_properties();
                // Back on the sheet, the walk follows the space it belongs to.
                self.walk_space_changed();
                self.command_line.push_output(crate::t!("PSPACE").as_ref());
                if context_changed {
                    self.sync_dyn_fields();
                }
                cancel_task
            }

            Message::MspaceCommand => {
                let i = self.active_tab;
                if self.tabs[i].scene.current_layout == "Model" {
                    self.command_line.push_error(
                        crate::t!("MS is only available in paper space layouts.").as_ref(),
                    );
                    return Task::none();
                }
                if self.tabs[i].scene.active_viewport.is_some() {
                    // Already in MSPACE — nothing to do.
                    return Task::none();
                }
                match self.tabs[i].scene.first_user_viewport() {
                    Some(handle) => Task::done(Message::EnterViewport(handle)),
                    None => {
                        self.command_line
                            .push_error(crate::t!("No viewport found in this layout.").as_ref());
                        Task::none()
                    }
                }
            }

            Message::PspaceCommand => Task::done(Message::ExitViewport),

            Message::Undo => {
                if self.table_cell_editor.is_some() {
                    self.mtext_undo();
                    return Task::none();
                }
                if self.mtext_editor.is_some() {
                    self.mtext_cancel();
                }
                // Mid-command Ctrl+Z: a drawing command steps itself back
                // (PLINE pops the last vertex) instead of the document undo
                // swallowing the whole in-progress object.
                let i = self.active_tab;
                let step = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .and_then(|c| c.on_undo_step());
                if let Some(r) = step {
                    return self.apply_cmd_result(r);
                }
                self.undo_active_tab();
                Task::none()
            }
            Message::Redo => {
                if self.table_cell_editor.is_some() {
                    self.mtext_redo();
                    return Task::none();
                }
                if self.mtext_editor.is_some() {
                    self.mtext_cancel();
                }
                self.redo_active_tab();
                Task::none()
            }

            Message::UndoMany(steps) => {
                if self.mtext_editor.is_some() {
                    self.mtext_cancel();
                }
                self.ribbon.close_dropdown();
                self.undo_steps(steps);
                Task::none()
            }

            Message::RedoMany(steps) => {
                if self.mtext_editor.is_some() {
                    self.mtext_cancel();
                }
                self.ribbon.close_dropdown();
                self.redo_steps(steps);
                Task::none()
            }

            Message::Noop => Task::none(),
            Message::StatusMenuTooltipHidden(hidden) => {
                self.status_menu_tooltip_hidden = hidden;
                if hidden {
                    self.polar_custom_input.clear();
                }
                Task::none()
            }

            // ── Unsaved-changes dialog ────────────────────────────────────
            Message::UnsavedDialogCancel => {
                self.pending_close = None;
                self.pending_tab_closes.clear();
                self.close_unsaved_dialog_window()
            }

            Message::UnsavedDialogDiscard => self.on_unsaved_dialog_discard(),

            Message::UnsavedDialogSave => self.on_unsaved_dialog_save(),
            Message::BlockEditChangesSave => self.resolve_block_edit_close(true),
            Message::BlockSaveAsName(name) => {
                let i = self.active_tab;
                let error = self.validate_block_save_as_name(i, &name).err();
                if let Some(state) = self.block_save_as.as_mut() {
                    state.name = name;
                    state.error = error;
                }
                Task::none()
            }
            Message::BlockSaveAsApply => self.apply_block_save_as_dialog(),
            Message::BlockVisibility(action) => self.handle_block_visibility(action),
            Message::BlockLookup(action) => self.handle_block_lookup(action),
            Message::BlockPropertiesTable(action) => self.handle_block_properties_table(action),
            Message::ParametersManagerEdit(name, cell) => {
                // Whichever order the click's two messages arrive in, the number
                // typed in the cell that was open goes to the parameter it was
                // opened on before this click moves the field somewhere else.
                let _ = self.commit_parameters_manager(false);
                let i = self.active_tab;
                let Some(definition) = self.tabs[i]
                    .active_block_edit_session()
                    .map(|session| session.br_handle)
                else {
                    self.clear_parameters_manager_cell(i);
                    return Task::none();
                };
                let rows = crate::scene::block_parameters_manager::rows(
                    &self.tabs[i].scene.document,
                    definition,
                );
                use crate::ui::window::parameters_manager::Cell;
                let Some(row) = rows
                    .iter()
                    .find(|row| row.name.eq_ignore_ascii_case(&name))
                    .filter(|row| match cell {
                        Cell::Description => row.description_writable,
                        Cell::Order => row.order_writable,
                        Cell::Expression | Cell::Value => row.numeric,
                    })
                else {
                    // Nothing to open here, so nothing stays open: a cell left on
                    // another row would take whatever is typed next.
                    self.clear_parameters_manager_cell(i);
                    return Task::none();
                };
                let target = crate::scene::block_parameters_manager::target_of(
                    &self.tabs[i].scene.document,
                    definition,
                    &row.name,
                );
                let Some((parameter, Some(component))) = target else {
                    self.clear_parameters_manager_cell(i);
                    return Task::none();
                };
                let name = row.name.clone();
                let opened = match cell {
                    Cell::Description => row.description.clone(),
                    Cell::Order => row.order.clone(),
                    Cell::Expression | Cell::Value => String::new(),
                };
                let panel = &mut self.tabs[i].parameters_panel;
                panel.editing = Some(crate::ui::window::parameters_manager::Edit {
                    definition,
                    parameter,
                    component,
                    name,
                    cell,
                });
                // A text field opens on the text it replaces, as an in-place
                // edit does; a numeric field starts empty so a typed number
                // replaces the printed one without first deleting it.
                panel.edit_buf = opened.clone();
                panel.edit_original = opened;
                iced::widget::operation::focus(
                    crate::ui::window::parameters_manager::edit_input_id(),
                )
            }
            Message::ParametersManagerInput(value) => {
                let panel = &mut self.tabs[self.active_tab].parameters_panel;
                if panel.editing.is_some() {
                    panel.edit_buf = value;
                }
                Task::none()
            }
            // The two fields narrow the list and the footer's count; they write
            // no parameter.
            Message::ParametersManagerFilter(value) => {
                self.tabs[self.active_tab].parameters_panel.filter = value;
                Task::none()
            }
            Message::ParametersManagerFilterMode(mode) => {
                self.tabs[self.active_tab].parameters_panel.mode = mode;
                self.tabs[self.active_tab].parameters_panel.filter_menu = false;
                Task::none()
            }
            Message::ParametersManagerFilterMenu => {
                let panel = &mut self.tabs[self.active_tab].parameters_panel;
                panel.filter_menu = !panel.filter_menu;
                Task::none()
            }
            Message::ParametersManagerColumn(column) => {
                let panel = &mut self.tabs[self.active_tab].parameters_panel;
                let index = column.index();
                panel.columns[index] = !panel.columns[index];
                Task::none()
            }
            Message::ParametersManagerSearch(value) => {
                self.tabs[self.active_tab].parameters_panel.search = value;
                Task::none()
            }
            Message::ParametersManagerCommit => self.commit_parameters_manager(true),
            Message::ParametersManagerShow(name) => {
                let _ = self.commit_parameters_manager(false);
                self.toggle_parameters_manager_show(&name)
            }
            // A click moved focus off the open cell: keep what was typed, as
            // Enter would. An untouched numeric field stays open, an untouched
            // description keeps the text it opened on, and an emptied
            // description is a text to write.
            Message::ParametersManagerFocus(focused) => {
                let commit = {
                    let panel = &self.tabs[self.active_tab].parameters_panel;
                    let Some(edit) = panel.editing.as_ref() else {
                        return Task::none();
                    };
                    let typed = if edit.cell.is_text() {
                        panel.edit_buf != panel.edit_original
                    } else {
                        !panel.edit_buf.trim().is_empty()
                    };
                    typed && focused != Some(crate::ui::window::parameters_manager::edit_input_id())
                };
                if commit {
                    return self.commit_parameters_manager(false);
                }
                Task::none()
            }
            Message::BlockEditPickerName(name) => {
                if let Some(state) = self.block_edit_picker.as_mut() {
                    state.name = name.clone();
                    let trimmed = name.trim();
                    state.error = if trimmed.is_empty() {
                        None
                    } else {
                        crate::scene::block_service::validate_block_name(trimmed)
                            .err()
                            .map(|e| e.to_string())
                    };
                    if let Some(tab_idx) = self.tabs.iter().position(|t| t.id == state.document_id)
                    {
                        if let Some(matched) = state
                            .existing_names
                            .iter()
                            .find(|n| n.eq_ignore_ascii_case(trimmed))
                        {
                            state.preview = self.tabs[tab_idx].scene.block_preview_wires(matched);
                            state.description = self.tabs[tab_idx]
                                .scene
                                .document
                                .block_records
                                .get(matched)
                                .map(|b| b.description.clone())
                                .unwrap_or_default();
                        } else {
                            state.preview.clear();
                            state.description.clear();
                        }
                    }
                }
                Task::none()
            }
            Message::BlockEditPickerSelect(name) => {
                if let Some(state) = self.block_edit_picker.as_mut() {
                    state.name = name.clone();
                    state.error = None;
                    if let Some(tab_idx) = self.tabs.iter().position(|t| t.id == state.document_id)
                    {
                        state.preview = self.tabs[tab_idx].scene.block_preview_wires(&name);
                        state.description = self.tabs[tab_idx]
                            .scene
                            .document
                            .block_records
                            .get(&name)
                            .map(|b| b.description.clone())
                            .unwrap_or_default();
                    }
                }
                Task::none()
            }
            Message::BlockEditPickerApply => self.apply_bedit_dialog(),
            Message::Rename(msg) => self.on_rename_dialog(msg),
            Message::DesignCenter(msg) => self.on_design_center(msg),
            Message::Purge(msg) => self.on_purge_dialog(msg),
            Message::DwgProps(msg) => self.on_drawing_properties(msg),
            Message::GeographicLocation(msg) => self.on_geographic_location(msg),
            Message::SelectSimilarSettingsToggle(bit) => {
                if let Some(mode) = self.select_similar_settings.as_mut() {
                    *mode ^= bit;
                }
                Task::none()
            }
            Message::SelectSimilarSettingsOk => {
                if let Some(mode) = self.select_similar_settings {
                    self.select_similar_mode = mode;
                    self.persist_settings_if_changed();
                }
                self.close_active_modal();
                Task::none()
            }
            Message::BlockEditChangesDiscard => self.resolve_block_edit_close(false),
            Message::BlockEditChangesCancel => {
                self.cancel_block_edit_close();
                Task::none()
            }

            Message::AecDropSameVersion => self.on_aec_drop_same_version(),
            Message::AecDropProceed => self.on_aec_drop_proceed(),
            Message::AecDropBack => {
                self.active_modal = Some(crate::app::ModalKind::SaveDialog);
                Task::none()
            }
            Message::ConversionLossProceed => self.on_conversion_loss_proceed(),
            Message::IfccadAlphaProceed => self.on_ifccad_alpha_proceed(),
            Message::ConversionLossBack => {
                self.active_modal = None;
                self.pending_conversion_save = None;
                self.conversion_losses.clear();
                Task::none()
            }

            Message::AutoSave => self.on_autosave(),

            Message::ThumbnailCaptureFrame => self.on_thumbnail_capture_frame(),

            Message::ThumbnailCaptureFinished => {
                self.thumbnail_capture_clean = false;
                Task::none()
            }

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveFinished(outcome) => {
                let task = self.on_save_finished(outcome);
                if self.terminating && self.active_save_jobs.is_empty() {
                    return self.leave_keeping_recovery();
                }
                task
            }
            // A service manager stopping the program (a container closing at
            // logout): unsaved work goes to the recovery copies, which are
            // kept, and the program leaves once they are written.
            #[cfg(not(target_arch = "wasm32"))]
            Message::Terminate => {
                self.terminating = true;
                let task = self.on_autosave();
                if self.active_save_jobs.is_empty() {
                    return self.leave_keeping_recovery();
                }
                task
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebSaveScreenshot {
                tab_id,
                filename,
                ext,
                version,
                bounds,
                screenshot,
            } => self.on_web_save_screenshot(tab_id, filename, ext, version, bounds, screenshot),

            #[cfg(target_arch = "wasm32")]
            Message::WebSaveSerialized {
                tab_id,
                filename,
                autosave_key,
                version,
                geometry_epoch,
                edit_revision,
                camera_generation,
                result,
            } => self.on_web_save_serialized(
                tab_id,
                filename,
                autosave_key,
                version,
                geometry_epoch,
                edit_revision,
                camera_generation,
                result,
            ),

            #[cfg(target_arch = "wasm32")]
            Message::WebSaveHandlePicked {
                tab_id,
                filename,
                ext,
                version,
                picked,
            } => self.on_web_save_handle_picked(tab_id, filename, ext, version, picked),

            #[cfg(target_arch = "wasm32")]
            Message::WebSaveWritten {
                tab_id,
                filename,
                autosave_key,
                version,
                geometry_epoch,
                edit_revision,
                camera_generation,
                result,
                bytes,
            } => self.on_web_save_written(
                tab_id,
                filename,
                autosave_key,
                version,
                geometry_epoch,
                edit_revision,
                camera_generation,
                result,
                bytes,
            ),

            #[cfg(target_arch = "wasm32")]
            Message::WebSaveallSerialized { skipped, results } => {
                self.on_web_saveall_serialized(skipped, results)
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebArchiveSerialized {
                cmd,
                zip_name,
                filename,
                deps,
                skipped,
                warnings,
                result,
            } => self.on_web_archive_serialized(
                cmd, zip_name, filename, deps, skipped, warnings, result,
            ),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveFileInUseRetry => self.on_save_file_in_use_retry(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveFileInUseSaveAs => self.on_save_file_in_use_save_as(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveFileInUseCancel => {
                self.close_active_modal();
                Task::none()
            }

            #[cfg(not(target_arch = "wasm32"))]
            Message::ExternalChangeReload => self.on_external_change_reload(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::ExternalChangeSaveAs => self.on_external_change_save_as(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::ExternalChangeOverwrite => self.on_external_change_overwrite(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::ExternalChangeCancel => {
                self.close_active_modal();
                Task::none()
            }

            // ── Page Setup ────────────────────────────────────────────────
            Message::UpdateCheckResult(latest) => {
                let Some(info) = latest else {
                    return Task::none();
                };
                self.update_notice_version = Some(info.version);
                self.update_notice_body = Some(info.body);
                self.pending_startup_modals
                    .push_back(super::super::super::ModalKind::UpdateNotice);
                Task::none()
            }
            Message::UpdateCheckManual(latest) => {
                let Some(info) = latest else {
                    self.command_line.push_output(&crate::tr!(
                        "view",
                        "no-newer-release",
                        version = env!("OCS_APP_VERSION")
                    ));
                    return Task::none();
                };
                self.update_notice_version = Some(info.version);
                self.update_notice_body = Some(info.body);
                self.active_modal = Some(super::super::super::ModalKind::UpdateNotice);
                self.reset_modal_geometry();
                Task::none()
            }
            Message::DonationPromptDonate => {
                self.close_active_modal();
                self.dispatch_view("DONATE", self.active_tab)
                    .unwrap_or_else(Task::none)
            }
            Message::UpdateNoticeClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::UpdateNoticeOpenRelease => {
                let open =
                    crate::sys::open_url(crate::io::update_check::DOWNLOAD_PAGE, self.main_window);
                self.close_active_modal();
                open
            }
            Message::AssocPromptYes => {
                self.file_assoc_enabled = true;
                self.mark_assoc_prompted();
                self.close_active_modal();
                // set_default_app registers the handler first, then makes us the
                // default — boot no longer does this automatically.
                Task::perform(
                    crate::io::file_association::set_default_app(),
                    Message::AssocResult,
                )
            }
            Message::AssocPromptNo => {
                self.file_assoc_enabled = false;
                self.mark_assoc_prompted();
                self.close_active_modal();
                Task::none()
            }
            Message::AssocResult(result) => {
                match result {
                    Ok(msg) => self.command_line.push_info(&msg),
                    Err(err) => self
                        .command_line
                        .push_error(crate::tf!("Could not set default app: {err}").as_ref()),
                }
                Task::none()
            }
            Message::PlotDialogOpen => self.on_plot_dialog_open(),
            Message::PlotDlg(m) => self.on_plot_dlg(m),
            Message::BlockPalette(m) => self.on_block_palette(m),
            Message::ToolPalettes(m) => self.on_tool_palettes(m),
            Message::GraphicAttributes(m) => self.on_graphic_attributes(m),
            Message::Dock(m) => self.on_dock(m),
            Message::AuthoringPaletteTabSelected(tab) => {
                if self.dock_panel_visible(crate::ui::dock::PanelId::AuthoringPalettes) {
                    self.authoring_palette_tab = tab;
                }
                Task::none()
            }
            Message::PrintAllOpen => self.on_print_all_open(),
            Message::PrintAllToggle(name) => {
                if let Some((_, selected)) = self
                    .print_all_layouts
                    .iter_mut()
                    .find(|(layout, _)| layout == &name)
                {
                    *selected = !*selected;
                }
                Task::none()
            }
            Message::PrintAllSelectAll => {
                for (_, selected) in &mut self.print_all_layouts {
                    *selected = true;
                }
                Task::none()
            }
            Message::PrintAllSelectNone => {
                for (_, selected) in &mut self.print_all_layouts {
                    *selected = false;
                }
                Task::none()
            }
            Message::PrintAllOptions => self.on_print_all_options(),
            Message::PrintAllPdf => {
                let i = self.active_tab;
                // DXEVAL: publishing compares extraction tables as it starts.
                if let Some(task) = self.evaluate_extraction_tables(
                    i,
                    crate::app::commands::extraction_eval::EVENT_PUBLISH,
                    Some(Message::PrintAllPdf),
                ) {
                    return task;
                }
                let stem = self.tabs[i]
                    .current_path
                    .as_deref()
                    .and_then(|path| path.file_stem())
                    .map(|name| format!("{}_layouts", name.to_string_lossy()))
                    .unwrap_or_else(|| "drawing_layouts".into());
                #[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
                {
                    let Some(window_id) = self.main_window else {
                        return Task::done(Message::PrintAllPdfPath(None));
                    };
                    iced::window::run(window_id, move |parent| {
                        crate::io::pdf_export::pick_plot_path_owned(
                            stem,
                            parent,
                            crate::io::pdf_export::PlotFileKind::Pdf,
                        )
                    })
                    .map(Message::PrintAllPdfPath)
                }
                #[cfg(target_os = "windows")]
                {
                    Task::perform(
                        crate::io::pdf_export::pick_plot_path_async(
                            stem,
                            crate::io::pdf_export::PlotFileKind::Pdf,
                        ),
                        Message::PrintAllPdfPath,
                    )
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Task::perform(
                        crate::io::pdf_export::pick_plot_path_owned(stem),
                        Message::PrintAllPdfPath,
                    )
                }
            }
            Message::PrintAllPdfTo(path) => {
                // DXEVAL: publishing compares extraction tables as it starts.
                match self.evaluate_extraction_tables(
                    self.active_tab,
                    crate::app::commands::extraction_eval::EVENT_PUBLISH,
                    Some(Message::PrintAllPdfTo(path.clone())),
                ) {
                    Some(task) => task,
                    None => self.on_print_all_pdf_path_some(path),
                }
            }
            Message::PrintAllPdfPath(None) => Task::none(),
            Message::PrintAllPdfPath(Some(path)) => self.on_print_all_pdf_path_some(path),
            Message::PrintAllPrint => {
                // DXEVAL: publishing compares extraction tables as it starts.
                match self.evaluate_extraction_tables(
                    self.active_tab,
                    crate::app::commands::extraction_eval::EVENT_PUBLISH,
                    Some(Message::PrintAllPrint),
                ) {
                    Some(task) => task,
                    None => self.on_print_all_print(),
                }
            }
            Message::PrintAllFinished(result) => {
                match result {
                    Ok(message) => self.command_line.push_info(&message),
                    Err(error) => {
                        self.command_line.push_error(&error);
                        if self.active_modal.is_none()
                            && !self.dialog_open(super::super::super::ModalKind::PrintAll)
                        {
                            self.active_modal = Some(super::super::super::ModalKind::PrintAll);
                            self.reset_modal_geometry();
                        }
                    }
                }
                Task::none()
            }

            // ── Plot / Export ─────────────────────────────────────────────
            export @ (Message::PlotExport | Message::PlotExportDwf) => {
                let dwf = matches!(export, Message::PlotExportDwf);
                #[cfg(target_arch = "wasm32")]
                if dwf {
                    self.command_line.push_error(
                        crate::t!("DWF export is not available in the web version.").as_ref(),
                    );
                    return Task::none();
                }
                let (stem, preset) = self.plot_output_target();
                if let Some(path) = preset {
                    let path = if dwf {
                        path.with_extension("dwf")
                    } else {
                        path
                    };
                    return Task::done(Message::PlotExportPath(Some(path)));
                }
                // The save dialog follows the dialog's file device, so the
                // SVG choice proposes `stem.svg` up front; EXPORTDWF
                // proposes `stem.dwf`.
                #[cfg(not(target_arch = "wasm32"))]
                let svg = if self.plot_dialog.file_image && !dwf {
                    crate::io::pdf_export::PlotFileKind::Image
                } else if dwf || self.plot_dialog.file_dwf {
                    crate::io::pdf_export::PlotFileKind::Dwf
                } else if self.plot_dialog.file_svg {
                    crate::io::pdf_export::PlotFileKind::Svg
                } else {
                    crate::io::pdf_export::PlotFileKind::Pdf
                };
                #[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
                {
                    let Some(window_id) = self.main_window else {
                        return Task::done(Message::PlotExportPath(None));
                    };
                    iced::window::run(window_id, move |parent| {
                        crate::io::pdf_export::pick_plot_path_owned(stem, parent, svg)
                    })
                    .map(Message::PlotExportPath)
                }
                #[cfg(target_os = "windows")]
                {
                    Task::perform(
                        crate::io::pdf_export::pick_plot_path_async(stem, svg),
                        Message::PlotExportPath,
                    )
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Task::perform(
                        crate::io::pdf_export::pick_plot_path_owned(stem),
                        Message::PlotExportPath,
                    )
                }
            }
            Message::PlotExportPath(None) => Task::none(),
            Message::PlotExportPath(Some(path)) => self.on_plot_export_path_some(path),

            Message::PlotWindowExport => {
                let (stem, preset) = self.plot_output_target();
                if let Some(path) = preset {
                    return Task::done(Message::PlotWindowExportPath(Some(path)));
                }
                #[cfg(not(target_arch = "wasm32"))]
                let svg = if self.plot_dialog.file_image {
                    crate::io::pdf_export::PlotFileKind::Image
                } else if self.plot_dialog.file_dwf {
                    crate::io::pdf_export::PlotFileKind::Dwf
                } else if self.plot_dialog.file_svg {
                    crate::io::pdf_export::PlotFileKind::Svg
                } else {
                    crate::io::pdf_export::PlotFileKind::Pdf
                };
                #[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
                {
                    let Some(window_id) = self.main_window else {
                        return Task::done(Message::PlotWindowExportPath(None));
                    };
                    iced::window::run(window_id, move |parent| {
                        crate::io::pdf_export::pick_plot_path_owned(stem, parent, svg)
                    })
                    .map(Message::PlotWindowExportPath)
                }
                #[cfg(target_os = "windows")]
                {
                    Task::perform(
                        crate::io::pdf_export::pick_plot_path_async(stem, svg),
                        Message::PlotWindowExportPath,
                    )
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Task::perform(
                        crate::io::pdf_export::pick_plot_path_owned(stem),
                        Message::PlotWindowExportPath,
                    )
                }
            }
            Message::PlotWindowExportPath(None) => Task::none(),
            Message::PlotWindowExportPath(Some(path)) => self.on_plot_window_export_path_some(path),

            Message::BackgroundIoFinished(result, reopen_plot) => {
                match result {
                    Ok(message) => self.command_line.push_info(&message),
                    Err(error) => self.command_line.push_error(&error),
                }
                if reopen_plot {
                    self.active_modal = Some(crate::app::ModalKind::Plot);
                }
                Task::none()
            }

            Message::BackgroundTaskFailed { context, detail } => {
                self.command_line
                    .push_error(crate::tf!("{context} failed: {detail}").as_ref());
                Task::none()
            }
            Message::UnderlayRastersDrawn(tab_id, generation, done) => {
                self.finish_underlay_reseed(tab_id, generation, *done);
                Task::none()
            }
            Message::PointCloudRead(done) => {
                self.finish_point_cloud_display_read(&done);
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::EmbedImageRead(tab_id, generation, read) => {
                self.finish_control_embed_image(tab_id, generation, *read);
                Task::none()
            }

            // ── Print to system printer ───────────────────────────────────────
            Message::PrintToPrinter => self.on_print_to_printer(),
            Message::PrintResult(Ok(printer)) => {
                self.command_line
                    .push_info(crate::tf!("Sent to printer: {printer}").as_ref());
                Task::none()
            }
            Message::PrintResult(Err(e)) => {
                self.command_line
                    .push_error(crate::tf!("Print failed: {e}").as_ref());
                Task::none()
            }

            // ── Plot Style Table ──────────────────────────────────────────────
            Message::LinetypeLoaded(Some((lin_dir, source))) => {
                let i = self.active_tab;
                // In the open Linetype Manager a load is staged like its
                // other changes: Apply keeps it, Close drops it.
                let staged = self.linetypes_are_staged();
                let before_names: rustc_hash::FxHashSet<String> = if staged {
                    rustc_hash::FxHashSet::default()
                } else {
                    self.tabs[i]
                        .scene
                        .document
                        .line_types
                        .iter()
                        .map(|lt| lt.name.to_ascii_uppercase())
                        .collect()
                };
                if !staged {
                    self.push_undo_snapshot(i, "LINETYPE");
                }
                let added = crate::io::linetypes::populate_document_from_source(
                    &mut self.tabs[i].scene.document,
                    &source,
                    lin_dir.as_deref(),
                );
                if staged {
                    self.load_linetype_bufs(i);
                } else if added > 0 {
                    let added_names: Vec<String> = self.tabs[i]
                        .scene
                        .document
                        .line_types
                        .iter()
                        .filter(|lt| !before_names.contains(&lt.name.to_ascii_uppercase()))
                        .map(|lt| lt.name.clone())
                        .collect();
                    self.tabs[i].dirty = true;
                    self.tabs[i]
                        .scene
                        .invalidate_linetype_dependencies(&added_names);
                }
                self.command_line
                    .push_output(crate::tf!("{added} linetype(s) loaded.").as_ref());
                Task::none()
            }
            Message::LinetypeLoaded(None) => Task::none(),
            Message::PlotStyleLoad => {
                let tab = &self.tabs[self.active_tab];
                let id = tab.id;
                let layout = tab.scene.current_layout.clone();
                Task::perform(crate::io::pick_plot_style(), move |result| {
                    Message::PlotStyleLoaded(id, layout.clone(), result)
                })
            }
            Message::PlotStyleLoaded(id, layout, Ok(Some(picked))) => {
                let Some(i) = self.tabs.iter().position(|tab| tab.id == id) else {
                    return Task::none();
                };
                if let Err(error) = self.bind_picked_plot_style(i, &layout, &picked) {
                    self.command_line.push_error(&error);
                    return Task::none();
                }
                if i != self.active_tab || self.tabs[i].scene.current_layout != layout {
                    return Task::none();
                }
                let table = picked.table;
                self.plot_dialog.style_name = table.name.clone();
                self.plot_dialog.apply_plot_styles = true;
                self.plot_dialog.style_missing = false;
                self.plot_dialog.style_error = None;
                self.report_plot_style_warnings(&table);
                self.command_line.push_output(
                    crate::tf!(
                        "Plot style '{}' loaded ({} color entries).",
                        table.name,
                        table
                            .aci_entries
                            .iter()
                            .filter(|e| e.color.is_some())
                            .count()
                    )
                    .as_ref(),
                );
                self.active_plot_style = Some(table);
                if self.dialog_open(super::super::super::ModalKind::Plotstyle) {
                    self.plotstyle_panel_baseline = Some(self.active_plot_style.clone());
                }
                self.plot_dialog.plot_styles = crate::io::plot_style::available_table_names();
                Task::none()
            }
            Message::PlotStyleLoaded(_, _, Ok(None)) => Task::none(),
            Message::PlotStyleLoaded(id, layout, Err(error)) => {
                // The file the user pointed at could not be read: say why,
                // in the dialog as well as on the command line.
                self.command_line.push_error(&error);
                let tab = &self.tabs[self.active_tab];
                if tab.id == id && tab.scene.current_layout == layout {
                    self.plot_dialog.style_error = Some(error);
                }
                Task::none()
            }
            Message::PlotStyleClear => {
                self.active_plot_style = None;
                self.plot_dialog.style_name.clear();
                self.plot_dialog.style_missing = false;
                self.commit_plotstyle_panel_baseline();
                self.command_line
                    .push_output(crate::t!("Plot style table cleared.").as_ref());
                Task::none()
            }

            // ── Plot Style Panel ──────────────────────────────────────────────
            Message::PlotStylePanelOpen => {
                // The Plot dialog's selected table is authoritative. Make sure
                // the editor opens that table rather than a stale active table.
                let selected_style = self.plot_dialog.style_name.clone();

                if selected_style.is_empty() {
                    self.active_plot_style = None;
                } else {
                    let needs_load = self
                        .active_plot_style
                        .as_ref()
                        .is_none_or(|table| !table.name.eq_ignore_ascii_case(&selected_style));

                    if needs_load {
                        let base =
                            crate::io::drawing_dir(&self.tabs[self.active_tab].scene.document);
                        match crate::io::plot_style::PlotStyleTable::load_named_at(
                            &selected_style,
                            base,
                        ) {
                            Ok(table) => {
                                self.report_plot_style_warnings(&table);
                                self.active_plot_style = Some(table);
                                self.plot_dialog.style_missing = false;
                                self.plot_dialog.style_error = None;
                            }
                            Err(error) => {
                                self.plot_dialog.style_missing = true;
                                self.command_line.push_error(&error);
                                self.plot_dialog.style_error = Some(error);
                                return Task::none();
                            }
                        }
                    }
                }
                // Initialise edit buffers for ACI 1.
                self.plotstyle_panel_aci = 1;
                let entry = self
                    .active_plot_style
                    .as_ref()
                    .and_then(|t| t.aci_entries.get(1));
                self.ps_color_buf = entry
                    .and_then(|e| {
                        e.color
                            .map(|[r, g, b]| format!("#{:02X}{:02X}{:02X}", r, g, b))
                    })
                    .unwrap_or_default();
                self.ps_lineweight_buf = entry
                    .map(|e| e.lineweight.to_string())
                    .unwrap_or("255".into());
                self.ps_screening_buf = entry
                    .map(|e| e.screening.to_string())
                    .unwrap_or("100".into());
                // When launched from PLOT, preserve the parent dialog so the Plot Style
                // editor can appear above it instead of replacing it.
                if self.active_modal == Some(super::super::super::ModalKind::Plot) {
                    self.plotstyle_parent_plot_geometry =
                        Some((self.modal_offset, self.modal_resize));

                    // The child editor starts with its own centred geometry.
                    self.reset_modal_geometry();
                } else {
                    // Direct command launch: Plotstyle is a normal standalone modal.
                    self.plotstyle_parent_plot_geometry = None;
                }

                self.active_modal = Some(super::super::super::ModalKind::Plotstyle);
                self.plotstyle_panel_baseline = Some(self.active_plot_style.clone());
                Task::none()
            }
            Message::PlotStylePanelClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::PlotStylePanelSelectAci(aci) => {
                self.plotstyle_panel_aci = aci;
                let entry = self
                    .active_plot_style
                    .as_ref()
                    .and_then(|t| t.aci_entries.get(aci as usize));
                self.ps_color_buf = entry
                    .and_then(|e| {
                        e.color
                            .map(|[r, g, b]| format!("#{:02X}{:02X}{:02X}", r, g, b))
                    })
                    .unwrap_or_default();
                self.ps_lineweight_buf = entry
                    .map(|e| e.lineweight.to_string())
                    .unwrap_or("255".into());
                self.ps_screening_buf = entry
                    .map(|e| e.screening.to_string())
                    .unwrap_or("100".into());
                Task::none()
            }
            Message::PlotStylePanelColorBuf(s) => {
                self.ps_color_buf = s;
                self.on_plot_style_panel_apply()
            }

            Message::PlotStylePanelLwBuf(s) => {
                self.ps_lineweight_buf = s;
                self.on_plot_style_panel_apply()
            }
            Message::PlotStylePanelLwSet(index) => {
                self.ps_lineweight_buf = index.to_string();
                self.on_plot_style_panel_apply()
            }

            Message::PlotStylePanelScreenBuf(s) => {
                self.ps_screening_buf = s;
                self.on_plot_style_panel_apply()
            }

            Message::PlotStylePanelApply => self.on_plot_style_panel_apply(),

            Message::PlotStylePanelSaveDirect => {
                if self.active_plot_style.is_none() {
                    self.command_line.push_error(
                        crate::t!("No plot style table loaded. Load or create one first.").as_ref(),
                    );
                    return Task::none();
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    let table = self.active_plot_style.as_ref().expect("checked above");
                    let table_name = table.name.clone();

                    let result = crate::io::plot_style::ensure_plot_styles_dir().and_then(|dir| {
                        let path = dir.join(&table_name);
                        table.save(&path)?;
                        Ok(path)
                    });

                    match result {
                        Ok(path) => {
                            self.plot_dialog.style_name = table_name;
                            self.plot_dialog.style_missing = false;
                            self.plot_dialog.plot_styles =
                                crate::io::plot_style::available_table_names();
                            self.tabs[self.active_tab]
                                .scene
                                .invalidate_display_plot_style();
                            self.commit_plotstyle_panel_baseline();

                            self.command_line.push_output(
                                crate::tf!("Plot style table saved to \"{}\".", path.display())
                                    .as_ref(),
                            );
                        }
                        Err(error) => {
                            self.command_line
                                .push_error(crate::tf!("Save error: {error}").as_ref());
                        }
                    }

                    Task::none()
                }

                #[cfg(target_arch = "wasm32")]
                {
                    // Browsers cannot overwrite a local file directly, so fall back
                    // to the existing Save As flow.
                    self.on_plot_style_panel_save()
                }
            }

            Message::PlotStylePanelSave => self.on_plot_style_panel_save(),
            Message::PlotStylePanelSavePath(Some(path)) => {
                let path = if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("ctb"))
                {
                    path
                } else {
                    path.with_extension("ctb")
                };
                if let Some(table) = &self.active_plot_style {
                    match table.save(&path) {
                        Ok(()) => {
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            if let Some(table) = self.active_plot_style.as_mut() {
                                table.name = name.clone();
                            }
                            self.plot_dialog.style_name = name;
                            self.plot_dialog.style_missing = false;
                            self.plot_dialog.plot_styles =
                                crate::io::plot_style::available_table_names();
                            self.tabs[self.active_tab]
                                .scene
                                .invalidate_display_plot_style();
                            self.commit_plotstyle_panel_baseline();
                            self.command_line.push_output(
                                crate::tf!("Plot style table saved to \"{}\".", path.display())
                                    .as_ref(),
                            );
                        }
                        Err(e) => self
                            .command_line
                            .push_error(crate::tf!("Save error: {e}").as_ref()),
                    }
                }
                Task::none()
            }
            Message::PlotStylePanelSavePath(None) => Task::none(),

            // ── TextStyle Font Browser ────────────────────────────────────────
            Message::TextStyleDialogOpen => self.on_text_style_dialog_open(),
            Message::StyleImportPick(kind) => self.style_import_pick(kind),
            Message::StyleImportFile(kind, path) => self.style_import_file(kind, path),
            Message::StyleImportLoaded(kind, result) => self.style_import_loaded(kind, result),
            Message::StyleImportToggle(name) => {
                self.style_import_toggle(&name);
                Task::none()
            }
            Message::StyleImportApply => {
                self.style_import_apply();
                Task::none()
            }
            Message::StyleImportConfirm(overwrite) => {
                self.style_import_confirm(overwrite);
                Task::none()
            }
            Message::StyleImportCancel => {
                self.style_import_cancel();
                Task::none()
            }
            Message::TextStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::TextStyleDialogSelect(name) => {
                if self.style_editor_blocked(crate::app::StyleKind::Text)
                    || name == self.textstyle_selected
                {
                    return Task::none();
                }
                self.text_style_go_on(crate::ui::style::textstyle::Pending::Select(name))
            }
            Message::TextStyleInUseFilter(on) => {
                self.textstyle_in_use_only = on;
                Task::none()
            }
            Message::TextStyleSample(text) => {
                self.textstyle_sample = text;
                Task::none()
            }
            Message::TextStyleDialogCompare(name) => {
                self.textstyle_compare = name;
                Task::none()
            }
            Message::TextStyleDialogSetCurrent => {
                self.text_style_go_on(crate::ui::style::textstyle::Pending::SetCurrent)
            }
            Message::TextStyleDialogNew => {
                if self.style_editor_blocked(crate::app::StyleKind::Text) {
                    return Task::none();
                }
                self.text_style_go_on(crate::ui::style::textstyle::Pending::New)
            }
            Message::TextStyleDialogCopy => {
                if self.style_editor_blocked(crate::app::StyleKind::Text) {
                    return Task::none();
                }
                self.text_style_go_on(crate::ui::style::textstyle::Pending::Copy)
            }
            Message::TextStyleDialogDelete => {
                if self.style_editor_blocked(crate::app::StyleKind::Text) {
                    return Task::none();
                }
                self.text_style_go_on(crate::ui::style::textstyle::Pending::Delete)
            }
            Message::TextStyleCard(message) => self.on_text_style_card(message),
            Message::TextStyleContext(name) => {
                if self.style_editor_blocked(crate::app::StyleKind::Text) {
                    return Task::none();
                }
                let task = if name == self.textstyle_selected {
                    Task::none()
                } else {
                    self.text_style_go_on(crate::ui::style::textstyle::Pending::Select(name.clone()))
                };
                self.textstyle_context = Some(name);
                task
            }
            // ── Shared inline rename (all style managers) ─────────────────
            Message::StyleRenameStart(crate::app::StyleKind::Text, name)
                if self.active_modal == Some(super::super::super::ModalKind::TextStyle) =>
            {
                if self.style_editor_blocked(crate::app::StyleKind::Text) {
                    return Task::none();
                }
                self.text_style_go_on(crate::ui::style::textstyle::Pending::Rename(name))
            }
            Message::StyleRenameStart(kind, name) => {
                self.style_rename_start(kind, name);
                // Focus the freshly-shown rename field so the user can type
                // immediately after the double click.
                iced::widget::operation::focus(crate::ui::style::style_list::rename_input_id())
            }
            Message::StyleRenameEdit(s) => {
                self.style_rename_buf = s;
                Task::none()
            }
            Message::StyleRenameCommit(kind) => {
                self.style_rename_commit(kind);
                Task::none()
            }
            Message::StyleRenameFocus(focused) => {
                let typed = self
                    .style_rename
                    .as_ref()
                    .is_some_and(|name| *name != self.style_rename_buf);
                if typed && focused != Some(crate::ui::style::style_list::rename_input_id()) {
                    if let Some(kind) = self.style_rename_kind {
                        self.style_rename_commit(kind);
                    }
                }
                Task::none()
            }
            Message::StyleRenameCancel => {
                self.style_rename_cancel();
                Task::none()
            }
            Message::TextStyleEdit { field, value } => {
                let draft = &mut self.textstyle_draft;
                match field {
                    "width" => draft.width.text = value,
                    "oblique" => draft.oblique.text = value,
                    "height" => draft.height.text = value,
                    "bigfont" if draft.big_font_on => draft.style.big_font_file = value,
                    _ => {}
                }
                Task::none()
            }
            Message::TextStyleToggle(field) => {
                if self.selected_textstyle_is_read_only() {
                    return Task::none();
                }
                let s = &mut self.textstyle_draft.style;
                match field {
                    "backward" => s.flags.backward = !s.flags.backward,
                    "upside_down" => s.flags.upside_down = !s.flags.upside_down,
                    "vertical" => s.is_vertical = !s.is_vertical,
                    "annotative" => s.annotative = !s.annotative,
                    "orientation" => {
                        self.textstyle_draft.orientation = !self.textstyle_draft.orientation
                    }
                    "use_big_font" => {
                        self.textstyle_draft.toggle_big_font();
                        // Turned on with no big font named, the first big
                        // font of the list is taken.
                        if self.textstyle_draft.big_font_on
                            && self.textstyle_draft.style.big_font_file.trim().is_empty()
                        {
                            let mut big_fonts: Vec<String> =
                                crate::io::font_repo::folder_shx_fonts()
                                    .into_iter()
                                    .filter(|file| {
                                        crate::io::font_repo::font_file_path(file, None)
                                            .is_some_and(|path| {
                                                crate::scene::text::shx::is_bigfont(&path)
                                            })
                                    })
                                    .collect();
                            big_fonts.sort_by_key(|file| file.to_ascii_lowercase());
                            if let Some(first) = big_fonts.into_iter().next() {
                                self.textstyle_draft.style.big_font_file = first;
                            }
                        }
                    }
                    _ => {}
                }
                Task::none()
            }
            Message::TextStyleApply => self.on_text_style_apply(),
            Message::TextStyleFont(event) => {
                if self.selected_textstyle_is_read_only() {
                    return Task::none();
                }
                self.textstyle_bigfont_picker.open = false;
                let is_toggle = matches!(event, crate::ui::font_picker::FontPickerEvent::Toggle);
                let doc = &self.tabs[self.active_tab].scene.document;
                match self.textstyle_font_picker.update(event, doc, false) {
                    // A stroke font replaces the system font, which would
                    // otherwise keep taking precedence.
                    Some(FontChoice::Stroke(file)) => self.textstyle_draft.pick_stroke_font(file),
                    Some(FontChoice::TrueType(family)) => {
                        self.textstyle_draft.pick_system_font(family)
                    }
                    Some(FontChoice::StyleDefault) | None => {}
                }
                if is_toggle && self.textstyle_font_picker.open {
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        crate::ui::font_picker::FONT_PICKER_SEARCH_ID,
                    ));
                }
                Task::none()
            }
            Message::TextStyleBigFont(event) => {
                if self.selected_textstyle_is_read_only() || !self.textstyle_draft.big_font_on {
                    return Task::none();
                }
                self.textstyle_font_picker.open = false;
                let is_toggle = matches!(event, crate::ui::font_picker::FontPickerEvent::Toggle);
                let doc = &self.tabs[self.active_tab].scene.document;
                if let Some(picked) = self.textstyle_bigfont_picker.update_bigfont(event, doc) {
                    if let FontChoice::Stroke(file) = picked {
                        self.textstyle_draft.style.big_font_file = file;
                    }
                }
                if is_toggle && self.textstyle_bigfont_picker.open {
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        crate::ui::font_picker::FONT_PICKER_SEARCH_ID,
                    ));
                }
                Task::none()
            }
            Message::TextStyleFaceStyle(face) => {
                if !self.selected_textstyle_is_read_only() {
                    self.textstyle_draft.set_face(face.bold(), face.italic());
                }
                Task::none()
            }

            // ── TableStyle Dialog ─────────────────────────────────────────────
            Message::TableStyleDialogOpen => {
                use document::objects::ObjectType;
                let i = self.active_tab;
                // The dialog opens on the current style.
                let document = &self.tabs[i].scene.document;
                let current = &document.header.current_table_style_name;
                let names: Vec<&String> = document
                    .objects
                    .values()
                    .filter_map(|o| match o {
                        ObjectType::TableStyle(s) => Some(&s.name),
                        _ => None,
                    })
                    .collect();
                self.tablestyle_selected = names
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(current))
                    .or(names.first())
                    .map_or_else(|| "Standard".to_string(), |name| (*name).clone());
                self.tablestyle_parent_table = false;
                self.table_style_ui.cards = Default::default();
                self.load_tablestyle_bufs(i);
                self.active_modal = Some(super::super::super::ModalKind::TableStyle);
                self.style_stage_begin();
                Task::none()
            }
            Message::TableStyleListInUse(on) => {
                self.tablestyle_in_use_only = on;
                Task::none()
            }
            Message::TableStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::TableStyleDialogSelect(name) => {
                self.stage_tablestyle_bufs();
                self.table_style_ui.cards = Default::default();
                self.tablestyle_selected = name;
                let i = self.active_tab;
                self.load_tablestyle_bufs(i);
                Task::none()
            }
            Message::TableStyleDialogTab(tab) => {
                self.tablestyle_tab = tab;
                Task::none()
            }
            Message::TableStyleDialogCompare(name) => {
                self.tablestyle_compare = name;
                Task::none()
            }

            Message::TableStyleEdit { field, value } => {
                if field == "description" {
                    self.table_style_ui.description = value;
                    // Written at once like the other fields: picking another
                    // cell style reloads the buffers from the style.
                    self.stage_tablestyle_bufs();
                }
                Task::none()
            }

            Message::TableStyleApply => {
                self.stage_tablestyle_bufs();
                self.style_stage_commit();
                Task::none()
            }

            Message::TableStyleSetFlow(value) => {
                use document::objects::TableFlowDirection;
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    s.flow_direction = match value.as_str() {
                        "Up" => TableFlowDirection::Up,
                        _ => TableFlowDirection::Down,
                    };
                    s.sync_rows_from_cell_styles();
                }
                self.drop_table_style_pictures(i);
                Task::none()
            }

            Message::TableColorMore(field) => {
                self.table_style_ui.color_open = if self.table_style_ui.color_open == Some(field) {
                    None
                } else {
                    Some(field)
                };
                Task::none()
            }
            Message::TableStyleCellSelect(name) => {
                self.table_style_ui.cell = name;
                self.table_style_ui.color_open = None;
                let i = self.active_tab;
                self.load_tablestyle_bufs(i);
                Task::none()
            }
            Message::TableStyleCells(message) => self.on_table_style_cells(message),
            Message::TableStyleCellEdit { field, value } => {
                self.table_style_ui.color_open = None;
                self.on_table_cell_style_edit(field, value)
            }
            Message::TableStyleTemplatePick => self.on_table_style_template_pick(),
            Message::TableStyleTemplateRemove => self.on_table_style_template_remove(),

            Message::TableStyleToggle(field) => {
                use document::objects::ObjectType;
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let ObjectType::TableStyle(s) = obj {
                        if s.name == name {
                            match field {
                                "title_sup" => s.title_suppressed = !s.title_suppressed,
                                "header_sup" => s.header_suppressed = !s.header_suppressed,
                                _ => {}
                            }
                        }
                    }
                }
                Task::none()
            }

            Message::TableStyleToggleAnnotative => {
                use document::objects::ObjectType;
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let ObjectType::TableStyle(s) = obj {
                        if s.name == name {
                            s.annotative = !s.annotative;
                        }
                    }
                }
                Task::none()
            }

            Message::TableStyleDialogNew => self.style_new(crate::app::StyleKind::Table),
            Message::TableStyleDialogCopy => self.style_copy(crate::app::StyleKind::Table),
            Message::TableStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::Table);
                Task::none()
            }
            Message::TableStyleDialogSetCurrent => {
                // Staged: persists on Apply. The header field is the round-trip
                // source of truth ($CTABLESTYLE); the ribbon mirrors it.
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                if self
                    .style_names(crate::app::StyleKind::Table)
                    .contains(&name)
                {
                    self.tabs[i].scene.document.header.current_table_style_name = name.clone();
                    self.ribbon.active_table_style = name.clone();
                    self.command_line
                        .push_output(crate::tf!("Current table style: {name}").as_ref());
                }
                Task::none()
            }

            // ── MLineStyle Dialog ─────────────────────────────────────────────
            Message::Linetype(message) => self.on_linetype_message(message),
            Message::MlStyleDialogOpen => self.on_ml_style_dialog_open(),
            Message::MlStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::MlStyleDialogSelect(name) => {
                self.stage_mlstyle_bufs();
                self.mlstyle_selected = name;
                let i = self.active_tab;
                self.load_mlstyle_bufs(i);
                Task::none()
            }
            Message::MlStyleDialogTab(tab) => {
                self.mlstyle_tab = tab;
                Task::none()
            }
            Message::MlStyleDialogCompare(name) => {
                self.mlstyle_compare = name;
                Task::none()
            }
            Message::MlStyleDialogSetCurrent => {
                use document::objects::ObjectType;
                let i = self.active_tab;
                let name = self.mlstyle_selected.clone();
                let exists = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .any(|o| matches!(o, ObjectType::MLineStyle(s) if s.name == name));
                if exists {
                    // Staged: persists on Apply.
                    self.tabs[i].scene.document.header.multiline_style = name.clone();
                    self.command_line
                        .push_output(crate::tf!("Current multiline style: {}", name).as_ref());
                }
                Task::none()
            }
            Message::MlStyleApply => {
                self.stage_mlstyle_bufs();
                self.style_stage_commit();
                Task::none()
            }
            Message::MlStyleDialogNew => self.style_new(crate::app::StyleKind::MLine),
            Message::MlStyleDialogCopy => self.style_copy(crate::app::StyleKind::MLine),
            Message::MlStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::MLine);
                Task::none()
            }
            Message::MlStyleEdit { field, value } => {
                match field {
                    "description" => self.mln_description = value,
                    "start_angle" => self.mln_start_angle = value,
                    "end_angle" => self.mln_end_angle = value,
                    "fill_color" => self.mln_fill_color = value,
                    _ => {}
                }
                self.stage_mlstyle_bufs();
                Task::none()
            }
            Message::MlStyleToggle(field) => {
                let i = self.active_tab;
                if let Some(style) = self.mlstyle_mut(i) {
                    match field {
                        "fill" => style.flags.fill_on = !style.flags.fill_on,
                        "joints" => style.flags.display_joints = !style.flags.display_joints,
                        "start_square" => {
                            style.flags.start_square_cap = !style.flags.start_square_cap
                        }
                        "start_inner" => {
                            style.flags.start_inner_arcs_cap = !style.flags.start_inner_arcs_cap
                        }
                        "start_round" => style.flags.start_round_cap = !style.flags.start_round_cap,
                        "end_square" => style.flags.end_square_cap = !style.flags.end_square_cap,
                        "end_inner" => {
                            style.flags.end_inner_arcs_cap = !style.flags.end_inner_arcs_cap
                        }
                        "end_round" => style.flags.end_round_cap = !style.flags.end_round_cap,
                        _ => {}
                    }
                }
                Task::none()
            }
            Message::MlStyleElementEdit {
                index,
                field,
                value,
            } => {
                if let Some(element) = self.mln_elements.get_mut(index) {
                    match field {
                        "offset" => element[0] = value,
                        "color" => element[1] = value,
                        "linetype" => element[2] = value,
                        _ => {}
                    }
                }
                self.stage_mlstyle_bufs();
                Task::none()
            }
            Message::MlStyleElementAdd => {
                let i = self.active_tab;
                if let Some(style) = self.mlstyle_mut(i) {
                    style
                        .elements
                        .push(document::objects::MLineStyleElement::default());
                }
                self.load_mlstyle_bufs(i);
                Task::none()
            }
            Message::MlStyleElementDelete(index) => {
                let i = self.active_tab;
                if let Some(style) = self.mlstyle_mut(i) {
                    if style.elements.len() > 1 && index < style.elements.len() {
                        style.elements.remove(index);
                    }
                }
                self.load_mlstyle_bufs(i);
                Task::none()
            }

            // ── MLeaderStyle Dialog ───────────────────────────────────────────
            Message::MLeaderStyleDialogOpen => self.on_mleader_style_dialog_open(),
            Message::MLeaderStyleDlg(message) => self.on_mleader_style_dlg(message),
            Message::MLeaderStyleEdit { field, value } => {
                self.mls_mark_dirty();
                self.on_mleader_style_edit(field, value)
            }
            Message::MLeaderStyleToggle(field) => {
                self.mls_mark_dirty();
                self.on_mleader_style_toggle(field)
            }
            Message::MLeaderColorMore(field) => {
                self.mls_dialog.color_open = if self.mls_dialog.color_open == Some(field) {
                    None
                } else {
                    Some(field)
                };
                Task::none()
            }
            Message::MLeaderStyleSetEnum { field, value } => {
                self.mls_mark_dirty();
                self.on_mleader_style_set_enum(field, value)
            }
            Message::MLeaderStyleLineWeightChanged(line_weight) => {
                self.mls_mark_dirty();
                self.mls_dialog.color_open = None;
                let i = self.active_tab;
                if let Some(s) = self.mleaderstyle_mut(i) {
                    s.line_weight = line_weight;
                }
                Task::none()
            }
            Message::MLeaderStyleSetHandle { field, value } => {
                self.mls_mark_dirty();
                self.on_mleader_style_set_handle(field, value)
            }

            // ── DimStyle Dialog ───────────────────────────────────────────────
            Message::DimStyleDialogOpen => self.on_dim_style_dialog_open(),
            Message::DimStyleDlg(message) => self.on_dim_style_dlg(message),
            Message::OpenColorWindow(target, color) => {
                if matches!(
                    target,
                    crate::app::ColorPickTarget::GraphicAttributesLine
                        | crate::app::ColorPickTarget::GraphicAttributesSolid
                ) {
                    self.graphic_attributes.open_menu = None;
                }
                let (current, varies, name) = self.color_picker_current(&target, color);
                let mut picker = crate::ui::color_select::ColorPicker::new(target, current, varies);
                if !matches!(picker.target, crate::app::ColorPickTarget::Interface(_)) {
                    self.read_color_books();
                    let by_layer = self.color_picker_layer_color(&picker.target);
                    let tab = &self.tabs[self.active_tab];
                    picker = picker.with_drawing(
                        crate::ui::color_select::drawing_colors::collect(&tab.scene.document),
                        &tab.recent_colors,
                        std::sync::Arc::clone(&self.color_books),
                        name,
                        by_layer,
                    );
                }
                self.color_picker = Some(picker);
                self.modal_offset = iced::Vector::ZERO;
                self.dsd.open_list = None;
                self.mls_dialog.color_open = None;
                self.table_style_ui.color_open = None;
                self.ribbon.close_dropdown();

                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open = false;
                self.tabs[i].properties.open_color_field = None;
                self.tabs[i].layers.color_picker_row = None;

                Task::none()
            }

            Message::ColorPicker(msg) => {
                let Some(picker) = self.color_picker.as_mut() else {
                    return Task::none();
                };
                match picker.update(msg) {
                    Some(crate::ui::color_select::Outcome::Commit(color, name)) => {
                        self.update(Message::ColorWindowPick(color, name))
                    }
                    Some(crate::ui::color_select::Outcome::LoadBook) => Task::perform(
                        crate::ui::color_select::book::load_from_dialog(),
                        Message::ColorBookLoaded,
                    ),
                    Some(crate::ui::color_select::Outcome::Cancel) => {
                        self.color_picker = None;
                        Task::none()
                    }
                    Some(crate::ui::color_select::Outcome::Screenshot) => {
                        use crate::ui::color_select::{eyedropper::EyedropperMsg, ColorPickerMsg};
                        match self.main_window {
                            Some(id) => iced::window::screenshot(id).map(|shot| {
                                Message::ColorPicker(ColorPickerMsg::Eyedropper(
                                    EyedropperMsg::Shot(shot),
                                ))
                            }),
                            // Nothing to capture: leave the eyedropper instead
                            // of waiting for a screenshot that never comes.
                            None => self.update(Message::ColorPicker(ColorPickerMsg::Cancel)),
                        }
                    }
                    None => Task::none(),
                }
            }
            Message::CloseColorPicker => {
                self.color_picker = None;
                Task::none()
            }

            Message::ColorWindowPick(color, name) => {
                // Interface colours are not drawing colours: keep them out of
                // the recent list.
                if !matches!(
                    self.color_picker.as_ref().map(|picker| &picker.target),
                    Some(crate::app::ColorPickTarget::Interface(_))
                ) {
                    self.note_recent_color(color);
                }
                self.on_color_window_pick(color, name)
            }
            Message::ColorBookLoaded(Ok(book)) => {
                // A book loaded again replaces the earlier copy.
                let index = crate::ui::color_select::book::add_or_replace(
                    std::sync::Arc::make_mut(&mut self.color_books),
                    book,
                );
                if let Some(picker) = self.color_picker.as_mut() {
                    picker.show_book(std::sync::Arc::clone(&self.color_books), index);
                }
                Task::none()
            }
            Message::ColorBookLoaded(Err(error)) => {
                if let Some(picker) = self.color_picker.as_mut().filter(|_| error != "Cancelled") {
                    picker.set_status(error);
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
