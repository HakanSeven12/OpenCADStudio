//! DESIGN CENTER: browse the named content of an open or saved drawing
//! and add the picked items to the current drawing.

use crate::app::Message;
use crate::ui::style::form::hdivider;
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Element, Fill, Length, Theme};

/// The kinds of named content, as the Rename service names them.
pub(crate) const KINDS: &[&str] = &[
    "BLOCK",
    "DIMSTYLE",
    "LAYER",
    "LINETYPE",
    "MLEADERSTYLE",
    "STYLE",
];

#[derive(Debug, Clone)]
pub enum DesignCenterMsg {
    Source(usize),
    Kind(usize),
    Toggle(String),
    SelectAll,
    Browse,
    Picked(Result<std::path::PathBuf, String>),
    Add,
}

/// A drawing the content comes from.
pub(crate) enum Source {
    /// An open drawing, by its tab id.
    Tab(u64),
    /// A drawing read from disk.
    File(Box<document::Document>),
}

pub(crate) struct State {
    pub sources: Vec<(String, Source)>,
    pub source: usize,
    pub kind: usize,
    pub items: Vec<String>,
    pub picked: std::collections::BTreeSet<String>,
}

fn kind_label(kind: &str) -> String {
    match kind {
        "BLOCK" => crate::tr!("rename-dialog", "blocks"),
        "DIMSTYLE" => crate::tr!("rename-dialog", "dimension-styles"),
        "LAYER" => crate::tr!("rename-dialog", "layers"),
        "LINETYPE" => crate::tr!("rename-dialog", "linetypes"),
        "MLEADERSTYLE" => crate::tr!("design-center", "multileader-styles"),
        _ => crate::tr!("rename-dialog", "text-styles"),
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
    let row_button = |label: String, selected: bool, msg: DesignCenterMsg| -> Element<'_, Message> {
        button(text(label).size(12))
            .on_press(Message::DesignCenter(msg))
            .style(list_item_style(selected))
            .padding([4, 8])
            .width(Fill)
            .into()
    };
    let sources = state
        .sources
        .iter()
        .enumerate()
        .map(|(index, (label, _))| {
            row_button(
                label.clone(),
                index == state.source,
                DesignCenterMsg::Source(index),
            )
        })
        .collect();
    let kinds = KINDS
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            row_button(
                kind_label(kind),
                index == state.kind,
                DesignCenterMsg::Kind(index),
            )
        })
        .collect();
    let items = state
        .items
        .iter()
        .map(|name| {
            row_button(
                name.clone(),
                state.picked.contains(name),
                DesignCenterMsg::Toggle(name.clone()),
            )
        })
        .collect();
    let lists = row![
        column![
            text(crate::tr!("design-center", "drawings")).size(12),
            list(sources),
        ]
        .spacing(4)
        .width(Length::FillPortion(3)),
        column![
            text(crate::tr!("rename-dialog", "named-objects")).size(12),
            list(kinds),
        ]
        .spacing(4)
        .width(Length::FillPortion(2)),
        column![
            text(crate::tr!("rename-dialog", "items")).size(12),
            list(items)
        ]
        .spacing(4)
        .width(Length::FillPortion(3)),
    ]
    .spacing(12)
    .height(Fill);

    let send = |msg| Message::DesignCenter(msg);
    let buttons = row![
        button(text(crate::tr!("design-center", "browse")))
            .on_press(send(DesignCenterMsg::Browse)),
        button(text(crate::tr!("design-center", "select-all"))).on_press_maybe(
            (!state.items.is_empty()).then_some(send(DesignCenterMsg::SelectAll))
        ),
        Space::new().width(Fill),
        button(text(crate::tr!("design-center", "add")))
            .on_press_maybe((!state.picked.is_empty()).then_some(send(DesignCenterMsg::Add)))
            .style(button::primary),
        button(text(crate::t!("Close").into_owned())).on_press(Message::CloseModal),
    ]
    .spacing(8);

    container(column![lists, hdivider(sizing.width), buttons].spacing(10))
        .padding(16)
        .width(sizing.width)
        .height(sizing.height)
        .into()
}
