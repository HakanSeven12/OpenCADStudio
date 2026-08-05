use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_blocks(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // ── BASE — drawing insertion base point ───────────────────────
            // Bare BASE picks a point interactively; BASE <x> <y> [z] sets it
            // directly. The base point is stored per active space (model/paper).
            cmd if cmd == "BASE" || cmd.starts_with("BASE ") => {
                let rest = cmd.strip_prefix("BASE").unwrap_or("").trim();
                if rest.is_empty() {
                    use crate::modules::insert::base_point::BaseCommand;
                    let c = BaseCommand::new();
                    self.command_line.push_info(&c.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(c));
                } else {
                    let nums: Vec<f64> = rest
                        .split(|ch| ch == ' ' || ch == ',')
                        .filter(|s| !s.is_empty())
                        .filter_map(|s| s.parse::<f64>().ok())
                        .collect();
                    if nums.len() >= 2 {
                        let z = nums.get(2).copied().unwrap_or(0.0);
                        let pt = acadrust::types::Vector3::new(nums[0], nums[1], z);
                        let is_paper = self.tabs[i].scene.current_layout != "Model";
                        self.push_undo_snapshot(i, "BASE");
                        if is_paper {
                            self.tabs[i]
                                .scene
                                .document
                                .header
                                .paper_space_insertion_base = pt;
                        } else {
                            self.tabs[i]
                                .scene
                                .document
                                .header
                                .model_space_insertion_base = pt;
                        }
                        self.tabs[i].dirty = true;
                        let space = if is_paper {
                            "paper space"
                        } else {
                            "model space"
                        };
                        self.command_line.push_output(crate::tf!(
                            "Base point ({}, {}, {}) set for {space}.",
                            nums[0], nums[1], z
                        ).as_ref());
                    } else {
                        self.command_line.push_error(crate::t!("Usage: BASE <x> <y> [z]").as_ref());
                    }
                }
            }

            "COPYCLIP" => {
                // MText editor open: Ctrl+C copies its selected text, not the
                // drawing's entities.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    return match self.mtext_selected_text() {
                        Some(text) => {
                            self.command_line
                                .push_info(crate::t!("Copied selected text to clipboard.").as_ref());
                            Some(iced::clipboard::write(text).discard())
                        }
                        None => Some(Task::none()),
                    };
                }
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("COPYCLIP");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let entities: Vec<_> = handles
                        .iter()
                        .filter_map(|&h| self.tabs[i].scene.document.get_entity(h).cloned())
                        .collect();
                    self.clipboard_base = super::super::helpers::entities_lower_left_by_bbox(
                        &self.tabs[i].scene.document,
                        &handles,
                    );
                    self.clipboard = entities;
                    self.clipboard_deps = super::super::ClipboardDeps::capture(
                        &self.tabs[i].scene.document,
                        &self.clipboard,
                    );
                    self.command_line.push_info(crate::tf!(
                        "{} object(s) copied to clipboard.",
                        self.clipboard.len()
                    ).as_ref());
                }
            }

            // COPYBASE — copy the selection to the clipboard with a picked base
            // point (vs COPYCLIP, which uses the selection's lower-left corner).
            "COPYBASE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("COPYBASE");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::clipboard::copy_base::CopyBaseCommand;
                    let cmd = CopyBaseCommand::new();
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            // Internal: the base point picked by COPYBASE — perform the copy.
            cmd if cmd.starts_with("COPYBASE_AT ") => {
                let coords: Vec<f64> = cmd
                    .trim_start_matches("COPYBASE_AT")
                    .split_whitespace()
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if coords.len() == 3 && !handles.is_empty() {
                    let base = glam::DVec3::new(coords[0], coords[1], coords[2]);
                    let entities: Vec<_> = handles
                        .iter()
                        .filter_map(|&h| self.tabs[i].scene.document.get_entity(h).cloned())
                        .collect();
                    self.clipboard_base = base;
                    self.clipboard = entities;
                    self.clipboard_deps = super::super::ClipboardDeps::capture(
                        &self.tabs[i].scene.document,
                        &self.clipboard,
                    );
                    self.command_line.push_info(crate::tf!(
                        "{} object(s) copied to clipboard (base {:.3},{:.3}).",
                        self.clipboard.len(),
                        base.x,
                        base.y
                    ).as_ref());
                }
            }

            "CUTCLIP" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("CUTCLIP");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let entities: Vec<_> = handles
                        .iter()
                        .filter_map(|&h| self.tabs[i].scene.document.get_entity(h).cloned())
                        .collect();
                    self.clipboard_base = super::super::helpers::entities_lower_left_by_bbox(
                        &self.tabs[i].scene.document,
                        &handles,
                    );
                    let count = entities.len();
                    self.clipboard = entities;
                    self.clipboard_deps = super::super::ClipboardDeps::capture(
                        &self.tabs[i].scene.document,
                        &self.clipboard,
                    );
                    self.push_undo_snapshot(i, "CUTCLIP");
                    self.tabs[i].scene.erase_entities(&handles);
                    self.tabs[i].scene.deselect_all();
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    self.command_line
                        .push_info(crate::tf!("{} object(s) cut to clipboard.", count).as_ref());
                }
            }

            "PASTECLIP" => {
                if self.clipboard.is_empty() {
                    return Some(self.read_system_clipboard_for_paste());
                } else {
                    let wires = self.tabs[i].scene.wires_for_entities(&self.clipboard);
                    let base = self.clipboard_base;
                    use crate::modules::draw::clipboard::paste::PasteCommand;
                    // The ghost anchor is a display-only offset; the precise
                    // paste delta is computed in f64 at commit time.
                    let cmd = PasteCommand::new(wires, base.as_vec3());
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            // PASTEORIG — paste at the entities' original coordinates (no pick).
            "PASTEORIG" => {
                if self.clipboard.is_empty() {
                    self.command_line
                        .push_error(crate::t!("PASTEORIG: clipboard is empty.").as_ref());
                } else {
                    let count = self.clipboard.len();
                    self.push_undo_snapshot(i, "PASTEORIG");
                    // No transform: entities keep their original coordinates.
                    let _ = self.finalize_paste(i, None);
                    self.tabs[i].dirty = true;
                    self.refresh_layer_panel();
                    self.refresh_properties();
                    self.command_line.push_output(crate::tf!(
                        "PASTEORIG: {} object(s) pasted at original coordinates.",
                        count
                    ).as_ref());
                }
            }

            // PASTEBLOCK — wrap the clipboard contents in a new block definition
            // and place one insert of it at the clipboard's original location.
            "PASTEBLOCK" => {
                if self.clipboard.is_empty() {
                    self.command_line
                        .push_error(crate::t!("PASTEBLOCK: clipboard is empty.").as_ref());
                } else {
                    self.push_undo_snapshot(i, "PASTEBLOCK");
                    self.merge_clipboard_deps(i);
                    // Recreate any block definition the clipboard's INSERTs
                    // reference, so nested blocks inside the new wrapper block
                    // don't render empty. (#135 / #158)
                    self.merge_clipboard_blocks(i);
                    // Recreate each entity's xdictionary graph (XCLIP filters)
                    // and stamp the new root onto the wrapped entity, so the
                    // block's nested insert keeps its clip. (#xclip-paste)
                    let ext_roots = self.recreate_clipboard_ext_roots(i);
                    let name = self.unique_block_name("Block");
                    let base = self.clipboard_base;
                    let mut entities = self.clipboard.clone();
                    for (&idx, &root) in &ext_roots {
                        if let Some(e) = entities.get_mut(idx) {
                            e.common_mut().xdictionary_handle = Some(root);
                        }
                    }
                    match self.tabs[i]
                        .scene
                        .define_block_from_owned_entities(entities, &name, base)
                    {
                        Ok(entity_handles) => {
                            let remaps: Vec<_> = ext_roots
                                .iter()
                                .filter_map(|(&idx, &root)| {
                                    Some((
                                        root,
                                        self.clipboard.get(idx)?.common().handle,
                                        *entity_handles.get(idx)?,
                                    ))
                                })
                                .collect();
                            let scene = &mut self.tabs[i].scene;
                            for (root, source, target) in remaps {
                                super::super::command_driver::remap_ext_subtree_reference(
                                    &mut scene.document,
                                    root,
                                    source,
                                    target,
                                );
                                crate::scene::annotative::translate_annotation_contexts(
                                    &mut scene.document,
                                    target,
                                    -base,
                                );
                            }
                            scene.bump_geometry();
                            // Block defined; now place it interactively so the
                            // user picks the drop point (insertion uses the
                            // clipboard lower-left corner as the block's base). The
                            // clipboard wires rubber-band under the cursor.
                            self.tabs[i].scene.populate_meshes_from_document();
                            self.tabs[i].dirty = true;
                            let wires = self.tabs[i].scene.wires_for_entities(&self.clipboard);
                            use crate::modules::insert::insert_block::InsertBlockCommand;
                            let cmd =
                                InsertBlockCommand::new_for_block(name, wires, base.as_vec3());
                            self.command_line.push_info(&cmd.prompt());
                            self.tabs[i].active_cmd = Some(Box::new(cmd));
                        }
                        Err(e) => self.command_line.push_error(crate::tf!("PASTEBLOCK: {e}").as_ref()),
                    }
                }
            }

            "BLOCK" | "BMAKE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("BLOCK");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::insert::create_block::CreateBlockCommand;
                    let cmd = CreateBlockCommand::new(handles);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            "INSERT" => {
                let blocks = self.tabs[i].scene.custom_block_names();
                if blocks.is_empty() {
                    self.command_line
                        .push_error(crate::t!("No user-defined blocks found in this drawing.").as_ref());
                } else {
                    use crate::modules::insert::insert_block::InsertBlockCommand;
                    let cmd = InsertBlockCommand::new(blocks);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            "MINSERT" => {
                let blocks = self.tabs[i].scene.custom_block_names();
                if blocks.is_empty() {
                    self.command_line
                        .push_error(crate::t!("No user-defined blocks found in this drawing.").as_ref());
                } else {
                    use crate::modules::insert::minsert::MinsertCommand;
                    let cmd = MinsertCommand::new(blocks);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            // ATTSYNC <block> — reconcile every insert of <block> against the
            // block's current attribute definitions: drop attributes whose tag
            // no longer exists and add any newly-defined ones (keeping the values
            // of attributes that remain).
            "ATTSYNC" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("ATTSYNC", "ATTSYNC  block name to sync:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ATTSYNC ") => {
                let arg = cmd.trim_start_matches("ATTSYNC").trim();
                let blocks = self.tabs[i].scene.custom_block_names();
                if arg.is_empty() {
                    self.command_line.push_info(crate::tf!(
                        "Usage: ATTSYNC <block name>.  Blocks: {}",
                        if blocks.is_empty() {
                            "(none)".to_string()
                        } else {
                            blocks.join(", ")
                        }
                    ).as_ref());
                    return Some(Task::none());
                }
                let Some(block) = blocks.iter().find(|b| b.eq_ignore_ascii_case(arg)).cloned()
                else {
                    self.command_line
                        .push_error(crate::tf!("ATTSYNC: no block named \"{arg}\".").as_ref());
                    return Some(Task::none());
                };
                // Gather the block's attribute definitions (tag, default value).
                let doc = &self.tabs[i].scene.document;
                let attdefs: Vec<(String, String)> = doc
                    .block_records
                    .get(&block)
                    .map(|br| {
                        br.entity_handles
                            .iter()
                            .filter_map(|h| doc.get_entity(*h))
                            .filter_map(|e| match e {
                                acadrust::EntityType::AttributeDefinition(a) => {
                                    Some((a.tag.clone(), a.default_value.clone()))
                                }
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.push_undo_snapshot(i, "ATTSYNC");
                let mut synced = 0usize;
                let mut changes = Vec::new();
                for e in self.tabs[i].scene.document.entities_mut() {
                    if let acadrust::EntityType::Insert(ins) = e {
                        if ins.block_name.eq_ignore_ascii_case(&block) {
                            ins.attributes.retain(|a| {
                                attdefs.iter().any(|(t, _)| t.eq_ignore_ascii_case(&a.tag))
                            });
                            for (tag, default) in &attdefs {
                                if !ins
                                    .attributes
                                    .iter()
                                    .any(|a| a.tag.eq_ignore_ascii_case(tag))
                                {
                                    ins.attributes
                                        .push(acadrust::entities::AttributeEntity::new(
                                            tag.clone(),
                                            default.clone(),
                                        ));
                                }
                            }
                            synced += 1;
                            changes.push((ins.common.handle, crate::scene::ChangeKind::Modified));
                        }
                    }
                }
                self.tabs[i].scene.bump_entities(&changes);
                self.tabs[i].dirty = true;
                self.command_line.push_output(crate::tf!(
                    "ATTSYNC: synchronised {synced} insert(s) of \"{block}\" against {} attribute definition(s).",
                    attdefs.len()
                ).as_ref());
            }

            // ADCENTER / CONTENTBROWSER — report the drawing's named content
            // (blocks and layers) from the command line in place of the browser
            // panel.
            "ADCENTER" | "CONTENTBROWSER" => {
                let blocks = self.tabs[i].scene.custom_block_names();
                let layers: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .names()
                    .map(|s| s.to_string())
                    .collect();
                self.command_line.push_output(crate::tf!(
                    "Blocks ({}): {}",
                    blocks.len(),
                    if blocks.is_empty() {
                        "(none)".to_string()
                    } else {
                        blocks.join(", ")
                    }
                ).as_ref());
                self.command_line.push_output(crate::tf!(
                    "Layers ({}): {}",
                    layers.len(),
                    layers.join(", ")
                ).as_ref());
            }

            // BLOCKPALETTE / BLOCKSPALETTE — toggle the docked Insert Block panel.
            "BLOCKPALETTE" | "BLOCKSPALETTE" => {
                self.show_block_palette ^= true;
                if self.show_block_palette {
                    // Always open expanded so the panel is immediately usable;
                    // the user can still collapse it via the title-bar button.
                    self.block_palette_expanded = true;
                    self.refresh_block_palette();
                }
            }

            // ATTMAN / BATTMAN — the Block Attribute Manager. Rather than a
            // command-line listing, both route to the attribute editor
            // (ATTEDIT): edit the selected block's attributes, or pick a block.
            cmd if cmd == "ATTMAN"
                || cmd == "BATTMAN"
                || cmd.starts_with("ATTMAN ")
                || cmd.starts_with("BATTMAN ") =>
            {
                self.open_attedit_dialog();
            }

            "XATTACH" => {
                // Launch the file picker; XAttachPickResult will start the command.
                return Some(Task::done(Message::XAttachPick));
            }

            cmd if cmd == "WBLOCK" || cmd == "WB" || cmd.starts_with("WBLOCK ") => {
                let arg = cmd.splitn(2, ' ').nth(1).unwrap_or("").trim();
                if arg.is_empty() {
                    // No argument: use selected entities (*) if any, else ask.
                    let sel: Vec<_> = self.tabs[i].scene.selected.iter().copied().collect();
                    if sel.is_empty() {
                        self.command_line.push_error(
                            "WBLOCK  Select entities first, or: WBLOCK <block name>  or  WBLOCK *",
                        );
                    } else {
                        return Some(Task::done(Message::WblockSave("*".to_string())));
                    }
                } else {
                    return Some(Task::done(Message::WblockSave(arg.to_string())));
                }
            }

            "XREF" => {
                // List all xref blocks in the current drawing.
                let xrefs: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .iter()
                    .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                    .map(|br| {
                        format!(
                            "  {} — {}",
                            br.name,
                            if br.xref_path.is_empty() {
                                "(no path)".to_string()
                            } else {
                                br.xref_path.clone()
                            }
                        )
                    })
                    .collect();
                if xrefs.is_empty() {
                    self.command_line
                        .push_output(crate::t!("XREF  No external references in this drawing.").as_ref());
                } else {
                    self.command_line.push_output(crate::t!("XREF  External references:").as_ref());
                    for line in xrefs {
                        self.command_line.push_output(&line);
                    }
                }
            }

            "XRELOAD" => {
                // Reload all xrefs for the current drawing.
                if let Some(path) = &self.tabs[i].current_path.clone() {
                    if let Some(base_dir) = path.parent() {
                        let (infos, _dropped) = crate::io::xref::resolve_xrefs(
                            &mut self.tabs[i].scene.document,
                            base_dir,
                        );
                        for info in &infos {
                            match info.status {
                                crate::io::xref::XrefStatus::Loaded => {
                                    self.command_line
                                        .push_output(crate::tf!("XREF  Reloaded \"{}\"", info.name).as_ref());
                                }
                                crate::io::xref::XrefStatus::Recovered => {
                                    self.command_line.push_error(crate::tf!(
                                        "XREF  Reloaded with repairs: \"{}\"",
                                        info.name
                                    ).as_ref());
                                }
                                crate::io::xref::XrefStatus::NotFound => {
                                    self.command_line.push_error(crate::tf!(
                                        "XREF  Not found: \"{}\" ({})",
                                        info.name, info.path
                                    ).as_ref());
                                }
                                crate::io::xref::XrefStatus::Failed => {
                                    self.command_line.push_error(crate::tf!(
                                        "XREF  Reload failed: \"{}\" ({})",
                                        info.name, info.path
                                    ).as_ref());
                                }
                                crate::io::xref::XrefStatus::Unloaded => {
                                    self.command_line.push_info(crate::tf!(
                                        "XREF  Unloaded (skipped): \"{}\"",
                                        info.name
                                    ).as_ref());
                                }
                            }
                        }
                        self.tabs[i].scene.populate_hatches_from_document();
                        self.tabs[i].scene.populate_images_from_document();
                        self.tabs[i].scene.populate_meshes_from_document();
                        // The reload may have merged new xref layers /
                        // linetypes — mirror them into the Layers panel and
                        // ribbon dropdowns (#407).
                        self.refresh_layer_panel();
                    }
                } else {
                    self.command_line
                        .push_error(crate::t!("XREF  Save the drawing first to resolve relative XREF paths.").as_ref());
                }
            }

            // XOPEN — open the source file of the selected external reference in
            // a new tab (reuses the existence-checked OpenRecent path).
            "XOPEN" => {
                let names: Vec<String> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter_map(|(_, e)| match e {
                        acadrust::EntityType::Insert(ins) => Some(ins.block_name.clone()),
                        _ => None,
                    })
                    .collect();
                let path = names.iter().find_map(|bn| {
                    let br = self.tabs[i].scene.document.block_records.get(bn)?;
                    if br.xref_path.is_empty() {
                        None
                    } else {
                        Some(br.xref_path.clone())
                    }
                });
                match path {
                    Some(p) => {
                        return Some(Task::done(Message::OpenRecent(std::path::PathBuf::from(p))))
                    }
                    None => self
                        .command_line
                        .push_error(crate::t!("XOPEN: select an external reference (xref) to open.").as_ref()),
                }
            }

            // NCOPY — copy the nested objects of the selected block reference(s)
            // into model space, keeping the block (a non-destructive extraction;
            // every nested object is copied — the block is not exploded).
            "NCOPY" | "NCOPYALL" => {
                use crate::modules::draw::modify::explode::explode_entity;
                let inserts: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter(|(_, e)| matches!(e, acadrust::EntityType::Insert(_)))
                    .map(|(h, _)| *h)
                    .collect();
                if inserts.is_empty() {
                    self.command_line
                        .push_error(crate::t!("NCOPY: select a block reference first.").as_ref());
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "NCOPY");
                let mut n = 0usize;
                for h in &inserts {
                    let nested = match self.tabs[i].scene.document.get_entity(*h).cloned() {
                        Some(ins) => explode_entity(&ins, &self.tabs[i].scene.document),
                        None => Vec::new(),
                    };
                    for e in nested {
                        self.tabs[i].scene.add_entity(e);
                        n += 1;
                    }
                }
                self.tabs[i].dirty = true;
                self.command_line.push_output(crate::tf!(
                    "NCOPY: copied {n} nested object(s) into model space (blocks kept)."
                ).as_ref());
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;

    fn fresh_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// Run one command line and return only the command-line text it appended.
    fn run_capture(app: &mut OpenCADStudio, cmd: &str) -> String {
        let start = app.command_line.history.len();
        let _ = app.run_command_line(cmd);
        app.command_line.history[start..]
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ATTMAN and BATTMAN are the Block Attribute Manager; this build routes both
    // to the attribute editor (ATTEDIT) instead of the old command-line listing.
    // With nothing selected they start the same interactive pick as ATTEDIT,
    // byte for byte, and never emit the old "no matching block" listing text.
    #[test]
    fn attman_and_battman_route_to_attedit() {
        let attedit = run_capture(&mut fresh_app(), "ATTEDIT");
        let attman = run_capture(&mut fresh_app(), "ATTMAN");
        let battman = run_capture(&mut fresh_app(), "BATTMAN");

        assert_eq!(
            attman, attedit,
            "ATTMAN must behave like ATTEDIT, got: {attman:?} vs {attedit:?}"
        );
        assert_eq!(
            battman, attedit,
            "BATTMAN must behave like ATTEDIT, got: {battman:?} vs {attedit:?}"
        );
        assert!(
            !attman.contains("no matching block"),
            "ATTMAN must not run the old command-line listing, got: {attman:?}"
        );
    }

    #[test]
    fn blockpalette_toggles_panel() {
        let mut app = fresh_app();
        assert!(!app.show_block_palette);
        let _ = app.run_command_line("BLOCKPALETTE");
        assert!(app.show_block_palette);
        assert!(
            app.block_palette_expanded,
            "palette must open expanded, not as the collapsed bar"
        );
        let _ = app.run_command_line("BLOCKSPALETTE");
        assert!(!app.show_block_palette);
    }

    #[test]
    fn blockpalette_refresh_lists_blocks() {
        let mut app = fresh_app();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut line = acadrust::entities::Line::new();
        line.start = acadrust::types::Vector3::ZERO;
        line.end = acadrust::types::Vector3::new(10.0, 0.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(
                vec![acadrust::EntityType::Line(line)],
                "Widget",
                glam::DVec3::ZERO,
            )
            .unwrap();
        let _ = app.run_command_line("BLOCKPALETTE");
        assert!(app.show_block_palette);
        assert_eq!(app.block_palette.cached_names, vec!["Widget"]);
        assert_eq!(app.block_palette.blocks.len(), 1);
    }

    #[test]
    fn blockpalette_refresh_if_stale() {
        let mut app = fresh_app();
        app.automation_op(r#"{"op":"new"}"#);
        app.show_block_palette = true;
        app.refresh_block_palette();
        assert!(app.block_palette.blocks.is_empty());
        let i = app.active_tab;
        let mut line = acadrust::entities::Line::new();
        line.start = acadrust::types::Vector3::ZERO;
        line.end = acadrust::types::Vector3::new(10.0, 0.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(
                vec![acadrust::EntityType::Line(line)],
                "Widget",
                glam::DVec3::ZERO,
            )
            .unwrap();
        app.refresh_block_palette_if_stale();
        assert!(app.block_palette.blocks.iter().any(|b| b.name == "Widget"));
    }

    #[test]
    fn blockpalette_refreshes_when_switching_to_same_named_block_in_another_tab() {
        let mut app = fresh_app();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut line = acadrust::entities::Line::new();
        line.end = acadrust::types::Vector3::new(10.0, 0.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(
                vec![acadrust::EntityType::Line(line)],
                "Widget",
                glam::DVec3::ZERO,
            )
            .unwrap();
        app.show_block_palette = true;
        app.refresh_block_palette();
        let first_tab_id = app.tabs[i].id;

        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(99));
        let other = app.tabs.len() - 1;
        let mut line = acadrust::entities::Line::new();
        line.end = acadrust::types::Vector3::new(50.0, 0.0, 0.0);
        app.tabs[other]
            .scene
            .define_block_from_owned_entities(
                vec![acadrust::EntityType::Line(line)],
                "Widget",
                glam::DVec3::ZERO,
            )
            .unwrap();
        app.active_tab = other;
        app.refresh_block_palette_if_stale();

        assert_ne!(app.tabs[other].id, first_tab_id);
        assert_eq!(app.block_palette.source_tab_id, Some(app.tabs[other].id));
        assert_eq!(
            app.block_palette.source_block_epoch,
            app.tabs[other].scene.block_epoch
        );
    }
}
