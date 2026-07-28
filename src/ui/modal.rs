//! Shared in-canvas modal overlay.
//!
//! Former pop-up *windows* (layer manager, style editors, About, …) render as
//! centered overlays on top of the main view instead of separate OS windows.
//! The native build has one main window and the web build has only the canvas,
//! so both stack dialogs here — one code path for every platform.

use crate::app::Message;
use iced::widget::{button, column, container, mouse_area, opaque, row, stack};
use iced::{Background, Border, Element, Length, Padding, Theme, Vector};

/// Stack `content` over `base` behind a dimmed backdrop, framed with a
/// draggable title bar (the ✕ close button at its right end). The backdrop only
/// dims and blocks clicks from reaching the view beneath — it does **not**
/// dismiss the dialog; closing is the ✕ button alone (`on_close`).
///
/// `offset` shifts the dialog from screen-centre so it can be dragged by its
/// title bar; pass `Vector::ZERO` to keep it centred.
pub fn modal<'a>(
    base: impl Into<Element<'a, Message>>,
    title: &'a str,
    title_width: f32,
    content: impl Into<Element<'a, Message>>,
    on_close: Message,
    offset: Vector,
    resizable: bool,
) -> Element<'a, Message> {
    let close = button(crate::ui::icons::themed_danger(
        crate::ui::icons::CLOSE,
        13.0,
    ))
    .on_press(on_close)
    .padding([1, 7])
    .style(close_style);

    // Draggable title bar: a grip handle next to the ✕. Kept `Shrink` (no
    // `Fill`) so a single Fill child can't blow the dialog out to the full
    // screen width — the dialog stays sized to its content. Pressing the grip
    // starts a drag (handled in `update`).
    let grip = mouse_area(
        container(crate::ui::icons::themed_primary(crate::ui::icons::MOVE, 14.0))
            .padding([1, 7])
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.extended_palette().background.weakest.color,
                )),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
    )
    .on_press(Message::ModalGrab)
    .interaction(iced::mouse::Interaction::Grab);

    // The dialog name is centred across the dialog width with the grip + ✕
    // overlaid at the right edge. The bar takes an explicit `title_width`
    // (the caller's content width) instead of `Fill` — a Fill child inside
    // the Shrink frame would blow the dialog out to the full screen.
    let title_text = iced::widget::text(title).size(15);
    let title_bar = stack![
        container(title_text)
            .width(Length::Fixed(title_width))
            .height(Length::Fixed(24.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center),
        container(row![grip, close].spacing(6).align_y(iced::Center))
            .width(Length::Fixed(title_width))
            .height(Length::Fixed(24.0))
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Center),
    ];

    let panel_style = |theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.base.color,
        )),
        border: Border {
            color: theme.extended_palette().background.neutral.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    };

    // The dialog is always sized to its content (the caller fixes the content's
    // width/height, growing it by the shared resize delta). When `resizable`, a
    // corner grip is appended bottom-right; dragging it drives `ModalResizeGrab`
    // + the shared drag move/release, which bumps that delta. The title bar sits
    // top-right above the content either way (`align_x(Right)`).
    let body = if resizable {
        let resize = mouse_area(
            container(crate::ui::icons::themed_primary(crate::ui::icons::RESIZE, 15.0))
                .padding([0, 2]),
        )
        .on_press(Message::ModalResizeGrab)
        .interaction(iced::mouse::Interaction::Grab);
        column![title_bar, content.into(), resize]
    } else {
        column![title_bar, content.into()]
    };
    let framed: Element<'a, Message> = container(
        body.spacing(6).align_x(iced::alignment::Horizontal::Right),
    )
    .padding(10)
    .style(panel_style)
    .into();

    // Position via asymmetric padding (padding is non-negative): shifting a
    // centred box by `d` on an axis needs (near − far) padding = 2·d there.
    let pad = Padding {
        top: offset.y.max(0.0) * 2.0,
        right: (-offset.x).max(0.0) * 2.0,
        bottom: (-offset.y).max(0.0) * 2.0,
        left: offset.x.max(0.0) * 2.0,
    };

    // The backdrop fills the screen so dragging keeps tracking the cursor even
    // when it leaves the title bar; release anywhere ends the drag.
    let backdrop = mouse_area(
        container(framed)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(pad)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme
                        .extended_palette()
                        .background
                        .strongest
                        .color
                        .scale_alpha(0.55),
                )),
                ..Default::default()
            }),
    )
    .on_move(Message::ModalDragMove)
    .on_release(Message::ModalDragRelease);

    stack![
        base.into(),
        // `opaque` blocks pointer events from passing through, so the dimmed
        // backdrop swallows clicks instead of closing or hitting the view.
        opaque(backdrop),
    ]
    .into()
}

fn close_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => palette.danger.strong,
        _ => palette.background.weakest,
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
