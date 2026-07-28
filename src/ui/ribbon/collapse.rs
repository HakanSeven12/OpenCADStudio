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
use iced::advanced::{mouse, overlay, renderer, Clipboard, Renderer as _, Shell};
use iced::{
    Background, Border, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size,
    Theme, Vector,
};

use crate::app::Message;

/// One panel in its five renderings. `tight` is the collapsed button with a
/// small representative icon instead of the large one — the last step before the
/// buttons are squeezed together.
pub struct Panel<'a> {
    pub id: String,
    pub full: Element<'a, Message>,
    pub compact: Element<'a, Message>,
    pub button: Element<'a, Message>,
    pub tight: Element<'a, Message>,
    pub flyout: Element<'a, Message>,
}

// Number of tree slots per panel: [full, compact, button, tight, flyout].
const SLOTS: usize = 5;

// Per-panel degradation level; also the offset of the shown element within the
// panel's tree slots. Degrade order (from the right): FULL → COMPACT → COLLAPSED
// (big-icon button) → TIGHT (small-icon button). `flyout` (slot 4) is overlay
// only, never a level.
const FULL: u8 = 0;
const COMPACT: u8 = 1;
const COLLAPSED: u8 = 2;
const TIGHT: u8 = 3;

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
    fn forced_level(self) -> Option<u8> {
        match self {
            CollapseMode::Auto => None,
            CollapseMode::Full => Some(FULL),
            CollapseMode::Compact => Some(COMPACT),
            CollapseMode::Collapsed => Some(COLLAPSED),
        }
    }

}

impl std::fmt::Display for CollapseMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

pub struct CollapsePanels<'a> {
    panels: Vec<Panel<'a>>,
    /// Title of the panel whose flyout is open (if any).
    open: Option<String>,
    /// Fallback row height, used only when there are no panels to measure.
    row_h: f32,
    /// Chosen degradation level per panel; set during layout.
    levels: RefCell<Vec<u8>>,
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
            levels: RefCell::new(vec![FULL; n]),
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

    fn shown(&self, i: usize, level: u8) -> &Element<'a, Message> {
        match level {
            FULL => &self.panels[i].full,
            COMPACT => &self.panels[i].compact,
            COLLAPSED => &self.panels[i].button,
            _ => &self.panels[i].tight,
        }
    }

    fn shown_mut(&mut self, i: usize, level: u8) -> &mut Element<'a, Message> {
        match level {
            FULL => &mut self.panels[i].full,
            COMPACT => &mut self.panels[i].compact,
            COLLAPSED => &mut self.panels[i].button,
            _ => &mut self.panels[i].tight,
        }
    }

    fn levels_snapshot(&self, n: usize) -> Vec<u8> {
        let mut v = self.levels.borrow().clone();
        v.resize(n, FULL);
        v
    }
}

impl<'a> Widget<Message, Theme, Renderer> for CollapsePanels<'a> {
    fn children(&self) -> Vec<widget::Tree> {
        let mut v = Vec::with_capacity(self.panels.len() * SLOTS);
        for p in &self.panels {
            v.push(widget::Tree::new(&p.full));
            v.push(widget::Tree::new(&p.compact));
            v.push(widget::Tree::new(&p.button));
            v.push(widget::Tree::new(&p.tight));
            v.push(widget::Tree::new(&p.flyout));
        }
        v
    }

    fn diff(&self, tree: &mut widget::Tree) {
        let mut refs: Vec<&dyn Widget<Message, Theme, Renderer>> = Vec::new();
        for p in &self.panels {
            refs.push(p.full.as_widget());
            refs.push(p.compact.as_widget());
            refs.push(p.button.as_widget());
            refs.push(p.tight.as_widget());
            refs.push(p.flyout.as_widget());
        }
        tree.diff_children(&refs);
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

        // Measure each panel at all four densities. Width drives the degradation
        // decision; height drives the row height.
        let mut full_w = vec![0.0f32; n];
        let mut compact_w = vec![0.0f32; n];
        let mut button_w = vec![0.0f32; n];
        let mut tight_w = vec![0.0f32; n];
        let mut full_h = vec![0.0f32; n];
        let mut compact_h = vec![0.0f32; n];
        let mut button_h = vec![0.0f32; n];
        let mut tight_h = vec![0.0f32; n];
        for i in 0..n {
            let s = self.panels[i].full.as_widget_mut()
                .layout(&mut tree.children[SLOTS * i], renderer, &natural)
                .size();
            full_w[i] = s.width;
            full_h[i] = s.height;
            let s = self.panels[i].compact.as_widget_mut()
                .layout(&mut tree.children[SLOTS * i + 1], renderer, &natural)
                .size();
            compact_w[i] = s.width;
            compact_h[i] = s.height;
            let s = self.panels[i].button.as_widget_mut()
                .layout(&mut tree.children[SLOTS * i + 2], renderer, &natural)
                .size();
            button_w[i] = s.width;
            button_h[i] = s.height;
            let s = self.panels[i].tight.as_widget_mut()
                .layout(&mut tree.children[SLOTS * i + 3], renderer, &natural)
                .size();
            tight_w[i] = s.width;
            tight_h[i] = s.height;
        }

        let width_of = |lv: u8, i: usize| -> f32 {
            match lv {
                FULL => full_w[i],
                COMPACT => compact_w[i],
                COLLAPSED => button_w[i],
                _ => tight_w[i],
            }
        };
        let height_of = |lv: u8, i: usize| -> f32 {
            match lv {
                FULL => full_h[i],
                COMPACT => compact_h[i],
                COLLAPSED => button_h[i],
                _ => tight_h[i],
            }
        };
        let total = |levels: &[u8]| -> f32 { (0..n).map(|i| width_of(levels[i], i)).sum() };

        // A forced mode pins every panel to one level. Otherwise (Auto) degrade
        // from the RIGHT, one panel at a time (gradual): first FULL → COMPACT,
        // then COMPACT → COLLAPSED, each phase only while the row still overflows.
        // If even the all-collapsed row overflows, every button drops to its
        // small icon (tight) together, then the buttons are squeezed.
        let levels = if let Some(level) = self.mode.forced_level() {
            vec![level; n]
        } else {
            let mut levels = vec![FULL; n];
            for degraded in [COMPACT, COLLAPSED] {
                for i in (0..n).rev() {
                    if total(&levels) <= max_w {
                        break;
                    }
                    levels[i] = degraded;
                }
            }
            if total(&levels) > max_w {
                levels
                    .iter_mut()
                    .filter(|l| **l == COLLAPSED)
                    .for_each(|l| *l = TIGHT);
            }
            levels
        };
        // The row is "tight" once any panel has dropped to its small icon — the
        // last, most cramped state. The tab bar hides its mode selector then.
        if let Some(out) = &self.tight_out {
            out.store(levels.iter().any(|&l| l == TIGHT), Ordering::Relaxed);
        }
        *self.levels.borrow_mut() = levels.clone();

        // Same idea as the tab bar squeezing its gaps before wrapping: once every
        // panel is at its tightest and the row STILL overflows, pull the buttons
        // together (up to MAX_PANEL_SQUEEZE per gap, reclaiming their edge
        // padding) so more of them stay on-screen before anything is clipped. In
        // Auto this only fires when everything is already tight, so it never
        // overlaps a full/compact panel; a forced density is left to overflow.
        let squeeze = if self.mode == CollapseMode::Auto && n > 1 && total(&levels) > max_w {
            ((total(&levels) - max_w) / (n - 1) as f32).min(MAX_PANEL_SQUEEZE)
        } else {
            0.0
        };

        // The row is as tall as the tallest shown panel, so the ribbon height
        // shrinks as its panels degrade to shorter collapsed / tight buttons.
        let row_h = if n == 0 {
            self.row_h
        } else {
            (0..n)
                .map(|i| height_of(levels[i], i))
                .fold(0.0f32, f32::max)
        };

        // Place the chosen element for each panel left-to-right.
        let mut children: Vec<layout::Node> = Vec::with_capacity(n);
        let mut x = 0.0f32;
        for i in 0..n {
            if i > 0 {
                x -= squeeze;
            }
            let level = levels[i];
            let tree_idx = SLOTS * i + level as usize;
            let node = self.shown_mut(i, level).as_widget_mut().layout(
                &mut tree.children[tree_idx],
                renderer,
                &natural,
            );
            let h = node.size().height;
            let w = node.size().width;
            let y = ((row_h - h) / 2.0).max(0.0);
            children.push(node.move_to(Point::new(x, y)));
            x += w;
        }

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
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let levels = self.levels_snapshot(self.panels.len());
        for (i, child_layout) in layout.children().enumerate() {
            let level = levels[i];
            let tree_idx = SLOTS * i + level as usize;
            self.shown_mut(i, level).as_widget_mut().update(
                &mut tree.children[tree_idx],
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
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
            let tree_idx = SLOTS * i + level as usize;
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
            let tree_idx = SLOTS * i + level as usize;
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
            let tree_idx = SLOTS * i + level as usize;
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
        let is_btn = |lv: u8| lv == COLLAPSED || lv == TIGHT;
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
                Background::Color(theme.extended_palette().background.neutral.color),
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
                let lvl = levels.get(p).copied().unwrap_or(FULL);
                if (lvl == COLLAPSED || lvl == TIGHT) && layout.children().nth(p).is_some() {
                    let child_layout = layout.children().nth(p).unwrap();
                    let b = child_layout.bounds();
                    let anchor =
                        Point::new(b.x + translation.x, b.y + b.height + translation.y);
                    return Some(overlay::Element::new(Box::new(FlyoutOverlay {
                        flyout: &mut self.panels[p].flyout,
                        tree: &mut tree.children[SLOTS * p + 4],
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
            let level = levels.get(i).copied().unwrap_or(FULL);
            let child = match level {
                FULL => &mut panel.full,
                COMPACT => &mut panel.compact,
                COLLAPSED => &mut panel.button,
                _ => &mut panel.tight,
            };
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
/// closes it when the user presses outside.
struct FlyoutOverlay<'a, 'b> {
    flyout: &'b mut Element<'a, Message>,
    tree: &'b mut widget::Tree,
    anchor: Point,
}

impl overlay::Overlay<Message, Theme, Renderer> for FlyoutOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let viewport = Rectangle::with_size(bounds);
        let limits = layout::Limits::new(Size::ZERO, viewport.size());
        let node = self
            .flyout
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
        let child = layout.children().next().unwrap();
        self.flyout.as_widget().draw(
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
        clipboard: &mut dyn Clipboard,
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

        self.flyout
            .as_widget_mut()
            .update(self.tree, event, child, cursor, renderer, clipboard, shell, &vp);
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let child = layout.children().next().unwrap();
        self.flyout
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
        self.flyout
            .as_widget()
            .mouse_interaction(self.tree, child, cursor, &child.bounds(), renderer)
    }
}
