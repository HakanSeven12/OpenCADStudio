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
/// Room kept free at the top right for the view cube and the UCS list; the
/// toolbar is centred in the height below it.
const TOP_RESERVE: f32 = 200.0;

fn icon_el(icon: IconKind) -> Element<'static, Message> {
    match icon {
        IconKind::Glyph(s) => text(s).size(ICON_SIZE * 0.85).into(),
        IconKind::Svg(bytes) => crate::ui::icons::semantic(bytes, ICON_SIZE),
    }
}

fn tip_panel(label: &'static str) -> Element<'static, Message> {
    // Ribbon labels break over two lines; a tooltip reads them on one.
    let label = crate::t!(label).replace('\n', " ");
    container(text(label).size(11))
        .padding([2, 6])
        .style(|theme: &Theme| {
            let palette = theme.palette();
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

/// The REFEDIT session toolbar's head: the definition being edited as an
/// accent pill (REFEDITNAME) and how many objects the working set holds.
pub fn refedit_header(name: &str, count: usize) -> Element<'static, Message> {
    // Long names are cut so the bar stays narrow; the full name is in the tip.
    let short: String = if name.chars().count() > 7 {
        format!("{}…", name.chars().take(6).collect::<String>())
    } else {
        name.to_string()
    };
    let pill = container(text(short).size(10))
        .padding([1, 6])
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.primary.base.color)),
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                text_color: Some(palette.primary.base.text),
                ..Default::default()
            }
        });
    let tip = container(text(name.to_string()).size(11)).padding([2, 6]).style(|theme: &Theme| {
        let palette = theme.palette();
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
    });
    column![
        tooltip(pill, tip, tooltip::Position::Left).gap(6),
        text(format!("{} {}", count, crate::tr!("ref-edit", "toolbar-objects")))
            .size(9)
            .style(crate::ui::style::common::muted_style),
    ]
    .spacing(2)
    .align_x(iced::Center)
    .into()
}

/// Build the floating right-edge vertical toolbar from `tools`, vertically
/// centred over the canvas. Returns `None` when `tools` is empty so the caller
/// can skip pushing an overlay.
pub fn view(tools: &[ToolDef]) -> Option<Element<'static, Message>> {
    view_with_active(tools, &|_| false)
}

/// As [`view`], with the tools `active` names drawn as switched on.
pub fn view_with_active(
    tools: &[ToolDef],
    active: &dyn Fn(&str) -> bool,
) -> Option<Element<'static, Message>> {
    view_sections(None, &[tools], active)
}

/// As [`view_with_active`], with `header` on top and a rule between the
/// groups of tools.
pub fn view_sections(
    header: Option<Element<'static, Message>>,
    groups: &[&[ToolDef]],
    active: &dyn Fn(&str) -> bool,
) -> Option<Element<'static, Message>> {
    if groups.iter().all(|g| g.is_empty()) {
        return None;
    }

    let mut col = column![].spacing(4).align_x(iced::Center);
    if let Some(header) = header {
        col = col.push(header);
    }
    let tools = groups.iter().filter(|g| !g.is_empty()).enumerate().flat_map(|(group, tools)| {
        tools.iter().enumerate().map(move |(k, t)| (group > 0 && k == 0, t))
    });
    for (starts_group, t) in tools {
        if starts_group {
            col = col.push(
                container(iced::widget::Space::new())
                    .width(Length::Fixed(BTN_SIZE - 4.0))
                    .height(Length::Fixed(1.0))
                    .style(|theme: &Theme| container::Style {
                        background: Some(Background::Color(theme.palette().background.neutral.color)),
                        ..Default::default()
                    }),
            );
        }
        let on = active(t.id);
        let btn = button(icon_el(t.icon))
            .on_press(Message::RibbonToolClick {
                tool_id: t.id.to_string(),
                event: t.event.clone(),
            })
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE))
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: if on {
                        Some(Background::Color(palette.primary.weak.color))
                    } else {
                        hovered.then_some(Background::Color(palette.background.strong.color))
                    },
                    border: Border {
                        radius: 3.0.into(),
                        color: palette.primary.base.color,
                        width: if on { 1.0 } else { 0.0 },
                    },
                    text_color: palette.background.base.text,
                    ..Default::default()
                }
            });
        // Label tooltip on the left so it never runs off the right edge.
        col = col.push(tooltip(btn, tip_panel(t.label), tooltip::Position::Left).gap(6));
    }

    let panel = container(col).padding(4).style(|theme: &Theme| {
        let palette = theme.palette();
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
                top: TOP_RESERVE,
                ..iced::Padding::ZERO
            })
            .into(),
    )
}
