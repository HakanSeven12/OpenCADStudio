//! The boundary warning an empty HATCH / GRADIENT pick shows.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Element, Fill};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::dialog_button;
use crate::ui::window::import_dialogs::warning_badge;
use crate::ui::window::pdf_dialogs::{chip, well_style};

/// The warning an empty-area pick shows.
#[derive(Debug, Clone, Default)]
pub struct BoundaryWarningState {
    pub details: bool,
    pub dont_show: bool,
    pub gap: f64,
}

#[derive(Debug, Clone)]
pub enum HatchDialogMsg {
    WarningDetails(bool),
    WarningDontShow(bool),
    WarningClose,
}

fn msg(m: HatchDialogMsg) -> Message {
    Message::HatchDialog(m)
}

pub fn view_warning<'a>(
    state: &'a BoundaryWarningState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let tr = |key: &str| crate::i18n::loader().get_attr("hatch-warning", key);
    let toggle = button(
        text(if state.details { tr("hide-details") } else { tr("show-details") }).size(11),
    )
    .on_press(msg(HatchDialogMsg::WarningDetails(!state.details)))
    .style(button::text)
    .padding([2, 0]);
    let mut text_column = column![
        text(tr("heading")).size(13).font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::DEFAULT
        }),
        text(tr("body")).size(11).style(muted_style),
        toggle,
    ]
    .spacing(8);
    if state.details {
        text_column = text_column.push(
            container(
                column![
                    text(crate::tr!(
                        "hatch-warning",
                        "detail-gap",
                        gap = state.gap.to_string()
                    ))
                    .size(11),
                    text(tr("detail-hint")).size(11).style(muted_style),
                ]
                .spacing(4),
            )
            .padding([8, 10])
            .width(Fill)
            .style(well_style),
        );
    }
    column![
        row![warning_badge(), text_column.width(Fill)].spacing(12),
        iced::widget::rule::horizontal(1),
        row![
            chip(
                tr("dont-show"),
                state.dont_show,
                msg(HatchDialogMsg::WarningDontShow(!state.dont_show))
            ),
            Space::new().width(Fill),
            dialog_button(
                std::borrow::Cow::Owned(tr("close")),
                msg(HatchDialogMsg::WarningClose),
                true
            ),
        ]
        .align_y(iced::Center),
    ]
    .spacing(12)
    .padding([12, 14])
    .width(sizing.width)
    .into()
}
