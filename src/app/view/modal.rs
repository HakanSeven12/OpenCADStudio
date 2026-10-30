use super::super::{Message, OpenCADStudio};
use crate::t;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Element, Fill, Fit, Theme};
use std::borrow::Cow;

#[cfg(test)]
#[path = "conversion_loss_tests.rs"]
mod conversion_loss_tests;

impl OpenCADStudio {
    /// Title shown in the active modal's title bar. Keep in sync with the
    /// [`Self::modal_content`] dispatch.
    pub(super) fn modal_title(&self) -> String {
        use super::super::ModalKind as K;
        match self.active_modal {
            Some(K::About) => crate::tr!("modal", "about"),
            Some(K::Shortcuts) => crate::tr!("modal", "keyboard-shortcuts"),
            Some(K::Aliases) => crate::tr!("modal", "command-aliases"),
            Some(K::NamedParameters) => crate::t!("Named Parameters").into_owned(),
            Some(K::BlockEditorSettings) => crate::t!("Block Editor Settings").into_owned(),
            Some(K::BlockCycleOrder) => crate::t!("Grip Cycle Order").into_owned(),
            Some(K::Options) => crate::tr!("action", "options"),
            Some(K::FindReplace) => crate::tr!("modal", "find-replace"),
            Some(K::Overkill) => crate::t!("Delete Duplicate Objects").into_owned(),
            Some(K::PluginManager) => crate::tr!("modal", "plugin-manager"),
            Some(K::UpdateNotice) => crate::tr!("modal", "update-available"),
            Some(K::DonationPrompt) => crate::tr!("donation", "title"),
            Some(K::LayerStateManager) => crate::tr!("modal", "layer-state-manager"),
            Some(K::LayerTranslator) => crate::t!("Layer Translator").into_owned(),
            Some(K::DrawingUnits) => crate::t!("Drawing Units").into_owned(),
            Some(K::BlockDefinition) => crate::t!("Block Definition").into_owned(),
            Some(K::PdfAttach) => match self.pdf_attach.as_ref().map(|s| s.kind) {
                Some(codec::entities::UnderlayType::Dwf) => {
                    crate::t!("Attach DWF Underlay").into_owned()
                }
                Some(codec::entities::UnderlayType::Dgn) => {
                    crate::t!("Attach DGN Underlay").into_owned()
                }
                _ => crate::t!("Attach PDF Underlay").into_owned(),
            },
            Some(K::PointCloudAttach) => crate::t!("Attach Point Cloud").into_owned(),
            Some(K::PointCloudColorMap) => crate::t!("Point Cloud Color Map").into_owned(),
            Some(K::PcSection) => crate::t!("Extract Section Lines from Point Cloud").into_owned(),
            Some(K::ViewBackground) => format!(
                "{} — {}",
                crate::tr!("view-background", "title"),
                self.view_background
                    .as_ref()
                    .map_or("", |s| s.view.as_str())
            ),
            Some(K::ImageAdjust) => crate::tr!("image-adjust", "title"),
            Some(K::RefEdit) => crate::tr!("ref-edit", "dialog-title"),
            Some(K::PdfPassword) => crate::tr!("pdf-password", "title"),
            Some(K::DimInspect) => crate::tr!("dim-inspect", "title"),
            Some(K::PdfShxSettings) => crate::tr!("pdf-shx", "title"),
            Some(K::UnderlayLayers) => crate::t!("Underlay Layers").into_owned(),
            Some(K::PdfImportSettings) => crate::t!("PDF Import Settings").into_owned(),
            Some(K::Txt2MtxtSettings) => crate::tr!("txt2mtxt", "title"),
            Some(K::Spell) => crate::ui::window::spell_dialog::title(),
            Some(K::PdfImportFile) => crate::t!("Import PDF").into_owned(),
            Some(K::ImportFile) => crate::t!("Import File").into_owned(),
            Some(K::DgnImport) => crate::t!("DGN Import").into_owned(),
            Some(K::TdsImport) => crate::tr!("import-3ds", "title"),
            Some(K::TdsMaterial) => crate::tr!("import-3ds-alert", "title"),
            Some(K::TdsMissingMap) => crate::tr!("import-3ds", "title"),
            Some(K::ExtractionOutdated) => crate::tr!("extraction-outdated", "title"),
            Some(K::XrefAttach) => crate::t!("Attach External Reference").into_owned(),
            Some(K::WriteBlock) => crate::t!("Write Block").into_owned(),
            Some(K::GeometricTolerance) => crate::t!("Geometric Tolerance").into_owned(),
            Some(K::AttDef) => crate::t!("Attribute Definition").into_owned(),
            Some(K::AttDefEdit) => crate::t!("Edit Attribute Definition").into_owned(),
            Some(K::AttMan) => crate::t!("Block Attribute Manager").into_owned(),
            Some(K::CountInvalidArea) => crate::t!("Count - Invalid Area").into_owned(),
            Some(K::Field) => crate::t!("Field").into_owned(),
            Some(K::DraftingSettings) => crate::t!("Drafting Settings").into_owned(),
            Some(K::AutoConstrainSettings) => crate::t!("Constraint Settings").into_owned(),
            Some(K::LayerStateEditor) => crate::tr!("modal", "edit-layer-state"),
            Some(K::Plot) => crate::tr!("modal", "plot"),
            Some(K::PrintAll) => t!("Print All").into_owned(),
            Some(K::LayoutManager) => crate::tr!("modal", "layout-manager"),
            Some(K::ScaleManager) => crate::tr!("modal", "scale-manager"),
            Some(K::StyleImport) => t!("Import Styles").into_owned(),
            Some(K::AnnoObjectScale) => crate::tr!("modal", "annotation-object-scale"),
            Some(K::Hyperlink) if self.hyperlink_options.edit => {
                crate::tr!("hyperlink-dialog", "title-edit")
            }
            Some(K::Hyperlink) => crate::tr!("hyperlink-dialog", "title-insert"),
            Some(K::InsertTable) => crate::t!("Insert Table").into_owned(),
            Some(K::DataLinkManager) => crate::t!("Data Link Manager").into_owned(),
            Some(K::DataExtraction) => crate::t!("Data Extraction Wizard").into_owned(),
            Some(K::DataExtractionUnsaved) => crate::t!("Data Extraction - Unsaved Drawing").into_owned(),
            Some(K::Plotstyle) => crate::tr!("modal", "plot-style-editor"),
            Some(K::TextStyle) => crate::tr!("modal", "text-style-manager"),
            Some(K::MlStyle) => crate::tr!("modal", "multiline-style-manager"),
            Some(K::TableStyle) => crate::tr!("modal", "table-style-manager"),
            Some(K::MLeaderStyle) => crate::tr!("modal", "multileader-style-manager"),
            Some(K::DimStyle) => crate::tr!("modal", "dimension-style-manager"),
            Some(K::AssocPrompt) => crate::tr!("modal", "default-application"),
            Some(K::AecDropWarning) => crate::tr!("modal", "save-warning"),
            Some(K::ConversionLoss) => crate::tr!("modal", "save-warning"),
            #[cfg(not(target_arch = "wasm32"))]
            Some(K::FileInUse) => crate::tr!("modal", "unable-save"),
            #[cfg(not(target_arch = "wasm32"))]
            Some(K::ExternalChange) => crate::tr!("modal", "drawing-changed"),
            Some(K::LayerDeleteWarning) => crate::tr!("modal", "delete-layer"),
            Some(K::Unsaved) => crate::tr!("modal", "unsaved-changes"),
            Some(K::BlockEditChanges) => crate::t!("Block Editor").into_owned(),
            Some(K::BlockEditPicker) => crate::t!("Edit Block Definition").into_owned(),
            Some(K::Rename) => crate::tr!("rename-dialog", "title"),
            Some(K::Purge) => crate::tr!("purge-dialog", "title"),
            Some(K::DrawingProperties) => crate::tr!("dwgprops-dialog", "title"),
            Some(K::SelectSimilarSettings) => crate::tr!("select-similar", "title"),
            Some(K::BlockVisibility) => crate::t!("Visibility States").into_owned(),
            Some(K::BlockLookup) => crate::t!("Property Lookup Table").into_owned(),
            Some(K::BlockPropertiesTable) => crate::t!("Block Properties Table").into_owned(),
            Some(K::BlockSaveAs) => crate::t!("Save Block As").into_owned(),
            Some(K::PointStyle) => crate::tr!("modal", "point-style"),
            Some(K::AttributeEditor) => crate::tr!("modal", "attribute-editor"),
            Some(K::SaveDialog) => crate::tr!("modal", "save-drawing-as"),
            Some(K::Recovery) => crate::tr!("modal", "recovery-report"),
            Some(K::MissingFonts) => crate::t!("Missing fonts").into_owned(),
            Some(K::RecoveryPrompt) => crate::tr!("modal", "recovery-prompt"),
            #[cfg(target_arch = "wasm32")]
            Some(K::AutosaveRestore) => crate::t!("Recover Drawing").into_owned(),
            Some(K::GpuWarning) => crate::tr!("gpu", "title"),
            Some(K::XrefHelp) => crate::t!("Reference Manager Help").into_owned(),
            Some(K::SheetSet) => self
                .sheet_set
                .dialog
                .as_ref()
                .map(|d| d.title())
                .unwrap_or_default(),
            Some(K::PluginDialog) => self
                .plugin_dialog_state
                .as_ref()
                .map(|s| s.title())
                .unwrap_or_else(|| "Plugin Dialog".to_string()),
            None => String::new(),
        }
    }
    pub(super) fn plot_modal_content<'s>(&'s self, extra: iced::Vector) -> Element<'s, Message> {
        sized_flow(extra, 940, 690, |flow| {
            crate::ui::window::plot::view_window(&self.plot_dialog, self.print_all_options, flow)
        })
    }
    /// Build the currently-open modal dialog's content (Plan B), or `None`.
    /// Iced 0.15 measures the content first, so dialogs start at their natural
    /// size and overflowing regions become scrollable where a cap is supplied.
    pub(super) fn modal_content<'s>(&'s self) -> Option<Element<'s, Message>> {
        let ex = self.modal_resize;
        Some(match self.active_modal? {
            super::super::ModalKind::StyleImport => sized_flow(ex, 540, 480, |flow| {
                crate::ui::style::style_manager::import_view(
                    self.style_import.as_ref().expect("style import dialog without draft"),
                    &self.tabs[self.active_tab].scene.document,
                    flow,
                )
            }),
            super::super::ModalKind::About => {
                automatic_flow(ex, crate::ui::window::about::view_window)
            }
            super::super::ModalKind::XrefHelp => {
                automatic_flow(ex, crate::ui::window::xref_help::view_window)
            }
            super::super::ModalKind::Shortcuts => self.shortcuts_modal_content(ex),
            super::super::ModalKind::Aliases => self.aliases_modal_content(ex),
            super::super::ModalKind::NamedParameters => {
                let scene = &self.tabs[self.active_tab].scene;
                sized_flow(ex, 820, 520, |flow| {
                    crate::ui::window::named_parameters::view_window(
                        &self.named_parameter_editor_rows,
                        scene,
                        flow,
                    )
                })
            }
            super::super::ModalKind::BlockEditorSettings => {
                crate::ui::window::block_editor_settings::view_window(self.block_editor_settings.as_ref()?)
            }
            super::super::ModalKind::BlockCycleOrder => {
                crate::ui::window::block_cycle_order::view_window(self.block_cycle_order.as_ref()?)
            }
            super::super::ModalKind::Options => {
                let dirty = self.options_dirty();
                let close_confirm = self.options_close_confirm;
                sized_flow(ex, 880, 620, |flow| {
                    crate::ui::window::options::view_window(
                        &self.default_save_format,
                        self.file_assoc_enabled,
                        self.show_constraint_values,
                        &self.ui_theme,
                        &self.theme_color_inputs,
                        self.language,
                        self.options_tab,
                        self.cursor_size,
                        crate::ui::window::options::SelectionPrefs {
                            pick_box: self.pick_box,
                            aperture: self.snapper.osnap_radius_px.round() as i32,
                            pick_add: self.pick_add,
                            pick_drag_rect: self.pick_drag_rect,
                            grip_object_limit: self.grip_object_limit,
                            selection_cycling: self.selection_cycling,
                        },
                        crate::ui::window::options::AppPrefs {
                            savetime_min: self.savetime_min,
                            backup_on_save: self.backup_on_save,
                            page_setup_on_new_layout: self.plot_dialog.page_setup_on_new_layout,
                            textfill: crate::scene::text::sdf_atlas::textfill(),
                            cliprompt_lines: self.cliprompt_lines,
                            commandline_fade_ms: self.commandline_fade_ms,
                            command_line_opacity: self.command_line.opacity_percent(),
                            show_menu_bar: self.show_menu_bar,
                            show_ribbon: !self.ribbon_closed,
                            hatch_associative: self.hatch_associative,
                            zoom_wheel_reversed: self.zoom_wheel_reversed,
                            navigation_style: self.navigation_style,
                            touchpad_pan: self.touchpad_pan,
                            ui_scale_percent: self.ui_scale_percent,
                            zoom_factor: self.zoom_factor,
                            texteditmode: self.texteditmode,
                            dimension_continue_mode: self.dimension_continue_mode,
                            qdim_snap_priority: self.quick_dimension_snap_priority,
                            annotation_auto_scale: self.annotation_auto_scale,
                            polar_increment_deg: self.polar_increment_deg,
                            show_viewcube: self.show_viewcube,
                            show_ucs_icon: self.show_ucs_icon,
                            ucs_icon_at_origin: self.ucs_icon_at_origin,
                            right_click_mode: self.right_click_mode,
                            right_click_hold_ms: self.right_click_hold_ms,
                        },
                        crate::ui::window::options::spacemouse::view(
                            self.spacemouse_preferences,
                            self.spacemouse.status(),
                            self.spacemouse_paused,
                            self.spacemouse_details,
                        ),
                        {
                            #[cfg(not(target_arch = "wasm32"))]
                            let page = crate::ui::window::options::graphics::view(
                                self.graphics_prefs,
                                crate::gpu_backend::active_gpu(),
                            );
                            #[cfg(target_arch = "wasm32")]
                            let page = iced::widget::Space::new().into();
                            page
                        },
                        &self.snap_angle_input,
                        &self.zoom_factor_input,
                        {
                            let header = self
                                .tabs
                                .get(self.active_tab)
                                .map(|tab| &tab.scene.document.header);
                            crate::ui::window::options::DrawingPrefs {
                                available: header.is_some(),
                                isolines: header.map_or(4, |h| h.isolines),
                                display_silhouette: header.is_some_and(|h| h.display_silhouette),
                                surface_u: header.map_or(6, |h| h.surface_u_density),
                                surface_v: header.map_or(6, |h| h.surface_v_density),
                                surface_type: header.map_or(6, |h| h.surface_type),
                                record_solid_history: header
                                    .is_some_and(|h| h.record_solid_history),
                                show_solid_history: header.map_or(1, |h| h.show_solid_history),
                            }
                        },
                        {
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                crate::ui::window::options::Folders {
                                    config: crate::config::config_dir()
                                        .map(|p| p.display().to_string()),
                                    plot_styles: crate::io::plot_style::plot_styles_dir()
                                        .ok()
                                        .map(|p| p.display().to_string()),
                                    plugins: crate::plugin::external::plugins_dir()
                                        .map(|p| p.display().to_string()),
                                    autosave: crate::config::config_dir()
                                        .map(|_| std::env::temp_dir().display().to_string()),
                                    fonts: crate::io::font_repo::fonts_dir()
                                        .map(|p| p.display().to_string()),
                                    color_books: crate::ui::color_select::book::folder()
                                        .map(|p| p.display().to_string()),
                                    fonts_moved: crate::io::font_repo::fonts_folder().is_some(),
                                    plot_styles_moved: crate::io::plot_style::plot_styles_folder()
                                        .is_some(),
                                    support: crate::io::font_repo::support_folder()
                                        .map(|p| p.display().to_string()),
                                }
                            }
                            #[cfg(target_arch = "wasm32")]
                            {
                                crate::ui::window::options::Folders::default()
                            }
                        },
                        &self.qnew_template,
                        self.double_click_block_refedit,
                        self.double_click_block_attedit,
                        self.cursor_type,
                        self.crosshair_color,
                        &self.crosshair_color_input,
                        self.snap_marker_color,
                        &self.snap_marker_color_input,
                        self.command_text_color,
                        &self.command_text_color_input,
                        self.lineweight_display_scale,
                        &self.model_space,
                        &self.model_bg_input,
                        &self.paper_bg_input,
                        &self.desk_bg_input,
                        dirty,
                        close_confirm,
                        flow,
                    )
                })
            }
            super::super::ModalKind::DraftingSettings => {
                let state = self.drafting_settings_state.as_ref();
                let dirty = self.drafting_settings_dirty();
                let confirm = self.drafting_settings_close_confirm;
                sized_flow(ex, 780, 500, |flow| {
                    if let Some(state) = state {
                        crate::ui::window::drafting_settings::view_window(
                            state, dirty, confirm, flow,
                        )
                    } else {
                        iced::widget::Space::new().into()
                    }
                })
            }
            super::super::ModalKind::AutoConstrainSettings => sized_flow(ex, 620, 610, |flow| {
                crate::ui::window::auto_constrain_settings::view_window(
                    &self.auto_constrain_settings,
                    self.auto_constrain_selected_row,
                    &self.auto_constrain_distance_input,
                    &self.auto_constrain_angle_input,
                    flow,
                )
            }),
            super::super::ModalKind::Overkill => {
                let state = self.overkill_dialog.as_ref()?;
                automatic_flow(ex, |flow| crate::ui::window::overkill::view_window(state, flow))
            }
            super::super::ModalKind::FindReplace => automatic_flow(ex, |flow| {
                crate::ui::window::find_replace::view_window(
                    &self.find_replace.search,
                    &self.find_replace.replacement,
                    &self.find_replace.status,
                    &self.find_replace.results,
                    self.find_replace.current_match,
                    flow,
                )
            }),
            super::super::ModalKind::PluginManager => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    sized_flow(ex, 940, 600, |flow| {
                        crate::ui::window::plugin_manager::view_window(
                            &self.disabled_plugins,
                            &self.external_plugins,
                            &self.loaded_plugin_ids,
                            &self.plugin_load_errors,
                            crate::ui::window::plugin_manager::MarketView {
                                registry: &self.plugin_registry,
                                registry_loading: self.plugin_registry_loading,
                                registry_error: self.plugin_registry_error.as_deref(),
                                registry_error_details_open: self
                                    .plugin_registry_error_details_open,
                                input: &self.plugin_repo_input,
                                search: &self.plugin_search_input,
                                repos: &self.plugin_repos,
                                release_tags: &self.repo_release_tags,
                                selected_tag: &self.repo_selected_tag,
                                selected_repo: self.selected_plugin_repo.as_deref(),
                                readmes: &self.plugin_readmes,
                                readme_loading: &self.plugin_readme_loading,
                                status: &self.marketplace_status,
                            },
                            &self.active_theme,
                            flow,
                        )
                    })
                }
                #[cfg(target_arch = "wasm32")]
                {
                    automatic_flow(ex, |flow| {
                        container(crate::ui::window::plugin_manager::view_web_notice())
                            .width(flow.width)
                            .height(flow.height)
                            .into()
                    })
                }
            }
            super::super::ModalKind::UpdateNotice => {
                let latest = self.update_notice_version.as_deref().unwrap_or("?");
                let body = self.update_notice_body.as_deref().unwrap_or("");
                sized_flow(ex, 560, 460, |flow| {
                    crate::ui::window::update_notice::view_window(latest, body, flow)
                })
            }
            super::super::ModalKind::LayerTranslator => {
                use crate::modules::draw::layers::laytrans;
                let i = self.active_tab;
                let current = self.tabs[i].active_layer.clone();
                let sources = laytrans::source_layers(&self.tabs[i].scene, &current);
                let state = self.layer_translator.as_ref()?;
                sized_flow(ex, 760, 460, |flow| {
                    crate::ui::window::layer_translator::view_window(state, sources.clone(), flow)
                })
            }
            super::super::ModalKind::DrawingUnits => {
                let state = self.drawing_units.as_ref()?;
                sized_flow(ex, 560, 420, |flow| {
                    crate::ui::window::drawing_units::view_window(state, flow)
                })
            }
            super::super::ModalKind::BlockDefinition => {
                let state = self.block_definition.as_ref()?;
                sized_flow(ex, 600, 560, |flow| {
                    crate::ui::window::block_definition::view_window(state, flow)
                })
            }
            super::super::ModalKind::BlockEditPicker => {
                let state = self.block_edit_picker.as_ref()?;
                sized_flow(ex, 540, 380, |flow| {
                    crate::ui::window::block_edit_picker::view(state, flow)
                })
            }
            super::super::ModalKind::SelectSimilarSettings => {
                crate::ui::window::select_similar::view_window(self.select_similar_settings?)
            }
            super::super::ModalKind::DrawingProperties => {
                let state = self.drawing_properties.as_ref()?;
                sized_flow(ex, 560, 440, |flow| {
                    crate::ui::window::drawing_properties::view(state, flow)
                })
            }
            super::super::ModalKind::Purge => {
                let state = self.purge_dialog.as_ref()?;
                sized_flow(ex, 460, 520, |flow| crate::ui::window::purge::view(state, flow))
            }
            super::super::ModalKind::Rename => {
                let state = self.rename_dialog.as_ref()?;
                sized_flow(ex, 520, 420, |flow| crate::ui::window::rename::view(state, flow))
            }
            super::super::ModalKind::BlockLookup => {
                let state = self.block_lookup.as_ref()?;
                sized_flow(ex, 940, 600, |flow| {
                    crate::ui::window::block_lookup::view(state, flow)
                })
            }
            super::super::ModalKind::BlockPropertiesTable => {
                let state = self.block_properties_table.as_ref()?;
                sized_flow(ex, 940, 600, |flow| {
                    crate::ui::window::block_properties_table::view(state, flow)
                })
            }
            super::super::ModalKind::BlockVisibility => {
                let state = self.block_visibility.as_ref()?;
                sized_flow(ex, 580, 470, |flow| {
                    crate::ui::window::block_visibility::view(state, flow)
                })
            }
            super::super::ModalKind::BlockSaveAs => {
                let state = self.block_save_as.as_ref()?;
                sized_flow(ex, 430, 370, |flow| {
                    crate::ui::window::block_save_as::view(state, flow)
                })
            }
            super::super::ModalKind::PdfAttach => {
                let state = self.pdf_attach.as_ref()?;
                sized_flow(ex, 880, 540, |flow| {
                    crate::ui::window::pdf_dialogs::view_attach(state, flow)
                })
            }
            super::super::ModalKind::PointCloudAttach => {
                let state = self.point_cloud_attach.as_ref()?;
                sized_flow(ex, 880, 560, |flow| {
                    crate::ui::window::pdf_dialogs::view_point_cloud_attach(state, flow)
                })
            }
            super::super::ModalKind::PointCloudColorMap => {
                let state = self.point_cloud_color_map.as_ref()?;
                sized_flow(ex, 760, 560, |flow| {
                    crate::ui::window::pdf_dialogs::view_point_cloud_color_map(state, flow)
                })
            }
            super::super::ModalKind::PcSection => {
                let state = self.pc_section.as_ref()?;
                sized_flow(ex, 760, 470, |flow| {
                    crate::ui::window::pdf_dialogs::view_pc_section(state, flow)
                })
            }
            super::super::ModalKind::ViewBackground => {
                let state = self.view_background.as_ref()?;
                sized_flow(ex, 420, 560, |flow| {
                    crate::ui::window::pdf_dialogs::view_background(state, flow)
                })
            }
            super::super::ModalKind::ImageAdjust => {
                let state = self.image_adjust.as_ref()?;
                sized_flow(ex, 520, 560, |flow| {
                    crate::ui::window::pdf_dialogs::image_adjust(state, flow)
                })
            }
            super::super::ModalKind::PdfShxSettings => {
                let state = self.pdf_shx_dialog.as_ref()?;
                sized_flow(ex, 780, 600, |flow| {
                    crate::ui::window::pdf_shx_dialog::view(state, flow)
                })
            }
            super::super::ModalKind::PdfPassword => {
                let state = self.pdf_password.as_ref()?;
                sized_flow(ex, 440, 430, |flow| {
                    crate::ui::window::pdf_password_dialog::view(state, flow)
                })
            }
            super::super::ModalKind::DimInspect => {
                let state = self.dim_inspect_dialog.as_ref()?;
                sized_flow(ex, 480, 470, |flow| {
                    crate::ui::window::dim_inspect_dialog::view(state, flow)
                })
            }
            super::super::ModalKind::RefEdit => {
                let state = self.refedit_dialog.as_ref()?;
                sized_flow(ex, 560, 470, |flow| {
                    crate::ui::window::refedit_dialog::view(state, flow)
                })
            }
            super::super::ModalKind::UnderlayLayers => {
                let state = self.underlay_layers.as_ref()?;
                sized_flow(ex, 460, 520, |flow| {
                    crate::ui::window::pdf_dialogs::view_layers(state, flow)
                })
            }
            super::super::ModalKind::PdfImportSettings => {
                let settings = self.pdf_import_settings.as_ref()?;
                sized_flow(ex, 640, 380, |flow| {
                    crate::ui::window::pdf_dialogs::view_import_settings(settings, flow)
                })
            }
            super::super::ModalKind::Spell => {
                let state = self.spell.as_ref()?;
                sized_flow(ex, 600, 620, |flow| crate::ui::window::spell_dialog::view(state, flow))
            }
            super::super::ModalKind::Txt2MtxtSettings => {
                let settings = self.txt2mtxt_dialog.as_ref()?;
                sized_flow(ex, 600, 470, |flow| {
                    crate::ui::window::pdf_dialogs::view_txt2mtxt_settings(settings, flow)
                })
            }
            super::super::ModalKind::PdfImportFile => {
                let state = self.pdf_import_file.as_ref()?;
                sized_flow(ex, 960, 560, |flow| {
                    crate::ui::window::pdf_dialogs::view_import_file(state, flow)
                })
            }
            super::super::ModalKind::ImportFile => {
                let state = self.import_dialogs.file.as_ref()?;
                let recent = &self.import_dialogs.recent;
                sized_flow(ex, 960, 620, |flow| {
                    crate::ui::window::import_dialogs::view_import_file(state, recent, flow)
                })
            }
            super::super::ModalKind::TdsImport => {
                let state = self.import_dialogs.tds.as_ref()?;
                sized_flow(ex, 860, 560, |flow| {
                    crate::ui::window::import_dialogs::view_tds_import(state, flow)
                })
            }
            super::super::ModalKind::TdsMaterial => {
                let state = self.import_dialogs.tds_material.as_ref()?;
                sized_flow(ex, 460, 420, |flow| {
                    crate::ui::window::import_dialogs::view_tds_material(state, flow)
                })
            }
            super::super::ModalKind::ExtractionOutdated => {
                let prompt = self.extraction_outdated.as_ref()?;
                let first = prompt.tables.first()?;
                let position = prompt.count - prompt.tables.len() + 1;
                sized_flow(ex, 480, 250, |flow| {
                    crate::ui::window::extraction_outdated::view(
                        &first.settings,
                        position,
                        prompt.count,
                        flow,
                    )
                })
            }
            super::super::ModalKind::TdsMissingMap => {
                let map = self.import_dialogs.tds_maps.first()?;
                sized_flow(ex, 460, 240, |flow| {
                    crate::ui::window::import_dialogs::view_tds_map(map, flow)
                })
            }
            super::super::ModalKind::DgnImport => {
                let state = self.import_dialogs.dgn.as_ref()?;
                sized_flow(ex, 900, 680, |flow| {
                    crate::ui::window::import_dialogs::view_dgn_import(state, flow)
                })
            }
            super::super::ModalKind::XrefAttach => {
                let state = self.xref_attach.as_ref()?;
                let height = if state.details { 520 } else { 450 };
                sized_flow(ex, 740, height, |flow| {
                    crate::ui::window::xref_attach::view_window(state, flow)
                })
            }
            super::super::ModalKind::WriteBlock => {
                let state = self.wblock.as_ref()?;
                sized_flow(ex, 440, 395, |flow| {
                    crate::ui::window::wblock::view_window(state, flow)
                })
            }
            super::super::ModalKind::AttDef => {
                let state = self.attdef_dialog.as_ref()?;
                sized_flow(ex, 720, 520, |flow| {
                    crate::ui::window::attdef_dialog::view(state, flow)
                })
            }
            super::super::ModalKind::SheetSet => {
                let (w, h) = match self.sheet_set.dialog.as_ref()? {
                    crate::ui::window::sheet_set::SsDialog::Wizard(_) => (720, 560),
                    crate::ui::window::sheet_set::SsDialog::Properties(_) => (640, 700),
                    crate::ui::window::sheet_set::SsDialog::Form(f) => match f.kind {
                        crate::ui::window::sheet_set::FormKind::Rename => (560, 470),
                        crate::ui::window::sheet_set::FormKind::RenameView => (520, 200),
                        crate::ui::window::sheet_set::FormKind::ImportLayout => (620, 470),
                        _ => (560, 420),
                    },
                    crate::ui::window::sheet_set::SsDialog::Confirm(..) => (460, 190),
                    crate::ui::window::sheet_set::SsDialog::Template(_) => (480, 400),
                    crate::ui::window::sheet_set::SsDialog::Category(_) => (480, 440),
                    crate::ui::window::sheet_set::SsDialog::BlockList(_) => (540, 330),
                    crate::ui::window::sheet_set::SsDialog::SelectBlock(_) => (580, 390),
                };
                sized_flow(ex, w, h, |flow| {
                    crate::ui::window::sheet_set::dialog_view(&self.sheet_set, flow)
                })
            }
            super::super::ModalKind::Field => {
                let state = self.field_dialog.as_ref()?;
                sized_flow(ex, 760, 660, |flow| {
                    crate::ui::window::field_dialog::view(state, flow)
                })
            }
            super::super::ModalKind::CountInvalidArea => sized_flow(ex, 520, 260, |flow| {
                crate::ui::window::count_palette::invalid_area_view(&self.count_palette, flow)
            }),
            super::super::ModalKind::AttMan => {
                let state = self.attman.as_ref()?;
                sized_flow(ex, 880, 660, |flow| {
                    crate::ui::window::attman_dialog::view(state, flow)
                })
            }
            super::super::ModalKind::AttDefEdit => {
                let state = self.attdef_edit.as_ref()?;
                sized_flow(ex, 460, 220, |flow| {
                    crate::ui::window::attdef_dialog::view_edit(state, flow)
                })
            }
            super::super::ModalKind::GeometricTolerance => {
                let state = self.geometric_tolerance.as_ref()?;
                sized_flow(ex, 670, 530, |flow| {
                    crate::ui::window::geometric_tolerance::view_window(state, flow)
                })
            }
            super::super::ModalKind::LayerStateManager => {
                let states = self.tabs[self.active_tab].scene.document.layer_states();
                sized_flow(ex, 720, 420, |flow| {
                    crate::ui::window::layer_state_manager::view_window(
                        states.clone(),
                        self.layer_state_selected.as_deref(),
                        &self.layer_state_name_buf,
                        &self.layer_state_description_buf,
                        &self.layer_state_filter,
                        flow,
                    )
                })
            }
            super::super::ModalKind::LayerStateEditor => {
                let tab = &self.tabs[self.active_tab];
                if let Some(state) = self.layer_state_edit_draft.as_ref() {
                    let mut linetypes: Vec<String> = tab
                        .scene
                        .document
                        .line_types
                        .iter()
                        .map(|line_type| line_type.name.clone())
                        .collect();
                    for layer in &state.layers {
                        if !layer.line_type.is_empty()
                            && !linetypes
                                .iter()
                                .any(|name| name.eq_ignore_ascii_case(&layer.line_type))
                        {
                            linetypes.push(layer.line_type.clone());
                        }
                    }
                    linetypes.sort_by_key(|name| name.to_lowercase());
                    // Shared ownership: the build closure runs twice per frame
                    // (INTRINSIC + FILL) and each row keeps its own copy, so a
                    // borrow cannot work here — an Arc clone per row is a
                    // refcount bump instead of N string clones (#21, cf. #34).
                    let linetypes = std::sync::Arc::<[String]>::from(linetypes);
                    sized_flow(ex, 1180, 560, {
                        let linetypes = std::sync::Arc::clone(&linetypes);
                        move |flow| {
                            crate::ui::window::layer_state_manager::view_editor(
                                state,
                                &self.layer_state_edit_filter,
                                self.layer_state_edit_color_open,
                                std::sync::Arc::clone(&linetypes),
                                flow,
                            )
                        }
                    })
                } else {
                    automatic_flow(ex, |flow| {
                        container(text(t!("The selected layer state is no longer available.")))
                            .padding(16)
                            .width(flow.width)
                            .height(flow.height)
                            .into()
                    })
                }
            }
            super::super::ModalKind::Plot => self.plot_modal_content(ex),
            super::super::ModalKind::PrintAll => sized_flow(ex, 520, 420, |flow| {
                crate::ui::window::print_all::view_window(
                    &self.print_all_layouts,
                    self.plot_dialog.printer.as_deref(),
                    flow,
                )
            }),
            super::super::ModalKind::LayoutManager => {
                let i = self.active_tab;
                let layouts = self.tabs[i].scene.layout_names();
                let current = self.tabs[i].scene.current_layout.clone();
                sized_flow(ex, 640, 320, |flow| {
                    crate::ui::window::layout_manager::view_window(
                        layouts.clone(),
                        &self.layout_manager_selected,
                        &self.layout_manager_rename_buf,
                        current.clone(),
                        flow,
                    )
                })
            }
            super::super::ModalKind::ScaleManager => {
                let tab = &self.tabs[self.active_tab];
                let scales: Vec<(String, String)> = tab
                    .scene
                    .scale_list()
                    .into_iter()
                    .map(|(name, _, _)| {
                        let ratio = tab
                            .scene
                            .scale_paper_drawing(&name)
                            .map(|(p, d)| format!("{p}:{d}"))
                            .unwrap_or_default();
                        (name, ratio)
                    })
                    .collect();
                let current = tab.scene.document.header.current_annotation_scale.clone();
                sized_flow(ex, 520, 360, |flow| {
                    crate::ui::style::scale_manager::view_window(
                        &scales,
                        &self.scale_manager_selected,
                        &current,
                        self.scale_rename.as_deref(),
                        &self.scale_rename_buf,
                        &self.scale_manager_paper_buf,
                        &self.scale_manager_drawing_buf,
                        flow,
                    )
                })
            }
            super::super::ModalKind::AnnoObjectScale => {
                let tab = &self.tabs[self.active_tab];
                let entity = self.anno_object_scale_target;
                // Which scales the object currently has a representation for.
                let members: Vec<codec::types::Handle> = entity
                    .map(|h| {
                        crate::scene::annotative::object_scale_memberships(&tab.scene.document, h)
                            .into_iter()
                            .map(|(_, sh)| sh)
                            .collect()
                    })
                    .unwrap_or_default();
                let label = entity
                    .and_then(|h| tab.scene.document.get_entity(h))
                    .map(|e| match e {
                        codec::EntityType::Text(_) => "TEXT",
                        codec::EntityType::MText(_) => "MTEXT",
                        codec::EntityType::Insert(_) => "BLOCK",
                        codec::EntityType::MultiLeader(_) => "MULTILEADER",
                        _ => "OBJECT",
                    })
                    .unwrap_or("—");
                let scales: Vec<(String, String, bool)> = tab
                    .scene
                    .scale_list()
                    .into_iter()
                    .map(|(name, _, _)| {
                        let sh = tab.scene.scale_object_handle(&name);
                        let ratio = tab
                            .scene
                            .scale_paper_drawing(&name)
                            .map(|(p, d)| format!("{p}:{d}"))
                            .unwrap_or_default();
                        let is_member = sh.map(|h| members.contains(&h)).unwrap_or(false);
                        (name, ratio, is_member)
                    })
                    .collect();
                sized_flow(ex, 360, 420, |flow| {
                    crate::ui::style::anno_object_scale::view_window(&label, &scales, flow)
                })
            }
            super::super::ModalKind::InsertTable => sized_flow(ex, 860, 560, |flow| {
                crate::ui::window::annotation_data::table_insert_view(&self.table_insert, flow)
            }),
            super::super::ModalKind::DataLinkManager => sized_flow(ex, 760, 560, |flow| {
                crate::ui::window::annotation_data::data_link_view(&self.data_link_manager, flow)
            }),
            super::super::ModalKind::DataExtractionUnsaved => sized_flow(ex, 440, 200, |flow| {
                crate::ui::window::annotation_data::data_extraction_unsaved_view(flow)
            }),
            super::super::ModalKind::DataExtraction => sized_flow(ex, 780, 570, |flow| {
                crate::ui::window::annotation_data::data_extraction_view(
                    &self.data_extraction,
                    flow,
                )
            }),
            super::super::ModalKind::Plotstyle => sized_flow(ex, 780, 540, |flow| {
                crate::ui::style::plotstyle::view_window(
                    &self.tabs[self.active_tab].scene.document,
                    self.active_plot_style.as_ref(),
                    self.plotstyle_panel_aci,
                    &self.ps_color_buf,
                    &self.ps_lineweight_buf,
                    &self.ps_screening_buf,
                    flow,
                )
            }),
            super::super::ModalKind::TextStyle => {
                let tab = &self.tabs[self.active_tab];
                let doc = &tab.scene.document;
                let styles: Vec<String> = tab
                    .scene
                    .document
                    .text_styles
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                let selected_style = doc.text_styles.get(&self.textstyle_selected);
                let (backward, upside_down, vertical, annotative, read_only) = selected_style
                    .map(|style| {
                        (
                            style.flags.backward,
                            style.flags.upside_down,
                            style.is_vertical,
                            style.annotative,
                            style.xref_dependent,
                        )
                    })
                    .unwrap_or((false, false, false, false, false));
                let ttf_flags = selected_style.map_or(0, |style| style.true_type_font_flags);
                let in_use =
                    self.style_in_use(crate::app::StyleKind::Text, &self.textstyle_selected);
                let compare_opts: Vec<String> = styles
                    .iter()
                    .filter(|name| !name.eq_ignore_ascii_case(&self.textstyle_selected))
                    .cloned()
                    .collect();
                let compare_name = compare_opts
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(&self.textstyle_compare))
                    .cloned()
                    .or_else(|| compare_opts.first().cloned())
                    .unwrap_or_default();
                let mut comparison_sections = Vec::new();
                if let (Some(a), Some(b)) = (selected_style, doc.text_styles.get(&compare_name)) {
                    if (
                        &a.font_file,
                        &a.big_font_file,
                        &a.true_type_font,
                        a.is_shape_file,
                    ) != (
                        &b.font_file,
                        &b.big_font_file,
                        &b.true_type_font,
                        b.is_shape_file,
                    ) {
                        comparison_sections.push(crate::i18n::translate("Fonts").into_owned());
                    }
                    if (a.height, a.width_factor, a.oblique_angle, a.last_height)
                        != (b.height, b.width_factor, b.oblique_angle, b.last_height)
                    {
                        comparison_sections.push(crate::i18n::translate("Size").into_owned());
                    }
                    if (a.flags, a.is_vertical, a.annotative)
                        != (b.flags, b.is_vertical, b.annotative)
                    {
                        comparison_sections.push(crate::i18n::translate("Effects").into_owned());
                    }
                }
                sized_flow(ex, 960, 680, |flow| {
                    crate::ui::style::textstyle::view_window(
                        crate::ui::style::textstyle::TextStyleView {
                            styles: styles.clone(),
                            selected: &self.textstyle_selected,
                            current: &tab.scene.document.header.current_text_style_name,
                            tab: self.textstyle_tab,
                            compare_name: compare_name.clone(),
                            compare_opts: compare_opts.clone(),
                            comparison_sections: comparison_sections.clone(),
                            read_only,
                            in_use,
                            font_buf: &self.textstyle_font,
                            width_buf: &self.textstyle_width,
                            oblique_buf: &self.textstyle_oblique,
                            height_buf: &self.textstyle_height,
                            bigfont_buf: &self.textstyle_bigfont,
                            ttf_buf: &self.textstyle_ttf,
                            bold: ttf_flags & crate::entities::text_support::TTF_FLAG_BOLD != 0,
                            italic: ttf_flags & crate::entities::text_support::TTF_FLAG_ITALIC
                                != 0,
                            backward,
                            upside_down,
                            vertical,
                            annotative,
                            rename_active: self.style_rename.as_deref(),
                            rename_buf: &self.style_rename_buf,
                        },
                        flow,
                    )
                })
            }
            super::super::ModalKind::MlStyle => {
                use codec::objects::ObjectType;
                let tab = &self.tabs[self.active_tab];
                let styles: Vec<String> = tab
                    .scene
                    .document
                    .objects
                    .values()
                    .filter_map(|o| match o {
                        ObjectType::MLineStyle(s) => Some(s.name.clone()),
                        _ => None,
                    })
                    .collect();
                let selected_style = tab.scene.document.objects.values().find_map(|o| match o {
                    ObjectType::MLineStyle(s) if s.name == self.mlstyle_selected => Some(s),
                    _ => None,
                });
                let compare_opts: Vec<String> = styles
                    .iter()
                    .filter(|name| !name.eq_ignore_ascii_case(&self.mlstyle_selected))
                    .cloned()
                    .collect();
                let compare_name = compare_opts
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(&self.mlstyle_compare))
                    .cloned()
                    .or_else(|| compare_opts.first().cloned())
                    .unwrap_or_default();
                let compare_style =
                    tab.scene
                        .document
                        .objects
                        .values()
                        .find_map(|object| match object {
                            ObjectType::MLineStyle(style) if style.name == compare_name => {
                                Some(style)
                            }
                            _ => None,
                        });
                let mut comparison_sections = Vec::new();
                if let (Some(a), Some(b)) = (selected_style, compare_style) {
                    if (
                        &a.description,
                        a.flags,
                        a.fill_color,
                        a.start_angle,
                        a.end_angle,
                    ) != (
                        &b.description,
                        b.flags,
                        b.fill_color,
                        b.start_angle,
                        b.end_angle,
                    ) {
                        comparison_sections
                            .push(crate::i18n::translate("Caps and Fill").into_owned());
                    }
                    if a.elements != b.elements {
                        comparison_sections.push(crate::i18n::translate("Elements").into_owned());
                    }
                }
                let in_use =
                    self.style_in_use(crate::app::StyleKind::MLine, &self.mlstyle_selected);
                sized_flow(ex, 960, 680, |flow| {
                    crate::ui::style::mlstyle::view_window(
                        crate::ui::style::mlstyle::MlStyleView {
                            styles: styles.clone(),
                            selected: &self.mlstyle_selected,
                            style: selected_style,
                            current: tab.scene.document.header.multiline_style.clone(),
                            tab: self.mlstyle_tab,
                            compare_name: compare_name.clone(),
                            compare_opts: compare_opts.clone(),
                            comparison_sections: comparison_sections.clone(),
                            in_use,
                            description: &self.mln_description,
                            start_angle: &self.mln_start_angle,
                            end_angle: &self.mln_end_angle,
                            fill_color: &self.mln_fill_color,
                            elements: &self.mln_elements,
                            rename_active: self.style_rename.as_deref(),
                            rename_buf: &self.style_rename_buf,
                        },
                        flow,
                    )
                })
            }
            super::super::ModalKind::TableStyle => {
                use codec::objects::ObjectType;
                let tab = &self.tabs[self.active_tab];
                let styles: Vec<String> = tab
                    .scene
                    .document
                    .objects
                    .values()
                    .filter_map(|o| match o {
                        ObjectType::TableStyle(s) => Some(s.name.clone()),
                        _ => None,
                    })
                    .filter(|name| {
                        !self.tablestyle_in_use_only
                            || self.style_in_use(crate::app::StyleKind::Table, name)
                    })
                    .collect();
                let selected_style = tab.scene.document.objects.values().find_map(|o| match o {
                    ObjectType::TableStyle(s) if s.name == self.tablestyle_selected => Some(s),
                    _ => None,
                });
                let compare_opts: Vec<String> = styles
                    .iter()
                    .filter(|name| !name.eq_ignore_ascii_case(&self.tablestyle_selected))
                    .cloned()
                    .collect();
                let compare_name = compare_opts
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(&self.tablestyle_compare))
                    .cloned()
                    .or_else(|| compare_opts.first().cloned())
                    .unwrap_or_default();
                let compare_style =
                    tab.scene
                        .document
                        .objects
                        .values()
                        .find_map(|object| match object {
                            ObjectType::TableStyle(style) if style.name == compare_name => {
                                Some(style)
                            }
                            _ => None,
                        });
                let mut comparison_sections = Vec::new();
                if let (Some(a), Some(b)) = (selected_style, compare_style) {
                    if (
                        &a.description,
                        a.flow_direction,
                        a.horizontal_margin,
                        a.vertical_margin,
                        a.title_suppressed,
                        a.header_suppressed,
                        a.annotative,
                    ) != (
                        &b.description,
                        b.flow_direction,
                        b.horizontal_margin,
                        b.vertical_margin,
                        b.title_suppressed,
                        b.header_suppressed,
                        b.annotative,
                    ) {
                        comparison_sections.push(crate::i18n::translate("General").into_owned());
                    }
                    for (label, different) in [
                        ("Data Row", a.data_row_style != b.data_row_style),
                        ("Header Row", a.header_row_style != b.header_row_style),
                        ("Title Row", a.title_row_style != b.title_row_style),
                    ] {
                        if different {
                            comparison_sections.push(crate::i18n::translate(label).into_owned());
                        }
                    }
                }
                let in_use =
                    self.style_in_use(crate::app::StyleKind::Table, &self.tablestyle_selected);
                sized_flow(ex, 960, 680, |flow| {
                    crate::ui::style::tablestyle::view_window(
                        crate::ui::style::tablestyle::TableStyleView {
                            styles: styles.clone(),
                            selected: &self.tablestyle_selected,
                            current: &tab.scene.document.header.current_table_style_name,
                            style: selected_style,
                            tab: self.tablestyle_tab,
                            compare_name: compare_name.clone(),
                            compare_opts: compare_opts.clone(),
                            comparison_sections: comparison_sections.clone(),
                            in_use,
                            in_use_only: self.tablestyle_in_use_only,
                            hmargin: &self.ts_hmargin,
                            vmargin: &self.ts_vmargin,
                            description: &self.ts_description,
                            cell_textstyle: &self.ts_cell_textstyle,
                            cell_height: &self.ts_cell_height,
                            cell_textcolor: &self.ts_cell_textcolor,
                            cell_fillcolor: &self.ts_cell_fillcolor,
                            cell_datatype: &self.ts_cell_datatype,
                            cell_unittype: &self.ts_cell_unittype,
                            cell_format: &self.ts_cell_format,
                            border_lw: &self.ts_border_lw,
                            border_color: &self.ts_border_color,
                            border_spacing: &self.ts_border_spacing,
                            rename_active: self.style_rename.as_deref(),
                            rename_buf: &self.style_rename_buf,
                            color_open: self.ts_color_open,
                        },
                        flow,
                    )
                })
            }
            super::super::ModalKind::MLeaderStyle => {
                use codec::objects::ObjectType;
                let tab = &self.tabs[self.active_tab];
                let styles: Vec<String> = tab
                    .scene
                    .document
                    .objects
                    .values()
                    .filter_map(|o| match o {
                        ObjectType::MultiLeaderStyle(s) => Some(s.name.clone()),
                        _ => None,
                    })
                    .collect();
                let selected_style = tab.scene.document.objects.values().find_map(|o| match o {
                    ObjectType::MultiLeaderStyle(s) if s.name == self.mleaderstyle_selected => {
                        Some(s)
                    }
                    _ => None,
                });
                let doc = &tab.scene.document;
                let mut block_opts: Vec<String> = vec!["None".to_string()];
                block_opts.extend(doc.block_records.iter().map(|b| b.name.clone()));
                let arrow_opts = crate::entities::arrowheads::options(
                    selected_style
                        .and_then(|s| s.arrowhead_handle)
                        .map(|h| crate::entities::arrowheads::label(doc, h))
                        .as_deref(),
                );
                let mut lt_opts: Vec<String> = vec!["ByBlock".to_string()];
                lt_opts.extend(doc.line_types.iter().map(|lt| lt.name.clone()));
                let mut textstyle_opts: Vec<String> = vec!["None".to_string()];
                textstyle_opts.extend(
                    doc.text_styles
                        .iter()
                        .filter(|t| !t.is_shape_file)
                        .map(|t| t.name.clone()),
                );
                let opt_block = |h: Option<codec::types::Handle>| -> String {
                    match h {
                        Some(h) => doc
                            .block_records
                            .iter()
                            .find(|b| b.handle == h)
                            .map(|b| b.name.clone())
                            .unwrap_or_else(|| "None".to_string()),
                        None => "None".to_string(),
                    }
                };
                let opt_lt = |h: Option<codec::types::Handle>| -> String {
                    match h {
                        Some(h) => doc
                            .line_types
                            .iter()
                            .find(|lt| lt.handle == h)
                            .map(|lt| lt.name.clone())
                            .unwrap_or_else(|| "ByBlock".to_string()),
                        None => "ByBlock".to_string(),
                    }
                };
                let opt_ts = |h: Option<codec::types::Handle>| -> String {
                    match h {
                        Some(h) => doc
                            .text_styles
                            .iter()
                            .find(|t| t.handle == h)
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| "None".to_string()),
                        None => "None".to_string(),
                    }
                };
                let (line_type_name, arrowhead_name, text_style_name, block_content_name) =
                    match selected_style {
                        Some(s) => (
                            opt_lt(s.line_type_handle),
                            crate::entities::arrowheads::label(
                                doc,
                                s.arrowhead_handle.unwrap_or(codec::types::Handle::NULL),
                            ),
                            opt_ts(s.text_style_handle),
                            opt_block(s.block_content_handle),
                        ),
                        None => Default::default(),
                    };
                let compare_opts: Vec<String> = styles
                    .iter()
                    .filter(|name| !name.eq_ignore_ascii_case(&self.mleaderstyle_selected))
                    .cloned()
                    .collect();
                let compare_name = compare_opts
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(&self.mleaderstyle_compare))
                    .cloned()
                    .or_else(|| compare_opts.first().cloned())
                    .unwrap_or_default();
                let compare_style = doc.objects.values().find_map(|object| match object {
                    ObjectType::MultiLeaderStyle(style) if style.name == compare_name => {
                        Some(style)
                    }
                    _ => None,
                });
                let mut comparison_sections = Vec::new();
                if let (Some(a), Some(b)) = (selected_style, compare_style) {
                    if (
                        &a.description,
                        a.path_type,
                        a.line_color,
                        a.line_type_handle,
                        a.line_weight,
                        a.arrowhead_handle,
                        a.arrowhead_size,
                        a.break_gap_size,
                    ) != (
                        &b.description,
                        b.path_type,
                        b.line_color,
                        b.line_type_handle,
                        b.line_weight,
                        b.arrowhead_handle,
                        b.arrowhead_size,
                        b.break_gap_size,
                    ) {
                        comparison_sections
                            .push(crate::i18n::translate("Leader Format").into_owned());
                    }
                    if (
                        a.enable_landing,
                        a.enable_dogleg,
                        a.landing_distance,
                        a.landing_gap,
                        a.scale_factor,
                        a.align_space,
                        a.max_leader_points,
                        a.first_segment_angle,
                        a.second_segment_angle,
                        a.leader_draw_order,
                        a.multileader_draw_order,
                        a.is_annotative,
                    ) != (
                        b.enable_landing,
                        b.enable_dogleg,
                        b.landing_distance,
                        b.landing_gap,
                        b.scale_factor,
                        b.align_space,
                        b.max_leader_points,
                        b.first_segment_angle,
                        b.second_segment_angle,
                        b.leader_draw_order,
                        b.multileader_draw_order,
                        b.is_annotative,
                    ) {
                        comparison_sections
                            .push(crate::i18n::translate("Leader Structure").into_owned());
                    }
                    if (
                        a.content_type,
                        &a.default_text,
                        a.text_style_handle,
                        a.text_height,
                        a.text_color,
                        a.text_angle_type,
                        a.text_alignment,
                    ) != (
                        b.content_type,
                        &b.default_text,
                        b.text_style_handle,
                        b.text_height,
                        b.text_color,
                        b.text_angle_type,
                        b.text_alignment,
                    ) || (
                        a.text_left_attachment,
                        a.text_right_attachment,
                        a.text_top_attachment,
                        a.text_bottom_attachment,
                        a.text_attachment_direction,
                        a.text_frame,
                        a.text_always_left,
                    ) != (
                        b.text_left_attachment,
                        b.text_right_attachment,
                        b.text_top_attachment,
                        b.text_bottom_attachment,
                        b.text_attachment_direction,
                        b.text_frame,
                        b.text_always_left,
                    ) {
                        comparison_sections.push(crate::i18n::translate("Content").into_owned());
                    }
                    if (
                        a.block_content_handle,
                        a.block_content_color,
                        a.block_content_connection,
                        a.block_content_rotation,
                        a.block_content_scale_x,
                        a.block_content_scale_y,
                        a.block_content_scale_z,
                        a.enable_block_scale,
                        a.enable_block_rotation,
                    ) != (
                        b.block_content_handle,
                        b.block_content_color,
                        b.block_content_connection,
                        b.block_content_rotation,
                        b.block_content_scale_x,
                        b.block_content_scale_y,
                        b.block_content_scale_z,
                        b.enable_block_scale,
                        b.enable_block_rotation,
                    ) {
                        comparison_sections
                            .push(crate::i18n::translate("Block Content").into_owned());
                    }
                }
                let in_use =
                    self.style_in_use(crate::app::StyleKind::MLeader, &self.mleaderstyle_selected);
                sized_flow(ex, 960, 680, |flow| {
                    crate::ui::style::mleaderstyle::view_window(
                        crate::ui::style::mleaderstyle::MLeaderStyleView {
                            styles: styles.clone(),
                            selected: &self.mleaderstyle_selected,
                            style: selected_style,
                            current: tab.active_mleader_style.clone(),
                            tab: self.mleaderstyle_tab,
                            compare_name: compare_name.clone(),
                            compare_opts: compare_opts.clone(),
                            comparison_sections: comparison_sections.clone(),
                            in_use,
                            landing_distance: &self.mls_landing_distance,
                            landing_gap: &self.mls_landing_gap,
                            arrowhead_size: &self.mls_arrowhead_size,
                            text_height: &self.mls_text_height,
                            scale_factor: &self.mls_scale_factor,
                            break_gap: &self.mls_break_gap,
                            first_seg_angle: &self.mls_first_seg_angle,
                            second_seg_angle: &self.mls_second_seg_angle,
                            max_points: &self.mls_max_points,
                            default_text: &self.mls_default_text,
                            line_color: &self.mls_line_color,
                            text_color: &self.mls_text_color,
                            description: &self.mls_description,
                            align_space: &self.mls_align_space,
                            block_color: &self.mls_block_color,
                            block_rotation: &self.mls_block_rotation,
                            block_scale_x: &self.mls_block_scale_x,
                            block_scale_y: &self.mls_block_scale_y,
                            block_scale_z: &self.mls_block_scale_z,
                            block_opts: block_opts.clone(),
                            arrow_opts: arrow_opts.clone(),
                            lt_opts: lt_opts.clone(),
                            textstyle_opts: textstyle_opts.clone(),
                            line_type_name: line_type_name.clone(),
                            arrowhead_name: arrowhead_name.clone(),
                            text_style_name: text_style_name.clone(),
                            block_content_name: block_content_name.clone(),
                            rename_active: self.style_rename.as_deref(),
                            rename_buf: &self.style_rename_buf,
                            color_open: self.mls_color_open,
                        },
                        flow,
                    )
                })
            }
            super::super::ModalKind::DimStyle => {
                let tab = &self.tabs[self.active_tab];
                let styles: Vec<String> = tab
                    .scene
                    .document
                    .dim_styles
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                let doc = &tab.scene.document;
                // Dropdown options (names must match the records exactly so the
                // selection can be resolved back to a handle on the update side).
                // The standard arrowheads, then the user arrow blocks the
                // selected style uses; not every block of the drawing. (#1820)
                let block_opts = crate::entities::arrowheads::options(
                    doc.dim_styles
                        .get(&self.dimstyle_selected)
                        .iter()
                        .flat_map(|d| [d.dimblk, d.dimblk1, d.dimblk2, d.dimldrblk])
                        .map(|h| crate::entities::arrowheads::label(doc, h))
                        .collect::<Vec<_>>()
                        .iter()
                        .map(String::as_str),
                );
                // User Arrow… offers the drawing's own blocks on demand.
                let mut block_opts = block_opts;
                block_opts.push(crate::tr!("dimstyle", "user-arrow"));
                let user_arrow_blocks: Vec<String> = doc
                    .block_records
                    .iter()
                    .filter(|record| {
                        !record.name.starts_with('*')
                            && !record.name.contains('|')
                            && !record.flags.is_xref
                            && !record.flags.is_xref_overlay
                    })
                    .map(|record| record.name.clone())
                    .collect();
                let mut lt_opts: Vec<String> = vec!["ByBlock".to_string()];
                lt_opts.extend(doc.line_types.iter().map(|lt| lt.name.clone()));
                let text_style_opts: Vec<String> = doc
                    .text_styles
                    .iter()
                    .filter(|style| !style.is_shape_file)
                    .map(|style| style.name.clone())
                    .collect();
                let text_style_fixed_height = doc
                    .text_styles
                    .get(&self.ds_dimtxsty)
                    .map(|style| style.height)
                    .filter(|height| *height > 0.0);
                let blk_name =
                    |h: codec::types::Handle| crate::entities::arrowheads::label(doc, h);
                let lt_name = |h: codec::types::Handle| -> String {
                    if h.is_null() {
                        "ByBlock".to_string()
                    } else {
                        doc.line_types
                            .iter()
                            .find(|lt| lt.handle == h)
                            .map(|lt| lt.name.clone())
                            .unwrap_or_else(|| "ByBlock".to_string())
                    }
                };
                let ds_sel = doc.dim_styles.get(&self.dimstyle_selected);
                let read_only =
                    ds_sel.is_none_or(|style| style.xref_dependent || style.name.contains('|'));
                let in_use = doc.entities().any(|entity| {
                    matches!(entity, codec::EntityType::Dimension(dimension)
                    if dimension.base().style_name.eq_ignore_ascii_case(&self.dimstyle_selected))
                });
                let compare_opts: Vec<String> = styles
                    .iter()
                    .filter(|name| !name.eq_ignore_ascii_case(&self.dimstyle_selected))
                    .cloned()
                    .collect();
                let compare_name = compare_opts
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(&self.dimstyle_compare))
                    .cloned()
                    .or_else(|| compare_opts.first().cloned())
                    .unwrap_or_default();
                let mut comparison_sections = Vec::new();
                if let (Some(a), Some(b)) = (ds_sel, doc.dim_styles.get(&compare_name)) {
                    if (
                        a.dimdle, a.dimdli, a.dimgap, a.dimclrd, a.dimlwd, a.dimsd1, a.dimsd2,
                    ) != (
                        b.dimdle, b.dimdli, b.dimgap, b.dimclrd, b.dimlwd, b.dimsd1, b.dimsd2,
                    ) || (
                        a.dimexe, a.dimexo, a.dimclre, a.dimlwe, a.dimse1, a.dimse2, a.dimfxl,
                        a.dimfxlon,
                    ) != (
                        b.dimexe, b.dimexo, b.dimclre, b.dimlwe, b.dimse1, b.dimse2, b.dimfxl,
                        b.dimfxlon,
                    ) || (a.dimltex_handle, a.dimltex1_handle, a.dimltex2_handle)
                        != (b.dimltex_handle, b.dimltex1_handle, b.dimltex2_handle)
                    {
                        comparison_sections.push(crate::i18n::translate("Lines").into_owned());
                    }
                    if (
                        a.dimasz,
                        a.dimblk,
                        a.dimblk1,
                        a.dimblk2,
                        a.dimldrblk,
                        a.dimsah,
                        a.dimcen,
                        a.dimtsz,
                    ) != (
                        b.dimasz,
                        b.dimblk,
                        b.dimblk1,
                        b.dimblk2,
                        b.dimldrblk,
                        b.dimsah,
                        b.dimcen,
                        b.dimtsz,
                    ) || (a.dimarcsym, a.dimjogang) != (b.dimarcsym, b.dimjogang)
                    {
                        comparison_sections
                            .push(crate::i18n::translate("Symbols and Arrows").into_owned());
                    }
                    if (
                        a.dimclrt,
                        a.dimtxt,
                        &a.dimtxsty,
                        a.dimjust,
                        a.dimtad,
                        a.dimtvp,
                    ) != (
                        b.dimclrt,
                        b.dimtxt,
                        &b.dimtxsty,
                        b.dimjust,
                        b.dimtad,
                        b.dimtvp,
                    ) || (
                        a.dimtih,
                        a.dimtoh,
                        a.dimtfill,
                        a.dimtfillclr,
                        a.dimtxtdirection,
                    ) != (
                        b.dimtih,
                        b.dimtoh,
                        b.dimtfill,
                        b.dimtfillclr,
                        b.dimtxtdirection,
                    ) {
                        comparison_sections.push(crate::i18n::translate("Text").into_owned());
                    }
                    if (
                        a.dimatfit,
                        a.dimtix,
                        a.dimsoxd,
                        a.dimtmove,
                        a.dimupt,
                        a.dimtofl,
                        a.dimscale,
                        a.annotative,
                    ) != (
                        b.dimatfit,
                        b.dimtix,
                        b.dimsoxd,
                        b.dimtmove,
                        b.dimupt,
                        b.dimtofl,
                        b.dimscale,
                        b.annotative,
                    ) {
                        comparison_sections.push(crate::i18n::translate("Fit").into_owned());
                    }
                    if (
                        a.dimlfac, a.dimlunit, a.dimdec, &a.dimpost, a.dimdsep, a.dimrnd, a.dimzin,
                        a.dimfrac,
                    ) != (
                        b.dimlfac, b.dimlunit, b.dimdec, &b.dimpost, b.dimdsep, b.dimrnd, b.dimzin,
                        b.dimfrac,
                    ) || (a.dimaunit, a.dimadec, a.dimazin) != (b.dimaunit, b.dimadec, b.dimazin)
                    {
                        comparison_sections
                            .push(crate::i18n::translate("Primary Units").into_owned());
                    }
                    if (
                        a.dimalt,
                        a.dimaltf,
                        a.dimaltd,
                        a.dimaltu,
                        a.dimalttd,
                        a.dimaltrnd,
                        &a.dimapost,
                        a.dimaltz,
                        a.dimalttz,
                    ) != (
                        b.dimalt,
                        b.dimaltf,
                        b.dimaltd,
                        b.dimaltu,
                        b.dimalttd,
                        b.dimaltrnd,
                        &b.dimapost,
                        b.dimaltz,
                        b.dimalttz,
                    ) {
                        comparison_sections
                            .push(crate::i18n::translate("Alternate Units").into_owned());
                    }
                    if (
                        a.dimtol, a.dimlim, a.dimtp, a.dimtm, a.dimtdec, a.dimtfac, a.dimtolj,
                        a.dimtzin,
                    ) != (
                        b.dimtol, b.dimlim, b.dimtp, b.dimtm, b.dimtdec, b.dimtfac, b.dimtolj,
                        b.dimtzin,
                    ) {
                        comparison_sections.push(crate::i18n::translate("Tolerances").into_owned());
                    }
                }
                let (
                    dimblk_name,
                    dimblk1_name,
                    dimblk2_name,
                    dimldrblk_name,
                    dimltex_name,
                    dimltex1_name,
                    dimltex2_name,
                ) = match ds_sel {
                    Some(d) => (
                        blk_name(d.dimblk),
                        blk_name(d.dimblk1),
                        blk_name(d.dimblk2),
                        blk_name(d.dimldrblk),
                        lt_name(d.dimltex_handle),
                        lt_name(d.dimltex1_handle),
                        lt_name(d.dimltex2_handle),
                    ),
                    None => Default::default(),
                };
                sized_flow(ex, 960, 680, |flow| {
                    crate::ui::style::dimstyle::view_window(
                        styles.clone(),
                        &self.dimstyle_selected,
                        &self.tabs[self.active_tab]
                            .scene
                            .document
                            .header
                            .current_dimstyle_name,
                        self.dimstyle_tab,
                        crate::ui::style::dimstyle::DimStyleValues {
                            dimdle: &self.ds_dimdle,
                            dimdli: &self.ds_dimdli,
                            dimgap: &self.ds_dimgap,
                            dimexe: &self.ds_dimexe,
                            dimexo: &self.ds_dimexo,
                            dimsd1: self.ds_dimsd1,
                            dimsd2: self.ds_dimsd2,
                            dimse1: self.ds_dimse1,
                            dimse2: self.ds_dimse2,
                            dimasz: &self.ds_dimasz,
                            dimcen: &self.ds_dimcen,
                            dimtsz: &self.ds_dimtsz,
                            dimtxt: &self.ds_dimtxt,
                            dimtxsty: &self.ds_dimtxsty,
                            dimtad: &self.ds_dimtad,
                            dimtih: self.ds_dimtih,
                            dimtoh: self.ds_dimtoh,
                            dimscale: &self.ds_dimscale,
                            dimlfac: &self.ds_dimlfac,
                            dimlunit: &self.ds_dimlunit,
                            dimdec: &self.ds_dimdec,
                            dimpost: &self.ds_dimpost,
                            dimtol: self.ds_dimtol,
                            dimlim: self.ds_dimlim,
                            dimtp: &self.ds_dimtp,
                            dimtm: &self.ds_dimtm,
                            dimtdec: &self.ds_dimtdec,
                            dimtfac: &self.ds_dimtfac,
                            annotative: self.ds_annotative,
                            dimclrd: &self.ds_dimclrd,
                            dimlwd: &self.ds_dimlwd,
                            dimclre: &self.ds_dimclre,
                            dimlwe: &self.ds_dimlwe,
                            dimfxl: &self.ds_dimfxl,
                            dimfxlon: self.ds_dimfxlon,
                            dimsah: self.ds_dimsah,
                            dimarcsym: &self.ds_dimarcsym,
                            dimjogang: &self.ds_dimjogang,
                            dimclrt: &self.ds_dimclrt,
                            dimjust: &self.ds_dimjust,
                            dimtvp: &self.ds_dimtvp,
                            dimtfill: &self.ds_dimtfill,
                            dimtfillclr: &self.ds_dimtfillclr,
                            dimtxtdirection: self.ds_dimtxtdirection,
                            dimatfit: &self.ds_dimatfit,
                            dimtix: self.ds_dimtix,
                            dimsoxd: self.ds_dimsoxd,
                            dimtmove: &self.ds_dimtmove,
                            dimupt: self.ds_dimupt,
                            dimtofl: self.ds_dimtofl,
                            dimdsep: &self.ds_dimdsep,
                            dimrnd: &self.ds_dimrnd,
                            dimzin: &self.ds_dimzin,
                            dimfrac: &self.ds_dimfrac,
                            dimaunit: &self.ds_dimaunit,
                            dimadec: &self.ds_dimadec,
                            dimazin: &self.ds_dimazin,
                            dimalt: self.ds_dimalt,
                            dimaltf: &self.ds_dimaltf,
                            dimaltd: &self.ds_dimaltd,
                            dimaltu: &self.ds_dimaltu,
                            dimalttd: &self.ds_dimalttd,
                            dimaltrnd: &self.ds_dimaltrnd,
                            dimapost: &self.ds_dimapost,
                            dimaltz: &self.ds_dimaltz,
                            dimalttz: &self.ds_dimalttz,
                            dimtolj: &self.ds_dimtolj,
                            dimtzin: &self.ds_dimtzin,
                            dimblk_name: dimblk_name.clone(),
                            dimblk1_name: dimblk1_name.clone(),
                            dimblk2_name: dimblk2_name.clone(),
                            dimldrblk_name: dimldrblk_name.clone(),
                            dimltex_name: dimltex_name.clone(),
                            dimltex1_name: dimltex1_name.clone(),
                            dimltex2_name: dimltex2_name.clone(),
                            block_opts: block_opts.clone(),
                            user_arrow_blocks: user_arrow_blocks.clone(),
                            user_arrow_field: self.dimstyle_user_arrow,
                            lt_opts: lt_opts.clone(),
                            text_style_opts: text_style_opts.clone(),
                            text_style_fixed_height,
                            compare_name: compare_name.clone(),
                            compare_opts: compare_opts.clone(),
                            comparison_sections: comparison_sections.clone(),
                            read_only,
                            in_use,
                            color_open: self.ds_color_open.clone(),
                        },
                        self.style_rename.as_deref(),
                        &self.style_rename_buf,
                        flow,
                    )
                })
            }
            super::super::ModalKind::AssocPrompt => automatic_flow(ex, default_assoc_dialog_window),
            super::super::ModalKind::DonationPrompt => {
                sized_flow(ex, 540, 360, donation_dialog_window)
            }
            super::super::ModalKind::GpuWarning => sized_flow(ex, 520, 400, |flow| {
                gpu_warning_window(&self.gpu_status, flow)
            }),
            super::super::ModalKind::AecDropWarning => {
                let src_label = self
                    .tabs
                    .get(self.active_tab)
                    .map(|t| {
                        let is_dxf = t
                            .current_path
                            .as_ref()
                            .and_then(|path| path.extension())
                            .and_then(|extension| extension.to_str())
                            .map(|extension| extension.eq_ignore_ascii_case("dxf"))
                            .unwrap_or(false);
                        let version = if is_dxf {
                            t.scene.document.version
                        } else {
                            t.scene
                                .document
                                .dwg_source_version
                                .unwrap_or(t.scene.document.version)
                        };
                        crate::io::format_for_version(version, is_dxf)
                    })
                    .unwrap_or_else(|| "DWG".to_string());
                automatic_flow(ex, |flow| {
                    aec_drop_dialog_window(
                        self.aec_drop_count,
                        &self.save_dialog_format,
                        &src_label,
                        flow,
                    )
                })
            }
            super::super::ModalKind::ConversionLoss => {
                let target = self
                    .pending_conversion_save
                    .as_ref()
                    .map(super::super::PendingConversionSave::format_name)
                    .unwrap_or_default();
                sized_flow(ex, 640, 480, |flow| {
                    conversion_loss_dialog_window(&self.conversion_losses, &target, flow)
                })
            }
            #[cfg(not(target_arch = "wasm32"))]
            super::super::ModalKind::FileInUse => {
                let (path, error) = self
                    .pending_save_failure
                    .as_ref()
                    .map(|failure| (failure.path.display().to_string(), failure.error.clone()))
                    .unwrap_or_default();
                automatic_flow(ex, |flow| file_in_use_dialog_window(&path, &error, flow))
            }
            #[cfg(not(target_arch = "wasm32"))]
            super::super::ModalKind::ExternalChange => {
                let path = self
                    .pending_external_change
                    .as_ref()
                    .map(|conflict| conflict.path.display().to_string())
                    .unwrap_or_default();
                automatic_flow(ex, |flow| external_change_dialog_window(&path, flow))
            }
            super::super::ModalKind::LayerDeleteWarning => {
                let (names, count) = self
                    .layer_delete_pending
                    .clone()
                    .unwrap_or_else(|| (Vec::new(), 0));
                automatic_flow(ex, |flow| layer_delete_warning_window(&names, count, flow))
            }
            super::super::ModalKind::Unsaved => {
                let tab_name = match &self.pending_close {
                    Some(super::super::PendingClose::Tab(tab_id)) => self
                        .tabs
                        .iter()
                        .find(|t| t.id == *tab_id)
                        .map(|t| t.tab_display_name())
                        .unwrap_or_default(),
                    Some(super::super::PendingClose::Quit) => self
                        .tabs
                        .iter()
                        .find(|t| t.dirty)
                        .map(|t| t.tab_display_name())
                        .unwrap_or_default(),
                    None => String::new(),
                };
                automatic_flow(ex, |flow| {
                    unsaved_changes_dialog_window(&tab_name, flow, false)
                })
            }
            super::super::ModalKind::BlockEditChanges => {
                let name = self
                    .pending_block_close
                    .as_ref()
                    .and_then(|(id, _)| self.tabs.iter().find(|tab| tab.id == *id))
                    .and_then(|tab| tab.active_block_edit_session())
                    .map(|session| session.block_name.clone())
                    .unwrap_or_default();
                automatic_flow(ex, |flow| unsaved_changes_dialog_window(&name, flow, true))
            }
            super::super::ModalKind::PointStyle => sized_flow(ex, 360, 470, |flow| {
                crate::ui::style::point_style::view_window(
                    self.point_mode_buf.unwrap_or_else(|| {
                        self.tabs[self.active_tab]
                            .scene
                            .document
                            .header
                            .point_display_mode
                    }),
                    self.point_size_relative,
                    &self.point_size_buf,
                    flow,
                )
            }),
            super::super::ModalKind::Hyperlink => sized_flow(ex, 520, 380, |flow| {
                crate::ui::window::hyperlink_dialog::view(
                    &self.hyperlink_editor_url,
                    &self.hyperlink_editor_description,
                    self.hyperlink_editor_mixed,
                    &self.hyperlink_options,
                    flow,
                )
            }),
            super::super::ModalKind::AttributeEditor => {
                let doc = &self.tabs[self.active_tab].scene.document;
                let layers: Vec<String> = doc.layers.iter().map(|l| l.name.clone()).collect();
                let mut linetypes: Vec<String> = vec!["ByLayer".to_string()];
                linetypes.extend(
                    doc.line_types
                        .iter()
                        .map(|lt| lt.name.clone())
                        .filter(|n| !n.is_empty() && n != "ByLayer"),
                );
                let styles: Vec<String> = doc
                    .text_styles
                    .iter()
                    .map(|s| s.name.trim().to_string())
                    .filter(|n| !n.is_empty())
                    .collect();
                sized_flow(ex, 640, 500, |flow| {
                    crate::ui::window::attribute_editor::view_window(
                        &self.attr_editor_block,
                        &self.attr_editor_rows,
                        self.attr_editor_selected,
                        self.attr_editor_tab,
                        layers.clone(),
                        linetypes.clone(),
                        styles.clone(),
                        flow,
                    )
                })
            }
            super::super::ModalKind::SaveDialog => automatic_flow(ex, |flow| {
                save_as_dialog_window(&self.save_dialog_filename, &self.save_dialog_format, flow)
            }),
            super::super::ModalKind::MissingFonts => {
                let fonts = self.missing_fonts.as_ref()?;
                let font_source = &self.font_source_input;
                automatic_flow(ex, |flow| {
                    crate::ui::window::missing_fonts::view_window(
                        fonts,
                        &font_source,
                        self.missing_fonts_downloading,
                        flow,
                    )
                })
            }
            super::super::ModalKind::Recovery => {
                let report = self.recovery_report.as_ref()?;
                sized_flow(ex, 680, 460, |flow| {
                    crate::ui::window::recovery::view_window(
                        report,
                        self.pending_opens.is_empty(),
                        flow,
                    )
                })
            }
            super::super::ModalKind::RecoveryPrompt => {
                let opening = self.opening.as_ref()?;
                let error = opening.recovery_error.as_deref()?;
                automatic_flow(ex, |flow| {
                    crate::ui::window::recovery::view_prompt(&opening.name, error, flow)
                })
            }
            #[cfg(target_arch = "wasm32")]
            super::super::ModalKind::AutosaveRestore => {
                let name = self
                    .web_autosave_pending
                    .first()
                    .map(|entry| entry.name.clone())
                    .unwrap_or_default();
                let pending = self.web_autosave_pending.len();
                // What the crashed session had open, when it remembered.
                let session_drawings = self
                    .web_session
                    .as_ref()
                    .map(|session| session.tabs.len())
                    .filter(|count| *count > 0);
                automatic_flow(ex, |flow| {
                    autosave_restore_dialog_window(&name, pending, session_drawings, flow)
                })
            }
            super::super::ModalKind::PluginDialog => {
                let state = self.plugin_dialog_state.as_ref()?;
                automatic_flow(ex, |_flow| state.view())
            }

        })
    }

    fn shortcuts_modal_content<'s>(&'s self, ex: iced::Vector) -> Element<'s, Message> {
        // Keys claimed by two rows — the cells turn red and a persistent
        // warning names the command already using each key.
        let (duplicate_set, duplicate_conflicts) = duplicate_rows(
            &self.shortcut_editor_rows,
            crate::app::shortcuts::normalize_key,
        );
        // Commands the dispatcher can't run: not a registered command,
        // plugin command, alias, or input action.
        let valid: rustc_hash::FxHashSet<String> =
            crate::command::all_registered_command_names()
                .into_iter()
                .map(str::to_uppercase)
                .chain(
                    self.command_line
                        .dynamic_commands
                        .iter()
                        .map(|cmd| cmd.to_uppercase()),
                )
                .chain(self.command_aliases.keys().cloned())
                .chain(
                    crate::app::shortcuts::INPUT_ACTIONS
                        .iter()
                        .map(|action| action.to_string()),
                )
                .collect();
        let unknown_set: rustc_hash::FxHashSet<String> = self
            .shortcut_editor_rows
            .iter()
            .filter_map(|(_, command)| {
                let command = command.trim();
                (!command.is_empty() && !valid.contains(command)).then(|| command.to_string())
            })
            .collect();
        sized_flow(ex, 720, 520, |flow| {
            crate::ui::window::shortcuts::view_window(
                &self.shortcut_editor_rows,
                self.shortcut_capture_row,
                self.shortcut_pending_add,
                self.shortcut_reset_confirm,
                &duplicate_set,
                &duplicate_conflicts,
                &unknown_set,
                self.shortcut_close_confirm,
                flow,
            )
        })
    }

    fn aliases_modal_content<'s>(&'s self, ex: iced::Vector) -> Element<'s, Message> {
        // Aliases claimed by two rows — the cells turn red and a persistent
        // warning names the command already using each alias.
        let (duplicate_set, duplicate_conflicts) = duplicate_rows(
            &self.alias_editor_rows,
            |alias| alias.trim().to_uppercase(),
        );
        // Commands the dispatcher can't run: not a registered command,
        // plugin command, or input action.
        let valid: rustc_hash::FxHashSet<String> =
            crate::command::all_registered_command_names()
                .into_iter()
                .map(str::to_uppercase)
                .chain(
                    self.command_line
                        .dynamic_commands
                        .iter()
                        .map(|cmd| cmd.to_uppercase()),
                )
                .chain(
                    crate::app::shortcuts::INPUT_ACTIONS
                        .iter()
                        .map(|action| action.to_string()),
                )
                .collect();
        let unknown_set: rustc_hash::FxHashSet<String> = self
            .alias_editor_rows
            .iter()
            .filter_map(|(_, command)| {
                let command = command.trim();
                (!command.is_empty() && !valid.contains(command)).then(|| command.to_string())
            })
            .collect();
        sized_flow(ex, 480, 520, |flow| {
            crate::ui::window::alias_editor::view_window(
                &self.alias_editor_rows,
                &self.alias_import_report,
                self.alias_pending_add,
                self.alias_reset_confirm,
                &duplicate_set,
                &duplicate_conflicts,
                &unknown_set,
                self.alias_close_confirm,
                flow,
            )
        })
    }
}

fn duplicate_rows(
    rows: &[(String, String)],
    normalize: impl Fn(&str) -> String,
) -> (rustc_hash::FxHashSet<String>, Vec<(String, String)>) {
    let mut grouped: std::collections::BTreeMap<String, Vec<&(String, String)>> =
        std::collections::BTreeMap::new();
    for row in rows {
        let key = normalize(&row.0);
        if !key.is_empty() {
            grouped.entry(key).or_default().push(row);
        }
    }

    let duplicates: Vec<_> = grouped
        .into_iter()
        .filter(|(_, rows)| rows.len() > 1)
        .map(|(key, rows)| {
            // Report the established binding: the first duplicate row with a
            // non-empty command, matching both editors' previous behavior.
            let command = rows
                .iter()
                .find(|(_, command)| !command.is_empty())
                .map(|(_, command)| command.clone())
                .unwrap_or_default();
            (key, command)
        })
        .collect();
    let duplicate_set = duplicates.iter().map(|(key, _)| key.clone()).collect();
    (duplicate_set, duplicates)
}

/// Build a dialog with an explicit initial size cap. Use this for dense forms
/// and tables that contain their own scrollable regions; the cap keeps their
/// intrinsic first layout bounded, while `extra` records user resizing.
fn sized_flow<'a>(
    extra: iced::Vector,
    max_width: u16,
    max_height: u16,
    mut build: impl FnMut(crate::ui::modal::ModalSizing) -> Element<'a, Message>,
) -> Element<'a, Message> {
    crate::ui::modal::intrinsic(
        build(crate::ui::modal::ModalSizing::INTRINSIC),
        build(crate::ui::modal::ModalSizing::FILL),
        iced::Size::new(max_width as f32, max_height as f32),
        extra,
    )
}

/// Build a dialog at its content's natural size. Use this for compact prompts
/// and small forms whose contents should determine the initial frame.
fn automatic_flow<'a>(
    extra: iced::Vector,
    mut build: impl FnMut(crate::ui::modal::ModalSizing) -> Element<'a, Message>,
) -> Element<'a, Message> {
    crate::ui::modal::intrinsic(
        build(crate::ui::modal::ModalSizing::INTRINSIC),
        build(crate::ui::modal::ModalSizing::FILL),
        iced::Size::new(f32::INFINITY, f32::INFINITY),
        extra,
    )
}

fn dialog_button<'a>(
    label: impl Into<Cow<'a, str>>,
    message: Message,
    style: fn(&Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    crate::ui::style::form::dialog_button_styled(label.into(), message, style).into()
}

fn dialog_body_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.base.color)),
        text_color: Some(palette.background.base.text),
        ..Default::default()
    }
}

fn dialog_muted_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

/// Compact Save-As options dialog: pick the format/version and a default file
/// name. The destination folder and overwrite confirmation come from the
/// native OS save dialog (native) or the browser download (web) that follows.
fn save_as_dialog_window<'a>(
    filename: &'a str,
    format: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let options = crate::io::save_format_options();
    let sel_fmt = options.iter().find(|s| *s == format).cloned();
    let label = |s: Cow<'static, str>| text(s).size(11).style(dialog_muted_text_style);
    let field_width = if matches!(sizing.width, iced::Length::Fill) {
        Fill
    } else {
        iced::Length::Shrink
    };

    let mut items: Vec<Element<'a, Message>> = Vec::new();
    items.push(text(t!("Save Drawing As")).size(14).into());
    items.push(Space::new().height(12).into());

    // Web has no native file dialog, so the file name is typed here. On native
    // the OS save dialog collects the name, so this field is omitted.
    #[cfg(target_arch = "wasm32")]
    {
        items.push(
            row![
                label(t!("File name:")).width(70),
                iced::widget::text_input("drawing.dwg", filename)
                    .on_input(Message::SaveDialogFilenameChanged)
                    .size(13)
                    .padding([5, 8])
                    .width(field_width),
            ]
            .align_y(iced::Alignment::Center)
            .spacing(6)
            .width(sizing.width)
            .into(),
        );
        items.push(Space::new().height(8).into());
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = filename;

    items.push(
        row![
            label(t!("Format:")).width(70),
            iced::widget::pick_list(sel_fmt, options, |value| value.clone())
                .on_select(Message::SaveDialogFormatChanged)
            .width(field_width),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(6)
        .width(sizing.width)
        .into(),
    );
    items.push(Space::new().height(16).into());
    items.push(
        row![
            Space::new().width(Fit),
            dialog_button(
                t!("Save as..."),
                Message::SaveDialogConfirm,
                button::primary
            ),
            Space::new().width(8),
            dialog_button(t!("Cancel"), Message::SaveDialogCancel, button::secondary),
        ]
        .into(),
    );

    let body = column(items)
        .spacing(0)
        .width(sizing.width)
        .height(sizing.height);

    container(body)
        .style(dialog_body_style)
        .padding([14, 16])
        .width(sizing.width)
        .height(sizing.height)
        .into()
}

fn unsaved_changes_dialog_window(
    name: &str,
    sizing: crate::ui::modal::ModalSizing,
    block_edit: bool,
) -> Element<'static, Message> {
    container(
        column![
            text(if block_edit {
                crate::tf!("Save changes to {}?", name).into_owned()
            } else {
                name.to_owned()
            })
            .size(14),
            iced::widget::Space::new().height(4),
            text(if block_edit {
                String::new()
            } else {
                crate::tr!("modal", "unsaved-save-prompt")
            })
            .size(12)
            .style(dialog_muted_text_style),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Save"),
                    if block_edit {
                        Message::BlockEditChangesSave
                    } else {
                        Message::UnsavedDialogSave
                    },
                    button::primary
                ),
                iced::widget::Space::new().width(8),
                dialog_button(
                    t!("Discard"),
                    if block_edit {
                        Message::BlockEditChangesDiscard
                    } else {
                        Message::UnsavedDialogDiscard
                    },
                    button::danger
                ),
                iced::widget::Space::new().width(8),
                dialog_button(
                    t!("Cancel"),
                    if block_edit {
                        Message::BlockEditChangesCancel
                    } else {
                        Message::UnsavedDialogCancel
                    },
                    button::secondary
                ),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

/// Web crash-recovery prompt: an autosave snapshot of `name` is newer than
/// the drawing's last save. Restore opens it as a recovered tab; Discard
/// deletes the snapshot. With more than one snapshot still pending, "Restore
/// all" puts the whole remaining list through the same guarded open queue.
/// When the crashed session left a tab list behind, a muted line says how
/// much it had open. Mirrors the unsaved-changes dialog's layout.
#[cfg(target_arch = "wasm32")]
fn autosave_restore_dialog_window(
    name: &str,
    pending: usize,
    session_drawings: Option<usize>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let mut body = column![text(crate::tf!(
        "\"{name}\" has an autosave copy newer than the last save."
    ))
    .size(14)]
    .spacing(0);
    if let Some(count) = session_drawings {
        body = body.push(iced::widget::Space::new().height(4)).push(
            text(crate::tf!(
                "Session had {count} open drawing(s).",
                count = count
            ))
            .size(12)
            .style(dialog_muted_text_style),
        );
    }
    body = body
        .push(iced::widget::Space::new().height(4))
        .push(
            text(t!("Restore it as a recovered tab, or discard the copy."))
                .size(12)
                .style(dialog_muted_text_style),
        )
        .push(iced::widget::Space::new().height(20));
    let mut buttons = row![dialog_button(
        t!("Restore"),
        Message::WebAutosaveRestore,
        button::primary
    )];
    // Only worth its width once there is a batch to take in one click.
    if pending > 1 {
        buttons = buttons
            .push(iced::widget::Space::new().width(8))
            .push(dialog_button(
                t!("Restore all"),
                Message::WebAutosaveRestoreAll,
                button::secondary,
            ));
    }
    buttons = buttons
        .push(iced::widget::Space::new().width(8))
        .push(dialog_button(
            t!("Discard"),
            Message::WebAutosaveDiscard,
            button::danger,
        ));
    body = body.push(buttons);
    container(body)
        .style(dialog_body_style)
        .center_x(sizing.width)
        .center_y(sizing.height)
        .padding([24, 28])
        .into()
}

#[cfg(not(target_arch = "wasm32"))]
fn file_in_use_dialog_window(
    path: &str,
    error: &str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| t!("Drawing").into_owned());
    let heading = t!(
        "\"%{file_name}\" could not be saved.",
        file_name = file_name
    );
    let path_line = t!("Path: %{path}", path = path);
    let details = t!("Details: %{error}", error = error);

    container(
        column![
            text(heading).size(14),
            Space::new().height(8),
            text(t!(
                "The file is open or being used by another application. Close it there and retry, or save this drawing under a different name."
            ))
            .size(13)
            .width(Fit),
            Space::new().height(12),
            text(path_line).size(11).style(dialog_muted_text_style).width(Fit),
            Space::new().height(4),
            text(details).size(11).style(dialog_muted_text_style).width(Fit),
            Space::new().height(18),
            row![
                dialog_button(
                    t!("Retry"),
                    Message::SaveFileInUseRetry,
                    button::primary
                ),
                Space::new().width(8),
                dialog_button(
                    t!("Save As"),
                    Message::SaveFileInUseSaveAs,
                    button::secondary
                ),
                Space::new().width(8),
                dialog_button(
                    t!("Cancel"),
                    Message::SaveFileInUseCancel,
                    button::secondary
                ),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .padding([18, 20])
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

#[cfg(not(target_arch = "wasm32"))]
fn external_change_dialog_window(
    path: &str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| t!("Drawing").into_owned());
    let heading = t!(
        "\"%{file_name}\" was changed by another application.",
        file_name = file_name
    );
    let path_line = t!("Path: %{path}", path = path);

    container(
        column![
            text(heading).size(14),
            Space::new().height(8),
            text(t!(
                "Saving now could destroy those external changes. Reload the disk copy, save your local work elsewhere, or explicitly overwrite it."
            ))
            .size(13)
            .width(Fit),
            Space::new().height(12),
            text(path_line).size(11).style(dialog_muted_text_style).width(Fit),
            Space::new().height(18),
            row![
                dialog_button(
                    t!("Reload from Disk"),
                    Message::ExternalChangeReload,
                    button::primary
                ),
                Space::new().width(8),
                dialog_button(
                    t!("Save As"),
                    Message::ExternalChangeSaveAs,
                    button::secondary
                ),
                Space::new().width(8),
                dialog_button(
                    t!("Overwrite"),
                    Message::ExternalChangeOverwrite,
                    button::danger
                ),
                Space::new().width(8),
                dialog_button(
                    t!("Cancel"),
                    Message::ExternalChangeCancel,
                    button::secondary
                ),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .padding([18, 20])
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

/// Warning shown before a lossy Save-As: the drawing carries unsupported
/// (AEC / application) objects that survive only as verbatim source-version
/// bytes, so saving to a different version or to DXF would drop them. Offers to
/// save in the source version (keep them) or proceed (drop them).
fn conversion_loss_dialog_window<'a>(
    losses: &'a [String],
    target: &str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let body_text = t!(
        "Saving as \"%{target}\" cannot keep the following, so it will be lost:",
        target = target,
    );
    // Leave room for the scrollbar and its rounded scroll offset at the end.
    let mut lines = column![].spacing(4).padding(iced::Padding {
        right: 14.0,
        bottom: 1.0,
        ..iced::Padding::ZERO
    });
    for loss in losses {
        lines = lines.push(
            text(format!("• {loss}"))
                .size(12)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        );
    }

    container(
        column![
            text(body_text).size(13),
            Space::new().height(8),
            iced::widget::scrollable(lines)
                .width(sizing.width)
                .height(sizing.height),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Save anyway"),
                    Message::ConversionLossProceed,
                    button::warning
                ),
                iced::widget::Space::new().width(8),
                dialog_button(t!("Back"), Message::ConversionLossBack, button::secondary),
            ],
        ]
        .spacing(0)
        .width(sizing.width)
        .height(sizing.height),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

fn aec_drop_dialog_window(
    count: usize,
    target: &str,
    src_version: &str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let body_text = t!(
        "This drawing contains %{count} AEC/Civil objects that \"%{target}\" cannot store, so they will not be saved.\n\nTo keep them, save in the source version (%{src_version}).",
        count = count,
        target = target,
        src_version = src_version,
    );

    container(
        column![
            text(body_text).size(13),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Save in source version"),
                    Message::AecDropSameVersion,
                    button::primary
                ),
                iced::widget::Space::new().width(8),
                dialog_button(t!("Save anyway"), Message::AecDropProceed, button::warning),
                iced::widget::Space::new().width(8),
                dialog_button(t!("Back"), Message::AecDropBack, button::secondary),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

/// Confirmation shown when the chosen Save-As filename already exists in the
/// target folder. "Replace" overwrites; "Cancel" returns to the Save dialog.
/// Confirm deleting layer(s) that still have objects on them. "Delete Objects"
/// erases them and removes the layers; "Cancel" leaves everything.
fn layer_delete_warning_window(
    names: &[String],
    count: usize,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let obj = if count == 1 {
        t!("object")
    } else {
        t!("objects")
    };
    let has = if names.len() == 1 {
        t!("has")
    } else {
        t!("hold")
    };
    let those = if count == 1 {
        t!("that object")
    } else {
        t!("those objects")
    };
    let subject = if names.len() == 1 {
        t!("Layer \"%{name}\"", name = names[0]).into_owned()
    } else {
        t!("%{count} selected layers", count = names.len()).into_owned()
    };
    let body_text = t!(
        "%{subject} still %{has} %{count} %{obj}.\n\nDeleting will also remove %{those} from the drawing. Continue?",
        subject = subject,
        has = has,
        count = count,
        obj = obj,
        those = those,
    );

    container(
        column![
            text(body_text).size(13),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Delete Objects"),
                    Message::LayerDeleteConfirm,
                    button::danger
                ),
                iced::widget::Space::new().width(8),
                dialog_button(t!("Cancel"), Message::CloseModal, button::secondary),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

/// What the graphics verdict means and what usually fixes it. Two
/// situations share the dialog — a software rasterizer (slow, but drawing)
/// and no renderer at all (a blank viewport) — and the remedy hint is per
/// platform. "OK" closes it for this session; the status-bar pill brings it
/// back. "Don't show again" silences this verdict only, so a different
/// failure on the same machine still prompts.
fn gpu_warning_window(
    status: &crate::scene::pipeline::GpuStatus,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    use crate::scene::pipeline::GpuStatus;
    let (headline, consequences) = match status {
        GpuStatus::Software(adapter) => (
            crate::tr!("gpu", "software-headline", adapter = adapter.name.clone()),
            Some(crate::tr!("gpu", "software-consequences")),
        ),
        _ => (crate::tr!("gpu", "no-renderer-headline"), None),
    };
    let mut body = column![text(headline).size(13)].spacing(8);
    if let Some(consequences) = consequences {
        body = body.push(text(consequences).size(13));
    }
    body = body.push(
        text(crate::app::startup::gpu_platform_hint())
            .size(13)
            .style(dialog_muted_text_style),
    );

    container(
        column![
            body,
            Space::new().height(20),
            row![
                dialog_button(t!("OK"), Message::CloseModal, button::primary),
                Space::new().width(8),
                dialog_button(
                    crate::tr!("gpu", "silence"),
                    Message::GpuWarningSilence,
                    button::secondary
                ),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

fn donation_dialog_window(sizing: crate::ui::modal::ModalSizing) -> Element<'static, Message> {
    container(
        column![
            text(crate::tr!("donation", "heading")).size(18),
            row![
                text(crate::tr!("donation", "body")).size(14).width(Fill),
                crate::ui::icons::themed(crate::ui::icons::HEART, 52.0),
            ]
            .spacing(20)
            .align_y(iced::Center),
            row![
                Space::new().width(Fill),
                dialog_button(
                    crate::tr!("start", "donate"),
                    Message::DonationPromptDonate,
                    button::primary,
                ),
                dialog_button(
                    crate::tr!("donation", "decline"),
                    Message::CloseModal,
                    button::secondary,
                ),
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(18)
        .width(sizing.width),
    )
    .style(dialog_body_style)
    .padding([24, 28])
    .into()
}

/// One-time default application prompt.
fn default_assoc_dialog_window(sizing: crate::ui::modal::ModalSizing) -> Element<'static, Message> {
    container(
        column![
            text(t!("Make Open CAD Studio your default CAD app?"))
                .size(15),
            iced::widget::Space::new().height(10),
            text(t!("Open .dwg and .dxf drawings in Open CAD Studio by default. You can change this later in your system settings."))
                .size(12)
                .style(dialog_muted_text_style),
            iced::widget::Space::new().height(22),
            row![
                iced::widget::Space::new().width(Fit),
                dialog_button(t!("Not now"), Message::AssocPromptNo, button::secondary),
                iced::widget::Space::new().width(8),
                dialog_button(
                    t!("Yes, set as default"),
                    Message::AssocPromptYes,
                    button::primary
                ),
            ]
            .align_y(iced::Center),
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}
