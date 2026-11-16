//! The quiet text lines of a palette: what shows when there is nothing to
//! list, and a status line at the bottom.

use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km, text};
use iced::widget::{container, text as label};
use iced::{Background, Element, Length, Padding, Theme};

/// What a palette shows when there is nothing to list ("No selection",
/// "No blocks in this drawing"): one muted line, inset like the grid.
pub fn empty_state<'a>(message: impl Into<String>) -> Element<'a, Message> {
    container(
        label(message.into())
            .size(text::UI_SMALL)
            .line_height(text::line())
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(chrome::muted(theme)),
            }),
    )
    .padding(Padding {
        top: km::TOOL_GAP,
        bottom: km::TOOL_GAP,
        left: km::GRID_INSET_L,
        right: km::GRID_INSET_R,
    })
    .width(Length::Fill)
    .into()
}

/// A palette's status line at its bottom ("Current layer: 0 · 1 of 1
/// layers shown"): one row high per line, on the category-bar colour.
pub fn footer<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    // One line fills a grid row; a wrapped note grows the footer.
    container(content)
        .width(Length::Fill)
        .padding(Padding {
            top: FOOTER_PAD_Y,
            bottom: FOOTER_PAD_Y,
            left: km::GRID_INSET_L,
            right: km::GRID_INSET_R,
        })
        .align_y(iced::Center)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface_raised(theme))),
            text_color: Some(chrome::muted(theme)),
            ..Default::default()
        })
        .into()
}

/// Footer text: [`text::UI`] on a whole-pixel line.
pub fn footer_text<'a>(value: impl Into<String>) -> Element<'a, Message> {
    label(value.into())
        .size(text::UI)
        .line_height(text::line())
        .wrapping(iced::widget::text::Wrapping::Word)
        .into()
}

/// Room above and below a footer's line, so one line is a grid row high.
const FOOTER_PAD_Y: f32 = (km::ROW_H - text::UI_LINE) / 2.0;
