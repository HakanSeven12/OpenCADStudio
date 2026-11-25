//! The surface list a Create Points > Surface command opens when Enter is
//! pressed at its surface prompt: the drawing's surfaces, one picked.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Background, Border, Element, Fill, Theme};

use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, dialog_button_styled_opt};
use crate::ui::window::pdf_dialogs::card;

/// The window's state: the surfaces and the one picked.
#[derive(Debug, Clone)]
pub struct SurfaceList {
    pub names: Vec<String>,
    pub picked: usize,
}

#[derive(Debug, Clone)]
pub enum SurfaceListMsg {
    Pick(usize),
    /// Pick the surface in the drawing instead.
    FromDrawing,
    Ok,
    Help,
}

fn msg(message: SurfaceListMsg) -> Message {
    Message::SurfaceList(message)
}

pub fn view<'a>(state: &'a SurfaceList, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let badge = container(crate::ui::icons::themed_primary(crate::ui::icons::MESH, 18.0))
        .width(34)
        .height(34)
        .center_x(34)
        .center_y(34)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().primary.base.color.scale_alpha(0.13))),
            border: Border { radius: 17.0.into(), ..Default::default() },
            ..Default::default()
        });
    let heading = row![
        badge,
        column![
            text(t!("Select a Surface")).size(13).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::DEFAULT
            }),
            text(t!("The points take their elevation from the surface picked."))
                .size(11)
                .style(muted_style),
        ]
        .spacing(3),
    ]
    .spacing(10)
    .align_y(iced::Center);
    let mut list = column![].spacing(2);
    for (k, name) in state.names.iter().enumerate() {
        list = list.push(
            button(text(name.as_str()).size(12))
                .on_press(msg(SurfaceListMsg::Pick(k)))
                .style(button_style(k == state.picked))
                .padding([5, 10])
                .width(Fill),
        );
    }
    column![
        heading,
        card(t!("Surfaces").into_owned(), list),
        row![
            Space::new().width(Fill),
            button(text(t!("Pick in drawing")).size(11))
                .on_press(msg(SurfaceListMsg::FromDrawing))
                .style(button_style(false))
                .padding([4, 10]),
        ],
        row![
            button(text("?").size(12))
                .on_press(msg(SurfaceListMsg::Help))
                .style(button_style(false))
                .padding([5, 11]),
            Space::new().width(Fill),
            dialog_button(t!("Cancel"), Message::CloseModal, false),
            dialog_button_styled_opt(t!("OK"), Some(msg(SurfaceListMsg::Ok)), button::primary),
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}
