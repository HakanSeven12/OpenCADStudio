use crate::app::settings::AutoConstrainSettings;
use crate::app::Message;
use crate::ui::style::form::dialog_button;
use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

pub fn view_window<'a>(
    settings: &'a AutoConstrainSettings,
    selected_row: usize,
    distance_input: &'a str,
    angle_input: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let heading = row![
        text(crate::t!("Priority")).width(70),
        text(crate::t!("Constraint Type")).width(Fill),
        text(crate::t!("Apply")).width(60),
    ]
    .spacing(8);

    let mut list = column![heading].spacing(3);
    for (index, kind) in settings.priority.iter().copied().enumerate() {
        let enabled = settings.enabled.contains(&kind);
        let label = row![
            text(format!("{}", index + 1)).width(70),
            text(crate::t!(kind.label())).width(Fill),
            checkbox(enabled)
                .on_toggle(move |_| Message::AutoConstrainToggleKind(kind))
                .width(60),
        ]
        .spacing(8)
        .align_y(iced::Center);
        list = list.push(
            button(label)
                .on_press(Message::AutoConstrainSelectRow(index))
                .style(if selected_row == index {
                    button::primary
                } else {
                    button::text
                })
                .width(Fill)
                .padding([5, 8]),
        );
    }

    let reorder = column![
        button(text(crate::t!("Move Up")))
            .on_press_maybe((selected_row > 0).then_some(Message::AutoConstrainMoveUp))
            .width(Fill),
        button(text(crate::t!("Move Down")))
            .on_press_maybe(
                (selected_row + 1 < settings.priority.len())
                    .then_some(Message::AutoConstrainMoveDown)
            )
            .width(Fill),
        Space::new().height(8),
        button(text(crate::t!("Select All")))
            .on_press(Message::AutoConstrainSelectAll)
            .width(Fill),
        button(text(crate::t!("Clear All")))
            .on_press(Message::AutoConstrainClearAll)
            .width(Fill),
        button(text(crate::t!("Reset")))
            .on_press(Message::AutoConstrainReset)
            .width(Fill),
    ]
    .spacing(6)
    .width(125);

    let types = row![
        container(scrollable(list).height(260))
            .padding(6)
            .width(Fill),
        reorder
    ]
    .spacing(12);

    let conditions = column![
        checkbox(settings.tangent_must_share_point)
            .label(crate::t!("Tangent objects must share an intersection point"))
            .on_toggle(|_| Message::AutoConstrainToggleTangentPoint),
        checkbox(settings.perpendicular_must_intersect)
            .label(crate::t!("Perpendicular objects must share an intersection point"))
            .on_toggle(|_| Message::AutoConstrainTogglePerpendicularIntersection),
    ]
    .spacing(8);

    let tolerances = column![
        text(crate::t!("Tolerances")).size(14),
        row![
            text(crate::t!("Distance")).width(130),
            text_input("0.05", distance_input)
                .on_input(Message::AutoConstrainDistanceChanged)
                .width(150),
        ]
        .align_y(iced::Center),
        row![
            text(crate::t!("Angle")).width(130),
            text_input("1", angle_input)
                .on_input(Message::AutoConstrainAngleChanged)
                .width(150),
            text("°"),
        ]
        .align_y(iced::Center),
    ]
    .spacing(7);

    let actions = row![
        Space::new().width(Fill),
        dialog_button(crate::t!("OK"), Message::AutoConstrainOk, true),
        dialog_button(crate::t!("Apply"), Message::AutoConstrainApply, false),
        dialog_button(crate::t!("Cancel"), Message::AutoConstrainCancel, false),
    ]
    .spacing(8)
    .align_y(iced::Center);

    container(
        column![
            text(crate::t!("Auto Constrain")).size(16),
            types,
            conditions,
            tolerances,
            Space::new().height(Fill),
            actions,
        ]
        .spacing(14)
        .width(sizing.width)
        .height(sizing.height),
    )
    .padding([14, 16])
    .width(sizing.width)
    .height(sizing.height)
    .style(container::rounded_box)
    .into()
}
