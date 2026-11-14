//! A dropdown: a field-style head with a caret, and under it a list that
//! starts at the field's left edge, at least the field's width and as wide
//! as its longest option. The list is the shared [`DropdownList`] (arrows,
//! Enter, type-ahead, Escape) in the palette menu's frame; its options light
//! like palette menu items.
//!
//! The open state lives in the caller's own state, so opening and closing
//! needs no widget-tree handle and the keys the list takes always match the
//! list on screen.

use iced::widget::{button, container, mouse_area, row, text};
use iced::{Background, Element, Length, Theme};

use crate::app::Message;
use crate::ui::dropdown_list::{DropdownList, DropdownRow};
use crate::ui::style::{chrome, palette_metrics as pm, text as ui_text};

/// An option: lit under the pointer, and lit when it is the `current` value.
pub fn dropdown_option_style(current: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| button::Style {
        background: (current
            || matches!(status, button::Status::Hovered | button::Status::Pressed))
        .then(|| Background::Color(chrome::hover(theme))),
        text_color: chrome::text(theme),
        ..Default::default()
    }
}

/// A row of a list in a dropdown or popup that is not a choice (a
/// command, "Select Color..."): lit under the pointer.
pub fn dropdown_row_style(theme: &Theme, status: button::Status) -> button::Style {
    dropdown_option_style(false)(theme, status)
}

/// The head: `shown` on one line (in hint colour when `muted`, as for a
/// mixed selection) and a caret that points up while `open`. A click
/// anywhere in it sends `toggle`.
pub fn dropdown_head<'a>(
    shown: impl Into<String>,
    muted: bool,
    open: bool,
    toggle: Message,
) -> Element<'a, Message> {
    let caret = button(
        container(if open {
            crate::ui::icons::themed_arrow_up(9.0)
        } else {
            crate::ui::icons::themed_arrow_down(9.0)
        })
        .align_y(iced::Center),
    )
    .on_press(toggle.clone())
    .style(super::cell_button_style)
    .padding([0.0, pm::CELL_PAD_X]);
    let caret = super::on_selected_row(caret);
    let label = super::one_line(text(shown.into()))
        .size(ui_text::UI)
        .line_height(ui_text::line())
        .width(Length::Fill);
    let label = if muted {
        label.style(|theme: &Theme| text::Style {
            color: Some(crate::ui::style::readable::hint_text(theme)),
        })
    } else {
        label
    };
    let head = container(row![label, caret].align_y(iced::Center))
        .style(super::combo_container_style)
        .padding([0.0, pm::CELL_PAD_X])
        .width(Length::Fill);
    mouse_area(head).on_press(toggle).into()
}

/// One option of the list: `label` on one line; picking it, by a click or
/// by Enter on the highlighted row, sends `message`. `key` is the text
/// type-ahead matches.
pub fn dropdown_option<'a>(
    label: impl Into<String>,
    key: impl Into<String>,
    message: Message,
    current: bool,
) -> DropdownRow<'a, Message> {
    let item = button(
        super::one_line(text(label.into()))
            .size(ui_text::UI)
            .line_height(ui_text::line()),
    )
    .on_press(message.clone())
    .style(dropdown_option_style(current))
    .padding([2.0, pm::DROPDOWN_ITEM_PAD_X])
    .width(Length::Fill);
    DropdownRow::item(item, key.into(), message)
}

/// The width a list needs for its longest option (`labels`), never
/// narrower than `field`, the head's width.
pub fn dropdown_width<'s>(labels: impl Iterator<Item = &'s str>, field: f32) -> f32 {
    let widest = labels
        .map(|label| crate::ui::style::text_metrics::width(label, ui_text::UI, false))
        .fold(0.0_f32, f32::max);
    (widest + 2.0 * pm::DROPDOWN_ITEM_PAD_X + 2.0 * pm::DROPDOWN_PAD + pm::SCROLLBAR_W)
        .max(field)
        .ceil()
}

/// `head` with `list` under it, `width` wide, while `open`; `dismiss` is
/// sent by a click outside it.
pub fn dropdown<'a>(
    head: Element<'a, Message>,
    list: DropdownList<'a, Message>,
    width: f32,
    dismiss: Message,
    open: bool,
) -> Element<'a, Message> {
    let popup = container(list)
        .style(crate::ui::palette_menu::frame_style)
        .padding(pm::DROPDOWN_PAD);
    crate::ui::color_select::editable_drop_down_below(
        head,
        popup.into(),
        Some(Length::Fixed(width)),
        Length::Shrink,
        dismiss,
        open,
    )
}

#[cfg(test)]
mod tests {
    use super::dropdown_width;

    #[test]
    fn a_list_is_never_narrower_than_its_field() {
        assert_eq!(dropdown_width(["a"].into_iter(), 120.0), 120.0);
        let long = "a much longer option than the field can show";
        assert!(dropdown_width([long].into_iter(), 40.0) > 40.0);
    }
}
