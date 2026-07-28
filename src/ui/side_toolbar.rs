//! Reusable right-edge vertical toolbar.
//!
//! A single centred column of icon buttons floated over the right edge of the
//! canvas — a lightweight, context-specific alternative to a contextual ribbon
//! tab. It is built from a flat list of [`ToolDef`]s and dispatches each tool's
//! command through the existing [`Message::RibbonToolClick`] path, so any
//! module's tools can drive it. First used for paper-space viewport / plot
//! actions; reusable for any future context action set.

use iced::widget::{button, column, container, text, tooltip};
use iced::{Background, Border, Element, Length, Theme};

use crate::app::Message;
use crate::modules::{IconKind, ToolDef};

const ICON_SIZE: f32 = 22.0;
const BTN_SIZE: f32 = 38.0;
/// Gap between the toolbar and the right edge of the canvas.
const EDGE_MARGIN: f32 = 8.0;

fn icon_el(icon: IconKind) -> Element<'static, Message> {
    match icon {
        IconKind::Glyph(s) => text(s).size(ICON_SIZE * 0.85).into(),
        IconKind::Svg(bytes) => crate::ui::icons::semantic(bytes, ICON_SIZE),
    }
}

fn tip_panel(label: &'static str) -> Element<'static, Message> {
    container(text(label).size(11))
        .padding([2, 6])
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: Some(Background::Color(palette.background.strong.color)),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: Some(palette.background.strong.text),
            ..Default::default()
            }
        })
        .into()
}

/// Build the floating right-edge vertical toolbar from `tools`, vertically
/// centred over the canvas. Returns `None` when `tools` is empty so the caller
/// can skip pushing an overlay.
pub fn view(tools: &[ToolDef]) -> Option<Element<'static, Message>> {
    if tools.is_empty() {
        return None;
    }

    let mut col = column![].spacing(4).align_x(iced::Center);
    for t in tools {
        let btn = button(icon_el(t.icon))
            .on_press(Message::RibbonToolClick {
                tool_id: t.id.to_string(),
                event: t.event.clone(),
            })
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE))
            .style(|theme: &Theme, status| {
                let palette = theme.extended_palette();
                let hovered = matches!(
                    status,
                    button::Status::Hovered | button::Status::Pressed
                );
                button::Style {
                background: hovered
                    .then_some(Background::Color(palette.background.strong.color)),
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                text_color: palette.background.base.text,
                ..Default::default()
                }
            });
        // Label tooltip on the left so it never runs off the right edge.
        col = col.push(
            tooltip(btn, tip_panel(t.label), tooltip::Position::Left).gap(6),
        );
    }

    let panel = container(col).padding(4).style(|theme: &Theme| {
        let palette = theme.extended_palette();
        container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..Default::default()
        }
    });

    // Fill the canvas, pin the panel to the right edge and centre it
    // vertically — no manual size/position math needed.
    Some(
        container(iced::widget::opaque(panel))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Right)
            .align_y(iced::Center)
            .padding(iced::Padding {
                right: EDGE_MARGIN,
                ..iced::Padding::ZERO
            })
            .into(),
    )
}
