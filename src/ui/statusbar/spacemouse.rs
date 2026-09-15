use super::status_menu::{self, Entry};
use crate::app::Message;
use crate::input::spacemouse::{NavigationMode, Preferences, Status};
use iced::widget::{button, row, text};
use iced::{Element, Fill};

pub(crate) fn view(
    preferences: Preferences,
    status: Status,
    paused: bool,
    label: String,
    sheet: bool,
) -> Element<'static, Message> {
    let root = button(
        row![
            crate::ui::icons::themed(crate::ui::window::options::spacemouse::ICON, 15.),
            text(label).size(11),
            crate::ui::icons::themed_arrow_toggle(false, 8.)
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .padding([3, 6])
    .style(button::subtle);
    let mut entries = vec![Entry::stay(
        text(format!("SpaceMouse · {}", crate::t!(status.label()))).size(11),
    )];
    for mode in NavigationMode::ALL {
        let label = format!(
            "{}  {}",
            if mode == preferences.mode {
                "●"
            } else {
                "○"
            },
            crate::t!(mode.label())
        );
        entries.push(Entry::close(
            button(text(label).size(12))
                .width(Fill)
                .padding([7, 10])
                .style(button::text)
                .on_press(Message::SpaceMouseMode(mode)),
        ));
    }
    if sheet {
        entries.push(Entry::stay(
            text(crate::t!("Paper sheets keep rotation locked.")).size(11),
        ));
    }
    entries.push(Entry::close(
        button(
            text(crate::t!(if paused {
                "Resume SpaceMouse"
            } else {
                "Pause SpaceMouse"
            }))
            .size(12),
        )
        .on_press(Message::SpaceMousePause)
        .style(button::text)
        .width(Fill)
        .padding([7, 10]),
    ));
    for (label, message) in [
        ("SpaceMouse preferences…", Message::SpaceMousePreferences),
        ("3Dconnexion settings…", Message::SpaceMouseDriverSettings),
    ] {
        entries.push(Entry::close(
            button(text(crate::t!(label)).size(12))
                .on_press(message)
                .style(button::text)
                .width(Fill)
                .padding([7, 10]),
        ));
    }
    status_menu::menu_bar(root, entries, 260.)
}
