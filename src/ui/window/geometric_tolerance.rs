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
    plain_frame: bool,
    symbol_tail: String,
    extra_rows: Vec<String>,
    original_text: Option<String>,
    dirty: bool,
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
            plain_frame: false,
            symbol_tail: String::new(),
            extra_rows: Vec::new(),
            original_text: None,
            dirty: false,
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

fn remove_escape(input: &str, code: &str) -> Option<String> {
    let target = code.chars().next()?;
    for (start, ch) in input.char_indices() {
        if ch != '{' {
            continue;
        }
        let tail = &input[start + 1..];
        let Some(relative_end) = tail.find('}') else {
            break;
        };
        let end = start + 1 + relative_end;
        if crate::entities::tolerance::symbol_font_switch(&input[start + 1..end])
            == Some(target)
        {
            let mut remaining = String::with_capacity(input.len() - (end + 1 - start));
            remaining.push_str(&input[..start]);
            remaining.push_str(&input[end + 1..]);
            return Some(remaining);
        }
    }
    None
}

fn parse_material(input: &str) -> (String, String) {
    for code in ["m", "l", "s"] {
        if let Some(value) = remove_escape(input, code) {
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
        let lines: Vec<&str> = normalized.split('\n').collect();
        state.original_text = (!raw.is_empty()).then(|| raw.to_string());

        if let Some(frame) = lines.first().copied() {
            let cells: Vec<&str> = frame.split("%%v").collect();
            if let Some(cell) = cells.first() {
                let mut remaining = (*cell).to_string();
                for (code, _) in SYMBOLS.iter().filter(|(code, _)| !code.is_empty()) {
                    if let Some(value) = remove_escape(&remaining, code) {
                        state.symbol = (*code).to_string();
                        remaining = value;
                        break;
                    }
                }
                state.symbol_tail = remaining;
            }
            state.plain_frame = !raw.is_empty() && cells.len() == 1 && state.symbol.is_empty();
            if state.plain_frame {
                state.tolerances[0].value = frame.to_string();
                state.symbol_tail.clear();
            } else {
                for index in 0..2 {
                    if let Some(cell) = cells.get(index + 1) {
                        let (cell, diameter) = remove_escape(cell, "n")
                            .map_or_else(|| ((*cell).to_string(), false), |value| (value, true));
                        let (value, material) = parse_material(&cell);
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
            }
        }

        let mut next = 1;
        if let Some(projected) = lines.get(next).copied() {
            if let Some(value) = remove_escape(projected, "p") {
                state.projected_height = value;
                state.projected_zone = true;
                next += 1;
            } else if lines.get(next + 1).is_some()
                && !projected.contains("%%v")
                && !lines[next + 1].contains("%%v")
            {
                state.projected_height = projected.to_string();
                next += 1;
            }
        }
        if next > 1 {
            if let Some(identifier) = lines.get(next).copied() {
                if !identifier.contains("%%v") {
                    state.datum_identifier = identifier.to_string();
                    next += 1;
                }
            }
        }
        state.extra_rows = lines[next..].iter().map(|row| (*row).to_string()).collect();
        state
    }

    pub fn apply_field(&mut self, field: Field) {
        let changed = match field {
            Field::Symbol(value) => {
                let changed = self.symbol != value;
                self.symbol = value;
                changed
            }
            Field::ToleranceValue(index, value) if index < 2 => {
                let changed = self.tolerances[index].value != value;
                self.tolerances[index].value = value;
                changed
            }
            Field::ToleranceMaterial(index, value) if index < 2 => {
                let changed = self.tolerances[index].material != value;
                self.tolerances[index].material = value;
                changed
            }
            Field::DatumValue(index, value) if index < 3 => {
                let changed = self.datums[index].value != value;
                self.datums[index].value = value;
                changed
            }
            Field::DatumMaterial(index, value) if index < 3 => {
                let changed = self.datums[index].material != value;
                self.datums[index].material = value;
                changed
            }
            Field::ProjectedHeight(value) => {
                let changed = self.projected_height != value;
                self.projected_height = value;
                changed
            }
            Field::DatumIdentifier(value) => {
                let changed = self.datum_identifier != value;
                self.datum_identifier = value;
                changed
            }
            _ => false,
        };
        self.dirty |= changed;
    }

    pub fn apply_toggle(&mut self, toggle: Toggle) {
        let changed = match toggle {
            Toggle::Diameter(index, value) if index < 2 => {
                let changed = self.tolerances[index].diameter != value;
                self.tolerances[index].diameter = value;
                changed
            }
            Toggle::ProjectedZone(value) => {
                let changed = self.projected_zone != value;
                self.projected_zone = value;
                changed
            }
            _ => false,
        };
        self.dirty |= changed;
    }

    pub fn is_valid(&self) -> bool {
        !self.symbol.is_empty()
            || !self.symbol_tail.is_empty()
            || self.tolerances.iter().any(|entry| !entry.value.trim().is_empty())
            || self.datums.iter().any(|entry| !entry.value.trim().is_empty())
            || !self.projected_height.trim().is_empty()
            || !self.datum_identifier.trim().is_empty()
            || self.extra_rows.iter().any(|row| !row.trim().is_empty())
    }

    pub fn to_text(&self) -> String {
        if !self.dirty {
            if let Some(original) = &self.original_text {
                return original.clone();
            }
        }

        let only_plain_value = self.plain_frame
            && self.symbol.is_empty()
            && self.symbol_tail.is_empty()
            && self.tolerances[1] == ToleranceEntry::default()
            && self.datums.iter().all(|entry| *entry == DatumEntry::default());
        let frame = if only_plain_value {
            self.tolerances[0].value.clone()
        } else {
            let mut cells = Vec::with_capacity(6);
            cells.push(format!("{}{}", escape(&self.symbol), self.symbol_tail));
            for entry in &self.tolerances {
                let mut value = String::new();
                if entry.diameter && !entry.value.trim().is_empty() {
                    value.push_str(&escape("n"));
                }
                value.push_str(&entry.value);
                if !entry.value.trim().is_empty() {
                    value.push_str(&escape(&entry.material));
                }
                cells.push(value);
            }
            for entry in &self.datums {
                let mut value = entry.value.clone();
                if !value.trim().is_empty() {
                    value.push_str(&escape(&entry.material));
                }
                cells.push(value);
            }
            while cells.last().is_some_and(String::is_empty) {
                cells.pop();
            }
            cells.join("%%v")
        };
        let mut rows = vec![frame];
        if !self.projected_height.trim().is_empty() || self.projected_zone {
            let mut projected = self.projected_height.clone();
            if self.projected_zone {
                projected.push_str(&escape("p"));
            }
            rows.push(projected);
        }
        if !self.datum_identifier.trim().is_empty() {
            if rows.len() == 1 {
                rows.push(String::new());
            }
            rows.push(self.datum_identifier.clone());
        }
        rows.extend(self.extra_rows.iter().cloned());
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
