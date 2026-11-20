use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn on_layer_toggle_vp_freeze(
        &mut self,
        layer_idx: usize,
        vp_col_idx: usize,
    ) -> Task<Message> {
        let i = self.active_tab;
        let vp_handle = self.tabs[i]
            .layers
            .vp_cols
            .get(vp_col_idx)
            .map(|c| c.handle);
        let layer_name = self.tabs[i]
            .layers
            .layers
            .get(layer_idx)
            .map(|l| l.name.clone());

        if let (Some(vp_handle), Some(layer_name)) = (vp_handle, layer_name) {
            // Get the layer handle from the document
            if let Some(doc_layer) = self.tabs[i].scene.document.layers.get(&layer_name) {
                let layer_handle = doc_layer.handle;
                self.push_undo_snapshot(i, "VPLAYER");

                // Toggle frozen_layers on the viewport entity
                for e in self.tabs[i].scene.document.entities_mut() {
                    if let document::EntityType::Viewport(vp) = e {
                        if vp.common.handle == vp_handle {
                            if vp.frozen_layers.contains(&layer_handle) {
                                vp.frozen_layers.retain(|h| h != &layer_handle);
                            } else {
                                vp.frozen_layers.push(layer_handle);
                            }
                            break;
                        }
                    }
                }

                // Re-sync layer panel with updated VP info
                let vp_info = self.tabs[i].scene.viewport_list();
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                self.sync_ribbon_layers();
                // Per-viewport layer visibility changes the resident
                // assembly, not any entity's tessellated geometry.
                self.tabs[i].scene.bump_geometry_no_blocks();
                self.tabs[i].dirty = true;
            }
        }
        Task::none()
    }

    pub(in crate::app) fn on_layer_new(&mut self) -> Task<Message> {
        self.on_layer_new_with(false)
    }

    /// A new layer, taking its look from the selected layer (colour,
    /// linetype, lineweight, transparency, plot) or the defaults without one
    /// (#2610); `frozen_in_viewports` also freezes it in every layout
    /// viewport. One undo step either way.
    pub(in crate::app) fn on_layer_new_with(&mut self, frozen_in_viewports: bool) -> Task<Message> {
        let i = self.active_tab;
        let template = self.tabs[i]
            .layers
            .selected
            .and_then(|row| self.tabs[i].layers.layers.get(row))
            .and_then(|row| self.tabs[i].scene.document.layers.get(&row.name))
            .cloned();
        let mut n = 1;
        let new_name = loop {
            let candidate = format!("Layer{}", n);
            if !self.tabs[i].scene.document.layers.contains(&candidate) {
                break candidate;
            }
            n += 1;
        };
        self.push_undo_snapshot(i, "LAYER NEW");
        use document::tables::layer::Layer as DocLayer;
        // A layer needs a real handle or it is dropped on a DWG save
        // (the format is handle-based; issue #67).
        let mut dl = DocLayer::new(&new_name);
        if let Some(template) = template {
            dl.color = template.color;
            dl.color_name = template.color_name.clone();
            dl.book_name = template.book_name.clone();
            dl.line_type = template.line_type.clone();
            dl.line_weight = template.line_weight;
            dl.transparency = template.transparency;
            dl.is_plottable = template.is_plottable;
        }
        // `allocate_handle` advances the seed so the layer gets a
        // unique handle; the non-advancing `next_handle` getter hands
        // out the same value twice and the later object overwrites it.
        dl.handle = self.tabs[i].scene.document.allocate_handle();
        // Viewports made later freeze it too.
        dl.flags.frozen_in_new_viewport = frozen_in_viewports;
        let layer_handle = dl.handle;
        let _ = self.tabs[i].scene.document.layers.add(dl);
        if frozen_in_viewports {
            for entity in self.tabs[i].scene.document.entities_mut() {
                if let document::EntityType::Viewport(vp) = entity {
                    if !vp.frozen_layers.contains(&layer_handle) {
                        vp.frozen_layers.push(layer_handle);
                    }
                }
            }
            self.tabs[i]
                .scene
                .invalidate_layer_visibility(&[new_name.clone()]);
        }
        self.tabs[i].dirty = true;
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        let new_idx = self.tabs[i]
            .layers
            .layers
            .iter()
            .position(|l| l.name == new_name);
        if let Some(idx) = new_idx {
            self.tabs[i].layers.selected = Some(idx);
            self.tabs[i].layers.selected_multi = vec![idx];
            self.tabs[i].layers.editing = Some(new_name.clone());
            self.tabs[i].layers.edit_buf = new_name.clone();
        }
        self.sync_ribbon_layers();
        // With alphabetical ordering (#270) the new layer can land
        // anywhere in a long list; scroll its row into view so the
        // rename prompt is visible (#271). Offset is layout-independent
        // (idx × fixed row height), so it holds even before the freshly
        // added row is measured.
        // The name field takes the keyboard, so typing renames the layer
        // instead of reaching the command line (#2610).
        match new_idx {
            Some(idx) => Task::batch([
                iced::widget::operation::scroll_to(
                    iced::advanced::widget::Id::new(
                        crate::ui::window::layers::LAYER_TABLE_SCROLL_ID,
                    ),
                    iced::widget::scrollable::AbsoluteOffset {
                        x: 0.0,
                        y: idx as f32 * crate::ui::window::layers::ROW_H,
                    },
                ),
                crate::app::update::focus_layer_name_field(),
            ]),
            None => Task::none(),
        }
    }

    pub(in crate::app) fn on_layer_delete(&mut self) -> Task<Message> {
        let i = self.active_tab;
        // Every selected layer (multi-select), or the anchor if none.
        let mut names = self.selected_layer_names(i);
        if names.is_empty() {
            return Task::none();
        }
        // Layer "0", Defpoints, the current layer and layers of an xref
        // can't be deleted — drop them from the batch and note it.
        let current = self.tabs[i]
            .scene
            .document
            .header
            .current_layer_name
            .clone();
        let before = names.len();
        names.retain(|n| !layer_is_protected(n, &current));
        if names.len() < before {
            self.command_line.push_info(
                crate::t!("Layer \"0\", Defpoints, the current layer and xref layers can't be deleted — skipped.")
                    .as_ref(),
            );
        }
        if names.is_empty() {
            return Task::none();
        }
        // Total objects across the layers. Empty → delete straight away;
        // non-empty → warn first (deleting also removes those objects).
        let count = self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| names.contains(&e.common().layer))
            .count();
        if count == 0 {
            self.push_undo_snapshot(i, "LAYER DELETE");
            for name in &names {
                self.tabs[i].scene.document.layers.remove(name);
            }
            self.tabs[i].dirty = true;
            self.sync_layer_panel(i);
        } else {
            self.layer_delete_pending = Some((names, count));
            self.active_modal = Some(crate::app::ModalKind::LayerDeleteWarning);
            self.modal_offset = iced::Vector::ZERO;
            self.modal_resize = iced::Vector::ZERO;
        }
        Task::none()
    }

    /// User confirmed deleting non-empty layer(s): erase every object on them,
    /// then remove the layer records.
    pub(in crate::app) fn on_layer_delete_confirm(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.active_modal = None;
        self.reset_modal_geometry();
        let Some((names, _)) = self.layer_delete_pending.take() else {
            return Task::none();
        };
        self.push_undo_snapshot(i, "LAYER DELETE");
        let handles: Vec<document::Handle> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| names.contains(&e.common().layer))
            .map(|e| e.common().handle)
            .collect();
        // Remove the layer records first so the lock guard in `erase_entities`
        // (which looks the layer up by name) can't block the objects.
        for name in &names {
            self.tabs[i].scene.document.layers.remove(name);
        }
        self.tabs[i].scene.erase_entities(&handles);
        self.tabs[i].dirty = true;
        self.sync_layer_panel(i);
        self.refresh_properties();
        Task::none()
    }

    /// Resolve a layer name to its current panel-row index at dispatch time.
    /// `Message::Layer*` carries the name (not the row) so a resort between
    /// view() and handler dispatch cannot mistarget; `None`
    /// (deleted/renamed meanwhile) lets the caller no-op gracefully (#22).
    pub(in crate::app) fn layer_panel_index(&self, i: usize, name: &str) -> Option<usize> {
        self.tabs[i]
            .layers
            .layers
            .iter()
            .position(|l| l.name == name)
    }

    /// Layers a Layer-manager row toggle (visibility / lock / freeze /
    /// transparency) should affect: the whole multi-selection when the clicked
    /// row is part of it, otherwise just the clicked row — so those toggles work
    /// in bulk like the color / linetype / lineweight edits already do (#236).
    pub(in crate::app) fn layer_row_action_targets(&self, i: usize, idx: usize) -> Vec<String> {
        let Some(clicked) = self.tabs[i].layers.layers.get(idx).map(|l| l.name.clone()) else {
            return Vec::new();
        };
        let selected = self.selected_layer_names(i);
        if selected.iter().any(|n| n == &clicked) {
            selected
        } else {
            vec![clicked]
        }
    }

    /// Layer names for the current Layer-manager selection — every row in the
    /// multi-selection, or the anchor row when the multi-set is empty. Bulk
    /// property edits and deletion act on these.
    pub(in crate::app) fn selected_layer_names(&self, i: usize) -> Vec<String> {
        let panel = &self.tabs[i].layers;
        let idxs: Vec<usize> = if panel.selected_multi.is_empty() {
            panel.selected.into_iter().collect()
        } else {
            panel.selected_multi.clone()
        };
        idxs.iter()
            .filter_map(|&x| panel.layers.get(x).map(|l| l.name.clone()))
            .collect()
    }

    /// Rebuild the Layer-manager panel + ribbon after a layer-table change.
    pub(in crate::app) fn sync_layer_panel(&mut self, i: usize) {
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        self.tabs[i].layers.selected = None;
        self.tabs[i].layers.selected_multi.clear();
        self.sync_ribbon_layers();
    }

    pub(in crate::app) fn on_layer_set_current(&mut self) -> Task<Message> {
        let i = self.active_tab;
        if let Some(idx) = self.tabs[i].layers.selected {
            if let Some(layer) = self.tabs[i].layers.layers.get(idx) {
                let name = layer.name.clone();
                if name == self.tabs[i].layers.current_layer {
                    return Task::none();
                }
                if layer.frozen {
                    self.command_line
                        .push_error(crate::t!("A frozen layer can't be made current.").as_ref());
                    return Task::none();
                }
                // Mirror the change into the document header (CLAYER) too,
                // not just the per-tab default. Otherwise the no-selection
                // ribbon refresh (e.g. after Esc) re-reads the stale header
                // layer and the dropdown snaps back to it. See #93.
                let handle = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .get(&name)
                    .map(|l| l.handle)
                    .unwrap_or(document::types::Handle::NULL);
                self.tabs[i].scene.document.header.current_layer_name = name.clone();
                self.tabs[i].scene.document.header.current_layer_handle = handle;
                self.tabs[i].active_layer = name.clone();
                self.tabs[i].layers.current_layer = name.clone();
                self.tabs[i].dirty = true;
                self.ribbon.active_layer = name;
            }
        }
        Task::none()
    }

    pub(in crate::app) fn on_layer_rename_commit(&mut self) -> Task<Message> {
        self.commit_layer_rename(true)
    }

    /// Apply the typed layer name. A name that can't be used says why (#2610).
    /// On Enter (`refocus`) the field stays open with what was typed and
    /// takes the keyboard again; a click elsewhere keeps the old name.
    pub(in crate::app) fn commit_layer_rename(&mut self, refocus: bool) -> Task<Message> {
        let i = self.active_tab;
        let Some(old_name) = self.tabs[i].layers.editing.take() else {
            return Task::none();
        };
        let new_name = self.tabs[i].layers.edit_buf.trim().to_string();
        // The name was captured at RenameStart; look it up fresh so a
        // sort/sync/filter in between cannot retarget the rename. Deleted
        // meanwhile → clear and no-op.
        let still_there = self.tabs[i]
            .layers
            .layers
            .iter()
            .any(|l| l.name == old_name);
        if !new_name.is_empty() && new_name != old_name && still_there {
            if let Some(problem) =
                layer_rename_problem(&self.tabs[i].scene.document, &old_name, &new_name)
            {
                self.command_line.push_error(&problem);
                if refocus {
                    self.tabs[i].layers.editing = Some(old_name);
                    return crate::app::update::focus_layer_name_field();
                }
                // A click elsewhere moves on; the old name stays.
                self.tabs[i].layers.edit_buf.clear();
                return Task::none();
            }
            self.push_undo_snapshot(i, "LAYER RENAME");
            if !self.tabs[i].rename_layer(&old_name, &new_name) {
                self.discard_last_undo_entry(i);
            }
        }
        self.tabs[i].layers.edit_buf.clear();
        self.refresh_layer_panel();
        Task::none()
    }

    /// A layer row's right-click menu. A row outside the selection becomes
    /// the selection first, as a click would.
    pub(in crate::app) fn on_layer_menu(
        &mut self,
        name: String,
        action: crate::ui::window::layers::LayerMenuAction,
    ) -> Task<Message> {
        use crate::ui::window::layers::LayerMenuAction as A;
        let i = self.active_tab;
        let Some(idx) = self.layer_panel_index(i, &name) else {
            return Task::none();
        };
        // Merging into this row keeps the selection it merges.
        let merge_sources = self.tabs[i].layers.selected_names();
        let panel = &mut self.tabs[i].layers;
        panel.keys_active = true;
        if action == A::MergeHere {
            // The selection stays as it is.
        } else if !panel.selected_multi.contains(&idx) {
            panel.selected_multi = vec![idx];
        }
        panel.selected = Some(idx);
        match action {
            A::SetCurrent => self.on_layer_set_current(),
            A::New => self.on_layer_new(),
            A::Rename => self.update(Message::LayerRenameStart(name)),
            A::Delete => self.on_layer_delete(),
            A::SelectObjects => {
                let layers = self.tabs[i].layers.selected_names();
                let count = self.tabs[i].scene.select_objects_on_layers(&layers);
                self.command_line.push_output(
                    crate::tf!("{} object(s) on the selected layers are selected.", count).as_ref(),
                );
                self.refresh_properties();
                Task::none()
            }
            A::NewGroup => self.update(Message::LayerGroupNew),
            // A ticked group: the selected layers are all in it, so the
            // item takes them out.
            A::AddToGroup(group) => {
                let panel = &self.tabs[i].layers;
                let selected = panel.selected_names();
                let all_in = panel.groups.iter().any(|(g, members)| {
                    g == &group
                        && selected
                            .iter()
                            .all(|s| members.iter().any(|m| m.eq_ignore_ascii_case(s)))
                });
                self.set_layer_group(i, &group, &selected, !all_in);
                Task::none()
            }
            A::RemoveFromGroup => self.update(Message::LayerGroupRemove),
            A::FilterObjects => {
                let layers = self.tabs[i].layers.selected_names().join(",");
                self.filter_open_for_layers(layers);
                Task::none()
            }
            A::MergeHere => {
                let sources: Vec<String> = merge_sources
                    .into_iter()
                    .filter(|source| !source.eq_ignore_ascii_case(&name))
                    .collect();
                if sources.is_empty() {
                    self.command_line.push_info(
                        crate::t!(
                            "Select the layers to merge, then right-click the one they go into."
                        )
                        .as_ref(),
                    );
                } else {
                    self.merge_layers(i, sources, name);
                }
                Task::none()
            }
        }
    }

    /// Keys for the Layer manager's table after a click in it: ↑/↓ move
    /// (Shift extends), Ctrl+A selects all, Del deletes, F2 renames and Esc
    /// clears the selection. `None` leaves the key to the rest of the app.
    pub(in crate::app) fn layer_manager_key(&mut self, key: &str) -> Option<Task<Message>> {
        use crate::ui::window::layers::{TreeAction, TreeNode};
        let i = self.active_tab;
        if key == "ESCAPE" && self.tabs[i].layers.tree_rename.is_some() {
            self.tabs[i].layers.tree_rename = None;
            return Some(Task::none());
        }
        let panel = &self.tabs[i].layers;
        if panel.tree_rename.is_some() {
            return None;
        }
        if !panel.keys_active
            || panel.editing.is_some()
            || panel.transparency_edit.is_some()
            || panel.description_edit.is_some()
            || !self.dock_panel_visible(crate::ui::dock::PanelId::Layers)
        {
            return None;
        }
        // A running command keeps Esc and Delete for itself.
        if self.tabs[i].active_cmd.is_some() && matches!(key, "ESCAPE" | "DELETE") {
            return None;
        }
        match key {
            "CTRL+A" => {
                let panel = &mut self.tabs[i].layers;
                panel.selected_multi = panel.visible_indices();
                Some(Task::none())
            }
            "DELETE"
                if panel.tree_focus
                    && matches!(panel.view, TreeNode::Group(_) | TreeNode::Filter(_)) =>
            {
                let node = panel.view.clone();
                Some(self.update(Message::LayerTreeAction(node, TreeAction::Delete)))
            }
            "DELETE" => Some(self.on_layer_delete()),
            "F2" if panel.tree_focus
                && matches!(panel.view, TreeNode::Group(_) | TreeNode::Filter(_)) =>
            {
                let node = panel.view.clone();
                Some(self.update(Message::LayerTreeAction(node, TreeAction::Rename)))
            }
            "F2" => {
                let name = panel
                    .selected
                    .and_then(|row| panel.layers.get(row))?
                    .name
                    .clone();
                Some(self.update(Message::LayerRenameStart(name)))
            }
            "ESCAPE" => {
                let panel = &mut self.tabs[i].layers;
                panel.selected = None;
                panel.selected_multi.clear();
                panel.keys_active = false;
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// ↑/↓ in the Layer manager's table: the row above or below the anchor
    /// among the rows shown; Shift extends the selection to it.
    pub(in crate::app) fn layer_manager_arrow(&mut self, down: bool, extend: bool) -> bool {
        let i = self.active_tab;
        let panel = &self.tabs[i].layers;
        if !panel.keys_active
            || panel.editing.is_some()
            || panel.transparency_edit.is_some()
            || panel.description_edit.is_some()
            || !self.dock_panel_visible(crate::ui::dock::PanelId::Layers)
        {
            return false;
        }
        let rows = panel.visible_indices();
        if rows.is_empty() {
            return true;
        }
        let at = panel
            .selected
            .and_then(|row| rows.iter().position(|r| *r == row));
        let next = match at {
            Some(at) if down => rows[(at + 1).min(rows.len() - 1)],
            Some(at) => rows[at.saturating_sub(1)],
            None if down => rows[0],
            None => rows[rows.len() - 1],
        };
        let panel = &mut self.tabs[i].layers;
        if extend {
            if !panel.selected_multi.contains(&next) {
                panel.selected_multi.push(next);
            }
        } else {
            panel.selected_multi = vec![next];
        }
        panel.selected = Some(next);
        true
    }

    /// Esc while the Layer manager is typing a name or a transparency drops
    /// what was typed. Returns whether there was something to drop.
    pub(in crate::app) fn cancel_layer_manager_edit(&mut self) -> bool {
        let layers = &mut self.tabs[self.active_tab].layers;
        if layers.editing.take().is_some() {
            layers.edit_buf.clear();
            return true;
        }
        layers.description_edit.take().is_some() || layers.transparency_edit.take().is_some()
    }

    /// Apply the typed description to the layer as one undo step (#2610).
    pub(in crate::app) fn on_layer_description_commit(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some((name, typed)) = self.tabs[i].layers.description_edit.take() else {
            return Task::none();
        };
        let typed = typed.trim().to_string();
        let unchanged = self.tabs[i]
            .scene
            .document
            .layers
            .get(&name)
            .is_none_or(|layer| layer.description == typed);
        if unchanged {
            return Task::none();
        }
        let undo = self.begin_layer_undo(i, "LAYER DESCRIPTION", std::slice::from_ref(&name));
        if let Some(layer) = self.tabs[i].scene.document.layers.get_mut(&name) {
            layer.description = typed.clone();
        }
        if let Some(row) = self.tabs[i]
            .layers
            .layers
            .iter_mut()
            .find(|l| l.name == name)
        {
            row.description = typed;
        }
        self.tabs[i].dirty = true;
        self.commit_layer_undo(i, undo);
        Task::none()
    }

    /// Count the objects on each layer for the Layer manager's Objects column
    /// and status, again only when the drawing or its layers changed and the
    /// manager is open. Objects inside block definitions count too, apart.
    pub(in crate::app) fn refresh_layer_counts(&mut self) {
        use std::hash::{Hash, Hasher};
        if self.tabs.get(self.active_tab).is_none() {
            return;
        }
        if !self.dock_panel_visible(crate::ui::dock::PanelId::Layers) {
            let i = self.active_tab;
            if !self.tabs[i].layers.eye_layers.is_empty() {
                self.tabs[i].layers.eye_layers.clear();
                self.tabs[i].layers.highlighted = 0;
                self.tabs[i].layers.highlight_key = None;
                self.tabs[i].scene.set_filter_preview_highlights(&[]);
            }
            return;
        }
        let i = self.active_tab;
        self.refresh_layer_tree(i);
        self.refresh_layer_highlight(i);
        let scene = &self.tabs[i].scene;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for layer in scene.document.layers.iter() {
            layer.name.hash(&mut hasher);
        }
        let key = (scene.geometry_epoch, hasher.finish());
        if self.tabs[i].layers.counts_key == Some(key) {
            return;
        }
        let mut counts: std::collections::HashMap<String, (usize, usize)> = Default::default();
        for record in scene.document.block_records.iter() {
            let in_block = !record.is_layout();
            for handle in &record.entity_handles {
                if let Some(entity) = scene.document.get_entity(*handle) {
                    let entry = counts
                        .entry(entity.common().layer.to_lowercase())
                        .or_default();
                    entry.0 += 1;
                    if in_block {
                        entry.1 += 1;
                    }
                }
            }
        }
        self.tabs[i].layers.set_counts(counts, key);
    }

    /// FILTER over the layers: the LAYERFILTER filter `name`, or a new one
    /// starting from what the search box holds (#2610).
    pub(in crate::app) fn on_layer_filter_edit(&mut self, name: Option<String>) -> Task<Message> {
        use crate::filter::{Condition, Filter, Item, Match, Op};
        let i = self.active_tab;
        self.load_layer_filters(i);
        // `Some("")`: a new, empty filter (the toolbar's New Filter); `None`
        // starts from the search text (Advanced…).
        let blank = name.as_deref() == Some("");
        let (name, filter) = match name.and_then(|name| {
            self.layer_filters
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(&name))
                .map(|f| (f.name.clone(), f.to_filter()))
        }) {
            Some(found) => found,
            None => {
                let search = if blank {
                    String::new()
                } else {
                    self.tabs[i].layers.filter.trim().to_string()
                };
                let condition = if search.is_empty() {
                    Condition::default()
                } else {
                    // The search box finds names containing the text.
                    let value = if search.contains(['*', '?', ',']) {
                        search
                    } else {
                        format!("*{search}*")
                    };
                    Condition {
                        property: Some("name".into()),
                        op: Op::Eq,
                        value,
                        ..Default::default()
                    }
                };
                (
                    String::new(),
                    Filter {
                        root: Match::All,
                        items: vec![Item::Condition(condition)],
                    },
                )
            }
        };
        self.filter_open_layers(name, filter);
        Task::none()
    }

    /// Save as filter: the search text as a property filter on the name,
    /// shown in the tree (#2610 LM-22a).
    pub(in crate::app) fn on_layer_save_search(&mut self) -> Task<Message> {
        use crate::filter::{Condition, Filter, Item, Match, Op};
        use crate::ui::window::layers::TreeNode;
        let i = self.active_tab;
        let search = self.tabs[i].layers.filter.trim().to_string();
        if search.is_empty() {
            return Task::none();
        }
        self.load_layer_filters(i);
        // The search finds names containing the text; a pattern stays one.
        let value = if search.contains(['*', '?', ',']) {
            search.clone()
        } else {
            format!("*{search}*")
        };
        let mut name = search.clone();
        let mut n = 2;
        while crate::app::layer_filters::find_filter_mut(&mut self.layer_filters, &name).is_some() {
            name = format!("{search} ({n})");
            n += 1;
        }
        self.layer_filters
            .push(crate::app::layer_filters::LayerFilter {
                name: name.clone(),
                conditions: Some(Filter {
                    root: Match::All,
                    items: vec![Item::Condition(Condition {
                        property: Some("name".into()),
                        op: Op::Eq,
                        value,
                        ..Default::default()
                    })],
                }),
                ..Default::default()
            });
        self.persist_layer_filters(i, "LAYERFILTER");
        let panel = &mut self.tabs[i].layers;
        panel.filter.clear();
        panel.view = TreeNode::Filter(name);
        self.refresh_layer_counts();
        Task::none()
    }

    /// The tree's rename field: Enter. A group is renamed in the drawing, a
    /// filter in the list; an empty or taken name leaves it as it was.
    pub(in crate::app) fn on_layer_tree_rename_commit(&mut self) -> Task<Message> {
        use crate::ui::window::layers::TreeNode;
        let i = self.active_tab;
        let Some((node, typed)) = self.tabs[i].layers.tree_rename.take() else {
            return Task::none();
        };
        let new = typed.trim().to_string();
        match node {
            TreeNode::Group(old) if !new.is_empty() && new != old => {
                let taken =
                    self.tabs[i].layers.groups.iter().any(|(g, _)| {
                        g.eq_ignore_ascii_case(&new) && !g.eq_ignore_ascii_case(&old)
                    });
                if taken {
                    self.command_line
                        .push_error(&crate::tr!("layer-groups", "exists", name = new));
                } else {
                    self.rename_layer_group_named(i, &old, &new);
                    self.tabs[i].layers.view = TreeNode::Group(new);
                }
            }
            TreeNode::Filter(old) if !new.is_empty() && new != old => {
                self.load_layer_filters(i);
                let taken = !new.eq_ignore_ascii_case(&old)
                    && crate::app::layer_filters::find_filter_mut(&mut self.layer_filters, &new)
                        .is_some();
                if taken {
                    self.command_line
                        .push_error(&crate::tf!("A filter named \"{}\" already exists.", new));
                } else if let Some(filter) =
                    crate::app::layer_filters::find_filter_mut(&mut self.layer_filters, &old)
                {
                    filter.name = new.clone();
                    self.persist_layer_filters(i, "LAYERFILTER RENAME");
                    if self.tabs[i].layers.view == TreeNode::Filter(old) {
                        self.tabs[i].layers.view = TreeNode::Filter(new);
                    }
                    self.refresh_layer_counts();
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// A filter tree node's right-click menu: edit or delete a LAYERFILTER
    /// filter, or switch all of a node's layers at once (one undo step; the
    /// current layer is never frozen).
    pub(in crate::app) fn on_layer_tree_action(
        &mut self,
        node: crate::ui::window::layers::TreeNode,
        action: crate::ui::window::layers::TreeAction,
    ) -> Task<Message> {
        use crate::ui::window::layers::{TreeAction as A, TreeNode};
        let i = self.active_tab;
        match (&node, action) {
            (TreeNode::Filter(name), A::Edit) => {
                return self.on_layer_filter_edit(Some(name.clone()));
            }
            (TreeNode::Filter(name), A::Delete) => {
                self.load_layer_filters(i);
                crate::app::layer_filters::remove_filter(&mut self.layer_filters, name);
                self.persist_layer_filters(i, "LAYERFILTER DELETE");
                if self.tabs[i].layers.view == node {
                    self.tabs[i].layers.view = TreeNode::All;
                }
                self.refresh_layer_counts();
                return Task::none();
            }
            // A group goes from the tree's menu; its layers stay. Called
            // directly: a name with a space would not survive the command
            // line.
            (TreeNode::Group(name), A::Delete) => {
                let name = name.clone();
                if self.tabs[i].layers.view == node {
                    self.tabs[i].layers.set_group(None);
                }
                self.delete_layer_group_named(i, &name);
                return Task::none();
            }
            (TreeNode::Group(name) | TreeNode::Filter(name), A::Rename) => {
                let panel = &mut self.tabs[i].layers;
                panel.tree_rename = Some((node.clone(), name.clone()));
                return iced::widget::operation::focus(
                    crate::ui::window::layers::tree_rename_input_id(),
                )
                .chain(iced::widget::operation::select_all(
                    crate::ui::window::layers::tree_rename_input_id(),
                ));
            }
            (TreeNode::Unused, A::Purge) => {
                let unused = self.tabs[i].layers.node_layers(&TreeNode::Unused);
                self.open_purge_dialog();
                if let Some(state) = self.purge_dialog.as_mut() {
                    for (name, ticked) in &mut state.items[0] {
                        *ticked = unused.iter().any(|u| u.eq_ignore_ascii_case(name));
                    }
                }
                return Task::none();
            }
            (_, A::Edit | A::Delete | A::Rename | A::Purge) => return Task::none(),
            _ => {}
        }
        let mut names = self.tabs[i].layers.node_layers(&node);
        let current = self.tabs[i]
            .scene
            .document
            .header
            .current_layer_name
            .clone();
        if action == A::Freeze {
            names.retain(|name| !name.eq_ignore_ascii_case(&current));
        }
        let label = format!("LAYER {action:?}").to_uppercase();
        match action {
            A::On => self.edit_layers(i, &label, &names, |layer| layer.turn_on()),
            A::Off => self.edit_layers(i, &label, &names, |layer| layer.turn_off()),
            A::Freeze => self.edit_layers(i, &label, &names, |layer| layer.freeze()),
            A::Thaw => self.edit_layers(i, &label, &names, |layer| layer.thaw()),
            A::Lock => self.edit_layers(i, &label, &names, |layer| layer.lock()),
            A::Unlock => self.edit_layers(i, &label, &names, |layer| layer.unlock()),
            A::Edit | A::Delete | A::Rename | A::Purge => {}
        }
        self.refresh_layer_panel();
        Task::none()
    }

    /// With Show on and no selection dialog open, light the selected layers'
    /// objects in the drawing; again only when the layers or the drawing
    /// changed.
    pub(in crate::app) fn refresh_layer_highlight(&mut self, i: usize) {
        if self.tabs[i].layers.eye_layers.is_empty()
            || self.filter_dialog.is_some()
            || self.qselect.is_some()
        {
            // The dialog owns the highlight now; recompute once it closes.
            self.tabs[i].layers.highlight_key = None;
            return;
        }
        let names = self.tabs[i].layers.eye_layers.clone();
        let key = (self.tabs[i].scene.geometry_epoch, names.clone());
        if self.tabs[i].layers.highlight_key.as_ref() == Some(&key) {
            return;
        }
        let handles = self.tabs[i].scene.objects_on_layers(&names);
        self.tabs[i].scene.set_filter_preview_highlights(&handles);
        self.tabs[i].layers.highlighted = handles.len();
        self.tabs[i].layers.highlight_key = Some(key);
    }

    /// The filter tree's live parts: the layers of the selected objects and
    /// what each LAYERFILTER filter matches. Set only when they changed.
    pub(in crate::app) fn refresh_layer_tree(&mut self, i: usize) {
        // This runs on every message while the panel shows. Its inputs are the
        // selected objects' layers, the layer table, the filters the drawing
        // keeps and the ribbon setting; folding them into one key keeps an idle
        // message from reloading the filters or walking the layers.
        let key = layer_tree_key(&self.tabs[i].scene, self.ribbon_layers_follow_filter);
        if self.tabs[i].layers.tree_key == Some(key) {
            return;
        }
        #[cfg(test)]
        TREE_EXPENSIVE_RUNS.with(|runs| runs.set(runs.get() + 1));
        self.load_layer_filters(i);
        self.tabs[i].layers.ribbon_follows = self.ribbon_layers_follow_filter;
        self.tabs[i].layers.tree_key = Some(key);
        let scene = &self.tabs[i].scene;
        let mut selected: Vec<String> = scene
            .selected
            .iter()
            .filter_map(|h| scene.document.get_entity(*h))
            .map(|e| e.common().layer.to_lowercase())
            .collect();
        selected.sort();
        selected.dedup();
        // Every filter, a nested one right after its parent, its layers those
        // its own conditions and all of its parents' take (#2610).
        fn walk(
            list: &[crate::app::layer_filters::LayerFilter],
            parents: &mut Vec<crate::filter::Filter>,
            depth: u8,
            document: &document::Document,
            out: &mut Vec<(String, Vec<String>)>,
            depths: &mut Vec<u8>,
        ) {
            let source = crate::app::layer_filters::LayerProperties;
            for filter in list {
                let own = filter.to_filter();
                let members = document
                    .layers
                    .iter()
                    .filter(|layer| !layer.name.starts_with('*'))
                    .filter(|layer| own.matches(&source, *layer))
                    .filter(|layer| parents.iter().all(|p| p.matches(&source, *layer)))
                    .map(|layer| layer.name.clone())
                    .collect();
                out.push((filter.name.clone(), members));
                depths.push(depth);
                parents.push(own);
                walk(&filter.children, parents, depth + 1, document, out, depths);
                parents.pop();
            }
        }
        let mut filters: Vec<(String, Vec<String>)> = Vec::new();
        let mut depths = Vec::new();
        walk(
            &self.layer_filters,
            &mut Vec::new(),
            0,
            &scene.document,
            &mut filters,
            &mut depths,
        );
        let panel = &mut self.tabs[i].layers;
        if panel.selected_object_layers != selected {
            panel.selected_object_layers = selected;
        }
        if panel.filters != filters {
            if let crate::ui::window::layers::TreeNode::Filter(name) = &panel.view {
                if !filters.iter().any(|(f, _)| f.eq_ignore_ascii_case(name)) {
                    panel.view = crate::ui::window::layers::TreeNode::All;
                }
            }
            panel.filters = filters;
        }
        panel.filter_depths = depths;
    }

    /// Apply the typed transparency to the layer (and the selection it is
    /// part of) as one undo step. A value that isn't a whole number from 0 to
    /// 90 says so and the field shows the layer's value again (#2610).
    pub(in crate::app) fn on_layer_transparency_commit(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some((name, typed)) = self.tabs[i].layers.transparency_edit.take() else {
            return Task::none();
        };
        let Some(idx) = self.layer_panel_index(i, &name) else {
            return Task::none();
        };
        let typed = typed.trim();
        if typed.is_empty() {
            return Task::none();
        }
        let Some(value) = typed.parse::<i32>().ok().filter(|v| (0..=90).contains(v)) else {
            self.command_line
                .push_error(crate::t!("Transparency is a whole number from 0 to 90.").as_ref());
            return Task::none();
        };
        let targets: Vec<String> = self
            .layer_row_action_targets(i, idx)
            .into_iter()
            .filter(|target| {
                self.tabs[i]
                    .layers
                    .layers
                    .iter()
                    .any(|l| &l.name == target && l.transparency != value)
            })
            .collect();
        if targets.is_empty() {
            return Task::none();
        }
        let undo = self.begin_layer_undo(i, "LAYER TRANSPARENCY", &targets);
        for target in &targets {
            if let Some(layer) = self.tabs[i].scene.document.layers.get_mut(target) {
                layer.transparency =
                    document::types::Transparency::from_percent(value as f64 / 100.0);
            }
            if let Some(pl) = self.tabs[i]
                .layers
                .layers
                .iter_mut()
                .find(|l| &l.name == target)
            {
                pl.transparency = value;
            }
        }
        self.tabs[i].scene.invalidate_layer_dependencies(&targets);
        self.tabs[i].dirty = true;
        self.commit_layer_undo(i, undo);
        Task::none()
    }

}
