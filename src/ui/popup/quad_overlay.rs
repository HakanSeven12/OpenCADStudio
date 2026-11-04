//! Quad cursor menu — the *view*.
//!
//! Renders the panels `ui::popup::quad_menu::quad_panels_for_target` built
//! for the cursor target (docs/quad-menu.md §3.5): collapsed the single
//! default-command tile, expanded the active panel's grid with the tab strip
//! on the card's bottom edge. Every message routes through [`QuadMsg`];
//! the pick router in `app::update` translates them into command launches
//! and quad state changes, so the view holds no quad logic of its own.

use crate::app::Message;
use crate::ui::popup::quad_menu::{QuadIcon, QuadItem, QuadMsg, QuadPanel, QuadUi};
use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Background, Border, Color, Element, Length, Theme};

/// Side of one quad tile: square command buttons laid out at tool size
/// (docs/quad-menu.md §3.9, ribbon tile precedent).
pub const QUAD_TILE: f32 = 38.0;
/// Gap between the grid's tiles.
pub const QUAD_TILE_GAP: f32 = 4.0;
/// Padding around the grid inside the quad card.
pub const QUAD_PANEL_PAD: f32 = 4.0;
/// How far the hover quad's tile sits up and right of the cursor, so the
/// object under the cursor still takes a click.
pub const QUAD_HOVER_GAP: f32 = 10.0;
/// Icon canvas inside a tile (the SVG assets are 24×24).
const QUAD_ICON: f32 = 24.0;
/// Height of one bottom tab.
const QUAD_TAB_H: f32 = 20.0;

/// The collapsed quad: only the panel's default command, centred on the
/// anchor by the caller. Entering the tile expands the card
/// (`QuadMsg::Expand(true)`); pressing it in place launches the command,
/// with the cursor discipline the context menu's default row follows.
pub fn quad_collapsed_tile(default: &QuadItem, panel_id: &str) -> Element<'static, Message> {
    let tile = quad_tile(default, panel_id, 0);
    mouse_area(tile)
        .on_enter(Message::Quad(QuadMsg::Expand(true)))
        .into()
}

/// The expanded quad card: the active panel's items in `quad_width` columns,
/// with the follow-up panels as tabs on the bottom edge. The active tab's
/// content replaces the grid (BricsCAD layout); tab `0` is the primary panel.
pub fn quad_expanded_card(
    quad: &QuadUi,
    panels: &[QuadPanel],
    quad_width: usize,
) -> Element<'static, Message> {
    let cols = quad_width.max(1);
    // An out-of-range or empty tab falls back to the primary grid.
    let active = if quad.active_tab > 0 {
        panels
            .get(quad.active_tab)
            .filter(|panel| !panel.items.is_empty())
    } else {
        None
    }
    .or_else(|| panels.first());

    let mut card = column![].spacing(QUAD_TILE_GAP);
    if let Some(panel) = active {
        card = card.push(quad_grid(panel, cols));
    }

    // Tabs keep their real panel index: the tabs pick through the same
    // `QuadMsg::Tab` index the state stores (`panels` position, primary = 0).
    let mut tabs = row![].spacing(2);
    let mut tab_count = 0usize;
    for (idx, panel) in panels.iter().enumerate().skip(1) {
        if panel.items.is_empty() {
            continue;
        }
        tab_count += 1;
        tabs = tabs.push(quad_tab(idx, panel, quad.active_tab == idx));
    }
    if tab_count > 0 {
        card = card.push(container(iced::widget::rule::horizontal(1)));
        card = card.push(tabs);
    }

    // Shrink, not the container default Fill: the overlay layer is
    // full-viewport and Fill would stretch the panel background across the
    // whole screen instead of hugging the grid + tabs (the context menu's
    // card is constrained the same way — `viewport_context_menu_overlay`).
    container(card)
        .padding(QUAD_PANEL_PAD)
        .width(Length::Shrink)
        .height(Length::Shrink)
        .id(iced::widget::Id::new("quad-expanded-card"))
        .style(crate::ui::color_select::popup_panel_style)
        .into()
}

/// The active panel's items as rows of `cols` fixed tiles.
fn quad_grid(panel: &QuadPanel, cols: usize) -> Element<'static, Message> {
    let mut cells: Vec<Element<'static, Message>> = Vec::with_capacity(panel.items.len());
    for (idx, item) in panel.items.iter().enumerate() {
        cells.push(quad_tile(item, &panel.id, idx));
    }
    let mut grid = column![].spacing(QUAD_TILE_GAP);
    while !cells.is_empty() {
        let n = cells.len().min(cols);
        let mut line = row![].spacing(QUAD_TILE_GAP);
        for cell in cells.drain(..n) {
            line = line.push(cell);
        }
        grid = grid.push(line);
    }
    grid.into()
}

/// One quad cell: the item's icon at tool size on a tile, the label as a
/// tooltip below, a press emitting `QuadMsg::Pick` (every model item is
/// enabled — the quad only offers commands the target can run).
fn quad_tile(item: &QuadItem, panel_id: &str, index: usize) -> Element<'static, Message> {
    let icon = container(quad_icon(&item.icon))
        .width(Length::Fixed(QUAD_TILE))
        .height(Length::Fixed(QUAD_TILE))
        .align_x(iced::Center)
        .align_y(iced::Center);
    let btn = button(icon)
        .width(Length::Fixed(QUAD_TILE))
        .height(Length::Fixed(QUAD_TILE))
        .padding(0)
        .style(quad_tile_style)
        .on_press(Message::Quad(QuadMsg::Pick {
            panel_id: panel_id.to_string(),
            item_index: index,
        }));
    iced::widget::tooltip(
        btn,
        quad_tile_tip(item),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// A quad item's icon: command SVGs in their own colours (as the ribbon
/// draws them), plugin SVGs through [`owned_svg`] (identical geometry, the
/// bytes owned by the panel's `Arc`), the glyph fallback in the theme's
/// text colour.
fn quad_icon(icon: &QuadIcon) -> Element<'static, Message> {
    match icon {
        QuadIcon::Svg(bytes) => crate::ui::icons::tool(bytes, QUAD_ICON),
        QuadIcon::SvgOwned(bytes) => owned_svg(bytes.clone(), QUAD_ICON, 1.0).into(),
        QuadIcon::Glyph(glyph) => text(glyph.clone()).size(QUAD_ICON * 0.7).into(),
    }
}

/// `svg::Handle`s for plugin-owned artwork, keyed by the `Arc`'s data
/// pointer. The bytes are immutable and the map keeps its `Arc` clone alive,
/// so the pointer is stable identity (no eviction needed at the §4 caps);
/// building a handle (`from_memory`) copies the bytes and hashes them, and
/// this turns both into a once-per-artwork cost instead of per tile per
/// frame — the renderer's parsed-tree cache then keeps per-handle ids.
static OWNED_SVG_HANDLES: std::sync::OnceLock<
    std::sync::Mutex<
        rustc_hash::FxHashMap<usize, (std::sync::Arc<[u8]>, iced::advanced::svg::Handle)>,
    >,
> = std::sync::OnceLock::new();

fn owned_svg_handle(bytes: &std::sync::Arc<[u8]>) -> iced::advanced::svg::Handle {
    let mut handles = OWNED_SVG_HANDLES
        .get_or_init(|| std::sync::Mutex::new(rustc_hash::FxHashMap::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    handles
        .entry(std::sync::Arc::as_ptr(bytes) as *const u8 as usize)
        .or_insert_with(|| {
            (bytes.clone(), iced::advanced::svg::Handle::from_memory(bytes.to_vec()))
        })
        .1
        .clone()
}

/// A plugin-owned quad icon (API v15): the same draw as `icons::tool` —
/// square, centred, content-fitted, `opacity`-dimmed — over the panel's own
/// `Arc<[u8]>` bytes instead of a `&'static` slice (`icons.rs`'s helpers take
/// `&'static` and recolour the host's placeholder palette; plugin artwork is
/// drawn as authored — `currentColor` participates in theming natively).
///
/// The svg handle comes from [`owned_svg_handle`]: one `from_memory` build
/// per artwork (the bytes copy + hash), reused across tiles and frames.
struct OwnedSvg {
    bytes: std::sync::Arc<[u8]>,
    size: f32,
    opacity: f32,
}

fn owned_svg(bytes: std::sync::Arc<[u8]>, size: f32, opacity: f32) -> OwnedSvg {
    OwnedSvg { bytes, size, opacity }
}

impl<M> iced::advanced::Widget<M, Theme, iced::Renderer> for OwnedSvg {
    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        _renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        iced::advanced::layout::atomic(
            limits,
            Length::Fixed(self.size),
            Length::Fixed(self.size),
        )
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        use iced::advanced::svg::Renderer as _;
        use iced::advanced::svg as core_svg;
        use iced::{Point, Rectangle};
        use iced::ContentFit;

        let handle = owned_svg_handle(&self.bytes);
        let measured = renderer.measure_svg(&handle);
        if measured.width == 0 || measured.height == 0 {
            // Defensive (§4): malformed plugin artwork renders nothing, it
            // never fails the frame.
            return;
        }
        let image_size = iced::Size::new(measured.width as f32, measured.height as f32);
        // The icon keeps its own square, centred, when a stretched button
        // lays it out wider (the `tool` mirror).
        let laid = layout.bounds();
        let bounds = Rectangle::new(
            Point::new(
                laid.center_x() - self.size / 2.0,
                laid.center_y() - self.size / 2.0,
            ),
            iced::Size::new(self.size, self.size),
        );
        let svg = core_svg::Svg {
            handle,
            color: None,
            rotation: iced::Radians(0.0),
            opacity: self.opacity,
        };
        let fitted = ContentFit::Contain.fit(image_size, bounds.size());
        let position = Point::new(
            bounds.center_x() - fitted.width / 2.0,
            bounds.center_y() - fitted.height / 2.0,
        );
        renderer.draw_svg(svg, Rectangle::new(position, fitted), bounds);
    }
}

impl<'a, M: 'a> From<OwnedSvg> for iced::Element<'a, M, Theme, iced::Renderer> {
    fn from(widget: OwnedSvg) -> Self {
        iced::Element::new(widget)
    }
}

/// The tile tooltip: the item's label (the model carries no extra hint
/// text — the label is the tooltip).
fn quad_tile_tip(item: &QuadItem) -> iced::widget::Container<'static, Message> {
    // Panel titles and item labels are data (docs/quad-menu.md §3.8): the
    // source-catalog pass-through localizes the known ones and keeps
    // plug-in text verbatim.
    container(text(crate::t!(&item.label).into_owned()).size(11))
        .padding([3, 6])
        .style(container::rounded_box)
}

/// Tile colours: hover / press tint like the snap cells.
fn quad_tile_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let background = matches!(status, button::Status::Hovered | button::Status::Pressed)
        .then_some(Background::Color(palette.primary.weak.color));
    let text_color = if matches!(status, button::Status::Disabled) {
        palette.background.base.text.scale_alpha(0.42)
    } else {
        palette.background.base.text
    };
    button::Style {
        background,
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

/// A bottom tab: the panel's label on a narrow strip. Hovering or pressing
/// the tab activates it (`QuadMsg::Tab`, which the pick router keeps
/// expanded — the tab's items show under the cursor, as in commercial
/// solutions).
fn quad_tab(index: usize, panel: &QuadPanel, active: bool) -> Element<'static, Message> {
    let btn = button(
        container(text(crate::t!(&panel.label).into_owned()).size(11))
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .width(Length::Shrink)
    .height(Length::Fixed(QUAD_TAB_H))
    .padding([2, 10])
    .style(move |theme: &Theme, status| quad_tab_style(theme, status, active))
    .on_press(Message::Quad(QuadMsg::Tab(index)));
    mouse_area(btn)
        .on_enter(Message::Quad(QuadMsg::Tab(index)))
        .into()
}

/// Tab colours: the active tab keeps the tile tint, hover the same tint.
fn quad_tab_style(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let palette = theme.palette();
    let background =
        if active || matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Some(Background::Color(palette.primary.weak.color))
        } else {
            None
        };
    button::Style {
        background,
        text_color: palette.background.base.text,
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}
