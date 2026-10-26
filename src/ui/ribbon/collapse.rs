//! Adaptive ribbon panels.
//!
//! Lays the active module's panels on one row. When they don't all fit, the row
//! degrades **from the right**, one panel at a time: first a panel shrinks to
//! compact icon columns, then it collapses to a title button. If even the
//! all-collapsed row overflows, every button drops to its small icon together,
//! then the buttons are squeezed. The row's height tracks the tallest shown
//! panel, so it shrinks as the panels do.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Renderer as _, Shell, mouse, overlay, renderer};
use iced::{
    Background, Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size,
    Theme, Vector, keyboard,
};

use crate::app::Message;
use crate::ui::style::chrome;

/// One panel in its four density renderings. `tight` is the collapsed button
/// with a small representative icon instead of the large one — the last step
/// before the buttons are squeezed together. An open flyout draws the panel's
/// own `full` rendering; see `FlyoutOverlay`. Elements are built lazily and
/// indexed by `Level`.
pub(crate) struct Panel<'a> {
    pub(crate) id: String,
    elements: [Option<Element<'a, Message>>; SLOTS],
    build: Box<dyn Fn(Level) -> Element<'a, Message> + 'a>,
}

impl<'a> Panel<'a> {
    /// The panel at full size.
    pub(crate) fn full(&self) -> Element<'a, Message> {
        (self.build)(Level::Full)
    }

    pub(crate) fn deferred(
        id: impl Into<String>,
        build: impl Fn(Level) -> Element<'a, Message> + 'a,
    ) -> Self {
        Self {
            id: id.into(),
            elements: std::array::from_fn(|_| None),
            build: Box::new(build),
        }
    }
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
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
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
pub(crate) fn decide_levels(mode: CollapseMode, widths: &[Widths], max_w: f32) -> Vec<Level> {
    if let Some(level) = mode.forced_level() {
        return vec![level; widths.len()];
    }
    let width_of = |lv: Level, i: usize| match lv {
        Level::Full => widths[i].full,
        Level::Compact => widths[i].compact,
        Level::Collapsed => widths[i].button,
        Level::Tight => widths[i].tight,
    };
    let total =
        |levels: &[Level]| -> f32 { (0..levels.len()).map(|i| width_of(levels[i], i)).sum() };
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
    /// Fingerprint of content that can affect natural panel dimensions.
    measure_key: u64,
    /// Height of the title strip at the bottom of a full or compact panel,
    /// where a press starts a drag; 0 turns dragging off.
    drag_strip: f32,
    /// The message a drop sends: the panel ids in their new order.
    on_reorder: Option<Box<dyn Fn(Vec<String>) -> Message + 'a>>,
}

/// How far the pointer moves from the press before the press becomes a drag.
const DRAG_THRESHOLD: f32 = 5.0;
/// Inset of the placeholder's dashed outline.
const PLACEHOLDER_INSET: f32 = 3.0;
/// Opacity of the panel copy that follows the pointer.
const GHOST_ALPHA: f32 = 0.9;

/// Tree state: the measurements and the drag in progress.
#[derive(Default)]
struct State {
    measure: MeasureCache,
    drag: Option<Drag>,
}

/// A panel pressed on its title strip, and dragged once the pointer moved.
#[derive(Clone, Debug)]
struct Drag {
    /// Index of the panel.
    panel: usize,
    /// Where the press landed.
    press: Point,
    /// The press point within the panel.
    grab: Vector,
    /// The pointer moved far enough: the panel is being dragged.
    moving: bool,
    /// Escape ended the drag; the release still belongs to it.
    cancelled: bool,
    /// Panel indices in the order the row shows while dragging.
    order: Vec<usize>,
    /// The pointer's last position.
    cursor: Point,
}

impl Drag {
    fn reordered(&self) -> bool {
        self.order
            .iter()
            .enumerate()
            .any(|(slot, &panel)| slot != panel)
    }
}

/// Move `panel` in `order` once the pointer at `x` passes the middle of a
/// neighbour; `bounds` are the panels' rectangles as laid out for `order`,
/// by panel index. Returns whether the order changed.
fn drag_to(order: &mut [usize], panel: usize, x: f32, bounds: &[Rectangle]) -> bool {
    let Some(mut slot) = order.iter().position(|&p| p == panel) else {
        return false;
    };
    let start = slot;
    while slot > 0 && x < bounds[order[slot - 1]].center_x() {
        order.swap(slot - 1, slot);
        slot -= 1;
    }
    if slot == start {
        while slot + 1 < order.len() && x > bounds[order[slot + 1]].center_x() {
            order.swap(slot, slot + 1);
            slot += 1;
        }
    }
    slot != start
}

/// A 1 px dashed outline along the inside of `r`.
fn dashed_outline(renderer: &mut Renderer, r: Rectangle, color: Color) {
    const DASH: f32 = 4.0;
    const GAP: f32 = 3.0;
    let mut quad = |bounds: Rectangle| {
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: true,
            },
            Background::Color(color),
        );
    };
    let mut x = r.x;
    while x < r.x + r.width {
        let width = DASH.min(r.x + r.width - x);
        quad(Rectangle::new(Point::new(x, r.y), Size::new(width, 1.0)));
        quad(Rectangle::new(
            Point::new(x, r.y + r.height - 1.0),
            Size::new(width, 1.0),
        ));
        x += DASH + GAP;
    }
    let mut y = r.y;
    while y < r.y + r.height {
        let height = DASH.min(r.y + r.height - y);
        quad(Rectangle::new(Point::new(r.x, y), Size::new(1.0, height)));
        quad(Rectangle::new(
            Point::new(r.x + r.width - 1.0, y),
            Size::new(1.0, height),
        ));
        y += DASH + GAP;
    }
}

#[derive(Default)]
struct MeasureCache {
    key: Option<u64>,
    widths: Vec<Widths>,
    heights: Vec<[f32; 4]>,
}

impl MeasureCache {
    fn matches(&self, key: u64, panel_count: usize) -> bool {
        self.key == Some(key)
            && self.widths.len() == panel_count
            && self.heights.len() == panel_count
    }
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
            measure_key: 0,
            drag_strip: 0.0,
            on_reorder: None,
        }
    }

    /// Let a press on the bottom `strip` px of a full or compact panel drag
    /// it to another place; a drop sends `on_reorder` with the panel ids in
    /// their new order.
    pub fn draggable(
        mut self,
        strip: f32,
        on_reorder: impl Fn(Vec<String>) -> Message + 'a,
    ) -> Self {
        self.drag_strip = strip;
        self.on_reorder = Some(Box::new(on_reorder));
        self
    }

    /// Reuse natural measurements while the panel content and font metrics stay
    /// unchanged. The available window width is intentionally not part of the
    /// key: these measurements use unbounded natural limits.
    pub fn measure_key(mut self, key: u64) -> Self {
        self.measure_key = key;
        self
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
        self.panels[i].elements[level as usize]
            .as_ref()
            .expect("selected ribbon density is built during layout")
    }

    fn shown_mut(&mut self, i: usize, level: Level) -> &mut Element<'a, Message> {
        self.panels[i].elements[level as usize]
            .as_mut()
            .expect("selected ribbon density is built during layout")
    }

    fn ensure_element(&mut self, tree: &mut widget::Tree, i: usize, level: Level) {
        let index = level as usize;
        if self.panels[i].elements[index].is_none() {
            let mut element = (self.panels[i].build)(level);
            tree.children[slot(i, level)].diff(element.as_widget_mut());
            self.panels[i].elements[index] = Some(element);
        }
    }

    fn levels_snapshot(&self, n: usize) -> Vec<Level> {
        let mut v = self.levels.borrow().clone();
        v.resize(n, Level::Full);
        v
    }
}

impl<'a> Widget<Message, Theme, Renderer> for CollapsePanels<'a> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        let child_count = self.panels.len() * SLOTS;
        tree.children.truncate(child_count);
        tree.children.resize_with(child_count, widget::Tree::empty);
        for (panel_index, panel) in self.panels.iter_mut().enumerate() {
            for level in [Level::Full, Level::Compact, Level::Collapsed, Level::Tight] {
                if let Some(element) = panel.elements[level as usize].as_mut() {
                    tree.children[slot(panel_index, level)].diff(element.as_widget_mut());
                }
            }
        }
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
            let cache = &tree.state.downcast_ref::<State>().measure;
            if cache.matches(self.measure_key, n) {
                widths.clone_from(&cache.widths);
                heights.clone_from(&cache.heights);
            } else {
                for i in 0..n {
                    self.ensure_element(tree, i, Level::Full);
                    let f = self.panels[i].elements[Level::Full as usize]
                        .as_mut()
                        .unwrap()
                        .as_widget_mut()
                        .layout(&mut tree.children[slot(i, Level::Full)], renderer, &natural)
                        .size();
                    self.ensure_element(tree, i, Level::Compact);
                    let c = self.panels[i].elements[Level::Compact as usize]
                        .as_mut()
                        .unwrap()
                        .as_widget_mut()
                        .layout(
                            &mut tree.children[slot(i, Level::Compact)],
                            renderer,
                            &natural,
                        )
                        .size();
                    self.ensure_element(tree, i, Level::Collapsed);
                    let b = self.panels[i].elements[Level::Collapsed as usize]
                        .as_mut()
                        .unwrap()
                        .as_widget_mut()
                        .layout(
                            &mut tree.children[slot(i, Level::Collapsed)],
                            renderer,
                            &natural,
                        )
                        .size();
                    self.ensure_element(tree, i, Level::Tight);
                    let t = self.panels[i].elements[Level::Tight as usize]
                        .as_mut()
                        .unwrap()
                        .as_widget_mut()
                        .layout(
                            &mut tree.children[slot(i, Level::Tight)],
                            renderer,
                            &natural,
                        )
                        .size();
                    widths.push(Widths {
                        full: f.width,
                        compact: c.width,
                        button: b.width,
                        tight: t.width,
                    });
                    heights.push([f.height, c.height, b.height, t.height]);
                }
                let cache = &mut tree.state.downcast_mut::<State>().measure;
                cache.key = Some(self.measure_key);
                cache.widths.clone_from(&widths);
                cache.heights.clone_from(&heights);
            }
        }
        let levels = if let Some(level) = self.mode.forced_level() {
            vec![level; n]
        } else {
            decide_levels(self.mode, &widths, max_w)
        };

        // Measurements let Auto choose a density without building all variants
        // again. On a cache hit only the selected element is built. If a
        // collapsed panel is open, its Full element is also needed by the flyout.
        for (i, &level) in levels.iter().enumerate() {
            self.ensure_element(tree, i, level);
            if matches!(level, Level::Collapsed | Level::Tight)
                && self.open.as_deref() == Some(self.panels[i].id.as_str())
            {
                self.ensure_element(tree, i, Level::Full);
            }
        }

        // The row is "tight" once any panel has dropped to its small icon — the
        // last, most cramped state. The tab bar hides its mode selector then.
        if let Some(out) = &self.tight_out {
            out.store(levels.iter().any(|&l| l == Level::Tight), Ordering::Relaxed);
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

        // A drag shows the panels in its order; it ends if the panels change.
        let state = tree.state.downcast_mut::<State>();
        if state
            .drag
            .as_ref()
            .is_some_and(|drag| drag.order.len() != n)
        {
            state.drag = None;
        }
        let order: Vec<usize> = match &state.drag {
            Some(drag) if drag.moving => drag.order.clone(),
            _ => (0..n).collect(),
        };

        // Place the chosen element for each panel left-to-right, in `order`.
        let mut placed: Vec<(layout::Node, f32, f32)> = (0..n)
            .map(|i| {
                let level = levels[i];
                let tree_idx = slot(i, level);
                let node = self.shown_mut(i, level).as_widget_mut().layout(
                    &mut tree.children[tree_idx],
                    renderer,
                    &natural,
                );
                let h = node.size().height;
                (node, 0.0, h)
            })
            .collect();
        let mut x = 0.0f32;
        for (k, &i) in order.iter().enumerate() {
            if k > 0 {
                x -= squeeze;
            }
            placed[i].1 = x;
            x += placed[i].0.size().width;
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
            placed.iter().map(|(_, _, h)| *h).fold(self.row_h, f32::max)
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
        let mut cursor = cursor;
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if self.drag_strip > 0.0 && !shell.is_event_captured() =>
            {
                // A press on a full or compact panel's title strip may become
                // a drag; until the pointer moves it stays an ordinary click.
                state.drag = cursor.position().and_then(|at| {
                    layout.children().enumerate().find_map(|(i, child)| {
                        let b = child.bounds();
                        let strip = b.y + b.height - self.drag_strip;
                        (matches!(levels.get(i), Some(Level::Full | Level::Compact))
                            && b.contains(at)
                            && at.y >= strip)
                            .then(|| Drag {
                                panel: i,
                                press: at,
                                grab: at - b.position(),
                                moving: false,
                                cancelled: false,
                                order: (0..self.panels.len()).collect(),
                                cursor: at,
                            })
                    })
                });
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(drag) = state.drag.as_mut() {
                    if !drag.moving && position.distance(drag.press) >= DRAG_THRESHOLD {
                        drag.moving = true;
                        shell.invalidate_layout();
                    }
                    if drag.moving {
                        drag.cursor = *position;
                        if !drag.cancelled {
                            let bounds: Vec<Rectangle> =
                                layout.children().map(|child| child.bounds()).collect();
                            if drag_to(&mut drag.order, drag.panel, position.x, &bounds) {
                                shell.invalidate_layout();
                            }
                        }
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(drag) = state.drag.take() {
                    if drag.moving {
                        // The pressed button lets go without a click.
                        cursor = mouse::Cursor::Unavailable;
                        if !drag.cancelled && drag.reordered() {
                            if let Some(on_reorder) = &self.on_reorder {
                                shell.publish(on_reorder(
                                    drag.order
                                        .iter()
                                        .map(|&i| self.panels[i].id.clone())
                                        .collect(),
                                ));
                            }
                        }
                        shell.invalidate_layout();
                        shell.capture_event();
                    }
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                // Escape puts the panel back; the release still ends the drag.
                if let Some(drag) = state.drag.as_mut().filter(|drag| drag.moving) {
                    drag.cancelled = true;
                    drag.order = (0..self.panels.len()).collect();
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
            }
            Event::Mouse(_) if state.drag.as_ref().is_some_and(|drag| drag.moving) => {
                // The panels take no pointer input while one is dragged.
                return;
            }
            _ => {}
        }
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
        if tree
            .state
            .downcast_ref::<State>()
            .drag
            .as_ref()
            .is_some_and(|drag| drag.moving)
        {
            return mouse::Interaction::Grabbing;
        }
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
        let n = self.panels.len();
        let drag = tree
            .state
            .downcast_ref::<State>()
            .drag
            .as_ref()
            .filter(|drag| drag.moving && drag.order.len() == n);
        for (i, child_layout) in layout.children().enumerate() {
            if drag.is_some_and(|drag| drag.panel == i) {
                // The dragged panel's place: a fill with a dashed outline.
                let b = child_layout.bounds();
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: b,
                        border: Border::default(),
                        shadow: Shadow::default(),
                        snap: true,
                    },
                    Background::Color(chrome::title_bar(theme)),
                );
                dashed_outline(renderer, b.shrink(PLACEHOLDER_INSET), chrome::accent(theme));
                continue;
            }
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
        let order: Vec<usize> = drag.map_or_else(|| (0..n).collect(), |drag| drag.order.clone());
        for pair in order.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            if is_btn(levels[left]) && is_btn(levels[right]) {
                continue;
            }
            let x = bounds[right].x;
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

        // A dragged panel's copy follows the pointer; nothing else shows.
        let dragged = tree
            .state
            .downcast_ref::<State>()
            .drag
            .as_ref()
            .filter(|drag| drag.moving)
            .map(|drag| (drag.panel, drag.grab, drag.cursor));
        if let Some((p, grab, at)) = dragged {
            let level = levels.get(p).copied().unwrap_or(Level::Full);
            let content = self.panels.get_mut(p)?.elements[level as usize].as_mut()?;
            return Some(overlay::Element::new(Box::new(GhostOverlay {
                content,
                tree: &mut tree.children[slot(p, level)],
                grab,
                at,
            })));
        }

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
                    let anchor = Point::new(b.x + translation.x, b.y + b.height + translation.y);
                    // The flyout is the panel's own `full` rendering (the row
                    // never shows slot 0 while the panel is button-form, so it
                    // is free to double as the overlay content).
                    return Some(overlay::Element::new(Box::new(FlyoutOverlay {
                        content: self.panels[p].elements[Level::Full as usize]
                            .as_mut()
                            .expect("open collapsed flyout has full panel content"),
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
        for ((i, panel), child_layout) in self.panels.iter_mut().enumerate().zip(layout.children())
        {
            if tree_rest.len() < SLOTS {
                break;
            }
            let (chunk, rest) = tree_rest.split_at_mut(SLOTS);
            tree_rest = rest;
            let level = levels.get(i).copied().unwrap_or(Level::Full);
            let Some(child) = panel.elements[level as usize].as_mut() else {
                continue;
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

/// The copy of a dragged panel that follows the pointer, drawn faded and
/// taking no pointer input.
struct GhostOverlay<'a, 'b> {
    content: &'b mut Element<'a, Message>,
    tree: &'b mut widget::Tree,
    /// The press point within the panel.
    grab: Vector,
    /// Pointer position to use when the draw has none.
    at: Point,
}

impl overlay::Overlay<Message, Theme, Renderer> for GhostOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, _bounds: Size) -> layout::Node {
        let natural = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, f32::INFINITY));
        let node = self
            .content
            .as_widget_mut()
            .layout(self.tree, renderer, &natural);
        layout::Node::with_children(node.size(), vec![node])
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        // Drawn at the pointer, so moving it needs a redraw, not a layout.
        let b = layout.bounds();
        let top_left = cursor.position().unwrap_or(self.at) - self.grab;
        // The tool area's background (see `Ribbon::view`).
        let background = theme.palette().background.weakest.color;
        let quad = |bounds| renderer::Quad {
            bounds,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        };
        renderer.with_translation(top_left - b.position(), |renderer| {
            renderer.fill_quad(
                quad(b),
                Background::Color(background.scale_alpha(GHOST_ALPHA)),
            );
            let child = layout.children().next().unwrap();
            self.content.as_widget().draw(
                self.tree,
                renderer,
                theme,
                style,
                child,
                mouse::Cursor::Unavailable,
                &b,
            );
            // A veil of the background fades the panel's own colours too.
            renderer.fill_quad(
                quad(b),
                Background::Color(background.scale_alpha(1.0 - GHOST_ALPHA)),
            );
        });
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
        self.content.as_widget().mouse_interaction(
            self.tree,
            child,
            cursor,
            &child.bounds(),
            renderer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects(widths: &[f32]) -> Vec<Rectangle> {
        let mut x = 0.0;
        widths
            .iter()
            .map(|&w| {
                let r = Rectangle::new(Point::new(x, 0.0), Size::new(w, 80.0));
                x += w;
                r
            })
            .collect()
    }

    #[test]
    fn a_dragged_panel_swaps_once_the_pointer_passes_a_neighbours_middle() {
        // Panels 0..3, 100 px each; panel 1 is dragged.
        let bounds = rects(&[100.0, 100.0, 100.0]);
        let mut order = vec![0, 1, 2];
        assert!(!drag_to(&mut order, 1, 160.0, &bounds));
        assert!(!drag_to(&mut order, 1, 249.0, &bounds));
        assert!(drag_to(&mut order, 1, 251.0, &bounds));
        assert_eq!(order, [0, 2, 1]);

        let mut order = vec![0, 1, 2];
        assert!(drag_to(&mut order, 1, 40.0, &bounds));
        assert_eq!(order, [1, 0, 2]);
    }

    #[test]
    fn a_fast_drag_passes_several_panels_at_once() {
        let bounds = rects(&[100.0, 100.0, 100.0, 100.0]);
        let mut order = vec![0, 1, 2, 3];
        assert!(drag_to(&mut order, 0, 390.0, &bounds));
        assert_eq!(order, [1, 2, 3, 0]);
    }

    #[test]
    fn a_swap_does_not_swing_back_with_unequal_widths() {
        // A narrow panel passes a wide one; after the swap the wide one's
        // middle is behind the pointer, so the order holds.
        let mut order = vec![0, 1];
        let before = rects(&[50.0, 300.0]);
        assert!(drag_to(&mut order, 0, 201.0, &before));
        assert_eq!(order, [1, 0]);
        let after = vec![
            Rectangle::new(Point::new(300.0, 0.0), Size::new(50.0, 80.0)),
            Rectangle::new(Point::new(0.0, 0.0), Size::new(300.0, 80.0)),
        ];
        assert!(!drag_to(&mut order, 0, 201.0, &after));
        assert_eq!(order, [1, 0]);
    }

    #[test]
    fn natural_measurements_are_reused_only_for_the_same_key_and_panel_count() {
        let cache = MeasureCache {
            key: Some(17),
            widths: vec![Widths::default(); 2],
            heights: vec![[0.0; 4]; 2],
        };

        assert!(cache.matches(17, 2));
        assert!(!cache.matches(18, 2));
        assert!(!cache.matches(17, 1));
    }

    #[test]
    fn auto_builds_all_densities_once_then_only_the_selected_density() {
        use iced_test::runtime::user_interface::{Cache, UserInterface};
        use std::sync::atomic::{AtomicUsize, Ordering};

        fn view(counter: Arc<AtomicUsize>, open: Option<String>) -> Element<'static, Message> {
            let panel = Panel::deferred("test", move |level| {
                counter.fetch_add(1, Ordering::Relaxed);
                let (width, height) = match level {
                    Level::Full => (200.0, 80.0),
                    Level::Compact => (150.0, 70.0),
                    Level::Collapsed => (100.0, 50.0),
                    Level::Tight => (50.0, 30.0),
                };
                iced::widget::container(iced::widget::text("panel"))
                    .width(Length::Fixed(width))
                    .height(Length::Fixed(height))
                    .into()
            });
            CollapsePanels::new(vec![panel], open, 80.0)
                .measure_key(17)
                .into()
        }

        let count = Arc::new(AtomicUsize::new(0));
        let _one = crate::headless_ui_lock();
        let mut renderer = iced_test::futures::futures::executor::block_on(
            <Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                None,
            ),
        )
        .expect("headless renderer");

        let ui = UserInterface::build(
            view(count.clone(), None),
            Size::new(1000.0, 100.0),
            Cache::default(),
            &mut renderer,
        );
        assert_eq!(count.load(Ordering::Relaxed), 4);

        let ui = UserInterface::build(
            view(count.clone(), None),
            Size::new(1000.0, 100.0),
            ui.into_cache(),
            &mut renderer,
        );
        assert_eq!(count.load(Ordering::Relaxed), 5, "only Full is rebuilt");

        let ui = UserInterface::build(
            view(count.clone(), None),
            Size::new(170.0, 100.0),
            ui.into_cache(),
            &mut renderer,
        );
        assert_eq!(count.load(Ordering::Relaxed), 6, "resize builds Compact");

        let _ui = UserInterface::build(
            view(count.clone(), Some("test".to_string())),
            Size::new(20.0, 100.0),
            ui.into_cache(),
            &mut renderer,
        );
        assert_eq!(
            count.load(Ordering::Relaxed),
            8,
            "tight selection builds Tight and Full for the open flyout"
        );
    }

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
        let widths = [w(50.0, 40.0, 30.0, 20.0), w(200.0, 100.0, 60.0, 30.0)];
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
        assert_eq!(decide_levels(CollapseMode::Collapsed, &[], 1e9), vec![]);
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
            let total =
                |levels: &[u8]| -> f32 { (0..levels.len()).map(|i| width_of(levels[i], i)).sum() };
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
                levels.iter_mut().filter(|l| **l == 2).for_each(|l| *l = 3);
            }
            levels
        }

        let mut rng: u64 = 0x243F_6A88_85A3_08D3;
        for n in 0..=6usize {
            for _ in 0..40 {
                let mut widths = Vec::with_capacity(n);
                for _ in 0..n {
                    rng = rng
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let full = (rng % 2000) as f32 / 10.0;
                    rng = rng
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let compact = (rng % (full as u64 * 10 + 1)) as f32 / 10.0;
                    rng = rng
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let button = (rng % (compact as u64 * 10 + 1)) as f32 / 10.0;
                    rng = rng
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
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
