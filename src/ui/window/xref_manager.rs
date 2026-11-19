//! Reference Manager palette — list of external references with operations.
//!
//! Task 8b build: the toolbar's mutating affordances are wired to the same
//! engine fns the CLI uses, operating on the multi-selection with per-item
//! report lines. Session state (unloaded set, stat cache) lives per-tab on
//! `DocumentTab`; this panel only caches display rows plus text inputs.

use crate::app::Message;
use crate::io::xref::collect_entries_with_prev;
use crate::io::xref_model::{
    normalize_lexical, Pathtype, RefKind, RefStatus, RefType, ReferenceEntry,
};
use crate::ui::kit;
use crate::ui::kit::menu::{self, Item};
use crate::ui::style::common::muted_style;
use crate::ui::style::text as ui_text;
use crate::ui::style::{chrome, kit_metrics as km};
use document::Document;
use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
use iced::Padding;
use iced::{Background, Element, Fill, Length, Theme};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// `true` on web builds, where the filesystem and native file pickers do not
/// exist. Mutating affordances stay compiled but render disabled; the
/// read-only list renders everywhere.
const IS_WASM: bool = cfg!(target_arch = "wasm32");

/// Fixed table height so the details pane below keeps stable space.
const TABLE_H: f32 = 480.0;
/// Minimum table area height under split dragging.
const TABLE_MIN_H: f32 = 120.0;
/// Minimum table content width so columns stay readable in narrow docks.
const TABLE_MIN_W: f32 = 480.0;
/// Inter-column gutter shared by header, rows, and grab zones so columns align.
const COL_GUTTER: f32 = km::CELL_PAD_X;
/// Tree-mode text size: a step above the table cells, like the
/// industry tree.
/// Indent per tree depth level.
const INDENT_W: f32 = km::INDENT;
/// Height of a reference row, the grid's row.
const ROW_H: f32 = km::ROW_H;

/// Dashed vertical tree guide (one per ancestor level), stretched to the row.
const TREE_GUIDE_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 100\" preserveAspectRatio=\"none\"><line x1=\"8\" y1=\"0\" x2=\"8\" y2=\"100\" stroke=\"#808080\" stroke-width=\"1.5\" stroke-dasharray=\"4,4\"/></svg>";

/// Shared parsed handle for [`TREE_GUIDE_SVG`] (parsed once, not per row).
fn tree_guide_handle() -> iced::widget::svg::Handle {
    static HANDLE: std::sync::OnceLock<iced::widget::svg::Handle> = std::sync::OnceLock::new();
    HANDLE
        .get_or_init(|| iced::widget::svg::Handle::from_memory(TREE_GUIDE_SVG))
        .clone()
}

/// Indent cells for `depth` ancestor levels: dashed guides explaining the
/// hierarchy, same total width as the old blank indent.
fn tree_indent(depth: u32) -> iced::widget::Row<'static, crate::app::Message> {
    let mut row = iced::widget::Row::new().spacing(0);
    for _ in 0..depth {
        row = row.push(
            container(
                iced::widget::svg(tree_guide_handle())
                    .width(Length::Fixed(INDENT_W))
                    .height(Fill),
            )
            .align_y(iced::Center),
        );
    }
    row
}
const TREE_FONT_SZ: f32 = ui_text::UI;
/// A row's side padding: its text a cell padding in.
const ROW_PAD: Padding = Padding {
    top: 0.0,
    bottom: 0.0,
    left: km::CELL_PAD_X,
    right: km::CELL_PAD_X,
};
/// A band of its own under the table (the missing-files notice).
const BAND_PAD: Padding = Padding {
    top: km::TOOL_GAP,
    bottom: km::TOOL_GAP,
    left: km::GRID_INSET_L,
    right: km::GRID_INSET_R,
};
/// The preview image's width and the preview field's height.
const PREVIEW_W: f32 = 220.0;
const PREVIEW_H: f32 = 120.0;

/// A table or tree row: `row_selected` when selected, the body otherwise.
fn row_style(selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| container::Style {
        background: selected.then(|| Background::Color(chrome::row_selected(theme))),
        text_color: Some(chrome::text(theme)),
        ..Default::default()
    }
}
/// Longest edge of a preview image, in pixels.
const PREVIEW_MAX: u32 = 256;

/// One visible row in [`XrefManagerPanel::display_rows`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayRow {
    /// Index into [`XrefManagerPanel::entries`].
    pub index: usize,
    /// Tree depth (0 in list mode and for roots).
    pub depth: u32,
    /// True for entries enumerated from a nested file (never rendered).
    pub is_nested: bool,
}

/// Display-row index reserved for the host-drawing pseudo-row (list first
/// row, tree root). It addresses no entry: row rendering special-cases it
/// and selection ignores it.
pub const HOST_ROW: usize = usize::MAX;

/// Palette state for the Reference Manager.
///
/// Session state (unloaded set, stat cache) lives per-tab on `DocumentTab`
/// and is passed into [`refresh`](XrefManagerPanel::refresh); this panel
/// only caches display rows, expansion, selection, and text inputs.
#[derive(Debug, Clone)]
pub struct XrefManagerPanel {
    /// Cached [`collect_entries_with_prev`] output for the active drawing.
    pub entries: Vec<ReferenceEntry>,
    /// Display name of the host drawing (first list row / tree root).
    pub host_name: String,
    /// Absolute path of the host drawing, if saved (host row Saved Path).
    pub host_path: String,
    /// Multi-selected entry indices — consumed by batch path ops.
    pub selected: HashSet<usize>,
    /// Anchor entry driving the details pane (last row clicked).
    pub anchor: Option<usize>,
    /// List (`false`) vs tree (`true`) presentation.
    pub tree: bool,
    /// Parent keys expanded in tree mode. Empty by default = collapsed.
    pub expanded: HashSet<u64>,
    /// Parent key → child entry indices, rebuilt on every refresh.
    pub children: HashMap<u64, Vec<usize>>,
    /// Entry indices enumerated from nested files (not the host drawing).
    pub nested: HashSet<usize>,
    /// Document tab id that produced `entries` (stale check).
    pub source_tab_id: Option<u64>,
    /// `edit_revision` that produced `entries` — the palette auto-rescans
    /// when the tab's revision moves (reuses the undo-snapshot counter, so
    /// CLI and palette mutations both trip it).
    pub source_edit_revision: u64,
    /// Reference-table column widths: Reference, Status, Size, Type, Date,
    /// Saved Path. Draggable via the header dividers (clamped).
    pub col_widths: [f32; 6],
    /// Reference-table area height. Draggable via the split divider above
    /// the Details/Preview pane (clamped).
    pub table_h: f32,
    /// Open dropdown menus (one at a time; dismissed together).
    pub attach_open: bool,
    pub refresh_open: bool,
    pub path_open: bool,
    pub row_change_path_open: bool,
    /// Decoded preview images keyed by `(entry key, resolved path)` — rebuilt
    /// on every refresh for the anchor entry only, so the per-frame `view`
    /// stays pure and file I/O never happens during rendering.
    pub previews: HashMap<(u64, String), iced::widget::image::Handle>,
    /// The anchor entry's preview to decode on a worker; the app takes it
    /// ([`Self::take_preview_request`]).
    preview_request: Option<PreviewRequest>,
    /// Previews being decoded, so a selection asks for each one once.
    preview_pending: std::collections::HashSet<(u64, String)>,
    /// Counts the refreshes: a decode started before the last one is dropped.
    preview_generation: u64,
    /// Details (`false`) vs Preview (`true`) lower pane.
    pub show_preview: bool,
}

/// Default reference-table column widths: Reference, Status, Size, Type,
/// Date, Saved Path.
pub const DEFAULT_COL_WIDTHS: [f32; 6] = [150.0, 104.0, 76.0, 76.0, 96.0, 200.0];
/// Narrowest any reference-table column drags to.
pub const COL_MIN_W: f32 = 48.0;
/// Widest any reference-table column drags to.
pub const COL_MAX_W: f32 = 420.0;

impl Default for XrefManagerPanel {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            host_name: String::new(),
            host_path: String::new(),
            selected: HashSet::new(),
            anchor: None,
            tree: false,
            expanded: HashSet::new(),
            children: HashMap::new(),
            nested: HashSet::new(),
            source_tab_id: None,
            source_edit_revision: 0,
            col_widths: DEFAULT_COL_WIDTHS,
            table_h: TABLE_H,
            attach_open: false,
            refresh_open: false,
            path_open: false,
            row_change_path_open: false,
            previews: HashMap::new(),
            preview_request: None,
            preview_pending: std::collections::HashSet::new(),
            preview_generation: 0,
            show_preview: false,
        }
    }
}

/// Row-pick modifier mode for [`XrefManagerPanel::click_select`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectExtend {
    /// Plain click: exactly one row selected.
    Single,
    /// Ctrl/Cmd-click: toggle one row.
    Toggle,
    /// Shift-click: contiguous range from the anchor.
    Range,
}

/// Selection-gated palette operation the toolbar offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrefPaletteOp {
    Open,
    Detach,
    Unload,
    Reload,
    Bind,
    Overlay,
    Attach,
    Pathtype(Pathtype),
}

/// Stable row identity carried by row messages instead of the positional
/// display index (#49): a rescan/reorder between view and dispatch must not
/// retarget the row. Same `(key, saved_path)` pair `refresh` uses to preserve
/// selection (key alone can collide across host/nested scopes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XrefRowId {
    pub key: u64,
    pub saved_path: String,
}

impl XrefRowId {
    pub fn of(key: u64, saved_path: &str) -> Self {
        Self {
            key,
            saved_path: saved_path.to_string(),
        }
    }
}

impl XrefManagerPanel {
    /// Rebuild `entries` from `doc` via [`collect_entries_with_prev`] and
    /// recompute the parent → children linkage for tree mode. Selection/anchor
    /// survive by `(key, saved_path)` identity; dead indices are dropped.
    ///
    /// `unloaded`/`prev` are the active tab's session sets, so CLI and
    /// palette agree. Returns fresh `(key, mtime)` baselines for every
    /// `Loaded` entry — the caller writes them into the tab's stat cache,
    /// which makes `Stale` detectable from the second refresh on.
    /// Wasm-safe by construction: the only I/O is `collect_entries`' path
    /// stat plus `load_file` for nested enumeration, both already wasm-safe.
    pub fn refresh(
        &mut self,
        doc: &Document,
        base_dir: &Path,
        unloaded: &HashSet<crate::io::xref_model::UnloadKey>,
        prev: &HashMap<u64, SystemTime>,
        host_name: &str,
        host_path: &str,
    ) -> Vec<(u64, SystemTime)> {
        self.host_name = host_name.to_string();
        self.host_path = host_path.to_string();
        let old = std::mem::take(&mut self.entries);
        let sel_ids: HashSet<(u64, String)> = self
            .selected
            .iter()
            .filter_map(|&i| old.get(i))
            .map(|e| (e.key, e.saved_path.clone()))
            .collect();
        let anchor_id: Option<(u64, String)> = self
            .anchor
            .and_then(|i| old.get(i))
            .map(|e| (e.key, e.saved_path.clone()));

        let entries = collect_entries_with_prev(doc, base_dir, unloaded, prev);
        // Direct references, keyed by (handle, saved path) exactly as
        // `collect_entries` emits them. The saved path disambiguates a nested
        // entry whose foreign handle happens to collide with a host handle.
        let direct_ids = direct_identities(doc);
        let mut nested = HashSet::new();
        for (i, e) in entries.iter().enumerate() {
            if !direct_ids.contains(&(e.key, e.saved_path.clone())) {
                nested.insert(i);
            }
        }
        // Parent linkage: each loaded drawing root's file is opened read-only
        // and its direct reference paths are matched (by lexical identity)
        // against nested entries. First parent wins; unmatchable nested
        // entries stay unclaimed and render as roots so nothing vanishes.
        let mut children: HashMap<u64, Vec<usize>> = HashMap::new();
        let mut claimed: HashSet<usize> = HashSet::new();
        let direct_order: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(i, _)| !nested.contains(i))
            .map(|(i, _)| i)
            .collect();
        for &pi in &direct_order {
            let parent = &entries[pi];
            if parent.kind != RefKind::DwgXref
                || parent.status != RefStatus::Loaded
                || parent.found_at.is_none()
            {
                continue;
            }
            let found = parent.found_at.as_deref().unwrap_or("");
            let child_paths: HashSet<String> = match crate::io::load_file(Path::new(found)) {
                Ok(nested_doc) => nested_doc
                    .block_records
                    .iter()
                    .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                    .map(|br| normalize_lexical(&br.xref_path))
                    .collect(),
                Err(_) => continue,
            };
            for (ci, child) in entries.iter().enumerate() {
                if !nested.contains(&ci) || claimed.contains(&ci) {
                    continue;
                }
                if child_paths.contains(&normalize_lexical(&child.saved_path)) {
                    children.entry(parent.key).or_default().push(ci);
                    claimed.insert(ci);
                }
            }
        }

        let live_keys: HashSet<u64> = entries.iter().map(|e| e.key).collect();
        self.entries = entries;
        self.nested = nested;
        self.children = children;
        self.selected = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| sel_ids.contains(&(e.key, e.saved_path.clone())))
            .map(|(i, _)| i)
            .collect();
        self.anchor = anchor_id.and_then(|(k, p)| {
            self.entries
                .iter()
                .position(|e| e.key == k && e.saved_path == p)
        });
        // Previews decode on a worker, asked for here and on selection change
        // (view stays pure): the anchor entry only, and only under single
        // selection. A decode started before this refresh is dropped.
        self.previews.clear();
        self.preview_request = None;
        self.preview_pending.clear();
        self.preview_generation += 1;
        self.ensure_preview_for_anchor();
        self.expanded.retain(|k| live_keys.contains(k));
        self.entries
            .iter()
            .filter(|e| e.status == RefStatus::Loaded)
            .filter_map(|e| e.modified.map(|m| (e.key, m)))
            .collect()
    }

    /// Resolve a row id to its current display index (entries reorder on
    /// every rescan). `None` = row gone meanwhile; callers no-op (#49).
    pub fn row_index(&self, id: &XrefRowId) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.key == id.key && e.saved_path == id.saved_path)
    }

    /// Click handling for row picks with normal GUI list semantics:
    /// - plain click selects exactly one row (previous selection clears);
    /// - Ctrl/Cmd-click toggles one row;
    /// - Shift-click extends a contiguous range from the anchor (or the
    ///   clicked row when there is no anchor yet).
    /// Tree mode always single-selects per spec, whatever the modifiers.
    pub fn click_select(&mut self, id: &XrefRowId, extend: SelectExtend) {
        let Some(index) = self.row_index(id) else {
            return;
        };
        if index == HOST_ROW || index >= self.entries.len() {
            return;
        }
        if self.tree {
            // Tree view selects a single file reference at a time.
            if self.selected.contains(&index) {
                return;
            }
            self.selected.clear();
            self.selected.insert(index);
            self.anchor = Some(index);
            self.ensure_preview_for_anchor();
            return;
        }
        match extend {
            SelectExtend::Single => {
                self.selected.clear();
                self.selected.insert(index);
                self.anchor = Some(index);
            }
            SelectExtend::Toggle => {
                if !self.selected.remove(&index) {
                    self.selected.insert(index);
                    self.anchor = Some(index);
                } else if self.anchor == Some(index) {
                    self.anchor = self.selected.iter().copied().max();
                }
            }
            SelectExtend::Range => {
                // The anchor stays fixed across extends: each Shift-click
                // re-ranges from the original anchor (plain click moves it).
                // Moving it every time collapsed the range and dropped rows.
                let from = self.anchor.unwrap_or(index);
                let (lo, hi) = if from <= index {
                    (from, index)
                } else {
                    (index, from)
                };
                self.selected.clear();
                self.selected
                    .extend((lo..=hi).filter(|i| *i < self.entries.len()));
                if self.anchor.is_none() {
                    self.anchor = Some(index);
                }
            }
        }
        self.ensure_preview_for_anchor();
    }

    /// Legacy toggle entry point: Ctrl-style toggle in list mode, single
    /// select in tree mode. Kept for tests and non-pointer callers.
    pub fn toggle_select(&mut self, index: usize) {
        let Some(id) = self
            .entries
            .get(index)
            .map(|e| XrefRowId::of(e.key, &e.saved_path))
        else {
            return;
        };
        self.click_select(&id, SelectExtend::Toggle);
    }

    /// Right-click selection: select the row alone when it is not already
    /// selected (standard right-click behavior — the menu then acts on the
    /// selection, which now includes this row). Keeps multi-selections when
    /// right-clicking inside them.
    pub fn right_click_select(&mut self, id: &XrefRowId) {
        let Some(index) = self.row_index(id) else {
            return;
        };
        if index == HOST_ROW || index >= self.entries.len() {
            return;
        }
        if !self.selected.contains(&index) {
            self.click_select(id, SelectExtend::Single);
        } else {
            self.ensure_preview_for_anchor();
        }
    }

    /// Flip list/tree presentation.
    pub fn toggle_tree(&mut self) {
        self.tree = !self.tree;
    }

    fn ensure_preview_for_anchor(&mut self) {
        if self.selected.len() > 1 {
            return;
        }
        let Some(a) = self.anchor.and_then(|i| self.entries.get(i)) else {
            return;
        };
        let Some(found) = a.found_at.clone() else {
            return;
        };
        let key = (a.key, found.clone());
        if self.previews.contains_key(&key) || self.preview_pending.contains(&key) {
            return;
        }
        self.preview_pending.insert(key.clone());
        self.preview_request = Some(PreviewRequest {
            generation: self.preview_generation,
            key,
            entry: a.clone(),
        });
    }

    /// The preview to decode on a worker ([`read_preview`]), if a selection
    /// or a refresh asked for one.
    pub fn take_preview_request(&mut self) -> Option<PreviewRequest> {
        self.preview_request.take()
    }

    /// A decode ended: its preview goes into the cache, unless the panel
    /// was refreshed after the decode started. `None`: the file has no
    /// preview, and the placeholder shows.
    pub fn preview_read(
        &mut self,
        generation: u64,
        key: (u64, String),
        preview: Option<iced::widget::image::Handle>,
    ) {
        if generation != self.preview_generation {
            return;
        }
        self.preview_pending.remove(&key);
        if let Some(preview) = preview {
            self.previews.insert(key, preview);
        }
    }

    /// Decode the asked-for preview here, as the app's worker does.
    #[cfg(test)]
    fn read_preview_now(&mut self) {
        if let Some(request) = self.take_preview_request() {
            let preview = read_preview(&request.entry);
            self.preview_read(request.generation, request.key, preview);
        }
    }

    /// Total table content width: column widths plus inter-column gutters.
    /// Header, rows, and grab zones all share this geometry so columns align.
    pub fn table_content_width(&self) -> f32 {
        (self.col_widths.iter().sum::<f32>() + COL_GUTTER * 5.0).max(TABLE_MIN_W)
    }

    /// Apply a horizontal divider drag to column `i` (clamped). Called from
    /// the dock drag handler with the pointer delta.
    pub fn drag_col_by(&mut self, i: usize, dx: f32) {
        if let Some(w) = self.col_widths.get_mut(i) {
            *w = (*w + dx).clamp(COL_MIN_W, COL_MAX_W);
        }
    }

    /// Apply a vertical split-divider drag to the table area height
    /// (clamped). Called with the pointer delta.
    pub fn drag_table_by(&mut self, dy: f32) {
        self.table_h = (self.table_h + dy).clamp(TABLE_MIN_H, 1200.0);
    }

    /// Expand/collapse one tree parent (no-op for unknown keys).
    pub fn toggle_expand(&mut self, parent_key: u64) {
        if !self.expanded.remove(&parent_key) && self.children.contains_key(&parent_key) {
            self.expanded.insert(parent_key);
        }
    }

    /// True when the selection includes at least one nested row. Nested rows
    /// stay read-only — every mutating button disables with a reason then.
    pub fn selection_has_nested(&self) -> bool {
        self.selected.iter().any(|i| self.nested.contains(i))
    }

    /// Selected entry indices that are directly actionable (nested rows
    /// excluded — they have no host definition to mutate).
    pub fn actionable_selection(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self
            .selected
            .iter()
            .copied()
            .filter(|i| *i < self.entries.len() && !self.nested.contains(i))
            .collect();
        out.sort_unstable();
        out
    }

    /// Flat render order for the current mode. Tree mode emits roots
    /// (direct entries plus unclaimed nested ones) with expanded children
    /// inline, skipping repeats by normalized saved path so a cyclic closure
    /// can never recurse on screen.
    pub fn display_rows(&self) -> Vec<DisplayRow> {
        // The host drawing leads in both modes (list first row, tree root).
        let host = DisplayRow {
            index: HOST_ROW,
            depth: 0,
            is_nested: false,
        };
        if !self.tree {
            let mut rows = vec![host];
            rows.extend(
                self.entries
                    .iter()
                    .enumerate()
                    .map(|(index, _)| DisplayRow {
                        index,
                        depth: 0,
                        is_nested: self.nested.contains(&index),
                    }),
            );
            return rows;
        }
        fn append_tree(
            panel: &XrefManagerPanel,
            index: usize,
            depth: u32,
            seen: &mut HashSet<(String, String)>,
            rows: &mut Vec<DisplayRow>,
        ) {
            let Some(entry) = panel.entries.get(index) else {
                return;
            };
            if !claim_path(seen, &entry.saved_path, &entry.name) {
                return;
            }
            rows.push(DisplayRow {
                index,
                depth,
                is_nested: panel.nested.contains(&index),
            });
            if !panel.expanded.contains(&entry.key) {
                return;
            }
            if let Some(children) = panel.children.get(&entry.key) {
                for &child in children {
                    append_tree(panel, child, depth + 1, seen, rows);
                }
            }
        }

        let mut rows = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        // Roots first, then recursively expanded descendants.
        for (index, _) in self.entries.iter().enumerate() {
            let dominated = self.children.values().any(|kids| kids.contains(&index));
            if self.nested.contains(&index) && dominated {
                continue;
            }
            append_tree(self, index, 0, &mut seen, &mut rows);
        }
        // The host drawing leads: first list row, tree root. In tree mode
        // everything below renders one level deeper as its descendants.
        if self.tree {
            for row in rows.iter_mut() {
                row.depth += 1;
            }
        }
        rows.insert(
            0,
            DisplayRow {
                index: HOST_ROW,
                depth: 0,
                is_nested: false,
            },
        );
        rows
    }

    /// Render the palette as a docked side panel (EXTERNALREFERENCES).
    ///
    /// `missing` is the active tab's open-time NotFound count — a neutral
    /// notice renders while non-zero. Mirrors the block palette's dock
    /// chrome (title, pin, close) and sizing conventions.
    pub fn view<'a>(
        &'a self,
        width: f32,
        chrome: crate::ui::dock::Chrome,
        missing: usize,
        doc: &'a Document,
    ) -> Element<'a, Message> {
        use crate::ui::dock::PanelId;
        let title_bar = crate::ui::dock::title_bar(
            PanelId::ExternalReferences,
            format!(
                "{} ({})",
                crate::t!("External References").as_ref(),
                self.entries.len()
            ),
            chrome,
        );
        // Table content width: column widths plus gutters, stretched to the
        // dock when wider so rows fill the panel; narrower docks sidescroll.
        let table_w = (width - km::GRID_INSET_L - km::GRID_INSET_R - 2.0 * km::CELL_PAD_X)
            .max(self.table_content_width());
        // ── Toolbar: split-button groups per spec ───────────────────────
        // Attach ▾ (default DWG), Refresh ▾ (default Refresh), Change Path ▾.
        // The main button runs the default; the triangle opens the rest in an
        // overlay menu. Formats without an attach command in this build
        // (DWF/DGN/point clouds/coordination models) are omitted, not dead.
        let web_tip = crate::t!(
            "File attach is not available on web — the reference list below is read-only."
        )
        .into_owned();
        let attach = if IS_WASM {
            toolbar_tip(crate::t!("Attach DWG").into_owned(), web_tip.clone())
        } else {
            split_button(
                crate::t!("Attach DWG").into_owned(),
                Some(Message::XAttachPick),
                self.attach_open,
                Message::XrefManagerAttachMenu,
                vec![
                    menu_item(
                        crate::t!("Attach Image").into_owned(),
                        Some(Message::ImagePick),
                        None,
                    ),
                    menu_item(
                        crate::t!("Embed Image (in drawing)").into_owned(),
                        Some(Message::ImageEmbedPick),
                        None,
                    ),
                    menu_item(
                        crate::t!("Attach PDF").into_owned(),
                        Some(Message::PdfAttachPick),
                        None,
                    ),
                ],
            )
        };
        let refresh = if IS_WASM {
            toolbar_tip(crate::t!("Refresh").into_owned(), web_tip.clone())
        } else {
            split_button(
                crate::t!("Refresh").into_owned(),
                Some(Message::XrefManagerRefresh),
                self.refresh_open,
                Message::XrefManagerRefreshMenu,
                vec![menu_item(
                    crate::t!("Reload All References").into_owned(),
                    Some(Message::XrefManagerReloadAll),
                    None,
                )],
            )
        };
        // List / Tree view buttons with active-state coloring: the current
        // mode renders in the primary accent (blue), the other is clickable.
        let mode_btn = |label: String, active: bool| -> Element<'static, Message> {
            // The mode shown sends nothing (it stays on), but stays enabled
            // so it reads as the active one, not a disabled one.
            let msg = if active {
                Message::Noop
            } else {
                Message::XrefManagerToggleTree
            };
            kit::text_button(label, Some(msg), active)
        };
        let list_btn = mode_btn(crate::t!("List").into_owned(), !self.tree);
        let tree_btn = mode_btn(crate::t!("Tree").into_owned(), self.tree);
        // Reference operations (Detach/Unload/Reload/Bind/Overlay/Attach)
        // live in the per-row right-click menu, not the toolbar.
        // Change Path group: greyed until a reference whose path can change
        // (a direct, non-nested row) is selected. Each option carries its own
        // gate so non-executable choices render greyed with the reason.
        let has_direct = self
            .entries
            .iter()
            .enumerate()
            .any(|(i, _)| self.selected.contains(&i) && !self.nested.contains(&i));
        let single_direct_anchor = self.anchor.is_some_and(|a| {
            self.selected.len() == 1 && self.entries.get(a).is_some() && !self.nested.contains(&a)
        });
        let host_saved = !self.host_path.is_empty();
        let web_readonly: Option<String> = IS_WASM.then(|| {
            crate::t!(
                "Reference changes are not available on web — the reference list is read-only."
            )
            .into_owned()
        });
        let path_group_gate: Option<String> = web_readonly.clone().or_else(|| {
            if has_direct {
                None
            } else if self.selected.is_empty() {
                Some(crate::t!("Select a reference first.").into_owned())
            } else {
                Some(
                    crate::t!("Nested references are read-only — edit them in their host drawing.")
                        .into_owned(),
                )
            }
        });
        let change_path = match path_group_gate {
            Some(reason) => toolbar_tip(crate::t!("Change Path").into_owned(), reason),
            None => {
                let relative_gate = if host_saved {
                    None
                } else {
                    Some(
                        crate::t!("XREF  Save the drawing first to resolve relative XREF paths.")
                            .into_owned(),
                    )
                };
                let new_path_gate = if single_direct_anchor {
                    None
                } else {
                    Some(crate::t!("Select a single reference first.").into_owned())
                };
                split_button(
                    crate::t!("Change Path").into_owned(),
                    Some(Message::XrefManagerPathMenu),
                    self.path_open,
                    Message::XrefManagerPathMenu,
                    vec![
                        menu_item(
                            crate::t!("Make Absolute").into_owned(),
                            Some(Message::XrefManagerOp(XrefPaletteOp::Pathtype(
                                Pathtype::Full,
                            ))),
                            None,
                        ),
                        menu_item(
                            crate::t!("Make Relative").into_owned(),
                            Some(Message::XrefManagerOp(XrefPaletteOp::Pathtype(
                                Pathtype::Relative,
                            ))),
                            relative_gate,
                        ),
                        menu_item(
                            crate::t!("Remove Path").into_owned(),
                            Some(Message::XrefManagerOp(XrefPaletteOp::Pathtype(
                                Pathtype::None,
                            ))),
                            None,
                        ),
                        menu_item(
                            crate::t!("Select New Path").into_owned(),
                            Some(Message::XrefPathPick),
                            new_path_gate,
                        ),
                        menu_item(
                            crate::t!("Find and Replace").into_owned(),
                            Some(Message::XrefFindReplacePrompt),
                            None,
                        ),
                    ],
                )
            }
        };
        // Help opens a modal window explaining the manager (and its web
        // shortcomings): the old disabled tip could never be clicked, and
        // an anchored dropdown squeezes the text into the toolbar's width.
        let help = toolbar_btn(crate::t!("Help").into_owned(), Some(Message::XrefHelpOpen));
        let toolbar = kit::toolbar(vec![attach, refresh, change_path, help]);

        // ── File References pane title ──────────────────────────────────
        // List / Tree live on this row, right-aligned.
        let table_title: Element<'_, Message> = kit::band(
            row![
                text(crate::t!("File References"))
                    .size(ui_text::UI)
                    .line_height(crate::ui::style::text::line())
                    .font(kit::SEMIBOLD)
                    .width(Fill),
                list_btn,
                tree_btn,
            ]
            .spacing(km::TOOL_GAP)
            .align_y(iced::Center),
        );
        // Each divider between header cells grabs for a width drag
        // (`XrefColGrab(i)` resizes column `i`); mirrors the Layers
        // Name-column divider.
        let mut header_row = row![].spacing(0).align_y(iced::Center);
        let headers = [
            crate::t!("Reference").into_owned(),
            crate::t!("Status").into_owned(),
            crate::t!("Size").into_owned(),
            crate::t!("Type").into_owned(),
            crate::t!("Date").into_owned(),
            crate::t!("Saved Path").into_owned(),
        ];
        for (i, label) in headers.into_iter().enumerate() {
            header_row = header_row.push(
                kit::one_line(text(label))
                    .size(ui_text::UI)
                    .line_height(crate::ui::style::text::line())
                    .font(kit::SEMIBOLD)
                    .width(Length::Fixed(self.col_widths[i])),
            );
            if i + 1 < self.col_widths.len() {
                let grab = mouse_area(
                    container(
                        container(iced::widget::Space::new().width(km::line()).height(Fill)).style(
                            |theme: &Theme| container::Style {
                                background: Some(Background::Color(chrome::surface(theme))),
                                ..Default::default()
                            },
                        ),
                    )
                    .center_x(Length::Fixed(COL_GUTTER))
                    .height(Length::Fixed(ROW_H)),
                )
                .on_press(Message::XrefColGrab(i))
                .interaction(iced::mouse::Interaction::ResizingHorizontally);
                header_row = header_row.push(grab);
            }
        }
        let col_header: Element<'_, Message> = mouse_area(
            container(header_row.width(Length::Fixed(table_w)))
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(chrome::surface_raised(theme))),
                    ..Default::default()
                })
                .height(Length::Fixed(ROW_H))
                .padding(ROW_PAD)
                .width(Fill),
        )
        .on_move(Message::XrefColMove)
        .on_release(Message::XrefColRelease)
        .into();

        // ── Reference rows ────────────────────────────────────────────────
        let mut rows_col = column![].spacing(0);
        for display in self.display_rows() {
            if display.index == HOST_ROW {
                rows_col = rows_col.push(host_row(
                    display,
                    &self.host_name,
                    &self.host_path,
                    table_w,
                    &self.col_widths,
                ));
                continue;
            }
            let Some(entry) = self.entries.get(display.index) else {
                continue;
            };
            let is_sel = self.selected.contains(&display.index);
            let kids = self.children.get(&entry.key);
            let has_children = kids.is_some_and(|k| !k.is_empty());
            let is_expanded = has_children && self.expanded.contains(&entry.key);
            rows_col = rows_col.push(xref_row(
                display,
                entry,
                is_sel,
                self.tree && has_children,
                is_expanded,
                table_w,
                &self.col_widths,
                self.row_change_path_open,
            ));
        }
        // ── Reference table: header + rows scroll together. A horizontal
        // outer scrollable carries a fixed-width column; the inner vertical
        // scrollable pages rows. (A single Both-direction scrollable renders
        // blank content in this iced rev — verified by screenshot — so the
        // axes stay split across nested scrollables.)
        let table_rows: Element<'_, Message> = if self.entries.is_empty() {
            container(kit::empty_state(crate::t!(
                "XREF  No external references in this drawing."
            )))
            .width(Fill)
            .height(Length::Fixed(self.table_h))
            .into()
        } else if self.tree {
            // Tree view replaces the table: icon + name rows in nesting
            // order, no columns. Single-select still applies.
            let mut tree_col = column![].spacing(0);
            for display in self.display_rows() {
                if display.index == HOST_ROW {
                    tree_col = tree_col.push(tree_host_row(display, &self.host_name));
                    continue;
                }
                let Some(entry) = self.entries.get(display.index) else {
                    continue;
                };
                let is_sel = self.selected.contains(&display.index);
                let kids = self.children.get(&entry.key);
                let has_children = kids.is_some_and(|k| !k.is_empty());
                let is_expanded = has_children && self.expanded.contains(&entry.key);
                tree_col = tree_col.push(tree_row(
                    display,
                    entry,
                    is_sel,
                    has_children,
                    is_expanded,
                    self.row_change_path_open,
                ));
            }
            scrollable(tree_col)
                .style(kit::scroll_style)
                .height(Length::Fixed(self.table_h))
                .into()
        } else {
            scrollable(
                column![
                    col_header,
                    scrollable(rows_col)
                        .style(kit::scroll_style)
                        .height(Length::Fixed(self.table_h)),
                ]
                .width(Length::Fixed(table_w)),
            )
            .direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::new()
                    .width(km::SCROLL_THUMB_W)
                    .scroller_width(km::SCROLL_THUMB_W),
            ))
            .style(kit::scroll_style)
            .height(Length::Fixed(self.table_h))
            .into()
        };
        // Thin external padding + darkest surface around the table, like the
        // properties panel content.
        let table_rows: Element<'_, Message> = container(table_rows)
            .padding(Padding {
                top: km::TOOL_GAP,
                bottom: 0.0,
                left: km::GRID_INSET_L,
                right: km::GRID_INSET_R,
            })
            .width(Fill)
            .into();

        // ── Missing-on-open notice + details pane ─────────────────────────
        let notice: Option<Element<'_, Message>> = if missing > 0 {
            Some(
                container(text(crate::tf!(
                    "{} reference(s) not found — open the reference manager with EXTERNALREFERENCES.",
                    missing
                )))
                .padding(BAND_PAD)
                .width(Fill)
                .into(),
            )
        } else {
            None
        };
        // ── Details / Preview lower pane ────────────────────────────────
        // Toggle buttons mirror the spec's Details/Preview switch. Preview is
        // a placeholder in v1 (no thumbnail pipeline): a grey field, or
        // "Preview not available" — never a broken image control.
        // The tab shown sends nothing: pressing it again keeps it.
        let flip = |shown: bool| {
            if shown {
                Message::Noop
            } else {
                Message::XrefManagerTogglePreview
            }
        };
        let pane_tabs = kit::tabs(
            vec![
                (crate::t!("Details").into_owned(), flip(!self.show_preview)),
                (crate::t!("Preview").into_owned(), flip(self.show_preview)),
            ],
            usize::from(self.show_preview),
        );
        let lower: Element<'_, Message> = if self.show_preview {
            // Single selection → the decoded image, or "Preview not
            // available" (DXF/PDF/unresolvable). Anything else → the spec's
            // solid grey field.
            let single = self.anchor.and_then(|i| {
                (self.selected.len() <= 1)
                    .then(|| self.entries.get(i))
                    .flatten()
            });
            let preview_body: Element<'_, Message> = match single {
                Some(e) => {
                    let cached = e
                        .found_at
                        .as_deref()
                        .and_then(|found| self.previews.get(&(e.key, found.to_string())));
                    match cached {
                        Some(handle) => container(
                            iced::widget::image(handle.clone()).width(Length::Fixed(PREVIEW_W)),
                        )
                        .center_x(Fill)
                        .center_y(Fill)
                        .width(Fill)
                        .height(Length::Fixed(PREVIEW_H))
                        .into(),
                        None => container(
                            column![
                                text(e.name.as_str()).size(crate::ui::style::text::UI),
                                text(crate::t!("Preview not available"))
                                    .size(crate::ui::style::text::UI)
                                    .style(muted_style),
                            ]
                            .spacing(km::TOOL_GAP)
                            .align_x(iced::Center),
                        )
                        .center_x(Fill)
                        .center_y(Fill)
                        .width(Fill)
                        .height(Length::Fixed(PREVIEW_H))
                        .into(),
                    }
                }
                None => container(
                    text("—")
                        .size(crate::ui::style::text::UI)
                        .style(muted_style),
                )
                .center_x(Fill)
                .center_y(Fill)
                .width(Fill)
                .height(Length::Fixed(PREVIEW_H))
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(chrome::surface_raised(theme))),
                    ..Default::default()
                })
                .into(),
            };
            column![pane_tabs, kit::band(preview_body)].into()
        } else {
            let details = details_pane(self.anchor.and_then(|i| self.entries.get(i)), doc);
            column![pane_tabs, kit::band(details)].into()
        };

        // Horizontal split divider between the table and the lower pane:
        // grabs vertically to give the table more (or less) height.
        let split = mouse_area(
            iced::widget::canvas(crate::ui::dock::SplitterGrip { active: false })
                .width(Fill)
                .height(Length::Fixed(km::SPLITTER_H)),
        )
        .on_press(Message::XrefSplitGrab)
        .interaction(iced::mouse::Interaction::ResizingVertically);
        // The toolbar band keeps a tool gap above it; the rest of the top
        // inset puts it as far from the title bar as from the left.
        let top_inset =
            iced::widget::Space::new().height(Length::Fixed(km::TOP_INSET - km::TOOL_GAP));
        let mut content = column![
            title_bar,
            top_inset,
            toolbar,
            table_title,
            table_rows,
            split
        ];
        if let Some(notice) = notice {
            content = content.push(notice);
        }
        content = content.push(lower);
        // Panel-wide move/release tracking for the split drag (mirrors the
        // header divider tracking for columns).
        let content: Element<'_, Message> = mouse_area(content.spacing(0))
            .on_move(Message::XrefSplitMove)
            .on_release(Message::XrefSplitRelease)
            .into();
        // Fixed to the dock width like the block palette: the panel never
        // sizes itself from its (wider) table content, so the dock width
        // setting — and resize — actually takes effect.
        container(content)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::surface(theme))),
                ..Default::default()
            })
            .width(Length::Fixed(width))
            .height(Fill)
            .into()
    }
}

/// `(key, saved_path)` identities for every reference the host drawing holds
/// directly — mirrors the three [`collect_entries`] scans so nested entries
/// (foreign handles) classify correctly even on handle collisions.
fn direct_identities(doc: &Document) -> HashSet<(u64, String)> {
    use document::objects::ObjectType;

    let mut ids: HashSet<(u64, String)> = HashSet::new();
    for br in doc.block_records.iter() {
        if br.flags.is_xref || br.flags.is_xref_overlay {
            ids.insert((br.handle.value(), br.xref_path.clone()));
        }
    }
    for (handle, obj) in doc.objects.iter() {
        match obj {
            ObjectType::ImageDefinition(def) => {
                ids.insert((handle.value(), def.file_name.clone()));
            }
            ObjectType::UnderlayDefinition(def) => {
                ids.insert((handle.value(), def.file_path.clone()));
            }
            ObjectType::ClassObject(object) => {
                if let document::objects::ClassObjectData::PointCloudDefinitionEx(def) =
                    &object.data
                {
                    ids.insert((handle.value(), def.source_filename.clone()));
                }
            }
            _ => {}
        }
    }
    ids
}

/// Claim a `(normalized saved path, entry name)` pair for display; empty
/// paths always pass, exact repeats are skipped (cycle guard). Two entries
/// sharing one file under different names both render.
fn claim_path(seen: &mut HashSet<(String, String)>, saved_path: &str, name: &str) -> bool {
    if saved_path.is_empty() {
        return true;
    }
    seen.insert((normalize_lexical(saved_path), name.to_string()))
}

// ── Text helpers ──────────────────────────────────────────────────────────

pub(crate) fn status_text(status: RefStatus) -> std::borrow::Cow<'static, str> {
    match status {
        RefStatus::Loaded => crate::t!("Loaded"),
        RefStatus::Unloaded => crate::t!("Unloaded"),
        RefStatus::NotFound => crate::t!("Not found"),
        // Unreadable file: the industry palette term is Unresolved.
        RefStatus::Failed => crate::t!("Unresolved"),
        RefStatus::Stale => crate::t!("Stale"),
        RefStatus::Orphaned => crate::t!("Orphaned"),
        RefStatus::Unreferenced => crate::t!("Unreferenced"),
    }
}

/// Status icon for the list view: one distinct glyph + theme color per
/// lifecycle state, drawn from the shared monochrome chrome set so the
/// artwork stays font-independent on the web build.
fn status_icon(status: RefStatus) -> Element<'static, Message> {
    use crate::ui::icons;
    let (bytes, tone) = status_icon_key(status);
    match tone {
        StatusTone::Success => icons::themed_success(bytes, 12.0),
        StatusTone::Secondary => icons::themed_secondary(bytes, 12.0),
        StatusTone::Warning => icons::themed_warning(bytes, 12.0),
        StatusTone::Danger => icons::themed_danger(bytes, 12.0),
    }
}

/// Icon tone: which theme color a status glyph renders in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StatusTone {
    Success,
    Secondary,
    Warning,
    Danger,
}

/// Pure status → (glyph bytes, tone) mapping behind [`status_icon`].
/// Every lifecycle state resolves to a distinct pair.
fn status_icon_key(status: RefStatus) -> (&'static [u8], StatusTone) {
    use crate::ui::icons;
    use StatusTone as T;
    match status {
        RefStatus::Loaded => (icons::CHECK, T::Success),
        RefStatus::Unloaded => (icons::MINUS, T::Secondary),
        RefStatus::Stale => (icons::DIRTY_DOT, T::Warning),
        RefStatus::NotFound => (icons::CLOSE, T::Danger),
        RefStatus::Failed => (icons::DOT, T::Danger),
        RefStatus::Orphaned => (icons::DOT, T::Warning),
        RefStatus::Unreferenced => (icons::DOC, T::Secondary),
    }
}

fn type_text(entry: &ReferenceEntry) -> std::borrow::Cow<'static, str> {
    match entry.kind {
        RefKind::DwgXref => match entry.ref_type {
            RefType::Attach => crate::t!("Attach"),
            RefType::Overlay => crate::t!("Overlay"),
        },
        // Raster images display their file format (spec: type column shows
        // the image format); unknown extensions fall back to Image.
        RefKind::Image => image_format(&entry.saved_path),
        RefKind::Underlay => {
            let ext = std::path::Path::new(&entry.saved_path.replace('\\', "/"))
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_uppercase())
                .unwrap_or_default();
            match ext.as_str() {
                "DWF" | "DWFX" => crate::t!("DWF"),
                "DGN" => crate::t!("DGN"),
                _ => crate::t!("PDF"),
            }
        }
        // The file's own kind: RCP (project) or RCS (scan).
        RefKind::PointCloud => {
            let ext = std::path::Path::new(&entry.saved_path.replace('\\', "/"))
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_uppercase())
                .unwrap_or_default();
            if ext == "RCS" {
                "RCS".into()
            } else {
                "RCP".into()
            }
        }
    }
}

/// Uppercase file extension of an image path (`plan.BMP` → `BMP`).
/// Extensionless or overlong suffixes fall back to `Image`.
fn image_format(saved_path: &str) -> std::borrow::Cow<'static, str> {
    let file = saved_path.rsplit(['/', '\\']).next().unwrap_or("");
    let ext = file
        .rsplit('.')
        .next()
        .filter(|_| file.contains('.'))
        .unwrap_or("");
    if ext.is_empty() || ext.len() > 5 {
        crate::t!("Image")
    } else {
        std::borrow::Cow::Owned(ext.to_ascii_uppercase())
    }
}

fn format_size(size_bytes: Option<u64>) -> String {
    match size_bytes {
        None => "—".to_string(),
        Some(b) if b < 1024 => format!("{b} B"),
        Some(b) if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        Some(b) if b < 1024 * 1024 * 1024 => format!("{:.1} MB", b as f64 / 1_048_576.0),
        Some(b) => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
    }
}

/// Calendar date (`YYYY-MM-DD`) for a file modification time. Hand-rolled
/// civil-from-days so no date crate joins the dependency tree.
fn format_date(modified: Option<SystemTime>) -> String {
    let Some(t) = modified else {
        return "—".to_string();
    };
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy as i64 - (153 * mp + 2) as i64 / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as i64;
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

// ── Widget helpers (layers.rs conventions) ────────────────────────────────

fn toolbar_btn(label: String, msg: Option<Message>) -> Element<'static, Message> {
    kit::text_button(label, msg, false)
}

/// Disabled toolbar affordance with an explanatory tooltip (gated
/// operations, nested-selection blocks, or web-gated mutations).
fn toolbar_tip(label: String, tip: String) -> Element<'static, Message> {
    kit::tooltip(
        kit::text_button(label, None, false),
        tip,
        kit::TipPosition::Bottom,
    )
}

/// Split button: a default action plus a caret that opens the rest in the
/// kit menu (spec toolbar: Attach ▾, Refresh ▾, Change Path ▾).
///
/// `main_msg` runs the default action; `toggle` flips this menu (closing the
/// others is handled at the message site); `items` are the menu's items.
/// Dismissal (Escape / outside click) funnels to
/// [`Message::XrefManagerDismissMenus`].
fn split_button(
    main_label: String,
    main_msg: Option<Message>,
    menu_open: bool,
    toggle: Message,
    items: Vec<Item>,
) -> Element<'static, Message> {
    let main = kit::text_button(main_label, main_msg, false);
    let caret = button(container(crate::ui::icons::dropdown_arrow()).center(Length::Fill))
        .on_press(toggle)
        .style(kit::flat_button_style(menu_open))
        .width(Length::Fixed(km::BAR_BUTTON))
        .height(Length::Fixed(km::TOOL_H))
        .padding(0);
    let head = row![main, caret].spacing(km::LINE).align_y(iced::Center);
    let width = menu::width(&items);
    iced_aw::DropDown::new(head, menu::view(items), menu_open)
        .width(Length::Fixed(width))
        .alignment(iced_aw::drop_down::Alignment::Bottom)
        .offset(km::LINE * 2.0)
        .on_dismiss(Message::XrefManagerDismissMenus)
        .into()
}

/// One dropdown-menu item: pressable when the option can execute,
/// otherwise greyed with the reason as its tooltip.
fn menu_item(label: String, msg: Option<Message>, gate: Option<String>) -> Item {
    match (msg, gate) {
        (Some(m), None) => Item::new(label, m),
        (_, reason) => Item::disabled(label, reason.unwrap_or_default()),
    }
}

/// Per-row right-click menu, the kit menu: universal tools, then path
/// tools. No Bind, no Attach/Overlay — those are DWG-only and hidden
/// entirely. Change Path Type opens its list beside the menu on hover.
/// `nested` greys everything but Open: a nested reference is changed in its
/// host drawing. `current` is the path type the reference already has, which
/// its flyout greys.
fn row_menu_for(
    id: XrefRowId,
    change_path_open: bool,
    nested: bool,
    current: Pathtype,
) -> Element<'static, Message> {
    let op = |label: &str, op: XrefPaletteOp| {
        let enabled = !nested || matches!(op, XrefPaletteOp::Open);
        Item::new(label, Message::XrefRowOp(id.clone(), op)).enabled(enabled)
    };
    let pathtype = |label: &str, pt: Pathtype| {
        Item::new(
            label,
            Message::XrefRowOp(id.clone(), XrefPaletteOp::Pathtype(pt)),
        )
        .enabled(pt != current)
    };
    let items = vec![
        op(&crate::t!("Open"), XrefPaletteOp::Open),
        op(&crate::t!("Attach..."), XrefPaletteOp::Attach),
        op(&crate::t!("Unload"), XrefPaletteOp::Unload),
        op(&crate::t!("Reload"), XrefPaletteOp::Reload),
        op(&crate::t!("Detach"), XrefPaletteOp::Detach),
        Item::Separator,
        Item::submenu(
            crate::t!("Change Path Type"),
            Message::XrefRowChangePathEnter,
            vec![
                pathtype(&crate::t!("Make Absolute"), Pathtype::Full),
                pathtype(&crate::t!("Make Relative"), Pathtype::Relative),
                pathtype(&crate::t!("Remove Path"), Pathtype::None),
            ],
        )
        .open(change_path_open)
        .enabled(!nested),
        Item::Separator,
        Item::new(
            crate::t!("Select New Path"),
            Message::XrefRowPathPick(id.clone()),
        )
        .enabled(!nested),
        Item::new(
            crate::t!("Find and Replace..."),
            Message::XrefRowFindReplacePrompt(id.clone()),
        )
        .enabled(!nested),
    ];
    menu::with_submenus(items, Message::XrefRowChangePathLeave)
}

fn xref_row<'a>(
    display: DisplayRow,
    entry: &'a ReferenceEntry,
    is_selected: bool,
    show_expand: bool,
    is_expanded: bool,
    table_w: f32,
    cw: &[f32; 6],
    change_path_open: bool,
) -> Element<'a, Message> {
    let mut name = entry.name.clone();
    if display.is_nested {
        name.push_str(&crate::t!(" (nested — not rendered)"));
    }
    let mut name_cell = row![].align_y(iced::Center);
    if display.depth > 0 {
        name_cell = name_cell
            .push(iced::widget::Space::new().width(Length::Fixed(INDENT_W * display.depth as f32)));
    }
    // Tree parents own the only interactive cell besides selection: the
    // expand/collapse arrow. Nested rows never show one.
    if show_expand {
        let key = entry.key;
        let arrow = button(
            container(if is_expanded {
                crate::ui::icons::themed_arrow_down(km::CARET)
            } else {
                crate::ui::icons::themed_arrow_right(km::CARET)
            })
            .center(Length::Fill),
        )
        .on_press(Message::XrefManagerToggleExpand(key))
        .style(kit::cell_button_style)
        .width(Length::Fixed(km::INDENT))
        .height(Length::Fixed(ROW_H))
        .padding(0);
        name_cell = name_cell.push(arrow);
    }
    let name_text: Element<'_, Message> = kit::one_line(text(name))
        .size(ui_text::UI)
        .line_height(crate::ui::style::text::line())
        .width(Fill)
        .into();
    name_cell = name_cell.push(name_text);
    let gutter = || iced::widget::Space::new().width(Length::Fixed(COL_GUTTER));
    let status_cell: Element<'_, Message> = row![
        container(status_icon(entry.status)).align_y(iced::Center),
        kit::one_line(text(status_text(entry.status)))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Fill),
    ]
    .spacing(km::TOOL_GAP)
    .align_y(iced::Center)
    .width(Length::Fixed(cw[1]))
    .into();
    let content = row![
        name_cell.width(Length::Fixed(cw[0])),
        gutter(),
        status_cell,
        gutter(),
        kit::one_line(text(format_size(entry.size_bytes)))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[2])),
        gutter(),
        kit::one_line(text(type_text(entry)))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[3])),
        gutter(),
        kit::one_line(text(format_date(entry.modified)))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[4])),
        gutter(),
        kit::one_line(text(entry.saved_path.clone()))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[5])),
    ]
    .spacing(0)
    .width(Length::Fixed(table_w))
    .align_y(iced::Center);

    // #49: messages carry stable identity.
    let row_id = XrefRowId::of(entry.key, &entry.saved_path);
    let row = mouse_area(
        container(content)
            .style(row_style(is_selected))
            .padding(ROW_PAD)
            .height(Length::Fixed(ROW_H))
            .width(Fill),
    )
    .on_press(Message::XrefManagerSelect(row_id.clone()));
    let (nested, current) = (
        display.is_nested,
        crate::io::xref_model::pathtype_of(&entry.saved_path),
    );
    let cm = iced_aw::ContextMenu::new(row, {
        let row_id = row_id.clone();
        move || row_menu_for(row_id.clone(), change_path_open, nested, current)
    });
    kit::press_first(
        cm,
        iced::mouse::Button::Right,
        Message::XrefRowRightClick(row_id),
    )
}

/// One tree-mode row: indent + optional expand arrow + file icon + name.
/// Same selection highlight and right-click menu as table rows.
fn tree_row(
    display: DisplayRow,
    entry: &ReferenceEntry,
    is_selected: bool,
    show_expand: bool,
    is_expanded: bool,
    change_path_open: bool,
) -> Element<'_, Message> {
    let mut cells = row![].align_y(iced::Center);
    cells = cells.push(tree_indent(display.depth));
    if show_expand {
        let key = entry.key;
        let arrow = button(
            container(if is_expanded {
                crate::ui::icons::themed_arrow_down(km::CARET)
            } else {
                crate::ui::icons::themed_arrow_right(km::CARET)
            })
            .center(Length::Fill),
        )
        .on_press(Message::XrefManagerToggleExpand(key))
        .style(kit::cell_button_style)
        .width(Length::Fixed(km::INDENT))
        .height(Length::Fixed(ROW_H))
        .padding(0);
        cells = cells.push(arrow);
    }
    cells = cells.push(
        container(crate::ui::icons::themed_secondary(
            crate::ui::icons::DOC,
            km::ROW_ICON,
        ))
        .align_y(iced::Center),
    );
    cells = cells.push(
        kit::one_line(text(entry.name.clone()))
            .size(TREE_FONT_SZ)
            .line_height(crate::ui::style::text::line())
            .width(Fill),
    );
    // #49: messages carry stable identity.
    let row_id = XrefRowId::of(entry.key, &entry.saved_path);
    let row = mouse_area(
        container(cells.spacing(km::TOOL_GAP).width(Fill))
            .style(row_style(is_selected))
            .padding(ROW_PAD)
            .height(Length::Fixed(ROW_H))
            .width(Fill),
    )
    .on_press(Message::XrefManagerSelect(row_id.clone()));
    let (nested, current) = (
        display.is_nested,
        crate::io::xref_model::pathtype_of(&entry.saved_path),
    );
    let cm = iced_aw::ContextMenu::new(row, {
        let row_id = row_id.clone();
        move || row_menu_for(row_id.clone(), change_path_open, nested, current)
    });
    kit::press_first(
        cm,
        iced::mouse::Button::Right,
        Message::XrefRowRightClick(row_id),
    )
}

/// Tree-mode host root: home icon + name with the current-drawing marker.
/// Never selectable, like its table counterpart.
fn tree_host_row(display: DisplayRow, host_name: &str) -> Element<'_, Message> {
    let mut name = if host_name.is_empty() {
        crate::t!("Untitled").into_owned()
    } else {
        host_name.to_string()
    };
    name.push('*');
    let mut cells = row![].align_y(iced::Center);
    cells = cells.push(tree_indent(display.depth));
    cells =
        cells.push(container(crate::ui::icons::themed_home(km::ROW_ICON)).align_y(iced::Center));
    cells = cells.push(
        kit::one_line(text(name))
            .size(TREE_FONT_SZ)
            .line_height(crate::ui::style::text::line())
            .width(Fill),
    );
    container(cells.spacing(km::TOOL_GAP).width(Fill))
        .style(row_style(false))
        .padding(ROW_PAD)
        .height(Length::Fixed(ROW_H))
        .width(Fill)
        .into()
}
fn host_row<'a>(
    display: DisplayRow,
    host_name: &'a str,
    host_path: &'a str,
    table_w: f32,
    cw: &[f32; 6],
) -> Element<'a, Message> {
    let name = if host_name.is_empty() {
        crate::t!("Untitled").into_owned()
    } else {
        host_name.to_string()
    };
    let saved = if host_path.is_empty() {
        "—"
    } else {
        host_path
    };
    let mut name_cell = row![].align_y(iced::Center);
    if display.depth > 0 {
        name_cell = name_cell
            .push(iced::widget::Space::new().width(Length::Fixed(INDENT_W * display.depth as f32)));
    }
    name_cell = name_cell.push({
        let name_text: Element<'_, Message> = kit::one_line(text(name))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Fill)
            .into();
        name_text
    });
    let gutter = || iced::widget::Space::new().width(Length::Fixed(COL_GUTTER));
    let content = row![
        name_cell.width(Length::Fixed(cw[0])),
        gutter(),
        kit::one_line(text(crate::t!("Opened")))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[1])),
        gutter(),
        kit::one_line(text("—"))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[2])),
        gutter(),
        kit::one_line(text(crate::t!("Current")))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[3])),
        gutter(),
        kit::one_line(text("—"))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[4])),
        gutter(),
        kit::one_line(text(saved))
            .size(ui_text::UI)
            .line_height(crate::ui::style::text::line())
            .width(Length::Fixed(cw[5])),
    ]
    .spacing(0)
    .width(Length::Fixed(table_w))
    .align_y(iced::Center);
    container(content)
        .style(row_style(false))
        .padding(ROW_PAD)
        .height(Length::Fixed(ROW_H))
        .width(Fill)
        .into()
}

fn details_pane<'a>(entry: Option<&'a ReferenceEntry>, doc: &'a Document) -> Element<'a, Message> {
    let inner: Element<'_, Message> = match entry {
        None => kit::value_text(crate::t!("Select a reference to inspect its details"), true),
        Some(e) => {
            let found = e.found_at.as_deref().unwrap_or("—");
            let saved = if e.saved_path.is_empty() {
                "—"
            } else {
                e.saved_path.as_str()
            };
            let size = format_size(e.size_bytes);
            let date = format_date(e.modified);
            let mut rows = column![
                detail_row(crate::t!("Reference"), e.name.as_str()),
                detail_row(crate::t!("Status"), status_text(e.status)),
                detail_row(crate::t!("Size"), size),
                detail_row(crate::t!("Type"), type_text(e)),
                detail_row(crate::t!("Date"), date),
                detail_row(crate::t!("Found At"), found),
                detail_row(crate::t!("Saved Path"), saved),
            ];
            // Image-specific properties (read-only). The file format exposes
            // pixel dimensions and resolution units; color system / color
            // depth are not stored by the format and are omitted deliberately.
            if e.kind == RefKind::Image {
                if let Some(def) = find_image_def(doc, e.key) {
                    rows = rows
                        .push(detail_row(
                            crate::t!("Pixel Width"),
                            format!("{}", def.size_in_pixels.0),
                        ))
                        .push(detail_row(
                            crate::t!("Pixel Height"),
                            format!("{}", def.size_in_pixels.1),
                        ))
                        .push(detail_row(
                            crate::t!("Resolution Unit"),
                            format!("{:?}", def.resolution_unit),
                        ));
                }
                if let Some((w, h)) = find_image_display_size(doc, e.key) {
                    rows = rows
                        .push(detail_row(crate::t!("Display Width"), format!("{:.2}", w)))
                        .push(detail_row(crate::t!("Display Height"), format!("{:.2}", h)));
                }
            }
            rows.into()
        }
    };
    container(inner).width(Fill).into()
}

/// Decode one entry's preview image (`None` = placeholder territory).
///
/// Sources mirror what the formats actually carry: DWG files embed a preview
/// (read header-only via `dwg_thumbnailer`, no full parse — DXF has none, so
/// it resolves to no preview); raster images decode through the `image`
/// crate and downscale; PDFs have no rasterizer in this build. Only
/// Loaded/Stale rows resolve — Unloaded/NotFound show the placeholder by
/// design. Web builds skip filesystem decoding entirely.
fn reference_preview(entry: &ReferenceEntry) -> Option<image::RgbaImage> {
    if IS_WASM {
        return None;
    }
    // Any entry whose file resolves decodes — including Unloaded and
    // Unreferenced rows — so previews don't depend on load state. Missing
    // files fail the decode below and fall back to the placeholder.
    let found = entry.found_at.as_deref().filter(|s| !s.is_empty())?;
    match entry.kind {
        RefKind::DwgXref => {
            let ext = found
                .rsplit(['/', '\\'])
                .next()
                .and_then(|f| f.rsplit('.').next())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext != "dwg" {
                return None;
            }
            dwg_thumbnailer::extract(std::path::Path::new(found), PREVIEW_MAX)
        }
        RefKind::Image => {
            let img = image::open(found).ok()?;
            Some(img.thumbnail(PREVIEW_MAX, PREVIEW_MAX).to_rgba8())
        }
        RefKind::Underlay | RefKind::PointCloud => None,
    }
}

/// A preview for the app to decode on a worker: the panel's refresh count
/// when it was asked for, the cache key, and the entry.
#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub generation: u64,
    pub key: (u64, String),
    pub entry: ReferenceEntry,
}

/// Decodes an entry's preview ([`read_preview`]).
pub type PreviewReader = fn(&ReferenceEntry) -> Option<iced::widget::image::Handle>;

/// Decode `entry`'s preview into an image for the panel (run on a worker
/// thread).
pub fn read_preview(entry: &ReferenceEntry) -> Option<iced::widget::image::Handle> {
    let img = reference_preview(entry)?;
    let (w, h) = (img.width(), img.height());
    Some(iced::widget::image::Handle::from_rgba(w, h, img.into_raw()))
}

/// Image definition backing a [`RefKind::Image`] entry, looked up by the
/// entry key (definition-object handle) for the Details pane extras.
fn find_image_def(doc: &Document, key: u64) -> Option<&document::objects::ImageDefinition> {
    doc.objects
        .get(&document::types::Handle::from(key))
        .and_then(|o| match o {
            document::objects::ObjectType::ImageDefinition(def) => Some(def),
            _ => None,
        })
}

fn find_image_display_size(doc: &Document, key: u64) -> Option<(f64, f64)> {
    let handle = document::types::Handle::from(key);
    for entity in doc.entities() {
        if let document::EntityType::RasterImage(img) = entity {
            if img.definition_handle == Some(handle) {
                let w = (img.u_vector.x.powi(2) + img.u_vector.y.powi(2) + img.u_vector.z.powi(2))
                    .sqrt();
                let h = (img.v_vector.x.powi(2) + img.v_vector.y.powi(2) + img.v_vector.z.powi(2))
                    .sqrt();
                return Some((w, h));
            }
        }
    }
    None
}

fn detail_row<'a>(
    label: impl Into<std::borrow::Cow<'a, str>>,
    value: impl Into<std::borrow::Cow<'a, str>>,
) -> Element<'a, Message> {
    let label: std::borrow::Cow<'a, str> = label.into();
    let value: std::borrow::Cow<'a, str> = value.into();
    kit::row_view(
        kit::name_text(label.into_owned()),
        kit::value_cell(value.into_owned(), false),
        false,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::xref_model::RefKind;

    #[test]
    fn size_formats_without_deps() {
        assert_eq!(format_size(None), "—");
        assert_eq!(format_size(Some(0)), "0 B");
        assert_eq!(format_size(Some(900)), "900 B");
        assert_eq!(format_size(Some(2048)), "2.0 KB");
        assert_eq!(format_size(Some(5 * 1_048_576)), "5.0 MB");
    }

    #[test]
    fn date_formats_known_days() {
        assert_eq!(format_date(None), "—");
        let day = |days: u64| {
            format_date(Some(
                UNIX_EPOCH + std::time::Duration::from_secs(days * 86_400),
            ))
        };
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(10957), "2000-01-01");
        assert_eq!(day(19723), "2024-01-01");
    }

    fn entry(key: u64, name: &str, saved: &str) -> ReferenceEntry {
        let mut e = ReferenceEntry::new(key, name, RefKind::DwgXref);
        e.saved_path = saved.to_string();
        e
    }

    #[test]
    fn select_toggles_anchor_and_set() {
        let mut panel = XrefManagerPanel::default();
        panel.entries = vec![entry(1, "A", "a.dwg"), entry(2, "B", "b.dwg")];
        panel.toggle_select(0);
        panel.toggle_select(1);
        assert!(panel.selected.contains(&0) && panel.selected.contains(&1));
        assert_eq!(panel.anchor, Some(1));
        panel.toggle_select(1);
        assert!(!panel.selected.contains(&1));
        assert_eq!(panel.anchor, Some(0));
        panel.toggle_select(9); // out of range: no-op
        assert_eq!(panel.selected.len(), 1);
    }

    #[test]
    fn tree_skips_repeat_paths() {
        let mut panel = XrefManagerPanel::default();
        panel.entries = vec![
            entry(1, "A", "a.dwg"),
            entry(2, "B", "b.dwg"),
            entry(3, "B2", "b.dwg"),
        ];
        panel.tree = true;
        // Host row leads; same file under two names renders twice; only
        // exact (path, name) repeats collapse.
        let rows = panel.display_rows();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].index, HOST_ROW);
        assert!(rows[1..].iter().all(|r| r.depth >= 1));
        panel.entries.push(entry(4, "B", "b.dwg"));
        assert_eq!(panel.display_rows().len(), 4);
        // Mark B2 nested under A: A claims (a.dwg, A), B2-as-child claims
        // (b.dwg, B2); the B root still renders as (b.dwg, B).
        panel.nested.insert(2);
        panel.children.insert(1, vec![2]);
        panel.expanded.insert(1);
        let rows = panel.display_rows();
        assert_eq!(rows.len(), 4); // host, A, B2-as-child-of-A, B
        assert!(rows.iter().any(|r| r.index == 2 && r.is_nested));
        // A second parent claiming the same child shows nothing twice.
        panel.entries.push(entry(5, "C", "c.dwg"));
        panel.children.insert(5, vec![2]);
        panel.expanded.insert(5);
        let count = panel.display_rows().iter().filter(|r| r.index == 2).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn host_row_leads_and_ignores_selection() {
        let mut panel = XrefManagerPanel::default();
        panel.entries = vec![entry(1, "A", "a.dwg")];
        panel.host_name = "host".to_string();
        for tree in [false, true] {
            panel.tree = tree;
            let rows = panel.display_rows();
            assert_eq!(rows[0].index, HOST_ROW);
            assert_eq!(rows[0].depth, 0);
        }
        panel.toggle_select(HOST_ROW); // no-op, never selected
        assert!(panel.selected.is_empty());
        assert_eq!(panel.anchor, None);
    }

    #[test]
    fn tree_selects_single_reference() {
        let mut panel = XrefManagerPanel::default();
        panel.entries = vec![entry(1, "A", "a.dwg"), entry(2, "B", "b.dwg")];
        panel.tree = true;
        panel.toggle_select(0);
        panel.toggle_select(1);
        assert_eq!(panel.selected, HashSet::from([1]));
        assert_eq!(panel.anchor, Some(1));
        panel.tree = false;
        panel.toggle_select(0);
        assert_eq!(panel.selected, HashSet::from([0, 1]));
    }

    #[test]
    fn actionable_selection_skips_nested() {
        let mut panel = XrefManagerPanel::default();
        panel.entries = vec![entry(1, "A", "a.dwg"), entry(2, "B", "b.dwg")];
        panel.nested.insert(1);
        panel.toggle_select(0);
        panel.toggle_select(1);
        assert!(panel.selection_has_nested());
        // Only the direct row is actionable; the nested row stays read-only.
        assert_eq!(panel.actionable_selection(), vec![0]);
        panel.toggle_select(1);
        assert!(!panel.selection_has_nested());
    }

    #[test]
    fn column_drag_clamps_and_ignores_unknown() {
        let mut panel = XrefManagerPanel::default();
        assert_eq!(panel.col_widths, super::DEFAULT_COL_WIDTHS);
        panel.drag_col_by(0, 40.0);
        assert_eq!(panel.col_widths[0], super::DEFAULT_COL_WIDTHS[0] + 40.0);
        panel.drag_col_by(1, -1000.0);
        assert_eq!(panel.col_widths[1], super::COL_MIN_W);
        panel.drag_col_by(2, 10000.0);
        assert_eq!(panel.col_widths[2], super::COL_MAX_W);
        panel.drag_col_by(99, 10.0); // no-op, no panic
        assert_eq!(
            panel.table_content_width(),
            panel.col_widths.iter().sum::<f32>() + super::COL_GUTTER * 5.0
        );
    }

    #[test]
    fn table_split_drag_clamps() {
        let mut panel = XrefManagerPanel::default();
        assert_eq!(panel.table_h, super::TABLE_H);
        panel.drag_table_by(60.0);
        assert_eq!(panel.table_h, super::TABLE_H + 60.0);
        panel.drag_table_by(-10000.0);
        assert_eq!(panel.table_h, super::TABLE_MIN_H);
    }

    #[test]
    fn status_icons_are_unique_per_status() {
        use super::{status_icon_key, RefStatus};
        let all = [
            RefStatus::Loaded,
            RefStatus::Unloaded,
            RefStatus::NotFound,
            RefStatus::Failed,
            RefStatus::Stale,
            RefStatus::Orphaned,
            RefStatus::Unreferenced,
        ];
        let mut seen = std::collections::HashSet::new();
        for status in all {
            let (bytes, tone) = status_icon_key(status);
            assert!(
                seen.insert((bytes.as_ptr(), tone)),
                "status {status:?} shares its icon with another state"
            );
        }
        assert_eq!(seen.len(), all.len());
    }

    #[test]
    fn image_format_reads_extension() {
        assert_eq!(image_format("C:/exa/plan.BMP"), "BMP");
        assert_eq!(image_format("a/b/c.png"), "PNG");
        assert_eq!(image_format("noext"), "Image");
        assert_eq!(image_format("toolong.abcdef"), "Image");
        assert_eq!(image_format(""), "Image");
    }

    #[test]
    fn click_select_single_then_range() {
        use super::SelectExtend;
        use super::XrefRowId;
        let id = |key: u64, saved: &str| XrefRowId::of(key, saved);
        let mut panel = XrefManagerPanel::default();
        panel.entries = vec![
            entry(1, "A", "a.dwg"),
            entry(2, "B", "b.dwg"),
            entry(3, "C", "c.dwg"),
        ];
        // Plain click selects exactly one row.
        panel.click_select(&id(1, "a.dwg"), SelectExtend::Single);
        panel.click_select(&id(3, "c.dwg"), SelectExtend::Single);
        assert_eq!(panel.selected, HashSet::from([2]));
        assert_eq!(panel.anchor, Some(2));
        // Shift-click extends a contiguous range from the anchor.
        panel.click_select(&id(3, "c.dwg"), SelectExtend::Single);
        panel.click_select(&id(1, "a.dwg"), SelectExtend::Range);
        assert_eq!(panel.selected, HashSet::from([0, 1, 2]));
        // Repeated extends keep the original anchor — rows are never lost.
        panel.entries.push(entry(4, "D", "d.dwg"));
        panel.entries.push(entry(5, "E", "e.dwg"));
        panel.click_select(&id(5, "e.dwg"), SelectExtend::Range);
        assert_eq!(panel.selected, HashSet::from([2, 3, 4]));
        assert_eq!(panel.anchor, Some(2));
        // Ctrl-click toggles one row.
        panel.click_select(&id(4, "d.dwg"), SelectExtend::Toggle);
        assert_eq!(panel.selected, HashSet::from([2, 4]));
        // Range with no anchor starts at the clicked row.
        let mut fresh = XrefManagerPanel::default();
        fresh.entries = vec![entry(1, "A", "a.dwg"), entry(2, "B", "b.dwg")];
        fresh.click_select(&id(2, "b.dwg"), SelectExtend::Range);
        assert_eq!(fresh.selected, HashSet::from([1]));
    }

    #[test]
    fn expand_collapses_by_default() {
        let mut panel = XrefManagerPanel::default();
        panel.entries = vec![entry(1, "A", "a.dwg"), entry(2, "B", "b.dwg")];
        panel.nested.insert(1);
        panel.children.insert(1, vec![1]);
        panel.tree = true;
        assert!(!panel.display_rows().iter().any(|r| r.is_nested));
        panel.toggle_expand(1);
        assert!(panel.display_rows().iter().any(|r| r.is_nested));
        panel.toggle_expand(1);
        assert!(!panel.display_rows().iter().any(|r| r.is_nested));
    }

    fn preview_doc2(dir: &std::path::Path) -> document::Document {
        // Two referenced images → genuine multi-select.
        use document::objects::{ImageDefinition, ObjectType};
        let mut doc = document::Document::new();
        for (i, name) in ["a.png", "b.png"].iter().enumerate() {
            let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]));
            img.save(dir.join(name)).unwrap();
            let path = dir.join(name).to_string_lossy().into_owned();
            let h = doc.allocate_handle();
            let mut def = ImageDefinition::with_dimensions(path.clone(), 8, 8);
            def.handle = h;
            doc.objects.insert(h, ObjectType::ImageDefinition(def));
            let mut ent = document::entities::RasterImage::new(
                &path,
                document::types::Vector3::ZERO,
                8.0 + i as f64,
                8.0,
            );
            ent.definition_handle = Some(h);
            doc.add_entity(document::EntityType::RasterImage(ent))
                .unwrap();
        }
        doc
    }

    fn preview_doc(dir: &std::path::Path, name: &str) -> document::Document {
        // Referenced 8x8 PNG under `dir`; the entry resolves Loaded.
        use document::objects::{ImageDefinition, ObjectType};
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]));
        img.save(dir.join(name)).unwrap();
        let img_path = dir.join(name).to_string_lossy().into_owned();
        let mut doc = document::Document::new();
        let h = doc.allocate_handle();
        let mut def = ImageDefinition::with_dimensions(img_path.clone(), 8, 8);
        def.handle = h;
        doc.objects.insert(h, ObjectType::ImageDefinition(def));
        let mut ent = document::entities::RasterImage::new(
            &img_path,
            document::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        ent.definition_handle = Some(h);
        doc.add_entity(document::EntityType::RasterImage(ent))
            .unwrap();
        doc
    }

    #[test]
    fn preview_decodes_for_single_selection() {
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_preview_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = preview_doc(&dir, "img.png");
        let mut panel = XrefManagerPanel::default();
        let empty = std::collections::HashSet::new();
        let no_prev = std::collections::HashMap::new();
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        panel.read_preview_now();
        assert!(panel.previews.is_empty(), "nothing selected → no decode");
        panel.toggle_select(0);
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        panel.read_preview_now();
        assert_eq!(panel.previews.len(), 1, "single anchor decodes once");
        let (handle, _) = panel.previews.iter().next().unwrap();
        assert!(panel.entries.iter().any(|e| e.key == handle.0));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A selection asks for its preview once and decodes nothing itself; a
    /// decode that ends after a refresh is dropped.
    #[test]
    fn a_selection_asks_for_its_preview_and_a_stale_one_is_dropped() {
        let dir = std::env::temp_dir().join(format!("ocs_xref_preview_ask_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = preview_doc(&dir, "img.png");
        let mut panel = XrefManagerPanel::default();
        let empty = std::collections::HashSet::new();
        let no_prev = std::collections::HashMap::new();
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        panel.toggle_select(0);
        assert!(panel.previews.is_empty(), "no decode during the selection");
        let request = panel.take_preview_request().expect("a request");
        let id = XrefRowId::of(panel.entries[0].key, &panel.entries[0].saved_path);
        panel.click_select(&id, SelectExtend::Single);
        assert!(
            panel.take_preview_request().is_none(),
            "asked once while it decodes"
        );
        let preview = read_preview(&request.entry);
        assert!(preview.is_some());
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        panel.preview_read(request.generation, request.key, preview);
        assert!(
            panel.previews.is_empty(),
            "a decode from before the refresh"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_skipped_for_multi_selection() {
        // Spec: the preview pane shows an image for a single selected
        // reference only — multi-select decodes nothing (grey field).
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_preview_multi_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = preview_doc2(&dir);
        let mut panel = XrefManagerPanel::default();
        let empty = std::collections::HashSet::new();
        let no_prev = std::collections::HashMap::new();
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        // One image row exists; duplicate it as a second row to multi-select.
        panel.entries.push(panel.entries[0].clone());
        panel.toggle_select(0);
        panel.toggle_select(1);
        assert_eq!(panel.selected.len(), 2);
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        panel.read_preview_now();
        assert!(
            panel.previews.is_empty(),
            "multi-select → grey field, no decode"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_missing_without_embedded_data() {
        // A DWG reference whose file has no embedded preview resolves to no
        // preview (placeholder path), never an error.
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_preview_dwg_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plan.dwg"), b"not-a-real-dwg").unwrap();
        let mut doc = document::Document::new();
        let mut br = document::tables::BlockRecord::new("PLAN");
        br.flags.is_xref = true;
        br.xref_path = dir.join("plan.dwg").to_string_lossy().into_owned();
        br.handle = doc.allocate_handle();
        doc.block_records.add(br).unwrap();
        let mut panel = XrefManagerPanel::default();
        let empty = std::collections::HashSet::new();
        let no_prev = std::collections::HashMap::new();
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        panel.toggle_select(0);
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        panel.read_preview_now();
        assert!(
            panel.previews.is_empty(),
            "undecodable file → placeholder, no cache entry"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_decodes_unloaded_entry_with_file() {
        // Previews key off file resolvability, not load state: an Unloaded
        // image whose file exists still decodes.
        use crate::io::xref_model::UnloadKey;
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_preview_unloaded_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = preview_doc(&dir, "img.png");
        let key = doc
            .objects
            .iter()
            .find_map(|(h, o)| match o {
                document::objects::ObjectType::ImageDefinition(_) => Some(h.value()),
                _ => None,
            })
            .unwrap();
        let mut unloaded = std::collections::HashSet::new();
        unloaded.insert(UnloadKey::Direct(key));
        let no_prev = std::collections::HashMap::new();
        let mut panel = XrefManagerPanel::default();
        panel.refresh(&doc, &dir, &unloaded, &no_prev, "host", "");
        panel.toggle_select(0);
        panel.refresh(&doc, &dir, &unloaded, &no_prev, "host", "");
        panel.read_preview_now();
        assert_eq!(panel.previews.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Headless click-delivery tests for the row context menu ──────────
    //
    // These drive the REAL iced runtime through the `iced_test` headless
    // simulator — including the `iced_aw::ContextMenu` overlay — and prove
    // that clicks on menu rows publish their messages.

    type MenuSimulator = iced_test::simulator::Simulator<'static, Message>;

    /// Builds the shipped row menu inside a real `ContextMenu` overlay and
    /// opens it with a synthetic right-click on the underlay, mirroring a
    /// user right-clicking a reference row.
    fn open_row_menu(change_path_open: bool) -> MenuSimulator {
        let row_id = XrefRowId::of(0, "");
        let mut ui = iced_test::simulator(iced_aw::ContextMenu::new(text("anchor"), move || {
            row_menu_for(row_id.clone(), change_path_open, false, Pathtype::Relative)
        }));
        let anchor = ui.find("anchor").expect("anchor underlay present");
        let center = anchor.visible_bounds().expect("anchor visible").center();
        ui.point_at(center);
        ui.simulate([iced_core::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Right,
        ))]);
        ui
    }

    fn published_ops(ui: MenuSimulator) -> Vec<(XrefRowId, XrefPaletteOp)> {
        let mut ops = Vec::new();
        for message in ui.into_messages() {
            if let Message::XrefRowOp(id, op) = message {
                ops.push((id, op));
            }
        }
        ops
    }

    #[test]
    fn context_menu_row_click_delivers_op() {
        let unload = crate::t!("Unload").into_owned();
        let mut ui = open_row_menu(false);
        ui.click(unload.as_str())
            .expect("Unload row is clickable inside the overlay");
        assert!(
            published_ops(ui).contains(&(XrefRowId::of(0, ""), XrefPaletteOp::Unload)),
            "clicking Unload must publish XrefRowOp(row, Unload)"
        );
    }

    #[test]
    fn hover_change_path_row_requests_flyout() {
        // The "Change Path Type >" row reveals the second menu on hover.
        let label = crate::t!("Change Path Type").into_owned();
        let mut ui = open_row_menu(false);
        let target = ui
            .find(label.as_str())
            .expect("Change Path Type row present in overlay");
        let center = target.visible_bounds().expect("row visible").center();
        ui.point_at(center);
        ui.simulate([iced_core::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: center,
        })]);
        let mut saw_enter = false;
        for message in ui.into_messages() {
            if matches!(message, Message::XrefRowChangePathEnter) {
                saw_enter = true;
            }
        }
        assert!(
            saw_enter,
            "hovering Change Path Type must publish XrefRowChangePathEnter (opens the second menu)"
        );
    }

    #[test]
    fn context_menu_flyout_click_delivers_pathtype_op() {
        let make_absolute = crate::t!("Make Absolute").into_owned();
        let mut ui = open_row_menu(true);
        ui.click(make_absolute.as_str())
            .expect("flyout row is clickable inside the overlay");
        assert!(
            published_ops(ui).contains(&(
                XrefRowId::of(0, ""),
                XrefPaletteOp::Pathtype(Pathtype::Full)
            )),
            "clicking Make Absolute must publish the Full-pathtype op"
        );
    }

    #[test]
    fn context_menu_overlay_subtree_state_is_reset_every_view() {
        // Root-cause pin. `iced_aw::ContextMenu` keeps its overlay `Element`
        // in the WIDGET STRUCT (`overlay_instance`), which is rebuilt on every
        // `view()` — not in persistent widget `Tree` state. So `diff` takes
        // the `None` arm below on every app update cycle and wipes the whole
        // overlay subtree state:
        //
        //   match self.overlay_instance.as_mut() {
        //       Some(overlay) => tree.children[1].diff(overlay),
        //       None => tree.children[1] = Tree::empty(),   // <-- always hit
        //   }
        //
        // Consequence: a `button` inside the menu sets `is_pressed` on press
        // and only publishes on RELEASE — but any message in between (ticks,
        // hover, clock) rebuilds the view, wipes `is_pressed`, and the
        // release publishes nothing ("buttons do nothing"). Menu rows must
        // therefore publish on PRESS via a single `mouse_area` (see
        // `menu_row`), which needs no transient state. If this test ever
        // fails, iced_aw learned to persist overlay state and rows may use
        // buttons again.
        use iced_core::Widget;

        let mut menu = iced_aw::ContextMenu::new(text("anchor"), || {
            row_menu_for(XrefRowId::of(0, ""), false, false, Pathtype::Relative)
        });
        let mut tree = iced_core::widget::Tree::new(
            &menu as &dyn Widget<Message, iced::Theme, iced::Renderer>,
        );
        menu.diff(&mut tree);
        assert_eq!(tree.children.len(), 2);
        assert!(
            tree.children[1].children.is_empty(),
            "fresh ContextMenu widget must start with an empty overlay subtree"
        );
    }
}
