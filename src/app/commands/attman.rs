//! Block Attribute Manager (BATTMAN / ATTMAN): loading an attributed block's
//! definitions into the dialog and writing the edited list back to the
//! block definition and, when asked, to its existing references.

use document::entities::AttributeDefinition;
use document::{EntityType, Handle};
use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::modules::insert::block_cmdline::BlockTable;
use crate::scene::view::dispatch::{apply_common_prop, apply_geom_prop};
use crate::ui::window::attman_dialog::{
    self as dialog, AttManBlock, AttManMsg, AttManRow, AttManState, EditTab,
};

impl OpenCADStudio {
    /// Open the dialog on the block of the first preselected attributed
    /// reference, else on the first attributed block.
    pub(in crate::app) fn open_attman(
        &mut self,
        i: usize,
        preselected: &[Handle],
    ) -> Task<Message> {
        let document = &self.tabs[i].scene.document;
        let table = BlockTable::of(document);
        let model = document
            .block_records
            .get("*Model_Space")
            .map_or(Handle::NULL, |br| br.handle);
        let blocks: Vec<AttManBlock> = table
            .attributed
            .iter()
            .map(|name| {
                let mut found = 0;
                let mut in_space = 0;
                for entity in document.entities() {
                    if let EntityType::Insert(insert) = entity {
                        if insert.block_name.eq_ignore_ascii_case(name) {
                            found += 1;
                            if insert.common.owner_handle == model {
                                in_space += 1;
                            }
                        }
                    }
                }
                AttManBlock {
                    name: name.clone(),
                    found,
                    in_space,
                }
            })
            .collect();
        let start = preselected
            .iter()
            .find_map(|h| match document.get_entity(*h) {
                Some(EntityType::Insert(insert)) => blocks
                    .iter()
                    .position(|b| b.name.eq_ignore_ascii_case(&insert.block_name)),
                _ => None,
            })
            .unwrap_or(0);
        let mut styles: Vec<String> = document
            .text_styles
            .iter()
            .filter(|s| !s.name.is_empty())
            .map(|s| s.name.clone())
            .collect();
        styles.sort_by_key(|s| s.to_lowercase());
        let layers: Vec<String> = document.layers.iter().map(|l| l.name.clone()).collect();
        let linetypes: Vec<String> = document
            .line_types
            .iter()
            .map(|lt| lt.name.clone())
            .filter(|n| !n.eq_ignore_ascii_case("ByLayer") && !n.eq_ignore_ascii_case("ByBlock"))
            .collect();
        self.attman = Some(AttManState {
            blocks,
            block: start,
            rows: Vec::new(),
            removed: Vec::new(),
            selected: None,
            tab: EditTab::Attribute,
            edit_open: true,
            drafts: Default::default(),
            dirty: false,
            live_steps: 0,
            more_columns: false,
            styles,
            layers,
            linetypes,
            error: None,
        });
        self.attman_load(i);
        self.tabs[i].active_cmd = None;
        self.active_modal = Some(crate::app::ModalKind::AttMan);
        Task::none()
    }

    /// Read the shown block's attribute definitions, in block order.
    fn attman_load(&mut self, i: usize) {
        let document = &self.tabs[i].scene.document;
        let Some(state) = self.attman.as_mut() else {
            return;
        };
        let Some(name) = state.current_block().map(|b| b.name.clone()) else {
            return;
        };
        state.rows = document
            .block_records
            .get(&name)
            .map(|br| {
                br.entity_handles
                    .iter()
                    .filter_map(|h| match document.get_entity(*h) {
                        Some(EntityType::AttributeDefinition(def)) => Some(AttManRow {
                            handle: *h,
                            original_tag: def.tag.clone(),
                            live_tag: def.tag.clone(),
                            def: def.clone(),
                        }),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        state.removed.clear();
        state.dirty = false;
        state.selected = (!state.rows.is_empty()).then_some(0);
        state.load_drafts();
    }

    pub(in crate::app) fn on_attman(&mut self, m: AttManMsg) -> Task<Message> {
        let i = self.active_tab;
        let Some(state) = self.attman.as_mut() else {
            return Task::none();
        };
        // Edits of the selected row.
        let mut edited = true;
        let at = state.selected;
        match m {
            AttManMsg::Block(index) => {
                if index == state.block || index >= state.blocks.len() {
                    return Task::none();
                }
                // Pending changes of the shown block are kept.
                if state.dirty && !self.attman_commit(i, true, false) {
                    return Task::none();
                }
                if let Some(state) = self.attman.as_mut() {
                    state.block = index;
                }
                self.attman_load(i);
                return Task::none();
            }
            AttManMsg::PickBlock => {
                self.active_modal = None;
                let command = crate::modules::insert::attman::AttmanPickCommand;
                use crate::command::CadCommand as _;
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                return Task::none();
            }
            AttManMsg::Select(index) => {
                if index < state.rows.len() {
                    state.selected = Some(index);
                    state.load_drafts();
                }
                return Task::none();
            }
            AttManMsg::MoveUp | AttManMsg::MoveDown => {
                let Some(at) = at else {
                    return Task::none();
                };
                let to = if matches!(m, AttManMsg::MoveUp) {
                    at.checked_sub(1)
                } else {
                    Some(at + 1).filter(|t| *t < state.rows.len())
                };
                let Some(to) = to else {
                    return Task::none();
                };
                state.rows.swap(at, to);
                state.selected = Some(to);
            }
            AttManMsg::Remove => {
                let Some(at) = at else {
                    return Task::none();
                };
                if state.rows.len() < 2 {
                    return Task::none();
                }
                let row = state.rows.remove(at);
                state.removed.push(row.handle);
                state.selected = Some(at.min(state.rows.len() - 1));
                state.load_drafts();
            }
            AttManMsg::ToggleEdit => {
                state.edit_open = !state.edit_open;
                return Task::none();
            }
            AttManMsg::Tab(tab) => {
                state.tab = tab;
                return Task::none();
            }
            AttManMsg::MoreColumns => {
                state.more_columns = !state.more_columns;
                return Task::none();
            }
            AttManMsg::Column(column, on) => {
                let bit = 1u32 << (column as u32);
                let mut prefs = dialog::prefs();
                prefs.columns = if on {
                    prefs.columns | bit
                } else {
                    prefs.columns & !bit
                };
                return Task::none();
            }
            AttManMsg::ApplyExisting(on) => {
                dialog::prefs().apply_existing = on;
                return Task::none();
            }
            AttManMsg::Emphasize(on) => {
                dialog::prefs().emphasize_duplicates = on;
                return Task::none();
            }
            AttManMsg::Live(on) => {
                dialog::prefs().live = on;
                if on && state.dirty {
                    self.attman_commit(i, false, false);
                }
                return Task::none();
            }
            AttManMsg::DismissError => {
                state.error = None;
                return Task::none();
            }
            AttManMsg::Help => {
                self.command_line.push_info(
                    crate::t!("BATTMAN manages the attribute definitions of a block.").as_ref(),
                );
                return Task::none();
            }
            AttManMsg::Sync => {
                self.attman_commit(i, true, true);
                return Task::none();
            }
            AttManMsg::Apply => {
                self.attman_commit(i, true, false);
                return Task::none();
            }
            AttManMsg::Ok => {
                if self.attman_commit(i, true, false) {
                    self.attman = None;
                    self.close_active_modal();
                }
                return Task::none();
            }
            AttManMsg::Cancel => {
                self.attman_withdraw_live();
                self.attman = None;
                self.close_active_modal();
                return Task::none();
            }
            _ => {
                edited = false;
            }
        }
        if edited {
            state.dirty = true;
            return self.attman_after_edit(i);
        }
        // Field edits of the selected row.
        let Some(row) = at.and_then(|at| state.rows.get_mut(at)) else {
            return Task::none();
        };
        let def = &mut row.def;
        let yes = |on: bool| if on { "Yes" } else { "No" };
        match m {
            AttManMsg::Tag(value) => {
                state.drafts.tag = value.clone();
                if crate::ui::window::attdef_dialog::tag_error(&value).is_none() {
                    geom(def, "att_tag", &value);
                }
            }
            AttManMsg::Prompt(value) => geom(def, "att_prompt", &value),
            AttManMsg::Default(value) => geom(def, "att_default", &value),
            AttManMsg::Mode(key, on) => {
                let field = match key {
                    "invisible" => "att_invisible",
                    "verify" => "att_verify",
                    "preset" => "att_preset",
                    "multiline" => "att_multiline",
                    _ => return Task::none(),
                };
                geom(def, field, yes(on));
            }
            AttManMsg::Style(value) => geom(def, "att_style", &value),
            AttManMsg::Justify(justify) => geom(def, "att_justify", justify.label()),
            AttManMsg::Height(value) => {
                state.drafts.height = value.clone();
                geom(def, "att_h", &value);
            }
            AttManMsg::Rotation(value) => {
                state.drafts.rotation = value.clone();
                geom(def, "att_rot", &value);
            }
            AttManMsg::Width(value) => {
                state.drafts.width = value.clone();
                geom(def, "att_wf", &value);
            }
            AttManMsg::Oblique(value) => {
                state.drafts.oblique = value.clone();
                geom(def, "att_ob", &value);
            }
            AttManMsg::Boundary(value) => {
                state.drafts.boundary = value.clone();
                geom(def, "att_bwidth", &value);
            }
            AttManMsg::Annotative(on) => geom(def, "att_annotative", yes(on)),
            AttManMsg::Backwards(on) => geom(def, "att_backward", yes(on)),
            AttManMsg::UpsideDown(on) => geom(def, "att_upside_down", yes(on)),
            AttManMsg::Layer(value) => common(def, "layer", &value),
            AttManMsg::Linetype(value) => common(def, "linetype", &value),
            AttManMsg::Color(value) => {
                if let Some(color) = dialog::parse_color(&value) {
                    def.common.color = color;
                }
            }
            AttManMsg::Lineweight(value) => {
                if let Some(lw) = dialog::parse_lineweight(&value) {
                    def.common.line_weight = lw;
                }
            }
            _ => return Task::none(),
        }
        state.dirty = true;
        self.attman_after_edit(i)
    }

    fn attman_after_edit(&mut self, i: usize) -> Task<Message> {
        if dialog::prefs().live {
            self.attman_commit(i, false, false);
        }
        Task::none()
    }

    /// The pick of `_ATTMAN_PICK <hex handle>`: show the picked reference's
    /// block. A reference of a block without attributes is reported.
    pub(in crate::app) fn attman_picked(&mut self, i: usize, handle: &str) -> Task<Message> {
        self.active_modal = Some(crate::app::ModalKind::AttMan);
        let Ok(value) = u64::from_str_radix(handle, 16) else {
            return Task::none();
        };
        let name = match self.tabs[i].scene.document.get_entity(Handle::new(value)) {
            Some(EntityType::Insert(insert)) => insert.block_name.clone(),
            _ => return Task::none(),
        };
        let index = self.attman.as_ref().and_then(|state| {
            state
                .blocks
                .iter()
                .position(|b| b.name.eq_ignore_ascii_case(&name))
        });
        match index {
            Some(index) => self.on_attman(AttManMsg::Block(index)),
            None => {
                if let Some(state) = self.attman.as_mut() {
                    state.error = Some(
                        crate::tf!("Block table contains no attributed block named {}.", name)
                            .into_owned(),
                    );
                }
                Task::none()
            }
        }
    }

    /// Take back the changes written live since the last Apply, quietly.
    fn attman_withdraw_live(&mut self) {
        let Some(state) = self.attman.as_mut() else {
            return;
        };
        let steps = std::mem::take(&mut state.live_steps);
        for row in &mut state.rows {
            row.live_tag = row.original_tag.clone();
        }
        if steps > 0 {
            self.undo_steps_reported(steps, false);
        }
    }

    /// Write the working copy to the drawing. `finish` (Apply / OK / Sync)
    /// leaves exactly one undo step: the live changes are taken back and the
    /// whole list is written again in one step. Otherwise (a live edit) the
    /// write joins the open live step. `sync` rebuilds the references'
    /// attributes even when "apply to existing references" is off. Returns
    /// false when a field is invalid.
    fn attman_commit(&mut self, i: usize, finish: bool, sync: bool) -> bool {
        let Some(state) = self.attman.as_mut() else {
            return false;
        };
        if finish {
            if let Some(error) = state
                .rows
                .iter()
                .find_map(|row| crate::ui::window::attdef_dialog::tag_error(&row.def.tag))
                .or_else(|| {
                    crate::ui::window::attdef_dialog::tag_error(&state.drafts.tag)
                        .filter(|_| state.selected.is_some())
                })
            {
                state.error = Some(crate::t!(error).into_owned());
                return false;
            }
            state.error = None;
            self.attman_withdraw_live();
        }
        let Some(state) = self.attman.as_mut() else {
            return false;
        };
        let Some(name) = state.current_block().map(|b| b.name.clone()) else {
            return false;
        };
        let rows: Vec<(Handle, String, AttributeDefinition)> = state
            .rows
            .iter()
            .map(|r| (r.handle, r.live_tag.clone(), r.def.clone()))
            .collect();
        let removed = state.removed.clone();
        let write = state.dirty || sync;
        let synced = sync || dialog::prefs().apply_existing;
        // A live step something else closed starts a new one.
        let open = !finish && state.live_steps > 0 && self.tabs[i].history.pending.is_some();
        if write {
            if !open {
                self.push_undo_snapshot(i, "BATTMAN");
            }
            self.attman_write(i, &name, &rows, &removed, synced);
        }
        if finish && write {
            self.finish_pending_history(i);
        }
        let Some(state) = self.attman.as_mut() else {
            return false;
        };
        if finish {
            state.dirty = false;
            state.removed.clear();
            for row in &mut state.rows {
                row.original_tag = row.def.tag.clone();
                row.live_tag = row.def.tag.clone();
            }
        } else if write {
            if !open {
                state.live_steps += 1;
            }
            // The references now hold the written tags.
            if synced {
                for row in &mut state.rows {
                    row.live_tag = row.def.tag.clone();
                }
            }
        }
        true
    }

    /// The definition list of `block` becomes `rows` (in that order) less
    /// `removed`; with `sync`, every reference is rebuilt from it, keeping
    /// each attribute's value through a tag rename.
    fn attman_write(
        &mut self,
        i: usize,
        block: &str,
        rows: &[(Handle, String, AttributeDefinition)],
        removed: &[Handle],
        sync: bool,
    ) {
        let document = &mut self.tabs[i].scene.document;
        for handle in removed {
            if document.get_entity(*handle).is_some() {
                document.remove_entity(*handle);
            }
        }
        // Each definition keeps its object; the definitions take the slots
        // the block's definitions held, in the new order.
        for (handle, _, def) in rows {
            if let Some(EntityType::AttributeDefinition(slot)) = document.get_entity_mut(*handle) {
                let mut def = def.clone();
                def.common.handle = *handle;
                def.common.owner_handle = slot.common.owner_handle;
                *slot = def;
            }
        }
        if let Some(br) = document.block_records.get_mut(block) {
            br.entity_handles.retain(|h| !removed.contains(h));
            let mut order = rows.iter().map(|(handle, _, _)| *handle);
            for slot in br.entity_handles.iter_mut() {
                if rows.iter().any(|(handle, _, _)| handle == slot) {
                    if let Some(next) = order.next() {
                        *slot = next;
                    }
                }
            }
        }
        // Annotative definitions carry the scale context data.
        for (handle, _, def) in rows {
            crate::scene::annotative::set_entity_annotative(
                &mut self.tabs[i].scene.document,
                *handle,
                def.flags.annotative,
            );
        }
        if sync {
            // A renamed definition keeps the value each reference held.
            let renames: Vec<(String, String)> = rows
                .iter()
                .filter(|(_, old, def)| !old.eq_ignore_ascii_case(&def.tag))
                .map(|(_, old, def)| (old.clone(), def.tag.clone()))
                .collect();
            if !renames.is_empty() {
                let inserts: Vec<Handle> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter_map(|e| match e {
                        EntityType::Insert(ins) if ins.block_name.eq_ignore_ascii_case(block) => {
                            Some(ins.common.handle)
                        }
                        _ => None,
                    })
                    .collect();
                for handle in inserts {
                    if let Some(EntityType::Insert(ins)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        for attribute in &mut ins.attributes {
                            if let Some((_, new)) = renames
                                .iter()
                                .find(|(old, _)| old.eq_ignore_ascii_case(&attribute.tag))
                            {
                                attribute.tag = new.clone();
                            }
                        }
                    }
                }
            }
            self.attsync_block_with(i, block, false);
        }
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
    }
}

fn geom(def: &mut AttributeDefinition, field: &str, value: &str) {
    let mut entity = EntityType::AttributeDefinition(def.clone());
    apply_geom_prop(&mut entity, field, value);
    if let EntityType::AttributeDefinition(updated) = entity {
        *def = updated;
    }
}

fn common(def: &mut AttributeDefinition, field: &str, value: &str) {
    let mut entity = EntityType::AttributeDefinition(def.clone());
    apply_common_prop(&mut entity, field, value);
    if let EntityType::AttributeDefinition(updated) = entity {
        *def = updated;
    }
}
