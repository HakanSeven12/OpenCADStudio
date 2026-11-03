//! Picked table cell windows: Cell Borders (line properties and the edges of
//! the picked range they go on), Cell Contents (the contents of a cell,
//! their order and how they are laid out), Cell Block (a block as the
//! content of the picked cells) and Cell Data Format (how their values
//! read).

use iced::widget::{button, column, container, pick_list, row, text, text_input, Space};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, chip};

/// Which edges of the picked range the border settings go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderSet {
    All,
    Outside,
    Inside,
    None,
    Top,
    Bottom,
    Left,
    Right,
    InsideHorizontal,
    InsideVertical,
}

#[derive(Debug, Clone)]
pub enum TableCellDialogMsg {
    Lineweight(String),
    Linetype(String),
    Color(String),
    Double(bool),
    Spacing(String),
    Edges(BorderSet),
    Pick(usize),
    Up,
    Down,
    Delete,
    Layout(u32),
    ContentSpacing(String),
    BlockName(String),
    BlockBrowse,
    BlockPath(String),
    Align(i32),
    BlockScale(String),
    AutoFit(bool),
    BlockRotation(String),
    FormatKind(usize),
    FormatUnit(usize),
    Precision(String),
    Symbol(String),
    Append(bool),
    Negative(bool),
    Separator(usize),
    Axis(usize),
    DateFormat(String),
    Conversion(String),
    Prefix(String),
    Suffix(String),
    DecimalSeparator(usize),
    ThousandsSeparator(usize),
    Zero(u32),
    Help,
    Ok,
    /// OK in the invalid data type warning.
    WarnOk,
    /// The typed value that is not of the cell's data type: the cell's
    /// type changes to the value's.
    EntryType,
    /// The typed value goes back into the cell's editor.
    EntryValue,
}

fn msg(m: TableCellDialogMsg) -> Message {
    Message::TableCellDialog(m)
}

pub struct CellBordersState {
    pub lineweights: Vec<String>,
    pub lineweight: String,
    pub linetypes: Vec<String>,
    pub linetype: String,
    pub colors: Vec<String>,
    pub color: String,
    pub double: bool,
    pub spacing: String,
    /// The chosen edge sets (the last one alone when it is All or None).
    pub sets: Vec<BorderSet>,
    /// Picked rows × columns, for the sample grid.
    pub rows: usize,
    pub columns: usize,
}

pub struct CellContentsState {
    /// Each content as listed: "Text: …" / "Block: …".
    pub items: Vec<String>,
    /// The cell's content indices, in their new order.
    pub order: Vec<usize>,
    pub selected: Option<usize>,
    /// Content layout bit: 1 flow, 2 stacked horizontally, 4 vertically.
    pub layout: u32,
    pub spacing: String,
}

/// Whether an edge of the sample grid gets the settings.
fn edge_chosen(sets: &[BorderSet], outside: bool, horizontal: bool, side: BorderSet) -> bool {
    sets.iter().any(|set| match set {
        BorderSet::All => true,
        BorderSet::Outside => outside,
        BorderSet::Inside => !outside,
        BorderSet::InsideHorizontal => !outside && horizontal,
        BorderSet::InsideVertical => !outside && !horizontal,
        BorderSet::None => false,
        edge => outside && *edge == side,
    })
}

/// A 2×2 sample of the picked cells: the edges the settings go on drawn
/// in the accent colour, the others faint.
fn sample<'a>(state: &CellBordersState) -> Element<'a, Message> {
    let rows = state.rows.clamp(1, 2);
    let columns = state.columns.clamp(1, 2);
    let mut grid = column![].spacing(0);
    for r in 0..rows {
        let mut line = row![].spacing(0);
        for c in 0..columns {
            let top = edge_chosen(&state.sets, r == 0, true, BorderSet::Top);
            let bottom = edge_chosen(&state.sets, r + 1 == rows, true, BorderSet::Bottom);
            let left = edge_chosen(&state.sets, c == 0, false, BorderSet::Left);
            let right = edge_chosen(&state.sets, c + 1 == columns, false, BorderSet::Right);
            let none = state.sets.contains(&BorderSet::None);
            let cell = container(
                column![
                    edge_bar(top && !none, true),
                    row![
                        edge_bar(left && !none, false),
                        Space::new().width(Fill).height(Fill),
                        edge_bar(right && !none, false),
                    ]
                    .height(Fill),
                    edge_bar(bottom && !none, true),
                ],
            )
            .width(Length::Fixed(54.0))
            .height(Length::Fixed(30.0));
            line = line.push(cell);
        }
        grid = grid.push(line);
    }
    container(grid).center_x(Fill).padding(6).into()
}

fn edge_bar<'a>(on: bool, horizontal: bool) -> Element<'a, Message> {
    let bar = container(Space::new()).style(move |theme: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(if on {
            theme.palette().primary.base.color
        } else {
            theme.palette().background.strong.color
        })),
        ..Default::default()
    });
    if horizontal {
        bar.width(Fill).height(Length::Fixed(if on { 3.0 } else { 1.0 }))
    } else {
        bar.height(Fill).width(Length::Fixed(if on { 3.0 } else { 1.0 }))
    }
    .into()
}

fn labelled<'a>(label: String, input: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        text(label).size(11).width(Length::Fixed(110.0)),
        input.into()
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn list<'a>(
    options: &'a [String],
    selected: &'a str,
    on: fn(String) -> TableCellDialogMsg,
) -> Element<'a, Message> {
    use crate::ui::properties::LocalizedChoice;
    let options: Vec<LocalizedChoice> = options.iter().cloned().map(LocalizedChoice::new).collect();
    pick_list(Some(LocalizedChoice::new(selected.to_string())), options, |value| {
        value.to_string()
    })
    .on_select(move |value: LocalizedChoice| msg(on(value.raw)))
        .text_size(11)
        .padding([3, 6])
        .width(Fill)
        .into()
}

fn footer<'a>() -> Element<'a, Message> {
    row![
        button(text("?").size(12))
            .on_press(msg(TableCellDialogMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button_styled_opt(crate::t!("Cancel"), Some(Message::CloseModal), button::secondary),
        dialog_button_styled_opt(
            crate::t!("OK"),
            Some(msg(TableCellDialogMsg::Ok)),
            button::primary
        ),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

pub fn view_borders<'a>(
    state: &'a CellBordersState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use TableCellDialogMsg as M;
    let line = card(
        crate::tr!("table-cells", "line"),
        column![
            labelled(
                crate::tr!("table-cells", "lineweight"),
                list(&state.lineweights, &state.lineweight, M::Lineweight)
            ),
            labelled(
                crate::tr!("table-cells", "linetype"),
                list(&state.linetypes, &state.linetype, M::Linetype)
            ),
            labelled(
                crate::tr!("table-cells", "color"),
                list(&state.colors, &state.color, M::Color)
            ),
            row![
                chip(
                    crate::tr!("table-cells", "double"),
                    state.double,
                    msg(M::Double(!state.double))
                ),
                text(crate::tr!("table-cells", "spacing")).size(11),
                {
                    let mut input = text_input("", &state.spacing)
                        .size(11)
                        .padding([4, 8])
                        .width(Length::Fixed(90.0))
                        .style(field_style);
                    if state.double {
                        input = input.on_input(|v| msg(M::Spacing(v)));
                    }
                    input
                },
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(8),
    );
    let set = |kind: BorderSet, label: String| -> Element<'a, Message> {
        let on = state.sets.contains(&kind);
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(M::Edges(kind)))
            .style(button_style(on))
            .padding([5, 6])
            .width(Fill)
            .into()
    };
    let edges = card(
        crate::tr!("table-cells", "apply-to"),
        row![
            column![
                row![
                    set(BorderSet::All, crate::tr!("table-cells", "all")),
                    set(BorderSet::Outside, crate::tr!("table-cells", "outside")),
                    set(BorderSet::Inside, crate::tr!("table-cells", "inside")),
                    set(BorderSet::None, crate::tr!("table-cells", "none")),
                ]
                .spacing(4),
                row![
                    set(BorderSet::Top, crate::tr!("table-cells", "top")),
                    set(BorderSet::Bottom, crate::tr!("table-cells", "bottom")),
                    set(BorderSet::Left, crate::tr!("table-cells", "left")),
                    set(BorderSet::Right, crate::tr!("table-cells", "right")),
                ]
                .spacing(4),
                row![
                    set(BorderSet::InsideHorizontal, crate::tr!("table-cells", "inside-horizontal")),
                    set(BorderSet::InsideVertical, crate::tr!("table-cells", "inside-vertical")),
                ]
                .spacing(4),
            ]
            .spacing(4)
            .width(Fill),
            container(sample(state)).width(Length::Fixed(130.0)),
        ]
        .spacing(10)
        .align_y(iced::Center),
    );
    column![line, edges, footer()]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

pub fn view_contents<'a>(
    state: &'a CellContentsState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use TableCellDialogMsg as M;
    let mut items = column![].spacing(2);
    if state.order.is_empty() {
        items = items.push(text(crate::tr!("table-cells", "empty")).size(11).style(muted_style));
    }
    for (position, &index) in state.order.iter().enumerate() {
        let label = state.items.get(index).cloned().unwrap_or_default();
        items = items.push(
            button(text(format!("{}.  {label}", position + 1)).size(11))
                .on_press(msg(M::Pick(position)))
                .style(button_style(state.selected == Some(position)))
                .padding([4, 8])
                .width(Fill),
        );
    }
    let picked = state.selected.is_some();
    let tool = |label: String, message: M, enabled: bool| -> Element<'a, Message> {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press_maybe(enabled.then(|| msg(message)))
            .style(button_style(false))
            .padding([5, 8])
            .width(Fill)
            .into()
    };
    let contents = card(
        crate::tr!("table-cells", "contents"),
        row![
            container(items).width(Fill),
            column![
                tool(crate::tr!("table-cells", "move-up"), M::Up, picked && state.selected != Some(0)),
                tool(
                    crate::tr!("table-cells", "move-down"),
                    M::Down,
                    picked && state.selected.is_some_and(|p| p + 1 < state.order.len())
                ),
                tool(crate::tr!("table-cells", "delete"), M::Delete, picked),
            ]
            .spacing(4)
            .width(Length::Fixed(110.0)),
        ]
        .spacing(10),
    );
    let layout_button = |bit: u32, label: String| -> Element<'a, Message> {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(M::Layout(bit)))
            .style(button_style(state.layout == bit))
            .padding([5, 6])
            .width(Fill)
            .into()
    };
    let layout = card(
        crate::tr!("table-cells", "layout"),
        column![
            row![
                layout_button(1, crate::tr!("table-cells", "flow")),
                layout_button(2, crate::tr!("table-cells", "stack-horizontal")),
                layout_button(4, crate::tr!("table-cells", "stack-vertical")),
            ]
            .spacing(4),
            labelled(crate::tr!("table-cells", "content-spacing"), {
                let mut input = text_input("", &state.spacing)
                    .size(11)
                    .padding([4, 8])
                    .width(Length::Fixed(90.0))
                    .style(field_style);
                if state.layout != 1 {
                    input = input.on_input(|v| msg(M::ContentSpacing(v)));
                }
                input
            }),
        ]
        .spacing(8),
    );
    column![contents, layout, footer()]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

pub struct CellBlockState {
    pub names: Vec<String>,
    pub name: String,
    /// A drawing file picked with Browse: it comes in as a block.
    pub path: String,
    /// Cell alignment 1–9 (top left … bottom right).
    pub alignment: i32,
    pub scale: String,
    pub autofit: bool,
    pub rotation: String,
}

/// The data types a cell can read as, in the window's order.
pub const FORMAT_KINDS: [&str; 9] = [
    "Angle",
    "Currency",
    "Date",
    "Decimal Number",
    "General",
    "Percentage",
    "Point",
    "Text",
    "Whole Number",
];
pub const ANGLE_FORMATS: [(&str, &str); 7] = [
    ("(none)", ""),
    ("Current units", "%au5"),
    ("Decimal degrees", "%au0"),
    ("Deg/min/sec", "%au1"),
    ("Grads", "%au2"),
    ("Radians", "%au3"),
    ("Surveyor's units", "%au4"),
];
pub const SEPARATORS: [char; 3] = [',', ';', '/'];

/// Date pictures offered as examples.
pub const DATE_EXAMPLES: [&str; 8] = [
    "M/d/yyyy",
    "MMMM d, yyyy",
    "yyyy-MM-dd",
    "dd.MM.yyyy",
    "d MMMM yyyy",
    "MMM-yy",
    "h:mm tt",
    "HH:mm:ss",
];

pub struct CellFormatState {
    pub kind: usize,
    pub unit: usize,
    /// 0 current precision, n + 1 for n decimals.
    pub precision: usize,
    pub symbol: String,
    pub append: bool,
    pub negative_parens: bool,
    pub separator: usize,
    pub axes: [bool; 3],
    pub date: String,
    /// The picked cell's number, previewed in the settings (a sample
    /// when it holds none), and the preview.
    pub sample: Option<f64>,
    pub preview: String,
    /// Additional format: conversion factor, text before and after, the
    /// decimal (`.` `,` space) and thousands (none `,` `.` `'`) separators
    /// and zero suppression bits (4 leading, 8 trailing, 1 0 feet, 2 0
    /// inches).
    pub conversion: String,
    pub prefix: String,
    pub suffix: String,
    pub decimal: usize,
    pub thousands: usize,
    pub zeros: u32,
}

pub const DECIMAL_SEPARATORS: [char; 3] = ['.', ',', ' '];
pub const THOUSANDS_SEPARATORS: [Option<char>; 4] = [None, Some(','), Some('.'), Some('\'')];

impl CellFormatState {
    /// Reads a stored picture back into the settings.
    pub fn read_picture(&mut self, picture: &str) {
        let digits = |key: &str| -> Option<usize> {
            let at = picture.find(key)? + key.len();
            let n: String = picture[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
            n.parse().ok()
        };
        if let Some(n) = digits("%pr") {
            self.precision = n + 1;
        }
        self.negative_parens = picture.contains("%nn1");
        if let Some(at) = picture.find("%ps[") {
            let inner = &picture[at + 4..];
            if let Some(end) = inner.find(']') {
                let (prefix, suffix) = inner[..end].split_once(',').unwrap_or((&inner[..end], ""));
                if self.kind == 1 {
                    self.append = prefix.is_empty() && !suffix.is_empty();
                    self.symbol = if self.append { suffix } else { prefix }.to_string();
                } else if self.kind == 5 {
                    self.append = suffix.starts_with('%');
                    self.prefix = prefix.to_string();
                    self.suffix = suffix.trim_start_matches('%').to_string();
                } else {
                    self.prefix = prefix.to_string();
                    self.suffix = suffix.to_string();
                }
            }
        } else if self.kind == 5 && !picture.is_empty() {
            self.append = false;
        }
        if let Some(code) = digits("%ds") {
            self.decimal = DECIMAL_SEPARATORS.iter().position(|c| *c as usize == code).unwrap_or(0);
        }
        if let Some(code) = digits("%th") {
            self.thousands = THOUSANDS_SEPARATORS
                .iter()
                .position(|c| c.is_some_and(|c| c as usize == code))
                .unwrap_or(0);
        }
        if let Some(bits) = digits("%zs") {
            self.zeros = bits as u32;
        }
        if let Some(at) = picture.find("%ct8[") {
            let inner = &picture[at + 5..];
            if let Some(end) = inner.find(']') {
                self.conversion = inner[..end].to_string();
            }
        }
        match self.kind {
            0 => {
                if let Some(at) = ANGLE_FORMATS
                    .iter()
                    .position(|(_, code)| !code.is_empty() && picture.starts_with(code))
                {
                    self.unit = at;
                }
            }
            3 | 6 => {
                if let Some(at) = codec::fields::FORMULA_FORMATS
                    .iter()
                    .position(|(_, code)| !code.is_empty() && picture.starts_with(code))
                {
                    self.unit = at;
                }
                if let Some(bits) = digits("%pt") {
                    self.axes = [bits & 1 != 0, bits & 2 != 0, bits & 4 != 0];
                }
                if let Some(code) = digits("%ls") {
                    self.separator = SEPARATORS
                        .iter()
                        .position(|c| *c as usize == code)
                        .unwrap_or(0);
                }
            }
            2 => self.date = picture.to_string(),
            _ => {}
        }
    }
}

impl CellFormatState {
    fn units(&self) -> Vec<&'static str> {
        match self.kind {
            0 => ANGLE_FORMATS.iter().map(|(name, _)| *name).collect(),
            3 | 6 => codec::fields::FORMULA_FORMATS.iter().map(|(name, _)| *name).collect(),
            _ => Vec::new(),
        }
    }

    fn precision_code(&self) -> String {
        if self.precision == 0 {
            String::new()
        } else {
            format!("%pr{}", self.precision - 1)
        }
    }

    /// Whether the settings have an additional format part.
    pub fn has_additional(&self) -> bool {
        matches!(self.kind, 0 | 1 | 3 | 5 | 6)
    }

    /// Feet and inches zero suppression apply to architectural and
    /// engineering units only.
    pub fn feet_inches(&self) -> bool {
        matches!(self.kind, 3 | 6) && matches!(self.unit, 3 | 4)
    }

    /// The field picture the settings make (`%lu2%pr2%nn1%ds44%zs12%th46`,
    /// `%ps[$,]`).
    pub fn picture(&self) -> String {
        let base = match self.kind {
            0 => ANGLE_FORMATS[self.unit.min(6)].1.to_string(),
            1 | 5 => "%lu2".to_string(),
            2 => return self.date.clone(),
            3 | 6 => codec::fields::FORMULA_FORMATS[self.unit.min(6)].1.to_string(),
            _ => return String::new(),
        };
        if base.is_empty() {
            return String::new();
        }
        let mut picture = format!("{base}{}", self.precision_code());
        if self.kind == 1 && self.negative_parens {
            picture.push_str("%nn1");
        }
        if self.kind == 6 {
            let bits = self
                .axes
                .iter()
                .enumerate()
                .filter(|(_, on)| **on)
                .fold(0, |acc, (axis, _)| acc | (1 << axis));
            if bits != 7 {
                picture.push_str(&format!("%pt{bits}"));
            }
            let separator = SEPARATORS[self.separator.min(2)];
            if separator != ',' {
                picture.push_str(&format!("%ls{}", separator as u32));
            }
        }
        let decimal = DECIMAL_SEPARATORS[self.decimal.min(2)];
        if decimal != '.' {
            picture.push_str(&format!("%ds{}", decimal as u32));
        }
        let zeros = if self.feet_inches() { self.zeros } else { self.zeros & 12 };
        if zeros != 0 {
            picture.push_str(&format!("%zs{zeros}"));
        }
        if let Some(separator) = THOUSANDS_SEPARATORS[self.thousands.min(3)] {
            picture.push_str(&format!("%th{}", separator as u32));
        }
        let factor = crate::entities::common::parse_f64(&self.conversion).unwrap_or(1.0);
        if (factor - 1.0).abs() > 1e-12 && factor != 0.0 {
            picture.push_str(&format!("%ct8[{factor}]"));
        }
        let (symbol_before, symbol_after) = match self.kind {
            1 if self.append => ("", self.symbol.as_str()),
            1 => (self.symbol.as_str(), ""),
            5 if self.append => ("", "%"),
            _ => ("", ""),
        };
        let before = format!("{}{symbol_before}", self.prefix);
        let after = format!("{symbol_after}{}", self.suffix);
        if !before.is_empty() || !after.is_empty() {
            picture.push_str(&format!("%ps[{before},{after}]"));
        }
        picture
    }
}

fn input<'a>(value: &str, on: Option<fn(String) -> TableCellDialogMsg>) -> Element<'a, Message> {
    let mut field = text_input("", value).size(11).padding([4, 8]).width(Fill).style(field_style);
    if let Some(on) = on {
        field = field.on_input(move |v| msg(on(v)));
    }
    field.into()
}

pub fn view_block<'a>(
    state: &'a CellBlockState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use TableCellDialogMsg as M;
    let block = card(
        crate::tr!("table-cells", "block"),
        column![
            labelled(
                crate::tr!("table-cells", "block-name"),
                row![
                    list(&state.names, &state.name, M::BlockName),
                    button(text(crate::tr!("table-cells", "browse")).size(11))
                        .on_press(msg(M::BlockBrowse))
                        .style(button_style(false))
                        .padding([4, 10]),
                ]
                .spacing(6)
                .align_y(iced::Center)
            ),
            labelled(
                crate::tr!("table-cells", "block-path"),
                text(state.path.clone()).size(11).style(muted_style)
            ),
        ]
        .spacing(8),
    );
    // A 3×3 pad: where in the cell the block sits.
    let mut pad = column![].spacing(3);
    for r in 0..3 {
        let mut line = row![].spacing(3);
        for c in 0..3 {
            let alignment = r * 3 + c + 1;
            let on = state.alignment == alignment;
            line = line.push(
                button(
                    container(
                        container(Space::new())
                            .width(Length::Fixed(8.0))
                            .height(Length::Fixed(8.0))
                            .style(move |theme: &iced::Theme| container::Style {
                                background: Some(iced::Background::Color(if on {
                                    theme.palette().primary.base.text
                                } else {
                                    theme.palette().background.strong.color
                                })),
                                ..Default::default()
                            }),
                    )
                    .center_x(Fill)
                    .center_y(Fill),
                )
                .on_press(msg(M::Align(alignment)))
                .style(button_style(on))
                .width(Length::Fixed(34.0))
                .height(Length::Fixed(24.0))
                .padding(0),
            );
        }
        pad = pad.push(line);
    }
    let scale_input: Option<fn(String) -> TableCellDialogMsg> =
        (!state.autofit).then_some(M::BlockScale);
    let rotation_input: Option<fn(String) -> TableCellDialogMsg> = Some(M::BlockRotation);
    let place = card(
        crate::tr!("table-cells", "placement"),
        row![
            column![text(crate::tr!("table-cells", "alignment")).size(11), pad].spacing(6),
            column![
                labelled(crate::tr!("table-cells", "scale"), input(&state.scale, scale_input)),
                row![
                    Space::new().width(Length::Fixed(118.0)),
                    chip(
                        crate::tr!("table-cells", "autofit"),
                        state.autofit,
                        msg(M::AutoFit(!state.autofit))
                    ),
                ],
                labelled(
                    crate::tr!("table-cells", "rotation"),
                    input(&state.rotation, rotation_input)
                ),
            ]
            .spacing(8)
            .width(Fill),
        ]
        .spacing(14),
    );
    column![block, place, footer()]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}

fn choice_list<'a>(
    names: &[&'static str],
    selected: usize,
    on: fn(usize) -> TableCellDialogMsg,
) -> Element<'a, Message> {
    let mut list = column![].spacing(2);
    for (index, name) in names.iter().enumerate() {
        list = list.push(
            button(text(crate::t!(*name).to_string()).size(11))
                .on_press(msg(on(index)))
                .style(button_style(selected == index))
                .padding([3, 8])
                .width(Fill),
        );
    }
    list.into()
}

/// The invalid data type warning: the cells whose values read as no value
/// of the chosen type, which converting empties; Cancel leaves every cell
/// as it was.
pub fn view_warning<'a>(cells: &'a [String], sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let mut chips = row![].spacing(4);
    for cell in cells.iter().take(8) {
        chips = chips.push(chip(cell.clone(), false, Message::Noop));
    }
    if cells.len() > 8 {
        chips = chips.push(text(format!("+{}", cells.len() - 8)).size(11).style(muted_style));
    }
    column![
        row![
            crate::ui::window::import_dialogs::warning_badge(),
            column![
                text(crate::tr!("table-cells", "warning-heading")).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::DEFAULT
                }),
                chips,
                text(crate::tr!("table-cells", "warning-note")).size(11).style(muted_style),
            ]
            .spacing(8),
        ]
        .spacing(12),
        iced::widget::rule::horizontal(1),
        row![
            Space::new().width(Fill),
            dialog_button_styled_opt(
                std::borrow::Cow::Owned(crate::tr!("table-cells", "warning-cancel")),
                Some(Message::CloseModal),
                iced::widget::button::secondary
            ),
            dialog_button_styled_opt(
                std::borrow::Cow::Owned(crate::tr!("table-cells", "warning-ok")),
                Some(msg(TableCellDialogMsg::WarnOk)),
                iced::widget::button::primary
            ),
        ]
        .spacing(8),
    ]
    .spacing(12)
    .padding([12, 14])
    .width(sizing.width)
    .into()
}

/// A value typed into a cell is not of the cell's data type: the type can
/// change to the value's, the value can go back into the editor, or Cancel
/// drops the value.
pub fn view_entry<'a>(
    cell: String,
    value: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use TableCellDialogMsg as M;
    let choice = |title: String, note: String, message: M| {
        iced::widget::button(
            column![
                text(title).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::DEFAULT
                }),
                text(note).size(11).style(muted_style),
            ]
            .spacing(2),
        )
        .on_press(msg(message))
        .width(Fill)
        .padding([8, 10])
        .style(iced::widget::button::secondary)
    };
    column![
        row![
            crate::ui::window::import_dialogs::warning_badge(),
            column![
                text(crate::tr!("table-cells", "entry-heading")).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::DEFAULT
                }),
                row![chip(cell, false, Message::Noop), chip(value.to_string(), true, Message::Noop)]
                    .spacing(4),
            ]
            .spacing(8),
        ]
        .spacing(12),
        choice(
            crate::tr!("table-cells", "entry-type"),
            crate::tr!("table-cells", "entry-type-note"),
            M::EntryType,
        ),
        choice(
            crate::tr!("table-cells", "entry-value"),
            crate::tr!("table-cells", "entry-value-note"),
            M::EntryValue,
        ),
        iced::widget::rule::horizontal(1),
        row![
            Space::new().width(Fill),
            dialog_button_styled_opt(
                std::borrow::Cow::Owned(crate::tr!("table-cells", "warning-cancel")),
                Some(Message::CloseModal),
                iced::widget::button::secondary
            ),
        ],
    ]
    .spacing(10)
    .padding([12, 14])
    .width(sizing.width)
    .into()
}

pub fn view_format<'a>(
    state: &'a CellFormatState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use TableCellDialogMsg as M;
    let preview = container(text(state.preview.clone()).size(15))
        .padding([8, 10])
        .width(Fill)
        .style(|theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(theme.palette().background.weak.color)),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    let mut settings = column![preview].spacing(8);
    let units = state.units();
    if !units.is_empty() {
        settings = settings.push(labelled(
            crate::tr!("table-cells", "format"),
            choice_list(&units, state.unit, M::FormatUnit),
        ));
    }
    if matches!(state.kind, 0 | 1 | 3 | 5 | 6) {
        let mut precisions = vec![crate::tr!("table-cells", "current-precision")];
        precisions.extend((0..=8).map(|n| {
            if n == 0 {
                "0".to_string()
            } else {
                format!("0.{}", "0".repeat(n))
            }
        }));
        let selected = precisions[state.precision.min(9)].clone();
        let enabled = !(matches!(state.kind, 0 | 3 | 6) && state.unit == 0);
        let precision: Element<'a, Message> = if enabled {
            pick_list(Some(selected), precisions, |value| value.clone())
                .on_select(|value| msg(M::Precision(value)))
                .text_size(11)
                .padding([3, 6])
                .width(Fill)
                .into()
        } else {
            text(selected).size(11).style(muted_style).into()
        };
        settings = settings.push(labelled(crate::tr!("table-cells", "precision"), precision));
    }
    if state.kind == 1 {
        let symbol_input: Option<fn(String) -> TableCellDialogMsg> = Some(M::Symbol);
        settings = settings.push(labelled(
            crate::tr!("table-cells", "symbol"),
            input(&state.symbol, symbol_input),
        ));
        settings = settings.push(
            row![
                chip(
                    crate::tr!("table-cells", "append-symbol"),
                    state.append,
                    msg(M::Append(!state.append))
                ),
                chip(
                    crate::tr!("table-cells", "negative-parens"),
                    state.negative_parens,
                    msg(M::Negative(!state.negative_parens))
                ),
            ]
            .spacing(6),
        );
    }
    if state.kind == 5 {
        settings = settings.push(chip(
            crate::tr!("table-cells", "append-symbol"),
            state.append,
            msg(M::Append(!state.append)),
        ));
    }
    if state.kind == 6 {
        let mut separators = row![].spacing(4);
        for (index, separator) in SEPARATORS.iter().enumerate() {
            separators = separators.push(
                button(text(separator.to_string()).size(11))
                    .on_press(msg(M::Separator(index)))
                    .style(button_style(state.separator == index))
                    .padding([4, 12]),
            );
        }
        settings = settings.push(labelled(crate::tr!("table-cells", "list-separator"), separators));
        settings = settings.push(
            row![
                chip("X".to_string(), state.axes[0], msg(M::Axis(0))),
                chip("Y".to_string(), state.axes[1], msg(M::Axis(1))),
                chip("Z".to_string(), state.axes[2], msg(M::Axis(2))),
            ]
            .spacing(6),
        );
    }
    if state.kind == 2 {
        let date_input: Option<fn(String) -> TableCellDialogMsg> = Some(M::DateFormat);
        settings = settings.push(labelled(
            crate::tr!("table-cells", "date-format"),
            input(&state.date, date_input),
        ));
        let mut examples = column![].spacing(2);
        for example in DATE_EXAMPLES {
            examples = examples.push(
                button(text(example).size(11))
                    .on_press(msg(M::DateFormat(example.to_string())))
                    .style(button_style(state.date == example))
                    .padding([3, 8])
                    .width(Fill),
            );
        }
        settings = settings.push(examples);
    }
    if state.has_additional() {
        let conversion: Option<fn(String) -> TableCellDialogMsg> = Some(M::Conversion);
        let prefix: Option<fn(String) -> TableCellDialogMsg> = Some(M::Prefix);
        let suffix: Option<fn(String) -> TableCellDialogMsg> = Some(M::Suffix);
        let mut decimals = row![].spacing(4);
        for (index, separator) in DECIMAL_SEPARATORS.iter().enumerate() {
            let label = if *separator == ' ' {
                crate::tr!("table-cells", "space")
            } else {
                separator.to_string()
            };
            decimals = decimals.push(
                button(text(label).size(11))
                    .on_press(msg(M::DecimalSeparator(index)))
                    .style(button_style(state.decimal == index))
                    .padding([4, 10]),
            );
        }
        let mut thousands = row![].spacing(4);
        for (index, separator) in THOUSANDS_SEPARATORS.iter().enumerate() {
            let label = separator
                .map(|c| c.to_string())
                .unwrap_or_else(|| crate::tr!("table-cells", "separator-none"));
            thousands = thousands.push(
                button(text(label).size(11))
                    .on_press(msg(M::ThousandsSeparator(index)))
                    .style(button_style(state.thousands == index))
                    .padding([4, 10]),
            );
        }
        let zero = |bit: u32, label: String, enabled: bool| -> Element<'a, Message> {
            if enabled {
                chip(label, state.zeros & bit != 0, msg(M::Zero(bit)))
            } else {
                text(label).size(11).style(muted_style).into()
            }
        };
        settings = settings.push(card(
            crate::tr!("table-cells", "additional-format"),
            column![
                labelled(
                    crate::tr!("table-cells", "conversion"),
                    input(&state.conversion, conversion)
                ),
                labelled(crate::tr!("table-cells", "prefix"), input(&state.prefix, prefix)),
                labelled(crate::tr!("table-cells", "suffix"), input(&state.suffix, suffix)),
                labelled(crate::tr!("table-cells", "decimal-separator"), decimals),
                labelled(crate::tr!("table-cells", "thousands-separator"), thousands),
                row![
                    zero(4, crate::tr!("table-cells", "zero-leading"), true),
                    zero(8, crate::tr!("table-cells", "zero-trailing"), true),
                    zero(1, crate::tr!("table-cells", "zero-feet"), state.feet_inches()),
                    zero(2, crate::tr!("table-cells", "zero-inches"), state.feet_inches()),
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(6),
        ));
    }
    if matches!(state.kind, 4 | 7 | 8) {
        settings = settings.push(
            text(crate::tr!("table-cells", "no-format-settings")).size(11).style(muted_style),
        );
    }
    let body = row![
        container(card(
            crate::tr!("table-cells", "data-type"),
            choice_list(&FORMAT_KINDS, state.kind, M::FormatKind)
        ))
        .width(Length::Fixed(160.0)),
        container(card(crate::tr!("table-cells", "format-settings"), settings)).width(Fill),
    ]
    .spacing(10);
    column![body, footer()]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}
