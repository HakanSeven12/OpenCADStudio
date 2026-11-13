//! The annotation scale question asked before the first annotative object of
//! a session: the scale the annotation is meant to show at, and whether to
//! ask again.

use iced::widget::{checkbox, column, pick_list, row, text, Space};
use iced::{Element, Fill};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::dialog_button_styled_opt;

/// The window's state: the drawing's scales and the one picked.
#[derive(Debug, Clone)]
pub struct AnnoScalePrompt {
    pub scales: Vec<String>,
    pub picked: String,
    pub dont_ask: bool,
}

#[derive(Debug, Clone)]
pub enum AnnoScaleMsg {
    Pick(String),
    DontAsk(bool),
    Ok,
}

fn msg(message: AnnoScaleMsg) -> Message {
    Message::AnnoScalePrompt(message)
}

pub fn view<'a>(
    state: &'a AnnoScalePrompt,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    column![
        row![
            crate::ui::icons::tinted(
                include_bytes!("../../../assets/icons/ui/annotative.svg"),
                28.0,
                |theme: &iced::Theme| theme.palette().primary.base.color,
            ),
            column![
                text(crate::tr!("anno-scale-prompt", "heading"))
                    .size(13)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::DEFAULT
                    }),
                text(crate::tr!("anno-scale-prompt", "note"))
                    .size(11)
                    .style(muted_style),
            ]
            .spacing(6),
        ]
        .spacing(12),
        row![
            text(crate::tr!("anno-scale-prompt", "scale")).size(12),
            pick_list(Some(state.picked.clone()), state.scales.clone(), |scale| scale.clone())
                .on_select(|scale| msg(AnnoScaleMsg::Pick(scale)))
                .text_size(12)
            .width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center),
        checkbox(state.dont_ask)
            .label(crate::tr!("anno-scale-prompt", "dont-ask"))
            .on_toggle(|on| msg(AnnoScaleMsg::DontAsk(on)))
            .size(13)
            .text_size(11),
        iced::widget::rule::horizontal(1),
        row![
            Space::new().width(Fill),
            dialog_button_styled_opt(
                std::borrow::Cow::Owned(crate::tr!("anno-scale-prompt", "ok")),
                Some(msg(AnnoScaleMsg::Ok)),
                iced::widget::button::primary
            ),
        ],
    ]
    .spacing(12)
    .padding([12, 14])
    .width(sizing.width)
    .into()
}
