use super::{Message, OpenCADStudio};
use crate::scene::VIEWCUBE_DRAW_PX;
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use iced::time::Instant;
use iced::Task;

/// Keystroke-derived messages that an open modal dialog must swallow so the
/// keyboard can't reach the main window (command line, F-key toggles, edit
/// shortcuts) while a dialog is up. `CommandEscape` is handled separately (it
/// closes the modal); a modal's own text fields emit their own messages, which
/// are not in this set. See [`OpenCADStudio::update`] and #126.
fn is_modal_blocked_key_msg(msg: &Message) -> bool {
    matches!(
        msg,
        Message::CommandInput(_)
            | Message::CommandAppendChar(_)
            | Message::CommandSpace
            | Message::CommandFinalize
            | Message::CommandBackspace
            | Message::CommandHistoryPrev
            | Message::CommandHistoryNext
            | Message::DynTabNext
            | Message::MTextCaretMove(_)
            | Message::DeleteSelected
            | Message::ToggleSnapEnabled
            | Message::ToggleGrid
            | Message::ToggleOrtho
            | Message::ToggleGridSnap
            | Message::TogglePolar
            | Message::ToggleOTrack
            | Message::ToggleDynInput
            | Message::TabNew
            | Message::OpenFile
            | Message::SaveFile
            | Message::SaveAs
            | Message::Undo
            | Message::Redo
    )
}

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_DRAW_PX;

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

mod command;
mod dialog;
mod dynamic;
mod file;
mod style;
mod util;
mod viewport;

impl OpenCADStudio {
    /// Close the active in-canvas modal (Plan B), mirroring what closing the
    /// old OS window did: a style editor discards its staged (un-applied)
    /// changes, and the ribbon tool that launched the dialog is de-highlighted.
    fn close_active_modal(&mut self) {
        use super::ModalKind::*;
        if matches!(
            self.active_modal,
            Some(TextStyle | DimStyle | TableStyle | MLeaderStyle | MlStyle)
        ) {
            self.style_stage_discard();
        }
        match self.active_modal {
            Some(Layers) => self.ribbon.deactivate_tool_if("LAYERS"),
            Some(Plot) => {
                self.ribbon.deactivate_tool_if("PLOT");
                self.ribbon.deactivate_tool_if("PRINT");
                self.ribbon.deactivate_tool_if("PAGESETUP");
            }
            Some(TextStyle) => {
                self.ribbon.deactivate_tool_if("STYLE");
                self.ribbon.deactivate_tool_if("TEXTSTYLE");
            }
            Some(TableStyle) => self.ribbon.deactivate_tool_if("TABLESTYLE"),
            Some(MlStyle) => self.ribbon.deactivate_tool_if("MLSTYLE"),
            Some(MLeaderStyle) => self.ribbon.deactivate_tool_if("MLEADERSTYLE"),
            Some(LayoutManager) => {
                self.ribbon.deactivate_tool_if("LAYOUTMANAGER");
                self.ribbon.deactivate_tool_if("LAYOUTPANEL");
            }
            Some(Plotstyle) => {
                self.ribbon.deactivate_tool_if("PLOTSTYLE");
                self.ribbon.deactivate_tool_if("STYLESMANAGER");
            }
            Some(DimStyle) => self.ribbon.deactivate_tool_if("DIMSTYLE"),
            Some(Shortcuts) => {
                self.ribbon.deactivate_tool_if("SHORTCUTS");
                self.ribbon.deactivate_tool_if("KEYBOARD");
            }
            Some(About) => self.ribbon.deactivate_tool_if("ABOUT"),
            // Dismissing these via ✕ is the cancel/decline path.
            Some(Unsaved) => self.pending_close = None,
            Some(AssocPrompt) => self.mark_assoc_prompted(),
            // Cancel: leave the layer (and its objects) untouched.
            Some(LayerDeleteWarning) => self.layer_delete_pending = None,
            // Cancel: drop the working copy without touching the block.
            Some(AttributeEditor) => {
                self.attr_editor_handle = None;
                self.attr_editor_block.clear();
                self.attr_editor_rows.clear();
                self.attr_editor_selected = 0;
                self.attr_editor_tab = crate::ui::window::attribute_editor::AttrTab::Attribute;
            }
            // Closing (✕) discards edits made since the last Apply — matching the
            // style editors. Committing happens only through the Apply button.
            Some(Aliases) => {
                self.alias_editor_rows.clear();
                self.ribbon.deactivate_tool_if("ALIASEDIT");
            }
            _ => {}
        }
        self.active_modal = None;
        // Recentre the next dialog and drop any in-progress drag.
        self.modal_offset = iced::Vector::ZERO;
        self.modal_drag_last = None;
        self.modal_dragging = false;
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        // A modal dialog must capture the keyboard the same way it already
        // captures the mouse. Otherwise keystrokes from the global key
        // subscription leak past the modal into the command line and fire as
        // commands once the dialog closes. While a modal is open, Escape
        // closes it and every other keystroke-derived message is swallowed;
        // the modal's own text fields keep working because they emit their own
        // (non-blocked) messages. (#126)
        if self.active_modal.is_some() {
            if matches!(msg, Message::CommandEscape) {
                return self.update(Message::CloseModal);
            }
            if is_modal_blocked_key_msg(&msg) {
                return Task::none();
            }
        }
        let task = self.update_inner(msg);
        // After every message, mirror the active command step's prompt so
        // its history line stays pinned (non-fading) until the step changes.
        let prompt = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .map(|c| c.prompt());
        self.command_line.set_step_prompt(prompt);
        // Mirror the step's clickable options so they render as buttons (#304).
        let opts = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .map(|c| c.options())
            .unwrap_or_default();
        self.command_line.set_step_options(opts);
        // Persist UI preferences whenever a toggle changes them (issue #68).
        self.persist_settings_if_changed();
        // OTRACK acquires tracking points only while a command or grip drag is
        // running; drop them once neither is active so the temporary tracking
        // points / vectors disappear when the command ends (issue #64).
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_none()
            && self.tabs[i].active_grip.is_none()
            && !self.snapper.tracking_points.is_empty()
        {
            self.snapper.clear_tracking();
            self.otrack_active = None;
        }
        task
    }

    /// Drop the OTRACK acquired points and the live alignment vector once a
    /// point has been committed to the active command. Temporary tracking
    /// points are reset on every input so they don't pile up across a
    /// multi-point command and overwhelm the next pick (issue #85).
    fn reset_tracking_after_point(&mut self) {
        self.snapper.clear_tracking();
        self.otrack_active = None;
    }

    fn update_inner(&mut self, msg: Message) -> Task<Message> {
        match msg {
            // Web: a drawing referenced a script whose Noto subset isn't loaded
            // yet (recorded during text tessellation). Kick off one fetch per
            // pending script; the result comes back as `WebFontLoaded`. (#141)
            Message::PollWebFonts => {
                let pending = crate::scene::text::web_font::take_pending();
                if pending.is_empty() {
                    return Task::none();
                }
                Task::batch(pending.into_iter().map(|script| {
                    Task::perform(crate::scene::text::web_font::fetch(script), move |res| {
                        Message::WebFontLoaded(script, res)
                    })
                }))
            }

            // Web: a per-script font arrived. Store it, drop the stale fallback
            // glyph cache (entries that resolved to nothing while it loaded),
            // and re-tessellate so the text appears. (#141)
            Message::WebFontLoaded(script, res) => {
                match res {
                    Ok(bytes) => {
                        crate::scene::text::web_font::insert(script, Some(bytes));
                        crate::scene::text::ttf_glyph::clear_fallback_cache();
                        for tab in self.tabs.iter_mut() {
                            tab.scene.bump_geometry();
                        }
                    }
                    Err(e) => {
                        crate::scene::text::web_font::insert(script, None);
                        self.command_line
                            .push_error(&format!("Font load failed ({script:?}): {e}"));
                    }
                }
                Task::none()
            }

            Message::Tick(t) => self.on_tick(t),

            Message::OpenFile => self.on_open_file(),

            Message::OpenPathPicked(None) => Task::none(),

            Message::OpenUrl(url) => {
                crate::sys::open_url(&url);
                Task::none()
            }

            Message::StartSectionSelect(section) => {
                self.start_section = section;
                Task::none()
            }

            Message::TogglePropertiesBar => {
                self.props_expanded = !self.props_expanded;
                Task::none()
            }

            Message::ScrollLayoutTabs(dx) => iced::widget::operation::scroll_by(
                iced::advanced::widget::Id::new(crate::ui::statusbar::LAYOUT_TABS_SCROLL_ID),
                iced::widget::scrollable::AbsoluteOffset { x: dx, y: 0.0 },
            ),

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
                self.remove_recent(&path);
                Task::none()
            }

            Message::SetRecentLimit(limit) => {
                self.set_recent_limit(limit);
                // Resync the input box to the clamped, applied value.
                self.recent_limit_input = self.recent_limit.to_string();
                Task::none()
            }

            Message::RecentLimitInput(s) => {
                // Keep only digits while typing; applied on Enter (SetRecentLimit).
                self.recent_limit_input = s.chars().filter(|c| c.is_ascii_digit()).collect();
                Task::none()
            }

            Message::OpenPathPicked(Some((path, size_bytes))) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".into());
                let phase = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(
                    super::OPEN_PHASE_READING,
                ));
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
                self.on_file_opened(name, path, doc, caches)
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
                use crate::modules::draw::draw::raster_image::ImageCommand;
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
                        .add_filter("CAD Files", &["dwg", "dxf", "bak", "DWG", "DXF", "BAK"])
                        .add_filter("DWG Files", &["dwg", "DWG"])
                        .add_filter("DXF Files", &["dxf", "DXF"])
                        .add_filter("Backup Files", &["bak", "BAK"])
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(crate::sys::handle_path(&h)),
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
                            .map(|h| crate::sys::handle_path(&h));
                        (name, path)
                    },
                    |(name, path)| Message::WblockSaveResult(name, path),
                )
            }

            Message::WblockSaveResult(block_name, Some(path)) => {
                self.on_wblock_save_result_some(block_name, path)
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
                            .map(|h| crate::sys::handle_path(&h));
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
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    Message::StlExportPath,
                )
            }

            Message::StlExportPath(Some(path)) => self.on_stl_export_path_some(path),

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
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    Message::StepExportPath,
                )
            }

            Message::StepExportPath(Some(path)) => self.on_step_export_path_some(path),

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
                        .map(|h| crate::sys::handle_path(&h))
                },
                Message::ObjImportPath,
            ),

            Message::ObjImportPath(Some(path)) => self.on_obj_import_path_some(path),

            Message::ObjImportPath(None) => Task::none(),

            Message::SaveFile => self.on_save_file(),

            Message::SaveAs => {
                if self.read_only {
                    self.command_line
                        .push_error("Read-only session (--read-only): saving is disabled.");
                    return Task::none();
                }
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

            Message::SaveDialogConfirm => self.on_save_dialog_confirm(),

            Message::SaveDialogCancel => self.close_save_dialog_window(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveDialogPathPicked(picked) => self.on_save_dialog_path_picked(picked),

            #[cfg(target_arch = "wasm32")]
            Message::SaveDialogPathPicked(_) => Task::none(),

            Message::ClearScene => {
                let i = self.active_tab;
                self.push_undo_snapshot(i, "CLEAR");
                self.tabs[i].scene.clear();
                crate::io::linetypes::populate_document(&mut self.tabs[i].scene.document);
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

            Message::SetRenderMode(mode) => self.on_set_render_mode(mode),

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

            Message::SetRibbonCollapseMode(mode) => {
                self.ribbon.set_collapse_mode(mode);
                self.ribbon.close_dropdown();
                self.save_config();
                Task::none()
            }

            Message::RibbonToolClick { tool_id, event } => {
                self.on_ribbon_tool_click(tool_id, event)
            }
            Message::PluginFileDialogResult { command, path } => {
                if let Some(path) = path {
                    // Dispatch "<command> <path>" with original case intact —
                    // the command line would upper-case the whole string and
                    // mangle case-sensitive paths on Linux/macOS.
                    let line = format!("{} {}", command, path.to_string_lossy());
                    let i = self.active_tab;
                    if !crate::plugin::try_dispatch(self, i, &line) {
                        self.command_line
                            .push_error(&format!("No plugin handled: {command}"));
                    }
                }
                Task::none()
            }

            // ── Document tabs ─────────────────────────────────────────────
            Message::TabNew => {
                // Preserve the outgoing drawing's Ortho / running OSNAP.
                self.stamp_header_sysvars(self.active_tab);
                self.tab_counter += 1;
                let new_tab = super::document::DocumentTab::new_drawing(self.tab_counter);
                self.tabs.push(new_tab);
                self.active_tab = self.tabs.len() - 1;
                let idx = self.active_tab;
                // A fresh drawing inherits the app's current Ortho / OSNAP so
                // creating one doesn't silently reset them (its header default is
                // 0 = no snaps); saving then persists them into the file.
                self.stamp_header_sysvars(idx);
                self.apply_bg_default(idx);
                self.sync_ribbon_layers();
                self.sync_ribbon_styles();
                // #21: reset ribbon Color / Linetype / Lineweight to the
                // fresh tab's defaults (ByLayer) instead of inheriting the
                // previous tab's last selection.
                self.sync_ribbon_from_selection();
                // A fresh drawing starts with grid/snap off (its tile defaults).
                self.adopt_view_display(self.active_tab);
                Task::none()
            }

            Message::TabSwitch(idx) => {
                if idx < self.tabs.len() {
                    if idx != self.active_tab {
                        // The attribute editor is tab-scoped; leaving its tab
                        // drops it (its handle is that document's, not this one's).
                        self.cancel_attr_editor();
                        // Persist the outgoing drawing's Ortho / running OSNAP
                        // before leaving it, so switching back restores them.
                        let prev = self.active_tab;
                        self.stamp_header_sysvars(prev);
                    }
                    self.active_tab = idx;
                    self.sync_ribbon_layers();
                    self.sync_ribbon_styles();
                    // #21: also re-seed ribbon Color / Linetype / Lineweight
                    // from the newly active tab so they reflect that doc's
                    // CECOLOR / CELTYPE / CELWEIGHT (or its current selection
                    // if there is one), not the prior tab's choice.
                    self.sync_ribbon_from_selection();
                    // Grid/snap follow the newly active drawing's viewport.
                    self.adopt_view_display(idx);
                    // Ortho / running OSNAP follow the newly active drawing.
                    self.adopt_header_sysvars(idx);
                    // Shared CJK ideographs follow the newly active drawing's
                    // language; re-tessellate if it differs from the last. (#141)
                    if crate::scene::text::web_font::set_cjk_lang_from_codepage(
                        &self.tabs[idx].scene.document.header.code_page,
                    ) {
                        crate::scene::text::ttf_glyph::clear_fallback_cache();
                        self.tabs[idx].scene.bump_geometry();
                    }
                }
                Task::none()
            }

            Message::TabClose(idx) => self.on_tab_close(idx),

            Message::CommandInput(s) => {
                // Space submits (acts like Enter) so a command advances
                // token-by-token, matching CAD convention. A leading `>` switches
                // to literal-space mode so an argument containing spaces (a text
                // string, a path, `UCS Z 90` as one line) can be typed; the `>`
                // is stripped on submit. (Unfocused Space repeats the last
                // command via CommandSpace.)
                // Command-line entry is shown uppercase.
                let s = s.to_uppercase();
                // A space submits (acts like Enter). The whole value is handed to
                // the submit path, which tokenises multi-token lines — so a typed
                // token, a pasted `LINE 0,0 10,10`, or API-fed text all run their
                // spaces as step separators. A leading `>` keeps spaces literal.
                if !s.starts_with('>') && s.contains(' ') {
                    self.command_line.input = s;
                    return self.update(Message::CommandSubmit);
                }
                self.command_line.input = s;
                // Typing invalidates the previous arrow-key cursor —
                // the matches list has likely changed.
                self.command_line.autocomplete_cursor = None;
                Task::none()
            }

            Message::CommandAppendChar(s) => self.on_command_append_char(s),

            Message::CommandBackspace => self.on_command_backspace(),

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
                self.tabs[i].scene.split_active_pane(horizontal);
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::CloseModelViewport => {
                let i = self.active_tab;
                self.tabs[i].scene.close_active_pane();
                self.tabs[i].scene.camera_generation += 1;
                self.sync_render_mode_to_active_tile(i);
                self.adopt_view_display(i);
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
                if self.tabs[i].active_cmd.is_none() && self.command_line.autocomplete_prev() {
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
                if self.tabs[i].active_cmd.is_none() && self.command_line.autocomplete_next() {
                    return Task::none();
                }
                self.command_line.history_next();
                Task::none()
            }

            Message::CommandHistoryToggle => {
                self.command_line.toggle_history();
                // On open, snapshot the current log into the read-only editor
                // buffer so it can be drag-selected across lines and copied.
                if self.command_line.history_open {
                    use iced::widget::text_editor::{Action, Motion};
                    self.history_content = iced::widget::text_editor::Content::with_text(
                        &self.command_line.history_plain_text(),
                    );
                    // Scroll to the newest line (bottom), the most useful when
                    // opening the log to grab a recent error.
                    self.history_content.perform(Action::Move(Motion::DocumentEnd));
                }
                Task::none()
            }

            Message::CommandHistoryCopy => {
                let text = self.command_line.history_plain_text();
                if text.is_empty() {
                    Task::none()
                } else {
                    iced::clipboard::write(text)
                }
            }

            Message::CommandHistoryClear => {
                self.command_line.clear_history();
                self.history_content = iced::widget::text_editor::Content::new();
                Task::none()
            }

            Message::CommandHistoryEdit(action) => {
                // Read-only: drop edits, keep selection / cursor / scroll so
                // the user can still highlight and Ctrl+C the log.
                if !action.is_edit() {
                    self.history_content.perform(action);
                }
                Task::none()
            }

            Message::CommandSuggestionPick(cmd) => {
                self.command_line.input.clear();
                self.command_line.close_history();
                self.dispatch_command(&cmd)
            }

            Message::CommandOptionPick(kw) => {
                // Clicking an option button feeds its keyword to the active
                // command through the same path as typed text; an empty keyword
                // finishes the step like Enter. (#304)
                self.command_line.input.clear();
                self.command_line.close_history();
                if kw.is_empty() {
                    return self.feed_command(crate::command::StepInput::Enter);
                }
                self.feed_active_cmd(&kw);
                Task::none()
            }

            Message::CommandSubmit => self.on_command_submit(),

            Message::CommandSpace => {
                // Space is a literal space inside the MText preview; otherwise
                // it finalises the active command like Enter.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    self.mtext_type(" ");
                    return Task::none();
                }
                // A leading `>` puts the command line in "literal space" mode so
                // the user can type arguments that contain spaces; otherwise
                // Space works like Enter. The `>` is stripped on submit.
                if self.command_line.input.starts_with('>') {
                    self.command_line.input.push(' ');
                    return Task::none();
                }
                return self.update(Message::CommandFinalize);
            }
            Message::CommandFinalize => self.on_command_finalize(),

            Message::CommandEscape => self.on_command_escape(),

            Message::Command(cmd) => {
                // Close viewport context menu if open.
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                // Any command also dismisses the Isolate action menu.
                self.isolate_popup_open = false;
                // "Pick window" (PLOTWINDOW) from Page Setup needs the backdrop
                // gone so the viewport pick lands; every other command leaves an
                // open modal (and its staged edits) untouched.
                if cmd.trim().eq_ignore_ascii_case("PLOTWINDOW")
                    || cmd.trim().eq_ignore_ascii_case("PW")
                {
                    self.close_active_modal();
                }
                self.dispatch_command(&cmd)
            }

            Message::ToggleLayers => {
                if self.active_modal == Some(super::ModalKind::Layers) {
                    self.ribbon.deactivate_tool_if("LAYERS");
                    self.active_modal = None;
                } else {
                    self.sync_ribbon_layers();
                    self.active_modal = Some(super::ModalKind::Layers);
                }
                Task::none()
            }

            Message::WindowCloseRequested(id) => {
                if self.main_window == Some(id) {
                    if self.tabs.iter().any(|t| t.dirty) {
                        self.pending_close = Some(super::PendingClose::Quit);
                        return self.open_unsaved_dialog_window();
                    }
                    return self.exit_app();
                }
                Task::none()
            }

            Message::OsWindowClosed(id) => {
                // Only the main window exists now; all dialogs are in-canvas
                // modals (Plan B). Closing it exits.
                if self.main_window == Some(id) {
                    return self.exit_app();
                }
                Task::none()
            }

            // ── Layer panel messages ───────────────────────────────────────
            Message::LayerToggleVisible(idx) => {
                let i = self.active_tab;
                // New state = toggle of the clicked row, applied to every target
                // (the whole selection when the clicked row is part of it) (#236).
                let on = self.tabs[i].layers.layers.get(idx).map(|l| !l.visible);
                let targets = self.layer_row_action_targets(i, idx);
                if let Some(on) = on {
                    if !targets.is_empty() {
                        self.push_undo_snapshot(i, "LAYER OFF/ON");
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                dl.flags.off = !on;
                            }
                            if let Some(pl) =
                                self.tabs[i].layers.layers.iter_mut().find(|l| &l.name == name)
                            {
                                pl.visible = on;
                            }
                        }
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(&format!(
                            "{} layer(s) turned {}",
                            targets.len(),
                            if on { "on" } else { "off" }
                        ));
                        self.sync_ribbon_layers();
                    }
                }
                Task::none()
            }

            Message::LayerSort(col) => {
                let i = self.active_tab;
                self.tabs[i].layers.sort_by(col);
                // Keep the ribbon dropdown's order (and its toggle indices) in
                // step with the re-sorted manager table.
                self.sync_ribbon_layers();
                Task::none()
            }

            Message::LayerToggleLock(idx) => {
                let i = self.active_tab;
                let locked = self.tabs[i].layers.layers.get(idx).map(|l| !l.locked);
                let targets = self.layer_row_action_targets(i, idx);
                if let Some(locked) = locked {
                    if !targets.is_empty() {
                        self.push_undo_snapshot(i, "LAYER LOCK/UNLOCK");
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                dl.flags.locked = locked;
                            }
                            if let Some(pl) =
                                self.tabs[i].layers.layers.iter_mut().find(|l| &l.name == name)
                            {
                                pl.locked = locked;
                            }
                        }
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(&format!(
                            "{} layer(s) {}",
                            targets.len(),
                            if locked { "locked" } else { "unlocked" }
                        ));
                        self.sync_ribbon_layers();
                    }
                }
                Task::none()
            }

            Message::LayerToggleFreeze(idx) => {
                let i = self.active_tab;
                let frozen = self.tabs[i].layers.layers.get(idx).map(|l| !l.frozen);
                let targets = self.layer_row_action_targets(i, idx);
                if let Some(frozen) = frozen {
                    if !targets.is_empty() {
                        self.push_undo_snapshot(i, "LAYER FREEZE");
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                if frozen {
                                    dl.freeze();
                                } else {
                                    dl.thaw();
                                }
                            }
                            if let Some(pl) =
                                self.tabs[i].layers.layers.iter_mut().find(|l| &l.name == name)
                            {
                                pl.frozen = frozen;
                            }
                        }
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(&format!(
                            "{} layer(s) {}",
                            targets.len(),
                            if frozen { "frozen" } else { "thawed" }
                        ));
                        self.sync_ribbon_layers();
                    }
                }
                Task::none()
            }

            Message::LayerToggleVpFreeze(layer_idx, vp_col_idx) => {
                self.on_layer_toggle_vp_freeze(layer_idx, vp_col_idx)
            }

            Message::LayerNew => self.on_layer_new(),

            Message::LayerDelete => self.on_layer_delete(),

            Message::LayerDeleteConfirm => self.on_layer_delete_confirm(),

            Message::LayerSetCurrent => self.on_layer_set_current(),

            Message::LayerSelect(idx) => {
                let i = self.active_tab;
                if self.tabs[i].layers.editing.is_some() {
                    return Task::done(Message::LayerRenameCommit);
                }
                let (shift, ctrl) = (self.shift_down, self.ctrl_down);
                let panel = &mut self.tabs[i].layers;
                if ctrl {
                    // Ctrl/Cmd-click toggles this row in the selection.
                    if let Some(pos) = panel.selected_multi.iter().position(|&x| x == idx) {
                        panel.selected_multi.remove(pos);
                    } else {
                        panel.selected_multi.push(idx);
                    }
                } else if shift {
                    // Shift-click selects the range from the anchor to here.
                    let anchor = panel.selected.unwrap_or(idx);
                    let (lo, hi) = (anchor.min(idx), anchor.max(idx));
                    panel.selected_multi = (lo..=hi).collect();
                } else if !panel.selected_multi.contains(&idx) {
                    // Plain click collapses to this row — but NOT when it is
                    // already part of a multi-selection, so clicking a property
                    // combo (linetype / lineweight) on one of the selected rows
                    // keeps the selection and the edit stays bulk (#236).
                    panel.selected_multi = vec![idx];
                }
                panel.selected = Some(idx);
                Task::none()
            }

            Message::LayerRenameStart(idx) => {
                let i = self.active_tab;
                self.tabs[i].layers.selected = Some(idx);
                self.tabs[i].layers.selected_multi = vec![idx];
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

            Message::LayerRenameCommit => self.on_layer_rename_commit(),

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
                    // Opening the swatch on a row outside the current
                    // multi-selection narrows to just that row; on a selected
                    // row it keeps the multi-selection so the pick applies to all.
                    if !panel.selected_multi.contains(&idx) {
                        panel.selected_multi = vec![idx];
                    }
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
                // Apply to every selected layer (multi-select), not just one.
                let names = self.selected_layer_names(i);
                if !names.is_empty() {
                    use crate::ui::window::layers::iced_color_from_acad;
                    let new_color = iced_color_from_acad(&AcadColor::Index(aci));
                    for name in &names {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.color = AcadColor::Index(aci);
                        }
                    }
                    for pl in self.tabs[i].layers.layers.iter_mut() {
                        if names.contains(&pl.name) {
                            pl.color = new_color;
                        }
                    }
                    self.tabs[i].dirty = true;
                    // ByLayer color is baked into the cached wires at
                    // tessellation time, so bump the geometry epoch to
                    // invalidate the wire cache and repaint with the new color.
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].layers.color_picker_row = None;
                    self.tabs[i].layers.color_full_palette = false;
                    self.sync_ribbon_layers();
                }
                Task::none()
            }

            Message::LayerLinetypeSet(lt) => {
                let i = self.active_tab;
                let names = self.selected_layer_names(i);
                if !names.is_empty() {
                    for name in &names {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.line_type = lt.clone();
                        }
                    }
                    for pl in self.tabs[i].layers.layers.iter_mut() {
                        if names.contains(&pl.name) {
                            pl.linetype = lt.clone();
                        }
                    }
                    self.tabs[i].dirty = true;
                    // Linetype is baked into the cached wires; repaint.
                    self.tabs[i].scene.bump_geometry();
                }
                Task::none()
            }

            Message::LayerLineweightSet(lw) => {
                let i = self.active_tab;
                let names = self.selected_layer_names(i);
                if !names.is_empty() {
                    for name in &names {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.line_weight = lw;
                        }
                    }
                    for pl in self.tabs[i].layers.layers.iter_mut() {
                        if names.contains(&pl.name) {
                            pl.lineweight = lw;
                        }
                    }
                    self.tabs[i].dirty = true;
                    // Lineweight is baked into the cached wires; repaint.
                    self.tabs[i].scene.bump_geometry();
                }
                Task::none()
            }

            Message::LayerTransparencyEdit(idx, s) => {
                let i = self.active_tab;
                let val = if let Ok(v) = s.parse::<i32>() {
                    Some(v.clamp(0, 90))
                } else if s.is_empty() {
                    Some(0)
                } else {
                    None
                };
                // Apply the edited transparency to every selected layer (#236).
                if let Some(v) = val {
                    let targets = self.layer_row_action_targets(i, idx);
                    for name in &targets {
                        if let Some(pl) =
                            self.tabs[i].layers.layers.iter_mut().find(|l| &l.name == name)
                        {
                            pl.transparency = v;
                        }
                    }
                }
                Task::none()
            }

            // ── Cursor / viewport messages ─────────────────────────────────
            Message::CursorMoved(p) => self.on_cursor_moved(p),

            Message::ViewportMove(p) => self.on_viewport_move(p),

            Message::ViewportExit => self.on_viewport_exit(),

            // ── Per-pane Model viewport ───────────────────────────────────
            Message::PaneResized(ev) => self.on_pane_resized(ev),
            Message::PaneClicked(pane) => self.on_pane_clicked(pane),
            Message::PaneDragged(ev) => self.on_pane_dragged(ev),
            Message::PaneMove(idx, local) => {
                let p = self.pane_canvas_point(idx, local);
                // While dragging a pane, just track the cursor (no focus swap or
                // snap) so the drop target reads cleanly.
                if self.pane_move_from.is_some() {
                    self.tabs[self.active_tab]
                        .scene
                        .selection
                        .borrow_mut()
                        .last_move_pos = Some(p);
                    return Task::none();
                }
                self.focus_model_pane(idx);
                self.on_viewport_move(p)
            }
            Message::PaneMoveStart => {
                let i = self.active_tab;
                self.pane_move_from = Some(self.tabs[i].scene.active_model_tile.get());
                Task::none()
            }
            Message::PanePress(idx) => {
                // A fresh press ends any stale (un-dropped) pane move.
                self.pane_move_from = None;
                self.focus_model_pane(idx);
                self.on_viewport_left_press()
            }
            Message::PaneRelease(idx) => {
                // Finishing a pane-move drag: swap the source pane with the one
                // released over, instead of the normal release handling.
                if let Some(from) = self.pane_move_from.take() {
                    let i = self.active_tab;
                    self.tabs[i].scene.swap_model_panes(from, idx);
                    self.tabs[i].scene.camera_generation += 1;
                    return Task::none();
                }
                self.focus_model_pane(idx);
                self.on_viewport_left_release()
            }
            Message::PaneRightPress(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportRightPress)
            }
            Message::PaneRightRelease(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportRightRelease)
            }
            Message::PaneMiddlePress(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportMiddlePress)
            }
            Message::PaneMiddleRelease(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportMiddleRelease)
            }
            Message::PaneScroll(idx, d) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportScroll(d))
            }

            Message::ViewportLeftPress => self.on_viewport_left_press(),

            Message::ViewportLeftRelease => self.on_viewport_left_release(),

            Message::ViewportRightPress => {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                let Some(p) = sel.last_move_pos else {
                    return Task::none();
                };
                sel.context_menu = None;
                sel.right_down = true;
                sel.right_press_pos = Some(p);
                sel.right_press_time = Some(iced::time::Instant::now());
                sel.right_last_pos = Some(p);
                sel.right_dragging = false;
                Task::none()
            }

            Message::ViewportRightRelease => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                let Some(click_pos) = sel.last_move_pos else {
                    return Task::none();
                };
                if !sel.right_down {
                    return Task::none();
                }
                let was_click = !sel.right_dragging;
                sel.right_down = false;
                sel.right_press_pos = None;
                sel.right_press_time = None;
                sel.right_last_pos = None;
                sel.right_dragging = false;
                sel.orbit_pivot = None;
                if !was_click {
                    return Task::none();
                }
                // A command name (or option keyword / value) typed into the
                // command line but not yet entered runs on right-click, exactly
                // as pressing Enter would. Route through CommandFinalize — the
                // canonical Enter action — so the same MText / grip-popup guards
                // apply and a non-empty line is forwarded to the submit path.
                // Without this the typed text would be swallowed by the context
                // menu (when idle) or the Enter cycle. Every other right-click
                // behaviour below is unchanged and only applies when the command
                // line is empty. Pending text always runs and resets the Enter
                // cycle so the next right-click acts as Enter again.
                if !self.command_line.input.trim().is_empty() {
                    sel.right_click_entered = false;
                    drop(sel);
                    return self.update(Message::CommandFinalize);
                }
                // A right-click (no orbit). While a command is active the first
                // right-click acts as Enter (commit / close); a second
                // consecutive right-click opens the context menu instead. When
                // idle it always opens the menu. (Right-drag, handled above,
                // always orbits.) Any other interaction — a left-click pick or a
                // new command — resets the cycle so the next right-click is Enter.
                if self.tabs[i].active_cmd.is_some() && !sel.right_click_entered {
                    sel.right_click_entered = true;
                    drop(sel);
                    return self.update(Message::CommandFinalize);
                }
                sel.right_click_entered = false;
                sel.context_menu = Some(click_pos);
                sel.draworder_submenu = false;
                Task::none()
            }

            Message::ViewportMiddlePress => self.on_viewport_middle_press(),

            Message::ViewportMiddleRelease => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.middle_down = false;
                sel.middle_last_pos = None;
                // End of a Shift+MMB orbit — drop the captured pivot so the next
                // gesture recomputes it against the current selection. (#229)
                sel.orbit_pivot = None;
                Task::none()
            }

            Message::ViewportScroll(delta) => self.on_viewport_scroll(delta),

            Message::ViewportClick => self.on_viewport_click(),

            Message::WindowResized(w, h) => {
                self.vp_size = ((w - 440.0).max(200.0), h);
                self.win_size = (w, h);
                Task::none()
            }

            Message::ViewCubeSnap(region) => self.on_view_cube_snap(region),
            Message::ViewCubeSnapWorld(region) => self.on_view_cube_snap_world(region),

            Message::ViewCubeHome => {
                let i = self.active_tab;
                let r_ucs = self.tabs[i].scene.viewcube_ucs_mat();
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.home_view(r_ucs));
                } else {
                    self.tabs[i].scene.camera.borrow_mut().home_view(r_ucs);
                }
                self.tabs[i].scene.camera_generation += 1;
                self.command_line.push_output("View: Home");
                Task::none()
            }

            Message::ViewCubeRoll(cw) => {
                let i = self.active_tab;
                let ang = if cw {
                    std::f32::consts::FRAC_PI_2
                } else {
                    -std::f32::consts::FRAC_PI_2
                };
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.roll_by(ang));
                } else {
                    self.tabs[i].scene.camera.borrow_mut().roll_by(ang);
                }
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::ViewCubeNudge(dir) => {
                use crate::scene::NudgeDir;
                let (horizontal, positive) = match dir {
                    NudgeDir::Up => (false, false),
                    NudgeDir::Down => (false, true),
                    NudgeDir::Left => (true, false),
                    NudgeDir::Right => (true, true),
                };
                let i = self.active_tab;
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.nudge_90(horizontal, positive));
                } else {
                    self.tabs[i]
                        .scene
                        .camera
                        .borrow_mut()
                        .nudge_90(horizontal, positive);
                }
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::SetViewcubeUcs(name) => {
                let i = self.active_tab;
                if name.is_empty() || name == "WCS" {
                    self.tabs[i].active_ucs = None;
                    self.command_line.push_output("UCS: World");
                } else if let Some(named) = self.tabs[i].scene.document.ucss.get(&name).cloned() {
                    self.tabs[i].active_ucs = Some(named);
                    self.command_line.push_output(&format!("UCS: {}", name));
                }
                self.tabs[i].sync_ucs_to_scene();
                self.tabs[i].scene.camera_generation += 1;
                self.tabs[i].dirty = true;
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

            Message::HoverDwellTick => self.on_hover_dwell_tick(),

            Message::VisibilityPick(idx) => {
                if let Some(popup) = self.visibility_popup.take() {
                    self.apply_visibility_state(popup.insert_handle, idx);
                }
                Task::none()
            }

            Message::GripMenuPick(idx) => self.on_grip_menu_pick(idx),

            // ── Snap / mode toggles ───────────────────────────────────────
            Message::ToggleSnapEnabled => {
                self.snapper.toggle_global();
                self.sync_vport_display(self.active_tab);
                Task::none()
            }
            Message::ToggleGridSnap => {
                self.snapper.toggle_grid_snap();
                self.sync_vport_display(self.active_tab);
                Task::none()
            }
            Message::ToggleGrid => {
                self.show_grid ^= true;
                self.sync_vport_display(self.active_tab);
                Task::none()
            }
            Message::ToggleOrtho => {
                self.ortho_mode ^= true;
                if self.ortho_mode {
                    self.polar_mode = false;
                }
                // If the user manually toggles ortho during a command that
                // suppressed it (e.g. RECTANG), the toggle is permanent —
                // don't restore the pre-command state when the command ends.
                self.rect_suppressed_ortho = false;
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
            Message::CycleCoordsMode => {
                // $COORDS 0 (static) → 1 (live absolute) → 2 (polar) → 0.
                let i = self.active_tab;
                if i < self.tabs.len() {
                    let mode = {
                        let h = &mut self.tabs[i].scene.document.header;
                        h.coords_mode = (h.coords_mode + 1).rem_euclid(3);
                        h.coords_mode
                    };
                    self.tabs[i].dirty = true;
                    let label = match mode {
                        0 => "static",
                        2 => "polar",
                        _ => "live",
                    };
                    self.command_line
                        .push_output(&format!("COORDS = {mode} ({label})"));
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
                self.ribbon.set_viewcube(self.show_viewcube);
                Task::none()
            }
            Message::ToggleProperties => {
                self.show_properties ^= true;
                self.ribbon.set_properties(self.show_properties);
                Task::none()
            }
            Message::ToggleFileTabs => {
                self.show_file_tabs ^= true;
                self.ribbon.set_file_tabs(self.show_file_tabs);
                Task::none()
            }
            Message::ToggleLayoutTabs => {
                self.show_layout_tabs ^= true;
                self.ribbon.set_layout_tabs(self.show_layout_tabs);
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
                self.polar_popup_open = false;
                Task::none()
            }
            Message::TogglePolarPopup => {
                self.polar_popup_open ^= true;
                if self.polar_popup_open {
                    // Start the custom field empty each time; picking a preset or
                    // typing a value is what actually enables polar tracking.
                    self.polar_custom_input.clear();
                }
                Task::none()
            }
            Message::ClosePolarPopup => {
                self.polar_popup_open = false;
                Task::none()
            }
            Message::PolarCustomInput(s) => {
                self.polar_custom_input = s;
                Task::none()
            }
            Message::SubmitPolarCustom => {
                // Accept any positive angle up to a full turn; ignore garbage.
                if let Ok(v) = self.polar_custom_input.trim().parse::<f32>() {
                    if v > 0.0 && v <= 360.0 {
                        self.polar_increment_deg = v;
                        self.polar_mode = true;
                        self.ortho_mode = false;
                    }
                }
                self.polar_custom_input.clear();
                self.polar_popup_open = false;
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
            Message::ToggleLayoutList => {
                self.layout_list_open ^= true;
                Task::none()
            }
            Message::CloseLayoutList => {
                self.layout_list_open = false;
                Task::none()
            }
            Message::ToggleStatusBarMenu => {
                self.statusbar_menu_open ^= true;
                Task::none()
            }
            Message::CloseStatusBarMenu => {
                self.statusbar_menu_open = false;
                Task::none()
            }
            Message::ToggleStatusPill(pill) => {
                // Keep the menu open so several pills can be toggled in a row.
                self.statusbar_config.toggle(pill);
                self.save_config();
                Task::none()
            }
            Message::ToggleCleanScreen => {
                self.clean_screen ^= true;
                Task::none()
            }
            Message::ToggleTransparencyDisplay => {
                let i = self.active_tab;
                if i < self.tabs.len() {
                    // No retessellate — the wire shader reads the flag from uniforms.
                    self.tabs[i].scene.transparency_display ^= true;
                }
                Task::none()
            }
            Message::ToggleQuickProperties => {
                self.quick_properties ^= true;
                Task::none()
            }
            Message::ToggleSelectionCycling => {
                self.selection_cycling ^= true;
                self.cycle_candidates = None;
                self.tabs[self.active_tab].scene.set_hover_highlight(None);
                Task::none()
            }
            Message::CycleSelect(handle) => {
                // Add the picked object to the current selection (accumulate).
                self.cycle_candidates = None;
                let i = self.active_tab;
                self.tabs[i].scene.set_hover_highlight(None);
                self.tabs[i].scene.select_entity(handle, false);
                self.tabs[i].scene.expand_selection_for_groups(&[handle]);
                self.refresh_properties();
                Task::none()
            }
            Message::CycleHover(handle) => {
                let i = self.active_tab;
                self.tabs[i].scene.set_hover_highlight(handle);
                Task::none()
            }
            Message::CycleHoverExit(handle) => {
                // Only clear if another row hasn't already taken the highlight;
                // enter/exit can fire out of order when moving between rows.
                let i = self.active_tab;
                if self.tabs[i].scene.hover_highlight == Some(handle) {
                    self.tabs[i].scene.set_hover_highlight(None);
                }
                Task::none()
            }
            Message::CycleCancel => {
                self.cycle_candidates = None;
                self.tabs[self.active_tab].scene.set_hover_highlight(None);
                Task::none()
            }
            Message::ToggleSelectionFilterPopup => {
                self.selection_filter_popup_open ^= true;
                Task::none()
            }
            Message::CloseSelectionFilterPopup => {
                self.selection_filter_popup_open = false;
                Task::none()
            }
            Message::ToggleSelectionFilterType(name) => {
                let f = &mut self.tabs[self.active_tab].scene.selection_filter;
                if !f.remove(&name) {
                    f.insert(name);
                }
                Task::none()
            }
            Message::SelectionFilterSelectAll => {
                self.tabs[self.active_tab].scene.selection_filter.clear();
                Task::none()
            }
            Message::SelectionFilterClearAll => {
                let i = self.active_tab;
                let types = self.tabs[i].scene.entity_type_names_in_layout();
                let f = &mut self.tabs[i].scene.selection_filter;
                for t in types {
                    f.insert(t.to_string());
                }
                Task::none()
            }
            Message::ToggleUnitsPopup => {
                self.units_popup_open ^= true;
                Task::none()
            }
            Message::CloseUnitsPopup => {
                self.units_popup_open = false;
                Task::none()
            }
            Message::SetDrawingUnits(code) => {
                self.units_popup_open = false;
                let i = self.active_tab;
                self.tabs[i].scene.document.header.insertion_units = code;
                self.tabs[i].dirty = true;
                Task::none()
            }
            Message::ToggleIsolatePopup => {
                self.isolate_popup_open ^= true;
                Task::none()
            }
            Message::CloseIsolatePopup => {
                self.isolate_popup_open = false;
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
            Message::ToggleRibbonPanel(id) => {
                self.ribbon.toggle_collapsed_panel(&id);
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
                // In the MText preview, Delete removes text at the caret.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    self.mtext_delete();
                    return Task::none();
                }
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let handles: Vec<_> = self.tabs[i].scene.selected.iter().cloned().collect();
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "ERASE");
                    // Stash the erased entities so OOPS can restore them.
                    self.oops_cache = handles
                        .iter()
                        .filter_map(|h| self.tabs[i].scene.document.get_entity(*h).cloned())
                        .collect();
                    self.tabs[i].scene.erase_entities(&handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::SetModifiers { shift, ctrl } => {
                let ctrl_changed = self.ctrl_down != ctrl;
                self.shift_down = shift;
                self.ctrl_down = ctrl;
                // A live command may key its preview off Ctrl (arc-direction
                // flip). Rebuild it at the current cursor so the flip shows
                // without waiting for the next mouse move.
                let i = self.active_tab;
                if ctrl_changed && self.tabs[i].active_cmd.is_some() {
                    let p = self.tabs[i].last_cursor_screen;
                    return Task::done(Message::ViewportMove(p));
                }
                Task::none()
            }

            // ── In-place MText editor ───────────────────────────────────
            Message::MTextEdit(action) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.content.perform(action);
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextFmt(kind) => {
                self.mtext_apply_fmt(kind);
                Task::none()
            }
            Message::MTextHeight(s) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.height = s;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColorChanged(color) => {
                self.mtext_apply_color(color);
                Task::none()
            }
            Message::MTextColorPickerToggle => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.color_picker_open = !ed.color_picker_open;
                }
                Task::none()
            }
            Message::MTextStyle(s) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.style = s;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextFont(f) => {
                self.mtext_apply_font(&f);
                Task::none()
            }
            Message::MTextOblique(s) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.oblique = s;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextWidth(s) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.width = s;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextCharSpace(s) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.char_space = s;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextJustify(ap) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.attachment = ap;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextAlign(a) => {
                self.mtext_apply_align(a);
                Task::none()
            }
            Message::MTextLineSpacing(f) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.line_spacing = f;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextShowPreview(on) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.show_preview = on;
                }
                self.rebuild_mtext_preview();
                // Focus the text area when switching to Edit so the caret
                // shows and typing/clicking edits immediately.
                if on {
                    Task::none()
                } else {
                    iced::widget::operation::focus(iced::widget::Id::new(
                        super::view::MTEXT_TEXT_ID,
                    ))
                }
            }
            Message::MTextSelStart(off) => {
                // Count quick same-spot clicks: 1 = place caret, 2 = select the
                // word, 3 = select all.
                let now = Instant::now();
                let count = match self.mtext_click_time {
                    Some(t)
                        if now.duration_since(t).as_millis() < 400
                            && off.abs_diff(self.mtext_click_off) <= 1 =>
                    {
                        (self.mtext_click_count + 1).min(3)
                    }
                    _ => 1,
                };
                self.mtext_click_time = Some(now);
                self.mtext_click_off = off;
                self.mtext_click_count = count;
                match count {
                    2 => self.mtext_select_word(off),
                    3 => self.mtext_select_all(),
                    _ => {
                        if let Some(ed) = self.mtext_editor.as_mut() {
                            ed.sel_anchor = off;
                            ed.sel = Some((off, off));
                            ed.caret = off;
                            ed.caret_blink_on = true;
                        }
                    }
                }
                Task::none()
            }
            Message::MTextSelTo(off) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    let a = ed.sel_anchor;
                    ed.sel = Some((a.min(off), a.max(off)));
                    ed.caret = off;
                    ed.caret_blink_on = true;
                }
                Task::none()
            }
            Message::MTextCaretMove(d) => {
                self.mtext_caret_move(d);
                Task::none()
            }
            Message::MTextCaretBlink => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.caret_blink_on = !ed.caret_blink_on;
                }
                Task::none()
            }
            Message::MTextOk => {
                let committed = self.mtext_commit();
                self.post_editor_closed(committed)
            }
            Message::MTextCancel => {
                self.mtext_cancel();
                self.post_editor_closed(false)
            }

            Message::TextInlineInput(s) => {
                if let Some(ed) = self.text_inline.as_mut() {
                    ed.value = s;
                }
                Task::none()
            }

            // Ctrl+V. The MText editor and (on the web) the TEXT editor read the
            // system clipboard asynchronously — the only paste path that works
            // in the browser, where the synchronous clipboard the iced
            // text_input expects is empty. With no editor open it falls through
            // to the entity paste command.
            Message::PasteShortcut => self.on_paste_shortcut(),

            Message::SelectAllShortcut => {
                let i = self.active_tab;
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    // Ctrl+A in the MText editor selects all of its text.
                    self.mtext_select_all();
                    Task::none()
                } else if self.active_modal == Some(super::ModalKind::Layers) {
                    // Select every row in the Layer Manager (#236).
                    let n = self.tabs[i].layers.layers.len();
                    self.tabs[i].layers.selected_multi = (0..n).collect();
                    self.tabs[i].layers.selected = (n > 0).then_some(0);
                    Task::none()
                } else {
                    self.dispatch_command("SELECTALL")
                }
            }
            Message::MTextPasteClip(text) => {
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    // CR/LF arrive as line breaks; MText keeps "\n", drop "\r".
                    self.mtext_type(&text.replace('\r', ""));
                    self.rebuild_mtext_preview();
                }
                Task::none()
            }
            Message::TextInlinePasteClip(text) => {
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    // Single-line field: collapse newlines, append at the end.
                    let flat = text.replace(['\r', '\n'], " ");
                    if let Some(ed) = self.text_inline.as_mut() {
                        ed.value.push_str(&flat);
                    }
                }
                Task::none()
            }
            Message::TextInlineOk => {
                let committed = self.text_inline_commit();
                self.post_editor_closed(committed)
            }

            Message::DrawOrderSubmenuToggle => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.draworder_submenu = !sel.draworder_submenu;
                Task::none()
            }

            Message::DrawOrderPickRef(above) => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let to_move: Vec<_> = self.tabs[i].scene.selected.iter().cloned().collect();
                if to_move.is_empty() {
                    self.command_line
                        .push_error("DRAWORDER: select entities first.");
                } else {
                    use crate::command::CadCommand;
                    let cmd = super::commands::DrawOrderRefCommand::new(to_move, above);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
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

            Message::InvertSelection => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let count = self.tabs[i].scene.invert_selection();
                self.command_line
                    .push_output(&format!("Invert Selection: {} object(s) selected.", count));
                self.refresh_properties();
                Task::none()
            }

            Message::QSelectOpen => self.on_qselect_open(),

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

            Message::RibbonLayerChanged(layer) => self.on_ribbon_layer_changed(layer),

            Message::RibbonColorChanged(color) => self.on_ribbon_color_changed(color),
            Message::RibbonColorPaletteToggle => {
                self.ribbon.prop_color_palette_open ^= true;
                Task::none()
            }
            Message::RibbonLinetypeChanged(lt) => self.on_ribbon_linetype_changed(lt),
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
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_line_weight(entity, lw);
                        }
                    }
                    // Lineweight is baked into the cached wire geometry —
                    // re-tessellate so the change shows immediately (issue #231
                    // class).
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty = true;
                    self.ribbon.active_lineweight = lw;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::RibbonStyleChanged { key, name } => self.on_ribbon_style_changed(key, name),

            Message::PropLayerChanged(layer) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_common_prop(
                                entity, "layer", &layer,
                            );
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
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
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_color(entity, color);
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
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
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_line_weight(entity, lw);
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
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
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_common_prop(
                                entity, "linetype", &lt,
                            );
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropHatchPatternChanged(name) => self.on_prop_hatch_pattern_changed(name),

            Message::PropBoolToggle(field) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    for &handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            match field {
                                "invisible" => {
                                    crate::scene::view::dispatch::toggle_invisible(entity)
                                }
                                _ => crate::scene::view::dispatch::apply_geom_prop(
                                    entity, field, "toggle",
                                ),
                            }
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropVertexStep(delta) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                // Vertex navigation applies to a single selected polyline.
                let n = if handles.len() == 1 {
                    match self.tabs[i].scene.document.get_entity(handles[0]) {
                        Some(acadrust::EntityType::LwPolyline(p)) => p.vertices.len(),
                        Some(acadrust::EntityType::Polyline2D(p)) => p.vertices.len(),
                        _ => 0,
                    }
                } else {
                    0
                };
                if n > 0 {
                    let cur = self.tabs[i].properties.prop_vertex.min(n - 1) as i64;
                    // Wrap around so ◀ from the first vertex lands on the last.
                    let next = (cur + delta as i64).rem_euclid(n as i64) as usize;
                    self.tabs[i].properties.prop_vertex = next;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropGeomChoiceChanged { field, value } => {
                self.on_prop_geom_choice_changed(field, value)
            }

            Message::PropGeomInput { field, value } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(field.to_string(), value);
                Task::none()
            }

            Message::PropGeomCommit(field) => self.on_prop_geom_commit(field),

            Message::PropAttrInput { tag, value } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(crate::ui::properties::attr_edit_key(&tag), value);
                Task::none()
            }

            Message::PropAttrCommit(tag) => self.on_prop_attr_commit(tag),

            Message::PropColorPickerToggle => {
                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open =
                    !self.tabs[i].properties.color_picker_open;
                if self.tabs[i].properties.color_picker_open {
                    self.tabs[i].properties.color_palette_open = false;
                }
                Task::none()
            }

            Message::PropBgColorPickerToggle => {
                let i = self.active_tab;
                self.tabs[i].properties.bg_color_picker_open =
                    !self.tabs[i].properties.bg_color_picker_open;
                Task::none()
            }

            Message::PropBgColorChanged(color) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    for &handle in &handles {
                        match self.tabs[i].scene.document.get_entity_mut(handle) {
                            Some(acadrust::EntityType::MText(m)) => {
                                m.background_color = color.clone();
                                // Picking a colour turns the background on in Fill
                                // mode (specific colour), preserving the frame bit.
                                m.background_fill_flags =
                                    (m.background_fill_flags & !0x02) | 0x01;
                            }
                            Some(acadrust::EntityType::Hatch(h)) => {
                                crate::entities::hatch::set_background_color(h, &color);
                            }
                            _ => {}
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].properties.bg_color_picker_open = false;
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropColorFieldToggle(field) => {
                let i = self.active_tab;
                let p = &mut self.tabs[i].properties;
                p.open_color_field = if p.open_color_field.as_deref() == Some(field.as_str()) {
                    None
                } else {
                    Some(field)
                };
                Task::none()
            }

            Message::PropColorFieldChanged { field, color } => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "CHPROP");
                    let idx = if field == "gradient_color_2" { 1 } else { 0 };
                    for &handle in &handles {
                        if let Some(acadrust::EntityType::Hatch(h)) =
                            self.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            while h.gradient_color.colors.len() <= idx {
                                let value = if h.gradient_color.colors.is_empty() {
                                    0.0
                                } else {
                                    1.0
                                };
                                h.gradient_color.colors.push(
                                    acadrust::entities::hatch::GradientColorEntry {
                                        value,
                                        color: acadrust::types::Color::Index(7),
                                    },
                                );
                            }
                            h.gradient_color.colors[idx].color = color.clone();
                        }
                    }
                    // Rebuild hatch seeds so the gradient fill picks up the new
                    // colour (synced_hatch_models only patches the main colour).
                    self.tabs[i].scene.populate_hatches_from_document();
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].properties.open_color_field = None;
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropColorPickerClose => {
                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open = false;
                self.tabs[i].properties.color_palette_open = false;
                Task::none()
            }

            Message::PropColorPaletteToggle => {
                self.tabs[self.active_tab].properties.color_palette_open =
                    !self.tabs[self.active_tab].properties.color_palette_open;
                Task::none()
            }

            Message::LayoutSwitch(name) => {
                self.layout_list_open = false;
                self.on_layout_switch(name)
            }

            Message::LayoutCreate => self.on_layout_create(),

            Message::LayoutDelete(name) => {
                let i = self.active_tab;
                self.push_undo_snapshot(i, "LAYOUT DEL");
                if self.tabs[i].scene.delete_layout(&name) {
                    self.layout_context_menu = None;
                    self.layout_rename_state = None;
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
                    // Focus the inline field so the user types into it
                    // directly instead of the command line (issue #86).
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        crate::ui::statusbar::LAYOUT_RENAME_INPUT_ID,
                    ));
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

            Message::LayoutRenameCommit => self.on_layout_rename_commit(),

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
                self.active_modal = Some(super::ModalKind::LayoutManager);
                Task::none()
            }
            Message::LayoutManagerClose => {
                self.close_active_modal();
                Task::none()
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
                        self.tabs[i].scene.set_current_layout(new_name.clone());
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
                        self.tabs[i].scene.set_current_layout("Model".to_string());
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
                self.tabs[i].scene.set_current_layout(name.clone());
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
                self.active_modal = Some(super::ModalKind::Shortcuts);
                Task::none()
            }
            Message::ShortcutsPanelClose => {
                self.close_active_modal();
                Task::none()
            }

            // ── Command Alias Editor (ALIASEDIT) ──────────────────────────────
            Message::AliasEditorOpen => {
                // Seed the working buffer from the current table, sorted by alias
                // so the list is stable and diffable.
                let mut rows: Vec<(String, String)> = self
                    .command_aliases
                    .iter()
                    .map(|(a, c)| (a.clone(), c.clone()))
                    .collect();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                self.alias_editor_rows = rows;
                self.active_modal = Some(super::ModalKind::Aliases);
                Task::none()
            }
            Message::AliasEditorInput { idx, field, value } => {
                use crate::ui::window::alias_editor::AliasField;
                // Aliases and commands are stored uppercase; uppercasing as the
                // user types keeps display and the committed table consistent.
                let value = value.to_uppercase();
                if let Some(rowdata) = self.alias_editor_rows.get_mut(idx) {
                    match field {
                        AliasField::Alias => rowdata.0 = value,
                        AliasField::Command => rowdata.1 = value,
                    }
                }
                Task::none()
            }
            Message::AliasEditorAdd => {
                self.alias_editor_rows.push((String::new(), String::new()));
                Task::none()
            }
            Message::AliasEditorRemove(idx) => {
                if idx < self.alias_editor_rows.len() {
                    self.alias_editor_rows.remove(idx);
                }
                Task::none()
            }
            Message::AliasEditorApply => {
                self.apply_alias_editor_rows();
                self.command_line.push_info(&format!(
                    "{} alias(es) applied.",
                    self.command_aliases.len()
                ));
                Task::none()
            }

            // ── About window ──────────────────────────────────────────────
            Message::AboutOpen => {
                self.active_modal = Some(super::ModalKind::About);
                Task::none()
            }

            Message::CloseModal => {
                self.close_active_modal();
                Task::none()
            }
            Message::AttrEditorOpen(handle) => {
                self.open_attribute_editor(handle);
                Task::none()
            }
            Message::AttrEditorTab(t) => {
                self.attr_editor_tab = t;
                Task::none()
            }
            Message::AttrEditorSelect(idx) => {
                if idx < self.attr_editor_rows.len() {
                    self.attr_editor_selected = idx;
                }
                Task::none()
            }
            Message::AttrEditorInput { idx, value } => {
                if let Some(r) = self.attr_editor_rows.get_mut(idx) {
                    r.value = value;
                }
                Task::none()
            }
            Message::AttrEditorTextStyle(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.text_style = s;
                }
                Task::none()
            }
            Message::AttrEditorJustify(label) => {
                if let Some((h, v)) =
                    crate::ui::window::attribute_editor::justify_from_label(&label)
                {
                    if let Some(r) = self.attr_row_selected_mut() {
                        r.h_align = h;
                        r.v_align = v;
                    }
                }
                Task::none()
            }
            Message::AttrEditorHeight(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.height = s;
                }
                Task::none()
            }
            Message::AttrEditorRotation(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.rotation = s;
                }
                Task::none()
            }
            Message::AttrEditorWidth(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.width_factor = s;
                }
                Task::none()
            }
            Message::AttrEditorOblique(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.oblique = s;
                }
                Task::none()
            }
            Message::AttrEditorBackwards(b) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.backwards = b;
                }
                Task::none()
            }
            Message::AttrEditorUpsideDown(b) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.upside_down = b;
                }
                Task::none()
            }
            Message::AttrEditorLayer(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.layer = s;
                }
                Task::none()
            }
            Message::AttrEditorLinetype(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.linetype = if s == "ByLayer" { String::new() } else { s };
                }
                Task::none()
            }
            Message::AttrEditorColor(label) => {
                if let Some(c) = crate::ui::window::attribute_editor::color_from_label(&label) {
                    if let Some(r) = self.attr_row_selected_mut() {
                        r.color = c;
                    }
                }
                Task::none()
            }
            Message::AttrEditorLineweight(lw) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.line_weight = lw;
                }
                Task::none()
            }
            Message::AttrEditorApply => self.on_attr_editor_apply(),
            Message::ModalGrab => {
                // Start a drag; the first ModalDragMove seeds the reference.
                self.modal_dragging = true;
                self.modal_drag_last = None;
                Task::none()
            }
            Message::ModalDragMove(p) => {
                if self.modal_dragging {
                    if let Some(last) = self.modal_drag_last {
                        self.modal_offset.x += p.x - last.x;
                        self.modal_offset.y += p.y - last.y;
                        // Clamp so the dialog stops at the window edge instead
                        // of being squeezed (the off-centre padding shrinks the
                        // dialog once it overlaps a border).
                        if let Some((cw, ch)) = self.modal_outer_size() {
                            let ww = self.vp_size.0 + 440.0;
                            let wh = self.vp_size.1;
                            let max_x = ((ww - cw) * 0.5).max(0.0);
                            let max_y = ((wh - ch) * 0.5).max(0.0);
                            self.modal_offset.x = self.modal_offset.x.clamp(-max_x, max_x);
                            self.modal_offset.y = self.modal_offset.y.clamp(-max_y, max_y);
                        }
                    }
                    self.modal_drag_last = Some(p);
                }
                Task::none()
            }
            Message::ModalDragRelease => {
                self.modal_dragging = false;
                self.modal_drag_last = None;
                Task::none()
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

            // ── Plugin Manager window ─────────────────────────────────────
            Message::PluginManagerOpen => {
                // Refresh the on-disk external-plugin list each time the manager
                // opens so newly dropped-in packages show up.
                self.external_plugins = crate::plugin::external::discover();
                self.active_modal = Some(super::ModalKind::PluginManager);
                // Fetch the curated registry and release lists for linked repos.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let mut tasks = vec![self.fetch_registry_task()];
                    tasks.extend(
                        self.plugin_repos
                            .clone()
                            .into_iter()
                            .map(|r| self.fetch_releases_task(r)),
                    );
                    return Task::batch(tasks);
                }
                #[cfg(target_arch = "wasm32")]
                Task::none()
            }
            Message::PluginManagerClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::SetPluginEnabled(id, enabled) => {
                if enabled {
                    self.disabled_plugins.remove(&id);
                } else {
                    self.disabled_plugins.insert(id);
                }
                self.rebuild_ribbon_modules();
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::PluginRepoInput(s) => {
                self.plugin_repo_input = s;
                Task::none()
            }
            Message::PluginRepoAdd => {
                let repo = self
                    .plugin_repo_input
                    .trim()
                    .trim_start_matches("https://github.com/")
                    .trim_end_matches('/')
                    .to_string();
                if repo.is_empty() || self.plugin_repos.contains(&repo) {
                    return Task::none();
                }
                self.plugin_repos.push(repo.clone());
                self.plugin_repo_input.clear();
                self.persist_settings_if_changed();
                self.marketplace_status = format!("Fetching releases for {repo}…");
                self.fetch_releases_task(repo)
            }
            Message::PluginRepoRemove(repo) => {
                self.plugin_repos.retain(|r| r != &repo);
                self.repo_release_tags.remove(&repo);
                self.repo_selected_tag.remove(&repo);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::PluginRegistryFetched(Ok(entries)) => {
                // Fetch releases for every curated repo so the dropdowns fill in.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let tasks: Vec<_> = entries
                        .iter()
                        .map(|e| self.fetch_releases_task(e.repo.clone()))
                        .collect();
                    self.plugin_registry = entries;
                    return Task::batch(tasks);
                }
                #[cfg(target_arch = "wasm32")]
                {
                    self.plugin_registry = entries;
                    Task::none()
                }
            }
            Message::PluginRegistryFetched(Err(e)) => {
                self.marketplace_status = format!("Registry: {e}");
                Task::none()
            }
            Message::PatronsFetched(Ok(names)) => {
                // Merge the hand-maintained supporters and rank everyone by
                // amount (also sorts the web list, which arrives unsorted).
                self.patrons = crate::patreon::merge_manual(names);
                Task::none()
            }
            // No token / offline: still show any hand-maintained supporters
            // (Start page shows a "Support on Patreon" prompt when empty).
            Message::PatronsFetched(Err(_)) => {
                self.patrons = crate::patreon::merge_manual(Vec::new());
                Task::none()
            }
            Message::PluginReleasesFetched(repo, Ok(tags)) => {
                if let Some(first) = tags.first() {
                    self.repo_selected_tag
                        .entry(repo.clone())
                        .or_insert_with(|| first.clone());
                }
                self.marketplace_status = format!("{repo}: {} installable release(s)", tags.len());
                self.repo_release_tags.insert(repo, tags);
                Task::none()
            }
            Message::PluginReleasesFetched(repo, Err(e)) => {
                self.marketplace_status = format!("{repo}: {e}");
                Task::none()
            }
            Message::PluginReleaseSelect(repo, tag) => {
                self.repo_selected_tag.insert(repo, tag);
                Task::none()
            }
            Message::PluginInstall(repo) => {
                let Some(tag) = self.repo_selected_tag.get(&repo).cloned() else {
                    return Task::none();
                };
                self.marketplace_status = format!("Installing {repo} {tag}…");
                self.install_task(repo, tag)
            }
            Message::PluginInstalled(Ok(id)) => {
                self.marketplace_status = format!("Installed '{id}'. Restart to load it.");
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.external_plugins = crate::plugin::external::discover();
                }
                Task::none()
            }
            Message::PluginInstalled(Err(e)) => {
                self.marketplace_status = format!("Install failed: {e}");
                Task::none()
            }
            Message::PluginUninstall(id) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    match crate::plugin::external::uninstall(&id) {
                        Ok(()) => {
                            self.marketplace_status =
                                format!("Uninstalled '{id}'. Restart to unload it.");
                            self.external_plugins = crate::plugin::external::discover();
                        }
                        Err(e) => {
                            self.marketplace_status = format!("Uninstall failed: {e}");
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                let _ = id;
                Task::none()
            }
            Message::PointStyleSetMode(mode) => {
                self.set_point_mode_bits(!0, mode);
                Task::none()
            }
            Message::PointStyleSizeRelative(relative) => {
                self.point_size_relative = relative;
                self.apply_point_size();
                Task::none()
            }
            Message::PointStyleSizeInput(s) => {
                self.point_size_buf = s;
                Task::none()
            }
            Message::PointStyleApplySize => {
                self.apply_point_size();
                Task::none()
            }
            Message::PointStyleOk => {
                self.apply_point_size();
                self.close_active_modal();
                Task::none()
            }

            Message::EnterViewport(handle) => {
                let i = self.active_tab;
                // Clear paper-space selection before entering model space.
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.active_viewport = Some(handle);
                // Fold a stale UTM saved view onto the effective (auto-fit)
                // centre so pan/zoom, paper↔model and the display all agree —
                // otherwise the camera auto-fits to the model while the cursor
                // math stays at the origin, jittering as pan toggles the two.
                self.tabs[i].scene.normalize_active_viewport_view();
                // Grid/snap follow the entered viewport.
                self.adopt_view_display(i);
                // Adopt the entered viewport's own per-viewport UCS.
                self.tabs[i].refresh_active_ucs();
                self.refresh_properties();
                self.command_line.push_output("MSPACE");
                Task::none()
            }

            Message::ExitViewport => {
                let i = self.active_tab;
                // Clear model-space selection before returning to paper space.
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.active_viewport = None;
                // Grid/snap return to the paper sheet's own state.
                self.adopt_view_display(i);
                // Paper space has no UCS — drop the viewport's UCS.
                self.tabs[i].refresh_active_ucs();
                self.refresh_properties();
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

            Message::UnsavedDialogDiscard => self.on_unsaved_dialog_discard(),

            Message::UnsavedDialogSave => self.on_unsaved_dialog_save(),

            Message::AecDropSameVersion => self.on_aec_drop_same_version(),
            Message::AecDropProceed => self.on_aec_drop_proceed(),
            Message::AecDropBack => {
                self.active_modal = Some(crate::app::ModalKind::SaveDialog);
                Task::none()
            }

            Message::AutoSave => self.on_autosave(),

            Message::UnsavedPickedSavePath(Some(path)) => {
                self.on_unsaved_picked_save_path_some(path)
            }

            Message::UnsavedPickedSavePath(None) => {
                // User cancelled the save-as dialog — re-open the confirmation dialog.
                if self.pending_close.is_some() {
                    return self.open_unsaved_dialog_window();
                }
                Task::none()
            }

            // ── Page Setup ────────────────────────────────────────────────
            Message::UpdateCheckResult(latest) => {
                let Some(info) = latest else {
                    return Task::none();
                };
                self.update_notice_version = Some(info.version);
                self.update_notice_body = Some(info.body);
                self.active_modal = Some(super::ModalKind::UpdateNotice);
                Task::none()
            }
            Message::UpdateNoticeClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::UpdateNoticeOpenRelease => {
                crate::sys::open_url(crate::io::update_check::RELEASES_PAGE);
                self.close_active_modal();
                Task::none()
            }
            Message::AssocPromptYes => {
                self.file_assoc_enabled = true;
                self.mark_assoc_prompted();
                self.active_modal = None;
                // set_default_app registers the handler first, then makes us the
                // default — boot no longer does this automatically.
                Task::perform(
                    crate::io::file_association::set_default_app(),
                    Message::AssocResult,
                )
            }
            Message::AssocPromptNo => {
                self.file_assoc_enabled = false;
                self.mark_assoc_prompted();
                self.active_modal = None;
                Task::none()
            }
            Message::AssocResult(result) => {
                match result {
                    Ok(msg) => self.command_line.push_info(&msg),
                    Err(err) => self
                        .command_line
                        .push_error(&format!("Could not set default app: {err}")),
                }
                Task::none()
            }
            Message::PlotDialogOpen => self.on_plot_dialog_open(),
            Message::PlotDlg(m) => self.on_plot_dlg(m),

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
            Message::PlotExportPath(Some(path)) => self.on_plot_export_path_some(path),

            Message::PlotFormat(f) => {
                self.plot_format = f;
                Task::none()
            }
            Message::PlotOrientation(o) => {
                self.plot_orientation = o;
                Task::none()
            }
            Message::PlotWindowExport => {
                let i = self.active_tab;
                let stem = self.tabs[i]
                    .current_path
                    .as_deref()
                    .and_then(|p: &std::path::Path| p.file_stem())
                    .map(|s: &std::ffi::OsStr| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".into());
                Task::perform(
                    crate::io::pdf_export::pick_pdf_path_owned(stem),
                    Message::PlotWindowExportPath,
                )
            }
            Message::PlotWindowExportPath(None) => Task::none(),
            Message::PlotWindowExportPath(Some(path)) => self.on_plot_window_export_path_some(path),

            // ── Print to system printer ───────────────────────────────────────
            Message::PrintToPrinter => self.on_print_to_printer(),
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
                self.active_modal = Some(super::ModalKind::Plotstyle);
                Task::none()
            }
            Message::PlotStylePanelClose => {
                self.close_active_modal();
                Task::none()
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

            Message::PlotStylePanelApply => self.on_plot_style_panel_apply(),

            Message::PlotStylePanelSave => self.on_plot_style_panel_save(),

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
            Message::TextStyleDialogOpen => self.on_text_style_dialog_open(),
            Message::TextStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::TextStyleDialogSelect(name) => {
                let i = self.active_tab;
                self.textstyle_selected = name;
                self.load_textstyle_bufs(i);
                Task::none()
            }
            Message::TextStyleDialogSetCurrent => {
                // Staged: persists on Apply.
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                if self.tabs[i].scene.document.text_styles.get(&name).is_some() {
                    self.tabs[i].scene.document.header.current_text_style_name = name.clone();
                    self.sync_ribbon_styles();
                    self.command_line
                        .push_output(&format!("Current text style: {}", name));
                }
                Task::none()
            }
            Message::TextStyleDialogNew => {
                self.style_new(crate::app::StyleKind::Text);
                Task::none()
            }
            Message::TextStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::Text);
                Task::none()
            }
            Message::TextStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::Text);
                Task::none()
            }
            // ── Shared inline rename (all style managers) ─────────────────
            Message::StyleRenameStart(kind, name) => {
                self.style_rename_start(kind, name);
                // Focus the freshly-shown rename field so the user can type
                // immediately after the double click.
                iced::widget::operation::focus(crate::ui::style::style_list::rename_input_id())
            }
            Message::StyleRenameEdit(s) => {
                self.style_rename_buf = s;
                Task::none()
            }
            Message::StyleRenameCommit(kind) => {
                self.style_rename_commit(kind);
                Task::none()
            }
            Message::StyleRenameCancel => {
                self.style_rename_cancel();
                Task::none()
            }
            Message::TextStyleEdit { field, value } => {
                match field {
                    "font" => self.textstyle_font = value,
                    "width" => self.textstyle_width = value,
                    "oblique" => self.textstyle_oblique = value,
                    "height" => self.textstyle_height = value,
                    "bigfont" => self.textstyle_bigfont = value,
                    "ttf" => self.textstyle_ttf = value,
                    _ => {}
                }
                Task::none()
            }
            Message::TextStyleToggle(field) => {
                // Staged: mutate live for preview, persist on Apply.
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                if let Some(s) = self.tabs[i].scene.document.text_styles.get_mut(&name) {
                    match field {
                        "backward" => s.flags.backward = !s.flags.backward,
                        "upside_down" => s.flags.upside_down = !s.flags.upside_down,
                        "annotative" => s.annotative = !s.annotative,
                        _ => {}
                    }
                }
                Task::none()
            }
            Message::TextStyleApply => self.on_text_style_apply(),
            Message::TextStyleFontPick(font_file) => {
                // Staged: update the buffer + live style; persist on Apply.
                let i = self.active_tab;
                self.textstyle_font = font_file.clone();
                let name = self.textstyle_selected.clone();
                if let Some(s) = self.tabs[i].scene.document.text_styles.get_mut(&name) {
                    s.font_file = font_file;
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
                self.load_tablestyle_bufs(i);
                self.active_modal = Some(super::ModalKind::TableStyle);
                self.style_stage_begin();
                Task::none()
            }
            Message::TableStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::TableStyleDialogSelect(name) => {
                self.tablestyle_selected = name;
                let i = self.active_tab;
                self.load_tablestyle_bufs(i);
                Task::none()
            }

            Message::TableStyleEdit { field, value } => {
                match field {
                    "hmargin" => self.ts_hmargin = value,
                    "vmargin" => self.ts_vmargin = value,
                    "description" => self.ts_description = value,
                    _ => {}
                }
                Task::none()
            }

            Message::TableStyleApply => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                let h: Option<f64> = self.ts_hmargin.trim().parse().ok();
                let v: Option<f64> = self.ts_vmargin.trim().parse().ok();
                let desc = self.ts_description.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let ObjectType::TableStyle(s) = obj {
                        if s.name == name {
                            if let Some(h) = h {
                                s.horizontal_margin = h;
                            }
                            if let Some(v) = v {
                                s.vertical_margin = v;
                            }
                            s.description = desc.clone();
                        }
                    }
                }
                self.style_stage_commit();
                Task::none()
            }

            Message::TableStyleSetFlow(value) => {
                use acadrust::objects::TableFlowDirection;
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    s.flow_direction = match value.as_str() {
                        "Up" => TableFlowDirection::Up,
                        _ => TableFlowDirection::Down,
                    };
                }
                Task::none()
            }

            Message::TableColorMore(row, field) => {
                self.ts_color_open = if self.ts_color_open == Some((row, field)) {
                    None
                } else {
                    Some((row, field))
                };
                Task::none()
            }
            Message::TableStyleCellEdit { row, field, value } => {
                self.ts_color_open = None;
                let r = row as usize;
                if r < 3 {
                    match field {
                        "textstyle" => self.ts_cell_textstyle[r] = value,
                        "height" => self.ts_cell_height[r] = value,
                        "textcolor" => self.ts_cell_textcolor[r] = value,
                        "fillcolor" => self.ts_cell_fillcolor[r] = value,
                        "datatype" => self.ts_cell_datatype[r] = value,
                        "unittype" => self.ts_cell_unittype[r] = value,
                        "format" => self.ts_cell_format[r] = value,
                        _ => {}
                    }
                }
                Task::none()
            }

            Message::TableStyleBorderEdit {
                cell,
                border,
                field,
                value,
            } => {
                let (c, b) = (cell as usize, border as usize);
                if c < 3 && b < 6 {
                    match field {
                        "lw" => self.ts_border_lw[c][b] = value,
                        "color" => self.ts_border_color[c][b] = value,
                        "spacing" => self.ts_border_spacing[c][b] = value,
                        _ => {}
                    }
                }
                Task::none()
            }

            Message::TableStyleBorderSetType {
                cell,
                border,
                value,
            } => {
                use acadrust::objects::TableBorderType;
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    if let Some(bd) =
                        Self::ts_cell_of(s, cell).and_then(|c| Self::ts_border_of(c, border))
                    {
                        bd.border_type = match value.as_str() {
                            "Double" => TableBorderType::Double,
                            _ => TableBorderType::Single,
                        };
                    }
                }
                Task::none()
            }

            Message::TableStyleBorderToggleInvisible { cell, border } => {
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    if let Some(bd) =
                        Self::ts_cell_of(s, cell).and_then(|c| Self::ts_border_of(c, border))
                    {
                        bd.is_invisible = !bd.is_invisible;
                    }
                }
                Task::none()
            }

            Message::TableStyleCellToggleFill(row) => {
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    if let Some(c) = Self::ts_cell_of(s, row) {
                        c.fill_enabled = !c.fill_enabled;
                    }
                }
                Task::none()
            }

            Message::TableStyleCellSetAlign { row, value } => {
                use acadrust::objects::CellAlignment;
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    if let Some(c) = Self::ts_cell_of(s, row) {
                        c.alignment = match value.as_str() {
                            "TopLeft" => CellAlignment::TopLeft,
                            "TopCenter" => CellAlignment::TopCenter,
                            "TopRight" => CellAlignment::TopRight,
                            "MiddleLeft" => CellAlignment::MiddleLeft,
                            "MiddleRight" => CellAlignment::MiddleRight,
                            "BottomLeft" => CellAlignment::BottomLeft,
                            "BottomCenter" => CellAlignment::BottomCenter,
                            "BottomRight" => CellAlignment::BottomRight,
                            _ => CellAlignment::MiddleCenter,
                        };
                    }
                }
                Task::none()
            }

            Message::TableStyleCellApply(row) => self.on_table_style_cell_apply(row),

            Message::TableStyleToggle(field) => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let ObjectType::TableStyle(s) = obj {
                        if s.name == name {
                            match field {
                                "title_sup" => s.title_suppressed = !s.title_suppressed,
                                "header_sup" => s.header_suppressed = !s.header_suppressed,
                                _ => {}
                            }
                        }
                    }
                }
                Task::none()
            }

            Message::TableStyleToggleAnnotative => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let ObjectType::TableStyle(s) = obj {
                        if s.name == name {
                            s.annotative = !s.annotative;
                        }
                    }
                }
                Task::none()
            }

            Message::TableStyleDialogNew => {
                self.style_new(crate::app::StyleKind::Table);
                Task::none()
            }
            Message::TableStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::Table);
                Task::none()
            }
            Message::TableStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::Table);
                Task::none()
            }
            Message::TableStyleDialogSetCurrent => {
                // Staged: persists on Apply. The header field is the round-trip
                // source of truth ($CTABLESTYLE); the ribbon mirrors it.
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                if self
                    .style_names(crate::app::StyleKind::Table)
                    .contains(&name)
                {
                    self.tabs[i].scene.document.header.current_table_style_name = name.clone();
                    self.ribbon.active_table_style = name.clone();
                    self.command_line
                        .push_output(&format!("Current table style: {name}"));
                }
                Task::none()
            }

            // ── MLineStyle Dialog ─────────────────────────────────────────────
            Message::MlStyleDialogOpen => self.on_ml_style_dialog_open(),
            Message::MlStyleDialogClose => {
                self.close_active_modal();
                Task::none()
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
                    // Staged: persists on Apply.
                    self.tabs[i].scene.document.header.multiline_style = name.clone();
                    self.command_line
                        .push_output(&format!("Current multiline style: {}", name));
                }
                Task::none()
            }
            // Placeholder so the Multiline manager has the same Set Current +
            // Apply pair as every other style manager. The editor is currently
            // read-only, so there is nothing to apply yet — wire this up when
            // editable MLineStyle properties land.
            Message::MlStyleApply => {
                // Multiline styles have no editable properties yet; Apply still
                // commits any staged structural / current-style changes.
                self.style_stage_commit();
                Task::none()
            }
            Message::MlStyleDialogNew => {
                self.style_new(crate::app::StyleKind::MLine);
                Task::none()
            }
            Message::MlStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::MLine);
                Task::none()
            }
            Message::MlStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::MLine);
                Task::none()
            }

            // ── MLeaderStyle Dialog ───────────────────────────────────────────
            Message::MLeaderStyleDialogOpen => self.on_mleader_style_dialog_open(),
            Message::MLeaderStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::MLeaderStyleDialogSelect(name) => {
                self.mleaderstyle_selected = name;
                let i = self.active_tab;
                self.load_mleaderstyle_bufs(i);
                Task::none()
            }
            Message::MLeaderStyleDialogSetCurrent => self.on_mleader_style_dialog_set_current(),
            Message::MLeaderStyleDialogNew => {
                self.style_new(crate::app::StyleKind::MLeader);
                Task::none()
            }
            Message::MLeaderStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::MLeader);
                Task::none()
            }
            Message::MLeaderStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::MLeader);
                Task::none()
            }
            Message::MLeaderStyleEdit { field, value } => self.on_mleader_style_edit(field, value),
            Message::MLeaderStyleToggle(field) => {
                let i = self.active_tab;
                if let Some(s) = self.mleaderstyle_mut(i) {
                    match field {
                        "enable_landing" => s.enable_landing = !s.enable_landing,
                        "enable_dogleg" => s.enable_dogleg = !s.enable_dogleg,
                        "text_frame" => s.text_frame = !s.text_frame,
                        "text_always_left" => s.text_always_left = !s.text_always_left,
                        "annotative" => s.is_annotative = !s.is_annotative,
                        "enable_block_scale" => s.enable_block_scale = !s.enable_block_scale,
                        "enable_block_rotation" => {
                            s.enable_block_rotation = !s.enable_block_rotation
                        }
                        _ => {}
                    }
                }
                Task::none()
            }
            Message::MLeaderColorMore(field) => {
                self.mls_color_open = if self.mls_color_open == Some(field) {
                    None
                } else {
                    Some(field)
                };
                Task::none()
            }
            Message::MLeaderStyleSetEnum { field, value } => {
                self.on_mleader_style_set_enum(field, value)
            }
            Message::MLeaderStyleSetHandle { field, value } => {
                self.on_mleader_style_set_handle(field, value)
            }
            Message::MLeaderStyleApply => self.on_mleader_style_apply(),

            // ── DimStyle Dialog ───────────────────────────────────────────────
            Message::DimStyleDialogOpen => self.on_dim_style_dialog_open(),
            Message::DimStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::DimStyleDialogApply => {
                let i = self.active_tab;
                self.apply_dimstyle_bufs(i);
                self.style_stage_commit();
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
                self.style_new(crate::app::StyleKind::Dim);
                Task::none()
            }
            Message::DimStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::Dim);
                Task::none()
            }
            Message::DimStyleDialogSetCurrent => {
                // Staged: persists on Apply.
                let i = self.active_tab;
                self.tabs[i].scene.document.header.current_dimstyle_name =
                    self.dimstyle_selected.clone();
                self.sync_ribbon_styles();
                self.command_line.push_output(&format!(
                    "Current dim style set to '{}'.",
                    self.dimstyle_selected
                ));
                Task::none()
            }
            Message::DimStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::Dim);
                Task::none()
            }
            Message::DsEdit(field, val) => {
                self.apply_ds_edit(field, val);
                self.ds_color_open = None;
                Task::none()
            }
            Message::DsToggle(field) => {
                self.apply_ds_toggle(field);
                Task::none()
            }
            Message::DsColorMore(field) => {
                self.ds_color_open = if self.ds_color_open.as_ref() == Some(&field) {
                    None
                } else {
                    Some(field)
                };
                Task::none()
            }
            Message::OpenColorWindow(target) => {
                self.color_pick_target = Some(target);
                self.ds_color_open = None;
                self.mls_color_open = None;
                self.ts_color_open = None;
                self.ribbon.close_dropdown();
                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open = false;
                self.tabs[i].layers.color_picker_row = None;
                // Shown as a nested modal over the active dialog (Plan B):
                // `color_pick_target.is_some()` drives the overlay in view_main.
                Task::none()
            }
            Message::CloseColorPicker => {
                self.color_pick_target = None;
                Task::none()
            }
            Message::ColorWindowPick(color) => self.on_color_window_pick(color),
            Message::DsSetHandle { field, value } => self.on_ds_set_handle(field, value),
        }
    }
}
