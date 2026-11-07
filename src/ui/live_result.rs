//! The live result of Quick Select and FILTER (#2611): the count large, a
//! bar for how much of the scope it is, and per-type / per-layer lines with
//! bars of their own.

use crate::app::Message;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};

/// The count, large, and what it is out of beside it.
pub fn big_count<'a>(count: usize, of: impl Into<String>) -> Element<'a, Message> {
    row![
        text(fmt_count(count))
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
        text(fmt_count(count))
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
                text(crate::ui::text_util::elide(&label, 14))
                    .size(11)
                    .wrapping(iced::widget::text::Wrapping::None)
            )
            .clip(true)
            .width(Length::Fixed(96.0)),
            track(count, total, false),
            text(fmt_count(count))
                .size(11)
                .width(Length::Fixed(44.0))
                .align_x(iced::alignment::Horizontal::Right),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .on_press(on_press)
    .padding([3, 4])
    .height(22)
    .width(Fill)
    .style(move |theme: &Theme, status| {
        let palette = theme.palette();
        let background = if on {
            Some(quiet_selection(theme))
        } else if matches!(status, button::Status::Hovered) {
            Some(palette.background.strong.color)
        } else {
            None
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: palette.background.base.text,
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

/// The selection colour of the lists: the accent mixed into the
/// background, quiet in a dark theme as in the mockups.
pub fn quiet_selection(theme: &Theme) -> iced::Color {
    let palette = theme.palette();
    let bg = palette.background.base.color;
    let accent = palette.primary.base.color;
    let dark = bg.r * 0.299 + bg.g * 0.587 + bg.b * 0.114 < 0.5;
    let t = if dark { 0.32 } else { 0.18 };
    let mix = |a: f32, b: f32| a + (b - a) * t;
    iced::Color::from_rgb(
        mix(bg.r, accent.r),
        mix(bg.g, accent.g),
        mix(bg.b, accent.b),
    )
}

/// A count with its thousands apart, as the mockup writes them: 3 997.
pub fn fmt_count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    out
}

/// The "Show" switch of a live result: on, it has the selection colour and
/// an accent border (mockup), off it is a plain button.
pub fn show_toggle<'a>(on: bool, label: String, on_press: Message) -> Element<'a, Message> {
    button(
        row![
            crate::ui::icons::themed(
                if on {
                    crate::ui::icons::EYE
                } else {
                    crate::ui::icons::EYE_OFF
                },
                12.0
            ),
            text(label).size(11)
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .on_press(on_press)
    .padding([3, 8])
    .style(move |theme: &Theme, status| {
        let palette = theme.palette();
        button::Style {
            background: Some(Background::Color(if on {
                quiet_selection(theme)
            } else if matches!(status, button::Status::Hovered) {
                palette.background.strong.color
            } else {
                palette.background.weak.color
            })),
            text_color: palette.background.base.text,
            border: Border {
                color: if on {
                    palette.primary.base.color
                } else {
                    palette.background.neutral.color
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

/// A read-back sentence with its joining words (and, or, not) in the
/// accent colour, as the mockup's "Reads:" line.
pub fn reads<'a>(sentence: String, keywords: &[String]) -> Element<'a, Message> {
    use iced::widget::text::Span;
    let mut spans: Vec<Span<'a, Message>> = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, spans: &mut Vec<Span<'a, Message>>| {
        if word.is_empty() {
            return;
        }
        let key = keywords
            .iter()
            .any(|k| k.eq_ignore_ascii_case(word.trim_matches(|c| c == '(' || c == ')')));
        let span = iced::widget::span(std::mem::take(word));
        // The accent of both built-in themes, readable on dark and light.
        spans.push(if key {
            span.color(iced::Color::from_rgb8(0x1E, 0x8F, 0xD0))
        } else {
            span
        });
    };
    for c in sentence.chars() {
        word.push(c);
        if c == ' ' {
            flush(&mut word, &mut spans);
        }
    }
    flush(&mut word, &mut spans);
    iced::widget::rich_text(spans).size(12).into()
}

/// The "i" beside a value: what patterns a value may use (#2611 mockup,
/// States), shown on hover.
pub fn pattern_help<'a>() -> Element<'a, Message> {
    let line = |pattern: &'static str, what: std::borrow::Cow<'static, str>| {
        row![
            text(pattern)
                .size(11)
                .font(iced::Font::MONOSPACE)
                .width(Length::Fixed(80.0)),
            text(what).size(11),
        ]
        .spacing(6)
    };
    let tip = container(
        column![
            text(crate::t!("A value may use patterns:")).size(11),
            line("FAL*", crate::t!("any characters after FAL")),
            line("FAL-?", crate::t!("one character after FAL-")),
            line("A#", crate::t!("one digit")),
            line("FAL*,AJTO*", crate::t!("either of them")),
            line("~FAL*", crate::t!("not these")),
        ]
        .spacing(3),
    )
    .padding([6, 8])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    });
    iced::widget::tooltip(
        container(crate::ui::icons::themed_secondary(
            crate::ui::icons::INFO,
            13.0,
        ))
        .padding(4),
        tip,
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

/// "Searching only the N selected objects · Search all" (mockup, States):
/// the selection scope says so, with a way back to everything.
pub fn selection_note<'a>(count: usize, search_all: Message) -> Element<'a, Message> {
    row![
        muted(
            crate::tf!("Searching only the {} selected objects", fmt_count(count)).into_owned(),
            11.0
        ),
        button(text(crate::t!("Search all")).size(11))
            .on_press(search_all)
            .style(button::text)
            .padding([0, 4]),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .into()
}

pub fn muted<'a>(label: impl text::IntoFragment<'a>, size: f32) -> iced::widget::Text<'a> {
    text(label).size(size).style(|theme: &Theme| text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
    })
}
