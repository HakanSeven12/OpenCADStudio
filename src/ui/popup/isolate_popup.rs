//! Isolate / Hide / End Isolation status menu.

use iced::widget::{button, row, text};
use iced::{Element, Fill};

use crate::app::Message;
use crate::ui::statusbar::status_menu::Entry;

/// - `has_selection`: enables Isolate / Hide (they act on the selection).
/// - `isolation_active`: enables End Isolation (something is hidden).
pub fn menu_entries(
    has_selection: bool,
    isolation_active: bool,
) -> Vec<Entry<'static>> {
    vec![
        action_entry(
            "Isolate Objects",
            has_selection,
            Message::Command("ISOLATEOBJECTS".to_string()),
        ),
        action_entry(
            "Hide Objects",
            has_selection,
            Message::Command("HIDEOBJECTS".to_string()),
        ),
        action_entry(
            "End Isolation",
            isolation_active,
            Message::Command("UNISOLATEOBJECTS".to_string()),
        ),
    ]
}

fn action_entry(label: &'static str, enabled: bool, msg: Message) -> Entry<'static> {
    let row = action_row(label, enabled, msg);
    if enabled {
        Entry::close(row)
    } else {
        Entry::stay(row)
    }
}

fn action_row(label: &'static str, enabled: bool, msg: Message) -> Element<'static, Message> {
    let lbl = text(label).size(11);
    let content = row![lbl].align_y(iced::Center);

    let mut btn = button(content)
        .style(button::subtle)
        .width(Fill)
        .padding([4, 12]);
    if enabled {
        btn = btn.on_press(msg);
    }
    btn.into()
}
