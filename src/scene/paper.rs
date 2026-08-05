// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

impl Scene {
    pub(super) fn paper_viewport_handles(
        &self,
    ) -> (Handle, Handle, Arc<Vec<Handle>>) {
        {
            let cache = self.paper_viewport_cache.borrow();
            if let Some(cache) = cache.get(&self.current_layout) {
                if cache.epoch == self.geometry_epoch && cache.layout == self.current_layout {
                    return (
                        cache.layout_block,
                        cache.sheet,
                        Arc::clone(&cache.content),
                    );
                }
            }
        }

        let layout_block = self.current_layout_block_handle();
        let sheet = self.current_layout_sheet_viewport_handle();
        let paper_limits = self.paper_limits();
        let is_content = |handle: Handle| {
            let Some(EntityType::Viewport(vp)) = self.document.get_entity(handle) else {
                return false;
            };
            vp.common.owner_handle == layout_block
                && if sheet.is_valid() {
                    handle != sheet
                } else {
                    Self::is_content_viewport(vp)
                }
        };
        let content = if let Some(block) = self
            .document
            .block_records
            .iter()
            .find(|block| block.handle == layout_block)
            .filter(|block| !block.entity_handles.is_empty())
        {
            block
                .entity_handles
                .iter()
                .copied()
                .filter(|handle| is_content(*handle))
                .collect()
        } else {
            self.document
                .entities()
                .filter_map(|entity| {
                    let handle = entity.common().handle;
                    is_content(handle).then_some(handle)
                })
                .collect()
        };
        let content = Arc::new(content);
        self.paper_viewport_cache.borrow_mut().insert(
            self.current_layout.clone(),
            PaperViewportCache {
                epoch: self.geometry_epoch,
                layout: self.current_layout.clone(),
                layout_block,
                sheet,
                content: Arc::clone(&content),
                paper_limits,
            },
        );
        (layout_block, sheet, content)
    }

    pub fn grid_views(&self, vw: f32, vh: f32) -> Vec<(iced::Rectangle, Camera, Handle)> {
        self.active_viewports(vw, vh, acadrust::entities::ViewportRenderMode::Wireframe2D)
            .into_iter()
            .filter(|inst| inst.grid_on)
            .map(|inst| (inst.screen_rect, inst.camera, inst.handle))
            .collect()
    }
    /// The viewports to render this frame, one entry per scissor pass.
    ///
    /// - **Model layout**: a single full-canvas instance driven by the
    ///   scene camera (tiled splits will append more later). `model_mode`
    ///   supplies its render mode (held on the tab, not the scene).
    /// - **Paper layout**: one instance per content viewport entity
    ///   (`id > 1`, owned by the current layout block, switched on), using each
    ///   viewport's own camera and render mode. `model_mode` temporarily
    ///   overrides the active viewport so the visual-style gallery can preview
    ///   on hover without modifying the document.
    pub fn active_viewports(
        &self,
        canvas_w: f32,
        canvas_h: f32,
        model_mode: acadrust::entities::ViewportRenderMode,
    ) -> Vec<ViewportInstance> {
        if self.current_layout == "Model" {
            let tiles = self.model_tiles.borrow();
            let active = self.active_model_tile.get().min(tiles.len().saturating_sub(1));
            return tiles
                .iter()
                .enumerate()
                .map(|(i, tile)| {
                    // The active tile renders the live camera (orbit/pan act
                    // on it); inactive tiles use their stored snapshot.
                    let camera = if i == active {
                        self.camera.borrow().clone()
                    } else {
                        tile.camera.clone()
                    };
                    ViewportInstance {
                        handle: Handle::NULL,
                        tile_idx: Some(i),
                        screen_rect: iced::Rectangle {
                            x: tile.rect.x * canvas_w,
                            y: tile.rect.y * canvas_h,
                            width: tile.rect.width * canvas_w,
                            height: tile.rect.height * canvas_h,
                        },
                        camera,
                        // The active tile shows the live mode the picker
                        // drives; every other tile keeps its own stored
                        // style so editing one never disturbs the rest.
                        render_mode: if i == active { model_mode } else { tile.render_mode },
                        active: i == active,
                        grid_on: tile.grid_on,
                        paper_sheet: false,
                    }
                })
                .collect();
        }
        let (_, sheet_handle, content_handles) = self.paper_viewport_handles();
        let mut out: Vec<ViewportInstance> = Vec::new();
        // The full-canvas sheet viewport renders the paper-space entities
        // themselves — the layout's own view, drawn first so the floating
        // content viewports overlay it. Its camera keeps the paper pan/zoom
        // (target + ortho size) but is LOCKED to the top/plan orientation:
        // paper is 2-D, so the sheet never orbits.
        let mut sheet_cam = self.camera.borrow().clone();
        sheet_cam.yaw = 0.0;
        sheet_cam.pitch = std::f32::consts::FRAC_PI_2;
        sheet_cam.rotation = view::camera::yaw_pitch_to_quat(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        sheet_cam.projection = view::camera::Projection::Orthographic;
        let sheet_grid_on = match self
            .document
            .get_entity(sheet_handle)
        {
            Some(EntityType::Viewport(vp)) => vp.status.grid_on,
            _ => false,
        };
        out.push(ViewportInstance {
            handle: Handle::NULL,
            tile_idx: None,
            screen_rect: iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: canvas_w,
                height: canvas_h,
            },
            camera: sheet_cam,
            render_mode: acadrust::entities::ViewportRenderMode::Wireframe2D,
            active: false,
            grid_on: sheet_grid_on,
            paper_sheet: true,
        });
        for &handle in content_handles.iter() {
            let Some(EntityType::Viewport(vp)) = self.document.get_entity(handle) else {
                continue;
            };
            if !vp.status.is_on
                || vp.common.invisible
                || self.entity_temporarily_hidden(handle)
                || self.layer_hidden(&vp.common.layer)
            {
                continue;
            }
            let h = vp.common.handle;
            let (Some(screen_rect), Some(camera)) = (
                self.viewport_screen_rect(h, (canvas_w, canvas_h)),
                self.camera_for_viewport(h),
            ) else {
                continue;
            };
            out.push(ViewportInstance {
                handle: h,
                tile_idx: None,
                screen_rect,
                camera,
                render_mode: if self.active_viewport == Some(h) {
                    model_mode
                } else {
                    vp.render_mode
                },
                active: self.active_viewport == Some(h),
                grid_on: vp.status.grid_on,
                paper_sheet: false,
            });
        }
        out
    }

    pub(super) fn paper_sheet_render_models(
        &self,
    ) -> (
        Arc<Vec<HatchModel>>,
        Arc<Vec<HatchModel>>,
        Arc<Vec<ImageModel>>,
    ) {
        let selected = self.selected_hatch_sig();
        let reuse = {
            let cache = self.paper_sheet_render_cache.borrow();
            if let Some(cache) = cache.get(&self.current_layout) {
                if cache.layout == self.current_layout
                    && cache.selected == selected
                    && cache.paper_bg == self.paper_bg_color
                    && self.category_cache_valid(
                        cache.epoch,
                        super::CACHE_CATEGORY_HATCH,
                        |handle| self.hatches.contains_key(&handle),
                    )
                    && self.category_cache_valid(
                        cache.epoch,
                        super::CACHE_CATEGORY_WIPEOUT,
                        |handle| {
                            matches!(
                                self.document.get_entity(handle),
                                Some(EntityType::Wipeout(_))
                            )
                        },
                    )
                    && self.category_cache_valid(
                        cache.epoch,
                        super::CACHE_CATEGORY_IMAGE,
                        |handle| self.images.contains_key(&handle),
                    )
                {
                    Some((
                        Arc::clone(&cache.hatches),
                        Arc::clone(&cache.wipeouts),
                        Arc::clone(&cache.images),
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(models) = reuse {
            if let Some(cache) = self
                .paper_sheet_render_cache
                .borrow_mut()
                .get_mut(&self.current_layout)
            {
                cache.epoch = self.geometry_epoch;
            }
            return models;
        }

        let mut hatches = Vec::new();
        if let Some(sheet) = self.paper_sheet_fill() {
            hatches.push(sheet);
        }
        hatches.extend(self.paper_canvas_hatches().iter().cloned());
        let hatches = Arc::new(hatches);
        let wipeouts = self.paper_canvas_wipeouts();
        let images = self.paper_sheet_images();
        self.paper_sheet_render_cache.borrow_mut().insert(
            self.current_layout.clone(),
            PaperSheetRenderCache {
                epoch: self.geometry_epoch,
                layout: self.current_layout.clone(),
                selected,
                paper_bg: self.paper_bg_color,
                hatches: Arc::clone(&hatches),
                wipeouts: Arc::clone(&wipeouts),
                images: Arc::clone(&images),
            },
        );
        (hatches, wipeouts, images)
    }

    /// Convert a paper-space Viewport entity's position/size into a pixel
    /// `Rectangle` relative to the top-left of the canvas.
    ///
    /// Uses the same top-down ortho transform as the GPU sheet viewport so the
    /// overlay lands exactly over the drawn viewport border regardless of zoom
    /// or pan level.
    pub fn viewport_screen_rect(
        &self,
        vp_handle: Handle,
        canvas_px: (f32, f32),
    ) -> Option<iced::Rectangle> {
        let vp = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => vp,
            _ => return None,
        };

        let (canvas_w, canvas_h) = canvas_px;
        if canvas_w < 1.0 || canvas_h < 1.0 {
            return None;
        }

        let cam = self.camera.borrow();
        let aspect = canvas_w / canvas_h;
        let half_h = cam.ortho_size();
        let half_w = half_h * aspect;
        let tx = cam.target.x as f32;
        let ty = cam.target.y as f32;
        drop(cam);

        // Top-down ortho mapping matching the GPU sheet viewport's camera.
        let to_px = |wx: f32, wy: f32| -> (f32, f32) {
            let x = (wx - tx + half_w) / (2.0 * half_w) * canvas_w;
            let y = (ty + half_h - wy) / (2.0 * half_h) * canvas_h;
            (x, y)
        };

        let cx = vp.center.x as f32;
        let cy = vp.center.y as f32;
        let hw = (vp.width / 2.0) as f32;
        let hh = (vp.height / 2.0) as f32;

        let (x0, y0) = to_px(cx - hw, cy + hh); // top-left in screen
        let (x1, y1) = to_px(cx + hw, cy - hh); // bottom-right in screen

        let w = (x1 - x0).max(1.0);
        let h = (y1 - y0).max(1.0);

        Some(iced::Rectangle {
            x: x0,
            y: y0,
            width: w,
            height: h,
        })
    }

    /// Physical sheet bounds in canvas pixels. Uses the same forced top-down
    /// paper transform as the GPU sheet viewport, ignoring stored camera twist.
    pub fn paper_sheet_screen_rect(
        &self,
        canvas_px: (f32, f32),
    ) -> Option<iced::Rectangle> {
        let ((x0, y0), (x1, y1)) = self.paper_limits()?;
        let (canvas_w, canvas_h) = canvas_px;
        if canvas_w < 1.0 || canvas_h < 1.0 {
            return None;
        }

        let cam = self.camera.borrow();
        let half_h = cam.ortho_size();
        let half_w = half_h * canvas_w / canvas_h;
        let tx = cam.target.x as f32;
        let ty = cam.target.y as f32;
        drop(cam);
        let to_px = |wx: f32, wy: f32| -> (f32, f32) {
            let x = (wx - tx + half_w) / (2.0 * half_w) * canvas_w;
            let y = (ty + half_h - wy) / (2.0 * half_h) * canvas_h;
            (x, y)
        };
        let min_x = x0.min(x1) as f32;
        let max_x = x0.max(x1) as f32;
        let min_y = y0.min(y1) as f32;
        let max_y = y0.max(y1) as f32;
        let (left, top) = to_px(min_x, max_y);
        let (right, bottom) = to_px(max_x, min_y);
        Some(iced::Rectangle {
            x: left,
            y: top,
            width: (right - left).max(0.0),
            height: (bottom - top).max(0.0),
        })
    }
    // ── Paper-space helpers ───────────────────────────────────────────────

    /// Paper-layout hatch fills, restricted to the active layout block (used by
    /// paper-space hatch hit-testing / export). The GPU-rendered
    /// content viewports already draw model-block hatches inside their
    /// own scissor; including those here would also draw them on the
    /// paper sheet through the paper camera (huge / off-position), so
    /// restrict the canvas list to entities owned by the active paper
    /// layout block. Iterates the source `self.hatches` map (keyed by
    /// entity handle) rather than the already-flattened arc — the
    /// flattened arc carries pattern names, not handles, so filtering
    /// there is unreliable.
    pub fn paper_canvas_hatches(&self) -> Arc<Vec<HatchModel>> {
        let layout_block = self.current_layout_block_handle();
        let layer_hidden = |layer: &str| {
            self.document
                .layers
                .get(layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
        };
        let mut models: Vec<HatchModel> = Vec::new();
        let annotation_scale_handle = self.paper_annotation_scale_handle();
        let all_visible = self.annotation_all_visible();
        for (&handle, model) in self.hatches.iter() {
            let Some(source) = self.document.get_entity(handle) else {
                continue;
            };
            let contextual = crate::scene::annotative::entity_for_annotation_context(
                &self.document,
                source,
                annotation_scale_handle,
            );
            let entity = contextual.as_ref();
            // Paper-space SOLIDs already carry WCS-aware wire fill triangles.
            // Keep their cached XY HatchModel out of the sheet set so the same
            // entity is not emitted twice (#617). Model fills projected through
            // floating viewports still use `plot_hatches_for_block` below.
            if matches!(entity, EntityType::Solid(_)) {
                continue;
            }
            let c = entity.common();
            if c.invisible
                || self.entity_temporarily_hidden(handle)
                || layer_hidden(&c.layer)
                || crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    c,
                    annotation_scale_handle,
                    all_visible,
                )
            {
                continue;
            }
            if !self.belongs_to_visible_block(handle, c.owner_handle, layout_block) {
                continue;
            }
            let mut m = match entity {
                EntityType::Hatch(dxf)
                    if crate::scene::annotative::active_object_context_for_scale(
                        &self.document,
                        handle,
                        annotation_scale_handle,
                    )
                    .is_some() =>
                {
                    Self::hatch_model_from_dxf(dxf, model.color)
                        .unwrap_or_else(|| model.clone())
                }
                _ => model.clone(),
            };
            let style = self.render_style(entity);
            m.color = style.0;
            m.aci = style.4;
            m.line_weight_px = style.3;
            if let EntityType::Hatch(dxf) = entity {
                // Only re-apply pattern_scale/angle for catalog-derived patterns
                // (empty stored lines). A pattern built from the hatch's own
                // stored lines is already final (scale 1 / angle 0).
                if let model::hatch_model::HatchPattern::Pattern(_) = &m.pattern {
                    if dxf.pattern.lines.is_empty() {
                        m.angle_offset = dxf.pattern_angle as f32;
                        m.scale = dxf.pattern_scale as f32;
                    }
                }
            }
            if self.selected.contains(&handle) {
                m.color = [0.15, 0.55, 1.00, m.color[3]];
            }
            models.push(m);
        }
        // Hatch fills nested inside a block INSERT are owned by the block
        // record, so the loop above — which only keeps hatches owned by
        // `layout_block` — never sees them. Walk the layout's visible block
        // instances and materialize their fills at world position, so export
        // carries the block's colours instead of bare outlines.
        let hatch_bg = if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        };
        let instanced = self.instanced_hatch_models(
            layout_block,
            hatch_bg,
            false,
            None,
            annotation_scale_handle,
            all_visible,
            None,
        );
        models.extend(instanced);
        Arc::new(models)
    }

    /// Plot-only hatch set for a specific block. Paper PDF generation uses this
    /// for the model block behind each floating viewport; unlike
    /// `paper_canvas_hatches`, it must not include the active paper block.
    pub(super) fn plot_hatches_for_block(
        &self,
        block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
    ) -> Vec<HatchModel> {
        let layer_hidden = |layer: &str| {
            self.document
                .layers
                .get(layer)
                .map(|l| l.flags.off || l.flags.frozen)
                .unwrap_or(false)
        };
        let mut models = Vec::new();
        for (&handle, model) in self.hatches.iter() {
            let Some(source) = self.document.get_entity(handle) else {
                continue;
            };
            let contextual = crate::scene::annotative::entity_for_annotation_context(
                &self.document,
                source,
                annotation_scale_handle,
            );
            let entity = contextual.as_ref();
            let common = entity.common();
            if common.invisible
                || self.entity_temporarily_hidden(handle)
                || layer_hidden(&common.layer)
                || self.layer_frozen_in(&common.layer, frozen)
                || crate::scene::annotative::annotative_offscale_for(
                    &self.document,
                    common,
                    annotation_scale_handle,
                    all_visible,
                )
                || !self.belongs_to_visible_block(handle, common.owner_handle, block)
            {
                continue;
            }
            let mut hatch = match entity {
                EntityType::Hatch(dxf)
                    if crate::scene::annotative::active_object_context_for_scale(
                        &self.document,
                        handle,
                        annotation_scale_handle,
                    )
                    .is_some() =>
                {
                    Self::hatch_model_from_dxf(dxf, model.color)
                        .unwrap_or_else(|| model.clone())
                }
                _ => model.clone(),
            };
            let style = self.render_style(entity);
            hatch.color = style.0;
            hatch.aci = style.4;
            hatch.line_weight_px = style.3;
            if let EntityType::Hatch(dxf) = entity {
                if let model::hatch_model::HatchPattern::Pattern(_) = &hatch.pattern {
                    if dxf.pattern.lines.is_empty() {
                        hatch.angle_offset = dxf.pattern_angle as f32;
                        hatch.scale = dxf.pattern_scale as f32;
                    }
                }
            }
            models.push(hatch);
        }
        models.extend(self.instanced_hatch_models(
            block,
            self.paper_bg_color,
            false,
            frozen,
            annotation_scale_handle,
            all_visible,
            None,
        ));
        models
    }

    /// Plot-only wipeout masks for a specific block. Nested instances follow
    /// the same scene graph as the on-screen model path.
    pub(super) fn plot_wipeouts_for_block(
        &self,
        block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        highlight_selection: bool,
    ) -> Vec<HatchModel> {
        self.wipeout_models_for_block_graph(
            block,
            frozen,
            annotation_scale_handle,
            all_visible,
            self.paper_bg_color,
            highlight_selection,
        )
    }

    /// Paper-layout wipeout fills (paper hit-testing / export). Same rationale as
    /// `paper_canvas_hatches` — only include wipeouts owned by the
    /// active paper layout block, so model wipeouts (drawn through their
    /// content viewport's GPU pipeline) don't get a second mis-projected
    /// copy on the paper sheet.
    pub fn paper_canvas_wipeouts(&self) -> Arc<Vec<HatchModel>> {
        let layout_block = self.current_layout_block_handle();
        Arc::new(self.plot_wipeouts_for_block(
            layout_block,
            None,
            self.paper_annotation_scale_handle(),
            self.annotation_all_visible(),
            true,
        ))
    }

    /// Build a Camera oriented and scaled to match a paper-space Viewport entity.
    /// Used by `active_viewports` to render model-space content through each
    /// content viewport's own view direction and scale.
    pub(super) fn camera_for_viewport(&self, vp_handle: Handle) -> Option<view::camera::Camera> {
        let vp = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => vp,
            _ => return None,
        };

        // Floating-viewport–specific step: decide saved-view vs auto-fit, then
        // hand the effective view to the shared `camera_from_view` decoder so
        // twist / view_center / distance behave identically to a model VPORT.
        //
        // UTM / coordinate-shifted drawings often arrive with
        // `view_target = (0, 0, 0)` and a stale `view_center` from before the
        // file was geo-referenced; the saved view points at empty WCS while the
        // actual model sits ~`world_offset` away. Decode the saved view first
        // and keep it only if its target actually frames the model cluster.
        //
        // The overlap test runs on the *decoded* target (wire-space, so the
        // cluster is `±cluster_half` about the origin), NOT a raw
        // `view_target + view_center` sum: under a view twist `view_center` is a
        // DCS offset, so the raw sum lands far from the real WCS centre and
        // would wrongly trip the auto-fit — replacing the saved view_height with
        // the whole-cluster fit and rendering the content at the wrong zoom.
        let saved_h = vp.view_height.abs();
        let aspect_d = (vp.width / vp.height.max(1.0)).max(1e-9);
        let cluster_half = self.local_extent_max.max(1.0) as f64;
        // Full model-space bounds. The overlap test below accepts the saved view
        // when its frame touches ANY drawn geometry — not just the dense median
        // cluster (`local_center ± cluster_half`). A drawing with a second,
        // sparser cluster (e.g. model-documentation view geometry sitting apart
        // from a big symbol library) has viewports legitimately aimed at that
        // second cluster; testing only the dense one wrongly auto-fits them onto
        // the library. Fall back to the cluster box when no extents are known.
        let full_bounds = self.model_space_extents().map(|(mn, mx)| {
            (mn.x as f64, mn.y as f64, mx.x as f64, mx.y as f64)
        });
        // Absolute drawing centre. Geometry now reaches the scene at absolute
        // (UTM) coordinates — the old code centred the overlap test and the
        // auto-fit on the origin, which was right only while world_offset
        // re-centred the model there. Without it a UTM drawing sits ~5.7e6 away,
        // so a stale `(0,0,0)` saved view failed the overlap test AND the
        // auto-fit aimed at empty origin → blank viewports.
        // Frame the overlap test / auto-fit on the robust cluster centre (median
        // of entity centroids), NOT the raw extents centre: a drawing with a
        // far second cluster (e.g. a small-coordinate legend beside a UTM survey)
        // has an extents centre in the empty gap, which would reject a valid
        // saved view and then auto-fit onto blank space. Fall back to the extents
        // centre only when no cluster centre was computed.
        let (cx, cy) = if self.local_center != [0.0, 0.0] {
            (self.local_center[0], self.local_center[1])
        } else {
            self.model_space_extents()
                .map(|(mn, mx)| {
                    (((mn.x + mx.x) * 0.5) as f64, ((mn.y + mx.y) * 0.5) as f64)
                })
                .unwrap_or((0.0, 0.0))
        };

        // A non-zero `view_target` is a deliberately-aimed saved view (a model
        // documentation drawing view, a detail/section viewport, any viewport
        // panned onto a specific WCS point). It is authoritative — use it as-is.
        // The overlap test / auto-fit below only rescues the STALE default,
        // where `view_target == (0,0,0)` points at empty WCS while the model
        // sits at UTM. Guarding on the target avoids the tight median cluster
        // (which collapses onto the densest sub-cluster) wrongly rejecting a
        // valid view that frames a smaller, off-centre sub-cluster.
        let target_set =
            vp.view_target.x.abs() > 1e-6 || vp.view_target.y.abs() > 1e-6;
        // A fully-uninitialised view (target AND centre both zero) is a stale
        // placeholder viewport — frame its saved view (the origin) and leave it
        // empty rather than auto-fitting the whole model into it. The auto-fit
        // rescue is meant only for a stale target=(0,0,0) paired with a NON-zero
        // (pre-georeference) view_centre that points at empty WCS while the model
        // sits far away — that case still falls through to the overlap test.
        let center_set =
            vp.view_center.x.abs() > 1e-6 || vp.view_center.y.abs() > 1e-6;

        if let Some(cam) = self.camera_from_view_mode(
            vp.view_direction,
            vp.view_target,
            acadrust::types::Vector2 {
                x: vp.view_center.x,
                y: vp.view_center.y,
            },
            saved_h,
            vp.twist_angle,
            vp.status.perspective,
            vp.lens_length,
        ) {
            if target_set || !center_set {
                return Some(cam);
            }
            let half_h = saved_h * 0.5;
            let half_w = half_h * aspect_d;
            let (tx, ty) = (cam.target.x as f64, cam.target.y as f64);
            // Prefer the true model bounds; fall back to the median cluster box.
            let (bx0, by0, bx1, by1) = full_bounds.unwrap_or((
                cx - cluster_half,
                cy - cluster_half,
                cx + cluster_half,
                cy + cluster_half,
            ));
            let overlaps = tx + half_w >= bx0
                && tx - half_w <= bx1
                && ty + half_h >= by0
                && ty - half_h <= by1;
            if overlaps {
                return Some(cam);
            }
        }

        // Auto-fit: aim at the content cluster centre, drop the stale view_center.
        let fit_h = cluster_half * 2.0 * 1.05;
        let tgt = acadrust::types::Vector3 {
            x: cx,
            y: cy,
            z: vp.view_target.z,
        };
        self.camera_from_view_mode(
            vp.view_direction,
            tgt,
            acadrust::types::Vector2::ZERO,
            fit_h,
            vp.twist_angle,
            vp.status.perspective,
            vp.lens_length,
        )
    }

    /// Collect model-space WireModels visible through `vp_handle`, respecting
    /// global layer visibility, the viewport's per-viewport layer freeze list,
    /// and the per-viewport frustum + LOD cull derived from
    /// `screen_height_px` (the on-paper pixel height of this viewport).
    fn model_wires_for_viewport(
        &self,
        vp_handle: Handle,
        _screen_height_px: f32,
    ) -> Arc<Vec<WireModel>> {
        use rustc_hash::FxHashSet as HSet;

        // The viewport's frozen-layer set is the only resident-geometry input.
        // Its live zoom is camera magnification, not CANNOSCALE: tying
        // annotation geometry to view_height rebuilt the entire model on every
        // wheel tick whenever the drawing contained one annotative object.
        // Explicit viewport annotation-scale changes still rebuild the resident
        // set; PSLTSCALE is a viewport GPU uniform.
        let frozen = match self.document.get_entity(vp_handle) {
            Some(EntityType::Viewport(vp)) => {
                let f: HSet<Handle> = vp.frozen_layers.iter().cloned().collect();
                f
            }
            _ => HSet::default(),
        };

        let scale_handle = self.viewport_scale_handle(vp_handle);
        self.resident_wires_for(
            self.model_space_block_handle(),
            Some(self.viewport_annotation_multiplier(vp_handle)),
            scale_handle,
            Some(&frozen),
            Some(vp_handle),
        )
    }

    /// Resident model wires for a paper content viewport. Just the unified
    /// static-hold (`resident_wires_for`) keyed on the viewport's frozen set +
    /// explicit CANNOSCALE — no per-viewport height/view cache: the set is
    /// camera-independent, so paper zoom and MSPACE zoom reuse it as-is.
    pub(crate) fn model_wires_for_viewport_arc(
        &self,
        vp_handle: Handle,
        screen_height_px: f32,
    ) -> Arc<Vec<WireModel>> {
        self.model_wires_for_viewport(vp_handle, screen_height_px)
    }
}
