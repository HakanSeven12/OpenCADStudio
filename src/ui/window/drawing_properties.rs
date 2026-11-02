//! Drawing Properties dialog (DWGPROPS): read-only General tab, editable
//! Summary tab and a name/value table of custom properties.

use crate::app::Message;
use crate::ui::style::form::{dialog_button, field_style, hdivider, labeled_field};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Fill, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Summary,
    Custom,
}

#[derive(Debug, Clone, Copy)]
pub enum SummaryField {
    Title,
    Subject,
    Author,
    Keywords,
    Comments,
    HyperlinkBase,
}

#[derive(Debug, Clone)]
pub enum DwgPropsMsg {
    Tab(Tab),
    Summary(SummaryField, String),
    CustomName(usize, String),
    CustomValue(usize, String),
    AddRow,
    RemoveRow(usize),
    Ok,
}

pub(crate) struct State {
    pub tab: Tab,
    /// The summary being edited; the document keeps its own until OK.
    pub summary: codec::document::SummaryInfo,
    /// Read-only General tab: label, value.
    pub general: Vec<(String, String)>,
    /// Why OK was refused, shown under the table.
    pub error: Option<String>,
}

fn field(label: String, value: &str, which: SummaryField) -> Element<'_, Message> {
    labeled_field(
        label.into(),
        value,
        move |v| Message::DwgProps(DwgPropsMsg::Summary(which, v)),
        320.0,
    )
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let tab_button = |tab: Tab, label: String| {
        let is_active = state.tab == tab;
        button(text(label).size(11))
            .on_press(Message::DwgProps(DwgPropsMsg::Tab(tab)))
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let pair = match (is_active, status) {
                    (true, _) => palette.primary.strong,
                    (false, button::Status::Hovered | button::Status::Pressed) => {
                        palette.background.strong
                    }
                    _ => palette.background.weak,
                };
                button::Style {
                    background: Some(Background::Color(pair.color)),
                    text_color: pair.text,
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: iced::border::Radius {
                            top_left: 4.0,
                            top_right: 4.0,
                            bottom_right: 0.0,
                            bottom_left: 0.0,
                        },
                    },
                    ..Default::default()
                }
            })
            .padding([4, 8])
    };
    let tab_row = row![
        tab_button(Tab::General, crate::tr!("dwgprops-dialog", "tab-general")),
        tab_button(Tab::Summary, crate::tr!("dwgprops-dialog", "tab-summary")),
        tab_button(Tab::Custom, crate::tr!("dwgprops-dialog", "tab-custom")),
    ]
    .spacing(2);

    let body: Element<'_, Message> = match state.tab {
        Tab::General => {
            let mut rows = column![].spacing(8);
            for (label, value) in &state.general {
                rows = rows.push(
                    row![
                        text(label.as_str()).size(11).width(150),
                        text(value.as_str()).size(12),
                    ]
                    .spacing(8),
                );
            }
            scrollable(rows).height(Fill).into()
        }
        Tab::Summary => {
            let s = &state.summary;
            column![
                field(
                    crate::tr!("dwgprops-dialog", "title-field"),
                    &s.title,
                    SummaryField::Title
                ),
                field(
                    crate::tr!("dwgprops-dialog", "subject"),
                    &s.subject,
                    SummaryField::Subject
                ),
                field(
                    crate::tr!("dwgprops-dialog", "author"),
                    &s.author,
                    SummaryField::Author
                ),
                field(
                    crate::tr!("dwgprops-dialog", "keywords"),
                    &s.keywords,
                    SummaryField::Keywords
                ),
                field(
                    crate::tr!("dwgprops-dialog", "comments"),
                    &s.comments,
                    SummaryField::Comments
                ),
                field(
                    crate::tr!("dwgprops-dialog", "hyperlink-base"),
                    &s.hyperlink_base,
                    SummaryField::HyperlinkBase
                ),
            ]
            .spacing(8)
            .into()
        }
        Tab::Custom => {
            let mut rows = column![row![
                text(crate::tr!("dwgprops-dialog", "name"))
                    .size(11)
                    .width(Fill),
                text(crate::tr!("dwgprops-dialog", "value"))
                    .size(11)
                    .width(Fill),
                Space::new().width(70),
            ]
            .spacing(6)]
            .spacing(6);
            for (index, (name, value)) in state.summary.custom_properties.iter().enumerate() {
                rows = rows.push(
                    row![
                        text_input("", name)
                            .on_input(move |v| Message::DwgProps(DwgPropsMsg::CustomName(index, v)))
                            .style(field_style)
                            .size(12)
                            .width(Fill),
                        text_input("", value)
                            .on_input(move |v| {
                                Message::DwgProps(DwgPropsMsg::CustomValue(index, v))
                            })
                            .style(field_style)
                            .size(12)
                            .width(Fill),
                        button(text(crate::tr!("dwgprops-dialog", "remove")).size(11))
                            .on_press(Message::DwgProps(DwgPropsMsg::RemoveRow(index)))
                            .width(70),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                );
            }
            column![
                container(scrollable(rows).height(Fill))
                    .style(container::bordered_box)
                    .padding(8)
                    .height(Fill)
                    .width(Fill),
                button(text(crate::tr!("dwgprops-dialog", "add")).size(11))
                    .on_press(Message::DwgProps(DwgPropsMsg::AddRow)),
            ]
            .spacing(8)
            .height(Fill)
            .into()
        }
    };

    let error: Element<'_, Message> = match &state.error {
        Some(message) => text(message.as_str()).size(12).style(text::danger).into(),
        None => Space::new().height(0).into(),
    };
    let buttons = row![
        Space::new().width(Fill),
        dialog_button(
            crate::tr!("dwgprops-dialog", "ok"),
            Message::DwgProps(DwgPropsMsg::Ok),
            true
        ),
        dialog_button(
            crate::tr!("dwgprops-dialog", "cancel"),
            Message::CloseModal,
            false
        ),
    ]
    .spacing(8)
    .align_y(iced::Center);
    container(
        column![
            tab_row,
            hdivider(sizing.width),
            body,
            error,
            hdivider(sizing.width),
            buttons
        ]
        .spacing(10),
    )
    .padding(16)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
