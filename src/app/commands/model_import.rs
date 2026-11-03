//! ACISIN and WMFIN, and the files IMPORT routes here: ACIS text (SAT),
//! STEP and IGES models and Windows metafiles, read into the current drawing
//! as one undo step.

use super::*;
use crate::io::tds_import::TdsStep;
use std::collections::HashMap;

use crate::io::model_import::{self, ModelFormat};

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
                Some(Task::perform(super::dgn_import::pick(title, filters), |path| match path {
                    Some(path) => Message::Command(format!("_ACISINFILE {path}")),
                    None => Message::Noop,
                }))
            }
            ("ACISIN", true) => {
                use crate::command::FreeTextValuePromptCommand;
                let command = FreeTextValuePromptCommand::new("_ACISINFILE", "Enter SAT file name:");
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
                    self.command_line
                        .push_error(&format!("\"{name}\": file not found in the current directory, {here}"));
                    return Some(Task::none());
                }
                Some(self.import_model(i, rest, ModelFormat::Sat))
            }
            ("_ACISINFILE", true) => Some(Task::none()),
            ("WMFIN", true) if crate::sys::file_dia() => {
                let filters = vec![("Metafile (*.wmf)".to_string(), vec!["wmf".to_string()])];
                let title = crate::t!("Import WMF").into_owned();
                Some(Task::perform(super::dgn_import::pick(title, filters), |path| match path {
                    Some(path) => Message::Command(format!("_WMFINFILE {path}")),
                    None => Message::Noop,
                }))
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
                let filters = vec![("SVG (*.svg; *.svgz)".to_string(), vec!["svg".to_string(), "svgz".to_string()])];
                let title = crate::t!("Import SVG").into_owned();
                Some(Task::perform(super::dgn_import::pick(title, filters), |path| match path {
                    Some(path) => Message::Command(format!("_SVGINFILE {path}")),
                    None => Message::Noop,
                }))
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

    /// Reads a model file into drawing `i`.
    pub(in crate::app) fn import_model(&mut self, i: usize, path: &str, format: ModelFormat) -> Task<Message> {
        let Some(bytes) = crate::scene::model::pdf_raster::source_bytes(path) else {
            self.command_line
                .push_error(&format!("File {path} does not exist."));
            return Task::none();
        };
        let translated = format != ModelFormat::Sat;
        let drawing_mm = crate::app::properties::insunits_to_mm(
            self.tabs[i].scene.document.header.insertion_units,
        );
        let stem = std::path::Path::new(&path.replace('\\', "/"))
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(imported) = model_import::read(format, &bytes, drawing_mm, &stem) else {
            self.command_line
                .push_error(&format!("Import failed: {path} has no model the importer reads."));
            return Task::none();
        };
        self.push_undo_snapshot(i, "IMPORT");
        let scene = &mut self.tabs[i].scene;
        // Each block goes under a name the drawing does not use yet (a
        // product named like a block already there gets a new name, not the
        // old definition), and the inserts are pointed at the names the
        // blocks got. An insert of a block that could not be made is left
        // out.
        fn retarget(entities: Vec<codec::EntityType>, defined: &HashMap<String, String>) -> Vec<codec::EntityType> {
            entities
                .into_iter()
                .filter_map(|entity| match entity {
                    codec::EntityType::Insert(mut insert) => {
                        insert.block_name = defined.get(&insert.block_name)?.clone();
                        Some(codec::EntityType::Insert(insert))
                    }
                    other => Some(other),
                })
                .collect()
        }
        let mut taken: Vec<String> = scene.document.block_records.iter().map(|b| b.name.clone()).collect();
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
    /// prompts (WMFIN).
    pub(in crate::app) fn import_wmf(&mut self, i: usize, path: &str, prompt: bool) -> Task<Message> {
        let Some(bytes) = crate::scene::model::pdf_raster::source_bytes(path) else {
            self.command_line
                .push_error(&format!("File {path} does not exist."));
            return Task::none();
        };
        let (_, size, _) = self.tabs[i].scene.view_window();
        let Some(mut picture) =
            crate::io::wmf_import::read(&bytes, [0.5 * size[0], 0.5 * size[1]])
        else {
            self.command_line
                .push_error(&format!("{path} is not a valid Windows metafile."));
            return Task::none();
        };
        self.push_undo_snapshot(i, if prompt { "WMFIN" } else { "IMPORT" });
        let scene = &mut self.tabs[i].scene;
        // A text style per face, `WMF-<face><n>` with `n` counting the
        // face's bold and italic variants; the style's font is the
        // installed file of that face, none when it is not installed. A
        // style the drawing already has is used as it is.
        let mut styles: Vec<String> = Vec::new();
        for (k, face) in picture.fonts.iter().enumerate() {
            let n = picture.fonts[..k].iter().filter(|f| f.name == face.name).count();
            // The face name comes from the file.
            let name = crate::scene::sanitized_table_name(&format!("WMF-{}{n}", face.name), "WMF");
            if !scene.document.text_styles.contains(&name) {
                let mut style = codec::tables::TextStyle::new(name.as_str());
                style.handle = scene.document.allocate_handle();
                style.font_file =
                    crate::scene::text::sysfont::face_file(&face.name, face.bold, face.italic).unwrap_or_default();
                let _ = scene.document.text_styles.add(style);
            }
            styles.push(name);
        }
        for (at, font) in &picture.text_fonts {
            if let Some(codec::EntityType::Text(text)) = picture.entities.get_mut(*at) {
                text.style = styles[*font].clone();
            }
        }
        self.place_picture_block(i, "WMF", picture.entities, prompt)
    }

    /// An SVG drawing as a new block `SVG<n>`, fitted to half the current
    /// view: placed at the view's top-left corner (IMPORT), or by the
    /// insertion prompts (SVGIN).
    pub(in crate::app) fn import_svg(&mut self, i: usize, path: &str, prompt: bool) -> Task<Message> {
        let Some(bytes) = crate::scene::model::pdf_raster::source_bytes(path) else {
            self.command_line
                .push_error(&format!("File {path} does not exist."));
            return Task::none();
        };
        let (_, size, _) = self.tabs[i].scene.view_window();
        let picture = match crate::io::svg_import::read(&bytes, [0.5 * size[0], 0.5 * size[1]]) {
            Ok(picture) => picture,
            Err(crate::io::svg_import::Error::NotSvg) => {
                self.command_line
                    .push_error(&format!("{path} is not a valid SVG file."));
                return Task::none();
            }
            Err(crate::io::svg_import::Error::Empty) => {
                self.command_line
                    .push_error(&format!("{path} has no shapes to import."));
                return Task::none();
            }
        };
        self.push_undo_snapshot(i, if prompt { "SVGIN" } else { "IMPORT" });
        self.place_picture_block(i, "SVG", picture.entities, prompt)
    }

    /// Defines `entities` as a new block `<prefix><n>` and puts it in the
    /// drawing: at the view's top-left corner, or by the insertion prompts.
    fn place_picture_block(
        &mut self,
        i: usize,
        prefix: &str,
        entities: Vec<codec::EntityType>,
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
            let corner = codec::types::Vector3::new(
                centre[0] - 0.5 * size[0] + pixel,
                centre[1] + 0.5 * size[1] - 2.0 * pixel,
                0.0,
            );
            let insert = codec::entities::Insert::new(&name, corner);
            self.tabs[i].scene.add_entity(codec::EntityType::Insert(insert));
            self.refresh_properties();
        }
        Task::none()
    }
}

impl OpenCADStudio {
    /// The 3D Studio Import window for a file; an error when it is no 3D
    /// Studio file.
    pub(in crate::app) fn open_tds_import_dialog(&mut self, path: &str) -> Result<(), String> {
        let scene = crate::scene::model::pdf_raster::source_bytes(path)
            .and_then(|bytes| crate::io::tds_import::read(&bytes))
            .ok_or_else(|| format!("{path} is not a valid 3D Studio file."))?;
        let mut objects: Vec<(String, &'static str, usize)> = scene
            .objects
            .iter()
            .map(|(name, object)| {
                let materials = match object {
                    crate::io::tds_import::TdsObject::Mesh(mesh) => scene.mesh_materials(mesh).len(),
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
        });
        self.active_modal = Some(crate::app::ModalKind::TdsImport);
        Ok(())
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
        let Some(scene) = crate::scene::model::pdf_raster::source_bytes(&state.path)
            .and_then(|bytes| crate::io::tds_import::read(&bytes))
        else {
            return Task::none();
        };
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
                            let diffuse = scene.materials.iter().find(|m| m.name == *material).and_then(|m| m.diffuse);
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
                self.import_dialogs.tds_material = Some(crate::ui::window::import_dialogs::TdsMaterialState {
                    tab: i,
                    request,
                    queue: several,
                    choice: 0,
                    material: 0,
                    list_open: false,
                    remember: false,
                });
                self.active_modal = Some(crate::app::ModalKind::TdsMaterial);
                return Task::none();
            }
            None => {}
        }
        self.run_tds_import(i, request);
        Task::none()
    }

    /// The answer for the asked mesh (and, when the choice is kept, for the
    /// rest); the import runs once no mesh is left to ask about.
    pub(in crate::app) fn answer_tds_material(&mut self) {
        use crate::io::tds_import::MultiMaterial;
        let Some(mut state) = self.import_dialogs.tds_material.take() else {
            return;
        };
        let choice = |state: &crate::ui::window::import_dialogs::TdsMaterialState,
                      materials: &[(String, Option<[u8; 3]>)],
                      chosen: Option<&str>| match state.choice {
            0 => MultiMaterial::Split,
            1 => MultiMaterial::First,
            _ => MultiMaterial::Material(
                chosen.and_then(|c| materials.iter().find(|(m, _)| m == c)).map(|(m, _)| m.clone()),
            ),
        };
        let (name, materials) = state.queue.remove(0);
        let chosen = state.material.checked_sub(1).and_then(|k| materials.get(k)).map(|(m, _)| m.clone());
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
            self.run_tds_import(state.tab, state.request);
        } else {
            // The next mesh is offered the same choice, without a material.
            state.material = 0;
            state.list_open = false;
            self.import_dialogs.tds_material = Some(state);
        }
    }

    /// Cancel: the import stops at the asked mesh; the objects before it
    /// come in.
    pub(in crate::app) fn stop_tds_import(&mut self, mut state: crate::ui::window::import_dialogs::TdsMaterialState) {
        state.request.stop_at = state.queue.first().map(|(name, _)| name.clone());
        self.run_tds_import(state.tab, state.request);
    }

    /// The import itself, one undo step: the meshes on their layers with
    /// their materials, the cameras as named views.
    pub(in crate::app) fn run_tds_import(
        &mut self,
        i: usize,
        request: crate::modules::insert::tds_import::TdsImportRequest,
    ) {
        use codec::objects::ObjectType;
        let Some(scene) = crate::scene::model::pdf_raster::source_bytes(&request.path)
            .and_then(|bytes| crate::io::tds_import::read(&bytes))
        else {
            self.command_line
                .push_error(&format!("{} is not a valid 3D Studio file.", request.path));
            return;
        };
        let import = crate::io::tds_import::convert(
            &scene,
            &crate::io::tds_import::TdsOptions {
                selected: request.selected.clone(),
                layers: request.layers,
                multi: request.multi.clone(),
                compact: request.compact,
                single_layer: self.tabs[i].scene.document.header.current_layer_name.clone(),
                stop_at: request.stop_at.clone(),
            },
        );
        // Named views take the current view's proportions.
        let (_, size, _) = self.tabs[i].scene.view_window();
        let aspect = if size[0] > 0.0 && size[1] > 0.0 { size[0] / size[1] } else { 1.0 };
        self.push_undo_snapshot(i, "IMPORT");
        let document = &mut self.tabs[i].scene.document;
        // Materials live in the material dictionary; a drawing without one
        // imports the meshes without materials.
        let dictionary = document.header.acad_material_dict_handle;
        let has_dictionary = matches!(document.objects.get(&dictionary), Some(ObjectType::Dictionary(_)));
        // Materials by name: the drawing's own when it has one, else one
        // made from the file's colours.
        let mut materials: std::collections::HashMap<String, codec::Handle> = Default::default();
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
                    let mut material = codec::objects::Material::new();
                    material.handle = document.allocate_handle();
                    material.owner = dictionary;
                    material.name = name.clone();
                    let color = |rgb: Option<[u8; 3]>| match rgb {
                        Some([r, g, b]) => codec::objects::MaterialColor {
                            flag: 1,
                            factor: 1.0,
                            rgb: Some(crate::scene::convert::acis_import::packed_true_colour(r, g, b)),
                        },
                        None => codec::objects::MaterialColor::default(),
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
                    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
                        dictionary.add_entry(name.clone(), handle);
                    }
                    document.objects.insert(handle, ObjectType::Material(material));
                    handle
                }
            };
            materials.insert(name.clone(), handle);
        }
        for part in &import.parts {
            if !document.layers.contains(&part.layer) {
                let mut layer = codec::tables::Layer::new(part.layer.as_str());
                layer.handle = document.allocate_handle();
                let _ = document.layers.add(layer);
            }
        }
        for view in &import.views {
            let mut named = codec::tables::View::new(view.name.as_str());
            named.target = codec::types::Vector3::new(view.target[0], view.target[1], view.target[2]);
            named.direction =
                codec::types::Vector3::new(view.direction[0], view.direction[1], view.direction[2]);
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
        let mut parts: Vec<Option<crate::io::tds_import::TdsPart>> = import.parts.into_iter().map(Some).collect();
        let mut first_material = true;
        // Asked about meshes one by one, each mesh also links to its
        // material record.
        let asked = request.asked;
        let mut records: HashMap<String, codec::Handle> = HashMap::new();
        let mut waiting: Vec<(String, codec::Handle)> = Vec::new();
        for step in import.steps {
            match step {
                TdsStep::Part(k) => {
                    let Some(part) = parts.get_mut(k).and_then(Option::take) else { continue };
                    let mut mesh = part.mesh;
                    mesh.common.layer = part.layer.clone();
                    if let Some(color) = part.color {
                        mesh.common.color = codec::types::Color::Index(color);
                    }
                    mesh.common.material_handle =
                        part.material.as_ref().and_then(|m| materials.get(m).copied());
                    if mesh.common.material_handle.is_some() {
                        mesh.common.material_flags = 0b11;
                    }
                    let record = part.material.as_ref().and_then(|m| records.get(m));
                    if let Some(&record) = record.filter(|_| asked) {
                        let finish = crate::io::tds_legacy::finish_xdata(record);
                        mesh.common.extended_data.add_record(finish);
                    }
                    if legacy {
                        mesh.common.extended_data.add_record(crate::io::tds_legacy::xdata(
                            "AVE_ENTITY_MATERIAL",
                            crate::io::tds_legacy::ENTITY_MATERIAL,
                        ));
                    }
                    let handle = scene.add_entity(codec::EntityType::PolyfaceMesh(mesh));
                    if let Some(m) = part.material.filter(|m| asked && !records.contains_key(m)) {
                        waiting.push((m, handle));
                    }
                }
                TdsStep::Material(name) => {
                    let source = scene_materials.iter().find(|m| m.name == name);
                    let unit = |c: Option<[u8; 3]>| c.unwrap_or([0; 3]).map(|v| f64::from(f32::from(v) / 255.0));
                    let colours = [
                        unit(source.and_then(|m| m.ambient)),
                        unit(source.and_then(|m| m.diffuse)),
                        unit(source.and_then(|m| m.specular)),
                    ];
                    let finish = crate::io::tds_legacy::MaterialFinish::of(source);
                    let handle = materials.get(&name);
                    let handle = handle.map(|h| format!("{:X}", h.value())).unwrap_or_default();
                    let mut insert = codec::entities::Insert::new("RM_SDB", codec::types::Vector3::ZERO);
                    insert.common.layer = current_layer.clone();
                    insert.common.extended_data.add_record(crate::io::tds_legacy::material_xdata(
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
                    with_attributes(&mut insert, crate::io::tds_legacy::material_block(), |tag| {
                        values.iter().find(|(t, _)| *t == tag).map(|(_, v)| v.clone())
                    });
                    let record = scene.add_entity(codec::EntityType::Insert(insert));
                    for (_, mesh) in waiting.iter().filter(|(m, _)| *m == name) {
                        if let Some(entity) = scene.document.get_entity_mut(*mesh) {
                            let finish = crate::io::tds_legacy::finish_xdata(record);
                            let data = &mut entity.common_mut().extended_data;
                            let rest: Vec<_> = data.records().to_vec();
                            *data = codec::xdata::ExtendedData::new();
                            data.add_record(finish);
                            for r in rest {
                                data.add_record(r);
                            }
                        }
                    }
                    waiting.retain(|(m, _)| *m != name);
                    records.insert(name, record);
                }
                TdsStep::Light { name, position, color, spot } => {
                    let at = codec::types::Vector3::new(position[0], position[1], position[2]);
                    let (block, definitions, record) = match spot {
                        None => (
                            "overhead",
                            crate::io::tds_legacy::light_block(),
                            crate::io::tds_legacy::light_xdata(&name, color),
                        ),
                        Some((target, hotspot, falloff)) => (
                            "sh_spot",
                            crate::io::tds_legacy::spot_block(),
                            crate::io::tds_legacy::spot_xdata(&name, target, color, hotspot, falloff),
                        ),
                    };
                    let mut insert = codec::entities::Insert::new(block, at);
                    if let Some((target, ..)) = spot {
                        aim_at(&mut insert, at, codec::types::Vector3::new(target[0], target[1], target[2]));
                    }
                    insert.common.layer = current_layer.clone();
                    insert.common.extended_data.add_record(record);
                    with_attributes(&mut insert, definitions, |tag| {
                        (tag == "SNAME").then(|| name.clone())
                    });
                    scene.add_entity(codec::EntityType::Insert(insert));
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
fn legacy_insert(block: &str, color: Option<u8>, spec: &str, app: &str) -> codec::EntityType {
    let mut insert = codec::entities::Insert::new(block, codec::types::Vector3::ZERO);
    insert.common.layer = crate::io::tds_legacy::LAYER.into();
    if let Some(c) = color {
        insert.common.color = codec::types::Color::Index(c);
    }
    insert.common.extended_data.add_record(crate::io::tds_legacy::xdata(app, spec));
    codec::EntityType::Insert(insert)
}

/// Turns a spotlight's insert so the block's Y axis points from the
/// light at `from` to its target, its X axis level: the insert's normal,
/// rotation and (object coordinate) point.
fn aim_at(insert: &mut codec::entities::Insert, from: codec::types::Vector3, to: codec::types::Vector3) {
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
    insert.normal = codec::types::Vector3::new(n.x, n.y, n.z);
    insert.rotation = x.dot(ay).atan2(x.dot(ax));
    insert.insert_point = codec::types::Vector3::new(p.dot(ax), p.dot(ay), p.dot(n));
}

/// Gives an insert the attributes of a block's definitions (the legacy
/// blocks' base is the origin), with `value` for a tag (else the
/// definition's default).
fn with_attributes(
    insert: &mut codec::entities::Insert,
    definitions: Vec<codec::EntityType>,
    value: impl Fn(&str) -> Option<String>,
) {
    let transform =
        crate::scene::render_graph::insert_transform_with_base(insert, codec::types::Vector3::ZERO);
    for entity in definitions {
        let codec::EntityType::AttributeDefinition(def) = entity else { continue };
        let mut attribute = codec::entities::AttributeEntity::from_definition(&def, value(&def.tag));
        // A centred or right-justified value starts its share of the drawn
        // width (start to last ink) before the alignment point.
        use codec::entities::HorizontalAlignment as H;
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
        codec::entities::Entity::apply_transform(&mut attribute, &transform);
        insert.attributes.push(attribute);
    }
}

impl OpenCADStudio {
    /// The layer, style, application names and blocks the legacy render
    /// records of a 3D Studio import use.
    fn define_tds_legacy(&mut self, i: usize, import: &crate::io::tds_import::TdsImport) {
        use crate::io::tds_legacy::{light_block, material_block, LAYER};
        let scene = &mut self.tabs[i].scene;
        for app in ["AVE_RENDER", "AVE_MATERIAL", "AVE_FINISH", "AVE_ENTITY_MATERIAL"] {
            scene.ensure_app_id(app);
        }
        // The render layer is locked before the records go on it.
        scene.ensure_layer(LAYER);
        if let Some(layer) = scene.document.layers.get_mut(LAYER) {
            layer.flags.locked = true;
        }
        scene.layer_table_dirty = true;
        let document = &mut scene.document;
        let lights = import.steps.iter().any(|s| matches!(s, TdsStep::Light { .. }));
        if lights && !document.text_styles.iter().any(|s| s.name.eq_ignore_ascii_case(LAYER)) {
            let mut style = codec::tables::TextStyle::new(LAYER);
            style.handle = document.allocate_handle();
            style.height = 0.2;
            style.last_height = 2.5;
            style.font_file = crate::io::tds_legacy::FONT.into();
            let _ = document.text_styles.add(style);
        }
        let mut blocks: Vec<(&str, Vec<codec::EntityType>, &str)> = Vec::new();
        if import.globals {
            blocks.push(("AVE_RENDER", Vec::new(), LAYER));
            blocks.push(("AVE_GLOBAL", Vec::new(), LAYER));
        }
        if import.steps.iter().any(|s| matches!(s, TdsStep::Material(_))) {
            blocks.push(("RM_SDB", material_block(), LAYER));
        }
        let spot = |s: &TdsStep| matches!(s, TdsStep::Light { spot: Some(_), .. });
        if import.steps.iter().any(|s| matches!(s, TdsStep::Light { spot: None, .. })) {
            blocks.push(("overhead", light_block(), "0"));
        }
        if import.steps.iter().any(spot) {
            blocks.push(("sh_spot", crate::io::tds_legacy::spot_block(), "0"));
        }
        for (name, entities, layer) in blocks {
            if scene.document.block_records.get(name).is_some() {
                continue;
            }
            scene.define_block_raw(name, codec::types::Vector3::ZERO, entities);
            let begin = scene.document.block_records.get(name).map(|b| b.block_entity_handle);
            if let Some(common) = begin.and_then(|h| scene.document.get_entity_mut(h)).map(|e| e.common_mut()) {
                common.layer = layer.into();
            }
        }
    }
}
