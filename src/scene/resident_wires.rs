use super::*;

impl Scene {
    /// Unified static-hold wire builder — the ONE tessellation path every
    /// space renders through (Model tiles, BEDIT, the paper sheet block, and
    /// paper content viewports): the FULL, un-culled, LOD-free set of `block`,
    /// held resident per `(block, ambient bg, anno, frozen)` key and rebuilt
    /// only when `geometry_epoch` changes. Mints a stable
    /// [`WIRE_CONTENT_GEN`] id per build (relayed via `last_model_wire_gen`)
    /// so the GPU upload gate and `render_signature` skip unchanged content.
    /// Whether the per-viewport annotation scale changes wire output at all
    /// Cached per geometry epoch; the scan short-circuits on the first
    /// annotative entity. PSLTSCALE no longer participates because dash scaling
    /// is applied in the wire shader from the viewport uniform.
    pub(crate) fn annotation_affects_wires(&self) -> bool {
        if let Some((epoch, v)) = self.annotation_affects_wires.get() {
            if epoch == self.geometry_epoch {
                return v;
            }
            // Skip the whole-document annotative scan when nothing annotative
            // changed since (PSLTSCALE toggles route through a full delta, which
            // fails this check and forces the recompute below).
            if self.category_cache_valid(epoch, CACHE_CATEGORY_ANNOTATIVE, |h| {
                self.document
                    .get_entity(h)
                    .map(|e| crate::scene::annotative::is_annotative(&self.document, e))
                    .unwrap_or(false)
            }) {
                self.annotation_affects_wires
                    .set(Some((self.geometry_epoch, v)));
                return v;
            }
        }
        let t_scan = crate::perf::enabled().then(iced::time::Instant::now);
        let v = self
            .document
            .entities()
            .any(|e| crate::scene::annotative::is_annotative(&self.document, e));
        if let Some(t) = t_scan {
            crate::perf_record!(
                "[perf] anno-scan {:.1}ms entities={} result={}",
                t.elapsed().as_secs_f64() * 1000.0,
                self.document.entity_count(),
                v
            );
        }
        self.annotation_affects_wires
            .set(Some((self.geometry_epoch, v)));
        v
    }

    /// Whether `wires` is one of the resident sets themselves, which only the
    /// scene may hold (see [`Self::try_resident_patch`]), and not a copy made
    /// from one.
    pub(in crate::scene) fn is_resident_wire_set(&self, wires: &Arc<Vec<WireModel>>) -> bool {
        self.resident_wire_sets
            .borrow()
            .values()
            .any(|set| Arc::ptr_eq(&set.wires, wires))
    }

    pub(crate) fn resident_wires_for(
        &self,
        block: Handle,
        anno_scale_override: Option<f32>,
        annotation_scale_handle: Option<Handle>,
        frozen_layers: Option<&HashSet<Handle>>,
        style_viewport: Option<Handle>,
    ) -> Arc<Vec<WireModel>> {
        if self.constraint_shading_active() && self.block_edit_block == Some(block) {
            let _ = self.block_constraint_report(block);
        }
        // Normalize an inert anno override away so distinct viewport scales
        // share one resident set when annotation can't change the wires.
        let anno_scale_override = if self.annotation_affects_wires() {
            anno_scale_override
        } else {
            None
        };
        // Ambient bg is part of the key: the same block tessellated against
        // the model vs paper background adapts wire colors differently.
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let all_visible = self.annotation_all_visible();
        let key = self.resident_wire_key(
            block,
            bg,
            anno_scale_override,
            annotation_scale_handle,
            all_visible,
            frozen_layers,
            style_viewport,
        ) ^ if self.constraint_shading_active() {
            0xC057_A705_0000_0001
        } else {
            0
        };
        {
            let sets = self.resident_wire_sets.borrow();
            if let Some(set) = sets.get(&key) {
                if set.epoch == self.geometry_epoch {
                    set.used.set(resident_touch());
                    self.last_model_wire_gen.set(set.gen);
                    return Arc::clone(&set.wires);
                }
            }
        }
        // Incremental: bring the cached set up to date by replaying just the
        // changed entities into it, instead of re-cloning + re-sorting the whole
        // set. Falls back to the full build below when it can't (no cache entry,
        // journal un-replayable, or a structural assumption violated).
        // A live section changes the displayed wires of every intersected
        // solid, even when the delta journal contains only the section entity.
        // Rebuild the resident set in that case so an incremental patch cannot
        // retain the source solid's uncut edge cache.
        if self.active_live_section(block).is_none() && !self.constraint_shading_active() {
            let t_patch = crate::perf::enabled().then(iced::time::Instant::now);
            let patched = self.try_resident_patch(
                key,
                block,
                bg,
                anno_scale_override,
                annotation_scale_handle,
                all_visible,
                frozen_layers,
                style_viewport,
            );
            if let Some(t) = t_patch {
                crate::perf_record!(
                    "[perf] resident-patch-attempt {:.1}ms hit={}",
                    t.elapsed().as_secs_f64() * 1000.0,
                    patched.is_some()
                );
            }
            if let Some(arc) = patched {
                return arc;
            }
        }
        // Build once: full tessellation, no cull, no zoom LOD — the resident
        // set is zoom-independent (GPU analytical circles/arcs/ellipses).
        let t_tess = iced::time::Instant::now();
        let mut atlas_gen = crate::scene::text::sdf_atlas::generation();
        self.building_resident_set.set(true);
        let mut wires = self.wires_for_block_culled(
            block,
            None,
            None,
            frozen_layers,
            anno_scale_override,
            annotation_scale_handle,
            all_visible,
            style_viewport,
            None,
        );
        // Baking a drawing's glyphs for the first time can grow the SDF atlas
        // part-way through this pass (a CJK sheet set bakes thousands of
        // tiles). Text tessellated before the growth carries UVs that now
        // address the wrong tiles — visible as scrambled glyphs — and this
        // set is cached by geometry epoch, so nothing would ever re-lay it
        // out. The memo guard already folds the generation in, so rebuilding
        // once the atlas has settled re-tessellates only the stale text.
        for _ in 0..3 {
            let now = crate::scene::text::sdf_atlas::generation();
            if now == atlas_gen {
                break;
            }
            atlas_gen = now;
            // Block definitions tessellated earlier in this pass hold the
            // stale quads too, and their cache is keyed by block epoch (which
            // has not moved), so drop them or the rebuild would reuse them.
            self.block_defn_cache.borrow_mut().clear();
            // The pass just built backs the memo meanwhile, so only its text
            // (dropped as stale) is tessellated again, not the drawing.
            let previous = Arc::new(std::mem::take(&mut wires));
            if self.uses_resident_memo(block, frozen_layers) && self.resident_runs_match_memo(block)
            {
                if let Some(layout) = Self::resident_wire_layout(&previous) {
                    self.bind_memo_to_set(&previous, &layout.ranges);
                }
            }
            wires = self.wires_for_block_culled(
                block,
                None,
                None,
                frozen_layers,
                anno_scale_override,
                annotation_scale_handle,
                all_visible,
                style_viewport,
                None,
            );
        }
        self.building_resident_set.set(false);
        let perf = crate::perf::enabled();
        let t_post = perf.then(iced::time::Instant::now);
        // Synthesized nonprint markers (geo-location daisy) live in model space
        // only and are derived from document objects, not entities — append them
        // to the freshly built resident set (incremental patches preserve them).
        if block == self.model_space_block_handle() {
            self.append_scene_markers(&mut wires, bg);
        }
        let markers_ms = crate::perf::elapsed_ms(t_post);
        let t_fade = perf.then(iced::time::Instant::now);
        self.apply_refedit_fade(&mut wires, bg);
        wires.iter_mut().for_each(WireModel::compact_resident_geometry);
        let fade_ms = crate::perf::elapsed_ms(t_fade);
        let t_layout = perf.then(iced::time::Instant::now);
        let layout = Self::resident_wire_layout(&wires);
        if perf {
            crate::perf_record!(
                "[perf] resident-post markers={:.1}ms fade={:.1}ms layout={:.1}ms wires={}",
                markers_ms,
                fade_ms,
                crate::perf::elapsed_ms(t_layout),
                wires.len(),
            );
        }
        self.last_tess_ms
            .set(t_tess.elapsed().as_secs_f32() * 1000.0);
        self.last_tess_wires.set(wires.len());
        let arc = Arc::new(wires);
        // The memo keeps one copy of what this set holds; read before the
        // previous set is evicted below, which may hold runs it still needs.
        let t_memo = perf.then(iced::time::Instant::now);
        if self.uses_resident_memo(block, frozen_layers) {
            match &layout {
                Some(layout) if self.resident_runs_match_memo(block) => {
                    self.bind_memo_to_set(&arc, &layout.ranges);
                }
                _ => self.detach_memo_backing(),
            }
        }
        if let Some(t) = t_memo {
            crate::perf_record!(
                "[perf] memo-bind {:.1}ms wires={}",
                t.elapsed().as_secs_f64() * 1000.0,
                arc.len()
            );
        }
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        // A full rebuild of the Model set — the GPU arena must rebuild too, not
        // apply a stale patch, so drop this set's pending handoff. Only this
        // viewport's: another viewport's set is a separate chain with its own
        // generations, and clearing it here is what left every viewport
        // patchless on a two-viewport layout.
        if block == self.model_space_block_handle() {
            self.model_wire_gpu_patch
                .borrow_mut()
                .remove(&style_viewport);
        }
        // Every content viewport of the shown layout holds a set of its own;
        // a cap below their count cleared them all each frame and rebuilt
        // every viewport's wires on every redraw (#1769).
        let shown = if self.current_layout == "Model" {
            0
        } else {
            self.paper_viewport_handles().2.len()
        };
        let cur_epoch = self.geometry_epoch;
        let mut sets = self.resident_wire_sets.borrow_mut();
        let evicted = sets.insert(
            key,
            ResidentWireSet {
                estimated_bytes: Self::resident_wire_bytes(&arc),
                epoch: cur_epoch,
                gen,
                used: std::cell::Cell::new(resident_touch()),
                wires: Arc::clone(&arc),
                layout,
            },
        );
        self.prune_resident_wire_sets(&mut sets, shown);
        // Tearing down the previous set walks every wire's String, layer Arc
        // and point Vec (~39 ms for 200k wires); drop it off the rebuild path.
        if let Some(evicted) = evicted {
            reaper::reap(evicted);
        }
        arc
    }

    pub(crate) fn low_memory_viewing() -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::cli::low_memory_viewing()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub(crate) fn resident_wire_bytes(wires: &[WireModel]) -> usize {
        wires.iter().map(WireModel::estimated_resident_bytes).sum()
    }

    /// Idle sets are bounded by bytes as well as count. Keep the visible
    /// layout and its content viewports even when one drawing exceeds budget.
    pub(crate) fn prune_resident_wire_sets(&self, sets: &mut HashMap<u64, ResidentWireSet>, shown: usize) {
        let floor = self.geometry_journal_floor.get();
        let mut bytes: usize = sets.values().map(|set| set.estimated_bytes).sum();
        let cap = shown.max(4) + 2;
        let protected = shown + 1;
        let budget = if Self::low_memory_viewing() { 128 } else { 512 } << 20;

        // Journal-compatible stale sets can be patched on the next use.
        while let Some((&key, _)) = sets
            .iter()
            .filter(|(_, set)| {
                set.epoch < floor || sets.len() > cap || (sets.len() > protected && bytes > budget)
            })
            .min_by_key(|(_, set)| (set.epoch >= floor, set.used.get()))
        {
            if let Some(evicted) = sets.remove(&key) {
                bytes = bytes.saturating_sub(evicted.estimated_bytes);
                let source = Arc::as_ptr(&evicted.wires) as usize;
                self.interaction_index_cache.borrow_mut().retain(|(_, ptr, _, _)| *ptr != source);
                reaper::reap(evicted);
            }
        }
    }

    pub(crate) fn resident_wire_key(
        &self,
        block: Handle,
        bg: [f32; 4],
        anno_scale_override: Option<f32>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        frozen_layers: Option<&HashSet<Handle>>,
        style_viewport: Option<Handle>,
    ) -> u64 {
        let mut key: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix =
            |value: u64| key = key.rotate_left(17) ^ value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        mix(block.value());
        for component in bg {
            mix(component.to_bits() as u64);
        }
        mix(anno_scale_override
            .map(|scale| scale.to_bits() as u64)
            .unwrap_or(u64::MAX));
        mix(annotation_scale_handle
            .map(|handle| handle.value())
            .unwrap_or(0));
        mix(all_visible as u64);
        mix(self.viewport_style_key(style_viewport));
        mix(self
            .viewport_complex_lt
            .get()
            .map_or(u64::MAX, |factor| factor.to_bits() as u64));
        // quantized_wpp removed: GPU analytical rendering for circles/arcs/
        // ellipses makes tessellation zoom-independent, so the resident set is
        // shared across all zoom levels.
        match frozen_layers {
            Some(frozen) => {
                let mut signature = 0u64;
                for handle in frozen {
                    signature ^= handle.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
                }
                mix(signature);
                mix(frozen.len() as u64);
            }
            None => mix(u64::MAX - 1),
        }
        // LAYWALK changes what the gate admits, so a walked build must get
        // its own key and never reuse (or be reused by) the un-walked set.
        match self.interaction_layer_walk() {
            Some(visible) => {
                let mut signature = 0u64;
                for handle in visible {
                    signature ^= handle.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
                }
                mix(signature);
                mix(visible.len() as u64 + 1);
            }
            None => mix(u64::MAX - 2),
        }
        key
    }

    /// Quantize world units per pixel (`wpp`) into half-octave bands (~1.414x zoom ratio).
    ///
    /// This provides discrete, stable zoom bands so that panning (where wpp is constant)
    /// and small subpixel viewport drifts or mouse-wheel motions within a band do not
    /// cause re-tessellation or GPU buffer re-uploads, while zooming past a band threshold
    /// smoothly scales circle tessellation detail.
    pub fn quantize_wpp(wpp: Option<f32>) -> Option<f32> {
        let w = wpp.filter(|&w| w.is_finite() && w > 0.0)?;
        let step = (w.log2() * 2.0).floor() * 0.5;
        Some(2.0f32.powf(step))
    }

    /// The GPU wire-arena handoff for a viewport whose content id is `gen`:
    /// `(prev_gen, changed handles)` when the Model set reached `gen` via an
    /// incremental resident patch, else `None`. Read (not consumed) so every
    /// Model tile applies the same patch; gen-matching keeps it from applying to
    /// a viewport that rebuilt independently.
    pub(crate) fn model_wire_patch_for(&self, gen: u64) -> Option<(u64, Arc<WireGpuPatch>)> {
        let mut pending = self.model_wire_gpu_patch.borrow_mut();
        // The caller knows only the content generation it is rendering, not
        // which viewport produced it, so the entry is found by generation.
        // Generations come from one global counter, so at most one entry can
        // match, and the map holds one entry per content viewport.
        match pending.values_mut().find(|(_, new, _, _)| *new == gen) {
            Some((prev, _new, patch, delivered)) => {
                // Multiple viewport slots can request the same handoff while
                // preparing one frame. Keep it available to all of them, but
                // let the next geometry update know that the current version
                // has already been exposed to the renderer and should start a
                // fresh patch chain.
                *delivered = true;
                Some((*prev, Arc::clone(patch)))
            }
            _ => None,
        }
    }

    /// The wires a memoized run stands for; `None` when an `InSet` run's
    /// backing set is gone, so the caller tessellates afresh.
    pub(crate) fn resolve_memo_run(&self, handle: Handle, run: &MemoRun) -> Option<MemoHit> {
        match run {
            MemoRun::Owned(wires) => Some(MemoHit::Owned(Arc::clone(wires))),
            MemoRun::InSet => {
                let backing = self.resident_memo_backing.borrow();
                let set = backing.set.upgrade()?;
                let &(start, len) = backing.ranges.get(&handle)?;
                (start + len <= set.len()).then_some(MemoHit::Slice(set, start, len))
            }
            MemoRun::Pending => None,
        }
    }

    /// Whether building `block` reads and fills the resident memo.
    pub(crate) fn uses_resident_memo(&self, block: Handle, frozen_layers: Option<&HashSet<Handle>>) -> bool {
        block == self.model_space_block_handle()
            && frozen_layers.is_none_or(|layers| layers.is_empty())
            // A walked build filters entities differently than the memo's
            // un-walked set membership, so it must not read or fill it.
            && self.interaction_layer_walk().is_none()
            && resident_memo_enabled()
    }

    /// Whether a resident set's entity runs are exactly the memo's: nothing
    /// recolours or replaces them after tessellation.
    pub(crate) fn resident_runs_match_memo(&self, block: Handle) -> bool {
        self.count_display.is_none()
            && self.refedit_keep.is_none()
            && !self.block_visibility_reveal
            && self.active_live_section(block).is_none()
    }

    /// Point the memo at `set`: an entity with a run there keeps no copy of
    /// its own; a run only the previous backing held is copied out first.
    pub(crate) fn bind_memo_to_set(
        &self,
        set: &Arc<Vec<WireModel>>,
        ranges: &HashMap<Handle, (usize, usize)>,
    ) {
        let old = std::mem::take(&mut *self.resident_memo_backing.borrow_mut());
        let old_set = old.set.upgrade();
        self.resident_tess_memo.borrow_mut().retain(|handle, run| {
            if ranges.contains_key(handle) {
                *run = MemoRun::InSet;
            } else if let MemoRun::Pending = run {
                return false;
            } else if let MemoRun::InSet = run {
                match old_set.as_ref().zip(old.ranges.get(handle)) {
                    Some((wires, &(start, len))) if start + len <= wires.len() => {
                        *run = MemoRun::Owned(Arc::new(wires[start..start + len].to_vec()));
                    }
                    _ => return false,
                }
            }
            true
        });
        *self.resident_memo_backing.borrow_mut() = MemoBacking {
            set: Arc::downgrade(set),
            ranges: ranges.clone(),
            keep: None,
        };
    }

    /// Give every `InSet` run a copy of its own and drop the backing, before
    /// a build whose set will not match the memo.
    pub(crate) fn detach_memo_backing(&self) {
        let old = std::mem::take(&mut *self.resident_memo_backing.borrow_mut());
        let set = old.set.upgrade();
        self.resident_tess_memo.borrow_mut().retain(|handle, run| {
            if let MemoRun::Pending = run {
                return false;
            }
            if let MemoRun::InSet = run {
                match set.as_ref().zip(old.ranges.get(handle)) {
                    Some((wires, &(start, len))) if start + len <= wires.len() => {
                        *run = MemoRun::Owned(Arc::new(wires[start..start + len].to_vec()));
                    }
                    _ => return false,
                }
            }
            true
        });
    }

    /// An abandoned patch took the backing set apart: keep it alive for the
    /// full build that follows. Runs the patch left untouched are intact, and
    /// the runs it blanked were copied out or dropped from the memo first.
    pub(crate) fn keep_memo_backing(&self, wires: Vec<WireModel>) {
        let keep = Arc::new(wires);
        let mut backing = self.resident_memo_backing.borrow_mut();
        backing.set = Arc::downgrade(&keep);
        backing.keep = Some(keep);
    }

    pub(crate) fn resident_wire_layout(wires: &[WireModel]) -> Option<ResidentWireLayout> {
        let mut order = Vec::new();
        let mut ranges = HashMap::default();
        let mut current: Option<(Handle, usize)> = None;
        let mut marker_start = wires.len();

        for (index, wire) in wires.iter().enumerate() {
            let Some(handle) = Self::handle_from_wire_name(&wire.name) else {
                if let Some((handle, start)) = current.take() {
                    if ranges.insert(handle, (start, index - start)).is_some() {
                        return None;
                    }
                }
                marker_start = index;
                if wires[index..]
                    .iter()
                    .any(|tail| Self::handle_from_wire_name(&tail.name).is_some())
                {
                    return None;
                }
                break;
            };
            if current.map(|(h, _)| h) != Some(handle) {
                if let Some((previous, start)) = current.take() {
                    if ranges.insert(previous, (start, index - start)).is_some() {
                        return None;
                    }
                }
                if ranges.contains_key(&handle) {
                    return None;
                }
                order.push(handle);
                current = Some((handle, index));
            }
        }
        if let Some((handle, start)) = current {
            if ranges
                .insert(handle, (start, marker_start - start))
                .is_some()
            {
                return None;
            }
        }
        Some(ResidentWireLayout {
            order,
            ranges,
            vacant: HashMap::default(),
            tombstoned_wires: 0,
            marker_start,
        })
    }

    /// Bring the resident set for `key` up to the current epoch by replaying only
    /// the changed entities into the cached assembly. Returns the patched `Arc`,
    /// or `None` to fall back to a full rebuild — the safe default whenever the
    /// fast path can't apply: no cache entry, the journal can't be replayed
    /// (a `full` delta or ring overflow), the cached `Arc` is still shared (so
    /// its wires can't be moved out), or a structural assumption is violated
    /// (a wire not named with its entity handle, or an entity's wires not
    /// contiguous). Because every failure falls back to the identical full
    /// build, correctness never depends on the fast path being taken.
    pub(crate) fn try_resident_patch(
        &self,
        key: u64,
        block: Handle,
        bg: [f32; 4],
        anno_scale_override: Option<f32>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        frozen_layers: Option<&HashSet<Handle>>,
        style_viewport: Option<Handle>,
    ) -> Option<Arc<Vec<WireModel>>> {
        let perf = crate::perf::enabled();
        // Each early exit here costs a full wire assembly, so say which one.
        let declined = |reason: &str| -> Option<Arc<Vec<WireModel>>> {
            if perf {
                crate::perf_record!("[perf] resident-patch-skip reason={reason}");
            }
            None
        };
        if self.viewport_style_key(style_viewport) != 0 {
            return declined("viewport-style");
        }
        let t_patch = iced::time::Instant::now();
        // A patch whose set will not equal the memo's runs leaves the memo
        // holding its own copies, taken while the set is still whole.
        let shareable =
            self.uses_resident_memo(block, frozen_layers) && self.resident_runs_match_memo(block);
        if self.uses_resident_memo(block, frozen_layers) && !shareable {
            self.detach_memo_backing();
        }
        // The entry must exist, be stale, and be uniquely held so we can move
        // its wires out rather than deep-clone them.
        let (cached_epoch, backed_here) = {
            let sets = self.resident_wire_sets.borrow();
            let Some(entry) = sets.get(&key) else {
                drop(sets);
                return declined("no-entry");
            };
            if entry.epoch == self.geometry_epoch {
                return None;
            }
            if entry.layout.is_none() {
                drop(sets);
                return declined("no-layout");
            }
            let strong = Arc::strong_count(&entry.wires);
            if strong != 1 {
                if perf {
                    crate::perf_record!("[perf] resident-shared strong={strong}");
                }
                return None;
            }
            let backed_here = shareable
                && self
                    .resident_memo_backing
                    .borrow()
                    .set
                    .ptr_eq(&Arc::downgrade(&entry.wires));
            (entry.epoch, backed_here)
        };
        let Some(deltas) = self.replay_since(cached_epoch) else {
            return declined("journal-too-old");
        };

        // Take ownership of the cached assembly (guaranteed unique above).
        // `prev_gen` is the content id the GPU currently holds for this set — the
        // base the wire-arena patch replays from.
        let (prev_gen, mut estimated_bytes, owned, mut layout) = {
            let removed = self.resident_wire_sets.borrow_mut().remove(&key)?;
            (
                removed.gen,
                removed.estimated_bytes,
                Arc::try_unwrap(removed.wires).ok()?,
                removed.layout?,
            )
        };
        // Every early return below abandons the patch; the memo's `InSet`
        // runs then still need this set, so the guard hands it back.
        let mut reclaim = ReclaimOnDecline {
            scene: self,
            wires: Some(owned),
            backed: backed_here,
        };
        let owned = reclaim.wires.as_mut()?;
        if layout.marker_start > owned.len() {
            return None;
        }

        // Re-tessellate the changed entities, replicating the full build's exact
        // context (anno derivation, empty selection, no cull / no LOD). Faded
        // copy goes into the assembly; the raw copy refreshes the memo, matching
        // wires_for_block_culled (memo stores pre-fade wires).
        let anno = if let Some(a) = anno_scale_override {
            a
        } else if self.current_layout == "Model" {
            self.annotation_scale
        } else {
            1.0
        };
        let blk = self.block_cache_arc_for(annotation_scale_handle, all_visible, style_viewport);
        let blk_done = t_patch.elapsed();
        let empty_sel: HashSet<Handle> = HashSet::default();
        let mut new_runs: HashMap<Handle, Vec<WireModel>> = HashMap::default();
        let mut memo_updates: Vec<(Handle, MemoRun)> = Vec::new();
        let mut visible_changed: HashSet<Handle> = HashSet::default();
        // Every geometry edit drops its handle's memo entry, so an entry that
        // survived its delta marks a visibility-only change: its wires come
        // back from the memo, provided it was built under this display context.
        let avp = style_viewport.or(self.active_viewport);
        let memo_ok = self.resident_tess_guard.get()
            == tess_memo_guard(None, anno, annotation_scale_handle, all_visible, bg, avp)
            && self.resident_tess_atlas.get() == crate::scene::text::sdf_atlas::generation()
            && self.resident_tess_complex_lt.get() == self.viewport_complex_lt_guard();
        // References to a block re-expanded after its definition changed
        // share one prototype: the first one comes back with the full run
        // and the others as a stub of it. Longest runs first gives the full
        // run back to the reference whose slots held it; in the journal's
        // order another reference could get it and grow past its slots.
        let mut from_memo = 0usize;
        // What is tessellated afresh, in groups that run side by side. The
        // references to one block share a group and keep their order in it,
        // so the block's full run still goes to the one that held it; any
        // other entity is a group of its own.
        let mut groups: Vec<Vec<(Handle, &EntityType)>> = Vec::new();
        let mut group_of_block: HashMap<&str, usize> = HashMap::default();
        let mut tess_order: Vec<(Handle, ChangeKind)> = deltas.clone();
        tess_order.sort_by_key(|(h, _)| {
            std::cmp::Reverse(
                layout
                    .ranges
                    .get(h)
                    .or_else(|| layout.vacant.get(h))
                    .map_or(0, |&(_, len)| len),
            )
        });
        for (h, kind) in &tess_order {
            if matches!(kind, ChangeKind::Removed) {
                continue;
            }
            let Some(e) = self.document.get_entity(*h) else {
                continue;
            };
            if !self.resident_entity_visible(
                e,
                block,
                frozen_layers,
                annotation_scale_handle,
                all_visible,
            ) {
                continue;
            }
            visible_changed.insert(*h);
            let memoized = memo_ok
                .then(|| {
                    let memo = self.resident_tess_memo.borrow();
                    match memo.get(h)? {
                        MemoRun::Owned(wires) => Some(wires.as_ref().clone()),
                        MemoRun::InSet if backed_here => {
                            let &(start, len) = layout.ranges.get(h)?;
                            owned.get(start..start + len).map(<[WireModel]>::to_vec)
                        }
                        run => self
                            .resolve_memo_run(*h, run)
                            .map(|hit| hit.wires().to_vec()),
                    }
                })
                .flatten();
            if let Some(mut faded) = memoized {
                self.apply_refedit_fade(&mut faded, bg);
                new_runs.insert(*h, faded);
                from_memo += 1;
                continue;
            }
            let group = match e {
                EntityType::Insert(insert) => *group_of_block
                    .entry(insert.block_name.as_str())
                    .or_insert_with(|| {
                        groups.push(Vec::new());
                        groups.len() - 1
                    }),
                _ => {
                    groups.push(Vec::new());
                    groups.len() - 1
                }
            };
            groups[group].push((*h, e));
        }
        // A layer switched off inside a block changes thousands of
        // references at once: tessellate on all cores, as the full build does.
        let tessellated: Vec<(Handle, Vec<WireModel>)> = {
            use crate::par::prelude::*;
            let document = &self.document;
            let selection = &empty_sel;
            let block_cache: &cache::block_cache::BlockCache = &blk;
            let viewport_lt = self.viewport_lt_for(anno_scale_override);
            let reference_only = self.reference_only.handles();
            groups
                .par_iter()
                .flat_map_iter(move |group| {
                    group.iter().map(move |&(h, e)| {
                        let wires = tessellate_entity(
                            document,
                            selection,
                            avp,
                            bg,
                            anno,
                            annotation_scale_handle,
                            e,
                            Some(block_cache),
                            None,
                            None,
                            viewport_lt,
                            reference_only,
                        );
                        (h, wires)
                    })
                })
                .collect()
        };
        for (h, mut raw) in tessellated {
            // Solid B-rep corners snap via the mesh edge list (their wire
            // path is an empty shell). Enrich before the memo/assembly split
            // so both share the same snap-bearing wires.
            self.attach_solid_snaps(h, &mut raw);
            raw.iter_mut().for_each(WireModel::compact_resident_geometry);
            // A set the memo shares its runs with holds these wires as they
            // are: the memo is bound to it below and needs no copy of its
            // own. Should the patch decline first, a pending entry counts as
            // a miss and the full build tessellates the entity again.
            memo_updates.push((
                h,
                if shareable {
                    MemoRun::Pending
                } else {
                    MemoRun::Owned(Arc::new(raw.clone()))
                },
            ));
            let mut faded = raw;
            self.apply_refedit_fade(&mut faded, bg);
            new_runs.insert(h, faded);
        }
        let tessellated = memo_updates.len();
        self.resident_patch_tessellated
            .set(self.resident_patch_tessellated.get() + tessellated as u64);
        {
            let mut memo = self.resident_tess_memo.borrow_mut();
            for (h, run) in memo_updates {
                memo.insert(h, run);
            }
        }
        let tess_done = t_patch.elapsed();
        // Capture whether any replaced/removed OLD run fed the Face3D/fill
        // pass before blanking its resident slots. Treating every removal as a
        // face change made deleting an ordinary line rescan/re-upload the whole
        // fill pass on large drawings.
        let old_face_pass_changed = deltas.iter().any(|(handle, _)| {
            layout.ranges.get(handle).is_some_and(|&(start, len)| {
                start + len <= layout.marker_start
                    && owned[start..start + len]
                        .iter()
                        .any(|wire| !wire.fill_tris.is_empty())
            })
        });
        let old_text_pass_changed = deltas.iter().any(|(handle, _)| {
            layout.ranges.get(handle).is_some_and(|&(start, len)| {
                start + len <= layout.marker_start
                    && owned[start..start + len]
                        .iter()
                        .any(|wire| !wire.text_verts.is_empty())
            })
        });

        for (handle, _) in &deltas {
            if let Some(&(start, len)) = layout.ranges.get(handle) {
                if let Some(old) = owned.get(start..start + len) {
                    estimated_bytes = estimated_bytes.saturating_sub(Self::resident_wire_bytes(old));
                }
            }
            if let Some(new) = new_runs.get(handle) {
                estimated_bytes = estimated_bytes.saturating_add(Self::resident_wire_bytes(new));
            }
        }
        // Keep the submission-order directory current. Adds are inserted by
        // the same effective SortEntitiesTable key used by the full builder;
        // hidden Modified entities retain a zero-length placeholder so grip
        // commit restores them at their original position.
        {
            let cache = self.sort_cache.borrow();
            let sort_map = cache.as_ref().and_then(|(_, index)| index.get(&block));
            let effective = |handle: Handle| {
                sort_map
                    .and_then(|map| map.get(&handle.value()))
                    .copied()
                    .unwrap_or(handle.value())
            };
            // Hashed once: a linear `order.contains` per re-shown entity made
            // turning a big layer back on quadratic. A run in the set (live
            // or blanked) is always listed in the order, so an edit of drawn
            // entities alone (a move, its undo) needs no lookup at all.
            let has_run = |handle: &Handle| {
                layout.ranges.contains_key(handle) || layout.vacant.contains_key(handle)
            };
            let needs_lookup = visible_changed.iter().any(|handle| !has_run(handle));
            let ordered: HashSet<Handle> = if needs_lookup {
                layout.order.iter().copied().collect()
            } else {
                HashSet::default()
            };
            debug_assert!(
                visible_changed
                    .iter()
                    .filter(|handle| has_run(handle))
                    .all(|handle| layout.order.contains(handle)),
                "every run in the resident set is listed in its order"
            );
            let listed = |handle: &Handle| has_run(handle) || ordered.contains(handle);
            // A removed entity keeps its order entry as a tombstone. Undo/Redo
            // can then restore the exact physical slot and submission position.
            //
            // The journal gives the changes in no order. Lowest key first, each
            // new entity goes in behind the ones before it: at the end of the
            // list for the usual add, with nothing to move. In the journal's
            // order a copy of many entities moved the others for each one.
            let mut unlisted: Vec<(u64, Handle)> = deltas
                .iter()
                .filter(|&&(handle, kind)| {
                    !matches!(kind, ChangeKind::Removed)
                        && visible_changed.contains(&handle)
                        && !listed(&handle)
                })
                .map(|&(handle, _)| (effective(handle), handle))
                .collect();
            unlisted.sort_unstable_by_key(|&(key, handle)| (key, handle.value()));
            for (key, handle) in unlisted {
                let position = layout
                    .order
                    .partition_point(|&existing| effective(existing) <= key);
                layout.order.insert(position, handle);
            }
        }

        // Patch changed runs without shifting the resident Vec. Delete/hide
        // blanks the old slots; same-shaped Undo/Redo restores them in place.
        // Only a true tail add/resize touches Vec length (and shifts the handful
        // of synthesized marker wires, never the drawing's entity runs).
        // The GPU patch takes a copy of every changed run. With thousands of
        // them, the copies are made side by side.
        let mut gpu_copies: HashMap<Handle, Arc<Vec<WireModel>>> = {
            use crate::par::prelude::*;
            new_runs
                .par_iter()
                .map(|(handle, run)| (*handle, Arc::new(run.clone())))
                .collect()
        };
        let mut gpu_runs: HashMap<Handle, Arc<Vec<WireModel>>> = HashMap::default();
        let mut index_edits = Vec::new();
        // A new run can only go in at the tail, so the new entities are placed
        // last and in the order of the list: each one then goes in behind the
        // one before it. In the journal's order, which is no order, the second
        // of two new entities could come first, leave the other with a run
        // after its place, and send the whole set to a full build.
        let is_new = |handle: &Handle, kind: ChangeKind| {
            !matches!(kind, ChangeKind::Removed)
                && !layout.ranges.contains_key(handle)
                && !layout.vacant.contains_key(handle)
        };
        let new_handles: HashSet<Handle> = deltas
            .iter()
            .filter(|&&(handle, kind)| is_new(&handle, kind))
            .map(|&(handle, _)| handle)
            .collect();
        let mut placement: Vec<(Handle, ChangeKind)> = deltas
            .iter()
            .copied()
            .filter(|(handle, _)| !new_handles.contains(handle))
            .collect();
        let known = placement.len();
        {
            let mut new_in_order: Vec<(usize, Handle, ChangeKind)> = Vec::new();
            if !new_handles.is_empty() {
                let kinds: HashMap<Handle, ChangeKind> = deltas.iter().copied().collect();
                for (index, handle) in layout.order.iter().enumerate() {
                    if new_handles.contains(handle) {
                        new_in_order.push((index, *handle, kinds[handle]));
                    }
                }
            }
            // One that the list does not hold is placed with the known ones,
            // where it declines the patch as it did.
            let found: HashSet<Handle> =
                new_in_order.iter().map(|&(_, handle, _)| handle).collect();
            let missing: Vec<(Handle, ChangeKind)> = deltas
                .iter()
                .copied()
                .filter(|(handle, _)| new_handles.contains(handle) && !found.contains(handle))
                .collect();
            placement.extend(missing);
            placement.extend(new_in_order.iter().map(|&(_, handle, kind)| (handle, kind)));
        }
        // For each new entity, where the next run after its place starts, or
        // `None` at the tail. Read when the first new entity is placed: the
        // known entities are all placed by then.
        let mut next_run_start: Option<HashMap<Handle, Option<usize>>> = None;
        for (placed, &(handle, kind)) in placement.iter().enumerate() {
            if placed >= known && next_run_start.is_none() {
                let mut after = None;
                let mut starts = HashMap::default();
                for listed in layout.order.iter().rev() {
                    if new_handles.contains(listed) {
                        starts.insert(*listed, after);
                    } else if let Some(range) = layout.ranges.get(listed) {
                        after = Some(range.0);
                    }
                }
                next_run_start = Some(starts);
            }
            let new_run = if matches!(kind, ChangeKind::Removed) {
                Vec::new()
            } else {
                new_runs.remove(&handle).unwrap_or_default()
            };
            gpu_runs.insert(handle, gpu_copies.remove(&handle).unwrap_or_default());
            let new_len = new_run.len();
            let old_range = layout.ranges.remove(&handle);

            if new_len == 0 {
                if let Some((start, old_len)) = old_range {
                    if start + old_len > layout.marker_start {
                        return declined("hide-outside-entity-runs");
                    }
                    let blank = || {
                        let mut wire = WireModel::solid(String::new(), Vec::new(), [0.0; 4], false);
                        wire.aabb = [
                            f32::INFINITY,
                            f32::INFINITY,
                            f32::NEG_INFINITY,
                            f32::NEG_INFINITY,
                        ];
                        wire
                    };
                    // A hidden entity's memo run lives in these slots: it
                    // moves out as they are blanked, for when it is shown
                    // again.
                    let mut moved_out = false;
                    if backed_here && !matches!(kind, ChangeKind::Removed) {
                        let mut memo = self.resident_tess_memo.borrow_mut();
                        if let Some(run) = memo.get_mut(&handle) {
                            if matches!(run, MemoRun::InSet) {
                                *run = MemoRun::Owned(Arc::new(
                                    owned[start..start + old_len]
                                        .iter_mut()
                                        .map(|slot| std::mem::replace(slot, blank()))
                                        .collect(),
                                ));
                                moved_out = true;
                            }
                        }
                    }
                    if !moved_out {
                        for slot in &mut owned[start..start + old_len] {
                            *slot = blank();
                        }
                    }
                    layout.vacant.insert(handle, (start, old_len));
                    layout.tombstoned_wires = layout.tombstoned_wires.saturating_add(old_len);
                    index_edits.push(WireIndexEdit {
                        handle,
                        start,
                        old_len,
                        new_len: old_len,
                        visible: false,
                    });
                }
                continue;
            }

            if let Some((start, vacant_len)) = layout.vacant.remove(&handle) {
                if vacant_len != new_len || start + vacant_len > layout.marker_start {
                    if perf {
                        crate::perf_record!(
                            "[perf] resident-patch-skip reason=restore-length-differs vacant={vacant_len} now={new_len} handle={}",
                            handle.value()
                        );
                    }
                    return None;
                }
                for (slot, wire) in owned[start..start + vacant_len].iter_mut().zip(new_run) {
                    *slot = wire;
                }
                layout.ranges.insert(handle, (start, new_len));
                layout.tombstoned_wires = layout.tombstoned_wires.saturating_sub(vacant_len);
                index_edits.push(WireIndexEdit {
                    handle,
                    start,
                    old_len: vacant_len,
                    new_len,
                    visible: true,
                });
                continue;
            }

            if let Some((start, old_len)) = old_range {
                if start + old_len > layout.marker_start || layout.marker_start > owned.len() {
                    return declined("run-outside-entity-runs");
                }
                // A shorter run that is not the last one keeps all its slots,
                // so no later run moves and the handle index stays as it is:
                // switching a layer off inside a block shortens every
                // reference to that block, all through the set. The unused
                // slots hold blank wires under the entity's name, with the
                // default unbounded box: a window selection passes over such
                // a wire, where an empty box would leave the entity out.
                let mut new_run = new_run;
                if new_len < old_len && start + old_len != layout.marker_start {
                    let name = handle.value().to_string();
                    new_run.resize_with(old_len, || {
                        WireModel::solid(name.clone(), Vec::new(), [0.0; 4], false)
                    });
                }
                if old_len == new_run.len() {
                    for (slot, wire) in owned[start..start + old_len].iter_mut().zip(new_run) {
                        *slot = wire;
                    }
                    layout.ranges.insert(handle, (start, old_len));
                    continue;
                }
                // Any other shape change is relocation-free only for the
                // terminal entity (the live-polyline case). A run that grows
                // anywhere else takes the clean full-build fallback instead
                // of shifting every following range.
                if start + old_len != layout.marker_start {
                    return declined("resized-run-not-at-the-tail");
                }
                index_edits.push(WireIndexEdit {
                    handle,
                    start,
                    old_len,
                    new_len,
                    visible: true,
                });
                owned.splice(start..start + old_len, new_run);
                let delta = new_len as isize - old_len as isize;
                layout.marker_start = (layout.marker_start as isize + delta) as usize;
                layout.ranges.insert(handle, (start, new_len));
                continue;
            }

            // A genuinely new handle must append after all live entity runs.
            // Monotonic document handles make this the ordinary Add path. A
            // middle insertion (custom sort key) falls back to a full rebuild.
            let insertion = next_run_start
                .as_ref()
                .and_then(|starts| starts.get(&handle))?
                .unwrap_or(layout.marker_start);
            if insertion != layout.marker_start {
                return declined("new-run-not-at-the-tail");
            }
            index_edits.push(WireIndexEdit {
                handle,
                start: insertion,
                old_len: 0,
                new_len,
                visible: true,
            });
            owned.splice(insertion..insertion, new_run);
            layout.marker_start += new_len;
            layout.ranges.insert(handle, (insertion, new_len));
        }
        if !new_runs.is_empty() {
            return declined("unplaced-runs-left");
        }
        if layout.tombstoned_wires > layout.marker_start / 2 {
            return declined("too-many-tombstones");
        }
        let added: HashSet<Handle> = deltas
            .iter()
            .filter_map(|&(handle, kind)| matches!(kind, ChangeKind::Added).then_some(handle))
            .collect();
        let mut saw_added = false;
        let mut new_handles_are_suffix = true;
        for handle in &layout.order {
            if added.contains(handle) {
                saw_added = true;
            } else if saw_added && layout.ranges.contains_key(handle) {
                new_handles_are_suffix = false;
                break;
            }
        }
        let face_pass_changed = old_face_pass_changed
            || deltas.iter().any(|&(handle, _)| {
                matches!(
                    self.document.get_entity(handle),
                    Some(EntityType::Face3D(_))
                ) || gpu_runs
                    .get(&handle)
                    .is_some_and(|run| run.iter().any(|wire| !wire.fill_tris.is_empty()))
            });
        let text_pass_changed = old_text_pass_changed
            || gpu_runs
                .values()
                .any(|run| run.iter().any(|wire| !wire.text_verts.is_empty()));

        let place_done = t_patch.elapsed();
        let arc = Arc::new(reclaim.wires.take().unwrap_or_default());
        self.last_tess_wires.set(arc.len());
        self.resident_patch_hits
            .set(self.resident_patch_hits.get() + 1);
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        // Hand the exact changed handles to the GPU wire arena so it patches just
        // those entities' slabs. Only for the Model set (the arena is model-only);
        // the render layer verifies prev_gen against what the GPU actually holds.
        if wire_gpu_patch_enabled() && block == self.model_space_block_handle() {
            let next_patch = WireGpuPatch {
                changes: Arc::new(deltas.clone()),
                runs: Arc::new(gpu_runs),
                index_edits: Arc::new(index_edits),
                new_handles_are_suffix,
                face_pass_changed,
                text_pass_changed,
            };
            let mut pending = self.model_wire_gpu_patch.borrow_mut();
            let combined = match pending.remove(&style_viewport) {
                Some((base, pending_gen, previous, false)) if pending_gen == prev_gen => (
                    base,
                    gen,
                    Arc::new(compose_wire_gpu_patches(&previous, next_patch)),
                    false,
                ),
                _ => (prev_gen, gen, Arc::new(next_patch), false),
            };
            pending.insert(style_viewport, combined);
            // One entry per content viewport is the working set; switching
            // layouts would otherwise leave an entry — and its patch's changed
            // handles and runs — behind for every viewport ever rendered.
            // Generations only grow, so the lowest is the stalest.
            const MAX_PENDING_WIRE_PATCHES: usize = 8;
            while pending.len() > MAX_PENDING_WIRE_PATCHES {
                let stalest = pending
                    .iter()
                    .min_by_key(|(_, (_, new, _, _))| *new)
                    .map(|(key, _)| *key);
                match stalest {
                    Some(key) => {
                        pending.remove(&key);
                    }
                    None => break,
                }
            }
        }
        let gpu_done = t_patch.elapsed();
        if shareable {
            self.bind_memo_to_set(&arc, &layout.ranges);
        }
        let bind_done = t_patch.elapsed();
        let cur_epoch = self.geometry_epoch;
        // Same bound as the full build: this path used to rely on the epoch
        // prune alone, so weakening that without a cap here would let
        // abandoned keys accumulate full sets.
        let shown = if self.current_layout == "Model" {
            0
        } else {
            self.paper_viewport_handles().2.len()
        };
        let mut sets = self.resident_wire_sets.borrow_mut();
        sets.insert(
            key,
            ResidentWireSet {
                estimated_bytes,
                epoch: cur_epoch,
                gen,
                used: std::cell::Cell::new(resident_touch()),
                wires: Arc::clone(&arc),
                layout: Some(layout),
            },
        );
        self.prune_resident_wire_sets(&mut sets, shown);
        if perf {
            let ms = |from: std::time::Duration, to: std::time::Duration| {
                to.saturating_sub(from).as_secs_f64() * 1000.0
            };
            crate::perf_record!(
                "[perf] resident-patch {:>7.1}ms [blk_cache={:.1} tess={:.1} place={:.1} gpu_patch={:.1} memo_bind={:.1}] wires={} changes={} tessellated={} from_memo={}",
                t_patch.elapsed().as_secs_f64() * 1000.0,
                blk_done.as_secs_f64() * 1000.0,
                ms(blk_done, tess_done),
                ms(tess_done, place_done),
                ms(place_done, gpu_done),
                ms(gpu_done, bind_done),
                arc.len(),
                deltas.len(),
                tessellated,
                from_memo,
            );
        }
        Some(arc)
    }
}
