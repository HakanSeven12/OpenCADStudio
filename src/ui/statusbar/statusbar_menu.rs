//! Status-bar customization and layout-list menu entries.

use iced::widget::{button, row, text};
use iced::{Background, Element, Fill, Theme};

use crate::app::Message;
use crate::ui::statusbar::statusbar_config::{StatusBarConfig, StatusPill};
use crate::ui::statusbar::status_menu::Entry;

pub fn customization_entries(config: &StatusBarConfig) -> Vec<Entry<'static>> {
    StatusPill::ALL
        .iter()
        .map(|&pill| {
            Entry::stay(menu_row(
                pill.label(),
                config.is_visible(pill),
                Message::ToggleStatusPill(pill),
            ))
        })
        .collect()
}

pub fn layout_entries<'a>(
    layouts: &[String],
    current: &str,
) -> Vec<Entry<'a>> {
    layouts
        .iter()
        .map(|name| Entry::close(layout_row(name.clone(), name == current)))
        .collect()
}

fn layout_row<'a>(name: String, is_current: bool) -> Element<'a, Message> {
    let lbl = text(name.clone()).size(11);
    button(row![lbl].align_y(iced::Center))
        .on_press(Message::LayoutSwitch(name))
        .style(move |theme: &Theme, status| {
            let mut style = button::subtle(theme, status);
            if is_current && status == button::Status::Active {
                let palette = theme.extended_palette();
                style.background = Some(Background::Color(palette.primary.weak.color));
                style.text_color = palette.primary.weak.text;
            }
            style
        })
        .width(Fill)
        .padding([4, 12])
        .into()
}

fn menu_row(label: &'static str, checked: bool, msg: Message) -> Element<'static, Message> {
    let check = crate::ui::icons::themed_check_cell(checked);

    let lbl = text(label).size(11);

    let content = row![check, lbl].spacing(6).align_y(iced::Center);

    button(content)
        .on_press(msg)
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}
