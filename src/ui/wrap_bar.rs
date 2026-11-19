//! Adaptive two-block bar that wraps its trailing block onto a second row when
//! the width can't hold both blocks side by side. Used by the ribbon tab row
//! (lead = quick-access + undo/redo, trail = tabs) and the status bar
//! (lead = menu, trail = status pills, with the layout-tab strip as the Fill
//! `middle` filler between them).
//!
//! Layout, per row:
//!   • one row  → `lead` at the left; `trail` packed after it (`justify_end`
//!                false), flush to the right edge (`justify_end` true) or,
//!                with `center_trail`, in the middle of the bar or of the
//!                wider span a `CenterSpan` names, never over `lead`; an
//!                optional Fill `middle` stretches through the gap.
//!   • wrapped  → `trail` first uses free space on `lead`'s final row; only
//!                when it does not fit is another row added. Alignment follows
//!                `justify_end`; a `WrapFlow::center` trail centers each of
//!                its rows in the bar itself (the `CenterSpan` does not apply).
//!
//! The measured total height is written to `height_out` (if set) so callers can
//! anchor overlays below the possibly-taller bar.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use rustc_hash::FxHashMap;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Renderer as _, Shell};
use iced::{
    Background, Border, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};

use crate::app::Message;

thread_local! {
    /// Screen bounds of every ribbon dropdown button, keyed by dropdown id, so
    /// an open dropdown's overlay can anchor directly below its widget at any
    /// size. Written by `PosReport` on draw, read by `dropdown_bounds`. Each
    /// entry keeps the frame it was drawn in (see `fresh_bounds`).
    static DD_BOUNDS: RefCell<FxHashMap<String, (Rectangle, u64)>> =
        RefCell::new(FxHashMap::default());
    /// Advanced by `end_frame` once everything above the widgets has drawn.
    static FRAME: Cell<u64> = const { Cell::new(0) };
}

/// Last-drawn screen bounds of the dropdown button with this id.
pub fn dropdown_bounds(id: &str) -> Option<Rectangle> {
    DD_BOUNDS.with(|m| m.borrow().get(id).map(|(bounds, _)| *bounds))
}

/// Bounds of the widget `id` only when it was drawn in the current frame, so
/// a widget that left the screen does not report its old place.
pub fn fresh_bounds(id: &str) -> Option<Rectangle> {
    let frame = FRAME.with(Cell::get);
    DD_BOUNDS.with(|m| {
        m.borrow()
            .get(id)
            .filter(|(_, drawn)| *drawn == frame)
            .map(|(bounds, _)| *bounds)
    })
}

/// Bounds of the widget `id` when it was drawn in the current or the last
/// finished frame (for use between frames).
pub fn recent_bounds(id: &str) -> Option<Rectangle> {
    let frame = FRAME.with(Cell::get);
    DD_BOUNDS.with(|m| {
        m.borrow()
            .get(id)
            .filter(|(_, drawn)| drawn + 1 >= frame)
            .map(|(bounds, _)| *bounds)
    })
}

/// Record `bounds` for the widget `id` as if it had just been drawn.
#[cfg(test)]
pub fn set_drawn_bounds(id: &str, bounds: Rectangle) {
    let frame = FRAME.with(Cell::get);
    DD_BOUNDS.with(|m| m.borrow_mut().insert(id.to_string(), (bounds, frame)));
}

/// Close the current frame for `fresh_bounds`.
pub fn end_frame() {
    FRAME.with(|frame| frame.set(frame.get() + 1));
}

/// Where something `inner` wide starts to sit in the middle of `outer`, in
/// whole pixels; 0 when it does not fit.
fn centered_x(outer: f32, inner: f32) -> f32 {
    ((outer - inner) / 2.0).floor().max(0.0)
}

/// Where something `inner` wide starts, within a bar `width` wide, to sit
/// in the middle of a span reaching `left` px before the bar and `right` px
/// past it; kept inside the bar.
fn centered_in_span(width: f32, inner: f32, (left, right): (f32, f32)) -> f32 {
    let x = ((width + right - left - inner) / 2.0).floor();
    x.min(width - inner).max(0.0)
}

/// The room a bar's neighbours take before and after it, for centering in
/// the whole span rather than the bar. Neighbours of a fixed width report it
/// while they are laid out (`PosReport::report_width`); iced lays them out
/// before a `Fill` bar beside them, so the bar reads it in the same pass.
#[derive(Clone, Default)]
pub struct CenterSpan {
    before: Arc<AtomicU32>,
    after: Arc<AtomicU32>,
}

impl CenterSpan {
    /// A span reaching `before` px before the bar and `after` px past it,
    /// until a neighbour reports otherwise.
    pub fn new(before: f32, after: f32) -> Self {
        Self {
            before: Arc::new(AtomicU32::new(before.to_bits())),
            after: Arc::new(AtomicU32::new(after.to_bits())),
        }
    }

    /// Where a neighbour before the bar reports its width.
    pub fn before(&self) -> Arc<AtomicU32> {
        self.before.clone()
    }

    /// Where a neighbour after the bar reports its width.
    pub fn after(&self) -> Arc<AtomicU32> {
        self.after.clone()
    }

    /// (before, after) in px, never negative.
    fn insets(&self) -> (f32, f32) {
        let read = |side: &AtomicU32| f32::from_bits(side.load(Ordering::Relaxed)).max(0.0);
        (read(&self.before), read(&self.after))
    }
}

pub struct WrapBar<'a> {
    lead: Element<'a, Message>,
    /// Optional Fill filler occupying the gap between `lead` and `trail`.
    middle: Option<Element<'a, Message>>,
    trail: Element<'a, Message>,
    /// Gap between adjacent blocks on a shared row.
    spacing: f32,
    /// Minimum height of a single row.
    min_row_h: f32,
    /// When true, `trail` sits at the right edge (justified); otherwise it is
    /// packed immediately after `lead`.
    justify_end: bool,
    /// When true and both blocks share a row, `trail` sits in the middle of
    /// the bar, but never over `lead`.
    center_trail: bool,
    /// The wider span `trail` centers in; none centers it in the bar.
    center_span: Option<CenterSpan>,
    /// Receives the measured total height (bits of an `f32`).
    height_out: Option<Arc<AtomicU32>>,
}

impl<'a> WrapBar<'a> {
    pub fn new(lead: Element<'a, Message>, trail: Element<'a, Message>) -> Self {
        Self {
            lead,
            middle: None,
            trail,
            spacing: 0.0,
            min_row_h: 28.0,
            justify_end: false,
            center_trail: false,
            center_span: None,
            height_out: None,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn min_row_h(mut self, h: f32) -> Self {
        self.min_row_h = h;
        self
    }

    pub fn justify_end(mut self, justify: bool) -> Self {
        self.justify_end = justify;
        self
    }

    /// Center `trail` in the bar while it shares a row with `lead`. A
    /// wrapped `trail` is centered by its own flow (`WrapFlow::center`).
    pub fn center_trail(mut self, center: bool) -> Self {
        self.center_trail = center;
        self
    }

    /// Center `trail` in `span`, wider than the bar: the bar's whole row,
    /// say, when the bar sits between other blocks. The span is read while
    /// the bar is laid out.
    pub fn center_span(mut self, span: CenterSpan) -> Self {
        self.center_span = Some(span);
        self
    }

    pub fn middle(mut self, middle: Element<'a, Message>) -> Self {
        self.middle = Some(middle);
        self
    }

    pub fn report_height(mut self, out: Arc<AtomicU32>) -> Self {
        self.height_out = Some(out);
        self
    }

    /// Elements in row order: lead, [middle], trail.
    fn refs(&self) -> Vec<&Element<'a, Message>> {
        let mut v = Vec::with_capacity(3);
        v.push(&self.lead);
        if let Some(m) = &self.middle {
            v.push(m);
        }
        v.push(&self.trail);
        v
    }

    fn refs_mut(&mut self) -> Vec<&mut Element<'a, Message>> {
        let mut v = Vec::with_capacity(3);
        v.push(&mut self.lead);
        if let Some(m) = &mut self.middle {
            v.push(m);
        }
        v.push(&mut self.trail);
        v
    }
}

impl<'a> Widget<Message, Theme, Renderer> for WrapBar<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        let mut refs = self.refs_mut();
        tree.diff_children(&mut refs);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max = limits.max();
        // Measure blocks at their natural (unwrapped) width so the fit decision
        // and one-row placement use true content widths. A flex-wrap `trail`
        // (WrapFlow) only wraps when it is later laid out with a bounded width.
        let natural = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, f32::INFINITY));

        let has_middle = self.middle.is_some();
        let trail_idx = if has_middle { 2 } else { 1 };

        let mut lead_node =
            self.lead
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &natural);
        let mut trail_node =
            self.trail
                .as_widget_mut()
                .layout(&mut tree.children[trail_idx], renderer, &natural);

        let mut lead_sz = lead_node.size();
        let mut trail_sz = trail_node.size();

        let width = if max.width.is_finite() {
            max.width
        } else {
            lead_sz.width + self.spacing + trail_sz.width
        };

        let fits =
            max.width.is_finite() && lead_sz.width + self.spacing + trail_sz.width <= max.width;

        let row_h = lead_sz.height.max(trail_sz.height).max(self.min_row_h);
        let bounded = layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY));

        let (lead_pos, trail_pos, middle_x, middle_gap, total_h);
        if fits {
            // One row: lead left, trail packed after it or flush right.
            let after_lead = lead_sz.width + self.spacing;
            let trail_x = if self.justify_end {
                (width - trail_sz.width).max(after_lead)
            } else if self.center_trail {
                let insets = self
                    .center_span
                    .as_ref()
                    .map_or((0.0, 0.0), CenterSpan::insets);
                centered_in_span(width, trail_sz.width, insets).max(after_lead)
            } else {
                after_lead
            };
            lead_pos = Point::new(0.0, (row_h - lead_sz.height) / 2.0);
            trail_pos = Point::new(trail_x, (row_h - trail_sz.height) / 2.0);
            middle_x = lead_sz.width + self.spacing;
            middle_gap = (trail_x - self.spacing - middle_x).max(0.0);
            total_h = row_h;
        } else if has_middle {
            // 3-slot justified bar: lead on row 1, middle fills the rest of it,
            // trail (flex-wrap) drops onto the row(s) below.
            lead_pos = Point::new(0.0, (row_h - lead_sz.height) / 2.0);
            middle_x = lead_sz.width + self.spacing;
            middle_gap = (width - middle_x).max(0.0);

            trail_node = self.trail.as_widget_mut().layout(
                &mut tree.children[trail_idx],
                renderer,
                &bounded,
            );
            trail_sz = trail_node.size();
            trail_pos = Point::new(0.0, row_h);
            total_h = row_h + trail_sz.height;
        } else {
            // 2-slot dual-wrap: lead and trail each wrap within their OWN row
            // band. When the lead itself wrapped, first try the unused space on
            // its final row before adding another row for the trail.
            lead_node = self
                .lead
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &bounded);
            lead_sz = lead_node.size();
            let lead_h = lead_sz.height.max(self.min_row_h);

            lead_pos = Point::new(0.0, (lead_h - lead_sz.height) / 2.0);
            middle_x = 0.0;
            middle_gap = 0.0;

            let shared_last_row = if self.justify_end && trail_sz.width <= width {
                let mut last_center: Option<f32> = None;
                for child in lead_node.children() {
                    let bounds = child.bounds();
                    let center = bounds.y + bounds.height / 2.0;
                    last_center = Some(last_center.map_or(center, |value| value.max(center)));
                }
                last_center.and_then(|center| {
                    let last_right = lead_node
                        .children()
                        .iter()
                        .filter_map(|child| {
                            let bounds = child.bounds();
                            let child_center = bounds.y + bounds.height / 2.0;
                            ((child_center - center).abs() < 0.5).then_some(bounds.x + bounds.width)
                        })
                        .fold(0.0f32, f32::max);
                    let trail_x = (width - trail_sz.width).max(0.0);
                    (last_right + self.spacing <= trail_x)
                        .then_some((trail_x, lead_pos.y + center - trail_sz.height / 2.0))
                })
            } else {
                None
            };

            if let Some((trail_x, trail_y)) = shared_last_row {
                trail_pos = Point::new(trail_x, trail_y);
                total_h = lead_h.max(trail_y + trail_sz.height);
            } else {
                trail_node = self.trail.as_widget_mut().layout(
                    &mut tree.children[trail_idx],
                    renderer,
                    &bounded,
                );
                trail_sz = trail_node.size();
                let trail_h = trail_sz.height.max(self.min_row_h);
                let trail_x = if self.justify_end {
                    (width - trail_sz.width).max(0.0)
                } else {
                    0.0
                };
                trail_pos = Point::new(trail_x, lead_h + (trail_h - trail_sz.height) / 2.0);
                total_h = lead_h + trail_h;
            }
        }

        let mut children: Vec<layout::Node> = Vec::with_capacity(3);
        children.push(lead_node.move_to(lead_pos));

        if has_middle {
            let mid_limits =
                layout::Limits::new(Size::new(middle_gap, 0.0), Size::new(middle_gap, row_h));
            let mut mid_node = self.middle.as_mut().unwrap().as_widget_mut().layout(
                &mut tree.children[1],
                renderer,
                &mid_limits,
            );
            let mid_y = ((row_h - mid_node.size().height) / 2.0).max(0.0);
            mid_node = mid_node.move_to(Point::new(middle_x, mid_y));
            children.push(mid_node);
        }

        children.push(trail_node.move_to(trail_pos));

        if let Some(out) = &self.height_out {
            out.store(total_h.to_bits(), Ordering::Relaxed);
        }

        layout::Node::with_children(Size::new(width, total_h), children)
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
        for ((child, state), child_layout) in self
            .refs_mut()
            .into_iter()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                state,
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
        let mut interaction = mouse::Interaction::default();
        for ((child, state), child_layout) in self
            .refs()
            .into_iter()
            .zip(tree.children.iter())
            .zip(layout.children())
        {
            let i = child.as_widget().mouse_interaction(
                state,
                child_layout,
                cursor,
                viewport,
                renderer,
            );
            if i != mouse::Interaction::default() {
                interaction = i;
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
        for ((child, state), child_layout) in self
            .refs_mut()
            .into_iter()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .operate(state, child_layout, renderer, operation);
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
        for ((child, state), child_layout) in self
            .refs()
            .into_iter()
            .zip(tree.children.iter())
            .zip(layout.children())
        {
            child.as_widget().draw(
                state,
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // At most one child (the hovered tooltip, if any) yields an overlay;
        // return the first. Borrow fields and tree slots disjointly so the
        // returned overlay can outlive this call.
        let layouts: Vec<Layout<'b>> = layout.children().collect();
        let has_middle = self.middle.is_some();

        // Split the tree children into disjoint mutable slots.
        let (lead_tree, rest) = tree.children.split_at_mut(1);
        if let Some(ll) = layouts.first() {
            if let Some(ov) = self.lead.as_widget_mut().overlay(
                &mut lead_tree[0],
                *ll,
                renderer,
                viewport,
                translation,
            ) {
                return Some(ov);
            }
        }

        if has_middle {
            let (mid_tree, trail_tree) = rest.split_at_mut(1);
            if let (Some(m), Some(ml)) = (self.middle.as_mut(), layouts.get(1)) {
                if let Some(ov) = m.as_widget_mut().overlay(
                    &mut mid_tree[0],
                    *ml,
                    renderer,
                    viewport,
                    translation,
                ) {
                    return Some(ov);
                }
            }
            if let Some(tl) = layouts.get(2) {
                return self.trail.as_widget_mut().overlay(
                    &mut trail_tree[0],
                    *tl,
                    renderer,
                    viewport,
                    translation,
                );
            }
        } else if let Some(tl) = layouts.get(1) {
            return self.trail.as_widget_mut().overlay(
                &mut rest[0],
                *tl,
                renderer,
                viewport,
                translation,
            );
        }
        None
    }
}

impl<'a> From<WrapBar<'a>> for Element<'a, Message> {
    fn from(w: WrapBar<'a>) -> Self {
        Element::new(w)
    }
}

/// Flex-wrap row: lays its items left-to-right and wraps onto a new row when
/// the next item would exceed the available width. Each row is `row_h` tall and
/// items are vertically centred. Used for the status-bar pills so they spread
/// across multiple rows when the width can't hold them on one line.
pub struct WrapFlow<'a> {
    items: Vec<Element<'a, Message>>,
    spacing_x: f32,
    /// Smallest inter-item gap when the row overflows. `INFINITY` (the default)
    /// means "never compress" — strictly opt-in per flow.
    min_spacing_x: f32,
    spacing_y: f32,
    row_h: f32,
    /// Align every wrapped row to the right edge of the available width.
    justify_end: bool,
    /// Center every row in the available width once the flow is bounded.
    center: bool,
    /// Receives the natural single-row width (as `f32` bits). Multiple flows
    /// may share one output; the widest measured value wins.
    natural_width_out: Option<Arc<AtomicU32>>,
}

impl<'a> WrapFlow<'a> {
    pub fn new(items: Vec<Element<'a, Message>>) -> Self {
        Self {
            items,
            spacing_x: 2.0,
            min_spacing_x: f32::INFINITY,
            spacing_y: 0.0,
            row_h: 28.0,
            justify_end: false,
            center: false,
            natural_width_out: None,
        }
    }

    pub fn spacing_x(mut self, s: f32) -> Self {
        self.spacing_x = s;
        self
    }

    /// Allow the inter-item gap to shrink to `s` (from `spacing_x`) when the
    /// items don't fit, so they get a bit closer before wrapping to a new row.
    pub fn min_spacing_x(mut self, s: f32) -> Self {
        self.min_spacing_x = s;
        self
    }

    pub fn row_h(mut self, h: f32) -> Self {
        self.row_h = h;
        self
    }

    pub fn justify_end(mut self, justify: bool) -> Self {
        self.justify_end = justify;
        self
    }

    /// Center each row in the available width. Unbounded, the flow is as
    /// wide as its widest row and the caller places it.
    pub fn center(mut self, center: bool) -> Self {
        self.center = center;
        self
    }

    pub fn report_natural_width(mut self, out: Arc<AtomicU32>) -> Self {
        self.natural_width_out = Some(out);
        self
    }
}

impl<'a> Widget<Message, Theme, Renderer> for WrapFlow<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(&mut self.items);
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

        // Measure each item exactly once and total their widths.
        let mut measured: Vec<layout::Node> = Vec::with_capacity(self.items.len());
        let mut sum_w = 0.0f32;
        for (item, state) in self.items.iter_mut().zip(tree.children.iter_mut()) {
            let node = item.as_widget_mut().layout(state, renderer, &natural);
            sum_w += node.size().width;
            measured.push(node);
        }

        // If this flow opted into compression and the items don't fit one row at
        // the normal gap, shrink the gap (down to min_spacing_x) so they get a
        // bit closer before wrapping. Only fires when max_w is a real bound
        // (i.e. the flow is already width-constrained, e.g. overflowing tabs).
        let n = measured.len();
        if let Some(out) = &self.natural_width_out {
            let natural_width = sum_w + n.saturating_sub(1) as f32 * self.spacing_x;
            out.fetch_max(natural_width.to_bits(), Ordering::Relaxed);
        }
        let mut eff_gap = self.spacing_x;
        if self.min_spacing_x < self.spacing_x && max_w.is_finite() && n > 1 {
            let line_w = sum_w + (n - 1) as f32 * self.spacing_x;
            if line_w > max_w {
                eff_gap =
                    ((max_w - sum_w) / (n - 1) as f32).clamp(self.min_spacing_x, self.spacing_x);
            }
        }

        let mut positioned: Vec<(layout::Node, f32, f32, usize)> = Vec::with_capacity(n);
        let mut row_widths = vec![0.0f32];
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut row_index = 0usize;

        for node in measured {
            let sz = node.size();
            if x > 0.0 && x + sz.width > max_w {
                x = 0.0;
                y += self.row_h + self.spacing_y;
                row_index += 1;
                row_widths.push(0.0);
            }
            let cy = y + ((self.row_h - sz.height) / 2.0).max(0.0);
            positioned.push((node, x, cy, row_index));
            row_widths[row_index] = x + sz.width;
            x += sz.width + eff_gap;
        }

        let used_w = row_widths.iter().copied().fold(0.0f32, f32::max);
        let flow_w = if (self.justify_end || self.center) && max_w.is_finite() {
            max_w.max(0.0)
        } else {
            used_w
        };
        let nodes = positioned
            .into_iter()
            .map(|(node, x, cy, row)| {
                let offset = if self.justify_end {
                    (flow_w - row_widths[row]).max(0.0)
                } else if self.center {
                    centered_x(flow_w, row_widths[row])
                } else {
                    0.0
                };
                node.move_to(Point::new(x + offset, cy))
            })
            .collect();
        let total_h = if self.items.is_empty() {
            self.row_h
        } else {
            y + self.row_h
        };
        layout::Node::with_children(Size::new(flow_w.max(0.0), total_h), nodes)
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
        for ((item, state), child_layout) in self
            .items
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            item.as_widget_mut().update(
                state,
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
        let mut interaction = mouse::Interaction::default();
        for ((item, state), child_layout) in self
            .items
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
        {
            let i =
                item.as_widget()
                    .mouse_interaction(state, child_layout, cursor, viewport, renderer);
            if i != mouse::Interaction::default() {
                interaction = i;
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
        for ((item, state), child_layout) in self
            .items
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            item.as_widget_mut()
                .operate(state, child_layout, renderer, operation);
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
        for ((item, state), child_layout) in self
            .items
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
        {
            item.as_widget().draw(
                state,
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.items,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<WrapFlow<'a>> for Element<'a, Message> {
    fn from(w: WrapFlow<'a>) -> Self {
        Element::new(w)
    }
}

/// Picks the widest variant that fits the available width and shows only that
/// one — used by the ribbon tool area to swap a full-size panel row for a
/// compact (icon-only) one when the window is too narrow. Variants must be
/// ordered widest-first; the last is shown when none fit.
pub struct DensitySwap<'a> {
    variants: Vec<Element<'a, Message>>,
    chosen: Cell<usize>,
    height_out: Option<Arc<AtomicU32>>,
    /// Receives the FIRST (widest) variant's natural width in bits of an `f32`,
    /// measured every layout regardless of which variant is shown — so a caller
    /// can place a neighbouring widget relative to the full-size content even
    /// while a narrower variant is on screen.
    width0_out: Vec<Arc<AtomicU32>>,
}

impl<'a> DensitySwap<'a> {
    pub fn new(variants: Vec<Element<'a, Message>>) -> Self {
        Self {
            variants,
            chosen: Cell::new(0),
            height_out: None,
            width0_out: Vec::new(),
        }
    }

    /// Report the chosen variant's height (bits of an `f32`) so callers can
    /// anchor overlays below a possibly-taller (wrapped) tool area.
    pub fn report_height(mut self, out: Arc<AtomicU32>) -> Self {
        self.height_out = Some(out);
        self
    }

    /// Report the first variant's natural (unconstrained) width — see `width0_out`.
    pub fn report_width0(mut self, out: Arc<AtomicU32>) -> Self {
        self.width0_out.push(out);
        self
    }
}

impl<'a> Widget<Message, Theme, Renderer> for DensitySwap<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(&mut self.variants);
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

        // Widest-first: keep the first variant whose natural width fits; else the
        // last (which is expected to wrap to fit any width).
        let mut pick = self.variants.len().saturating_sub(1);
        for (i, v) in self.variants.iter_mut().enumerate() {
            let n = v
                .as_widget_mut()
                .layout(&mut tree.children[i], renderer, &natural);
            if i == 0 {
                for out in &self.width0_out {
                    out.store(n.size().width.to_bits(), Ordering::Relaxed);
                }
            }
            if n.size().width <= max_w {
                pick = i;
                break;
            }
        }
        self.chosen.set(pick);

        let node =
            self.variants[pick]
                .as_widget_mut()
                .layout(&mut tree.children[pick], renderer, limits);
        let sz = node.size();
        if let Some(out) = &self.height_out {
            out.store(sz.height.to_bits(), Ordering::Relaxed);
        }
        layout::Node::with_children(sz, vec![node])
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
        let i = self.chosen.get();
        if let Some(child_layout) = layout.children().next() {
            self.variants[i].as_widget_mut().update(
                &mut tree.children[i],
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
        let i = self.chosen.get();
        layout
            .children()
            .next()
            .map(|child_layout| {
                self.variants[i].as_widget().mouse_interaction(
                    &tree.children[i],
                    child_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .unwrap_or_default()
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let i = self.chosen.get();
        if let Some(child_layout) = layout.children().next() {
            self.variants[i].as_widget_mut().operate(
                &mut tree.children[i],
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
        let i = self.chosen.get();
        if let Some(child_layout) = layout.children().next() {
            self.variants[i].as_widget().draw(
                &tree.children[i],
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let i = self.chosen.get();
        let child_layout = layout.children().next()?;
        self.variants[i].as_widget_mut().overlay(
            &mut tree.children[i],
            child_layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<DensitySwap<'a>> for Element<'a, Message> {
    fn from(w: DensitySwap<'a>) -> Self {
        Element::new(w)
    }
}

/// A transparent wrapper that records its child's screen bounds under `id` on
/// every draw, so an open dropdown can anchor its overlay just below the widget.
pub struct PosReport<'a> {
    id: std::borrow::Cow<'static, str>,
    child: Element<'a, Message>,
    /// Receives the laid-out width plus the extra px (`report_width`).
    width_out: Option<(Arc<AtomicU32>, f32)>,
}

impl<'a> PosReport<'a> {
    pub fn new(id: &'static str, child: impl Into<Element<'a, Message>>) -> Self {
        Self {
            id: std::borrow::Cow::Borrowed(id),
            child: child.into(),
            width_out: None,
        }
    }

    /// Report under a runtime-built id (e.g. one per layout tab).
    pub fn owned(id: String, child: impl Into<Element<'a, Message>>) -> Self {
        Self {
            id: std::borrow::Cow::Owned(id),
            child: child.into(),
            width_out: None,
        }
    }

    /// Also write the child's width, plus `extra` px (a gap beside it, say),
    /// into `out` each time it is laid out: a `CenterSpan` side.
    pub fn report_width(mut self, out: Arc<AtomicU32>, extra: f32) -> Self {
        self.width_out = Some((out, extra));
        self
    }
}

impl<'a> Widget<Message, Theme, Renderer> for PosReport<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
    }

    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self
            .child
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        if let Some((out, extra)) = &self.width_out {
            out.store((node.size().width + extra).to_bits(), Ordering::Relaxed);
        }
        node
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
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.child.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        DD_BOUNDS.with(|m| {
            m.borrow_mut().insert(
                self.id.to_string(),
                (layout.bounds(), FRAME.with(Cell::get)),
            );
        });
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
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.child.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<PosReport<'a>> for Element<'a, Message> {
    fn from(w: PosReport<'a>) -> Self {
        Element::new(w)
    }
}

// ── Drag-to-reorder tabs ─────────────────────────────────────────────────

#[derive(Clone)]
enum ReorderSource {
    Document {
        from: usize,
        targets: Arc<[usize]>,
    },
    Layout {
        from: String,
        targets: Arc<[String]>,
    },
}

#[derive(Default)]
struct ReorderState {
    pressed_at: Option<Point>,
    dragging: bool,
}

/// Transparent title wrapper that turns a normal tab into a drag source.
///
/// `PosReport` remains outside this wrapper and records the full target bounds.
/// Keeping this wrapper on the title only means a document tab's close button
/// can never accidentally start a reorder.
pub struct ReorderTab<'a> {
    source: ReorderSource,
    child: Element<'a, Message>,
}

impl<'a> ReorderTab<'a> {
    pub fn document(
        from: usize,
        targets: Arc<[usize]>,
        child: impl Into<Element<'a, Message>>,
    ) -> Self {
        Self {
            source: ReorderSource::Document { from, targets },
            child: child.into(),
        }
    }

    pub fn layout(
        from: String,
        targets: Arc<[String]>,
        child: impl Into<Element<'a, Message>>,
    ) -> Self {
        Self {
            source: ReorderSource::Layout { from, targets },
            child: child.into(),
        }
    }

    fn drop_target(&self, point: Point) -> Option<(Message, Rectangle, bool)> {
        match &self.source {
            ReorderSource::Document { from, targets } => targets.iter().find_map(|&to| {
                if to == *from {
                    return None;
                }
                let bounds = dropdown_bounds(&format!("DOC_TAB:{to}"))?;
                bounds.contains(point).then(|| {
                    let after = point.x >= bounds.x + bounds.width / 2.0;
                    (
                        Message::TabReorder {
                            from: *from,
                            to,
                            after,
                        },
                        bounds,
                        after,
                    )
                })
            }),
            ReorderSource::Layout { from, targets } => targets.iter().find_map(|to| {
                if to == from {
                    return None;
                }
                let bounds = dropdown_bounds(&format!("SB_LAYOUT_TAB:{to}"))?;
                bounds.contains(point).then(|| {
                    let after = point.x >= bounds.x + bounds.width / 2.0;
                    (
                        Message::LayoutReorder {
                            from: from.clone(),
                            to: to.clone(),
                            after,
                        },
                        bounds,
                        after,
                    )
                })
            }),
        }
    }
}

impl<'a> Widget<Message, Theme, Renderer> for ReorderTab<'a> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ReorderState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ReorderState::default())
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
    }

    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.child
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        const START_DISTANCE_SQUARED: f32 = 16.0;
        let state = tree.state.downcast_mut::<ReorderState>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(layout.bounds()) =>
            {
                state.pressed_at = cursor.position();
                state.dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(start) = state.pressed_at {
                    let dx = position.x - start.x;
                    let dy = position.y - start.y;
                    if state.dragging || dx * dx + dy * dy >= START_DISTANCE_SQUARED {
                        state.dragging = true;
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let was_dragging = state.dragging;
                state.pressed_at = None;
                state.dragging = false;
                if was_dragging {
                    if let Some(point) = cursor.position() {
                        if let Some((message, _, _)) = self.drop_target(point) {
                            shell.publish(message);
                        }
                    }
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
            _ => {}
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
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<ReorderState>();
        // Only a drag shows the hand: resting on a tab shows the tab's own
        // pointer, though pressing and dragging still reorders it.
        if state.dragging {
            return mouse::Interaction::Grabbing;
        }
        self.child.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        self.child.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );

        let state = tree.state.downcast_ref::<ReorderState>();
        if state.dragging {
            if let Some(point) = cursor.position() {
                if let Some((_, bounds, after)) = self.drop_target(point) {
                    let x = if after {
                        bounds.x + bounds.width - 1.0
                    } else {
                        bounds.x - 1.0
                    };
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x,
                                y: bounds.y + 2.0,
                                width: 2.0,
                                height: (bounds.height - 4.0).max(1.0),
                            },
                            border: Border::default(),
                            shadow: Shadow::default(),
                            snap: true,
                        },
                        Background::Color(theme.palette().primary.base.color),
                    );
                }
            }
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.child.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<ReorderTab<'a>> for Element<'a, Message> {
    fn from(w: ReorderTab<'a>) -> Self {
        Element::new(w)
    }
}

#[cfg(test)]
mod centering_tests {
    use super::*;
    use iced::widget::Space;

    fn block(w: f32) -> Element<'static, Message> {
        Space::new().width(w).height(20.0).into()
    }

    /// Lays `element` out `width` wide and returns its children's bounds.
    fn layout_of(mut element: Element<'static, Message>, width: f32) -> Vec<Rectangle> {
        let _one = crate::headless_ui_lock();
        let renderer = iced_test::futures::futures::executor::block_on(
            <Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
            ),
        )
        .expect("headless renderer");
        let mut tree = widget::Tree::new(element.as_widget());
        tree.diff(element.as_widget_mut());
        let limits = layout::Limits::new(Size::ZERO, Size::new(width, 200.0));
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        node.children().iter().map(|c| c.bounds()).collect()
    }

    #[test]
    fn a_centered_trail_sits_in_the_middle_of_the_bar() {
        let bar = WrapBar::new(block(100.0), block(200.0))
            .spacing(6.0)
            .center_trail(true);
        let b = layout_of(bar.into(), 1000.0);
        assert_eq!(b[0].x, 0.0);
        assert_eq!(b[1].x, 400.0);
    }

    #[test]
    fn a_centered_trail_never_covers_the_lead() {
        // The middle would start at 175, inside the 300 px lead.
        let bar = WrapBar::new(block(300.0), block(400.0))
            .spacing(6.0)
            .center_trail(true);
        let b = layout_of(bar.into(), 750.0);
        assert_eq!(b[1].x, 306.0);
    }

    #[test]
    fn a_centered_trail_centers_in_the_wider_span_around_the_bar() {
        // A bar 1000 px wide with 200 px before it and none past it: the
        // span's middle is at 400 in the bar, so the 200 px trail starts
        // at 300.
        let bar = WrapBar::new(block(100.0), block(200.0))
            .spacing(6.0)
            .center_trail(true)
            .center_span(CenterSpan::new(200.0, 0.0));
        let b = layout_of(bar.into(), 1000.0);
        assert_eq!(b[1].x, 300.0);
    }

    #[test]
    fn neighbours_report_their_room_in_the_same_layout() {
        // A 300 px block, the bar and a 100 px block, 6 px apart in a row
        // 1000 wide: the bar's 200 px trail centers in the whole row.
        let span = CenterSpan::new(0.0, 0.0);
        let row = iced::widget::row![
            PosReport::new("test_before", block(300.0)).report_width(span.before(), 6.0),
            iced::widget::container(
                WrapBar::new(block(50.0), block(200.0))
                    .spacing(6.0)
                    .center_trail(true)
                    .center_span(span.clone()),
            )
            .width(Length::Fill),
            PosReport::new("test_after", block(100.0)).report_width(span.after(), 6.0),
        ]
        .spacing(6.0);
        let mut element: Element<'static, Message> = row.into();
        let _one = crate::headless_ui_lock();
        let renderer = iced_test::futures::futures::executor::block_on(
            <Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
            ),
        )
        .expect("headless renderer");
        let mut tree = widget::Tree::new(element.as_widget());
        tree.diff(element.as_widget_mut());
        let limits = layout::Limits::new(Size::ZERO, Size::new(1000.0, 200.0));
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let bar = &node.children()[1];
        let trail = &bar.children()[0].children()[1];
        let trail_x = bar.bounds().x + trail.bounds().x;
        assert_eq!(span.insets(), (306.0, 106.0));
        assert_eq!(trail_x, 400.0, "the trail's middle is the row's");
    }

    #[test]
    fn a_span_centered_trail_stays_inside_the_bar() {
        assert_eq!(centered_in_span(1000.0, 200.0, (0.0, 0.0)), 400.0);
        assert_eq!(centered_in_span(1000.0, 200.0, (0.0, 2000.0)), 800.0);
        assert_eq!(centered_in_span(1000.0, 200.0, (2000.0, 0.0)), 0.0);
    }

    #[test]
    fn an_uncentered_trail_follows_the_lead() {
        let bar = WrapBar::new(block(100.0), block(200.0)).spacing(6.0);
        let b = layout_of(bar.into(), 1000.0);
        assert_eq!(b[1].x, 106.0);
    }

    #[test]
    fn a_centered_flow_centers_each_wrapped_row() {
        // Three 100 px items with a 10 px gap in 250 px: two on the first
        // row (210 px), one on the second.
        let flow = WrapFlow::new(vec![block(100.0), block(100.0), block(100.0)])
            .spacing_x(10.0)
            .row_h(20.0)
            .center(true);
        let b = layout_of(flow.into(), 250.0);
        assert_eq!(b[0].x, 20.0);
        assert_eq!(b[1].x, 130.0);
        assert_eq!(b[2].x, 75.0);
        assert!(b[2].y > b[0].y);
    }

    #[test]
    fn an_unbounded_centered_flow_is_as_wide_as_its_row() {
        let flow = WrapFlow::new(vec![block(100.0), block(100.0)])
            .spacing_x(10.0)
            .center(true);
        let b = layout_of(flow.into(), f32::INFINITY);
        assert_eq!(b[0].x, 0.0);
        assert_eq!(b[1].x, 110.0);
    }
}
