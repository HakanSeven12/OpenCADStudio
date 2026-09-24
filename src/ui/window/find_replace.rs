use crate::app::Message;
use crate::t;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt};
use iced::widget::{button, column, row, text, text_input, Space};
use iced::{Element, Fill, Length, Shrink};

pub const FIND_INPUT_ID: &str = "find-replace-search";

pub fn view_window<'a>(
    search: &'a str,
    replacement: &'a str,
    status: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let field_width = if matches!(sizing.width, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let find_input = text_input("", search)
        .id(iced::widget::Id::new(FIND_INPUT_ID))
        .on_input(Message::FindReplaceSearchChanged)
        .on_submit(Message::FindReplaceNext)
        .padding([6, 8])
        .size(13);
    let replacement_input = text_input("", replacement)
        .on_input(Message::FindReplaceReplacementChanged)
        .padding([6, 8])
        .size(13);

    let enabled = !search.trim().is_empty();

    column![
        row![
            text(t!("Find:")).size(12).width(90),
            find_input.width(field_width),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text(t!("Replace with:")).size(12).width(90),
            replacement_input.width(field_width),
        ]
        .spacing(8)
        .align_y(iced::Center),
        text(t!("Searches Text, MText, Attribute Definitions, and block attribute values."))
            .size(11),
        text(status).size(11),
        row![
            Space::new().width(field_width),
            dialog_button(t!("Close"), Message::CloseModal, false),
            dialog_button_styled_opt(
                t!("Replace"),
                enabled.then_some(Message::FindReplaceOne),
                button::secondary,
            ),
            dialog_button_styled_opt(
                t!("Replace All"),
                enabled.then_some(Message::FindReplaceAll),
                button::danger,
            ),
            dialog_button_styled_opt(
                t!("Find Next"),
                enabled.then_some(Message::FindReplaceNext),
                button::primary,
            ),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(10)
    .padding(12)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
