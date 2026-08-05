use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_inquiry(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            "3DORBIT" => {
                self.tabs[i].orbit_mode = true;
                self.clear_navigation_hover(i);
            }

            // ── Selection utilities ───────────────────────────────────────
            "SELECTALL" => {
                use crate::scene::Scene;
                let handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .entity_wires()
                    .iter()
                    .filter_map(|w| Scene::handle_from_wire_name(&w.name))
                    // Objects on a locked layer aren't selectable.
                    .filter(|&h| !self.tabs[i].scene.is_layer_locked(h))
                    .collect();
                let count = handles.len();
                for h in handles {
                    self.tabs[i].scene.select_entity(h, false);
                }
                self.command_line
                    .push_output(crate::tf!("SELECTALL: {} object(s) selected.", count).as_ref());
                self.refresh_properties();
            }

            "DESELECT" | "DESELALL" => {
                self.tabs[i].scene.deselect_all();
                self.command_line.push_output(crate::t!("Deselected.").as_ref());
                self.refresh_properties();
            }

            "SELECTSIMILAR" | "SELSIM" => {
                let added = self.tabs[i].scene.select_similar();
                self.command_line
                    .push_output(crate::tf!("Select Similar: {} added.", added).as_ref());
                self.refresh_properties();
            }

            // ADDSELECTED — draw a new object of the same type as the selected
            // one, inheriting its general properties. (#239)
            "ADDSELECTED" => {
                return Some(self.cmd_add_selected(i));
            }

            // QSELECT builds a selection set by object type / property; FILTER is
            // the same criteria-based selection.
            "QSELECT" | "FILTER" => {
                return Some(Task::done(Message::QSelectOpen));
            }

            // CAL / QUICKCALC <expression> — evaluate an arithmetic expression
            //   (+ - * /, parentheses, unary minus, decimals).
            "CAL" | "QUICKCALC" | "QC" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("CAL", "CAL  expression  (e.g. (2+3)*4):");
                self.command_line.push_info(&c.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("CAL ")
                || cmd.starts_with("QUICKCALC ")
                || cmd.starts_with("QC ") =>
            {
                let expr = cmd.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim();
                if expr.is_empty() {
                    self.command_line
                        .push_info(crate::t!("Usage: CAL <expression>   e.g. CAL (2+3)*4").as_ref());
                } else {
                    match arith_eval(expr) {
                        Ok(v) => self.command_line.push_output(crate::tf!("= {v}").as_ref()),
                        Err(e) => self.command_line.push_error(crate::tf!("CAL: {e}").as_ref()),
                    }
                }
            }

            // ── LIST — entity info ────────────────────────────────────────
            "LIST" => {
                let selected: Vec<_> = self.tabs[i].scene.selected_entities();
                if selected.is_empty() {
                    self.command_line
                        .push_error(crate::t!("LIST: no entities selected. Select entities first.").as_ref());
                } else {
                    for (handle, _) in &selected {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity(*handle) {
                            let type_name = crate::entities::names::dxf_name(entity);
                            let common = entity.common();
                            let color_str = common
                                .color
                                .index()
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "ByLayer".to_string());
                            let linetype =
                                if common.linetype.is_empty() || common.linetype == "ByLayer" {
                                    "ByLayer".to_string()
                                } else {
                                    common.linetype.clone()
                                };
                            // Entity-specific details
                            let details = entity_list_details(entity);
                            self.command_line.push_output(crate::tf!(
                                "{type_name}  Handle:{:X}  Layer:{}  Color:{}  LT:{}{}",
                                handle.value(),
                                common.layer,
                                color_str,
                                linetype,
                                if details.is_empty() {
                                    String::new()
                                } else {
                                    format!("\n    {details}")
                                }
                            ).as_ref());
                        }
                    }
                }
            }

            // SELHANDLES — report the current selection for diagnostics: the
            // active space (Model / which paper layout), a per-type and
            // per-block breakdown, and the raw comma-separated hex handle list
            // (so what renders on screen can be compared against the file).
            "SELHANDLES" | "SELH" => {
                let scene = &self.tabs[i].scene;
                let selected = scene.selected_entities();
                if selected.is_empty() {
                    self.command_line
                        .push_error(crate::t!("SELHANDLES: no entities selected. Select entities first.").as_ref());
                } else {
                    use std::collections::BTreeMap;
                    let space = if scene.current_layout == "Model" {
                        "Model space".to_string()
                    } else {
                        format!("Paper space '{}'", scene.current_layout)
                    };
                    let mut handles: Vec<u64> = Vec::with_capacity(selected.len());
                    let mut type_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
                    let mut block_counts: BTreeMap<String, usize> = BTreeMap::new();
                    for (h, e) in &selected {
                        handles.push(h.value());
                        *type_counts
                            .entry(crate::entities::names::dxf_name(e))
                            .or_default() += 1;
                        if let acadrust::EntityType::Insert(ins) = e {
                            *block_counts.entry(ins.block_name.clone()).or_default() += 1;
                        }
                    }
                    handles.sort_unstable();
                    let types: Vec<String> =
                        type_counts.iter().map(|(t, n)| format!("{t}×{n}")).collect();
                    let list: Vec<String> = handles.iter().map(|h| format!("{:X}", h)).collect();
                    let mut msg = format!(
                        "SELHANDLES: {} selected in {}\n  Types: {}",
                        handles.len(),
                        space,
                        types.join(", ")
                    );
                    if !block_counts.is_empty() {
                        let blocks: Vec<String> =
                            block_counts.iter().map(|(b, n)| format!("{b}×{n}")).collect();
                        msg.push_str(&format!("\n  Blocks: {}", blocks.join(", ")));
                    }
                    msg.push_str(&format!("\n  Handles: {}", list.join(",")));
                    self.command_line.push_output(&msg);
                }
            }

            // DBLIST — list data for every entity in the drawing (LIST over the
            // whole database rather than the current selection).
            "DBLIST" => {
                // Format every entity first so the immutable document borrow is
                // released before writing to the command line.
                let lines: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .map(|entity| {
                        let type_name = crate::entities::names::dxf_name(entity);
                        let common = entity.common();
                        let color_str = common
                            .color
                            .index()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "ByLayer".to_string());
                        let linetype = if common.linetype.is_empty() || common.linetype == "ByLayer"
                        {
                            "ByLayer".to_string()
                        } else {
                            common.linetype.clone()
                        };
                        let details = entity_list_details(entity);
                        format!(
                            "{type_name}  Handle:{:X}  Layer:{}  Color:{}  LT:{}{}",
                            common.handle.value(),
                            common.layer,
                            color_str,
                            linetype,
                            if details.is_empty() {
                                String::new()
                            } else {
                                format!("\n    {details}")
                            }
                        )
                    })
                    .collect();
                if lines.is_empty() {
                    self.command_line
                        .push_info(crate::t!("DBLIST: drawing has no entities.").as_ref());
                } else {
                    let count = lines.len();
                    for l in lines {
                        self.command_line.push_output(&l);
                    }
                    self.command_line
                        .push_output(crate::tf!("DBLIST: {count} objects.").as_ref());
                }
            }

            // ── Break / Join ─────────────────────────────────────────────────
            "JOIN" => {
                use crate::modules::draw::modify::join::JoinCommand;
                // Pickfirst: with objects already selected, join them right
                // away instead of asking for a selection again.
                let selected: Vec<acadrust::Handle> =
                    self.tabs[i].scene.selected.iter().copied().collect();
                if selected.len() >= 2 {
                    let task =
                        self.apply_cmd_result(crate::command::CmdResult::JoinEntities(selected));
                    return Some(task);
                }
                let cmd = JoinCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "BREAK" => {
                use crate::modules::draw::modify::break_cmd::BreakInteractiveCommand;
                let cmd = BreakInteractiveCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "BREAKATPOINT" => {
                use crate::modules::draw::modify::break_cmd::BreakAtPointCommand;
                let cmd = BreakAtPointCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "PEDIT" => {
                use crate::modules::draw::modify::pedit::{PeditCommand, PeditTarget};
                let info = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter_map(|e| {
                        let h = e.common().handle.value();
                        match e {
                            acadrust::EntityType::LwPolyline(_)
                            | acadrust::EntityType::Polyline2D(_) => Some((
                                h,
                                PeditTarget {
                                    is_poly: true,
                                    convertible: false,
                                },
                            )),
                            acadrust::EntityType::Line(_) | acadrust::EntityType::Arc(_) => {
                                Some((
                                    h,
                                    PeditTarget {
                                        is_poly: false,
                                        convertible: true,
                                    },
                                ))
                            }
                            _ => None,
                        }
                    })
                    .collect();
                // Pickfirst: an already-selected polyline (or line/arc, via
                // the convert prompt) skips the select step.
                let preselected: Vec<acadrust::Handle> =
                    self.tabs[i].scene.selected.iter().copied().collect();
                let cmd_obj = PeditCommand::new(info).with_preselection(&preselected);
                self.command_line.push_info(&cmd_obj.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd_obj));
            }

            "SPLINEDIT" => {
                use crate::modules::draw::modify::splinedit::SplineditCommand;
                let cmd_obj = SplineditCommand::new();
                self.command_line.push_info(&cmd_obj.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd_obj));
            }

            // Bare ATTEDIT (and the ATE alias) open the attribute editor dialog.
            // If a single block with attributes is already selected it opens on
            // that block; otherwise the pick command runs and the editor opens
            // once a block is chosen (see `command_driver`).
            "ATTEDIT" => {
                self.open_attedit_dialog();
            }

            // ── REFEDIT — in-place block editing ─────────────────────────────
            "REFEDIT" => {
                use crate::modules::draw::modify::refedit::RefEditPickCommand;
                // If a session is already active, tell the user.
                if self.tabs[i].refedit_session.is_some() {
                    self.command_line
                        .push_error(crate::t!("REFEDIT: a session is already active. Use REFCLOSE first.").as_ref());
                } else {
                    // Check if a single INSERT is already selected.
                    let selected: Vec<_> =
                        self.tabs[i].scene.selected_entities().into_iter().collect();
                    if selected.len() == 1 {
                        if let Some(acadrust::EntityType::Insert(_)) =
                            selected.first().map(|(_, e)| e)
                        {
                            let handle = selected[0].0;
                            // Skip pick phase — jump straight to begin.
                            let _ =
                                self.dispatch_command(&format!("REFEDIT_BEGIN:{}", handle.value()));
                            return Some(Task::none());
                        }
                    }
                    let cmd_obj = RefEditPickCommand::new();
                    self.command_line.push_info(&cmd_obj.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd_obj));
                }
            }

            "BEDIT" => {
                use crate::modules::draw::modify::block_edit::BlockEditPickCommand;
                if self.tabs[i].refedit_session.is_some() {
                    self.command_line
                        .push_error(crate::t!("BEDIT: finish the active REFEDIT (REFCLOSE) first.").as_ref());
                } else {
                    // Jump straight to begin when a single INSERT is preselected.
                    let selected: Vec<_> =
                        self.tabs[i].scene.selected_entities().into_iter().collect();
                    if selected.len() == 1 {
                        if let Some(acadrust::EntityType::Insert(_)) =
                            selected.first().map(|(_, e)| e)
                        {
                            let handle = selected[0].0;
                            let _ =
                                self.dispatch_command(&format!("BEDIT_BEGIN:{}", handle.value()));
                            return Some(Task::none());
                        }
                    }
                    let cmd_obj = BlockEditPickCommand::new();
                    self.command_line.push_info(&cmd_obj.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd_obj));
                }
            }

            cmd if cmd.starts_with("BEDIT_BEGIN:") => {
                use crate::modules::draw::modify::block_edit::BlockEditSession;
                use acadrust::Handle;

                let handle_u64: u64 = cmd["BEDIT_BEGIN:".len()..].parse().unwrap_or(0);
                let insert_handle = Handle::new(handle_u64);

                let insert = match self.tabs[i].scene.document.get_entity(insert_handle) {
                    Some(acadrust::EntityType::Insert(ins)) => ins.clone(),
                    _ => {
                        self.command_line
                            .push_error(crate::t!("BEDIT: selected object is not a block reference.").as_ref());
                        return Some(Task::none());
                    }
                };

                // Resolve the block record; reject external references (xrefs).
                let (br_handle, is_xref) = match self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&insert.block_name)
                {
                    Some(br) => (br.handle, br.flags.is_xref),
                    None => {
                        self.command_line.push_error(crate::tf!(
                            "BEDIT: block \"{}\" not found.",
                            insert.block_name
                        ).as_ref());
                        return Some(Task::none());
                    }
                };
                if is_xref {
                    self.command_line
                        .push_error(crate::t!("BEDIT: cannot edit an external reference (xref).").as_ref());
                    return Some(Task::none());
                }
                if self.tabs[i]
                    .block_edits
                    .iter()
                    .any(|session| session.br_handle == br_handle)
                {
                    self.tabs[i].active_cmd = None;
                    return Some(Task::done(Message::BlockEditSwitch(
                        insert.block_name.clone(),
                    )));
                }

                // Snapshot the complete block definition so Discard can restore
                // structural markers, ATTDEFs and geometry with exact handles.
                let snapshot: Vec<_> = {
                    let br = self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .get(&insert.block_name)
                        .unwrap();
                    br.entity_handles
                        .iter()
                        .filter_map(|h| self.tabs[i].scene.document.get_entity(*h).cloned())
                        .collect()
                };
                let dependent_snapshot: Vec<_> = self.tabs[i]
                    .scene
                    .block_definition_dependent_handles(br_handle)
                    .into_iter()
                    .filter_map(|handle| {
                        self.tabs[i].scene.document.get_entity_arc(handle)
                    })
                    .collect();
                let reference_attributes: Vec<_> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter_map(|entity| match entity {
                        acadrust::EntityType::Insert(reference)
                            if reference.block_name.eq_ignore_ascii_case(&insert.block_name)
                                && !reference.attributes.is_empty() =>
                        {
                            Some((
                                reference.common.handle,
                                reference.attributes.clone(),
                            ))
                        }
                        _ => None,
                    })
                    .collect();

                self.push_undo_snapshot(i, "BEDIT");

                // Capture the camera before the editor reframes it, so leaving
                // the block editor returns the view exactly where it was (#425).
                let current_camera = self.tabs[i].scene.camera.borrow().clone();
                let current_ucs = self.tabs[i].active_ucs.clone();
                let (return_layout, return_block, return_camera) =
                    if let Some(parent_index) = self.tabs[i].active_block_edit {
                        let parent = &mut self.tabs[i].block_edits[parent_index];
                        parent.editor_camera = current_camera.clone();
                        parent.editor_ucs = current_ucs;
                        (
                            parent.return_layout.clone(),
                            Some(parent.block_name.clone()),
                            parent.return_camera.clone(),
                        )
                    } else {
                        self.tabs[i].scene.sync_camera_to_document();
                        (
                            self.tabs[i].scene.current_layout.clone(),
                            None,
                            current_camera.clone(),
                        )
                    };
                // A block editor renders in model style; switch to Model first so
                // the paper-space code paths stay off, then scope to the block.
                if self.tabs[i].scene.current_layout != "Model" {
                    self.tabs[i].scene.set_current_layout("Model".to_string());
                }
                self.tabs[i].scene.block_edit_block = Some(br_handle);
                self.tabs[i].block_edits.push(BlockEditSession {
                    block_name: insert.block_name.clone(),
                    br_handle,
                    return_layout,
                    return_block,
                    snapshot,
                    dependent_snapshot,
                    reference_attributes,
                    return_camera,
                    editor_camera: current_camera,
                    editor_ucs: None,
                });
                self.tabs[i].active_block_edit = Some(self.tabs[i].block_edits.len() - 1);
                // Every BEDIT tab owns an isolated local UCS. Never inherit or
                // overwrite the drawing's model-space UCS.
                self.tabs[i].refresh_active_ucs();

                self.tabs[i].scene.deselect_all();
                // The first click of a double-click selects the model-space
                // INSERT and populates the cached grip overlay. Clearing only
                // Scene::selected leaves that INSERT's grips active inside the
                // block editor, where dragging one moves the outer reference
                // instead of block-local geometry (#517).
                self.tabs[i].active_grip = None;
                self.grip_hover = None;
                self.grip_popup = None;
                self.visibility_popup = None;
                // Entering BEDIT changes which block is assembled, not the
                // cached geometry of that block's entities.
                self.tabs[i].scene.bump_geometry_no_blocks();
                // Frame the camera on the block's own geometry (block-local, near
                // origin) — fit_all() goes through current_layout_block_handle so
                // it already scopes to the edited block. Without this the view
                // stays wherever model/paper space was. (#261)
                self.tabs[i].scene.fit_all();
                let editor_camera = self.tabs[i].scene.camera.borrow().clone();
                if let Some(session) = self.tabs[i].active_block_edit_session_mut() {
                    session.editor_camera = editor_camera;
                }
                self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
                self.refresh_properties();
                self.tabs[i].active_cmd = None;
                self.tabs[i].dirty = true;
                self.command_line.push_info(crate::tf!(
                    "BEDIT: Editing block \"{}\". Use Save Block or Discard to finish.",
                    insert.block_name
                ).as_ref());
            }

            "BEDIT_SAVE" => {
                let session = match self.tabs[i].active_block_edit.take() {
                    Some(index) if index < self.tabs[i].block_edits.len() => {
                        self.tabs[i].block_edits.remove(index)
                    }
                    None => {
                        self.command_line
                            .push_error(crate::t!("BEDIT_SAVE: no block editor is open.").as_ref());
                        return Some(Task::none());
                    }
                    Some(_) => {
                        self.command_line
                            .push_error(crate::t!("BEDIT_SAVE: invalid block editor state.").as_ref());
                        return Some(Task::none());
                    }
                };
                // Edits are live on the block record — just leave the block space.
                self.restore_after_block_edit_close(
                    i,
                    session.return_block.as_deref(),
                    &session.return_layout,
                    &session.return_camera,
                );
                self.tabs[i].dirty = true;
                self.command_line.push_output(crate::tf!(
                    "BEDIT: Block \"{}\" saved. All references updated.",
                    session.block_name
                ).as_ref());
            }

            "BEDIT_DISCARD" => {
                let session = match self.tabs[i].active_block_edit.take() {
                    Some(index) if index < self.tabs[i].block_edits.len() => {
                        self.tabs[i].block_edits.remove(index)
                    }
                    None => {
                        self.command_line
                            .push_error(crate::t!("BEDIT_DISCARD: no block editor is open.").as_ref());
                        return Some(Task::none());
                    }
                    Some(_) => {
                        self.command_line
                            .push_error(crate::t!("BEDIT_DISCARD: invalid block editor state.").as_ref());
                        return Some(Task::none());
                    }
                };
                // Restore the block definition to its on-entry snapshot: remove the
                // block's current entities, then re-add the snapshot ones (mirrors
                // the REFCLOSE_SAVE write-back).
                let old_handles: Vec<_> = match self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&session.block_name)
                {
                    Some(br) => br.entity_handles.clone(),
                    None => vec![],
                };
                for h in &old_handles {
                    self.tabs[i].scene.document.remove_entity(*h);
                }
                if let Some(br) = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get_mut(&session.block_name)
                {
                    br.entity_handles.clear();
                }
                let block_name = session.block_name.clone();
                let return_block = session.return_block.clone();
                let return_layout = session.return_layout.clone();
                let return_camera = session.return_camera.clone();
                for mut entity in session.snapshot {
                    // Preserve the original handle so ATTDEF references and
                    // other handle-based relationships survive Discard.
                    entity.common_mut().owner_handle = session.br_handle;
                    let _ = self.tabs[i].scene.document.add_entity(entity);
                }
                for (handle, attributes) in session.reference_attributes {
                    if let Some(acadrust::EntityType::Insert(reference)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        reference.attributes = attributes;
                    }
                }
                for entity in session.dependent_snapshot {
                    let handle = entity.common().handle;
                    let _ = self.tabs[i]
                        .scene
                        .document
                        .replace_entity_arc(handle, entity);
                }
                self.restore_after_block_edit_close(
                    i,
                    return_block.as_deref(),
                    &return_layout,
                    &return_camera,
                );
                self.tabs[i].dirty = true;
                self.command_line.push_output(crate::tf!(
                    "BEDIT: Block \"{}\" edit discarded.",
                    block_name
                ).as_ref());
            }

            cmd if cmd.starts_with("REFEDIT_BEGIN:") => {
                use crate::modules::draw::modify::refedit::{
                    apply_insert_transform, RefEditSession,
                };
                use acadrust::Handle;

                let handle_u64: u64 = cmd["REFEDIT_BEGIN:".len()..].parse().unwrap_or(0);
                let insert_handle = Handle::new(handle_u64);

                // Get INSERT entity.
                let insert = match self.tabs[i].scene.document.get_entity(insert_handle) {
                    Some(acadrust::EntityType::Insert(ins)) => ins.clone(),
                    _ => {
                        self.command_line
                            .push_error(crate::t!("REFEDIT: selected object is not an INSERT.").as_ref());
                        return Some(Task::none());
                    }
                };

                // Build the INSERT's full placement transform (OCS + rotation +
                // scale, including non-uniform / mirrored) and its inverse, so
                // edits round-trip back to block-local coordinates on SAVE.
                let sx = insert.x_scale();
                let sy = insert.y_scale();
                let sz = insert.z_scale();
                let forward = insert.get_transform();
                let inverse = {
                    use acadrust::types::{Matrix3, Matrix4, Transform};
                    let ocs_t =
                        Matrix4::from_matrix3(Matrix3::arbitrary_axis(insert.normal).transpose());
                    let t_inv = Matrix4::translation(
                        -insert.insert_point.x,
                        -insert.insert_point.y,
                        -insert.insert_point.z,
                    );
                    let r_inv = Matrix4::rotation_z(-insert.rotation);
                    let s_inv = Matrix4::scaling(1.0 / sx, 1.0 / sy, 1.0 / sz);
                    // inverse(OCS·T·R·S) = S⁻¹·R⁻¹·T⁻¹·OCSᵀ
                    Transform::from_matrix(s_inv * r_inv * t_inv * ocs_t)
                };

                // Find the block record.
                let br_handle = match self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&insert.block_name)
                {
                    Some(br) => br.handle,
                    None => {
                        self.command_line.push_error(crate::tf!(
                            "REFEDIT: block \"{}\" not found.",
                            insert.block_name
                        ).as_ref());
                        return Some(Task::none());
                    }
                };

                // Collect block-local entities (skip structural Block/BlockEnd/AttDef).
                let block_entities: Vec<_> = {
                    let br = self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .get(&insert.block_name)
                        .unwrap();
                    br.entity_handles
                        .iter()
                        .filter_map(|h| self.tabs[i].scene.document.get_entity(*h).cloned())
                        .filter(|e| {
                            !matches!(
                                e,
                                acadrust::EntityType::Block(_)
                                    | acadrust::EntityType::BlockEnd(_)
                                    | acadrust::EntityType::AttributeDefinition(_)
                            )
                        })
                        .collect()
                };

                if block_entities.is_empty() {
                    self.command_line.push_error(crate::t!("REFEDIT: block is empty.").as_ref());
                    return Some(Task::none());
                }

                let session = RefEditSession {
                    block_name: insert.block_name.clone(),
                    br_handle,
                    temp_handles: vec![],
                    forward,
                    inverse,
                    // Everything allocated from here on is part of the session.
                    handle_watermark: self
                        .tabs[i]
                        .scene
                        .document
                        .next_handle()
                        .max(self.tabs[i].scene.document.header.handle_seed),
                };

                self.push_undo_snapshot(i, "REFEDIT");
                self.tabs[i].refedit_session = Some(session.clone());

                // Add block entities to model space with INSERT transform applied.
                let mut temp_handles = Vec::new();
                for mut entity in block_entities {
                    apply_insert_transform(&mut entity, &session);
                    entity.common_mut().handle = acadrust::Handle::NULL;
                    entity.common_mut().owner_handle = acadrust::Handle::NULL;
                    let h = self.tabs[i].scene.add_entity(entity);
                    temp_handles.push(h);
                }
                self.tabs[i].refedit_session.as_mut().unwrap().temp_handles = temp_handles.clone();

                // Fade everything except the entities being edited, so the
                // surrounding drawing stays visible for context but the block's
                // geometry stands out. (#136)
                self.tabs[i]
                    .scene
                    .set_refedit_keep(Some(temp_handles.iter().copied().collect()));

                // Select the temp entities so user can see what they're editing.
                self.tabs[i].scene.deselect_all();
                for h in &temp_handles {
                    self.tabs[i].scene.select_entity(*h, false);
                }
                self.tabs[i].dirty = true;

                // No active command — the user edits the block's geometry freely
                // (move, grips, draw, erase…) and runs REFCLOSE when done. (#136)
                self.tabs[i].active_cmd = None;
                self.command_line.push_info(crate::tf!(
                    "REFEDIT: Editing block \"{}\". Run REFCLOSE to save, REFCLOSE_DISCARD to cancel.",
                    insert.block_name
                ).as_ref());
            }

            "REFCLOSE" => {
                if self.tabs[i].refedit_session.is_some() {
                    use crate::modules::draw::modify::refedit::RefCloseCommand;
                    let cmd_obj = RefCloseCommand::new();
                    self.command_line.push_info(&cmd_obj.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd_obj));
                } else {
                    self.command_line
                        .push_error(crate::t!("REFCLOSE: no REFEDIT session active.").as_ref());
                }
            }

            "REFCLOSE_SAVE" => {
                use crate::modules::draw::modify::explode::normalize_entity_for_block;
                use crate::modules::draw::modify::refedit::apply_insert_inverse_transform;

                let session = match self.tabs[i].refedit_session.take() {
                    Some(s) => s,
                    None => {
                        self.command_line
                            .push_error(crate::t!("REFCLOSE: no REFEDIT session active.").as_ref());
                        return Some(Task::none());
                    }
                };

                self.push_undo_snapshot(i, "REFCLOSE");

                // The working set: the temp copies PLUS everything the user
                // created in this space during the session (drawn / pasted /
                // offset entities carry handles at or above the watermark) —
                // those must fold into the block too, not stay behind in
                // model space (#423).
                let working: Vec<acadrust::Handle> = {
                    let space = self.tabs[i].scene.current_layout_block_handle_pub();
                    let drawn = self.tabs[i].scene.document.entities().filter(|e| {
                        let c = e.common();
                        c.owner_handle == space
                            && c.handle.value() >= session.handle_watermark
                            && !session.temp_handles.contains(&c.handle)
                    });
                    session
                        .temp_handles
                        .iter()
                        .copied()
                        .chain(drawn.map(|e| e.common().handle))
                        .collect()
                };

                // Collect the edited temp entities.
                let new_entities: Vec<acadrust::EntityType> = working
                    .iter()
                    .filter_map(|h| self.tabs[i].scene.document.get_entity(*h).cloned())
                    .collect();

                // Remove temp entities from model space.
                self.tabs[i].scene.erase_entities(&working);

                // Apply inverse INSERT transform → block-local coordinates.
                let new_entities: Vec<_> = new_entities
                    .into_iter()
                    .map(|mut entity| {
                        apply_insert_inverse_transform(&mut entity, &session);
                        let mut entity = normalize_entity_for_block(entity);
                        entity.common_mut().handle = acadrust::Handle::NULL;
                        entity.common_mut().owner_handle = session.br_handle;
                        entity
                    })
                    .collect();

                // Remove old block entities from the document.
                let old_handles: Vec<_> = match self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&session.block_name)
                {
                    Some(br) => br.entity_handles.clone(),
                    None => vec![],
                };
                for h in &old_handles {
                    self.tabs[i].scene.document.remove_entity(*h);
                }
                // Flush the entity_handles list from the block record.
                if let Some(br) = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get_mut(&session.block_name)
                {
                    br.entity_handles.clear();
                }

                // Add the new block entities.
                for entity in new_entities {
                    let _ = self.tabs[i].scene.document.add_entity(entity);
                }

                self.tabs[i].dirty = true;
                self.command_line.push_output(crate::tf!(
                    "REFCLOSE: Block \"{}\" saved. All references updated.",
                    session.block_name
                ).as_ref());
                // End the edit fade before rebuilding, so the restored geometry
                // recolours bright. (#136)
                self.tabs[i].scene.set_refedit_keep(None);
                // Rebuild hatch/image/mesh caches since block content changed.
                self.tabs[i].scene.rebuild_derived_caches();
            }

            "REFCLOSE_DISCARD" => {
                let session = match self.tabs[i].refedit_session.take() {
                    Some(s) => s,
                    None => {
                        self.command_line
                            .push_error(crate::t!("REFCLOSE: no REFEDIT session active.").as_ref());
                        return Some(Task::none());
                    }
                };
                // Remove the working set without modifying the block — the
                // temp copies plus anything created during the session (#423).
                let working: Vec<acadrust::Handle> = {
                    let space = self.tabs[i].scene.current_layout_block_handle_pub();
                    let drawn = self.tabs[i].scene.document.entities().filter(|e| {
                        let c = e.common();
                        c.owner_handle == space
                            && c.handle.value() >= session.handle_watermark
                            && !session.temp_handles.contains(&c.handle)
                    });
                    session
                        .temp_handles
                        .iter()
                        .copied()
                        .chain(drawn.map(|e| e.common().handle))
                        .collect()
                };
                self.tabs[i].scene.erase_entities(&working);
                self.tabs[i].scene.deselect_all();
                // End the edit fade — restore the drawing to full brightness.
                self.tabs[i].scene.set_refedit_keep(None);
                self.command_line
                    .push_output(crate::t!("REFCLOSE: Changes discarded.").as_ref());
            }

            "ALIGN" => {
                use crate::modules::draw::modify::align::AlignCommand;
                let cmd = AlignCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "LENGTHEN" => {
                use crate::modules::draw::modify::lengthen::LengthenCommand;
                let cmd = LengthenCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIVIDE" => {
                use crate::modules::draw::inquiry::divide::DivideCommand;
                let cmd = DivideCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "MEASURE" => {
                use crate::modules::draw::inquiry::divide::MeasureCommand;
                let cmd = MeasureCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // ── Inquiry ──────────────────────────────────────────────────────
            "DIST" => {
                use crate::modules::draw::inquiry::dist::DistCommand;
                let cmd = DistCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "ID" => {
                use crate::modules::draw::inquiry::id::IdCommand;
                let cmd = IdCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "AREA" => {
                use crate::modules::draw::inquiry::area::AreaCommand;
                let cmd = AreaCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // ── MASSPROP — area, perimeter, centroid of selected entities ────
            "MASSPROP" => {
                let selected = self.tabs[i].scene.selected_entities();
                if selected.is_empty() {
                    self.command_line
                        .push_error(crate::t!("MASSPROP: no entities selected. Select entities first.").as_ref());
                } else {
                    for (handle, _) in &selected {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity(*handle) {
                            use crate::entities::traits::EntityTypeOps;
                            if let Some(props) = entity.mass_props() {
                                self.command_line.push_output(crate::tf!(
                                    "{}  Area={:.4}  Perimeter={:.4}  Centroid=({:.4},{:.4})",
                                    crate::entities::names::dxf_name(entity),
                                    props.area,
                                    props.perimeter,
                                    props.cx,
                                    props.cy,
                                ).as_ref());
                            }
                        }
                    }
                }
            }

            // ── FLATTEN — move selected (or all) entities to Z=0 ─────────────
            "FLATTEN" => {
                let handles: Vec<acadrust::Handle> = {
                    let sel = self.tabs[i].scene.selected_entities();
                    if sel.is_empty() {
                        // Flatten all entities
                        self.tabs[i]
                            .scene
                            .document
                            .entities()
                            .map(|e| e.common().handle)
                            .collect()
                    } else {
                        sel.into_iter().map(|(h, _)| h).collect()
                    }
                };
                if handles.is_empty() {
                    self.command_line.push_error(crate::t!("FLATTEN: no entities.").as_ref());
                } else {
                    self.push_undo_snapshot(i, "FLATTEN");
                    for h in &handles {
                        if let Some(e) = self.tabs[i].scene.document.get_entity_mut(*h) {
                            flatten_entity_z(e);
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(crate::tf!(
                        "FLATTEN: {} entity(ies) moved to Z=0.",
                        handles.len()
                    ).as_ref());
                    self.refresh_properties();
                }
            }

            // ── QSELECT — quick-select entities by property ───────────────────
            // QSELECT TYPE <type>          — select all entities of given type
            // QSELECT LAYER <name>         — select all entities on layer
            // QSELECT COLOR <n>            — select all entities with color index n
            // QSELECT LINETYPE <name>      — select all entities with linetype
            cmd if cmd == "QSELECT" || cmd.starts_with("QSELECT ") => {
                let rest = cmd.split_once(' ').map(|(_, r)| r.trim()).unwrap_or("");
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                let prop = parts.first().map(|s| s.to_uppercase()).unwrap_or_default();
                let val = parts.get(1).map(|s| s.trim()).unwrap_or("").to_uppercase();

                let matched: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter(|e| {
                        let c = e.common();
                        match prop.as_str() {
                            "TYPE" => crate::entities::names::dxf_name(e).to_uppercase() == val,
                            "LAYER" => c.layer.to_uppercase() == val,
                            "COLOR" => c
                                .color
                                .index()
                                .map(|n| n.to_string() == val)
                                .unwrap_or(val == "BYLAYER"),
                            "LINETYPE" => c.linetype.to_uppercase() == val,
                            _ => false,
                        }
                    })
                    .map(|e| e.common().handle)
                    .collect();

                if prop.is_empty() {
                    self.command_line
                        .push_info(crate::t!("Usage: QSELECT TYPE|LAYER|COLOR|LINETYPE <value>").as_ref());
                } else if matched.is_empty() {
                    self.command_line
                        .push_output(crate::t!("QSELECT: no matching entities.").as_ref());
                } else {
                    self.tabs[i].scene.deselect_all();
                    for h in &matched {
                        self.tabs[i].scene.select_entity(*h, false);
                    }
                    self.command_line
                        .push_output(crate::tf!("QSELECT: {} entity(ies) selected.", matched.len()).as_ref());
                    self.refresh_properties();
                }
            }

            // ── COUNT — entity statistics ─────────────────────────────────────
            "COUNT" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "COUNT",
                    "COUNT  tally  [All (by type) / by Layer]:",
                    vec![("All", "TYPE", None), ("By layer", "LAYER", None)],
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("COUNT ") => {
                let filter = cmd.split_once(' ').map(|(_, r)| r.trim().to_uppercase());
                let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
                for e in self.tabs[i].scene.document.entities() {
                    let layer = &e.common().layer;
                    let type_name = crate::entities::names::dxf_name(e);
                    let key = match &filter {
                        Some(f) if f == "LAYER" => layer.clone(),
                        Some(f) if f == "TYPE" => type_name.to_string(),
                        Some(f) => {
                            // Filter by layer name
                            if layer.to_uppercase() != *f {
                                continue;
                            }
                            type_name.to_string()
                        }
                        None => type_name.to_string(),
                    };
                    *counts.entry(key).or_default() += 1;
                }
                let total: usize = counts.values().sum();
                for (k, n) in &counts {
                    self.command_line.push_output(crate::tf!("  {k}: {n}").as_ref());
                }
                self.command_line
                    .push_output(crate::tf!("COUNT: {total} entity(ies) total.").as_ref());
            }

            "DATAEXTRACTION" | "EATTEXT" | "ATTEXT" => {
                let csv = build_data_extraction_csv(&self.tabs[i].scene.document);
                return Some(Task::done(Message::DataExtractionSave(csv)));
            }

            // ── Find / Replace ────────────────────────────────────────────────
            // FIND <search>              — list all Text/MText/Dimension containing <search>
            // FIND <search> REPLACE <rep> — replace first occurrence (case-insensitive)
            // FINDALL <search> REPLACE <rep> — replace all occurrences
            "FIND" => {
                return Some(Task::done(Message::FindReplaceOpen));
            }
            "FINDALL" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "FINDALL",
                    "FINDALL  text to find  (add REPLACE <text> by typing):",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("FIND ") || cmd.starts_with("FINDALL ") => {
                let all_mode = cmd.starts_with("FINDALL");
                let rest = cmd.split_once(' ').map(|(_, r)| r.trim()).unwrap_or("");

                // Split at " REPLACE " keyword (case-insensitive)
                let (search, replacement) = if let Some(pos) = rest.to_uppercase().find(" REPLACE ")
                {
                    (&rest[..pos], Some(rest[pos + 9..].trim()))
                } else {
                    (rest, None)
                };

                if search.is_empty() {
                    self.command_line.push_error(crate::t!("FIND: specify search text.").as_ref());
                } else {
                    let search_lc = search.to_lowercase();
                    let mut count = 0usize;
                    let handles: Vec<acadrust::Handle> = self.tabs[i]
                        .scene
                        .document
                        .entities()
                        .filter_map(|e| {
                            use crate::entities::traits::EntityTypeOps;
                            let txt = e.text_content()?;
                            if txt.to_lowercase().contains(&search_lc) {
                                Some(e.common().handle)
                            } else {
                                None
                            }
                        })
                        .collect();

                    if let Some(rep) = replacement {
                        // Replace mode
                        let targets: Vec<_> = if all_mode {
                            handles.clone()
                        } else {
                            handles.iter().copied().take(1).collect()
                        };
                        if targets.is_empty() {
                            self.command_line
                                .push_output(crate::tf!("FIND: \"{}\" not found.", search).as_ref());
                        } else {
                            self.push_undo_snapshot(i, "FIND/REPLACE");
                            for h in &targets {
                                if let Some(e) = self.tabs[i].scene.document.get_entity_mut(*h) {
                                    crate::entities::traits::EntityTypeOps::replace_text(
                                        e, search, rep,
                                    );
                                    count += 1;
                                }
                            }
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(crate::tf!(
                                "FIND/REPLACE: replaced {} occurrence(s) of \"{}\" → \"{}\".",
                                count, search, rep
                            ).as_ref());
                            self.refresh_properties();
                        }
                    } else {
                        // List mode
                        if handles.is_empty() {
                            self.command_line
                                .push_output(crate::tf!("FIND: \"{}\" not found.", search).as_ref());
                        } else {
                            for h in &handles {
                                if let Some(e) = self.tabs[i].scene.document.get_entity(*h) {
                                    use crate::entities::traits::EntityTypeOps;
                                    let txt = e.text_content().unwrap_or_default();
                                    self.command_line.push_output(crate::tf!(
                                        "  Handle {:X}: \"{}\"",
                                        h.value(),
                                        txt
                                    ).as_ref());
                                }
                            }
                            self.command_line.push_output(crate::tf!(
                                "FIND: {} match(es) for \"{}\".",
                                handles.len(),
                                search
                            ).as_ref());
                        }
                    }
                }
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }

    fn restore_after_block_edit_close(
        &mut self,
        i: usize,
        return_block: Option<&str>,
        return_layout: &str,
        return_camera: &crate::scene::view::camera::Camera,
    ) {
        self.tabs[i].scene.block_edit_block = None;
        self.tabs[i].scene.deselect_all();
        self.tabs[i].active_grip = None;
        self.grip_hover = None;
        self.grip_popup = None;
        self.visibility_popup = None;

        let parent_index = return_block.and_then(|parent_name| {
            self.tabs[i]
                .block_edits
                .iter()
                .position(|candidate| candidate.block_name == parent_name)
        });
        if let Some(parent_index) = parent_index {
            let (br_handle, editor_camera) = {
                let parent = &self.tabs[i].block_edits[parent_index];
                (parent.br_handle, parent.editor_camera.clone())
            };
            self.tabs[i].scene.set_current_layout("Model".to_string());
            self.tabs[i].scene.block_edit_block = Some(br_handle);
            self.tabs[i].active_block_edit = Some(parent_index);
            *self.tabs[i].scene.camera.borrow_mut() = editor_camera;
        } else {
            let return_layout = if self.tabs[i]
                .scene
                .layout_names()
                .iter()
                .any(|name| name == return_layout)
            {
                return_layout.to_string()
            } else {
                "Model".to_string()
            };
            self.tabs[i].active_block_edit = None;
            self.tabs[i].scene.set_current_layout(return_layout);
            *self.tabs[i].scene.camera.borrow_mut() = return_camera.clone();
        }

        self.tabs[i].scene.camera_generation += 1;
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        self.tabs[i].scene.rebuild_derived_caches();
        self.tabs[i].refresh_active_ucs();
        self.refresh_properties();
        self.adopt_view_display(i);
        self.sync_dyn_fields();
    }

    /// ADDSELECTED — start the draw command that creates the same kind of
    /// object as the currently-selected one, adopting its general properties
    /// (layer, colour, linetype, lineweight, linetype scale) as the current
    /// defaults so the new object is drawn to match. Issue #239.
    pub(super) fn cmd_add_selected(&mut self, i: usize) -> Task<Message> {
        // Use the first selected object as the template.
        let Some(handle) = self.tabs[i].scene.selected.iter().next().copied() else {
            self.command_line.push_info(
                "ADDSELECTED: select an object first, then run ADDSELECTED to draw a new one like it.",
            );
            return Task::none();
        };
        // Pull the template's type + general properties into owned values, then
        // drop the borrow so the document can be mutated below.
        let info = self.tabs[i].scene.document.get_entity(handle).map(|e| {
            let c = e.common();
            (
                add_selected_verb(e),
                crate::entities::names::dxf_name(e).to_string(),
                c.layer.clone(),
                c.color,
                c.linetype.clone(),
                c.linetype_scale,
                c.line_weight,
                // A dimension template also carries its dimension style, adopted
                // as the current DIMSTYLE so the cloned dimension matches (#239).
                match e {
                    acadrust::EntityType::Dimension(d) => Some(d.base().style_name.clone()),
                    _ => None,
                },
            )
        });
        let Some((verb, kind, layer, color, linetype, lt_scale, lw, template_dimstyle)) = info
        else {
            self.command_line
                .push_error(crate::t!("ADDSELECTED: selected object not found.").as_ref());
            return Task::none();
        };
        let Some(verb) = verb else {
            self.command_line
                .push_error(crate::tf!("ADDSELECTED: creating a new {kind} is not supported.").as_ref());
            return Task::none();
        };

        // Clear the selection so adopting the properties as defaults doesn't
        // rewrite the template, and so the draw starts on a clean slate.
        self.tabs[i].scene.deselect_all();

        // Snapshot the current drawing defaults so the override below reverts
        // once the launched draw command ends — ADDSELECTED must adopt the
        // template's properties for the new object without permanently changing
        // CLAYER / CECOLOR / CELTYPE / CELWEIGHT (issue #239).
        let restore = crate::app::AddSelectedRestore {
            layer_name: self.tabs[i].scene.document.header.current_layer_name.clone(),
            layer_handle: self.tabs[i].scene.document.header.current_layer_handle,
            color: self.tabs[i].scene.document.header.current_entity_color,
            linetype_name: self.tabs[i].scene.document.header.current_linetype_name.clone(),
            linetype_handle: self.tabs[i].scene.document.header.current_linetype_handle,
            line_weight: self.tabs[i].scene.document.header.current_line_weight,
            lt_scale: self.tabs[i].scene.document.header.current_entity_linetype_scale,
            dimstyle_name: self.tabs[i].scene.document.header.current_dimstyle_name.clone(),
            dimstyle_handle: self.tabs[i].scene.document.header.current_dimstyle_handle,
            tab_active_layer: self.tabs[i].active_layer.clone(),
            tab_layers_current: self.tabs[i].layers.current_layer.clone(),
            ribbon_layer: self.ribbon.active_layer.clone(),
            ribbon_color: self.ribbon.active_color,
            ribbon_linetype: self.ribbon.active_linetype.clone(),
            ribbon_lineweight: self.ribbon.active_lineweight,
        };
        self.add_selected_restore = Some(restore);

        // Adopt the template's general properties as the current defaults. The
        // entity-creation path stamps new objects from the tab's active layer
        // and the ribbon's active colour / linetype / lineweight, mirrored into
        // the header so they persist (CLAYER / CECOLOR / CELTYPE / CELWEIGHT /
        // CELTSCALE).
        let layer_handle = self.tabs[i]
            .scene
            .document
            .layers
            .get(&layer)
            .map(|l| l.handle)
            .unwrap_or(acadrust::types::Handle::NULL);
        let lt_handle = self.tabs[i]
            .scene
            .document
            .line_types
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(&linetype))
            .map(|x| x.handle)
            .unwrap_or(acadrust::types::Handle::NULL);
        {
            let header = &mut self.tabs[i].scene.document.header;
            header.current_layer_name = layer.clone();
            header.current_layer_handle = layer_handle;
            header.current_entity_color = color;
            header.current_linetype_name = linetype.clone();
            header.current_linetype_handle = lt_handle;
            header.current_line_weight = lw.value();
            header.current_entity_linetype_scale = lt_scale;
        }
        // A dimension template also sets the current DIMSTYLE so the cloned
        // dimension inherits the template's dimension style (#239).
        if let Some(ds) = &template_dimstyle {
            let ds_handle = self.tabs[i]
                .scene
                .document
                .dim_styles
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(ds))
                .map(|s| s.handle)
                .unwrap_or(acadrust::types::Handle::NULL);
            let header = &mut self.tabs[i].scene.document.header;
            header.current_dimstyle_name = ds.clone();
            header.current_dimstyle_handle = ds_handle;
        }
        self.tabs[i].active_layer = layer.clone();
        self.tabs[i].layers.current_layer = layer.clone();
        self.tabs[i].dirty = true;
        self.ribbon.active_layer = layer.clone();
        self.ribbon.active_color = color;
        self.ribbon.active_linetype = linetype;
        self.ribbon.active_lineweight = lw;
        self.refresh_properties();

        self.command_line.push_output(crate::tf!(
            "Add Selected: drawing a new {kind} on layer \"{layer}\"."
        ).as_ref());
        // Launch the matching draw command (installs its interactive step).
        self.dispatch_command(verb)
    }

    /// Restore the drawing defaults ADDSELECTED overrode, once the draw command
    /// it launched ends (commit / cancel / interrupt). No-op unless an
    /// ADDSELECTED override is pending. Issue #239.
    pub(crate) fn restore_add_selected_defaults(&mut self) {
        let Some(r) = self.add_selected_restore.take() else {
            return;
        };
        let i = self.active_tab;
        {
            let h = &mut self.tabs[i].scene.document.header;
            h.current_layer_name = r.layer_name;
            h.current_layer_handle = r.layer_handle;
            h.current_entity_color = r.color;
            h.current_linetype_name = r.linetype_name;
            h.current_linetype_handle = r.linetype_handle;
            h.current_line_weight = r.line_weight;
            h.current_entity_linetype_scale = r.lt_scale;
            h.current_dimstyle_name = r.dimstyle_name;
            h.current_dimstyle_handle = r.dimstyle_handle;
        }
        self.tabs[i].active_layer = r.tab_active_layer;
        self.tabs[i].layers.current_layer = r.tab_layers_current;
        self.ribbon.active_layer = r.ribbon_layer;
        self.ribbon.active_color = r.ribbon_color;
        self.ribbon.active_linetype = r.ribbon_linetype;
        self.ribbon.active_lineweight = r.ribbon_lineweight;
        self.refresh_properties();
    }
}

/// Map a template entity to the draw-command verb that creates the same kind of
/// object, or `None` when there is no interactive creator for it. Used by
/// ADDSELECTED (issue #239).
fn add_selected_verb(entity: &acadrust::EntityType) -> Option<&'static str> {
    use acadrust::EntityType;
    Some(match entity {
        EntityType::Point(_) => "POINT",
        EntityType::Line(_) => "LINE",
        EntityType::Circle(_) => "CIRCLE",
        EntityType::Arc(_) => "ARC",
        EntityType::Ellipse(_) => "ELLIPSE",
        EntityType::LwPolyline(_)
        | EntityType::Polyline(_)
        | EntityType::Polyline2D(_)
        | EntityType::Polyline3D(_) => "PLINE",
        EntityType::Text(_) => "TEXT",
        EntityType::MText(_) => "MTEXT",
        EntityType::Spline(_) => "SPLINE",
        EntityType::Hatch(_) => "HATCH",
        EntityType::Solid(_) => "SOLID",
        EntityType::Ray(_) => "RAY",
        EntityType::XLine(_) => "XLINE",
        // Dimensions launch the matching dimension command by their stored type
        // (issue #239). Angular 2-line and 3-point both use DIMANGULAR.
        EntityType::Dimension(d) => {
            use acadrust::entities::DimensionType;
            match d.base().dimension_type {
                DimensionType::Linear => "DIMLINEAR",
                DimensionType::Aligned => "DIMALIGNED",
                DimensionType::Angular | DimensionType::Angular3Point => "DIMANGULAR",
                DimensionType::Diameter => "DIMDIAMETER",
                DimensionType::Radius => "DIMRADIUS",
                DimensionType::Ordinate => "DIMORDINATE",
                DimensionType::ArcLength => "DIMARC",
                DimensionType::LargeRadial => "DIMJOGGED",
            }
        }
        _ => return None,
    })
}

fn entity_list_details(entity: &acadrust::EntityType) -> String {
    use std::f64::consts::PI;
    match entity {
        acadrust::EntityType::Line(l) => format!(
            "from ({:.4},{:.4},{:.4}) to ({:.4},{:.4},{:.4})  len={:.4}",
            l.start.x,
            l.start.y,
            l.start.z,
            l.end.x,
            l.end.y,
            l.end.z,
            ((l.end.x - l.start.x).powi(2)
                + (l.end.y - l.start.y).powi(2)
                + (l.end.z - l.start.z).powi(2))
            .sqrt()
        ),
        acadrust::EntityType::Circle(c) => format!(
            "center ({:.4},{:.4},{:.4})  r={:.4}  area={:.4}",
            c.center.x,
            c.center.y,
            c.center.z,
            c.radius,
            PI * c.radius * c.radius
        ),
        acadrust::EntityType::Arc(a) => format!(
            "center ({:.4},{:.4},{:.4})  r={:.4}  start={:.2}° end={:.2}°",
            a.center.x,
            a.center.y,
            a.center.z,
            a.radius,
            a.start_angle.to_degrees(),
            a.end_angle.to_degrees()
        ),
        acadrust::EntityType::LwPolyline(p) => format!(
            "{} vertices  closed={}  elevation={:.4}",
            p.vertices.len(),
            p.is_closed,
            p.elevation
        ),
        acadrust::EntityType::Text(t) => format!(
            "\"{}\"  h={:.4}  at ({:.4},{:.4})",
            t.value, t.height, t.insertion_point.x, t.insertion_point.y
        ),
        acadrust::EntityType::MText(t) => format!(
            "\"{}\"  h={:.4}  at ({:.4},{:.4})",
            t.value.chars().take(40).collect::<String>(),
            t.height,
            t.insertion_point.x,
            t.insertion_point.y
        ),
        acadrust::EntityType::Insert(ins) => format!(
            "block=\"{}\"  at ({:.4},{:.4},{:.4})  scale=({:.4},{:.4},{:.4})  rot={:.2}°",
            ins.block_name,
            ins.insert_point.x,
            ins.insert_point.y,
            ins.insert_point.z,
            ins.x_scale(),
            ins.y_scale(),
            ins.z_scale(),
            ins.rotation.to_degrees()
        ),
        acadrust::EntityType::Spline(s) => format!(
            "{} ctrl pts  degree={}  closed={}",
            s.control_points.len(),
            s.degree,
            s.flags.closed
        ),
        acadrust::EntityType::Ellipse(e) => format!(
            "center ({:.4},{:.4})  major_len={:.4}  ratio={:.4}",
            e.center.x,
            e.center.y,
            e.major_axis_length(),
            e.minor_axis_ratio
        ),
        _ => String::new(),
    }
}

fn flatten_entity_z(entity: &mut acadrust::EntityType) {
    match entity {
        acadrust::EntityType::Line(l) => {
            l.start.z = 0.0;
            l.end.z = 0.0;
        }
        acadrust::EntityType::Circle(c) => {
            c.center.z = 0.0;
        }
        acadrust::EntityType::Arc(a) => {
            a.center.z = 0.0;
        }
        acadrust::EntityType::LwPolyline(p) => {
            p.elevation = 0.0;
        }
        acadrust::EntityType::Text(t) => {
            t.insertion_point.z = 0.0;
        }
        acadrust::EntityType::MText(t) => {
            t.insertion_point.z = 0.0;
        }
        acadrust::EntityType::Insert(ins) => {
            ins.insert_point.z = 0.0;
        }
        acadrust::EntityType::Point(p) => {
            p.location.z = 0.0;
        }
        acadrust::EntityType::Spline(s) => {
            for cp in &mut s.control_points {
                cp.z = 0.0;
            }
            for fp in &mut s.fit_points {
                fp.z = 0.0;
            }
        }
        acadrust::EntityType::Ellipse(e) => {
            e.center.z = 0.0;
        }
        _ => {}
    }
}

// ── DATAEXTRACTION ─────────────────────────────────────────────────────────

/// Build a CSV string with one row per entity in model space.
/// Columns: Type, Handle, Layer, Color, Linetype, ExtraInfo
fn build_data_extraction_csv(doc: &acadrust::CadDocument) -> String {
    use acadrust::EntityType;

    let mut out = String::from("Type,Handle,Layer,Color,Linetype,ExtraInfo\n");

    let ms_handle = doc.header.model_space_block_handle;
    for e in doc.entities() {
        // Skip Block/EndBlock sentinels and paper-space entities.
        if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
            continue;
        }
        if !ms_handle.is_null() && e.common().owner_handle != ms_handle {
            continue;
        }
        let type_name = crate::entities::names::dxf_name(e);
        let handle = format!("{:X}", e.common().handle.value());
        let layer = csv_escape(&e.common().layer);
        let color = format!("{}", e.common().color);
        let lt = csv_escape(&e.common().linetype);
        let extra = csv_escape(&entity_extra_info(e));
        out.push_str(&format!(
            "{type_name},{handle},{layer},{color},{lt},{extra}\n"
        ));
    }
    out
}

/// Return a short geometry summary for CSV ExtraInfo column.
fn entity_extra_info(entity: &acadrust::EntityType) -> String {
    use acadrust::EntityType;
    match entity {
        EntityType::Line(e) => format!(
            "({:.3},{:.3})-({:.3},{:.3})",
            e.start.x, e.start.y, e.end.x, e.end.y
        ),
        EntityType::Circle(e) => {
            format!("C({:.3},{:.3}) R={:.3}", e.center.x, e.center.y, e.radius)
        }
        EntityType::Arc(e) => format!(
            "C({:.3},{:.3}) R={:.3} {:.1}°-{:.1}°",
            e.center.x,
            e.center.y,
            e.radius,
            e.start_angle.to_degrees(),
            e.end_angle.to_degrees()
        ),
        EntityType::Text(e) => e.value.clone(),
        EntityType::MText(e) => e.value.chars().take(60).collect(),
        EntityType::Insert(e) => format!(
            "BLK={} @({:.3},{:.3})",
            e.block_name, e.insert_point.x, e.insert_point.y
        ),
        EntityType::LwPolyline(e) => format!("{} vertices", e.vertices.len()),
        EntityType::Polyline(e) => format!("{} vertices", e.vertices.len()),
        EntityType::Polyline2D(e) => format!("{} vertices", e.vertices.len()),
        EntityType::Polyline3D(e) => format!("{} vertices", e.vertices.len()),
        EntityType::Hatch(e) => format!("PAT={}", e.pattern.name),
        EntityType::Dimension(e) => format!("{:.3}", e.base().actual_measurement),
        EntityType::Spline(e) => format!("{} ctrl pts", e.control_points.len()),
        _ => String::new(),
    }
}

/// Escape a string for a CSV field (wrap in quotes if it contains comma/quote/newline).
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ── CAL — arithmetic expression evaluator ──────────────────────────────────
// A small recursive-descent parser for `+ - * /`, parentheses, unary signs and
// decimal numbers. Self-contained (no external dependency).
fn arith_eval(expr: &str) -> Result<f64, String> {
    let mut p = ArithParser {
        chars: expr.chars().filter(|c| !c.is_whitespace()).collect(),
        pos: 0,
    };
    let v = p.expr()?;
    if p.pos != p.chars.len() {
        return Err(format!("unexpected '{}'", p.chars[p.pos]));
    }
    Ok(v)
}

impl OpenCADStudio {
    /// Open the attribute editor. If a single block with attributes is already
    /// selected it opens directly on that block; otherwise it starts the
    /// block-pick command and opens once a block is chosen (see
    /// `command_driver`). Shared by ATTEDIT and its command aliases
    /// ATTMAN / BATTMAN.
    pub(in crate::app) fn open_attedit_dialog(&mut self) {
        let i = self.active_tab;
        let selected_attr_insert = {
            let sel = self.tabs[i].scene.selected_entities();
            if sel.len() == 1 {
                let (h, e) = sel[0];
                match e {
                    acadrust::EntityType::Insert(ins) if !ins.attributes.is_empty() => Some(h),
                    _ => None,
                }
            } else {
                None
            }
        };
        if let Some(handle) = selected_attr_insert {
            self.open_attribute_editor(handle);
        } else {
            use crate::modules::draw::modify::attedit::AtteditCommand;
            let cmd_obj = AtteditCommand::new();
            self.command_line.push_info(&cmd_obj.prompt());
            self.tabs[i].active_cmd = Some(Box::new(cmd_obj));
        }
    }
}

struct ArithParser {
    chars: Vec<char>,
    pos: usize,
}

impl ArithParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    // expr = term (('+' | '-') term)*
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(op) = self.peek() {
            if op == '+' || op == '-' {
                self.pos += 1;
                let rhs = self.term()?;
                v = if op == '+' { v + rhs } else { v - rhs };
            } else {
                break;
            }
        }
        Ok(v)
    }

    // term = factor (('*' | '/') factor)*
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.factor()?;
        while let Some(op) = self.peek() {
            if op == '*' || op == '/' {
                self.pos += 1;
                let rhs = self.factor()?;
                if op == '/' {
                    if rhs == 0.0 {
                        return Err("division by zero".into());
                    }
                    v /= rhs;
                } else {
                    v *= rhs;
                }
            } else {
                break;
            }
        }
        Ok(v)
    }

    // factor = ('+' | '-') factor | '(' expr ')' | number
    fn factor(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some('+') => {
                self.pos += 1;
                self.factor()
            }
            Some('-') => {
                self.pos += 1;
                Ok(-self.factor()?)
            }
            Some('(') => {
                self.pos += 1;
                let v = self.expr()?;
                if self.peek() != Some(')') {
                    return Err("missing ')'".into());
                }
                self.pos += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(c) => Err(format!("unexpected '{c}'")),
            None => Err("unexpected end of expression".into()),
        }
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f64>().map_err(|_| format!("bad number '{s}'"))
    }
}
