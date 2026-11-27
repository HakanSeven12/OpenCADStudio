use super::*;

impl Scene {
    /// Cached tessellation of the current layout block's paper-space entities,
    /// shared by `entity_wires_arc()` and the GPU sheet viewport. A thin
    /// "dressing" layer over the unified resident set (border drop +
    /// printable-area guide) — camera-independent, epoch-keyed, so paper
    /// pan/zoom never re-tessellates the sheet.
    pub(in crate::scene) fn paper_sheet_wires_arc(&self) -> Arc<Vec<WireModel>> {
        {
            let cache = self.caches.paper_sheet_cache.borrow();
            if let Some((epoch, gen, arc)) = cache.get(&self.current_layout) {
                if *epoch == self.geometry_epoch {
                    self.last_model_wire_gen.set(*gen);
                    return Arc::clone(arc);
                }
            }
        }
        let layout_block = self.current_layout_block_handle();
        let scale = self.paper_annotation_scale_handle();
        let base = self.resident_wires_for(layout_block, None, scale, None, None);
        let mut wires = (*base).clone();
        // The overall "sheet" viewport now IS the paper view itself, so its own
        // border rectangle must not be drawn as an entity on the sheet.
        let sheet = self.current_layout_sheet_viewport_handle();
        if sheet.is_valid() {
            let sheet_name = sheet.value().to_string();
            wires.retain(|w| w.name != sheet_name);
        }
        // Printable-area guide (paper inset by plot margins), paper space only.
        if let Some(pa) = self.printable_area_wire() {
            wires.push(pa);
        }
        let arc = Arc::new(wires);
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        let mut cache = self.caches.paper_sheet_cache.borrow_mut();
        cache.retain(|_, (epoch, _, _)| *epoch == self.geometry_epoch);
        cache.insert(
            self.current_layout.clone(),
            (self.geometry_epoch, gen, Arc::clone(&arc)),
        );
        arc
    }

    /// Build WireModels from all document entities for the current layout.
    /// Returns a shared `Arc` so `build_primitive()` can skip the clone during
    /// navigation frames where no preview wires are active.
    pub(crate) fn entity_wires_arc(&self) -> Arc<Vec<WireModel>> {
        let key = (self.geometry_epoch, self.camera_generation);
        {
            let cache = self.caches.wire_cache.borrow();
            if let Some((cached_key, gen, ref arc)) = *cache {
                if cached_key == key {
                    self.last_model_wire_gen.set(gen);
                    return Arc::clone(arc);
                }
            }
        }
        let layout_block = self.current_layout_block_handle();
        // Model space: reuse the resident, camera-independent static wire set the
        // render already holds (keyed on geometry_epoch only). This is the FULL
        // entity set — pick / snap want every entity, not a view-culled subset —
        // and it does NOT re-tessellate on a camera move. The old path went
        // through the camera_generation-keyed, view-culled paper_sheet set, so
        // every pan/rotate cold-missed it and paid a full O(visible) re-tess
        // (~300 ms on large drawings) the first time hit-testing ran after the
        // move — the "jump" at the start of each gesture. The tile args are
        // unused by `model_tile_wires_arc`.
        if self.current_layout == "Model" {
            let cam = self.camera.borrow().clone();
            return self.model_tile_wires_arc(0, &cam, 1.0, 1.0);
        }
        // Paper space: extend sheet wires with projected viewport content.
        // This composite is the CPU pick/snap set; the projection follows the
        // paper camera, so the key keeps `camera_generation`.
        let mut wires = (*self.paper_sheet_wires_arc()).clone();
        wires.extend(self.viewport_content_wires(layout_block, None, None, false, None));
        let arc = Arc::new(wires);
        let gen = WIRE_CONTENT_GEN.fetch_add(1, Ordering::Relaxed);
        self.last_model_wire_gen.set(gen);
        *self.caches.wire_cache.borrow_mut() = Some((key, gen, Arc::clone(&arc)));
        arc
    }

    /// Build WireModels from all document entities + optional preview wire.
    ///
    /// Returns the memoized set by `Arc` — callers only ever iterate it, and
    /// the previous per-call deep clone was a full copy of every wire buffer
    /// (hundreds of MB on large mesh imports, #358).
    pub fn entity_wires(&self) -> Arc<Vec<WireModel>> {
        let wires = self.entity_wires_arc();
        match crate::scene::model::instance_model::expand_stubs(&wires) {
            std::borrow::Cow::Borrowed(_) => wires,
            std::borrow::Cow::Owned(expanded) => Arc::new(expanded),
        }
    }

    /// Return paper entities and projected model-viewport entities separately.
    /// A plot-only render override is applied to cloned wires; viewport entities
    /// and their saved display modes remain unchanged.
    pub fn plot_wire_groups(
        &self,
        render_mode_override: Option<document::entities::ViewportRenderMode>,
    ) -> (Vec<WireModel>, Vec<WireModel>) {
        let _constraint_display = self.suppress_constraint_shading();
        let apply_mode = |wires: &mut Vec<WireModel>, mode| {
            let flags = view::render::render_mode_flags(mode);
            for wire in wires.iter_mut().filter(|wire| wire.fill_is_3d) {
                if !flags.face3d_fill && !flags.mesh_fill {
                    wire.fill_tris.clear();
                    wire.fill_tris_low.clear();
                }
                if !flags.show_3d_edges {
                    wire.points.clear();
                    wire.points_low.clear();
                }
            }
        };
        if self.current_layout == "Model" {
            let mut wires =
                crate::scene::model::instance_model::expand_stubs(&self.entity_wires_arc())
                    .into_owned();
            // Model space plots in plan, so solids show their top view.
            let mode = render_mode_override.unwrap_or_else(|| self.active_model_tile_render_mode());
            wires.extend(self.mesh_plot_wires(
                &self.meshes_arc(),
                [0.0, 0.0, 1.0],
                None,
                Some(mode),
            ));
            apply_mode(&mut wires, mode);
            // On screen the wire shader multiplies dash patterns by this ratio;
            // a plot has no shader, so apply it to the patterns themselves.
            if let Some(ratio) = view::render::model_linetype_ratio(&self.document) {
                for wire in wires.iter_mut().filter(|wire| wire.pattern_length > 0.0) {
                    wire.pattern_length *= ratio;
                    wire.pattern = wire.pattern.map(|value| value * ratio);
                }
            }
            return (wires, Vec::new());
        }
        let paper_block = self.current_layout_block_handle();
        let (_, _, viewport_handles) = self.paper_viewport_handles();
        let mut model_wires = Vec::new();
        for handle in viewport_handles.iter().copied() {
            let Some(EntityType::Viewport(viewport)) = self.document.get_entity(handle) else {
                continue;
            };
            if viewport.common.owner_handle != paper_block || !viewport.is_on() {
                continue;
            }
            // The viewport's shade plot: 1 wireframe, 2 hidden, 3 rendered;
            // anything else plots as displayed.
            let mode = render_mode_override.unwrap_or(match viewport.shade_plot_mode {
                1 => document::entities::ViewportRenderMode::Wireframe2D,
                2 => document::entities::ViewportRenderMode::HiddenLine,
                3 => document::entities::ViewportRenderMode::GouraudShaded,
                _ => viewport.render_mode,
            });
            let mut wires =
                self.viewport_content_wires(paper_block, Some(handle), None, true, Some(mode));
            apply_mode(&mut wires, mode);
            model_wires.extend(wires);
        }
        let paper =
            crate::scene::model::instance_model::expand_stubs(&self.paper_sheet_wires_arc())
                .into_owned();
        (paper, model_wires)
    }

    pub(crate) fn plot_wire_depths(&self, wires: &[WireModel]) -> Vec<f32> {
        let depths = self.draw_depth_map();
        wires
            .iter()
            .map(|wire| pipeline::wire_gpu::wire_draw_depth(wire, depths.as_ref()))
            .collect()
    }

    /// Invalidate only the draw-order depth cache without dropping geometry tessellation.
    pub fn invalidate_draw_depth(&self) {
        *self.caches.draw_depth_cache.borrow_mut() = None;
    }

    /// Per-entity stable draw-order depth, keyed by entity handle value.
    /// A full build assigns sparse labels in effective draw order. Incremental
    /// Add/Remove then changes only the named handle: existing siblings retain
    /// their depth, avoiding an O(all entities) map rewrite and GPU const upload.
    /// See [`Self::draw_depth_generation`].
    pub(in crate::scene) fn draw_depth_generation(&self) -> u64 {
        self.draw_depth_generation.get()
    }

    #[doc(hidden)]
    pub fn draw_depth_map(&self) -> Arc<HashMap<u64, [f32; 2]>> {
        {
            let cache = self.caches.draw_depth_cache.borrow();
            if let Some(cache) = cache.as_ref() {
                if cache.epoch == self.geometry_epoch {
                    return Arc::clone(&cache.depths);
                }
            }
        }
        let perf = crate::perf::enabled();
        let t_depth = iced::time::Instant::now();

        // Replay Add/Remove into the retained sparse order directory. Removed
        // entries stay in `blocks`/`owners` as tombstones, so Undo/Redo restores
        // their exact prior label without shifting a sibling.
        let stale_cache = self.caches.draw_depth_cache.borrow_mut().take();
        if let Some(mut cache) = stale_cache {
            if let Some(deltas) = self.replay_since(cache.epoch) {
                if deltas
                    .iter()
                    .all(|(_, kind)| matches!(kind, ChangeKind::Modified))
                {
                    cache.epoch = self.geometry_epoch;
                    let arc = Arc::clone(&cache.depths);
                    *self.caches.draw_depth_cache.borrow_mut() = Some(cache);
                    return arc;
                }

                let depths = Arc::make_mut(&mut cache.depths);
                let ms = self.model_space_block_handle();
                let mut incremental_ok = true;
                for &(handle, kind) in &deltas {
                    match kind {
                        ChangeKind::Modified => {}
                        ChangeKind::Removed => {
                            depths.remove(&handle.value());
                        }
                        ChangeKind::Added => {
                            let Some(entity) = self.document.get_entity(handle) else {
                                continue;
                            };
                            if matches!(
                                entity,
                                EntityType::Solid3D(_)
                                    | EntityType::Region(_)
                                    | EntityType::Body(_)
                                    | EntityType::Surface(_)
                                    | EntityType::Mesh(_)
                                    | EntityType::PolygonMesh(_)
                                    | EntityType::PolyfaceMesh(_)
                            ) {
                                continue;
                            }
                            let common = entity.common();
                            let block = if common.owner_handle.is_null() {
                                ms
                            } else {
                                common.owner_handle
                            };
                            let value = handle.value();
                            if let Some(&known_block) = cache.owners.get(&value) {
                                if known_block != block {
                                    incremental_ok = false;
                                    break;
                                }
                                let restored = cache
                                    .blocks
                                    .get(&block)
                                    .and_then(|order| draw_depth_entry(order, value));
                                let Some(entry) = restored else {
                                    incremental_ok = false;
                                    break;
                                };
                                depths
                                    .insert(value, draw_depth_value(entry.label, entry.half_label));
                                continue;
                            }
                            // A newly allocated handle cannot already have a
                            // SortEntitiesTable override; its effective key is
                            // therefore its own handle.
                            let order = cache.blocks.entry(block).or_default();
                            let position = order.partition_point(|entry| {
                                (entry.effective, entry.handle) <= (value, value)
                            });
                            let Some((label, half_label)) =
                                inserted_draw_depth_label(order, position)
                            else {
                                // Pathological middle insertion exhausted the
                                // reserved label gap. A full rebuild is the safe
                                // rare fallback (ordinary monotonic adds use the
                                // two-million-slot tail reserve).
                                incremental_ok = false;
                                break;
                            };
                            order.insert(
                                position,
                                DrawDepthEntry {
                                    handle: value,
                                    effective: value,
                                    label,
                                    half_label,
                                },
                            );
                            cache.owners.insert(value, block);
                            depths.insert(value, draw_depth_value(label, half_label));
                        }
                    }
                }
                if incremental_ok {
                    cache.epoch = self.geometry_epoch;
                    let arc = Arc::clone(&cache.depths);
                    *self.caches.draw_depth_cache.borrow_mut() = Some(cache);
                    if perf {
                        crate::perf_record!(
                            "[perf] draw-depth-patch {:>7.1}ms entries={} changes={}",
                            t_depth.elapsed().as_secs_f64() * 1000.0,
                            arc.len(),
                            deltas.len(),
                        );
                    }
                    return arc;
                }
            }
        }

        // From here the labels are assigned afresh, so an existing entity's
        // depth can move. Anything holding a baked depth has to rebuild.
        self.draw_depth_generation
            .set(self.draw_depth_generation.get().wrapping_add(1));

        use document::objects::ObjectType;
        // Per-block SortEntitiesTable overrides: block -> (entity_val -> sort_val).
        let mut overrides: HashMap<Handle, HashMap<u64, u64>> = HashMap::default();
        for obj in self.document.objects.values() {
            if let ObjectType::SortEntitiesTable(t) = obj {
                if !t.is_empty() {
                    overrides.insert(
                        t.block_owner_handle,
                        t.entries()
                            .map(|e| (e.entity_handle.value(), e.sort_handle.value()))
                            .collect(),
                    );
                }
            }
        }
        let ms = self.model_space_block_handle();
        // Group entities by owning block, carrying each entity's effective key.
        let mut by_block: HashMap<Handle, Vec<(u64, u64)>> = HashMap::default();
        for e in self.document.entities() {
            let c = e.common();
            // 3D meshes keep real geometric depth — exclude them from
            // draw-order biasing so 3D occlusion is never flattened.
            if matches!(
                e,
                EntityType::Solid3D(_)
                    | EntityType::Region(_)
                    | EntityType::Body(_)
                    | EntityType::Surface(_)
                    | EntityType::Mesh(_)
                    | EntityType::PolygonMesh(_)
                    | EntityType::PolyfaceMesh(_)
            ) {
                continue;
            }
            let block = if c.owner_handle.is_null() {
                ms
            } else {
                c.owner_handle
            };
            let hv = c.handle.value();
            let eff = overrides
                .get(&block)
                .and_then(|m| m.get(&hv))
                .copied()
                .unwrap_or(hv);
            by_block.entry(block).or_default().push((hv, eff));
        }
        let total_entries: usize = by_block.values().map(Vec::len).sum();
        let mut depth_map: HashMap<u64, [f32; 2]> =
            HashMap::with_capacity_and_hasher(total_entries, Default::default());
        let mut owners: HashMap<u64, Handle> =
            HashMap::with_capacity_and_hasher(total_entries, Default::default());
        let mut blocks: HashMap<Handle, Vec<DrawDepthEntry>> =
            HashMap::with_capacity_and_hasher(by_block.len(), Default::default());
        for (block, mut order) in by_block {
            order.sort_unstable_by_key(|(handle, effective)| (*effective, *handle));
            let entries = seed_draw_depth_entries(order);
            for entry in &entries {
                depth_map.insert(
                    entry.handle,
                    draw_depth_value(entry.label, entry.half_label),
                );
                owners.insert(entry.handle, block);
            }
            blocks.insert(block, entries);
        }
        let arc = Arc::new(depth_map);
        *self.caches.draw_depth_cache.borrow_mut() = Some(DrawDepthCache {
            epoch: self.geometry_epoch,
            depths: Arc::clone(&arc),
            blocks,
            owners,
        });
        if perf {
            crate::perf_record!(
                "[perf] draw-depth     {:>7.1}ms entries={}",
                t_depth.elapsed().as_secs_f64() * 1000.0,
                arc.len(),
            );
        }
        arc
    }

    #[cfg(test)]
    pub(crate) fn hatch_models_arc(&self) -> Arc<Vec<HatchModel>> {
        self.hatch_models_arc_for_view(true)
    }

    /// Whether `handle` is an INSERT that can place a hatch fill, because a
    /// block it references, directly or nested, holds one. An INSERT of plain
    /// geometry can move without touching any hatch cache. `memo` is the
    /// per-block answer shared across the handles of one validity check.
    pub(in crate::scene) fn insert_may_place_hatches(
        &self,
        handle: Handle,
        memo: &RefCell<std::collections::HashMap<String, bool>>,
    ) -> bool {
        let Some(entity @ EntityType::Insert(_)) = self.document.get_entity(handle) else {
            return false;
        };
        render_graph::entity_render_block_uses(&self.document, entity, self.annotation_scale)
            .into_iter()
            .filter(|block_use| block_use.active)
            .any(|block_use| {
                render_graph::block_contains_hatch(
                    &self.document,
                    &block_use.insert.block_name,
                    &mut memo.borrow_mut(),
                )
            })
    }

    /// Bring a stale resident hatch list up to date by redoing only the
    /// INSERTs that changed since it was built. `None` whenever that is not
    /// provably equivalent to a full rebuild (no replayable journal, a direct
    /// fill or an unknown reference changed), and the caller rebuilds.
    pub(in crate::scene) fn patch_hatch_models(
        &self,
        key: &(Handle, String),
        target_block: Handle,
        scale: Option<Handle>,
        sel_sig: u64,
    ) -> Option<Arc<Vec<HatchModel>>> {
        let started = crate::perf::enabled().then(iced::time::Instant::now);
        let (epoch, cached) = {
            let cache = self.caches.hatch_cache.borrow();
            let (epoch, cached_sel, arc) = cache.get(key)?;
            if *cached_sel != sel_sig {
                return None;
            }
            (*epoch, Arc::clone(arc))
        };
        let hosts = {
            let held = self.caches.hatch_hosts.borrow();
            let (weak, hosts) = held.get(key)?;
            if !weak
                .upgrade()
                .is_some_and(|live| Arc::ptr_eq(&live, &cached))
                || hosts.len() != cached.len()
            {
                return None;
            }
            Arc::clone(hosts)
        };
        let changes = self.replay_since(epoch)?;
        let known_hosts: rustc_hash::FxHashSet<Handle> = hosts
            .iter()
            .copied()
            .filter(|host| !host.is_null())
            .collect();
        let memo = RefCell::new(std::collections::HashMap::new());
        let mut redo: rustc_hash::FxHashSet<Handle> = rustc_hash::FxHashSet::default();
        let mut rebuilt_inserts: rustc_hash::FxHashSet<Handle> = rustc_hash::FxHashSet::default();
        let mut rebuilt_direct = Vec::new();
        for (handle, _) in &changes {
            if self.hatches.contains_key(handle) {
                // Direct hatches can be updated by handle. A hatch inside a
                // block can affect many placements and still needs the full
                // path.
                let entity = self.document.get_entity(*handle)?;
                if !self.belongs_to_visible_block(
                    *handle,
                    entity.common().owner_handle,
                    target_block,
                ) {
                    return None;
                }
                redo.insert(*handle);
                rebuilt_direct.push(*handle);
                continue;
            }
            match self.document.get_entity(*handle) {
                // Gone: harmless when it drew fills as a reference, unknown
                // otherwise (it may have been a direct fill).
                None => {
                    if !known_hosts.contains(handle) {
                        return None;
                    }
                    redo.insert(*handle);
                }
                Some(EntityType::Insert(_)) => {
                    redo.insert(*handle);
                    rebuilt_inserts.insert(*handle);
                }
                // Another kind of reference (a dimension's arrowhead block,
                // say) is not rebuilt per host here.
                Some(entity) => {
                    if known_hosts.contains(handle)
                        || render_graph::entity_render_block_uses(
                            &self.document,
                            entity,
                            self.annotation_scale,
                        )
                        .into_iter()
                        .filter(|block_use| block_use.active)
                        .any(|block_use| {
                            render_graph::block_contains_hatch(
                                &self.document,
                                &block_use.insert.block_name,
                                &mut memo.borrow_mut(),
                            )
                        })
                    {
                        return None;
                    }
                }
            }
        }
        let fresh = if rebuilt_inserts.is_empty() {
            Vec::new()
        } else {
            self.hatch_models_for_hosts(
                target_block,
                &rebuilt_inserts,
                scale,
                self.annotation_all_visible(),
            )
        };
        let mut models = Vec::with_capacity(cached.len());
        let mut new_hosts = Vec::with_capacity(cached.len());
        for (model, host) in cached.iter().zip(hosts.iter()) {
            if redo.contains(host) {
                continue;
            }
            models.push(model.clone());
            new_hosts.push(*host);
        }
        if !rebuilt_direct.is_empty() {
            let all_visible = self.annotation_all_visible();
            let bg = if self.current_layout == "Model" {
                self.bg_color
            } else {
                self.paper_bg_color
            };
            let depths = self.draw_depth_map();
            for handle in rebuilt_direct {
                if !self.hatch_fill_shown(handle, target_block, None, scale, all_visible) {
                    continue;
                }
                let (backdrop, fill) = self.synced_hatch_model(
                    handle,
                    &self.hatches[&handle],
                    scale,
                    None,
                    bg,
                    &depths,
                    false,
                );
                for model in backdrop.into_iter().chain(std::iter::once(fill)) {
                    models.push(model);
                    new_hosts.push(handle);
                }
            }
        }
        let fresh_count = fresh.len();
        for (host, model) in fresh {
            new_hosts.push(host);
            models.push(model);
        }
        let arc = Arc::new(models);
        self.caches.hatch_cache.borrow_mut().insert(
            key.clone(),
            (self.geometry_epoch, sel_sig, Arc::clone(&arc)),
        );
        self.caches.hatch_hosts
            .borrow_mut()
            .insert(key.clone(), (Arc::downgrade(&arc), Arc::new(new_hosts)));
        if let Some(started) = started {
            crate::perf_record!(
                "[perf] hatch-models patch={:.1}ms hosts={} fresh={} models={}",
                started.elapsed().as_secs_f64() * 1000.0,
                redo.len(),
                fresh_count,
                arc.len(),
            );
        }
        Some(arc)
    }

    pub(in crate::scene) fn hatch_models_arc_for_view(&self, requested_tint_selected: bool) -> Arc<Vec<HatchModel>> {
        if self.constraint_shading_active() {
            let _ = self.block_constraint_report(self.block_edit_block.unwrap());
        }
        // Model-space selection is rendered as a tiny overlay. Keep the
        // resident model set stable across select/deselect so its expensive
        // tessellation/style sync and GPU buffers remain warm.
        let tint_selected = requested_tint_selected && self.current_layout != "Model";
        // Hatch models bake the selection tint (issue #71), so they depend on
        // the selected set in paper layouts, but NOT on hover. Keying on
        // `selection_generation` (which also bumps on every hover) made each
        // hover-over a new entity rebuild every hatch model.
        let sel_sig = if tint_selected {
            self.selected_hatch_sig()
        } else {
            0
        };
        let target_block = self.content_render_block_handle();
        let hatch_memo = RefCell::new(std::collections::HashMap::new());
        let key = (
            target_block,
            if self.constraint_shading_active() {
                format!("{}:constraint-status", self.current_layout)
            } else {
                self.current_layout.clone()
            },
        );
        {
            let reuse = {
                let cache = self.caches.hatch_cache.borrow();
                match cache.get(&key) {
                    // Selection tint is baked in, so the selected set must also
                    // match; category = a changed direct fill or an INSERT
                    // whose expanded block tree can contain hatch fills.
                    Some((cached_epoch, cached_sel, arc))
                        if *cached_sel == sel_sig
                            && self.category_cache_valid(
                                *cached_epoch,
                                CACHE_CATEGORY_HATCH | CACHE_CATEGORY_INSERT_HATCH,
                                |h| {
                                    self.hatches.contains_key(&h)
                                        || self.insert_may_place_hatches(h, &hatch_memo)
                                        // An INSERT that drew fills before
                                        // the edit (repointed to a plain
                                        // block) still changes the list.
                                        || self.caches.hatch_hosts.borrow().get(&key).is_some_and(
                                            |(_, hosts)| hosts.contains(&h),
                                        )
                                },
                            ) =>
                    {
                        Some(Arc::clone(arc))
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                if let Some((e, _, _)) = self.caches.hatch_cache.borrow_mut().get_mut(&key) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        // Out of date, but when the journal reaches back to the cached list
        // and only some INSERTs changed, redo just those references' fills.
        if !tint_selected && !self.constraint_shading_active() {
            if let Some(arc) = self.patch_hatch_models(&key, target_block, scale, sel_sig) {
                return arc;
            }
        }
        let build_started = crate::perf::enabled().then(iced::time::Instant::now);
        let (models, hosts) = self.synced_hatch_models_hosted(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
            tint_selected,
        );
        let arc = Arc::new(models);
        self.caches.hatch_hosts
            .borrow_mut()
            .insert(key.clone(), (Arc::downgrade(&arc), Arc::new(hosts)));
        let build_ms = crate::perf::elapsed_ms(build_started);
        if build_ms >= 1.0 {
            crate::perf_record!(
                "[perf] hatch-models rebuild={build_ms:.1}ms models={} source_hatches={} selected={} tint_selected={tint_selected}",
                arc.len(),
                self.hatches.len(),
                self.selected.len(),
            );
        }
        self.caches.hatch_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, sel_sig, Arc::clone(&arc)));
        arc
    }

    /// Order-independent signature of the selected set. Cheap (the set is
    /// normally a handful of entities) and unchanged by hover, so caches that
    /// only depend on what's *selected* don't thrash on rollover.
    pub(in crate::scene) fn selected_set_sig(&self) -> u64 {
        let mut sig: u64 = self.selected.len() as u64;
        for h in self.selected.iter() {
            sig ^= h.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        sig
    }

    pub(in crate::scene) fn selected_hatch_sig(&self) -> u64 {
        let mut sig = 0u64;
        let mut count = 0u64;
        for &h in &self.selected {
            if !self.hatches.contains_key(&h)
                && !matches!(self.document.get_entity(h), Some(EntityType::Insert(_)))
            {
                continue;
            }
            count += 1;
            sig ^= h.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        sig ^ count
    }

    /// Whether a wipeout-model set cached at `cached_epoch` is still current.
    ///
    /// The displayed set also contains masks expanded from INSERTs. COPY and
    /// grip edits modify or add INSERT handles without changing the child
    /// WIPEOUT entity, so both categories count, removed INSERTs included via
    /// the recorded removal bits. Moving plain geometry touches neither and
    /// leaves the set exactly as it was.
    ///
    /// Both cache paths ask through here, because they did not: the
    /// per-viewport one compared `geometry_epoch` outright, so a drag of plain
    /// geometry in a paper-space viewport rebuilt every mask in the drawing —
    /// 96 ms a frame, 153 frames of one measured drag, 14.8 s in all — while
    /// the same drag in model space kept them. Sharing the predicate is what
    /// stops the two drifting apart again.
    pub(in crate::scene) fn wipeout_cache_current(&self, cached_epoch: u64, cached_empty: bool) -> bool {
        let any_wipeout = std::cell::OnceCell::<bool>::new();
        let dependencies = self.caches.dependency_index_cache.borrow();
        let mask_hosts = dependencies
            .as_ref()
            .filter(|_| cached_empty && self.block_edit_block.is_none())
            .map(|index| &index.wipeout_hosts);
        self.category_cache_valid(
            cached_epoch,
            CACHE_CATEGORY_WIPEOUT | CACHE_CATEGORY_INSERT_HATCH,
            |h| {
                // An INSERT only matters when a block it places holds a mask;
                // moving plain geometry leaves the set as it was.
                match self.document.get_entity(h) {
                    Some(EntityType::Wipeout(_)) => true,
                    // An empty cache has no old placements to remove. If the
                    // live graph also has no path from this entity to a mask,
                    // this edit cannot add one to the cached view.
                    Some(_) if mask_hosts.is_some() => {
                        mask_hosts.is_some_and(|hosts| hosts.contains(&h))
                    }
                    // Without a single mask in the drawing no INSERT can place
                    // one; skip walking the block tree of every edited INSERT.
                    Some(EntityType::Insert(_))
                        if !*any_wipeout.get_or_init(|| {
                            self.document
                                .entities()
                                .any(|e| matches!(e, EntityType::Wipeout(_)))
                        }) =>
                    {
                        false
                    }
                    // No host list is kept: the INSERT may have placed a mask
                    // before the edit (its block repointed), so any INSERT
                    // edit counts.
                    Some(EntityType::Insert(_)) => true,
                    _ => false,
                }
            },
        )
    }

    pub(crate) fn wipeout_models_arc(&self) -> Arc<Vec<HatchModel>> {
        let target_block = self.content_render_block_handle();
        let key = (target_block, self.current_layout.clone());
        {
            let reuse = {
                let cache = self.caches.wipeout_cache.borrow();
                match cache.get(&key) {
                    Some((cached_epoch, arc))
                        if self.wipeout_cache_current(*cached_epoch, arc.is_empty()) =>
                    {
                        Some(Arc::clone(arc))
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                if let Some((e, _)) = self.caches.wipeout_cache.borrow_mut().get_mut(&key) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        let rebuild_started = crate::perf::enabled().then(iced::time::Instant::now);
        let arc = Arc::new(self.wipeout_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        ));
        if let Some(started) = rebuild_started {
            crate::perf_record!(
                "[perf] wipeout-models rebuild={:.1}ms models={}",
                started.elapsed().as_secs_f64() * 1000.0,
                arc.len(),
            );
        }
        self.caches.wipeout_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    pub(crate) fn images_arc(&self) -> Arc<Vec<ImageModel>> {
        let target_block = self.content_render_block_handle();
        {
            let reuse = {
                let cache = self.caches.image_cache.borrow();
                match cache.get(&target_block) {
                    Some((cached_epoch, arc))
                        if self.category_cache_valid(
                            *cached_epoch,
                            CACHE_CATEGORY_IMAGE,
                            |h| self.is_image_entity(h),
                        ) =>
                    {
                        Some(Arc::clone(arc))
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                // No image changed since — keep it warm, just advance the sync
                // epoch so the next replay window stays short.
                if let Some((e, _)) = self.caches.image_cache.borrow_mut().get_mut(&target_block) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        let arc = Arc::new(self.image_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        ));
        self.caches.image_cache
            .borrow_mut()
            .insert(target_block, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Splice a re-delivered raster's fresh model into every cached gathered
    /// image list. The reseed takes no geometry bump — the entity, its draw
    /// order and every wire are untouched — so the lists stay valid and only
    /// their rasters are refreshed; without this the gather would re-walk the
    /// whole drawing on the UI thread right after the view settled (#3339).
    /// A raster replacement changes pixels, size and the triangles the
    /// raster is drawn with (one with a zoomed-in region has the whole
    /// picture and the region in one texture) — the gathered placement, clip
    /// and draw depth stay valid — so lists that are current are patched in
    /// place, an instanced clone (an image inside a block reference) through
    /// the placement its corners give; lists already stale for other
    /// reasons are evicted so the next gather rebuilds them. The sheet of a
    /// layout keeps a list of its own pictures, patched the same way.
    pub(crate) fn patch_image_caches(&self, prev_epoch: u64, handles: &[Handle]) {
        if handles.is_empty() {
            return;
        }
        let perf = crate::perf::enabled();
        let started = perf.then(iced::time::Instant::now);
        let a = self.patch_image_cache_entries(&self.caches.image_cache, prev_epoch, handles);
        let b = self.patch_image_cache_entries(&self.caches.frozen_image_cache, prev_epoch, handles);
        let c = self.patch_paper_sheet_images(prev_epoch, handles);
        let counts = (a.0 + b.0 + c.0, a.1 + b.1 + c.1, a.2 + b.2 + c.2);
        if perf {
            crate::perf_record!(
                "[perf] image-patch   {:>7.2}ms entries={} lists_patched={} evicted={}",
                started.map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0),
                counts.0,
                counts.1,
                counts.2,
            );
        }
    }

    /// Returns (entries seen, lists patched, lists evicted for a rebuild).
    pub(in crate::scene) fn patch_image_cache_entries<K>(
        &self,
        cache: &RefCell<HashMap<K, (u64, Arc<Vec<ImageModel>>)>>,
        prev_epoch: u64,
        handles: &[Handle],
    ) -> (usize, usize, usize) {
        let epoch = self.geometry_epoch;
        let mut seen = 0usize;
        let mut patched = 0usize;
        let mut stale = 0usize;
        cache.borrow_mut().retain(|_, (entry_epoch, arc)| {
            seen += 1;
            // A list the patch cannot reason about — one that predates other
            // changes, or holds an instanced clone whose placement its
            // corners do not give — is evicted: the reseed records no geometry change, so the
            // journal replay would otherwise revalidate it with the old pixels.
            if *entry_epoch != prev_epoch || self.holds_unplaceable_clone(arc, handles) {
                stale += 1;
                return false;
            }
            if self.splice_fresh_images(arc, handles) {
                *entry_epoch = epoch;
                patched += 1;
            }
            true
        });
        (seen, patched, stale)
    }

    /// The same for the list the sheet of each layout keeps of its own
    /// pictures (`paper_sheet_render_models_for_view`). A sheet whose list
    /// holds none of `handles` is left as it is, whatever its epoch: the
    /// reseed has nothing for it.
    pub(in crate::scene) fn patch_paper_sheet_images(
        &self,
        prev_epoch: u64,
        handles: &[Handle],
    ) -> (usize, usize, usize) {
        let epoch = self.geometry_epoch;
        let mut seen = 0usize;
        let mut patched = 0usize;
        let mut stale = 0usize;
        self.caches.paper_sheet_render_cache
            .borrow_mut()
            .retain(|_, sheet| {
                seen += 1;
                let holds = sheet
                    .images
                    .iter()
                    .any(|model| model.entity.is_some_and(|entity| handles.contains(&entity)));
                if !holds {
                    return true;
                }
                if sheet.epoch != prev_epoch || self.holds_unplaceable_clone(&sheet.images, handles) {
                    stale += 1;
                    return false;
                }
                if self.splice_fresh_images(&mut sheet.images, handles) {
                    sheet.epoch = epoch;
                    patched += 1;
                }
                true
            });
        (seen, patched, stale)
    }

    /// `list` holds one of `handles` as an instanced clone (an image inside
    /// a block reference) whose placement cannot be read back from its
    /// corners (a degenerate quad), so only the gather can place it.
    pub(in crate::scene) fn holds_unplaceable_clone(&self, list: &[ImageModel], handles: &[Handle]) -> bool {
        list.iter().any(|model| {
            model.render_instance.is_some()
                && model.entity.is_some_and(|entity| handles.contains(&entity))
                && model.entity.and_then(|entity| self.images.get(&entity)).is_some_and(
                    |fresh| image_clone_placement(fresh, model).is_none(),
                )
        })
    }

    /// Gives the entries of `list` that serve `handles` the rasters the scene
    /// has for them now, and drops those whose raster did not come back.
    /// Returns whether the list held any.
    pub(in crate::scene) fn splice_fresh_images(&self, list: &mut Arc<Vec<ImageModel>>, handles: &[Handle]) -> bool {
        let mut touched = false;
        Arc::make_mut(list).retain_mut(|model| {
            let Some(entity) = model.entity else {
                return true;
            };
            if !handles.contains(&entity) {
                return true;
            }
            match self.images.get(&entity) {
                Some(fresh) => {
                    if !Arc::ptr_eq(&model.pixels, &fresh.pixels) {
                        model.file_path = fresh.file_path.clone();
                        model.pixels = Arc::clone(&fresh.pixels);
                        model.width = fresh.width;
                        model.height = fresh.height;
                        // The triangles go with the texture: they say
                        // which part of it each one draws. A clone's are
                        // placed as the gather placed its corners.
                        let place = model
                            .render_instance
                            .is_some()
                            .then(|| image_clone_placement(fresh, model))
                            .flatten();
                        model.verts.clone_from(&fresh.verts);
                        if let Some(place) = place {
                            for vertex in &mut model.verts {
                                let point = place(split_sum(vertex.pos, vertex.pos_low));
                                (vertex.pos, vertex.pos_low) = split_f64(point);
                            }
                        }
                    }
                    touched = true;
                    true
                }
                None => {
                    // The raster did not come back; the cached list drops
                    // the entry the rebuild would not gather either.
                    touched = true;
                    false
                }
            }
        });
        touched
    }

    /// Image / OLE-frame models owned by `target_block`, optionally dropping
    /// those whose layer is frozen in a content viewport (`frozen`).
    pub(in crate::scene) fn image_models(
        &self,
        target_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<ImageModel> {
        self.collect_images(
            target_block,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            false,
            |image, _| image,
        )
    }

    pub fn paper_plot_images(&self) -> Vec<crate::scene::project::PlotImage> {
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        self.placed_images(
            self.current_layout_block_handle(),
            None,
            scale,
            self.annotation_all_visible(),
            None,
            true,
        )
    }

    pub(in crate::scene) fn placed_images(
        &self,
        target_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        plotting: bool,
    ) -> Vec<crate::scene::project::PlotImage> {
        self.collect_images(
            target_block,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            plotting,
            |image, context| crate::scene::project::PlotImage {
                image,
                clips: context.clips.clone(),
            },
        )
    }

    pub(in crate::scene) fn collect_images<T>(
        &self,
        target_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        plotting: bool,
        mut collect: impl FnMut(ImageModel, &render_graph::InstanceContext) -> T,
    ) -> Vec<T> {
        if self.images.is_empty() {
            return Vec::new();
        }
        let depth_map = self.draw_depth_map();
        let visibility_reveal = if plotting {
            HashSet::default()
        } else {
            self.visibility_reveal_handles()
        };
        // Plot/export passes draw the drawing as stored, not as walked;
        // live renders get the walk only when they show the interaction space.
        let layer_walk = (!plotting)
            .then(|| self.render_space_layer_walk(target_block, viewport))
            .flatten();
        let graph = render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_visibility_reveal(&visibility_reveal)
        .with_layer_walk(layer_walk)
        .with_annotation_scale(self.annotation_scale)
        .with_viewport(viewport);
        let mut models = Vec::new();
        let mut image_sources = rustc_hash::FxHashMap::default();
        let root = if self.block_edit_block.is_none() {
            viewport.map(|viewport| render_graph::SceneRoot::Viewport {
                paper_block: self.current_layout_block_handle(),
                viewport,
                model_block: target_block,
            })
        } else {
            None
        }
        .unwrap_or_else(|| self.render_scene_root(target_block));
        let mut image_block_memo = std::collections::HashMap::new();
        let is_image = |entity: &EntityType| self.images.contains_key(&entity.common().handle);
        graph.walk_root(
            root,
            |entity, context| {
                if plotting
                    && self
                        .document
                        .layers
                        .get(&entity.common().layer)
                        .is_some_and(|layer| !layer.is_plottable)
                {
                    return false;
                }
                if context.is_instanced() {
                    return true;
                }
                if self.entity_temporarily_hidden(entity.common().handle) {
                    return false;
                }
                // Skip the placements of blocks that hold no picture, as the
                // hatch walk does: they are most of a large drawing.
                let block_uses: Vec<_> =
                    render_graph::entity_render_block_uses(&self.document, entity, 1.0)
                        .into_iter()
                        .filter(|block_use| block_use.active)
                        .collect();
                block_uses.is_empty()
                    || block_uses.into_iter().any(|block_use| {
                        render_graph::block_contains_kind(
                            &self.document,
                            &block_use.insert.block_name,
                            &mut image_block_memo,
                            &is_image,
                        )
                    })
            },
            |entity, context| {
                let handle = entity.common().handle;
                let Some(model) = self.images.get(&handle) else {
                    return;
                };
                // A plot draws a PDF underlay's page at print resolution, not
                // at the resolution it is shown at (#2822).
                let mut placed = plotting
                    .then(|| self.print_underlay_image(entity))
                    .flatten()
                    .unwrap_or_else(|| model.clone());
                // Stamp the real handle: a seed can build the model before
                // the entity's handle is assigned, so the builder cannot.
                placed.entity = Some(handle);
                if context.is_instanced() {
                    for (corner, low) in placed.corners.iter_mut().zip(&mut placed.corners_low) {
                        let point = document::types::Vector3::new(
                            corner[0] as f64 + low[0] as f64,
                            corner[1] as f64 + low[1] as f64,
                            corner[2] as f64 + low[2] as f64,
                        );
                        let point = context.transform.apply(point);
                        *corner = [point.x as f32, point.y as f32, point.z as f32];
                        *low = [
                            (point.x - corner[0] as f64) as f32,
                            (point.y - corner[1] as f64) as f32,
                            (point.z - corner[2] as f64) as f32,
                        ];
                    }
                    for vertex in &mut placed.verts {
                        let point = document::types::Vector3::new(
                            vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                            vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
                            vertex.pos[2] as f64 + vertex.pos_low[2] as f64,
                        );
                        let point = context.transform.apply(point);
                        vertex.pos = [point.x as f32, point.y as f32, point.z as f32];
                        vertex.pos_low = [
                            (point.x - vertex.pos[0] as f64) as f32,
                            (point.y - vertex.pos[1] as f64) as f32,
                            (point.z - vertex.pos[2] as f64) as f32,
                        ];
                    }
                    let matrix = &context.transform.matrix.m;
                    let linear = [
                        matrix[0][0].to_bits(),
                        matrix[0][1].to_bits(),
                        matrix[0][2].to_bits(),
                        matrix[1][0].to_bits(),
                        matrix[1][1].to_bits(),
                        matrix[1][2].to_bits(),
                        matrix[2][0].to_bits(),
                        matrix[2][1].to_bits(),
                        matrix[2][2].to_bits(),
                    ];
                    let source_id = *image_sources
                        .entry((handle.value(), linear))
                        .or_insert_with(crate::scene::model::instance_model::next_source_id);
                    placed.render_instance =
                        Some(crate::scene::model::instance_model::RenderInstance {
                            source_id,
                            translation: [matrix[0][3], matrix[1][3], matrix[2][3]],
                        });
                }
                if !plotting
                    && self.visibility_revealed(if context.is_instanced() {
                        context.root_handle
                    } else {
                        handle
                    })
                {
                    placed.opacity *= 0.3;
                }
                placed.draw_depth = context.draw_depth(handle, depth_map.as_ref());
                models.push(collect(placed, context));
            },
        );
        models
    }

    /// Images owned by the active paper layout block only. The full-canvas
    /// sheet viewport uses this so model-block images don't bleed onto the
    /// paper sheet (mirrors `paper_canvas_hatches`).
    pub(crate) fn paper_sheet_images(&self) -> Arc<Vec<ImageModel>> {
        let layout_block = self.current_layout_block_handle();
        Arc::new(self.image_models(
            layout_block,
            None,
            self.paper_annotation_scale_handle(),
            self.annotation_all_visible(),
            None,
        ))
    }

    /// Bring a stale mesh list up to date by redoing only the INSERTs that
    /// changed since it was built. `None` whenever that is not provably the
    /// same as a full rebuild (no replayable journal, a top-level solid or a
    /// section changed), and the caller rebuilds.
    pub(in crate::scene) fn patch_mesh_models(
        &self,
        key: &(Handle, String),
        target_block: Handle,
        scale: Option<Handle>,
    ) -> Option<Arc<Vec<MeshLodSet>>> {
        if self.constraint_shading_active() || self.active_live_section(target_block).is_some() {
            return None;
        }
        let started = crate::perf::enabled().then(iced::time::Instant::now);
        let (epoch, cached) = {
            let cache = self.caches.mesh_cache.borrow();
            let (epoch, arc) = cache.get(key)?;
            (*epoch, Arc::clone(arc))
        };
        let changes = self.replay_since(epoch)?;
        let mut redo: HashSet<Handle> = HashSet::default();
        let mut inserts: HashSet<Handle> = HashSet::default();
        for (handle, _) in &changes {
            if self.meshes.contains_key(handle) || self.block_meshes.contains_key(handle) {
                return None;
            }
            match self.document.get_entity(*handle) {
                Some(EntityType::Insert(_)) => {
                    redo.insert(*handle);
                    inserts.insert(*handle);
                }
                // Section objects cut the solids they cross.
                Some(EntityType::Extended(_)) => return None,
                // Anything else places no block mesh.
                Some(_) => {}
                None => {
                    // Gone: a top-level solid leaves the list through the full
                    // rebuild; a reference takes its own placements with it.
                    if cached.iter().any(|set| {
                        set.instance_handle.is_none() && set.entity_handle() == Some(*handle)
                    }) {
                        return None;
                    }
                    if cached
                        .iter()
                        .any(|set| set.instance_handle == Some(*handle))
                    {
                        redo.insert(*handle);
                    }
                }
            }
        }
        let fresh = if inserts.is_empty() {
            Vec::new()
        } else {
            self.instanced_block_meshes(
                target_block,
                None,
                scale,
                self.annotation_all_visible(),
                None,
                None,
                Some(&inserts),
            )
        };
        let fresh_count = fresh.len();
        let mut models: Vec<MeshLodSet> = cached
            .iter()
            .filter(|set| !set.instance_handle.is_some_and(|host| redo.contains(&host)))
            .cloned()
            .collect();
        models.extend(fresh);
        let arc = Arc::new(models);
        self.caches.mesh_cache
            .borrow_mut()
            .insert(key.clone(), (self.geometry_epoch, Arc::clone(&arc)));
        if let Some(started) = started {
            crate::perf_record!(
                "[perf] mesh-models patch={:.1}ms hosts={} fresh={} models={}",
                started.elapsed().as_secs_f64() * 1000.0,
                redo.len(),
                fresh_count,
                arc.len(),
            );
        }
        Some(arc)
    }

    pub(crate) fn meshes_arc(&self) -> Arc<Vec<MeshLodSet>> {
        if self.constraint_shading_active() {
            let _ = self.block_constraint_report(self.block_edit_block.unwrap());
        }
        let target_block = self.content_render_block_handle();
        let key = (
            target_block,
            if self.constraint_shading_active() {
                format!("{}:constraint-status", self.current_layout)
            } else {
                self.current_layout.clone()
            },
        );
        {
            let reuse = {
                let mesh_memo = RefCell::new(std::collections::HashMap::new());
                let cache = self.caches.mesh_cache.borrow();
                match cache.get(&key) {
                    // Top-level solids seed self.meshes; the instanced_block part
                    // is driven by INSERTs, so an INSERT edit (e.g. a move) must
                    // also invalidate. Block-definition edits route through
                    // bump_geometry (a full delta) and invalidate regardless.
                    // An INSERT can only place a mesh when a block it uses,
                    // directly or nested, holds one; moving an INSERT of plain
                    // geometry keeps this warm.
                    Some((cached_epoch, arc))
                        if self.category_cache_valid(*cached_epoch, CACHE_CATEGORY_MESH, |h| {
                            self.meshes.contains_key(&h)
                                // An INSERT that placed solids before the
                                // edit (repointed to a plain block).
                                || arc.iter().any(|set| set.instance_handle == Some(h))
                                || match self.document.get_entity(h) {
                                    Some(entity @ EntityType::Insert(_)) => {
                                        !self.block_meshes.is_empty()
                                            && render_graph::entity_render_block_uses(
                                                &self.document,
                                                entity,
                                                self.annotation_scale,
                                            )
                                            .into_iter()
                                            .filter(|block_use| block_use.active)
                                            .any(|block_use| {
                                                render_graph::block_contains_mesh(
                                                    &self.document,
                                                    &block_use.insert.block_name,
                                                    &mut mesh_memo.borrow_mut(),
                                                )
                                            })
                                    }
                                    Some(EntityType::Extended(extended)) => matches!(
                                        extended.data,
                                        document::entities::ExtendedEntityData::SectionObject(_)
                                    ),
                                    _ => false,
                                }
                        }) =>
                    {
                        Some(Arc::clone(arc))
                    }
                    _ => None,
                }
            };
            if let Some(arc) = reuse {
                if let Some((e, _)) = self.caches.mesh_cache.borrow_mut().get_mut(&key) {
                    *e = self.geometry_epoch;
                }
                return arc;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        if let Some(arc) = self.patch_mesh_models(&key, target_block, scale) {
            return arc;
        }
        let rebuild_started = crate::perf::enabled().then(iced::time::Instant::now);
        let mut models = self.mesh_models(
            target_block,
            None,
            scale,
            self.annotation_all_visible(),
            None,
        );
        self.shade_constraint_meshes(target_block, &mut models);
        if let Some(started) = rebuild_started {
            crate::perf_record!(
                "[perf] mesh-models rebuild={:.1}ms models={}",
                started.elapsed().as_secs_f64() * 1000.0,
                models.len(),
            );
        }
        let arc = Arc::new(models);
        self.caches.mesh_cache
            .borrow_mut()
            .insert(key, (self.geometry_epoch, Arc::clone(&arc)));
        arc
    }

    /// Block rendered by model-content viewports. Paper layout entities belong
    /// only to the sheet viewport; floating paper viewports always show model
    /// space. BEDIT is the sole exception and renders the edited block itself.
    pub(in crate::scene) fn content_render_block_handle(&self) -> Handle {
        self.block_edit_block
            .filter(|block| !block.is_null())
            .unwrap_or_else(|| self.model_space_block_handle())
    }

    pub(in crate::scene) fn render_scene_root(&self, block: Handle) -> render_graph::SceneRoot {
        let role = if self.block_edit_block == Some(block) {
            render_graph::BlockRootRole::DefinitionEdit
        } else if block == self.model_space_block_handle() {
            render_graph::BlockRootRole::ModelSpace
        } else {
            render_graph::BlockRootRole::PaperSpace
        };
        render_graph::SceneRoot::Block(render_graph::BlockRoot {
            record: block,
            role,
        })
    }

    pub(crate) fn interaction_block_handle(&self) -> Handle {
        if self.block_edit_block.is_none() && self.active_viewport.is_some() {
            self.model_space_block_handle()
        } else {
            self.current_layout_block_handle()
        }
    }

    pub(in crate::scene) fn interaction_viewport_frozen_layers(&self) -> Option<&[Handle]> {
        if self.block_edit_block.is_some() {
            return None;
        }
        let handle = self.active_viewport?;
        match self.document.get_entity(handle) {
            Some(EntityType::Viewport(viewport)) => Some(&viewport.frozen_layers),
            _ => None,
        }
    }

    pub(in crate::scene) fn interaction_space_key(&self) -> u64 {
        let base = self.interaction_space_base_key();
        // Fold the session LAYWALK set in like the viewport freeze fold:
        // render caches keyed here must never reuse an un-walked build.
        let walk = Self::layer_walk_sig(self.layer_walk.visible.get(&base));
        base ^ walk.rotate_left(11)
    }

    /// The space identity WITHOUT any walk: block ^ viewport ^ per-viewport
    /// freeze. `LayerWalkState` entries are keyed by this value, so a walk
    /// can be set or cleared without ever moving the key it lives under.
    /// During block edit the viewport is ignored, so a walk stored under this
    /// key applies to the edited block space — matching the other gates.
    pub(in crate::scene) fn interaction_space_base_key(&self) -> u64 {
        let block = self.interaction_block_handle().value();
        let viewport = self
            .active_viewport
            .filter(|_| self.block_edit_block.is_none())
            .map_or(0, |handle| handle.value());
        let frozen = self
            .interaction_viewport_frozen_layers()
            .map_or(0, |layers| {
                layers
                    .iter()
                    .fold(layers.len() as u64, |signature, handle| {
                        signature ^ handle.value().wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    })
            });
        block ^ viewport.rotate_left(29) ^ frozen.rotate_left(47)
    }

    /// Order-independent signature of an optional walk set, folded like the
    /// frozen-set folds so iteration order cannot change a cache key. `None`
    /// and the empty set differ: a hide-everything walk never aliases the
    /// un-walked key.
    pub(in crate::scene) fn layer_walk_sig(walk: Option<&HashSet<Handle>>) -> u64 {
        let Some(visible) = walk else {
            return 0;
        };
        visible
            .iter()
            .fold(visible.len() as u64 + 1, |signature, handle| {
                signature ^ handle.value().wrapping_mul(0x9E37_79B9_7F4A_7C15)
            })
    }

    /// The walk set governing the current interaction space, if any. An
    /// empty set is a valid walk (it hides every layered entity).
    pub(in crate::scene) fn interaction_layer_walk(&self) -> Option<&HashSet<Handle>> {
        // Asked once per entity by every visibility gate: without any walk,
        // skip the space key, whose layout lookup scans every object.
        if self.layer_walk.visible.is_empty() {
            return None;
        }
        self.layer_walk
            .visible
            .get(&self.interaction_space_base_key())
    }

    /// Walk set for a render of `block` seen through `viewport` (`None` =
    /// the sheet / tiles directly). Only the current interaction space
    /// carries a walk: a graph that serves a foreign viewport or an export
    /// pass must keep drawing the un-walked content.
    pub(crate) fn render_space_layer_walk(
        &self,
        block: Handle,
        viewport: Option<Handle>,
    ) -> Option<&HashSet<Handle>> {
        let walk = self.interaction_layer_walk()?;
        let interaction_viewport = self
            .active_viewport
            .filter(|_| self.block_edit_block.is_none());
        if viewport.filter(|handle| !handle.is_null()) != interaction_viewport {
            return None;
        }
        (block == self.interaction_block_handle()).then_some(walk)
    }

    /// Public handle on the key the LAYWALK state is stored under (see
    /// `interaction_space_base_key`): stable with and without an active walk,
    /// so the app can set / clear / compare space identities.
    pub fn interaction_space_key_pub(&self) -> u64 {
        self.interaction_space_base_key()
    }

    /// The visible-layer set walked in space `space`, if that space is walked.
    pub fn space_layer_walk(&self, space: u64) -> Option<&HashSet<Handle>> {
        self.layer_walk.visible.get(&space)
    }

    /// Install or remove (`None`) the walk set for space `space`. Pure state:
    /// the caller bumps the geometry epoch so every derived cache rebuilds.
    pub fn set_space_layer_walk(&mut self, space: u64, visible: Option<HashSet<Handle>>) {
        match visible {
            Some(visible) => {
                self.layer_walk.visible.insert(space, visible);
            }
            None => {
                self.layer_walk.visible.remove(&space);
            }
        }
    }

    /// Drop walks for every space but `keep` — the app guarantees only the
    /// interactively walked space is ever hidden.
    pub fn clear_other_space_layer_walks(&mut self, keep: u64) {
        self.layer_walk.visible.retain(|space, _| *space == keep);
    }

    /// Drop every walk set. The pallet's cleanup path calls this on every
    /// close, so a drawing can never be left half-hidden.
    pub fn clear_all_layer_walks(&mut self) {
        self.layer_walk.visible.clear();
    }

    /// The entity handles of the block record being interacted with: model
    /// space, or the current layout's paper-space block. Per-layer actions
    /// (LAYWALK's zoom and select) work on this set, never on block-definition
    /// geometry.
    pub fn interaction_space_handles(&self) -> Vec<Handle> {
        let block = self.interaction_block_handle();
        self.document
            .block_records
            .iter()
            .find(|record| record.handle == block)
            .map(|record| record.entity_handles.clone())
            .unwrap_or_default()
    }

    /// Whether any space currently has a walk set.
    pub fn layer_walk_active(&self) -> bool {
        !self.layer_walk.visible.is_empty()
    }

    /// Whether the current interaction space is walked.
    pub fn layer_walk_active_for_current_space(&self) -> bool {
        self.layer_walk
            .visible
            .contains_key(&self.interaction_space_base_key())
    }

    /// Convenience clone of the current space's walk set for snapshot/undo-free restores.
    pub fn current_space_walk_visible(&self) -> Option<HashSet<Handle>> {
        self.space_layer_walk(self.interaction_space_base_key())
            .cloned()
    }

    /// True when the current space's walk hides this layer — the predicate
    /// every visibility gate ANDs in next to its off/frozen test. No set,
    /// or a layer outside any space (unknown record), never hides.
    pub fn layer_walk_hidden(&self, layer: Option<&document::tables::layer::Layer>) -> bool {
        let Some(layer) = layer else {
            return false;
        };
        self.interaction_layer_walk()
            .is_some_and(|walk| !walk.contains(&layer.handle))
    }

    pub(in crate::scene) fn interaction_layer_frozen(&self, layer: &str) -> bool {
        // The interaction-space LAYWALK hides layers like a VP freeze would,
        // so every per-viewport-freeze consumer honours it through this gate.
        if self.layer_walk_hidden(self.document.layers.get(layer)) {
            return true;
        }
        let Some(frozen) = self.interaction_viewport_frozen_layers() else {
            return false;
        };
        self.document
            .layers
            .get(layer)
            .is_some_and(|record| frozen.contains(&record.handle))
    }

    pub(in crate::scene) fn interaction_meshes_arc(&self) -> Arc<Vec<MeshLodSet>> {
        let block = self.interaction_block_handle();
        if self.block_edit_block.is_none() && block == self.model_space_block_handle() {
            let Some(frozen) = self.interaction_viewport_frozen_layers() else {
                return self.meshes_arc();
            };
            if frozen.is_empty() {
                return self.meshes_arc();
            }
            let key = self.interaction_space_key();
            if let Some((epoch, cached_key, meshes)) = self.caches.interaction_mesh_cache.borrow().as_ref()
            {
                if *epoch == self.geometry_epoch && *cached_key == key {
                    return Arc::clone(meshes);
                }
            }
            let frozen: HashSet<Handle> = frozen.iter().copied().collect();
            let viewport = self.active_viewport.unwrap_or(Handle::NULL);
            let meshes = self.meshes_for_viewport(viewport, &frozen);
            *self.caches.interaction_mesh_cache.borrow_mut() =
                Some((self.geometry_epoch, key, Arc::clone(&meshes)));
            return meshes;
        }
        let key = self.interaction_space_key();
        if let Some((epoch, cached_key, meshes)) = self.caches.interaction_mesh_cache.borrow().as_ref() {
            if *epoch == self.geometry_epoch && *cached_key == key {
                return Arc::clone(meshes);
            }
        }
        let meshes = Arc::new(self.mesh_models(
            block,
            None,
            self.displayed_annotation_scale_handle(),
            self.annotation_all_visible(),
            self.active_viewport,
        ));
        *self.caches.interaction_mesh_cache.borrow_mut() =
            Some((self.geometry_epoch, key, Arc::clone(&meshes)));
        meshes
    }

    pub(in crate::scene) fn mesh_pick_lookup(&self, meshes: &Arc<Vec<MeshLodSet>>) -> Arc<HashMap<Handle, Vec<u32>>> {
        let source = Arc::as_ptr(meshes) as usize;
        {
            let cache = self.caches.mesh_pick_lookup_cache.borrow();
            if let Some((ptr, weak, lookup)) = cache.as_ref() {
                if *ptr == source
                    && weak
                        .upgrade()
                        .is_some_and(|cached| Arc::ptr_eq(&cached, meshes))
                {
                    return Arc::clone(lookup);
                }
            }
        }
        let mut lookup: HashMap<Handle, Vec<u32>> = HashMap::default();
        for (index, set) in meshes.iter().enumerate() {
            let Some(handle) = set.entity_handle() else {
                continue;
            };
            lookup.entry(handle).or_default().push(index as u32);
        }
        let lookup = Arc::new(lookup);
        *self.caches.mesh_pick_lookup_cache.borrow_mut() =
            Some((source, Arc::downgrade(meshes), Arc::clone(&lookup)));
        lookup
    }

    /// The box around the vertices of each mesh `meshes` draws, in the mesh's
    /// own coordinates, by the mesh's address. A mesh with no vertices, or
    /// with one that is not finite, has none. Measured once for a mesh list:
    /// a selection box asked every solid of the drawing for its vertices on
    /// every pointer move.
    pub(in crate::scene) fn mesh_pick_boxes(
        &self,
        meshes: &Arc<Vec<MeshLodSet>>,
    ) -> Arc<rustc_hash::FxHashMap<usize, [glam::DVec3; 2]>> {
        let source = Arc::as_ptr(meshes) as usize;
        {
            let cache = self.caches.mesh_pick_boxes_cache.borrow();
            if let Some((ptr, weak, boxes)) = cache.as_ref() {
                if *ptr == source
                    && weak
                        .upgrade()
                        .is_some_and(|cached| Arc::ptr_eq(&cached, meshes))
                {
                    return Arc::clone(boxes);
                }
            }
        }
        let mut boxes = rustc_hash::FxHashMap::default();
        let mut measured = rustc_hash::FxHashSet::default();
        for set in meshes.iter() {
            let Some(mesh) = set.geometry_lods().first() else {
                continue;
            };
            let address = std::ptr::from_ref(mesh) as usize;
            if measured.insert(address) {
                if let Some(local) = pick::hit_test::mesh_local_box(mesh) {
                    boxes.insert(address, local);
                }
            }
        }
        let boxes = Arc::new(boxes);
        *self.caches.mesh_pick_boxes_cache.borrow_mut() =
            Some((source, Arc::downgrade(meshes), Arc::clone(&boxes)));
        boxes
    }

    /// Solid-mesh set (top-level + block-instanced), optionally dropping those
    /// whose layer is frozen in a content viewport (`frozen`). `None` reproduces
    /// the full set.
    pub(in crate::scene) fn mesh_models(
        &self,
        target_block: Handle,
        frozen: Option<&HashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<MeshLodSet> {
        let live_section = self.active_live_section(target_block);
        // Top-level solids: drop those whose layer is off/frozen or that are
        // flagged invisible / isolated-hidden, mirroring the 2D wire path, plus
        // any whose layer is frozen in the requesting viewport.
        let mut all: Vec<MeshLodSet> = self
            .meshes
            .iter()
            .filter(|(&h, _)| {
                self.mesh_entity_visible(h)
                    && self
                        .document
                        .get_entity(h)
                        .map(|e| {
                            !self.layer_frozen_in(&e.common().layer, frozen)
                                && self.belongs_to_visible_block(
                                    h,
                                    e.common().owner_handle,
                                    target_block,
                                )
                        })
                        .unwrap_or(false)
            })
            .filter_map(|(&handle, set)| {
                let mut set = if let Some(section) = live_section.as_ref() {
                    match self.sectioned_body(handle, section) {
                        Ok(Some(body)) => self
                            .prepare_solid_model_display(handle, &body)
                            .map(|display| display.0)
                            .unwrap_or_else(|| set.clone()),
                        Ok(None) => return None,
                        Err(()) => set.clone(),
                    }
                } else {
                    set.clone()
                };
                if let Some(entity) = self.document.get_entity(handle) {
                    let style = crate::scene::view::render::render_style_for_viewport(
                        &self.document,
                        entity,
                        viewport,
                    );
                    // Colour 7 lines contrast with what the solid is drawn
                    // on; shaded faces keep the colour so they still read as
                    // lit. White edges on a white sheet hid a wireframe solid
                    // in a layout viewport. (#1446)
                    let edge = crate::scene::view::render::adapt_to_bg(style.0, self.current_bg());
                    set.edge_color = Some(edge);
                    let attached_material = set
                        .material
                        .as_ref()
                        .is_some_and(|material| material.handle.is_some());
                    for mesh in &mut set.lods {
                        if !attached_material {
                            mesh.color[0..3].copy_from_slice(&style.0[0..3]);
                        }
                        mesh.color[3] = style.0[3];
                    }
                    if let Some(material) = set.material.as_mut() {
                        if !attached_material {
                            material.diffuse[0..3].copy_from_slice(&style.0[0..3]);
                        }
                        material.diffuse[3] = style.0[3];
                    }
                }
                if self.visibility_revealed(handle) {
                    for mesh in &mut set.lods {
                        mesh.color = crate::scene::cache::block_cache::fade_toward_bg(
                            mesh.color,
                            self.current_bg(),
                        );
                    }
                    if let Some(material) = set.material.as_mut() {
                        material.diffuse = crate::scene::cache::block_cache::fade_toward_bg(
                            material.diffuse,
                            self.current_bg(),
                        );
                    }
                    // The edges carry their own colour now; fade them too.
                    set.edge_color = set.edge_color.map(|color| {
                        crate::scene::cache::block_cache::fade_toward_bg(color, self.current_bg())
                    });
                }
                Some(set)
            })
            .collect();
        // Block-definition solids are instanced per INSERT of the ACTIVE space's
        // block so a block placed at an INSERT scale renders at the right size
        // (#123) — model space normally, the edited block in a BEDIT editor so
        // model-space solids don't leak into it (#261).
        all.extend(self.instanced_block_meshes(
            target_block,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            live_section.as_ref(),
            None,
        ));
        all
    }

    /// The planes a live section keeps the positive side of, in model space,
    /// as `[nx, ny, nz, d]` with `n·p + d >= 0` kept: what point clouds are
    /// clipped by, as solids are cut.
    pub(crate) fn live_section_clip_planes(&self) -> Vec<[f64; 4]> {
        let Some(section) = self.active_live_section(self.model_space_block_handle()) else {
            return Vec::new();
        };
        Self::section_planes(&section.data, section.slice_depth, None)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|plane| {
                let normal = glam::DVec3::from(plane.normal()?).normalize_or_zero();
                (normal != glam::DVec3::ZERO).then(|| {
                    let d = -normal.dot(glam::DVec3::from(plane.origin));
                    [normal.x, normal.y, normal.z, d]
                })
            })
            .collect()
    }

    pub(in crate::scene) fn active_live_section(&self, target_block: Handle) -> Option<LiveSection> {
        if !self.has_section_objects() {
            return None;
        }
        self.document
            .entities()
            .filter_map(|entity| {
                let EntityType::Extended(extended) = entity else {
                    return None;
                };
                let document::entities::ExtendedEntityData::SectionObject(data) = &extended.data
                else {
                    return None;
                };
                let owner = extended.common.owner_handle;
                (data.flags & 1 != 0
                    && !extended.common.invisible
                    && (owner.is_null() || owner == target_block))
                    .then(|| LiveSection {
                        data: data.clone(),
                        slice_depth: crate::entities::extended::section_is_slice(extended).then(
                            || {
                                crate::entities::extended::section_slice_depth(extended)
                                    .unwrap_or(0.0)
                            },
                        ),
                    })
            })
            .last()
    }

    pub(in crate::scene) fn sectioned_body(
        &self,
        handle: Handle,
        section: &LiveSection,
    ) -> Result<Option<ocs_solid::model::Brep>, ()> {
        let body = self.section_source_body(handle).ok_or(())?;
        Self::section_body(&body, &section.data, section.slice_depth, None)
    }

    /// Build a temporary entity whose wire data matches the live-section body.
    /// The document entity stays unchanged so disabling the section restores
    /// the complete source geometry and saving never persists the clipped body.
    pub(in crate::scene) fn sectioned_wire_entity(
        &self,
        handle: Handle,
        section: &LiveSection,
    ) -> Result<Option<EntityType>, ()> {
        let Some(body) = self.sectioned_body(handle, section)? else {
            return Ok(None);
        };
        let (_, wires, center) = self.prepare_solid_model_display(handle, &body).ok_or(())?;
        let mut entity = self.document.get_entity(handle).cloned().ok_or(())?;
        let reference = document::types::Vector3::new(center[0], center[1], center[2]);
        match &mut entity {
            EntityType::Solid3D(solid) => {
                solid.point_of_reference = reference;
                solid.wires = wires;
                solid.silhouettes.clear();
            }
            EntityType::Surface(surface) => {
                surface.point_of_reference = reference;
                surface.wires = wires;
                surface.silhouettes.clear();
            }
            EntityType::Region(region) => {
                region.point_of_reference = reference;
                region.wires = wires;
                region.silhouettes.clear();
            }
            EntityType::Body(body) => {
                body.point_of_reference = reference;
                body.wires = wires;
                body.silhouettes.clear();
            }
            // The cut mesh is drawn as the solid it bounds.
            EntityType::Mesh(_) | EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_) => {
                let mut solid = document::entities::Solid3D::new();
                solid.common = entity.common().clone();
                solid.point_of_reference = reference;
                solid.wires = wires;
                entity = EntityType::Solid3D(solid);
            }
            _ => return Err(()),
        }
        Ok(Some(entity))
    }

    pub(in crate::scene) fn section_source_body(
        &self,
        handle: Handle,
    ) -> Option<std::borrow::Cow<'_, ocs_solid::model::Brep>> {
        if let Some(body) = self.solid_models.get(&handle) {
            return Some(std::borrow::Cow::Borrowed(body));
        }
        let body = match self.document.get_entity(handle) {
            Some(EntityType::Solid3D(solid)) => {
                crate::scene::convert::solid3d_tess::solid_record(solid)
            }
            Some(EntityType::Region(region)) => {
                crate::scene::convert::solid3d_tess::region_record(region)
            }
            Some(EntityType::Body(body)) => {
                crate::scene::convert::solid3d_tess::acis_record(&body.acis_data)
            }
            Some(EntityType::Surface(surface)) => {
                crate::scene::convert::solid3d_tess::surface_record(surface)
            }
            // A mesh that closes a volume is cut as the solid it bounds.
            Some(
                mesh @ (EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_)),
            ) => crate::entities::mesh::closed_mesh_body(mesh),
            _ => None,
        }?;
        Some(std::borrow::Cow::Owned(body))
    }

    pub(in crate::scene) fn section_body(
        body: &ocs_solid::model::Brep,
        data: &document::entities::SectionObjectData,
        slice_depth: Option<f64>,
        body_to_world: Option<&document::types::Transform>,
    ) -> Result<Option<ocs_solid::model::Brep>, ()> {
        let planes = Self::section_planes(data, slice_depth, body_to_world).ok_or(())?;
        let mut current = body.clone();
        for plane in planes {
            match keep_section_positive(&current, plane)? {
                Some(next) => current = next,
                None => return Ok(None),
            }
        }
        Ok(Some(current))
    }

    /// The planes bounding what section `data` keeps, each facing inward:
    /// the section plane, then the back of a slice, the ends of a boundary
    /// and the top and bottom of a volume.
    pub(in crate::scene) fn section_planes(
        data: &document::entities::SectionObjectData,
        slice_depth: Option<f64>,
        body_to_world: Option<&document::types::Transform>,
    ) -> Option<Vec<ocs_geom::space::Plane>> {
        let (origin, tangent, vertical, viewing) = section_frame(data)?;
        let mut planes = vec![section_plane(origin, tangent, viewing, body_to_world)?];
        if data.state == 1 && slice_depth.is_none() {
            return Some(planes);
        }

        let depth = slice_depth.unwrap_or_else(|| {
            data.vertices
                .first()
                .zip(data.back_line_vertices.first())
                .map_or(0.0, |(front, back)| {
                    (glam::DVec3::new(back.x, back.y, back.z)
                        - glam::DVec3::new(front.x, front.y, front.z))
                    .dot(viewing)
                    .abs()
                })
        });
        if depth <= 1e-12 {
            return Some(planes);
        }
        planes.push(section_plane(
            origin + viewing * depth,
            tangent,
            -viewing,
            body_to_world,
        )?);
        if data.state == 1 {
            return Some(planes);
        }

        let first = data.vertices.first()?;
        let last = data.vertices.last()?;
        let first = glam::DVec3::new(first.x, first.y, first.z);
        let last = glam::DVec3::new(last.x, last.y, last.z);
        if (last - first).length() <= 1e-12 {
            return Some(planes);
        }
        planes.push(section_plane(first, vertical, tangent, body_to_world)?);
        planes.push(section_plane(last, vertical, -tangent, body_to_world)?);
        if data.state != 4 {
            return Some(planes);
        }

        planes.push(section_plane(
            origin - vertical * data.bottom_height,
            tangent,
            vertical,
            body_to_world,
        )?);
        planes.push(section_plane(
            origin + vertical * data.top_height,
            tangent,
            -vertical,
            body_to_world,
        )?);
        Some(planes)
    }

    /// Whether a top-level object in the current space can be picked right
    /// now. Mirrors the visibility predicate the resident wire build uses, so
    /// a selection command can never add an object the canvas does not draw.
    pub fn entity_pickable(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        let c = entity.common();
        if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
            return false;
        }
        if self.display_entity_invisible(c) || self.entity_temporarily_hidden(handle) {
            return false;
        }
        let layer = self.document.layers.get(&c.layer);
        if layer
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
            || self.layer_walk_hidden(layer)
        {
            return false;
        }
        if let Some(frozen) = self.interaction_viewport_frozen_layers() {
            if frozen.contains(&c.handle) || layer.is_some_and(|l| frozen.contains(&l.handle)) {
                return false;
            }
        }
        let scale = if self.current_layout == "Model" {
            self.displayed_annotation_scale_handle()
        } else {
            self.paper_annotation_scale_handle()
        };
        !crate::scene::annotative::annotative_offscale_for(
            &self.document,
            c,
            scale,
            self.annotation_all_visible(),
        )
    }

    /// True when `layer` is turned off or frozen — entities on it never render.
    /// Also true when the current space's LAYWALK set omits it: every hatch /
    /// fill / mesh consumer funnels through here, so the walk hides them too.
    pub(in crate::scene) fn layer_hidden(&self, layer: &str) -> bool {
        let record = self.document.layers.get(layer);
        record
            .map(|l| l.flags.off || l.flags.frozen)
            .unwrap_or(false)
            || self.layer_walk_hidden(record)
    }

    /// Handles hidden inside content viewport `vp`: its VP-frozen layers plus
    /// model-space references bound to another viewport (Inventor drawing
    /// views overlap in model space; each is drawn only in its own viewport).
    /// Entity and layer handles never collide, so both ride the same set and
    /// its cache signature.
    pub(crate) fn viewport_hidden_handles(&self, vp: Handle) -> HashSet<Handle> {
        let mut hidden: HashSet<Handle> = match self.document.get_entity(vp) {
            Some(EntityType::Viewport(v)) => v.frozen_layers.iter().copied().collect(),
            _ => HashSet::default(),
        };
        if self
            .document
            .classes
            .get_by_name("ACIDBLOCKREFERENCE")
            .is_some()
        {
            hidden.extend(
                self.view_rep_references()
                    .iter()
                    .filter(|(_, bound)| *bound != vp)
                    .map(|&(handle, _)| handle),
            );
        }
        hidden
    }

    /// Model-space INSERTs carrying a view-representation binding, cached for
    /// the current geometry epoch. Any entity add/remove/modify bumps the
    /// epoch, so a new or rebound reference is picked up on the next call; the
    /// model-space block handle is part of the key because a structure-only
    /// undo can swap it without touching geometry.
    pub(in crate::scene) fn view_rep_references(&self) -> Arc<Vec<(Handle, Handle)>> {
        let model = self.model_space_block_handle();
        if let Some((epoch, cached_model, references)) =
            self.caches.view_rep_reference_cache.borrow().as_ref()
        {
            if *epoch == self.geometry_epoch && *cached_model == model {
                return Arc::clone(references);
            }
        }
        let references: Vec<(Handle, Handle)> = self
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Insert(insert) if insert.common.owner_handle == model => insert
                    .view_rep_handle
                    .map(|bound| (insert.common.handle, bound)),
                _ => None,
            })
            .collect();
        let references = Arc::new(references);
        *self.caches.view_rep_reference_cache.borrow_mut() =
            Some((self.geometry_epoch, model, Arc::clone(&references)));
        references
    }

    /// True when `layer`'s handle is in a content viewport's per-viewport
    /// frozen-layer set (VP freeze). Mirrors the wire path's test in
    /// [`Scene::resident_entity_visible`] so fills / images / meshes hide on the
    /// same layers wires do. `None` / empty set → never frozen.
    pub(in crate::scene) fn layer_frozen_in(&self, layer: &str, frozen: Option<&HashSet<Handle>>) -> bool {
        // LAYWALK rides the same gate: consumers of VP freeze (fills, plot
        // fills, meshes) hide walked-out layers as if the viewport froze them.
        if self.layer_walk_hidden(self.document.layers.get(layer)) {
            return true;
        }
        match frozen {
            Some(fz) if !fz.is_empty() => self
                .document
                .layers
                .get(layer)
                .map(|l| fz.contains(&l.handle))
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Order-independent signature of a per-viewport frozen-layer set, so
    /// viewports that freeze the same layers share one cached filtered fill /
    /// image / mesh set. Matches the fold the resident wire cache uses.
    pub(in crate::scene) fn frozen_layers_sig(frozen: &HashSet<Handle>) -> u64 {
        let mut sig: u64 = frozen.len() as u64;
        for h in frozen.iter() {
            sig ^= h.value().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        sig
    }

    pub(in crate::scene) fn viewport_style_key(&self, viewport: Option<Handle>) -> u64 {
        let Some(viewport) = viewport.filter(|handle| handle.is_valid()) else {
            return 0;
        };
        let stale = self
            .caches.viewport_style_override_cache
            .borrow()
            .as_ref()
            .map(|(epoch, _)| *epoch != self.geometry_epoch)
            .unwrap_or(true);
        if stale {
            use document::objects::KnownXRecordKind;
            let kinds = [
                KnownXRecordKind::LayerViewportAlphaOverride,
                KnownXRecordKind::LayerViewportColorOverride,
                KnownXRecordKind::LayerViewportLinetypeOverride,
                KnownXRecordKind::LayerViewportLineweightOverride,
            ];
            let mut overridden = HashSet::default();
            for layer in self.document.layers.iter() {
                for kind in kinds {
                    overridden.extend(
                        self.document
                            .layer_viewport_overrides(layer.handle, kind)
                            .into_iter()
                            .map(|(handle, _)| handle),
                    );
                }
            }
            *self.caches.viewport_style_override_cache.borrow_mut() =
                Some((self.geometry_epoch, overridden));
        }
        self.caches.viewport_style_override_cache
            .borrow()
            .as_ref()
            .is_some_and(|(_, overridden)| overridden.contains(&viewport))
            .then_some(viewport.value())
            .unwrap_or(0)
    }

}
