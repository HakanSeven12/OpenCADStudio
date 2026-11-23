// Entity CRUD, transactions, rollback, selection and XDATA.

use super::*;

impl<'a> HostSession<'a> {
    /// Recompute host-derived state for entities a script created or edited.
    fn normalize_scripted_entity(
        &self,
        old: Option<&EntityType>,
        entity: &mut EntityType,
    ) -> Result<(), String> {
        if let EntityType::Dimension(dimension) = entity {
            crate::entities::dimension::normalize_scripted_dimension(dimension);
        }
        if let EntityType::Viewport(viewport) = entity {
            // A viewport needs an id unique in its layout and at least 2 (id 1
            // is the never-drawn sheet viewport, and a lone id 0 can be taken for
            // it), and its scale follows its paper height and model view height;
            // both are what the MVIEW command sets.
            if old.is_none() {
                let owner = viewport.common.owner_handle;
                let highest = self
                    .document()
                    .entities()
                    .filter_map(|e| match e {
                        EntityType::Viewport(other) if other.common.owner_handle == owner => {
                            Some(other.id)
                        }
                        _ => None,
                    })
                    .max()
                    .unwrap_or(1);
                viewport.id = (highest + 1).max(2);
            }
            viewport.custom_scale = viewport.height / viewport.view_height;
        }
        if let EntityType::SectionSymbol(symbol) = entity {
            // `points` is the canonical geometry: the point counts follow it
            // (they equal the point count in every verified export) and the
            // end, tick and label projections are refreshed from it. A symbol
            // whose points did not change is left exactly as it was read.
            let unchanged = matches!(old, Some(EntityType::SectionSymbol(previous)) if previous.points == symbol.points);
            if !unchanged {
                let count = i32::try_from(symbol.points.len()).unwrap_or(i32::MAX);
                symbol.raw_point_count_90 = count;
                symbol.raw_point_record_count = count;
                symbol.sync_display_fields();
            }
        }
        if let EntityType::Spline(spline) = entity {
            // The DXF writer emits weights only for a rational spline, so the
            // flag must follow the weights a script sets.
            spline.flags.rational = !spline.weights.is_empty();
        }
        if let EntityType::Helix(new) = entity {
            let old = match old {
                Some(EntityType::Helix(old)) => Some(&**old),
                _ => None,
            };
            crate::entities::helix::normalize_scripted_helix(old, new)?;
        }
        if let EntityType::Table(new) = entity {
            let old = match old {
                Some(EntityType::Table(old)) => Some(&**old),
                _ => None,
            };
            crate::entities::table::normalize_scripted_table(old, new);
        }
        if let EntityType::MLine(new) = entity {
            let old = match old {
                Some(EntityType::MLine(old)) => Some(old),
                _ => None,
            };
            crate::entities::mline::normalize_scripted_mline(old, new, self.document())?;
        }
        Ok(())
    }

    /// A scripted RasterImage names a file; the host creates the image
    /// definition object it needs (pixel size read from the file) and links it.
    fn prepare_scripted_raster_image(&mut self, entity: &mut EntityType) -> Result<(), String> {
        let EntityType::RasterImage(image) = entity else {
            return Ok(());
        };
        if image
            .definition_handle
            .is_some_and(|handle| !handle.is_null())
        {
            return Ok(());
        }
        let (width, height) = image::image_dimensions(&image.file_path)
            .map_err(|error| format!("cannot read image {:?}: {error}", image.file_path))?;
        // One definition per file, as AutoCAD keeps it: reuse an existing one.
        let existing = self
            .document()
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                document::objects::ObjectType::ImageDefinition(definition)
                    if definition.file_name == image.file_path =>
                {
                    Some(*handle)
                }
                _ => None,
            });
        let handle = match existing {
            Some(handle) => handle,
            None => {
                let handle = self.document_mut().allocate_handle();
                let mut definition = document::objects::ImageDefinition::with_dimensions(
                    image.file_path.clone(),
                    width,
                    height,
                );
                definition.handle = handle;
                definition.is_loaded = true;
                if let Some(dictionary) =
                    crate::app::command_driver::clipboard::register_image_definition(
                        self.document_mut(),
                        handle,
                        &image.file_path,
                    )
                {
                    definition.owner = dictionary;
                }
                self.document_mut().objects.insert(
                    handle,
                    document::objects::ObjectType::ImageDefinition(definition),
                );
                handle
            }
        };
        image.definition_handle = Some(handle);
        // `size` is the image's pixel size, and an untouched clip boundary
        // covers the whole image, so both follow the file.
        let untouched_clip = image.clip_boundary
            == document::entities::ClipBoundary::full_image(image.size.x, image.size.y);
        image.size = document::types::Vector2::new(f64::from(width), f64::from(height));
        if untouched_clip {
            image.clip_boundary =
                document::entities::ClipBoundary::full_image(f64::from(width), f64::from(height));
        }
        Ok(())
    }

    /// True when `entity` (not yet in the document) targets a locked layer.
    /// Additions apply the same lock refusal as stamps and edits (P1-10).
    pub(super) fn new_entity_layer_locked(&self, entity: &EntityType) -> bool {
        self.app.tabs[self.tab]
            .scene
            .document
            .layers
            .get(&entity.common().layer)
            .is_some_and(|layer| layer.is_locked())
    }

    pub fn add_entity(&mut self, mut entity: EntityType) -> Handle {
        if self.prepare_scripted_raster_image(&mut entity).is_err() {
            return Handle::NULL;
        }
        let epoch = self.geometry_epoch();
        if self.normalize_scripted_entity(None, &mut entity).is_err() {
            return Handle::NULL;
        }
        if let EntityType::AttributeEntity(mut attribute) = entity {
            let owner = attribute.common.owner_handle;
            let Some(EntityType::Insert(mut insert)) = self.document().get_entity(owner).cloned()
            else {
                return Handle::NULL;
            };
            if attribute.common.handle.is_null() {
                attribute.common.handle = self.document_mut().allocate_handle();
            }
            let handle = attribute.common.handle;
            if insert
                .attributes
                .iter()
                .any(|existing| existing.common.handle == handle)
            {
                return Handle::NULL;
            }
            insert.attributes.push(attribute);
            if insert.seqend_handle.is_none() {
                insert.seqend_handle = Some(self.document_mut().allocate_handle());
            }
            if !self.app.tabs[self.tab]
                .scene
                .update_entity_plugin(EntityType::Insert(insert))
            {
                return Handle::NULL;
            }
            self.app.tabs[self.tab].mark_plugin_entity_dirty(handle, epoch);
            self.publish_document_view();
            return handle;
        }
        // Locked layers refuse additions like every other mutation path
        // (attributes ride their owning insert, which the scene refuses).
        if self.new_entity_layer_locked(&entity) {
            eprintln!("[plugin] add_entity refused: the target layer is locked");
            return Handle::NULL;
        }
        let handle = self.app.tabs[self.tab].scene.add_entity(entity);
        self.app.sync_registered_layers(self.tab);
        self.app.tabs[self.tab].mark_plugin_entity_dirty(handle, epoch);
        self.publish_document_view();
        handle
    }

    pub fn add_entities(&mut self, mut entities: Vec<EntityType>) -> Vec<Handle> {
        // P1-10: preparation/normalization/locked-layer failures are no
        // longer ignored. A refused item keeps its result slot as NULL (the
        // legacy positional contract holds) instead of entering the document
        // unprepared, unnormalized, or on a locked layer; the valid remainder
        // proceeds.
        let mut flags = Vec::with_capacity(entities.len());
        for entity in &mut entities {
            flags.push(
                self.prepare_scripted_raster_image(entity).is_ok()
                    && self.normalize_scripted_entity(None, entity).is_ok()
                    && !self.new_entity_layer_locked(entity),
            );
        }
        let mut prepared = Vec::with_capacity(entities.len());
        let mut slots: Vec<Option<usize>> = Vec::with_capacity(entities.len());
        for (entity, ok) in entities.drain(..).zip(flags.iter().copied()) {
            if ok {
                slots.push(Some(prepared.len()));
                prepared.push(entity);
            } else {
                slots.push(None);
            }
        }
        let epoch = self.geometry_epoch();
        let added = self.app.tabs[self.tab].scene.add_entities(prepared);
        self.app.sync_registered_layers(self.tab);
        let mut added_iter = added.into_iter();
        let mut handles = Vec::with_capacity(slots.len());
        let mut any_added = false;
        for slot in slots {
            match slot {
                Some(_) => {
                    let handle = added_iter
                        .next()
                        .expect("one added handle per prepared item");
                    self.app.tabs[self.tab].mark_plugin_entity_dirty(handle, epoch);
                    handles.push(handle);
                    any_added = true;
                }
                None => handles.push(Handle::NULL),
            }
        }
        if any_added {
            self.publish_document_view();
        }
        handles
    }

    pub fn bump_geometry(&mut self) {
        self.app.tabs[self.tab].scene.bump_geometry();
    }

    pub(super) fn bump_handles_incremental(&mut self, handles: &[Handle]) {
        if handles.is_empty() {
            return;
        }
        let changes: Vec<_> = handles
            .iter()
            .copied()
            .map(|h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        let scene = &mut self.app.tabs[self.tab].scene;
        if scene.changes_touch_block_definition(&changes) {
            scene.bump_geometry();
        } else {
            scene.bump_entities(&changes);
        }
    }

    /// Replace the entity carrying `entity`'s handle in place, refreshing the
    /// scene's derived caches. Returns `false` when no entity has that handle.
    pub fn update_entity(&mut self, entity: EntityType) -> bool {
        let result_handle = entity.common().handle;
        // API v13 dirty regions: mark BEFORE the write's boundary publish
        // (the inner publishes immediately) so the region set is complete
        // when the notification rides.
        let epoch = self.geometry_epoch();
        self.app.tabs[self.tab].mark_plugin_entity_dirty(result_handle, epoch);
        self.update_entity_inner(entity)
    }

    fn update_entity_inner(&mut self, mut entity: EntityType) -> bool {
        let old = self.document().get_entity(entity.common().handle).cloned();
        // P1-10 route consistency: a direct update runs the same
        // reference-coverage validation as the transaction route (attributes
        // keep their nested replace path, which the scene validates).
        if !matches!(entity, EntityType::AttributeEntity(_)) {
            let Some(existing) = old.as_ref() else {
                return false;
            };
            let bound = ocs_plugin_api::entity_coverage::bind_canvas_entity_references(
                self.document(),
                &mut entity,
            )
            .is_ok();
            let valid = bound
                && ocs_plugin_api::entity_coverage::validate_entity_mutation(existing, &entity)
                    .is_ok();
            if !valid {
                return false;
            }
        }
        if self
            .normalize_scripted_entity(old.as_ref(), &mut entity)
            .is_err()
        {
            return false;
        }
        let ok = match entity {
            EntityType::AttributeEntity(attribute) => self.replace_nested_attribute(attribute),
            entity => self.app.tabs[self.tab].scene.update_entity_plugin(entity),
        };
        self.app.sync_registered_layers(self.tab);
        if ok {
            self.publish_document_view();
            self.refresh_grips_of_active_tab();
        }
        ok
    }

    pub fn update_entities_transaction(
        &mut self,
        label: &str,
        mut entities: Vec<EntityType>,
    ) -> Result<(), String> {
        use std::collections::HashSet;
        if label.trim().is_empty() {
            return Err("transaction label is empty".into());
        }
        if entities.is_empty() {
            return Ok(());
        }
        let mut seen = HashSet::new();
        for entity in &mut entities {
            let handle = entity.common().handle;
            if handle.is_null() || !seen.insert(handle) {
                return Err(format!("null or duplicate entity handle: {handle:?}"));
            }
            let Some(existing) = self.host_model_entity(handle) else {
                return Err(format!("entity {handle:?} does not exist"));
            };
            if std::mem::discriminant(&existing) != std::mem::discriminant(entity) {
                return Err(format!("entity {handle:?} changes kind"));
            }
            if existing.common().owner_handle != entity.common().owner_handle {
                return Err(format!("entity {handle:?} changes owner"));
            }
            ocs_plugin_api::entity_coverage::bind_canvas_entity_references(self.document(), entity)
                .map_err(|error| format!("entity {handle:?}: {error}"))?;
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&existing, entity)
                .map_err(|error| format!("entity {handle:?}: {error}"))?;
            self.normalize_scripted_entity(Some(&existing), entity)
                .map_err(|error| format!("entity {handle:?}: {error}"))?;
            let lock_handle = if matches!(entity, EntityType::AttributeEntity(_)) {
                entity.common().owner_handle
            } else {
                handle
            };
            if self.app.tabs[self.tab].scene.is_layer_locked(lock_handle) {
                return Err(format!("entity {handle:?} is on a locked layer"));
            }
        }
        // API v13 dirty regions: mark every entity BEFORE the batch below
        // (the inner publishes immediately) so the region set is complete
        // when the notification rides.
        let epoch = self.geometry_epoch();
        for entity in &entities {
            self.app.tabs[self.tab].mark_plugin_entity_dirty(entity.common().handle, epoch);
        }
        self.push_undo(label);
        // The plain entities ride ONE batched scene write: a single
        // update_entities_plugin call publishes one geometry bump for the
        // whole set, so the parametric constraint solver (which runs on the
        // bump) sees the FINAL state of every entity at once. A per-entity
        // loop here would solve over each half-updated intermediate and the
        // constrained neighbors would drag the result off the written
        // geometry (the plugin settle drift the consumer walk caught:
        // bonded elements written one by one landed ~1e0 off the records).
        // Nested attributes keep their special replace path (they are not
        // scene-model entities).
        let mut plain = Vec::with_capacity(entities.len());
        // Before-images for a defensive rollback. Validation above makes the
        // commit effectively infallible while the session owns the document
        // exclusively, but the host must never PANIC on plugin input: a late
        // refusal (locked layer re-locked mid-batch, hash miss, memory
        // pressure) previously aborted the process AFTER earlier entries had
        // already mutated. The applied writes are rolled back through the
        // same plugin-write paths, so caches, undo deltas and the geometry
        // bump see a coherent restored state and the caller gets an honest
        // error.
        let mut rollback_nested: Vec<EntityType> = Vec::new();
        let mut rollback_plain: Vec<EntityType> = Vec::new();
        for entity in entities {
            match entity {
                EntityType::AttributeEntity(attribute) => {
                    let owner = attribute.common.owner_handle;
                    if let Some(before) = self.host_model_entity(owner) {
                        rollback_nested.push(before);
                    }
                    if !self.replace_nested_attribute(attribute) {
                        self.rollback_transaction_writes(
                            &mut rollback_nested,
                            &mut rollback_plain,
                            usize::MAX,
                        );
                        self.discard_pending_transaction_scope();
                        return Err("transaction commit refused a nested attribute write after \
                                    validation; applied writes were rolled back"
                            .into());
                    }
                }
                entity => plain.push(entity),
            }
        }
        if !plain.is_empty() {
            rollback_plain = plain
                .iter()
                .filter_map(|entity| self.host_model_entity(entity.common().handle))
                .collect();
            let expected = plain.len();
            let moved = self.app.tabs[self.tab].scene.update_entities_plugin(plain);
            if moved != expected {
                // Only the first `moved` entries were applied — restore those.
                self.rollback_transaction_writes(&mut rollback_nested, &mut rollback_plain, moved);
                self.discard_pending_transaction_scope();
                return Err(format!(
                    "transaction commit wrote {moved} of {expected} entities after \
                     validation; applied writes were rolled back"
                ));
            }
        }
        self.app.sync_registered_layers(self.tab);
        self.set_dirty();
        self.publish_document_view();
        self.refresh_grips_of_active_tab();
        Ok(())
    }

    /// Restore the before-images of a partially applied transaction commit.
    /// `plain_applied` bounds how many plain before-images to restore
    /// (usize::MAX = all nested images and the whole plain set).
    fn rollback_transaction_writes(
        &mut self,
        nested: &mut Vec<EntityType>,
        plain: &mut Vec<EntityType>,
        plain_applied: usize,
    ) {
        // Exact reverse of the application order (plain entries ride the
        // batched write AFTER the nested attribute writes, and one batch can
        // contain BOTH a nested attribute of insert H and H itself): undo the
        // plain writes first, then the nested attribute writes. Restoring in
        // the other order would splice the pre-plain (but already
        // attribute-mutated) before-image over the original content.
        if !plain.is_empty() {
            let applied = plain.len().min(plain_applied);
            let _ = self.app.tabs[self.tab]
                .scene
                .update_entities_plugin(plain.drain(..applied).collect());
        }
        for before in nested.drain(..).rev() {
            let _ = self.app.tabs[self.tab].scene.update_entity_plugin(before);
        }
        self.app.sync_registered_layers(self.tab);
        self.publish_document_view();
    }

    /// Close a failed transaction's pending undo scope so subsequent plugin
    /// writes cannot silently merge into it: the rollback above restored the
    /// content, and the scope's delta records are dropped (net-zero change,
    /// no undo entry).
    fn discard_pending_transaction_scope(&mut self) {
        if self.app.tabs[self.tab].history.pending.is_some() {
            self.app.discard_last_undo_entry(self.tab);
        }
    }

    pub fn selection(&self) -> Vec<Handle> {
        self.app.tabs[self.tab].scene.selected_handles_in_order()
    }

    pub fn set_selection(&mut self, handles: &[Handle]) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for handle in handles {
            if !seen.insert(*handle) {
                return Err(format!("duplicate selected entity: {handle:?}"));
            }
            if self.document().get_entity(*handle).is_none() {
                return Err(format!("entity {handle:?} does not exist"));
            }
        }
        if self.selection() == handles {
            return Ok(());
        }
        let scene = &mut self.app.tabs[self.tab].scene;
        scene.replace_selection_exact(handles);
        self.refresh_grips_of_active_tab();
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.tab == self.app.active_tab {
                self.app.last_plugin_selection = Some((
                    self.tab_id(),
                    self.app.tabs[self.tab].scene.selection_fingerprint(),
                ));
            }
            let revision = self.app.bump_selection_revision(self.tab_id());
            crate::plugin::v4_support::publish_selection_changed_versioned(
                self.tab_id(),
                self.selection(),
                revision,
                false,
            );
        }
        Ok(())
    }

    /// Erase through the plugin path while keeping reference-only designations
    /// undoable with the erase (API v9). The designation set is scene state,
    /// not a document write: erasing drops the set entry in
    /// `Scene::remove_entities`, so its first-touch before-image must join the
    /// open plugin scope here — otherwise undoing the erase would restore the
    /// entity without its designation. No-op for undesignated handles and when
    /// no scope is open (then the erase follows the same no-bookkeeping
    /// contract as any other entity write).
    pub(super) fn erase_entities_recording_designations(&mut self, handles: &[Handle]) {
        for &handle in handles {
            if self.app.tabs[self.tab]
                .scene
                .reference_only
                .is_marked(handle)
            {
                self.app
                    .record_reference_designation_before_pending(self.tab, handle, true);
            }
        }
        self.app.tabs[self.tab].scene.erase_entities_plugin(handles);
    }

    /// Delete the entity with `handle`, keeping the scene's render caches in
    /// sync. Returns `false` when the entity is absent or on a locked layer
    /// (which `erase_entities` refuses to remove). A failed removal no longer
    /// marks the entity dirty (P1-17): the dirty-region claim follows the
    /// actual outcome.
    pub fn remove_entity(&mut self, handle: Handle) -> bool {
        self.remove_entity_inner(handle)
    }

    /// Dirty-region mark for a successful removal: a removed handle still
    /// rides the set — consumers detect its absence.
    fn mark_removed_entity_dirty(&mut self, handle: Handle) {
        let epoch = self.geometry_epoch();
        self.app.tabs[self.tab].mark_plugin_entity_dirty(handle, epoch);
    }

    fn remove_entity_inner(&mut self, handle: Handle) -> bool {
        if self.document().get_entity(handle).is_none() {
            let Some(EntityType::AttributeEntity(attribute)) = self.nested_attribute(handle) else {
                return false;
            };
            let owner = attribute.common.owner_handle;
            let Some(EntityType::Insert(mut insert)) = self.document().get_entity(owner).cloned()
            else {
                return false;
            };
            insert
                .attributes
                .retain(|candidate| candidate.common.handle != handle);
            let removed = self.app.tabs[self.tab]
                .scene
                .update_entity(EntityType::Insert(insert));
            if removed {
                self.mark_removed_entity_dirty(handle);
                self.publish_document_view();
                self.refresh_grips_of_active_tab();
            }
            return removed;
        }
        let owner = self
            .document()
            .get_entity(handle)
            .map(|entity| entity.common().owner_handle);
        self.erase_entities_recording_designations(&[handle]);
        let removed = self.document().get_entity(handle).is_none();
        if removed {
            // The document keeps a block record's member list separately from the
            // entity store, and removing an entity leaves its handle listed; drop
            // it so a definition never lists a member that no longer exists.
            if let Some(owner) = owner {
                if let Some(record) = self
                    .document_mut()
                    .block_records
                    .iter_mut()
                    .find(|record| record.handle == owner)
                {
                    record.entity_handles.retain(|member| *member != handle);
                }
            }
            self.mark_removed_entity_dirty(handle);
            self.publish_document_view();
            self.refresh_grips_of_active_tab();
        }
        removed
    }

    // ── XDATA convenience ──────────────────────────────────────────────────
    // Plugins persist domain data as XDATA on plain entities so it round-trips
    // through DWG/DXF. These wrap the `document::xdata` API keyed by entity
    // handle and keep the APPID table in sync.

    /// Read the XDATA record for `app_name` attached to entity `handle`, if any.
    pub fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord> {
        self.document()
            .get_entity(handle)?
            .common()
            .extended_data
            .get_record(app_name)
    }

    /// Attach `record` to entity `handle`, replacing any existing record for the
    /// same application. Registers the application in the APPID table when
    /// missing so the file stays valid for other CAD apps. Returns `false` when
    /// the entity does not exist.
    pub fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool {
        if !self.write_record_quiet(handle, record) {
            return false;
        }
        self.bump_geometry();
        self.set_dirty();
        self.publish_document_view();
        true
    }

    /// [`Self::write_record`] without the boundary publish/bump: batch paths
    /// that stamp many records publish ONCE at the end (P1-17). The dirty
    /// (entity, app) mark still runs so the final boundary carries it.
    pub(super) fn write_record_quiet(
        &mut self,
        handle: Handle,
        record: ExtendedDataRecord,
    ) -> bool {
        if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
            return false;
        }
        let app = record.application_name.clone();
        self.ensure_app_id(&app);
        let app_handle = self.document().app_ids.get(&app).map(|a| a.handle.value());
        let Some(entity) = self.document_mut().get_entity_mut(handle) else {
            return false;
        };
        let xd = &mut entity.common_mut().extended_data;
        // Drop any existing record for this app, then append the new one.
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| r.application_name != app)
            .cloned()
            .collect();
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        xd.add_record(record);
        // Drop stale verbatim EED for this app so the fresh record — not the
        // pre-edit bytes captured on a prior read — wins on the next save.
        // Otherwise a plugin's edit made after a save/reopen (which registered
        // the app in `raw_dwg_eed`) would not persist.
        if let Some(ah) = app_handle {
            xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
        }
        // API v13 dirty regions: the touched (entity, app) pair rides the
        // boundary notification — marked BEFORE the publish that carries it.
        let epoch = self.geometry_epoch();
        self.app.tabs[self.tab].mark_plugin_record_dirty(handle, app, epoch);
        self.bump_handles_incremental(&[handle]);
        true
    }

    /// Remove the XDATA record for `app_name` from entity `handle`. Returns
    /// `true` when a record was actually removed.
    pub fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool {
        if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
            return false;
        }
        let app_handle = self
            .document()
            .app_ids
            .get(app_name)
            .map(|a| a.handle.value());
        let Some(entity) = self.document_mut().get_entity_mut(handle) else {
            return false;
        };
        let xd = &mut entity.common_mut().extended_data;
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| r.application_name != app_name)
            .cloned()
            .collect();
        let removed_record = kept.len() != xd.records().len();
        // Also drop verbatim EED for this app so the removal persists across a
        // save (a record read back from DWG lives in `raw_dwg_eed`).
        let removed_raw = app_handle
            .map(|ah| xd.raw_dwg_eed.iter().any(|(a, _)| *a == ah))
            .unwrap_or(false);
        if !removed_record && !removed_raw {
            return false;
        }
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        if let Some(ah) = app_handle {
            xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
        }
        self.bump_handles_incremental(&[handle]);
        self.set_dirty();
        self.publish_document_view();
        true
    }

    /// Register `name` in the APPID table if it is not already present, so XDATA
    /// written under it survives a DWG/DXF round-trip. The entry is given a real
    /// handle — a null-handle APPID is written as handle 0, which the DWG EED
    /// reference then can't resolve, so the XDATA would be dropped on reopen.
    fn ensure_app_id(&mut self, name: &str) {
        let doc = self.document_mut();
        if !doc.app_ids.contains(name) {
            let mut app = AppId::new(name);
            app.handle = doc.allocate_handle();
            let _ = doc.app_ids.add(app);
        }
    }
}
