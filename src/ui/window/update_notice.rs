use crate::app::Message;
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Theme};
use crate::t;
use std::borrow::Cow;

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

/// Renders one of the two "Installed" / "Latest" cards. The `highlight`
/// flag tints the border + label with the accent colour, making the new
/// version the visual anchor of the row.
fn version_card<'a>(
    label: Cow<'static, str>,
    value: String,
    highlight: bool,
    width: iced::Length,
) -> Element<'a, Message> {
    container(
        column![
            text(label)
                .size(10)
                .style(move |theme: &Theme| if highlight {
                    primary_style(theme)
                } else {
                    muted_style(theme)
                }),
            text(value).size(20),
        ]
        .spacing(4)
        .align_x(iced::Center),
    )
    .width(width)
    .padding(iced::Padding {
        top: 14.0,
        right: 12.0,
        bottom: 14.0,
        left: 12.0,
    })
    .align_x(iced::Center)
    .style(move |theme: &Theme| {
        let palette = theme.palette();
        let pair = if highlight {
            palette.primary.weak
        } else {
            palette.background.base
        };
        container::Style {
        background: Some(Background::Color(pair.color)),
        border: Border {
            color: if highlight {
                palette.primary.base.color
            } else {
                palette.background.neutral.color
            },
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
        }
    })
    .into()
}

/// Light-weight renderer for a single line of GitHub-style release-notes
/// markdown. Recognises:
///   * `## Heading` → bold accent line
///   * `### Heading` → smaller bold line
///   * `- bullet`   → indented bullet text
///   * `**bold**` runs and `` `code` `` runs (rendered tonally, not styled
///     differently — iced's text widget has no inline run styling).
/// Anything else is plain body text. Strips the markdown markers so the
/// dialog reads cleanly even if the user has a Patreon-formatted note.
fn render_notes_line<'a>(raw: &str) -> Element<'a, Message> {
    let trimmed = raw.trim_end();
    if trimmed.is_empty() {
        return Space::new()
            .height(iced::Length::Fixed(6.0))
            .into();
    }
    if let Some(rest) = trimmed.strip_prefix("## ") {
        return text(strip_inline_md(rest))
            .size(13)
            .style(primary_style)
            .into();
    }
    if let Some(rest) = trimmed.strip_prefix("### ") {
        return text(strip_inline_md(rest))
            .size(12)
            .into();
    }
    if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
        return row![
            container(crate::ui::icons::themed_secondary(crate::ui::icons::DOT, 5.0)).width(14),
            text(strip_inline_md(rest)).size(11),
        ]
        .spacing(4)
        .into();
    }
    text(strip_inline_md(trimmed)).size(11).into()
}

/// Drop `**…**` and `` `…` `` markers without preserving emphasis (iced 0.14
/// Text widgets style the whole string uniformly). Keeps the inner text.
fn strip_inline_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '*' && chars.peek() == Some(&'*') {
            chars.next();
            continue;
        }
        if c == '`' {
            continue;
        }
        out.push(c);
    }
    out
}

pub fn view_window<'a>(
    latest: &'a str,
    body: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let header = container(
        column![
            text(t!("New Release Available")).size(20).style(primary_style),
            text(t!("A newer Open CAD Studio version is published on GitHub."))
                .size(11)
                .style(muted_style),
        ]
        .spacing(4)
        .align_x(iced::Center),
    )
    .width(sizing.width)
    .padding(iced::Padding {
        top: 14.0,
        right: 0.0,
        bottom: 6.0,
        left: 0.0,
    })
    .align_x(iced::Center);

    // Two side-by-side version cards (Installed → Latest) with the latest
    // one accent-tinted to draw the eye. Replaces the previous label/value
    // row layout. The arrow between them is purely decorative.
    let installed = version_card(
        t!("Installed"),
        format!("v{}", env!("OCS_APP_VERSION")),
        false,
        sizing.width,
    );
    let latest_card = version_card(t!("Latest"), format!("v{}", latest), true, sizing.width);
    let arrow = container(crate::ui::icons::themed_secondary(
        crate::ui::icons::ARROW_LONG_RIGHT,
        20.0,
    ))
    .width(iced::Length::Fixed(32.0))
        .height(sizing.height)
        .align_x(iced::Center)
        .align_y(iced::Center);
    let info_block = row![installed, arrow, latest_card]
        .spacing(0)
        .align_y(iced::Center)
        .width(sizing.width);

    let later_btn = button(text(t!("Later")).size(11))
        .on_press(Message::UpdateNoticeClose)
        .style(button::secondary)
        .padding([6, 16]);

    let open_btn = button(text(t!("Open Release Page")).size(11))
        .on_press(Message::UpdateNoticeOpenRelease)
        .style(button::primary)
        .padding([6, 16]);

    let footer = row![Space::new().width(sizing.width), later_btn, open_btn]
        .spacing(8)
        .align_y(iced::Center)
        .padding(iced::Padding {
            top: 14.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        });

    // Release notes panel. Rendered as a light-markdown column inside a
    // bordered scrollable so long bodies stay contained and don't
    // explode the window. Empty body → "No release notes provided."
    let notes_heading = container(text(t!("What's new")).size(11).style(muted_style))
        .padding(iced::Padding {
            top: 10.0,
            right: 0.0,
            bottom: 4.0,
            left: 0.0,
        });

    let notes_body: Element<'a, Message> = if body.trim().is_empty() {
        text(t!("No release notes provided."))
            .size(11)
            .style(muted_style)
            .into()
    } else {
        let mut col = column![].spacing(4);
        for line in body.lines() {
            col = col.push(render_notes_line(line));
        }
        scrollable(container(col).padding([10, 14]))
            .height(sizing.height)
            .into()
    };

    let notes_block = container(notes_body)
        .width(sizing.width)
        .height(sizing.height)
        .style(container::bordered_box);

    // Keep the notes panel content-sized. The outer modal supplies the maximum
    // height, so long release notes scroll instead of stretching the dialog.
    let notes_fill = container(notes_block)
        .width(sizing.width)
        .height(sizing.height);

    container(
        column![header, info_block, notes_heading, notes_fill, footer]
            .spacing(0)
            .height(sizing.height)
            .padding(iced::Padding {
                top: 0.0,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            }),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
        ..Default::default()
    })
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
