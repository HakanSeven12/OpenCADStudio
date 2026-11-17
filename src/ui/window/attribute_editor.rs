//! Enhanced attribute editor — edit the attributes of one block reference
//! (INSERT). Opened by EATTEDIT, by double-clicking a block that carries
//! attributes, or from the Properties palette.
//!
//! The window is one session: every change shows in the drawing at once,
//! Apply keeps what is there, Cancel takes back what was not applied, and
//! the whole session is one undo step. Constant attributes are not listed.
//!
//! Three tabs:
//! * **Attribute**    — the tag / prompt / value list; the selected row's
//!                      value is edited below it (multi-line values in the
//!                      text editor, fields from the Field dialog).
//! * **Text Options** — the selected attribute's text formatting.
//! * **Properties**   — the selected attribute's layer, linetype, colour,
//!                      lineweight (plot style shown, not editable).
//!
//! This module is layout over a working copy ([`AttrRow`]); the session lives
//! in `app::attr_editor`.

use crate::app::Message;
use crate::t;
use document::entities::{HorizontalAlignment, VerticalAlignment};
use document::types::{Color as AcadColor, LineWeight};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};
use std::borrow::Cow;
use std::fmt;

use crate::ui::properties::{lw_options, LwItem};
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{accent_text, card, card_style, chip, well_style};

const LABEL_W: f32 = 104.0;

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
    /// Constant attributes cannot be edited.
    pub is_constant: bool,
    /// Position of this attribute in the block reference's attribute list.
    pub index: usize,
    /// A multi-line attribute: its value is edited in the text editor and
    /// its text options are those of multi-line text.
    pub multiline: bool,
    /// Boundary width of a multi-line attribute (0 = no wrapping).
    pub boundary_width: String,
    /// Annotative attribute (shown; the block decides).
    pub annotative: bool,
    /// A field (code and referenced objects) the value was set from.
    pub field: Option<(String, Vec<document::Handle>)>,
    /// The value was retyped or a field inserted: the stored field is
    /// replaced when the row is written.
    pub field_changed: bool,
}

impl PartialEq for AttrRow {
    /// Rows compare by what the user can change, so a pending edit is one
    /// that differs from what was last written.
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
            && self.text_style == other.text_style
            && self.height == other.height
            && self.rotation == other.rotation
            && self.width_factor == other.width_factor
            && self.oblique == other.oblique
            && self.h_align == other.h_align
            && self.v_align == other.v_align
            && self.backwards == other.backwards
            && self.upside_down == other.upside_down
            && self.layer == other.layer
            && self.color == other.color
            && self.linetype == other.linetype
            && self.line_weight == other.line_weight
            && self.boundary_width == other.boundary_width
            && self.field_changed == other.field_changed
    }
}

/// The value as one line: the list and a multi-line row's value box show the
/// paragraph breaks as spaces.
pub fn one_line(value: &str) -> String {
    value.replace("\\P", " ").replace('\n', " ")
}

/// Combined justification options (horizontal × vertical), matching the
/// standard text-justification list. The stored labels are the English source
/// texts; `justify_label` / `justify_from_label` translate both directions.
pub const JUSTIFY: &[(&str, HorizontalAlignment, VerticalAlignment)] = &[
    ("Left", HorizontalAlignment::Left, VerticalAlignment::Baseline),
    ("Aligned", HorizontalAlignment::Aligned, VerticalAlignment::Baseline),
    ("Fit", HorizontalAlignment::Fit, VerticalAlignment::Baseline),
    ("Center", HorizontalAlignment::Center, VerticalAlignment::Baseline),
    ("Middle", HorizontalAlignment::Middle, VerticalAlignment::Baseline),
    ("Right", HorizontalAlignment::Right, VerticalAlignment::Baseline),
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

/// A multi-line attribute is placed by one of the nine corners and sides of
/// its text box: the last nine entries of [`JUSTIFY`].
pub fn justify_options(multiline: bool) -> &'static [(&'static str, HorizontalAlignment, VerticalAlignment)] {
    if multiline {
        &JUSTIFY[6..]
    } else {
        JUSTIFY
    }
}

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


/// The last entry of the colour list: the colour window.
pub const SELECT_COLOR: &str = "Select Color...";

/// The answer to "the attributes have been changed" before another block is
/// picked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttrAnswer {
    /// Write the changes, then pick.
    Save,
    /// Take the changes back, then pick.
    Discard,
    /// Stay on this block.
    Stay,
}

/// What the window shows.
pub struct AttrEditorView<'a> {
    pub block: &'a str,
    pub rows: &'a [AttrRow],
    pub selected: usize,
    pub tab: AttrTab,
    pub layers: Vec<String>,
    pub linetypes: Vec<String>,
    pub styles: Vec<String>,
    pub plot_style: String,
    /// There are changes Apply would write.
    pub pending: bool,
    pub error: Option<&'a str>,
    /// The "keep these changes?" question before Select block.
    pub asking: bool,
}

static PICK_BLOCK: &[u8] = include_bytes!("../../../assets/icons/ui/pick_point.svg");
static ICON_WARN: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");
static ICON_ANNOTATIVE: &[u8] = include_bytes!("../../../assets/icons/ui/annotative.svg");

fn label<'a>(value: Cow<'static, str>, enabled: bool) -> Element<'a, Message> {
    let t = text(value).size(11).width(Length::Fixed(LABEL_W));
    if enabled {
        t.into()
    } else {
        t.style(muted_style).into()
    }
}

/// `label  field` on one row; a disabled field keeps its place but takes no
/// input and shows nothing, as for a multi-line attribute's width factor.
fn edit_field<'a>(
    name: Cow<'static, str>,
    value: &'a str,
    enabled: bool,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let shown = if enabled { value } else { "" };
    let mut input = text_input("", shown)
        .size(12)
        .padding([4, 8])
        .width(Fill)
        .style(field_style);
    if enabled {
        input = input
            .on_input(on_input)
            .on_submit(Message::AttrEditorApply);
    }
    row![label(name, enabled), input]
        .spacing(6)
        .align_y(iced::Center)
        .into()
}

fn pick_field<'a>(
    name: Cow<'static, str>,
    options: Vec<String>,
    selected: Option<String>,
    enabled: bool,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let options = options
        .into_iter()
        .map(LocalizedChoice::new)
        .collect::<Vec<_>>();
    let selected = selected.map(LocalizedChoice::new);
    let mut pl = crate::ui::dropdown_list::pick_list(selected, options, |value| value.to_string())
        .text_size(12)
        .padding([4, 8])
        .width(Fill);
    if enabled {
        pl = pl.on_select(move |choice: LocalizedChoice| on_select(choice.raw));
    }
    row![label(name, enabled), pl]
        .spacing(6)
        .align_y(iced::Center)
        .into()
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

/// The three tabs as one segmented control.
fn tabs<'a>(active: AttrTab) -> Element<'a, Message> {
    let tab = |name: Cow<'static, str>, this: AttrTab| -> Element<'a, Message> {
        button(text(name).size(11).width(Fill).align_x(iced::Center))
            .on_press(Message::AttrEditorTab(this))
            .style(button_style(this == active))
            .padding([5, 6])
            .width(Fill)
            .into()
    };
    container(
        row![
            tab(t!("Attribute"), AttrTab::Attribute),
            tab(t!("Text Options"), AttrTab::TextOptions),
            tab(t!("Properties"), AttrTab::Properties),
        ]
        .spacing(2),
    )
    .padding(2)
    .width(Fill)
    .style(well_style)
    .into()
}

/// The block and the selected tag, with Select block on the right.
fn header<'a>(view: &AttrEditorView<'a>) -> Element<'a, Message> {
    let tag = view
        .rows
        .get(view.selected)
        .map(|r| r.tag.clone())
        .unwrap_or_default();
    let item = |name: Cow<'static, str>, value: String| {
        column![
            text(name.to_uppercase()).size(10).style(accent_text),
            text(value).size(13),
        ]
        .spacing(2)
    };
    let select = button(
        row![
            crate::ui::icons::themed_success(PICK_BLOCK, 14.0),
            text(t!("Select block")).size(11),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::AttrEditorSelectBlock)
    .style(button_style(false))
    .padding([5, 10]);
    container(
        row![
            item(t!("Block"), view.block.to_string()).width(Fill),
            item(t!("Tag"), tag).width(Fill),
            select,
        ]
        .spacing(12)
        .align_y(iced::Center),
    )
    .padding([10, 12])
    .width(Fill)
    .style(card_style)
    .into()
}

/// Attribute tab: the list and the selected row's value.
fn attribute_tab<'a>(view: &AttrEditorView<'a>) -> Element<'a, Message> {
    let cell = |s: String, w: Length, muted: bool| {
        let t = text(s)
            .size(12)
            .wrapping(iced::advanced::text::Wrapping::None);
        container(if muted { t.style(muted_style) } else { t })
            .width(w)
            .clip(true)
    };
    let head = row![
        cell(t!("Tag").into_owned(), Length::FillPortion(3), true),
        cell(t!("Prompt").into_owned(), Length::FillPortion(4), true),
        cell(t!("Value").into_owned(), Length::FillPortion(5), true),
    ]
    .spacing(6)
    .padding([0, 6]);
    let mut list = column![].spacing(1);
    for (idx, r) in view.rows.iter().enumerate() {
        let is_sel = idx == view.selected;
        let line = row![
            cell(r.tag.clone(), Length::FillPortion(3), false),
            cell(r.prompt.clone(), Length::FillPortion(4), false),
            cell(one_line(&r.value), Length::FillPortion(5), false),
        ]
        .spacing(6);
        list = list.push(
            button(line)
                .on_press(Message::AttrEditorSelect(idx))
                .padding([4, 6])
                .width(Fill)
                .style(move |theme: &Theme, status| {
                    let palette = theme.palette();
                    let pair = if is_sel {
                        palette.primary.strong
                    } else if matches!(status, button::Status::Hovered) {
                        palette.background.strong
                    } else {
                        palette.background.base
                    };
                    button::Style {
                        background: Some(Background::Color(pair.color)),
                        text_color: pair.text,
                        border: Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
        );
    }
    let list = container(scrollable(list).height(Length::Fixed(132.0)))
        .padding(4)
        .width(Fill)
        .style(well_style);

    let row = &view.rows[view.selected.min(view.rows.len() - 1)];
    let shown = if row.multiline {
        one_line(&row.value)
    } else {
        row.value.clone()
    };
    let mut value = text_input("", &shown)
        .size(12)
        .padding([4, 8])
        .width(Fill)
        .style(field_style);
    if !row.multiline {
        let sel = view.selected;
        value = value
            .on_input(move |v| Message::AttrEditorInput { idx: sel, value: v })
            .on_submit(Message::AttrEditorApply);
    }
    let side = |face: Element<'a, Message>, tip: Cow<'static, str>, m: Option<Message>| {
        let b = button(container(face).center_x(Fill))
            .style(button_style(false))
            .padding([4, 0])
            .width(Length::Fixed(30.0))
            .on_press_maybe(m);
        iced::widget::tooltip(
            b,
            text(tip).size(11),
            iced::widget::tooltip::Position::Bottom,
        )
    };
    let value_row = row![
        label(t!("Value"), true),
        value,
        side(
            text("…").size(12).into(),
            t!("Edit the multi-line value"),
            row.multiline.then_some(Message::AttrEditorEditValue),
        ),
        side(
            text("fx").size(12).into(),
            t!("Insert field"),
            Some(Message::AttrEditorInsertField),
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![head, list, value_row].spacing(8).into()
}

fn chip_maybe<'a>(name: String, on: bool, enabled: bool, m: Message) -> Element<'a, Message> {
    if enabled {
        chip(name, on, m)
    } else {
        button(text(name).size(11).style(muted_style))
            .style(button_style(false))
            .padding([4, 10])
            .into()
    }
}

/// Text Options tab: the selected attribute's text formatting. A
/// multi-line attribute has the nine box justifications and a boundary
/// width instead of the single-line flags.
fn text_options_tab<'a>(view: &AttrEditorView<'a>) -> Element<'a, Message> {
    let r = &view.rows[view.selected.min(view.rows.len() - 1)];
    let single = !r.multiline;
    let style_sel = (!r.text_style.is_empty()).then(|| r.text_style.clone());
    let justify: Vec<String> = justify_options(r.multiline)
        .iter()
        .map(|(l, _, _)| (*l).to_string())
        .collect();
    let justify_sel = Some(justify_label(r.h_align, r.v_align).to_string());
    let annotative = row![
        label(Cow::Borrowed(""), true),
        crate::ui::icons::themed(ICON_ANNOTATIVE, 14.0),
        text(if r.annotative {
            t!("Annotative")
        } else {
            t!("Not annotative")
        })
        .size(11)
        .style(muted_style),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let flags = row![
        label(Cow::Borrowed(""), single),
        chip_maybe(
            t!("Backwards").into_owned(),
            single && r.backwards,
            single,
            Message::AttrEditorBackwards(!r.backwards),
        ),
        chip_maybe(
            t!("Upside down").into_owned(),
            single && r.upside_down,
            single,
            Message::AttrEditorUpsideDown(!r.upside_down),
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let left = column![
        pick_field(
            t!("Justification"),
            justify,
            justify_sel,
            true,
            Message::AttrEditorJustify
        ),
        edit_field(t!("Height"), &r.height, true, Message::AttrEditorHeight),
        edit_field(
            t!("Rotation"),
            &r.rotation,
            true,
            Message::AttrEditorRotation
        ),
        annotative,
    ]
    .spacing(8)
    .width(Fill);
    let right = column![
        flags,
        edit_field(
            t!("Width Factor"),
            &r.width_factor,
            single,
            Message::AttrEditorWidth
        ),
        edit_field(
            t!("Oblique Angle"),
            &r.oblique,
            single,
            Message::AttrEditorOblique
        ),
        edit_field(
            t!("Boundary width"),
            &r.boundary_width,
            r.multiline,
            Message::AttrEditorBoundary
        ),
    ]
    .spacing(8)
    .width(Fill);
    column![
        pick_field(
            t!("Text Style"),
            view.styles.clone(),
            style_sel,
            true,
            Message::AttrEditorTextStyle
        ),
        row![left, right].spacing(14),
    ]
    .spacing(8)
    .into()
}

/// Properties tab: layer, linetype, colour, lineweight and the plot style.
fn properties_tab<'a>(view: &AttrEditorView<'a>) -> Element<'a, Message> {
    let r = &view.rows[view.selected.min(view.rows.len() - 1)];
    let lt_sel = Some(if r.linetype.is_empty() {
        "ByLayer".to_string()
    } else {
        r.linetype.clone()
    });
    let mut colors: Vec<String> = COLOR_OPTIONS.iter().map(|s| s.to_string()).collect();
    let current = color_label(r.color);
    if !colors.iter().any(|o| o == &current) {
        colors.insert(0, current.clone());
    }
    colors.push(SELECT_COLOR.to_string());
    let lw = crate::ui::dropdown_list::pick_list(
        Some(LwItem(r.line_weight)),
        lw_options()
            .into_iter()
            .filter(|it| it.0 != LineWeight::Default)
            .collect::<Vec<_>>(),
        |value| value.to_string(),
    )
    .on_select(|it: LwItem| Message::AttrEditorLineweight(it.0))
    .text_size(12)
    .padding([4, 8])
    .width(Fill);
    column![
        pick_field(
            t!("Layer"),
            view.layers.clone(),
            Some(r.layer.clone()),
            true,
            Message::AttrEditorLayer
        ),
        pick_field(
            t!("Linetype"),
            view.linetypes.clone(),
            lt_sel,
            true,
            Message::AttrEditorLinetype
        ),
        row![
            pick_field(t!("Color"), colors, Some(current), true, Message::AttrEditorColor),
            row![label(t!("Lineweight"), true), lw]
                .spacing(6)
                .align_y(iced::Center),
        ]
        .spacing(14),
        pick_field(
            t!("Plot style"),
            vec![view.plot_style.clone()],
            Some(view.plot_style.clone()),
            false,
            |_| Message::AttrEditorApply,
        ),
    ]
    .spacing(8)
    .into()
}

/// The validation message band, as in the attribute definition dialog.
fn error_band<'a>(error: &str) -> Element<'a, Message> {
    container(
        row![
            crate::ui::icons::semantic(ICON_WARN, 14.0),
            text(error.to_string()).size(11),
            Space::new().width(Fill),
            button(crate::ui::icons::themed(crate::ui::icons::CLOSE, 10.0))
                .on_press(Message::AttrEditorDismissError)
                .style(button_style(false))
                .padding([1, 6]),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding([6, 10])
    .width(Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().danger.base.color.scale_alpha(0.18),
        )),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// The question before another block is picked while changes are pending.
fn question_card<'a>() -> Element<'a, Message> {
    let answer = |name: Cow<'static, str>, a: AttrAnswer, primary: bool| {
        dialog_button(name, Message::AttrEditorAnswer(a), primary)
    };
    container(
        column![
            row![
                crate::ui::icons::semantic(ICON_WARN, 14.0),
                text(t!(
                    "The attributes of this block have changed. Keep the changes before picking another block?"
                ))
                .size(11),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                Space::new().width(Fill),
                answer(t!("Keep changes"), AttrAnswer::Save, true),
                answer(t!("Discard changes"), AttrAnswer::Discard, false),
                answer(t!("Stay"), AttrAnswer::Stay, false),
            ]
            .spacing(6),
        ]
        .spacing(8),
    )
    .padding([8, 10])
    .width(Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().primary.base.color.scale_alpha(0.14),
        )),
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// Build the window.
pub fn view_window<'a>(
    view: AttrEditorView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let body: Element<'a, Message> = if view.rows.is_empty() {
        text(t!("This block has no attributes."))
            .size(12)
            .style(muted_style)
            .into()
    } else {
        match view.tab {
            AttrTab::Attribute => attribute_tab(&view),
            AttrTab::TextOptions => text_options_tab(&view),
            AttrTab::Properties => properties_tab(&view),
        }
    };
    let section = match view.tab {
        AttrTab::Attribute => t!("Attribute"),
        AttrTab::TextOptions => t!("Text Options"),
        AttrTab::Properties => t!("Properties"),
    };
    // While the question is open it takes the tab's place, so the window
    // keeps its size.
    let main: Element<'a, Message> = if view.asking {
        question_card()
    } else {
        card(section.into_owned(), body)
    };
    let mut content = column![header(&view), tabs(view.tab), main].spacing(10);
    if let Some(error) = view.error {
        content = content.push(error_band(error));
    }
    let footer = row![
        button(text("?").size(12))
            .on_press(Message::AttrEditorHelp)
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button_styled_opt(
            t!("Apply"),
            view.pending.then_some(Message::AttrEditorApply),
            iced::widget::button::secondary,
        ),
        dialog_button(t!("Cancel"), Message::AttrEditorCancel, false),
        dialog_button(t!("OK"), Message::AttrEditorOk, true),
    ]
    .spacing(6)
    .align_y(iced::Center);
    content
        .push(footer)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}
