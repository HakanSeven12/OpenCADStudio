use super::*;

impl Scene {
    /// Each object drawn in the current space that `keep` accepts, once, as a
    /// `(handle, entity)` clone for a modify command's working copy. Only the
    /// resident wire names are read and block copies stay unexpanded: one
    /// object can own many wires, and cloning per expanded wire copied a large
    /// drawing many times over when the command started (#1978).
    pub fn visible_entities_for_edit(
        &self,
        keep: impl Fn(&EntityType) -> bool,
    ) -> Vec<(Handle, EntityType)> {
        let wires = self.entity_wires_arc();
        let mut seen: HashSet<Handle> = HashSet::default();
        wires
            .iter()
            .filter_map(|wire| Self::handle_from_wire_name(&wire.name))
            .filter(|handle| seen.insert(*handle))
            .filter_map(|handle| {
                let entity = self.document.get_entity(handle).filter(|e| keep(e))?;
                Some((handle, entity.clone()))
            })
            .collect()
    }

    /// Only the handles of editable, visible HATCH entities. Modification
    /// commands need these in addition to the wire-based entity list: hatch
    /// fills are rendered separately and otherwise never reach TRIM.
    /// Unlike visible_hatches_for_click this never clones GPU hatch models.
    pub fn visible_hatch_handles_for_edit(&self) -> Vec<Handle> {
        let interactive = self.hatch_interaction_filter();
        self.hatches
            .keys()
            .copied()
            .filter(|handle| {
                interactive(*handle)
                    && matches!(
                        self.document.get_entity(*handle),
                        Some(EntityType::Hatch(_))
                    )
                    && !self.is_reference_only(*handle)
            })
            .collect()
    }

    /// Hatches eligible for click / box / lasso hit-testing in the current
    /// layout. Filters out block-internal source hatches (stored in
    /// `self.hatches` at block-local coords for the block-defn position,
    /// which doesn't project correctly through the offset-rel view_proj
    /// and was causing the wrong hatch to be selected on click).
    pub fn visible_hatches_for_click(
        &self,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> HashMap<Handle, HatchModel> {
        let interactive = self.hatch_interaction_filter();
        let visible = |h: Handle, m: &HatchModel| interactive(h).then(|| (h, m.clone()));
        if let Some(handles) = candidate_handles {
            handles
                .iter()
                .filter_map(|&handle| {
                    self.hatches
                        .get(&handle)
                        .and_then(|hatch| visible(handle, hatch))
                })
                .collect()
        } else {
            self.hatches
                .iter()
                .filter_map(|(&handle, hatch)| visible(handle, hatch))
                .collect()
        }
    }

    /// Whether a fill takes part in picking. The layout block and annotation
    /// state each search the document's objects, so they are resolved once
    /// per filter, not once per fill (#2023).
    pub(in crate::scene) fn hatch_interaction_filter(&self) -> impl Fn(Handle) -> bool + '_ {
        let scale = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();
        let block = self.interaction_block_handle();
        move |handle| {
            let Some(entity) = self.document.get_entity(handle) else {
                return false;
            };
            // Use the SOLID's WCS-aware wire fill for interaction as well as
            // drawing. Its cached HatchModel is flattened to XY for plotting
            // and must not leave an invisible selection target at z=0 (#617).
            if matches!(entity, EntityType::Solid(_)) {
                return false;
            }
            let common = entity.common();
            if self.display_entity_invisible(common)
                || self.entity_temporarily_hidden(handle)
                || self.layer_hidden(&common.layer)
                || self.interaction_layer_frozen(&common.layer)
                || crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    common,
                    scale,
                    all_visible,
                )
            {
                return false;
            }
            self.belongs_to_visible_block(handle, common.owner_handle, block)
        }
    }

    /// Instanced hatch models keyed by their block-backed host handle.
    pub fn insert_hatches_for_click(&self) -> Arc<HashMap<Handle, Vec<HatchModel>>> {
        let interaction_block = self.interaction_block_handle();
        let space_key = self.interaction_space_key();
        {
            let c = self.caches.insert_hatch_cache.borrow();
            if let Some((epoch, cached_space, ref arc)) = *c {
                if cached_space == space_key
                    && self.category_cache_valid(epoch, CACHE_CATEGORY_INSERT_HATCH, |handle| {
                        self.document.get_entity(handle).is_some_and(|entity| {
                            render_graph::entity_render_block_uses(
                                &self.document,
                                entity,
                                self.annotation_scale,
                            )
                            .into_iter()
                            .any(|block_use| block_use.active)
                        })
                    })
                {
                    let arc = Arc::clone(arc);
                    drop(c);
                    if let Some((cached_epoch, _, _)) =
                        self.caches.insert_hatch_cache.borrow_mut().as_mut()
                    {
                        *cached_epoch = self.geometry_epoch;
                    }
                    return arc;
                }
            }
        }
        // Only some entities changed since the cached map was built (no
        // structural delta in between): rebuild the entries of those
        // entities alone. A paste of many INSERTs otherwise paid a walk of
        // every block reference in the drawing on the first click after it.
        let stale = self.caches.insert_hatch_cache.borrow_mut().take();
        if let Some((epoch, cached_space, mut map)) = stale {
            if cached_space == space_key {
                if let Some(changes) = self.replay_since(epoch) {
                    let changed: Vec<Handle> = changes.iter().map(|(handle, _)| *handle).collect();
                    let fresh = self.collect_insert_hatches(
                        interaction_block,
                        changed
                            .iter()
                            .filter_map(|handle| self.document.get_entity(*handle)),
                    );
                    let entries = Arc::make_mut(&mut map);
                    for handle in &changed {
                        entries.remove(handle);
                    }
                    entries.extend(fresh);
                    *self.caches.insert_hatch_cache.borrow_mut() =
                        Some((self.geometry_epoch, space_key, Arc::clone(&map)));
                    return map;
                }
            }
        }
        let arc =
            Arc::new(self.collect_insert_hatches(interaction_block, self.document.entities()));
        *self.caches.insert_hatch_cache.borrow_mut() =
            Some((self.geometry_epoch, space_key, Arc::clone(&arc)));
        arc
    }

    /// Hatches inside the block references among `entities`, placed in
    /// world space, keyed by the reference that draws them.
    pub(in crate::scene) fn collect_insert_hatches<'a>(
        &'a self,
        interaction_block: Handle,
        entities: impl Iterator<Item = &'a EntityType>,
    ) -> HashMap<Handle, Vec<HatchModel>> {
        // No block definition holds a hatch, so no reference places one.
        if !render_graph::any_block_definition_holds(&self.document, |entity| {
            matches!(entity, EntityType::Hatch(_))
        }) {
            return HashMap::default();
        }
        let layer_hidden =
            |layer: &str| self.layer_hidden(layer) || self.interaction_layer_frozen(layer);
        let mut out: HashMap<Handle, Vec<HatchModel>> = HashMap::default();
        // Skip block subtrees that contain no hatch at all. The presence test
        // is memoised across instances.
        let mut hatch_memo: std::collections::HashMap<String, bool> =
            std::collections::HashMap::new();
        let annotation_scale_handle = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();
        let depth_map = self.draw_depth_map();
        let frozen: HashSet<Handle> = self
            .interaction_viewport_frozen_layers()
            .into_iter()
            .flatten()
            .copied()
            .collect();
        let visibility_reveal = self.visibility_reveal_handles();
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            (!frozen.is_empty()).then_some(&frozen),
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_visibility_reveal(&visibility_reveal)
        // The cache is keyed on `interaction_space_key`, already walk-folded,
        // so this build always serves the walked interaction space.
        .with_layer_walk(self.interaction_layer_walk())
        .with_annotation_scale(self.annotation_scale);
        for entity in entities {
            let contextual = crate::scene::annotative::entity_for_annotation_context(
                &self.document,
                entity,
                annotation_scale_handle,
            );
            let host = contextual.as_ref();
            let common = host.common();
            // Cheap, memoised filter first: most entities use no block that
            // holds a hatch, and the visibility tests below are far costlier
            // per entity than this lookup.
            let block_uses: Vec<_> =
                render_graph::entity_render_block_uses(&self.document, host, self.annotation_scale)
                    .into_iter()
                    .filter(|block_use| {
                        block_use.active
                            && render_graph::block_contains_hatch(
                                &self.document,
                                &block_use.insert.block_name,
                                &mut hatch_memo,
                            )
                    })
                    .collect();
            if block_uses.is_empty() {
                continue;
            }
            if self.display_entity_invisible(common)
                || self.entity_temporarily_hidden(common.handle)
                || layer_hidden(&common.layer)
                || crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    common,
                    annotation_scale_handle,
                    all_visible,
                )
            {
                continue;
            }
            if !self.belongs_to_visible_block(common.handle, common.owner_handle, interaction_block)
            {
                continue;
            }
            for block_use in block_uses {
                graph.walk_block_use(
                    &block_use,
                    common.handle,
                    |_, _| true,
                    |entity, context| {
                        let EntityType::Hatch(source) = entity else {
                            return;
                        };
                        let mut placed = EntityType::Hatch(source.clone());
                        placed.apply_transform(&context.transform);
                        let EntityType::Hatch(hatch) = placed else {
                            return;
                        };
                        let color = crate::scene::view::render::adapt_to_bg(
                            context.style_for(&self.document, entity).0,
                            self.current_bg(),
                        );
                        let Some(mut model) = Self::hatch_model_from_dxf(&hatch, color) else {
                            return;
                        };
                        for clip in &context.clips {
                            let clipped = pick::xclip::clip_hatch_boundary(
                                &model.boundary,
                                model.world_origin,
                                clip,
                            );
                            if clipped.is_empty() {
                                return;
                            }
                            model.boundary = Arc::new(clipped);
                        }
                        out.entry(common.handle).or_default().push(model);
                    },
                );
            }
        }
        out
    }

    /// Wires that should participate in hit-testing, snapping, and selection.
    ///
    /// - Model layout: all entity wires (same as entity_wires).
    /// - PSPACE (paper layout, no active viewport): paper-space entities only —
    ///   viewport content is NOT interactive.
    /// - MSPACE (active viewport set): model-space content of the active viewport
    ///   only — paper-space entities are NOT interactive.
    pub fn hit_test_wires(&self) -> Arc<Vec<WireModel>> {
        if self.current_layout == "Model" {
            // Model uses the resident, camera-independent full wire set, so the
            // interaction index survives pan/zoom and reaches every entity.
            return self.entity_wires_arc();
        }
        match self.active_viewport {
            None => self.paper_sheet_wires_arc(),
            // MSPACE editing already uses the viewport's model camera, so its
            // interaction source must stay in model coordinates too. The old
            // projected-Paper copy rebuilt a large Vec/Arc and interaction
            // index on entry even though the resident model set already exists.
            Some(vp_handle) => self.model_wires_for_viewport_arc(vp_handle, 0.0),
        }
    }

    /// Wires of the space being edited that may cross a world-XY box: the
    /// indexed broad phase in model space, every wire elsewhere.
    pub fn interaction_candidates_xy(
        &self,
        aabb: [f64; 4],
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        let wires = self.hit_test_wires();
        if self.current_layout != "Model" || !self.interaction_index_worthwhile(&wires) {
            return self.materialize_stubs_cached(
                crate::scene::pick::interaction_index::InteractionCandidates::all(wires),
            );
        }
        self.indexed_interaction_candidates_xy(wires, aabb, false, false, false, None)
    }

    /// Small simple sets are cheaper to scan. Either many wires or substantial
    /// sub-geometry (one giant polyline/text batch/mesh) enables the index.
    const INTERACTION_INDEX_MIN_WIRES: usize = 4_000;
    const INTERACTION_INDEX_MIN_WORK: usize = 20_000;

    /// `InteractionCandidates::materialize_stubs`, but an unrestricted
    /// expansion of a wire set already expanded under this `geometry_epoch`
    /// is reused instead of recomputed. A restricted (XY- or index-limited)
    /// query depends on the query too, so it is passed straight through.
    pub(in crate::scene) fn materialize_stubs_cached(
        &self,
        candidates: crate::scene::pick::interaction_index::InteractionCandidates,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        if !candidates.expansion_is_unrestricted() {
            return candidates.materialize_stubs();
        }
        let Some(source) = candidates.snapshot_arc() else {
            return candidates.materialize_stubs();
        };
        if !candidates.has_stubs() {
            return candidates;
        }
        // Which guard rejected the entry, so the cost of a miss can be
        // attributed: `geometry` moves on any entity edit, `block` only when a
        // definition changes, and the expansion depends on the latter.
        let mut miss = "no-entry";
        if let Some((weak, epoch, block_epoch, wires)) = self.caches.stub_expansion_cache.borrow().as_ref()
        {
            let same_source = weak
                .upgrade()
                .is_some_and(|alive| Arc::ptr_eq(&alive, &source));
            let same_geometry = *epoch == self.geometry_epoch;
            let same_block = *block_epoch == self.block_epoch;
            if same_geometry && same_source {
                if crate::perf::enabled() {
                    crate::perf_record!(
                        "[perf] stub-cache hit wires={} source={}",
                        wires.len(),
                        source.len(),
                    );
                }
                return candidates.with_materialized(Arc::clone(wires));
            }
            miss = match (same_source, same_geometry, same_block) {
                (false, _, _) => "source",
                (true, false, true) => "geometry-only",
                (true, false, false) => "geometry-and-block",
                (true, true, _) => "unreachable",
            };
        }
        if crate::perf::enabled() {
            crate::perf_record!("[perf] stub-cache miss reason={}", miss);
        }
        let expanded = candidates.materialize_stubs();
        if let Some(wires) = expanded.snapshot_arc() {
            let budget = if Self::low_memory_viewing() { 16 } else { 64 } << 20;
            let mut cache = self.caches.stub_expansion_cache.borrow_mut();
            if wires.len() * std::mem::size_of::<WireModel>() > budget
                || Self::resident_wire_bytes(&wires) > budget
            {
                *cache = None;
                return expanded;
            }
            *cache = Some((
                Arc::downgrade(&source),
                self.geometry_epoch,
                self.block_epoch,
                wires,
            ));
        }
        expanded
    }

    pub(in crate::scene) fn interaction_index_worthwhile(&self, wires: &[WireModel]) -> bool {
        wires.len() >= Self::INTERACTION_INDEX_MIN_WIRES
            || crate::scene::pick::interaction_index::InteractionIndex::estimated_work(wires)
                >= Self::INTERACTION_INDEX_MIN_WORK
            // A slot can stand for a whole block. Keep aperture queries
            // indexed even when the number of resident slots is small.
            || wires.iter().any(|wire| wire.stub.is_some())
            || self.hatches.len() >= Self::INTERACTION_INDEX_MIN_WIRES
            || self.meshes.len().saturating_add(self.block_meshes.len())
                >= Self::INTERACTION_INDEX_MIN_WIRES
            || self
                .hatches
                .values()
                .any(|hatch| hatch.boundary.len() >= Self::INTERACTION_INDEX_MIN_WORK)
            || self
                .meshes
                .values()
                .chain(self.block_meshes.values())
                .any(|set| {
                    set.lods
                        .first()
                        .is_some_and(|mesh| mesh.verts.len() >= Self::INTERACTION_INDEX_MIN_WORK)
                })
    }

    pub(crate) fn view_needs_projected_interaction(&self) -> bool {
        let camera = self.navigation_camera();
        camera.projection == view::camera::Projection::Perspective
            || (camera.rotation * glam::Vec3::Z).truncate().length_squared() > 1.0e-12
    }

    /// Navigation can start building a projected index before the first pick.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn prepare_projected_interaction(&self) {
        if !self.view_needs_projected_interaction() {
            return;
        }
        let cache = self.caches.interaction_index_cache.borrow();
        if let Some((_, _, _, index)) = cache.iter().rev().find(|(epoch, _, source, _)| {
            *epoch == self.geometry_epoch && source.strong_count() > 0
        }) {
            index.prepare_screen_in_background();
        }
    }

    pub(in crate::scene) fn cached_interaction_index(
        &self,
        wires: &Arc<Vec<WireModel>>,
    ) -> Option<Arc<crate::scene::pick::interaction_index::InteractionIndex>> {
        let source = Arc::as_ptr(wires) as usize;
        let mut cache = self.caches.interaction_index_cache.borrow_mut();
        cache
            .retain(|(epoch, _, weak, _)| *epoch == self.geometry_epoch && weak.strong_count() > 0);
        if let Some(position) = cache.iter().position(|(_, ptr, weak, _)| {
            *ptr == source
                && weak
                    .upgrade()
                    .is_some_and(|cached| Arc::ptr_eq(&cached, wires))
        }) {
            let entry = cache.remove(position);
            let index = Arc::clone(&entry.3);
            cache.push(entry);
            if self.current_layout == "Model" {
                *self.caches.interaction_base_index_cache.borrow_mut() = Some((
                    self.geometry_epoch,
                    self.interaction_space_key(),
                    Arc::clone(&index),
                ));
            }
            return Some(index);
        }
        None
    }

    pub(in crate::scene) fn cache_interaction_index(
        &self,
        epoch: u64,
        wires: Arc<Vec<WireModel>>,
        index: Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    ) {
        const MAX_CACHED_SOURCES: usize = 4;
        let source = Arc::as_ptr(&wires) as usize;
        let mut cache = self.caches.interaction_index_cache.borrow_mut();
        cache.retain(|(cached_epoch, ptr, weak, _)| {
            *cached_epoch == self.geometry_epoch
                && weak.strong_count() > 0
                && !(*cached_epoch == epoch && *ptr == source)
        });
        cache.push((epoch, source, Arc::downgrade(&wires), Arc::clone(&index)));
        if cache.len() > MAX_CACHED_SOURCES {
            cache.remove(0);
        }
        if epoch == self.geometry_epoch && self.current_layout == "Model" {
            *self.caches.interaction_base_index_cache.borrow_mut() =
                Some((epoch, self.interaction_space_key(), index));
        }
    }

    /// Return a stable build key when this source needs its large
    /// interaction index. The app uses it to deduplicate background builds.
    pub fn interaction_index_build_key(
        &self,
        wires: &Arc<Vec<WireModel>>,
        screen_height_px: f32,
    ) -> Option<(u64, usize)> {
        let key = (self.geometry_epoch, Arc::as_ptr(wires) as usize);
        if !self.interaction_source_is_resident(wires, screen_height_px.max(1.0))
            || !self.interaction_index_worthwhile(wires)
            || self.interaction_overlay_base().is_some()
        {
            return None;
        }
        if self.cached_interaction_index(wires).is_some() {
            if self.interaction_index_pending_key.get() == Some(key) {
                self.interaction_index_pending_key.set(None);
            }
            return None;
        }
        Some(key)
    }

    pub fn mark_interaction_index_pending(&self, epoch: u64, source: usize) {
        self.interaction_index_pending_key
            .set(Some((epoch, source)));
    }

    /// Install a fully prepared background index only if its geometry/source
    /// still belongs to this scene.
    pub fn install_prepared_interaction_index(
        &self,
        epoch: u64,
        source: usize,
        wires: std::sync::Weak<Vec<WireModel>>,
        index: Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    ) -> bool {
        if self.interaction_index_pending_key.get() == Some((epoch, source)) {
            self.interaction_index_pending_key.set(None);
        }
        let Some(wires) = wires.upgrade() else {
            return false;
        };
        if epoch != self.geometry_epoch || Arc::as_ptr(&wires) as usize != source {
            return false;
        }
        self.cache_interaction_index(epoch, wires, index);
        true
    }

    const INTERACTION_OVERLAY_MAX_HANDLES: usize = 2_048;

    pub(in crate::scene) fn interaction_overlay_can_replay(&self, base_epoch: u64, changes: usize) -> bool {
        changes <= Self::INTERACTION_OVERLAY_MAX_HANDLES
            || self
                .caches.geometry_deltas
                .borrow()
                .iter()
                .filter(|delta| delta.epoch > base_epoch && !delta.layer_visibility_only)
                .map(|delta| delta.changes.len())
                .sum::<usize>()
                <= Self::INTERACTION_OVERLAY_MAX_HANDLES
    }

    pub(in crate::scene) fn interaction_overlay_base(&self) -> Option<OverlayBase> {
        let space_key = self.interaction_space_key();
        let (epoch, index) = {
            let cache = self.caches.interaction_base_index_cache.borrow();
            let (epoch, cached_space, index) = cache.as_ref()?;
            if *epoch == self.geometry_epoch || *cached_space != space_key {
                return None;
            }
            (*epoch, Arc::clone(index))
        };
        {
            let cached = self.caches.interaction_overlay_changes.borrow();
            if let Some((base_epoch, geometry_epoch, cached_space, changes)) = cached.as_ref() {
                if *base_epoch == epoch
                    && *geometry_epoch == self.geometry_epoch
                    && *cached_space == space_key
                {
                    return self
                        .interaction_overlay_can_replay(epoch, changes.len())
                        .then_some((epoch, index, Arc::clone(changes)));
                }
            }
        }
        let changes = Arc::new(self.replay_since(epoch)?);
        *self.caches.interaction_overlay_changes.borrow_mut() =
            Some((epoch, self.geometry_epoch, space_key, Arc::clone(&changes)));
        self.interaction_overlay_can_replay(epoch, changes.len())
            .then_some((epoch, index, changes))
    }

    /// Recover the wire snapshot that was indexed by an immutable overlay
    /// base. The weak source is kept in `interaction_index_cache` so using
    /// the base index never pairs its wire ordinals with a newer snapshot.
    pub(in crate::scene) fn interaction_source_for_index(
        &self,
        epoch: u64,
        index: &Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    ) -> Option<Arc<Vec<WireModel>>> {
        self.caches.interaction_index_cache.borrow().iter().find_map(
            |(cached_epoch, _, wires, cached_index)| {
                (*cached_epoch == epoch && Arc::ptr_eq(cached_index, index))
                    .then(|| wires.upgrade())
                    .flatten()
            },
        )
    }

    pub(in crate::scene) fn interaction_overlay_changed_index(
        &self,
        base_epoch: u64,
        changes: &[(Handle, ChangeKind)],
    ) -> (
        Arc<Vec<WireModel>>,
        Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    ) {
        let space_key = self.interaction_space_key();
        {
            let cache = self.caches.interaction_overlay_index_cache.borrow();
            if let Some((epoch, cached_base, cached_space, wires, index)) = cache.as_ref() {
                if *epoch == self.geometry_epoch
                    && *cached_base == base_epoch
                    && *cached_space == space_key
                {
                    return (Arc::clone(wires), Arc::clone(index));
                }
            }
        }
        let block = self.interaction_block_handle();
        let frozen: Option<HashSet<Handle>> = self
            .interaction_viewport_frozen_layers()
            .map(|layers| layers.iter().copied().collect());
        let annotation_scale_handle = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();

        let changed_live: Vec<Handle> = changes
            .iter()
            .filter_map(|(handle, kind)| {
                if matches!(kind, ChangeKind::Removed) {
                    return None;
                }

                let entity = self.document.get_entity(*handle)?;
                self.resident_entity_visible(
                    entity,
                    block,
                    frozen.as_ref(),
                    annotation_scale_handle,
                    all_visible,
                )
                .then_some(*handle)
            })
            .collect();
        let wires = Arc::new(self.wire_models_for(&changed_live));
        let index =
            Arc::new(crate::scene::pick::interaction_index::InteractionIndex::build(&wires));
        *self.caches.interaction_overlay_index_cache.borrow_mut() = Some((
            self.geometry_epoch,
            base_epoch,
            space_key,
            Arc::clone(&wires),
            Arc::clone(&index),
        ));
        (wires, index)
    }

    pub(in crate::scene) fn interaction_overlay_wires(
        &self,
        base_keys: impl IntoIterator<Item = (u64, u32)>,
        changed_keys: impl IntoIterator<Item = (u64, u32)>,
        changes: &[(Handle, ChangeKind)],
        changed_wires: &Arc<Vec<WireModel>>,
    ) -> OverlayGather {
        enum MemoSliceRef<'a> {
            Owned(&'a Arc<Vec<WireModel>>),
            Slice(&'a Arc<Vec<WireModel>>, usize, usize),
        }
        impl<'a> MemoSliceRef<'a> {
            #[inline]
            fn shared_at(&self, ordinal: usize) -> Option<(&'a Arc<Vec<WireModel>>, u32)> {
                match *self {
                    Self::Owned(wires) => {
                        (ordinal < wires.len()).then_some((wires, ordinal as u32))
                    }
                    Self::Slice(set, start, len) => {
                        (ordinal < len).then_some((set, (start + ordinal) as u32))
                    }
                }
            }
        }

        let changed: HashSet<Handle> = changes.iter().map(|(handle, _)| *handle).collect();
        let memo = self.caches.resident_tess_memo.borrow();
        let backing_ref = self.caches.resident_memo_backing.borrow();
        let backing_set = backing_ref.set.upgrade();
        let base_iter = base_keys.into_iter();
        let base_hint = base_iter.size_hint().0;
        let mut wires =
            crate::scene::pick::interaction_index::SharedWireRuns::with_capacity(base_hint);
        let mut base_slots = HashMap::with_capacity_and_hasher(base_hint, Default::default());
        let mut misses: HashMap<Handle, Vec<u32>> = HashMap::default();
        let mut prev_handle = Handle::NULL;
        let mut prev_hit: Option<MemoSliceRef<'_>> = None;
        let mut prev_valid = false;
        for (handle_value, ordinal) in base_iter {
            let handle = Handle::new(handle_value);
            if handle != prev_handle {
                prev_handle = handle;
                if changed.contains(&handle) || self.document.get_entity(handle).is_none() {
                    prev_valid = false;
                    prev_hit = None;
                } else {
                    prev_valid = true;
                    prev_hit = match memo.get(&handle) {
                        Some(MemoRun::Owned(w)) => Some(MemoSliceRef::Owned(w)),
                        Some(MemoRun::InSet) => {
                            if let (Some(set), Some(&(start, len))) =
                                (backing_set.as_ref(), backing_ref.ranges.get(&handle))
                            {
                                (start + len <= set.len())
                                    .then_some(MemoSliceRef::Slice(set, start, len))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                }
            }
            if !prev_valid {
                continue;
            }
            if let Some((run, index)) = prev_hit
                .as_ref()
                .and_then(|hit| hit.shared_at(ordinal as usize))
            {
                base_slots.insert((handle_value, ordinal), wires.len() as u32);
                wires.push(run, index);
            } else {
                misses.entry(handle).or_default().push(ordinal);
            }
        }
        drop(backing_ref);
        drop(memo);
        // A memo miss is uncommon (guard change / legacy source). Preserve
        // correctness without expanding every nearby entity: regenerate that
        // handle, then retain only the exact wire ordinals the base index
        // reported.
        for (handle, ordinals) in misses {
            let entity_wires = Arc::new(self.wire_models_for(&[handle]));
            for ordinal in ordinals {
                if entity_wires.get(ordinal as usize).is_some() {
                    base_slots.insert((handle.value(), ordinal), wires.len() as u32);
                    wires.push(&entity_wires, ordinal);
                }
            }
        }
        let changed_keys: HashSet<(u64, u32)> = changed_keys.into_iter().collect();
        let mut changed_slots = HashMap::default();
        let mut next_ordinal: HashMap<u64, u32> = HashMap::default();
        for (position, wire) in changed_wires.iter().enumerate() {
            if let Some(handle) = pipeline::wire_gpu::fast_parse_u64(&wire.name) {
                let ordinal = next_ordinal.entry(handle).or_default();
                let key = (handle, *ordinal);
                if changed_keys.contains(&key) {
                    changed_slots.insert(key, wires.len() as u32);
                    wires.push(changed_wires, position as u32);
                }
                *ordinal += 1;
            }
        }
        (Arc::new(wires), base_slots, changed_slots)
    }

    pub(in crate::scene) fn indexed_interaction_candidates_xy(
        &self,
        wires: Arc<Vec<WireModel>>,
        aabb: [f64; 4],
        allow_pending_empty: bool,
        area_only: bool,
        pick_only: bool,
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        // The stubbed query falls back to the whole unindexed set when no
        // index is cached yet, and that fallback is unrestricted — so it goes
        // through the cache, which passes a genuinely restricted result
        // straight on.
        self.materialize_stubs_cached(self.indexed_interaction_candidates_xy_stubbed(
            wires,
            aabb,
            allow_pending_empty,
            area_only,
            pick_only,
            snap,
        ))
    }

    /// `snap` narrows the query to the object-snap feature indexes (and,
    /// for the live preview, to discrete features only).
    pub(in crate::scene) fn indexed_interaction_candidates_xy_stubbed(
        &self,
        wires: Arc<Vec<WireModel>>,
        aabb: [f64; 4],
        allow_pending_empty: bool,
        area_only: bool,
        pick_only: bool,
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        let direct = |index: &InteractionIndex, wires| match snap {
            Some(category) => index.query_snap_xy_with_category(wires, aabb, category),
            None if pick_only => index.query_xy_selection_for_materialize(wires, aabb, true),
            None if area_only => index.query_xy_selection_for_materialize(wires, aabb, false),
            None => index.query_xy_for_materialize(wires, aabb),
        };
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            // Nothing changed since the base was indexed: query its own
            // snapshot instead of gathering an overlay copy.
            if changes.is_empty() {
                if let Some(source) = self.interaction_source_for_index(base_epoch, &base) {
                    return direct(&base, source);
                }
            }
            let perf = crate::perf::enabled();
            let t0 = iced::time::Instant::now();
            let base_parts = snap.map(|category| base.snap_candidate_parts_xy(aabb, category));
            let keys = match &base_parts {
                Some(parts) => base.snap_candidate_keys(parts),
                None if pick_only => base.query_pick_wire_keys_xy(aabb),
                None => base.query_wire_keys_xy(aabb),
            };
            let key_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let t_changed = iced::time::Instant::now();
            let (changed_wires, changed_index) =
                self.interaction_overlay_changed_index(base_epoch, &changes);
            let changed_ms = t_changed.elapsed().as_secs_f64() * 1000.0;
            let changed_parts =
                snap.map(|category| changed_index.snap_candidate_parts_xy(aabb, category));
            let changed_keys = match &changed_parts {
                Some(parts) => changed_index.snap_candidate_keys(parts),
                None if pick_only => changed_index.query_pick_wire_keys_xy(aabb),
                None => changed_index.query_wire_keys_xy(aabb),
            };
            let t_local = iced::time::Instant::now();
            let (local, base_slots, changed_slots) = self.interaction_overlay_wires(
                keys.iter().copied(),
                changed_keys,
                &changes,
                &changed_wires,
            );
            let local_ms = t_local.elapsed().as_secs_f64() * 1000.0;
            let t_remap = iced::time::Instant::now();
            // `indexed_interaction_candidates_xy` runs `materialize_stubs` next,
            // which discards all primitive ref lists whenever any candidate
            // wire is a stub, so skip remapping them in that case.
            let result = if local.has_any_stub() {
                InteractionCandidates::shared_indexed_all(local, Some(aabb))
            } else {
                match (snap, base_parts, changed_parts) {
                    (Some(category), Some(base_parts), Some(changed_parts)) => {
                        let mut result = base.query_remapped_snap_xy_from_parts_with_category(
                            Arc::clone(&local),
                            &base_slots,
                            base_parts,
                            aabb,
                            category,
                        );
                        result.extend_indexed(
                            changed_index.query_remapped_snap_xy_from_parts_with_category(
                                local,
                                &changed_slots,
                                changed_parts,
                                aabb,
                                category,
                            ),
                        );
                        result
                    }
                    _ if area_only || pick_only => {
                        let mut result = base.query_remapped_xy_area(
                            Arc::clone(&local),
                            &base_slots,
                            aabb,
                            pick_only,
                        );
                        let replacements = changed_index.query_remapped_xy_area(
                            local,
                            &changed_slots,
                            aabb,
                            pick_only,
                        );
                        if pick_only {
                            result.extend_indexed(replacements);
                        } else {
                            result.extend_indexed_area(replacements);
                        }
                        result
                    }
                    _ => {
                        let mut result =
                            base.query_remapped_xy(Arc::clone(&local), &base_slots, aabb);
                        result.extend_indexed(changed_index.query_remapped_xy(
                            local,
                            &changed_slots,
                            aabb,
                        ));
                        result
                    }
                }
            };
            let remap_ms = t_remap.elapsed().as_secs_f64() * 1000.0;
            let total_ms = t0.elapsed().as_secs_f64() * 1000.0;
            if perf && total_ms >= 50.0 {
                crate::perf_record!(
                    "[perf] interaction-overlay {:>7.1}ms keys={} wires={} query={:.1} changed={:.1} gather={:.1} remap={:.1}",
                    total_ms,
                    keys.len(),
                    result.len(),
                    key_ms,
                    changed_ms,
                    local_ms,
                    remap_ms,
                );
            }
            return result;
        }
        if let Some(index) = self.cached_interaction_index(&wires) {
            direct(&index, wires)
        } else {
            self.unindexed_interaction_candidates(wires, allow_pending_empty, snap, Some(aabb))
        }
    }

    /// No index for `wires` yet. A pending-empty caller gets an empty result
    /// while the index builds; the snap preview gets one for any large set,
    /// so it never scans the whole drawing on a pointer move.
    ///
    /// `near` is the region the caller is about to hit-test, when it has one.
    /// Passing it keeps the stub expansion local — the whole-set expansion of
    /// a stub-heavy drawing costs ~190 ms, against 7-13 ms for one aperture.
    pub(in crate::scene) fn unindexed_interaction_candidates(
        &self,
        wires: Arc<Vec<WireModel>>,
        allow_pending_empty: bool,
        snap: Option<SnapQueryCategory>,
        near: Option<[f64; 4]>,
    ) -> InteractionCandidates {
        let pending = allow_pending_empty
            && (snap.is_some() && wires.len() > 512
                || self.interaction_index_pending_key.get()
                    == Some((self.geometry_epoch, Arc::as_ptr(&wires) as usize)));
        if pending {
            InteractionCandidates::pending(wires)
        } else {
            match near {
                Some(aabb) => InteractionCandidates::all_near(wires, aabb),
                None => InteractionCandidates::all(wires),
            }
        }
    }

    pub(in crate::scene) fn indexed_interaction_candidates_screen(
        &self,
        wires: Arc<Vec<WireModel>>,
        screen_rect: [f32; 4],
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        allow_pending_empty: bool,
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        self.materialize_stubs_cached(self.indexed_interaction_candidates_screen_stubbed(
            wires,
            screen_rect,
            view_rot,
            eye,
            bounds,
            allow_pending_empty,
            snap,
        ))
    }

    pub(in crate::scene) fn indexed_interaction_candidates_screen_stubbed(
        &self,
        wires: Arc<Vec<WireModel>>,
        screen_rect: [f32; 4],
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        allow_pending_empty: bool,
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        let direct = |index: &InteractionIndex, wires| match snap {
            Some(category) => index.query_snap_screen_with_category(
                wires,
                screen_rect,
                view_rot,
                eye,
                bounds,
                category,
            ),
            None => index.query_screen(wires, screen_rect, view_rot, eye, bounds),
        };
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            if changes.is_empty() {
                if let Some(source) = self.interaction_source_for_index(base_epoch, &base) {
                    return direct(&base, source);
                }
            }
            let perf = crate::perf::enabled();
            let t0 = iced::time::Instant::now();
            let base_parts = snap.map(|category| {
                base.snap_candidate_parts_screen(screen_rect, view_rot, eye, bounds, category)
            });
            let keys = match &base_parts {
                Some(parts) => base.snap_candidate_keys(parts),
                None => base.query_wire_keys_screen(screen_rect, view_rot, eye, bounds),
            };
            let key_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let t_changed = iced::time::Instant::now();
            let (changed_wires, changed_index) =
                self.interaction_overlay_changed_index(base_epoch, &changes);
            let changed_ms = t_changed.elapsed().as_secs_f64() * 1000.0;
            let changed_parts = snap.map(|category| {
                changed_index.snap_candidate_parts_screen(
                    screen_rect,
                    view_rot,
                    eye,
                    bounds,
                    category,
                )
            });
            let changed_keys = match &changed_parts {
                Some(parts) => changed_index.snap_candidate_keys(parts),
                None => changed_index.query_wire_keys_screen(screen_rect, view_rot, eye, bounds),
            };
            let t_local = iced::time::Instant::now();
            let (local, base_slots, changed_slots) = self.interaction_overlay_wires(
                keys.iter().copied(),
                changed_keys,
                &changes,
                &changed_wires,
            );
            let local_ms = t_local.elapsed().as_secs_f64() * 1000.0;
            let t_remap = iced::time::Instant::now();
            let result = match (snap, base_parts, changed_parts) {
                (Some(category), Some(base_parts), Some(changed_parts)) => {
                    let mut result = base.query_remapped_snap_screen_from_parts_with_category(
                        Arc::clone(&local),
                        &base_slots,
                        base_parts,
                        screen_rect,
                        view_rot,
                        eye,
                        bounds,
                        category,
                    );
                    result.extend_indexed(
                        changed_index.query_remapped_snap_screen_from_parts_with_category(
                            local,
                            &changed_slots,
                            changed_parts,
                            screen_rect,
                            view_rot,
                            eye,
                            bounds,
                            category,
                        ),
                    );
                    result
                }
                _ => {
                    let mut result = base.query_remapped_screen(
                        Arc::clone(&local),
                        &base_slots,
                        screen_rect,
                        view_rot,
                        eye,
                        bounds,
                    );
                    result.extend_indexed(changed_index.query_remapped_screen(
                        local,
                        &changed_slots,
                        screen_rect,
                        view_rot,
                        eye,
                        bounds,
                    ));
                    result
                }
            };
            let remap_ms = t_remap.elapsed().as_secs_f64() * 1000.0;
            let total_ms = t0.elapsed().as_secs_f64() * 1000.0;
            if perf && total_ms >= 50.0 {
                crate::perf_record!(
                    "[perf] interaction-overlay {:>7.1}ms keys={} wires={} query={:.1} changed={:.1} gather={:.1} remap={:.1}",
                    total_ms,
                    keys.len(),
                    result.len(),
                    key_ms,
                    changed_ms,
                    local_ms,
                    remap_ms,
                );
            }
            return result;
        }
        if let Some(index) = self.cached_interaction_index(&wires) {
            direct(&index, wires)
        } else {
            self.unindexed_interaction_candidates(wires, allow_pending_empty, snap, None)
        }
    }

    pub(in crate::scene) fn indexed_interaction_pick_radius(
        &self,
        wires: &Arc<Vec<WireModel>>,
        base_radius_px: f32,
        viewport_height_px: f32,
        include_line_weight: bool,
    ) -> f32 {
        if let Some((base_epoch, base, changes)) = self.interaction_overlay_base() {
            let (_, changed) = self.interaction_overlay_changed_index(base_epoch, &changes);
            base.pick_radius_px(
                changed.pick_radius_px(base_radius_px, viewport_height_px, include_line_weight),
                viewport_height_px,
                include_line_weight,
            )
        } else {
            self.cached_interaction_index(wires)
                .map_or(base_radius_px, |index| {
                    index.pick_radius_px(base_radius_px, viewport_height_px, include_line_weight)
                })
        }
    }

    pub(in crate::scene) fn interaction_source_is_resident(
        &self,
        wires: &Arc<Vec<WireModel>>,
        screen_height_px: f32,
    ) -> bool {
        if self.current_layout == "Model" {
            return Arc::ptr_eq(&self.entity_wires_arc(), wires);
        }
        match self.active_viewport {
            Some(viewport) => {
                let resident = self.model_wires_for_viewport_arc(viewport, screen_height_px);
                Arc::ptr_eq(&resident, wires)
            }
            None => Arc::ptr_eq(&self.paper_sheet_wires_arc(), wires),
        }
    }

    /// Prepare hover indexes after derived geometry is installed during open.
    pub(crate) fn warm_interaction_caches(&self) {
        let perf = crate::perf::enabled();
        let t_start = perf.then(iced::time::Instant::now);
        let t_ei = perf.then(iced::time::Instant::now);
        let _ = self.entity_index();
        let ei_ms = crate::perf::elapsed_ms(t_ei);

        let t_ihi = perf.then(iced::time::Instant::now);
        let _ = self.interaction_handle_index();
        let ihi_ms = crate::perf::elapsed_ms(t_ihi);
        let total_ms = crate::perf::elapsed_ms(t_start);

        if perf {
            crate::perf_record!(
                "[perf] warm_interaction_caches total={:.1}ms entity_index={:.1}ms interaction_handle_index={:.1}ms",
                total_ms,
                ei_ms,
                ihi_ms,
            );
        }
    }

    pub(in crate::scene) fn interaction_handle_index(
        &self,
    ) -> Arc<crate::scene::pick::interaction_index::InteractionHandleIndex> {
        let space_key = self.interaction_space_key();
        {
            let reuse = {
                let cache = self.caches.interaction_handle_index_cache.borrow();
                match cache.as_ref() {
                    Some((epoch, cached_space, index))
                        if *cached_space == space_key
                            && self.category_cache_valid(
                                *epoch,
                                CACHE_CATEGORY_INTERACTION,
                                |handle| {
                                    self.hatches.contains_key(&handle)
                                        || self.meshes.contains_key(&handle)
                                        || self.block_meshes.contains_key(&handle)
                                        || matches!(
                                            self.document.get_entity(handle),
                                            Some(EntityType::Insert(_))
                                        )
                                },
                            ) =>
                    {
                        Some(Arc::clone(index))
                    }
                    _ => None,
                }
            };
            if let Some(index) = reuse {
                if let Some((epoch, _, _)) =
                    self.caches.interaction_handle_index_cache.borrow_mut().as_mut()
                {
                    *epoch = self.geometry_epoch;
                }
                return index;
            }
        }
        let rebuild_started = crate::perf::enabled().then(iced::time::Instant::now);
        let mut entries: Vec<(u64, [f64; 6])> = Vec::new();
        let interactive = self.hatch_interaction_filter();
        for (&handle, hatch) in &self.hatches {
            if !interactive(handle) {
                continue;
            }
            if let Some([min_x, min_y, max_x, max_y]) = hatch_interaction_aabb(hatch) {
                entries.push((handle.value(), [min_x, min_y, 0.0, max_x, max_y, 0.0]));
            }
        }
        let direct_ms = crate::perf::elapsed_ms(rebuild_started);
        for (&handle, hatches) in self.insert_hatches_for_click().iter() {
            for hatch in hatches {
                if let Some([min_x, min_y, max_x, max_y]) = hatch_interaction_aabb(hatch) {
                    entries.push((handle.value(), [min_x, min_y, 0.0, max_x, max_y, 0.0]));
                }
            }
        }
        let inserts_ms = crate::perf::elapsed_ms(rebuild_started);
        let index =
            Arc::new(crate::scene::pick::interaction_index::InteractionHandleIndex::build(entries));
        if rebuild_started.is_some() {
            crate::perf_record!(
                "[perf] handle-index rebuild direct={:.1}ms through-inserts={:.1}ms total={:.1}ms",
                direct_ms,
                inserts_ms,
                crate::perf::elapsed_ms(rebuild_started),
            );
        }
        *self.caches.interaction_handle_index_cache.borrow_mut() =
            Some((self.geometry_epoch, space_key, Arc::clone(&index)));
        index
    }

    /// Cursor-local interaction candidates. `radius_px` is supplied by the
    /// consumer (OSNAP aperture or click tolerance), avoiding the old fixed
    /// 64-pixel neighbourhood. Returned wires stay borrowed from `wires`;
    /// no heavyweight `WireModel` clone occurs.
    pub fn interaction_candidates_near(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        self.interaction_candidates_near_impl(
            wires,
            cursor,
            view_rot,
            eye,
            bounds,
            radius_px,
            false,
            false,
            Some(SnapQueryCategory::Full),
        )
    }

    /// Fast cursor preview query. If the resident index is still being built,
    /// return an empty indexed result instead of scanning the entire drawing;
    /// the settled/click snap path continues to use the complete fallback.
    pub fn interaction_snap_preview_candidates_near(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        self.interaction_candidates_near_impl(
            wires,
            cursor,
            view_rot,
            eye,
            bounds,
            radius_px,
            false,
            true,
            Some(SnapQueryCategory::DiscretePreview),
        )
    }

    pub fn interaction_pick_candidates_near(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        self.interaction_candidates_near_impl(
            wires,
            cursor,
            view_rot,
            eye,
            bounds,
            radius_px,
            self.document.header.lineweight_display,
            false,
            None,
        )
    }

    pub fn interaction_hover_candidates_near(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        self.interaction_candidates_near_impl(
            wires,
            cursor,
            view_rot,
            eye,
            bounds,
            radius_px,
            self.document.header.lineweight_display,
            true,
            None,
        )
    }

    pub(in crate::scene) fn interaction_candidates_near_impl(
        &self,
        wires: Arc<Vec<WireModel>>,
        cursor: glam::DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        radius_px: f32,
        include_line_weight: bool,
        allow_pending_empty: bool,
        snap: Option<SnapQueryCategory>,
    ) -> InteractionCandidates {
        let resident = self.interaction_source_is_resident(&wires, bounds.height);
        // Short-circuits as before: only asked when the source is resident.
        let worthwhile = resident && self.interaction_index_worthwhile(&wires);
        if !resident || !worthwhile {
            if crate::perf::enabled() {
                crate::perf_record!(
                    "[perf] pick-unindexed resident={} worthwhile={} wires={}",
                    resident,
                    worthwhile,
                    wires.len(),
                );
            }
            return self.materialize_stubs_cached(InteractionCandidates::all(wires));
        }
        let flat_ortho = view_rot.z_axis.x.abs() < 1e-9
            && view_rot.z_axis.y.abs() < 1e-9
            && (view_rot.w_axis.w - 1.0).abs() < 1e-6;
        // Flat picks bring marker wires in separately. One large PDSIZE
        // marker should never expand the search for every ordinary edge.
        let radius_px = if snap.is_some() {
            radius_px
        } else {
            self.indexed_interaction_pick_radius(
                &wires,
                radius_px,
                if flat_ortho { 0.0 } else { bounds.height },
                include_line_weight,
            )
        };
        if !flat_ortho {
            let clip = view_rot * (cursor - eye).as_vec3().extend(1.0);
            if !clip.is_finite() || clip.w <= 1e-6 {
                return self.materialize_stubs_cached(self.unindexed_interaction_candidates(
                    wires,
                    allow_pending_empty,
                    snap,
                    None,
                ));
            }
            let ndc = clip.truncate() / clip.w;
            let screen = iced::Point::new(
                (ndc.x + 1.0) * 0.5 * bounds.width,
                (1.0 - ndc.y) * 0.5 * bounds.height,
            );
            let radius = radius_px.max(0.0);
            return self.indexed_interaction_candidates_screen(
                wires,
                [
                    screen.x - radius,
                    screen.y - radius,
                    screen.x + radius,
                    screen.y + radius,
                ],
                view_rot,
                eye,
                bounds,
                allow_pending_empty,
                snap,
            );
        }
        let world_x_px = ((view_rot.x_axis.x * bounds.width * 0.5).powi(2)
            + (view_rot.x_axis.y * bounds.height * 0.5).powi(2))
        .sqrt();
        let world_y_px = ((view_rot.y_axis.x * bounds.width * 0.5).powi(2)
            + (view_rot.y_axis.y * bounds.height * 0.5).powi(2))
        .sqrt();
        let s = world_x_px.min(world_y_px);
        if s <= 1e-6 {
            return self.materialize_stubs_cached(self.unindexed_interaction_candidates(
                wires,
                allow_pending_empty,
                snap,
                None,
            ));
        }
        let radius = radius_px.max(0.0) as f64 / s as f64;
        let query = [
            cursor.x - radius,
            cursor.y - radius,
            cursor.x + radius,
            cursor.y + radius,
        ];
        self.indexed_interaction_candidates_xy(
            wires,
            query,
            allow_pending_empty,
            false,
            snap.is_none(),
            snap,
        )
    }

    /// Shared rectangular broad phase for box/lasso/fence and command windows.
    /// Flat orthographic views query world XY; tilted/perspective views query
    /// projected 3D bounds using the supplied screen rectangle.
    /// Candidates for an area selection: box, crossing, fence and lasso.
    ///
    /// Every caller feeds the result to the box/fence hit tests and to
    /// `interaction_candidate_handles`, none of which snap, so the flat-ortho
    /// path asks for the reduced category set. A caller that needs snapping
    /// belongs on `interaction_candidates_near` instead.
    pub fn interaction_box_candidates(
        &self,
        wires: Arc<Vec<WireModel>>,
        aabb: [f64; 4],
        screen_rect: [f32; 4],
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        let flat_ortho = view_rot.z_axis.x.abs() < 1e-9
            && view_rot.z_axis.y.abs() < 1e-9
            && (view_rot.w_axis.w - 1.0).abs() < 1e-6;
        if flat_ortho && self.interaction_source_is_resident(&wires, bounds.height) {
            if let Some(index) = self.cached_interaction_index(&wires) {
                let pad_px = self.indexed_interaction_pick_radius(&wires, 0.0, bounds.height, true);
                let x = ((view_rot.x_axis.x * bounds.width * 0.5).powi(2)
                    + (view_rot.x_axis.y * bounds.height * 0.5).powi(2))
                .sqrt();
                let y = ((view_rot.y_axis.x * bounds.width * 0.5).powi(2)
                    + (view_rot.y_axis.y * bounds.height * 0.5).powi(2))
                .sqrt();
                let scale = x.min(y);
                if scale > 1e-6 {
                    let pad = pad_px as f64 / scale as f64;
                    let padded = [aabb[0] - pad, aabb[1] - pad, aabb[2] + pad, aabb[3] + pad];
                    return self.materialize_stubs_cached(index.query_xy_box_wires(wires, padded));
                }
            }
        }
        self.interaction_candidates_in_aabb(wires, aabb, screen_rect, view_rot, eye, bounds)
    }

    /// An already prepared resident index is required by incremental box
    /// previews. Its identity also invalidates a preview when an index is
    /// replaced asynchronously during the gesture.
    pub(crate) fn box_preview_interaction_index(
        &self,
        wires: &Arc<Vec<WireModel>>,
        height: f32,
    ) -> Option<Arc<crate::scene::pick::interaction_index::InteractionIndex>> {
        self.interaction_source_is_resident(wires, height)
            .then(|| self.cached_interaction_index(wires))
            .flatten()
    }

    /// Fills and solid meshes have selection paths beyond the wire index.
    /// Recheck them conservatively instead of assuming their wire bounds
    /// describe every filled/instanced part of an entity.
    pub(crate) fn box_preview_non_wire_handles(&self) -> HashSet<Handle> {
        let mut handles: HashSet<Handle> = self.hatches.keys().copied().collect();
        handles.extend(self.insert_hatches_for_click().keys().copied());
        handles.extend(
            self.interaction_meshes_arc()
                .iter()
                .filter_map(|set| set.entity_handle()),
        );
        handles
    }

    pub fn interaction_candidates_in_aabb(
        &self,
        wires: Arc<Vec<WireModel>>,
        aabb: [f64; 4],
        screen_rect: [f32; 4],
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> crate::scene::pick::interaction_index::InteractionCandidates {
        let flat_ortho = view_rot.z_axis.x.abs() < 1e-9
            && view_rot.z_axis.y.abs() < 1e-9
            && (view_rot.w_axis.w - 1.0).abs() < 1e-6;
        if !self.interaction_source_is_resident(&wires, bounds.height)
            || !self.interaction_index_worthwhile(&wires)
        {
            return self.materialize_stubs_cached(
                crate::scene::pick::interaction_index::InteractionCandidates::all(wires),
            );
        }
        let pad_px = self.indexed_interaction_pick_radius(&wires, 0.0, bounds.height, true);
        if flat_ortho {
            let world_x_px = ((view_rot.x_axis.x * bounds.width * 0.5).powi(2)
                + (view_rot.x_axis.y * bounds.height * 0.5).powi(2))
            .sqrt();
            let world_y_px = ((view_rot.y_axis.x * bounds.width * 0.5).powi(2)
                + (view_rot.y_axis.y * bounds.height * 0.5).powi(2))
            .sqrt();
            let scale = world_x_px.min(world_y_px);
            let pad = if scale > 1e-6 {
                pad_px as f64 / scale as f64
            } else {
                0.0
            };
            self.indexed_interaction_candidates_xy(
                wires,
                [aabb[0] - pad, aabb[1] - pad, aabb[2] + pad, aabb[3] + pad],
                false,
                true,
                false,
                None,
            )
        } else {
            self.indexed_interaction_candidates_screen(
                wires,
                [
                    screen_rect[0] - pad_px,
                    screen_rect[1] - pad_px,
                    screen_rect[2] + pad_px,
                    screen_rect[3] + pad_px,
                ],
                view_rot,
                eye,
                bounds,
                false,
                None,
            )
        }
    }

    pub fn interaction_candidate_handles(
        &self,
        candidates: &crate::scene::pick::interaction_index::InteractionCandidates,
    ) -> Option<HashSet<Handle>> {
        let handles = self.interaction_candidate_handles_all(candidates);
        // Reference-only entities (API v9) are plugin-owned display geometry:
        // their fills, solids and instanced meshes are not pickable, so the
        // click/hover fallback paths that consume this set skip them and a
        // click passes through to the editable entity underneath.
        handles.map(|mut handles| {
            handles.retain(|h| !self.is_reference_only(*h));
            handles
        })
    }

    pub(in crate::scene) fn interaction_candidate_handles_all(
        &self,
        candidates: &crate::scene::pick::interaction_index::InteractionCandidates,
    ) -> Option<HashSet<Handle>> {
        let mut handles: HashSet<Handle> = candidates
            .iter()
            .filter_map(|wire| Self::handle_from_wire_name(&wire.name))
            .collect();
        if candidates.is_indexed()
            && candidates.query_aabb().is_none()
            && candidates.screen_query().is_none()
        {
            return Some(handles);
        }
        if let Some(aabb) = candidates.query_aabb() {
            let index = self.entity_index();
            handles.extend(index.tree.query_rect(aabb));
            handles.extend(
                self.interaction_handle_index()
                    .query_xy(aabb)
                    .into_iter()
                    .map(Handle::new),
            );
            return Some(handles);
        }
        let (screen_rect, view_rot, eye, bounds) = candidates.screen_query()?;
        handles.extend(
            self.interaction_handle_index()
                .query_screen(screen_rect, view_rot, eye, bounds)
                .into_iter()
                .map(Handle::new),
        );
        Some(handles)
    }

    /// Handles whose geometry meets a world-XY box. The wire candidates are a
    /// broad phase: a wire without a finite box (an INSERT's snap carrier, a
    /// dimension part) comes back for any box, and an unindexed set returns
    /// every wire. STRETCH took all of them, so every block in the drawing
    /// joined it and vanished from its preview (#1891).
    pub fn interaction_handles_in_world_aabb(&self, aabb: [f64; 4]) -> HashSet<Handle> {
        let wires = self.hit_test_wires();
        let candidates =
            self.indexed_interaction_candidates_xy(wires, aabb, false, false, false, None);
        let mut handles: HashSet<Handle> = candidates
            .iter()
            .filter(|wire| wire_meets_xy_box(wire, aabb))
            .filter_map(|wire| Self::handle_from_wire_name(&wire.name))
            .collect();
        let insert_hatches = self.insert_hatches_for_click();
        let meshes = self.interaction_meshes_arc();
        let interactive = self.hatch_interaction_filter();
        handles.extend(
            self.interaction_handle_index()
                .query_xy(aabb)
                .into_iter()
                .filter_map(|value| {
                    let handle = Handle::new(value);
                    (interactive(handle) || insert_hatches.contains_key(&handle)).then_some(handle)
                }),
        );
        handles.extend(meshes.iter().filter_map(|set| {
            let handle = set.entity_handle()?;
            let bounds = mesh_interaction_aabb(set)?;
            (bounds[3] >= aabb[0]
                && bounds[0] <= aabb[2]
                && bounds[4] >= aabb[1]
                && bounds[1] <= aabb[3])
                .then_some(handle)
        }));
        handles
    }

    /// Top-level solid handles caught by a rectangular selection box.
    pub fn mesh_box_hit(
        &self,
        a: iced::Point,
        b: iced::Point,
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        _candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let boxes = self.mesh_pick_boxes(&meshes);
        let handles: Vec<Handle> = lookup.keys().copied().collect();
        let mut out = Vec::new();
        for handle in handles {
            if matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let test = |set: &MeshLodSet| {
                set.geometry_lods().first().is_some_and(|mesh| {
                    // A solid clear inside or clear outside the box is told
                    // from the box around its vertices.
                    let told = boxes
                        .get(&(std::ptr::from_ref(mesh) as usize))
                        .and_then(|local| {
                            pick::hit_test::box_hit_from_bounds(
                                a,
                                b,
                                local,
                                set.instance_transform,
                                view_rot,
                                eye,
                                bounds,
                            )
                        });
                    told.unwrap_or_else(|| {
                        !pick::hit_test::mesh_box_hit(
                            a,
                            b,
                            crossing,
                            std::iter::once((handle, mesh, set.instance_transform)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                    })
                })
            };
            let mut sets = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize));
            let hit = if crossing {
                sets.any(test)
            } else {
                let mut count = 0;
                sets.all(|set| {
                    count += 1;
                    test(set)
                }) && count > 0
            };
            if hit {
                out.push(handle);
            }
        }
        out
    }

    /// Top-level solid handles caught by a lasso polygon.
    /// Everything a picked path takes, gathered from every kind of geometry the
    /// selection can reach: wires, hatches, hatches inside blocks, meshes and
    /// block meshes, narrowed first to what the path's own bounds can touch.
    ///
    /// `open_fence` distinguishes the two gestures that arrive here. A fence is
    /// a line drawn through the drawing and takes only what it actually cuts; a
    /// polygon closes back on itself and can also take what it encloses, or
    /// merely touches when `crossing`. The lasso and the Fence / WPolygon /
    /// CPolygon keywords both come through this, so they cannot drift apart on
    /// which geometry they remember to look at. (#596)
    #[allow(clippy::too_many_arguments)]
    pub fn path_hit_handles(
        &self,
        poly_pts: &[iced::Point],
        crossing: bool,
        open_fence: bool,
        all_wires: std::sync::Arc<Vec<WireModel>>,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        to_world: impl Fn(iced::Point) -> glam::DVec3,
    ) -> Vec<Handle> {
        let world_aabb = poly_pts.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |mut aabb, &point| {
                let world = to_world(point);
                aabb[0] = aabb[0].min(world.x);
                aabb[1] = aabb[1].min(world.y);
                aabb[2] = aabb[2].max(world.x);
                aabb[3] = aabb[3].max(world.y);
                aabb
            },
        );
        let screen_rect = poly_pts.iter().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |mut rect, point| {
                rect[0] = rect[0].min(point.x);
                rect[1] = rect[1].min(point.y);
                rect[2] = rect[2].max(point.x);
                rect[3] = rect[3].max(point.y);
                rect
            },
        );
        let area_candidates = self.interaction_candidates_in_aabb(
            all_wires,
            world_aabb,
            screen_rect,
            view_rot,
            eye,
            bounds,
        );
        let candidate_handles = self.interaction_candidate_handles(&area_candidates);
        let wire_hits = if open_fence {
            crate::scene::pick::hit_test::poly_fence_hit(
                poly_pts,
                &area_candidates,
                view_rot,
                eye,
                bounds,
            )
        } else {
            crate::scene::pick::hit_test::poly_hit(
                poly_pts,
                crossing,
                &area_candidates,
                view_rot,
                eye,
                bounds,
            )
        };
        let mut handles: Vec<Handle> = wire_hits
            .into_iter()
            .filter_map(Self::handle_from_wire_name)
            .collect();
        // An open fence has no interior, so the area-based geometry can only be
        // reached by the polygon gestures.
        if !open_fence {
            handles.extend(crate::scene::pick::hit_test::poly_hit_hatch(
                poly_pts,
                crossing,
                &self.visible_hatches_for_click(candidate_handles.as_ref()),
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            ));
            handles.extend(crate::scene::pick::hit_test::poly_hit_insert_hatch(
                poly_pts,
                crossing,
                self.insert_hatches_for_click().as_ref(),
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            ));
            handles.extend(self.mesh_poly_hit(
                poly_pts,
                crossing,
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            ));
            handles.extend(self.block_mesh_poly_hit(
                poly_pts,
                crossing,
                view_rot,
                eye,
                bounds,
                candidate_handles.as_ref(),
            ));
        }
        handles.retain(|&h| self.passes_selection_filter(h) && !self.is_reference_only(h));
        handles
    }

    pub fn mesh_poly_hit(
        &self,
        poly: &[iced::Point],
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        _candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let boxes = self.mesh_pick_boxes(&meshes);
        let handles: Vec<Handle> = lookup.keys().copied().collect();
        let mut out = Vec::new();
        for handle in handles {
            if matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let test = |set: &MeshLodSet| {
                set.geometry_lods().first().is_some_and(|mesh| {
                    // A solid clear of the rectangle around the lasso is told
                    // from the box around its vertices.
                    let missed =
                        boxes
                            .get(&(std::ptr::from_ref(mesh) as usize))
                            .is_some_and(|local| {
                                pick::hit_test::poly_misses_bounds(
                                    poly,
                                    local,
                                    set.instance_transform,
                                    view_rot,
                                    eye,
                                    bounds,
                                )
                            });
                    !missed
                        && !pick::hit_test::mesh_poly_hit(
                            poly,
                            crossing,
                            std::iter::once((handle, mesh, set.instance_transform)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                })
            };
            let mut sets = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize));
            let hit = if crossing {
                sets.any(test)
            } else {
                let mut count = 0;
                sets.all(|set| {
                    count += 1;
                    test(set)
                }) && count > 0
            };
            if hit {
                out.push(handle);
            }
        }
        out
    }

    /// Front-most solid under the cursor across BOTH top-level solid meshes
    /// (keyed by their own handle) and block-internal solid instances (keyed
    /// by the parent INSERT). Combining them in one depth-sorted test means a
    /// block in front of a stray solid wins, instead of the solid always
    /// taking priority by virtue of being tried first.
    pub fn solid_click_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> Option<Handle> {
        self.solid_hit(cursor, view_rot, eye, bounds, candidate_handles, false)
    }

    /// Normal object selection for shaded 3D bodies is edge based. This keeps
    /// the broad face interior available to modelling commands without making
    /// it part of ordinary object picking.
    pub fn solid_edge_click_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
        tolerance_px: f32,
    ) -> Option<Handle> {
        let meshes = self.interaction_meshes_arc();
        pick::hit_test::mesh_edge_click_hit(
            cursor,
            meshes.iter().filter_map(|set| {
                let handle = set.entity_handle()?;
                if candidate_handles.is_some_and(|handles| !handles.contains(&handle)) {
                    return None;
                }
                let (edges, edges_low) = set.geometry_edges();
                (!edges.is_empty()).then_some((handle, edges, edges_low, set.instance_transform))
            }),
            view_rot,
            eye,
            bounds,
            tolerance_px,
        )
    }

    pub fn solid_edge_hover_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
        tolerance_px: f32,
    ) -> Option<Handle> {
        self.solid_edge_click_hit(
            cursor,
            view_rot,
            eye,
            bounds,
            candidate_handles,
            tolerance_px,
        )
    }

    pub fn solid_click_point_for(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        target: Handle,
    ) -> Option<glam::DVec3> {
        let meshes = self.interaction_meshes_arc();
        pick::hit_test::mesh_click_point(
            cursor,
            meshes.iter().filter_map(|set| {
                (set.entity_handle()? == target).then_some(())?;
                let mesh = set.geometry_lods().first()?;
                let bvh = set.geometry_triangle_bvh(0);
                Some((
                    target,
                    mesh,
                    bvh,
                    set.instance_transform,
                    mesh_interaction_aabb(set)?,
                ))
            }),
            view_rot,
            eye,
            bounds,
        )
    }

    /// The solid under the cursor and where the pick ray meets it.
    pub fn solid_click_hit_point(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Option<(Handle, glam::DVec3)> {
        let meshes = self.interaction_meshes_arc();
        pick::hit_test::mesh_click_handle_point(
            cursor,
            meshes.iter().filter_map(|set| {
                let handle = set.entity_handle()?;
                let mesh = set.geometry_lods().first()?;
                let bvh = set.geometry_triangle_bvh(0);
                Some((
                    handle,
                    mesh,
                    bvh,
                    set.instance_transform,
                    mesh_interaction_aabb(set)?,
                ))
            }),
            view_rot,
            eye,
            bounds,
        )
    }

    pub fn solid_planar_face_normal_at(
        &mut self,
        handle: Handle,
        pick: glam::DVec3,
    ) -> Option<glam::DVec3> {
        self.restore_solid_models(&[handle]);
        let body = self.solid_models.get(&handle)?;
        let face = model::solid_model::nearest_planar_face(body, pick.to_array())?;
        model::solid_model::planar_face_normal(body, face).map(glam::DVec3::from_array)
    }

    pub fn view_center_surface_pivot(&self, bounds: iced::Rectangle) -> Option<glam::DVec3> {
        self.surface_pivot_at(
            bounds,
            iced::Point::new(bounds.width * 0.5, bounds.height * 0.5),
        )
    }

    /// The UCS of the planar solid face under `local` (relative to `bounds`),
    /// Z along the face normal, and the picked point on the face.
    pub fn planar_face_ucs_at(
        &mut self,
        bounds: iced::Rectangle,
        local: iced::Point,
    ) -> Option<(document::tables::Ucs, glam::DVec3)> {
        let camera = self.camera.borrow();
        let view_rot = camera.view_proj_rte(bounds);
        let eye = camera.eye();
        drop(camera);
        let (handle, pick) = self.solid_click_hit_point(local, view_rot, eye, bounds)?;
        self.restore_solid_models(&[handle]);
        let body = self.solid_models.get(&handle)?;
        let face = crate::scene::model::solid_model::nearest_planar_face(body, pick.to_array())?;
        crate::scene::model::solid_model::planar_face_ucs(body, face, pick.to_array())
            .map(|ucs| (ucs, pick))
    }

    /// Outline the planar solid face under `local` (relative to `bounds`), the
    /// face a Dynamic UCS would align to; `None` clears the outline.
    pub fn set_ducs_face_at(&mut self, at: Option<(iced::Rectangle, iced::Point)>) {
        let face = at.and_then(|(bounds, local)| {
            let camera = self.camera.borrow();
            let view_rot = camera.view_proj_rte(bounds);
            let eye = camera.eye();
            drop(camera);
            let (handle, pick) = self.solid_click_hit_point(local, view_rot, eye, bounds)?;
            self.restore_solid_models(&[handle]);
            let body = self.solid_models.get(&handle)?;
            let face =
                crate::scene::model::solid_model::nearest_planar_face(body, pick.to_array())?;
            Some((handle, face))
        });
        if face == self.ducs_face {
            return;
        }
        self.ducs_face = face;
        self.ducs_face_wires = face
            .and_then(|(handle, face)| {
                let body = self.solid_models.get(&handle)?;
                Some(
                    crate::scene::model::solid_model::face_outline(body, face)
                        .into_iter()
                        .map(|points| {
                            let mut wire = WireModel::solid_f64(
                                format!("ducs-face:{}", handle.value()),
                                points,
                                WireModel::HOVER,
                                false,
                            );
                            wire.set_fixed_screen_width(2.0);
                            wire
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();
        self.bump_hover();
    }

    /// The nearest drawn surface under `local` (relative to `bounds`): a solid
    /// or mesh face, or a wire's shaded triangles (3DFACE, filled SOLID,
    /// thickness walls) — drawings made only of faces must lock too.
    pub fn surface_pivot_at(
        &self,
        bounds: iced::Rectangle,
        local: iced::Point,
    ) -> Option<glam::DVec3> {
        let camera = self.camera.borrow();
        let view_rot = camera.view_proj_rte(bounds);
        let eye = camera.eye();
        drop(camera);
        let (near, _) = pick::hit_test::pick_ray(local, view_rot, eye, bounds)?;
        let wires = self.interaction_pick_candidates_near(
            self.hit_test_wires(),
            near,
            view_rot,
            eye,
            bounds,
            1.0,
        );
        let meshes = self.interaction_meshes_arc();
        pick::hit_test::surface_click_point(
            local,
            meshes.iter().filter_map(|set| {
                let handle = set.entity_handle()?;
                let mesh = set.geometry_lods().first()?;
                let bvh = set.geometry_triangle_bvh(0);
                Some((
                    handle,
                    mesh,
                    bvh,
                    set.instance_transform,
                    mesh_interaction_aabb(set)?,
                ))
            }),
            &wires,
            view_rot,
            eye,
            bounds,
        )
    }

    /// Hover may use the coarsest cached solid LOD. Click selection keeps the
    /// full-resolution mesh, while rollover only needs a stable parent handle
    /// and must stay within an interactive frame budget.
    pub fn solid_hover_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidate_handles: Option<&HashSet<Handle>>,
    ) -> Option<Handle> {
        self.solid_hit(cursor, view_rot, eye, bounds, candidate_handles, true)
    }

    pub(in crate::scene) fn solid_hit(
        &self,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        _candidate_handles: Option<&HashSet<Handle>>,
        coarse_lod: bool,
    ) -> Option<Handle> {
        // Reuse the renderer's cached top-level and block-instance mesh set.
        let meshes = self.interaction_meshes_arc();
        let sets: Vec<(Handle, &MeshLodSet)> = meshes
            .iter()
            .filter_map(|set| {
                let handle = set.entity_handle()?;
                Some((handle, set))
            })
            .collect();
        pick::hit_test::mesh_click_hit(
            cursor,
            sets.iter().filter_map(|(handle, set)| {
                let lods = set.geometry_lods();
                if lods.is_empty() {
                    return None;
                }
                let lod_idx = if coarse_lod { lods.len() - 1 } else { 0 };
                let mesh = lods.get(lod_idx)?;
                let bvh = set.geometry_triangle_bvh(lod_idx);
                Some((
                    *handle,
                    mesh,
                    bvh,
                    set.instance_transform,
                    mesh_interaction_aabb(set)?,
                ))
            }),
            view_rot,
            eye,
            bounds,
        )
    }

    /// Parent INSERT handles whose block-internal solid meshes fall in a
    /// rectangular selection box. A block whose visible body is a solid has
    /// no wires to catch, so box/lasso selection must test its instanced
    /// meshes too.
    pub fn block_mesh_box_hit(
        &self,
        a: iced::Point,
        b: iced::Point,
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        _candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let boxes = self.mesh_pick_boxes(&meshes);
        let handles: Vec<Handle> = lookup.keys().copied().collect();
        let mut out = Vec::new();
        for handle in handles {
            if !matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let test = |set: &MeshLodSet| {
                set.geometry_lods().first().is_some_and(|mesh| {
                    // A solid clear inside or clear outside the box is told
                    // from the box around its vertices.
                    let told = boxes
                        .get(&(std::ptr::from_ref(mesh) as usize))
                        .and_then(|local| {
                            pick::hit_test::box_hit_from_bounds(
                                a,
                                b,
                                local,
                                set.instance_transform,
                                view_rot,
                                eye,
                                bounds,
                            )
                        });
                    told.unwrap_or_else(|| {
                        !pick::hit_test::mesh_box_hit(
                            a,
                            b,
                            crossing,
                            std::iter::once((handle, mesh, set.instance_transform)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                    })
                })
            };
            let mut sets = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize));
            let hit = if crossing {
                sets.any(test)
            } else {
                let mut count = 0;
                sets.all(|set| {
                    count += 1;
                    test(set)
                }) && count > 0
            };
            if hit {
                out.push(handle);
            }
        }
        out
    }

    /// Parent INSERT handles whose block-internal solid meshes fall in a lasso.
    pub fn block_mesh_poly_hit(
        &self,
        poly: &[iced::Point],
        crossing: bool,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        _candidate_handles: Option<&HashSet<Handle>>,
    ) -> Vec<Handle> {
        if self.block_meshes.is_empty() {
            return Vec::new();
        }
        let meshes = self.interaction_meshes_arc();
        let lookup = self.mesh_pick_lookup(&meshes);
        let boxes = self.mesh_pick_boxes(&meshes);
        let handles: Vec<Handle> = lookup.keys().copied().collect();
        let mut out = Vec::new();
        for handle in handles {
            if !matches!(
                self.document.get_entity(handle),
                Some(EntityType::Insert(_))
            ) {
                continue;
            }
            let Some(indices) = lookup.get(&handle) else {
                continue;
            };
            let test = |set: &MeshLodSet| {
                set.geometry_lods().first().is_some_and(|mesh| {
                    // A solid clear of the rectangle around the lasso is told
                    // from the box around its vertices.
                    let missed =
                        boxes
                            .get(&(std::ptr::from_ref(mesh) as usize))
                            .is_some_and(|local| {
                                pick::hit_test::poly_misses_bounds(
                                    poly,
                                    local,
                                    set.instance_transform,
                                    view_rot,
                                    eye,
                                    bounds,
                                )
                            });
                    !missed
                        && !pick::hit_test::mesh_poly_hit(
                            poly,
                            crossing,
                            std::iter::once((handle, mesh, set.instance_transform)),
                            view_rot,
                            eye,
                            bounds,
                        )
                        .is_empty()
                })
            };
            let mut sets = indices
                .iter()
                .filter_map(|&index| meshes.get(index as usize));
            let hit = if crossing {
                sets.any(test)
            } else {
                let mut count = 0;
                sets.all(|set| {
                    count += 1;
                    test(set)
                }) && count > 0
            };
            if hit {
                out.push(handle);
            }
        }
        out
    }

}
