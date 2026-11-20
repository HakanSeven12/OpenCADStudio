// HostSession — plugin-facing API implemented inside `app` (private field access).

use std::any::Any;

use document::tables::AppId;
use document::xdata::ExtendedDataRecord;
use ocs_plugin_api::host::{
    Document, EntityType, Handle, HostApi, HostSettingValue, PluginClassParameterRow,
};
use ocs_plugin_api::shm::{DocumentSnapshotStore, DocumentViewData};

use super::OpenCADStudio;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugin::v4_support;

use super::dock::PLUGIN_PALLET_MENU_MAX_ROWS;

/// Session adapter: one active document tab, command line, undo.
pub(crate) struct HostSession<'a> {
    app: &'a mut OpenCADStudio,
    tab: usize,
    doc_store: Option<DocumentSnapshotStore<DocumentViewData>>,
}

/// A plugin request that drives commands: sets `plugin_request_active` (the
/// driven commands join the plugin's open undo scope) and optionally
/// suppresses the command-line echo. Both reset on drop, so a panicking
/// command cannot leave the host quiet or the scope joined.
struct PluginRequest<'s, 'a> {
    session: &'s mut HostSession<'a>,
    was_active: bool,
    quiet: bool,
}

impl<'s, 'a> PluginRequest<'s, 'a> {
    fn new(session: &'s mut HostSession<'a>, quiet: bool) -> Self {
        let was_active = std::mem::replace(&mut session.app.plugin_request_active, true);
        if quiet {
            session.app.command_line.begin_quiet_replay();
        }
        Self {
            session,
            was_active,
            quiet,
        }
    }
}

impl<'a> std::ops::Deref for PluginRequest<'_, 'a> {
    type Target = HostSession<'a>;
    fn deref(&self) -> &Self::Target {
        self.session
    }
}

impl std::ops::DerefMut for PluginRequest<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.session
    }
}

impl Drop for PluginRequest<'_, '_> {
    fn drop(&mut self) {
        if self.quiet {
            self.session.app.command_line.end_quiet_replay();
        }
        self.session.app.plugin_request_active = self.was_active;
    }
}

mod classes;
mod commands;
mod entities;
mod host_api;
mod interactive;
mod parametric_grips;
mod quad;
mod session;
mod solids;
mod tables;
mod validation;
mod xrecords;

pub(in crate::app) use self::classes::{
    find_design_record_entries, resolve_plugin_class_binding, rows_from_entries, rows_from_schema,
    PluginClassInstanceBinding,
};
pub(in crate::app) use self::interactive::convert_preview_wires;
pub(crate) use self::interactive::PluginProcessInteractiveAdapter;
pub use self::quad::plugin_quad_panels;
use self::validation::resolve_toggle_panel_id;

#[cfg(test)]
mod plugin_host_tests;

impl<'a> HostSession<'a> {
    pub(crate) fn new(app: &'a mut OpenCADStudio, tab: usize) -> Self {
        Self {
            app,
            tab,
            doc_store: None,
        }
    }

    pub fn tab_index(&self) -> usize {
        self.tab
    }

    pub fn tab_id(&self) -> u64 {
        self.app.tabs[self.tab].id
    }

    /// Resolve the legacy panel aliases, then enforce the authenticated
    /// transport owner. Alias lookup must not turn into an ownership bypass:
    /// a title/command that resolves to another plugin's panel is refused.
    fn owned_panel_id(&self, plugin_id: &str, panel_id: &str) -> Option<String> {
        let resolved = resolve_toggle_panel_id(self.app, panel_id)?;
        match self.app.plugin_panels.get(&resolved) {
            Some(panel) if panel.owner_plugin_id.as_deref() == Some(plugin_id) => Some(resolved),
            _ => None,
        }
    }

    pub fn document_path(&self, tab_id: u64) -> Option<std::path::PathBuf> {
        self.app
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.current_path.clone())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn document_view_v4(
        &mut self,
        tab_id: u64,
    ) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        if tab_id != self.tab_id() {
            return None;
        }
        // A failed open is honest: the error lands in the plugin error queue
        // surface (stderr log) and the caller sees None instead of a path
        // that cannot be served.
        v4_support::open_document_view_v4(tab_id, self.document()).ok()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn close_document_view_v4(&mut self, tab_id: u64) {
        if tab_id == self.tab_id() {
            v4_support::close_document_view_v4(tab_id);
        }
    }

    pub fn document(&self) -> &Document {
        &self.app.tabs[self.tab].scene.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.app.tabs[self.tab].scene.document
    }

    fn nested_attribute(&self, handle: Handle) -> Option<EntityType> {
        self.document().entities().find_map(|entity| {
            let EntityType::Insert(insert) = entity else {
                return None;
            };
            insert
                .attributes
                .iter()
                .find(|attribute| attribute.common.handle == handle)
                .cloned()
                .map(EntityType::AttributeEntity)
        })
    }

    fn host_model_entity(&self, handle: Handle) -> Option<EntityType> {
        self.document()
            .get_entity(handle)
            .cloned()
            .or_else(|| self.nested_attribute(handle))
    }

    fn replace_nested_attribute(&mut self, attribute: document::entities::AttributeEntity) -> bool {
        let owner = attribute.common.owner_handle;
        let handle = attribute.common.handle;
        let Some(EntityType::Insert(mut insert)) = self.document().get_entity(owner).cloned()
        else {
            return false;
        };
        let Some(slot) = insert
            .attributes
            .iter_mut()
            .find(|candidate| candidate.common.handle == handle)
        else {
            return false;
        };
        *slot = attribute;
        self.app.tabs[self.tab]
            .scene
            .update_entity_plugin(EntityType::Insert(insert))
    }

    pub fn document_view(&mut self) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        if self.doc_store.is_none() {
            let mut store =
                DocumentSnapshotStore::<DocumentViewData>::new(self.tab as u64, 8 * 1024 * 1024)
                    .ok()?;
            store.publish(&self.document().into()).ok()?;
            self.doc_store = Some(store);
        }
        let store = self.doc_store.as_ref()?;
        Some(ocs_plugin_api::shm::DocumentViewInfo {
            path: store.path().to_string_lossy().to_string(),
            version: store.version(),
        })
    }

    fn publish_document_view(&mut self) {
        let doc = &self.app.tabs[self.tab].scene.document;
        // Only publish views that have been opened by a consumer. V3 is lazily
        // created by document_view(); V4 is tracked per-tab by the V4 manager.
        if let Some(store) = self.doc_store.as_mut() {
            if let Err(e) = store.publish(&doc.into()) {
                eprintln!(
                    "[host] failed to publish document view for tab {}: {e}",
                    self.tab
                );
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let key = (self.tab_id(), self.app.tabs[self.tab].scene.geometry_epoch);
            let key_changed = self.app.last_plugin_document != Some(key);
            if key_changed {
                v4_support::publish_drawing_changed(key.0, key.1);
                // The plain `DocumentChanged` follows the same immediate
                // publish contract (API v9): a plugin-initiated write settles
                // here, so native plugins' `on_notification` sees it now —
                // the message boundary's fingerprint guard then dedupes.
                v4_support::publish_document_changed(key.1);
            }
            // API v13/P1-18: the snapshot publication is fallible. A failed
            // publication must NOT broadcast dirty regions or record the key
            // as settled — that would claim a boundary the plugins cannot
            // observe. The failure is logged; the next publish retries.
            match v4_support::publish_document_view_v4(self.tab_id(), doc) {
                Ok(()) => {
                    if key_changed {
                        // API v13 dirty regions: the plugin-write boundary
                        // carries the touched identity set, so consumers
                        // visit only what changed (empty sets mean "walk
                        // everything").
                        self.publish_plugin_dirty_regions();
                    }
                    self.app.last_plugin_document =
                        Some((self.tab_id(), self.app.tabs[self.tab].scene.geometry_epoch));
                }
                Err(e) => {
                    eprintln!(
                        "[host] document boundary for tab {} withheld: {e}",
                        self.tab
                    );
                }
            }
        }
    }

    /// Take this tab's accumulated dirty-region set and broadcast it as
    /// `DocumentChangedRegions` (API v13). The plugin-write path's twin of
    /// the message-boundary publisher in `update::mod`.
    #[cfg(not(target_arch = "wasm32"))]
    fn publish_plugin_dirty_regions(&mut self) {
        let tab = &mut self.app.tabs[self.tab];
        let (entities, records) = tab.take_plugin_dirty();
        v4_support::publish_document_changed_regions(
            tab.id,
            tab.scene.geometry_epoch,
            entities,
            records,
        );
    }

    fn geometry_epoch(&self) -> u64 {
        self.app.tabs[self.tab].scene.geometry_epoch
    }

    /// Plugin-driven edits refresh the app-side selection caches exactly
    /// like built-in edit paths: a selected entity's grip markers are cached
    /// and would otherwise rest at their pre-edit positions (found by the
    /// civil plugin's manual QA, 2026-10-12).
    fn refresh_grips_of_active_tab(&mut self) {
        // The grip cache mirrors the active tab's scene; a session may target
        // a background tab, whose edits must not disturb it.
        if self.tab == self.app.active_tab {
            self.app.refresh_selected_grips();
        }
    }
}
