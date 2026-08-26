// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

impl Scene {
    // ── Layout management ─────────────────────────────────────────────────

    pub(crate) fn model_pane_min_px(&self) -> f32 {
        let measured = f32::from_bits(
            self.model_pane_min_px
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        if measured.is_finite() && measured > 1.0 {
            measured
        } else {
            MODEL_PANE_MIN_FALLBACK_PX
        }
    }

    pub(crate) fn model_pane_min_reporter(
        &self,
    ) -> std::sync::Arc<std::sync::atomic::AtomicU32> {
        self.model_pane_min_px.clone()
    }

    /// Rename a paper-space layout.  Updates the Layout object name in the document.
    pub fn rename_layout(&mut self, old_name: &str, new_name: &str) {
        for obj in self.document.objects.values_mut() {
            if let ObjectType::Layout(l) = obj {
                if l.name == old_name {
                    l.name = new_name.to_string();
                    return;
                }
            }
        }
    }

    /// Delete a paper-space layout and all entities owned by it.
    /// Returns `false` if the layout was not found or is "Model".
    pub fn delete_layout(&mut self, name: &str) -> bool {
        if name == "Model" {
            return false;
        }

        let layout_info = self.document.objects.values().find_map(|obj| {
            if let ObjectType::Layout(l) = obj {
                if l.name == name {
                    return Some((l.handle, l.block_record));
                }
            }
            None
        });

        let (layout_handle, block_handle) = match layout_info {
            Some(info) => info,
            None => return false,
        };

        // Remove all entities that belong to this layout's block record.
        let to_remove: Vec<Handle> = self
            .document
            .entities()
            .filter(|e| e.common().owner_handle == block_handle)
            .map(|e| e.common().handle)
            .collect();
        for h in &to_remove {
            self.hatches.remove(h);
            self.meshes.remove(h);
            self.solid_models.remove(h);
            self.document.remove_entity(*h);
        }

        // Remove the Layout object itself.
        self.document.objects.remove(&layout_handle);

        // Drop the layout's dictionary entry so it does not dangle.
        let dict_handle = self.document.header.acad_layout_dict_handle;
        if let Some(ObjectType::Dictionary(d)) = self.document.objects.get_mut(&dict_handle) {
            d.entries.retain(|(k, _)| k != name);
        }

        // Remove the now-empty paper-space block record.
        let block_name = self
            .document
            .block_records
            .iter()
            .find(|b| b.handle == block_handle)
            .map(|b| b.name.clone());
        if let Some(bn) = block_name {
            self.document.block_records.remove(&bn);
        }

        // If the deleted layout was active, fall back to Model space.
        if self.current_layout == name {
            self.current_layout = "Model".to_string();
            self.sync_active_space_to_document();
        }

        self.bump_geometry();
        true
    }

    /// Swap the `tab_order` of two paper layouts so they appear in swapped order.
    pub fn swap_layout_order(&mut self, name_a: &str, name_b: &str) {
        let mut order_a: Option<i16> = None;
        let mut order_b: Option<i16> = None;
        for obj in self.document.objects.values() {
            if let ObjectType::Layout(l) = obj {
                if l.name == name_a {
                    order_a = Some(l.tab_order);
                }
                if l.name == name_b {
                    order_b = Some(l.tab_order);
                }
            }
        }
        if let (Some(oa), Some(ob)) = (order_a, order_b) {
            for obj in self.document.objects.values_mut() {
                if let ObjectType::Layout(l) = obj {
                    if l.name == name_a {
                        l.tab_order = ob;
                    } else if l.name == name_b {
                        l.tab_order = oa;
                    }
                }
            }
        }
    }

    /// Apply a complete paper-layout tab order. Model remains fixed at zero.
    pub fn set_layout_tab_order(&mut self, ordered_names: &[String]) {
        for obj in self.document.objects.values_mut() {
            if let ObjectType::Layout(layout) = obj {
                if layout.name == "Model" {
                    layout.tab_order = 0;
                } else if let Some(index) =
                    ordered_names.iter().position(|name| name == &layout.name)
                {
                    layout.tab_order = index as i16 + 1;
                }
            }
        }
    }

    /// Rebuild the `pane_grid` layout from the current `model_tiles` rects
    /// (each pane value = its tile index) — used after loading a tiled config
    /// from the VPort table so the panes match the restored tiles.
    pub fn rebuild_panes_from_tiles(&mut self) {
        let items: Vec<(usize, iced::Rectangle)> = self
            .model_tiles
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, t)| (i, t.rect))
            .collect();
        let full = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let config = if items.is_empty() {
            iced::widget::pane_grid::Configuration::Pane(0)
        } else {
            config_from_rects(&items, full)
        };
        self.model_panes = iced::widget::pane_grid::State::with_configuration(config);
    }

    /// Replace the Model pane layout with a `pane_grid` configuration (VPORTS
    /// presets). Each resulting pane inherits the active tile's camera / style;
    /// pane values are compacted to `0..N` and `model_tiles` rebuilt to match.
    pub fn set_model_panes(&mut self, config: iced::widget::pane_grid::Configuration<usize>) {
        let cam = self.camera.borrow().clone();
        let (mode, grid_on, snap_on) = {
            let tiles = self.model_tiles.borrow();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            tiles
                .get(active)
                .map(|t| (t.render_mode, t.grid_on, t.snap_on))
                .unwrap_or((
                    acadrust::entities::ViewportRenderMode::Wireframe2D,
                    false,
                    false,
                ))
        };
        self.model_panes = iced::widget::pane_grid::State::with_configuration(config);
        let order: Vec<iced::widget::pane_grid::Pane> =
            self.model_panes.iter().map(|(p, _)| *p).collect();
        let mut tiles = Vec::with_capacity(order.len());
        for (i, pane) in order.iter().enumerate() {
            if let Some(v) = self.model_panes.get_mut(*pane) {
                *v = i;
            }
            tiles.push(ModelTile {
                rect: iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                camera: cam.clone(),
                render_mode: mode,
                grid_on,
                snap_on,
            });
        }
        *self.model_tiles.borrow_mut() = tiles;
        self.active_model_tile.set(0);
    }

    /// Split the active Model pane through `pane_grid`, adding a new pane that
    /// inherits the active pane's camera / render-mode / grid. `horizontal`
    /// true → a horizontal divider (panes stacked); false → vertical (panes
    /// side by side). The active pane stays active.
    pub fn split_active_pane(&mut self, horizontal: bool) {
        use iced::widget::pane_grid::Axis;
        let active_idx = self.active_model_tile.get();
        // Keep the active tile's stored camera current before cloning it.
        if let Some(t) = self.model_tiles.borrow_mut().get_mut(active_idx) {
            t.camera = self.camera.borrow().clone();
        }
        let Some(active_pane) = self
            .model_panes
            .iter()
            .find(|(_, &v)| v == active_idx)
            .map(|(p, _)| *p)
        else {
            return;
        };
        let Some(clone) = self.model_tiles.borrow().get(active_idx).cloned() else {
            return;
        };
        let new_val = self.model_tiles.borrow().len();
        self.model_tiles.borrow_mut().push(clone);
        let axis = if horizontal {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        if self.model_panes.split(axis, active_pane, new_val).is_none() {
            self.model_tiles.borrow_mut().pop();
            return;
        }
        self.compact_panes(active_pane);
    }

    /// Close the active Model pane (no-op on the last pane); the sibling that
    /// absorbs the space becomes active and its camera goes live.
    pub fn close_active_pane(&mut self) {
        if self.model_panes.len() <= 1 {
            return;
        }
        let active_idx = self.active_model_tile.get();
        if let Some(t) = self.model_tiles.borrow_mut().get_mut(active_idx) {
            t.camera = self.camera.borrow().clone();
        }
        let Some(active_pane) = self
            .model_panes
            .iter()
            .find(|(_, &v)| v == active_idx)
            .map(|(p, _)| *p)
        else {
            return;
        };
        if let Some((_, sibling)) = self.model_panes.close(active_pane) {
            self.compact_panes(sibling);
            let cam = self
                .model_tiles
                .borrow()
                .get(self.active_model_tile.get())
                .map(|t| t.camera.clone());
            if let Some(c) = cam {
                *self.camera.borrow_mut() = c;
                self.projection_bounds_epoch.set(0);
            }
        }
    }

    /// Swap two Model panes (by tile index) in the `pane_grid` layout — each
    /// viewport moves to the other's position, keeping its own camera / style.
    pub fn swap_model_panes(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        let pa = self.model_panes.iter().find(|(_, &v)| v == a).map(|(p, _)| *p);
        let pb = self.model_panes.iter().find(|(_, &v)| v == b).map(|(p, _)| *p);
        if let (Some(pa), Some(pb)) = (pa, pb) {
            self.model_panes.swap(pa, pb);
        }
    }

    /// Renumber pane values to a compact `0..N` in layout order and rebuild
    /// `model_tiles` to match (preserving each pane's data), so the pane value
    /// is always a valid `model_tiles` index. `active_pane` becomes the new
    /// active tile.
    fn compact_panes(&mut self, active_pane: iced::widget::pane_grid::Pane) {
        let old = self.model_tiles.borrow().clone();
        let order: Vec<iced::widget::pane_grid::Pane> =
            self.model_panes.iter().map(|(p, _)| *p).collect();
        let fallback = old
            .first()
            .cloned()
            .unwrap_or_else(|| ModelTile {
                rect: iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                camera: self.camera.borrow().clone(),
                render_mode: acadrust::entities::ViewportRenderMode::Wireframe2D,
                grid_on: false,
                snap_on: false,
            });
        let mut new_tiles = Vec::with_capacity(order.len());
        let mut new_active = 0usize;
        for (new_idx, pane) in order.iter().enumerate() {
            let old_idx = *self.model_panes.get(*pane).unwrap_or(&0);
            new_tiles.push(old.get(old_idx).cloned().unwrap_or_else(|| fallback.clone()));
            if let Some(v) = self.model_panes.get_mut(*pane) {
                *v = new_idx;
            }
            if *pane == active_pane {
                new_active = new_idx;
            }
        }
        *self.model_tiles.borrow_mut() = new_tiles;
        self.active_model_tile.set(new_active);
    }

    /// Make Model tile `idx` active, stashing the live camera into the outgoing
    /// tile and loading the incoming one. Returns `true` if the active tile
    /// changed. The caller bumps `camera_generation` and syncs the display.
    pub fn set_active_model_tile(&self, idx: usize) -> bool {
        if self.current_layout != "Model" {
            return false;
        }
        let old = self.active_model_tile.get();
        if idx == old || idx >= self.model_tiles.borrow().len() {
            return false;
        }
        let incoming = {
            let mut tiles = self.model_tiles.borrow_mut();
            if let Some(t) = tiles.get_mut(old) {
                t.camera = self.camera.borrow().clone();
            }
            tiles.get(idx).map(|t| t.camera.clone())
        };
        if let Some(cam) = incoming {
            *self.camera.borrow_mut() = cam;
            self.projection_bounds_epoch.set(0);
        }
        self.active_model_tile.set(idx);
        true
    }

    /// Divider bars between Model panes, as pixel rectangles within a
    /// `(vw, vh)` canvas — derived from the `pane_grid` split regions so they
    /// land exactly in the spacing gaps the renderer leaves. Each bar is
    /// `TILE_DIVIDER_PX` thick. Empty outside Model / for a single pane.
    pub fn model_pane_dividers(&self, vw: f32, vh: f32) -> Vec<iced::Rectangle> {
        use iced::widget::pane_grid::Axis;
        if self.current_layout != "Model" || vw < 1.0 || vh < 1.0 {
            return vec![];
        }
        let half = TILE_DIVIDER_PX * 0.5;
        self.model_panes
            .layout()
            .split_regions(
                TILE_DIVIDER_PX,
                self.model_pane_min_px(),
                iced::Size::new(vw, vh),
            )
            .values()
            .map(|(axis, region, ratio)| match axis {
                Axis::Vertical => {
                    let x = region.x + ratio * region.width;
                    iced::Rectangle {
                        x: x - half,
                        y: region.y,
                        width: TILE_DIVIDER_PX,
                        height: region.height,
                    }
                }
                Axis::Horizontal => {
                    let y = region.y + ratio * region.height;
                    iced::Rectangle {
                        x: region.x,
                        y: y - half,
                        width: region.width,
                        height: TILE_DIVIDER_PX,
                    }
                }
            })
            .collect()
    }

    /// Top-left pixel corner of Model pane `idx` within a `(vw, vh)` canvas,
    /// derived from the `pane_grid` layout (so it matches the rendered pane).
    pub fn pane_origin_px(&self, idx: usize, vw: f32, vh: f32) -> iced::Point {
        if vw < 1.0 || vh < 1.0 {
            return iced::Point::ORIGIN;
        }
        let regions = self.model_panes.layout().pane_regions(
            TILE_DIVIDER_PX,
            self.model_pane_min_px(),
            iced::Size::new(vw, vh),
        );
        self.model_panes
            .iter()
            .find(|(_, &v)| v == idx)
            .and_then(|(pane, _)| regions.get(pane))
            .map(|r| iced::Point::new(r.x, r.y))
            .unwrap_or(iced::Point::ORIGIN)
    }

    /// Replace the Model tiled layout with the given normalized rectangles
    /// (each in 0..1). Every tile inherits the current camera; the first
    /// tile becomes active. Used by VPORTS presets and `reset_model_tiles`.
    /// Derive `model_tiles` rects from the `pane_grid` layout (the layout
    /// source of truth) for a canvas of `(canvas_w, canvas_h)` pixels — each
    /// pane's value is its `model_tiles` index. Called before the renderer /
    /// hit-test read tile rects so they track the live pane_grid layout and any
    /// in-flight resize, with the divider gap applied exactly as pane_grid draws
    /// it. No-op outside the Model layout.
    #[allow(dead_code)]
    pub fn sync_tiles_from_panes(&self, canvas_w: f32, canvas_h: f32) {
        if self.current_layout != "Model" || canvas_w < 1.0 || canvas_h < 1.0 {
            return;
        }
        let regions = self.model_panes.layout().pane_regions(
            TILE_DIVIDER_PX,
            self.model_pane_min_px(),
            iced::Size::new(canvas_w, canvas_h),
        );
        let mut tiles = self.model_tiles.borrow_mut();
        for (pane, &idx) in self.model_panes.iter() {
            if let (Some(r), Some(tile)) = (regions.get(pane), tiles.get_mut(idx)) {
                tile.rect = iced::Rectangle {
                    x: r.x / canvas_w,
                    y: r.y / canvas_h,
                    width: r.width / canvas_w,
                    height: r.height / canvas_h,
                };
            }
        }
    }


    /// Screen-pixel rectangle of the active Model tile within a canvas of
    /// `(vw, vh)`. Full canvas outside the Model layout or for a single
    /// tile. Used to map cursor coordinates into the active tile so pick /
    /// pan / ViewCube work per-pane in a tiled layout.
    /// Canvas bounds + camera for every Model tile whose grid display is on.
    /// Each pane renders its own grid independently of which tile is active or
    /// hovered, so the grid never flickers as the cursor crosses panes. The
    /// active tile uses the live camera (mid-orbit/pan); others use their
    /// stored camera. (#121)
    /// Screen rect + camera for every grid-on sub-view in the current layout —
    /// model tiles in model space, the sheet plus each floating viewport
    /// (clipped to its rectangle) in paper space. Derived from the same
    /// `active_viewports` enumeration the renderer uses, so the grid overlay can
    /// never drift from the views actually on screen (issue #121). The grid

    pub fn active_model_tile_bounds(&self, vw: f32, vh: f32) -> iced::Rectangle {
        if self.current_layout != "Model" {
            return iced::Rectangle { x: 0.0, y: 0.0, width: vw, height: vh };
        }
        let tiles = self.model_tiles.borrow();
        let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
        match tiles.get(active) {
            Some(t) => iced::Rectangle {
                x: t.rect.x * vw,
                y: t.rect.y * vh,
                width: (t.rect.width * vw).max(1.0),
                height: (t.rect.height * vh).max(1.0),
            },
            None => iced::Rectangle { x: 0.0, y: 0.0, width: vw, height: vh },
        }
    }
}


/// Reconstruct a `pane_grid` configuration from a set of (tile-index, rect)
/// items covering `region`, by recursively finding a full vertical or
/// horizontal guillotine cut. Non-guillotine input falls back to chained panes.
fn config_from_rects(
    items: &[(usize, iced::Rectangle)],
    region: iced::Rectangle,
) -> iced::widget::pane_grid::Configuration<usize> {
    use iced::widget::pane_grid::{Axis, Configuration as C};
    if items.len() == 1 {
        return C::Pane(items[0].0);
    }
    let eps = 1e-3;
    // Try a vertical cut: a boundary x where every rect is wholly left or right.
    let mut cuts: Vec<f32> = items.iter().map(|(_, r)| r.x + r.width).collect();
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    for &x in &cuts {
        if x <= region.x + eps || x >= region.x + region.width - eps {
            continue;
        }
        let split_ok = items
            .iter()
            .all(|(_, r)| r.x + r.width <= x + eps || r.x >= x - eps);
        if !split_ok {
            continue;
        }
        let (a, b): (Vec<_>, Vec<_>) = items
            .iter()
            .cloned()
            .partition(|(_, r)| r.x + r.width <= x + eps);
        if a.is_empty() || b.is_empty() {
            continue;
        }
        let ra = iced::Rectangle {
            width: x - region.x,
            ..region
        };
        let rb = iced::Rectangle {
            x,
            width: region.x + region.width - x,
            ..region
        };
        return C::Split {
            axis: Axis::Vertical,
            ratio: ((x - region.x) / region.width).clamp(0.05, 0.95),
            a: Box::new(config_from_rects(&a, ra)),
            b: Box::new(config_from_rects(&b, rb)),
        };
    }
    // Try a horizontal cut: a boundary y where every rect is wholly above/below.
    let mut cuts: Vec<f32> = items.iter().map(|(_, r)| r.y + r.height).collect();
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    for &y in &cuts {
        if y <= region.y + eps || y >= region.y + region.height - eps {
            continue;
        }
        let split_ok = items
            .iter()
            .all(|(_, r)| r.y + r.height <= y + eps || r.y >= y - eps);
        if !split_ok {
            continue;
        }
        let (a, b): (Vec<_>, Vec<_>) = items
            .iter()
            .cloned()
            .partition(|(_, r)| r.y + r.height <= y + eps);
        if a.is_empty() || b.is_empty() {
            continue;
        }
        let ra = iced::Rectangle {
            height: y - region.y,
            ..region
        };
        let rb = iced::Rectangle {
            y,
            height: region.y + region.height - y,
            ..region
        };
        return C::Split {
            axis: Axis::Horizontal,
            ratio: ((y - region.y) / region.height).clamp(0.05, 0.95),
            a: Box::new(config_from_rects(&a, ra)),
            b: Box::new(config_from_rects(&b, rb)),
        };
    }
    // Non-guillotine fallback: peel the first pane off the rest.
    let (first, rest) = items.split_first().unwrap();
    C::Split {
        axis: Axis::Vertical,
        ratio: 0.5,
        a: Box::new(C::Pane(first.0)),
        b: Box::new(config_from_rects(rest, region)),
    }
}
