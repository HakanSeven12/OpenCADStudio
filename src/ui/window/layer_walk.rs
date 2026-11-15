//! LayerWalk pallet (LAYWALK) — click through layers, see only their objects.
//!
//! The walk is a session-only visibility override (`Scene::layer_walk`): the
//! layer table is never touched while walking, and closing the pallet either
//! restores (the default) or commits the walked visibility as one undo step.
//! This file holds the pallet's state, its pure selection / filter / menu
//! logic (unit-tested below) and its view; everything that reaches into the
//! drawing lives in `app::layer_walk`.
//!
//! Rows are name-keyed, like the Layer Manager's edit state (#22): a
//! layer-table rebuild between a click and the next view pass cannot retarget
//! a row through a stale index.

use crate::app::Message;
use crate::io::xref_model::wildcard_match;
use crate::t;
use crate::ui::kit;
use crate::ui::style::text as ui_text;
use crate::ui::style::{chrome, kit_metrics as km};
use document::tables::layer::Layer as DocLayer;
use document::tables::Table;
use iced::widget::{
    button, checkbox, column, combo_box, container, mouse_area, row, scrollable, text,
};
use iced::Padding;
use iced::{Background, Element, Fill, Length, Theme};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::rc::Rc;

/// Icon size inside a walk row (mirrors the Layer Manager table).
/// Height of a walk row: the palette grid's.
const ROW_H: f32 = km::ROW_H;

// ── Messages ─────────────────────────────────────────────────────────────

/// What the walk pallet can ask the app to do. Interaction-heavy variants
/// (picking, filtering, walking) are handled in `app::layer_walk`.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerWalkMsg {
    /// Start dragging the divider after a column: 0 the name, 1 the count.
    ColumnGrab(usize),
    /// A row was pressed. Iced's `mouse_area` carries no modifier state, so
    /// the view bakes in the app's live Ctrl/Shift tracking (#1664).
    RowClick {
        name: String,
        ctrl: bool,
        shift: bool,
    },
    /// A double click on a row: toggle Always Show (hold).
    RowDoubleClick {
        name: String,
        ctrl: bool,
        shift: bool,
    },
    /// The row's pin: toggle Always Show (hold) on that one layer.
    ToggleHold {
        name: String,
    },
    /// The pointer entered or left a row. Hover owns the row's tint and the
    /// unheld pin, so the list shows one marked row instead of a speck per row.
    RowHover {
        name: String,
        on: bool,
    },
    /// The pointer was released over a row: the end of a click-drag range.
    RowRelease {
        name: String,
    },
    /// Up/Down with the list focused: move the selection one layer.
    KeyNav {
        up: bool,
    },
    /// The filter box is being typed in (nothing happens until Enter).
    FilterEdit(String),
    /// Enter in the filter box: the pattern takes effect and matching layers
    /// are highlighted. An empty box clears the filter again.
    FilterApply,
    /// Dismisses the filter: the list and the highlight return to all layers.
    FilterClear,
    /// The Filter checkbox: show only the layers matching the pattern.
    FilterOnlyMatch(bool),
    /// A saved filter picked from the dropdown.
    SavedFilterPick(String),
    /// Sort the list; clicking the active column again flips the direction.
    Sort(LayerWalkSort),
    MenuItem(LayerWalkMenuItem),
    ToggleRestoreOnExit(bool),
    ToggleLayer0AlwaysOn(bool),
    SelectObjects,
    Purge,
    /// The inline purge-confirmation dialog.
    PurgeConfirm(bool),
    Inspect,
    InspectClose,
    KeepResult,
    Close,
}

/// Menu rows, in the order the context menu shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerWalkMenuItem {
    HoldSelection,
    ReleaseSelection,
    ReleaseAll,
    SelectAll,
    ClearAll,
    Invert,
    SelectUnreferenced,
    SaveState,
    Inspect,
    CopyAsFilter,
    SaveFilter,
    DeleteFilter,
    ZoomExtents,
    SelectObjectsOnLayer,
}

/// What a context-menu row does, once the enable rules are resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuRow {
    Item(LayerWalkMenuItem),
    Separator,
    /// A row shown greyed with its reason (AutoCAD greys these rather than
    /// hiding them, so the menu keeps its shape).
    Disabled(LayerWalkMenuItem, Cow<'static, str>),
}

/// Sortable list columns (the walk keeps it to what its rows show).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayerWalkSort {
    #[default]
    Name,
    Count,
}

// ── Session state ─────────────────────────────────────────────────────────

/// The walk pallet's per-tab session.
#[derive(Clone, Debug, Default)]
pub struct LayerWalkSession {
    pub selected: Vec<String>,
    /// Always-Show (held) layers: visible regardless of the selection.
    pub held: Vec<String>,
    /// Shift-range and keyboard anchor row (index into `rows`).
    pub anchor: Option<usize>,
    /// Row a press landed on, for click-drag range selection.
    pub drag_start: Option<usize>,
    /// Filter box contents (live text; inert until `pattern` is set).
    pub filter_buf: String,
    /// The applied wildcard pattern (`Some` ⇔ a filter is active).
    pub pattern: Option<String>,
    /// Active saved filter name (the dropdown's selection).
    pub saved_active: Option<String>,
    pub show_only_matching: bool,
    pub restore_on_exit: bool,
    pub layer0_always_on: bool,
    /// The row under the pointer, for the hover tint and the unheld pin. Only
    /// ever set from a row, so a stale name simply stops matching in the view.
    pub hovered: Option<String>,
    pub sort: LayerWalkSort,
    pub sort_asc: bool,
    /// True while the list owns the arrow keys (set by a row click).
    pub list_focused: bool,
    /// The inline purge confirmation, while it is showing.
    pub purge_confirm: bool,
    /// The inline inspect dialog, while it is showing.
    pub inspect: bool,
    /// Kept for the app: the drawing space the override is installed under.
    pub space: Option<u64>,
}

impl LayerWalkSession {
    /// A freshly opened walk: every layer visible in the drawing is selected,
    /// restore-on-exit is on, Layer 0 pin is off.
    pub fn fresh(rows: &[LayerWalkRow]) -> Self {
        Self {
            selected: rows
                .iter()
                .filter(|row| row.underlying_shown())
                .map(|row| row.name.clone())
                .collect(),
            held: Vec::new(),
            anchor: None,
            drag_start: None,
            filter_buf: String::new(),
            pattern: None,
            saved_active: None,
            show_only_matching: false,
            restore_on_exit: true,
            layer0_always_on: false,
            hovered: None,
            sort: LayerWalkSort::Name,
            sort_asc: true,
            list_focused: false,
            purge_confirm: false,
            inspect: false,
            space: None,
        }
    }
}

// ── Row model ─────────────────────────────────────────────────────────────

/// One list row. `count` is every object in the drawing on the layer —
/// PURGE's reference view, block-definition geometry included.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerWalkRow {
    pub name: String,
    /// The layer table record, so the walk set can be built without a lookup.
    pub handle: document::Handle,
    pub color: document::types::Color,
    pub count: usize,
    /// Removable by PURGE's rules if selected.
    pub purged: bool,
    /// No object in the drawing references the layer.
    pub unreferenced: bool,
    /// Off / frozen / locked in the underlying drawing.
    pub off: bool,
    pub frozen: bool,
    pub locked: bool,
}

impl LayerWalkRow {
    /// Visible in the drawing itself, before the walk overrides anything.
    pub fn underlying_shown(&self) -> bool {
        !self.off && !self.frozen
    }
}

/// Layers the walk can show: the Layer Manager's exclusion rule, unchanged.
pub fn walk_eligible(doc_layers: &Table<DocLayer>) -> Vec<&DocLayer> {
    doc_layers
        .iter()
        .filter(|l| !l.name.starts_with('*'))
        .collect()
}

/// Build the rows from the eligible layers. `counts` and `used` are keyed by
/// the uppercased layer name and computed once by the app (`app::layer_walk`).
pub fn sync_rows(
    doc_layers: &[&DocLayer],
    counts: &FxHashMap<String, usize>,
    used: &FxHashSet<String>,
    current_layer: &str,
    sort: LayerWalkSort,
    sort_asc: bool,
) -> Vec<LayerWalkRow> {
    let current = current_layer.to_uppercase();
    let mut rows: Vec<LayerWalkRow> = doc_layers
        .iter()
        .map(|l| {
            let key = l.name.to_uppercase();
            let count = counts.get(&key).copied().unwrap_or(0);
            LayerWalkRow {
                color: l.color,
                handle: l.handle,
                purged: !used.contains(&key)
                    && !l.flags.xref_dependent
                    && key != "0"
                    && key != current
                    && !key.contains('|'),
                unreferenced: !used.contains(&key),
                off: l.flags.off,
                frozen: l.flags.frozen,
                locked: l.flags.locked,
                name: l.name.clone(),
                count,
            }
        })
        .collect();
    sort_rows(&mut rows, sort, sort_asc);
    rows
}

fn sort_rows(rows: &mut [LayerWalkRow], sort: LayerWalkSort, asc: bool) {
    rows.sort_by(|a, b| {
        let ord = match sort {
            LayerWalkSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            LayerWalkSort::Count => a
                .count
                .cmp(&b.count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        };
        if asc {
            ord
        } else {
            ord.reverse()
        }
    });
}

// ── Pure logic ────────────────────────────────────────────────────────────

/// Row indices of the layers matching the active pattern.
pub fn matching_indices(rows: &[LayerWalkRow], pattern: Option<&str>) -> Vec<usize> {
    let Some(pattern) = pattern else {
        return vec![];
    };
    rows.iter()
        .enumerate()
        .filter(|(_, r)| wildcard_match(&r.name, pattern))
        .map(|(i, _)| i)
        .collect()
}

/// Rows the list actually draws: everything, or only the matching layers
/// while the Filter checkbox is ticked.
pub fn visible_rows<'a>(
    rows: &'a [LayerWalkRow],
    session: &LayerWalkSession,
) -> Vec<(usize, &'a LayerWalkRow)> {
    let order: Vec<usize> = if session.show_only_matching && session.pattern.is_some() {
        matching_indices(rows, session.pattern.as_deref())
    } else {
        (0..rows.len()).collect()
    };
    order.into_iter().map(|i| (i, &rows[i])).collect()
}

fn index_of(rows: &[LayerWalkRow], name: &str) -> Option<usize> {
    rows.iter().position(|r| r.name == name)
}

/// The drawn rows between two row indices of the full list, inclusive and in
/// display order — so a range never sweeps in rows the filter hides (#50).
pub fn range_between(
    rows: &[LayerWalkRow],
    session: &LayerWalkSession,
    a: usize,
    b: usize,
) -> Vec<String> {
    let shown: Vec<usize> = visible_rows(rows, session)
        .into_iter()
        .map(|(i, _)| i)
        .collect();
    let (Some(pa), Some(pb)) = (
        shown.iter().position(|&i| i == a),
        shown.iter().position(|&i| i == b),
    ) else {
        return vec![];
    };
    let (lo, hi) = if pa <= pb { (pa, pb) } else { (pb, pa) };
    shown[lo..=hi]
        .iter()
        .filter_map(|&i| rows.get(i).map(|r| r.name.clone()))
        .collect()
}

/// Convert one click into the new selection. Plain replaces, Ctrl toggles,
/// Shift ranges from the anchor.
pub fn click_select(
    rows: &[LayerWalkRow],
    session: &mut LayerWalkSession,
    name: &str,
    ctrl: bool,
    shift: bool,
) {
    let Some(index) = index_of(rows, name) else {
        return;
    };
    if ctrl {
        let mut set: FxHashSet<usize> = indices_of(rows, &session.selected).into_iter().collect();
        if !set.remove(&index) {
            set.insert(index);
        }
        let mut kept: Vec<usize> = set.into_iter().collect();
        kept.sort_unstable();
        session.selected = names_at(rows, &kept);
        session.anchor = Some(index);
        session.drag_start = Some(index);
    } else if shift && session.anchor.is_some() {
        let anchor = session.anchor.unwrap_or(index);
        session.selected = range_between(rows, session, anchor, index);
    } else {
        session.selected = vec![name.to_string()];
        session.anchor = Some(index);
        session.drag_start = Some(index);
    }
}

fn indices_of(rows: &[LayerWalkRow], names: &[String]) -> Vec<usize> {
    names
        .iter()
        .filter_map(|name| index_of(rows, name))
        .collect()
}

fn names_at(rows: &[LayerWalkRow], indices: &[usize]) -> Vec<String> {
    indices
        .iter()
        .filter_map(|&i| rows.get(i).map(|r| r.name.clone()))
        .collect()
}

/// Up/Down: move the anchor and the selection one drawn row at a time.
pub fn key_navigate(rows: &[LayerWalkRow], session: &mut LayerWalkSession, up: bool) -> Option<()> {
    let shown: Vec<usize> = visible_rows(rows, session)
        .into_iter()
        .map(|(i, _)| i)
        .collect();
    if shown.is_empty() {
        return None;
    }
    let current = session
        .anchor
        .filter(|i| shown.contains(i))
        .or_else(|| indices_of(rows, &session.selected).into_iter().next())
        .or_else(|| indices_of(rows, &session.selected).into_iter().next())?;
    let pos = shown.iter().position(|&i| i == current).unwrap_or(0);
    let next = if up {
        pos.checked_sub(1)?
    } else {
        (pos + 1).min(shown.len() - 1)
    };
    let index = shown[next];
    session.anchor = Some(index);
    session.drag_start = Some(index);
    session.selected = vec![rows[index].name.clone()];
    Some(())
}

/// Click-drag end: releasing over another row takes the dragged range.
pub fn drag_select(rows: &[LayerWalkRow], session: &mut LayerWalkSession, end_index: usize) {
    let Some(start) = session.drag_start else {
        return;
    };
    if end_index == start {
        return;
    }
    session.selected = range_between(rows, session, start, end_index);
    session.anchor = Some(end_index);
}

/// Visible layers = selected ∪ held, plus layer 0 while its pin is on. Nothing
/// else is protected: the current layer is a row like any other, so a walk can
/// hide it — which is what makes the pin the only guard worth honouring.
pub fn visible_set(session: &LayerWalkSession) -> FxHashSet<String> {
    let mut set: FxHashSet<String> = session.selected.iter().cloned().collect();
    set.extend(session.held.iter().cloned());
    if session.layer0_always_on {
        set.insert("0".to_string());
    }
    set
}

/// How many of the rows the walk actually draws are on screen — the pallet's
/// own status line, so "Clear All" has a visible result.
pub fn visible_counts(rows: &[LayerWalkRow], session: &LayerWalkSession) -> (usize, usize) {
    let visible = visible_set(session);
    let drawn = visible_rows(rows, session);
    let shown = drawn
        .iter()
        .filter(|(_, row)| visible.contains(&row.name))
        .count();
    (shown, drawn.len())
}

/// The walk set as table handles — what `Scene::set_space_layer_walk` keys on.
/// A name with no row (deleted while the pallet was open) simply drops out.
pub fn visible_handles(
    rows: &[LayerWalkRow],
    session: &LayerWalkSession,
) -> FxHashSet<document::Handle> {
    let names = visible_set(session);
    rows.iter()
        .filter(|r| names.contains(&r.name))
        .map(|r| r.handle)
        .collect()
}

/// The walk is worth committing when its visible set differs from the
/// drawing's own layer visibility.
pub fn walk_changes_underlying(rows: &[LayerWalkRow], session: &LayerWalkSession) -> bool {
    let visible = visible_set(session);
    rows.iter()
        .any(|r| r.underlying_shown() != visible.contains(&r.name))
}

/// Objects on the selected (or, with `use_all`, every) layer.
pub fn object_count(rows: &[LayerWalkRow], names: &FxHashSet<String>) -> usize {
    rows.iter()
        .filter(|r| names.contains(&r.name))
        .map(|r| r.count)
        .sum()
}

/// Menu rows in the fixed LAYWALK order, with the enable rules resolved.
pub fn menu_rows(
    rows: &[LayerWalkRow],
    session: &LayerWalkSession,
    saved_names: &[String],
) -> Vec<MenuRow> {
    let any_sel = !session.selected.is_empty();
    let sel_held = session
        .selected
        .iter()
        .any(|name| session.held.iter().any(|h| h == name));
    let any_held = !session.held.is_empty();
    let unreferenced = rows.iter().any(|r| r.unreferenced);
    let pattern_active = session.pattern.is_some();
    let saved_active = session
        .saved_active
        .as_deref()
        .is_some_and(|name| saved_names.iter().any(|s| s == name));

    let gate = |item: LayerWalkMenuItem, ok: bool, reason: &'static str| {
        if ok {
            MenuRow::Item(item)
        } else {
            MenuRow::Disabled(item, t!(reason))
        }
    };
    vec![
        gate(
            LayerWalkMenuItem::HoldSelection,
            any_sel,
            "Laywalk: select at least one layer first.",
        ),
        gate(
            LayerWalkMenuItem::ReleaseSelection,
            any_sel && sel_held,
            "Laywalk: none of the selected layers is held.",
        ),
        gate(
            LayerWalkMenuItem::ReleaseAll,
            any_held,
            "Laywalk: no layer is held.",
        ),
        MenuRow::Separator,
        MenuRow::Item(LayerWalkMenuItem::SelectAll),
        MenuRow::Item(LayerWalkMenuItem::ClearAll),
        MenuRow::Item(LayerWalkMenuItem::Invert),
        gate(
            LayerWalkMenuItem::SelectUnreferenced,
            unreferenced,
            "Laywalk: every layer is referenced.",
        ),
        MenuRow::Separator,
        gate(
            LayerWalkMenuItem::SaveState,
            any_sel,
            "Laywalk: select the layers the state should keep.",
        ),
        MenuRow::Item(LayerWalkMenuItem::Inspect),
        MenuRow::Separator,
        gate(
            LayerWalkMenuItem::CopyAsFilter,
            session.selected.len() == 1,
            "Laywalk: select exactly one layer to copy its name.",
        ),
        gate(
            LayerWalkMenuItem::SaveFilter,
            pattern_active,
            "Laywalk: apply a filter first.",
        ),
        gate(
            LayerWalkMenuItem::DeleteFilter,
            saved_active,
            "Laywalk: the filter box does not hold a saved filter.",
        ),
        MenuRow::Separator,
        gate(
            LayerWalkMenuItem::ZoomExtents,
            any_sel,
            "Laywalk: select at least one layer first.",
        ),
        gate(
            LayerWalkMenuItem::SelectObjectsOnLayer,
            any_sel,
            "Laywalk: select at least one layer first.",
        ),
    ]
}

/// Hold/release bookkeeping (held layers stay a name-keyed list).
pub fn set_held(session: &mut LayerWalkSession, names: &[String], on: bool) {
    for name in names {
        let held = session.held.iter().any(|h| h == name);
        if on && !held {
            session.held.push(name.clone());
        } else if !on {
            session.held.retain(|h| h != name);
        }
    }
}

/// A rendered menu line: the enable rules already resolved into labels.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item(Cow<'static, str>, LayerWalkMenuItem),
    Separator,
    Disabled(Cow<'static, str>, Cow<'static, str>),
}

/// `menu_rows` with the labels resolved, ready to clone into every row's
/// `iced_aw::ContextMenu` closure (the menu never depends on its row).
pub fn menu_entries(
    rows: &[LayerWalkRow],
    session: &LayerWalkSession,
    saved_names: &[String],
) -> Vec<MenuEntry> {
    menu_rows(rows, session, saved_names)
        .into_iter()
        .map(|row| match row {
            MenuRow::Separator => MenuEntry::Separator,
            MenuRow::Item(item) => MenuEntry::Item(menu_label(item.clone()), item),
            MenuRow::Disabled(item, reason) => MenuEntry::Disabled(menu_label(item), reason),
        })
        .collect()
}

fn menu_label(item: LayerWalkMenuItem) -> Cow<'static, str> {
    match item {
        LayerWalkMenuItem::HoldSelection => crate::tr!("layer-walk", "hold-selection").into(),
        LayerWalkMenuItem::ReleaseSelection => crate::tr!("layer-walk", "release-selection").into(),
        LayerWalkMenuItem::ReleaseAll => crate::tr!("layer-walk", "release-all").into(),
        LayerWalkMenuItem::SelectAll => t!("Select All"),
        LayerWalkMenuItem::ClearAll => t!("Clear All"),
        LayerWalkMenuItem::Invert => t!("Invert Selection"),
        LayerWalkMenuItem::SelectUnreferenced => {
            crate::tr!("layer-walk", "select-unreferenced").into()
        }
        LayerWalkMenuItem::SaveState => crate::tr!("layer-walk", "save-state").into(),
        LayerWalkMenuItem::Inspect => crate::tr!("layer-walk", "inspect").into(),
        LayerWalkMenuItem::CopyAsFilter => crate::tr!("layer-walk", "copy-as-filter").into(),
        LayerWalkMenuItem::SaveFilter => crate::tr!("layer-walk", "save-filter").into(),
        LayerWalkMenuItem::DeleteFilter => crate::tr!("layer-walk", "delete-filter").into(),
        LayerWalkMenuItem::ZoomExtents => crate::tr!("layer-walk", "zoom-extents").into(),
        LayerWalkMenuItem::SelectObjectsOnLayer => {
            crate::tr!("layer-walk", "select-objects-on-layer").into()
        }
    }
}

// ── View ──────────────────────────────────────────────────────────────────

fn msg(m: LayerWalkMsg) -> Message {
    Message::Laywalk(m)
}

/// Scrollable id, so the app can focus the list for arrow navigation.
pub const LIST_ID: &str = "layer-walk-list";

/// Column widths: the swatch, the hold pin and the state cluster are fixed;
/// the name and the count take the widths the user drags them to.
/// The state column is exactly one effective eye plus the two underlying marks
/// it can carry, so it never leaves dead width next to the name.
const COL_SWATCH: f32 = 22.0;
const COL_PIN: f32 = 22.0;
const COL_STATE: f32 = 36.0;
/// How much a row the walk hides loses: alpha carries the signal because the
/// bundled UI font has no italic cut.
const HIDDEN_ALPHA: f32 = 0.38;
/// How strongly a row matching the filter is tinted.
const MATCH_ALPHA: f32 = 0.30;
/// A row that is off or frozen in the drawing is only muted: the walk keeps it
/// out of the eye's way, but it cannot switch such a layer on.
const UNDERLYING_ALPHA: f32 = 0.55;
/// The narrowest a column can be dragged.
pub const COL_MIN_W: f32 = 24.0;
/// The name and object-count columns' widths until the user sizes them.
pub const DEFAULT_COL_W: [f32; 2] = [240.0, 48.0];

/// The whole pallet: chrome title, header, list, inline dialogs and footer.
/// The pallet stays non-modal, so its dialogs are drawn inside it rather than
/// floating over the drawing.
pub fn view<'a>(
    rows: &'a [LayerWalkRow],
    session: &'a LayerWalkSession,
    filter_combo: &'a combo_box::State<String>,
    saved_filters: &'a [String],
    total_layers: usize,
    ctrl: bool,
    shift: bool,
    col_w: [f32; 2],
    width: f32,
    chrome: crate::ui::dock::Chrome,
) -> Element<'a, Message> {
    // The name column keeps its width while the palette has room for it and
    // the fixed columns, and gives way when it does not.
    let fixed = COL_SWATCH + col_w[1] + COL_PIN + COL_STATE + 4.0 * km::TOOL_GAP;
    let room = width - km::GRID_INSET_L - km::GRID_INSET_R - 2.0 * km::CELL_PAD_X - fixed;
    let col_w = [col_w[0].min(room).max(COL_MIN_W), col_w[1]];
    let body = container(view_content(
        rows,
        session,
        filter_combo,
        saved_filters,
        total_layers,
        ctrl,
        shift,
        col_w,
    ))
    .width(Length::Fixed(width))
    .height(Fill);
    kit::palette(
        crate::ui::dock::title_bar(
            crate::ui::dock::PanelId::Laywalk,
            crate::tr!("layer-walk", "title", count = total_layers),
            chrome,
        ),
        body,
        width,
    )
}

#[allow(clippy::too_many_arguments)]
fn view_content<'a>(
    rows: &'a [LayerWalkRow],
    session: &'a LayerWalkSession,
    filter_combo: &'a combo_box::State<String>,
    saved_filters: &'a [String],
    total_layers: usize,
    ctrl: bool,
    shift: bool,
    col_w: [f32; 2],
) -> Element<'a, Message> {
    // ── Toolbar ───────────────────────────────────────────────────────────
    let select_btn = kit::tool_button(
        kit::tool_icon(crate::ui::icons::PREVIEW),
        crate::tr!("layer-walk", "select-objects-tip"),
        Some(msg(LayerWalkMsg::SelectObjects)),
        false,
    );
    let filter_combo_box: Element<'_, Message> = if saved_filters.is_empty() {
        container(kit::select_box(kit::value_text(
            crate::tr!("layer-walk", "saved-filters"),
            true,
        )))
        .width(Length::Fixed(km::NAME_COL_W))
        .into()
    } else {
        combo_box(
            filter_combo,
            crate::tr!("layer-walk", "saved-filters").as_ref(),
            session.saved_active.as_ref(),
            |name: String| msg(LayerWalkMsg::SavedFilterPick(name)),
        )
        .size(ui_text::UI)
        .line_height(ui_text::line())
        .padding(kit::BOX_PAD)
        .width(Length::Fixed(km::NAME_COL_W))
        .input_style(kit::combo_style)
        .menu_style(kit::pick_menu_style)
        .into()
    };
    let filter_box = kit::search_field(
        crate::tr!("layer-walk", "pattern-placeholder").as_ref(),
        &session.filter_buf,
        |s| msg(LayerWalkMsg::FilterEdit(s)),
    )
    .on_submit(msg(LayerWalkMsg::FilterApply));
    // The Filter checkbox only does something while a pattern is active; with
    // no toggle it renders in the theme's disabled state, which is the mute.
    let filter_toggle: Option<Box<dyn Fn(bool) -> Message>> =
        session.pattern.is_some().then(|| {
            Box::new(|on: bool| msg(LayerWalkMsg::FilterOnlyMatch(on)))
                as Box<dyn Fn(bool) -> Message>
        });
    let filter_check: Element<'_, Message> = checkbox(session.show_only_matching)
        .label(t!("Filter"))
        .on_toggle_maybe(filter_toggle)
        .size(km::CHECK)
        .text_size(ui_text::UI)
        .into();
    // An applied filter is the only thing worth clearing: an empty box is
    // already inert, so the ✕ shows up exactly when it does something.
    let clear_filter: Element<'_, Message> = if session.pattern.is_some() {
        kit::tool_button(
            kit::tool_icon(crate::ui::icons::CLOSE),
            crate::tr!("layer-walk", "clear-filter"),
            Some(msg(LayerWalkMsg::FilterClear)),
            false,
        )
    } else {
        iced::widget::Space::new().width(Length::Fixed(0.0)).into()
    };
    let action = |label: Cow<'static, str>, item: LayerWalkMenuItem| -> Element<'_, Message> {
        kit::text_button(label, Some(msg(LayerWalkMsg::MenuItem(item))), false)
    };
    // The running total is the one number the walk cannot be read without:
    // how much of the drawing is on screen right now.
    let (shown, drawn) = visible_counts(rows, session);
    let status = kit::value_text(
        crate::tr!(
            "layer-walk",
            "visible-count",
            visible = shown,
            total = drawn
        ),
        true,
    );
    // Two lines, not eight controls on one: at 420 px the fields had no
    // visible edge and every button was a guess.
    let header = column![
        kit::band(
            row![select_btn, filter_combo_box, filter_box, clear_filter]
                .spacing(km::TOOL_GAP)
                .align_y(iced::Center),
        ),
        kit::band(
            row![
                filter_check,
                action(t!("Select All"), LayerWalkMenuItem::SelectAll),
                action(t!("Clear All"), LayerWalkMenuItem::ClearAll),
                action(
                    crate::tr!("layer-walk", "invert").into(),
                    LayerWalkMenuItem::Invert
                ),
                iced::widget::Space::new().width(Fill),
                status,
            ]
            .spacing(km::TOOL_GAP)
            .align_y(iced::Center),
        ),
    ];

    // ── Column header ─────────────────────────────────────────────────────
    // `right` mirrors the cell: the count is right-aligned, so its header is
    // too, or the two edges never line up.
    let sort_head = |label: Cow<'static, str>, col: LayerWalkSort, right: bool| {
        let title = text(label)
            .size(ui_text::UI)
            .line_height(ui_text::line())
            .font(kit::SEMIBOLD)
            .wrapping(iced::widget::text::Wrapping::None);
        let mut content = if right {
            row![iced::widget::Space::new().width(Fill), title]
        } else {
            row![title]
        }
        .spacing(km::TOOL_GAP)
        .align_y(iced::Center);
        if session.sort == col {
            content = content.push(crate::ui::icons::themed_arrow_toggle(
                session.sort_asc,
                km::CARET,
            ));
        }
        let head: Element<'_, Message> = button(container(content).clip(true).center_y(Fill))
            .on_press(msg(LayerWalkMsg::Sort(col)))
            .style(|theme: &Theme, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then(|| Background::Color(chrome::hover(theme))),
                text_color: chrome::text(theme),
                ..Default::default()
            })
            .padding(Padding::ZERO.right(km::CELL_PAD_X))
            .width(Fill)
            .height(Fill)
            .into();
        head
    };
    // Each column's divider sizes it, as in the Layer Manager.
    let header_cell = |head: Element<'a, Message>, width: f32, grab: usize| {
        container(
            row![
                head,
                kit::column_divider(Some(msg(LayerWalkMsg::ColumnGrab(grab))))
            ]
            .height(Fill),
        )
        .width(Length::Fixed(width))
        .height(Length::Fixed(km::ROW_H))
    };
    let header_grid = container(
        row![
            header_cell(
                sort_head(t!("Name"), LayerWalkSort::Name, false),
                col_w[0],
                0
            ),
            iced::widget::Space::new().width(Length::Fixed(COL_SWATCH)),
            header_cell(
                sort_head(t!("Objects"), LayerWalkSort::Count, true),
                col_w[1],
                1
            ),
        ]
        .spacing(km::TOOL_GAP)
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(chrome::surface_raised(theme))),
        ..Default::default()
    })
    .height(Length::Fixed(km::ROW_H))
    .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
    .width(Fill);

    // ── Rows ──────────────────────────────────────────────────────────────
    let sel: FxHashSet<&str> = session.selected.iter().map(String::as_str).collect();
    let held: FxHashSet<&str> = session.held.iter().map(String::as_str).collect();
    let visible = visible_set(session);
    let matches: FxHashSet<usize> = matching_indices(rows, session.pattern.as_deref())
        .into_iter()
        .collect();
    let entries = Rc::new(menu_entries(rows, session, saved_filters));
    let drawn = visible_rows(rows, session);
    let list: Element<'_, Message> = if drawn.is_empty() {
        kit::empty_state(if session.pattern.is_some() {
            crate::tr!("layer-walk", "no-matches")
        } else {
            crate::tr!("layer-walk", "no-layers")
        })
    } else {
        let mut col = column![].spacing(0);
        for (index, row) in drawn.iter() {
            col = col.push(walk_row(
                row,
                sel.contains(row.name.as_str()),
                held.contains(row.name.as_str()),
                visible.contains(&row.name),
                session.hovered.as_deref() == Some(row.name.as_str()),
                matches.contains(index),
                ctrl,
                shift,
                col_w,
                Rc::clone(&entries),
            ));
        }
        scrollable(col)
            .id(iced::advanced::widget::Id::new(LIST_ID))
            .style(kit::scroll_style)
            .width(Fill)
            .height(Fill)
            .into()
    };

    // ── Footer ────────────────────────────────────────────────────────────
    let any_purged = rows
        .iter()
        .any(|r| r.purged && sel.contains(r.name.as_str()));
    let footer = row![
        footer_btn(
            crate::ui::icons::TRASH,
            t!("Purge"),
            LayerWalkMsg::Purge,
            any_purged,
        ),
        iced::widget::Space::new().width(Fill),
        footer_check(
            crate::tr!("layer-walk", "restore-on-exit"),
            session.restore_on_exit,
            LayerWalkMsg::ToggleRestoreOnExit,
        ),
        footer_check(
            crate::tr!("layer-walk", "layer-0-always-on"),
            session.layer0_always_on,
            LayerWalkMsg::ToggleLayer0AlwaysOn,
        ),
        footer_btn(
            crate::ui::icons::CHECK,
            crate::tr!("layer-walk", "keep-result"),
            LayerWalkMsg::KeepResult,
            walk_changes_underlying(rows, session),
        ),
        footer_btn(
            crate::ui::icons::CLOSE,
            t!("Close"),
            LayerWalkMsg::Close,
            true
        ),
    ]
    .spacing(km::TOOL_GAP)
    .align_y(iced::Center);

    // The inline dialogs sit between the column header and the list: the
    // pallet is non-modal, so nothing is drawn over the drawing.
    let dialog: Option<Element<'static, Message>> = if session.purge_confirm {
        let count = rows
            .iter()
            .filter(|r| r.purged && sel.contains(r.name.as_str()))
            .count();
        Some(purge_confirm_card(count))
    } else if session.inspect {
        let selected: FxHashSet<String> = session.selected.iter().cloned().collect();
        Some(inspect_card(
            total_layers,
            session.selected.len(),
            object_count(rows, &selected),
        ))
    } else {
        None
    };

    // The list (its column header, an inline dialog, the rows) sits inside
    // the palette's insets, as the bands above it do.
    let mut grid = column![header_grid];
    if let Some(dialog) = dialog {
        grid = grid.push(
            container(dialog)
                .padding([km::TOOL_GAP, 0.0])
                .width(Fill)
                .align_x(iced::Center),
        );
    }
    let grid = container(grid.push(list).height(Fill))
        .padding(Padding {
            top: km::TOOL_GAP,
            bottom: 0.0,
            left: km::GRID_INSET_L,
            right: km::GRID_INSET_R,
        })
        .width(Fill)
        .height(Fill);
    column![
        header,
        grid,
        container(footer)
            .padding(Padding {
                top: km::TOOL_GAP,
                bottom: km::TOOL_GAP,
                left: km::GRID_INSET_L,
                right: km::GRID_INSET_R,
            })
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::surface_raised(theme))),
                ..Default::default()
            })
            .width(Fill),
    ]
    .height(Fill)
    .into()
}

/// One row: name, colour swatch, object count, hold pin and the walk's eye.
/// The body container has to ask for `align_y`: it defaults to `Start`, which
/// pins a 16 px line of content to the top edge of the 26 px band.
///
/// Alpha carries the walk state (the bundled UI font has no italic cut): a row
/// the walk hides dims, a row off or frozen in the table is only muted.
#[allow(clippy::too_many_arguments)]
fn walk_row(
    row: &LayerWalkRow,
    is_selected: bool,
    is_held: bool,
    is_visible: bool,
    is_hovered: bool,
    matches: bool,
    ctrl: bool,
    shift: bool,
    col_w: [f32; 2],
    entries: Rc<Vec<MenuEntry>>,
) -> Element<'_, Message> {
    let dim = if !is_visible {
        HIDDEN_ALPHA
    } else if row.underlying_shown() {
        1.0
    } else {
        UNDERLYING_ALPHA
    };
    // One line, cut with "…" at the cell: a long name wrapped into the
    // rows below.
    let mut label = kit::one_line(text(row.name.as_str()))
        .size(ui_text::UI)
        .line_height(crate::ui::style::text::line());
    if matches && !is_selected {
        label = label.font(iced::Font::DEFAULT.weight(iced::font::Weight::Semibold));
    }
    let name = row.name.clone();
    // The gesture areas are siblings, never nested: a `mouse_area` handles an
    // event over its whole bounds whether or not a child area also claims it,
    // so a pin inside the row area would toggle hold *and* reselect the row.
    let left = mouse_area(
        row![
            container(label.width(Fill))
                .width(Length::Fixed(col_w[0]))
                .clip(true),
            container(crate::ui::color_select::swatch(
                crate::ui::window::layers::iced_color_from_acad(&row.color).scale_alpha(dim)
            ))
            .width(Length::Fixed(COL_SWATCH))
            .align_x(iced::Center),
            kit::tooltip(
                text(row.count.to_string())
                    .size(ui_text::UI)
                    .line_height(ui_text::line())
                    .width(Length::Fixed(col_w[1]))
                    .align_x(iced::Right),
                crate::tr!("layer-walk", "count-tip", count = row.count),
                kit::TipPosition::FollowCursor,
            ),
        ]
        .spacing(km::TOOL_GAP)
        .align_y(iced::Center)
        .width(Fill),
    )
    .on_press(msg(LayerWalkMsg::RowClick {
        name: name.clone(),
        ctrl,
        shift,
    }))
    .on_double_click(msg(LayerWalkMsg::RowDoubleClick {
        name: name.clone(),
        ctrl,
        shift,
    }))
    .on_release(msg(LayerWalkMsg::RowRelease { name: name.clone() }))
    .interaction(iced::mouse::Interaction::Pointer);
    // A held row is a chip: the pin alone in grey at this size reads as a
    // smudge. An unheld row shows its pin only under the pointer, so the
    // column stays quiet until a row is actually being aimed at.
    let glyph: Element<'_, Message> = if is_held {
        container(crate::ui::icons::themed_primary(
            crate::ui::icons::PIN_ACTIVE,
            km::BAR_ICON,
        ))
        .width(Length::Fixed(COL_PIN))
        .height(Length::Fixed(km::ROW_ICON))
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::lit(theme))),
            text_color: Some(chrome::strong(theme)),
            ..Default::default()
        })
        .into()
    } else if is_hovered {
        container(crate::ui::icons::themed_secondary(
            crate::ui::icons::PIN,
            km::BAR_ICON,
        ))
        .width(Length::Fixed(COL_PIN))
        .align_x(iced::Center)
        .into()
    } else {
        iced::widget::Space::new()
            .width(Length::Fixed(COL_PIN))
            .into()
    };
    let pin = mouse_area(
        container(kit::tooltip(
            glyph,
            if is_held {
                crate::tr!("layer-walk", "pin-held-tip")
            } else {
                crate::tr!("layer-walk", "pin-hold-tip")
            },
            kit::TipPosition::FollowCursor,
        ))
        .width(Length::Fixed(COL_PIN))
        .align_x(iced::Center),
    )
    .on_press(msg(LayerWalkMsg::ToggleHold { name }))
    .interaction(iced::mouse::Interaction::Pointer);
    let cells = row![left, pin, state_cells(row, is_visible)]
        .spacing(km::TOOL_GAP)
        .align_y(iced::Center);
    let body = container(
        mouse_area(cells)
            .on_enter(msg(LayerWalkMsg::RowHover {
                name: row.name.clone(),
                on: true,
            }))
            .on_exit(msg(LayerWalkMsg::RowHover {
                name: row.name.clone(),
                on: false,
            })),
    )
    .width(Fill)
    .height(Length::Fixed(ROW_H))
    .align_y(iced::Center)
    .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
    .style(move |theme: &Theme| {
        // Hover outranks selection outranks the filter tint, as in every
        // palette list; other rows lie on the surface.
        let band = if is_hovered {
            Some(chrome::hover(theme))
        } else if is_selected {
            Some(chrome::row_selected(theme))
        } else if matches {
            Some(chrome::row_selected(theme).scale_alpha(MATCH_ALPHA))
        } else {
            None
        };
        let text_color = if is_hovered || is_selected {
            chrome::strong(theme)
        } else {
            chrome::text(theme)
        };
        container::Style {
            background: band.map(Background::Color),
            text_color: Some(text_color.scale_alpha(dim)),
            ..Default::default()
        }
    });
    // `ContextMenu` watches for the right press itself, over its underlay's
    // bounds; it does not need a `mouse_area` of its own.
    iced_aw::ContextMenu::new(body, move || row_menu((*entries).clone())).into()
}

/// The walk's own eye, then the two underlying marks it cannot change.
///
/// The eye reports what the drawing actually shows: a layer outside the walk
/// set reads closed, and so does one the walk holds while it is off or frozen
/// in the table — the override hides geometry, it does not switch layers on.
/// The tooltip names which of the two reasons applies.
fn state_cells<'a>(row: &LayerWalkRow, is_visible: bool) -> Element<'a, Message> {
    let on_screen = is_visible && row.underlying_shown();
    let tip = if on_screen {
        crate::tr!("layer-walk", "eye-shown")
    } else if !is_visible {
        crate::tr!("layer-walk", "eye-hidden-walk")
    } else {
        crate::tr!("layer-walk", "eye-hidden")
    };
    let eye: Element<'a, Message> = if on_screen {
        crate::ui::icons::themed(crate::ui::icons::layer_visible(true), km::BAR_ICON)
    } else {
        crate::ui::icons::themed_disabled(crate::ui::icons::layer_visible(false), km::BAR_ICON)
    };
    let mut state = row![kit::tooltip(eye, tip, kit::TipPosition::FollowCursor)]
        .spacing(km::LINE * 2.0)
        .align_y(iced::Center);
    if row.frozen {
        state = state.push(kit::tooltip(
            crate::ui::icons::themed_disabled(crate::ui::icons::layer_freeze(true), km::CARET),
            crate::tr!("layer-walk", "frozen-tip"),
            kit::TipPosition::FollowCursor,
        ));
    }
    if row.locked {
        state = state.push(kit::tooltip(
            crate::ui::icons::themed_disabled(crate::ui::icons::layer_lock(true), km::CARET),
            crate::tr!("layer-walk", "locked-tip"),
            kit::TipPosition::FollowCursor,
        ));
    }
    container(state.width(Length::Fixed(COL_STATE))).into()
}

// ── Context menu ──────────────────────────────────────────────────────────

/// A row's right-click menu. Built once per view pass and shared by every row
/// through an `Rc`, so the list stays cheap on a drawing with thousands of
/// layers; the entries only clone when a menu actually opens.
fn row_menu(entries: Vec<MenuEntry>) -> Element<'static, Message> {
    use crate::ui::kit::menu::Item;
    let items = entries
        .into_iter()
        .map(|entry| match entry {
            MenuEntry::Separator => Item::Separator,
            MenuEntry::Item(label, item) => Item::new(label, msg(LayerWalkMsg::MenuItem(item))),
            MenuEntry::Disabled(label, reason) => Item::disabled(label, reason),
        })
        .collect();
    crate::ui::kit::menu::view(items)
}

// ── Inline dialogs ────────────────────────────────────────────────────────

/// The purge confirmation the footer's Purge button raises.
pub fn purge_confirm_card(count: usize) -> Element<'static, Message> {
    card(column![
        kit::value_text(
            crate::tr!("layer-walk", "purge-question", count = count),
            false
        ),
        row![
            dialog_btn(t!("Yes"), LayerWalkMsg::PurgeConfirm(true), true),
            dialog_btn(t!("No"), LayerWalkMsg::PurgeConfirm(false), false),
        ]
        .spacing(km::TOOL_GAP),
    ])
}

/// The Inspect dialog: walk totals.
pub fn inspect_card(total: usize, selected: usize, objects: usize) -> Element<'static, Message> {
    card(column![
        kit::value_text(
            crate::tr!("layer-walk", "inspect-total", count = total),
            false
        ),
        kit::value_text(
            crate::tr!("layer-walk", "inspect-selected", count = selected),
            false
        ),
        kit::value_text(
            crate::tr!("layer-walk", "inspect-objects", count = objects),
            false
        ),
        dialog_btn(t!("OK"), LayerWalkMsg::InspectClose, true),
    ])
}

/// Width of an inline dialog.
const CARD_W: f32 = 300.0;

/// An inline dialog: the kit menu's frame around its lines.
fn card(body: iced::widget::Column<'static, Message>) -> Element<'static, Message> {
    container(body.spacing(km::TOOL_GAP))
        .padding(km::GRID_INSET_L)
        .width(Length::Fixed(CARD_W))
        .style(crate::ui::kit::menu::frame_style)
        .into()
}

fn dialog_btn<'a>(
    label: Cow<'static, str>,
    message: LayerWalkMsg,
    primary: bool,
) -> Element<'a, Message> {
    kit::text_button(label, Some(msg(message)), primary)
}

// ── Footer controls ───────────────────────────────────────────────────────

fn footer_check<'a>(
    label: impl Into<Cow<'static, str>>,
    checked: bool,
    m: fn(bool) -> LayerWalkMsg,
) -> Element<'a, Message> {
    let label: Cow<'static, str> = label.into();
    checkbox(checked)
        .label(label)
        .on_toggle(move |on| msg(m(on)))
        .size(km::CHECK)
        .text_size(ui_text::UI)
        .into()
}

/// A footer button: the kit's flat button with an icon and its label.
fn footer_btn<'a>(
    icon: &'static [u8],
    label: impl Into<Cow<'static, str>>,
    message: LayerWalkMsg,
    enabled: bool,
) -> Element<'a, Message> {
    let label: Cow<'static, str> = label.into();
    button(
        row![
            if enabled {
                crate::ui::icons::themed(icon, km::ROW_ICON)
            } else {
                crate::ui::icons::themed_disabled(icon, km::ROW_ICON)
            },
            text(label).size(ui_text::UI).line_height(ui_text::line()),
        ]
        .spacing(km::TOOL_GAP)
        .align_y(iced::Center),
    )
    .on_press_maybe(enabled.then(|| msg(message)))
    .style(kit::flat_button_style(false))
    .height(Length::Fixed(km::TOOL_H))
    .padding([0.0, km::CELL_PAD_X])
    .into()
}

// ── Tests (pure logic; app-level behaviour tests live with the wiring) ──────

#[cfg(test)]
mod tests {
    use super::*;
    use document::types::Color as AcadColor;

    fn row(name: &str, count: usize) -> LayerWalkRow {
        LayerWalkRow {
            name: name.to_string(),
            handle: document::Handle::new(name.len() as u64 + 1),
            color: AcadColor::Index(7),
            count,
            purged: false,
            unreferenced: count == 0,
            off: false,
            frozen: false,
            locked: false,
        }
    }

    fn fixture() -> Vec<LayerWalkRow> {
        vec![
            row("0", 5),
            LayerWalkRow {
                name: "WALL".into(),
                count: 10,
                ..row("WALL", 10)
            },
            row("DOOR", 2),
            LayerWalkRow {
                name: "OLD".into(),
                purged: true,
                off: true,
                ..row("OLD", 0)
            },
        ]
    }

    #[test]
    fn open_selects_the_currently_visible_layers() {
        let rows = fixture();
        let s = LayerWalkSession::fresh(&rows);
        // OLD is off in the drawing, so it is not selected on open.
        assert_eq!(s.selected, vec!["0", "WALL", "DOOR"]);
        assert!(s.restore_on_exit);
        assert!(!s.layer0_always_on);
    }

    #[test]
    fn click_replaces_ctrl_toggles_shift_ranges() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        click_select(&rows, &mut s, "DOOR", false, false);
        assert_eq!(s.selected, vec!["DOOR"]);
        click_select(&rows, &mut s, "OLD", true, false);
        assert_eq!(s.selected, vec!["DOOR", "OLD"]);
        click_select(&rows, &mut s, "OLD", true, false);
        assert_eq!(s.selected, vec!["DOOR"]);
        // A plain click anchors, and Shift ranges from that anchor in drawn
        // order: WALL(1) … OLD(3).
        click_select(&rows, &mut s, "WALL", false, false);
        click_select(&rows, &mut s, "OLD", false, true);
        assert_eq!(s.selected, vec!["WALL", "DOOR", "OLD"]);
        // The Ctrl+click moves the anchor too, so the next range starts there.
        click_select(&rows, &mut s, "DOOR", true, false);
        click_select(&rows, &mut s, "WALL", false, true);
        assert_eq!(s.selected, vec!["WALL", "DOOR"]);
    }

    #[test]
    fn arrow_keys_step_through_the_drawn_rows() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        s.selected = vec!["WALL".into()];
        s.anchor = Some(1);
        key_navigate(&rows, &mut s, true);
        assert_eq!(s.selected, vec!["0"]);
        key_navigate(&rows, &mut s, false);
        assert_eq!(s.selected, vec!["WALL"]);
        // At the top, Up stays put; Down runs to the end and stops there.
        s.anchor = Some(0);
        s.selected = vec!["0".into()];
        key_navigate(&rows, &mut s, true);
        assert_eq!(s.selected, vec!["0"]);
        for _ in 0..4 {
            key_navigate(&rows, &mut s, false);
        }
        assert_eq!(s.selected, vec!["OLD"]);
    }

    #[test]
    fn visible_set_is_selected_union_held_with_the_zero_pin() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        s.selected = vec!["WALL".into()];
        s.held = vec!["DOOR".into()];
        let mut want: FxHashSet<String> = FxHashSet::default();
        want.insert("WALL".into());
        want.insert("DOOR".into());
        assert_eq!(visible_set(&s), want);

        // A row the walk hides is hidden even when the drawing itself keeps it
        // on; only the 0 pin adds to the set.
        assert!(!visible_set(&s).contains("0"));
        s.selected.clear();
        s.held.clear();
        assert!(visible_set(&s).is_empty());
        s.layer0_always_on = true;
        assert_eq!(
            visible_set(&s),
            ["0".to_string()].into_iter().collect::<FxHashSet<String>>()
        );
    }

    #[test]
    fn visible_counts_cover_the_drawn_rows() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        s.selected = vec!["WALL".into(), "DOOR".into()];
        assert_eq!(visible_counts(&rows, &s), (2, 4));
        s.layer0_always_on = true;
        assert_eq!(visible_counts(&rows, &s), (3, 4));
        let filtered = matching_indices(&rows, Some("D*"));
        assert_eq!(visible_counts(&rows, &s), (3, 4));
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn wildcard_filter_applies_only_on_enter_and_highlights() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        s.filter_buf = "D*".into();
        // Typing alone changes nothing: still every row, no highlight set.
        assert_eq!(visible_rows(&rows, &s).len(), 4);
        assert!(matching_indices(&rows, s.pattern.as_deref()).is_empty());
        // Enter applies the pattern.
        s.pattern = Some(s.filter_buf.clone());
        assert_eq!(matching_indices(&rows, s.pattern.as_deref()), vec![2]);
        s.show_only_matching = true;
        assert_eq!(visible_rows(&rows, &s).len(), 1);
        s.show_only_matching = false;
        assert_eq!(visible_rows(&rows, &s).len(), 4);
    }

    #[test]
    fn hold_and_release_bookkeeping() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        s.selected = vec!["WALL".into()];
        let walls = s.selected.clone();
        set_held(&mut s, &walls, true);
        assert_eq!(s.held, vec!["WALL"]);
        // Held layers stay visible even when the selection drops them.
        s.selected = vec!["DOOR".into()];
        let set = visible_set(&s);
        assert!(set.contains("WALL") && set.contains("DOOR"));
        set_held(&mut s, &["WALL".into()], false);
        assert!(s.held.is_empty());
        assert!(!visible_set(&s).contains("WALL"));
    }

    #[test]
    fn menu_enable_rules_match_the_spec() {
        let rows = fixture();
        let s = LayerWalkSession::fresh(&rows);
        let off = |items: &[MenuRow], item: &LayerWalkMenuItem| {
            items
                .iter()
                .any(|r| matches!(r, MenuRow::Disabled(x, _) if x == item))
        };
        let on = |items: &[MenuRow], item: &LayerWalkMenuItem| {
            items
                .iter()
                .any(|r| matches!(r, MenuRow::Item(x) if x == item))
        };
        let m = menu_rows(&rows, &s, &[]);
        assert!(on(&m, &LayerWalkMenuItem::HoldSelection));
        assert!(off(&m, &LayerWalkMenuItem::ReleaseSelection));
        assert!(off(&m, &LayerWalkMenuItem::ReleaseAll));
        assert!(on(&m, &LayerWalkMenuItem::SelectUnreferenced)); // OLD has no objects
        assert!(off(&m, &LayerWalkMenuItem::SaveFilter));
        assert!(off(&m, &LayerWalkMenuItem::DeleteFilter));
        assert!(off(&m, &LayerWalkMenuItem::CopyAsFilter)); // multi-selection
        assert_eq!(
            m.iter().filter(|r| matches!(r, MenuRow::Separator)).count(),
            4
        );

        let mut s = LayerWalkSession::fresh(&rows);
        s.held = vec!["WALL".into()];
        s.selected = vec!["WALL".into()];
        s.pattern = Some("*".into());
        s.saved_active = Some("all".into());
        let m = menu_rows(&rows, &s, &["all".into()]);
        assert!(on(&m, &LayerWalkMenuItem::ReleaseSelection));
        assert!(on(&m, &LayerWalkMenuItem::ReleaseAll));
        assert!(on(&m, &LayerWalkMenuItem::CopyAsFilter));
        assert!(on(&m, &LayerWalkMenuItem::SaveFilter));
        assert!(on(&m, &LayerWalkMenuItem::DeleteFilter));
        // A stale saved-filter name is not "active".
        let m = menu_rows(&rows, &s, &["other".into()]);
        assert!(off(&m, &LayerWalkMenuItem::DeleteFilter));
    }

    #[test]
    fn purge_button_enabled_only_when_the_selection_holds_a_purgeable_layer() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        let enabled = |s: &LayerWalkSession| {
            let sel: FxHashSet<&str> = s.selected.iter().map(String::as_str).collect();
            rows.iter()
                .any(|r| r.purged && sel.contains(r.name.as_str()))
        };
        assert!(!enabled(&s));
        click_select(&rows, &mut s, "OLD", true, false);
        assert!(enabled(&s));
    }

    #[test]
    fn clear_all_keeps_held_layers_visible() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        s.selected = vec!["WALL".into()];
        s.held = vec!["DOOR".into()];
        s.selected.clear();
        let set = visible_set(&s);
        assert!(set.contains("DOOR"));
        assert!(!set.contains("WALL"));
    }

    #[test]
    fn empty_and_single_layer_drawings_walk_cleanly() {
        let rows: Vec<LayerWalkRow> = vec![];
        let s = LayerWalkSession::fresh(&rows);
        assert!(s.selected.is_empty());
        assert!(visible_rows(&rows, &s).is_empty());
        let mut s = s;
        assert!(key_navigate(&rows, &mut s, false).is_none());
        assert!(!walk_changes_underlying(&rows, &s));

        let rows = vec![row("ONLY", 3)];
        let mut s = LayerWalkSession::fresh(&rows);
        click_select(&rows, &mut s, "ONLY", false, false);
        assert_eq!(s.selected, vec!["ONLY"]);
        assert!(visible_set(&s).contains("ONLY"));
    }

    #[test]
    fn xref_names_with_pipe_survive_the_round_trip() {
        let rows = vec![row("REF1|WALL", 1), row("WALL", 1)];
        let mut s = LayerWalkSession::fresh(&rows);
        click_select(&rows, &mut s, "REF1|WALL", false, false);
        assert_eq!(s.selected, vec!["REF1|WALL"]);
        assert!(visible_set(&s).contains("REF1|WALL"));
        assert_eq!(matching_indices(&rows, Some("REF1|*")), vec![0]);
    }

    #[test]
    fn walk_changes_underlying_tracks_the_visible_set() {
        let rows = fixture();
        let s = LayerWalkSession::fresh(&rows);
        // Selected = the visible layers exactly: nothing to commit.
        assert!(!walk_changes_underlying(&rows, &s));
        let mut s2 = s;
        s2.selected.push("OLD".into());
        assert!(walk_changes_underlying(&rows, &s2));
    }

    #[test]
    fn drag_range_uses_the_drawn_order_not_the_raw_indices() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        s.drag_start = Some(0);
        drag_select(&rows, &mut s, 3);
        assert_eq!(s.selected, vec!["0", "WALL", "DOOR", "OLD"]);

        // With the filter narrowed to A*, only the drawn rows are in range.
        let rows = vec![row("A1", 1), row("X", 0), row("A2", 1)];
        let mut s = LayerWalkSession::fresh(&rows);
        s.pattern = Some("A*".into());
        s.show_only_matching = true;
        s.drag_start = Some(0);
        drag_select(&rows, &mut s, 2);
        assert_eq!(s.selected, vec!["A1", "A2"]);
    }

    #[test]
    fn released_press_on_the_same_row_is_not_a_drag() {
        let rows = fixture();
        let mut s = LayerWalkSession::fresh(&rows);
        click_select(&rows, &mut s, "DOOR", false, false);
        drag_select(&rows, &mut s, 2);
        assert_eq!(s.selected, vec!["DOOR"]);
    }

    #[test]
    fn object_count_sums_the_selected_layers() {
        let rows = fixture();
        let names: FxHashSet<String> = ["WALL", "DOOR"].into_iter().map(String::from).collect();
        assert_eq!(object_count(&rows, &names), 12);
    }

    #[test]
    fn many_layers_stay_responsive_in_the_pure_logic() {
        let rows: Vec<LayerWalkRow> = (0..5_000)
            .map(|i| row(&format!("L{i:04}"), i % 7))
            .collect();
        let mut s = LayerWalkSession::fresh(&rows);
        let start = std::time::Instant::now();
        s.pattern = Some("L4*".into());
        let m = matching_indices(&rows, s.pattern.as_deref());
        assert!(!m.is_empty());
        s.show_only_matching = true;
        assert_eq!(visible_rows(&rows, &s).len(), m.len());
        click_select(&rows, &mut s, "L4999", false, false);
        assert_eq!(s.selected, vec!["L4999"]);
        assert!(key_navigate(&rows, &mut s, false).is_some());
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}
