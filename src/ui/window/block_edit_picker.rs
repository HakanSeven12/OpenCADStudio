use crate::app::Message;
use crate::scene::WireModel;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{hdivider, vsep};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill, Length, Theme};

pub(crate) struct State {
    pub document_id: u64,
    pub name: String,
    pub existing_names: Vec<String>,
    pub preview: Vec<WireModel>,
    pub description: String,
    pub error: Option<String>,
}

fn list_item_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        if selected {
            button::primary(theme, status)
        } else {
            button::subtle(theme, status)
        }
    }
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let can_apply = state.error.is_none() && !state.name.trim().is_empty();

    // ── Left: Block name field and list of existing blocks ─────────────────────
    let name_input = text_input(crate::t!("Block name").as_ref(), &state.name)
        .on_input(Message::BlockEditPickerName)
        .on_submit_maybe(can_apply.then_some(Message::BlockEditPickerApply))
        .style(crate::ui::style::form::field_style);

    let list_items: Vec<Element<'_, Message>> = state
        .existing_names
        .iter()
        .map(|name| {
            let is_sel = name.eq_ignore_ascii_case(state.name.trim());
            button(text(name.clone()).size(12))
                .on_press(Message::BlockEditPickerSelect(name.clone()))
                .style(list_item_style(is_sel))
                .padding([4, 8])
                .width(Fill)
                .into()
        })
        .collect();

    let list_scrollable = container(scrollable(column(list_items).spacing(2)).height(Fill))
        .style(container::bordered_box)
        .padding(2)
        .height(Fill);

    let left_col = column![
        text(crate::t!("Block to create or edit").into_owned()).size(12),
        name_input,
        list_scrollable,
    ]
    .spacing(8)
    .width(Length::Fixed(240.0))
    .height(Fill);

    // ── Right: Preview and Description ─────────────────────────────────────────
    let preview_box = column![
        text(crate::t!("Preview").into_owned()).size(12),
        container(super::block_palette::preview_canvas(&state.preview, 150.0))
            .style(container::bordered_box)
            .width(Fill)
            .height(Length::Fixed(150.0)),
    ]
    .spacing(4);

    let desc_text = if state.description.is_empty() {
        crate::t!("(No description)").into_owned()
    } else {
        state.description.clone()
    };

    let desc_box = column![
        text(crate::t!("Description").into_owned()).size(12),
        container(scrollable(text(desc_text).size(11).style(muted_style)).height(Fill))
            .style(container::bordered_box)
            .padding(6)
            .width(Fill)
            .height(Fill),
    ]
    .spacing(4)
    .height(Fill);

    let right_col = column![preview_box, desc_box]
        .spacing(8)
        .width(Fill)
        .height(Fill);

    // ── Body ───────────────────────────────────────────────────────────────────
    let body = row![left_col, vsep(sizing.height), right_col]
        .spacing(12)
        .height(Fill);

    // ── Bottom: Error message and OK / Cancel buttons ───────────────────────────
    let error_text = text(state.error.clone().unwrap_or_default())
        .size(11)
        .style(text::danger);

    let buttons = row![
        error_text,
        Space::new().width(Fill),
        button(text(crate::t!("OK").into_owned()))
            .on_press_maybe(can_apply.then_some(Message::BlockEditPickerApply))
            .style(button::primary),
        button(text(crate::t!("Cancel").into_owned())).on_press(Message::CloseModal),
    ]
    .spacing(8)
    .align_y(iced::Center);

    container(column![body, hdivider(sizing.width), buttons].spacing(10))
        .padding(16)
        .width(sizing.width)
        .height(sizing.height)
        .into()
}
