use super::*;
use crate::snap::Snapper;
use crate::ui::{CommandLine, Ribbon, StatusBar};
use iced::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use iced::window;
use iced::{Point, Task, Theme};

impl OpenCADStudio {
    /// Install the Start-page video list, decoding each thumbnail's JPEG into
    /// an image Handle exactly once (a fresh Handle per view frame would
    /// re-upload the texture every frame).
    pub(in crate::app) fn set_videos(&mut self, videos: Vec<crate::videos::VideoEntry>) {
        if videos.is_empty() {
            return;
        }
        self.video_thumbs = videos
            .iter()
            .filter_map(|v| {
                let bytes = v.thumb.clone()?;
                Some((v.id.clone(), iced::widget::image::Handle::from_bytes(bytes)))
            })
            .collect();
        self.videos = videos;
    }

    pub(crate) fn new() -> Self {
        crate::sys::startup_times::mark("App::new: begin");
        let config = config::AppConfig::load();
        crate::sys::startup_times::mark("App::new: config loaded");
        if let Err(error) = crate::i18n::set_language(config.settings.language) {
            eprintln!("Unable to apply saved UI language: {error}");
        }
        // Boot with only the Welcome/Start tab. The user creates drawings
        // explicitly (File → New); we never auto-spawn Drawing1.
        let start_tab = DocumentTab::new_start();
        let mut app = Self {
            control: control::State::new(),
            start: Instant::now(),
            tabs: vec![start_tab],
            active_tab: 0,
            hovered_doc_tab: None,
            doc_tab_menu: false,
            doc_tab_layouts: None,
            doc_tab_hover_gen: 0,
            tab_counter: 0,
            ribbon: Ribbon::with_modules(crate::plugin::all_ribbon_modules()),
            // Populated from the consolidated config after construction
            // (`apply_config`); default empty here.
            recent_files: Vec::new(),
            recent_thumbs: std::collections::HashMap::new(),
            recent_limit: recent::RECENT_DEFAULT,
            recent_limit_input: recent::RECENT_DEFAULT.to_string(),
            recent_view: RecentView::default(),
            command_line: CommandLine::new(),
            patrons: Vec::new(),
            videos: Vec::new(),
            video_thumbs: std::collections::HashMap::new(),
            videos_loading: false,
            discussions: Vec::new(),
            discussions_loading: false,
            collapsed_property_sections: rustc_hash::FxHashSet::default(),
            layer_filters: Vec::new(),
            ribbon_layers_follow_filter: false,
            start_section: StartSection::default(),
            start_action_w: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            history_content: iced::widget::text_editor::Content::new(),
            command_history_resizing: false,
            command_history_drag_last: None,
            command_line_moving: false,
            status_bar: StatusBar::new(),
            cursor_pos: Point::ORIGIN,
            vp_size: (1280.0, 720.0),
            win_size: (1280.0, 720.0),
            snapper: Snapper::default(),
            snap_popup_open: false,
            drafting_settings_state: None,
            drafting_settings_saved: None,
            drafting_settings_close_confirm: false,
            scale_ui: Default::default(),
            polar_popup_open: false,
            polar_custom_input: String::new(),
            statusbar_menu_open: false,
            layout_list_open: false,
            units_popup_open: false,
            isolate_popup_open: false,
            selection_filter_popup_open: false,
            status_menu_tooltip_hidden: false,
            statusbar_config: crate::ui::statusbar::statusbar_config::StatusBarConfig::default(),
            annotation_auto_scale: -4,
            last_saved_config: None,
            otrack_active: None,
            otrack_cross: None,
            otrack_kind: None,
            clean_screen: false,
            clean_screen_properties: false,
            clean_screen_parameters_manager: false,
            clean_screen_authoring_palettes: false,
            clean_screen_tool_palettes: false,
            block_picker_dismissed: false,
            quick_properties: false,
            quick_properties_anchor: Point::new(12.0, 12.0),
            quick_properties_float: false,
            quick_properties_pos: Point::new(40.0, 40.0),
            selection_cycling: 0,
            cycle_badge: None,
            osnap_z: false,
            select_similar_mode: crate::scene::SELECT_SIMILAR_DEFAULT,
            pick_add: true,
            pick_add_retains: true,
            select_remove_mode: false,
            selection_reprompt: false,
            last_layer_translation: None,
            layer_translator: None,
            drawing_units: None,
            block_definition: None,
            block_definition_open_editor: None,
            block_edit_picker: None,
            rename_dialog: None,
            design_center: None,
            drawing_properties: None,
            geographic_location: None,
            purge_dialog: None,
            purge_only: None,
            select_similar_settings: None,
            block_save_as: None,
            block_visibility: None,
            block_lookup: None,
            block_properties_table: None,
            block_visibility_mode: false,
            block_editor_preferences: settings::BlockEditorPreferences::default(),
            block_editor_settings: None,
            visual_styles_crease: None,
            block_cycle_order: None,
            block_dependency_cache: std::cell::RefCell::new(None),
            pdf_attach: None,
            point_cloud_attach: None,
            point_cloud_prompt_read: None,
            graph_file_reader: crate::app::node_graph::files::read_file,
            import_window_read: 0,
            attdef_dialog: None,
            insert_attr_dialog: None,
            attman: None,
            attdef_edit: None,
            field_dialog: None,
            field_formula_parent: None,
            field_number_format: Default::default(),
            point_cloud_color_map: None,
            pc_section: None,
            view_background: None,
            image_adjust: None,
            refedit_dialog: None,
            pdf_password: None,
            plot_preview: None,
            dim_inspect_dialog: None,
            table_cell_borders: None,
            table_cell_contents: None,
            table_cell_block: None,
            table_cell_format: None,
            anno_scale_prompt: None,
            surface_list: None,
            point_tools: Default::default(),
            mtext_dialog: None,
            table_cell_warning: None,
            table_cell_entry: None,
            table_cell_entry_typed: false,
            data_link_cell_target: false,
            pdf_shx_dialog: None,
            underlay_layers: None,
            pdf_import_settings: None,
            pdf_import_file: None,
            import_dialogs: Default::default(),
            options_parent: None,
            options_child: None,
            xref_attach: None,
            wblock: None,
            geometric_tolerance: None,
            pick_drag_mode: 2,
            pick_auto: 5,
            pick_first: true,
            perf_hud: false,
            cycle_candidates: None,
            pre_cmd_tangent: None,
            add_selected_restore: None,
            rect_suppressed_ortho: false,
            ortho_mode: false,
            polar_mode: false,
            polar_increment_deg: 45.0,
            zoom_wheel_reversed: false,
            navigation_style: Default::default(),
            nav_held: Default::default(),
            nav_pending: None,
            nav_claimed: Default::default(),
            nav_drag_by_button: false,
            nav_middle_press: None,
            nav_right_press: None,
            nav_zoom_latched: false,
            nav_replay: false,
            alt_down: false,
            dynamic_ucs: true,
            dynamic_ucs_restore: None,
            // A Mac's built-in trackpad is the common case there.
            touchpad_pan: cfg!(target_os = "macos"),
            touch: Default::default(),
            touch_offset: None,
            ui_scale_percent: 100,
            zoom_factor: 60,
            zoom_factor_input: 60.to_string(),
            cursor_size: 5,
            pick_box: 5,
            autosnap_magnet: true,
            hatch_associative: true,
            hatch_drafts: Default::default(),
            modemacro: String::new(),
            context_menu_custom: Default::default(),
            ribbon_customize: None,
            context_menu_editing: None,
            hatch_gap_tolerance: 0.0,
            hatch_prefs: Default::default(),
            hatch_boundary_warning_off: false,
            hatch_panel_shown: false,
            outline_switch_hover: false,
            hatch_boundary_warning: None,
            show_menu_bar: false,
            main_maximized: true,
            title_bar_pressed: false,
            ribbon_closed: false,
            command_line_hidden: false,
            double_click_block_refedit: false,
            double_click_block_attedit: true,
            right_click_mode: settings::RightClickMode::ShortcutMenu,
            right_click_hold_ms: 250,
            // Quad cursor menu defaults mirror `UserSettings::default()`; a
            // restored config overwrites these in `apply_settings`.
            quad: false,
            quad_on_hover: true,
            quad_on_select: true,
            quad_on_right_click: true,
            quad_suppress_hover_when_selection: true,
            quad_width: crate::ui::popup::quad_menu::QUAD_WIDTH_DEFAULT,
            quad_command_launch: false,
            quad_hover_delay_ms: 300,
            quad_panels: Vec::new(),
            quad_hover_since: None,
            quad_dwell_pick: None,
            hover_move_seq: 0,
            quad_mru: crate::ui::popup::quad_menu::QuadMru::new(),
            quad_open_target: None,
            grip_object_limit: settings::DEFAULT_GRIP_OBJECT_LIMIT,
            ncopy_bind: false,
            cursor_type: settings::CursorType::Crosshair,
            crosshair_color: None,
            crosshair_color_input: String::new(),
            snap_marker_color: None,
            snap_marker_color_input: String::new(),
            command_text_color: None,
            command_text_color_input: String::new(),
            isolines_awaiting_regen: false,
            snap_angle_input: "0".to_string(),
            lineweight_display_scale: 100,
            isometric_drafting: false,
            iso_plane: settings::IsoPlane::Left,
            snap_angle_deg: 0.0,
            show_grid: false,
            grid_spacing_x: 10.0,
            grid_spacing_y: 10.0,
            grid_major_every: 5,
            grid_adaptive: true,
            grid_beyond_limits: true,
            grid_axes: true,
            dyn_input: true,
            dyn_mode: 3,
            dyn_pointer_cartesian: false,
            dyn_pointer_absolute: false,
            dyn_pointer_on_type: false,
            options_tab: crate::ui::window::options::OptionsTab::General,
            #[cfg(not(target_arch = "wasm32"))]
            graphics_prefs: crate::gpu_backend::load_prefs(),
            options_saved: None,
            options_close_confirm: false,
            spacemouse: {
                let service = crate::input::spacemouse::Service::default();
                service.set_actions(navigation::actions());
                service
            },
            spacemouse_ui: Default::default(),
            texteditmode: false,
            quick_dimension_snap_priority: 0,
            dimension_continue_mode: 1,
            backup_on_save: true,
            file_assoc_enabled: true,
            show_constraint_values: true,
            auto_constrain_settings: settings::AutoConstrainSettings::default(),
            txt2mtxt_settings: settings::Txt2MtxtSettings::default(),
            spell_prefs: settings::SpellPrefs::default(),
            spell: None,
            dim_inspect_settings: settings::DimInspectSettings::default(),
            txt2mtxt_dialog: None,
            matchprop_dialog: None,
            auto_constrain_saved: None,
            auto_constrain_selected_row: 0,
            auto_constrain_distance_input: "0.05".to_string(),
            auto_constrain_angle_input: "1".to_string(),
            constraint_solve_mode: true,
            constraint_infer: false,
            result_depth: 0,
            constraint_bar_display: 3,
            constraint_form_annotational: false,
            dim_constraint_last: "Aligned",
            geom_constraint_last: "Coincident",
            suppress_plugin_dispatch: false,
            plugin_request_active: false,
            constraint_bar_mode: 4095,
            savetime_min: 10,
            save_file_path: String::new(),
            script_commands: true,
            script_command_rejected: None,
            default_bg_color: None,
            default_paper_bg_color: None,
            cliprompt_lines: 3,
            commandline_fade_ms: 3000,
            block_mru: Vec::new(),
            block_freq: std::collections::HashMap::new(),
            block_mru_list: 50,
            selection_report: None,
            last_pick: None,
            selection_undo: (0, Vec::new()),
            selection_undoing: false,
            selection_group_pending: false,
            selection_named_set_pending: false,
            selection_group_named: false,
            last_pick_groups: 0,
            last_entered_point: None,
            selection_subobject: None,
            typed_window_corner: None,
            refedit_options: Default::default(),
            block_redefine_mode: 1,
            block_navigate: ".".to_string(),
            #[cfg(not(target_arch = "wasm32"))]
            block_usage_last_persist: None,
            awaiting_vports: false,
            vpmax_state: None,
            pending_setvar: None,
            step_schema: crate::io::step::Schema::Ap214,
            delete_objects: 1,
            ucs_icon_hover: false,
            ucs_icon_selected: false,
            ucs_grip_drag: None,
            pane_move_from: None,
            dyn_user_reshaped: false,
            dyn_coord_absolute: false,
            dyn_coord_wcs: false,
            grip_hover: None,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_hover: None,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_deliveries: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_live_originals: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_descriptor_drag: None,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_settle_watchdog: crate::app::update::SETTLE_WATCHDOG,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_settle_strikes: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            pending_plugin_grips_rearm: Default::default(),
            grip_popup: None,
            grip_pending: None,
            visibility_popup: None,
            table_popup: None,
            grip_add_provisional: None,
            grip_ctrl_cycle: None,
            grip_preview_handles: Vec::new(),
            hover_dwell: None,
            hover_pick_due: iced::time::Instant::now(),
            move_owed: None,
            box_preview_pending: None,
            properties_refresh_pending: None,
            box_preview_due: None,
            box_preview_cache: None,
            move_pass_due: None,
            move_withheld: 0,
            snap_dwell: None,
            constraint_glyph_tooltip: None,
            grip_originals: Vec::new(),
            grip_history_originals: Vec::new(),
            grip_dirty_before: None,
            grip_engage_press: None,
            grip_point_override: None,
            grip_reference_wires: Vec::new(),
            grip_text_verts: Vec::new(),
            grip_text_slide: false,
            qselect: None,
            filter_dialog: None,
            restoring_layer_previous: false,
            layer_import: None,
            transparent_select: None,
            qselect_settings: None,
            show_ucs_icon: true,
            show_viewport_controls: true,
            ucs_icon_at_origin: true,
            show_viewcube: true,
            navvcube_opacity: 50,
            render_bar_w: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            render_mode_menu_open: false,
            render_mode_preview: None,
            show_properties: true,
            show_layers: false,
            show_graphic_attributes: false,
            show_command_history: false,
            points_menu: Default::default(),
            graphic_attributes: Default::default(),
            show_block_palette: false,
            show_tool_palettes: false,
            tool_palettes: Default::default(),
            show_authoring_palettes: false,
            show_parameters_manager: false,
            authoring_palette_tab: Default::default(),
            show_node_graph: false,
            node_graph_links: Default::default(),
            property_target_override: None,
            graph_undo_open: false,
            show_external_references: false,
            show_browser: false,
            pc_manager: Default::default(),
            count_palette: Default::default(),
            geometry_cleanup: Default::default(),
            bricscad_visibility: Default::default(),
            files_palette: Default::default(),
            quick_calc: Default::default(),
            assistant: Default::default(),
            sheet_set: Default::default(),
            sheet_set_pending_layout: None,
            sheet_set_import_parent: None,
            block_palette: Default::default(),
            xref_manager: Default::default(),
            dock: Default::default(),
            dock_peek: None,
            dock_hover_pending: None,
            dock_hover_gen: 0,
            dock_float_menu: None,
            tool_windows: Vec::new(),
            tool_focus: None,
            keyboard_tool: None,
            #[cfg(not(target_arch = "wasm32"))]
            tool_window_to_raise: None,
            tool_drag: None,
            #[cfg(not(target_arch = "wasm32"))]
            os_windows: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            focused_window: None,
            dock_float_pointer: iced::Point::ORIGIN,
            dock_float_menu_at: iced::Point::ORIGIN,
            dock_drag: None,
            dock_drag_last: None,
            calendar_dismissed: None,
            dock_fold_on_release: None,
            dock_drag_window: None,
            dock_title_hover: None,
            dock_edge_menu: None,
            dock_edge_menu_y: None,
            dock_strip_pointer_y: 0.0,
            dock_grip_hover: None,
            dock_grip_lit: None,
            dock_grip_gen: 0,
            dock_icon_hover: None,
            xref_col_drag: None,
            xref_col_last: None,
            xref_split_drag: false,
            show_file_tabs: true,
            file_tabs_above_docks: false,
            show_layout_tabs: true,
            last_point: None,
            vp_snap_frame: None,
            accepted_snaps: Vec::new(),
            pending_click_snap: None,
            main_window: None,
            thumbnail_capture_clean: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending_native_thumbnail_save: None,
            #[cfg(target_arch = "wasm32")]
            pending_web_thumbnail_save: None,
            #[cfg(target_arch = "wasm32")]
            save_plain_reuse_fs_handle: false,
            #[cfg(target_arch = "wasm32")]
            web_autosave: Default::default(),
            #[cfg(target_arch = "wasm32")]
            web_session_focus_pending: false,
            #[cfg(target_arch = "wasm32")]
            web_session: None,
            color_picker: None,
            color_books: Default::default(),
            color_books_read: false,
            active_modal: None,
            plugin_dialog_state: None,
            plugin_panels: std::collections::BTreeMap::new(),
            panel_surfaces:
                crate::app::panel_surfaces::PanelSurfaceStore::default(),
            panel_event_router:
                crate::app::panel_events::PanelEventRouter::new(),
            panel_widget_ui: std::collections::BTreeMap::new(),
            panel_table_ui: std::collections::BTreeMap::new(),
            plugin_panel_ids: std::collections::HashMap::new(),
            plugin_panel_by_num: std::collections::HashMap::new(),
            hyperlink_editor_handles: Vec::new(),
            hyperlink_editor_block: None,
            hyperlink_editor_url: String::new(),
            hyperlink_editor_description: String::new(),
            hyperlink_editor_mixed: false,
            hyperlink_editor_dirty: false,
            hyperlink_options: Default::default(),
            pending_startup_modals: std::collections::VecDeque::new(),
            gpu_status: crate::scene::pipeline::GpuStatus::Unknown,
            gpu_status_generation: 0,
            gpu_warning_silenced: String::new(),
            plotstyle_parent_plot_geometry: None,
            find_replace: FindReplaceState::default(),
            mtext_find_prefs: None,
            overkill_dialog: None,
            aec_drop_acknowledged: false,
            aec_drop_count: 0,
            conversion_losses: Vec::new(),
            pending_conversion_save: None,
            pending_ifccad_alpha_save: None,
            conversion_loss_allowed: false,
            layer_delete_pending: None,
            modal_offset: iced::Vector::ZERO,
            modal_drag_last: None,
            modal_dragging: false,
            layer_name_col_w: 130.0,
            layer_col_w: std::collections::HashMap::new(),
            layer_walk_col_w: crate::ui::window::layer_walk::DEFAULT_COL_W,
            modal_resize: iced::Vector::ZERO,
            modal_content_size: None,
            modal_resizing: false,
            attr_editor_ui: Default::default(),
            disabled_plugins: rustc_hash::FxHashSet::default(),
            #[cfg(not(target_arch = "wasm32"))]
            plugin_tools: rustc_hash::FxHashMap::default(),
            #[cfg(not(target_arch = "wasm32"))]
            last_plugin_selection: None,
            #[cfg(not(target_arch = "wasm32"))]
            selection_revision_by_tab: rustc_hash::FxHashMap::default(),
            #[cfg(not(target_arch = "wasm32"))]
            last_plugin_document: None,
            external_plugins: Vec::new(),
            loaded_plugin_ids: rustc_hash::FxHashSet::default(),
            plugin_load_errors: rustc_hash::FxHashMap::default(),
            pending_plugin_approvals: Vec::new(),
            plugin_registry: Vec::new(),
            plugin_registry_loading: false,
            plugin_registry_error: None,
            plugin_registry_error_details_open: false,
            plugin_repos: Vec::new(),
            plugin_repo_input: String::new(),
            plugin_search_input: String::new(),
            repo_release_tags: rustc_hash::FxHashMap::default(),
            repo_selected_tag: rustc_hash::FxHashMap::default(),
            selected_plugin_repo: None,
            plugin_readmes: rustc_hash::FxHashMap::default(),
            plugin_readme_loading: rustc_hash::FxHashSet::default(),
            plugin_releases_loading: rustc_hash::FxHashSet::default(),
            plugin_installing: rustc_hash::FxHashSet::default(),
            ole_edits: Vec::new(),
            plugin_installed_now: rustc_hash::FxHashMap::default(),
            marketplace_status: String::new(),
            point_size_buf: String::new(),
            point_style_tab_id: None,
            point_mode_buf: None,
            point_size_relative: true,
            default_assoc_prompted: false,
            check_missing_fonts: true,
            font_source_url: String::new(),
            qnew_template: String::new(),
            transmittal_layout: crate::io::transmit::TransmittalLayout::Structured,
            start_templates: None,
            font_source_input: String::new(),
            donation_prompt_version: String::new(),
            read_only: false,
            update_notice_version: None,
            update_notice_body: None,
            clipboard: Vec::new(),
            oops_cache: Vec::new(),
            clipboard_base: glam::DVec3::ZERO,
            clipboard_deps: ClipboardDeps::default(),
            clipboard_source_tab: None,
            shift_down: false,
            ctrl_down: false,
            cont_anchor: None,
            mtext_editor: None,
            table_cell_editor: None,
            table_edit_command_line: false,
            command_mtext_input: false,
            pending_command_editor_text: None,
            text_inline: None,
            snap_override_popup: None,
            held_override: None,
            axis_lock_dir: None,
            angle_override: None,
            drag_move: None,
            layout_rename_state: None,
            last_vp_click_time: None,
            last_vp_click_pos: None,
            mtext_click_time: None,
            mtext_click_off: 0,
            mtext_click_count: 0,
            mtext_clipboard: None,
            plot_window: None,
            plot_paper: crate::io::paper_catalog::default_paper().clone(),
            plot_orientation: crate::io::paper_catalog::Orientation::Landscape,
            plot_dialog: crate::ui::window::plot::PlotDialogState::default(),
            plot_prev: None,
            plot_prev_context: None,
            plot_reconcile_printer_media: None,
            plot_setup_template: None,
            print_all_layouts: Vec::new(),
            print_all_options: false,
            print_all_settings_override: false,
            print_all_options_prev: None,
            print_all_plot_style_prev: None,
            print_all_plot_window_prev: None,
            print_all_plot_setup_prev: None,
            opening: None,
            layout_settling: false,
            plugin_frame: None,
            plugin_frame_drawn: false,
            plugin_frame_pass: false,
            plugin_run: None,
            plugin_serving: false,
            open_job_serial: 0,
            recovery_report: None,
            missing_fonts: None,
            missing_fonts_path: None,
            missing_fonts_downloading: false,
            missing_fonts_silence: false,
            suppressed_missing_fonts: rustc_hash::FxHashSet::default(),
            pending_opens: std::collections::VecDeque::new(),
            pending_startup_script_lines: Vec::new(),
            script_queue: std::collections::VecDeque::new(),
            script_running: false,
            script_paused: false,
            sdi: false,
            exit_requested: false,
            #[cfg(not(target_arch = "wasm32"))]
            terminating: false,
            active_interaction_index: None,
            queued_interaction_indices: std::collections::VecDeque::new(),
            pending_close: None,
            pending_tab_closes: std::collections::VecDeque::new(),
            pending_block_close: None,
            #[cfg(not(target_arch = "wasm32"))]
            active_save_jobs: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            autosave_jobs: std::collections::HashSet::new(),
            #[cfg(not(target_arch = "wasm32"))]
            saves_after_autosave: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            sheet_publish: None,
            #[cfg(not(target_arch = "wasm32"))]
            save_job_serial: 0,
            #[cfg(not(target_arch = "wasm32"))]
            pending_save_leases: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            pending_save_failure: None,
            extraction_outdated: None,
            extraction_checked: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending_external_change: None,
            save_dialog_format: crate::io::DEFAULT_SAVE_FORMAT.to_string(),
            save_dialog_filename: "drawing.dwg".to_string(),
            save_dialog_for_unsaved: false,
            default_save_format: crate::io::DEFAULT_SAVE_FORMAT.to_string(),
            language: crate::i18n::Language::default(),
            // Plot style
            active_plot_style: crate::io::plot_style::PlotStyleTable::load_named(
                crate::io::plot_style::DEFAULT_PLOT_STYLE,
            )
            .or_else(|_| {
                crate::io::plot_style::PlotStyleTable::builtin(
                    crate::io::plot_style::DEFAULT_PLOT_STYLE,
                )
            })
            .ok(),
            plotstyle_panel_baseline: None,
            // Color scheme (default: Oxocarbon)
            active_theme: Theme::Oxocarbon,
            ui_theme: config::UiThemeConfig::default(),
            theme_color_inputs: config::UiThemePalette::default().hex_values(),
            model_space: config::ModelSpaceThemeConfig::default(),
            model_bg_input: String::new(),
            paper_bg_input: String::new(),
            desk_bg_input: String::new(),
            saved_custom_palette: None,
            #[cfg(not(target_arch = "wasm32"))]
            caught_panics: 0,
            failed_views: Default::default(),
            // Keyboard shortcuts
            shortcut_bindings: rustc_hash::FxHashMap::default(),
            shortcut_editor_rows: Vec::new(),
            shortcut_filter: String::new(),
            shortcut_capture_row: None,
            shortcut_pending_add: false,
            shortcut_suggest: None,
            shortcut_reset_confirm: false,
            shortcut_close_confirm: false,
            // Command aliases (populated from ocad.pgp just after construction)
            command_aliases: rustc_hash::FxHashMap::default(),
            menu_bar_popup: None,
            alias_editor_rows: Vec::new(),
            alias_pending_add: false,
            alias_reset_confirm: false,
            alias_close_confirm: false,
            alias_import_report: String::new(),
            named_parameter_editor_rows: Vec::new(),
            cal_context: calc::CalcContext::default(),
            // Layout Manager
            layout_manager_selected: "Model".to_string(),
            layer_state_selected: None,
            layer_state_name_buf: String::new(),
            layer_state_description_buf: String::new(),
            layer_state_filter: String::new(),
            layer_state_edit_draft: None,
            layer_state_edit_filter: String::new(),
            layer_state_edit_color_open: None,
            anno_object_scale_target: None,
            anno_object_scale_extra: Vec::new(),
            anno_object_scale: Default::default(),
            table_insert: Default::default(),
            data_link_manager: Default::default(),
            data_link_parent_table: false,
            tablestyle_parent_table: false,
            tablestyle_in_use_only: false,
            data_extraction: Default::default(),
            layout_manager_rename_buf: String::new(),
            plotstyle_panel_aci: 1,
            ps_color_buf: String::new(),
            ps_lineweight_buf: "255".to_string(),
            ps_screening_buf: "100".to_string(),
            // TextStyle font browser
            style_rename: None,
            style_rename_buf: String::new(),
            style_rename_kind: None,
            style_stage: None,
            style_parent_stage: None,
            style_import: None,
            textstyle_selected: "Standard".to_string(),
            textstyle_counts: Default::default(),
            textstyle_in_use_only: false,
            textstyle_sample: String::new(),
            textstyle_compare: String::new(),
            textstyle_draft: Default::default(),
            textstyle_card: None,
            textstyle_context: None,
            textstyle_font_picker: Default::default(),
            textstyle_bigfont_picker: Default::default(),
            textstyle_users: Default::default(),
            // TableStyle dialog
            tablestyle_selected: "Standard".to_string(),
            tablestyle_tab: 0,
            tablestyle_compare: String::new(),
            table_style_ui: Default::default(),
            // MLineStyle dialog
            linetype_selected: String::new(),
            linetype_tab: 0,
            linetype_in_use_only: false,
            linetype_keys_active: true,
            linetype_list_view: None,
            linetype_counts: Default::default(),
            linetype_draft: Default::default(),
            linetype_library_status: String::new(),
            mlstyle_selected: "Standard".to_string(),
            mlstyle_tab: 0,
            mlstyle_compare: String::new(),
            mln_description: String::new(),
            mln_start_angle: "90".to_string(),
            mln_end_angle: "90".to_string(),
            mln_fill_color: "256".to_string(),
            mln_elements: Vec::new(),
            // MLeaderStyle dialog
            mleaderstyle_selected: "Standard".to_string(),
            mls: Default::default(),
            mls_dialog: Default::default(),
            // DimStyle dialog
            dimstyle_selected: "Standard".to_string(),
            dsd: Default::default(),
            viewport_switch_keeps_command: false,
            chspace_pending: None,
            alignspace_pending: None,
        };
        crate::sys::startup_times::mark("App::new: struct built");
        // Restore the consolidated user config (settings.json) into live state
        // so preferences, recents, status-bar layout, ribbon density and print
        // options survive across sessions (issue #68). `last_saved_config` is
        // seeded below so the first change — not the boot — triggers a write.
        app.apply_config(config);
        // MCP control starts as the button was left (#3126); `--http` needs it.
        #[cfg(not(target_arch = "wasm32"))]
        let forced = crate::app::control::http_bridge::gui_http_requested();
        #[cfg(target_arch = "wasm32")]
        let forced = false;
        app.control.enabled = app.control.start_enabled || forced;
        crate::sys::startup_times::mark("App::new: apply_config done");
        // Load command aliases from ocad.pgp (writes the shipped default file
        // on first launch). The hide-set keeps aliases out of autocomplete while
        // their target command still shows.
        app.command_aliases = alias::load_aliases();
        app.command_line.command_aliases = app.command_aliases.clone();
        crate::sys::startup_times::mark("App::new: aliases loaded");
        // Load external plugin packages from the plugins folder once, then fold
        // their ribbon tabs into the ribbon. Skipped under test/wasm.
        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        {
            crate::sys::startup_times::mark("App::new: loading external plugins");
            for (id, res) in crate::plugin::external::load_at_startup(&mut app) {
                if let Err(e) = res {
                    app.command_line
                        .push_error(crate::tf!("Plugin '{id}' failed to load: {e}").as_ref());
                    app.plugin_load_errors.insert(id, e);
                }
            }
            crate::sys::startup_times::mark("App::new: external plugins loaded");
            app.loaded_plugin_ids = crate::plugin::external::loaded_ids().into_iter().collect();
            app.external_plugins = crate::plugin::external::discover();
            app.pending_plugin_approvals =
                crate::plugin::external::collect_pending_approvals(&app.external_plugins);
            if !app.pending_plugin_approvals.is_empty() {
                app.pending_startup_modals
                    .push_back(ModalKind::PluginConsent);
            }
            app.rebuild_ribbon_modules();
        }
        crate::sys::startup_times::mark("App::new: done");
        app.last_saved_config = Some(app.current_config());
        app.sync_ribbon_layers();
        app
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        let mut app = Self::new();
        // `new` loads the real settings file, so without this every test runs
        // against whatever the developer last set in the application — a suite
        // that passes on a clean machine and fails on a used one. It surfaced
        // when a persisted `GRIPOBJLIMIT` made the grip-limit test see 32767
        // where it expected the default, and the number of persisted settings
        // only grows.
        app.apply_config(crate::app::config::AppConfig::default());
        app.last_saved_config = Some(app.current_config());
        app
    }

    /// Test hook for transports outside `crate::app` (the REST routing
    /// tests): push a fresh tab and return its document id, so a second
    /// open document can be addressed without reaching into `tabs`.
    #[cfg(test)]
    pub(crate) fn push_test_document(&mut self) -> u64 {
        let tab = tab_document::DocumentTab::new_drawing(1000 + self.tabs.len());
        let id = tab.id;
        self.tabs.push(tab);
        id
    }

    /// Open a declarative plugin modal dialog on the currently active tab.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn open_plugin_dialog(&mut self, def: ocs_plugin_api::dialog::DialogDef) {
        let tab = self.active_tab;
        self.open_plugin_dialog_on_tab(tab, def, None);
    }

    /// Open a declarative plugin modal dialog associated with `tab` and an optional `explicit_plugin_id`.
    pub(crate) fn open_plugin_dialog_on_tab(
        &mut self,
        tab: usize,
        def: ocs_plugin_api::dialog::DialogDef,
        explicit_plugin_id: Option<String>,
    ) {
        let (tab_id, plugin_id, command_id) = if let Some(t) = self.tabs.get(tab) {
            let pid = explicit_plugin_id.or_else(|| {
                t.active_cmd
                    .as_ref()
                    .and_then(|c| c.owner_plugin_id().map(str::to_string))
            });
            let cid = t.active_cmd.as_ref().and_then(|c| c.plugin_command_id());
            (Some(t.id), pid, cid)
        } else {
            (None, explicit_plugin_id, None)
        };
        self.plugin_dialog_state = Some(
            crate::ui::window::plugin_dialog::PluginDialogState::new(def)
                .with_owner(tab_id, plugin_id, command_id),
        );
        self.active_modal = Some(ModalKind::PluginDialog);
    }

    /// Resolve the tab index that owns the currently open plugin dialog, falling back to `active_tab`.
    pub(crate) fn dialog_target_tab_index(&self) -> usize {
        if let Some(tid) = self.plugin_dialog_state.as_ref().and_then(|s| s.tab_id) {
            if let Some(idx) = self.tabs.iter().position(|t| t.id == tid) {
                return idx;
            }
        }
        self.active_tab
    }

    /// Apply queued `(panel_or_dialog_id, values)` updates from `take_pending_panel_updates`
    /// to matching registered panels or the currently open modal dialog.
    pub(crate) fn apply_panel_or_dialog_updates(
        &mut self,
        updates: Vec<(
            String,
            std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>,
        )>,
    ) {
        for (target_id, values) in updates {
            if let Some(panel) = self.plugin_panels.get_mut(&target_id) {
                panel.update_values(values);
                continue;
            }
            let lower = target_id.to_lowercase();
            if let Some((_, panel)) = self
                .plugin_panels
                .iter_mut()
                .find(|(k, _)| k.to_lowercase() == lower)
            {
                panel.update_values(values);
                continue;
            }
            if let Some(state) = &mut self.plugin_dialog_state {
                if target_id.is_empty()
                    || state.def.title == target_id
                    || state.def.title.eq_ignore_ascii_case(&target_id)
                {
                    state.update_values(values);
                }
            }
        }
    }

    /// Update control values in an active modal dialog in-place.
    #[allow(dead_code)]
    pub fn update_dialog(
        &mut self,
        values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>,
    ) -> bool {
        if let Some(state) = &mut self.plugin_dialog_state {
            state.update_values(values);
            true
        } else {
            false
        }
    }

    /// Whether an Escape belongs to a date field's calendar that has just
    /// closed on it: the calendar closes on the key but passes it on.
    pub(crate) fn escape_closed_a_calendar(&mut self) -> bool {
        self.calendar_dismissed
            .take()
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(300))
    }

    /// Dispatch a reactive action event from an open plugin modal dialog to the owning command/plugin.
    pub(crate) fn dispatch_plugin_dialog_action(
        &mut self,
        control_id: String,
        action: String,
        value: Option<ocs_plugin_api::dialog::DialogValue>,
    ) -> Task<Message> {
        let tab_idx = self.dialog_target_tab_index();
        let (action_resp, panel_updates, fallback_notify) =
            if let Some(state) = &self.plugin_dialog_state {
                let current_values = state.collect_response_map();
                let dialog_title = state.def.title.clone();
                let owner_plugin_id = state.plugin_id.clone();
                let owner_command_id = state.command_id;
                let event = ocs_plugin_api::dialog::DialogEvent {
                    control_id: control_id.clone(),
                    action: action.clone(),
                    value: value.clone(),
                    current_values: current_values.clone(),
                };
                if let Some(cmd) = self
                    .tabs
                    .get_mut(tab_idx)
                    .and_then(|t| t.active_cmd.as_mut())
                {
                    let resp = cmd.on_dialog_action(event);
                    let updates = cmd.take_pending_panel_updates();
                    (
                        Some(resp),
                        updates,
                        Some((
                            dialog_title,
                            owner_plugin_id,
                            owner_command_id,
                            current_values,
                        )),
                    )
                } else {
                    (
                        None,
                        Vec::new(),
                        Some((
                            dialog_title,
                            owner_plugin_id,
                            owner_command_id,
                            current_values,
                        )),
                    )
                }
            } else {
                (None, Vec::new(), None)
            };

        self.apply_panel_or_dialog_updates(panel_updates);

        match action_resp {
            Some(ocs_plugin_api::dialog::DialogActionResponse::UpdateValues(values)) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    state.update_values(values);
                }
            }
            Some(ocs_plugin_api::dialog::DialogActionResponse::ReplaceDialog(new_def)) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    let tab_id = state.tab_id;
                    let plugin_id = state.plugin_id.clone();
                    let command_id = state.command_id;
                    *state = crate::ui::window::plugin_dialog::PluginDialogState::new(*new_def)
                        .with_owner(tab_id, plugin_id, command_id);
                }
            }
            Some(ocs_plugin_api::dialog::DialogActionResponse::Submit) => {
                return self.on_plugin_dialog_submit();
            }
            Some(ocs_plugin_api::dialog::DialogActionResponse::Close) => {
                return self.on_plugin_dialog_cancel();
            }
            Some(ocs_plugin_api::dialog::DialogActionResponse::None) | None => {
                if let Some((dialog_title, owner_plugin_id, owner_command_id, mut current_vals)) =
                    fallback_notify
                {
                    if let Some(v) = value {
                        current_vals.insert("__event_value__".to_string(), v);
                    }
                    crate::plugin::v4_support::publish_panel_action_to_plugin(
                        owner_plugin_id.as_deref(),
                        owner_command_id,
                        dialog_title,
                        control_id,
                        action,
                        Some(ocs_plugin_api::dialog::DialogValue::Fields(current_vals)),
                    );
                }
            }
        }
        Task::none()
    }

    pub(crate) fn on_plugin_dialog_submit(&mut self) -> Task<Message> {
        let tab_idx = self.dialog_target_tab_index();
        let owner_plugin_id = self
            .plugin_dialog_state
            .as_ref()
            .and_then(|s| s.plugin_id.clone());
        let response = if let Some(state) = &self.plugin_dialog_state {
            state.collect_response()
        } else {
            ocs_plugin_api::dialog::DialogResponse::Cancelled
        };
        self.close_active_modal();

        let (cmd_result, next_dialog, panel_updates) = if let Some(cmd) = self
            .tabs
            .get_mut(tab_idx)
            .and_then(|t| t.active_cmd.as_mut())
        {
            let res = cmd.on_dialog_response(response);
            let next = if matches!(
                res,
                crate::command::CmdResult::CommitAndExit(_)
                    | crate::command::CmdResult::CommitEntitiesAndExit(_)
                    | crate::command::CmdResult::Cancel
            ) {
                None
            } else {
                cmd.modal_dialog()
            };
            let updates = cmd.take_pending_panel_updates();
            (Some(res), next, updates)
        } else {
            (None, None, Vec::new())
        };

        self.apply_panel_or_dialog_updates(panel_updates);

        if let Some(def) = next_dialog {
            self.open_plugin_dialog_on_tab(tab_idx, def, owner_plugin_id);
        }

        if let Some(res) = cmd_result {
            self.apply_cmd_result(res)
        } else {
            Task::none()
        }
    }

    pub(crate) fn on_plugin_dialog_cancel(&mut self) -> Task<Message> {
        let tab_idx = self.dialog_target_tab_index();
        let owner_plugin_id = self
            .plugin_dialog_state
            .as_ref()
            .and_then(|s| s.plugin_id.clone());
        self.close_active_modal();

        let (cmd_result, next_dialog, panel_updates) = if let Some(cmd) = self
            .tabs
            .get_mut(tab_idx)
            .and_then(|t| t.active_cmd.as_mut())
        {
            let res = cmd.on_dialog_response(ocs_plugin_api::dialog::DialogResponse::Cancelled);
            let next = if matches!(
                res,
                crate::command::CmdResult::CommitAndExit(_)
                    | crate::command::CmdResult::CommitEntitiesAndExit(_)
                    | crate::command::CmdResult::Cancel
            ) {
                None
            } else {
                cmd.modal_dialog()
            };
            let updates = cmd.take_pending_panel_updates();
            (Some(res), next, updates)
        } else {
            (None, None, Vec::new())
        };

        self.apply_panel_or_dialog_updates(panel_updates);

        if let Some(def) = next_dialog {
            self.open_plugin_dialog_on_tab(tab_idx, def, owner_plugin_id);
        }

        if let Some(res) = cmd_result {
            self.apply_cmd_result(res)
        } else {
            Task::none()
        }
    }

    /// Install `cmd` as the active interactive command for tab `tab`.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn set_active_command(
        &mut self,
        tab: usize,
        cmd: Box<dyn crate::command::CadCommand>,
    ) {
        let dialog = cmd.modal_dialog();
        let owner_pid = cmd.owner_plugin_id().map(str::to_string);
        let cmd_id = cmd.plugin_command_id();
        let tab_id = self.tabs.get(tab).map(|t| t.id);
        if let Some(t) = self.tabs.get_mut(tab) {
            t.active_cmd = Some(cmd);
        }
        if let Some(dialog_def) = dialog {
            self.open_plugin_dialog_on_tab(tab, dialog_def, owner_pid);
        } else if let Some(state) = &mut self.plugin_dialog_state {
            if state.tab_id.is_none() {
                state.tab_id = tab_id;
            }
            if state.plugin_id.is_none() {
                state.plugin_id = owner_pid;
            }
            if state.command_id.is_none() {
                state.command_id = cmd_id;
            }
        }
    }

    /// Push an error message from the plugin runtime to the command line.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn push_plugin_error(&mut self, msg: &str) {
        self.command_line.push_error(msg);
    }

    /// Whether the app draws the main window's title bar (see
    /// `ui::title_bar::drawn`).
    pub(crate) fn draws_title_bar(&self) -> bool {
        crate::ui::title_bar::drawn()
    }

    /// Boot function for `iced::daemon`: returns initial state plus a task that
    /// opens the primary application window. Native only — the web build uses
    /// [`Self::boot_web`].
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn boot() -> (Self, Task<Message>) {
        use helpers::build_window_icon;
        // File association is no longer re-registered on every launch. It is set
        // up once via the first-launch prompt below (when the user hasn't been
        // asked yet) and afterwards managed entirely by the FILEASSOC command.
        crate::sys::startup_times::mark("boot: begin");
        // The desktop's window buttons (a `gsettings` call on GNOME) are read
        // while the rest starts, not in the first view.
        std::thread::spawn(|| {
            crate::ui::title_bar::layout();
            crate::ui::title_bar::double_click();
        });
        let state = Self::new();
        crate::sys::startup_times::mark("boot: App constructed");
        let (id, open_task) = window::open(window::Settings {
            maximized: true,
            // The app draws the title bar over the ribbon; macOS keeps its
            // frame for the traffic lights, over a content view that runs
            // under the hidden title.
            decorations: !crate::ui::title_bar::frameless(),
            #[cfg(target_os = "macos")]
            platform_specific: window::settings::PlatformSpecific {
                title_hidden: crate::ui::title_bar::drawn(),
                titlebar_transparent: crate::ui::title_bar::drawn(),
                fullsize_content_view: crate::ui::title_bar::drawn(),
            },
            icon: build_window_icon().and_then(|rgba| window::icon::from_rgba(rgba, 32, 32).ok()),
            exit_on_close_request: false,
            // A Wayland compositor has no StartupWMClass to go on: it resolves a
            // window's dock icon by matching the window's app_id against the
            // basename of an installed .desktop file. iced defaults the id to an
            // empty string, so nothing matched and the dock button came up blank
            // even though the AppImage ships both the .desktop and the icon.
            // (X11 uses the .desktop's StartupWMClass and was unaffected.)
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                application_id: crate::io::file_association::APP_ID.to_string(),
                ..Default::default()
            },
            ..Default::default()
        });
        let mut s = state;
        s.main_window = Some(id);
        tool_windows::set_main_window(id);
        let open_main = open_task.map(|_| Message::Noop);
        let check_update = if crate::config::managed() {
            Task::none()
        } else {
            Task::perform(
                crate::io::update_check::check_for_update(),
                Message::UpdateCheckResult,
            )
        };
        let focus_cmd = s.focus_cmd_input();
        // Startup configuration from the command line (see `cli`). File
        // arguments — also how the OS file association launches us when
        // drawings are double-clicked — go through `OpenExternal`, the same
        // door a second launch hands files to: it existence-checks each path,
        // skips one already open, and queues the rest behind the load in
        // flight. Handing them straight to `OpenRecent` would let each
        // overwrite the single `opening` slot and silently drop all but one.
        // `--new` opens a fresh drawing tab instead of the welcome screen.
        // `--read-only` disables saving. `--script` queues command lines.
        let cfg = crate::cli::gui_config();
        s.read_only = cfg.read_only;
        // GPU backend / renderer fallback: the resolver ran before iced
        // booted, so surface its verdict here where the user can see it.
        if let Some(notice) = cfg.gpu_fallback_notice {
            s.command_line.push_warning(&notice);
            crate::scene::pipeline::report_gpu_line(&format!("[gpu] {notice}"));
        }
        if cfg.gpu_compat_auto {
            let notice = crate::gpu_backend::compat_notice();
            s.command_line.push_warning(&notice);
            crate::scene::pipeline::report_gpu_line(&format!("[gpu] {notice}"));
        }
        let has_files = !cfg.files.is_empty();
        let cli_open: Task<Message> = if has_files {
            Task::batch(
                cfg.files
                    .into_iter()
                    .map(|p| Task::done(Message::OpenExternal(p))),
            )
        } else if cfg.new || !cfg.script_lines.is_empty() {
            Task::done(Message::TabNew)
        } else {
            Task::none()
        };
        // Startup command script: if files were passed to open, defer the
        // script lines until the document finishes opening. Otherwise, dispatch
        // them on the newly created drawing tab.
        let has_script = !cfg.script_lines.is_empty();
        let script: Task<Message> = if cfg.script_lines.is_empty() {
            Task::none()
        } else if has_files {
            s.pending_startup_script_lines = cfg.script_lines;
            Task::none()
        } else {
            s.script_queue.extend(cfg.script_lines);
            Task::done(Message::ScriptPump)
        };
        // Unsaved drawings a crashed session left behind.
        #[cfg(not(target_arch = "wasm32"))]
        s.recover_ended_session_autosaves();
        // An unattended script must not stop at a startup dialog.
        if !has_script {
            s.queue_startup_prompts();
        }
        // Fetch the Patreon supporters list once at boot for the Start page.
        #[cfg(not(target_arch = "wasm32"))]
        let patrons_fetch = Task::perform(
            async { crate::patreon::fetch_patrons() },
            Message::PatronsFetched,
        );
        #[cfg(target_arch = "wasm32")]
        let patrons_fetch = Task::none();
        // Tutorial videos: show the on-disk cache instantly, refresh from the
        // live playlist in the background. Nothing ships in the binary. The
        // fetch runs on its own OS thread — its several sequential HTTP
        // requests would otherwise sit on the async executor and hold up the
        // rest of the boot tasks (the Start page waited on it).
        #[cfg(not(target_arch = "wasm32"))]
        let videos_fetch = {
            s.set_videos(crate::videos::load_cached());
            s.videos_loading = true;
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::videos::fetch_playlist());
            });
            Task::perform(
                async move {
                    rx.await
                        .unwrap_or_else(|_| Err("video fetch thread died".into()))
                },
                Message::VideosFetched,
            )
        };
        #[cfg(target_arch = "wasm32")]
        let videos_fetch = Task::none();
        // GitHub Discussions: seed from the last successful fetch, then refresh
        // the public feed and pinned section on a background thread.
        #[cfg(not(target_arch = "wasm32"))]
        let discussions_fetch = {
            s.discussions = crate::discussions::load_cached();
            s.discussions_loading = true;
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::discussions::fetch_discussions());
            });
            Task::perform(
                async move {
                    rx.await
                        .unwrap_or_else(|_| Err("discussion fetch thread died".into()))
                },
                Message::DiscussionsFetched,
            )
        };
        #[cfg(target_arch = "wasm32")]
        let discussions_fetch = Task::none();
        // Recent-file thumbnails: decoded off-thread — parsing every recent
        // DWG's preview on the boot path held the first frame back.
        let thumbs_fetch = s.refresh_recent_thumbs();
        (
            s,
            Task::batch([
                open_main,
                check_update,
                focus_cmd,
                cli_open,
                script,
                patrons_fetch,
                videos_fetch,
                discussions_fetch,
                thumbs_fetch,
            ]),
        )
    }

    /// Single-window boot for the web build: no OS-window creation (the browser
    /// canvas is the only window), no file-association registration or CLI file
    /// open. Secondary manager windows are unavailable on the web for now.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn boot_web() -> (Self, Task<Message>) {
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut s = Self::new();
        let embed_mode = crate::io::host_bridge::is_embed_mode();
        if embed_mode {
            s.donation_prompt_version = env!("OCS_APP_VERSION").to_string();
            s.control.automation_seen = true;
            s.control.enabled = crate::io::host_bridge::is_automation_allowed();
            crate::io::host_bridge::install();
        }
        // A web host that intends to automate can opt in before the first
        // control request by setting `opencadstudio.automation` to "1" in
        // localStorage (the same pre-load mechanism the settings use): the
        // startup donation prompt then never queues (#1349).
        if !embed_mode
            && web_sys::window()
                .and_then(|window| window.local_storage().ok().flatten())
                .and_then(|storage| storage.get_item("opencadstudio.automation").ok().flatten())
                == Some("1".to_string())
        {
            s.control.automation_seen = true;
        }
        s.queue_startup_prompts();
        let focus = s.focus_cmd_input();
        let primary_font =
            crate::scene::text::web_font::preload_language(&crate::i18n::active_language_tag());
        let fonts = Task::batch([
            Task::done(Message::PollWebFonts),
            Task::done(Message::ApplyWebFont(primary_font)),
        ]);
        // Web can't reach the Patreon API directly (CORS); fetch the CI-built
        // supporters.json served on the same origin instead.
        let patrons = Task::perform(crate::patreon::fetch_patrons_web(), Message::PatronsFetched);
        s.videos_loading = true;
        let videos = Task::perform(crate::videos::fetch_playlist_web(), Message::VideosFetched);
        s.discussions_loading = true;
        let discussions = Task::perform(
            crate::discussions::fetch_discussions_web(),
            Message::DiscussionsFetched,
        );
        let thumbs_fetch = if embed_mode {
            Task::none()
        } else {
            s.refresh_recent_thumbs()
        };
        // Crash-recovery scan: OPFS snapshots newer than a drawing's last
        // saved copy come back as a restore prompt (stale ones are deleted).
        let autosave_scan = if embed_mode {
            Task::none()
        } else {
            Task::perform(
                crate::io::web_autosave::scan(),
                Message::WebAutosavesScanned,
            )
        };
        // URL deep link (`?file=…`): a shareable/embeddable link that opens a
        // drawing at boot. An http(s) URL is fetched with the same capped
        // streaming reader the remote-image fetch uses and handed to the
        // guarded drop-open queue; a bare name reads the OPFS recent copy the
        // same way `open_recent_web` does. Everything else (other schemes,
        // non-.dwg/.dxf URL tails) is refused with a console warning — the
        // same scheme gate the image fetch applies. Pure parsing/validation
        // lives in `io::web_link` and is unit-tested there.
        let deep_link: Task<Message> = {
            let search = if embed_mode {
                String::new()
            } else {
                web_sys::window()
                    .and_then(|window| window.location().search().ok())
                    .unwrap_or_default()
            };
            match crate::io::web_link::parse_link(&search) {
                crate::io::web_link::ParsedLink::Target(target) => {
                    // Opening a URL from the query string is an implicit trust
                    // decision: name the origin before the fetch starts (info
                    // only — the bytes still go through the parse + recovery
                    // machinery like any other open). Same-origin links stay
                    // quiet: that drawing already comes from this site.
                    if let crate::io::web_link::LinkTarget::Url { url, name } = &target {
                        if !crate::io::web_link::same_origin(url) {
                            let origin = crate::io::web_link::display_origin(url)
                                .unwrap_or_else(|| url.clone());
                            s.command_line
                                .push_info(&format!("Opening {name} from {origin}…"));
                        }
                    }
                    Task::perform(crate::io::web_link::open_link(target), |outcome| {
                        Message::WebLinkSettled(outcome)
                    })
                }
                crate::io::web_link::ParsedLink::Ignored(refusal, value) => {
                    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(
                        &crate::io::web_link::refusal_warning(refusal, &value),
                    ));
                    Task::none()
                }
                crate::io::web_link::ParsedLink::Absent => Task::none(),
            }
        };
        (
            s,
            Task::batch([
                focus,
                fonts,
                patrons,
                videos,
                discussions,
                thumbs_fetch,
                autosave_scan,
                deep_link,
            ]),
        )
    }
}
