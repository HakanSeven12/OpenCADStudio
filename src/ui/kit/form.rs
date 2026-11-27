//! The kit for windows: popovers and dialogs (later Options too). Where a
//! palette is a grid of cells ([`super::row_view`]), a window lays its
//! labels and controls straight on its surface: no grid lines, no cell
//! backgrounds, the label and the control each half a row. The look and
//! every size come from mockup B of Interface Settings; the sizes are the
//! `kit_metrics::FORM_*` constants.

use crate::app::Message;
use crate::ui::dropdown_list::DropdownList;
use crate::ui::style::{chrome, kit_metrics as km, text};
use iced::widget::button as btn;
use iced::widget::{checkbox, column, container, mouse_area, text as label, Space};
use iced::{Background, Border, Color, Element, Length, Padding, Shadow, Theme, Vector};

/// A popover: its head (`title` and a close button that sends `on_close`),
/// `body` and an optional `footer`, `km::FORM_W` wide, in a 1 px border with
/// a drop shadow. A press on its empty space stays in it.
pub fn popover<'a>(
    title: impl Into<String>,
    on_close: Message,
    body: impl Into<Element<'a, Message>>,
    footer: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let close = btn(container(crate::ui::icons::tinted(
        crate::ui::icons::CLOSE,
        km::TITLE_ICON,
        chrome::title_text,
    ))
    .center(Length::Fill))
    .on_press(on_close)
    .width(Length::Fixed(km::FORM_CLOSE))
    .height(Length::Fixed(km::FORM_CLOSE))
    .padding(0)
    .style(|theme: &Theme, status| btn::Style {
        background: matches!(status, btn::Status::Hovered | btn::Status::Pressed)
            .then(|| Background::Color(chrome::hover(theme))),
        text_color: chrome::text(theme),
        ..Default::default()
    });
    let head = container(
        iced::widget::row![
            label(title.into())
                .size(text::UI)
                .line_height(text::line())
                .font(super::SEMIBOLD)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
            close,
        ]
        .align_y(iced::Center),
    )
    .height(Length::Fixed(km::TITLE_H))
    .padding(
        Padding::ZERO
            .left(km::FORM_TITLE_X)
            .right(km::FORM_TITLE_X / 4.0),
    )
    .align_y(iced::Center)
    // Head and footer: the darker grey of the ribbon's panel-title strip.
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(chrome::title_bar(theme))),
        text_color: Some(chrome::text(theme)),
        ..Default::default()
    });
    let body = container(body).width(Length::Fill).padding(Padding {
        top: km::FORM_BODY_PAD_TOP,
        right: km::FORM_BODY_PAD_RIGHT,
        bottom: km::FORM_BODY_PAD_BOTTOM,
        left: km::FORM_BODY_PAD_LEFT,
    });
    // The dark hairline runs round the outside, under the head and over
    // the footer; nothing else inside is outlined.
    let mut content = column![head, hline(), body];
    if let Some(footer) = footer {
        content = content.push(hline()).push(footer);
    }
    // Inset by the outline, so the head's and footer's own backgrounds do
    // not paint over it.
    let framed = container(content)
        .width(Length::Fixed(km::FORM_W))
        .padding(km::line())
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface(theme))),
            text_color: Some(chrome::text(theme)),
            // The dark hairline, as under the ribbon.
            border: Border {
                color: chrome::edge(theme),
                width: km::line(),
                radius: 0.0.into(),
            },
            shadow: Shadow {
                color: Color::BLACK.scale_alpha(km::FORM_SHADOW_ALPHA),
                offset: Vector::new(0.0, km::FORM_SHADOW_Y),
                blur_radius: km::FORM_SHADOW_BLUR,
            },
            ..Default::default()
        });
    mouse_area(framed).on_press(Message::Noop).into()
}

/// The 1 px line under a popover's head.
fn hline<'a>() -> Element<'a, Message> {
    container(Space::new().width(Length::Fill).height(km::line()))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::edge(theme))),
            ..Default::default()
        })
        .into()
}

/// The body of a window: `children` one under another, `km::FORM_ROW_GAP`
/// apart, so stacked choice fields do not touch.
pub fn rows<'a>(children: Vec<Element<'a, Message>>) -> iced::widget::Column<'a, Message> {
    iced::widget::Column::with_children(children).spacing(km::FORM_ROW_GAP)
}

/// A group's head: a bar without a caret, `label` in semibold, with
/// `km::FORM_SECTION_GAP` of room above it.
pub fn section<'a>(title: impl Into<String>) -> Element<'a, Message> {
    let bar = container(
        super::one_line(label(title.into()))
            .size(text::UI)
            .line_height(text::line())
            .font(super::SEMIBOLD),
    )
    .width(Length::Fill)
    .height(Length::Fixed(km::FORM_ROW_H))
    .padding(Padding::ZERO.left(km::FORM_LABEL_X))
    .align_y(iced::Center)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(chrome::surface_raised(theme))),
        text_color: Some(chrome::text(theme)),
        ..Default::default()
    });
    column![
        Space::new().height(km::FORM_SECTION_GAP),
        bar,
        Space::new().height(km::FORM_SECTION_GAP_BELOW),
    ]
    .into()
}

/// A label on one line in the text colour.
fn row_label<'a>(value: String, enabled: bool) -> iced::widget::Text<'a> {
    super::one_line(label(value))
        .size(text::UI)
        .line_height(text::line())
        .style(move |theme: &Theme| iced::widget::text::Style {
            color: Some(if enabled {
                chrome::text(theme)
            } else {
                chrome::muted(theme)
            }),
        })
}

/// Two halves of a row, `left` and `right`.
fn halves<'a>(left: Element<'a, Message>, right: Element<'a, Message>) -> Element<'a, Message> {
    iced::widget::row![
        container(left)
            .width(Length::FillPortion(1))
            .height(Length::Fixed(km::FORM_ROW_H))
            .align_y(iced::Center)
            .clip(true),
        container(right)
            .width(Length::FillPortion(1))
            .height(Length::Fixed(km::FORM_ROW_H))
            .align_y(iced::Center),
    ]
    .height(Length::Fixed(km::FORM_ROW_H))
    .into()
}

/// A row: `label` in the left half, `km::FORM_LABEL_X` in, and `control` in
/// the right half.
pub fn row<'a>(name: impl Into<String>, control: Element<'a, Message>) -> Element<'a, Message> {
    let name =
        container(row_label(name.into(), true)).padding(Padding::ZERO.left(km::FORM_LABEL_X));
    halves(name.into(), control)
}

/// A check box in the accent colour.
fn check_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let (checked, hovered, disabled) = match status {
        checkbox::Status::Active { is_checked } => (is_checked, false, false),
        checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
        checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
    };
    let accent = if disabled {
        chrome::muted(theme)
    } else {
        chrome::accent(theme)
    };
    checkbox::Style {
        background: Background::Color(if checked {
            accent
        } else if hovered {
            chrome::hover(theme)
        } else {
            chrome::surface(theme)
        }),
        icon_color: chrome::on_accent(theme),
        border: Border {
            // An empty box needs more edge than a field to read on a
            // light theme.
            color: if checked {
                accent
            } else {
                chrome::hover_border(theme)
            },
            width: km::line(),
            radius: 0.0.into(),
        },
        text_color: None,
    }
}

/// A check box `km::FORM_LABEL_X` in, then `label` `km::FORM_CHECK_GAP`
/// after it; a press on either sends `on_toggle`. Without it, disabled.
pub fn check<'a>(
    name: impl Into<String>,
    checked: bool,
    on_toggle: Option<Message>,
) -> Element<'a, Message> {
    let enabled = on_toggle.is_some();
    let box_ = checkbox(checked)
        .on_toggle_maybe(on_toggle.clone().map(|message| move |_| message.clone()))
        .size(km::FORM_CHECK)
        .style(check_style);
    let mut name = mouse_area(container(row_label(name.into(), enabled)).width(Length::Fill));
    if let Some(message) = on_toggle {
        name = name.on_press(message);
    }
    container(
        iced::widget::row![box_, name]
            .spacing(km::FORM_CHECK_GAP)
            .align_y(iced::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(km::FORM_ROW_H))
    .padding(Padding::ZERO.left(km::FORM_LABEL_X))
    .align_y(iced::Center)
    .into()
}

/// [`check`] in the left half and `control` in the right half. The caller
/// disables `control` (a [`select`] with `disabled`) while the box is
/// cleared.
pub fn check_row<'a>(
    name: impl Into<String>,
    checked: bool,
    on_toggle: Option<Message>,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    halves(check(name, checked, on_toggle), control)
}

/// A choice field: `shown` in a 1 px bordered box with square corners and
/// the caret at its right, `km::FORM_ROW_H` high. A press sends `toggle`;
/// while `open`, `list` (`list_w` wide) hangs under it, and a press outside
/// sends `toggle` again. `disabled`: the text muted, nothing to press.
pub fn select<'a>(
    shown: impl Into<String>,
    open: bool,
    toggle: Message,
    list: DropdownList<'a, Message>,
    list_w: f32,
    disabled: bool,
) -> Element<'a, Message> {
    let caret = if open {
        crate::ui::icons::themed_arrow_up(km::FORM_CARET)
    } else {
        crate::ui::icons::themed_arrow_down(km::FORM_CARET)
    };
    let head = btn(iced::widget::row![
        row_label(shown.into(), !disabled).width(Length::Fill),
        caret,
    ]
    .align_y(iced::Center))
    .on_press_maybe((!disabled).then(|| toggle.clone()))
    .width(Length::Fill)
    .height(Length::Fixed(km::FORM_ROW_H))
    .padding(
        Padding::ZERO
            .left(km::FORM_SELECT_TEXT_X)
            .right(km::FORM_SELECT_TEXT_X),
    )
    .style(move |theme: &Theme, status| {
        let hovered = matches!(status, btn::Status::Hovered | btn::Status::Pressed);
        btn::Style {
            background: Some(Background::Color(if hovered || open {
                chrome::surface_raised(theme)
            } else {
                chrome::surface(theme)
            })),
            text_color: chrome::text(theme),
            border: Border {
                color: if open {
                    chrome::accent(theme)
                } else {
                    chrome::border(theme)
                },
                width: km::line(),
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    });
    if disabled {
        return head.into();
    }
    super::dropdown(head.into(), list, list_w, toggle, open)
}

/// A row of choice tiles, two side by side: each an icon in a
/// `FORM_TILE_ICON_W` × `FORM_TILE_ICON_H` box and its label. The chosen one
/// has the accent edge and a light accent fill; a press on another sends
/// its message.
pub fn tiles<'a, T: Copy + PartialEq + 'a>(
    options: Vec<(T, &'static [u8], String)>,
    selected: T,
    on_select: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let tiles: Vec<Element<'a, Message>> = options
        .into_iter()
        .map(|(value, icon, name)| {
            let on = value == selected;
            let content = iced::widget::row![
                container(crate::ui::icons::tool(icon, km::FORM_TILE_ICON_H))
                    .width(Length::Fixed(km::FORM_TILE_ICON_W))
                    .height(Length::Fixed(km::FORM_TILE_ICON_H))
                    .center(Length::Fixed(km::FORM_TILE_ICON_W)),
                super::one_line(label(name))
                    .size(text::UI)
                    .line_height(text::line()),
            ]
            .spacing(km::FORM_TILE_ICON_GAP)
            .align_y(iced::Center);
            btn(content)
                .on_press(on_select(value))
                .width(Length::Fill)
                .height(Length::Fixed(km::FORM_TILE_H))
                .padding(km::FORM_TILE_PAD)
                .style(move |theme: &Theme, status| {
                    let hovered = matches!(status, btn::Status::Hovered | btn::Status::Pressed);
                    let fill = if on {
                        let alpha = if chrome::is_dark(theme) {
                            km::FORM_TILE_FILL_DARK
                        } else {
                            km::FORM_TILE_FILL_LIGHT
                        };
                        Some(chrome::accent(theme).scale_alpha(alpha))
                    } else if hovered {
                        Some(chrome::hover(theme))
                    } else {
                        None
                    };
                    btn::Style {
                        background: fill.map(Background::Color),
                        text_color: if on {
                            chrome::strong(theme)
                        } else {
                            chrome::text(theme)
                        },
                        border: Border {
                            color: if on {
                                chrome::accent(theme)
                            } else {
                                chrome::border(theme)
                            },
                            width: km::line(),
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .into()
        })
        .collect();
    container(iced::widget::row(tiles).spacing(km::FORM_TILE_GAP))
        .width(Length::Fill)
        .padding(
            Padding::ZERO
                .top(km::FORM_TILES_TOP)
                .bottom(km::FORM_TILES_BOTTOM),
        )
        .into()
}

/// A note under a row, in line with the labels after the check boxes: muted,
/// or in the theme's warning colour when `warn`. It wraps rather than being
/// cut.
pub fn hint<'a>(note: impl Into<String>, warn: bool) -> Element<'a, Message> {
    container(
        label(note.into())
            .size(text::UI)
            .line_height(text::line())
            .style(move |theme: &Theme| iced::widget::text::Style {
                color: Some(if warn {
                    chrome::warning(theme)
                } else {
                    chrome::muted(theme)
                }),
            }),
    )
    .width(Length::Fill)
    .padding(Padding::ZERO.left(km::FORM_CHECK_LABEL_X))
    .align_y(iced::Center)
    .into()
}

/// A footer: `buttons` at its right, on the raised surface.
pub fn footer<'a>(buttons: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut content = iced::widget::row![Space::new().width(Length::Fill)]
        .spacing(km::FORM_TILE_GAP)
        .align_y(iced::Center);
    for button in buttons {
        content = content.push(button);
    }
    container(content)
        .width(Length::Fill)
        .padding(Padding {
            top: km::FORM_FOOTER_PAD_Y,
            bottom: km::FORM_FOOTER_PAD_Y,
            right: km::FORM_FOOTER_PAD_RIGHT,
            left: km::FORM_FOOTER_PAD_LEFT,
        })
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::title_bar(theme))),
            text_color: Some(chrome::text(theme)),
            ..Default::default()
        })
        .into()
}

/// A text button with a border: transparent, lit under the pointer.
pub fn button<'a>(caption: impl Into<String>, on_press: Message) -> Element<'a, Message> {
    iced::widget::button(
        container(
            label(caption.into())
                .size(text::UI)
                .line_height(text::line()),
        )
        .center_y(Length::Fill),
    )
    .on_press(on_press)
    .height(Length::Fixed(km::FORM_BUTTON_H))
    .padding(
        Padding::ZERO
            .left(km::FORM_BUTTON_PAD_X)
            .right(km::FORM_BUTTON_PAD_X),
    )
    .style(|theme: &Theme, status| {
        let hovered = matches!(status, btn::Status::Hovered | btn::Status::Pressed);
        btn::Style {
            background: hovered.then(|| Background::Color(chrome::hover(theme))),
            text_color: chrome::text(theme),
            border: Border {
                color: chrome::border(theme),
                width: km::line(),
                radius: km::FORM_BUTTON_RADIUS.into(),
            },
            ..Default::default()
        }
    })
    .into()
}
