use super::interactive::PluginInteractiveAdapter;
use super::validation::resolve_toggle_panel_id;
use super::*;

/// The stable contract a plugin's `dispatch` sees. Each method forwards to the
/// inherent `HostSession` method of the same name (inherent methods take
/// resolution priority, so this is plain delegation, not recursion). The
/// per-tab plugin-state accessors expose the raw `Any` box; the typed
/// `ocs_plugin_api::host::plugin_state*` helpers wrap them.
impl HostApi for HostSession<'_> {
    fn tab_index(&self) -> usize {
        self.tab_index()
    }
    fn document(&self) -> &Document {
        self.document()
    }
    fn document_mut(&mut self) -> &mut Document {
        self.document_mut()
    }
    fn add_entity(&mut self, entity: EntityType) -> Handle {
        self.add_entity(entity)
    }
    fn add_entities(&mut self, entities: Vec<EntityType>) -> Vec<Handle> {
        self.add_entities(entities)
    }
    fn update_entity(&mut self, entity: EntityType) -> bool {
        self.update_entity(entity)
    }
    fn update_entities_transaction(
        &mut self,
        label: &str,
        entities: Vec<EntityType>,
    ) -> Result<(), String> {
        self.update_entities_transaction(label, entities)
    }
    fn selection(&self) -> Vec<Handle> {
        self.selection()
    }
    fn set_selection(&mut self, handles: &[Handle]) -> Result<(), String> {
        self.set_selection(handles)
    }
    fn solid_operation(
        &mut self,
        operation: ocs_plugin_api::host::SolidOperation,
    ) -> Result<Handle, String> {
        self.solid_operation(operation)
    }
    fn remove_entity(&mut self, handle: Handle) -> bool {
        self.remove_entity(handle)
    }
    fn bump_geometry(&mut self) {
        self.bump_geometry()
    }
    fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord> {
        self.read_record(handle, app_name)
    }
    fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool {
        self.write_record(handle, record)
    }
    fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool {
        self.remove_record(handle, app_name)
    }
    fn push_undo(&mut self, label: &str) {
        self.push_undo(label)
    }
    fn begin_undo_group(&mut self, label: &str) {
        self.begin_undo_group(label)
    }
    fn end_undo_group(&mut self) {
        self.end_undo_group()
    }
    fn query_entities(&mut self, query: &str) -> Result<ocs_plugin_api::host::EntityPage, String> {
        self.query_entities(query)
    }
    fn set_dirty(&mut self) {
        self.set_dirty()
    }
    fn push_info(&mut self, msg: &str) {
        self.push_info(msg)
    }
    fn push_output(&mut self, msg: &str) {
        self.push_output(msg)
    }
    fn push_error(&mut self, msg: &str) {
        self.push_error(msg)
    }
    fn start_interactive(&mut self, command: Box<dyn ocs_plugin_api::host::InteractiveCommand>) {
        let dialog = command.modal_dialog();
        self.app.tabs[self.tab].active_cmd =
            Some(Box::new(PluginInteractiveAdapter { inner: command }));
        if let Some(def) = dialog {
            self.app.open_plugin_dialog_on_tab(self.tab, def, None);
        }
    }
    fn plugin_state_any(&self, plugin_id: &str) -> Option<&(dyn Any + Send + Sync)> {
        self.app.tabs[self.tab]
            .plugin_state
            .get(plugin_id)
            .map(|b| b.as_ref())
    }
    fn plugin_state_any_mut(&mut self, plugin_id: &str) -> Option<&mut (dyn Any + Send + Sync)> {
        self.app.tabs[self.tab]
            .plugin_state
            .get_mut(plugin_id)
            .map(|b| b.as_mut())
    }
    fn ensure_plugin_state_any(
        &mut self,
        plugin_id: &'static str,
        init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
    ) -> Option<&mut (dyn Any + Send + Sync)> {
        Some(
            self.app.tabs[self.tab]
                .plugin_state
                .entry(plugin_id)
                .or_insert_with(|| init())
                .as_mut(),
        )
    }
    fn document_reader(&self) -> Box<dyn ocs_plugin_api::host::DocumentReader + '_> {
        Box::new(ocs_plugin_api::host::CadDocumentReader(self.document()))
    }
    fn document_view(&mut self) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        self.document_view()
    }
    fn tab_id(&self) -> u64 {
        self.tab_id()
    }
    fn document_path(&self, tab_id: u64) -> Option<std::path::PathBuf> {
        self.document_path(tab_id)
    }
    fn system_variable(&self, name: &str) -> Option<HostSettingValue> {
        self.system_variable(name)
    }
    fn set_system_variable(
        &mut self,
        name: &str,
        value: HostSettingValue,
    ) -> Result<HostSettingValue, String> {
        self.set_system_variable(name, value)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn document_view_v4(&mut self, tab_id: u64) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        self.document_view_v4(tab_id)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn close_document_view_v4(&mut self, tab_id: u64) {
        self.close_document_view_v4(tab_id)
    }
    fn add_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> Option<Handle> {
        self.add_layer(config)
    }
    fn modify_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> bool {
        self.modify_layer(config)
    }
    fn execute_command(&mut self, cmd: &str) -> bool {
        self.execute_command(cmd)
    }
    fn table_operation(
        &mut self,
        operation: ocs_plugin_api::host::TableOperation,
    ) -> Result<Handle, String> {
        self.table_operation(operation)
    }
    fn run_command(
        &mut self,
        request: ocs_plugin_api::host::CommandRequest,
    ) -> Result<ocs_plugin_api::host::CommandOutcome, String> {
        self.run_command(request)
    }
    fn create_xrecord(
        &mut self,
        dictionary_path: &[&str],
        name: &str,
        entries: Vec<ocs_plugin_api::host::XRecordEntry>,
    ) -> Result<Handle, String> {
        self.create_xrecord(dictionary_path, name, entries)
    }
    fn read_xrecord(
        &self,
        dictionary_path: &[&str],
        name: &str,
    ) -> Result<Vec<ocs_plugin_api::host::XRecordEntry>, String> {
        self.read_xrecord(dictionary_path, name)
    }
    fn update_xrecord(
        &mut self,
        dictionary_path: &[&str],
        name: &str,
        entries: Vec<ocs_plugin_api::host::XRecordEntry>,
    ) -> Result<Handle, String> {
        self.update_xrecord(dictionary_path, name, entries)
    }
    fn delete_xrecord(
        &mut self,
        dictionary_path: &[&str],
        name: &str,
    ) -> Result<ocs_plugin_api::host::XRecord, String> {
        self.delete_xrecord(dictionary_path, name)
    }
    fn list_xrecords(&self, dictionary_path: &[&str]) -> Result<Vec<String>, String> {
        self.list_xrecords(dictionary_path)
    }
    fn set_geo_location(
        &mut self,
        location: &ocs_plugin_api::host::GeoLocation,
    ) -> Result<Handle, String> {
        self.set_geo_location(location)
    }
    fn remove_geo_location(&mut self) -> Result<bool, String> {
        self.remove_geo_location()
    }
    fn set_reference_only(&mut self, handle: Handle, reference_only: bool) -> Result<(), String> {
        self.set_reference_only(handle, reference_only)
    }
    fn entity_grips(&self, handle: Handle) -> Result<Vec<ocs_plugin_api::host::GripInfo>, String> {
        self.entity_grips(handle)
    }
    fn grip_drag(&mut self, handle: Handle, grip_id: usize, to: [f64; 3]) -> Result<(), String> {
        self.grip_drag(handle, grip_id, to)
    }
    fn set_plugin_grips(
        &mut self,
        plugin_id: &str,
        grips: Vec<ocs_plugin_api::host::PluginGrip>,
    ) -> Result<(), String> {
        self.set_plugin_grips(plugin_id, grips)
    }
    fn entities_batch(
        &self,
        handles: &[Handle],
    ) -> Result<Vec<Option<document::EntityType>>, String> {
        self.entities_batch(handles)
    }
    fn constraint_create(
        &mut self,
        constraint: ocs_plugin_api::host::PluginConstraint,
    ) -> Result<(), String> {
        self.constraint_create(constraint)
    }
    fn read_xrecords(
        &self,
        reads: &[ocs_plugin_api::host::XRecordRead],
    ) -> Result<Vec<Result<Vec<ocs_plugin_api::host::XRecordEntry>, String>>, String> {
        self.read_xrecords(reads)
    }
    fn update_xrecords(
        &mut self,
        updates: &[ocs_plugin_api::host::XRecordUpdate],
    ) -> Result<Vec<Result<Handle, String>>, String> {
        self.update_xrecords(updates)
    }
    fn list_xrecords_with_entries(
        &self,
        dictionary_path: &[&str],
    ) -> Result<Vec<(String, Vec<ocs_plugin_api::host::XRecordEntry>)>, String> {
        self.list_xrecords_with_entries(dictionary_path)
    }
    fn stage_parametric_chain(
        &mut self,
        reference_name: &str,
        chain: &ocs_plugin_api::host::ParametricChain,
        tolerance: f64,
    ) -> Result<Vec<Handle>, String> {
        self.stage_parametric_chain(reference_name, chain, tolerance)
    }
    fn sample_parametric(
        &self,
        chain: &ocs_plugin_api::host::ParametricChain,
        s: f64,
    ) -> Result<Option<ocs_plugin_api::host::ParametricSample>, String> {
        self.sample_parametric(chain, s)
    }
    fn register_plugin_class(
        &mut self,
        plugin_id: &str,
        class: &ocs_plugin_api::host::PluginClassDeclaration,
    ) -> Result<(), String> {
        self.register_plugin_class(plugin_id, class)
    }
    fn stage_plugin_class_instance(
        &mut self,
        plugin_id: &str,
        instance: &ocs_plugin_api::host::PluginClassInstance,
    ) -> Result<Vec<Handle>, String> {
        self.stage_plugin_class_instance(plugin_id, instance)
    }
    fn plugin_class_instance_properties(
        &self,
        handle: Handle,
    ) -> Result<Vec<ocs_plugin_api::host::PluginClassParameterRow>, String> {
        // API v14 (additive): the wire frame serves the session's own read —
        // the identity stamp resolves, the design record prints its declared
        // rows. Same-name inherent call resolution: see register_plugin_class.
        self.plugin_class_instance_properties(handle)
    }

    fn show_dialog(
        &mut self,
        dialog: ocs_plugin_api::dialog::DialogDef,
    ) -> ocs_plugin_api::dialog::DialogResponse {
        self.app.open_plugin_dialog_on_tab(self.tab, dialog, None);
        ocs_plugin_api::dialog::DialogResponse::Cancelled
    }
    fn show_dialog_for_plugin(
        &mut self,
        plugin_id: &str,
        dialog: ocs_plugin_api::dialog::DialogDef,
    ) -> ocs_plugin_api::dialog::DialogResponse {
        self.app
            .open_plugin_dialog_on_tab(self.tab, dialog, Some(plugin_id.to_string()));
        ocs_plugin_api::dialog::DialogResponse::Cancelled
    }
    fn register_panel(&mut self, panel: ocs_plugin_api::dialog::PanelDef) -> bool {
        let id = panel.id.clone();
        let cmd_opt = panel.command.clone();
        let num = if let Some(&existing_num) = self.app.plugin_panel_ids.get(&id) {
            existing_num
        } else {
            let next_num = (self.app.plugin_panel_ids.len() as u32) + 1;
            self.app.plugin_panel_ids.insert(id.clone(), next_num);
            self.app.plugin_panel_by_num.insert(next_num, id.clone());
            next_num
        };

        let panel_enum = crate::ui::dock::PanelId::Plugin(num);
        if self.app.dock.location(panel_enum).is_none() {
            self.app
                .dock
                .dock(panel_enum, crate::app::config::DockSide::Right, usize::MAX);
        }
        if let Some(w) = panel.width {
            let mut panel_settings = self.app.dock.settings(panel_enum);
            panel_settings.width = w;
            self.app.dock.panels.insert(panel_enum, panel_settings);
        }

        let state = crate::ui::window::plugin_panel::PluginPanelState::from_def(panel);
        self.app.plugin_panels.insert(id.clone(), state);

        // Pallet-menu listing is capped per edge so a plugin cannot widen
        // the palette beyond a bounded length (see docs/plugin-pallet-menu.md).
        if self.app.plugin_panels.len() > PLUGIN_PALLET_MENU_MAX_ROWS {
            eprintln!(
                "[plugin] {id}: registered beyond the {}-row pallet-menu cap; \
                 the edge (+) menu lists the first {} plugin panels only",
                PLUGIN_PALLET_MENU_MAX_ROWS, PLUGIN_PALLET_MENU_MAX_ROWS
            );
        }

        // Keep command line autocomplete updated with dynamic panel commands
        if let Some(ref c) = cmd_opt {
            let cu = c.to_uppercase();
            if !self.app.command_line.dynamic_commands.contains(&cu) {
                self.app.command_line.dynamic_commands.push(cu);
            }
        }
        let id_u = id.to_uppercase();
        if !self.app.command_line.dynamic_commands.contains(&id_u) {
            self.app.command_line.dynamic_commands.push(id_u);
        }

        true
    }
    fn update_panel(
        &mut self,
        panel_id: &str,
        values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>,
    ) -> bool {
        if let Some(panel) = self.app.plugin_panels.get_mut(panel_id) {
            panel.update_values(values);
            return true;
        }
        let lower = panel_id.to_lowercase();
        if let Some((_, panel)) = self
            .app
            .plugin_panels
            .iter_mut()
            .find(|(k, _)| k.to_lowercase() == lower)
        {
            panel.update_values(values);
            return true;
        }
        if let Some(state) = &mut self.app.plugin_dialog_state {
            if panel_id.is_empty()
                || state.def.title == panel_id
                || state.def.title.eq_ignore_ascii_case(panel_id)
            {
                state.update_values(values);
                return true;
            }
        }
        false
    }
    fn toggle_panel(&mut self, panel_id: &str, show: Option<bool>) -> bool {
        let matched_id = resolve_toggle_panel_id(self.app, panel_id);

        if let Some(ref id) = matched_id {
            if let Some(&num) = self.app.plugin_panel_ids.get(id) {
                let panel_enum = crate::ui::dock::PanelId::Plugin(num);
                let is_currently_open = self
                    .app
                    .plugin_panels
                    .get(id)
                    .map(|p| p.visible)
                    .unwrap_or(false);

                let new_visible = if let Some(s) = show {
                    s
                } else {
                    !is_currently_open
                };

                if let Some(panel) = self.app.plugin_panels.get_mut(id) {
                    panel.visible = new_visible;
                }

                if new_visible {
                    if self.app.dock.location(panel_enum).is_none() {
                        self.app.dock.dock(
                            panel_enum,
                            crate::app::config::DockSide::Right,
                            usize::MAX,
                        );
                    }
                    self.app.dock_reveal(panel_enum);
                } else {
                    let _ = self.app.dock_set_open(panel_enum, false);
                }
                return true;
            }
        }
        false
    }

    fn register_panel_for_plugin(
        &mut self,
        plugin_id: &str,
        panel: ocs_plugin_api::dialog::PanelDef,
    ) -> bool {
        if plugin_id.is_empty() || panel.id.trim().is_empty() {
            return false;
        }
        let panel_id = panel.id.clone();
        if let Some(existing) = self.app.plugin_panels.get(&panel_id) {
            if existing.owner_plugin_id.as_deref() != Some(plugin_id) {
                return false;
            }
            if existing.def == panel {
                // Re-registering an unchanged form is idempotent: do not
                // discard values, collapsed sections or focus mid-edit.
                return true;
            }
        }
        if !self.register_panel(panel) {
            return false;
        }
        if let Some(state) = self.app.plugin_panels.get_mut(&panel_id) {
            state.owner_plugin_id = Some(plugin_id.to_owned());
            true
        } else {
            false
        }
    }

    fn update_panel_for_plugin(
        &mut self,
        plugin_id: &str,
        panel_id: &str,
        values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>,
    ) -> bool {
        if let Some(panel_id) = self.owned_panel_id(plugin_id, panel_id) {
            return self.update_panel(&panel_id, values);
        }
        // A modal dialog the plugin opened is updated in place; it is never
        // routed through `update_panel`, whose side-panel lookup by name
        // could reach another plugin's panel with the same title.
        if let Some(state) = &mut self.app.plugin_dialog_state {
            if state.plugin_id.as_deref() == Some(plugin_id)
                && (panel_id.is_empty() || state.def.title.eq_ignore_ascii_case(panel_id))
            {
                state.update_values(values);
                return true;
            }
        }
        false
    }

    fn toggle_panel_for_plugin(
        &mut self,
        plugin_id: &str,
        panel_id: &str,
        show: Option<bool>,
    ) -> bool {
        let Some(panel_id) = self.owned_panel_id(plugin_id, panel_id) else {
            return false;
        };
        self.toggle_panel(&panel_id, show)
    }

    fn panel_snapshot(
        &self,
        panel_id: &str,
    ) -> Result<ocs_plugin_api::dialog::PanelSnapshot, String> {
        let Some(resolved) = resolve_toggle_panel_id(self.app, panel_id) else {
            return Err(format!("panel {panel_id:?} is not registered"));
        };
        let panel = self
            .app
            .plugin_panels
            .get(&resolved)
            .ok_or_else(|| format!("panel {panel_id:?} is not registered"))?;
        // A direct HostApi command is already executing in a document-bound
        // command context. Unlike the IPC entry point, there is no second
        // transport identity to authenticate here; the caller's command is
        // the capability. The snapshot still carries the registered owner
        // and the actual session tab for the plugin's own target checks.
        Ok(panel.snapshot(self.tab_id()))
    }

    fn panel_snapshot_for_plugin(
        &self,
        plugin_id: &str,
        panel_id: &str,
    ) -> Result<ocs_plugin_api::dialog::PanelSnapshot, String> {
        let Some(resolved) = resolve_toggle_panel_id(self.app, panel_id) else {
            return Err(format!("panel {panel_id:?} is not registered"));
        };
        let panel = self
            .app
            .plugin_panels
            .get(&resolved)
            .ok_or_else(|| format!("panel {panel_id:?} is not registered"))?;
        if panel.owner_plugin_id.as_deref() != Some(plugin_id) {
            return Err(format!(
                "panel {panel_id:?} is not owned by plugin {plugin_id:?}"
            ));
        }
        // HostSession is created by the normal, document-bound command
        // dispatcher. The tab is attached here, never supplied by the form.
        Ok(panel.snapshot(self.tab_id()))
    }

    fn register_quad_panels(
        &mut self,
        plugin_id: &str,
        panels: &[ocs_plugin_api::dialog::QuadPanelDef],
    ) -> Vec<bool> {
        // Same-name inherent: see register_plugin_class (the trait serves
        // the wire; the inherent carries the rules).
        self.register_quad_panels(plugin_id, panels)
    }
    fn unregister_quad_panels(&mut self, plugin_id: &str, panel_ids: &[&str]) -> Vec<bool> {
        self.unregister_quad_panels(plugin_id, panel_ids)
    }
    fn validate_parametric_chain(
        &self,
        chain: &ocs_plugin_api::host::ParametricChain,
        tolerance: f64,
    ) -> Result<(), String> {
        self.validate_parametric_chain(chain, tolerance)
    }
}
