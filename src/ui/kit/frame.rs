//! A palette's frame and its bands: the body every palette sits in, and
//! the inset strip a toolbar, search field or selector stands in.

use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km};
use iced::widget::{column, container};
use iced::{Background, Element, Length, Padding, Theme};

/// A palette: its title bar flush along the top and its bands below, on
/// the surface, `width` wide and full height. The bands set their own
/// insets ([`band`], [`crate::ui::kit::scroll_area`]).
pub fn palette<'a>(
    title_bar: Element<'a, Message>,
    bands: impl Into<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    // Each band keeps a tool gap above it; the first one gets the rest of
    // the top inset, so it sits as far from the title bar as from the left.
    let bands = container(bands)
        .height(Length::Fill)
        .padding(Padding::ZERO.top(km::TOP_INSET - km::TOOL_GAP));
    container(column![title_bar, bands])
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface(theme))),
            text_color: Some(chrome::text(theme)),
            ..Default::default()
        })
        .into()
}

/// A band of a palette (toolbar, search field, selector, tabs): `content`
/// inset as the grid is, 8 px left and 10 px right, with a 4 px gap above.
pub fn band<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .padding(Padding {
            top: km::TOOL_GAP,
            bottom: 0.0,
            left: km::GRID_INSET_L,
            right: km::GRID_INSET_R,
        })
        .into()
}
