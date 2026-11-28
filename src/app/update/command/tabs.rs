use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn begin_tab_close_queue(&mut self, tab_ids: Vec<u64>) -> Task<Message> {
        self.pending_tab_closes.clear();
        self.pending_tab_closes.extend(tab_ids);
        self.continue_tab_close_queue()
    }

    /// Close queued drawings until a dirty tab requires confirmation. Queue
    /// entries use stable document ids because removing an earlier tab changes
    /// every later vector index.
    pub(in crate::app) fn continue_tab_close_queue(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        while let Some(tab_id) = self.pending_tab_closes.pop_front() {
            let Some(idx) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                continue;
            };
            if self.tabs[idx].is_start || self.plugin_run_holds_tab(tab_id) {
                continue;
            }
            if self.tabs[idx].active_block_edit.is_some() || self.tabs[idx].block_test.is_some() {
                tasks.push(self.on_tab_close(idx));
                if self.pending_block_close.is_some() || self.pending_close.is_some() {
                    break;
                }
                continue;
            }
            if self.tabs[idx].dirty {
                self.pending_close = Some(crate::app::PendingClose::Tab(tab_id));
                tasks.push(self.open_unsaved_dialog_window());
                break;
            }
            tasks.push(self.on_tab_close(idx));
        }
        Task::batch(tasks)
    }

    /// Remove tab `idx` and run full cleanup (MText/tool/grip/attr/modal/automation
    /// teardown, autosave removal, and placed-position cleanup), preserving the active
    /// tab when closing a background tab.
    pub(in crate::app) fn close_tab_at(
        &mut self,
        idx: usize,
        return_id: Option<u64>,
        remove_web_autosave: bool,
    ) {
        if idx >= self.tabs.len() {
            return;
        }
        let prev_active_id = self.tabs.get(self.active_tab).map(|tab| tab.id);
        let closing_id = self.tabs[idx].id;
        let closing_was_active = idx == self.active_tab;

        self.cancel_attr_editor();
        self.table_popup = None;
        if self.focus_dialog(crate::app::ModalKind::InsertAttributes) {
            self.cancel_insert_attr_dialog();
            self.close_active_modal();
        }
        if closing_was_active {
            self.cancel_active_grip_edit();
            self.grip_pending = None;
            self.visibility_popup = None;
            self.grip_popup = None;
            self.grip_hover = None;
            if self.mtext_editor.is_some() {
                self.mtext_cancel();
            }
            self.close_document_tools(None);
        }
        self.queued_interaction_indices
            .retain(|(tab_id, ..)| *tab_id != closing_id);
        let pinned = self
            .control
            .get_point
            .as_ref()
            .map(|s| s.document_id)
            .filter(|&id| id == closing_id)
            .or_else(|| {
                self.control
                    .user_select
                    .as_ref()
                    .map(|s| s.document_id)
                    .filter(|&id| id == closing_id)
            });
        if pinned.is_some() {
            self.resolve_get_point(None);
            self.resolve_user_select(false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = std::fs::remove_file(self.autosave_target(idx));
        #[cfg(target_arch = "wasm32")]
        {
            crate::io::fs_access::drop_handle(closing_id);
            if remove_web_autosave && !crate::io::host_bridge::is_embed_mode() {
                let key = Self::web_autosave_key(&self.tabs[idx]);
                if self.web_autosave_in_flight {
                    self.web_autosave_clear_after.push(key);
                } else {
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = crate::io::web_autosave::remove(&key).await;
                    });
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = remove_web_autosave;

        crate::modules::draw::draw::attdef::session().forget_place_of(closing_id);
        crate::modules::annotate::text::forget_place_of(closing_id);

        if self.tabs.len() == 1 {
            self.tab_counter += 1;
            self.tabs[0] = crate::app::tab_document::DocumentTab::new_drawing(self.tab_counter);
            self.active_tab = 0;
            self.apply_display_defaults(0);
        } else {
            self.tabs.remove(idx);
            if let Some(target_id) = return_id {
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == target_id) {
                    self.active_tab = index;
                    self.adopt_header_sysvars(index);
                } else {
                    self.active_tab = idx.min(self.tabs.len() - 1);
                    self.adopt_header_sysvars(self.active_tab);
                }
            } else if !closing_was_active {
                self.active_tab = prev_active_id
                    .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
                    .unwrap_or_else(|| self.active_tab.min(self.tabs.len() - 1));
            } else {
                self.active_tab = idx.min(self.tabs.len() - 1);
                self.adopt_header_sysvars(self.active_tab);
            }
        }

        self.ribbon
            .set_block_testing(self.tabs[self.active_tab].block_test.is_some());
        self.ribbon
            .set_block_editing(self.tabs[self.active_tab].active_block_edit.is_some());
        self.sync_ribbon_layers();
        self.sync_ribbon_styles();
        self.sync_ribbon_from_selection();
        #[cfg(not(target_arch = "wasm32"))]
        v4_support::on_tab_closed(closing_id);
        #[cfg(target_arch = "wasm32")]
        self.persist_web_session();
    }

    pub(in crate::app) fn on_tab_close(&mut self, idx: usize) -> Task<Message> {
        // The drawing a plugin command works on stays until it ends.
        let tab_id = self.tabs.get(idx).map(|tab| tab.id);
        if tab_id.is_some_and(|id| self.plugin_run_holds_tab(id)) {
            return Task::none();
        }
        let test_return_id = self
            .tabs
            .get(idx)
            .and_then(|tab| tab.block_test.as_ref())
            .map(|test| {
                if idx == self.active_tab {
                    test.source_id
                } else {
                    self.tabs[self.active_tab].id
                }
            });
        if test_return_id.is_some() {
            if idx == self.active_tab {
                self.cancel_active_grip_edit();
                self.grip_pending = None;
                self.visibility_popup = None;
                self.table_popup = None;
                self.grip_popup = None;
                self.grip_hover = None;
            }
            self.tabs[idx].dirty = false;
        }
        // Start tab is fixed — close requests on it are no-ops.
        if self.tabs.get(idx).is_some_and(|t| t.is_start) {
            return Task::none();
        }
        if self
            .tabs
            .get(idx)
            .is_some_and(|tab| tab.active_block_edit.is_some())
        {
            return self.request_block_edit_close(
                idx,
                crate::app::block_editor::BlockEditContinuation::Tab(self.tabs[idx].id),
            );
        }
        // Closing a tab shifts indices / the active tab. The attribute
        // editor holds a document-local handle into one tab, so drop it
        // now rather than risk it applying to a different tab's document.
        self.cancel_attr_editor();
        // The Block Properties Table list names an insert of the tab being
        // closed, so it goes with that tab.
        self.table_popup = None;
        // The insertion attribute form parks a placement on the tab it belongs
        // to, so a closed tab has to take the form — and the placement — with it.
        if self.focus_dialog(crate::app::ModalKind::InsertAttributes) {
            self.cancel_insert_attr_dialog();
            self.close_active_modal();
        }
        if self.tabs.get(idx).is_some_and(|t| t.dirty) {
            self.pending_close = Some(crate::app::PendingClose::Tab(self.tabs[idx].id));
            return self.open_unsaved_dialog_window();
        }
        self.close_tab_at(idx, test_return_id, false);
        Task::none()
    }

}
