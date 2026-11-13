//! The insertion-time `Edit Attributes` form that `ATTDIA` puts in front of the
//! command-line prompt chain: the block being placed and one answered row per
//! askable attribute, fifteen to a page.

use crate::app::Message;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::card_style;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
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
    /// The first row the page shows.
    pub first: usize,
}

impl InsertAttrState {
    /// One step forward; the last page always shows a full page, so its first
    /// row is the fifteenth from the end.
    pub fn next_page(&mut self) {
        self.first = (self.first + PAGE).min(self.rows.len().saturating_sub(PAGE));
    }

    pub fn previous_page(&mut self) {
        self.first = self.first.saturating_sub(PAGE);
    }

    fn has_previous(&self) -> bool {
        self.first > 0
    }

    fn has_next(&self) -> bool {
        self.first + PAGE < self.rows.len()
    }
}

#[derive(Debug, Clone)]
pub enum InsertAttrMsg {
    Value(usize, String),
    Previous,
    Next,
    Ok,
    Help,
}

/// Rows a page shows; Previous and Next step through longer lists.
pub const PAGE: usize = 15;

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

pub fn view(
    state: &InsertAttrState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'_, Message> {
    let total = state.rows.len();
    let last = (state.first + PAGE).min(total);
    let mut name = row![text(state.block_name.clone()).size(12)];
    if total > PAGE {
        name = name.push(Space::new().width(Fill)).push(
            text(format!("{}–{} / {}", state.first + 1, last, total))
                .size(11)
                .style(crate::ui::style::readable::muted_style),
        );
    }
    let mut body = column![line(caption(&crate::t!("Block name")), name.into())].spacing(6);
    for (index, row) in state.rows.iter().enumerate().take(last).skip(state.first) {
        body = body.push(line(caption(&row.label), slot(&row.value, index)));
    }
    let step = |label: &str, go: bool, msg: InsertAttrMsg| {
        dialog_button_styled_opt(
            label.to_owned(),
            go.then_some(Message::InsertAttrDialog(msg)),
            button::secondary,
        )
    };
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
        step(&crate::t!("Previous"), state.has_previous(), InsertAttrMsg::Previous),
        spread(),
        step(&crate::t!("Next"), state.has_next(), InsertAttrMsg::Next),
        spread(),
        dialog_button(
            crate::t!("Help").into_owned(),
            Message::InsertAttrDialog(InsertAttrMsg::Help),
            false,
        ),
    ];
    // The rows scroll inside the frame so the footer is never pushed out of it.
    container(
        column![
            scrollable(container(body).padding([10, 8])).height(sizing.height),
            container(footer).padding([10, 12]).width(Fill)
        ]
        .spacing(8)
        .height(sizing.height),
    )
    .style(card_style)
    .width(Fill)
    .height(sizing.height)
    .into()
}
