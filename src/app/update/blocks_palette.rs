//! The Blocks palette on the app side: tabs, the recent / favorite lists,
//! library folders, the insertion options, click and drag insertion, and the
//! block context menu.

use std::path::{Path, PathBuf};

use crate::app::settings::PaletteBlockRef;
use crate::app::{Message, OpenCADStudio};
use crate::modules::insert::insert_block::{BlockCatalog, InsertBlockCommand};
use crate::ui::window::block_palette::{
    BlockEntry, BlockPaletteMsg, Item, LibraryEntry, MenuAction, OptionMsg, Tab,
};
use iced::Task;

/// Library folders remembered in the Libraries list.
const MAX_LIBRARIES: usize = 10;

fn now_secs() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Block".to_string())
}

fn is_drawing(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"))
}

impl OpenCADStudio {
    pub(super) fn on_block_palette(&mut self, m: BlockPaletteMsg) -> Task<Message> {
        match m {
            BlockPaletteMsg::Search(s) => self.block_palette.search = s,
            BlockPaletteMsg::Tab(tab) => self.show_block_palette_tab(tab),
            BlockPaletteMsg::View(v) => {
                self.block_palette.view = v;
                self.persist_settings_if_changed();
            }
            BlockPaletteMsg::Refresh => self.refresh_block_palette(),
            BlockPaletteMsg::PickFile => {
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title(crate::t!("Select Drawing to Insert as Block").as_ref())
                            .add_filter(crate::t!("DWG/DXF Files").as_ref(), &["dwg", "dxf", "DWG", "DXF"])
                            .pick_file()
                            .await;
                        match handle {
                            Some(h) => Ok(crate::sys::handle_path(&h)),
                            None => Err("Cancelled".to_string()),
                        }
                    },
                    |r| Message::BlockPalette(BlockPaletteMsg::FilePicked(r)),
                );
            }
            BlockPaletteMsg::FilePicked(Ok(path)) => return self.palette_insert(Item::File(path), None),
            BlockPaletteMsg::FilePicked(Err(e)) | BlockPaletteMsg::LibraryPicked(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(&e);
                }
            }
            BlockPaletteMsg::Option(o) => self.on_block_option(o),
            BlockPaletteMsg::ToggleOptions => {
                self.block_palette.options_collapsed ^= true;
            }
            // Browsers expose no folder picker.
            #[cfg(target_arch = "wasm32")]
            BlockPaletteMsg::LibraryBrowse => {}
            #[cfg(not(target_arch = "wasm32"))]
            BlockPaletteMsg::LibraryBrowse => {
                return Task::perform(
                    async {
                        match rfd::AsyncFileDialog::new()
                            .set_title(crate::t!("Select a Block Library Folder").as_ref())
                            .pick_folder()
                            .await
                        {
                            Some(h) => Ok(crate::sys::handle_path(&h)),
                            None => Err("Cancelled".to_string()),
                        }
                    },
                    |r| Message::BlockPalette(BlockPaletteMsg::LibraryPicked(r)),
                );
            }
            BlockPaletteMsg::LibraryPicked(Ok(path)) => self.open_block_library(path),
            BlockPaletteMsg::LibrarySelect(path) => self.open_block_library(PathBuf::from(path)),
            BlockPaletteMsg::LibraryUp => {
                if let Some(parent) = self.block_palette.library_dir.as_ref().and_then(|d| d.parent()) {
                    let parent = parent.to_path_buf();
                    self.list_block_library(parent);
                }
            }
            BlockPaletteMsg::Press(item) => self.block_palette.pressed = Some(item),
            BlockPaletteMsg::Release(item) => {
                if self.block_palette.pressed.take().as_ref() == Some(&item) {
                    return match item {
                        Item::Folder(dir) => {
                            self.list_block_library(dir);
                            Task::none()
                        }
                        item => self.palette_insert(item, None),
                    };
                }
            }
            BlockPaletteMsg::Menu(item, action) => return self.on_block_menu(item, action),
        }
        Task::none()
    }

    /// BLOCKSPALETTE: open (expanded) on `tab`, or on the tab it was left on.
    pub(crate) fn open_blocks_palette(&mut self, tab: Option<Tab>) {
        self.show_block_palette = true;
        self.dock_expanded = Some(crate::ui::dock::PanelId::BlockPalette);
        self.show_block_palette_tab(tab.unwrap_or(self.block_palette.tab));
    }

    fn show_block_palette_tab(&mut self, tab: Tab) {
        self.block_palette.tab = tab;
        if tab == Tab::Libraries && self.block_palette.library_dir.is_none() {
            let start = Some(self.block_navigate.trim())
                .filter(|p| !p.is_empty() && *p != "." && Path::new(p).is_dir())
                .map(PathBuf::from)
                .or_else(|| self.block_palette.libraries.first().map(PathBuf::from));
            if let Some(dir) = start {
                self.list_block_library(dir);
            }
        }
        self.refresh_block_palette();
    }

    fn open_block_library(&mut self, root: PathBuf) {
        let key = root.display().to_string();
        let libraries = &mut self.block_palette.libraries;
        libraries.retain(|l| l != &key);
        libraries.insert(0, key);
        libraries.truncate(MAX_LIBRARIES);
        self.list_block_library(root);
        self.persist_settings_if_changed();
    }

    /// Show `dir`: its sub-folders, then its drawings, each by name.
    fn list_block_library(&mut self, dir: PathBuf) {
        let mut entries: Vec<LibraryEntry> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                // Hidden folders (".name") hold tool data, not blocks.
                if e.file_name().to_string_lossy().starts_with('.') {
                    return None;
                }
                if path.is_dir() {
                    Some(LibraryEntry { path, folder: true, thumb: None })
                } else if is_drawing(&path) {
                    let thumb = crate::io::thumbnail::read_handle(&path);
                    Some(LibraryEntry { path, folder: false, thumb })
                } else {
                    None
                }
            })
            .collect();
        entries.sort_by_key(|e| {
            (!e.folder, e.path.file_name().map(|n| n.to_string_lossy().to_lowercase()))
        });
        self.block_palette.library_dir = Some(dir);
        self.block_palette.library_entries = entries;
    }

    fn on_block_option(&mut self, o: OptionMsg) {
        let p = &mut self.block_palette;
        let number = |t: &str| t.trim().parse::<f64>().ok().filter(|v| v.is_finite());
        match o {
            OptionMsg::InsertionPoint(v) => p.options.insertion_point = v,
            OptionMsg::Scale(v) => p.options.scale = v,
            OptionMsg::Uniform(v) => p.options.uniform = v,
            OptionMsg::Rotation(v) => p.options.rotation = v,
            OptionMsg::AutoPlacement(v) => p.options.auto_placement = v,
            OptionMsg::Repeat(v) => p.options.repeat = v,
            OptionMsg::Explode(v) => p.options.explode = v,
            OptionMsg::X(t) => {
                if let Some(v) = number(&t).filter(|v| *v != 0.0) {
                    p.options.x = v;
                }
                p.option_text[0] = t;
            }
            OptionMsg::Y(t) => {
                if let Some(v) = number(&t).filter(|v| *v != 0.0) {
                    p.options.y = v;
                }
                p.option_text[1] = t;
            }
            OptionMsg::Z(t) => {
                if let Some(v) = number(&t).filter(|v| *v != 0.0) {
                    p.options.z = v;
                }
                p.option_text[2] = t;
            }
            OptionMsg::Angle(t) => {
                if let Some(v) = number(&t) {
                    p.options.angle = v;
                }
                p.option_text[3] = t;
            }
        }
        self.persist_settings_if_changed();
    }

    /// Start placing `name` as a palette click would.
    #[cfg(test)]
    pub(super) fn start_block_placement(&mut self, name: &str) {
        let _ = self.start_palette_insert(name, None);
    }

    fn start_palette_insert(&mut self, name: &str, drop_at: Option<glam::DVec3>) -> Task<Message> {
        use crate::command::CadCommand;
        let i = self.active_tab;
        let wires = self
            .block_palette
            .blocks
            .iter()
            .find(|b| b.name.eq_ignore_ascii_case(name))
            .map(|b| b.wires.clone())
            .unwrap_or_else(|| self.tabs[i].scene.block_preview_wires(name));
        let mut options = self.block_palette.options.clone();
        if drop_at.is_some() {
            // A dropped block goes in where it lands with the fixed values.
            options.insertion_point = false;
            options.scale = false;
            options.rotation = false;
            options.repeat = false;
        }
        let mut cmd = InsertBlockCommand::palette(name.to_string(), wires, &options);
        if let Some(point) = drop_at {
            cmd.set_point(point);
        }
        self.reset_command_start_state(i);
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        cmd.set_working_plane(plane);
        let first = cmd.initial();
        let prompt = cmd.prompt();
        self.tabs[i].active_cmd = Some(Box::new(cmd));
        self.block_palette.placing = Some(name.to_string());
        match first {
            Some(result) => self.apply_cmd_result(result),
            None => {
                self.command_line.push_info(&prompt);
                Task::none()
            }
        }
    }

    /// Insert `item` (a click, or a drop at `drop_at`), bringing its block
    /// into the drawing first when it lives elsewhere.
    pub(crate) fn palette_insert(&mut self, item: Item, drop_at: Option<glam::DVec3>) -> Task<Message> {
        match self.resolve_palette_block(&item) {
            Ok(name) => {
                self.refresh_block_palette();
                self.start_palette_insert(&name, drop_at)
            }
            Err(e) => {
                if !e.is_empty() {
                    self.command_line.push_error(&e);
                }
                Task::none()
            }
        }
    }

    fn block_exists(&self, name: &str) -> bool {
        self.tabs[self.active_tab].scene.document.block_records.get(name).is_some()
    }

    fn current_drawing_path(&self) -> String {
        self.tabs[self.active_tab]
            .current_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    fn resolve_palette_block(&mut self, item: &Item) -> Result<String, String> {
        match item {
            Item::Current(name) => Ok(name.clone()),
            Item::Ref(r) if r.drawing => self.block_from_drawing(PathBuf::from(&r.source)),
            Item::Ref(r) => {
                let elsewhere = !r.source.is_empty() && r.source != self.current_drawing_path();
                if self.block_exists(&r.name) && !(elsewhere && self.block_redefine_mode == 2) {
                    return Ok(r.name.clone());
                }
                if !elsewhere {
                    return Err(crate::tf!("Block \"{}\" not found.", r.name).into_owned());
                }
                let doc = crate::io::load_file(Path::new(&r.source)).map_err(|e| e.to_string())?;
                let redefine = self.block_exists(&r.name);
                self.import_block_definition(&doc, &r.name, redefine)?;
                Ok(r.name.clone())
            }
            Item::File(path) => self.block_from_drawing(path.clone()),
            Item::Folder(_) => Err(String::new()),
        }
    }

    /// The block standing for drawing `path`: the drawing's own block when
    /// the drawing already has one of that name (redefined from the file
    /// under BLOCKREDEFINEMODE 2), else the file inserted as a new block.
    fn block_from_drawing(&mut self, path: PathBuf) -> Result<String, String> {
        let stem = file_stem(&path);
        let name = if self.block_exists(&stem) && self.block_redefine_mode != 2 {
            stem
        } else {
            self.import_drawing_block(path.clone(), true)?
        };
        self.block_palette
            .file_sources
            .insert(name.to_ascii_uppercase(), path.display().to_string());
        Ok(name)
    }

    /// Bring block `name` (and what it needs) from `source` into the active
    /// drawing; with `redefine` an existing definition takes the source's.
    fn import_block_definition(
        &mut self,
        source: &codec::CadDocument,
        name: &str,
        redefine: bool,
    ) -> Result<(), String> {
        if source.block_records.get(name).is_none() {
            return Err(crate::tf!("Block \"{}\" not found.", name).into_owned());
        }
        let probe = codec::EntityType::Insert(codec::entities::Insert::new(
            name.to_string(),
            codec::types::Vector3::ZERO,
        ));
        let deps = crate::app::ClipboardDeps::capture(source, std::slice::from_ref(&probe));
        let i = self.active_tab;
        self.push_undo_snapshot(i, "INSERT");
        self.merge_dependencies(i, &deps);
        for def in deps.blocks {
            let scene = &mut self.tabs[i].scene;
            if redefine && def.name.eq_ignore_ascii_case(name) {
                scene.redefine_block_raw(&def.name, def.base_point, def.entities);
            } else if scene.document.block_records.get(&def.name).is_none() {
                scene.define_block_raw(&def.name, def.base_point, def.entities);
            }
        }
        self.tabs[i].scene.populate_meshes_from_document();
        self.tabs[i].dirty = true;
        Ok(())
    }

    fn palette_ref(&self, item: &Item) -> Option<PaletteBlockRef> {
        let time = now_secs();
        match item {
            Item::Current(name) => Some(PaletteBlockRef {
                name: name.clone(),
                source: self.current_drawing_path(),
                time,
                drawing: false,
            }),
            Item::Ref(r) => Some(r.clone()),
            Item::File(path) => Some(PaletteBlockRef {
                name: file_stem(path),
                source: path.display().to_string(),
                time,
                drawing: true,
            }),
            Item::Folder(_) => None,
        }
    }

    fn on_block_menu(&mut self, item: Item, action: MenuAction) -> Task<Message> {
        let same = |a: &PaletteBlockRef, b: &PaletteBlockRef| {
            a.name.eq_ignore_ascii_case(&b.name) && a.source == b.source
        };
        match action {
            MenuAction::Insert => return self.palette_insert(item, None),
            MenuAction::Redefine => {
                let result = match &item {
                    Item::Ref(r) if r.drawing => self.import_drawing_block(PathBuf::from(&r.source), true),
                    Item::File(path) => self.import_drawing_block(path.clone(), true),
                    Item::Ref(r) if !r.source.is_empty() => crate::io::load_file(Path::new(&r.source))
                        .map_err(|e| e.to_string())
                        .and_then(|doc| self.import_block_definition(&doc, &r.name, true))
                        .map(|_| r.name.clone()),
                    _ => return Task::none(),
                };
                match result {
                    Ok(name) => {
                        self.command_line
                            .push_output(&crate::tf!("Block \"{name}\" redefined."));
                        self.refresh_block_palette();
                    }
                    Err(e) => self.command_line.push_error(&e),
                }
            }
            MenuAction::Favorite => {
                if let Some(r) = self.palette_ref(&item) {
                    let favorites = &mut self.block_palette.favorites;
                    favorites.retain(|f| !same(f, &r));
                    favorites.insert(0, r);
                    self.persist_settings_if_changed();
                    self.refresh_block_palette();
                }
            }
            MenuAction::Unfavorite => {
                if let Some(r) = self.palette_ref(&item) {
                    self.block_palette.favorites.retain(|f| !same(f, &r));
                    self.persist_settings_if_changed();
                }
            }
            MenuAction::Edit => {
                let name = item.name();
                let i = self.active_tab;
                let reference = self.tabs[i].scene.document.entities().find_map(|e| match e {
                    codec::EntityType::Insert(ins) if ins.block_name.eq_ignore_ascii_case(&name) => {
                        Some(ins.common.handle)
                    }
                    _ => None,
                });
                match reference {
                    Some(handle) => return self.dispatch_command(&format!("BEDIT_BEGIN:{}", handle.value())),
                    None => self.command_line.push_error(&crate::tf!(
                        "Block \"{name}\" has no reference in the drawing to edit."
                    )),
                }
            }
            MenuAction::Remove => match self.block_palette.tab {
                Tab::Current => {
                    let name = item.name();
                    let i = self.active_tab;
                    let referenced = self.tabs[i].scene.document.entities().any(|e| {
                        matches!(e, codec::EntityType::Insert(ins) if ins.block_name.eq_ignore_ascii_case(&name))
                    });
                    if referenced {
                        self.command_line.push_error(&crate::tf!(
                            "Block \"{name}\" is referenced in the drawing and cannot be removed."
                        ));
                    } else {
                        self.push_undo_snapshot(i, "PURGE");
                        crate::app::control::erase_block_definition(&mut self.tabs[i].scene, &name);
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(&crate::tf!("Block \"{name}\" removed."));
                        self.refresh_block_palette();
                    }
                }
                Tab::Recent | Tab::Favorites => {
                    if let Some(r) = self.palette_ref(&item) {
                        let list = if self.block_palette.tab == Tab::Recent {
                            &mut self.block_palette.recent
                        } else {
                            &mut self.block_palette.favorites
                        };
                        list.retain(|f| !same(f, &r));
                        self.persist_settings_if_changed();
                    }
                }
                Tab::Libraries => {}
            },
        }
        Task::none()
    }

    /// A press in the drawing ends any palette press; a release there with
    /// a block still pressed drops it at the cursor.
    pub(super) fn take_block_drop(&mut self) -> Option<Item> {
        self.block_palette.pressed.take()
    }

    pub(super) fn drop_block(&mut self, item: Item) -> Task<Message> {
        let at = self.tabs[self.active_tab].last_cursor_world;
        self.palette_insert(item, Some(at))
    }

    /// Put `name` at the top of the Recent list (BLOCKMRULIST long).
    pub(crate) fn note_recent_block(&mut self, name: &str) {
        let (source, drawing) = match self.block_palette.file_sources.get(&name.to_ascii_uppercase()) {
            Some(path) => (path.clone(), true),
            None => (self.current_drawing_path(), false),
        };
        let entry = PaletteBlockRef {
            name: if drawing { file_stem(Path::new(&source)) } else { name.to_string() },
            source,
            time: now_secs(),
            drawing,
        };
        let recent = &mut self.block_palette.recent;
        recent.retain(|r| !(r.name.eq_ignore_ascii_case(&entry.name) && r.source == entry.source));
        recent.insert(0, entry);
        self.trim_recent_blocks();
    }

    pub(crate) fn trim_recent_blocks(&mut self) {
        self.block_palette.recent.truncate(self.block_mru_list as usize);
    }

    /// Rebuild the Current Drawing list, and the previews kept for blocks
    /// of other drawings and for whole drawings.
    pub(crate) fn refresh_block_palette(&mut self) {
        let i = self.active_tab;
        let names = self.tabs[i].scene.custom_block_names();
        self.block_palette.cached_names = names.clone();
        self.block_palette.source_tab_id = Some(self.tabs[i].id);
        self.block_palette.source_block_epoch = self.tabs[i].scene.block_epoch;
        let doc = &self.tabs[i].scene.document;
        let annotative = |name: &str| {
            doc.block_records.get(name).is_some_and(|br| {
                matches!(
                    doc.get_entity(br.block_entity_handle),
                    Some(codec::EntityType::Block(b)) if b.common.extended_data.records().iter().any(|r| r.application_name == "AcadAnnotative")
                )
            })
        };
        let entries: Vec<BlockEntry> = names
            .into_iter()
            .map(|name| BlockEntry {
                wires: self.tabs[i].scene.block_preview_wires(&name),
                annotative: annotative(&name),
                dynamic: false,
                name,
            })
            .collect();
        let here = self.current_drawing_path();
        for entry in &entries {
            let key = format!("{here}|{}", entry.name.to_ascii_uppercase());
            self.block_palette.ref_wires.insert(key, entry.wires.clone());
        }
        self.block_palette.blocks = entries;
        let drawings: Vec<String> = self
            .block_palette
            .recent
            .iter()
            .chain(&self.block_palette.favorites)
            .filter(|r| r.drawing && !self.block_palette.file_thumbs.contains_key(&r.source))
            .map(|r| r.source.clone())
            .collect();
        for source in drawings {
            let thumb = crate::io::thumbnail::read_handle(Path::new(&source));
            self.block_palette.file_thumbs.insert(source, thumb);
        }
    }

    /// Cheap per-update check: rebuild when the active drawing's definitions
    /// changed, even when their names happen to stay the same.
    pub(crate) fn refresh_block_palette_if_stale(&mut self) {
        if !self.show_block_palette {
            return;
        }
        let i = self.active_tab;
        if self.block_palette.source_tab_id != Some(self.tabs[i].id)
            || self.block_palette.source_block_epoch != self.tabs[i].scene.block_epoch
        {
            self.refresh_block_palette();
        }
    }

    /// What `-INSERT`'s name step knows about the drawing's blocks.
    pub(crate) fn block_catalog(&self) -> BlockCatalog {
        let doc = &self.tabs[self.active_tab].scene.document;
        let host = doc.header.insertion_units;
        let mut user = Vec::new();
        let mut unnamed = 0;
        let mut units = rustc_hash::FxHashMap::default();
        for br in doc.block_records.iter() {
            if br.flags.is_xref || br.name.eq_ignore_ascii_case("*Model_Space") || br.name.to_ascii_uppercase().starts_with("*PAPER_SPACE") {
                continue;
            }
            if br.name.starts_with('*') {
                unnamed += 1;
                continue;
            }
            let factor = crate::app::properties::insert_unit_scale(host, br.units).unwrap_or(1.0);
            units.insert(
                br.name.to_ascii_uppercase(),
                format!(
                    "Units: {}   Conversion:{:>10}",
                    crate::app::properties::insunits_name(br.units),
                    crate::app::properties::format_unit_factor(factor)
                ),
            );
            user.push(br.name.clone());
        }
        user.sort_by_key(|n| n.to_ascii_uppercase());
        let search_path = std::env::current_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        BlockCatalog { user, unnamed, units, search_path }
    }
}
