//! Enhanced attribute editor dialog — edit the attributes of one block
//! reference (INSERT). Opened by double-clicking a block that carries
//! attributes, or by ATTEDIT with such a block selected.
//!
//! Three tabs, mirroring the standard enhanced attribute editor:
//! * **Attribute**   — the tag / prompt / value list; the selected row's value
//!                     is edited below the list.
//! * **Text Options** — the selected attribute's text formatting (style,
//!                     justification, height, rotation, width, oblique, flags).
//! * **Properties**  — the selected attribute's common entity properties
//!                     (layer, linetype, colour, lineweight).
//!
//! This module is pure layout over a working copy ([`AttrRow`]); applying the
//! edits, undo and repaint live in the update handler (`Message::AttrEditorApply`).

use crate::app::Message;
use acadrust::entities::{HorizontalAlignment, VerticalAlignment};
use acadrust::types::{Color as AcadColor, LineWeight};
use iced::widget::{
    button, checkbox, column, container, row, scrollable, text, text_input, Space,
};
use iced::{Background, Border, Element, Length, Theme};
use crate::t;
use std::borrow::Cow;
use std::fmt;

use crate::ui::properties::{lw_options, LwItem};

const LABEL_W: f32 = 120.0;

/// Which tab of the editor is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttrTab {
    Attribute,
    TextOptions,
    Properties,
}

/// Editable working copy of one attribute. Numeric fields are kept as strings
/// so the user can type freely; they are parsed when Apply is pressed. Angles
/// are shown and entered in degrees.
#[derive(Clone)]
pub struct AttrRow {
    pub tag: String,
    pub prompt: String,
    pub value: String,
    // Text options
    pub text_style: String,
    pub height: String,
    pub rotation: String,
    pub width_factor: String,
    pub oblique: String,
    pub h_align: HorizontalAlignment,
    pub v_align: VerticalAlignment,
    pub backwards: bool,
    pub upside_down: bool,
    // Properties (common entity)
    pub layer: String,
    pub color: AcadColor,
    pub linetype: String,
    pub line_weight: LineWeight,
}

/// Combined justification options (horizontal × vertical), matching the
/// standard text-justification list. The stored labels are the English source
/// texts; `justify_label` / `justify_from_label` translate both directions.
pub const JUSTIFY: &[(&str, HorizontalAlignment, VerticalAlignment)] = &[
    ("Left", HorizontalAlignment::Left, VerticalAlignment::Baseline),
    ("Center", HorizontalAlignment::Center, VerticalAlignment::Baseline),
    ("Right", HorizontalAlignment::Right, VerticalAlignment::Baseline),
    ("Aligned", HorizontalAlignment::Aligned, VerticalAlignment::Baseline),
    ("Middle", HorizontalAlignment::Middle, VerticalAlignment::Baseline),
    ("Fit", HorizontalAlignment::Fit, VerticalAlignment::Baseline),
    ("Top Left", HorizontalAlignment::Left, VerticalAlignment::Top),
    ("Top Center", HorizontalAlignment::Center, VerticalAlignment::Top),
    ("Top Right", HorizontalAlignment::Right, VerticalAlignment::Top),
    ("Middle Left", HorizontalAlignment::Left, VerticalAlignment::Middle),
    ("Middle Center", HorizontalAlignment::Center, VerticalAlignment::Middle),
    ("Middle Right", HorizontalAlignment::Right, VerticalAlignment::Middle),
    ("Bottom Left", HorizontalAlignment::Left, VerticalAlignment::Bottom),
    ("Bottom Center", HorizontalAlignment::Center, VerticalAlignment::Bottom),
    ("Bottom Right", HorizontalAlignment::Right, VerticalAlignment::Bottom),
];

/// The justification label for an (h, v) pair (defaults to "Left"). Kept in
/// English (like the colour names) — the labels are standard CAD terms and
/// their keys would collide with ribbon tool labels such as "Right".
pub fn justify_label(h: HorizontalAlignment, v: VerticalAlignment) -> Cow<'static, str> {
    JUSTIFY
        .iter()
        .find(|(_, jh, jv)| *jh == h && *jv == v)
        .map(|(l, _, _)| Cow::Borrowed(*l))
        .unwrap_or(Cow::Borrowed("Left"))
}

/// Resolve a justification label back to its (h, v) pair.
pub fn justify_from_label(label: &str) -> Option<(HorizontalAlignment, VerticalAlignment)> {
    JUSTIFY
        .iter()
        .find(|(l, _, _)| *l == label)
        .map(|(_, h, v)| (*h, *v))
}

const COLOR_OPTIONS: &[&str] = &[
    "ByLayer", "ByBlock", "Red", "Yellow", "Green", "Cyan", "Blue", "Magenta", "White",
];

/// Display label for a colour (falls back to `Color <n>` for non-standard ACI).
pub fn color_label(c: AcadColor) -> String {
    match c {
        AcadColor::ByLayer => "ByLayer".into(),
        AcadColor::None => "None".into(),
        AcadColor::ByBlock => "ByBlock".into(),
        AcadColor::Index(1) => "Red".into(),
        AcadColor::Index(2) => "Yellow".into(),
        AcadColor::Index(3) => "Green".into(),
        AcadColor::Index(4) => "Cyan".into(),
        AcadColor::Index(5) => "Blue".into(),
        AcadColor::Index(6) => "Magenta".into(),
        AcadColor::Index(7) => "White".into(),
        AcadColor::Index(n) => crate::tf!("Color {n}").into_owned(),
        AcadColor::Rgb { r, g, b } => format!("{r},{g},{b}"),
    }
}

/// Resolve a colour label back to an `AcadColor` (None if unknown).
pub fn color_from_label(label: &str) -> Option<AcadColor> {
    Some(match label {
        "ByLayer" => AcadColor::ByLayer,
        "ByBlock" => AcadColor::ByBlock,
        "Red" => AcadColor::Index(1),
        "Yellow" => AcadColor::Index(2),
        "Green" => AcadColor::Index(3),
        "Cyan" => AcadColor::Index(4),
        "Blue" => AcadColor::Index(5),
        "Magenta" => AcadColor::Index(6),
        "White" => AcadColor::Index(7),
        _ => return None,
    })
}

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.palette();
    let border = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.neutral.color,
    };
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        icon: palette.background.base.text,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

/// Horizontal 1px divider in the shared border colour (matches style windows).
fn hdivider<'a>(width: Length) -> Element<'a, Message> {
    container(Space::new().width(width).height(1))
        .width(width)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

/// One `label : widget` row with a fixed-width label column.
fn field_row<'a>(
    label: Cow<'static, str>,
    widget: Element<'a, Message>,
    width: Length,
) -> Element<'a, Message> {
    row![
        container(text(label).size(12).style(muted_style)).width(LABEL_W),
        widget,
    ]
    .spacing(8)
    .align_y(iced::Center)
    .width(width)
    .into()
}

/// A labelled editable text field for one of the selected row's string buffers.
fn edit_field<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
    width: Length,
) -> Element<'a, Message> {
    let ti = text_input("", value)
        .on_input(on_input)
        .on_submit(Message::AttrEditorApply)
        .style(field_style)
        .size(13)
        .padding([3, 6])
        .width(width);
    field_row(label, ti.into(), width)
}

/// A labelled pick_list of owned string options.
fn pick_field<'a>(
    label: Cow<'static, str>,
    options: Vec<String>,
    selected: Option<String>,
    on_select: impl Fn(String) -> Message + 'a,
    width: Length,
) -> Element<'a, Message> {
    let pl = iced::widget::pick_list(selected, options, |value| value.to_string())
        .on_select(on_select)
        .text_size(13)
        .padding([3, 6])
        .width(width);
    field_row(label, pl.into(), width)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LocalizedChoice {
    raw: String,
    label: String,
}

impl LocalizedChoice {
    fn new(raw: String) -> Self {
        let label = raw
            .strip_prefix("Color ")
            .map(|number| crate::tf!("Color {}", number).into_owned())
            .unwrap_or_else(|| crate::i18n::translate(&raw).into_owned());
        Self { raw, label }
    }
}

impl fmt::Display for LocalizedChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// A translated pick list which keeps the stored and emitted value unchanged.
fn localized_pick_field<'a>(
    label: Cow<'static, str>,
    options: Vec<String>,
    selected: Option<String>,
    on_select: impl Fn(String) -> Message + 'a,
    width: Length,
) -> Element<'a, Message> {
    let options = options.into_iter().map(LocalizedChoice::new).collect::<Vec<_>>();
    let selected = selected.map(LocalizedChoice::new);
    let pl = iced::widget::pick_list(selected, options, |value| value.to_string())
        .on_select(move |choice: LocalizedChoice| on_select(choice.raw))
        .text_size(13)
        .padding([3, 6])
        .width(width);
    field_row(label, pl.into(), width)
}

fn tab_button<'a>(label: Cow<'static, str>, this: AttrTab, active: AttrTab) -> Element<'a, Message> {
    let is_active = this == active;
    button(text(label).size(11))
        .padding([4, 12])
        .on_press(Message::AttrEditorTab(this))
        .style(move |theme: &Theme, status| {
            let palette = theme.palette();
            let pair = match (is_active, status) {
                (true, _) => palette.primary.strong,
                (false, button::Status::Hovered | button::Status::Pressed) => {
                    palette.background.strong
                }
                _ => palette.background.weak,
            };
            button::Style {
            background: Some(Background::Color(pair.color)),
            text_color: pair.text,
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
            }
        })
        .into()
}

/// Build the enhanced attribute editor. `rows` is the working copy (attribute
/// order); `selected` is the highlighted row driving the Text Options /
/// Properties tabs. `layers` / `linetypes` / `styles` are the document's pick
/// lists.
pub fn view_window<'a>(
    block: &'a str,
    rows: &'a [AttrRow],
    selected: usize,
    tab: AttrTab,
    layers: Vec<String>,
    linetypes: Vec<String>,
    styles: Vec<String>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let width = sizing.width;
    let height = sizing.height;
    // ── Top toolbar: block name on the left, Apply on the right ───────────
    // Mirrors the style-manager windows (actions left, primary action right).
    let apply = button(text(t!("Apply")).size(11))
        .padding([4, 14])
        .on_press(Message::AttrEditorApply)
        .style(button::primary);
    let toolbar = container(
        row![
            text(t!("Block:  %{block}", block = block)).size(12).style(muted_style),
            Space::new().width(width),
            apply,
        ]
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.weak.color
        )),
        ..Default::default()
    })
    .width(width)
    .padding([5, 8]);

    let tabs = row![
        tab_button(t!("Attribute"), AttrTab::Attribute, tab),
        tab_button(t!("Text Options"), AttrTab::TextOptions, tab),
        tab_button(t!("Properties"), AttrTab::Properties, tab),
    ]
    .spacing(2);

    let body: Element<'_, Message> = if rows.is_empty() {
        text(t!("This block has no attributes.")).size(13).style(muted_style).into()
    } else {
        match tab {
            AttrTab::Attribute => attribute_tab(rows, selected, width, height),
            AttrTab::TextOptions => {
                text_options_tab(&rows[selected.min(rows.len() - 1)], styles, width, height)
            }
            AttrTab::Properties => {
                properties_tab(
                    &rows[selected.min(rows.len() - 1)],
                    layers,
                    linetypes,
                    width,
                    height,
                )
            }
        }
    };

    let content = container(column![tabs, body].spacing(8))
        .width(width)
        .height(height)
        .padding(12);

    container(column![toolbar, hdivider(width), content])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.base.color
            )),
            ..Default::default()
        })
        .width(width)
        .height(height)
        .into()
}

/// Attribute tab: tag / prompt / value list with row-select, plus a value box.
fn attribute_tab<'a>(
    rows: &'a [AttrRow],
    selected: usize,
    width: Length,
    height: Length,
) -> Element<'a, Message> {
    let head = row![
        container(text(t!("Tag")).size(11).style(muted_style)).width(130),
        container(text(t!("Prompt")).size(11).style(muted_style)).width(width),
        container(text(t!("Value")).size(11).style(muted_style)).width(140),
    ]
    .spacing(6);

    let mut list = column![].spacing(1);
    for (idx, r) in rows.iter().enumerate() {
        let is_sel = idx == selected;
        let line = row![
            container(text(r.tag.as_str()).size(12)).width(130),
            container(text(r.prompt.as_str()).size(12).style(muted_style)).width(width),
            container(text(r.value.as_str()).size(12)).width(140),
        ]
        .spacing(6);
        let btn = button(line)
            .on_press(Message::AttrEditorSelect(idx))
            .padding([3, 4])
            .width(width)
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let hovered = matches!(status, button::Status::Hovered);
                let pair = if is_sel {
                    palette.primary.strong
                } else if hovered {
                    palette.background.strong
                } else {
                    palette.background.weak
                };
                button::Style {
                    background: Some(Background::Color(pair.color)),
                    text_color: pair.text,
                    border: Border::default(),
                    ..Default::default()
                }
            });
        list = list.push(btn);
    }

    let sel = selected.min(rows.len() - 1);
    let value_box = text_input("", rows[sel].value.as_str())
        .on_input(move |v| Message::AttrEditorInput { idx: sel, value: v })
        .on_submit(Message::AttrEditorApply)
        .style(field_style)
        .size(13)
        .padding([4, 6])
        .width(width);

    column![
        head,
        scrollable(list).width(width).height(height),
        Space::new().height(8),
        field_row(t!("Value:"), value_box.into(), width),
    ]
    .spacing(6)
    .width(width)
    .height(height)
    .into()
}

/// Text Options tab: the selected attribute's text formatting.
fn text_options_tab<'a>(
    r: &'a AttrRow,
    styles: Vec<String>,
    width: Length,
    height: Length,
) -> Element<'a, Message> {
    let style_sel = if r.text_style.is_empty() {
        None
    } else {
        Some(r.text_style.clone())
    };
    let justify_opts: Vec<String> = JUSTIFY.iter().map(|(l, _, _)| (*l).to_string()).collect();
    let justify_sel = Some(justify_label(r.h_align, r.v_align).to_string());

    column![
        pick_field(t!("Text Style"), styles, style_sel, |s| {
            Message::AttrEditorTextStyle(s)
        }, width),
        localized_pick_field(t!("Justification"), justify_opts, justify_sel, |s| {
            Message::AttrEditorJustify(s)
        }, width),
        edit_field(t!("Height"), &r.height, Message::AttrEditorHeight, width),
        edit_field(t!("Rotation"), &r.rotation, Message::AttrEditorRotation, width),
        edit_field(t!("Width Factor"), &r.width_factor, Message::AttrEditorWidth, width),
        edit_field(t!("Oblique Angle"), &r.oblique, Message::AttrEditorOblique, width),
        field_row(
            Cow::Borrowed(""),
            checkbox(r.backwards)
                .label(t!("Backwards"))
                .on_toggle(Message::AttrEditorBackwards)
                .size(15)
                .text_size(12)
                .into(),
            width,
        ),
        field_row(
            Cow::Borrowed(""),
            checkbox(r.upside_down)
                .label(t!("Upside down"))
                .on_toggle(Message::AttrEditorUpsideDown)
                .size(15)
                .text_size(12)
                .into(),
            width,
        ),
    ]
    .spacing(8)
    .width(width)
    .height(height)
    .into()
}

/// Properties tab: the selected attribute's common entity properties.
fn properties_tab<'a>(
    r: &'a AttrRow,
    layers: Vec<String>,
    linetypes: Vec<String>,
    width: Length,
    height: Length,
) -> Element<'a, Message> {
    let layer_sel = Some(r.layer.clone());
    let lt_sel = Some(if r.linetype.is_empty() {
        "ByLayer".to_string()
    } else {
        r.linetype.clone()
    });
    let mut color_opts: Vec<String> = COLOR_OPTIONS.iter().map(|s| s.to_string()).collect();
    // The attribute may carry a colour outside the standard set (ACI 8-255 or
    // RGB). Surface its label so it displays correctly and stays selectable,
    // rather than showing blank; picking a listed colour still overrides it.
    let cur_color = color_label(r.color);
    if !color_opts.iter().any(|o| o == &cur_color) {
        color_opts.insert(0, cur_color.clone());
    }
    let color_sel = Some(cur_color);

    let lw_opts = lw_options();
    let lw_sel = LwItem(r.line_weight);
    let lw = iced::widget::pick_list(Some(lw_sel), lw_opts, |value| value.to_string())
    .on_select(|it: LwItem| Message::AttrEditorLineweight(it.0))
    .text_size(13)
    .padding([3, 6])
    .width(width);

    column![
        pick_field(t!("Layer"), layers, layer_sel, Message::AttrEditorLayer, width),
        localized_pick_field(t!("Linetype"), linetypes, lt_sel, Message::AttrEditorLinetype, width),
        localized_pick_field(t!("Color"), color_opts, color_sel, |s| {
            Message::AttrEditorColor(s)
        }, width),
        field_row(t!("Lineweight"), lw.into(), width),
    ]
    .spacing(8)
    .width(width)
    .height(height)
    .into()
}
