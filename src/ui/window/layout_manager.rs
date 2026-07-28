//! Layout Manager window — fills the entire OS window.

use crate::app::Message;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Element, Fill, Theme};

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().primary.base.color),
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

fn hdivider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color,
            )),
            ..Default::default()
        })
        .into()
}

fn vsep<'a>() -> Element<'a, Message> {
    container(Space::new().width(1).height(Fill))
        .width(1)
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color,
            )),
            ..Default::default()
        })
        .into()
}

pub fn view_window<'a>(
    layouts: Vec<String>,
    selected: &'a str,
    rename_buf: &'a str,
    current: String,
) -> Element<'a, Message> {
    let is_model = selected == "Model";

    // ── Toolbar ───────────────────────────────────────────────────────────
    let toolbar = container(
        row![
            button(text("New Layout").size(11))
                .on_press(Message::LayoutManagerNew)
                .style(btn_s(false))
                .padding([4, 10]),
            button(text("Delete").size(11))
                .on_press(Message::LayoutManagerDelete)
                .style(move |theme: &Theme, status| {
                    if is_model {
                        button::secondary(theme, status)
                    } else {
                        button::danger(theme, status)
                    }
                })
                .padding([4, 10]),
            Space::new().width(Fill),
            button(
                row![
                    crate::ui::icons::themed_arrow_left(9.0),
                    text("Move Left").size(11),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::LayoutManagerMoveLeft)
            .style(btn_s(false))
            .padding([4, 8]),
            button(
                row![
                    text("Move Right").size(11),
                    crate::ui::icons::themed_arrow_right(9.0),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::LayoutManagerMoveRight)
            .style(btn_s(false))
            .padding([4, 8]),
            button(text("Set Current").size(11))
                .on_press(Message::LayoutManagerSetCurrent)
                .style(btn_s(true))
                .padding([4, 10]),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.weakest.color,
        )),
        ..Default::default()
    })
    .width(Fill)
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
                .width(Fill)
                .into()
        })
        .collect();

    let layout_list = container(
        column![
            text("Layouts").size(10).style(muted_style),
            container(scrollable(column(list_items).spacing(2)).height(Fill))
                .style(container::bordered_box)
                .width(Fill)
                .height(Fill)
                .padding(2),
        ]
        .spacing(4)
        .height(Fill),
    )
    .width(220)
    .height(Fill)
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
                "Model Space"
            } else {
                "Paper Space Layout"
            })
            .size(13),
            Space::new().height(8),
            row![
                text("Name:").size(11).style(muted_style).width(80),
                text(selected).size(11),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                text("Status:").size(11).style(muted_style).width(80),
                text(if selected == current.as_str() {
                    "Active"
                } else {
                    "Inactive"
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
            text("Rename").size(10).style(muted_style),
            row![
                text_input("New name…", rename_buf)
                    .on_input(Message::LayoutManagerRenameBuf)
                    .on_submit(Message::LayoutManagerRenameCommit)
                    .size(11)
                    .padding([4, 8]),
                button(text("OK").size(11))
                    .on_press(Message::LayoutManagerRenameCommit)
                    .style(btn_s(true))
                    .padding([4, 10]),
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(8),
    )
    .width(Fill)
    .padding([12, 12]);

    let body = row![layout_list, vsep(), details].height(Fill);

    container(column![toolbar, hdivider(), body].spacing(0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color,
            )),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into()
}
