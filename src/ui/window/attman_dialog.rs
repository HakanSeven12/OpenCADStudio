//! Block Attribute Manager (BATTMAN / ATTMAN): the attribute definitions of
//! one attributed block as a table, with their order, editing, removal and
//! the update of the existing references.
//!
//! Blocks are chips across the top; the columns the table shows are chips
//! under it; the row actions sit in a column beside it; the selected row is
//! edited in a card below (Attribute / Text / Properties) and the settings
//! are switches in the footer.

use document::entities::AttributeDefinition;
use document::Handle;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, toggler, Space,
};
use iced::{Background, Border, Color, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::modules::draw::draw::attdef::Justify;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};
use crate::ui::window::pdf_dialogs::{accent_text, card_style, well_style};

static ICON_UP: &[u8] = include_bytes!("../../../assets/icons/ui/up.svg");
static ICON_PICK: &[u8] = include_bytes!("../../../assets/icons/blocks/select_objects.svg");

/// The columns the table can show besides Tag, in the order of their chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Prompt,
    Default,
    Modes,
    Annotative,
    Style,
    Height,
    Layer,
    Justify,
    Rotation,
    WidthFactor,
    Oblique,
    BoundaryWidth,
    Linetype,
    Color,
    Lineweight,
}

impl Column {
    pub const ALL: [Column; 15] = [
        Column::Prompt,
        Column::Default,
        Column::Modes,
        Column::Annotative,
        Column::Style,
        Column::Height,
        Column::Layer,
        Column::Justify,
        Column::Rotation,
        Column::WidthFactor,
        Column::Oblique,
        Column::BoundaryWidth,
        Column::Linetype,
        Column::Color,
        Column::Lineweight,
    ];
    /// The chips shown before "+N".
    const FIRST: usize = 7;

    pub fn label(self) -> &'static str {
        match self {
            Column::Prompt => "Prompt",
            Column::Default => "Default",
            Column::Modes => "Modes",
            Column::Annotative => "Annotative",
            Column::Style => "Style",
            Column::Height => "Height",
            Column::Layer => "Layer",
            Column::Justify => "Justification",
            Column::Rotation => "Rotation",
            Column::WidthFactor => "Width factor",
            Column::Oblique => "Oblique angle",
            Column::BoundaryWidth => "Boundary width",
            Column::Linetype => "Linetype",
            Column::Color => "Color",
            Column::Lineweight => "Lineweight",
        }
    }

    pub fn from_name(name: &str) -> Option<Column> {
        let key: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        Column::ALL.into_iter().find(|c| {
            c.label()
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .eq_ignore_ascii_case(&key)
        })
    }

    fn bit(self) -> u32 {
        1 << (self as u32)
    }

    fn cell(self, def: &AttributeDefinition) -> String {
        let num = |v: f64| crate::ui::window::block_palette::short(v);
        match self {
            Column::Prompt => def.prompt.clone(),
            Column::Default => def.default_value.clone(),
            Column::Modes => modes(def),
            Column::Annotative => yes_no(def.flags.annotative),
            Column::Style => style_name(def),
            Column::Height => num(def.height),
            Column::Layer => def.common.layer.clone(),
            Column::Justify => t!(justify(def).label()).into_owned(),
            Column::Rotation => num(def.rotation.to_degrees()),
            Column::WidthFactor => num(def.width_factor),
            Column::Oblique => num(def.oblique_angle.to_degrees()),
            Column::BoundaryWidth => def
                .embedded_mtext
                .as_ref()
                .map_or(String::new(), |m| num(m.rectangle_width)),
            Column::Linetype => crate::ui::properties::linetype_display_name(&def.common.linetype),
            Column::Color => color_label(def.common.color),
            Column::Lineweight => lineweight_label(def.common.line_weight),
        }
    }
}

/// What the dialog remembers between openings.
#[derive(Debug, Clone, Copy)]
pub struct AttManPrefs {
    /// Bit set of [`Column`]s shown.
    pub columns: u32,
    /// Apply and Sync rebuild the attributes of the existing references.
    pub apply_existing: bool,
    /// Rows sharing a tag are drawn in the danger colour.
    pub emphasize_duplicates: bool,
    /// Every edit is written to the drawing at once.
    pub live: bool,
}

static PREFS: std::sync::Mutex<AttManPrefs> = std::sync::Mutex::new(AttManPrefs {
    columns: (1 << Column::Prompt as u32)
        | (1 << Column::Default as u32)
        | (1 << Column::Modes as u32)
        | (1 << Column::Annotative as u32),
    apply_existing: true,
    emphasize_duplicates: true,
    live: true,
});

pub fn prefs() -> std::sync::MutexGuard<'static, AttManPrefs> {
    PREFS.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditTab {
    #[default]
    Attribute,
    Text,
    Properties,
}

/// One attributed block the dialog can show.
#[derive(Debug, Clone)]
pub struct AttManBlock {
    pub name: String,
    /// References anywhere in the drawing.
    pub found: usize,
    /// References in the current space.
    pub in_space: usize,
}

/// One attribute definition of the shown block.
#[derive(Debug, Clone)]
pub struct AttManRow {
    /// The definition object in the block.
    pub handle: Handle,
    /// The tag the definition had when the block was loaded or applied.
    pub original_tag: String,
    /// The tag the references hold now (changed by live writes).
    pub live_tag: String,
    /// The working copy.
    pub def: AttributeDefinition,
}

/// The edit card's typed fields, kept as text while they are typed.
#[derive(Debug, Clone, Default)]
pub struct Drafts {
    pub tag: String,
    pub height: String,
    pub rotation: String,
    pub width: String,
    pub oblique: String,
    pub boundary: String,
}

pub struct AttManState {
    pub blocks: Vec<AttManBlock>,
    pub block: usize,
    pub rows: Vec<AttManRow>,
    /// Definitions removed from the shown block since it was loaded.
    pub removed: Vec<Handle>,
    pub selected: Option<usize>,
    pub tab: EditTab,
    /// The edit card is shown.
    pub edit_open: bool,
    pub drafts: Drafts,
    /// The working copy differs from the drawing.
    pub dirty: bool,
    /// Undo steps the live changes since the last Apply took (one unless
    /// something closed the open step).
    pub live_steps: usize,
    /// The column chips past the first ones are shown.
    pub more_columns: bool,
    pub styles: Vec<String>,
    pub layers: Vec<String>,
    pub linetypes: Vec<String>,
    pub error: Option<String>,
}

impl AttManState {
    pub fn current_block(&self) -> Option<&AttManBlock> {
        self.blocks.get(self.block)
    }

    pub fn selected_row(&self) -> Option<&AttManRow> {
        self.selected.and_then(|i| self.rows.get(i))
    }

    /// Load the drafts from the selected row.
    pub fn load_drafts(&mut self) {
        let num = |v: f64| crate::ui::window::block_palette::short(v);
        self.drafts = match self.selected_row() {
            Some(row) => {
                let d = &row.def;
                Drafts {
                    tag: d.tag.clone(),
                    height: num(d.height),
                    rotation: num(d.rotation.to_degrees()),
                    width: num(d.width_factor),
                    oblique: num(d.oblique_angle.to_degrees()),
                    boundary: d
                        .embedded_mtext
                        .as_ref()
                        .map_or("0".into(), |m| num(m.rectangle_width)),
                }
            }
            None => Drafts::default(),
        };
    }

    /// Tags held by more than one row, upper-cased.
    fn duplicate_tags(&self) -> Vec<String> {
        let mut seen = std::collections::HashMap::<String, usize>::new();
        for row in &self.rows {
            *seen.entry(row.def.tag.to_ascii_uppercase()).or_default() += 1;
        }
        seen.into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(tag, _)| tag)
            .collect()
    }
}

/// One edit in the dialog.
#[derive(Debug, Clone)]
pub enum AttManMsg {
    Block(usize),
    PickBlock,
    Select(usize),
    MoveUp,
    MoveDown,
    ToggleEdit,
    Remove,
    Tab(EditTab),
    Column(Column, bool),
    MoreColumns,
    Tag(String),
    Prompt(String),
    Default(String),
    /// "invisible", "verify", "preset" or "multiline".
    Mode(&'static str, bool),
    Style(String),
    Justify(Justify),
    Height(String),
    Rotation(String),
    Width(String),
    Oblique(String),
    Boundary(String),
    Annotative(bool),
    Backwards(bool),
    UpsideDown(bool),
    Layer(String),
    Linetype(String),
    Color(String),
    Lineweight(String),
    ApplyExisting(bool),
    Emphasize(bool),
    Live(bool),
    Sync,
    Apply,
    Ok,
    Cancel,
    Help,
    DismissError,
}

fn msg(m: AttManMsg) -> Message {
    Message::AttMan(m)
}

// ── Cell text ─────────────────────────────────────────────────────────────

fn yes_no(on: bool) -> String {
    if on { t!("Yes") } else { t!("No") }.into_owned()
}

/// Mode letters: I invisible, C constant, V verify, P preset, L locked
/// position, M multiple lines.
pub fn modes(def: &AttributeDefinition) -> String {
    let f = &def.flags;
    [
        (f.invisible, 'I'),
        (f.constant, 'C'),
        (f.verify, 'V'),
        (f.preset, 'P'),
        (f.locked_position || def.lock_position, 'L'),
        (def.is_multiline, 'M'),
    ]
    .into_iter()
    .filter_map(|(on, letter)| on.then_some(letter))
    .collect()
}

pub fn justify(def: &AttributeDefinition) -> Justify {
    Justify::from_alignment(def.horizontal_alignment, def.vertical_alignment)
}

fn style_name(def: &AttributeDefinition) -> String {
    if def.text_style.trim().is_empty() {
        "Standard".into()
    } else {
        def.text_style.clone()
    }
}

/// The colours the Properties tab offers.
pub const COLORS: [&str; 9] = [
    "ByLayer", "ByBlock", "Red", "Yellow", "Green", "Cyan", "Blue", "Magenta", "White",
];

pub fn color_label(color: document::types::Color) -> String {
    use document::types::Color as C;
    match color {
        C::ByLayer => "ByLayer".into(),
        C::ByBlock => "ByBlock".into(),
        C::Index(n @ 1..=7) => COLORS[n as usize + 1].into(),
        C::Index(n) => format!("{n}"),
        C::Rgb { r, g, b } => format!("{r},{g},{b}"),
        C::None => "None".into(),
    }
}

pub fn parse_color(label: &str) -> Option<document::types::Color> {
    use document::types::Color as C;
    match COLORS.iter().position(|c| c.eq_ignore_ascii_case(label.trim())) {
        Some(0) => Some(C::ByLayer),
        Some(1) => Some(C::ByBlock),
        Some(i) => Some(C::Index(i as u8 - 1)),
        None => label
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|n| (1..=255).contains(n))
            .map(C::Index),
    }
}

/// The lineweights the Properties tab offers, in 1/100 mm.
const LINEWEIGHTS: [i16; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
    200, 211,
];

pub fn lineweight_label(lw: document::types::LineWeight) -> String {
    use document::types::LineWeight as L;
    match lw {
        L::ByLayer => "ByLayer".into(),
        L::ByBlock => "ByBlock".into(),
        L::Default => "Default".into(),
        L::Value(v) => format!("{:.2} mm", f64::from(v) / 100.0),
    }
}

pub fn parse_lineweight(label: &str) -> Option<document::types::LineWeight> {
    use document::types::LineWeight as L;
    let s = label.trim();
    match s.to_ascii_lowercase().as_str() {
        "bylayer" => Some(L::ByLayer),
        "byblock" => Some(L::ByBlock),
        "default" => Some(L::Default),
        _ => {
            let mm = s.trim_end_matches("mm").trim().parse::<f64>().ok()?;
            let v = (mm * 100.0).round() as i16;
            LINEWEIGHTS.contains(&v).then_some(L::Value(v))
        }
    }
}

fn lineweight_options() -> Vec<String> {
    ["ByLayer", "ByBlock", "Default"]
        .into_iter()
        .map(str::to_string)
        .chain(
            LINEWEIGHTS
                .iter()
                .map(|v| lineweight_label(document::types::LineWeight::Value(*v))),
        )
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JustifyItem(Justify);

impl std::fmt::Display for JustifyItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(self.0.label()))
    }
}

// ── View ──────────────────────────────────────────────────────────────────

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding([10, 12])
        .width(Fill)
        .style(card_style)
        .into()
}

/// A pill: framed in the accent colour while on.
fn pill<'a>(label: String, on: bool, message: Option<Message>) -> Element<'a, Message> {
    let b = button(text(label).size(11))
        .padding([4, 11])
        .style(move |theme: &Theme, status| {
            let palette = theme.palette();
            let mut style = button_style(false)(theme, status);
            style.border = Border {
                width: 1.0,
                radius: 12.0.into(),
                color: if on {
                    palette.primary.base.color
                } else {
                    palette.background.strong.color
                },
            };
            if on {
                style.background = Some(Background::Color(
                    palette.primary.base.color.scale_alpha(0.16),
                ));
            }
            style
        });
    match message {
        Some(m) => b.on_press(m).into(),
        None => b.into(),
    }
}

fn icon_button<'a>(
    icon: &'static [u8],
    rotation: f32,
    message: Option<Message>,
    danger: bool,
) -> Element<'a, Message> {
    let enabled = message.is_some();
    let svg = iced::widget::svg(crate::ui::icons::themed_handle(icon))
        .width(14)
        .height(14)
        .rotation(iced::Radians(rotation))
        .style(move |theme: &Theme, _| iced::widget::svg::Style {
            color: Some(if danger && enabled {
                theme.palette().danger.base.color
            } else if enabled {
                theme.palette().background.base.text
            } else {
                theme.palette().background.base.text.scale_alpha(0.35)
            }),
        });
    let b = button(container(svg).center_x(Fill).center_y(Fill))
        .width(30)
        .height(28)
        .padding(0)
        .style(button_style(false));
    match message {
        Some(m) => b.on_press(m).into(),
        None => b.into(),
    }
}

fn labeled_switch<'a>(label: String, on: bool, f: fn(bool) -> AttManMsg) -> Element<'a, Message> {
    row![
        toggler(on).on_toggle(move |v| msg(f(v))).size(16.0),
        text(label).size(11),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn field_row<'a>(
    label: String,
    value: &'a str,
    on_input: Option<fn(String) -> AttManMsg>,
) -> Element<'a, Message> {
    let input: Element<'a, Message> = match on_input {
        Some(f) => text_input("", value)
            .on_input(move |v| msg(f(v)))
            .style(field_style)
            .size(11)
            .padding([4, 8])
            .width(Fill)
            .into(),
        None => crate::ui::read_only::field(value, 11.0, Fill),
    };
    row![
        text(label)
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(96.0)),
        input
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn choice_row<'a>(
    label: String,
    selected: String,
    options: Vec<String>,
    on: Option<fn(String) -> AttManMsg>,
) -> Element<'a, Message> {
    let list: Element<'a, Message> = match on {
        Some(f) => pick_list(Some(selected), options, |s: &String| s.clone())
            .on_select(move |v| msg(f(v)))
            .text_size(11)
            .padding([3, 8])
            .width(Fill)
            .into(),
        None => crate::ui::read_only::field(&selected, 11.0, Fill),
    };
    row![
        text(label)
            .size(11)
            .style(muted_style)
            .width(Length::Fixed(96.0)),
        list
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn segmented<'a>(current: EditTab) -> Element<'a, Message> {
    let tabs = [
        (EditTab::Attribute, t!("Attribute").into_owned()),
        (EditTab::Text, t!("Text").into_owned()),
        (EditTab::Properties, t!("Properties").into_owned()),
    ];
    let buttons = tabs.into_iter().map(|(tab, label)| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(AttManMsg::Tab(tab)))
            .style(button_style(tab == current))
            .padding([5, 6])
            .width(Fill)
            .into()
    });
    container(iced::widget::Row::with_children(buttons.collect::<Vec<_>>()).spacing(2))
        .padding(2)
        .width(Length::Fixed(330.0))
        .style(well_style)
        .into()
}

fn table<'a>(state: &'a AttManState, columns: &[Column], emphasize: bool) -> Element<'a, Message> {
    let duplicates = if emphasize {
        state.duplicate_tags()
    } else {
        Vec::new()
    };
    let header = {
        let mut r = row![text(t!("Tag")).size(11).style(muted_style).width(Fill)];
        for c in columns {
            r = r.push(text(t!(c.label())).size(11).style(muted_style).width(Fill));
        }
        container(r.spacing(8)).padding([4, 8])
    };
    let rows = state.rows.iter().enumerate().map(|(index, entry)| {
        let duplicate = duplicates.contains(&entry.def.tag.to_ascii_uppercase());
        let chosen = state.selected == Some(index);
        let tint = move |theme: &Theme| text::Style {
            color: duplicate.then(|| theme.palette().danger.base.color),
        };
        let mut r = row![text(entry.def.tag.clone()).size(11).style(tint).width(Fill)];
        for c in columns {
            r = r.push(text(c.cell(&entry.def)).size(11).style(tint).width(Fill));
        }
        button(r.spacing(8))
            .on_press(msg(AttManMsg::Select(index)))
            .padding([5, 8])
            .width(Fill)
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let mut style = button::text(theme, status);
                style.text_color = palette.background.base.text;
                style.background = if chosen {
                    Some(Background::Color(palette.primary.base.color.scale_alpha(0.18)))
                } else if matches!(status, button::Status::Hovered) {
                    Some(Background::Color(palette.background.strong.color))
                } else {
                    None
                };
                style
            })
            .into()
    });
    column![
        header,
        container(Space::new().height(1))
            .width(Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.strong.color)),
                ..Default::default()
            }),
        scrollable(column(rows.collect::<Vec<Element<'a, Message>>>()).spacing(1))
            .height(Length::Fixed(150.0)),
    ]
    .spacing(2)
    .width(Fill)
    .into()
}

fn edit_card<'a>(state: &'a AttManState, row_: &'a AttManRow) -> Element<'a, Message> {
    let def = &row_.def;
    let title = text(format!("{} — {}", def.tag, t!("Edit")).to_uppercase())
        .size(10)
        .style(accent_text);
    let body: Element<'a, Message> = match state.tab {
        EditTab::Attribute => {
            let constant = def.flags.constant;
            let mode = |key: &'static str, label: &str, on: bool| {
                pill(
                    t!(label).into_owned(),
                    on,
                    Some(msg(AttManMsg::Mode(key, !on))),
                )
            };
            let fields = column![
                field_row(t!("Tag").into_owned(), &state.drafts.tag, Some(AttManMsg::Tag)),
                field_row(
                    t!("Prompt").into_owned(),
                    &def.prompt,
                    (!constant).then_some(AttManMsg::Prompt as fn(String) -> AttManMsg),
                ),
                field_row(
                    t!("Default").into_owned(),
                    &def.default_value,
                    Some(AttManMsg::Default),
                ),
            ]
            .spacing(6)
            .width(Fill);
            let modes = column![
                row![
                    mode("invisible", "Invisible", def.flags.invisible),
                    mode("verify", "Verify", def.flags.verify),
                ]
                .spacing(6),
                row![
                    mode("preset", "Preset", def.flags.preset),
                    mode("multiline", "Multiple lines", def.is_multiline),
                ]
                .spacing(6),
                text(t!("The Constant mode cannot be changed in the definition."))
                    .size(10)
                    .style(muted_style),
            ]
            .spacing(6)
            .width(Length::Fixed(230.0));
            row![fields, modes].spacing(18).into()
        }
        EditTab::Text => {
            let justify_list: Element<'a, Message> = if def.is_multiline {
                crate::ui::read_only::field(&t!(justify(def).label()), 11.0, Fill)
            } else {
                pick_list(
                    Some(JustifyItem(justify(def))),
                    Justify::ALL.map(JustifyItem).to_vec(),
                    |j: &JustifyItem| j.to_string(),
                )
                .on_select(|j: JustifyItem| msg(AttManMsg::Justify(j.0)))
                .text_size(11)
                .padding([3, 8])
                .width(Fill)
                .into()
            };
            let left = column![
                choice_row(
                    t!("Text Style").into_owned(),
                    style_name(def),
                    state.styles.clone(),
                    Some(AttManMsg::Style),
                ),
                row![
                    text(t!("Justification"))
                        .size(11)
                        .style(muted_style)
                        .width(Length::Fixed(96.0)),
                    justify_list
                ]
                .spacing(8)
                .align_y(iced::Center),
                field_row(
                    t!("Height").into_owned(),
                    &state.drafts.height,
                    Some(AttManMsg::Height)
                ),
                field_row(
                    t!("Rotation").into_owned(),
                    &state.drafts.rotation,
                    Some(AttManMsg::Rotation)
                ),
            ]
            .spacing(6)
            .width(Fill);
            let right = column![
                field_row(
                    t!("Width factor").into_owned(),
                    &state.drafts.width,
                    Some(AttManMsg::Width)
                ),
                field_row(
                    t!("Oblique angle").into_owned(),
                    &state.drafts.oblique,
                    Some(AttManMsg::Oblique)
                ),
                field_row(
                    t!("Boundary width").into_owned(),
                    &state.drafts.boundary,
                    def.is_multiline
                        .then_some(AttManMsg::Boundary as fn(String) -> AttManMsg),
                ),
                row![
                    pill(
                        t!("Annotative").into_owned(),
                        def.flags.annotative,
                        Some(msg(AttManMsg::Annotative(!def.flags.annotative))),
                    ),
                    pill(
                        t!("Backwards").into_owned(),
                        def.text_generation_flags & 0x2 != 0,
                        Some(msg(AttManMsg::Backwards(
                            def.text_generation_flags & 0x2 == 0
                        ))),
                    ),
                    pill(
                        t!("Upside down").into_owned(),
                        def.text_generation_flags & 0x4 != 0,
                        Some(msg(AttManMsg::UpsideDown(
                            def.text_generation_flags & 0x4 == 0
                        ))),
                    ),
                ]
                .spacing(6),
            ]
            .spacing(6)
            .width(Fill);
            row![left, right].spacing(18).into()
        }
        EditTab::Properties => {
            let mut linetypes = vec!["ByLayer".to_string(), "ByBlock".to_string()];
            linetypes.extend(state.linetypes.iter().cloned());
            let linetype = if def.common.linetype.is_empty() {
                "ByLayer".to_string()
            } else {
                def.common.linetype.clone()
            };
            row![
                column![
                    choice_row(
                        t!("Layer").into_owned(),
                        def.common.layer.clone(),
                        state.layers.clone(),
                        Some(AttManMsg::Layer),
                    ),
                    choice_row(
                        t!("Linetype").into_owned(),
                        linetype,
                        linetypes,
                        Some(AttManMsg::Linetype),
                    ),
                ]
                .spacing(6)
                .width(Fill),
                column![
                    choice_row(
                        t!("Color").into_owned(),
                        color_label(def.common.color),
                        COLORS.iter().map(|c| c.to_string()).collect(),
                        Some(AttManMsg::Color),
                    ),
                    choice_row(
                        t!("Lineweight").into_owned(),
                        lineweight_label(def.common.line_weight),
                        lineweight_options(),
                        Some(AttManMsg::Lineweight),
                    ),
                ]
                .spacing(6)
                .width(Fill),
            ]
            .spacing(18)
            .into()
        }
    };
    let live = prefs().live;
    panel(
        column![
            title,
            segmented(state.tab),
            body,
            labeled_switch(
                t!("Show changes in the drawing as they are made").into_owned(),
                live,
                AttManMsg::Live
            ),
        ]
        .spacing(10),
    )
}

pub fn view<'a>(
    state: &'a AttManState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let prefs = *prefs();
    // ── Blocks ──
    let mut chips = row![icon_button(
        ICON_PICK,
        0.0,
        Some(msg(AttManMsg::PickBlock)),
        false
    )]
    .spacing(6)
    .align_y(iced::Center);
    for (index, block) in state.blocks.iter().enumerate() {
        let on = index == state.block;
        let label = if on {
            format!(
                "{} · {}",
                block.name,
                crate::tf!("{} references", block.found)
            )
        } else {
            block.name.clone()
        };
        chips = chips.push(pill(label, on, Some(msg(AttManMsg::Block(index)))));
    }
    let counts = state.current_block().map_or(String::new(), |b| {
        crate::tf!(
            "In drawing: {} · in model space: {}",
            b.found,
            b.in_space
        )
        .into_owned()
    });
    let blocks_card = panel(
        row![
            scrollable(chips).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(3).scroller_width(3)
            )),
            Space::new().width(Fill),
            text(counts).size(10).style(muted_style),
        ]
        .spacing(10)
        .align_y(iced::Center),
    );

    // ── Table ──
    let columns: Vec<Column> = Column::ALL
        .into_iter()
        .filter(|c| prefs.columns & c.bit() != 0)
        .collect();
    let mut column_chips = row![text(t!("Columns:")).size(11).style(muted_style)]
        .spacing(6)
        .align_y(iced::Center);
    let shown = if state.more_columns {
        Column::ALL.len()
    } else {
        Column::FIRST
    };
    for c in &Column::ALL[..shown] {
        let on = prefs.columns & c.bit() != 0;
        column_chips = column_chips.push(pill(
            t!(c.label()).into_owned(),
            on,
            Some(msg(AttManMsg::Column(*c, !on))),
        ));
    }
    column_chips = column_chips.push(pill(
        if state.more_columns {
            "−".to_string()
        } else {
            format!("+{}", Column::ALL.len() - Column::FIRST)
        },
        false,
        Some(msg(AttManMsg::MoreColumns)),
    ));
    let count = state.rows.len();
    let at = state.selected;
    let actions = column![
        icon_button(
            ICON_UP,
            0.0,
            at.filter(|i| *i > 0).map(|_| msg(AttManMsg::MoveUp)),
            false
        ),
        icon_button(
            ICON_UP,
            std::f32::consts::PI,
            at.filter(|i| i + 1 < count).map(|_| msg(AttManMsg::MoveDown)),
            false
        ),
        icon_button(
            crate::ui::icons::MORE,
            0.0,
            at.map(|_| msg(AttManMsg::ToggleEdit)),
            false
        ),
        // The last attribute of a block cannot be removed.
        icon_button(
            crate::ui::icons::CLOSE,
            0.0,
            at.filter(|_| count > 1).map(|_| msg(AttManMsg::Remove)),
            true
        ),
    ]
    .spacing(6);
    let table_card = panel(
        row![
            column![
                table(state, &columns, prefs.emphasize_duplicates),
                scrollable(column_chips).direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(3).scroller_width(3)
                )),
            ]
            .spacing(10)
            .width(Fill),
            actions,
        ]
        .spacing(10),
    );

    let mut content = column![blocks_card, table_card].spacing(10);
    if state.edit_open {
        if let Some(selected) = state.selected_row() {
            content = content.push(edit_card(state, selected));
        }
    }
    if let Some(error) = &state.error {
        content = content.push(
            container(
                row![
                    crate::ui::icons::semantic(
                        include_bytes!("../../../assets/icons/ui/warning_triangle.svg"),
                        15.0
                    ),
                    text(error.clone()).size(11).width(Fill),
                    button(crate::ui::icons::semantic(crate::ui::icons::CLOSE, 12.0))
                        .on_press(msg(AttManMsg::DismissError))
                        .style(button_style(false))
                        .padding([1, 6]),
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .padding([6, 10])
            .width(Fill)
            .style(|theme: &Theme| {
                let danger = theme.palette().danger.base.color;
                container::Style {
                    background: Some(Background::Color(danger.scale_alpha(0.16))),
                    border: Border {
                        width: 1.0,
                        radius: 6.0.into(),
                        color: danger,
                    },
                    ..Default::default()
                }
            }),
        );
    }
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(AttManMsg::Help))
            .style(button_style(false))
            .padding([3, 10]),
        labeled_switch(
            t!("Apply changes to existing references").into_owned(),
            prefs.apply_existing,
            AttManMsg::ApplyExisting
        ),
        labeled_switch(
            t!("Emphasize duplicate tags").into_owned(),
            prefs.emphasize_duplicates,
            AttManMsg::Emphasize
        ),
        Space::new().width(Fill),
        dialog_button(t!("Sync"), msg(AttManMsg::Sync), false),
        dialog_button(t!("Apply"), msg(AttManMsg::Apply), false),
        dialog_button(t!("OK"), msg(AttManMsg::Ok), true),
    ]
    .spacing(12)
    .align_y(iced::Center);
    container(content.push(footer))
        .padding([10, 14])
        .width(sizing.width)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            ..Default::default()
        })
        .into()
}
