//! The insertion-time `Edit Attributes` form that `ATTDIA` puts in front of the
//! command-line prompt chain: the block being placed, one answered row per
//! askable attribute, and the disabled slots the form keeps below them.

use crate::app::Message;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::card_style;
use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Element, Fill, Length};

/// One attribute the operator answers while the block is being placed.
#[derive(Debug, Clone)]
pub struct InsertAttrRow {
    /// What the attribute is called in the definition.
    pub tag: String,
    /// What the form shows: the prompt when the definition has one.
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct InsertAttrState {
    pub block_name: String,
    pub rows: Vec<InsertAttrRow>,
}

#[derive(Debug, Clone)]
pub enum InsertAttrMsg {
    Value(usize, String),
    Ok,
    Help,
}

/// Empty slots kept below the answered attributes, as the form shows them.
/// The reading counted fifteen; the picture of the same form shows fourteen.
const SPARE_SLOTS: usize = 15;

/// Width of the label column; the block name and every field start after it.
const LABEL_WIDTH: f32 = 175.0;

/// A row carries its name and a colon of its own; an empty name shows nothing.
pub fn caption(name: &str) -> String {
    if name.is_empty() {
        String::new()
    } else {
        format!("{}:", name.trim_end_matches(':'))
    }
}

fn line<'a>(label: String, value: Element<'a, Message>) -> Element<'a, Message> {
    row![
        container(text(label).size(12)).width(Length::Fixed(LABEL_WIDTH)),
        container(value).width(Fill).padding([0, 8]),
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn slot<'a>(value: &'a str, index: usize) -> Element<'a, Message> {
    text_input("", value)
        .size(12)
        .padding([5, 8])
        .width(Fill)
        .style(field_style)
        .on_input(move |value| Message::InsertAttrDialog(InsertAttrMsg::Value(index, value)))
        .into()
}

fn empty_slot() -> Element<'static, Message> {
    text_input("", "")
        .size(12)
        .padding([5, 8])
        .width(Fill)
        .style(field_style)
        .into()
}

pub fn view(
    state: &InsertAttrState,
    _sizing: crate::ui::modal::ModalSizing,
) -> Element<'_, Message> {
    let mut body = column![line(
        caption(&crate::t!("Block name")),
        text(state.block_name.clone()).size(12).into()
    )]
    .spacing(6);
    for (index, row) in state.rows.iter().enumerate() {
        body = body.push(line(caption(&row.label), slot(&row.value, index)));
    }
    for _ in 0..SPARE_SLOTS {
        body = body.push(line(String::new(), empty_slot()));
    }
    // One block is being placed, so there is nothing to step between.
    let spread = || Space::new().width(Fill);
    let footer = row![
        dialog_button(
            crate::t!("OK").into_owned(),
            Message::InsertAttrDialog(InsertAttrMsg::Ok),
            true,
        ),
        spread(),
        dialog_button(crate::t!("Cancel").into_owned(), Message::CloseModal, false),
        spread(),
        dialog_button_styled_opt(crate::t!("Previous").into_owned(), None, button::secondary),
        spread(),
        dialog_button_styled_opt(crate::t!("Next").into_owned(), None, button::secondary),
        spread(),
        dialog_button(
            crate::t!("Help").into_owned(),
            Message::InsertAttrDialog(InsertAttrMsg::Help),
            false,
        ),
    ];
    container(
        column![
            container(body).padding([10, 8]),
            container(footer).padding([10, 12]).width(Fill)
        ]
        .spacing(8),
    )
    .style(card_style)
    .width(Fill)
    .into()
}
