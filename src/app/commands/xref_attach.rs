//! Attaching drawing references: ATTACH / XATTACH (file picker, then the
//! Attach External Reference dialog), the -XREF command line, the `?` list,
//! and the commit that creates the definition and its INSERT as one undo step.

use super::*;
use crate::io::xref_model::Pathtype;
use crate::modules::insert::xattach::IMAGE_EXTENSIONS;
use crate::modules::insert::xattach::{
    existing_reference, path_to_block_name, stored_path, XAttachCommand, XrefAttachRequest,
    XrefPlacement,
};
use crate::modules::insert::xref_cmd::display_path;
use crate::ui::window::xref_attach::{XrefAttachMsg, XrefAttachState};

/// ATTACH's file types, in the reference order (the picker counts from 1).
const ATTACH_FILTERS: [(&str, &[&str]); 8] = [
    ("All image files", IMAGE_EXTENSIONS),
    ("DWF files (*.dwfx; *.dwf)", &["dwfx", "dwf"]),
    ("MicroStation DGN (*.dgn)", &["dgn"]),
    ("All DGN Files (*.*)", &["*"]),
    ("PDF files (*.pdf)", &["pdf"]),
    ("Point Cloud (*.rcp; *.rcs)", &["rcp", "rcs"]),
    ("Drawing (*.dwg)", &["dwg"]),
    ("All files (*.*)", &["*"]),
];
const ALL_DGN_FILTER: u32 = 4;

impl OpenCADStudio {
    /// DWFATTACH / DGNATTACH on the command line: with a file, from its
    /// sheet / model prompt; without one, from the path prompt.
    pub(super) fn start_underlay_attach(
        &mut self,
        i: usize,
        kind: document::entities::UnderlayType,
        file: &str,
    ) {
        use crate::command::CadCommand;
        let insunits = self.tabs[i].scene.document.header.insertion_units;
        let mut command =
            crate::modules::insert::pdf_attach::PdfAttachCommand::for_kind(kind, insunits);
        if !file.trim().is_empty() {
            if let Some(crate::command::CmdResult::ReportError(message)) =
                command.on_text_input(file)
            {
                self.command_line.push_rejection(&message);
            }
        }
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
    }

    /// Commands owned here; `None` defers to the other families.
    pub(super) fn dispatch_xref_attach(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let upper = cmd.to_ascii_uppercase();
        let (verb, rest) = match cmd.split_once(char::is_whitespace) {
            Some((verb, rest)) => (verb.to_ascii_uppercase(), rest.trim()),
            None => (upper.clone(), ""),
        };
        // FILEDIA 0 sends the picking commands to their command-line forms.
        let dialog = crate::sys::file_dia();
        match (verb.as_str(), rest.is_empty()) {
            ("ATTACH", true) if dialog => Some(Task::done(Message::AttachPick)),
            ("ATTACH", true) | ("-ATTACH", _) => {
                use crate::command::CadCommand;
                let command = crate::modules::insert::xref_cmd::AttachCommand::new();
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                if !rest.is_empty() {
                    return Some(
                        self.feed_command(crate::command::StepInput::Text(rest.to_string())),
                    );
                }
                Some(Task::none())
            }
            ("DWFATTACH", true) if dialog => Some(Task::done(Message::UnderlayAttachPick(
                document::entities::UnderlayType::Dwf,
            ))),
            ("DGNATTACH", true) if dialog => Some(Task::done(Message::UnderlayAttachPick(
                document::entities::UnderlayType::Dgn,
            ))),
            // A file after DWFATTACH / DGNATTACH, or the - forms, go on the
            // command line.
            ("DWFATTACH" | "-DWFATTACH", _) => {
                self.start_underlay_attach(i, document::entities::UnderlayType::Dwf, rest);
                Some(Task::none())
            }
            ("DGNATTACH" | "-DGNATTACH", _) => {
                self.start_underlay_attach(i, document::entities::UnderlayType::Dgn, rest);
                Some(Task::none())
            }
            // POINTCLOUDATTACH picks the file; with a file, or as
            // -POINTCLOUDATTACH, it asks on the command line.
            ("POINTCLOUDATTACH", true) if dialog => Some(Task::done(Message::PointCloudAttachPick)),
            ("POINTCLOUDATTACH" | "-POINTCLOUDATTACH", _) => {
                use crate::command::CadCommand;
                let command = crate::modules::insert::pc_attach::PointCloudAttachCommand::new();
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                if !rest.is_empty() {
                    return Some(
                        self.feed_command(crate::command::StepInput::Text(rest.to_string())),
                    );
                }
                Some(Task::none())
            }
            ("XATTACH", true) if dialog => Some(Task::done(Message::XAttachPick)),
            ("XATTACH", true) => {
                let command =
                    crate::modules::insert::xref_cmd::XrefCommand::attach(self.saved_xref_paths(i));
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
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
                let command =
                    crate::modules::insert::xref_cmd::XrefCommand::new(self.saved_xref_paths(i));
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
                Some(self.start_xref_attach(i, request, XrefPlacement::on_screen(), "-XREF"))
            }
            _ => None,
        }
    }

    /// Saved paths by reference name, for -XREF.
    fn saved_xref_paths(&self, i: usize) -> Vec<(String, String)> {
        self.tabs[i]
            .scene
            .document
            .block_records
            .iter()
            .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
            .map(|br| (br.name.clone(), br.xref_path.clone()))
            .collect()
    }

    /// `-XREF ?` — the reference table as the reference application prints it,
    /// with the live status column (Loaded / Unloaded / Not found / …) the
    /// palette shows.
    fn list_xrefs(&mut self, i: usize, pattern: &str) {
        let entries = self.refresh_xref_report(i);
        let rows: Vec<(String, String, String, String)> = entries
            .iter()
            .filter(|e| {
                e.parent_key.is_none() && e.kind == crate::io::xref_model::RefKind::DwgXref
            })
            .filter(|e| pattern == "*" || crate::io::xref_model::wildcard_match(&e.name, pattern))
            .map(|e| {
                let kind = match e.ref_type {
                    crate::io::xref_model::RefType::Overlay => "Overlay",
                    crate::io::xref_model::RefType::Attach => "Attach",
                };
                (
                    e.name.clone(),
                    kind.to_string(),
                    crate::ui::window::xref_manager::status_text(e.status).into_owned(),
                    display_path(&e.saved_path),
                )
            })
            .collect();
        let out = &mut self.command_line;
        out.push_output("");
        out.push_output(&format!(
            "{:<34}{:<13}{:<12}{}",
            "Xref name", "Xref Type", "Status", "Path"
        ));
        out.push_output(&format!(
            "{:<34}{:<13}{:<12}{}",
            "----------------------", "---------", "--------", "----------"
        ));
        out.push_output("");
        for (name, kind, status, path) in &rows {
            out.push_output(&format!(
                "{:<34}{:<13}{:<12}{}",
                format!("\"{name}\""),
                kind,
                status,
                path
            ));
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
    ) -> Task<Message> {
        self.start_xref_attach_with(i, request, placement, label, read_preview)
    }

    /// [`Self::start_xref_attach`] with the ghost's reader given, so a test
    /// can give one that panics.
    pub(in crate::app) fn start_xref_attach_with(
        &mut self,
        i: usize,
        request: XrefAttachRequest,
        placement: XrefPlacement,
        label: &'static str,
        reader: PreviewReader,
    ) -> Task<Message> {
        let name = path_to_block_name(&request.path);
        let full = crate::modules::insert::xattach::resolve_xref_store_path(&request.path, None);
        let host = self.tabs[i].current_path.clone();
        if let Some(host) = host.as_deref() {
            if crate::modules::insert::xattach::is_self_attach(host, &full, host.parent()) {
                self.command_line.push_rejection(
                    "Error: Possible circular reference to current drawing. *Invalid*",
                );
                return Task::none();
            }
        }
        let existing = existing_reference(&self.tabs[i].scene, &name);
        if let Some(existing) = existing.as_ref() {
            self.command_line
                .push_output(&format!("Xref \"{}\" has already been defined.", existing));
            self.command_line.push_output("Using existing definition.");
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
                self.command_line.push_rejection("*Invalid*");
                return Task::none();
            }
            self.command_line
                .push_output(&format!("\"{}\" loaded.", name));
        }
        let command = XAttachCommand::new(request.clone(), placement, label);
        if let Some(entity) = command.immediate() {
            return self.commit_xref_attach(i, request, entity);
        }
        // A reference already in the drawing gives its ghost at once; a
        // file's ghost is read off the UI thread and comes when the read
        // ends ([`Self::finish_xref_preview`]).
        let (command, task) = match existing.as_deref() {
            Some(name) => {
                let scene = &self.tabs[i].scene;
                let units = scene
                    .document
                    .block_records
                    .get(name)
                    .map(|br| br.units)
                    .unwrap_or(0);
                let wires = scene.block_preview_wires(name);
                (
                    command.with_preview(wires, self.xref_unit(i, units)),
                    Task::none(),
                )
            }
            None => (command, self.read_xref_preview_with(i, full, reader)),
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        task
    }

    /// Drawing units per reference unit, for a reference in `units`.
    fn xref_unit(&self, i: usize, units: i16) -> f64 {
        let host_units = self.tabs[i].scene.document.header.insertion_units;
        crate::app::properties::insert_unit_scale(host_units, units).unwrap_or(1.0)
    }

    /// Reads the placement ghost of the file at `path` off the UI thread.
    fn read_xref_preview_with(
        &mut self,
        i: usize,
        path: String,
        reader: PreviewReader,
    ) -> Task<Message> {
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        let worker_path = path.clone();
        crate::app::update::background_task(
            "XATTACH",
            move || reader(&worker_path),
            move |read| {
                Message::XrefAttach(XrefAttachMsg::PreviewRead(
                    tab_id,
                    generation,
                    Box::new((path, read)),
                ))
            },
        )
    }

    /// The end of the ghost's read: the attach that asked for it gets its
    /// ghost, if it still waits in that drawing for the same file. A result
    /// for a drawing that was closed, replaced or cleared is dropped.
    fn finish_xref_preview(
        &mut self,
        tab_id: u64,
        generation: u64,
        path: String,
        read: crate::ui::window::xref_attach::XrefPreviewRead,
    ) -> Task<Message> {
        let Some((entities, units)) = read else {
            return Task::none();
        };
        let Some(i) = self.import_target(tab_id, generation) else {
            return Task::none();
        };
        let waits = self.tabs[i]
            .active_cmd
            .as_ref()
            .and_then(|command| command.xattach_request())
            .is_some_and(|request| {
                crate::modules::insert::xattach::resolve_xref_store_path(&request.path, None)
                    == path
            });
        if !waits {
            return Task::none();
        }
        let wires = self.tabs[i].scene.wires_for_entities(&entities);
        let unit = self.xref_unit(i, units);
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.on_xref_preview(wires, unit);
        }
        Task::none()
    }

    /// The reference's attach: a reference already in the drawing is used at
    /// once; else its file is read off the UI thread, and the definition and
    /// its INSERT are committed when the read ends
    /// ([`Self::finish_xref_attach`]). The command ends now.
    pub(in crate::app) fn commit_xref_attach(
        &mut self,
        i: usize,
        request: XrefAttachRequest,
        entity: document::EntityType,
    ) -> Task<Message> {
        self.commit_xref_attach_with(i, request, entity, read_reference)
    }

    /// [`Self::commit_xref_attach`] with the reader given, so a test can
    /// give one that panics.
    pub(in crate::app) fn commit_xref_attach_with(
        &mut self,
        i: usize,
        request: XrefAttachRequest,
        entity: document::EntityType,
        reader: fn(&str, &std::path::Path) -> crate::io::xref::XrefRead,
    ) -> Task<Message> {
        let host = self.tabs[i].current_path.clone();
        let stem = path_to_block_name(&request.path);
        let (raw_path, base_dir) =
            crate::modules::insert::xattach::xref_read_inputs(&request, host.as_deref());
        let base_dir = match base_dir {
            Some(base_dir) if existing_reference(&self.tabs[i].scene, &stem).is_none() => base_dir,
            _ => {
                self.place_xref_attach(i, request, entity, None);
                return Task::none();
            }
        };
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.command_line.push_info(&format!(
            "Reading {}…",
            crate::entities::underlay::display_path(&raw_path)
        ));
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        crate::app::update::background_task(
            "XATTACH",
            move || reader(&raw_path, &base_dir),
            move |read| {
                Message::XrefAttach(XrefAttachMsg::Read(
                    tab_id,
                    generation,
                    Box::new((request, entity, read)),
                ))
            },
        )
    }

    /// The end of the reference's read: the attach is committed in the
    /// drawing it was started for. A result for a drawing that was closed,
    /// replaced or cleared is dropped, and so is one for a drawing that is
    /// not the current one now (`commit_entity_handle` writes to the
    /// current drawing).
    fn finish_xref_attach(
        &mut self,
        tab_id: u64,
        generation: u64,
        request: XrefAttachRequest,
        entity: document::EntityType,
        read: crate::io::xref::XrefRead,
    ) -> Task<Message> {
        let Some(i) = self.import_target(tab_id, generation) else {
            return Task::none();
        };
        if i != self.active_tab {
            self.command_line.push_error(
                "Attach cancelled: a different drawing became current while the reference was read.",
            );
            return Task::none();
        }
        self.place_xref_attach(i, request, entity, Some(read));
        Task::none()
    }

    /// Creates (or reuses) the definition and commits its INSERT — one undo
    /// step for both, so undo leaves no definition behind. With `read`, the
    /// reference's content comes from it, and no file is read here.
    fn place_xref_attach(
        &mut self,
        i: usize,
        request: XrefAttachRequest,
        mut entity: document::EntityType,
        read: Option<crate::io::xref::XrefRead>,
    ) {
        let pending = self.begin_undo(i, "XATTACH", 1, false);
        let host = self.tabs[i].current_path.clone();
        let stem = path_to_block_name(&request.path);
        let name = crate::modules::insert::xattach::prepare_named_xref_definition_with(
            &mut self.tabs[i].scene,
            &request,
            host.as_deref(),
            &stem,
            read,
        );
        if host.is_none() && request.path_type == Pathtype::Relative {
            self.tabs[i].xref_relative_on_save.insert(name.clone());
        }
        if let document::EntityType::Insert(insert) = &mut entity {
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
        // The new reference must appear in the automation report too, not
        // only in the palette: a status query right after an attach would
        // otherwise answer with the pre-attach snapshot.
        self.refresh_xref_report(i);
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
            state.unit_label =
                crate::t!(crate::app::properties::insunits_name(source_units)).into_owned();
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
            Message::UnderlayAttachPick(kind) => Task::perform(
                async move {
                    let (title, filter, extensions): (&str, &str, &[&str]) = match kind {
                        document::entities::UnderlayType::Dgn => {
                            ("Select DGN File", "DGN Files", &["dgn"])
                        }
                        _ => ("Select DWF File", "DWF Files", &["dwf", "dwfx"]),
                    };
                    let handle = crate::sys::file_dialog()
                        .set_title(crate::t!(title).as_ref())
                        .add_filter(crate::t!(filter).as_ref(), extensions)
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(crate::sys::handle_path(&h)),
                        None => Err("Cancelled".to_string()),
                    }
                },
                Message::AttachPickResult,
            ),
            Message::PointCloudAttachPick => Task::perform(
                async {
                    let handle = crate::sys::file_dialog()
                        .set_title(crate::t!("Select Point Cloud File").as_ref())
                        .add_filter(crate::t!("Point Cloud Project").as_ref(), &["rcp"])
                        .add_filter(crate::t!("Point Cloud Scan").as_ref(), &["rcs"])
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(crate::sys::handle_path(&h)),
                        None => Err("Cancelled".to_string()),
                    }
                },
                Message::AttachPickResult,
            ),
            Message::AttachPick => {
                let filters = ATTACH_FILTERS
                    .iter()
                    .map(|(label, extensions)| (crate::t!(label).to_string(), *extensions))
                    .collect();
                let title = crate::t!("Select Reference File").to_string();
                let index = crate::sys::attach_filter();
                Task::perform(
                    async move {
                        match crate::sys::pick_file_with_filter(title, filters, index).await {
                            Some((path, index)) => {
                                crate::sys::set_attach_filter(index);
                                Ok(path)
                            }
                            None => Err("Cancelled".to_string()),
                        }
                    },
                    Message::AttachPickResult,
                )
            }
            Message::AttachPickResult(Ok(path)) => {
                // The file type the picker was left on is a profile setting.
                self.save_config();
                let mut ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                // Any file picked under "All DGN Files" is a DGN.
                let known = ["dwg", "dxf", "pdf", "dwf", "dwfx", "dgn", "rcp", "rcs"];
                if crate::sys::attach_filter() == ALL_DGN_FILTER
                    && !known.contains(&ext.as_str())
                    && !IMAGE_EXTENSIONS.contains(&ext.as_str())
                {
                    ext = "dgn".to_string();
                }
                match ext.as_str() {
                    // An underlay file is read off the UI thread, then goes
                    // on to the Attach window.
                    "pdf" | "dwf" | "dwfx" | "dgn" => {
                        let kind = match ext.as_str() {
                            "pdf" => document::entities::UnderlayType::Pdf,
                            "dgn" => document::entities::UnderlayType::Dgn,
                            _ => document::entities::UnderlayType::Dwf,
                        };
                        self.read_underlay_for_attach(kind, &path.to_string_lossy(), true)
                    }
                    // A point cloud is read off the UI thread, then goes on
                    // to the Attach Point Cloud dialog.
                    "rcp" | "rcs" => self.read_point_cloud_for_attach(path),
                    e if IMAGE_EXTENSIONS.contains(&e) => {
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
        if let XrefAttachMsg::Read(tab_id, generation, read) = edit {
            let (request, entity, read) = *read;
            return self.finish_xref_attach(tab_id, generation, request, entity, read);
        }
        if let XrefAttachMsg::PreviewRead(tab_id, generation, read) = edit {
            let (path, read) = *read;
            return self.finish_xref_preview(tab_id, generation, path, read);
        }
        let Some(mut state) = self.xref_attach.take() else {
            return Task::none();
        };
        let mut task = Task::none();
        match edit {
            // Taken before the window's state, above.
            XrefAttachMsg::Read(..) | XrefAttachMsg::PreviewRead(..) => {}
            XrefAttachMsg::Name(name) => {
                if let Some((_, saved)) = state
                    .existing
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(&name))
                    .cloned()
                {
                    let host = self.tabs[self.active_tab].current_path.clone();
                    let base = host.as_deref().and_then(|h| h.parent());
                    let full =
                        crate::modules::insert::xattach::resolve_xref_store_path(&saved, base);
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
                    scale: (!state.scale_on_screen)
                        .then(|| state.scale_values())
                        .flatten(),
                    rotation: (!state.rotation_on_screen)
                        .then(|| crate::entities::common::parse_typed_angle(&state.rotation))
                        .flatten()
                        .map(f64::to_degrees),
                };
                let invalid = (!state.insert_on_screen && placement.insert.is_none())
                    || (!state.scale_on_screen && placement.scale.is_none_or(|s| s.contains(&0.0)))
                    || (!state.rotation_on_screen && placement.rotation.is_none());
                if invalid || state.path.is_empty() {
                    self.command_line
                        .push_rejection(crate::t!("Invalid input.").as_ref());
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
                return self.start_xref_attach(i, request, placement, "XATTACH");
            }
        }
        self.xref_attach = Some(state);
        task
    }
}

/// What the placement ghost's read gives: a file's model-space objects
/// and units.
type PreviewReader = fn(&str) -> crate::ui::window::xref_attach::XrefPreviewRead;

/// The model-space objects and units of the drawing at `path`, for the
/// placement ghost: none when it cannot be read or holds too many objects
/// for a ghost. It reads the file, so it runs on a worker.
fn read_preview(path: &str) -> crate::ui::window::xref_attach::XrefPreviewRead {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // ponytail: a huge reference skips the ghost; cap by entity count.
        const MAX_PREVIEW_ENTITIES: usize = 20_000;
        let source = crate::io::load_file(std::path::Path::new(path)).ok()?;
        let owner = source.header.model_space_block_handle;
        let entities: Vec<document::EntityType> = source
            .entities()
            .filter(|e| e.common().owner_handle == owner)
            .filter(|e| {
                !matches!(
                    e,
                    document::EntityType::Block(_) | document::EntityType::BlockEnd(_)
                )
            })
            .take(MAX_PREVIEW_ENTITIES + 1)
            .cloned()
            .collect();
        (entities.len() <= MAX_PREVIEW_ENTITIES)
            .then_some((entities, source.header.insertion_units))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

/// The read of a reference's file for its attach, on a worker.
fn read_reference(raw_path: &str, base_dir: &std::path::Path) -> crate::io::xref::XrefRead {
    crate::io::xref::read_xref_file(raw_path, base_dir, None)
}

#[cfg(test)]
mod xref_attach_read_tests {
    use super::*;
    use glam::DVec3;

    /// An app with a saved source drawing (one line) and a new host drawing
    /// as the current tab; the source's path.
    fn source_and_host(stem: &str) -> (OpenCADStudio, String) {
        let dir =
            std::env::temp_dir().join(format!("ocs-xattach-read-{stem}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let source = dir.join(format!("{stem}.dwg"));
        let path = source.display().to_string().replace('\\', "/");
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{path}"}}"#));
        let _ = app.automation_op(r#"{"op":"new"}"#);
        (app, path)
    }

    /// The request and the INSERT for an attach of `path` at the origin.
    fn attach(path: &str) -> (XrefAttachRequest, document::EntityType) {
        let request = XrefAttachRequest {
            path: path.to_string(),
            overlay: false,
            path_type: Pathtype::Full,
        };
        let placement = XrefPlacement {
            insert: Some(DVec3::ZERO),
            scale: Some([1.0; 3]),
            rotation: Some(0.0),
        };
        let entity = XAttachCommand::new(request.clone(), placement, "XATTACH")
            .immediate()
            .expect("a placed INSERT");
        (request, entity)
    }

    fn references(app: &OpenCADStudio, i: usize) -> usize {
        app.tabs[i]
            .scene
            .document
            .block_records
            .iter()
            .filter(|record| record.flags.is_xref || record.flags.is_xref_overlay)
            .count()
    }

    fn inserts(app: &OpenCADStudio, i: usize) -> usize {
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, document::EntityType::Insert(_)))
            .count()
    }

    /// The attach waits for the read: nothing is attached until the worker
    /// ends, then the reference and its INSERT come in one undo step.
    #[test]
    fn the_attach_waits_for_the_read() {
        let (mut app, path) = source_and_host("waits");
        let i = app.active_tab;
        let undo = app.tabs[i].history.undo_stack.len();
        let (request, entity) = attach(&path);
        let task = app.commit_xref_attach(i, request, entity);
        assert_eq!(
            references(&app, i),
            0,
            "nothing is attached during the read"
        );
        assert!(app
            .command_line
            .history
            .iter()
            .any(|entry| entry.text.contains("Reading ")));
        app.drive_headless_task(task).expect("the attach's task");
        assert_eq!(references(&app, i), 1);
        assert_eq!(inserts(&app, i), 1);
        app.finish_all_pending_history();
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo + 1);
    }

    /// A reader that panics gives an error, and nothing is attached.
    #[test]
    fn a_reader_panic_attaches_nothing() {
        let (mut app, path) = source_and_host("panics");
        let i = app.active_tab;
        let (request, entity) = attach(&path);
        let task = app.commit_xref_attach_with(i, request, entity, |_, _| panic!("a reader bug"));
        app.drive_headless_task(task).expect("the attach's task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("XATTACH failed: a reader bug")
        );
        assert_eq!(references(&app, i), 0);
        assert_eq!(inserts(&app, i), 0);
    }

    /// A result for a drawing that was replaced during the read is dropped.
    #[test]
    fn a_result_for_a_replaced_drawing_is_dropped() {
        let (mut app, path) = source_and_host("replaced");
        let i = app.active_tab;
        let (request, entity) = attach(&path);
        let task = app.commit_xref_attach(i, request, entity);
        app.tabs[i].document_generation += 1;
        app.drive_headless_task(task).expect("the attach's task");
        assert_eq!(references(&app, i), 0);
    }

    /// A result for a drawing that is no longer the current one is dropped
    /// with a message: the commit writes to the current drawing.
    #[test]
    fn a_result_for_a_drawing_that_is_not_current_is_dropped() {
        let (mut app, path) = source_and_host("not-current");
        let i = app.active_tab;
        let (request, entity) = attach(&path);
        let task = app.commit_xref_attach(i, request, entity);
        app.push_test_document();
        app.active_tab = app.tabs.len() - 1;
        app.drive_headless_task(task).expect("the attach's task");
        assert_eq!(references(&app, i), 0);
        assert_eq!(references(&app, app.active_tab), 0);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Attach cancelled: a different drawing became current while the reference was read.")
        );
    }

    /// The attach merges the worker's read and reads no file itself: a
    /// file that is gone when the read ends still gives its content.
    #[test]
    fn the_attach_merges_the_workers_read() {
        let (mut app, path) = source_and_host("merges");
        let i = app.active_tab;
        let (request, entity) = attach(&path);
        let task = app.commit_xref_attach_with(i, request, entity, |raw_path, base_dir| {
            let read = crate::io::xref::read_xref_file(raw_path, base_dir, None);
            let _ = std::fs::remove_file(raw_path);
            read
        });
        app.drive_headless_task(task).expect("the attach's task");
        let record = app.tabs[i]
            .scene
            .document
            .block_records
            .iter()
            .find(|record| record.flags.is_xref)
            .map(|record| record.handle)
            .expect("the reference");
        let content = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == record)
            .count();
        assert_eq!(content, 1, "the source's line, from the worker's read");
    }

    /// A reference that is already in the drawing is attached at once,
    /// without a read.
    #[test]
    fn an_existing_reference_is_attached_at_once() {
        let (mut app, path) = source_and_host("existing");
        let i = app.active_tab;
        let (request, entity) = attach(&path);
        let task = app.commit_xref_attach(i, request, entity);
        app.drive_headless_task(task)
            .expect("the first attach's task");
        let (request, entity) = attach(&path);
        let _ = app.commit_xref_attach_with(i, request, entity, |_, _| panic!("no read"));
        assert_eq!(references(&app, i), 1);
        assert_eq!(inserts(&app, i), 2, "the second INSERT is placed at once");
    }
}
