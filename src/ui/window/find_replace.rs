use crate::app::Message;
use crate::t;
use iced::widget::{button, column, row, text, text_input, Space};
use iced::{Element, Fill, Length, Shrink};
use std::borrow::Cow;

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
    let action = |label: Cow<'static, str>, message: Message| {
        let button = button(text(label).size(12)).padding([6, 12]);
        if enabled {
            button.on_press(message)
        } else {
            button
        }
    };

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
        text(t!(
            "Searches Text, MText, Attribute Definitions, and block attribute values."
        ))
        .size(11),
        text(status).size(11),
        row![
            Space::new().width(field_width),
            button(text(t!("Close")).size(12))
                .on_press(Message::CloseModal)
                .padding([6, 12])
                .style(button::secondary),
            action(t!("Replace"), Message::FindReplaceOne).style(button::secondary),
            action(t!("Replace All"), Message::FindReplaceAll).style(button::danger),
            action(t!("Find Next"), Message::FindReplaceNext).style(button::primary),
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
