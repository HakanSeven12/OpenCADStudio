//! Linear-format status menu — the button that decides how every coordinate
//! and distance in the drawing is written.
//!
//! It offers LUNITS, not INSUNITS. Picking "Architectural" changes what the
//! readout, the properties panel and every dimension say; picking a *unit*
//! changes nothing anybody can see, which is why that choice lives in the UNITS
//! dialog next to the insertion scale it belongs to. (#668)

use iced::widget::{button, container, row, text, Space};
use iced::{Background, Element, Fill, Theme};

use crate::app::Message;
use crate::t;
use crate::ui::statusbar::status_menu::Entry;

use crate::modules::draw::units;

pub fn menu_entries(current: i16) -> Vec<Entry<'static>> {
    let mut entries: Vec<Entry<'static>> = units::linear_formats()
        .map(|(code, label, sample)| {
            Entry::close(format_row(
                label,
                sample,
                code == current,
                Message::SetLinearFormat(code),
            ))
        })
        .collect();

    // Precision, angles and the insertion unit are all part of the same
    // decision and all live one dialog away, so the way there is offered here
    // rather than left to be found.
    entries.push(Entry::stay(divider()));
    entries.push(Entry::close(link(
        t!("Units\u{2026}").into_owned(),
        "UNITS",
    )));
    entries.push(Entry::close(link(
        t!("Convert drawing\u{2026}").into_owned(),
        "DWGUNITS",
    )));
    entries
}

fn divider() -> Element<'static, Message> {
    container(Space::new().height(1))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .width(Fill)
        .padding([0, 4])
        .into()
}

fn link(label: String, command: &'static str) -> Element<'static, Message> {
    button(text(label).size(11))
        .on_press(Message::Command(command.into()))
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}

/// One format, shown with a sample of itself so the difference between
/// Engineering and Architectural is visible rather than recalled.
fn format_row(
    label: &'static str,
    sample: &'static str,
    active: bool,
    msg: Message,
) -> Element<'static, Message> {
    let content = row![
        crate::ui::icons::themed_check_cell(active),
        text(t!(label)).size(11),
        Space::new().width(Fill),
        text(sample).size(10).style(|theme: &Theme| {
            iced::widget::text::Style {
                color: Some(crate::ui::style::readable::muted_text(theme)),
            }
        }),
    ]
    .spacing(6)
    .align_y(iced::Center);

    button(content)
        .on_press(msg)
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}
