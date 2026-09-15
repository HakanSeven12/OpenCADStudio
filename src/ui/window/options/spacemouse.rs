use crate::app::Message;
use crate::input::spacemouse::{NavigationMode, Preferences, Status};
use iced::widget::{button, checkbox, column, container, pick_list, row, slider, text, Space};
use iced::{Element, Fill};

pub(crate) const ICON: &[u8] = include_bytes!("../../../../assets/icons/ui/spacemouse.svg");

pub(crate) fn view(
    preferences: Preferences,
    status: Status,
    paused: bool,
    details: bool,
) -> Element<'static, Message> {
    let supported = cfg!(windows);
    let mut enabled = checkbox(preferences.enabled).size(15);
    if supported {
        enabled = enabled.on_toggle(Message::SpaceMouseEnabled);
    }
    let mode = pick_list(Some(preferences.mode), NavigationMode::ALL, |mode| {
        crate::t!(mode.label()).into_owned()
    })
    .width(220);
    let mode = if supported {
        mode.on_select(Message::SpaceMouseMode)
    } else {
        mode
    };
    let status_text = if !preferences.enabled {
        crate::t!("Disabled").into_owned()
    } else if paused {
        crate::t!("Paused").into_owned()
    } else {
        crate::t!(status.label()).into_owned()
    };
    let mut content = column![
        row![
            crate::ui::icons::themed(ICON, 22.),
            text(crate::t!("SpaceMouse")).size(15)
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![enabled, text(crate::t!("Enable SpaceMouse")).size(12)]
            .spacing(8)
            .align_y(iced::Center),
        row![
            text(crate::t!("Status")).size(12).width(150),
            text(status_text).size(12)
        ]
        .spacing(10),
        row![text(crate::t!("Navigation")).size(12).width(150), mode]
            .spacing(10)
            .align_y(iced::Center),
        text(crate::t!(preferences.mode.description()).into_owned())
            .size(11)
            .width(Fill),
    ]
    .spacing(12);
    if preferences.mode != NavigationMode::Full3D && supported {
        content = content.push(
            row![
                checkbox(preferences.pan_reversed)
                    .size(15)
                    .on_toggle(Message::SpaceMousePanReversed),
                text(crate::t!("Reverse pan direction")).size(12),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
        content = content.push(
            row![
                text(crate::t!("Pan speed")).size(12).width(150),
                slider(
                    10..=300,
                    preferences.pan_speed.clamp(10, 300),
                    Message::SpaceMousePanSpeed
                )
                .step(10)
                .width(160),
                text(format!("{}%", preferences.pan_speed.clamp(10, 300))).size(11),
            ]
            .spacing(10)
            .align_y(iced::Center),
        );
    }
    if paused {
        content = content.push(
            button(text(crate::t!("Resume SpaceMouse")).size(11))
                .on_press(Message::SpaceMousePause)
                .style(button::secondary),
        );
    }
    content = content.push(
        button(
            text(crate::t!(if supported {
                "Open 3Dconnexion settings…"
            } else {
                "SpaceMouse information…"
            }))
            .size(11),
        )
        .on_press(Message::SpaceMouseDriverSettings)
        .style(button::secondary)
        .padding([5, 10]),
    );
    content = content.push(
        text(crate::t!(
            "Configure 3D navigation, buttons, and radial menus in 3Dconnexion settings. Pan only and Pan and zoom share the push/tilt mapping and pan settings shown here."
        ))
        .size(11)
        .width(Fill),
    );
    content = content.push(
        button(
            row![
                crate::ui::icons::themed_arrow_toggle(details, 9.),
                text(crate::t!("Connection details")).size(11)
            ]
            .spacing(6),
        )
        .on_press(Message::SpaceMouseDetails)
        .style(button::text),
    );
    if details {
        let detail = match status {
            Status::Unavailable(reason) => reason,
            Status::Unsupported => "This version supports SpaceMouse through 3DxWare on Windows. Other platforms do not yet have a device adapter.".into(),
            Status::Disconnected => "3DxWare is running. Connect a SpaceMouse; Open CAD Studio reconnects automatically.".into(),
            _ => "Uses the installed 3DxWare driver. Navigation pauses while a dialog is open. Your selected navigation mode is saved across drawings and sessions.".into(),
        };
        content = content.push(text(crate::t!(&detail).into_owned()).size(11).width(Fill));
    }
    container(content.push(Space::new().height(8)))
        .width(Fill)
        .into()
}
