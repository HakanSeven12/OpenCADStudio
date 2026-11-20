use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn sync_solid_models_for_save(&mut self, i: usize) {
        use document::EntityType;
        let scene = &mut self.tabs[i].scene;
        let targets: Vec<(document::Handle, bool)> = scene
            .document
            .entities()
            .filter_map(|entity| {
                let EntityType::Solid3D(solid) = entity else {
                    return None;
                };
                let h = solid.common.handle;
                (!solid.acis_data.has_data()).then_some((h, solid.wires.is_empty()))
            })
            .collect();
        for (h, needs_wires) in targets {
            let body = scene.solid_models.get(&h);
            let sat = body.and_then(crate::scene::convert::acis_export::solid_to_sat);
            let wires = needs_wires.then(|| {
                if let Some(body) = body {
                    return crate::scene::model::solid_model::edge_wires(body);
                }
                let Some(mesh) = scene.meshes.get(&h).or_else(|| scene.block_meshes.get(&h)) else {
                    return Vec::new();
                };
                mesh.edge_verts
                    .chunks_exact(2)
                    .enumerate()
                    .map(|(index, points)| {
                        let first_low = mesh
                            .edge_verts_low
                            .get(index * 2)
                            .copied()
                            .unwrap_or([0.0; 3]);
                        let second_low = mesh
                            .edge_verts_low
                            .get(index * 2 + 1)
                            .copied()
                            .unwrap_or([0.0; 3]);
                        document::entities::Wire::from_points(vec![
                            document::types::Vector3::new(
                                points[0][0] as f64 + first_low[0] as f64,
                                points[0][1] as f64 + first_low[1] as f64,
                                points[0][2] as f64 + first_low[2] as f64,
                            ),
                            document::types::Vector3::new(
                                points[1][0] as f64 + second_low[0] as f64,
                                points[1][1] as f64 + second_low[1] as f64,
                                points[1][2] as f64 + second_low[2] as f64,
                            ),
                        ])
                    })
                    .collect()
            });
            if let Some(EntityType::Solid3D(solid)) = scene.document.get_entity_mut(h) {
                if let Some(sat) = sat {
                    solid.set_sat_document(&sat);
                }
                if let Some(wires) = wires.filter(|wires| !wires.is_empty()) {
                    solid.wires = wires;
                }
            }
        }
    }

    /// Write a header variable on the active drawing and mark it modified.
    pub(in crate::app) fn set_drawing_var(
        &mut self,
        write: impl FnOnce(&mut document::document::HeaderVariables),
    ) {
        let i = self.active_tab;
        if let Some(tab) = self.tabs.get_mut(i) {
            write(&mut tab.scene.document.header);
            tab.dirty = true;
        }
    }

    /// Re-tessellate active-drawing solids after an ISOLINES change.
    pub(in crate::app) fn regenerate_meshes(&mut self) {
        let i = self.active_tab;
        if let Some(tab) = self.tabs.get_mut(i) {
            tab.scene.populate_meshes_from_document();
        }
    }

    /// Rebuild text geometry in every open drawing.
    ///
    /// `TEXTFILL` is a process-global, so a change to it invalidates the text
    /// in every tab. Doing only the active one leaves the others rendering the
    /// previous setting until some unrelated edit happens to rebuild them.
    pub(in crate::app) fn invalidate_text_everywhere(&mut self) {
        for tab in &mut self.tabs {
            tab.scene.invalidate_text_geometry_dependencies();
        }
    }

    /// Snapshot the persisted UI preferences from live state.
    pub(in crate::app) fn current_settings(&self) -> crate::app::settings::UserSettings {
        // One copy: the guard must not be held across the literal below.
        let block_defaults = *crate::modules::insert::block_cmdline::defaults();
        crate::app::settings::UserSettings {
            spacemouse: self.spacemouse_preferences,
            dyn_input: self.dyn_input,
            dyn_mode: self.dyn_mode,
            dyn_pointer_cartesian: self.dyn_pointer_cartesian,
            dyn_pointer_absolute: self.dyn_pointer_absolute,
            dyn_pointer_on_type: self.dyn_pointer_on_type,
            polar: self.polar_mode,
            polar_increment_deg: self.polar_increment_deg,
            zoom_wheel_reversed: self.zoom_wheel_reversed,
            navigation_style: self.navigation_style,
            dynamic_ucs: self.dynamic_ucs,
            touchpad_pan: self.touchpad_pan,
            ui_scale_percent: self.ui_scale_percent,
            zoom_factor: self.zoom_factor,
            cursor_size: self.cursor_size,
            pick_box: self.pick_box,
            aperture: self.snapper.osnap_radius_px,
            hatch_associative: self.hatch_associative,
            hatch_gap_tolerance: self.hatch_gap_tolerance,
            hatch_start_method: self.hatch_prefs.method,
            hatch_draw_path: self.hatch_prefs.path,
            hatch_path_width: self.hatch_prefs.width,
            hatch_path_alignment: self.hatch_prefs.alignment,
            hatch_boundary_warning_off: self.hatch_boundary_warning_off,
            show_menu_bar: self.show_menu_bar,
            ribbon_closed: self.ribbon_closed,
            command_line_hidden: self.command_line_hidden,
            layer_columns: crate::ui::window::layers::column_layout_names(),
            ribbon_layers_follow_filter: self.ribbon_layers_follow_filter,
            node_graph_links: self.node_graph_links,
            options_tab: self.options_tab,
            show_viewcube: self.show_viewcube,
            navvcube_opacity: self.navvcube_opacity,
            show_ucs_icon: self.show_ucs_icon,
            show_viewport_controls: self.show_viewport_controls,
            ucs_icon_at_origin: self.ucs_icon_at_origin,
            selection_cycling: self.selection_cycling > 0,
            selection_cycling_mode: Some(self.selection_cycling),
            osnap_z: self.osnap_z,
            select_similar_mode: self.select_similar_mode,
            double_click_block_refedit: self.double_click_block_refedit,
            double_click_block_attedit: self.double_click_block_attedit,
            right_click_mode: self.right_click_mode,
            right_click_hold_ms: self.right_click_hold_ms,
            quad: self.quad,
            quad_on_hover: self.quad_on_hover,
            quad_on_select: self.quad_on_select,
            quad_on_right_click: self.quad_on_right_click,
            quad_suppress_hover_when_selection: self.quad_suppress_hover_when_selection,
            quad_width: crate::app::settings::clamp_quad_width(self.quad_width),
            quad_command_launch: self.quad_command_launch,
            quad_hover_delay_ms: crate::app::settings::clamp_quad_hover_delay_ms(
                self.quad_hover_delay_ms,
            ),
            grip_object_limit: self.grip_object_limit,
            image_frame: crate::scene::frame::profile_image_mode(),
            attdef_on_screen: crate::modules::draw::draw::attdef::session().on_screen,
            table_insert: *crate::modules::annotate::table_cmd::session(),
            block_base_on_screen: block_defaults.base_on_screen,
            block_objects_on_screen: block_defaults.objects_on_screen,
            block_object_mode: crate::modules::insert::block_cmdline::mode_code(
                block_defaults.mode,
            ),
            block_allow_exploding: block_defaults.allow_exploding,
            block_open_in_editor: block_defaults.open_in_editor,
            wblock_object_mode: crate::modules::insert::block_cmdline::mode_code(
                block_defaults.wblock_mode,
            ),
            field_display: crate::entities::field::display(),
            annotation_scale_prompt: crate::app::commands::annotation_scale_prompt(),
            hyperlink_options: crate::app::commands::hyperlink_options(),
            xref_fade: crate::scene::cache::block_cache::xref_fade_ctl(),
            file_dia: crate::sys::file_dia(),
            ole_hide: crate::entities::ole2frame::hide_mode(),
            ole_quality: crate::app::commands::ole_quality(),
            mtext_toolbar: crate::app::commands::mtext_toolbar(),
            mtext_editor: crate::app::mtext_editor::prefs::prefs(),
            group_display_mode: crate::app::commands::group_display_mode(),
            selection_anno_display: crate::app::commands::selection_anno_display(),
            anno_monitor: crate::app::commands::anno_monitor(),
            log_file_mode: crate::app::commands::log_file_mode(),
            log_file_path: crate::app::commands::log_file_folder_setting(),
            save_file_path: self.save_file_path.clone(),
            offset_gap_type: crate::modules::draw::modify::offset::gap_type(),
            trim_extend_quick: crate::modules::draw::modify::trim::quick_mode(),
            match_mode: crate::entities::match_props::match_mode(),
            measuregeom_quick: crate::modules::draw::inquiry::measuregeom::quick_default(),
            revcloud_create_mode: crate::entities::revcloud::create_mode(),
            revcloud_arc_variance: crate::entities::revcloud::arc_variance(),
            revcloud_grips: crate::entities::revcloud::grips_on(),
            #[cfg(not(target_arch = "wasm32"))]
            fonts_folder: crate::io::font_repo::fonts_folder().map(|p| p.display().to_string()),
            #[cfg(not(target_arch = "wasm32"))]
            plot_styles_folder: crate::io::plot_style::plot_styles_folder()
                .map(|p| p.display().to_string()),
            support_folder: crate::io::font_repo::support_folder().map(|p| p.display().to_string()),
            #[cfg(target_arch = "wasm32")]
            fonts_folder: None,
            #[cfg(target_arch = "wasm32")]
            plot_styles_folder: None,
            attach_filter: crate::sys::attach_filter(),
            surface_associativity: crate::scene::model::sweep_model::surface_associativity(),
            ucs_ortho: crate::modules::view::view_cmd::ucs_ortho(),
            ucs_view: crate::modules::view::view_cmd::ucs_view(),
            layout_create_viewport: crate::scene::layout_create_viewport(),
            pdf_shx_text: crate::modules::insert::pdf_shx_text::settings(),
            dgn_import: crate::modules::insert::dgn_import::settings(),
            ncopy_bind: self.ncopy_bind,
            cursor_type: self.cursor_type,
            crosshair_color: self.crosshair_color,
            snap_marker_color: self.snap_marker_color,
            command_text_color: self.command_text_color,
            lineweight_display_scale: self.lineweight_display_scale,
            isometric_drafting: self.isometric_drafting,
            iso_plane: self.iso_plane,
            snap_angle_deg: self.snap_angle_deg,
            otrack: self.snapper.otrack_enabled,
            default_assoc_prompted: self.default_assoc_prompted,
            check_missing_fonts: self.check_missing_fonts,
            font_source_url: self.font_source_url.clone(),
            qnew_template: self.qnew_template.clone(),
            transmittal_layout: self.transmittal_layout,
            donation_prompt_version: self.donation_prompt_version.clone(),
            gpu_warning_silenced: self.gpu_warning_silenced.clone(),
            disabled_plugins: {
                let mut v: Vec<String> = self.disabled_plugins.iter().cloned().collect();
                v.sort();
                v
            },
            plugin_repos: self.plugin_repos.clone(),
            literal_spaces: self.command_line.literal_spaces,
            command_history_height: self.command_line.history_height,
            osmode: crate::app::settings::osmode_from_snaps(
                self.snapper.enabled.iter(),
                self.snapper.snap_enabled,
            ),
            texteditmode: self.texteditmode,
            quick_dimension_snap_priority: self.quick_dimension_snap_priority,
            dimension_continue_mode: self.dimension_continue_mode,
            delete_objects: self.delete_objects,
            textfill: crate::scene::text::sdf_atlas::textfill(),
            backup_on_save: self.backup_on_save,
            file_assoc_enabled: self.file_assoc_enabled,
            show_constraint_values: self.show_constraint_values,
            auto_constrain: self.auto_constrain_settings.clone(),
            txt2mtxt: self.txt2mtxt_settings,
            spell: self.spell_prefs.clone(),
            dim_inspect: self.dim_inspect_settings.clone(),
            constraint_solve_mode: self.constraint_solve_mode,
            constraint_infer: self.constraint_infer,
            constraint_bar_display: self.constraint_bar_display,
            constraint_bar_mode: self.constraint_bar_mode,
            savetime_min: self.savetime_min,
            script_commands: self.script_commands,
            default_save_format: self.default_save_format.clone(),
            pick_add: self.pick_add,
            pick_add_retains: self.pick_add_retains,
            pick_drag_rect: self.pick_drag_mode != 0 && self.pick_auto & 4 == 0,
            pick_drag_mode: Some(self.pick_drag_mode),
            pick_auto: self.pick_auto,
            pick_first: self.pick_first,
            quick_properties: self.quick_properties,
            quick_properties_float: self.quick_properties_float,
            quick_properties_pos: [self.quick_properties_pos.x, self.quick_properties_pos.y],
            language: self.language,
            cliprompt_lines: crate::app::settings::clamp_clipromptlines(self.cliprompt_lines),
            command_line_opacity: self.command_line.opacity_percent(),
            command_line_font: self.command_line.font_family().to_owned(),
            command_line_font_size: self.command_line.font_size(),
            commandline_fade_ms: crate::app::settings::clamp_commandline_fade_ms(
                self.commandline_fade_ms,
            ),
            snap_spacing_x: self.snapper.snap_spacing_x,
            snap_spacing_y: self.snapper.snap_spacing_y,
            grid_spacing_x: self.grid_spacing_x,
            grid_spacing_y: self.grid_spacing_y,
            grid_major_every: self.grid_major_every,
            grid_adaptive: self.grid_adaptive,
            grid_beyond_limits: self.grid_beyond_limits,
            grid_axes: self.grid_axes,
            block_mru: self.block_mru.clone(),
            block_freq: self.block_freq.clone(),
            block_recent: self.block_palette.recent.clone(),
            block_favorites: self.block_palette.favorites.clone(),
            block_libraries: self.block_palette.libraries.clone(),
            block_palette_view: self.block_palette.view as u8,
            block_insert: self.block_palette.options.clone(),
            block_mru_list: self.block_mru_list,
            block_redefine_mode: self.block_redefine_mode,
            block_navigate: self.block_navigate.clone(),
            block_editor: self.block_editor_preferences.clone(),
            refedit: self.refedit_options.clone(),
            xfadectl: crate::scene::cache::block_cache::refedit_fade_ctl() as u8,
            count_color: self.count_palette.color,
            count_error_color: self.count_palette.error_color,
            count_service: self.count_palette.service,
            count_invalid_area: self.count_palette.invalid_choice,
            sheet_set: self.sheet_set.settings,
            quick_calc: self.quick_calc.settings(),
            tool_palettes: self.tool_palettes.to_persisted(),
        }
    }

    /// Apply restored preferences to live state.
    pub(in crate::app) fn apply_settings(&mut self, s: &crate::app::settings::UserSettings) {
        self.dyn_mode = s.dyn_mode;
        self.dyn_pointer_cartesian = s.dyn_pointer_cartesian;
        self.dyn_pointer_absolute = s.dyn_pointer_absolute;
        self.dyn_pointer_on_type = s.dyn_pointer_on_type;
        self.set_dyn_input(s.dyn_input);
        self.polar_mode = s.polar;
        self.polar_increment_deg = s.polar_increment_deg;
        self.zoom_wheel_reversed = s.zoom_wheel_reversed;
        self.navigation_style = s.navigation_style;
        self.dynamic_ucs = s.dynamic_ucs;
        self.touchpad_pan = s.touchpad_pan;
        self.ui_scale_percent = crate::app::settings::clamp_ui_scale(s.ui_scale_percent);
        self.zoom_factor = crate::app::settings::clamp_zoom_factor(s.zoom_factor);
        // Like the drafting-rotation field below, the Options field edits a
        // buffer rather than the value, so it is reseeded whenever the value
        // is restored from behind it — a start-up, or the Options window's
        // own Close putting a change back.
        self.zoom_factor_input = self.zoom_factor.to_string();
        self.cursor_size = s.cursor_size.clamp(1, 100);
        self.pick_box = s.pick_box.clamp(0, 50);
        self.snapper.osnap_radius_px = if s.aperture.is_finite() {
            s.aperture.clamp(
                crate::snap::MIN_OSNAP_RADIUS_PX,
                crate::snap::MAX_OSNAP_RADIUS_PX,
            )
        } else {
            crate::snap::DEFAULT_OSNAP_RADIUS_PX
        };
        self.hatch_associative = s.hatch_associative;
        let max_gap = crate::modules::draw::draw::hatch::MAX_GAP_TOLERANCE;
        self.hatch_gap_tolerance = s.hatch_gap_tolerance;
        if !(0.0..=max_gap).contains(&self.hatch_gap_tolerance) {
            self.hatch_gap_tolerance = 0.0;
        }
        self.hatch_prefs = crate::modules::draw::draw::hatch::HatchPrefs {
            method: s.hatch_start_method.min(2),
            path: s.hatch_draw_path,
            width: if s.hatch_path_width.is_finite() && s.hatch_path_width > 0.0 {
                s.hatch_path_width
            } else {
                0.25
            },
            alignment: s.hatch_path_alignment.min(2),
        };
        self.hatch_boundary_warning_off = s.hatch_boundary_warning_off;
        self.show_menu_bar = s.show_menu_bar;
        self.ribbon_closed = s.ribbon_closed;
        self.command_line_hidden = s.command_line_hidden;
        crate::ui::window::layers::set_column_layout_names(&s.layer_columns);
        self.ribbon_layers_follow_filter = s.ribbon_layers_follow_filter;
        self.node_graph_links = s.node_graph_links;
        self.options_tab = s.options_tab;
        self.spacemouse_preferences = s.spacemouse;
        // These four drive real features with commands and status-bar pills,
        // but they lived only on the app struct: turning the ViewCube off and
        // restarting brought it straight back.
        self.show_viewcube = s.show_viewcube;
        self.navvcube_opacity = s.navvcube_opacity.min(100);
        self.show_ucs_icon = s.show_ucs_icon;
        self.show_viewport_controls = s.show_viewport_controls;
        // The ribbon's toggles keep their own copy of both.
        self.ribbon.set_viewcube(self.show_viewcube);
        self.ribbon.set_ucs_icon(self.show_ucs_icon);
        self.ucs_icon_at_origin = s.ucs_icon_at_origin;
        self.selection_cycling = s
            .selection_cycling_mode
            .filter(|mode| (-2..=2).contains(mode))
            .unwrap_or(if s.selection_cycling { 2 } else { 0 });
        self.osnap_z = s.osnap_z;
        self.select_similar_mode = s.select_similar_mode & 255;
        self.double_click_block_refedit = s.double_click_block_refedit;
        self.double_click_block_attedit = s.double_click_block_attedit;
        self.right_click_mode = s.right_click_mode;
        self.right_click_hold_ms =
            crate::app::settings::clamp_right_click_hold_ms(s.right_click_hold_ms);
        self.quad = s.quad;
        self.quad_on_hover = s.quad_on_hover;
        self.quad_on_select = s.quad_on_select;
        self.quad_on_right_click = s.quad_on_right_click;
        self.quad_suppress_hover_when_selection = s.quad_suppress_hover_when_selection;
        self.quad_width = crate::app::settings::clamp_quad_width(s.quad_width);
        self.quad_command_launch = s.quad_command_launch;
        self.quad_hover_delay_ms =
            crate::app::settings::clamp_quad_hover_delay_ms(s.quad_hover_delay_ms);
        self.grip_object_limit = s.grip_object_limit.clamp(0, 32767);
        crate::scene::frame::set_profile_image_mode(s.image_frame);
        crate::modules::draw::draw::attdef::session().on_screen = s.attdef_on_screen;
        *crate::modules::annotate::table_cmd::session() = s.table_insert;
        {
            use crate::modules::insert::block_cmdline as bc;
            let mut remembered = bc::defaults();
            remembered.base_on_screen = s.block_base_on_screen;
            remembered.objects_on_screen = s.block_objects_on_screen;
            remembered.mode = bc::mode_from_code(s.block_object_mode);
            remembered.allow_exploding = s.block_allow_exploding;
            remembered.open_in_editor = s.block_open_in_editor;
            remembered.wblock_mode = bc::mode_from_code(s.wblock_object_mode);
        }
        crate::entities::field::set_display(s.field_display);
        crate::app::commands::set_annotation_scale_prompt(s.annotation_scale_prompt);
        crate::app::commands::set_hyperlink_options(s.hyperlink_options);
        crate::scene::cache::block_cache::set_xref_fade_ctl(s.xref_fade);
        self.ribbon.xref_fade = crate::scene::cache::block_cache::xref_fade_ctl();
        crate::sys::set_file_dia(s.file_dia);
        crate::entities::ole2frame::set_hide_mode(s.ole_hide);
        crate::app::commands::set_ole_quality(s.ole_quality);
        crate::app::commands::set_mtext_toolbar(s.mtext_toolbar);
        crate::app::mtext_editor::prefs::set_prefs(s.mtext_editor.clone());
        crate::app::commands::set_group_display_mode(s.group_display_mode);
        crate::app::commands::set_selection_anno_display(s.selection_anno_display);
        crate::app::commands::set_anno_monitor(s.anno_monitor);
        // The log's folder and mode are process-wide, and every test that builds
        // an app comes through here beside the one test that turns the log on.
        if !cfg!(test) {
            crate::app::commands::set_log_file_folder(&s.log_file_path);
            crate::app::commands::set_log_file_mode(s.log_file_mode);
        }
        crate::modules::draw::modify::offset::set_gap_type(s.offset_gap_type);
        crate::modules::draw::modify::trim::set_quick_mode(s.trim_extend_quick);
        crate::entities::match_props::set_match_mode(s.match_mode);
        crate::modules::draw::inquiry::measuregeom::set_quick_default(s.measuregeom_quick);
        crate::entities::revcloud::set_create_mode(s.revcloud_create_mode);
        crate::entities::revcloud::set_arc_variance(s.revcloud_arc_variance);
        crate::entities::revcloud::set_grips_on(s.revcloud_grips);
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::io::font_repo::set_fonts_folder(s.fonts_folder.as_ref().map(Into::into));
            crate::io::plot_style::set_plot_styles_folder(
                s.plot_styles_folder.as_ref().map(Into::into),
            );
            crate::io::font_repo::set_support_folder(s.support_folder.as_ref().map(Into::into));
        }
        crate::sys::set_attach_filter(s.attach_filter);
        crate::scene::model::sweep_model::set_surface_associativity(s.surface_associativity);
        crate::modules::view::view_cmd::set_ucs_ortho(s.ucs_ortho);
        crate::modules::view::view_cmd::set_ucs_view(s.ucs_view);
        crate::scene::set_layout_create_viewport(s.layout_create_viewport);
        crate::modules::insert::pdf_shx_text::set_settings(s.pdf_shx_text.clone());
        crate::modules::insert::dgn_import::set_settings(s.dgn_import);
        self.ncopy_bind = s.ncopy_bind;
        self.cursor_type = s.cursor_type;
        self.crosshair_color = s.crosshair_color;
        self.crosshair_color_input = s
            .crosshair_color
            .map(crate::app::config::rgb_to_hex)
            .unwrap_or_default();
        self.snap_marker_color = s.snap_marker_color;
        self.snap_marker_color_input = s
            .snap_marker_color
            .map(crate::app::config::rgb_to_hex)
            .unwrap_or_default();
        self.command_text_color = s.command_text_color;
        self.command_text_color_input = s
            .command_text_color
            .map(crate::app::config::rgb_to_hex)
            .unwrap_or_default();
        crate::ui::command_line::set_text_color(s.command_text_color);
        self.lineweight_display_scale = s.lineweight_display_scale.clamp(25, 200);
        self.isometric_drafting = s.isometric_drafting;
        self.iso_plane = s.iso_plane;
        self.snap_angle_deg = if s.snap_angle_deg.is_finite() {
            s.snap_angle_deg.rem_euclid(360.0)
        } else {
            0.0
        };
        // The Options field edits a buffer rather than the value, so it has to
        // be reseeded whenever the value is restored from behind it.
        self.snap_angle_input = crate::app::settings::format_snap_angle(self.snap_angle_deg);
        // Ortho + running OSNAP are per-drawing (adopted from the header on
        // open / tab switch), not app-global, so they are not applied here.
        self.snapper.otrack_enabled = s.otrack;
        self.default_assoc_prompted = s.default_assoc_prompted;
        self.check_missing_fonts = s.check_missing_fonts;
        self.font_source_url = s.font_source_url.clone();
        self.qnew_template = s.qnew_template.clone();
        self.transmittal_layout = s.transmittal_layout;
        self.font_source_input = s.font_source_url.clone();
        self.donation_prompt_version = s.donation_prompt_version.clone();
        self.gpu_warning_silenced = s.gpu_warning_silenced.clone();
        self.disabled_plugins = s.disabled_plugins.iter().cloned().collect();
        self.plugin_repos = s.plugin_repos.clone();
        self.command_line.literal_spaces = s.literal_spaces;
        self.command_line.history_height = if s.command_history_height.is_finite() {
            s.command_history_height.clamp(
                crate::ui::command_line::HISTORY_HEIGHT_MIN,
                crate::ui::command_line::HISTORY_HEIGHT_MAX,
            )
        } else {
            crate::ui::command_line::HISTORY_HEIGHT_DEFAULT
        };
        let (modes, snap_enabled) = crate::app::settings::snaps_from_osmode(s.osmode);
        self.snapper.enabled = modes.into_iter().collect();
        self.snapper.snap_enabled = snap_enabled;
        self.texteditmode = s.texteditmode;
        self.quick_dimension_snap_priority = s.quick_dimension_snap_priority.min(1);
        self.dimension_continue_mode = s.dimension_continue_mode.clamp(0, 1);
        self.delete_objects = s.delete_objects.clamp(0, 3);
        crate::scene::text::sdf_atlas::set_textfill(s.textfill);
        self.backup_on_save = s.backup_on_save;
        self.file_assoc_enabled = s.file_assoc_enabled;
        self.show_constraint_values = s.show_constraint_values;
        self.auto_constrain_settings = s.auto_constrain.clone();
        self.txt2mtxt_settings = s.txt2mtxt;
        self.spell_prefs = s.spell.clone();
        self.dim_inspect_settings = s.dim_inspect.clone();
        self.auto_constrain_settings.sanitize();
        self.auto_constrain_distance_input =
            format!("{}", self.auto_constrain_settings.distance_tolerance);
        self.auto_constrain_angle_input =
            format!("{}", self.auto_constrain_settings.angle_tolerance_deg);
        self.constraint_solve_mode = s.constraint_solve_mode;
        self.constraint_infer = s.constraint_infer;
        self.constraint_bar_display = s.constraint_bar_display.clamp(0, 3);
        self.constraint_bar_mode = s.constraint_bar_mode.clamp(0, 4095);
        self.savetime_min = s.savetime_min;
        self.save_file_path = s.save_file_path.clone();
        self.script_commands = s.script_commands;
        self.default_save_format =
            crate::io::canonical_save_format(&s.default_save_format).to_string();
        self.pick_add = s.pick_add;
        self.pick_add_retains = s.pick_add && s.pick_add_retains;
        let pick_drag_mode = s.pick_drag_mode.filter(|&mode| mode <= 2);
        self.pick_drag_mode = pick_drag_mode.unwrap_or(2);
        self.pick_auto = if pick_drag_mode.is_none() && s.pick_drag_rect {
            // Older profiles used a rectangle for both click-click and drag.
            3
        } else {
            s.pick_auto & 7
        };
        self.pick_first = s.pick_first;
        self.quick_properties = s.quick_properties;
        self.quick_properties_float = s.quick_properties_float;
        self.quick_properties_pos = iced::Point::new(
            s.quick_properties_pos[0].max(0.0),
            s.quick_properties_pos[1].max(0.0),
        );
        if crate::i18n::set_language(s.language).is_ok() {
            self.language = s.language;
        }
        self.cliprompt_lines = crate::app::settings::clamp_clipromptlines(s.cliprompt_lines);
        self.command_line
            .set_cliprompt_lines(self.cliprompt_lines.clamp(0, 50) as u8);
        self.command_line
            .set_opacity_percent(s.command_line_opacity);
        self.command_line
            .set_font(&s.command_line_font, s.command_line_font_size);
        self.commandline_fade_ms =
            crate::app::settings::clamp_commandline_fade_ms(s.commandline_fade_ms);
        self.command_line
            .set_commandline_fade_ms(self.commandline_fade_ms.clamp(0, 60000) as u32);
        self.snapper.snap_spacing_x = crate::app::settings::sanitize_snap_spacing(s.snap_spacing_x);
        self.snapper.snap_spacing_y = crate::app::settings::sanitize_snap_spacing(s.snap_spacing_y);
        self.grid_spacing_x = crate::app::settings::sanitize_snap_spacing(s.grid_spacing_x);
        self.grid_spacing_y = crate::app::settings::sanitize_snap_spacing(s.grid_spacing_y);
        self.grid_major_every = crate::app::settings::sanitize_grid_major(s.grid_major_every);
        self.grid_adaptive = s.grid_adaptive;
        self.grid_beyond_limits = s.grid_beyond_limits;
        self.grid_axes = s.grid_axes;
        // Block usage: clone but cap to sane sizes (MRU 20, freq map 200)
        self.block_mru = s.block_mru.iter().take(20).cloned().collect();
        self.block_freq = s
            .block_freq
            .iter()
            .take(200)
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        self.block_mru_list = s.block_mru_list.min(100);
        self.block_redefine_mode = s.block_redefine_mode.min(2);
        self.block_navigate = s.block_navigate.clone();
        self.block_editor_preferences = s.block_editor.clone();
        crate::scene::cache::block_cache::set_refedit_fade_ctl(i32::from(s.xfadectl.min(90)));
        self.refedit_options = s.refedit.clone();
        self.count_palette.color = s.count_color;
        self.count_palette.error_color = s.count_error_color;
        self.count_palette.service = s.count_service;
        self.count_palette.invalid_choice = s.count_invalid_area.min(2);
        self.sheet_set.settings = s.sheet_set;
        self.quick_calc.apply_settings(&s.quick_calc);
        self.tool_palettes.apply_persisted(s.tool_palettes.clone());
        self.refresh_tool_palettes();
        self.block_palette.recent = s.block_recent.clone();
        self.block_palette.favorites = s.block_favorites.clone();
        self.block_palette.libraries = s.block_libraries.clone();
        self.block_palette.view =
            crate::ui::window::block_palette::ViewMode::from_u8(s.block_palette_view);
        self.block_palette.set_options(s.block_insert.clone());
        self.trim_recent_blocks();
        // Push restored display defaults onto every drawing tab that exists now.
        // Tabs created later pick them up at their construction site.
        for idx in 0..self.tabs.len() {
            self.apply_display_defaults(idx);
        }
        self.rebuild_ribbon_modules();
    }

    pub(crate) fn record_block_insert(&mut self, name: &str) {
        self.tabs[self.active_tab].insname = name.to_string();
        self.note_recent_block(name);
        let key = name.to_ascii_uppercase();
        *self.block_freq.entry(key).or_insert(0) += 1;
        self.block_mru.retain(|n| !n.eq_ignore_ascii_case(name));
        self.block_mru.insert(0, name.to_string());
        if self.block_mru.len() > 20 {
            self.block_mru.truncate(20);
        }
        // Cap freq map to 200 most frequent to bound persistence size.
        if self.block_freq.len() > 200 {
            let mut items: Vec<(String, u32)> = self
                .block_freq
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            items.sort_by(|a, b| b.1.cmp(&a.1));
            items.truncate(200);
            self.block_freq = items.into_iter().collect();
        }
        // Debounce disk writes: at most once per second (2.4) to avoid thrash
        // during rapid scripting/batch inserts. Immediate first write ensures
        // crash recovery. Wasm has no `std::time::Instant` guarantee, so persist
        // immediately there.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = std::time::Instant::now();
            let should_persist = match self.block_usage_last_persist {
                None => true,
                Some(t) => now.duration_since(t).as_secs_f32() >= 1.0,
            };
            if should_persist {
                self.block_usage_last_persist = Some(now);
                self.persist_settings_if_changed();
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.persist_settings_if_changed();
        }
    }

    pub(crate) fn block_usage_snapshot(&self) -> rustc_hash::FxHashMap<String, (u32, usize)> {
        let mut out = rustc_hash::FxHashMap::default();
        for (idx, name) in self.block_mru.iter().enumerate() {
            let up = name.to_ascii_uppercase();
            let freq = self.block_freq.get(&up).copied().unwrap_or(0);
            out.insert(up, (freq, idx));
        }
        // Ensure every freq entry has at least an entry (for blocks not in MRU)
        for (up, freq) in &self.block_freq {
            out.entry(up.clone()).or_insert((*freq, usize::MAX));
        }
        out
    }

    pub(crate) fn ranked_block_names(&self, names: &[String]) -> Vec<String> {
        let mut scored: Vec<(&String, u32, usize)> = names
            .iter()
            .map(|n| {
                let up = n.to_ascii_uppercase();
                let f = self.block_freq.get(&up).copied().unwrap_or(0);
                let m = self
                    .block_mru
                    .iter()
                    .position(|x| x.eq_ignore_ascii_case(n))
                    .unwrap_or(usize::MAX);
                (n, f, m)
            })
            .collect();
        scored.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.0.cmp(b.0))
        });
        scored.into_iter().map(|(n, _, _)| n.clone()).collect()
    }

    /// Adopt the per-drawing sysvars stored in tab `i`'s document header —
    /// Ortho (`$ORTHOMODE`) and the running object-snap set (`$OSMODE`) — into
    /// the live app state, so each drawing keeps its own. Called when a drawing
    /// becomes active (open, tab switch). The app-settings values seed the boot
    /// default; the per-file header overrides it here.
    pub(in crate::app) fn adopt_header_sysvars(&mut self, i: usize) {
        if self.tabs[i].is_start {
            return;
        }
        let (ortho, osmode) = {
            let h = &self.tabs[i].scene.document.header;
            (h.ortho_mode, h.object_snap_mode)
        };
        // Header dimension variables that differ from the current style are
        // its overrides.
        self.tabs[i].scene.dim_overrides =
            crate::scene::creation_style::header_dim_overrides(&self.tabs[i].scene.document);
        self.ortho_mode = ortho;
        // Ortho and Polar are mutually exclusive (ToggleOrtho clears Polar).
        if ortho {
            self.polar_mode = false;
        }
        // OSMODE has no file slot in modern DWG (R2000+ moved it to the
        // registry), so a header value of 0 just means "absent" — keep the
        // user's app-level set instead of wiping it. Only a legacy R13/R14 or
        // DXF file that really carries a nonzero mask overrides.
        if osmode != 0 {
            let (modes, snap_enabled) = crate::app::settings::snaps_from_osmode(osmode);
            self.snapper.enabled = modes.into_iter().collect();
            self.snapper.snap_enabled = snap_enabled;
        }
    }

    /// Stamp the live per-drawing sysvars (Ortho, running OSNAP) onto tab `i`'s
    /// document header just before it is written to disk (or before switching
    /// away from it), so they persist with the drawing.
    pub(in crate::app) fn stamp_header_sysvars(&mut self, i: usize) {
        if self.tabs[i].is_start {
            return;
        }
        let osmode = crate::app::settings::osmode_from_snaps(
            self.snapper.enabled.iter(),
            self.snapper.snap_enabled,
        );
        let h = &mut self.tabs[i].scene.document.header;
        h.ortho_mode = self.ortho_mode;
        h.object_snap_mode = osmode;
        // The header's dimension variables are the current style with its
        // overrides, so the overrides stay with the drawing.
        let scene = &mut self.tabs[i].scene;
        crate::scene::creation_style::stamp_header_dim_overrides(
            &mut scene.document,
            scene.dim_overrides.as_ref(),
        );
    }

    /// Apply persisted viewport display defaults to tab `idx`.
    pub(in crate::app) fn apply_display_defaults(&mut self, idx: usize) {
        let bg = self.default_bg_color;
        let paper_bg = self.default_paper_bg_color;
        let tab = &mut self.tabs[idx];
        if tab.is_start {
            return;
        }
        tab.scene.model_lineweight_scale = self.lineweight_display_scale as f32 / 100.0;
        tab.scene.selection_color = self.model_space.resolve_selection_color();
        tab.scene.selection_effect = self.model_space.selection_effect;
        tab.scene.selection_widen = f32::from(self.model_space.selection_highlight_widen.min(6));
        tab.scene.desk_bg_color = self.model_space.resolve_desk_bg();
        if bg.is_none() && paper_bg.is_none() {
            return;
        }
        if let Some(c) = bg {
            tab.bg_color = Some(c);
            tab.scene.bg_color = c;
        }
        if let Some(c) = paper_bg {
            tab.paper_bg_color = Some(c);
            tab.scene.paper_bg_color = c;
        }
        tab.scene.recolor_meshes();
        tab.scene.bump_geometry();
    }

    /// Check if a suspended command exists on the active tab and resume it
    /// with the outcome of the text editor.
    pub(in crate::app) fn post_editor_closed(&mut self, committed: bool) -> Task<Message> {
        self.reset_modal_geometry();
        let i = self.active_tab;
        if let Some(mut cmd) = self.tabs[i].suspended_cmd.take() {
            if committed {
                if let Some(value) = self.pending_command_editor_text.take() {
                    cmd.on_editor_text(value);
                }
            } else {
                self.pending_command_editor_text = None;
            }
            let res = cmd.on_editor_closed(committed);
            self.tabs[i].active_cmd = Some(cmd);
            self.apply_cmd_result(res)
        } else {
            self.focus_cmd_input()
        }
    }

    /// Rebuild the ribbon's tab list from the registry, dropping the tabs of any
    /// disabled plugins. Call after `disabled_plugins` changes.
    pub(in crate::app) fn rebuild_ribbon_modules(&mut self) {
        let modules = crate::plugin::ribbon_modules_enabled(&self.disabled_plugins);
        self.ribbon.set_modules(modules);
        // Refresh the command-line autocomplete pool so a newly enabled plugin's
        // commands become typeable (and a disabled one's drop out). This runs on
        // startup load, settings reload, and every enable/disable toggle (#272).
        let mut cmds = crate::plugin::plugin_command_names(&self.disabled_plugins);
        for state in self.plugin_panels.values() {
            if let Some(ref c) = state.def.command {
                let cu = c.to_uppercase();
                if !cmds.contains(&cu) {
                    cmds.push(cu);
                }
            }
            let id_u = state.def.id.to_uppercase();
            if !cmds.contains(&id_u) {
                cmds.push(id_u);
            }
        }
        self.command_line.dynamic_commands = cmds;
    }

    /// Snapshot of disabled plugin ids — lets the registry skip them while it
    /// holds a `&mut` borrow of the app via `HostSession`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn disabled_plugin_ids(&self) -> rustc_hash::FxHashSet<String> {
        self.disabled_plugins.clone()
    }

    /// Background task: fetch the curated plugin registry.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn fetch_registry_task(&self) -> Task<Message> {
        Task::perform(
            async {
                on_worker_thread(crate::plugin::marketplace::fetch_registry)
                    .await
                    .unwrap_or_else(|| Err("registry fetch thread died".into()))
            },
            Message::PluginRegistryFetched,
        )
    }

    /// Background task: fetch `owner/repo`'s installable releases and their
    /// manifest API versions.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn fetch_releases_task(&mut self, repo: String) -> Task<Message> {
        self.plugin_releases_loading.insert(repo.clone());
        let label = repo.clone();
        Task::perform(
            async move {
                on_worker_thread(move || crate::plugin::marketplace::fetch_release_info(&repo))
                    .await
                    .unwrap_or_else(|| Err("release fetch thread died".into()))
            },
            move |res| Message::PluginReleasesFetched(label, res),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn fetch_releases_task(&mut self, repo: String) -> Task<Message> {
        Task::done(Message::PluginReleasesFetched(
            repo,
            Err("External plugins are available in the desktop app.".to_string()),
        ))
    }

    /// Background task: fetch a repository README from its default branch.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn fetch_plugin_readme_task(&self, repo: String) -> Task<Message> {
        let label = repo.clone();
        Task::perform(
            async move {
                on_worker_thread(move || crate::plugin::marketplace::fetch_readme(&repo))
                    .await
                    .unwrap_or_else(|| Err("README fetch thread died".into()))
            },
            move |result| Message::PluginReadmeFetched(label, result),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn fetch_plugin_readme_task(&self, repo: String) -> Task<Message> {
        Task::done(Message::PluginReadmeFetched(
            repo,
            Err("Plugin details are available in the desktop app.".to_string()),
        ))
    }

    /// Background task: download and install the `tag` release of `owner/repo`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn install_task(&self, repo: String, tag: String) -> Task<Message> {
        let label = repo.clone();
        Task::perform(
            async move {
                on_worker_thread(move || {
                    let releases = crate::plugin::marketplace::fetch_releases(&repo)?;
                    let rel = releases
                        .into_iter()
                        .find(|r| r.tag == tag)
                        .ok_or_else(|| format!("release {tag} not found"))?;
                    crate::plugin::marketplace::install(&rel, &repo)
                })
                .await
                .unwrap_or_else(|| Err("install thread died".into()))
            },
            move |result| Message::PluginInstalled(label, result),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn install_task(&self, repo: String, _tag: String) -> Task<Message> {
        Task::done(Message::PluginInstalled(
            repo,
            Err("External plugins are available in the desktop app.".to_string()),
        ))
    }

    /// Gather the full persisted config (all sections) from live app state.
    pub(in crate::app) fn current_config(&self) -> crate::app::config::AppConfig {
        crate::app::config::AppConfig {
            settings: self.current_settings(),
            theme: self.ui_theme.clone(),
            recent: crate::app::config::RecentConfig {
                files: self
                    .recent_files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                limit: self.recent_limit,
                view: self.recent_view,
            },
            start: crate::app::config::StartConfig {
                section: self.start_section,
            },
            statusbar: self.statusbar_config.clone(),
            dock: {
                let mut dock = self.dock.clone();
                dock.ensure_settings();
                dock
            },
            show_graphic_attributes: self.show_graphic_attributes,
            show_tool_palettes: self.show_tool_palettes,
            show_command_history: self.show_command_history,
            show_layers: self.show_layers,
            show_block_palette: self.show_block_palette,
            show_file_tabs: self.show_file_tabs,
            file_tabs_above_docks: self.file_tabs_above_docks,
            show_layout_tabs: self.show_layout_tabs,
            graphic_fills_on_current_layer: self.graphic_attributes.fill_on_current_layer,
            annotation_auto_scale: self.annotation_auto_scale,
            ribbon: crate::app::config::RibbonConfig {
                layout: self.ribbon.layout().clone(),
            },
            plot: self.plot_dialog.clone(),
            context_menu: self.context_menu_custom.clone(),
            mcp_enabled: self.control.start_enabled,
            shortcuts: crate::app::config::ShortcutConfig {
                bindings: self
                    .shortcut_bindings
                    .iter()
                    .map(|(key, command)| (key.clone(), command.clone()))
                    .collect(),
                // Loading offered every default (see `apply_config`).
                known_defaults: crate::app::shortcuts::default_bindings()
                    .into_keys()
                    .collect(),
            },
            model_space: self.model_space.clone(),
        }
    }

    /// Distribute a loaded config into live app state (called once at startup).
    pub(in crate::app) fn apply_config(&mut self, cfg: crate::app::config::AppConfig) {
        self.apply_settings(&cfg.settings);
        self.ui_theme = cfg.theme.clone();
        // A saved `System` scheme re-resolves at boot: the stored palette is
        // whatever the last resolution produced, so refresh it alongside the
        // live theme (COLORSCHEME SYSTEM documents this boot-time read).
        if self.ui_theme.name.eq_ignore_ascii_case("System") {
            self.ui_theme.palette = crate::app::config::UiThemePalette::from_iced(
                crate::app::config::resolve_system_theme().seed(),
            );
        }
        self.active_theme = self.ui_theme.to_iced();
        self.theme_color_inputs = self.ui_theme.palette.hex_values();
        self.model_space = cfg.model_space;
        self.model_bg_input = self
            .model_space
            .custom_bg
            .map(crate::app::config::rgb_to_hex)
            .unwrap_or_default();
        self.paper_bg_input = self
            .model_space
            .custom_paper_bg
            .map(crate::app::config::rgb_to_hex)
            .unwrap_or_default();
        self.desk_bg_input = self
            .model_space
            .custom_desk_bg
            .map(crate::app::config::rgb_to_hex)
            .unwrap_or_default();
        self.sync_model_space_theme(false);
        self.recent_files = cfg
            .recent
            .files
            .iter()
            .map(std::path::PathBuf::from)
            .collect();
        self.recent_limit = cfg.recent.limit.clamp(
            crate::app::recent::RECENT_MIN,
            crate::app::recent::RECENT_MAX,
        );
        self.recent_files.truncate(self.recent_limit);
        self.recent_limit_input = self.recent_limit.to_string();
        self.recent_view = cfg.recent.view;
        // Thumbnails are decoded by a background task queued at boot
        // (`refresh_recent_thumbs`) — never here on the boot path.
        self.start_section = cfg.start.section;
        self.statusbar_config = cfg.statusbar;
        let mut dock = cfg.dock;
        dock.ensure_settings();
        self.dock = dock;
        self.show_graphic_attributes = cfg.show_graphic_attributes;
        self.show_tool_palettes = cfg.show_tool_palettes;
        self.ribbon.show_tool_palettes = self.show_tool_palettes;
        self.show_command_history = cfg.show_command_history;
        if self.show_command_history {
            self.sync_command_history_panel();
        }
        // The pallets and tabs come back as they were left (#2616).
        self.show_layers = cfg.show_layers && self.dock.is_placed(crate::ui::dock::PanelId::Layers);
        self.show_block_palette =
            cfg.show_block_palette && self.dock.is_placed(crate::ui::dock::PanelId::BlockPalette);
        self.show_file_tabs = cfg.show_file_tabs;
        self.control.start_enabled = cfg.mcp_enabled;
        self.file_tabs_above_docks = cfg.file_tabs_above_docks;
        self.ribbon.set_file_tabs(self.show_file_tabs);
        self.show_layout_tabs = cfg.show_layout_tabs;
        self.ribbon.set_layout_tabs(self.show_layout_tabs);
        self.graphic_attributes.fill_on_current_layer = cfg.graphic_fills_on_current_layer;
        if self.show_graphic_attributes {
            self.dock
                .dock_below_properties(crate::ui::dock::PanelId::GraphicAttributes);
        }
        self.ribbon
            .set_graphic_attributes(self.show_graphic_attributes);
        self.annotation_auto_scale = cfg.annotation_auto_scale.clamp(-4, 4);
        self.ribbon.set_layout(cfg.ribbon.layout);
        self.plot_dialog = cfg.plot;
        self.context_menu_custom = cfg.context_menu;
        let known = cfg.shortcuts.known_defaults;
        self.shortcut_bindings = cfg.shortcuts.bindings.into_iter().collect();
        // A default shipped after the table was saved (Ctrl+9, Shift+F12 …)
        // is added once; a default the user removed is not brought back.
        for (key, action) in crate::app::shortcuts::default_bindings() {
            if !known.contains(&key) {
                self.shortcut_bindings.entry(key).or_insert(action);
            }
        }
        crate::app::shortcuts::sync_space_dispatch_flag(&self.shortcut_bindings);
    }

    /// Write the config only when it changed since the last write, so a toggle
    /// persists immediately without thrashing native or browser storage.
    pub(in crate::app) fn save_config(&mut self) {
        let cur = self.current_config();
        if self.last_saved_config.as_ref() != Some(&cur) {
            cur.save();
            self.last_saved_config = Some(cur);
        }
    }

    /// Back-compat name for the many "a preference changed, persist it" sites.
    pub(in crate::app) fn persist_settings_if_changed(&mut self) {
        // Keep resize feedback live without writing settings on every pointer
        // move. The release message saves the final height once.
        if !self.command_history_resizing {
            self.save_config();
        }
    }

    /// Sync the active theme and model space configuration into all open tabs
    /// and the application default background settings.
    pub(in crate::app) fn sync_model_space_theme(&mut self, geometry_changed: bool) {
        // VSEDGESMOOTH: meshes draw their edges again when the angle changes.
        let smooth = self.model_space.edge_smooth.clamp(0.0, 180.0);
        if (crate::entities::mesh::edge_smooth_degrees() - smooth).abs() > f32::EPSILON {
            crate::entities::mesh::set_edge_smooth_degrees(smooth);
            // Block definitions hold meshes too, so every mesh is built again.
            for tab in &mut self.tabs {
                tab.scene.populate_meshes_from_document();
            }
        }
        let model_bg = self.model_space.resolve_model_bg(&self.active_theme);
        let paper_bg = self.model_space.resolve_paper_bg();
        let desk_bg = self.model_space.resolve_desk_bg();
        let sel_color = self.model_space.resolve_selection_color();
        let sel_effect = self.model_space.selection_effect;
        let sel_widen = f32::from(self.model_space.selection_highlight_widen.min(6));
        self.default_bg_color = Some(model_bg);
        self.default_paper_bg_color = Some(paper_bg);

        for tab in &mut self.tabs {
            let old_bg = tab.scene.bg_color;
            let old_paper = tab.scene.paper_bg_color;
            let old_desk = std::mem::replace(&mut tab.scene.desk_bg_color, desk_bg);
            let old_sel_color = tab.scene.selection_color;
            let old_sel_effect = tab.scene.selection_effect;
            tab.scene.bg_color = model_bg;
            tab.scene.paper_bg_color = paper_bg;
            tab.scene.selection_color = sel_color;
            tab.scene.selection_effect = sel_effect;
            let old_sel_widen = std::mem::replace(&mut tab.scene.selection_widen, sel_widen);
            tab.scene.edge_color = self.model_space.edge_color;
            tab.scene.occluded_edges = self.model_space.occluded_edges;
            tab.bg_color = Some(model_bg);
            tab.paper_bg_color = Some(paper_bg);

            if old_sel_color != sel_color
                || old_sel_effect != sel_effect
                || old_sel_widen != sel_widen
            {
                tab.scene.selection_generation = tab.scene.selection_generation.wrapping_add(1);
            }

            if geometry_changed
                || old_bg != model_bg
                || old_paper != paper_bg
                || old_desk != desk_bg
            {
                tab.scene.recolor_meshes();
                tab.scene.bump_geometry();
            }
            tab.scene
                .request_refresh(crate::scene::ViewportRefreshScope::All);
        }
    }

    /// Record that the one-time default-association prompt has been answered and
    /// flush it to disk, so the dialog never reappears on later launches.
    pub(in crate::app) fn mark_assoc_prompted(&mut self) {
        self.default_assoc_prompted = true;
        self.persist_settings_if_changed();
    }

}
