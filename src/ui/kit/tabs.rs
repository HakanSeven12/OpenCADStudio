//! Segmented tabs at the top of a palette (Block Palette, Tool Palettes,
//! Authoring Palettes, Sheet Set): equal parts on a `surface_raised` track,
//! the active one on the surface.

use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km, text};
use iced::widget::{button, container, row, text as label};
use iced::{Background, Element, Length, Padding, Theme};

/// The tabs `items` (label, message), `active` lit, in a palette band.
pub fn tabs<'a>(items: Vec<(String, Message)>, active: usize) -> Element<'a, Message> {
    let parts = items.into_iter().enumerate().map(|(i, (name, message))| {
        let is_active = i == active;
        // Tabs share the width in proportion to their labels, so a long one
        // ("Current Drawing") is not cut while a short one has room to spare.
        let portion = (crate::ui::style::text_metrics::width(&name, text::UI, false)
            + 2.0 * km::CELL_PAD_X)
            .ceil()
            .max(1.0) as u16;
        button(
            container(
                label(name)
                    .size(text::UI)
                    .line_height(text::line())
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .center(Length::Fill)
            .clip(true),
        )
        .on_press(message)
        .clip(true)
        .width(Length::FillPortion(portion))
        .height(Length::Fixed(km::TOOL_H - 2.0 * km::line()))
        .padding(0)
        .style(move |theme: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: if is_active {
                    Some(Background::Color(chrome::surface(theme)))
                } else if hovered {
                    Some(Background::Color(chrome::hover(theme)))
                } else {
                    None
                },
                text_color: if is_active || hovered {
                    chrome::strong(theme)
                } else {
                    chrome::tab_text(theme)
                },
                ..Default::default()
            }
        })
        .into()
    });
    super::band(
        container(row(parts).spacing(km::line()))
            .padding(Padding::new(km::line()))
            .width(Length::Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::surface_raised(theme))),
                ..Default::default()
            }),
    )
}

/// One vertical tab along a palette's side (Tool Palettes, Authoring
/// Palettes): its name in capitals reading upward, the open one on the
/// surface and the others on the strip, as the palette strip shows
/// them.
pub fn side_tab<'a>(name: &str, selected: bool, message: Message) -> Element<'a, Message> {
    let height = crate::ui::dock::strip_tab_h(name, crate::ui::dock::StripNames::NamesOnly);
    button(
        iced::widget::canvas(crate::ui::dock::VerticalLabel {
            text: name.to_uppercase(),
            centered: true,
        })
        .width(Length::Fill)
        .height(height),
    )
    .on_press(message)
    .width(km::STRIP_BODY_W)
    .padding(0)
    .style(move |theme: &Theme, status| button::Style {
        background: Some(Background::Color(if selected {
            chrome::surface(theme)
        } else if status == button::Status::Hovered {
            chrome::hover(theme)
        } else {
            chrome::strip(theme)
        })),
        ..Default::default()
    })
    .into()
}
