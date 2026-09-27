use crate::app::Message;
use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};

pub fn view() -> Element<'static, Message> {
    let content = column![
        text("Hatch settings").size(12),
        text("Additional hatch settings will be available here.").size(11),
        Space::new().height(12),
        container(
            row![button(text(crate::t!("Close")).size(11))
                .on_press(Message::HatchEditorClose)
                .style(button::secondary)
                .padding([5, 14])]
            .width(Fill),
        )
        .width(Fill)
        .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(8)
    .padding(10);
    let panel = container(content)
        .width(Length::Fixed(390.0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            border: Border {
                color: theme.palette().primary.base.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
    mouse_area(panel)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}
