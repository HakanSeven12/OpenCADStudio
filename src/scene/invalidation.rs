use super::*;

impl Scene {
    /// Push one delta onto the journal, evicting the oldest past the cap and
    /// raising the floor so a consumer that fell behind falls back to a full
    /// rebuild. Every `geometry_epoch` bump must call this exactly once so the
    /// journal never has a gap (a gap would let a consumer serve stale geometry).
    pub(crate) fn push_geometry_delta(
        &self,
        epoch: u64,
        changes: Vec<(Handle, ChangeKind)>,
        full: bool,
    ) {
        let pending_categories = std::mem::take(&mut *self.pending_removed_categories.borrow_mut());
        let removed_categories = if full {
            HashMap::default()
        } else {
            let mut removed = HashMap::default();
            for &(handle, kind) in &changes {
                if let Some(&bits) = pending_categories.get(&handle) {
                    if matches!(kind, ChangeKind::Removed) {
                        removed.insert(handle, bits);
                    }
                }
            }
            removed
        };
        let mut ring = self.geometry_deltas.borrow_mut();
        ring.push_back(GeometryDelta {
            epoch,
            changes,
            removed_categories,
            full,
            layer_visibility_only: false,
        });
        while ring.len() > GEOMETRY_JOURNAL_CAP {
            if let Some(ev) = ring.pop_front() {
                self.geometry_journal_floor.set(ev.epoch);
            }
        }
    }

    /// Coalesce the exact set of handles that changed between a consumer's
    /// last-synced epoch and now, or `None` if the range can't be replayed
    /// (a spanned `full` delta, or the consumer fell off the end of the ring) —
    /// in which case the consumer must rebuild from scratch. Added+Removed of the
    /// same handle cancel; otherwise the last change wins.
    pub(crate) fn replay_since(&self, last_epoch: u64) -> Option<Vec<(Handle, ChangeKind)>> {
        // Anything at or below the floor was evicted — can't catch up.
        if last_epoch < self.geometry_journal_floor.get() {
            return None;
        }
        let ring = self.geometry_deltas.borrow();
        let mut state: HashMap<Handle, ChangeKind> = HashMap::default();
        for d in ring.iter() {
            if d.epoch <= last_epoch {
                continue;
            }
            if d.full {
                return None;
            }
            for &(h, k) in &d.changes {
                use ChangeKind::*;
                match (state.get(&h).copied(), k) {
                    (None, k) => {
                        state.insert(h, k);
                    }
                    // Added then removed within the window ⇒ never existed.
                    (Some(Added), Removed) => {
                        state.remove(&h);
                    }
                    // Added stays Added even after later coordinate edits.
                    (Some(Added), _) => {}
                    // Removed then re-added ⇒ treat as a modify (exists, re-tess).
                    (Some(Removed), Added) => {
                        state.insert(h, Modified);
                    }
                    // Removal dominates a prior modify.
                    (Some(Modified), Removed) => {
                        state.insert(h, Removed);
                    }
                    (Some(Removed), _) => {}
                    (Some(Modified), _) => {}
                }
            }
        }
        Some(state.into_iter().collect())
    }

    /// Whether this entity contributes glyphs to the SDF text buffer, so a
    /// change to it must invalidate the cached text vertices. Every entity whose
    /// `to_render` can emit a `GlyphRun` belongs here — miss one and its glyphs
    /// linger after an erase and fail to appear after an add.
    pub(crate) fn entity_affects_text_cache(&self, entity: &EntityType) -> bool {
        if matches!(
            entity,
            EntityType::Text(_)
                | EntityType::MText(_)
                | EntityType::Dimension(_)
                | EntityType::MultiLeader(_)
                | EntityType::Leader(_)
                | EntityType::Table(_)
                | EntityType::Tolerance(_)
                | EntityType::AttributeEntity(_)
                | EntityType::AttributeDefinition(_)
                | EntityType::Insert(_)
        ) {
            return true;
        }
        // A raster reference it cannot resolve draws the saved path as a glyph
        // run in place of the picture (`RasterImage::to_render`), so its
        // broken-reference placeholder lives in the text buffer; a resolvable
        // one contributes only its frame. Same memoised probe the converter
        // uses, so the two cannot disagree — and keeping it a probe rather than
        // a blanket type match keeps a working image's grip-drag from
        // rebuilding every glyph in the drawing on each mouse move.
        if let EntityType::RasterImage(image) = entity {
            let path = image.file_path.trim();
            if !path.is_empty() && crate::scene::model::image_model::resolve_image(path).is_none() {
                return true;
            }
        }
        // Complex-linetype glyphs ride the host entity's wire.
        let lt = crate::scene::view::render::linetype_name_for(&self.document, entity);
        crate::io::linetypes::resolve_complex_lt(&self.document, lt).is_some()
    }

    /// Stage the cache categories an entity belongs to before erase removes the
    /// only cheap way to classify it. The next Removed geometry delta consumes
    /// this entry; even a zero mask is meaningful ("known plain geometry").
    pub(crate) fn remember_removed_cache_categories(&self, handle: Handle) {
        let Some(entity) = self.document.get_entity(handle) else {
            return;
        };
        let mut bits = 0u16;
        if crate::scene::annotative::is_annotative(&self.document, entity) {
            bits |= CACHE_CATEGORY_ANNOTATIVE;
        }
        if self.hatches.contains_key(&handle) {
            bits |= CACHE_CATEGORY_HATCH | CACHE_CATEGORY_INTERACTION;
        }
        if matches!(entity, EntityType::Wipeout(_)) {
            bits |= CACHE_CATEGORY_WIPEOUT;
        }
        if self.images.contains_key(&handle) {
            bits |= CACHE_CATEGORY_IMAGE;
        }
        let is_insert = matches!(entity, EntityType::Insert(_));
        let is_section = matches!(
            entity,
            EntityType::Extended(extended) if matches!(
                extended.data,
                document::entities::ExtendedEntityData::SectionObject(_)
            )
        );
        if self.meshes.contains_key(&handle)
            || self.block_meshes.contains_key(&handle)
            || is_insert
            || is_section
        {
            bits |= CACHE_CATEGORY_MESH | CACHE_CATEGORY_INTERACTION;
        }
        if is_insert {
            bits |= CACHE_CATEGORY_INSERT_HATCH;
        }
        if self.entity_affects_text_cache(entity) {
            bits |= CACHE_CATEGORY_TEXT;
        }
        self.pending_removed_categories
            .borrow_mut()
            .insert(handle, bits);
    }

    /// True when a per-category derived cache synced at `cached_epoch` remains
    /// valid. Adds/edits are classified from live state; removals use the tiny
    /// pre-erase category mask stored in the journal. Thus deleting a LINE no
    /// longer rebuilds every hatch/image/mesh/text cache merely because its type
    /// became unknowable after removal.
    /// Whether `handle` draws as an image. A modified image whose model was
    /// dropped (an underlay turned off) is still one: the cached image list
    /// that holds it must be rebuilt (#2839).
    pub(crate) fn is_image_entity(&self, handle: Handle) -> bool {
        self.images.contains_key(&handle)
            || matches!(
                self.document.get_entity(handle),
                Some(
                    EntityType::RasterImage(_) | EntityType::Ole2Frame(_) | EntityType::Underlay(_)
                )
            )
    }

    pub(crate) fn category_cache_valid(
        &self,
        cached_epoch: u64,
        category: u16,
        in_category: impl Fn(Handle) -> bool,
    ) -> bool {
        if cached_epoch == self.geometry_epoch {
            return true;
        }
        if cached_epoch < self.geometry_journal_floor.get() {
            return false;
        }
        let ring = self.geometry_deltas.borrow();
        for delta in ring.iter().filter(|delta| delta.epoch > cached_epoch) {
            if delta.full {
                return false;
            }
            for &(handle, kind) in &delta.changes {
                match kind {
                    ChangeKind::Removed => {
                        let Some(bits) = delta.removed_categories.get(&handle) else {
                            return false;
                        };
                        if bits & category != 0 {
                            return false;
                        }
                    }
                    ChangeKind::Added | ChangeKind::Modified if in_category(handle) => {
                        return false;
                    }
                    ChangeKind::Added | ChangeKind::Modified => {}
                }
            }
        }
        true
    }

    /// Whether the per-entity draw-order labels can be replayed since
    /// `last_epoch`. Add/Remove keep every existing label stable; only a full
    /// structural delta (DRAWORDER, file/layout/block rebuild, journal overflow)
    /// requires consumers to regenerate their depth-bearing data.
    pub(crate) fn draw_ranks_stable(&self, last_epoch: u64) -> bool {
        self.replay_since(last_epoch).is_some()
    }

    /// True when no text-bearing entity changed since `last_epoch`, so cached SDF
    /// glyphs stay valid. Text comes from Text / MText / Dimension / MultiLeader /
    /// Leader / Table / Tolerance / attributes (incl. ATTDEF), from block
    /// references (their baked text moves with the instance), and from a raster
    /// image whose reference cannot be resolved (it draws the saved path in place
    /// of the picture) — an edit or removal of any of those invalidates it; a
    /// plain line / arc / polyline edit does not. See
    /// [`Scene::entity_affects_text_cache`], which classifies both directions:
    /// live entities for an add/edit, the pre-erase category mask for a removal.
    pub(crate) fn text_unchanged(&self, last_epoch: u64) -> bool {
        self.category_cache_valid(last_epoch, CACHE_CATEGORY_TEXT, |handle| {
            self.document
                .get_entity(handle)
                .is_some_and(|entity| self.entity_affects_text_cache(entity))
        })
    }

    /// Report the exact handles a mutation changed. Folds the memo drop and the
    /// epoch bump so a call site can't bump geometry without recording what
    /// changed — the delta lets every derived cache patch per-handle instead of
    /// re-walking the whole drawing. Blocks are untouched (`block_epoch` stays),
    /// so this is for top-level entity add / edit / erase, not block-defn edits.
    /// Begin capturing before-images for a delta-undo entry. Pair with
    /// [`Scene::take_undo_recording`]. Only entity-only commands — whose
    /// mutations flow through the five primitives (add / update / erase /
    /// transform / copy) and that touch no layers / objects / block records —
    /// may use this; anything else must fall back to a full history snapshot.
    pub fn begin_undo_recording(&mut self) {
        self.undo_recording = Some(UndoRecording::default());
    }

    /// Whether a delta-undo recording is currently open.
    pub fn is_recording_undo(&self) -> bool {
        self.undo_recording.is_some()
    }

    /// Finish and return the open recording (if any) for the app to build a
    /// history delta from.
    pub fn take_undo_recording(&mut self) -> Option<UndoRecording> {
        self.undo_recording.take()
    }

    /// Record the pre-mutation image of `handle` (first touch wins). `before`
    /// is `None` for a freshly added entity. No-op when not recording — the
    /// callers guard with [`Scene::is_recording_undo`] so the clone is skipped
    /// entirely on the common (no-recording) path.
    #[doc(hidden)]
    pub fn record_undo_before(&mut self, handle: Handle, before: Option<Arc<EntityType>>) {
        if let Some(rec) = self.undo_recording.as_mut() {
            if !rec.before.contains_key(&handle) {
                rec.order.push(handle);
                rec.before.insert(handle, before);
            }
        }
    }

    /// Record one document.objects entry before its first mutation. This keeps
    /// Group erase and RasterImage add on targeted history rather than forcing
    /// a clone of all document structure.
    pub(crate) fn record_undo_object_before(&mut self, handle: Handle, before: Option<ObjectType>) {
        if let Some(rec) = self.undo_recording.as_mut() {
            if !rec.object_before.contains_key(&handle) {
                rec.object_order.push(handle);
                rec.object_before.insert(handle, before);
            }
        }
    }

    /// Record one parametric-constraint scope's whole-set image before its first
    /// mutation within the open recording (first touch wins) — e.g. ERASE
    /// about to remove some of a scope's constraints via
    /// `refresh_parametric_constraints`'s deletion policy. `parametric_constraints`
    /// lives on `Scene`, outside `document`/`document.objects`, so neither
    /// `record_undo_before` nor `record_undo_object_before` ever sees this
    /// change on their own.
    pub(crate) fn record_undo_parametric_constraints_before(
        &mut self,
        scope: parametric_constraints::ParametricScope,
        before: parametric_constraints::ParametricConstraintSet,
    ) {
        if let Some(rec) = self.undo_recording.as_mut() {
            if !rec.parametric_constraints_before.contains_key(&scope) {
                rec.parametric_constraints_order.push(scope);
                rec.parametric_constraints_before.insert(scope, before);
            }
        }
    }

    /// Records the drawing-wide parameter table before its first mutation in
    /// the current undo transaction.
    pub(crate) fn record_undo_named_parameters_before(&mut self) {
        let before = self.named_parameters.clone();
        if let Some(recording) = self.undo_recording.as_mut() {
            recording.named_parameters_before.get_or_insert(before);
        }
    }

    /// Record a reference-only designation's value before its first change
    /// within the open delta recording (API v9). The designation set lives on
    /// `Scene`, outside the document, so it needs its own channel — like
    /// the parametric-constraint directory above — for undo to restore the
    /// designation together with the entity it belongs to. First touch wins.
    pub(crate) fn record_undo_reference_designation_before(&mut self, handle: Handle) {
        if let Some(recording) = self.undo_recording.as_mut() {
            if !recording
                .reference_designations_before
                .iter()
                .any(|(recorded, _)| *recorded == handle)
            {
                let before = self.reference_only.is_marked(handle);
                recording
                    .reference_designations_before
                    .push((handle, before));
            }
        }
    }

    /// Flag the open recording as touching non-entity state (a new layer, a
    /// group edit, a `*D` block record) that a pure-entity delta cannot
    /// restore. The app's per-command predicate keeps this from firing.
    pub(crate) fn poison_undo_recording(&mut self) {
        if let Some(rec) = self.undo_recording.as_mut() {
            rec.poisoned = true;
        }
    }

    /// Re-apply one side of an entity delta to the document. For each
    /// `(handle, before, after)`, install the chosen `target` — `before` when
    /// `undo`, `after` on redo: overwrite the entity in place, re-insert it with
    /// its original handle, or remove it. Returns the exact per-handle change
    /// list; derived-cache reseeding and the geometry bump are deferred so a
    /// multi-step undo/redo can process each final handle only once.
    pub(crate) fn apply_entity_delta(
        &mut self,
        entities: &[(Handle, Option<Arc<EntityType>>, Option<Arc<EntityType>>)],
        undo: bool,
    ) -> Vec<(Handle, ChangeKind)> {
        if entities.iter().any(|(_, before, after)| {
            before
                .as_deref()
                .is_some_and(|entity| matches!(entity, EntityType::Light(_)))
                || after
                    .as_deref()
                    .is_some_and(|entity| matches!(entity, EntityType::Light(_)))
        }) {
            self.lighting_cache.borrow_mut().clear();
        }
        let mut changes: Vec<(Handle, ChangeKind)> = Vec::with_capacity(entities.len());
        for (h, before, after) in entities {
            let target = if undo { before } else { after };
            let existed = self.document.get_entity(*h).is_some();
            match target {
                Some(ent) => {
                    if existed {
                        let _ = self.document.replace_entity_arc(*h, Arc::clone(ent));
                        changes.push((*h, ChangeKind::Modified));
                    } else {
                        // Removal keeps the original block membership in place;
                        // restoring only the flat storage avoids an O(all block
                        // members) scan and cannot duplicate the owner link.
                        let _ = self.document.restore_entity_arc(Arc::clone(ent));
                        changes.push((*h, ChangeKind::Added));
                    }
                }
                None => {
                    if existed {
                        self.remember_removed_cache_categories(*h);
                        self.document.remove_entity_arc(*h);
                        changes.push((*h, ChangeKind::Removed));
                    }
                }
            }
        }

        // History replay can apply an entity removal the designation channels
        // predate (an "added" side rolled back). Sweep so the set never keeps
        // an entry for an absent entity; exact designations come right after
        // this from the delta's own channel.
        if !self.reference_only.is_empty() {
            self.reference_only.retain_present(&self.document);
        }
        if !changes.is_empty() {
            self.invalidate_dependency_index();
        }

        changes
    }

    /// Check changed entities and scan the document only when the hint is unknown.
    pub(crate) fn associative_centers_possible(&self, changes: &[(Handle, ChangeKind)]) -> bool {
        if changes.iter().any(|(handle, _)| {
            self.document.get_entity(*handle).is_some_and(|entity| {
                crate::scene::centerline::entity_has_center_association(entity)
                    || crate::scene::centermark::entity_has_center_association(entity)
            })
        }) {
            self.has_associative_centers.set(Some(true));
            return true;
        }
        if let Some(known) = self.has_associative_centers.get() {
            return known;
        }
        // Undetermined: settle it with one pass over the drawing, then never
        // again for this document.
        let any = self.document.entities().any(|entity| {
            crate::scene::centerline::entity_has_center_association(entity)
                || crate::scene::centermark::entity_has_center_association(entity)
        });
        self.has_associative_centers.set(Some(any));
        any
    }

    /// Fast check for whether any OLE2FRAME entities exist in the document.
    pub(crate) fn has_ole_frames(&self) -> bool {
        if let Some(known) = self.has_ole_frames.get() {
            return known;
        }
        let any = self
            .document
            .entities()
            .any(|e| matches!(e, EntityType::Ole2Frame(_)));
        self.has_ole_frames.set(Some(any));
        any
    }

    /// Fast check for whether any Underlay entities exist in the document.
    pub(crate) fn has_underlays(&self) -> bool {
        if let Some(known) = self.has_underlays.get() {
            return known;
        }
        let any = self
            .document
            .entities()
            .any(|e| matches!(e, EntityType::Underlay(_)));
        self.has_underlays.set(Some(any));
        any
    }

    /// Fast check for whether any section object exists in the document.
    pub(crate) fn has_section_objects(&self) -> bool {
        if let Some(known) = self.has_section_objects.get() {
            return known;
        }
        let any = self.document.entities().any(is_section_object);
        self.has_section_objects.set(Some(any));
        any
    }

    /// Fast check for whether any table exists in the document.
    pub(crate) fn has_tables(&self) -> bool {
        if let Some(known) = self.has_tables.get() {
            return known;
        }
        let any = self
            .document
            .entities()
            .any(|e| matches!(e, EntityType::Table(_)));
        self.has_tables.set(Some(any));
        any
    }

    /// Fast check for whether any Face3D entities exist in the document.
    pub(crate) fn has_face3d(&self) -> bool {
        if let Some(known) = self.has_face3d.get() {
            return known;
        }
        let any = self
            .document
            .entities()
            .any(|e| matches!(e, EntityType::Face3D(_)));
        self.has_face3d.set(Some(any));
        any
    }

    /// Whether a table changed since the drawing was opened or last saved.
    pub(crate) fn table_modified_since_save(&self, handle: Handle) -> bool {
        self.tables_modified.contains(&handle)
    }

    /// After a save: the tables it marked out of date keep up-to-date cell
    /// geometry from now on (flag 3) until they change again (measured: the
    /// first save after a change writes 7, the next ones 3; a table no save
    /// has touched keeps the flag it was read with).
    pub(crate) fn note_tables_saved(&mut self) {
        for handle in std::mem::take(&mut self.tables_modified) {
            if let Some(EntityType::Table(table)) = self.document.get_entity_mut(handle) {
                for cell in table.rows.iter_mut().flat_map(|row| row.cells.iter_mut()) {
                    cell.geometry_data_flag = 3;
                }
            }
        }
    }

    /// Settles a changed table (typed values, row heights) and removes the
    /// fields it lists but no cell shows any more, recording the removed
    /// objects for undo.
    pub(crate) fn prune_table_fields(&mut self, changes: &[(Handle, ChangeKind)]) {
        for (table, kind) in changes {
            if !matches!(kind, ChangeKind::Modified) || !self.table_cells_rewritten(*table) {
                continue;
            }
            // Typed text read as the cells' data types and rows grown to
            // their wrapped text, whichever path wrote the cells.
            if let Some(EntityType::Table(current)) = self.document.get_entity(*table) {
                let mut next = (**current).clone();
                if crate::entities::table::settle_table(&mut next, &self.document) {
                    next.block_record_handle = None;
                    if let Some(EntityType::Table(target)) = self.document.get_entity_mut(*table) {
                        **target = next;
                    }
                }
            }
            let doomed = self.document.orphan_table_fields(*table);
            if doomed.is_empty() {
                continue;
            }
            if self.is_recording_undo() {
                for handle in &doomed {
                    let before = self.document.objects.get(handle).cloned();
                    self.record_undo_object_before(*handle, before);
                }
                let lists: Vec<(Handle, document::objects::ObjectType)> = self
                    .document
                    .objects
                    .iter()
                    .filter(|(_, object)| {
                        matches!(object, document::objects::ObjectType::FieldList(_))
                    })
                    .map(|(handle, object)| (*handle, object.clone()))
                    .collect();
                for (handle, before) in lists {
                    self.record_undo_object_before(handle, Some(before));
                }
            }
            self.document.prune_table_fields(*table);
        }
    }

    /// Whether a modified table had its cells or layout rewritten, not only
    /// moved, turned or scaled: its rows or columns differ from the image the
    /// open undo recording keeps; without a recording, the edit dropped the
    /// table's saved picture. A table read from a file and only moved stays
    /// as the file has it.
    pub(crate) fn table_cells_rewritten(&self, handle: Handle) -> bool {
        let Some(EntityType::Table(table)) = self.document.get_entity(handle) else {
            return false;
        };
        match self.undo_recording.as_ref() {
            Some(recording) => match recording.before.get(&handle) {
                Some(Some(before)) => match before.as_ref() {
                    EntityType::Table(before) => {
                        before.rows != table.rows || before.columns != table.columns
                    }
                    _ => true,
                },
                _ => false,
            },
            None => table.block_record_handle.is_none(),
        }
    }

    pub fn bump_entities(&mut self, changes: &[(Handle, ChangeKind)]) {
        self.bump_entities_with_parametric_policy(changes, &[], false);
    }

    /// Update the fields an evaluation event re-evaluates (see
    /// [`crate::entities::field::update_fields`]) and redraw the hosts whose
    /// text changed. Returns the number of hosts whose text changed and the
    /// number of fields the hosts hold.
    pub fn update_fields(&mut self, event: i32, hosts: Option<&[Handle]>) -> (usize, usize) {
        let (changed, found) =
            crate::entities::field::update_fields(&mut self.document, event, hosts);
        if !changed.is_empty() {
            self.bump_text_hosts(&changed);
        }
        (changed.len(), found)
    }

    /// Re-evaluate fields reading drawing/custom properties (`acvar`).
    pub fn update_summary_field_hosts(&mut self) {
        let hosts = crate::entities::field::acvar_hosts(&self.document);
        if !hosts.is_empty() {
            self.update_fields(32, Some(&hosts));
        }
    }

    /// Redraw text hosts whose stored text changed (re-evaluated fields). An
    /// attribute is drawn by its INSERT, and a host inside a block definition
    /// by every reference to that block, so those redraw instead of the host;
    /// nothing else re-tessellates.
    pub fn bump_text_hosts(&mut self, hosts: &[Handle]) {
        if hosts.is_empty() {
            return;
        }
        if self.dependency_index_cache.borrow().is_none() {
            *self.dependency_index_cache.borrow_mut() =
                Some(Arc::new(self.rebuild_dependency_index()));
        }
        // One pass over the drawing for all hosts: which block definition
        // holds an entity, which INSERT draws an attribute, which tables
        // draw from a block.
        let layout_blocks: HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Layout(layout) if !layout.block_record.is_null() => {
                    Some(layout.block_record)
                }
                _ => None,
            })
            .collect();
        let mut definitions: HashMap<Handle, (Handle, String)> = HashMap::default();
        let mut members: HashMap<Handle, Handle> = HashMap::default();
        for record in self.document.block_records.iter() {
            let name = record.name.to_ascii_uppercase();
            if layout_blocks.contains(&record.handle)
                || name.starts_with("*MODEL_SPACE")
                || name.starts_with("*PAPER_SPACE")
            {
                continue;
            }
            definitions.insert(record.handle, (record.handle, normalize_name(&record.name)));
            for member in &record.entity_handles {
                members.insert(*member, record.handle);
            }
        }
        let mut attribute_owner: HashMap<Handle, Handle> = HashMap::default();
        let mut tables_of: HashMap<Handle, Vec<Handle>> = HashMap::default();
        for entity in self.document.entities() {
            match entity {
                EntityType::Insert(insert) => {
                    for attribute in &insert.attributes {
                        attribute_owner.insert(attribute.common.handle, insert.common.handle);
                    }
                }
                EntityType::Table(table) => {
                    if let Some(record) = table.block_record_handle {
                        tables_of
                            .entry(record)
                            .or_default()
                            .push(table.common.handle);
                    }
                }
                _ => {}
            }
        }
        let mut targets = DependencyTargets::default();
        for &host in hosts {
            // An attribute is drawn by its INSERT.
            let handle = match self.document.get_entity(host) {
                Some(_) => host,
                None => match attribute_owner.get(&host) {
                    Some(insert) => *insert,
                    None => continue,
                },
            };
            let owner = self
                .document
                .get_entity(handle)
                .map(|e| e.common().owner_handle);
            let record = owner.and_then(|owner| definitions.get(&owner)).or_else(|| {
                members
                    .get(&handle)
                    .and_then(|record| definitions.get(record))
            });
            let Some((record_handle, name)) = record else {
                targets.render_handles.insert(handle);
                continue;
            };
            // A host inside a block definition is drawn by every reference
            // to that block, and a table by its own block of cell texts.
            if let Some(index) = self.dependency_index_cache.borrow().as_ref() {
                if let Some(users) = index.block_roots.get(name) {
                    targets.render_handles.extend(users.iter().copied());
                }
            }
            targets
                .render_handles
                .extend(tables_of.get(record_handle).into_iter().flatten().copied());
            targets.touches_block_definition = true;
        }
        self.invalidate_dependency_targets(targets);
    }

    pub fn bump_entities_with_parametric_driven(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[parametric_constraints::ParametricRef],
    ) {
        self.bump_entities_with_parametric_policy(changes, driven_refs, false);
    }

    pub fn bump_entities_with_parametric_policy(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[parametric_constraints::ParametricRef],
        retain_size: bool,
    ) {
        self.bump_entities_with_parametric_originals(changes, driven_refs, retain_size, &[]);
    }

    pub fn bump_entities_with_parametric_originals(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[parametric_constraints::ParametricRef],
        retain_size: bool,
        retained_originals: &[(Handle, EntityType)],
    ) {
        self.bump_entities_with_solve_policy(
            changes,
            driven_refs,
            retain_size,
            retained_originals,
            &[],
            true,
            false,
            None,
        );
    }

    /// Apply a newly-created relation. A constraint handler names every
    /// referenced entity `Modified` before the solve runs, so the delta is
    /// filtered against the pre-solve copies: only geometry that actually
    /// moved is published. `pre_move` carries the copies a handler took
    /// before it nudged geometry onto the relation itself; an empty slice
    /// makes the copies here, which is right for a handler that wrote none.
    pub fn bump_entities_after_constraint_added(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[parametric_constraints::ParametricRef],
        retain_size: bool,
        pre_move: &[(Handle, EntityType)],
    ) {
        self.bump_entities_with_solve_policy(
            changes,
            driven_refs,
            retain_size,
            &[],
            &[],
            true,
            false,
            Some(pre_move),
        );
    }

    /// Refresh display and associations after a solved grip or exact history restore.
    pub(crate) fn bump_entities_after_parametric_solve(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) {
        self.bump_entities_with_solve_policy(changes, &[], false, &[], &[], false, false, None);
    }

    /// Mark a set of block definition names dirty so the next `BlockCache`
    /// rebuild refreshes only those definitions (and drops removed ones)
    /// while keeping all unrelated cached blocks warm.
    pub(crate) fn mark_block_definitions_dirty<I: IntoIterator<Item = String>>(
        &mut self,
        names: I,
    ) {
        let touched: HashSet<String> = names.into_iter().filter(|n| !n.is_empty()).collect();
        if touched.is_empty() {
            return;
        }
        let from = self.block_epoch;
        self.block_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        // Switches made before the cache is rebuilt add up into one edit
        // from the cache's epoch. Once a cache was rebuilt at `from`, the
        // edit starts there: carrying the older base along made every
        // later switch miss it and rebuild all the definitions.
        let rebuilt_at_from = self
            .block_defn_cache
            .borrow()
            .values()
            .any(|(epoch, _)| *epoch == from);
        self.block_visibility_edits = match self.block_visibility_edits.take() {
            Some((base, to, mut existing)) if to == from && !rebuilt_at_from => {
                existing.extend(touched);
                Some((base, self.block_epoch, existing))
            }
            _ => Some((from, self.block_epoch, touched)),
        };
    }

    /// Incrementally invalidate a single block definition's cached display
    /// and the `Insert` reference that owns it (used when switching a dynamic
    /// block's visibility state), keeping all other blocks and entities cached.
    pub fn bump_block_visibility(&mut self, insert_handle: Handle, block_name: &str) {
        self.mark_block_definitions_dirty([block_name.to_string()]);
        self.bump_entities_restyled(&[(insert_handle, ChangeKind::Modified)]);
    }

    /// Incrementally invalidate changed entities together with any block
    /// definitions they belong to (plus `extra_block_handles` / `extra_block_names`)
    /// and all dependent root `Insert` references (including transitive nesting
    /// parents and open-BEDIT nested references), keeping unrelated block
    /// definitions, tessellation memos, and resident caches warm.
    pub(crate) fn bump_entities_with_block_deps(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        extra_block_handles: &[Handle],
        extra_block_names: &[String],
        solve_parametric: bool,
    ) {
        self.bump_entities_with_block_deps_inner(
            changes,
            extra_block_handles,
            extra_block_names,
            solve_parametric,
            false,
        );
    }

    /// Variant of [`Self::bump_entities_with_block_deps`] that skips native
    /// constraint-graph synchronization (matching `bump_geometry()` for callers
    /// like `save_block_edit_as` that already copied/preserved native graphs).
    pub(crate) fn bump_entities_restyled_with_block_deps(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        extra_block_handles: &[Handle],
        extra_block_names: &[String],
    ) {
        self.bump_entities_with_block_deps_inner(
            changes,
            extra_block_handles,
            extra_block_names,
            false,
            true,
        );
    }

    pub(crate) fn bump_entities_with_block_deps_inner(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        extra_block_handles: &[Handle],
        extra_block_names: &[String],
        solve_parametric: bool,
        appearance_only: bool,
    ) {
        let touched_names =
            self.touched_block_definition_names(changes, extra_block_handles, extra_block_names);
        if touched_names.is_empty() {
            if !changes.is_empty() {
                self.bump_entities_with_solve_policy_inner(
                    changes,
                    &[],
                    false,
                    &[],
                    &[],
                    solve_parametric,
                    appearance_only,
                    !appearance_only,
                    None,
                );
            }
            return;
        }
        let dependent_handles = self.collect_block_dependent_handles(&touched_names);
        let mut all_changes = changes.to_vec();
        let mut seen: HashSet<Handle> = all_changes.iter().map(|(h, _)| *h).collect();
        for dep_handle in dependent_handles {
            if seen.insert(dep_handle) && self.document.get_entity(dep_handle).is_some() {
                all_changes.push((dep_handle, ChangeKind::Modified));
            }
        }
        self.mark_block_definitions_dirty(touched_names);
        self.bump_entities_with_solve_policy_inner(
            &all_changes,
            &[],
            false,
            &[],
            &[],
            solve_parametric,
            appearance_only,
            !appearance_only,
            None,
        );
    }

    /// Incrementally invalidate a single block definition record (e.g., after
    /// placing or editing dynamic-block parameters inside BEDIT) along with any
    /// root `Insert` references to it.
    pub(crate) fn bump_block_definition(&mut self, block_handle: Handle) {
        self.bump_entities_with_block_deps(&[], &[block_handle], &[], false);
    }

    #[cfg(test)]
    pub(crate) fn has_resident_tess_memo(&self, handle: Handle) -> bool {
        self.resident_tess_memo.borrow().contains_key(&handle)
    }

    #[cfg(test)]
    pub(crate) fn active_block_cache_arc(&self) -> Arc<cache::block_cache::BlockCache> {
        let scale = crate::scene::annotative::scale_handle_by_name(
            &self.document,
            &self.document.header.current_annotation_scale,
        );
        self.block_cache_arc_for(scale, self.annotation_all_visible(), self.active_viewport)
    }

    /// Refresh display after a restyle that moved no geometry: nothing
    /// associated to these entities is re-resolved or re-solved.
    pub(crate) fn bump_entities_restyled(&mut self, changes: &[(Handle, ChangeKind)]) {
        self.bump_entities_with_solve_policy(changes, &[], false, &[], &[], false, true, None);
    }

    /// Apply a newly-created ordered relation while temporarily anchoring
    /// its reference side. The anchors guide only this first solve and are
    /// never added to the persistent constraint graph. `pre_move` carries
    /// the copies a handler took before it nudged geometry onto the relation
    /// itself, so the delta still names that geometry.
    pub fn bump_entities_with_initial_parametric_policy(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        fixed_refs: &[parametric_constraints::ParametricRef],
        retain_size: bool,
        pre_move: &[(Handle, EntityType)],
    ) {
        self.bump_entities_with_solve_policy(
            changes,
            &[],
            retain_size,
            &[],
            fixed_refs,
            true,
            false,
            Some(pre_move),
        );
    }

    pub fn bump_entities_with_parametric_transform_policy(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[parametric_constraints::ParametricRef],
        transformed_refs: &[parametric_constraints::ParametricRef],
        retained_originals: &[(Handle, EntityType)],
    ) {
        self.bump_entities_with_solve_policy(
            changes,
            driven_refs,
            true,
            retained_originals,
            transformed_refs,
            true,
            false,
            None,
        );
    }

    pub(crate) fn bump_entities_with_solve_policy(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[parametric_constraints::ParametricRef],
        retain_size: bool,
        retained_originals: &[(Handle, EntityType)],
        fixed_refs: &[parametric_constraints::ParametricRef],
        solve_parametric: bool,
        appearance_only: bool,
        report_before: Option<&[(Handle, EntityType)]>,
    ) {
        self.bump_entities_with_solve_policy_inner(
            changes,
            driven_refs,
            retain_size,
            retained_originals,
            fixed_refs,
            solve_parametric,
            appearance_only,
            true,
            report_before,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bump_entities_with_solve_policy_inner(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[parametric_constraints::ParametricRef],
        retain_size: bool,
        retained_originals: &[(Handle, EntityType)],
        fixed_refs: &[parametric_constraints::ParametricRef],
        solve_parametric: bool,
        appearance_only: bool,
        sync_native_graph: bool,
        report_before: Option<&[(Handle, EntityType)]>,
    ) {
        // A solve can name an entity it then leaves untouched — a neighbour
        // that already satisfied the relation, or the datum the solver
        // anchors. When the caller says its list is speculative, keep the
        // pre-solve copies so the published delta names only geometry that
        // actually moved; every refresh below still sees the full list,
        // because they have to run whether or not the entity ends up
        // modified.
        let reported_before: Vec<(Handle, EntityType)> = match report_before {
            Some(pre_move) => changes
                .iter()
                .filter(|(_, kind)| matches!(kind, ChangeKind::Modified))
                .filter_map(|(handle, _)| {
                    let snapshot = pre_move
                        .iter()
                        .find(|(h, _)| h == handle)
                        .map(|(_, entity)| entity.clone())
                        .or_else(|| self.document.get_entity(*handle).cloned())?;
                    Some((*handle, snapshot))
                })
                .collect(),
            None => Vec::new(),
        };
        // A cell rewritten over a field (typed text, a copied cell, a removed
        // row or column) no longer shows it; the field objects go with it.
        self.prune_table_fields(changes);
        for (handle, kind) in changes {
            if !matches!(kind, ChangeKind::Removed)
                && matches!(
                    self.document.get_entity(*handle),
                    Some(EntityType::Table(_))
                )
            {
                self.tables_modified.insert(*handle);
            }
        }
        if changes.iter().any(|(handle, kind)| {
            matches!(kind, ChangeKind::Removed)
                || self
                    .document
                    .get_entity(*handle)
                    .is_some_and(|entity| matches!(entity, EntityType::Hatch(_)))
        }) {
            self.associative_hatch_source_cache.borrow_mut().take();
        }
        // A pasted, duplicated or restored carrier arrives with its
        // association xdata already attached, and an erased one takes its
        // edges with it; neither names a line as modified, so the centre
        // indexes drop here and rebuild on the next associated edit. Lazy:
        // an edit with nothing to refresh never pays for the rebuild.
        if changes
            .iter()
            .any(|(_, kind)| matches!(kind, ChangeKind::Added | ChangeKind::Removed))
        {
            self.associative_centerline_source_cache.borrow_mut().take();
            self.associative_centermark_source_cache.borrow_mut().take();
        }
        let bump_started = crate::perf::enabled().then(iced::time::Instant::now);
        let bump_mark = |stage: &str| {
            if let Some(started) = bump_started {
                crate::perf_record!(
                    "[perf] bump {stage}={:.1}ms",
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
        };
        let mut changes = changes.to_vec();
        // A table whose content this app changed (its picture reference
        // dropped by the edit, or a new table without one) has its drawing
        // (the *T block) redrawn from the cells. Tables keeping their picture
        // (moved, restyled, opened from a file) are never redrawn: the block
        // may hold authored content the redraw does not reproduce.
        // Its cell formulas are worked out again into that block.
        if !appearance_only {
            let mut rebuilt = false;
            for (handle, kind) in &changes {
                if !matches!(kind, ChangeKind::Removed)
                    && matches!(
                        self.document.get_entity(*handle),
                        Some(EntityType::Table(t)) if t.block_record_handle.is_none()
                    )
                {
                    // Cell edits request this rebuild by dropping the handle.
                    // The codec then refills the block by name: drop the name
                    // too when another table still uses that block.
                    let name = match self.document.get_entity(*handle) {
                        Some(EntityType::Table(t)) => t.block_name.clone(),
                        _ => String::new(),
                    };
                    let from_cells = !name.is_empty();
                    let named = self
                        .document
                        .block_records
                        .get(&name)
                        .filter(|_| from_cells)
                        .map(|record| record.handle);
                    let shared = from_cells
                        && self.document.entities().any(|e| {
                            e.common().handle != *handle
                                && matches!(e, EntityType::Table(o)
                                    if o.block_name.eq_ignore_ascii_case(&name)
                                        || (named.is_some() && o.block_record_handle == named))
                        });
                    if shared {
                        if let Some(EntityType::Table(t)) = self.document.get_entity_mut(*handle) {
                            t.block_name.clear();
                        }
                    }
                    // A fresh picture adds block records and entities outside
                    // the table's entity delta; undo needs the full snapshot.
                    if name.is_empty() || shared {
                        self.poison_undo_recording();
                    }
                    rebuilt |= self.document.refresh_table_block(*handle);
                    rebuilt |= !crate::entities::field::refresh_table_formulas(
                        &mut self.document,
                        *handle,
                    )
                    .is_empty();
                    // The fresh picture stores each cell's raw text; a formula
                    // cell's picture must carry its computed value instead.
                    rebuilt |= !crate::entities::table::refresh_picture_formulas(
                        &mut self.document,
                        *handle,
                    )
                    .is_empty();
                    // A reader may rebuild the table from its TABLECONTENT
                    // copy: keep the copy in step, with its old value on record.
                    if let Some((content, before)) =
                        table_content::sync_table_content_object(&mut self.document, *handle)
                    {
                        self.record_undo_object_before(content, Some(before));
                    }
                    // A table that had let go of its picture (no handle, the
                    // name kept) is drawn from its cells. It stays so when the
                    // picture had to be made anew, as for a copy of such a
                    // table: the copy then shows its formulas, fields and
                    // wrapping as its source does. The picture stays, found
                    // by name, for saving.
                    if from_cells {
                        if let Some(EntityType::Table(t)) = self.document.get_entity_mut(*handle) {
                            t.block_record_handle = None;
                        }
                    }
                }
            }
            // The block's cached drawing is stale once its texts change.
            if rebuilt {
                self.block_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
            }
        }
        // A restyle (layer on/off, colour, text or dimension style) moves no
        // geometry, so nothing measured from it is re-resolved: an
        // association that disagrees with its dimension would otherwise
        // re-place the dimension on a layer toggle.
        if !appearance_only {
            // One membership set shared by every associative family below, so a
            // re-solved handle is checked once instead of rescanning the
            // growing change list per returned handle (O(changes x refreshed)
            // on a bulk edit). Hydrated lazily: an edit that refreshes nothing
            // — the common case — never builds it. Families still run in the
            // same order and see the same accumulated list.
            let mut refreshed_handles: Option<HashSet<Handle>> = None;
            if self.associative_centers_possible(&changes) {
                self.note_center_disassociations(&changes);
                let refreshed = self.refresh_associative_centerlines(&changes);
                Self::absorb_refreshed(&mut changes, &mut refreshed_handles, refreshed);
                let refreshed = self.refresh_associative_center_marks(&changes);
                Self::absorb_refreshed(&mut changes, &mut refreshed_handles, refreshed);
            }
            bump_mark("before-dimensions");
            let refreshed = self.refresh_associative_dimensions(&changes);
            Self::absorb_refreshed(&mut changes, &mut refreshed_handles, refreshed);
            bump_mark("dimensions");
            let refreshed = self.refresh_associative_hatches(&changes);
            Self::absorb_refreshed(&mut changes, &mut refreshed_handles, refreshed);
            bump_mark("hatches");
            let refreshed = self.refresh_associative_sweeps(&changes);
            Self::absorb_refreshed(&mut changes, &mut refreshed_handles, refreshed);
            bump_mark("sweeps");
            let refreshed = self.refresh_associative_surfaces(&changes);
            Self::absorb_refreshed(&mut changes, &mut refreshed_handles, refreshed);
            bump_mark("surfaces");
            let refreshed = self.refresh_associative_path_arrays(&changes);
            Self::absorb_refreshed(&mut changes, &mut refreshed_handles, refreshed);
            bump_mark("path-arrays");
            if solve_parametric && !self.parametric_constraints.is_empty() {
                let refreshed = self.refresh_parametric_constraints_with_initial_policy(
                    &changes,
                    driven_refs,
                    retain_size,
                    retained_originals,
                    fixed_refs,
                );
                // The solve records everything it moved, including geometry
                // the caller named. Only a constraint addition reports the
                // rest; every other solve swallows the record.
                let moved = std::mem::take(&mut self.solve_moved);
                if report_before.is_some() {
                    // The refreshes above may have appended annotation
                    // handles to `changes`; those are never solved, so
                    // counting them as named changes nothing.
                    let named: HashSet<Handle> =
                        changes.iter().map(|(handle, _)| *handle).collect();
                    self.constraint_network_moves
                        .extend(moved.into_iter().filter(|handle| !named.contains(handle)));
                }
                Self::absorb_refreshed(&mut changes, &mut refreshed_handles, refreshed);
            }
        }
        if sync_native_graph
            && (!self.parametric_constraints.is_empty() || !self.named_parameters.is_empty())
        {
            self.refresh_dynamic_dimension_texts();
            // The stored associative graph rebuilds only when the constraint
            // model actually changed since the last sync — never for a pure
            // geometry edit. Nothing in the session reads the graph between
            // edits: glyph placement and the solver run off the runtime
            // constraint sets, and every consumer of the stored graph —
            // save/export, parameter and constraint commands, undo replay —
            // flushes with `sync_native_parametric_graph` itself. Constraint
            // commands mutate the runtime sets with no explicit flush of
            // their own, so hashing the model here is what still syncs an
            // add/remove/toggle/value edit on this same bump. The refresh
            // runs first because it can rewrite a reference constraint's
            // parameter, which is itself graph input.
            let fingerprint = self.constraint_model_fingerprint();
            bump_mark("fingerprint");
            if self.native_graph_fingerprint != Some(fingerprint) {
                self.sync_native_parametric_graph();
                bump_mark("native-graph-sync");
            }
        }
        // A modified image-bearing entity redraws its raster: an underlay's
        // display adjustments and clip live on the entity, not the image.
        for (handle, kind) in &changes {
            // A raster a worker just drew (`apply_reseed`) is not drawn again.
            if !matches!(kind, ChangeKind::Modified) || self.fresh_images.contains(handle) {
                continue;
            }
            let seed = match self.document.get_entity(*handle) {
                Some(
                    entity @ (EntityType::RasterImage(_)
                    | EntityType::Ole2Frame(_)
                    | EntityType::Underlay(_)),
                ) => Some(self.image_seed_for(entity)),
                _ => None,
            };
            if let Some(seed) = seed {
                self.images.remove(handle);
                if let Some(model) = seed {
                    self.images.insert(*handle, model);
                }
            }
        }
        bump_mark("before-dependency-index");
        if !changes.is_empty() {
            self.refresh_dependency_index_for_changes(&changes);
        }
        bump_mark("dependency-index");
        let cached_light_changed = self.lighting_cache.borrow().values().any(|lights| {
            changes
                .iter()
                .any(|(handle, _)| lights.iter().any(|light| light.handle == *handle))
        });
        let live_light_changed = changes.iter().any(|(handle, _)| {
            self.document
                .get_entity(*handle)
                .is_some_and(|entity| matches!(entity, EntityType::Light(_)))
        });
        if cached_light_changed || live_light_changed {
            for &(handle, _) in &changes {
                let exists = self
                    .document
                    .get_entity(handle)
                    .is_some_and(|entity| matches!(entity, EntityType::Light(_)));
                crate::entities::object_data::update_light_entity(
                    &mut self.object_data_cache,
                    handle,
                    exists,
                );
            }
            self.lighting_cache.borrow_mut().clear();
        }
        if let Some(true) = self.has_face3d.get() {
            if changes
                .iter()
                .any(|(_, k)| matches!(k, ChangeKind::Removed))
            {
                self.has_face3d.set(None);
            }
        } else if changes.iter().any(|(h, _)| {
            self.document
                .get_entity(*h)
                .is_some_and(|e| matches!(e, EntityType::Face3D(_)))
        }) {
            self.has_face3d.set(Some(true));
        }
        if let Some(true) = self.has_section_objects.get() {
            if changes
                .iter()
                .any(|(_, k)| matches!(k, ChangeKind::Removed))
            {
                self.has_section_objects.set(None);
            }
        } else if changes
            .iter()
            .any(|(h, _)| self.document.get_entity(*h).is_some_and(is_section_object))
        {
            self.has_section_objects.set(Some(true));
        }
        if let Some(true) = self.has_tables.get() {
            if changes
                .iter()
                .any(|(_, k)| matches!(k, ChangeKind::Removed))
            {
                self.has_tables.set(None);
            }
        } else if changes.iter().any(|(h, _)| {
            self.document
                .get_entity(*h)
                .is_some_and(|e| matches!(e, EntityType::Table(_)))
        }) {
            self.has_tables.set(Some(true));
        }
        if let Some(true) = self.has_underlays.get() {
            if changes
                .iter()
                .any(|(_, k)| matches!(k, ChangeKind::Removed))
            {
                self.has_underlays.set(None);
            }
        } else if changes.iter().any(|(h, _)| {
            self.document
                .get_entity(*h)
                .is_some_and(|e| matches!(e, EntityType::Underlay(_)))
        }) {
            self.has_underlays.set(Some(true));
        }
        if let Some(true) = self.has_ole_frames.get() {
            if changes
                .iter()
                .any(|(_, k)| matches!(k, ChangeKind::Removed))
            {
                self.has_ole_frames.set(None);
            }
        } else if changes.iter().any(|(h, _)| {
            self.document
                .get_entity(*h)
                .is_some_and(|e| matches!(e, EntityType::Ole2Frame(_)))
        }) {
            self.has_ole_frames.set(Some(true));
        }
        let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.geometry_epoch = epoch;
        self.invalidate_projection_bounds();
        {
            let mut tm = self.tess_memo.borrow_mut();
            let mut rm = self.resident_tess_memo.borrow_mut();
            for &(h, k) in &changes {
                // A changed or removed entity must re-tessellate; a fresh Add has
                // no memo entry yet, so there is nothing to drop.
                if matches!(k, ChangeKind::Modified | ChangeKind::Removed) {
                    tm.remove(&h);
                    rm.remove(&h);
                }
            }
        }
        self.push_geometry_delta(
            epoch,
            Self::reported_changes(changes, &reported_before, &self.document),
            false,
        );
    }

    /// The delta names what moved. A `Modified` handle whose entity is
    /// byte-identical to the pre-solve copy is dropped — the solver
    /// considered it and left it alone — while added and removed handles,
    /// and anything the refresh pass appended after the snapshot, are
    /// always named.
    pub(crate) fn reported_changes(
        changes: Vec<(Handle, ChangeKind)>,
        before: &[(Handle, EntityType)],
        document: &Document,
    ) -> Vec<(Handle, ChangeKind)> {
        if before.is_empty() {
            return changes;
        }
        changes
            .into_iter()
            .filter(|(handle, kind)| {
                if !matches!(kind, ChangeKind::Modified) {
                    return true;
                }
                match before.iter().find(|(h, _)| h == handle) {
                    Some((_, snapshot)) => document.get_entity(*handle) != Some(snapshot),
                    None => true,
                }
            })
            .collect()
    }

    /// Append the handles an associative refresh re-solved, skipping the ones
    /// the edit already names (and the ones an earlier family appended).
    /// `members` is hydrated from `changes` on the first non-empty refresh and
    /// mirrors it afterwards; the old per-handle `changes.iter().any(..)` scan
    /// made this O(changes x refreshed) for every family.
    pub(crate) fn absorb_refreshed(
        changes: &mut Vec<(Handle, ChangeKind)>,
        members: &mut Option<HashSet<Handle>>,
        refreshed: Vec<(Handle, ChangeKind)>,
    ) {
        if refreshed.is_empty() {
            return;
        }
        let members =
            members.get_or_insert_with(|| changes.iter().map(|(handle, _)| *handle).collect());
        changes.reserve(refreshed.len());
        for change in refreshed {
            if members.insert(change.0) {
                changes.push(change);
            }
        }
    }

    /// True when any changed handle belongs to an ordinary block definition
    /// rather than model/paper space. History replay needs this distinction:
    /// restoring a block-local entity must invalidate the assembled definition
    /// and every INSERT that consumes it, not only that entity's direct wire.
    ///
    pub(crate) fn layout_block_handles(&self) -> HashSet<Handle> {
        self.document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Layout(layout) if !layout.block_record.is_null() => {
                    Some(layout.block_record)
                }
                _ => None,
            })
            .collect()
    }

    pub(crate) fn is_block_definition_record(
        record: &document::tables::BlockRecord,
        layout_blocks: &HashSet<Handle>,
    ) -> bool {
        let name = record.name.to_ascii_uppercase();
        !layout_blocks.contains(&record.handle)
            && !name.starts_with("*MODEL_SPACE")
            && !name.starts_with("*PAPER_SPACE")
    }

    pub(crate) fn touched_block_definition_names(
        &self,
        changes: &[(Handle, ChangeKind)],
        extra_block_handles: &[Handle],
        extra_block_names: &[String],
    ) -> Vec<String> {
        let layout_blocks = self.layout_block_handles();
        let mut def_by_handle: HashMap<Handle, &str> = HashMap::default();
        let mut def_by_marker: HashMap<Handle, &str> = HashMap::default();
        let mut def_records = Vec::new();
        let mut layout_names: HashSet<String> = HashSet::default();
        for record in self.document.block_records.iter() {
            if !Self::is_block_definition_record(record, &layout_blocks) {
                layout_names.insert(normalize_name(&record.name));
                continue;
            }
            let name = record.name.as_str();
            def_by_handle.insert(record.handle, name);
            if !record.block_entity_handle.is_null() {
                def_by_marker.insert(record.block_entity_handle, name);
            }
            if !record.block_end_handle.is_null() {
                def_by_marker.insert(record.block_end_handle, name);
            }
            def_records.push(record);
        }
        let mut seen: HashSet<String> = HashSet::default();
        let mut names = Vec::new();
        let mut push_name = |name: &str| {
            if !name.is_empty() && seen.insert(name.to_string()) {
                names.push(name.to_string());
            }
        };
        for &handle in extra_block_handles {
            if let Some(&name) = def_by_handle.get(&handle) {
                push_name(name);
            }
        }
        for name in extra_block_names {
            if !layout_names.contains(&normalize_name(name)) {
                push_name(name);
            }
        }
        for (handle, _) in changes {
            let mut matched_marker = false;
            if let Some(&name) = def_by_marker.get(handle) {
                push_name(name);
                matched_marker = true;
            }
            let owner = self
                .document
                .get_entity(*handle)
                .map(|entity| entity.common().owner_handle);
            if let Some(oh) = owner {
                if let Some(&name) = def_by_handle.get(&oh) {
                    push_name(name);
                    continue;
                }
                if !matched_marker && !oh.is_null() && layout_blocks.contains(&oh) {
                    continue;
                }
            }
            if !matched_marker {
                for record in &def_records {
                    if record.entity_handles.contains(handle) {
                        push_name(&record.name);
                    }
                }
            }
        }
        names
    }

    pub(crate) fn collect_block_dependent_handles(&self, touched_names: &[String]) -> Vec<Handle> {
        let touched_norm: HashSet<String> = touched_names
            .iter()
            .map(|name| normalize_name(name))
            .filter(|name| !name.is_empty())
            .collect();
        if touched_norm.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.dependency_index_cache.borrow().is_none() {
            *self.dependency_index_cache.borrow_mut() =
                Some(Arc::new(self.rebuild_dependency_index()));
        }
        if let Some(index) = self.dependency_index_cache.borrow().as_ref() {
            for norm in &touched_norm {
                if let Some(roots) = index.block_roots.get(norm) {
                    out.extend(roots.iter().copied());
                }
            }
        }
        if let Some(edited_handle) = self.block_edit_block {
            if let Some(record) = self
                .document
                .block_records
                .iter()
                .find(|r| r.handle == edited_handle)
            {
                let mut reaches_cache: HashMap<String, bool> = HashMap::default();
                let mut visiting: HashSet<String> = HashSet::default();
                for &entity_handle in &record.entity_handles {
                    let Some(entity) = self.document.get_entity(entity_handle) else {
                        continue;
                    };
                    let uses = render_graph::entity_block_uses(&self.document, entity, 1.0);
                    if uses.iter().any(|u| {
                        self.block_name_reaches_any(
                            &u.insert.block_name,
                            &touched_norm,
                            &mut reaches_cache,
                            &mut visiting,
                        )
                    }) {
                        out.push(entity_handle);
                    }
                }
            }
        }
        out
    }

    pub(crate) fn block_name_reaches_any(
        &self,
        name: &str,
        targets: &HashSet<String>,
        cache: &mut HashMap<String, bool>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        let norm = normalize_name(name);
        if norm.is_empty() {
            return false;
        }
        if targets.contains(&norm) {
            return true;
        }
        if let Some(&cached) = cache.get(&norm) {
            return cached;
        }
        if !visiting.insert(norm.clone()) {
            return false;
        }
        let mut found = false;
        if let Some(record) = self.document.block_records.get(name) {
            for &entity_handle in &record.entity_handles {
                if let Some(entity) = self.document.get_entity(entity_handle) {
                    let uses = render_graph::entity_block_uses(&self.document, entity, 1.0);
                    if uses.iter().any(|u| {
                        self.block_name_reaches_any(&u.insert.block_name, targets, cache, visiting)
                    }) {
                        found = true;
                        break;
                    }
                }
            }
        }
        visiting.remove(&norm);
        cache.insert(norm, found);
        found
    }

    /// Removed entities may no longer have a live slot, so fall back to the
    /// BlockRecord membership list, which deliberately retains their handles
    /// for symmetric undo/redo restoration.
    #[allow(dead_code)]
    pub(crate) fn changes_touch_block_definition(&self, changes: &[(Handle, ChangeKind)]) -> bool {
        let layout_blocks = self.layout_block_handles();
        changes.iter().any(|(handle, _)| {
            let owner = self
                .document
                .get_entity(*handle)
                .map(|entity| entity.common().owner_handle);
            self.document.block_records.iter().any(|record| {
                Self::is_block_definition_record(record, &layout_blocks)
                    && (owner == Some(record.handle)
                        || record.block_entity_handle == *handle
                        || record.block_end_handle == *handle
                        || record.entity_handles.contains(handle))
            })
        })
    }

    pub fn bump_geometry(&mut self) {
        let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.geometry_epoch = epoch;
        self.invalidate_projection_bounds();
        // A full structural change may move lights between blocks or alter
        // layer visibility without naming the affected handles.
        self.lighting_cache.borrow_mut().clear();
        *self.document_render_env_cache.borrow_mut() = None;
        self.background_image_cache.borrow_mut().clear();
        self.has_underlays.set(None);
        self.has_ole_frames.set(None);
        self.has_face3d.set(None);
        self.has_section_objects.set(None);
        self.has_tables.set(None);
        // Default: also invalidate block definitions. Safe for every caller;
        // operations that know blocks are untouched use `bump_geometry_no_blocks`.
        self.block_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        // A change this coarse can carry a new document; re-determine whether
        // it holds associative centerlines or center marks.
        self.has_associative_centers.set(None);
        self.has_dimension_associations.set(None);
        // Structural change — drop both per-entity tessellation memos.
        self.tess_memo.borrow_mut().clear();
        self.resident_tess_memo.borrow_mut().clear();
        *self.resident_memo_backing.borrow_mut() = MemoBacking::default();
        self.invalidate_dependency_index();
        // Can't name the changed handles → force every journal consumer to rebuild.
        self.push_geometry_delta(epoch, Vec::new(), true);
    }

    /// A whole-document state change that no incremental diff can describe
    /// (a whole-snapshot history restore): journal an explicit full-walk
    /// marker so every journal consumer — the plugin delta publisher (API
    /// v27) included — falls back to a full rebuild instead of presenting a
    /// partial set as complete. Unlike [`Self::bump_geometry`] this bumps
    /// only the geometry epoch and the journal: the caller has already
    /// rebuilt whatever derived caches it owns.
    pub fn bump_history_epoch(&mut self) {
        let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.geometry_epoch = epoch;
        self.push_geometry_delta(epoch, Vec::new(), true);
    }

    /// Stable per-viewport identity within this document, so request
    /// resolution and builder consumption can never disagree.
    pub fn instance_id_for(&self, inst: &ViewportInstance) -> u64 {
        if let Some(t) = inst.tile_idx {
            0x1000_0000_0000_0000 | (t as u64)
        } else if inst.paper_sheet {
            0x2000_0000_0000_0000
        } else {
            0x3000_0000_0000_0000 | inst.handle.value()
        }
    }

    /// [`Self::instance_id_for`] made unique across documents: the key of the
    /// viewport's GPU slot. The multiplier is odd, so scenes never map one
    /// viewport id to the same key.
    pub(in crate::scene) fn render_instance_id(&self, inst: &ViewportInstance) -> u64 {
        self.instance_id_for(inst) ^ self.render_id.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    /// REDRAW core: resolve a scope to the concrete `instance_id`s it targets
    /// and add them to the pending force set. `All` unions over `Active` so the
    /// superset always wins (never shrinks a prior `All`). No DB / undo touch.
    pub fn request_refresh(&self, scope: ViewportRefreshScope) {
        let mut set = self.pending_refresh.borrow_mut();
        if let ViewportRefreshScope::All = scope {
            set.clear();
            // All: every current tile in Model, else sheet + every paper content vp.
            if self.current_layout == "Model" {
                let tiles = self.model_tiles.borrow();
                for (t, _) in tiles.iter().enumerate() {
                    set.insert(0x1000_0000_0000_0000 | (t as u64));
                }
            } else {
                set.insert(0x2000_0000_0000_0000); // sheet
                let (_sheet, _sheet_h, content) = self.paper_viewport_handles();
                for h in content.iter() {
                    set.insert(0x3000_0000_0000_0000 | h.value());
                }
            }
        } else if self.current_layout != "Model" {
            // Active in paper: entered content vp, else the sheet (D6(a)).
            match self.active_viewport {
                Some(h) => {
                    set.insert(0x3000_0000_0000_0000 | h.value());
                }
                None => {
                    set.insert(0x2000_0000_0000_0000);
                }
            }
        } else {
            // Active in Model: the active tile (always resolvable).
            let active = self.active_model_tile.get();
            set.insert(0x1000_0000_0000_0000 | (active as u64));
        }
    }

    /// True while any force is still pending (never cleared falsely).
    #[cfg(test)]
    pub fn refresh_pending_any(&self) -> bool {
        !self.pending_refresh.borrow().is_empty()
    }

    /// Builder hook: if `instance_id` has a pending force, mark it forced and
    /// remove it (one-shot per id). Returns whether to set force_rasterize.
    pub(crate) fn refresh_consume(&self, instance_id: u64) -> bool {
        self.pending_refresh.borrow_mut().remove(&instance_id)
    }

    /// Drop a single entity from the tessellation memo so the next render
    /// re-tessellates just that entity while reusing every other. Pair with
    /// [`bump_geometry_no_blocks`] for an incremental single-entity edit.
    pub fn mark_entity_dirty(&mut self, handle: Handle) {
        self.tess_memo.borrow_mut().remove(&handle);
        self.resident_tess_memo.borrow_mut().remove(&handle);
    }

    /// Invalidate the visible-wire tessellation but KEEP the cached block
    /// definitions. Use only when the edit provably can't change any block
    /// defn (top-level entity create/edit, grip-moving an entity or insert) —
    /// it skips the all-blocks re-tessellation that otherwise spikes edit time.
    ///
    /// Prefer [`bump_entities`] when the changed handles are known: this variant
    /// can't name them, so it pushes a `full` journal delta and every derived
    /// cache falls back to a whole-drawing rebuild. Kept for migration and for
    /// callers whose changed set is genuinely unknowable.
    pub fn bump_geometry_no_blocks(&mut self) {
        self.bump_geometry_no_blocks_with(true);
    }

    /// Advance the geometry epoch when switching the active edited block space
    /// (`block_edit_block`) without invalidating cached block definitions or
    /// poisoning other spaces' resident wire sets with a full journal delta.
    pub fn bump_active_space(&mut self) {
        *self.entity_index_cache.borrow_mut() = None;
        self.bump_geometry_no_blocks_with(false);
    }

    pub(crate) fn bump_geometry_no_blocks_with(&mut self, full: bool) {
        let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.geometry_epoch = epoch;
        self.invalidate_projection_bounds();
        self.lighting_cache.borrow_mut().clear();
        *self.document_render_env_cache.borrow_mut() = None;
        self.background_image_cache.borrow_mut().clear();
        self.has_underlays.set(None);
        self.has_ole_frames.set(None);
        self.has_face3d.set(None);
        self.has_section_objects.set(None);
        self.has_tables.set(None);
        self.invalidate_dependency_index();
        self.push_geometry_delta(epoch, Vec::new(), full);
    }

    /// Invalidate the cached document render environment (fog parameters and image).
    pub fn invalidate_render_environment_cache(&self) {
        *self.document_render_env_cache.borrow_mut() = None;
        self.background_image_cache.borrow_mut().clear();
    }

    /// Mark the selection / hover-highlight set dirty without invalidating the
    /// (selection-independent) wire tessellation. Only the GPU xray overlay is
    /// rebuilt — no re-tessellation. Use this for pure select / deselect /
    /// hover changes; use [`bump_geometry`] when the geometry itself changed.
    pub fn bump_selection(&mut self) {
        self.selection_generation = self.selection_generation.wrapping_add(1);
    }

    /// Mark the hover / box-preview / constraint / filter highlight set dirty
    /// without touching the committed-selection generation. The xray overlay
    /// keys its *selected* GPU buffers on `selection_generation`, so a
    /// rollover or marquee tick never re-uploads the (potentially huge)
    /// committed selection — only the small hover half.
    pub fn bump_hover(&mut self) {
        self.hover_generation = self.hover_generation.wrapping_add(1);
    }

    /// Invalidate the selectable-group snapshot. Call after any change that
    /// creates, edits or removes a group object (and from undo / redo, which
    /// can restore them wholesale).
    pub fn bump_groups_epoch(&mut self) {
        self.groups_epoch = self.groups_epoch.wrapping_add(1);
    }

    pub(crate) fn bump_selection_set(&mut self) {
        self.selection_fingerprint_dirty = true;
        self.bump_selection();
    }

    /// Milliseconds after the last camera change during which the view counts as
    /// "actively navigating" for interaction-LOD purposes.
    const NAV_SETTLE_MS: u128 = 130;

    /// Interaction LOD: true while the view is actively being panned / zoomed /
    /// orbited. Detected purely from `camera_generation` (bumped by every camera
    /// move) plus a short settle timer, so no navigation call site needs to opt
    /// in. Called on the render path; it stamps the change time as a side effect.
    ///
    /// While this is true the per-pixel hatch pass is skipped (it dominates the
    /// GPU frame — a full-screen procedural pattern/boundary test). When it flips
    /// back to false on settle, the frame renders hatches once and the
    /// scene-render cache holds that image, so a still view is full quality.
    pub fn navigating_lod(&self) -> bool {
        let gen = self.camera_generation;
        if gen != self.nav_last_gen.get() {
            self.nav_last_gen.set(gen);
            self.nav_changed_at.set(Some(iced::time::Instant::now()));
        }
        self.nav_changed_at
            .get()
            .is_some_and(|t| t.elapsed().as_millis() < Self::NAV_SETTLE_MS)
    }

    pub(crate) fn record_nav_perf(&self, op: NavPerfOp, started: iced::time::Instant) {
        self.record_nav_perf_caused(op, "-", started);
    }

    pub(crate) fn record_nav_perf_caused(
        &self,
        op: NavPerfOp,
        cause: &'static str,
        started: iced::time::Instant,
    ) {
        if !crate::perf::enabled() {
            return;
        }
        let (space, mode) = if self.current_layout == "Model" {
            ("Model", "MODEL")
        } else if self.active_viewport.is_some() {
            ("Paper", "MSPACE")
        } else {
            ("Paper", "PSPACE")
        };
        self.nav_perf_pending.set(Some(NavPerfSample {
            op,
            cause,
            space,
            mode,
            started,
            input_ms: started.elapsed().as_secs_f64() * 1000.0,
            build_ms: 0.0,
        }));
    }

    pub(in crate::scene) fn take_nav_perf(&self) -> Option<NavPerfSample> {
        self.nav_perf_pending.take()
    }

    /// Whether the interaction-LOD hatch suppression is enabled (env
    /// `OCS_HATCH_LOD`), read once. Default OFF: the tessellated hatch pass is
    /// cheap enough that suppression — and its zoom flicker (#258) — is not
    /// needed. Kept behind the flag as a safety net for pathological drawings.
    pub fn hatch_lod_enabled(&self) -> bool {
        use std::sync::OnceLock;
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("OCS_HATCH_LOD").is_some())
    }

    /// True for a short window around navigation — used by the subscription to
    /// keep requesting frames just past the settle point so the one full-quality
    /// (hatched) frame actually renders after the cursor stops, even when no
    /// input event would otherwise trigger a redraw. Read-only (no side effect).
    pub fn is_settling(&self) -> bool {
        self.nav_changed_at
            .get()
            .is_some_and(|t| t.elapsed().as_millis() < Self::NAV_SETTLE_MS + 130)
    }
}
