//! Reference Manager palette — list of external references with operations.
//!
//! Task 8b build: the toolbar's mutating affordances are wired to the same
//! engine fns the CLI uses, operating on the multi-selection with per-item
//! report lines. Session state (unloaded set, stat cache) lives per-tab on
//! `DocumentTab`; this panel only caches display rows plus text inputs.

use crate::app::Message;
use crate::io::xref::collect_entries_with_prev;
use crate::io::xref_model::{normalize_lexical, Pathtype, RefKind, RefStatus, RefType, ReferenceEntry};
use crate::ui::ROW_H;
use crate::ui::style::common::muted_style;
use acadrust::CadDocument;
use iced::widget::{button, column, container, mouse_area, row, scrollable, text, tooltip};
use iced::Padding;
use iced::{Background, Border, Element, Fill, Length, Theme};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// `true` on web builds, where the filesystem and native file pickers do not
/// exist. Mutating affordances stay compiled but render disabled; the
/// read-only list renders everywhere.
const IS_WASM: bool = cfg!(target_arch = "wasm32");



/// Font size for table cells (mirrors `layers.rs`).
const FONT_SZ: f32 = ROW_H * 0.42; // ≈11 px at ROW_H=26
/// Fixed table height so the details pane below keeps stable space.
const TABLE_H: f32 = 480.0;
/// Minimum table area height under split dragging.
const TABLE_MIN_H: f32 = 120.0;
/// Minimum table content width so columns stay readable in narrow docks.
const TABLE_MIN_W: f32 = 480.0;
/// Inter-column gutter shared by header, rows, and grab zones so columns align.
const COL_GUTTER: f32 = 6.0;
/// Tree-mode text size: a step above the table cells, like the
/// industry tree.
/// Indent per tree depth level.
const INDENT_W: f32 = 16.0;

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
const TREE_FONT_SZ: f32 = 13.0;
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
        doc: &CadDocument,
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
        // Previews decode here and on selection change (view stays pure):
        // the anchor entry only, and only under single selection.
        self.previews.clear();
        self.ensure_preview_for_anchor();
        self.expanded.retain(|k| live_keys.contains(k));
        self.entries
            .iter()
            .filter(|e| e.status == RefStatus::Loaded)
            .filter_map(|e| e.modified.map(|m| (e.key, m)))
            .collect()
    }

    /// Click handling for row picks with normal GUI list semantics:
    /// - plain click selects exactly one row (previous selection clears);
    /// - Ctrl/Cmd-click toggles one row;
    /// - Shift-click extends a contiguous range from the anchor (or the
    ///   clicked row when there is no anchor yet).
    /// Tree mode always single-selects per spec, whatever the modifiers.
    pub fn click_select(&mut self, index: usize, extend: SelectExtend) {
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
                let (lo, hi) = if from <= index { (from, index) } else { (index, from) };
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
        self.click_select(index, SelectExtend::Toggle);
    }

    /// Right-click selection: select the row alone when it is not already
    /// selected (standard right-click behavior — the menu then acts on the
    /// selection, which now includes this row). Keeps multi-selections when
    /// right-clicking inside them.
    pub fn right_click_select(&mut self, index: usize) {
        if index == HOST_ROW || index >= self.entries.len() {
            return;
        }
        if !self.selected.contains(&index) {
            self.click_select(index, SelectExtend::Single);
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
        if self.previews.contains_key(&key) {
            return;
        }
        if let Some(img) = reference_preview(a) {
            let (w, h) = (img.width(), img.height());
            self.previews
                .insert(key, iced::widget::image::Handle::from_rgba(w, h, img.into_raw()));
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
            rows.extend(self.entries.iter().enumerate().map(|(index, _)| {
                DisplayRow {
                    index,
                    depth: 0,
                    is_nested: self.nested.contains(&index),
                }
            }));
            return rows;
        }
        fn append_tree(
            panel: &XrefManagerPanel,
            index: usize,
            depth: u32,
            seen: &mut HashSet<(String, String)>,
            rows: &mut Vec<DisplayRow>,
        ) {
            let Some(entry) = panel.entries.get(index) else { return; };
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
            let dominated = self
                .children
                .values()
                .any(|kids| kids.contains(&index));
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
        auto_collapse: bool,
        missing: usize,
        doc: &'a CadDocument,
    ) -> Element<'a, Message> {
        use crate::ui::dock::{DockMsg, PanelId};
        // ── Dock chrome (title, pin, close) — matches the block palette ──
        let pin_icon = if auto_collapse {
            crate::ui::icons::themed_primary_weak_text(crate::ui::icons::PIN, 12.0)
        } else {
            crate::ui::icons::themed_secondary(crate::ui::icons::PIN, 12.0)
        };
        let pin = button(pin_icon)
            .on_press(Message::Dock(DockMsg::AutoCollapseToggle(
                PanelId::ExternalReferences,
            )))
            .style(move |theme: &Theme, status| {
                let mut style = button::subtle(theme, status);
                if auto_collapse {
                    let palette = theme.palette();
                    style.background = Some(Background::Color(palette.primary.weak.color));
                    style.text_color = palette.primary.weak.text;
                    style.border.color = palette.primary.base.color;
                    style.border.width = 1.0;
                }
                style
            })
            .padding([3, 5]);
        let pin = tooltip(pin, text("Auto").size(10), tooltip::Position::Bottom).gap(4);
        let close = button(crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 12.0))
            .on_press(Message::Dock(DockMsg::Close(PanelId::ExternalReferences)))
            .style(button::subtle)
            .padding([3, 5]);
        let close = tooltip(close, text("Close").size(10), tooltip::Position::Bottom).gap(4);
        let title_bar = mouse_area(
            container(
                row![
                    text(format!(
                        "{} ({})",
                        crate::t!("External References").as_ref(),
                        self.entries.len()
                    ))
                    .size(12),
                    iced::widget::Space::new().width(Fill),
                    pin,
                    close,
                ]
                .spacing(3)
                .align_y(iced::Center),
            )
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.weak.color)),
                ..Default::default()
            })
            .width(Fill)
            .padding([3, 6]),
        )
        .on_press(Message::Dock(DockMsg::DockGrab(PanelId::ExternalReferences)))
        .interaction(iced::mouse::Interaction::Grab);
        // Table content width: column widths plus gutters, stretched to the
        // dock when wider so rows fill the panel; narrower docks sidescroll.
        let table_w = (width - 16.0).max(self.table_content_width());
        // ── Toolbar: split-button groups per spec ───────────────────────
        // Attach ▾ (default DWG), Refresh ▾ (default Refresh), Change Path ▾.
        // The main button runs the default; the triangle opens the rest in an
        // overlay menu. Formats without an attach command in this build
        // (DWF/DGN/point clouds/coordination models) are omitted, not dead.
        let web_tip = crate::t!("File attach is not available on web — the reference list below is read-only.").into_owned();
        let attach = if IS_WASM {
            toolbar_tip(crate::t!("Attach DWG").into_owned(), web_tip.clone(), false)
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
            toolbar_tip(crate::t!("Refresh").into_owned(), web_tip.clone(), false)
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
            let msg = if active {
                None
            } else {
                Some(Message::XrefManagerToggleTree)
            };
            let mut b = button(text(label).size(11))
                .style(move |theme: &Theme, status| {
                    let palette = theme.palette();
                    if active {
                        button::Style {
                            background: Some(Background::Color(palette.primary.base.color)),
                            border: Border {
                                radius: 3.0.into(),
                                color: palette.primary.base.color,
                                width: 1.0,
                            },
                            text_color: palette.primary.base.text,
                            ..Default::default()
                        }
                    } else {
                        let pair = match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                palette.background.strong
                            }
                            _ => palette.background.weak,
                        };
                        button::Style {
                            background: Some(Background::Color(pair.color)),
                            border: Border {
                                radius: 3.0.into(),
                                color: palette.background.neutral.color,
                                width: 1.0,
                            },
                            text_color: pair.text,
                            ..Default::default()
                        }
                    }
                })
                .padding([4, 10]);
            if let Some(m) = msg {
                b = b.on_press(m);
            }
            b.into()
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
            self.selected.len() == 1
                && self.entries.get(a).is_some()
                && !self.nested.contains(&a)
        });
        let host_saved = !self.host_path.is_empty();
        let web_readonly: Option<String> = IS_WASM.then(|| {
            crate::t!("Reference changes are not available on web — the reference list is read-only.").into_owned()
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
            Some(reason) => toolbar_tip(crate::t!("Change Path").into_owned(), reason, false),
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
        let help = toolbar_btn(
            crate::t!("Help").into_owned(),
            Some(Message::XrefHelpOpen),
            false,
        );
        let toolbar = container(
            row![
                attach,
                refresh,
                change_path,
                help
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.weakest.color,
            )),
            ..Default::default()
        })
        .width(Fill)
        .padding([4, 8]);

        // ── File References pane title ──────────────────────────────────
        // List / Tree live on this row, right-aligned.
        let table_title: Element<'_, Message> = container(
            row![
                text(format!("{} :", crate::t!("File References").as_ref()))
                    .size(11)
                    .width(Fill),
                list_btn,
                tree_btn,
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .width(Fill)
        .padding(Padding {
            top: 4.0,
            right: 8.0,
            bottom: 0.0,
            left: 8.0,
        })
        .into();
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
                text(label)
                    .size(10)
                    .style(muted_style)
                    .width(Length::Fixed(self.col_widths[i])),
            );
            if i + 1 < self.col_widths.len() {
                let grab = mouse_area(
                    container(iced::widget::Space::new().width(Length::Fixed(6.0)))
                        .width(Length::Fixed(6.0))
                        .height(Length::Fixed(ROW_H * 0.7)),
                )
                .on_press(Message::XrefColGrab(i))
                .interaction(iced::mouse::Interaction::ResizingHorizontally);
                header_row = header_row.push(grab);
            }
        }
        let col_header: Element<'_, Message> = mouse_area(
            container(header_row.width(Length::Fixed(table_w)))
            .style(|theme: &Theme| {
                let palette = theme.palette();
                container::Style {
                    background: Some(Background::Color(palette.background.weak.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }
            })
            .padding([4, 8])
            .width(Fill),
        )
        .on_move(|p| Message::XrefColMove(p))
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
            container(
                text(crate::t!("XREF  No external references in this drawing."))
                    .size(12)
                    .color(iced::Color {
                        r: 0.55,
                        g: 0.55,
                        b: 0.55,
                        a: 1.0,
                    }),
            )
            .center_x(Fill)
            .center_y(Fill)
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
            scrollable(tree_col).height(Length::Fixed(self.table_h)).into()
        } else {
            scrollable(
                column![
                    col_header,
                    scrollable(rows_col).height(Length::Fixed(self.table_h)),
                ]
                .width(Length::Fixed(table_w)),
            )
            .direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::new(),
            ))
            .height(Length::Fixed(self.table_h))
            .into()
        };
        // Thin external padding + darkest surface around the table, like the
        // properties panel content.
        let table_rows: Element<'_, Message> = container(table_rows)
            .padding(4)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.palette().background.weakest.color,
                )),
                ..Default::default()
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
                .padding([6, 8])
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
        let details_tab = toolbar_btn(
            crate::t!("Details").into_owned(),
            if self.show_preview {
                Some(Message::XrefManagerTogglePreview)
            } else {
                None
            },
            false,
        );
        let preview_tab = toolbar_btn(
            crate::t!("Preview").into_owned(),
            if self.show_preview {
                None
            } else {
                Some(Message::XrefManagerTogglePreview)
            },
            false,
        );
        let pane_tabs = row![details_tab, preview_tab]
            .spacing(4)
            .align_y(iced::Center);
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
                    let cached = e.found_at.as_deref().and_then(|found| {
                        self.previews.get(&(e.key, found.to_string()))
                    });
                    match cached {
                        Some(handle) => container(
                            iced::widget::image(handle.clone())
                                .width(Length::Fixed(220.0)),
                        )
                        .center_x(Fill)
                        .center_y(Fill)
                        .width(Fill)
                        .height(Length::Fixed(120.0))
                        .into(),
                        None => container(
                            column![
                                text(e.name.as_str()).size(11),
                                text(crate::t!("Preview not available")).size(11).style(muted_style),
                            ]
                            .spacing(4)
                            .align_x(iced::Center),
                        )
                        .center_x(Fill)
                        .center_y(Fill)
                        .width(Fill)
                        .height(Length::Fixed(120.0))
                        .into(),
                    }
                }
                None => container(text("—").size(11).style(muted_style))
                    .center_x(Fill)
                    .center_y(Fill)
                    .width(Fill)
                    .height(Length::Fixed(120.0))
                    .style(|theme: &Theme| container::Style {
                        background: Some(Background::Color(
                            theme.palette().background.weak.color,
                        )),
                        ..Default::default()
                    })
                    .into(),
            };
            container(column![pane_tabs, preview_body].spacing(2))
                .padding([6, 8])
                .width(Fill)
                .into()
        } else {
            let details =
                details_pane(self.anchor.and_then(|i| self.entries.get(i)), doc);
            container(column![pane_tabs, details].spacing(2))
                .padding([6, 8])
                .width(Fill)
                .into()
        };

        // Horizontal split divider between the table and the lower pane:
        // grabs vertically to give the table more (or less) height.
        let split = mouse_area(
            container(iced::widget::Space::new().height(Length::Fixed(6.0)))
                .width(Fill)
                .height(Length::Fixed(6.0)),
        )
        .on_press(Message::XrefSplitGrab)
        .interaction(iced::mouse::Interaction::ResizingVertically);
        let mut content = column![title_bar, toolbar, table_title, table_rows, split];
        if let Some(notice) = notice {
            content = content.push(notice);
        }
        content = content.push(lower);
        // Panel-wide move/release tracking for the split drag (mirrors the
        // header divider tracking for columns).
        let content: Element<'_, Message> = mouse_area(content.spacing(0))
            .on_move(|p| Message::XrefSplitMove(p))
            .on_release(Message::XrefSplitRelease)
            .into();
        // Fixed to the dock width like the block palette: the panel never
        // sizes itself from its (wider) table content, so the dock width
        // setting — and resize — actually takes effect.
        container(content)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.palette().background.base.color,
                )),
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
fn direct_identities(doc: &CadDocument) -> HashSet<(u64, String)> {
    use acadrust::entities::UnderlayType;
    use acadrust::objects::ObjectType;

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
                if def.underlay_type == UnderlayType::Pdf {
                    ids.insert((handle.value(), def.file_path.clone()));
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

fn status_text(status: RefStatus) -> std::borrow::Cow<'static, str> {
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
        RefKind::Pdf => crate::t!("PDF"),
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
    let d = doy as i64 - (153 * mp as u64 + 2) as i64 / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as i64;
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

// ── Widget helpers (layers.rs conventions) ────────────────────────────────

fn row_button_style(selected: bool, index: usize) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let palette = theme.palette();
        let highlighted = matches!(status, button::Status::Hovered);
        let pair = if highlighted {
            palette.background.strong
        } else if selected {
            palette.primary.weak
        } else if index % 2 == 0 {
            palette.background.base
        } else {
            palette.background.weak
        };
        button::Style {
            background: highlighted.then_some(Background::Color(pair.color)),
            text_color: pair.text,
            ..Default::default()
        }
    }
}

fn toolbar_btn(label: String, msg: Option<Message>, fill: bool) -> Element<'static, Message> {
    let mut b = button(text(label).size(11))
        .style(|theme: &Theme, status| {
            let palette = theme.palette();
            let pair = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    palette.background.strong
                }
                _ => palette.background.weak,
            };
            button::Style {
                background: Some(Background::Color(pair.color)),
                border: Border {
                    radius: 3.0.into(),
                    color: palette.background.neutral.color,
                    width: 1.0,
                },
                text_color: pair.text,
                ..Default::default()
            }
        })
        .padding([4, 10]);
    if fill {
        b = b.width(Fill);
    }
    if let Some(m) = msg {
        b = b.on_press(m);
    }
    b.into()
}

/// Disabled toolbar affordance with an explanatory tooltip (gated
/// operations, nested-selection blocks, or web-gated mutations).
fn toolbar_tip(label: String, tip: String, fill: bool) -> Element<'static, Message> {
    let b = button(text(label).size(11).style(|theme: &Theme| {
        iced::widget::text::Style {
            color: Some(
                theme
                    .palette()
                    .background
                    .base
                    .text
                    .scale_alpha(0.42),
            ),
        }
    }))
    .style(|theme: &Theme, _| {
        let palette = theme.palette();
        button::Style {
            background: Some(Background::Color(palette.background.weak.color)),
            border: Border {
                radius: 3.0.into(),
                color: palette.background.neutral.color,
                width: 1.0,
            },
            text_color: palette.background.weak.text.scale_alpha(0.42),
            ..Default::default()
        }
    })
    .padding([4, 10]);
    let b = if fill { b.width(Fill) } else { b };
    tooltip(b, text(tip).size(11), tooltip::Position::Bottom).into()
}

/// Split button: a default action plus a triangle that opens the rest in an
/// overlay menu (spec toolbar: Attach ▾, Refresh ▾, Change Path ▾).
///
/// Both halves share one bordered group look: the main half keeps only the
/// left corners rounded (no right border), the caret keeps only the right
/// corners (no left border), so the pair reads as a single control. Hover
/// and press states highlight each half independently.
/// `main_msg` runs the default action; `toggle` flips this menu (closing the
/// others is handled at the message site); `items` are the menu rows, each
/// full-width. Dismissal (Escape / outside click) funnels to
/// [`Message::XrefManagerDismissMenus`].
fn split_button(
    main_label: String,
    main_msg: Option<Message>,
    menu_open: bool,
    toggle: Message,
    items: Vec<Element<'static, Message>>,
) -> Element<'static, Message> {
    use iced::border::Radius;
    // Triangle matches the main button's height and bordered style so the
    // group reads as one split control: same padding, icon box fixed to the
    // 11px text line height.
    let half_style = |left: bool| {
        move |theme: &Theme, status| {
            let palette = theme.palette();
            let pair = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    palette.background.strong
                }
                _ => palette.background.weak,
            };
            let radius = if left {
                Radius {
                    top_left: 3.0,
                    top_right: 0.0,
                    bottom_right: 0.0,
                    bottom_left: 3.0,
                }
            } else {
                Radius {
                    top_left: 0.0,
                    top_right: 3.0,
                    bottom_right: 3.0,
                    bottom_left: 0.0,
                }
            };
            button::Style {
                background: Some(Background::Color(pair.color)),
                border: Border {
                    radius: radius.into(),
                    color: palette.background.neutral.color,
                    width: 1.0,
                },
                text_color: pair.text,
                ..Default::default()
            }
        }
    };
    let mut main = button(text(main_label).size(11))
        .style(half_style(true))
        .padding([4, 10]);
    if let Some(m) = main_msg {
        main = main.on_press(m);
    }
    let caret = button(
        container(crate::ui::icons::themed_arrow_down(9.0))
            .align_y(iced::Center)
            .height(Length::Fixed(15.0)),
    )
    .on_press(toggle)
    .style(half_style(false))
    .padding([4, 10]);
    // Overlap the halves by the 1px shared border so no double line shows
    // where they meet.
    let head = row![main, caret].spacing(-1.0).align_y(iced::Center);
    let popup: Element<'static, Message> = container(
        column(items.into_iter().map(|item| {
            container(item).width(Fill).into()
        }))
        .spacing(2)
        .padding(4),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    })
    .width(Length::Fixed(220.0))
    .into();
    iced_aw::DropDown::new(head, popup, menu_open)
        .alignment(iced_aw::drop_down::Alignment::Bottom)
        .offset(2.0)
        .on_dismiss(Message::XrefManagerDismissMenus)
        .into()
}

/// One dropdown-menu row: a full-width button when the option can execute,
/// otherwise greyed text carrying the reason.
fn menu_item(label: String, msg: Option<Message>, gate: Option<String>) -> Element<'static, Message> {
    match (msg, gate) {
        (Some(m), None) => toolbar_btn(label, Some(m), true),
        (_, reason) => toolbar_tip(label, reason.unwrap_or_default(), true),
    }
}

/// Wrapper that publishes a right-click message before delegating to child
/// (which may capture right-clicks, e.g. `ContextMenu`).
struct RightClickArea<'a> {
    child: Element<'a, Message>,
    on_right_press: Message,
}

impl<'a> RightClickArea<'a> {
    pub fn new(child: impl Into<Element<'a, Message>>, on_right_press: Message) -> Self {
        Self {
            child: child.into(),
            on_right_press,
        }
    }
}

impl<'a> iced_core::Widget<Message, Theme, iced::Renderer> for RightClickArea<'a> {
    fn diff(&mut self, tree: &mut iced_core::widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
    }

    fn size(&self) -> iced::Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut iced_core::widget::Tree,
        renderer: &iced::Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        self.child
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut iced_core::widget::Tree,
        event: &iced_core::Event,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut iced_core::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        if let iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(
            iced_core::mouse::Button::Right,
        )) = event
        {
            if cursor.is_over(layout.bounds()) {
                shell.publish(self.on_right_press.clone());
            }
        }
        self.child.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &iced_core::widget::Tree,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &iced::Renderer,
    ) -> iced_core::mouse::Interaction {
        self.child
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut iced_core::widget::Tree,
        layout: iced_core::Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced_core::widget::Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn draw(
        &self,
        tree: &iced_core::widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &iced_core::renderer::Style,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        self.child.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut iced_core::widget::Tree,
        layout: iced_core::Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &iced::Rectangle,
        translation: iced::Vector,
    ) -> Option<iced_core::overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.child.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<RightClickArea<'a>> for Element<'a, Message> {
    fn from(widget: RightClickArea<'a>) -> Self {
        Element::new(widget)
    }
}

const MENU_ROW_H: f32 = 24.0;
const MENU_SEP_H: f32 = 5.0;
const MENU_PADDING: f32 = 4.0;
const MENU_SPACING: f32 = 1.0;

/// One right-click menu row, styled like the model-space context menu:
/// borderless subtle text rows. Hovering a regular row dismisses any open flyout.
///
/// NOTE: this must stay a SINGLE interactive widget (`mouse_area` alone).
/// Wrapping a `button` in a `mouse_area` here breaks click delivery inside
/// the `ContextMenu` overlay: the overlay subtree state (including a button's
/// press state) is wiped on every view rebuild, so a press → release button
/// never publishes. `on_press` fires in the press batch itself and needs no
/// saved state.
fn menu_row(label: String, msg: Message) -> Element<'static, Message> {
    mouse_area(
        container(text(label).size(12))
            .padding([4, 12])
            .height(Length::Fixed(MENU_ROW_H))
            .width(Fill),
    )
    .on_press(msg)
    .on_enter(Message::XrefRowChangePathLeave)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

/// One flyout submenu row: does not close the flyout on hover. Same
/// single-`mouse_area` rule as [`menu_row`] — no nested `button`.
fn submenu_row(label: String, msg: Message) -> Element<'static, Message> {
    mouse_area(
        container(text(label).size(12))
            .padding([4, 12])
            .height(Length::Fixed(MENU_ROW_H))
            .width(Fill),
    )
    .on_press(msg)
    .on_enter(Message::XrefRowChangePathEnter)
    .on_exit(Message::XrefRowChangePathLeave)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

fn menu_separator() -> Element<'static, Message> {
    container(
        container(iced::widget::Space::new().height(Length::Fixed(1.0)))
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.neutral.color)),
                ..Default::default()
            })
            .width(Fill),
    )
    .padding([2, 8])
    .height(Length::Fixed(MENU_SEP_H))
    .width(Fill)
    .into()
}

/// Per-row right-click menu. Eight items exactly: universal tools +
/// path tools. No Bind, no Attach/Overlay — those are DWG-only and
/// hidden entirely. Change Path Type shows a right triangle and a
/// second list adjacent to "Change Path Type" on hover.
fn row_menu_for(index: usize, change_path_open: bool) -> Element<'static, Message> {
    let item = |label: &str, op: XrefPaletteOp| -> Element<'static, Message> {
        menu_row(label.to_string(), Message::XrefRowOp(index, op))
    };
    let pathtype_item = |label: &str, pt: Pathtype| -> Element<'static, Message> {
        submenu_row(
            label.to_string(),
            Message::XrefRowOp(index, XrefPaletteOp::Pathtype(pt)),
        )
    };
    let change_path_row: Element<'static, Message> = mouse_area(
        container(
            row![
                text(crate::t!("Change Path Type").into_owned()).size(12).width(Fill),
                crate::ui::icons::themed_arrow_right(10.0),
            ]
            .align_y(iced::Center)
            .spacing(8),
        )
        .style(move |theme: &Theme| {
            let palette = theme.palette();
            let bg = if change_path_open {
                Some(Background::Color(palette.background.weak.color))
            } else {
                None
            };
            container::Style {
                background: bg,
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([4, 12])
        .height(Length::Fixed(MENU_ROW_H))
        .width(Fill),
    )
    .on_enter(Message::XrefRowChangePathEnter)
    .into();

    let main: Element<'static, Message> = container(
        column![
            item(&crate::t!("Open").into_owned(), XrefPaletteOp::Open),
            item(&crate::t!("Attach...").into_owned(), XrefPaletteOp::Attach),
            item(&crate::t!("Unload").into_owned(), XrefPaletteOp::Unload),
            item(&crate::t!("Reload").into_owned(), XrefPaletteOp::Reload),
            item(&crate::t!("Detach").into_owned(), XrefPaletteOp::Detach),
            menu_separator(),
            change_path_row,
            menu_separator(),
            menu_row(
                crate::t!("Select New Path").into_owned(),
                Message::XrefRowPathPick(index),
            ),
            menu_row(
                crate::t!("Find and Replace...").into_owned(),
                Message::XrefRowFindReplacePrompt(index),
            ),
        ]
        .spacing(MENU_SPACING)
        .padding(MENU_PADDING),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    })
    .width(Length::Fixed(220.0))
    .into();

    if !change_path_open {
        return main;
    }

    // Top offset so the flyout top border aligns directly with the "Change Path Type" row.
    // 5 items * 24.0 + 5 * 1.0 (spacing) + 5.0 (separator) + 1.0 (spacing) + 4.0 (container padding) = 135.0
    let flyout_offset = MENU_PADDING + 5.0 * MENU_ROW_H + 5.0 * MENU_SPACING + MENU_SEP_H + MENU_SPACING;

    // Plain container on purpose: each submenu row tracks hover itself
    // (`on_enter` keeps the flyout open, `on_exit` closes it). Wrapping the
    // clickable rows in an outer `mouse_area` would nest interactives and
    // break click delivery inside the `ContextMenu` overlay — see `menu_row`.
    let flyout: Element<'static, Message> = container(
        column![
            pathtype_item(
                &crate::t!("Make Absolute").into_owned(),
                Pathtype::Full
            ),
            pathtype_item(
                &crate::t!("Make Relative").into_owned(),
                Pathtype::Relative
            ),
            pathtype_item(&crate::t!("Remove Path").into_owned(), Pathtype::None),
        ]
        .spacing(MENU_SPACING)
        .padding(MENU_PADDING),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    })
    .width(Length::Fixed(180.0))
    .into();

    let flyout_column = column![
        iced::widget::Space::new().height(Length::Fixed(flyout_offset)),
        flyout,
    ];

    row![main, flyout_column]
        .spacing(0)
        .align_y(iced::alignment::Vertical::Top)
        .into()
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
    let mut name_cell = row![].spacing(2).align_y(iced::Center);
    if display.depth > 0 {
        name_cell = name_cell.push(
            iced::widget::Space::new().width(Length::Fixed(INDENT_W * display.depth as f32)),
        );
    }
    // Tree parents own the only interactive cell besides selection: the
    // expand/collapse arrow. Nested rows never show one.
    if show_expand {
        let key = entry.key;
        let arrow = button(crate::ui::icons::themed_arrow_toggle(is_expanded, 10.0))
            .on_press(Message::XrefManagerToggleExpand(key))
            .style(row_button_style(is_selected, display.index))
            .padding(Padding {
                top: 6.0,
                bottom: 6.0,
                left: 2.0,
                right: 2.0,
            });
        name_cell = name_cell.push(arrow);
    }
    let name_text: Element<'_, Message> = text(name).size(FONT_SZ).into();
    name_cell = name_cell.push(name_text);
    let gutter = || iced::widget::Space::new().width(Length::Fixed(COL_GUTTER));
    let status_cell: Element<'_, Message> = row![
        container(status_icon(entry.status)).align_y(iced::Center),
        text(status_text(entry.status)).size(FONT_SZ).width(Fill),
    ]
    .spacing(4)
    .align_y(iced::Center)
    .width(Length::Fixed(cw[1]))
    .into();
    let content = row![
        name_cell.width(Length::Fixed(cw[0])),
        gutter(),
        status_cell,
        gutter(),
        text(format_size(entry.size_bytes)).size(FONT_SZ).width(Length::Fixed(cw[2])),
        gutter(),
        text(type_text(entry)).size(FONT_SZ).width(Length::Fixed(cw[3])),
        gutter(),
        text(format_date(entry.modified)).size(FONT_SZ).width(Length::Fixed(cw[4])),
        gutter(),
        text(entry.saved_path.clone()).size(FONT_SZ).width(Length::Fixed(cw[5])),
    ]
    .spacing(0)
    .width(Length::Fixed(table_w))
    .align_y(iced::Center);

    let index = display.index;
    let row = mouse_area(
        container(content)
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                let pair = if is_selected {
                    palette.primary.weak
                } else if index % 2 == 0 {
                    palette.background.base
                } else {
                    palette.background.weak
                };
                container::Style {
                    background: Some(Background::Color(pair.color)),
                    text_color: Some(pair.text),
                    ..Default::default()
                }
            })
            .padding(Padding {
                top: 0.0,
                bottom: 0.0,
                left: 8.0,
                right: 8.0,
            })
            .height(Length::Fixed(ROW_H))
            .width(Fill),
    )
    .on_press(Message::XrefManagerSelect(index));
    let cm = iced_aw::ContextMenu::new(row, move || row_menu_for(index, change_path_open));
    RightClickArea::new(cm, Message::XrefRowRightClick(index)).into()
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
    let mut cells = row![].spacing(2).align_y(iced::Center);
    cells = cells.push(tree_indent(display.depth));
    if show_expand {
        let key = entry.key;
        let arrow = button(crate::ui::icons::themed_arrow_toggle(is_expanded, 10.0))
            .on_press(Message::XrefManagerToggleExpand(key))
            .style(row_button_style(is_selected, display.index))
            .padding(Padding {
                top: 6.0,
                bottom: 6.0,
                left: 2.0,
                right: 2.0,
            });
        cells = cells.push(arrow);
    }
    cells = cells.push(
        container(crate::ui::icons::themed_secondary(crate::ui::icons::DOC, 14.0))
            .align_y(iced::Center),
    );
    cells = cells.push(text(entry.name.clone()).size(TREE_FONT_SZ));
    let index = display.index;
    let row = mouse_area(
        container(cells.spacing(4).width(Fill))
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                let pair = if is_selected {
                    palette.primary.weak
                } else if index % 2 == 0 {
                    palette.background.base
                } else {
                    palette.background.weak
                };
                container::Style {
                    background: Some(Background::Color(pair.color)),
                    text_color: Some(pair.text),
                    ..Default::default()
                }
            })
            .padding(Padding {
                top: 5.0,
                bottom: 5.0,
                left: 8.0,
                right: 8.0,
            })
            .width(Fill),
    )
    .on_press(Message::XrefManagerSelect(index));
    let cm = iced_aw::ContextMenu::new(row, move || row_menu_for(index, change_path_open));
    RightClickArea::new(cm, Message::XrefRowRightClick(index)).into()
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
    let mut cells = row![].spacing(2).align_y(iced::Center);
    cells = cells.push(tree_indent(display.depth));
    cells = cells.push(
        container(crate::ui::icons::themed_home(14.0)).align_y(iced::Center),
    );
    cells = cells.push(text(name).size(TREE_FONT_SZ));
    container(cells.spacing(4).width(Fill))
        .style(|theme: &Theme| {
            let pair = theme.palette().background.base;
            container::Style {
                background: Some(Background::Color(pair.color)),
                text_color: Some(pair.text),
                ..Default::default()
            }
        })
        .padding(Padding {
            top: 5.0,
            bottom: 5.0,
            left: 8.0,
            right: 8.0,
        })
        .width(Fill)
        .into()
}
fn host_row<'a>(display: DisplayRow, host_name: &'a str, host_path: &'a str, table_w: f32, cw: &[f32; 6]) -> Element<'a, Message> {
    let name = if host_name.is_empty() {
        crate::t!("Untitled").into_owned()
    } else {
        host_name.to_string()
    };
    let saved = if host_path.is_empty() { "—" } else { host_path };
    let mut name_cell = row![].spacing(2).align_y(iced::Center);
    if display.depth > 0 {
        name_cell = name_cell.push(
            iced::widget::Space::new().width(Length::Fixed(INDENT_W * display.depth as f32)),
        );
    }
    name_cell = name_cell.push({
        let name_text: Element<'_, Message> = text(name).size(FONT_SZ).into();
        name_text
    });
    let gutter = || iced::widget::Space::new().width(Length::Fixed(COL_GUTTER));
    let content = row![
        name_cell.width(Length::Fixed(cw[0])),
        gutter(),
        text(crate::t!("Opened")).size(FONT_SZ).width(Length::Fixed(cw[1])),
        gutter(),
        text("—").size(FONT_SZ).width(Length::Fixed(cw[2])),
        gutter(),
        text(crate::t!("Current")).size(FONT_SZ).width(Length::Fixed(cw[3])),
        gutter(),
        text("—").size(FONT_SZ).width(Length::Fixed(cw[4])),
        gutter(),
        text(saved).size(FONT_SZ).width(Length::Fixed(cw[5])),
    ]
    .spacing(0)
    .width(Length::Fixed(table_w))
    .align_y(iced::Center);
    container(content)
        .style(|theme: &Theme| {
            let pair = theme.palette().background.base;
            container::Style {
                background: Some(Background::Color(pair.color)),
                text_color: Some(pair.text),
                ..Default::default()
            }
        })
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 8.0,
            right: 8.0,
        })
        .height(Length::Fixed(ROW_H))
        .width(Fill)
        .into()
}

fn details_pane<'a>(entry: Option<&'a ReferenceEntry>, doc: &'a CadDocument) -> Element<'a, Message> {
    let inner: Element<'_, Message> = match entry {
        None => text(crate::t!("Select a reference to inspect its details"))
            .size(11)
            .style(muted_style)
            .into(),
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
            ]
            .spacing(1);
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
                        .push(detail_row(
                            crate::t!("Display Width"),
                            format!("{:.2}", w),
                        ))
                        .push(detail_row(
                            crate::t!("Display Height"),
                            format!("{:.2}", h),
                        ));
                }
            }
            rows.into()
        }
    };
    container(
        column![
            text(crate::t!("Details")).size(10).style(muted_style),
            inner,
        ]
        .spacing(2),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .padding([6, 8])
    .width(Fill)
    .into()
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
        RefKind::Pdf => None,
    }
}

/// Image definition backing a [`RefKind::Image`] entry, looked up by the
/// entry key (definition-object handle) for the Details pane extras.
fn find_image_def(
    doc: &CadDocument,
    key: u64,
) -> Option<&acadrust::objects::ImageDefinition> {
    doc.objects.get(&acadrust::types::Handle::from(key)).and_then(|o| match o {
        acadrust::objects::ObjectType::ImageDefinition(def) => Some(def),
        _ => None,
    })
}

fn find_image_display_size(doc: &CadDocument, key: u64) -> Option<(f64, f64)> {
    let handle = acadrust::types::Handle::from(key);
    for entity in doc.entities() {
        if let acadrust::EntityType::RasterImage(img) = entity {
            if img.definition_handle == Some(handle) {
                let w = (img.u_vector.x.powi(2) + img.u_vector.y.powi(2) + img.u_vector.z.powi(2)).sqrt();
                let h = (img.v_vector.x.powi(2) + img.v_vector.y.powi(2) + img.v_vector.z.powi(2)).sqrt();
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
    row![
        text(label).size(11).style(muted_style).width(Length::Fixed(84.0)),
        text(value).size(11),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
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
            format_date(Some(UNIX_EPOCH + std::time::Duration::from_secs(days * 86_400)))
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
        let count = panel
            .display_rows()
            .iter()
            .filter(|r| r.index == 2)
            .count();
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
        assert_eq!(panel.table_content_width(), panel.col_widths.iter().sum::<f32>() + super::COL_GUTTER * 5.0);
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
        let mut panel = XrefManagerPanel::default();
        panel.entries = vec![
            entry(1, "A", "a.dwg"),
            entry(2, "B", "b.dwg"),
            entry(3, "C", "c.dwg"),
        ];
        // Plain click selects exactly one row.
        panel.click_select(0, SelectExtend::Single);
        panel.click_select(2, SelectExtend::Single);
        assert_eq!(panel.selected, HashSet::from([2]));
        assert_eq!(panel.anchor, Some(2));
        // Shift-click extends a contiguous range from the anchor.
        panel.click_select(2, SelectExtend::Single);
        panel.click_select(0, SelectExtend::Range);
        assert_eq!(panel.selected, HashSet::from([0, 1, 2]));
        // Repeated extends keep the original anchor — rows are never lost.
        panel.entries.push(entry(4, "D", "d.dwg"));
        panel.entries.push(entry(5, "E", "e.dwg"));
        panel.click_select(4, SelectExtend::Range);
        assert_eq!(panel.selected, HashSet::from([2, 3, 4]));
        assert_eq!(panel.anchor, Some(2));
        // Ctrl-click toggles one row.
        panel.click_select(3, SelectExtend::Toggle);
        assert_eq!(panel.selected, HashSet::from([2, 4]));
        // Range with no anchor starts at the clicked row.
        let mut fresh = XrefManagerPanel::default();
        fresh.entries = vec![entry(1, "A", "a.dwg"), entry(2, "B", "b.dwg")];
        fresh.click_select(1, SelectExtend::Range);
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

    fn preview_doc2(dir: &std::path::Path) -> acadrust::CadDocument {
        // Two referenced images → genuine multi-select.
        use acadrust::objects::{ImageDefinition, ObjectType};
        let mut doc = acadrust::CadDocument::new();
        for (i, name) in ["a.png", "b.png"].iter().enumerate() {
            let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]));
            img.save(dir.join(name)).unwrap();
            let path = dir.join(name).to_string_lossy().into_owned();
            let h = doc.allocate_handle();
            let mut def = ImageDefinition::with_dimensions(path.clone(), 8, 8);
            def.handle = h;
            doc.objects.insert(h, ObjectType::ImageDefinition(def));
            let mut ent = acadrust::entities::RasterImage::new(
                &path,
                acadrust::types::Vector3::ZERO,
                8.0 + i as f64,
                8.0,
            );
            ent.definition_handle = Some(h);
            doc.add_entity(acadrust::EntityType::RasterImage(ent)).unwrap();
        }
        doc
    }

    fn preview_doc(dir: &std::path::Path, name: &str) -> acadrust::CadDocument {
        // Referenced 8x8 PNG under `dir`; the entry resolves Loaded.
        use acadrust::objects::{ImageDefinition, ObjectType};
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]));
        img.save(dir.join(name)).unwrap();
        let img_path = dir.join(name).to_string_lossy().into_owned();
        let mut doc = acadrust::CadDocument::new();
        let h = doc.allocate_handle();
        let mut def = ImageDefinition::with_dimensions(img_path.clone(), 8, 8);
        def.handle = h;
        doc.objects.insert(h, ObjectType::ImageDefinition(def));
        let mut ent = acadrust::entities::RasterImage::new(
            &img_path,
            acadrust::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        ent.definition_handle = Some(h);
        doc.add_entity(acadrust::EntityType::RasterImage(ent)).unwrap();
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
        assert!(panel.previews.is_empty(), "nothing selected → no decode");
        panel.toggle_select(0);
        panel.refresh(&doc, &dir, &empty, &no_prev, "host", "");
        assert_eq!(panel.previews.len(), 1, "single anchor decodes once");
        let (handle, _) = panel.previews.iter().next().unwrap();
        assert!(panel.entries.iter().any(|e| e.key == handle.0));
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
        let mut doc = acadrust::CadDocument::new();
        let mut br = acadrust::tables::BlockRecord::new("PLAN");
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
                acadrust::objects::ObjectType::ImageDefinition(_) => Some(h.value()),
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
        let mut ui = iced_test::simulator(iced_aw::ContextMenu::new(
            text("anchor"),
            move || row_menu_for(0, change_path_open),
        ));
        let anchor = ui.find("anchor").expect("anchor underlay present");
        let center = anchor
            .visible_bounds()
            .expect("anchor visible")
            .center();
        ui.point_at(center);
        ui.simulate([iced_core::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Right,
        ))]);
        ui
    }

    fn published_ops(ui: MenuSimulator) -> Vec<(usize, XrefPaletteOp)> {
        let mut ops = Vec::new();
        for message in ui.into_messages() {
            if let Message::XrefRowOp(index, op) = message {
                ops.push((index, op));
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
            published_ops(ui).contains(&(0, XrefPaletteOp::Unload)),
            "clicking Unload must publish XrefRowOp(0, Unload)"
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
        let center = target
            .visible_bounds()
            .expect("row visible")
            .center();
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
            published_ops(ui).contains(&(0, XrefPaletteOp::Pathtype(Pathtype::Full))),
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

        let mut menu = iced_aw::ContextMenu::new(text("anchor"), || row_menu_for(0, false));
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
