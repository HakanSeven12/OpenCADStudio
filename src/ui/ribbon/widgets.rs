// Shared rendering helpers, button styles, colours, layout constants, and
// free functions used by the Ribbon view/overlay methods.

use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::time::Duration;

use codec::types::{Color as AcadColor, LineWeight};
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
use crate::t;
use crate::ui::icons;
use crate::ui::properties::{acad_color_display, linetype_display_name, LwItem};
use crate::ui::style::{chrome, ribbon_metrics as metrics};
use crate::ui::wrap_bar::PosReport;

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
    pub show_count_palette: bool,
    pub show_graphic_attributes: bool,
    pub show_authoring_palettes: bool,
    pub show_block_palette: bool,
    pub show_sheet_set: bool,
    pub show_file_tabs: bool,
    pub show_layout_tabs: bool,
}

// ── Layout constants ──────────────────────────────────────────────────────

// The sizes come from `ribbon_metrics`; these names are the ribbon's.

/// Height of a small (one-row) button; three rows and their gaps make the
/// height of a large button.
pub(super) const ROW_H: f32 = metrics::ROW_H;
/// Gap between the rows of small buttons.
pub(super) const ROW_GAP: f32 = metrics::GAP;
/// Height of a large button: three small rows.
pub(super) const LARGE_H: f32 = metrics::LARGE_H;
/// Icon size inside a large button.
pub(super) const LARGE_ICON: f32 = metrics::LARGE_ICON;
/// Icon size inside a small button.
pub(super) const SMALL_ICON: f32 = metrics::SMALL_ICON;
/// Narrowest large button.
pub(super) const LARGE_W: f32 = 52.0;
/// Horizontal button padding surrounding a large tool's label.
const LARGE_LABEL_HPAD: f32 = 8.0;
/// Large ribbon labels use at most two lines before their button grows.
const LARGE_LABEL_LINES: f32 = 2.0;
const LARGE_LABEL_SIZE: f32 = 12.0;
/// Line height of a large button's label, relative to its size.
const LARGE_LABEL_LINE: f32 = 1.15;
/// Padding around a small button's icon.
const SMALL_PAD: f32 = metrics::SMALL_PAD;
/// Width of an icon-only small button.
pub(super) const SMALL_W: f32 = metrics::SMALL_W;
/// Width of the arrow part of a split button.
pub(super) const ARROW_W: f32 = metrics::SPLIT_ARROW_W;
/// Size of button labels.
pub(super) const LABEL_SIZE: f32 = 12.0;
/// Ribbon tool-area height with panel titles.
pub(super) const TOOL_BAR_H: f32 = metrics::TOOL_AREA_H;
/// Ribbon tool-area height without panel titles.
pub(super) const TOOL_BAR_H_UNTITLED: f32 = metrics::TOOL_AREA_UNTITLED_H;
/// Height of the title strip at the bottom of a panel.
pub(super) const PANEL_TITLE_H: f32 = metrics::TITLE_STRIP_H;
/// Size of a panel title.
pub(super) const PANEL_TITLE_SIZE: f32 = 11.5;

/// Gap between the combo dropdown and the tool rows beneath it.
const COMBO_ROW_SPACING: f32 = ROW_GAP;
/// Shared padding of combo-style panels (Layers, Properties, style selectors).
/// The top inset pins every primary dropdown to the same baseline regardless
/// of how many icon rows sit underneath it.
const COMBO_PANEL_PAD: Padding = Padding {
    top: 0.0,
    bottom: 0.0,
    left: 4.0,
    right: 4.0,
};

// ── Automatic large-button sizing ────────────────────────────────────────

thread_local! {
    /// Ribbon layout runs on every pointer-driven view update. Cache the font-
    /// measured width per translated label so the automatic sizing stays cheap.
    /// Tagged with the UI-font generation it was measured with.
    static LARGE_WIDTH_CACHE: RefCell<(u32, HashMap<String, f32>)> =
        RefCell::new((0, HashMap::default()));
}

/// Dropdown items that report a state and cannot be picked.
pub(super) fn is_disabled_item(cmd: &str) -> bool {
    matches!(cmd, "FRAMES3" | "UOSNAP2")
}

fn ribbon_label_bounds(
    renderer: &iced::Renderer,
    label: &str,
    width: f32,
    wrapping: advanced_text::Wrapping,
) -> Size {
    use advanced_text::{Paragraph as _, Renderer as _};

    let paragraph =
        <iced::Renderer as advanced_text::Renderer>::Paragraph::with_text(advanced_text::Text {
            content: label,
            bounds: Size::new(width, f32::INFINITY),
            size: Pixels(LARGE_LABEL_SIZE),
            line_height: advanced_text::LineHeight::Relative(LARGE_LABEL_LINE),
            font: renderer.default_font(),
            align_x: advanced_text::Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            shaping: advanced_text::Shaping::default(),
            wrapping,
            ellipsis: advanced_text::Ellipsis::None,
            hint_factor: None,
        });
    paragraph.min_bounds()
}

/// Measure the translated label at the normal button width. It wraps first;
/// only labels that would need more than two lines widen their button. The
/// binary search uses the renderer's real font metrics, so locale and UI scale
/// changes do not rely on character-count estimates.
fn measure_large_width(renderer: &iced::Renderer, label: &str) -> f32 {
    let base_inner = (LARGE_W - LARGE_LABEL_HPAD).max(1.0);
    let line_height = advanced_text::LineHeight::Relative(LARGE_LABEL_LINE)
        .to_absolute(Pixels(LARGE_LABEL_SIZE))
        .0;
    let max_label_height = line_height * LARGE_LABEL_LINES + 0.5;
    let fits = |width: f32| {
        let bounds = ribbon_label_bounds(renderer, label, width, advanced_text::Wrapping::Word);
        bounds.width <= width + 0.5 && bounds.height <= max_label_height
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

/// The width of a large split button: its label's lines unwrapped, the last
/// one followed by the arrow when there are two.
fn measure_split_width(renderer: &iced::Renderer, label: &str) -> f32 {
    let (first, second) = split_label(label);
    let natural = |line: &str| {
        ribbon_label_bounds(renderer, line, f32::INFINITY, advanced_text::Wrapping::None).width
    };
    let widest = match second {
        Some(second) => natural(first).max(natural(second) + 2.0 + metrics::DROPDOWN_ARROW_W),
        None => natural(first),
    };
    (widest + LARGE_LABEL_HPAD).ceil().max(LARGE_W)
}

/// A large split button's label as its first line and, when it has an
/// explicit break, its second.
fn split_label(label: &str) -> (&str, Option<&str>) {
    match label.split_once('\n') {
        Some((first, second)) => (first, Some(second)),
        None => (label, None),
    }
}

fn automatic_large_width(renderer: &iced::Renderer, label: &str, split: bool) -> f32 {
    let generation = crate::sys::ui_font::generation();
    // A split button's label is measured differently: key it apart.
    let key = if split {
        format!("\u{1}{label}")
    } else {
        label.to_string()
    };
    if let Some(width) = LARGE_WIDTH_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.0 != generation {
            *cache = (generation, HashMap::default());
        }
        cache.1.get(&key).copied()
    }) {
        return width;
    }

    let width = if split {
        measure_split_width(renderer, label)
    } else {
        measure_large_width(renderer, label)
    };
    LARGE_WIDTH_CACHE.with(|cache| {
        cache.borrow_mut().1.insert(key, width);
    });
    width
}

struct AutomaticLargeWidth<'a> {
    label: String,
    split: bool,
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
        let width = automatic_large_width(renderer, &self.label, self.split);
        self.content
            .as_widget_mut()
            .layout(tree, renderer, &limits.width(Length::Fixed(width)))
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
        self.content
            .as_widget_mut()
            .update(tree, event, layout, cursor, renderer, shell, viewport);
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
    Element::new(AutomaticLargeWidth {
        label,
        split: false,
        content,
    })
}

/// A large split button sized to its label (see `measure_split_width`).
fn automatic_split_button<'a>(
    label: String,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(AutomaticLargeWidth {
        label,
        split: true,
        content,
    })
}

/// A large button's label: 12 px, centred, at most two lines.
fn large_label<'a>(label: String) -> iced::widget::Text<'a, Theme, iced::Renderer> {
    text(label)
        .size(LARGE_LABEL_SIZE)
        .line_height(LARGE_LABEL_LINE)
        .width(Fill)
        .align_x(iced::Center)
}

// ── Tab-bar constants ──────────────────────────────────────────────────────

pub(super) const TOP_ARR_W: f32 = metrics::QAT_SPLIT_ARROW_W;
pub(super) const TOP_HIST_W: f32 = metrics::QAT_BUTTON;
pub(super) const TOP_HIST_GAP: f32 = metrics::QAT_GAP;
/// Height of the quick-access buttons.
pub(super) const TOP_BUTTON_H: f32 = metrics::QAT_BUTTON;

// ── Dropdown / combo ID constants ─────────────────────────────────────────

pub(crate) const UNDO_HISTORY_ID: &str = "UNDO_HISTORY";
pub(crate) const REDO_HISTORY_ID: &str = "REDO_HISTORY";
pub(crate) const LAYER_COMBO_ID: &str = "LAYER_COMBO";
pub(super) const PROP_COLOR_ID: &str = "PROP_COLOR";
pub(super) const PROP_LINETYPE_ID: &str = "PROP_LINETYPE";
pub(super) const PROP_LW_ID: &str = "PROP_LW";

// ── Style context (passed from Ribbon to render_large) ────────────────────

#[derive(Clone, Copy)]
pub(super) struct StyleContext<'a> {
    pub text_style_names: &'a [String],
    pub active_text_style: &'a str,
    pub dim_style_names: &'a [String],
    pub active_dim_style: &'a str,
    pub mleader_style_names: &'a [String],
    pub active_mleader_style: &'a str,
    pub table_style_names: &'a [String],
    pub active_table_style: &'a str,
    /// Annotate > Text: what the find box holds and the text height shown.
    pub text_find: &'a str,
    pub text_height: &'a str,
    /// Annotate > Dimensions: DIMLAYER (empty: the current layer) and the
    /// colour of the layer it names.
    pub dim_layer: &'a str,
    pub dim_layer_color: Option<iced::Color>,
    /// Annotate > Leaders: MLEADERLAYER, likewise.
    pub mleader_layer: &'a str,
    pub mleader_layer_color: Option<iced::Color>,
}

impl<'a> StyleContext<'a> {
    pub(super) fn names_for(&self, key: StyleKey) -> &[String] {
        match key {
            StyleKey::TextStyle => self.text_style_names,
            StyleKey::DimStyle => self.dim_style_names,
            StyleKey::MLeaderStyle => self.mleader_style_names,
            StyleKey::TableStyle => self.table_style_names,
        }
    }
    pub(super) fn active_for(&self, key: StyleKey) -> &str {
        match key {
            StyleKey::TextStyle => self.active_text_style,
            StyleKey::DimStyle => self.active_dim_style,
            StyleKey::MLeaderStyle => self.active_mleader_style,
            StyleKey::TableStyle => self.active_table_style,
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
    // The buttons of a column take its width, so their outlines line up.
    let col = super::stretch_column::StretchColumn::new(std::mem::take(buf), ROW_GAP);
    out.push(col.into());
}

pub(super) fn make_icon(icon: IconKind, size: f32) -> Element<'static, Message> {
    match icon {
        IconKind::Glyph(s) => text(s).size(size * 0.7).into(),
        IconKind::Svg(bytes) => icons::tool(bytes, size),
    }
}

pub(super) fn is_active_tool(id: &str, active_tool: &Option<String>, state: &ToggleState) -> bool {
    match id {
        "ORTHO" => state.ortho_mode,
        "PERSP" => !state.ortho_mode,
        "NAVVCUBE" => state.show_viewcube,
        "UCSICON" => state.show_ucs_icon,
        "PROPERTIES" => state.show_properties,
        "COUNTLIST" => state.show_count_palette,
        "GRAPHICATTRIBUTES" => state.show_graphic_attributes,
        "BAUTHORPALETTE" => state.show_authoring_palettes,
        "BLOCKPALETTE" => state.show_block_palette,
        "SHEETSET" => state.show_sheet_set,
        "FILETAB" => state.show_file_tabs,
        "LAYOUTTAB" => state.show_layout_tabs,
        id => active_tool.as_deref() == Some(id),
    }
}

// ── Button style ───────────────────────────────────────────────────────────

/// A ribbon button: lit while its command runs or its switch is on, and on
/// hover filled with `hover` inside a `hover_border` outline.
pub(super) fn tool_btn_style(
    theme: &Theme,
    is_active: bool,
    status: button::Status,
) -> button::Style {
    let mut style = split_part_style(theme, is_active, status);
    if matches!(status, button::Status::Hovered | button::Status::Pressed) && !is_active {
        style.border.color = chrome::hover_border(theme);
        style.border.width = 1.0;
    }
    style
}

/// One part of a split button. The parts fill on hover; the outline is the
/// split button's own, around both parts (see `split_outline`).
pub(super) fn split_part_style(
    theme: &Theme,
    is_active: bool,
    status: button::Status,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let background = if is_active {
        Some(chrome::lit(theme))
    } else if hovered {
        Some(chrome::hover(theme))
    } else {
        None
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: if is_active || hovered {
            chrome::strong(theme)
        } else {
            chrome::text(theme)
        },
        border: Border {
            radius: BUTTON_RADIUS.into(),
            color: if is_active {
                chrome::lit_border(theme)
            } else {
                Color::TRANSPARENT
            },
            width: if is_active { 1.0 } else { 0.0 },
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

/// Corner radius of ribbon buttons.
pub(super) const BUTTON_RADIUS: f32 = 2.0;

/// The outline a split button shows while the pointer is on either part.
pub(super) fn split_outline<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    let outline = container(iced::widget::Space::new().width(Fill).height(Fill))
        .width(Fill)
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            border: Border {
                color: chrome::hover_border(theme),
                width: crate::ui::icons::hairline(),
                radius: BUTTON_RADIUS.into(),
            },
            ..Default::default()
        });
    iced::widget::hover(content, outline)
}

/// A combo box in a panel (layer, colour, linetype, lineweight, styles).
pub(super) fn combo_btn_style(
    theme: &Theme,
    is_open: bool,
    status: button::Status,
    _radius: f32,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered || is_open {
            chrome::hover(theme)
        } else {
            chrome::bar(theme)
        })),
        text_color: chrome::text(theme),
        border: Border {
            radius: BUTTON_RADIUS.into(),
            width: crate::ui::icons::hairline(),
            color: if is_open {
                chrome::accent(theme)
            } else if hovered {
                chrome::hover_border(theme)
            } else {
                chrome::border(theme)
            },
        },
        ..Default::default()
    }
}

/// A row of a ribbon popup or dropdown. An idle row is clear, so it does
/// not paint over the popup's border.
pub(crate) fn popup_row_style(theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| Background::Color(chrome::hover(theme))),
        text_color: if hovered {
            chrome::strong(theme)
        } else {
            chrome::text(theme)
        },
        ..Default::default()
    }
}

/// A ribbon popup or dropdown: `panel` inside a 1 px `border`.
pub(crate) fn popup_panel_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(chrome::panel(theme))),
        text_color: Some(chrome::text(theme)),
        border: Border {
            color: chrome::border(theme),
            width: crate::ui::icons::hairline(),
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn muted_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(chrome::muted(theme)),
    }
}

// ── Tooltip helpers ────────────────────────────────────────────────────────

pub(super) fn make_tip(tip: String) -> Element<'static, Message> {
    text(tip).size(11).into()
}

/// The sentence the reference shows under a tool's name, where known.
fn tool_description(id: &str) -> Option<&'static str> {
    Some(match id {
        "DCLINEAR" => "Constrains the horizontal or vertical distance between points",
        "DCHORIZONTAL" => "Constrains the X-distance between points on an object, or between two points on different objects",
        "DCVERTICAL" => "Constrains the Y-distance between points on an object, or between two points on different objects",
        "DCALIGNED" => "Constrains the distance between two points on an object or between two points on different objects",
        "DCRADIUS" => "Constrains the radius of a circle or arc",
        "DCDIAMETER" => "Constrains the diameter of a circle or arc",
        "DCANGULAR" => "Constrains the angle between line or polyline segments, the angle swept out by an arc or a polyline arc segment, or the angle between three points on objects",
        "DCCONVERT" => "Convert dimensions to dimensional constraints",
        _ => return None,
    })
}

/// A tool's tooltip: its name, its description when one is known, and the
/// command it runs.
fn tool_tip_text(label: &str, id: &str) -> String {
    // A macro (`PDFIMPORT;FILE`) shows the command it starts.
    let id = id.split(';').next().unwrap_or(id);
    match tool_description(id) {
        Some(description) => format!("{label}\n{}\n{} {id}", t!(description), t!("Command:")),
        None => format!("{label}\n{} {id}", t!("Command:")),
    }
}

pub(super) fn tip_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(chrome::panel(theme))),
        border: Border {
            color: chrome::border(theme),
            width: crate::ui::icons::hairline(),
            radius: BUTTON_RADIUS.into(),
        },
        text_color: Some(chrome::text(theme)),
        ..Default::default()
    }
}

// ── Small item renderer ────────────────────────────────────────────────────

/// A small tool with its name beside the icon, `width` wide.
fn labeled_tool<'a>(
    t: &ToolDef,
    active_tool: &Option<String>,
    state: ToggleState,
    width: Length,
) -> Element<'a, Message> {
    let active = is_active_tool(t.id, active_tool, &state);
    let event = t.event.clone();
    let tool_id = t.id.to_string();
    let label = t!(t.label).into_owned();
    let tip_text = tool_tip_text(&label, t.id);
    // One row: a label's line break becomes a space.
    let label = label.replace('\n', " ");
    let content = row![
        make_icon(t.icon, SMALL_ICON),
        super::keytips::label_anchor(
            || super::keytips::label_id(t.id),
            text(label)
                .size(LABEL_SIZE)
                .wrapping(advanced_text::Wrapping::None),
        ),
    ]
    .spacing(metrics::LABEL_GAP)
    .align_y(iced::Center);
    let btn = button(content)
        .on_press(Message::RibbonToolClick { tool_id, event })
        .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
        .width(width)
        .height(ROW_H)
        .padding(Padding {
            top: 0.0,
            right: metrics::LABEL_PAD_RIGHT,
            bottom: 0.0,
            left: SMALL_PAD,
        });
    super::keytips::anchor(
        t.id,
        tooltip(btn, make_tip(tip_text), TipPos::Right)
            .gap(6.0)
            .delay(Duration::from_millis(400))
            .style(tip_style),
    )
}

/// A small item of the Dimensions panel's bottom row: labeled tools take
/// only the width of their name there.
pub(super) fn render_row_item<'a>(
    item: &RibbonItem,
    active_tool: &Option<String>,
    open_dd: &Option<String>,
    last_cmd: &HashMap<&'static str, &'static str>,
    state: ToggleState,
) -> Element<'a, Message> {
    match item {
        RibbonItem::LabeledTool(t) => labeled_tool(t, active_tool, state, Length::Shrink),
        _ => render_small(item, active_tool, open_dd, last_cmd, state),
    }
}

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
            let tip_text = tool_tip_text(&t!(t.label), t.id);
            let btn = button(make_icon(t.icon, SMALL_ICON))
                .on_press(Message::RibbonToolClick { tool_id, event })
                .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
                .width(Fill)
                .height(ROW_H)
                .padding(SMALL_PAD);
            super::keytips::anchor(
                t.id,
                tooltip(btn, make_tip(tip_text), TipPos::Right)
                    .gap(6.0)
                    .delay(Duration::from_millis(400))
                    .style(tip_style),
            )
        }

        RibbonItem::LabeledTool(t) => labeled_tool(t, active_tool, state, Fill),

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
            let tip_text = tool_tip_text(&t!(cur_label), last);

            let icon_btn = button(make_icon(cur_icon, SMALL_ICON))
                .on_press(Message::RibbonToolClick {
                    tool_id: last.to_string(),
                    event: ModuleEvent::Command(last.to_string()),
                })
                .style(move |theme: &Theme, status| split_part_style(theme, active, status))
                .width(Fill)
                .height(ROW_H)
                .padding(SMALL_PAD);

            let arr_tip = format!("{} {}", t!(cur_label), t!("options"));
            let arr_btn = button(
                container(icons::dropdown_arrow())
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::Center)
                    .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(id.to_string()))
            .style(move |theme: &Theme, status| split_part_style(theme, dd_open, status))
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
                split_outline(
                    row![icon_with_tip, arr_with_tip]
                        .spacing(0)
                        .width(Fill)
                        .height(ROW_H)
                        .into(),
                ),
            )
            .into()
        }

        RibbonItem::LabeledDropdown {
            id,
            label,
            icon,
            items,
            default,
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
                        .find(|(candidate, _, _)| *candidate == cmd)
                        .map(|(_, _, item_icon)| *item_icon)
                })
                .or_else(|| items.first().map(|(_, _, item_icon)| *item_icon))
                .unwrap_or(*icon);
            let localized_label = if label.is_empty() {
                items
                    .iter()
                    .find(|(cmd, _, _)| *cmd == last)
                    .map(|(_, item_label, _)| t!(*item_label).into_owned())
                    .unwrap_or_default()
            } else {
                t!(*label).into_owned()
            };
            let face = row![
                make_icon(cur_icon, SMALL_ICON),
                super::keytips::label_anchor(
                    || super::keytips::label_id(id),
                    text(localized_label.replace('\n', " "))
                        .size(LABEL_SIZE)
                        .wrapping(advanced_text::Wrapping::None),
                ),
            ]
            .spacing(metrics::LABEL_GAP)
            .align_y(iced::Center);
            // A state that is only shown (not chosen) opens the list instead.
            let face_btn = button(face)
                .on_press(if is_disabled_item(last) {
                    Message::ToggleRibbonDropdown(id.to_string())
                } else {
                    Message::RibbonToolClick {
                        tool_id: last.to_string(),
                        event: ModuleEvent::Command(last.to_string()),
                    }
                })
                .style(move |theme: &Theme, status| split_part_style(theme, active, status))
                .width(Fill)
                .height(ROW_H)
                .padding(Padding {
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: SMALL_PAD,
                });
            let arrow = button(
                container(icons::dropdown_arrow())
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::Center)
                    .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(id.to_string()))
            .style(move |theme: &Theme, status| split_part_style(theme, dd_open, status))
            .width(Length::Fixed(ARROW_W))
            .height(ROW_H)
            .padding(0);
            let face_tip = tool_tip_text(&localized_label, last);
            let arrow_tip = format!("{} {}", localized_label, t!("options"));
            let parts = row![
                tooltip(face_btn, make_tip(face_tip), TipPos::Right)
                    .gap(6.0)
                    .delay(Duration::from_millis(400))
                    .style(tip_style),
                tooltip(arrow, make_tip(arrow_tip), TipPos::Right)
                    .gap(6.0)
                    .delay(Duration::from_millis(400))
                    .style(tip_style),
            ]
            .spacing(0)
            .width(Fill)
            .height(ROW_H);
            PosReport::new(*id, split_outline(parts.into())).into()
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
    pub style_ctx: &'a StyleContext<'a>,
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
        .and_then(|cmd| {
            items
                .iter()
                .find(|(c, _, _)| *c == cmd)
                .map(|(_, _, ik)| *ik)
        })
        .or_else(|| items.first().map(|(_, _, ik)| *ik))
        .unwrap_or(icon);
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
        .unwrap_or(id);
    // FOLLOW_CHOSEN: the chosen item's icon and name.
    let explicit_label =
        explicit_label.filter(|label| *label != ocs_plugin_api::ribbon::FOLLOW_CHOSEN);
    // An empty label: the face keeps its own icon and names the chosen item.
    let names_item = explicit_label == Some("");
    let cur_icon = if names_item { icon } else { cur_icon };
    let explicit_label = explicit_label.filter(|label| !label.is_empty());
    let label = t!(explicit_label.unwrap_or(cur_label)).into_owned();
    let tip_text = tool_tip_text(&t!(cur_label), last);
    let arr_tip = format!("{} {}", label, t!("options"));

    // The icon runs the current item; the label with its arrow opens the
    // list. A one-line label has the arrow under it, a two-line one at the
    // end of its second line.
    let top_btn = button(
        container(make_icon(cur_icon, LARGE_ICON))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Bottom),
    )
    .on_press(Message::RibbonToolClick {
        tool_id: last.to_string(),
        event: ModuleEvent::Command(last.to_string()),
    })
    .style(move |theme: &Theme, status| split_part_style(theme, active, status))
    .width(Fill)
    // The face lights to a pixel above the arrow part.
    .height(Length::Fixed(metrics::LARGE_FACE_H - 1.0))
    .padding(Padding {
        top: metrics::LARGE_PAD_TOP,
        right: 4.0,
        bottom: 0.0,
        left: 4.0,
    });

    let arrow = icons::dropdown_arrow();
    let (first, second) = split_label(&label);
    let lines: Element<'_, Message> = match second {
        Some(second) => column![
            large_label(first.to_string()),
            row![
                text(second.to_string())
                    .size(LARGE_LABEL_SIZE)
                    .line_height(LARGE_LABEL_LINE),
                arrow,
            ]
            .spacing(2)
            .align_y(iced::Center),
        ]
        .align_x(iced::Center)
        .width(Fill)
        .into(),
        // The arrow sits a fixed distance above the bottom, just under the
        // label.
        None => column![
            large_label(first.to_string()).wrapping(advanced_text::Wrapping::None),
            iced::widget::Space::new().height(Fill),
            arrow,
            iced::widget::Space::new().height(metrics::LARGE_ARROW_BOTTOM),
        ]
        .align_x(iced::Center)
        .width(Fill)
        .height(Fill)
        .into(),
    };
    let arr_btn = button(super::keytips::label_anchor(
        || super::keytips::label_id(id),
        lines,
    ))
    .on_press(Message::ToggleRibbonDropdown(id.to_string()))
    .style(move |theme: &Theme, status| split_part_style(theme, dd_open, status))
    .width(Fill)
    .height(Fill)
    // The label lines up with a large button's: 2 px under the icon.
    .padding(Padding {
        top: 1.0,
        right: 2.0,
        bottom: 0.0,
        left: 2.0,
    });

    let top_with_tip = tooltip(top_btn, make_tip(tip_text), TipPos::Right)
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style);
    let arr_with_tip = tooltip(arr_btn, make_tip(arr_tip), TipPos::Right)
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style);

    // The arrow part starts at `LARGE_FACE_H` and runs to the bottom.
    let content = column![top_with_tip, arr_with_tip]
        .spacing(1)
        .width(Fill)
        .height(Length::Fixed(LARGE_H));

    PosReport::new(
        id,
        automatic_split_button(label, split_outline(content.into())),
    )
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
            let icon_el: Element<Message> = make_icon(t.icon, SMALL_ICON);
            let msg = module_event_to_message(event);
            let btn = tooltip(
                button(icon_el)
                    .on_press(msg)
                    .style(move |theme: &Theme, status| tool_btn_style(theme, is_active, status))
                    .width(SMALL_W)
                    .height(ROW_H)
                    .padding(SMALL_PAD),
                make_tip(tip.to_string()),
                TipPos::Right,
            )
            .gap(4.0)
            .delay(Duration::from_millis(400))
            .style(tip_style);
            super::keytips::anchor(t.id, btn)
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
pub(super) fn render_large<'a>(item: &RibbonItem, ctx: &RenderCtx<'_>) -> Element<'a, Message> {
    render_large_with(item, ctx, Vec::new())
}

/// [`render_large`], with `below` (small labeled tools) as the bottom row of
/// the Dimensions style/layer lists.
pub(super) fn render_large_with<'a>(
    item: &RibbonItem,
    ctx: &RenderCtx<'_>,
    below: Vec<Element<'a, Message>>,
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
        RibbonItem::LargeTool(t) | RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) => {
            let active = is_active_tool(t.id, active_tool, &state);
            let event = t.event.clone();
            let tool_id = t.id.to_string();
            let label = t!(t.label).into_owned();
            let tip_text = tool_tip_text(&label, t.id);
            let btn = button(
                column![
                    container(make_icon(t.icon, LARGE_ICON))
                        .width(Fill)
                        .height(LARGE_ICON)
                        .align_x(iced::Center),
                    super::keytips::label_anchor(
                        || super::keytips::label_id(t.id),
                        large_label(label.clone()).wrapping(advanced_text::Wrapping::Word),
                    ),
                ]
                .align_x(iced::Center)
                .spacing(2)
                .width(Fill),
            )
            .on_press(Message::RibbonToolClick { tool_id, event })
            .style(move |theme: &Theme, status| tool_btn_style(theme, active, status))
            .width(Fill)
            .height(Length::Fixed(LARGE_H))
            .padding(Padding {
                top: metrics::LARGE_PAD_TOP,
                right: 4.0,
                bottom: 0.0,
                left: 4.0,
            });
            super::keytips::anchor(
                t.id,
                tooltip(
                    automatic_large_button(label, btn.into()),
                    make_tip(tip_text),
                    TipPos::Right,
                )
                .gap(6.0)
                .delay(Duration::from_millis(400))
                .style(tip_style),
            )
        }

        RibbonItem::LargeDropdown {
            id,
            label,
            icon,
            items,
            default,
        } => render_large_dropdown(*id, *icon, Some(*label), items, *default, ctx),

        RibbonItem::LabeledDropdown {
            id,
            label,
            icon,
            items,
            default,
        } => render_large_dropdown(*id, *icon, Some(*label), items, *default, ctx),

        RibbonItem::ToolGrid { columns } => columns
            .iter()
            .fold(
                row![].spacing(2).height(Fill).align_y(iced::Top),
                |row, tools| {
                    let column = tools.iter().fold(
                        column![].spacing(ROW_GAP).width(Length::Fixed(SMALL_W)),
                        |column, tool| {
                            column.push(render_small(
                                &RibbonItem::Tool(tool.clone()),
                                active_tool,
                                open_dd,
                                last_cmd,
                                state,
                            ))
                        },
                    );
                    row.push(column)
                },
            )
            .into(),

        // A plain Dropdown renders large too (used by a collapsed panel whose
        // representative tool is a dropdown).
        RibbonItem::Dropdown {
            id,
            icon,
            items,
            default,
        } => render_large_dropdown(*id, *icon, None, items, *default, ctx),

        RibbonItem::LayerComboGroup { row2, row3 } => {
            const TOOL_BUTTON_W: f32 = 26.0;
            const TOOL_SPACING: f32 = 2.0;
            const GROUP_PADDING: f32 = 8.0;
            let tool_count = row2.len().max(row3.len()) as f32;
            let tools_w = tool_count * TOOL_BUTTON_W
                + (tool_count - 1.0).max(0.0) * TOOL_SPACING
                + GROUP_PADDING;
            // Wide enough to read a typical layer name (about 25 characters)
            // in the closed combo.
            let combo_w = (LARGE_W * 2.5).max(tools_w).max(250.0);

            let info = layer_infos.iter().find(|l| l.name == active_layer);
            let lc = info.map(|l| l.color).unwrap_or(Color::WHITE);
            let lv = info.map(|l| l.visible).unwrap_or(true);
            let lf = info.map(|l| l.frozen).unwrap_or(false);
            let ll = info.map(|l| l.locked).unwrap_or(false);
            let is_open = open_dd.as_deref() == Some(LAYER_COMBO_ID);

            let vis_icon = icons::tool(icons::layer_visible(lv), SMALL_ICON);
            let freeze_icon = icons::tool(icons::layer_freeze(lf), SMALL_ICON);
            let lock_icon = icons::tool(icons::layer_lock(ll), SMALL_ICON);
            let swatch = container(text(""))
                .style(move |theme: &Theme| container::Style {
                    background: Some(Background::Color(lc)),
                    border: Border {
                        color: chrome::border(theme),
                        width: crate::ui::icons::hairline(),
                        radius: 1.0.into(),
                    },
                    ..Default::default()
                })
                .width(12)
                .height(12);

            const FIXED_COMBO_W: f32 = 14.0 * 3.0 + 12.0 + 9.0 + 4.0 * 4.0 + 8.0 * 2.0;
            let name_w = (combo_w - FIXED_COMBO_W).max(24.0);
            // About 6 px per glyph at 11 px. The dropdown itself keeps the
            // complete layer name available; only its closed ribbon label is
            // shortened to preserve the fixed row height.
            let name_budget = ((name_w / 6.0) as usize).max(4);
            let active_layer_label = crate::ui::text_util::elide(active_layer, name_budget);

            let combo_btn = button(
                row![
                    vis_icon,
                    freeze_icon,
                    lock_icon,
                    swatch,
                    container(super::keytips::label_anchor(
                        || super::keytips::label_id(LAYER_COMBO_ID),
                        text(active_layer_label).size(11),
                    ))
                    .width(name_w)
                    .clip(true),
                    icons::dropdown_arrow(),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(LAYER_COMBO_ID.to_string()))
            .style(move |theme: &Theme, status| combo_btn_style(theme, is_open, status, 3.0))
            .padding([0, 8])
            .height(metrics::COMBO_H)
            .width(Fill);

            combo_panel_col(
                combo_w,
                vec![
                    container(PosReport::new(LAYER_COMBO_ID, combo_btn))
                        .width(Fill)
                        .into(),
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
                                color: chrome::border(theme),
                                width: crate::ui::icons::hairline(),
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
                        container(super::keytips::label_anchor(
                            || super::keytips::label_id(dd_id),
                            text(label).size(11),
                        ))
                        .width(Fill)
                        .clip(true),
                        icons::dropdown_arrow(),
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .on_press(Message::ToggleRibbonDropdown(dd_id.to_string()))
                .style(move |theme: &Theme, status| combo_btn_style(theme, is_open, status, 2.0))
                .padding([0, 8])
                .height(metrics::COMBO_H)
                .width(Length::Fixed(PROP_W))
            };

            // ByLayer shows the current layer's colour.
            let color_swatch = super::layer_color(layer_infos, active_layer)
                .filter(|_| active_color == AcadColor::ByLayer)
                .unwrap_or_else(|| acad_color_display(active_color).0);
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
                .align_y(iced::Top)
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
                    container(super::keytips::label_anchor(
                        || super::keytips::label_id(combo_id),
                        text(active.clone()).size(11),
                    ))
                    .width(Fill)
                    .clip(true),
                    icons::dropdown_arrow(),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::ToggleRibbonDropdown(combo_id.to_string()))
            .style(move |theme: &Theme, status| combo_btn_style(theme, is_open, status, 3.0))
            .padding([0, 8])
            .height(metrics::COMBO_H)
            .width(Fill);

            // The open style list renders as a floating overlay
            // (`Ribbon::style_combo_overlay`) so it isn't clipped by the fixed
            // ribbon-row height — matching the Draw-tab dropdowns. (#153)
            let items_panel: Element<Message> =
                iced::widget::Space::new().width(0).height(0).into();

            let mut col_items: Vec<Element<Message>> =
                vec![
                    container(row![PosReport::new(*combo_id, combo_btn), items_panel].spacing(0))
                        .width(Fill)
                        .into(),
                ];
            // Dimensions: the style list and the dimension layer list each
            // lead a row of small tools.
            if *style_key == StyleKey::DimStyle && rows.len() == 2 {
                let combo = col_items.pop().expect("the style combo");
                let side = |tools: &[ToolDef]| {
                    container(tool_row(tools, active_tool)).width(Length::Shrink)
                };
                col_items.push(
                    row![container(combo).width(Fill), side(&rows[0])]
                        .spacing(3)
                        .align_y(iced::Center)
                        .into(),
                );
                col_items.push(
                    row![
                        container(layer_combo(
                            DIM_LAYER_COMBO,
                            style_ctx.dim_layer,
                            style_ctx.dim_layer_color,
                            open_dd.as_deref() == Some(DIM_LAYER_COMBO),
                            format!("{}\n{} DIMLAYER", t!("Dimension Layer"), t!("Command:")),
                        ))
                        .width(Fill),
                        side(&rows[1])
                    ]
                    .spacing(3)
                    .align_y(iced::Center)
                    .into(),
                );
                // The bottom row is wider than the lists with their tools.
                let width = if below.is_empty() { 1.7 } else { 2.0 };
                if !below.is_empty() {
                    col_items.push(row(below).spacing(2).align_y(iced::Center).into());
                }
                return combo_panel_col(STYLE_COMBO_W * width, col_items);
            }
            // Leaders: the style list, the multileader layer list, then the
            // small tools.
            if *style_key == StyleKey::MLeaderStyle {
                col_items.push(
                    container(layer_combo(
                        MLEADER_LAYER_COMBO,
                        style_ctx.mleader_layer,
                        style_ctx.mleader_layer_color,
                        open_dd.as_deref() == Some(MLEADER_LAYER_COMBO),
                        format!("{}\n{} MLEADERLAYER", t!("Multileader Layer"), t!("Command:")),
                    ))
                    .width(Fill)
                    .into(),
                );
            }
            // Tables: each row leads with an icon-only update button, then
            // the tool named beside its icon.
            if *style_key == StyleKey::TableStyle {
                for row_tools in rows {
                    let items: Vec<Element<Message>> = row_tools
                        .iter()
                        .enumerate()
                        .map(|(k, t)| {
                            if k == 0 {
                                tool_row(std::slice::from_ref(t), active_tool)
                            } else {
                                labeled_tool(t, active_tool, state, Length::Shrink)
                            }
                        })
                        .collect();
                    col_items.push(row(items).spacing(2).align_y(iced::Center).into());
                }
                return combo_panel_col(STYLE_COMBO_W, col_items);
            }
            for row_tools in rows {
                col_items.push(tool_row(row_tools, active_tool));
            }
            if *style_key == StyleKey::TextStyle {
                col_items.push(text_find_box(style_ctx.text_find));
                col_items.push(text_height_box(style_ctx.text_height));
            }

            // The Leaders lists are wider, so the layer list shows its name.
            let width = if *style_key == StyleKey::MLeaderStyle {
                STYLE_COMBO_W * 1.4
            } else {
                STYLE_COMBO_W
            };
            combo_panel_col(width, col_items)
        }
    }
}

/// Annotate > Dimensions: the dimension layer list's id.
pub(crate) const DIM_LAYER_COMBO: &str = "DIM_LAYER_COMBO";
/// Annotate > Leaders: the multileader layer list's id.
pub(crate) const MLEADER_LAYER_COMBO: &str = "MLEADER_LAYER_COMBO";
/// Annotate > Centerlines slide-out: the center layer list's id.
pub(crate) const CENTER_LAYER_COMBO: &str = "CENTER_LAYER_COMBO";

/// A layer list of the Annotate panels (`id`): the layer new annotations of
/// its kind go on (DIMLAYER, MLEADERLAYER), with its colour; "Use Current"
/// when it is the current layer.
pub(super) fn layer_combo<'a>(
    id: &'static str,
    layer: &str,
    color: Option<iced::Color>,
    open: bool,
    tip: String,
) -> Element<'a, Message> {
    let swatch: Element<'a, Message> = match color {
        Some(color) => container(text(""))
            .width(10)
            .height(10)
            .style(move |_: &Theme| container::Style {
                background: Some(color.into()),
                border: Border {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                    width: crate::ui::icons::hairline(),
                    radius: 2.0.into(),
                },
                ..Default::default()
            })
            .into(),
        None => iced::widget::Space::new().width(10).height(10).into(),
    };
    let label = if layer.is_empty() {
        t!("Use Current").into_owned()
    } else {
        layer.to_string()
    };
    let combo = button(
        row![
            make_icon(
                IconKind::Svg(include_bytes!("../../../assets/icons/underlay_layers.svg")),
                12.0
            ),
            swatch,
            container(text(label).size(11).wrapping(advanced_text::Wrapping::None))
                .width(Fill)
                .clip(true),
            if open {
                icons::themed_arrow_up(9.0)
            } else {
                icons::themed_arrow_down(9.0)
            },
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .on_press(Message::ToggleRibbonDropdown(id.to_string()))
    .style(move |theme: &Theme, status| combo_btn_style(theme, open, status, 3.0))
    .padding([3, 8])
    .width(Fill);
    let combo = tooltip(PosReport::new(id, combo), make_tip(tip), TipPos::Bottom)
        .delay(Duration::from_millis(400))
        .style(tip_style);
    super::keytips::anchor(id, combo)
}

/// Annotate > Text find box id, and its text height box.
pub(crate) const TEXT_FIND_ID: &str = "ribbon_text_find";
pub(crate) const TEXT_HEIGHT_ID: &str = "ribbon_text_height";

/// Annotate > Text: text to find; Enter or the magnifier opens Find and
/// Replace with it.
fn text_find_box<'a>(value: &str) -> Element<'a, Message> {
    let input = iced::widget::text_input(&t!("Find text"), value)
        .id(iced::widget::Id::new(TEXT_FIND_ID))
        .on_input(Message::RibbonTextFind)
        .on_submit(Message::RibbonTextFindSubmit)
        .size(11)
        .padding([3, 6])
        .width(Fill);
    let search = button(make_icon(
        IconKind::Svg(include_bytes!("../../../assets/icons/find.svg")),
        14.0,
    ))
    .on_press(Message::RibbonTextFindSubmit)
    .style(move |theme: &Theme, status| tool_btn_style(theme, false, status))
    .padding(3);
    let search = tooltip(search, make_tip(t!("Find text").into_owned()), TipPos::Bottom)
        .delay(Duration::from_millis(400))
        .style(tip_style);
    row![input, search]
        .spacing(2)
        .align_y(iced::Center)
        .width(Fill)
        .into()
}

/// Annotate > Text: the text height new text takes (TEXTSIZE).
fn text_height_box<'a>(value: &str) -> Element<'a, Message> {
    let input = iced::widget::text_input("", value)
        .id(iced::widget::Id::new(TEXT_HEIGHT_ID))
        .on_input(Message::RibbonTextHeight)
        .on_submit(Message::RibbonTextHeightSubmit)
        .size(11)
        .padding([3, 6])
        .width(Fill);
    let input = tooltip(
        input,
        make_tip(format!("{}\n{} TEXTSIZE", t!("Text Height"), t!("Command:"))),
        TipPos::Bottom,
    )
    .delay(Duration::from_millis(400))
    .style(tip_style);
    super::keytips::anchor(super::keytips::TEXT_HEIGHT, input)
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
/// A Quick Access button; greyed out and inert unless `enabled` (the Start
/// tab has no drawing for it).
pub(super) fn quick_access_btn<'a>(
    icon_bytes: &'static [u8],
    label: &'static str,
    cmd: &'static str,
    enabled: bool,
) -> Element<'a, Message> {
    // The bundled UI SVGs are black-stroked; tint them to a light chrome grey so
    // they read on the dark top strip (raw black is invisible there).
    let icon = if enabled {
        icons::chrome(icon_bytes, SMALL_ICON)
    } else {
        icons::chrome_disabled(icon_bytes, SMALL_ICON)
    };
    let btn = button(
        container(icon)
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press_maybe(enabled.then(|| Message::Command(cmd.to_string())))
    .style(move |theme: &Theme, status| top_hist_btn_style(theme, enabled, false, status))
    .width(Length::Fixed(TOP_HIST_W))
    .height(TOP_BUTTON_H)
    .padding([2, 0]);
    super::keytips::anchor_with(
        || super::keytips::quick_access_anchor(cmd),
        tooltip(btn, make_tip(t!(label).into_owned()), TipPos::Bottom)
            .gap(6.0)
            .delay(Duration::from_millis(400))
            .style(tip_style),
    )
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
            icons::chrome_undo(SMALL_ICON, active)
        } else {
            icons::chrome_redo(SMALL_ICON, active)
        };
        let btn = button(
            container(glyph)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
        )
        .style(move |theme: &Theme, status| qat_split_part_style(theme, active, dd_open, status))
        .width(Length::Fixed(TOP_HIST_W))
        .height(TOP_BUTTON_H)
        .padding(0);
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
            // An inactive button's muted text colour fades the arrow.
            container(icons::dropdown_arrow())
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
        )
        .style(move |theme: &Theme, status| qat_split_part_style(theme, active, dd_open, status))
        .width(Length::Fixed(TOP_ARR_W))
        .height(TOP_BUTTON_H)
        .padding(0);
        let btn = if active {
            btn.on_press(Message::ToggleRibbonDropdown(dropdown_id.to_string()))
        } else {
            btn
        };
        tooltip(
            btn,
            make_tip(format!("{}", t!("%{label} history", label = t!(label)))),
            TipPos::Right,
        )
        .gap(6.0)
        .delay(Duration::from_millis(400))
        .style(tip_style)
    };

    // A split button: its hover outline goes round both parts, inside them,
    // so it takes no room.
    let parts: Element<'a, Message> = row![main_btn, arrow_btn].spacing(0).into();
    let parts = if active { split_outline(parts) } else { parts };
    PosReport::new(dropdown_id, parts).into()
}

/// A part of a Quick Access split button: filled on hover, without the
/// outline, which is the split button's.
pub(super) fn qat_split_part_style(
    theme: &Theme,
    active: bool,
    open: bool,
    status: button::Status,
) -> button::Style {
    let mut style = top_hist_btn_style(theme, active, open, status);
    style.border.width = 0.0;
    style
}

/// A quick-access button on the tab row; `active` is false while it has
/// nothing to do (no undo step), `open` while its history list is.
pub(super) fn top_hist_btn_style(
    theme: &Theme,
    active: bool,
    open: bool,
    status: button::Status,
) -> button::Style {
    let hovered = active && matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: (open || hovered).then(|| Background::Color(chrome::hover(theme))),
        text_color: if active {
            chrome::text(theme)
        } else {
            chrome::muted(theme)
        },
        border: Border {
            radius: BUTTON_RADIUS.into(),
            color: if open || hovered {
                chrome::hover_border(theme)
            } else {
                Color::TRANSPARENT
            },
            width: crate::ui::icons::hairline(),
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn the_ribbon_has_the_design_sizes() {
        // Three 22 px rows make a 70 px large button.
        assert_eq!(LARGE_H, 70.0);
        // 99 px with panel titles, 89 px without.
        assert_eq!(TOOL_BAR_H, 99.0);
        assert_eq!(TOOL_BAR_H_UNTITLED, 89.0);
        // Two label lines fit under the icon of a large button.
        let line = LARGE_LABEL_SIZE * LARGE_LABEL_LINE;
        assert!(3.0 + LARGE_ICON + 2.0 + LARGE_LABEL_LINES * line <= LARGE_H);
    }
}

#[cfg(test)]
mod style_context_tests {
    use super::StyleContext;
    use crate::modules::StyleKey;

    #[test]
    fn style_context_borrowed_without_cloning() {
        // RED: should construct from borrowed slices/strs without cloning.
        let text_names = vec!["Standard".to_string(), "MyStyle".to_string()];
        let active_text = "Standard".to_string();
        let dim_names = vec!["DimStd".to_string()];
        let active_dim = "DimStd".to_string();
        let mleader_names: Vec<String> = vec![];
        let active_mleader = String::new();
        let table_names: Vec<String> = vec![];
        let active_table = String::new();

        let ctx = StyleContext {
            text_style_names: &text_names,
            active_text_style: &active_text,
            dim_style_names: &dim_names,
            active_dim_style: &active_dim,
            mleader_style_names: &mleader_names,
            active_mleader_style: &active_mleader,
            table_style_names: &table_names,
            active_table_style: &active_table,
            text_find: "",
            text_height: "",
            dim_layer: "",
            dim_layer_color: None,
            mleader_layer: "",
            mleader_layer_color: None,
        };
        assert_eq!(ctx.names_for(StyleKey::TextStyle), &text_names);
        assert_eq!(ctx.active_for(StyleKey::TextStyle), "Standard");
        assert_eq!(ctx.names_for(StyleKey::DimStyle), &dim_names);
    }

    #[test]
    fn style_context_no_clone_needed() {
        let names = vec!["A".to_string(), "B".to_string()];
        let active = "A".to_string();
        let empty: Vec<String> = vec![];
        let empty_s = String::new();
        let ctx = StyleContext {
            text_style_names: &names,
            active_text_style: &active,
            dim_style_names: &empty,
            active_dim_style: &empty_s,
            mleader_style_names: &empty,
            active_mleader_style: &empty_s,
            table_style_names: &empty,
            active_table_style: &empty_s,
            text_find: "",
            text_height: "",
            dim_layer: "",
            dim_layer_color: None,
            mleader_layer: "",
            mleader_layer_color: None,
        };
        assert_eq!(ctx.active_for(StyleKey::TableStyle), "");
        assert!(ctx.names_for(StyleKey::MLeaderStyle).is_empty());
    }
}
