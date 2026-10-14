use crate::app::Message;
use crate::scene::WireModel;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Space};
use iced::{Element, Fill};

pub(crate) struct State {
    pub document_id: u64,
    pub source: codec::Handle,
    pub name: String,
    pub existing_names: Vec<String>,
    pub preview: Vec<WireModel>,
    pub error: Option<String>,
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let field = text_input(crate::t!("Block name").as_ref(), &state.name)
        .on_input(Message::BlockSaveAsName)
        .on_submit_maybe(state.error.is_none().then_some(Message::BlockSaveAsApply))
        .style(crate::ui::style::form::field_style);
    let names = pick_list(
        None::<String>,
        state.existing_names.as_slice(),
        |name: &String| name.clone(),
    )
    .on_select(Message::BlockSaveAsName)
    .placeholder(crate::t!("Existing blocks").into_owned())
    .width(Fill);
    container(
        column![
            text(crate::t!("Block name").into_owned()).size(13),
            field,
            names,
            super::block_palette::preview_canvas(&state.preview, 160.0),
            text(state.error.clone().unwrap_or_default())
                .size(12)
                .style(text::danger),
            row![
                Space::new().width(Fill),
                button(text(crate::t!("OK").into_owned()))
                    .on_press_maybe(state.error.is_none().then_some(Message::BlockSaveAsApply))
                    .style(button::primary),
                button(text(crate::t!("Cancel").into_owned())).on_press(Message::CloseModal),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .padding(16)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
