//! The PDF dialogs' state changes: Attach PDF Underlay, Underlay Layers,
//! PDF Import Settings and Import PDF.

use super::*;
use crate::io::xref_model::Pathtype;
use crate::modules::insert::pdf_import::{self, PdfFileImport};
use crate::ui::window::pdf_dialogs::{
    click_page, page_thumbs, LayerTarget, PdfAttachState, PdfDialogMsg, PdfImportFileState,
    RotationChoice, UnderlayLayersState,
};

/// "7.8740 × 3.9369" (inches) for a page.
fn page_size_text(path: &str, page: &str) -> String {
    crate::scene::model::pdf_raster::page_size_inches(path, page)
        .map(|(w, h)| format!("{w:.4} × {h:.4}"))
        .unwrap_or_default()
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

    /// PDFATTACH after the file is chosen.
    pub(in crate::app) fn open_pdf_attach_dialog(&mut self, path: &str) {
        let i = self.active_tab;
        let mut existing: Vec<(String, String)> = Vec::new();
        for object in self.tabs[i].scene.document.objects.values() {
            if let codec::objects::ObjectType::UnderlayDefinition(def) = object {
                let name = file_stem(&def.file_path);
                if !existing.iter().any(|(n, _)| *n == name) {
                    existing.push((name, def.file_path.clone()));
                }
            }
        }
        let mut state = PdfAttachState::new(existing);
        self.load_pdf_attach_file(&mut state, path);
        self.pdf_attach = Some(state);
        self.active_modal = Some(crate::app::ModalKind::PdfAttach);
    }

    fn load_pdf_attach_file(&self, state: &mut PdfAttachState, path: &str) {
        state.path = path.to_string();
        state.name = file_stem(path);
        state.pages = page_thumbs(path);
        state.selected = vec![0];
        state.anchor = 0;
        state.found_in = crate::entities::underlay::display_path(path);
        state.page_size = page_size_text(path, "1");
        state.saved_path =
            crate::entities::underlay::display_path(&self.pdf_stored_path(path, state.path_type.0));
    }

    /// ULAYERS: every PDF underlay of the drawing, the selected one first
    /// shown.
    pub(in crate::app) fn open_underlay_layers_dialog(&mut self, i: usize) {
        let document = &self.tabs[i].scene.document;
        let selected: Vec<codec::Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .map(|(h, _)| *h)
            .collect();
        let mut targets: Vec<LayerTarget> = Vec::new();
        for entity in document.entities() {
            let codec::EntityType::Underlay(u) = entity else {
                continue;
            };
            if u.underlay_type != codec::entities::UnderlayType::Pdf {
                continue;
            }
            let Some(def) = crate::entities::underlay::definition(u, document) else {
                continue;
            };
            let mut name = crate::entities::underlay::definition_display_name(def);
            let same = targets.iter().filter(|t| t.name.starts_with(&name)).count();
            if same > 0 {
                name = format!("{name} ({})", same + 1);
            }
            let mut layers: Vec<String> = crate::scene::model::pdf_layers::layers(&def.file_path)
                .into_iter()
                .map(|l| l.name)
                .collect();
            layers.sort_by_key(|n| n.to_lowercase());
            layers.dedup();
            targets.push(LayerTarget {
                name,
                handle: u.common.handle,
                layers,
                hidden: crate::scene::model::pdf_layers::hidden_layers(u),
            });
        }
        if targets.is_empty() {
            self.command_line.push_error("No PDF underlays found.");
            return;
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

    /// PDFIMPORT's File option after the file is chosen.
    pub(in crate::app) fn open_pdf_import_file(&mut self, path: &str) {
        self.pdf_import_file = Some(PdfImportFileState {
            path: path.to_string(),
            pages: page_thumbs(path),
            selected: 0,
            page_text: "1".into(),
            insert_on_screen: false,
            scale: "1".into(),
            rotation: RotationChoice(0),
            page_size: page_size_mm(path, "1"),
            settings: pdf_import::import_settings(),
        });
        self.active_modal = Some(crate::app::ModalKind::PdfImportFile);
    }

    /// Writes the layers each underlay turns off, as one undo step.
    fn apply_underlay_layers(&mut self, i: usize, targets: Vec<LayerTarget>) {
        use crate::scene::model::pdf_layers::{hidden_layers, LAYER_OVERRIDE_APP};
        let changed: Vec<LayerTarget> = targets
            .into_iter()
            .filter(|t| match self.tabs[i].scene.document.get_entity(t.handle) {
                Some(codec::EntityType::Underlay(u)) => hidden_layers(u) != t.hidden,
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
            if let Some(codec::EntityType::Underlay(u)) =
                self.tabs[i].scene.document.get_entity_mut(target.handle)
            {
                let data = &mut u.common.extended_data;
                if let Some(app) = app_handle {
                    data.raw_dwg_eed.retain(|(a, _)| *a != app);
                }
                data.remove_record(LAYER_OVERRIDE_APP);
                if !target.hidden.is_empty() {
                    let mut record = codec::xdata::ExtendedDataRecord::new(LAYER_OVERRIDE_APP);
                    for name in &target.hidden {
                        record.add_value(codec::xdata::XDataValue::String(name.clone()));
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
                let text = match which {
                    "attach" => crate::t!("Attaches a PDF file as an underlay."),
                    "layers" => crate::t!("Turns the layers of a PDF underlay on or off."),
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
                if let Some(state) = &self.pdf_attach {
                    state.remember();
                }
                return Task::done(Message::PdfAttachPick);
            }
            PdfDialogMsg::AttachName(name) => {
                let Some(mut state) = self.pdf_attach.take() else {
                    return Task::none();
                };
                if let Some((_, path)) = state.existing.iter().find(|(n, _)| *n == name).cloned() {
                    self.load_pdf_attach_file(&mut state, &path);
                }
                self.pdf_attach = Some(state);
            }
            PdfDialogMsg::AttachPage(page) => {
                let (ctrl, shift) = (self.ctrl_down, self.shift_down);
                if let Some(state) = self.pdf_attach.as_mut() {
                    click_page(&mut state.selected, &mut state.anchor, page, ctrl, shift);
                    let first = state.page_labels().into_iter().next().unwrap_or_default();
                    state.page_size = page_size_text(&state.path, &first);
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
                            None => t.hidden.push(layer),
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

            // ── Import PDF ────────────────────────────────────────────────
            PdfDialogMsg::ImportBrowse => return Task::done(Message::PdfImportPick),
            PdfDialogMsg::ImportPage(page) => {
                if let Some(s) = self.pdf_import_file.as_mut() {
                    s.selected = page;
                    s.page_text = (page + 1).to_string();
                    s.page_size = page_size_mm(&s.path, &s.page_text);
                }
            }
            PdfDialogMsg::ImportPageText(text) => {
                if let Some(s) = self.pdf_import_file.as_mut() {
                    if let Some(n) = text.trim().parse::<usize>().ok().filter(|n| (1..=s.pages.len()).contains(n)) {
                        s.selected = n - 1;
                        s.page_size = page_size_mm(&s.path, &n.to_string());
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
                let Some(scale) = state.scale.trim().parse::<f64>().ok().filter(|v| *v > 0.0) else {
                    self.command_line.push_error("Value must be positive and nonzero.");
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
                self.run_pdf_import(i, crate::app::commands::pdf_import::PdfImportSource::File(import));
            }
        }
        Task::none()
    }

    fn pdf_attach_ok(&mut self, i: usize) -> Task<Message> {
        use crate::command::CadCommand;
        let Some(state) = self.pdf_attach.as_ref() else {
            return Task::none();
        };
        let number = |s: &str| s.trim().parse::<f64>().ok();
        let insertion = if state.insert_on_screen {
            None
        } else {
            match (number(&state.insert[0]), number(&state.insert[1]), number(&state.insert[2])) {
                (Some(x), Some(y), Some(z)) => Some(glam::DVec3::new(x, y, z)),
                _ => {
                    self.command_line.push_error("Requires numeric value.");
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
                    self.command_line.push_error("Value must be positive and nonzero.");
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
                    self.command_line.push_error("Requires numeric value.");
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
        let mut command = crate::modules::insert::pdf_attach::PdfAttachCommand::from_dialog(
            &state.path,
            &stored,
            &state.page_labels(),
            scale,
            rotation,
            insunits,
        );
        match insertion {
            Some(point) => {
                let result = command.on_point(point);
                self.tabs[i].active_cmd = Some(Box::new(command));
                if matches!(result, crate::command::CmdResult::NeedPoint) {
                    self.command_line.push_info(
                        &self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()).unwrap_or_default(),
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

/// "200 × 100 mm" for a page.
fn page_size_mm(path: &str, page: &str) -> String {
    crate::scene::model::pdf_raster::page_size_inches(path, page)
        .map(|(w, h)| format!("{:.0} × {:.0} mm", w * 25.4, h * 25.4))
        .unwrap_or_default()
}
