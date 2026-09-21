//! Layout Manager window — fills the entire OS window.

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{hdivider, vsep};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Element, Theme};
use crate::t;

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn btn_s(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        if accent {
            button::primary(theme, status)
        } else {
            button::secondary(theme, status)
        }
    }
}

fn list_item(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        if active {
            button::primary(theme, status)
        } else {
            button::subtle(theme, status)
        }
    }
}

pub fn view_window<'a>(
    layouts: Vec<String>,
    selected: &'a str,
    rename_buf: &'a str,
    current: String,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let is_model = selected == "Model";

    // ── Toolbar ───────────────────────────────────────────────────────────
    let toolbar = container(
        row![
            button(text(t!("New Layout")).size(11))
                .on_press(Message::LayoutManagerNew)
                .style(btn_s(false))
                .padding([4, 10]),
            button(text(t!("Delete")).size(11))
                .on_press(Message::LayoutManagerDelete)
                .style(move |theme: &Theme, status| {
                    if is_model {
                        button::secondary(theme, status)
                    } else {
                        button::danger(theme, status)
                    }
                })
                .padding([4, 10]),
            Space::new().width(sizing.width),
            button(
                row![
                    crate::ui::icons::themed_arrow_left(9.0),
                    text(t!("Move Left")).size(11),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::LayoutManagerMoveLeft)
            .style(btn_s(false))
            .padding([4, 8]),
            button(
                row![
                    text(t!("Move Right")).size(11),
                    crate::ui::icons::themed_arrow_right(9.0),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::LayoutManagerMoveRight)
            .style(btn_s(false))
            .padding([4, 8]),
            button(text(t!("Set Current")).size(11))
                .on_press(Message::LayoutManagerSetCurrent)
                .style(btn_s(true))
                .padding([4, 10]),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.weakest.color,
        )),
        ..Default::default()
    })
    .width(sizing.width)
    .padding([5, 8]);

    // ── Left: Layout list ─────────────────────────────────────────────────
    let list_items: Vec<Element<'_, Message>> = layouts
        .iter()
        .map(|name| {
            let is_sel = name.as_str() == selected;
            let is_cur = name.as_str() == current.as_str();
            let mut item_row = row![text(name.clone()).size(12)]
                .spacing(5)
                .align_y(iced::Center);
            if is_cur {
                item_row = item_row.push(crate::ui::icons::themed_arrow_left(8.0));
            }
            button(item_row)
                .on_press(Message::LayoutManagerSelect(name.clone()))
                .style(list_item(is_sel))
                .padding([5, 10])
                .width(sizing.width)
                .into()
        })
        .collect();

    let layout_list = container(
        column![
            text(t!("Layouts")).size(10).style(muted_style),
            container(scrollable(column(list_items).spacing(2)).height(sizing.height))
                .style(container::bordered_box)
                .width(sizing.width)
                .height(sizing.height)
                .padding(2),
        ]
        .spacing(4)
        .height(sizing.height),
    )
    .width(220)
    .height(sizing.height)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    // ── Right: Details + rename ───────────────────────────────────────────
    let details = container(
        column![
            text(if is_model {
                t!("Model Space")
            } else {
                t!("Paper Space Layout")
            })
            .size(13),
            Space::new().height(8),
            row![
                text(t!("Name:")).size(11).style(muted_style).width(80),
                text(selected).size(11),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                text(t!("Status:")).size(11).style(muted_style).width(80),
                text(if selected == current.as_str() {
                    t!("Active")
                } else {
                    t!("Inactive")
                })
                .size(11)
                .style(move |theme: &Theme| {
                    if selected == current.as_str() {
                        primary_style(theme)
                    } else {
                        muted_style(theme)
                    }
                }),
            ]
            .spacing(8)
            .align_y(iced::Center),
            Space::new().height(16),
            text(t!("Rename")).size(10).style(muted_style),
            row![
                text_input(t!("New name…").as_ref(), rename_buf)
                    .on_input(Message::LayoutManagerRenameBuf)
                    .on_submit(Message::LayoutManagerRenameCommit)
                    .size(11)
                    .padding([4, 8]),
                button(text(t!("OK")).size(11))
                    .on_press(Message::LayoutManagerRenameCommit)
                    .style(btn_s(true))
                    .padding([4, 10]),
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(8),
    )
    .width(sizing.width)
    .padding([12, 12]);

    let body = row![layout_list, vsep(sizing.height), details].height(sizing.height);

    container(column![toolbar, hdivider(sizing.width), body].spacing(0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.base.color,
            )),
            ..Default::default()
        })
        .width(sizing.width)
        .height(sizing.height)
        .into()
}
