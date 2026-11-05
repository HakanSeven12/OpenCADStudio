//! Customize User Interface (CUI): the user's own ribbon panels, shown on
//! the My Tools tab, and the way to the keyboard shortcuts. Panels are made,
//! named, ordered and deleted on the left; the selected panel's commands are
//! added (by name or alias), ordered and removed on the right. Every change
//! shows on the ribbon at once.

use crate::app::Message;
use crate::ui::ribbon::CustomPanel;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::dialog_button;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Length};

#[derive(Debug, Clone, Default)]
pub struct State {
    pub selected: Option<usize>,
    /// The command being typed for the selected panel.
    pub draft: String,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Select(usize),
    NewPanel,
    DeletePanel,
    /// Move the selected panel up (`true`) or down.
    MovePanel(bool),
    Title(String),
    Draft(String),
    AddCommand,
    RemoveCommand(usize),
    MoveCommand(usize, bool),
    Shortcuts,
}

fn msg(m: Msg) -> Message {
    Message::RibbonCustomize(m)
}

fn small<'a>(glyph: &'static str, m: Msg) -> Element<'a, Message> {
    button(text(glyph).size(11))
        .on_press(msg(m))
        .padding([1, 6])
        .style(button::text)
        .into()
}

pub fn view_window<'a>(
    state: &State,
    panels: &[CustomPanel],
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let mut list = column![].spacing(2);
    for (index, panel) in panels.iter().enumerate() {
        let selected = state.selected == Some(index);
        list = list.push(
            button(text(panel.title.clone()).size(12))
                .on_press(msg(Msg::Select(index)))
                .width(Length::Fill)
                .padding([3, 6])
                .style(if selected { button::primary } else { button::text }),
        );
    }
    let left = column![
        text(crate::tr!("ribbon-customize", "panels")).size(11).style(muted_style),
        container(scrollable(list)).height(Length::Fixed(220.0)),
        row![
            dialog_button(crate::tr!("ribbon-customize", "new-panel"), msg(Msg::NewPanel), false),
            small("↑", Msg::MovePanel(true)),
            small("↓", Msg::MovePanel(false)),
            small("×", Msg::DeletePanel),
        ]
        .spacing(4)
        .align_y(iced::Center),
    ]
    .spacing(6)
    .width(Length::Fixed(200.0));

    let right: Element<'a, Message> = match state.selected.and_then(|index| panels.get(index)) {
        None => text(crate::tr!("ribbon-customize", "pick-panel"))
            .size(12)
            .style(muted_style)
            .into(),
        Some(panel) => {
            let mut commands = column![].spacing(2);
            for (index, command) in panel.commands.iter().enumerate() {
                commands = commands.push(
                    row![
                        text(command.to_string()).size(12).width(Length::Fill),
                        small("↑", Msg::MoveCommand(index, true)),
                        small("↓", Msg::MoveCommand(index, false)),
                        small("×", Msg::RemoveCommand(index)),
                    ]
                    .align_y(iced::Center),
                );
            }
            column![
                text(crate::tr!("ribbon-customize", "title")).size(11).style(muted_style),
                text_input("", &panel.title)
                    .on_input(|title| msg(Msg::Title(title)))
                    .size(12)
                    .padding([3, 6]),
                text(crate::tr!("ribbon-customize", "commands")).size(11).style(muted_style),
                container(scrollable(commands)).height(Length::Fixed(170.0)),
                row![
                    text_input(&crate::tr!("ribbon-customize", "command-placeholder"), &state.draft)
                        .on_input(|typed| msg(Msg::Draft(typed)))
                        .on_submit(msg(Msg::AddCommand))
                        .size(12)
                        .padding([3, 6])
                        .width(Length::Fill),
                    dialog_button(crate::tr!("ribbon-customize", "add"), msg(Msg::AddCommand), false),
                ]
                .spacing(4),
            ]
            .spacing(6)
            .into()
        }
    };

    column![
        text(crate::tr!("ribbon-customize", "hint")).size(11).style(muted_style),
        row![left, container(right).width(sizing.width)].spacing(14),
        row![
            dialog_button(crate::tr!("ribbon-customize", "shortcuts"), msg(Msg::Shortcuts), false),
            Space::new().width(Length::Fill),
            dialog_button(crate::tr!("ribbon-customize", "close"), Message::CloseModal, true),
        ]
        .spacing(6),
    ]
    .spacing(10)
    .padding(12)
    .into()
}
