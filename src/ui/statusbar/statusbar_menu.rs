//! Status-bar customization and layout-list menu entries.

use iced::Element;

use crate::app::Message;
use crate::t;
use crate::ui::statusbar::status_menu::Entry;
use crate::ui::statusbar::statusbar_config::{StatusBarConfig, StatusPill};

pub fn customization_entries(config: &StatusBarConfig) -> Vec<Entry<'static>> {
    StatusPill::ALL
        .iter()
        .map(|&pill| {
            Entry::stay(menu_row(
                t!(pill.label()),
                config.is_visible(pill),
                Message::ToggleStatusPill(pill),
            ))
        })
        .collect()
}

pub fn layout_entries<'a>(layouts: &[String], current: &str) -> Vec<Entry<'a>> {
    layouts
        .iter()
        .map(|name| Entry::close(layout_row(name.clone(), name == current)))
        .collect()
}

/// A layout, the current one marked: a palette menu item.
fn layout_row<'a>(name: String, is_current: bool) -> Element<'a, Message> {
    crate::ui::palette_menu::entry(
        crate::ui::palette_menu::Item::new(name.clone(), Message::LayoutSwitch(name))
            .mark(crate::ui::palette_menu::Mark::Radio(is_current)),
    )
}

/// A status pill shown or hidden: a palette menu item with its check.
fn menu_row(
    label: std::borrow::Cow<'static, str>,
    checked: bool,
    msg: Message,
) -> Element<'static, Message> {
    crate::ui::palette_menu::entry(
        crate::ui::palette_menu::Item::new(label, msg)
            .mark(crate::ui::palette_menu::Mark::Check(checked)),
    )
}
