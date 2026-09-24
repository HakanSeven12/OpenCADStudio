use super::*;

impl OpenCADStudio {
    pub(super) fn handle_copy_to_clipboard(&mut self, handles: Vec<Handle>, base: glam::DVec3) {
        let i = self.active_tab;
        let count = self.copy_entities_to_clipboard(i, &handles, base);
        self.command_line
            .push_info(crate::tf!("{} object(s) copied to clipboard.", count).as_ref());
        let prompt = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|command| command.prompt());
        if let Some(prompt) = prompt {
            self.command_line.push_info(&prompt);
        }
        self.command_line.set_step_options(
            self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|command| command.options())
                .unwrap_or_default(),
        );
        if !self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.entity_pick_highlights_hover())
        {
            self.tabs[i].scene.set_hover_highlight(None);
        }
        self.sync_dyn_fields();
        self.refresh_area_preview(i);
    }

    pub(super) fn handle_batch_copy(
        &mut self,
        mut handles: Vec<Handle>,
        transforms: Vec<crate::command::EntityTransform>,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if handles.is_empty() {
            self.tabs[i].active_cmd = None;
            return Some(Task::none());
        }
        let label = self.history_label_from_active_cmd(i, "ARRAY");
        let count = transforms.len();
        // Same gate as COPY (dimension-free), sized by the total number
        // of copies the array will add.
        let delta_safe = self.delta_copy_safe(i, &handles);
        let pending = self.begin_undo(i, label.clone(), handles.len() * count, delta_safe);
        for t in &transforms {
            self.tabs[i].scene.copy_entities(&handles, t);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        let noun = if count == 1 { "copy" } else { "copies" };
        self.command_line
            .push_output(crate::tf!("{label}: {count} {noun} created.").as_ref());
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_replace_many(
        &mut self,
        replacements: Vec<(Handle, Vec<acadrust::EntityType>)>,
        additions: Vec<acadrust::EntityType>,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if let Some((handle, _)) = replacements
            .iter()
            .find(|(handle, _)| self.tabs[i].scene.is_layer_locked(*handle))
        {
            self.reject_locked_edit(i, *handle);
            self.tabs[i].active_cmd = None;
            return Some(Task::none());
        }
        let label = self.history_label_from_active_cmd(i, "FILLET");
        let was_catchment = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.name() == "SS_CATCHMENT");
        self.push_undo_snapshot(i, label);
        for (handle, entities) in replacements {
            self.replace_command_entity(i, handle, entities);
        }
        for entity in additions {
            self.tabs[i].scene.add_entity(entity);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        if was_catchment {
            self.command_line
                .push_info(crate::t!("Catchment tagged successfully.").as_ref());
        }
        self.refresh_properties();
        None
    }

    pub(super) fn handle_replace_many_continue(
        &mut self,
        replacements: Vec<(Handle, Vec<acadrust::EntityType>)>,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if let Some((handle, _)) = replacements
            .iter()
            .find(|(handle, _)| self.tabs[i].scene.is_layer_locked(*handle))
        {
            self.reject_locked_edit(i, *handle);
            return Some(Task::none());
        }
        let label = self.history_label_from_active_cmd(i, "TRIM");
        self.push_undo_snapshot(i, label);
        for (handle, entities) in replacements {
            let new_handles = self.replace_command_entity(i, handle, entities);
            if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                command.on_entity_replaced(handle, &new_handles);
            }
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].snap_result = None;
        if let Some(prompt) = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|command| command.prompt())
        {
            self.command_line.push_info(&prompt);
        }
        self.refresh_properties();
        None
    }

    pub(super) fn handle_replace_entity(
        &mut self,
        handle: Handle,
        new_entities: Vec<acadrust::EntityType>,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Some(Task::none());
        }
        // Detect SPLINEDIT sentinel: a single XLine with a magic layer name.
        if new_entities.len() == 1 {
            if let acadrust::EntityType::XLine(ref xl) = new_entities[0] {
                let op = xl.common.layer.clone();
                if op.starts_with("__SPLINEDIT_") {
                    let label = self.history_label_from_active_cmd(i, "SPLINEDIT");
                    self.push_undo_snapshot(i, label);
                    crate::modules::draw::modify::splinedit::apply_spline_op(
                        &mut self.tabs[i].scene.document,
                        handle,
                        &op,
                    );
                    self.tabs[i].dirty = true;
                    let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                    if let Some(p) = prompt {
                        self.command_line.push_info(&p);
                    }
                    return Some(Task::none());
                }
            }
        }
        let label = self.history_label_from_active_cmd(i, "TRIM");
        self.push_undo_snapshot(i, label);
        self.tabs[i].scene.erase_entities(&[handle]);
        let new_handles: Vec<acadrust::Handle> = new_entities
            .into_iter()
            .map(|e| self.tabs[i].scene.add_entity(e))
            .collect();
        // Rebuild replaced dimensions from edited data.
        for &nh in &new_handles {
            if matches!(
                self.tabs[i].scene.document.get_entity(nh),
                Some(acadrust::EntityType::Dimension(_))
            ) {
                self.tabs[i].scene.invalidate_dim_block_recorded(nh);
            }
        }
        let pedit_entities = if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.name() == "PEDIT")
        {
            new_handles
                .iter()
                .filter_map(|new| self.tabs[i].scene.document.get_entity(*new).cloned())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if let Some(cmd) = &mut self.tabs[i].active_cmd {
            cmd.on_entity_replaced(handle, &new_handles);
            for entity in pedit_entities {
                cmd.inject_picked_entity(entity);
            }
        }
        self.tabs[i].dirty = true;
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
        None
    }

    pub(super) fn handle_paste_clipboard(&mut self, base_pt: glam::DVec3) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        if self.clipboard.is_empty() {
            self.command_line
                .push_error(crate::t!("Clipboard is empty.").as_ref());
        } else {
            let delta = base_pt - self.clipboard_base;
            let translate = crate::command::EntityTransform::Translate(delta);
            self.push_undo_snapshot(i, "PASTECLIP");
            let count = self.clipboard.len();
            let by_index = self.finalize_paste(i, Some(translate));
            self.tabs[i].scene.deselect_all();
            for h in by_index.iter().copied().filter(|h| !h.is_null()) {
                self.tabs[i].scene.select_entity(h, false);
            }
            self.tabs[i].dirty = true;
            // Surface any layers the paste brought in (cross-drawing)
            // in the layer manager and the layer dropdown.
            self.refresh_layer_panel();
            self.refresh_properties();
            self.command_line
                .push_info(crate::tf!("{count} object(s) pasted.").as_ref());
        }
    }

    pub(crate) fn merge_clipboard_deps(&mut self, i: usize) {
        let deps = self.clipboard_deps.clone();
        self.merge_dependencies(i, &deps);
    }

    /// Recreate any block definition the pasted INSERTs reference but tab
    /// `i`'s document lacks (cross-drawing paste), so the block reference
    /// renders its geometry instead of nothing. No-op for same-document
    /// pastes. (#135)
    pub(crate) fn merge_clipboard_blocks(&mut self, i: usize) {
        if self.clipboard_deps.blocks.is_empty() {
            return;
        }
        let blocks = self.clipboard_deps.blocks.clone();
        for def in blocks {
            if self.tabs[i]
                .scene
                .document
                .block_records
                .get(&def.name)
                .is_some()
            {
                continue;
            }
            self.tabs[i]
                .scene
                .define_block_raw(&def.name, def.base_point, def.entities);
        }
    }

    /// Shared paste finalize for every paste path (PASTECLIP, PASTEORIG):
    /// recreate the clipboard's dependency records and block definitions, add
    /// each entity with fresh handles (optionally transformed), recreate each
    /// entity's xdictionary graph (XCLIP filters etc.), and tessellate pasted
    /// solids. Returns the new handles, index-aligned with the clipboard
    /// (NULL where an add failed). Keeping this in one place means a new
    /// cross-drawing concern is wired once, not re-implemented per command.
    pub(crate) fn finalize_paste(
        &mut self,
        i: usize,
        translate: Option<crate::command::EntityTransform>,
    ) -> Vec<Handle> {
        self.merge_clipboard_deps(i);
        self.merge_clipboard_blocks(i);
        let by_index: Vec<Handle> = self
            .clipboard
            .clone()
            .into_iter()
            .map(|mut entity| {
                if let Some(t) = &translate {
                    crate::scene::view::dispatch::apply_transform(&mut entity, t);
                }
                // A dimension draws from its baked `*D` block (baked in WCS), so
                // give the paste its own transformed copy of that block and
                // re-point it — otherwise the pasted dimension renders at the
                // source location instead of the paste point. The block was
                // snapshotted into the clipboard at copy time, so this works
                // cross-drawing too. Mirrors the in-drawing copy. (#290, #161)
                if let acadrust::EntityType::Dimension(d) = &entity {
                    let bn = d.base().block_name.clone();
                    if !bn.trim().is_empty() {
                        let subs = self
                            .clipboard_deps
                            .dim_blocks
                            .iter()
                            .find(|b| b.name.eq_ignore_ascii_case(&bn))
                            .map(|def| def.entities.clone());
                        if let Some(subs) = subs {
                            let bt = translate.clone().unwrap_or(
                                crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
                            );
                            if let Some(new_bn) =
                                self.tabs[i].scene.define_transformed_block(&subs, &bt)
                            {
                                if let acadrust::EntityType::Dimension(d) = &mut entity {
                                    d.base_mut().block_name = new_bn;
                                }
                            }
                        }
                    }
                }
                self.tabs[i].scene.add_entity_clone(entity)
            })
            .collect();
        let annotation_delta = match translate {
            Some(crate::command::EntityTransform::Translate(delta)) => delta,
            _ => glam::DVec3::ZERO,
        };
        self.merge_clipboard_ext_objects(i, &by_index, annotation_delta);
        // Source handles stored in the clipboard map one-to-one to the freshly
        // pasted handles. Use that map to reconnect LEADER -> copied annotation.
        let mut handle_map = rustc_hash::FxHashMap::default();

        for (source, &copied) in self.clipboard.iter().zip(by_index.iter()) {
            if !copied.is_null() {
                handle_map.insert(source.common().handle, copied);
            }
        }

        let leader_links: Vec<(Handle, Handle)> = self
            .clipboard
            .iter()
            .filter_map(|source| {
                let acadrust::EntityType::Leader(leader) = source else {
                    return None;
                };

                let copied_leader = handle_map.get(&source.common().handle).copied()?;
                let copied_annotation = handle_map
                    .get(&leader.annotation_handle)
                    .copied()
                    .unwrap_or(Handle::NULL);

                Some((copied_leader, copied_annotation))
            })
            .collect();

        for (leader_handle, annotation_handle) in leader_links {
            if let Some(acadrust::EntityType::Leader(leader)) =
                self.tabs[i].scene.document.get_entity_mut(leader_handle)
            {
                leader.annotation_handle = annotation_handle;
            }

            let _ = self.tabs[i]
                .scene
                .sync_displayed_annotation_context(leader_handle);
        }
        // Recreate any group whose whole membership was copied, so a pasted
        // group stays grouped — cross-drawing too, since the groups were
        // snapshotted into the clipboard at copy time. `by_index` is aligned
        // with `self.clipboard`, so the source handle of each pasted entity maps
        // its clipboard clone to its new handle. Same shared `recreate_groups`
        // the in-drawing COPY path uses. (#440)
        if !self.clipboard_deps.groups.is_empty() {
            let groups = self.clipboard_deps.groups.clone();
            self.tabs[i].scene.recreate_groups(groups, &handle_map);
        }
        // Constraints wholly within the pasted selection follow it.
        self.tabs[i]
            .scene
            .duplicate_parametric_constraints_for(&handle_map);
        // Incremental: `add_entity` already tessellated every pasted top-level
        // solid, and existing document solids are still cached — so only newly
        // introduced block-definition solids need building. The full rebuild
        // would clear and re-tessellate the entire document (every solid in the
        // drawing) on each paste, which is what made a large paste stall.
        self.tabs[i].scene.populate_missing_meshes_from_document();
        by_index
    }

    /// Recreate the extension-dictionary object graph (XCLIP spatial filters,
    /// attached XRecords, …) captured for each copied entity, cloning every
    /// object into this document with fresh handles, remapping all internal
    /// references, and re-pointing the pasted entity's `xdictionary_handle` at
    /// the new root. `by_index` is the paste's new entity handles, aligned with
    /// the clipboard order (NULL where the add failed). No-op without captures.
    pub(crate) fn merge_clipboard_ext_objects(
        &mut self,
        i: usize,
        by_index: &[Handle],
        annotation_delta: glam::DVec3,
    ) {
        if self.clipboard_deps.ext_objects.is_empty() {
            return;
        }
        let captures = self.clipboard_deps.ext_objects.clone();
        let doc = &mut self.tabs[i].scene.document;
        for cap in &captures {
            let Some(&new_entity) = by_index.get(cap.entity_index) else {
                continue;
            };
            if new_entity.is_null() {
                continue;
            }
            if let Some(new_root) = recreate_ext_subtree(doc, cap, Some(new_entity)) {
                if let Some(e) = doc.get_entity_mut(new_entity) {
                    e.common_mut().xdictionary_handle = Some(new_root);
                }
                crate::scene::annotative::translate_annotation_contexts(
                    doc,
                    new_entity,
                    annotation_delta,
                );
            }
        }
        // The wires were tessellated before the filters existed; refresh only
        // the freshly-pasted entities whose clip object graph was attached.
        let changes: Vec<_> = by_index
            .iter()
            .copied()
            .filter(|handle| !handle.is_null())
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
    }

    /// Recreate the captured xdictionary subtrees in this document (fresh
    /// handles, remapped references) WITHOUT an added host entity, returning
    /// `entity_index → new xdictionary root`. Used by PASTEBLOCK, which folds
    /// the clipboard into a new block definition: the caller stamps each new
    /// root onto the matching entity's `xdictionary_handle` before defining the
    /// block, so the block's nested insert keeps its XCLIP filter.
    pub(crate) fn recreate_clipboard_ext_roots(
        &mut self,
        i: usize,
    ) -> std::collections::HashMap<usize, Handle> {
        let mut out = std::collections::HashMap::new();
        if self.clipboard_deps.ext_objects.is_empty() {
            return out;
        }
        let captures = self.clipboard_deps.ext_objects.clone();
        let doc = &mut self.tabs[i].scene.document;
        for cap in &captures {
            if let Some(new_root) = recreate_ext_subtree(doc, cap, None) {
                out.insert(cap.entity_index, new_root);
            }
        }
        out
    }
}

fn recreate_ext_subtree(
    doc: &mut acadrust::CadDocument,
    cap: &crate::app::ClipExtObjects,
    entity_handle: Option<Handle>,
) -> Option<Handle> {
    use std::collections::HashMap;
    let mut remap: HashMap<Handle, Handle> = HashMap::new();
    if let Some(eh) = entity_handle {
        remap.insert(cap.src_entity_handle, eh);
    }
    for (old, scale) in &cap.annotation_scales {
        let target = crate::scene::annotative::ensure_scale_object(doc, scale);
        remap.insert(*old, target);
    }
    for (old, _) in &cap.objects {
        remap.insert(*old, doc.allocate_handle());
    }
    for (old, obj) in &cap.objects {
        let mut obj = obj.clone();
        let new_h = remap[old];
        remap_object(&mut obj, new_h, &remap);
        doc.objects.insert(new_h, obj);
    }
    remap.get(&cap.root).copied()
}

/// Replace references to a clipboard entity inside one recreated extension
/// dictionary graph after its final block-owned handle becomes known.
pub(crate) fn remap_ext_subtree_reference(
    doc: &mut acadrust::CadDocument,
    root: Handle,
    source_entity: Handle,
    target_entity: Handle,
) {
    use acadrust::objects::ObjectType;
    use rustc_hash::FxHashSet;
    use std::collections::HashMap;

    let remap = HashMap::from([(source_entity, target_entity)]);
    let mut seen = FxHashSet::default();
    let mut pending = vec![root];
    while let Some(handle) = pending.pop() {
        if handle.is_null() || !seen.insert(handle) {
            continue;
        }
        let children = match doc.objects.get(&handle) {
            Some(ObjectType::Dictionary(dictionary)) => {
                let mut children: Vec<_> =
                    dictionary.entries.iter().map(|(_, child)| *child).collect();
                if let Some(extension) = dictionary.xdictionary_handle {
                    children.push(extension);
                }
                children
            }
            Some(ObjectType::DictionaryWithDefault(dictionary)) => {
                let mut children: Vec<_> =
                    dictionary.entries.iter().map(|(_, child)| *child).collect();
                children.push(dictionary.default_handle);
                children
            }
            _ => Vec::new(),
        };
        pending.extend(children);
        if let Some(mut object) = doc.objects.remove(&handle) {
            remap_object(&mut object, handle, &remap);
            doc.objects.insert(handle, object);
        }
    }
}

/// Rewrite a cloned extension-dictionary object onto fresh handles: set its own
/// handle to `new_handle` and remap its owner and any handle references it holds
/// through `remap` (a handle still in the source space stays unchanged, which is
/// correct for cross-references that point outside the captured subtree).
fn remap_object(
    obj: &mut acadrust::objects::ObjectType,
    new_handle: acadrust::Handle,
    remap: &std::collections::HashMap<acadrust::Handle, acadrust::Handle>,
) {
    use acadrust::objects::ObjectType;
    let map = |h: acadrust::Handle| remap.get(&h).copied().unwrap_or(h);
    match obj {
        ObjectType::Dictionary(d) => {
            d.handle = new_handle;
            d.owner = map(d.owner);
            for (_, h) in d.entries.iter_mut() {
                *h = map(*h);
            }
            if let Some(x) = d.xdictionary_handle.as_mut() {
                *x = map(*x);
            }
            for r in d.reactors.iter_mut() {
                *r = map(*r);
            }
        }
        ObjectType::DictionaryWithDefault(d) => {
            d.handle = new_handle;
            d.owner = map(d.owner);
            for (_, h) in d.entries.iter_mut() {
                *h = map(*h);
            }
            d.default_handle = map(d.default_handle);
        }
        ObjectType::DictionaryVariable(v) => {
            v.handle = new_handle;
            v.owner_handle = map(v.owner_handle);
        }
        ObjectType::SpatialFilter(s) => {
            s.handle = new_handle;
            s.owner = map(s.owner);
        }
        ObjectType::XRecord(x) => {
            x.handle = new_handle;
            x.owner = map(x.owner);
            for entry in &mut x.entries {
                if let acadrust::objects::XRecordValue::Handle(handle) = &mut entry.value {
                    *handle = map(*handle);
                }
            }
        }
        ObjectType::Group(g) => {
            g.handle = new_handle;
            g.owner = map(g.owner);
            for h in g.entities.iter_mut() {
                *h = map(*h);
            }
        }
        ObjectType::ObjectContextData(context) => {
            context.handle = new_handle;
            context.owner_handle = map(context.owner_handle);
            for reactor in &mut context.reactors {
                *reactor = map(*reactor);
            }
            if let Some(dictionary) = &mut context.xdictionary_handle {
                *dictionary = map(*dictionary);
            }
            context.scale = map(context.scale);
            match &mut context.kind {
                acadrust::objects::ObjectContextKind::Dim(dimension) => {
                    dimension.block = map(dimension.block);
                }
                acadrust::objects::ObjectContextKind::HatchView(hatch) => {
                    hatch.view = map(hatch.view);
                }
                acadrust::objects::ObjectContextKind::MTextAttribute(attribute) => {
                    if let Some(embedded) = &mut attribute.context {
                        embedded.owner_handle = map(embedded.owner_handle);
                        for reactor in &mut embedded.reactors {
                            *reactor = map(*reactor);
                        }
                        if let Some(dictionary) = &mut embedded.xdictionary_handle {
                            *dictionary = map(*dictionary);
                        }
                        embedded.scale = map(embedded.scale);
                    }
                }
                acadrust::objects::ObjectContextKind::MLeader(mleader) => {
                    if let Some(handle) = &mut mleader.text_style_handle {
                        *handle = map(*handle);
                    }
                    if let Some(handle) = &mut mleader.block_content_handle {
                        *handle = map(*handle);
                    }
                    if let Some(handle) = &mut mleader.scale_handle {
                        *handle = map(*handle);
                    }
                    for root in &mut mleader.leader_roots {
                        for line in &mut root.lines {
                            if let Some(handle) = &mut line.line_type_handle {
                                *handle = map(*handle);
                            }
                            if let Some(handle) = &mut line.arrowhead_handle {
                                *handle = map(*handle);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // Other leaf object kinds don't appear in an entity xdictionary; if one
        // does, it's inserted with the fresh handle below via the caller's key,
        // but its internal owner is left as-is (best effort).
        _ => {}
    }
}
