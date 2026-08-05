//! `dialog` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use crate::ui::window::block_palette::BlockPaletteMsg;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    ortho_constrain, parse_coord, polar_constrain_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
    CoordKind,
};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::model::object::GripApply;
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};


impl OpenCADStudio {
    pub(in crate::app) fn open_save_dialog_window(&mut self, tab_idx: usize) -> Task<Message> {
        // Default the format dropdown to the loaded file's own format — its
        // DWG-vs-DXF kind (from the extension) and its version (from the parsed
        // document) — so Save-As round-trips the format instead of silently
        // re-targeting it. A new/unsaved drawing has no source format, so it
        // uses the application-wide default chosen in Options (#529).
        self.save_dialog_format = if let Some(path) = &self.tabs[tab_idx].current_path {
            let document = &self.tabs[tab_idx].scene.document;
            let is_dxf = crate::io::source_is_dxf(Some(path), document);
            let version = if is_dxf {
                document.version
            } else {
                document.dwg_source_version.unwrap_or(document.version)
            };
            crate::io::format_for_version(version, is_dxf)
        } else {
            self.default_save_format.clone()
        };

        // Pre-fill the default file name from the current path or the tab name;
        // the destination folder comes from the native OS dialog that follows.
        if let Some(p) = &self.tabs[tab_idx].current_path.clone() {
            if let Some(name) = p.file_name() {
                self.save_dialog_filename = name.to_string_lossy().into_owned();
            }
        } else {
            let (ext, _) = crate::io::parse_save_format(&self.save_dialog_format);
            self.save_dialog_filename = format!("{}.{ext}", self.tabs[tab_idx].tab_display_name());
        }
        if self.tabs[tab_idx].recovery_save_as_required {
            let (ext, _) = crate::io::parse_save_format(&self.save_dialog_format);
            let stem = std::path::Path::new(&self.save_dialog_filename)
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.tabs[tab_idx].tab_display_name());
            self.save_dialog_filename = format!("{stem}_recovered.{ext}");
        }
        self.aec_drop_acknowledged = false;
        self.active_modal = Some(crate::app::ModalKind::SaveDialog);
        Task::none()
    }


    pub(in crate::app) fn close_save_dialog_window(&mut self) -> Task<Message> {
        self.aec_drop_acknowledged = false;
        if self.active_modal == Some(crate::app::ModalKind::SaveDialog) {
            self.active_modal = None;
            self.reset_modal_geometry();
        }
        Task::none()
    }


    pub(in crate::app) fn open_unsaved_dialog_window(&mut self) -> Task<Message> {
        self.active_modal = Some(crate::app::ModalKind::Unsaved);
        // The unsaved-changes prompt renders inside the main window, so bring
        // that window to the foreground — a close signal can arrive while the
        // app is backgrounded, leaving the prompt unseen behind other windows.
        // `gain_focus` alone is ignored by most Linux WMs (focus-stealing
        // prevention), so pair it with an urgency hint so the window is at
        // least flagged for attention when the compositor blocks the raise.
        match self.main_window {
            Some(id) => Task::batch([
                iced::window::gain_focus(id),
                iced::window::request_user_attention(
                    id,
                    Some(iced::window::UserAttention::Critical),
                ),
            ]),
            None => Task::none(),
        }
    }


    pub(in crate::app) fn close_unsaved_dialog_window(&mut self) -> Task<Message> {
        if self.active_modal == Some(crate::app::ModalKind::Unsaved) {
            self.active_modal = None;
            self.reset_modal_geometry();
        }
        Task::none()
    }


pub(super) fn on_ribbon_tool_click(&mut self, tool_id: String, event: ModuleEvent) -> Task<Message> {
                // On the Start page there is no drawing to act on, so a tool that
                // touches the scene is inert — point the user at New / Open
                // instead of running it into the empty welcome tab (#299).
                //
                // Commands are exempt: `dispatch_command` already decides which
                // ones stand alone (About, Donate, Report, the web link…) and
                // reports the rest. Refusing them here shadowed that list and
                // killed the welcome page's own buttons, which by definition can
                // only ever be clicked while `is_start` holds (#388, #389).
                // Keep the policy in one place — this door must not second-guess
                // it. Every other event below mutates the scene or its panels.
                if self.tabs[self.active_tab].is_start && !matches!(event, ModuleEvent::Command(_)) {
                    self.ribbon.close_dropdown();
                    self.command_line
                        .push_info(crate::t!("No drawing open — use New or Open first.").as_ref());
                    return Task::none();
                }
                // Dismiss any open dropdown / collapsed-panel flyout on tool use,
                // and remember this tool as its panel's last-used one.
                self.ribbon.close_dropdown();
                self.ribbon.note_panel_tool(&tool_id);
                self.ribbon.activate_tool(&tool_id);
                match event {
                    ModuleEvent::Command(cmd) => {
                        let task = self.dispatch_command(&cmd);
                        // One-shot tools (view changes, clipboard, toggles,
                        // audits…) leave nothing running: no interactive
                        // command and no dialog. Their highlight would stick
                        // forever — turn it off now. Interactive commands and
                        // dialog owners keep theirs; the command end / modal
                        // close clears those. (#355)
                        let i = self.active_tab;
                        if self.tabs[i].active_cmd.is_none()
                            && self.active_modal.is_none()
                            && !self.tabs[i].pan_mode
                            && !self.tabs[i].orbit_mode
                        {
                            self.ribbon.deactivate_tool();
                        }
                        return task;
                    }
                    ModuleEvent::OpenFileDialog => {
                        self.command_line
                            .push_info(crate::t!("Open DWG/DXF: not yet implemented.").as_ref());
                    }
                    ModuleEvent::ClearModels => {
                        let i = self.active_tab;
                        self.tabs[i].scene.clear();
                        self.tabs[i].properties = PropertiesPanel::empty();
                        self.command_line.push_output(crate::t!("Scene cleared.").as_ref());
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
                    ModuleEvent::PluginFileDialog {
                        command,
                        title,
                        filter_name,
                        extensions,
                    } => {
                        return Task::perform(
                            async move {
                                let exts: Vec<&str> =
                                    extensions.iter().map(|s| s.as_str()).collect();
                                let path = crate::sys::file_dialog()
                                    .set_title(title)
                                    .add_filter(filter_name, &exts)
                                    .add_filter("All Files", &["*"])
                                    .pick_file()
                                    .await
                                    .map(|h| crate::sys::handle_path(&h));
                                (command, path)
                            },
                            |(command, path)| Message::PluginFileDialogResult { command, path },
                        );
                    }
                }
                // Every non-Command event above is a one-shot (state toggle,
                // clear, dialog spawn) — nothing stays running to clear the
                // highlight later, so turn it off here. (#355)
                self.ribbon.deactivate_tool();
                Task::none()
    }

    pub(super) fn on_unsaved_dialog_discard(&mut self) -> Task<Message> {
                match self.pending_close.take() {
                    Some(crate::app::PendingClose::Tab(idx)) => {
                        let close_win = self.close_unsaved_dialog_window();
                        // Discarded — drop this tab's autosave recovery copy.
                        #[cfg(not(target_arch = "wasm32"))]
                        let _ = std::fs::remove_file(self.autosave_target(idx));
                        if self.tabs.len() == 1 {
                            self.tab_counter += 1;
                            self.tabs[0] =
                                crate::app::document::DocumentTab::new_drawing(self.tab_counter);
                            self.active_tab = 0;
                            self.apply_bg_default(0);
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
                        return Task::batch([close_win, self.continue_tab_close_queue()]);
                    }
                    Some(crate::app::PendingClose::Quit) => {
                        if let Some(idx) = self.tabs.iter().position(|t| t.dirty) {
                            self.tabs[idx].dirty = false;
                        }
                        if self.tabs.iter().any(|t| t.dirty) {
                            // More dirty tabs remain — keep window open.
                            self.pending_close = Some(crate::app::PendingClose::Quit);
                        } else {
                            let close_win = self.close_unsaved_dialog_window();
                            return Task::batch(vec![close_win, self.exit_app()]);
                        }
                    }
                    None => {}
                }
                Task::none()
    }

    pub(super) fn on_unsaved_dialog_save(&mut self) -> Task<Message> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(pending) = self.pending_close.clone() else {
                return Task::none();
            };
            let (idx, continuation) = match pending {
                crate::app::PendingClose::Tab(idx) => {
                    (idx, crate::app::SaveContinuation::CloseTab)
                }
                crate::app::PendingClose::Quit => {
                    let Some(idx) = self.tabs.iter().position(|tab| tab.dirty) else {
                        self.pending_close = None;
                        return Task::batch([
                            self.close_unsaved_dialog_window(),
                            self.exit_app(),
                        ]);
                    };
                    (idx, crate::app::SaveContinuation::Quit)
                }
            };

            if self.active_save_jobs.contains_key(&self.tabs[idx].id) {
                self.command_line
                    .push_info(crate::t!("Save already running for this drawing.").as_ref());
                return Task::none();
            }

            if !self.tabs[idx].recovery_save_as_required {
                if let Some(path) = self.tabs[idx].current_path.clone() {
                    let version = self.tabs[idx].scene.document.version;
                    self.prepare_native_save(idx);
                    let close = self.close_unsaved_dialog_window();
                    let save = self.queue_native_save(
                        idx,
                        path,
                        version,
                        crate::app::SavePurpose::Manual,
                        continuation,
                        false,
                        true,
                    );
                    return Task::batch([close, save]);
                }
            }

            self.active_tab = idx;
            self.save_dialog_for_unsaved = true;
            let close = self.close_unsaved_dialog_window();
            let save = self.save_with_default_format(idx);
            return Task::batch([close, save]);
        }

        #[cfg(target_arch = "wasm32")]
        {
            match self.pending_close.take() {
                Some(crate::app::PendingClose::Tab(idx)) => {
                    self.pending_close = Some(crate::app::PendingClose::Tab(idx));
                    self.save_dialog_for_unsaved = true;
                    let close = self.close_unsaved_dialog_window();
                    let save = self.save_with_default_format(idx);
                    Task::batch([close, save])
                }
                Some(crate::app::PendingClose::Quit) => {
                    if let Some(idx) = self.tabs.iter().position(|tab| tab.dirty) {
                        self.active_tab = idx;
                        self.pending_close = Some(crate::app::PendingClose::Quit);
                        self.save_dialog_for_unsaved = true;
                        let close = self.close_unsaved_dialog_window();
                        let save = self.save_with_default_format(idx);
                        Task::batch([close, save])
                    } else {
                        Task::batch([
                            self.close_unsaved_dialog_window(),
                            self.exit_app(),
                        ])
                    }
                }
                None => Task::none(),
            }
        }
    }

    /// Build a unique block name from a file stem: the stem itself, then
    /// "stem (2)", "stem (3)", … on collisions.
    fn block_name_from_file(&self, stem: &str) -> String {
        let i = self.active_tab;
        let base = stem.trim();
        if base.is_empty() {
            return self.unique_block_name("Block");
        }
        let doc = &self.tabs[i].scene.document;
        if doc.block_records.get(base).is_none() {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let name = format!("{base} ({n})");
            if doc.block_records.get(&name).is_none() {
                return name;
            }
            n += 1;
        }
    }

    /// Load an external DWG/DXF at `path` and define its model-space contents as
    /// one new block in the active drawing. Returns the new block's name, or an
    /// error message. Nested block definitions are imported first so nested
    /// INSERTs render (AutoCAD's "inserting a drawing imports its block defs").
    fn import_file_as_block(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        let doc = crate::io::load_file(&path).map_err(|e| e.to_string())?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Block".to_string());
        self.import_document_as_block(doc, stem)
    }

    /// Define one block in the active drawing from a loaded `CadDocument`'s
    /// model-space entities (base = the file's model-space insertion base).
    fn import_document_as_block(
        &mut self,
        doc: acadrust::CadDocument,
        stem: String,
    ) -> Result<String, String> {
        let i = self.active_tab;
        // Model-space block record handle (Layout object first, name fallback).
        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let acadrust::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .unwrap_or(acadrust::Handle::NULL);
        let mut entities: Vec<acadrust::EntityType> = if model_br.is_null() {
            Vec::new()
        } else {
            let br = doc.block_records.iter().find(|br| br.handle == model_br);
            let handles = br.map(|b| b.entity_handles.clone()).unwrap_or_default();
            if !handles.is_empty() {
                // Authoritative ownership list (DWG and well-formed DXF).
                handles
                    .iter()
                    .filter_map(|h| doc.get_entity(*h))
                    .filter(|e| {
                        !matches!(e, acadrust::EntityType::Block(_) | acadrust::EntityType::BlockEnd(_))
                    })
                    .cloned()
                    .collect()
            } else {
                // Legacy DXF that omits 330 group codes: treat null-owner
                // entities as model-space content (mirrors belongs_to_visible_block).
                doc.entities()
                    .filter(|e| {
                        let o = e.common().owner_handle;
                        o == model_br || o.is_null()
                    })
                    .filter(|e| {
                        !matches!(e, acadrust::EntityType::Block(_) | acadrust::EntityType::BlockEnd(_))
                    })
                    .cloned()
                    .collect()
            }
        };
        if entities.is_empty() {
            return Err("No model-space entities in that file.".to_string());
        }
        let base = glam::DVec3::new(
            doc.header.model_space_insertion_base.x,
            doc.header.model_space_insertion_base.y,
            doc.header.model_space_insertion_base.z,
        );
        let name = self.block_name_from_file(&stem);
        // Capture every table record needed by the top-level entities and their
        // nested block definitions. Importing only the definitions leaves
        // source-only layers, linetypes, and text/dimension styles dangling.
        let deps = crate::app::ClipboardDeps::capture(&doc, &entities);
        // Capture the imported file's nested block definitions once. When a
        // name collides with a block already in the active drawing, we must
        // *preserve both*: keep the destination's block and import the file's
        // under a unique name, then re-point every INSERT at the renamed one —
        // otherwise a nested reference silently resolves to the destination's
        // unrelated definition. (#135-style collision, but for file imports.)
        let mut defs = deps.blocks.clone();
        let mut rename_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        // Reserve every destination block name plus every imported dependency
        // name, so importing a source file that itself holds "Door" and
        // "Door (2)" cannot generate a colliding second "Door (2)".
        let mut reserved: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        for existing in self.tabs[i].scene.document.block_records.names() {
            reserved.insert(existing.to_string());
        }
        for def in &defs {
            reserved.insert(def.name.clone());
        }
        for def in &defs {
            // Keep the name only if it is truly unused in the active drawing.
            let used_in_dest = self.tabs[i]
                .scene
                .document
                .block_records
                .get(&def.name)
                .is_some();
            let dest_name = if used_in_dest {
                let mut n = 2;
                loop {
                    let candidate = format!("{} ({})", def.name, n);
                    if reserved.insert(candidate.clone()) {
                        break candidate;
                    }
                    n += 1;
                }
            } else {
                reserved.insert(def.name.clone());
                def.name.clone()
            };
            rename_map.insert(def.name.clone(), dest_name);
        }
        // Rewrite every INSERT in the model-space entities and in every captured
        // definition, then rename the definitions and define them.
        for entity in entities
            .iter_mut()
            .chain(defs.iter_mut().flat_map(|def| def.entities.iter_mut()))
        {
            if let acadrust::EntityType::Insert(ins) = entity {
                if let Some(new_name) = rename_map.get(&ins.block_name) {
                    ins.block_name = new_name.clone();
                }
            }
        }
        for def in &mut defs {
            if let Some(new_name) = rename_map.get(&def.name) {
                def.name = new_name.clone();
            }
        }
        // Every mutation below belongs to the one INSERT FILE undo step.
        self.push_undo_snapshot(i, "INSERT FILE");
        self.merge_dependencies(i, &deps);
        for def in defs {
            self.tabs[i]
                .scene
                .define_block_raw(&def.name, def.base_point, def.entities);
        }
        self.tabs[i]
            .scene
            .define_block_from_owned_entities(entities, &name, base)?;
        self.tabs[i].scene.populate_meshes_from_document();
        self.tabs[i].dirty = true;
        Ok(name)
    }

    pub(super) fn on_block_palette(&mut self, m: crate::ui::window::block_palette::BlockPaletteMsg) -> iced::Task<Message> {
        use crate::ui::window::block_palette::{BlockEntry, BlockPaletteMsg};
        match m {
            BlockPaletteMsg::Search(s) => {
                self.block_palette.search = s;
                iced::Task::none()
            }
            BlockPaletteMsg::CyclePreviewSize => {
                self.block_palette.preview_size =
                    crate::ui::window::block_palette::cycle_preview_size(
                        self.block_palette.preview_size,
                    );
                iced::Task::none()
            }
            BlockPaletteMsg::Refresh => {
                self.refresh_block_palette();
                iced::Task::none()
            }
            BlockPaletteMsg::ToggleBar => {
                // Collapse bar (mirrors the Properties panel).
                self.block_palette_expanded ^= true;
                if self.block_palette_expanded {
                    self.refresh_block_palette();
                }
                iced::Task::none()
            }
            BlockPaletteMsg::Close => {
                self.show_block_palette = false;
                iced::Task::none()
            }
            BlockPaletteMsg::PickFile => iced::Task::perform(
                async {
                    let handle = rfd::AsyncFileDialog::new()
                        .set_title("Select Drawing to Insert as Block")
                        .add_filter("DWG/DXF Files", &["dwg", "dxf", "DWG", "DXF"])
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(crate::sys::handle_path(&h)),
                        None => Err("Cancelled".to_string()),
                    }
                },
                |r| Message::BlockPalette(BlockPaletteMsg::FilePicked(r)),
            ),
            BlockPaletteMsg::FilePicked(Ok(path)) => {
                match self.import_file_as_block(path) {
                    Ok(name) => {
                        self.command_line
                            .push_output(&format!("Inserting \"{name}\" from file."));
                        self.refresh_block_palette();
                        self.start_block_placement(&name);
                    }
                    Err(e) if e != "Cancelled" => {
                        self.command_line.push_error(&format!("INSERT FILE: {e}"));
                    }
                    Err(_) => {}
                }
                iced::Task::none()
            }
            BlockPaletteMsg::FilePicked(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(&e);
                }
                iced::Task::none()
            }
            BlockPaletteMsg::Insert(name) => {
                self.start_block_placement(&name);
                iced::Task::none()
            }
        }
    }

    /// Start placing `name` through the INSERT command, skipping the name prompt.
    fn start_block_placement(&mut self, name: &str) {
        let i = self.active_tab;
        let wires = self
            .block_palette
            .blocks
            .iter()
            .find(|b| b.name == name)
            .map(|b| b.wires.clone())
            .unwrap_or_else(|| self.tabs[i].scene.block_preview_wires(name));
        use crate::modules::insert::insert_block::InsertBlockCommand;
        let cmd = InsertBlockCommand::new_for_block(name.to_string(), wires, glam::Vec3::ZERO);
        use crate::command::CadCommand;
        self.command_line.push_info(&cmd.prompt());
        self.tabs[i].active_cmd = Some(Box::new(cmd));
        self.block_palette.placing = Some(name.to_string());
    }

    /// Rebuild the panel's block list + cached wires from the active drawing.
    pub(crate) fn refresh_block_palette(&mut self) {
        let i = self.active_tab;
        let names = self.tabs[i].scene.custom_block_names();
        self.block_palette.cached_names = names.clone();
        self.block_palette.source_tab_id = Some(self.tabs[i].id);
        self.block_palette.source_block_epoch = self.tabs[i].scene.block_epoch;
        self.block_palette.blocks = names
            .into_iter()
            .map(|name| {
                let wires = self.tabs[i].scene.block_preview_wires(&name);
                crate::ui::window::block_palette::BlockEntry { name, wires }
            })
            .collect();
    }

    /// Cheap per-update check: rebuild when the active drawing's definitions
    /// changed, even when their names happen to stay the same.
    pub(crate) fn refresh_block_palette_if_stale(&mut self) {
        if !self.show_block_palette {
            return;
        }
        let i = self.active_tab;
        // `block_epoch` advances for every block-definition change, so it
        // avoids re-scanning every block name on unrelated application updates.
        if self.block_palette.source_tab_id != Some(self.tabs[i].id)
            || self.block_palette.source_block_epoch != self.tabs[i].scene.block_epoch
        {
            self.refresh_block_palette();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use acadrust::entities::Line;
    use acadrust::types::Vector3;
    use acadrust::EntityType;

    fn fresh() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// A foreign document: the fresh scene's document plus one model-space LINE.
    fn foreign_doc(app: &OpenCADStudio) -> acadrust::CadDocument {
        let mut doc = app.tabs[app.active_tab].scene.document.clone();
        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let acadrust::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .expect("fresh document has a model-space block record");
        let mut line = Line::new();
        line.common.owner_handle = model_br;
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(100.0, 50.0, 0.0);
        doc.add_entity(EntityType::Line(line)).unwrap();
        doc
    }

    #[test]
    fn import_document_as_block_defines_block() {
        let mut app = fresh();
        let doc = foreign_doc(&app);
        let name = app.import_document_as_block(doc, "Fixture".to_string()).unwrap();
        assert_eq!(name, "Fixture");
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .get("Fixture")
            .is_some());
    }

    #[test]
    fn import_document_as_block_merges_source_only_layer() {
        use acadrust::tables::Layer;

        let mut app = fresh();
        let mut doc = foreign_doc(&app);
        let mut layer = Layer::new("Imported Fixtures");
        layer.handle = doc.allocate_handle();
        doc.layers.add(layer).unwrap();
        let model = doc
            .objects
            .values()
            .find_map(|o| match o {
                acadrust::objects::ObjectType::Layout(l) if l.name == "Model" => {
                    Some(l.block_record)
                }
                _ => None,
            })
            .unwrap();
        let line = doc
            .entities_mut()
            .find(|entity| entity.common().owner_handle == model)
            .unwrap();
        line.common_mut().layer = "Imported Fixtures".to_string();

        app.import_document_as_block(doc, "Fixture".to_string())
            .unwrap();
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .contains("Imported Fixtures"));
    }

    /// A foreign document whose model space INSERTs a `Fixture` block, whose
    /// definition itself INSERTs a `Door` block — so the imported `Door` is a
    /// *nested dependency*, not a top-level entity. The `Door` in this file is
    /// unrelated to any `Door` in the destination drawing.
    fn nested_foreign_doc(app: &OpenCADStudio) -> acadrust::CadDocument {
        use acadrust::entities::Insert;
        use acadrust::tables::BlockRecord;
        use acadrust::Handle;
        let mut doc = foreign_doc(app);

        // "Door" block definition: one LINE of geometry.
        let door_h = Handle::new(doc.next_handle());
        let mut door_br = BlockRecord::new("Door");
        door_br.handle = door_h;
        doc.block_records.add(door_br).unwrap();
        let mut door_line = Line::new();
        door_line.start = Vector3::new(0.0, 0.0, 0.0);
        door_line.end = Vector3::new(5.0, 0.0, 0.0);
        door_line.common.owner_handle = door_h;
        doc.add_entity(EntityType::Line(door_line)).unwrap();

        // "Fixture" block definition: INSERTs the nested "Door".
        let fixture_h = Handle::new(doc.next_handle());
        let mut fixture_br = BlockRecord::new("Fixture");
        fixture_br.handle = fixture_h;
        doc.block_records.add(fixture_br).unwrap();
        let mut nested = Insert::new("Door", Vector3::new(2.0, 0.0, 0.0));
        nested.common.owner_handle = fixture_h;
        doc.add_entity(EntityType::Insert(nested)).unwrap();

        // Model space references the fixture so the import captures it.
        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let acadrust::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .expect("fresh document has a model-space block record");
        let mut top = Insert::new("Fixture", Vector3::new(0.0, 0.0, 0.0));
        top.common.owner_handle = model_br;
        doc.add_entity(EntityType::Insert(top)).unwrap();

        doc
    }

    /// The nested-INSERT's block name inside a defined block record.
    fn nested_insert_target(
        doc: &acadrust::CadDocument,
        block: &str,
    ) -> Option<String> {
        let br = doc.block_records.get(block)?;
        br.entity_handles.iter().find_map(|h| match doc.get_entity(*h)? {
            EntityType::Insert(ins) => Some(ins.block_name.clone()),
            _ => None,
        })
    }

    /// A foreign document whose model space INSERTs a `Fixture` block whose
    /// definition INSERTs `Door (2)`, whose definition in turn INSERTs `Door`.
    /// The file therefore carries *both* `Door` and `Door (2)` as nested deps.
    fn doubly_nested_foreign_doc(app: &OpenCADStudio) -> acadrust::CadDocument {
        use acadrust::entities::Insert;
        use acadrust::tables::BlockRecord;
        use acadrust::Handle;
        let mut doc = foreign_doc(app);

        let door_h = Handle::new(doc.next_handle());
        let mut door_br = BlockRecord::new("Door");
        door_br.handle = door_h;
        doc.block_records.add(door_br).unwrap();
        let mut door_line = Line::new();
        door_line.start = Vector3::new(0.0, 0.0, 0.0);
        door_line.end = Vector3::new(5.0, 0.0, 0.0);
        door_line.common.owner_handle = door_h;
        doc.add_entity(EntityType::Line(door_line)).unwrap();

        let door2_h = Handle::new(doc.next_handle());
        let mut door2_br = BlockRecord::new("Door (2)");
        door2_br.handle = door2_h;
        doc.block_records.add(door2_br).unwrap();
        let mut mid = Insert::new("Door", Vector3::ZERO);
        mid.common.owner_handle = door2_h;
        doc.add_entity(EntityType::Insert(mid)).unwrap();

        let fixture_h = Handle::new(doc.next_handle());
        let mut fixture_br = BlockRecord::new("Fixture");
        fixture_br.handle = fixture_h;
        doc.block_records.add(fixture_br).unwrap();
        let mut nested = Insert::new("Door (2)", Vector3::new(2.0, 0.0, 0.0));
        nested.common.owner_handle = fixture_h;
        doc.add_entity(EntityType::Insert(nested)).unwrap();

        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let acadrust::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .expect("fresh document has a model-space block record");
        let mut top = Insert::new("Fixture", Vector3::new(0.0, 0.0, 0.0));
        top.common.owner_handle = model_br;
        doc.add_entity(EntityType::Insert(top)).unwrap();

        doc
    }

    #[test]
    fn import_reserves_source_names_so_nested_deps_cannot_collide() {
        let mut app = fresh();
        let i = app.active_tab;
        // Source file itself carries "Door" and "Door (2)"; destination already
        // has "Door". The imported "Door" must land on "Door (3)" — NOT steal
        // "Door (2)", which the source file reserves for its own definition.
        let doc = doubly_nested_foreign_doc(&app);
        let mut dest_door = Line::new();
        dest_door.start = Vector3::new(0.0, 0.0, 0.0);
        dest_door.end = Vector3::new(3.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(
                vec![EntityType::Line(dest_door)],
                "Door",
                glam::DVec3::ZERO,
            )
            .unwrap();

        let _ = app.import_document_as_block(doc, "Imported".to_string()).unwrap();
        let doc = &app.tabs[i].scene.document;
        // The file's own "Door (2)" is kept intact and targets the file's
        // renamed "Door" (now "Door (3)" — "Door (2)" was taken by the source).
        assert_eq!(
            nested_insert_target(doc, "Door (2)"),
            Some("Door (3)".to_string())
        );
        // The file's plain "Door" got bumped to "Door (3)" so both stay distinct.
        assert!(
            doc.block_records.get("Door").is_some(),
            "destination Door preserved"
        );
        assert!(
            doc.block_records.get("Door (2)").is_some(),
            "source Door (2) preserved under its own name"
        );
        assert!(
            doc.block_records.get("Door (3)").is_some(),
            "source Door renamed to Door (3), not Door (2)"
        );
        assert_eq!(
            nested_insert_target(doc, "Fixture"),
            Some("Door (2)".to_string())
        );
    }

    #[test]
    fn import_nested_block_collision_preserves_both_definitions() {
        let mut app = fresh();
        let i = app.active_tab;
        // Build the foreign document from the pristine scene first, so its
        // nested "Door" is genuinely distinct from the destination's.
        let doc = nested_foreign_doc(&app);
        // Destination drawing already has its own, unrelated "Door" block.
        let mut dest_door = Line::new();
        dest_door.start = Vector3::new(0.0, 0.0, 0.0);
        dest_door.end = Vector3::new(3.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(
                vec![EntityType::Line(dest_door)],
                "Door",
                glam::DVec3::ZERO,
            )
            .unwrap();

        let name = app.import_document_as_block(doc, "Imported".to_string()).unwrap();
        assert_eq!(name, "Imported");

        let doc = &app.tabs[i].scene.document;
        // Both definitions survive: the destination's original and the imported one.
        assert!(
            doc.block_records.get("Door").is_some(),
            "destination's own Door must be preserved"
        );
        assert!(
            doc.block_records.get("Door (2)").is_some(),
            "imported nested Door must be renamed to Door (2)"
        );
        // The imported Fixture's nested INSERT must point at the imported Door (2),
        // not silently resolve to the destination's unrelated Door.
        assert_eq!(
            nested_insert_target(doc, "Fixture"),
            Some("Door (2)".to_string()),
            "Fixture must reference the renamed imported Door, not the destination's"
        );
        assert_eq!(
            nested_insert_target(doc, "Door (2)"),
            None,
            "imported Door (2) has no nested INSERTs"
        );
    }

    #[test]
    fn blockpalette_refresh_lists_and_places_block() {
        let mut app = fresh();
        let doc = foreign_doc(&app);
        let name = app.import_document_as_block(doc, "Fixture".to_string()).unwrap();
        app.refresh_block_palette();
        assert!(app.block_palette.blocks.iter().any(|b| b.name == "Fixture"));
        app.start_block_placement(&name);
        let cmd = app.tabs[app.active_tab].active_cmd.as_ref().expect("INSERT running");
        assert_eq!(cmd.name(), "INSERT");
        assert_eq!(app.block_palette.placing.as_deref(), Some("Fixture"));
    }

    #[test]
    fn blockpalette_collapse_and_close_toggle_state() {
        let mut app = fresh();
        app.show_block_palette = true;
        app.block_palette_expanded = true;
        let _ = app.on_block_palette(BlockPaletteMsg::ToggleBar);
        assert!(!app.block_palette_expanded, "collapse hides the panel body");
        let _ = app.on_block_palette(BlockPaletteMsg::ToggleBar);
        assert!(app.block_palette_expanded, "bar click re-expands the panel");
        let _ = app.on_block_palette(BlockPaletteMsg::Close);
        assert!(!app.show_block_palette, "close dismisses the sidebar");
    }

    #[test]
    fn block_name_from_file_avoids_collisions() {
        let mut app = fresh();
        let i = app.active_tab;
        let mut line = Line::new();
        line.start = Vector3::ZERO;
        line.end = Vector3::new(1.0, 0.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(vec![EntityType::Line(line)], "Chair", glam::DVec3::ZERO)
            .unwrap();
        assert_eq!(app.block_name_from_file("Chair"), "Chair (2)");
        assert_eq!(app.block_name_from_file("Table"), "Table");
    }
}
