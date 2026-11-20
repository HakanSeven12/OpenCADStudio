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
    /// A suggestion row was clicked: its command goes through `AddCommand`.
    PickSuggestion(String),
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

/// Most suggestion rows shown under the add box.
const SUGGESTION_ROWS: usize = 6;

/// Height of the block that holds a panel's commands, the add row and the
/// suggestion rows. It is fixed: the dialog's window is sized once, before
/// anything is typed, so rows that made the content taller would be cut off.
/// They take their room from the command list instead, which scrolls.
const COMMANDS_BLOCK_HEIGHT: f32 = 240.0;

/// Command names to offer for the typed text: the command line's own
/// autocomplete ranking (prefix matches first, then substring matches; an
/// alias offers the command it stands for), cut to a few rows.
pub fn suggestions(
    typed: &str,
    dynamic: &[String],
    aliases: &rustc_hash::FxHashMap<String, String>,
) -> Vec<String> {
    let mut found = crate::ui::command_line::ranked_matches(typed, dynamic, aliases);
    found.truncate(SUGGESTION_ROWS);
    found
}

fn suggestion_list<'a>(suggestions: &[String]) -> Element<'a, Message> {
    let mut list = column![].spacing(1);
    for name in suggestions {
        list = list.push(
            button(text(name.clone()).size(12))
                .on_press(msg(Msg::PickSuggestion(name.clone())))
                .width(Length::Fill)
                .padding([2, 6])
                .style(button::text),
        );
    }
    list.into()
}

pub fn view_window<'a>(
    state: &State,
    suggestions: &[String],
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
                .style(if selected {
                    button::primary
                } else {
                    button::text
                }),
        );
    }
    let left = column![
        text(crate::tr!("ribbon-customize", "panels"))
            .size(11)
            .style(muted_style),
        container(scrollable(list)).height(Length::Fixed(220.0)),
        row![
            dialog_button(
                crate::tr!("ribbon-customize", "new-panel"),
                msg(Msg::NewPanel),
                false
            ),
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
            let mut commands_block = column![
                scrollable(commands).height(Length::Fill),
                row![
                    text_input(
                        &crate::tr!("ribbon-customize", "command-placeholder"),
                        &state.draft
                    )
                    .on_input(|typed| msg(Msg::Draft(typed)))
                    .on_submit(msg(Msg::AddCommand))
                    .size(12)
                    .padding([3, 6])
                    .width(Length::Fill),
                    dialog_button(
                        crate::tr!("ribbon-customize", "add"),
                        msg(Msg::AddCommand),
                        false
                    ),
                ]
                .spacing(4),
            ]
            .spacing(6);
            if !suggestions.is_empty() {
                commands_block = commands_block.push(suggestion_list(suggestions));
            }
            column![
                text(crate::tr!("ribbon-customize", "title"))
                    .size(11)
                    .style(muted_style),
                text_input("", &panel.title)
                    .on_input(|title| msg(Msg::Title(title)))
                    .size(12)
                    .padding([3, 6]),
                text(crate::tr!("ribbon-customize", "commands"))
                    .size(11)
                    .style(muted_style),
                container(commands_block).height(Length::Fixed(COMMANDS_BLOCK_HEIGHT)),
            ]
            .spacing(6)
            .into()
        }
    };

    column![
        text(crate::tr!("ribbon-customize", "hint"))
            .size(11)
            .style(muted_style),
        row![left, container(right).width(sizing.width)].spacing(14),
        row![
            dialog_button(
                crate::tr!("ribbon-customize", "shortcuts"),
                msg(Msg::Shortcuts),
                false
            ),
            Space::new().width(Length::Fill),
            dialog_button(
                crate::tr!("ribbon-customize", "close"),
                Message::CloseModal,
                true
            ),
        ]
        .spacing(6),
    ]
    .spacing(10)
    .padding(12)
    .into()
}
