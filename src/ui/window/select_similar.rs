//! Select Similar settings: which properties besides the type must match.

use crate::app::Message;
use iced::widget::{button, checkbox, column, container, row, text, Space};
use iced::{Element, Fill};

pub(crate) fn view_window(mode: u16) -> Element<'static, Message> {
    let labels = [
        (1, crate::tr!("select-similar", "color")),
        (2, crate::tr!("select-similar", "layer")),
        (4, crate::tr!("select-similar", "linetype")),
        (8, crate::tr!("select-similar", "linetype-scale")),
        (16, crate::tr!("select-similar", "lineweight")),
        (32, crate::tr!("select-similar", "plot-style")),
        (64, crate::tr!("select-similar", "object-style")),
        (128, crate::tr!("select-similar", "name")),
    ];
    let mut boxes = column![text(crate::tr!("select-similar", "based-on")).size(13)].spacing(8);
    for (bit, label) in labels {
        boxes = boxes.push(
            checkbox(mode & bit != 0)
                .label(label)
                .on_toggle(move |_| Message::SelectSimilarSettingsToggle(bit)),
        );
    }
    let buttons = row![
        Space::new().width(Fill),
        button(text(crate::t!("OK").into_owned()))
            .on_press(Message::SelectSimilarSettingsOk)
            .style(button::primary),
        button(text(crate::t!("Cancel").into_owned())).on_press(Message::CloseModal),
    ]
    .spacing(8);
    container(column![boxes, buttons].spacing(16))
        .padding(18)
        .width(300)
        .into()
}
