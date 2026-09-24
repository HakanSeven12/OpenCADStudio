use super::*;

impl OpenCADStudio {
    pub(super) fn handle_match_entity_layer(&mut self, dest: Vec<Handle>, src: Handle) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        let src_layer = self.tabs[i]
            .scene
            .document
            .get_entity(src)
            .map(|e| e.common().layer.clone());
        let dest: Vec<_> = dest
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        if dest.is_empty() {
            self.command_line
                .push_info(crate::t!("No editable objects selected.").as_ref());
        } else if let Some(layer) = src_layer {
            self.push_undo_snapshot(i, "LAYMATCH");
            for h in &dest {
                if let Some(e) = self.tabs[i].scene.document.get_entity_mut(*h) {
                    e.as_entity_mut().set_layer(layer.clone());
                }
            }
            // New layer changes the baked by-layer colour/linetype/
            // lineweight — re-tessellate the moved entities so they
            // repaint immediately (issue #231 class).
            self.invalidate_property_targets(i, &dest);
            self.tabs[i].dirty = true;
            self.command_line
                .push_info(crate::tf!("Layer matched to \"{layer}\".").as_ref());
            self.sync_ribbon_layers();
        } else {
            self.command_line
                .push_error(crate::t!("Source object not found.").as_ref());
        }
    }


    pub(super) fn handle_match_properties(
        &mut self,
        mut dest: Vec<Handle>,
        src: Handle,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        dest.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if dest.is_empty() {
            return Some(Task::none());
        }
        // The command stays active after each apply so more targets
        // can keep being picked; Enter / Esc ends it (#362).
        let src_clone = self.tabs[i].scene.document.get_entity(src).cloned();
        let src_common = src_clone.as_ref().map(|e| e.common().clone());
        let thickness = src_clone
            .as_ref()
            .and_then(crate::scene::view::dispatch::entity_thickness);
        // This application record is the hatch background colour.
        // Keep the outer Option to distinguish "source is not a
        // hatch" from "source hatch has no background".
        let hatch_background_xdata: Option<Option<Vec<acadrust::xdata::XDataValue>>> =
            match src_clone.as_ref() {
                Some(acadrust::EntityType::Hatch(h)) => Some(
                    h.common
                        .extended_data
                        .get_record("HATCHBACKGROUNDCOLOR")
                        .map(|r| r.values.clone()),
                ),
                _ => None,
            };
        // Dimension-style overrides ride the ACAD record, identified
        // by a leading DSTYLE string. Matching replicates that payload
        // (or clears the destination when the source has none).
        let dstyle_xdata: Option<Vec<(i16, acadrust::xdata::XDataValue)>> = src_clone
            .as_ref()
            .filter(|e| {
                matches!(
                    e,
                    acadrust::EntityType::Dimension(_) | acadrust::EntityType::Leader(_)
                )
            })
            .map(|e| crate::entities::dim_override::pairs(&e.common().extended_data));

        if let Some(common) = src_common {
            self.apply_property_op(i, "MATCHPROP", &dest, |app, handle| {
                let mut is_dim = false;
                let mut is_hatch = false;
                if let Some(e) = app.tabs[i].scene.document.get_entity_mut(handle) {
                    e.as_entity_mut().set_layer(common.layer.clone());
                    crate::scene::view::dispatch::apply_color(e, common.color);
                    crate::scene::view::dispatch::apply_line_weight(e, common.line_weight);
                    {
                        let dst_common = e.common_mut();
                        dst_common.linetype = common.linetype.clone();
                        dst_common.linetype_handle = common.linetype_handle;
                        dst_common.linetype_scale = common.linetype_scale;
                        dst_common.transparency = common.transparency.clone();
                        dst_common.color_name = common.color_name.clone();
                        dst_common.color_book_handle = common.color_book_handle;
                        dst_common.full_visual_style_handle = common.full_visual_style_handle;
                        dst_common.face_visual_style_handle = common.face_visual_style_handle;
                        dst_common.edge_visual_style_handle = common.edge_visual_style_handle;
                        dst_common.material_flags = common.material_flags;
                        dst_common.material_handle = common.material_handle;
                        dst_common.shadow_flags = common.shadow_flags;
                        dst_common.plotstyle_flags = common.plotstyle_flags;
                        dst_common.plotstyle_handle = common.plotstyle_handle;
                    }
                    if let Some(value) = thickness {
                        crate::scene::view::dispatch::set_entity_thickness(e, value);
                    }
                    if let Some(se) = &src_clone {
                        is_dim = matches!(e, acadrust::EntityType::Dimension(_));
                        is_hatch = matches!(e, acadrust::EntityType::Hatch(_));
                        match_special_props(se, e);
                    }
                }
                if is_hatch {
                    if let Some(values) = &hatch_background_xdata {
                        crate::scene::view::dispatch::set_entity_xdata(
                            &mut app.tabs[i].scene.document,
                            handle,
                            "HATCHBACKGROUNDCOLOR",
                            values.clone(),
                        );
                    }
                }
                // Dim-style overrides follow the style for dimension /
                // leader destinations — through set_entity_xdata so no
                // stale raw record survives.
                if dstyle_xdata.is_some()
                    || matches!(
                        app.tabs[i].scene.document.get_entity(handle),
                        Some(acadrust::EntityType::Dimension(_) | acadrust::EntityType::Leader(_))
                    )
                {
                    if matches!(
                        app.tabs[i].scene.document.get_entity(handle),
                        Some(acadrust::EntityType::Dimension(_) | acadrust::EntityType::Leader(_))
                    ) && matches!(
                        src_clone,
                        Some(acadrust::EntityType::Dimension(_) | acadrust::EntityType::Leader(_))
                    ) {
                        crate::entities::dim_override::replace(
                            &mut app.tabs[i].scene.document,
                            handle,
                            dstyle_xdata.clone().unwrap_or_default(),
                        );
                    }
                }
                // A restyled dimension renders from its baked *D block —
                // drop the stale block so the new style shows (#398).
                if is_dim {
                    app.tabs[i].scene.invalidate_dim_block_recorded(handle);
                }
                // Hatch fills render from a prebuilt model (#415).
                app.tabs[i].scene.refresh_fill_model(handle);
            });
            // Color / linetype / lineweight are baked into the cached
            // wires at tessellation time; re-tessellate only the matched
            // objects instead of rebuilding a large drawing.
            let changes: Vec<_> = dest
                .iter()
                .copied()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities(&changes);
            self.command_line
                .push_info(crate::tf!("Properties matched to {} object(s).", dest.len()).as_ref());
            // Clear the consumed target selection and keep prompting.
            self.tabs[i].scene.deselect_all();
            if let Some(cmd) = &self.tabs[i].active_cmd {
                self.command_line.push_info(&cmd.prompt());
            }
        } else {
            self.command_line
                .push_error(crate::t!("Source object not found.").as_ref());
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.tabs[i].scene.clear_preview_wire();
        }
        None
    }

}

/// MATCHPROP special properties (#281): copy every STYLE-affecting field from
/// `src` to `dst` when the destination supports it — never content (text
/// strings, block names) or placement (positions, rotations of the object
/// itself). Text formatting crosses TEXT ↔ MTEXT; the dimension style crosses
/// Dimension / Leader / Tolerance.
fn match_special_props(src: &acadrust::EntityType, dst: &mut acadrust::EntityType) {
    use acadrust::EntityType as E;

    // Text-ish source formatting.
    let text_fmt = match src {
        E::Text(t) => Some((
            t.style.clone(),
            t.height,
            Some(t.width_factor),
            Some(t.oblique_angle),
        )),
        E::MText(m) => Some((m.style.clone(), m.height, None, None)),
        E::AttributeDefinition(a) => Some((
            a.text_style.clone(),
            a.height,
            Some(a.width_factor),
            Some(a.oblique_angle),
        )),
        E::AttributeEntity(a) => Some((
            a.text_style.clone(),
            a.height,
            Some(a.width_factor),
            Some(a.oblique_angle),
        )),
        _ => None,
    };
    if let Some((style, height, wf, ob)) = text_fmt {
        match dst {
            E::Text(t) => {
                t.style = style.clone();
                t.height = height;
                if let Some(value) = wf {
                    t.width_factor = value;
                }
                if let Some(value) = ob {
                    t.oblique_angle = value;
                }
            }
            E::MText(m) => {
                m.style = style.clone();
                m.height = height;
            }
            E::AttributeDefinition(a) => {
                a.text_style = style.clone();
                a.height = height;
                if let Some(value) = wf {
                    a.width_factor = value;
                }
                if let Some(value) = ob {
                    a.oblique_angle = value;
                }
            }
            E::AttributeEntity(a) => {
                a.text_style = style;
                a.height = height;
                if let Some(value) = wf {
                    a.width_factor = value;
                }
                if let Some(value) = ob {
                    a.oblique_angle = value;
                }
            }
            _ => {}
        }
    }
    // MText-only extras (line spacing, background fill).
    if let (E::MText(sm), E::MText(dm)) = (src, dst as &mut E) {
        dm.drawing_direction = sm.drawing_direction;
        dm.line_spacing_factor = sm.line_spacing_factor;
        dm.line_spacing_style = sm.line_spacing_style;
        dm.background_fill_flags = sm.background_fill_flags;
        dm.background_scale = sm.background_scale;
        dm.background_color = sm.background_color;
        dm.background_transparency = sm.background_transparency;
    }

    // Dimension style name crosses the three dim-styled families.
    let dim_style = match src {
        E::Dimension(d) => Some(d.base().style_name.clone()),
        E::Leader(l) => Some(l.dimension_style.clone()),
        E::Tolerance(t) => Some(t.dimension_style_name.clone()),
        _ => None,
    };
    if let Some(ds) = dim_style {
        match dst {
            E::Dimension(d) => d.base_mut().style_name = ds,
            E::Leader(l) => l.dimension_style = ds,
            E::Tolerance(t) => t.dimension_style_name = ds,
            _ => {}
        }
    }

    // Hatch pattern / gradient — everything but the boundary.
    if let (E::Hatch(sh), E::Hatch(dh)) = (src, dst as &mut E) {
        dh.pattern = sh.pattern.clone();
        dh.pattern_type = sh.pattern_type;
        dh.pattern_angle = sh.pattern_angle;
        dh.pattern_scale = sh.pattern_scale;
        dh.is_solid = sh.is_solid;
        dh.is_double = sh.is_double;
        dh.style = sh.style;
        dh.gradient_color = sh.gradient_color.clone();
    }

    // Polyline display style crosses lightweight and legacy 2D polylines.
    // Per-vertex/tapered widths are resampled over the destination vertices;
    // they must not be flattened into the source's constant-width field.
    if let Some(style) = PolylineMatchStyle::from_entity(src) {
        style.apply_to(dst);
    }

    if let (E::Leader(sl), E::Leader(dl)) = (src, dst as &mut E) {
        dl.arrow_enabled = sl.arrow_enabled;
        dl.path_type = sl.path_type;
        dl.hookline_direction = sl.hookline_direction;
        dl.hookline_enabled = sl.hookline_enabled;
        dl.override_color = sl.override_color;
        dl.dimension_gap = sl.dimension_gap;
        dl.arrowhead_type = sl.arrowhead_type;
        dl.arrow_size = sl.arrow_size;
        dl.byblock_color = sl.byblock_color;
    }
    if let (E::Tolerance(st), E::Tolerance(dt)) = (src, dst as &mut E) {
        dt.dimension_style_handle = st.dimension_style_handle;
        dt.text_height = st.text_height;
        dt.dimension_gap = st.dimension_gap;
    }

    // Paper-space geometry and view position stay; viewport display/plot
    // styling and effective scale follow the source.
    if let (E::Viewport(sv), E::Viewport(dv)) = (src, dst as &mut E) {
        let scale = crate::scene::vp_effective_scale(sv.custom_scale, sv.view_height, sv.height);
        dv.custom_scale = scale;
        if scale.abs() > 1e-9 {
            dv.view_height = dv.height / scale;
        }
        dv.status.locked = sv.status.locked;
        dv.status.hide_plot = sv.status.hide_plot;
        dv.render_mode = sv.render_mode;
        dv.style_sheet = sv.style_sheet.clone();
        dv.shade_plot_mode = sv.shade_plot_mode;
        dv.background_handle = sv.background_handle;
        dv.shade_plot_handle = sv.shade_plot_handle;
        dv.visual_style_handle = sv.visual_style_handle;
        dv.default_lighting = sv.default_lighting;
        dv.default_lighting_type = sv.default_lighting_type;
        dv.brightness = sv.brightness;
        dv.contrast = sv.contrast;
        dv.ambient_color = sv.ambient_color;
    }

    if let (E::Table(st), E::Table(dt)) = (src, dst as &mut E) {
        dt.table_style_handle = st.table_style_handle;
        dt.base_style = st.base_style.clone();
        dt.override_flag = st.override_flag;
        dt.override_border_color = st.override_border_color;
        dt.override_border_line_weight = st.override_border_line_weight;
        dt.override_border_visibility = st.override_border_visibility;
        dt.legacy_style_override = st.legacy_style_override.clone();
        dt.legacy_border_colors = st.legacy_border_colors.clone();
        dt.legacy_border_line_weights = st.legacy_border_line_weights.clone();
        dt.legacy_border_visibility = st.legacy_border_visibility.clone();
    }

    if let (E::MLine(sm), E::MLine(dm)) = (src, dst as &mut E) {
        dm.style_handle = sm.style_handle;
        dm.style_name = sm.style_name.clone();
        dm.style_element_count = sm.style_element_count;
        dm.justification = sm.justification;
        dm.scale_factor = sm.scale_factor;
        // Stored segment offsets bake the old style into each vertex. Empty
        // data deliberately selects the renderer's style-derived fallback.
        for vertex in &mut dm.vertices {
            vertex.segments.clear();
        }
    }

    // MultiLeader style + every style-affecting override; content and
    // geometry (leader points, text, block handle) stay.
    if let (E::MultiLeader(sm), E::MultiLeader(dm)) = (src, dst as &mut E) {
        dm.style_handle = sm.style_handle;
        dm.path_type = sm.path_type;
        dm.line_color = sm.line_color;
        dm.line_type_handle = sm.line_type_handle;
        dm.line_weight = sm.line_weight;
        dm.enable_landing = sm.enable_landing;
        dm.enable_dogleg = sm.enable_dogleg;
        dm.dogleg_length = sm.dogleg_length;
        dm.arrowhead_handle = sm.arrowhead_handle;
        dm.arrowhead_size = sm.arrowhead_size;
        dm.text_style_handle = sm.text_style_handle;
        dm.text_color = sm.text_color;
        dm.text_frame = sm.text_frame;
        dm.text_height = sm.text_height;
        dm.text_left_attachment = sm.text_left_attachment;
        dm.text_right_attachment = sm.text_right_attachment;
        dm.text_top_attachment = sm.text_top_attachment;
        dm.text_bottom_attachment = sm.text_bottom_attachment;
        dm.text_attachment_direction = sm.text_attachment_direction;
        dm.text_attachment_point = sm.text_attachment_point;
        dm.text_alignment = sm.text_alignment;
        dm.text_angle_type = sm.text_angle_type;
        dm.text_direction_negative = sm.text_direction_negative;
        dm.text_align_in_ipe = sm.text_align_in_ipe;
        dm.block_content_color = sm.block_content_color;
        dm.block_connection_type = sm.block_connection_type;
        dm.block_scale = sm.block_scale;
        dm.scale_factor = sm.scale_factor;
        dm.property_override_flags = sm.property_override_flags;
        dm.enable_annotation_scale = sm.enable_annotation_scale;
        dm.extend_leader_to_text = sm.extend_leader_to_text;
        dm.arrowhead_overrides = sm.arrowhead_overrides.clone();

        let sc = &sm.context;
        let dc = &mut dm.context;
        dc.scale_factor = sc.scale_factor;
        dc.text_height = sc.text_height;
        dc.text_width = sc.text_width;
        dc.text_boundary_height = sc.text_boundary_height;
        dc.line_spacing_factor = sc.line_spacing_factor;
        dc.line_spacing_style = sc.line_spacing_style;
        dc.text_color = sc.text_color;
        dc.text_attachment_point = sc.text_attachment_point;
        dc.text_flow_direction = sc.text_flow_direction;
        dc.text_alignment = sc.text_alignment;
        dc.text_left_attachment = sc.text_left_attachment;
        dc.text_right_attachment = sc.text_right_attachment;
        dc.text_top_attachment = sc.text_top_attachment;
        dc.text_bottom_attachment = sc.text_bottom_attachment;
        dc.text_height_automatic = sc.text_height_automatic;
        dc.word_break = sc.word_break;
        dc.text_style_handle = sc.text_style_handle;
        dc.block_content_scale = sc.block_content_scale;
        dc.block_content_color = sc.block_content_color;
        dc.block_connection_type = sc.block_connection_type;
        dc.column_type = sc.column_type;
        dc.column_width = sc.column_width;
        dc.column_gutter = sc.column_gutter;
        dc.column_flow_reversed = sc.column_flow_reversed;
        dc.column_sizes = sc.column_sizes.clone();
        dc.background_fill_enabled = sc.background_fill_enabled;
        dc.background_mask_fill_on = sc.background_mask_fill_on;
        dc.background_fill_color = sc.background_fill_color;
        dc.background_scale_factor = sc.background_scale_factor;
        dc.background_transparency = sc.background_transparency;
        dc.arrowhead_size = sc.arrowhead_size;
        dc.landing_gap = sc.landing_gap;
        dc.scale_handle = sc.scale_handle;
    }

    // External-reference identity, placement and clip geometry stay. Only
    // display controls/appearance are matched.
    if let (E::RasterImage(si), E::RasterImage(di)) = (src, dst as &mut E) {
        di.flags = si.flags;
        di.clipping_enabled = si.clipping_enabled;
        di.brightness = si.brightness;
        di.contrast = si.contrast;
        di.fade = si.fade;
        di.clip_boundary.clip_mode = si.clip_boundary.clip_mode;
    }
    if let (E::Wipeout(sw), E::Wipeout(dw)) = (src, dst as &mut E) {
        dw.flags = sw.flags;
        dw.clipping_enabled = sw.clipping_enabled;
        dw.brightness = sw.brightness;
        dw.contrast = sw.contrast;
        dw.fade = sw.fade;
        dw.clip_mode = sw.clip_mode;
    }
    if let (E::Underlay(su), E::Underlay(du)) = (src, dst as &mut E) {
        du.flags = su.flags;
        du.contrast = su.contrast;
        du.fade = su.fade;
        du.clip_inverted = su.clip_inverted;
    }
}

struct PolylineMatchStyle {
    plinegen: bool,
    widths: Vec<(f64, f64)>,
}

impl PolylineMatchStyle {
    fn from_entity(entity: &acadrust::EntityType) -> Option<Self> {
        use acadrust::EntityType as E;

        match entity {
            E::LwPolyline(poly) => {
                let widths = if poly.vertices.is_empty() {
                    vec![(poly.constant_width, poly.constant_width)]
                } else {
                    poly.vertices
                        .iter()
                        .map(|v| {
                            (
                                width_or_default(v.start_width, poly.constant_width),
                                width_or_default(v.end_width, poly.constant_width),
                            )
                        })
                        .collect()
                };
                Some(Self {
                    plinegen: poly.plinegen,
                    widths,
                })
            }
            E::Polyline2D(poly) => {
                let widths = if poly.vertices.is_empty() {
                    vec![(poly.start_width, poly.end_width)]
                } else {
                    poly.vertices
                        .iter()
                        .map(|v| {
                            (
                                width_or_default(v.start_width, poly.start_width),
                                width_or_default(v.end_width, poly.end_width),
                            )
                        })
                        .collect()
                };
                Some(Self {
                    plinegen: poly.flags.bits()
                        & acadrust::entities::PolylineFlags::LINETYPE_CONTINUOUS.bits()
                        != 0,
                    widths,
                })
            }
            _ => None,
        }
    }

    fn apply_to(&self, entity: &mut acadrust::EntityType) {
        use acadrust::EntityType as E;

        match entity {
            E::LwPolyline(poly) => {
                poly.plinegen = self.plinegen;
                let sampled = resample_widths(&self.widths, poly.vertices.len());
                let first = sampled.first().copied().unwrap_or((0.0, 0.0));
                let can_use_constant = widths_are_same(&sampled) && nearly_equal(first.0, first.1);
                poly.constant_width = if can_use_constant { first.0 } else { 0.0 };
                for (vertex, (start, end)) in poly.vertices.iter_mut().zip(sampled) {
                    if can_use_constant {
                        vertex.start_width = 0.0;
                        vertex.end_width = 0.0;
                    } else {
                        vertex.start_width = start;
                        vertex.end_width = end;
                    }
                }
            }
            E::Polyline2D(poly) => {
                let mut bits = poly.flags.bits();
                let flag = acadrust::entities::PolylineFlags::LINETYPE_CONTINUOUS.bits();
                if self.plinegen {
                    bits |= flag;
                } else {
                    bits &= !flag;
                }
                poly.flags = acadrust::entities::PolylineFlags::from_bits(bits);
                let sampled = resample_widths(&self.widths, poly.vertices.len());
                let first = sampled.first().copied().unwrap_or((0.0, 0.0));
                let can_use_defaults = widths_are_same(&sampled);
                if can_use_defaults {
                    poly.start_width = first.0;
                    poly.end_width = first.1;
                } else {
                    poly.start_width = 0.0;
                    poly.end_width = 0.0;
                }
                for (vertex, (start, end)) in poly.vertices.iter_mut().zip(sampled) {
                    if can_use_defaults {
                        vertex.start_width = 0.0;
                        vertex.end_width = 0.0;
                    } else {
                        vertex.start_width = start;
                        vertex.end_width = end;
                    }
                }
            }
            _ => {}
        }
    }
}

fn width_or_default(value: f64, default: f64) -> f64 {
    if value.abs() <= 1e-12 {
        default
    } else {
        value
    }
}

fn nearly_equal(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}

fn widths_are_same(widths: &[(f64, f64)]) -> bool {
    let Some(first) = widths.first() else {
        return true;
    };
    widths
        .iter()
        .all(|value| nearly_equal(value.0, first.0) && nearly_equal(value.1, first.1))
}

fn resample_widths(source: &[(f64, f64)], count: usize) -> Vec<(f64, f64)> {
    if count == 0 || source.is_empty() {
        return Vec::new();
    }
    if count == 1 || source.len() == 1 {
        return vec![source[0]; count];
    }
    (0..count)
        .map(|index| {
            let source_index = index.saturating_mul(source.len() - 1) / count.saturating_sub(1);
            source[source_index]
        })
        .collect()
}
