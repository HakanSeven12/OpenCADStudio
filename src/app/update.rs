use super::helpers::{
    ortho_constrain, parse_coord, polar_constrain, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
    CoordKind,
};
use super::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::grip::{find_hit_grip, find_hit_grip_paper, GripEdit};
use crate::scene::object::GripApply;
use crate::scene::{
    self, hover_id, Scene, TileEdgeOrient, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::window;
use iced::{mouse, Task};

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_DRAW_PX;
/// Pixel distance from a Model-tile inner divider that still registers as
/// a resize grip on the press.
const TILE_EDGE_HIT_PX: f32 = 4.0;
/// Normalized minimum tile size before `collapse_small_model_tiles` merges
/// it into a neighbour. Sized to comfortably contain the ViewCube + its
/// padding so a tile that's still GPU-rendered always has room to show
/// the gizmo.
fn tile_min_norm(canvas_w: f32, canvas_h: f32) -> (f32, f32) {
    let px = VIEWCUBE_DRAW_PX + 2.0 * VIEWCUBE_PAD + 16.0;
    (
        (px / canvas_w.max(1.0)).min(0.95),
        (px / canvas_h.max(1.0)).min(0.95),
    )
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

impl OpenCADStudio {
    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Tick(t) => {
                let i = self.active_tab;
                self.tabs[i].scene.update(t - self.start);

                // If the camera moved since we last synced, write it back to
                // the document and mark the file dirty.
                let gen = self.tabs[i].scene.camera_generation;
                if gen != self.tabs[i].last_synced_camera_gen {
                    self.tabs[i].last_synced_camera_gen = gen;
                    if self.tabs[i].scene.sync_camera_to_document() {
                        self.tabs[i].dirty = true;
                    }
                }

                Task::none()
            }

            Message::OpenFile => {
                Task::perform(crate::io::pick_open_path(), Message::OpenPathPicked)
            }

            Message::OpenPathPicked(None) => Task::none(),

            Message::OpenRecent(path) => {
                // Recents are read from disk every save → the path may be
                // stale. Skip silently if the file no longer exists; the
                // entry stays in the list so the user can clean it up.
                match std::fs::metadata(&path) {
                    Ok(m) => self.update(Message::OpenPathPicked(Some((path, m.len())))),
                    Err(_) => {
                        self.command_line.push_error(&format!(
                            "Recent file no longer exists: {}",
                            path.display()
                        ));
                        Task::none()
                    }
                }
            }

            Message::RecentRemove(path) => {
                self.app_menu.remove_recent(&path);
                Task::none()
            }

            Message::OpenPathPicked(Some((path, size_bytes))) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".into());
                let phase =
                    std::sync::Arc::new(std::sync::atomic::AtomicU8::new(super::OPEN_PHASE_READING));
                self.opening = Some(super::OpenProgress {
                    name: name.clone(),
                    size_bytes,
                    phase: phase.clone(),
                    started: Instant::now(),
                });
                let size_label = format_size(size_bytes);
                self.command_line
                    .push_info(&format!("Opening \"{name}\" ({size_label})…"));
                Task::perform(
                    crate::io::open_path_with_phase(path, phase),
                    Message::FileOpened,
                )
            }

            Message::OpenCancel => {
                if let Some(p) = self.opening.take() {
                    self.command_line
                        .push_info(&format!("Open cancelled: \"{}\"", p.name));
                }
                Task::none()
            }

            Message::FileOpened(Ok((name, path, doc, caches))) => {
                // If the user clicked Cancel while the parser was running, the
                // overlay state was cleared and we silently drop the result.
                if self.opening.is_none() {
                    return Task::none();
                }
                self.opening = None;
                let entity_count = doc.entities().count();
                self.command_line
                    .push_output(&format!("Opened \"{name}\" — {entity_count} entities"));
                if caches.corrupt_dropped > 0 {
                    self.command_line.push_error(&format!(
                        "Warning: {} corrupt entities dropped (parser junk — bad normals / counts)",
                        caches.corrupt_dropped
                    ));
                }
                self.app_menu.push_recent(path.clone());

                let current_is_empty = {
                    let t = &self.tabs[self.active_tab];
                    !t.is_start
                        && t.current_path.is_none()
                        && !t.dirty
                        && self.tabs[self.active_tab].scene.document.entities().count() == 0
                };
                let i = if current_is_empty {
                    self.active_tab
                } else {
                    self.tab_counter += 1;
                    let new_tab = super::document::DocumentTab::new_drawing(self.tab_counter);
                    self.tabs.push(new_tab);
                    let idx = self.tabs.len() - 1;
                    self.active_tab = idx;
                    idx
                };

                self.tabs[i].current_path = Some(path.clone());
                self.tabs[i].scene.document = doc;
                let stored = self.tabs[i].scene.document.header.user_real1;
                self.tabs[i].scene.annotation_scale =
                    if stored > 1e-9 { stored as f32 } else { 1.0 };

                // Auto-resolve XREFs relative to the opened file's directory.
                if let Some(base_dir) = path.parent() {
                    let xrefs =
                        crate::io::xref::resolve_xrefs(&mut self.tabs[i].scene.document, base_dir);
                    // xref content arrives un-purged: parser-garbage entities
                    // inside the referenced file can trigger infinite loops in
                    // tessellation. Run the corrupt-entity guard again.
                    let extra_dropped = crate::io::purge_corrupt_entities(
                        &mut self.tabs[i].scene.document,
                    );
                    if extra_dropped > 0 {
                        self.command_line.push_error(&format!(
                            "Warning: {extra_dropped} corrupt xref entities dropped"
                        ));
                    }
                    for info in &xrefs {
                        match info.status {
                            crate::io::xref::XrefStatus::Loaded => {
                                self.command_line
                                    .push_output(&format!("XREF  Loaded \"{}\"", info.name));
                            }
                            crate::io::xref::XrefStatus::NotFound => {
                                self.command_line.push_error(&format!(
                                    "XREF  Not found: \"{}\" ({})",
                                    info.name, info.path
                                ));
                            }
                            crate::io::xref::XrefStatus::Unloaded => {
                                self.command_line.push_info(&format!(
                                    "XREF  Unloaded (skipped): \"{}\"",
                                    info.name
                                ));
                            }
                        }
                    }
                }

                // Caches were built on the background thread inside open_path().
                self.tabs[i].scene.world_offset = caches.world_offset;
                self.tabs[i].scene.local_extent_max = caches.local_extent_max;
                self.tabs[i].scene.hatches = caches.hatches;
                self.tabs[i].scene.images = caches.images;
                self.tabs[i].scene.meshes = caches.meshes;
                // Invalidate the wire cache so the new document is tessellated.
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].scene.selected = std::collections::HashSet::new();
                self.tabs[i].scene.preview_wires = vec![];
                self.tabs[i].scene.current_layout = "Model".to_string();
                crate::linetypes::populate_document(&mut self.tabs[i].scene.document);
                self.tabs[i].properties = PropertiesPanel::empty();
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                let vp_info = self.tabs[i].scene.viewport_list();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                self.sync_ribbon_layers();
                // Reset the Home-ribbon Color / Linetype / Lineweight chips
                // to the newly opened document's CECOLOR / CELTYPE / CELWEIGHT
                // defaults (or to ByLayer when the file leaves them empty).
                // Without this they stick to whatever the prior tab had
                // selected — see #21.
                self.sync_ribbon_from_selection();
                self.tabs[i].scene.restore_saved_camera();
                self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
                self.tabs[i].dirty = false;
                self.tabs[i].history = super::document::HistoryState::default();
                self.refresh_selected_grips();
                Task::none()
            }

            Message::FileOpened(Err(e)) => {
                // If the user cancelled, the overlay was already cleared and
                // we suppress the noise.
                let was_open = self.opening.take().is_some();
                if was_open && e != "Cancelled" {
                    self.command_line.push_error(&format!("Open failed: {e}"));
                }
                Task::none()
            }

            Message::ImagePick => {
                Task::perform(crate::io::pick_image_file(), Message::ImagePickResult)
            }

            Message::ImagePickResult(Ok((path, pw, ph))) => {
                use crate::command::CadCommand;
                use crate::modules::home::draw::raster_image::ImageCommand;
                let path_str = path.to_string_lossy().into_owned();
                let short = std::path::Path::new(&path_str)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&path_str)
                    .to_string();
                self.command_line
                    .push_output(&format!("IMAGE  \"{short}\": {pw}×{ph} px"));
                let cmd = ImageCommand::new(path_str, pw, ph);
                let i = self.active_tab;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
                Task::none()
            }

            Message::ImagePickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(&format!("IMAGE: {e}"));
                }
                Task::none()
            }

            Message::XAttachPick => Task::perform(
                async {
                    let handle = rfd::AsyncFileDialog::new()
                        .set_title("Select External Reference File")
                        .add_filter("CAD Files", &["dwg", "dxf", "DWG", "DXF"])
                        .add_filter("DWG Files", &["dwg", "DWG"])
                        .add_filter("DXF Files", &["dxf", "DXF"])
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(h.path().to_path_buf()),
                        None => Err("Cancelled".to_string()),
                    }
                },
                Message::XAttachPickResult,
            ),

            Message::XAttachPickResult(Ok(path)) => {
                use crate::command::CadCommand;
                use crate::modules::insert::xattach::XAttachCommand;
                let path_str = path.to_string_lossy().into_owned();
                let cmd = XAttachCommand::with_path(path_str);
                let i = self.active_tab;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
                Task::none()
            }

            Message::XAttachPickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(&format!("XATTACH: {e}"));
                }
                Task::none()
            }

            Message::WblockSave(block_name) => {
                let name = block_name.clone();
                Task::perform(
                    async move {
                        let path = rfd::AsyncFileDialog::new()
                            .set_title("Save Block As")
                            .set_file_name("block.dwg")
                            .add_filter("DWG Files", &["dwg"])
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf());
                        (name, path)
                    },
                    |(name, path)| Message::WblockSaveResult(name, path),
                )
            }

            Message::WblockSaveResult(block_name, Some(path)) => {
                let i = self.active_tab;
                let result = if block_name == "*" {
                    let handles: Vec<_> = self.tabs[i].scene.selected.iter().copied().collect();
                    crate::modules::insert::wblock::extract_entities_to_doc(
                        &self.tabs[i].scene.document,
                        &handles,
                    )
                } else {
                    crate::modules::insert::wblock::extract_block_to_doc(
                        &self.tabs[i].scene.document,
                        &block_name,
                    )
                };
                match result {
                    Ok(doc) => match crate::io::save(&doc, &path) {
                        Ok(()) => {
                            let fname = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.to_string_lossy().into_owned());
                            self.command_line.push_output(&format!(
                                "WBLOCK  Saved \"{block_name}\" → \"{fname}\""
                            ));
                        }
                        Err(e) => self
                            .command_line
                            .push_error(&format!("WBLOCK save failed: {e}")),
                    },
                    Err(e) => self.command_line.push_error(&format!("WBLOCK: {e}")),
                }
                Task::none()
            }

            Message::WblockSaveResult(_, None) => Task::none(),

            Message::DataExtractionSave(csv) => {
                let csv_clone = csv.clone();
                Task::perform(
                    async move {
                        let path = rfd::AsyncFileDialog::new()
                            .set_title("Save Data Extraction")
                            .set_file_name("extraction.csv")
                            .add_filter("CSV", &["csv"])
                            .add_filter("All Files", &["*"])
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf());
                        (csv_clone, path)
                    },
                    |(csv, path)| Message::DataExtractionSaveResult(csv, path),
                )
            }

            Message::DataExtractionSaveResult(csv, Some(path)) => {
                match std::fs::write(&path, csv.as_bytes()) {
                    Ok(()) => {
                        let rows = csv.lines().count().saturating_sub(1);
                        let fname = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.to_string_lossy().into_owned());
                        self.command_line
                            .push_output(&format!("DATAEXTRACTION  {rows} rows → \"{fname}\""));
                    }
                    Err(e) => self
                        .command_line
                        .push_error(&format!("DATAEXTRACTION: write failed: {e}")),
                }
                Task::none()
            }

            Message::DataExtractionSaveResult(_, None) => Task::none(),

            Message::StlExport => {
                let i = self.active_tab;
                if self.tabs[i].scene.meshes.is_empty() {
                    self.command_line
                        .push_error("STLOUT: no 3D mesh data in this drawing.");
                    return Task::none();
                }
                Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Export STL")
                            .set_file_name("export.stl")
                            .add_filter("STL Files", &["stl"])
                            .add_filter("All Files", &["*"])
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::StlExportPath,
                )
            }

            Message::StlExportPath(Some(path)) => {
                // Re-build STL bytes (we can't easily pass them through the message).
                let i = self.active_tab;
                // STL gets the highest-resolution LOD (slot 0) so the
                // exported geometry isn't downgraded by the view-dependent
                // mesh LOD ladder used for rendering.
                let meshes: Vec<crate::scene::mesh_model::MeshModel> = self.tabs[i]
                    .scene
                    .meshes
                    .values()
                    .filter_map(|s| s.lods.first().cloned())
                    .collect();
                let mesh_refs: Vec<&crate::scene::mesh_model::MeshModel> = meshes.iter().collect();
                match crate::io::stl::build_stl(&mesh_refs) {
                    Some(bytes) => match std::fs::write(&path, bytes) {
                        Ok(()) => self
                            .command_line
                            .push_output(&format!("STLOUT: exported to \"{}\"", path.display())),
                        Err(e) => self
                            .command_line
                            .push_error(&format!("STLOUT: write error: {e}")),
                    },
                    None => self
                        .command_line
                        .push_error("STLOUT: no mesh data to export."),
                }
                Task::none()
            }

            Message::StlExportPath(None) => Task::none(),

            // ── STEP AP203 export ─────────────────────────────────────────
            Message::StepExport => {
                let i = self.active_tab;
                if self.tabs[i].scene.meshes.is_empty() {
                    self.command_line
                        .push_error("STEPOUT: no 3D mesh data in this drawing.");
                    return Task::none();
                }
                Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Export STEP AP203")
                            .set_file_name("export.step")
                            .add_filter("STEP Files", &["step", "stp"])
                            .add_filter("All Files", &["*"])
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::StepExportPath,
                )
            }

            Message::StepExportPath(Some(path)) => {
                let i = self.active_tab;
                // Export uses LOD 0 (full resolution); see StlExportPath above.
                let meshes: Vec<crate::scene::mesh_model::MeshModel> = self.tabs[i]
                    .scene
                    .meshes
                    .values()
                    .filter_map(|s| s.lods.first().cloned())
                    .collect();
                let mesh_refs: Vec<&crate::scene::mesh_model::MeshModel> = meshes.iter().collect();
                match crate::io::step::build_step(&mesh_refs) {
                    Some(text) => match std::fs::write(&path, text.as_bytes()) {
                        Ok(()) => self
                            .command_line
                            .push_output(&format!("STEPOUT: exported to \"{}\"", path.display())),
                        Err(e) => self
                            .command_line
                            .push_error(&format!("STEPOUT: write error: {e}")),
                    },
                    None => self
                        .command_line
                        .push_error("STEPOUT: no mesh data to export."),
                }
                Task::none()
            }

            Message::StepExportPath(None) => Task::none(),

            // ── OBJ import ────────────────────────────────────────────────
            Message::ObjImport => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Import OBJ Mesh")
                        .add_filter("Wavefront OBJ", &["obj", "OBJ"])
                        .add_filter("All Files", &["*"])
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::ObjImportPath,
            ),

            Message::ObjImportPath(Some(path)) => {
                let src = match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        self.command_line
                            .push_error(&format!("IMPORTOBJ: read error: {e}"));
                        return Task::none();
                    }
                };
                let color = [0.7f32, 0.7, 0.85, 1.0];
                match crate::io::obj::parse_obj(&src, color) {
                    None => {
                        self.command_line
                            .push_error("IMPORTOBJ: no usable geometry in file.");
                    }
                    Some(mut mesh) => {
                        let i = self.active_tab;
                        let file_stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "obj_mesh".into());
                        mesh.name = file_stem.clone();
                        self.push_undo_snapshot(i, "IMPORTOBJ");
                        use crate::modules::insert::solid3d_cmds::empty_solid3d;
                        let entity = empty_solid3d();
                        let handle = self.tabs[i].scene.add_entity(entity);
                        if !handle.is_null() {
                            self.tabs[i]
                                .scene
                                .meshes
                                .insert(handle, crate::scene::MeshLodSet::from_single(mesh));
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(&format!(
                                "IMPORTOBJ: imported \"{}\" as mesh.",
                                file_stem
                            ));
                        }
                    }
                }
                Task::none()
            }

            Message::ObjImportPath(None) => Task::none(),

            Message::SaveFile => {
                let i = self.active_tab;
                if let Some(path) = &self.tabs[i].current_path {
                    let path = path.clone();
                    self.tabs[i].scene.document.header.user_real1 =
                        self.tabs[i].scene.annotation_scale as f64;
                    match crate::io::save(&self.tabs[i].scene.document, &path) {
                        Ok(()) => {
                            self.command_line
                                .push_output(&format!("Saved: {}", path.display()));
                            self.tabs[i].dirty = false;
                        }
                        Err(e) => self.command_line.push_error(&format!("Save failed: {e}")),
                    }
                    Task::none()
                } else {
                    self.save_dialog_for_unsaved = false;
                    self.open_save_dialog_window(i)
                }
            }

            Message::SaveAs => {
                let i = self.active_tab;
                self.save_dialog_for_unsaved = false;
                self.open_save_dialog_window(i)
            }

            Message::SaveDialogFormatChanged(fmt) => {
                let (ext, _) = crate::io::parse_save_format(&fmt);
                let stem = std::path::Path::new(&self.save_dialog_filename)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".to_string());
                self.save_dialog_filename = format!("{stem}.{ext}");
                self.save_dialog_format = fmt;
                Task::none()
            }

            Message::SaveDialogFilenameChanged(name) => {
                self.save_dialog_filename = name;
                Task::none()
            }

            Message::SaveDialogNavigate(path) => {
                self.save_dialog_folder = path.clone();
                self.save_dialog_entries = crate::io::read_dir_entries(&path);
                Task::none()
            }

            Message::SaveDialogEntryClicked(path, is_dir) => {
                if is_dir {
                    self.save_dialog_folder = path.clone();
                    self.save_dialog_entries = crate::io::read_dir_entries(&path);
                } else {
                    // Fill filename from clicked file.
                    if let Some(name) = path.file_name() {
                        self.save_dialog_filename = name.to_string_lossy().into_owned();
                    }
                }
                Task::none()
            }

            Message::SaveDialogConfirm => {
                let path = self.save_dialog_folder.join(&self.save_dialog_filename);
                let (_, version) = crate::io::parse_save_format(&self.save_dialog_format);
                let close = self.close_save_dialog_window();
                let i = self.active_tab;
                self.tabs[i].scene.document.header.user_real1 =
                    self.tabs[i].scene.annotation_scale as f64;
                match crate::io::save_as_version(&self.tabs[i].scene.document, &path, version) {
                    Ok(()) => {
                        self.command_line
                            .push_output(&format!("Saved: {}", path.display()));
                        self.tabs[i].current_path = Some(path.clone());
                        self.tabs[i].dirty = false;
                        if self.save_dialog_for_unsaved {
                            let next = self.update(Message::UnsavedPickedSavePath(Some(path)));
                            return Task::batch([close, next]);
                        }
                    }
                    Err(e) => self.command_line.push_error(&format!("Save failed: {e}")),
                }
                close
            }

            Message::SaveDialogCancel => self.close_save_dialog_window(),

            Message::ClearScene => {
                let i = self.active_tab;
                self.push_undo_snapshot(i, "CLEAR");
                self.tabs[i].scene.clear();
                crate::linetypes::populate_document(&mut self.tabs[i].scene.document);
                self.tabs[i].properties = PropertiesPanel::empty();
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                let vp_info = self.tabs[i].scene.viewport_list();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                self.command_line
                    .push_output("Scene cleared. Standard linetypes loaded.");
                self.tabs[i].current_path = None;
                self.tabs[i].dirty = true;
                self.sync_ribbon_layers();
                Task::none()
            }

            Message::SetWireframe(w) => {
                // Back-compat shim: forward to the new render-mode path so
                // the ribbon button + WIREFRAME / SOLID command line still
                // work without duplicating the rendering plumbing.
                let mode = if w {
                    acadrust::entities::ViewportRenderMode::Wireframe2D
                } else {
                    acadrust::entities::ViewportRenderMode::FlatShaded
                };
                Task::done(Message::SetRenderMode(mode))
            }

            Message::SetRenderMode(mode) => {
                use acadrust::entities::ViewportRenderMode as M;
                let i = self.active_tab;
                let label = match mode {
                    M::Wireframe2D => "Wireframe 2D",
                    M::Wireframe3D => "Wireframe 3D",
                    M::HiddenLine => "Hidden Line",
                    M::FlatShaded => "Flat Shaded",
                    M::GouraudShaded => "Gouraud Shaded",
                    M::FlatShadedWithEdges => "Flat Shaded + Edges",
                    M::GouraudShadedWithEdges => "Gouraud Shaded + Edges",
                };
                // In a paper layout with an active (double-clicked)
                // viewport, the picker drives that viewport entity's own
                // render mode; the model-layout tab style is untouched.
                if self.tabs[i].scene.set_active_viewport_render_mode(mode) {
                    self.tabs[i].scene.bump_geometry();
                    self.command_line
                        .push_output(&format!("Viewport visual style: {label}"));
                    return Task::none();
                }
                self.tabs[i].render_mode = mode;
                // Keep the legacy `wireframe` bool synced — both wireframe
                // modes set it, everything else clears it.
                let wf = matches!(mode, M::Wireframe2D | M::Wireframe3D);
                self.tabs[i].wireframe = wf;
                self.ribbon.set_wireframe(wf);
                self.tabs[i].visual_style = label.into();
                // Re-upload face3d fills on the next frame — the render
                // pipeline keys its upload cache off `geometry_epoch`.
                self.tabs[i].scene.bump_geometry();
                self.command_line
                    .push_output(&format!("Visual style: {label}"));
                Task::none()
            }

            Message::SetProjection(ortho) => {
                use crate::scene::Projection;
                let proj = if ortho {
                    Projection::Orthographic
                } else {
                    Projection::Perspective
                };
                let i = self.active_tab;
                self.tabs[i].scene.camera.borrow_mut().projection = proj;
                self.tabs[i].scene.camera_generation += 1;
                self.ribbon.set_ortho(ortho);
                self.command_line.push_output(if ortho {
                    "Projection: Orthographic"
                } else {
                    "Projection: Perspective"
                });
                Task::none()
            }

            Message::RibbonSelectTab(idx) => {
                self.ribbon.select(idx);
                Task::none()
            }

            Message::RibbonToolClick { tool_id, event } => {
                self.ribbon.activate_tool(&tool_id);
                match event {
                    ModuleEvent::Command(cmd) => return self.dispatch_command(&cmd),
                    ModuleEvent::OpenFileDialog => {
                        self.command_line
                            .push_info("Open DWG/DXF: not yet implemented.");
                    }
                    ModuleEvent::ClearModels => {
                        let i = self.active_tab;
                        self.tabs[i].scene.clear();
                        self.tabs[i].properties = PropertiesPanel::empty();
                        self.command_line.push_output("Scene cleared.");
                    }
                    ModuleEvent::SetWireframe(w) => {
                        let i = self.active_tab;
                        self.tabs[i].wireframe = w;
                        self.ribbon.set_wireframe(w);
                        self.tabs[i].visual_style = if w {
                            "Wireframe".into()
                        } else {
                            "Shaded".into()
                        };
                        self.command_line.push_output(if w {
                            "Visual style: Wireframe"
                        } else {
                            "Visual style: Shaded"
                        });
                    }
                    ModuleEvent::ToggleLayers => {
                        return Task::done(Message::ToggleLayers);
                    }
                }
                Task::none()
            }

            // ── Application menu ──────────────────────────────────────────
            Message::ToggleAppMenu => {
                self.app_menu.toggle();
                Task::none()
            }
            Message::CloseAppMenu => {
                self.app_menu.close();
                Task::none()
            }
            Message::CloseAppMenuAndRun(cmd) => {
                self.app_menu.close();
                self.dispatch_command(&cmd.clone())
            }
            Message::AppMenuSearch(s) => {
                self.app_menu.search = s;
                Task::none()
            }

            // ── Document tabs ─────────────────────────────────────────────
            Message::TabNew => {
                self.tab_counter += 1;
                let new_tab = super::document::DocumentTab::new_drawing(self.tab_counter);
                self.tabs.push(new_tab);
                self.active_tab = self.tabs.len() - 1;
                self.sync_ribbon_layers();
                // #21: reset ribbon Color / Linetype / Lineweight to the
                // fresh tab's defaults (ByLayer) instead of inheriting the
                // previous tab's last selection.
                self.sync_ribbon_from_selection();
                Task::none()
            }

            Message::TabSwitch(idx) => {
                if idx < self.tabs.len() {
                    self.active_tab = idx;
                    self.sync_ribbon_layers();
                    // #21: also re-seed ribbon Color / Linetype / Lineweight
                    // from the newly active tab so they reflect that doc's
                    // CECOLOR / CELTYPE / CELWEIGHT (or its current selection
                    // if there is one), not the prior tab's choice.
                    self.sync_ribbon_from_selection();
                }
                Task::none()
            }

            Message::TabClose(idx) => {
                // Start tab is fixed — close requests on it are no-ops.
                if self.tabs.get(idx).map_or(false, |t| t.is_start) {
                    return Task::none();
                }
                if self.tabs.get(idx).map_or(false, |t| t.dirty) {
                    self.pending_close = Some(super::PendingClose::Tab(idx));
                    return self.open_unsaved_dialog_window();
                }
                // Only-tab case: when the lone non-start tab closes, fall
                // back to the Start tab if it exists; otherwise spawn a
                // fresh blank drawing (legacy behaviour).
                if self.tabs.len() == 1 {
                    self.tab_counter += 1;
                    self.tabs[0] =
                        super::document::DocumentTab::new_drawing(self.tab_counter);
                    self.active_tab = 0;
                } else {
                    self.tabs.remove(idx);
                    if self.active_tab >= self.tabs.len() {
                        self.active_tab = self.tabs.len() - 1;
                    }
                }
                // The active tab is now either a brand-new blank or a
                // different existing tab; in both cases the ribbon needs
                // to track that doc's defaults / selection. #21.
                self.sync_ribbon_layers();
                self.sync_ribbon_from_selection();
                Task::none()
            }

            Message::CommandInput(s) => {
                // Space submits the current input the same way Enter does
                // (CAD convention) — unless the active command is collecting
                // free-form text (TEXT / MTEXT / DDEDIT / attribute value
                // prompts) where Space must reach the buffer as a literal
                // character. `wants_text_with_spaces()` flags those prompts.
                let i = self.active_tab;
                let allow_literal_space = self
                    .tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.wants_text_input() && c.wants_text_with_spaces())
                    .unwrap_or(false);
                if !allow_literal_space && s.ends_with(' ') {
                    self.command_line.input = s.trim_end_matches(' ').to_string();
                    return Task::done(Message::CommandSubmit);
                }
                self.command_line.input = s;
                // Typing invalidates the previous arrow-key cursor —
                // the matches list has likely changed.
                self.command_line.autocomplete_cursor = None;
                Task::none()
            }

            Message::CommandAppendChar(s) => {
                // Filter out control characters — only push the typed
                // glyph(s). `Tab`, etc. arrive as Named keys, not here.
                if s.chars().all(|c| !c.is_control()) {
                    let i = self.active_tab;
                    // `,` is the coordinate separator in dynamic input,
                    // not a decimal point: typing it locks the current
                    // field's buffer and advances to the next coordinate,
                    // reshaping the field set when going polar → cartesian
                    // (Distance → X, Y) or 2-D → 3-D (X, Y → X, Y, Z).
                    // See #35.
                    if s == ","
                        && self.dyn_input
                        && !self.tabs[i].dyn_fields.is_empty()
                    {
                        self.dyn_comma_advance();
                        self.command_line.autocomplete_cursor = None;
                        return self.focus_cmd_input();
                    }
                    // While dynamic input is showing fields, numeric
                    // glyphs edit the focused field instead of the
                    // command line. Letters still go to the command line
                    // so command-option keywords keep working.
                    let numeric = !s.is_empty()
                        && s.chars()
                            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'));
                    if numeric && self.dyn_input && !self.tabs[i].dyn_fields.is_empty() {
                        let a = self.tabs[i].dyn_active.min(self.tabs[i].dyn_fields.len() - 1);
                        self.tabs[i].dyn_fields[a]
                            .buffer
                            .get_or_insert_with(String::new)
                            .push_str(&s);
                    } else {
                        self.command_line.input.push_str(&s);
                    }
                }
                self.command_line.autocomplete_cursor = None;
                self.focus_cmd_input()
            }

            Message::CommandBackspace => {
                let i = self.active_tab;
                // Backspace edits the focused dynamic-input field first;
                // emptying it unlocks the field (back to cursor tracking).
                if self.dyn_input && !self.tabs[i].dyn_fields.is_empty() {
                    let a = self.tabs[i].dyn_active.min(self.tabs[i].dyn_fields.len() - 1);
                    if let Some(buf) = self.tabs[i].dyn_fields[a].buffer.as_mut() {
                        buf.pop();
                        if buf.is_empty() {
                            self.tabs[i].dyn_fields[a].buffer = None;
                        }
                        return self.focus_cmd_input();
                    }
                }
                self.command_line.input.pop();
                self.command_line.autocomplete_cursor = None;
                self.focus_cmd_input()
            }

            Message::DynTabNext if self.grip_popup.is_some() => {
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = (popup.selected + 1) % popup.items.len();
                    }
                }
                Task::none()
            }

            Message::DynTabNext => {
                let i = self.active_tab;
                let n = self.tabs[i].dyn_fields.len();
                if n > 0 {
                    self.tabs[i].dyn_active = (self.tabs[i].dyn_active + 1) % n;
                }
                self.focus_cmd_input()
            }

            Message::SplitModelViewport(horizontal) => {
                let i = self.active_tab;
                self.tabs[i].scene.split_active_model_tile(horizontal);
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::CommandHistoryPrev => {
                // Grip popup wins first — arrow keys walk its items.
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = if popup.selected == 0 {
                            popup.items.len() - 1
                        } else {
                            popup.selected - 1
                        };
                    }
                    return Task::none();
                }
                // While autocomplete is showing suggestions, ↑ walks up
                // that list. Otherwise it falls back to recall history.
                let i = self.active_tab;
                if self.tabs[i].active_cmd.is_none()
                    && self.command_line.autocomplete_prev()
                {
                    return Task::none();
                }
                self.command_line.history_prev();
                Task::none()
            }

            Message::CommandHistoryNext => {
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = (popup.selected + 1) % popup.items.len();
                    }
                    return Task::none();
                }
                let i = self.active_tab;
                if self.tabs[i].active_cmd.is_none()
                    && self.command_line.autocomplete_next()
                {
                    return Task::none();
                }
                self.command_line.history_next();
                Task::none()
            }

            Message::CommandHistoryToggle => {
                self.command_line.toggle_history();
                Task::none()
            }

            Message::CommandSuggestionPick(cmd) => {
                self.command_line.input.clear();
                self.command_line.close_history();
                self.dispatch_command(&cmd)
            }

            Message::CommandSubmit => {
                // Submitting a command implicitly dismisses the history
                // dropdown so the dispatched command's new prompt is
                // immediately visible on the overlay.
                self.command_line.close_history();
                // Grip-menu value prompt — consume the typed number and
                // route it through `apply_grip_menu_value`.
                if let Some(pending) = self.grip_pending.take() {
                    let raw = self.command_line.input.trim().to_string();
                    self.command_line.input.clear();
                    let Ok(v) = raw.parse::<f64>() else {
                        self.command_line.push_error(&format!(
                            "{}: expected a number, got \"{raw}\"",
                            pending.label
                        ));
                        return Task::none();
                    };
                    let i = self.active_tab;
                    use crate::entities::traits::EntityTypeOps;
                    self.push_undo_snapshot(i, pending.label);
                    if let Some(entity) = self
                        .tabs[i]
                        .scene
                        .document
                        .get_entity_mut(pending.handle)
                    {
                        entity.apply_grip_menu_value(
                            pending.grip_id,
                            pending.action,
                            v,
                        );
                    }
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                    self.refresh_selected_grips();
                    self.refresh_properties();
                    return Task::none();
                }
                // Interactive VPORTS: the entry after a bare `VPORTS` is the
                // tiled configuration. Empty input defaults to SINGLE.
                if self.awaiting_vports {
                    self.awaiting_vports = false;
                    let cfg = self.command_line.input.trim().to_string();
                    self.command_line.input.clear();
                    let cfg = if cfg.is_empty() { "SINGLE".to_string() } else { cfg };
                    return self.dispatch_command(&format!("VPORTS {cfg}"));
                }
                // If the user navigated the autocomplete list with the
                // arrow keys, Enter dispatches the highlighted command
                // rather than the partial text actually in the buffer.
                let i_tab = self.active_tab;
                if self.tabs[i_tab].active_cmd.is_none() {
                    if let Some(picked) = self.command_line.selected_suggestion() {
                        let cmd = picked.to_string();
                        self.command_line.input.clear();
                        self.command_line.autocomplete_cursor = None;
                        return self.dispatch_command(&cmd);
                    }
                }
                let i = self.active_tab;
                // With the command line empty, a typed dynamic-input value
                // commits as a point pick instead of an empty submit.
                if self.tabs[i].active_cmd.is_some()
                    && self.command_line.input.trim().is_empty()
                {
                    if let Some(task) = self.try_dyn_commit() {
                        return task;
                    }
                }
                if self.tabs[i].active_cmd.is_some() {
                    let text = self.command_line.input.trim().to_string();
                    self.command_line.input.clear();

                    if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.wants_text_input())
                        .unwrap_or(false)
                    {
                        if let Some(result) = self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .and_then(|c| c.on_text_input(&text))
                        {
                            return self.apply_cmd_result(result);
                        }
                        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
                        if let Some(p) = prompt {
                            self.command_line.push_info(&p);
                        }
                        let pt = self.tabs[i].last_cursor_world;
                        let previews = self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .map(|c| c.on_preview_wires(pt))
                            .unwrap_or_default();
                        self.tabs[i].scene.set_preview_wires(previews);
                        return self.focus_cmd_input();
                    }

                    if text.is_empty() {
                        let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_enter());
                        if let Some(r) = result {
                            return self.apply_cmd_result(r);
                        }
                        return Task::none();
                    }

                    if let Some((coord, kind)) = parse_coord(&text) {
                        // Resolve relative vs absolute. `@` forces relative,
                        // `#` forces absolute, no prefix follows DYN: when
                        // dynamic input is on, bare coordinates are relative
                        // to the last point (issue #26).
                        // `@` forces relative, `#` forces absolute, no
                        // prefix follows DYN (on → relative). The very
                        // first point has no reference, so relative falls
                        // back to absolute.
                        let want_relative = match kind {
                            CoordKind::Relative => true,
                            CoordKind::Absolute => false,
                            CoordKind::Default => self.dyn_input,
                        };
                        let ucs = self.tabs[i].active_ucs.clone();
                        let wcs_pt = match (want_relative, self.last_point) {
                            (true, Some(base)) => {
                                // Offset from the last point, rotated by the
                                // UCS axes (no origin translation).
                                let offset = match &ucs {
                                    Some(u) => ucs_rotate_vec(coord, u),
                                    None => coord,
                                };
                                base + offset
                            }
                            _ => {
                                // Absolute: typed coordinates are in active UCS.
                                match &ucs {
                                    Some(u) => ucs_to_wcs(coord, u),
                                    None => coord,
                                }
                            }
                        };
                        self.last_point = Some(wcs_pt);
                        self.dyn_user_reshaped = false;
                        self.sync_dyn_fields();
                        let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_point(wcs_pt));
                        if let Some(r) = result {
                            let task = self.apply_cmd_result(r);
                            // The rubber-band preview that the command
                            // last published reflects the *previous*
                            // last_point — a typed coordinate doesn't
                            // fire a mouse-move, so re-run the preview
                            // hook now using the current cursor world
                            // pos so the next segment immediately starts
                            // from the just-committed point. See #32.
                            self.refresh_active_cmd_preview(i);
                            return task;
                        }
                        return Task::none();
                    }

                    if let Some(result) = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .and_then(|c| c.on_text_input(&text))
                    {
                        return self.apply_cmd_result(result);
                    }

                    self.command_line.push_error(&format!(
                        "Expected coordinates (x,y) or a number, got: \"{text}\""
                    ));
                    return self.focus_cmd_input();
                }
                if let Some(cmd) = self.command_line.submit() {
                    return self.dispatch_command(&cmd);
                }
                // Empty Enter / Space with no active command repeats the
                // last dispatched command — same shortcut `CommandFinalize`
                // already implements, mirrored here so the trailing-space
                // submit path goes through it too.
                if let Some(cmd) = self.tabs[i].last_cmd.clone() {
                    return self.dispatch_command(&cmd);
                }
                Task::none()
            }

            Message::CommandFinalize => {
                // Grip popup open → Enter commits the highlighted item.
                if self.grip_popup.is_some() {
                    let idx = self
                        .grip_popup
                        .as_ref()
                        .map(|p| p.selected)
                        .unwrap_or(0);
                    return Task::done(Message::GripMenuPick(idx));
                }
                // A typed dynamic-input value commits as a point pick
                // before the plain-Enter (on_enter) path runs.
                if let Some(task) = self.try_dyn_commit() {
                    return task;
                }
                let i = self.active_tab;
                if self.tabs[i].active_cmd.is_some() {
                    let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_enter());
                    if let Some(r) = result {
                        return self.apply_cmd_result(r);
                    }
                    Task::none()
                } else if let Some(cmd) = self.tabs[i].last_cmd.clone() {
                    self.dispatch_command(&cmd)
                } else {
                    Task::none()
                }
            }

            Message::CommandEscape => {
                // Grip popup intercepts Escape — dismisses the menu
                // without doing anything else.
                if self.grip_popup.take().is_some() {
                    self.grip_hover = None;
                    return Task::none();
                }
                if self.grip_pending.take().is_some() {
                    self.command_line.input.clear();
                    return Task::none();
                }
                // Cancel layout rename / context menus first, then fall through.
                let i_e = self.active_tab;
                if self.qselect.take().is_some() {
                    return Task::none();
                }
                {
                    let mut sel = self.tabs[i_e].scene.selection.borrow_mut();
                    if sel.context_menu.is_some() {
                        sel.context_menu = None;
                        return Task::none();
                    }
                }
                if self.layout_rename_state.take().is_some()
                    || self.layout_context_menu.take().is_some()
                {
                    return Task::none();
                }
                // Typed text on the command line cancels first — one
                // Esc empties the buffer, a second Esc then escalates
                // to whatever the current mode would otherwise do
                // (cancel command / exit viewport / deselect).
                if !self.command_line.input.is_empty() {
                    self.command_line.input.clear();
                    self.command_line.autocomplete_cursor = None;
                    self.command_line.close_history();
                    return Task::none();
                }
                let i = self.active_tab;
                if self.tabs[i].active_cmd.is_some() {
                    let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_escape());
                    if let Some(r) = result {
                        return self.apply_cmd_result(r);
                    }
                } else if self.tabs[i].scene.active_viewport.is_some() {
                    // ESC while in MSPACE → exit back to paper space.
                    return Task::done(Message::ExitViewport);
                } else {
                    self.tabs[i].scene.deselect_all();
                    self.refresh_properties();
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    sel.box_anchor = None;
                    sel.box_current = None;
                    sel.box_crossing = false;
                }
                Task::none()
            }

            Message::Command(cmd) => {
                // Close viewport context menu if open.
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                self.dispatch_command(&cmd)
            }

            Message::ToggleLayers => {
                if let Some(id) = self.layer_window.take() {
                    // Close path: `OsWindowClosed`'s deactivate guard
                    // sees `layer_window == None` after the take(), so do
                    // it here so the button flips off in the same frame
                    // (#40).
                    self.ribbon.deactivate_tool_if("LAYERS");
                    window::close(id)
                } else {
                    self.sync_ribbon_layers();
                    let (id, task) = window::open(window::Settings {
                        size: iced::Size::new(900.0, 360.0),
                        resizable: true,
                        level: window::Level::AlwaysOnTop,
                        ..Default::default()
                    });
                    self.layer_window = Some(id);
                    task.map(|_| Message::Noop)
                }
            }

            Message::WindowCloseRequested(id) => {
                if self.main_window == Some(id) {
                    if self.tabs.iter().any(|t| t.dirty) {
                        self.pending_close = Some(super::PendingClose::Quit);
                        return self.open_unsaved_dialog_window();
                    }
                    return iced::exit();
                }
                Task::none()
            }

            Message::OsWindowClosed(id) => {
                if self.main_window == Some(id) {
                    // Main window was explicitly closed by us — exit.
                    return iced::exit();
                }
                if self.unsaved_dialog_window == Some(id) {
                    // User closed the dialog window via OS ✕ — treat as Cancel.
                    self.unsaved_dialog_window = None;
                    self.pending_close = None;
                    return Task::none();
                }
                if self.save_dialog_window == Some(id) {
                    self.save_dialog_window = None;
                    return Task::none();
                }
                // Each popup window that's launched from a ribbon tool
                // also turns that tool blue (`activate_tool`); when the
                // window closes, the matching tool needs to be cleared
                // or the button stays highlighted with no window behind
                // it. The mapped IDs are the ribbon `ToolDef.id`s that
                // dispatched the open in the first place. See #40.
                if self.layer_window == Some(id) {
                    self.layer_window = None;
                    self.ribbon.deactivate_tool_if("LAYERS");
                }
                if self.page_setup_window == Some(id) {
                    self.page_setup_window = None;
                    self.ribbon.deactivate_tool_if("PAGESETUP");
                }
                if self.textstyle_window == Some(id) {
                    self.textstyle_window = None;
                    self.ribbon.deactivate_tool_if("STYLE");
                    self.ribbon.deactivate_tool_if("TEXTSTYLE");
                }
                if self.tablestyle_window == Some(id) {
                    self.tablestyle_window = None;
                    self.ribbon.deactivate_tool_if("TABLESTYLE");
                }
                if self.mlstyle_window == Some(id) {
                    self.mlstyle_window = None;
                    self.ribbon.deactivate_tool_if("MLSTYLE");
                }
                if self.layout_manager_window == Some(id) {
                    self.layout_manager_window = None;
                    self.ribbon.deactivate_tool_if("LAYOUTMANAGER");
                    self.ribbon.deactivate_tool_if("LAYOUTPANEL");
                }
                if self.plotstyle_window == Some(id) {
                    self.plotstyle_window = None;
                    self.ribbon.deactivate_tool_if("PLOTSTYLE");
                    self.ribbon.deactivate_tool_if("STYLESMANAGER");
                }
                if self.dimstyle_window == Some(id) {
                    self.dimstyle_window = None;
                    self.ribbon.deactivate_tool_if("DIMSTYLE");
                }
                if self.shortcuts_window == Some(id) {
                    self.shortcuts_window = None;
                    self.ribbon.deactivate_tool_if("SHORTCUTS");
                    self.ribbon.deactivate_tool_if("KEYBOARD");
                }
                if self.about_window == Some(id) {
                    self.about_window = None;
                    self.ribbon.deactivate_tool_if("ABOUT");
                }
                if self.update_notice_window == Some(id) {
                    self.update_notice_window = None;
                }
                Task::none()
            }

            // ── Layer panel messages ───────────────────────────────────────
            Message::LayerToggleVisible(idx) => {
                let i = self.active_tab;
                if idx < self.tabs[i].layers.layers.len() {
                    self.push_undo_snapshot(i, "LAYER OFF/ON");
                    let l = &mut self.tabs[i].layers.layers[idx];
                    l.visible = !l.visible;
                    let name = l.name.clone();
                    let on = l.visible;
                    self.tabs[i].scene.toggle_layer_visibility(&name);
                    self.command_line.push_output(&format!(
                        "Layer \"{}\" {}",
                        name,
                        if on { "on" } else { "off" }
                    ));
                }
                Task::none()
            }

            Message::LayerToggleLock(idx) => {
                let i = self.active_tab;
                if idx < self.tabs[i].layers.layers.len() {
                    self.push_undo_snapshot(i, "LAYER LOCK/UNLOCK");
                    let l = &mut self.tabs[i].layers.layers[idx];
                    l.locked = !l.locked;
                    let name = l.name.clone();
                    let locked = l.locked;
                    self.tabs[i].scene.toggle_layer_lock(&name);
                    self.command_line.push_output(&format!(
                        "Layer \"{}\" {}",
                        name,
                        if locked { "locked" } else { "unlocked" }
                    ));
                }
                Task::none()
            }

            Message::LayerToggleFreeze(idx) => {
                let i = self.active_tab;
                if idx < self.tabs[i].layers.layers.len() {
                    self.push_undo_snapshot(i, "LAYER FREEZE");
                    let l = &mut self.tabs[i].layers.layers[idx];
                    l.frozen = !l.frozen;
                    let name = l.name.clone();
                    let frozen = l.frozen;
                    if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(&name) {
                        if frozen {
                            dl.freeze();
                        } else {
                            dl.thaw();
                        }
                    }
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }

            Message::LayerToggleVpFreeze(layer_idx, vp_col_idx) => {
                let i = self.active_tab;
                let vp_handle = self.tabs[i]
                    .layers
                    .vp_cols
                    .get(vp_col_idx)
                    .map(|c| c.handle);
                let layer_name = self.tabs[i]
                    .layers
                    .layers
                    .get(layer_idx)
                    .map(|l| l.name.clone());

                if let (Some(vp_handle), Some(layer_name)) = (vp_handle, layer_name) {
                    // Get the layer handle from the document
                    if let Some(doc_layer) = self.tabs[i].scene.document.layers.get(&layer_name) {
                        let layer_handle = doc_layer.handle;
                        self.push_undo_snapshot(i, "VPLAYER");

                        // Toggle frozen_layers on the viewport entity
                        for e in self.tabs[i].scene.document.entities_mut() {
                            if let acadrust::EntityType::Viewport(vp) = e {
                                if vp.common.handle == vp_handle {
                                    if vp.frozen_layers.contains(&layer_handle) {
                                        vp.frozen_layers.retain(|h| h != &layer_handle);
                                    } else {
                                        vp.frozen_layers.push(layer_handle);
                                    }
                                    break;
                                }
                            }
                        }

                        // Re-sync layer panel with updated VP info
                        let vp_info = self.tabs[i].scene.viewport_list();
                        let doc_layers = self.tabs[i].scene.document.layers.clone();
                        self.tabs[i]
                            .layers
                            .sync_with_viewports(&doc_layers, vp_info);
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }

            Message::LayerNew => {
                let i = self.active_tab;
                let mut n = 1;
                let new_name = loop {
                    let candidate = format!("Layer{}", n);
                    if !self.tabs[i].scene.document.layers.contains(&candidate) {
                        break candidate;
                    }
                    n += 1;
                };
                self.push_undo_snapshot(i, "LAYER NEW");
                use acadrust::tables::layer::Layer as DocLayer;
                let _ = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .add(DocLayer::new(&new_name));
                self.tabs[i].dirty = true;
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                let vp_info = self.tabs[i].scene.viewport_list();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                let new_idx = self.tabs[i]
                    .layers
                    .layers
                    .iter()
                    .position(|l| l.name == new_name);
                if let Some(idx) = new_idx {
                    self.tabs[i].layers.selected = Some(idx);
                    self.tabs[i].layers.editing = Some(idx);
                    self.tabs[i].layers.edit_buf = new_name.clone();
                }
                self.sync_ribbon_layers();
                Task::none()
            }

            Message::LayerDelete => {
                let i = self.active_tab;
                if let Some(idx) = self.tabs[i].layers.selected {
                    let name = self.tabs[i]
                        .layers
                        .layers
                        .get(idx)
                        .map(|l| l.name.clone())
                        .unwrap_or_default();
                    if name == "0" {
                        return Task::none();
                    }
                    self.push_undo_snapshot(i, "LAYER DELETE");
                    self.tabs[i].scene.document.layers.remove(&name);
                    self.tabs[i].dirty = true;
                    let doc_layers = self.tabs[i].scene.document.layers.clone();
                    let vp_info = self.tabs[i].scene.viewport_list();
                    self.tabs[i]
                        .layers
                        .sync_with_viewports(&doc_layers, vp_info);
                    self.tabs[i].layers.selected = None;
                    self.sync_ribbon_layers();
                }
                Task::none()
            }

            Message::LayerSetCurrent => {
                let i = self.active_tab;
                if let Some(idx) = self.tabs[i].layers.selected {
                    if let Some(layer) = self.tabs[i].layers.layers.get(idx) {
                        let name = layer.name.clone();
                        self.tabs[i].active_layer = name.clone();
                        self.tabs[i].layers.current_layer = name.clone();
                        self.ribbon.active_layer = name;
                    }
                }
                Task::none()
            }

            Message::LayerSelect(idx) => {
                let i = self.active_tab;
                if self.tabs[i].layers.editing.is_some() {
                    return Task::done(Message::LayerRenameCommit);
                }
                self.tabs[i].layers.selected = Some(idx);
                Task::none()
            }

            Message::LayerRenameStart(idx) => {
                let i = self.active_tab;
                self.tabs[i].layers.selected = Some(idx);
                if let Some(layer) = self.tabs[i].layers.layers.get(idx) {
                    self.tabs[i].layers.edit_buf = layer.name.clone();
                }
                self.tabs[i].layers.editing = Some(idx);
                Task::none()
            }

            Message::LayerRenameEdit(s) => {
                let i = self.active_tab;
                self.tabs[i].layers.edit_buf = s;
                Task::none()
            }

            Message::LayerRenameCommit => {
                let i = self.active_tab;
                let editing_idx = self.tabs[i].layers.editing.take();
                if let Some(idx) = editing_idx {
                    let new_name = self.tabs[i].layers.edit_buf.trim().to_string();
                    let old_name = self.tabs[i]
                        .layers
                        .layers
                        .get(idx)
                        .map(|l| l.name.clone())
                        .unwrap_or_default();
                    if !new_name.is_empty()
                        && new_name != old_name
                        && !self.tabs[i].scene.document.layers.contains(&new_name)
                    {
                        self.push_undo_snapshot(i, "LAYER RENAME");
                        if let Some(old_layer) = self.tabs[i].scene.document.layers.get(&old_name) {
                            use acadrust::tables::layer::Layer as DocLayer;
                            let mut nl = DocLayer::new(&new_name);
                            nl.color = old_layer.color.clone();
                            nl.flags = old_layer.flags.clone();
                            let _ = self.tabs[i].scene.document.layers.add(nl);
                        }
                        self.tabs[i].scene.document.layers.remove(&old_name);
                        for e in self.tabs[i].scene.document.entities_mut() {
                            if e.as_entity().layer() == old_name {
                                e.as_entity_mut().set_layer(new_name.clone());
                            }
                        }
                        self.tabs[i].dirty = true;
                    }
                    let doc_layers = self.tabs[i].scene.document.layers.clone();
                    let vp_info = self.tabs[i].scene.viewport_list();
                    self.tabs[i]
                        .layers
                        .sync_with_viewports(&doc_layers, vp_info);
                    self.tabs[i].layers.edit_buf.clear();
                    self.sync_ribbon_layers();
                }
                Task::none()
            }

            Message::LayerColorPickerToggle(idx) => {
                let i = self.active_tab;
                let panel = &mut self.tabs[i].layers;
                if panel.color_picker_row == Some(idx) {
                    panel.color_picker_row = None;
                    panel.color_full_palette = false;
                } else {
                    panel.color_picker_row = Some(idx);
                    panel.color_full_palette = false;
                    panel.selected = Some(idx);
                }
                Task::none()
            }

            Message::LayerColorMorePalette => {
                let i = self.active_tab;
                self.tabs[i].layers.color_full_palette = !self.tabs[i].layers.color_full_palette;
                Task::none()
            }

            Message::LayerColorSet(aci) => {
                let i = self.active_tab;
                if let Some(idx) = self.tabs[i].layers.selected {
                    if let Some(layer) = self.tabs[i].layers.layers.get(idx) {
                        let name = layer.name.clone();
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(&name) {
                            dl.color = AcadColor::Index(aci);
                        }
                        use crate::ui::layers::iced_color_from_acad;
                        let new_color = iced_color_from_acad(&AcadColor::Index(aci));
                        if let Some(pl) = self.tabs[i].layers.layers.get_mut(idx) {
                            pl.color = new_color;
                        }
                        self.tabs[i].dirty = true;
                    }
                    self.tabs[i].layers.color_picker_row = None;
                    self.tabs[i].layers.color_full_palette = false;
                    self.sync_ribbon_layers();
                }
                Task::none()
            }

            Message::LayerLinetypeSet(lt) => {
                let i = self.active_tab;
                if let Some(idx) = self.tabs[i].layers.selected {
                    if let Some(layer) = self.tabs[i].layers.layers.get(idx) {
                        let name = layer.name.clone();
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(&name) {
                            dl.line_type = lt.clone();
                        }
                        if let Some(pl) = self.tabs[i].layers.layers.get_mut(idx) {
                            pl.linetype = lt;
                        }
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }

            Message::LayerLineweightSet(lw) => {
                let i = self.active_tab;
                if let Some(idx) = self.tabs[i].layers.selected {
                    if let Some(layer) = self.tabs[i].layers.layers.get(idx) {
                        let name = layer.name.clone();
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(&name) {
                            dl.line_weight = lw;
                        }
                        if let Some(pl) = self.tabs[i].layers.layers.get_mut(idx) {
                            pl.lineweight = lw;
                        }
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }

            Message::LayerTransparencyEdit(idx, s) => {
                let i = self.active_tab;
                if let Some(pl) = self.tabs[i].layers.layers.get_mut(idx) {
                    if let Ok(v) = s.parse::<i32>() {
                        pl.transparency = v.clamp(0, 90);
                    } else if s.is_empty() {
                        pl.transparency = 0;
                    }
                }
                Task::none()
            }

            // ── Cursor / viewport messages ─────────────────────────────────
            Message::CursorMoved(p) => {
                // `p` is relative to the ViewCube hit area's top-left. Map
                // it back to full-canvas coordinates so ViewportClick's
                // hit-test lines up. The hit area sits in the top-right of
                // the full canvas in model space, or of the active
                // viewport's screen rectangle in a paper layout.
                let i = self.active_tab;
                let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                let (ox, oy) = match self
                    .tabs[i]
                    .scene
                    .active_viewport
                    .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (vw, vh)))
                {
                    Some(rect) => (
                        rect.x + rect.width - VIEWCUBE_PAD - VIEWCUBE_HIT_SIZE,
                        rect.y + VIEWCUBE_PAD,
                    ),
                    None => {
                        // Model layout: the cube sits in the active tile's
                        // top-right corner.
                        let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                        (
                            tb.x + tb.width - VIEWCUBE_PAD - VIEWCUBE_HIT_SIZE,
                            tb.y + VIEWCUBE_PAD,
                        )
                    }
                };
                self.cursor_pos = iced::Point::new(ox + p.x, oy + p.y);

                // Drive the ViewCube hover highlight directly from this
                // message — it fires whenever the cube's hit-area overlay
                // sees motion, so we don't depend on the shader widget's
                // `Program::update` receiving the same event (overlays sit
                // above the shader and can mask it). Map the cursor into
                // the active viewport's local box and use that box's size,
                // since that's where the cube is actually drawn.
                let tile = match self
                    .tabs[i]
                    .scene
                    .active_viewport
                    .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (vw, vh)))
                {
                    Some(rect) => rect,
                    None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
                };
                let cam_rot = self.tabs[i].scene.camera.borrow().view_rotation_mat();
                let hover = hover_id(
                    self.cursor_pos.x - tile.x,
                    self.cursor_pos.y - tile.y,
                    tile.width,
                    tile.height,
                    cam_rot,
                    VIEWCUBE_PX,
                );
                self.tabs[i].scene.viewcube_hover.set(hover);
                Task::none()
            }

            Message::ViewportMove(p) => {
                let i = self.active_tab;

                // A Model-tile divider drag short-circuits the rest of
                // the move handling — the cursor neither pans the camera
                // nor updates snap state while the user is resizing
                // panes.
                if let Some(drag) = self.tile_drag.as_mut() {
                    let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                    if vw > 1.0 && vh > 1.0 {
                        let new_coord = match drag.orient {
                            TileEdgeOrient::Vertical => (p.x / vw).clamp(0.0, 1.0),
                            TileEdgeOrient::Horizontal => (p.y / vh).clamp(0.0, 1.0),
                        };
                        let (min_w, min_h) = tile_min_norm(vw, vh);
                        let min_size = match drag.orient {
                            TileEdgeOrient::Vertical => min_w,
                            TileEdgeOrient::Horizontal => min_h,
                        };
                        self.tabs[i].scene.move_model_tile_edge(
                            drag.orient,
                            drag.last_applied,
                            new_coord,
                            min_size,
                        );
                        drag.last_applied = new_coord;
                        self.tabs[i].scene.camera_generation += 1;
                    }
                    return Task::none();
                }

                // Keep the ViewCube hover in sync as the cursor leaves the
                // hit-area overlay and moves over the rest of the viewport.
                // `hover_id` returns None outside the cube box, which clears
                // any stale highlight from the previous `CursorMoved`.
                let (svw, svh) = self.tabs[i].scene.selection.borrow().vp_size;
                let cube_tile = match self
                    .tabs[i]
                    .scene
                    .active_viewport
                    .and_then(|h| self.tabs[i].scene.viewport_screen_rect(h, (svw, svh)))
                {
                    Some(rect) => rect,
                    None => self.tabs[i].scene.active_model_tile_bounds(svw, svh),
                };
                let cam_rot = self.tabs[i].scene.camera.borrow().view_rotation_mat();
                self.tabs[i].scene.viewcube_hover.set(hover_id(
                    p.x - cube_tile.x,
                    p.y - cube_tile.y,
                    cube_tile.width,
                    cube_tile.height,
                    cam_rot,
                    VIEWCUBE_PX,
                ));

                // Multi-functional grip hover: detect cursor sitting on a
                // selected entity's grip and, after a dwell, open the
                // popup menu. See scene::object::GripMenuItem.
                self.update_grip_hover(i, p);

                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.last_move_pos = Some(p);

                if sel.left_down {
                    let press = sel.left_press_pos.unwrap_or(p);
                    let dx = p.x - press.x;
                    let dy = p.y - press.y;
                    let dist2 = dx * dx + dy * dy;
                    let elapsed_ms = sel
                        .left_press_time
                        .map(|t| Instant::now().duration_since(t).as_millis())
                        .unwrap_or(u128::MAX);
                    if !sel.left_dragging && elapsed_ms >= POLY_START_DELAY_MS && dist2 > 9.0 {
                        sel.left_dragging = true;
                        sel.poly_active = true;
                        sel.poly_crossing = p.x < press.x;
                        sel.poly_points.clear();
                        sel.poly_points.push(press);
                        sel.poly_points.push(p);
                    } else if sel.left_dragging && sel.poly_active {
                        if sel.poly_points.last().map_or(true, |lp| {
                            let ddx = p.x - lp.x;
                            let ddy = p.y - lp.y;
                            ddx * ddx + ddy * ddy > 16.0
                        }) {
                            sel.poly_points.push(p);
                        }
                    }
                } else if sel.box_anchor.is_some() {
                    sel.box_current = Some(p);
                    if let Some(a) = sel.box_anchor {
                        sel.box_crossing = p.x < a.x;
                    }
                }

                if sel.right_down {
                    if let Some(press) = sel.right_press_pos {
                        let dx = p.x - press.x;
                        let dy = p.y - press.y;
                        // 8 px (64 squared) threshold so normal hand jitter
                        // between a right-button press and release doesn't
                        // promote a click-and-release to an orbit drag, which
                        // would suppress the context menu on release.
                        if !sel.right_dragging && (dx * dx + dy * dy) > 64.0 {
                            sel.right_dragging = true;
                            sel.context_menu = None;
                        }
                    }
                    if sel.right_dragging {
                        if let Some(last) = sel.right_last_pos {
                            let (dx, dy) = (p.x - last.x, p.y - last.y);
                            if self.tabs[i].scene.active_viewport.is_some() {
                                // Update position before dropping the borrow.
                                sel.right_last_pos = Some(p);
                                drop(sel);
                                self.tabs[i].scene.orbit_active_viewport(dx, dy);
                                return Task::none();
                            } else {
                                sel.right_last_pos = Some(p);
                                drop(sel);
                                self.tabs[i].scene.camera.borrow_mut().orbit(dx, dy);
                                self.tabs[i].scene.camera_generation += 1;
                                return Task::none();
                            }
                        } else {
                            sel.right_last_pos = Some(p);
                        }
                    }
                }

                let (mid_down, mid_last, vp_size) =
                    (sel.middle_down, sel.middle_last_pos, sel.vp_size);
                if mid_down {
                    if let Some(last) = mid_last {
                        let (dx, dy) = (p.x - last.x, p.y - last.y);
                        // Pan scale uses the active tile's size (ortho size
                        // is relative to viewport height), so a tiled pane
                        // pans at the correct rate.
                        let bounds = self
                            .tabs[i]
                            .scene
                            .active_model_tile_bounds(vp_size.0, vp_size.1);
                        // Drop `sel` before calling mutable scene methods.
                        drop(sel);
                        if self.tabs[i].scene.active_viewport.is_some() {
                            self.tabs[i].scene.pan_active_viewport(dx, dy, bounds);
                        } else {
                            // `bounds` is the active tile; pan by its height so
                            // the point under the cursor tracks correctly.
                            self.tabs[i]
                                .scene
                                .camera
                                .borrow_mut()
                                .pan_screen(dx, dy, bounds.height);
                            self.tabs[i].scene.camera_generation += 1;
                        }
                        self.tabs[i].scene.selection.borrow_mut().middle_last_pos = Some(p);
                        return Task::none();
                    }
                    sel.middle_last_pos = Some(p);
                }

                let dragging = sel.left_down || sel.right_down || sel.middle_down;
                let vp_size = sel.vp_size;
                drop(sel);

                // Hover (no button held): the tile under the cursor becomes
                // active, so the camera + tile bounds used for picking below
                // follow the pane the cursor is in. During a drag the active
                // tile stays put so the operation finishes in its own pane.
                if !dragging && vp_size.0 > 1.0 && vp_size.1 > 1.0 {
                    if self
                        .tabs[i]
                        .scene
                        .set_active_model_tile_at(p.x / vp_size.0, p.y / vp_size.1)
                    {
                        self.tabs[i].scene.camera_generation += 1;
                    }
                }

                // Tile-relative picking: shadow `p` with the cursor mapped
                // into the active Model tile and `vp_size` with the tile's
                // size, so every pick / snap / view_proj below operates in
                // the active pane. `p_full` keeps the canvas-space cursor
                // for screen overlays (cursor marker, snap glyph).
                let p_full = p;
                let tile_b = self
                    .tabs[i]
                    .scene
                    .active_model_tile_bounds(vp_size.0, vp_size.1);
                let p = iced::Point {
                    x: p_full.x - tile_b.x,
                    y: p_full.y - tile_b.y,
                };
                let vp_size = (tile_b.width, tile_b.height);

                // ── Grip drag ─────────────────────────────────────────────
                if let Some(grip) = self.tabs[i].active_grip.clone() {
                    let (vw, vh) = vp_size;
                    let bounds = iced::Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: vw,
                        height: vh,
                    };
                    let cam = self.tabs[i].scene.camera.borrow();
                    let raw_paper = cam.pick_on_target_plane(p, bounds);
                    let vp_mat = cam.view_proj(bounds);
                    drop(cam);
                    let raw = self.tabs[i].scene.paper_to_model(raw_paper);

                    let edited_name = grip.handle.value().to_string();
                    let all_wires = self.tabs[i].scene.hit_test_wires();
                    let snap_wires: Vec<_> = all_wires
                        .iter()
                        .filter(|w| w.name != edited_name)
                        .cloned()
                        .collect();
                    let snap_hit = self.snapper.snap(raw, p, &snap_wires, vp_mat, bounds);
                    let mut snapped = snap_hit.map(|s| s.world).unwrap_or(raw);
                    self.tabs[i].snap_result = snap_hit;
                    if let Some(s) = self.tabs[i].snap_result.as_mut() {
                        s.screen.x += tile_b.x;
                        s.screen.y += tile_b.y;
                    }

                    if snap_hit.is_none() {
                        let base = grip.origin_world;
                        if self.ortho_mode {
                            snapped = ortho_constrain(snapped, base);
                        } else if self.polar_mode {
                            snapped = polar_constrain(snapped, base, self.polar_increment_deg);
                        }
                    }

                    // Paper-space entities use sheet coordinates (no world_offset).
                    // Only add world_offset when converting local → DXF space in model space.
                    let wo_vec = if self.tabs[i].scene.current_layout == "Model" {
                        let wo = self.tabs[i].scene.world_offset;
                        glam::Vec3::new(wo[0] as f32, wo[1] as f32, wo[2] as f32)
                    } else {
                        glam::Vec3::ZERO
                    };
                    let apply = if grip.is_translate {
                        GripApply::Translate(snapped - grip.last_world)
                    } else {
                        GripApply::Absolute(snapped + wo_vec)
                    };
                    self.tabs[i]
                        .scene
                        .apply_grip(grip.handle, grip.grip_id, apply);
                    self.tabs[i].dirty = true;
                    self.tabs[i].active_grip.as_mut().unwrap().last_world = snapped;
                    self.refresh_selected_grips();
                    self.refresh_properties();
                    return Task::none();
                }

                if self.tabs[i].active_cmd.is_some() {
                    let (vw, vh) = vp_size;
                    let bounds = iced::Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: vw,
                        height: vh,
                    };
                    let cursor_paper = if let Some(ref ucs) = self.tabs[i].active_ucs {
                        let origin = glam::Vec3::new(
                            ucs.origin.x as f32,
                            ucs.origin.y as f32,
                            ucs.origin.z as f32,
                        );
                        let normal = ucs_z_axis(ucs);
                        self.tabs[i]
                            .scene
                            .camera
                            .borrow()
                            .pick_on_plane(p, bounds, normal, origin)
                    } else {
                        self.tabs[i]
                            .scene
                            .camera
                            .borrow()
                            .pick_on_target_plane(p, bounds)
                    };
                    let view_proj = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                    // Sync grid-snap spacing to the adaptive spacing of the visible grid.
                    self.snapper.grid_spacing =
                        crate::ui::overlay::compute_grid_step(view_proj, bounds);
                    // In MSPACE, map paper-space cursor to model space so that
                    // command previews and snapping work in the correct coordinate space.
                    let cursor_world = self.tabs[i].scene.paper_to_model(cursor_paper);

                    let all_wires = self.tabs[i].scene.hit_test_wires();
                    let needs_entity = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.needs_entity_pick())
                        .unwrap_or(false);
                    let is_gathering = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.is_selection_gathering())
                        .unwrap_or(false);
                    let needs_tan = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.needs_tangent_pick())
                        .unwrap_or(false);
                    self.tabs[i].snap_result = if needs_entity || is_gathering {
                        None
                    } else if needs_tan {
                        self.snapper.snap_tangent_only(
                            cursor_world,
                            p,
                            &all_wires[..],
                            view_proj,
                            bounds,
                        )
                    } else {
                        self.snapper
                            .snap(cursor_world, p, &all_wires[..], view_proj, bounds)
                    };

                    // Object Snap Tracking: update dwell and override snap if tracking.
                    let otrack_snap_world = {
                        let snap_world = self.tabs[i].snap_result.map(|s| s.world);
                        self.snapper
                            .update_otrack_dwell(snap_world, view_proj, bounds);
                        if self.tabs[i].snap_result.is_none() {
                            self.snapper
                                .otrack_snap(cursor_world, view_proj, bounds)
                                .map(|(w, _)| w)
                        } else {
                            None
                        }
                    };
                    if let Some(ow) = otrack_snap_world {
                        // Override the effective point with the OST alignment.
                        // (don't set snap_result so the normal snap marker stays hidden)
                        self.tabs[i].last_cursor_world = ow;
                    }

                    let effective = {
                        // snap.world is paper-space for viewport-projected wires; convert
                        // to model-space so previews use consistent coordinates.
                        let mut pt = self.tabs[i]
                            .snap_result
                            .map(|s| self.tabs[i].scene.paper_to_model(s.world))
                            .unwrap_or(cursor_world);
                        // Clamp to world XY only when no UCS is active; with a UCS the
                        // point already lies on the UCS XY plane.
                        if self.tabs[i].active_cmd.is_some() && self.tabs[i].active_ucs.is_none() {
                            pt.z = 0.0;
                        }
                        if let Some(base) = self.last_point {
                            if self.ortho_mode {
                                pt = ortho_constrain(pt, base);
                            } else if self.polar_mode {
                                pt = polar_constrain(pt, base, self.polar_increment_deg);
                            }
                        }
                        pt
                    };
                    self.tabs[i].last_cursor_world = effective;
                    self.tabs[i].last_cursor_screen = p_full;
                    // Snap glyph is positioned in canvas space; shift the
                    // tile-local snap screen point back to the full canvas.
                    if let Some(s) = self.tabs[i].snap_result.as_mut() {
                        s.screen.x += tile_b.x;
                        s.screen.y += tile_b.y;
                    }

                    let mut previews = if needs_entity {
                        let hover_handle =
                            scene::hit_test::click_hit(p, &all_wires[..], view_proj, bounds)
                                .and_then(|s| Scene::handle_from_wire_name(s))
                                .unwrap_or(acadrust::Handle::NULL);
                        self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .map(|c| c.on_hover_entity(hover_handle, effective))
                            .unwrap_or_default()
                    } else {
                        self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .map(|c| c.on_preview_wires(effective))
                            .unwrap_or_default()
                    };
                    // Polar tracking guide line: dotted line from last_point along
                    // the snapped angle direction, extending across the drawing.
                    if self.polar_mode && !needs_entity {
                        if let Some(base) = self.last_point {
                            let dx = effective.x - base.x;
                            let dy = effective.y - base.y;
                            if (dx * dx + dy * dy).sqrt() > 1e-4 {
                                let far = 1e5_f32;
                                let dir = glam::Vec3::new(dx, dy, 0.0).normalize();
                                let far_pos = base + dir * far;
                                let far_neg = base - dir * far;
                                let guide = crate::scene::WireModel {
                                    name: "__polar_guide__".into(),
                                    points: vec![
                                        [far_neg.x, far_neg.y, far_neg.z],
                                        [far_pos.x, far_pos.y, far_pos.z],
                                    ],
                                    color: [0.2, 0.7, 0.9, 0.6],
                                    selected: false,
                                    aci: 0,
                                    pattern_length: 0.8,
                                    pattern: [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                                    line_weight_px: 1.0,
                                    snap_pts: vec![],
                                    tangent_geoms: vec![],
                                    key_vertices: vec![],
                                    aabb: crate::scene::WireModel::UNBOUNDED_AABB,
                                    plinegen: true,
                                    vp_scissor: None,
                                    fill_tris: vec![],
                                };
                                previews.push(guide);
                            }
                        }
                    }
                    self.tabs[i].scene.set_preview_wires(previews);
                } else {
                    self.tabs[i].snap_result = None;
                }

                self.sync_dyn_fields();
                Task::none()
            }

            Message::ViewportExit => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.left_down = false;
                sel.left_press_pos = None;
                sel.left_press_time = None;
                sel.left_dragging = false;
                sel.right_down = false;
                sel.right_press_pos = None;
                sel.right_last_pos = None;
                sel.right_dragging = false;
                sel.middle_down = false;
                sel.middle_last_pos = None;
                sel.box_anchor = None;
                sel.box_current = None;
                sel.box_crossing = false;
                sel.poly_active = false;
                sel.poly_points.clear();
                sel.poly_crossing = false;
                // Don't touch `context_menu` here. ViewportExit also fires
                // when an upper overlay (the right-click menu panel) takes
                // the cursor, so clearing the menu state on every exit
                // would close the menu the moment it opens. Outside-click
                // dismiss is handled in `ViewportLeftPress`.
                Task::none()
            }

            Message::ViewportLeftPress => {
                let i = self.active_tab;
                // Click anywhere outside the popup dismisses it. The
                // menu's own buttons live above this mouse_area, so a
                // press that reaches here means the cursor is not on
                // any menu item.
                if self.grip_popup.take().is_some() {
                    self.grip_hover = None;
                    return Task::none();
                }
                // Same dismiss-on-outside-click for the right-click
                // context menu: its panel is opaque, so a press that
                // reaches here is outside the menu.
                {
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    if sel.context_menu.take().is_some() {
                        return Task::none();
                    }
                }
                let (p, vp_size) = {
                    let sel = self.tabs[i].scene.selection.borrow();
                    let p = match sel.last_move_pos {
                        Some(p) => p,
                        None => return Task::none(),
                    };
                    (p, sel.vp_size)
                };
                let (vw, vh) = vp_size;

                if vw > 1.0 && vh > 1.0 {
                    let cam = self.tabs[i].scene.camera.borrow();
                    if scene::hit_test(p.x, p.y, vw, vh, cam.view_rotation_mat(), VIEWCUBE_PX)
                        .is_some()
                    {
                        return Task::none();
                    }
                }

                // Tiled Model layout: an inner divider near the cursor
                // becomes a resize grip. Start the drag and short-circuit
                // (no pick / select / camera swap). Released on
                // `ViewportLeftRelease`.
                if self.tabs[i].active_cmd.is_none()
                    && self.tabs[i].scene.current_layout == "Model"
                    && vw > 1.0
                    && vh > 1.0
                {
                    let canvas_bounds = iced::Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: vw,
                        height: vh,
                    };
                    if let Some(edge) = self.tabs[i]
                        .scene
                        .hit_model_tile_edge(p, canvas_bounds, TILE_EDGE_HIT_PX)
                    {
                        self.tile_drag = Some(crate::app::TileDrag {
                            orient: edge.orient,
                            last_applied: edge.coord,
                        });
                        return Task::none();
                    }
                }

                // Tiled Model layout: clicking a non-active tile activates
                // it (swapping in its camera) instead of selecting / drawing.
                if self.tabs[i].active_cmd.is_none() && vw > 1.0 && vh > 1.0 {
                    if self
                        .tabs[i]
                        .scene
                        .set_active_model_tile_at(p.x / vw, p.y / vh)
                    {
                        self.tabs[i].scene.camera_generation += 1;
                        return Task::none();
                    }
                }

                // From here the click targets the active tile: map the
                // cursor into it and use the tile's size for picking, so
                // grip / selection hit-tests land in the right pane.
                let p_full = p;
                let tile_b = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                let p = iced::Point {
                    x: p_full.x - tile_b.x,
                    y: p_full.y - tile_b.y,
                };
                let (vw, vh) = (tile_b.width, tile_b.height);
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };

                if self.tabs[i].active_cmd.is_none() && !self.tabs[i].selected_grips.is_empty() {
                    if let Some(handle) = self.tabs[i].selected_handle {
                        let is_paper = self.tabs[i].scene.current_layout != "Model";
                        let grip_hit = if is_paper {
                            let cam = self.tabs[i].scene.camera.borrow();
                            let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
                            let half_h = cam.ortho_size();
                            let half_w = half_h * aspect;
                            let tx = cam.target.x;
                            let ty = cam.target.y;
                            drop(cam);
                            find_hit_grip_paper(
                                p,
                                &self.tabs[i].selected_grips,
                                tx,
                                ty,
                                half_w,
                                half_h,
                                bounds,
                            )
                        } else {
                            let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                            find_hit_grip(p, &self.tabs[i].selected_grips, vp_mat, bounds)
                        };
                        if let Some((grip_id, is_translate, world)) = grip_hit {
                            self.tabs[i].active_grip = Some(GripEdit {
                                handle,
                                grip_id,
                                is_translate,
                                origin_world: world,
                                last_world: world,
                            });
                            self.grip_hover = None;
                            self.grip_popup = None;
                            return Task::none();
                        }
                    }
                }

                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.left_down = true;
                // Stored in full-canvas space (like ViewportMove's cursor and
                // the overlay box / lasso drawing); release maps it into the
                // active tile. Tile-local here would double-offset the anchor.
                sel.left_press_pos = Some(p_full);
                sel.left_press_time = Some(Instant::now());
                sel.left_dragging = false;
                Task::none()
            }

            Message::ViewportLeftRelease => {
                let i = self.active_tab;

                // End an in-flight tile-divider drag. Any tile that fell
                // below the minimum (viewcube fits comfortably) gets
                // absorbed into its longest-contact neighbour, so the
                // user can drag a divider all the way to one side to
                // remove a pane.
                if self.tile_drag.take().is_some() {
                    let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                    let (min_w, min_h) = tile_min_norm(vw, vh);
                    self.tabs[i]
                        .scene
                        .collapse_small_model_tiles(min_w, min_h);
                    self.tabs[i].scene.camera_generation += 1;
                    return Task::none();
                }

                let (p, is_click, is_down) = {
                    let sel = self.tabs[i].scene.selection.borrow();
                    let p = match sel.last_move_pos {
                        Some(p) => p,
                        None => return Task::none(),
                    };
                    (p, !sel.left_dragging, sel.left_down)
                };

                if self.tabs[i].active_grip.is_some() {
                    self.tabs[i].active_grip = None;
                    self.refresh_properties();
                    return Task::none();
                }

                // Map the release point into the active Model tile so the
                // click's pick / on_point / selection use the active pane's
                // camera + bounds. `p_full` keeps the canvas point for the
                // box/poly selection rectangle (drawn in canvas space).
                let p_full = p;
                let (tile_vw, tile_vh, tile_off) = {
                    let (svw, svh) = self.tabs[i].scene.selection.borrow().vp_size;
                    let tb = self.tabs[i].scene.active_model_tile_bounds(svw, svh);
                    (tb.width, tb.height, iced::Point::new(tb.x, tb.y))
                };
                let p = iced::Point {
                    x: p_full.x - tile_off.x,
                    y: p_full.y - tile_off.y,
                };

                let is_gathering = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.is_selection_gathering())
                    .unwrap_or(false);

                if is_down && is_click && self.tabs[i].active_cmd.is_some() && !is_gathering {
                    let (vw, vh) = (tile_vw, tile_vh);
                    let bounds = iced::Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: vw,
                        height: vh,
                    };

                    let snap_taken = self.tabs[i].snap_result.take();
                    let tangent_obj_at_click = snap_taken.and_then(|s| s.tangent_obj);

                    let world_pt = {
                        // Project screen point onto the active UCS XY plane (or world XY when
                        // no UCS is active).
                        let raw_paper = if let Some(ref ucs) = self.tabs[i].active_ucs {
                            let origin = glam::Vec3::new(
                                ucs.origin.x as f32,
                                ucs.origin.y as f32,
                                ucs.origin.z as f32,
                            );
                            let normal = ucs_z_axis(ucs);
                            self.tabs[i]
                                .scene
                                .camera
                                .borrow()
                                .pick_on_plane(p, bounds, normal, origin)
                        } else {
                            self.tabs[i]
                                .scene
                                .camera
                                .borrow()
                                .pick_on_target_plane(p, bounds)
                        };
                        // Convert paper-space → model-space when inside a viewport.
                        let raw = self.tabs[i].scene.paper_to_model(raw_paper);
                        let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                        let all_wires = self.tabs[i].scene.hit_test_wires();
                        let needs_tan = self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .map(|c| c.needs_tangent_pick())
                            .unwrap_or(false);
                        let needs_entity_click = self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .map(|c| c.needs_entity_pick())
                            .unwrap_or(false);
                        let snap_hit = if needs_entity_click {
                            None
                        } else if needs_tan {
                            self.snapper
                                .snap_tangent_only(raw, p, &all_wires[..], vp_mat, bounds)
                        } else {
                            self.snapper.snap(raw, p, &all_wires[..], vp_mat, bounds)
                        };
                        // snap.world is in paper-space (projected wire coords in MSPACE);
                        // convert to model-space so commands receive consistent coordinates.
                        let mut pt = snap_hit
                            .map(|s| self.tabs[i].scene.paper_to_model(s.world))
                            .unwrap_or(raw);
                        // When no UCS is active clamp to world XY; with a UCS the point is
                        // already constrained to that plane by the ray–plane intersection.
                        if self.tabs[i].active_ucs.is_none() {
                            pt.z = 0.0;
                        }
                        if let Some(base) = self.last_point {
                            if self.ortho_mode {
                                pt = ortho_constrain(pt, base);
                            } else if self.polar_mode {
                                pt = polar_constrain(pt, base, self.polar_increment_deg);
                            }
                        }
                        pt
                    };

                    let result = if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.needs_entity_pick())
                        .unwrap_or(false)
                    {
                        let vp_mat2 = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                        let all_wires2 = self.tabs[i].scene.hit_test_wires();
                        let hit = scene::hit_test::click_hit(p, &all_wires2[..], vp_mat2, bounds)
                            .and_then(|s| Scene::handle_from_wire_name(s));
                        if let Some(handle) = hit {
                            let result = self.tabs[i]
                                .active_cmd
                                .as_mut()
                                .map(|c| c.on_entity_pick(handle, world_pt));
                            // HATCHEDIT: after pick, inject hatch model data into the command.
                            if self.tabs[i]
                                .active_cmd
                                .as_ref()
                                .map(|c| c.name() == "HATCHEDIT")
                                .unwrap_or(false)
                            {
                                if let Some(model) =
                                    self.tabs[i].scene.hatches.get(&handle).cloned()
                                {
                                    use crate::command::CadCommand;
                                    use crate::modules::home::draw::hatchedit::HatcheditCommand;
                                    let cmd: Box<dyn CadCommand> =
                                        Box::new(HatcheditCommand::with_handle(
                                            handle,
                                            model.name.clone(),
                                            model.scale,
                                            model.angle_offset,
                                        ));
                                    self.command_line.push_info(&cmd.prompt());
                                    self.tabs[i].active_cmd = Some(cmd);
                                } else {
                                    self.command_line
                                        .push_error("HATCHEDIT: not a hatch entity.");
                                    self.tabs[i].active_cmd = None;
                                }
                            }
                            // DIMTEDIT / MLEADERADD / MLEADERREMOVE: inject cloned entity via trait.
                            {
                                let needs_inject = self.tabs[i]
                                    .active_cmd
                                    .as_ref()
                                    .map(|c| {
                                        matches!(
                                            c.name(),
                                            "DIMTEDIT" | "MLEADERADD" | "MLEADERREMOVE"
                                        )
                                    })
                                    .unwrap_or(false);
                                if needs_inject {
                                    if let Some(entity) =
                                        self.tabs[i].scene.document.get_entity(handle).cloned()
                                    {
                                        if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                                            cmd.inject_picked_entity(entity);
                                            let prompt = cmd.prompt();
                                            self.command_line.push_info(&prompt);
                                        }
                                    }
                                }
                            }
                            result
                        } else {
                            self.command_line.push_info("Nothing found at that point.");
                            None
                        }
                    } else if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.needs_tangent_pick())
                        .unwrap_or(false)
                    {
                        if let Some(obj) = tangent_obj_at_click {
                            self.tabs[i]
                                .active_cmd
                                .as_mut()
                                .map(|c| c.on_tangent_point(obj, world_pt))
                        } else {
                            self.command_line.push_info("Select a tangent object.");
                            None
                        }
                    } else {
                        self.last_point = Some(world_pt);
                        self.dyn_user_reshaped = false;
                        self.sync_dyn_fields();
                        self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .map(|c| c.on_point(world_pt))
                    };

                    if let Some(r) = result {
                        let task = self.apply_cmd_result(r);
                        let mut sel = self.tabs[i].scene.selection.borrow_mut();
                        sel.left_down = false;
                        sel.left_press_pos = None;
                        sel.left_press_time = None;
                        sel.left_dragging = false;
                        return task;
                    }
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    sel.left_down = false;
                    sel.left_press_pos = None;
                    sel.left_press_time = None;
                    sel.left_dragging = false;
                    return Task::none();
                }

                let (is_down2, is_dragging, box_anchor, box_crossing, _vp_size, elapsed_ms) = {
                    let sel = self.tabs[i].scene.selection.borrow();
                    let elapsed = sel
                        .left_press_time
                        .map(|t| Instant::now().duration_since(t).as_millis())
                        .unwrap_or(u128::MAX);
                    (
                        sel.left_down,
                        sel.left_dragging,
                        sel.box_anchor,
                        sel.box_crossing,
                        sel.vp_size,
                        elapsed,
                    )
                };

                let mut selection_just_completed = false;

                // Active-tile-local selection: tile-sized bounds and the box
                // anchor mapped into the tile, so box / crossing selection
                // matches the active pane (p is already tile-local).
                let vp_size = (tile_vw, tile_vh);
                let box_anchor = box_anchor.map(|a| iced::Point {
                    x: a.x - tile_off.x,
                    y: a.y - tile_off.y,
                });

                if is_down2 {
                    let bounds = iced::Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: vp_size.0,
                        height: vp_size.1,
                    };

                    if is_dragging {
                        if elapsed_ms < POLY_START_DELAY_MS {
                            if let Some(a) = box_anchor {
                                let crossing = box_crossing;
                                let all_wires = self.tabs[i].scene.hit_test_wires();
                                let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                                let mut handles: Vec<Handle> = scene::hit_test::box_hit(
                                    a,
                                    p,
                                    crossing,
                                    &all_wires[..],
                                    vp_mat,
                                    bounds,
                                )
                                .into_iter()
                                .filter_map(|s| Scene::handle_from_wire_name(s))
                                .collect();
                                handles.extend(scene::hit_test::box_hit_hatch(
                                    a,
                                    p,
                                    crossing,
                                    &self.tabs[i].scene.visible_hatches_for_click(),
                                    vp_mat,
                                    bounds,
                                ));
                                self.tabs[i].scene.deselect_all();
                                for h in &handles {
                                    self.tabs[i].scene.select_entity(*h, false);
                                }
                                self.tabs[i].scene.expand_selection_for_groups(&handles);
                                self.refresh_properties();
                                selection_just_completed = true;
                            }
                        } else {
                            let (poly_pts, crossing) = {
                                let sel = self.tabs[i].scene.selection.borrow();
                                // Map lasso points into the active tile.
                                let pts: Vec<iced::Point> = sel
                                    .poly_points
                                    .iter()
                                    .map(|pp| iced::Point {
                                        x: pp.x - tile_off.x,
                                        y: pp.y - tile_off.y,
                                    })
                                    .collect();
                                (pts, sel.poly_crossing)
                            };
                            self.tabs[i].scene.selection.borrow_mut().poly_last_crossing = crossing;
                            let all_wires = self.tabs[i].scene.hit_test_wires();
                            let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                            let mut handles: Vec<Handle> = scene::hit_test::poly_hit(
                                &poly_pts,
                                crossing,
                                &all_wires[..],
                                vp_mat,
                                bounds,
                            )
                            .into_iter()
                            .filter_map(|s| Scene::handle_from_wire_name(s))
                            .collect();
                            handles.extend(scene::hit_test::poly_hit_hatch(
                                &poly_pts,
                                crossing,
                                &self.tabs[i].scene.visible_hatches_for_click(),
                                vp_mat,
                                bounds,
                            ));
                            self.tabs[i].scene.deselect_all();
                            for h in &handles {
                                self.tabs[i].scene.select_entity(*h, false);
                            }
                            self.tabs[i].scene.expand_selection_for_groups(&handles);
                            self.refresh_properties();
                            selection_just_completed = true;
                        }
                        let mut sel = self.tabs[i].scene.selection.borrow_mut();
                        sel.poly_active = false;
                        sel.poly_points.clear();
                        sel.poly_crossing = false;
                        sel.box_anchor = None;
                        sel.box_current = None;
                    } else {
                        if box_anchor.is_none() {
                            let all_wires = self.tabs[i].scene.hit_test_wires();
                            let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                            let hit = scene::hit_test::click_hit(p, &all_wires[..], vp_mat, bounds)
                                .and_then(|s| Scene::handle_from_wire_name(s))
                                .or_else(|| {
                                    scene::hit_test::click_hit_hatch(
                                        p,
                                        &self.tabs[i].scene.visible_hatches_for_click(),
                                        vp_mat,
                                        bounds,
                                    )
                                })
                                .or_else(|| {
                                    // Block-internal hatch: resolve to the
                                    // parent Insert (AutoCAD behaviour).
                                    scene::hit_test::click_hit_insert_hatch(
                                        p,
                                        &self.tabs[i].scene.insert_hatches_for_click(),
                                        vp_mat,
                                        bounds,
                                    )
                                });
                            if let Some(handle) = hit {
                                self.tabs[i].scene.select_entity(handle, true);
                                self.tabs[i].scene.expand_selection_for_groups(&[handle]);
                                self.refresh_properties();
                                selection_just_completed = true;
                            } else {
                                self.tabs[i].scene.deselect_all();
                                self.refresh_properties();
                                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                                // Full-canvas space: ViewportMove updates
                                // box_current in canvas coords and the overlay
                                // draws there; release maps back into the tile.
                                sel.box_anchor = Some(p_full);
                                sel.box_current = Some(p_full);
                                sel.box_crossing = false;
                            }
                        } else {
                            let a = box_anchor.unwrap();
                            let crossing = box_crossing;
                            let all_wires = self.tabs[i].scene.hit_test_wires();
                            let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                            let mut handles: Vec<Handle> = scene::hit_test::box_hit(
                                a,
                                p,
                                crossing,
                                &all_wires[..],
                                vp_mat,
                                bounds,
                            )
                            .into_iter()
                            .filter_map(|s| Scene::handle_from_wire_name(s))
                            .collect();
                            handles.extend(scene::hit_test::box_hit_hatch(
                                a,
                                p,
                                crossing,
                                &self.tabs[i].scene.visible_hatches_for_click(),
                                vp_mat,
                                bounds,
                            ));
                            self.tabs[i].scene.deselect_all();
                            for h in &handles {
                                self.tabs[i].scene.select_entity(*h, false);
                            }
                            self.tabs[i].scene.expand_selection_for_groups(&handles);
                            self.refresh_properties();
                            let mut sel = self.tabs[i].scene.selection.borrow_mut();
                            sel.box_last = Some((a, p));
                            sel.box_last_crossing = crossing;
                            sel.box_anchor = None;
                            sel.box_current = None;
                            sel.box_crossing = false;
                            selection_just_completed = true;
                        }
                    }

                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    sel.left_down = false;
                    sel.left_press_pos = None;
                    sel.left_press_time = None;
                    sel.left_dragging = false;
                }

                if is_gathering && selection_just_completed {
                    let handles: Vec<Handle> = self.tabs[i]
                        .scene
                        .selected_entities()
                        .into_iter()
                        .map(|(h, _)| h)
                        .collect();
                    if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                        let result = cmd.on_selection_complete(handles);
                        return self.apply_cmd_result(result);
                    }
                }

                // ── Double-click in Model Space: DDEDIT for Text/MText ────
                if is_click
                    && is_down
                    && self.tabs[i].active_cmd.is_none()
                    && self.tabs[i].scene.current_layout == "Model"
                {
                    let now = Instant::now();
                    let is_double_model = self
                        .last_vp_click_time
                        .map(|t| {
                            let dt = now.duration_since(t).as_millis();
                            let last = self.last_vp_click_pos.unwrap_or(p);
                            let d = (p.x - last.x).hypot(p.y - last.y);
                            dt < 400 && d < 8.0
                        })
                        .unwrap_or(false);

                    self.last_vp_click_time = Some(now);
                    self.last_vp_click_pos = Some(p);

                    if is_double_model {
                        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                        let bounds = iced::Rectangle {
                            x: 0.0,
                            y: 0.0,
                            width: vw,
                            height: vh,
                        };
                        let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                        let all_wires = self.tabs[i].scene.hit_test_wires();
                        let hit = scene::hit_test::click_hit(p, &all_wires[..], vp_mat, bounds)
                            .and_then(|s| Scene::handle_from_wire_name(s));
                        if let Some(handle) = hit {
                            if let Some(entity) = self.tabs[i].scene.document.get_entity(handle) {
                                use crate::modules::annotate::ddedit::{
                                    entity_text, DdeditCommand,
                                };
                                if let Some(cur) = entity_text(entity) {
                                    let cmd = DdeditCommand::with_handle(handle, cur.clone());
                                    self.command_line
                                        .push_info(&format!("DDEDIT  Enter new text <{cur}>:"));
                                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                                    return self.focus_cmd_input();
                                }
                            }
                        }
                    }
                }

                // ── Double-click: enter/exit MSPACE ───────────────────────
                // Only when no command is running, no drag, and we're in paper space.
                if is_click
                    && is_down   // ensures there was a matching left-press
                    && self.tabs[i].active_cmd.is_none()
                    && self.tabs[i].scene.current_layout != "Model"
                {
                    let now = Instant::now();
                    let is_double = self
                        .last_vp_click_time
                        .map(|t| {
                            let dt = now.duration_since(t).as_millis();
                            let last = self.last_vp_click_pos.unwrap_or(p);
                            let d = (p.x - last.x).hypot(p.y - last.y);
                            dt < 400 && d < 8.0
                        })
                        .unwrap_or(false);

                    self.last_vp_click_time = Some(now);
                    self.last_vp_click_pos = Some(p);

                    if is_double {
                        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                        let bounds = iced::Rectangle {
                            x: 0.0,
                            y: 0.0,
                            width: vw,
                            height: vh,
                        };

                        // 1) Try direct wire hit — works when the border is clicked.
                        let hit_vp: Option<acadrust::Handle> = {
                            let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
                            let all_wires = self.tabs[i].scene.hit_test_wires();
                            scene::hit_test::click_hit(p, &all_wires[..], vp_mat, bounds)
                                .and_then(|s| Scene::handle_from_wire_name(s))
                                .and_then(|h| {
                                    if let Some(AcadEntityType::Viewport(vp)) =
                                        self.tabs[i].scene.document.get_entity(h)
                                    {
                                        if Scene::is_content_viewport(vp) {
                                            Some(h)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                })
                        };

                        // 2) Geometric fallback: check if the cursor is inside any
                        //    viewport's bounding rectangle in paper space.  This handles
                        //    double-clicks on model-entity content wires or empty areas.
                        let hit_vp = hit_vp.or_else(|| {
                            let paper_pt = self.tabs[i]
                                .scene
                                .camera
                                .borrow()
                                .pick_on_target_plane(p, bounds);
                            self.tabs[i]
                                .scene
                                .viewport_at_paper_point(paper_pt.x, paper_pt.y)
                        });

                        if let Some(handle) = hit_vp {
                            return Task::done(Message::EnterViewport(handle));
                        } else if self.tabs[i].scene.active_viewport.is_some() {
                            // Double-clicked outside all viewports while in MSPACE → exit.
                            return Task::done(Message::ExitViewport);
                        }
                    }
                }

                Task::none()
            }

            Message::ViewportRightPress => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                let Some(p) = sel.last_move_pos else {
                    return Task::none();
                };
                sel.context_menu = None;
                sel.right_down = true;
                sel.right_press_pos = Some(p);
                sel.right_last_pos = Some(p);
                sel.right_dragging = false;
                Task::none()
            }

            Message::ViewportRightRelease => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                let Some(_p) = sel.last_move_pos else {
                    return Task::none();
                };
                if sel.right_down {
                    if !sel.right_dragging {
                        sel.context_menu = sel.last_move_pos;
                    }
                    sel.right_down = false;
                    sel.right_press_pos = None;
                    sel.right_last_pos = None;
                    sel.right_dragging = false;
                }
                Task::none()
            }

            Message::ViewportMiddlePress => {
                let i = self.active_tab;
                let now = Instant::now();
                let is_double = {
                    let sel = self.tabs[i].scene.selection.borrow();
                    sel.middle_last_press_time
                        .map(|t| now.duration_since(t).as_millis() < 300)
                        .unwrap_or(false)
                };
                {
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    let Some(p) = sel.last_move_pos else {
                        return Task::none();
                    };
                    sel.middle_down = true;
                    sel.middle_last_pos = Some(p);
                    sel.middle_last_press_time = Some(now);
                }
                if is_double {
                    self.tabs[i].scene.fit_all();
                    self.command_line.push_output("Zoom Extents");
                }
                Task::none()
            }

            Message::ViewportMiddleRelease => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.middle_down = false;
                sel.middle_last_pos = None;
                Task::none()
            }

            Message::ViewportScroll(delta) => {
                let s = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y * 0.01,
                };
                let i = self.active_tab;
                let cursor = self.tabs[i].scene.selection.borrow().last_move_pos;
                let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                if self.tabs[i].scene.active_viewport.is_some() {
                    // In MSPACE: zoom the active viewport's model-space view,
                    // keeping the model point under the cursor stationary.
                    let cursor_paper = cursor.map(|cp| {
                        let pt = self.tabs[i]
                            .scene
                            .camera
                            .borrow()
                            .pick_on_target_plane(cp, bounds);
                        glam::Vec2::new(pt.x, pt.y)
                    });
                    self.tabs[i].scene.zoom_active_viewport(s, cursor_paper);
                } else {
                    // Model space: zoom about the cursor within the active
                    // tile so the point under it stays put in that pane.
                    let tile_b = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                    let mut cam = self.tabs[i].scene.camera.borrow_mut();
                    if let Some(cursor) = cursor {
                        let local = iced::Point {
                            x: cursor.x - tile_b.x,
                            y: cursor.y - tile_b.y,
                        };
                        let tb = iced::Rectangle {
                            x: 0.0,
                            y: 0.0,
                            width: tile_b.width,
                            height: tile_b.height,
                        };
                        cam.zoom_about_point(local, tb, s);
                    } else {
                        cam.zoom(s);
                    }
                    drop(cam);
                    self.tabs[i].scene.camera_generation += 1;
                }
                Task::none()
            }

            Message::ViewportClick => {
                let i = self.active_tab;
                let rot = self.tabs[i].scene.active_view_rotation_mat();
                let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
                // The ViewCube draws in the top-right of whichever area
                // owns it: the full canvas in model space, or the active
                // viewport's screen rectangle in a paper layout. Map the
                // cursor into that area before hit-testing so paper-space
                // picks line up with the gizmo.
                let (cx, cy, w, h) = match self
                    .tabs[i]
                    .scene
                    .active_viewport
                    .and_then(|hndl| self.tabs[i].scene.viewport_screen_rect(hndl, (vw, vh)))
                {
                    Some(rect) => (
                        self.cursor_pos.x - rect.x,
                        self.cursor_pos.y - rect.y,
                        rect.width,
                        rect.height,
                    ),
                    None => {
                        // Model layout: hit-test within the active tile.
                        let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                        (
                            self.cursor_pos.x - tb.x,
                            self.cursor_pos.y - tb.y,
                            tb.width,
                            tb.height,
                        )
                    }
                };
                if let Some(region) = scene::hit_test(cx, cy, w, h, rot, VIEWCUBE_PX) {
                    return Task::done(Message::ViewCubeSnap(region));
                }
                Task::none()
            }

            Message::WindowResized(w, h) => {
                self.vp_size = ((w - 440.0).max(200.0), h);
                Task::none()
            }

            Message::ViewCubeSnap(region) => {
                let i = self.active_tab;
                let mut region = region;
                // "Already there → flip to opposite" check: compare the
                // current gaze direction with the region's target gaze.
                let target_dir = region.snap_direction();
                let cur_dir = {
                    let cam = self.tabs[i].scene.camera.borrow();
                    cam.rotation * glam::Vec3::Z
                };
                if cur_dir.dot(target_dir) > 0.9999 {
                    region = region.opposite();
                }
                let eye_dir = region.snap_direction();

                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .snap_active_viewport_to_direction(eye_dir);
                } else {
                    let mut cam = self.tabs[i].scene.camera.borrow_mut();
                    cam.snap_to_direction(eye_dir);
                }
                self.tabs[i].scene.camera_generation += 1;
                self.command_line
                    .push_output(&format!("View: {}", region.label()));
                Task::none()
            }

            Message::GripDwellTick => {
                let i = self.active_tab;
                // Reuse the move-time logic — `p` is the last cursor
                // position the viewport saw, which is also what the
                // hover state was last set with.
                let p = self.tabs[i]
                    .scene
                    .selection
                    .borrow()
                    .last_move_pos
                    .unwrap_or(self.cursor_pos);
                self.update_grip_hover(i, p);
                Task::none()
            }

            Message::GripMenuPick(idx) => {
                let i = self.active_tab;
                let Some(popup) = self.grip_popup.take() else {
                    return Task::none();
                };
                self.grip_hover = None;
                let Some(item) = popup.items.get(idx).cloned() else {
                    return Task::none();
                };
                use crate::entities::traits::EntityTypeOps;
                use crate::scene::object::GripMenuAction;
                if matches!(item.action, GripMenuAction::Stretch) {
                    return Task::none();
                }
                // Actions that need a follow-up number stash a pending
                // state + prompt; the next typed value drives
                // `apply_grip_menu_value`.
                let prompt = self
                    .tabs[i]
                    .scene
                    .document
                    .get_entity(popup.handle)
                    .and_then(|e| e.grip_menu_value_prompt(popup.grip_id, item.action));
                if let Some(label) = prompt {
                    self.grip_pending = Some(super::GripPendingValue {
                        handle: popup.handle,
                        grip_id: popup.grip_id,
                        action: item.action,
                        label,
                    });
                    self.command_line.push_info(&format!("{label}:"));
                    return self.focus_cmd_input();
                }
                // One-shot action — apply immediately.
                self.push_undo_snapshot(i, item.label);
                if let Some(entity) = self
                    .tabs[i]
                    .scene
                    .document
                    .get_entity_mut(popup.handle)
                {
                    entity.apply_grip_menu(popup.grip_id, item.action);
                }
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].dirty = true;
                self.refresh_selected_grips();
                self.refresh_properties();
                Task::none()
            }

            // ── Snap / mode toggles ───────────────────────────────────────
            Message::ToggleSnapEnabled => {
                self.snapper.toggle_global();
                Task::none()
            }
            Message::ToggleGridSnap => {
                self.snapper.toggle(crate::snap::SnapType::Grid);
                Task::none()
            }
            Message::ToggleGrid => {
                self.show_grid ^= true;
                Task::none()
            }
            Message::ToggleOrtho => {
                self.ortho_mode ^= true;
                if self.ortho_mode {
                    self.polar_mode = false;
                }
                Task::none()
            }
            Message::ToggleLineweightDisplay => {
                let i = self.active_tab;
                if i < self.tabs.len() {
                    let h = &mut self.tabs[i].scene.document.header;
                    h.lineweight_display = !h.lineweight_display;
                    // No retessellate — the wire shader reads the flag from uniforms.
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }
            Message::TogglePolar => {
                self.polar_mode ^= true;
                if self.polar_mode {
                    self.ortho_mode = false;
                }
                Task::none()
            }
            Message::ToggleDynInput => {
                self.dyn_input ^= true;
                Task::none()
            }
            Message::ToggleViewCube => {
                self.show_viewcube ^= true;
                Task::none()
            }
            Message::ToggleProperties => {
                self.show_properties ^= true;
                Task::none()
            }
            Message::ToggleFileTabs => {
                self.show_file_tabs ^= true;
                Task::none()
            }
            Message::ToggleLayoutTabs => {
                self.show_layout_tabs ^= true;
                Task::none()
            }
            Message::ToggleOTrack => {
                self.snapper.otrack_enabled ^= true;
                if !self.snapper.otrack_enabled {
                    self.snapper.clear_tracking();
                }
                Task::none()
            }
            Message::SetPolarAngle(deg) => {
                self.polar_increment_deg = deg;
                self.polar_mode = true;
                self.ortho_mode = false;
                Task::none()
            }
            Message::SetAnnotationScale(scale) => {
                self.scale_popup_open = false;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.scene.annotation_scale = scale;
                    tab.scene.bump_geometry();
                }
                Task::none()
            }
            Message::SetViewportScale(scale) => {
                self.scale_popup_open = false;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.scene.set_viewport_scale(scale);
                }
                Task::none()
            }
            Message::ToggleScalePopup => {
                self.scale_popup_open ^= true;
                Task::none()
            }
            Message::CloseScalePopup => {
                self.scale_popup_open = false;
                Task::none()
            }
            Message::ToggleSnap(t) => {
                self.snapper.toggle(t);
                Task::none()
            }
            Message::ToggleSnapPopup => {
                self.snap_popup_open ^= true;
                Task::none()
            }
            Message::CloseSnapPopup => {
                self.snap_popup_open = false;
                Task::none()
            }
            Message::SnapSelectAll => {
                self.snapper.enable_all();
                Task::none()
            }
            Message::SnapClearAll => {
                self.snapper.disable_all();
                Task::none()
            }

            // ── Ribbon dropdowns ──────────────────────────────────────────
            Message::ToggleRibbonDropdown(id) => {
                self.ribbon.toggle_dropdown(&id);
                Task::none()
            }
            Message::CloseRibbonDropdown => {
                self.ribbon.close_dropdown();
                Task::none()
            }
            Message::DropdownSelectItem { dropdown_id, cmd } => {
                self.ribbon.select_dropdown_item(dropdown_id, cmd);
                self.ribbon.activate_tool(cmd);
                self.dispatch_command(cmd)
            }

            Message::DeleteSelected => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let handles: Vec<_> = self.tabs[i].scene.selected.iter().cloned().collect();
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "ERASE");
                    self.tabs[i].scene.erase_entities(&handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::SelectSimilar => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let added = self.tabs[i].scene.select_similar();
                self.command_line
                    .push_output(&format!("Select Similar: {} added.", added));
                self.refresh_properties();
                Task::none()
            }

            Message::QSelectOpen => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                // Seed the type filter from the first selected entity so a
                // right-click → Quick Select on a known object opens the
                // panel pre-tuned to that entity's type. Property defaults
                // to "(Any property)" so the user immediately picks what
                // they want to compare.
                let mut type_filter: Option<String> = None;
                if let Some(&h) = self.tabs[i].scene.selected.iter().next() {
                    if let Some(e) = self.tabs[i].scene.document.get_entity(h) {
                        use crate::entities::traits::entity_type_name;
                        type_filter = Some(entity_type_name(e).to_string());
                    }
                }
                self.qselect = Some(crate::app::QSelectState {
                    type_filter,
                    property: None,
                    operator: crate::app::QSelectOp::Eq,
                    value: String::new(),
                    append: false,
                });
                Task::none()
            }

            Message::QSelectClose => {
                self.qselect = None;
                Task::none()
            }

            Message::QSelectSetType(t) => {
                if let Some(state) = self.qselect.as_mut() {
                    // Drop the property when it no longer applies to the
                    // chosen type: type-specific fields like `start_x`
                    // would otherwise stay selected but never match.
                    let kept_property = state.property.clone().and_then(|p| {
                        let i = self.active_tab;
                        let props = self.tabs[i].scene.qselect_properties(t.as_deref());
                        if props.iter().any(|(f, _)| f == &p.field) {
                            Some(p)
                        } else {
                            None
                        }
                    });
                    state.type_filter = t;
                    state.property = kept_property;
                }
                Task::none()
            }

            Message::QSelectSetProperty(p) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.property = p;
                }
                Task::none()
            }

            Message::QSelectSetOperator(op) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.operator = op;
                }
                Task::none()
            }

            Message::QSelectSetValue(v) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.value = v;
                }
                Task::none()
            }

            Message::QSelectSetAppend(b) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.append = b;
                }
                Task::none()
            }

            Message::QSelectApply => {
                let Some(state) = self.qselect.take() else {
                    return Task::none();
                };
                let i = self.active_tab;
                let matched = self.tabs[i].scene.qselect(
                    state.type_filter.as_deref(),
                    state.property.as_ref().map(|p| p.field.as_str()),
                    state.operator,
                    &state.value,
                    state.append,
                );
                self.command_line
                    .push_output(&format!("QSELECT: {} object(s) selected.", matched));
                self.refresh_properties();
                Task::none()
            }

            // ── Properties panel messages ─────────────────────────────────
            Message::PropSelectionGroupChanged(group) => {
                self.tabs[self.active_tab].properties.selected_group = Some(group);
                self.refresh_properties();
                Task::none()
            }

            Message::RibbonLayerChanged(layer) => {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    // No selection — change the creation default. Persist
                    // into the tab's header (CLAYER) so it survives a tab
                    // switch and rides the next save. #21.
                    let handle = self.tabs[i]
                        .scene
                        .document
                        .layers
                        .get(&layer)
                        .map(|l| l.handle)
                        .unwrap_or(acadrust::types::Handle::NULL);
                    self.tabs[i].scene.document.header.current_layer_name = layer.clone();
                    self.tabs[i].scene.document.header.current_layer_handle = handle;
                    self.tabs[i].active_layer = layer.clone();
                    self.tabs[i].layers.current_layer = layer.clone();
                    self.tabs[i].dirty = true;
                    self.ribbon.active_layer = layer;
                } else {
                    // Apply to selection; leave the creation default alone
                    // (matches AutoCAD; "Make current" is a separate action).
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::dispatch::apply_common_prop(entity, "layer", &layer);
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.ribbon.active_layer = layer;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::RibbonColorChanged(color) => {
                let i = self.active_tab;
                self.ribbon.prop_color_palette_open = false;
                self.ribbon.close_dropdown();
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    // Persist the new default into the tab's header so it
                    // round-trips through tab switches and writes back on
                    // save (CECOLOR). #21.
                    self.tabs[i].scene.document.header.current_entity_color = color;
                    self.tabs[i].dirty = true;
                    self.ribbon.active_color = color;
                } else {
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::dispatch::apply_color(entity, color);
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.ribbon.active_color = color;
                    self.refresh_properties();
                }
                Task::none()
            }
            Message::RibbonColorPaletteToggle => {
                self.ribbon.prop_color_palette_open ^= true;
                Task::none()
            }
            Message::RibbonLinetypeChanged(lt) => {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    // Persist into the tab's header (CELTYPE). Resolve to a
                    // handle when the name matches a line_types entry so the
                    // handle-based lookup stays in sync. #21.
                    let handle = self.tabs[i]
                        .scene
                        .document
                        .line_types
                        .iter()
                        .find(|x| x.name.eq_ignore_ascii_case(&lt))
                        .map(|x| x.handle)
                        .unwrap_or(acadrust::types::Handle::NULL);
                    self.tabs[i].scene.document.header.current_linetype_name = lt.clone();
                    self.tabs[i].scene.document.header.current_linetype_handle = handle;
                    self.tabs[i].dirty = true;
                    self.ribbon.active_linetype = lt;
                } else {
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::dispatch::apply_common_prop(entity, "linetype", &lt);
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.ribbon.active_linetype = lt;
                    self.refresh_properties();
                }
                Task::none()
            }
            Message::RibbonLineweightChanged(lw) => {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    // Persist into the tab's header (CELWEIGHT). #21.
                    self.tabs[i].scene.document.header.current_line_weight = lw.value();
                    self.tabs[i].dirty = true;
                    self.ribbon.active_lineweight = lw;
                } else {
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::dispatch::apply_line_weight(entity, lw);
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.ribbon.active_lineweight = lw;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::RibbonStyleChanged { key, name } => {
                use crate::modules::StyleKey;
                self.ribbon.close_dropdown();
                match key {
                    StyleKey::TextStyle => {
                        self.ribbon.active_text_style = name.clone();
                        let i = self.active_tab;
                        let found = self.tabs[i]
                            .scene
                            .document
                            .text_styles
                            .iter()
                            .find(|s| s.name == name)
                            .map(|ts| ts.handle);
                        if let Some(h) = found {
                            self.tabs[i].scene.document.header.current_text_style_handle = h;
                            self.tabs[i].scene.document.header.current_text_style_name = name;
                        }
                    }
                    StyleKey::DimStyle => {
                        self.ribbon.active_dim_style = name.clone();
                        let i = self.active_tab;
                        let found = self.tabs[i]
                            .scene
                            .document
                            .dim_styles
                            .get(&name)
                            .map(|ds| ds.handle);
                        if let Some(h) = found {
                            self.tabs[i].scene.document.header.current_dimstyle_handle = h;
                            self.tabs[i].scene.document.header.current_dimstyle_name = name;
                        }
                    }
                    StyleKey::MLeaderStyle => {
                        self.ribbon.active_mleader_style = name.clone();
                        let i = self.active_tab;
                        self.tabs[i].active_mleader_style = name;
                    }
                    StyleKey::TableStyle => {
                        self.ribbon.active_table_style = name;
                    }
                }
                Task::none()
            }

            Message::PropLayerChanged(layer) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::dispatch::apply_common_prop(entity, "layer", &layer);
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropColorChanged(color) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::dispatch::apply_color(entity, color);
                        }
                    }
                    self.tabs[i].properties.color_picker_open = false;
                    self.tabs[i].properties.color_palette_open = false;
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropLwChanged(lw) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::dispatch::apply_line_weight(entity, lw);
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropLinetypeChanged(lt) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::dispatch::apply_common_prop(entity, "linetype", &lt);
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropHatchPatternChanged(name) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    use crate::scene::hatch_patterns;
                    if let Some(entry) = hatch_patterns::find(&name) {
                        self.push_undo_snapshot(i, "HATCHEDIT");
                        for handle in handles {
                            if let Some(acadrust::EntityType::Hatch(dxf)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                dxf.pattern = hatch_patterns::build_dxf_pattern(entry);
                                dxf.is_solid = matches!(
                                    entry.gpu,
                                    crate::scene::hatch_model::HatchPattern::Solid
                                );
                            }
                            if let Some(model) = self.tabs[i].scene.hatches.get_mut(&handle) {
                                model.pattern = entry.gpu.clone();
                                model.name = name.clone();
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                    }
                }
                Task::none()
            }

            Message::PropBoolToggle(field) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    for handle in handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            match field {
                                "invisible" => crate::scene::dispatch::toggle_invisible(entity),
                                _ => {
                                    crate::scene::dispatch::apply_geom_prop(entity, field, "toggle")
                                }
                            }
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropGeomChoiceChanged { field, value } => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    if field == "vp_ucs_name" {
                        // Resolve UCS name → cloned data, then mutate viewports.
                        let ucs_data = self.tabs[i]
                            .scene
                            .document
                            .ucss
                            .iter()
                            .find(|u| u.name == value)
                            .cloned();
                        if let Some(ucs) = ucs_data {
                            for handle in &handles {
                                if let Some(acadrust::EntityType::Viewport(vp)) =
                                    self.tabs[i].scene.document.get_entity_mut(*handle)
                                {
                                    vp.ucs_handle = ucs.handle;
                                    vp.ucs_origin = ucs.origin.clone();
                                    vp.ucs_x_axis = ucs.x_axis.clone();
                                    vp.ucs_y_axis = ucs.y_axis.clone();
                                }
                            }
                        }
                    } else if field == "vp_named_view" {
                        // Assign a named view to viewport(s): copy camera parameters.
                        let view_data = self.tabs[i]
                            .scene
                            .document
                            .views
                            .iter()
                            .find(|v| v.name == value)
                            .cloned();
                        if let Some(view) = view_data {
                            for handle in &handles {
                                if let Some(acadrust::EntityType::Viewport(vp)) =
                                    self.tabs[i].scene.document.get_entity_mut(*handle)
                                {
                                    vp.view_target = view.target.clone();
                                    vp.view_direction = view.direction.clone();
                                    if view.height > 0.0 {
                                        vp.view_height = view.height;
                                    }
                                }
                            }
                            self.tabs[i].scene.camera_generation += 1;
                        }
                    } else {
                        for handle in handles {
                            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                crate::scene::dispatch::apply_geom_prop(entity, field, &value);
                            }
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropGeomInput { field, value } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(field.to_string(), value);
                Task::none()
            }

            Message::PropGeomCommit(field) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    if let Some(val) = self.tabs[i].properties.edit_buf.remove(field) {
                        self.push_undo_snapshot(i, "CHPROP");
                        if field == "frozen_layers" {
                            // Resolve layer names → handles, then apply to viewports.
                            let layer_handles: Vec<acadrust::Handle> = val
                                .split(',')
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .filter_map(|name| {
                                    self.tabs[i]
                                        .scene
                                        .document
                                        .layers
                                        .iter()
                                        .find(|l| l.name.eq_ignore_ascii_case(name))
                                        .map(|l| l.handle)
                                })
                                .collect();
                            for handle in handles {
                                if let Some(acadrust::EntityType::Viewport(vp)) =
                                    self.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    vp.frozen_layers = layer_handles.clone();
                                }
                            }
                        } else {
                            for handle in handles {
                                if let Some(entity) =
                                    self.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    match field {
                                        "linetype_scale" | "transparency" => {
                                            crate::scene::dispatch::apply_common_prop(
                                                entity, field, &val,
                                            );
                                        }
                                        _ => {
                                            crate::scene::dispatch::apply_geom_prop(
                                                entity, field, &val,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                    }
                }
                Task::none()
            }

            Message::PropColorPickerToggle => {
                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open =
                    !self.tabs[i].properties.color_picker_open;
                if self.tabs[i].properties.color_picker_open {
                    self.tabs[i].properties.color_palette_open = false;
                }
                Task::none()
            }

            Message::PropColorPaletteToggle => {
                self.tabs[self.active_tab].properties.color_palette_open =
                    !self.tabs[self.active_tab].properties.color_palette_open;
                Task::none()
            }

            Message::LayoutSwitch(name) => {
                let i = self.active_tab;
                let going_to_paper = name != "Model";
                // Cancel any pending rename/context-menu and active viewport when switching.
                self.layout_rename_state = None;
                self.layout_context_menu = None;
                self.tabs[i].scene.active_viewport = None;
                self.tabs[i].scene.set_current_layout(name);
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.restore_saved_camera();
                self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
                if going_to_paper {
                    if let Some(idx) = self.ribbon.layout_module_index() {
                        self.ribbon.select(idx);
                    }
                } else if self.ribbon.active_is_layout() {
                    self.ribbon.select(0);
                }
                // Refresh VP freeze columns for the new layout.
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                let vp_info = self.tabs[i].scene.viewport_list();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                Task::none()
            }

            Message::LayoutCreate => {
                let i = self.active_tab;
                // Find a unique name (e.g. Layout2, Layout3, ...).
                let existing = self.tabs[i].scene.layout_names();
                let mut idx = existing.len();
                let new_name = loop {
                    let candidate = format!("Layout{}", idx);
                    if !existing.contains(&candidate) {
                        break candidate;
                    }
                    idx += 1;
                };
                self.push_undo_snapshot(i, "LAYOUT");
                match self.tabs[i].scene.document.add_layout(&new_name) {
                    Ok(_) => {
                        // Override the acadrust default limits (12×9 imperial) with A4 landscape.
                        for obj in self.tabs[i].scene.document.objects.values_mut() {
                            if let acadrust::objects::ObjectType::Layout(l) = obj {
                                if l.name == new_name {
                                    l.min_limits = (0.0, 0.0);
                                    l.max_limits = (297.0, 210.0);
                                    l.min_extents = (0.0, 0.0, 0.0);
                                    l.max_extents = (297.0, 210.0, 0.0);
                                    break;
                                }
                            }
                        }
                        self.tabs[i].scene.current_layout = new_name.clone();
                        self.tabs[i].scene.deselect_all();
                        self.tabs[i].scene.fit_all();
                        if let Some(idx) = self.ribbon.layout_module_index() {
                            self.ribbon.select(idx);
                        }
                        self.command_line.push_output(&format!(
                            "Layout \"{new_name}\" created — use MVIEW to add a viewport"
                        ));
                        self.tabs[i].dirty = true;
                    }
                    Err(e) => self
                        .command_line
                        .push_error(&format!("Failed to create layout: {e}")),
                }
                Task::none()
            }

            Message::LayoutDelete(name) => {
                let i = self.active_tab;
                self.push_undo_snapshot(i, "LAYOUT DEL");
                if self.tabs[i].scene.delete_layout(&name) {
                    self.layout_context_menu = None;
                    self.layout_rename_state = None;
                    // If we fell back to Model space, update ribbon.
                    if self.tabs[i].scene.current_layout == "Model"
                        && self.ribbon.active_is_layout()
                    {
                        self.ribbon.select(0);
                    }
                    self.command_line
                        .push_output(&format!("Layout \"{name}\" silindi"));
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }

            Message::LayoutRenameStart(name) => {
                if name != "Model" {
                    self.layout_rename_state = Some((name.clone(), name));
                    self.layout_context_menu = None;
                }
                Task::none()
            }

            Message::LayoutRenameEdit(val) => {
                if let Some((orig, _)) = &self.layout_rename_state {
                    let orig = orig.clone();
                    self.layout_rename_state = Some((orig, val));
                }
                Task::none()
            }

            Message::LayoutRenameCommit => {
                if let Some((orig, new_name)) = self.layout_rename_state.take() {
                    let new_name = new_name.trim().to_string();
                    if !new_name.is_empty() && new_name != orig {
                        let i = self.active_tab;
                        let exists = self.tabs[i]
                            .scene
                            .layout_names()
                            .iter()
                            .any(|n| *n == new_name);
                        if exists {
                            self.command_line
                                .push_error(&format!("\"{}\" name already in use", new_name));
                        } else {
                            self.push_undo_snapshot(i, "LAYOUT RENAME");
                            self.tabs[i].scene.rename_layout(&orig, &new_name);
                            if self.tabs[i].scene.current_layout == orig {
                                self.tabs[i].scene.current_layout = new_name.clone();
                            }
                            self.tabs[i].dirty = true;
                            self.command_line
                                .push_output(&format!("Layout \"{orig}\" → \"{new_name}\""));
                        }
                    }
                }
                Task::none()
            }

            Message::LayoutRenameCancel => {
                self.layout_rename_state = None;
                Task::none()
            }

            Message::LayoutContextMenu(name) => {
                if name != "Model" {
                    self.layout_context_menu = Some(name);
                }
                Task::none()
            }

            Message::LayoutContextMenuClose => {
                self.layout_context_menu = None;
                Task::none()
            }

            // ── Layout Manager Panel ──────────────────────────────────────────
            Message::LayoutManagerOpen => {
                let i = self.active_tab;
                let current = self.tabs[i].scene.current_layout.clone();
                self.layout_manager_selected = current.clone();
                self.layout_manager_rename_buf = if current == "Model" {
                    String::new()
                } else {
                    current
                };
                if let Some(id) = self.layout_manager_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(640.0, 320.0),
                    resizable: true,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.layout_manager_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::LayoutManagerClose => {
                if let Some(id) = self.layout_manager_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::LayoutManagerSelect(name) => {
                self.layout_manager_rename_buf = if name == "Model" {
                    String::new()
                } else {
                    name.clone()
                };
                self.layout_manager_selected = name;
                Task::none()
            }
            Message::LayoutManagerRenameBuf(s) => {
                self.layout_manager_rename_buf = s;
                Task::none()
            }
            Message::LayoutManagerRenameCommit => {
                let i = self.active_tab;
                let old_name = self.layout_manager_selected.clone();
                let new_name = self.layout_manager_rename_buf.trim().to_string();
                if old_name == "Model" {
                    self.command_line
                        .push_error("Cannot rename the Model layout.");
                } else if new_name.is_empty() {
                    self.command_line.push_error("Layout name cannot be empty.");
                } else if new_name == old_name {
                    // no-op
                } else {
                    self.push_undo_snapshot(i, "LAYOUT RENAME");
                    self.tabs[i].scene.rename_layout(&old_name, &new_name);
                    if self.tabs[i].scene.current_layout == old_name {
                        self.tabs[i].scene.current_layout = new_name.clone();
                    }
                    self.layout_manager_selected = new_name.clone();
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(&format!("Layout renamed: '{old_name}' → '{new_name}'"));
                }
                Task::none()
            }
            Message::LayoutManagerNew => {
                let i = self.active_tab;
                let existing = self.tabs[i].scene.layout_names();
                let n = (1usize..)
                    .find(|n| !existing.contains(&format!("Layout{n}")))
                    .unwrap_or(1);
                let name = format!("Layout{n}");
                self.push_undo_snapshot(i, "LAYOUT NEW");
                match self.tabs[i].scene.document.add_layout(&name) {
                    Ok(_) => {
                        self.tabs[i].dirty = true;
                        self.layout_manager_selected = name.clone();
                        self.layout_manager_rename_buf = name.clone();
                        self.command_line
                            .push_output(&format!("Layout '{name}' created."));
                    }
                    Err(e) => self.command_line.push_error(&format!("LAYOUT: {e}")),
                }
                Task::none()
            }
            Message::LayoutManagerDelete => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    self.command_line
                        .push_error("Cannot delete the Model layout.");
                } else {
                    self.push_undo_snapshot(i, "LAYOUT DELETE");
                    self.tabs[i].scene.delete_layout(&name);
                    self.tabs[i].dirty = true;
                    // Switch to Model if active layout was deleted.
                    if self.tabs[i].scene.current_layout == name {
                        self.tabs[i].scene.current_layout = "Model".to_string();
                        self.tabs[i].scene.bump_geometry();
                    }
                    self.layout_manager_selected = "Model".to_string();
                    self.layout_manager_rename_buf = String::new();
                    self.command_line
                        .push_output(&format!("Layout '{name}' deleted."));
                }
                Task::none()
            }
            Message::LayoutManagerMoveLeft => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    return Task::none();
                }
                let names = self.tabs[i].scene.layout_names();
                // Find position among paper layouts only.
                let paper: Vec<&str> = names.iter().skip(1).map(|s| s.as_str()).collect();
                if let Some(pos) = paper.iter().position(|&n| n == name) {
                    if pos > 0 {
                        self.push_undo_snapshot(i, "LAYOUT REORDER");
                        self.tabs[i].scene.swap_layout_order(&name, paper[pos - 1]);
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }
            Message::LayoutManagerMoveRight => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    return Task::none();
                }
                let names = self.tabs[i].scene.layout_names();
                let paper: Vec<&str> = names.iter().skip(1).map(|s| s.as_str()).collect();
                if let Some(pos) = paper.iter().position(|&n| n == name) {
                    if pos + 1 < paper.len() {
                        self.push_undo_snapshot(i, "LAYOUT REORDER");
                        self.tabs[i].scene.swap_layout_order(&name, paper[pos + 1]);
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }
            Message::LayoutManagerSetCurrent => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                self.tabs[i].scene.current_layout = name.clone();
                self.tabs[i].scene.bump_geometry();
                self.command_line
                    .push_output(&format!("Switched to layout '{name}'."));
                Task::none()
            }

            Message::SetTheme(theme) => {
                self.active_theme = theme;
                Task::none()
            }

            // ── Keyboard Shortcuts Panel ──────────────────────────────────────
            Message::ShortcutsPanelOpen => {
                if let Some(id) = self.shortcuts_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(720.0, 520.0),
                    resizable: true,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.shortcuts_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::ShortcutsPanelClose => {
                if let Some(id) = self.shortcuts_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }

            // ── About window ──────────────────────────────────────────────
            Message::AboutOpen => {
                if let Some(id) = self.about_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(340.0, 240.0),
                    resizable: false,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.about_window = Some(id);
                task.map(|_| Message::Noop)
            }

            Message::AboutCopyInfo => {
                let info = format!(
                    "Open CAD Studio v{}\nOS: {}\nArch: {}",
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::OS,
                    std::env::consts::ARCH,
                );
                iced::clipboard::write(info)
            }

            Message::EnterViewport(handle) => {
                let i = self.active_tab;
                // Clear paper-space selection before entering model space.
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.active_viewport = Some(handle);
                self.refresh_properties();
                self.command_line.push_output("MSPACE");
                Task::none()
            }

            Message::ExitViewport => {
                let i = self.active_tab;
                // Clear model-space selection before returning to paper space.
                self.tabs[i].scene.deselect_all();
                self.refresh_properties();
                self.tabs[i].scene.active_viewport = None;
                self.command_line.push_output("PSPACE");
                Task::none()
            }

            Message::MspaceCommand => {
                let i = self.active_tab;
                if self.tabs[i].scene.current_layout == "Model" {
                    self.command_line
                        .push_error("MS is only available in paper space layouts.");
                    return Task::none();
                }
                if self.tabs[i].scene.active_viewport.is_some() {
                    // Already in MSPACE — nothing to do.
                    return Task::none();
                }
                match self.tabs[i].scene.first_user_viewport() {
                    Some(handle) => Task::done(Message::EnterViewport(handle)),
                    None => {
                        self.command_line
                            .push_error("No viewport found in this layout.");
                        Task::none()
                    }
                }
            }

            Message::PspaceCommand => Task::done(Message::ExitViewport),

            Message::Undo => {
                self.undo_active_tab();
                Task::none()
            }
            Message::Redo => {
                self.redo_active_tab();
                Task::none()
            }

            Message::UndoMany(steps) => {
                self.ribbon.close_dropdown();
                self.undo_steps(steps);
                Task::none()
            }

            Message::RedoMany(steps) => {
                self.ribbon.close_dropdown();
                self.redo_steps(steps);
                Task::none()
            }

            Message::Noop => Task::none(),

            // ── Unsaved-changes dialog ────────────────────────────────────
            Message::UnsavedDialogCancel => {
                self.pending_close = None;
                self.close_unsaved_dialog_window()
            }

            Message::UnsavedDialogDiscard => {
                match self.pending_close.take() {
                    Some(super::PendingClose::Tab(idx)) => {
                        let close_win = self.close_unsaved_dialog_window();
                        if self.tabs.len() == 1 {
                            self.tab_counter += 1;
                            self.tabs[0] =
                                super::document::DocumentTab::new_drawing(self.tab_counter);
                            self.active_tab = 0;
                        } else {
                            self.tabs.remove(idx);
                            if self.active_tab >= self.tabs.len() {
                                self.active_tab = self.tabs.len() - 1;
                            }
                        }
                        // The active tab is now a fresh blank or a
                        // different existing tab; sync ribbon chips so
                        // they don't keep showing the discarded tab's
                        // last selection. #21.
                        self.sync_ribbon_layers();
                        self.sync_ribbon_from_selection();
                        return close_win;
                    }
                    Some(super::PendingClose::Quit) => {
                        if let Some(idx) = self.tabs.iter().position(|t| t.dirty) {
                            self.tabs[idx].dirty = false;
                        }
                        if self.tabs.iter().any(|t| t.dirty) {
                            // More dirty tabs remain — keep window open.
                            self.pending_close = Some(super::PendingClose::Quit);
                        } else {
                            let close_win = self.close_unsaved_dialog_window();
                            return Task::batch(vec![close_win, iced::exit()]);
                        }
                    }
                    None => {}
                }
                Task::none()
            }

            Message::UnsavedDialogSave => {
                match self.pending_close.take() {
                    Some(super::PendingClose::Tab(idx)) => {
                        if let Some(path) = self.tabs[idx].current_path.clone() {
                            match crate::io::save(&self.tabs[idx].scene.document, &path) {
                                Ok(()) => {
                                    self.command_line
                                        .push_output(&format!("Saved: {}", path.display()));
                                    self.tabs[idx].dirty = false;
                                    let close_win = self.close_unsaved_dialog_window();
                                    let close_tab = self.update(Message::TabClose(idx));
                                    return Task::batch(vec![close_win, close_tab]);
                                }
                                Err(e) => {
                                    // Keep dialog open for retry.
                                    self.command_line.push_error(&format!("Save failed: {e}"));
                                    self.pending_close = Some(super::PendingClose::Tab(idx));
                                }
                            }
                        } else {
                            // No path — close unsaved dialog, open custom Save As dialog.
                            self.pending_close = Some(super::PendingClose::Tab(idx));
                            self.save_dialog_for_unsaved = true;
                            let close_win = self.close_unsaved_dialog_window();
                            let open_save = self.open_save_dialog_window(idx);
                            return Task::batch([close_win, open_save]);
                        }
                    }
                    Some(super::PendingClose::Quit) => {
                        if let Some(idx) = self.tabs.iter().position(|t| t.dirty) {
                            if let Some(path) = self.tabs[idx].current_path.clone() {
                                match crate::io::save(&self.tabs[idx].scene.document, &path) {
                                    Ok(()) => {
                                        self.command_line
                                            .push_output(&format!("Saved: {}", path.display()));
                                        self.tabs[idx].dirty = false;
                                    }
                                    Err(e) => {
                                        self.command_line.push_error(&format!("Save failed: {e}"));
                                        self.pending_close = Some(super::PendingClose::Quit);
                                        return Task::none();
                                    }
                                }
                            } else {
                                // No path — close unsaved dialog, open custom Save As dialog.
                                self.active_tab = idx;
                                self.pending_close = Some(super::PendingClose::Quit);
                                self.save_dialog_for_unsaved = true;
                                let close_win = self.close_unsaved_dialog_window();
                                let open_save = self.open_save_dialog_window(idx);
                                return Task::batch([close_win, open_save]);
                            }
                        }
                        if self.tabs.iter().any(|t| t.dirty) {
                            // More dirty tabs — keep window open.
                            self.pending_close = Some(super::PendingClose::Quit);
                        } else {
                            let close_win = self.close_unsaved_dialog_window();
                            return Task::batch(vec![close_win, iced::exit()]);
                        }
                    }
                    None => {}
                }
                Task::none()
            }

            Message::UnsavedPickedSavePath(Some(path)) => {
                let (_, version) = crate::io::parse_save_format(&self.save_dialog_format);
                match self.pending_close.take() {
                    Some(super::PendingClose::Tab(idx)) => {
                        match crate::io::save_as_version(
                            &self.tabs[idx].scene.document,
                            &path,
                            version,
                        ) {
                            Ok(()) => {
                                self.command_line
                                    .push_output(&format!("Saved: {}", path.display()));
                                self.tabs[idx].current_path = Some(path);
                                self.tabs[idx].dirty = false;
                                return self.update(Message::TabClose(idx));
                            }
                            Err(e) => {
                                self.command_line.push_error(&format!("Save failed: {e}"));
                                self.pending_close = Some(super::PendingClose::Tab(idx));
                                return self.open_unsaved_dialog_window();
                            }
                        }
                    }
                    Some(super::PendingClose::Quit) => {
                        let i = self.active_tab;
                        match crate::io::save_as_version(
                            &self.tabs[i].scene.document,
                            &path,
                            version,
                        ) {
                            Ok(()) => {
                                self.command_line
                                    .push_output(&format!("Saved: {}", path.display()));
                                self.tabs[i].current_path = Some(path);
                                self.tabs[i].dirty = false;
                                if self.tabs.iter().any(|t| t.dirty) {
                                    self.pending_close = Some(super::PendingClose::Quit);
                                    return self.open_unsaved_dialog_window();
                                } else {
                                    return iced::exit();
                                }
                            }
                            Err(e) => {
                                self.command_line.push_error(&format!("Save failed: {e}"));
                                self.pending_close = Some(super::PendingClose::Quit);
                                return self.open_unsaved_dialog_window();
                            }
                        }
                    }
                    None => {}
                }
                Task::none()
            }

            Message::UnsavedPickedSavePath(None) => {
                // User cancelled the save-as dialog — re-open the confirmation dialog.
                if self.pending_close.is_some() {
                    return self.open_unsaved_dialog_window();
                }
                Task::none()
            }

            // ── Page Setup ────────────────────────────────────────────────
            Message::PageSetupOpen => {
                let i = self.active_tab;
                // Populate edit buffers from current paper limits.
                let (w, h) = if let Some(((x0, y0), (x1, y1))) = self.tabs[i].scene.paper_limits() {
                    (x1 - x0, y1 - y0)
                } else {
                    (297.0, 210.0) // A4 default
                };
                self.page_setup_w = format!("{w:.1}");
                self.page_setup_h = format!("{h:.1}");
                if let Some(id) = self.page_setup_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(520.0, 460.0),
                    resizable: false,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.page_setup_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::PageSetupClose => {
                if let Some(id) = self.page_setup_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::UpdateCheckResult(latest) => {
                let Some(info) = latest else {
                    return Task::none();
                };
                self.update_notice_version = Some(info.version);
                self.update_notice_body = Some(info.body);
                if let Some(id) = self.update_notice_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    // Sized for the new release-notes panel — wide enough
                    // for typical GitHub release headlines, tall enough for
                    // a meaningful scroll preview without dwarfing the app.
                    size: iced::Size::new(560.0, 460.0),
                    resizable: true,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.update_notice_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::UpdateNoticeClose => {
                if let Some(id) = self.update_notice_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::UpdateNoticeOpenRelease => {
                let _ = open::that(crate::update_check::RELEASES_PAGE);
                if let Some(id) = self.update_notice_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::PageSetupWidthEdit(s) => {
                self.page_setup_w = s;
                Task::none()
            }
            Message::PageSetupHeightEdit(s) => {
                self.page_setup_h = s;
                Task::none()
            }
            Message::PageSetupPreset(name) => {
                // Paper size presets defined in view.rs — mirror them here.
                let sizes: &[(&str, f64, f64)] = &[
                    ("A4 Portrait", 210.0, 297.0),
                    ("A4 Landscape", 297.0, 210.0),
                    ("A3 Portrait", 297.0, 420.0),
                    ("A3 Landscape", 420.0, 297.0),
                    ("A2 Portrait", 420.0, 594.0),
                    ("A2 Landscape", 594.0, 420.0),
                    ("A1 Portrait", 594.0, 841.0),
                    ("A1 Landscape", 841.0, 594.0),
                    ("A0 Portrait", 841.0, 1189.0),
                    ("A0 Landscape", 1189.0, 841.0),
                    ("Letter Portrait", 215.9, 279.4),
                    ("Letter Landscape", 279.4, 215.9),
                ];
                if let Some(&(_, w, h)) = sizes.iter().find(|(n, _, _)| *n == name) {
                    self.page_setup_w = format!("{w:.1}");
                    self.page_setup_h = format!("{h:.1}");
                }
                Task::none()
            }
            Message::PageSetupPlotArea(s) => {
                self.page_setup_plot_area = s;
                Task::none()
            }
            Message::PageSetupCenterToggle => {
                self.page_setup_center = !self.page_setup_center;
                Task::none()
            }
            Message::PageSetupOffsetXEdit(s) => {
                self.page_setup_offset_x = s;
                Task::none()
            }
            Message::PageSetupOffsetYEdit(s) => {
                self.page_setup_offset_y = s;
                Task::none()
            }
            Message::PageSetupRotation(s) => {
                self.page_setup_rotation = s;
                Task::none()
            }
            Message::PageSetupScale(s) => {
                self.page_setup_scale = s;
                Task::none()
            }
            Message::PageSetupCommit => {
                let i = self.active_tab;
                let layout_name = self.tabs[i].scene.current_layout.clone();
                if layout_name != "Model" {
                    let w: f64 = self.page_setup_w.parse::<f64>().unwrap_or(297.0).max(1.0);
                    let h: f64 = self.page_setup_h.parse::<f64>().unwrap_or(210.0).max(1.0);
                    let plot_area = self.page_setup_plot_area.clone();
                    let center = self.page_setup_center;
                    let offset_x = self.page_setup_offset_x.parse::<f64>().unwrap_or(0.0);
                    let offset_y = self.page_setup_offset_y.parse::<f64>().unwrap_or(0.0);
                    let rotation: i16 = self.page_setup_rotation.parse().unwrap_or(0);
                    let scale_str = self.page_setup_scale.clone();

                    // Update the Layout object's limits.
                    for obj in self.tabs[i].scene.document.objects.values_mut() {
                        if let acadrust::objects::ObjectType::Layout(l) = obj {
                            if l.name == layout_name {
                                l.min_limits = (0.0, 0.0);
                                l.max_limits = (w, h);
                                l.min_extents = (0.0, 0.0, 0.0);
                                l.max_extents = (w, h, 0.0);
                                break;
                            }
                        }
                    }

                    // Find or create the PlotSettings object for this layout.
                    use acadrust::objects::{
                        ObjectType, PlotPaperUnits, PlotRotation, PlotSettings, PlotType,
                    };
                    let plot_handle =
                        self.tabs[i]
                            .scene
                            .document
                            .objects
                            .iter()
                            .find_map(|(h, obj)| {
                                if let ObjectType::PlotSettings(ps) = obj {
                                    if ps.page_name == layout_name {
                                        Some(*h)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            });

                    let ps_entry = if let Some(h) = plot_handle {
                        self.tabs[i].scene.document.objects.get_mut(&h)
                    } else {
                        // Create a new PlotSettings object and insert it.
                        let mut ps = PlotSettings::new(layout_name.clone());
                        ps.handle =
                            acadrust::Handle::new(self.tabs[i].scene.document.next_handle());
                        let h = ps.handle;
                        self.tabs[i]
                            .scene
                            .document
                            .objects
                            .insert(h, ObjectType::PlotSettings(ps));
                        self.tabs[i].scene.document.objects.get_mut(&h)
                    };

                    if let Some(ObjectType::PlotSettings(ps)) = ps_entry {
                        ps.paper_width = w;
                        ps.paper_height = h;
                        ps.paper_units = PlotPaperUnits::Millimeters;
                        ps.plot_type = if plot_area == "Extents" {
                            PlotType::Extents
                        } else {
                            PlotType::Layout
                        };
                        ps.flags.plot_centered = center;
                        ps.origin_x = offset_x;
                        ps.origin_y = offset_y;
                        ps.rotation = match rotation {
                            90 => PlotRotation::Degrees90,
                            180 => PlotRotation::Degrees180,
                            270 => PlotRotation::Degrees270,
                            _ => PlotRotation::None,
                        };
                        // Apply plot scale.
                        use acadrust::objects::ScaledType;
                        let (num, den) = parse_plot_scale(&scale_str);
                        if scale_str == "Fit" {
                            ps.set_scale_to_fit();
                        } else {
                            ps.scale_type = ScaledType::CustomScale;
                            ps.scale_numerator = num;
                            ps.scale_denominator = den;
                        }
                    }

                    self.tabs[i].dirty = true;
                    self.command_line.push_info(&format!(
                        "Page setup: {w:.1}×{h:.1} mm  area={plot_area}  \
                         center={center}  rot={rotation}°"
                    ));
                }
                if let Some(id) = self.page_setup_window.take() {
                    return window::close(id);
                }
                Task::none()
            }

            // ── Plot / Export ─────────────────────────────────────────────
            Message::PlotExport => {
                let i = self.active_tab;
                let stem = self.tabs[i]
                    .current_path
                    .as_deref()
                    .and_then(|p: &std::path::Path| p.file_stem())
                    .map(|s: &std::ffi::OsStr| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".into());
                Task::perform(
                    crate::io::pdf_export::pick_pdf_path_owned(stem),
                    Message::PlotExportPath,
                )
            }
            Message::PlotExportPath(None) => Task::none(),
            Message::PlotExportPath(Some(path)) => {
                let i = self.active_tab;
                let scene = &self.tabs[i].scene;
                let layout_name = scene.current_layout.clone();
                let wires = scene.entity_wires();
                let hatches = scene.paper_canvas_hatches();
                let wipeouts = scene.paper_canvas_wipeouts();

                // Read PlotSettings for current layout (if available).
                use acadrust::objects::{ObjectType, PlotType};
                let ps_snap = scene.document.objects.values().find_map(|obj| {
                    if let ObjectType::PlotSettings(ps) = obj {
                        if ps.page_name == layout_name {
                            Some(ps.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                // Determine paper size and drawing offset.
                let (paper_w, paper_h, mut draw_ox, mut draw_oy, rotation_deg) =
                    if let Some(((x0, y0), (x1, y1))) = scene.paper_limits() {
                        let (pw, ph) = (x1 - x0, y1 - y0);

                        // If PlotSettings says Extents, use model space extents instead.
                        let use_extents = ps_snap
                            .as_ref()
                            .map(|ps| matches!(ps.plot_type, PlotType::Extents))
                            .unwrap_or(false);

                        let (ox, oy) = if use_extents {
                            if let Some((mn, _mx)) = scene.model_space_extents() {
                                (-mn.x as f64, -mn.y as f64)
                            } else {
                                (-x0, -y0)
                            }
                        } else {
                            (-x0, -y0)
                        };

                        let rot = ps_snap
                            .as_ref()
                            .map(|ps| ps.rotation.to_degrees() as i32)
                            .unwrap_or(0);

                        (pw, ph, ox, oy, rot)
                    } else {
                        // Model space: fit with 5% margin.
                        let margin = 1.05_f64;
                        if let Some((mn, mx)) = scene.model_space_extents() {
                            let w = ((mx.x - mn.x) as f64 * margin).max(1.0);
                            let h = ((mx.y - mn.y) as f64 * margin).max(1.0);
                            let pad_x = (w - (mx.x - mn.x) as f64) * 0.5;
                            let pad_y = (h - (mx.y - mn.y) as f64) * 0.5;
                            (w, h, -(mn.x as f64) + pad_x, -(mn.y as f64) + pad_y, 0)
                        } else {
                            (297.0, 210.0, 0.0, 0.0, 0)
                        }
                    };

                // Apply PlotSettings offset and centering.
                if let Some(ref ps) = ps_snap {
                    if ps.flags.plot_centered {
                        // Centering: compute wire extents and re-centre.
                        let all_x: Vec<f32> = wires
                            .iter()
                            .flat_map(|w| w.points.iter().map(|p| p[0]))
                            .filter(|v| !v.is_nan())
                            .collect();
                        let all_y: Vec<f32> = wires
                            .iter()
                            .flat_map(|w| w.points.iter().map(|p| p[1]))
                            .filter(|v| !v.is_nan())
                            .collect();
                        if let (Some(&min_x), Some(&max_x), Some(&min_y), Some(&max_y)) = (
                            all_x.iter().copied().reduce(f32::min).as_ref(),
                            all_x.iter().copied().reduce(f32::max).as_ref(),
                            all_y.iter().copied().reduce(f32::min).as_ref(),
                            all_y.iter().copied().reduce(f32::max).as_ref(),
                        ) {
                            let cx = (min_x + max_x) as f64 / 2.0;
                            let cy = (min_y + max_y) as f64 / 2.0;
                            draw_ox += paper_w / 2.0 - cx;
                            draw_oy += paper_h / 2.0 - cy;
                        }
                    } else {
                        draw_ox += ps.origin_x;
                        draw_oy += ps.origin_y;
                    }
                }

                // For rotation: swap paper dimensions and note angle for export.
                let (eff_w, eff_h) = match rotation_deg {
                    90 | 270 => (paper_h, paper_w),
                    _ => (paper_w, paper_h),
                };

                match crate::io::pdf_export::export_pdf(
                    &wires,
                    hatches.as_slice(),
                    wipeouts.as_slice(),
                    eff_w,
                    eff_h,
                    draw_ox as f32,
                    draw_oy as f32,
                    rotation_deg,
                    &path,
                    self.active_plot_style.as_ref(),
                ) {
                    Ok(()) => self.command_line.push_info(&format!(
                        "Exported: {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )),
                    Err(e) => self.command_line.push_error(&format!("Export failed: {e}")),
                }
                Task::none()
            }

            // ── Print to system printer ───────────────────────────────────────
            Message::PrintToPrinter => {
                let i = self.active_tab;
                let scene = &self.tabs[i].scene;
                let layout_name = scene.current_layout.clone();
                let wires = scene.entity_wires();
                let hatches: Vec<_> = scene.paper_canvas_hatches().as_ref().clone();
                let wipeouts: Vec<_> = scene.paper_canvas_wipeouts().as_ref().clone();
                use acadrust::objects::{ObjectType, PlotType};
                let ps_snap = scene.document.objects.values().find_map(|obj| {
                    if let ObjectType::PlotSettings(ps) = obj {
                        if ps.page_name == layout_name {
                            Some(ps.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });
                let (paper_w, paper_h, draw_ox, draw_oy, rotation_deg) =
                    if let Some(((x0, y0), (x1, y1))) = scene.paper_limits() {
                        let (pw, ph) = (x1 - x0, y1 - y0);
                        let use_extents = ps_snap
                            .as_ref()
                            .map(|ps| matches!(ps.plot_type, PlotType::Extents))
                            .unwrap_or(false);
                        let (ox, oy) = if use_extents {
                            if let Some((mn, _mx)) = scene.model_space_extents() {
                                (-mn.x as f64, -mn.y as f64)
                            } else {
                                (-x0, -y0)
                            }
                        } else {
                            (-x0, -y0)
                        };
                        let rot = ps_snap
                            .as_ref()
                            .map(|ps| ps.rotation.to_degrees() as i32)
                            .unwrap_or(0);
                        (pw, ph, ox, oy, rot)
                    } else {
                        if let Some((mn, mx)) = scene.model_space_extents() {
                            let margin = 1.05_f64;
                            let w = ((mx.x - mn.x) as f64 * margin).max(1.0);
                            let h = ((mx.y - mn.y) as f64 * margin).max(1.0);
                            let pad_x = (w - (mx.x - mn.x) as f64) * 0.5;
                            let pad_y = (h - (mx.y - mn.y) as f64) * 0.5;
                            (w, h, -(mn.x as f64) + pad_x, -(mn.y as f64) + pad_y, 0)
                        } else {
                            (297.0, 210.0, 0.0, 0.0, 0)
                        }
                    };
                let (eff_w, eff_h) = match rotation_deg {
                    90 | 270 => (paper_h, paper_w),
                    _ => (paper_w, paper_h),
                };
                let plot_style = self.active_plot_style.clone();
                self.command_line.push_info("Sending to system printer…");
                Task::perform(
                    async move {
                        crate::io::print_to_printer::print_wires(
                            wires,
                            hatches,
                            wipeouts,
                            eff_w,
                            eff_h,
                            draw_ox as f32,
                            draw_oy as f32,
                            rotation_deg,
                            plot_style,
                        )
                        .await
                    },
                    Message::PrintResult,
                )
            }
            Message::PrintResult(Ok(printer)) => {
                self.command_line
                    .push_info(&format!("Sent to printer: {printer}"));
                Task::none()
            }
            Message::PrintResult(Err(e)) => {
                self.command_line.push_error(&format!("Print failed: {e}"));
                Task::none()
            }

            // ── Plot Style Table ──────────────────────────────────────────────
            Message::PlotStyleLoad => {
                Task::perform(crate::io::pick_plot_style(), Message::PlotStyleLoaded)
            }
            Message::PlotStyleLoaded(Some(table)) => {
                self.command_line.push_output(&format!(
                    "Plot style '{}' loaded ({} color entries).",
                    table.name,
                    table
                        .aci_entries
                        .iter()
                        .filter(|e| e.color.is_some())
                        .count()
                ));
                self.active_plot_style = Some(table);
                Task::none()
            }
            Message::PlotStyleLoaded(None) => Task::none(),
            Message::PlotStyleClear => {
                self.active_plot_style = None;
                self.command_line.push_output("Plot style table cleared.");
                Task::none()
            }

            // ── Plot Style Panel ──────────────────────────────────────────────
            Message::PlotStylePanelOpen => {
                // Initialise edit buffers for ACI 1.
                self.plotstyle_panel_aci = 1;
                let entry = self
                    .active_plot_style
                    .as_ref()
                    .and_then(|t| t.aci_entries.get(1));
                self.ps_color_buf = entry
                    .and_then(|e| {
                        e.color
                            .map(|[r, g, b]| format!("#{:02X}{:02X}{:02X}", r, g, b))
                    })
                    .unwrap_or_default();
                self.ps_lineweight_buf = entry
                    .map(|e| e.lineweight.to_string())
                    .unwrap_or("255".into());
                self.ps_screening_buf = entry
                    .map(|e| e.screening.to_string())
                    .unwrap_or("100".into());
                if let Some(id) = self.plotstyle_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(780.0, 540.0),
                    resizable: true,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.plotstyle_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::PlotStylePanelClose => {
                if let Some(id) = self.plotstyle_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::PlotStylePanelSelectAci(aci) => {
                self.plotstyle_panel_aci = aci;
                let entry = self
                    .active_plot_style
                    .as_ref()
                    .and_then(|t| t.aci_entries.get(aci as usize));
                self.ps_color_buf = entry
                    .and_then(|e| {
                        e.color
                            .map(|[r, g, b]| format!("#{:02X}{:02X}{:02X}", r, g, b))
                    })
                    .unwrap_or_default();
                self.ps_lineweight_buf = entry
                    .map(|e| e.lineweight.to_string())
                    .unwrap_or("255".into());
                self.ps_screening_buf = entry
                    .map(|e| e.screening.to_string())
                    .unwrap_or("100".into());
                Task::none()
            }
            Message::PlotStylePanelColorBuf(s) => {
                self.ps_color_buf = s;
                Task::none()
            }
            Message::PlotStylePanelLwBuf(s) => {
                self.ps_lineweight_buf = s;
                Task::none()
            }
            Message::PlotStylePanelScreenBuf(s) => {
                self.ps_screening_buf = s;
                Task::none()
            }

            Message::PlotStylePanelApply => {
                let aci = self.plotstyle_panel_aci as usize;
                if let Some(table) = self.active_plot_style.as_mut() {
                    if let Some(entry) = table.aci_entries.get_mut(aci) {
                        // Parse color.
                        let color_str = self.ps_color_buf.trim();
                        if color_str.is_empty() {
                            entry.color = None;
                        } else if color_str.starts_with('#') && color_str.len() == 7 {
                            let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(0);
                            let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(0);
                            let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(0);
                            entry.color = Some([r, g, b]);
                        }
                        if let Ok(lw) = self.ps_lineweight_buf.trim().parse::<u8>() {
                            entry.lineweight = lw;
                        }
                        if let Ok(sc) = self.ps_screening_buf.trim().parse::<u8>() {
                            entry.screening = sc.min(100);
                        }
                        self.command_line
                            .push_output(&format!("Plot style ACI {aci} updated."));
                    }
                } else {
                    // No table loaded: create an identity table and apply.
                    let mut table = crate::io::plot_style::PlotStyleTable::identity("Custom.ctb");
                    if let Some(entry) = table.aci_entries.get_mut(aci) {
                        let color_str = self.ps_color_buf.trim();
                        if color_str.starts_with('#') && color_str.len() == 7 {
                            let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(0);
                            let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(0);
                            let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(0);
                            entry.color = Some([r, g, b]);
                        }
                        if let Ok(lw) = self.ps_lineweight_buf.trim().parse::<u8>() {
                            entry.lineweight = lw;
                        }
                        if let Ok(sc) = self.ps_screening_buf.trim().parse::<u8>() {
                            entry.screening = sc.min(100);
                        }
                    }
                    self.active_plot_style = Some(table);
                    self.command_line
                        .push_output(&format!("Created new CTB table, ACI {aci} updated."));
                }
                Task::none()
            }

            Message::PlotStylePanelSave => {
                if self.active_plot_style.is_none() {
                    self.command_line
                        .push_error("No plot style table loaded. Load or create one first.");
                    return Task::none();
                }
                let default_name = self
                    .active_plot_style
                    .as_ref()
                    .map(|t| t.name.clone())
                    .unwrap_or("export.ctb".into());
                Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Save Plot Style Table")
                            .set_file_name(&default_name)
                            .add_filter("Plot Style Files", &["ctb", "stb", "CTB", "STB"])
                            .add_filter("All Files", &["*"])
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::PlotStylePanelSavePath,
                )
            }

            Message::PlotStylePanelSavePath(Some(path)) => {
                if let Some(table) = &self.active_plot_style {
                    match table.save(&path) {
                        Ok(()) => self.command_line.push_output(&format!(
                            "Plot style table saved to \"{}\".",
                            path.display()
                        )),
                        Err(e) => self.command_line.push_error(&format!("Save error: {e}")),
                    }
                }
                Task::none()
            }
            Message::PlotStylePanelSavePath(None) => Task::none(),

            // ── TextStyle Font Browser ────────────────────────────────────────
            Message::TextStyleDialogOpen => {
                let i = self.active_tab;
                let cur = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_text_style_name
                    .clone();
                let exists = self.tabs[i].scene.document.text_styles.get(&cur).is_some();
                self.textstyle_selected = if exists {
                    cur
                } else {
                    self.tabs[i]
                        .scene
                        .document
                        .text_styles
                        .iter()
                        .next()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "Standard".to_string())
                };
                self.load_textstyle_bufs(i);
                if let Some(id) = self.textstyle_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(620.0, 460.0),
                    resizable: true,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.textstyle_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::TextStyleDialogClose => {
                if let Some(id) = self.textstyle_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::TextStyleDialogSelect(name) => {
                let i = self.active_tab;
                self.textstyle_selected = name;
                self.load_textstyle_bufs(i);
                Task::none()
            }
            Message::TextStyleDialogSetCurrent => {
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                if self.tabs[i].scene.document.text_styles.get(&name).is_some() {
                    self.push_undo_snapshot(i, "STYLE SET");
                    self.tabs[i].scene.document.header.current_text_style_name = name.clone();
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(&format!("Current text style: {}", name));
                }
                Task::none()
            }
            Message::TextStyleDialogNew => {
                let i = self.active_tab;
                let doc = &self.tabs[i].scene.document;
                let mut n = 1u32;
                let new_name = loop {
                    let candidate = format!("Style{}", n);
                    if !doc.text_styles.contains(&candidate) {
                        break candidate;
                    }
                    n += 1;
                };
                self.push_undo_snapshot(i, "STYLE NEW");
                let style = acadrust::tables::TextStyle::new(&new_name);
                let _ = self.tabs[i].scene.document.text_styles.add(style);
                self.textstyle_selected = new_name.clone();
                self.textstyle_font = String::new();
                self.textstyle_width = "1.0".to_string();
                self.textstyle_oblique = "0.0".to_string();
                self.tabs[i].dirty = true;
                Task::none()
            }
            Message::TextStyleDialogDelete => {
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                if name.eq_ignore_ascii_case("Standard") {
                    self.command_line
                        .push_error("Cannot delete the Standard text style.");
                    return Task::none();
                }
                self.push_undo_snapshot(i, "STYLE DEL");
                self.tabs[i].scene.document.text_styles.remove(&name);
                self.textstyle_selected = self.tabs[i]
                    .scene
                    .document
                    .text_styles
                    .iter()
                    .next()
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "Standard".to_string());
                self.load_textstyle_bufs(i);
                self.tabs[i].dirty = true;
                Task::none()
            }
            Message::TextStyleEdit { field, value } => {
                match field {
                    "font" => self.textstyle_font = value,
                    "width" => self.textstyle_width = value,
                    "oblique" => self.textstyle_oblique = value,
                    _ => {}
                }
                Task::none()
            }
            Message::TextStyleApply => {
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                if self.tabs[i].scene.document.text_styles.get(&name).is_some() {
                    self.push_undo_snapshot(i, "STYLE EDIT");
                    let font = self.textstyle_font.clone();
                    let width_str = self.textstyle_width.clone();
                    let oblique_str = self.textstyle_oblique.clone();
                    if let Some(s) = self.tabs[i].scene.document.text_styles.get_mut(&name) {
                        s.font_file = font;
                        if let Ok(w) = width_str.trim().parse::<f64>() {
                            s.width_factor = w;
                        }
                        if let Ok(a) = oblique_str.trim().parse::<f64>() {
                            s.oblique_angle = a.to_radians();
                        }
                    }
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }
            Message::TextStyleFontPick(font_file) => {
                let i = self.active_tab;
                self.textstyle_font = font_file.clone();
                let name = self.textstyle_selected.clone();
                if self.tabs[i].scene.document.text_styles.get(&name).is_some() {
                    self.push_undo_snapshot(i, "STYLE FONT");
                    if let Some(s) = self.tabs[i].scene.document.text_styles.get_mut(&name) {
                        s.font_file = font_file;
                    }
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }

            // ── TableStyle Dialog ─────────────────────────────────────────────
            Message::TableStyleDialogOpen => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                self.tablestyle_selected = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .find_map(|o| {
                        if let ObjectType::TableStyle(s) = o {
                            Some(s.name.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "Standard".to_string());
                if let Some(id) = self.tablestyle_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(620.0, 420.0),
                    resizable: true,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.tablestyle_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::TableStyleDialogClose => {
                if let Some(id) = self.tablestyle_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::TableStyleDialogSelect(name) => {
                self.tablestyle_selected = name;
                Task::none()
            }

            Message::TableStyleDialogNew => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let doc = &self.tabs[i].scene.document;
                let mut n = 1u32;
                let new_name = loop {
                    let candidate = format!("TS{}", n);
                    let taken = doc.objects.values().any(|o| {
                        matches!(o, ObjectType::TableStyle(s) if s.name.eq_ignore_ascii_case(&candidate))
                    });
                    if !taken {
                        break candidate;
                    }
                    n += 1;
                };
                self.push_undo_snapshot(i, "TABLESTYLE NEW");
                let mut style = acadrust::objects::TableStyle::standard();
                style.name = new_name.clone();
                let nh = acadrust::Handle::new(self.tabs[i].scene.document.next_handle());
                style.handle = nh;
                self.tabs[i]
                    .scene
                    .document
                    .objects
                    .insert(nh, ObjectType::TableStyle(style));
                self.tablestyle_selected = new_name;
                self.tabs[i].dirty = true;
                Task::none()
            }
            Message::TableStyleDialogDelete => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                if name.eq_ignore_ascii_case("Standard") {
                    self.command_line
                        .push_error("Cannot delete the Standard style.");
                    return Task::none();
                }
                let handle = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .iter()
                    .find_map(|(&h, o)| {
                        if let ObjectType::TableStyle(s) = o {
                            if s.name == name {
                                Some(h)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });
                if let Some(h) = handle {
                    self.push_undo_snapshot(i, "TABLESTYLE DEL");
                    self.tabs[i].scene.document.objects.remove(&h);
                    self.tablestyle_selected = self.tabs[i]
                        .scene
                        .document
                        .objects
                        .values()
                        .find_map(|o| {
                            if let ObjectType::TableStyle(s) = o {
                                Some(s.name.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "Standard".to_string());
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }

            // ── MLineStyle Dialog ─────────────────────────────────────────────
            Message::MlStyleDialogOpen => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let cur = self.tabs[i].scene.document.header.multiline_style.clone();
                let exists = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .any(|o| matches!(o, ObjectType::MLineStyle(s) if s.name == cur));
                self.mlstyle_selected = if exists {
                    cur
                } else {
                    self.tabs[i]
                        .scene
                        .document
                        .objects
                        .values()
                        .find_map(|o| {
                            if let ObjectType::MLineStyle(s) = o {
                                Some(s.name.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "Standard".to_string())
                };
                if let Some(id) = self.mlstyle_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(620.0, 420.0),
                    resizable: true,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.mlstyle_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::MlStyleDialogClose => {
                if let Some(id) = self.mlstyle_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::MlStyleDialogSelect(name) => {
                self.mlstyle_selected = name;
                Task::none()
            }
            Message::MlStyleDialogSetCurrent => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.mlstyle_selected.clone();
                let exists = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .any(|o| matches!(o, ObjectType::MLineStyle(s) if s.name == name));
                if exists {
                    self.push_undo_snapshot(i, "MLSTYLE SET");
                    self.tabs[i].scene.document.header.multiline_style = name.clone();
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(&format!("Current multiline style: {}", name));
                }
                Task::none()
            }
            Message::MlStyleDialogNew => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                // Generate a unique name.
                let doc = &self.tabs[i].scene.document;
                let mut n = 1u32;
                let base = "MLS";
                let new_name = loop {
                    let candidate = format!("{}{}", base, n);
                    let taken = doc.objects.values().any(|o| {
                        matches!(o, ObjectType::MLineStyle(s) if s.name.eq_ignore_ascii_case(&candidate))
                    });
                    if !taken {
                        break candidate;
                    }
                    n += 1;
                };
                self.push_undo_snapshot(i, "MLSTYLE NEW");
                let mut style = acadrust::objects::MLineStyle::standard();
                style.name = new_name.clone();
                let nh = acadrust::Handle::new(self.tabs[i].scene.document.next_handle());
                style.handle = nh;
                self.tabs[i]
                    .scene
                    .document
                    .objects
                    .insert(nh, ObjectType::MLineStyle(style));
                self.mlstyle_selected = new_name;
                self.tabs[i].dirty = true;
                Task::none()
            }
            Message::MlStyleDialogDelete => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.mlstyle_selected.clone();
                if name.eq_ignore_ascii_case("Standard") {
                    self.command_line
                        .push_error("Cannot delete the Standard style.");
                    return Task::none();
                }
                let handle = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .iter()
                    .find_map(|(&h, o)| {
                        if let ObjectType::MLineStyle(s) = o {
                            if s.name == name {
                                Some(h)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });
                if let Some(h) = handle {
                    self.push_undo_snapshot(i, "MLSTYLE DEL");
                    self.tabs[i].scene.document.objects.remove(&h);
                    // Select first remaining style.
                    self.mlstyle_selected = self.tabs[i]
                        .scene
                        .document
                        .objects
                        .values()
                        .find_map(|o| {
                            if let ObjectType::MLineStyle(s) = o {
                                Some(s.name.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "Standard".to_string());
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }

            // ── DimStyle Dialog ───────────────────────────────────────────────
            Message::DimStyleDialogOpen => {
                let i = self.active_tab;
                // Pick the document's current dim style or "Standard".
                let cur = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_dimstyle_name
                    .clone();
                let selected = if self.tabs[i].scene.document.dim_styles.get(&cur).is_some() {
                    cur
                } else {
                    self.tabs[i]
                        .scene
                        .document
                        .dim_styles
                        .iter()
                        .next()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "Standard".to_string())
                };
                self.dimstyle_selected = selected.clone();
                self.load_dimstyle_bufs(i);
                if let Some(id) = self.dimstyle_window {
                    return window::gain_focus(id);
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(720.0, 560.0),
                    resizable: true,
                    level: window::Level::AlwaysOnTop,
                    ..Default::default()
                });
                self.dimstyle_window = Some(id);
                task.map(|_| Message::Noop)
            }
            Message::DimStyleDialogClose => {
                if let Some(id) = self.dimstyle_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::DimStyleDialogApply => {
                let i = self.active_tab;
                self.apply_dimstyle_bufs(i);
                Task::none()
            }
            Message::DimStyleDialogSelect(name) => {
                let i = self.active_tab;
                self.dimstyle_selected = name;
                self.load_dimstyle_bufs(i);
                Task::none()
            }
            Message::DimStyleDialogTab(tab) => {
                self.dimstyle_tab = tab;
                Task::none()
            }
            Message::DimStyleDialogNew => {
                // Delegate to the DIMSTYLE NEW command via command line prompt.
                self.command_line.push_info("Enter new DimStyle name:");
                if let Some(id) = self.dimstyle_window.take() {
                    return window::close(id);
                }
                Task::none()
            }
            Message::DimStyleDialogSetCurrent => {
                let i = self.active_tab;
                self.push_undo_snapshot(i, "DIMSTYLE SETCURRENT");
                self.tabs[i].scene.document.header.current_dimstyle_name =
                    self.dimstyle_selected.clone();
                self.tabs[i].dirty = true;
                self.command_line.push_output(&format!(
                    "Current dim style set to '{}'.",
                    self.dimstyle_selected
                ));
                Task::none()
            }
            Message::DimStyleDialogDelete => {
                let i = self.active_tab;
                let name = self.dimstyle_selected.clone();
                if name == "Standard" {
                    self.command_line
                        .push_error("Cannot delete the Standard dim style.");
                } else if self.tabs[i]
                    .scene
                    .document
                    .dim_styles
                    .remove(&name)
                    .is_some()
                {
                    self.tabs[i].dirty = true;
                    // Select first remaining style.
                    self.dimstyle_selected = self.tabs[i]
                        .scene
                        .document
                        .dim_styles
                        .iter()
                        .next()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "Standard".to_string());
                    self.load_dimstyle_bufs(i);
                    self.command_line
                        .push_output(&format!("DimStyle '{}' deleted.", name));
                }
                Task::none()
            }
            Message::DsEdit(field, val) => {
                self.apply_ds_edit(field, val);
                Task::none()
            }
            Message::DsToggle(field) => {
                self.apply_ds_toggle(field);
                Task::none()
            }
        }
    }
}

// ── DimStyle dialog helpers ─────────────────────────────────────────────────

impl OpenCADStudio {
    /// Rebuild the active tab's dynamic-input field set to match what the
    /// command is currently asking for. Called on cursor move and after
    /// command-state changes. The field set only changes shape when the
    /// command's `dyn_field()` or the presence of a base point changes;
    /// existing typed buffers survive an unchanged shape.
    fn sync_dyn_fields(&mut self) {
        use super::document::{DynComponent, DynFieldEntry};
        let i = self.active_tab;
        if !self.dyn_input || self.tabs[i].active_cmd.is_none() {
            self.tabs[i].dyn_fields.clear();
            self.tabs[i].dyn_active = 0;
            return;
        }
        let field = self
            .tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.dyn_field())
            .unwrap_or(crate::command::DynField::Point);
        let has_base = self.last_point.is_some();
        let default: Vec<DynComponent> = match field {
            crate::command::DynField::Distance => vec![DynComponent::Distance],
            crate::command::DynField::Angle => vec![DynComponent::Angle],
            crate::command::DynField::Point if has_base => {
                vec![DynComponent::Distance, DynComponent::Angle]
            }
            crate::command::DynField::Point => vec![DynComponent::X, DynComponent::Y],
        };
        // Multiple shapes can satisfy the same command request — e.g. a
        // `Point` is happy with either `[Distance, Angle]` (polar) or
        // `[X, Y]` / `[X, Y, Z]` (cartesian). If the user already
        // reshaped via `,` (see #35) the existing set is still a valid
        // Point configuration and must not be reverted on every mouse
        // move.
        let current: Vec<DynComponent> =
            self.tabs[i].dyn_fields.iter().map(|f| f.component).collect();
        // Only treat a cartesian / polar variant as "good enough to keep"
        // when the user explicitly reshaped via `,`. Otherwise we follow
        // the command's default so e.g. clicking the first point of LINE
        // flips a stale `[X, Y]` (from before there was a base) over to
        // the polar `[Distance, Angle]` the prompt actually wants.
        let current_is_acceptable = if self.dyn_user_reshaped {
            match field {
                crate::command::DynField::Distance => {
                    matches!(current.as_slice(), [DynComponent::Distance])
                }
                crate::command::DynField::Angle => {
                    matches!(current.as_slice(), [DynComponent::Angle])
                }
                crate::command::DynField::Point => matches!(
                    current.as_slice(),
                    [DynComponent::Distance, DynComponent::Angle]
                        | [DynComponent::X, DynComponent::Y]
                        | [DynComponent::X, DynComponent::Y, DynComponent::Z]
                ),
            }
        } else {
            current == default
        };
        if !current_is_acceptable {
            self.tabs[i].dyn_fields = default.into_iter().map(DynFieldEntry::new).collect();
            self.tabs[i].dyn_active = 0;
        }
    }

    /// Track cursor dwell over a selected entity's grip. Sets
    /// `grip_hover` while the cursor sits within `GRIP_THRESHOLD_PX` of
    /// a grip and opens `grip_popup` once the dwell exceeds the
    /// threshold. Cursor drift clears both.
    fn update_grip_hover(&mut self, i: usize, p: iced::Point) {
        const HOVER_OPEN_MS: u128 = 600;
        const POPUP_DISMISS_PX: f32 = 80.0;
        if self.tabs[i].active_cmd.is_some()
            || self.tabs[i].active_grip.is_some()
            || self.tabs[i].selected_grips.is_empty()
        {
            self.grip_hover = None;
            self.grip_popup = None;
            return;
        }
        let Some(handle) = self.tabs[i].selected_handle else {
            self.grip_hover = None;
            self.grip_popup = None;
            return;
        };
        let (vw, vh) = self.tabs[i].scene.selection.borrow().vp_size;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: vw,
            height: vh,
        };
        let is_paper = self.tabs[i].scene.current_layout != "Model";
        let hit = if is_paper {
            let cam = self.tabs[i].scene.camera.borrow();
            let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
            let half_h = cam.ortho_size();
            let half_w = half_h * aspect;
            let tx = cam.target.x;
            let ty = cam.target.y;
            drop(cam);
            find_hit_grip_paper(
                p,
                &self.tabs[i].selected_grips,
                tx,
                ty,
                half_w,
                half_h,
                bounds,
            )
        } else {
            let vp_mat = self.tabs[i].scene.camera.borrow().view_proj(bounds);
            find_hit_grip(p, &self.tabs[i].selected_grips, vp_mat, bounds)
        };
        match hit {
            Some((grip_id, _, _)) => {
                let same = self
                    .grip_hover
                    .as_ref()
                    .map_or(false, |h| h.handle == handle && h.grip_id == grip_id);
                if !same {
                    self.grip_hover = Some(super::GripHover {
                        handle,
                        grip_id,
                        screen: p,
                        started: std::time::Instant::now(),
                    });
                    self.grip_popup = None;
                } else if let Some(h) = self.grip_hover.as_mut() {
                    h.screen = p;
                }
                // Open popup once dwell crosses the threshold.
                if self.grip_popup.is_none()
                    && self
                        .grip_hover
                        .as_ref()
                        .map_or(false, |h| h.started.elapsed().as_millis() >= HOVER_OPEN_MS)
                {
                    let entity_opt = self.tabs[i].scene.document.get_entity(handle);
                    if let Some(e) = entity_opt {
                        use crate::entities::traits::EntityTypeOps;
                        let items = e.grip_menu(grip_id);
                        if !items.is_empty() {
                            self.grip_popup = Some(super::GripPopup {
                                handle,
                                grip_id,
                                anchor: p,
                                items,
                                selected: 0,
                            });
                        }
                    }
                }
            }
            None => {
                self.grip_hover = None;
                if let Some(popup) = &self.grip_popup {
                    let dx = p.x - popup.anchor.x;
                    let dy = p.y - popup.anchor.y;
                    if (dx * dx + dy * dy).sqrt() > POPUP_DISMISS_PX {
                        self.grip_popup = None;
                    }
                }
            }
        }
    }

    /// Re-run the active command's preview hook against the current
    /// cursor world position. Keyboard-driven point commits (typed
    /// coordinates in the command line or dynamic input) don't fire a
    /// mouse-move event, so without this the rubber-band preview keeps
    /// dangling from the previous `last_point` until the user actually
    /// moves the mouse. See #32.
    fn refresh_active_cmd_preview(&mut self, i: usize) {
        if self.tabs[i].active_cmd.is_none() {
            return;
        }
        let cur = self.tabs[i].last_cursor_world;
        let previews = self.tabs[i]
            .active_cmd
            .as_mut()
            .map(|c| c.on_preview_wires(cur))
            .unwrap_or_default();
        self.tabs[i].scene.set_preview_wires(previews);
    }

    /// Resolve the world point implied by the current dynamic-input field
    /// values. Locked fields use their typed buffer; the rest fall back to
    /// the live cursor-derived value. Returns `None` when the field set
    /// isn't one we know how to turn into a point.
    fn dyn_resolve_point(&self) -> Option<glam::Vec3> {
        use super::document::DynComponent;
        let i = self.active_tab;
        let fields = &self.tabs[i].dyn_fields;
        if fields.is_empty() {
            return None;
        }
        let w = self.tabs[i].last_cursor_world;
        let base = self.last_point.unwrap_or(glam::Vec3::ZERO);
        // Buffer value parsed as f32, or the supplied live value.
        let val = |idx: usize, live: f32| -> f32 {
            fields[idx]
                .buffer
                .as_ref()
                .and_then(|s| s.trim().replace(',', ".").parse::<f32>().ok())
                .unwrap_or(live)
        };
        let dx = w.x - base.x;
        let dy = w.y - base.y;
        let live_d = (dx * dx + dy * dy).sqrt();
        let live_a = dy.atan2(dx); // radians
        let comps: Vec<DynComponent> = fields.iter().map(|f| f.component).collect();
        // DYN-on defaults to RELATIVE coordinates when a base point is set
        // (see #26 / #35). The live cartesian fallback is the cursor
        // position relative to base; typed values are relative deltas.
        let has_base = self.last_point.is_some();
        match comps.as_slice() {
            [DynComponent::X, DynComponent::Y] if has_base => {
                Some(glam::Vec3::new(base.x + val(0, dx), base.y + val(1, dy), base.z))
            }
            [DynComponent::X, DynComponent::Y] => {
                Some(glam::Vec3::new(val(0, w.x), val(1, w.y), base.z))
            }
            [DynComponent::X, DynComponent::Y, DynComponent::Z] if has_base => {
                Some(glam::Vec3::new(
                    base.x + val(0, dx),
                    base.y + val(1, dy),
                    base.z + val(2, 0.0),
                ))
            }
            [DynComponent::X, DynComponent::Y, DynComponent::Z] => Some(glam::Vec3::new(
                val(0, w.x),
                val(1, w.y),
                val(2, base.z),
            )),
            [DynComponent::Distance, DynComponent::Angle] => {
                let d = val(0, live_d);
                let a = val(1, live_a.to_degrees()).to_radians();
                Some(glam::Vec3::new(base.x + d * a.cos(), base.y + d * a.sin(), base.z))
            }
            [DynComponent::Distance] => {
                // Keep the cursor's direction, override the magnitude.
                let dir = glam::Vec3::new(dx, dy, 0.0).normalize_or(glam::Vec3::X);
                Some(base + dir * val(0, live_d))
            }
            [DynComponent::Angle] => {
                // Keep the cursor's distance, override the angle.
                let a = val(0, live_a.to_degrees()).to_radians();
                Some(glam::Vec3::new(
                    base.x + live_d * a.cos(),
                    base.y + live_d * a.sin(),
                    base.z,
                ))
            }
            _ => None,
        }
    }

    /// Handle `,` while a dynamic-input field set is showing. Locks the
    /// current field's buffer if it has one, then either advances within
    /// the existing field set or reshapes it: a polar `[Distance, Angle]`
    /// configuration becomes cartesian `[X(buf), Y]`, and a cartesian
    /// `[X, Y]` configuration extends to `[X, Y, Z]`. Default fallthrough
    /// is "advance to next field", matching `Tab`. See #35.
    fn dyn_comma_advance(&mut self) {
        use super::document::{DynComponent, DynFieldEntry};
        let i = self.active_tab;
        if self.tabs[i].dyn_fields.is_empty() {
            return;
        }
        // The user picked a shape — `sync_dyn_fields` preserves it until
        // the next commit / command-start clears the flag.
        self.dyn_user_reshaped = true;
        let active = self
            .tabs[i]
            .dyn_active
            .min(self.tabs[i].dyn_fields.len() - 1);
        let comps: Vec<DynComponent> = self
            .tabs[i]
            .dyn_fields
            .iter()
            .map(|f| f.component)
            .collect();
        let cur_buf = self.tabs[i].dyn_fields[active].buffer.clone();
        match (comps.as_slice(), active) {
            // First polar field — `,` switches to cartesian, locking the
            // typed value as X.
            ([DynComponent::Distance, DynComponent::Angle], 0)
            | ([DynComponent::Distance], 0) => {
                let mut x_field = DynFieldEntry::new(DynComponent::X);
                x_field.buffer = cur_buf;
                self.tabs[i].dyn_fields =
                    vec![x_field, DynFieldEntry::new(DynComponent::Y)];
                self.tabs[i].dyn_active = 1;
            }
            // Already cartesian X (first field) — just advance to Y.
            ([DynComponent::X, DynComponent::Y], 0)
            | ([DynComponent::X, DynComponent::Y, DynComponent::Z], 0) => {
                self.tabs[i].dyn_active = 1;
            }
            // Cartesian Y — extend to 3-D by appending Z.
            ([DynComponent::X, DynComponent::Y], 1) => {
                self.tabs[i]
                    .dyn_fields
                    .push(DynFieldEntry::new(DynComponent::Z));
                self.tabs[i].dyn_active = 2;
            }
            // Cartesian Y in the 3-D set — advance to Z.
            ([DynComponent::X, DynComponent::Y, DynComponent::Z], 1) => {
                self.tabs[i].dyn_active = 2;
            }
            // Z, Angle, or any singleton: nothing further to advance to.
            _ => {}
        }
    }

    /// If dynamic input has at least one locked (typed) field, resolve the
    /// implied point, feed it to the active command as a point pick, reset
    /// the field buffers, and return the resulting task. Returns `None`
    /// when there is nothing typed, so the caller falls back to its normal
    /// Enter handling.
    fn try_dyn_commit(&mut self) -> Option<Task<Message>> {
        let i = self.active_tab;
        if !self.dyn_input
            || self.tabs[i].active_cmd.is_none()
            || self.tabs[i].dyn_fields.is_empty()
            || !self.tabs[i].dyn_fields.iter().any(|f| f.locked())
        {
            return None;
        }
        let pt = self.dyn_resolve_point()?;
        self.last_point = Some(pt);
        self.dyn_user_reshaped = false;
        self.sync_dyn_fields();
        let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_point(pt));
        for f in self.tabs[i].dyn_fields.iter_mut() {
            f.buffer = None;
        }
        self.tabs[i].dyn_active = 0;
        let task = result.map(|r| self.apply_cmd_result(r))?;
        // Match the command-line path: refresh the rubber-band preview
        // so the next segment immediately starts from the new
        // last_point even though no mouse-move fires after a typed
        // coordinate. See #32.
        self.refresh_active_cmd_preview(i);
        Some(task)
    }

    /// Populate all edit buffers from the currently selected dim style.
    fn load_dimstyle_bufs(&mut self, tab: usize) {
        let doc = &self.tabs[tab].scene.document;
        let Some(ds) = doc.dim_styles.get(&self.dimstyle_selected) else {
            return;
        };
        self.ds_dimdle = format!("{}", ds.dimdle);
        self.ds_dimdli = format!("{}", ds.dimdli);
        self.ds_dimgap = format!("{}", ds.dimgap);
        self.ds_dimexe = format!("{}", ds.dimexe);
        self.ds_dimexo = format!("{}", ds.dimexo);
        self.ds_dimsd1 = ds.dimsd1;
        self.ds_dimsd2 = ds.dimsd2;
        self.ds_dimse1 = ds.dimse1;
        self.ds_dimse2 = ds.dimse2;
        self.ds_dimasz = format!("{}", ds.dimasz);
        self.ds_dimcen = format!("{}", ds.dimcen);
        self.ds_dimtsz = format!("{}", ds.dimtsz);
        self.ds_dimtxt = format!("{}", ds.dimtxt);
        self.ds_dimtxsty = ds.dimtxsty.clone();
        self.ds_dimtad = format!("{}", ds.dimtad);
        self.ds_dimtih = ds.dimtih;
        self.ds_dimtoh = ds.dimtoh;
        self.ds_dimscale = format!("{}", ds.dimscale);
        self.ds_dimlfac = format!("{}", ds.dimlfac);
        self.ds_dimlunit = format!("{}", ds.dimlunit);
        self.ds_dimdec = format!("{}", ds.dimdec);
        self.ds_dimpost = ds.dimpost.clone();
        self.ds_dimtol = ds.dimtol;
        self.ds_dimlim = ds.dimlim;
        self.ds_dimtp = format!("{}", ds.dimtp);
        self.ds_dimtm = format!("{}", ds.dimtm);
        self.ds_dimtdec = format!("{}", ds.dimtdec);
        self.ds_dimtfac = format!("{}", ds.dimtfac);
    }

    /// Write edit buffers back into the selected dim style document entry.
    fn apply_dimstyle_bufs(&mut self, tab: usize) {
        self.push_undo_snapshot(tab, "DIMSTYLE EDIT");
        let doc = &mut self.tabs[tab].scene.document;
        let Some(ds) = doc.dim_styles.get_mut(&self.dimstyle_selected) else {
            return;
        };
        macro_rules! set_f64 {
            ($field:ident, $buf:expr) => {
                if let Ok(v) = $buf.trim().parse::<f64>() {
                    ds.$field = v;
                }
            };
        }
        macro_rules! set_i16 {
            ($field:ident, $buf:expr) => {
                if let Ok(v) = $buf.trim().parse::<i16>() {
                    ds.$field = v;
                }
            };
        }
        set_f64!(dimdle, self.ds_dimdle);
        set_f64!(dimdli, self.ds_dimdli);
        set_f64!(dimgap, self.ds_dimgap);
        set_f64!(dimexe, self.ds_dimexe);
        set_f64!(dimexo, self.ds_dimexo);
        set_f64!(dimasz, self.ds_dimasz);
        set_f64!(dimcen, self.ds_dimcen);
        set_f64!(dimtsz, self.ds_dimtsz);
        set_f64!(dimtxt, self.ds_dimtxt);
        set_f64!(dimscale, self.ds_dimscale);
        set_f64!(dimlfac, self.ds_dimlfac);
        set_f64!(dimtp, self.ds_dimtp);
        set_f64!(dimtm, self.ds_dimtm);
        set_f64!(dimtfac, self.ds_dimtfac);
        set_i16!(dimtad, self.ds_dimtad);
        set_i16!(dimlunit, self.ds_dimlunit);
        set_i16!(dimdec, self.ds_dimdec);
        set_i16!(dimtdec, self.ds_dimtdec);
        ds.dimsd1 = self.ds_dimsd1;
        ds.dimsd2 = self.ds_dimsd2;
        ds.dimse1 = self.ds_dimse1;
        ds.dimse2 = self.ds_dimse2;
        ds.dimtih = self.ds_dimtih;
        ds.dimtoh = self.ds_dimtoh;
        ds.dimtol = self.ds_dimtol;
        ds.dimlim = self.ds_dimlim;
        ds.dimpost = self.ds_dimpost.clone();
        ds.dimtxsty = self.ds_dimtxsty.clone();
        self.tabs[tab].dirty = true;
        self.command_line
            .push_output(&format!("DimStyle '{}' updated.", self.dimstyle_selected));
    }

    /// Update a single string buffer field.
    fn apply_ds_edit(&mut self, field: super::DsField, val: String) {
        use super::DsField::*;
        match field {
            Dimdle => self.ds_dimdle = val,
            Dimdli => self.ds_dimdli = val,
            Dimgap => self.ds_dimgap = val,
            Dimexe => self.ds_dimexe = val,
            Dimexo => self.ds_dimexo = val,
            Dimasz => self.ds_dimasz = val,
            Dimcen => self.ds_dimcen = val,
            Dimtsz => self.ds_dimtsz = val,
            Dimtxt => self.ds_dimtxt = val,
            Dimtxsty => self.ds_dimtxsty = val,
            Dimtad => self.ds_dimtad = val,
            Dimscale => self.ds_dimscale = val,
            Dimlfac => self.ds_dimlfac = val,
            Dimlunit => self.ds_dimlunit = val,
            Dimdec => self.ds_dimdec = val,
            Dimpost => self.ds_dimpost = val,
            Dimtp => self.ds_dimtp = val,
            Dimtm => self.ds_dimtm = val,
            Dimtdec => self.ds_dimtdec = val,
            Dimtfac => self.ds_dimtfac = val,
            // Bool fields — no-op for string edit
            _ => {}
        }
    }

    /// Toggle a boolean buffer field.
    fn apply_ds_toggle(&mut self, field: super::DsField) {
        use super::DsField::*;
        match field {
            Dimsd1 => self.ds_dimsd1 = !self.ds_dimsd1,
            Dimsd2 => self.ds_dimsd2 = !self.ds_dimsd2,
            Dimse1 => self.ds_dimse1 = !self.ds_dimse1,
            Dimse2 => self.ds_dimse2 = !self.ds_dimse2,
            Dimtih => self.ds_dimtih = !self.ds_dimtih,
            Dimtoh => self.ds_dimtoh = !self.ds_dimtoh,
            Dimtol => self.ds_dimtol = !self.ds_dimtol,
            Dimlim => self.ds_dimlim = !self.ds_dimlim,
            _ => {}
        }
    }

    /// Populate edit buffers from the currently selected text style.
    fn open_save_dialog_window(&mut self, tab_idx: usize) -> Task<Message> {
        if let Some(id) = self.save_dialog_window {
            return window::gain_focus(id);
        }
        // Pre-fill filename and folder from current path or defaults.
        if let Some(p) = &self.tabs[tab_idx].current_path.clone() {
            if let Some(name) = p.file_name() {
                self.save_dialog_filename = name.to_string_lossy().into_owned();
            }
            if let Some(dir) = p.parent() {
                self.save_dialog_folder = dir.to_path_buf();
            }
        } else {
            let (ext, _) = crate::io::parse_save_format(&self.save_dialog_format);
            self.save_dialog_filename = format!("{}.{ext}", self.tabs[tab_idx].tab_display_name());
        }
        self.save_dialog_entries = crate::io::read_dir_entries(&self.save_dialog_folder.clone());
        let (id, task) = window::open(window::Settings {
            size: iced::Size::new(560.0, 480.0),
            resizable: true,
            level: window::Level::AlwaysOnTop,
            ..Default::default()
        });
        self.save_dialog_window = Some(id);
        task.map(|_| Message::Noop)
    }

    fn close_save_dialog_window(&mut self) -> Task<Message> {
        if let Some(id) = self.save_dialog_window.take() {
            window::close(id)
        } else {
            Task::none()
        }
    }

    fn open_unsaved_dialog_window(&mut self) -> Task<Message> {
        if let Some(id) = self.unsaved_dialog_window {
            return window::gain_focus(id);
        }
        let (id, task) = window::open(window::Settings {
            size: iced::Size::new(420.0, 155.0),
            resizable: false,
            level: window::Level::AlwaysOnTop,
            ..Default::default()
        });
        self.unsaved_dialog_window = Some(id);
        task.map(|_| Message::Noop)
    }

    fn close_unsaved_dialog_window(&mut self) -> Task<Message> {
        if let Some(id) = self.unsaved_dialog_window.take() {
            window::close(id)
        } else {
            Task::none()
        }
    }

    fn load_textstyle_bufs(&mut self, tab: usize) {
        let doc = &self.tabs[tab].scene.document;
        if let Some(s) = doc.text_styles.get(&self.textstyle_selected) {
            self.textstyle_font = s.font_file.clone();
            self.textstyle_width = format!("{:.4}", s.width_factor);
            self.textstyle_oblique = format!("{:.2}", s.oblique_angle.to_degrees());
        }
    }
}

/// Parse a scale string like "1:50" or "2:1" into (numerator, denominator).
/// Returns (1.0, 1.0) for "Fit" or unknown formats.
fn parse_plot_scale(s: &str) -> (f64, f64) {
    if s == "Fit" {
        return (1.0, 1.0);
    }
    if let Some((a, b)) = s.split_once(':') {
        let num: f64 = a.trim().parse().unwrap_or(1.0);
        let den: f64 = b.trim().parse().unwrap_or(1.0);
        if den > 0.0 {
            return (num, den);
        }
    }
    (1.0, 1.0)
}
