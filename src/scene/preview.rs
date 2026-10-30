// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;
use crate::command::{AreaPreviewRegion, AreaPreviewSource};
use crate::scene::model::hatch_model::HatchPattern;

mod block_parameter;

impl Scene {
    // ── Preview wire ──────────────────────────────────────────────────────

    pub fn set_preview_wires(&mut self, wires: Vec<WireModel>) {
        // Preview wires are an overlay appended to the cached base wire set in
        // `build_primitive`; they are NOT part of the tessellation cache. So a
        // preview update must NOT bump `geometry_epoch` — that would re-
        // tessellate the whole model on every rubber-band frame. The overlay
        // forces a GPU wire re-upload on its own (the `has_overlay` content-id
        // path), and iced redraws after the message that set the preview.
        self.preview_wires = wires;
    }

    /// Publish all edited hatches as one live fill overlay.
    pub fn set_preview_hatches(&mut self, handles: &[Handle]) {
        let mut models = Vec::new();
        for &handle in handles {
            // Keep the existing direct Hatch/Solid preview path. INSERT has no
            // entry in `self.hatches`, so this is a no-op for it and the full
            // block expansion below supplies its fills.
            self.append_preview_hatch(handle, &mut models);
        }
        models.extend(self.preview_insert_hatch_models(handles));
        self.preview_hatches = std::sync::Arc::new(models);
    }

    pub fn set_command_preview_hatches(&mut self, models: Vec<HatchModel>) {
        self.preview_hatches = std::sync::Arc::new(models);
    }

    /// Selected Model-space hatches use a small overlay instead of baking the
    /// selection colour into the resident hatch list. This keeps selection
    /// changes from regenerating and uploading every hatch in a large drawing.
    pub(super) fn selected_hatch_overlay_models(&self) -> std::sync::Arc<Vec<HatchModel>> {
        if self.current_layout != "Model" || !self.document.header.fill_mode {
            return std::sync::Arc::new(Vec::new());
        }
        let selection_sig = self.selected_hatch_sig();
        if selection_sig == 0 {
            return std::sync::Arc::new(Vec::new());
        }
        if let Some((epoch, sig, models)) = self.selected_hatch_overlay_cache.borrow().as_ref() {
            if *epoch == self.geometry_epoch && *sig == selection_sig {
                return std::sync::Arc::clone(models);
            }
        }

        let started = crate::perf::enabled().then(iced::time::Instant::now);
        let depth_map = self.draw_depth_map();
        let target_block = self.content_render_block_handle();
        let annotation_scale = self.displayed_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();
        // Same visibility and style sync as the resident set, so the tinted
        // copy lands exactly on the fill it covers.
        let mut models: Vec<HatchModel> = self
            .selected
            .iter()
            .filter_map(|&handle| Some((handle, self.hatches.get(&handle)?)))
            .filter(|&(handle, _)| {
                self.hatch_fill_shown(handle, target_block, None, annotation_scale, all_visible)
            })
            .map(|(handle, model)| {
                self.synced_hatch_model(
                    handle,
                    model,
                    annotation_scale,
                    None,
                    self.bg_color,
                    &depth_map,
                    true,
                )
                .1
            })
            .collect();
        let selected_inserts: Vec<Handle> = self
            .selected
            .iter()
            .copied()
            .filter(|&handle| {
                let Some(codec::EntityType::Insert(insert)) = self.document.get_entity(handle)
                else {
                    return false;
                };
                let common = &insert.common;
                !self.display_entity_invisible(common)
                    && !self.entity_temporarily_hidden(handle)
                    && !self.layer_hidden(&common.layer)
            })
            .collect();
        models.extend(self.preview_insert_hatch_models(&selected_inserts));
        let models = std::sync::Arc::new(models);
        let elapsed_ms = crate::perf::elapsed_ms(started);
        if elapsed_ms >= 1.0 {
            crate::perf_record!(
                "[perf] hatch-selection-overlay build={elapsed_ms:.1}ms selected={} models={}",
                self.selected.len(),
                models.len(),
            );
        }
        *self.selected_hatch_overlay_cache.borrow_mut() = Some((
            self.geometry_epoch,
            selection_sig,
            std::sync::Arc::clone(&models),
        ));
        models
    }

    /// Merge command/grip previews with the selected-hatch tint overlay and
    /// reuse the result until either source changes.
    pub(super) fn render_preview_hatches(&self) -> std::sync::Arc<Vec<HatchModel>> {
        let selected = self.selected_hatch_overlay_models();
        if selected.is_empty() {
            return std::sync::Arc::clone(&self.preview_hatches);
        }
        if let Some((preview, overlay, combined)) =
            self.combined_preview_hatch_cache.borrow().as_ref()
        {
            if std::sync::Arc::ptr_eq(preview, &self.preview_hatches)
                && std::sync::Arc::ptr_eq(overlay, &selected)
            {
                return std::sync::Arc::clone(combined);
            }
        }
        let mut models = Vec::with_capacity(self.preview_hatches.len() + selected.len());
        models.extend(self.preview_hatches.iter().cloned());
        models.extend(selected.iter().cloned());
        let combined = std::sync::Arc::new(models);
        *self.combined_preview_hatch_cache.borrow_mut() = Some((
            std::sync::Arc::clone(&self.preview_hatches),
            selected,
            std::sync::Arc::clone(&combined),
        ));
        combined
    }

    pub fn set_area_preview_regions(&mut self, regions: &[AreaPreviewRegion]) {
        let mut models = Vec::new();
        for region in regions {
            match &region.source {
                AreaPreviewSource::Handles(handles) => {
                    for &handle in handles {
                        self.append_area_preview_handle(handle, region.subtract, &mut models);
                    }
                }
                AreaPreviewSource::Boundary(boundary) => {
                    if let Some(model) =
                        Self::area_preview_hatch(std::slice::from_ref(boundary), region.subtract)
                    {
                        models.push(model);
                    }
                }
            }
        }
        self.preview_hatches = std::sync::Arc::new(models);
    }

    fn append_area_preview_handle(
        &self,
        handle: Handle,
        subtract: bool,
        models: &mut Vec<HatchModel>,
    ) {
        if let Some(mut model) = self.hatches.get(&handle).cloned() {
            Self::style_area_preview_hatch(&mut model, subtract);
            models.push(model);
            return;
        }

        let direct_boundary =
            crate::scene::project::clip_boundary_polygon_for_document(&self.document, handle, 0.0);
        let mut rings = if direct_boundary.len() >= 3 {
            vec![direct_boundary
                .into_iter()
                .map(|point| [point[0] as f64, point[1] as f64])
                .collect()]
        } else {
            Vec::new()
        };
        if !rings.is_empty() {
            if let Some(model) = Self::area_preview_hatch(&rings, subtract) {
                models.push(model);
            }
            return;
        }
        for wire in self.wire_models_for(&[handle]) {
            let mut ring = Vec::new();
            for (index, point) in wire.points.iter().enumerate() {
                let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
                let x = point[0] as f64 + low[0] as f64;
                let y = point[1] as f64 + low[1] as f64;
                if x.is_finite() && y.is_finite() {
                    let candidate = [x, y];
                    if ring.last().is_none_or(|last| *last != candidate) {
                        ring.push(candidate);
                    }
                } else {
                    Self::push_area_preview_ring(&mut rings, &mut ring);
                }
            }
            Self::push_area_preview_ring(&mut rings, &mut ring);
        }
        if let Some(model) = Self::area_preview_hatch(&rings, subtract) {
            models.push(model);
        }
    }

    fn push_area_preview_ring(rings: &mut Vec<Vec<[f64; 2]>>, ring: &mut Vec<[f64; 2]>) {
        if ring.len() >= 3 {
            let min_x = ring
                .iter()
                .map(|point| point[0])
                .fold(f64::INFINITY, f64::min);
            let max_x = ring
                .iter()
                .map(|point| point[0])
                .fold(f64::NEG_INFINITY, f64::max);
            let min_y = ring
                .iter()
                .map(|point| point[1])
                .fold(f64::INFINITY, f64::min);
            let max_y = ring
                .iter()
                .map(|point| point[1])
                .fold(f64::NEG_INFINITY, f64::max);
            let diagonal_sq = (max_x - min_x).powi(2) + (max_y - min_y).powi(2);
            let twice_area = ring
                .iter()
                .zip(ring.iter().cycle().skip(1))
                .take(ring.len())
                .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
                .sum::<f64>()
                .abs();
            if twice_area > diagonal_sq * 1e-12 {
                rings.push(std::mem::take(ring));
                return;
            }
        }
        ring.clear();
    }

    fn area_preview_hatch(rings: &[Vec<[f64; 2]>], subtract: bool) -> Option<HatchModel> {
        let origin = rings.iter().find_map(|ring| ring.first()).copied()?;
        let mut boundary = Vec::new();
        let mut first = true;
        for ring in rings.iter().filter(|ring| ring.len() >= 3) {
            if !first {
                boundary.push([f32::NAN, f32::NAN]);
            }
            first = false;
            boundary.extend(
                ring.iter()
                    .map(|point| [(point[0] - origin[0]) as f32, (point[1] - origin[1]) as f32]),
            );
        }
        if boundary.len() < 3 {
            return None;
        }
        let mut model = HatchModel {
            pattern_origin: None,
            render_instance: None,
            world_origin: origin,
            boundary: std::sync::Arc::new(boundary),
            boundary_wcs: None,
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: codec::entities::HatchStyleType::Normal,
            pattern: HatchPattern::Solid,
            name: "AREA_PREVIEW".into(),
            color: [0.0; 4],
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            draw_depth: 0.0,
        };
        Self::style_area_preview_hatch(&mut model, subtract);
        Some(model)
    }

    fn style_area_preview_hatch(model: &mut HatchModel, subtract: bool) {
        model.pattern = HatchPattern::Solid;
        model.name = "AREA_PREVIEW".into();
        model.color = if subtract {
            [1.0, 0.28, 0.18, 0.16]
        } else {
            [0.15, 0.55, 1.0, 0.12]
        };
        model.aci = 0;
        model.line_weight_px = 1.0;
        model.angle_offset = 0.0;
        model.scale = 1.0;
        model.draw_depth = 0.0;
    }

    fn append_preview_hatch(&self, handle: Handle, models: &mut Vec<HatchModel>) {
        let Some(mut model) = self.hatches.get(&handle).cloned() else {
            return;
        };
        let Some(entity) = self.document.get_entity(handle) else {
            return;
        };

        let style = self.render_style(entity);
        model.aci = style.4;
        model.line_weight_px = style.3;
        if !matches!(
            model.pattern,
            crate::scene::model::hatch_model::HatchPattern::Gradient { .. }
        ) {
            model.color = style.0;
        }
        if self.selected.contains(&handle) {
            model.color = [0.15, 0.55, 1.00, model.color[3]];
        }
        model.draw_depth = self
            .draw_depth_map()
            .get(&handle.value())
            .map_or(0.0, |depth| depth[0]);

        if let EntityType::Hatch(hatch) = entity {
            models.extend(self.hatch_backdrop(hatch, &model, (style.0, style.4), None));
        }
        models.push(model);
    }

    pub fn set_preview_text(&mut self, verts: Vec<crate::scene::pipeline::text_gpu::TextVertex>) {
        // Overlay glyphs — same reasoning as `set_preview_wires`: no geometry
        // bump. Uploaded to a dedicated per-frame text buffer in `prepare`.
        self.preview_text = verts;
    }

    pub fn clear_preview_wire(&mut self) {
        self.set_command_preview_hidden(&[]);
        // No geometry bump — see `set_preview_wires`. Dropping the overlay
        // flips the wire content id back to the base tessellation id, which
        // re-uploads the base wires (without the preview) on the next frame.
        self.preview_wires = vec![];
        self.solid_history_preview_wires.clear();
        if !self.preview_hatches.is_empty() {
            self.preview_hatches = std::sync::Arc::new(Vec::new());
        }
        self.preview_text = vec![];
        self.interim_wire = None;
        // Drop any point-picked window marquee (STRETCH) so it doesn't linger
        // after the command ends before the next mouse move. (#291)
        self.selection.borrow_mut().gesture.preview_box = None;
    }

    pub fn wire_models_for(&self, handles: &[codec::Handle]) -> Vec<WireModel> {
        handles
            .iter()
            .flat_map(|h| {
                match self.document.get_entity(*h) {
                    // Hatches carry no outline in the normal wire set, but an
                    // edit preview (move / copy / array / grip-drag) needs to
                    // show the shape following the cursor. Build a live boundary
                    // from the current HatchModel — `apply_grip` keeps it in
                    // step, so the preview tracks a dragged grip in real time.
                    Some(EntityType::Hatch(_)) => self.hatch_outline_wire(*h).into_iter().collect(),
                    Some(e) => self.tessellate_one(e),
                    None => Vec::new(),
                }
            })
            .collect()
    }

    /// Current candidate geometry for a hot grip. Solid-history candidates
    /// live outside the document until placement; ordinary entities continue
    /// to use their current tessellation.
    pub fn grip_wire_models_for(&self, handles: &[codec::Handle]) -> Vec<WireModel> {
        handles
            .iter()
            .flat_map(|handle| {
                self.solid_history_preview_wires
                    .get(handle)
                    .cloned()
                    .unwrap_or_else(|| self.wire_models_for(std::slice::from_ref(handle)))
            })
            .collect()
    }

    /// Split a MIRROR selection into plain ghost wires (reflected wholesale) and
    /// text ghosts paired with their bounding-box centre. Lets the preview match
    /// the commit for TEXT: MIRRTEXT on → true glyph mirror (full reflection, same
    /// as plain geometry); MIRRTEXT off → right-reading at the mirror-symmetric
    /// position (reflect the centre, translate) instead of hugging the axis.
    pub fn mirror_preview_parts(
        &self,
        handles: &[Handle],
    ) -> (Vec<WireModel>, Vec<(WireModel, glam::DVec3)>) {
        let mut plain: Vec<WireModel> = Vec::new();
        let mut texts: Vec<(WireModel, glam::DVec3)> = Vec::new();
        for h in handles {
            let Some(e) = self.document.get_entity(*h) else {
                continue;
            };
            if matches!(e, EntityType::Text(_)) {
                let wires = self.tessellate_one(e);
                let mut lo = [f64::INFINITY; 2];
                let mut hi = [f64::NEG_INFINITY; 2];
                for w in &wires {
                    for (i, p) in w.points.iter().enumerate() {
                        if !p[0].is_finite() || !p[1].is_finite() {
                            continue;
                        }
                        // Reconstruct f64 world from the double-single pair (the
                        // low residual may be absent on some wires).
                        let l = w.points_low.get(i).copied().unwrap_or([0.0; 3]);
                        let (x, y) = (p[0] as f64 + l[0] as f64, p[1] as f64 + l[1] as f64);
                        lo[0] = lo[0].min(x);
                        lo[1] = lo[1].min(y);
                        hi[0] = hi[0].max(x);
                        hi[1] = hi[1].max(y);
                    }
                }
                let center = if lo[0] <= hi[0] {
                    glam::DVec3::new((lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, 0.0)
                } else {
                    glam::DVec3::ZERO
                };
                for w in wires {
                    texts.push((w, center));
                }
            } else {
                plain.extend(self.tessellate_one(e));
            }
        }
        (plain, texts)
    }

    /// Boundary wire for edit previews and selected hatches.
    pub(super) fn hatch_outline_wire(&self, handle: Handle) -> Option<WireModel> {
        let m = self.hatches.get(&handle)?;
        Self::hatch_model_outline_wire(handle, m)
    }

    fn hatch_model_outline_wire(handle: Handle, m: &HatchModel) -> Option<WireModel> {
        let (wx, wy) = (m.world_origin[0], m.world_origin[1]);
        let pts: Vec<[f64; 3]> = m
            .boundary
            .iter()
            .map(|&[x, y]| {
                if x.is_finite() && y.is_finite() {
                    [wx + x as f64, wy + y as f64, 0.0]
                } else {
                    [f64::NAN; 3]
                }
            })
            .collect();
        if pts.len() < 2 {
            return None;
        }
        let mut wire = WireModel::solid_f64(handle.value().to_string(), pts, m.color, false);
        wire.aci = m.aci;
        wire.line_weight_px = m.line_weight_px;
        Some(wire)
    }

    /// Fill-only geometry has no resident wire for the normal rollover xray.
    /// Supply an orange boundary overlay while an entity-pick command previews
    /// a top-level hatch or an Insert whose visible children contain hatches.
    pub fn fill_hover_preview_wires(&self, handle: Handle) -> Vec<WireModel> {
        let mut wires: Vec<WireModel> = match self.document.get_entity(handle) {
            Some(EntityType::Hatch(_)) => self.hatch_outline_wire(handle).into_iter().collect(),
            Some(EntityType::Insert(_)) => self
                .insert_hatches_for_click()
                .get(&handle)
                .into_iter()
                .flatten()
                .filter_map(|model| Self::hatch_model_outline_wire(handle, model))
                .collect(),
            _ => Vec::new(),
        };
        for wire in &mut wires {
            wire.color = WireModel::HOVER;
            wire.line_weight_px = wire.line_weight_px.max(2.0);
        }
        wires
    }

    /// Highlight a MATCHPROP source while the command gathers its first target.
    pub fn matchprop_source_highlight_wires(&self, handle: Handle) -> Vec<WireModel> {
        let Some(entity) = self.document.get_entity(handle).cloned() else {
            return Vec::new();
        };
        let mut wires = self.wires_for_entities(&[entity]);
        // Hatch fills and nested insert fills have no resident outline wire,
        // so include their transient boundary geometry as well.
        wires.extend(self.fill_hover_preview_wires(handle));
        Self::matchprop_glow_with_layers(&wires, [(10.0, 0.11), (6.0, 0.22), (2.5, 0.48)])
    }

    /// Add a faint aura behind the property preview, leaving its matched style
    /// easy to read. The original wires should be appended after these layers.
    pub fn matchprop_target_glow_wires(&self, wires: &[WireModel]) -> Vec<WireModel> {
        Self::matchprop_glow_with_layers(wires, [(8.0, 0.035), (5.0, 0.065), (2.5, 0.11)])
    }

    fn matchprop_glow_with_layers(
        wires: &[WireModel],
        layers: [(f32, f32); 3],
    ) -> Vec<WireModel> {
        let mut glow = Vec::with_capacity(wires.len() * 3);
        for wire in wires {
            // Layer translucent, widened copies over the resident entity. Its
            // original color remains visible through the blue hover aura.
            for (width, alpha) in layers {
                let mut layer = wire.clone();
                layer.color = [0.22, 0.60, 1.0, alpha];
                layer.line_weight_px = layer.line_weight_px.max(width);
                layer.selected = false;
                glow.push(layer);
            }
        }
        glow
    }

    /// Build a non-mutating MATCHPROP target preview using the same property
    /// kernel as the eventual apply, including the fill style for hatches.
    pub fn matchprop_target_preview(
        &self,
        source: Handle,
        target: Handle,
    ) -> Option<(Vec<WireModel>, Vec<HatchModel>)> {
        let source_entity = self.document.get_entity(source)?.clone();
        let mut target_entity = self.document.get_entity(target)?.clone();
        crate::entities::match_props::match_properties_kernel(
            &source_entity,
            &mut target_entity,
            &crate::entities::match_props::MatchOpts::all(),
        );

        // The interactive handler also transfers this hatch-specific xdata
        // outside the shared entity kernel. Mirror it on the preview clone so
        // the fill reflects the same background that a click will apply.
        if let (EntityType::Hatch(source_hatch), EntityType::Hatch(target_hatch)) =
            (&source_entity, &mut target_entity)
        {
            match crate::entities::hatch::background_color(source_hatch) {
                Some(color) => crate::entities::hatch::set_background_color(target_hatch, &color),
                None => crate::entities::hatch::clear_background_color(target_hatch),
            }
        }

        let mut wires = self.wires_for_entities(std::slice::from_ref(&target_entity));
        for wire in &mut wires {
            wire.selected = false;
        }

        let color = self.render_style(&target_entity).0;
        let hatch = match &target_entity {
            EntityType::Hatch(dxf) => Self::hatch_model_from_dxf(dxf, color),
            EntityType::Solid(solid) => Some(Self::solid_hatch_model(solid, color)),
            _ => None,
        };
        let hatches = hatch.into_iter().collect::<Vec<_>>();
        for model in &hatches {
            if let Some(outline) = Self::hatch_model_outline_wire(target, model) {
                wires.push(outline);
            }
        }
        Some((wires, hatches))
    }

    /// Build wire models for an arbitrary slice of entities (e.g. clipboard contents).
    /// Entities need not be in the document — they are tessellated directly.
    pub fn wires_for_entities(&self, entities: &[codec::EntityType]) -> Vec<WireModel> {
        entities
            .iter()
            .flat_map(|e| self.tessellate_one(e))
            .collect()
    }

    pub fn set_interim_wire(&mut self, w: WireModel) {
        // Overlay wire — same reasoning as `set_preview_wires`: no geometry
        // bump, so the model isn't re-tessellated on every interim update.
        self.interim_wire = Some(w);
    }

    /// Tessellate one block definition's entities into block-local wire models
    /// (insertion base at origin) for the block-palette thumbnail. Nested INSERTs
    /// expand through the block cache; invisible members (another visibility
    /// state) are left out, as on the canvas. Unknown / empty block → `vec![]`.
    pub(crate) fn block_preview_wires(&self, name: &str) -> Vec<WireModel> {
        use codec::EntityType;
        let Some(br) = self.document.block_records.get(name) else {
            return vec![];
        };
        let mut out = Vec::new();
        for &eh in &br.entity_handles {
            let Some(e) = self.document.get_entity(eh) else {
                continue;
            };
            if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
                || e.common().invisible
            {
                continue;
            }
            out.extend(self.tessellate_one(e));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::Line;
    use codec::types::Vector3;
    use codec::EntityType;

    #[test]
    fn block_preview_wires_tessellates_block_entities() {
        let mut s = Scene::new();
        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 5.0, 0.0);
        s.define_block_from_owned_entities(
            vec![EntityType::Line(line)],
            "Widget",
            glam::DVec3::ZERO,
        )
        .unwrap();
        let wires = s.block_preview_wires("Widget");
        assert!(!wires.is_empty(), "a LINE block must tessellate to wires");
    }

    #[test]
    fn block_preview_wires_empty_for_unknown_block() {
        let s = Scene::new();
        assert!(s.block_preview_wires("Nope").is_empty());
    }
}
