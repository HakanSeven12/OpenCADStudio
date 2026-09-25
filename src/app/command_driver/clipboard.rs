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
        replacements: Vec<(Handle, Vec<codec::EntityType>)>,
        additions: Vec<codec::EntityType>,
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
        replacements: Vec<(Handle, Vec<codec::EntityType>)>,
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
        new_entities: Vec<codec::EntityType>,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Some(Task::none());
        }
        // Detect SPLINEDIT sentinel: a single XLine with a magic layer name.
        if new_entities.len() == 1 {
            if let codec::EntityType::XLine(ref xl) = new_entities[0] {
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
        let new_handles: Vec<codec::Handle> = new_entities
            .into_iter()
            .map(|e| self.tabs[i].scene.add_entity(e))
            .collect();
        // Rebuild replaced dimensions from edited data.
        for &nh in &new_handles {
            if matches!(
                self.tabs[i].scene.document.get_entity(nh),
                Some(codec::EntityType::Dimension(_))
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
        // Single shared copy — see `merge_block_defs`. One batched geometry
        // rebuild instead of one per definition; same end state as the
        // per-definition bumps `Scene::define_block_raw` used to run.
        if merge_block_defs(&mut self.tabs[i].scene.document, &blocks) > 0 {
            self.tabs[i].scene.bump_geometry();
        }
    }

    /// Shared paste finalize for every paste path (PASTECLIP, PASTEORIG):
    /// document-level paste runs in [`paste_entities_kernel`] (fresh handles,
    /// dependency records, block definitions, `*D` re-point, xdictionary
    /// graphs, LEADER relink); the Scene/render tail stays here (tessellation
    /// bumps, group + constraint recreation, annotation-context display sync).
    /// Returns the new handles, index-aligned with the clipboard
    /// (NULL where an add failed). Keeping the document half in one kernel
    /// means a new cross-drawing concern is wired once, not re-implemented
    /// per command — and headless callers share it without a Scene.
    pub(crate) fn finalize_paste(
        &mut self,
        i: usize,
        translate: Option<crate::command::EntityTransform>,
    ) -> Vec<Handle> {
        // Per-helper narrow/keep report (Task 4): the kernel ports what
        // `&mut CadDocument` can do and this shell keeps the rest —
        //   merge_dependencies (table records) … KEEP in kernel (verbatim
        //     port; pure document-table work, no Document-level free fn existed)
        //   merge_clipboard_blocks / define_block_raw … KEEP in kernel minus
        //     the trailing bump_geometry (render cache — shelled below)
        //   add_entity_clone … NARROW: sub-handle reset + NULL handles +
        //     doc.add_entity in kernel; entity_mode default, layer/app-id
        //     ensure, ImageDefinition auto-create, BEDIT/paper-space owner
        //     routing, undo recording and per-add tessellation stay caller-side
        //     (Scene-only; NO Scene methods refactored)
        //   define_transformed_block (`*D` dim blocks) … KEEP in kernel
        //     (verbatim port; no render-cache tail exists)
        //   recreate_ext_subtree / remap_object … USE existing Document-level
        //     free fns directly (no Scene involved)
        //   translate_annotation_contexts … USE existing Document-level free fn
        //   LEADER annotation_handle relink … KEEP in kernel (get_entity_mut);
        //     sync_displayed_annotation_context stays caller-side (Scene)
        //   recreate_groups … CALLER-SIDE (Scene::recreate_groups: undo
        //     recording + group-dict naming; no Document equivalent exists)
        //   duplicate_parametric_constraints_for … CALLER-SIDE (Scene-owned
        //     constraint sets; no Document equivalent exists)
        //   populate_missing_meshes_from_document / bumps … CALLER-SIDE
        let clipboard = self.clipboard.clone();
        let deps = self.clipboard_deps.clone();
        let (by_index, handle_map) = paste_entities_kernel(
            &mut self.tabs[i].scene.document,
            &clipboard,
            &deps,
            translate.as_ref(),
        );
        // Batched equivalent of `add_entity`'s per-add bump (same pattern as
        // `Scene::add_entities`): fresh top-level solids tessellate once here
        // instead of once per entity. Block-definition merges (and pasted
        // Block/BlockEnd sentinels) need the full geometry rebuild, matching
        // the `bump_geometry` tails the Scene helpers used to run.
        let mut needs_geometry_bump = !deps.blocks.is_empty();
        let mut changes = Vec::new();
        for &handle in &by_index {
            if handle.is_null() {
                continue;
            }
            if matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(
                    codec::EntityType::Block(_) | codec::EntityType::BlockEnd(_)
                )
            ) {
                needs_geometry_bump = true;
            } else {
                changes.push((handle, crate::scene::ChangeKind::Added));
            }
        }
        if needs_geometry_bump {
            self.tabs[i].scene.bump_geometry();
        } else if !changes.is_empty() {
            self.tabs[i].scene.bump_entities(&changes);
        }
        // The wires tessellated above predate the freshly attached clip object
        // graphs; refresh the pasted entities so XCLIP-filtered inserts
        // render clipped immediately.
        if !deps.ext_objects.is_empty() {
            let changes: Vec<_> = by_index
                .iter()
                .copied()
                .filter(|handle| !handle.is_null())
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities(&changes);
        }
        // Source handles stored in the clipboard map one-to-one to the freshly
        // pasted handles (see `handle_map`); refresh each pasted LEADER's
        // displayed annotation context now that the relink ran in the kernel.
        for &handle in by_index.iter().filter(|h| !h.is_null()) {
            if matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(codec::EntityType::Leader(_))
            ) {
                let _ = self.tabs[i]
                    .scene
                    .sync_displayed_annotation_context(handle);
            }
        }
        // Recreate any group whose whole membership was copied, so a pasted
        // group stays grouped — cross-drawing too, since the groups were
        // snapshotted into the clipboard at copy time. `by_index` is aligned
        // with `self.clipboard`, so the kernel's `handle_map` maps each
        // clipboard clone to its new handle. Same shared `recreate_groups`
        // the in-drawing COPY path uses. (#440)
        if !deps.groups.is_empty() {
            self.tabs[i].scene.recreate_groups(deps.groups.clone(), &handle_map);
        }
        // Constraints wholly within the pasted selection follow it.
        self.tabs[i]
            .scene
            .duplicate_parametric_constraints_for(&handle_map);
        // Incremental: fresh top-level solids already tessellated above, and
        // existing document solids are still cached — so only newly
        // introduced block-definition solids need building. The full rebuild
        // would clear and re-tessellate the entire document (every solid in the
        // drawing) on each paste, which is what made a large paste stall.
        self.tabs[i].scene.populate_missing_meshes_from_document();
        by_index
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

/// PASTECLIP kernel: paste a clipboard payload (`entities` + `deps`, as
/// captured by [`copy_to_clipboard_kernel`]) into `doc`, optionally
/// translated, returning the new handles index-aligned with `entities`
/// Recreate missing layer/linetype/text-style/dim-style records with fresh
/// handles from the target document. Shared by `merge_dependencies` (PASTEBLOCK
/// path) and `paste_entities_kernel` — one copy, not a port per caller.
pub(crate) fn merge_table_records(doc: &mut codec::CadDocument, deps: &crate::app::ClipboardDeps) {
    use codec::TableEntry;
    for rec in &deps.layers {
        if !doc.layers.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            let _ = doc.layers.add(r);
        }
    }
    for rec in &deps.linetypes {
        if !doc.line_types.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            let _ = doc.line_types.add(r);
        }
    }
    for rec in &deps.text_styles {
        if !doc.text_styles.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            let _ = doc.text_styles.add(r);
        }
    }
    for rec in &deps.dim_styles {
        if !doc.dim_styles.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            let _ = doc.dim_styles.add(r);
        }
    }
}

/// Recreate block definitions the pasted INSERTs reference but the target
/// document lacks. Shared by `merge_clipboard_blocks` and
/// `paste_entities_kernel`. Returns how many definitions were created, so
/// render-cache callers can bump once instead of per definition.
pub(crate) fn merge_block_defs(doc: &mut codec::CadDocument, blocks: &[crate::app::BlockDef]) -> usize {
    let mut created = 0;
    for def in blocks {
        let before = doc.block_records.len();
        define_block_raw_doc(doc, &def.name, def.base_point, &def.entities);
        created += usize::from(doc.block_records.len() > before);
    }
    created
}

/// (`NULL` where an add failed) plus the source-handle → pasted-handle map.
///
/// Document-level work lives here; Scene/render concerns stay caller-side in
/// `finalize_paste` (tessellation bumps, `populate_missing_meshes`,
/// `recreate_groups`, `duplicate_parametric_constraints_for`,
/// `sync_displayed_annotation_context`, undo/dirty/selection/echo/panels).
/// See the per-helper report in `finalize_paste`.
pub(crate) fn paste_entities_kernel(
    doc: &mut codec::CadDocument,
    entities: &[codec::EntityType],
    deps: &crate::app::ClipboardDeps,
    translate: Option<&crate::command::EntityTransform>,
) -> (
    Vec<Handle>,
    rustc_hash::FxHashMap<Handle, Handle>,
) {
    // Shared table-record merge (also used by `merge_dependencies`): recreate
    // missing records with fresh handles from the target document.
    merge_table_records(doc, deps);
    // Shared block-definition merge (also used by `merge_clipboard_blocks`).
    merge_block_defs(doc, &deps.blocks);

    let by_index: Vec<Handle> = entities
        .iter()
        .cloned()
        .map(|mut entity| {
            if let Some(t) = translate {
                crate::scene::view::dispatch::apply_transform(&mut entity, t);
            }
            // Same `*D`-block re-point as `finalize_paste` (#290, #161): the
            // baked dimension geometry lives in WCS, so the paste gets its own
            // transformed copy of the snapshotted block.
            if let codec::EntityType::Dimension(d) = &entity {
                let bn = d.base().block_name.clone();
                if !bn.trim().is_empty() {
                    if let Some(subs) = deps
                        .dim_blocks
                        .iter()
                        .find(|b| b.name.eq_ignore_ascii_case(&bn))
                        .map(|def| def.entities.clone())
                    {
                        let bt = translate.cloned().unwrap_or(
                            crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
                        );
                        if let Some(new_bn) = define_transformed_block_doc(doc, &subs, &bt) {
                            if let codec::EntityType::Dimension(d) = &mut entity {
                                d.base_mut().block_name = new_bn;
                            }
                        }
                    }
                }
            }
            add_entity_clone_doc(doc, entity)
        })
        .collect();

    // Port of `merge_clipboard_ext_objects` minus the trailing `bump_entities`
    // (render cache — caller-side): recreate each captured xdictionary graph
    // with fresh handles and re-point the pasted entity at its new root.
    let annotation_delta = match translate {
        Some(crate::command::EntityTransform::Translate(delta)) => *delta,
        _ => glam::DVec3::ZERO,
    };
    for cap in &deps.ext_objects {
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
            crate::scene::annotative::translate_annotation_contexts(doc, new_entity, annotation_delta);
        }
    }

    // Source handles map one-to-one to the freshly pasted handles; reconnect
    // LEADER -> copied annotation through the map (the
    // `sync_displayed_annotation_context` refresh stays caller-side).
    let mut handle_map: rustc_hash::FxHashMap<Handle, Handle> = rustc_hash::FxHashMap::default();
    for (source, &copied) in entities.iter().zip(by_index.iter()) {
        if !copied.is_null() {
            handle_map.insert(source.common().handle, copied);
        }
    }
    let leader_links: Vec<(Handle, Handle)> = entities
        .iter()
        .filter_map(|source| {
            let codec::EntityType::Leader(leader) = source else {
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
        if let Some(codec::EntityType::Leader(leader)) = doc.get_entity_mut(leader_handle) {
            leader.annotation_handle = annotation_handle;
        }
    }
    (by_index, handle_map)
}

/// Document-level clone-add: the `&mut CadDocument` half of
/// `Scene::add_entity_clone` — fresh inline sub-handles (INSERT attributes,
/// 3D-polyline vertices; `reset_clone_subhandles` is `scene`-private so the
/// two lines are mirrored here), NULL top-level/owner handles, then
/// `doc.add_entity`. The `entity_mode` default (Model vs paper space) and all
/// tessellation/render caching stay caller-side in the Scene shell.
fn add_entity_clone_doc(doc: &mut codec::CadDocument, mut entity: codec::EntityType) -> Handle {
    match &mut entity {
        codec::EntityType::Insert(ins) => {
            for att in ins.attributes.iter_mut() {
                att.common.handle = doc.allocate_handle();
            }
        }
        codec::EntityType::Polyline3D(p) => {
            for v in p.vertices.iter_mut() {
                v.handle = doc.allocate_handle();
            }
        }
        _ => {}
    }
    entity.common_mut().handle = Handle::NULL;
    entity.common_mut().owner_handle = Handle::NULL;
    doc.add_entity(entity).unwrap_or(Handle::NULL)
}

/// Document-level verbatim block-definition recreate: the `&mut CadDocument`
/// half of `Scene::define_block_raw` (no-op when the block exists). The
/// trailing `bump_geometry` stays caller-side.
fn define_block_raw_doc(
    doc: &mut codec::CadDocument,
    name: &str,
    base_point: codec::types::Vector3,
    entities: &[codec::EntityType],
) {
    if name.is_empty() || doc.block_records.get(name).is_some() {
        return;
    }
    let next = doc.next_handle();
    let br_handle = Handle::new(next);
    let block_handle = Handle::new(next + 1);
    let end_handle = Handle::new(next + 2);
    let mut block_record = codec::tables::BlockRecord::new(name);
    block_record.handle = br_handle;
    block_record.block_entity_handle = block_handle;
    block_record.block_end_handle = end_handle;
    if doc.block_records.add(block_record).is_err() {
        return;
    }
    let mut block = codec::entities::Block::new(name, base_point);
    block.common.handle = block_handle;
    block.common.owner_handle = br_handle;
    let _ = doc.add_entity(codec::EntityType::Block(block));
    let mut block_end = codec::entities::BlockEnd::new();
    block_end.common.handle = end_handle;
    block_end.common.owner_handle = br_handle;
    let _ = doc.add_entity(codec::EntityType::BlockEnd(block_end));
    for mut entity in entities.iter().cloned() {
        add_entity_clone_doc_inner(doc, &mut entity, br_handle);
        let _ = doc.add_entity(entity);
    }
}

/// Shared sub-handle reset + owner stamp for block-definition members.
fn add_entity_clone_doc_inner(
    doc: &mut codec::CadDocument,
    entity: &mut codec::EntityType,
    owner: Handle,
) {
    match entity {
        codec::EntityType::Insert(ins) => {
            for att in ins.attributes.iter_mut() {
                att.common.handle = doc.allocate_handle();
            }
        }
        codec::EntityType::Polyline3D(p) => {
            for v in p.vertices.iter_mut() {
                v.handle = doc.allocate_handle();
            }
        }
        _ => {}
    }
    entity.common_mut().handle = Handle::NULL;
    entity.common_mut().owner_handle = owner;
}

/// Document-level fresh `*D<n>` block from `subs`: the `&mut CadDocument`
/// half of `Scene::define_transformed_block` (which has no render-cache tail,
/// so this is a verbatim port). Returns the new block name, or `None` when
/// `subs` is empty.
fn define_transformed_block_doc(
    doc: &mut codec::CadDocument,
    subs: &[codec::EntityType],
    t: &crate::command::EntityTransform,
) -> Option<String> {
    if subs.is_empty() {
        return None;
    }
    let mut n = 0u64;
    let new_name = loop {
        let cand = format!("*D{n}");
        if doc.block_records.get(&cand).is_none() {
            break cand;
        }
        n += 1;
    };
    let next = doc.next_handle();
    let br_handle = Handle::new(next);
    let block_handle = Handle::new(next + 1);
    let end_handle = Handle::new(next + 2);
    let mut br = codec::tables::BlockRecord::new(&new_name);
    br.handle = br_handle;
    br.block_entity_handle = block_handle;
    br.block_end_handle = end_handle;
    doc.block_records.add(br).ok()?;
    let mut block = codec::entities::Block::new(&new_name, codec::types::Vector3::ZERO);
    block.common.handle = block_handle;
    block.common.owner_handle = br_handle;
    doc.add_entity(codec::EntityType::Block(block)).ok()?;
    let mut block_end = codec::entities::BlockEnd::new();
    block_end.common.handle = end_handle;
    block_end.common.owner_handle = br_handle;
    doc.add_entity(codec::EntityType::BlockEnd(block_end))
        .ok()?;
    for sub in subs {
        let mut sub = sub.clone();
        crate::scene::view::dispatch::apply_transform(&mut sub, t);
        add_entity_clone_doc_inner(doc, &mut sub, br_handle);
        let _ = doc.add_entity(sub);
    }
    Some(new_name)
}

/// COPYCLIP kernel: clone the selected entities out of `doc` and snapshot
/// the table records / block definitions / xdictionary graphs they depend on,
/// so a later paste can recreate them (including cross-drawing). Clipboard
/// storage (`clipboard`, `clipboard_base`, `clipboard_deps`) and the echo stay
/// caller-side in `copy_entities_to_clipboard` / `handle_copy_to_clipboard`.
pub(crate) fn copy_to_clipboard_kernel(
    doc: &codec::CadDocument,
    handles: &[Handle],
) -> (Vec<codec::EntityType>, crate::app::ClipboardDeps) {
    let entities: Vec<_> = handles
        .iter()
        .filter_map(|&handle| doc.get_entity(handle).cloned())
        .collect();
    let deps = crate::app::ClipboardDeps::capture(doc, &entities);
    (entities, deps)
}

fn recreate_ext_subtree(
    doc: &mut codec::CadDocument,
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
    doc: &mut codec::CadDocument,
    root: Handle,
    source_entity: Handle,
    target_entity: Handle,
) {
    use codec::objects::ObjectType;
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
pub(crate) fn remap_object(
    obj: &mut codec::objects::ObjectType,
    new_handle: codec::Handle,
    remap: &std::collections::HashMap<codec::Handle, codec::Handle>,
) {
    use codec::objects::ObjectType;
    let map = |h: codec::Handle| remap.get(&h).copied().unwrap_or(h);
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
                if let codec::objects::XRecordValue::Handle(handle) = &mut entry.value {
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
                codec::objects::ObjectContextKind::Dim(dimension) => {
                    dimension.block = map(dimension.block);
                }
                codec::objects::ObjectContextKind::HatchView(hatch) => {
                    hatch.view = map(hatch.view);
                }
                codec::objects::ObjectContextKind::MTextAttribute(attribute) => {
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
                codec::objects::ObjectContextKind::MLeader(mleader) => {
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

#[cfg(test)]
mod clipboard_xdict_clone_tests {
    use super::recreate_ext_subtree;
    use super::remap_object;
    use codec::Handle;
    // Same re-export path `commands/blocks.rs:356` uses — this import proves
    // the `mod.rs` re-export still covers external callers.
    use super::super::remap_ext_subtree_reference;
    use codec::objects::{ObjectType, XRecordValue};

    /// A dictionary root pointing at one XRecord leaf that references the
    /// source entity — the minimal shape of a captured XCLIP filter graph.
    fn xdict_capture_fixture() -> crate::app::ClipExtObjects {
        let mut src = codec::CadDocument::new();
        let src_entity = src.allocate_handle();
        let src_root = src.allocate_handle();
        let src_leaf = src.allocate_handle();

        let mut dict = codec::objects::Dictionary::new();
        dict.handle = src_root;
        dict.owner = src_entity;
        dict.add_entry("XCLIP", src_leaf);

        let mut xrec = codec::objects::XRecord::new();
        xrec.handle = src_leaf;
        xrec.owner = src_root;
        xrec.entries.push(codec::objects::XRecordEntry::new(
            340,
            XRecordValue::Handle(src_entity),
        ));

        crate::app::ClipExtObjects {
            entity_index: 0,
            src_entity_handle: src_entity,
            root: src_root,
            objects: vec![
                (src_root, ObjectType::Dictionary(dict)),
                (src_leaf, ObjectType::XRecord(xrec)),
            ],
            annotation_scales: Vec::new(),
        }
    }

    fn leaf_of(doc: &codec::CadDocument, root: Handle) -> Handle {
        match doc.objects.get(&root) {
            Some(ObjectType::Dictionary(d)) => {
                assert_eq!(d.entries.len(), 1);
                d.entries[0].1
            }
            other => panic!("expected recreated Dictionary root, got {other:?}"),
        }
    }

    /// Headless cross-drawing clone: recreate a captured xdictionary subtree
    /// in a fresh document and assert every remapped handle resolves.
    #[test]
    fn cross_drawing_clone_remaps_xdict_handles() {
        let cap = xdict_capture_fixture();
        let mut dst = codec::CadDocument::new();
        let before: Vec<Handle> = dst.objects.keys().copied().collect();
        let dst_entity = dst.allocate_handle();

        let new_root = recreate_ext_subtree(&mut dst, &cap, Some(dst_entity))
            .expect("recreated xdictionary root");
        assert!(!new_root.is_null());
        assert!(
            !before.contains(&new_root),
            "clone must use a fresh handle"
        );
        assert_eq!(dst.objects.len(), before.len() + 2);

        let leaf = leaf_of(&dst, new_root);
        assert!(!before.contains(&leaf), "leaf must use a fresh handle");
        match dst.objects.get(&new_root) {
            Some(ObjectType::Dictionary(d)) => {
                assert_eq!(d.handle, new_root);
                assert_eq!(d.owner, dst_entity);
            }
            other => panic!("expected recreated Dictionary root, got {other:?}"),
        }
        match dst.objects.get(&leaf) {
            Some(ObjectType::XRecord(x)) => {
                assert_eq!(x.handle, leaf);
                assert_eq!(x.owner, new_root);
                assert_eq!(x.entries.len(), 1);
                assert!(
                    matches!(&x.entries[0].value, XRecordValue::Handle(h) if *h == dst_entity),
                    "leaf entity reference must point at the pasted entity"
                );
            }
            other => panic!("expected recreated XRecord leaf, got {other:?}"),
        }
    }

    /// PASTEBLOCK-style two-phase clone: recreate without a host entity, then
    /// re-point the subtree at the final block-owned handle.
    #[test]
    fn cross_drawing_clone_two_phase_remap_resolves_reference() {
        let cap = xdict_capture_fixture();
        let mut dst = codec::CadDocument::new();
        let before_len = dst.objects.len();

        let root =
            recreate_ext_subtree(&mut dst, &cap, None).expect("recreated xdictionary root");
        let leaf = leaf_of(&dst, root);
        match dst.objects.get(&leaf) {
            Some(ObjectType::XRecord(x)) => assert!(
                matches!(&x.entries[0].value, XRecordValue::Handle(h) if *h == cap.src_entity_handle),
                "unbound clone must keep the source-space reference"
            ),
            other => panic!("expected recreated XRecord leaf, got {other:?}"),
        }

        let target = dst.allocate_handle();
        remap_ext_subtree_reference(&mut dst, root, cap.src_entity_handle, target);
        assert_eq!(dst.objects.len(), before_len + 2);
        match dst.objects.get(&leaf) {
            Some(ObjectType::XRecord(x)) => assert!(
                matches!(&x.entries[0].value, XRecordValue::Handle(h) if *h == target),
                "remapped leaf must point at the block-owned entity"
            ),
            other => panic!("expected recreated XRecord leaf, got {other:?}"),
        }
    }

    /// The widened `pub(crate)` kernel rewrites a cloned dictionary onto
    /// fresh handles through the remap table.
    #[test]
    fn remap_object_rewrites_dictionary_handles() {
        use std::collections::HashMap;
        let mut scratch = codec::CadDocument::new();
        let old_self = scratch.allocate_handle();
        let old_owner = scratch.allocate_handle();
        let old_child = scratch.allocate_handle();
        let new_self = scratch.allocate_handle();
        let new_owner = scratch.allocate_handle();
        let new_child = scratch.allocate_handle();

        let mut dict = codec::objects::Dictionary::new();
        dict.handle = old_self;
        dict.owner = old_owner;
        dict.add_entry("K", old_child);
        let mut obj = ObjectType::Dictionary(dict);
        let remap = HashMap::from([(old_owner, new_owner), (old_child, new_child)]);
        remap_object(&mut obj, new_self, &remap);
        match obj {
            ObjectType::Dictionary(d) => {
                assert_eq!(d.handle, new_self);
                assert_eq!(d.owner, new_owner);
                assert_eq!(d.entries[0].1, new_child);
            }
            other => panic!("expected Dictionary, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod copy_to_clipboard_kernel_tests {
    use super::copy_to_clipboard_kernel;

    fn line_on(layer: &str) -> codec::EntityType {
        let mut line = codec::entities::Line::new();
        line.common.layer = layer.to_string();
        codec::EntityType::Line(line)
    }

    #[test]
    fn clones_entities_and_captures_layer_deps() {
        let mut doc = codec::CadDocument::new();
        doc.layers
            .add(codec::tables::Layer::new("WALLS"))
            .expect("add layer");
        let a = doc.add_entity(line_on("WALLS")).unwrap();
        let b = doc.add_entity(line_on("WALLS")).unwrap();

        let (entities, deps) = copy_to_clipboard_kernel(&doc, &[a, b]);

        assert_eq!(entities.len(), 2);
        assert!(entities.iter().all(|e| e.common().layer == "WALLS"));
        let handles: Vec<_> = entities.iter().map(|e| e.common().handle).collect();
        assert!(handles.contains(&a));
        assert!(handles.contains(&b));
        assert!(deps.layers.iter().any(|l| l.name == "WALLS"));
    }

    #[test]
    fn skips_handles_missing_from_the_document() {
        let mut doc = codec::CadDocument::new();
        let kept = doc.add_entity(line_on("0")).unwrap();
        let missing = doc.allocate_handle();

        let (entities, _) = copy_to_clipboard_kernel(&doc, &[kept, missing]);

        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].common().handle, kept);
    }

    #[test]
    fn empty_selection_yields_empty_payload() {
        let doc = codec::CadDocument::new();

        let (entities, deps) = copy_to_clipboard_kernel(&doc, &[]);

        assert!(entities.is_empty());
        assert!(deps.layers.is_empty());
        assert!(deps.blocks.is_empty());
    }
}

#[cfg(test)]
mod paste_entities_kernel_tests {
    use super::{copy_to_clipboard_kernel, paste_entities_kernel};
    use codec::Handle;

    fn line_on(layer: &str) -> codec::EntityType {
        let mut line = codec::entities::Line::new();
        line.common.layer = layer.to_string();
        codec::EntityType::Line(line)
    }

    #[test]
    fn pastes_with_fresh_handles_and_translate() {
        let mut seed = codec::CadDocument::new();
        seed.layers
            .add(codec::tables::Layer::new("WALLS"))
            .expect("add layer");
        // Position the seed line away from the origin so the translate is visible.
        let mut seed_line = codec::entities::Line::from_points(
            codec::types::Vector3::new(0.0, 0.0, 0.0),
            codec::types::Vector3::new(10.0, 0.0, 0.0),
        );
        seed_line.common.layer = "WALLS".to_string();
        let seed_handle = seed
            .add_entity(codec::EntityType::Line(seed_line))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[seed_handle]);

        let mut dst = codec::CadDocument::new();
        let delta = glam::DVec3::new(5.0, 7.0, 0.0);
        let translate = crate::command::EntityTransform::Translate(delta);
        let (handles, handle_map) =
            paste_entities_kernel(&mut dst, &entities, &deps, Some(&translate));

        assert_eq!(handles.len(), 1);
        let pasted = handles[0];
        assert!(!pasted.is_null());
        assert_ne!(pasted, seed_handle, "paste must use a fresh handle");
        assert_eq!(handle_map.get(&seed_handle), Some(&pasted));
        let line = match dst.get_entity(pasted) {
            Some(codec::EntityType::Line(l)) => l,
            other => panic!("expected pasted Line, got {other:?}"),
        };
        assert!((line.start.x - 5.0).abs() < 1e-9, "start.x = {}", line.start.x);
        assert!((line.start.y - 7.0).abs() < 1e-9, "start.y = {}", line.start.y);
        assert!((line.end.x - 15.0).abs() < 1e-9, "end.x = {}", line.end.x);
        // The layer dependency must have been recreated in the target document.
        assert!(dst.layers.contains("WALLS"));
        assert_eq!(line.common.layer, "WALLS");
    }

    #[test]
    fn remaps_leader_annotation_link_to_pasted_handles() {
        let mut seed = codec::CadDocument::new();
        let note = seed
            .add_entity(codec::EntityType::MText(codec::entities::MText::new()))
            .unwrap();
        let mut leader = codec::entities::Leader::default();
        leader.annotation_handle = note;
        let leader_handle = seed
            .add_entity(codec::EntityType::Leader(leader))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[leader_handle, note]);

        let mut dst = codec::CadDocument::new();
        let (handles, _) = paste_entities_kernel(&mut dst, &entities, &deps, None);

        assert_eq!(handles.len(), 2);
        let pasted_leader = match dst.get_entity(handles[0]) {
            Some(codec::EntityType::Leader(l)) => l,
            other => panic!("expected pasted Leader, got {other:?}"),
        };
        assert_eq!(
            pasted_leader.annotation_handle, handles[1],
            "leader must point at the pasted annotation, not the source handle"
        );
    }

    #[test]
    fn recreates_ext_subtree_and_repoints_xdictionary() {
        // Minimal XCLIP-shaped capture: a dictionary root over one XRecord leaf
        // that references the source entity (mirrors clipboard_xdict_clone_tests).
        use codec::objects::{Dictionary, ObjectType, XRecord, XRecordEntry, XRecordValue};
        let mut seed = codec::CadDocument::new();
        let seed_entity = seed.add_entity(line_on("0")).unwrap();
        let src_root = seed.allocate_handle();
        let src_leaf = seed.allocate_handle();
        let mut dict = Dictionary::new();
        dict.handle = src_root;
        dict.owner = seed_entity;
        dict.add_entry("XCLIP", src_leaf);
        let mut xrec = XRecord::new();
        xrec.handle = src_leaf;
        xrec.owner = src_root;
        xrec.entries.push(XRecordEntry::new(
            340,
            XRecordValue::Handle(seed_entity),
        ));
        seed.objects.insert(src_root, ObjectType::Dictionary(dict));
        seed.objects.insert(src_leaf, ObjectType::XRecord(xrec));
        if let Some(e) = seed.get_entity_mut(seed_entity) {
            e.common_mut().xdictionary_handle = Some(src_root);
        }
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[seed_entity]);
        assert_eq!(deps.ext_objects.len(), 1, "capture must snapshot the xdict");

        let mut dst = codec::CadDocument::new();
        let before: Vec<Handle> = dst.objects.keys().copied().collect();
        let (handles, _) = paste_entities_kernel(&mut dst, &entities, &deps, None);

        assert_eq!(handles.len(), 1);
        let pasted = handles[0];
        let new_root = dst
            .get_entity(pasted)
            .and_then(|e| e.common().xdictionary_handle)
            .expect("pasted entity must point at a recreated xdictionary root");
        // Handles are document-local: freshness is judged within the target
        // document, not against the source document's numbers.
        assert!(
            !before.contains(&new_root),
            "recreated root must use a fresh handle in the target document"
        );
        let leaf = match dst.objects.get(&new_root) {
            Some(ObjectType::Dictionary(d)) => {
                assert_eq!(d.entries.len(), 1);
                d.entries[0].1
            }
            other => panic!("expected recreated Dictionary root, got {other:?}"),
        };
        match dst.objects.get(&leaf) {
            Some(ObjectType::XRecord(x)) => assert!(
                matches!(&x.entries[0].value, XRecordValue::Handle(h) if *h == pasted),
                "leaf reference must point at the pasted entity"
            ),
            other => panic!("expected recreated XRecord leaf, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod headless_reuse_proof_tests {
    // Task 5 reuse proof — headless only. This module constructs NO
    // `OpenCADStudio` value and touches no UI/scene state: only
    // `codec::CadDocument`, entities, and the shared kernels
    // (`copy_to_clipboard_kernel`, `paste_entities_kernel`,
    // `match_layer_kernel`, `match_properties_kernel` + `MatchOpts`).
    use super::{copy_to_clipboard_kernel, paste_entities_kernel};
    use crate::entities::match_props::{MatchOpts, match_layer_kernel, match_properties_kernel};

    #[test]
    fn headless_copy_paste_then_match_kernels_share_one_path() {
        // Phase 1: copy in doc A, paste into doc B with a translate.
        let mut doc_a = codec::CadDocument::new();
        doc_a
            .layers
            .add(codec::tables::Layer::new("WALLS"))
            .expect("add layer");
        let mut seed_line = codec::entities::Line::from_points(
            codec::types::Vector3::new(0.0, 0.0, 0.0),
            codec::types::Vector3::new(10.0, 0.0, 0.0),
        );
        seed_line.common.layer = "WALLS".to_string();
        let seed_handle = doc_a
            .add_entity(codec::EntityType::Line(seed_line))
            .unwrap();

        let (entities, deps) = copy_to_clipboard_kernel(&doc_a, &[seed_handle]);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].common().layer, "WALLS");

        let mut doc_b = codec::CadDocument::new();
        let translate =
            crate::command::EntityTransform::Translate(glam::DVec3::new(5.0, 7.0, 0.0));
        let (handles, handle_map) =
            paste_entities_kernel(&mut doc_b, &entities, &deps, Some(&translate));

        assert_eq!(handles.len(), 1);
        let pasted = handles[0];
        assert!(!pasted.is_null());
        assert_ne!(pasted, seed_handle, "paste must use a fresh handle");
        assert_eq!(handle_map.get(&seed_handle), Some(&pasted));
        assert!(doc_b.layers.contains("WALLS"), "layer dep must follow");
        match doc_b.get_entity(pasted) {
            Some(codec::EntityType::Line(l)) => {
                assert_eq!(l.common.layer, "WALLS");
                assert!((l.start.x - 5.0).abs() < 1e-9, "start.x = {}", l.start.x);
                assert!((l.start.y - 7.0).abs() < 1e-9, "start.y = {}", l.start.y);
                assert!((l.end.x - 15.0).abs() < 1e-9, "end.x = {}", l.end.x);
            }
            other => panic!("expected pasted Line, got {other:?}"),
        }

        // Phase 2: match kernels on a src/dst pair in the same headless doc.
        let mut src_line = codec::entities::Line::new();
        src_line.common.layer = "WALLS".to_string();
        src_line.common.linetype_scale = 2.5;
        src_line.thickness = 1.25;
        let src = doc_b
            .add_entity(codec::EntityType::Line(src_line))
            .unwrap();
        let mut dst_line = codec::entities::Line::new();
        dst_line.common.layer = "0".to_string();
        dst_line.common.linetype_scale = 1.0;
        dst_line.thickness = 0.0;
        let dst = doc_b
            .add_entity(codec::EntityType::Line(dst_line))
            .unwrap();

        let count = match_layer_kernel(&mut doc_b, &[dst], src).expect("match layer");
        assert_eq!(count, 1);
        assert_eq!(
            doc_b.get_entity(dst).unwrap().common().layer,
            "WALLS",
            "match_layer_kernel must transfer the layer"
        );

        let src_clone = doc_b.get_entity(src).cloned().unwrap();
        {
            let dst_entity = doc_b.get_entity_mut(dst).unwrap();
            match_properties_kernel(&src_clone, dst_entity, &MatchOpts::all());
        }
        match doc_b.get_entity(dst) {
            Some(codec::EntityType::Line(l)) => {
                assert_eq!(l.common.layer, "WALLS");
                assert!((l.common.linetype_scale - 2.5).abs() < 1e-9);
                assert!((l.thickness - 1.25).abs() < 1e-9);
            }
            other => panic!("expected matched Line, got {other:?}"),
        }
    }
}
