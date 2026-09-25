//! Attaching drawing references: ATTACH / XATTACH (file picker, then the
//! Attach External Reference dialog), the -XREF command line, the `?` list,
//! and the commit that creates the definition and its INSERT as one undo step.

use super::*;
use crate::io::xref_model::Pathtype;
use crate::modules::insert::xattach::{
    existing_reference, path_to_block_name, prepare_xref_definition, stored_path,
    XAttachCommand, XrefAttachRequest, XrefPlacement,
};
use crate::modules::insert::xref_cmd::display_path;
use crate::ui::window::xref_attach::{XrefAttachMsg, XrefAttachState};

impl OpenCADStudio {
    /// Commands owned here; `None` defers to the other families.
    pub(super) fn dispatch_xref_attach(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let upper = cmd.to_ascii_uppercase();
        let (verb, rest) = match cmd.split_once(char::is_whitespace) {
            Some((verb, rest)) => (verb.to_ascii_uppercase(), rest.trim()),
            None => (upper.clone(), ""),
        };
        match (verb.as_str(), rest.is_empty()) {
            ("ATTACH", true) => Some(Task::done(Message::AttachPick)),
            ("XATTACH", true) => Some(Task::done(Message::XAttachPick)),
            // A file named on the command line skips the picker.
            // Handled at once, so the placement it starts belongs to this
            // command line (automation included).
            ("ATTACH", false) => Some(self.update(Message::AttachPickResult(Ok(
                std::path::PathBuf::from(rest.trim_matches('"')),
            )))),
            ("XATTACH", false) => {
                self.open_xref_attach_dialog(std::path::PathBuf::from(rest.trim_matches('"')));
                Some(Task::none())
            }
            // XREF opens the External References palette; -XREF asks on the
            // command line. Both keep their argument forms for scripts.
            ("XREF", true) | ("XREFSHOW", true) => {
                if !self.show_external_references {
                    return self.dispatch_blocks("EXTERNALREFERENCES", i);
                }
                self.refresh_xref_manager();
                Some(self.finish_dispatch(cmd))
            }
            ("-XREF", true) => {
                let saved: Vec<(String, String)> = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .iter()
                    .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                    .map(|br| (br.name.clone(), br.xref_path.clone()))
                    .collect();
                let command = crate::modules::insert::xref_cmd::XrefCommand::new(saved);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            ("XREFLIST", _) => {
                self.list_xrefs(i, if rest.is_empty() { "*" } else { rest });
                Some(self.finish_dispatch(cmd))
            }
            ("XREFATTACHFILE", false) | ("XREFOVERLAYFILE", false) => {
                let request = XrefAttachRequest {
                    path: rest.trim_matches('"').to_string(),
                    overlay: verb == "XREFOVERLAYFILE",
                    // The reference default (REFPATHTYPE 1): relative once the
                    // drawing is saved, the full path before that.
                    path_type: Pathtype::Relative,
                };
                self.start_xref_attach(i, request, XrefPlacement::on_screen(), "-XREF");
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// `-XREF ?` — the reference table as the reference application prints it.
    fn list_xrefs(&mut self, i: usize, pattern: &str) {
        let rows: Vec<(String, &'static str, String)> = self.tabs[i]
            .scene
            .document
            .block_records
            .iter()
            .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
            .filter(|br| {
                pattern == "*" || crate::io::xref_model::wildcard_match(&br.name, pattern)
            })
            .map(|br| {
                let kind = if br.flags.is_xref_overlay { "Overlay" } else { "Attach" };
                (br.name.clone(), kind, display_path(&br.xref_path))
            })
            .collect();
        let out = &mut self.command_line;
        out.push_output("");
        out.push_output(&format!(
            "{:<34}{:<13}{}",
            "Xref name",
            "Xref Type",
            "Path"
        ));
        out.push_output(&format!("{:<34}{:<13}{}", "----------------------", "---------", "----------"));
        out.push_output("");
        for (name, kind, path) in &rows {
            out.push_output(&format!("{:<34}{:<13}{}", format!("\"{name}\""), *kind, path));
        }
        out.push_output("");
        out.push_output(&format!("Total Xref(s): {}", rows.len()));
    }

    /// Checks the file, reports it the way the reference does, then asks
    /// for whatever is left on-screen (or commits straight away).
    pub(in crate::app) fn start_xref_attach(
        &mut self,
        i: usize,
        request: XrefAttachRequest,
        placement: XrefPlacement,
        label: &'static str,
    ) {
        let name = path_to_block_name(&request.path);
        let full = crate::modules::insert::xattach::resolve_xref_store_path(&request.path, None);
        let host = self.tabs[i].current_path.clone();
        if let Some(host) = host.as_deref() {
            if crate::modules::insert::xattach::is_self_attach(host, &full, host.parent()) {
                self.command_line.push_error(
                    "Error: Possible circular reference to current drawing. *Invalid*",
                );
                return;
            }
        }
        let existing = existing_reference(&self.tabs[i].scene, &name);
        if let Some(existing) = existing.as_ref() {
            self.command_line
                .push_output(&format!("Xref \"{}\" has already been defined.", existing));
            self.command_line
                .push_output("Using existing definition.");
        } else {
            let verb = if request.overlay {
                format!("Overlay Xref \"{}\": {}", name, display_path(&full))
            } else {
                format!("Attach Xref \"{}\": {}", name, display_path(&full))
            };
            self.command_line.push_output(&verb);
            if !std::path::Path::new(&full).is_file() {
                let file = std::path::Path::new(&full.replace('\\', "/"))
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or(full.clone());
                self.command_line
                    .push_output(&format!("\"{}\" cannot be found.", file));
                self.command_line.push_error("*Invalid*");
                return;
            }
            self.command_line
                .push_output(&format!("\"{}\" loaded.", name));
        }
        let command = XAttachCommand::new(request.clone(), placement, label);
        if let Some(entity) = command.immediate() {
            self.commit_xref_attach(i, request, entity);
            return;
        }
        let (wires, source_units) = self.xref_preview(i, existing.as_deref(), &full);
        let host_units = self.tabs[i].scene.document.header.insertion_units;
        let unit = crate::app::properties::insert_unit_scale(host_units, source_units).unwrap_or(1.0);
        let command = command.with_preview(wires, unit);
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
    }

    /// The reference's geometry for the placement ghost, in its own units:
    /// an existing definition's content, or the file's model space.
    fn xref_preview(
        &self,
        i: usize,
        existing: Option<&str>,
        path: &str,
    ) -> (Vec<crate::scene::model::wire_model::WireModel>, i16) {
        // ponytail: a huge reference skips the ghost; cap by entity count.
        #[cfg(not(target_arch = "wasm32"))]
        const MAX_PREVIEW_ENTITIES: usize = 20_000;
        let scene = &self.tabs[i].scene;
        if let Some(name) = existing {
            let units = scene
                .document
                .block_records
                .get(name)
                .map(|br| br.units)
                .unwrap_or(0);
            return (scene.block_preview_wires(name), units);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(source) = crate::io::load_file(std::path::Path::new(path)) {
            let owner = source.header.model_space_block_handle;
            let entities: Vec<codec::EntityType> = source
                .entities()
                .filter(|e| e.common().owner_handle == owner)
                .filter(|e| {
                    !matches!(
                        e,
                        codec::EntityType::Block(_) | codec::EntityType::BlockEnd(_)
                    )
                })
                .take(MAX_PREVIEW_ENTITIES + 1)
                .cloned()
                .collect();
            if entities.len() <= MAX_PREVIEW_ENTITIES {
                return (
                    scene.wires_for_entities(&entities),
                    source.header.insertion_units,
                );
            }
        }
        let _ = path;
        (Vec::new(), 0)
    }

    /// Creates (or reuses) the definition and commits its INSERT — one undo
    /// step for both, so undo leaves no definition behind.
    pub(in crate::app) fn commit_xref_attach(
        &mut self,
        i: usize,
        request: XrefAttachRequest,
        mut entity: codec::EntityType,
    ) {
        let pending = self.begin_undo(i, "XATTACH", 1, false);
        let host = self.tabs[i].current_path.clone();
        let name = prepare_xref_definition(&mut self.tabs[i].scene, &request, host.as_deref());
        if host.is_none() && request.path_type == Pathtype::Relative {
            self.tabs[i].xref_relative_on_save.insert(name.clone());
        }
        if let codec::EntityType::Insert(insert) = &mut entity {
            insert.block_name = name;
        }
        // Resolving merged the reference's layers and linetypes — mirror
        // them into the Layers panel and ribbon dropdowns now (#407).
        self.refresh_layer_panel();
        let _ = self.commit_entity_handle(entity);
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        self.refresh_xref_manager();
    }

    /// Opens the Attach External Reference dialog for `path`.
    pub(in crate::app) fn open_xref_attach_dialog(&mut self, path: std::path::PathBuf) {
        let i = self.active_tab;
        let existing: Vec<(String, String)> = self.tabs[i]
            .scene
            .document
            .block_records
            .iter()
            .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
            .map(|br| (br.name.clone(), br.xref_path.clone()))
            .collect();
        let mut state = XrefAttachState::new(existing);
        self.load_xref_attach_file(&mut state, &path.to_string_lossy());
        self.xref_attach = Some(state);
        self.active_modal = Some(crate::app::ModalKind::XrefAttach);
    }

    /// Fills the dialog from the chosen file: name, preview, block unit and
    /// the paths shown under Details.
    fn load_xref_attach_file(&self, state: &mut XrefAttachState, path: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        let i = self.active_tab;
        state.path = path.to_string();
        state.name = path_to_block_name(path);
        state.found_in = display_path(path);
        #[cfg(not(target_arch = "wasm32"))]
        {
            state.preview = crate::io::thumbnail::read_handle(std::path::Path::new(path));
            let host_units = self.tabs[i].scene.document.header.insertion_units;
            let source_units = crate::io::load_file(std::path::Path::new(path))
                .map(|doc| doc.header.insertion_units)
                .unwrap_or(0);
            state.unit_label = crate::t!(crate::app::properties::insunits_name(source_units))
                .into_owned();
            let factor =
                crate::app::properties::insert_unit_scale(host_units, source_units).unwrap_or(1.0);
            state.unit_factor = crate::app::properties::format_unit_factor(factor);
        }
        self.refresh_xref_attach_saved_path(state);
    }

    fn refresh_xref_attach_saved_path(&self, state: &mut XrefAttachState) {
        let host = self.tabs[self.active_tab].current_path.clone();
        let request = XrefAttachRequest {
            path: state.path.clone(),
            overlay: state.overlay,
            path_type: state.path_type.0,
        };
        state.saved_path = display_path(&stored_path(&request, host.as_deref()));
    }

    /// Messages of the ATTACH picker and the dialog.
    pub(in crate::app) fn update_xref_attach(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AttachPick => Task::perform(
                async {
                    let handle = crate::sys::file_dialog()
                        .set_title(crate::t!("Select Reference File").as_ref())
                        .add_filter(
                            crate::t!("All Reference Files").as_ref(),
                            &[
                                "dwg", "dxf", "pdf", "png", "jpg", "jpeg", "bmp", "tif", "tiff",
                            ],
                        )
                        .add_filter(crate::t!("Drawing").as_ref(), &["dwg", "dxf"])
                        .add_filter(crate::t!("PDF Files").as_ref(), &["pdf"])
                        .add_filter(
                            crate::t!("Images").as_ref(),
                            &["png", "jpg", "jpeg", "bmp", "tif", "tiff"],
                        )
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(crate::sys::handle_path(&h)),
                        None => Err("Cancelled".to_string()),
                    }
                },
                Message::AttachPickResult,
            ),
            Message::AttachPickResult(Ok(path)) => {
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                match ext.as_str() {
                    "pdf" => match std::fs::read(&path) {
                        Ok(bytes) => self.update(Message::PdfAttachPickResult(Ok((
                            path,
                            std::sync::Arc::new(bytes),
                        )))),
                        Err(e) => self.update(Message::PdfAttachPickResult(Err(e.to_string()))),
                    },
                    "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" => {
                        let result = image::image_dimensions(&path)
                            .map(|(w, h)| (path.clone(), w, h))
                            .map_err(|e| e.to_string());
                        self.update(Message::ImagePickResult(result))
                    }
                    _ => {
                        self.open_xref_attach_dialog(path);
                        Task::none()
                    }
                }
            }
            Message::AttachPickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line
                        .push_error(format!("ATTACH: {e}").as_ref());
                }
                Task::none()
            }
            Message::XrefAttachBrowseResult(Ok(path)) => {
                if let Some(mut state) = self.xref_attach.take() {
                    self.load_xref_attach_file(&mut state, &path.to_string_lossy());
                    self.xref_attach = Some(state);
                }
                Task::none()
            }
            Message::XrefAttachBrowseResult(Err(_)) => Task::none(),
            Message::XrefAttach(edit) => self.edit_xref_attach(edit),
            _ => Task::none(),
        }
    }

    fn edit_xref_attach(&mut self, edit: XrefAttachMsg) -> Task<Message> {
        let Some(mut state) = self.xref_attach.take() else {
            return Task::none();
        };
        let mut task = Task::none();
        match edit {
            XrefAttachMsg::Name(name) => {
                if let Some((_, saved)) = state
                    .existing
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(&name))
                    .cloned()
                {
                    let host = self.tabs[self.active_tab].current_path.clone();
                    let base = host.as_deref().and_then(|h| h.parent());
                    let full = crate::modules::insert::xattach::resolve_xref_store_path(&saved, base);
                    self.load_xref_attach_file(&mut state, &full);
                }
            }
            XrefAttachMsg::Browse => {
                task = Task::perform(
                    async {
                        let handle = crate::sys::file_dialog()
                            .set_title(crate::t!("Select Reference File").as_ref())
                            .add_filter(crate::t!("Drawing").as_ref(), &["dwg", "dxf"])
                            .pick_file()
                            .await;
                        match handle {
                            Some(h) => Ok(crate::sys::handle_path(&h)),
                            None => Err("Cancelled".to_string()),
                        }
                    },
                    Message::XrefAttachBrowseResult,
                );
            }
            XrefAttachMsg::Overlay(overlay) => state.overlay = overlay,
            XrefAttachMsg::ScaleOnScreen(on) => state.scale_on_screen = on,
            XrefAttachMsg::Scale(axis, value) => {
                if axis == 0 && state.uniform {
                    state.scale = [value.clone(), value.clone(), value];
                } else if let Some(slot) = state.scale.get_mut(axis) {
                    *slot = value;
                }
            }
            XrefAttachMsg::Uniform(on) => {
                state.uniform = on;
                if on {
                    let x = state.scale[0].clone();
                    state.scale = [x.clone(), x.clone(), x];
                }
            }
            XrefAttachMsg::InsertOnScreen(on) => state.insert_on_screen = on,
            XrefAttachMsg::Insert(axis, value) => {
                if let Some(slot) = state.insert.get_mut(axis) {
                    *slot = value;
                }
            }
            XrefAttachMsg::PathType(choice) => {
                state.path_type = choice;
                self.refresh_xref_attach_saved_path(&mut state);
            }
            XrefAttachMsg::RotationOnScreen(on) => state.rotation_on_screen = on,
            XrefAttachMsg::Rotation(value) => state.rotation = value,
            XrefAttachMsg::Details(on) => state.details = on,
            XrefAttachMsg::Help => {
                self.command_line.push_info(
                    crate::t!("Inserts references to external files such as other drawings, raster images, and underlays.")
                        .as_ref(),
                );
            }
            XrefAttachMsg::Apply => {
                let placement = XrefPlacement {
                    insert: (!state.insert_on_screen)
                        .then(|| state.insert_point())
                        .flatten(),
                    scale: (!state.scale_on_screen).then(|| state.scale_values()).flatten(),
                    rotation: (!state.rotation_on_screen)
                        .then(|| crate::entities::common::parse_typed_angle(&state.rotation))
                        .flatten()
                        .map(f64::to_degrees),
                };
                let invalid = (!state.insert_on_screen && placement.insert.is_none())
                    || (!state.scale_on_screen
                        && placement
                            .scale
                            .is_none_or(|s| s.iter().any(|v| *v == 0.0)))
                    || (!state.rotation_on_screen && placement.rotation.is_none());
                if invalid || state.path.is_empty() {
                    self.command_line
                        .push_error(crate::t!("Invalid input.").as_ref());
                    self.xref_attach = Some(state);
                    return Task::none();
                }
                let request = XrefAttachRequest {
                    path: state.path.clone(),
                    overlay: state.overlay,
                    path_type: state.path_type.0,
                };
                self.active_modal = None;
                let i = self.active_tab;
                self.start_xref_attach(i, request, placement, "XATTACH");
                return Task::none();
            }
        }
        self.xref_attach = Some(state);
        task
    }
}
