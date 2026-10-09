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
    /// A press on `panel`'s strip icon: show its group, and start a possible
    /// drag of the pallet.
    IconPress(PanelId),
    /// Begin dragging the splitter between pallets `upper` and `lower` of a
    /// group (side, group, upper, lower).
    SplitGrab(DockSide, usize, usize, usize),
    /// Give every pallet of a group the same height again (side, group).
    SplitReset(DockSide, usize),
    /// Begin resizing floating `panel` from its corner grip; `true` for the
    /// bottom-left grip (a panel whose title bar is on the right).
    FloatResizeGrab(PanelId, bool),
    /// The pointer entered (`Some`) or left (`None`) a docked title bar.
    TitleHover(Option<PanelId>),
    /// Open (`Some`) the pallet menu of an edge's icon strip (its + button),
    /// or close it (`None`).
    EdgeMenu(Option<DockSide>),
    /// Pallet menu pick: hide `panel` when it shows, else open it docked on
    /// the edge (as a new group unless it already has a group there).
    EdgeMenuToggle(DockSide, PanelId),
    /// Begin dragging a whole group by its edge band in the icon strip.
    GroupGrab(DockSide, usize),
    /// The pointer entered (`Some`) or left (`None`) a group's edge band.
    GripHover(Option<(DockSide, usize)>),
    /// The pointer entered (`Some`) or left (`None`) a pallet's strip icon.
    IconHover(Option<PanelId>),
    /// Double-click on a docked title bar: float the panel.
    FloatOut(PanelId),
    /// Double-click on a floating title bar: dock the panel on that side.
    DockTo(PanelId, DockSide),
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
    /// The Layer Manager (LAYERS).
    Layers,
}

impl PanelId {
    /// Every panel, in the order the pallet menu lists them.
    pub const ALL: [PanelId; 9] = [
        PanelId::Properties,
        PanelId::BlockPalette,
        PanelId::ExternalReferences,
        PanelId::Browser,
        PanelId::NodeGraph,
        PanelId::PointCloudManager,
        PanelId::Count,
        PanelId::SheetSetManager,
        PanelId::Layers,
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
            PanelId::Layers => "Layer Manager",
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
            PanelId::Layers => include_bytes!("../../assets/icons/layers/panel.svg"),
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
            PanelId::Layers => 560.0,
        }
    }

    /// Widest a panel may be dragged or sized to. The references table is
    /// column-rich, so it allows double the shared maximum.
    fn max_width(self) -> f32 {
        match self {
            PanelId::ExternalReferences | PanelId::Layers => DOCK_MAX_W * 2.0,
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

/// One group of an edge: pallets shown together, stacked top to bottom, each
/// with an adjustable share of the edge height. An edge shows one group at a
/// time; its icon strip lists every group so the user can switch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DockGroup {
    /// Pallets top → bottom.
    pub panels: Vec<PanelId>,
    /// Relative height of each pallet, parallel to `panels`.
    pub weights: Vec<f32>,
}

impl DockGroup {
    pub fn single(id: PanelId) -> Self {
        Self {
            panels: vec![id],
            weights: vec![1.0],
        }
    }

    pub fn stack(panels: Vec<PanelId>) -> Self {
        let weights = vec![1.0; panels.len()];
        Self { panels, weights }
    }

    /// Keep one sane weight per pallet (configs edited by hand, or written
    /// by an older build).
    fn heal(&mut self) {
        self.weights.resize(self.panels.len(), 1.0);
        for w in &mut self.weights {
            *w = if w.is_finite() && *w > 0.0 {
                w.clamp(MIN_WEIGHT, MAX_WEIGHT)
            } else {
                1.0
            };
        }
    }

    fn insert(&mut self, index: usize, id: PanelId, weight: f32) {
        let index = index.min(self.panels.len());
        self.panels.insert(index, id);
        self.weights.insert(index, weight);
    }

    fn remove(&mut self, id: PanelId) -> Option<(usize, f32)> {
        let i = self.panels.iter().position(|p| *p == id)?;
        self.panels.remove(i);
        Some((i, self.weights.remove(i)))
    }

    fn mean_weight(&self) -> f32 {
        if self.weights.is_empty() {
            1.0
        } else {
            self.weights.iter().sum::<f32>() / self.weights.len() as f32
        }
    }
}

/// On-disk forms of one entry of an edge list. Configs from before groups
/// stored each stacked pallet as a bare id; this branch's earlier builds
/// stored tab groups.
#[derive(Deserialize)]
#[serde(untagged)]
enum GroupRepr {
    Single(PanelId),
    Stack {
        panels: Vec<PanelId>,
        #[serde(default)]
        weights: Vec<f32>,
    },
    Tabs { tabs: Vec<PanelId> },
}

/// Read an edge list. Bare ids were pallets stacked on the edge, all shown
/// at once, so they become one group to keep the old layout.
fn edge_from_reprs<'de, D>(de: D) -> Result<Vec<DockGroup>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let reprs = Vec::<GroupRepr>::deserialize(de)?;
    let mut groups = Vec::new();
    let mut legacy: Vec<PanelId> = Vec::new();
    for repr in reprs {
        match repr {
            GroupRepr::Single(id) => legacy.push(id),
            GroupRepr::Stack { panels, weights } => groups.push(DockGroup { panels, weights }),
            GroupRepr::Tabs { tabs } => groups.push(DockGroup::stack(tabs)),
        }
    }
    if !legacy.is_empty() {
        groups.insert(0, DockGroup::stack(legacy));
    }
    for g in &mut groups {
        g.heal();
    }
    groups.retain(|g| !g.panels.is_empty());
    Ok(groups)
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
/// Smallest on-screen height of a stacked pallet while dragging a splitter.
pub const GROUP_MIN_H: f32 = 80.0;
const MIN_WEIGHT: f32 = 0.05;
const MAX_WEIGHT: f32 = 20.0;

/// The whole dock layout: per edge its groups and which one shows, the
/// floating panels, plus per-panel settings. Only the persisted layout lives
/// here; transient drag/hover state is app state (see `update::mod`) so it is
/// skipped by serialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockState {
    /// Groups on the left edge, in icon-strip order.
    #[serde(deserialize_with = "edge_from_reprs")]
    pub left: Vec<DockGroup>,
    /// Groups on the right edge, in icon-strip order.
    #[serde(deserialize_with = "edge_from_reprs")]
    pub right: Vec<DockGroup>,
    /// The group each edge shows (left, right).
    pub shown: (usize, usize),
    /// Auto-hide per edge (left, right): the edge shows only its icon strip
    /// until an icon is hovered.
    pub auto_hide: (bool, bool),
    /// Floating panels, back → front.
    pub floating: Vec<FloatPanel>,
    /// Per-panel width / auto-collapse settings, keyed by `PanelId`. A
    /// group's pallets share one width, so each group keeps its own.
    pub panels: BTreeMap<PanelId, DockPanel>,
}

impl Default for DockState {
    fn default() -> Self {
        Self {
            left: vec![DockGroup::single(PanelId::Properties)],
            right: vec![DockGroup::single(PanelId::BlockPalette)],
            shown: (0, 0),
            auto_hide: (false, false),
            floating: Vec::new(),
            panels: BTreeMap::new(),
        }
    }
}

impl DockState {
    /// The groups on `side`, in icon-strip order.
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

    /// The group `side` shows (it may hold no open pallet; see the app's
    /// `dock_shown_group` for the one actually drawn).
    pub fn shown(&self, side: DockSide) -> usize {
        match side {
            DockSide::Left => self.shown.0,
            DockSide::Right => self.shown.1,
        }
    }

    /// Make group `gi` the one `side` shows. Returns whether it changed.
    pub fn show_group(&mut self, side: DockSide, gi: usize) -> bool {
        let slot = match side {
            DockSide::Left => &mut self.shown.0,
            DockSide::Right => &mut self.shown.1,
        };
        let changed = *slot != gi;
        *slot = gi;
        changed
    }

    /// Show the group holding `id`. Returns whether the shown group changed.
    pub fn show_group_of(&mut self, id: PanelId) -> bool {
        match self.location(id) {
            Some((side, gi)) => self.show_group(side, gi),
            None => false,
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
                g.heal();
                let keep: Vec<bool> = g.panels.iter().map(|id| seen.insert(*id)).collect();
                let mut k = keep.iter();
                g.weights.retain(|_| *k.next().expect("parallel"));
                let mut k = keep.iter();
                g.panels.retain(|_| *k.next().expect("parallel"));
            }
            groups.retain(|g| !g.panels.is_empty());
            let len = groups.len();
            let shown = self.shown(side).min(len.saturating_sub(1));
            self.show_group(side, shown);
        }
        self.floating.retain(|f| seen.insert(f.id));
    }

    /// Where (if anywhere) a panel is docked: its side and group index.
    pub fn location(&self, id: PanelId) -> Option<(DockSide, usize)> {
        for side in [DockSide::Left, DockSide::Right] {
            if let Some(i) = self.groups(side).iter().position(|g| g.panels.contains(&id)) {
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

    /// The floating auto-hide flag of `id` (a docked pallet follows its
    /// edge instead; see [`Self::auto_hides`]).
    pub fn auto_collapse(&self, id: PanelId) -> bool {
        self.settings(id).auto_collapse
    }

    /// Whether `side` auto-hides.
    pub fn edge_auto_hide(&self, side: DockSide) -> bool {
        match side {
            DockSide::Left => self.auto_hide.0,
            DockSide::Right => self.auto_hide.1,
        }
    }

    /// Whether `id` auto-hides where it is: by its edge's setting when
    /// docked, by its own when floating.
    pub fn auto_hides(&self, id: PanelId) -> bool {
        match self.location(id) {
            Some((side, _)) => self.edge_auto_hide(side),
            None => self.auto_collapse(id),
        }
    }

    /// Flip auto-hide where `id` is: its whole edge when docked, the pallet
    /// itself when floating. Returns the new state.
    pub fn toggle_auto_hide(&mut self, id: PanelId) -> bool {
        let on = !self.auto_hides(id);
        match self.location(id) {
            Some((DockSide::Left, _)) => self.auto_hide.0 = on,
            Some((DockSide::Right, _)) => self.auto_hide.1 = on,
            None => self.set_auto_collapse(id, on),
        }
        on
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

    /// The saved width a group's pallets share: the widest of them, so a
    /// group whose pallets were sized apart renders at one width.
    pub fn group_width(&self, side: DockSide, group: usize) -> f32 {
        self.groups(side)[group]
            .panels
            .iter()
            .map(|id| self.settings(*id).width)
            .fold(DOCK_MIN_W, f32::max)
    }

    /// Set the width of `id` and of every other pallet in its group.
    pub fn set_group_width(&mut self, id: PanelId, width: f32) {
        let panels = match self.location(id) {
            Some((side, gi)) => self.groups(side)[gi].panels.clone(),
            None => vec![id],
        };
        for p in panels {
            self.set_width(p, width);
        }
    }

    /// Take `id` out of the layout. Returns the group it left, its position
    /// and weight there, and whether the group disappeared (it held nothing
    /// else).
    fn detach(&mut self, id: PanelId) -> Option<(DockSide, usize, usize, f32, bool)> {
        self.floating.retain(|f| f.id != id);
        let (side, gi) = self.location(id)?;
        let (pos, weight) = self.groups_mut(side)[gi].remove(id).expect("located");
        if !self.groups(side)[gi].panels.is_empty() {
            return Some((side, gi, pos, weight, false));
        }
        self.groups_mut(side).remove(gi);
        // Keep showing the same group, or its neighbour when it went away.
        let shown = self.shown(side);
        if shown > gi || (shown == gi && shown > 0 && shown >= self.groups(side).len()) {
            self.show_group(side, shown - 1);
        }
        Some((side, gi, pos, weight, true))
    }

    /// Dock `id` as a new group of its own on `side` at insertion `index`
    /// (0 = first, `len` = last), counted before the move, and show it.
    /// Returns whether the layout changed.
    pub fn dock(&mut self, id: PanelId, side: DockSide, index: usize) -> bool {
        let before = self.clone();
        let mut index = index;
        if let Some((old_side, old_gi, _, _, removed)) = self.detach(id) {
            if old_side == side && removed && old_gi < index {
                index -= 1;
            }
        }
        let groups = self.groups_mut(side);
        let index = index.min(groups.len());
        groups.insert(index, DockGroup::single(id));
        self.show_group(side, index);
        *self != before
    }

    /// Stack `id` into group `group` on `side` at position `index` (0 = top,
    /// `len` = bottom), both counted before the move, and show the group. A
    /// pallet already in the group moves within it. A joining pallet takes on
    /// the group's width. Returns whether the layout changed.
    pub fn join_group(&mut self, id: PanelId, side: DockSide, group: usize, index: usize) -> bool {
        if group >= self.groups(side).len() {
            return false;
        }
        // A pallet alone in its group has nowhere to move within it (and
        // detaching it would remove the group being joined).
        if self.groups(side)[group].panels == [id] {
            return self.show_group(side, group);
        }
        let before = self.clone();
        let width = self.group_width(side, group);
        let joining = !self.groups(side)[group].panels.contains(&id);
        let mut group = group;
        let mut index = index;
        let mut weight = self.groups(side)[group].mean_weight();
        if let Some((old_side, old_gi, pos, w, removed)) = self.detach(id) {
            if old_side == side && old_gi == group {
                // Moving within the group keeps the pallet's height.
                weight = w;
                if pos < index {
                    index -= 1;
                }
            } else if old_side == side && removed && old_gi < group {
                group -= 1;
            }
        }
        self.groups_mut(side)[group].insert(index, id, weight);
        if joining {
            self.set_width(id, width);
        }
        self.show_group(side, group);
        *self != before
    }

    /// Move group `group` on `side` to insertion `index` on `to` (counted
    /// before the move) and show it there. Returns whether the layout changed.
    pub fn move_group(&mut self, side: DockSide, group: usize, to: DockSide, index: usize) -> bool {
        if group >= self.groups(side).len() {
            return false;
        }
        let mut index = index;
        if side == to {
            if index == group || index == group + 1 {
                return false;
            }
            if group < index {
                index -= 1;
            }
        }
        let shown_here = self.shown(side);
        let g = self.groups_mut(side).remove(group);
        if side != to && shown_here > group {
            self.show_group(side, shown_here - 1);
        } else if side != to && shown_here >= self.groups(side).len() && shown_here > 0 {
            self.show_group(side, shown_here - 1);
        }
        let dest = self.groups_mut(to);
        let index = index.min(dest.len());
        dest.insert(index, g);
        self.show_group(to, index);
        true
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

    /// Move the boundary between pallets `upper` and `lower` of group
    /// `group` on `side` by `delta` weight units, keeping their combined
    /// share and each above `min_weight`.
    pub fn shift_split(
        &mut self,
        side: DockSide,
        group: usize,
        upper: usize,
        lower: usize,
        delta: f32,
        min_weight: f32,
    ) {
        let Some(g) = self.groups_mut(side).get_mut(group) else {
            return;
        };
        let w = &mut g.weights;
        if upper >= w.len() || lower >= w.len() || upper == lower {
            return;
        }
        let total = w[upper] + w[lower];
        let min = min_weight.clamp(MIN_WEIGHT, total * 0.5);
        let up = (w[upper] + delta).clamp(min, total - min);
        w[upper] = up;
        w[lower] = total - up;
    }

    /// Give every pallet of group `group` on `side` the same height again.
    pub fn reset_splits(&mut self, side: DockSide, group: usize) {
        if let Some(g) = self.groups_mut(side).get_mut(group) {
            g.weights.iter_mut().for_each(|w| *w = 1.0);
        }
    }
}

/// Where a dragged panel lands on release.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropTarget {
    /// A new group on `side` at insertion `index` (0 = first, len = last).
    Edge { side: DockSide, index: usize },
    /// Stacked into group `group` on `side` at position `index` (counted
    /// before the move; 0 = top).
    Join {
        side: DockSide,
        group: usize,
        index: usize,
    },
    /// Floating with its top-left corner at (`x`, `y`).
    Float { x: f32, y: f32 },
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
/// Width of the grabbable divider between a docked slot and the viewport.
pub const DOCK_DIVIDER_W: f32 = 5.0;
/// Width of an edge's vertical icon strip (the groups' tabs).
pub const DOCK_STRIP_W: f32 = 40.0;
/// Height of one pallet icon in the strip.
pub const STRIP_CELL_H: f32 = 32.0;
/// Space above and below a group's icons, inside the group.
pub const STRIP_PAD: f32 = 6.0;
/// Hairline divider between groups.
pub const STRIP_DIVIDER_H: f32 = 1.0;
/// Space between the last group and the + button.
pub const STRIP_PLUS_GAP: f32 = 8.0;
/// Width of the band along a group's window-side edge that drags the whole
/// group (the icons drag single pallets).
pub const STRIP_GRIP_W: f32 = 8.0;
/// How far the "new group" zone around a divider reaches into the icons on
/// either side.
const STRIP_NEW_GROUP_SLOP: f32 = 4.0;

/// Where one group sits in an edge's icon strip, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripGroup {
    /// Group index on the edge.
    pub group: usize,
    /// Top of the group's row (just below the divider above it).
    pub top: f32,
    /// Top of the group's first icon.
    pub icons_top: f32,
    /// Number of icons (open pallets).
    pub icons: usize,
}

impl StripGroup {
    /// Bottom of the group's last icon.
    pub fn icons_bottom(&self) -> f32 {
        self.icons_top + self.icons as f32 * STRIP_CELL_H
    }

    /// Bottom of the group's row (where its divider starts).
    pub fn bottom(&self) -> f32 {
        self.icons_bottom() + STRIP_PAD
    }
}

/// Lay out the icon strip for `groups` = (group index, open pallet count),
/// returning each group's place and the top of the + button below them.
pub fn strip_layout(groups: &[(usize, usize)]) -> (Vec<StripGroup>, f32) {
    let mut y = 0.0;
    let placed = groups
        .iter()
        .map(|&(group, icons)| {
            let g = StripGroup {
                group,
                top: y,
                icons_top: y + STRIP_PAD,
                icons,
            };
            y = g.bottom() + STRIP_DIVIDER_H;
            g
        })
        .collect();
    (placed, y + STRIP_PLUS_GAP)
}

/// What a dragged pallet released at height `y` over the icon strip joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripHit {
    /// A new group before group `group`; `None` = after the last group.
    NewGroup(Option<usize>),
    /// Group `group`, at position `index` among its open pallets.
    Tab { group: usize, index: usize },
}

/// Classify a pointer at strip height `y`: on a divider (or the gap above
/// the first group) starts a new group there, over a group's icons joins it
/// between the nearest icons, below every group starts a new last group.
pub fn strip_hit(y: f32, layout: &[StripGroup]) -> StripHit {
    for g in layout {
        if y < g.icons_top + STRIP_NEW_GROUP_SLOP {
            return StripHit::NewGroup(Some(g.group));
        }
        if y < g.icons_bottom() - STRIP_NEW_GROUP_SLOP {
            let index = (((y - g.icons_top) / STRIP_CELL_H).round().max(0.0) as usize)
                .min(g.icons);
            return StripHit::Tab {
                group: g.group,
                index,
            };
        }
    }
    StripHit::NewGroup(None)
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
    .on_double_click(Message::Dock(DockMsg::FloatOut(id)))
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
    fn legacy_bare_ids_load_as_one_stacked_group() {
        // Before groups, every id on an edge was shown stacked; keep that.
        let json = r#"{"left":["properties","block_palette"],"right":[]}"#;
        let state: DockState = serde_json::from_str(json).unwrap();
        assert_eq!(state.left.len(), 1);
        assert_eq!(
            state.left[0].panels,
            vec![PanelId::Properties, PanelId::BlockPalette]
        );
        assert_eq!(state.left[0].weights, vec![1.0, 1.0]);
        assert!(state.floating.is_empty());
    }

    #[test]
    fn layout_round_trips_through_serde() {
        let mut state = DockState::default();
        state.join_group(PanelId::Browser, DockSide::Left, 0, 1);
        state.dock(PanelId::Count, DockSide::Left, 1);
        state.float(FloatPanel {
            id: PanelId::SheetSetManager,
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
    fn join_group_stacks_and_shows_the_group() {
        let mut state = DockState::default();
        state.set_width(PanelId::Properties, 320.0);
        // Below Properties in the left group.
        assert!(state.join_group(PanelId::BlockPalette, DockSide::Left, 0, 1));
        assert!(state.right.is_empty(), "its old group disappears");
        assert_eq!(
            state.left[0].panels,
            vec![PanelId::Properties, PanelId::BlockPalette]
        );
        assert_eq!(state.left[0].weights.len(), 2);
        // It takes on the group's width.
        assert_eq!(state.settings(PanelId::BlockPalette).width, 320.0);
        // Moving it above Properties keeps it in the group.
        assert!(state.join_group(PanelId::BlockPalette, DockSide::Left, 0, 0));
        assert_eq!(state.left[0].panels[0], PanelId::BlockPalette);
        // Dropping it where it already is changes nothing.
        assert!(!state.join_group(PanelId::BlockPalette, DockSide::Left, 0, 1));
        // Nor does dropping a pallet that is alone in its group onto it.
        state.dock(PanelId::Count, DockSide::Right, 0);
        for index in 0..=1 {
            assert!(!state.join_group(PanelId::Count, DockSide::Right, 0, index));
        }
        assert_eq!(state.right[0].panels, vec![PanelId::Count]);
    }

    #[test]
    fn join_group_index_counts_groups_before_the_move() {
        let mut state = DockState::default();
        state.left = vec![
            DockGroup::single(PanelId::BlockPalette),
            DockGroup::single(PanelId::Properties),
        ];
        state.right.clear();
        // Group 1 (Properties) is group 0 once the palette's group is gone.
        assert!(state.join_group(PanelId::BlockPalette, DockSide::Left, 1, 1));
        assert_eq!(state.left.len(), 1);
        assert_eq!(
            state.left[0].panels,
            vec![PanelId::Properties, PanelId::BlockPalette]
        );
        assert_eq!(state.shown(DockSide::Left), 0);
    }

    #[test]
    fn dock_starts_a_new_group_and_shows_it() {
        let mut state = DockState::default();
        state.join_group(PanelId::BlockPalette, DockSide::Left, 0, 1);
        assert!(state.dock(PanelId::BlockPalette, DockSide::Left, 1));
        assert_eq!(state.left.len(), 2);
        assert_eq!(state.left[0].panels, vec![PanelId::Properties]);
        assert_eq!(state.left[1].panels, vec![PanelId::BlockPalette]);
        assert_eq!(state.shown(DockSide::Left), 1);
        assert!(state.show_group_of(PanelId::Properties));
        assert_eq!(state.shown(DockSide::Left), 0);
    }

    #[test]
    fn removing_the_shown_group_keeps_a_valid_shown_index() {
        let mut state = DockState::default();
        state.dock(PanelId::Browser, DockSide::Left, 1);
        assert_eq!(state.shown(DockSide::Left), 1);
        // Floating Browser removes the last group; the edge shows group 0.
        state.float(FloatPanel {
            id: PanelId::Browser,
            x: 0.0,
            y: 0.0,
            w: 250.0,
            h: 300.0,
        });
        assert_eq!(state.left.len(), 1);
        assert_eq!(state.shown(DockSide::Left), 0);
    }

    #[test]
    fn move_group_reorders_and_crosses_edges() {
        let mut state = DockState::default();
        state.left.push(DockGroup::single(PanelId::Browser));
        state.left.push(DockGroup::single(PanelId::Count));
        // Same spot (before itself or after itself) changes nothing.
        assert!(!state.move_group(DockSide::Left, 1, DockSide::Left, 1));
        assert!(!state.move_group(DockSide::Left, 1, DockSide::Left, 2));
        assert!(state.move_group(DockSide::Left, 0, DockSide::Left, 3));
        assert_eq!(state.left[2].panels, vec![PanelId::Properties]);
        assert_eq!(state.shown(DockSide::Left), 2, "the moved group shows");
        assert!(state.move_group(DockSide::Left, 2, DockSide::Right, 0));
        assert_eq!(state.right[0].panels, vec![PanelId::Properties]);
        assert_eq!(state.left.len(), 2);
        assert!(state.shown(DockSide::Left) < 2);
    }

    #[test]
    fn shift_split_keeps_combined_share_and_minimum() {
        let mut state = DockState::default();
        state.left = vec![DockGroup::stack(vec![PanelId::BlockPalette, PanelId::Properties])];
        state.shift_split(DockSide::Left, 0, 0, 1, 0.5, 0.1);
        assert_eq!(state.left[0].weights, vec![1.5, 0.5]);
        state.shift_split(DockSide::Left, 0, 0, 1, 10.0, 0.1);
        assert!((state.left[0].weights[1] - 0.1).abs() < 1e-6);
        assert!((state.left[0].weights.iter().sum::<f32>() - 2.0).abs() < 1e-6);
        state.reset_splits(DockSide::Left, 0);
        assert_eq!(state.left[0].weights, vec![1.0, 1.0]);
    }

    #[test]
    fn auto_hide_is_per_edge_for_docked_and_per_pallet_when_floating() {
        let mut state = DockState::default();
        state.join_group(PanelId::Browser, DockSide::Left, 0, 1);
        assert!(state.toggle_auto_hide(PanelId::Properties));
        // The whole left edge auto-hides, the right edge does not.
        assert!(state.auto_hides(PanelId::Browser));
        assert!(!state.auto_hides(PanelId::BlockPalette));
        // A floating pallet keeps its own setting.
        state.float(FloatPanel {
            id: PanelId::Count,
            x: 0.0,
            y: 0.0,
            w: 250.0,
            h: 300.0,
        });
        assert!(!state.auto_hides(PanelId::Count));
        assert!(state.toggle_auto_hide(PanelId::Count));
        assert!(state.auto_hides(PanelId::Count));
        assert!(!state.edge_auto_hide(DockSide::Right));
    }

    #[test]
    fn ensure_settings_drops_duplicates_and_clamps_the_shown_group() {
        let mut state = DockState::default();
        state.right.push(DockGroup::single(PanelId::Properties));
        state.shown = (5, 7);
        state.ensure_settings();
        assert_eq!(state.location(PanelId::Properties), Some((DockSide::Left, 0)));
        assert_eq!(state.right.len(), 1);
        assert_eq!(state.shown, (0, 0));
    }

    #[test]
    fn strip_hit_maps_dividers_icons_and_the_end() {
        // Two groups: three icons, then one.
        let (layout, plus_top) = strip_layout(&[(0, 3), (2, 1)]);
        let g0 = layout[0];
        let g1 = layout[1];
        assert_eq!(g0.icons_top, STRIP_PAD);
        assert_eq!(g1.top, g0.bottom() + STRIP_DIVIDER_H);
        assert_eq!(plus_top, g1.bottom() + STRIP_DIVIDER_H + STRIP_PLUS_GAP);
        // Above the first icon: a new group before group 0.
        assert_eq!(strip_hit(2.0, &layout), StripHit::NewGroup(Some(0)));
        // Top half of the first icon: into group 0 at the top.
        assert_eq!(
            strip_hit(g0.icons_top + 10.0, &layout),
            StripHit::Tab { group: 0, index: 0 }
        );
        // Between the first and second icon of group 0.
        assert_eq!(
            strip_hit(g0.icons_top + STRIP_CELL_H + 4.0, &layout),
            StripHit::Tab { group: 0, index: 1 }
        );
        // Lower half of the last icon: into group 0 at the end.
        assert_eq!(
            strip_hit(g0.icons_bottom() - 8.0, &layout),
            StripHit::Tab { group: 0, index: 3 }
        );
        // On the divider between the groups: a new group before group 2.
        assert_eq!(strip_hit(g0.bottom(), &layout), StripHit::NewGroup(Some(2)));
        // Below everything: a new last group.
        assert_eq!(strip_hit(plus_top + 20.0, &layout), StripHit::NewGroup(None));
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
    fn every_panel_has_its_own_icon() {
        let ids = PanelId::ALL;
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
    fn slot_spans_follow_weights() {
        assert_eq!(
            slot_spans(&[1.0, 3.0], 400.0),
            vec![(0.0, 100.0), (100.0, 400.0)]
        );
        assert!(slot_spans(&[], 400.0).is_empty());
    }
}
