use super::*;

impl Scene {
    /// Whether entity `e` renders as direct content of `block_handle` right now:
    /// visible (not invisible / hidden / on an off-or-frozen layer / frozen
    /// through the viewport) and owned by the block. Shared by the full resident
    /// build and the incremental patch so a changed entity is classified
    /// identically either way.
    pub(in crate::scene) fn resident_entity_visible(
        &self,
        e: &EntityType,
        block_handle: Handle,
        frozen_layers: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
    ) -> bool {
        let layer = self.document.layers.get(&e.common().layer);
        self.resident_entity_visible_with_layer(
            e,
            block_handle,
            frozen_layers,
            annotation_scale_handle,
            all_visible,
            layer,
        )
    }

    /// Visibility predicate with the layer resolved by the caller.
    pub(in crate::scene) fn resident_entity_visible_with_layer(
        &self,
        e: &EntityType,
        block_handle: Handle,
        frozen_layers: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        layer: Option<&document::tables::layer::Layer>,
    ) -> bool {
        let c = e.common();
        if self.display_entity_invisible(c) {
            return false;
        }
        // Session-only Isolate / Hide and interactive replacement previews.
        if self.entity_temporarily_hidden(c.handle) {
            return false;
        }
        // Block/BlockEnd are block-defn sentinels, not drawable geometry.
        if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
            return false;
        }
        if layer
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
        {
            return false;
        }
        // Session LAYWALK: in a walked space, layers outside the visible
        // set hide exactly like frozen ones — in memory, for this space only.
        if self.layer_walk_hidden(layer) {
            return false;
        }
        if let Some(frozen) = frozen_layers {
            if !frozen.is_empty() {
                if frozen.contains(&c.handle) {
                    return false;
                }
                if let Some(lh) = layer.map(|l| l.handle) {
                    if frozen.contains(&lh) {
                        return false;
                    }
                }
            }
        }
        if crate::scene::annotative::annotative_offscale_for(
            &self.document,
            c,
            annotation_scale_handle,
            all_visible,
        ) {
            return false;
        }
        self.belongs_to_visible_block(c.handle, c.owner_handle, block_handle)
    }

    pub(in crate::scene) fn wires_for_block_culled<'doc>(
        &'doc self,
        block_handle: Handle,
        view_aabb: Option<[f32; 4]>,
        wpp: Option<f32>,
        // Layers frozen specifically through the requesting viewport.
        // Hidden in addition to the document-level off / frozen flags.
        // `None` skips the per-viewport check (Model-space callers).
        frozen_layers: Option<&HashSet<Handle>>,
        // Paper-space content viewports compute their own annotation
        // scale from `vp_effective_scale`; the Model-space and paper-
        // sheet paths use `self.annotation_scale` / 1.0 respectively.
        // `None` selects the default branch on `current_layout`.
        anno_scale_override: Option<f32>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        style_viewport: Option<Handle>,
        // Build only these entities — the same wires a full build would give
        // them, though not in document order — instead of the whole block.
        // View culling is not applied.
        only: Option<&HashSet<Handle>>,
    ) -> Vec<WireModel> {
        use document::objects::ObjectType;

        // Each INSERT tessellated below resolves its block's base point.
        let _owners = block_authoring::DictionaryOwners::publish(&self.document);
        // Skip diagnostic clock reads when PERF is disabled.
        let perf = crate::perf::enabled();
        let t_fn = perf.then(iced::time::Instant::now);

        // ── Ensure sort-order index is current ────────────────────────────
        // Replaces the old O(objects) find_map with one rebuild per epoch,
        // after which every wires_for_block call is an O(1) HashMap lookup.
        {
            let needs_rebuild = self
                .caches.sort_cache
                .borrow()
                .as_ref()
                .map(|(e, _)| *e != self.geometry_epoch)
                .unwrap_or(true);

            if needs_rebuild {
                let mut idx: HashMap<Handle, HashMap<u64, u64>> = HashMap::default();
                for obj in self.document.objects.values() {
                    if let ObjectType::SortEntitiesTable(t) = obj {
                        if !t.is_empty() {
                            let map = t
                                .entries()
                                .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
                                .collect();
                            idx.insert(t.block_owner_handle, map);
                        }
                    }
                }
                *self.caches.sort_cache.borrow_mut() = Some((self.geometry_epoch, idx));
            }
        }

        // Visibility test reused by both paths below — and by the resident
        // incremental patch, so a changed entity is included/excluded exactly as
        // a from-scratch build would (no divergence).
        // Resolve each distinct layer name once for this candidate pass.
        type LayerRef<'a> = Option<&'a document::tables::layer::Layer>;
        let layer_of: RefCell<rustc_hash::FxHashMap<&'doc str, LayerRef<'doc>>> =
            RefCell::new(rustc_hash::FxHashMap::default());
        let layer_for =
            |e: &'doc EntityType| match layer_of.borrow_mut().entry(e.common().layer.as_str()) {
                std::collections::hash_map::Entry::Occupied(hit) => *hit.get(),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    *slot.insert(self.document.layers.get(e.common().layer.as_str()))
                }
            };
        let has_temporary_hidden = self.object_isolation.is_active()
            || !self.preview_hidden.is_empty()
            || !self.command_preview_hidden.is_empty()
            || !self.hidden_dynamic_dimensions.is_empty();
        let visibility_ok = |e: &'doc EntityType| {
            let c = e.common();
            if c.invisible && self.display_entity_invisible(c) {
                return false;
            }
            if has_temporary_hidden && self.entity_temporarily_hidden(c.handle) {
                return false;
            }
            if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                return false;
            }
            let layer = layer_for(e);
            if layer
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
            {
                return false;
            }
            if self.layer_walk_hidden(layer) {
                return false;
            }
            if let Some(frozen) = frozen_layers {
                if !frozen.is_empty() {
                    if frozen.contains(&c.handle) {
                        return false;
                    }
                    if let Some(lh) = layer.map(|l| l.handle) {
                        if frozen.contains(&lh) {
                            return false;
                        }
                    }
                }
            }
            if !all_visible
                && crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    c,
                    annotation_scale_handle,
                    all_visible,
                )
            {
                return false;
            }
            self.belongs_to_visible_block(c.handle, c.owner_handle, block_handle)
        };

        let sort_cache_ms = crate::perf::elapsed_ms(t_fn);
        let t_visible = perf.then(iced::time::Instant::now);
        // Which path the selection took, and how many candidates it looked at.
        // Left uninitialised on purpose: every branch below must set it, and
        // the compiler enforces that. A default here is what made the first
        // version of this diagnostic report a constant.
        let visible_path: &str;
        let mut visible_candidates: usize;
        // PERF-only breakdown for index maintenance and handle resolution.
        let mut visible_index_ms = 0.0f64;
        let mut visible_probe_ms = 0.0f64;
        // PERF-only ablation of the visibility predicate (see the block that
        // fills them). Each is a full pass, so their sum is also how much the
        // diagnostic itself adds to `visible` on a PERF run.
        let mut vis_a_layer_ms = 0.0f64;
        let mut vis_a_hidden_ms = 0.0f64;
        let mut vis_a_invis_ms = 0.0f64;
        let mut vis_a_walk_ms = 0.0f64;
        let mut vis_a_block_ms = 0.0f64;
        let mut vis_a_full_ms = 0.0f64;

        // Phase 2.1 — quadtree-driven candidate selection. When a view
        // AABB exists (Model layout with a settled camera), only iterate
        // entities whose stored WCS bbox intersects the view; unindexable
        // entities (Insert/Viewport) are appended via a small linear scan.
        // Paper space and the first-frame "settle" path fall back to the
        // full doc scan — preserving prior behaviour.
        let visible: Vec<&EntityType> = if let Some(only) = only {
            debug_assert!(view_aabb.is_none(), "a restricted build is never culled");
            visible_path = "only";
            visible_candidates = only.len();
            // Same membership as the full-scan / block-index paths below: any
            // entity when no block ownership is known, else direct content of
            // this block by owner handle or, without one, by the block map.
            let members: Vec<&EntityType> = {
                let block_map = self.entity_block_map();
                let any_block = block_handle.is_null() || block_map.is_empty();
                only.iter()
                    .filter_map(|&handle| self.document.get_entity(handle))
                    .filter(|entity| {
                        let common = entity.common();
                        any_block
                            || if common.owner_handle.is_null() {
                                block_map.get(&common.handle) == Some(&block_handle)
                            } else {
                                common.owner_handle == block_handle
                            }
                    })
                    .collect()
            };
            members
                .into_iter()
                .filter(|entity| visibility_ok(entity))
                .collect()
        } else if let Some(local_view) = view_aabb {
            visible_path = "quadtree";
            let view_wcs: [f64; 4] = [
                local_view[0] as f64,
                local_view[1] as f64,
                local_view[2] as f64,
                local_view[3] as f64,
            ];
            let (candidates, unbounded): (Vec<Handle>, Vec<Handle>) = {
                let idx = self.entity_index();
                (idx.tree.query_rect(view_wcs), idx.unbounded_handles.clone())
            };
            let mut out: Vec<&EntityType> =
                Vec::with_capacity(candidates.len() + unbounded.len() + 16);
            visible_candidates = candidates.len() + unbounded.len();
            for h in candidates {
                if let Some(e) = self.document.get_entity(h) {
                    if visibility_ok(e) {
                        out.push(e);
                    }
                }
            }
            // Unbounded entities — always emit regardless of view, mirroring
            // legacy `entity_aabb`'s UNBOUNDED_AABB sentinel.
            for h in unbounded {
                if let Some(e) = self.document.get_entity(h) {
                    if visibility_ok(e) {
                        out.push(e);
                    }
                }
            }
            // Inserts/Viewports/Block/BlockEnd — handled by their own paths
            // (block expansion, viewport rendering); always candidates. Taken
            // from the per-epoch list rather than by walking the document,
            // which preserves document order because the list is built that
            // way.
            {
                let unindexable = self.unindexable_handles();
                visible_candidates += unindexable.1.len();
                for &h in &unindexable.1 {
                    if let Some(e) = self.document.get_entity(h) {
                        if visibility_ok(e) {
                            out.push(e);
                        }
                    }
                }
            }
            out
        } else if block_handle.is_null() || self.entity_block_map().is_empty() {
            // Either every entity qualifies, or the drawing declares no block
            // contents anywhere and `belongs_to_visible_block` turns permissive
            // — the legacy-DXF case. Both need the full walk, and both must
            // keep document order, which is draw order for the wire pass.
            visible_path = "full-scan";
            visible_candidates = self.document.entity_count();
            self.document
                .entities()
                .filter(|e| visibility_ok(e))
                .collect()
        } else {
            // The per-epoch index preserves document order for each block.
            visible_path = "block-index";
            // Time index maintenance separately from the candidate loop.
            let t_members = perf.then(iced::time::Instant::now);
            let members = self.block_members();
            visible_index_ms = crate::perf::elapsed_ms(t_members);
            let (_, by_block) = &*members;
            let claimed = by_block
                .get(&block_handle)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            visible_candidates = claimed.len();
            let mut out: Vec<&EntityType> = Vec::with_capacity(claimed.len());
            for &handle in claimed {
                if let Some(entity) = self.document.get_entity(handle) {
                    if visibility_ok(entity) {
                        out.push(entity);
                    }
                }
            }
            if perf {
                // PERF-only lower bound for warm handle resolution.
                let t_probe = iced::time::Instant::now();
                let mut resolved = 0usize;
                for &handle in claimed {
                    if self.document.get_entity(handle).is_some() {
                        resolved += 1;
                    }
                }
                std::hint::black_box(resolved);
                visible_probe_ms = t_probe.elapsed().as_secs_f64() * 1000.0;

                // Which part of the predicate owns the rest of `visible`.
                //
                // Behind its own variable, not plain `PERF=1`: these are six
                // extra passes over every candidate, and they run INSIDE the
                // `visible` region, so leaving them on inflates `visible` and
                // `wires-build` (measured: 2406 ms → 4389 ms) and makes a
                // PERF run incomparable with any other.
                //
                // Timing each call individually cannot answer this either: at
                // ~1.3 µs per entity the two `Instant::now()` reads cost more
                // than the work and report themselves (tried; the two halves
                // came back equal to 3 digits and `visible` went 350 ms →
                // 4650 ms). So each component gets ONE timer over the whole
                // list, and its cost is that run minus `visible_probe`.
                //
                // Read them as WARM figures. They run after the main loop has
                // already touched every entity, so they exclude the cold-miss
                // cost that dominates the real pass: `full` here is ~149 ms
                // against ~650 ms for the same work done first. Do not
                // conclude from a small `full` that the predicate is cheap.
                let ablate = |f: &dyn Fn(&'doc EntityType) -> bool| {
                    let t = iced::time::Instant::now();
                    let mut n = 0usize;
                    for &handle in claimed {
                        if let Some(e) = self.document.get_entity(handle) {
                            n += f(e) as usize;
                        }
                    }
                    std::hint::black_box(n);
                    t.elapsed().as_secs_f64() * 1000.0
                };
                if std::env::var_os("OCS_PERF_ABLATE").is_some() {
                    vis_a_layer_ms = ablate(&|e| layer_for(e).is_some());
                    vis_a_hidden_ms =
                        ablate(&|e| self.entity_temporarily_hidden(e.common().handle));
                    vis_a_invis_ms = ablate(&|e| self.display_entity_invisible(e.common()));
                    // Note this resolves the layer itself, so its figure
                    // includes `layer`; subtract to get the walk test alone.
                    vis_a_walk_ms = ablate(&|e| self.layer_walk_hidden(layer_for(e)));
                    vis_a_block_ms = ablate(&|e| {
                        let c = e.common();
                        self.belongs_to_visible_block(c.handle, c.owner_handle, block_handle)
                    });
                    vis_a_full_ms = ablate(&|e| visibility_ok(e));
                }
            }
            out
        };

        let visible_ms = crate::perf::elapsed_ms(t_visible);
        let visible_count = visible.len();

        // Tessellate in parallel across all available CPU cores.
        use crate::par::prelude::*;
        let doc = &self.document;
        // Selection / hover highlight is NOT baked into tessellation. It is
        // applied per frame in the GPU xray overlay pass from the live
        // selection set (`Scene::selected` ∪ hover). Keeping `sel` empty here
        // makes the wire cache selection-independent, so picking an entity
        // bumps only `selection_generation` (cheap overlay refresh) instead of
        // `geometry_epoch` (a full model re-tessellation).
        let empty_sel: HashSet<Handle> = HashSet::default();
        let sel: &HashSet<Handle> = &empty_sel;
        let avp = style_viewport.or(self.active_viewport);
        // A paper-space content viewport renders MODEL block entities while
        // the user is sitting in a paper layout — that path expects
        // `world_offset` subtraction even though `current_layout != "Model"`.
        // Decide based on the block being tessellated, not the layout.
        let is_model_block = block_handle == self.model_space_block_handle();
        // The reference-only designation set drives the wire stamp; captured
        // as a shared ref like `doc`/`sel` so the rayon workers can read it.
        let reference_only_set = self.reference_only.handles();
        let bg = if self.current_layout == "Model" {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let anno = if let Some(a) = anno_scale_override {
            a
        } else if self.current_layout == "Model" {
            self.annotation_scale
        } else {
            1.0
        };
        // Content shown inside a paper-space viewport carries a scale override
        // or the PSLTSCALE factor; only there do complex linetypes ignore the
        // annotation scale.
        let paper = self.viewport_lt_for(anno_scale_override);
        let t_blk = perf.then(iced::time::Instant::now);
        let blk_cache =
            self.block_cache_arc_for(annotation_scale_handle, all_visible, style_viewport);
        let blk_ms = crate::perf::elapsed_ms(t_blk);
        let blk_ref: &cache::block_cache::BlockCache = &blk_cache;
        // Per-entity tessellation memo. Same classify/tessellate logic, two
        // SEPARATE stores so they can't thrash each other:
        //   * culled path (`view_aabb == Some`) → `tess_memo`, guard keyed on the
        //     per-view cull params (zoom/tol, frustum, entered viewport);
        //   * resident path (`view_aabb == None`, no per-view cull, Model block) →
        //     the camera-INDEPENDENT `resident_tess_memo`, guard keyed only on
        //     anno / bg. This is the set the main GPU render holds
        //     (`model_tile_wires_arc`), so memoizing it makes a single-entity edit
        //     re-tessellate just the changed entity instead of the whole model.
        // Frozen-layer / anno-override viewport paths bypass (their params would
        // thrash a shared memo). Hit-test also passes `view_aabb == None` but is
        // not the Model block, so it never lands on the resident branch.
        // An empty vp-frozen set and an anno override are memo-compatible: the
        // guard hash below mixes anno/bg, so a param change invalidates rather
        // than corrupts. Only a NON-empty frozen set (entity subset changes)
        // bypasses the memo.
        let frozen_empty = frozen_layers.map(|f| f.is_empty()).unwrap_or(true);
        let base_ok = is_model_block && frozen_empty;
        let resident = base_ok && view_aabb.is_none() && resident_memo_enabled();
        let memo_active = base_ok && (view_aabb.is_some() || resident);
        // Reported in one line below. `tess_ms` / `memo_misses` are assigned by
        // both branches; the rest exist only on the memo path and stay zero.
        let mut classify_ms = 0.0f64;
        let mut hits_ms = 0.0f64;
        let tess_ms;
        let mut materialize_ms = 0.0f64;
        let mut memo_hits = 0usize;
        let memo_misses;
        // Two consecutive identical resident builds both missed every entry,
        // which a stable guard cannot explain — the memo is being cleared
        // between them. Report the guard and the inputs that can move it.
        let mut guard_dbg = 0u64;
        let mut guard_was_stale = false;
        let t_build = perf.then(iced::time::Instant::now);
        let mut wires: Vec<WireModel> = if memo_active {
            // Invalidate when any baked display input changes.
            let guard = tess_memo_guard(
                view_aabb,
                anno,
                annotation_scale_handle,
                all_visible,
                bg,
                avp,
            ) ^ if self.constraint_shading_active() {
                0xC057_A705_0000_0001
            } else {
                0
            };
            let (memo_cell, guard_cell, atlas_cell, complex_lt_cell) = if resident {
                (
                    &self.caches.resident_tess_memo,
                    &self.resident_tess_guard,
                    &self.resident_tess_atlas,
                    &self.resident_tess_complex_lt,
                )
            } else {
                (
                    &self.caches.tess_memo,
                    &self.tess_memo_guard,
                    &self.tess_memo_atlas,
                    &self.tess_memo_complex_lt,
                )
            };
            guard_dbg = guard;
            guard_was_stale = guard_cell.get() != guard;
            if guard_was_stale {
                memo_cell.borrow_mut().clear();
                guard_cell.set(guard);
            }
            drop_stale_text(self, &mut memo_cell.borrow_mut(), atlas_cell);
            drop_stale_complex_lt(self, &mut memo_cell.borrow_mut(), complex_lt_cell);
            // Classify (serial, cheap): reuse memoized Arcs, collect misses.
            let t_classify = perf.then(iced::time::Instant::now);
            let mut hit_arcs: Vec<MemoHit> = Vec::new();
            let mut misses: Vec<&EntityType> = Vec::new();
            {
                let memo = memo_cell.borrow();
                for e in &visible {
                    let h = e.common().handle;
                    match memo.get(&h).and_then(|run| self.resolve_memo_run(h, run)) {
                        Some(hit) => hit_arcs.push(hit),
                        None => misses.push(*e),
                    }
                }
            }
            classify_ms = crate::perf::elapsed_ms(t_classify);
            memo_hits = hit_arcs.len();
            memo_misses = misses.len();
            // Materialize hits + tessellate misses, both in parallel.
            let t_hits = perf.then(iced::time::Instant::now);
            let hit_wires: Vec<WireModel> = hit_arcs
                .par_iter()
                .flat_map_iter(|hit| hit.wires().iter().cloned())
                .collect();
            hits_ms = crate::perf::elapsed_ms(t_hits);
            let is_building_resident = resident && self.building_resident_set.get();
            let mut out = hit_wires;
            if is_building_resident {
                const MISS_CHUNK_SIZE: usize = 256;
                struct MissChunk {
                    wires: Vec<WireModel>,
                    handles: Vec<Handle>,
                    solid_ranges: Vec<(Handle, usize, usize)>,
                }
                let t_tess_miss = perf.then(iced::time::Instant::now);
                let chunks: Vec<MissChunk> = misses
                    .par_chunks(MISS_CHUNK_SIZE)
                    .map(|chunk| {
                        let mut wires = Vec::with_capacity(chunk.len());
                        let mut handles = Vec::with_capacity(chunk.len());
                        let mut solid_ranges = Vec::new();
                        for &e in chunk {
                            let h = e.common().handle;
                            let w = tessellate_entity(
                                doc,
                                sel,
                                avp,
                                bg,
                                anno,
                                annotation_scale_handle,
                                e,
                                Some(blk_ref),
                                view_aabb,
                                wpp,
                                paper,
                                reference_only_set,
                            );
                            if Self::is_solid_shell_entity(e) {
                                let start = wires.len();
                                wires.extend(w);
                                let end = wires.len();
                                solid_ranges.push((h, start, end));
                            } else {
                                wires.extend(w);
                            }
                            handles.push(h);
                        }
                        MissChunk {
                            wires,
                            handles,
                            solid_ranges,
                        }
                    })
                    .collect();
                tess_ms = crate::perf::elapsed_ms(t_tess_miss);
                let t_materialize = perf.then(iced::time::Instant::now);
                let total_new_wires: usize = chunks.iter().map(|c| c.wires.len()).sum();
                out.reserve(total_new_wires);
                {
                    let mut memo = memo_cell.borrow_mut();
                    memo.reserve(misses.len());
                    for chunk in chunks {
                        let base = out.len();
                        out.extend(chunk.wires);
                        for (h, start, end) in chunk.solid_ranges {
                            self.attach_solid_snaps(h, &mut out[base + start..base + end]);
                        }
                        for h in chunk.handles {
                            // The resident set built from `out` becomes the
                            // memo's backing: no second copy (#1542, #1698).
                            memo.insert(h, MemoRun::Pending);
                        }
                    }
                }
                materialize_ms = crate::perf::elapsed_ms(t_materialize);
            } else {
                let t_tess_miss = perf.then(iced::time::Instant::now);
                let miss_pairs: Vec<(Handle, bool, Vec<WireModel>)> = misses
                    .par_iter()
                    .map(|&e| {
                        let w = tessellate_entity(
                            doc,
                            sel,
                            avp,
                            bg,
                            anno,
                            annotation_scale_handle,
                            e,
                            Some(blk_ref),
                            view_aabb,
                            wpp,
                            paper,
                            reference_only_set,
                        );
                        (e.common().handle, Self::is_solid_shell_entity(e), w)
                    })
                    .collect();
                tess_ms = crate::perf::elapsed_ms(t_tess_miss);
                // Materialize new wires while the memo retains their shared source.
                let t_materialize = perf.then(iced::time::Instant::now);
                let total_new_wires: usize = miss_pairs.iter().map(|(_, _, w)| w.len()).sum();
                out.reserve(total_new_wires);
                {
                    let mut memo = memo_cell.borrow_mut();
                    memo.reserve(miss_pairs.len());
                    for (h, is_solid, mut wires) in miss_pairs {
                        // Enrich solid shells with B-rep vertex snaps before the memo/assembly split.
                        if is_solid {
                            self.attach_solid_snaps(h, &mut wires);
                        }
                        let a = Arc::new(wires);
                        out.extend(a.iter().cloned());
                        memo.insert(h, MemoRun::Owned(a));
                    }
                }
                materialize_ms = crate::perf::elapsed_ms(t_materialize);
            }
            out
        } else {
            let t_tess_all = perf.then(iced::time::Instant::now);
            let out: Vec<WireModel> = visible
                .into_par_iter()
                .flat_map(|e| {
                    tessellate_entity(
                        doc,
                        sel,
                        avp,
                        bg,
                        anno,
                        annotation_scale_handle,
                        e,
                        Some(blk_ref),
                        view_aabb,
                        wpp,
                        paper,
                        reference_only_set,
                    )
                })
                .collect();
            tess_ms = crate::perf::elapsed_ms(t_tess_all);
            memo_misses = visible_count;
            out
        };

        // Normal Wireframe renders the persisted entity wires, whereas shaded
        // and 3D Wireframe modes render the temporary sectioned mesh. Replace
        // only the displayed wire run with one tessellated from that same
        // temporary body so every visual style shows the identical live cut.
        if let Some(section) = self.active_live_section(block_handle) {
            let handles: Vec<Handle> = self.meshes.keys().copied().collect();
            let mut replaced_names: HashSet<String> = HashSet::default();
            let mut replacements: Vec<WireModel> = Vec::new();
            for handle in handles {
                if only.is_some_and(|only| !only.contains(&handle)) {
                    continue;
                }
                let Some(source) = self.document.get_entity(handle) else {
                    continue;
                };
                if !visibility_ok(source) {
                    continue;
                }
                let replacement = match self.sectioned_wire_entity(handle, &section) {
                    Ok(Some(entity)) => Some(tessellate_entity(
                        doc,
                        sel,
                        avp,
                        bg,
                        anno,
                        annotation_scale_handle,
                        &entity,
                        Some(blk_ref),
                        view_aabb,
                        wpp,
                        paper,
                        reference_only_set,
                    )),
                    Ok(None) => Some(Vec::new()),
                    Err(()) => None,
                };
                let Some(replacement) = replacement else {
                    continue;
                };
                replaced_names.insert(handle.value().to_string());
                replacements.extend(replacement);
            }
            if !replaced_names.is_empty() {
                wires.retain(|wire| !replaced_names.contains(&wire.name));
                wires.extend(replacements);
            }
        }
        let build_ms = crate::perf::elapsed_ms(t_build);

        // Resolve display colours for the live background. Memo hits were
        // built under whatever background was current then, misses under this
        // one; the pass recomputes both from their recorded raw colour, so a
        // mixed set lands in the same place either way.
        let t_colors = perf.then(iced::time::Instant::now);
        crate::scene::view::render::resolve_colors_for_bg(&mut wires, bg);
        self.shade_constraint_wires(block_handle, &mut wires);
        let colors_ms = crate::perf::elapsed_ms(t_colors);

        // Apply draw order via the cached index (O(1) block lookup).
        let t_sort = perf.then(iced::time::Instant::now);
        let mut sorted = false;
        {
            let cache = self.caches.sort_cache.borrow();
            if let Some((_, ref idx)) = *cache {
                if let Some(sort_map) = idx.get(&block_handle) {
                    sorted = true;
                    // Parse each handle once while preserving stable draw order.
                    wires.sort_by_cached_key(|w| {
                        let key = Self::handle_from_wire_name(&w.name)
                            .map(|h| h.value())
                            .unwrap_or(u64::MAX);
                        // Entities absent from the table sort by their own
                        // handle — the same key space the table's sort handles
                        // live in — so reordered and untouched entities interleave
                        // correctly instead of all collapsing to one constant.
                        sort_map.get(&key).copied().unwrap_or(key)
                    });
                }
            }
        }
        if perf {
            crate::perf_record!(
                "[perf] wires-build total={:.1}ms sort_cache={:.1} visible={:.1} blk_cache={:.1} colors={:.1} \
build={:.1} [classify={:.1} hits={:.1} tess={:.1} materialize={:.1}] sort={:.1}({}) \
entities={} memo_hit={} memo_miss={} wires={} memo={} visible_path={} candidates={} \
guard={:016x} guard_stale={} avp={} anno={:.4} anno_h={} all_vis={} sdf_gen={} \
vis_index={:.1} visible_probe={:.1} \
vis_a[layer={:.1} hidden={:.1} invis={:.1} walk={:.1} block={:.1} full={:.1}]",
                crate::perf::elapsed_ms(t_fn),
                sort_cache_ms,
                visible_ms,
                blk_ms,
                colors_ms,
                build_ms,
                classify_ms,
                hits_ms,
                tess_ms,
                materialize_ms,
                crate::perf::elapsed_ms(t_sort),
                if sorted { "applied" } else { "skipped" },
                visible_count,
                memo_hits,
                memo_misses,
                wires.len(),
                if memo_active { "on" } else { "off" },
                visible_path,
                visible_candidates,
                guard_dbg,
                guard_was_stale,
                avp.map(|h| h.value()).unwrap_or(0),
                anno,
                annotation_scale_handle.map(|h| h.value()).unwrap_or(0),
                all_visible,
                crate::scene::text::sdf_atlas::generation(),
                visible_index_ms,
                visible_probe_ms,
                vis_a_layer_ms,
                vis_a_hidden_ms,
                vis_a_invis_ms,
                vis_a_walk_ms,
                vis_a_block_ms,
                vis_a_full_ms,
            );
        }
        wires
    }

    /// Unindexable entity handles in document order, cached per geometry epoch.
    pub(in crate::scene) fn unindexable_handles(&self) -> std::cell::Ref<'_, (u64, Vec<Handle>)> {
        {
            let cache = self.caches.unindexable_cache.borrow();
            if cache
                .as_ref()
                .is_some_and(|(epoch, _)| *epoch == self.geometry_epoch)
            {
                drop(cache);
                return std::cell::Ref::map(self.caches.unindexable_cache.borrow(), |c| {
                    c.as_ref().unwrap()
                });
            }
        }
        let handles: Vec<Handle> = self
            .document
            .entities()
            .filter(|entity| is_unindexable_entity(entity))
            .map(|entity| entity.common().handle)
            .collect();
        *self.caches.unindexable_cache.borrow_mut() = Some((self.geometry_epoch, handles));
        std::cell::Ref::map(self.caches.unindexable_cache.borrow(), |c| c.as_ref().unwrap())
    }

    /// Resolve block membership once per geometry epoch. A single addition is
    /// appended safely; any other delta rebuilds the document-ordered index.
    pub(in crate::scene) fn block_members(&self) -> std::cell::Ref<'_, BlockMembers> {
        let mut cached_epoch = None;
        {
            let cache = self.caches.block_members_cache.borrow();
            if let Some((epoch, _)) = cache.as_ref() {
                if *epoch == self.geometry_epoch {
                    drop(cache);
                    return std::cell::Ref::map(self.caches.block_members_cache.borrow(), |c| {
                        c.as_ref().unwrap()
                    });
                }
                cached_epoch = Some(*epoch);
            }
        }

        if let Some(since) = cached_epoch {
            let lone_add = self
                .replay_since(since)
                .filter(|deltas| deltas.len() == 1 && deltas[0].1 == ChangeKind::Added);
            if let Some(deltas) = lone_add {
                let handle = deltas[0].0;
                // Resolve the owner before touching the cache: both lookups
                // borrow, and `entity_block_map` is its own `RefCell`.
                let owner = self.document.get_entity(handle).and_then(|entity| {
                    let owner = entity.common().owner_handle;
                    if !owner.is_null() {
                        Some(owner)
                    } else {
                        self.entity_block_map().get(&handle).copied()
                    }
                });
                let mut cache = self.caches.block_members_cache.borrow_mut();
                if let Some((epoch, members)) = cache.as_mut() {
                    if let Some(owner) = owner {
                        members.entry(owner).or_default().push(handle);
                    }
                    *epoch = self.geometry_epoch;
                    drop(cache);
                    return std::cell::Ref::map(self.caches.block_members_cache.borrow(), |c| {
                        c.as_ref().unwrap()
                    });
                }
            }
        }
        let mut members: HashMap<Handle, Vec<Handle>> = HashMap::default();
        {
            let map = self.entity_block_map();
            for entity in self.document.entities() {
                let common = entity.common();
                let owner = common.owner_handle;
                if !owner.is_null() {
                    members.entry(owner).or_default().push(common.handle);
                } else if let Some(&block) = map.get(&common.handle) {
                    members.entry(block).or_default().push(common.handle);
                }
            }
        }
        *self.caches.block_members_cache.borrow_mut() = Some((self.geometry_epoch, members));
        std::cell::Ref::map(self.caches.block_members_cache.borrow(), |c| c.as_ref().unwrap())
    }

    /// Whether the entity is direct content of this block.
    pub(in crate::scene) fn belongs_to_visible_block(
        &self,
        entity_handle: Handle,
        owner_handle: Handle,
        block_handle: Handle,
    ) -> bool {
        if block_handle.is_null() {
            return true;
        }
        if owner_handle == block_handle {
            return true;
        }
        if !owner_handle.is_null() {
            return false;
        }

        // owner_handle is null (common in DXF files that omit group code 330).
        // Use the current layout's entity_handles as the authoritative list when
        // available — this prevents block-definition geometry from leaking into
        // the viewport even when owner handles are missing.
        if let Some(br) = self
            .document
            .block_records
            .iter()
            .find(|br| br.handle == block_handle)
        {
            if !br.entity_handles.is_empty() {
                return br.entity_handles.contains(&entity_handle);
            }
        }

        // P: epoch-cached reverse map replaces O(B) block_records scan.
        let map = self.entity_block_map();
        if let Some(&owner) = map.get(&entity_handle) {
            return owner == block_handle;
        }
        // Map miss. Permissive only when NO BlockRecord enumerated its
        // entity_handles — that's a legacy DXF that omits 330 group codes
        // everywhere, where dropping unknown-owner entities would empty
        // model space. When at least one block did enumerate, the file is
        // capable of declaring ownership, so an unknown-owner entity is
        // an orphan (typically a block-defn entity whose owner was lost on
        // round-trip) and must not leak into the queried block.
        if map.is_empty() {
            return true;
        }
        false
    }

    /// Build (and epoch-cache) a reverse map: entity_handle → block_record_handle,
    /// covering every entity explicitly listed in a block_record's entity_handles.
    pub(in crate::scene) fn entity_block_map(&self) -> std::cell::Ref<'_, HashMap<Handle, Handle>> {
        {
            let cache = self.caches.entity_block_map_cache.borrow();
            if let Some((epoch, _)) = *cache {
                if epoch == self.geometry_epoch {
                    drop(cache);
                    return std::cell::Ref::map(self.caches.entity_block_map_cache.borrow(), |c| {
                        &c.as_ref().unwrap().1
                    });
                }
            }
        }
        let mut map: HashMap<Handle, Handle> = HashMap::default();
        for br in self.document.block_records.iter() {
            for &eh in &br.entity_handles {
                map.insert(eh, br.handle);
            }
        }
        *self.caches.entity_block_map_cache.borrow_mut() = Some((self.geometry_epoch, map));
        std::cell::Ref::map(self.caches.entity_block_map_cache.borrow(), |c| {
            &c.as_ref().unwrap().1
        })
    }

    pub(in crate::scene) fn entity_dependency_signature(&self, entity: &EntityType) -> u64 {
        self.entity_dependency_signature_with(
            entity,
            &render_graph::entity_block_uses(&self.document, entity, 1.0),
            &normalize_name(&entity.common().layer),
        )
    }

    /// `block_uses` must be exactly what `entity_block_uses` returns for this
    /// entity, and `normalized_layer` the uppercased common layer. The rebuild
    /// passes what its roots loop already collected instead of recomputing
    /// them for every entity a second time. The hash feeds only in-memory
    /// cache-invalidation comparisons, so the hasher choice is free.
    pub(in crate::scene) fn entity_dependency_signature_with(
        &self,
        entity: &EntityType,
        block_uses: &[render_graph::BlockUse],
        normalized_layer: &str,
    ) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        let common = entity.common();
        common.handle.hash(&mut hasher);
        common.owner_handle.hash(&mut hasher);
        normalized_layer.hash(&mut hasher);
        normalize_name(&common.linetype).hash(&mut hasher);

        let category = match entity {
            EntityType::Point(_) => 1_u8,
            EntityType::Text(_)
            | EntityType::MText(_)
            | EntityType::AttributeDefinition(_)
            | EntityType::AttributeEntity(_)
            | EntityType::Dimension(_)
            | EntityType::Leader(_)
            | EntityType::Tolerance(_)
            | EntityType::MultiLeader(_)
            | EntityType::Table(_)
            | EntityType::Shape(_) => 2,
            EntityType::Hatch(_) => 3,
            EntityType::Insert(insert) if !insert.attributes.is_empty() => 4,
            EntityType::Wipeout(_) => 5,
            _ => 0,
        };
        category.hash(&mut hasher);
        crate::scene::annotative::is_annotative(&self.document, entity).hash(&mut hasher);

        for block_use in block_uses {
            block_use.block.hash(&mut hasher);
            normalize_name(&block_use.insert.block_name).hash(&mut hasher);
            block_use.role.hash(&mut hasher);
            block_use.scale_policy.hash(&mut hasher);
            block_use.active.hash(&mut hasher);
        }

        match entity {
            EntityType::Text(text) => normalize_name(&text.style).hash(&mut hasher),
            EntityType::MText(text) => normalize_name(&text.style).hash(&mut hasher),
            EntityType::AttributeDefinition(attribute) => {
                normalize_name(&attribute.text_style).hash(&mut hasher)
            }
            EntityType::AttributeEntity(attribute) => {
                normalize_name(&attribute.text_style).hash(&mut hasher)
            }
            EntityType::Insert(insert) => {
                for attribute in &insert.attributes {
                    normalize_name(&attribute.common.layer).hash(&mut hasher);
                    normalize_name(&attribute.text_style).hash(&mut hasher);
                }
            }
            EntityType::Dimension(dimension) => {
                normalize_name(&dimension.base().style_name).hash(&mut hasher)
            }
            EntityType::Leader(leader) => normalize_name(&leader.dimension_style).hash(&mut hasher),
            EntityType::Tolerance(tolerance) => {
                normalize_name(&tolerance.dimension_style_name).hash(&mut hasher)
            }
            EntityType::Table(table) => {
                table.table_style_handle.hash(&mut hasher);
                for row in &table.rows {
                    row.style
                        .as_ref()
                        .and_then(|style| style.text_style_handle)
                        .hash(&mut hasher);
                    for cell in &row.cells {
                        cell.style
                            .as_ref()
                            .and_then(|style| style.text_style_handle)
                            .hash(&mut hasher);
                        for content in &cell.contents {
                            content.text_style_handle.hash(&mut hasher);
                        }
                    }
                }
            }
            EntityType::MultiLeader(leader) => {
                leader.style_handle.hash(&mut hasher);
                leader.text_style_handle.hash(&mut hasher);
                leader.context.text_style_handle.hash(&mut hasher);
            }
            EntityType::MLine(line) => line.style_handle.hash(&mut hasher),
            _ => {}
        }
        hasher.finish()
    }

    pub(in crate::scene) fn refresh_dependency_index_for_changes(&self, changes: &[(Handle, ChangeKind)]) {
        let mut cache = self.caches.dependency_index_cache.borrow_mut();
        let Some(index) = cache.as_ref() else {
            return;
        };
        let unchanged = changes.iter().all(|(handle, kind)| {
            if !matches!(kind, ChangeKind::Modified) {
                return false;
            }
            let Some(entity) = self.document.get_entity(*handle) else {
                return false;
            };
            index.signatures.get(handle).copied() == Some(self.entity_dependency_signature(entity))
        });
        if !unchanged {
            cache.take();
        }
    }

    /// doc(hidden) public only so `benches/dependency_index_rebuild.rs` can
    /// time the real production rebuild that follows an edit-dropped cache.
    #[doc(hidden)]
    pub fn dependency_index_rebuild_for_bench(&self) -> (usize, usize, usize) {
        let index = self.rebuild_dependency_index();
        (
            index.layers.len(),
            index.text_styles.len(),
            index.block_roots.values().map(|roots| roots.len()).sum(),
        )
    }

    pub(in crate::scene) fn rebuild_dependency_index(&self) -> SceneDependencyIndex {
        let mut layout_blocks: HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|object| match object {
                document::objects::ObjectType::Layout(layout) if !layout.block_record.is_null() => {
                    Some(layout.block_record)
                }
                _ => None,
            })
            .collect();
        // DWG imports may have model/paper block records without matching
        // Layout objects. They still own directly rendered entities, so their
        // layer dependencies must resolve to those entity handles.
        layout_blocks.extend(self.document.block_records.iter().filter_map(|record| {
            let name = normalize_name(&record.name);
            (name.starts_with("*MODEL_SPACE") || name.starts_with("*PAPER_SPACE"))
                .then_some(record.handle)
        }));
        let block_names: HashMap<Handle, String> = self
            .document
            .block_records
            .iter()
            .map(|record| (record.handle, normalize_name(&record.name)))
            .collect();
        let membership: HashMap<Handle, Handle> = self
            .document
            .block_records
            .iter()
            .flat_map(|record| {
                record
                    .entity_handles
                    .iter()
                    .copied()
                    .map(move |handle| (handle, record.handle))
            })
            .collect();
        let text_style_names: HashMap<Handle, String> = self
            .document
            .text_styles
            .iter()
            .map(|style| (style.handle, style.name.clone()))
            .collect();
        let linetype_names: HashMap<Handle, String> = self
            .document
            .line_types
            .iter()
            .map(|lt| (lt.handle, lt.name.clone()))
            .collect();

        let mut roots: HashMap<String, HashSet<Handle>> = HashMap::default();
        let mut parents: HashMap<String, HashSet<String>> = HashMap::default();
        let mut block_uses_of: HashMap<Handle, Vec<render_graph::BlockUse>> = HashMap::default();
        for entity in self.document.entities() {
            let common = entity.common();
            let owner = if common.owner_handle.is_null() {
                membership
                    .get(&common.handle)
                    .copied()
                    .unwrap_or(Handle::NULL)
            } else {
                common.owner_handle
            };
            let block_uses = render_graph::entity_block_uses(&self.document, entity, 1.0);
            for block_use in &block_uses {
                let target = normalize_name(&block_use.insert.block_name);
                if target.is_empty() {
                    continue;
                }
                if owner.is_null() || layout_blocks.contains(&owner) {
                    roots.entry(target).or_default().insert(common.handle);
                } else if let Some(parent) = block_names.get(&owner) {
                    parents.entry(target).or_default().insert(parent.clone());
                }
            }
            if !block_uses.is_empty() {
                block_uses_of.insert(common.handle, block_uses);
            }
        }
        // Propagate root users through nested block references.
        // Fixed-point form is cycle-safe and block graphs are normally shallow.
        let mut changed = true;
        while changed {
            changed = false;
            for (child, parent_names) in &parents {
                let inherited: Vec<Handle> = parent_names
                    .iter()
                    .flat_map(|parent| roots.get(parent).into_iter().flatten().copied())
                    .collect();
                let entry = roots.entry(child.clone()).or_default();
                let before = entry.len();
                entry.extend(inherited);
                changed |= entry.len() != before;
            }
        }

        let mut index = SceneDependencyIndex::default();
        index.signatures.reserve(self.document.entity_count());
        let mut normalized_layers: HashMap<&str, String> = HashMap::default();
        let mut complex_linetypes: HashMap<&str, bool> = HashMap::default();
        let defines_complex = crate::catalog::linetypes::defines_complex_lt(&self.document);
        for entity in self.document.entities() {
            let common = entity.common();
            let owner = if common.owner_handle.is_null() {
                membership
                    .get(&common.handle)
                    .copied()
                    .unwrap_or(Handle::NULL)
            } else {
                common.owner_handle
            };
            let inside_block = !owner.is_null() && !layout_blocks.contains(&owner);
            let render_handles = if inside_block {
                RenderHandles::Block(block_names.get(&owner).and_then(|name| roots.get(name)))
            } else {
                RenderHandles::SelfOnly(common.handle)
            };
            if matches!(entity, EntityType::Wipeout(_)) {
                match &render_handles {
                    RenderHandles::SelfOnly(handle) => {
                        index.wipeout_hosts.insert(*handle);
                    }
                    RenderHandles::Block(Some(roots)) => {
                        index.wipeout_hosts.extend(roots.iter().copied());
                    }
                    RenderHandles::Block(None) => {}
                }
            }
            let block_uses = block_uses_of
                .get(&common.handle)
                .map(|uses| uses.as_slice())
                .unwrap_or(&[]);
            // Layers repeat across the document; memoize the uppercased key
            // instead of allocating it once per entity.
            let normalized_layer = normalized_layers
                .entry(common.layer.as_str())
                .or_insert_with(|| normalize_name(&common.layer));
            index.signatures.insert(
                common.handle,
                self.entity_dependency_signature_with(entity, block_uses, normalized_layer),
            );
            if matches!(entity, EntityType::Point(_)) {
                render_handles.extend_into(&mut index.points, common.handle);
            }
            if matches!(
                entity,
                EntityType::Text(_)
                    | EntityType::MText(_)
                    | EntityType::AttributeDefinition(_)
                    | EntityType::AttributeEntity(_)
                    | EntityType::Dimension(_)
                    | EntityType::Leader(_)
                    | EntityType::Tolerance(_)
                    | EntityType::MultiLeader(_)
                    | EntityType::Table(_)
                    | EntityType::Shape(_)
            ) || matches!(entity, EntityType::Insert(insert) if !insert.attributes.is_empty())
            {
                render_handles.extend_into(&mut index.text_geometry, common.handle);
            }
            // A complex linetype bakes the annotation scale into its wires, so
            // changing the scale has to re-tessellate it like annotative content.
            let mut is_complex = |name| {
                *complex_linetypes.entry(name).or_insert_with(|| {
                    crate::catalog::linetypes::resolve_complex_lt(&self.document, name).is_some()
                })
            };
            let lt_name = view::render::linetype_name_for_viewport(&self.document, entity, None);
            let complex_linetype = is_complex(lt_name);
            // An MLINE draws its elements' linetypes, read off its style; a style
            // without elements draws two ByLayer lines.
            let complex_mline = defines_complex
                && match entity {
                    EntityType::MLine(mline) => {
                        let layer = mline.common.layer.as_str();
                        match crate::entities::mline::resolved_mline_style(mline, &self.document)
                            .filter(|style| !style.elements.is_empty())
                        {
                            Some(style) => style.elements.iter().any(|element| {
                                is_complex(crate::entities::mline::element_linetype_name(
                                    &self.document,
                                    layer,
                                    &element.linetype,
                                    None,
                                ))
                            }),
                            None => is_complex(crate::entities::mline::element_linetype_name(
                                &self.document,
                                layer,
                                "ByLayer",
                                None,
                            )),
                        }
                    }
                    _ => false,
                };
            if complex_linetype
                || complex_mline
                || matches!(entity, EntityType::Hatch(_))
                || crate::scene::annotative::is_annotative(&self.document, entity)
            {
                render_handles.extend_into(&mut index.annotation_geometry, common.handle);
            }
            // The raw-name probe avoids a normalized-name allocation per
            // entity; a miss still inserts under the normalized key, so
            // case-variant names keep resolving to the same entry.
            let add = |map: &mut HashMap<String, DependencyTargets>, name: &str| {
                let target = match map.get_mut(name) {
                    Some(target) => target,
                    None => map.entry(normalize_name(name)).or_default(),
                };
                render_handles.extend_into(target, common.handle);
            };
            let add_handle = |map: &mut HashMap<Handle, DependencyTargets>,
                              handle: Option<Handle>| {
                let Some(handle) = handle.filter(|handle| !handle.is_null()) else {
                    return;
                };
                let target = map.entry(handle).or_default();
                render_handles.extend_into(target, common.handle);
            };
            let is_direct_linetype = |name: &str| {
                !name.is_empty()
                    && !name.eq_ignore_ascii_case("bylayer")
                    && !name.eq_ignore_ascii_case("byblock")
            };
            add(&mut index.layers, &common.layer);
            if is_direct_linetype(&common.linetype) {
                add(&mut index.linetypes, &common.linetype);
            }
            match entity {
                EntityType::Text(text) => add(&mut index.text_styles, &text.style),
                EntityType::MText(text) => add(&mut index.text_styles, &text.style),
                EntityType::AttributeDefinition(attribute) => {
                    add(&mut index.text_styles, &attribute.text_style)
                }
                EntityType::AttributeEntity(attribute) => {
                    add(&mut index.text_styles, &attribute.text_style)
                }
                EntityType::Insert(insert) => {
                    for attribute in &insert.attributes {
                        add(&mut index.layers, &attribute.common.layer);
                        if is_direct_linetype(&attribute.common.linetype) {
                            add(&mut index.linetypes, &attribute.common.linetype);
                        }
                        add(&mut index.text_styles, &attribute.text_style);
                    }
                }
                EntityType::Dimension(dimension) => {
                    add(&mut index.dim_styles, &dimension.base().style_name);
                    if let Some(style) = self.document.dim_styles.get(&dimension.base().style_name)
                    {
                        add(&mut index.text_styles, &style.dimtxsty);
                        for lt_handle in [
                            style.dimltex_handle,
                            style.dimltex1_handle,
                            style.dimltex2_handle,
                        ] {
                            if let Some(name) = linetype_names.get(&lt_handle) {
                                add(&mut index.linetypes, name);
                            }
                        }
                    }
                }
                EntityType::Leader(leader) => {
                    add(&mut index.dim_styles, &leader.dimension_style);
                    if let Some(style) = self.document.dim_styles.get(&leader.dimension_style) {
                        add(&mut index.text_styles, &style.dimtxsty);
                    }
                }
                EntityType::Tolerance(tolerance) => {
                    add(&mut index.dim_styles, &tolerance.dimension_style_name);
                    if let Some(style) = self
                        .document
                        .dim_styles
                        .get(&tolerance.dimension_style_name)
                    {
                        add(&mut index.text_styles, &style.dimtxsty);
                    }
                }
                EntityType::Table(table) => {
                    add_handle(&mut index.object_styles, table.table_style_handle);
                    if let Some(ObjectType::TableStyle(style)) = table
                        .table_style_handle
                        .and_then(|handle| self.document.objects.get(&handle))
                    {
                        for row in [
                            &style.data_row_style,
                            &style.header_row_style,
                            &style.title_row_style,
                        ] {
                            add(&mut index.text_styles, &row.text_style_name);
                        }
                    }
                    for row in &table.rows {
                        if let Some(style) = &row.style {
                            if let Some(name) = style
                                .text_style_handle
                                .and_then(|handle| text_style_names.get(&handle))
                            {
                                add(&mut index.text_styles, name);
                            }
                            for border in [
                                &style.top_border,
                                &style.right_border,
                                &style.bottom_border,
                                &style.left_border,
                            ]
                            .into_iter()
                            .chain(style.additional_borders.iter().map(|(_, border)| border))
                            {
                                if let Some(name) = border
                                    .line_type_handle
                                    .and_then(|handle| linetype_names.get(&handle))
                                {
                                    add(&mut index.linetypes, name);
                                }
                            }
                        }
                        for cell in &row.cells {
                            if let Some(style) = &cell.style {
                                if let Some(name) = style
                                    .text_style_handle
                                    .and_then(|handle| text_style_names.get(&handle))
                                {
                                    add(&mut index.text_styles, name);
                                }
                                for border in [
                                    &style.top_border,
                                    &style.right_border,
                                    &style.bottom_border,
                                    &style.left_border,
                                ]
                                .into_iter()
                                .chain(style.additional_borders.iter().map(|(_, border)| border))
                                {
                                    if let Some(name) = border
                                        .line_type_handle
                                        .and_then(|handle| linetype_names.get(&handle))
                                    {
                                        add(&mut index.linetypes, name);
                                    }
                                }
                            }
                            for content in &cell.contents {
                                if let Some(name) = content
                                    .text_style_handle
                                    .and_then(|handle| text_style_names.get(&handle))
                                {
                                    add(&mut index.text_styles, name);
                                }
                            }
                        }
                    }
                }
                EntityType::MultiLeader(leader) => {
                    add_handle(&mut index.object_styles, leader.style_handle);
                    for handle in [leader.text_style_handle, leader.context.text_style_handle] {
                        if let Some(name) = handle.and_then(|handle| text_style_names.get(&handle))
                        {
                            add(&mut index.text_styles, name);
                        }
                    }
                    for handle in
                        [leader.line_type_handle].into_iter().chain(
                            leader.context.leader_roots.iter().flat_map(|root| {
                                root.lines.iter().map(|line| line.line_type_handle)
                            }),
                        )
                    {
                        if let Some(name) = handle.and_then(|handle| linetype_names.get(&handle)) {
                            add(&mut index.linetypes, name);
                        }
                    }
                    if let Some(ObjectType::MultiLeaderStyle(style)) = leader
                        .style_handle
                        .and_then(|handle| self.document.objects.get(&handle))
                    {
                        if let Some(name) = style
                            .text_style_handle
                            .and_then(|handle| text_style_names.get(&handle))
                        {
                            add(&mut index.text_styles, name);
                        }
                        if let Some(name) = style
                            .line_type_handle
                            .and_then(|handle| linetype_names.get(&handle))
                        {
                            add(&mut index.linetypes, name);
                        }
                    }
                }
                EntityType::MLine(line) => {
                    add_handle(&mut index.object_styles, line.style_handle);
                    if let Some(style) =
                        crate::entities::mline::resolved_mline_style(line, &self.document)
                    {
                        for element in &style.elements {
                            if is_direct_linetype(&element.linetype) {
                                add(&mut index.linetypes, &element.linetype);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        index.block_roots = roots;
        index
    }

    pub(in crate::scene) fn dependency_targets(&self, kind: DependencyKind, names: &[String]) -> DependencyTargets {
        if self.caches.dependency_index_cache.borrow().is_none() {
            *self.caches.dependency_index_cache.borrow_mut() =
                Some(Arc::new(self.rebuild_dependency_index()));
        }
        let cache = self.caches.dependency_index_cache.borrow();
        let index = cache.as_ref().unwrap();
        let map = match kind {
            DependencyKind::Layer => &index.layers,
            DependencyKind::Linetype => &index.linetypes,
            DependencyKind::TextStyle => &index.text_styles,
            DependencyKind::DimStyle => &index.dim_styles,
        };
        let mut combined = DependencyTargets::default();
        for name in names {
            let Some(target) = map.get(&normalize_name(name)) else {
                continue;
            };
            combined.extend_from(target);
        }
        combined
    }

    pub(in crate::scene) fn invalidate_dependency_targets(&mut self, targets: DependencyTargets) {
        if targets.render_handles.is_empty() {
            return;
        }
        if targets.touches_block_definition {
            self.block_epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
        }
        let sources: Vec<Handle> = targets.source_handles.iter().copied().collect();
        self.recolor_meshes_for_handles(&sources);
        let changes: Vec<(Handle, ChangeKind)> = targets
            .render_handles
            .into_iter()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.bump_entities_restyled(&changes);
    }

    /// Off/freeze toggle on `names`, nothing else changed. Visibility gates an
    /// entity before it is tessellated, so entities carrying these layers keep
    /// their tessellation memo and only leave or re-enter the resident set;
    /// block users re-expand, since their definitions bake the children's
    /// visibility. Nothing is recoloured.
    pub fn invalidate_layer_visibility(&mut self, names: &[String]) {
        // Turning a layer off or freezing it removes its entities from the
        // interactive scene as well as from rendering. Drop any entities that
        // were already selected before the visibility change, otherwise a
        // subsequent MOVE/ERASE/etc. could still modify invisible geometry.
        let block = self.interaction_block_handle();
        let frozen: Option<HashSet<Handle>> = self
            .interaction_viewport_frozen_layers()
            .map(|layers| layers.iter().copied().collect());
        let annotation_scale_handle = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();

        let visible_selection: Vec<Handle> = self
            .selected_handles_in_order()
            .into_iter()
            .filter(|handle| {
                self.document.get_entity(*handle).is_some_and(|entity| {
                    self.resident_entity_visible(
                        entity,
                        block,
                        frozen.as_ref(),
                        annotation_scale_handle,
                        all_visible,
                    )
                })
            })
            .collect();
        self.replace_selection_exact(&visible_selection);

        let targets = self.dependency_targets(DependencyKind::Layer, names);
        if targets.render_handles.is_empty() {
            return;
        }
        if targets.touches_block_definition {
            let records: HashMap<Handle, &str> = self
                .document
                .block_records
                .iter()
                .map(|record| (record.handle, record.name.as_str()))
                .collect();
            let touched: Vec<String> = targets
                .source_handles
                .iter()
                .filter_map(|handle| self.document.get_entity(*handle))
                .filter_map(|entity| records.get(&entity.common().owner_handle))
                .map(|name| name.to_string())
                .collect();
            self.mark_block_definitions_dirty(touched);
        }
        let kept: Vec<_> = {
            let mut tm = self.caches.tess_memo.borrow_mut();
            let mut rm = self.caches.resident_tess_memo.borrow_mut();
            targets
                .render_handles
                .iter()
                .filter(|handle| !targets.block_users.contains(handle))
                .map(|&handle| (handle, tm.remove(&handle), rm.remove(&handle)))
                .collect()
        };
        let changes: Vec<(Handle, ChangeKind)> = targets
            .render_handles
            .iter()
            .map(|&handle| (handle, ChangeKind::Modified))
            .collect();
        self.bump_entities_restyled(&changes);
        // A series of layer toggles can name thousands of handles, but most
        // are hidden and never enter the overlay's replacement index.
        // Keep using the base instead of rescanning all resident geometry.
        if let Some(delta) = self.caches.geometry_deltas.get_mut().back_mut() {
            debug_assert_eq!(delta.epoch, self.geometry_epoch);
            delta.layer_visibility_only = true;
        }
        let mut tm = self.caches.tess_memo.borrow_mut();
        let mut rm = self.caches.resident_tess_memo.borrow_mut();
        for (handle, culled, resident) in kept {
            if let Some(wires) = culled {
                tm.insert(handle, wires);
            }
            if let Some(wires) = resident {
                rm.insert(handle, wires);
            }
        }
    }

    pub fn invalidate_layer_dependencies(&mut self, names: &[String]) {
        let targets = self.dependency_targets(DependencyKind::Layer, names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_linetype_dependencies(&mut self, names: &[String]) {
        if names.is_empty() {
            return;
        }
        let wanted: HashSet<String> = names.iter().map(|name| normalize_name(name)).collect();
        let wanted_handles: HashSet<Handle> = self
            .document
            .line_types
            .iter()
            .filter(|lt| wanted.contains(&normalize_name(&lt.name)))
            .map(|lt| lt.handle)
            .collect();
        let affected_layers: Vec<String> = self
            .document
            .layers
            .iter()
            .filter(|layer| {
                wanted.contains(&normalize_name(&layer.line_type))
                    || (!wanted_handles.is_empty()
                        && self
                            .document
                            .layer_viewport_overrides(
                                layer.handle,
                                document::objects::KnownXRecordKind::LayerViewportLinetypeOverride,
                            )
                            .into_iter()
                            .any(|(_, value)| {
                                value
                                    .as_handle()
                                    .is_some_and(|handle| wanted_handles.contains(&handle))
                            }))
            })
            .map(|layer| layer.name.clone())
            .collect();
        let mut targets = self.dependency_targets(DependencyKind::Linetype, names);
        if !affected_layers.is_empty() {
            let layer_targets = self.dependency_targets(DependencyKind::Layer, &affected_layers);
            targets.extend_from(&layer_targets);
        }
        self.invalidate_dependency_targets(targets);
        self.invalidate_dependency_index();
    }

    pub fn invalidate_text_style_dependencies(&mut self, name: &str) {
        self.invalidate_text_style_dependencies_many(&[name.to_string()]);
    }

    pub fn invalidate_text_style_dependencies_many(&mut self, names: &[String]) {
        let targets = self.dependency_targets(DependencyKind::TextStyle, names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_dim_style_dependencies(&mut self, name: &str) {
        self.invalidate_dim_style_dependencies_many(&[name.to_string()]);
    }

    pub fn invalidate_dim_style_dependencies_many(&mut self, names: &[String]) {
        let targets = self.dependency_targets(DependencyKind::DimStyle, names);
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_object_style_dependencies(&mut self, handles: &[Handle]) {
        if self.caches.dependency_index_cache.borrow().is_none() {
            *self.caches.dependency_index_cache.borrow_mut() =
                Some(Arc::new(self.rebuild_dependency_index()));
        }
        let mut combined = DependencyTargets::default();
        if let Some(index) = self.caches.dependency_index_cache.borrow().as_ref() {
            for handle in handles {
                let Some(target) = index.object_styles.get(handle) else {
                    continue;
                };
                combined
                    .render_handles
                    .extend(target.render_handles.iter().copied());
                combined
                    .source_handles
                    .extend(target.source_handles.iter().copied());
                combined.touches_block_definition |= target.touches_block_definition;
            }
        }
        self.invalidate_dependency_targets(combined);
    }

    pub(in crate::scene) fn invalidate_dependency_category(
        &mut self,
        select: impl FnOnce(&SceneDependencyIndex) -> &DependencyTargets,
    ) {
        if self.caches.dependency_index_cache.borrow().is_none() {
            *self.caches.dependency_index_cache.borrow_mut() =
                Some(Arc::new(self.rebuild_dependency_index()));
        }
        let targets = self
            .caches.dependency_index_cache
            .borrow()
            .as_deref()
            .map(select)
            .cloned()
            .unwrap_or_default();
        self.invalidate_dependency_targets(targets);
    }

    pub fn invalidate_point_dependencies(&mut self) {
        self.invalidate_dependency_category(|index| &index.points);
    }

    pub fn invalidate_text_geometry_dependencies(&mut self) {
        self.invalidate_dependency_category(|index| &index.text_geometry);
    }

    pub fn invalidate_annotation_dependencies(&mut self) {
        self.invalidate_dependency_category(|index| &index.annotation_geometry);
    }

    pub(crate) fn invalidate_dependency_index(&self) {
        self.caches.dependency_index_cache.borrow_mut().take();
        // Objects (and so DIMASSOC associations) may have appeared or gone;
        // the direction that matters is undo/redo and plugin edits that
        // bypass `store_dimension_association_full`.
        self.has_dimension_associations.set(None);
        // Entity additions, erasures, in-place restores (undo/redo replays
        // entity images) and plugin edits can add, drop or re-point a centre
        // association without naming a line as modified.
        self.caches.associative_centerline_source_cache.borrow_mut().take();
        self.caches.associative_centermark_source_cache.borrow_mut().take();
    }

    /// Spatial index + always-emit list for top-level entities. Lazily
    /// rebuilt on `geometry_epoch` change.
    ///
    /// `tree` holds entities whose `bounding_box()` is finite and
    /// non-degenerate. `unbounded_handles` holds entities whose bbox
    /// is degenerate or non-finite — the legacy `entity_aabb` treated
    /// those as `UNBOUNDED_AABB` (never culled), so the wire path must
    /// always emit them regardless of view. Inserts/Viewports/Blocks
    /// /BlockEnds are filtered out at build time and re-added by the
    /// wire path via a separate scan (their WCS bbox depends on
    /// transforms handled elsewhere).
    pub(crate) fn entity_index(&self) -> std::cell::Ref<'_, EntityIndex> {
        {
            let cache = self.caches.entity_index_cache.borrow();
            if let Some((epoch, _)) = *cache {
                if epoch == self.geometry_epoch {
                    drop(cache);
                    return std::cell::Ref::map(self.caches.entity_index_cache.borrow(), |c| {
                        &c.as_ref().unwrap().1
                    });
                }
            }
        }

        // Incremental path: if the cache is only a few edits behind, patch just
        // the changed handles into the existing quadtree (which supports
        // insert/remove/update, out-of-root items falling into overflow) instead
        // of re-walking every entity. Resolve each change against the document
        // first, then apply under one borrow.
        enum IdxOp {
            Remove(Handle),
            SetBounded(Handle, [f64; 4]),
            SetUnbounded(Handle),
        }
        let cached_epoch = self.caches.entity_index_cache.borrow().as_ref().map(|c| c.0);
        if let Some(ce) = cached_epoch {
            if let Some(deltas) = self.replay_since(ce) {
                let mut ops: Vec<IdxOp> = Vec::with_capacity(deltas.len());
                for (h, kind) in &deltas {
                    match kind {
                        ChangeKind::Removed => ops.push(IdxOp::Remove(*h)),
                        ChangeKind::Added | ChangeKind::Modified => {
                            match self.document.get_entity(*h) {
                                None => ops.push(IdxOp::Remove(*h)),
                                Some(e) if is_unindexable_entity(e) => ops.push(IdxOp::Remove(*h)),
                                Some(e) => match entity_world_aabb_f64(e) {
                                    Some(ab) => ops.push(IdxOp::SetBounded(*h, ab)),
                                    None => ops.push(IdxOp::SetUnbounded(*h)),
                                },
                            }
                        }
                    }
                }
                let mut cache = self.caches.entity_index_cache.borrow_mut();
                if let Some((epoch, idx)) = cache.as_mut() {
                    // replay_since coalesces to one entry per handle, so each
                    // touched handle appears in exactly one op: sweep the
                    // unbounded list once for the whole batch instead of one
                    // retain per op (O(ops x handles) on point-heavy drawings).
                    let mut touched: HashSet<Handle> = HashSet::default();
                    let mut unbounded_pushes: Vec<Handle> = Vec::new();
                    for op in &ops {
                        match *op {
                            IdxOp::Remove(h) => {
                                idx.tree.remove(h);
                                touched.insert(h);
                            }
                            IdxOp::SetBounded(h, ab) => {
                                idx.tree.update(h, ab);
                                touched.insert(h);
                            }
                            IdxOp::SetUnbounded(h) => {
                                idx.tree.remove(h);
                                touched.insert(h);
                                unbounded_pushes.push(h);
                            }
                        }
                    }
                    idx.unbounded_handles.retain(|x| !touched.contains(x));
                    idx.unbounded_handles.extend(unbounded_pushes);
                    *epoch = self.geometry_epoch;
                    drop(cache);
                    return std::cell::Ref::map(self.caches.entity_index_cache.borrow(), |c| {
                        &c.as_ref().unwrap().1
                    });
                }
            }
        }

        let perf = crate::perf::enabled();
        let t_aabbs_start = perf.then(iced::time::Instant::now);
        let mut items: Vec<(Handle, [f64; 4])> = Vec::new();
        let mut unbounded: Vec<Handle> = Vec::new();
        let mut union: Option<[f64; 4]> = None;
        for e in self.document.entities() {
            if is_unindexable_entity(e) {
                continue;
            }
            match entity_world_aabb_f64(e) {
                Some(ab) => {
                    union = Some(match union {
                        None => ab,
                        Some(u) => [
                            u[0].min(ab[0]),
                            u[1].min(ab[1]),
                            u[2].max(ab[2]),
                            u[3].max(ab[3]),
                        ],
                    });
                    items.push((e.common().handle, ab));
                }
                None => unbounded.push(e.common().handle),
            }
        }
        let aabbs_ms = crate::perf::elapsed_ms(t_aabbs_start);
        let root = match union {
            Some(u) => {
                let w = (u[2] - u[0]).max(1.0);
                let h = (u[3] - u[1]).max(1.0);
                let mx = w * 0.01;
                let my = h * 0.01;
                [u[0] - mx, u[1] - my, u[2] + mx, u[3] + my]
            }
            None => [-1.0, -1.0, 1.0, 1.0],
        };
        let t_tree_start = perf.then(iced::time::Instant::now);
        let tree = pick::quadtree::QuadTree::from_items(root, items);
        let tree_ms = crate::perf::elapsed_ms(t_tree_start);

        if perf {
            crate::perf_record!(
                "[perf] entity_index-rebuild aabbs={:.1}ms tree={:.1}ms entities={} root={:?}",
                aabbs_ms,
                tree_ms,
                tree.len(),
                root,
            );
        }

        *self.caches.entity_index_cache.borrow_mut() = Some((
            self.geometry_epoch,
            EntityIndex {
                tree,
                unbounded_handles: unbounded,
            },
        ));
        std::cell::Ref::map(self.caches.entity_index_cache.borrow(), |c| &c.as_ref().unwrap().1)
    }

    /// Resolve the active layout and annotation context for tessellation.
    ///
    /// This scans the drawing's objects for the annotation scale and for the
    /// current layout. A pass over many entities takes it one time, at its
    /// first entity, and gives it to [`Self::tessellate_one_in`].
    pub(in crate::scene) fn tessellation_context(&self) -> TessellationContext {
        #[cfg(test)]
        TESSELLATION_CONTEXTS.with(|count| count.set(count.get() + 1));
        let is_model = self.current_layout == "Model";
        let bg = if is_model {
            self.bg_color
        } else {
            self.paper_bg_color
        };
        let anno = if is_model {
            self.annotation_scale
        } else if let Some(viewport) = self.active_viewport {
            self.viewport_annotation_multiplier(viewport)
        } else {
            1.0
        };
        let annotation_scale_handle = if is_model {
            crate::scene::annotative::scale_handle_by_name(
                &self.document,
                &self.document.header.current_annotation_scale,
            )
        } else if let Some(viewport) = self.active_viewport {
            self.viewport_scale_handle(viewport)
        } else {
            self.paper_annotation_scale_handle()
        };
        let blk_cache = self.block_cache_arc_for(
            annotation_scale_handle,
            self.annotation_all_visible(),
            self.active_viewport,
        );
        (bg, anno, annotation_scale_handle, blk_cache, !is_model)
    }

    pub(in crate::scene) fn tessellate_one(&self, e: &EntityType) -> Vec<WireModel> {
        self.tessellate_one_in(&std::cell::OnceCell::new(), e)
    }

    /// [`Self::tessellate_one`] for a pass over many entities: the first call
    /// resolves the context into `context` and the later calls use it. The
    /// scene cannot change during the pass, so the context stays correct. A
    /// pass that tessellates nothing resolves nothing, as before.
    pub(in crate::scene) fn tessellate_one_in(
        &self,
        context: &std::cell::OnceCell<TessellationContext>,
        e: &EntityType,
    ) -> Vec<WireModel> {
        let (bg, anno, annotation_scale_handle, blk_cache, paper) =
            context.get_or_init(|| self.tessellation_context());
        let (bg, anno, annotation_scale_handle, paper) =
            (*bg, *anno, *annotation_scale_handle, *paper);
        // tessellate_one is used for one-off lookups (hit test, properties).
        // Skip culling here so the caller always gets the full geometry.
        let mut wires = tessellate_entity(
            &self.document,
            &self.selected,
            self.active_viewport,
            bg,
            anno,
            annotation_scale_handle,
            e,
            Some(blk_cache),
            None,
            None,
            paper.then_some(1.0),
            self.reference_only.handles(),
        );
        // Same solid-corner snaps as the resident paths, so one-off consumers
        // (overlay changed-wires, edit previews) agree with hit-test/snap.
        self.attach_solid_snaps(e.common().handle, &mut wires);
        self.shade_constraint_wires(e.common().owner_handle, &mut wires);
        wires
    }

    /// `doc(hidden)` public only so `benches/model_space_handle_lookup.rs`
    /// can time the real production lookup.
    #[doc(hidden)]
    pub fn model_space_block_handle(&self) -> Handle {
        let key = (
            self.geometry_epoch,
            self.layout_epoch,
            self.document.objects.len(),
        );
        if let Some((geometry, layout, objects, handle)) = self.model_space_block_cache.get() {
            if (geometry, layout, objects) == key {
                return handle;
            }
        }
        let handle = self.resolve_model_space_block_handle();
        self.model_space_block_cache
            .set(Some((key.0, key.1, key.2, handle)));
        handle
    }

    pub(in crate::scene) fn resolve_model_space_block_handle(&self) -> Handle {
        // Primary: Layout object's block_record (DWG reader sets this).
        if let Some(h) = self.document.objects.values().find_map(|obj| {
            if let ObjectType::Layout(l) = obj {
                if l.name == "Model" && !l.block_record.is_null() {
                    Some(l.block_record)
                } else {
                    None
                }
            } else {
                None
            }
        }) {
            return h;
        }
        // Fallback for DXF files: conventional block-record name.
        self.document
            .block_records
            .get("*Model_Space")
            .map(|br| br.handle)
            .unwrap_or(Handle::NULL)
    }

    /// Compute the axis-aligned bounding box of all model-space entities.
    /// Result is epoch-cached so repeated ZOOM E / auto-fit calls are O(1).
    pub fn model_space_extents(&self) -> Option<(glam::Vec3, glam::Vec3)> {
        {
            let cache = self.caches.model_extents_cache.borrow();
            if let Some((epoch, ext)) = *cache {
                if epoch == self.geometry_epoch {
                    return ext;
                }
            }
        }
        let result = self.compute_model_space_extents();
        *self.caches.model_extents_cache.borrow_mut() = Some((self.geometry_epoch, result));
        result
    }

    /// The AABB centre of the current selection, in absolute world coordinates
    /// (same space as `Camera::target`) — the point the 3D view orbits around
    /// when something is selected. `None` keeps the current camera target as
    /// the orbit centre. (#229)
    pub fn orbit_pivot(&self) -> Option<glam::DVec3> {
        self.navigation_selection_bounds()
            .map(|(min, max)| (min + max) * 0.5)
    }

    pub(crate) fn navigation_selection_bounds(&self) -> Option<(glam::DVec3, glam::DVec3)> {
        if self.selected.is_empty() {
            return None;
        }
        let model_view = self.current_layout == "Model" || self.active_viewport.is_some();
        let block = if self.active_viewport.is_some() {
            self.model_space_block_handle()
        } else {
            self.current_layout_block_handle()
        };
        let scale = if model_view {
            crate::scene::annotative::scale_handle_by_name(
                &self.document,
                &self.document.header.current_annotation_scale,
            )
        } else {
            self.paper_annotation_scale_handle()
        };
        let wires = self.wires_for_block_culled(
            block,
            None,
            None,
            None,
            None,
            scale,
            self.annotation_all_visible(),
            None,
            Some(&self.selected),
        );
        let mut min = glam::DVec3::splat(f64::INFINITY);
        let mut max = glam::DVec3::splat(f64::NEG_INFINITY);
        let mut any = false;
        let mut include = |point: glam::DVec3| {
            if point.is_finite() {
                min = min.min(point);
                max = max.max(point);
                any = true;
            }
        };
        for wire in &wires {
            let Some(h) = Self::handle_from_wire_name(&wire.name) else {
                continue;
            };
            if !self.selected.contains(&h) {
                continue;
            }
            for (index, &[x, y, z]) in wire.points.iter().enumerate() {
                let [lx, ly, lz] = wire.points_low.get(index).unwrap_or([0.0; 3]);
                include(glam::DVec3::new(
                    x as f64 + lx as f64,
                    y as f64 + ly as f64,
                    z as f64 + lz as f64,
                ));
            }
        }
        for set in self.interaction_meshes_arc().iter() {
            let Some(handle) = set.entity_handle() else {
                continue;
            };
            if !self.selected.contains(&handle) {
                continue;
            }
            if let Some(aabb) = mesh_interaction_aabb(set) {
                include(glam::DVec3::new(aabb[0], aabb[1], aabb[2]));
                include(glam::DVec3::new(aabb[3], aabb[4], aabb[5]));
            }
        }
        if any {
            Some((min, max))
        } else {
            None
        }
    }

    pub(in crate::scene) fn compute_model_space_extents(&self) -> Option<(glam::Vec3, glam::Vec3)> {
        let model_block = self.model_space_block_handle();
        // Shared with ZOOM EXTENTS so the two never disagree about what the
        // drawing covers. The previous walk here read only `key_vertices`,
        // which is empty for every tessellated curve (see `WireModel`) — so
        // circles, arcs, ellipses and splines contributed nothing, and
        // neither did hatches or images. Everything that reads drawing
        // extents (plot Extents, sheet-set framing, viewport auto-fit,
        // SpaceMouse model bounds) inherited that blind spot.
        if let Some((min, max)) = self.visible_block_bounds(model_block) {
            return Some((min.as_vec3(), max.as_vec3()));
        }
        // Last resort: saved EXTMIN/EXTMAX before the wire cache is built.
        const SANE_EXTENT: f64 = 1.0e16;
        let h = &self.document.header;
        let hmin = h.model_space_extents_min;
        let hmax = h.model_space_extents_max;
        if hmin.x < hmax.x
            && hmin.y < hmax.y
            && hmin.x.abs() < SANE_EXTENT
            && hmax.x.abs() < SANE_EXTENT
            && hmin.y.abs() < SANE_EXTENT
            && hmax.y.abs() < SANE_EXTENT
        {
            return Some((
                glam::Vec3::new(hmin.x as f32, hmin.y as f32, hmin.z as f32),
                glam::Vec3::new(hmax.x as f32, hmax.y as f32, hmax.z as f32),
            ));
        }
        None
    }

    /// Set a newly created viewport's `view_target` and `view_height` so that
    /// all model-space content is visible at a reasonable scale.
    pub fn auto_fit_viewport(&mut self, vp_handle: Handle) {
        self.fit_viewport_to_model(vp_handle, 1.1);
    }

    /// Fit a viewport's view to the model-space content, with `margin` (1 is
    /// tight) around it.
    pub fn fit_viewport_to_model(&mut self, vp_handle: Handle, margin: f64) {
        let extents = self.model_space_extents();
        let (min, max) = match extents {
            Some(e) => e,
            None => return,
        };
        let (min, max) = (min.as_dvec3(), max.as_dvec3());
        let center = (min + max) * 0.5;

        let vp = match self.document.get_entity_mut(vp_handle) {
            Some(document::EntityType::Viewport(vp)) => vp,
            _ => return,
        };
        let direction = glam::DVec3::new(
            vp.view_direction.x,
            vp.view_direction.y,
            vp.view_direction.z,
        )
        .normalize_or(glam::DVec3::Z);
        // The extents box as seen along the view direction: its span across
        // the view's right and up axes. Looking down Z that is its X/Y size.
        let right = glam::DVec3::Z.cross(direction).normalize_or(glam::DVec3::X);
        let up = direction.cross(right);
        let (mut span_right, mut span_up) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
        for corner in 0..8 {
            let point = glam::DVec3::new(
                if corner & 1 == 0 { min.x } else { max.x },
                if corner & 2 == 0 { min.y } else { max.y },
                if corner & 4 == 0 { min.z } else { max.z },
            );
            let (along_right, along_up) = (point.dot(right), point.dot(up));
            span_right = (span_right.0.min(along_right), span_right.1.max(along_right));
            span_up = (span_up.0.min(along_up), span_up.1.max(along_up));
        }
        let content_w = (span_right.1 - span_right.0).max(1e-3);
        let content_h = (span_up.1 - span_up.0).max(1e-3);

        // Aim at the model's centre; a plan view keeps it on the XY plane.
        vp.view_target.x = center.x;
        vp.view_target.y = center.y;
        vp.view_target.z = if direction == glam::DVec3::Z {
            0.0
        } else {
            center.z
        };

        // Choose the scale that fits both dimensions with the margin.
        let scale_w = vp.width / (content_w * margin);
        let scale_h = vp.height / (content_h * margin);
        let fit_scale = scale_w.min(scale_h).min(1000.0).max(1e-6);

        vp.custom_scale = fit_scale;
        vp.view_height = vp.height / fit_scale;
    }
}
