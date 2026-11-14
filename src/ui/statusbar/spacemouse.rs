use super::status_menu::{self, Entry};
use crate::app::Message;
use crate::input::spacemouse::{NavigationMode, Preferences, Status};
use iced::widget::{button, row, text};
use iced::Element;

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
    use crate::ui::palette_menu::{entry, Item, Mark};
    let mut entries = vec![Entry::stay(entry(Item::Heading(format!(
        "SpaceMouse · {}",
        crate::t!(status.label())
    ))))];
    for mode in NavigationMode::ALL {
        entries.push(Entry::close(entry(
            Item::new(crate::t!(mode.label()), Message::SpaceMouseMode(mode))
                .mark(Mark::Radio(mode == preferences.mode)),
        )));
    }
    if sheet {
        entries.push(Entry::stay(entry(Item::Heading(
            crate::t!("Paper sheets keep rotation locked.").into_owned(),
        ))));
    }
    entries.push(Entry::stay(entry(Item::Separator)));
    entries.push(Entry::close(entry(Item::new(
        crate::t!(if paused {
            "Resume SpaceMouse"
        } else {
            "Pause SpaceMouse"
        }),
        Message::SpaceMousePause,
    ))));
    for (label, message) in [
        ("SpaceMouse preferences…", Message::SpaceMousePreferences),
        ("3Dconnexion settings…", Message::SpaceMouseDriverSettings),
    ] {
        entries.push(Entry::close(entry(Item::new(crate::t!(label), message))));
    }
    status_menu::menu_bar(root, entries, 260.)
}
