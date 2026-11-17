//! ACISIN and WMFIN, and the files IMPORT routes here: ACIS text (SAT),
//! STEP and IGES models and Windows metafiles, read into the current drawing
//! as one undo step.

use super::*;
use crate::io::tds_import::TdsStep;
use std::collections::HashMap;

use crate::io::model_import::{self, ModelFormat};
use crate::io::picture_import::{self, PictureFormat, PictureRead};

impl OpenCADStudio {
    /// ACISIN and WMFIN; `None` defers to the other families.
    pub(super) fn dispatch_model_import(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = match cmd.trim().split_once(char::is_whitespace) {
            Some((verb, rest)) => (verb.to_ascii_uppercase(), rest.trim().trim_matches('"')),
            None => (cmd.trim().to_ascii_uppercase(), ""),
        };
        match (verb.as_str(), rest.is_empty()) {
            ("ACISIN", true) if crate::sys::file_dia() => {
                let filters = vec![("ACIS (*.sat)".to_string(), vec!["sat".to_string()])];
                let title = crate::t!("Select ACIS File").into_owned();
                Some(Task::perform(
                    super::dgn_import::pick(title, filters),
                    |path| match path {
                        Some(path) => Message::Command(format!("_ACISINFILE {path}")),
                        None => Message::Noop,
                    },
                ))
            }
            ("ACISIN", true) => {
                use crate::command::FreeTextValuePromptCommand;
                let command =
                    FreeTextValuePromptCommand::new("_ACISINFILE", "Enter SAT file name:");
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            ("ACISIN" | "_ACISINFILE", false) => {
                if crate::scene::model::pdf_raster::source_bytes(rest).is_none() {
                    let name = std::path::Path::new(rest)
                        .file_name()
                        .map_or_else(|| rest.to_string(), |n| n.to_string_lossy().into_owned());
                    let here = std::env::current_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default();
                    self.command_line.push_error(&format!(
                        "\"{name}\": file not found in the current directory, {here}"
                    ));
                    return Some(Task::none());
                }
                Some(self.import_model(i, rest, ModelFormat::Sat))
            }
            ("_ACISINFILE", true) => Some(Task::none()),
            ("WMFIN", true) if crate::sys::file_dia() => {
                let filters = vec![("Metafile (*.wmf)".to_string(), vec!["wmf".to_string()])];
                let title = crate::t!("Import WMF").into_owned();
                Some(Task::perform(
                    super::dgn_import::pick(title, filters),
                    |path| match path {
                        Some(path) => Message::Command(format!("_WMFINFILE {path}")),
                        None => Message::Noop,
                    },
                ))
            }
            ("WMFIN", true) => {
                use crate::command::FreeTextValuePromptCommand;
                let command = FreeTextValuePromptCommand::new(
                    "_WMFINFILE",
                    "Enter name of WMF file to import (no extension):",
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            ("WMFIN" | "_WMFINFILE", false) => {
                let path = if std::path::Path::new(rest).extension().is_some() {
                    rest.to_string()
                } else {
                    format!("{rest}.wmf")
                };
                Some(self.import_wmf(i, &path, true))
            }
            ("_WMFINFILE", true) => Some(Task::none()),
            ("SVGIN", true) if crate::sys::file_dia() => {
                let filters = vec![(
                    "SVG (*.svg; *.svgz)".to_string(),
                    vec!["svg".to_string(), "svgz".to_string()],
                )];
                let title = crate::t!("Import SVG").into_owned();
                Some(Task::perform(
                    super::dgn_import::pick(title, filters),
                    |path| match path {
                        Some(path) => Message::Command(format!("_SVGINFILE {path}")),
                        None => Message::Noop,
                    },
                ))
            }
            ("SVGIN", true) => {
                use crate::command::FreeTextValuePromptCommand;
                let command = FreeTextValuePromptCommand::new(
                    "_SVGINFILE",
                    "Enter name of SVG file to import (no extension):",
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            ("SVGIN" | "_SVGINFILE", false) => {
                let path = if std::path::Path::new(rest).extension().is_some() {
                    rest.to_string()
                } else {
                    format!("{rest}.svg")
                };
                Some(self.import_svg(i, &path, true))
            }
            ("_SVGINFILE", true) => Some(Task::none()),
            _ => None,
        }
    }

    /// Reads a model file into drawing `i`: the file is read and parsed off
    /// the UI thread (inline where the platform has no threads), and the
    /// objects are placed when the result arrives
    /// ([`Self::finish_model_import`]).
    pub(in crate::app) fn import_model(
        &mut self,
        i: usize,
        path: &str,
        format: ModelFormat,
    ) -> Task<Message> {
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        let drawing_mm = crate::app::properties::insunits_to_mm(
            self.tabs[i].scene.document.header.insertion_units,
        );
        let path = path.to_string();
        let worker_path = path.clone();
        self.command_line.push_info(&format!("Reading {path}…"));
        crate::app::update::background_task(
            "IMPORT",
            move || model_import::load(format, &worker_path, drawing_mm),
            move |result| Message::ModelImportFinished(tab_id, generation, path, format, result),
        )
    }

    /// The tab a finished import goes to: the one it was started for, while
    /// that tab still holds the same document. If not, the command line says
    /// why the result is dropped.
    fn import_target(&mut self, tab_id: u64, generation: u64) -> Option<usize> {
        let found = self
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id && tab.document_generation == generation);
        if found.is_none() {
            let message = if self.tabs.iter().any(|tab| tab.id == tab_id) {
                "Import: the drawing it was headed for was replaced or cleared."
            } else {
                "Import: the drawing it was headed for was closed."
            };
            self.command_line.push_info(message);
        }
        found
    }

    /// Places a finished import on the UI thread: its blocks under names the
    /// drawing does not use yet, then its objects, as one undo step.
    pub(in crate::app) fn finish_model_import(
        &mut self,
        tab_id: u64,
        generation: u64,
        path: &str,
        format: ModelFormat,
        result: Result<model_import::Imported, model_import::ImportFailure>,
    ) -> Task<Message> {
        let Some(i) = self.import_target(tab_id, generation) else {
            return Task::none();
        };
        let imported = match result {
            Ok(imported) => imported,
            Err(model_import::ImportFailure::Missing) => {
                self.command_line
                    .push_error(&format!("File {path} does not exist."));
                return Task::none();
            }
            Err(model_import::ImportFailure::Unreadable) => {
                self.command_line.push_error(&format!(
                    "Import failed: {path} has no model the importer reads."
                ));
                return Task::none();
            }
        };
        let translated = format != ModelFormat::Sat;
        self.push_undo_snapshot(i, "IMPORT");
        let scene = &mut self.tabs[i].scene;
        // Each block goes under a name the drawing does not use yet (a
        // product named like a block already there gets a new name, not the
        // old definition), and the inserts are pointed at the names the
        // blocks got. An insert of a block that could not be made is left
        // out.
        fn retarget(
            entities: Vec<document::EntityType>,
            defined: &HashMap<String, String>,
        ) -> Vec<document::EntityType> {
            entities
                .into_iter()
                .filter_map(|entity| match entity {
                    document::EntityType::Insert(mut insert) => {
                        insert.block_name = defined.get(&insert.block_name)?.clone();
                        Some(document::EntityType::Insert(insert))
                    }
                    other => Some(other),
                })
                .collect()
        }
        let mut taken: Vec<String> = scene
            .document
            .block_records
            .iter()
            .map(|b| b.name.clone())
            .collect();
        let mut defined: HashMap<String, String> = HashMap::new();
        for (name, entities) in imported.blocks {
            let unique = crate::modules::insert::xattach::unique_block_name(&name, &taken);
            let entities = retarget(entities, &defined);
            match scene.define_block_from_owned_entities(entities, &unique, glam::DVec3::ZERO) {
                Ok(_) => {
                    taken.push(unique.clone());
                    defined.insert(name, unique);
                }
                Err(error) => self.command_line.push_error(&format!("{name}: {error}")),
            }
        }
        for entity in retarget(imported.entities, &defined) {
            scene.add_entity(entity);
        }
        self.tabs[i].dirty = true;
        if translated {
            self.command_line.push_output("Import Job Complete");
        }
        self.refresh_properties();
        Task::none()
    }

    /// A metafile as a new block `WMF<n>`, fitted to half the current view:
    /// placed at the view's top-left corner (IMPORT), or by the insertion
    /// prompts (WMFIN). The file is read off the UI thread.
    pub(in crate::app) fn import_wmf(
        &mut self,
        i: usize,
        path: &str,
        prompt: bool,
    ) -> Task<Message> {
        self.read_picture(
            i,
            PictureFormat::Wmf,
            PictureSource::File(path.to_string()),
            prompt,
        )
    }

    /// An SVG drawing as a new block `SVG<n>`, fitted to half the current
    /// view: placed at the view's top-left corner (IMPORT), or by the
    /// insertion prompts (SVGIN). The file is read off the UI thread.
    pub(in crate::app) fn import_svg(
        &mut self,
        i: usize,
        path: &str,
        prompt: bool,
    ) -> Task<Message> {
        self.read_picture(
            i,
            PictureFormat::Svg,
            PictureSource::File(path.to_string()),
            prompt,
        )
    }

    /// SVG markup pasted from the system clipboard, as a block placed by
    /// the insertion prompts. The markup is read off the UI thread.
    pub(in crate::app) fn paste_svg_text(&mut self, i: usize, text: String) -> Task<Message> {
        self.read_picture(i, PictureFormat::Svg, PictureSource::Clipboard(text), true)
    }

    /// Reads a picture for drawing `i` on a worker (inline where the
    /// platform has no threads). A panic in the reader becomes an error on
    /// the command line. The result is placed when it arrives
    /// ([`Self::finish_picture_import`]).
    fn read_picture(
        &mut self,
        i: usize,
        format: PictureFormat,
        source: PictureSource,
        prompt: bool,
    ) -> Task<Message> {
        self.read_picture_with(i, format, source, prompt, read_picture_source)
    }

    /// [`Self::read_picture`] with the reader given, so a test can give one
    /// that panics.
    fn read_picture_with(
        &mut self,
        i: usize,
        format: PictureFormat,
        source: PictureSource,
        prompt: bool,
        reader: fn(PictureFormat, PictureSource, [f64; 2]) -> PictureRead,
    ) -> Task<Message> {
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        let (_, size, _) = self.tabs[i].scene.view_window();
        let fit = [0.5 * size[0], 0.5 * size[1]];
        let path = match &source {
            PictureSource::File(path) => {
                self.command_line.push_info(&format!("Reading {path}…"));
                Some(path.clone())
            }
            PictureSource::Clipboard(_) => None,
        };
        crate::app::update::background_task(
            picture_command(path.is_none(), format, prompt),
            move || reader(format, source, fit),
            move |read| {
                Message::PictureImportFinished(tab_id, generation, path, format, prompt, read)
            },
        )
    }

    /// Places a finished picture read on the UI thread, as one undo step.
    /// When the insertion prompts must place it but a different command
    /// started during the read, the picture is not placed: that command
    /// stays.
    pub(in crate::app) fn finish_picture_import(
        &mut self,
        tab_id: u64,
        generation: u64,
        path: Option<&str>,
        format: PictureFormat,
        prompt: bool,
        read: PictureRead,
    ) -> Task<Message> {
        let Some(i) = self.import_target(tab_id, generation) else {
            return Task::none();
        };
        let command = picture_command(path.is_none(), format, prompt);
        let (metafile, entities) = match (read, path) {
            (PictureRead::Wmf(picture), _) => (Some(picture), Vec::new()),
            (PictureRead::Svg(entities), _) => (None, entities),
            (failure, path) => {
                let message = match (failure, path) {
                    (_, None) => crate::tr!("clipboard", "no-supported-content"),
                    (PictureRead::Missing, Some(path)) => format!("File {path} does not exist."),
                    (PictureRead::Empty, Some(path)) => {
                        format!("{path} has no shapes to import.")
                    }
                    (_, Some(path)) => match format {
                        PictureFormat::Wmf => format!("{path} is not a valid Windows metafile."),
                        PictureFormat::Svg => format!("{path} is not a valid SVG file."),
                    },
                };
                self.command_line.push_error(&message);
                return Task::none();
            }
        };
        if prompt && self.tabs[i].active_cmd.is_some() {
            self.command_line.push_error(&format!(
                "{command} cancelled: a different command started while the picture was read."
            ));
            return Task::none();
        }
        self.push_undo_snapshot(i, command);
        let entities = match metafile {
            Some(picture) => self.add_metafile_text_styles(i, picture),
            None => entities,
        };
        let prefix = match format {
            PictureFormat::Wmf => "WMF",
            PictureFormat::Svg => "SVG",
        };
        self.place_picture_block(i, prefix, entities, prompt)
    }

    /// Gives each face a metafile's text uses a text style in drawing `i`,
    /// and returns the metafile's objects with their texts set to those
    /// styles.
    fn add_metafile_text_styles(
        &mut self,
        i: usize,
        mut picture: crate::io::wmf_import::Picture,
    ) -> Vec<document::EntityType> {
        let scene = &mut self.tabs[i].scene;
        // A text style per face, `WMF-<face><n>` with `n` counting the
        // face's bold and italic variants; the style's font is the
        // installed file of that face, none when it is not installed. A
        // style the drawing already has is used as it is.
        let mut styles: Vec<String> = Vec::new();
        for (k, face) in picture.fonts.iter().enumerate() {
            let n = picture.fonts[..k]
                .iter()
                .filter(|f| f.name == face.name)
                .count();
            // The face name comes from the file.
            let name = crate::scene::sanitized_table_name(&format!("WMF-{}{n}", face.name), "WMF");
            if !scene.document.text_styles.contains(&name) {
                let mut style = document::tables::TextStyle::new(name.as_str());
                style.handle = scene.document.allocate_handle();
                style.font_file =
                    crate::scene::text::sysfont::face_file(&face.name, face.bold, face.italic)
                        .unwrap_or_default();
                let _ = scene.document.text_styles.add(style);
            }
            styles.push(name);
        }
        for (at, font) in &picture.text_fonts {
            if let Some(document::EntityType::Text(text)) = picture.entities.get_mut(*at) {
                text.style = styles[*font].clone();
            }
        }
        picture.entities
    }

    /// Defines `entities` as a new block `<prefix><n>` and puts it in the
    /// drawing: at the view's top-left corner, or by the insertion prompts.
    fn place_picture_block(
        &mut self,
        i: usize,
        prefix: &str,
        entities: Vec<document::EntityType>,
        prompt: bool,
    ) -> Task<Message> {
        let (centre, size, pixel) = self.tabs[i].scene.view_window();
        let scene = &mut self.tabs[i].scene;
        let mut n = 0;
        let name = loop {
            let name = format!("{prefix}{n}");
            if scene.document.block_records.get(&name).is_none() {
                break name;
            }
            n += 1;
        };
        if let Err(error) =
            scene.define_block_from_owned_entities(entities, &name, glam::DVec3::ZERO)
        {
            self.command_line.push_error(&error);
            return Task::none();
        }
        self.tabs[i].dirty = true;
        if prompt {
            use crate::command::CadCommand;
            use crate::modules::insert::insert_block::InsertBlockCommand;
            let preview = self.tabs[i].scene.block_preview_wires(&name);
            let command = InsertBlockCommand::classic_for_block(name, preview, String::new());
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
        } else {
            // The view's top-left corner, a pixel or two in.
            let corner = document::types::Vector3::new(
                centre[0] - 0.5 * size[0] + pixel,
                centre[1] + 0.5 * size[1] - 2.0 * pixel,
                0.0,
            );
            let insert = document::entities::Insert::new(&name, corner);
            self.tabs[i]
                .scene
                .add_entity(document::EntityType::Insert(insert));
            self.refresh_properties();
        }
        Task::none()
    }
}

/// Where the bytes of a picture import come from.
enum PictureSource {
    /// A file, by path.
    File(String),
    /// SVG markup pasted from the system clipboard.
    Clipboard(String),
}

/// The reader of a picture import: the file, or the pasted markup, read
/// as `format` and fitted to `fit`.
fn read_picture_source(format: PictureFormat, source: PictureSource, fit: [f64; 2]) -> PictureRead {
    match source {
        PictureSource::File(path) => picture_import::load(format, &path, fit),
        PictureSource::Clipboard(text) => picture_import::read(format, text.as_bytes(), fit),
    }
}

/// The reader of the 3D Studio Import window: the file at `path` (a source
/// registered this session, else the disk) as a 3D Studio model; none when
/// it is missing or no 3D Studio file.
fn read_tds_file(path: &str) -> Option<crate::io::tds_import::TdsScene> {
    crate::scene::model::pdf_raster::source_bytes(path)
        .and_then(|bytes| crate::io::tds_import::read(&bytes))
}

/// The command a picture import runs for: it names the undo step and a
/// failed read.
fn picture_command(pasted: bool, format: PictureFormat, prompt: bool) -> &'static str {
    match (pasted, format, prompt) {
        (true, _, _) => "PASTECLIP",
        (false, PictureFormat::Wmf, true) => "WMFIN",
        (false, PictureFormat::Svg, true) => "SVGIN",
        (false, _, false) => "IMPORT",
    }
}

impl OpenCADStudio {
    /// Reads a 3D Studio file off the UI thread for the 3D Studio Import
    /// window, which opens when the read ends
    /// ([`Self::open_tds_import_dialog`]). A panic in the reader becomes an
    /// error on the command line.
    pub(in crate::app) fn read_tds_for_dialog(&mut self, path: &str) -> Task<Message> {
        self.read_tds_for_dialog_with(path, read_tds_file)
    }

    /// [`Self::read_tds_for_dialog`] with the reader given, so a test can
    /// give one that panics.
    fn read_tds_for_dialog_with(
        &mut self,
        path: &str,
        reader: fn(&str) -> Option<crate::io::tds_import::TdsScene>,
    ) -> Task<Message> {
        use crate::ui::window::import_dialogs::ImportDialogMsg;
        self.command_line.push_info(&format!("Reading {path}…"));
        let path = path.to_string();
        let worker_path = path.clone();
        crate::app::update::background_task(
            "IMPORT",
            move || reader(&worker_path).map(std::sync::Arc::new),
            move |scene| Message::ImportDialog(ImportDialogMsg::TdsRead(path, scene)),
        )
    }

    /// The 3D Studio Import window for a file read off the UI thread; an
    /// error when it is no 3D Studio file. It takes the place of the Import
    /// File window (which a script or the automation `run` operation leaves
    /// open: they send IMPORT and the file name apart) and of its own older
    /// copy (Change…). If a different window opened during the read, the 3D
    /// Studio Import window does not open.
    pub(in crate::app) fn open_tds_import_dialog(
        &mut self,
        path: &str,
        scene: Option<std::sync::Arc<crate::io::tds_import::TdsScene>>,
    ) {
        let Some(scene) = scene else {
            self.command_line
                .push_error(&format!("{path} is not a valid 3D Studio file."));
            return;
        };
        if self.active_modal.is_some_and(|modal| {
            !matches!(
                modal,
                crate::app::ModalKind::TdsImport | crate::app::ModalKind::ImportFile
            )
        }) {
            self.command_line.push_error(
                "3D Studio Import cancelled: a different window opened while the file was read.",
            );
            return;
        }
        let mut objects: Vec<(String, &'static str, usize)> = scene
            .objects
            .iter()
            .map(|(name, object)| {
                let materials = match object {
                    crate::io::tds_import::TdsObject::Mesh(mesh) => {
                        scene.mesh_materials(mesh).len()
                    }
                    _ => 0,
                };
                (name.clone(), object.kind(), materials)
            })
            .collect();
        objects.sort_by_key(|o| o.0.to_lowercase());
        objects.insert(0, ("Global Settings".to_string(), "Global", 0));
        // The layer and material choices made last stay; nothing starts
        // checked.
        let previous = self.import_dialogs.tds.take();
        self.import_dialogs.tds = Some(crate::ui::window::import_dialogs::TdsImportState {
            path: path.to_string(),
            checked: vec![false; objects.len()],
            objects,
            materials: scene.materials.len(),
            filter: 0,
            search: String::new(),
            layers: previous.as_ref().map_or(0, |s| s.layers),
            multi: previous.as_ref().map_or(0, |s| s.multi),
            scene,
        });
        self.active_modal = Some(crate::app::ModalKind::TdsImport);
    }

    /// The window's choices as an import: asked mesh by mesh first when set
    /// to ask and some checked mesh has several materials.
    pub(in crate::app) fn start_tds_import(
        &mut self,
        i: usize,
        state: crate::ui::window::import_dialogs::TdsImportState,
    ) -> Task<Message> {
        use crate::io::tds_import::{LayerMode, MultiMaterial, TdsObject};
        use crate::modules::insert::tds_import::TdsImportRequest;
        let scene = state.scene.clone();
        let selected: Vec<String> = state
            .objects
            .iter()
            .zip(&state.checked)
            .filter(|(_, on)| **on)
            .map(|((name, _, _), _)| name.clone())
            .collect();
        // The import runs through the objects in the window's order; the
        // window lists a mesh's materials last assigned first.
        let several: Vec<(String, Vec<(String, Option<[u8; 3]>)>)> = selected
            .iter()
            .filter_map(|name| scene.objects.iter().find(|(n, _)| n == name))
            .filter_map(|(name, object)| match object {
                TdsObject::Mesh(mesh) => {
                    let mut materials: Vec<(String, Option<[u8; 3]>)> = Vec::new();
                    for (material, faces) in mesh.groups.iter().rev() {
                        if !faces.is_empty() && !materials.iter().any(|(m, _)| m == material) {
                            let diffuse = scene
                                .materials
                                .iter()
                                .find(|m| m.name == *material)
                                .and_then(|m| m.diffuse);
                            materials.push((material.clone(), diffuse));
                        }
                    }
                    (materials.len() > 1).then(|| (name.clone(), materials))
                }
                _ => None,
            })
            .collect();
        let choice = match state.multi {
            1 => Some(MultiMaterial::Split),
            2 => Some(MultiMaterial::First),
            3 => Some(MultiMaterial::Material(None)),
            _ => None,
        };
        let mut request = TdsImportRequest {
            path: state.path.clone(),
            selected,
            layers: match state.layers {
                1 => LayerMode::ByMaterial,
                2 => LayerMode::ByObjectColor,
                3 => LayerMode::Single,
                _ => LayerMode::ByObject,
            },
            multi: Default::default(),
            compact: matches!(state.multi, 0 | 1),
            asked: false,
            stop_at: None,
        };
        match choice {
            Some(choice) => {
                for (name, _) in several {
                    request.multi.insert(name, choice.clone());
                }
            }
            None if !several.is_empty() => {
                request.asked = true;
                self.import_dialogs.tds_material =
                    Some(crate::ui::window::import_dialogs::TdsMaterialState {
                        tab: i,
                        request,
                        queue: several,
                        choice: 0,
                        material: 0,
                        list_open: false,
                        remember: false,
                        scene,
                    });
                self.active_modal = Some(crate::app::ModalKind::TdsMaterial);
                return Task::none();
            }
            None => {}
        }
        self.run_tds_import(i, request, scene)
    }

    /// The answer for the asked mesh (and, when the choice is kept, for the
    /// rest); the import runs once no mesh is left to ask about.
    pub(in crate::app) fn answer_tds_material(&mut self) -> Task<Message> {
        use crate::io::tds_import::MultiMaterial;
        let Some(mut state) = self.import_dialogs.tds_material.take() else {
            return Task::none();
        };
        let choice = |state: &crate::ui::window::import_dialogs::TdsMaterialState,
                      materials: &[(String, Option<[u8; 3]>)],
                      chosen: Option<&str>| match state.choice {
            0 => MultiMaterial::Split,
            1 => MultiMaterial::First,
            _ => MultiMaterial::Material(
                chosen
                    .and_then(|c| materials.iter().find(|(m, _)| m == c))
                    .map(|(m, _)| m.clone()),
            ),
        };
        let (name, materials) = state.queue.remove(0);
        let chosen = state
            .material
            .checked_sub(1)
            .and_then(|k| materials.get(k))
            .map(|(m, _)| m.clone());
        let answer = choice(&state, &materials, chosen.as_deref());
        state.request.multi.insert(name, answer);
        if state.remember {
            for (name, materials) in std::mem::take(&mut state.queue) {
                let answer = choice(&state, &materials, chosen.as_deref());
                state.request.multi.insert(name, answer);
            }
        }
        if state.queue.is_empty() {
            self.close_active_modal();
            return self.run_tds_import(state.tab, state.request, state.scene);
        }
        // The next mesh is offered the same choice, without a material.
        state.material = 0;
        state.list_open = false;
        self.import_dialogs.tds_material = Some(state);
        Task::none()
    }

    /// Cancel: the import stops at the asked mesh; the objects before it
    /// come in.
    pub(in crate::app) fn stop_tds_import(
        &mut self,
        mut state: crate::ui::window::import_dialogs::TdsMaterialState,
    ) -> Task<Message> {
        state.request.stop_at = state.queue.first().map(|(name, _)| name.clone());
        self.run_tds_import(state.tab, state.request, state.scene)
    }

    /// The import itself: the file's model converted off the UI thread
    /// with the window's choices; [`Self::finish_tds_import`] places the
    /// result when it arrives. A panic in the conversion becomes an error
    /// on the command line.
    pub(in crate::app) fn run_tds_import(
        &mut self,
        i: usize,
        request: crate::modules::insert::tds_import::TdsImportRequest,
        scene: std::sync::Arc<crate::io::tds_import::TdsScene>,
    ) -> Task<Message> {
        self.run_tds_import_with(i, request, scene, crate::io::tds_import::convert)
    }

    /// [`Self::run_tds_import`] with the conversion given, so a test can
    /// give one that panics.
    fn run_tds_import_with(
        &mut self,
        i: usize,
        request: crate::modules::insert::tds_import::TdsImportRequest,
        scene: std::sync::Arc<crate::io::tds_import::TdsScene>,
        converter: fn(
            &crate::io::tds_import::TdsScene,
            &crate::io::tds_import::TdsOptions,
        ) -> crate::io::tds_import::TdsImport,
    ) -> Task<Message> {
        use crate::ui::window::import_dialogs::ImportDialogMsg;
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        let options = crate::io::tds_import::TdsOptions {
            selected: request.selected.clone(),
            layers: request.layers,
            multi: request.multi.clone(),
            compact: request.compact,
            single_layer: self.tabs[i]
                .scene
                .document
                .header
                .current_layer_name
                .clone(),
            stop_at: request.stop_at.clone(),
        };
        let worker_scene = scene.clone();
        crate::app::update::background_task(
            "IMPORT",
            move || converter(&worker_scene, &options),
            move |import| {
                Message::ImportDialog(ImportDialogMsg::TdsConverted(
                    tab_id,
                    generation,
                    Box::new(request),
                    scene,
                    Box::new(import),
                ))
            },
        )
    }

    /// Places a finished 3D Studio import on the UI thread, one undo step:
    /// the meshes on their layers with their materials, the cameras as
    /// named views. A result for a drawing that was closed, replaced or
    /// cleared is dropped.
    pub(in crate::app) fn finish_tds_import(
        &mut self,
        tab_id: u64,
        generation: u64,
        request: crate::modules::insert::tds_import::TdsImportRequest,
        scene: std::sync::Arc<crate::io::tds_import::TdsScene>,
        import: crate::io::tds_import::TdsImport,
    ) {
        use document::objects::ObjectType;
        let Some(i) = self.import_target(tab_id, generation) else {
            return;
        };
        // Named views take the current view's proportions.
        let (_, size, _) = self.tabs[i].scene.view_window();
        let aspect = if size[0] > 0.0 && size[1] > 0.0 {
            size[0] / size[1]
        } else {
            1.0
        };
        self.push_undo_snapshot(i, "IMPORT");
        let document = &mut self.tabs[i].scene.document;
        // Materials live in the material dictionary; a drawing without one
        // imports the meshes without materials.
        let dictionary = document.header.acad_material_dict_handle;
        let has_dictionary = matches!(
            document.objects.get(&dictionary),
            Some(ObjectType::Dictionary(_))
        );
        // Materials by name: the drawing's own when it has one, else one
        // made from the file's colours.
        let mut materials: std::collections::HashMap<String, document::Handle> = Default::default();
        for name in &import.materials {
            let existing = document.objects.iter().find_map(|(h, o)| match o {
                ObjectType::Material(m) if m.name.eq_ignore_ascii_case(name) => Some(*h),
                _ => None,
            });
            let handle = match existing {
                Some(h) => h,
                None if !has_dictionary => continue,
                None => {
                    let source = scene.materials.iter().find(|m| m.name == *name);
                    let mut material = document::objects::Material::new();
                    material.handle = document.allocate_handle();
                    material.owner = dictionary;
                    material.name = name.clone();
                    let color = |rgb: Option<[u8; 3]>| match rgb {
                        Some([r, g, b]) => document::objects::MaterialColor {
                            flag: 1,
                            factor: 1.0,
                            rgb: Some(crate::scene::convert::acis_import::packed_true_colour(
                                r, g, b,
                            )),
                        },
                        None => document::objects::MaterialColor::default(),
                    };
                    material.ambient_color = color(source.and_then(|m| m.ambient));
                    material.diffuse_color = color(source.and_then(|m| m.diffuse));
                    material.specular_color = color(source.and_then(|m| m.specular));
                    // Shininess is the gloss, transparency takes from the
                    // opacity, and a texture map is the diffuse map's file.
                    if let Some(shininess) = source.and_then(|m| m.shininess) {
                        // As the reference keeps it: one less the roughness,
                        // in single precision.
                        material.specular_gloss_factor = f64::from(1.0 - (1.0 - shininess));
                    }
                    if let Some(transparency) = source.and_then(|m| m.transparency) {
                        material.opacity_percent = f64::from(1.0 - transparency);
                    }
                    if let Some((file, _)) = source.and_then(|m| m.map.as_ref()) {
                        material.diffuse_map.file_name = file.clone();
                        material.diffuse_map.source = 1;
                        material.diffuse_map.projection = 1;
                    }
                    let handle = material.handle;
                    if let Some(ObjectType::Dictionary(dictionary)) =
                        document.objects.get_mut(&dictionary)
                    {
                        dictionary.add_entry(name.clone(), handle);
                    }
                    document
                        .objects
                        .insert(handle, ObjectType::Material(material));
                    handle
                }
            };
            materials.insert(name.clone(), handle);
        }
        for part in &import.parts {
            if !document.layers.contains(&part.layer) {
                let mut layer = document::tables::Layer::new(part.layer.as_str());
                layer.handle = document.allocate_handle();
                let _ = document.layers.add(layer);
            }
        }
        for view in &import.views {
            let mut named = document::tables::View::new(view.name.as_str());
            named.target =
                document::types::Vector3::new(view.target[0], view.target[1], view.target[2]);
            named.direction = document::types::Vector3::new(
                view.direction[0],
                view.direction[1],
                view.direction[2],
            );
            named.height = view.height;
            named.width = view.height * aspect;
            named.lens_length = view.lens;
            named.perspective = true;
            named.handle = match document.views.get(&view.name) {
                Some(old) => old.handle,
                None => document.allocate_handle(),
            };
            document.views.add_or_replace(named);
        }
        let current_layer = document.header.current_layer_name.clone();
        let scene_materials = &scene.materials;
        let legacy = import.globals || import.steps.iter().any(|s| !matches!(s, TdsStep::Part(_)));
        if legacy {
            self.define_tds_legacy(i, &import);
        }
        let scene = &mut self.tabs[i].scene;
        if import.globals {
            for (color, spec) in crate::io::tds_legacy::RENDER_SETTINGS {
                scene.add_entity(legacy_insert("AVE_RENDER", Some(color), spec, "AVE_RENDER"));
            }
            scene.add_entity(legacy_insert(
                "AVE_GLOBAL",
                Some(1),
                crate::io::tds_legacy::GLOBAL_MATERIAL,
                "AVE_MATERIAL",
            ));
        }
        let mut parts: Vec<Option<crate::io::tds_import::TdsPart>> =
            import.parts.into_iter().map(Some).collect();
        let mut first_material = true;
        // Asked about meshes one by one, each mesh also links to its
        // material record.
        let asked = request.asked;
        let mut records: HashMap<String, document::Handle> = HashMap::new();
        let mut waiting: Vec<(String, document::Handle)> = Vec::new();
        for step in import.steps {
            match step {
                TdsStep::Part(k) => {
                    let Some(part) = parts.get_mut(k).and_then(Option::take) else {
                        continue;
                    };
                    let mut mesh = part.mesh;
                    mesh.common.layer = part.layer.clone();
                    if let Some(color) = part.color {
                        mesh.common.color = document::types::Color::Index(color);
                    }
                    mesh.common.material_handle = part
                        .material
                        .as_ref()
                        .and_then(|m| materials.get(m).copied());
                    if mesh.common.material_handle.is_some() {
                        mesh.common.material_flags = 0b11;
                    }
                    let record = part.material.as_ref().and_then(|m| records.get(m));
                    if let Some(&record) = record.filter(|_| asked) {
                        let finish = crate::io::tds_legacy::finish_xdata(record);
                        mesh.common.extended_data.add_record(finish);
                    }
                    if legacy {
                        mesh.common
                            .extended_data
                            .add_record(crate::io::tds_legacy::xdata(
                                "AVE_ENTITY_MATERIAL",
                                crate::io::tds_legacy::ENTITY_MATERIAL,
                            ));
                    }
                    let handle = scene.add_entity(document::EntityType::PolyfaceMesh(mesh));
                    if let Some(m) = part.material.filter(|m| asked && !records.contains_key(m)) {
                        waiting.push((m, handle));
                    }
                }
                TdsStep::Material(name) => {
                    let source = scene_materials.iter().find(|m| m.name == name);
                    let unit = |c: Option<[u8; 3]>| {
                        c.unwrap_or([0; 3]).map(|v| f64::from(f32::from(v) / 255.0))
                    };
                    let colours = [
                        unit(source.and_then(|m| m.ambient)),
                        unit(source.and_then(|m| m.diffuse)),
                        unit(source.and_then(|m| m.specular)),
                    ];
                    let finish = crate::io::tds_legacy::MaterialFinish::of(source);
                    let handle = materials.get(&name);
                    let handle = handle
                        .map(|h| format!("{:X}", h.value()))
                        .unwrap_or_default();
                    let mut insert =
                        document::entities::Insert::new("RM_SDB", document::types::Vector3::ZERO);
                    insert.common.layer = current_layer.clone();
                    insert
                        .common
                        .extended_data
                        .add_record(crate::io::tds_legacy::material_xdata(
                            &name,
                            colours,
                            &finish,
                            first_material,
                            &handle,
                        ));
                    first_material = false;
                    let values = crate::io::tds_legacy::material_values(
                        &name,
                        colours[1],
                        finish.roughness,
                        asked,
                    );
                    with_attributes(
                        &mut insert,
                        crate::io::tds_legacy::material_block(),
                        |tag| {
                            values
                                .iter()
                                .find(|(t, _)| *t == tag)
                                .map(|(_, v)| v.clone())
                        },
                    );
                    let record = scene.add_entity(document::EntityType::Insert(insert));
                    for (_, mesh) in waiting.iter().filter(|(m, _)| *m == name) {
                        if let Some(entity) = scene.document.get_entity_mut(*mesh) {
                            let finish = crate::io::tds_legacy::finish_xdata(record);
                            let data = &mut entity.common_mut().extended_data;
                            let rest: Vec<_> = data.records().to_vec();
                            *data = document::xdata::ExtendedData::new();
                            data.add_record(finish);
                            for r in rest {
                                data.add_record(r);
                            }
                        }
                    }
                    waiting.retain(|(m, _)| *m != name);
                    records.insert(name, record);
                }
                TdsStep::Light {
                    name,
                    position,
                    color,
                    spot,
                } => {
                    let at = document::types::Vector3::new(position[0], position[1], position[2]);
                    let (block, definitions, record) = match spot {
                        None => (
                            "overhead",
                            crate::io::tds_legacy::light_block(),
                            crate::io::tds_legacy::light_xdata(&name, color),
                        ),
                        Some((target, hotspot, falloff)) => (
                            "sh_spot",
                            crate::io::tds_legacy::spot_block(),
                            crate::io::tds_legacy::spot_xdata(
                                &name, target, color, hotspot, falloff,
                            ),
                        ),
                    };
                    let mut insert = document::entities::Insert::new(block, at);
                    if let Some((target, ..)) = spot {
                        aim_at(
                            &mut insert,
                            at,
                            document::types::Vector3::new(target[0], target[1], target[2]),
                        );
                    }
                    insert.common.layer = current_layer.clone();
                    insert.common.extended_data.add_record(record);
                    with_attributes(&mut insert, definitions, |tag| {
                        (tag == "SNAME").then(|| name.clone())
                    });
                    scene.add_entity(document::EntityType::Insert(insert));
                }
            }
        }
        for line in &import.messages {
            self.command_line.push_output(line);
        }
        // Each missing texture map is also shown in a window of its own.
        self.import_dialogs.tds_maps = import.missing_maps;
        if !self.import_dialogs.tds_maps.is_empty() {
            self.active_modal = Some(crate::app::ModalKind::TdsMissingMap);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }
}

/// A legacy render settings insert at the origin on the render layer.
fn legacy_insert(block: &str, color: Option<u8>, spec: &str, app: &str) -> document::EntityType {
    let mut insert = document::entities::Insert::new(block, document::types::Vector3::ZERO);
    insert.common.layer = crate::io::tds_legacy::LAYER.into();
    if let Some(c) = color {
        insert.common.color = document::types::Color::Index(c);
    }
    insert
        .common
        .extended_data
        .add_record(crate::io::tds_legacy::xdata(app, spec));
    document::EntityType::Insert(insert)
}

/// Turns a spotlight's insert so the block's Y axis points from the
/// light at `from` to its target, its X axis level: the insert's normal,
/// rotation and (object coordinate) point.
fn aim_at(
    insert: &mut document::entities::Insert,
    from: document::types::Vector3,
    to: document::types::Vector3,
) {
    use glam::DVec3;
    let y = DVec3::new(to.x - from.x, to.y - from.y, to.z - from.z).normalize_or_zero();
    let x = y.cross(DVec3::Z).try_normalize().unwrap_or(DVec3::X);
    let n = x.cross(y).normalize_or_zero();
    if n == DVec3::ZERO {
        return;
    }
    // The object coordinate system of the normal (arbitrary axis rule).
    let ax = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 {
        DVec3::Y.cross(n)
    } else {
        DVec3::Z.cross(n)
    }
    .normalize();
    let ay = n.cross(ax);
    let p = DVec3::new(from.x, from.y, from.z);
    insert.normal = document::types::Vector3::new(n.x, n.y, n.z);
    insert.rotation = x.dot(ay).atan2(x.dot(ax));
    insert.insert_point = document::types::Vector3::new(p.dot(ax), p.dot(ay), p.dot(n));
}

/// Gives an insert the attributes of a block's definitions (the legacy
/// blocks' base is the origin), with `value` for a tag (else the
/// definition's default).
fn with_attributes(
    insert: &mut document::entities::Insert,
    definitions: Vec<document::EntityType>,
    value: impl Fn(&str) -> Option<String>,
) {
    let transform = crate::scene::render_graph::insert_transform_with_base(
        insert,
        document::types::Vector3::ZERO,
    );
    for entity in definitions {
        let document::EntityType::AttributeDefinition(def) = entity else {
            continue;
        };
        let mut attribute =
            document::entities::AttributeEntity::from_definition(&def, value(&def.tag));
        // A centred or right-justified value starts its share of the drawn
        // width (start to last ink) before the alignment point.
        use document::entities::HorizontalAlignment as H;
        let share = match attribute.horizontal_alignment {
            H::Center | H::Middle => 0.5,
            H::Right => 1.0,
            _ => 0.0,
        };
        if share > 0.0 {
            if let Some(bounds) = crate::entities::text_support::text_local_bounds(
                crate::io::tds_legacy::FONT,
                &attribute.value,
                attribute.height as f32,
                attribute.width_factor as f32,
                attribute.oblique_angle as f32,
            ) {
                attribute.insertion_point.x =
                    attribute.alignment_point.x - share * f64::from(bounds.ink_max[0]);
            }
        }
        document::entities::Entity::apply_transform(&mut attribute, &transform);
        insert.attributes.push(attribute);
    }
}

impl OpenCADStudio {
    /// The layer, style, application names and blocks the legacy render
    /// records of a 3D Studio import use.
    fn define_tds_legacy(&mut self, i: usize, import: &crate::io::tds_import::TdsImport) {
        use crate::io::tds_legacy::{light_block, material_block, LAYER};
        let scene = &mut self.tabs[i].scene;
        for app in [
            "AVE_RENDER",
            "AVE_MATERIAL",
            "AVE_FINISH",
            "AVE_ENTITY_MATERIAL",
        ] {
            scene.ensure_app_id(app);
        }
        // The render layer is locked before the records go on it.
        scene.ensure_layer(LAYER);
        if let Some(layer) = scene.document.layers.get_mut(LAYER) {
            layer.flags.locked = true;
        }
        scene.layer_table_dirty = true;
        let document = &mut scene.document;
        let lights = import
            .steps
            .iter()
            .any(|s| matches!(s, TdsStep::Light { .. }));
        if lights
            && !document
                .text_styles
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(LAYER))
        {
            let mut style = document::tables::TextStyle::new(LAYER);
            style.handle = document.allocate_handle();
            style.height = 0.2;
            style.last_height = 2.5;
            style.font_file = crate::io::tds_legacy::FONT.into();
            let _ = document.text_styles.add(style);
        }
        let mut blocks: Vec<(&str, Vec<document::EntityType>, &str)> = Vec::new();
        if import.globals {
            blocks.push(("AVE_RENDER", Vec::new(), LAYER));
            blocks.push(("AVE_GLOBAL", Vec::new(), LAYER));
        }
        if import
            .steps
            .iter()
            .any(|s| matches!(s, TdsStep::Material(_)))
        {
            blocks.push(("RM_SDB", material_block(), LAYER));
        }
        let spot = |s: &TdsStep| matches!(s, TdsStep::Light { spot: Some(_), .. });
        if import
            .steps
            .iter()
            .any(|s| matches!(s, TdsStep::Light { spot: None, .. }))
        {
            blocks.push(("overhead", light_block(), "0"));
        }
        if import.steps.iter().any(spot) {
            blocks.push(("sh_spot", crate::io::tds_legacy::spot_block(), "0"));
        }
        for (name, entities, layer) in blocks {
            if scene.document.block_records.get(name).is_some() {
                continue;
            }
            scene.define_block_raw(name, document::types::Vector3::ZERO, entities);
            let begin = scene
                .document
                .block_records
                .get(name)
                .map(|b| b.block_entity_handle);
            if let Some(common) = begin
                .and_then(|h| scene.document.get_entity_mut(h))
                .map(|e| e.common_mut())
            {
                common.layer = layer.into();
            }
        }
    }
}

/// Bench bridge for the UI-thread cost of a model import
/// (`benches/model_import.rs`). Holds a hermetic app and imports a file into
/// a fresh drawing, reporting what the UI thread runs of the import.
#[doc(hidden)]
#[cfg(any(test, not(target_arch = "wasm32")))]
pub struct ModelImportBenchHarness {
    app: OpenCADStudio,
}

#[cfg(any(test, not(target_arch = "wasm32")))]
impl ModelImportBenchHarness {
    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new();
        app.apply_config(crate::app::config::AppConfig::default());
        app.last_saved_config = Some(app.current_config());
        app
    }

    pub fn setup() -> Self {
        let mut app = Self::new_app();
        app.automation_op(r#"{"op":"new"}"#);
        Self { app }
    }

    /// The drawing's unit in millimetres, as the import scales to it.
    pub fn drawing_mm(&self) -> Option<f64> {
        crate::app::properties::insunits_to_mm(
            self.app.tabs[self.app.active_tab]
                .scene
                .document
                .header
                .insertion_units,
        )
    }

    /// Imports `path` as `format` into a fresh drawing and reports
    /// `(ui, total, sink)` in nanoseconds: the UI-thread work once the parsed
    /// result arrives, the whole import, and a hash of what the drawing holds
    /// afterwards. The read runs here rather than on the worker the
    /// production entry hands it to, because a bench cannot drive the
    /// returned task.
    pub fn import(&mut self, path: &str, format: ModelFormat) -> (u64, u64, u64) {
        self.reset();
        let i = self.app.active_tab;
        let tab_id = self.app.tabs[i].id;
        let generation = self.app.tabs[i].document_generation;
        let drawing_mm = self.drawing_mm();
        let started = std::time::Instant::now();
        let result = model_import::load(format, path, drawing_mm);
        let loaded = started.elapsed().as_nanos() as u64;
        let apply = std::time::Instant::now();
        let _ = self
            .app
            .finish_model_import(tab_id, generation, path, format, result);
        let ui = apply.elapsed().as_nanos() as u64;
        (ui, loaded + ui, self.sink())
    }

    /// A fresh drawing with no history carried from the previous sample.
    fn reset(&mut self) {
        self.app.automation_op(r#"{"op":"new"}"#);
        let i = self.app.active_tab;
        self.app.tabs[i].history = Default::default();
        self.app.tabs[i].dirty = false;
    }

    /// Everything the import leaves in the drawing: entity and block counts,
    /// block names, and each object's type, layer, colour and geometry.
    fn sink(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let document = &self.app.tabs[self.app.active_tab].scene.document;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        document.entity_count().hash(&mut hasher);
        let mut names: Vec<&str> = document
            .block_records
            .iter()
            .map(|block| block.name.as_str())
            .collect();
        names.sort();
        names.hash(&mut hasher);
        for block in document.block_records.iter() {
            block.entity_handles.len().hash(&mut hasher);
        }
        for entity in document.entities() {
            std::mem::discriminant(entity).hash(&mut hasher);
            entity.common().layer.hash(&mut hasher);
            entity.common().color.index().hash(&mut hasher);
            match entity {
                document::EntityType::Region(region) => {
                    region.acis_data.sat_data.hash(&mut hasher);
                }
                document::EntityType::Surface(surface) => {
                    surface.acis_data.sat_data.hash(&mut hasher);
                }
                document::EntityType::Spline(spline) => {
                    spline.degree.hash(&mut hasher);
                    for point in &spline.control_points {
                        point.x.to_bits().hash(&mut hasher);
                        point.y.to_bits().hash(&mut hasher);
                        point.z.to_bits().hash(&mut hasher);
                    }
                }
                document::EntityType::Insert(insert) => {
                    insert.block_name.hash(&mut hasher);
                    insert.insert_point.x.to_bits().hash(&mut hasher);
                    insert.insert_point.y.to_bits().hash(&mut hasher);
                    insert.rotation.to_bits().hash(&mut hasher);
                }
                _ => {}
            }
        }
        hasher.finish()
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;

    fn app_with_new_drawing() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// The import places nothing itself: the file is read and parsed on a
    /// worker, and only the finished result changes the drawing.
    #[test]
    fn an_import_is_not_applied_before_its_result_arrives() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/step/box.step");
        let blocks = app.tabs[i].scene.document.block_records.len();
        let _task = app.import_model(i, path, ModelFormat::Step);
        assert_eq!(
            app.tabs[i].scene.document.entity_count(),
            0,
            "the worker reads the file; nothing is placed until its result arrives"
        );
        assert_eq!(app.tabs[i].scene.document.block_records.len(), blocks);
    }

    /// The message the reader sends is what places the import: driving the
    /// task `import_model` returns runs the whole seam, worker and message
    /// included.
    #[test]
    fn a_finished_import_reaches_its_drawing_through_its_message() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/step/box.step");
        let task = app.import_model(i, path, ModelFormat::Step);
        app.drive_headless_task(task).expect("the import task");
        assert!(
            app.tabs[i].scene.document.entity_count() > 0,
            "the result the worker sends is placed by its message"
        );
        assert!(app.tabs[i].dirty);
    }

    fn line(y: f64) -> document::EntityType {
        document::EntityType::Line(document::entities::Line::from_coords(
            0.0, y, 0.0, 10.0, y, 0.0,
        ))
    }

    fn direct_insert(block: &str) -> document::EntityType {
        document::EntityType::Insert(document::entities::Insert::new(
            block,
            document::types::Vector3::ZERO,
        ))
    }

    fn inserts(app: &OpenCADStudio, i: usize) -> Vec<String> {
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert.block_name.clone()),
                _ => None,
            })
            .collect()
    }

    /// The finished result is placed as one undoable edit: the block, then
    /// the objects that reference it.
    #[test]
    fn a_finished_import_places_its_blocks_and_objects() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tab_id = app.tabs[i].id;
        let generation = app.tabs[i].document_generation;
        let imported = model_import::Imported {
            blocks: vec![("Part".to_string(), vec![line(0.0)])],
            entities: vec![line(1.0), direct_insert("Part")],
        };
        let _ = app.finish_model_import(
            tab_id,
            generation,
            "model.step",
            ModelFormat::Step,
            Ok(imported),
        );
        assert!(app.tabs[i]
            .scene
            .document
            .block_records
            .get("Part")
            .is_some());
        // The two objects, plus the block's own line.
        assert_eq!(app.tabs[i].scene.document.entity_count(), 3);
        assert_eq!(inserts(&app, i), vec!["Part".to_string()]);
        assert!(app.tabs[i].dirty);
    }

    /// A block the drawing already has is not redefined: the imported one
    /// takes a name of its own and its inserts are pointed at that name.
    #[test]
    fn a_block_name_the_drawing_uses_is_not_redefined() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tab_id = app.tabs[i].id;
        let generation = app.tabs[i].document_generation;
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(vec![line(5.0)], "Part", glam::DVec3::ZERO)
            .expect("the existing block");
        let existing = app.tabs[i]
            .scene
            .document
            .block_records
            .get("Part")
            .map(|block| block.entity_handles.len());
        let imported = model_import::Imported {
            blocks: vec![("Part".to_string(), vec![line(0.0)])],
            entities: vec![direct_insert("Part")],
        };
        let _ = app.finish_model_import(
            tab_id,
            generation,
            "model.step",
            ModelFormat::Step,
            Ok(imported),
        );
        assert!(app.tabs[i]
            .scene
            .document
            .block_records
            .get("Part_1")
            .is_some());
        assert_eq!(inserts(&app, i), vec!["Part_1".to_string()]);
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .get("Part")
                .map(|block| block.entity_handles.len()),
            existing
        );
    }

    #[test]
    fn a_missing_file_reports_and_leaves_the_drawing_alone() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tab_id = app.tabs[i].id;
        let generation = app.tabs[i].document_generation;
        let _ = app.finish_model_import(
            tab_id,
            generation,
            "missing.step",
            ModelFormat::Step,
            Err(model_import::ImportFailure::Missing),
        );
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("File missing.step does not exist.")
        );
        assert_eq!(app.tabs[i].scene.document.entity_count(), 0);
        assert!(!app.tabs[i].dirty);
    }

    #[test]
    fn an_unreadable_file_reports_and_leaves_the_drawing_alone() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tab_id = app.tabs[i].id;
        let generation = app.tabs[i].document_generation;
        let _ = app.finish_model_import(
            tab_id,
            generation,
            "notes.step",
            ModelFormat::Step,
            Err(model_import::ImportFailure::Unreadable),
        );
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Import failed: notes.step has no model the importer reads.")
        );
        assert_eq!(app.tabs[i].scene.document.entity_count(), 0);
        assert!(!app.tabs[i].dirty);
    }

    #[test]
    fn a_finished_import_into_a_closed_drawing_is_dropped() {
        let mut app = app_with_new_drawing();
        let imported = model_import::Imported {
            blocks: Vec::new(),
            entities: vec![line(0.0)],
        };
        let _ = app.finish_model_import(u64::MAX, 0, "model.step", ModelFormat::Step, Ok(imported));
        assert_eq!(
            app.tabs[app.active_tab].scene.document.entity_count(),
            0,
            "the drawing is gone; nothing is added to another one"
        );
        assert!(app.command_line.last_error.is_none());
        assert!(
            app.command_line
                .history
                .iter()
                .any(|entry| entry.text.contains("was closed")),
            "the command line says the drawing was closed, not replaced"
        );
    }

    /// A stale import's file error does not blame the file: the drawing it
    /// was reading for is already gone.
    #[test]
    fn a_stale_import_error_names_the_lost_drawing() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tab_id = app.tabs[i].id;
        let generation = app.tabs[i].document_generation;
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.finish_model_import(
            tab_id,
            generation,
            "missing.step",
            ModelFormat::Step,
            Err(model_import::ImportFailure::Missing),
        );
        assert!(app.command_line.last_error.is_none());
        assert!(app
            .command_line
            .history
            .iter()
            .any(|entry| entry.text.contains("was replaced")));
    }

    /// A tab keeps its id when a whole document replaces its content (open
    /// into an empty tab, template, `new`, CLEAR), so a result headed for the
    /// old document must not land in the new one.
    #[test]
    fn a_finished_import_into_a_replaced_drawing_is_dropped() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tab_id = app.tabs[i].id;
        let generation = app.tabs[i].document_generation;
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let imported = model_import::Imported {
            blocks: Vec::new(),
            entities: vec![line(0.0)],
        };
        let _ = app.finish_model_import(
            tab_id,
            generation,
            "model.step",
            ModelFormat::Step,
            Ok(imported),
        );
        assert_eq!(
            app.tabs[i].scene.document.entity_count(),
            0,
            "the document the import was reading for is gone; nothing is added to its successor"
        );
        assert!(!app.tabs[i].dirty);
        assert!(
            app.command_line
                .history
                .iter()
                .any(|entry| entry.text.contains("was replaced")),
            "the command line says which drawing was lost"
        );
    }

    /// CLEAR empties the drawing in place, without a file load; a result
    /// headed for what was cleared must not land in the cleared drawing.
    #[test]
    fn a_finished_import_into_a_cleared_drawing_is_dropped() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tab_id = app.tabs[i].id;
        let generation = app.tabs[i].document_generation;
        let _ = app.update(crate::app::Message::ClearScene);
        let imported = model_import::Imported {
            blocks: Vec::new(),
            entities: vec![line(0.0)],
        };
        let _ = app.finish_model_import(
            tab_id,
            generation,
            "model.step",
            ModelFormat::Step,
            Ok(imported),
        );
        assert_eq!(
            app.tabs[i].scene.document.entity_count(),
            0,
            "the drawing the import was reading for was cleared; nothing is added"
        );
    }

    /// The ribbon's Clear tool empties the drawing by its own route; a
    /// result in flight for the cleared drawing is dropped there too.
    #[test]
    fn a_finished_import_into_a_drawing_cleared_from_the_ribbon_is_dropped() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let tab_id = app.tabs[i].id;
        let generation = app.tabs[i].document_generation;
        let _ = app.update(crate::app::Message::RibbonToolClick {
            tool_id: "CLEAR".to_string(),
            event: crate::modules::ModuleEvent::ClearModels,
        });
        let imported = model_import::Imported {
            blocks: Vec::new(),
            entities: vec![line(0.0)],
        };
        let _ = app.finish_model_import(
            tab_id,
            generation,
            "model.step",
            ModelFormat::Step,
            Ok(imported),
        );
        assert_eq!(
            app.tabs[i].scene.document.entity_count(),
            0,
            "the ribbon cleared the drawing the import was reading for; nothing is added"
        );
    }
}

#[cfg(test)]
mod picture_import_tests {
    use super::*;

    fn app_with_new_drawing() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn fixture(name: &str) -> String {
        format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    fn last_line(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .last()
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    }

    fn has_block(app: &OpenCADStudio, i: usize, name: &str) -> bool {
        app.tabs[i].scene.document.block_records.get(name).is_some()
    }

    fn undo_steps(app: &mut OpenCADStudio, i: usize) -> usize {
        app.finish_all_pending_history();
        app.tabs[i].history.undo_stack.len()
    }

    /// The import places nothing itself: the metafile is read on a worker,
    /// the command line says so, and only the result places the block.
    #[test]
    fn a_metafile_is_placed_only_when_its_read_ends() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = fixture("wmf/shapes.wmf");
        let task = app.import_wmf(i, &path, false);
        assert!(
            !has_block(&app, i, "WMF0"),
            "nothing is placed during the read"
        );
        assert_eq!(last_line(&app), format!("ⓘ Reading {path}…"));
        app.drive_headless_task(task).expect("the import task");
        assert!(has_block(&app, i, "WMF0"), "the result places the block");
        assert!(app.tabs[i].dirty);
    }

    /// The same for an SVG file.
    #[test]
    fn an_svg_drawing_is_placed_only_when_its_read_ends() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = "picture-import-test-square.svg";
        crate::scene::model::pdf_raster::register_source(
            path,
            std::sync::Arc::new(
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="8" height="8"/></svg>"#
                    .to_vec(),
            ),
        );
        let task = app.import_svg(i, path, false);
        assert!(
            !has_block(&app, i, "SVG0"),
            "nothing is placed during the read"
        );
        assert_eq!(last_line(&app), format!("ⓘ Reading {path}…"));
        app.drive_headless_task(task).expect("the import task");
        assert!(has_block(&app, i, "SVG0"), "the result places the block");
    }

    /// The picture fits half the view in which it was started: a wide one
    /// by the width of the view, a tall one by its height.
    #[test]
    fn a_picture_fits_half_the_view() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let (_, view, _) = app.tabs[i].scene.view_window();
        for (n, (name, svg)) in [
            ("wide", r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="10"><rect width="100" height="10" fill="none" stroke="black"/></svg>"#),
            ("tall", r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="100"><rect width="10" height="100" fill="none" stroke="black"/></svg>"#),
        ]
        .into_iter()
        .enumerate()
        {
            let path = format!("picture-import-test-{name}.svg");
            crate::scene::model::pdf_raster::register_source(
                &path,
                std::sync::Arc::new(svg.as_bytes().to_vec()),
            );
            let task = app.import_svg(i, &path, false);
            app.drive_headless_task(task).expect("the import task");
            let wires = app.tabs[i].scene.block_preview_wires(&format!("SVG{n}"));
            assert!(!wires.is_empty(), "{name}: the block has objects");
            let [x0, y0, x1, y1] = wires.iter().fold(
                [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
                |[a, b, c, d], w| [a.min(w.aabb[0]), b.min(w.aabb[1]), c.max(w.aabb[2]), d.max(w.aabb[3])],
            );
            let (long, side) = if name == "wide" {
                (f64::from(x1 - x0), view[0])
            } else {
                (f64::from(y1 - y0), view[1])
            };
            // The wire boxes are a little larger than the shapes (1 % here); a fit
            // from the wrong view size is about twice as large or near zero.
            assert!(
                long <= 0.5 * side * 1.05 && long >= 0.25 * side,
                "{name}: {long} against half of the view side {side}"
            );
        }
    }

    /// A model import also says that it reads the file (TODO 2.2).
    #[test]
    fn a_model_import_says_that_it_reads_the_file() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let path = fixture("step/box.step");
        let _task = app.import_model(i, &path, ModelFormat::Step);
        assert_eq!(last_line(&app), format!("ⓘ Reading {path}…"));
    }

    /// A reader that panics gives an error on the command line. The drawing
    /// does not change, no undo step is added, and the next import works.
    #[test]
    fn a_reader_panic_is_an_error_and_the_app_continues() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let undo = undo_steps(&mut app, i);
        let blocks = app.tabs[i].scene.document.block_records.len();
        let task = app.read_picture_with(
            i,
            PictureFormat::Wmf,
            PictureSource::File("panics.wmf".to_string()),
            false,
            |_, _, _| panic!("a reader bug"),
        );
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("IMPORT failed: a reader bug")
        );
        assert_eq!(app.tabs[i].scene.document.block_records.len(), blocks);
        assert_eq!(app.tabs[i].scene.document.entity_count(), 0);
        assert_eq!(undo_steps(&mut app, i), undo);
        let task = app.import_wmf(i, &fixture("wmf/shapes.wmf"), false);
        app.drive_headless_task(task).expect("the next import task");
        assert!(has_block(&app, i, "WMF0"), "the next import works");
    }

    /// A result for a drawing that was replaced during the read is dropped.
    #[test]
    fn a_picture_read_for_a_replaced_drawing_is_dropped() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let task = app.import_wmf(i, &fixture("wmf/shapes.wmf"), false);
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.drive_headless_task(task).expect("the import task");
        assert!(!has_block(&app, i, "WMF0"));
        assert!(!app.tabs[i].dirty);
        assert!(last_line(&app).contains("was replaced"));
    }

    /// A result for a drawing that was closed during the read is dropped.
    #[test]
    fn a_picture_read_for_a_closed_drawing_is_dropped() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let read = picture_import::read(
            PictureFormat::Wmf,
            &std::fs::read(fixture("wmf/shapes.wmf")).unwrap(),
            [100.0, 100.0],
        );
        let _ = app.finish_picture_import(
            u64::MAX,
            0,
            Some("shapes.wmf"),
            PictureFormat::Wmf,
            false,
            read,
        );
        assert!(!has_block(&app, i, "WMF0"));
        assert!(app.command_line.last_error.is_none());
        assert!(last_line(&app).contains("was closed"));
    }

    /// WMFIN places the block with the insertion prompts. If a different
    /// command started during the read, the block is not placed, and that
    /// command stays.
    #[test]
    fn a_command_started_during_the_read_stays_and_the_picture_is_not_placed() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let undo = undo_steps(&mut app, i);
        let task = app.import_wmf(i, &fixture("wmf/shapes.wmf"), true);
        let _ = app.update(Message::Command("LINE".to_string()));
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .map(|command| command.name()),
            Some("LINE")
        );
        assert!(!has_block(&app, i, "WMF0"));
        assert_eq!(undo_steps(&mut app, i), undo);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("WMFIN cancelled: a different command started while the picture was read.")
        );
    }

    /// The file name typed at WMFIN's prompt (FILEDIA off) ends that prompt
    /// before the read, so the insertion prompts place the block.
    #[test]
    fn a_file_name_typed_at_the_prompt_is_placed_by_the_insertion_prompts() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        app.tabs[i].active_cmd = Some(Box::new(crate::command::FreeTextValuePromptCommand::new(
            "_WMFINFILE",
            "Enter name of WMF file to import (no extension):",
        )));
        let task = app.feed_command(crate::command::StepInput::Text(fixture("wmf/shapes.wmf")));
        app.drive_headless_task(task).expect("the import task");
        assert!(has_block(&app, i, "WMF0"));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .map(|command| command.name()),
            Some("-INSERT"),
            "the insertion prompts place the block"
        );
    }

    /// Pasted markup that is not SVG gives the clipboard's message.
    #[test]
    fn pasted_markup_that_is_not_svg_gives_the_clipboard_message() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let blocks = app.tabs[i].scene.document.block_records.len();
        let task = app.update(Message::SystemClipboardPaste(
            crate::app::SystemClipboardText::Text("<svg this is not markup".to_string()),
        ));
        app.drive_headless_task(task).expect("the paste task");
        assert_eq!(app.tabs[i].scene.document.block_records.len(), blocks);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(crate::tr!("clipboard", "no-supported-content").as_str())
        );
    }

    /// A file that is not there gives the same message as before.
    #[test]
    fn a_missing_picture_file_is_reported() {
        let mut app = app_with_new_drawing();
        let i = app.active_tab;
        let task = app.import_svg(i, "picture-import-test-not-here.svg", true);
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("File picture-import-test-not-here.svg does not exist.")
        );
        assert!(app.tabs[i].active_cmd.is_none());
    }
}

#[cfg(test)]
mod svg_paste_tests {
    #[test]
    fn copied_svg_pastes_back_as_drawing() {
        let mut app = crate::app::OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(document::EntityType::Line(
            document::entities::Line::from_coords(0.0, 0.0, 0.0, 40.0, 20.0, 0.0),
        ));
        let svg = crate::io::svg_clipboard::selection_svg(&app.tabs[i].scene, &[line])
            .expect("the line draws");
        let blocks = app.tabs[i].scene.document.block_records.len();
        // The markup is read on a worker; its result places the block.
        let task = app.update(crate::app::Message::SystemClipboardPaste(
            crate::app::SystemClipboardText::Text(svg),
        ));
        app.drive_headless_task(task).expect("the paste task");
        assert_eq!(app.tabs[i].scene.document.block_records.len(), blocks + 1);
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "placed by the insertion prompts"
        );
    }
}
