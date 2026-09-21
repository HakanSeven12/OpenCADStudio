//! General edge-stack docking for side panels.
//!
//! Any number of dockable panels (Properties, the block palette, future
//! palettes) live in an ordered vertical stack on the left or right edge of
//! the drawing view. This module owns the persisted layout (which panels are
//! docked, on which side, in what vertical order, at what width, and whether
//! each auto-collapses) plus the pure geometry used to render and hover an
//! edge's stacked panels.

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
}

impl PanelId {
    /// Localized-friendly display name used by the collapsed/edge chrome.
    pub fn title(self) -> &'static str {
        match self {
            PanelId::Properties => "Properties",
            PanelId::BlockPalette => "Block Palette",
            PanelId::ExternalReferences => "External References",
            PanelId::Browser => "Browser",
        }
    }

    /// Default dock width for a freshly-created panel instance.
    fn default_width(self) -> f32 {
        match self {
            PanelId::Properties => 250.0,
            PanelId::BlockPalette => 260.0,
            PanelId::ExternalReferences => 460.0,
            PanelId::Browser => 230.0,
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

/// The whole dock layout: two ordered per-side stacks plus per-panel settings.
/// Only the persisted layout lives here; transient drag/hover state is app
/// state (see `update::mod`) so it is skipped by serialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockState {
    /// Panels anchored to the left edge, top → bottom.
    pub left: Vec<PanelId>,
    /// Panels anchored to the right edge, top → bottom.
    pub right: Vec<PanelId>,
    /// Per-panel width / auto-collapse settings, keyed by `PanelId`.
    pub panels: BTreeMap<PanelId, DockPanel>,
}

impl Default for DockState {
    fn default() -> Self {
        Self {
            left: vec![PanelId::Properties],
            right: vec![PanelId::BlockPalette],
            panels: BTreeMap::new(),
        }
    }
}

impl DockState {
    /// Edge-collection of the panels ordered for rendering on `side`.
    fn stack(&self, side: DockSide) -> &[PanelId] {
        match side {
            DockSide::Left => &self.left,
            DockSide::Right => &self.right,
        }
    }

    fn stack_mut(&mut self, side: DockSide) -> &mut Vec<PanelId> {
        match side {
            DockSide::Left => &mut self.left,
            DockSide::Right => &mut self.right,
        }
    }

    /// Guarantee every known `PanelId` has a settings entry, so rendering and
    /// resize never hit a missing configuration. Also a cheap heal for configs
    /// written by an older version.
    pub fn ensure_settings(&mut self) {
        for id in [
            PanelId::Properties,
            PanelId::BlockPalette,
            PanelId::ExternalReferences,
            PanelId::Browser,
        ] {
            self.panels.entry(id).or_insert_with(|| DockPanel::for_id(id));
        }
    }

    /// Where (if anywhere) a panel is currently docked.
    pub fn location(&self, id: PanelId) -> Option<(DockSide, usize)> {
        for side in [DockSide::Left, DockSide::Right] {
            if let Some(i) = self.stack(side).iter().position(|p| *p == id) {
                return Some((side, i));
            }
        }
        None
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

    /// Dock `id` to `side` at `index` (clamped), removing it from any other
    /// stack first. Returns whether the layout actually changed.
    pub fn dock(&mut self, id: PanelId, side: DockSide, index: usize) -> bool {
        if let Some((old_side, old_i)) = self.location(id) {
            if old_side == side && old_i == index {
                return false;
            }
            self.stack_mut(old_side).remove(old_i);
        }
        let stack = self.stack_mut(side);
        let index = index.min(stack.len());
        stack.insert(index, id);
        true
    }
}

/// Insertion index (0..=total) for dropping a panel whose pointer is at
/// screen-local `y` within an edge column of `avail` height holding `total`
/// slots. Used to compute the live drag target index while reordering.
pub fn drop_index(y: f32, total: usize, avail: f32) -> usize {
    if total == 0 || avail <= 0.0 {
        return 0;
    }
    let h = avail / total as f32;
    let idx = (y / h).round() as usize;
    // A single full-height panel must not offer a top/bottom split: any drop
    // lands in the one available slot.
    if total <= 1 {
        return 0;
    }
    idx.min(total)
}

/// Smallest docked width a panel may be dragged or sized to.
pub const DOCK_MIN_W: f32 = 200.0;
/// Largest docked width a panel may be dragged or sized to.
pub const DOCK_MAX_W: f32 = 600.0;

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
    fn drop_index_maps_between_zero_and_total() {
        assert_eq!(drop_index(0.0, 3, 300.0), 0);
        assert_eq!(drop_index(100.0, 3, 300.0), 1);
        assert_eq!(drop_index(299.0, 3, 300.0), 3);
        assert_eq!(drop_index(300.0, 3, 300.0), 3);
        assert_eq!(drop_index(50.0, 0, 300.0), 0);
    }

    #[test]
    fn drop_index_single_slot_is_always_zero() {
        // One panel sharing the full edge height: any pointer y lands in the
        // single slot, so the insertion index must be 0 (no top/bottom split).
        assert_eq!(drop_index(5.0, 1, 900.0), 0);
        assert_eq!(drop_index(450.0, 1, 900.0), 0);
        assert_eq!(drop_index(895.0, 1, 900.0), 0);
    }

    #[test]
    fn drop_index_multi_slot_maps_position() {
        // Two panels on an edge produce insertion positions 0..=total: pointer
        // near the top lands before the first slot (0); pointer near the bottom
        // of the last slot lands after the last (== total, append).
        assert_eq!(drop_index(10.0, 2, 900.0), 0);
        assert_eq!(drop_index(890.0, 2, 900.0), 2);
    }
}