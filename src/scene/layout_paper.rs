use super::*;

impl Scene {
    pub(crate) fn layout_sheet_viewport_handle(
        document: &document::Document,
        layout: &document::objects::Layout,
    ) -> Handle {
        let owned_viewport = |handle| match document.get_entity(handle) {
            Some(EntityType::Viewport(vp)) if vp.common.owner_handle == layout.block_record => {
                Some(vp)
            }
            _ => None,
        };

        let block_handles = document
            .block_records
            .iter()
            .find(|block| block.handle == layout.block_record)
            .map(|block| block.entity_handles.as_slice())
            .unwrap_or_default();

        // A layout whose viewports are all numbered 2 and up has no sheet yet:
        // they are content viewports added (e.g. by a script) before the layout
        // was first opened, and opening it must still create its sheet. Only
        // the layout's own lists are scanned: this runs every frame.
        let mut owned_ids = layout
            .viewports
            .iter()
            .chain(block_handles)
            .filter_map(|&handle| owned_viewport(handle).map(|vp| vp.id))
            .peekable();
        if owned_ids.peek().is_some() && owned_ids.all(|id| id > 1) {
            return Handle::NULL;
        }

        // The ordered handles put the overall sheet viewport first. Some DWGs
        // number that viewport 2 and a model-content viewport 1, so an ID must
        // never override the drawing's stored order. `layout.viewport` is not
        // used here: it records the last active viewport, which can be content.
        let listed = layout
            .viewports
            .iter()
            .copied()
            .filter_map(|handle| owned_viewport(handle).map(|vp| (handle, vp)));
        if let Some((handle, _)) = listed.into_iter().next() {
            return handle;
        }

        let block_viewports = block_handles
            .iter()
            .copied()
            .filter_map(|handle| owned_viewport(handle).map(|vp| (handle, vp)));
        if let Some((handle, _)) = block_viewports.into_iter().next() {
            return handle;
        }

        document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Viewport(vp) if vp.common.owner_handle == layout.block_record => {
                    Some((
                        vp.common.handle,
                        vp.id == 1,
                        vp.width.abs() * vp.height.abs(),
                    ))
                }
                _ => None,
            })
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.total_cmp(&b.2)))
            .map(|(handle, _, _)| handle)
            .unwrap_or(Handle::NULL)
    }

    pub(crate) fn is_sheet_viewport(
        document: &document::Document,
        vp: &document::entities::Viewport,
    ) -> bool {
        document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::Layout(layout) if layout.block_record == vp.common.owner_handle => {
                    Some(Self::layout_sheet_viewport_handle(document, layout) == vp.common.handle)
                }
                _ => None,
            })
            // Keep the legacy classification for orphaned viewport entities.
            .unwrap_or(vp.id == 1)
    }

    pub(crate) fn sheet_viewport_handles(&self) -> std::collections::HashSet<Handle> {
        self.document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Layout(layout) => {
                    let handle = Self::layout_sheet_viewport_handle(&self.document, layout);
                    handle.is_valid().then_some(handle)
                }
                _ => None,
            })
            .collect()
    }

    /// The handles an edit command may act on: layout sheet viewports are
    /// not user-editable. Model space has no sheet viewport.
    pub(crate) fn without_sheet_viewports(&self, handles: &[Handle]) -> Vec<Handle> {
        let any_viewport = handles.iter().any(|handle| {
            matches!(
                self.document.get_entity(*handle),
                Some(EntityType::Viewport(_))
            )
        });
        if !any_viewport {
            return handles.to_vec();
        }
        let sheets: std::collections::HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|object| match object {
                ObjectType::Layout(layout) if layout.name != "Model" => {
                    Some(Self::layout_sheet_viewport_handle(&self.document, layout))
                }
                _ => None,
            })
            .collect();
        handles
            .iter()
            .copied()
            .filter(|handle| !sheets.contains(handle))
            .collect()
    }

    pub(crate) fn current_layout_sheet_viewport_handle(&self) -> Handle {
        self.document
            .objects
            .values()
            .find_map(|obj| {
                let ObjectType::Layout(layout) = obj else {
                    return None;
                };
                if layout.name == self.current_layout {
                    Some(Self::layout_sheet_viewport_handle(&self.document, layout))
                } else {
                    None
                }
            })
            .unwrap_or(Handle::NULL)
    }

    /// Ensure a paper layout has its overall (`id == 1`) viewport; true when
    /// it had none (the layout opens for the first time).
    pub fn ensure_sheet_viewport(&mut self, layout_name: &str) -> bool {
        Self::ensure_layout_sheet_viewport(&mut self.document, layout_name)
    }

    /// [`Self::ensure_sheet_viewport`] on a document, for the copy a save
    /// writes.
    pub(crate) fn ensure_layout_sheet_viewport(
        document: &mut document::Document,
        layout_name: &str,
    ) -> bool {
        if layout_name == "Model" {
            return false;
        }
        // Locate the layout and its paper limits.
        let info = document.objects.iter().find_map(|(h, obj)| {
            if let ObjectType::Layout(l) = obj {
                if l.name == layout_name {
                    return Some((
                        *h,
                        l.block_record,
                        Self::layout_sheet_viewport_handle(document, l),
                        l.min_limits,
                        l.max_limits,
                    ));
                }
            }
            None
        });
        let Some((layout_handle, block_record, sheet, min_lim, max_lim)) = info else {
            return false;
        };
        if block_record.is_null() {
            return false;
        }

        if sheet.is_valid()
            && matches!(
                document.get_entity(sheet),
                Some(EntityType::Viewport(vp)) if vp.common.owner_handle == block_record
            )
        {
            return false;
        }

        // Create the full-screen overall viewport covering the paper limits.
        let mut vp = document::entities::Viewport::new();
        vp.id = 1;
        vp.status = document::entities::ViewportStatusFlags::default_on();
        fit_sheet_viewport(&mut vp, min_lim, max_lim);
        if let Ok(handle) =
            document.add_entity_to_layout(EntityType::Viewport(Box::new(vp)), layout_name)
        {
            if let Some(ObjectType::Layout(l)) = document.objects.get_mut(&layout_handle) {
                l.viewports.retain(|candidate| *candidate != handle);
                l.viewports.insert(0, handle);
            }
            // First in the block too: where a file keeps no viewport list
            // (DXF), the block's first viewport is read as the sheet, and a
            // layout can already hold viewports onto the model.
            if let Some(block) = document
                .block_records
                .iter_mut()
                .find(|block| block.handle == block_record)
            {
                if block.entity_handles.contains(&handle) {
                    block
                        .entity_handles
                        .retain(|candidate| *candidate != handle);
                    block.entity_handles.insert(0, handle);
                }
            }
        }
        true
    }

    /// Fit a layout's sheet viewport to its page. (Taking the sheet out
    /// instead would leave a viewport onto the model looking like the sheet.)
    pub fn fit_layout_sheet(&mut self, layout_name: &str) {
        let layout = self
            .document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::Layout(layout) if layout.name == layout_name => Some(layout.clone()),
                _ => None,
            });
        let Some(layout) = layout else {
            return;
        };
        let sheet = Self::layout_sheet_viewport_handle(&self.document, &layout);
        if let Some(EntityType::Viewport(vp)) = self.document.get_entity_mut(sheet) {
            fit_sheet_viewport(vp, layout.min_limits, layout.max_limits);
        }
    }

    /// Gives a layout whose page setup was never initialized (no paper size
    /// and no media name, as the reference leaves a layout until it is first
    /// opened) the default page setup; true when it did.
    pub fn initialize_layout_page_setup(&mut self, layout_name: &str) -> bool {
        let plot_style = self.new_layout_plot_style.clone();
        for object in self.document.objects.values_mut() {
            if let ObjectType::Layout(layout) = object {
                if layout.name == layout_name {
                    if !page_setup_uninitialized(layout) {
                        return false;
                    }
                    crate::scene::apply_default_page_setup(layout, &plot_style);
                    // A sheet written for a layout never set up is not the
                    // page's; it is fitted to the page.
                    self.fit_layout_sheet(layout_name);
                    self.bump_layout_epoch();
                    return true;
                }
            }
        }
        false
    }

    /// The viewport a layout gets when it first opens (LAYOUTCREATEVIEWPORT),
    /// as the reference makes it: centred on the printable area, 0.8 of its
    /// width and height, showing the model's extents.
    pub fn add_layout_default_viewport(&mut self, layout_name: &str) {
        if !layout_create_viewport() || !self.layout_content_viewports().is_empty() {
            return;
        }
        let Some(((x0, y0), (x1, y1))) = self.printable_area_limits() else {
            return;
        };
        let mut viewport = document::entities::Viewport::new();
        viewport.center = document::types::Vector3::new((x0 + x1) / 2.0, (y0 + y1) / 2.0, 0.0);
        viewport.width = (x1 - x0).abs() * 0.8;
        viewport.height = (y1 - y0).abs() * 0.8;
        viewport.id = 2;
        viewport.status = document::entities::ViewportStatusFlags::default_on();
        if let Ok(handle) = self
            .document
            .add_entity_to_layout(EntityType::Viewport(Box::new(viewport)), layout_name)
        {
            // The reference zooms the new viewport to the model's extents
            // with a margin of under one percent.
            self.fit_viewport_to_model(handle, 1.0075);
            self.notify_viewport_changed(handle);
            self.bump_entities(&[(handle, ChangeKind::Added)]);
        }
    }

    pub(crate) fn is_content_viewport_in_layout(
        &self,
        vp: &document::entities::Viewport,
        layout_block: Handle,
    ) -> bool {
        if vp.common.owner_handle != layout_block {
            return false;
        }
        let sheet_handle = self.current_layout_sheet_viewport_handle();
        if sheet_handle.is_valid() {
            vp.common.handle != sheet_handle
        } else {
            !Self::is_sheet_viewport(&self.document, vp)
        }
    }

    /// Public accessor for the block-record handle of the current layout.
    /// Used by external callers (e.g. `commit_entity`) that need the handle
    /// without going through private API.
    pub fn current_layout_block_handle_pub(&self) -> Handle {
        self.current_layout_block_handle()
    }

    /// A unique id for a new viewport on the current layout: one past the
    /// largest id there, at least 2 (1 is the layout's own paper viewport).
    pub fn next_viewport_id(&self) -> i16 {
        let layout_block = self.current_layout_block_handle();
        let max_id = self
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Viewport(v) if v.common.owner_handle == layout_block => {
                    Some(v.id)
                }
                _ => None,
            })
            .max()
            .unwrap_or(1);
        (max_id + 1).max(2)
    }

    /// True when an entity belongs to the active layout, including imported
    /// DXF entities whose owner handle is NULL but whose BlockRecord still
    /// lists the entity. Keep command validation aligned with the same
    /// ownership fallback used by rendering and hit-testing.
    pub(crate) fn entity_belongs_to_current_layout(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        self.belongs_to_visible_block(
            handle,
            entity.common().owner_handle,
            self.current_layout_block_handle(),
        )
    }

    /// True when an entity belongs to the space receiving newly drawn
    /// entities: BEDIT's block, paper space, or model space (including MSPACE).
    pub(crate) fn entity_belongs_to_active_space(&self, handle: Handle) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return false;
        };
        let block = if let Some(block) = self.block_edit_block {
            block
        } else if self.current_layout != "Model" && self.active_viewport.is_none() {
            self.current_layout_block_handle()
        } else {
            self.model_space_block_handle()
        };
        self.belongs_to_visible_block(handle, entity.common().owner_handle, block)
    }

    /// Returns the block-record handle for `current_layout`.
    ///
    /// Primary path: the Layout object's `block_record` field (set correctly
    /// by the DWG reader).
    ///
    /// Fallback for DXF files: the DXF reader never reads group code 340
    /// (block_record handle), so `block_record` is NULL after loading DXF.
    /// In that case we derive the block-record name from the DXF convention:
    ///   Model            → "*Model_Space"
    ///   first paper tab  → "*Paper_Space"
    ///   second paper tab → "*Paper_Space0"
    ///   Nth paper tab    → "*Paper_Space{N-2}"
    pub(crate) fn current_layout_block_handle(&self) -> Handle {
        // BEDIT block editor: the active space IS a block record — draw / pick /
        // own only its own (block-local) entities (issue #261).
        if let Some(h) = self.block_edit_block {
            if !h.is_null() {
                return h;
            }
        }
        // Locate the Layout object for the active layout name.
        let layout = self.document.objects.values().find_map(|obj| {
            if let ObjectType::Layout(l) = obj {
                if l.name == self.current_layout {
                    Some(l)
                } else {
                    None
                }
            } else {
                None
            }
        });

        if let Some(l) = layout {
            // Fast path: block_record already set (DWG reader).
            if !l.block_record.is_null() {
                return l.block_record;
            }

            // Fallback: resolve via conventional DXF block-record name.
            let br_name: String = if self.current_layout == "Model" {
                "*Model_Space".into()
            } else {
                // tab_order 1 → "*Paper_Space",  2 → "*Paper_Space0", etc.
                let tab = l.tab_order;
                if tab <= 1 {
                    "*Paper_Space".into()
                } else {
                    format!("*Paper_Space{}", tab - 2)
                }
            };

            if let Some(br) = self.document.block_records.get(&br_name) {
                return br.handle;
            }

            // Last resort: match by position among paper layouts when tab_order
            // is unreliable (some exporters set it to 0 for all layouts).
            if self.current_layout != "Model" {
                let mut ps_brs: Vec<_> = self
                    .document
                    .block_records
                    .iter()
                    .filter(|br| br.is_paper_space())
                    .collect();
                ps_brs.sort_by(|a, b| a.name.cmp(&b.name));

                let mut paper_layouts: Vec<(i16, &str)> = self
                    .document
                    .objects
                    .values()
                    .filter_map(|obj| {
                        if let ObjectType::Layout(l) = obj {
                            if l.name != "Model" {
                                Some((l.tab_order, l.name.as_str()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect();
                paper_layouts.sort_by_key(|(o, n)| (*o, *n));

                if let Some(pos) = paper_layouts
                    .iter()
                    .position(|(_, n)| *n == self.current_layout)
                {
                    if let Some(br) = ps_brs.get(pos) {
                        return br.handle;
                    }
                }
            } else if let Some(br) = self.document.block_records.get("*Model_Space") {
                return br.handle;
            }
        }

        Handle::NULL
    }

    /// Returns `(min, max)` paper-space limits for the current layout, or `None`
    /// when in Model space.  Falls back to A4 landscape if nothing reliable is found.
    /// A solid white fill covering the paper sheet's printable area, rendered
    /// by the GPU hatch pipeline behind the paper entities. Replaces the 2-D
    /// white-rectangle the old PaperCanvas drew. `None` in model space or when
    /// the layout has no limits.
    pub(crate) fn paper_sheet_fill(&self) -> Option<HatchModel> {
        let ((x0, y0), (x1, y1)) = self.paper_limits()?;
        let (x0, y0, x1, y1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
        Some(HatchModel {
            entity: None,
            effective_layer: None,
            pattern_origin: None,
            render_instance: None,
            world_origin: [0.0, 0.0],
            boundary: Arc::new(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]),
            boundary_wcs: None,
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: document::entities::HatchStyleType::Normal,
            pattern: crate::scene::model::hatch_model::HatchPattern::Solid,
            name: "SOLID".to_string(),
            color: self.paper_bg_color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            // Draw-order bias is signed: entity fills/wires land in (-1, 1)
            // (0 = neutral). A value below -1 marks the page: the draw-order
            // shader puts it at the far end of the depth range, behind every
            // object the sheet holds, whatever its Z. The sheet is the
            // canvas, never on top.
            draw_depth: -2.0,
        })
    }

    /// Dashed rectangle marking the printable area — the paper inset by the
    /// layout's plot margins. `None` in model space, when
    /// the layout has no margins, or when the inset would be degenerate.
    pub(crate) fn printable_area_wire(&self) -> Option<WireModel> {
        if self.current_layout == "Model" {
            return None;
        }
        let ((x0, y0), (x1, y1)) = self.paper_limits()?;
        let (left, bottom, right, top, rot) = self.document.objects.values().find_map(|obj| {
            if let ObjectType::Layout(l) = obj {
                if l.name == self.current_layout {
                    return Some((
                        l.plot_margin_left,
                        l.plot_margin_bottom,
                        l.plot_margin_right,
                        l.plot_margin_top,
                        l.plot_rotation,
                    ));
                }
            }
            None
        })?;
        // `paper_limits()` already swaps the sheet for a 90°/270° rotation, so the
        // margins must rotate to the same edges.
        let (ml, mb, mr, mt) = page_setup::rotated_margins((left, bottom, right, top), rot);
        // Plot margins are millimetres like the paper size; scale them into the
        // layout's paper-space units so the inset matches the (already scaled)
        // sheet rect. Without this an inch paper space insets an ~8-inch sheet
        // by ~6 "inches" of margin and the printable rect collapses.
        let f = self.paper_space_unit_factor();
        let (ml, mb, mr, mt) = (ml * f, mb * f, mr * f, mt * f);
        // Nothing to show when there are no margins (printable area == sheet).
        if ml <= 0.0 && mb <= 0.0 && mr <= 0.0 && mt <= 0.0 {
            return None;
        }
        let (px0, py0, px1, py1) = (x0 + ml, y0 + mb, x1 - mr, y1 - mt);
        if px1 - px0 < 1e-3 || py1 - py0 < 1e-3 {
            return None;
        }
        let (px0, py0, px1, py1) = (px0 as f32, py0 as f32, px1 as f32, py1 as f32);
        let mut wire = WireModel::solid(
            "paper_printable_area".to_string(),
            vec![
                [px0, py0, 0.0],
                [px1, py0, 0.0],
                [px1, py1, 0.0],
                [px0, py1, 0.0],
                [px0, py0, 0.0],
            ],
            [0.5, 0.5, 0.5, 1.0],
            false,
        );
        // Dashed: 4 mm dash, 3 mm gap. The dash lengths are millimetres, so
        // scale them into the layout's paper-space units too (`f`) — otherwise
        // an inch paper space draws 4-"inch" dashes (25.4× too long).
        let ff = f as f32;
        wire.pattern_length = 7.0 * ff;
        wire.pattern = [4.0 * ff, -3.0 * ff, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        Some(wire)
    }

    /// The effective plot settings embedded in the current layout.
    pub fn effective_plot_settings(&self) -> Option<document::objects::PlotSettings> {
        self.plot_settings_for(&self.current_layout)
    }

    /// Plot settings embedded in a specific layout.
    pub fn plot_settings_for(&self, name: &str) -> Option<document::objects::PlotSettings> {
        use document::objects::{
            ObjectType, PaperMargin, PlotPaperUnits, PlotRotation, PlotSettings, PlotType,
            PlotWindow, ScaledType, ShadePlotMode, ShadePlotResolutionLevel,
        };
        self.document.objects.values().find_map(|o| {
            let ObjectType::Layout(l) = o else {
                return None;
            };
            if l.name.as_str() != name {
                return None;
            }
            let mut ps = PlotSettings::new(l.plot_page_name.clone());
            ps.printer_name = l.plot_printer_name.clone();
            ps.paper_width = l.paper_width;
            ps.paper_height = l.paper_height;
            ps.paper_size = l.paper_size.clone();
            ps.plot_view_name = l.plot_view_name.clone();
            ps.current_style_sheet = l.plot_style_sheet.clone();
            ps.margins = PaperMargin::new(
                l.plot_margin_left,
                l.plot_margin_bottom,
                l.plot_margin_right,
                l.plot_margin_top,
            );
            ps.origin_x = l.plot_origin_x;
            ps.origin_y = l.plot_origin_y;
            ps.plot_window = PlotWindow::new(
                l.plot_window_min_x,
                l.plot_window_min_y,
                l.plot_window_max_x,
                l.plot_window_max_y,
            );
            ps.paper_units = PlotPaperUnits::from_code(l.plot_paper_units);
            ps.rotation = PlotRotation::from_code(l.plot_rotation);
            ps.plot_type = PlotType::from_code(l.plot_type);
            ps.scale_type = ScaledType::from_code(l.plot_scale_type);
            ps.scale_numerator = l.plot_scale_numerator;
            ps.scale_denominator = l.plot_scale_denominator;
            ps.flags = l.plot_flags;
            ps.standard_scale_factor = l.plot_scale_factor;
            ps.paper_image_origin_x = l.paper_image_origin_x;
            ps.paper_image_origin_y = l.paper_image_origin_y;
            ps.shade_plot_mode = ShadePlotMode::from_code(l.shade_plot_mode);
            ps.shade_plot_resolution = ShadePlotResolutionLevel::from_code(l.shade_plot_resolution);
            ps.shade_plot_dpi = l.shade_plot_dpi;
            ps.plot_view_handle = l.plot_view_handle;
            ps.visual_style_handle = l.visual_style_handle;
            Some(ps)
        })
    }

    /// Replace the plot-settings portion embedded in one layout.
    pub fn set_layout_plot_settings(
        &mut self,
        name: &str,
        ps: &document::objects::PlotSettings,
    ) -> bool {
        let Some(layout) = self.document.objects.values_mut().find_map(|object| {
            let ObjectType::Layout(layout) = object else {
                return None;
            };
            (layout.name == name).then_some(layout)
        }) else {
            return false;
        };

        layout.plot_page_name = ps.page_name.clone();
        layout.plot_printer_name = ps.printer_name.clone();
        layout.paper_size = ps.paper_size.clone();
        layout.plot_view_name = ps.plot_view_name.clone();
        layout.plot_style_sheet = ps.current_style_sheet.clone();
        layout.plot_margin_left = ps.margins.left;
        layout.plot_margin_bottom = ps.margins.bottom;
        layout.plot_margin_right = ps.margins.right;
        layout.plot_margin_top = ps.margins.top;
        layout.paper_width = ps.paper_width;
        layout.paper_height = ps.paper_height;
        layout.plot_origin_x = ps.origin_x;
        layout.plot_origin_y = ps.origin_y;
        layout.plot_window_min_x = ps.plot_window.lower_left_x;
        layout.plot_window_min_y = ps.plot_window.lower_left_y;
        layout.plot_window_max_x = ps.plot_window.upper_right_x;
        layout.plot_window_max_y = ps.plot_window.upper_right_y;
        layout.plot_scale_numerator = ps.scale_numerator;
        layout.plot_scale_denominator = ps.scale_denominator;
        layout.plot_paper_units = ps.paper_units.to_code();
        layout.plot_rotation = ps.rotation.to_code();
        layout.plot_type = ps.plot_type.to_code();
        layout.plot_scale_type = ps.scale_type.to_code();
        layout.shade_plot_mode = ps.shade_plot_mode.to_code();
        layout.shade_plot_resolution = ps.shade_plot_resolution.to_code();
        layout.shade_plot_dpi = ps.shade_plot_dpi;
        layout.plot_flags = ps.flags;
        layout.plot_scale_factor = ps.standard_scale_factor;
        layout.paper_image_origin_x = ps.paper_image_origin_x;
        layout.paper_image_origin_y = ps.paper_image_origin_y;
        layout.plot_view_handle = ps.plot_view_handle;
        layout.visual_style_handle = ps.visual_style_handle;
        layout.raw_plot_settings_codes = None;
        if name != "Model" {
            Self::place_centred_window_sheet(layout);
        }
        // The sheet bounds derive from the paper size, scale and units just
        // written; drop the layout's cached copy so the next draw or plot
        // reads the new sheet instead of the one from before this change.
        self.paper_viewport_cache.borrow_mut().remove(name);
        self.display_plot_style_cache
            .borrow_mut()
            .retain(|(layout_name, _), _| layout_name != name);
        self.styled_wire_cache.borrow_mut().clear();
        self.styled_hatch_cache.borrow_mut().clear();
        self.styled_wire_fill_cache.borrow_mut().clear();
        true
    }

    pub fn invalidate_display_plot_style(&self) {
        self.display_plot_style_cache.borrow_mut().clear();
        self.styled_wire_cache.borrow_mut().clear();
        self.styled_hatch_cache.borrow_mut().clear();
        self.styled_wire_fill_cache.borrow_mut().clear();
    }

    /// PlotSettings store the paper size and plot margins in millimetres, but a
    /// layout's paper space is laid out in its own units — its viewports,
    /// drawing limits and the sheet the user sees can be in inches, millimetres,
    /// or an arbitrary scaled unit. Return the factor that converts those
    /// millimetre plot-settings values into the active layout's paper-space
    /// units (`1.0` for a millimetre paper space, `1/25.4` for an inch one, and
    /// anything else for a scaled layout).
    ///
    /// The unit is derived from the plot scale, which maps paper-space units to
    /// the physical page: `mm_per_unit = (scale_num / scale_den) × page_unit_mm`
    /// where `page_unit_mm` is 25.4 for an inch page (code 72 == 0) or 1 for a
    /// millimetre page. The scale and the page unit are a matched pair, so this
    /// is correct even when the page unit alone looks wrong (an inch page at
    /// scale 1:25.4 is a millimetre paper space). Falls back to `1.0` when the
    /// scale is missing or degenerate.
    pub fn paper_space_unit_factor(&self) -> f64 {
        if self.current_layout == "Model" {
            return 1.0;
        }
        self.document
            .objects
            .values()
            .find_map(|obj| match obj {
                ObjectType::Layout(l) if l.name == self.current_layout => {
                    Some(Self::layout_unit_factor(l))
                }
                _ => None,
            })
            .unwrap_or(1.0)
    }

    /// Millimetre → paper-space-unit factor for a specific layout (see
    /// [`Scene::paper_space_unit_factor`]).
    pub(crate) fn layout_unit_factor(l: &document::objects::Layout) -> f64 {
        // Millimetres represented by one plotted page unit: an inch page
        // (plot_paper_units == 0) is 25.4 mm, a millimetre page is 1 mm.
        let page_unit_mm = if l.plot_paper_units == 0 { 25.4 } else { 1.0 };
        let (num, den) = (l.plot_scale_numerator, l.plot_scale_denominator);
        // Scale type 0 is "scaled to fit" only with the standard-scale flag; a
        // custom scale leaves it 0 too. A fitted plot's stored ratio is not the
        // unit paper space is laid out in: trusting it scaled the layout's
        // lineweights by that ratio after a fitted window plot (#3103).
        let fitted = l.plot_flags.use_standard_scale && l.plot_scale_type == 0;
        let (wx0, wy0, wx1, wy1) = (
            l.plot_window_min_x,
            l.plot_window_min_y,
            l.plot_window_max_x,
            l.plot_window_max_y,
        );
        if fitted && l.plot_type == 4 && wx1 > wx0 && wy1 > wy0 {
            // The plot scales the window onto the sheet, so the sheet spans
            // the window at that scale.
            let (pw, ph) = if l.plot_rotation == 1 || l.plot_rotation == 3 {
                (l.paper_height, l.paper_width)
            } else {
                (l.paper_width, l.paper_height)
            };
            if pw > 1e-6 && ph > 1e-6 {
                let (mm_per_unit, ..) = crate::io::paper_catalog::window_to_sheet(
                    (wx1 - wx0, wy1 - wy0),
                    (pw, ph),
                    crate::io::paper_catalog::PlotScale::Fit,
                );
                return 1.0 / mm_per_unit.max(1e-9);
            }
        }
        if !fitted && num > 1e-12 && den > 1e-12 {
            // One paper-space unit spans this many millimetres on the page.
            let mm_per_unit = (num / den) * page_unit_mm;
            if mm_per_unit > 1e-9 {
                return 1.0 / mm_per_unit;
            }
        }
        // No usable plot scale — assume one unit per page unit (mm or inch).
        if fitted {
            1.0 / page_unit_mm
        } else {
            1.0
        }
    }

    /// The sheet of a paper layout in paper-space units, from its plot
    /// settings; none when the layout has no paper size.
    pub fn layout_sheet(l: &document::objects::Layout) -> Option<((f64, f64), (f64, f64))> {
        // Use the physical paper dimensions from PlotSettings if available
        // (populated from DWG embedded plot settings or DXF codes 44/45/73).
        // Rotation 1=90° or 3=270° → swap width and height.
        if l.paper_width <= 1e-6 || l.paper_height <= 1e-6 {
            return None;
        }
        let (pw, ph) = if l.plot_rotation == 1 || l.plot_rotation == 3 {
            (l.paper_height, l.paper_width)
        } else {
            (l.paper_width, l.paper_height)
        };
        // paper_width/height are millimetres; scale them into
        // the layout's paper-space units so the sheet lands in
        // the same coordinate system as the viewports drawn on
        // it. An inch-based paper space would otherwise get a
        // 25.4× oversized sheet, shrinking the content into the
        // corner.
        let (wx0, wy0, wx1, wy1) = (
            l.plot_window_min_x,
            l.plot_window_min_y,
            l.plot_window_max_x,
            l.plot_window_max_y,
        );
        let window_plot = l.plot_type == 4 && wx1 > wx0 && wy1 > wy0;
        let f = Self::layout_unit_factor(l);
        let (pw, ph) = (pw * f, ph * f);
        // A window plot maps that window onto the sheet, so the
        // sheet sits around it rather than at the origin.
        let (ox, oy) = if window_plot {
            if l.plot_flags.plot_centered {
                ((wx0 + wx1 - pw) / 2.0, (wy0 + wy1 - ph) / 2.0)
            } else {
                (wx0 - l.plot_origin_x * f, wy0 - l.plot_origin_y * f)
            }
        } else {
            // The printable area's corner sits at minus the
            // paper image origin (paper-space units, usually
            // 0); the sheet starts the stored margins (turned
            // with the plot) before it. LIMMIN is not used; it
            // goes stale when the device or medium changes.
            let (left, bottom, ..) = crate::scene::rotated_margins(
                (
                    l.plot_margin_left,
                    l.plot_margin_bottom,
                    l.plot_margin_right,
                    l.plot_margin_top,
                ),
                l.plot_rotation,
            );
            (
                -l.paper_image_origin_x - left * f,
                -l.paper_image_origin_y - bottom * f,
            )
        };
        Some(((ox, oy), (ox + pw, oy + ph)))
    }

    /// A centred window plot places the sheet around its window. The paper
    /// image origin and the limits other programs open the sheet with follow
    /// that window, or they keep the place of the window the layout had
    /// before (#3209). Other layouts are left as they are. True when a value
    /// changed.
    pub fn place_centred_window_sheet(layout: &mut document::objects::Layout) -> bool {
        let window_plot = layout.plot_type == 4
            && layout.plot_window_max_x > layout.plot_window_min_x
            && layout.plot_window_max_y > layout.plot_window_min_y;
        if !window_plot || !layout.plot_flags.plot_centered {
            return false;
        }
        let before = (
            layout.paper_image_origin_x,
            layout.paper_image_origin_y,
            layout.min_limits,
            layout.max_limits,
        );
        // ponytail: 1:1 layouts; a scaled window plot would need the window
        // corner in paper units.
        layout.paper_image_origin_x = -layout.plot_window_min_x;
        layout.paper_image_origin_y = -layout.plot_window_min_y;
        if let Some((min, max)) = Self::layout_sheet(layout) {
            layout.min_limits = min;
            layout.max_limits = max;
        }
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-9;
        let (x, y, min, max) = before;
        !(close(x, layout.paper_image_origin_x)
            && close(y, layout.paper_image_origin_y)
            && close(min.0, layout.min_limits.0)
            && close(min.1, layout.min_limits.1)
            && close(max.0, layout.max_limits.0)
            && close(max.1, layout.max_limits.1))
    }

    pub fn paper_limits(&self) -> Option<((f64, f64), (f64, f64))> {
        if self.current_layout == "Model" {
            return None;
        }

        if let Some(cache) = self.paper_viewport_cache.borrow().get(&self.current_layout) {
            if cache.epoch == self.geometry_epoch && cache.layout == self.current_layout {
                return cache.paper_limits;
            }
        }

        self.document
            .objects
            .values()
            .find_map(|obj| {
                if let ObjectType::Layout(l) = obj {
                    if l.name != self.current_layout {
                        return None;
                    }

                    if let Some(sheet) = Self::layout_sheet(l) {
                        return Some(sheet);
                    }

                    // Fall back to the Layout's drawing limits.
                    let (min, max) = (l.min_limits, l.max_limits);
                    let w = (max.0 - min.0).abs();
                    let h = (max.1 - min.1).abs();
                    if w < 1e-6 || h < 1e-6 {
                        return Some(((0.0, 0.0), (297.0, 210.0)));
                    }
                    Some((min, max))
                } else {
                    None
                }
            })
            .or(Some(((0.0, 0.0), (297.0, 210.0))))
    }

    /// Printable rectangle of the current paper layout, in paper-space units.
    /// Falls back to the whole sheet when the layout has no usable margins.
    pub fn printable_area_limits(&self) -> Option<((f64, f64), (f64, f64))> {
        let ((x0, y0), (x1, y1)) = self.paper_limits()?;
        let margins = self.document.objects.values().find_map(|object| {
            if let ObjectType::Layout(layout) = object {
                if layout.name == self.current_layout {
                    return Some((
                        layout.plot_margin_left,
                        layout.plot_margin_bottom,
                        layout.plot_margin_right,
                        layout.plot_margin_top,
                        layout.plot_rotation,
                    ));
                }
            }
            None
        });
        let Some((left, bottom, right, top, rotation)) = margins else {
            return Some(((x0, y0), (x1, y1)));
        };
        let (left, bottom, right, top) =
            crate::scene::rotated_margins((left, bottom, right, top), rotation);
        let factor = self.paper_space_unit_factor();
        let printable = (
            (x0 + left * factor, y0 + bottom * factor),
            (x1 - right * factor, y1 - top * factor),
        );
        if printable.1 .0 - printable.0 .0 < 1e-6 || printable.1 .1 - printable.0 .1 < 1e-6 {
            Some(((x0, y0), (x1, y1)))
        } else {
            Some(printable)
        }
    }

    /// Scale of the active, selected, or first content viewport in the current
    /// paper layout. Returns `None` in Model space or when no viewport exists.
    pub fn first_viewport_scale(&self) -> Option<f64> {
        let handle = self.target_viewport_handle()?;
        let EntityType::Viewport(vp) = self.document.get_entity(handle)? else {
            return None;
        };
        Some(vp_effective_scale(
            vp.custom_scale,
            vp.view_height,
            vp.height,
        ))
    }

    /// Annotation/viewport scales defined in the drawing's scale list
    /// (the `ACAD_SCALELIST` dictionary), as `(label, annotation_multiplier,
    /// viewport_factor)`. The annotation multiplier sizes model-space
    /// text/dims (50.0 for "1:50"); the viewport factor is the paper/drawing
    /// ratio (0.02 for "1:50"). Sorted smallest ratio first (1:100 … 1:1 …
    /// 10:1). Falls back to a standard ratio set when the drawing carries no
    /// scale list of its own, so the scale picker is always usable. (#154)
    /// Standard fallback sets as (label, paper/drawing factor). The drawing's
    /// INSUNITS selects metric ratios or imperial architectural labels. Used
    /// when no file scales exist and materialised on demand by
    /// [`Scene::ensure_real_scale_list`].
    pub(crate) const DEFAULT_METRIC_SCALES: &'static [(&'static str, f64)] = &[
        ("1:1000", 0.001),
        ("1:500", 0.002),
        ("1:200", 0.005),
        ("1:100", 0.01),
        ("1:50", 0.02),
        ("1:25", 0.04),
        ("1:20", 0.05),
        ("1:10", 0.1),
        ("1:5", 0.2),
        ("1:2", 0.5),
        ("1:1", 1.0),
        ("2:1", 2.0),
        ("5:1", 5.0),
        ("10:1", 10.0),
    ];

    pub(crate) const DEFAULT_IMPERIAL_SCALES: &'static [(&'static str, f64)] = &[
        ("1/128\" = 1'-0\"", 1.0 / 1536.0),
        ("1/64\" = 1'-0\"", 1.0 / 768.0),
        ("1/32\" = 1'-0\"", 1.0 / 384.0),
        ("1/16\" = 1'-0\"", 1.0 / 192.0),
        ("3/32\" = 1'-0\"", 1.0 / 128.0),
        ("1/8\" = 1'-0\"", 1.0 / 96.0),
        ("3/16\" = 1'-0\"", 1.0 / 64.0),
        ("1/4\" = 1'-0\"", 1.0 / 48.0),
        ("3/8\" = 1'-0\"", 1.0 / 32.0),
        ("1/2\" = 1'-0\"", 1.0 / 24.0),
        ("3/4\" = 1'-0\"", 1.0 / 16.0),
        ("1\" = 1'-0\"", 1.0 / 12.0),
        ("1-1/2\" = 1'-0\"", 1.0 / 8.0),
        ("3\" = 1'-0\"", 1.0 / 4.0),
        ("6\" = 1'-0\"", 1.0 / 2.0),
        ("1'-0\" = 1'-0\"", 1.0),
    ];

}
