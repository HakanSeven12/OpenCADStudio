//! Purge dialog: what PURGE can remove, by kind, to pick from.

use crate::app::Message;
use crate::ui::style::form::hdivider;
use iced::widget::{button, checkbox, column, container, row, scrollable, text, Space};
use iced::{Element, Fill};

/// Kinds in the order `purge_pass` reports them.
pub(crate) const KINDS: usize = 4;

#[derive(Debug, Clone)]
pub enum PurgeMsg {
    /// Tick or untick one item: kind, index.
    Item(usize, usize),
    /// Tick or untick every item of a kind.
    Kind(usize),
    Nested(bool),
    PurgeChecked,
    PurgeAll,
}

pub(crate) struct State {
    /// Per kind (layers, text styles, linetypes, blocks): name, ticked.
    pub items: [Vec<(String, bool)>; KINDS],
    pub nested: bool,
}

fn kind_label(kind: usize) -> String {
    match kind {
        0 => crate::tr!("purge-dialog", "layers"),
        1 => crate::tr!("purge-dialog", "text-styles"),
        2 => crate::tr!("purge-dialog", "linetypes"),
        _ => crate::tr!("purge-dialog", "blocks"),
    }
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let mut list = column![].spacing(4);
    let mut any = false;
    // Blocks first, as removing them frees what only they used.
    for kind in [3, 0, 2, 1] {
        let items = &state.items[kind];
        if items.is_empty() {
            continue;
        }
        any = true;
        let all = items.iter().all(|(_, ticked)| *ticked);
        list = list.push(
            checkbox(all)
                .label(format!("{} ({})", kind_label(kind), items.len()))
                .text_size(12)
                .on_toggle(move |_| Message::Purge(PurgeMsg::Kind(kind))),
        );
        for (index, (name, ticked)) in items.iter().enumerate() {
            list = list.push(
                row![
                    Space::new().width(22),
                    checkbox(*ticked)
                        .label(name.clone())
                        .text_size(11)
                        .on_toggle(move |_| Message::Purge(PurgeMsg::Item(kind, index))),
                ]
                .align_y(iced::Center),
            );
        }
    }
    let body: Element<'_, Message> = if any {
        container(scrollable(list).height(Fill))
            .style(container::bordered_box)
            .padding(8)
            .height(Fill)
            .width(Fill)
            .into()
    } else {
        container(text(crate::tr!("purge-dialog", "nothing")).size(12))
            .padding(8)
            .height(Fill)
            .into()
    };
    let checked = state.items.iter().flatten().any(|(_, ticked)| *ticked);
    let buttons = row![
        checkbox(state.nested)
            .label(crate::tr!("purge-dialog", "nested"))
            .text_size(12)
            .on_toggle(|nested| Message::Purge(PurgeMsg::Nested(nested))),
        Space::new().width(Fill),
        button(text(crate::tr!("purge-dialog", "purge-checked")).size(12))
            .on_press_maybe(checked.then_some(Message::Purge(PurgeMsg::PurgeChecked))),
        button(text(crate::tr!("purge-dialog", "purge-all")).size(12))
            .on_press_maybe(any.then_some(Message::Purge(PurgeMsg::PurgeAll)))
            .style(button::primary),
        button(text(crate::t!("Close").into_owned()).size(12)).on_press(Message::CloseModal),
    ]
    .spacing(8)
    .align_y(iced::Center);
    container(column![body, hdivider(sizing.width), buttons].spacing(10))
        .padding(16)
        .width(sizing.width)
        .height(sizing.height)
        .into()
}
