//! The boxes at the top of a palette: a search field and a selector (the
//! Properties object list box).

use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km, text};
use iced::widget::{container, text_input};
use iced::{Background, Element, Length, Padding, Theme};

/// Vertical padding of a 22 px box around a 14 px text line and its 1 px
/// border.
const BOX_PAD_Y: f32 = (km::TOOL_H - text::UI_LINE - 2.0 * km::LINE) / 2.0;

/// Padding of a text input or combo filling a 22 px box (a toolbar-height
/// field, as opposed to a [`super::FIELD_PAD`] grid cell).
pub const BOX_PAD: Padding = Padding {
    top: BOX_PAD_Y,
    bottom: BOX_PAD_Y,
    left: km::CELL_PAD_X,
    right: km::CELL_PAD_X,
};

/// A search field: the `surface_raised` box, 22 px, 11 px text. Returned as a
/// text input so callers can add `on_submit`, an `id`, ...
pub fn search_field<'a>(
    placeholder: &str,
    value: &str,
    on_input: impl Fn(String) -> Message + 'a,
) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value)
        .on_input(on_input)
        .size(text::UI)
        .line_height(text::line())
        .padding(BOX_PAD)
        .width(Length::Fill)
        .style(super::combo_style)
}

/// A selector box (the object list at the top of Properties): `content`
/// in the 22 px `surface_raised` box, filling the width.
pub fn select_box<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(km::TOOL_H))
        .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
        .align_y(iced::Center)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface_raised(theme))),
            text_color: Some(chrome::text(theme)),
            ..Default::default()
        })
        .into()
}

/// A button that opens a list (a picker head, an L / B / C mode button):
/// the `surface_raised` box, `hover` while hovered, the accent edge while its
/// list is `open`.
pub fn box_button_style(
    open: bool,
) -> impl Fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |theme: &Theme, status: iced::widget::button::Status| {
        use iced::widget::button::Status;
        let hovered = matches!(status, Status::Hovered | Status::Pressed);
        iced::widget::button::Style {
            background: Some(Background::Color(if hovered {
                chrome::hover(theme)
            } else {
                chrome::surface_raised(theme)
            })),
            text_color: if hovered {
                chrome::strong(theme)
            } else {
                chrome::text(theme)
            },
            border: iced::Border {
                color: if open {
                    chrome::accent(theme)
                } else {
                    chrome::surface_raised(theme)
                },
                width: km::line(),
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    }
}

/// A pick list in a palette: the `surface_raised` box, accent edge while
/// open, 11 px text (use with `.text_size(text::UI)` and [`BOX_PAD`]).
pub fn pick_list_style(
    theme: &Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    use iced::widget::pick_list::Status;
    let (background, edge) = match status {
        Status::Opened { .. } => (chrome::surface_raised(theme), chrome::accent(theme)),
        Status::Hovered => (chrome::hover(theme), chrome::hover(theme)),
        Status::Active | Status::Disabled => {
            (chrome::surface_raised(theme), chrome::surface_raised(theme))
        }
    };
    iced::widget::pick_list::Style {
        text_color: if matches!(status, Status::Hovered) {
            chrome::strong(theme)
        } else {
            chrome::text(theme)
        },
        placeholder_color: chrome::muted(theme),
        handle_color: chrome::text(theme),
        background: Background::Color(background),
        border: iced::Border {
            color: edge,
            width: km::line(),
            radius: 0.0.into(),
        },
    }
}

/// The list a palette pick list opens: the kit menu's look.
pub fn pick_menu_style(theme: &Theme) -> iced::overlay::menu::Style {
    // The list of a dropdown in the colours of the kit menus (the
    // right-click menus): their frame and shadow, the item under the
    // pointer or the keys lit as theirs.
    let frame = crate::ui::kit::menu::frame_style(theme);
    iced::overlay::menu::Style {
        background: Background::Color(chrome::surface(theme)),
        border: frame.border,
        text_color: chrome::text(theme),
        selected_text_color: chrome::strong(theme),
        selected_background: Background::Color(chrome::hover(theme)),
        shadow: frame.shadow,
    }
}

// A field's text, padding and border fit its box, checked when the crate
// compiles.
const _: () = assert!(2.0 * BOX_PAD_Y + text::UI_LINE + 2.0 * km::LINE == km::TOOL_H);
