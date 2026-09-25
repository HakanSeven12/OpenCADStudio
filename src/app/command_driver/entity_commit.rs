use super::*;

impl OpenCADStudio {
    pub(super) fn handle_commit_entity(
        &mut self,
        entity: codec::EntityType,
        preserve_commit_layer: bool,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        let source_handle = entity.common().handle;
        if !source_handle.is_null()
            && self.tabs[i]
                .scene
                .document
                .get_entity(source_handle)
                .is_some()
            && self.reject_locked_edit(i, source_handle)
        {
            return Some(Task::none());
        }
        // A line/arc drawn by a repeating command advances the ARC_CONT
        // continuation anchor, so ending one run and launching another
        // keeps continuing from the last segment (mirrors the
        // CommitAndExit arm). Non-continuable repeats (point/ray)
        // leave the anchor untouched. (#327)
        if matches!(
            entity,
            codec::EntityType::Line(_)
                | codec::EntityType::Arc(_)
                | codec::EntityType::LwPolyline(_)
                | codec::EntityType::Polyline2D(_)
        ) {
            self.update_cont_anchor(&entity);
        }
        let label = self.history_label_from_active_cmd(i, "ENTITY");
        // Ordinary drawables, viewports and raster images use targeted
        // entity/object deltas. Block sentinels and novel layers retain
        // the structure snapshot fallback.
        let delta_safe = self.delta_add_safe(i, &entity);
        let pending = self.begin_undo(i, label, 1, delta_safe);
        let is_associative_dimension = matches!(
            entity,
            codec::EntityType::Dimension(
                codec::entities::Dimension::Linear(_)
                    | codec::entities::Dimension::Aligned(_)
            )
        );
        let association_enabled =
            self.tabs[i].scene.document.header.dimension_associativity == 2;
        let committed = if preserve_commit_layer {
            self.commit_entity_handle_preserve_layer(entity)
        } else {
            self.commit_entity_handle(entity)
        };
        if is_associative_dimension && association_enabled {
            if let Some(handle) = committed {
                let sources = self.infer_dimension_sources_guarded(i, handle);
                self.tabs[i]
                    .scene
                    .attach_dimension_association(handle, sources);
            }
        }
        if let Some(handle) = committed {
            self.apply_continuous_constraints(i, &[handle]);
        }
        self.tabs[i].dirty = true;
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_commit_entities(
        &mut self,
        mut entities: Vec<codec::EntityType>,
        preserve_commit_style: bool,
        preserve_commit_layer: bool,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        let locked_source = entities.iter().find_map(|entity| {
            let handle = entity.common().handle;
            (!handle.is_null()
                && self.tabs[i].scene.document.get_entity(handle).is_some()
                && self.tabs[i].scene.is_layer_locked(handle))
            .then_some(handle)
        });
        if let Some(handle) = locked_source {
            self.reject_locked_edit(i, handle);
            return Some(Task::none());
        }
        let label = self.history_label_from_active_cmd(i, "ENTITY");
        let symbol_names = self.tabs[i]
            .active_cmd
            .as_ref()
            .and_then(|command| command.nested_copy_symbol_names())
            .cloned();
        let delta_safe = symbol_names.is_none()
            && entities.iter().all(|entity| self.delta_add_safe(i, entity));
        let pending = self.begin_undo(i, label, entities.len(), delta_safe);
        if let Some(names) = symbol_names {
            let retained = self.tabs[i]
                .scene
                .document
                .localize_nested_copy_symbols(&mut entities, &names);
            if retained > 0 {
                self.command_line.push_info(&format!("{} copied object(s) retain imported style references whose dependencies cannot be localized.", retained));
            }
            self.refresh_layer_panel();
        }
        let mut committed = Vec::new();
        for entity in entities {
            let handle = if preserve_commit_style {
                self.commit_entity_handle_preserve_style(entity)
            } else if preserve_commit_layer {
                self.commit_entity_handle_preserve_layer(entity)
            } else {
                self.commit_entity_handle(entity)
            };
            committed.extend(handle);
        }
        self.apply_continuous_constraints(i, &committed);
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_commit_entities_and_exit(
        &mut self,
        mut entities: Vec<codec::EntityType>,
        preserve_commit_style: bool,
        preserve_commit_layer: bool,
    ) {
        let i = self.active_tab;
        let label = self.history_label_from_active_cmd(i, "ENTITY");
        let symbol_names = self.tabs[i]
            .active_cmd
            .as_ref()
            .and_then(|command| command.nested_copy_symbol_names())
            .cloned();
        let delta_safe = symbol_names.is_none()
            && entities.iter().all(|entity| self.delta_add_safe(i, entity));
        let pending = self.begin_undo(i, label, entities.len(), delta_safe);
        if let Some(names) = symbol_names {
            let retained = self.tabs[i]
                .scene
                .document
                .localize_nested_copy_symbols(&mut entities, &names);
            if retained > 0 {
                self.command_line.push_info(&format!("{} copied object(s) retain imported style references whose dependencies cannot be localized.", retained));
            }
            self.refresh_layer_panel();
        }
        let mut committed = Vec::new();
        for entity in entities {
            let handle = if preserve_commit_style {
                self.commit_entity_handle_preserve_style(entity)
            } else if preserve_commit_layer {
                self.commit_entity_handle_preserve_layer(entity)
            } else {
                self.commit_entity_handle(entity)
            };
            committed.extend(handle);
        }
        self.apply_continuous_constraints(i, &committed);
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    pub(super) fn handle_commit_and_exit(
        &mut self,
        entity: codec::EntityType,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        // XATTACH: the definition and its INSERT are created together
        // in one undo step.
        let xattach_request = self.tabs[i]
            .active_cmd
            .as_ref()
            .filter(|cmd| cmd.name() == "XATTACH")
            .and_then(|cmd| cmd.xattach_request());
        if let Some(request) = xattach_request {
            self.commit_xref_attach(i, request, entity);
            return Some(Task::none());
        }
        let insert_block_name = match &entity {
            codec::EntityType::Insert(ins) => Some(ins.block_name.clone()),
            _ => None,
        };
        // Record where this draw ended so ARC_CONT can continue from it
        // (before `entity` is moved into commit_entity).
        self.update_cont_anchor(&entity);
        let label = self.history_label_from_active_cmd(i, "ENTITY");
        let delta_safe = self.delta_add_safe(i, &entity);
        let pending = self.begin_undo(i, label, 1, delta_safe);
        let is_associative_dimension = matches!(
            entity,
            codec::EntityType::Dimension(
                codec::entities::Dimension::Linear(_)
                    | codec::entities::Dimension::Aligned(_)
            )
        );
        let association_enabled =
            self.tabs[i].scene.document.header.dimension_associativity == 2;
        let committed = self.commit_entity_handle(entity);
        if is_associative_dimension && association_enabled {
            if let Some(handle) = committed {
                let sources = self.infer_dimension_sources_guarded(i, handle);
                self.tabs[i]
                    .scene
                    .attach_dimension_association(handle, sources);
            }
        }
        if let Some(handle) = committed {
            self.apply_continuous_constraints(i, &[handle]);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        if let Some(name) = insert_block_name {
            self.record_block_insert(&name);
        }
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_commit_dimension(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::CommitDimension {
            mut entity,
            association,
            preserve_base_style,
            continue_command,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        let label = self.history_label_from_active_cmd(i, "DIMENSION");
        let association_mode = self.tabs[i].scene.document.header.dimension_associativity;
        let single_source_dimension = matches!(
            &entity,
            codec::EntityType::Dimension(codec::entities::Dimension::Ordinate(_))
        );
        // A dimension placed on the sheet but measuring model geometry
        // through a viewport carries the compensation as a negative
        // DIMLFAC override.
        if !preserve_base_style {
            crate::scene::creation_style::apply_current_creation_styles(
                &self.tabs[i].scene.document,
                &mut entity,
            );
        }
        if !self.apply_viewport_dimension_measurement(i, &mut entity) {
            return Some(Task::none());
        }
        // Projected points cannot use direct paper-space source inference.
        let association_allowed = self.dimension_association_allowed(i);
        let inherited_dimension = if preserve_base_style {
            match &entity {
                codec::EntityType::Dimension(dimension) => Some((
                    dimension.base().common.layer.clone(),
                    dimension.base().style_name.clone(),
                )),
                _ => None,
            }
        } else {
            None
        };
        let pending = if association_mode == 0 {
            let layer = self.tabs[i].active_layer.clone();
            if layer != "0" || entity.as_entity().layer().is_empty() {
                entity.as_entity_mut().set_layer(layer);
            }
            crate::scene::view::dispatch::apply_color(
                &mut entity,
                self.ribbon.active_color,
            );
            crate::scene::view::dispatch::apply_common_prop(
                &mut entity,
                "linetype",
                &self.ribbon.active_linetype.clone(),
            );
            crate::scene::view::dispatch::apply_line_weight(
                &mut entity,
                self.ribbon.active_lineweight,
            );
            let celtscale = self.tabs[i]
                .scene
                .document
                .header
                .current_entity_linetype_scale;
            if (celtscale - 1.0).abs() > 1e-9 && celtscale.abs() > 1e-9 {
                entity.common_mut().linetype_scale = celtscale;
            }
            crate::scene::creation_style::apply_current_creation_styles(
                &self.tabs[i].scene.document,
                &mut entity,
            );
            if let (Some((layer, style_name)), codec::EntityType::Dimension(dimension)) =
                (inherited_dimension, &mut entity)
            {
                dimension.base_mut().common.layer = layer;
                dimension.base_mut().style_name = style_name;
            }
            let pieces = crate::modules::draw::modify::explode::explode_entity(
                &entity,
                &self.tabs[i].scene.document,
            );
            let delta_safe = pieces.iter().all(|piece| self.delta_add_safe(i, piece));
            let pending = self.begin_undo(i, label, pieces.len(), delta_safe);
            for piece in pieces {
                self.tabs[i].scene.add_entity(piece);
            }
            pending
        } else {
            let delta_safe = self.delta_add_safe(i, &entity);
            let pending = self.begin_undo(i, label, 1, delta_safe);
            if let Some(handle) =
                self.commit_entity_handle_with_dimension_policy(entity, preserve_base_style)
            {
                if association_mode == 2 && association_allowed {
                    let mut changes = vec![(handle, crate::scene::ChangeKind::Modified)];
                    match association {
                        crate::command::DimensionAssociationInput::Infer(source) => {
                            let sources: Vec<_> = source.map_or_else(
                                || self.infer_dimension_sources_guarded(i, handle),
                                |source| {
                                    if single_source_dimension {
                                        vec![Some(source)]
                                    } else {
                                        vec![Some(source), Some(source)]
                                    }
                                },
                            );
                            self.tabs[i]
                                .scene
                                .attach_dimension_association(handle, sources.clone());
                            changes.extend(sources.into_iter().flatten().map(|source| {
                                (source, crate::scene::ChangeKind::Modified)
                            }));
                        }
                        crate::command::DimensionAssociationInput::Explicit(sources) => {
                            self.tabs[i].scene.attach_dimension_association_sources(
                                handle,
                                sources.clone(),
                            );
                            changes.extend(sources.into_iter().flatten().map(|source| {
                                (source.handle, crate::scene::ChangeKind::Modified)
                            }));
                        }
                    }
                    changes.sort_by_key(|(handle, _)| handle.value());
                    changes.dedup_by_key(|(handle, _)| handle.value());
                    self.tabs[i].scene.bump_entities(&changes);
                } else if association_mode == 2 {
                    // Preserve the acquired viewport and source paths.
                    self.attach_viewport_dimension_association(i, handle);
                }
            }
            pending
        };
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].snap_result = None;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        if continue_command {
            if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|cmd| cmd.prompt()) {
                self.command_line.push_info(&prompt);
            }
        } else {
            self.tabs[i].active_cmd = None;
            self.restore_pre_cmd_tangent();
        }
        None
    }

    pub(super) fn handle_commit_dimensions_and_exit(
        &mut self,
        dimensions: Vec<(
            codec::EntityType,
            crate::command::DimensionAssociationInput,
        )>,
    ) {
        let i = self.active_tab;
        let association_mode = self.tabs[i].scene.document.header.dimension_associativity;
        let mut made = 0;
        let pending = if association_mode == 0 {
            let mut pieces = Vec::new();
            for (mut entity, _) in dimensions {
                let layer = self.tabs[i].active_layer.clone();
                if layer != "0" || entity.as_entity().layer().is_empty() {
                    entity.as_entity_mut().set_layer(layer);
                }
                crate::scene::view::dispatch::apply_color(
                    &mut entity,
                    self.ribbon.active_color,
                );
                crate::scene::view::dispatch::apply_common_prop(
                    &mut entity,
                    "linetype",
                    &self.ribbon.active_linetype.clone(),
                );
                crate::scene::view::dispatch::apply_line_weight(
                    &mut entity,
                    self.ribbon.active_lineweight,
                );
                let celtscale = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_entity_linetype_scale;
                if (celtscale - 1.0).abs() > 1e-9 && celtscale.abs() > 1e-9 {
                    entity.common_mut().linetype_scale = celtscale;
                }
                crate::scene::creation_style::apply_current_creation_styles(
                    &self.tabs[i].scene.document,
                    &mut entity,
                );
                let exploded = crate::modules::draw::modify::explode::explode_entity(
                    &entity,
                    &self.tabs[i].scene.document,
                );
                if !exploded.is_empty() {
                    made += 1;
                }
                pieces.extend(exploded);
            }
            let delta_safe = pieces.iter().all(|piece| self.delta_add_safe(i, piece));
            let pending = self.begin_undo(i, "QDIM", pieces.len(), delta_safe);
            for piece in pieces {
                self.tabs[i].scene.add_entity(piece);
            }
            pending
        } else {
            let delta_safe = dimensions
                .iter()
                .all(|(entity, _)| self.delta_add_safe(i, entity));
            let pending = self.begin_undo(i, "QDIM", dimensions.len(), delta_safe);
            for (entity, association) in dimensions {
                let Some(handle) = self.commit_entity_handle(entity) else {
                    continue;
                };
                made += 1;
                if association_mode != 2 {
                    continue;
                }
                let sources = match association {
                    crate::command::DimensionAssociationInput::Infer(source) => {
                        source.map_or_else(
                            || {
                                self.infer_dimension_sources_guarded(i, handle)
                                    .into_iter()
                                    .map(|source| {
                                        source.map(
                                            crate::command::DimensionAssociationSource::inferred,
                                        )
                                    })
                                    .collect()
                            },
                            |source| {
                                vec![Some(
                                    crate::command::DimensionAssociationSource::inferred(
                                        source,
                                    ),
                                )]
                            },
                        )
                    }
                    crate::command::DimensionAssociationInput::Explicit(sources) => {
                        sources
                    }
                };
                self.tabs[i]
                    .scene
                    .attach_dimension_association_sources(handle, sources.clone());
                let mut changes = vec![(handle, crate::scene::ChangeKind::Modified)];
                changes.extend(
                    sources
                        .into_iter()
                        .flatten()
                        .map(|source| (source.handle, crate::scene::ChangeKind::Modified)),
                );
                changes.sort_by_key(|(handle, _)| handle.value());
                changes.dedup_by_key(|(handle, _)| handle.value());
                self.tabs[i].scene.bump_entities(&changes);
            }
            pending
        };
        if made > 0 {
            self.tabs[i].dirty = true;
        }
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.command_line
            .push_output(crate::tf!("QDIM  {made} dimensions created.").as_ref());
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    pub(super) fn handle_commit_solid(&mut self, result: CmdResult) {
        let i = self.active_tab;
        let CmdResult::CommitSolid {
            entity,
            solid,
            history,
            erase_source,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        let label = self.history_label_from_active_cmd(i, "SOLID");
        let erase_source = erase_source.filter(|handle| {
            self.delete_objects != 0 && !self.tabs[i].scene.is_layer_locked(*handle)
        });
        let pending =
            self.begin_undo(i, label, 1 + usize::from(erase_source.is_some()), true);
        let handle = self.add_solid_model(entity, *solid, history);
        if !handle.is_null() {
            if let Some(source) = erase_source {
                self.tabs[i].scene.erase_entities(&[source]);
            }
            self.tabs[i].dirty = true;
            if let Some(pd) = pending {
                self.commit_undo_delta(i, pd);
            }
        }
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
    }

    pub(super) fn handle_commit_and_edit_text(
        &mut self,
        entity: codec::EntityType,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        let annotative_mleader = matches!(
            &entity,
            codec::EntityType::MultiLeader(ml) if ml.enable_annotation_scale
        );
        let label = self.history_label_from_active_cmd(i, "ENTITY");
        let delta_safe = self.delta_add_safe(i, &entity) && !annotative_mleader;
        let pending = self.begin_undo(i, label, 1, delta_safe);
        let handle = self.commit_entity_handle(entity);

        if annotative_mleader {
            let scale_handle = self.tabs[i].scene.creation_annotation_scale_handle();
            if let (Some(handle), Some(scale_handle)) = (handle, scale_handle) {
                crate::scene::annotative::create_annotation_context(
                    &mut self.tabs[i].scene.document,
                    handle,
                    scale_handle,
                );
                self.tabs[i]
                    .scene
                    .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
            }
        }

        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.ribbon.deactivate_tool();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        if let Some(h) = handle {
            return Some(self.begin_text_edit(h));
        }
        None
    }

    pub(super) fn handle_commit_many_and_edit_text(
        &mut self,
        entities: Vec<codec::EntityType>,
        edit_index: usize,
        open_editor: bool,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        let label = self.history_label_from_active_cmd(i, "ENTITY");
        let delta_safe = entities.iter().all(|entity| self.delta_add_safe(i, entity));
        let pending = self.begin_undo(i, label, entities.len(), delta_safe);
        let mut edit_handle = None;
        let mut leader_handle = None;
        for (idx, entity) in entities.into_iter().enumerate() {
            let is_leader = matches!(entity, codec::EntityType::Leader(_));
            let h = self.commit_entity_handle(entity);
            if idx == edit_index {
                edit_handle = h;
            }
            if is_leader {
                leader_handle = h;
            }
        }
        // Link the leader to its annotation so the pair edits as a unit
        // (double-click on the leader resolves to the text entity).
        if let (Some(lh), Some(ah)) = (leader_handle, edit_handle) {
            let linked = if let Some(codec::EntityType::Leader(l)) =
                self.tabs[i].scene.document.get_entity_mut(lh)
            {
                l.annotation_handle = ah;
                true
            } else {
                false
            };

            if linked {
                // The LEADER may already have received its annotation context while
                // annotation_handle was still NULL. Refresh it now that the MTEXT link
                // is known so the context represents the finished leader.
                self.tabs[i].scene.sync_displayed_annotation_context(lh);

                self.tabs[i]
                    .scene
                    .bump_entities(&[(lh, crate::scene::ChangeKind::Modified)]);
            }
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.ribbon.deactivate_tool();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        if open_editor {
            if let Some(h) = edit_handle {
                return Some(self.begin_text_edit(h));
            }
        }
        None
    }

    pub(super) fn handle_create_block(
        &mut self,
        mut handles: Vec<Handle>,
        name: String,
        base: glam::DVec3,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if handles.is_empty() {
            self.command_line
                .push_info(crate::t!("No editable objects selected.").as_ref());
            return Some(Task::none());
        }
        self.push_undo_snapshot(i, "BLOCK");
        let ucs = self.tabs[i].ucs_xform();
        let world_to_block = ucs.to_ucs_transform_at(base);
        let block_to_world = ucs.to_wcs_transform_at(base);
        match self.tabs[i].scene.create_block_from_entities(
            &handles,
            &name,
            &world_to_block,
            &block_to_world,
        ) {
            Ok(insert_handle) => {
                self.tabs[i].dirty = true;
                self.tabs[i].scene.deselect_all();
                if !insert_handle.is_null() {
                    self.tabs[i].scene.select_entity(insert_handle, false);
                }
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line
                    .push_output(crate::tf!("Block \"{name}\" created.").as_ref());
                self.refresh_properties();
            }
            Err(err) => {
                self.discard_last_undo_entry(i);
                self.command_line.push_error(&err);
                let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                if let Some(p) = prompt {
                    self.command_line.push_info(&p);
                }
            }
        }
        None
    }

    pub(super) fn handle_create_block_with_options(
        &mut self,
        mut options: Box<crate::scene::CreateBlockOptions>,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        options.handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if options.handles.is_empty() {
            self.command_line
                .push_info(crate::t!("No editable objects selected.").as_ref());
            return Some(Task::none());
        }
        self.push_undo_snapshot(i, "BLOCK");
        let name = options.name.clone();
        match self.tabs[i].scene.create_block_with_options(*options) {
            Ok(insert_handle) => {
                self.tabs[i].dirty = true;
                self.tabs[i].scene.deselect_all();
                if !insert_handle.is_null() {
                    self.tabs[i].scene.select_entity(insert_handle, false);
                }
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.command_line
                    .push_output(crate::tf!("Block \"{name}\" created.").as_ref());
                self.refresh_properties();
                self.refresh_block_palette();
            }
            Err(err) => {
                self.discard_last_undo_entry(i);
                self.command_line.push_error(&err);
            }
        }
        None
    }

    pub(super) fn handle_commit_hatch(&mut self, hatch: crate::scene::HatchModel) {
        let i = self.active_tab;
        let label = self.history_label_from_active_cmd(i, "HATCH");
        let pending = self.begin_undo(i, label, 1, true);
        let layer = self.tabs[i].active_layer.clone();
        let new_handle = self.tabs[i].scene.add_hatch(hatch, Some(&layer), None);
        if !new_handle.is_null() {
            self.tabs[i].scene.select_entity(new_handle, true);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    pub(super) fn handle_commit_styled_hatch(
        &mut self,
        hatch: crate::scene::HatchModel,
        color: codec::types::Color,
        transparency: codec::types::Transparency,
    ) {
        let i = self.active_tab;
        let label = self.history_label_from_active_cmd(i, "HATCH");
        let pending = self.begin_undo(i, label, 1, true);
        let layer = self.tabs[i].active_layer.clone();
        let new_handle =
            self.tabs[i]
                .scene
                .add_hatch(hatch, Some(&layer), Some((color, transparency)));
        if !new_handle.is_null() {
            self.tabs[i].scene.select_entity(new_handle, true);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    pub(super) fn handle_commit_hatch_with_boundaries(
        &mut self,
        mut hatch: crate::scene::HatchModel,
        boundaries: Vec<codec::EntityType>,
        entity_style: Option<(codec::types::Color, codec::types::Transparency)>,
    ) {
        let i = self.active_tab;
        let label = self.history_label_from_active_cmd(i, "HATCH");
        let pending = self.begin_undo(i, label, boundaries.len() + 1, true);
        let mut sources = Vec::with_capacity(boundaries.len());
        for boundary in boundaries {
            let handles = self
                .commit_entity_handle(boundary)
                .into_iter()
                .collect::<Vec<_>>();
            sources.push(handles);
        }
        if let Some(paths) = hatch.boundary_paths.as_mut() {
            for (path, handles) in std::sync::Arc::make_mut(paths).iter_mut().zip(&sources)
            {
                path.boundary_handles = handles.clone();
                path.flags.set_external(!handles.is_empty());
            }
        }
        hatch.boundary_sources = Some(std::sync::Arc::new(sources));
        let layer = self.tabs[i].active_layer.clone();
        let new_handle = self.tabs[i]
            .scene
            .add_hatch(hatch, Some(&layer), entity_style);
        if !new_handle.is_null() {
            self.tabs[i].scene.select_entity(new_handle, true);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    pub(super) fn handle_commit_hatches(
        &mut self,
        hatches: Vec<crate::scene::HatchModel>,
        entity_style: Option<(codec::types::Color, codec::types::Transparency)>,
    ) {
        let i = self.active_tab;
        let label = self.history_label_from_active_cmd(i, "HATCH");
        let pending = self.begin_undo(i, label, hatches.len(), true);
        let layer = self.tabs[i].active_layer.clone();
        for hatch in hatches {
            let new_handle =
                self.tabs[i]
                    .scene
                    .add_hatch(hatch, Some(&layer), entity_style.clone());
            if !new_handle.is_null() {
                self.tabs[i].scene.select_entity(new_handle, true);
            }
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    pub(super) fn handle_update_entity_and_finish(
        &mut self,
        handle: Handle,
        entity: codec::EntityType,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            self.tabs[i].active_cmd = None;
            return Some(Task::none());
        }
        let label = self.history_label_from_active_cmd(i, "EDIT");
        self.push_undo_snapshot(i, label);
        let is_dimension = matches!(entity, codec::EntityType::Dimension(_));
        if let Some(current) = self.tabs[i].scene.document.get_entity_mut(handle) {
            *current = entity;
            if is_dimension {
                self.tabs[i].scene.invalidate_dim_block_recorded(handle);
            }
            self.tabs[i]
                .scene
                .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
        } else {
            self.command_line.push_error(
                crate::t!("The selected object is no longer available.").as_ref(),
            );
        }
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.refresh_properties();
        None
    }

    pub(super) fn handle_reassociate_center_mark(
        &mut self,
        target: Handle,
        source: Handle,
        point: glam::DVec3,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, target) {
            return Some(Task::none());
        }
        self.push_undo_snapshot(i, "CENTERREASSOCIATE");
        if self.tabs[i]
            .scene
            .reassociate_center_mark(target, source, point)
        {
            self.tabs[i].dirty = true;
            self.command_line.push_output(
                crate::t!("CENTERREASSOCIATE: center mark associated.").as_ref(),
            );
        } else {
            self.command_line.push_error(
                crate::t!("CENTERREASSOCIATE: the selected source is not circular.")
                    .as_ref(),
            );
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.refresh_properties();
        None
    }

    pub(super) fn handle_attreq_needed(&mut self, block_name: String) -> Option<Task<Message>> {
        let i = self.active_tab;
        // Collect the full AttributeDefinitions owned by this block
        // record so each created attribute keeps its geometry (#255).
        let attdefs: Vec<codec::entities::AttributeDefinition> = {
            let doc = &self.tabs[i].scene.document;
            if let Some(br) = doc.block_records.get(&block_name) {
                br.entity_handles
                    .iter()
                    .filter_map(|&h| {
                        if let Some(codec::EntityType::AttributeDefinition(ad)) =
                            doc.get_entity(h)
                        {
                            Some(ad.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                vec![]
            }
        };

        if attdefs.is_empty() {
            // No attribute definitions — commit the INSERT directly.
            let entity = self.tabs[i]
                .active_cmd
                .as_mut()
                .and_then(|c| c.attreq_take_insert());
            if let Some(entity) = entity {
                let insert_name = match &entity {
                    codec::EntityType::Insert(ins) => Some(ins.block_name.clone()),
                    _ => None,
                };
                let label = self.history_label_from_active_cmd(i, "INSERT");
                let delta_safe = self.delta_add_safe(i, &entity);
                let pending = self.begin_undo(i, label, 1, delta_safe);
                self.commit_entity(entity);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                if let Some(n) = insert_name {
                    self.record_block_insert(&n);
                }
                self.restore_pre_cmd_tangent();
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
            }
        } else {
            let completed = self.tabs[i]
                .active_cmd
                .as_mut()
                .and_then(|cmd| cmd.attreq_set_attdefs(attdefs));
            if let Some(entity) = completed {
                return Some(self.apply_cmd_result(CmdResult::CommitAndExit(entity)));
            }
            let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
            if let Some(p) = prompt {
                self.command_line.push_info(&p);
            }
        }
        None
    }

    pub(super) fn handle_commit_live_entity(&mut self, entity: codec::EntityType) {
        let i = self.active_tab;
        let label = self.history_label_from_active_cmd(i, "ENTITY");
        let delta_safe = self.delta_add_safe(i, &entity);
        let pending = self.begin_undo(i, label, 1, delta_safe);
        let handle = self.commit_entity_handle(entity);
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        if let Some(h) = handle {
            // Keep the live document Arc unique while later vertices
            // replace the geometry in place. The final Arc is captured
            // by UpdateLiveEntity when the command completes.
            self.defer_live_entity_history_after(i, h);
            if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                cmd.set_live_handle(h);
            }
        }
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
    }

    pub(super) fn handle_update_live_entity(
        &mut self,
        handle: Handle,
        entity: codec::EntityType,
        finish: bool,
    ) {
        let i = self.active_tab;
        let tracks_draw_anchor = matches!(
            &entity,
            codec::EntityType::Line(_)
                | codec::EntityType::Arc(_)
                | codec::EntityType::LwPolyline(_)
                | codec::EntityType::Polyline(_)
                | codec::EntityType::Polyline2D(_)
                | codec::EntityType::Polyline3D(_)
        );
        // Replace the live entity's geometry in place, preserving its
        // handle and layer (the fresh entity from the command carries
        // defaults — a NULL handle would desync it from the document
        // map key and drop it from rendering / hit-test). No undo
        // snapshot — the create already pushed one, so the whole object
        // reverts as a unit.
        if let Some(old) = self.tabs[i].scene.document.get_entity_mut(handle) {
            // The initial live commit assigns the handle, owning block,
            // current layer and common display properties.  Geometry
            // refreshes must retain all of that identity; replacing only
            // the handle and layer reset owner_handle to NULL and made
            // the completed entity unavailable to scoped operations such
            // as parametric constraints.
            let common = old.common().clone();
            let mut new = entity;
            *new.common_mut() = common;
            *old = new;
            self.tabs[i]
                .scene
                .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
        }
        if tracks_draw_anchor {
            self.tabs[i].last_draw_anchor = Some(handle);
        }
        if finish {
            self.finish_live_entity_history(i, handle);
            self.tabs[i].scene.clear_preview_wire();
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.restore_pre_cmd_tangent();
        } else {
            let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
            if let Some(p) = prompt {
                self.command_line.push_info(&p);
            }
        }
    }

    pub(super) fn handle_finalize_live_entity(&mut self, handle: Handle) {
        let i = self.active_tab;
        // The live geometry already matches the command's committed
        // vertices. Close the deferred history image and UI state
        // without publishing a redundant Modified delta.
        self.finish_live_entity_history(i, handle);
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
    }

    pub(super) fn handle_remove_live_entity(&mut self, handle: Handle) {
        let i = self.active_tab;
        // The command backed off below a valid entity (PLINE Undo at
        // one remaining vertex): take the live entity out of the
        // document, drop its provisional history entry and keep
        // prompting. A later second point creates one fresh entry.
        self.tabs[i].scene.erase_entities(&[handle]);
        if self.tabs[i]
            .last_draw_anchor
            .is_some_and(|anchor_handle| anchor_handle == handle)
        {
            self.tabs[i].last_draw_anchor = None;
        }
        self.discard_last_undo_entry(i);
        self.tabs[i].dirty = true;
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
    }
}
