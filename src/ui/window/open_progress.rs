//! Modal overlay shown while a CAD file is being loaded.
//!
//! Displays the file name, size, current phase, measured progress, and a
//! Cancel button.

use iced::time::Instant;
use iced::widget::{button, column, container, row, stack, text, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};
use std::sync::atomic::Ordering;

use crate::app::{
    Message, OpenProgress, OPEN_PHASE_CACHING, OPEN_PHASE_FINALIZING, OPEN_PHASE_PARSING,
    OPEN_PHASE_READING, OPEN_PHASE_XREF,
};

const CARD_WIDTH: f32 = 420.0;
const BAR_TRACK_WIDTH: f32 = 380.0;
const BAR_TRACK_HEIGHT: f32 = 6.0;

fn phase_label(phase: u8) -> &'static str {
    match phase {
        OPEN_PHASE_READING => "Reading file…",
        OPEN_PHASE_PARSING => "Parsing entities…",
        OPEN_PHASE_XREF => "Loading references…",
        OPEN_PHASE_CACHING => "Building scene caches…",
        OPEN_PHASE_FINALIZING => "Finalizing…",
        _ => "Working…",
    }
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

pub fn view<'a>(progress: &'a OpenProgress, _now: Instant) -> Element<'a, Message> {
    let phase = progress.state.phase.load(Ordering::Acquire);
    let basis_points = progress
        .state
        .basis_points
        .load(Ordering::Relaxed)
        .min(10000);
    let fraction = basis_points as f32 / 10000.0;
    let fill_width = BAR_TRACK_WIDTH * fraction;
    let trailing = (BAR_TRACK_WIDTH - fill_width).max(0.0);

    let bar_fill: Element<'_, Message> = container(
        Space::new()
            .width(Length::Fixed(fill_width))
            .height(Length::Fixed(BAR_TRACK_HEIGHT)),
    )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().primary.base.color
            )),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into();

    let bar_value: Element<'_, Message> = row![
        bar_fill,
        Space::new()
            .width(Length::Fixed(trailing))
            .height(Length::Fixed(BAR_TRACK_HEIGHT)),
    ]
    .into();

    let bar_track: Element<'_, Message> = container(
        stack![
            container(Space::new().width(Length::Fixed(BAR_TRACK_WIDTH)).height(Length::Fixed(BAR_TRACK_HEIGHT)))
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(
                        theme.extended_palette().background.strong.color
                    )),
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            bar_value,
        ]
        .width(Length::Fixed(BAR_TRACK_WIDTH))
        .height(Length::Fixed(BAR_TRACK_HEIGHT)),
    )
    .into();

    // ── Card body ────────────────────────────────────────────────────────
    let title = text("Opening file").size(15);

    let name_line = text(format!(
        "{}  ({})",
        progress.name,
        format_size(progress.size_bytes)
    ))
    .size(13)
    .style(|theme: &Theme| iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.82)),
    });

    let phase_line = text(format!(
        "{}  {:.1}%",
        phase_label(phase),
        basis_points as f32 / 100.0
    ))
        .size(12)
        .style(|theme: &Theme| iced::widget::text::Style {
            color: Some(theme.extended_palette().primary.base.color),
        });

    let cancel_btn: Element<'_, Message> = button(text("Cancel").size(12))
        .on_press(Message::OpenCancel)
        .style(button::danger)
        .padding([4, 14])
        .into();

    let cancel_row: Element<'_, Message> = container(cancel_btn).align_right(Fill).into();

    let card = container(
        column![title, name_line, bar_track, phase_line, cancel_row]
            .spacing(10)
            .width(Length::Fixed(CARD_WIDTH)),
    )
    .padding([18, 22])
    .style(|theme: &Theme| {
        let palette = theme.extended_palette();
        container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
        }
    });

    // ── Backdrop (click-blocker + dim) ────────────────────────────────────
    let backdrop: Element<'_, Message> = container(Space::new().width(Fill).height(Fill))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.strong.color.scale_alpha(0.72)
            )),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into();

    let centered: Element<'_, Message> = container(card)
        .center_x(Fill)
        .center_y(Fill)
        .width(Fill)
        .height(Fill)
        .into();

    stack![backdrop, centered].width(Fill).height(Fill).into()
}
