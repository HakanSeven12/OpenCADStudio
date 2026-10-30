//! App side of the LAYWALK pallet.
//!
//! The pallet's own state, pure logic and view live in
//! [`crate::ui::window::layer_walk`]; this file is the only place the walk
//! touches the drawing. Everything here is a session-only override
//! (`Scene::set_space_layer_walk`): the layer table stays untouched while
//! walking, the drawing is not marked dirty and undo records nothing, so
//! closing the pallet with *Restore on exit* — the default — returns the
//! drawing to exactly what it was.
//!
//! Rows are rebuilt from the layer table by [`OpenCADStudio::walk_rebuild`],
//! which `refresh_layer_panel` calls while a walk is open. That is how layers
//! created, renamed or deleted by another command mid-walk reach the list.

use iced::{Element, Fill, Task};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{DocumentTab, Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::ui::dock::PanelId;
use crate::ui::window::layer_walk::{
    self, LayerWalkMenuItem, LayerWalkMsg, LayerWalkRow, LayerWalkSession, LayerWalkSort,
};
use codec::Handle;

impl OpenCADStudio {
    // ── Open / close ──────────────────────────────────────────────────────

    /// LAYWALK: open the pallet with a fresh walk, or close the open one
    /// exactly as the Close button would, so *Restore on exit* is honoured.
    pub(super) fn toggle_layer_walk(&mut self) -> Task<Message> {
        let open = self.tabs[self.active_tab].layer_walk.is_some();
        if open {
            self.close_layer_walk(false);
        } else {
            self.open_layer_walk();
        }
        Task::none()
    }

    /// Open the pallet, selecting every layer currently visible.
    pub(super) fn open_layer_walk(&mut self) {
        let i = self.active_tab;
        let rows = self.walk_rows_now();
        let session = LayerWalkSession::fresh(&rows);
        {
            let tab = &mut self.tabs[i];
            tab.layer_walk_rows = rows;
            tab.layer_walk = Some(session);
        }
        self.walk_apply();
        self.dock_open_at_default(PanelId::Laywalk);
        self.ribbon.activate_tool("LAYWALK");
    }

    /// Close the pallet. `keep_result` is the Keep Result button; otherwise
    /// the footer's *Restore on exit* checkbox decides, which is the whole
    /// contract for the close paths.
    pub(super) fn close_layer_walk(&mut self, keep_result: bool) {
        let i = self.active_tab;
        let commit = keep_result
            || self.tabs[i]
                .layer_walk
                .as_ref()
                .is_some_and(|s| !s.restore_on_exit);
        let committed = commit && self.walk_commit();
        self.abandon_layer_walk();
        if committed {
            self.refresh_layer_panel();
            self.command_line
                .push_output(crate::tr!("layer-walk", "kept").as_ref());
        } else {
            self.command_line
                .push_output(crate::tr!("layer-walk", "restored").as_ref());
        }
    }

    /// Tear the walk down without committing: its rows, its override and its
    /// ribbon highlight all go. Every close path ends here, so a drawing can
    /// never be left half-hidden.
    pub(super) fn abandon_layer_walk(&mut self) {
        let i = self.active_tab;
        self.abandon_layer_walk_on(i);
    }

    /// The same teardown for one tab. Command dispatch runs against whichever
    /// drawing the command names, which is not always the active one, so the
    /// ribbon highlight and the pallet only follow when it is.
    pub(super) fn abandon_layer_walk_on(&mut self, tab: usize) {
        self.reset_layer_walk_for(tab);
        self.tabs[tab].scene.bump_geometry_no_blocks();
        if tab == self.active_tab {
            self.ribbon.deactivate_tool_if("LAYWALK");
            self.dock_unpeek(PanelId::Laywalk);
        }
    }

    /// Drop a walk on one tab — used by the tab-close, tab-switch and
    /// document-load paths, where committing would be wrong.
    pub(super) fn reset_layer_walk_for(&mut self, tab: usize) {
        let tab = &mut self.tabs[tab];
        tab.layer_walk = None;
        tab.layer_walk_rows = Vec::new();
        tab.layer_walk_picking = false;
        tab.scene.clear_all_layer_walks();
    }

    // ── View ──────────────────────────────────────────────────────────────

    pub(super) fn layer_walk_view<'a>(
        &'a self,
        tab: &'a DocumentTab,
        width: f32,
        chrome: crate::ui::dock::Chrome,
    ) -> Element<'a, Message> {
        let Some(session) = tab.layer_walk.as_ref() else {
            return iced::widget::Space::new().width(Fill).height(Fill).into();
        };
        layer_walk::view(
            &tab.layer_walk_rows,
            session,
            &tab.layer_walk_combo,
            &tab.layer_walk_filters,
            tab.layer_walk_rows.len(),
            self.ctrl_down,
            self.shift_down,
            width,
            chrome,
        )
    }

    /// Whether the arrow keys belong to the walk list: a row was clicked and
    /// nothing else took the keyboard since.
    pub(super) fn layer_walk_owns_arrows(&self) -> bool {
        self.tabs[self.active_tab]
            .layer_walk
            .as_ref()
            .is_some_and(|s| s.list_focused)
    }

    /// The viewport (or any other widget) took the keyboard away from the list.
    pub(super) fn layer_walk_lost_focus(&mut self) {
        if let Some(session) = self.tabs[self.active_tab].layer_walk.as_mut() {
            session.list_focused = false;
        }
    }

    /// The space being interacted with changed (layout switch, viewport
    /// enter/exit): the walk follows it. The pallet always describes the
    /// drawing on screen, and only that space ever carries an override.
    pub(super) fn walk_space_changed(&mut self) {
        if self.tabs[self.active_tab].layer_walk.is_some() {
            self.walk_apply();
        }
    }

    // ── Rows & override ───────────────────────────────────────────────────

    /// Per-layer object counts plus the PURGE reference set, both keyed by the
    /// uppercased layer name. The rules are PURGE's verbatim, so a row's count
    /// and its purgeability agree with what `PURGE LAyers` does.
    fn walk_layer_stats(&self) -> (FxHashMap<String, usize>, FxHashSet<String>) {
        let doc = &self.tabs[self.active_tab].scene.document;
        let mut counts: FxHashMap<String, usize> = FxHashMap::default();
        let mut used: FxHashSet<String> = FxHashSet::default();
        let mut note = |layer: &str| {
            if layer.is_empty() {
                return;
            }
            let key = layer.to_uppercase();
            *counts.entry(key.clone()).or_insert(0) += 1;
            used.insert(key);
        };
        for e in doc.entities() {
            note(&e.common().layer);
        }
        // An INSERT holds its attributes inside itself, so their layers are
        // referenced by the instance and appear nowhere in the entity list.
        for e in doc.entities() {
            if let codec::EntityType::Insert(ins) = e {
                for a in &ins.attributes {
                    note(&a.common.layer);
                }
            }
        }
        // The current layer is never unreferenced, empty or not (PURGE's rule).
        used.insert(doc.header.current_layer_name.to_uppercase());
        (counts, used)
    }

    fn walk_rows_now(&self) -> Vec<LayerWalkRow> {
        let tab = &self.tabs[self.active_tab];
        let (counts, used) = self.walk_layer_stats();
        let layers = layer_walk::walk_eligible(&tab.scene.document.layers);
        let (sort, asc) = tab
            .layer_walk
            .as_ref()
            .map(|s| (s.sort, s.sort_asc))
            .unwrap_or((LayerWalkSort::Name, true));
        layer_walk::sync_rows(
            &layers,
            &counts,
            &used,
            &tab.scene.document.header.current_layer_name,
            sort,
            asc,
        )
    }

    /// Re-read the layer table into the pallet and re-install the override.
    /// `refresh_layer_panel` calls this while a walk is open, so a layer
    /// added, renamed or erased by any other command keeps the list honest.
    pub(super) fn walk_rebuild(&mut self) {
        let i = self.active_tab;
        if self.tabs[i].layer_walk.is_none() {
            return;
        }
        let rows = self.walk_rows_now();
        {
            let tab = &mut self.tabs[i];
            let names: FxHashSet<&str> = rows.iter().map(|r| r.name.as_str()).collect();
            if let Some(session) = tab.layer_walk.as_mut() {
                session.selected.retain(|n| names.contains(n.as_str()));
                session.held.retain(|n| names.contains(n.as_str()));
                // Row indices die with the rebuild; keyboard navigation
                // re-seeds from the surviving selection.
                session.anchor = None;
                session.drag_start = None;
                // A saved filter whose name was just erased stays a saved
                // filter — the user can still re-apply or delete it.
            }
            tab.layer_walk_rows = rows;
        }
        self.walk_apply();
    }

    /// Install the session's visible set as the override for the space being
    /// interacted with — model space, or the entered layout viewport. Other
    /// spaces' walks are dropped, so at most one space is ever walked.
    pub(super) fn walk_apply(&mut self) {
        let i = self.active_tab;
        let space = self.tabs[i].scene.interaction_space_key_pub();
        let (mut visible, covers_every_row) = {
            let tab = &self.tabs[i];
            let Some(session) = tab.layer_walk.as_ref() else {
                return;
            };
            let visible = layer_walk::visible_handles(&tab.layer_walk_rows, session);
            let full = visible.len() >= tab.layer_walk_rows.len();
            (visible, full)
        };
        // The walk only manages the layers it lists, so a `*`-prefixed system
        // layer joins the set rather than being hidden by omission.
        for l in self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .filter(|l| l.name.starts_with('*'))
        {
            visible.insert(l.handle);
        }
        let tab = &mut self.tabs[i];
        tab.scene.clear_other_space_layer_walks(space);
        tab.scene.set_space_layer_walk(
            space,
            if covers_every_row {
                None
            } else {
                Some(visible)
            },
        );
        if let Some(session) = tab.layer_walk.as_mut() {
            session.space = Some(space);
        }
        tab.scene.bump_geometry_no_blocks();
    }

    // ── Message handling ──────────────────────────────────────────────────

    pub(super) fn on_layer_walk(&mut self, m: LayerWalkMsg) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].layer_walk.is_none() {
            return Task::none();
        }
        match m {
            LayerWalkMsg::RowClick { name, ctrl, shift } => {
                {
                    let tab = &mut self.tabs[i];
                    if let Some(session) = tab.layer_walk.as_mut() {
                        layer_walk::click_select(&tab.layer_walk_rows, session, &name, ctrl, shift);
                        session.list_focused = true;
                    }
                }
                self.walk_apply();
            }
            LayerWalkMsg::RowDoubleClick { name, ctrl, shift } => {
                {
                    let tab = &mut self.tabs[i];
                    let Some(session) = tab.layer_walk.as_mut() else {
                        return Task::none();
                    };
                    // Ctrl / Shift + double-click works on the whole selection.
                    let mut names: Vec<String> = if ctrl || shift {
                        session.selected.clone()
                    } else {
                        vec![name]
                    };
                    if names.is_empty() {
                        return Task::none();
                    }
                    names.sort();
                    let all_held = names.iter().all(|n| session.held.iter().any(|h| h == n));
                    layer_walk::set_held(session, &names, !all_held);
                }
                self.walk_apply();
            }
            LayerWalkMsg::ToggleHold { name } => {
                {
                    let tab = &mut self.tabs[i];
                    let Some(session) = tab.layer_walk.as_mut() else {
                        return Task::none();
                    };
                    let held = session.held.iter().any(|h| h == &name);
                    layer_walk::set_held(session, std::slice::from_ref(&name), !held);
                }
                self.walk_apply();
            }
            LayerWalkMsg::RowHover { name, on } => {
                // View-only: the hover never changes what is on screen, so it
                // must not re-run the override. A leave only clears its own
                // row, so an enter/leave pair cannot drop the new hover.
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    if on {
                        session.hovered = Some(name);
                    } else if session.hovered.as_deref() == Some(name.as_str()) {
                        session.hovered = None;
                    }
                }
            }
            LayerWalkMsg::RowRelease { name } => {
                {
                    let tab = &mut self.tabs[i];
                    let index = tab.layer_walk_rows.iter().position(|r| r.name == name);
                    if let (Some(index), Some(session)) = (index, tab.layer_walk.as_mut()) {
                        layer_walk::drag_select(&tab.layer_walk_rows, session, index);
                    }
                }
                self.walk_apply();
            }
            LayerWalkMsg::KeyNav { up } => {
                {
                    let tab = &mut self.tabs[i];
                    if let Some(session) = tab.layer_walk.as_mut() {
                        layer_walk::key_navigate(&tab.layer_walk_rows, session, up);
                    }
                }
                self.walk_apply();
            }
            LayerWalkMsg::FilterEdit(text) => {
                // Typing is inert until Enter.
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.filter_buf = text;
                }
            }
            LayerWalkMsg::FilterApply => {
                let buf = self.tabs[i]
                    .layer_walk
                    .as_ref()
                    .map(|s| s.filter_buf.trim().to_string())
                    .unwrap_or_default();
                let saved = if buf.is_empty() {
                    None
                } else {
                    self.tabs[i]
                        .layer_walk_filters
                        .iter()
                        .find(|f| f.eq_ignore_ascii_case(&buf))
                        .cloned()
                };
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.pattern = if buf.is_empty() { None } else { Some(buf) };
                    session.saved_active = saved;
                    if session.pattern.is_none() {
                        session.show_only_matching = false;
                    }
                }
            }
            LayerWalkMsg::FilterClear => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.filter_buf.clear();
                    session.pattern = None;
                    session.saved_active = None;
                    session.show_only_matching = false;
                }
            }
            LayerWalkMsg::FilterOnlyMatch(on) => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.show_only_matching = on;
                }
            }
            LayerWalkMsg::SavedFilterPick(name) => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.filter_buf = name.clone();
                    session.pattern = Some(name.clone());
                    session.saved_active = Some(name);
                }
            }
            LayerWalkMsg::Sort(col) => {
                {
                    let tab = &mut self.tabs[i];
                    let Some(session) = tab.layer_walk.as_mut() else {
                        return Task::none();
                    };
                    if session.sort == col {
                        session.sort_asc = !session.sort_asc;
                    } else {
                        session.sort = col;
                        session.sort_asc = true;
                    }
                }
                let rows = self.walk_rows_now();
                self.tabs[i].layer_walk_rows = rows;
            }
            LayerWalkMsg::MenuItem(item) => return self.walk_menu_item(item),
            LayerWalkMsg::ToggleRestoreOnExit(on) => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.restore_on_exit = on;
                }
            }
            LayerWalkMsg::ToggleLayer0AlwaysOn(on) => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.layer0_always_on = on;
                }
                self.walk_apply();
            }
            LayerWalkMsg::SelectObjects => return self.walk_select_objects(),
            LayerWalkMsg::Purge => {
                let any = {
                    let tab = &self.tabs[i];
                    let Some(session) = tab.layer_walk.as_ref() else {
                        return Task::none();
                    };
                    let selected: FxHashSet<&str> =
                        session.selected.iter().map(String::as_str).collect();
                    tab.layer_walk_rows
                        .iter()
                        .any(|r| r.purged && selected.contains(r.name.as_str()))
                };
                if any {
                    if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                        session.purge_confirm = true;
                    }
                }
            }
            LayerWalkMsg::PurgeConfirm(yes) => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.purge_confirm = false;
                }
                if yes {
                    self.walk_purge();
                }
            }
            LayerWalkMsg::Inspect => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.inspect = true;
                }
            }
            LayerWalkMsg::InspectClose => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.inspect = false;
                }
            }
            LayerWalkMsg::KeepResult => return self.walk_keep_result(),
            LayerWalkMsg::Close => self.close_layer_walk(false),
        }
        Task::none()
    }

    fn walk_menu_item(&mut self, item: LayerWalkMenuItem) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].layer_walk.is_none() {
            return Task::none();
        }
        match item {
            LayerWalkMenuItem::HoldSelection | LayerWalkMenuItem::ReleaseSelection => {
                let on = item == LayerWalkMenuItem::HoldSelection;
                {
                    let tab = &mut self.tabs[i];
                    let Some(session) = tab.layer_walk.as_mut() else {
                        return Task::none();
                    };
                    let names = session.selected.clone();
                    layer_walk::set_held(session, &names, on);
                }
                self.walk_apply();
            }
            LayerWalkMenuItem::ReleaseAll => {
                {
                    let tab = &mut self.tabs[i];
                    let Some(session) = tab.layer_walk.as_mut() else {
                        return Task::none();
                    };
                    session.held.clear();
                }
                self.walk_apply();
            }
            LayerWalkMenuItem::SelectAll | LayerWalkMenuItem::Invert => {
                let invert = item == LayerWalkMenuItem::Invert;
                let names = {
                    let tab = &self.tabs[i];
                    let Some(session) = tab.layer_walk.as_ref() else {
                        return Task::none();
                    };
                    // Only the rows the list actually draws — a filter that
                    // hides a layer keeps it out of the range.
                    let selected: FxHashSet<&str> =
                        session.selected.iter().map(String::as_str).collect();
                    layer_walk::visible_rows(&tab.layer_walk_rows, session)
                        .into_iter()
                        .map(|(_, r)| r.name.clone())
                        // Select All takes every drawn row; Invert takes the
                        // drawn rows that are not in yet.
                        .filter(|n| !invert || !selected.contains(n.as_str()))
                        .collect()
                };
                {
                    let tab = &mut self.tabs[i];
                    let Some(session) = tab.layer_walk.as_mut() else {
                        return Task::none();
                    };
                    session.selected = names;
                    session.anchor = None;
                }
                self.walk_apply();
            }
            LayerWalkMenuItem::ClearAll => {
                {
                    let tab = &mut self.tabs[i];
                    let Some(session) = tab.layer_walk.as_mut() else {
                        return Task::none();
                    };
                    session.selected.clear();
                    session.anchor = None;
                }
                self.walk_apply();
            }
            LayerWalkMenuItem::SelectUnreferenced => {
                let names = {
                    let tab = &self.tabs[i];
                    tab.layer_walk_rows
                        .iter()
                        .filter(|r| r.unreferenced)
                        .map(|r| r.name.clone())
                        .collect()
                };
                {
                    let tab = &mut self.tabs[i];
                    let Some(session) = tab.layer_walk.as_mut() else {
                        return Task::none();
                    };
                    session.selected = names;
                    session.anchor = None;
                }
                self.walk_apply();
            }
            LayerWalkMenuItem::SaveState => self.walk_save_layer_state(),
            LayerWalkMenuItem::Inspect => {
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.inspect = true;
                }
            }
            LayerWalkMenuItem::CopyAsFilter => {
                let name = {
                    let tab = &self.tabs[i];
                    tab.layer_walk
                        .as_ref()
                        .and_then(|s| s.selected.first().cloned())
                };
                let Some(name) = name else {
                    return Task::none();
                };
                if let Some(session) = self.tabs[i].layer_walk.as_mut() {
                    session.filter_buf = name.clone();
                    session.pattern = Some(name);
                    session.saved_active = None;
                }
            }
            LayerWalkMenuItem::SaveFilter => return self.walk_save_filter(),
            LayerWalkMenuItem::DeleteFilter => {
                let name = self.tabs[i]
                    .layer_walk
                    .as_ref()
                    .and_then(|s| s.saved_active.clone());
                let Some(name) = name else {
                    return Task::none();
                };
                {
                    let tab = &mut self.tabs[i];
                    tab.layer_walk_filters
                        .retain(|f| !f.eq_ignore_ascii_case(&name));
                    tab.layer_walk_combo =
                        iced::widget::combo_box::State::new(tab.layer_walk_filters.clone());
                    if let Some(session) = tab.layer_walk.as_mut() {
                        session.saved_active = None;
                        if session.pattern.as_deref() == Some(name.as_str()) {
                            session.pattern = None;
                            session.filter_buf.clear();
                            session.show_only_matching = false;
                        }
                    }
                }
                self.command_line
                    .push_output(crate::tf!("LAYWALK: deleted filter \"{name}\".").as_ref());
            }
            LayerWalkMenuItem::ZoomExtents => return self.walk_zoom_extents(),
            LayerWalkMenuItem::SelectObjectsOnLayer => self.walk_select_objects_on_layer(),
        }
        Task::none()
    }

    // ── Actions ───────────────────────────────────────────────────────────

    fn walk_save_filter(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let name = self.tabs[i]
            .layer_walk
            .as_ref()
            .and_then(|s| s.pattern.clone())
            .unwrap_or_default();
        if name.is_empty() {
            return Task::none();
        }
        {
            let tab = &mut self.tabs[i];
            if !tab
                .layer_walk_filters
                .iter()
                .any(|f| f.eq_ignore_ascii_case(&name))
            {
                tab.layer_walk_filters.push(name.clone());
                tab.layer_walk_filters.sort();
            }
            tab.layer_walk_combo =
                iced::widget::combo_box::State::new(tab.layer_walk_filters.clone());
            if let Some(session) = tab.layer_walk.as_mut() {
                session.saved_active = Some(name.clone());
            }
        }
        self.command_line
            .push_output(crate::tf!("LAYWALK: saved filter \"{name}\".").as_ref());
        Task::none()
    }

    /// Keep Result: write the walked visibility into the layer table as one
    /// undo step, then drop the override.
    fn walk_keep_result(&mut self) -> Task<Message> {
        self.walk_commit();
        self.abandon_layer_walk();
        self.refresh_layer_panel();
        Task::none()
    }

    /// The commit path, mirroring LAYOFF / LAYON: layer 0 is never turned off,
    /// a layer the walk shows is turned on and thawed, and the whole thing is
    /// one `begin_layer_undo` / `commit_layer_undo` pair. Returns whether the
    /// table actually changed.
    fn walk_commit(&mut self) -> bool {
        let i = self.active_tab;
        let (visible, names) = {
            let tab = &self.tabs[i];
            let Some(session) = tab.layer_walk.as_ref() else {
                return false;
            };
            if !layer_walk::walk_changes_underlying(&tab.layer_walk_rows, session) {
                return false;
            }
            let visible = layer_walk::visible_set(session);
            // The walk's own set is committed as-is: a layer it shows is
            // turned on and thawed, a layer it hides is turned off. The only
            // protection lives in the pallet, in the layer-0 pin — an unpinned
            // row 0 is a row like any other.
            let names: Vec<String> = tab
                .layer_walk_rows
                .iter()
                .filter(|r| r.underlying_shown() != visible.contains(&r.name))
                .map(|r| r.name.clone())
                .collect();
            (visible, names)
        };
        if names.is_empty() {
            return false;
        }
        let undo = self.begin_layer_undo(i, "LAYWALK", &names);
        for name in &names {
            if let Some(l) = self.tabs[i].scene.document.layers.get_mut(name) {
                if visible.contains(name) {
                    l.turn_on();
                    l.thaw();
                } else {
                    l.turn_off();
                }
            }
        }
        self.tabs[i].scene.invalidate_layer_visibility(&names);
        self.tabs[i].dirty = true;
        self.commit_layer_undo(i, undo);
        true
    }

    /// PURGE's rules, already resolved per row: remove the selected rows that
    /// carry no reference and are not protected.
    fn walk_purge(&mut self) {
        let i = self.active_tab;
        let names: Vec<String> = {
            let tab = &self.tabs[i];
            let Some(session) = tab.layer_walk.as_ref() else {
                return;
            };
            let selected: FxHashSet<&str> = session.selected.iter().map(String::as_str).collect();
            tab.layer_walk_rows
                .iter()
                .filter(|r| r.purged && selected.contains(r.name.as_str()))
                .map(|r| r.name.clone())
                .collect()
        };
        if names.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, "LAYWALK PURGE");
        let mut removed = 0usize;
        for name in &names {
            if self.tabs[i].scene.document.layers.remove(name).is_some() {
                removed += 1;
            }
        }
        self.tabs[i].dirty = true;
        // The panel refresh rebuilds the walk rows and re-installs the override.
        self.refresh_layer_panel();
        self.command_line
            .push_output(crate::tr!("layer-walk", "purged", count = removed).as_ref());
    }

    /// The standard object-selection prompt, with the pallet out of the way
    /// while it runs. `handle_relaunch` clears `active_cmd` before dispatching
    /// `LAYWALK_SELECT`, so the pallet reappears on a pick and on a cancel.
    fn walk_select_objects(&mut self) -> Task<Message> {
        use crate::modules::draw::select::SelectObjectsCommand;
        let i = self.active_tab;
        self.tabs[i].layer_walk_picking = true;
        let cmd = SelectObjectsCommand::new("LAYWALK_SELECT");
        self.command_line.push_info(&cmd.prompt());
        self.tabs[i].active_cmd = Some(Box::new(cmd));
        self.focus_cmd_input()
    }

    /// The pick finished: select the picked objects' layers, replacing the
    /// current selection.
    pub(super) fn walk_picked_objects(&mut self) {
        let i = self.active_tab;
        self.tabs[i].layer_walk_picking = false;
        let layers: FxHashSet<String> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(_, e)| e.common().layer.clone())
            .filter(|l| !l.is_empty())
            .collect();
        let names = {
            let tab = &self.tabs[i];
            let mut names: Vec<String> = tab
                .layer_walk_rows
                .iter()
                .filter(|r| layers.contains(&r.name))
                .map(|r| r.name.clone())
                .collect();
            names.sort();
            names
        };
        if names.is_empty() {
            self.command_line
                .push_error(crate::tr!("layer-walk", "no-walked-layer").as_ref());
            return;
        }
        {
            let tab = &mut self.tabs[i];
            let Some(session) = tab.layer_walk.as_mut() else {
                return;
            };
            session.selected = names;
            session.anchor = None;
            session.drag_start = None;
            session.list_focused = true;
        }
        self.walk_apply();
    }

    /// Save Layer State...: the existing editor, pre-filled so the stored
    /// state records what the walk is showing. `capture_layer_state` builds
    /// *and* stores, which a pre-filled dialog must not do, so the draft is
    /// assembled here from the same fields.
    fn walk_save_layer_state(&mut self) {
        self.load_layer_state_editor(None);
        let i = self.active_tab;
        let draft = {
            let tab = &self.tabs[i];
            let Some(session) = tab.layer_walk.as_ref() else {
                return;
            };
            let visible = layer_walk::visible_handles(&tab.layer_walk_rows, session);
            let managed: FxHashSet<Handle> = tab.layer_walk_rows.iter().map(|r| r.handle).collect();
            let doc = &tab.scene.document;
            let layers = doc
                .layers
                .iter()
                .map(|l| codec::LayerStateLayer {
                    layer_handle: l.handle,
                    layer_name: l.name.clone(),
                    // The walk's decision replaces the table's for every layer
                    // it manages; anything else keeps what it already was.
                    off: if managed.contains(&l.handle) {
                        !visible.contains(&l.handle)
                    } else {
                        l.flags.off
                    },
                    frozen: l.flags.frozen,
                    locked: l.flags.locked,
                    plottable: l.is_plottable,
                    new_viewport_frozen: l.flags.frozen_in_new_viewport,
                    color: l.color,
                    color_name: l.color_name.clone(),
                    book_name: l.book_name.clone(),
                    line_type: l.line_type.clone(),
                    line_weight: l.line_weight,
                    plot_style: if l.plot_style.is_empty() {
                        format!("Color_{}", l.color.approximate_index())
                    } else {
                        l.plot_style.clone()
                    },
                    transparency: Some(l.transparency),
                })
                .collect::<Vec<_>>();
            codec::LayerState {
                name: self.layer_state_name_buf.clone(),
                description: String::new(),
                mask: codec::LayerStateMask::SUPPORTED,
                hidden: false,
                current_layer: doc.header.current_layer_name.clone(),
                layers,
            }
        };
        self.layer_state_edit_draft = Some(draft);
        self.layer_state_edit_filter.clear();
        self.layer_state_edit_color_open = None;
        self.active_modal = Some(super::ModalKind::LayerStateEditor);
    }

    /// The entities of the space being walked that sit on a selected layer.
    /// Block-definition geometry never joins this: the walk's zoom and select
    /// work on the drawing, not on definitions.
    fn walk_selected_layer_entities(&self) -> Vec<Handle> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.layer_walk.as_ref() else {
            return Vec::new();
        };
        let selected: FxHashSet<&str> = session.selected.iter().map(String::as_str).collect();
        tab.scene
            .interaction_space_handles()
            .iter()
            .filter_map(|h| tab.scene.document.get_entity(*h))
            .filter(|e| selected.contains(e.common().layer.as_str()))
            .map(|e| e.common().handle)
            .collect()
    }

    fn walk_zoom_extents(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.walk_selected_layer_entities();
        if handles.is_empty() {
            self.command_line
                .push_error(crate::tr!("layer-walk", "no-objects").as_ref());
            return Task::none();
        }
        self.tabs[i].scene.remember_current_view();
        if !self.tabs[i].scene.zoom_to_entities(&handles) {
            self.command_line
                .push_error(crate::tr!("layer-walk", "nothing-to-zoom").as_ref());
        }
        Task::none()
    }

    fn walk_select_objects_on_layer(&mut self) {
        let i = self.active_tab;
        let handles = self.walk_selected_layer_entities();
        let n = handles.len();
        self.tabs[i].scene.replace_selection_exact(&handles);
        self.tabs[i].scene.bump_selection();
        self.command_line
            .push_output(crate::tr!("layer-walk", "selected-objects", count = n).as_ref());
    }

    /// A command that rewrites the layer table, or the whole document, ends
    /// the walk instead of letting it silently hide the result — restoring is
    /// the honest default, and those commands own visibility from now on.
    pub(super) fn layer_walk_conflicts_with(cmd: &str) -> bool {
        const CONFLICTS: &[&str] = &[
            "UNDO",
            "REDO",
            "MUNDO",
            "NEW",
            "OPEN",
            "SAVEAS",
            "WQ",
            "LAYOFF",
            "LAYON",
            "LAYFRZ",
            "LAYTHW",
            "LAYISO",
            "LAYUNISO",
            "LAYMRG",
            "LAYDEL",
            "LAYER",
            "PURGE",
            "LAYERSTATE",
            "VPLAYER",
            "ERASE",
        ];
        let head = cmd.split_whitespace().next().unwrap_or("").to_uppercase();
        CONFLICTS.iter().any(|c| head == *c)
    }
}
