use super::*;

impl Scene {
    /// True when a top-level entity must be omitted for a session-only object
    /// visibility command or an interactive replacement preview.
    pub(crate) fn entity_temporarily_hidden(&self, handle: Handle) -> bool {
        self.object_isolation.hides(handle)
            || self.preview_hidden.contains(&handle)
            || self.command_preview_hidden.contains(&handle)
            || self.mtext_editor_hidden == Some(handle)
            || self.hidden_dynamic_dimensions.contains(&handle)
    }

    /// Replace the command-owned source hide set and refresh only handles whose
    /// visibility changed. An empty slice restores every source on completion.
    pub fn set_command_preview_hidden(&mut self, handles: &[Handle]) {
        let desired: HashSet<_> = handles.iter().copied().collect();
        if desired == self.command_preview_hidden {
            return;
        }
        let changes: Vec<_> = self
            .command_preview_hidden
            .symmetric_difference(&desired)
            .copied()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.command_preview_hidden = desired;
        if !changes.is_empty() {
            // Visibility is a scene concern. Normal edit bookkeeping can
            // regenerate table pictures, prune fields and solve associations.
            let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
            self.geometry_epoch = epoch;
            self.invalidate_projection_bounds();
            for (handle, _) in &changes {
                self.caches.tess_memo.borrow_mut().remove(handle);
                self.caches.resident_tess_memo.borrow_mut().remove(handle);
            }
            self.push_geometry_delta(epoch, changes, false);
        }
    }

    /// The MTEXT the in-place editor replaces while it is open.
    pub(crate) fn mtext_editor_hidden(&self) -> Option<Handle> {
        self.mtext_editor_hidden
    }

    /// Hide only the source replaced by the MTEXT editor, independently from
    /// command previews and table-cell previews.
    pub(crate) fn set_mtext_editor_hidden(&mut self, handle: Option<Handle>) {
        if self.mtext_editor_hidden == handle {
            return;
        }
        let old = std::mem::replace(&mut self.mtext_editor_hidden, handle);
        let changes: Vec<_> = old
            .into_iter()
            .chain(handle)
            .map(|h| (h, ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            let epoch = GEOMETRY_EPOCH.fetch_add(1, Ordering::Relaxed);
            self.geometry_epoch = epoch;
            self.invalidate_projection_bounds();
            for (h, _) in &changes {
                self.caches.tess_memo.borrow_mut().remove(h);
                self.caches.resident_tess_memo.borrow_mut().remove(h);
            }
            self.push_geometry_delta(epoch, changes, false);
        }
    }

    /// Set (or clear) the previewed entity that renders with the selection
    /// highlight without joining the real selection. Only refreshes the GPU
    /// xray overlay (no re-tessellation).
    pub fn set_hover_highlight(&mut self, handle: Option<Handle>) {
        if self.hover_highlight == handle {
            return;
        }
        // Selectable groups roll over as one unit, matching click selection.
        // Exclude already-selected members because they already contribute to
        // the overlay and remain in the selected colour.
        //
        // The raw expansion of the new hover is computed once and stored
        // (`hover_expanded`); the old → new comparison reuses the stored old
        // expansion instead of re-running the group scan a second time, and
        // `hover_highlight_handles` (every rendered frame) reads the stored
        // set too.
        let expanded = handle
            .map(|handle| self.handles_expanded_for_selectable_groups(&[handle]))
            .unwrap_or_default();
        let contribution = |expanded: &HashSet<Handle>, selected: &HashSet<Handle>| {
            expanded
                .iter()
                .filter(|handle| !selected.contains(*handle))
                .copied()
                .collect::<HashSet<_>>()
        };
        let changed = contribution(&self.hover_expanded, &self.selected)
            != contribution(&expanded, &self.selected);
        self.hover_highlight = handle;
        self.hover_expanded = expanded;
        if changed {
            self.bump_hover();
        }
    }

    /// Handles that currently contribute the rollover colour. The stored hover
    /// remains the picked member for hit-test/UI bookkeeping; rendering uses
    /// the stored group expansion (`hover_expanded`) — no per-frame scan.
    pub fn hover_highlight_handles(&self) -> HashSet<Handle> {
        let mut handles = self.hover_expanded.clone();
        handles.extend(self.constraint_hover_highlights.iter().copied());
        handles.extend(self.box_hover_highlights.iter().copied());
        handles.extend(self.filter_preview_highlights.iter().copied());
        handles
    }

    /// Cached interaction sets for [`crate::scene::view`]'s `ViewportData`:
    /// `(selected handles, hover handles, selected signature)` keyed by the
    /// bit-encoded `(selection_generation, hover_generation)`. Per rendered
    /// frame per viewport this used to rebuild a fresh selected-set clone,
    /// expand the hover set and hash the selection — idle CPU that scaled
    /// with the selection size even when nothing moved. Cache invalidation
    /// rides the two generations: selection mutations, sub-segment picks and
    /// table-cell picks bump the selection generation; hover / marquee /
    /// constraint / filter sets bump the hover one.
    pub(in crate::scene) fn cached_viewport_interaction_sets(
        &self,
        show_interaction: bool,
    ) -> (
        std::sync::Arc<rustc_hash::FxHashSet<Handle>>,
        std::sync::Arc<rustc_hash::FxHashSet<Handle>>,
        u64,
    ) {
        let sel_key = self
            .selection_generation
            .wrapping_mul(2)
            .wrapping_add(u64::from(!show_interaction));
        let hover_key = self
            .hover_generation
            .wrapping_mul(2)
            .wrapping_add(u64::from(!show_interaction));
        {
            let cache = self.caches.viewport_interaction_sets.borrow();
            if let Some((cached_sel, cached_hover, selected, hover, sig)) = cache.as_ref() {
                if *cached_sel == sel_key && *cached_hover == hover_key {
                    return (
                        std::sync::Arc::clone(selected),
                        std::sync::Arc::clone(hover),
                        *sig,
                    );
                }
            }
        }
        let selected = if show_interaction {
            let cells = self.table_cells_handle();
            self.selected
                .iter()
                .copied()
                .filter(|handle| self.picked_segments(*handle).is_none())
                .filter(|handle| Some(*handle) != cells)
                .collect()
        } else {
            rustc_hash::FxHashSet::default()
        };
        let hover = if show_interaction {
            self.hover_highlight_handles()
        } else {
            rustc_hash::FxHashSet::default()
        };
        let sig = if show_interaction {
            self.selected_set_sig()
        } else {
            0
        };
        let selected = std::sync::Arc::new(selected);
        let hover = std::sync::Arc::new(hover);
        *self.caches.viewport_interaction_sets.borrow_mut() = Some((
            sel_key,
            hover_key,
            std::sync::Arc::clone(&selected),
            std::sync::Arc::clone(&hover),
            sig,
        ));
        (selected, hover, sig)
    }

    #[cfg(test)]
    pub(crate) fn box_hover_highlights(&self) -> &HashSet<Handle> {
        &self.box_hover_highlights
    }

    /// Roll over everything a selection box still being drawn would catch,
    /// before the release commits it. An empty slice clears the preview.
    pub fn set_box_hover_highlights(&mut self, handles: &[Handle]) {
        self.set_box_hover_highlights_limited(handles, usize::MAX);
    }

    pub fn set_box_hover_highlights_limited(&mut self, handles: &[Handle], limit: usize) {
        // Over the limit the preview used to be dropped *entirely* — a huge
        // marquee gave no feedback at all (and expanding tens of thousands of
        // candidates per tick was pure waste). Cap to the limit instead,
        // sorted ascending by handle so the highlighted subset stays stable
        // across ticks instead of flickering with the hash-set order. Group
        // expansion can multiply the count, so the cap applies to the
        // expanded set too.
        let mut handles: Vec<Handle> = handles.to_vec();
        if handles.len() > limit {
            handles.sort_unstable_by_key(|handle| handle.value());
            handles.dedup();
            handles.truncate(limit);
        }
        let mut handles = if handles.is_empty() {
            HashSet::default()
        } else {
            self.handles_expanded_for_selectable_groups(&handles)
        };
        if handles.len() > limit {
            let mut ordered: Vec<Handle> = handles.iter().copied().collect();
            ordered.sort_unstable_by_key(|handle| handle.value());
            handles.clear();
            handles.extend(ordered[..limit].iter().copied());
        }
        if handles != self.box_hover_highlights {
            self.box_hover_highlights = handles;
            // Box preview only ever changes the marquee highlight; bumping the
            // selection generation would rebuild the committed selection's GPU
            // overlay on every ~16 ms tick.
            self.bump_hover();
        }
    }

    /// Highlight what a selection filter would find (#2611). Unlike the box
    /// preview, pointer moves don't clear it; an empty slice does.
    pub fn set_filter_preview_highlights(&mut self, handles: &[Handle]) {
        let handles: HashSet<Handle> = handles.iter().copied().collect();
        if handles != self.filter_preview_highlights {
            self.filter_preview_highlights = handles;
            self.bump_hover();
        }
    }

    pub fn set_constraint_hover_highlights(
        &mut self,
        references: &[crate::scene::parametric_constraints::ParametricRef],
    ) {
        let mut references = references.to_vec();
        references.dedup();
        if references == self.constraint_hover_refs {
            return;
        }

        let mut handles = HashSet::default();
        let mut wires = Vec::new();
        for reference in &references {
            let Some(segment) = reference.segment_index() else {
                handles.insert(reference.entity);
                continue;
            };
            let Some(entity) = self.document.get_entity(reference.entity) else {
                continue;
            };
            let Some(mut curve) = crate::entities::curve::entity_curve(entity) else {
                handles.insert(reference.entity);
                continue;
            };
            let Some(segment_curve) = curve.curve.segments().into_iter().nth(segment) else {
                continue;
            };
            curve.curve = segment_curve;
            let mut wire = WireModel::solid_f64(
                format!("constraint-segment:{}:{segment}", reference.entity.value()),
                curve.tessellate(8.0),
                WireModel::HOVER,
                false,
            );
            wire.set_fixed_screen_width(2.0);
            wires.push(wire);
        }

        self.constraint_hover_refs = references;
        self.constraint_hover_highlights = handles;
        self.constraint_hover_wires = wires;
        self.bump_hover();
    }

    /// Keep the current selection visible and temporarily filter every other
    /// top-level object. Block-definition children are intentionally not added
    /// here: an isolated INSERT remains a complete visible block instance.
    pub fn isolate_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let keep = self.selected.clone();
        let active_block = self.interaction_block_handle();
        let hide: Vec<Handle> = self
            .document
            .entities()
            .filter(|entity| {
                let common = entity.common();
                !common.handle.is_null()
                    && !keep.contains(&common.handle)
                    && self.belongs_to_visible_block(
                        common.handle,
                        common.owner_handle,
                        active_block,
                    )
            })
            .map(|entity| entity.common().handle)
            .collect();
        let changes: Vec<_> = hide
            .iter()
            .copied()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.object_isolation.hidden.extend(hide);
        self.object_isolation.keep = Some(keep);
        // Isolation hides entities the memoised glyph placements read
        // (`entity_temporarily_hidden`), so invalidate them too.
        self.bump_constraints_epoch();
        if !changes.is_empty() {
            self.bump_entities(&changes);
        }
    }

    /// Temporarily hide the current selection without changing any entity
    /// property in the document.
    pub fn hide_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let changes: Vec<_> = self
            .selected
            .iter()
            .copied()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.object_isolation
            .hidden
            .extend(self.selected.iter().copied());
        self.selected.clear();
        self.selected_order.clear();
        self.bump_selection_set();
        // Newly hidden entities change `entity_temporarily_hidden`, which the
        // memoised glyph placements read — invalidate them too.
        self.bump_constraints_epoch();
        self.bump_entities(&changes);
    }

    /// Clear every session-only object visibility filter.
    pub fn end_isolation(&mut self) {
        if !self.object_isolation.is_active() {
            return;
        }
        let changes: Vec<_> = self
            .object_isolation
            .hidden
            .iter()
            .copied()
            .map(|handle| (handle, ChangeKind::Modified))
            .collect();
        self.object_isolation = ObjectIsolationState::default();
        // Restoring isolated entities changes `entity_temporarily_hidden`,
        // which the memoised glyph placements read — invalidate them too.
        // (`reset_transient_visibility` needs no bump: it only runs on
        // doc-replace, where `load_parametric_constraints_from_document`
        // already bumps the epoch for the whole-set replace.)
        self.bump_constraints_epoch();
        if !changes.is_empty() {
            self.bump_entities(&changes);
        }
    }

    /// Top-level objects of the current space that the drawing itself saves
    /// as invisible (`EntityCommon::invisible`, DXF 60) — e.g. objects another
    /// CAD program hid with HIDEOBJECTS and wrote to the file. Only objects
    /// owned by the space count: inside block definitions (and in the block
    /// editor) the flag drives dynamic-block visibility states.
    pub fn persisted_hidden_objects(&self) -> Vec<Handle> {
        if self.block_edit_block.is_some() {
            return Vec::new();
        }
        let space = self.interaction_block_handle();
        if space.is_null() {
            return Vec::new();
        }
        self.document
            .entities()
            .filter(|entity| {
                let common = entity.common();
                common.invisible
                    && !common.handle.is_null()
                    && self.belongs_to_visible_block(common.handle, common.owner_handle, space)
            })
            .map(|entity| entity.common().handle)
            .collect()
    }

    /// True when UNISOLATEOBJECTS has something to show: a session filter
    /// from ISOLATEOBJECTS / HIDEOBJECTS, or objects saved as invisible in
    /// the current space.
    pub fn has_hidden_objects(&self) -> bool {
        if self.object_isolation.is_active() {
            return true;
        }
        let space = if self.block_edit_block.is_some() {
            Handle::NULL
        } else {
            self.interaction_block_handle()
        };
        if let Some((epoch, cached_space, any)) = self.persisted_hidden_cache.get() {
            if cached_space == space {
                if epoch == self.geometry_epoch {
                    return any;
                }
                // The status bar asks on every frame and the answer is a
                // scan of every object in the space. Fold the edits since
                // the cached answer instead where they cannot change it:
                // with no hidden object before, only an edited object that
                // is now hidden can make one; with one before, only adds
                // leave it standing. Anything else scans again.
                if let Some(changes) = self.replay_since(epoch) {
                    let unchanged = if any {
                        changes
                            .iter()
                            .all(|(_, kind)| matches!(kind, ChangeKind::Added))
                    } else {
                        changes.iter().all(|(handle, _)| {
                            self.document
                                .get_entity(*handle)
                                .is_none_or(|entity| !entity.common().invisible)
                        })
                    };
                    if unchanged {
                        self.persisted_hidden_cache
                            .set(Some((self.geometry_epoch, space, any)));
                        return any;
                    }
                }
            }
        }
        let any = !self.persisted_hidden_objects().is_empty();
        self.persisted_hidden_cache
            .set(Some((self.geometry_epoch, space, any)));
        any
    }

    /// Clear the saved invisible flag of every object
    /// [`Self::persisted_hidden_objects`] reports. Returns each changed
    /// object's previous state for undo.
    pub fn show_persisted_hidden_objects(&mut self) -> Vec<(Handle, Arc<EntityType>)> {
        let mut originals = Vec::new();
        for handle in self.persisted_hidden_objects() {
            let Some(original) = self.document.get_entity_arc(handle) else {
                continue;
            };
            if let Some(entity) = self.document.get_entity_mut(handle) {
                entity.common_mut().invisible = false;
                originals.push((handle, original));
            }
        }
        let changes: Vec<_> = originals
            .iter()
            .map(|(handle, _)| (*handle, ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            self.bump_constraints_epoch();
            self.bump_entities(&changes);
        }
        originals
    }

    /// A newly opened/replaced document starts with no session visibility.
    /// Persisted `EntityCommon::invisible` values stay untouched and continue
    /// to serve file/dynamic-block visibility. Covers LAYWALK too: the walk
    /// is a session override, never a drawing property.
    pub fn reset_transient_visibility(&mut self) {
        self.block_visibility_reveal = false;
        self.object_isolation = ObjectIsolationState::default();
        self.layer_walk = LayerWalkState::default();
        self.preview_hidden.clear();
        self.command_preview_hidden.clear();
        self.mtext_editor_hidden = None;
    }

    /// True if any currently selected entity is a Viewport.
    /// Used to enable the scale picker when a viewport is selected in paper space.
    pub fn has_selected_viewport(&self) -> bool {
        self.selected
            .iter()
            .any(|&h| matches!(self.document.get_entity(h), Some(EntityType::Viewport(_))))
    }

    /// First content viewport handle in the current layout, used as fallback target
    /// when no viewport is active or explicitly selected.
    pub(in crate::scene) fn first_viewport_handle(&self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return None;
        }
        let layout_block = self.current_layout_block_handle();
        if layout_block.is_null() {
            return None;
        }
        self.document.entities().find_map(|e| {
            if let EntityType::Viewport(vp) = e {
                if self.is_content_viewport_in_layout(vp, layout_block) {
                    return Some(vp.common.handle);
                }
            }
            None
        })
    }

    pub(in crate::scene) fn target_viewport_handle(&self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return None;
        }
        self.active_viewport
            .filter(|handle| {
                matches!(
                    self.document.get_entity(*handle),
                    Some(EntityType::Viewport(_))
                )
            })
            .or_else(|| {
                self.selected.iter().copied().find(|handle| {
                    matches!(
                        self.document.get_entity(*handle),
                        Some(EntityType::Viewport(_))
                    )
                })
            })
            .or_else(|| self.first_viewport_handle())
    }

    pub(in crate::scene) fn explicit_viewport_handle(&self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return None;
        }
        self.active_viewport
            .filter(|handle| {
                matches!(
                    self.document.get_entity(*handle),
                    Some(EntityType::Viewport(_))
                )
            })
            .or_else(|| {
                self.selected.iter().copied().find(|handle| {
                    matches!(
                        self.document.get_entity(*handle),
                        Some(EntityType::Viewport(_))
                    )
                })
            })
    }

    pub(crate) fn viewport_scale_handle(&self, viewport: Handle) -> Option<Handle> {
        if let Some(handle) = self.document.viewport_annotation_scale(viewport) {
            if matches!(
                self.document.objects.get(&handle),
                Some(ObjectType::Scale(_))
            ) {
                return Some(handle);
            }
        }
        let EntityType::Viewport(vp) = self.document.get_entity(viewport)? else {
            return None;
        };
        let factor = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
        self.document
            .objects
            .iter()
            .filter_map(|(handle, object)| match object {
                ObjectType::Scale(scale) if !scale.is_temporary => {
                    Some((*handle, (self.effective_scale_factor(scale) - factor).abs()))
                }
                _ => None,
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(handle, _)| handle)
    }

    /// The name of the annotation scale a viewport shows its annotative
    /// objects at.
    pub(crate) fn viewport_annotation_scale_name(&self, viewport: Handle) -> Option<String> {
        match self
            .document
            .objects
            .get(&self.viewport_scale_handle(viewport)?)
        {
            Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
            _ => None,
        }
    }

    pub(in crate::scene) fn viewport_annotation_multiplier(&self, viewport: Handle) -> f32 {
        self.viewport_scale_handle(viewport)
            .and_then(|handle| match self.document.objects.get(&handle) {
                Some(ObjectType::Scale(scale)) => Some(scale.inverse_factor() as f32),
                _ => None,
            })
            .unwrap_or(self.annotation_scale)
    }

    pub fn displayed_annotation_scale_name(&self) -> String {
        if self.current_layout == "Model" {
            return self.document.header.current_annotation_scale.clone();
        }
        // Same viewport the pill number comes from (active → selected →
        // first-content fallback), so label, number and badge always describe
        // one viewport (#48). Degenerate case: with zero Scale objects the
        // name falls back to paper/"1:1" while the number stays factual.
        // The render paths use displayed_annotation_scale_HANDLE, which stays
        // explicit-only — untouched by this fix.
        self.target_viewport_handle()
            .and_then(|viewport| self.viewport_scale_handle(viewport))
            .and_then(|handle| match self.document.objects.get(&handle) {
                Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
                _ => None,
            })
            .or_else(|| {
                self.paper_annotation_scale_handle().and_then(|handle| {
                    match self.document.objects.get(&handle) {
                        Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
                        _ => None,
                    }
                })
            })
            .unwrap_or_else(|| "1:1".to_string())
    }

    /// The viewport(s) of the current paper layout, excluding the layout's
    /// own sheet viewport (id 1). Used by commands that act on a layout
    /// viewport without the user picking one.
    pub fn layout_viewport_handles(&self) -> Vec<Handle> {
        let layout_block = self.current_layout_block_handle();
        self.document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Viewport(vp) if vp.id > 1 && vp.common.owner_handle == layout_block => {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .collect()
    }

    /// The effective paper-to-model scale (paper units per model unit) that
    /// `viewport` is displaying at right now.
    pub fn viewport_effective_scale(&self, viewport: Handle) -> Option<f64> {
        let EntityType::Viewport(vp) = self.document.get_entity(viewport)? else {
            return None;
        };
        Some(crate::scene::vp_effective_scale(
            vp.custom_scale,
            vp.view_height,
            vp.height,
        ))
    }

    /// Display name of the named scale object driving `viewport`, if any.
    pub fn viewport_display_scale_name(&self, viewport: Handle) -> Option<String> {
        let scale_handle = self.viewport_scale_handle(viewport)?;
        match self.document.objects.get(&scale_handle) {
            Some(ObjectType::Scale(scale)) => Some(scale.name.clone()),
            _ => None,
        }
    }

    pub fn set_viewport_scale_named(&mut self, name: &str) -> Option<Handle> {
        let viewport = self.scale_target_viewport()?;
        self.set_viewport_scale_named_for(viewport, name)
    }

    /// The viewport a scale picked from the list applies to: the active or a
    /// selected one, else the layout's only viewport. With several and none
    /// picked there is none, so no viewport changes behind the user's back.
    pub fn scale_target_viewport(&self) -> Option<Handle> {
        self.explicit_viewport_handle().or_else(|| {
            let (_, _, content) = self.paper_viewport_handles();
            match content.as_slice() {
                [only] => Some(*only),
                _ => None,
            }
        })
    }

    /// Whether `viewport`'s display is locked against zoom and scale changes.
    pub fn viewport_display_locked(&self, viewport: Handle) -> bool {
        matches!(
            self.document.get_entity(viewport),
            Some(EntityType::Viewport(vp)) if vp.status.locked
        )
    }

    pub fn set_viewport_scale_named_for(&mut self, viewport: Handle, name: &str) -> Option<Handle> {
        let scale_handle = self.scale_handle_ensuring(name)?;

        // Honor legacy standard ratios without converting custom/file ratios twice.
        let factor = match self.document.objects.get(&scale_handle) {
            Some(ObjectType::Scale(scale)) => self.effective_scale_factor(scale),
            _ => return None,
        };

        let locked = matches!(
            self.document.get_entity(viewport),
            Some(EntityType::Viewport(vp)) if vp.status.locked
        );

        if locked || factor <= 1.0e-9 {
            return None;
        }

        if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(viewport) {
            vp.custom_scale = factor;
            vp.view_height = vp.height / factor;
        } else {
            return None;
        }

        self.document
            .set_viewport_annotation_scale(viewport, scale_handle);

        // Viewport scale changed: the dimensions drawn on the sheet
        // through it measure the same model geometry at a new paper
        // size and have to be re-placed.
        self.notify_viewport_changed(viewport);
        self.caches.resident_wire_sets.borrow_mut().clear();
        self.bump_geometry();

        Some(scale_handle)
    }

    /// Scale the active viewport to `factor` paper units per model unit
    /// (ZOOM nXP). A named scale of that factor is linked as its annotation
    /// scale; otherwise the viewport keeps a custom scale.
    pub fn set_viewport_scale_factor(&mut self, factor: f64) -> bool {
        let Some(viewport) = self.explicit_viewport_handle() else {
            return false;
        };
        self.set_viewport_scale_factor_for(viewport, factor)
    }

    /// Scale `viewport` to `factor` paper units per model unit, as
    /// [`Self::set_viewport_scale_factor`] does for the active one.
    pub fn set_viewport_scale_factor_for(&mut self, viewport: Handle, factor: f64) -> bool {
        let named = self
            .document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::Scale(scale)
                    if !scale.is_temporary
                        && (self.effective_scale_factor(scale) - factor).abs()
                            <= 1.0e-9 * factor.max(1.0) =>
                {
                    Some(scale.name.clone())
                }
                _ => None,
            });
        if let Some(name) = named {
            return self.set_viewport_scale_named_for(viewport, &name).is_some();
        }
        match self.document.get_entity_mut(viewport) {
            Some(EntityType::Viewport(vp)) if !vp.status.locked && factor > 1.0e-9 => {
                vp.custom_scale = factor;
                vp.view_height = vp.height / factor;
            }
            _ => return false,
        }
        self.notify_viewport_changed(viewport);
        self.caches.resident_wire_sets.borrow_mut().clear();
        self.bump_geometry();
        true
    }

    pub fn viewport_annotation_scale_synced(&self) -> Option<bool> {
        // Same viewport as the pill number and label (#48).
        let viewport = self.target_viewport_handle()?;
        let EntityType::Viewport(vp) = self.document.get_entity(viewport)? else {
            return None;
        };

        let scale = self.viewport_scale_handle(viewport)?;
        let ObjectType::Scale(scale) = self.document.objects.get(&scale)? else {
            return None;
        };

        let effective = vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);

        let expected = self.effective_scale_factor(scale);

        Some((effective - expected).abs() <= 1.0e-6 * expected.max(1.0))
    }
    pub fn sync_viewport_annotation_scale(&mut self) -> bool {
        // Same fallback as the pill and the displayed scale name: an active
        // viewport wins, then a picked one, then the first with content.
        let Some(viewport) = self.target_viewport_handle() else {
            return false;
        };
        let Some(scale_handle) = self.viewport_scale_handle(viewport) else {
            return false;
        };
        let factor = match self.document.objects.get(&scale_handle) {
            Some(ObjectType::Scale(scale)) => self.effective_scale_factor(scale),
            _ => return false,
        };
        let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(viewport) else {
            return false;
        };
        if vp.status.locked || factor <= 1.0e-9 {
            return false;
        }
        vp.custom_scale = factor;
        vp.view_height = vp.height / factor;
        self.document
            .set_viewport_annotation_scale(viewport, scale_handle);
        // Viewport scale changed: the dimensions drawn on the sheet
        // through it measure the same model geometry at a new paper
        // size and have to be re-placed.
        self.notify_viewport_changed(viewport);
        self.caches.resident_wire_sets.borrow_mut().clear();
        self.bump_geometry();
        true
    }

    /// Sorted list of layout names: "Model" first, then paper layouts by tab order.
    pub fn layout_names(&self) -> Vec<String> {
        let mut names = vec!["Model".to_string()];
        // Deduplicate by name: prefer the entry with a non-null block_record (the
        // real layout from the file) over the default placeholder created by
        // Document::new().
        let mut by_name: rustc_hash::FxHashMap<String, (i16, Handle)> = Default::default();
        for obj in self.document.objects.values() {
            if let ObjectType::Layout(l) = obj {
                if l.name == "Model" || l.name.is_empty() {
                    continue;
                }
                let entry = by_name
                    .entry(l.name.clone())
                    .or_insert((l.tab_order, l.block_record));
                if entry.1.is_null() && !l.block_record.is_null() {
                    *entry = (l.tab_order, l.block_record);
                }
            }
        }
        let mut paper: Vec<(i16, String)> = by_name
            .into_iter()
            .map(|(name, (order, _))| (order, name))
            .collect();
        paper.sort_by_key(|(order, _)| *order);
        names.extend(paper.into_iter().map(|(_, n)| n));
        names
    }

    pub fn bump_layout_epoch(&mut self) {
        self.layout_epoch += 1;
    }

    pub fn cached_layout_names(&self) -> std::sync::Arc<Vec<String>> {
        if let Some((epoch, names)) = self.caches.layout_names_cache.borrow().as_ref() {
            if *epoch == self.layout_epoch {
                return std::sync::Arc::clone(names);
            }
        }
        let names = std::sync::Arc::new(self.layout_names());
        *self.caches.layout_names_cache.borrow_mut() =
            Some((self.layout_epoch, std::sync::Arc::clone(&names)));
        names
    }

    pub fn bump_scale_epoch(&mut self) {
        self.scale_epoch += 1;
    }

    /// Current parametric-constraint-set epoch (cache key input for the
    /// memoised `cached_glyph_placements` accessor).
    #[allow(dead_code)]
    pub(crate) fn constraints_epoch(&self) -> u64 {
        self.constraints_epoch
    }

    /// Drops the named parameters in `names` that no constraint drives any more.
    ///
    /// A parameter declared by a constraint goes with that constraint, but a
    /// name any constraint set still drives stays. The scan spans every set
    /// because a block set may name a drawing-level parameter. Names that only
    /// formulas read are not consulted, so the rule errs toward keeping.
    pub(crate) fn drop_unread_named_parameters(&mut self, names: Vec<String>) {
        let unread: Vec<String> = names
            .into_iter()
            .filter(|name| {
                !self
                    .parametric_constraints
                    .iter()
                    .flat_map(|set| set.constraints.iter())
                    .any(|constraint| {
                        matches!(
                            &constraint.driving_param,
                            Some(crate::scene::named_parameters::DrivingValue::Named(used))
                                if used == name
                        )
                    })
            })
            .collect();
        if unread.is_empty() {
            return;
        }
        self.record_undo_named_parameters_before();
        for name in &unread {
            self.named_parameters_mut().remove(name);
        }
    }

    /// Invalidate constraint-glyph consumers. Call on EVERY mutation of the
    /// constraint set or its visibility overrides — a missed bump serves
    /// stale glyphs.
    pub(crate) fn bump_constraints_epoch(&mut self) {
        self.constraints_epoch += 1;
    }

    pub fn cached_scale_picker_list(&self) -> std::sync::Arc<Vec<(String, f32, f64)>> {
        let factor = self.default_scale_unit_factor();
        let imperial = self.prefers_imperial_scales();
        let current = self.document.header.current_annotation_scale.clone();
        if let Some((epoch, cached_factor, cached_imperial, cached_current, list)) =
            self.caches.scale_picker_cache.borrow().as_ref()
        {
            if *epoch == self.scale_epoch
                && *cached_factor == factor
                && *cached_imperial == imperial
                && *cached_current == current
            {
                return std::sync::Arc::clone(list);
            }
        }
        let list = std::sync::Arc::new(self.scale_picker_list());
        *self.caches.scale_picker_cache.borrow_mut() = Some((
            self.scale_epoch,
            factor,
            imperial,
            current,
            std::sync::Arc::clone(&list),
        ));
        list
    }

    pub fn bump_ucs_epoch(&mut self) {
        self.ucs_epoch += 1;
    }

    /// Named-UCS names for the ViewCube picker, memoised behind `ucs_epoch`
    /// (#34, mirrors `cached_layout_names`). Same empty-name filter the view
    /// blocks used when building the list inline every frame.
    pub fn cached_ucs_names(&self) -> std::sync::Arc<[String]> {
        if let Some((epoch, names)) = self.caches.ucs_names_cache.borrow().as_ref() {
            if *epoch == self.ucs_epoch {
                return std::sync::Arc::clone(names);
            }
        }
        let names: std::sync::Arc<[String]> = self
            .document
            .ucss
            .iter()
            .map(|u| u.name.clone())
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>()
            .into();
        *self.caches.ucs_names_cache.borrow_mut() = Some((self.ucs_epoch, std::sync::Arc::clone(&names)));
        names
    }

    /// Wire set for the Model layout, shared by every tile.
    ///
    /// The model wire geometry is **camera-independent**, so it is tessellated
    /// in full (un-culled, fixed detail) once per geometry epoch, held resident
    /// (`model_static_wires`), and returned for any camera/tile. A pan/zoom only
    /// changes the view uniform — the GPU re-draws the same buffer, with no
    /// frustum cull, no zoom LOD, and no re-tessellation/re-upload on camera
    /// moves. The per-tile camera args are unused (kept for call-site symmetry
    /// with the paper-space wire sources).
    #[doc(hidden)]
    pub fn model_tile_wires_arc(
        &self,
        _tile_idx: usize,
        _cam: &Camera,
        _cam_aspect: f32,
        _tile_pixel_height: f32,
    ) -> Arc<Vec<WireModel>> {
        // In a BEDIT block editor the resident set is the edited block's own
        // (block-local) entities; otherwise model space. (#261)
        let block = self
            .block_edit_block
            .unwrap_or_else(|| self.model_space_block_handle());
        let scale = crate::scene::annotative::scale_handle_by_name(
            &self.document,
            &self.document.header.current_annotation_scale,
        );
        self.resident_wires_for(block, None, scale, None, None)
    }
}
