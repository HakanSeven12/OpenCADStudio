//! REFEDIT, -REFEDIT, REFSET and REFCLOSE: in-place editing of a block
//! reference or an external reference, with the reference's prompts and
//! messages. The session itself lives in `DocumentTab::refedit_session`.

use crate::app::{Message, OpenCADStudio};
use crate::modules::draw::modify::refedit::{
    apply_insert_inverse_transform, apply_insert_transform, level_hits, nesting_chain,
    rename_symbols, ChainLevel, RefCloseCommand, RefEditNames, RefEditOptionsCommand,
    RefEditPickCommand, RefEditSession, RefSetCommand, SymbolKind, NOT_CHECKED_OUT,
};
use crate::ui::window::refedit_dialog::{RefEditDialogMsg, RefEditDialogState, RefEditLevel};
use codec::{EntityType, Handle};
use iced::Task;
use std::collections::HashSet;

/// `** Command not allowed, NAME already checked out for editing **`
fn already_checked_out(name: &str) -> String {
    crate::tf!("** Command not allowed, {} already checked out for editing **", name).into_owned()
}

impl OpenCADStudio {
    pub(super) fn dispatch_refedit(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            "REFEDIT" | "-REFEDIT" => Some(self.start_refedit(i, cmd == "-REFEDIT")),
            "REFSET" | "REFSET_ADD" | "REFSET_REMOVE" => {
                if self.tabs[i].refedit_session.is_none() {
                    self.command_line.push_error(crate::t!(NOT_CHECKED_OUT).as_ref());
                    return Some(Task::none());
                }
                self.command_line.push_info(
                    crate::t!("Transfer objects between the RefEdit working set and host drawing...")
                        .as_ref(),
                );
                // The side toolbar's Add / Remove go straight to the selection.
                let command = match cmd {
                    "REFSET_ADD" => RefSetCommand::selecting(true),
                    "REFSET_REMOVE" => RefSetCommand::selecting(false),
                    _ => RefSetCommand::new(),
                };
                use crate::command::CadCommand;
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            "REFCLOSE" => {
                if self.tabs[i].refedit_session.is_none() {
                    self.command_line.push_error(crate::t!(NOT_CHECKED_OUT).as_ref());
                    return Some(Task::none());
                }
                let command = RefCloseCommand::new();
                use crate::command::CadCommand;
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            "REFCLOSE_SAVE" => {
                self.close_refedit(i, true);
                Some(Task::none())
            }
            "REFCLOSE_DISCARD" => {
                self.close_refedit(i, false);
                Some(Task::none())
            }
            _ if cmd.starts_with("REFEDIT_PICKED:") => {
                self.refedit_picked(i, &cmd["REFEDIT_PICKED:".len()..]);
                Some(Task::none())
            }
            _ if cmd.starts_with("REFEDIT_BEGIN:") => {
                self.begin_refedit(i, &cmd["REFEDIT_BEGIN:".len()..]);
                Some(Task::none())
            }
            _ if cmd.starts_with("REFSET_APPLY:") => {
                self.apply_refset(i, cmd.ends_with("ADD"));
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// Block references and xrefs of the current space.
    fn refedit_references(&self, i: usize) -> HashSet<Handle> {
        let space = self.tabs[i].scene.current_layout_block_handle_pub();
        self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| matches!(e, EntityType::Insert(_)) && e.common().owner_handle == space)
            .map(|e| e.common().handle)
            .collect()
    }

    fn start_refedit(&mut self, i: usize, command_line: bool) -> Task<Message> {
        if let Some(session) = &self.tabs[i].refedit_session {
            let message = already_checked_out(&session.block_name);
            self.command_line.push_error(&message);
            return Task::none();
        }
        let references = self.refedit_references(i);
        // A single preselected reference is taken as the pick.
        let selected: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        if let [handle] = selected[..] {
            if references.contains(&handle) {
                self.tabs[i].scene.deselect_all();
                self.refedit_picked(
                    i,
                    &format!("{}:{}", command_line as u8, handle.value()),
                );
                return Task::none();
            }
        }
        use crate::command::CadCommand;
        let command = RefEditPickCommand::new(command_line, references);
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// `cli:handle[:x:y:z]` — the reference was picked (at the point).
    fn refedit_picked(&mut self, i: usize, args: &str) {
        let parts: Vec<&str> = args.split(':').collect();
        let command_line = parts.first() == Some(&"1");
        let Some(top) = parts.get(1).and_then(|v| v.parse::<u64>().ok()).map(Handle::new) else {
            return;
        };
        let point = (parts.len() >= 5)
            .then(|| {
                Some(glam::DVec3::new(
                    parts[2].parse().ok()?,
                    parts[3].parse().ok()?,
                    parts[4].parse().ok()?,
                ))
            })
            .flatten();
        self.tabs[i].scene.deselect_all();
        let chain = nesting_chain(&self.tabs[i].scene.document, top, point);
        if chain.is_empty() {
            self.command_line
                .push_error(crate::t!("Object is not a valid reference").as_ref());
            return;
        }
        if command_line {
            let hits = chain
                .iter()
                .map(|level| level_hits(&self.tabs[i].scene.document, level))
                .collect();
            let command = RefEditOptionsCommand::new(
                top,
                chain,
                hits,
                self.refedit_options.lock,
                self.refedit_options.unique_names,
            );
            use crate::command::CadCommand;
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
        } else {
            self.open_refedit_dialog(i, top, chain);
        }
    }

    /// The Edit Reference dialog over the picked chain: the innermost level
    /// chosen, the remembered settings.
    fn open_refedit_dialog(&mut self, i: usize, top: Handle, chain: Vec<ChainLevel>) {
        let scene = &self.tabs[i].scene;
        let document = &scene.document;
        let levels = chain
            .iter()
            .map(|level| {
                let record = document.block_records.get(&level.block_name);
                RefEditLevel {
                    insert: level.insert,
                    name: level.block_name.clone(),
                    wires: scene.block_preview_wires(&level.block_name),
                    has_attdefs: record.is_some_and(|br| {
                        br.entity_handles.iter().any(|h| {
                            matches!(document.get_entity(*h), Some(EntityType::AttributeDefinition(_)))
                        })
                    }),
                    xref_path: record
                        .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                        .map(|br| br.xref_path.clone())
                        .filter(|p| !p.is_empty()),
                }
            })
            .collect::<Vec<_>>();
        let options = &self.refedit_options;
        self.refedit_dialog = Some(RefEditDialogState {
            top,
            level: levels.len().saturating_sub(1),
            levels,
            chain,
            nested: options.nested,
            attributes: options.attributes,
            lock: options.lock,
            unique: options.unique_names,
        });
        self.active_modal = Some(crate::app::ModalKind::RefEdit);
    }

    pub(in crate::app) fn update_refedit_dialog(&mut self, message: RefEditDialogMsg) -> Task<Message> {
        let Some(state) = self.refedit_dialog.as_mut() else {
            return Task::none();
        };
        match message {
            RefEditDialogMsg::Level(level) => {
                state.level = level.min(state.levels.len().saturating_sub(1))
            }
            RefEditDialogMsg::Nested(on) => state.nested = on,
            RefEditDialogMsg::Attributes(on) => state.attributes = on,
            RefEditDialogMsg::Lock(on) => state.lock = on,
            RefEditDialogMsg::Unique(on) => state.unique = on,
            RefEditDialogMsg::Help => {
                self.command_line
                    .push_info(&crate::tr!("ref-edit", "dialog-help"));
            }
            RefEditDialogMsg::Ok => {
                let Some(state) = self.refedit_dialog.take() else {
                    return Task::none();
                };
                self.close_active_modal();
                let attributes = state.attributes && state.attributes_enabled();
                self.refedit_options = crate::app::settings::RefEditOptions {
                    unique_names: state.unique,
                    attributes: state.attributes,
                    lock: state.lock,
                    nested: state.nested,
                };
                let i = self.active_tab;
                if state.nested {
                    let hits = state
                        .chain
                        .iter()
                        .map(|level| level_hits(&self.tabs[i].scene.document, level))
                        .collect();
                    let command = RefEditOptionsCommand::nested_pick(
                        state.top,
                        state.chain,
                        hits,
                        state.level,
                        attributes,
                        state.lock,
                        state.unique,
                    );
                    use crate::command::CadCommand;
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                } else {
                    let last = state.level.min(state.chain.len().saturating_sub(1));
                    let path: Vec<String> = state.chain[1..=last]
                        .iter()
                        .map(|l| l.insert.value().to_string())
                        .collect();
                    return self.dispatch_command(&format!(
                        "REFEDIT_BEGIN:{}:{}:{}::{}:{}",
                        state.top.value(),
                        path.join(","),
                        attributes as u8,
                        state.lock as u8,
                        state.unique as u8
                    ));
                }
            }
        }
        Task::none()
    }

    /// `top[:path[:attributes[:nested]]]` — open the session.
    fn begin_refedit(&mut self, i: usize, args: &str) {
        let mut parts = args.split(':');
        let Some(top) = parts
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Handle::new)
        else {
            return;
        };
        let handles = |s: Option<&str>| -> Vec<Handle> {
            s.unwrap_or("")
                .split(',')
                .filter_map(|v| v.parse::<u64>().ok())
                .map(Handle::new)
                .collect()
        };
        let path = handles(parts.next());
        let attributes = parts.next() == Some("1");
        let nested = handles(parts.next());
        let lock = parts.next().map_or(self.refedit_options.lock, |v| v == "1");
        let unique = parts
            .next()
            .map_or(self.refedit_options.unique_names, |v| v == "1");
        if self.tabs[i].refedit_session.is_some() || self.reject_locked_edit(i, top) {
            return;
        }
        let document = &self.tabs[i].scene.document;
        let mut chain = nesting_chain(document, top, None);
        if chain.is_empty() {
            self.command_line
                .push_error(crate::t!("Object is not a valid reference").as_ref());
            return;
        }
        // Follow the stored path of definition-owned references.
        for handle in path {
            let Some(EntityType::Insert(inner)) = document.get_entity(handle) else {
                break;
            };
            let base = crate::scene::render_graph::block_base_point(document, &inner.block_name);
            let local = crate::scene::render_graph::insert_transform_with_base(inner, base);
            let outer = chain.last().expect("non-empty chain").forward.clone();
            chain.push(crate::modules::draw::modify::refedit::ChainLevel {
                insert: handle,
                block_name: inner.block_name.clone(),
                forward: local.then(&outer),
            });
        }
        let level = chain.last().expect("non-empty chain").clone();
        // An external reference's dependent names (`XREF|NAME`) of the
        // outermost xref in the chain.
        let xref_prefix = chain.iter().find_map(|l| {
            document
                .block_records
                .get(&l.block_name)
                .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                .map(|br| format!("{}|", br.name))
        });
        let Some(br) = document.block_records.get(&level.block_name) else {
            return;
        };
        let br_handle = br.handle;
        let xref_path = (br.flags.is_xref || br.flags.is_xref_overlay)
            .then(|| br.xref_path.clone())
            .filter(|p| !p.is_empty());
        let extracted: Vec<Handle> = br
            .entity_handles
            .iter()
            .copied()
            .filter(|h| match document.get_entity(*h) {
                Some(EntityType::Block(_) | EntityType::BlockEnd(_)) | None => false,
                Some(EntityType::AttributeDefinition(_)) => attributes,
                Some(_) => nested.is_empty() || nested.contains(h),
            })
            .collect();
        let Some(inverse) =
            crate::modules::draw::modify::refedit::inverse_placement(&level.forward)
        else {
            return;
        };
        let session = RefEditSession {
            block_name: level.block_name.clone(),
            br_handle,
            temp_handles: Vec::new(),
            extracted: extracted.clone(),
            added: HashSet::new(),
            removed: HashSet::new(),
            forward: level.forward.clone(),
            inverse,
            // Everything allocated from here on is part of the session.
            handle_watermark: document.next_handle().max(document.header.handle_seed),
            xref_path,
            lock,
            names: RefEditNames::default(),
        };
        let copies: Vec<EntityType> = extracted
            .iter()
            .filter_map(|h| document.get_entity(*h).cloned())
            .collect();

        self.push_undo_snapshot(i, "REFEDIT");
        let mut session = session;
        let copies = match xref_prefix {
            Some(prefix) => {
                let (copies, names) = self.refedit_host_names(i, copies, &prefix, unique);
                session.names = names;
                copies
            }
            None => copies,
        };
        let mut temp_handles = Vec::new();
        for mut entity in copies {
            apply_insert_transform(&mut entity, &session);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = Handle::NULL;
            temp_handles.push(self.tabs[i].scene.add_entity(entity));
        }
        session.temp_handles = temp_handles.clone();
        self.tabs[i].refedit_session = Some(session);
        self.tabs[i].refedit_closed.clear();
        // Fade everything outside the working set by XFADECTL. (#136)
        self.tabs[i]
            .scene
            .set_refedit_keep(Some(temp_handles.iter().copied().collect::<rustc_hash::FxHashSet<_>>()));
        self.tabs[i].scene.refedit_lock = lock;
        self.tabs[i].scene.deselect_all();
        self.tabs[i].dirty = true;
        // No active command — the definition's objects are edited freely
        // (move, grips, draw, erase…) until REFCLOSE.
        self.tabs[i].active_cmd = None;
    }

    /// REFSET Add / Remove on the current selection.
    fn apply_refset(&mut self, i: usize, add: bool) {
        let Some(mut session) = self.tabs[i].refedit_session.take() else {
            self.command_line.push_error(crate::t!(NOT_CHECKED_OUT).as_ref());
            return;
        };
        let picked: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        let mut moved = 0usize;
        for h in picked {
            let inside = session.in_working_set(h);
            if add && !inside {
                session.removed.remove(&h);
                if !session.in_working_set(h) {
                    session.added.insert(h);
                }
                moved += 1;
            } else if !add && inside {
                session.added.remove(&h);
                if session.in_working_set(h) {
                    session.removed.insert(h);
                }
                moved += 1;
            }
        }
        let keep: rustc_hash::FxHashSet<Handle> = self.tabs[i]
            .scene
            .document
            .entities()
            .map(|e| e.common().handle)
            .filter(|h| session.in_working_set(*h))
            .collect();
        self.tabs[i].refedit_session = Some(session);
        self.tabs[i].scene.set_refedit_keep(Some(keep));
        self.tabs[i].scene.deselect_all();
        self.command_line.push_output(&if add {
            crate::tf!("{} Added to working set", moved).into_owned()
        } else {
            crate::tf!("{} Removed from working set", moved).into_owned()
        });
    }

    /// REFCLOSE Save (`save`) or Discard.
    fn close_refedit(&mut self, i: usize, save: bool) {
        let Some(session) = self.tabs[i].refedit_session.take() else {
            self.command_line.push_error(crate::t!(NOT_CHECKED_OUT).as_ref());
            return;
        };
        self.push_undo_snapshot(i, "REFCLOSE");
        self.command_line
            .push_output(crate::t!("Regenerating model.").as_ref());
        let space = self.tabs[i].scene.current_layout_block_handle_pub();
        // The working set: the copies still in it, everything created in this
        // space during the session (#423) and the host objects added with
        // REFSET — less what REFSET removed.
        let created: Vec<Handle> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| {
                let c = e.common();
                c.owner_handle == space && c.handle.value() >= session.handle_watermark
            })
            .map(|e| e.common().handle)
            .collect();
        let working: Vec<Handle> = created
            .iter()
            .chain(session.added.iter())
            .copied()
            .filter(|h| session.in_working_set(*h))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if !save {
            // The copies and new objects go; host objects added with REFSET
            // stay where they are, and copies moved out with REFSET Remove
            // stay in the host.
            let drop: Vec<Handle> = working
                .iter()
                .copied()
                .filter(|h| !session.added.contains(h))
                .collect();
            self.tabs[i].scene.erase_entities(&drop);
            self.finish_refedit_close(i, session);
            return;
        }

        use crate::modules::draw::modify::explode::normalize_entity_for_block;
        let originals: HashSet<Handle> = session.temp_handles.iter().copied().collect();
        let added_count = working.iter().filter(|h| !originals.contains(h)).count();
        let removed_count = session.extracted.len()
            - working.iter().filter(|h| originals.contains(h)).count();
        let edited: Vec<EntityType> = working
            .iter()
            .filter_map(|h| self.tabs[i].scene.document.get_entity(*h).cloned())
            .map(|mut entity| {
                // Back to the reference's own dependent names.
                rename_symbols(&mut entity, &|kind, name| {
                    session.names.original(kind, name).map(str::to_string)
                });
                apply_insert_inverse_transform(&mut entity, &session);
                let mut entity = normalize_entity_for_block(entity);
                entity.common_mut().handle = Handle::NULL;
                entity.common_mut().owner_handle = session.br_handle;
                entity
            })
            .collect();
        self.tabs[i].scene.erase_entities(&working);
        // The extracted definition objects are replaced; the rest stay.
        for h in &session.extracted {
            self.tabs[i].scene.document.remove_entity(*h);
        }
        if let Some(br) = self.tabs[i]
            .scene
            .document
            .block_records
            .get_mut(&session.block_name)
        {
            let extracted: HashSet<Handle> = session.extracted.iter().copied().collect();
            br.entity_handles.retain(|h| !extracted.contains(h));
        }
        for entity in edited {
            let _ = self.tabs[i].scene.document.add_entity(entity);
        }
        let instances = self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| {
                matches!(e, EntityType::Insert(ins)
                    if ins.block_name.eq_ignore_ascii_case(&session.block_name))
            })
            .count();
        let name = session.block_name.clone();
        match added_count {
            0 => {}
            1 => self
                .command_line
                .push_output(crate::tf!("1 object added to {}", name).as_ref()),
            n => self
                .command_line
                .push_output(crate::tf!("{} objects added to {}", n, name).as_ref()),
        }
        match removed_count {
            0 => {}
            1 => self
                .command_line
                .push_output(crate::tf!("1 object removed from {}", name).as_ref()),
            n => self
                .command_line
                .push_output(crate::tf!("{} objects removed from {}", n, name).as_ref()),
        }
        let reloaded = match &session.xref_path {
            // The file gets the whole new definition: the objects that were
            // not checked out stay in it as well as the edited ones.
            Some(raw) => match self.write_back_xref(i, &session, raw) {
                Ok(()) => true,
                Err(error) => {
                    self.command_line.push_error(&error);
                    false
                }
            },
            None => false,
        };
        let updated = match (session.xref_path.is_some(), instances) {
            (true, 1) => crate::t!("1 xref instance updated").into_owned(),
            (true, n) => crate::tf!("{} xref instances updated", n).into_owned(),
            (false, 1) => crate::t!("1 block instance updated").into_owned(),
            (false, n) => crate::tf!("{} block instances updated", n).into_owned(),
        };
        self.command_line.push_output(&updated);
        self.command_line.push_output(&if reloaded {
            crate::tf!("{} redefined and reloaded.", name).into_owned()
        } else {
            crate::tf!("{} redefined.", name).into_owned()
        });
        self.finish_refedit_close(i, session);
        // Rebuild hatch/image/mesh caches since block content changed.
        self.tabs[i].scene.rebuild_derived_caches();
    }

    fn finish_refedit_close(&mut self, i: usize, session: RefEditSession) {
        self.drop_refedit_names(i, &session.names);
        self.tabs[i].refedit_closed.push(session);
        self.tabs[i].scene.refedit_lock = false;
        self.tabs[i].scene.set_refedit_keep(None);
        self.tabs[i].scene.deselect_all();
        self.tabs[i].dirty = true;
    }

    /// Write the edited definition into the referenced drawing and reload it.
    fn write_back_xref(
        &mut self,
        i: usize,
        session: &RefEditSession,
        raw: &str,
    ) -> Result<(), String> {
        let edited: Vec<EntityType> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == session.br_handle)
            .cloned()
            .map(|mut entity| {
                entity.common_mut().handle = Handle::NULL;
                entity
            })
            .collect();
        let base_dir = self.tabs[i]
            .current_path
            .as_ref()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_default();
        let path = crate::io::xref::resolve_path(raw, &base_dir)
            .ok_or_else(|| format!("\"{raw}\" cannot be found."))?;
        let mut source = crate::io::load_file(&path)?;
        let prefix = format!("{}|", session.block_name);
        let strip = |name: &str| -> String {
            if name.len() > prefix.len()
                && name.is_char_boundary(prefix.len())
                && name[..prefix.len()].eq_ignore_ascii_case(&prefix)
            {
                name[prefix.len()..].to_string()
            } else {
                name.to_string()
            }
        };
        // Dependencies come from the host (where they carry the xref prefix)
        // and go back under their own names.
        let mut deps = crate::app::ClipboardDeps::capture(&self.tabs[i].scene.document, &edited);
        {
            use codec::TableEntry;
            for layer in &mut deps.layers {
                let name = strip(layer.name());
                layer.name = name;
            }
            for linetype in &mut deps.linetypes {
                let name = strip(linetype.name());
                linetype.name = name;
            }
            for style in &mut deps.text_styles {
                let name = strip(style.name());
                style.name = name;
            }
        }
        for block in &mut deps.blocks {
            block.name = strip(&block.name);
        }
        let entities: Vec<EntityType> = edited
            .into_iter()
            .map(|mut entity| {
                let common = entity.common_mut();
                common.layer = strip(&common.layer);
                common.linetype = strip(&common.linetype);
                common.layer_handle = None;
                common.linetype_handle = None;
                common.owner_handle = Handle::NULL;
                match &mut entity {
                    EntityType::Insert(insert) => insert.block_name = strip(&insert.block_name),
                    EntityType::Text(text) => text.style = strip(&text.style),
                    EntityType::MText(text) => text.style = strip(&text.style),
                    _ => {}
                }
                entity
            })
            .collect();
        let model: Vec<Handle> = source
            .model_space_entities()
            .map(|e| e.common().handle)
            .collect();
        for h in model {
            source.remove_entity(h);
        }
        crate::app::command_driver::clipboard::paste_entities_kernel(
            &mut source,
            &entities,
            &deps,
            None,
            crate::app::command_driver::clipboard::PasteSpace::Model,
        );
        if self.backup_on_save {
            crate::io::write_backup(&path);
        }
        crate::io::save(&source, &path)?;
        let keys: rustc_hash::FxHashSet<Handle> = [session.br_handle].into_iter().collect();
        self.tabs[i].xref_stat_cache.0.remove(&session.br_handle.value());
        let _ = crate::io::xref::resolve_xrefs_for_keys(
            &mut self.tabs[i].scene.document,
            &base_dir,
            &keys,
        );
        Ok(())
    }

    /// Undo of REFCLOSE checks the reference out again; undo of REFEDIT ends
    /// the session (their redo does the reverse).
    pub(in crate::app) fn refedit_after_history_step(&mut self, i: usize, label: &str, undo: bool) {
        match (label, undo) {
            ("REFCLOSE", true) | ("REFEDIT", false) => {
                let reopened = if undo {
                    self.tabs[i].refedit_closed.pop()
                } else {
                    self.tabs[i].refedit_reopen.pop()
                };
                if let Some(session) = reopened {
                    let keep: rustc_hash::FxHashSet<Handle> = self.tabs[i]
                        .scene
                        .document
                        .entities()
                        .map(|e| e.common().handle)
                        .filter(|h| session.in_working_set(*h))
                        .collect();
                    self.tabs[i].scene.refedit_lock = session.lock;
                    self.tabs[i].refedit_session = Some(session);
                    self.tabs[i].scene.set_refedit_keep(Some(keep));
                }
            }
            ("REFCLOSE", false) | ("REFEDIT", true) => {
                if let Some(session) = self.tabs[i].refedit_session.take() {
                    if undo {
                        self.tabs[i].refedit_reopen.push(session);
                    } else {
                        self.tabs[i].refedit_closed.push(session);
                    }
                    self.tabs[i].scene.refedit_lock = false;
                    self.tabs[i].scene.set_refedit_keep(None);
                }
            }
            _ => {}
        }
    }

    /// The working set's objects of an external reference under host names:
    /// `XREF|NAME` becomes `$N$NAME` (the first free N) with records of its
    /// own when `unique`, else `NAME`, sharing a host record of that name when
    /// one exists. Missing records are copied from the reference's.
    fn refedit_host_names(
        &mut self,
        i: usize,
        mut copies: Vec<EntityType>,
        prefix: &str,
        unique: bool,
    ) -> (Vec<EntityType>, RefEditNames) {
        let document = &mut self.tabs[i].scene.document;
        let deps = crate::app::ClipboardDeps::capture(document, &copies);
        let strip = |name: &str| -> Option<String> {
            (name.len() > prefix.len()
                && name.is_char_boundary(prefix.len())
                && name[..prefix.len()].eq_ignore_ascii_case(prefix))
            .then(|| name[prefix.len()..].to_string())
        };
        let exists = |document: &codec::CadDocument, kind: SymbolKind, name: &str| match kind {
            SymbolKind::Layer => document.layers.contains(name),
            SymbolKind::Linetype => document.line_types.contains(name),
            SymbolKind::TextStyle => document.text_styles.contains(name),
            SymbolKind::Block => document.block_records.get(name).is_some(),
        };
        let wanted: Vec<(SymbolKind, String)> = deps
            .layers
            .iter()
            .map(|r| (SymbolKind::Layer, r.name.clone()))
            .chain(deps.linetypes.iter().map(|r| (SymbolKind::Linetype, r.name.clone())))
            .chain(deps.text_styles.iter().map(|r| (SymbolKind::TextStyle, r.name.clone())))
            .chain(deps.blocks.iter().map(|b| (SymbolKind::Block, b.name.clone())))
            .collect();
        let mut names = RefEditNames::default();
        for (kind, original) in wanted {
            let Some(base) = strip(&original) else {
                continue;
            };
            let taken = |names: &RefEditNames, candidate: &str| {
                exists(document, kind, candidate)
                    || names
                        .renamed
                        .iter()
                        .any(|(k, used, _)| *k == kind && used.eq_ignore_ascii_case(candidate))
            };
            // Layer 0 is the host's own in every drawing.
            let used = if kind == SymbolKind::Layer && base == "0" {
                base
            } else if unique {
                (0u32..)
                    .map(|n| format!("${n}${base}"))
                    .find(|candidate| !taken(&names, candidate))
                    .expect("a free name")
            } else {
                base
            };
            if !exists(document, kind, &used)
                && !names
                    .created
                    .iter()
                    .any(|(k, n)| *k == kind && n.eq_ignore_ascii_case(&used))
            {
                names.created.push((kind, used.clone()));
            }
            names.renamed.push((kind, used, original));
        }
        let used_for = |kind: SymbolKind, name: &str| {
            names
                .renamed
                .iter()
                .find(|(k, _, original)| *k == kind && original.eq_ignore_ascii_case(name))
                .map(|(_, used, _)| used.clone())
        };
        let created = |kind: SymbolKind, name: &str| -> Option<String> {
            used_for(kind, name).filter(|used| {
                names
                    .created
                    .iter()
                    .any(|(k, n)| *k == kind && n.eq_ignore_ascii_case(used))
            })
        };
        for record in &deps.layers {
            if let Some(used) = created(SymbolKind::Layer, &record.name) {
                let mut copy = record.clone();
                copy.name = used;
                copy.flags.xref_dependent = false;
                copy.xref_block_record_handle = Handle::NULL;
                if let Some(linetype) = used_for(SymbolKind::Linetype, &copy.line_type) {
                    copy.line_type = linetype;
                }
                copy.handle = document.allocate_handle();
                let _ = document.layers.add(copy);
            }
        }
        for record in &deps.linetypes {
            if let Some(used) = created(SymbolKind::Linetype, &record.name) {
                let mut copy = record.clone();
                copy.name = used;
                copy.xref_dependent = false;
                copy.xref_block_record_handle = Handle::NULL;
                copy.handle = document.allocate_handle();
                let _ = document.line_types.add(copy);
            }
        }
        for record in &deps.text_styles {
            if let Some(used) = created(SymbolKind::TextStyle, &record.name) {
                let mut copy = record.clone();
                copy.name = used;
                copy.xref_dependent = false;
                copy.xref_block_record_handle = Handle::NULL;
                copy.handle = document.allocate_handle();
                let _ = document.text_styles.add(copy);
            }
        }
        let rename = |kind: SymbolKind, name: &str| used_for(kind, name);
        let blocks: Vec<crate::app::BlockDef> = deps
            .blocks
            .iter()
            .filter_map(|def| {
                let used = created(SymbolKind::Block, &def.name)?;
                let mut entities = def.entities.clone();
                for entity in &mut entities {
                    rename_symbols(entity, &rename);
                }
                Some(crate::app::BlockDef {
                    name: used,
                    base_point: def.base_point,
                    entities,
                })
            })
            .collect();
        crate::app::command_driver::clipboard::merge_block_defs(document, &blocks);
        for entity in &mut copies {
            rename_symbols(entity, &rename);
        }
        (copies, names)
    }

    /// Removes the host records made for a session that nothing uses any more.
    fn drop_refedit_names(&mut self, i: usize, names: &RefEditNames) {
        for (kind, name) in names.created.iter().rev() {
            let scene = &mut self.tabs[i].scene;
            let in_use = scene.document.entities().any(|entity| {
                let common = entity.common();
                match kind {
                    SymbolKind::Layer => common.layer.eq_ignore_ascii_case(name),
                    SymbolKind::Linetype => common.linetype.eq_ignore_ascii_case(name),
                    SymbolKind::TextStyle => match entity {
                        EntityType::Text(t) => t.style.eq_ignore_ascii_case(name),
                        EntityType::MText(t) => t.style.eq_ignore_ascii_case(name),
                        EntityType::AttributeDefinition(a) => {
                            a.text_style.eq_ignore_ascii_case(name)
                        }
                        _ => false,
                    },
                    SymbolKind::Block => matches!(
                        entity,
                        EntityType::Insert(insert) if insert.block_name.eq_ignore_ascii_case(name)
                    ),
                }
            });
            // A linetype made for the session may also be a kept layer's.
            let in_use = in_use
                || (*kind == SymbolKind::Linetype
                    && scene
                        .document
                        .layers
                        .iter()
                        .any(|layer| layer.line_type.eq_ignore_ascii_case(name)));
            if in_use {
                continue;
            }
            match kind {
                SymbolKind::Layer => {
                    scene.document.layers.remove(name);
                }
                SymbolKind::Linetype => {
                    scene.document.line_types.remove(name);
                }
                SymbolKind::TextStyle => {
                    scene.document.text_styles.remove(name);
                }
                SymbolKind::Block => {
                    let _ = crate::scene::block_service::erase(scene, name);
                }
            }
        }
    }
}
