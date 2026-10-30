//! The window an open, save, plot or publish raises (DXEVAL) when
//! an extraction table no longer shows the drawing's data: update it, all of
//! them, or leave it / all of them as they are.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Border, Element, Fill, Theme};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::dialog_button_styled_opt;

const AMBER: iced::Color = iced::Color::from_rgb8(0xd9, 0xa4, 0x00);
const EXTRACT_ICON: &[u8] = include_bytes!("../../../assets/icons/data_extract.svg");

/// What the window answers for the table it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutdatedChoice {
    Update,
    UpdateAll,
    Skip,
    SkipAll,
}

/// `settings` is the extraction's settings file; `position`/`count` place
/// this table among the outdated ones.
pub fn view<'a>(
    settings: &'a str,
    position: usize,
    count: usize,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let name = std::path::Path::new(settings)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| settings.to_string());
    let badge = container(
        iced::widget::svg(crate::ui::icons::themed_handle(crate::ui::icons::WARNING))
            .width(18.0)
            .height(18.0)
            .style(|_: &Theme, _| iced::widget::svg::Style { color: Some(AMBER) }),
    )
    .width(34)
    .height(34)
    .center_x(34)
    .center_y(34)
    .style(|_: &Theme| container::Style {
        background: Some(Background::Color(AMBER.scale_alpha(0.15))),
        border: Border {
            radius: 17.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let chip = container(
        row![
            crate::ui::icons::themed_secondary(EXTRACT_ICON, 12.0),
            text(name).size(11)
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .padding([2, 8])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        border: Border {
            width: 1.0,
            radius: 10.0.into(),
            color: theme.palette().background.strong.color,
        },
        ..Default::default()
    });
    let mut heading = row![
        text(crate::tr!("extraction-outdated", "heading"))
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::DEFAULT
            }),
        Space::new().width(Fill),
    ];
    if count > 1 {
        heading = heading.push(
            text(crate::tr!(
                "extraction-outdated",
                "counter",
                position = position.to_string(),
                count = count.to_string()
            ))
            .size(11)
            .style(muted_style),
        );
    }
    let choice = |label: String, choice: OutdatedChoice, primary: bool| {
        dialog_button_styled_opt(
            std::borrow::Cow::Owned(label),
            Some(Message::ExtractionOutdated(choice)),
            if primary {
                button::primary
            } else {
                button::secondary
            },
        )
    };
    let many = count > 1;
    let mut actions = row![choice(
        crate::tr!("extraction-outdated", "skip"),
        OutdatedChoice::Skip,
        false
    )]
    .spacing(8);
    if many {
        actions = actions.push(choice(
            crate::tr!("extraction-outdated", "skip-all"),
            OutdatedChoice::SkipAll,
            false,
        ));
    }
    actions = actions.push(Space::new().width(Fill));
    if many {
        actions = actions.push(choice(
            crate::tr!("extraction-outdated", "update-all"),
            OutdatedChoice::UpdateAll,
            false,
        ));
    }
    actions = actions.push(choice(
        crate::tr!("extraction-outdated", "update"),
        OutdatedChoice::Update,
        true,
    ));
    column![
        row![
            badge,
            column![
                heading,
                chip,
                text(settings).size(10).style(muted_style),
                text(crate::tr!("extraction-outdated", "note"))
                    .size(11)
                    .style(muted_style),
            ]
            .spacing(8),
        ]
        .spacing(12),
        iced::widget::rule::horizontal(1),
        actions,
    ]
    .spacing(12)
    .padding([12, 14])
    .width(sizing.width)
    .into()
}
