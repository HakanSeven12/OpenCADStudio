//! Adaptive ribbon panels.
//!
//! Lays the active module's panels on one row. When they don't all fit, the row
//! degrades **from the right**, one panel at a time: first a panel shrinks to
//! compact icon columns, then it collapses to a title button. If even the
//! all-collapsed row overflows, every button drops to its small icon together,
//! then the buttons are squeezed. The row's height tracks the tallest shown
//! panel, so it shrinks as the panels do.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Renderer as _, Shell};
use iced::{
    Background, Border, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size,
    Theme, Vector,
};

use crate::app::Message;

/// One panel in its four density renderings. `tight` is the collapsed button
/// with a small representative icon instead of the large one — the last step
/// before the buttons are squeezed together. An open flyout draws the panel's
/// own `full` rendering; see `FlyoutOverlay`. `elements` is indexed by `Level`.
pub(crate) struct Panel<'a> {
    pub(crate) id: String,
    pub(crate) elements: [Element<'a, Message>; 4],
}

// Number of tree slots per panel: [full, compact, button, tight].
const SLOTS: usize = 4;

/// A panel's degradation level; also the offset of its shown element within the
/// panel's tree slots. Degrade order (from the right): `Full` → `Compact` →
/// `Collapsed` (big-icon button) → `Tight` (small-icon button).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Level {
    Full = 0,
    Compact = 1,
    Collapsed = 2,
    Tight = 3,
}

/// Tree-slot index of panel `i`'s rendering at `level`
/// (`SLOTS` slots per panel: `[full, compact, button, tight]`).
fn slot(i: usize, level: Level) -> usize {
    i * SLOTS + level as usize
}

/// When even the all-tight row still overflows, the collapsed buttons are pulled
/// together by up to this many px per gap — reclaiming their edge padding —
/// before anything is clipped. Mirrors the tab bar squeezing its gaps shut
/// before wrapping.
const MAX_PANEL_SQUEEZE: f32 = 8.0;

/// How the ribbon tool panels are sized. `Auto` adapts to the window width (the
/// step-by-step degradation); the others pin every panel to one density so the
/// user can override the automatic choice. The selection is persisted.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize,
)]
pub enum CollapseMode {
    /// Size panels to the window: degrade from the right as space runs out.
    #[default]
    Auto,
    /// Always full-size panels (large buttons), even if they overflow.
    Full,
    /// Always compact panels (small icon columns).
    Compact,
    /// Always collapsed to title buttons.
    Collapsed,
}

impl CollapseMode {
    /// Every mode, in dropdown order.
    pub const ALL: &'static [CollapseMode] = &[
        CollapseMode::Auto,
        CollapseMode::Full,
        CollapseMode::Compact,
        CollapseMode::Collapsed,
    ];

    /// Label shown in the dropdown.
    pub fn label(self) -> &'static str {
        match self {
            CollapseMode::Auto => "Auto",
            CollapseMode::Full => "Full",
            CollapseMode::Compact => "Compact",
            CollapseMode::Collapsed => "Collapsed",
        }
    }


    /// The degradation level every panel is pinned to, or `None` for `Auto`.
    fn forced_level(self) -> Option<Level> {
        match self {
            CollapseMode::Auto => None,
            CollapseMode::Full => Some(Level::Full),
            CollapseMode::Compact => Some(Level::Compact),
            CollapseMode::Collapsed => Some(Level::Collapsed),
        }
    }

}

impl std::fmt::Display for CollapseMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Natural widths of one panel at each degradation level. Only the widths drive
/// the degradation decision; the decision itself is pure and tested.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Widths {
    /// Width of the full-size panel (`Level::Full`).
    pub full: f32,
    /// Width of the compact icon-column panel (`Level::Compact`).
    pub compact: f32,
    /// Width of the collapsed title button (`Level::Collapsed`).
    pub button: f32,
    /// Width of the tightest small-icon button (`Level::Tight`).
    pub tight: f32,
}

/// Choose the degradation level per panel. `Auto` degrades from the right, one
/// panel at a time: first Full -> Compact, then Compact -> Collapsed, each phase
/// only while the row still overflows; if even the all-collapsed row overflows,
/// every collapsed panel drops to Tight at once. Forced modes pin everything.
pub(crate) fn decide_levels(
    mode: CollapseMode,
    widths: &[Widths],
    max_w: f32,
) -> Vec<Level> {
    if let Some(level) = mode.forced_level() {
        return vec![level; widths.len()];
    }
    let width_of = |lv: Level, i: usize| match lv {
        Level::Full => widths[i].full,
        Level::Compact => widths[i].compact,
        Level::Collapsed => widths[i].button,
        Level::Tight => widths[i].tight,
    };
    let total = |levels: &[Level]| -> f32 {
        (0..levels.len()).map(|i| width_of(levels[i], i)).sum()
    };
    let mut levels = vec![Level::Full; widths.len()];
    for degraded in [Level::Compact, Level::Collapsed] {
        for i in (0..widths.len()).rev() {
            if total(&levels) <= max_w {
                break;
            }
            levels[i] = degraded;
        }
    }
    if total(&levels) > max_w {
        levels
            .iter_mut()
            .filter(|l| **l == Level::Collapsed)
            .for_each(|l| *l = Level::Tight);
    }
    levels
}

pub(crate) struct CollapsePanels<'a> {
    panels: Vec<Panel<'a>>,
    /// Title of the panel whose flyout is open (if any).
    open: Option<String>,
    /// Fallback row height, used only when there are no panels to measure.
    row_h: f32,
    /// Chosen degradation level per panel; set during layout.
    levels: RefCell<Vec<Level>>,
    /// If set, the measured row height is written here each layout (read when
    /// anchoring dropdowns below the ribbon).
    height_out: Option<Arc<AtomicU32>>,
    /// If set, `true` is written here whenever the row is in the tight state
    /// (some panel dropped to its small icon), so the tab bar can react.
    tight_out: Option<Arc<AtomicBool>>,
    /// User-chosen density. `Auto` runs the width-based degradation; the others
    /// pin every panel to one level regardless of the window width.
    mode: CollapseMode,
}

impl<'a> CollapsePanels<'a> {
    pub fn new(panels: Vec<Panel<'a>>, open: Option<String>, row_h: f32) -> Self {
        let n = panels.len();
        Self {
            panels,
            open,
            row_h,
            levels: RefCell::new(vec![Level::Full; n]),
            height_out: None,
            tight_out: None,
            mode: CollapseMode::Auto,
        }
    }

    /// Report the measured row height into `out` on every layout.
    pub fn report_height(mut self, out: Arc<AtomicU32>) -> Self {
        self.height_out = Some(out);
        self
    }

    /// Report whether the row is in the tight state into `out` on every layout.
    pub fn report_tight(mut self, out: Arc<AtomicBool>) -> Self {
        self.tight_out = Some(out);
        self
    }

    /// Pin the panels to a density (or `Auto` to size by window width).
    pub fn mode(mut self, mode: CollapseMode) -> Self {
        self.mode = mode;
        self
    }

    /// The shown element of panel `i` at `level` — an index into its `elements`
    /// array, which the tree slots mirror (`slot(i, level)`).
    fn shown(&self, i: usize, level: Level) -> &Element<'a, Message> {
        &self.panels[i].elements[level as usize]
    }

    fn shown_mut(&mut self, i: usize, level: Level) -> &mut Element<'a, Message> {
        &mut self.panels[i].elements[level as usize]
    }

    fn levels_snapshot(&self, n: usize) -> Vec<Level> {
        let mut v = self.levels.borrow().clone();
        v.resize(n, Level::Full);
        v
    }
}

impl<'a> Widget<Message, Theme, Renderer> for CollapsePanels<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        let mut refs = Vec::with_capacity(self.panels.len() * SLOTS);
        for p in &mut self.panels {
            refs.extend(p.elements.iter_mut());
        }
        tree.diff_children(&mut refs);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max_w = limits.max().width;
        let natural = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, f32::INFINITY));
        let n = self.panels.len();

        // Auto needs each panel's natural width at all four densities to pick the
        // degradation; heights drive the row height. A forced mode pins every
        // panel to one level, so it measures nothing at all.
        let mut widths = Vec::with_capacity(n);
        let mut heights = Vec::with_capacity(n);
        let auto = self.mode == CollapseMode::Auto;
        if auto {
            for i in 0..n {
                let f = self.panels[i].elements[Level::Full as usize].as_widget_mut()
                    .layout(&mut tree.children[slot(i, Level::Full)], renderer, &natural)
                    .size();
                let c = self.panels[i].elements[Level::Compact as usize].as_widget_mut()
                    .layout(&mut tree.children[slot(i, Level::Compact)], renderer, &natural)
                    .size();
                let b = self.panels[i].elements[Level::Collapsed as usize].as_widget_mut()
                    .layout(&mut tree.children[slot(i, Level::Collapsed)], renderer, &natural)
                    .size();
                let t = self.panels[i].elements[Level::Tight as usize].as_widget_mut()
                    .layout(&mut tree.children[slot(i, Level::Tight)], renderer, &natural)
                    .size();
                widths.push(Widths {
                    full: f.width,
                    compact: c.width,
                    button: b.width,
                    tight: t.width,
                });
                heights.push([f.height, c.height, b.height, t.height]);
            }
        }
        let levels = if let Some(level) = self.mode.forced_level() {
            vec![level; n]
        } else {
            decide_levels(self.mode, &widths, max_w)
        };

        // The row is "tight" once any panel has dropped to its small icon — the
        // last, most cramped state. The tab bar hides its mode selector then.
        if let Some(out) = &self.tight_out {
            out.store(
                levels.iter().any(|&l| l == Level::Tight),
                Ordering::Relaxed,
            );
        }
        *self.levels.borrow_mut() = levels.clone();

        // Same idea as the tab bar squeezing its gaps before wrapping: once every
        // panel is at its tightest and the row STILL overflows, pull the buttons
        // together (up to MAX_PANEL_SQUEEZE per gap, reclaiming their edge
        // padding) so more of them stay on-screen before anything is clipped. In
        // Auto this only fires when everything is already tight, so it never
        // overlaps a full/compact panel; a forced density is left to overflow.
        let squeeze = if auto {
            let width_of = |lv: Level, i: usize| match lv {
                Level::Full => widths[i].full,
                Level::Compact => widths[i].compact,
                Level::Collapsed => widths[i].button,
                Level::Tight => widths[i].tight,
            };
            let total: f32 = (0..n).map(|i| width_of(levels[i], i)).sum();
            if n > 1 && total > max_w {
                ((total - max_w) / (n - 1) as f32).min(MAX_PANEL_SQUEEZE)
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Place the chosen element for each panel left-to-right.
        let mut placed: Vec<(layout::Node, f32, f32)> = Vec::with_capacity(n);
        let mut x = 0.0f32;
        for i in 0..n {
            if i > 0 {
                x -= squeeze;
            }
            let level = levels[i];
            let tree_idx = slot(i, level);
            let node = self.shown_mut(i, level).as_widget_mut().layout(
                &mut tree.children[tree_idx],
                renderer,
                &natural,
            );
            let h = node.size().height;
            let w = node.size().width;
            placed.push((node, x, h));
            x += w;
        }

        // The row is as tall as the tallest shown panel, so the ribbon height
        // shrinks as its panels degrade to shorter collapsed / tight buttons.
        // Auto centers against the heights it measured; a forced mode derives the
        // height from the placed panels (nothing was measured).
        let row_h = if auto {
            if n == 0 {
                self.row_h
            } else {
                (0..n)
                    .map(|i| heights[i][levels[i] as usize])
                    .fold(0.0f32, f32::max)
            }
        } else {
            placed
                .iter()
                .map(|(_, _, h)| *h)
                .fold(self.row_h, f32::max)
        };
        let children: Vec<layout::Node> = placed
            .into_iter()
            .map(|(node, node_x, h)| {
                let y = ((row_h - h) / 2.0).max(0.0);
                node.move_to(Point::new(node_x, y))
            })
            .collect();

        if let Some(out) = &self.height_out {
            out.store(row_h.to_bits(), Ordering::Relaxed);
        }

        layout::Node::with_children(Size::new(x, row_h), children)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let levels = self.levels_snapshot(self.panels.len());
        for (i, child_layout) in layout.children().enumerate() {
            let level = levels[i];
            let tree_idx = slot(i, level);
            self.shown_mut(i, level).as_widget_mut().update(
                &mut tree.children[tree_idx],
                event,
                child_layout,
                cursor,
                renderer,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let levels = self.levels_snapshot(self.panels.len());
        let mut interaction = mouse::Interaction::default();
        for (i, child_layout) in layout.children().enumerate() {
            let level = levels[i];
            let tree_idx = slot(i, level);
            let it = self.shown(i, level).as_widget().mouse_interaction(
                &tree.children[tree_idx],
                child_layout,
                cursor,
                viewport,
                renderer,
            );
            if it != mouse::Interaction::default() {
                interaction = it;
            }
        }
        interaction
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let levels = self.levels_snapshot(self.panels.len());
        for (i, child_layout) in layout.children().enumerate() {
            let level = levels[i];
            let tree_idx = slot(i, level);
            self.shown_mut(i, level).as_widget_mut().operate(
                &mut tree.children[tree_idx],
                child_layout,
                renderer,
                operation,
            );
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let levels = self.levels_snapshot(self.panels.len());
        for (i, child_layout) in layout.children().enumerate() {
            let level = levels[i];
            let tree_idx = slot(i, level);
            self.shown(i, level).as_widget().draw(
                &tree.children[tree_idx],
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );
        }

        // 1px divider between adjacent panels, except between two button-form
        // panels (collapsed or tight — they read better with no line between).
        let is_btn = |lv: Level| lv == Level::Collapsed || lv == Level::Tight;
        let bounds: Vec<Rectangle> = layout.children().map(|l| l.bounds()).collect();
        let wb = layout.bounds();
        for i in 0..self.panels.len().saturating_sub(1) {
            if is_btn(levels[i]) && is_btn(levels[i + 1]) {
                continue;
            }
            let x = bounds[i + 1].x;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x,
                        y: wb.y,
                        width: 1.0,
                        height: wb.height,
                    },
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: true,
                },
                Background::Color(theme.palette().background.neutral.color),
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let levels = self.levels_snapshot(self.panels.len());

        // An open flyout owns the overlay slot.
        if let Some(open_id) = self.open.clone() {
            if let Some(p) = self.panels.iter().position(|pan| pan.id == open_id) {
                // Only a button-form panel (collapsed or tight) shows a flyout.
                let lvl = levels.get(p).copied().unwrap_or(Level::Full);
                if (lvl == Level::Collapsed || lvl == Level::Tight)
                    && layout.children().nth(p).is_some()
                {
                    let child_layout = layout.children().nth(p).unwrap();
                    let b = child_layout.bounds();
                    let anchor =
                        Point::new(b.x + translation.x, b.y + b.height + translation.y);
                    // The flyout is the panel's own `full` rendering (the row
                    // never shows slot 0 while the panel is button-form, so it
                    // is free to double as the overlay content).
                    return Some(overlay::Element::new(Box::new(FlyoutOverlay {
                        content: &mut self.panels[p].elements[Level::Full as usize],
                        tree: &mut tree.children[slot(p, Level::Full)],
                        anchor,
                    })));
                }
            }
        }

        // No flyout: forward the SHOWN children's overlays. Every ribbon
        // tooltip is an iced overlay produced inside these children, so
        // returning None here left all of them permanently dead (#411 — a
        // regression from the day this widget replaced the plain row).
        // Split borrows so each shown child and its tree slot are disjoint.
        let mut overlays = Vec::new();
        let mut tree_rest = tree.children.as_mut_slice();
        for ((i, panel), child_layout) in
            self.panels.iter_mut().enumerate().zip(layout.children())
        {
            if tree_rest.len() < SLOTS {
                break;
            }
            let (chunk, rest) = tree_rest.split_at_mut(SLOTS);
            tree_rest = rest;
            let level = levels.get(i).copied().unwrap_or(Level::Full);
            let child = &mut panel.elements[level as usize];
            if let Some(o) = child.as_widget_mut().overlay(
                &mut chunk[level as usize],
                child_layout,
                _renderer,
                _viewport,
                translation,
            ) {
                overlays.push(o);
            }
        }
        if overlays.is_empty() {
            None
        } else {
            Some(overlay::Group::with_children(overlays).overlay())
        }
    }
}

impl<'a> From<CollapsePanels<'a>> for Element<'a, Message> {
    fn from(w: CollapsePanels<'a>) -> Self {
        Element::new(w)
    }
}

/// Overlay that renders an open panel's flyout anchored below its button and
/// closes it when the user presses outside. Draws the panel's `full` rendering
/// (`content`) inside a thin styled box.
struct FlyoutOverlay<'a, 'b> {
    content: &'b mut Element<'a, Message>,
    tree: &'b mut widget::Tree,
    anchor: Point,
}

impl overlay::Overlay<Message, Theme, Renderer> for FlyoutOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let viewport = Rectangle::with_size(bounds);
        let limits = layout::Limits::new(Size::ZERO, viewport.size());
        let node = self
            .content
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let size = node.size();
        let mut x = self.anchor.x;
        let mut y = self.anchor.y;
        if x + size.width > viewport.width {
            x = (viewport.width - size.width).max(0.0);
        }
        if y + size.height > viewport.height {
            y = (self.anchor.y - size.height).max(0.0);
        }
        layout::Node::with_children(size, vec![node]).translate(Vector::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let b = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds: b,
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                shadow: Shadow::default(),
                snap: true,
            },
            Background::Color(theme.palette().background.weakest.color),
        );
        let child = layout.children().next().unwrap();
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            child,
            cursor,
            &child.bounds(),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let child = layout.children().next().unwrap();
        let vp = child.bounds();

        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            if !cursor.is_over(vp) {
                shell.publish(Message::CloseRibbonDropdown);
                shell.capture_event();
                return;
            }
        }

        self.content
            .as_widget_mut()
            .update(self.tree, event, child, cursor, renderer, shell, &vp);
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let child = layout.children().next().unwrap();
        self.content
            .as_widget_mut()
            .operate(self.tree, child, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let child = layout.children().next().unwrap();
        self.content
            .as_widget()
            .mouse_interaction(self.tree, child, cursor, &child.bounds(), renderer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(full: f32, compact: f32, button: f32, tight: f32) -> Widths {
        Widths {
            full,
            compact,
            button,
            tight,
        }
    }

    /// Forced modes pin every panel to the same level, regardless of width.
    #[test]
    fn forced_modes_pin_every_panel() {
        let widths = [w(200.0, 150.0, 100.0, 50.0), w(180.0, 130.0, 90.0, 40.0)];
        assert_eq!(
            decide_levels(CollapseMode::Full, &widths, 10.0),
            vec![Level::Full, Level::Full]
        );
        assert_eq!(
            decide_levels(CollapseMode::Compact, &widths, 10.0),
            vec![Level::Compact, Level::Compact]
        );
        assert_eq!(
            decide_levels(CollapseMode::Collapsed, &widths, 10.0),
            vec![Level::Collapsed, Level::Collapsed]
        );
    }

    /// A huge Auto width keeps every panel full.
    #[test]
    fn auto_huge_width_keeps_everything_full() {
        let widths = [w(200.0, 150.0, 100.0, 50.0), w(180.0, 130.0, 90.0, 40.0)];
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 1e9),
            vec![Level::Full, Level::Full]
        );
    }

    /// Auto degrades from the right: the overflowing rightmost panel shrinks to
    /// compact first, and stops as soon as the row fits.
    #[test]
    fn auto_degrades_from_the_right_one_panel_at_a_time() {
        let widths = [
            w(50.0, 40.0, 30.0, 20.0),
            w(200.0, 100.0, 60.0, 30.0),
        ];
        // Full row = 250 > 240; rightmost compact => 50 + 100 = 150 <= 240.
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 240.0),
            vec![Level::Full, Level::Compact]
        );
    }

    /// The row only keeps degrading left while it still overflows.
    #[test]
    fn auto_stops_degrading_once_the_row_fits() {
        let widths = [w(50.0, 25.0, 20.0, 15.0), w(50.0, 25.0, 20.0, 15.0)];
        // Full row 100 > 75; rightmost compact => 75 <= 75; left panel stays FULL.
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 75.0),
            vec![Level::Full, Level::Compact]
        );
    }

    /// A single panel past its collapse limit and still overflowing drops right
    /// through to a tight small-icon button.
    #[test]
    fn auto_escalates_a_single_panel_past_collapsed_to_tight() {
        let widths = [w(50.0, 40.0, 30.0, 20.0)];
        // full 50 > 25 -> compact 40 > 25 -> collapsed 30 > 25 -> all-collapsed
        // (still) overflows -> TIGHT.
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 25.0),
            vec![Level::Tight]
        );
    }

    /// When even the all-collapsed row overflows, every collapsed panel drops to
    /// tight at once.
    #[test]
    fn auto_cascade_drops_every_collapsed_panel_to_tight() {
        let widths = [w(100.0, 90.0, 50.0, 30.0), w(100.0, 90.0, 50.0, 30.0)];
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 20.0),
            vec![Level::Tight, Level::Tight]
        );
    }

    /// The first (compact) phase completes across the whole row before the
    /// second (collapsed) phase starts: the left panel ends up compact even
    /// though at FULL it would have fit next to the eventually-collapsed right
    /// panel (30 + 40 = 70 <= 100).
    #[test]
    fn auto_runs_the_compact_phase_across_the_row_first() {
        let widths = [w(30.0, 26.0, 15.0, 12.0), w(100.0, 80.0, 40.0, 20.0)];
        // phase compact: right (30+100=130>100 -> compact), left (26+80=110>100 -> compact)
        // phase collapsed: right (26+40=66<=100 after right->collapsed is 26+80=106>100), left (66<=100 -> stop)
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 100.0),
            vec![Level::Compact, Level::Collapsed]
        );
    }

    /// No panels means no levels, in Auto and every forced mode.
    #[test]
    fn no_panels_stays_empty() {
        assert_eq!(decide_levels(CollapseMode::Auto, &[], 1e9), vec![]);
        assert_eq!(decide_levels(CollapseMode::Full, &[], 1e9), vec![]);
        assert_eq!(
            decide_levels(CollapseMode::Collapsed, &[], 1e9),
            vec![]
        );
    }

    /// A single panel walked the whole ladder as the width shrinks: FULL,
    /// COMPACT, COLLAPSED, then TIGHT once it still overflows.
    #[test]
    fn single_panel_walks_the_whole_ladder() {
        let widths = [w(100.0, 60.0, 40.0, 20.0)];
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 1e9),
            vec![Level::Full]
        );
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 100.0),
            vec![Level::Full]
        );
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 75.0),
            vec![Level::Compact]
        );
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 45.0),
            vec![Level::Collapsed]
        );
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 20.0),
            vec![Level::Tight]
        );
    }

    /// Zero natural widths never overflow, even against a zero-width row: the
    /// first total (0) satisfies `<= max_w` immediately.
    #[test]
    fn zero_widths_never_overflow() {
        let widths = [w(0.0, 0.0, 0.0, 0.0), w(0.0, 0.0, 0.0, 0.0)];
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 0.0),
            vec![Level::Full, Level::Full]
        );
    }

    /// An endless row (max_w = 0) degrades everything to tight, same as positive
    /// widths against an impossible budget.
    #[test]
    fn endless_row_degrades_everything_to_tight() {
        let widths = [w(50.0, 30.0, 20.0, 10.0), w(50.0, 30.0, 20.0, 10.0)];
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 0.0),
            vec![Level::Tight, Level::Tight]
        );
    }

    /// An unbounded row keeps every panel full.
    #[test]
    fn unbounded_row_keeps_everything_full() {
        let widths = [w(50.0, 30.0, 20.0, 10.0), w(50.0, 30.0, 20.0, 10.0)];
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, f32::INFINITY),
            vec![Level::Full, Level::Full]
        );
    }

    /// A NaN budget is not a number, so every `<=` / `>` comparison is false:
    /// both phases fully degrade (all Collapsed) but the tight cascade never
    /// fires. Degenerate input; the point is it must not panic and must be
    /// deterministic.
    #[test]
    fn nan_budget_degrades_both_phases_but_no_tight_cascade() {
        let widths = [w(50.0, 30.0, 20.0, 10.0), w(50.0, 30.0, 20.0, 10.0)];
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, f32::NAN),
            vec![Level::Collapsed, Level::Collapsed]
        );
    }

    /// A row that fits exactly at full width stays full (<=, not <).
    #[test]
    fn exact_fit_at_full_stays_full() {
        let widths = [w(30.0, 20.0, 15.0, 10.0), w(40.0, 25.0, 15.0, 10.0)];
        assert_eq!(
            decide_levels(CollapseMode::Auto, &widths, 70.0),
            vec![Level::Full, Level::Full]
        );
    }

    /// The refactor must be a no-op: `decide_levels` equals the pre-refactor
    /// inline `u8` algorithm (two rightmost-first phases + the tight cascade)
    /// on a deterministic sweep of widths and budgets, including the degenerate
    /// 0 / 0 / inf / NaN corner cases.
    #[test]
    fn matches_the_pre_refactor_algorithm() {
        fn legacy(mode: CollapseMode, widths: &[Widths], max_w: f32) -> Vec<u8> {
            let forced = match mode {
                CollapseMode::Auto => None,
                CollapseMode::Full => Some(0),
                CollapseMode::Compact => Some(1),
                CollapseMode::Collapsed => Some(2),
            };
            if let Some(level) = forced {
                return vec![level; widths.len()];
            }
            let width_of = |lv: u8, i: usize| match lv {
                0 => widths[i].full,
                1 => widths[i].compact,
                2 => widths[i].button,
                _ => widths[i].tight,
            };
            let total = |levels: &[u8]| -> f32 {
                (0..levels.len()).map(|i| width_of(levels[i], i)).sum()
            };
            let mut levels = vec![0u8; widths.len()];
            for degraded in [1u8, 2u8] {
                for i in (0..widths.len()).rev() {
                    if total(&levels) <= max_w {
                        break;
                    }
                    levels[i] = degraded;
                }
            }
            if total(&levels) > max_w {
                levels
                    .iter_mut()
                    .filter(|l| **l == 2)
                    .for_each(|l| *l = 3);
            }
            levels
        }

        let mut rng: u64 = 0x243F_6A88_85A3_08D3;
        for n in 0..=6usize {
            for _ in 0..40 {
                let mut widths = Vec::with_capacity(n);
                for _ in 0..n {
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let full = (rng % 2000) as f32 / 10.0;
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let compact = (rng % (full as u64 * 10 + 1)) as f32 / 10.0;
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let button = (rng % (compact as u64 * 10 + 1)) as f32 / 10.0;
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let tight = (rng % (button as u64 * 10 + 1)) as f32 / 10.0;
                    widths.push(Widths {
                        full,
                        compact,
                        button,
                        tight,
                    });
                }
                let budgets = [
                    0.0,
                    0.5,
                    1.0,
                    (rng % 3000) as f32 / 10.0,
                    800.0,
                    1e9,
                    f32::INFINITY,
                    f32::NAN,
                ];
                for mw in budgets {
                    let now: Vec<u8> = decide_levels(CollapseMode::Auto, &widths, mw)
                        .iter()
                        .map(|&l| l as u8)
                        .collect();
                    assert_eq!(
                        now,
                        legacy(CollapseMode::Auto, &widths, mw),
                        "n={n} widths={widths:?} max_w={mw}"
                    );
                }
            }
        }
    }
}
