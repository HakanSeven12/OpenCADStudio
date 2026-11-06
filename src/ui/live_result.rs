//! The live result of Quick Select and FILTER (#2611): the count large, a
//! bar for how much of the scope it is, and per-type / per-layer lines with
//! bars of their own.

use crate::app::Message;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};

/// The count, large, and what it is out of beside it.
pub fn big_count<'a>(count: usize, of: impl Into<String>) -> Element<'a, Message> {
    row![
        text(count.to_string())
            .size(30)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::DEFAULT
            })
            .line_height(1.0),
        column![Space::new().height(Fill), muted(of.into(), 12.0)].height(30),
    ]
    .spacing(8)
    .into()
}

/// The count, large, with two lines beside it (Quick Select).
pub fn big_count_two_lines<'a>(
    count: usize,
    first: impl Into<String>,
    second: impl Into<String>,
) -> Element<'a, Message> {
    row![
        text(count.to_string())
            .size(30)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::DEFAULT
            })
            .line_height(1.0),
        column![text(first.into()).size(12), muted(second.into(), 12.0)].spacing(1),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

/// A thin bar filled `part` of `whole`: the accent colour for the result,
/// muted for its lines.
pub fn track<'a>(part: usize, whole: usize, accent: bool) -> Element<'a, Message> {
    let fill = if whole == 0 {
        0
    } else {
        // At least a sliver for anything found.
        let share = (part as f64 / whole as f64 * 1000.0).round() as u16;
        if part > 0 {
            share.clamp(15, 1000)
        } else {
            0
        }
    };
    let mut bar = row![].height(6).width(Fill);
    if fill > 0 {
        bar = bar.push(
            container(Space::new())
                .width(Length::FillPortion(fill))
                .height(6)
                .style(move |theme: &Theme| {
                    let palette = theme.palette();
                    container::Style {
                        background: Some(Background::Color(if accent {
                            palette.primary.base.color
                        } else {
                            palette.background.base.text.scale_alpha(0.45)
                        })),
                        border: Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
        );
    }
    if fill < 1000 {
        bar = bar.push(Space::new().width(Length::FillPortion(1000 - fill)));
    }
    container(bar)
        .width(Fill)
        .height(6)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.strong.color)),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// A per-type or per-layer line: the name, its bar against the result, the
/// count; a click shows only these (`on` while it does).
pub fn bucket<'a>(
    label: String,
    count: usize,
    total: usize,
    on: bool,
    on_press: Message,
) -> Element<'a, Message> {
    button(
        row![
            container(
                text(label)
                    .size(11)
                    .wrapping(iced::widget::text::Wrapping::None)
            )
            .clip(true)
            .width(Length::Fixed(90.0)),
            track(count, total, false),
            text(count.to_string())
                .size(11)
                .width(Length::Fixed(40.0))
                .align_x(iced::alignment::Horizontal::Right),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .on_press(on_press)
    .padding([2, 4])
    .width(Fill)
    .style(move |theme: &Theme, status| {
        let palette = theme.palette();
        let background = if on {
            Some(palette.primary.weak.color)
        } else if matches!(status, button::Status::Hovered) {
            Some(palette.background.strong.color)
        } else {
            None
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: if on {
                palette.primary.weak.text
            } else {
                palette.background.base.text
            },
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

pub fn muted<'a>(label: impl text::IntoFragment<'a>, size: f32) -> iced::widget::Text<'a> {
    text(label).size(size).style(|theme: &Theme| text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
    })
}
