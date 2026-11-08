//! Command-line Block Definition commands: -BLOCK, -WBLOCK, -BEDIT,
//! ATTSYNC, INSBASE, and the BATTMAN check for attributed blocks. The
//! interactive steps live in `modules::insert::block_cmdline`.

use crate::app::{Message, OpenCADStudio};
use crate::modules::insert::block_cmdline::{
    AttsyncCommand, BlockTable, DashBeditCommand, DashBlockCommand, DashWblockCommand, WblockSource,
};
use iced::Task;

impl OpenCADStudio {
    pub(super) fn dispatch_block_cmdline(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        use crate::command::CadCommand;
        match cmd {
            "-BLOCK" | "-BMAKE" => {
                let preselected: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                let table = BlockTable::of(&self.tabs[i].scene.document);
                let units = self.tabs[i].scene.document.header.insertion_units;
                let command =
                    DashBlockCommand::new(table, self.tabs[i].ucs_xform(), preselected, units);
                self.command_line.push_output(&command.settings_line());
                self.start_block_cmdline(i, Box::new(command));
            }
            "-WBLOCK" | "-WB" => {
                let preselected: Vec<_> = self.tabs[i].scene.selected.iter().copied().collect();
                let table = BlockTable::of(&self.tabs[i].scene.document);
                let command = DashWblockCommand::new(table, preselected);
                self.command_line.push_output(&command.settings_line());
                if crate::sys::file_dia() {
                    // FILEDIA 1: the output file comes from the file dialog,
                    // the rest from the command line.
                    return Some(Task::done(Message::WblockSave("-".to_string())));
                }
                self.start_block_cmdline(i, Box::new(command));
            }
            cmd if cmd.starts_with("_-WBLOCKFILE ") => {
                let path = cmd["_-WBLOCKFILE ".len()..].trim().to_string();
                let preselected: Vec<_> = self.tabs[i].scene.selected.iter().copied().collect();
                let table = BlockTable::of(&self.tabs[i].scene.document);
                let mut command = DashWblockCommand::new(table, preselected);
                let _ = command.on_text_input(&path);
                self.start_block_cmdline(i, Box::new(command));
            }
            cmd if cmd.starts_with("_WBLOCKWRITE ") => {
                self.tabs[i].active_cmd = None;
                let Ok(request) =
                    serde_json::from_str::<serde_json::Value>(&cmd["_WBLOCKWRITE ".len()..])
                else {
                    return Some(Task::none());
                };
                let path = std::path::PathBuf::from(request["path"].as_str().unwrap_or(""));
                let source = &request["source"];
                let source = if let Some(name) = source["block"].as_str() {
                    WblockSource::Block(name.to_string())
                } else if source["drawing"].as_bool() == Some(true) {
                    WblockSource::Drawing
                } else {
                    let handles = source["handles"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_u64())
                                .map(document::Handle::new)
                                .collect()
                        })
                        .unwrap_or_default();
                    let b = &source["base"];
                    let base = glam::DVec3::new(
                        b[0].as_f64().unwrap_or(0.0),
                        b[1].as_f64().unwrap_or(0.0),
                        b[2].as_f64().unwrap_or(0.0),
                    );
                    let mode = crate::modules::insert::block_cmdline::mode_from_code(
                        source["mode"].as_u64().unwrap_or(0) as u8,
                    );
                    WblockSource::Objects {
                        handles,
                        base,
                        mode,
                    }
                };
                let unit = self.tabs[i].scene.document.header.insertion_units;
                return Some(self.wblock_write(i, source, path, unit));
            }
            "-BEDIT" => {
                if self.tabs[i].refedit_session.is_some() {
                    self.command_line.push_error(
                        crate::t!("BEDIT: finish the active REFEDIT (REFCLOSE) first.").as_ref(),
                    );
                    return Some(Task::none());
                }
                let table = BlockTable::of(&self.tabs[i].scene.document);
                self.start_block_cmdline(i, Box::new(DashBeditCommand::new(table)));
            }
            "ATTSYNC" => {
                let doc = &self.tabs[i].scene.document;
                let table = BlockTable::of(doc);
                let references = doc
                    .entities()
                    .filter_map(|entity| match entity {
                        document::EntityType::Insert(insert) => {
                            Some((insert.common.handle, insert.block_name.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                self.start_block_cmdline(i, Box::new(AttsyncCommand::new(table, references)));
            }
            cmd if cmd.starts_with("_ATTSYNCBLOCK ") => {
                self.tabs[i].active_cmd = None;
                let block = cmd["_ATTSYNCBLOCK ".len()..].trim().to_string();
                self.attsync_block(i, &block);
                self.command_line.push_output("ATTSYNC complete.");
            }
            // ATTREDEF — take the attributes of an existing reference as the
            // block's new attribute definitions, then bring every reference
            // in line with them.
            "ATTREDEF" => {
                use crate::command::FreeTextValuePromptCommand;
                let command = FreeTextValuePromptCommand::new(
                    "ATTREDEF",
                    "ATTREDEF  Enter name of block to redefine:",
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            cmd if cmd.starts_with("ATTREDEF ") => {
                let name = cmd["ATTREDEF ".len()..].trim().to_string();
                if name.is_empty() {
                    self.command_line
                        .push_info(crate::t!("Usage: ATTREDEF <block name>").as_ref());
                    return Some(Task::none());
                }
                self.attredef_block(i, &name);
            }
            "INSBASE" => {
                let base = self.current_insertion_base(i);
                let command = crate::modules::insert::base_point::BaseCommand::insbase(base);
                self.start_block_cmdline(i, Box::new(command));
            }
            cmd if cmd == "ATTMAN"
                || cmd == "BATTMAN"
                || cmd.starts_with("ATTMAN ")
                || cmd.starts_with("BATTMAN ") =>
            {
                if BlockTable::of(&self.tabs[i].scene.document)
                    .attributed
                    .is_empty()
                {
                    self.command_line
                        .push_output("This drawing contains no attributed blocks.");
                    return Some(Task::none());
                }
                let preselected = self.tabs[i].scene.selected_handles_in_order();
                return Some(self.open_attman(i, &preselected));
            }
            cmd if cmd == "_ATTMAN_PICK" || cmd.starts_with("_ATTMAN_PICK ") => {
                self.tabs[i].active_cmd = None;
                return Some(self.attman_picked(i, cmd["_ATTMAN_PICK".len()..].trim()));
            }
            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }

    fn start_block_cmdline(&mut self, i: usize, command: Box<dyn crate::command::CadCommand>) {
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[i].active_cmd = Some(command);
    }

    /// The insertion base of the active space (INSBASE / BASE).
    pub(in crate::app) fn current_insertion_base(&self, i: usize) -> glam::DVec3 {
        let header = &self.tabs[i].scene.document.header;
        let base = if self.tabs[i].scene.current_layout != "Model" {
            header.paper_space_insertion_base
        } else {
            header.model_space_insertion_base
        };
        glam::DVec3::new(base.x, base.y, base.z)
    }

    /// Rebuild the attributes of every editable reference of `block` from the
    /// block's attribute definitions, in their order: each attribute takes
    /// its definition's placement and text settings through the reference's
    /// transform, keeps the value it had (new ones take the default), and
    /// attributes whose definition is gone are dropped.
    pub(in crate::app) fn attsync_block(&mut self, i: usize, block: &str) {
        self.attsync_block_with(i, block, true);
    }

    /// [`Self::attsync_block`], opening its own undo step only when `undo`.
    pub(in crate::app) fn attsync_block_with(&mut self, i: usize, block: &str, undo: bool) {
        use document::Entity as _;
        let doc = &self.tabs[i].scene.document;
        let attdefs = block_attribute_definitions(doc, block);
        let base = crate::scene::render_graph::block_base_point(doc, block);
        // A dynamic block's references point at their own anonymous
        // representation, so they are found through the definition.
        let def = doc.block_records.get(block).map(|br| br.handle);
        let inserts: Vec<_> = doc
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Insert(insert)
                    if (insert.block_name.eq_ignore_ascii_case(block)
                        || def.is_some()
                            && doc.dynamic_definition_for_insert(insert.common.handle) == def)
                        && !self.tabs[i].scene.is_layer_locked(insert.common.handle) =>
                {
                    Some(insert.common.handle)
                }
                _ => None,
            })
            .collect();
        if inserts.is_empty() {
            return;
        }
        if undo {
            self.push_undo_snapshot(i, "ATTSYNC");
        }
        let mut changes = Vec::new();
        for handle in inserts {
            if self.rebuild_block_attributes(i, handle, &attdefs, base) {
                changes.push((handle, crate::scene::ChangeKind::Modified));
            }
        }
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
    }

    /// Rebuild one reference's attributes from the definitions, in their
    /// order, keeping each value by tag and dropping attributes whose
    /// definition is gone. Returns whether the reference changed.
    fn rebuild_block_attributes(
        &mut self,
        i: usize,
        handle: document::Handle,
        attdefs: &[document::entities::AttributeDefinition],
        base: document::types::Vector3,
    ) -> bool {
        use document::Entity as _;
        let document = &self.tabs[i].scene.document;
        let Some(document::EntityType::Insert(ins)) = document.get_entity(handle) else {
            return false;
        };
        let transform = crate::scene::render_graph::insert_transform_with_base(ins, base);
        let had_end = ins.seqend_handle;
        let mut rebuilt = Vec::with_capacity(attdefs.len());
        for def in attdefs {
            let previous = ins
                .attributes
                .iter()
                .find(|a| a.tag.eq_ignore_ascii_case(&def.tag));
            let mut attribute = document::entities::AttributeEntity::from_definition(
                def,
                previous.map(|a| a.value.clone()),
            );
            attribute.apply_transform(&transform);
            attribute.common.handle = previous.map_or(document::Handle::NULL, |a| a.common.handle);
            attribute.common.owner_handle = handle;
            rebuilt.push(attribute);
        }
        for attribute in &mut rebuilt {
            if attribute.common.handle.is_null() {
                attribute.common.handle = self.tabs[i].scene.document.allocate_handle();
            }
            // The definition's own text object cannot be shared with the
            // attribute that copies it, any more than its handle can be.
            if let Some(model) = &mut attribute.embedded_mtext {
                model.common.handle = self.tabs[i].scene.document.allocate_handle();
                model.common.owner_handle = attribute.common.handle;
                model.common.raw_record = None;
                model.common.xdictionary_handle = None;
                model.common.reactors.clear();
            }
        }
        let end = if rebuilt.is_empty() {
            None
        } else if let Some(end) = had_end {
            Some(end)
        } else {
            Some(self.tabs[i].scene.document.allocate_handle())
        };
        if let Some(document::EntityType::Insert(ins)) =
            self.tabs[i].scene.document.get_entity_mut(handle)
        {
            ins.attributes = rebuilt;
            ins.seqend_handle = end;
            true
        } else {
            false
        }
    }

    /// ATTREDEF: make the attributes of one reference the block's attribute
    /// definitions, then synchronise every reference to them. The reference's
    /// world placement is folded back through the insert transform, so the
    /// definitions keep sitting on the geometry they were authored against.
    pub(in crate::app) fn attredef_block(&mut self, i: usize, block: &str) {
        use document::Entity as _;
        use document::entities::{AttributeDefinition, EntityType};

        let doc = &self.tabs[i].scene.document;
        let Some(record) = doc
            .block_records
            .iter()
            .find(|record| record.name.eq_ignore_ascii_case(block))
        else {
            self.command_line.push_error(
                crate::tf!("ATTREDEF: no block named \"{block}\".").as_ref(),
            );
            return;
        };
        let name = record.name.clone();
        let record_handle = record.handle;
        let mut old_attdefs: Vec<document::Handle> = record
            .entity_handles
            .iter()
            .copied()
            .filter(|handle| {
                matches!(
                    doc.get_entity(*handle),
                    Some(EntityType::AttributeDefinition(_))
                )
            })
            .collect();
        let old_definitions: Vec<AttributeDefinition> = old_attdefs
            .iter()
            .filter_map(|handle| match doc.get_entity(*handle) {
                Some(EntityType::AttributeDefinition(definition)) => Some(definition.clone()),
                _ => None,
            })
            .collect();

        // Prefer a selected reference as the template; otherwise the first
        // reference of the block is the one whose attributes are kept.
        let selected: rustc_hash::FxHashSet<_> =
            self.tabs[i].scene.selected.iter().copied().collect();
        let references: Vec<document::Handle> = doc
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Insert(insert) if insert.block_name.eq_ignore_ascii_case(&name) => {
                    Some(insert.common.handle)
                }
                _ => None,
            })
            .collect();
        let Some(template_handle) = references
            .iter()
            .copied()
            .find(|handle| selected.contains(handle))
            .or_else(|| references.first().copied())
        else {
            self.command_line.push_error(
                crate::tf!("ATTREDEF: no reference of \"{name}\" to redefine from.").as_ref(),
            );
            return;
        };
        let Some(EntityType::Insert(template)) = doc.get_entity(template_handle) else {
            return;
        };
        if template.attributes.is_empty() {
            self.command_line.push_error(
                crate::tf!("ATTREDEF: the reference of \"{name}\" has no attributes.").as_ref(),
            );
            return;
        }

        let base = crate::scene::render_graph::block_base_point(doc, &name);
        let transform = crate::scene::render_graph::insert_transform_with_base(template, base);
        let Some(inverse) = crate::modules::draw::modify::refedit::inverse_placement(&transform)
        else {
            self.command_line.push_error(
                crate::tf!("ATTREDEF: the reference of \"{name}\" cannot be inverted.").as_ref(),
            );
            return;
        };

        let mut definitions = Vec::with_capacity(template.attributes.len());
        for attribute in &template.attributes {
            let prompt = old_definitions
                .iter()
                .find(|definition| definition.tag.eq_ignore_ascii_case(&attribute.tag))
                .map(|definition| definition.prompt.clone())
                .unwrap_or_else(|| attribute.tag.clone());
            let mut definition =
                AttributeDefinition::new(attribute.tag.clone(), prompt, attribute.value.clone());
            let mut local = attribute.clone();
            local.apply_transform(&inverse);
            definition.insertion_point = local.insertion_point;
            definition.alignment_point = local.alignment_point;
            definition.height = local.height;
            definition.rotation = local.rotation;
            definition.width_factor = local.width_factor;
            definition.oblique_angle = local.oblique_angle;
            definition.text_style = local.text_style;
            definition.text_generation_flags = local.text_generation_flags;
            definition.horizontal_alignment = local.horizontal_alignment;
            definition.vertical_alignment = local.vertical_alignment;
            definition.flags = local.flags;
            definition.field_length = local.field_length;
            definition.normal = local.normal;
            definition.mtext_flag = local.mtext_flag;
            definition.is_multiline = local.is_multiline;
            definition.line_count = local.line_count;
            definition.embedded_mtext = local.embedded_mtext;
            definition.lock_position = local.lock_position;
            definition.common = local.common;
            definitions.push(definition);
        }

        self.push_undo_snapshot(i, "ATTREDEF");
        let doc = &mut self.tabs[i].scene.document;
        let replaced: rustc_hash::FxHashSet<document::Handle> =
            old_attdefs.iter().copied().collect();
        for handle in old_attdefs.drain(..) {
            doc.remove_entity(handle);
        }
        let mut new_handles = Vec::with_capacity(definitions.len());
        for mut definition in definitions {
            definition.common.handle = doc.allocate_handle();
            definition.common.owner_handle = record_handle;
            definition.common.raw_record = None;
            definition.common.xdictionary_handle = None;
            definition.common.reactors.clear();
            if let Some(mtext) = &mut definition.embedded_mtext {
                mtext.common.handle = doc.allocate_handle();
                mtext.common.owner_handle = definition.common.handle;
                mtext.common.raw_record = None;
                mtext.common.xdictionary_handle = None;
                mtext.common.reactors.clear();
            }
            match doc.add_entity(EntityType::AttributeDefinition(definition)) {
                Ok(handle) => new_handles.push(handle),
                Err(_) => {}
            }
        }
        if let Some(record) = doc.block_records.get_mut(&name) {
            record.entity_handles.retain(|handle| !replaced.contains(handle));
            for handle in &new_handles {
                if !record.entity_handles.contains(handle) {
                    record.entity_handles.push(*handle);
                }
            }
        }

        self.attsync_block_with(i, &name, false);
        self.tabs[i].dirty = true;
        self.command_line.push_output(
            crate::tf!(
                "ATTREDEF: redefined \"{name}\" from its reference ({} attribute(s)).",
                new_handles.len()
            )
            .as_ref(),
        );
    }
}

/// The attribute definitions a block record owns, in its own order.
fn block_attribute_definitions(
    doc: &document::Document,
    block: &str,
) -> Vec<document::entities::AttributeDefinition> {
    doc.block_records
        .get(block)
        .map(|record| {
            record
                .entity_handles
                .iter()
                .filter_map(|handle| doc.get_entity(*handle))
                .filter_map(|entity| match entity {
                    document::EntityType::AttributeDefinition(definition) => {
                        Some(definition.clone())
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}
