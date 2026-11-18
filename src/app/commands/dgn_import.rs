//! IMPORT (the Import File window, or a file name on the command line,
//! routed by format) and DGNIMPORT / -DGNIMPORT (the DGN Import window or
//! the command line), and the import itself: a DGN model's layers, text
//! styles, cells and objects added as one undo step, into a new drawing
//! unless the window chose the current one.

use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::io::model_import::ModelFormat;
use crate::modules::insert::dgn_import::{self, DgnImportCommand, DgnImportRequest, ReferenceMode};
use crate::ui::window::import_dialogs::{
    unsupported_reason, DgnImportState, DgnReadFrom, DgnWindowRead, ImportDialogMsg,
    ImportFileState, Support, FORMATS,
};

/// How many files the Import File window offers as recent.
const RECENT: usize = 5;

/// Picks a file and registers its bytes (the web build reads them only
/// here); `None` when the picker is cancelled.
pub(super) async fn pick(title: String, filters: Vec<(String, Vec<String>)>) -> Option<String> {
    let mut dialog = crate::sys::file_dialog().set_title(title.as_str());
    for (name, extensions) in &filters {
        let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(name.as_str(), &extensions);
    }
    let handle = dialog.pick_file().await?;
    let path = crate::sys::handle_path(&handle)
        .to_string_lossy()
        .into_owned();
    let bytes = handle.read().await;
    crate::scene::model::pdf_raster::register_source(&path, Arc::new(bytes));
    Some(path)
}

fn dgn_filters() -> Vec<(String, Vec<String>)> {
    vec![
        (
            "MicroStation DGN (*.dgn)".to_string(),
            vec!["dgn".to_string()],
        ),
        ("All DGN Files (*.*)".to_string(), vec!["*".to_string()]),
    ]
}

fn extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn exists(path: &str) -> bool {
    crate::scene::model::pdf_raster::source_bytes(path).is_some()
}

/// Whether the file is a DGN model DGNIMPORT reads.
fn is_dgn(path: &str) -> bool {
    crate::scene::model::underlay_vector::dgn_import_models(path)
        .and_then(|models| models.into_iter().next())
        .is_some_and(|model| {
            crate::scene::model::underlay_vector::dgn_import(path, &model, None).is_some()
        })
}

/// What the DGN Import window shows of a file: its design models, the first
/// model read with `code_page`, and a thumbnail of each model (sheet models
/// are not imported). None when it is no DGN model. It reads the file, so
/// it runs on a worker.
fn read_dgn_window(path: &str, code_page: Option<u16>) -> Option<DgnWindowRead> {
    use crate::scene::model::underlay_vector::{dgn_import, dgn_import_models};
    let models = dgn_import_models(path).unwrap_or_default();
    let import = dgn_import(path, models.first()?, code_page)?;
    let thumbs =
        crate::ui::window::pdf_dialogs::item_thumbs(document::entities::UnderlayType::Dgn, path)
            .into_iter()
            .filter(|m| models.contains(&m.label))
            .collect();
    Some(DgnWindowRead {
        models,
        import,
        thumbs,
    })
}

/// The importer a file goes to.
enum Route {
    Pdf,
    /// A DGN model if the read on a worker finds one; else the error.
    Dgn(String),
    Model(ModelFormat),
    Wmf,
    Svg,
    Tds,
    Obj,
}

/// The importer for `path` chosen as `format` (an index of [`FORMATS`]; `None`
/// follows the extension), or why there is none.
fn route(path: &str, format: Option<usize>) -> Result<Route, String> {
    let f = format.map(|k| &FORMATS[k]);
    if let Some(f) = f {
        if let Some(reason) = unsupported_reason(f.support) {
            return Err(format!("{}: {reason}", f.name));
        }
    }
    let ext = extension(path);
    let by_extension = f.is_none_or(|f| f.support == Support::ByExtension);
    // Wavefront OBJ is not among the listed types; All Files still reads it.
    if by_extension && ext == "obj" {
        return Ok(Route::Obj);
    }
    // The chosen format, or the one whose patterns name the extension.
    let named = if by_extension {
        FORMATS
            .iter()
            .find(|f| f.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)))
    } else {
        f
    };
    match named.map(|f| f.name) {
        Some("ACIS") => return Ok(Route::Model(ModelFormat::Sat)),
        Some("STEP") => return Ok(Route::Model(ModelFormat::Step)),
        Some("IGES") => return Ok(Route::Model(ModelFormat::Iges)),
        Some("Metafile") => return Ok(Route::Wmf),
        Some("SVG") => return Ok(Route::Svg),
        Some("3D Studio") => return Ok(Route::Tds),
        _ => {}
    }
    let pdf = if by_extension {
        ext == "pdf"
    } else {
        f.is_some_and(|f| f.name == "PDF Files")
    };
    if pdf {
        return Ok(Route::Pdf);
    }
    let dgn_format = f.is_some_and(|f| f.name.contains("DGN"));
    let not_dgn = if dgn_format || ext == "dgn" {
        crate::t!("Invalid or unsupported DGN file.").into_owned()
    } else {
        // Any other extension: the format whose patterns name it.
        let named = FORMATS
            .iter()
            .find(|f| f.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)));
        match named.and_then(|f| unsupported_reason(f.support).map(|r| (f, r))) {
            Some((f, reason)) => format!("{}: {reason}", f.name),
            None => crate::t!("OpenCAD Studio has no importer for this format.").into_owned(),
        }
    };
    // Whether the file is a DGN model is found by a read on a worker.
    if dgn_format || by_extension {
        return Ok(Route::Dgn(not_dgn));
    }
    Err(not_dgn)
}

impl OpenCADStudio {
    /// IMPORT, DGNIMPORT and -DGNIMPORT; `None` defers to the other
    /// families.
    pub(super) fn dispatch_import(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = match cmd.trim().split_once(char::is_whitespace) {
            Some((verb, rest)) => (verb.to_ascii_uppercase(), rest.trim().trim_matches('"')),
            None => (cmd.trim().to_ascii_uppercase(), ""),
        };
        let dialog = crate::sys::file_dia();
        match (verb.as_str(), rest.is_empty()) {
            ("IMPORT", true) if dialog => {
                self.open_import_file_dialog();
                Some(Task::none())
            }
            ("IMPORT", true) => {
                use crate::command::{CadCommand, FreeTextValuePromptCommand};
                let command =
                    FreeTextValuePromptCommand::new("_IMPORTFILE", "Enter import file name:");
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            // A file named after IMPORT (or typed at its prompt).
            ("IMPORT" | "_IMPORTFILE", false) => Some(self.import_routed(rest, None)),
            ("_IMPORTFILE", true) => Some(Task::none()),
            ("DGNIMPORT", true) if dialog => Some(self.pick_dgn_file()),
            ("DGNIMPORT" | "-DGNIMPORT", _) => {
                use crate::command::CadCommand;
                let command = DgnImportCommand::new();
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                if !rest.is_empty() {
                    return Some(
                        self.feed_command(crate::command::StepInput::Text(rest.to_string())),
                    );
                }
                Some(Task::none())
            }
            _ => None,
        }
    }

    fn pick_dgn_file(&mut self) -> Task<Message> {
        let title = crate::t!("Import DGN File").into_owned();
        Task::perform(pick(title, dgn_filters()), |path| {
            Message::ImportDialog(ImportDialogMsg::DgnPicked(path))
        })
    }

    pub(in crate::app) fn open_import_file_dialog(&mut self) {
        let path = self
            .import_dialogs
            .recent
            .first()
            .cloned()
            .unwrap_or_default();
        self.import_dialogs.file = Some(ImportFileState {
            path,
            format: 0,
            search: String::new(),
            only_supported: false,
            error: None,
        });
        self.active_modal = Some(crate::app::ModalKind::ImportFile);
    }

    /// Reads a file off the UI thread for the DGN Import window, which
    /// opens when the read ends ([`Self::open_dgn_import_dialog`]). `from`
    /// says where an error goes when the file is no DGN model. A panic in
    /// the reader becomes an error on the command line.
    pub(in crate::app) fn read_dgn_for_dialog(
        &mut self,
        path: &str,
        from: DgnReadFrom,
    ) -> Task<Message> {
        self.read_dgn_for_dialog_with(path, from, read_dgn_window)
    }

    /// [`Self::read_dgn_for_dialog`] with the reader given, so a test can
    /// give one that panics.
    fn read_dgn_for_dialog_with(
        &mut self,
        path: &str,
        from: DgnReadFrom,
        reader: fn(&str, Option<u16>) -> Option<DgnWindowRead>,
    ) -> Task<Message> {
        self.command_line.push_info(&format!("Reading {path}…"));
        let code_page = dgn_import::settings().code_page;
        let context = if from == DgnReadFrom::DgnImport {
            "DGNIMPORT"
        } else {
            "IMPORT"
        };
        let path = path.to_string();
        let worker_path = path.clone();
        crate::app::update::background_task(
            context,
            move || reader(&worker_path, code_page).map(Arc::new),
            move |read| Message::ImportDialog(ImportDialogMsg::DgnRead(path, read, from)),
        )
    }

    /// IMPORT with FILEDIA off: finds off the UI thread if the file is a
    /// DGN model; if so, -DGNIMPORT continues with it, else the command
    /// line shows `not_dgn`.
    fn check_dgn_for_import(&mut self, path: &str, not_dgn: String) -> Task<Message> {
        self.check_dgn_for_import_with(path, not_dgn, is_dgn)
    }

    /// [`Self::check_dgn_for_import`] with the check given, so a test can
    /// give one that panics.
    fn check_dgn_for_import_with(
        &mut self,
        path: &str,
        not_dgn: String,
        check: fn(&str) -> bool,
    ) -> Task<Message> {
        self.command_line.push_info(&format!("Reading {path}…"));
        let path = path.to_string();
        let worker_path = path.clone();
        crate::app::update::background_task(
            "IMPORT",
            move || check(&worker_path),
            move |dgn| Message::ImportDialog(ImportDialogMsg::DgnChecked(path, dgn, not_dgn)),
        )
    }

    /// The DGN Import window for a file read off the UI thread; an error
    /// when it is no DGN model, where `from` says. It takes the place of the
    /// Import File window and of its own older copy (Change…). If a
    /// different window opened during the read, the DGN Import window does
    /// not open.
    pub(in crate::app) fn open_dgn_import_dialog(
        &mut self,
        path: &str,
        read: Option<Arc<DgnWindowRead>>,
        from: DgnReadFrom,
    ) {
        let import_file_open = self.active_modal == Some(crate::app::ModalKind::ImportFile);
        let Some(read) = read else {
            match from {
                DgnReadFrom::ImportFileWindow(error) if import_file_open => {
                    if let Some(s) = self.import_dialogs.file.as_mut() {
                        s.error = Some(error);
                    }
                }
                DgnReadFrom::ImportFileWindow(error) | DgnReadFrom::Import(error) => {
                    self.command_line.push_error(&error)
                }
                DgnReadFrom::DgnImport => self
                    .command_line
                    .push_error(crate::t!("Invalid or unsupported DGN file.").as_ref()),
            }
            return;
        };
        if self.active_modal.is_some_and(|modal| {
            !matches!(
                modal,
                crate::app::ModalKind::DgnImport | crate::app::ModalKind::ImportFile
            )
        }) {
            self.command_line.push_error(
                "DGN Import cancelled: a different window opened while the file was read.",
            );
            return;
        }
        if from != DgnReadFrom::DgnImport {
            self.remember_import(path);
        }
        if import_file_open {
            self.close_active_modal();
        }
        let import = &read.import;
        // The options chosen last (DGNIMPORTMODE and the kept import
        // settings when the window opens) stay when the file is changed.
        let previous = self.import_dialogs.dgn.take();
        let saved = dgn_import::settings();
        let keep =
            |f: fn(&DgnImportState) -> bool, default: bool| previous.as_ref().map_or(default, f);
        self.import_dialogs.dgn = Some(DgnImportState {
            path: path.to_string(),
            version: if import.v7 { "V7" } else { "V8" },
            models: read.thumbs.clone(),
            model: 0,
            current_drawing: keep(|s| s.current_drawing, saved.mode != 0),
            prefix_duplicates: keep(|s| s.prefix_duplicates, saved.mode == 2),
            sub_units: keep(|s| s.sub_units, false),
            explode_text_nodes: keep(|s| s.explode_text_nodes, saved.explode_text_nodes),
            code_page: previous.as_ref().map_or(saved.code_page, |s| s.code_page),
            references: previous.as_ref().map_or(saved.references, |s| s.references),
            existing_reference: previous
                .as_ref()
                .map_or(saved.existing_reference, |s| s.existing_reference),
            units: import.unit_names.clone(),
            mapping_tab: previous.as_ref().map_or(0, |s| s.mapping_tab),
            mapping: [0, 1, 2, 3].map(|tab| dgn_import::mapping_rows(import, tab)),
        });
        self.active_modal = Some(crate::app::ModalKind::DgnImport);
    }

    /// Reads the window's model again off the UI thread with the window's
    /// code page; [`Self::apply_dgn_decode`] shows its mapping rows and
    /// unit names when the read ends.
    fn decode_dgn_model(&mut self) -> Task<Message> {
        self.decode_dgn_model_with(crate::scene::model::underlay_vector::dgn_import)
    }

    /// [`Self::decode_dgn_model`] with the reader given, so a test can give
    /// one that panics.
    fn decode_dgn_model_with(
        &mut self,
        reader: fn(
            &str,
            &str,
            Option<u16>,
        ) -> Option<crate::scene::model::underlay_vector::DgnImport>,
    ) -> Task<Message> {
        let Some(s) = self.import_dialogs.dgn.as_ref() else {
            return Task::none();
        };
        let Some(model) = s.models.get(s.model).map(|m| m.label.clone()) else {
            return Task::none();
        };
        let path = s.path.clone();
        let code_page = s.code_page;
        let worker_path = path.clone();
        let worker_model = model.clone();
        crate::app::update::background_task(
            "DGNIMPORT",
            move || reader(&worker_path, &worker_model, code_page).map(Arc::new),
            move |import| {
                Message::ImportDialog(ImportDialogMsg::DgnDecoded(path, model, code_page, import))
            },
        )
    }

    /// The mapping rows and unit names of a model read again. A read for a
    /// file, model or code page that the window no longer shows is dropped.
    fn apply_dgn_decode(
        &mut self,
        path: &str,
        model: &str,
        code_page: Option<u16>,
        import: Option<Arc<crate::scene::model::underlay_vector::DgnImport>>,
    ) {
        let (Some(s), Some(import)) = (self.import_dialogs.dgn.as_mut(), import) else {
            return;
        };
        let current = s.path == path
            && s.code_page == code_page
            && s.models.get(s.model).is_some_and(|m| m.label == model);
        if current {
            s.mapping = [0, 1, 2, 3].map(|tab| dgn_import::mapping_rows(&import, tab));
            s.units = import.unit_names.clone();
        }
    }

    fn remember_import(&mut self, path: &str) {
        let recent = &mut self.import_dialogs.recent;
        recent.retain(|p| p != path);
        recent.insert(0, path.to_string());
        recent.truncate(RECENT);
    }

    /// IMPORT on the command line: the file to its importer.
    fn import_routed(&mut self, path: &str, format: Option<usize>) -> Task<Message> {
        if !exists(path) {
            self.command_line
                .push_error(&format!("File {path} does not exist."));
            return Task::none();
        }
        match route(path, format) {
            Ok(Route::Pdf) => {
                self.remember_import(path);
                self.dispatch_command(&format!("_PDFIMPORTFILE {path}"))
            }
            Ok(Route::Dgn(not_dgn)) => {
                if crate::sys::file_dia() {
                    return self.read_dgn_for_dialog(path, DgnReadFrom::Import(not_dgn));
                }
                self.check_dgn_for_import(path, not_dgn)
            }
            Ok(Route::Model(format)) => {
                self.remember_import(path);
                let i = self.active_tab;
                self.import_model(i, path, format)
            }
            Ok(Route::Wmf) => {
                self.remember_import(path);
                let i = self.active_tab;
                self.import_wmf(i, path, false)
            }
            Ok(Route::Svg) => {
                self.remember_import(path);
                let i = self.active_tab;
                self.import_svg(i, path, false)
            }
            // The options window opens whatever FILEDIA is.
            Ok(Route::Tds) => {
                self.remember_import(path);
                self.read_tds_for_dialog(path)
            }
            Ok(Route::Obj) => {
                self.remember_import(path);
                self.on_obj_import_path_some(path.into())
            }
            Err(error) => {
                self.command_line.push_error(&error);
                Task::none()
            }
        }
    }

    pub(in crate::app) fn update_import_dialog(
        &mut self,
        message: ImportDialogMsg,
    ) -> Task<Message> {
        let i = self.active_tab;
        match message {
            ImportDialogMsg::Help(which) => {
                let help = match which {
                    "dgn" => crate::t!(
                        "Imports a DGN model as drawing objects: levels as layers, cells as blocks."
                    ),
                    "3ds" => std::borrow::Cow::Owned(crate::tr!("import-3ds", "help")),
                    "3ds-material" => {
                        std::borrow::Cow::Owned(crate::tr!("import-3ds-alert", "help"))
                    }
                    _ => {
                        crate::t!("Imports a file of the chosen format with the format's importer.")
                    }
                };
                self.command_line.push_info(help.as_ref());
            }
            // ── Import File ──────────────────────────────────────────────
            ImportDialogMsg::Path(path) => {
                if let Some(s) = self.import_dialogs.file.as_mut() {
                    s.path = path;
                    s.error = None;
                }
            }
            ImportDialogMsg::Recent(n) => {
                let path = self.import_dialogs.recent.get(n).cloned();
                if let (Some(s), Some(path)) = (self.import_dialogs.file.as_mut(), path) {
                    s.path = path;
                    s.error = None;
                }
            }
            ImportDialogMsg::Format(k) => {
                if let Some(s) = self.import_dialogs.file.as_mut() {
                    s.format = k.min(FORMATS.len() - 1);
                    s.error = None;
                }
            }
            ImportDialogMsg::Search(text) => {
                if let Some(s) = self.import_dialogs.file.as_mut() {
                    s.search = text;
                }
            }
            ImportDialogMsg::OnlySupported(on) => {
                if let Some(s) = self.import_dialogs.file.as_mut() {
                    s.only_supported = on;
                }
            }
            ImportDialogMsg::Browse => {
                let Some(s) = self.import_dialogs.file.as_ref() else {
                    return Task::none();
                };
                let f = &FORMATS[s.format];
                let filters = vec![(
                    format!("{} ({})", f.name, f.patterns),
                    f.extensions.iter().map(|e| e.to_string()).collect(),
                )];
                let title = crate::t!("Import File").into_owned();
                return Task::perform(pick(title, filters), |path| {
                    Message::ImportDialog(ImportDialogMsg::Picked(path))
                });
            }
            ImportDialogMsg::Picked(Some(path)) => {
                if let Some(s) = self.import_dialogs.file.as_mut() {
                    s.path = path;
                    s.error = None;
                }
            }
            ImportDialogMsg::Picked(None) => {}
            ImportDialogMsg::ImportOk => {
                let Some(s) = self.import_dialogs.file.as_ref() else {
                    return Task::none();
                };
                let path = s.path.trim().trim_matches('"').to_string();
                let format = s.format;
                let error = if path.is_empty() {
                    Some(crate::t!("Filename cannot be blank").into_owned())
                } else if !exists(&path) {
                    Some(format!("File {path} does not exist."))
                } else {
                    None
                };
                let routed = match error {
                    Some(error) => Err(error),
                    None => route(&path, Some(format)),
                };
                match routed {
                    Err(error) => {
                        if let Some(s) = self.import_dialogs.file.as_mut() {
                            s.error = Some(error);
                        }
                    }
                    Ok(Route::Pdf) => {
                        self.remember_import(&path);
                        self.close_active_modal();
                        return self.dispatch_command(&format!("_PDFIMPORTFILE {path}"));
                    }
                    // The Import File window stays open during the read: the
                    // DGN Import window takes its place, or it shows the
                    // error.
                    Ok(Route::Dgn(not_dgn)) => {
                        return self
                            .read_dgn_for_dialog(&path, DgnReadFrom::ImportFileWindow(not_dgn));
                    }
                    Ok(Route::Model(kind)) => {
                        self.remember_import(&path);
                        self.close_active_modal();
                        return self.import_model(i, &path, kind);
                    }
                    Ok(Route::Wmf) => {
                        self.remember_import(&path);
                        self.close_active_modal();
                        return self.import_wmf(i, &path, false);
                    }
                    Ok(Route::Svg) => {
                        self.remember_import(&path);
                        self.close_active_modal();
                        return self.import_svg(i, &path, false);
                    }
                    Ok(Route::Tds) => {
                        self.remember_import(&path);
                        self.close_active_modal();
                        return self.read_tds_for_dialog(&path);
                    }
                    Ok(Route::Obj) => {
                        self.remember_import(&path);
                        self.close_active_modal();
                        return self.on_obj_import_path_some(path.into());
                    }
                }
            }
            // ── 3D Studio Import ─────────────────────────────────────────
            ImportDialogMsg::TdsChange => {
                let title = crate::t!("Import File").into_owned();
                let filters = vec![("3D Studio (*.3ds)".to_string(), vec!["3ds".to_string()])];
                return Task::perform(pick(title, filters), |path| {
                    Message::ImportDialog(ImportDialogMsg::TdsPicked(path))
                });
            }
            ImportDialogMsg::TdsPicked(Some(path)) => return self.read_tds_for_dialog(&path),
            ImportDialogMsg::TdsRead(path, scene) => self.open_tds_import_dialog(&path, scene),
            ImportDialogMsg::TdsConverted(tab_id, generation, request, scene, import) => {
                self.finish_tds_import(tab_id, generation, *request, scene, *import)
            }
            ImportDialogMsg::TdsPicked(None) => {}
            ImportDialogMsg::TdsCheck(k, on) => {
                if let Some(c) = self
                    .import_dialogs
                    .tds
                    .as_mut()
                    .and_then(|s| s.checked.get_mut(k))
                {
                    *c = on;
                }
            }
            ImportDialogMsg::TdsAll | ImportDialogMsg::TdsClear => {
                let on = matches!(message, ImportDialogMsg::TdsAll);
                if let Some(s) = self.import_dialogs.tds.as_mut() {
                    s.checked.iter_mut().for_each(|c| *c = on);
                }
            }
            ImportDialogMsg::TdsFilter(k) => {
                if let Some(s) = self.import_dialogs.tds.as_mut() {
                    s.filter = k.min(4);
                }
            }
            ImportDialogMsg::TdsSearch(text) => {
                if let Some(s) = self.import_dialogs.tds.as_mut() {
                    s.search = text;
                }
            }
            ImportDialogMsg::TdsLayers(k) => {
                if let Some(s) = self.import_dialogs.tds.as_mut() {
                    s.layers = k.min(3);
                }
            }
            ImportDialogMsg::TdsMulti(k) => {
                if let Some(s) = self.import_dialogs.tds.as_mut() {
                    s.multi = k.min(3);
                }
            }
            ImportDialogMsg::TdsOk => {
                let Some(state) = self.import_dialogs.tds.take() else {
                    return Task::none();
                };
                if !state.checked.iter().any(|c| *c) {
                    self.import_dialogs.tds = Some(state);
                    return Task::none();
                }
                self.close_active_modal();
                return self.start_tds_import(i, state);
            }
            // ── Multiple-material mesh ──────────────────────────────────
            ImportDialogMsg::TdsMatChoice(k) => {
                if let Some(s) = self.import_dialogs.tds_material.as_mut() {
                    s.choice = k.min(2);
                    s.list_open = false;
                }
            }
            ImportDialogMsg::TdsMatList(open) => {
                if let Some(s) = self.import_dialogs.tds_material.as_mut() {
                    s.list_open = open && s.choice == 2;
                }
            }
            ImportDialogMsg::TdsMatPick(k) => {
                if let Some(s) = self.import_dialogs.tds_material.as_mut() {
                    let count = s.queue.first().map_or(0, |(_, m)| m.len());
                    s.choice = 2;
                    s.material = k.min(count);
                    s.list_open = false;
                }
            }
            ImportDialogMsg::TdsMatRemember(on) => {
                if let Some(s) = self.import_dialogs.tds_material.as_mut() {
                    s.remember = on;
                }
            }
            ImportDialogMsg::TdsMatApply => return self.answer_tds_material(),
            // ── DGN Import ───────────────────────────────────────────────
            ImportDialogMsg::DgnChange => return self.pick_dgn_file(),
            ImportDialogMsg::DgnPicked(Some(path)) => {
                return self.read_dgn_for_dialog(&path, DgnReadFrom::DgnImport)
            }
            ImportDialogMsg::DgnRead(path, read, from) => {
                self.open_dgn_import_dialog(&path, read, from)
            }
            ImportDialogMsg::DgnChecked(path, dgn, not_dgn) => {
                if !dgn {
                    self.command_line.push_error(&not_dgn);
                    return Task::none();
                }
                self.remember_import(&path);
                return self.dispatch_command(&format!("-DGNIMPORT {path}"));
            }
            ImportDialogMsg::DgnDecoded(path, model, code_page, import) => {
                self.apply_dgn_decode(&path, &model, code_page, import)
            }
            ImportDialogMsg::DgnPrepared(tab_id, generation, request, prepared) => {
                return self.finish_dgn_import(tab_id, generation, *request, *prepared);
            }
            ImportDialogMsg::DgnPicked(None) => {}
            ImportDialogMsg::DgnModel(k) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    if k < s.models.len() {
                        s.model = k;
                        // Re-decode the mapping rows for the page the window
                        // chose, so the layer names match what the import uses.
                        return self.decode_dgn_model();
                    }
                }
            }
            ImportDialogMsg::DgnCurrent(on) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    s.current_drawing = on;
                }
            }
            ImportDialogMsg::DgnPrefix(on) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    s.prefix_duplicates = on;
                }
            }
            ImportDialogMsg::DgnSub(on) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    s.sub_units = on;
                }
            }
            ImportDialogMsg::DgnExplode(on) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    s.explode_text_nodes = on;
                }
            }
            ImportDialogMsg::DgnCodePage(label) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    // The picker labels map to the page the parser takes; an
                    // unknown label keeps the dialog as it was.
                    let Some(code_page) =
                        crate::modules::insert::dgn_import::code_page_from_label(&label)
                    else {
                        return Task::none();
                    };
                    s.code_page = code_page;
                    // Re-decode the mapping rows and unit names with the new
                    // page, the way switching models does, so the preview
                    // layer names match the level names the import will use.
                    return self.decode_dgn_model();
                }
            }
            ImportDialogMsg::DgnReferences(mode) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    s.references = mode;
                }
            }
            ImportDialogMsg::DgnExisting(choice) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    s.existing_reference = choice;
                }
            }
            ImportDialogMsg::DgnMappingTab(tab) => {
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    s.mapping_tab = tab.min(3);
                }
            }
            ImportDialogMsg::DgnOk => {
                let Some(s) = self.import_dialogs.dgn.as_ref() else {
                    return Task::none();
                };
                let request = DgnImportRequest {
                    path: s.path.clone(),
                    model: s
                        .models
                        .get(s.model)
                        .map(|m| m.label.clone())
                        .unwrap_or_default(),
                    sub_units: s.sub_units,
                    explode_text_nodes: s.explode_text_nodes,
                    current_drawing: s.current_drawing,
                    prefix_duplicates: s.prefix_duplicates,
                    references: s.references,
                    existing_reference: s.existing_reference,
                    // The window's picker chooses the V7 text code page; it
                    // overrides the code-page token the command line stored.
                    code_page: s.code_page,
                };
                // The window's choices become DGNIMPORTMODE and the kept
                // settings the command line imports with.
                dgn_import::set_settings(dgn_import::DgnImportSettings {
                    mode: match (s.current_drawing, s.prefix_duplicates) {
                        (false, _) => 0,
                        (true, false) => 1,
                        (true, true) => 2,
                    },
                    explode_text_nodes: s.explode_text_nodes,
                    references: s.references,
                    existing_reference: s.existing_reference,
                    code_page: s.code_page,
                });
                self.save_config();
                self.close_active_modal();
                return self.run_dgn_import(i, request);
            }
        }
        Task::none()
    }

    /// The import: the model and its references read and converted off the
    /// UI thread; [`Self::finish_dgn_import`] places the result when it
    /// arrives, into a new drawing (or the current one), one undo step. The
    /// GUI and positional command keep their command-line error reporting;
    /// a panic in the reader becomes an error on the command line.
    pub(in crate::app) fn run_dgn_import(
        &mut self,
        i: usize,
        request: DgnImportRequest,
    ) -> Task<Message> {
        self.run_dgn_import_with(i, request, dgn_import::prepare_import)
    }

    /// [`Self::run_dgn_import`] with the reader given, so a test can give
    /// one that panics.
    fn run_dgn_import_with(
        &mut self,
        i: usize,
        request: DgnImportRequest,
        reader: fn(
            &DgnImportRequest,
        ) -> Result<dgn_import::PreparedImport, dgn_import::DgnImportError>,
    ) -> Task<Message> {
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        self.command_line
            .push_info(&format!("Reading {}…", request.path));
        let worker_request = request.clone();
        crate::app::update::background_task(
            "DGNIMPORT",
            move || reader(&worker_request),
            move |prepared| {
                Message::ImportDialog(ImportDialogMsg::DgnPrepared(
                    tab_id,
                    generation,
                    Box::new(request),
                    Box::new(prepared),
                ))
            },
        )
    }

    /// Places a DGN import read off the UI thread, or shows why it failed.
    /// An import into the current drawing is dropped if that drawing was
    /// closed, replaced or cleared during the read.
    pub(in crate::app) fn finish_dgn_import(
        &mut self,
        tab_id: u64,
        generation: u64,
        request: DgnImportRequest,
        prepared: Result<dgn_import::PreparedImport, dgn_import::DgnImportError>,
    ) -> Task<Message> {
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.report_dgn_error(error);
                return Task::none();
            }
        };
        let i = if request.current_drawing {
            match self.import_target(tab_id, generation) {
                Some(i) => i,
                None => return Task::none(),
            }
        } else {
            self.active_tab
        };
        self.place_dgn_import(i, &request, prepared).0
    }

    fn report_dgn_error(&mut self, error: dgn_import::DgnImportError) {
        match error {
            dgn_import::DgnImportError::InvalidDgn => self
                .command_line
                .push_error(crate::t!("Invalid or unsupported DGN file.").as_ref()),
            dgn_import::DgnImportError::EmptyModel => self
                .command_line
                .push_error(crate::t!("The DGN model has nothing that can be imported.").as_ref()),
            other => {
                let message = other.to_string();
                self.command_line.push_error(&message);
            }
        }
    }

    /// Fallible execution for semantic callers (the automation `dgn_import`
    /// operation), in one step: it reads the files on the UI thread. On
    /// success the count is the model's top-level converted entities,
    /// excluding block-definition members and any reference entities
    /// attached afterwards.
    pub(in crate::app) fn try_run_dgn_import(
        &mut self,
        i: usize,
        request: DgnImportRequest,
    ) -> Result<(Task<Message>, usize), dgn_import::DgnImportError> {
        let prepared = dgn_import::prepare_import(&request)?;
        Ok(self.place_dgn_import(i, &request, prepared))
    }

    /// Places a prepared import, one undo step: into a new drawing unless
    /// the request chose drawing `i`; then the model's reference
    /// attachments as the request says. The count is as for
    /// [`Self::try_run_dgn_import`].
    fn place_dgn_import(
        &mut self,
        i: usize,
        request: &DgnImportRequest,
        prepared: dgn_import::PreparedImport,
    ) -> (Task<Message>, usize) {
        let dgn_import::PreparedImport {
            import,
            converted,
            references,
        } = prepared;
        let entities = converted.entities.len();
        let mut task = Task::none();
        let i = if request.current_drawing {
            i
        } else {
            task = self.update(Message::TabNew);
            self.active_tab
        };
        self.push_undo_snapshot(i, "DGNIMPORT");
        let naming = if !request.current_drawing {
            Naming::Number
        } else if request.prefix_duplicates {
            let stem = std::path::Path::new(&request.path.replace('\\', "/"))
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let prefix = format!("{stem} - {}$0$", request.model);
            Naming::Prefix(dgn_import::symbol_name(&prefix))
        } else {
            Naming::Keep
        };
        let layers = place_converted(&mut self.tabs[i].scene, converted, &naming);
        if request.references != ReferenceMode::Ignore {
            let k = dgn_import::unit_scale(&import, request.sub_units);
            // A reference is placed on the model's default level.
            let layer = import
                .levels
                .get(&64)
                .map(|l| dgn_import::symbol_name(&l.0))
                .unwrap_or_else(|| "0".to_string());
            let layer = layers.get(&layer).cloned().unwrap_or(layer);
            for reference in references {
                self.attach_dgn_reference(i, request, reference, k, &layer);
            }
        }
        if !request.current_drawing {
            self.tabs[i].scene.fit_all();
        }
        self.tabs[i].dirty = true;
        self.command_line.push_output("Regenerating model.");
        self.refresh_layer_panel();
        self.refresh_properties();
        (task, entities)
    }

    /// One reference attachment, read off the UI thread with the import
    /// ([`dgn_import::prepare_import`]): its file translated to
    /// "<file>-<model>.dwg" beside it and attached as the external reference
    /// "<file> - <model>", or the file attached as a DGN underlay, at the
    /// attachment's place.
    fn attach_dgn_reference(
        &mut self,
        i: usize,
        request: &DgnImportRequest,
        prepared: dgn_import::PreparedReference,
        units: f64,
        layer: &str,
    ) {
        let dgn_import::PreparedReference {
            reference,
            file,
            model,
            converted,
        } = prepared;
        let path = file.to_string_lossy().into_owned();
        // Sub units scale the placement and the size alike.
        let origin = glam::DVec3::from_array(reference.origin) * units;
        let scale = reference.scale * units;
        if request.references == ReferenceMode::Underlay {
            let mut placed = crate::modules::insert::pdf_attach::underlays_for_pages(
                document::entities::UnderlayType::Dgn,
                std::slice::from_ref(&model),
                origin,
                scale,
                reference.rotation,
            );
            let scene = &mut self.tabs[i].scene;
            for (page, mut entity) in placed.drain(..) {
                let definition = crate::modules::insert::pdf_attach::ensure_underlay_definition(
                    &mut scene.document,
                    document::entities::UnderlayType::Dgn,
                    &path,
                    &page,
                );
                if let document::EntityType::Underlay(underlay) = &mut entity {
                    underlay.definition_handle = definition;
                }
                entity.common_mut().layer = layer.to_string();
                scene.add_entity(entity);
            }
            return;
        }
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = dgn_import::reference_drawing(&file, &model);
        // A converted model is written; none keeps the drawing on disk.
        if let Some(converted) = converted {
            let mut scene = crate::scene::Scene::new();
            place_converted(&mut scene, converted, &Naming::Number);
            if let Err(error) = crate::io::save(&scene.document, &target) {
                self.command_line.push_error(&error);
                return;
            }
        }
        let xref = crate::modules::insert::xattach::XrefAttachRequest {
            path: target.to_string_lossy().into_owned(),
            overlay: false,
            path_type: crate::io::xref_model::Pathtype::Relative,
        };
        let name = dgn_import::symbol_name(&format!("{stem} - {model}"));
        let full = crate::modules::insert::xattach::resolve_xref_store_path(&xref.path, None);
        self.command_line
            .push_output(&format!("Attach Xref \"{name}\": {full}"));
        let host = self.tabs[i].current_path.clone();
        let block = crate::modules::insert::xattach::prepare_named_xref_definition(
            &mut self.tabs[i].scene,
            &xref,
            host.as_deref(),
            &name,
        );
        if host.is_none() {
            self.tabs[i].xref_relative_on_save.insert(block.clone());
        }
        self.command_line
            .push_output(&format!("\"{name}\" loaded: {full}"));
        let mut insert = document::entities::Insert::new(
            block,
            document::types::Vector3::new(origin.x, origin.y, origin.z),
        )
        .with_scale(scale, scale, scale)
        .with_rotation(reference.rotation);
        insert.common.layer = layer.to_string();
        insert.common.linetype = "Continuous".to_string();
        self.tabs[i]
            .scene
            .add_entity(document::EntityType::Insert(insert));
        self.refresh_xref_manager();
    }
}

/// How imported names meet the names a drawing already has: a new drawing
/// numbers a clashing layer "<name>_1" and a clashing block or text style
/// "<name>1" (layer 0 and the Standard style are shared); the current
/// drawing either keeps its own definitions or prefixes every imported name
/// (layers 0 and Defpoints excepted).
enum Naming {
    Number,
    Keep,
    Prefix(String),
}

impl Naming {
    fn name(&self, kind: char, name: &str, taken: &dyn Fn(&str) -> bool) -> String {
        match self {
            Naming::Keep => name.to_string(),
            Naming::Prefix(p) => {
                let shared = kind == 'l' && (name == "0" || name.eq_ignore_ascii_case("Defpoints"));
                if shared {
                    name.to_string()
                } else {
                    format!("{p}{name}")
                }
            }
            Naming::Number => {
                let shared = (kind == 'l' && name == "0")
                    || (kind == 's' && name.eq_ignore_ascii_case("Standard"));
                if shared || !taken(name) {
                    return name.to_string();
                }
                let sep = if kind == 'l' { "_" } else { "" };
                (1..)
                    .map(|n| format!("{name}{sep}{n}"))
                    .find(|n| !taken(n))
                    .unwrap_or_default()
            }
        }
    }
}

/// Adds converted objects to a drawing under the naming rule; returns the
/// layer names as placed.
fn place_converted(
    scene: &mut crate::scene::Scene,
    converted: dgn_import::Converted,
    naming: &Naming,
) -> HashMap<String, String> {
    let mut layers: HashMap<String, String> = HashMap::new();
    for (name, color) in &converted.layers {
        let doc = &scene.document;
        let taken = |n: &str| doc.layers.iter().any(|l| l.name.eq_ignore_ascii_case(n));
        let target = naming.name('l', name, &taken);
        if !scene
            .document
            .layers
            .iter()
            .any(|l| l.name.eq_ignore_ascii_case(&target))
        {
            let mut layer = document::tables::Layer::new(target.as_str());
            layer.handle = scene.document.allocate_handle();
            layer.color = *color;
            if converted.layers_off.contains(name) {
                layer.flags.off = true;
            }
            let _ = scene.document.layers.add(layer);
        }
        layers.insert(name.clone(), target);
    }
    let mut styles: HashMap<String, String> = HashMap::new();
    for (name, font) in &converted.styles {
        let doc = &scene.document;
        let taken = |n: &str| {
            doc.text_styles
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(n))
        };
        let target = naming.name('s', name, &taken);
        if !scene
            .document
            .text_styles
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(&target))
        {
            let mut style = document::tables::TextStyle::new(target.as_str());
            style.handle = scene.document.allocate_handle();
            style.font_file = font.clone();
            let _ = scene.document.text_styles.add(style);
        }
        styles.insert(name.clone(), target);
    }
    // Dimension styles take the values the file gives them, their text in
    // the text style they name and closed arrows (a leader none); without
    // values, the drawing's style of the same name (else the current one).
    let mut dim_styles: HashMap<String, String> = HashMap::new();
    for (name, values) in &converted.dim_styles {
        let doc = &scene.document;
        let taken = |n: &str| {
            doc.dim_styles
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(n))
        };
        let target = naming.name('s', name, &taken);
        if !scene
            .document
            .dim_styles
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(&target))
        {
            let doc = &scene.document;
            let base = values.clone().or_else(|| {
                doc.dim_styles
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(name))
                    .or_else(|| {
                        doc.dim_styles.iter().find(|s| {
                            s.name
                                .eq_ignore_ascii_case(&doc.header.current_dimstyle_name)
                        })
                    })
                    .cloned()
            });
            let mut style =
                base.unwrap_or_else(|| document::tables::DimStyle::new(target.as_str()));
            if values.is_some() {
                let text = styles
                    .get(&style.dimtxsty)
                    .cloned()
                    .unwrap_or_else(|| style.dimtxsty.clone());
                if let Some(t) = scene
                    .document
                    .text_styles
                    .iter()
                    .find(|t| t.name.eq_ignore_ascii_case(&text))
                {
                    style.dimtxsty = t.name.clone();
                    style.dimtxsty_handle = t.handle;
                }
                let arrow = crate::entities::arrowheads::resolve(&mut scene.document, "_Closed");
                style.dimblk1 = arrow.unwrap_or(document::Handle::NULL);
                style.dimblk2 = style.dimblk1;
                style.dimldrblk =
                    crate::entities::arrowheads::resolve(&mut scene.document, "_None")
                        .unwrap_or(document::Handle::NULL);
            }
            style.name = target.clone();
            style.handle = scene.document.allocate_handle();
            let _ = scene.document.dim_styles.add(style);
        }
        dim_styles.insert(name.clone(), target);
    }
    let mut blocks: HashMap<String, String> = HashMap::new();
    // Names the drawing has or this import gave, upper-cased: one lookup
    // each, however many blocks the file brings.
    let mut claimed: std::collections::HashSet<String> = scene
        .document
        .block_records
        .iter()
        .map(|b| b.name.to_ascii_uppercase())
        .collect();
    for (name, _) in &converted.blocks {
        let taken = |n: &str| claimed.contains(&n.to_ascii_uppercase());
        let target = naming.name('b', name, &taken);
        claimed.insert(target.to_ascii_uppercase());
        blocks.insert(name.clone(), target);
    }
    let fix = |mut entity: document::EntityType| {
        let common = entity.common_mut();
        if let Some(layer) = layers.get(&common.layer) {
            common.layer = layer.clone();
        }
        match &mut entity {
            document::EntityType::Text(t) => {
                if let Some(s) = styles.get(&t.style) {
                    t.style = s.clone();
                }
            }
            document::EntityType::MText(t) => {
                if let Some(s) = styles.get(&t.style) {
                    t.style = s.clone();
                }
            }
            document::EntityType::Insert(insert) => {
                if let Some(b) = blocks.get(&insert.block_name) {
                    insert.block_name = b.clone();
                }
            }
            document::EntityType::Dimension(d) => {
                if let Some(s) = dim_styles.get(&d.base().style_name) {
                    d.base_mut().style_name = s.clone();
                }
            }
            document::EntityType::Leader(l) => {
                if let Some(s) = dim_styles.get(&l.dimension_style) {
                    l.dimension_style = s.clone();
                }
            }
            _ => {}
        }
        entity
    };
    for (name, entities) in converted.blocks {
        let target = &blocks[&name];
        if scene.document.block_records.get(target).is_some() {
            continue;
        }
        let entities = entities
            .into_iter()
            .map(|e| placed_dimension_text(&scene.document, fix(e)))
            .collect();
        if let Ok(handles) =
            scene.define_block_from_owned_entities(entities, target, glam::DVec3::ZERO)
        {
            scene.bind_outline_hatches(&handles);
        }
    }
    let mut handles = Vec::new();
    for entity in converted.entities {
        let entity = placed_dimension_text(&scene.document, fix(entity));
        handles.push(scene.add_entity(entity));
    }
    scene.bind_outline_hatches(&handles);
    layers
}

/// A dimension whose text the user has not placed gets the place its style
/// lays the text out at in this drawing.
fn placed_dimension_text(
    document: &document::Document,
    mut entity: document::EntityType,
) -> document::EntityType {
    if let document::EntityType::Dimension(d) = &mut entity {
        if !d.base().text_user_positioned {
            if let Some(p) = crate::entities::dimension::automatic_text_position(document, d) {
                d.base_mut().text_middle_point = p;
            }
        }
    }
    entity
}

#[cfg(test)]
mod dgn_dialog_tests {
    use super::*;
    use crate::scene::model::underlay_vector::dgn_import;
    use crate::ui::window::pdf_dialogs::PageThumb;

    fn stub_dialog() -> DgnImportState {
        DgnImportState {
            path: "/tmp/no-such-ocs-drawing.dgn".into(),
            version: "V7",
            models: vec![PageThumb {
                label: "Default".into(),
                image: None,
            }],
            model: 0,
            current_drawing: false,
            prefix_duplicates: false,
            sub_units: false,
            explode_text_nodes: false,
            code_page: None,
            references: crate::modules::insert::dgn_import::ReferenceMode::Translate,
            existing_reference: crate::modules::insert::dgn_import::ExistingReference::Prompt,
            units: ("m".into(), "mm".into()),
            mapping_tab: 0,
            mapping: Default::default(),
        }
    }

    #[test]
    fn dialog_code_page_choice_stores_the_v7_page() {
        use crate::modules::insert::dgn_import::DgnImportSettings;
        let _own = crate::modules::insert::dgn_import::OwnSettings::take();
        // The checkbox must win over the stored code page for this window
        // even when a command-line import earlier kept 1250.
        dgn_import::set_settings(DgnImportSettings {
            mode: 0,
            explode_text_nodes: false,
            references: crate::modules::insert::dgn_import::ReferenceMode::Translate,
            existing_reference: crate::modules::insert::dgn_import::ExistingReference::Prompt,
            code_page: Some(1250),
        });
        let mut app = OpenCADStudio::new_for_test();
        app.import_dialogs.dgn = Some(stub_dialog());

        let _ = app.update_import_dialog(ImportDialogMsg::DgnCodePage(
            crate::modules::insert::dgn_import::code_page_label(None),
        ));
        assert_eq!(
            app.import_dialogs.dgn.as_ref().expect("dialog").code_page,
            None
        );

        // Unticking and importing resets the stored page to the default; the
        // stub path only fails the import itself, after the choice is stored.
        let _ = app.update_import_dialog(ImportDialogMsg::DgnOk);
        assert_eq!(dgn_import::settings().code_page, None);

        let mut app = OpenCADStudio::new_for_test();
        app.import_dialogs.dgn = Some(stub_dialog());
        let _ = app.update_import_dialog(ImportDialogMsg::DgnCodePage(
            crate::modules::insert::dgn_import::code_page_label(Some(1250)),
        ));
        let _ = app.update_import_dialog(ImportDialogMsg::DgnOk);
        assert_eq!(dgn_import::settings().code_page, Some(1250));
        // Unpicked defaults stay the plain 1252: the label round-trips.
        assert_eq!(
            crate::modules::insert::dgn_import::code_page_label(None),
            "Windows-1252 (Western, default)"
        );
        assert_eq!(
            crate::modules::insert::dgn_import::code_page_from_label(
                &crate::modules::insert::dgn_import::code_page_label(Some(1257))
            ),
            Some(Some(1257))
        );
    }
}

#[cfg(test)]
mod dgn_worker_tests {
    use super::*;
    use crate::modules::insert::dgn_import::{test_files, ExistingReference, PreparedImport};

    fn app_with_new_drawing() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// A V7 file with one line, registered under a name of its own.
    fn v7_file(name: &str) -> String {
        let path = format!("memory://dgn-worker-{name}-{}.dgn", std::process::id());
        crate::scene::model::pdf_raster::register_source(
            &path,
            Arc::new(test_files::minimal_v7_line()),
        );
        path
    }

    fn junk_file(name: &str) -> String {
        let path = format!("memory://dgn-worker-{name}-{}.dgn", std::process::id());
        crate::scene::model::pdf_raster::register_source(
            &path,
            Arc::new(b"not a design file".to_vec()),
        );
        path
    }

    fn request(path: &str, current_drawing: bool) -> DgnImportRequest {
        DgnImportRequest {
            path: path.to_string(),
            model: "Default".to_string(),
            sub_units: false,
            explode_text_nodes: false,
            current_drawing,
            prefix_duplicates: false,
            references: ReferenceMode::Ignore,
            existing_reference: ExistingReference::Prompt,
            code_page: None,
        }
    }

    fn last_line(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .last()
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    }

    fn lines(app: &OpenCADStudio, i: usize) -> usize {
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| matches!(e, document::EntityType::Line(_)))
            .count()
    }

    fn panicking_check(_: &str) -> bool {
        panic!("check failed")
    }

    fn panicking_window_reader(_: &str, _: Option<u16>) -> Option<DgnWindowRead> {
        panic!("window reader failed")
    }

    fn panicking_import_reader(
        _: &DgnImportRequest,
    ) -> Result<PreparedImport, dgn_import::DgnImportError> {
        panic!("import reader failed")
    }

    fn panicking_decoder(
        _: &str,
        _: &str,
        _: Option<u16>,
    ) -> Option<crate::scene::model::underlay_vector::DgnImport> {
        panic!("decoder failed")
    }

    /// A decoder whose model has unit names that the test file does not
    /// have, so a test sees whether the window took its result.
    fn feet_decoder(
        _: &str,
        _: &str,
        _: Option<u16>,
    ) -> Option<crate::scene::model::underlay_vector::DgnImport> {
        Some(crate::scene::model::underlay_vector::DgnImport {
            unit_names: ("ft".into(), "in".into()),
            ..Default::default()
        })
    }

    /// The window opens only when the read on the worker ends.
    #[test]
    fn the_window_opens_only_when_its_read_ends() {
        let mut app = app_with_new_drawing();
        let path = v7_file("window");
        let task = app.read_dgn_for_dialog(&path, DgnReadFrom::DgnImport);
        assert!(app.active_modal.is_none(), "no window during the read");
        assert_eq!(last_line(&app), format!("ⓘ Reading {path}…"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::DgnImport));
        let s = app.import_dialogs.dgn.as_ref().expect("window state");
        assert_eq!(s.path, path);
        assert_eq!(s.version, "V7");
        assert_eq!(s.units, ("m".to_string(), "mm".to_string()));
    }

    #[test]
    fn a_panic_in_the_window_read_is_an_error() {
        let mut app = app_with_new_drawing();
        let path = v7_file("window-panic");
        let task =
            app.read_dgn_for_dialog_with(&path, DgnReadFrom::DgnImport, panicking_window_reader);
        app.drive_headless_task(task).expect("the read task");
        assert!(app.active_modal.is_none());
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("DGNIMPORT failed: window reader failed")
        );
    }

    /// If a different window opened during the read, the DGN Import window
    /// does not take its place.
    #[test]
    fn a_window_opened_during_the_read_stays() {
        let mut app = app_with_new_drawing();
        let path = v7_file("window-other");
        let task = app.read_dgn_for_dialog(&path, DgnReadFrom::DgnImport);
        app.active_modal = Some(crate::app::ModalKind::About);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::About));
        assert!(app.import_dialogs.dgn.is_none());
        assert!(app
            .command_line
            .last_error
            .as_deref()
            .is_some_and(|e| e.starts_with("DGN Import cancelled")));
    }

    /// IMPORT in the Import File window: the window stays open during the
    /// read; a file that is no DGN model shows the error there, a DGN model
    /// opens the DGN Import window in its place.
    #[test]
    fn the_import_file_window_waits_for_the_read() {
        let mut app = app_with_new_drawing();
        for (path, dgn) in [(junk_file("import-junk"), false), (v7_file("import"), true)] {
            app.open_import_file_dialog();
            let s = app.import_dialogs.file.as_mut().unwrap();
            s.path = path.clone();
            s.format = 1; // MicroStation DGN
            let task = app.update_import_dialog(ImportDialogMsg::ImportOk);
            assert_eq!(app.active_modal, Some(crate::app::ModalKind::ImportFile));
            app.drive_headless_task(task).expect("the read task");
            if dgn {
                assert_eq!(app.active_modal, Some(crate::app::ModalKind::DgnImport));
                assert_eq!(app.import_dialogs.recent.first(), Some(&path));
            } else {
                assert_eq!(app.active_modal, Some(crate::app::ModalKind::ImportFile));
                assert_eq!(
                    app.import_dialogs.file.as_ref().unwrap().error.as_deref(),
                    Some("Invalid or unsupported DGN file.")
                );
                assert!(!app.import_dialogs.recent.contains(&path));
                app.close_active_modal();
            }
        }
    }

    /// IMPORT on the command line or from a script: the error goes to the
    /// command line, also when the Import File window is open (the
    /// automation `run` operation sends IMPORT and the file name apart, so
    /// that window opens first). After OK in an Import File window that
    /// closed during the read, the error also goes to the command line.
    #[test]
    fn a_command_line_import_shows_the_error_on_the_command_line() {
        let mut app = app_with_new_drawing();
        app.open_import_file_dialog();
        let path = junk_file("command-line-junk");
        let from = DgnReadFrom::Import("not a DGN model".into());
        let task = app.read_dgn_for_dialog(&path, from);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("not a DGN model")
        );
        assert!(app.import_dialogs.file.as_ref().unwrap().error.is_none());

        app.active_modal = None;
        let from = DgnReadFrom::ImportFileWindow("closed window".into());
        let task = app.read_dgn_for_dialog(&path, from);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("closed window")
        );
        assert!(app.import_dialogs.file.as_ref().unwrap().error.is_none());
    }

    /// A model number that the window does not list changes nothing and
    /// reads nothing; a listed one is read again on a worker.
    #[test]
    fn only_a_listed_model_is_read_again() {
        let mut app = app_with_new_drawing();
        let path = v7_file("model-choice");
        window(&mut app, &path);
        assert_eq!(app.import_dialogs.dgn.as_ref().unwrap().models.len(), 1);
        let task = app.update_import_dialog(ImportDialogMsg::DgnModel(1));
        assert_eq!(task.units(), 0);
        assert_eq!(app.import_dialogs.dgn.as_ref().unwrap().model, 0);
        let task = app.update_import_dialog(ImportDialogMsg::DgnModel(0));
        assert!(task.units() > 0, "the read runs as a task");
        app.drive_headless_task(task).expect("the decode task");
        assert!(app.command_line.last_error.is_none());
    }

    /// IMPORT with FILEDIA off: -DGNIMPORT starts only for a DGN model.
    #[test]
    fn import_without_the_window_checks_the_file_first() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let junk = junk_file("check-junk");
        let task = app.check_dgn_for_import(&junk, "not a DGN model".into());
        app.drive_headless_task(task).expect("the check task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("not a DGN model")
        );
        assert!(app.tabs[i].active_cmd.is_none());

        let path = v7_file("check");
        let task = app.check_dgn_for_import(&path, "not a DGN model".into());
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "nothing starts during the read"
        );
        app.drive_headless_task(task).expect("the check task");
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "-DGNIMPORT asks for the model"
        );
        assert_eq!(app.import_dialogs.recent.first(), Some(&path));
    }

    #[test]
    fn a_panic_in_the_check_is_an_error() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = v7_file("check-panic");
        let task = app.check_dgn_for_import_with(&path, "not a DGN model".into(), panicking_check);
        app.drive_headless_task(task).expect("the check task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("IMPORT failed: check failed")
        );
        assert!(app.tabs[i].active_cmd.is_none());
    }

    /// The import places nothing itself: the model is read and converted
    /// on a worker, and only the result places the line, as one undo step.
    #[test]
    fn the_import_is_placed_only_when_its_read_ends() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = v7_file("import-current");
        app.finish_all_pending_history();
        let undo = app.tabs[i].history.undo_stack.len();
        let task = app.run_dgn_import(i, request(&path, true));
        assert_eq!(lines(&app, i), 0, "nothing is placed during the read");
        assert_eq!(last_line(&app), format!("ⓘ Reading {path}…"));
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(lines(&app, i), 1);
        app.finish_all_pending_history();
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo + 1);
        assert!(app.tabs[i].dirty);
    }

    #[test]
    fn a_panic_in_the_import_read_is_an_error() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tabs = app.tabs.len();
        let path = v7_file("import-panic");
        let task = app.run_dgn_import_with(i, request(&path, false), panicking_import_reader);
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("DGNIMPORT failed: import reader failed")
        );
        assert_eq!(app.tabs.len(), tabs, "no new drawing");
        assert_eq!(lines(&app, i), 0);
    }

    #[test]
    fn a_file_that_is_no_dgn_model_is_an_error_after_the_read() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = junk_file("import-junk-run");
        let task = app.run_dgn_import(i, request(&path, true));
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Invalid or unsupported DGN file.")
        );
    }

    /// An import into the current drawing is dropped if that drawing was
    /// replaced during the read; an import into a new drawing is not.
    #[test]
    fn only_an_import_into_a_replaced_drawing_is_dropped() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = v7_file("import-replaced");
        let task = app.run_dgn_import(i, request(&path, true));
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(lines(&app, app.active_tab), 0, "dropped");
        assert!(app
            .command_line
            .history
            .iter()
            .any(|entry| entry.text.contains("was replaced")));

        let tabs = app.tabs.len();
        let i = app.active_tab;
        let task = app.run_dgn_import(i, request(&path, false));
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.drive_headless_task(task).expect("the import task");
        assert!(app.tabs.len() > tabs, "a new drawing");
        assert_eq!(lines(&app, app.active_tab), 1);
    }

    fn window(app: &mut OpenCADStudio, path: &str) {
        let task = app.read_dgn_for_dialog(path, DgnReadFrom::DgnImport);
        app.drive_headless_task(task).expect("the read task");
        assert!(app.import_dialogs.dgn.is_some());
    }

    /// A model read again for the code page the window shows changes the
    /// window when it ends; a read for a code page that the window no
    /// longer shows is dropped.
    #[test]
    fn only_a_current_decode_changes_the_window() {
        let mut app = app_with_new_drawing();
        let path = v7_file("decode");
        window(&mut app, &path);
        let task = app.decode_dgn_model_with(feet_decoder);
        app.import_dialogs.dgn.as_mut().unwrap().code_page = Some(1250);
        app.drive_headless_task(task).expect("the decode task");
        let units = &app.import_dialogs.dgn.as_ref().unwrap().units;
        assert_eq!(
            units,
            &("m".to_string(), "mm".to_string()),
            "stale: dropped"
        );

        let task = app.decode_dgn_model_with(feet_decoder);
        app.drive_headless_task(task).expect("the decode task");
        let units = &app.import_dialogs.dgn.as_ref().unwrap().units;
        assert_eq!(units, &("ft".to_string(), "in".to_string()));
    }

    #[test]
    fn a_code_page_change_reads_the_model_on_a_worker() {
        let mut app = app_with_new_drawing();
        let path = v7_file("decode-page");
        window(&mut app, &path);
        let label = crate::modules::insert::dgn_import::code_page_label(Some(1250));
        let task = app.update_import_dialog(ImportDialogMsg::DgnCodePage(label));
        assert!(task.units() > 0, "the read runs as a task");
        assert_eq!(
            app.import_dialogs.dgn.as_ref().unwrap().code_page,
            Some(1250)
        );
        app.drive_headless_task(task).expect("the decode task");
        assert!(app.command_line.last_error.is_none());
    }

    #[test]
    fn a_panic_in_the_decode_is_an_error_and_the_window_stays() {
        let mut app = app_with_new_drawing();
        let path = v7_file("decode-panic");
        window(&mut app, &path);
        let task = app.decode_dgn_model_with(panicking_decoder);
        app.drive_headless_task(task).expect("the decode task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("DGNIMPORT failed: decoder failed")
        );
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::DgnImport));
    }

    /// The UI half of a translated reference: the converted model is
    /// written beside its file and attached as an external reference.
    #[test]
    fn a_translated_reference_is_written_and_attached() {
        let folder = std::env::temp_dir().join(format!("ocs-dgn-attach-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let host = v7_file("attach-host");
        let prepared = dgn_import::prepare_import(&request(&host, true)).expect("read");
        let reference = dgn_import::PreparedReference {
            reference: Default::default(),
            file: folder.join("site.dgn"),
            model: "Default".to_string(),
            converted: Some(prepared.converted.clone()),
        };
        let prepared = PreparedImport {
            references: vec![reference],
            ..prepared
        };
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let mut translate = request(&host, true);
        translate.references = ReferenceMode::Translate;
        let _ = app.place_dgn_import(i, &translate, prepared);
        let written = folder.join("site-Default.dwg").is_file();
        let document = &app.tabs[i].scene.document;
        let inserts: Vec<String> = document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Insert(insert) => Some(insert.block_name.clone()),
                _ => None,
            })
            .collect();
        let _ = std::fs::remove_dir_all(&folder);
        assert!(written, "the drawing is written");
        assert_eq!(inserts, ["site - Default"], "the reference is attached");
        assert!(document.block_records.get("site - Default").is_some());
    }
}
