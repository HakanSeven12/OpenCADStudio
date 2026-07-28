//! Polar-tracking angle status menu.

use iced::widget::{button, container, row, text, text_input};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::ui::statusbar::status_menu::Entry;

/// Angle increments offered in the picker, in degrees. Matches the common
/// drafting set and adds the fine 1° step requested in #264.
const PRESETS: &[f32] = &[90.0, 45.0, 30.0, 22.5, 18.0, 15.0, 10.0, 5.0, 1.0];

/// Format an angle without a trailing `.0` (so `22.5°` but `15°`).
pub fn angle_label(deg: f32) -> String {
    if (deg.fract()).abs() < 1e-3 {
        format!("{:.0}°", deg)
    } else {
        format!("{deg}°")
    }
}

pub fn menu_entries<'a>(
    current: f32,
    custom: &'a str,
) -> Vec<Entry<'a>> {
    let mut entries: Vec<Entry<'a>> = PRESETS
        .iter()
        .map(|&deg| {
            let active = (current - deg).abs() < 1e-3;
            Entry::close(angle_row(deg, active))
        })
        .collect();

    // Free-entry custom angle: type a value and press Enter to apply.
    let custom_field = text_input("Custom…", custom)
        .on_input(Message::PolarCustomInput)
        .on_submit(Message::SubmitPolarCustom)
        .size(11)
        .padding([2, 6])
        .width(Length::Fixed(58.0));
    let custom_row = container(
        row![
            custom_field,
            text("°").size(11),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .padding([5, 10]);
    entries.push(Entry::stay(custom_row));
    entries
}

fn angle_row<'a>(deg: f32, active: bool) -> Element<'a, Message> {
    let check = crate::ui::icons::themed_check_cell(active);

    let lbl = text(angle_label(deg)).size(11);

    let content = row![check, lbl].spacing(6).align_y(iced::Center);

    button(content)
        .on_press(Message::SetPolarAngle(deg))
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}
