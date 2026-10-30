//! IMPORT (the Import File window, or a file name on the command line,
//! routed by format) and DGNIMPORT / -DGNIMPORT (the DGN Import window or
//! the command line), and the import itself: a DGN model's layers, text
//! styles, cells and objects added as one undo step, into a new drawing
//! unless the window chose the current one.

use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::io::model_import::ModelFormat;
use crate::modules::insert::dgn_import::{
    self, DgnImportCommand, DgnImportRequest, ExistingReference, ReferenceMode,
};
use crate::ui::window::import_dialogs::{
    unsupported_reason, DgnImportState, ImportDialogMsg, ImportFileState, Support, FORMATS,
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
        ("MicroStation DGN (*.dgn)".to_string(), vec!["dgn".to_string()]),
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
        .is_some_and(|model| crate::scene::model::underlay_vector::dgn_import(path, &model).is_some())
}

/// The importer a file goes to.
enum Route {
    Pdf,
    Dgn,
    Model(ModelFormat),
    Wmf,
    Tds,
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
        Some("3D Studio") => return Ok(Route::Tds),
        _ => {}
    }
    let pdf = if by_extension { ext == "pdf" } else { f.is_some_and(|f| f.name == "PDF Files") };
    if pdf {
        return Ok(Route::Pdf);
    }
    let dgn_format = f.is_some_and(|f| f.name.contains("DGN"));
    if (dgn_format || by_extension) && is_dgn(path) {
        return Ok(Route::Dgn);
    }
    if dgn_format || ext == "dgn" {
        return Err(crate::t!("Invalid or unsupported DGN file.").into_owned());
    }
    // Any other extension: the format whose patterns name it.
    let named = FORMATS
        .iter()
        .find(|f| f.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)));
    Err(match named.and_then(|f| unsupported_reason(f.support).map(|r| (f, r))) {
        Some((f, reason)) => format!("{}: {reason}", f.name),
        None => crate::t!("OpenCAD Studio has no importer for this format.").into_owned(),
    })
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
                let command = FreeTextValuePromptCommand::new("_IMPORTFILE", "Enter import file name:");
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
        let path = self.import_dialogs.recent.first().cloned().unwrap_or_default();
        self.import_dialogs.file = Some(ImportFileState {
            path,
            format: 0,
            search: String::new(),
            only_supported: false,
            error: None,
        });
        self.active_modal = Some(crate::app::ModalKind::ImportFile);
    }

    /// The DGN Import window for a file; an error when it is no DGN model.
    pub(in crate::app) fn open_dgn_import_dialog(&mut self, path: &str) -> Result<(), String> {
        use crate::scene::model::underlay_vector::{dgn_import, dgn_import_models};
        let models = dgn_import_models(path).unwrap_or_default();
        let Some(import) = models.first().and_then(|m| dgn_import(path, m)) else {
            return Err(crate::t!("Invalid or unsupported DGN file.").into_owned());
        };
        // The options chosen last (DGNIMPORTMODE and the kept import
        // settings when the window opens) stay when the file is changed.
        let previous = self.import_dialogs.dgn.take();
        let saved = dgn_import::settings();
        let keep = |f: fn(&DgnImportState) -> bool, default: bool| {
            previous.as_ref().map_or(default, f)
        };
        self.import_dialogs.dgn = Some(DgnImportState {
            path: path.to_string(),
            version: if import.v7 { "V7" } else { "V8" },
            // Sheet models are not imported.
            models: crate::ui::window::pdf_dialogs::item_thumbs(
                codec::entities::UnderlayType::Dgn,
                path,
            )
            .into_iter()
            .filter(|m| models.contains(&m.label))
            .collect(),
            model: 0,
            current_drawing: keep(|s| s.current_drawing, saved.mode != 0),
            prefix_duplicates: keep(|s| s.prefix_duplicates, saved.mode == 2),
            sub_units: keep(|s| s.sub_units, false),
            explode_text_nodes: keep(|s| s.explode_text_nodes, saved.explode_text_nodes),
            references: previous.as_ref().map_or(saved.references, |s| s.references),
            existing_reference: previous
                .as_ref()
                .map_or(saved.existing_reference, |s| s.existing_reference),
            units: import.unit_names.clone(),
            mapping_tab: previous.as_ref().map_or(0, |s| s.mapping_tab),
            mapping: [0, 1, 2, 3].map(|tab| dgn_import::mapping_rows(&import, tab)),
        });
        self.active_modal = Some(crate::app::ModalKind::DgnImport);
        Ok(())
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
            Ok(Route::Dgn) => {
                self.remember_import(path);
                if crate::sys::file_dia() {
                    if let Err(error) = self.open_dgn_import_dialog(path) {
                        self.command_line.push_error(&error);
                    }
                    return Task::none();
                }
                self.dispatch_command(&format!("-DGNIMPORT {path}"))
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
            // The options window opens whatever FILEDIA is.
            Ok(Route::Tds) => {
                self.remember_import(path);
                if let Err(error) = self.open_tds_import_dialog(path) {
                    self.command_line.push_error(&error);
                }
                Task::none()
            }
            Err(error) => {
                self.command_line.push_error(&error);
                Task::none()
            }
        }
    }

    pub(in crate::app) fn update_import_dialog(&mut self, message: ImportDialogMsg) -> Task<Message> {
        let i = self.active_tab;
        match message {
            ImportDialogMsg::Help(which) => {
                let help = match which {
                    "dgn" => crate::t!(
                        "Imports a DGN model as drawing objects: levels as layers, cells as blocks."
                    ),
                    "3ds" => std::borrow::Cow::Owned(crate::tr!("import-3ds", "help")),
                    "3ds-material" => std::borrow::Cow::Owned(crate::tr!("import-3ds-alert", "help")),
                    _ => crate::t!(
                        "Imports a file of the chosen format with the format's importer."
                    ),
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
                    Ok(Route::Dgn) => {
                        self.remember_import(&path);
                        self.close_active_modal();
                        if let Err(error) = self.open_dgn_import_dialog(&path) {
                            self.command_line.push_error(&error);
                        }
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
                    Ok(Route::Tds) => {
                        self.remember_import(&path);
                        self.close_active_modal();
                        if let Err(error) = self.open_tds_import_dialog(&path) {
                            self.command_line.push_error(&error);
                        }
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
            ImportDialogMsg::TdsPicked(Some(path)) => {
                if let Err(error) = self.open_tds_import_dialog(&path) {
                    self.command_line.push_error(&error);
                }
            }
            ImportDialogMsg::TdsPicked(None) => {}
            ImportDialogMsg::TdsCheck(k, on) => {
                if let Some(c) = self.import_dialogs.tds.as_mut().and_then(|s| s.checked.get_mut(k)) {
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
            ImportDialogMsg::TdsMatApply => self.answer_tds_material(),
            // ── DGN Import ───────────────────────────────────────────────
            ImportDialogMsg::DgnChange => return self.pick_dgn_file(),
            ImportDialogMsg::DgnPicked(Some(path)) => {
                if let Err(error) = self.open_dgn_import_dialog(&path) {
                    self.command_line.push_error(&error);
                }
            }
            ImportDialogMsg::DgnPicked(None) => {}
            ImportDialogMsg::DgnModel(k) => {
                use crate::scene::model::underlay_vector::dgn_import;
                if let Some(s) = self.import_dialogs.dgn.as_mut() {
                    if let Some(model) = s.models.get(k) {
                        if let Some(import) = dgn_import(&s.path, &model.label) {
                            s.mapping = [0, 1, 2, 3].map(|tab| dgn_import::mapping_rows(&import, tab));
                            s.units = import.unit_names.clone();
                        }
                        s.model = k;
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
                });
                self.save_config();
                self.close_active_modal();
                return self.run_dgn_import(i, request);
            }
        }
        Task::none()
    }

    /// The import: into a new drawing (or the current one), one undo step;
    /// then the model's reference attachments as the request says.
    pub(in crate::app) fn run_dgn_import(&mut self, i: usize, request: DgnImportRequest) -> Task<Message> {
        let Some(import) =
            crate::scene::model::underlay_vector::dgn_import(&request.path, &request.model)
        else {
            self.command_line
                .push_error(crate::t!("Invalid or unsupported DGN file.").as_ref());
            return Task::none();
        };
        let converted =
            dgn_import::convert(&import, request.sub_units, request.explode_text_nodes);
        let no_references =
            request.references == ReferenceMode::Ignore || import.references.is_empty();
        if converted.entities.is_empty() && no_references {
            self.command_line.push_error(
                crate::t!("The DGN model has nothing that can be imported.").as_ref(),
            );
            return Task::none();
        }
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
            for reference in &import.references {
                self.attach_dgn_reference(i, &request, reference, k, &layer);
            }
        }
        if !request.current_drawing {
            self.tabs[i].scene.fit_all();
        }
        self.tabs[i].dirty = true;
        self.command_line.push_output("Regenerating model.");
        self.refresh_layer_panel();
        self.refresh_properties();
        task
    }

    /// One reference attachment: its file translated to "<file>-<model>.dwg"
    /// beside it and attached as the external reference "<file> - <model>",
    /// or the file attached as a DGN underlay, at the attachment's place.
    fn attach_dgn_reference(
        &mut self,
        i: usize,
        request: &DgnImportRequest,
        reference: &crate::scene::model::underlay_vector::DgnReference,
        units: f64,
        layer: &str,
    ) {
        use crate::scene::model::underlay_vector::{dgn_import, dgn_import_models};
        let host_dir = std::path::Path::new(&request.path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        // A relative name is beside the host file; failing that, the file
        // of that name in the host's folder.
        let named = std::path::Path::new(&reference.file.replace('\\', "/")).to_path_buf();
        let joined = host_dir.join(&named);
        let file = if joined.is_file() {
            joined
        } else {
            host_dir.join(named.file_name().unwrap_or_default())
        };
        let path = file.to_string_lossy().into_owned();
        if crate::scene::model::pdf_raster::source_bytes(&path).is_none() {
            return;
        }
        let Some(model) = (if reference.model.is_empty() {
            dgn_import_models(&path).and_then(|m| m.into_iter().next())
        } else {
            Some(reference.model.clone())
        }) else {
            return;
        };
        // Sub units scale the placement and the size alike.
        let origin = glam::DVec3::from_array(reference.origin) * units;
        let scale = reference.scale * units;
        if request.references == ReferenceMode::Underlay {
            let mut placed = crate::modules::insert::pdf_attach::underlays_for_pages(
                codec::entities::UnderlayType::Dgn,
                std::slice::from_ref(&model),
                origin,
                scale,
                reference.rotation,
            );
            let scene = &mut self.tabs[i].scene;
            for (page, mut entity) in placed.drain(..) {
                let definition = crate::modules::insert::pdf_attach::ensure_underlay_definition(
                    &mut scene.document,
                    codec::entities::UnderlayType::Dgn,
                    &path,
                    &page,
                );
                if let codec::EntityType::Underlay(underlay) = &mut entity {
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
        // The model name comes from the file: made a plain file name so the
        // drawing is written in the reference's folder.
        let model_name = dgn_import::symbol_name(&model);
        let target = file.with_file_name(format!("{stem}-{model_name}.dwg"));
        // ponytail: "Prompt to overwrite" asks nothing and keeps the existing
        // drawing, like "Do not overwrite"; add a confirmation if wanted.
        let write = !target.is_file() || request.existing_reference == ExistingReference::Overwrite;
        if write {
            let Some(source) = dgn_import(&path, &model) else {
                return;
            };
            let mut scene = crate::scene::Scene::new();
            let converted = dgn_import::convert(&source, false, request.explode_text_nodes);
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
        let mut insert = codec::entities::Insert::new(
            block,
            codec::types::Vector3::new(origin.x, origin.y, origin.z),
        )
        .with_scale(scale, scale, scale)
        .with_rotation(reference.rotation);
        insert.common.layer = layer.to_string();
        insert.common.linetype = "Continuous".to_string();
        self.tabs[i]
            .scene
            .add_entity(codec::EntityType::Insert(insert));
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
                let shared = kind == 'l'
                    && (name == "0" || name.eq_ignore_ascii_case("Defpoints"));
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
        if !scene.document.layers.iter().any(|l| l.name.eq_ignore_ascii_case(&target)) {
            let mut layer = codec::tables::Layer::new(target.as_str());
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
        let taken = |n: &str| doc.text_styles.iter().any(|s| s.name.eq_ignore_ascii_case(n));
        let target = naming.name('s', name, &taken);
        if !scene.document.text_styles.iter().any(|s| s.name.eq_ignore_ascii_case(&target)) {
            let mut style = codec::tables::TextStyle::new(target.as_str());
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
        let taken = |n: &str| doc.dim_styles.iter().any(|s| s.name.eq_ignore_ascii_case(n));
        let target = naming.name('s', name, &taken);
        if !scene.document.dim_styles.iter().any(|s| s.name.eq_ignore_ascii_case(&target)) {
            let doc = &scene.document;
            let base = values.clone().or_else(|| {
                doc.dim_styles
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(name))
                    .or_else(|| {
                        doc.dim_styles
                            .iter()
                            .find(|s| s.name.eq_ignore_ascii_case(&doc.header.current_dimstyle_name))
                    })
                    .cloned()
            });
            let mut style = base.unwrap_or_else(|| codec::tables::DimStyle::new(target.as_str()));
            if values.is_some() {
                let text = styles.get(&style.dimtxsty).cloned().unwrap_or_else(|| style.dimtxsty.clone());
                if let Some(t) = scene.document.text_styles.iter().find(|t| t.name.eq_ignore_ascii_case(&text)) {
                    style.dimtxsty = t.name.clone();
                    style.dimtxsty_handle = t.handle;
                }
                let arrow = crate::entities::arrowheads::resolve(&mut scene.document, "_Closed");
                style.dimblk1 = arrow.unwrap_or(codec::Handle::NULL);
                style.dimblk2 = style.dimblk1;
                style.dimldrblk = crate::entities::arrowheads::resolve(&mut scene.document, "_None")
                    .unwrap_or(codec::Handle::NULL);
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
    let mut claimed: std::collections::HashSet<String> =
        scene.document.block_records.iter().map(|b| b.name.to_ascii_uppercase()).collect();
    for (name, _) in &converted.blocks {
        let taken = |n: &str| claimed.contains(&n.to_ascii_uppercase());
        let target = naming.name('b', name, &taken);
        claimed.insert(target.to_ascii_uppercase());
        blocks.insert(name.clone(), target);
    }
    let fix = |mut entity: codec::EntityType| {
        let common = entity.common_mut();
        if let Some(layer) = layers.get(&common.layer) {
            common.layer = layer.clone();
        }
        match &mut entity {
            codec::EntityType::Text(t) => {
                if let Some(s) = styles.get(&t.style) {
                    t.style = s.clone();
                }
            }
            codec::EntityType::MText(t) => {
                if let Some(s) = styles.get(&t.style) {
                    t.style = s.clone();
                }
            }
            codec::EntityType::Insert(insert) => {
                if let Some(b) = blocks.get(&insert.block_name) {
                    insert.block_name = b.clone();
                }
            }
            codec::EntityType::Dimension(d) => {
                if let Some(s) = dim_styles.get(&d.base().style_name) {
                    d.base_mut().style_name = s.clone();
                }
            }
            codec::EntityType::Leader(l) => {
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
        if let Ok(handles) = scene.define_block_from_owned_entities(entities, target, glam::DVec3::ZERO) {
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
    document: &codec::Document,
    mut entity: codec::EntityType,
) -> codec::EntityType {
    if let codec::EntityType::Dimension(d) = &mut entity {
        if !d.base().text_user_positioned {
            if let Some(p) = crate::entities::dimension::automatic_text_position(document, d) {
                d.base_mut().text_middle_point = p;
            }
        }
    }
    entity
}
