use super::*;
pub(crate) use crate::entities::doc_edit::define_block_raw_doc;
pub(crate) use crate::scene::clipboard::kernel::*;

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
        base_point: Option<glam::DVec3>,
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
        // A named base point wins; the bounding-box corner is only a fallback.
        let source_point = base_point.unwrap_or_else(|| {
            crate::app::helpers::entities_lower_left_by_bbox(
                &self.tabs[i].scene.document,
                &handles,
            )
        });
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
        replacements: Vec<(Handle, Vec<document::EntityType>)>,
        additions: Vec<document::EntityType>,
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
        let table_cell = self.tabs[i]
            .active_cmd
            .as_ref()
            .and_then(|c| c.table_cell());
        self.push_undo_snapshot(i, label);
        for (handle, entities) in replacements {
            self.replace_command_entity(i, handle, entities);
        }
        // A formula typed on the command line is stored as a formula field,
        // as the cell editor does.
        if let Some((table, row, column)) = table_cell {
            let document = &mut self.tabs[i].scene.document;
            if crate::entities::table::typed_formula_to_field(document, table, row, column) {
                if let Some(document::EntityType::Table(target)) = document.get_entity_mut(table) {
                    target.block_record_handle = None;
                }
                self.tabs[i]
                    .scene
                    .bump_entities(&[(table, crate::scene::ChangeKind::Modified)]);
            }
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
        replacements: Vec<(Handle, Vec<document::EntityType>)>,
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
        new_entities: Vec<document::EntityType>,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Some(Task::none());
        }
        // Detect SPLINEDIT sentinel: a single XLine with a magic layer name.
        if new_entities.len() == 1 {
            if let document::EntityType::XLine(ref xl) = new_entities[0] {
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
        let hatch_detached = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|cmd| cmd.name() == "TRIM")
            && matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(document::EntityType::Hatch(hatch)) if hatch.is_associative
            )
            && new_entities.iter().any(|entity| {
                matches!(
                    entity, document::EntityType::Hatch(hatch) if !hatch.is_associative
                )
            });
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
            let mut new_blocks: Vec<Handle> = Vec::new();
            let mut changes: Vec<(Handle, crate::scene::ChangeKind)> = Vec::new();
            for record in self.tabs[i].scene.document.block_records.iter() {
                if record.handle.value() >= first_new_handle {
                    new_blocks.push(record.handle);
                    for &member in &record.entity_handles {
                        changes.push((member, crate::scene::ChangeKind::Added));
                    }
                }
            }
            if !new_blocks.is_empty() || !changes.is_empty() {
                self.tabs[i]
                    .scene
                    .bump_entities_restyled_with_block_deps(&changes, &new_blocks, &[]);
            }
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
        let block_records_before = self.tabs[i].scene.document.block_records.len();
        let (by_index, handle_map) = paste_entities_kernel(
            &mut self.tabs[i].scene.document,
            &clipboard,
            &deps,
            translate.as_ref(),
            space,
        );
        finish_paste_in_scene(
            &mut self.tabs[i].scene,
            &deps,
            &by_index,
            &handle_map,
            block_records_before,
            first_new_handle,
        );
        // Pasted back into its own drawing, a copied associative array gets
        // its own blocks and action, as COPY gives it; elsewhere the source
        // handles mean nothing and it stays a plain reference.
        if self.clipboard_source_tab == Some(self.tabs[i].id) {
            let arrays: Vec<(Handle, Handle)> = handle_map
                .iter()
                .filter(|(source, _)| {
                    crate::scene::assoc_array::array_of_insert(
                        &self.tabs[i].scene.document,
                        **source,
                    )
                    .is_some()
                })
                .map(|(&source, &copy)| (source, copy))
                .collect();
            for (source, copy) in arrays {
                if crate::scene::assoc_array::clone_array_for_copy(
                    &mut self.tabs[i].scene,
                    source,
                    copy,
                    &handle_map,
                ) {
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(copy, crate::scene::ChangeKind::Modified)]);
                }
            }
        }
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
            if cap.attribute_index.is_some() {
                continue;
            }
            if let Some(new_root) = recreate_ext_subtree(doc, cap, None, &records) {
                out.insert(cap.entity_index, new_root);
            }
        }
        out
    }
}

#[cfg(test)]
mod clipboard_xdict_clone_tests {
    use super::recreate_ext_subtree;
    use super::remap_object;
    use document::Handle;
    // Same re-export path `commands/blocks.rs:356` uses — this import proves
    // the `mod.rs` re-export still covers external callers.
    use super::super::remap_ext_subtree_reference;
    use document::objects::{ObjectType, XRecordValue};

    /// A dictionary root pointing at one XRecord leaf that references the
    /// source entity — the minimal shape of a captured XCLIP filter graph.
    fn xdict_capture_fixture() -> crate::app::ClipExtObjects {
        let mut src = document::Document::new();
        let src_entity = src.allocate_handle();
        let src_root = src.allocate_handle();
        let src_leaf = src.allocate_handle();

        let mut dict = document::objects::Dictionary::new();
        dict.handle = src_root;
        dict.owner = src_entity;
        dict.add_entry("XCLIP", src_leaf);

        let mut xrec = document::objects::XRecord::new();
        xrec.handle = src_leaf;
        xrec.owner = src_root;
        xrec.entries.push(document::objects::XRecordEntry::new(
            340,
            XRecordValue::Handle(src_entity),
        ));

        crate::app::ClipExtObjects {
            entity_index: 0,
            attribute_index: None,
            src_entity_handle: src_entity,
            root: src_root,
            objects: vec![
                (src_root, ObjectType::Dictionary(dict)),
                (src_leaf, ObjectType::XRecord(xrec)),
            ],
            annotation_scales: Vec::new(),
        }
    }

    fn leaf_of(doc: &document::Document, root: Handle) -> Handle {
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
        let mut dst = document::Document::new();
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

    #[test]
    fn a_pasted_block_definition_ends_after_its_body() {
        let mut doc = document::Document::new();
        let line = document::entities::Line::from_points(
            document::types::Vector3::ZERO,
            document::types::Vector3::new(1.0, 0.0, 0.0),
        );
        super::define_block_raw_doc(
            &mut doc,
            "PASTED_IN",
            document::types::Vector3::ZERO,
            &[document::EntityType::Line(line)],
        );
        let record = doc
            .block_records
            .get("PASTED_IN")
            .expect("the record is added");
        let marker = record.block_entity_handle.value();
        let end = record.block_end_handle.value();
        let body: Vec<_> = record
            .entity_handles
            .iter()
            .map(|handle| handle.value())
            .collect();
        assert!(
            body.iter().all(|handle| *handle > marker),
            "body handles follow the BLOCK marker: {body:?}"
        );
        assert!(
            end > *body.iter().max().expect("the body is not empty"),
            "the ENDBLK handle follows the body: marker {marker}, body {body:?}, end {end}"
        );
    }

    /// PASTEBLOCK-style two-phase clone: recreate without a host entity, then
    /// re-point the subtree at the final block-owned handle.
    #[test]
    fn cross_drawing_clone_two_phase_remap_resolves_reference() {
        let cap = xdict_capture_fixture();
        let mut dst = document::Document::new();
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
        let mut scratch = document::Document::new();
        let old_self = scratch.allocate_handle();
        let old_owner = scratch.allocate_handle();
        let old_child = scratch.allocate_handle();
        let new_self = scratch.allocate_handle();
        let new_owner = scratch.allocate_handle();
        let new_child = scratch.allocate_handle();

        let mut dict = document::objects::Dictionary::new();
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

    fn line_on(layer: &str) -> document::EntityType {
        let mut line = document::entities::Line::new();
        line.common.layer = layer.to_string();
        document::EntityType::Line(line)
    }

    #[test]
    fn clones_entities_and_captures_layer_deps() {
        let mut doc = document::Document::new();
        doc.layers
            .add(document::tables::Layer::new("WALLS"))
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
        let mut doc = document::Document::new();
        let kept = doc.add_entity(line_on("0")).unwrap();
        let missing = doc.allocate_handle();

        let (entities, _) = copy_to_clipboard_kernel(&doc, &[kept, missing]);

        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].common().handle, kept);
    }

    #[test]
    fn empty_selection_yields_empty_payload() {
        let doc = document::Document::new();

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
    use document::Handle;

    fn line_on(layer: &str) -> document::EntityType {
        let mut line = document::entities::Line::new();
        line.common.layer = layer.to_string();
        document::EntityType::Line(line)
    }

    #[test]
    fn pastes_with_fresh_handles_and_translate() {
        let mut seed = document::Document::new();
        seed.layers
            .add(document::tables::Layer::new("WALLS"))
            .expect("add layer");
        // Position the seed line away from the origin so the translate is visible.
        let mut seed_line = document::entities::Line::from_points(
            document::types::Vector3::new(0.0, 0.0, 0.0),
            document::types::Vector3::new(10.0, 0.0, 0.0),
        );
        seed_line.common.layer = "WALLS".to_string();
        let seed_handle = seed
            .add_entity(document::EntityType::Line(seed_line))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[seed_handle]);

        let mut dst = document::Document::new();
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
            Some(document::EntityType::Line(l)) => l,
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
        use document::tables::linetype::{
            LineTypeComplexContent, LineTypeComplexData, LineTypeElement,
        };
        // A shape-file style has no name; a linetype segment names it by handle.
        let mut seed = document::Document::new();
        let mut shapes = document::tables::TextStyle::new("");
        shapes.handle = seed.allocate_handle();
        shapes.is_shape_file = true;
        shapes.font_file = "ltypeshp.shx".to_string();
        let shapes_handle = shapes.handle;
        seed.text_styles.add_allow_duplicate(shapes);
        let mut batting = document::tables::LineType::new("BATTING");
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
        let mut line = document::entities::Line::new();
        line.common.linetype = "BATTING".to_string();
        let source = seed.add_entity(document::EntityType::Line(line)).unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[source]);

        let segment_style = |doc: &document::Document| {
            doc.line_types
                .get("BATTING")
                .expect("pasted linetype")
                .elements
                .iter()
                .find_map(|element| element.complex.as_ref())
                .expect("shape segment")
                .style_handle
        };
        let shape_styles = |doc: &document::Document| {
            doc.text_styles
                .iter()
                .filter(|s| s.is_shape_file && s.font_file == "ltypeshp.shx")
                .map(|s| s.handle)
                .collect::<Vec<_>>()
        };

        let mut dst = document::Document::new();
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
        use document::objects::{ImageDefinition, ImageDefinitionReactor, ObjectType};
        let mut seed = document::Document::new();
        let definition = seed.allocate_handle();
        let mut image_definition = ImageDefinition::with_dimensions("fx.bmp", 8, 8);
        image_definition.handle = definition;
        seed.objects
            .insert(definition, ObjectType::ImageDefinition(image_definition));
        let mut image = document::entities::RasterImage::new(
            "fx.bmp",
            document::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        image.definition_handle = Some(definition);
        let source = seed
            .add_entity(document::EntityType::RasterImage(image))
            .unwrap();
        let reactor = seed.allocate_handle();
        let mut image_reactor = ImageDefinitionReactor::new(source);
        image_reactor.handle = reactor;
        image_reactor.owner = definition;
        seed.objects
            .insert(reactor, ObjectType::ImageDefinitionReactor(image_reactor));
        if let Some(document::EntityType::RasterImage(image)) = seed.get_entity_mut(source) {
            image.definition_reactor_handle = Some(reactor);
        }
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[source]);

        let pasted_image = |doc: &document::Document, handle: Handle| match doc.get_entity(handle) {
            Some(document::EntityType::RasterImage(image)) => image.clone(),
            other => panic!("expected pasted RasterImage, got {other:?}"),
        };
        let definitions = |doc: &document::Document| {
            doc.objects
                .iter()
                .filter(
                    |(_, o)| matches!(o, ObjectType::ImageDefinition(d) if d.file_name == "fx.bmp"),
                )
                .map(|(h, _)| *h)
                .collect::<Vec<_>>()
        };

        let mut dst = document::Document::new();
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

    fn assert_pasted_image_reactor(doc: &document::Document, handle: Handle) -> (Handle, Handle) {
        use document::objects::ObjectType;
        let Some(document::EntityType::RasterImage(image)) = doc.get_entity(handle) else {
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
            .push(crate::app::tab_document::DocumentTab::new_drawing(99));
        let source_tab = app.tabs.len() - 1;
        let source = app.tabs[source_tab]
            .scene
            .add_entity(document::EntityType::RasterImage(
                document::entities::RasterImage::new(
                    "fx.bmp",
                    document::types::Vector3::ZERO,
                    8.0,
                    8.0,
                ),
            ));
        let (source_definition, source_reactor) =
            assert_pasted_image_reactor(&app.tabs[source_tab].scene.document, source);
        let (entities, deps) =
            copy_to_clipboard_kernel(&app.tabs[source_tab].scene.document, &[source]);
        app.clipboard = entities;
        app.clipboard_deps = deps;
        let target = if cross_drawing {
            app.tabs
                .push(crate::app::tab_document::DocumentTab::new_drawing(100));
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
    fn pasting_an_insert_of_an_existing_block_stays_incremental() {
        use document::entities::{Insert, Line};
        use document::types::Vector3;
        let mut app = crate::app::OpenCADStudio::new_for_test();
        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(99));
        let tab = app.tabs.len() - 1;
        app.tabs[tab].scene.define_block_raw(
            "PART",
            Vector3::ZERO,
            vec![document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            ))],
        );
        let source = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Insert(Insert::new(
                "PART",
                Vector3::ZERO,
            )));
        let (entities, deps) = copy_to_clipboard_kernel(&app.tabs[tab].scene.document, &[source]);
        assert!(
            !deps.blocks.is_empty(),
            "the clipboard carries the block it uses"
        );
        app.clipboard = entities;
        app.clipboard_deps = deps;
        app.active_tab = tab;
        let epoch_before = app.tabs[tab].scene.geometry_epoch;
        let records_before = app.tabs[tab].scene.document.block_records.len();
        let handles = app.finalize_paste(
            tab,
            Some(crate::command::EntityTransform::Translate(
                glam::DVec3::new(50.0, 0.0, 0.0),
            )),
        );
        assert_eq!(handles.len(), 1);
        assert_eq!(
            app.tabs[tab].scene.document.block_records.len(),
            records_before
        );
        let changes = app.tabs[tab]
            .scene
            .replay_since(epoch_before)
            .expect("pasting into an existing block must not force a full rebuild");
        assert!(changes.iter().any(|(h, _)| *h == handles[0]));
    }

    #[test]
    fn a_tab_to_tab_paste_through_the_interactive_path_brings_the_definition() {
        use document::entities::{AttributeDefinition, AttributeEntity, EntityType, Insert, Line};
        use document::types::Vector3;

        let mut app = crate::app::OpenCADStudio::new_for_test();
        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(99));
        let source_tab = app.tabs.len() - 1;
        let mut definition =
            AttributeDefinition::new("TAG1".into(), "Tag one".into(), "first".into());
        definition.insertion_point = Vector3::new(2.0, 1.0, 0.0);
        app.tabs[source_tab].scene.define_block_raw(
            "PART",
            Vector3::ZERO,
            vec![
                EntityType::Line(Line::from_points(
                    Vector3::ZERO,
                    Vector3::new(10.0, 0.0, 0.0),
                )),
                EntityType::AttributeDefinition(definition),
            ],
        );
        let attdef = app.tabs[source_tab]
            .scene
            .document
            .block_records
            .get("PART")
            .expect("the definition was created")
            .entity_handles
            .iter()
            .copied()
            .find(|handle| {
                matches!(
                    app.tabs[source_tab].scene.document.get_entity(*handle),
                    Some(EntityType::AttributeDefinition(_))
                )
            })
            .expect("the definition holds its attribute definition");
        let mut insert = Insert::new("PART", Vector3::ZERO);
        let mut attribute = AttributeEntity::new("TAG1".into(), "first".into());
        attribute.attdef_handle = attdef;
        insert.attributes.push(attribute);
        let source = app.tabs[source_tab]
            .scene
            .document
            .add_entity(EntityType::Insert(insert))
            .expect("the seeded reference");

        app.copy_entities_to_clipboard(source_tab, &[source], glam::DVec3::ZERO);
        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(100));
        let target = app.tabs.len() - 1;
        // Give the target a handle history of its own, so a source handle that
        // leaks into the copy cannot pass for one of the target's.
        while app.tabs[target].scene.document.next_handle() <= attdef.value() {
            app.tabs[target].scene.document.allocate_handle();
        }
        app.active_tab = target;
        app.handle_paste_clipboard(glam::DVec3::new(50.0, 0.0, 0.0));

        let doc = &app.tabs[target].scene.document;
        let record = doc
            .block_records
            .get("PART")
            .expect("the definition travelled");
        let pasted_attdef = record
            .entity_handles
            .iter()
            .copied()
            .find(|handle| {
                matches!(
                    doc.get_entity(*handle),
                    Some(EntityType::AttributeDefinition(_))
                )
            })
            .expect("the definition brought its attribute definition");
        assert_ne!(pasted_attdef, attdef);
        let pasted = doc
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(insert) if insert.block_name == "PART" => Some(insert),
                _ => None,
            })
            .expect("the reference arrived");
        assert_eq!(pasted.attributes.len(), 1, "with its value");
        assert_eq!(pasted.attributes[0].value, "first");
        assert_eq!(pasted.attributes[0].attdef_handle, pasted_attdef);
    }

    #[test]
    fn pasted_table_roundtrip_record_brings_its_table_content() {
        use document::objects::{Dictionary, ObjectType, XRecord, XRecordEntry};
        let mut seed = document::Document::new();
        let table = seed
            .add_entity(document::EntityType::Table(Box::new(
                document::entities::Table::new(document::types::Vector3::ZERO, 2, 2),
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
        let mut table_content =
            document::entities::Table::new(document::types::Vector3::ZERO, 2, 2);
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

        let mut dst = document::Document::new();
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
        use document::entities::{
            BlockAttribute, LeaderLine, MultiLeader, MultiLeaderArrowheadOverride,
        };
        use document::objects::{MultiLeaderStyle, ObjectType};
        let mut seed = document::Document::new();
        let mut text = document::tables::TextStyle::new("T1");
        text.handle = seed.allocate_handle();
        let text_handle = text.handle;
        seed.text_styles.add(text).unwrap();
        let mut lt = document::tables::LineType::new("L1");
        lt.handle = seed.allocate_handle();
        let line_handle = lt.handle;
        seed.line_types.add(lt).unwrap();
        let att = document::entities::AttributeDefinition::new("TAG".into(), "".into(), "".into());
        define_block_raw_doc(
            &mut seed,
            "CONTENT",
            document::types::Vector3::ZERO,
            &[document::EntityType::AttributeDefinition(att)],
        );
        let block = seed.block_records.get("CONTENT").unwrap().handle;
        let attribute = crate::scene::block_authoring::members(&seed, block)
            .into_iter()
            .find(|&h| {
                matches!(
                    seed.get_entity(h),
                    Some(document::EntityType::AttributeDefinition(_))
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
            .add_entity(document::EntityType::MultiLeader(Box::new(m)))
            .unwrap();
        let nested = seed.get_entity(source).unwrap().clone();
        define_block_raw_doc(
            &mut seed,
            "OUTER",
            document::types::Vector3::ZERO,
            &[nested],
        );
        let insert = seed
            .add_entity(document::EntityType::Insert(
                document::entities::Insert::new("OUTER", document::types::Vector3::ZERO),
            ))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[source, insert]);
        let mut dst = document::Document::new();
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
        let Some(document::EntityType::MultiLeader(m)) = dst.get_entity(handles[0]) else {
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
            matches!(dst.get_entity(attribute), Some(document::EntityType::AttributeDefinition(a)) if a.tag == "TAG" && a.common.owner_handle == block.unwrap())
        );
        assert_eq!(m.block_attributes[1].attribute_definition_handle, None);
        let record = dst.block_records.get("OUTER").unwrap().handle;
        let nested = crate::scene::block_authoring::members(&dst, record)
            .into_iter()
            .find_map(|h| match dst.get_entity(h) {
                Some(document::EntityType::MultiLeader(m)) => Some(m),
                _ => None,
            })
            .unwrap();
        let mut nested = nested.clone();
        let mut pasted = m.clone();
        assert_eq!(mleader_handles(&mut nested), mleader_handles(&mut pasted));
    }

    #[test]
    fn pasted_dim_style_points_at_the_targets_records() {
        use document::TableEntry;
        let mut seed = document::Document::new();
        let mut text_style = document::tables::TextStyle::new("DIMTXT");
        text_style.set_handle(seed.allocate_handle());
        let text_style = {
            let h = text_style.handle();
            seed.text_styles.add(text_style).unwrap();
            h
        };
        let mut line_type = document::tables::LineType::new("DASHED");
        line_type.set_handle(seed.allocate_handle());
        let line_type = {
            let h = line_type.handle();
            seed.line_types.add(line_type).unwrap();
            h
        };
        let tick = document::EntityType::Line(document::entities::Line::new());
        define_block_raw_doc(
            &mut seed,
            "ARW",
            document::types::Vector3::ZERO,
            std::slice::from_ref(&tick),
        );
        let mut style = document::tables::DimStyle::new("CUSTOM");
        style.set_handle(seed.allocate_handle());
        style.dimtxsty_handle = text_style;
        style.dimltex1_handle = line_type;
        style.dimblk = seed.block_records.get("ARW").unwrap().handle;
        seed.dim_styles.add(style).unwrap();
        let mut dim = document::entities::DimensionLinear::new(
            document::types::Vector3::ZERO,
            document::types::Vector3::new(10.0, 0.0, 0.0),
        );
        dim.base.style_name = "CUSTOM".to_string();
        let dim = seed
            .add_entity(document::EntityType::Dimension(Box::new(
                document::entities::Dimension::Linear(dim),
            )))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[dim]);

        // The target's handles run differently from the source's.
        let mut dst = document::Document::new();
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
        use document::objects::ObjectType;
        let circle = document::EntityType::Circle(document::entities::Circle::new());
        let mut seed = document::Document::new();
        define_block_raw_doc(
            &mut seed,
            "CELLBLK",
            document::types::Vector3::ZERO,
            std::slice::from_ref(&circle),
        );
        let block = seed.block_records.get("CELLBLK").unwrap().handle;
        let mut table = document::entities::Table::new(document::types::Vector3::ZERO, 2, 2);
        table.rows[0].cells[0].contents = vec![document::entities::CellContent::block(block)];
        let table = seed
            .add_entity(document::EntityType::Table(Box::new(table)))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[table]);

        // The target already uses the source block's handle for something else.
        let mut dst = document::Document::new();
        define_block_raw_doc(&mut dst, "OTHER", document::types::Vector3::ZERO, &[circle]);
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
        let Some(document::EntityType::Table(pasted)) = dst.get_entity(handles[0]) else {
            panic!("pasted table missing");
        };
        assert_eq!(
            table_cell_blocks(pasted).collect::<Vec<_>>(),
            vec![target],
            "the cell must point at the target's copy of its block"
        );

        // The TABLECONTENT copy of the cells follows the same map.
        let mut content = document::entities::Table::new(document::types::Vector3::ZERO, 2, 2);
        content.rows[0].cells[0].contents = vec![document::entities::CellContent::block(block)];
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
        use document::objects::{ImageDefinition, ObjectType};
        let circle = document::EntityType::Circle(document::entities::Circle::new());
        let mut seed = document::Document::new();
        define_block_raw_doc(
            &mut seed,
            "CELLBLK",
            document::types::Vector3::ZERO,
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
        let mut table = document::entities::Table::new(document::types::Vector3::ZERO, 2, 2);
        table.rows[0].cells[0].contents = vec![document::entities::CellContent::block(cell_block)];
        let mut image = document::entities::RasterImage::new(
            "fx.bmp",
            document::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        image.definition_handle = Some(definition);
        define_block_raw_doc(
            &mut seed,
            "OUTER",
            document::types::Vector3::ZERO,
            &[
                document::EntityType::Table(Box::new(table)),
                document::EntityType::RasterImage(image),
            ],
        );
        let insert = seed
            .add_entity(document::EntityType::Insert(
                document::entities::Insert::new("OUTER", document::types::Vector3::ZERO),
            ))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[insert]);

        // The target's handles run ahead of the source's, so a kept source
        // handle names something else (or nothing) there.
        let mut dst = document::Document::new();
        for _ in 0..64 {
            dst.allocate_handle();
        }
        define_block_raw_doc(&mut dst, "OTHER", document::types::Vector3::ZERO, &[circle]);
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
        let Some(document::EntityType::Table(pasted)) =
            members.iter().find_map(|&h| match dst.get_entity(h) {
                Some(e @ document::EntityType::Table(_)) => Some(e),
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
        let Some(document::EntityType::RasterImage(pasted)) =
            members.iter().find_map(|&h| match dst.get_entity(h) {
                Some(e @ document::EntityType::RasterImage(_)) => Some(e),
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
        let mut seed = document::Document::new();
        let note = seed
            .add_entity(document::EntityType::MText(Box::new(
                document::entities::MText::new(),
            )))
            .unwrap();
        let mut leader = document::entities::Leader::default();
        leader.annotation_handle = note;
        let leader_handle = seed
            .add_entity(document::EntityType::Leader(leader))
            .unwrap();
        let (entities, deps) = copy_to_clipboard_kernel(&seed, &[leader_handle, note]);

        let mut dst = document::Document::new();
        let (handles, _) = paste_entities_kernel(
            &mut dst,
            &entities,
            &deps,
            None,
            crate::app::PasteSpace::Model,
        );

        assert_eq!(handles.len(), 2);
        let pasted_leader = match dst.get_entity(handles[0]) {
            Some(document::EntityType::Leader(l)) => l,
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
        use document::objects::{Dictionary, ObjectType, XRecord, XRecordEntry, XRecordValue};
        let mut seed = document::Document::new();
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

        let mut dst = document::Document::new();
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
    // `document::Document`, entities, and the shared kernels
    // (`copy_to_clipboard_kernel`, `paste_entities_kernel`,
    // `match_layer_kernel`, `match_properties_kernel` + `MatchOpts`).
    use super::{copy_to_clipboard_kernel, paste_entities_kernel};
    use crate::entities::match_props::{match_layer_kernel, match_properties_kernel, MatchOpts};

    #[test]
    fn headless_copy_paste_then_match_kernels_share_one_path() {
        // Phase 1: copy in doc A, paste into doc B with a translate.
        let mut doc_a = document::Document::new();
        doc_a
            .layers
            .add(document::tables::Layer::new("WALLS"))
            .expect("add layer");
        let mut seed_line = document::entities::Line::from_points(
            document::types::Vector3::new(0.0, 0.0, 0.0),
            document::types::Vector3::new(10.0, 0.0, 0.0),
        );
        seed_line.common.layer = "WALLS".to_string();
        let seed_handle = doc_a
            .add_entity(document::EntityType::Line(seed_line))
            .unwrap();

        let (entities, deps) = copy_to_clipboard_kernel(&doc_a, &[seed_handle]);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].common().layer, "WALLS");

        let mut doc_b = document::Document::new();
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
            Some(document::EntityType::Line(l)) => {
                assert_eq!(l.common.layer, "WALLS");
                assert!((l.start.x - 5.0).abs() < 1e-9, "start.x = {}", l.start.x);
                assert!((l.start.y - 7.0).abs() < 1e-9, "start.y = {}", l.start.y);
                assert!((l.end.x - 15.0).abs() < 1e-9, "end.x = {}", l.end.x);
            }
            other => panic!("expected pasted Line, got {other:?}"),
        }

        // Phase 2: match kernels on a src/dst pair in the same headless doc.
        let mut src_line = document::entities::Line::new();
        src_line.common.layer = "WALLS".to_string();
        src_line.common.linetype_scale = 2.5;
        src_line.thickness = 1.25;
        let src = doc_b
            .add_entity(document::EntityType::Line(src_line))
            .unwrap();
        let mut dst_line = document::entities::Line::new();
        dst_line.common.layer = "0".to_string();
        dst_line.common.linetype_scale = 1.0;
        dst_line.thickness = 0.0;
        let dst = doc_b
            .add_entity(document::EntityType::Line(dst_line))
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
            Some(document::EntityType::Line(l)) => {
                assert_eq!(l.common.layer, "WALLS");
                assert!((l.common.linetype_scale - 2.5).abs() < 1e-9);
                assert!((l.thickness - 1.25).abs() < 1e-9);
            }
            other => panic!("expected matched Line, got {other:?}"),
        }
    }
    #[test]
    fn paste_owner_override_routes_entity_into_block_record() {
        let mut source = document::Document::new();

        let line = document::entities::Line::from_points(
            document::types::Vector3::new(0.0, 0.0, 0.0),
            document::types::Vector3::new(10.0, 0.0, 0.0),
        );
        let source_handle = source
            .add_entity(document::EntityType::Line(line))
            .expect("source line");

        let (entities, deps) = copy_to_clipboard_kernel(&source, &[source_handle]);

        let mut target = document::Document::new();

        let block_handle = target.allocate_handle();
        let mut block = document::tables::BlockRecord::new("BEDIT_TARGET");
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
    fn assert_no_shared_xdicts(doc: &document::Document) {
        use std::collections::HashMap;
        let mut owners: HashMap<document::Handle, usize> = HashMap::new();
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
        let mut doc = document::Document::new();
        let line = doc
            .add_entity(document::EntityType::Line(document::entities::Line::new()))
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
        let mut doc = document::Document::new();

        let dict_handle = doc.allocate_handle();
        let mut dict = document::objects::Dictionary::new();
        dict.handle = dict_handle;
        doc.objects
            .insert(dict_handle, document::objects::ObjectType::Dictionary(dict));

        let mut line = document::entities::Line::new();
        line.common.xdictionary_handle = Some(dict_handle);
        let src_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();

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

    fn defined(name: &str) -> document::Document {
        let mut doc = document::Document::new();
        define_block_raw_doc(&mut doc, name, document::types::Vector3::ZERO, &[]);
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
        scene.define_block_raw("*U9", document::types::Vector3::ZERO, vec![]);
        let record = scene.document.block_records.get("*U9").expect("defined");
        assert!(record.flags.anonymous);
    }
}

#[cfg(test)]
mod paste_viewport_clip_tests {
    use super::{copy_to_clipboard_kernel, paste_entities_kernel};
    use document::Handle;

    fn clipped_viewport(doc: &mut document::Document) -> (Handle, Handle) {
        let mut boundary = document::entities::LwPolyline::new();
        boundary.add_point(document::types::Vector2::new(0.0, 0.0));
        boundary.add_point(document::types::Vector2::new(10.0, 0.0));
        boundary.add_point(document::types::Vector2::new(5.0, 8.0));
        boundary.is_closed = true;
        let boundary = doc
            .add_entity(document::EntityType::LwPolyline(boundary))
            .unwrap();
        let mut viewport = document::entities::Viewport::new();
        viewport.clip_boundary_handle = boundary;
        viewport.status.non_rectangular_clipping = true;
        let viewport = doc
            .add_entity(document::EntityType::Viewport(Box::new(viewport)))
            .unwrap();
        (viewport, boundary)
    }

    fn paste(seed: &document::Document, handles: &[Handle]) -> (document::Document, Vec<Handle>) {
        let (entities, deps) = copy_to_clipboard_kernel(seed, handles);
        let mut dst = document::Document::new();
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

    fn viewport_in(doc: &document::Document, handles: &[Handle]) -> document::entities::Viewport {
        handles
            .iter()
            .find_map(|&h| match doc.get_entity(h) {
                Some(document::EntityType::Viewport(v)) => Some(v.as_ref().clone()),
                _ => None,
            })
            .expect("pasted viewport")
    }

    #[test]
    fn pasted_viewport_is_clipped_by_the_pasted_boundary() {
        let mut seed = document::Document::new();
        let (viewport, boundary) = clipped_viewport(&mut seed);
        let (dst, pasted) = paste(&seed, &[viewport, boundary]);
        let copy = viewport_in(&dst, &pasted);
        let pasted_boundary = pasted
            .iter()
            .copied()
            .find(|&h| matches!(dst.get_entity(h), Some(document::EntityType::LwPolyline(_))))
            .expect("pasted boundary");
        assert_eq!(copy.clip_boundary_handle, pasted_boundary);
        assert!(copy.status.non_rectangular_clipping);
    }

    #[test]
    fn pasted_viewport_without_its_boundary_is_not_clipped() {
        let mut seed = document::Document::new();
        let (viewport, _) = clipped_viewport(&mut seed);
        let (dst, pasted) = paste(&seed, &[viewport]);
        let copy = viewport_in(&dst, &pasted);
        assert!(copy.clip_boundary_handle.is_null());
        assert!(!copy.status.non_rectangular_clipping);
    }
}

#[cfg(test)]
mod paste_assoc_array_tests {
    use crate::app::OpenCADStudio;
    use crate::scene::assoc_array::{array_of_insert, create_from_spec, AssocArraySpec};
    use document::entities::{AttributeEntity, Circle, Insert};
    use document::types::Vector3;
    use document::EntityType;

    /// An associative array of attributed block references in drawing `i`.
    fn attributed_array(app: &mut OpenCADStudio, i: usize) -> document::Handle {
        let scene = &mut app.tabs[i].scene;
        scene.define_block_raw(
            "PART",
            Vector3::ZERO,
            vec![EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0))],
        );
        let source = scene.add_entity(EntityType::Insert(Insert::new("PART", Vector3::ZERO)));
        let mut att = AttributeEntity::new("NO".into(), "1".into());
        att.common.handle = scene.document.allocate_handle();
        att.common.owner_handle = source;
        if let Some(EntityType::Insert(insert)) = scene.document.get_entity_mut(source) {
            insert.attributes.push(att);
        }
        let spec = AssocArraySpec::Rectangular {
            columns: 3,
            rows: 2,
            column_spacing: 20.0,
            row_spacing: 30.0,
        };
        create_from_spec(scene, &[source], glam::DVec3::ZERO, &spec).unwrap()
    }

    /// Ctrl+C / Ctrl+V of an associative array in its own drawing gives an
    /// array of its own (counts in Properties, array grips), as COPY does.
    #[test]
    fn a_pasted_array_in_its_own_drawing_is_an_array_of_its_own() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let array = attributed_array(&mut app, i);
        app.copy_entities_to_clipboard(i, &[array], glam::DVec3::ZERO);
        app.handle_paste_clipboard(glam::DVec3::new(200.0, 0.0, 0.0));

        let document = &app.tabs[i].scene.document;
        let pasted: Vec<document::Handle> = app.tabs[i].scene.selected_handles_in_order();
        assert_eq!(pasted.len(), 1);
        let original = array_of_insert(document, array).unwrap();
        let copied =
            array_of_insert(document, pasted[0]).expect("the pasted reference is an array");
        assert_ne!(copied.action, original.action);
        assert_ne!(copied.array_block, original.array_block);

        app.undo_active_tab();
        assert!(app.tabs[i].scene.document.get_entity(pasted[0]).is_none());
        assert!(array_of_insert(&app.tabs[i].scene.document, array).is_some());
    }

    /// In another drawing the source's handles name nothing of the
    /// source's, so the paste stays a plain reference and nothing breaks.
    #[test]
    fn a_pasted_array_in_another_drawing_stays_a_plain_reference() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let source_tab = app.active_tab;
        let array = attributed_array(&mut app, source_tab);
        app.copy_entities_to_clipboard(source_tab, &[array], glam::DVec3::ZERO);
        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(100));
        let target = app.tabs.len() - 1;
        app.active_tab = target;
        assert_ne!(target, source_tab);
        // Same handle numbers in the target as in the source: an array there
        // under the source's handles must not be mistaken for the copied one.
        let _ = attributed_array(&mut app, target);
        let actions_before = app.tabs[target].scene.document.objects.len();
        app.handle_paste_clipboard(glam::DVec3::new(200.0, 0.0, 0.0));
        let pasted: Vec<document::Handle> = app.tabs[target].scene.selected_handles_in_order();
        assert_eq!(pasted.len(), 1);
        assert!(array_of_insert(&app.tabs[target].scene.document, pasted[0]).is_none());
        assert_eq!(
            app.tabs[target].scene.document.objects.len(),
            actions_before,
            "no array cloned"
        );
    }

    #[test]
    fn pasting_annotative_insert_clones_and_translates_attribute_contexts() {
        use document::entities::{AttributeEntity, Insert};
        use document::objects::{
            Dictionary, MTextAttributeContext, ObjectContextData, ObjectContextKind, ObjectType,
            Scale,
        };
        use document::types::{Vector2, Vector3};

        let mut src_doc = document::Document::new();
        let at_100 = crate::scene::annotative::ensure_scale_object(
            &mut src_doc,
            &Scale {
                name: "1:100".into(),
                paper_units: 1.0,
                drawing_units: 100.0,
                ..Scale::default()
            },
        );
        let mut att = AttributeEntity::new("L".into(), "110".into());
        att.common.handle = src_doc.allocate_handle();
        att.insertion_point = Vector3::new(10.0, 20.0, 0.0);
        att.alignment_point = Vector3::new(10.0, 20.0, 0.0);

        let att_xdict = src_doc.allocate_handle();
        let att_mgr = src_doc.allocate_handle();
        let att_coll = src_doc.allocate_handle();
        let att_leaf = src_doc.allocate_handle();

        let mut d_root = Dictionary::new();
        d_root.handle = att_xdict;
        d_root.owner = att.common.handle;
        d_root.hard_owner = true;
        d_root.add_entry("AcDbContextDataManager", att_mgr);

        let mut d_mgr = Dictionary::new();
        d_mgr.handle = att_mgr;
        d_mgr.owner = att_xdict;
        d_mgr.add_entry("ACDB_ANNOTATIONSCALES", att_coll);

        let mut d_coll = Dictionary::new();
        d_coll.handle = att_coll;
        d_coll.owner = att_mgr;
        d_coll.add_entry("*A1", att_leaf);

        let leaf = ObjectType::ObjectContextData(ObjectContextData {
            handle: att_leaf,
            owner_handle: att_coll,
            reactors: vec![att_coll],
            xdictionary_handle: None,
            class_version: 4,
            has_file_to_extension_dictionary: true,
            uses_legacy_mleader_context_base: false,
            is_default: true,
            scale: at_100,
            kind: ObjectContextKind::MTextAttribute(MTextAttributeContext {
                horizontal_mode: 0,
                rotation: 0.0,
                insertion: Vector2::new(12.0, 24.0),
                alignment: Vector2::new(12.0, 24.0),
                enable_context: false,
                context: None,
            }),
        });

        src_doc
            .objects
            .insert(att_xdict, ObjectType::Dictionary(d_root));
        src_doc
            .objects
            .insert(att_mgr, ObjectType::Dictionary(d_mgr));
        src_doc
            .objects
            .insert(att_coll, ObjectType::Dictionary(d_coll));
        src_doc.objects.insert(att_leaf, leaf);
        att.common.xdictionary_handle = Some(att_xdict);

        let mut ins = Insert::new("DOOR_SYM", Vector3::new(10.0, 20.0, 0.0));
        ins.attributes.push(att);
        let src_handle = src_doc.add_entity(document::EntityType::Insert(ins)).unwrap();
        let copied_entities = vec![src_doc.get_entity(src_handle).unwrap().clone()];
        let deps = crate::app::ClipboardDeps::capture(&src_doc, &copied_entities);

        let delta = glam::DVec3::new(25.0, 15.0, 0.0);
        let translate = crate::command::EntityTransform::Translate(delta);

        // 1. Same-document paste: attribute xdict must be cloned (distinct handle) and translated.
        let (same_pasted, _) = super::paste_entities_kernel(
            &mut src_doc,
            &copied_entities,
            &deps,
            Some(&translate),
            super::PasteSpace::Model,
        );
        let pasted_h = same_pasted[0];
        let document::EntityType::Insert(pasted_ins) = src_doc.get_entity(pasted_h).unwrap() else {
            panic!("expected pasted INSERT")
        };
        assert!(pasted_ins.attributes[0].common.xdictionary_handle.is_some());
        assert_ne!(
            pasted_ins.attributes[0].common.xdictionary_handle,
            Some(att_xdict),
            "pasted attribute must not alias source xdictionary"
        );
        let mut placed = pasted_ins.attributes[0].clone();
        assert!(crate::scene::annotative::place_attribute_for_annotation_scale(
            &src_doc,
            &mut placed,
            Some(at_100),
        ));
        assert!((placed.insertion_point.x - 37.0).abs() < 1e-9);
        assert!((placed.insertion_point.y - 39.0).abs() < 1e-9);

        // Source attribute context stays untouched.
        let document::EntityType::Insert(src_ins) = src_doc.get_entity(src_handle).unwrap() else {
            panic!("expected source INSERT")
        };
        let mut src_placed = src_ins.attributes[0].clone();
        assert!(crate::scene::annotative::place_attribute_for_annotation_scale(
            &src_doc,
            &mut src_placed,
            Some(at_100),
        ));
        assert!((src_placed.insertion_point.x - 12.0).abs() < 1e-9);
        assert!((src_placed.insertion_point.y - 24.0).abs() < 1e-9);

        // 2. Cross-document paste: attribute xdict and scale are recreated and translated.
        let mut dst_doc = document::Document::new();
        let (cross_pasted, _) = super::paste_entities_kernel(
            &mut dst_doc,
            &copied_entities,
            &deps,
            Some(&translate),
            super::PasteSpace::Model,
        );
        let dst_scale = crate::scene::annotative::scale_handle_by_name(&dst_doc, "1:100")
            .expect("1:100 scale recreated in target document");
        let document::EntityType::Insert(cross_ins) =
            dst_doc.get_entity(cross_pasted[0]).unwrap()
        else {
            panic!("expected cross-pasted INSERT")
        };
        let mut cross_placed = cross_ins.attributes[0].clone();
        assert!(crate::scene::annotative::place_attribute_for_annotation_scale(
            &dst_doc,
            &mut cross_placed,
            Some(dst_scale),
        ));
        assert!((cross_placed.insertion_point.x - 37.0).abs() < 1e-9);
        assert!((cross_placed.insertion_point.y - 39.0).abs() < 1e-9);
    }
}
