//! General edge-stack docking for side panels.
//!
//! Any number of dockable panels (Properties, the block palette, future
//! palettes) live in an ordered vertical stack of slots on the left or right
//! edge of the drawing view, or float over it. A slot holds one panel or
//! several sharing it as tabs, and owns an adjustable share of the edge
//! height. This module owns the persisted layout (which panels are docked or
//! floating, where, at what size, and whether each auto-collapses) plus the
//! pure geometry used to render and hover an edge's stacked slots.

use crate::app::config::DockSide;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Dock chrome interactions. These are panel-agnostic so any dockable panel
/// (Properties, the block palette, future palettes) shares one code path.
#[derive(Debug, Clone)]
pub enum DockMsg {
    /// Begin dragging `panel` to another side / position.
    DockGrab(PanelId),
    /// Begin resizing `panel`'s width.
    ResizeGrab(PanelId),
    /// Reset `panel`'s width to its default.
    WidthReset(PanelId),
    /// Toggle `panel`'s auto-collapse (pin) behavior.
    AutoCollapseToggle(PanelId),
    /// Close / hide `panel`.
    Close(PanelId),
    /// The pointer is over `panel`, raising it to full height.
    Hover(PanelId),
    /// Pointer moved while a panel is dragging or resizing.
    DragMove(iced::Point),
    /// Pointer released after a drag / resize.
    DragRelease,
    /// Show `panel`'s tab in its tab group.
    SelectTab(PanelId),
    /// Begin dragging the splitter between slots `upper` and `lower` on a side.
    SplitGrab(DockSide, usize, usize),
    /// Give every slot on a side the same height again.
    SplitReset(DockSide),
    /// Begin resizing floating `panel` from its corner grip; `true` for the
    /// bottom-left grip (a panel whose title bar is on the right).
    FloatResizeGrab(PanelId, bool),
    /// The pointer entered (`Some`) or left (`None`) a docked title bar.
    TitleHover(Option<PanelId>),
    /// Open (`Some`) the pallet menu of a slot (side, slot index), or close
    /// it (`None`).
    TabMenu(Option<(DockSide, usize)>),
    /// Pallet menu pick: show `panel` as a tab of the slot, or hide it when
    /// it already shows there.
    TabMenuToggle(DockSide, usize, PanelId),
    /// Bring floating `panel` to the front.
    FloatRaise(PanelId),
    /// The pointer left the edge column; collapse any auto-collapsing panel.
    HoverExit,
}

/// The dockable panels the application knows about. New palettes add a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelId {
    Properties,
    BlockPalette,
    ExternalReferences,
    /// Outline of the drawing's origin planes, open sketch and solid bodies.
    Browser,
    /// Node library of the node graph overlay.
    NodeGraph,
    /// Regions and scans of the drawing's point clouds.
    PointCloudManager,
    /// Block counts (COUNTLIST).
    Count,
    /// The open sheet sets: subsets and sheets.
    SheetSetManager,
}

impl PanelId {
    /// Every panel, in the order the pallet menu lists them.
    pub const ALL: [PanelId; 8] = [
        PanelId::Properties,
        PanelId::BlockPalette,
        PanelId::ExternalReferences,
        PanelId::Browser,
        PanelId::NodeGraph,
        PanelId::PointCloudManager,
        PanelId::Count,
        PanelId::SheetSetManager,
    ];

    /// Localized-friendly display name used by the collapsed/edge chrome.
    pub fn title(self) -> &'static str {
        match self {
            PanelId::Properties => "Properties",
            PanelId::BlockPalette => "Block Palette",
            PanelId::ExternalReferences => "External References",
            PanelId::Browser => "Browser",
            PanelId::NodeGraph => "Node Graph",
            PanelId::PointCloudManager => "Point Cloud Manager",
            PanelId::Count => "Count",
            PanelId::SheetSetManager => "Sheet Set Manager",
        }
    }

    /// The panel's icon: the one on the ribbon tool that opens it.
    pub fn icon(self) -> &'static [u8] {
        match self {
            PanelId::Properties => include_bytes!("../../assets/icons/properties.svg"),
            PanelId::BlockPalette => include_bytes!("../../assets/icons/blocks/insert.svg"),
            PanelId::ExternalReferences => crate::ui::icons::FOLDER_OPEN,
            PanelId::Browser => include_bytes!("../../assets/icons/content_browser.svg"),
            PanelId::NodeGraph => crate::ui::icons::NODE_GRAPH,
            PanelId::PointCloudManager => include_bytes!("../../assets/icons/pc_attach.svg"),
            PanelId::Count => include_bytes!("../../assets/icons/data_extract.svg"),
            PanelId::SheetSetManager => include_bytes!("../../assets/icons/sheetset.svg"),
        }
    }

    /// Default dock width for a freshly-created panel instance.
    fn default_width(self) -> f32 {
        match self {
            PanelId::Properties => 250.0,
            PanelId::BlockPalette => 260.0,
            PanelId::ExternalReferences => 460.0,
            PanelId::Browser => 230.0,
            PanelId::NodeGraph => 220.0,
            PanelId::PointCloudManager => 280.0,
            PanelId::Count => 280.0,
            PanelId::SheetSetManager => 280.0,
        }
    }

    /// Widest a panel may be dragged or sized to. The references table is
    /// column-rich, so it allows double the shared maximum.
    fn max_width(self) -> f32 {
        match self {
            PanelId::ExternalReferences => DOCK_MAX_W * 2.0,
            _ => DOCK_MAX_W,
        }
    }
}

/// The persistent, per-panel dock settings that survive a restart.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockPanel {
    pub width: f32,
    pub auto_collapse: bool,
}

impl Default for DockPanel {
    fn default() -> Self {
        Self {
            width: 250.0,
            auto_collapse: false,
        }
    }
}

impl DockPanel {
    fn for_id(id: PanelId) -> Self {
        Self {
            width: id.default_width(),
            auto_collapse: false,
        }
    }
}

/// One slot of an edge stack: one or more panels sharing the slot as tabs.
/// A single-panel group renders without a tab strip, exactly like the old
/// one-panel-per-slot stack. `weight` is the slot's share of the edge height
/// relative to the other slots on the same edge (equal weights = equal split).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "GroupRepr")]
pub struct DockGroup {
    /// Panels in tab order.
    pub tabs: Vec<PanelId>,
    /// The tab currently shown. Always one of `tabs`.
    pub active: PanelId,
    /// Relative share of the edge height.
    pub weight: f32,
}

/// On-disk forms of a [`DockGroup`]: configs written before tab groups stored
/// each slot as a bare panel id.
#[derive(Deserialize)]
#[serde(untagged)]
enum GroupRepr {
    Single(PanelId),
    Group {
        tabs: Vec<PanelId>,
        #[serde(default)]
        active: Option<PanelId>,
        #[serde(default = "default_weight")]
        weight: f32,
    },
}

fn default_weight() -> f32 {
    1.0
}

impl From<GroupRepr> for DockGroup {
    fn from(repr: GroupRepr) -> Self {
        match repr {
            GroupRepr::Single(id) => DockGroup::single(id, 1.0),
            GroupRepr::Group {
                tabs,
                active,
                weight,
            } => {
                // A hand-edited or corrupt group still yields a usable slot.
                let tabs = if tabs.is_empty() {
                    vec![PanelId::Properties]
                } else {
                    tabs
                };
                let active = active.filter(|a| tabs.contains(a)).unwrap_or(tabs[0]);
                let weight = if weight.is_finite() && weight > 0.0 {
                    weight.clamp(MIN_WEIGHT, MAX_WEIGHT)
                } else {
                    1.0
                };
                DockGroup {
                    tabs,
                    active,
                    weight,
                }
            }
        }
    }
}

impl DockGroup {
    pub fn single(id: PanelId, weight: f32) -> Self {
        Self {
            tabs: vec![id],
            active: id,
            weight,
        }
    }
}

/// A panel floating over the workspace, in workspace-local pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FloatPanel {
    pub id: PanelId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Smallest floating panel height.
pub const FLOAT_MIN_H: f32 = 160.0;
/// Smallest on-screen height of a docked slot while dragging a splitter.
pub const GROUP_MIN_H: f32 = 80.0;
const MIN_WEIGHT: f32 = 0.05;
const MAX_WEIGHT: f32 = 20.0;

/// The whole dock layout: two ordered per-side stacks of tab groups, the
/// floating panels, plus per-panel settings. Only the persisted layout lives
/// here; transient drag/hover state is app state (see `update::mod`) so it is
/// skipped by serialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockState {
    /// Slots anchored to the left edge, top → bottom.
    pub left: Vec<DockGroup>,
    /// Slots anchored to the right edge, top → bottom.
    pub right: Vec<DockGroup>,
    /// Floating panels, back → front.
    pub floating: Vec<FloatPanel>,
    /// Per-panel width / auto-collapse settings, keyed by `PanelId`.
    pub panels: BTreeMap<PanelId, DockPanel>,
}

impl Default for DockState {
    fn default() -> Self {
        Self {
            left: vec![DockGroup::single(PanelId::Properties, 1.0)],
            right: vec![DockGroup::single(PanelId::BlockPalette, 1.0)],
            floating: Vec::new(),
            panels: BTreeMap::new(),
        }
    }
}

impl DockState {
    /// The slots stacked on `side`, top → bottom.
    pub fn groups(&self, side: DockSide) -> &[DockGroup] {
        match side {
            DockSide::Left => &self.left,
            DockSide::Right => &self.right,
        }
    }

    fn groups_mut(&mut self, side: DockSide) -> &mut Vec<DockGroup> {
        match side {
            DockSide::Left => &mut self.left,
            DockSide::Right => &mut self.right,
        }
    }

    /// Guarantee every known `PanelId` has a settings entry, so rendering and
    /// resize never hit a missing configuration. Also a cheap heal for configs
    /// written by an older version (or edited by hand): a panel placed twice
    /// keeps only its first placement.
    pub fn ensure_settings(&mut self) {
        for id in PanelId::ALL {
            self.panels.entry(id).or_insert_with(|| DockPanel::for_id(id));
        }
        let mut seen = std::collections::BTreeSet::new();
        for side in [DockSide::Left, DockSide::Right] {
            let groups = self.groups_mut(side);
            for g in groups.iter_mut() {
                g.tabs.retain(|id| seen.insert(*id));
                if !g.tabs.contains(&g.active) {
                    if let Some(first) = g.tabs.first() {
                        g.active = *first;
                    }
                }
            }
            groups.retain(|g| !g.tabs.is_empty());
        }
        self.floating.retain(|f| seen.insert(f.id));
    }

    /// Where (if anywhere) a panel is docked: its side and slot index.
    pub fn location(&self, id: PanelId) -> Option<(DockSide, usize)> {
        for side in [DockSide::Left, DockSide::Right] {
            if let Some(i) = self.groups(side).iter().position(|g| g.tabs.contains(&id)) {
                return Some((side, i));
            }
        }
        None
    }

    /// The floating placement of `id`, if it floats.
    pub fn float_rect(&self, id: PanelId) -> Option<FloatPanel> {
        self.floating.iter().find(|f| f.id == id).copied()
    }

    /// Whether `id` has a place in the layout (docked or floating). A panel
    /// without one is docked on the right when it is opened.
    pub fn is_placed(&self, id: PanelId) -> bool {
        self.location(id).is_some() || self.float_rect(id).is_some()
    }

    pub fn settings(&self, id: PanelId) -> DockPanel {
        self.panels
            .get(&id)
            .copied()
            .unwrap_or_else(|| DockPanel::for_id(id))
    }

    /// Docked width for `id`, clamped to sane bounds.
    pub fn width(&self, id: PanelId, win_w: f32) -> f32 {
        self.settings(id)
            .width
            .clamp(DOCK_MIN_W, id.max_width().min(win_w * 0.45).max(DOCK_MIN_W))
    }

    pub fn auto_collapse(&self, id: PanelId) -> bool {
        self.settings(id).auto_collapse
    }

    /// Set the persisted width, clamped.
    pub fn set_width(&mut self, id: PanelId, width: f32) {
        let entry = self.panels.entry(id).or_insert_with(|| DockPanel::for_id(id));
        entry.width = width.clamp(DOCK_MIN_W, id.max_width());
    }

    /// Reset width to the panel's default.
    pub fn reset_width(&mut self, id: PanelId) {
        let entry = self.panels.entry(id).or_insert_with(|| DockPanel::for_id(id));
        entry.width = id.default_width();
    }

    pub fn set_auto_collapse(&mut self, id: PanelId, on: bool) {
        let entry = self.panels.entry(id).or_insert_with(|| DockPanel::for_id(id));
        entry.auto_collapse = on;
    }

    /// Take `id` out of the layout. Returns the docked slot it left and
    /// whether that slot disappeared (it held no other tab).
    fn detach(&mut self, id: PanelId) -> Option<(DockSide, usize, bool)> {
        self.floating.retain(|f| f.id != id);
        let (side, gi) = self.location(id)?;
        let groups = self.groups_mut(side);
        let g = &mut groups[gi];
        g.tabs.retain(|t| *t != id);
        if g.tabs.is_empty() {
            groups.remove(gi);
            return Some((side, gi, true));
        }
        if g.active == id {
            g.active = g.tabs[0];
        }
        Some((side, gi, false))
    }

    /// Dock `id` as its own slot on `side` at insertion `index` (0 = top,
    /// `len` = bottom), counted in the stack as it is *before* the move.
    /// Returns whether the layout actually changed.
    pub fn dock(&mut self, id: PanelId, side: DockSide, index: usize) -> bool {
        let before = self.clone();
        let weight = {
            let groups = self.groups(side);
            if groups.is_empty() {
                1.0
            } else {
                groups.iter().map(|g| g.weight).sum::<f32>() / groups.len() as f32
            }
        };
        let mut index = index;
        let mut weight = weight;
        if let Some((old_side, old_i, removed)) = self.detach(id) {
            if old_side == side && removed {
                // Moving within an edge keeps the slot's own height share.
                weight = before.groups(side)[old_i].weight;
                if old_i < index {
                    index -= 1;
                }
            }
        }
        let groups = self.groups_mut(side);
        let index = index.min(groups.len());
        groups.insert(index, DockGroup::single(id, weight));
        *self != before
    }

    /// Add `id` as a tab of slot `group` on `side` (slot index counted
    /// before the move) and show it. `index` is the tab position, counted in
    /// the slot's tabs before the move; `None` appends a newcomer and leaves
    /// a tab already in the slot where it is. A joining panel takes on the
    /// slot's width, so switching tabs never changes the column width.
    /// Returns whether the layout changed.
    pub fn add_tab(
        &mut self,
        id: PanelId,
        side: DockSide,
        group: usize,
        index: Option<usize>,
    ) -> bool {
        if group >= self.groups(side).len() {
            return false;
        }
        let before = self.clone();
        if let Some(pos) = self.groups(side)[group].tabs.iter().position(|t| *t == id) {
            // Reordering within the slot.
            let g = &mut self.groups_mut(side)[group];
            if let Some(index) = index {
                g.tabs.remove(pos);
                let index = if pos < index { index - 1 } else { index };
                g.tabs.insert(index.min(g.tabs.len()), id);
            }
            g.active = id;
            return *self != before;
        }
        let width = self.group_width(side, group);
        let mut group = group;
        if let Some((old_side, old_i, removed)) = self.detach(id) {
            if old_side == side && removed && old_i < group {
                group -= 1;
            }
        }
        let g = &mut self.groups_mut(side)[group];
        let index = index.unwrap_or(g.tabs.len()).min(g.tabs.len());
        g.tabs.insert(index, id);
        g.active = id;
        self.set_width(id, width);
        true
    }

    /// The saved width a slot's panels share: the widest of its tabs, so a
    /// slot whose tabs were sized apart (older configs) renders at one width.
    pub fn group_width(&self, side: DockSide, group: usize) -> f32 {
        self.groups(side)[group]
            .tabs
            .iter()
            .map(|id| self.settings(*id).width)
            .fold(DOCK_MIN_W, f32::max)
    }

    /// Set the width of `id` and of every other tab in its slot.
    pub fn set_group_width(&mut self, id: PanelId, width: f32) {
        let tabs = match self.location(id) {
            Some((side, gi)) => self.groups(side)[gi].tabs.clone(),
            None => vec![id],
        };
        for t in tabs {
            self.set_width(t, width);
        }
    }

    /// Float `id` at `rect`, on top of the other floating panels.
    pub fn float(&mut self, rect: FloatPanel) -> bool {
        let before = self.clone();
        self.detach(rect.id);
        self.floating.push(rect);
        *self != before
    }

    /// Bring a floating panel to the front.
    pub fn raise_float(&mut self, id: PanelId) {
        if let Some(i) = self.floating.iter().position(|f| f.id == id) {
            let f = self.floating.remove(i);
            self.floating.push(f);
        }
    }

    /// Resize a floating panel, keeping it at least a usable size. With
    /// `keep_right` the right edge stays put (resizing from the left corner).
    pub fn resize_float(&mut self, id: PanelId, w: f32, h: f32, keep_right: bool) {
        if let Some(f) = self.floating.iter_mut().find(|f| f.id == id) {
            let new_w = w.clamp(DOCK_MIN_W, id.max_width());
            if keep_right {
                f.x += f.w - new_w;
            }
            f.w = new_w;
            f.h = h.max(FLOAT_MIN_H);
        }
    }

    /// Show `id` in its tab group. Returns whether the active tab changed.
    pub fn select_tab(&mut self, id: PanelId) -> bool {
        let Some((side, gi)) = self.location(id) else {
            return false;
        };
        let g = &mut self.groups_mut(side)[gi];
        let changed = g.active != id;
        g.active = id;
        changed
    }

    /// Move the boundary between slots `upper` and `lower` on `side` by
    /// `delta` weight units, keeping their combined share and each above
    /// `min_weight`.
    pub fn shift_split(
        &mut self,
        side: DockSide,
        upper: usize,
        lower: usize,
        delta: f32,
        min_weight: f32,
    ) {
        let groups = self.groups_mut(side);
        if upper >= groups.len() || lower >= groups.len() || upper == lower {
            return;
        }
        let total = groups[upper].weight + groups[lower].weight;
        let min = min_weight.clamp(MIN_WEIGHT, total * 0.5);
        let up = (groups[upper].weight + delta).clamp(min, total - min);
        groups[upper].weight = up;
        groups[lower].weight = total - up;
    }

    /// Give every slot on `side` the same height again.
    pub fn reset_splits(&mut self, side: DockSide) {
        for g in self.groups_mut(side) {
            g.weight = 1.0;
        }
    }
}

/// Where a dragged panel lands on release.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropTarget {
    /// A new slot on `side` at insertion `index` (0 = top, len = bottom).
    Edge { side: DockSide, index: usize },
    /// A tab in slot `group` on `side`, at position `index` among the
    /// slot's tabs (before the move), or appended / left in place for `None`.
    Tab {
        side: DockSide,
        group: usize,
        index: Option<usize>,
    },
    /// Floating with its top-left corner at (`x`, `y`).
    Float { x: f32, y: f32 },
}

/// Which part of a docked slot the pointer is over while dragging: the top
/// and bottom bands (35 % each) split the slot (dock above / below), the
/// middle 30 % joins it as a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotZone {
    Above,
    Middle,
    Below,
}

/// Classify a pointer at `y` within a slot spanning `top..bottom`.
pub fn slot_zone(y: f32, top: f32, bottom: f32) -> SlotZone {
    let h = (bottom - top).max(1.0);
    let band = h * 0.35;
    if y < top + band {
        SlotZone::Above
    } else if y > bottom - band {
        SlotZone::Below
    } else {
        SlotZone::Middle
    }
}

/// Split `avail` pixels of height between slots of `weights`, returning each
/// slot's `(top, bottom)`.
pub fn slot_spans(weights: &[f32], avail: f32) -> Vec<(f32, f32)> {
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let mut y = 0.0;
    weights
        .iter()
        .map(|w| {
            let top = y;
            y += avail * w / total;
            (top, y)
        })
        .collect()
}

/// Integer share for iced's `FillPortion` from a slot weight.
pub fn portion(weight: f32) -> u16 {
    (weight * 1000.0).round().clamp(1.0, u16::MAX as f32) as u16
}

/// Smallest docked width a panel may be dragged or sized to.
pub const DOCK_MIN_W: f32 = 200.0;
/// Largest docked width a panel may be dragged or sized to.
pub const DOCK_MAX_W: f32 = 600.0;
/// Width of the band along an empty edge that docks a dragged panel there.
pub const DOCK_EDGE_ZONE: f32 = 48.0;
/// Width of a collapsed (auto-collapsing) slot's tab in the edge strip.
pub const DOCK_RAIL_W: f32 = 28.0;
/// Width of the grabbable divider between a docked slot and the viewport.
pub const DOCK_DIVIDER_W: f32 = 5.0;
/// Height of a docked slot's tab strip.
pub const DOCK_TAB_H: f32 = 24.0;

/// Pitch of one icon tab in a tab strip (tab plus the gap after it).
pub const DOCK_TAB_CELL_W: f32 = 30.0;
/// Left inset of the first tab in a tab strip.
pub const DOCK_TAB_INSET: f32 = 4.0;

/// Insertion position (0..=`count`) for a tab dropped at `x` on a strip of
/// `count` left-aligned tabs of pitch `cell` starting at `x0`.
pub fn tab_insert_index(x: f32, x0: f32, cell: f32, count: usize) -> usize {
    if count == 0 || cell <= 0.0 {
        return 0;
    }
    (((x - x0) / cell).round().max(0.0) as usize).min(count)
}

// ── Shared panel chrome ─────────────────────────────────────────────────

use crate::app::Message;
use iced::widget::{button, container, mouse_area, row, text, tooltip, Space};
use iced::{Background, Border, Color, Element, Length, Theme};

/// How a panel is framed where it is shown, passed to its view so the shared
/// chrome can adapt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Chrome {
    /// The panel auto-collapses (its pin is on).
    pub auto_collapse: bool,
    /// The panel floats; its title lives in the vertical side bar drawn by
    /// the floating frame instead of the horizontal title bar.
    pub floating: bool,
    /// The pointer is over the title bar, which shows its pin and close.
    pub title_hovered: bool,
}

/// The row every docked panel starts with: its title, the auto-collapse pin
/// and close. Pressing the row starts re-docking the panel. A floating panel
/// gets nothing here: its frame draws a vertical title bar beside it.
pub fn title_bar<'a>(id: PanelId, title: String, chrome: Chrome) -> Element<'a, Message> {
    if chrome.floating {
        return Space::new().width(0).height(0).into();
    }
    // The pin and close only show while the pointer is over the bar; the
    // fixed height keeps the bar from jumping when they appear.
    let mut bar = row![text(title).size(12), Space::new().width(Length::Fill)]
        .spacing(3)
        .height(Length::Fixed(TITLE_BUTTON_H))
        .align_y(iced::Center);
    if chrome.title_hovered {
        bar = bar
            .push(pin_button(id, chrome.auto_collapse, tooltip::Position::Bottom))
            .push(close_button(id, tooltip::Position::Bottom));
    }
    mouse_area(
        container(bar)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.weak.color)),
                ..Default::default()
            })
            .width(Length::Fill)
            .padding([3, 6]),
    )
    .on_press(Message::Dock(DockMsg::DockGrab(id)))
    .on_enter(Message::Dock(DockMsg::TitleHover(Some(id))))
    .on_exit(Message::Dock(DockMsg::TitleHover(None)))
    .interaction(iced::mouse::Interaction::Grab)
    .into()
}

/// Height of a title bar's button row.
const TITLE_BUTTON_H: f32 = 20.0;

/// The auto-hide pin. While auto-hide is on the pin is drawn tilted instead
/// of on a coloured background.
pub fn pin_button<'a>(id: PanelId, auto_collapse: bool, tip: tooltip::Position) -> Element<'a, Message> {
    let icon = crate::ui::icons::themed_secondary(
        if auto_collapse {
            crate::ui::icons::PIN_ACTIVE
        } else {
            crate::ui::icons::PIN
        },
        12.0,
    );
    let pin = button(icon)
        .on_press(Message::Dock(DockMsg::AutoCollapseToggle(id)))
        .style(button::subtle)
        .padding([3, 5]);
    tooltip(pin, text(crate::t!("Auto-hide")).size(10), tip)
        .gap(4)
        .into()
}

/// The close button of a panel's title bar.
pub fn close_button<'a>(id: PanelId, tip: tooltip::Position) -> Element<'a, Message> {
    let close = button(crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 12.0))
        .on_press(Message::Dock(DockMsg::Close(id)))
        .style(button::subtle)
        .padding([3, 5]);
    tooltip(close, text(crate::t!("Close")).size(10), tip)
        .gap(4)
        .into()
}

/// Side length of a panel toolbar button's icon.
pub const TOOL_H: f32 = 22.0;

/// A panel toolbar icon button, placed on the row under the title bar.
pub fn tool_button<'a>(
    icon: Element<'a, Message>,
    tip: String,
    message: Message,
) -> Element<'a, Message> {
    let button = button(icon)
        .on_press(message)
        .width(Length::Fixed(TOOL_H + 8.0))
        .height(Length::Fixed(TOOL_H + 8.0))
        .style(|theme: &Theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => {
                    theme.palette().background.strong.color
                }
                _ => Color::TRANSPARENT,
            })),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            text_color: crate::ui::window::block_palette::block_icon_button_text_color(
                theme, status,
            ),
            ..Default::default()
        });
    tooltip(button, text(tip).size(10), tooltip::Position::Bottom)
        .gap(4)
        .into()
}

/// A docked panel's body: its title bar flush along the top (as on the
/// Properties panel), then the padded content; fixed width, full height, on
/// the base background with a neutral edge.
pub fn frame<'a>(
    title_bar: Element<'a, Message>,
    content: impl Into<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    container(iced::widget::column![
        title_bar,
        container(content).padding(6).height(Length::Fill)
    ])
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_references_allows_double_max_width() {
        let mut state = DockState::default();
        state.ensure_settings();
        // Wider default for the column-rich references table, double the maximum.
        // (The 45%-of-window rule still dominates on narrow windows.)
        assert_eq!(state.width(PanelId::ExternalReferences, 3000.0), 460.0);
        state.set_width(PanelId::ExternalReferences, 5000.0);
        assert_eq!(state.width(PanelId::ExternalReferences, 3000.0), DOCK_MAX_W * 2.0);
        // Other panels keep the shared maximum.
        state.set_width(PanelId::BlockPalette, 5000.0);
        assert_eq!(state.width(PanelId::BlockPalette, 3000.0), DOCK_MAX_W);
    }

    #[test]
    fn default_docks_each_known_panel_on_an_edge() {
        let state = DockState::default();
        assert_eq!(state.location(PanelId::Properties), Some((DockSide::Left, 0)));
        assert_eq!(
            state.location(PanelId::BlockPalette),
            Some((DockSide::Right, 0))
        );
    }

    #[test]
    fn ensure_settings_seeds_missing_entries_with_defaults() {
        let mut state = DockState::default();
        state.ensure_settings();
        assert_eq!(state.width(PanelId::Properties, 1600.0), 250.0);
        assert_eq!(state.width(PanelId::BlockPalette, 1600.0), 260.0);
        assert!(!state.auto_collapse(PanelId::Properties));
    }

    #[test]
    fn dock_moves_between_sides() {
        let mut state = DockState::default();
        assert!(state.dock(PanelId::Properties, DockSide::Right, 0));
        assert_eq!(
            state.location(PanelId::Properties),
            Some((DockSide::Right, 0))
        );
        // It no longer occupies the left edge.
        assert!(state.left.is_empty());
        assert_eq!(state.right.len(), 2);
    }

    #[test]
    fn dock_clamps_index() {
        let mut state = DockState::default();
        assert!(state.dock(PanelId::Properties, DockSide::Right, 99));
        assert_eq!(
            state.location(PanelId::Properties),
            Some((DockSide::Right, 1))
        );
    }

    #[test]
    fn dock_noop_when_same_spot() {
        let mut state = DockState::default();
        assert!(!state.dock(PanelId::Properties, DockSide::Left, 0));
    }

    #[test]
    fn set_width_clamps() {
        let mut state = DockState::default();
        state.set_width(PanelId::BlockPalette, 10.0);
        assert_eq!(state.width(PanelId::BlockPalette, 1600.0), DOCK_MIN_W);
        state.set_width(PanelId::BlockPalette, 5000.0);
        assert_eq!(state.width(PanelId::BlockPalette, 1600.0), DOCK_MAX_W);
    }

    #[test]
    fn width_respects_maximum_window_fraction() {
        let mut state = DockState::default();
        state.set_width(PanelId::BlockPalette, 500.0);
        // Window too narrow -> capped by the 0.45 fraction, not DOCK_MAX_W.
        assert_eq!(state.width(PanelId::BlockPalette, 800.0), DOCK_MAX_W.min(360.0));
    }

    #[test]
    fn width_does_not_panic_when_window_minimized() {
        let mut state = DockState::default();
        state.set_width(PanelId::BlockPalette, 500.0);
        // A minimized or not-yet-laid-out window reports width 0; the clamp
        // must fall back to DOCK_MIN_W instead of panicking on min > max.
        assert_eq!(state.width(PanelId::BlockPalette, 0.0), DOCK_MIN_W);
        // Deleted right below the minimum dock width behaves the same way.
        assert_eq!(state.width(PanelId::BlockPalette, 100.0), DOCK_MIN_W);
    }

    #[test]
    fn legacy_config_with_bare_panel_ids_loads_as_single_slots() {
        let json = r#"{"left":["properties","block_palette"],"right":[]}"#;
        let state: DockState = serde_json::from_str(json).unwrap();
        assert_eq!(state.left.len(), 2);
        assert_eq!(state.left[0], DockGroup::single(PanelId::Properties, 1.0));
        assert_eq!(state.location(PanelId::BlockPalette), Some((DockSide::Left, 1)));
        assert!(state.floating.is_empty());
    }

    #[test]
    fn layout_round_trips_through_serde() {
        let mut state = DockState::default();
        state.add_tab(PanelId::Browser, DockSide::Left, 0, None);
        state.float(FloatPanel {
            id: PanelId::Count,
            x: 10.0,
            y: 20.0,
            w: 300.0,
            h: 400.0,
        });
        let json = serde_json::to_string(&state).unwrap();
        let back: DockState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn add_tab_joins_a_slot_and_shows_the_new_tab() {
        let mut state = DockState::default();
        assert!(state.add_tab(PanelId::BlockPalette, DockSide::Left, 0, None));
        assert!(state.right.is_empty(), "its old slot disappears");
        assert_eq!(state.left.len(), 1);
        assert_eq!(
            state.left[0].tabs,
            vec![PanelId::Properties, PanelId::BlockPalette]
        );
        assert_eq!(state.left[0].active, PanelId::BlockPalette);
        assert!(state.select_tab(PanelId::Properties));
        assert_eq!(state.left[0].active, PanelId::Properties);
    }

    #[test]
    fn add_tab_reorders_within_a_slot() {
        let mut state = DockState::default();
        state.add_tab(PanelId::BlockPalette, DockSide::Left, 0, None);
        state.add_tab(PanelId::Browser, DockSide::Left, 0, None);
        // Drag the first tab to the end.
        assert!(state.add_tab(PanelId::Properties, DockSide::Left, 0, Some(3)));
        assert_eq!(
            state.left[0].tabs,
            vec![PanelId::BlockPalette, PanelId::Browser, PanelId::Properties]
        );
        // Drag the last tab to the front.
        assert!(state.add_tab(PanelId::Properties, DockSide::Left, 0, Some(0)));
        assert_eq!(state.left[0].tabs[0], PanelId::Properties);
        // A middle drop (None) leaves a member where it is.
        assert!(!state.add_tab(PanelId::Properties, DockSide::Left, 0, None));
        // A newcomer can land at a given position.
        state.add_tab(PanelId::Count, DockSide::Left, 0, Some(1));
        assert_eq!(state.left[0].tabs[1], PanelId::Count);
    }

    #[test]
    fn tabs_share_the_slot_width() {
        let mut state = DockState::default();
        state.set_width(PanelId::Properties, 320.0);
        state.add_tab(PanelId::BlockPalette, DockSide::Left, 0, None);
        assert_eq!(state.settings(PanelId::BlockPalette).width, 320.0);
        state.set_group_width(PanelId::BlockPalette, 400.0);
        assert_eq!(state.settings(PanelId::Properties).width, 400.0);
        assert_eq!(state.group_width(DockSide::Left, 0), 400.0);
    }

    #[test]
    fn tab_insert_index_rounds_to_the_nearest_gap() {
        assert_eq!(tab_insert_index(0.0, 0.0, 30.0, 3), 0);
        assert_eq!(tab_insert_index(40.0, 0.0, 30.0, 3), 1);
        assert_eq!(tab_insert_index(85.0, 0.0, 30.0, 3), 3);
        assert_eq!(tab_insert_index(250.0, 0.0, 30.0, 3), 3);
        assert_eq!(tab_insert_index(-20.0, 0.0, 30.0, 3), 0);
    }

    #[test]
    fn docking_a_tab_out_keeps_the_rest_of_the_group() {
        let mut state = DockState::default();
        state.add_tab(PanelId::BlockPalette, DockSide::Left, 0, None);
        // Pull the shown tab out below the group.
        assert!(state.dock(PanelId::BlockPalette, DockSide::Left, 1));
        assert_eq!(state.left.len(), 2);
        assert_eq!(state.left[0].tabs, vec![PanelId::Properties]);
        assert_eq!(state.left[0].active, PanelId::Properties);
        assert_eq!(state.location(PanelId::BlockPalette), Some((DockSide::Left, 1)));
    }

    #[test]
    fn add_tab_index_counts_slots_before_the_move() {
        let mut state = DockState::default();
        state.left = vec![
            DockGroup::single(PanelId::BlockPalette, 1.0),
            DockGroup::single(PanelId::Properties, 1.0),
        ];
        state.right.clear();
        // Slot 1 (Properties) is slot 0 once the palette's slot is gone.
        assert!(state.add_tab(PanelId::BlockPalette, DockSide::Left, 1, None));
        assert_eq!(state.left.len(), 1);
        assert_eq!(
            state.left[0].tabs,
            vec![PanelId::Properties, PanelId::BlockPalette]
        );
    }

    #[test]
    fn moving_a_slot_down_keeps_its_height_share() {
        let mut state = DockState::default();
        state.left = vec![
            DockGroup::single(PanelId::BlockPalette, 0.5),
            DockGroup::single(PanelId::Properties, 1.5),
        ];
        assert!(state.dock(PanelId::BlockPalette, DockSide::Left, 2));
        assert_eq!(state.left[0].tabs, vec![PanelId::Properties]);
        assert_eq!(state.left[1], DockGroup::single(PanelId::BlockPalette, 0.5));
    }

    #[test]
    fn float_detaches_and_dock_brings_it_back() {
        let mut state = DockState::default();
        let rect = FloatPanel {
            id: PanelId::Properties,
            x: 100.0,
            y: 50.0,
            w: 250.0,
            h: 300.0,
        };
        assert!(state.float(rect));
        assert!(state.left.is_empty());
        assert_eq!(state.location(PanelId::Properties), None);
        assert_eq!(state.float_rect(PanelId::Properties), Some(rect));
        assert!(state.is_placed(PanelId::Properties));
        assert!(state.dock(PanelId::Properties, DockSide::Right, 0));
        assert!(state.floating.is_empty());
        assert_eq!(state.location(PanelId::Properties), Some((DockSide::Right, 0)));
    }

    #[test]
    fn float_resize_respects_minimums() {
        let mut state = DockState::default();
        state.float(FloatPanel {
            id: PanelId::Count,
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 300.0,
        });
        state.resize_float(PanelId::Count, 10.0, 10.0, false);
        let f = state.float_rect(PanelId::Count).unwrap();
        assert_eq!((f.w, f.h), (DOCK_MIN_W, FLOAT_MIN_H));
        // From the left corner the right edge stays where it was.
        state.resize_float(PanelId::Count, 350.0, 300.0, true);
        let f = state.float_rect(PanelId::Count).unwrap();
        assert_eq!((f.x, f.w), (DOCK_MIN_W - 350.0, 350.0));
    }

    #[test]
    fn raise_float_moves_it_to_the_front() {
        let mut state = DockState::default();
        for id in [PanelId::Count, PanelId::Browser] {
            state.float(FloatPanel {
                id,
                x: 0.0,
                y: 0.0,
                w: 300.0,
                h: 300.0,
            });
        }
        state.raise_float(PanelId::Count);
        assert_eq!(state.floating.last().map(|f| f.id), Some(PanelId::Count));
    }

    #[test]
    fn shift_split_keeps_combined_share_and_minimum() {
        let mut state = DockState::default();
        state.left = vec![
            DockGroup::single(PanelId::BlockPalette, 1.0),
            DockGroup::single(PanelId::Properties, 1.0),
        ];
        state.shift_split(DockSide::Left, 0, 1, 0.5, 0.1);
        assert_eq!((state.left[0].weight, state.left[1].weight), (1.5, 0.5));
        state.shift_split(DockSide::Left, 0, 1, 10.0, 0.1);
        assert!((state.left[1].weight - 0.1).abs() < 1e-6);
        assert!((state.left[0].weight + state.left[1].weight - 2.0).abs() < 1e-6);
        state.reset_splits(DockSide::Left);
        assert_eq!((state.left[0].weight, state.left[1].weight), (1.0, 1.0));
    }

    #[test]
    fn ensure_settings_drops_duplicate_placements() {
        let mut state = DockState::default();
        state.right.push(DockGroup::single(PanelId::Properties, 1.0));
        state.ensure_settings();
        assert_eq!(state.location(PanelId::Properties), Some((DockSide::Left, 0)));
        assert_eq!(state.right.len(), 1);
    }

    #[test]
    fn every_panel_has_its_own_icon() {
        let ids = [
            PanelId::Properties,
            PanelId::BlockPalette,
            PanelId::ExternalReferences,
            PanelId::Browser,
            PanelId::NodeGraph,
            PanelId::PointCloudManager,
            PanelId::Count,
            PanelId::SheetSetManager,
        ];
        for (i, a) in ids.iter().enumerate() {
            assert!(a.icon().starts_with(b"<svg"), "{a:?} icon is not an SVG");
            for b in &ids[i + 1..] {
                assert_ne!(a.icon(), b.icon(), "{a:?} and {b:?} share an icon");
            }
        }
    }

    #[test]
    fn floating_title_bar_replaces_the_horizontal_one() {
        let floating = Chrome {
            floating: true,
            ..Default::default()
        };
        let bar: Element<'_, Message> = title_bar(PanelId::Count, "Count".into(), floating);
        assert_eq!(
            bar.as_widget().size(),
            iced::Size::new(Length::Fixed(0.0), Length::Fixed(0.0))
        );
    }

    #[test]
    fn slot_zone_splits_into_stacking_and_tab_bands() {
        assert_eq!(slot_zone(10.0, 0.0, 400.0), SlotZone::Above);
        assert_eq!(slot_zone(130.0, 0.0, 400.0), SlotZone::Above);
        assert_eq!(slot_zone(200.0, 0.0, 400.0), SlotZone::Middle);
        assert_eq!(slot_zone(270.0, 0.0, 400.0), SlotZone::Below);
        assert_eq!(slot_zone(390.0, 0.0, 400.0), SlotZone::Below);
    }

    #[test]
    fn slot_spans_follow_weights() {
        assert_eq!(
            slot_spans(&[1.0, 3.0], 400.0),
            vec![(0.0, 100.0), (100.0, 400.0)]
        );
        assert!(slot_spans(&[], 400.0).is_empty());
    }
}
