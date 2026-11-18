//! The PDF dialogs' state changes: Attach PDF Underlay, Underlay Layers,
//! PDF Import Settings and Import PDF.

use super::*;
use crate::io::xref_model::Pathtype;
use crate::modules::insert::pdf_import::{self, PdfFileImport};
use crate::ui::window::pdf_dialogs::{
    click_page, item_thumbs, page_thumbs, LayerTarget, PdfAttachState, PdfDialogMsg,
    PdfImportFileRead, PdfImportFileState, PointCloudAttachState, RotationChoice,
    UnderlayAttachRead, UnderlayLayersState,
};

/// "7.8740 × 3.9369" (inches) for a page.
fn page_size_text(path: &str, page: &str) -> String {
    crate::scene::model::pdf_raster::page_size_inches(path, page)
        .map(|(w, h)| format!("{w:.4} × {h:.4}"))
        .unwrap_or_default()
}

/// The page's (PDF, inches) or the sheet's / model's size.
fn item_size_text(kind: document::entities::UnderlayType, path: &str, item: &str) -> String {
    if kind == document::entities::UnderlayType::Pdf {
        return page_size_text(path, item);
    }
    crate::scene::model::underlay_vector::sheet(kind, path, item)
        .map(|s| {
            format!(
                "{:.4} × {:.4}",
                s.rect[2] - s.rect[0],
                s.rect[3] - s.rect[1]
            )
        })
        .unwrap_or_default()
}

fn sub_per_master(kind: document::entities::UnderlayType, path: &str, item: &str) -> f64 {
    crate::scene::model::underlay_vector::sheet(kind, path, item)
        .map(|s| s.sub_per_master)
        .filter(|v| *v > 0.0)
        .unwrap_or(1.0)
}

fn file_stem(path: &str) -> String {
    std::path::Path::new(&path.replace('\\', "/"))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl OpenCADStudio {
    /// The path an attached file is stored with: full, relative to the saved
    /// drawing (the full path until it is saved), or the file name alone.
    fn pdf_stored_path(&self, path: &str, path_type: Pathtype) -> String {
        match path_type {
            Pathtype::Full => path.to_string(),
            Pathtype::Relative => self.tabs[self.active_tab]
                .current_path
                .as_deref()
                .and_then(|host| {
                    crate::io::xref_model::to_pathtype_result(path, host, Pathtype::Relative).ok()
                })
                .unwrap_or_else(|| path.to_string()),
            Pathtype::None => std::path::Path::new(&path.replace('\\', "/"))
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string()),
        }
    }

    /// PDFATTACH after the file is chosen, with its bytes registered; an
    /// encrypted file asks for its password first.
    pub(in crate::app) fn open_pdf_attach_dialog(&mut self, path: &str) -> Task<Message> {
        self.read_underlay_for_attach(document::entities::UnderlayType::Pdf, path, false)
    }

    /// PDFATTACH / DWFATTACH / DGNATTACH after the file is chosen: the file
    /// is read off the UI thread (from the disk first when `fresh`), and
    /// the Attach window opens when the read ends. A panic in the reader
    /// becomes an error on the command line.
    pub(in crate::app) fn read_underlay_for_attach(
        &mut self,
        kind: document::entities::UnderlayType,
        path: &str,
        fresh: bool,
    ) -> Task<Message> {
        self.read_underlay_for_attach_with(kind, path, fresh, read_for_attach_window)
    }

    /// [`Self::read_underlay_for_attach`] with the reader given, so a test
    /// can give one that panics.
    fn read_underlay_for_attach_with(
        &mut self,
        kind: document::entities::UnderlayType,
        path: &str,
        fresh: bool,
        reader: fn(document::entities::UnderlayType, &str, bool) -> UnderlayAttachRead,
    ) -> Task<Message> {
        self.command_line.push_info(&format!(
            "Reading {}…",
            crate::entities::underlay::display_path(path)
        ));
        let context = attach_context(kind);
        let path = path.to_string();
        let worker_path = path.clone();
        crate::app::update::background_task(
            context,
            move || std::sync::Arc::new(reader(kind, &worker_path, fresh)),
            move |read| Message::PdfDialog(PdfDialogMsg::AttachRead(kind, path, read)),
        )
    }

    /// The end of the read for the Attach window. The window takes the
    /// place of its own older copy (Browse); if a different window opened
    /// during the read, it does not open.
    fn finish_underlay_attach_read(
        &mut self,
        kind: document::entities::UnderlayType,
        path: &str,
        read: &UnderlayAttachRead,
    ) -> Task<Message> {
        if self
            .active_modal
            .is_some_and(|modal| modal != crate::app::ModalKind::PdfAttach)
        {
            self.command_line.push_error(
                "Attach underlay cancelled: a different window opened while the file was read.",
            );
            return Task::none();
        }
        self.open_attach_window(kind, path, read)
    }

    /// The Attach window for a file already read, or the password window,
    /// or the error of the read.
    fn open_attach_window(
        &mut self,
        kind: document::entities::UnderlayType,
        path: &str,
        read: &UnderlayAttachRead,
    ) -> Task<Message> {
        use crate::ui::window::pdf_password_dialog::PdfPasswordThen;
        let (pages, sizes, units) = match read {
            UnderlayAttachRead::Failed(e) => {
                self.command_line
                    .push_error(crate::tf!("PDFATTACH: {e}").as_ref());
                return Task::none();
            }
            UnderlayAttachRead::NoItems => {
                self.command_line.push_error(&format!(
                    "{} not found.",
                    crate::entities::underlay::display_path(path)
                ));
                return Task::none();
            }
            UnderlayAttachRead::Locked { stage, remembered } => {
                return self.open_pdf_password(path, *stage, *remembered, PdfPasswordThen::Attach);
            }
            UnderlayAttachRead::Items {
                pages,
                sizes,
                units,
                remembered,
            } => {
                if *remembered {
                    self.command_line
                        .push_output(&crate::scene::model::pdf_password::remembered_notice(path));
                }
                (pages, sizes, units)
            }
        };
        let i = self.active_tab;
        let mut existing: Vec<(String, String)> = Vec::new();
        for object in self.tabs[i].scene.document.objects.values() {
            if let document::objects::ObjectType::UnderlayDefinition(def) = object {
                if def.underlay_type != kind {
                    continue;
                }
                let name = file_stem(&def.file_path);
                if !existing.iter().any(|(n, _)| *n == name) {
                    existing.push((name, def.file_path.clone()));
                }
            }
        }
        let mut state = PdfAttachState::new(existing);
        state.kind = kind;
        self.set_pdf_attach_file(
            &mut state,
            path,
            pages.clone(),
            sizes.clone(),
            units.clone(),
        );
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        Self::refresh_pdf_attach_geopdf(&self.tabs[i].scene, plane, &mut state);
        self.pdf_attach = Some(state);
        self.active_modal = Some(crate::app::ModalKind::PdfAttach);
        Task::none()
    }

    /// A file chosen in the Attach window's name list: it is read off the
    /// UI thread, and the window shows it when the read ends
    /// ([`Self::finish_attach_name_read`]). The window stays open with its
    /// options.
    fn read_attach_name_with(
        &mut self,
        kind: document::entities::UnderlayType,
        path: &str,
        reader: fn(document::entities::UnderlayType, &str, bool) -> UnderlayAttachRead,
    ) -> Task<Message> {
        self.command_line.push_info(&format!(
            "Reading {}…",
            crate::entities::underlay::display_path(path)
        ));
        let path = path.to_string();
        let worker_path = path.clone();
        crate::app::update::background_task(
            attach_context(kind),
            move || std::sync::Arc::new(reader(kind, &worker_path, false)),
            move |read| Message::PdfDialog(PdfDialogMsg::AttachNameRead(path, read)),
        )
    }

    /// The end of the read of a file chosen in the name list: the window
    /// shows it, if the window is still open and that file is still the
    /// one chosen. A file with no page, sheet or model, or one that needs
    /// a password, shows no pages, as before.
    fn finish_attach_name_read(&mut self, path: &str, read: &UnderlayAttachRead) {
        if self.active_modal != Some(crate::app::ModalKind::PdfAttach) {
            return;
        }
        let Some(mut state) = self.pdf_attach.take() else {
            return;
        };
        if state.pending_path.as_deref() != Some(path) {
            self.pdf_attach = Some(state);
            return;
        }
        state.pending_path = None;
        let (pages, sizes, units) = match read {
            UnderlayAttachRead::Items {
                pages,
                sizes,
                units,
                ..
            } => (pages.clone(), sizes.clone(), units.clone()),
            _ => (Vec::new(), Vec::new(), Vec::new()),
        };
        self.set_pdf_attach_file(&mut state, path, pages, sizes, units);
        let i = self.active_tab;
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        Self::refresh_pdf_attach_geopdf(&self.tabs[i].scene, plane, &mut state);
        self.pdf_attach = Some(state);
    }

    /// Put a file that was read into the Attach window.
    fn set_pdf_attach_file(
        &self,
        state: &mut PdfAttachState,
        path: &str,
        pages: Vec<crate::ui::window::pdf_dialogs::PageThumb>,
        sizes: Vec<String>,
        units: Vec<f64>,
    ) {
        state.path = path.to_string();
        state.name = file_stem(path);
        state.pages = pages;
        state.selected = vec![0];
        state.anchor = 0;
        state.found_in = crate::entities::underlay::display_path(path);
        state.page_size = sizes.first().cloned().unwrap_or_default();
        state.sub_per_master = units.first().copied().unwrap_or(1.0);
        state.item_sizes = sizes;
        state.item_units = units;
        state.sub_units = false;
        state.saved_path =
            crate::entities::underlay::display_path(&self.pdf_stored_path(path, state.path_type.0));
    }

    fn refresh_pdf_attach_geopdf(
        scene: &crate::scene::Scene,
        plane: crate::command::WorkingPlane,
        state: &mut PdfAttachState,
    ) {
        let was_available = state.geopdf_available;
        state.geopdf_available = super::pdf_import::fully_georeferenced_pdf_pages(
            scene,
            state.kind,
            &state.path,
            &state.page_labels(),
            plane,
        )
        .is_some();
        // Preserve a deliberate manual choice while eligibility stays true.
        // A newly eligible selection starts in automatic mode again.
        state.geopdf_auto = state.geopdf_available && (state.geopdf_auto || !was_available);
    }

    /// POINTCLOUDATTACH after the file is chosen: the file is read off the
    /// UI thread, and the Attach Point Cloud window opens when the read
    /// ends ([`Self::finish_point_cloud_attach_read`]). A panic in the
    /// reader becomes an error on the command line.
    pub(in crate::app) fn read_point_cloud_for_attach(
        &mut self,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        self.read_point_cloud_for_attach_with(path, crate::scene::model::point_cloud::load)
    }

    /// [`Self::read_point_cloud_for_attach`] with the reader given, so a
    /// test can give one that panics.
    fn read_point_cloud_for_attach_with(
        &mut self,
        path: std::path::PathBuf,
        reader: fn(
            &std::path::Path,
        )
            -> Option<std::sync::Arc<crate::scene::model::point_cloud::CloudPoints>>,
    ) -> Task<Message> {
        self.command_line.push_info(&format!(
            "Reading {}…",
            crate::entities::underlay::display_path(&path.to_string_lossy())
        ));
        let worker_path = path.clone();
        crate::app::update::background_task(
            "POINTCLOUDATTACH",
            move || reader(&worker_path),
            move |cloud| Message::PdfDialog(PdfDialogMsg::CloudRead(path, cloud)),
        )
    }

    /// The end of the read for the Attach Point Cloud window: the window
    /// for a cloud, the reference's two errors for a file with no scan
    /// that can be read. The window takes the place of its own older copy
    /// (Browse); if a different window opened during the read, it does
    /// not open.
    pub(in crate::app) fn finish_point_cloud_attach_read(
        &mut self,
        path: &std::path::Path,
        cloud: Option<&crate::scene::model::point_cloud::CloudPoints>,
    ) {
        if self
            .active_modal
            .is_some_and(|modal| modal != crate::app::ModalKind::PointCloudAttach)
        {
            self.command_line.push_error(
                "Attach point cloud cancelled: a different window opened while the file was read.",
            );
            return;
        }
        let Some(cloud) = cloud else {
            self.command_line
                .push_error("All scans are not found or invalid.");
            self.command_line.push_error("Attach point cloud failed");
            return;
        };
        self.open_point_cloud_attach_dialog(path, cloud);
    }

    /// The Attach Point Cloud dialog for a cloud already read, placed on
    /// screen by default. It does not read the file.
    fn open_point_cloud_attach_dialog(
        &mut self,
        path: &std::path::Path,
        cloud: &crate::scene::model::point_cloud::CloudPoints,
    ) {
        let yes_no = |on: bool| {
            if on {
                crate::t!("Yes")
            } else {
                crate::t!("No")
            }
            .into_owned()
        };
        let data = format!(
            "RGB: {} · {}: {} · {}: {}",
            yes_no(cloud.has_rgb),
            crate::t!("Intensity"),
            yes_no(cloud.has_intensity),
            crate::t!("Normals"),
            yes_no(cloud.has_normals),
        );
        let data_more = format!(
            "{}: {} · {}: {}",
            crate::t!("Classification"),
            yes_no(false),
            crate::t!("Segmentation"),
            yes_no(false),
        );
        let factor = crate::app::properties::insert_unit_scale(
            self.tabs[self.active_tab]
                .scene
                .document
                .header
                .insertion_units,
            6,
        )
        .unwrap_or(1.0);
        let full = path.to_string_lossy().into_owned();
        let memory = crate::ui::window::pdf_dialogs::ATTACH_MEMORY
            .lock()
            .ok()
            .and_then(|m| m.clone())
            .unwrap_or_default();
        let mut state = PointCloudAttachState {
            path: full.clone(),
            name: file_stem(&full),
            preview: cloud
                .preview
                .clone()
                .map(iced::widget::image::Handle::from_bytes),
            summary: format!(
                "{} · {}",
                crate::tf!("{count} scans", count = cloud.scans),
                crate::tf!("{count} points", count = cloud.positions.len())
            ),
            data,
            data_more,
            size: cloud
                .bounds
                .map(|[lo, hi]| {
                    format!(
                        "{:.4} × {:.4} × {:.4}",
                        hi[0] - lo[0],
                        hi[1] - lo[1],
                        hi[2] - lo[2]
                    )
                })
                .unwrap_or_default(),
            unit: format!(
                "{} · {} {}",
                crate::t!("Meters"),
                crate::t!("Unit factor"),
                crate::app::properties::format_unit_factor(factor)
            ),
            found_in: crate::entities::underlay::display_path(&full),
            saved_path: String::new(),
            path_type: memory.path_type,
            insert_on_screen: true,
            insert: ["0.0000".into(), "0.0000".into(), "0.0000".into()],
            scale_on_screen: true,
            scale: "1.0000".into(),
            rotation_on_screen: true,
            rotation: "0".into(),
            lock: false,
            zoom: true,
        };
        state.saved_path = crate::entities::underlay::display_path(
            &self.pdf_stored_path(&full, state.path_type.0),
        );
        self.point_cloud_attach = Some(state);
        self.active_modal = Some(crate::app::ModalKind::PointCloudAttach);
    }

    /// The dialog's Attach: what it set goes to the command, which asks for
    /// the rest.
    fn point_cloud_attach_ok(&mut self, i: usize) -> Task<Message> {
        use crate::command::CadCommand;
        let Some(state) = self.point_cloud_attach.as_ref() else {
            return Task::none();
        };
        let number = |s: &str| s.trim().parse::<f64>().ok();
        let insertion = if state.insert_on_screen {
            None
        } else {
            match (
                number(&state.insert[0]),
                number(&state.insert[1]),
                number(&state.insert[2]),
            ) {
                (Some(x), Some(y), Some(z)) => Some(glam::DVec3::new(x, y, z)),
                _ => {
                    self.command_line.push_rejection("Requires numeric value.");
                    return Task::none();
                }
            }
        };
        let scale = if state.scale_on_screen {
            None
        } else {
            match number(&state.scale).filter(|v| *v > 0.0) {
                Some(v) => Some(v),
                None => {
                    self.command_line
                        .push_rejection("Value must be positive and nonzero.");
                    return Task::none();
                }
            }
        };
        let rotation = if state.rotation_on_screen {
            None
        } else {
            match number(&state.rotation) {
                Some(v) => Some(v),
                None => {
                    self.command_line.push_rejection("Requires numeric value.");
                    return Task::none();
                }
            }
        };
        let state = self.point_cloud_attach.take().expect("checked above");
        self.close_active_modal();
        let options = crate::modules::insert::pc_attach::AttachOptions {
            stored: (state.path_type.0 == Pathtype::None).then(|| {
                std::path::Path::new(&state.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }),
            relative: state.path_type.0 == Pathtype::Relative,
            locked: Some(state.lock),
            zoom: state.zoom,
            scale,
            rotation,
        };
        let mut command = crate::modules::insert::pc_attach::PointCloudAttachCommand::from_dialog(
            std::path::PathBuf::from(&state.path),
            options,
        );
        match insertion {
            Some(point) => {
                let result = command.on_point(point);
                self.tabs[i].active_cmd = Some(Box::new(command));
                self.apply_cmd_result(result)
            }
            None => {
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                self.focus_cmd_input()
            }
        }
    }

    /// ULAYERS: every underlay of the drawing, the selected one first shown
    /// (none: an empty dialog).
    pub(in crate::app) fn open_underlay_layers_dialog(&mut self, i: usize) {
        let document = &self.tabs[i].scene.document;
        let selected: Vec<document::Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .map(|(h, _)| *h)
            .collect();
        let mut targets: Vec<LayerTarget> = Vec::new();
        for entity in document.entities() {
            let document::EntityType::Underlay(u) = entity else {
                continue;
            };
            let Some(def) = crate::entities::underlay::definition(u, document) else {
                continue;
            };
            let mut name = crate::entities::underlay::definition_display_name(def);
            let same = targets.iter().filter(|t| t.name.starts_with(&name)).count();
            if same > 0 {
                name = format!("{name} ({})", same + 1);
            }
            let mut layers: Vec<String> = match u.underlay_type {
                document::entities::UnderlayType::Pdf => {
                    crate::scene::model::pdf_layers::layers(&def.file_path)
                        .into_iter()
                        .map(|l| l.name)
                        .collect()
                }
                kind => crate::scene::model::underlay_vector::layer_names(kind, &def.file_path),
            };
            layers.sort_by_key(|n| n.to_lowercase());
            layers.dedup();
            targets.push(LayerTarget {
                name,
                handle: u.common.handle,
                layers,
                hidden: crate::scene::model::pdf_layers::hidden_layers(u),
            });
        }
        let current = targets
            .iter()
            .position(|t| selected.contains(&t.handle))
            .unwrap_or(0);
        self.underlay_layers = Some(UnderlayLayersState {
            targets,
            current,
            search: String::new(),
        });
        self.active_modal = Some(crate::app::ModalKind::UnderlayLayers);
    }

    /// PDFIMPORT's Settings option: the dialog over the running command.
    pub(in crate::app) fn open_pdf_import_settings(&mut self) {
        self.pdf_import_settings = Some(pdf_import::import_settings());
        self.active_modal = Some(crate::app::ModalKind::PdfImportSettings);
    }

    /// TXT2MTXT's SEttings option: the dialog over the running command.
    pub(in crate::app) fn open_txt2mtxt_settings(&mut self) {
        self.txt2mtxt_dialog = Some(self.txt2mtxt_settings);
        self.active_modal = Some(crate::app::ModalKind::Txt2MtxtSettings);
    }

    /// MATCHPROP's Settings option: the Property Settings window over the
    /// running command, with the source object's basic values.
    pub(in crate::app) fn open_matchprop_settings(&mut self, src: document::Handle) {
        use document::types::Color;
        let doc = &self.tabs[self.active_tab].scene.document;
        let Some(entity) = doc.get_entity(src) else {
            return;
        };
        let common = entity.common();
        let swatch = match common.color {
            Color::ByLayer => doc
                .layers
                .get(&common.layer)
                .and_then(|layer| layer.color.rgb()),
            Color::ByBlock => None,
            color => color.rgb(),
        }
        .map(|(r, g, b)| [r, g, b]);
        let plot_style = !doc.header.plotstyle_mode;
        let values = [
            crate::ui::window::attman_dialog::color_label(common.color),
            common.layer.clone(),
            if common.linetype.is_empty() {
                "ByLayer".to_string()
            } else {
                common.linetype.clone()
            },
            crate::entities::common::format_decimal(common.linetype_scale),
            crate::ui::window::attman_dialog::lineweight_label(common.line_weight),
            match common.transparency {
                document::Transparency::ByLayer => "ByLayer".to_string(),
                document::Transparency::ByBlock => "ByBlock".to_string(),
                document::Transparency::Explicit(alpha) => {
                    ((alpha as f64 / 255.0 * 100.0).round() as u32).to_string()
                }
            },
            crate::entities::common::format_decimal(
                crate::scene::view::dispatch::entity_thickness(entity).unwrap_or(0.0),
            ),
            if plot_style {
                crate::app::properties::plot_style_name(
                    doc,
                    common.plotstyle_flags,
                    common.plotstyle_handle,
                )
            } else {
                "ByColor".to_string()
            },
        ];
        self.matchprop_dialog = Some(crate::ui::window::pdf_dialogs::MatchPropDialog {
            mode: crate::entities::match_props::match_mode(),
            values,
            swatch,
            plot_style,
        });
        self.active_modal = Some(crate::app::ModalKind::MatchPropSettings);
    }

    /// PDFIMPORT's File option after the file is chosen: the file is read
    /// off the UI thread, and the Import PDF window opens when the read
    /// ends ([`Self::open_pdf_import_window`]); an encrypted file asks for
    /// its password first. A panic in the reader becomes an error on the
    /// command line.
    pub(in crate::app) fn open_pdf_import_file(&mut self, path: &str) -> Task<Message> {
        self.open_pdf_import_file_with(path, read_pdf_for_import_window)
    }

    /// [`Self::open_pdf_import_file`] with the reader given, so a test can
    /// give one that panics.
    fn open_pdf_import_file_with(
        &mut self,
        path: &str,
        reader: fn(&str) -> PdfImportFileRead,
    ) -> Task<Message> {
        self.command_line.push_info(&format!("Reading {path}…"));
        let path = path.to_string();
        let worker_path = path.clone();
        crate::app::update::background_task(
            "PDFIMPORT",
            move || std::sync::Arc::new(reader(&worker_path)),
            move |read| Message::PdfDialog(PdfDialogMsg::ImportRead(path, read)),
        )
    }

    /// The Import PDF window for a file read off the UI thread, or the
    /// password window when the file needs one. It takes the place of the
    /// Import File window (which IMPORT from a script or the automation
    /// `run` operation leaves open) and of its own older copy (Browse). If a
    /// different window opened during the read, neither window opens.
    fn open_pdf_import_window(&mut self, path: &str, read: &PdfImportFileRead) -> Task<Message> {
        use crate::ui::window::pdf_password_dialog::PdfPasswordThen;
        if self.active_modal.is_some_and(|modal| {
            !matches!(
                modal,
                crate::app::ModalKind::PdfImportFile | crate::app::ModalKind::ImportFile
            )
        }) {
            self.command_line.push_error(
                "Import PDF cancelled: a different window opened while the file was read.",
            );
            return Task::none();
        }
        match read {
            PdfImportFileRead::Locked { stage, remembered } => {
                self.open_pdf_password(path, *stage, *remembered, PdfPasswordThen::ImportFile)
            }
            PdfImportFileRead::Pages {
                pages,
                page_sizes,
                remembered,
            } => {
                if *remembered {
                    self.command_line
                        .push_output(&crate::scene::model::pdf_password::remembered_notice(path));
                }
                self.pdf_import_file = Some(PdfImportFileState {
                    path: path.to_string(),
                    pages: pages.clone(),
                    page_sizes: page_sizes.clone(),
                    selected: 0,
                    page_text: "1".into(),
                    insert_on_screen: false,
                    scale: "1".into(),
                    rotation: RotationChoice(0),
                    page_size: page_sizes.first().cloned().unwrap_or_default(),
                    settings: pdf_import::import_settings(),
                });
                self.active_modal = Some(crate::app::ModalKind::PdfImportFile);
                Task::none()
            }
        }
    }

    /// Writes the layers each underlay turns off, as one undo step.
    fn apply_underlay_layers(&mut self, i: usize, targets: Vec<LayerTarget>) {
        use crate::scene::model::pdf_layers::{
            hidden_layers, DGN_OVERRIDE_PLACEHOLDER, LAYER_OVERRIDE_APP,
        };
        let changed: Vec<LayerTarget> = targets
            .into_iter()
            .filter(|t| match self.tabs[i].scene.document.get_entity(t.handle) {
                Some(document::EntityType::Underlay(u)) => hidden_layers(u) != t.hidden,
                _ => false,
            })
            .collect();
        if changed.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, "ULAYERS");
        self.tabs[i].scene.ensure_app_id(LAYER_OVERRIDE_APP);
        let app_handle = self.tabs[i]
            .scene
            .document
            .app_ids
            .get(LAYER_OVERRIDE_APP)
            .map(|a| a.handle.value());
        for target in &changed {
            if let Some(document::EntityType::Underlay(u)) =
                self.tabs[i].scene.document.get_entity_mut(target.handle)
            {
                let data = &mut u.common.extended_data;
                if let Some(app) = app_handle {
                    data.raw_dwg_eed.retain(|(a, _)| *a != app);
                }
                data.remove_record(LAYER_OVERRIDE_APP);
                if !target.hidden.is_empty() {
                    let mut record = document::xdata::ExtendedDataRecord::new(LAYER_OVERRIDE_APP);
                    // A DGN underlay's names follow a placeholder string.
                    if u.underlay_type == document::entities::UnderlayType::Dgn {
                        record.add_value(document::xdata::XDataValue::String(
                            DGN_OVERRIDE_PLACEHOLDER.to_string(),
                        ));
                    }
                    for name in &target.hidden {
                        record.add_value(document::xdata::XDataValue::String(name.clone()));
                    }
                    data.add_record(record);
                }
            }
            self.tabs[i].scene.reseed_derived_caches(target.handle);
        }
        let changes: Vec<_> = changed
            .iter()
            .map(|t| (t.handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    pub(in crate::app) fn update_pdf_dialog(&mut self, message: PdfDialogMsg) -> Task<Message> {
        let i = self.active_tab;
        match message {
            PdfDialogMsg::Help(which) => {
                let kind = self.pdf_attach.as_ref().map(|s| s.kind);
                let text = match which {
                    "attach" if kind == Some(document::entities::UnderlayType::Dwf) => {
                        crate::t!("Attaches a DWF or DWFx file as an underlay.")
                    }
                    "attach" if kind == Some(document::entities::UnderlayType::Dgn) => {
                        crate::t!("Attaches a DGN file as an underlay.")
                    }
                    "attach" => crate::t!("Attaches a PDF file as an underlay."),
                    "pointcloud" => crate::t!("Attaches a point cloud scan or project."),
                    "pcsection" => crate::t!("Traces the points on the kept side of a live section as lines."),
                    "background" => std::borrow::Cow::Owned(crate::tr!("view-background", "help")),
                    "imageadjust" => std::borrow::Cow::Owned(crate::tr!("image-adjust", "help")),
                    "pointcloudcolormap" => {
                        crate::t!("Sets the color schemes and ranges a point cloud is colored by intensity or elevation.")
                    }
                    "layers" => crate::t!("Turns the layers of a PDF underlay on or off."),
                    "txt2mtxt" => std::borrow::Cow::Owned(crate::tr!("txt2mtxt", "help")),
                    "matchprop" => {
                        std::borrow::Cow::Owned(crate::tr!("matchprop-settings", "help"))
                    }
                    _ => crate::t!("Imports the geometry, fills, raster images and text of a PDF file as drawing objects."),
                };
                self.command_line.push_info(text.as_ref());
            }
            PdfDialogMsg::Options => {
                self.options_parent = self.active_modal;
                self.options_open();
            }

            // ── Attach ────────────────────────────────────────────────────
            PdfDialogMsg::AttachBrowse => {
                let kind = self
                    .pdf_attach
                    .as_ref()
                    .map(|s| s.kind)
                    .unwrap_or(document::entities::UnderlayType::Pdf);
                if let Some(state) = &self.pdf_attach {
                    state.remember();
                }
                if kind != document::entities::UnderlayType::Pdf {
                    return Task::done(Message::UnderlayAttachPick(kind));
                }
                return Task::done(Message::PdfAttachPick);
            }
            PdfDialogMsg::AttachSubUnits(sub) => {
                if let Some(s) = self.pdf_attach.as_mut() {
                    s.sub_units = sub;
                    // The scale offered follows the units, as on the command line.
                    s.scale = if sub {
                        format!("{}", 1.0 / s.sub_per_master)
                    } else {
                        "1.0000".to_string()
                    };
                }
            }
            PdfDialogMsg::AttachName(name) => {
                let Some(state) = self.pdf_attach.as_mut() else {
                    return Task::none();
                };
                let Some((_, path)) = state.existing.iter().find(|(n, _)| *n == name).cloned()
                else {
                    return Task::none();
                };
                state.pending_path = Some(path.clone());
                let kind = state.kind;
                return self.read_attach_name_with(kind, &path, read_for_attach_window);
            }
            PdfDialogMsg::AttachNameRead(path, read) => {
                self.finish_attach_name_read(&path, &read);
            }
            PdfDialogMsg::AttachPage(page) => {
                let (ctrl, shift) = (self.ctrl_down, self.shift_down);
                let plane = if self.tabs[i].editing_model_space() {
                    self.tabs[i].ucs_xform().working_plane()
                } else {
                    crate::command::WorkingPlane::default()
                };
                if let Some(state) = self.pdf_attach.as_mut() {
                    if state.kind == document::entities::UnderlayType::Pdf {
                        click_page(&mut state.selected, &mut state.anchor, page, ctrl, shift);
                    } else {
                        // One sheet or model at a time.
                        state.selected = vec![page];
                        state.anchor = page;
                    }
                    // The sizes and units were read with the file.
                    let first = state.selected.first().copied();
                    state.page_size = first
                        .and_then(|item| state.item_sizes.get(item).cloned())
                        .unwrap_or_default();
                    state.sub_per_master = first
                        .and_then(|item| state.item_units.get(item).copied())
                        .unwrap_or(1.0);
                    Self::refresh_pdf_attach_geopdf(&self.tabs[i].scene, plane, state);
                }
            }
            PdfDialogMsg::AttachPathType(choice) => {
                let Some(mut state) = self.pdf_attach.take() else {
                    return Task::none();
                };
                state.path_type = choice;
                state.saved_path = crate::entities::underlay::display_path(
                    &self.pdf_stored_path(&state.path, choice.0),
                );
                self.pdf_attach = Some(state);
            }
            PdfDialogMsg::AttachGeoPdfAuto(on) => {
                if let Some(state) = self.pdf_attach.as_mut() {
                    if state.geopdf_available {
                        state.geopdf_auto = on;
                    }
                }
            }
            PdfDialogMsg::AttachInsertOnScreen(on) => {
                if let Some(s) = self.pdf_attach.as_mut() {
                    s.insert_on_screen = on;
                }
            }
            PdfDialogMsg::AttachInsert(axis, value) => {
                if let Some(s) = self.pdf_attach.as_mut() {
                    s.insert[axis.min(2)] = value;
                }
            }
            PdfDialogMsg::AttachScaleOnScreen(on) => {
                if let Some(s) = self.pdf_attach.as_mut() {
                    s.scale_on_screen = on;
                }
            }
            PdfDialogMsg::AttachScale(value) => {
                if let Some(s) = self.pdf_attach.as_mut() {
                    s.scale = value;
                }
            }
            PdfDialogMsg::AttachRotationOnScreen(on) => {
                if let Some(s) = self.pdf_attach.as_mut() {
                    s.rotation_on_screen = on;
                }
            }
            PdfDialogMsg::AttachRotation(value) => {
                if let Some(s) = self.pdf_attach.as_mut() {
                    s.rotation = value;
                }
            }
            PdfDialogMsg::AttachDetails(on) => {
                if let Some(s) = self.pdf_attach.as_mut() {
                    s.details = on;
                }
            }
            PdfDialogMsg::AttachOk => return self.pdf_attach_ok(i),

            // ── Attach Point Cloud ────────────────────────────────────────
            PdfDialogMsg::CloudBrowse => return Task::done(Message::PointCloudAttachPick),
            PdfDialogMsg::CloudOk => return self.point_cloud_attach_ok(i),
            PdfDialogMsg::AttachRead(kind, path, read) => {
                return self.finish_underlay_attach_read(kind, &path, &read);
            }
            PdfDialogMsg::CloudRead(path, cloud) => {
                self.finish_point_cloud_attach_read(&path, cloud.as_deref());
            }
            PdfDialogMsg::CloudPromptRead(tab_id, path, found) => {
                return self.finish_point_cloud_prompt_read(tab_id, &path, found);
            }
            PdfDialogMsg::CloudAttachRead(tab_id, generation, attach, cloud) => {
                let (label, placement) = *attach;
                self.finish_point_cloud_attach(tab_id, generation, label, placement, cloud);
            }
            message @ (PdfDialogMsg::MapTab(_)
            | PdfDialogMsg::MapScheme(_)
            | PdfDialogMsg::MapCount(_)
            | PdfDialogMsg::MapEven
            | PdfDialogMsg::MapReverse
            | PdfDialogMsg::MapGradient(_)
            | PdfDialogMsg::MapNew
            | PdfDialogMsg::MapDelete
            | PdfDialogMsg::MapRename
            | PdfDialogMsg::MapNameInput(_)
            | PdfDialogMsg::MapNameOk
            | PdfDialogMsg::MapNameCancel
            | PdfDialogMsg::MapMax(_)
            | PdfDialogMsg::MapMin(_)
            | PdfDialogMsg::MapInterval(_)
            | PdfDialogMsg::MapExtents(_)
            | PdfDialogMsg::MapOutOfRange(_)
            | PdfDialogMsg::MapCurrent(_)
            | PdfDialogMsg::MapApply
            | PdfDialogMsg::MapOk) => return self.update_point_cloud_color_map(message),
            message @ (PdfDialogMsg::BgKind(_)
            | PdfDialogMsg::BgPickColor(_)
            | PdfDialogMsg::BgColor(..)
            | PdfDialogMsg::BgThree(_)
            | PdfDialogMsg::BgRotation(_)
            | PdfDialogMsg::BgImage(_)
            | PdfDialogMsg::BgBrowse
            | PdfDialogMsg::BgPosition(_)
            | PdfDialogMsg::BgAspect(_)
            | PdfDialogMsg::BgOffset(_)
            | PdfDialogMsg::BgScale(_)
            | PdfDialogMsg::BgStep(..)
            | PdfDialogMsg::BgReset
            | PdfDialogMsg::BgApply) => return self.update_view_background(message),
            message @ (PdfDialogMsg::IaValue(..)
            | PdfDialogMsg::IaText(..)
            | PdfDialogMsg::IaStep(..)
            | PdfDialogMsg::IaReset
            | PdfDialogMsg::IaApply) => return self.update_image_adjust(message),
            message @ (PdfDialogMsg::SecPerimeter(_)
            | PdfDialogMsg::SecMaxPoints(_)
            | PdfDialogMsg::SecLayer(_)
            | PdfDialogMsg::SecColor(_)
            | PdfDialogMsg::SecPolylines(_)
            | PdfDialogMsg::SecWidth(_)
            | PdfDialogMsg::SecMinLength(_)
            | PdfDialogMsg::SecConnect(_)
            | PdfDialogMsg::SecAngle(_)
            | PdfDialogMsg::SecPick(_)
            | PdfDialogMsg::SecPreview(_)
            | PdfDialogMsg::SecCreate) => return self.update_pc_section(message),
            PdfDialogMsg::CloudPathType(choice) => {
                let stored = self
                    .point_cloud_attach
                    .as_ref()
                    .map(|s| self.pdf_stored_path(&s.path, choice.0));
                if let (Some(s), Some(stored)) = (self.point_cloud_attach.as_mut(), stored) {
                    s.path_type = choice;
                    s.saved_path = crate::entities::underlay::display_path(&stored);
                }
            }
            PdfDialogMsg::CloudInsertOnScreen(on) => {
                if let Some(s) = self.point_cloud_attach.as_mut() {
                    s.insert_on_screen = on;
                }
            }
            PdfDialogMsg::CloudInsert(k, value) => {
                if let Some(s) = self.point_cloud_attach.as_mut() {
                    s.insert[k.min(2)] = value;
                }
            }
            PdfDialogMsg::CloudScaleOnScreen(on) => {
                if let Some(s) = self.point_cloud_attach.as_mut() {
                    s.scale_on_screen = on;
                }
            }
            PdfDialogMsg::CloudScale(value) => {
                if let Some(s) = self.point_cloud_attach.as_mut() {
                    s.scale = value;
                }
            }
            PdfDialogMsg::CloudRotationOnScreen(on) => {
                if let Some(s) = self.point_cloud_attach.as_mut() {
                    s.rotation_on_screen = on;
                }
            }
            PdfDialogMsg::CloudRotation(value) => {
                if let Some(s) = self.point_cloud_attach.as_mut() {
                    s.rotation = value;
                }
            }
            PdfDialogMsg::CloudLock(on) => {
                if let Some(s) = self.point_cloud_attach.as_mut() {
                    s.lock = on;
                }
            }
            PdfDialogMsg::CloudZoom(on) => {
                if let Some(s) = self.point_cloud_attach.as_mut() {
                    s.zoom = on;
                }
            }

            // ── Underlay Layers ───────────────────────────────────────────
            PdfDialogMsg::LayersUnderlay(name) => {
                if let Some(s) = self.underlay_layers.as_mut() {
                    if let Some(at) = s.targets.iter().position(|t| t.name == name) {
                        s.current = at;
                    }
                }
            }
            PdfDialogMsg::LayersSearch(text) => {
                if let Some(s) = self.underlay_layers.as_mut() {
                    s.search = text;
                }
            }
            PdfDialogMsg::LayersToggle(layer) => {
                if let Some(s) = self.underlay_layers.as_mut() {
                    if let Some(t) = s.targets.get_mut(s.current) {
                        match t.hidden.iter().position(|h| *h == layer) {
                            Some(at) => {
                                t.hidden.remove(at);
                            }
                            // Only a layer the file has can be turned off.
                            None if t.layers.contains(&layer) => t.hidden.push(layer),
                            None => {}
                        }
                    }
                }
            }
            PdfDialogMsg::LayersOk => {
                if let Some(state) = self.underlay_layers.take() {
                    self.apply_underlay_layers(i, state.targets);
                }
                self.close_active_modal();
            }

            // ── Import settings ───────────────────────────────────────────
            PdfDialogMsg::Vector(_)
            | PdfDialogMsg::Fills(_)
            | PdfDialogMsg::Text(_)
            | PdfDialogMsg::Raster(_)
            | PdfDialogMsg::Layers(_)
            | PdfDialogMsg::AsBlock(_)
            | PdfDialogMsg::Join(_)
            | PdfDialogMsg::Hatches(_)
            | PdfDialogMsg::Lineweights(_)
            | PdfDialogMsg::Linetypes(_) => {
                let settings = if self.active_modal == Some(crate::app::ModalKind::PdfImportFile) {
                    self.pdf_import_file.as_mut().map(|s| &mut s.settings)
                } else {
                    self.pdf_import_settings.as_mut()
                };
                if let Some(s) = settings {
                    match message {
                        PdfDialogMsg::Vector(v) => s.vector = v,
                        PdfDialogMsg::Fills(v) => s.fills = v,
                        PdfDialogMsg::Text(v) => s.text = v,
                        PdfDialogMsg::Raster(v) => s.raster = v,
                        PdfDialogMsg::Layers(v) => s.layers = v,
                        PdfDialogMsg::AsBlock(v) => s.as_block = v,
                        PdfDialogMsg::Join(v) => s.join = v,
                        PdfDialogMsg::Hatches(v) => s.hatches = v,
                        PdfDialogMsg::Lineweights(v) => s.lineweights = v,
                        PdfDialogMsg::Linetypes(v) => s.linetypes = v,
                        _ => {}
                    }
                }
            }
            PdfDialogMsg::SettingsOk => {
                if let Some(s) = self.pdf_import_settings.take() {
                    pdf_import::set_import_settings(s);
                }
                self.close_active_modal();
            }

            // ── Text to MText Settings ────────────────────────────────────
            PdfDialogMsg::T2mCombine(on) => {
                if let Some(s) = self.txt2mtxt_dialog.as_mut() {
                    s.combine = on;
                }
            }
            PdfDialogMsg::T2mSelectOrder(on) => {
                if let Some(s) = self.txt2mtxt_dialog.as_mut() {
                    s.select_order = on;
                }
            }
            PdfDialogMsg::T2mWrap(on) => {
                if let Some(s) = self.txt2mtxt_dialog.as_mut() {
                    s.word_wrap = on;
                }
            }
            PdfDialogMsg::T2mUniform(on) => {
                if let Some(s) = self.txt2mtxt_dialog.as_mut() {
                    s.uniform_spacing = on;
                }
            }
            PdfDialogMsg::T2mOk => {
                if let Some(s) = self.txt2mtxt_dialog {
                    if s != self.txt2mtxt_settings {
                        self.txt2mtxt_settings = s;
                        self.save_config();
                    }
                }
                self.close_active_modal();
            }

            // ── Property Settings (MATCHPROP) ─────────────────────────────
            PdfDialogMsg::MpToggle(bit) => {
                if let Some(s) = self.matchprop_dialog.as_mut() {
                    if s.switchable() & bit != 0 {
                        s.mode ^= bit;
                    }
                }
            }
            PdfDialogMsg::MpAll => {
                if let Some(s) = self.matchprop_dialog.as_mut() {
                    s.mode |= s.switchable();
                }
            }
            PdfDialogMsg::MpNone => {
                if let Some(s) = self.matchprop_dialog.as_mut() {
                    s.mode &= !s.switchable();
                }
            }
            // The settings line again, then (on close) the destination
            // prompt; the command goes on with the new set.
            PdfDialogMsg::MpOk => {
                if let Some(s) = &self.matchprop_dialog {
                    let mode = s.mode;
                    if mode != crate::entities::match_props::match_mode() {
                        crate::entities::match_props::set_match_mode(mode);
                        self.save_config();
                    }
                    self.command_line
                        .push_output(&crate::entities::match_props::active_settings_line(mode));
                    self.refresh_active_cmd_preview(i);
                }
                self.close_active_modal();
            }

            // ── Import PDF ────────────────────────────────────────────────
            PdfDialogMsg::ImportBrowse => return Task::done(Message::PdfImportPick),
            PdfDialogMsg::ImportPage(page) => {
                if let Some(s) = self.pdf_import_file.as_mut() {
                    s.selected = page;
                    s.page_text = (page + 1).to_string();
                    s.page_size = s.page_sizes.get(page).cloned().unwrap_or_default();
                }
            }
            PdfDialogMsg::ImportPageText(text) => {
                if let Some(s) = self.pdf_import_file.as_mut() {
                    if let Some(n) = text
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .filter(|n| (1..=s.pages.len()).contains(n))
                    {
                        s.selected = n - 1;
                        s.page_size = s.page_sizes.get(n - 1).cloned().unwrap_or_default();
                    }
                    s.page_text = text;
                }
            }
            PdfDialogMsg::ImportInsertOnScreen(on) => {
                if let Some(s) = self.pdf_import_file.as_mut() {
                    s.insert_on_screen = on;
                }
            }
            PdfDialogMsg::ImportScale(value) => {
                if let Some(s) = self.pdf_import_file.as_mut() {
                    s.scale = value;
                }
            }
            PdfDialogMsg::ImportRotation(r) => {
                if let Some(s) = self.pdf_import_file.as_mut() {
                    s.rotation = r;
                }
            }
            PdfDialogMsg::ImportOk => {
                let Some(state) = self.pdf_import_file.as_ref() else {
                    return Task::none();
                };
                let Some(scale) = state.scale.trim().parse::<f64>().ok().filter(|v| *v > 0.0)
                else {
                    self.command_line
                        .push_rejection("Value must be positive and nonzero.");
                    return Task::none();
                };
                let state = self.pdf_import_file.take().expect("checked above");
                pdf_import::set_import_settings(state.settings);
                self.close_active_modal();
                let import = PdfFileImport {
                    path: state.path.clone(),
                    page: (state.selected + 1).to_string(),
                    scale,
                    rotation: (state.rotation.0 as f64).to_radians(),
                    insertion: glam::DVec3::ZERO,
                };
                if state.insert_on_screen {
                    use crate::command::CadCommand;
                    let command = pdf_import::PdfImportPointCommand::new(import);
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                    return self.focus_cmd_input();
                }
                return self.run_pdf_import(
                    i,
                    crate::app::commands::pdf_import::PdfImportSource::File(import),
                );
            }
            PdfDialogMsg::ImportRead(path, read) => {
                return self.open_pdf_import_window(&path, &read)
            }
            PdfDialogMsg::ImportConverted(tab_id, generation, job, converted) => {
                self.finish_pdf_import(tab_id, generation, *job, *converted);
            }
        }
        Task::none()
    }

    fn pdf_attach_ok(&mut self, i: usize) -> Task<Message> {
        use crate::command::CadCommand;
        let Some(state) = self.pdf_attach.as_ref() else {
            return Task::none();
        };
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        let automatic = if state.geopdf_auto {
            super::pdf_import::fully_georeferenced_pdf_pages(
                &self.tabs[i].scene,
                state.kind,
                &state.path,
                &state.page_labels(),
                plane,
            )
        } else {
            None
        };
        let number = |s: &str| s.trim().parse::<f64>().ok();
        let insertion = if automatic.is_some() {
            None
        } else if state.insert_on_screen {
            None
        } else {
            match (
                number(&state.insert[0]),
                number(&state.insert[1]),
                number(&state.insert[2]),
            ) {
                (Some(x), Some(y), Some(z)) => Some(glam::DVec3::new(x, y, z)),
                _ => {
                    self.command_line.push_rejection("Requires numeric value.");
                    return Task::none();
                }
            }
        };
        let scale = if automatic.is_some() {
            None
        } else if state.scale_on_screen {
            None
        } else {
            match number(&state.scale).filter(|v| *v > 0.0) {
                Some(v) => Some(v),
                None => {
                    self.command_line
                        .push_rejection("Value must be positive and nonzero.");
                    return Task::none();
                }
            }
        };
        let rotation = if automatic.is_some() {
            None
        } else if state.rotation_on_screen {
            None
        } else {
            match number(&state.rotation) {
                Some(v) => Some(v),
                None => {
                    self.command_line.push_rejection("Requires numeric value.");
                    return Task::none();
                }
            }
        };
        let state = self.pdf_attach.take().expect("checked above");
        state.remember();
        let stored = self.pdf_stored_path(&state.path, state.path_type.0);
        if stored != state.path {
            if let Some(bytes) = crate::scene::model::pdf_raster::source_bytes(&state.path) {
                crate::scene::model::pdf_raster::register_source(&stored, bytes);
            }
        }
        self.close_active_modal();
        let insunits = self.tabs[i].scene.document.header.insertion_units;
        let mut command = crate::modules::insert::pdf_attach::PdfAttachCommand::from_kind_dialog(
            state.kind,
            &state.path,
            &stored,
            &state.page_labels(),
            scale,
            rotation,
            state.sub_units,
            insunits,
        );
        command.set_auto_place(state.geopdf_auto);
        if let Some(pages) = automatic {
            self.tabs[i].active_cmd = Some(Box::new(command));
            return self.apply_cmd_result(crate::command::CmdResult::AttachPdfPages {
                kind: state.kind,
                path: stored,
                pages,
                auto_place: false,
            });
        }
        match insertion {
            Some(point) => {
                let result = command.on_point(point);
                self.tabs[i].active_cmd = Some(Box::new(command));
                if matches!(result, crate::command::CmdResult::NeedPoint) {
                    self.command_line.push_info(
                        &self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .map(|c| c.prompt())
                            .unwrap_or_default(),
                    );
                    return self.focus_cmd_input();
                }
                self.apply_cmd_result(result)
            }
            None => {
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                self.focus_cmd_input()
            }
        }
    }
}

/// The Import PDF window's read of a file, on a worker: the password the
/// file needs, or its pages and their sizes.
fn read_pdf_for_import_window(path: &str) -> PdfImportFileRead {
    use crate::scene::model::pdf_password::{gate, Gate};
    let remembered = match gate(path, true) {
        Gate::Open => false,
        Gate::Remembered => true,
        Gate::Ask { stage, remembered } => {
            return PdfImportFileRead::Locked { stage, remembered };
        }
    };
    let pages = page_thumbs(path);
    let page_sizes = pages.iter().map(|p| page_size_mm(path, &p.label)).collect();
    PdfImportFileRead::Pages {
        pages,
        page_sizes,
        remembered,
    }
}

/// The name of the command that a read for the Attach window reports a
/// panic with.
fn attach_context(kind: document::entities::UnderlayType) -> &'static str {
    match kind {
        document::entities::UnderlayType::Dwf => "DWFATTACH",
        document::entities::UnderlayType::Dgn => "DGNATTACH",
        _ => "PDFATTACH",
    }
}

/// The size text and the sub units per master unit of each page, sheet
/// or model.
fn item_sizes(
    kind: document::entities::UnderlayType,
    path: &str,
    pages: &[crate::ui::window::pdf_dialogs::PageThumb],
) -> (Vec<String>, Vec<f64>) {
    pages
        .iter()
        .map(|page| {
            (
                item_size_text(kind, path, &page.label),
                sub_per_master(kind, path, &page.label),
            )
        })
        .unzip()
}

/// The Attach window's read of a PDF, DWF or DGN file: the password the
/// file needs, or its pages, sheets or models. `fresh` reads the file from
/// the disk first, because a file picked again can have changed; else the
/// bytes registered for `path` are used.
fn read_for_attach_window(
    kind: document::entities::UnderlayType,
    path: &str,
    fresh: bool,
) -> UnderlayAttachRead {
    use crate::scene::model::pdf_password::{gate, Gate};
    use crate::scene::model::underlay_vector;
    let pdf = kind == document::entities::UnderlayType::Pdf;
    if fresh {
        match std::fs::read(path) {
            Ok(bytes) => {
                crate::scene::model::pdf_raster::register_source(path, std::sync::Arc::new(bytes))
            }
            Err(error) if pdf => return UnderlayAttachRead::Failed(error.to_string()),
            Err(_) => return UnderlayAttachRead::NoItems,
        }
        if !pdf {
            underlay_vector::forget(path);
        }
    }
    let mut remembered = false;
    if pdf {
        match gate(path, false) {
            Gate::Open => {}
            Gate::Remembered => remembered = true,
            Gate::Ask { stage, remembered } => {
                return UnderlayAttachRead::Locked { stage, remembered };
            }
        }
    } else if !underlay_vector::item_names(kind, path).is_some_and(|names| !names.is_empty()) {
        return UnderlayAttachRead::NoItems;
    }
    let pages = item_thumbs(kind, path);
    let (sizes, units) = item_sizes(kind, path, &pages);
    UnderlayAttachRead::Items {
        pages,
        sizes,
        units,
        remembered,
    }
}

/// "200 × 100 mm" for a page.
fn page_size_mm(path: &str, page: &str) -> String {
    crate::scene::model::pdf_raster::page_size_inches(path, page)
        .map(|(w, h)| format!("{:.0} × {:.0} mm", w * 25.4, h * 25.4))
        .unwrap_or_default()
}

#[cfg(test)]
mod import_window_tests {
    use super::*;
    use crate::app::ModalKind;
    use std::sync::Arc;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn last_line(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .last()
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    }

    /// A PDF with a 10 × 5 inch page and a 2 × 1 inch page, registered as
    /// a file picked this session under `name`.
    fn two_sizes(name: &str) -> String {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 720 360] >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 144 72] >>",
        ];
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend(b"0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        crate::scene::model::pdf_raster::register_source(name, Arc::new(bytes));
        name.to_string()
    }

    /// The window opens only when the read ends, after a `Reading` line,
    /// with the file's pages and the size of the first.
    #[test]
    fn the_window_opens_only_when_the_read_ends() {
        let mut app = new_app();
        let path = two_sizes("pdf-window-read.pdf");
        let task = app.open_pdf_import_file(&path);
        assert_eq!(app.active_modal, None, "nothing opens during the read");
        assert_eq!(last_line(&app), format!("ⓘ Reading {path}…"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfImportFile));
        let state = app.pdf_import_file.as_ref().expect("the window's state");
        assert_eq!(state.pages.len(), 2);
        assert_eq!(state.page_size, "254 × 127 mm");
    }

    /// A page chosen in the window shows its size, read with the pages.
    #[test]
    fn a_page_chosen_in_the_window_shows_its_size() {
        let mut app = new_app();
        let path = two_sizes("pdf-window-sizes.pdf");
        let task = app.open_pdf_import_file(&path);
        app.drive_headless_task(task).expect("the read task");
        let size = |app: &OpenCADStudio| app.pdf_import_file.as_ref().unwrap().page_size.clone();
        let page = |app: &OpenCADStudio| {
            let s = app.pdf_import_file.as_ref().unwrap();
            (s.selected, s.page_text.clone())
        };
        let _ = app.update(Message::PdfDialog(PdfDialogMsg::ImportPage(1)));
        assert_eq!(size(&app), "51 × 25 mm");
        assert_eq!(page(&app), (1, "2".to_string()));
        let _ = app.update(Message::PdfDialog(PdfDialogMsg::ImportPageText("1".into())));
        assert_eq!(size(&app), "254 × 127 mm");
        assert_eq!(page(&app), (0, "1".to_string()));
        let _ = app.update(Message::PdfDialog(PdfDialogMsg::ImportPageText("2".into())));
        assert_eq!(size(&app), "51 × 25 mm");
        assert_eq!(page(&app), (1, "2".to_string()));
    }

    /// A reader that panics gives an error, no window opens, and the next
    /// read works.
    #[test]
    fn a_reader_panic_is_an_error_and_no_window_opens() {
        let mut app = new_app();
        let task = app.open_pdf_import_file_with("panics.pdf", |_| panic!("a reader bug"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("PDFIMPORT failed: a reader bug")
        );
        assert_eq!(app.active_modal, None);
        assert!(app.pdf_import_file.is_none());
        let path = two_sizes("pdf-window-after-panic.pdf");
        let task = app.open_pdf_import_file(&path);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfImportFile));
    }

    /// A window that opened during the read stays, and the Import PDF
    /// window does not open.
    #[test]
    fn a_window_opened_during_the_read_stays() {
        let mut app = new_app();
        let task = app.open_pdf_import_file(&two_sizes("pdf-window-other.pdf"));
        app.active_modal = Some(ModalKind::PdfImportSettings);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfImportSettings));
        assert!(app.pdf_import_file.is_none());
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Import PDF cancelled: a different window opened while the file was read.")
        );
    }

    /// IMPORT with a file name, from a script or the automation `run`
    /// operation, leaves the Import File window open during the read; the
    /// Import PDF window takes its place, as before the read moved to a
    /// worker. So does a new copy of the Import PDF window (Browse).
    #[test]
    fn the_window_takes_the_place_of_the_import_file_window_and_of_itself() {
        let mut app = new_app();
        for open in [ModalKind::ImportFile, ModalKind::PdfImportFile] {
            let task = app.open_pdf_import_file(&two_sizes("pdf-window-place.pdf"));
            app.active_modal = Some(open);
            app.drive_headless_task(task).expect("the read task");
            assert_eq!(app.active_modal, Some(ModalKind::PdfImportFile), "{open:?}");
            assert!(app.command_line.last_error.is_none(), "{open:?}");
        }
    }

    /// An encrypted file opens the password window when the read ends, not
    /// the Import PDF window.
    #[test]
    fn an_encrypted_file_asks_for_its_password_after_the_read() {
        let mut app = new_app();
        let path = "pdf-window-locked.pdf";
        let file = "%PDF-1.4\ntrailer\n<< /Encrypt << /Filter /Standard /V 1 /R 2 \
            /O <95fe1f4219662a774442fb072e3d9f19e9d130ec09a4d0061e78fe920f7ab62f> \
            /U <0102030405060708090a0b0c0d0e0f100102030405060708090a0b0c0d0e0f10> /P -44 >> >>\n%%EOF\n";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(file.as_bytes().to_vec()));
        let task = app.open_pdf_import_file(path);
        assert_eq!(app.active_modal, None, "nothing opens during the read");
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfPassword));
        assert!(app.pdf_import_file.is_none());
    }
}

#[cfg(test)]
mod point_cloud_attach_read_tests {
    use super::*;
    use crate::app::ModalKind;
    use crate::scene::model::point_cloud::test_scan_file;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn last_line(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .last()
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    }

    /// The window opens only when the read ends, after a `Reading` line,
    /// with what the read found.
    #[test]
    fn the_window_opens_only_when_the_read_ends() {
        let mut app = new_app();
        let path = test_scan_file("attach-window", 100);
        let task = app.read_point_cloud_for_attach(path.clone());
        assert_eq!(app.active_modal, None, "nothing opens during the read");
        assert_eq!(
            last_line(&app),
            format!(
                "ⓘ Reading {}…",
                crate::entities::underlay::display_path(&path.to_string_lossy())
            )
        );
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PointCloudAttach));
        let state = app.point_cloud_attach.as_ref().expect("the window's state");
        assert_eq!(state.path, path.to_string_lossy());
        assert!(
            state.summary.contains("100 points"),
            "summary: {}",
            state.summary
        );
        let _ = std::fs::remove_file(path);
    }

    /// A reader that panics gives an error, no window opens, and the next
    /// read works.
    #[test]
    fn a_reader_panic_is_an_error_and_no_window_opens() {
        let mut app = new_app();
        let task =
            app.read_point_cloud_for_attach_with("panics.rcs".into(), |_| panic!("a reader bug"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("POINTCLOUDATTACH failed: a reader bug")
        );
        assert_eq!(app.active_modal, None);
        assert!(app.point_cloud_attach.is_none());
        let path = test_scan_file("attach-after-panic", 10);
        let task = app.read_point_cloud_for_attach(path.clone());
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PointCloudAttach));
        let _ = std::fs::remove_file(path);
    }

    /// The file picker's result for a scan or a project starts the read,
    /// and the window opens when it ends.
    #[test]
    fn the_picker_result_reads_scans_and_projects_on_a_worker() {
        let scan = test_scan_file("attach-picker", 12);
        let project = scan.with_extension("rcp");
        let xml = format!(
            "<Project><VoxelTreeRunTime Id=\"s1\"><Path Value=\"{}\"/>\
             <Translation x=\"0\" y=\"0\" z=\"0\"/></VoxelTreeRunTime></Project>",
            scan.display()
        );
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("project.xml", zip::write::SimpleFileOptions::default())
            .expect("start the project");
        std::io::Write::write_all(&mut zip, xml.as_bytes()).expect("write the project");
        std::fs::write(&project, zip.finish().expect("the project").into_inner())
            .expect("write the project file");
        for path in [&scan, &project] {
            let mut app = new_app();
            let task = app.update(Message::AttachPickResult(Ok(path.clone())));
            assert_eq!(app.active_modal, None, "nothing opens during the read");
            assert!(
                last_line(&app).starts_with("ⓘ Reading "),
                "{}",
                last_line(&app)
            );
            app.drive_headless_task(task).expect("the read task");
            assert_eq!(app.active_modal, Some(ModalKind::PointCloudAttach));
            let state = app.point_cloud_attach.as_ref().expect("the window's state");
            assert!(state.summary.contains("12 points"), "{}", state.summary);
        }
        let _ = std::fs::remove_file(scan);
        let _ = std::fs::remove_file(project);
    }

    /// A file with no scan that can be read gives the reference's two
    /// errors when the read ends, and no window.
    #[test]
    fn a_file_that_cannot_be_read_gives_the_two_errors() {
        let mut app = new_app();
        let path = std::env::temp_dir().join(format!(
            "ocs-pointcloud-attach-bad-{}.rcs",
            std::process::id()
        ));
        std::fs::write(&path, b"not a scan").expect("write the bad scan");
        let task = app.read_point_cloud_for_attach(path.clone());
        app.drive_headless_task(task).expect("the read task");
        let errors: Vec<String> = app
            .command_line
            .history
            .iter()
            .rev()
            .take(2)
            .map(|entry| entry.text.clone())
            .collect();
        assert!(
            errors[0].ends_with("Attach point cloud failed")
                && errors[1].ends_with("All scans are not found or invalid."),
            "lines: {errors:?}"
        );
        assert_eq!(app.active_modal, None);
        let _ = std::fs::remove_file(path);
    }

    /// A window that opened during the read stays, and the Attach Point
    /// Cloud window does not open.
    #[test]
    fn a_window_opened_during_the_read_stays() {
        let mut app = new_app();
        let path = test_scan_file("attach-other-window", 10);
        let task = app.read_point_cloud_for_attach(path.clone());
        app.active_modal = Some(ModalKind::PdfImportSettings);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfImportSettings));
        assert!(app.point_cloud_attach.is_none());
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(
                "Attach point cloud cancelled: a different window opened while the file was read."
            )
        );
        let _ = std::fs::remove_file(path);
    }

    /// Browse in the window: the new file's window takes the place of the
    /// old one.
    #[test]
    fn the_window_takes_the_place_of_its_older_copy() {
        let mut app = new_app();
        let first = test_scan_file("attach-first", 10);
        let second = test_scan_file("attach-second", 20);
        let task = app.read_point_cloud_for_attach(first.clone());
        app.drive_headless_task(task).expect("the first read");
        let task = app.read_point_cloud_for_attach(second.clone());
        app.drive_headless_task(task).expect("the second read");
        assert_eq!(app.active_modal, Some(ModalKind::PointCloudAttach));
        let state = app.point_cloud_attach.as_ref().expect("the window's state");
        assert_eq!(state.path, second.to_string_lossy());
        assert!(state.summary.contains("20 points"), "{}", state.summary);
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
    }

    /// The window shows the cloud the read gave, and does not read the
    /// file again: here the file is gone when the window opens.
    #[test]
    fn the_window_shows_the_read_result_without_a_second_read() {
        let mut app = new_app();
        let path = test_scan_file("attach-no-second-read", 30);
        let cloud = crate::scene::model::point_cloud::load(&path).expect("the scan reads");
        std::fs::remove_file(&path).expect("remove the scan");
        app.finish_point_cloud_attach_read(&path, Some(&cloud));
        assert_eq!(app.active_modal, Some(ModalKind::PointCloudAttach));
        let state = app.point_cloud_attach.as_ref().expect("the window's state");
        assert!(state.summary.contains("30 points"), "{}", state.summary);
        assert_eq!(app.command_line.last_error, None);
    }
}

#[cfg(test)]
mod underlay_attach_read_tests {
    use super::*;
    use crate::app::ModalKind;
    use document::entities::UnderlayType;
    use std::sync::Arc;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn last_line(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .last()
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    }

    /// A PDF with a 10 × 5 inch page and a 2 × 1 inch page.
    fn two_pages() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 720 360] >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 144 72] >>",
        ];
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend(b"0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        bytes
    }

    /// A file in the temporary folder, with a name for this process.
    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("ocs-underlay-attach-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).expect("write the test file");
        path
    }

    /// The window opens only when the read ends, after a `Reading` line,
    /// with the pages and the size of the first.
    #[test]
    fn the_window_opens_only_when_the_read_ends() {
        let mut app = new_app();
        let path = "underlay-attach-read.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_pages()));
        let task = app.open_pdf_attach_dialog(path);
        assert_eq!(app.active_modal, None, "nothing opens during the read");
        assert_eq!(last_line(&app), format!("ⓘ Reading {path}…"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfAttach));
        let state = app.pdf_attach.as_ref().expect("the window's state");
        assert_eq!(state.pages.len(), 2);
        assert_eq!(state.page_size, "10.0000 × 5.0000");
    }

    /// A reader that panics gives an error with the command's name, no
    /// window opens, and the next read works.
    #[test]
    fn a_reader_panic_is_an_error_and_no_window_opens() {
        let mut app = new_app();
        for (kind, name) in [
            (UnderlayType::Pdf, "PDFATTACH"),
            (UnderlayType::Dwf, "DWFATTACH"),
            (UnderlayType::Dgn, "DGNATTACH"),
        ] {
            let task = app.read_underlay_for_attach_with(kind, "panics", true, |_, _, _| {
                panic!("a reader bug")
            });
            app.drive_headless_task(task).expect("the read task");
            assert_eq!(
                app.command_line.last_error.as_deref(),
                Some(format!("{name} failed: a reader bug").as_str())
            );
            assert_eq!(app.active_modal, None);
            assert!(app.pdf_attach.is_none());
        }
        let path = "underlay-attach-after-panic.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_pages()));
        let task = app.open_pdf_attach_dialog(path);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfAttach));
    }

    /// The ATTACH picker's result for a PDF and for a DGN file starts the
    /// read from the disk, and the window opens when it ends.
    #[test]
    fn the_attach_picker_reads_pdf_and_dgn_files_on_a_worker() {
        let pdf = temp_file("picked.pdf", &two_pages());
        let dgn = temp_file(
            "picked.dgn",
            &crate::modules::insert::dgn_import::test_files::minimal_v7_line(),
        );
        for (path, kind, pages) in [(&pdf, UnderlayType::Pdf, 2), (&dgn, UnderlayType::Dgn, 1)] {
            let mut app = new_app();
            let task = app.update(Message::AttachPickResult(Ok(path.clone())));
            assert_eq!(app.active_modal, None, "nothing opens during the read");
            assert!(
                last_line(&app).starts_with("ⓘ Reading "),
                "{}",
                last_line(&app)
            );
            app.drive_headless_task(task).expect("the read task");
            assert_eq!(app.active_modal, Some(ModalKind::PdfAttach), "{path:?}");
            let state = app.pdf_attach.as_ref().expect("the window's state");
            assert_eq!(state.kind, kind);
            assert_eq!(state.pages.len(), pages, "{path:?}");
        }
        let _ = std::fs::remove_file(pdf);
        let _ = std::fs::remove_file(dgn);
    }

    /// A file picked again is read from the disk again: a DGN file that
    /// became damaged since the first read gives `not found`.
    #[test]
    fn a_file_picked_again_is_read_again() {
        let mut app = new_app();
        let dgn = temp_file(
            "picked-again.dgn",
            &crate::modules::insert::dgn_import::test_files::minimal_v7_line(),
        );
        let task = app.update(Message::AttachPickResult(Ok(dgn.clone())));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfAttach));
        app.active_modal = None;
        app.pdf_attach = None;
        std::fs::write(&dgn, b"not a design file").expect("damage the file");
        let task = app.update(Message::AttachPickResult(Ok(dgn.clone())));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, None);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(
                format!(
                    "{} not found.",
                    crate::entities::underlay::display_path(&dgn.to_string_lossy())
                )
                .as_str()
            )
        );
        let _ = std::fs::remove_file(dgn);
    }

    /// A DGN file that changed since its first read shows its new units
    /// when it is picked again: the read forgets what it read before.
    #[test]
    fn a_changed_file_picked_again_shows_its_new_units() {
        let mut app = new_app();
        let mut bytes = crate::modules::insert::dgn_import::test_files::minimal_v7_line();
        let dgn = temp_file("changed-units.dgn", &bytes);
        let task = app.update(Message::AttachPickResult(Ok(dgn.clone())));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.pdf_attach.as_ref().map(|s| s.sub_per_master),
            Some(1000.0)
        );
        app.active_modal = None;
        app.pdf_attach = None;
        // Subunits per master unit: high word, then low word.
        bytes[1112..1116].copy_from_slice(&[0, 0, 0xf4, 0x01]);
        std::fs::write(&dgn, &bytes).expect("change the file");
        let task = app.update(Message::AttachPickResult(Ok(dgn.clone())));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.pdf_attach.as_ref().map(|s| s.sub_per_master),
            Some(500.0)
        );
        let _ = std::fs::remove_file(dgn);
    }

    /// A PDF file that cannot be read gives the read's error.
    #[test]
    fn a_pdf_that_cannot_be_read_gives_the_error() {
        let mut app = new_app();
        let missing =
            std::env::temp_dir().join(format!("ocs-underlay-missing-{}.pdf", std::process::id()));
        let task = app.update(Message::AttachPickResult(Ok(missing)));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, None);
        let error = app.command_line.last_error.clone().unwrap_or_default();
        assert!(error.starts_with("PDFATTACH: "), "{error}");
    }

    /// A DGN or DWF file that cannot be read gives `not found`, not the
    /// PDF read error.
    #[test]
    fn a_dgn_that_cannot_be_read_gives_not_found() {
        let mut app = new_app();
        let missing =
            std::env::temp_dir().join(format!("ocs-underlay-missing-{}.dgn", std::process::id()));
        let task = app.update(Message::AttachPickResult(Ok(missing.clone())));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, None);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(
                format!(
                    "{} not found.",
                    crate::entities::underlay::display_path(&missing.to_string_lossy())
                )
                .as_str()
            )
        );
    }

    /// An encrypted file opens the password window when the read ends.
    #[test]
    fn an_encrypted_file_asks_for_its_password_after_the_read() {
        let mut app = new_app();
        let path = "underlay-attach-locked.pdf";
        let file = "%PDF-1.4\ntrailer\n<< /Encrypt << /Filter /Standard /V 1 /R 2 \
            /O <95fe1f4219662a774442fb072e3d9f19e9d130ec09a4d0061e78fe920f7ab62f> \
            /U <0102030405060708090a0b0c0d0e0f100102030405060708090a0b0c0d0e0f10> /P -44 >> >>\n%%EOF\n";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(file.as_bytes().to_vec()));
        let task = app.open_pdf_attach_dialog(path);
        assert_eq!(app.active_modal, None, "nothing opens during the read");
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfPassword));
        assert!(app.pdf_attach.is_none());
    }

    /// A different window opened during the read stays, and the Attach
    /// window does not open.
    #[test]
    fn a_window_opened_during_the_read_stays() {
        let mut app = new_app();
        let path = "underlay-attach-other-window.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_pages()));
        let task = app.open_pdf_attach_dialog(path);
        app.active_modal = Some(ModalKind::PointCloudAttach);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PointCloudAttach));
        assert!(app.pdf_attach.is_none());
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Attach underlay cancelled: a different window opened while the file was read.")
        );
    }

    /// A page change shows the size that the read found for that page, and
    /// does not read the file again: bytes that became bad after the read
    /// change nothing.
    #[test]
    fn a_page_change_does_not_read_the_file() {
        let mut app = new_app();
        let path = "underlay-attach-page-change.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_pages()));
        let task = app.open_pdf_attach_dialog(path);
        app.drive_headless_task(task).expect("the read task");
        crate::scene::model::pdf_raster::register_source(path, Arc::new(b"not a PDF".to_vec()));
        let _ = app.update(Message::PdfDialog(PdfDialogMsg::AttachPage(1)));
        let state = app.pdf_attach.as_ref().expect("the window's state");
        assert_eq!(state.selected, vec![1]);
        assert_eq!(state.page_size, "2.0000 × 1.0000");
        let _ = app.update(Message::PdfDialog(PdfDialogMsg::AttachPage(0)));
        let state = app.pdf_attach.as_ref().expect("the window's state");
        assert_eq!(state.page_size, "10.0000 × 5.0000");
        assert_eq!(state.sub_per_master, 1.0);
    }

    /// A page change takes the units of that item from what the read
    /// found (a DGN file has units for each model).
    #[test]
    fn a_page_change_takes_the_units_of_that_item() {
        let mut app = new_app();
        let task = app.read_underlay_for_attach_with(
            UnderlayType::Dgn,
            "two-models.dgn",
            false,
            |_, _, _| {
                let thumb = |label: &str| crate::ui::window::pdf_dialogs::PageThumb {
                    label: label.to_string(),
                    image: None,
                };
                UnderlayAttachRead::Items {
                    pages: vec![thumb("Default"), thumb("Detail")],
                    sizes: vec!["1 × 1".to_string(), "2 × 2".to_string()],
                    units: vec![1000.0, 250.0],
                    remembered: false,
                }
            },
        );
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.pdf_attach.as_ref().map(|s| s.sub_per_master),
            Some(1000.0)
        );
        let _ = app.update(Message::PdfDialog(PdfDialogMsg::AttachPage(1)));
        let state = app.pdf_attach.as_ref().expect("the window's state");
        assert_eq!(state.sub_per_master, 250.0);
        assert_eq!(state.page_size, "2 × 2");
    }

    /// The window open on `first.pdf`, with `second.pdf` in its name list.
    fn window_with_two_files(app: &mut OpenCADStudio, first: &str, second: &str) {
        for path in [first, second] {
            crate::scene::model::pdf_raster::register_source(path, Arc::new(two_pages()));
        }
        let task = app.open_pdf_attach_dialog(first);
        app.drive_headless_task(task).expect("the read task");
        let state = app.pdf_attach.as_mut().expect("the window's state");
        state.existing = vec![
            (file_stem(first), first.to_string()),
            (file_stem(second), second.to_string()),
        ];
    }

    /// A file chosen in the name list is read on a worker: the window stays
    /// open with its options, and shows the file when the read ends.
    #[test]
    fn a_name_change_reads_on_a_worker_and_keeps_the_options() {
        let mut app = new_app();
        let (first, second) = ("underlay-name-first.pdf", "underlay-name-second.pdf");
        window_with_two_files(&mut app, first, second);
        app.pdf_attach.as_mut().unwrap().scale = "4".to_string();
        let task = app.update(Message::PdfDialog(PdfDialogMsg::AttachName(file_stem(
            second,
        ))));
        assert_eq!(
            app.pdf_attach.as_ref().map(|s| s.path.as_str()),
            Some(first),
            "the read runs"
        );
        assert_eq!(last_line(&app), format!("ⓘ Reading {second}…"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfAttach));
        let state = app.pdf_attach.as_ref().expect("the window's state");
        assert_eq!(state.path, second);
        assert_eq!(state.pages.len(), 2);
        assert_eq!(state.page_size, "10.0000 × 5.0000");
        assert_eq!(state.scale, "4", "the options stay");
        assert_eq!(state.pending_path, None);
    }

    /// A reader that panics during a name change gives an error, and the
    /// window stays with its file.
    #[test]
    fn a_reader_panic_during_a_name_change_keeps_the_window() {
        let mut app = new_app();
        let (first, second) = (
            "underlay-name-panic-first.pdf",
            "underlay-name-panic-second.pdf",
        );
        window_with_two_files(&mut app, first, second);
        let task =
            app.read_attach_name_with(UnderlayType::Pdf, second, |_, _, _| panic!("a reader bug"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("PDFATTACH failed: a reader bug")
        );
        assert_eq!(app.active_modal, Some(ModalKind::PdfAttach));
        assert_eq!(
            app.pdf_attach.as_ref().map(|s| s.path.as_str()),
            Some(first)
        );
    }

    /// The read of a file chosen before another changes nothing; the read
    /// of the last file chosen shows it.
    #[test]
    fn an_older_name_read_changes_nothing() {
        let mut app = new_app();
        let (first, second) = (
            "underlay-name-old-first.pdf",
            "underlay-name-old-second.pdf",
        );
        window_with_two_files(&mut app, first, second);
        let older = app.update(Message::PdfDialog(PdfDialogMsg::AttachName(file_stem(
            second,
        ))));
        let newer = app.update(Message::PdfDialog(PdfDialogMsg::AttachName(file_stem(
            first,
        ))));
        app.drive_headless_task(older).expect("the older read");
        assert_eq!(
            app.pdf_attach.as_ref().map(|s| s.path.as_str()),
            Some(first)
        );
        assert_eq!(
            app.pdf_attach
                .as_ref()
                .and_then(|s| s.pending_path.as_deref()),
            Some(first),
            "the newer read is still awaited"
        );
        app.drive_headless_task(newer).expect("the newer read");
        assert_eq!(
            app.pdf_attach
                .as_ref()
                .and_then(|s| s.pending_path.as_deref()),
            None
        );
    }

    /// A window closed during the read is not opened again.
    #[test]
    fn a_closed_window_ignores_the_name_read() {
        let mut app = new_app();
        let (first, second) = (
            "underlay-name-closed-first.pdf",
            "underlay-name-closed-second.pdf",
        );
        window_with_two_files(&mut app, first, second);
        let task = app.update(Message::PdfDialog(PdfDialogMsg::AttachName(file_stem(
            second,
        ))));
        app.active_modal = None;
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, None);
        assert_eq!(
            app.pdf_attach.as_ref().map(|s| s.path.as_str()),
            Some(first)
        );
    }

    /// The window of a second read (Browse) takes the place of the first.
    #[test]
    fn the_window_of_a_second_read_takes_the_place_of_the_first() {
        let mut app = new_app();
        let first = "underlay-attach-first.pdf";
        let second = "underlay-attach-second.pdf";
        for path in [first, second] {
            crate::scene::model::pdf_raster::register_source(path, Arc::new(two_pages()));
        }
        let task = app.open_pdf_attach_dialog(first);
        app.drive_headless_task(task).expect("the read task");
        let task = app.open_pdf_attach_dialog(second);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfAttach));
        assert_eq!(
            app.pdf_attach.as_ref().map(|s| s.path.as_str()),
            Some(second)
        );
        assert!(app.command_line.last_error.is_none());
    }
}
