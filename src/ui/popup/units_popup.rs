//! Drawing-units status menu.

use iced::widget::{button, row, text};
use iced::{Element, Fill};

use crate::app::Message;
use crate::ui::statusbar::status_menu::Entry;

/// Units offered in the picker: (INSUNITS code, menu label).
const UNITS: &[(i16, &str)] = &[
    (0, "Unitless"),
    (4, "Millimeters"),
    (5, "Centimeters"),
    (6, "Meters"),
    (7, "Kilometers"),
    (1, "Inches"),
    (2, "Feet"),
    (3, "Miles"),
    (10, "Yards"),
];

/// Short label shown on the status-bar pill for an INSUNITS code.
pub fn unit_short(code: i16) -> &'static str {
    match code {
        1 => "in",
        2 => "ft",
        3 => "mi",
        4 => "mm",
        5 => "cm",
        6 => "m",
        7 => "km",
        10 => "yd",
        0 => "Unitless",
        _ => "Unit",
    }
}

pub fn menu_entries(current: i16) -> Vec<Entry<'static>> {
    UNITS
        .iter()
        .map(|&(code, label)| {
            Entry::close(unit_row(
                label,
                code == current,
                Message::SetDrawingUnits(code),
            ))
        })
        .collect()
}

fn unit_row(label: &'static str, active: bool, msg: Message) -> Element<'static, Message> {
    let check = crate::ui::icons::themed_check_cell(active);

    let lbl = text(label).size(11);

    let content = row![check, lbl].spacing(6).align_y(iced::Center);

    button(content)
        .on_press(msg)
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}
