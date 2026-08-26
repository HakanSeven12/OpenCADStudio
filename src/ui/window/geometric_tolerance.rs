//! Structured feature-control-frame editor.
//!
//! The entity stores a compact escape string, but users work with named
//! symbols, tolerance compartments and datum references.  This dialog keeps
//! that storage detail behind typed controls and can also reopen existing
//! frames without losing their compartment layout.

use std::fmt;

use acadrust::Handle;
use iced::widget::{button, checkbox, column, container, row, text, text_input, Space};
use iced::{Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::t;

const EMPTY: &str = "";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToleranceEntry {
    pub diameter: bool,
    pub value: String,
    pub material: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DatumEntry {
    pub value: String,
    pub material: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub editing: Option<Handle>,
    pub symbol: String,
    pub tolerances: [ToleranceEntry; 2],
    pub datums: [DatumEntry; 3],
    pub projected_height: String,
    pub projected_zone: bool,
    pub datum_identifier: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            editing: None,
            symbol: String::new(),
            tolerances: std::array::from_fn(|_| ToleranceEntry::default()),
            datums: std::array::from_fn(|_| DatumEntry::default()),
            projected_height: String::new(),
            projected_zone: false,
            datum_identifier: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Field {
    Symbol(String),
    ToleranceValue(usize, String),
    ToleranceMaterial(usize, String),
    DatumValue(usize, String),
    DatumMaterial(usize, String),
    ProjectedHeight(String),
    DatumIdentifier(String),
}

#[derive(Clone, Debug)]
pub enum Toggle {
    Diameter(usize, bool),
    ProjectedZone(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice {
    code: &'static str,
    label: &'static str,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(crate::i18n::translate(self.label).as_ref())
    }
}

const SYMBOLS: [(&str, &str); 15] = [
    ("", "None"),
    ("u", "Straightness"),
    ("c", "Flatness"),
    ("e", "Circularity"),
    ("g", "Cylindricity"),
    ("d", "Profile of a surface"),
    ("k", "Profile of a line"),
    ("j", "Position"),
    ("r", "Concentricity"),
    ("i", "Symmetry"),
    ("f", "Parallelism"),
    ("b", "Perpendicularity"),
    ("a", "Angularity"),
    ("h", "Circular runout"),
    ("t", "Total runout"),
];

const MATERIALS: [(&str, &str); 4] = [
    ("", "None"),
    ("m", "Maximum material condition"),
    ("l", "Least material condition"),
    ("s", "Regardless of feature size"),
];

fn choices(values: &'static [(&'static str, &'static str)]) -> Vec<Choice> {
    values
        .iter()
        .map(|(code, label)| Choice { code, label })
        .collect()
}

fn selected(values: &'static [(&'static str, &'static str)], code: &str) -> Option<Choice> {
    values
        .iter()
        .find(|(value, _)| value.eq_ignore_ascii_case(code))
        .map(|(code, label)| Choice { code, label })
}

fn escape(code: &str) -> String {
    if code.is_empty() {
        String::new()
    } else {
        format!("{{\\Fgdt;{code}}}")
    }
}

fn strip_escape(input: &str, code: &str) -> Option<String> {
    let marker = escape(code);
    input
        .strip_suffix(&marker)
        .map(std::string::ToString::to_string)
}

fn parse_material(input: &str) -> (String, String) {
    for code in ["m", "l", "s"] {
        if let Some(value) = strip_escape(input, code) {
            return (value, code.to_string());
        }
    }
    (input.to_string(), String::new())
}

impl State {
    pub fn from_text(editing: Option<Handle>, raw: &str) -> Self {
        let mut state = Self {
            editing,
            ..Self::default()
        };
        let normalized = raw
            .replace("^J", "\n")
            .replace("\\P", "\n")
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let mut lines = normalized.lines();

        if let Some(frame) = lines.next() {
            let cells: Vec<&str> = frame.split("%%v").collect();
            if let Some(cell) = cells.first() {
                state.symbol = SYMBOLS
                    .iter()
                    .find_map(|(code, _)| {
                        (!code.is_empty() && cell.contains(&escape(code))).then(|| code.to_string())
                    })
                    .unwrap_or_default();
            }
            for index in 0..2 {
                if let Some(cell) = cells.get(index + 1) {
                    let (cell, diameter) = if let Some(rest) = cell.strip_prefix(&escape("n")) {
                        (rest, true)
                    } else {
                        (*cell, false)
                    };
                    let (value, material) = parse_material(cell);
                    state.tolerances[index] = ToleranceEntry {
                        diameter,
                        value,
                        material,
                    };
                }
            }
            for index in 0..3 {
                if let Some(cell) = cells.get(index + 3) {
                    let (value, material) = parse_material(cell);
                    state.datums[index] = DatumEntry { value, material };
                }
            }
            if cells.len() == 1
                && state.symbol.is_empty()
                && !frame.trim().is_empty()
            {
                state.tolerances[0].value = frame.trim().to_string();
            }
        }

        if let Some(projected) = lines.next() {
            if let Some(value) = strip_escape(projected, "p") {
                state.projected_height = value;
                state.projected_zone = true;
            } else {
                state.projected_height = projected.to_string();
            }
        }
        if let Some(identifier) = lines.next() {
            state.datum_identifier = identifier.to_string();
        }
        state
    }

    pub fn apply_field(&mut self, field: Field) {
        match field {
            Field::Symbol(value) => self.symbol = value,
            Field::ToleranceValue(index, value) if index < 2 => {
                self.tolerances[index].value = value
            }
            Field::ToleranceMaterial(index, value) if index < 2 => {
                self.tolerances[index].material = value
            }
            Field::DatumValue(index, value) if index < 3 => self.datums[index].value = value,
            Field::DatumMaterial(index, value) if index < 3 => {
                self.datums[index].material = value
            }
            Field::ProjectedHeight(value) => self.projected_height = value,
            Field::DatumIdentifier(value) => self.datum_identifier = value,
            _ => {}
        }
    }

    pub fn apply_toggle(&mut self, toggle: Toggle) {
        match toggle {
            Toggle::Diameter(index, value) if index < 2 => {
                self.tolerances[index].diameter = value
            }
            Toggle::ProjectedZone(value) => self.projected_zone = value,
            _ => {}
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.symbol.is_empty()
            || self.tolerances.iter().any(|entry| !entry.value.trim().is_empty())
            || self.datums.iter().any(|entry| !entry.value.trim().is_empty())
            || !self.projected_height.trim().is_empty()
            || !self.datum_identifier.trim().is_empty()
    }

    pub fn to_text(&self) -> String {
        let mut cells = Vec::with_capacity(6);
        cells.push(escape(&self.symbol));
        for entry in &self.tolerances {
            let mut value = String::new();
            if entry.diameter && !entry.value.trim().is_empty() {
                value.push_str(&escape("n"));
            }
            value.push_str(entry.value.trim());
            if !entry.value.trim().is_empty() {
                value.push_str(&escape(&entry.material));
            }
            cells.push(value);
        }
        for entry in &self.datums {
            let mut value = entry.value.trim().to_string();
            if !value.is_empty() {
                value.push_str(&escape(&entry.material));
            }
            cells.push(value);
        }
        while cells.last().is_some_and(String::is_empty) {
            cells.pop();
        }
        let mut rows = vec![cells.join("%%v")];
        if !self.projected_height.trim().is_empty() || self.projected_zone {
            let mut projected = self.projected_height.trim().to_string();
            if self.projected_zone {
                projected.push_str(&escape("p"));
            }
            rows.push(projected);
        }
        if !self.datum_identifier.trim().is_empty() {
            if rows.len() == 1 {
                rows.push(String::new());
            }
            rows.push(self.datum_identifier.trim().to_string());
        }
        rows.join("\n")
    }
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
    }
}

fn panel<'a>(title: String, body: Element<'a, Message>) -> Element<'a, Message> {
    container(column![text(title).size(11).style(muted), body].spacing(6))
        .padding(8)
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            border: Border {
                width: 1.0,
                radius: 4.0.into(),
                color: theme.palette().background.strong.color,
            },
            ..Default::default()
        })
        .into()
}

fn material_picker<'a>(index: usize, datum: bool, code: &str) -> Element<'a, Message> {
    let options = choices(&MATERIALS);
    let selected = selected(&MATERIALS, code);
    iced::widget::pick_list(selected, options, |choice| choice.to_string())
        .on_select(move |choice| {
            if datum {
                Message::ToleranceDialogField(Field::DatumMaterial(
                    index,
                    choice.code.to_string(),
                ))
            } else {
                Message::ToleranceDialogField(Field::ToleranceMaterial(
                    index,
                    choice.code.to_string(),
                ))
            }
        })
        .text_size(11)
        .padding([3, 6])
        .width(Length::Fixed(190.0))
        .into()
}

pub fn view_window<'a>(
    state: &State,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let symbol_options = choices(&SYMBOLS);
    let symbol_selected = selected(&SYMBOLS, &state.symbol);
    let symbol = panel(
        t!("Geometric characteristic").into_owned(),
        iced::widget::pick_list(symbol_selected, symbol_options, |choice| choice.to_string())
            .on_select(|choice| {
                Message::ToleranceDialogField(Field::Symbol(choice.code.to_string()))
            })
            .text_size(12)
            .padding([4, 6])
            .width(Fill)
            .into(),
    );

    let tolerance_rows = state.tolerances.iter().enumerate().fold(
        column![].spacing(6),
        |column, (index, entry)| {
            column.push(
                row![
                    text(format!("{} {}", t!("Tolerance"), index + 1))
                        .size(11)
                        .style(muted)
                        .width(Length::Fixed(82.0)),
                    checkbox(entry.diameter)
                        .on_toggle(move |value| Message::ToleranceDialogToggle(
                            Toggle::Diameter(index, value)
                        ))
                        .size(14),
                    text(t!("Diameter")).size(11).width(Length::Fixed(62.0)),
                    text_input(EMPTY, &entry.value)
                        .on_input(move |value| Message::ToleranceDialogField(
                            Field::ToleranceValue(index, value)
                        ))
                        .size(12)
                        .padding([3, 6])
                        .width(Length::Fixed(125.0)),
                    material_picker(index, false, &entry.material),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
        },
    );
    let tolerances = panel(t!("Tolerance values").into_owned(), tolerance_rows.into());

    let datum_rows = state.datums.iter().enumerate().fold(
        column![].spacing(6),
        |column, (index, entry)| {
            column.push(
                row![
                    text(format!("{} {}", t!("Datum"), index + 1))
                        .size(11)
                        .style(muted)
                        .width(Length::Fixed(82.0)),
                    text_input(EMPTY, &entry.value)
                        .on_input(move |value| Message::ToleranceDialogField(
                            Field::DatumValue(index, value)
                        ))
                        .size(12)
                        .padding([3, 6])
                        .width(Length::Fixed(212.0)),
                    material_picker(index, true, &entry.material),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
        },
    );
    let datums = panel(t!("Datum references").into_owned(), datum_rows.into());

    let additions = panel(
        t!("Additional information").into_owned(),
        column![
            row![
                text(t!("Projected height"))
                    .size(11)
                    .style(muted)
                    .width(Length::Fixed(112.0)),
                text_input(EMPTY, &state.projected_height)
                    .on_input(|value| Message::ToleranceDialogField(Field::ProjectedHeight(value)))
                    .size(12)
                    .padding([3, 6])
                    .width(Length::Fixed(125.0)),
                checkbox(state.projected_zone)
                    .on_toggle(|value| Message::ToleranceDialogToggle(
                        Toggle::ProjectedZone(value)
                    ))
                    .size(14),
                text(t!("Projected tolerance zone")).size(11),
            ]
            .spacing(6)
            .align_y(iced::Center),
            row![
                text(t!("Datum identifier"))
                    .size(11)
                    .style(muted)
                    .width(Length::Fixed(112.0)),
                text_input(EMPTY, &state.datum_identifier)
                    .on_input(|value| Message::ToleranceDialogField(Field::DatumIdentifier(value)))
                    .size(12)
                    .padding([3, 6])
                    .width(Length::Fixed(212.0)),
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(6)
        .into(),
    );

    let mut actions = row![
        Space::new().width(Fill),
        button(text(t!("Cancel")).size(12))
            .on_press(Message::CloseModal)
            .padding([4, 12]),
    ]
    .spacing(6)
    .align_y(iced::Center);
    if state.editing.is_some() {
        actions = actions.push(
            button(text(t!("Apply")).size(12))
                .on_press_maybe(state.is_valid().then_some(Message::ToleranceDialogApply))
                .padding([4, 12]),
        );
    }
    actions = actions.push(
        button(text(t!("OK")).size(12))
            .on_press_maybe(state.is_valid().then_some(Message::ToleranceDialogOk))
            .style(button::primary)
            .padding([4, 12]),
    );

    column![symbol, tolerances, datums, additions, actions]
        .spacing(8)
        .padding(10)
        .width(sizing.width)
        .into()
}
