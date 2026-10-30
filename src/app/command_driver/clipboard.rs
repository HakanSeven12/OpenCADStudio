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

    /// ARRAYRECT / ARRAYPOLAR with Associative on: the objects become the
    /// source of an associative array, in one undo step.
    pub(super) fn handle_assoc_array(
        &mut self,
        mut handles: Vec<Handle>,
        spec: crate::scene::assoc_array::AssocArraySpec,
    ) {
        let i = self.active_tab;
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        if handles.is_empty() {
            return;
        }
        let label = self.history_label_from_active_cmd(i, "ARRAY");
        let source_point = crate::app::helpers::entities_lower_left_by_bbox(
            &self.tabs[i].scene.document,
            &handles,
        );
        self.push_undo_snapshot(i, label.clone());
        match crate::scene::assoc_array::create_from_spec(
            &mut self.tabs[i].scene,
            &handles,
            source_point,
            &spec,
        ) {
            Ok(array) => {
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entity(array, false);
                self.command_line
                    .push_output(crate::tf!("{label}: associative array created.").as_ref());
            }
            Err(error) => self.command_line.push_error(&format!("{label}: {error}")),
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
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
        let mut replaced_handles = Vec::with_capacity(replacements.len());
        for (handle, entities) in replacements {
            let new_handles = self.replace_command_entity(i, handle, entities);
            replaced_handles.push((handle, new_handles));
        }
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.on_entities_replaced(&replaced_handles);
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
        let hatch_detached = self.tabs[i].active_cmd.as_ref().is_some_and(|cmd| cmd.name() == "TRIM")
            && matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(codec::EntityType::Hatch(hatch)) if hatch.is_associative
            )
            && new_entities.iter().any(|entity| matches!(
                entity, codec::EntityType::Hatch(hatch) if !hatch.is_associative
            ));
        let label = self.history_label_from_active_cmd(i, "TRIM");
        self.push_undo_snapshot(i, label);
        // Apply one-to-one edits in place. Erase/add with the same handle
        // duplicates its owning block membership and emits duplicate DXF records.
        let new_handles = self.replace_command_entity(i, handle, new_entities);
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
        if hatch_detached {
            self.command_line.push_info(
                "TRIM: hatch boundary updated; associativity removed because its original source objects were not modified.",
            );
        }
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
    /// Also merges the multileader styles, and returns the map the clipboard's
    /// multileaders are remapped with ([`remap_mleader`]).
    pub(crate) fn merge_clipboard_blocks(
        &mut self,
        i: usize,
    ) -> std::collections::HashMap<Handle, Handle> {
        let deps = &self.clipboard_deps;
        let document = &mut self.tabs[i].scene.document;
        let first_new_handle = document.next_handle();
        merge_app_ids(document, deps);
        let apps = eed_app_map(document, deps);
        let images = merge_image_definitions(document, deps);
        // Single shared copy — see `merge_block_defs`. One batched geometry
        // rebuild instead of one per definition; same end state as the
        // per-definition bumps `Scene::define_block_raw` used to run.
        let (created, mleaders) = merge_blocks_and_mleader_styles(document, deps, &apps, &images);
        ensure_pasted_image_reactors(&mut self.tabs[i].scene, &[], deps, first_new_handle);
        if created > 0 {
            self.tabs[i].scene.bump_geometry();
        }
        mleaders
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
        // `&mut Document` can do and this shell keeps the rest —
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
        // Into the space new entities go: the edited block, the layout's
        // paper space (no viewport active), or model space.
        let scene = &self.tabs[i].scene;
        let layout = scene.current_layout.clone();
        let space = match scene.block_edit_block {
            Some(block) => PasteSpace::Block(block),
            None if layout != "Model" && scene.active_viewport.is_none() => {
                PasteSpace::Layout(&layout)
            }
            None => PasteSpace::Model,
        };
        let first_new_handle = self.tabs[i].scene.document.next_handle();
        let (by_index, handle_map) = paste_entities_kernel(
            &mut self.tabs[i].scene.document,
            &clipboard,
            &deps,
            translate.as_ref(),
            space,
        );
        ensure_pasted_image_reactors(&mut self.tabs[i].scene, &by_index, &deps, first_new_handle);
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
            // `add_entity` seeds fill and raster models; the document-level
            // add above does not, so a pasted hatch without an association
            // to rebuild it was never drawn. (#1807)
            if matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(
                    codec::EntityType::Hatch(_)
                        | codec::EntityType::Solid(_)
                        | codec::EntityType::RasterImage(_)
                        | codec::EntityType::Ole2Frame(_)
                        | codec::EntityType::Underlay(_)
                )
            ) {
                self.tabs[i].scene.reseed_derived_caches(handle);
            }
            if matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(codec::EntityType::Block(_) | codec::EntityType::BlockEnd(_))
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
                let _ = self.tabs[i].scene.sync_displayed_annotation_context(handle);
            }
        }
        // Recreate any group whose whole membership was copied, so a pasted
        // group stays grouped — cross-drawing too, since the groups were
        // snapshotted into the clipboard at copy time. `by_index` is aligned
        // with `self.clipboard`, so the kernel's `handle_map` maps each
        // clipboard clone to its new handle. Same shared `recreate_groups`
        // the in-drawing COPY path uses. (#440)
        if !deps.groups.is_empty() {
            self.tabs[i]
                .scene
                .recreate_groups(deps.groups.clone(), &handle_map);
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
        let records = block_record_map(doc, &self.clipboard_deps.blocks);
        for cap in &captures {
            if let Some(new_root) = recreate_ext_subtree(doc, cap, None, &records) {
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
/// Returns the names of the dimension styles it added.
pub(crate) fn merge_table_records(
    doc: &mut codec::Document,
    deps: &crate::app::ClipboardDeps,
) -> Vec<String> {
    use codec::TableEntry;
    for rec in &deps.layers {
        if !doc.layers.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            let _ = doc.layers.add(r);
        }
    }
    // Shape and text segments name their style by handle: find the target's
    // matching style (a shape file by its font file, it has no name) or
    // recreate it, so a pasted linetype never points into the source drawing.
    let mut segment_styles: rustc_hash::FxHashMap<Handle, Handle> =
        rustc_hash::FxHashMap::default();
    for style in &deps.linetype_styles {
        let existing = doc
            .text_styles
            .iter()
            .find(|s| {
                s.is_shape_file == style.is_shape_file
                    && if style.is_shape_file {
                        s.font_file.eq_ignore_ascii_case(&style.font_file)
                    } else {
                        s.name.eq_ignore_ascii_case(&style.name)
                    }
            })
            .map(|s| s.handle);
        let handle = existing.unwrap_or_else(|| {
            let mut r = style.clone();
            r.set_handle(doc.allocate_handle());
            let handle = r.handle;
            doc.text_styles.add_allow_duplicate(r);
            handle
        });
        segment_styles.insert(style.handle, handle);
    }
    for rec in &deps.linetypes {
        if !doc.line_types.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            for element in &mut r.elements {
                if let Some(complex) = element.complex.as_mut() {
                    if let Some(new_style) = segment_styles.get(&complex.style_handle) {
                        complex.style_handle = *new_style;
                    }
                }
            }
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
    let mut added_dim_styles = Vec::new();
    for rec in &deps.dim_styles {
        if !doc.dim_styles.contains(rec.name()) {
            let mut r = rec.clone();
            r.set_handle(doc.allocate_handle());
            repoint_dim_style(doc, &mut r, &deps.dim_style_refs);
            added_dim_styles.push(r.name.clone());
            let _ = doc.dim_styles.add(r);
        }
    }
    merge_app_ids(doc, deps);
    added_dim_styles
}

/// The arrow blocks a dimension style names.
pub(crate) fn dim_style_blocks(style: &codec::tables::DimStyle) -> [Handle; 4] {
    [style.dimblk, style.dimblk1, style.dimblk2, style.dimldrblk]
}

/// The entity, context and override references that must never retain source handles.
pub(crate) fn mleader_handles(m: &mut codec::entities::MultiLeader) -> Vec<&mut Option<Handle>> {
    let mut handles = vec![
        &mut m.style_handle,
        &mut m.text_style_handle,
        &mut m.line_type_handle,
        &mut m.arrowhead_handle,
        &mut m.block_content_handle,
        &mut m.context.text_style_handle,
        &mut m.context.block_content_handle,
    ];
    for root in &mut m.context.leader_roots {
        for line in &mut root.lines {
            handles.extend([&mut line.line_type_handle, &mut line.arrowhead_handle]);
        }
    }
    handles.extend(
        m.arrowhead_overrides
            .iter_mut()
            .map(|a| &mut a.arrowhead_handle),
    );
    handles.extend(
        m.block_attributes
            .iter_mut()
            .map(|a| &mut a.attribute_definition_handle),
    );
    handles
}

pub(crate) fn remap_mleader(entity: &mut codec::EntityType, map: &std::collections::HashMap<Handle, Handle>) {
    if let codec::EntityType::MultiLeader(m) = entity {
        for h in mleader_handles(m) {
            *h = h
                .and_then(|h| map.get(&h).copied())
                .filter(|h| !h.is_null());
        }
    }
}

/// Merge named styles after blocks exist, keeping target definitions on a name conflict.
fn merge_mleader_styles(
    doc: &mut codec::Document,
    deps: &crate::app::ClipboardDeps,
) -> std::collections::HashMap<Handle, Handle> {
    use codec::objects::ObjectType;
    let mut map = block_record_map(doc, &deps.blocks);
    for source in &deps.text_styles {
        if let Some(target) = doc.text_styles.get(&source.name) {
            map.insert(source.handle, target.handle);
        }
    }
    for source in &deps.linetypes {
        if let Some(target) = doc.line_types.get(&source.name) {
            map.insert(source.handle, target.handle);
        }
    }
    for def in &deps.blocks {
        let Some(&record) = map.get(&def.source) else {
            continue;
        };
        let members = crate::scene::block_authoring::members(doc, record);
        for source in &def.entities {
            let codec::EntityType::AttributeDefinition(a) = source else {
                continue;
            };
            if let Some(target) = members.iter().find(|&&h| matches!(doc.get_entity(h),
                Some(codec::EntityType::AttributeDefinition(b)) if a.tag.eq_ignore_ascii_case(&b.tag)))
            {
                map.insert(a.common.handle, *target);
            }
        }
    }
    for source in &deps.mleader_styles {
        let existing = doc.objects.iter().find_map(|(&h, o)| match o {
            ObjectType::MultiLeaderStyle(s) if s.name.eq_ignore_ascii_case(&source.name) => Some(h),
            _ => None,
        });
        let target = existing.unwrap_or_else(|| {
            let mut s = source.clone();
            s.handle = doc.allocate_handle();
            for h in [
                &mut s.text_style_handle,
                &mut s.line_type_handle,
                &mut s.arrowhead_handle,
                &mut s.block_content_handle,
            ] {
                *h = h.and_then(|h| map.get(&h).copied());
            }
            let h = s.handle;
            doc.objects.insert(h, ObjectType::MultiLeaderStyle(s));
            h
        });
        map.insert(source.handle, target);
    }
    if !deps.mleader_styles.is_empty() {
        crate::app::style_ops::sync_mleaderstyle_dictionary(doc);
    }
    map
}

/// This drawing's record named like source record `source`, or null.
fn dim_style_ref<T: codec::TableEntry>(
    table: &codec::tables::Table<T>,
    refs: &rustc_hash::FxHashMap<Handle, String>,
    source: Handle,
) -> Handle {
    refs.get(&source)
        .and_then(|name| table.get(name))
        .map_or(Handle::NULL, |record| record.handle())
}

/// Point a pasted dimension style, which still holds the source drawing's
/// handles, at this drawing's text style, linetypes and arrow blocks of the
/// same names (`refs` names each source handle). A record this drawing lacks
/// is cleared, so the style falls back to the default instead of naming an
/// object that is not there.
pub(crate) fn repoint_dim_style(
    doc: &codec::Document,
    style: &mut codec::tables::DimStyle,
    refs: &rustc_hash::FxHashMap<Handle, String>,
) {
    style.dimtxsty_handle = dim_style_ref(&doc.text_styles, refs, style.dimtxsty_handle);
    for h in [
        &mut style.dimltex_handle,
        &mut style.dimltex1_handle,
        &mut style.dimltex2_handle,
    ] {
        *h = dim_style_ref(&doc.line_types, refs, *h);
    }
    for h in [
        &mut style.dimblk,
        &mut style.dimblk1,
        &mut style.dimblk2,
        &mut style.dimldrblk,
    ] {
        *h = dim_style_ref(&doc.block_records, refs, *h);
    }
}

/// Point the dimension styles a paste added (`added`) at the arrow blocks
/// the block merge has since recreated: the table merge ran first, while
/// this drawing still lacked them.
fn repoint_added_dim_style_blocks(
    doc: &mut codec::Document,
    deps: &crate::app::ClipboardDeps,
    added: &[String],
) {
    for rec in deps.dim_styles.iter().filter(|r| added.contains(&r.name)) {
        let blocks = dim_style_blocks(rec)
            .map(|h| dim_style_ref(&doc.block_records, &deps.dim_style_refs, h));
        if let Some(style) = doc.dim_styles.get_mut(&rec.name) {
            [style.dimblk, style.dimblk1, style.dimblk2, style.dimldrblk] = blocks;
        }
    }
}

/// Register the copied XDATA's applications this drawing lacks.
pub(crate) fn merge_app_ids(doc: &mut codec::Document, deps: &crate::app::ClipboardDeps) {
    for rec in &deps.app_ids {
        if !doc.app_ids.contains(&rec.name) {
            let mut r = rec.clone();
            r.handle = doc.allocate_handle();
            let _ = doc.app_ids.add(r);
        }
    }
}

/// The `ACAD_IMAGE_DICT` dictionary under the named-objects root, created
/// when the drawing has none. `None` when the drawing has no root dictionary.
fn ensure_image_dictionary(document: &mut codec::Document) -> Option<Handle> {
    use codec::objects::{Dictionary, ObjectType};
    let root = document.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get(&root) else {
        return None;
    };
    if let Some(existing) = root_dictionary.get("ACAD_IMAGE_DICT") {
        if matches!(
            document.objects.get(&existing),
            Some(ObjectType::Dictionary(_))
        ) {
            return Some(existing);
        }
    }
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = root;
    document
        .objects
        .insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get_mut(&root) {
        root_dictionary.add_entry("ACAD_IMAGE_DICT", handle);
    }
    Some(handle)
}

/// Register image definition `definition` in ACAD_IMAGE_DICT under its file's
/// name (made unique), like an image AutoCAD attached. Returns the dictionary,
/// which is to own the definition; `None` when the drawing has no root
/// dictionary.
pub(crate) fn register_image_definition(
    document: &mut codec::Document,
    definition: Handle,
    file_path: &str,
) -> Option<Handle> {
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("IMAGE")
        .to_owned();
    let dictionary = ensure_image_dictionary(document)?;
    if let Some(codec::objects::ObjectType::Dictionary(entries)) =
        document.objects.get_mut(&dictionary)
    {
        let mut key = stem.clone();
        let mut suffix = 1;
        while entries.get(&key).is_some() {
            suffix += 1;
            key = format!("{stem}_{suffix}");
        }
        entries.add_entry(key, definition);
    }
    Some(dictionary)
}

// The kernel has allocated the final entity handles. Register reactors in the
// Scene shell so paste's existing undo scope captures objects and entity links.
fn ensure_pasted_image_reactors(
    scene: &mut crate::scene::Scene,
    handles: &[Handle],
    deps: &crate::app::ClipboardDeps,
    first_new_handle: u64,
) {
    let mut images = handles.to_vec();
    for block in &deps.blocks {
        if let Some(record) = scene
            .document
            .block_records
            .get(&block.name)
            .filter(|record| record.handle.value() >= first_new_handle)
        {
            // Only newly imported members; existing definitions belong to the
            // target drawing and COPY of an INSERT keeps their images intact.
            images.extend(crate::scene::block_authoring::members(
                &scene.document,
                record.handle,
            ));
        }
    }
    for handle in images {
        let Some(codec::EntityType::RasterImage(image)) = scene.document.get_entity(handle) else {
            continue;
        };
        let mut entity = codec::EntityType::RasterImage(image.clone());
        scene.ensure_image_definition(&mut entity);
        *scene.document.get_entity_mut(handle).expect("pasted image") = entity;
    }
}

/// Source image definition → this drawing's definition of the same file (one
/// definition per file, as AutoCAD keeps it), recreated and registered in
/// ACAD_IMAGE_DICT when the drawing has none. Pasting within the drawing
/// finds the very same object.
pub(crate) fn merge_image_definitions(
    doc: &mut codec::Document,
    deps: &crate::app::ClipboardDeps,
) -> rustc_hash::FxHashMap<Handle, Handle> {
    use codec::objects::ObjectType;
    let mut map = rustc_hash::FxHashMap::default();
    for definition in &deps.image_definitions {
        let same_file = |d: &codec::objects::ImageDefinition| {
            d.file_name.eq_ignore_ascii_case(&definition.file_name)
        };
        let existing = match doc.objects.get(&definition.handle) {
            Some(ObjectType::ImageDefinition(d)) if same_file(d) => Some(definition.handle),
            _ => doc
                .objects
                .iter()
                .find_map(|(handle, object)| match object {
                    ObjectType::ImageDefinition(d) if same_file(d) => Some(*handle),
                    _ => None,
                }),
        };
        let handle = existing.unwrap_or_else(|| {
            let handle = doc.allocate_handle();
            let mut copy = definition.clone();
            copy.handle = handle;
            // The source images' reactors; pasted images drop theirs.
            copy.reactors.clear();
            copy.owner = register_image_definition(doc, handle, &definition.file_name)
                .unwrap_or(Handle::NULL);
            doc.objects
                .insert(handle, ObjectType::ImageDefinition(copy));
            handle
        });
        map.insert(definition.handle, handle);
    }
    map
}

/// Point a pasted raster image at this drawing's image definition. Its
/// reactor link is dropped, as binding an xref does: the reactor belongs to
/// the source image, also when pasting within the same drawing.
fn remap_image_definition(
    entity: &mut codec::EntityType,
    definitions: &rustc_hash::FxHashMap<Handle, Handle>,
) {
    let codec::EntityType::RasterImage(image) = entity else {
        return;
    };
    if let Some(definition) = image.definition_handle {
        if let Some(&target) = definitions.get(&definition) {
            image.definition_handle = Some(target);
        }
    }
    image.definition_reactor_handle = None;
}

/// The block records a table's block cells show.
pub(crate) fn table_cell_blocks(
    table: &codec::entities::Table,
) -> impl Iterator<Item = Handle> + '_ {
    table
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .flat_map(|cell| &cell.contents)
        .filter_map(|content| content.block_handle)
}

/// Point a pasted table's block cells at this drawing's records of the same
/// blocks. `records` maps source records to target records.
fn remap_table_cell_blocks(
    table: &mut codec::entities::Table,
    records: &std::collections::HashMap<Handle, Handle>,
) {
    let contents = table
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
        .flat_map(|cell| &mut cell.contents);
    for content in contents {
        if let Some(block) = content.block_handle.as_mut() {
            if let Some(&target) = records.get(block) {
                *block = target;
            }
        }
    }
}

/// Source APPID handle → this drawing's APPID of the same name, for the
/// copied XDATA. Run after [`merge_app_ids`].
pub(crate) fn eed_app_map(
    doc: &codec::Document,
    deps: &crate::app::ClipboardDeps,
) -> rustc_hash::FxHashMap<u64, u64> {
    deps.app_ids
        .iter()
        .filter_map(|a| Some((a.handle.value(), doc.app_ids.get(&a.name)?.handle.value())))
        .collect()
}

/// Re-point a copied entity's verbatim EED at this drawing's APPIDs. A block
/// whose application the map does not know names no APPID here, and a DWG
/// carrying one is rejected outright by the reference program, so it goes.
pub(crate) fn remap_eed_apps(entity: &mut codec::EntityType, apps: &rustc_hash::FxHashMap<u64, u64>) {
    let remap = |xdata: &mut codec::xdata::ExtendedData| {
        xdata.raw_dwg_eed.retain_mut(|(app, _)| match apps.get(app) {
            Some(&target) => {
                *app = target;
                true
            }
            None => false,
        });
    };
    remap(&mut entity.common_mut().extended_data);
    if let codec::EntityType::Insert(insert) = entity {
        for attribute in &mut insert.attributes {
            remap(&mut attribute.common.extended_data);
        }
    }
}

/// Merge the clipboard's block definitions and multileader styles, and point
/// the multileaders inside newly added blocks at this drawing's records.
/// Returns how many blocks were added and the map the pasted multileaders are
/// remapped with ([`remap_mleader`]).
fn merge_blocks_and_mleader_styles(
    doc: &mut codec::Document,
    deps: &crate::app::ClipboardDeps,
    apps: &rustc_hash::FxHashMap<u64, u64>,
    images: &rustc_hash::FxHashMap<Handle, Handle>,
) -> (usize, std::collections::HashMap<Handle, Handle>) {
    let new_blocks: Vec<_> = deps
        .blocks
        .iter()
        .filter(|d| !doc.block_records.contains(&d.name))
        .collect();
    let created = merge_block_defs(doc, &deps.blocks, apps, images);
    let records = merge_mleader_styles(doc, deps);
    for def in new_blocks {
        if let Some(record) = doc.block_records.get(&def.name).map(|r| r.handle) {
            for member in crate::scene::block_authoring::members(doc, record) {
                if let Some(entity) = doc.get_entity_mut(member) {
                    remap_mleader(entity, &records);
                }
            }
        }
    }
    (created, records)
}

/// Recreate block definitions the pasted INSERTs reference but the target
/// document lacks. Shared by `merge_clipboard_blocks` and
/// `paste_entities_kernel`. Returns how many definitions were created, so
/// render-cache callers can bump once instead of per definition.
/// `images` maps source image definitions to this drawing's (see
/// [`merge_image_definitions`]), for raster images inside the blocks.
pub(crate) fn merge_block_defs(
    doc: &mut codec::Document,
    blocks: &[crate::app::BlockDef],
    apps: &rustc_hash::FxHashMap<u64, u64>,
    images: &rustc_hash::FxHashMap<Handle, Handle>,
) -> usize {
    let mut created = Vec::new();
    for def in blocks {
        let before = doc.block_records.len();
        let mut entities = def.entities.clone();
        for entity in &mut entities {
            remap_eed_apps(entity, apps);
            remap_image_definition(entity, images);
        }
        define_block_raw_doc(doc, &def.name, def.base_point, &entities);
        if doc.block_records.len() > before {
            created.push(def);
        }
    }
    // Graphs and table cells name other records (an evaluated copy names its
    // definition), so they are re-pointed once every record exists.
    let records = block_record_map(doc, blocks);
    for def in &created {
        if let Some(record) = doc.block_records.get(&def.name).map(|r| r.handle) {
            for member in crate::scene::block_authoring::members(doc, record) {
                if let Some(codec::EntityType::Table(table)) = doc.get_entity_mut(member) {
                    remap_table_cell_blocks(table, &records);
                }
            }
        }
        import_block_def_graph(doc, def, &records);
    }
    created.len()
}

/// Source record → this drawing's record of the same name, per definition.
pub(crate) fn block_record_map(
    doc: &codec::Document,
    blocks: &[crate::app::BlockDef],
) -> std::collections::HashMap<Handle, Handle> {
    blocks
        .iter()
        .filter_map(|def| Some((def.source, doc.block_records.get(&def.name)?.handle)))
        .collect()
}

/// Carry `def`'s object graph (a dynamic block's parameters and states) onto
/// the record just defined from it here. `records` maps the source records
/// the graph may name.
pub(crate) fn import_block_def_graph(
    doc: &mut codec::Document,
    def: &crate::app::BlockDef,
    records: &std::collections::HashMap<Handle, Handle>,
) {
    let Some(graph) = &def.graph else {
        return;
    };
    let Some(record) = doc.block_records.get(&def.name).map(|record| record.handle) else {
        return;
    };
    let members = crate::scene::block_authoring::members(doc, record);
    if members.len() != def.entities.len() {
        return;
    }
    let mut map = records.clone();
    map.extend(def.entities.iter().map(|e| e.common().handle).zip(members));
    doc.import_block_graph(graph, record, &map);
    crate::scene::block_authoring::retag_members(doc, record);
}

/// (`NULL` where an add failed) plus the source-handle → pasted-handle map.
///
/// Document-level work lives here; Scene/render concerns stay caller-side in
/// `finalize_paste` (tessellation bumps, `populate_missing_meshes`,
/// `recreate_groups`, `duplicate_parametric_constraints_for`,
/// `sync_displayed_annotation_context`, undo/dirty/selection/echo/panels).
/// See the per-helper report in `finalize_paste`.
/// Where pasted entities go, the way `Scene::add_entity` routes a new one.
#[derive(Clone, Copy)]
pub(crate) enum PasteSpace<'a> {
    Model,
    /// A layout's paper space, when no viewport of it is active.
    Layout(&'a str),
    /// The block definition being edited (BEDIT).
    Block(Handle),
}

pub(crate) fn paste_entities_kernel(
    doc: &mut codec::Document,
    entities: &[codec::EntityType],
    deps: &crate::app::ClipboardDeps,
    translate: Option<&crate::command::EntityTransform>,
    space: PasteSpace,
) -> (Vec<Handle>, rustc_hash::FxHashMap<Handle, Handle>) {
    // Shared table-record merge (also used by `merge_dependencies`): recreate
    // missing records with fresh handles from the target document.
    let added_dim_styles = merge_table_records(doc, deps);
    // Shared block-definition merge (also used by `merge_clipboard_blocks`).
    let apps = eed_app_map(doc, deps);
    let image_definitions = merge_image_definitions(doc, deps);
    let (_, records) = merge_blocks_and_mleader_styles(doc, deps, &apps, &image_definitions);
    repoint_added_dim_style_blocks(doc, deps, &added_dim_styles);

    let by_index: Vec<Handle> = entities
        .iter()
        .cloned()
        .map(|mut entity| {
            remap_eed_apps(&mut entity, &apps);
            remap_image_definition(&mut entity, &image_definitions);
            remap_mleader(&mut entity, &records);
            if let codec::EntityType::Table(table) = &mut entity {
                remap_table_cell_blocks(table, &records);
            }
            let picture_placement = crate::scene::render_graph::dim_picture_placement(&entity);
            if let Some(t) = translate {
                crate::scene::view::dispatch::apply_transform(&mut entity, t);
            }
            crate::scene::render_graph::clear_dim_picture_placement(&mut entity);
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
                        let mut subs = subs;
                        if let Some(placement) = &picture_placement {
                            for sub in &mut subs {
                                crate::scene::view::dispatch::apply_transform(sub, placement);
                            }
                        }
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
            add_entity_clone_doc(doc, entity, space)
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
        if let Some(new_root) = recreate_ext_subtree(doc, cap, Some(new_entity), &records) {
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

    // Source handles map one-to-one to the freshly pasted handles; reconnect
    // LEADER -> copied annotation through the map (the
    // `sync_displayed_annotation_context` refresh stays caller-side).
    let mut handle_map: rustc_hash::FxHashMap<Handle, Handle> = rustc_hash::FxHashMap::default();
    for (source, &copied) in entities.iter().zip(by_index.iter()) {
        if !copied.is_null() {
            handle_map.insert(source.common().handle, copied);
        }
    }
    // A pasted associative hatch follows the pasted boundary, or drops the
    // association when the boundary was not pasted with it. (#1370)
    crate::scene::remap_hatch_associations(doc, &handle_map);
    // Reactors on the source drawing's objects do not travel with the paste.
    crate::scene::remap_copied_reactors(doc, &handle_map);
    // A pasted clipped viewport is clipped by the pasted boundary.
    crate::scene::remap_copied_viewport_clips(doc, &handle_map, true);
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

/// Document-level clone-add: the `&mut Document` half of
/// `Scene::add_entity_clone` — fresh inline sub-handles
/// (`Scene::reset_clone_subhandles`), NULL top-level/owner handles, then
/// adds it to `space`. Tessellation/render caching stays caller-side in the
/// Scene shell.
fn add_entity_clone_doc(
    doc: &mut codec::Document,
    mut entity: codec::EntityType,
    space: PasteSpace,
) -> Handle {
    crate::scene::Scene::reset_clone_subhandles(doc, &mut entity);
    entity.common_mut().handle = Handle::NULL;
    entity.common_mut().owner_handle = Handle::NULL;
    match space {
        PasteSpace::Model => doc.add_entity(entity),
        PasteSpace::Layout(layout) => doc.add_entity_to_layout(entity, layout),
        PasteSpace::Block(block) => {
            entity.common_mut().owner_handle = block;
            doc.add_entity(entity)
        }
    }
    .unwrap_or(Handle::NULL)
}

/// Document-level verbatim block-definition recreate: the `&mut Document`
/// half of `Scene::define_block_raw` (no-op when the block exists). The
/// trailing `bump_geometry` stays caller-side.
pub(crate) fn define_block_raw_doc(
    doc: &mut codec::Document,
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
    block_record.flags.anonymous =
        name.starts_with('*') && !(block_record.is_model_space() || block_record.is_paper_space());
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
    doc: &mut codec::Document,
    entity: &mut codec::EntityType,
    owner: Handle,
) {
    crate::scene::Scene::reset_clone_subhandles(doc, entity);
    entity.common_mut().handle = Handle::NULL;
    entity.common_mut().owner_handle = owner;
    // An extension dictionary is exclusively owned; the clone must not claim
    // the source's (cross-drawing it would dangle into the source document),
    // or file validation reports "Extension dictionary <H> Cannot access".
    // Paste reattaches a recreated subtree afterwards (finalize_paste).
    entity.common_mut().xdictionary_handle = None;
}

/// Document-level fresh `*D<n>` block from `subs`: the `&mut Document`
/// half of `Scene::define_transformed_block` (which has no render-cache tail,
/// so this is a verbatim port). Returns the new block name, or `None` when
/// `subs` is empty.
fn define_transformed_block_doc(
    doc: &mut codec::Document,
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
    // See `Scene::define_transformed_block`.
    br.flags.anonymous = true;
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
    doc: &codec::Document,
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
    doc: &mut codec::Document,
    cap: &crate::app::ClipExtObjects,
    entity_handle: Option<Handle>,
    records: &std::collections::HashMap<Handle, Handle>,
) -> Option<Handle> {
    use std::collections::HashMap;
    let mut remap: HashMap<Handle, Handle> = records.clone();
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
        // A dynamic reference's link to its definition, indexed.
        if let codec::objects::ObjectType::DynamicBlock(object) = &obj {
            if let codec::objects::DynamicBlockData::Representation(link) = &object.data {
                if object.dxf_name == "ACDB_BLOCKREPRESENTATION_DATA" {
                    doc.block_representations.insert(new_h, link.block);
                }
            }
        }
        doc.objects.insert(new_h, obj);
    }
    remap.get(&cap.root).copied()
}

/// Replace references to a clipboard entity inside one recreated extension
/// dictionary graph after its final block-owned handle becomes known.
pub(crate) fn remap_ext_subtree_reference(
    doc: &mut codec::Document,
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
            for r in x.reactors.iter_mut() {
                *r = map(*r);
            }
            if let Some(d) = x.xdictionary_handle.as_mut() {
                *d = map(*d);
            }
            for entry in &mut x.entries {
                if let codec::objects::XRecordValue::Handle(handle) = &mut entry.value {
                    *handle = map(*handle);
                }
            }
            // The DWG writer can emit these instead of the entries.
            for reference in &mut x.object_references {
                reference.handle = map(reference.handle);
            }
            // The verbatim DWG record still carries the source handles.
            x.raw_dwg_data = None;
        }
        ObjectType::TableContent(t) => {
            t.common.handle = new_handle;
            t.common.owner_handle = map(t.common.owner_handle);
            for r in t.common.reactors.iter_mut() {
                *r = map(*r);
            }
            if let Some(d) = t.common.xdictionary_handle.as_mut() {
                *d = map(*d);
            }
            remap_table_cell_blocks(t, remap);
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
        ObjectType::DynamicBlock(object) => {
            object.handle = new_handle;
            object.visit_handles_mut(&mut |h| *h = map(*h));
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
        let mut src = codec::Document::new();
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

    fn leaf_of(doc: &codec::Document, root: Handle) -> Handle {
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
        let mut dst = codec::Document::new();
        let before: Vec<Handle> = dst.objects.keys().copied().collect();
        let dst_entity = dst.allocate_handle();

        let new_root = recreate_ext_subtree(&mut dst, &cap, Some(dst_entity), &Default::default())
            .expect("recreated xdictionary root");
        assert!(!new_root.is_null());
        assert!(!before.contains(&new_root), "clone must use a fresh handle");
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
        let mut dst = codec::Document::new();
        let before_len = dst.objects.len();

        let root = recreate_ext_subtree(&mut dst, &cap, None, &Default::default())
            .expect("recreated xdictionary root");
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
        let mut scratch = codec::Document::new();
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
        let mut doc = codec::Document::new();
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
        let mut doc = codec::Document::new();
        let kept = doc.add_entity(line_on("0")).unwrap();
        let missing = doc.allocate_handle();

        let (entities, _) = copy_to_clipboard_kernel(&doc, &[kept, missing]);

        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].common().handle, kept);
    }

    #[test]
    fn empty_selection_yields_empty_payload() {
        let doc = codec::Document::new();

        let (entities, deps) = copy_to_clipboard_kernel(&doc, &[]);

        assert!(entities.is_empty());
        assert!(deps.layers.is_empty());
        assert!(deps.blocks.is_empty());
    }
}

#[cfg(test)]
mod paste_entities_kernel_tests {
    use super::{
        copy_to_clipboard_kernel, define_block_raw_doc, mleader_handles, paste_entities_kernel,
        remap_object, table_cell_blocks,
    };
    use codec::Handle;

    fn line_on(layer: &str) -> codec::EntityType {
        let mut line = codec::entities::Line::new();
        line.common.layer = layer.to_string();
        codec::EntityType::Line(line)
    }

    #[test]
    fn pastes_with_fresh_handles_and_translate() {
        let mut seed = codec::Document::new();
        seed.layers
            .add(codec::tables::Layer::new("WALLS"))
            .expect("add layer");
        // Position the seed line away from the origin so the translate is visible.
        let mut seed_line = codec::entities::Line::from_points(
            codec::types::Vector3::new(0.0, 0.0, 0.0),
            codec::types::Vector3::new(10.0, 0.0, 0.0),
        );
        seed_line.common.layer = "WALLS".to_string();
        let seed_handle = seed.add_entity(codec::EntityType::Line(seed_line)).unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[seed_handle]);

        let mut dst = codec::Document::new();
        let delta = glam::DVec3::new(5.0, 7.0, 0.0);
        let translate = crate::command::EntityTransform::Translate(delta);
        let (handles, handle_map) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            Some(&translate),
            crate::app::PasteSpace::Model,
        );

        assert_eq!(handles.len(), 1);
        let pasted = handles[0];
        assert!(!pasted.is_null());
        assert_ne!(pasted, seed_handle, "paste must use a fresh handle");
        assert_eq!(handle_map.get(&seed_handle), Some(&pasted));
        let line = match dst.get_entity(pasted) {
            Some(codec::EntityType::Line(l)) => l,
            other => panic!("expected pasted Line, got {other:?}"),
        };
        assert!(
            (line.start.x - 5.0).abs() < 1e-9,
            "start.x = {}",
            line.start.x
        );
        assert!(
            (line.start.y - 7.0).abs() < 1e-9,
            "start.y = {}",
            line.start.y
        );
        assert!((line.end.x - 15.0).abs() < 1e-9, "end.x = {}", line.end.x);
        // The layer dependency must have been recreated in the target document.
        assert!(dst.layers.contains("WALLS"));
        assert_eq!(line.common.layer, "WALLS");
    }

    #[test]
    fn pasted_complex_linetype_points_at_the_targets_shape_style() {
        use codec::tables::linetype::{
            LineTypeComplexContent, LineTypeComplexData, LineTypeElement,
        };
        // A shape-file style has no name; a linetype segment names it by handle.
        let mut seed = codec::Document::new();
        let mut shapes = codec::tables::TextStyle::new("");
        shapes.handle = seed.allocate_handle();
        shapes.is_shape_file = true;
        shapes.font_file = "ltypeshp.shx".to_string();
        let shapes_handle = shapes.handle;
        seed.text_styles.add_allow_duplicate(shapes);
        let mut batting = codec::tables::LineType::new("BATTING");
        batting.add_element(LineTypeElement::dash(0.5));
        batting.add_element(LineTypeElement {
            length: -0.1,
            complex: Some(LineTypeComplexData {
                content: LineTypeComplexContent::Shape { shape_number: 135 },
                style_handle: shapes_handle,
                ..Default::default()
            }),
        });
        seed.line_types.add(batting).expect("add linetype");
        let mut line = codec::entities::Line::new();
        line.common.linetype = "BATTING".to_string();
        let source = seed.add_entity(codec::EntityType::Line(line)).unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[source]);

        let segment_style = |doc: &codec::Document| {
            doc.line_types
                .get("BATTING")
                .expect("pasted linetype")
                .elements
                .iter()
                .find_map(|element| element.complex.as_ref())
                .expect("shape segment")
                .style_handle
        };
        let shape_styles = |doc: &codec::Document| {
            doc.text_styles
                .iter()
                .filter(|s| s.is_shape_file && s.font_file == "ltypeshp.shx")
                .map(|s| s.handle)
                .collect::<Vec<_>>()
        };

        let mut dst = codec::Document::new();
        paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        assert_eq!(
            shape_styles(&dst),
            vec![segment_style(&dst)],
            "the segment must point at the target's shape-file style"
        );

        // A second paste finds that style by its font file: no duplicate.
        paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        assert_eq!(shape_styles(&dst).len(), 1);
    }

    #[test]
    fn pasted_image_points_at_the_targets_definition() {
        use codec::objects::{ImageDefinition, ImageDefinitionReactor, ObjectType};
        let mut seed = codec::Document::new();
        let definition = seed.allocate_handle();
        let mut image_definition = ImageDefinition::with_dimensions("fx.bmp", 8, 8);
        image_definition.handle = definition;
        seed.objects
            .insert(definition, ObjectType::ImageDefinition(image_definition));
        let mut image =
            codec::entities::RasterImage::new("fx.bmp", codec::types::Vector3::ZERO, 8.0, 8.0);
        image.definition_handle = Some(definition);
        let source = seed
            .add_entity(codec::EntityType::RasterImage(image))
            .unwrap();
        let reactor = seed.allocate_handle();
        let mut image_reactor = ImageDefinitionReactor::new(source);
        image_reactor.handle = reactor;
        image_reactor.owner = definition;
        seed.objects
            .insert(reactor, ObjectType::ImageDefinitionReactor(image_reactor));
        if let Some(codec::EntityType::RasterImage(image)) = seed.get_entity_mut(source) {
            image.definition_reactor_handle = Some(reactor);
        }
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[source]);

        let pasted_image = |doc: &codec::Document, handle: Handle| match doc.get_entity(handle) {
            Some(codec::EntityType::RasterImage(image)) => image.clone(),
            other => panic!("expected pasted RasterImage, got {other:?}"),
        };
        let definitions = |doc: &codec::Document| {
            doc.objects
                .iter()
                .filter(
                    |(_, o)| matches!(o, ObjectType::ImageDefinition(d) if d.file_name == "fx.bmp"),
                )
                .map(|(h, _)| *h)
                .collect::<Vec<_>>()
        };

        let mut dst = codec::Document::new();
        let (handles, _) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        let image = pasted_image(&dst, handles[0]);
        assert_eq!(
            definitions(&dst),
            vec![image.definition_handle.expect("definition link")],
            "the image must point at the target's definition of its file"
        );
        assert_eq!(image.definition_reactor_handle, None);
        // Registered in, and owned by, the target's ACAD_IMAGE_DICT.
        let root = dst.header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(root)) = dst.objects.get(&root) else {
            panic!("no root dictionary");
        };
        let dictionary = root.get("ACAD_IMAGE_DICT").expect("ACAD_IMAGE_DICT");
        let Some(ObjectType::ImageDefinition(copied)) =
            dst.objects.get(&image.definition_handle.unwrap())
        else {
            panic!("definition missing");
        };
        assert_eq!(copied.owner, dictionary);

        // A second paste reuses the definition of the same file.
        paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        assert_eq!(definitions(&dst).len(), 1);

        // Within the source drawing the paste keeps the very same definition,
        // but not the source image's reactor.
        let (handles, _) = paste_entities_kernel(
            &mut seed,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        let image = pasted_image(&seed, handles[0]);
        assert_eq!(image.definition_handle, Some(definition));
        assert_eq!(
            image.definition_reactor_handle, None,
            "the pasted image must not share the source image's reactor"
        );
    }

    fn assert_pasted_image_reactor(doc: &codec::Document, handle: Handle) -> (Handle, Handle) {
        use codec::objects::ObjectType;
        let Some(codec::EntityType::RasterImage(image)) = doc.get_entity(handle) else {
            panic!("missing pasted image");
        };
        let definition = image.definition_handle.expect("image definition link");
        let reactor = image.definition_reactor_handle.expect("image reactor link");
        let Some(ObjectType::ImageDefinitionReactor(object)) = doc.objects.get(&reactor) else {
            panic!("missing image reactor");
        };
        assert_eq!(object.handle, reactor);
        assert_eq!(object.owner, handle);
        assert_eq!(object.image_handle, handle);
        let Some(ObjectType::ImageDefinition(def)) = doc.objects.get(&definition) else {
            panic!("missing image definition");
        };
        assert_eq!(def.reactors.iter().filter(|h| **h == reactor).count(), 1);
        let root = doc.header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(root)) = doc.objects.get(&root) else {
            panic!("missing named object dictionary");
        };
        let dictionary = root.get("ACAD_IMAGE_DICT").expect("image dictionary entry");
        let Some(ObjectType::Dictionary(entries)) = doc.objects.get(&dictionary) else {
            panic!("missing image dictionary");
        };
        assert_eq!(entries.owner, doc.header.named_objects_dict_handle);
        assert_eq!(entries.get("fx"), Some(definition));
        assert_eq!(def.owner, dictionary);
        assert_eq!(def.reactors.iter().filter(|h| **h == dictionary).count(), 1);
        (definition, reactor)
    }

    fn check_pasted_image_owns_reactor(cross_drawing: bool) {
        let mut app = crate::app::OpenCADStudio::new_for_test();
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(99));
        let source_tab = app.tabs.len() - 1;
        let source = app.tabs[source_tab]
            .scene
            .add_entity(codec::EntityType::RasterImage(
                codec::entities::RasterImage::new("fx.bmp", codec::types::Vector3::ZERO, 8.0, 8.0),
            ));
        let (source_definition, source_reactor) =
            assert_pasted_image_reactor(&app.tabs[source_tab].scene.document, source);
        let (entities, deps) =
            copy_to_clipboard_kernel(&app.tabs[source_tab].scene.document, &[source]);
        app.clipboard = entities;
        app.clipboard_deps = deps;
        let target = if cross_drawing {
            app.tabs
                .push(crate::app::document::DocumentTab::new_drawing(100));
            let target = app.tabs.len() - 1;
            // Handles are document-local; separate their numeric ranges so a
            // reused source reactor link cannot accidentally pass this test.
            while app.tabs[target].scene.document.next_handle() <= source_reactor.value() {
                app.tabs[target].scene.document.allocate_handle();
            }
            target
        } else {
            source_tab
        };
        app.active_tab = target;
        app.push_undo_snapshot(target, "PASTECLIP");
        let handles = app.finalize_paste(
            target,
            Some(crate::command::EntityTransform::Translate(
                glam::DVec3::new(50.0, 0.0, 0.0),
            )),
        );
        assert_eq!(handles.len(), 1);
        let pasted = handles[0];
        let (definition, reactor) =
            assert_pasted_image_reactor(&app.tabs[target].scene.document, pasted);
        assert_ne!(reactor, source_reactor);
        if !cross_drawing {
            assert_ne!(pasted, source);
            assert_eq!(definition, source_definition);
        }
        assert_eq!(
            assert_pasted_image_reactor(&app.tabs[source_tab].scene.document, source),
            (source_definition, source_reactor),
        );

        app.undo_active_tab();
        assert!(app.tabs[target].scene.document.get_entity(pasted).is_none());
        assert!(!app.tabs[target]
            .scene
            .document
            .objects
            .contains_key(&reactor));
        if cross_drawing {
            assert!(!app.tabs[target]
                .scene
                .document
                .objects
                .contains_key(&definition));
        } else {
            assert_eq!(
                assert_pasted_image_reactor(&app.tabs[target].scene.document, source),
                (source_definition, source_reactor),
            );
        }
        app.redo_active_tab();
        assert_eq!(
            assert_pasted_image_reactor(&app.tabs[target].scene.document, pasted),
            (definition, reactor),
        );
    }

    #[test]
    fn pasted_image_owns_reactor_in_same_drawing() {
        check_pasted_image_owns_reactor(false);
    }

    #[test]
    fn pasted_image_owns_reactor_in_another_drawing() {
        check_pasted_image_owns_reactor(true);
    }

    #[test]
    fn pasted_table_roundtrip_record_brings_its_table_content() {
        use codec::objects::{Dictionary, ObjectType, XRecord, XRecordEntry};
        let mut seed = codec::Document::new();
        let table = seed
            .add_entity(codec::EntityType::Table(Box::new(
                codec::entities::Table::new(codec::types::Vector3::ZERO, 2, 2),
            )))
            .unwrap();
        // TABLE -> extension dictionary -> roundtrip XRECORD -360-> TABLECONTENT,
        // the layout AutoCAD writes.
        let xdict = seed.allocate_handle();
        let record = seed.allocate_handle();
        let content = seed.allocate_handle();
        let mut dictionary = Dictionary::new();
        dictionary.handle = xdict;
        dictionary.owner = table;
        dictionary.add_entry("ACAD_XREC_ROUNDTRIP", record);
        let mut roundtrip = XRecord::new();
        roundtrip.handle = record;
        roundtrip.owner = xdict;
        roundtrip.reactors.push(xdict);
        roundtrip.entries.push(XRecordEntry::string(
            102,
            "ACAD_ROUNDTRIP_2008_TABLE_ENTITY",
        ));
        roundtrip.entries.push(XRecordEntry::handle(360, content));
        roundtrip.synchronize_object_references();
        let mut table_content = codec::entities::Table::new(codec::types::Vector3::ZERO, 2, 2);
        table_content.common.handle = content;
        table_content.common.owner_handle = record;
        seed.objects
            .insert(xdict, ObjectType::Dictionary(dictionary));
        seed.objects.insert(record, ObjectType::XRecord(roundtrip));
        seed.objects
            .insert(content, ObjectType::TableContent(table_content));
        if let Some(entity) = seed.get_entity_mut(table) {
            entity.common_mut().xdictionary_handle = Some(xdict);
        }
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[table]);

        let mut dst = codec::Document::new();
        let (handles, _) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        let new_xdict = dst
            .get_entity(handles[0])
            .and_then(|entity| entity.common().xdictionary_handle)
            .expect("pasted table keeps its extension dictionary");
        let Some(ObjectType::Dictionary(dictionary)) = dst.objects.get(&new_xdict) else {
            panic!("extension dictionary missing");
        };
        let new_record = dictionary
            .get("ACAD_XREC_ROUNDTRIP")
            .expect("roundtrip record");
        let Some(ObjectType::XRecord(roundtrip)) = dst.objects.get(&new_record) else {
            panic!("roundtrip record missing");
        };
        assert_eq!(roundtrip.reactors, vec![new_xdict]);
        let new_content = roundtrip
            .entries
            .iter()
            .find(|entry| entry.code == 360)
            .and_then(|entry| entry.value.as_handle())
            .expect("hard owner of the table content");
        assert_eq!(
            roundtrip
                .object_references
                .iter()
                .map(|reference| reference.handle)
                .collect::<Vec<_>>(),
            vec![new_content]
        );
        let Some(ObjectType::TableContent(table_content)) = dst.objects.get(&new_content) else {
            panic!("the record must point at a table content in the target");
        };
        assert_eq!(table_content.common.handle, new_content);
        assert_eq!(table_content.common.owner_handle, new_record);
    }

    #[test]
    fn pasted_mleader_style_is_copied_and_repointed() {
        check_pasted_mleader_style(false);
    }

    #[test]
    fn pasted_mleader_style_reuses_target_definition() {
        check_pasted_mleader_style(true);
    }

    fn check_pasted_mleader_style(existing: bool) {
        use codec::entities::{
            BlockAttribute, LeaderLine, MultiLeader, MultiLeaderArrowheadOverride,
        };
        use codec::objects::{MultiLeaderStyle, ObjectType};
        let mut seed = codec::Document::new();
        let mut text = codec::tables::TextStyle::new("T1");
        text.handle = seed.allocate_handle();
        let text_handle = text.handle;
        seed.text_styles.add(text).unwrap();
        let mut lt = codec::tables::LineType::new("L1");
        lt.handle = seed.allocate_handle();
        let line_handle = lt.handle;
        seed.line_types.add(lt).unwrap();
        let att = codec::entities::AttributeDefinition::new("TAG".into(), "".into(), "".into());
        define_block_raw_doc(
            &mut seed,
            "CONTENT",
            codec::types::Vector3::ZERO,
            &[codec::EntityType::AttributeDefinition(att)],
        );
        let block = seed.block_records.get("CONTENT").unwrap().handle;
        let attribute = crate::scene::block_authoring::members(&seed, block)
            .into_iter()
            .find(|&h| {
                matches!(
                    seed.get_entity(h),
                    Some(codec::EntityType::AttributeDefinition(_))
                )
            })
            .unwrap();
        let mut style = MultiLeaderStyle::new("S1");
        style.handle = seed.allocate_handle();
        style.text_style_handle = Some(text_handle);
        style.line_type_handle = Some(line_handle);
        style.arrowhead_handle = Some(block);
        style.block_content_handle = Some(block);
        let mut m = MultiLeader::new();
        m.style_handle = Some(style.handle);
        m.text_style_handle = Some(text_handle);
        m.context.text_style_handle = Some(text_handle);
        m.line_type_handle = Some(line_handle);
        m.arrowhead_handle = Some(block);
        m.block_content_handle = Some(block);
        m.context.block_content_handle = Some(block);
        let mut line = LeaderLine::new(0);
        line.line_type_handle = Some(line_handle);
        line.arrowhead_handle = Some(block);
        m.add_leader_root().add_line(line);
        m.arrowhead_overrides.push(MultiLeaderArrowheadOverride {
            arrowhead_handle: Some(block),
            ..Default::default()
        });
        let mut att = BlockAttribute::new("value");
        att.attribute_definition_handle = Some(attribute);
        m.block_attributes.push(att);
        let mut unresolved = BlockAttribute::new("missing");
        unresolved.attribute_definition_handle = Some(Handle::new(0xFFFFFF));
        m.block_attributes.push(unresolved);
        seed.objects
            .insert(style.handle, ObjectType::MultiLeaderStyle(style));
        let source = seed
            .add_entity(codec::EntityType::MultiLeader(Box::new(m)))
            .unwrap();
        let nested = seed.get_entity(source).unwrap().clone();
        define_block_raw_doc(&mut seed, "OUTER", codec::types::Vector3::ZERO, &[nested]);
        let insert = seed
            .add_entity(codec::EntityType::Insert(codec::entities::Insert::new(
                "OUTER",
                codec::types::Vector3::ZERO,
            )))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[source, insert]);
        let mut dst = codec::Document::new();
        for _ in 0..50 {
            dst.allocate_handle();
        }
        let mut target_style = MultiLeaderStyle::new("s1");
        target_style.handle = dst.allocate_handle();
        target_style.text_height = 123.0;
        let target_handle = target_style.handle;
        if existing {
            dst.objects
                .insert(target_handle, ObjectType::MultiLeaderStyle(target_style));
        }
        let (handles, _) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        let styles: Vec<_> = dst
            .objects
            .values()
            .filter_map(|o| match o {
                ObjectType::MultiLeaderStyle(s) if s.name.eq_ignore_ascii_case("S1") => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(styles.len(), 1);
        let style = styles[0];
        let text = Some(dst.text_styles.get("T1").unwrap().handle);
        let line = Some(dst.line_types.get("L1").unwrap().handle);
        let block = Some(dst.block_records.get("CONTENT").unwrap().handle);
        if existing {
            assert_eq!(style.handle, target_handle);
            assert_eq!(style.text_height, 123.0);
            assert_eq!(style.text_style_handle, None);
        } else {
            assert_ne!(style.handle, deps.mleader_styles[0].handle);
            assert_eq!(
                [
                    style.text_style_handle,
                    style.line_type_handle,
                    style.arrowhead_handle,
                    style.block_content_handle
                ],
                [text, line, block, block]
            );
        }
        let Some(ObjectType::Dictionary(dict)) = dst.objects.get(&style.owner_handle) else {
            panic!("style dictionary missing")
        };
        assert_eq!(dict.get(&style.name), Some(style.handle));
        let root = dst.objects.values().any(|o| {
            matches!(o,
            ObjectType::Dictionary(d) if d.get("ACAD_MLEADERSTYLE") == Some(style.owner_handle))
        });
        assert!(root, "style dictionary must be registered in the target");
        let Some(codec::EntityType::MultiLeader(m)) = dst.get_entity(handles[0]) else {
            panic!("MLEADER missing")
        };
        assert_eq!(m.style_handle, Some(style.handle));
        assert_eq!(
            [
                m.text_style_handle,
                m.context.text_style_handle,
                m.line_type_handle,
                m.arrowhead_handle,
                m.block_content_handle,
                m.context.block_content_handle
            ],
            [text, text, line, block, block, block]
        );
        assert_eq!(m.context.leader_roots[0].lines[0].line_type_handle, line);
        assert_eq!(m.context.leader_roots[0].lines[0].arrowhead_handle, block);
        assert_eq!(m.arrowhead_overrides[0].arrowhead_handle, block);
        let attribute = m.block_attributes[0].attribute_definition_handle.unwrap();
        assert!(
            matches!(dst.get_entity(attribute), Some(codec::EntityType::AttributeDefinition(a)) if a.tag == "TAG" && a.common.owner_handle == block.unwrap())
        );
        assert_eq!(m.block_attributes[1].attribute_definition_handle, None);
        let record = dst.block_records.get("OUTER").unwrap().handle;
        let nested = crate::scene::block_authoring::members(&dst, record)
            .into_iter()
            .find_map(|h| match dst.get_entity(h) {
                Some(codec::EntityType::MultiLeader(m)) => Some(m),
                _ => None,
            })
            .unwrap();
        let mut nested = nested.clone();
        let mut pasted = m.clone();
        assert_eq!(mleader_handles(&mut nested), mleader_handles(&mut pasted));
    }

    #[test]
    fn pasted_dim_style_points_at_the_targets_records() {
        use codec::TableEntry;
        let mut seed = codec::Document::new();
        let mut text_style = codec::tables::TextStyle::new("DIMTXT");
        text_style.set_handle(seed.allocate_handle());
        let text_style = {
            let h = text_style.handle();
            seed.text_styles.add(text_style).unwrap();
            h
        };
        let mut line_type = codec::tables::LineType::new("DASHED");
        line_type.set_handle(seed.allocate_handle());
        let line_type = {
            let h = line_type.handle();
            seed.line_types.add(line_type).unwrap();
            h
        };
        let tick = codec::EntityType::Line(codec::entities::Line::new());
        define_block_raw_doc(
            &mut seed,
            "ARW",
            codec::types::Vector3::ZERO,
            std::slice::from_ref(&tick),
        );
        let mut style = codec::tables::DimStyle::new("CUSTOM");
        style.set_handle(seed.allocate_handle());
        style.dimtxsty_handle = text_style;
        style.dimltex1_handle = line_type;
        style.dimblk = seed.block_records.get("ARW").unwrap().handle;
        seed.dim_styles.add(style).unwrap();
        let mut dim = codec::entities::DimensionLinear::new(
            codec::types::Vector3::ZERO,
            codec::types::Vector3::new(10.0, 0.0, 0.0),
        );
        dim.base.style_name = "CUSTOM".to_string();
        let dim = seed
            .add_entity(codec::EntityType::Dimension(Box::new(
                codec::entities::Dimension::Linear(dim),
            )))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[dim]);

        // The target's handles run differently from the source's.
        let mut dst = codec::Document::new();
        for _ in 0..50 {
            dst.allocate_handle();
        }
        paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );

        let style = dst.dim_styles.get("CUSTOM").expect("pasted dim style");
        assert_eq!(
            style.dimtxsty_handle,
            dst.text_styles
                .get("DIMTXT")
                .expect("pasted text style")
                .handle()
        );
        assert_eq!(
            style.dimltex1_handle,
            dst.line_types
                .get("DASHED")
                .expect("pasted linetype")
                .handle()
        );
        assert_eq!(
            style.dimblk,
            dst.block_records
                .get("ARW")
                .expect("pasted arrow block")
                .handle
        );
        assert!(style.dimblk1.is_null() && style.dimltex2_handle.is_null());
    }

    #[test]
    fn pasted_table_block_cell_points_at_the_targets_block() {
        use codec::objects::ObjectType;
        let circle = codec::EntityType::Circle(codec::entities::Circle::new());
        let mut seed = codec::Document::new();
        define_block_raw_doc(
            &mut seed,
            "CELLBLK",
            codec::types::Vector3::ZERO,
            std::slice::from_ref(&circle),
        );
        let block = seed.block_records.get("CELLBLK").unwrap().handle;
        let mut table = codec::entities::Table::new(codec::types::Vector3::ZERO, 2, 2);
        table.rows[0].cells[0].contents = vec![codec::entities::CellContent::block(block)];
        let table = seed
            .add_entity(codec::EntityType::Table(Box::new(table)))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[table]);

        // The target already uses the source block's handle for something else.
        let mut dst = codec::Document::new();
        define_block_raw_doc(&mut dst, "OTHER", codec::types::Vector3::ZERO, &[circle]);
        let (handles, _) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        let target = dst
            .block_records
            .get("CELLBLK")
            .expect("the cell's block must be copied")
            .handle;
        let Some(codec::EntityType::Table(pasted)) = dst.get_entity(handles[0]) else {
            panic!("pasted table missing");
        };
        assert_eq!(
            table_cell_blocks(pasted).collect::<Vec<_>>(),
            vec![target],
            "the cell must point at the target's copy of its block"
        );

        // The TABLECONTENT copy of the cells follows the same map.
        let mut content = codec::entities::Table::new(codec::types::Vector3::ZERO, 2, 2);
        content.rows[0].cells[0].contents = vec![codec::entities::CellContent::block(block)];
        let mut object = ObjectType::TableContent(content);
        let remap = std::collections::HashMap::from([(block, target)]);
        remap_object(&mut object, Handle::new(0x500), &remap);
        let ObjectType::TableContent(content) = &object else {
            unreachable!();
        };
        assert_eq!(table_cell_blocks(content).collect::<Vec<_>>(), vec![target]);
    }

    #[test]
    fn pasted_block_repoints_its_tables_and_images() {
        use codec::objects::{ImageDefinition, ObjectType};
        let circle = codec::EntityType::Circle(codec::entities::Circle::new());
        let mut seed = codec::Document::new();
        define_block_raw_doc(
            &mut seed,
            "CELLBLK",
            codec::types::Vector3::ZERO,
            std::slice::from_ref(&circle),
        );
        let cell_block = seed.block_records.get("CELLBLK").unwrap().handle;
        let definition = seed.allocate_handle();
        let mut image_definition = ImageDefinition::with_dimensions("fx.bmp", 8, 8);
        image_definition.handle = definition;
        seed.objects
            .insert(definition, ObjectType::ImageDefinition(image_definition));
        // Block OUTER holds a table showing CELLBLK and an image of fx.bmp;
        // only an INSERT of OUTER is copied.
        let mut table = codec::entities::Table::new(codec::types::Vector3::ZERO, 2, 2);
        table.rows[0].cells[0].contents = vec![codec::entities::CellContent::block(cell_block)];
        let mut image =
            codec::entities::RasterImage::new("fx.bmp", codec::types::Vector3::ZERO, 8.0, 8.0);
        image.definition_handle = Some(definition);
        define_block_raw_doc(
            &mut seed,
            "OUTER",
            codec::types::Vector3::ZERO,
            &[
                codec::EntityType::Table(Box::new(table)),
                codec::EntityType::RasterImage(image),
            ],
        );
        let insert = seed
            .add_entity(codec::EntityType::Insert(codec::entities::Insert::new(
                "OUTER",
                codec::types::Vector3::ZERO,
            )))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[insert]);

        // The target's handles run ahead of the source's, so a kept source
        // handle names something else (or nothing) there.
        let mut dst = codec::Document::new();
        for _ in 0..64 {
            dst.allocate_handle();
        }
        define_block_raw_doc(&mut dst, "OTHER", codec::types::Vector3::ZERO, &[circle]);
        paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );
        let target_cell = dst
            .block_records
            .get("CELLBLK")
            .expect("the cell's block must be copied with the block")
            .handle;
        assert_ne!(
            target_cell, cell_block,
            "handles must differ for the test to bite"
        );
        let outer = dst.block_records.get("OUTER").expect("OUTER copied").handle;
        let members = crate::scene::block_authoring::members(&dst, outer);
        let Some(codec::EntityType::Table(pasted)) =
            members.iter().find_map(|&h| match dst.get_entity(h) {
                Some(e @ codec::EntityType::Table(_)) => Some(e),
                _ => None,
            })
        else {
            panic!("table missing from the pasted block");
        };
        assert_eq!(
            table_cell_blocks(pasted).collect::<Vec<_>>(),
            vec![target_cell],
            "the table in the block must point at the target's copy of its cell block"
        );
        let Some(codec::EntityType::RasterImage(pasted)) =
            members.iter().find_map(|&h| match dst.get_entity(h) {
                Some(e @ codec::EntityType::RasterImage(_)) => Some(e),
                _ => None,
            })
        else {
            panic!("image missing from the pasted block");
        };
        let target_definition = pasted.definition_handle.expect("definition link");
        assert!(
            matches!(
                dst.objects.get(&target_definition),
                Some(ObjectType::ImageDefinition(d)) if d.file_name == "fx.bmp"
            ),
            "the image in the block must point at the target's definition of its file"
        );
    }

    #[test]
    fn remaps_leader_annotation_link_to_pasted_handles() {
        let mut seed = codec::Document::new();
        let note = seed
            .add_entity(codec::EntityType::MText(Box::new(codec::entities::MText::new())))
            .unwrap();
        let mut leader = codec::entities::Leader::default();
        leader.annotation_handle = note;
        let leader_handle = seed.add_entity(codec::EntityType::Leader(leader)).unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[leader_handle, note]);

        let mut dst = codec::Document::new();
        let (handles, _) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );

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
        let mut seed = codec::Document::new();
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
        xrec.entries
            .push(XRecordEntry::new(340, XRecordValue::Handle(seed_entity)));
        seed.objects.insert(src_root, ObjectType::Dictionary(dict));
        seed.objects.insert(src_leaf, ObjectType::XRecord(xrec));
        if let Some(e) = seed.get_entity_mut(seed_entity) {
            e.common_mut().xdictionary_handle = Some(src_root);
        }
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[seed_entity]);
        assert_eq!(deps.ext_objects.len(), 1, "capture must snapshot the xdict");

        let mut dst = codec::Document::new();
        let before: Vec<Handle> = dst.objects.keys().copied().collect();
        let (handles, _) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );

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
    // `codec::Document`, entities, and the shared kernels
    // (`copy_to_clipboard_kernel`, `paste_entities_kernel`,
    // `match_layer_kernel`, `match_properties_kernel` + `MatchOpts`).
    use super::{copy_to_clipboard_kernel, paste_entities_kernel};
    use crate::entities::match_props::{match_layer_kernel, match_properties_kernel, MatchOpts};

    #[test]
    fn headless_copy_paste_then_match_kernels_share_one_path() {
        // Phase 1: copy in doc A, paste into doc B with a translate.
        let mut doc_a = codec::Document::new();
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

        let mut doc_b = codec::Document::new();
        let translate = crate::command::EntityTransform::Translate(glam::DVec3::new(5.0, 7.0, 0.0));
        let (handles, handle_map) = paste_entities_kernel(
            &mut doc_b,
            &entities,
            &deps,
            Some(&translate),
            crate::app::PasteSpace::Model,
        );

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
        let src = doc_b.add_entity(codec::EntityType::Line(src_line)).unwrap();
        let mut dst_line = codec::entities::Line::new();
        dst_line.common.layer = "0".to_string();
        dst_line.common.linetype_scale = 1.0;
        dst_line.thickness = 0.0;
        let dst = doc_b.add_entity(codec::EntityType::Line(dst_line)).unwrap();

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
    #[test]
    fn paste_owner_override_routes_entity_into_block_record() {
        let mut source = codec::Document::new();

        let line = codec::entities::Line::from_points(
            codec::types::Vector3::new(0.0, 0.0, 0.0),
            codec::types::Vector3::new(10.0, 0.0, 0.0),
        );
        let source_handle = source
            .add_entity(codec::EntityType::Line(line))
            .expect("source line");

        let (entities, deps) = copy_to_clipboard_kernel(&source, &[source_handle]);

        let mut target = codec::Document::new();

        let block_handle = target.allocate_handle();
        let mut block = codec::tables::BlockRecord::new("BEDIT_TARGET");
        block.handle = block_handle;
        target
            .block_records
            .add(block)
            .expect("target block record");

        let (handles, _) = paste_entities_kernel(
            &mut target,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Block(block_handle),
        );

        assert_eq!(handles.len(), 1);
        let pasted = handles[0];
        assert!(!pasted.is_null());

        let entity = target.get_entity(pasted).expect("pasted entity must exist");

        assert_eq!(
            entity.common().owner_handle,
            block_handle,
            "paste inside BEDIT must belong to the edited block record"
        );

        let record = target
            .block_records
            .get("BEDIT_TARGET")
            .expect("block record must still exist");

        assert!(
            record.entity_handles.contains(&pasted),
            "pasted entity must be registered as a member of the edited block"
        );
    }
}

#[cfg(test)]
mod define_transformed_block_doc_xdict_tests {
    use super::define_transformed_block_doc;

    /// No extension dictionary may be referenced by more than one entity.
    fn assert_no_shared_xdicts(doc: &codec::Document) {
        use std::collections::HashMap;
        let mut owners: HashMap<codec::Handle, usize> = HashMap::new();
        for entity in doc.entities() {
            if let Some(xd) = entity.common().xdictionary_handle {
                if !xd.is_null() {
                    *owners.entry(xd).or_default() += 1;
                }
            }
        }
        let shared: Vec<_> = owners
            .iter()
            .filter(|(_, &n)| n > 1)
            .map(|(h, n)| format!("{h:?} x{n}"))
            .collect();
        assert!(
            shared.is_empty(),
            "shared extension dictionaries: {}",
            shared.join(", ")
        );
    }

    #[test]
    fn clone_block_is_flagged_anonymous() {
        let mut doc = codec::Document::new();
        let line = doc
            .add_entity(codec::EntityType::Line(codec::entities::Line::new()))
            .unwrap();
        let subs = vec![doc.get_entity(line).unwrap().clone()];
        let name = define_transformed_block_doc(
            &mut doc,
            &subs,
            &crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
        )
        .expect("clone block must be created");
        assert!(doc.block_records.get(&name).unwrap().flags.anonymous);
    }

    /// Pasted/derived sub-entities must not share the source's extension
    /// dictionary. `define_transformed_block_doc` is the verbatim
    /// document-level port of `Scene::define_transformed_block`, and its
    /// clone helper likewise preserves `xdictionary_handle`; across drawings
    /// that handle would dangle into the source document.
    #[test]
    fn cloned_subs_do_not_share_source_xdict() {
        let mut doc = codec::Document::new();

        let dict_handle = doc.allocate_handle();
        let mut dict = codec::objects::Dictionary::new();
        dict.handle = dict_handle;
        doc.objects
            .insert(dict_handle, codec::objects::ObjectType::Dictionary(dict));

        let mut line = codec::entities::Line::new();
        line.common.xdictionary_handle = Some(dict_handle);
        let src_handle = doc.add_entity(codec::EntityType::Line(line)).unwrap();

        let subs = vec![doc.get_entity(src_handle).unwrap().clone()];
        let name = define_transformed_block_doc(
            &mut doc,
            &subs,
            &crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
        )
        .expect("clone block must be created");
        assert!(
            name.starts_with("*D"),
            "anonymous block expected, got {name}"
        );

        assert_no_shared_xdicts(&doc);
        // Source keeps its dictionary.
        match doc.get_entity(src_handle) {
            Some(e) => assert_eq!(
                e.common().xdictionary_handle,
                Some(dict_handle),
                "the source must keep its own dictionary"
            ),
            None => panic!("source entity missing"),
        }
        // Clones carry none.
        let owned = doc.block_records.get(&name).unwrap().entity_handles.clone();
        assert!(!owned.is_empty(), "clone block should own sub-entities");
        for h in &owned {
            match doc.get_entity(*h) {
                Some(e) => assert!(
                    e.common().xdictionary_handle.is_none(),
                    "cloned child {h:?} must not carry an extension dictionary"
                ),
                None => panic!("cloned child {h:?} missing"),
            }
        }
    }
}

#[cfg(test)]
mod define_block_raw_doc_anonymous_tests {
    use super::define_block_raw_doc;

    fn defined(name: &str) -> codec::Document {
        let mut doc = codec::Document::new();
        define_block_raw_doc(&mut doc, name, codec::types::Vector3::ZERO, &[]);
        doc
    }

    /// Pasting an INSERT of an anonymous / dynamic-variant block recreates it
    /// under its `*U<n>` name; the DWG writer takes the anonymous bit only
    /// from the flag, so the record must carry it.
    #[test]
    fn star_named_block_is_flagged_anonymous() {
        let doc = defined("*U7");
        let record = doc.block_records.get("*U7").expect("block defined");
        assert!(record.flags.anonymous, "*U7 must be flagged anonymous");
    }

    #[test]
    fn plain_named_block_is_not_flagged_anonymous() {
        let doc = defined("Door");
        let record = doc.block_records.get("Door").expect("block defined");
        assert!(!record.flags.anonymous);
    }

    #[test]
    fn layout_block_is_not_flagged_anonymous() {
        let doc = defined("*Paper_Space0");
        let record = doc
            .block_records
            .get("*Paper_Space0")
            .expect("block defined");
        assert!(!record.flags.anonymous, "a layout block is not anonymous");
    }

    #[test]
    fn scene_define_block_raw_flags_star_name_anonymous() {
        let mut scene = crate::scene::Scene::new();
        scene.define_block_raw("*U9", codec::types::Vector3::ZERO, vec![]);
        let record = scene.document.block_records.get("*U9").expect("defined");
        assert!(record.flags.anonymous);
    }
}

#[cfg(test)]
mod paste_viewport_clip_tests {
    use super::{copy_to_clipboard_kernel, paste_entities_kernel};
    use codec::Handle;

    fn clipped_viewport(doc: &mut codec::Document) -> (Handle, Handle) {
        let mut boundary = codec::entities::LwPolyline::new();
        boundary.add_point(codec::types::Vector2::new(0.0, 0.0));
        boundary.add_point(codec::types::Vector2::new(10.0, 0.0));
        boundary.add_point(codec::types::Vector2::new(5.0, 8.0));
        boundary.is_closed = true;
        let boundary = doc
            .add_entity(codec::EntityType::LwPolyline(boundary))
            .unwrap();
        let mut viewport = codec::entities::Viewport::new();
        viewport.clip_boundary_handle = boundary;
        viewport.status.non_rectangular_clipping = true;
        let viewport = doc
            .add_entity(codec::EntityType::Viewport(Box::new(viewport)))
            .unwrap();
        (viewport, boundary)
    }

    fn paste(seed: &codec::Document, handles: &[Handle]) -> (codec::Document, Vec<Handle>) {
        let (entities, deps) = copy_to_clipboard_kernel(seed, handles);
        let mut dst = codec::Document::new();
        let translate =
            crate::command::EntityTransform::Translate(glam::DVec3::new(100.0, 0.0, 0.0));
        let (pasted, _) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            Some(&translate),
            crate::app::PasteSpace::Model,
        );
        (dst, pasted)
    }

    fn viewport_in(doc: &codec::Document, handles: &[Handle]) -> codec::entities::Viewport {
        handles
            .iter()
            .find_map(|&h| match doc.get_entity(h) {
                Some(codec::EntityType::Viewport(v)) => Some(v.as_ref().clone()),
                _ => None,
            })
            .expect("pasted viewport")
    }

    #[test]
    fn pasted_viewport_is_clipped_by_the_pasted_boundary() {
        let mut seed = codec::Document::new();
        let (viewport, boundary) = clipped_viewport(&mut seed);
        let (dst, pasted) = paste(&seed, &[viewport, boundary]);
        let copy = viewport_in(&dst, &pasted);
        let pasted_boundary = pasted
            .iter()
            .copied()
            .find(|&h| matches!(dst.get_entity(h), Some(codec::EntityType::LwPolyline(_))))
            .expect("pasted boundary");
        assert_eq!(copy.clip_boundary_handle, pasted_boundary);
        assert!(copy.status.non_rectangular_clipping);
    }

    #[test]
    fn pasted_viewport_without_its_boundary_is_not_clipped() {
        let mut seed = codec::Document::new();
        let (viewport, _) = clipped_viewport(&mut seed);
        let (dst, pasted) = paste(&seed, &[viewport]);
        let copy = viewport_in(&dst, &pasted);
        assert!(copy.clip_boundary_handle.is_null());
        assert!(!copy.status.non_rectangular_clipping);
    }
}
