//! Find and Replace: which texts a search reads, in which order, and what
//! Find, Replace and Replace All do with them. The window is
//! `ui::window::find_replace`.

use super::{FindMatchKey, FindPart, FindResult, Message, OpenCADStudio};
use crate::scene::{FindZoom, Scene};
use crate::ui::window::find_replace::{
    FindAnswer, FindKind, FindMsg, FindNotice, FindOptions, FindPrompt, FindScope, LockChoice,
    ReplaceRun,
};
use document::{EntityType, Handle};
use iced::Task;

/// How many earlier texts each history keeps.
const HISTORY: usize = 20;

impl OpenCADStudio {
    /// Open the window. The texts, history and options stay from the last
    /// time; where to look starts again from the whole drawing.
    pub(super) fn open_find_replace(&mut self) -> Task<Message> {
        let state = &mut self.find_replace;
        state.scope = FindScope::Drawing;
        state.selection.clear();
        self.reset_find_search();
        self.find_replace.undo_pushed = false;
        self.active_modal = Some(super::ModalKind::FindReplace);
        self.modal_offset = iced::Vector::ZERO;
        self.modal_resize = iced::Vector::ZERO;
        iced::widget::operation::focus(iced::widget::Id::new(
            crate::ui::window::find_replace::FIND_INPUT_ID,
        ))
    }

    /// The ribbon's Find text box: the window opens on the current space and
    /// searches at once.
    pub(super) fn find_text_from_ribbon(&mut self, search: String) -> Task<Message> {
        self.find_replace.search = search;
        let task = self.open_find_replace();
        self.find_replace.scope = FindScope::Space;
        let found = self.find_replace_find();
        Task::batch([task, found])
    }

    fn reset_find_search(&mut self) {
        if self.find_replace.current_match.is_some() {
            self.clear_find_highlight();
        }
        let state = &mut self.find_replace;
        state.current_match = None;
        state.matches.clear();
        state.results.clear();
        state.highlighted.clear();
        state.anchor = None;
        state.changed.clear();
        state.pending_entities.clear();
        state.pending_blocks.clear();
        state.pending_unlocked_layers.clear();
        state.pending_xref_reload = false;
        state.notice = None;
        state.run = None;
    }

    pub(super) fn on_find_message(&mut self, message: FindMsg) -> Task<Message> {
        match message {
            FindMsg::Search(value) => {
                self.find_replace.search = value;
                self.reset_find_search();
            }
            FindMsg::Replacement(value) => self.find_replace.replacement = value,
            FindMsg::Scope(scope) => {
                self.find_replace.scope = scope;
                self.reset_find_search();
            }
            FindMsg::SelectObjects => {
                use crate::command::CadCommand;
                use crate::modules::draw::select::SelectObjectsCommand;
                self.active_modal = None;
                let i = self.active_tab;
                let cmd = SelectObjectsCommand::plain("FIND", "FIND_OBJECTS_GATHERED");
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }
            FindMsg::ListResults(on) => {
                self.find_replace.list_results = on;
                self.reset_find_search();
            }
            FindMsg::MoreOptions(on) => self.find_replace.more_options = on,
            FindMsg::Option(opt, on) => {
                *opt.slot(&mut self.find_replace.options) = on;
                self.reset_find_search();
            }
            FindMsg::Find => return self.find_replace_find(),
            FindMsg::Replace => return self.find_replace_one(),
            FindMsg::ReplaceAll => return self.find_replace_all(),
            FindMsg::Done => {
                self.clear_find_highlight();
                self.active_modal = None;
                self.command_line
                    .push_output(crate::tr!("find", "regenerating").as_str());
            }
            FindMsg::Help => self
                .command_line
                .push_info(crate::tr!("find", "help").as_str()),
            FindMsg::Row(index, keys) => {
                let (ctrl, shift) = keys.unwrap_or((self.ctrl_down, self.shift_down));
                let Some(result) = self.find_replace.results.get(index).cloned() else {
                    return Task::none();
                };
                let state = &mut self.find_replace;
                if shift {
                    let from = state.anchor.unwrap_or(index);
                    let (a, b) = (from.min(index), from.max(index));
                    state.highlighted = state.results[a..=b].iter().map(|r| r.key).collect();
                    state.current_match = Some(result.key);
                    return Task::none();
                }
                state.anchor = Some(index);
                if ctrl {
                    if let Some(at) = state.highlighted.iter().position(|k| *k == result.key) {
                        state.highlighted.remove(at);
                        state.current_match = state.highlighted.last().copied();
                    } else {
                        state.highlighted.push(result.key);
                        state.current_match = Some(result.key);
                    }
                    return Task::none();
                }
                state.highlighted = vec![result.key];
                state.current_match = Some(result.key);
                return self.visit_find_result(&result);
            }
            FindMsg::ZoomToResult => {
                // One highlighted row only, as the button allows.
                let current = match self.find_replace.highlighted.as_slice() {
                    [one] => Some(*one),
                    _ => return Task::none(),
                };
                if let Some(result) = self
                    .find_replace
                    .results
                    .iter()
                    .find(|r| Some(r.key) == current)
                    .cloned()
                {
                    return self.visit_find_result(&result);
                }
            }
            FindMsg::SelectHighlighted => {
                let picked: Vec<FindResult> = self
                    .find_replace
                    .results
                    .iter()
                    .filter(|r| self.find_replace.highlighted.contains(&r.key))
                    .cloned()
                    .collect();
                return self.find_select_results(picked);
            }
            FindMsg::SelectAll => {
                let all = self.find_replace.results.clone();
                return self.find_select_results(all);
            }
            FindMsg::Dismiss => self.find_replace.notice = None,
            FindMsg::Answer(answer) => return self.find_answer(answer),
        }
        Task::none()
    }

    /// Back from "Select objects": search the picked objects.
    pub(super) fn find_objects_gathered(&mut self, i: usize) {
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        self.tabs[i].active_cmd = None;
        self.find_replace.selection = handles;
        self.find_replace.scope = FindScope::Selection;
        self.reset_find_search();
        self.active_modal = Some(super::ModalKind::FindReplace);
    }

    fn remember_find_texts(&mut self) {
        let state = &mut self.find_replace;
        for (value, history) in [
            (state.search.clone(), &mut state.search_history),
            (state.replacement.clone(), &mut state.replace_history),
        ] {
            if value.is_empty() {
                continue;
            }
            history.retain(|h| *h != value);
            history.insert(0, value);
            history.truncate(HISTORY);
        }
    }

    /// Find: with List results every match at once, else the next match;
    /// past the last one the search reports it has finished.
    pub(super) fn find_replace_find(&mut self) -> Task<Message> {
        if self.find_replace.search.trim().is_empty() {
            return Task::none();
        }
        self.remember_find_texts();
        let i = self.active_tab;
        if self.find_replace.list_results {
            let matches = self.collect_find_matches(i);
            self.find_replace.matches = matches.clone();
            self.find_replace.results = matches;
            self.find_replace.notice = Some(FindNotice::Finished {
                found: self.find_replace.results.len(),
                changed: self.find_replace.changed.len(),
            });
            let first = self.find_replace.results.first().cloned();
            self.find_replace.current_match = first.as_ref().map(|r| r.key);
            self.find_replace.highlighted = first.iter().map(|r| r.key).collect();
            self.find_replace.anchor = first.is_some().then_some(0);
            return first.map_or_else(Task::none, |r| self.visit_find_result(&r));
        }
        let next = match self.find_replace.current_match {
            None => {
                self.find_replace.matches = self.collect_find_matches(i);
                0
            }
            Some(current) => self
                .find_replace
                .matches
                .iter()
                .position(|r| r.key == current)
                .map_or(0, |p| p + 1),
        };
        self.show_find_match(next)
    }

    /// Show match `index` of the running search, or end the search.
    fn show_find_match(&mut self, index: usize) -> Task<Message> {
        match self.find_replace.matches.get(index).cloned() {
            Some(result) => {
                self.find_replace.current_match = Some(result.key);
                self.visit_find_result(&result)
            }
            None => {
                self.find_replace.notice = Some(FindNotice::Finished {
                    found: self.find_replace.matches.len(),
                    changed: self.find_replace.changed.len(),
                });
                self.find_replace.current_match = None;
                self.clear_find_highlight();
                Task::none()
            }
        }
    }

    /// Bring a match into view (switching to its layout) and highlight its
    /// object.
    fn visit_find_result(&mut self, result: &FindResult) -> Task<Message> {
        let i = self.active_tab;
        let mut task = Task::none();
        if !result.location.eq_ignore_ascii_case(&self.tabs[i].scene.current_layout) {
            task = self.on_layout_switch(result.location.clone());
        }
        let zoom = if result.height > 0.0 {
            FindZoom::TextHeight(result.height)
        } else {
            FindZoom::Object
        };
        let key = result.key;
        let scene = &mut self.tabs[i].scene;
        match key.part {
            FindPart::Attribute(index) => {
                scene.focus_camera_on_insert_attribute(key.owner, index, zoom);
            }
            _ if key.owner == key.target => {
                scene.focus_camera_on_entity(key.target, zoom);
            }
            _ => {
                scene.focus_camera_on_block_entity(key.owner, key.target, zoom);
            }
        }
        scene.deselect_all();
        scene.select_entity(key.owner, false);
        let quads = match_quads(
            &scene.document,
            key,
            &self.find_replace.search,
            &self.find_replace.options.matching(),
        );
        let (frame, shade) = highlight(&quads);
        scene.set_preview_wires(frame);
        scene.set_command_preview_hatches(shade);
        self.refresh_properties();
        task
    }

    /// Drop the drawing's highlight of the match on show.
    pub(in crate::app) fn clear_find_highlight(&mut self) {
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_none() {
            self.tabs[i].scene.clear_preview_wire();
        }
    }

    /// Create Selection Set: the listed objects become the selection, which
    /// must lie in one space.
    fn find_select_results(&mut self, picked: Vec<FindResult>) -> Task<Message> {
        if picked.is_empty() {
            return Task::none();
        }
        let first = picked[0].location.clone();
        if picked.iter().any(|r| !r.location.eq_ignore_ascii_case(&first)) {
            self.find_replace.notice = Some(FindNotice::CannotSelect);
            return Task::none();
        }
        let mut owners: Vec<Handle> = Vec::new();
        for r in &picked {
            if !owners.contains(&r.key.owner) {
                owners.push(r.key.owner);
            }
        }
        let i = self.active_tab;
        let mut task = Task::none();
        if !first.eq_ignore_ascii_case(&self.tabs[i].scene.current_layout) {
            task = self.on_layout_switch(first);
        }
        self.clear_find_highlight();
        self.active_modal = None;
        self.command_line
            .push_output(crate::tr!("find", "regenerating").as_str());
        let scene = &mut self.tabs[i].scene;
        scene.deselect_all();
        scene.select_entities(&owners);
        self.command_line.push_output(
            crate::i18n::plain(crate::tr!(
                "find",
                "selected",
                count = owners.len().to_string(),
                text = self.find_replace.search.clone()
            ))
            .as_str(),
        );
        self.refresh_properties();
        task
    }

    // ── Replacing ─────────────────────────────────────────────────────────

    /// Replace the match on show, then show the next one.
    pub(super) fn find_replace_one(&mut self) -> Task<Message> {
        let Some(current) = self.find_replace.current_match else {
            return Task::none();
        };
        let Some(result) = self
            .find_replace
            .matches
            .iter()
            .chain(self.find_replace.results.iter())
            .find(|r| r.key == current)
            .cloned()
        else {
            return Task::none();
        };
        self.remember_find_texts();
        self.find_replace.run = Some(new_run(vec![result], true));
        self.continue_replace_run()
    }

    /// Replace every match.
    pub(super) fn find_replace_all(&mut self) -> Task<Message> {
        if self.find_replace.search.trim().is_empty() {
            return Task::none();
        }
        self.remember_find_texts();
        let i = self.active_tab;
        let matches = self.collect_find_matches(i);
        self.find_replace.matches = matches.clone();
        self.find_replace.run = Some(new_run(matches, false));
        self.continue_replace_run()
    }

    fn find_answer(&mut self, answer: FindAnswer) -> Task<Message> {
        let Some(run) = self.find_replace.run.as_mut() else {
            return Task::none();
        };
        let prompt = run.prompt.take();
        match (answer, prompt) {
            (FindAnswer::Cancel, _) | (_, None) => {
                self.find_replace.run = None;
                self.after_find_changes();
                return Task::none();
            }
            (FindAnswer::KeepLocked, _) => run.locked = Some(LockChoice::Keep),
            (FindAnswer::Unlock, _) => run.locked = Some(LockChoice::Unlock),
            (FindAnswer::SkipLocked, _) => run.locked = Some(LockChoice::Skip),
            (FindAnswer::AllBlocks, _) => run.all_blocks = true,
            (FindAnswer::ThisBlock, Some(FindPrompt::Block(name))) => run.blocks.push((name, true)),
            (FindAnswer::SkipBlock, Some(FindPrompt::Block(name))) => {
                run.blocks.push((name, false))
            }
            (FindAnswer::AllXrefs, _) => run.all_xrefs = true,
            (FindAnswer::ThisXref, Some(FindPrompt::Xref(name))) => run.xrefs.push((name, true)),
            (FindAnswer::SkipXref, Some(FindPrompt::Xref(name))) => run.xrefs.push((name, false)),
            _ => {}
        }
        self.continue_replace_run()
    }

    /// Go on with the replacement until it needs an answer or is done.
    fn continue_replace_run(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(mut run) = self.find_replace.run.take() else {
            return Task::none();
        };
        let pattern = self.find_replace.search.clone();
        let with = self.find_replace.replacement.clone();
        let opts = self.find_replace.options.matching();
        // Each external reference's file is written once, after the run.
        let mut xref_writes: Vec<String> = Vec::new();
        while run.next < run.matches.len() {
            let result = run.matches[run.next].clone();
            let key = result.key;
            let doc = &self.tabs[i].scene.document;
            // Locked layers.
            let layer = doc.get_entity(key.target).map(|e| e.common().layer.clone());
            let locked = [key.target, key.owner]
                .iter()
                .any(|h| self.tabs[i].scene.is_layer_locked(*h));
            if locked {
                match run.locked {
                    None => {
                        run.prompt = Some(FindPrompt::Locked);
                        self.find_replace.run = Some(run);
                        return Task::none();
                    }
                    Some(LockChoice::Skip) => {
                        run.next += 1;
                        continue;
                    }
                    Some(LockChoice::Unlock) => {
                        self.ensure_find_undo(i);
                        let layer_names: Vec<String> = {
                            let doc = &self.tabs[i].scene.document;
                            layer
                                .iter()
                                .chain(
                                    doc.get_entity(key.owner)
                                        .map(|e| &e.common().layer),
                                )
                                .cloned()
                                .collect()
                        };
                        let doc = &mut self.tabs[i].scene.document;
                        for name in &layer_names {
                            if let Some(l) = doc.layers.get_mut(name) {
                                if l.flags.locked {
                                    l.flags.locked = false;
                                    if !self
                                        .find_replace
                                        .pending_unlocked_layers
                                        .iter()
                                        .any(|n| n.eq_ignore_ascii_case(name))
                                    {
                                        self.find_replace
                                            .pending_unlocked_layers
                                            .push(name.clone());
                                    }
                                }
                            }
                        }
                    }
                    Some(LockChoice::Keep) => {}
                }
            }
            // Texts in an external reference or a block definition.
            let definition = block_name_of(&self.tabs[i].scene, key);
            if let Some(name) = definition.clone() {
                let (all, answers, prompt) = if result.kind == FindKind::Xref {
                    let path = self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .get(&name)
                        .map(|r| r.xref_path.clone())
                        .unwrap_or_default();
                    (run.all_xrefs, &run.xrefs, FindPrompt::Xref(path))
                } else {
                    (run.all_blocks, &run.blocks, FindPrompt::Block(name.clone()))
                };
                let key_name = match &prompt {
                    FindPrompt::Xref(path) => path.clone(),
                    _ => name.clone(),
                };
                let decided = answers
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(&key_name))
                    .map(|(_, yes)| *yes);
                match (all, decided) {
                    (true, _) | (_, Some(true)) => {}
                    (_, Some(false)) => {
                        run.next += 1;
                        continue;
                    }
                    (false, None) => {
                        run.prompt = Some(prompt);
                        self.find_replace.run = Some(run);
                        return Task::none();
                    }
                }
            }
            self.ensure_find_undo(i);
            let doc = &mut self.tabs[i].scene.document;
            if let Some(value) = field_text(doc, key) {
                let mut ranges = crate::app::text_match::str_ranges(&value, &pattern, &opts);
                if run.single {
                    ranges.truncate(1);
                }
                if !ranges.is_empty() {
                    let replaced = crate::app::text_match::replace_ranges(&value, &ranges, &with);
                    if set_field_text(doc, key, replaced) {
                        let counted = (
                            key.target,
                            match key.part {
                                FindPart::Attribute(n) => Some(n),
                                _ => None,
                            },
                        );
                        if !self.find_replace.changed.contains(&counted) {
                            self.find_replace.changed.push(counted);
                        }
                        if !self.find_replace.pending_entities.contains(&key.target) {
                            self.find_replace.pending_entities.push(key.target);
                        }
                        if result.kind == FindKind::Dimension {
                            self.tabs[i].scene.invalidate_dim_block_recorded(key.target);
                        }
                        if result.kind == FindKind::Xref {
                            if let Some(name) = definition {
                                if !xref_writes.iter().any(|n: &String| n.eq_ignore_ascii_case(&name)) {
                                    xref_writes.push(name);
                                }
                            }
                        } else if let Some(name) = definition {
                            if !self
                                .find_replace
                                .pending_blocks
                                .iter()
                                .any(|n| n.eq_ignore_ascii_case(&name))
                            {
                                self.find_replace.pending_blocks.push(name);
                            }
                        }
                    }
                }
            }
            run.next += 1;
        }
        for name in &xref_writes {
            self.write_back_find_xref(i, name, &pattern, &with);
        }
        if !xref_writes.is_empty() {
            self.find_replace.pending_xref_reload = true;
        }
        self.after_find_changes();
        if run.single {
            // The next match after the one replaced.
            let at = self
                .find_replace
                .matches
                .iter()
                .position(|r| r.key == run.matches[0].key)
                .unwrap_or(0);
            self.find_replace.matches = self.collect_find_matches(i);
            let still = self
                .find_replace
                .matches
                .iter()
                .position(|r| r.key == run.matches[0].key);
            let next = still.map_or(at, |p| p + 1);
            return self.show_find_match(next);
        }
        // The list keeps its rows and shows their new texts.
        let doc = &self.tabs[i].scene.document;
        for row in &mut self.find_replace.results {
            if let Some(value) = field_text(doc, row.key) {
                row.text = one_line(&shown_text(doc, row.key.target, row.key.part, &value));
            }
        }
        self.find_replace.current_match = None;
        self.find_replace.notice = Some(FindNotice::Finished {
            found: run.matches.len(),
            changed: self.find_replace.changed.len(),
        });
        Task::none()
    }

    /// The window is one undo step.
    fn ensure_find_undo(&mut self, i: usize) {
        if !self.find_replace.undo_pushed {
            self.push_undo_snapshot(i, "FIND");
            self.find_replace.undo_pushed = true;
        }
    }

    fn after_find_changes(&mut self) {
        let pending_entities = std::mem::take(&mut self.find_replace.pending_entities);
        let pending_blocks = std::mem::take(&mut self.find_replace.pending_blocks);
        let pending_unlocked = std::mem::take(&mut self.find_replace.pending_unlocked_layers);
        let xref_reload = std::mem::take(&mut self.find_replace.pending_xref_reload);
        if self.find_replace.changed.is_empty()
            && pending_entities.is_empty()
            && pending_unlocked.is_empty()
            && !xref_reload
        {
            return;
        }
        let i = self.active_tab;
        if xref_reload {
            self.tabs[i].scene.bump_geometry();
        } else {
            if !pending_unlocked.is_empty() {
                self.tabs[i]
                    .scene
                    .invalidate_layer_dependencies(&pending_unlocked);
            }
            if !pending_entities.is_empty() || !pending_blocks.is_empty() {
                let changes: Vec<_> = pending_entities
                    .iter()
                    .copied()
                    .map(|h| (h, crate::scene::ChangeKind::Modified))
                    .collect();
                for &handle in &pending_entities {
                    self.tabs[i].scene.reseed_derived_caches(handle);
                }
                self.tabs[i].scene.bump_entities_with_block_deps(
                    &changes,
                    &[],
                    &pending_blocks,
                    false,
                );
            }
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    /// Replace the text in an external reference's file and load it again.
    fn write_back_find_xref(&mut self, i: usize, block: &str, pattern: &str, with: &str) {
        let Some(raw) = self.tabs[i]
            .scene
            .document
            .block_records
            .get(block)
            .map(|r| r.xref_path.clone())
        else {
            return;
        };
        let base_dir = self.tabs[i]
            .current_path
            .as_ref()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_default();
        let Some(path) = crate::io::xref::resolve_path(&raw, &base_dir) else {
            return;
        };
        let Ok(mut source) = crate::io::load_file(&path) else {
            return;
        };
        let opts = self.find_replace.options.matching();
        // Only the texts that change are taken mutably: a mutable pass over
        // every entity drops their raw records and re-encodes the whole file.
        let edits: Vec<(Handle, String)> = source
            .entities()
            .filter_map(|e| {
                let value = match e {
                    EntityType::Text(t) => &t.value,
                    EntityType::MText(t) => &t.value,
                    _ => return None,
                };
                let ranges = crate::app::text_match::str_ranges(value, pattern, &opts);
                (!ranges.is_empty()).then(|| {
                    let replaced = crate::app::text_match::replace_ranges(value, &ranges, with);
                    (e.common().handle, replaced)
                })
            })
            .collect();
        if edits.is_empty() {
            return;
        }
        for (handle, replaced) in edits {
            match source.get_entity_mut(handle) {
                Some(EntityType::Text(t)) => t.value = replaced,
                Some(EntityType::MText(t)) => t.value = replaced,
                _ => {}
            }
        }
        if crate::io::save(&source, &path).is_ok() {
            let insert_keys: rustc_hash::FxHashSet<Handle> = self.tabs[i]
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
            for key in &insert_keys {
                self.tabs[i].xref_stat_cache.0.remove(&key.value());
            }
            let _ = crate::io::xref::resolve_xrefs_for_keys(
                &mut self.tabs[i].scene.document,
                &base_dir,
                &insert_keys,
            );
        }
    }

    // ── Searching ─────────────────────────────────────────────────────────

    /// Every match in search order: model space then the layouts in tab
    /// order (or the picked objects in picking order), each space by object
    /// age; an object's own text, then its hyperlink's text and address,
    /// then an insert's attributes and the texts of its definition.
    pub(super) fn collect_find_matches(&self, i: usize) -> Vec<FindResult> {
        let scene = &self.tabs[i].scene;
        let state = &self.find_replace;
        let pattern = state.search.trim();
        if pattern.is_empty() {
            return Vec::new();
        }
        let spaces = space_blocks(scene);
        let active = active_space_block(scene, &spaces);
        let doc = &scene.document;
        let mut tops: Vec<(usize, Handle)> = Vec::new();
        for entity in doc.entities() {
            let owner = entity.common().owner_handle;
            if let Some(n) = spaces.iter().position(|(h, _)| *h == owner) {
                tops.push((n, entity.common().handle));
            }
        }
        tops.sort_by_key(|(n, h)| (*n, h.value()));
        let tops: Vec<(usize, Handle)> = match state.scope {
            FindScope::Drawing => tops,
            FindScope::Space => tops
                .into_iter()
                .filter(|(n, _)| Some(*n) == active)
                .collect(),
            FindScope::Selection => state
                .selection
                .iter()
                .filter_map(|h| tops.iter().find(|(_, t)| t == h).copied())
                .collect(),
        };
        let mut walk = Walk {
            doc,
            opts: state.options,
            matching: state.options.matching(),
            pattern,
            out: Vec::new(),
        };
        for (n, handle) in tops {
            if let Some(entity) = doc.get_entity(handle) {
                walk.entity(handle, entity, &spaces[n].1, None, 0);
            }
        }
        walk.out
    }
}

fn new_run(matches: Vec<FindResult>, single: bool) -> ReplaceRun {
    ReplaceRun {
        matches,
        next: 0,
        single,
        prompt: None,
        locked: None,
        all_blocks: false,
        blocks: Vec::new(),
        all_xrefs: false,
        xrefs: Vec::new(),
    }
}

/// Model space then the paper layouts in tab order, as (block, name).
fn space_blocks(scene: &Scene) -> Vec<(Handle, String)> {
    let doc = &scene.document;
    let mut out = vec![(scene.model_space_block_handle(), "Model".to_string())];
    let mut layouts: Vec<(i16, String, Handle)> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            document::objects::ObjectType::Layout(l)
                if !l.name.eq_ignore_ascii_case("Model") && !l.name.is_empty() =>
            {
                Some((l.tab_order, l.name.clone(), l.block_record))
            }
            _ => None,
        })
        .collect();
    layouts.sort_by_key(|(order, _, _)| *order);
    layouts.dedup_by(|a, b| a.1 == b.1);
    // Layouts read without their block (DXF) follow the paper space blocks
    // in name order: *Paper_Space, *Paper_Space0, …
    let mut paper: Vec<&document::tables::BlockRecord> = doc
        .block_records
        .iter()
        .filter(|r| r.name.to_ascii_uppercase().starts_with("*PAPER_SPACE"))
        .collect();
    paper.sort_by_key(|r| (r.name.len(), r.name.to_ascii_uppercase()));
    for (n, (_, name, block)) in layouts.into_iter().enumerate() {
        let block = if block.is_null() {
            paper.get(n).map_or(Handle::NULL, |r| r.handle)
        } else {
            block
        };
        if !block.is_null() {
            out.push((block, name));
        }
    }
    out
}

fn active_space_block(scene: &Scene, spaces: &[(Handle, String)]) -> Option<usize> {
    if scene.current_layout.eq_ignore_ascii_case("Model") || scene.active_viewport.is_some() {
        return Some(0);
    }
    spaces
        .iter()
        .position(|(_, name)| name.eq_ignore_ascii_case(&scene.current_layout))
}

/// The block definition holding a match's text, when the text is not in a
/// space.
fn block_name_of(scene: &Scene, key: FindMatchKey) -> Option<String> {
    if key.owner == key.target {
        return None;
    }
    let doc = &scene.document;
    let owner = doc.get_entity(key.target)?.common().owner_handle;
    doc.block_records
        .iter()
        .find(|r| r.handle == owner)
        .map(|r| {
            // A text nested in an xref belongs to the xref's file.
            let mut name = r.name.clone();
            if let Some(EntityType::Insert(ins)) = doc.get_entity(key.owner) {
                if doc
                    .block_records
                    .get(&ins.block_name)
                    .is_some_and(|b| b.flags.is_xref || b.flags.is_xref_overlay)
                {
                    name = ins.block_name.clone();
                }
            }
            name
        })
}

struct Walk<'a> {
    doc: &'a document::Document,
    opts: FindOptions,
    matching: crate::app::text_match::MatchOptions,
    pattern: &'a str,
    out: Vec<FindResult>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Within {
    Block,
    Xref,
}

impl Walk<'_> {
    fn hidden_layer(&self, layer: &str) -> bool {
        self.doc
            .layers
            .get(layer)
            .is_some_and(|l| l.flags.off || l.flags.frozen)
    }

    fn push(&mut self, owner: Handle, target: Handle, part: FindPart, kind: FindKind, location: &str, value: &str, height: f64) {
        // Formatted texts are searched and listed as they read.
        let value = &shown_text(self.doc, target, part, value);
        if crate::app::text_match::str_ranges(value, self.pattern, &self.matching).is_empty() {
            return;
        }
        self.out.push(FindResult {
            key: FindMatchKey {
                owner,
                target,
                part,
            },
            location: location.to_string(),
            kind,
            text: one_line(value),
            height,
        });
    }

    fn entity(
        &mut self,
        owner: Handle,
        entity: &EntityType,
        location: &str,
        within: Option<Within>,
        depth: usize,
    ) {
        let common = entity.common();
        if self.opts.ignore_hidden && (common.invisible || self.hidden_layer(&common.layer)) {
            return;
        }
        let target = common.handle;
        let inner = |kind: FindKind| match within {
            Some(Within::Block) => FindKind::Block,
            Some(Within::Xref) => FindKind::Xref,
            None => kind,
        };
        match entity {
            EntityType::Text(t) if self.opts.texts => {
                self.push(owner, target, FindPart::Text, inner(FindKind::Text), location, &t.value, t.height)
            }
            EntityType::MText(t) if self.opts.texts => {
                self.push(owner, target, FindPart::Text, inner(FindKind::MText), location, &t.value, t.height)
            }
            EntityType::Dimension(d) if self.opts.dimensions => {
                if let Some(text) = d.base().text_override() {
                    if !text.trim().is_empty() && text.trim() != "<>" {
                        let height = dim_text_height(self.doc, &d.base().style_name);
                        self.push(owner, target, FindPart::Text, inner(FindKind::Dimension), location, text, height);
                    }
                }
            }
            EntityType::MultiLeader(ml) if self.opts.dimensions && ml.context.has_text_contents => {
                self.push(
                    owner,
                    target,
                    FindPart::Text,
                    inner(FindKind::MLeader),
                    location,
                    &ml.context.text_string,
                    ml.context.text_height,
                );
            }
            EntityType::Table(table) if self.opts.tables => {
                let height = crate::entities::table::data_text_height(table, self.doc);
                for row in 0..table.row_count() {
                    for col in 0..table.column_count() {
                        if let Some(text) = table.cell_text(row, col) {
                            let text = text.to_string();
                            self.push(owner, target, FindPart::Cell(row, col), inner(FindKind::Table), location, &text, height);
                        }
                    }
                }
            }
            _ => {}
        }
        if within.is_none() {
            if let Some(link) = crate::scene::hyperlink::Link::of(entity) {
                if self.opts.link_texts && !link.text.is_empty() {
                    self.push(owner, target, FindPart::LinkText, FindKind::LinkText, location, &link.text, 0.0);
                }
                if self.opts.links && !link.url.is_empty() {
                    self.push(owner, target, FindPart::LinkUrl, FindKind::Link, location, &link.url, 0.0);
                }
            }
        }
        if let EntityType::Insert(insert) = entity {
            if self.opts.attributes && within.is_none() {
                for (index, attribute) in insert.attributes.iter().enumerate() {
                    if self.opts.ignore_hidden
                        && (attribute.flags.invisible
                            || attribute.common.invisible
                            || self.hidden_layer(&attribute.common.layer))
                    {
                        continue;
                    }
                    let value = attribute.get_value().to_string();
                    self.push(
                        owner,
                        insert.common.handle,
                        FindPart::Attribute(index),
                        FindKind::Attribute,
                        location,
                        &value,
                        attribute.height,
                    );
                }
            }
            let Some(record) = self.doc.block_records.get(&insert.block_name) else {
                return;
            };
            let xref = record.flags.is_xref || record.flags.is_xref_overlay;
            let go = if xref { self.opts.xrefs } else { self.opts.blocks };
            if !go || depth > 16 {
                return;
            }
            let within = match within {
                Some(w) => Some(w),
                None if xref => Some(Within::Xref),
                None => Some(Within::Block),
            };
            let children = record.entity_handles.clone();
            for child in children {
                if let Some(child) = self.doc.get_entity(child) {
                    if matches!(child, EntityType::AttributeDefinition(_)) {
                        continue;
                    }
                    self.entity(owner, child, location, within, depth + 1);
                }
            }
        }
    }
}

fn dim_text_height(doc: &document::Document, style: &str) -> f64 {
    doc.dim_styles
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(style))
        .map(|s| {
            let scale = if s.dimscale > 1e-6 { s.dimscale } else { 1.0 };
            s.dimtxt * scale
        })
        .filter(|h| *h > 1e-9)
        .unwrap_or(2.5)
}

/// The text a match reads.
fn field_text(doc: &document::Document, key: FindMatchKey) -> Option<String> {
    let entity = doc.get_entity(key.target)?;
    match (key.part, entity) {
        (FindPart::Text, EntityType::Text(t)) => Some(t.value.clone()),
        (FindPart::Text, EntityType::MText(t)) => Some(t.value.clone()),
        (FindPart::Text, EntityType::Dimension(d)) => d.base().text_override().map(str::to_string),
        (FindPart::Text, EntityType::MultiLeader(ml)) => Some(ml.context.text_string.clone()),
        (FindPart::Cell(r, c), EntityType::Table(t)) => t.cell_text(r, c).map(str::to_string),
        (FindPart::Attribute(n), EntityType::Insert(ins)) => {
            ins.attributes.get(n).map(|a| a.get_value().to_string())
        }
        (FindPart::LinkText, e) => crate::scene::hyperlink::Link::of(e).map(|l| l.text),
        (FindPart::LinkUrl, e) => crate::scene::hyperlink::Link::of(e).map(|l| l.url),
        _ => None,
    }
}

/// Write a match's text back.
fn set_field_text(doc: &mut document::Document, key: FindMatchKey, value: String) -> bool {
    if let FindPart::LinkText | FindPart::LinkUrl = key.part {
        let Some(mut link) = doc.get_entity(key.target).and_then(crate::scene::hyperlink::Link::of)
        else {
            return false;
        };
        if key.part == FindPart::LinkText {
            link.text = value;
        } else {
            link.url = value;
        }
        crate::scene::hyperlink::set(doc, key.target, Some(&link));
        return true;
    }
    let Some(entity) = doc.get_entity_mut(key.target) else {
        return false;
    };
    match (key.part, entity) {
        (FindPart::Text, EntityType::Text(t)) => t.value = value,
        (FindPart::Text, EntityType::MText(t)) => t.value = value,
        (FindPart::Text, EntityType::Dimension(d)) => {
            crate::entities::dimension::set_dimension_text_override(d.base_mut(), Some(value))
        }
        (FindPart::Text, EntityType::MultiLeader(ml)) => ml.context.text_string = value,
        (FindPart::Cell(r, c), EntityType::Table(t)) => {
            if !t.set_cell_text(r, c, &value) {
                return false;
            }
            // The stored picture no longer shows the cells.
            t.block_record_handle = None;
        }
        (FindPart::Attribute(n), EntityType::Insert(ins)) => match ins.attributes.get_mut(n) {
            Some(a) => a.set_value(value),
            None => return false,
        },
        _ => return false,
    }
    true
}

/// Where a match reads in the drawing: the whole text's frame first, then
/// one quad per line the matched characters span. Texts and multiline texts
/// are boxed character by character; other matches (an attribute, a
/// dimension, a table cell, a hyperlink, a text in a block) show only their
/// object selected.
fn match_quads(
    doc: &document::Document,
    key: FindMatchKey,
    pattern: &str,
    opts: &crate::app::text_match::MatchOptions,
) -> Vec<[[f64; 2]; 4]> {
    use crate::entities::text_support::{resolve_dxf_special_chars, text_char_cells};
    // Cells as [left, bottom, right, top] in a frame at `origin`, turned by
    // `rotation`.
    let to_quads = |origin: [f64; 2], rotation: f64, boxes: Vec<[f64; 4]>| {
        let (sin, cos) = rotation.sin_cos();
        let world = |x: f64, y: f64| [origin[0] + x * cos - y * sin, origin[1] + x * sin + y * cos];
        boxes
            .into_iter()
            .filter(|[l, _, r, _]| r > l)
            .map(|[l, b, r, t]| [world(l, b), world(r, b), world(r, t), world(l, t)])
            .collect::<Vec<_>>()
    };
    // The frame of every cell, then each match's cells joined line by line
    // (cells of one line share their bottom).
    let runs = |cells: &[(usize, [f64; 4])], visible: &str| -> Vec<[f64; 4]> {
        let chars: Vec<Option<char>> = visible.chars().map(Some).collect();
        let join = |a: [f64; 4], c: [f64; 4]| [a[0].min(c[0]), a[1].min(c[1]), a[2].max(c[2]), a[3].max(c[3])];
        let frame = cells.iter().map(|(_, c)| *c).reduce(join);
        let mut out: Vec<[f64; 4]> = frame.into_iter().collect();
        for (start, end) in crate::app::text_match::find_ranges(&chars, pattern, opts) {
            let mut line: Option<[f64; 4]> = None;
            for (_, c) in cells.iter().filter(|(v, _)| (start..end).contains(v)) {
                line = Some(match line {
                    Some(a) if (a[1] - c[1]).abs() <= 1e-3 * (c[3] - c[1]).abs() => join(a, *c),
                    Some(a) => {
                        out.push(a);
                        *c
                    }
                    None => *c,
                });
            }
            out.extend(line);
        }
        out
    };
    let Some(entity) = doc.get_entity(key.target) else {
        return Vec::new();
    };
    match (key.part, entity) {
        (FindPart::Text, EntityType::Text(t)) if key.owner == key.target => {
            let p = crate::entities::text::text_run_placement_at_scale(t, doc, 1.0);
            let visible = resolve_dxf_special_chars(&t.value);
            let h = p.height as f64;
            let cells: Vec<(usize, [f64; 4])> =
                text_char_cells(&p.font, &visible, p.height, p.width_factor)
                    .into_iter()
                    .enumerate()
                    .map(|(v, (l, r))| (v, [l as f64, 0.0, r as f64, h]))
                    .collect();
            to_quads(p.origin, p.rotation as f64, runs(&cells, &visible))
        }
        (FindPart::Text, EntityType::MText(m)) if key.owner == key.target => {
            let cells: Vec<(usize, [f64; 4])> = crate::entities::mtext::glyph_boxes(m, doc)
                .into_iter()
                .map(|b| (b.vis, b.local.map(f64::from)))
                .collect();
            let flip = if crate::entities::text_support::resolve_text_style(&m.style, doc)
                .is_upside_down
            {
                std::f64::consts::PI
            } else {
                0.0
            };
            to_quads(
                [m.insertion_point.x, m.insertion_point.y],
                m.rotation + flip,
                runs(&cells, &mtext_visible_text(&m.value)),
            )
        }
        _ => Vec::new(),
    }
}

/// A multiline text's characters in the order its layout counts them (a
/// paragraph break, a tab and a stack's divider one each).
fn mtext_visible_text(raw: &str) -> String {
    use crate::entities::text_support::{adapt_mtext_paragraphs, MTextRunKind};
    let mut out = String::new();
    for (i, para) in adapt_mtext_paragraphs(raw, 1.0, false).iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for run in &para.runs {
            match &run.kind {
                MTextRunKind::Glyphs(text) => out.push_str(text),
                MTextRunKind::Tab => out.push('\t'),
                MTextRunKind::Stack {
                    numerator,
                    denominator,
                    ..
                } => {
                    out.push_str(numerator);
                    if !denominator.is_empty() {
                        out.push('/');
                        out.push_str(denominator);
                    }
                }
            }
        }
    }
    out
}

/// The highlight over a match, drawn over the drawing and never plotted:
/// the text's frame outlined and the matched characters shaded.
fn highlight(
    quads: &[[[f64; 2]; 4]],
) -> (Vec<crate::scene::WireModel>, Vec<crate::scene::HatchModel>) {
    use crate::scene::convert::tessellate::split_ds_xyz;
    use crate::scene::{WireGeom, WireModel};
    let Some((frame, matched)) = quads.split_first() else {
        return (Vec::new(), Vec::new());
    };
    let split = |corners: &[[f64; 2]; 4], order: &[usize]| {
        order
            .iter()
            .map(|&k| split_ds_xyz(corners[k][0], corners[k][1], 0.0))
            .unzip::<_, _, Vec<_>, Vec<_>>()
    };
    let (points, points_low) = split(frame, &[0, 1, 2, 3, 0]);
    let shade = matched
        .iter()
        .map(|q| {
            let origin = q[0];
            let ring = q.iter().map(|p| [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32]);
            crate::scene::HatchModel {
                entity: None,
                effective_layer: None,
                pattern_origin: None,
                render_instance: None,
                world_origin: origin,
                boundary: std::sync::Arc::new(ring.collect()),
                boundary_wcs: None,
                fill_plane: None,
                fill_plane_boundary: None,
                boundary_exterior: None,
                boundary_sources: None,
                boundary_paths: None,
                style: document::entities::HatchStyleType::Normal,
                pattern: crate::scene::model::hatch_model::HatchPattern::Solid,
                name: "FIND_MATCH".into(),
                color: FIND_MATCH_FILL,
                aci: 0,
                line_weight_px: 1.0,
                angle_offset: 0.0,
                scale: 1.0,
                draw_depth: 0.0,
            }
        })
        .collect();
    let wire = |name: &str, color: [f32; 4], geom: WireGeom| WireModel {
        display_visible: true,
        plot_visible: false,
        name: name.into(),
        color,
        line_weight_px: 1.0,
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        geom: std::sync::Arc::new(geom),
        ..Default::default()
    };
    let frame = wire(
        "__find_frame__",
        FIND_FRAME,
        WireGeom {
            points,
            points_low,
            ..WireGeom::default()
        },
    );
    (vec![frame], shade)
}

/// The shade over the matched characters and the outline of the text.
const FIND_MATCH_FILL: [f32; 4] = [0.20, 0.47, 0.85, 0.45];
const FIND_FRAME: [f32; 4] = [0.62, 0.66, 0.72, 1.0];

/// A text as it reads: multiline text, a multileader's text, a dimension's
/// override and a table cell carry formatting codes, shown without them.
fn shown_text(doc: &document::Document, target: Handle, part: FindPart, value: &str) -> String {
    let formatted = matches!(
        (part, doc.get_entity(target)),
        (FindPart::Text, Some(EntityType::MText(_) | EntityType::MultiLeader(_) | EntityType::Dimension(_)))
            | (FindPart::Cell(..), Some(EntityType::Table(_)))
    );
    if formatted {
        document::entities::mtext_format::parse_mtext(value, true).to_plain_text()
    } else {
        value.to_string()
    }
}

/// A text on one line, shortened, for the result list.
fn one_line(value: &str) -> String {
    let mut text = value.replace("\\P", " ").replace(['\r', '\n'], " ");
    if let Some((cut, _)) = text.char_indices().nth(80) {
        text.truncate(cut);
        text.push('…');
    }
    text
}
