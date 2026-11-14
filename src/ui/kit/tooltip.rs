//! The one tooltip of the app: the ribbon's (panel colour, 1 px border,
//! 11 px text), used by every palette.

use crate::app::Message;
use crate::ui::style::{palette_metrics as pm, text};
use iced::widget::{container, text as label, tooltip as iced_tooltip};
use iced::Element;

/// Where a tooltip opens.
pub use iced::widget::tooltip::Position as TipPosition;

/// How long the pointer rests before a tooltip shows, as on the ribbon.
const TIP_DELAY_MS: u64 = 400;

/// `content` with `tip` shown beside it at `position`.
pub fn tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    tip: impl Into<String>,
    position: TipPosition,
) -> Element<'a, Message> {
    tooltip_gap(content, tip, position, pm::TOOL_GAP)
}

/// [`tooltip`] `gap` away from its content (the ribbon's tips sit
/// further from their buttons).
pub fn tooltip_gap<'a>(
    content: impl Into<Element<'a, Message>>,
    tip: impl Into<String>,
    position: TipPosition,
    gap: f32,
) -> Element<'a, Message> {
    iced_tooltip(
        content,
        container(label(tip.into()).size(text::UI).line_height(text::line())).padding(pm::TIP_PAD),
        position,
    )
    .gap(gap)
    .delay(std::time::Duration::from_millis(TIP_DELAY_MS))
    .style(tooltip_style)
    .into()
}

/// A tooltip's panel: the panel colour inside a 1 px border.
pub fn tooltip_style(theme: &iced::Theme) -> container::Style {
    use crate::ui::style::chrome;
    container::Style {
        background: Some(iced::Background::Color(chrome::panel(theme))),
        border: iced::Border {
            color: chrome::border(theme),
            width: pm::line(),
            radius: super::toolbar::BUTTON_RADIUS.into(),
        },
        text_color: Some(chrome::text(theme)),
        ..Default::default()
    }
}
