//! RENAME dialog: pick a kind of named object, pick one of its items, type
//! the new name.

use crate::app::Message;
use crate::ui::style::form::hdivider;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill, Length, Theme};

/// The kinds of named object, as the rename service names them.
pub(crate) const KINDS: &[&str] = &[
    "BLOCK", "DIMSTYLE", "LAYER", "LINETYPE", "STYLE", "UCS", "VIEW",
];

#[derive(Debug, Clone)]
pub enum RenameMsg {
    Kind(usize),
    Item(String),
    NewName(String),
    Rename,
    Ok,
}

pub(crate) struct State {
    pub kind: usize,
    pub items: Vec<String>,
    pub item: Option<String>,
    pub new_name: String,
}

fn kind_label(kind: &str) -> String {
    match kind {
        "BLOCK" => crate::tr!("rename-dialog", "blocks"),
        "DIMSTYLE" => crate::tr!("rename-dialog", "dimension-styles"),
        "LAYER" => crate::tr!("rename-dialog", "layers"),
        "LINETYPE" => crate::tr!("rename-dialog", "linetypes"),
        "STYLE" => crate::tr!("rename-dialog", "text-styles"),
        "UCS" => crate::tr!("rename-dialog", "ucss"),
        _ => crate::tr!("rename-dialog", "views"),
    }
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

fn list<'a>(rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(scrollable(column(rows).spacing(2)).height(Fill))
        .style(container::bordered_box)
        .padding(2)
        .height(Fill)
        .into()
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let row_button = |label: String, selected: bool, msg: RenameMsg| -> Element<'_, Message> {
        button(text(label).size(12))
            .on_press(Message::Rename(msg))
            .style(list_item_style(selected))
            .padding([4, 8])
            .width(Fill)
            .into()
    };
    let kinds = KINDS
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            row_button(kind_label(kind), index == state.kind, RenameMsg::Kind(index))
        })
        .collect();
    let items = state
        .items
        .iter()
        .map(|name| {
            row_button(
                name.clone(),
                state.item.as_deref() == Some(name.as_str()),
                RenameMsg::Item(name.clone()),
            )
        })
        .collect();
    let lists = row![
        column![
            text(crate::tr!("rename-dialog", "named-objects")).size(12),
            list(kinds),
        ]
        .spacing(4)
        .width(Length::FillPortion(2)),
        column![text(crate::tr!("rename-dialog", "items")).size(12), list(items)]
            .spacing(4)
            .width(Length::FillPortion(3)),
    ]
    .spacing(12)
    .height(Fill);

    let can_rename = state.item.is_some() && !state.new_name.trim().is_empty();
    let label = |key: String| text(key).size(12).width(Length::Fixed(90.0));
    let old_name = row![
        label(crate::tr!("rename-dialog", "old-name")),
        text_input("", state.item.as_deref().unwrap_or_default())
            .style(crate::ui::style::form::field_style),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let rename_to = row![
        button(text(crate::tr!("rename-dialog", "rename-to")).size(12))
            .on_press_maybe(can_rename.then_some(Message::Rename(RenameMsg::Rename)))
            .width(Length::Fixed(90.0)),
        text_input("", &state.new_name)
            .on_input(|name| Message::Rename(RenameMsg::NewName(name)))
            .on_submit_maybe(can_rename.then_some(Message::Rename(RenameMsg::Rename)))
            .style(crate::ui::style::form::field_style),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let buttons = row![
        Space::new().width(Fill),
        button(text(crate::t!("OK").into_owned()))
            .on_press(Message::Rename(RenameMsg::Ok))
            .style(button::primary),
        button(text(crate::t!("Cancel").into_owned())).on_press(Message::CloseModal),
    ]
    .spacing(8);

    container(column![lists, old_name, rename_to, hdivider(sizing.width), buttons].spacing(10))
        .padding(16)
        .width(sizing.width)
        .height(sizing.height)
        .into()
}
