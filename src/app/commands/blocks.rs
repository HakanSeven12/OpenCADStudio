use super::*;

impl OpenCADStudio {
    /// Shared per-reference status reporting for XRELOAD and XREF Reload.
    /// Extracted verbatim from the XRELOAD arm — no behavior change.
    pub(in crate::app) fn report_xref_status(&mut self, info: &crate::io::xref::XrefInfo) {
        match info.status {
            crate::io::xref::XrefStatus::Loaded => {
                self.command_line
                    .push_output(crate::tf!("XREF  Reloaded \"{}\"", info.name).as_ref());
            }
            crate::io::xref::XrefStatus::Recovered => {
                self.command_line.push_error(
                    crate::tf!("XREF  Reloaded with repairs: \"{}\"", info.name).as_ref(),
                );
            }
            crate::io::xref::XrefStatus::NotFound => {
                self.command_line.push_error(
                    crate::tf!("XREF  Not found: \"{}\" ({})", info.name, info.path).as_ref(),
                );
            }
            crate::io::xref::XrefStatus::Failed => {
                self.command_line.push_error(
                    crate::tf!("XREF  Reload failed: \"{}\" ({})", info.name, info.path).as_ref(),
                );
            }
            crate::io::xref::XrefStatus::Unloaded => {
                self.command_line
                    .push_info(crate::tf!("XREF  Unloaded (skipped): \"{}\"", info.name).as_ref());
            }
        }
    }

    pub(in crate::app) fn copy_entities_to_clipboard(
        &mut self,
        i: usize,
        handles: &[acadrust::Handle],
        base: glam::DVec3,
    ) -> usize {
        let (entities, deps) = {
            let document = &self.tabs[i].scene.document;
            let entities: Vec<_> = handles
                .iter()
                .filter_map(|&handle| document.get_entity(handle).cloned())
                .collect();
            let deps = super::super::ClipboardDeps::capture(document, &entities);
            (entities, deps)
        };
        let count = entities.len();
        self.clipboard_base = base;
        self.clipboard = entities;
        self.clipboard_deps = deps;
        count
    }

    pub(in crate::app) fn commit_block_definition(&mut self, redefine: bool) -> Task<Message> {
        let Some(state) = self.block_definition.take() else {
            self.active_modal = None;
            return Task::none();
        };
        self.active_modal = None;
        let i = self.active_tab;
        let base_point = state.parse_base_point();
        let ucs = self.tabs[i].ucs_xform();
        let world_to_block = ucs.to_ucs_transform_at(base_point);
        let block_to_world = ucs.to_wcs_transform_at(base_point);
        let options = crate::scene::CreateBlockOptions {
            name: state.name.trim().to_string(),
            handles: state.selected_handles,
            base_point,
            world_to_block,
            block_to_world,
            mode: state.object_mode,
            annotative: state.annotative,
            match_orientation: state.match_orientation,
            scale_uniformly: state.scale_uniformly,
            allow_exploding: state.allow_exploding,
            unit: state.unit,
            description: state.description.trim().to_string(),
            hyperlink_url: state.hyperlink_url.trim().to_string(),
            hyperlink_desc: state.hyperlink_desc.trim().to_string(),
            redefine,
        };

        if state.base_point_specify_onscreen || state.objects_specify_onscreen {
            let cmd = crate::modules::insert::create_block::BlockOnScreenCommand::new(
                options,
                ucs,
                state.base_point_specify_onscreen,
                state.objects_specify_onscreen,
            );
            self.command_line.push_info(&cmd.prompt());
            self.tabs[i].active_cmd = Some(Box::new(cmd));
            Task::none()
        } else {
            self.apply_cmd_result(crate::command::CmdResult::CreateBlockWithOptions {
                options: Box::new(options),
            })
        }
    }

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
                            self.tabs[i].scene.persist_current_layout_state();
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
                        self.command_line.push_output(
                            crate::tf!(
                                "Base point ({}, {}, {}) set for {space}.",
                                nums[0],
                                nums[1],
                                z
                            )
                            .as_ref(),
                        );
                    } else {
                        self.command_line
                            .push_error(crate::t!("Usage: BASE <x> <y> [z]").as_ref());
                    }
                }
            }

            "COPYCLIP" => {
                // MText editor open: Ctrl+C copies its selected text, not the
                // drawing's entities.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    return match self.mtext_selected_text() {
                        Some(text) => {
                            self.command_line.push_info(
                                crate::t!("Copied selected text to clipboard.").as_ref(),
                            );
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
                    let base = super::super::helpers::entities_lower_left_by_bbox(
                        &self.tabs[i].scene.document,
                        &handles,
                    );
                    let count = self.copy_entities_to_clipboard(i, &handles, base);
                    self.command_line
                        .push_info(crate::tf!("{} object(s) copied to clipboard.", count).as_ref());
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
                    let count = self.copy_entities_to_clipboard(i, &handles, base);
                    self.command_line.push_info(
                        crate::tf!(
                            "{} object(s) copied to clipboard (base {:.3},{:.3}).",
                            count,
                            base.x,
                            base.y
                        )
                        .as_ref(),
                    );
                }
            }

            "CUTCLIP" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("CUTCLIP");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let base = super::super::helpers::entities_lower_left_by_bbox(
                        &self.tabs[i].scene.document,
                        &handles,
                    );
                    let count = self.copy_entities_to_clipboard(i, &handles, base);
                    self.push_undo_snapshot(i, "CUTCLIP");
                    self.tabs[i].scene.erase_entities(&handles);
                    self.tabs[i].scene.deselect_all();
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    self.command_line
                        .push_info(crate::tf!("{} object(s) cut to clipboard.", count).as_ref());
                }
            }

            "PASTE" | "PASTECLIP" => {
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
                    self.command_line.push_output(
                        crate::tf!(
                            "PASTEORIG: {} object(s) pasted at original coordinates.",
                            count
                        )
                        .as_ref(),
                    );
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
                        Err(e) => self
                            .command_line
                            .push_error(crate::tf!("PASTEBLOCK: {e}").as_ref()),
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
                let existing_names = self.tabs[i].scene.custom_block_names();
                let default_unit = self.tabs[i].scene.document.header.insertion_units;
                self.block_definition = Some(
                    crate::ui::window::block_definition::BlockDefinitionState::new(
                        existing_names,
                        handles,
                        default_unit,
                    ),
                );
                self.active_modal = Some(crate::app::ModalKind::BlockDefinition);
            }

            "-BLOCK" | "-BMAKE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("-BLOCK");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::insert::create_block::CreateBlockCommand;
                    let cmd = CreateBlockCommand::new(handles);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            cmd if cmd.starts_with("BLOCK_POINT_PICKED ") => {
                let parts: Vec<f64> = cmd["BLOCK_POINT_PICKED ".len()..]
                    .split_whitespace()
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();
                if parts.len() == 3 {
                    if let Some(state) = self.block_definition.as_mut() {
                        state.base_point_x = format!("{:.4}", parts[0]);
                        state.base_point_y = format!("{:.4}", parts[1]);
                        state.base_point_z = format!("{:.4}", parts[2]);
                    }
                }
                self.tabs[i].active_cmd = None;
                self.active_modal = Some(crate::app::ModalKind::BlockDefinition);
            }

            "BLOCK_POINT_CANCELLED" => {
                self.tabs[i].active_cmd = None;
                self.active_modal = Some(crate::app::ModalKind::BlockDefinition);
            }

            "BLOCK_OBJECTS_GATHERED" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if let Some(state) = self.block_definition.as_mut() {
                    state.selected_handles = handles;
                }
                self.tabs[i].active_cmd = None;
                self.active_modal = Some(crate::app::ModalKind::BlockDefinition);
            }

            "INSERT" => {
                let blocks = self.tabs[i].scene.custom_block_names();
                if blocks.is_empty() {
                    self.command_line.push_error(
                        crate::t!("No user-defined blocks found in this drawing.").as_ref(),
                    );
                } else {
                    use crate::modules::insert::insert_block::InsertBlockCommand;
                    let ranked = self.ranked_block_names(&blocks);
                    let snapshot = self.block_usage_snapshot();
                    let cmd = InsertBlockCommand::new_with_usage(
                        ranked,
                        snapshot,
                        self.cliprompt_lines.clamp(0, 50) as u8,
                    );
                    self.command_line.push_info(&cmd.prompt());
                    let opts = cmd.options();
                    self.command_line.set_step_options(opts.clone());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            "MINSERT" => {
                let blocks = self.tabs[i].scene.custom_block_names();
                if blocks.is_empty() {
                    self.command_line.push_error(
                        crate::t!("No user-defined blocks found in this drawing.").as_ref(),
                    );
                } else {
                    use crate::modules::insert::minsert::MinsertCommand;
                    let ranked = self.ranked_block_names(&blocks);
                    let snapshot = self.block_usage_snapshot();
                    let cmd = MinsertCommand::new_with_usage(
                        ranked,
                        snapshot,
                        self.cliprompt_lines.clamp(0, 50) as u8,
                    );
                    self.command_line.push_info(&cmd.prompt());
                    let opts = cmd.options();
                    self.command_line.set_step_options(opts.clone());
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
                    self.command_line.push_info(
                        crate::tf!(
                            "Usage: ATTSYNC <block name>.  Blocks: {}",
                            if blocks.is_empty() {
                                "(none)".to_string()
                            } else {
                                blocks.join(", ")
                            }
                        )
                        .as_ref(),
                    );
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
                let inserts: Vec<_> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter_map(|entity| match entity {
                        acadrust::EntityType::Insert(insert)
                            if insert.block_name.eq_ignore_ascii_case(&block)
                                && !self.tabs[i].scene.is_layer_locked(insert.common.handle) =>
                        {
                            Some(insert.common.handle)
                        }
                        _ => None,
                    })
                    .collect();
                if inserts.is_empty() {
                    self.command_line
                        .push_error(crate::t!("ATTSYNC: no editable block references.").as_ref());
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "ATTSYNC");
                let mut synced = 0usize;
                let mut changes = Vec::new();
                for handle in inserts {
                    let Some(acadrust::EntityType::Insert(ins)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    else {
                        continue;
                    };
                    ins.attributes
                        .retain(|a| attdefs.iter().any(|(t, _)| t.eq_ignore_ascii_case(&a.tag)));
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
                self.command_line.push_output(
                    crate::tf!(
                        "Blocks ({}): {}",
                        blocks.len(),
                        if blocks.is_empty() {
                            "(none)".to_string()
                        } else {
                            blocks.join(", ")
                        }
                    )
                    .as_ref(),
                );
                self.command_line.push_output(
                    crate::tf!("Layers ({}): {}", layers.len(), layers.join(", ")).as_ref(),
                );
            }

            // BLOCKPALETTE / BLOCKSPALETTE — toggle the docked Insert Block panel.
            "BLOCKPALETTE" | "BLOCKSPALETTE" => {
                self.show_block_palette ^= true;
                if self.show_block_palette {
                    // Always open expanded so the panel is immediately usable;
                    // the user can still collapse it via the pin (Auto) button.
                    self.dock_expanded = Some(crate::ui::dock::PanelId::BlockPalette);
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

            "PDFATTACH" => {
                return Some(Task::done(Message::PdfAttachPick));
            }
            cmd if cmd == "WBLOCK" || cmd == "WB" || cmd.starts_with("WBLOCK ") => {
                let arg = cmd.splitn(2, ' ').nth(1).unwrap_or("").trim();
                if arg.is_empty() {
                    // No argument: use selected entities (*) if any, else ask.
                    let sel: Vec<_> = self.tabs[i].scene.selected.iter().copied().collect();
                    if sel.is_empty() {
                        self.command_line.push_error(
                            crate::t!("WBLOCK  Select entities first, or: WBLOCK <block name>  or  WBLOCK *").as_ref(),
                        );
                    } else {
                        return Some(Task::done(Message::WblockSave("*".to_string())));
                    }
                } else {
                    return Some(Task::done(Message::WblockSave(arg.to_string())));
                }
            }

            // EXTERNALREFERENCES — toggle the docked External References panel.
            // (XREFMAN was a provisional name and is not kept, not even as an
            // alias.) The XREF command below is untouched.
            "EXTERNALREFERENCES" => {
                self.show_external_references ^= true;
                if self.show_external_references {
                    // Always open expanded so the panel is immediately usable;
                    // dock it on the right if the user closed it from the layout.
                    if self
                        .dock
                        .location(crate::ui::dock::PanelId::ExternalReferences)
                        .is_none()
                    {
                        self.dock.dock(
                            crate::ui::dock::PanelId::ExternalReferences,
                            crate::app::config::DockSide::Right,
                            usize::MAX,
                        );
                    }
                    self.dock_expanded = Some(crate::ui::dock::PanelId::ExternalReferences);
                    self.refresh_xref_manager();
                }
            }

            cmd if cmd.eq_ignore_ascii_case("XREF")
                || cmd.to_ascii_uppercase().starts_with("XREF ")
                || cmd.eq_ignore_ascii_case("-XREF")
                || cmd.to_ascii_uppercase().starts_with("-XREF ") =>
            {
                // XREF sub-option dispatcher (Task 5). The verb is
                // case-insensitive; arguments split on whitespace.
                // `-XREF` is accepted as an alias for industry muscle memory
                // (modern CAD opens the palette on `XREF`); bare `XREF` keeps
                // this repo's legacy list output for compatibility.
                let rest = cmd
                    .splitn(2, char::is_whitespace)
                    .nth(1)
                    .unwrap_or("")
                    .trim();
                let mut parts = rest.split_whitespace();
                let op = parts.next().unwrap_or("");
                #[cfg(target_arch = "wasm32")]
                if !op.is_empty() && op != "?" {
                    self.command_line.push_error(crate::t!(
                        "Reference changes are not available on web — the reference list is read-only."
                    ).as_ref());
                    return Some(self.finish_dispatch(cmd));
                }
                if op.is_empty() || op == "?" {
                    // List every reference via collect_entries_with_prev. The
                    // base dir is the drawing's parent (or "." unsaved); the
                    // tab's session sets keep CLI and palette in agreement.
                    // Header lines stay byte-identical to the legacy listing;
                    // each entry line gains an " [Attach]" / " [Overlay]"
                    // type suffix.
                    let base_dir: std::path::PathBuf = self.tabs[i]
                        .current_path
                        .as_ref()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let entries = crate::io::xref::collect_entries_with_prev(
                        &self.tabs[i].scene.document,
                        &base_dir,
                        self.tabs[i].xref_unloaded.as_set(),
                        &self.tabs[i].xref_stat_cache.0,
                    );
                    if entries.is_empty() {
                        self.command_line.push_output(
                            crate::t!("XREF  No external references in this drawing.").as_ref(),
                        );
                    } else {
                        self.command_line
                            .push_output(crate::t!("XREF  External references:").as_ref());
                        for entry in &entries {
                            let path = if entry.saved_path.is_empty() {
                                "(no path)".to_string()
                            } else {
                                entry.saved_path.clone()
                            };
                            let suffix = match entry.ref_type {
                                crate::io::xref_model::RefType::Overlay => " [Overlay]",
                                crate::io::xref_model::RefType::Attach => " [Attach]",
                            };
                            self.command_line
                                .push_output(&format!("  {} — {}{}", entry.name, path, suffix));
                        }
                    }
                } else if op.eq_ignore_ascii_case("Reload") {
                    // Reload all (no pattern) or the subset matching
                    // <pattern> (case-insensitive, `*`/`?` wildcards).
                    let pattern = parts.next().unwrap_or("").trim();
                    // A concrete pattern that matches no entry resolves
                    // nothing: pre-check the entry list (same base_dir +
                    // session sets as the list path) and return early
                    // without touching resolve_xrefs — no mutation, no
                    // populate/refresh. Bare Reload and `*` keep the
                    // resolve-everything path below.
                    if !pattern.is_empty() && pattern != "*" {
                        let base_dir: std::path::PathBuf = self.tabs[i]
                            .current_path
                            .as_ref()
                            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                            .unwrap_or_else(|| std::path::PathBuf::from("."));
                        let entries = crate::io::xref::collect_entries_with_prev(
                            &self.tabs[i].scene.document,
                            &base_dir,
                            self.tabs[i].xref_unloaded.as_set(),
                            &self.tabs[i].xref_stat_cache.0,
                        );
                        let any = entries
                            .iter()
                            .any(|e| crate::io::xref_model::wildcard_match(&e.name, pattern));
                        if !any {
                            self.command_line.push_error(
                                crate::tf!("XREF: no references match '{}'.", pattern).as_ref(),
                            );
                            return Some(self.finish_dispatch(cmd));
                        }
                    }
                    if let Some(path) = &self.tabs[i].current_path.clone() {
                        if let Some(base_dir) = path.parent() {
                            // Reloaded keys leave the session-unloaded set
                            // (so the listing agrees) with fresh stat
                            // baselines on the next palette refresh.
                            let reload_keys: Vec<u64> = {
                                let entries = crate::io::xref::collect_entries_with_prev(
                                    &self.tabs[i].scene.document,
                                    base_dir,
                                    self.tabs[i].xref_unloaded.as_set(),
                                    &self.tabs[i].xref_stat_cache.0,
                                );
                                // Drawing references only (mirrors the palette
                                // Reload): image/PDF rows keep their flags
                                // untouched and nested rows report per-entry
                                // instead of clearing state that resolve
                                // cannot rebuild.
                                let mut keys = Vec::new();
                                for e in &entries {
                                    let hit = pattern.is_empty()
                                        || pattern == "*"
                                        || crate::io::xref_model::wildcard_match(&e.name, pattern);
                                    if !hit {
                                        continue;
                                    }
                                    if e.parent_key.is_some() {
                                        self.command_line.push_error(crate::tf!(
                                            "XREF: cannot reload nested reference '{}'. Reload it in its host drawing.",
                                            e.name
                                        ).as_ref());
                                    } else if e.kind != crate::io::xref_model::RefKind::DwgXref {
                                        self.command_line.push_error(
                                            crate::tf!(
                                                "{}: reload applies to drawing references only.",
                                                e.name
                                            )
                                            .as_ref(),
                                        );
                                    } else {
                                        keys.push(e.key);
                                    }
                                }
                                keys
                            };
                            // Nothing reloadable (all pattern hits were
                            // nested/image rows, already reported above):
                            // return before snapshot + resolve so no empty
                            // undo entry is pushed and no spurious
                            // no-match error follows the per-entry reports.
                            if reload_keys.is_empty() && !pattern.is_empty() && pattern != "*" {
                                return Some(self.finish_dispatch(cmd));
                            }
                            self.push_undo_snapshot(i, "XREF-RELOAD");
                            for key in &reload_keys {
                                self.tabs[i].xref_unloaded.remove(key);
                                self.tabs[i].xref_stat_cache.remove(key);
                            }
                            let handles: rustc_hash::FxHashSet<acadrust::types::Handle> = self.tabs
                                [i]
                                .scene
                                .document
                                .block_records
                                .iter()
                                .filter(|br| reload_keys.contains(&br.handle.value()))
                                .map(|br| br.handle)
                                .collect();
                            let (infos, _dropped) = crate::io::xref::resolve_xrefs_for_keys(
                                &mut self.tabs[i].scene.document,
                                base_dir,
                                &handles,
                            );
                            // Targeted reload updates only the selected rows.
                            {
                                let fresh = crate::io::xref::collect_entries_with_prev(
                                    &self.tabs[i].scene.document,
                                    base_dir,
                                    self.tabs[i].xref_unloaded.as_set(),
                                    &self.tabs[i].xref_stat_cache.0,
                                );
                                for e in &fresh {
                                    if !reload_keys.contains(&e.key) {
                                        continue;
                                    }
                                    if e.status == crate::io::xref_model::RefStatus::Loaded {
                                        if let Some(m) = e.modified {
                                            self.tabs[i].xref_stat_cache.insert(e.key, m);
                                        }
                                    }
                                }
                            }
                            let matched: Vec<&crate::io::xref::XrefInfo> = if pattern.is_empty() {
                                infos.iter().collect()
                            } else {
                                infos
                                    .iter()
                                    .filter(|n| {
                                        crate::io::xref_model::wildcard_match(&n.name, pattern)
                                    })
                                    .collect()
                            };
                            if matched.is_empty() {
                                self.command_line.push_error(
                                    crate::tf!("XREF: no references match '{}'.", pattern).as_ref(),
                                );
                            } else {
                                for info in matched {
                                    self.report_xref_status(info);
                                }
                            }
                            self.post_ref_op(i);
                        }
                    } else {
                        self.command_line.push_error(
                            crate::t!(
                                "XREF  Save the drawing first to resolve relative XREF paths."
                            )
                            .as_ref(),
                        );
                    }
                } else if op.eq_ignore_ascii_case("Unload") {
                    // Unload <pattern> (empty/* = all): drop merged content
                    // per reference AND record the key session-unloaded, so
                    // listings (CLI + palette) agree until a Reload clears it.
                    let pattern = parts.next().unwrap_or("").trim();
                    let base_dir: std::path::PathBuf = self.tabs[i]
                        .current_path
                        .as_ref()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let entries = crate::io::xref::collect_entries_with_prev(
                        &self.tabs[i].scene.document,
                        &base_dir,
                        self.tabs[i].xref_unloaded.as_set(),
                        &self.tabs[i].xref_stat_cache.0,
                    );
                    let matched: Vec<_> = entries
                        .iter()
                        .filter(|e| {
                            pattern.is_empty()
                                || pattern == "*"
                                || crate::io::xref_model::wildcard_match(&e.name, pattern)
                        })
                        .collect();
                    if matched.is_empty() {
                        self.command_line.push_error(
                            crate::tf!("XREF: no references match '{}'.", pattern).as_ref(),
                        );
                    } else {
                        self.push_undo_snapshot(i, "XREF-UNLOAD");
                        let mut done = 0usize;
                        for e in matched {
                            if e.parent_key.is_some() {
                                self.command_line.push_error(crate::tf!(
                                    "XREF: cannot unload nested reference '{}'. Unload it in its host drawing.",
                                    e.name
                                ).as_ref());
                                continue;
                            }
                            match crate::io::xref::unload_reference(
                                &mut self.tabs[i].scene.document,
                                e.key,
                            ) {
                                Ok(name) => {
                                    self.command_line.push_output(
                                        crate::tf!("XREF: unloaded \"{}\".", name).as_ref(),
                                    );
                                    self.tabs[i].xref_unloaded.add(e.key);
                                    done += 1;
                                }
                                Err(msg) => self.command_line.push_error(msg.as_str()),
                            }
                        }
                        if done > 0 {
                            self.post_ref_op(i);
                        }
                    }
                } else if op.eq_ignore_ascii_case("Detach") {
                    // Detach <pattern> (empty/* = all). Nested entries are
                    // rejected per-entry; direct ones erase instances +
                    // definition + `name|*` dependents.
                    let pattern = parts.next().unwrap_or("").trim();
                    let base_dir: std::path::PathBuf = self.tabs[i]
                        .current_path
                        .as_ref()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let entries = crate::io::xref::collect_entries_with_prev(
                        &self.tabs[i].scene.document,
                        &base_dir,
                        self.tabs[i].xref_unloaded.as_set(),
                        &self.tabs[i].xref_stat_cache.0,
                    );
                    let matched: Vec<_> = entries
                        .iter()
                        .filter(|e| {
                            pattern.is_empty()
                                || pattern == "*"
                                || crate::io::xref_model::wildcard_match(&e.name, pattern)
                        })
                        .collect();
                    if matched.is_empty() {
                        self.command_line.push_error(
                            crate::tf!("XREF: no references match '{}'.", pattern).as_ref(),
                        );
                    } else {
                        self.push_undo_snapshot(i, "XREF-DETACH");
                        let mut done = 0usize;
                        for e in matched {
                            if e.parent_key.is_some() {
                                self.command_line.push_error(crate::tf!(
                                    "XREF: cannot detach nested reference '{}'. Detach it in its host drawing.",
                                    e.name
                                ).as_ref());
                                continue;
                            }
                            match crate::io::xref::detach_reference(
                                &mut self.tabs[i].scene.document,
                                e.key,
                            ) {
                                Ok(name) => {
                                    self.command_line.push_output(
                                        crate::tf!("XREF: detached \"{}\".", name).as_ref(),
                                    );
                                    self.tabs[i].xref_unloaded.remove(&e.key);
                                    self.tabs[i].xref_stat_cache.remove(&e.key);
                                    done += 1;
                                }
                                Err(msg) => self.command_line.push_error(msg.as_str()),
                            }
                        }
                        if done > 0 {
                            self.post_ref_op(i);
                        }
                    }
                } else if op.eq_ignore_ascii_case("Path") {
                    // Path Find <old> <new> → prefix rewrite across all;
                    // Path <pattern> <new path> → verbatim per-ref store.
                    let after_op = rest.get(op.len()..).unwrap_or("").trim();
                    if after_op
                        .split_whitespace()
                        .next()
                        .is_some_and(|w| w.eq_ignore_ascii_case("Find"))
                    {
                        // Find & Replace: tokens are Find <old> <new...>
                        // (`new` keeps spaces via remainder slice).
                        let toks: Vec<&str> = after_op.split_whitespace().collect();
                        let (old, new) = if toks.len() >= 3 {
                            // Find the exact token index for old to avoid substring
                            // contamination (e.g. "in" matching inside "Find").
                            if let Some(idx) =
                                toks.iter().position(|t| t.eq_ignore_ascii_case(&toks[1]))
                            {
                                let pos = after_op.find(toks[idx]).unwrap_or(0) + toks[idx].len();
                                (toks[idx].to_string(), after_op[pos..].trim().to_string())
                            } else {
                                (String::new(), String::new())
                            }
                        } else {
                            (String::new(), String::new())
                        };
                        if old.is_empty() || new.is_empty() {
                            self.command_line.push_error(
                                crate::t!("Usage: XREF Path Find <old> <new>").as_ref(),
                            );
                        } else {
                            self.push_undo_snapshot(i, "XREF-PATH");
                            let n = crate::io::xref::replace_path_prefix(
                                &mut self.tabs[i].scene.document,
                                &old,
                                &new,
                            );
                            self.command_line.push_output(
                                crate::tf!("XREF: updated {} reference(s).", n).as_ref(),
                            );
                            if n > 0 {
                                self.post_ref_op(i);
                            }
                        }
                    } else {
                        let pattern = parts.next().unwrap_or("").trim().to_string();
                        // New path keeps spaces: slice after pattern.
                        let new_raw: String = if pattern.is_empty() {
                            String::new()
                        } else {
                            after_op
                                .find(&pattern)
                                .map(|p| after_op[p + pattern.len()..].trim().to_string())
                                .unwrap_or_default()
                        };
                        if pattern.is_empty() || new_raw.is_empty() {
                            self.command_line.push_error(
                                crate::t!("Usage: XREF Path <pattern> <new path>").as_ref(),
                            );
                        } else {
                            let base_dir: std::path::PathBuf = self.tabs[i]
                                .current_path
                                .as_ref()
                                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                                .unwrap_or_else(|| std::path::PathBuf::from("."));
                            let entries = crate::io::xref::collect_entries_with_prev(
                                &self.tabs[i].scene.document,
                                &base_dir,
                                self.tabs[i].xref_unloaded.as_set(),
                                &self.tabs[i].xref_stat_cache.0,
                            );
                            let matched: Vec<_> = entries
                                .iter()
                                .filter(|e| {
                                    crate::io::xref_model::wildcard_match(&e.name, &pattern)
                                })
                                .collect();
                            if matched.is_empty() {
                                self.command_line.push_error(
                                    crate::tf!("XREF: no references match '{}'.", pattern).as_ref(),
                                );
                            } else {
                                self.push_undo_snapshot(i, "XREF-PATH");
                                let mut done = 0usize;
                                for e in matched {
                                    match crate::io::xref::set_ref_path(
                                        &mut self.tabs[i].scene.document,
                                        e.key,
                                        &new_raw,
                                    ) {
                                        Ok(name) => {
                                            self.command_line.push_output(
                                                crate::tf!(
                                                    "XREF: Path set for \"{}\" — Reload to apply.",
                                                    name
                                                )
                                                .as_ref(),
                                            );
                                            done += 1;
                                        }
                                        Err(msg) => self.command_line.push_error(msg.as_str()),
                                    }
                                }
                                if done > 0 {
                                    self.post_ref_op(i);
                                }
                            }
                        }
                    }
                } else if op.eq_ignore_ascii_case("Pathtype") {
                    // Pathtype <Full|Relative|None> [pattern] (empty/* = all).
                    let kind_tok = parts.next().unwrap_or("").trim();
                    let pattern = parts.next().unwrap_or("").trim().to_string();
                    let pathtype = if kind_tok.eq_ignore_ascii_case("Full") {
                        Some(crate::io::xref_model::Pathtype::Full)
                    } else if kind_tok.eq_ignore_ascii_case("Relative") {
                        Some(crate::io::xref_model::Pathtype::Relative)
                    } else if kind_tok.eq_ignore_ascii_case("None") {
                        Some(crate::io::xref_model::Pathtype::None)
                    } else {
                        None
                    };
                    let Some(pathtype) = pathtype else {
                        self.command_line.push_error(
                            crate::t!("Usage: XREF Pathtype <Full|Relative|None> [pattern]")
                                .as_ref(),
                        );
                        return Some(self.finish_dispatch(cmd));
                    };
                    let Some(host) = self.tabs[i].current_path.clone() else {
                        self.command_line.push_error(
                            crate::t!(
                                "XREF  Save the drawing first to resolve relative XREF paths."
                            )
                            .as_ref(),
                        );
                        return Some(self.finish_dispatch(cmd));
                    };
                    let base_dir: std::path::PathBuf = host
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let entries = crate::io::xref::collect_entries_with_prev(
                        &self.tabs[i].scene.document,
                        &base_dir,
                        self.tabs[i].xref_unloaded.as_set(),
                        &self.tabs[i].xref_stat_cache.0,
                    );
                    let matched: Vec<_> = entries
                        .iter()
                        .filter(|e| {
                            pattern.is_empty()
                                || pattern == "*"
                                || crate::io::xref_model::wildcard_match(&e.name, &pattern)
                        })
                        .collect();
                    if matched.is_empty() {
                        self.command_line.push_error(
                            crate::tf!(
                                "XREF: no references match '{}'.",
                                if pattern.is_empty() { "*" } else { &pattern }
                            )
                            .as_ref(),
                        );
                    } else {
                        self.push_undo_snapshot(i, "XREF-PATHTYPE");
                        let mut done = 0usize;
                        for e in matched {
                            match crate::io::xref::apply_pathtype(
                                &mut self.tabs[i].scene.document,
                                e.key,
                                pathtype,
                                &host,
                            ) {
                                Ok(_) => {
                                    self.command_line.push_output(
                                        crate::tf!(
                                            "XREF: Path set for \"{}\" — Reload to apply.",
                                            e.name
                                        )
                                        .as_ref(),
                                    );
                                    done += 1;
                                }
                                Err(msg) => self.command_line.push_error(msg.as_str()),
                            }
                        }
                        if done > 0 {
                            self.post_ref_op(i);
                        }
                    }
                } else if op.eq_ignore_ascii_case("Bind") {
                    // Bind <pattern> (empty/* = all): fold each direct
                    // reference into a local block. Nested entries are
                    // rejected per-entry; images are collected next to the
                    // host file; PDFs report the unavailable-import error.
                    let pattern = parts.next().unwrap_or("").trim();
                    let base_dir: std::path::PathBuf = self.tabs[i]
                        .current_path
                        .as_ref()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let entries = crate::io::xref::collect_entries_with_prev(
                        &self.tabs[i].scene.document,
                        &base_dir,
                        self.tabs[i].xref_unloaded.as_set(),
                        &self.tabs[i].xref_stat_cache.0,
                    );
                    let matched: Vec<_> = entries
                        .iter()
                        .filter(|e| {
                            pattern.is_empty()
                                || pattern == "*"
                                || crate::io::xref_model::wildcard_match(&e.name, pattern)
                        })
                        .collect();
                    if matched.is_empty() {
                        self.command_line.push_error(
                            crate::tf!("XREF: no references match '{}'.", pattern).as_ref(),
                        );
                    } else {
                        let Some(host) = self.tabs[i].current_path.clone() else {
                            self.command_line.push_error(
                                crate::t!(
                                    "XREF  Save the drawing first to resolve relative XREF paths."
                                )
                                .as_ref(),
                            );
                            return Some(self.finish_dispatch(cmd));
                        };
                        let host_dir: std::path::PathBuf = host
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| std::path::PathBuf::from("."));
                        self.push_undo_snapshot(i, "XREF-BIND");
                        let mut done = 0usize;
                        for e in matched {
                            if e.parent_key.is_some() {
                                self.command_line.push_error(crate::tf!(
                                    "XREF: cannot bind nested reference '{}'. Bind it in its host drawing.",
                                    e.name
                                ).as_ref());
                                continue;
                            }
                            match crate::io::xref::bind_reference(
                                &mut self.tabs[i].scene.document,
                                e.key,
                                &base_dir,
                                &host_dir,
                            ) {
                                Ok(outcome) => {
                                    if outcome.unremapped == 0 {
                                        self.command_line.push_output(
                                            crate::tf!("XREF: bound \"{}\".", outcome.name)
                                                .as_ref(),
                                        );
                                    } else {
                                        self.command_line.push_output(crate::tf!(
                                            "XREF: bound \"{}\" with {} unremapped style handles (see bind limitations).",
                                            outcome.name, outcome.unremapped
                                        ).as_ref());
                                    }
                                    self.tabs[i].xref_unloaded.remove(&e.key);
                                    self.tabs[i].xref_stat_cache.remove(&e.key);
                                    done += 1;
                                }
                                Err(msg) => self.command_line.push_error(msg.as_str()),
                            }
                        }
                        if done > 0 {
                            self.post_ref_op(i);
                        }
                    }
                } else if op.eq_ignore_ascii_case("Overlay") {
                    // Overlay <pattern>: flip DWG refs to Overlay type.
                    let pattern = parts.next().unwrap_or("").trim();
                    let base_dir: std::path::PathBuf = self.tabs[i]
                        .current_path
                        .as_ref()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let entries = crate::io::xref::collect_entries_with_prev(
                        &self.tabs[i].scene.document,
                        &base_dir,
                        self.tabs[i].xref_unloaded.as_set(),
                        &self.tabs[i].xref_stat_cache.0,
                    );
                    let matched: Vec<_> = entries
                        .iter()
                        .filter(|e| {
                            pattern.is_empty()
                                || pattern == "*"
                                || crate::io::xref_model::wildcard_match(&e.name, pattern)
                        })
                        .collect();
                    if matched.is_empty() {
                        self.command_line.push_error(
                            crate::tf!("XREF: no references match '{}'.", pattern).as_ref(),
                        );
                    } else {
                        self.push_undo_snapshot(i, "XREF-OVERLAY");
                        let mut done = 0usize;
                        for e in matched {
                            match crate::io::xref::set_ref_type(
                                &mut self.tabs[i].scene.document,
                                e.key,
                                crate::io::xref_model::RefType::Overlay,
                            ) {
                                Ok(name) => {
                                    self.command_line.push_output(
                                        crate::tf!("XREF: \"{}\" set to Overlay.", name).as_ref(),
                                    );
                                    done += 1;
                                }
                                Err(msg) => self.command_line.push_error(msg.as_str()),
                            }
                        }
                        if done > 0 {
                            self.post_ref_op(i);
                        }
                    }
                } else if op.eq_ignore_ascii_case("Attach") {
                    // Attach [<file>]: no-file-arg form launches the file
                    // picker (same flow as XATTACH); a file arg threads
                    // straight into placement as the pick result.
                    let after = rest.get(op.len()..).unwrap_or("").trim();
                    if after.is_empty() {
                        return Some(Task::done(Message::XAttachPick));
                    }
                    self.start_xref_attach(
                        i,
                        crate::modules::insert::xattach::XrefAttachRequest {
                            path: after.trim_matches('"').to_string(),
                            overlay: false,
                            path_type: crate::io::xref_model::Pathtype::Full,
                        },
                        crate::modules::insert::xattach::XrefPlacement::on_screen(),
                        "-XREF",
                    );
                } else {
                    self.command_line.push_error(crate::tf!(
                        "XREF: unknown option '{}'. Options: ? Reload Unload Detach Path Pathtype Bind Overlay Attach",
                        op
                    ).as_ref());
                }
            }

            "XRELOAD" => {
                // Reload all xrefs for the current drawing. Every direct key
                // leaves the session-unloaded set (reloaded content is live
                // again) with fresh stat baselines on the next refresh.
                if let Some(path) = &self.tabs[i].current_path.clone() {
                    if let Some(base_dir) = path.parent() {
                        let reload_keys: Vec<u64> = crate::io::xref::collect_entries_with_prev(
                            &self.tabs[i].scene.document,
                            base_dir,
                            self.tabs[i].xref_unloaded.as_set(),
                            &self.tabs[i].xref_stat_cache.0,
                        )
                        .iter()
                        .map(|e| e.key)
                        .collect();
                        self.push_undo_snapshot(i, "XREF-RELOAD");
                        for key in &reload_keys {
                            self.tabs[i].xref_unloaded.remove(key);
                            self.tabs[i].xref_stat_cache.remove(key);
                        }
                        let (infos, _dropped) = crate::io::xref::resolve_xrefs(
                            &mut self.tabs[i].scene.document,
                            base_dir,
                        );
                        // `resolve_xrefs` re-merges ALL references — refresh stat
                        // baselines for every currently-Loaded entry so no row
                        // reports false Stale.
                        {
                            let fresh = crate::io::xref::collect_entries_with_prev(
                                &self.tabs[i].scene.document,
                                base_dir,
                                self.tabs[i].xref_unloaded.as_set(),
                                &self.tabs[i].xref_stat_cache.0,
                            );
                            for e in &fresh {
                                if e.status == crate::io::xref_model::RefStatus::Loaded {
                                    if let Some(m) = e.modified {
                                        self.tabs[i].xref_stat_cache.insert(e.key, m);
                                    }
                                }
                            }
                        }
                        for info in &infos {
                            self.report_xref_status(info);
                        }
                        self.post_ref_op(i);
                    }
                } else {
                    self.command_line.push_error(
                        crate::t!("XREF  Save the drawing first to resolve relative XREF paths.")
                            .as_ref(),
                    );
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
                    None => self.command_line.push_error(
                        crate::t!("XOPEN: select an external reference (xref) to open.").as_ref(),
                    ),
                }
            }

            "NCOPY" | "NCOPYALL" => {
                let command = crate::modules::draw::modify::ncopy::NcopyCommand::new(
                    &self.tabs[i].scene.document,
                    self.ncopy_bind,
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
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

    #[test]
    fn ncopy_starts_nested_object_selection() {
        let mut app = fresh_app();
        let _ = app.run_command_line("NCOPY");
        assert_eq!(
            app.tabs[app.active_tab]
                .active_cmd
                .as_ref()
                .map(|active| active.name()),
            Some("NCOPY")
        );
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
            app.dock_expanded == Some(crate::ui::dock::PanelId::BlockPalette),
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
    fn xref_dash_alias_lists_like_xref() {
        // `-XREF` asks on the command line: `?` then Enter prints the
        // reference table; `XREF ?` keeps the legacy list output for scripts.
        let mut app = fresh_app();
        let out = run_capture(&mut app, "-XREF ?");
        assert!(out.contains("Total Xref(s): 0"), "got: {out:?}");
    }

    #[test]
    fn xref_question_mark_lists_count() {
        let mut app = fresh_app();
        let out = run_capture(&mut app, "XREF ?");
        assert!(out.contains("No external references") || out.contains("External references"));
    }

    #[test]
    fn externalreferences_opens_palette() {
        // EXTERNALREFERENCES is the palette command; XREFMAN is not kept,
        // not even as an alias (single-token verbs take the no-suggest path,
        // so an unknown verb reports "Unknown command").
        let mut app = fresh_app();
        let out = run_capture(&mut app, "EXTERNALREFERENCES");
        assert!(!out.contains("Unknown command"), "got: {out:?}");
        let out_old = run_capture(&mut app, "XREFMAN");
        assert!(out_old.contains("Unknown command"), "got: {out_old:?}");
    }

    #[test]
    fn externalreferences_populates_panel_table() {
        // The panel table must list whatever `XREF ?` lists: opening the
        // palette refreshes entries synchronously in the command arm.
        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN", "old/plan.dwg");
        let list = run_capture(&mut app, "XREF ?");
        assert!(list.contains("PLAN"), "CLI lists it, got: {list:?}");
        let _ = run_capture(&mut app, "EXTERNALREFERENCES");
        assert!(app.show_external_references);
        assert!(
            app.xref_manager.entries.iter().any(|e| e.name == "PLAN"),
            "panel table must list what XREF lists"
        );
        assert!(app.xref_manager.display_rows().len() >= 2);
    }

    #[test]
    fn xref_reload_no_match_resolves_nothing() {
        let mut app = fresh_app();
        let dirty_before = app.tabs[0].dirty;
        let out = run_capture(&mut app, "XREF Reload ZZZ_NO_SUCH_REF");
        assert!(out.contains("no references match"));
        assert_eq!(app.tabs[0].dirty, dirty_before);
    }

    fn add_dwg_xref(app: &mut OpenCADStudio, name: &str, saved: &str) {
        let i = app.active_tab;
        let mut br = acadrust::tables::BlockRecord::new(name);
        br.flags.is_xref = true;
        br.xref_path = saved.to_string();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
    }

    #[test]
    fn xref_path_stores_verbatim() {
        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN", "old/plan.dwg");
        let out = run_capture(&mut app, "XREF Path PLAN new\\raw path.dwg");
        assert!(out.contains("Path set"), "got: {out:?}");
        let i = app.active_tab;
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .get("PLAN")
                .unwrap()
                .xref_path,
            "new\\raw path.dwg"
        );
    }

    #[test]
    fn xref_pathtype_across_drives_errors() {
        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN", "D:/Lib/plan.dwg");
        let i = app.active_tab;
        app.tabs[i].current_path = Some(std::path::PathBuf::from("C:/Drawings/host.dwg"));
        let out = run_capture(&mut app, "XREF Pathtype Relative PLAN");
        assert!(out.contains("across drives"), "got: {out:?}");
    }

    #[test]
    fn xref_overlay_on_image_errors() {
        use acadrust::objects::{ImageDefinition, ObjectType};
        let mut app = fresh_app();
        let i = app.active_tab;
        let h = app.tabs[i].scene.document.allocate_handle();
        let mut def = ImageDefinition::with_dimensions("img.png", 8, 8);
        def.handle = h;
        app.tabs[i]
            .scene
            .document
            .objects
            .insert(h, ObjectType::ImageDefinition(def));
        // Reference it so collect_entries lists it.
        let mut img = acadrust::entities::RasterImage::new(
            "img.png",
            acadrust::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        img.definition_handle = Some(h);
        app.tabs[i]
            .scene
            .document
            .add_entity(acadrust::EntityType::RasterImage(img))
            .unwrap();
        let out = run_capture(&mut app, "XREF Overlay img.png");
        assert!(
            out.contains("overlays apply to drawing references only"),
            "got: {out:?}"
        );
    }

    #[test]
    fn xref_reload_image_keeps_flag_no_spurious_match() {
        // CLI mirror of the palette F7 guard: reloading an image row reports
        // the drawing-only error, leaves its unloaded flag untouched, and
        // does not follow with a spurious no-match error.
        use acadrust::objects::{ImageDefinition, ObjectType};
        let mut app = fresh_app();
        let i = app.active_tab;
        let h = app.tabs[i].scene.document.allocate_handle();
        let mut def = ImageDefinition::with_dimensions("img.png", 8, 8);
        def.handle = h;
        app.tabs[i]
            .scene
            .document
            .objects
            .insert(h, ObjectType::ImageDefinition(def));
        let mut img = acadrust::entities::RasterImage::new(
            "img.png",
            acadrust::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        img.definition_handle = Some(h);
        app.tabs[i]
            .scene
            .document
            .add_entity(acadrust::EntityType::RasterImage(img))
            .unwrap();
        app.tabs[i].current_path = Some(std::path::PathBuf::from("C:/Drawings/host.dwg"));
        app.tabs[i].xref_unloaded.add(h.value());
        let out = run_capture(&mut app, "XREF Reload img.png");
        assert!(
            out.contains("reload applies to drawing references only"),
            "got: {out:?}"
        );
        assert!(
            !out.contains("no references match"),
            "spurious no-match, got: {out:?}"
        );
        assert!(
            app.tabs[i].xref_unloaded.is_unloaded(h.value()),
            "image flag must stay untouched"
        );
    }

    #[test]
    fn xref_unload_reload_session_set_roundtrip() {
        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN", "old/plan.dwg");
        let i = app.active_tab;
        let key = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap()
            .handle
            .value();
        let out = run_capture(&mut app, "XREF Unload PLAN");
        assert!(out.contains("unloaded"), "got: {out:?}");
        assert!(
            app.tabs[i].xref_unloaded.is_unloaded(key),
            "unload must record the session set"
        );
        // Reload clears the session set even though the file is missing
        // (status report says Not found; the key leaves the set first).
        let dir = std::env::temp_dir();
        app.tabs[i].current_path = Some(dir.join("ocs_xref_host.dwg"));
        let out = run_capture(&mut app, "XREF Reload PLAN");
        assert!(!app.tabs[i].xref_unloaded.is_unloaded(key), "got: {out:?}");
    }

    #[test]
    fn xref_detach_nested_guard_errors() {
        use acadrust::tables::BlockRecord;
        // Host file on disk containing its own xref "INNER".
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_nested_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut host_doc = acadrust::CadDocument::new();
        let mut inner = BlockRecord::new("INNER");
        inner.flags.is_xref = true;
        inner.xref_path = "inner.dwg".to_string();
        host_doc.block_records.add(inner).unwrap();
        let bytes = crate::io::save_to_bytes(&host_doc, "dwg", host_doc.version).unwrap();
        std::fs::write(dir.join("host.dwg"), &bytes).unwrap();
        // App drawing references the host file; give the app a path in the
        // same dir so nested enumeration resolves.
        let mut app = fresh_app();
        let host_path = dir.join("host.dwg").to_string_lossy().into_owned();
        add_dwg_xref(&mut app, "HOST", &host_path);
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        let out = run_capture(&mut app, "XREF Detach INNER");
        assert!(out.contains("cannot detach nested"), "got: {out:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xref_bind_basic_rewrites_symbols() {
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_bind_cli_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut xref_doc = acadrust::CadDocument::new();
        xref_doc
            .layers
            .add(acadrust::tables::Layer::new("WALLS"))
            .unwrap();
        let mut line = acadrust::entities::Line::new();
        line.common.layer = "WALLS".to_string();
        xref_doc
            .add_entity(acadrust::EntityType::Line(line))
            .unwrap();
        let bytes = crate::io::save_to_bytes(&xref_doc, "dwg", xref_doc.version).unwrap();
        let xref_path = dir.join("plan.dwg");
        std::fs::write(&xref_path, &bytes).unwrap();

        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN", &xref_path.to_string_lossy());
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        let out = run_capture(&mut app, "XREF Bind PLAN");
        assert!(out.contains("bound \"PLAN\""), "got: {out:?}");
        let doc = &app.tabs[i].scene.document;
        assert!(doc.layers.get("PLAN$0$WALLS").is_some());
        let br = doc.block_records.get("PLAN").unwrap();
        assert!(!br.flags.is_xref && br.xref_path.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xref_bind_reports_unremapped_handles() {
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_bind_limited_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut xref_doc = acadrust::CadDocument::new();
        let mut line = acadrust::entities::Line::new();
        line.common.plotstyle_flags = 0b11;
        line.common.plotstyle_handle = Some(xref_doc.allocate_handle());
        xref_doc
            .add_entity(acadrust::EntityType::Line(line))
            .unwrap();
        let bytes = crate::io::save_to_bytes(&xref_doc, "dwg", xref_doc.version).unwrap();
        let xref_path = dir.join("plan.dwg");
        std::fs::write(&xref_path, &bytes).unwrap();

        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN", &xref_path.to_string_lossy());
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        let out = run_capture(&mut app, "XREF Bind PLAN");
        assert!(
            out.contains("with 1 unremapped style handles (see bind limitations)"),
            "got: {out:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xref_bind_pdf_errors() {
        use acadrust::objects::{ObjectType, UnderlayDefinition};
        let mut app = fresh_app();
        let i = app.active_tab;
        let h = app.tabs[i].scene.document.allocate_handle();
        let mut def = UnderlayDefinition::pdf("doc.pdf", "1");
        def.handle = h;
        app.tabs[i]
            .scene
            .document
            .objects
            .insert(h, ObjectType::UnderlayDefinition(def));
        app.tabs[i].current_path = Some(std::env::temp_dir().join("ocs_xref_bind_host.dwg"));
        let out = run_capture(&mut app, "XREF Bind doc.pdf");
        assert!(
            out.contains("PDF bind (vector import) is not available"),
            "got: {out:?}"
        );
    }

    #[test]
    fn xref_bind_nested_guard_errors() {
        use acadrust::tables::BlockRecord;
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_bind_nested_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut host_doc = acadrust::CadDocument::new();
        let mut inner = BlockRecord::new("INNER");
        inner.flags.is_xref = true;
        inner.xref_path = "inner.dwg".to_string();
        host_doc.block_records.add(inner).unwrap();
        let bytes = crate::io::save_to_bytes(&host_doc, "dwg", host_doc.version).unwrap();
        std::fs::write(dir.join("host.dwg"), &bytes).unwrap();
        let mut app = fresh_app();
        let host_path = dir.join("host.dwg").to_string_lossy().into_owned();
        add_dwg_xref(&mut app, "HOST", &host_path);
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        let out = run_capture(&mut app, "XREF Bind INNER");
        assert!(out.contains("cannot bind nested"), "got: {out:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xattach_definition_undo_removes_definition() {
        // F3: the CommitAndExit driver begins the undo transaction (structure
        // image, delta-unsafe) BEFORE prepare_xref_block + the INSERT commit,
        // so undo after XATTACH removes the definition, not just the entity.
        // This mirrors that ordering: begin first, then prepare, then undo.
        let mut app = fresh_app();
        let i = app.active_tab;
        app.tabs[i].current_path = Some(std::path::PathBuf::from("C:/Drawings/host.dwg"));
        let base = std::path::PathBuf::from("C:/Drawings");
        let pending = app.begin_undo(i, "ENTITY", 1, false).expect("begin undo");
        let name = crate::modules::insert::xattach::prepare_xref_block(
            &mut app.tabs[i].scene,
            "C:/refs/plan.dwg",
            Some(&base),
        );
        assert!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .get(&name)
                .is_some(),
            "prepare must create the definition"
        );
        app.commit_undo_delta(i, pending);
        app.undo_active_tab();
        assert!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .get(&name)
                .is_none(),
            "undo after XATTACH must remove the definition"
        );
    }

    #[test]
    fn xref_reload_pushes_undo() {
        // F2: XREF Reload resolves (merges content) — it must push an undo
        // snapshot first, mirroring the neighboring Unload/Detach arms. The
        // snapshot sits in `pending` until the next history flush, so assert
        // on the pending label, then that undo consumes it.
        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN", "old/plan.dwg");
        let i = app.active_tab;
        app.tabs[i].current_path = Some(std::path::PathBuf::from("C:/Drawings/host.dwg"));
        let out = run_capture(&mut app, "XREF Reload PLAN");
        assert!(!out.contains("no references match"), "got: {out:?}");
        assert_eq!(
            app.tabs[i]
                .history
                .pending
                .as_ref()
                .map(|p| p.label.as_str()),
            Some("XREF-RELOAD"),
            "reload must push an undo snapshot, got: {out:?}"
        );
        let redo_before = app.tabs[i].history.redo_stack.len();
        app.undo_active_tab();
        assert!(app.tabs[i].history.pending.is_none());
        let _ = redo_before;
    }

    #[test]
    fn xref_attach_with_file_starts_placement() {
        // F10: `XREF Attach <file>` threads the file into the XATTACH
        // placement command (same flow as the picker result). A missing file
        // is rejected up front, so the reference has to exist.
        let dir = std::env::temp_dir().join(format!("ocs_xref_attach_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("plan.dwg");
        std::fs::write(&file, b"fake").unwrap();
        let mut app = fresh_app();
        let out = run_capture(&mut app, &format!("XREF Attach {}", file.to_string_lossy()));
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            !out.contains("ships with reference operations"),
            "dead-end text must be gone, got: {out:?}"
        );
        let i = app.active_tab;
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().map(|c| c.name()),
            Some("XATTACH")
        );
    }

    #[test]
    fn xref_attach_bare_requests_picker_without_dead_end() {
        // F10: bare `XREF Attach` launches the file picker flow (the
        // returned task); nothing starts synchronously and the Task 8/9
        // dead-end text is gone.
        let mut app = fresh_app();
        let out = run_capture(&mut app, "XREF Attach");
        assert!(
            !out.contains("ships with reference operations"),
            "dead-end text must be gone, got: {out:?}"
        );
        let i = app.active_tab;
        assert!(app.tabs[i].active_cmd.is_none());
    }

    #[test]
    fn xref_selective_reload_touches_only_matched_baselines() {
        // F5 (targeted reload): `resolve_xrefs_for_keys` re-merges ONLY the
        // matched refs, so stat baselines refresh for the matched subset
        // alone. Unmatched rows keep their prior baseline (and their content
        // is untouched), so no false Stale can arise from this reload.
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_reload_skew_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let a_path = dir.join("plan_a.dwg");
        let b_path = dir.join("plan_b.dwg");
        std::fs::write(&a_path, b"fake-a").unwrap();
        std::fs::write(&b_path, b"fake-b").unwrap();
        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN-A", &a_path.to_string_lossy());
        add_dwg_xref(&mut app, "PLAN-B", &b_path.to_string_lossy());
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        let key_a = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN-A")
            .unwrap()
            .handle
            .value();
        let key_b = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN-B")
            .unwrap()
            .handle
            .value();
        let out = run_capture(&mut app, "XREF Reload PLAN-A");
        assert!(!out.contains("no references match"), "got: {out:?}");
        assert!(
            app.tabs[i].xref_stat_cache.0.contains_key(&key_a),
            "matched entry baseline refreshed, got: {out:?}"
        );
        assert!(
            !app.tabs[i].xref_stat_cache.0.contains_key(&key_b),
            "unmatched entry baseline untouched, got: {out:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xref_bind_undo_restores_xref_state() {
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_bind_undo_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut xref_doc = acadrust::CadDocument::new();
        xref_doc
            .layers
            .add(acadrust::tables::Layer::new("WALLS"))
            .unwrap();
        let mut line = acadrust::entities::Line::new();
        line.common.layer = "WALLS".to_string();
        xref_doc
            .add_entity(acadrust::EntityType::Line(line))
            .unwrap();
        let bytes = crate::io::save_to_bytes(&xref_doc, "dwg", xref_doc.version).unwrap();
        let xref_path = dir.join("plan.dwg");
        std::fs::write(&xref_path, &bytes).unwrap();
        let saved = xref_path.to_string_lossy().into_owned();

        let mut app = fresh_app();
        add_dwg_xref(&mut app, "PLAN", &saved);
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        // Resolve first so `PLAN|*` names exist pre-bind.
        let out = run_capture(&mut app, "XREF Reload PLAN");
        assert!(out.contains("Reloaded"), "got: {out:?}");
        assert!(app.tabs[i]
            .scene
            .document
            .layers
            .get("PLAN|WALLS")
            .is_some());
        let out = run_capture(&mut app, "XREF Bind PLAN");
        assert!(out.contains("bound \"PLAN\""), "got: {out:?}");
        assert!(app.tabs[i]
            .scene
            .document
            .layers
            .get("PLAN$0$WALLS")
            .is_some());

        app.undo_active_tab();
        let doc = &app.tabs[i].scene.document;
        let br = doc.block_records.get("PLAN").unwrap();
        assert!(br.flags.is_xref, "undo must restore the xref flag");
        assert_eq!(br.xref_path, saved, "undo must restore the xref path");
        assert!(
            doc.layers.get("PLAN|WALLS").is_some(),
            "undo must restore `|` names"
        );
        assert!(
            !doc.layers.names().any(|n| n.contains("PLAN$0")),
            "undo must drop the bound names"
        );
        std::fs::remove_dir_all(&dir).ok();
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
