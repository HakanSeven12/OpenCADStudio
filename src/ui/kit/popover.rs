//! A settings popover and the rows it is made of: the frame that hangs from
//! a button in the title bar, a check box row, a check box with a choice
//! beside it, and a row of choice tiles. Each row lines up with the
//! property grid ([`super::row_view`]), so a popover reads like a palette.

use crate::app::Message;
use crate::ui::style::{chrome, kit_metrics as km, text};
use iced::widget::{button, checkbox, column, container, row, Space};
use iced::{Background, Border, Element, Length, Padding, Theme};

/// A popover: `title_bar` flush along the top and `bands` under it on the
/// palette surface, `width` wide and as tall as its content, in the frame
/// (edge and shadow) every menu shares. A press on its empty space stays
/// in it instead of reaching what is under it.
pub fn popover<'a>(
    title_bar: Element<'a, Message>,
    bands: impl Into<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    let body = container(column![title_bar, bands.into()])
        .width(Length::Fixed(width))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface(theme))),
            text_color: Some(chrome::text(theme)),
            ..Default::default()
        });
    let framed = container(body)
        .style(super::menu::frame_style)
        .padding(km::DROPDOWN_PAD);
    iced::widget::mouse_area(framed)
        .on_press(Message::Noop)
        .into()
}

/// The grid inset of a row: room before it as before a grid row's name.
fn inset<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(km::ROW_H))
        .padding(Padding::ZERO.left(km::INDENT + km::CELL_PAD_X))
        .align_y(iced::Center)
        .into()
}

/// A check box with `label` after it, toggled by a press on either.
fn check<'a>(label: String, checked: bool, on_toggle: Option<Message>) -> Element<'a, Message> {
    checkbox(checked)
        .label(label)
        .on_toggle_maybe(on_toggle.map(|message| move |_| message.clone()))
        .size(km::CHECK)
        .spacing(km::CELL_PAD_X)
        .text_size(text::UI)
        .into()
}

/// A row with a check box and its `label`, one grid row high and inset as
/// the grid is. Without `on_toggle` it shows disabled.
pub fn check_row<'a>(
    label: impl Into<String>,
    checked: bool,
    on_toggle: Option<Message>,
) -> Element<'a, Message> {
    inset(check(label.into(), checked, on_toggle))
}

/// A check box and `label` in the left half and a choice (`dropdown`) in
/// the right half. Without a `dropdown`, because the box is cleared or the
/// choice is fixed for now, the right half shows the current choice
/// (`shown`) muted.
pub fn check_select_row<'a>(
    label: impl Into<String>,
    checked: bool,
    on_toggle: Option<Message>,
    dropdown: Option<Element<'a, Message>>,
    shown: &str,
) -> Element<'a, Message> {
    let value = match dropdown {
        Some(dropdown) => dropdown,
        None => super::read_only_field(shown),
    };
    row![
        container(inset(check(label.into(), checked, on_toggle))).width(Length::FillPortion(1)),
        container(value)
            .width(Length::FillPortion(1))
            .height(Length::Fixed(km::ROW_H))
            .align_y(iced::Center),
    ]
    .height(Length::Fixed(km::ROW_H))
    .into()
}

/// A row of choice tiles, one per option (`icon` and label), side by side
/// across the popover: the `selected` one has an accent border, a press on
/// another sends its message.
pub fn radio_row<'a, T: Copy + PartialEq + 'a>(
    options: Vec<(T, &'static [u8], String)>,
    selected: T,
    on_select: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let tiles = options.into_iter().map(|(value, icon, label)| {
        let on = value == selected;
        let content = row![
            crate::ui::icons::tool(icon, km::CHOICE_ICON),
            super::one_line(iced::widget::text(label))
                .size(text::UI)
                .line_height(text::line()),
        ]
        .spacing(km::CHOICE_GAP)
        .align_y(iced::Center);
        button(container(content).center(Length::Fill))
            .on_press(on_select(value))
            .width(Length::Fill)
            .height(Length::Fixed(km::CHOICE_TILE_H))
            .padding(0)
            .style(move |theme: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(if hovered {
                        chrome::hover(theme)
                    } else {
                        chrome::surface_raised(theme)
                    })),
                    text_color: if on || hovered {
                        chrome::strong(theme)
                    } else {
                        chrome::text(theme)
                    },
                    border: Border {
                        color: if on {
                            chrome::accent(theme)
                        } else {
                            chrome::surface_raised(theme)
                        },
                        width: km::line(),
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
    });
    container(
        row(tiles.collect::<Vec<Element<'a, Message>>>())
            .spacing(km::CHOICE_GAP)
            .width(Length::Fill),
    )
    .padding(Padding {
        top: km::TOOL_GAP,
        bottom: km::TOOL_GAP,
        left: km::GRID_INSET_L,
        right: km::GRID_INSET_R,
    })
    .into()
}

/// A short note under a row (why a choice does not hold now), in muted
/// text, inset as the grid is; it wraps rather than being cut.
pub fn note_row<'a>(note: impl Into<String>) -> Element<'a, Message> {
    container(
        iced::widget::text(note.into())
            .size(text::UI)
            .line_height(text::line())
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(chrome::muted(theme)),
            }),
    )
    .width(Length::Fill)
    .padding(
        Padding::ZERO
            .left(km::INDENT + km::CELL_PAD_X)
            .right(km::GRID_INSET_R),
    )
    .into()
}

/// Room between two groups of rows that have no bar between them.
pub fn gap<'a>() -> Element<'a, Message> {
    Space::new().height(km::TOOL_GAP).into()
}
