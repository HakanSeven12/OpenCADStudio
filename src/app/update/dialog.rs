//! `dialog` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    parse_coord, polar_constrain_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
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
use codec::types::Color as AcadColor;
use codec::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};

/// Pointer travel (px) before pressing a panel's title bar or tab becomes a
/// drag, so a plain click never moves the panel.
const DOCK_DRAG_THRESHOLD: f32 = 5.0;

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
                // Commands use `start_allowed`; other events need a drawing
                // and stay blocked on the Start page (#299, #388, #389).
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
                    // `dispatch_command` turns a one-shot tool's highlight off
                    // again once nothing is left running (#355).
                    ModuleEvent::Command(cmd) => return self.dispatch_command(&cmd),
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
                    ModuleEvent::SetVisualStyle(name) => {
                        use crate::modules::view::visual_style;
                        match visual_style::mode_for_keyword(&name) {
                            Some(mode) => return Task::done(Message::SetRenderMode(mode)),
                            None => {
                                // Name the styles that do exist, from the same
                                // list every other caller reads.
                                self.command_line.push_error(
                                    crate::tf!("Unknown visual style \"{name}\".").as_ref(),
                                );
                                self.command_line
                                    .push_info(crate::t!(visual_style::keyword_prompt()).as_ref());
                            }
                        }
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
                                    .add_filter(crate::t!("All Files").as_ref(), &["*"])
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
                    Some(crate::app::PendingClose::Tab(tab_id)) => {
                        let close_win = self.close_unsaved_dialog_window();
                        // Resolve the id now: the tab may have closed while
                        // the dialog was open (batched close, automation),
                        // in which case there is nothing left to discard.
                        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
                            return Task::batch([close_win, self.continue_tab_close_queue()]);
                        };
                        // Discarded — drop this tab's autosave recovery copy.
                        #[cfg(not(target_arch = "wasm32"))]
                        let _ = std::fs::remove_file(self.autosave_target(idx));
                        if self.tabs.len() == 1 {
                            self.tab_counter += 1;
                            self.tabs[0] =
                                crate::app::document::DocumentTab::new_drawing(self.tab_counter);
                            self.active_tab = 0;
                            self.apply_display_defaults(0);
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
                crate::app::PendingClose::Tab(tab_id) => {
                    // The pending target is an id; resolve it against the
                    // current tab list so a tab closed in the meantime can
                    // neither panic the save path nor save the wrong file.
                    let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
                        self.pending_close = None;
                        return Task::batch([
                            self.close_unsaved_dialog_window(),
                            self.continue_tab_close_queue(),
                        ]);
                    };
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
                Some(crate::app::PendingClose::Tab(tab_id)) => {
                    let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
                        return Task::batch([
                            self.close_unsaved_dialog_window(),
                            self.continue_tab_close_queue(),
                        ]);
                    };
                    self.pending_close = Some(crate::app::PendingClose::Tab(tab_id));
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
    pub(in crate::app) fn import_file_as_block(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        self.import_drawing_block(path, false)
    }

    /// [`Self::import_file_as_block`]; with `redefine` a block already named
    /// after the file takes the file's contents instead of a new name.
    pub(super) fn import_drawing_block(
        &mut self,
        path: std::path::PathBuf,
        redefine: bool,
    ) -> Result<String, String> {
        let doc = crate::io::load_file(&path).map_err(|e| e.to_string())?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Block".to_string());
        self.import_document_block(doc, stem, redefine)
    }

    /// Define one block in the active drawing from a loaded `CadDocument`'s
    /// model-space entities (base = the file's model-space insertion base).
    #[cfg(test)]
    fn import_document_as_block(
        &mut self,
        doc: codec::CadDocument,
        stem: String,
    ) -> Result<String, String> {
        self.import_document_block(doc, stem, false)
    }

    fn import_document_block(
        &mut self,
        doc: codec::CadDocument,
        stem: String,
        redefine: bool,
    ) -> Result<String, String> {
        let i = self.active_tab;
        // Model-space block record handle (Layout object first, name fallback).
        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let codec::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .unwrap_or(codec::Handle::NULL);
        let mut entities: Vec<codec::EntityType> = if model_br.is_null() {
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
                        !matches!(e, codec::EntityType::Block(_) | codec::EntityType::BlockEnd(_))
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
                        !matches!(e, codec::EntityType::Block(_) | codec::EntityType::BlockEnd(_))
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
        let redefine = redefine
            && self.tabs[i].scene.document.block_records.get(stem.trim()).is_some();
        let name = if redefine {
            stem.trim().to_string()
        } else {
            self.block_name_from_file(&stem)
        };
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
            if let codec::EntityType::Insert(ins) = entity {
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
        if redefine {
            self.tabs[i].scene.redefine_block_raw(
                &name,
                codec::types::Vector3::new(base.x, base.y, base.z),
                entities,
            );
        } else {
            self.tabs[i]
                .scene
                .define_block_from_owned_entities(entities, &name, base)?;
        }
        self.tabs[i].scene.populate_meshes_from_document();
        self.tabs[i].dirty = true;
        Ok(name)
    }

    /// Dock chrome interaction (grab / pin / resize / hover / move) applied to
    /// whichever panel the message names.
    pub(super) fn on_dock(&mut self, m: crate::ui::dock::DockMsg) -> iced::Task<Message> {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockDrag, DockMsg, DropTarget, FloatPanel};
        match m {
            DockMsg::DockGrab(id) => self.dock_begin(DockDrag::moving(id, None), Some(id)),
            DockMsg::IconPress(id) => {
                // An icon press shows the icon's group and may also start
                // dragging the pallet; a plain click never passes the drag
                // threshold.
                if self.dock.show_group_of(id) {
                    self.save_config();
                }
                self.dock_begin(DockDrag::moving(id, None), Some(id));
            }
            DockMsg::GroupGrab(side, gi) => {
                if let Some(&id) = self.dock_group_visible(side, gi).first() {
                    self.dock_begin(DockDrag::moving(id, Some((side, gi))), Some(id));
                }
            }
            DockMsg::ResizeGrab(id) => self.dock_begin(DockDrag::Width(id), Some(id)),
            DockMsg::SplitGrab(side, group, upper, lower) => self.dock_begin(
                DockDrag::Split {
                    side,
                    group,
                    upper,
                    lower,
                },
                None,
            ),
            DockMsg::FloatResizeGrab(panel, from_left) => {
                self.dock.raise_float(panel);
                self.dock_begin(DockDrag::FloatSize { panel, from_left }, None);
            }
            DockMsg::SplitReset(side, gi) => {
                self.dock.reset_splits(side, gi);
                self.save_config();
            }
            DockMsg::WidthReset(id) => {
                if let Some((side, gi)) = self.dock.location(id) {
                    self.dock.reset_group_width(side, gi);
                } else {
                    self.dock.reset_width(id);
                }
                self.save_config();
            }
            DockMsg::TitleHover(id) => self.dock_title_hover = id,
            DockMsg::GripHover(grip) => self.dock_grip_hover = grip,
            DockMsg::FloatRaise(id) => self.dock.raise_float(id),
            DockMsg::FloatOut(id) => {
                // Float beside the edge it was docked on, near the top.
                let (ww, _) = self.dock_workspace_size();
                let (w, h) = self.dock_float_size(id);
                let strip = crate::ui::dock::DOCK_STRIP_W;
                let x = match self.dock.location(id) {
                    Some((DockSide::Right, _)) => {
                        ww - strip - self.dock_column_width(DockSide::Right) - w - 60.0
                    }
                    _ => strip + self.dock_column_width(DockSide::Left) + 40.0,
                };
                self.dock_drag = None;
                if self.dock.float(FloatPanel {
                    id,
                    x: x.max(0.0),
                    y: 40.0,
                    w,
                    h,
                }) {
                    self.save_config();
                }
            }
            DockMsg::DockTo(id, side) => {
                self.dock_drag = None;
                if self.dock.dock(id, side, usize::MAX) {
                    self.save_config();
                }
            }
            DockMsg::EdgeMenu(side) => self.dock_edge_menu = side,
            DockMsg::EdgeMenuToggle(side, id) => {
                // Checked = open and docked on this edge: unchecking hides it.
                // Anything else (closed, floating, on the other edge) moves
                // here and opens, in its own group here if it has one, else
                // as a new group; adding a pallet ends the menu's job.
                let here = self.dock.location(id).map(|(s, _)| s) == Some(side);
                if !here {
                    self.dock.dock(id, side, usize::MAX);
                } else if self.dock_panel_visible(id) {
                    let task = self.dock_set_open(id, false);
                    self.save_config();
                    return task;
                }
                self.dock_edge_menu = None;
                let task = self.dock_set_open(id, true);
                self.save_config();
                return task;
            }
            DockMsg::AutoCollapseToggle(id) => {
                // Docked: auto-hide for the whole edge; floating: this pallet.
                if self.dock.toggle_auto_hide(id) {
                    self.dock_peek = None;
                }
                self.save_config();
            }
            DockMsg::Close(id) => return self.dock_set_open(id, false),
            DockMsg::Hover(id) => {
                self.dock_icon_hover = id;
                // Only an auto-hiding edge (or floating pallet) reacts to
                // hover: it reveals the hovered pallet's group. Otherwise
                // groups switch on click. Ignored mid-drag, when the pointer
                // is over the preview rather than the strip.
                if let Some(id) = id {
                    if self.dock_drag.is_none() && self.dock.auto_hides(id) {
                        self.dock_reveal(id);
                    }
                }
            }
            DockMsg::HoverExit => {
                if self.dock_drag.is_none()
                    && self.dock_peek.is_some_and(|id| self.dock.auto_hides(id))
                {
                    self.dock_peek = None;
                }
            }
            DockMsg::DragMove(point) => self.dock_drag_move(point),
            DockMsg::DragRelease => {
                let mut changed = false;
                match self.dock_drag.take() {
                    Some(DockDrag::Move {
                        group: Some((side, gi)),
                        target: Some(DropTarget::Edge { side: to, index }),
                        ..
                    }) => changed = self.dock.move_group(side, gi, to, index),
                    Some(DockDrag::Move {
                        panel,
                        group: None,
                        target: Some(target),
                        ..
                    }) => {
                        changed = match target {
                            DropTarget::Edge { side, index } => self.dock.dock(panel, side, index),
                            DropTarget::Join { side, group, index } => {
                                self.dock.join_group(panel, side, group, index)
                            }
                            DropTarget::Float { x, y } => {
                                let (w, h) = self.dock_float_size(panel);
                                self.dock.float(FloatPanel {
                                    id: panel,
                                    x,
                                    y,
                                    w,
                                    h,
                                })
                            }
                        };
                    }
                    Some(DockDrag::Move { .. } | DockDrag::LayerColumn) | None => {}
                    Some(DockDrag::Width(_) | DockDrag::Split { .. } | DockDrag::FloatSize { .. }) => {
                        changed = true;
                    }
                }
                if changed {
                    self.save_config();
                }
                self.dock_drag_last = None;
                self.xref_col_drag = None;
                self.xref_col_last = None;
                self.xref_split_drag = false;
            }
        }
        iced::Task::none()
    }

    /// Pointer motion while something in the dock is dragged.
    fn dock_drag_move(&mut self, point: iced::Point) {
        // NOTE: xref column drags intentionally do NOT ride this path: its
        // points live in workspace space while the header tracker reports
        // header-local points, and mixing the two produced a one-time jump
        // plus a stuck drag. Column moves arrive via Message::XrefColMove
        // only.
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockDrag, DropTarget};
        let last = self.dock_drag_last.replace(point);
        let Some(drag) = self.dock_drag else {
            return;
        };
        match drag {
            DockDrag::Move {
                panel,
                group,
                origin,
                target,
                grab,
            } => {
                let origin = origin.unwrap_or(point);
                let moved = (point.x - origin.x).hypot(point.y - origin.y);
                let started = target.is_some() || moved >= DOCK_DRAG_THRESHOLD;
                // Hold a floating panel where it was grabbed; a docked one by
                // its title bar.
                let grab = if target.is_none() && started {
                    match self.dock.float_rect(panel) {
                        Some(f) => iced::Vector::new(origin.x - f.x, origin.y - f.y),
                        None => iced::Vector::new(
                            (self.dock.width(panel, self.win_size.0) * 0.5).min(120.0),
                            14.0,
                        ),
                    }
                } else {
                    grab
                };
                // Store the grab first: the drop target reads it.
                self.dock_drag = Some(DockDrag::Move {
                    panel,
                    group,
                    origin: Some(origin),
                    target,
                    grab,
                });
                if !started {
                    return;
                }
                let found = self.dock_drop_target(point);
                // A whole group only moves between group positions: it
                // cannot join another group or float.
                let target = match (group, found) {
                    (Some(_), DropTarget::Edge { .. }) | (None, _) => Some(found),
                    (Some(_), _) => None,
                };
                self.dock_drag = Some(DockDrag::Move {
                    panel,
                    group,
                    origin: Some(origin),
                    target,
                    grab,
                });
            }
            DockDrag::Split {
                side,
                group,
                upper,
                lower,
            } => {
                let Some(last) = last else { return };
                let (_, avail) = self.dock_workspace_size();
                let total: f32 = self
                    .dock_slot_spans(side)
                    .iter()
                    .map(|(i, _, _)| self.dock.groups(side)[group].weights[*i])
                    .sum();
                if avail > 0.0 && total > 0.0 {
                    let per_px = total / avail;
                    self.dock.shift_split(
                        side,
                        group,
                        upper,
                        lower,
                        (point.y - last.y) * per_px,
                        crate::ui::dock::GROUP_MIN_H * per_px,
                    );
                }
            }
            DockDrag::FloatSize { panel, from_left } => {
                let (Some(last), Some(f)) = (last, self.dock.float_rect(panel)) else {
                    return;
                };
                let dx = point.x - last.x;
                self.dock.resize_float(
                    panel,
                    if from_left { f.w - dx } else { f.w + dx },
                    f.h + point.y - last.y,
                    from_left,
                );
            }
            DockDrag::LayerColumn => {
                let Some(last) = last else { return };
                self.layer_name_col_w =
                    (self.layer_name_col_w + point.x - last.x).clamp(60.0, 640.0);
            }
            DockDrag::Width(id) => {
                let Some(last) = last else { return };
                let dx = point.x - last.x;
                match self.dock.location(id) {
                    // The divider sizes the panel's group.
                    Some((side, gi)) => {
                        let delta = if side == DockSide::Left { dx } else { -dx };
                        let cur = self.dock_group_width(side, gi) + delta;
                        self.dock.set_group_width(side, gi, cur);
                    }
                    None => {
                        let cur = self.dock.settings(id).width + dx;
                        self.dock.set_width(id, cur);
                    }
                }
            }
        }
    }

    /// Start `drag`, peeking `peek` (a pallet being dragged or resized stays
    /// shown on an auto-hiding edge).
    fn dock_begin(&mut self, drag: crate::ui::dock::DockDrag, peek: Option<crate::ui::dock::PanelId>) {
        if let crate::ui::dock::DockDrag::Move { panel, .. } = drag {
            self.dock.raise_float(panel);
        }
        self.dock_drag = Some(drag);
        self.dock_drag_last = None;
        self.dock_edge_menu = None;
        if peek.is_some() {
            self.dock_peek = peek;
        }
    }

    /// Bring `id` into view: show its group on its edge and, where it
    /// auto-hides, keep it revealed until the pointer leaves.
    pub(crate) fn dock_reveal(&mut self, id: crate::ui::dock::PanelId) {
        self.dock.show_group_of(id);
        self.dock_peek = Some(id);
    }

    /// Open or close pallet `id`, running its usual open path (refreshes,
    /// ribbon highlight) or close path. An opened pallet is revealed where
    /// the layout has it.
    pub(crate) fn dock_set_open(
        &mut self,
        id: crate::ui::dock::PanelId,
        open: bool,
    ) -> iced::Task<Message> {
        use crate::ui::dock::PanelId;
        if open {
            let task = if self.dock_panel_visible(id) {
                iced::Task::none()
            } else {
                match id {
                    PanelId::Properties => {
                        self.show_properties = true;
                        self.ribbon.set_properties(true);
                        iced::Task::none()
                    }
                    PanelId::BlockPalette => {
                        self.open_blocks_palette(None);
                        iced::Task::none()
                    }
                    PanelId::ExternalReferences => {
                        self.show_external_references = true;
                        self.refresh_xref_manager();
                        iced::Task::none()
                    }
                    PanelId::Browser => {
                        self.show_browser = true;
                        iced::Task::none()
                    }
                    PanelId::NodeGraph => self.on_graph(crate::ui::node_graph::GraphMsg::Toggle),
                    PanelId::PointCloudManager => {
                        self.pc_manager.show = true;
                        iced::Task::none()
                    }
                    PanelId::Count => {
                        self.set_count_palette(true);
                        iced::Task::none()
                    }
                    PanelId::SheetSetManager => {
                        self.show_sheet_set_manager(true);
                        iced::Task::none()
                    }
                    PanelId::Layers => {
                        self.sync_ribbon_layers();
                        self.show_layers = true;
                        iced::Task::none()
                    }
                }
            };
            self.dock_reveal(id);
            return task;
        }
        match id {
            PanelId::BlockPalette => {
                self.show_block_palette = false;
                self.block_palette.placing = None;
            }
            PanelId::ExternalReferences => self.show_external_references = false,
            PanelId::Browser => self.show_browser = false,
            PanelId::NodeGraph => self.show_node_graph = false,
            PanelId::PointCloudManager => self.pc_manager.show = false,
            PanelId::Count => {
                self.count_palette.show = false;
                self.ribbon.set_count_palette(false);
            }
            PanelId::SheetSetManager => {
                self.sheet_set.show = false;
                self.ribbon.set_sheet_set(false);
            }
            PanelId::Properties => {
                self.show_properties = false;
                self.ribbon.set_properties(false);
            }
            PanelId::Layers => {
                self.show_layers = false;
                self.ribbon.deactivate_tool_if("LAYERS");
            }
        }
        if self.dock_peek == Some(id) {
            self.dock_peek = None;
        }
        if self
            .dock_drag
            .is_some_and(|d| d.movement().is_some_and(|(p, _, _)| p == id) || d == crate::ui::dock::DockDrag::Width(id))
        {
            self.dock_drag = None;
        }
        iced::Task::none()
    }

    /// Whether `id` is currently rendered (not closed, not on the start screen /
    /// clean-screen viewport). Mirrors the visibility filter the edge column
    /// renderer applies before building each side's stack.
    pub(crate) fn dock_panel_visible(&self, id: crate::ui::dock::PanelId) -> bool {
        use crate::ui::dock::PanelId;
        if self.tabs[self.active_tab].is_start || self.clean_screen {
            return false;
        }
        match id {
            PanelId::Properties => self.show_properties,
            PanelId::BlockPalette => self.show_block_palette,
            PanelId::ExternalReferences => self.show_external_references,
            PanelId::Browser => self.show_browser,
            PanelId::NodeGraph => self.show_node_graph,
            PanelId::PointCloudManager => self.pc_manager.show,
            PanelId::Count => self.count_palette.show,
            PanelId::SheetSetManager => self.sheet_set.show,
            PanelId::Layers => self.show_layers,
        }
    }

    /// Size of the workspace the dock lays out in: window width by viewport
    /// height.
    pub(crate) fn dock_workspace_size(&self) -> (f32, f32) {
        (
            self.win_size.0,
            self.tabs[self.active_tab].scene.selection.borrow().view.vp_size.1,
        )
    }

    /// The open pallets of group `gi` on `side`, top → bottom. Closed
    /// pallets keep their place but take no space, so a group with none open
    /// is not drawn at all.
    pub(crate) fn dock_group_visible(
        &self,
        side: crate::app::config::DockSide,
        gi: usize,
    ) -> Vec<crate::ui::dock::PanelId> {
        self.dock.groups(side)[gi]
            .panels
            .iter()
            .copied()
            .filter(|id| self.dock_panel_visible(*id))
            .collect()
    }

    /// Indices of the groups on `side` with an open pallet (listed in the
    /// icon strip).
    pub(crate) fn dock_visible_groups(&self, side: crate::app::config::DockSide) -> Vec<usize> {
        (0..self.dock.groups(side).len())
            .filter(|gi| !self.dock_group_visible(side, *gi).is_empty())
            .collect()
    }

    /// The group `side` draws: the one it shows, or the first with an open
    /// pallet when that one has none.
    pub(crate) fn dock_shown_group(&self, side: crate::app::config::DockSide) -> Option<usize> {
        let visible = self.dock_visible_groups(side);
        let shown = self.dock.shown(side);
        if visible.contains(&shown) {
            Some(shown)
        } else {
            visible.first().copied()
        }
    }

    /// Whether `side` draws its shown group beside the icon strip: always,
    /// unless the edge auto-hides and none of the group's icons is hovered.
    pub(crate) fn dock_edge_expanded(&self, side: crate::app::config::DockSide) -> bool {
        let Some(gi) = self.dock_shown_group(side) else {
            return false;
        };
        !self.dock.edge_auto_hide(side)
            || self
                .dock_peek
                .is_some_and(|e| self.dock_group_visible(side, gi).contains(&e))
    }

    /// On-screen width of group `gi` on `side`.
    pub(crate) fn dock_group_width(&self, side: crate::app::config::DockSide, gi: usize) -> f32 {
        self.dock.group_width_px(side, gi, self.win_size.0)
    }

    /// Width of the pallet column beside `side`'s icon strip: the shown
    /// group's width, or nothing when it is hidden.
    pub(crate) fn dock_column_width(&self, side: crate::app::config::DockSide) -> f32 {
        match self.dock_shown_group(side) {
            Some(gi) if self.dock_edge_expanded(side) => self.dock_group_width(side, gi),
            _ => 0.0,
        }
    }

    /// The icon strip of `side`: each listed group's place, and the top of
    /// the + button.
    pub(crate) fn dock_strip_layout(
        &self,
        side: crate::app::config::DockSide,
    ) -> (Vec<crate::ui::dock::StripGroup>, f32) {
        let groups: Vec<(usize, usize)> = self
            .dock_visible_groups(side)
            .into_iter()
            .map(|gi| (gi, self.dock_group_visible(side, gi).len()))
            .collect();
        crate::ui::dock::strip_layout(&groups)
    }

    /// The open pallets of `side`'s shown group, each as (position in the
    /// group, top, bottom) in the workspace.
    pub(crate) fn dock_slot_spans(
        &self,
        side: crate::app::config::DockSide,
    ) -> Vec<(usize, f32, f32)> {
        let Some(gi) = self.dock_shown_group(side) else {
            return Vec::new();
        };
        let (_, avail) = self.dock_workspace_size();
        let group = &self.dock.groups(side)[gi];
        let open: Vec<usize> = (0..group.panels.len())
            .filter(|i| self.dock_panel_visible(group.panels[*i]))
            .collect();
        let weights: Vec<f32> = open.iter().map(|i| group.weights[*i]).collect();
        open.into_iter()
            .zip(crate::ui::dock::slot_spans(&weights, avail))
            .map(|(i, (top, bottom))| (i, top, bottom))
            .collect()
    }

    /// Size a panel takes when floated: its current floating size, or its
    /// docked width by a comfortable share of the workspace height.
    pub(crate) fn dock_float_size(&self, id: crate::ui::dock::PanelId) -> (f32, f32) {
        if let Some(f) = self.dock.float_rect(id) {
            return (f.w, f.h);
        }
        let (_, avail) = self.dock_workspace_size();
        (
            self.dock.width(id, self.win_size.0),
            (avail * 0.6).clamp(crate::ui::dock::FLOAT_MIN_H, 520.0),
        )
    }

    /// Where a dragged pallet lands if released at workspace point `p`:
    /// on an edge's icon strip it joins a group between its icons or starts a
    /// new group; over the shown group's pallets it stacks above / below the
    /// one under the pointer; on an empty edge it starts a group; anywhere
    /// else it floats.
    pub(crate) fn dock_drop_target(&self, p: iced::Point) -> crate::ui::dock::DropTarget {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DropTarget, StripHit, DOCK_EDGE_ZONE, DOCK_STRIP_W};
        let (ww, wh) = self.dock_workspace_size();
        for side in [DockSide::Left, DockSide::Right] {
            let listed = !self.dock_visible_groups(side).is_empty();
            let from_edge = match side {
                DockSide::Left => p.x,
                DockSide::Right => ww - p.x,
            };
            if !listed {
                if from_edge <= DOCK_EDGE_ZONE {
                    return DropTarget::Edge {
                        side,
                        index: self.dock.groups(side).len(),
                    };
                }
                continue;
            }
            // Over the icon strip.
            if from_edge <= DOCK_STRIP_W {
                let (layout, _) = self.dock_strip_layout(side);
                return match crate::ui::dock::strip_hit(p.y, &layout) {
                    StripHit::NewGroup(at) => DropTarget::Edge {
                        side,
                        index: at.unwrap_or(self.dock.groups(side).len()),
                    },
                    StripHit::Tab { group, index } => {
                        // Map the gap among open pallets to a position among
                        // all of the group's pallets.
                        let open = self.dock_group_visible(side, group);
                        let all = &self.dock.groups(side)[group].panels;
                        let index = open
                            .get(index)
                            .and_then(|t| all.iter().position(|a| a == t))
                            .unwrap_or(all.len());
                        DropTarget::Join { side, group, index }
                    }
                };
            }
            // Over the shown group's pallets: above or below the one there.
            if from_edge <= DOCK_STRIP_W + self.dock_column_width(side) {
                let gi = self.dock_shown_group(side).expect("listed edge");
                let spans = self.dock_slot_spans(side);
                if let Some(&(pos, top, bottom)) = spans
                    .iter()
                    .find(|(_, _, bottom)| p.y < *bottom)
                    .or(spans.last())
                {
                    let index = if p.y < (top + bottom) * 0.5 { pos } else { pos + 1 };
                    return DropTarget::Join {
                        side,
                        group: gi,
                        index,
                    };
                }
            }
        }
        let grab = match self.dock_drag {
            Some(crate::ui::dock::DockDrag::Move { grab, .. }) => grab,
            _ => iced::Vector::new(0.0, 0.0),
        };
        let x = (p.x - grab.x).clamp(0.0, (ww - 60.0).max(0.0));
        let y = (p.y - grab.y).clamp(0.0, (wh - 30.0).max(0.0));
        DropTarget::Float { x, y }
    }

    /// Rebuild the Reference Manager's entry list from the active drawing.
    /// Uses the tab's session sets (unloaded + stat baseline) so CLI and
    /// palette agree; writes fresh load-time mtimes back into the tab's stat
    /// cache (`Stale` detectable from the second refresh on). Records the
    /// tab's `edit_revision` so the palette auto-rescans on doc changes.
    pub(crate) fn refresh_xref_manager(&mut self) {
        let i = self.active_tab;
        let base_dir: std::path::PathBuf = self.tabs[i]
            .current_path
            .as_ref()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let tab_id = self.tabs[i].id;
        let revision = self.tabs[i].edit_revision;
        let (host_name, host_path) = match &self.tabs[i].current_path {
            Some(p) => (
                p.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                p.to_string_lossy().into_owned(),
            ),
            None => (String::new(), String::new()),
        };
        let baselines = {
            let tab = &self.tabs[i];
            self.xref_manager.refresh(
                &tab.scene.document,
                &base_dir,
                tab.xref_unloaded.as_set(),
                &tab.xref_stat_cache.0,
                &host_name,
                &host_path,
            )
        };
        let tab = &mut self.tabs[i];
        for (key, mtime) in baselines {
            tab.xref_stat_cache.insert(key, mtime);
        }
        // A clean rescan clears the open-time missing notice.
        if !self
            .xref_manager
            .entries
            .iter()
            .any(|e| e.status == crate::io::xref_model::RefStatus::NotFound)
        {
            tab.xref_missing = 0;
        }
        self.xref_manager.source_tab_id = Some(tab_id);
        self.xref_manager.source_edit_revision = revision;
    }

    /// Re-scan when the palette is open but showing another tab's drawing or
    /// the active drawing changed since the last scan (CLI/palette mutation,
    /// XOPEN-return edit, undo/redo — all bump `edit_revision`). Cheap:
    /// `collect_entries` stats files without re-parsing the host.
    pub(crate) fn refresh_xref_manager_if_stale(&mut self) {
        if !self.dock_panel_visible(crate::ui::dock::PanelId::ExternalReferences) {
            return;
        }
        let i = self.active_tab;
        if self.xref_manager.source_tab_id != Some(self.tabs[i].id)
            || self.xref_manager.source_edit_revision != self.tabs[i].edit_revision
        {
            self.refresh_xref_manager();
        }
    }

    /// Shared post-mutation sequence for reference ops (CLI + palette):
    /// repopulate scene caches, mirror xref layers into the Layers panel,
    /// and mark the tab dirty. Both call sites use it so the 5-line
    /// sequence cannot drift.
    pub(crate) fn post_ref_op(&mut self, i: usize) {
        self.tabs[i].scene.populate_hatches_from_document();
        self.tabs[i].scene.populate_images_from_document();
        // Image/PDF unload is intentionally session-only (the file formats do
        // not persist an unloaded bit), so keep their source entities but
        // remove their decoded render models.  A reload repopulates the cache
        // after clearing the corresponding session key.
        let hidden_images: Vec<codec::types::Handle> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::RasterImage(image)
                    if image.definition_handle.is_some_and(|key| self.tabs[i].xref_unloaded.is_unloaded(key.value())) =>
                {
                    Some(entity.common().handle)
                }
                codec::EntityType::Underlay(underlay)
                    if self.tabs[i].xref_unloaded.is_unloaded(underlay.definition_handle.value()) =>
                {
                    Some(entity.common().handle)
                }
                _ => None,
            })
            .collect();
        for handle in hidden_images {
            self.tabs[i].scene.images.remove(&handle);
        }
        self.tabs[i].scene.populate_meshes_from_document();
        self.refresh_layer_panel();
        self.tabs[i].dirty = true;
    }

    /// Execute a toolbar [`XrefPaletteOp`] on the palette's actionable
    /// selection. Same engine fns as the CLI arms, batched with per-item
    /// report lines in the CLI wording; finishes with [`post_ref_op`] plus
    /// a palette rescan (mirror of the CLI post-op sequence).
    pub(crate) fn xref_manager_op(
        &mut self,
        op: crate::ui::window::xref_manager::XrefPaletteOp,
    ) {
        if cfg!(target_arch = "wasm32") {
            self.command_line.push_error(crate::t!("Reference changes are not available on web — the reference list is read-only.").as_ref());
            return;
        }
        use crate::ui::window::xref_manager::XrefPaletteOp;
        let i = self.active_tab;
        // Full selection (nested rows included) with per-entry metadata, so
        // nested rows report per-entry errors instead of being silently
        // dropped by `actionable_selection` (mirrors the CLI arms).
        let picked: Vec<(u64, String, crate::io::xref_model::RefKind, bool)> = {
            let mut idx: Vec<usize> = self.xref_manager.selected.iter().copied().collect();
            idx.sort_unstable();
            idx.iter()
                .filter_map(|idx| {
                    self.xref_manager.entries.get(*idx).map(|e| {
                        (
                            e.key,
                            e.name.clone(),
                            e.kind,
                            e.parent_key.is_some(),
                        )
                    })
                })
                .collect()
        };
        if picked.is_empty() {
            self.command_line
                .push_info(crate::t!("Select a reference first.").as_ref());
            return;
        }
        let needs_host = matches!(
            op,
            XrefPaletteOp::Reload | XrefPaletteOp::Bind | XrefPaletteOp::Pathtype(_)
        );
        let host: Option<std::path::PathBuf> = self.tabs[i].current_path.clone();
        if needs_host && host.is_none() {
            self.command_line
                .push_error(crate::t!("XREF  Save the drawing first to resolve relative XREF paths.").as_ref());
            return;
        }
        let label = match op {
            XrefPaletteOp::Open => "XREF-OPEN",
            XrefPaletteOp::Detach => "XREF-DETACH",
            XrefPaletteOp::Unload => "XREF-UNLOAD",
            XrefPaletteOp::Reload => "XREF-RELOAD",
            XrefPaletteOp::Bind => "XREF-BIND",
            XrefPaletteOp::Overlay => "XREF-OVERLAY",
            XrefPaletteOp::Attach => "XREF-ATTACH",
            XrefPaletteOp::Pathtype(_) => "XREF-PATHTYPE",
        };
        self.push_undo_snapshot(i, label);
        let mut done = 0usize;
        match op {
            XrefPaletteOp::Detach => {
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot detach nested reference '{}'. Detach it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::detach_reference(
                        &mut self.tabs[i].scene.document,
                        *key,
                    ) {
                        Ok(name) => {
                            self.command_line.push_output(crate::tf!(
                                "XREF: detached \"{}\".",
                                name
                            ).as_ref());
                            self.tabs[i].xref_unloaded.remove(key);
                            self.tabs[i].xref_stat_cache.remove(key);
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Unload => {
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot unload nested reference '{}'. Unload it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::unload_reference(
                        &mut self.tabs[i].scene.document,
                        *key,
                    ) {
                        Ok(name) => {
                            self.command_line.push_output(crate::tf!(
                                "XREF: unloaded \"{}\".",
                                name
                            ).as_ref());
                            self.tabs[i].xref_unloaded.add(*key);
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
                self.tabs[i].scene.reseed_underlays();
                // Point clouds redraw as unloaded (box and saved path).
                self.tabs[i].scene.bump_geometry();
            }
            XrefPaletteOp::Reload => {
                let base_dir: std::path::PathBuf = host
                    .as_ref()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                // Drawing references only: image/PDF rows keep their flags
                // untouched and report per-entry (their names never match the
                // DWG-only `XrefInfo` list, so matching after the flag clear
                // would spuriously report no-match).
                let dwg_picked: Vec<(u64, String)> = picked
                    .iter()
                    .filter(|(_, _, kind, is_nested)| {
                        if *is_nested {
                            return false;
                        }
                        *kind == crate::io::xref_model::RefKind::DwgXref
                    })
                    .map(|(key, name, _, _)| (*key, name.clone()))
                    .collect();
                for (_, name, kind, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot reload nested reference '{}'. Reload it in its host drawing.",
                            name
                        ).as_ref());
                    } else if *kind == crate::io::xref_model::RefKind::Underlay {
                        // An underlay reloads by clearing its definition's unloaded state.
                        for (key, row_name, row_kind, _) in &picked {
                            if row_kind != kind || row_name != name {
                                continue;
                            }
                            let handle = codec::types::Handle::new(*key);
                            if let Some(codec::objects::ObjectType::UnderlayDefinition(def)) =
                                self.tabs[i].scene.document.objects.get_mut(&handle)
                            {
                                def.unloaded = false;
                            }
                            self.tabs[i].xref_unloaded.remove(key);
                        }
                        self.tabs[i].scene.reseed_underlays();
                    } else if *kind == crate::io::xref_model::RefKind::PointCloud {
                        // A point cloud reloads by setting its definition loaded.
                        for (key, row_name, row_kind, _) in &picked {
                            if row_kind == kind && row_name == name {
                                crate::io::xref::set_point_cloud_loaded(
                                    &mut self.tabs[i].scene.document,
                                    codec::types::Handle::new(*key),
                                    true,
                                );
                            }
                        }
                        self.tabs[i].scene.bump_geometry();
                    } else if *kind != crate::io::xref_model::RefKind::DwgXref {
                        self.command_line.push_error(crate::tf!(
                            "{}: reload applies to drawing references only.",
                            name
                        ).as_ref());
                    }
                }
                for (key, _) in &dwg_picked {
                    self.tabs[i].xref_unloaded.remove(key);
                    self.tabs[i].xref_stat_cache.remove(key);
                }
                let handles: rustc_hash::FxHashSet<codec::types::Handle> = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .iter()
                    .filter(|br| dwg_picked.iter().any(|(key, _)| *key == br.handle.value()))
                    .map(|br| br.handle)
                    .collect();
                let (infos, _dropped) = crate::io::xref::resolve_xrefs_for_keys(
                    &mut self.tabs[i].scene.document,
                    &base_dir,
                    &handles,
                );
                // Targeted reload updates baselines only for selected rows.
                {
                    let fresh = crate::io::xref::collect_entries_with_prev(
                        &self.tabs[i].scene.document,
                        &base_dir,
                        self.tabs[i].xref_unloaded.as_set(),
                        &self.tabs[i].xref_stat_cache.0,
                    );
                    for e in &fresh {
                        if !dwg_picked.iter().any(|(key, _)| *key == e.key) {
                            continue;
                        }
                        if e.status == crate::io::xref_model::RefStatus::Loaded {
                            if let Some(m) = e.modified {
                                self.tabs[i].xref_stat_cache.insert(e.key, m);
                            }
                        }
                    }
                }
                for (_, name) in &dwg_picked {
                    match infos
                        .iter()
                        .find(|n| n.name.eq_ignore_ascii_case(name))
                    {
                        Some(info) => {
                            self.report_xref_status(info);
                            done += 1;
                        }
                        None => self.command_line.push_error(crate::tf!(
                            "XREF: no references match '{}'.",
                            name
                        ).as_ref()),
                    }
                }
            }
            XrefPaletteOp::Overlay => {
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot overlay nested reference '{}'. Overlay it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::set_ref_type(
                        &mut self.tabs[i].scene.document,
                        *key,
                        crate::io::xref_model::RefType::Overlay,
                    ) {
                        Ok(name) => {
                            self.command_line.push_output(crate::tf!(
                                "XREF: \"{}\" set to Overlay.",
                                name
                            ).as_ref());
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Attach => {
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot attach nested reference '{}'. Attach it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::set_ref_type(
                        &mut self.tabs[i].scene.document,
                        *key,
                        crate::io::xref_model::RefType::Attach,
                    ) {
                        Ok(name) => {
                            self.command_line.push_output(crate::tf!(
                                "XREF: \"{}\" set to Attach.",
                                name
                            ).as_ref());
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Bind => {
                let base_dir: std::path::PathBuf = host
                    .as_ref()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                let host_dir: std::path::PathBuf = host
                    .as_ref()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot bind nested reference '{}'. Bind it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::bind_reference(
                        &mut self.tabs[i].scene.document,
                        *key,
                        &base_dir,
                        &host_dir,
                    ) {
                        Ok(outcome) => {
                            if outcome.unremapped == 0 {
                                self.command_line.push_output(crate::tf!(
                                    "XREF: bound \"{}\".",
                                    outcome.name
                                ).as_ref());
                            } else {
                                self.command_line.push_output(crate::tf!(
                                    "XREF: bound \"{}\" with {} unremapped style handles (see bind limitations).",
                                    outcome.name, outcome.unremapped
                                ).as_ref());
                            }
                            self.tabs[i].xref_unloaded.remove(key);
                            self.tabs[i].xref_stat_cache.remove(key);
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Pathtype(pathtype) => {
                let host = host.unwrap_or_else(|| std::path::PathBuf::from("."));
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot set path type for nested reference '{}'. Set it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::apply_pathtype(
                        &mut self.tabs[i].scene.document,
                        *key,
                        pathtype,
                        &host,
                    ) {
                        Ok(_) => {
                            self.command_line.push_output(crate::tf!(
                                "XREF: Path set for \"{}\" — Reload to apply.",
                                name
                            ).as_ref());
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Open => {
                // Handled in update/mod.rs XrefRowOp dispatch (navigation), not here.
            }
        }
        if done > 0 {
            self.post_ref_op(i);
        }
        self.refresh_xref_manager();
    }

    /// Reload every direct drawing reference (toolbar Reload All). Same
    /// engine path as `XRELOAD`: undo snapshot, session flags cleared,
    /// full resolve, per-ref report, stat baselines refreshed.
    pub(crate) fn xref_manager_reload_all(&mut self) {
        if cfg!(target_arch = "wasm32") {
            self.command_line.push_error(crate::t!("Reference changes are not available on web — the reference list is read-only.").as_ref());
            return;
        }
        let i = self.active_tab;
        let Some(path) = self.tabs[i].current_path.clone() else {
            self.command_line
                .push_error(crate::t!("XREF  Save the drawing first to resolve relative XREF paths.").as_ref());
            return;
        };
        let Some(base_dir) = path.parent().map(|p| p.to_path_buf()) else {
            self.command_line
                .push_error(crate::t!("XREF  Save the drawing first to resolve relative XREF paths.").as_ref());
            return;
        };
        let reload_keys: Vec<u64> = crate::io::xref::collect_entries_with_prev(
            &self.tabs[i].scene.document,
            &base_dir,
            self.tabs[i].xref_unloaded.as_set(),
            &self.tabs[i].xref_stat_cache.0,
        )
        .iter()
        .filter(|e| e.kind == crate::io::xref_model::RefKind::DwgXref && e.parent_key.is_none())
        .map(|e| e.key)
        .collect();
        self.push_undo_snapshot(i, "XREF-RELOAD");
        for key in &reload_keys {
            self.tabs[i].xref_unloaded.remove(key);
            self.tabs[i].xref_stat_cache.remove(key);
        }
        let (infos, _dropped) =
            crate::io::xref::resolve_xrefs(&mut self.tabs[i].scene.document, &base_dir);
        let fresh = crate::io::xref::collect_entries_with_prev(
            &self.tabs[i].scene.document,
            &base_dir,
            self.tabs[i].xref_unloaded.as_set(),
            &self.tabs[i].xref_stat_cache.0,
        );
        for e in &fresh {
            if e.status == crate::io::xref_model::RefStatus::Loaded {
                if let Some(m) = e.modified {
                    self.tabs[i].xref_stat_cache.insert(e.key, m);
                }
            }
        }
        for info in &infos {
            self.report_xref_status(info);
        }
        self.post_ref_op(i);
        self.refresh_xref_manager();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use codec::entities::Line;
    use codec::types::Vector3;
    use codec::EntityType;

    fn fresh() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// A foreign document: the fresh scene's document plus one model-space LINE.
    fn foreign_doc(app: &OpenCADStudio) -> codec::CadDocument {
        let mut doc = app.tabs[app.active_tab].scene.document.clone();
        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let codec::objects::ObjectType::Layout(l) = o {
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
        use codec::tables::Layer;

        let mut app = fresh();
        let mut doc = foreign_doc(&app);
        let mut layer = Layer::new("Imported Fixtures");
        layer.handle = doc.allocate_handle();
        doc.layers.add(layer).unwrap();
        let model = doc
            .objects
            .values()
            .find_map(|o| match o {
                codec::objects::ObjectType::Layout(l) if l.name == "Model" => {
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
    fn nested_foreign_doc(app: &OpenCADStudio) -> codec::CadDocument {
        use codec::entities::Insert;
        use codec::tables::BlockRecord;
        use codec::Handle;
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
                if let codec::objects::ObjectType::Layout(l) = o {
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
        doc: &codec::CadDocument,
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
    fn doubly_nested_foreign_doc(app: &OpenCADStudio) -> codec::CadDocument {
        use codec::entities::Insert;
        use codec::tables::BlockRecord;
        use codec::Handle;
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
                if let codec::objects::ObjectType::Layout(l) = o {
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
        assert_eq!(cmd.name(), "-INSERT");
        assert_eq!(app.block_palette.placing.as_deref(), Some("Fixture"));
    }

    #[test]
    fn blockpalette_reflects_new_block_without_reopen() {
        use codec::types::Transform;

        let mut app = fresh();
        let i = app.active_tab;

        // Reproduce the user-facing flow: select entities and run the BLOCK
        // command, which goes through `create_block_from_entities` (not the
        // clipboard / paste-as-block path).
        let mut line = Line::new();
        line.start = Vector3::ZERO;
        line.end = Vector3::new(10.0, 0.0, 0.0);
        let first = app.tabs[i].scene.add_entity(EntityType::Line(line));
        app.tabs[i].scene.select_entity(first, false);
        app.show_block_palette = true;
        let ws = Transform::identity();
        let id = Transform::identity();
        app.tabs[i]
            .scene
            .create_block_from_entities(&[first], "First", &ws, &id)
            .unwrap();
        app.refresh_block_palette_if_stale();
        assert!(app.block_palette.blocks.iter().any(|b| b.name == "First"));

        // Create a second block on the same tab, then re-run the per-update
        // stale check. It must pick up the new block WITHOUT reopening.
        line = Line::new();
        line.start = Vector3::ZERO;
        line.end = Vector3::new(9.0, 0.0, 0.0);
        let second = app.tabs[i].scene.add_entity(EntityType::Line(line));
        app.tabs[i].scene.select_entity(second, false);
        app.tabs[i]
            .scene
            .create_block_from_entities(&[second], "Second", &ws, &id)
            .unwrap();
        app.refresh_block_palette_if_stale();
        assert!(
            app.block_palette.blocks.iter().any(|b| b.name == "Second"),
            "Second must appear without reopening the panel"
        );
    }

    #[test]
    fn blockpalette_pin_toggles_autocollapse_and_close_hides() {
        let mut app = fresh();
        app.show_block_palette = true;
        let id = crate::ui::dock::PanelId::BlockPalette;
        let _ = app.on_dock(crate::ui::dock::DockMsg::AutoCollapseToggle(id));
        assert!(app.dock.auto_hides(id), "pin enables auto-hide");
        let _ = app.on_dock(crate::ui::dock::DockMsg::AutoCollapseToggle(id));
        assert!(!app.dock.auto_hides(id), "second pin disables auto-hide");
        let _ = app.on_dock(crate::ui::dock::DockMsg::Close(id));
        assert!(!app.show_block_palette, "close dismisses the sidebar");
    }

    /// A hermetic app with a 1600×900 workspace and the default dock layout
    /// (Properties left, block palette right), both shown.
    fn dock_app() -> OpenCADStudio {
        let mut app = fresh();
        // Tests load the user's persisted config; reset the dock to a known
        // state so this stays hermetic.
        app.dock = Default::default();
        app.dock.ensure_settings();
        app.show_properties = true;
        app.show_block_palette = true;
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600.0, 900.0);
        app.win_size = (1600.0, 900.0).into();
        app
    }

    /// Where the current dock drag would land, if it has started.
    fn drag_target(app: &OpenCADStudio) -> Option<crate::ui::dock::DropTarget> {
        app.dock_drag.and_then(|d| d.movement()).and_then(|(_, _, target)| target)
    }

    /// Press `id`'s title bar at `from`, drag to `to`, and return the target
    /// shown just before release.
    fn drag_panel(
        app: &mut OpenCADStudio,
        id: crate::ui::dock::PanelId,
        from: iced::Point,
        to: iced::Point,
    ) -> Option<crate::ui::dock::DropTarget> {
        use crate::ui::dock::DockMsg;
        let _ = app.on_dock(DockMsg::DockGrab(id));
        let _ = app.on_dock(DockMsg::DragMove(from));
        let _ = app.on_dock(DockMsg::DragMove(to));
        let target = drag_target(app);
        let _ = app.on_dock(DockMsg::DragRelease);
        target
    }

    #[test]
    fn blockpalette_dropped_on_the_top_half_of_a_pallet_stacks_above_it() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DropTarget, PanelId};
        let mut app = dock_app();
        let id = PanelId::BlockPalette;
        assert_eq!(app.dock.location(id), Some((DockSide::Right, 0)));
        let target = drag_panel(
            &mut app,
            id,
            iced::Point::new(1500.0, 10.0),
            iced::Point::new(150.0, 300.0),
        );
        assert_eq!(
            target,
            Some(DropTarget::Join {
                side: DockSide::Left,
                group: 0,
                index: 0
            })
        );
        assert_eq!(
            app.dock.left[0].panels,
            vec![PanelId::BlockPalette, PanelId::Properties]
        );
        assert!(app.dock.right.is_empty());
    }

    #[test]
    fn dock_drop_on_the_bottom_half_stacks_below() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DropTarget, PanelId};
        let mut app = dock_app();
        let target = drag_panel(
            &mut app,
            PanelId::BlockPalette,
            iced::Point::new(1500.0, 10.0),
            iced::Point::new(150.0, 700.0),
        );
        assert_eq!(
            target,
            Some(DropTarget::Join {
                side: DockSide::Left,
                group: 0,
                index: 1
            })
        );
        assert_eq!(
            app.dock.left[0].panels,
            vec![PanelId::Properties, PanelId::BlockPalette]
        );
    }

    #[test]
    fn dock_dragging_a_lone_pallet_over_its_own_group_is_a_no_op() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DropTarget, PanelId};
        let mut app = dock_app();
        let before = app.dock.clone();
        // The right edge's only group holds just the block palette; drag it
        // around over itself.
        let target = drag_panel(
            &mut app,
            PanelId::BlockPalette,
            iced::Point::new(1500.0, 10.0),
            iced::Point::new(1450.0, 600.0),
        );
        assert_eq!(
            target,
            Some(DropTarget::Join {
                side: DockSide::Right,
                group: 0,
                index: 1
            })
        );
        assert_eq!(app.dock, before);
    }

    #[test]
    fn dock_click_without_drag_changes_nothing() {
        let mut app = dock_app();
        let before = app.dock.clone();
        let id = crate::ui::dock::PanelId::BlockPalette;
        let p = iced::Point::new(1500.0, 10.0);
        let target = drag_panel(&mut app, id, p, iced::Point::new(1502.0, 11.0));
        assert_eq!(target, None, "movement under the threshold is a click");
        assert_eq!(app.dock, before);
    }

    #[test]
    fn dock_drop_below_the_strip_groups_starts_a_new_shown_group() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DropTarget, PanelId};
        let mut app = dock_app();
        let (_, plus_top) = app.dock_strip_layout(DockSide::Left);
        let target = drag_panel(
            &mut app,
            PanelId::BlockPalette,
            iced::Point::new(1500.0, 10.0),
            iced::Point::new(18.0, plus_top + 10.0),
        );
        assert_eq!(
            target,
            Some(DropTarget::Edge {
                side: DockSide::Left,
                index: 1
            })
        );
        assert_eq!(app.dock.left.len(), 2);
        assert_eq!(app.dock.left[1].panels, vec![PanelId::BlockPalette]);
        // The new group is the one shown: the column holds only the palette.
        assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
        assert_eq!(app.dock_slot_spans(DockSide::Left), vec![(0, 0.0, 900.0)]);
    }

    #[test]
    fn dock_icon_press_switches_the_shown_group() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockGroup, DockMsg, PanelId};
        let mut app = dock_app();
        app.show_browser = true;
        app.dock.left = vec![
            DockGroup::stack(vec![PanelId::Properties, PanelId::BlockPalette]),
            DockGroup::single(PanelId::Browser),
        ];
        app.dock.right.clear();
        // Group 0 shows both stacked pallets.
        assert_eq!(app.dock_slot_spans(DockSide::Left).len(), 2);
        let _ = app.on_dock(DockMsg::IconPress(PanelId::Browser));
        let _ = app.on_dock(DockMsg::DragRelease);
        assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
        assert_eq!(app.dock_slot_spans(DockSide::Left), vec![(0, 0.0, 900.0)]);
        // Each group keeps its own width.
        app.dock.set_width(PanelId::Browser, 400.0);
        assert_eq!(app.dock_column_width(DockSide::Left), 400.0);
        let _ = app.on_dock(DockMsg::IconPress(PanelId::BlockPalette));
        let _ = app.on_dock(DockMsg::DragRelease);
        assert_eq!(app.dock_column_width(DockSide::Left), 260.0);
    }

    #[test]
    fn dock_drop_over_the_viewport_floats_the_panel() {
        use crate::ui::dock::{DropTarget, PanelId};
        let mut app = dock_app();
        let target = drag_panel(
            &mut app,
            PanelId::BlockPalette,
            iced::Point::new(1500.0, 10.0),
            iced::Point::new(800.0, 400.0),
        );
        assert!(matches!(target, Some(DropTarget::Float { .. })));
        assert!(app.dock.right.is_empty());
        let f = app.dock.float_rect(PanelId::BlockPalette).expect("floating");
        assert!(f.x > 600.0 && f.x < 800.0 && f.y > 350.0 && f.y < 400.0);
        // Dragging a floating panel keeps the grab point under the pointer.
        let target = drag_panel(
            &mut app,
            PanelId::BlockPalette,
            iced::Point::new(f.x + 20.0, f.y + 10.0),
            iced::Point::new(f.x + 120.0, f.y + 60.0),
        );
        assert_eq!(target, Some(DropTarget::Float { x: f.x + 100.0, y: f.y + 50.0 }));
        // Opening a floating panel must not re-dock it.
        assert!(app.dock.is_placed(PanelId::BlockPalette));
    }

    #[test]
    fn dock_dragging_an_icon_along_the_strip_reorders_its_group() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, DropTarget, PanelId, STRIP_CELL_H};
        let mut app = dock_app();
        app.dock.join_group(PanelId::BlockPalette, DockSide::Left, 0, 1);
        let (layout, _) = app.dock_strip_layout(DockSide::Left);
        let top = layout[0].icons_top;
        // Drag the Properties icon onto the lower half of the Blocks icon:
        // below it, still in the group (the divider under it would start a
        // new group).
        let _ = app.on_dock(DockMsg::IconPress(PanelId::Properties));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, top + 10.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(
            18.0,
            top + 2.0 * STRIP_CELL_H - 10.0,
        )));
        assert_eq!(
            drag_target(&app),
            Some(DropTarget::Join {
                side: DockSide::Left,
                group: 0,
                index: 2
            })
        );
        let _ = app.on_dock(DockMsg::DragRelease);
        assert_eq!(
            app.dock.left[0].panels,
            vec![PanelId::BlockPalette, PanelId::Properties]
        );
    }

    #[test]
    fn dock_dragging_an_icon_into_another_group_lands_between_its_icons() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockGroup, DockMsg, DropTarget, PanelId, STRIP_CELL_H};
        let mut app = dock_app();
        app.show_browser = true;
        app.dock.left = vec![
            DockGroup::stack(vec![PanelId::Properties, PanelId::Browser]),
            DockGroup::single(PanelId::BlockPalette),
        ];
        app.dock.right.clear();
        let (layout, _) = app.dock_strip_layout(DockSide::Left);
        // Drag the Blocks icon (group 1) to between Properties and Browser.
        let _ = app.on_dock(DockMsg::IconPress(PanelId::BlockPalette));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(20.0, layout[1].icons_top + 10.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(
            20.0,
            layout[0].icons_top + STRIP_CELL_H + 2.0,
        )));
        assert_eq!(
            drag_target(&app),
            Some(DropTarget::Join {
                side: DockSide::Left,
                group: 0,
                index: 1
            })
        );
        let _ = app.on_dock(DockMsg::DragRelease);
        assert_eq!(app.dock.left.len(), 1, "the emptied group goes away");
        assert_eq!(
            app.dock.left[0].panels,
            vec![PanelId::Properties, PanelId::BlockPalette, PanelId::Browser]
        );
    }

    #[test]
    fn dock_grip_drag_moves_a_whole_group() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockGroup, DockMsg, PanelId};
        let mut app = dock_app();
        app.show_browser = true;
        app.dock.left = vec![
            DockGroup::stack(vec![PanelId::Properties, PanelId::Browser]),
            DockGroup::single(PanelId::BlockPalette),
        ];
        app.dock.right.clear();
        let (layout, plus_top) = app.dock_strip_layout(DockSide::Left);
        let _ = app.on_dock(DockMsg::GroupGrab(DockSide::Left, 0));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, layout[0].top + 2.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, plus_top + 10.0)));
        let _ = app.on_dock(DockMsg::DragRelease);
        assert_eq!(app.dock.left[0].panels, vec![PanelId::BlockPalette]);
        assert_eq!(
            app.dock.left[1].panels,
            vec![PanelId::Properties, PanelId::Browser]
        );
        // A group dropped over the viewport does not float or split up.
        let before = app.dock.clone();
        let _ = app.on_dock(DockMsg::GroupGrab(DockSide::Left, 1));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(18.0, 300.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(800.0, 400.0)));
        assert_eq!(drag_target(&app), None);
        let _ = app.on_dock(DockMsg::DragRelease);
        assert_eq!(app.dock, before);
    }

    #[test]
    fn dock_closed_pallets_take_no_room_and_empty_groups_hide() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockGroup, PanelId};
        let mut app = dock_app();
        app.dock.left = vec![
            DockGroup::stack(vec![PanelId::BlockPalette, PanelId::Properties]),
            DockGroup::single(PanelId::Browser),
        ];
        app.show_block_palette = false;
        app.show_browser = false;
        // Only Properties is open: one listed group, full height.
        assert_eq!(app.dock_visible_groups(DockSide::Left), vec![0]);
        assert_eq!(app.dock_slot_spans(DockSide::Left), vec![(1, 0.0, 900.0)]);
        // A shown group with nothing open falls back to one that has.
        app.dock.show_group(DockSide::Left, 1);
        assert_eq!(app.dock_shown_group(DockSide::Left), Some(0));
    }

    #[test]
    fn dock_splitter_drag_changes_pallet_heights() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockGroup, DockMsg, PanelId};
        let mut app = dock_app();
        app.dock.left = vec![DockGroup::stack(vec![PanelId::BlockPalette, PanelId::Properties])];
        app.dock.right.clear();
        let _ = app.on_dock(DockMsg::SplitGrab(DockSide::Left, 0, 0, 1));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(100.0, 450.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(100.0, 675.0)));
        let _ = app.on_dock(DockMsg::DragRelease);
        // 225 px of a 900 px edge holding 2 weight units = 0.5 units.
        let w = &app.dock.left[0].weights;
        assert!((w[0] - 1.5).abs() < 1e-4 && (w[1] - 0.5).abs() < 1e-4);
        let _ = app.on_dock(DockMsg::SplitReset(DockSide::Left, 0));
        assert_eq!(app.dock.left[0].weights, vec![1.0, 1.0]);
    }

    #[test]
    fn dock_auto_hide_edge_shows_the_hovered_group() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockGroup, DockMsg, PanelId};
        let mut app = dock_app();
        app.show_browser = true;
        app.dock.left.push(DockGroup::single(PanelId::Browser));
        // The pin on any docked pallet turns auto-hide on for its edge.
        let _ = app.on_dock(DockMsg::AutoCollapseToggle(PanelId::Properties));
        assert!(app.dock.edge_auto_hide(DockSide::Left));
        assert!(!app.dock.edge_auto_hide(DockSide::Right));
        assert!(!app.dock_edge_expanded(DockSide::Left));
        // Hovering an icon flies its group out.
        let _ = app.on_dock(DockMsg::Hover(Some(PanelId::Browser)));
        assert!(app.dock_edge_expanded(DockSide::Left));
        assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
        let _ = app.on_dock(DockMsg::HoverExit);
        assert!(!app.dock_edge_expanded(DockSide::Left));
    }

    #[test]
    fn dock_hover_does_not_switch_groups_without_auto_hide() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockGroup, DockMsg, PanelId};
        let mut app = dock_app();
        app.show_browser = true;
        app.dock.left.push(DockGroup::single(PanelId::Browser));
        let _ = app.on_dock(DockMsg::Hover(Some(PanelId::Browser)));
        assert_eq!(app.dock_shown_group(DockSide::Left), Some(0));
        assert!(app.dock_edge_expanded(DockSide::Left));
    }

    #[test]
    fn dock_float_resize_grows_the_panel() {
        use crate::ui::dock::{DockMsg, FloatPanel, PanelId};
        let mut app = dock_app();
        app.dock.float(FloatPanel {
            id: PanelId::BlockPalette,
            x: 100.0,
            y: 100.0,
            w: 260.0,
            h: 300.0,
        });
        let _ = app.on_dock(DockMsg::FloatResizeGrab(PanelId::BlockPalette, false));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(360.0, 400.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(400.0, 450.0)));
        let _ = app.on_dock(DockMsg::DragRelease);
        let f = app.dock.float_rect(PanelId::BlockPalette).unwrap();
        assert_eq!((f.w, f.h), (300.0, 350.0));
        // The bottom-left grip grows the panel leftward, keeping its right edge.
        let _ = app.on_dock(DockMsg::FloatResizeGrab(PanelId::BlockPalette, true));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(100.0, 450.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(60.0, 450.0)));
        let _ = app.on_dock(DockMsg::DragRelease);
        let g = app.dock.float_rect(PanelId::BlockPalette).unwrap();
        assert_eq!((g.x, g.w, g.h), (f.x - 40.0, 340.0, 350.0));
    }

    #[test]
    fn dock_edge_menu_adds_and_hides_pallets() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, PanelId};
        let mut app = dock_app();
        app.show_browser = false;
        let _ = app.on_dock(DockMsg::EdgeMenu(Some(DockSide::Left)));
        // Checking a closed pallet opens it as a new, shown group here.
        let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::Browser));
        assert!(app.show_browser);
        assert_eq!(app.dock.left.len(), 2);
        assert_eq!(app.dock.left[1].panels, vec![PanelId::Browser]);
        assert_eq!(app.dock_shown_group(DockSide::Left), Some(1));
        assert_eq!(app.dock_edge_menu, None, "the menu closes after adding");
        let _ = app.on_dock(DockMsg::EdgeMenu(Some(DockSide::Left)));
        // Unchecking hides it but keeps its place.
        let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::Browser));
        assert!(!app.show_browser);
        assert_eq!(app.dock.location(PanelId::Browser), Some((DockSide::Left, 1)));
        // Opening it again from the same edge reuses that place.
        let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::Browser));
        assert_eq!(app.dock.left.len(), 2);
        // A pallet open on the other edge is not checked here: picking it
        // moves it over instead of hiding it.
        let _ = app.on_dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::BlockPalette));
        assert!(app.show_block_palette);
        assert!(app.dock.right.is_empty());
        assert_eq!(app.dock.location(PanelId::BlockPalette), Some((DockSide::Left, 2)));
    }

    #[test]
    fn dock_divider_resizes_only_its_group() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockGroup, DockMsg, PanelId};
        let mut app = dock_app();
        app.dock.left = vec![
            DockGroup::stack(vec![PanelId::Properties, PanelId::BlockPalette]),
            DockGroup::single(PanelId::Count),
        ];
        app.dock.right.clear();
        let _ = app.on_dock(DockMsg::ResizeGrab(PanelId::Properties));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(300.0, 100.0)));
        let _ = app.on_dock(DockMsg::DragMove(iced::Point::new(330.0, 100.0)));
        let _ = app.on_dock(DockMsg::DragRelease);
        // The group was 260 wide (its widest pallet); it is now 290, while
        // the other group and the pallets' own widths are untouched.
        assert_eq!(app.dock_group_width(DockSide::Left, 0), 290.0);
        assert_eq!(app.dock_group_width(DockSide::Left, 1), 280.0);
        assert_eq!(app.dock.settings(PanelId::Properties).width, 250.0);
    }

    #[test]
    fn layers_command_opens_a_floating_pallet_and_toggles_it_closed() {
        use crate::ui::dock::PanelId;
        let mut app = dock_app();
        let _ = app.update(Message::ToggleLayers);
        assert!(app.show_layers);
        assert!(app.active_modal.is_none(), "no modal backdrop");
        let f = app.dock.float_rect(PanelId::Layers).expect("floats on first use");
        assert!(f.x > 0.0 && f.y >= 0.0);
        let _ = app.update(Message::ToggleLayers);
        assert!(!app.show_layers);
        // Its place is kept for next time.
        assert!(app.dock.float_rect(PanelId::Layers).is_some());
    }

    /// A headless renderer for driving the real view.
    fn headless_renderer() -> iced::Renderer {
        iced_test::futures::futures::executor::block_on(
            <iced::Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                None,
            ),
        )
        .expect("headless renderer")
    }

    /// Size the real view is laid out at.
    const VIEW_SIZE: iced::Size = iced::Size::new(1600.0, 900.0);

    /// Centre of the first text `label` in the real view.
    fn find_in_view(app: &OpenCADStudio, label: &str) -> iced::Point {
        use iced_test::core::widget;
        use iced_test::runtime::user_interface::{Cache, UserInterface};
        use iced_test::selector::Bounded;
        use iced_test::Selector;
        let mut renderer = headless_renderer();
        let mut ui = UserInterface::build(app.view_main(), VIEW_SIZE, Cache::default(), &mut renderer);
        let mut find = Selector::find(label);
        ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
        match widget::Operation::finish(&find) {
            widget::operation::Outcome::Some(Some(target)) => target
                .visible_bounds()
                .expect("visible")
                .center(),
            _ => panic!("{label} not found"),
        }
    }

    /// Drive the real view through iced's `UserInterface` with the pointer at
    /// `at`: feed `events` one at a time, and between events hand the
    /// published messages to the app and rebuild the view while keeping the
    /// widget-tree cache, as the runtime does.
    fn drive_view(app: &mut OpenCADStudio, at: iced::Point, events: &[iced::Event]) {
        use iced_test::core::{mouse, shell, window};
        use iced_test::runtime::user_interface::{Cache, UserInterface};
        let mut renderer = headless_renderer();
        let mut cache = Cache::default();
        let cursor = mouse::Cursor::Available(at);
        for event in events {
            let mut messages = Vec::new();
            {
                let mut ui = UserInterface::build(app.view_main(), VIEW_SIZE, cache, &mut renderer);
                let _ = ui.update(
                    &window::Headless,
                    &shell::Waker::noop(),
                    std::slice::from_ref(event),
                    cursor,
                    &mut renderer,
                    &mut messages,
                );
                cache = ui.into_cache();
            }
            for message in messages {
                let _ = app.update(message);
            }
        }
    }

    #[test]
    fn docked_title_bar_double_click_floats_the_pallet_in_the_real_view() {
        use crate::ui::dock::PanelId;
        let mut app = dock_app();
        let press = iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
        let release =
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left));
        // The block palette's title text sits in its docked title bar.
        let title = find_in_view(&app, "Blocks");
        drive_view(&mut app, title, &[press.clone(), release.clone(), press, release]);
        assert!(
            app.dock.float_rect(PanelId::BlockPalette).is_some(),
            "a double-click on a docked title bar floats the pallet"
        );
    }

    #[test]
    fn hovering_a_strip_icon_reveals_an_auto_hiding_edge_in_the_real_view() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, PanelId, STRIP_CELL_H, STRIP_PAD};
        let mut app = dock_app();
        // Mirror the layout onto the left so the block palette's title marks
        // where the workspace starts, then auto-hide the right edge.
        let title = find_in_view(&app, "Blocks");
        let _ = app.on_dock(DockMsg::AutoCollapseToggle(PanelId::BlockPalette));
        assert!(!app.dock_edge_expanded(DockSide::Right));
        // The title bar text sits ~13 px below the workspace top; the strip's
        // first icon is centred STRIP_PAD + half a cell below it.
        let workspace_top = title.y - 13.0;
        let icon = iced::Point::new(
            VIEW_SIZE.width - crate::ui::dock::DOCK_STRIP_W / 2.0,
            workspace_top + STRIP_PAD + STRIP_CELL_H / 2.0,
        );
        drive_view(
            &mut app,
            icon,
            &[iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: icon })],
        );
        assert!(
            app.dock_edge_expanded(DockSide::Right),
            "hovering the icon reveals the auto-hiding edge"
        );
    }

    #[test]
    fn dock_double_clicks_float_and_dock_panels() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, PanelId};
        let mut app = dock_app();
        let _ = app.on_dock(DockMsg::FloatOut(PanelId::Properties));
        assert!(app.dock.left.is_empty());
        let f = app.dock.float_rect(PanelId::Properties).expect("floating");
        assert!(f.x > 0.0, "floats clear of the left edge");
        let _ = app.on_dock(DockMsg::DockTo(PanelId::Properties, DockSide::Right));
        assert!(app.dock.floating.is_empty());
        assert_eq!(app.dock.location(PanelId::Properties), Some((DockSide::Right, 1)));
    }

    #[test]
    fn blockpalette_width_reset() {
        use crate::app::config::DockSide;
        let mut app = dock_app();
        let id = crate::ui::dock::PanelId::BlockPalette;
        app.dock.set_group_width(DockSide::Right, 0, 500.0);
        let _ = app.on_dock(crate::ui::dock::DockMsg::WidthReset(id));
        assert_eq!(app.dock.group_width(DockSide::Right, 0), 260.0);
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

    #[test]
    fn save_as_rebases_live_doc_paths() {
        // F4: on Save-As completion the live document's relative reference
        // paths are rebased onto the new base dir (same helper the save
        // snapshot used), so the session agrees with the file just written.
        let mut app = fresh();
        let i = app.active_tab;
        let dir = std::env::temp_dir().join(format!(
            "ocs_saveas_rebase_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old_dir = dir.join("old");
        let new_dir = dir.join("new");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();
        let mut br = codec::tables::BlockRecord::new("PLAN");
        br.flags.is_xref = true;
        br.xref_path = "refs/plan.dwg".to_string();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i]
            .scene
            .document
            .block_records
            .add(br)
            .unwrap();
        app.tabs[i].current_path = Some(old_dir.join("host.dwg"));
        let tab_id = app.tabs[i].id;
        let job_id = 4242u64;
        app.active_save_jobs.insert(tab_id, job_id);
        let outcome = crate::app::SaveOutcome {
            job_id,
            tab_id,
            epoch: app.tabs[i].scene.geometry_epoch,
            revision: app.tabs[i].edit_revision,
            camera_generation: app.tabs[i].scene.camera_generation,
            path: new_dir.join("host.dwg"),
            version: app.tabs[i].scene.document.version,
            previous_autosave: None,
            set_current_path: true,
            purpose: crate::app::SavePurpose::SaveAs,
            continuation: crate::app::SaveContinuation::None,
            refreshed_preview: None,
            result: Ok(()),
        };
        let _ = app.on_save_finished(outcome);
        assert_eq!(
            app.tabs[i].current_path.as_deref(),
            Some(new_dir.join("host.dwg").as_path())
        );
        let br = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap();
        assert_eq!(br.xref_path, "../old/refs/plan.dwg");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn palette_tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ocs_palette_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn palette_output(app: &OpenCADStudio, start: usize) -> String {
        app.command_line.history[start..]
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn palette_rowop_nested_reports_per_entry() {
        // Behavioral matrix: every row op on a NESTED row via the GUI
        // message path. Nested rows must report per-entry errors (CLI
        // wording), never silently skip.
        use codec::tables::BlockRecord;
        use crate::app::Message;
        use crate::ui::window::xref_manager::XrefPaletteOp;
        let dir = palette_tmpdir("nestedmatrix");
        let mut host_doc = codec::CadDocument::new();
        let mut inner = BlockRecord::new("INNER");
        inner.flags.is_xref = true;
        inner.xref_path = "inner.dwg".to_string();
        host_doc.block_records.add(inner).unwrap();
        let bytes = crate::io::save_to_bytes(&host_doc, "dwg", host_doc.version).unwrap();
        std::fs::write(dir.join("host.dwg"), &bytes).unwrap();
        let mut app = fresh();
        let i = app.active_tab;
        let mut br = BlockRecord::new("HOST");
        br.flags.is_xref = true;
        br.xref_path = dir.join("host.dwg").to_string_lossy().into_owned();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        app.refresh_xref_manager();
        let idx = app.xref_manager.entries.iter().position(|e| e.name == "INNER").expect("nested INNER listed");
        assert!(app.xref_manager.entries[idx].parent_key.is_some());
        for op in [
            XrefPaletteOp::Detach,
            XrefPaletteOp::Unload,
            XrefPaletteOp::Reload,
            XrefPaletteOp::Bind,
            XrefPaletteOp::Overlay,
        ] {
            let start = app.command_line.history.len();
            let _ = app.update(Message::XrefRowOp(idx, op));
            let out = palette_output(&app, start);
            assert!(out.contains("INNER"), "op {op:?} on nested row gave: {out:?}");
            assert!(
                app.tabs[i].scene.document.block_records.get("HOST").is_some(),
                "op {op:?} must not mutate the direct reference"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn palette_detach_unload_rowop_gui_path() {
        // Reproduction for "Detach/Unload from the palette do nothing":
        // drive the exact GUI message path (row right-click menu item).
        use codec::tables::BlockRecord;
        use crate::app::Message;
        use crate::ui::window::xref_manager::XrefPaletteOp;
        let mut app = fresh();
        let i = app.active_tab;
        for name in ["PLAN", "SITE"] {
            let mut br = BlockRecord::new(name);
            br.flags.is_xref = true;
            br.xref_path = format!("old/{}.dwg", name.to_lowercase());
            br.handle = app.tabs[i].scene.document.allocate_handle();
            app.tabs[i].scene.document.block_records.add(br).unwrap();
        }
        app.tabs[i].current_path = Some(std::env::temp_dir().join("ocs_repro_host.dwg"));
        app.refresh_xref_manager();
        let idx = app.xref_manager.entries.iter().position(|e| e.name == "PLAN").expect("PLAN listed");
        let start = app.command_line.history.len();
        let _ = app.update(Message::XrefRowOp(idx, XrefPaletteOp::Detach));
        let out = palette_output(&app, start);
        assert!(out.contains("PLAN"), "detach output missing, got: {out:?}");
        assert!(app.tabs[i].scene.document.block_records.get("PLAN").is_none(), "PLAN definition must be gone");
        let idx = app.xref_manager.entries.iter().position(|e| e.name == "SITE").expect("SITE listed");
        let start = app.command_line.history.len();
        let _ = app.update(Message::XrefRowOp(idx, XrefPaletteOp::Unload));
        let out = palette_output(&app, start);
        assert!(out.contains("SITE"), "unload output missing, got: {out:?}");
        assert_eq!(
            app.xref_manager.entries.iter().find(|e| e.name == "SITE").unwrap().status,
            crate::io::xref_model::RefStatus::Unloaded
        );
    }

    #[test]
    fn palette_overlay_and_pathtype_rowop_gui_path() {
        // Row-menu Overlay + Change-Path row ops on a direct row via the
        // GUI message path: type flag flips, saved path clears.
        use codec::tables::BlockRecord;
        use crate::app::Message;
        use crate::io::xref_model::{Pathtype, RefType};
        use crate::ui::window::xref_manager::XrefPaletteOp;
        let mut app = fresh();
        let i = app.active_tab;
        let mut br = BlockRecord::new("PLAN");
        br.flags.is_xref = true;
        br.xref_path = "old/plan.dwg".to_string();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(std::env::temp_dir().join("ocs_rowop_host.dwg"));
        app.refresh_xref_manager();
        let idx = app.xref_manager.entries.iter().position(|e| e.name == "PLAN").expect("PLAN listed");
        let start = app.command_line.history.len();
        let _ = app.update(Message::XrefRowOp(idx, XrefPaletteOp::Overlay));
        let out = palette_output(&app, start);
        assert!(out.contains("PLAN"), "got: {out:?}");
        let br = app.tabs[i].scene.document.block_records.get("PLAN").unwrap();
        assert!(br.flags.is_xref_overlay && !br.flags.is_xref);
        assert_eq!(app.xref_manager.entries.iter().find(|e| e.name == "PLAN").unwrap().ref_type, RefType::Overlay);
        let start = app.command_line.history.len();
        let _ = app.update(Message::XrefRowOp(idx, XrefPaletteOp::Pathtype(Pathtype::None)));
        let out = palette_output(&app, start);
        assert!(out.contains("PLAN"), "got: {out:?}");
        let br = app.tabs[i].scene.document.block_records.get("PLAN").unwrap();
        assert_eq!(br.xref_path, "plan.dwg", "Remove Path strips to the bare filename");
    }

    #[test]
    fn palette_reload_all_resolves_direct_refs() {
        // Toolbar Reload All against a real on-disk reference: full
        // resolve, per-ref report, entry back to Loaded.
        use codec::tables::BlockRecord;
        let dir = palette_tmpdir("reloadall");
        let ref_doc = codec::CadDocument::new();
        let bytes = crate::io::save_to_bytes(&ref_doc, "dwg", ref_doc.version).unwrap();
        std::fs::write(dir.join("plan.dwg"), &bytes).unwrap();
        let mut app = fresh();
        let i = app.active_tab;
        let mut br = BlockRecord::new("PLAN");
        br.flags.is_xref = true;
        br.xref_path = dir.join("plan.dwg").to_string_lossy().into_owned();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(dir.join("host.dwg"));
        let start = app.command_line.history.len();
        app.xref_manager_reload_all();
        let out = palette_output(&app, start);
        assert!(out.contains("PLAN"), "reload-all must report the reference, got: {out:?}");
        let entry = app.xref_manager.entries.iter().find(|e| e.name == "PLAN").expect("PLAN listed");
        assert_eq!(entry.status, crate::io::xref_model::RefStatus::Loaded, "PLAN must resolve Loaded");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_replace_prompt_prefills_command_line() {
        // Toolbar Find and Replace hands the existing `XREF Path Find`
        // parsing a prefilled command line.
        use crate::app::Message;
        let mut app = fresh();
        let _ = app.update(Message::XrefFindReplacePrompt);
        assert_eq!(app.command_line.input, "XREF Path Find ");
    }

    #[test]
    fn palette_unload_nested_reports_per_entry() {
        // F6: palette Unload on a nested row reports the per-entry nested
        // error (CLI wording) instead of the confusing 'no loaded reference'.
        use codec::tables::BlockRecord;
        use crate::ui::window::xref_manager::XrefPaletteOp;
        let dir = palette_tmpdir("nested");
        let mut host_doc = codec::CadDocument::new();
        let mut inner = BlockRecord::new("INNER");
        inner.flags.is_xref = true;
        inner.xref_path = "inner.dwg".to_string();
        host_doc.block_records.add(inner).unwrap();
        let bytes = crate::io::save_to_bytes(&host_doc, "dwg", host_doc.version).unwrap();
        std::fs::write(dir.join("host.dwg"), &bytes).unwrap();
        let mut app = fresh();
        let i = app.active_tab;
        let mut br = BlockRecord::new("HOST");
        br.flags.is_xref = true;
        br.xref_path = dir.join("host.dwg").to_string_lossy().into_owned();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        app.refresh_xref_manager();
        let idx = app.xref_manager.entries.iter().position(|e| e.name == "INNER").expect("nested INNER listed");
        assert!(app.xref_manager.entries[idx].parent_key.is_some());
        app.xref_manager.selected.insert(idx);
        let start = app.command_line.history.len();
        app.xref_manager_op(XrefPaletteOp::Unload);
        let out = palette_output(&app, start);
        assert!(out.contains("INNER"), "got: {out:?}");
        assert_eq!(app.command_line.history.len(), start + 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn palette_reload_skips_images_without_clearing_flags() {
        // F7: palette Reload on an image/PDF row reports the drawing-only
        // error and leaves its unloaded flag untouched (no spurious
        // no-match after a flag clear).
        use codec::objects::{ImageDefinition, ObjectType};
        use crate::io::xref_model::RefKind;
        use crate::ui::window::xref_manager::XrefPaletteOp;
        let dir = palette_tmpdir("imgreload");
        let mut app = fresh();
        let i = app.active_tab;
        let h = app.tabs[i].scene.document.allocate_handle();
        let mut def = ImageDefinition::with_dimensions("img.png", 8, 8);
        def.handle = h;
        app.tabs[i].scene.document.objects.insert(h, ObjectType::ImageDefinition(def));
        let mut img = codec::entities::RasterImage::new(
            "img.png",
            codec::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        img.definition_handle = Some(h);
        app.tabs[i].scene.document.add_entity(codec::EntityType::RasterImage(img)).unwrap();
        app.tabs[i].current_path = Some(dir.join("host.dwg"));
        app.refresh_xref_manager();
        let idx = app.xref_manager.entries.iter().position(|e| e.kind == RefKind::Image).expect("image listed");
        let key = app.xref_manager.entries[idx].key;
        app.xref_manager.selected.insert(idx);
        app.tabs[i].xref_unloaded.add(key);
        let start = app.command_line.history.len();
        app.xref_manager_op(XrefPaletteOp::Reload);
        let out = palette_output(&app, start);
        assert!(out.contains("img.png"), "got: {out:?}");
        assert_eq!(app.command_line.history.len(), start + 1);
        assert!(app.tabs[i].xref_unloaded.is_unloaded(key), "image flag must stay untouched");
        std::fs::remove_dir_all(&dir).ok();
    }
}
