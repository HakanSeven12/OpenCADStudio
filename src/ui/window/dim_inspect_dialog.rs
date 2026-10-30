//! Inspection Dimension window (DIMINSPECT): the dimensions it applies to,
//! the frame shape shown as three live samples, and the label and inspection
//! rate switches with their values.

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Element, Fill, Length, Theme};

use crate::app::Message;
use crate::entities::dim_inspect::Shape;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, chip};

#[derive(Debug, Clone)]
pub enum DimInspectMsg {
    Shape(Shape),
    LabelOn(bool),
    Label(String),
    RateOn(bool),
    Rate(String),
    /// Hide the window and pick the dimensions.
    Select,
    /// Take the inspection off the dimensions.
    Remove,
    Help,
    Ok,
}

fn msg(m: DimInspectMsg) -> Message {
    Message::DimInspect(m)
}

pub struct DimInspectState {
    pub handles: Vec<codec::Handle>,
    /// Some of the dimensions show an inspection.
    pub any_inspected: bool,
    pub shape: Shape,
    pub label_on: bool,
    pub label: String,
    pub rate_on: bool,
    pub rate: String,
}

/// A sample of the inspection text in `shape`, with the current label and
/// rate.
fn sample<'a>(state: &DimInspectState, shape: Shape) -> Element<'a, Message> {
    let mut parts: Vec<String> = Vec::new();
    if state.label_on {
        parts.push(if state.label.trim().is_empty() {
            crate::tr!("dim-inspect", "label")
        } else {
            state.label.clone()
        });
    }
    parts.push("X.XX".to_string());
    if state.rate_on {
        parts.push(state.rate.clone());
    }
    let separator = if shape == Shape::None { "  " } else { " │ " };
    let mut body = parts.join(separator);
    if shape == Shape::Angular {
        body = format!("‹ {body} ›");
    }
    container(text(body).size(11))
        .padding([2, if shape == Shape::Round { 9 } else { 4 }])
        .style(move |theme: &Theme| container::Style {
            border: iced::Border {
                width: if shape == Shape::None { 0.0 } else { 1.0 },
                radius: if shape == Shape::Round { 10.0 } else { 1.0 }.into(),
                color: theme.palette().background.base.text,
            },
            ..Default::default()
        })
        .into()
}

/// One frame choice: its sample above its name, framed in the accent colour
/// while chosen.
fn shape_tile<'a>(state: &DimInspectState, shape: Shape, label: String) -> Element<'a, Message> {
    let chosen = state.shape == shape;
    button(
        column![
            container(sample(state, shape))
                .center_x(Fill)
                .height(Length::Fixed(30.0))
                .center_y(Length::Fixed(30.0)),
            text(label).size(11).width(Fill).align_x(iced::Center),
        ]
        .spacing(4),
    )
    .on_press(msg(DimInspectMsg::Shape(shape)))
    .style(button_style(chosen))
    .padding([8, 6])
    .width(Fill)
    .into()
}

/// A switch and the value it turns on.
fn switched_field<'a>(
    label: String,
    on: bool,
    value: &'a str,
    toggle: DimInspectMsg,
    edit: fn(String) -> DimInspectMsg,
) -> Element<'a, Message> {
    let mut input = text_input("", value)
        .size(11)
        .padding([4, 8])
        .width(Fill)
        .style(field_style);
    if on {
        input = input.on_input(move |v| msg(edit(v)));
    }
    row![
        container(chip(label, on, msg(toggle))).width(Length::Fixed(150.0)),
        input
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

pub fn view<'a>(
    state: &'a DimInspectState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let count = state.handles.len();
    let selected = if count == 0 {
        crate::tr!("dim-inspect", "none-selected")
    } else {
        crate::tr!("dim-inspect", "selected", count = count)
    };
    let dimensions = card(
        crate::tr!("dim-inspect", "dimensions"),
        row![
            button(
                row![
                    crate::ui::icons::themed_primary(include_bytes!("../../../assets/icons/dim_linear.svg"), 14.0),
                    text(crate::tr!("dim-inspect", "select")).size(11)
                ]
                .spacing(6)
                .align_y(iced::Center)
            )
            .on_press(msg(DimInspectMsg::Select))
            .style(button_style(false))
            .padding([5, 10]),
            text(selected).size(11).style(muted_style),
            Space::new().width(Fill),
            button(text(crate::tr!("dim-inspect", "remove")).size(11))
                .on_press_maybe(state.any_inspected.then(|| msg(DimInspectMsg::Remove)))
                .style(button_style(false))
                .padding([5, 10]),
        ]
        .spacing(10)
        .align_y(iced::Center),
    );
    let shape = card(
        crate::tr!("dim-inspect", "shape"),
        row![
            shape_tile(state, Shape::Round, crate::tr!("dim-inspect", "round")),
            shape_tile(state, Shape::Angular, crate::tr!("dim-inspect", "angular")),
            shape_tile(state, Shape::None, crate::tr!("dim-inspect", "no-frame")),
        ]
        .spacing(6),
    );
    let values = card(
        crate::tr!("dim-inspect", "label-rate"),
        column![
            switched_field(
                crate::tr!("dim-inspect", "label"),
                state.label_on,
                &state.label,
                DimInspectMsg::LabelOn(!state.label_on),
                DimInspectMsg::Label,
            ),
            switched_field(
                crate::tr!("dim-inspect", "rate"),
                state.rate_on,
                &state.rate,
                DimInspectMsg::RateOn(!state.rate_on),
                DimInspectMsg::Rate,
            ),
        ]
        .spacing(8),
    );
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(DimInspectMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button_styled_opt(crate::t!("Cancel"), Some(Message::CloseModal), button::secondary),
        dialog_button_styled_opt(
            crate::t!("OK"),
            Some(msg(DimInspectMsg::Ok)),
            button::primary
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![dimensions, shape, values, footer]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .into()
}
