use crate::app::Message;
use iced::widget::{button, column, container, row, text};
use iced::{Background, Element, Theme};

fn info_row<'a>(label: &'static str, value: String) -> Element<'a, Message> {
    row![
        text(label)
            .size(11)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
            })
            .width(100),
        text(value).size(11),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .padding([3, 0])
    .into()
}

pub fn view_window<'a>() -> Element<'a, Message> {
    let version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let logo = container(
        column![
            text("Open CAD Studio")
                .size(32)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.extended_palette().primary.base.color),
                }),
            text("CAD application for Architecture & Engineering")
                .size(11)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
                }),
        ]
        .spacing(4)
        .align_x(iced::Center),
    )
    .padding(iced::Padding {
        top: 20.0,
        right: 0.0,
        bottom: 16.0,
        left: 0.0,
    })
    .align_x(iced::Center);

    let info_block = container(
        column![
            info_row("Version", format!("v{}", version)),
            info_row("Platform", os.to_string()),
            info_row("Arch", arch.to_string()),
        ]
        .spacing(2)
        .padding([12, 16]),
    )
    .style(container::bordered_box);

    let copy_btn = button(text("Copy Info").size(11))
        .on_press(Message::AboutCopyInfo)
        .style(button::primary)
        .padding([6, 16]);

    let footer = row![copy_btn]
        .align_y(iced::Center)
        .padding(iced::Padding {
            top: 12.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        });

    container(
        column![logo, info_block, footer]
            .spacing(0)
            .padding(iced::Padding {
                top: 0.0,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            }),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.base.color,
        )),
        ..Default::default()
    })
    .into()
}
