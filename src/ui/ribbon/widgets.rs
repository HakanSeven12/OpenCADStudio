// Shared rendering helpers, button styles, colours, layout constants, and
// free functions used by the Ribbon view/overlay methods.

use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::time::Duration;

use acadrust::types::{Color as AcadColor, LineWeight};
use iced::advanced::{
    layout, mouse, overlay, renderer, text as advanced_text, widget, Layout, Shell, Widget,
};
// Ribbon tooltips anchor to the right of their button so the cursor — which
// rests on the button itself — never covers the tip text. (#143)
use iced::widget::tooltip::Position as TipPos;
use iced::widget::{button, column, container, row, text, tooltip};
use iced::{
    Background, Border, Color, Element, Event, Fill, Length, Padding, Pixels, Rectangle, Size,
    Theme, Vector,
};

use crate::app::Message;
use crate::modules::{IconKind, ModuleEvent, RibbonItem, StyleKey, ToolDef};
use crate::ui::wrap_bar::PosReport;
use crate::ui::icons;
use crate::ui::properties::{acad_color_display, linetype_display_name, LwItem};
use crate::t;

use super::LayerInfo;

/// Live on/off state of every ribbon toggle button. Passed as a single value
/// through the render path so adding a new toggle only touches `is_active_tool`
/// plus the `Ribbon::toggle_state` builder — not every render-function signature
/// and call site.
#[derive(Clone, Copy)]
pub(super) struct ToggleState {
    pub ortho_mode: bool,
    pub show_viewcube: bool,
    pub show_ucs_icon: bool,
    pub show_properties: bool,
    pub show_block_palette: bool,
    pub show_file_tabs: bool,
    pub show_layout_tabs: bool,
}

// ── Layout constants (single source of truth: ROW_H from ui::mod) ─────────

use crate::ui::ROW_H;

/// Icon size inside a 3-row (large) button.
pub(super) const LARGE_ICON: f32 = ROW_H * 1.5;
/// Icon size inside a 1-row (small) button.
pub(super) const SMALL_ICON: f32 = ROW_H * 0.7;
/// Width of a 3-row (large) button.
pub(super) const LARGE_W: f32 = ROW_H * 2.2;
/// Horizontal button padding surrounding a large tool's label.
const LARGE_LABEL_HPAD: f32 = 8.0;
/// Large ribbon labels use at most two lines before their button grows.
const LARGE_LABEL_LINES: f32 = 2.0;
const LARGE_LABEL_SIZE: f32 = 10.0;
/// Width of a 1-row (small) button.
pub(super) const SMALL_W: f32 = ROW_H;
/// Width of the ▾ strip on a small dropdown.
pub(super) const ARROW_W: f32 = ROW_H * 0.4;
/// Height of the ▾ strip at the bottom of a large dropdown.
pub(super) const LARGE_ARR: f32 = ROW_H * 0.55;
/// Total ribbon tool-area height = 3 × ROW_H + 6 px v-padding + 12 px group-label.
pub(super) const TOOL_BAR_H: f32 = 3.0 * ROW_H + 18.0;
/// Height of a collapsed panel button's large representative face (big icon +
/// its label). A collapsed button is this face plus the title opener, so it is
/// shorter than a full 3-row panel — the ribbon height follows it down.
pub(super) const COLLAPSED_FACE_H: f32 = LARGE_ICON + 20.0;

/// Gap between the combo dropdown and the tool rows beneath it.
const COMBO_ROW_SPACING: f32 = 3.0;
/// Shared padding of combo-style panels (Layers, Properties, style selectors).
/// The top inset pins every primary dropdown to the same baseline regardless
/// of how many icon rows sit underneath it.
const COMBO_PANEL_PAD: Padding = Padding {
    top: 6.0,
    bottom: 4.0,
    left: 4.0,
    right: 4.0,
};

// ── Automatic large-button sizing ────────────────────────────────────────

thread_local! {
    /// Ribbon layout runs on every pointer-driven view update. Cache the font-
    /// measured width per translated label so the automatic sizing stays cheap.
    static LARGE_WIDTH_CACHE: RefCell<HashMap<String, f32>> =
        RefCell::new(HashMap::default());
}

fn ribbon_label_bounds(
    renderer: &iced::Renderer,
    label: &str,
    width: f32,
    wrapping: advanced_text::Wrapping,
) -> Size {
    use advanced_text::{Paragraph as _, Renderer as _};

    let paragraph = <iced::Renderer as advanced_text::Renderer>::Paragraph::with_text(
        advanced_text::Text {
            content: label,
            bounds: Size::new(width, f32::INFINITY),
            size: Pixels(LARGE_LABEL_SIZE),
            line_height: advanced_text::LineHeight::default(),
            font: renderer.default_font(),
            align_x: advanced_text::Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            shaping: advanced_text::Shaping::default(),
            wrapping,
            ellipsis: advanced_text::Ellipsis::None,
            hint_factor: None,
        },
    );
    paragraph.min_bounds()
}

/// Measure the translated label at the normal button width. It wraps first;
/// only labels that would need more than two lines widen their button. The
/// binary search uses the renderer's real font metrics, so locale and UI scale
/// changes do not rely on character-count estimates.
fn measure_large_width(renderer: &iced::Renderer, label: &str) -> f32 {
    let base_inner = (LARGE_W - LARGE_LABEL_HPAD).max(1.0);
    let line_height = advanced_text::LineHeight::default()
        .to_absolute(Pixels(LARGE_LABEL_SIZE))
        .0;
    let max_label_height = line_height * LARGE_LABEL_LINES + 0.5;
    let fits = |width: f32| {
        ribbon_label_bounds(
            renderer,
            label,
            width,
            advanced_text::Wrapping::WordOrGlyph,
        )
        .height
            <= max_label_height
    };

    if fits(base_inner) {
        return LARGE_W;
    }

    let natural = ribbon_label_bounds(
        renderer,
        label,
        f32::INFINITY,
        advanced_text::Wrapping::None,
    )
    .width
    .max(base_inner);
    if !fits(natural) {
        return (natural + LARGE_LABEL_HPAD).ceil();
    }

    let mut low = base_inner;
    let mut high = natural;
    for _ in 0..10 {
        let mid = (low + high) * 0.5;
        if fits(mid) {
            high = mid;
        } else {
            low = mid;
        }
    }
    (high + LARGE_LABEL_HPAD).ceil().max(LARGE_W)
}

fn automatic_large_width(renderer: &iced::Renderer, label: &str) -> f32 {
    if let Some(width) = LARGE_WIDTH_CACHE.with(|cache| cache.borrow().get(label).copied()) {
        return width;
    }

    let width = measure_large_width(renderer, label);
    LARGE_WIDTH_CACHE.with(|cache| {
        cache.borrow_mut().insert(label.to_string(), width);
    });
    width
}

struct AutomaticLargeWidth<'a> {
    label: String,
    content: Element<'a, Message>,
}

impl Widget<Message, Theme, iced::Renderer> for AutomaticLargeWidth<'_> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        self.content.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, self.content.as_widget().size().height)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = automatic_large_width(renderer, &self.label);
        self.content.as_widget_mut().layout(
            tree,
            renderer,
            &limits.width(Length::Fixed(width)),
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

pub(super) fn automatic_large_button<'a>(
    label: String,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(AutomaticLargeWidth { label, content })
}

// ── Tab-bar constants ──────────────────────────────────────────────────────

pub(super) const TOP_ARR_W: f32 = 12.0;
pub(super) const TOP_HIST_W: f32 = 28.0;
pub(super) const TOP_HIST_GAP: f32 = 4.0;

// ── Dropdown / combo ID constants ─────────────────────────────────────────

pub(super) const UNDO_HISTORY_ID: &str = "UNDO_HISTORY";
pub(super) const REDO_HISTORY_ID: &str = "REDO_HISTORY";
pub(super) const LAYER_COMBO_ID: &str = "LAYER_COMBO";
/// Dropdown id for the tab-bar panel-density selector.
pub(super) const COLLAPSE_MODE_ID: &str = "COLLAPSE_MODE";
pub(super) const PROP_COLOR_ID: &str = "PROP_COLOR";
pub(super) const PROP_LINETYPE_ID: &str = "PROP_LINETYPE";
pub(super) const PROP_LW_ID: &str = "PROP_LW";

// ── Style context (passed from Ribbon to render_large) ────────────────────

pub(super) struct StyleContext {
    pub text_style_names: Vec<String>,
    pub active_text_style: String,
    pub dim_style_names: Vec<String>,
    pub active_dim_style: String,
    pub mleader_style_names: Vec<String>,
    pub active_mleader_style: String,
    pub table_style_names: Vec<String>,
    pub active_table_style: String,
}

impl StyleContext {
    pub(super) fn names_for(&self, key: StyleKey) -> &[String] {
        match key {
            StyleKey::TextStyle => &self.text_style_names,
            StyleKey::DimStyle => &self.dim_style_names,
            StyleKey::MLeaderStyle => &self.mleader_style_names,
            StyleKey::TableStyle => &self.table_style_names,
        }
    }
    pub(super) fn active_for(&self, key: StyleKey) -> &str {
        match key {
            StyleKey::TextStyle => &self.active_text_style,
            StyleKey::DimStyle => &self.active_dim_style,
            StyleKey::MLeaderStyle => &self.active_mleader_style,
            StyleKey::TableStyle => &self.active_table_style,
        }
    }
}

// ── Layout helpers ─────────────────────────────────────────────────────────

/// Flush up-to-3 small items as a vertical column into the group row.
pub(super) fn flush_small_col<'a>(
    buf: &mut Vec<Element<'a, Message>>,
    out: &mut Vec<Element<'a, Message>>,
) {
    if buf.is_empty() {
        return;
    }
    let col = column(std::mem::take(buf)).spacing(1);
    out.push(col.into());
}

pub(super) fn make_icon(icon: IconKind, size: f32) -> Element<'static, Message> {
    match icon {
        IconKind::Glyph(s) => text(s).size(size * 0.7).into(),
        IconKind::Svg(bytes) => icons::semantic(bytes, size),
    }
}

pub(super) fn is_active_tool(
    id: &str,
    active_tool: &Option<String>,
    state: &ToggleState,
) -> bool {
    match id {
        "ORTHO" => state.ortho_mode,
        "PERSP" => !state.ortho_mode,
        "NAVVCUBE" => state.show_viewcube,
        "UCSICON" => state.show_ucs_icon,
        "PROPERTIES" => state.show_properties,
        "BLOCKPALETTE" => state.show_block_palette,
        "FILETAB" => state.show_file_tabs,
        "LAYOUTTAB" => state.show_layout_tabs,
        id => active_tool.as_deref() == Some(id),
    }
}

// ── Button style ───────────────────────────────────────────────────────────

pub(super) fn tool_btn_style(
    theme: &Theme,
    is_active: bool,
    status: button::Status,
) -> button::Style {
    let palette = theme.palette();
    let pair = match (is_active, status) {
        (true, _) => palette.primary.weak,
        (_, button::Status::Hovered) => palette.background.weak,
        (_, button::Status::Pressed) => palette.primary.weak,
        _ => palette.background.base,
    };
    button::Style {
        background: is_active
            .then_some(Background::Color(pair.color))
            .or_else(|| matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then_some(Background::Color(pair.color))),
        text_color: pair.text,
        border: Border {
            radius: 3.0.into(),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

pub(super) fn combo_btn_style(
    theme: &Theme,
    is_open: bool,
    status: button::Status,
    radius: f32,
) -> button::Style {
    let palette = theme.palette();
    let pair = if is_open {
        palette.primary.weak
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        palette.background.weak
    } else {
        palette.background.weakest
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            radius: radius.into(),
            width: 1.0,
            color: if is_open {
                palette.primary.base.color
            } else {
                palette.background.neutral.color
            },
        },
        ..Default::default()
    }
}

pub(super) fn popup_row_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let pair = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        palette.background.weak
    } else {
        palette.background.base
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        ..Default::default()
    }
}

pub(super) fn popup_panel_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.base.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn muted_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.72)),
    }
}

// ── Tooltip helpers ────────────────────────────────────────────────────────

pub(super) fn make_tip(tip: String) -> Element<'static, Message> {
    text(tip).size(11).into()
}

pub(super) fn tip_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.strong.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: Some(palette.background.strong.text),
        ..Default::default()
    }
}

// ── Small item renderer ────────────────────────────────────────────────────

/// Render a 1-row small button (Tool or Dropdown).
pub(super) fn render_small<'a>(
    item: &RibbonItem,
    active_tool: &Option<String>,
    open_dd: &Option<String>,
    last_cmd: &HashMap<&'static str, &'static str>,
    state: ToggleState,
) -> Element<'a, Message> {
    match item {
        // Large variants render small too, so the ribbon can shrink a panel of
        // large buttons to icon-only columns when the width is tight.
        RibbonItem::Tool(t) | RibbonItem::LargeTool(t) => {
            let active = is_active_tool(t.id, active_tool, &state);
            let event = t.event.clone();
            let tool_id = t.id.to_string();
            let tip_text = format!("{}\n{} {}", t!(t.label), t!("Command:"), t.id);
            let btn = button(make_icon(t.icon, SMALL_ICON))
                .on_press(Message::RibbonToolClick { tool_id, event })
                .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
                .width(Length::Fixed(SMALL_W))
                .height(ROW_H)
                .padding([4, 4]);
            tooltip(btn, make_tip(tip_text), TipPos::Right)
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style)
                .into()
        }

        RibbonItem::Dropdown {
            id,
            icon,
            items,
            default,
            ..
        }
        | RibbonItem::LargeDropdown {
            id,
            icon,
            items,
            default,
            ..
        } => {
            let active = active_tool.as_deref() == Some(*id)
                || items
                    .iter()
                    .any(|(cmd, _, _)| active_tool.as_deref() == Some(*cmd));
            let dd_open = open_dd.as_deref() == Some(*id);
            let last = last_cmd.get(id).copied().unwrap_or(*default);
            let cur_icon = last_cmd
                .get(id)
                .copied()
                .and_then(|cmd| {
                    items
                        .iter()
                        .find(|(c, _, _)| *c == cmd)
                        .map(|(_, _, ik)| *ik)
                })
                .or_else(|| items.first().map(|(_, _, ik)| *ik))
                .unwrap_or(*icon);

            let cur_label = last_cmd
                .get(id)
                .copied()
                .and_then(|cmd| {
                    items
                        .iter()
                        .find(|(c, _, _)| *c == cmd)
                        .map(|(_, lbl, _)| *lbl)
                })
                .or_else(|| items.first().map(|(_, lbl, _)| *lbl))
                .unwrap_or(*id);
            let tip_text = format!("{}\n{} {}", t!(cur_label), t!("Command:"), last);

            let icon_btn = button(make_icon(cur_icon, SMALL_ICON))
                .on_press(Message::RibbonToolClick {
                    tool_id: last.to_string(),
                    event: ModuleEvent::Command(last.to_string()),
                })
                .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
                .width(Length::Fixed(SMALL_W))
                .height(ROW_H)
                .padding([4, 4]);

            let arr_tip = format!("{} {}", t!(cur_label), t!("options"));
            let arr_btn = button(
                container(icons::themed_arrow_down(8.0))
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::Center)
                    .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(id.to_string()))
            .style(move |theme: &Theme, status| {
                tool_btn_style(theme, dd_open, status)
            })
            .width(Length::Fixed(ARROW_W))
            .height(ROW_H)
            .padding(0);

            let icon_with_tip = tooltip(icon_btn, make_tip(tip_text), TipPos::Right)
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style);
            let arr_with_tip = tooltip(arr_btn, make_tip(arr_tip), TipPos::Right)
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style);

            PosReport::new(
                *id,
                row![icon_with_tip, arr_with_tip].spacing(0).height(ROW_H),
            )
            .into()
        }

        _ => text("").into(),
    }
}

// ── Large item renderer ────────────────────────────────────────────────────

/// Shared render context threaded through the large-item render path. Bundling
/// the per-view state keeps new fields from rippling through every render
/// signature and call site (same idea as `ToggleState`).
#[derive(Clone, Copy)]
pub(super) struct RenderCtx<'a> {
    pub active_tool: &'a Option<String>,
    pub open_dd: &'a Option<String>,
    pub last_cmd: &'a HashMap<&'static str, &'static str>,
    pub state: ToggleState,
    pub layer_infos: &'a [LayerInfo],
    pub active_layer: &'a str,
    pub active_color: AcadColor,
    pub active_linetype: &'a str,
    pub active_lineweight: LineWeight,
    pub style_ctx: &'a StyleContext,
    /// When compact, the Properties panel's Match button shrinks to a small icon.
    pub compact: bool,
}

/// A large dropdown button: the current icon on top, its ▾ directly beneath the
/// icon, then the label at the bottom. Shared by `LargeDropdown` / `Dropdown` in
/// the full ribbon and by a collapsed panel whose representative is a dropdown.
/// `explicit_label` overrides the derived (current-item) label when given.
pub(super) fn render_large_dropdown<'a>(
    id: &'static str,
    icon: IconKind,
    explicit_label: Option<&'a str>,
    items: &[(&'static str, &'static str, IconKind)],
    default: &'static str,
    ctx: &RenderCtx<'_>,
) -> Element<'a, Message> {
    let active_tool = ctx.active_tool;
    let open_dd = ctx.open_dd;
    let last_cmd = ctx.last_cmd;
    let active = active_tool.as_deref() == Some(id)
        || items
            .iter()
            .any(|(cmd, _, _)| active_tool.as_deref() == Some(*cmd));
    let dd_open = open_dd.as_deref() == Some(id);
    let last = last_cmd.get(id).copied().unwrap_or(default);
    let cur_icon = last_cmd
        .get(id)
        .copied()
        .and_then(|cmd| items.iter().find(|(c, _, _)| *c == cmd).map(|(_, _, ik)| *ik))
        .or_else(|| items.first().map(|(_, _, ik)| *ik))
        .unwrap_or(icon);
    let cur_label = last_cmd
        .get(id)
        .copied()
        .and_then(|cmd| items.iter().find(|(c, _, _)| *c == cmd).map(|(_, lbl, _)| *lbl))
        .or_else(|| items.first().map(|(_, lbl, _)| *lbl))
        .unwrap_or(id);
    let label = t!(explicit_label.unwrap_or(cur_label)).into_owned();
    let tip_text = format!("{}\n{} {}", t!(cur_label), t!("Command:"), last);
    let arr_tip = format!("{} {}", label, t!("options"));

    // The label owns the bottom of the face. The icon's Fill container centers
    // it in all remaining space between the button top and the label.
    let top_btn = button(
        column![
            container(make_icon(cur_icon, LARGE_ICON))
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
            text(label.clone())
                .size(10)
                .width(Fill)
                .align_x(iced::Center)
                .wrapping(advanced_text::Wrapping::WordOrGlyph),
        ]
        .align_x(iced::Center)
        .spacing(0)
        .width(Fill)
        .height(Fill),
    )
    .on_press(Message::RibbonToolClick {
        tool_id: last.to_string(),
        event: ModuleEvent::Command(last.to_string()),
    })
    .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
    .width(Fill)
    .height(Fill)
    .padding(Padding {
        top: 4.0,
        right: 4.0,
        bottom: 2.0,
        left: 4.0,
    });

    let arr_btn = button(
        container(icons::themed_arrow_down(9.0))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press(Message::ToggleRibbonDropdown(id.to_string()))
    .style(move |theme: &Theme, status| {
        tool_btn_style(theme, dd_open, status)
    })
    .width(Fill)
    .height(LARGE_ARR)
    .padding(0);

    let top_with_tip = tooltip(top_btn, make_tip(tip_text), TipPos::Right)
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style);
    let arr_with_tip = tooltip(arr_btn, make_tip(arr_tip), TipPos::Right)
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style);

    let content = column![top_with_tip, arr_with_tip]
        .spacing(0)
        .width(Fill)
        .height(Fill);

    PosReport::new(id, automatic_large_button(label, content.into()))
    .into()
}

/// A row of small tool buttons beneath a combo dropdown. Shared by the
/// Layers, Properties and style-combo panels so their icon rows render
/// identically.
fn tool_row<'a>(tools: &[ToolDef], active_tool: &Option<String>) -> Element<'a, Message> {
    let btns: Vec<Element<Message>> = tools
        .iter()
        .map(|t| {
            let is_active = active_tool.as_deref() == Some(t.id);
            let tip = t!(t.label);
            let event = t.event.clone();
            let icon_el: Element<Message> = make_icon(t.icon, 16.0);
            let msg = module_event_to_message(event);
            tooltip(
                button(icon_el)
                    .on_press(msg)
                    .style(move |theme: &Theme, status| {
                        tool_btn_style(theme, is_active, status)
                    })
                    .padding([2, 5]),
                make_tip(tip.to_string()),
                TipPos::Right,
            )
            .gap(4.0)
            .delay(Duration::from_millis(400))
            .style(tip_style)
            .into()
        })
        .collect();
    row(btns).spacing(2).align_y(iced::Center).into()
}

/// Fixed-height wrapper for a combo-style panel: dropdown on top, icon rows
/// below, all pinned to the panel's top edge so every panel's primary
/// dropdown shares one baseline.
fn combo_panel_col(width: f32, items: Vec<Element<'_, Message>>) -> Element<'_, Message> {
    container(column(items).spacing(COMBO_ROW_SPACING).align_x(iced::Left))
        .width(Length::Fixed(width))
        .height(Fill)
        .align_y(iced::Top)
        .padding(COMBO_PANEL_PAD)
        .into()
}

/// Render a full-height large button (LargeTool, LargeDropdown, LayerCombo, StyleCombo).
pub(super) fn render_large<'a>(
    item: &RibbonItem,
    ctx: &RenderCtx<'_>,
) -> Element<'a, Message> {
    let active_tool = ctx.active_tool;
    let open_dd = ctx.open_dd;
    let last_cmd = ctx.last_cmd;
    let state = ctx.state;
    let layer_infos = ctx.layer_infos;
    let active_layer = ctx.active_layer;
    let active_color = ctx.active_color;
    let active_linetype = ctx.active_linetype;
    let active_lineweight = ctx.active_lineweight;
    let style_ctx = ctx.style_ctx;
    let compact = ctx.compact;
    match item {
        // A plain Tool renders large too, so a collapsed panel can show its
        // representative tool as a big icon.
        RibbonItem::LargeTool(t) | RibbonItem::Tool(t) => {
            let active = is_active_tool(t.id, active_tool, &state);
            let event = t.event.clone();
            let tool_id = t.id.to_string();
            let label = t!(t.label).into_owned();
            let tip_text = format!("{}\n{} {}", label, t!("Command:"), t.id);
            let btn = button(
                column![
                    container(make_icon(t.icon, LARGE_ICON))
                        .width(Fill)
                        .height(Fill)
                        .align_x(iced::Center)
                        .align_y(iced::Center),
                    text(label.clone())
                        .size(10)
                        .width(Fill)
                        .align_x(iced::Center)
                        .wrapping(advanced_text::Wrapping::WordOrGlyph),
                ]
                .align_x(iced::Center)
                .spacing(0)
                .width(Fill)
                .height(Fill),
            )
            .on_press(Message::RibbonToolClick { tool_id, event })
            .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
            .width(Fill)
            .height(Fill)
            .padding(Padding {
                top: 4.0,
                right: 4.0,
                bottom: 4.0,
                left: 4.0,
            });
            tooltip(
                automatic_large_button(label, btn.into()),
                make_tip(tip_text),
                TipPos::Right,
            )
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style)
                .into()
        }

        RibbonItem::LargeDropdown {
            id,
            label,
            icon,
            items,
            default,
        } => {
                render_large_dropdown(*id, *icon, Some(*label), items, *default, ctx)
            }

        // A plain Dropdown renders large too (used by a collapsed panel whose
        // representative tool is a dropdown).
        RibbonItem::Dropdown {
            id,
            icon,
            items,
            default,
        } => {
                render_large_dropdown(*id, *icon, None, items, *default, ctx)
        }

        RibbonItem::LayerComboGroup { row2, row3 } => {
            const TOOL_BUTTON_W: f32 = 26.0;
            const TOOL_SPACING: f32 = 2.0;
            const GROUP_PADDING: f32 = 8.0;
            let tool_count = row2.len().max(row3.len()) as f32;
            let tools_w = tool_count * TOOL_BUTTON_W
                + (tool_count - 1.0).max(0.0) * TOOL_SPACING
                + GROUP_PADDING;
            let combo_w = (LARGE_W * 2.5).max(tools_w);

            let info = layer_infos.iter().find(|l| l.name == active_layer);
            let lc = info.map(|l| l.color).unwrap_or(Color::WHITE);
            let lv = info.map(|l| l.visible).unwrap_or(true);
            let lf = info.map(|l| l.frozen).unwrap_or(false);
            let ll = info.map(|l| l.locked).unwrap_or(false);
            let is_open = open_dd.as_deref() == Some(LAYER_COMBO_ID);

            let vis_icon = icons::semantic(icons::layer_visible(lv), 14.0);
            let freeze_icon = icons::semantic(icons::layer_freeze(lf), 14.0);
            let lock_icon = icons::semantic(icons::layer_lock(ll), 14.0);
            let swatch = container(text(""))
                .style(move |theme: &Theme| container::Style {
                    background: Some(Background::Color(lc)),
                    border: Border {
                        color: theme.palette().background.strong.color,
                        width: 1.0,
                        radius: 1.0.into(),
                    },
                    ..Default::default()
                })
                .width(12)
                .height(12);

            const FIXED_COMBO_W: f32 =
                14.0 * 3.0 + 12.0 + 9.0 + 4.0 * 4.0 + 8.0 * 2.0;
            let name_w = (combo_w - FIXED_COMBO_W).max(24.0);
            // About 6 px per glyph at 11 px. The dropdown itself keeps the
            // complete layer name available; only its closed ribbon label is
            // shortened to preserve the fixed row height.
            let name_budget = ((name_w / 6.0) as usize).max(4);
            let active_layer_label =
                crate::ui::text_util::elide(active_layer, name_budget);

            let combo_btn = button(
                row![
                    vis_icon,
                    freeze_icon,
                    lock_icon,
                    swatch,
                    container(text(active_layer_label).size(11))
                        .width(name_w)
                        .clip(true),
                    icons::themed_arrow_down(9.0),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(LAYER_COMBO_ID.to_string()))
            .style(move |theme: &Theme, status| {
                combo_btn_style(theme, is_open, status, 3.0)
            })
            .padding([3, 8])
            .width(Fill);

            combo_panel_col(
                combo_w,
                vec![
                    container(PosReport::new(LAYER_COMBO_ID, combo_btn)).width(Fill).into(),
                    tool_row(row2, active_tool),
                    tool_row(row3, active_tool),
                ],
            )
        }

        RibbonItem::PropertiesGroup { match_prop } => {
            // The Match Properties button is a large icon normally, but shrinks
            // to a small icon when the panel is compacted.
            let mp_el: Element<'a, Message> = if compact {
                render_small(
                    &RibbonItem::Tool(match_prop.clone()),
                    active_tool,
                    open_dd,
                    last_cmd,
                    state,
                )
            } else {
                render_large(
                    &RibbonItem::LargeTool(match_prop.clone()),
                    &RenderCtx {
                        compact: false,
                        ..*ctx
                    },
                )
            };

            const PROP_W: f32 = 130.0;

            let prop_row = |label: String, dd_id: &'static str, swatch: Option<Color>| {
                let is_open = open_dd.as_deref() == Some(dd_id);
                let swatch_el: Element<'a, Message> = if let Some(c) = swatch {
                    container(text(""))
                        .style(move |theme: &Theme| container::Style {
                            background: Some(Background::Color(c)),
                            border: Border {
                                color: theme.palette().background.strong.color,
                                width: 1.0,
                                radius: 1.0.into(),
                            },
                            ..Default::default()
                        })
                        .width(12)
                        .height(12)
                        .into()
                } else {
                    iced::widget::Space::new().width(0).into()
                };
                button(
                    row![
                        swatch_el,
                        container(text(label).size(10))
                            .width(Fill)
                            .clip(true),
                        if is_open {
                            icons::themed_arrow_up(8.0)
                        } else {
                            icons::themed_arrow_down(8.0)
                        },
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .on_press(Message::ToggleRibbonDropdown(dd_id.to_string()))
                .style(move |theme: &Theme, status| {
                    combo_btn_style(theme, is_open, status, 2.0)
                })
                .padding([3, 8])
                .width(Length::Fixed(PROP_W))
            };

            let (color_swatch, _) = acad_color_display(active_color);
            let color_row = prop_row(
                crate::ui::color_select::color_display_name(active_color),
                PROP_COLOR_ID,
                Some(color_swatch),
            );
            let lt_row = prop_row(
                linetype_display_name(active_linetype),
                PROP_LINETYPE_ID,
                None,
            );
            let lw_row = prop_row(LwItem(active_lineweight).to_string(), PROP_LW_ID, None);

            // Top-pinned like every combo panel, so the Color dropdown shares
            // the same baseline as the Layers / style selectors.
            let combos = container(
                column![
                    PosReport::new(PROP_COLOR_ID, color_row),
                    PosReport::new(PROP_LINETYPE_ID, lt_row),
                    PosReport::new(PROP_LW_ID, lw_row),
                ]
                .spacing(2)
                .align_x(iced::Left),
            )
            .height(Fill)
            .align_y(iced::Top)
            .padding(Padding {
                top: COMBO_PANEL_PAD.top,
                bottom: COMBO_PANEL_PAD.bottom,
                left: 0.0,
                right: COMBO_PANEL_PAD.right,
            });

            row![mp_el, combos]
                .spacing(4)
                .align_y(iced::Center)
                .height(Fill)
                .into()
        }

        RibbonItem::StyleComboGroup {
            style_key,
            combo_id,
            rows,
            ..
        } => {
            const STYLE_COMBO_W: f32 = LARGE_W * 2.3;
            let active: String = style_ctx.active_for(*style_key).to_string();
            let is_open = open_dd.as_deref() == Some(*combo_id);

            // ── combo button ──
            let combo_btn = button(
                row![
                    container(text(active.clone()).size(11))
                        .width(Fill)
                        .clip(true),
                    if is_open {
                        icons::themed_arrow_up(9.0)
                    } else {
                        icons::themed_arrow_down(9.0)
                    },
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(combo_id.to_string()))
            .style(move |theme: &Theme, status| {
                combo_btn_style(theme, is_open, status, 3.0)
            })
            .padding([3, 8])
            .width(Fill);

            // The open style list renders as a floating overlay
            // (`Ribbon::style_combo_overlay`) so it isn't clipped by the fixed
            // ribbon-row height — matching the Draw-tab dropdowns. (#153)
            let items_panel: Element<Message> =
                iced::widget::Space::new().width(0).height(0).into();

            let mut col_items: Vec<Element<Message>> =
                vec![container(row![PosReport::new(*combo_id, combo_btn), items_panel].spacing(0))
                    .width(Fill)
                    .into()];
            for row_tools in rows {
                col_items.push(tool_row(row_tools, active_tool));
            }

            combo_panel_col(STYLE_COMBO_W, col_items)
        }
    }
}

// ── Message helpers ────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn module_event_to_message(event: ModuleEvent) -> Message {
    match event {
        ModuleEvent::Command(cmd) => Message::Command(cmd),
        ModuleEvent::OpenFileDialog => Message::OpenFile,
        ModuleEvent::ClearModels => Message::ClearScene,
        ModuleEvent::SetVisualStyle(name) => {
            match crate::modules::view::visual_style::mode_for_keyword(&name) {
                Some(mode) => Message::SetRenderMode(mode),
                // Report through the command line rather than silently doing
                // nothing, so a plugin author sees the typo.
                None => Message::Command(format!("VISUALSTYLES {name}")),
            }
        }
        ModuleEvent::ToggleLayers => Message::ToggleLayers,
        // Needs the tool context + async picker — route through the normal
        // ribbon-click handler rather than a direct 1:1 message.
        e @ ModuleEvent::PluginFileDialog { .. } => Message::RibbonToolClick {
            tool_id: String::new(),
            event: e,
        },
    }
}

// ── History control ────────────────────────────────────────────────────────

/// Quick-access chrome button (New / Open / Save / Save As / Print) in the top
/// strip: an SVG icon that dispatches a command string, with a hover tooltip.
pub(super) fn quick_access_btn<'a>(
    icon_bytes: &'static [u8],
    label: &'static str,
    cmd: &'static str,
) -> Element<'a, Message> {
    // The bundled UI SVGs are black-stroked; tint them to a light chrome grey so
    // they read on the dark top strip (raw black is invisible there).
    let icon = icons::themed(icon_bytes, 16.0);
    let btn = button(
        container(icon)
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press(Message::Command(cmd.to_string()))
    .style(button::subtle)
    .width(Length::Fixed(TOP_HIST_W))
    .height(24)
    .padding([2, 0]);
    tooltip(btn, make_tip(t!(label).into_owned()), TipPos::Bottom)
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style)
        .into()
}

pub(super) fn render_history_control<'a>(
    label: &'static str,
    dropdown_id: &'static str,
    count: usize,
    open_dropdown: &Option<String>,
) -> Element<'a, Message> {
    let dd_open = open_dropdown.as_deref() == Some(dropdown_id);
    let active = count > 0;

    let main_btn = {
        let glyph = if dropdown_id == UNDO_HISTORY_ID {
            icons::themed_undo(15.0, active)
        } else {
            icons::themed_redo(15.0, active)
        };
        let btn = button(
            container(glyph)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
        )
        .style(move |theme: &Theme, status| {
            top_hist_btn_style(theme, active, dd_open, status)
        })
        .width(Length::Fixed(TOP_HIST_W))
        .height(24)
        .padding([2, 0]);
        let btn = if active {
            if dropdown_id == UNDO_HISTORY_ID {
                btn.on_press(Message::Undo)
            } else {
                btn.on_press(Message::Redo)
            }
        } else {
            btn
        };
        tooltip(
            btn,
            make_tip(format!(
                "{}\n{}",
                t!(label),
                t!("%{count} steps available", count = count)
            )),
            TipPos::Right,
        )
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style)
    };

    let arrow_btn = {
        let btn = button(
            container(if active {
                icons::themed_arrow_down(8.0)
            } else {
                icons::themed_disabled_arrow_down(8.0)
            })
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
        )
        .style(move |theme: &Theme, status| {
            top_hist_btn_style(theme, active, dd_open, status)
        })
        .width(Length::Fixed(TOP_ARR_W))
        .height(24)
        .padding(0);
        let btn = if active {
            btn.on_press(Message::ToggleRibbonDropdown(dropdown_id.to_string()))
        } else {
            btn
        };
        tooltip(
            btn,
            make_tip(format!(
                "{}",
                t!("%{label} history", label = t!(label))
            )),
            TipPos::Right,
        )
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style)
    };

    PosReport::new(dropdown_id, row![main_btn, arrow_btn].spacing(0)).into()
}

pub(super) fn top_hist_btn_style(
    theme: &Theme,
    active: bool,
    open: bool,
    status: button::Status,
) -> button::Style {
    let palette = theme.palette();
    let pair = match (active, open, status) {
        (false, _, _) => palette.background.weakest,
        (_, true, _) => palette.primary.weak,
        (_, _, button::Status::Hovered) => palette.background.weak,
        (_, _, button::Status::Pressed) => palette.primary.weak,
        _ => palette.background.base,
    };
    button::Style {
        background: (!active || open || matches!(
            status,
            button::Status::Hovered | button::Status::Pressed
        ))
        .then_some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            radius: 3.0.into(),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}
