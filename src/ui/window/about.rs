use crate::app::Message;
use crate::t;
use iced::widget::{button, column, container, row, svg, text, Space};
use iced::{Background, Border, Element, Fill, Length, Shrink, Theme};

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn surface_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(
            theme.palette().background.weakest.color,
        )),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

fn info_card<'a>(
    label: std::borrow::Cow<'static, str>,
    value: impl iced::widget::text::IntoFragment<'a>,
    width: Length,
) -> Element<'a, Message> {
    container(
        column![
            text(label).size(10).style(muted_style),
            text(value).size(14),
        ]
        .spacing(5),
    )
    .padding([10, 12])
    .width(width)
    .height(Length::Fixed(62.0))
    .style(surface_style)
    .into()
}

pub(crate) fn platform_name() -> &'static str {
    #[cfg(target_arch = "wasm32")]
    {
        "Web"
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::env::consts::OS {
            "linux" => "Linux",
            "windows" => "Windows",
            "macos" => "macOS",
            other => other,
        }
    }
}

pub(crate) fn architecture_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86-64",
        "aarch64" => "ARM64",
        "wasm32" => "WebAssembly 32-bit",
        other => other,
    }
}

pub fn view_window(
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let version = format!("v{}", env!("OCS_APP_VERSION"));
    let content_width = if matches!(sizing.width, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let card_width = if matches!(sizing.width, Length::Fill) {
        Length::FillPortion(1)
    } else {
        Length::Fixed(148.0)
    };
    let logo = svg(svg::Handle::from_memory(include_bytes!(
        "../../../assets/logo.svg"
    )))
    .width(Length::Fixed(72.0))
    .height(Length::Fixed(72.0));

    let hero = container(
        row![
            logo,
            column![
                text("Open CAD Studio").size(28).style(primary_style),
                text(t!("CAD application for Architecture & Engineering"))
                    .size(11)
                    .style(muted_style),
                text(version.clone()).size(13),
            ]
            .spacing(5),
        ]
        .spacing(18)
        .align_y(iced::Center),
    )
    .padding([14, 16])
    .width(content_width)
    .style(surface_style);

    let metadata = row![
        info_card(t!("Version"), version, card_width),
        info_card(t!("Platform"), platform_name(), card_width),
        info_card(t!("Arch"), architecture_name(), card_width),
    ]
    .spacing(8)
    .width(content_width);

    let copy = button(text(t!("Copy Info")).size(11))
        .on_press(Message::AboutCopyInfo)
        .style(button::primary)
        .padding([6, 16]);

    container(
        column![
            hero,
            metadata,
            row![Space::new().width(content_width), copy]
                .width(sizing.width)
                .align_y(iced::Center),
        ]
        .spacing(12)
        .padding(16)
        .width(sizing.width)
        .height(sizing.height),
    )
    .width(sizing.width)
    .height(sizing.height)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
        ..Default::default()
    })
    .into()
}
