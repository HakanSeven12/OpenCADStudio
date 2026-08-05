// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;
use crate::command::{AreaPreviewRegion, AreaPreviewSource};
use crate::scene::model::hatch_model::HatchPattern;

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
                    if let Some(model) = Self::area_preview_hatch(
                        std::slice::from_ref(boundary),
                        region.subtract,
                    ) {
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

        let direct_boundary = crate::scene::project::clip_boundary_polygon_for_document(
            &self.document,
            handle,
            0.0,
        );
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
            let min_x = ring.iter().map(|point| point[0]).fold(f64::INFINITY, f64::min);
            let max_x = ring
                .iter()
                .map(|point| point[0])
                .fold(f64::NEG_INFINITY, f64::max);
            let min_y = ring.iter().map(|point| point[1]).fold(f64::INFINITY, f64::min);
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
            world_origin: origin,
            boundary: std::sync::Arc::new(boundary),
            boundary_wcs: None,
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
            if let Some(background) = crate::entities::hatch::background_color(hatch) {
                let mut backdrop = model.clone();
                backdrop.pattern =
                    crate::scene::model::hatch_model::HatchPattern::Solid;
                let (background_color, background_aci) = match background {
                    acadrust::types::Color::ByLayer => {
                        let layer = self.document.layers.get(&hatch.common.layer);
                        let aci = layer
                            .and_then(|layer| match &layer.color {
                                acadrust::types::Color::Index(index) => Some(*index),
                                _ => None,
                            })
                            .unwrap_or(0);
                        (
                            crate::scene::view::render::layer_render_style(
                                &self.document,
                                &hatch.common.layer,
                            )
                            .color,
                            aci,
                        )
                    }
                    acadrust::types::Color::ByBlock => (style.0, style.4),
                    acadrust::types::Color::Index(index) => (
                        crate::scene::convert::tess_util::aci_to_rgba(
                            &acadrust::types::Color::Index(index),
                        ),
                        index,
                    ),
                    other => (
                        crate::scene::convert::tess_util::aci_to_rgba(&other),
                        0,
                    ),
                };
                backdrop.color = background_color;
                backdrop.aci = background_aci;
                backdrop.name = "SOLID".into();
                models.push(backdrop);
            }
        }
        models.push(model);
    }

    pub fn set_preview_text(&mut self, verts: Vec<crate::scene::pipeline::text_gpu::TextVertex>) {
        // Overlay glyphs — same reasoning as `set_preview_wires`: no geometry
        // bump. Uploaded to a dedicated per-frame text buffer in `prepare`.
        self.preview_text = verts;
    }

    pub fn clear_preview_wire(&mut self) {
        // No geometry bump — see `set_preview_wires`. Dropping the overlay
        // flips the wire content id back to the base tessellation id, which
        // re-uploads the base wires (without the preview) on the next frame.
        self.preview_wires = vec![];
        if !self.preview_hatches.is_empty() {
            self.preview_hatches = std::sync::Arc::new(Vec::new());
        }
        self.preview_text = vec![];
        self.interim_wire = None;
        // Drop any point-picked window marquee (STRETCH) so it doesn't linger
        // after the command ends before the next mouse move. (#291)
        self.selection.borrow_mut().preview_box = None;
    }

    pub fn wire_models_for(&self, handles: &[acadrust::Handle]) -> Vec<WireModel> {
        handles
            .iter()
            .flat_map(|h| {
                match self.document.get_entity(*h) {
                    // Hatches carry no outline in the normal wire set, but an
                    // edit preview (move / copy / array / grip-drag) needs to
                    // show the shape following the cursor. Build a live boundary
                    // from the current HatchModel — `apply_grip` keeps it in
                    // step, so the preview tracks a dragged grip in real time.
                    Some(EntityType::Hatch(_)) => {
                        self.hatch_outline_wire(*h).into_iter().collect()
                    }
                    Some(e) => self.tessellate_one(e),
                    None => Vec::new(),
                }
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

    /// Boundary outline wire for a hatch, reconstructed from its cached
    /// `HatchModel` (offsets from `world_origin`). Used only for edit previews —
    /// the normal render shows the fill, not this outline.
    fn hatch_outline_wire(&self, handle: Handle) -> Option<WireModel> {
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
        Some(WireModel::solid_f64(
            handle.value().to_string(),
            pts,
            m.color,
            false,
        ))
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

    /// Build wire models for an arbitrary slice of entities (e.g. clipboard contents).
    /// Entities need not be in the document — they are tessellated directly.
    pub fn wires_for_entities(&self, entities: &[acadrust::EntityType]) -> Vec<WireModel> {
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
    /// expand through the block cache. Unknown / empty block → `vec![]`.
    pub(crate) fn block_preview_wires(&self, name: &str) -> Vec<WireModel> {
        use acadrust::EntityType;
        let Some(br) = self.document.block_records.get(name) else {
            return vec![];
        };
        let mut out = Vec::new();
        for &eh in &br.entity_handles {
            let Some(e) = self.document.get_entity(eh) else {
                continue;
            };
            if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
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
    use acadrust::entities::Line;
    use acadrust::types::Vector3;
    use acadrust::EntityType;

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
