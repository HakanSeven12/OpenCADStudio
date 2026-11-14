//! Geographic Location dialog: set the root drawing CRS stored in GEODATA.

use crate::app::Message;
use crate::ui::style::form::{dialog_button, field_style};
use iced::widget::{column, container, row, text, text_input, Space};
use iced::{Element, Fill};

#[derive(Debug, Clone)]
pub enum GeographicLocationMsg {
    Definition(String),
    Apply,
}

pub(crate) struct State {
    pub definition: String,
    pub error: Option<String>,
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let hint = crate::tr!("geographic-location-dialog", "hint");
    let error: Element<'_, Message> = match &state.error {
        Some(message) => text(message.as_str()).size(12).style(text::danger).into(),
        None => Space::new().height(0).into(),
    };
    let buttons = row![
        Space::new().width(Fill),
        dialog_button(
            crate::tr!("geographic-location-dialog", "ok"),
            Message::GeographicLocation(GeographicLocationMsg::Apply),
            true,
        ),
        dialog_button(
            crate::tr!("geographic-location-dialog", "cancel"),
            Message::CloseModal,
            false,
        ),
    ]
    .spacing(8);

    container(
        column![
            text(crate::tr!("geographic-location-dialog", "description"))
                .size(12)
                .width(Fill),
            text(crate::tr!("geographic-location-dialog", "definition-label")).size(12),
            text_input(&hint, &state.definition)
                .on_input(|value| {
                    Message::GeographicLocation(GeographicLocationMsg::Definition(value))
                })
                .on_submit(Message::GeographicLocation(GeographicLocationMsg::Apply,))
                .style(field_style)
                .size(12)
                .width(Fill),
            error,
            Space::new().height(Fill),
            buttons,
        ]
        .spacing(10),
    )
    .padding(16)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
