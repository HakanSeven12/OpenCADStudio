use codec::objects::BlockVisibilityParameter;
use codec::Handle;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Element, Fill};

use crate::app::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub document: u64,
    pub definition: Handle,
    pub parameter: Handle,
}

#[derive(Debug, Clone)]
pub struct RibbonContext {
    pub target: Target,
    pub names: Vec<String>,
    pub current: Option<String>,
    pub mode: bool,
}

pub use crate::scene::block_visibility::NewStateVisibility as NewVisibility;

#[derive(Debug, Clone)]
pub enum Action {
    Open(Target),
    Current(Target, String),
    Mode(Target),
    Select(usize),
    Name(String),
    NewVisibility(NewVisibility),
    New,
    Rename,
    Delete,
    SetCurrent,
    Move(bool),
    Apply,
    Help,
}

pub(crate) struct State {
    pub target: Target,
    pub original: BlockVisibilityParameter,
    pub edited: BlockVisibilityParameter,
    pub current: usize,
    pub selected: usize,
    pub name: String,
    pub new_visibility: NewVisibility,
    pub error: Option<String>,
    /// A dialog cannot overwrite changes made by another entry point.
    pub epoch: u64,
}

impl State {
    pub fn select(&mut self, index: usize) {
        if let Some(state) = self.edited.states.get(index) {
            self.selected = index;
            self.name = state.name.clone();
            self.error = None;
        }
    }

    pub fn name_error(&self, renaming: bool) -> Option<String> {
        if self.name.trim().is_empty() || self.name.chars().any(char::is_control) {
            return Some(
                crate::t!("Enter a nonempty state name without control characters.").into_owned(),
            );
        }
        self.edited
            .states
            .iter()
            .enumerate()
            .any(|(index, state)| {
                (!renaming || index != self.selected)
                    && state.name.eq_ignore_ascii_case(self.name.trim())
            })
            .then(|| crate::t!("A visibility state with that name already exists.").into_owned())
    }
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let send = |action| Message::BlockVisibility(action);
    let mut states = column![].spacing(2);
    for (index, item) in state.edited.states.iter().enumerate() {
        let label = if index == state.current {
            crate::tf!("{} (current)", item.name).into_owned()
        } else {
            item.name.clone()
        };
        states = states.push(
            button(text(label).size(13))
                .width(Fill)
                .on_press(send(Action::Select(index)))
                .style(if index == state.selected {
                    button::primary
                } else {
                    button::text
                }),
        );
    }
    let control = |label: &'static str, action, enabled: bool| {
        button(text(crate::t!(label).into_owned()).size(13))
            .width(Fill)
            .on_press_maybe(enabled.then_some(send(action)))
    };
    container(
        column![
            row![
                container(scrollable(states))
                    .width(Fill)
                    .height(Fill)
                    .style(container::bordered_box),
                column![
                    control(
                        "Set Current",
                        Action::SetCurrent,
                        state.selected != state.current
                    ),
                    control("Delete", Action::Delete, state.edited.states.len() > 1),
                    control("Move Up", Action::Move(true), state.selected > 0),
                    control(
                        "Move Down",
                        Action::Move(false),
                        state.selected + 1 < state.edited.states.len()
                    ),
                ]
                .spacing(8)
                .width(130),
            ]
            .spacing(12)
            .height(Fill),
            text(crate::t!("State name").into_owned()).size(13),
            row![
                text_input(crate::t!("State name").as_ref(), &state.name)
                    .on_input(move |name| send(Action::Name(name)))
                    .style(crate::ui::style::form::field_style),
                button(text(crate::t!("Rename").into_owned())).on_press_maybe(
                    state
                        .name_error(true)
                        .is_none()
                        .then_some(send(Action::Rename))
                ),
            ]
            .spacing(8),
            row![
                pick_list(
                    Some(state.new_visibility),
                    [
                        NewVisibility::Keep,
                        NewVisibility::Show,
                        NewVisibility::Hide
                    ],
                    |mode: &NewVisibility| mode.to_string()
                )
                .on_select(move |mode| send(Action::NewVisibility(mode)))
                .width(Fill),
                button(text(crate::t!("New").into_owned())).on_press_maybe(
                    state
                        .name_error(false)
                        .is_none()
                        .then_some(send(Action::New))
                ),
            ]
            .spacing(8),
            text(state.error.clone().unwrap_or_default())
                .size(12)
                .style(text::danger),
            row![
                Space::new().width(Fill),
                button(text(crate::t!("OK").into_owned()))
                    .on_press(send(Action::Apply))
                    .style(button::primary),
                button(text(crate::t!("Cancel").into_owned())).on_press(Message::CloseModal),
                button(text(crate::t!("Help").into_owned())).on_press(send(Action::Help)),
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
