use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn on_ribbon_layer_changed(&mut self, layer: String) -> Task<Message> {
        let i = self.active_tab;
        self.ribbon.close_dropdown();
        let handles = self.property_target_handles(i);
        if handles.is_empty() {
            if self.has_property_selection(i) {
                return Task::none();
            }
            // No selection — change the creation default. Persist
            // into the tab's header (CLAYER) so it survives a tab
            // switch and rides the next save. #21.
            let handle = self.tabs[i]
                .scene
                .document
                .layers
                .get(&layer)
                .map(|l| l.handle)
                .unwrap_or(document::types::Handle::NULL);
            self.tabs[i].scene.document.header.current_layer_name = layer.clone();
            self.tabs[i].scene.document.header.current_layer_handle = handle;
            self.tabs[i].active_layer = layer.clone();
            self.tabs[i].layers.current_layer = layer.clone();
            self.tabs[i].dirty = true;
            self.ribbon.active_layer = layer;
        } else {
            // Apply to the selection; leave the creation default alone
            // ("Make current" is a separate action).
            // Layer drives by-layer colour/linetype/lineweight, which are
            // baked into the cached wire geometry — re-tessellate so the
            // change shows immediately (issue #231 class).
            self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                    crate::scene::view::dispatch::apply_common_prop(entity, "layer", &layer);
                }
            });
            self.ribbon.active_layer = layer;
        }
        Task::none()
    }

    pub(in crate::app) fn on_ribbon_color_changed(&mut self, color: AcadColor) -> Task<Message> {
        let i = self.active_tab;
        self.ribbon.prop_color_palette_open = false;
        self.ribbon.close_dropdown();
        self.note_recent_color(color);
        let handles = self.property_target_handles(i);
        if handles.is_empty() {
            if self.has_property_selection(i) {
                return Task::none();
            }
            // Persist the new default into the tab's header so it
            // round-trips through tab switches and writes back on
            // save (CECOLOR). #21.
            self.tabs[i].scene.document.header.current_entity_color = color;
            self.tabs[i].dirty = true;
            self.ribbon.active_color = color;
        } else {
            self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                    crate::scene::view::dispatch::apply_color(entity, color);
                }
            });
            self.ribbon.active_color = color;
        }
        Task::none()
    }

    pub(in crate::app) fn on_ribbon_lineweight_changed(
        &mut self,
        lw: document::types::LineWeight,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.ribbon.close_dropdown();
        let handles = self.property_target_handles(i);
        if handles.is_empty() {
            if self.has_property_selection(i) {
                return Task::none();
            }
            // Persist into the tab's header (CELWEIGHT). #21.
            self.tabs[i].scene.document.header.current_line_weight = lw.value();
            self.tabs[i].dirty = true;
            self.ribbon.active_lineweight = lw;
        } else {
            // Lineweight is baked into the cached wire geometry —
            // re-tessellate so the change shows immediately (issue #231
            // class).
            self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                    crate::scene::view::dispatch::apply_line_weight(entity, lw);
                }
            });
            self.ribbon.active_lineweight = lw;
        }
        Task::none()
    }

    pub(in crate::app) fn on_ribbon_linetype_changed(&mut self, lt: String) -> Task<Message> {
        let i = self.active_tab;
        self.ribbon.close_dropdown();
        let handles = self.property_target_handles(i);
        if handles.is_empty() {
            if self.has_property_selection(i) {
                return Task::none();
            }
            // Persist into the tab's header (CELTYPE). Resolve to a
            // handle when the name matches a line_types entry so the
            // handle-based lookup stays in sync. #21.
            let handle = self.tabs[i]
                .scene
                .document
                .line_types
                .iter()
                .find(|x| x.name.eq_ignore_ascii_case(&lt))
                .map(|x| x.handle)
                .unwrap_or(document::types::Handle::NULL);
            self.tabs[i].scene.document.header.current_linetype_name = lt.clone();
            self.tabs[i].scene.document.header.current_linetype_handle = handle;
            self.tabs[i].dirty = true;
            self.ribbon.active_linetype = lt;
        } else {
            // Linetype is baked into the cached wire geometry —
            // re-tessellate so the dashed/solid look updates immediately
            // (issue #231 class).
            self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                    crate::scene::view::dispatch::apply_common_prop(entity, "linetype", &lt);
                }
            });
            self.ribbon.active_linetype = lt;
        }
        Task::none()
    }

    pub(in crate::app) fn on_ribbon_style_changed(
        &mut self,
        key: crate::modules::StyleKey,
        name: String,
    ) -> Task<Message> {
        use crate::modules::StyleKey;
        self.ribbon.close_dropdown();
        match key {
            StyleKey::TextStyle => {
                self.ribbon.active_text_style = name.clone();
                let i = self.active_tab;
                let found = self.tabs[i]
                    .scene
                    .document
                    .text_styles
                    .iter()
                    .find(|s| s.name == name)
                    .map(|ts| ts.handle);
                if let Some(h) = found {
                    self.tabs[i].scene.document.header.current_text_style_handle = h;
                    self.tabs[i].scene.document.header.current_text_style_name = name;
                }
            }
            StyleKey::DimStyle => {
                self.ribbon.active_dim_style = name.clone();
                let i = self.active_tab;
                let found = self.tabs[i]
                    .scene
                    .document
                    .dim_styles
                    .get(&name)
                    .map(|ds| ds.handle);
                if let Some(h) = found {
                    self.tabs[i].scene.document.header.current_dimstyle_handle = h;
                    self.tabs[i].scene.document.header.current_dimstyle_name = name;
                    crate::scene::creation_style::sync_header_dim_vars(
                        &mut self.tabs[i].scene.document,
                    );
                }
            }
            StyleKey::MLeaderStyle => {
                let i = self.active_tab;

                self.ribbon.active_mleader_style = name.clone();
                self.tabs[i].active_mleader_style = name.clone();

                self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_mleader_style_name = name;

                self.tabs[i].dirty = true;
            }
            StyleKey::TableStyle => {
                self.ribbon.active_table_style = name.clone();
                let i = self.active_tab;
                let found = self.tabs[i].scene.document.objects.values().any(
                    |o| matches!(o, document::objects::ObjectType::TableStyle(ts) if ts.name == name),
                );
                if found {
                    self.tabs[i].scene.document.header.current_table_style_name = name;
                    self.tabs[i].dirty = true;
                }
            }
        }
        Task::none()
    }

    pub(in crate::app) fn on_prop_hatch_pattern_changed(&mut self, name: String) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.property_target_handles(i);
        self.apply_hatch_pattern(i, &handles, name)
    }

    pub(in crate::app) fn apply_hatch_pattern(
        &mut self,
        i: usize,
        handles: &[document::Handle],
        name: String,
    ) -> Task<Message> {
        if !handles.is_empty() {
            use crate::scene::model::hatch_patterns;
            if let Some(entry) = hatch_patterns::find(&name) {
                self.push_undo_snapshot(i, "HATCHEDIT");
                for &handle in handles {
                    if let Some(document::EntityType::Hatch(dxf)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        let mut pattern = hatch_patterns::build_dxf_pattern(entry);
                        // Stored pattern lines are final world-space
                        // geometry. Preserve the selected hatch's scale,
                        // angle and origin when replacing the catalog
                        // pattern.
                        crate::entities::hatch::scale_pattern_geometry(
                            &mut pattern,
                            dxf.pattern_scale,
                        );
                        crate::entities::hatch::rotate_pattern_geometry(
                            &mut pattern,
                            dxf.pattern_angle,
                        );
                        let origin = dxf.pattern_origin();
                        crate::entities::hatch::translate_pattern_geometry(
                            &mut pattern,
                            origin.x,
                            origin.y,
                        );
                        dxf.pattern = pattern;
                        dxf.is_solid = matches!(
                            entry.gpu,
                            crate::scene::model::hatch_model::HatchPattern::Solid
                        );
                        dxf.pattern_type = document::entities::HatchPatternType::Predefined;
                        dxf.gradient_color.enabled = false;
                    }
                    if let Some(model) = self.tabs[i].scene.hatches.get_mut(&handle) {
                        model.pattern = entry.gpu.clone();
                        model.name = name.clone();
                    }
                }
                self.invalidate_property_targets(i, handles);
                self.tabs[i].dirty = true;
                self.refresh_properties();
            }
        }
        Task::none()
    }

    pub(in crate::app) fn on_prop_geom_choice_changed(
        &mut self,
        field: &'static str,
        value: String,
    ) -> Task<Message> {
        let value = if field == "transparency" {
            let Some(transparency) =
                crate::scene::creation_style::parse_current_transparency(&value)
            else {
                self.command_line.push_error(
                    "Transparency: expected ByLayer, ByBlock, or an integer from 0 to 90.",
                );
                return Task::none();
            };
            let panel = &mut self.tabs[self.active_tab].properties;
            panel
                .edit_buf
                .remove(&crate::ui::properties::FieldKey::Geom(field));
            panel.edit_choice_open = None;
            crate::scene::creation_style::current_transparency_label(transparency)
        } else {
            value
        };
        if field == "authoring_alignment_type" {
            return self.set_authoring_alignment_type(&value);
        }
        if field == "authoring_base_location" {
            return self.set_authoring_base_location(&value);
        }
        if field.starts_with("bedit_") {
            return self.set_block_property(field, &value);
        }
        if matches!(field, "dyn_constraint_form" | "dyn_constraint_reference") {
            return self.on_dynamic_dimension_choice(field, &value);
        }
        let i = self.active_tab;
        // Match orientation to layout: an annotative text's AcadAnnoPO flag.
        if field == "anno_po" {
            let on = value == crate::t!("Yes").as_ref() || value.eq_ignore_ascii_case("Yes");
            let handles = self.property_target_handles(i);
            self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                let document = &mut app.tabs[i].scene.document;
                crate::scene::block_metadata::ensure_application(document, "AcadAnnoPO");
                crate::scene::view::dispatch::set_entity_xdata(
                    document,
                    handle,
                    "AcadAnnoPO",
                    on.then(|| vec![document::xdata::XDataValue::Integer16(1)]),
                );
            });
            self.tabs[i].properties.edit_choice_open = None;
            self.refresh_properties();
            return Task::none();
        }
        if field == "ole_lock_aspect" {
            self.set_ole_lock_aspect(i, &value);
            self.tabs[i].properties.edit_choice_open = None;
            return Task::none();
        }
        if field == "ext_pcx_locked" {
            let handles = self.point_cloud_lock_targets(i);
            self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                if let Some(document::EntityType::Extended(extended)) =
                    app.tabs[i].scene.document.get_entity_mut(handle)
                {
                    crate::entities::traits::PropertyEditable::apply_geom_prop(
                        extended, field, &value,
                    );
                }
            });
            self.tabs[i].properties.edit_choice_open = None;
            return Task::none();
        }
        let handles = self.property_target_handles(i);
        if crate::app::properties::field_moves_geometry(field)
            && self.any_entity_is_fixed(i, &handles)
        {
            self.command_line
                .push_error(crate::app::properties::HELD_BY_FIXED);
            self.tabs[i].properties.edit_choice_open = None;
            return Task::none();
        }
        // The Annotative Yes/No list drives the per-object annotative toggle
        // (MTEXT's own flag, the annotation context for the rest).
        if field == "annotative" {
            let wanted = value == crate::t!("Yes").as_ref() || value.eq_ignore_ascii_case("yes");
            let Some(handle) = handles.first().copied() else {
                return Task::none();
            };
            let document = &self.tabs[i].scene.document;
            let (toggle, current) = match document.get_entity(handle) {
                Some(document::EntityType::MText(text)) => ("is_annotative", text.is_annotative),
                // The same per-object state the row shows and the toggle
                // flips; a dimension's style only seeds it. (#1917)
                Some(entity) => (
                    "annotative_ctx",
                    crate::scene::annotative::is_annotative(document, entity),
                ),
                None => return Task::none(),
            };
            if wanted != current {
                return self.update(Message::PropBoolToggle(toggle));
            }
            return Task::none();
        }

        if !handles.is_empty() {
            if matches!(
                field,
                "spline_method"
                    | "knot_param"
                    | "cv_frame"
                    | "fill_type"
                    | "gradient_type"
                    | "style"
                    | "pattern_type_label"
            ) {
                let unchanged = handles.iter().all(|handle| {
                    match self.tabs[i].scene.document.get_entity(*handle) {
                        Some(document::EntityType::Spline(spline)) => match field {
                            "spline_method" => {
                                value
                                    == if crate::entities::spline::shows_fit_points(spline) {
                                        "Fit"
                                    } else {
                                        "Control Vertices"
                                    }
                            }
                            "knot_param" => {
                                let current = match spline.knot_parameterization {
                                    0 => "Chord",
                                    1 => "Square Root",
                                    2 => "Uniform",
                                    _ => "Custom",
                                };
                                value == current
                            }
                            "cv_frame" => {
                                value
                                    == if spline.cv_frame_visible {
                                        "Show"
                                    } else {
                                        "Hide"
                                    }
                            }
                            _ => false,
                        },
                        Some(document::EntityType::Hatch(hatch)) => match field {
                            "fill_type" => {
                                value
                                    == if hatch.gradient_color.is_single_color {
                                        "One color"
                                    } else {
                                        "Two color"
                                    }
                            }
                            "gradient_type" => {
                                let (kind, inverted) =
                                    crate::scene::model::hatch_model::GradientKind::from_name(
                                        &hatch.gradient_color.name,
                                    );
                                value == kind.choice_label(inverted)
                            }
                            "style" => {
                                value
                                    == match hatch.style {
                                        document::entities::HatchStyleType::Normal => "Normal",
                                        document::entities::HatchStyleType::Outer => "Outer",
                                        document::entities::HatchStyleType::Ignore => "Ignore",
                                    }
                            }
                            "pattern_type_label" => {
                                value
                                    == match hatch.pattern_type {
                                        document::entities::HatchPatternType::Predefined => {
                                            "Predefined"
                                        }
                                        document::entities::HatchPatternType::UserDefined => {
                                            "User Defined"
                                        }
                                        document::entities::HatchPatternType::Custom => "Custom",
                                    }
                            }
                            _ => false,
                        },
                        _ => false,
                    }
                });
                if unchanged {
                    return Task::none();
                }
            }
            // CTB mode has no named plot styles, so the `plot_style` arm
            // below is a no-op early return. Guard here — before the shared
            // snapshot — so the no-op path pushes no undo entry. (No other
            // arm returns early after the snapshot, so nothing else depends
            // on it existing at this point.)
            if field == "plot_style" && self.tabs[i].scene.document.header.plotstyle_mode {
                self.refresh_properties();
                return Task::none();
            }
            if field == "transparency" {
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if app.tabs[i].scene.is_layer_locked(handle) {
                        return;
                    }
                    if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                        crate::scene::view::dispatch::apply_common_prop(
                            entity,
                            "transparency",
                            &value,
                        );
                    }
                });
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == "material" {
                // Material source: ByLayer / ByBlock clear the handle; a
                // named material sets flag 3 + its handle (resolved here
                // because the update loop holds the document).
                let mat_handle: Option<document::Handle> = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .iter()
                    .find_map(|(h, o)| match o {
                        document::objects::ObjectType::Material(m) if m.name == value => Some(*h),
                        _ => None,
                    });
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if app.tabs[i].scene.is_layer_locked(handle) {
                        return;
                    }
                    if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                        let common = entity.common_mut();
                        match value.as_str() {
                            "ByLayer" => {
                                common.material_flags = 0;
                                common.material_handle = None;
                            }
                            "ByBlock" => {
                                common.material_flags = 1;
                                common.material_handle = None;
                            }
                            "Global" => {
                                common.material_flags = 2;
                                common.material_handle = None;
                            }
                            _ => {
                                if let Some(h) = mat_handle {
                                    common.material_flags = 3;
                                    common.material_handle = Some(h);
                                }
                            }
                        }
                    }
                });
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == "tol_text_style" {
                use crate::entities::dim_override as dov;
                use document::xdata::XDataValue;
                let style_handle = self.tabs[i]
                    .scene
                    .document
                    .text_styles
                    .iter()
                    .find(|entry| entry.name.eq_ignore_ascii_case(&value))
                    .map(|entry| entry.handle);
                if let Some(style_handle) = style_handle {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if app.tabs[i].scene.is_layer_locked(handle)
                            || !matches!(
                                app.tabs[i].scene.document.get_entity(handle),
                                Some(document::EntityType::Tolerance(_))
                            )
                        {
                            return;
                        }
                        dov::set(
                            &mut app.tabs[i].scene.document,
                            handle,
                            dov::DIMTXSTY,
                            Some(XDataValue::Handle(style_handle)),
                        );
                    });
                } else {
                    self.invalidate_property_targets(i, &handles);
                    self.refresh_properties();
                }
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field.starts_with("dim_") {
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if app.tabs[i].scene.is_layer_locked(handle) {
                        return;
                    }
                    if matches!(
                        app.tabs[i].scene.document.get_entity(handle),
                        Some(document::EntityType::Dimension(_))
                    ) {
                        let applied = crate::entities::dim_override::set_property(
                            &mut app.tabs[i].scene.document,
                            handle,
                            field,
                            &value,
                        );
                        if applied && field == "dim_text_inside" {
                            let radial = app.tabs[i]
                                .scene
                                .document
                                .get_entity_mut(handle)
                                .and_then(crate::entities::dimension::as_dimension_mut);
                            if let Some(document::entities::Dimension::LargeRadial(dimension)) =
                                radial
                            {
                                dimension.base.text_user_positioned = false;
                            }
                        }
                    }
                });
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == "vscale_std" {
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if matches!(
                        app.tabs[i].scene.document.get_entity(handle),
                        Some(document::EntityType::Viewport(_))
                    ) {
                        let _ = app.tabs[i]
                            .scene
                            .set_viewport_scale_named_for(handle, &value);
                    }
                });
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == "vp_ucs_name" {
                // Resolve UCS name → cloned data, then mutate viewports.
                let ucs_data = self.tabs[i]
                    .scene
                    .document
                    .ucss
                    .iter()
                    .find(|u| u.name == value)
                    .cloned();
                if let Some(ucs) = ucs_data {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if let Some(document::EntityType::Viewport(vp)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            vp.ucs_handle = ucs.handle;
                            vp.ucs_origin = ucs.origin;
                            vp.ucs_x_axis = ucs.x_axis;
                            vp.ucs_y_axis = ucs.y_axis;
                            vp.ucs_per_viewport = true;
                        }
                    });
                } else {
                    self.invalidate_property_targets(i, &handles);
                    self.refresh_properties();
                }
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == "vp_named_view" {
                // Assign a named view to viewport(s): copy camera parameters.
                let view_data = self.tabs[i]
                    .scene
                    .document
                    .views
                    .iter()
                    .find(|v| v.name == value)
                    .cloned();
                if let Some(view) = view_data {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if let Some(document::EntityType::Viewport(vp)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            vp.view_target = view.target;
                            vp.view_direction = view.direction;
                            if view.height > 0.0 {
                                vp.view_height = view.height;
                            }
                        }
                    });
                    self.tabs[i].scene.camera_generation += 1;
                } else {
                    self.invalidate_property_targets(i, &handles);
                    self.refresh_properties();
                }
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if crate::scene::model::solid_history::is_loft_geometry_choice(field) {
                // These choices change the generated body, not just history
                // flags. Use the same transactional rebuild as numeric edits.
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if app.tabs[i].scene.is_layer_locked(handle) {
                        return;
                    }
                    app.tabs[i]
                        .scene
                        .apply_solid_history_property(handle, field, &value);
                });
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == crate::scene::model::solid_history::PROP_OPERAND {
                // Which solid of a composite to edit: palette state only.
                for handle in &handles {
                    crate::scene::model::solid_history::select_operand(
                        &self.tabs[i].scene.document,
                        *handle,
                        &value,
                    );
                }
                self.tabs[i].properties.edit_choice_open = None;
                self.refresh_properties();
                return Task::none();
            }
            if crate::scene::model::solid_history::is_history_choice(field) {
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if app.tabs[i].scene.is_layer_locked(handle) {
                        return;
                    }
                    app.tabs[i]
                        .scene
                        .apply_solid_history_choice(handle, field, &value);
                });
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if crate::scene::model::solid_history::is_surface_property_choice(field) {
                use crate::scene::model::solid_history::{
                    PROP_SURFACE_MAINTAIN_ASSOCIATIVITY, PROP_SURFACE_SHOW_ASSOCIATIVITY,
                    PROP_SURFACE_WIREFRAME_TYPE,
                };
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if app.tabs[i].scene.is_layer_locked(handle) {
                        return;
                    }
                    let Some(mut state) = app.tabs[i].scene.document.get_entity(handle).and_then(
                        |entity| match entity {
                            document::EntityType::Surface(surface) => {
                                Some(crate::entities::solid3d::surface_property_state(surface))
                            }
                            _ => None,
                        },
                    ) else {
                        return;
                    };
                    match field {
                        PROP_SURFACE_WIREFRAME_TYPE => {
                            state.isolines = !value.eq_ignore_ascii_case("Isoparms")
                        }
                        PROP_SURFACE_MAINTAIN_ASSOCIATIVITY => {
                            state.maintain_associativity = value.eq_ignore_ascii_case("Yes")
                        }
                        PROP_SURFACE_SHOW_ASSOCIATIVITY => {
                            state.show_associativity = value.eq_ignore_ascii_case("Yes")
                        }
                        _ => return,
                    }
                    crate::scene::view::dispatch::set_entity_xdata(
                        &mut app.tabs[i].scene.document,
                        handle,
                        crate::entities::solid3d::SURFACE_PROPERTIES_APP,
                        Some(crate::entities::solid3d::surface_property_xdata_values(
                            state,
                        )),
                    );
                    if field == PROP_SURFACE_WIREFRAME_TYPE {
                        app.tabs[i].scene.reseed_derived_caches(handle);
                    }
                });
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == "tol_dim_style" {
                let style = self.tabs[i]
                    .scene
                    .document
                    .dim_styles
                    .iter()
                    .find(|entry| entry.name.eq_ignore_ascii_case(&value))
                    .map(|entry| (entry.handle, entry.name.clone(), entry.annotative));
                if let Some((style_handle, style_name, annotative)) = style {
                    let scale = self.tabs[i].scene.creation_annotation_scale_handle();
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if app.tabs[i].scene.is_layer_locked(handle) {
                            return;
                        }
                        if let Some(document::EntityType::Tolerance(tolerance)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            tolerance.dimension_style_handle = Some(style_handle);
                            tolerance.dimension_style_name = style_name.clone();
                        } else {
                            return;
                        }
                        crate::scene::annotative::set_entity_annotative(
                            &mut app.tabs[i].scene.document,
                            handle,
                            annotative,
                        );
                        if annotative {
                            if let Some(scale) = scale {
                                crate::scene::annotative::create_annotation_context(
                                    &mut app.tabs[i].scene.document,
                                    handle,
                                    scale,
                                );
                            }
                        }
                    });
                } else {
                    self.invalidate_property_targets(i, &handles);
                    self.refresh_properties();
                }
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == "tbl_cell_border_lt" {
                let linetype = self.tabs[i]
                    .scene
                    .document
                    .line_types
                    .iter()
                    .find(|lt| lt.name.eq_ignore_ascii_case(value.trim()))
                    .map(|lt| lt.handle);
                let cells = self.picked_cell_positions(i);
                if linetype.is_some() {
                    self.apply_property_op(i, "TABLE CELL BORDER", &handles, |app, handle| {
                        use crate::entities::table::TableCellBordersExt;
                        if let Some(document::EntityType::Table(table)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            for &(row, column) in &cells {
                                table.edit_cell_borders(row, column, |border| {
                                    border.line_type_handle = linetype;
                                    document::entities::table::BorderPropertyFlags::LINE_TYPE
                                });
                            }
                            table.block_record_handle = None;
                        }
                    });
                }
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            if field == "tbl_style_handle" {
                let style_handle =
                    self.tabs[i]
                        .scene
                        .document
                        .objects
                        .iter()
                        .find_map(|(handle, object)| match object {
                            document::objects::ObjectType::TableStyle(style)
                                if style.name.eq_ignore_ascii_case(value.trim()) =>
                            {
                                Some(*handle)
                            }
                            _ => None,
                        });
                if let Some(style_handle) = style_handle {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if let Some(document::EntityType::Table(table)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            table.table_style_handle = Some(style_handle);
                            // Shared-tail `tbl_` epilogue, now local: drop
                            // stale block_record refs so the new style takes
                            // effect (same final state as style loop + tail
                            // clearing, independent fields).
                            table.block_record_handle = None;
                        }
                    });
                } else {
                    // Unresolvable name: preserve the old tail's
                    // block_record clearing without pushing a snapshot.
                    for &handle in &handles {
                        if let Some(document::EntityType::Table(table)) =
                            self.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            table.block_record_handle = None;
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
                    self.refresh_properties();
                }
                self.tabs[i].properties.edit_choice_open = None;
                return Task::none();
            }
            self.push_undo_snapshot(i, "CHPROP");

            if matches!(
                field,
                "mleader_style"
                    | "text_style_handle"
                    | "arrowhead_handle"
                    | "line_type_handle"
                    | "block_content_handle"
            ) {
                // Resolve a picked name back to the handle the MLEADER
                // stores. The style/text-style rows keep their existing
                // handle on a failed lookup; the arrowhead/linetype rows
                // take the resolved value directly (None = the default
                // "Closed filled" / "ByBlock" option).
                // A standard arrowhead's block is created on first use.
                let picked_arrow = (field == "arrowhead_handle")
                    .then(|| {
                        crate::entities::arrowheads::resolve(
                            &mut self.tabs[i].scene.document,
                            &value,
                        )
                    })
                    .flatten()
                    .filter(|h| !h.is_null());
                let doc = &self.tabs[i].scene.document;
                let resolved_mleader_style = (field == "mleader_style")
                    .then(|| {
                        doc.objects.values().find_map(|object| match object {
                            document::objects::ObjectType::MultiLeaderStyle(style)
                                if style.name == value =>
                            {
                                Some(style.clone())
                            }
                            _ => None,
                        })
                    })
                    .flatten();
                let resolved: Option<document::Handle> = match field {
                    "mleader_style" => resolved_mleader_style.as_ref().map(|style| style.handle),
                    "text_style_handle" => doc
                        .text_styles
                        .iter()
                        .find(|s| s.name == value)
                        .map(|s| s.handle),
                    "arrowhead_handle" => picked_arrow,
                    "line_type_handle" => {
                        if value == "ByBlock" {
                            None
                        } else {
                            doc.line_types
                                .iter()
                                .find(|l| l.name == value)
                                .map(|l| l.handle)
                        }
                    }
                    "block_content_handle" => doc
                        .block_records
                        .iter()
                        .find(|block| block.name == value)
                        .map(|block| block.handle),
                    _ => None,
                };
                for &handle in &handles {
                    if self.tabs[i].scene.is_layer_locked(handle) {
                        continue;
                    }
                    let mut style_annotation = None;
                    if field == "mleader_style" {
                        if let Some(style) = &resolved_mleader_style {
                            crate::scene::annotative::apply_mleader_style_to_object(
                                &mut self.tabs[i].scene.document,
                                handle,
                                style,
                            );
                            style_annotation = Some(style.is_annotative);
                        }
                    } else if let Some(document::EntityType::MultiLeader(ml)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        match field {
                            "text_style_handle" => {
                                if let Some(h) = resolved {
                                    ml.text_style_handle = Some(h);
                                    ml.context.text_style_handle = Some(h);
                                    ml.property_override_flags.insert(
                                                document::entities::MultiLeaderPropertyOverrideFlags::TEXT_STYLE,
                                            );
                                }
                            }
                            "arrowhead_handle" => {
                                ml.arrowhead_handle = resolved;
                                for root in &mut ml.context.leader_roots {
                                    for line in &mut root.lines {
                                        line.arrowhead_handle = resolved;
                                        line.override_flags.insert(
                                                    document::entities::LeaderLinePropertyOverrideFlags::ARROWHEAD,
                                                );
                                    }
                                }
                                ml.property_override_flags.insert(
                                    document::entities::MultiLeaderPropertyOverrideFlags::ARROWHEAD,
                                );
                            }
                            "line_type_handle" => {
                                ml.line_type_handle = resolved;
                                for root in &mut ml.context.leader_roots {
                                    for line in &mut root.lines {
                                        line.line_type_handle = resolved;
                                        line.override_flags.insert(
                                                    document::entities::LeaderLinePropertyOverrideFlags::LINE_TYPE,
                                                );
                                    }
                                }
                                ml.property_override_flags.insert(
                                            document::entities::MultiLeaderPropertyOverrideFlags::LEADER_LINE_TYPE,
                                        );
                            }
                            "block_content_handle" => {
                                if let Some(block_handle) = resolved {
                                    ml.block_content_handle = Some(block_handle);
                                    ml.context.block_content_handle = Some(block_handle);
                                    ml.context.has_block_contents = true;
                                    ml.context.block_content_location =
                                        ml.context.content_base_point;
                                    ml.property_override_flags.insert(
                                                document::entities::MultiLeaderPropertyOverrideFlags::BLOCK_CONTENT,
                                            );
                                }
                            }
                            _ => {}
                        }
                    }
                    if let Some(annotative) = style_annotation {
                        if annotative {
                            if let Some(scale) =
                                self.tabs[i].scene.creation_annotation_scale_handle()
                            {
                                crate::scene::annotative::create_annotation_context(
                                    &mut self.tabs[i].scene.document,
                                    handle,
                                    scale,
                                );
                            }
                        } else {
                            crate::scene::annotative::clear_annotation_context(
                                &mut self.tabs[i].scene.document,
                                handle,
                            );
                        }
                    }
                }
            } else if matches!(field, "arrow_block" | "dim_line_lw" | "text_pos_vert") {
                // Leader dim-var overrides picked from a dropdown. The
                // arrow block resolves a friendly arrowhead name to a
                // block handle ("Closed filled" clears the override →
                // reverts to the style); the other two map a label to
                // the DIMLWD / DIMTAD enum value.
                use crate::entities::dim_override as dov;
                use document::xdata::XDataValue;
                let arrow_h: Option<document::Handle> =
                    if field == "arrow_block" && value != "Closed filled" {
                        self.tabs[i]
                            .scene
                            .document
                            .block_records
                            .iter()
                            .find(|b| crate::app::properties::arrowhead_label(&b.name) == value)
                            .map(|b| b.handle)
                    } else {
                        None
                    };
                for &handle in &handles {
                    if self.tabs[i].scene.is_layer_locked(handle) {
                        continue;
                    }
                    // Only leaders carry these rows; skip anything else so
                    // a multi-type selection can't get stray overrides.
                    if !matches!(
                        self.tabs[i].scene.document.get_entity(handle),
                        Some(document::EntityType::Leader(_))
                    ) {
                        continue;
                    }
                    let doc = &mut self.tabs[i].scene.document;
                    match field {
                        "dim_line_lw" => dov::set(
                            doc,
                            handle,
                            dov::DIMLWD,
                            Some(XDataValue::Integer16(
                                crate::app::properties::dim_lineweight_from_label(&value),
                            )),
                        ),
                        "text_pos_vert" => dov::set(
                            doc,
                            handle,
                            dov::DIMTAD,
                            Some(XDataValue::Integer16(
                                crate::app::properties::dimtad_from_label(&value),
                            )),
                        ),
                        "arrow_block" => {
                            // "Closed filled" is an explicit override to
                            // the null-handle default arrow (not a clear),
                            // so the pick sticks even when the style's
                            // arrow differs.
                            if value == "Closed filled" {
                                dov::set(
                                    doc,
                                    handle,
                                    dov::DIMLDRBLK,
                                    Some(XDataValue::Handle(document::Handle::NULL)),
                                );
                            } else if let Some(h) = arrow_h {
                                dov::set(doc, handle, dov::DIMLDRBLK, Some(XDataValue::Handle(h)));
                            }
                        }
                        _ => {}
                    }
                }
            } else if field == "dyn_visibility" {
                // A visibility state picked in Properties, applied like a
                // pick from the reference's visibility grip.
                self.tabs[i].properties.edit_choice_open = None;
                for &handle in &handles {
                    let state = self.tabs[i]
                        .scene
                        .document
                        .dynamic_visibility_for_insert(handle)
                        .and_then(|(_, param)| param.states.iter().position(|s| s.name == value));
                    if let Some(state) = state {
                        self.apply_visibility_state(handle, state);
                    }
                }
            } else if field == "dyn_lookup" {
                self.tabs[i].properties.edit_choice_open = None;
                let mut changed = false;
                for &handle in &handles {
                    if self.tabs[i].scene.is_layer_locked(handle) {
                        continue;
                    }
                    let result = if let Some(test) = self.tabs[i]
                        .block_test
                        .as_ref()
                        .filter(|test| test.insert == handle)
                    {
                        crate::scene::block_lookup::evaluate_test_choice(
                            &self.tabs[i].scene.document,
                            &test.baseline,
                            handle,
                            &value,
                        )
                    } else {
                        crate::scene::block_lookup::evaluate_reference_choice(
                            &self.tabs[i].scene.document,
                            handle,
                            &value,
                        )
                    };
                    match result {
                        Ok(Some(staged)) => {
                            self.apply_action_document_and_bump(i, staged);
                            self.tabs[i].dirty = true;
                            changed = true;
                        }
                        Ok(None) => {}
                        Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
                    }
                }
                if changed {
                    self.finish_pending_history(i);
                }
            } else if field == "block" {
                // Name dropdown on a block reference: re-point the
                // selected inserts to the picked definition. A stale
                // typed value in the text buffer would mask the pick,
                // so drop it; the pick also closes the list.
                self.tabs[i]
                    .properties
                    .edit_buf
                    .remove(&crate::ui::properties::FieldKey::Geom("block"));
                self.tabs[i].properties.edit_choice_open = None;
                let canon = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&value)
                    .map(|br| br.name.clone());
                if let Some(canon) = canon {
                    let to = self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .get(&canon)
                        .map(|record| record.handle);
                    if let Some(to) = to {
                        let mut planned = Vec::new();
                        for &handle in &handles {
                            if self.tabs[i].scene.is_layer_locked(handle) {
                                continue;
                            }
                            let Some((from, differs)) = self.tabs[i]
                                .scene
                                .document
                                .get_entity(handle)
                                .and_then(|entity| match entity {
                                    document::EntityType::Insert(ins) => {
                                        let record = self.tabs[i]
                                            .scene
                                            .document
                                            .block_records
                                            .get(&ins.block_name)?;
                                        // Compare what the row shows: an
                                        // evaluated copy already names this
                                        // definition, so re-picking it asks
                                        // for no change.
                                        let effective =
                                            crate::app::control::block_ops::resolve_effective_block(
                                                &self.tabs[i].scene.document,
                                                ins,
                                            )
                                            .1;
                                        Some((record.handle, effective != Some(to)))
                                    }
                                    _ => None,
                                })
                            else {
                                continue;
                            };
                            if differs {
                                planned.push((handle, from));
                            }
                        }
                        if !planned.is_empty() {
                            let mut changes = Vec::new();
                            for (handle, from) in planned {
                                // An evaluated copy must stop resolving
                                // through its old definition before it takes
                                // the new name.
                                crate::app::control::block_ops::detach_representation(
                                    &mut self.tabs[i].scene.document,
                                    handle,
                                );
                                self.tabs[i]
                                    .scene
                                    .repoint_representation(handle, from, to, &canon);
                                changes.push((handle, crate::scene::ChangeKind::Modified));
                            }
                            self.tabs[i].scene.bump_entities(&changes);
                        }
                    }
                }
            } else if field == "plot_style" {
                // Named plot-style pick: ByLayer / ByBlock clear the
                // handle; a named style resolves through the drawing's
                // ACAD_PLOTSTYLENAME dictionary to its placeholder handle.
                let ph: Option<document::Handle> =
                    crate::app::properties::plot_style_entries(&self.tabs[i].scene.document)
                        .and_then(|e| e.iter().find(|(n, _)| *n == value).map(|(_, h)| *h));
                for &handle in &handles {
                    if self.tabs[i].scene.is_layer_locked(handle) {
                        continue;
                    }
                    if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                        let common = entity.common_mut();
                        match value.as_str() {
                            "ByLayer" => {
                                common.plotstyle_flags = 0;
                                common.plotstyle_handle = None;
                            }
                            "ByBlock" => {
                                common.plotstyle_flags = 1;
                                common.plotstyle_handle = None;
                            }
                            "Normal" => {
                                common.plotstyle_flags = 2;
                                common.plotstyle_handle = None;
                            }
                            _ => {
                                if let Some(h) = ph {
                                    common.plotstyle_flags = 3;
                                    common.plotstyle_handle = Some(h);
                                }
                            }
                        }
                    }
                }
            } else {
                let plane = if self.tabs[i].editing_model_space() {
                    self.tabs[i].ucs_xform().working_plane()
                } else {
                    crate::command::WorkingPlane::default()
                };
                // A text taking a style takes what the style fixes: its height
                // when the style sets one, its width factor and oblique angle.
                // An annotative style's height is on paper: the text takes it
                // at the current annotation scale.
                let text_style = (field == "style")
                    .then(|| self.tabs[i].scene.document.text_styles.get(&value).cloned())
                    .flatten()
                    .map(|mut style| {
                        if style.annotative {
                            style.height *= self.tabs[i].scene.creation_annotation_multiplier();
                        }
                        style
                    });
                // A dimension taking a style takes its annotative state, as a
                // new dimension would. (#1917)
                let dim_style_annotative = (field == "style_name")
                    .then(|| {
                        self.tabs[i]
                            .scene
                            .document
                            .dim_styles
                            .iter()
                            .find(|style| style.name.eq_ignore_ascii_case(&value))
                            .map(|style| style.annotative)
                    })
                    .flatten();
                let dim_scale = dim_style_annotative
                    .filter(|annotative| *annotative)
                    .and_then(|_| self.tabs[i].scene.creation_annotation_scale_handle());
                for &handle in &handles {
                    let mline_style =
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(handle)
                            .and_then(|entity| match entity {
                                document::EntityType::MLine(mline) => {
                                    crate::entities::mline::resolved_mline_style(
                                        mline,
                                        &self.tabs[i].scene.document,
                                    )
                                    .cloned()
                                }
                                _ => None,
                            });
                    if matches!(field, "text_x" | "text_y") {
                        crate::entities::dimension::materialize_large_radial_text_position(
                            &mut self.tabs[i].scene.document,
                            handle,
                            &value,
                        );
                    }
                    if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                        crate::scene::view::dispatch::apply_geom_prop_in_working_plane(
                            entity, field, &value, plane,
                        );
                        match (entity as &mut document::EntityType, text_style.as_ref()) {
                            (document::EntityType::Text(text), Some(style)) => {
                                if style.height > 0.0 {
                                    text.height = style.height;
                                }
                                text.width_factor = style.width_factor;
                                text.oblique_angle = style.oblique_angle;
                            }
                            (document::EntityType::MText(text), Some(style))
                                if style.height > 0.0 =>
                            {
                                text.height = style.height;
                            }
                            _ => {}
                        }
                        if matches!(field, "ml_justification" | "ml_scale") {
                            if let (document::EntityType::MLine(mline), Some(style)) =
                                (entity, mline_style.as_ref())
                            {
                                crate::modules::draw::draw::mline::sync_mline_element_parameters(
                                    mline, style,
                                );
                            }
                        }
                    }
                    if let Some(annotative) = dim_style_annotative {
                        let document = &mut self.tabs[i].scene.document;
                        let current = match document.get_entity(handle) {
                            Some(document::EntityType::Dimension(dimension))
                                if !matches!(
                                    **dimension,
                                    document::entities::Dimension::Arc(_)
                                ) =>
                            {
                                crate::scene::annotative::object_is_annotative(
                                    document,
                                    &dimension.base().common,
                                )
                            }
                            _ => continue,
                        };
                        if current != annotative {
                            crate::scene::annotative::set_entity_annotative(
                                document, handle, annotative,
                            );
                            if let Some(scale) = dim_scale {
                                crate::scene::annotative::create_annotation_context(
                                    document, handle, scale,
                                );
                            }
                        }
                    }
                }
            }
            if field.starts_with("tbl_") {
                for &handle in &handles {
                    if let Some(document::EntityType::Table(table)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        table.block_record_handle = None;
                    }
                }
            }
            self.invalidate_property_targets(i, &handles);
            self.tabs[i].dirty = true;
            self.tabs[i].properties.edit_choice_open = None;
            if field == "spline_method" {
                self.tabs[i].properties.prop_vertex = 0;
                self.tabs[i].properties.prop_vertex_indicator_active = false;
            }
            self.refresh_properties();
        } else {
            match field {
                "transparency" => {
                    let Some(transparency) =
                        crate::scene::creation_style::parse_current_transparency(&value)
                    else {
                        self.command_line.push_error(
                            "Transparency: expected ByLayer, ByBlock, or an integer from 0 to 90.",
                        );
                        self.refresh_properties();
                        return Task::none();
                    };
                    if self.tabs[i].scene.document.current_entity_transparency() != transparency {
                        self.push_undo_snapshot(i, "CETRANSPARENCY");
                        if self.tabs[i]
                            .scene
                            .document
                            .set_current_entity_transparency(transparency)
                        {
                            self.tabs[i].dirty = true;
                        } else {
                            self.discard_last_undo_entry(i);
                            self.command_line.push_error(
                                "CETRANSPARENCY: drawing variable dictionary is invalid.",
                            );
                        }
                    }
                    self.refresh_properties();
                }
                "material" => {
                    let mat_handle: Option<document::Handle> = self.tabs[i]
                        .scene
                        .document
                        .objects
                        .iter()
                        .find_map(|(h, o)| match o {
                            document::objects::ObjectType::Material(m) if m.name == value => {
                                Some(*h)
                            }
                            _ => None,
                        });
                    match value.as_str() {
                        "ByLayer" | "ByBlock" | "Global" => {
                            self.tabs[i].scene.document.header.current_material_handle =
                                document::Handle::NULL;
                        }
                        _ => {
                            if let Some(h) = mat_handle {
                                self.tabs[i].scene.document.header.current_material_handle = h;
                            }
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                "plot_style" => {
                    if self.tabs[i].scene.document.header.plotstyle_mode {
                        self.refresh_properties();
                        return Task::none();
                    }
                    match value.as_str() {
                        "ByBlock" => self.tabs[i].scene.document.header.current_plotstyle_type = 1,
                        "Normal" => self.tabs[i].scene.document.header.current_plotstyle_type = 2,
                        _ => self.tabs[i].scene.document.header.current_plotstyle_type = 0,
                    }
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                "view_plot_style_table" => {
                    let next = if value.eq_ignore_ascii_case("None") {
                        String::new()
                    } else {
                        value.clone()
                    };
                    let layout_name = self.tabs[i].scene.current_layout.clone();
                    let current = self.tabs[i]
                        .scene
                        .document
                        .objects
                        .values()
                        .find_map(|object| {
                            let document::objects::ObjectType::Layout(layout) = object else {
                                return None;
                            };
                            layout
                                .name
                                .eq_ignore_ascii_case(&layout_name)
                                .then(|| layout.plot_style_sheet.clone())
                        })
                        .unwrap_or_else(|| self.tabs[i].scene.document.header.stylesheet.clone());
                    if current != next {
                        self.push_undo_snapshot(i, "PLOTSTYLE");
                        for object in self.tabs[i].scene.document.objects.values_mut() {
                            if let document::objects::ObjectType::Layout(layout) = object {
                                if layout.name.eq_ignore_ascii_case(&layout_name) {
                                    layout.plot_style_sheet = next.clone();
                                    layout.plot_flags.plot_plot_styles = !next.is_empty();
                                    layout.plot_flags.show_plot_styles = !next.is_empty();
                                }
                            }
                        }
                        self.tabs[i].scene.document.header.stylesheet = next;
                        self.tabs[i].dirty = true;
                    }
                    self.refresh_properties();
                }
                "view_annotation_scale" => {
                    let current = self.tabs[i]
                        .scene
                        .document
                        .header
                        .current_annotation_scale
                        .clone();
                    if !current.eq_ignore_ascii_case(&value) {
                        self.push_undo_snapshot(i, "CANNOSCALE");
                        if self.tabs[i]
                            .scene
                            .set_annotation_scale_named(&value)
                            .is_some()
                        {
                            self.tabs[i].dirty = true;
                        } else {
                            self.discard_last_undo_entry(i);
                        }
                    }
                    self.refresh_properties();
                }
                "view_ucs_icon_on" => {
                    let next = value.eq_ignore_ascii_case("Yes");
                    let active_viewport = self.tabs[i].scene.active_viewport;
                    let current = active_viewport
                        .and_then(|handle| self.tabs[i].scene.document.get_entity(handle))
                        .and_then(|entity| match entity {
                            document::EntityType::Viewport(viewport) => {
                                Some(viewport.ucs_icon_visible)
                            }
                            _ => None,
                        })
                        .or_else(|| {
                            self.tabs[i]
                                .scene
                                .document
                                .vports
                                .iter()
                                .find(|viewport| {
                                    viewport
                                        .name
                                        .trim_start_matches('*')
                                        .eq_ignore_ascii_case("active")
                                })
                                .map(|viewport| viewport.ucsicon_lower)
                        })
                        .unwrap_or(self.show_ucs_icon);
                    if current != next {
                        self.push_undo_snapshot(i, "UCSICON");
                        if let Some(handle) = active_viewport {
                            if let Some(document::EntityType::Viewport(viewport)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                viewport.ucs_icon_visible = next;
                            }
                        } else if let Some(viewport) = self.tabs[i]
                            .scene
                            .document
                            .vports
                            .iter_mut()
                            .find(|viewport| {
                                viewport
                                    .name
                                    .trim_start_matches('*')
                                    .eq_ignore_ascii_case("active")
                            })
                        {
                            viewport.ucsicon_lower = next;
                        }
                        self.show_ucs_icon = next;
                        self.persist_settings_if_changed();
                        self.tabs[i].dirty = true;
                    }
                    self.refresh_properties();
                }
                "view_ucs_icon_at_origin" => {
                    let next = value.eq_ignore_ascii_case("Yes");
                    let active_viewport = self.tabs[i].scene.active_viewport;
                    let current = active_viewport
                        .and_then(|handle| self.tabs[i].scene.document.get_entity(handle))
                        .and_then(|entity| match entity {
                            document::EntityType::Viewport(viewport) => {
                                Some(viewport.status.ucs_icon_at_origin)
                            }
                            _ => None,
                        })
                        .or_else(|| {
                            self.tabs[i]
                                .scene
                                .document
                                .vports
                                .iter()
                                .find(|viewport| {
                                    viewport
                                        .name
                                        .trim_start_matches('*')
                                        .eq_ignore_ascii_case("active")
                                })
                                .map(|viewport| viewport.ucsicon_origin)
                        })
                        .unwrap_or(self.ucs_icon_at_origin);
                    if current != next {
                        self.push_undo_snapshot(i, "UCSICON");
                        if let Some(handle) = active_viewport {
                            if let Some(document::EntityType::Viewport(viewport)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                viewport.status.ucs_icon_at_origin = next;
                            }
                        } else if let Some(viewport) = self.tabs[i]
                            .scene
                            .document
                            .vports
                            .iter_mut()
                            .find(|viewport| {
                                viewport
                                    .name
                                    .trim_start_matches('*')
                                    .eq_ignore_ascii_case("active")
                            })
                        {
                            viewport.ucsicon_origin = next;
                        }
                        self.ucs_icon_at_origin = next;
                        self.persist_settings_if_changed();
                        self.tabs[i].dirty = true;
                    }
                    self.refresh_properties();
                }
                "view_ucs_per_viewport" => {
                    let next = value.eq_ignore_ascii_case("Yes");
                    let active_viewport = self.tabs[i].scene.active_viewport;
                    let current = active_viewport
                        .and_then(|handle| self.tabs[i].scene.document.get_entity(handle))
                        .and_then(|entity| match entity {
                            document::EntityType::Viewport(viewport) => {
                                Some(viewport.ucs_per_viewport)
                            }
                            _ => None,
                        })
                        .or_else(|| {
                            self.tabs[i]
                                .scene
                                .document
                                .vports
                                .iter()
                                .find(|viewport| {
                                    viewport
                                        .name
                                        .trim_start_matches('*')
                                        .eq_ignore_ascii_case("active")
                                })
                                .map(|viewport| viewport.ucs_per_viewport)
                        })
                        .unwrap_or(true);
                    if current != next {
                        self.push_undo_snapshot(i, "UCSVP");
                        if let Some(handle) = active_viewport {
                            if let Some(document::EntityType::Viewport(viewport)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                viewport.ucs_per_viewport = next;
                            }
                        } else if let Some(viewport) = self.tabs[i]
                            .scene
                            .document
                            .vports
                            .iter_mut()
                            .find(|viewport| {
                                viewport
                                    .name
                                    .trim_start_matches('*')
                                    .eq_ignore_ascii_case("active")
                            })
                        {
                            viewport.ucs_per_viewport = next;
                        }
                        self.tabs[i].dirty = true;
                    }
                    self.refresh_properties();
                }
                "view_visual_style" => {
                    use document::entities::ViewportRenderMode as Mode;
                    let mode = match value.as_str() {
                        "2D Wireframe" | "Wireframe 2D" => Some(Mode::Wireframe2D),
                        "3D Wireframe" | "Wireframe 3D" => Some(Mode::Wireframe3D),
                        "Hidden Line" => Some(Mode::HiddenLine),
                        "Flat Shaded" => Some(Mode::FlatShaded),
                        "Gouraud Shaded" => Some(Mode::GouraudShaded),
                        "Flat Shaded + Edges" => Some(Mode::FlatShadedWithEdges),
                        "Gouraud Shaded + Edges" => Some(Mode::GouraudShadedWithEdges),
                        _ => None,
                    };
                    if let Some(mode) = mode {
                        let task = self.on_set_render_mode(mode);
                        self.refresh_properties();
                        return task;
                    }
                    self.refresh_properties();
                }
                _ => {}
            }
        }
        Task::none()
    }

    /// An associative array row: the typed value changes the array's
    /// parameter and its items are placed again, in one undo step.
    pub(in crate::app) fn on_assoc_array_field_commit(&mut self, i: usize, field: &'static str, handles: &[Handle]) {
        let Some(text) = self.tabs[i]
            .properties
            .edit_buf
            .remove(&crate::ui::properties::FieldKey::Geom(field))
        else {
            return;
        };
        let arrays: Vec<_> = handles
            .iter()
            .filter_map(|handle| {
                crate::scene::assoc_array::array_of_insert(&self.tabs[i].scene.document, *handle)
            })
            .collect();
        if arrays.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, "ARRAYEDIT");
        for array in &arrays {
            if let Err(error) = crate::scene::assoc_array::apply_field(
                &mut self.tabs[i].scene,
                array,
                field,
                &crate::app::expr_eval::eval_to_string(&text),
            ) {
                self.command_line.push_error(&format!("ARRAYEDIT: {error}"));
            }
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    pub(in crate::app) fn on_prop_geom_commit(&mut self, field: &'static str) -> Task<Message> {
        if field == "transparency" {
            let panel = &mut self.tabs[self.active_tab].properties;
            panel.active_field = None;
            if let Some(value) = panel
                .edit_buf
                .remove(&crate::ui::properties::FieldKey::Geom(field))
            {
                // Typed values and presets share validation, undo and the
                // current-entity default when no object is selected.
                return self.on_prop_geom_choice_changed(field, value);
            }
            return Task::none();
        }
        if field == crate::entities::civil::COGO_POINT_NAME {
            return self.commit_cogo_point_name();
        }
        if field == "authoring_grip_count" {
            return self.commit_block_grip_count();
        }
        if field.starts_with("authoring_display_grip_") {
            return self.commit_native_grip_location(field);
        }
        if field.starts_with("authoring_action_") {
            return self.commit_authoring_action_option(field);
        }
        if field == "authoring_name" {
            return self.commit_authoring_name();
        }
        if crate::scene::block_authoring::metadata::Field::from_key(field).is_some() {
            return self.commit_authoring_metadata(field);
        }
        if field.starts_with("authoring_set_") {
            return self.commit_authoring_value_set(field);
        }
        if field.starts_with("bedit_") {
            return self.commit_block_property(field);
        }
        use crate::ui::window::named_parameters::ParamField;
        // A dynamic dimension's Name/Expression rows edit its parameter.
        let dynamic_field = match field {
            "dyn_constraint_name" => Some(ParamField::Name),
            "dyn_constraint_expression" => Some(ParamField::Formula),
            _ => None,
        };
        if let Some(param_field) = dynamic_field {
            return self.on_dynamic_dimension_field_commit(field, param_field);
        }
        if field == "dyn_constraint_description" {
            return self.on_dynamic_dimension_description_commit(field);
        }
        let i = self.active_tab;
        self.tabs[i].properties.active_field = None;
        let handles = self.property_target_handles(i);
        if crate::app::properties::field_moves_geometry(field)
            && self.any_entity_is_fixed(i, &handles)
        {
            self.tabs[i]
                .properties
                .edit_buf
                .remove(&crate::ui::properties::FieldKey::Geom(field));
            self.command_line
                .push_error(crate::app::properties::HELD_BY_FIXED);
            return Task::none();
        }
        if field.starts_with("assoc_array_") {
            self.on_assoc_array_field_commit(i, field, &handles);
            return Task::none();
        }
        if !handles.is_empty() {
            let driven_reference = match field {
                "start_x" | "start_y" | "start_z" => Some(Some(0)),
                "end_x" | "end_y" | "end_z" => Some(Some(1)),
                "center_x" | "center_y" | "center_z" => Some(Some(-3)),
                "radius" | "diameter" | "circumference" | "area" | "major_r" | "minor_r"
                | "ratio" => Some(None),
                _ => None,
            };
            let retain_size = self.constraint_solve_mode
                && driven_reference.is_some_and(|marker| marker.is_some());
            let retained_originals: Vec<_> = if retain_size {
                handles
                    .iter()
                    .filter_map(|handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(*handle)
                            .cloned()
                            .map(|entity| (*handle, entity))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let evaluates_expression = self.tabs[i]
                .properties
                .sections
                .iter()
                .flat_map(|section| section.props.iter())
                .find(|property| property.field == field)
                .is_some_and(|property| matches!(property.value, PropValue::EditText(_)));
            if let Some(raw_val) = self.tabs[i]
                .properties
                .edit_buf
                .remove(&crate::ui::properties::FieldKey::Geom(field))
            {
                let val = if evaluates_expression {
                    self.property_expression_value(&raw_val)
                } else {
                    raw_val
                };
                // Underlay rows: reject what the reference rejects, and
                // write a Width / Height as the scale that produces it.
                if field.starts_with("ext_pcx_") {
                    if let Err(message) =
                        crate::entities::extended::validate_point_cloud_property(field, &val)
                    {
                        self.command_line.push_error(message);
                        return Task::none();
                    }
                }
                if field.starts_with("ole_") {
                    if let Err(message) = crate::entities::ole2frame::validate_property(field, &val)
                    {
                        self.command_line.push_error(message);
                        return Task::none();
                    }
                    if matches!(
                        field,
                        "ole_width" | "ole_height" | "ole_scale_width" | "ole_scale_height"
                    ) {
                        self.resize_ole_frames(i, &handles, field, &val);
                        return Task::none();
                    }
                }
                if field.starts_with("att_") {
                    if let Err(message) =
                        crate::entities::attribute::validate_definition_property(field, &val)
                    {
                        self.command_line.push_error(message);
                        return Task::none();
                    }
                }
                if field.starts_with("ul_")
                    || matches!(field, "ri_bright" | "ri_contrast" | "ri_fade")
                {
                    if let Err(message) = crate::entities::underlay::validate_property(field, &val)
                    {
                        self.command_line.push_error(message);
                        return Task::none();
                    }
                }
                let (field, val) = match field {
                    "ul_width" | "ul_height" => {
                        let scale = handles.iter().find_map(|handle| {
                            match self.tabs[i].scene.document.get_entity(*handle) {
                                Some(document::EntityType::Underlay(underlay)) => {
                                    crate::entities::underlay::size_to_scale(
                                        underlay,
                                        &self.tabs[i].scene.document,
                                        field,
                                        &val,
                                    )
                                }
                                _ => None,
                            }
                        });
                        match scale {
                            Some(scale) => ("ul_scale", scale),
                            None => return Task::none(),
                        }
                    }
                    _ => (field, val),
                };
                if matches!(
                    field,
                    "current_fit_point" | "current_control_point" | "pm_current_vertex"
                ) {
                    let count = handles
                        .iter()
                        .filter_map(|handle| {
                            let entity = self.tabs[i].scene.document.get_entity(*handle)?;
                            match (field, entity) {
                                ("current_fit_point", document::EntityType::Spline(spline)) => {
                                    Some(spline.fit_points.len())
                                }
                                ("current_control_point", document::EntityType::Spline(spline)) => {
                                    Some(crate::entities::spline::control_vertex_count(spline))
                                }
                                ("pm_current_vertex", document::EntityType::PolygonMesh(mesh)) => {
                                    Some(mesh.vertices.len())
                                }
                                _ => None,
                            }
                        })
                        .min()
                        .unwrap_or(0);
                    if let Ok(requested) = val.trim().parse::<usize>() {
                        if count > 0 {
                            let next = requested.clamp(1, count) - 1;
                            self.tabs[i].properties.prop_vertex = next;
                            self.tabs[i].properties.prop_vertex_indicator_active = true;
                        }
                    }
                    self.refresh_properties();
                    return Task::none();
                }
                self.push_undo_snapshot(i, "CHPROP");
                if field == "tbl_style_handle" {
                    let style_handle =
                        self.tabs[i]
                            .scene
                            .document
                            .objects
                            .iter()
                            .find_map(|(handle, object)| match object {
                                document::objects::ObjectType::TableStyle(style)
                                    if style.name.eq_ignore_ascii_case(val.trim()) =>
                                {
                                    Some(*handle)
                                }
                                _ => None,
                            });
                    if let Some(style_handle) = style_handle {
                        for &handle in &handles {
                            if let Some(document::EntityType::Table(table)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                table.table_style_handle = Some(style_handle);
                            }
                        }
                    }
                } else if let Some(axis) = match field {
                    "xref_x_scale" => Some(0),
                    "xref_y_scale" => Some(1),
                    "xref_z_scale" => Some(2),
                    _ => None,
                } {
                    // An xref's Scale rows show the scale without the
                    // unit conversion its INSERT carries; write it back
                    // with that conversion.
                    let value = val
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite() && *v != 0.0);
                    if let Some(value) = value {
                        let host = self.tabs[i].scene.document.header.insertion_units;
                        for &handle in &handles {
                            let factor = match self.tabs[i].scene.document.get_entity(handle) {
                                Some(document::EntityType::Insert(ins)) => self.tabs[i]
                                    .scene
                                    .document
                                    .block_records
                                    .get(&ins.block_name)
                                    .and_then(|br| {
                                        crate::app::properties::insert_unit_scale(host, br.units)
                                    })
                                    .unwrap_or(1.0),
                                _ => continue,
                            };
                            if let Some(document::EntityType::Insert(ins)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                match axis {
                                    0 => ins.set_x_scale(value * factor),
                                    1 => ins.set_y_scale(value * factor),
                                    _ => ins.set_z_scale(value * factor),
                                }
                            }
                        }
                    }
                } else if field == "block" {
                    // Name row on a block reference: an existing name
                    // re-points the selected inserts; a new one renames
                    // the definition they share. Commit closes the list.
                    self.tabs[i].properties.edit_choice_open = None;
                    self.apply_block_name_commit(i, &handles, val.trim());
                } else if field == "frozen_layers" {
                    // Resolve layer names → handles, then apply to viewports.
                    let layer_handles: Vec<document::Handle> = val
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .filter_map(|name| {
                            self.tabs[i]
                                .scene
                                .document
                                .layers
                                .iter()
                                .find(|l| l.name.eq_ignore_ascii_case(name))
                                .map(|l| l.handle)
                        })
                        .collect();
                    for &handle in &handles {
                        if let Some(document::EntityType::Viewport(vp)) =
                            self.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            vp.frozen_layers = layer_handles.clone();
                        }
                    }
                } else {
                    // Per-vertex geometry edits (vertex_x/y, widths) target
                    // the vertex the Current Vertex stepper is on. (polyline)
                    crate::scene::view::dispatch::set_prop_current_vertex(
                        self.tabs[i].properties.prop_vertex,
                    );
                    let plane = if self.tabs[i].editing_model_space() {
                        self.tabs[i].ucs_xform().working_plane()
                    } else {
                        crate::command::WorkingPlane::default()
                    };
                    for &handle in &handles {
                        // Skip objects on a locked layer.
                        if self.tabs[i].scene.is_layer_locked(handle) {
                            continue;
                        }
                        let insert_before = matches!(field, "ins_x" | "ins_y" | "ins_z")
                            .then(|| self.tabs[i].scene.document.get_entity(handle))
                            .flatten()
                            .and_then(|entity| match entity {
                                document::EntityType::Insert(insert) => Some(insert.clone()),
                                _ => None,
                            });
                        match field {
                            "tol_text_height" => {
                                use crate::entities::dim_override as dov;
                                let trimmed = val.trim();
                                if trimmed.is_empty() {
                                    dov::set(
                                        &mut self.tabs[i].scene.document,
                                        handle,
                                        dov::DIMTXT,
                                        None,
                                    );
                                } else if let Ok(height) = trimmed.parse::<f64>() {
                                    if height > 0.0 {
                                        dov::set(
                                            &mut self.tabs[i].scene.document,
                                            handle,
                                            dov::DIMTXT,
                                            Some(document::xdata::XDataValue::Real(height)),
                                        );
                                    }
                                }
                            }
                            _ if field.starts_with("dim_") => {
                                if matches!(
                                    self.tabs[i].scene.document.get_entity(handle),
                                    Some(document::EntityType::Dimension(_))
                                ) {
                                    let applied = crate::entities::dim_override::set_property(
                                        &mut self.tabs[i].scene.document,
                                        handle,
                                        field,
                                        &val,
                                    );
                                    if applied && field == "dim_text_inside" {
                                        if let Some(document::entities::Dimension::LargeRadial(
                                            dimension,
                                        )) = self.tabs[i]
                                            .scene
                                            .document
                                            .get_entity_mut(handle)
                                            .and_then(crate::entities::dimension::as_dimension_mut)
                                        {
                                            dimension.base.text_user_positioned = false;
                                        }
                                    }
                                }
                            }
                            // Annotative multiline text: the distance is on the
                            // model height of the shown representation.
                            "line_space_distance"
                                if self.tabs[i]
                                    .scene
                                    .annotative_text_height_factor(handle)
                                    .is_some() =>
                            {
                                let shown = self.tabs[i].scene.displayed_annotation_scale_handle();
                                let ratio = crate::scene::annotative::text_context_ratio(
                                    &self.tabs[i].scene.document,
                                    handle,
                                    shown,
                                )
                                .unwrap_or(1.0)
                                .max(1.0e-12);
                                if let (Ok(model), Some(entity)) = (
                                    val.trim().parse::<f64>(),
                                    self.tabs[i].scene.document.get_entity_mut(handle),
                                ) {
                                    crate::scene::view::dispatch::apply_geom_prop_in_working_plane(
                                        entity,
                                        field,
                                        &(model / ratio).to_string(),
                                        plane,
                                    );
                                }
                            }
                            // An annotative hatch's scale rows are on paper.
                            "pattern_scale" | "spacing" | "iso_pen_width"
                                if self.tabs[i]
                                    .scene
                                    .annotative_hatch_model_per_paper(handle)
                                    .is_some() =>
                            {
                                let per_paper = self.tabs[i]
                                    .scene
                                    .annotative_hatch_model_per_paper(handle)
                                    .unwrap_or(1.0);
                                if let (Ok(paper), Some(entity)) = (
                                    val.trim().parse::<f64>(),
                                    self.tabs[i].scene.document.get_entity_mut(handle),
                                ) {
                                    crate::scene::view::dispatch::apply_geom_prop_in_working_plane(
                                        entity,
                                        field,
                                        &(paper * per_paper).to_string(),
                                        plane,
                                    );
                                }
                                self.tabs[i].scene.sync_displayed_annotation_context(handle);
                            }
                            // Annotative text: the row holds the paper height
                            // (and, for multiline text, the paper width and
                            // height of its box).
                            "height" | "att_h" | "rect_w" | "rect_h"
                                if self.tabs[i]
                                    .scene
                                    .annotative_text_height_factor(handle)
                                    .is_some() =>
                            {
                                let mut factor = self.tabs[i]
                                    .scene
                                    .annotative_text_height_factor(handle)
                                    .unwrap_or(1.0);
                                // The box rows are the shown representation's:
                                // its model size is the paper size times that
                                // scale's model-per-paper.
                                if matches!(field, "rect_w" | "rect_h") {
                                    let shown =
                                        self.tabs[i].scene.displayed_annotation_scale_handle();
                                    factor *= crate::scene::annotative::text_context_ratio(
                                        &self.tabs[i].scene.document,
                                        handle,
                                        shown,
                                    )
                                    .unwrap_or(1.0);
                                }
                                if let (Ok(paper), Some(entity)) = (
                                    val.trim().parse::<f64>(),
                                    self.tabs[i].scene.document.get_entity_mut(handle),
                                ) {
                                    crate::scene::view::dispatch::apply_geom_prop_in_working_plane(
                                        entity,
                                        field,
                                        &(paper * factor).to_string(),
                                        plane,
                                    );
                                }
                                // The shown representation keeps the new box.
                                if matches!(field, "rect_w" | "rect_h") {
                                    self.tabs[i].scene.sync_displayed_annotation_context(handle);
                                }
                            }
                            "hyperlink" => {
                                // The address of the PE_URL record; its
                                // text and named location stay.
                                crate::scene::hyperlink::set_url(
                                    &mut self.tabs[i].scene.document,
                                    handle,
                                    &val,
                                );
                            }
                            "arrow_size" | "text_offset" | "dim_scale_overall" => {
                                // Real-valued leader dim-var overrides
                                // (ACAD_DSTYLE). A blank value clears the
                                // override → reverts to the style; an
                                // unparseable entry is ignored so a typo
                                // can't silently wipe a stored override.
                                use crate::entities::dim_override as dov;
                                let code = match field {
                                    "arrow_size" => dov::DIMASZ,
                                    "text_offset" => dov::DIMGAP,
                                    _ => dov::DIMSCALE,
                                };
                                let trimmed = val.trim();
                                if trimmed.is_empty() {
                                    dov::set(&mut self.tabs[i].scene.document, handle, code, None);
                                } else if let Ok(n) = trimmed.parse::<f64>() {
                                    dov::set(
                                        &mut self.tabs[i].scene.document,
                                        handle,
                                        code,
                                        Some(document::xdata::XDataValue::Real(n)),
                                    );
                                }
                            }
                            "linetype_scale" | "transparency" | "thickness" => {
                                // Thickness (DXF 39 extrusion) is a General-
                                // group common prop, not a geometry-group one:
                                // route it to apply_common_prop (which handles
                                // it via set_entity_thickness). apply_geom_prop
                                // ignores it, so the edit would otherwise be a
                                // silent no-op and revert to the old value.
                                if let Some(entity) =
                                    self.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    crate::scene::view::dispatch::apply_common_prop(
                                        entity, field, &val,
                                    );
                                }
                            }
                            _ => {
                                if crate::scene::model::solid_history::is_primitive_property(field)
                                {
                                    if !self.tabs[i]
                                        .scene
                                        .apply_solid_history_property(handle, field, &val)
                                    {
                                        if let Some(code) = self.tabs[i]
                                            .scene
                                            .solid_history_property_refusal(handle, field, &val)
                                        {
                                            self.command_line.push_output(
                                                crate::t!("Modeling Operation Error:").as_ref(),
                                            );
                                            self.command_line.push_output(
                                                crate::tf!(
                                                    "Error Code Number is {code}",
                                                    code = code
                                                )
                                                .as_ref(),
                                            );
                                        }
                                    }
                                } else if self.tabs[i]
                                    .scene
                                    .apply_solid_position_property(handle, field, &val, plane)
                                    .is_none()
                                {
                                    let mline_style =
                                        self.tabs[i].scene.document.get_entity(handle).and_then(
                                            |entity| match entity {
                                                document::EntityType::MLine(mline) => {
                                                    crate::entities::mline::resolved_mline_style(
                                                        mline,
                                                        &self.tabs[i].scene.document,
                                                    )
                                                    .cloned()
                                                }
                                                _ => None,
                                            },
                                        );
                                    if matches!(field, "text_x" | "text_y") {
                                        crate::entities::dimension::materialize_large_radial_text_position(
                                                    &mut self.tabs[i].scene.document,
                                                    handle,
                                                    &val,
                                                );
                                    }
                                    if let Some(entity) =
                                        self.tabs[i].scene.document.get_entity_mut(handle)
                                    {
                                        crate::scene::view::dispatch::apply_geom_prop_in_working_plane(
                                                    entity,
                                                    field,
                                                    &val,
                                                    plane,
                                                );
                                        if field == "ml_scale" {
                                            if let (
                                                document::EntityType::MLine(mline),
                                                Some(style),
                                            ) = (entity, mline_style.as_ref())
                                            {
                                                crate::modules::draw::draw::mline::sync_mline_element_parameters(
                                                            mline, style,
                                                        );
                                            }
                                        }
                                    }
                                    if matches!(field, "srf_u_isolines" | "srf_v_isolines") {
                                        self.tabs[i].scene.reseed_derived_caches(handle);
                                    }
                                }
                            }
                        }
                        if let Some(before) = insert_before {
                            self.tabs[i]
                                .scene
                                .carry_insert_attribute_contexts_after_position_edit(
                                    handle, &before,
                                );
                        }
                    }
                }
                // A cloud's Arc Length is REVCLOUD's arc length from now on.
                if field == "revcloud_arc_length" {
                    if let Ok(length) = val.trim().parse::<f64>() {
                        if length.is_finite() && length > 0.0 {
                            crate::io::set_drawing_variable(
                                &mut self.tabs[i].scene.document,
                                crate::entities::revcloud::ARC_LENGTH_VARIABLE,
                                &format!("{length:.6}"),
                            );
                        }
                    }
                }
                if field.starts_with("tbl_") {
                    for &handle in &handles {
                        if let Some(document::EntityType::Table(table)) =
                            self.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            table.block_record_handle = None;
                        }
                    }
                }
                let driven_refs: Vec<_> = driven_reference
                    .into_iter()
                    .flat_map(|marker| {
                        handles.iter().copied().map(move |handle| match marker {
                            Some(marker) => {
                                crate::scene::parametric_constraints::ParametricRef::point(
                                    handle, marker,
                                )
                            }
                            None => {
                                crate::scene::parametric_constraints::ParametricRef::whole(handle)
                            }
                        })
                    })
                    .collect();
                self.invalidate_property_targets_with_originals(
                    i,
                    &handles,
                    &driven_refs,
                    retain_size,
                    &retained_originals,
                );
                self.tabs[i].dirty = true;
                self.refresh_properties();
            }
        } else {
            let _ = self.tabs[i]
                .properties
                .edit_buf
                .remove(&crate::ui::properties::FieldKey::Geom(field));
            self.refresh_properties();
        }
        Task::none()
    }

    /// Properties-panel Name commit for block references. A `new` matching an
    /// existing regular block re-points the selected inserts to it; otherwise
    /// the definition the selection references is renamed and every insert of
    /// it follows, including a reference showing an evaluated copy of it.
    /// Anonymous/xref definitions are refused.
    pub(in crate::app) fn apply_block_name_commit(&mut self, i: usize, handles: &[document::Handle], new: &str) {
        if new.is_empty() {
            return;
        }
        // Existing target → swap the selected references to it. Anonymous,
        // xref and xref-dependent definitions are not valid targets.
        let target = self.tabs[i]
            .scene
            .document
            .block_records
            .get(new)
            .map(|br| {
                (
                    br.name.clone(),
                    br.handle,
                    br.is_anonymous() || br.flags.is_xref || br.name.contains('|'),
                )
            });
        if let Some((canon, to, protected)) = target {
            if protected {
                return;
            }
            let mut planned = Vec::new();
            for &handle in handles {
                if self.tabs[i].scene.is_layer_locked(handle) {
                    continue;
                }
                let Some((from, differs)) =
                    self.tabs[i].scene.document.get_entity(handle).and_then(
                        |entity| match entity {
                            document::EntityType::Insert(ins) => {
                                let record = self.tabs[i]
                                    .scene
                                    .document
                                    .block_records
                                    .get(&ins.block_name)?;
                                // An evaluated copy already carries the
                                // definition's name in this row.
                                let effective =
                                    crate::app::control::block_ops::resolve_effective_block(
                                        &self.tabs[i].scene.document,
                                        ins,
                                    )
                                    .1;
                                Some((record.handle, effective != Some(to)))
                            }
                            _ => None,
                        },
                    )
                else {
                    continue;
                };
                if differs {
                    planned.push((handle, from));
                }
            }
            if !planned.is_empty() {
                let mut changes = Vec::new();
                for (handle, from) in planned {
                    // An evaluated copy must stop resolving through its old
                    // definition before it takes the new name.
                    crate::app::control::block_ops::detach_representation(
                        &mut self.tabs[i].scene.document,
                        handle,
                    );
                    self.tabs[i]
                        .scene
                        .repoint_representation(handle, from, to, &canon);
                    changes.push((handle, crate::scene::ChangeKind::Modified));
                }
                self.tabs[i].scene.bump_entities(&changes);
            }
            return;
        }
        // New name → rename. Only when every selected insert references the
        // same definition; a mixed selection makes the rename target ambiguous.
        let mut old: Option<Option<document::Handle>> = None;
        for &handle in handles {
            if let Some(document::EntityType::Insert(ins)) =
                self.tabs[i].scene.document.get_entity(handle)
            {
                let (name, definition, _) = crate::app::control::block_ops::resolve_effective_block(
                    &self.tabs[i].scene.document,
                    ins,
                );
                let definition = definition.or_else(|| {
                    self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .get(&name)
                        .map(|record| record.handle)
                });
                match old {
                    None => old = Some(definition),
                    Some(current) if current == definition => {}
                    _ => return,
                }
            }
        }
        let Some(Some(old)) = old else {
            return;
        };
        let Some(old) = self.tabs[i]
            .scene
            .document
            .block_records
            .iter()
            .find(|record| record.handle == old)
            .map(|record| record.name.clone())
        else {
            return;
        };
        self.push_undo_snapshot(i, "BLOCK RENAME");
        if !self.tabs[i].scene.rename_block(&old, new) {
            self.command_line
                .push_error(crate::tf!("Could not rename block \"{old}\" to \"{new}\".").as_ref());
        }
    }

    /// Commit an edited block-attribute value from the Properties panel.
    /// Writes the new value into the matching attribute (by tag) of every
    /// selected INSERT, then re-tessellates so the attribute text repaints.
    /// The value is stored verbatim (attribute values are free-form text, so
    /// it is not run through the expression evaluator like numeric fields).
    pub(in crate::app) fn on_prop_attr_commit(&mut self, tag: String) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].properties.active_field = None;
        let key = crate::ui::properties::attr_edit_key(&tag);
        let handles = self.property_target_handles(i);
        if handles.is_empty() {
            return Task::none();
        }
        let Some(val) = self.tabs[i].properties.edit_buf.remove(&key) else {
            return Task::none();
        };
        self.push_undo_snapshot(i, "ATTEDIT");
        for &handle in &handles {
            if let Some(document::EntityType::Insert(ins)) =
                self.tabs[i].scene.document.get_entity_mut(handle)
            {
                if let Some(attr) = ins.attributes.iter_mut().find(|a| a.tag == tag) {
                    attr.set_value(val.clone());
                }
            }
        }
        self.invalidate_property_targets(i, &handles);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        Task::none()
    }

    /// Commit one plugin class parameter's edit (API v14): the stamped
    /// selection writes through the session's class-parameter path — one
    /// undo step inside the record update, the declaring plugin re-stages
    /// from the record's dirty notification; a refusal (unknown or
    /// non-editable parameter, unparseable value, unresolved record)
    /// reports and changes nothing. The section binds to one entity and
    /// exists only on the native build (the wasm build has no plugin
    /// registry the section could bind to), so the wasm route stays inert.
    pub(in crate::app) fn on_prop_plugin_param_commit(&mut self, name: String) -> Task<Message> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let i = self.active_tab;
            self.tabs[i].properties.active_field = None;
            let key = crate::ui::properties::plugin_param_edit_key(&name);
            let handles = self.property_target_handles(i);
            if handles.len() != 1 {
                return Task::none();
            }
            let Some(value) = self.tabs[i].properties.edit_buf.remove(&key) else {
                return Task::none();
            };
            let result;
            {
                let mut host = crate::app::plugin_host::HostSession::new(self, i);
                result = host.update_plugin_class_instance_parameter(handles[0], &name, &value);
            }
            if let Err(err) = result {
                self.command_line.push_error(&err);
            }
            self.refresh_properties();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = name;
        Task::none()
    }

    /// The currently-selected editor row, for the Text Options / Properties
    /// edit handlers (which all act on that one attribute).
    pub(in crate::app) fn attr_row_selected_mut(&mut self) -> Option<&mut AttrRow> {
        let sel = self.attr_editor_selected;
        self.attr_editor_rows.get_mut(sel)
    }
}

pub(crate) struct BlockAttrMeta {
    pub(crate) prompt: String,
    pub(crate) constant: bool,
}

/// Map each attribute tag to the metadata (prompt text, constant flag) declared on
/// the block's matching ATTDEF. Tags with no definition are absent.
pub(crate) fn block_attr_meta(
    doc: &document::Document,
    block_name: &str,
) -> rustc_hash::FxHashMap<String, BlockAttrMeta> {
    let mut map = rustc_hash::FxHashMap::default();
    if let Some(br) = doc.block_records.get(block_name) {
        for &eh in &br.entity_handles {
            if let Some(document::EntityType::AttributeDefinition(ad)) = doc.get_entity(eh) {
                map.insert(
                    ad.tag.clone(),
                    BlockAttrMeta {
                        prompt: ad.prompt.clone(),
                        constant: ad.flags.constant,
                    },
                );
            }
        }
    }
    map
}

/// Build the editor's working copy of one attribute. Angles are shown in
/// degrees; numeric fields become strings so the user can type freely.
pub(crate) fn attr_row_from_entity(
    a: &document::entities::AttributeEntity,
    meta: &rustc_hash::FxHashMap<String, BlockAttrMeta>,
) -> AttrRow {
    // Shown as the reference prints them: no trailing zeros.
    let fmt = |v: f64| crate::app::attr_editor::number(v);
    let ad_meta = meta.get(&a.tag);
    let prompt = ad_meta.map(|m| m.prompt.clone()).unwrap_or_default();
    let is_constant = a.flags.constant || ad_meta.is_some_and(|m| m.constant);
    AttrRow {
        tag: a.tag.clone(),
        prompt,
        value: a
            .embedded_mtext
            .as_ref()
            .map_or_else(|| a.get_value().to_string(), |m| m.value.clone()),
        text_style: a.text_style.clone(),
        height: fmt(a.height),
        rotation: fmt(a.rotation.to_degrees()),
        width_factor: fmt(a.width_factor),
        oblique: fmt(a.oblique_angle.to_degrees()),
        // A multi-line attribute is placed by its text box's attachment.
        h_align: a
            .embedded_mtext
            .as_ref()
            .map(|m| crate::app::attr_editor::alignment_of(m.attachment_point).0)
            .unwrap_or(a.horizontal_alignment),
        v_align: a
            .embedded_mtext
            .as_ref()
            .map(|m| crate::app::attr_editor::alignment_of(m.attachment_point).1)
            .unwrap_or(a.vertical_alignment),
        backwards: a.text_generation_flags & 0x2 != 0,
        upside_down: a.text_generation_flags & 0x4 != 0,
        layer: a.common.layer.clone(),
        color: a.common.color,
        linetype: a.common.linetype.clone(),
        line_weight: a.common.line_weight,
        is_constant,
        index: 0,
        multiline: a.is_multiline || a.embedded_mtext.is_some(),
        boundary_width: a
            .embedded_mtext
            .as_ref()
            .map(|m| fmt(m.rectangle_width))
            .unwrap_or_else(|| "0".to_string()),
        annotative: crate::app::attr_editor::attribute_is_annotative(a),
        field: None,
        field_changed: false,
    }
}

/// Write one edited row back onto its attribute; returns whether anything
/// changed. Numeric fields are parsed (angles from degrees); an unparseable
/// field is left as-is. Constant attributes ignore edits to their value.
pub(crate) fn apply_attr_row(a: &mut document::entities::AttributeEntity, row: &AttrRow) -> bool {
    let mut ch = false;
    let active_value = a
        .embedded_mtext
        .as_ref()
        .map_or(a.get_value(), |m| m.value.as_str());
    if !row.is_constant && active_value != row.value {
        // An explicit edit updates both forms. An unchanged row preserves its fallback.
        a.set_value(row.value.clone());
        if let Some(m) = a.embedded_mtext.as_mut() {
            m.value = row.value.clone();
        }
        ch = true;
    }
    if a.text_style != row.text_style {
        a.text_style = row.text_style.clone();
        ch = true;
    }
    // A zero height is ignored, as the reference does; the window refuses a
    // negative one, an unreadable one, and the out-of-range factors.
    if let Ok(h) = row
        .height
        .trim()
        .parse::<f64>()
        .map_err(|_| ())
        .and_then(|h| if h > 0.0 { Ok(h) } else { Err(()) })
    {
        if a.height != h {
            a.height = h;
            ch = true;
        }
    }
    if let Ok(deg) = row.rotation.trim().parse::<f64>() {
        // 400 is kept as 40.
        let rad = deg.rem_euclid(360.0).to_radians();
        if (a.rotation - rad).abs() > 1e-12 {
            a.rotation = rad;
            ch = true;
        }
    }
    if let Some(w) = row
        .width_factor
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|w| (0.01..=100.0).contains(w) && !row.multiline)
    {
        if a.width_factor != w {
            a.width_factor = w;
            ch = true;
        }
    }
    if let Some(deg) = row
        .oblique
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|d| d.abs() <= 85.0 && !row.multiline)
    {
        let rad = deg.to_radians();
        if (a.oblique_angle - rad).abs() > 1e-12 {
            a.oblique_angle = rad;
            ch = true;
        }
    }
    // A multi-line attribute is placed by its text box's attachment (below);
    // its own single-line justification stays as it is.
    if !row.multiline && a.horizontal_alignment != row.h_align {
        a.horizontal_alignment = row.h_align;
        ch = true;
    }
    if !row.multiline && a.vertical_alignment != row.v_align {
        a.vertical_alignment = row.v_align;
        ch = true;
    }
    // Text generation flags: bit 0x2 = backwards, bit 0x4 = upside down.
    let new_flags = (a.text_generation_flags & !0x6)
        | if row.backwards { 0x2 } else { 0 }
        | if row.upside_down { 0x4 } else { 0 };
    if new_flags != a.text_generation_flags {
        a.text_generation_flags = new_flags;
        ch = true;
    }
    if a.common.layer != row.layer {
        a.common.layer = row.layer.clone();
        ch = true;
    }
    if a.common.color != row.color {
        a.common.color = row.color;
        a.common.color_name = None;
        a.common.color_book_handle = None;
        ch = true;
    }
    if a.common.linetype != row.linetype {
        a.common.linetype = row.linetype.clone();
        ch = true;
    }
    if a.common.line_weight != row.line_weight {
        a.common.line_weight = row.line_weight;
        ch = true;
    }
    // A multi-line attribute draws from its embedded text: it follows.
    if let Some(m) = a.embedded_mtext.as_mut() {
        if let Some(w) = row
            .boundary_width
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|w| *w >= 0.0)
        {
            if m.rectangle_width != w {
                m.rectangle_width = w;
                ch = true;
            }
        }
        if m.height != a.height || m.rotation != a.rotation || m.style != a.text_style {
            m.height = a.height;
            m.rotation = a.rotation;
            m.style = a.text_style.clone();
            ch = true;
        }
        if let Some(p) = crate::app::attr_editor::attachment_for(row.h_align, row.v_align) {
            if m.attachment_point != p {
                m.attachment_point = p;
                ch = true;
            }
        }
    }
    ch
}
