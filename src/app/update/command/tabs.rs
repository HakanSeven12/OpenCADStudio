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
            if self.tabs[idx].is_start {
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

    pub(in crate::app) fn on_tab_close(&mut self, idx: usize) -> Task<Message> {
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
        // A draft belongs to its original tab and cannot survive its closure.
        if idx == self.active_tab && self.mtext_editor.is_some() {
            self.mtext_cancel();
        }
        // Pending client interactive requests pinned to this drawing
        // can never be answered once the tab is gone — cancel them.
        let closing_tab_id = self.tabs.get(idx).map(|t| t.id);
        if let Some(closing_id) = closing_tab_id {
            self.queued_interaction_indices
                .retain(|(tab_id, ..)| *tab_id != closing_id);
        }
        let pinned = self
            .control
            .get_point
            .as_ref()
            .map(|s| s.document_id)
            .filter(|&id| Some(id) == closing_tab_id)
            .or_else(|| {
                self.control
                    .user_select
                    .as_ref()
                    .map(|s| s.document_id)
                    .filter(|&id| Some(id) == closing_tab_id)
            });
        if pinned.is_some() {
            self.resolve_get_point(None);
            self.resolve_user_select(false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let tab_id = self.tabs.get(idx).map(|t| t.id);
        // This tab is closing for good — drop its autosave recovery copy.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = std::fs::remove_file(self.autosave_target(idx));
        // …and, on the web, its retained in-place save handle: the next tab
        // to reuse that id must start with a fresh pick instead of silently
        // writing over the closed drawing's file.
        #[cfg(target_arch = "wasm32")]
        if let Some(closing_id) = closing_tab_id {
            crate::io::fs_access::drop_handle(closing_id);
        }
        // The drawing is gone, and the positions it had placed are not the
        // next drawing's: forget them with the tab.
        if let Some(closing_id) = closing_tab_id {
            crate::modules::draw::draw::attdef::session().forget_place_of(closing_id);
            crate::modules::annotate::text::forget_place_of(closing_id);
        }
        // Tool dialogs on the closing drawing go with it.
        if idx == self.active_tab {
            self.close_document_tools(None);
        }
        // Only-tab case: when the lone non-start tab closes, fall
        // back to the Start tab if it exists; otherwise spawn a
        // fresh blank drawing (legacy behaviour).
        if self.tabs.len() == 1 {
            self.tab_counter += 1;
            self.tabs[0] = crate::app::tab_document::DocumentTab::new_drawing(self.tab_counter);
            self.active_tab = 0;
            self.apply_display_defaults(0);
        } else {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
        // The active tab is now either a brand-new blank or a
        // different existing tab; in both cases the ribbon needs
        // to track that doc's defaults / selection. #21.
        if let Some(index) =
            test_return_id.and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
        {
            self.active_tab = index;
            self.adopt_header_sysvars(index);
        }
        self.ribbon
            .set_block_testing(self.tabs[self.active_tab].block_test.is_some());
        self.ribbon
            .set_block_editing(self.tabs[self.active_tab].active_block_edit.is_some());
        self.sync_ribbon_layers();
        self.sync_ribbon_styles();
        self.sync_ribbon_from_selection();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(tab_id) = tab_id {
            v4_support::on_tab_closed(tab_id);
        }
        // The tab list just shrank: refresh the remembered session list
        // (web session memory).
        #[cfg(target_arch = "wasm32")]
        self.persist_web_session();
        Task::none()
    }

}
