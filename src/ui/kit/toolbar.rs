//! A palette's toolbar: a row of square icon buttons, and the small
//! buttons a category bar carries.

use super::tooltip::{tooltip, TipPosition};
use crate::app::Message;
use crate::ui::style::{chrome, palette_metrics as pm};
use iced::widget::{button, container, row};
use iced::{Background, Border, Color, Element, Length, Theme};

/// A row of toolbar buttons, 4 px apart, in a palette [`super::band`].
pub fn toolbar<'a>(buttons: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    super::band(row(buttons).spacing(pm::TOOL_GAP).align_y(iced::Center))
}

/// The look of a flat square button: nothing until hovered, then
/// `chrome::hover` with a `hover_border`; `active` (a switch that is on)
/// lit like a ribbon button.
pub fn flat_button_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, border) = match (active, hovered) {
            (true, _) => (
                Some(chrome::lit(theme)),
                if hovered {
                    chrome::hover_border(theme)
                } else {
                    chrome::lit_border(theme)
                },
            ),
            (false, true) => (Some(chrome::hover(theme)), chrome::hover_border(theme)),
            (false, false) => (None, Color::TRANSPARENT),
        };
        button::Style {
            background: background.map(Background::Color),
            border: Border {
                color: border,
                width: if background.is_some() {
                    pm::line()
                } else {
                    0.0
                },
                radius: BUTTON_RADIUS.into(),
            },
            // Lit or hovered, the glyph takes the strong text: the plain
            // text colour is too faint on the hover surface in some light
            // themes (WCAG AA).
            text_color: if matches!(status, button::Status::Disabled) {
                chrome::muted(theme)
            } else if active || hovered {
                chrome::strong(theme)
            } else {
                chrome::text(theme)
            },
            ..Default::default()
        }
    }
}

/// Corner radius of a lit or hovered button.
pub(super) const BUTTON_RADIUS: f32 = 2.0;

/// A toolbar button: `icon` centred in a 22 px square, its tooltip below.
/// `None` for `message` shows it disabled.
pub fn tool_button<'a>(
    icon: impl Into<Element<'a, Message>>,
    tip: impl Into<String>,
    message: Option<Message>,
    active: bool,
) -> Element<'a, Message> {
    let b = button(container(icon).center(Length::Fill))
        .on_press_maybe(message)
        .width(Length::Fixed(pm::TOOL_H))
        .height(Length::Fixed(pm::TOOL_H))
        .padding(0)
        .style(flat_button_style(active));
    tooltip(b, tip, TipPosition::Bottom)
}

/// A toolbar icon: `art` at the row icon size in the text colour (the
/// button's lit state brightens it to `strong`).
pub fn tool_icon<'a>(art: &'static [u8]) -> Element<'a, Message> {
    crate::ui::icons::tinted(art, pm::ROW_ICON, chrome::text)
}

/// A small square button for a category bar (collapse all, expand all, add).
pub fn bar_button<'a>(
    art: &'static [u8],
    tip: impl Into<String>,
    message: Message,
) -> Element<'a, Message> {
    let b = button(
        container(crate::ui::icons::tinted(art, pm::BAR_ICON, chrome::text)).center(Length::Fill),
    )
    .on_press(message)
    .width(Length::Fixed(pm::BAR_BUTTON))
    .height(Length::Fixed(pm::BAR_BUTTON))
    .padding(0)
    .style(flat_button_style(false));
    tooltip(b, tip, TipPosition::Bottom)
}

/// A text button of a palette (Create Table, Insert, Cancel): 22 px high on
/// `palette_bar`, lit by `hover`; `primary` fills it with the accent.
pub fn text_button<'a>(
    caption: impl Into<String>,
    message: Option<Message>,
    primary: bool,
) -> Element<'a, Message> {
    button(
        container(
            iced::widget::text(caption.into())
                .size(crate::ui::style::text::UI)
                .line_height(crate::ui::style::text::line()),
        )
        .center_y(Length::Fill),
    )
    .on_press_maybe(message)
    .height(Length::Fixed(pm::TOOL_H))
    .padding([0.0, pm::MENU_PAD_X])
    .style(text_button_style(primary))
    .into()
}

/// The style of a [`text_button`], for a text button laid out by hand
/// (a plugin panel's button with its own width).
pub fn text_button_style(primary: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let disabled = matches!(status, button::Status::Disabled);
        let background = match (primary, hovered) {
            (true, false) => chrome::accent(theme),
            (true, true) => chrome::mix(chrome::accent(theme), chrome::strong(theme), 0.15),
            (false, false) => chrome::palette_bar(theme),
            (false, true) => chrome::hover(theme),
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: if disabled {
                chrome::muted(theme)
            } else if primary {
                chrome::on_accent(theme)
            } else {
                chrome::text(theme)
            },
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: BUTTON_RADIUS.into(),
            },
            ..Default::default()
        }
    }
}
