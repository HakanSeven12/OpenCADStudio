//! OpenCADStudio-style OSNAP status menu.

use iced::widget::{button, container, row, text};
use iced::{Background, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::snap::{SnapType, Snapper, ALL_SNAP_MODES};
use crate::ui::statusbar::status_menu::Entry;

pub fn menu_entries<'a>(snapper: &'a Snapper) -> Vec<Entry<'a>> {
    let all_on = snapper.all_on();
    let none_on = snapper.none_on();

    let header = row![
        header_btn("Select All", Message::SnapSelectAll, !all_on),
        header_btn("Clear All", Message::SnapClearAll, !none_on),
    ]
    .spacing(1)
    .padding([4u16, 8]);

    // Divider
    let divider = container(iced::widget::Space::new().height(1))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weak.color,
            )),
            ..Default::default()
        })
        .width(Fill)
        .padding([0, 4]);

    let mut entries = vec![Entry::stay(header), Entry::stay(divider)];
    for &(snap_type, _glyph, label) in ALL_SNAP_MODES {
        entries.push(Entry::stay(snap_row(
            snap_type,
            label,
            snapper.is_on(snap_type),
        )));
    }
    entries
}

// ── Individual snap row ───────────────────────────────────────────────────

fn snap_row<'a>(snap_type: SnapType, label: &'a str, active: bool) -> Element<'a, Message> {
    let checkmark = crate::ui::icons::themed_check_cell(active);

    // SVG marker (not a Unicode glyph) so the symbols render on the web build,
    // whose bundled font lacks them and showed tofu boxes. (#138)
    let icon_el = container(crate::ui::icons::themed_success::<Message>(
        crate::ui::icons::osnap(snap_type),
        13.0,
    ))
    .width(Length::Fixed(16.0))
    .align_x(iced::Center);

    let label_el = text(label).size(11);

    let content = row![checkmark, icon_el, label_el]
        .spacing(4)
        .align_y(iced::Center);

    button(content)
        .on_press(Message::ToggleSnap(snap_type))
        .style(button::subtle)
        .width(Fill)
        .padding([3, 8])
        .into()
}

fn header_btn(label: &str, msg: Message, enabled: bool) -> Element<'_, Message> {
    let b = button(text(label).size(10));
    let b = if enabled { b.on_press(msg) } else { b };
    b.style(button::secondary)
    .padding([3, 8])
    .into()
}
