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
                                .map(codec::Handle::new)
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
                        codec::EntityType::Insert(insert) => {
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
    /// attributes whose definition is gone are dropped. Constant definitions
    /// carry no attribute.
    pub(in crate::app) fn attsync_block(&mut self, i: usize, block: &str) {
        self.attsync_block_with(i, block, true);
    }

    /// [`Self::attsync_block`], opening its own undo step only when `undo`.
    pub(in crate::app) fn attsync_block_with(&mut self, i: usize, block: &str, undo: bool) {
        use codec::Entity as _;
        let doc = &self.tabs[i].scene.document;
        let attdefs: Vec<codec::entities::AttributeDefinition> = doc
            .block_records
            .get(block)
            .map(|br| {
                br.entity_handles
                    .iter()
                    .filter_map(|h| doc.get_entity(*h))
                    .filter_map(|e| match e {
                        codec::EntityType::AttributeDefinition(a) if !a.flags.constant => {
                            Some(a.clone())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let base = crate::scene::render_graph::block_base_point(doc, block);
        // A dynamic block's references point at their own anonymous
        // representation, so they are found through the definition.
        let def = doc.block_records.get(block).map(|br| br.handle);
        let inserts: Vec<_> = doc
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Insert(insert)
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
            let document = &self.tabs[i].scene.document;
            let Some(codec::EntityType::Insert(ins)) = document.get_entity(handle) else {
                continue;
            };
            let transform = crate::scene::render_graph::insert_transform_with_base(ins, base);
            let mut rebuilt = Vec::with_capacity(attdefs.len());
            for def in &attdefs {
                let previous = ins
                    .attributes
                    .iter()
                    .find(|a| a.tag.eq_ignore_ascii_case(&def.tag));
                let mut attribute = codec::entities::AttributeEntity::from_definition(
                    def,
                    previous.map(|a| a.value.clone()),
                );
                attribute.apply_transform(&transform);
                attribute.common.handle = previous.map_or(codec::Handle::NULL, |a| a.common.handle);
                attribute.common.owner_handle = handle;
                rebuilt.push(attribute);
            }
            for attribute in &mut rebuilt {
                if attribute.common.handle.is_null() {
                    attribute.common.handle = self.tabs[i].scene.document.allocate_handle();
                }
            }
            if let Some(codec::EntityType::Insert(ins)) =
                self.tabs[i].scene.document.get_entity_mut(handle)
            {
                ins.attributes = rebuilt;
                changes.push((handle, crate::scene::ChangeKind::Modified));
            }
        }
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
    }
}
