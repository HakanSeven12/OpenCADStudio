//! Selection-cycling list box — shown at the cursor when a click lands on
//! two or more overlapping objects. Each row names a candidate; clicking it
//! adds that object to the current selection. Clicking outside dismisses it.

use iced::widget::{button, column, container, mouse_area, opaque, text};
use iced::{Element, Fill, Length};

use crate::app::Message;

/// Full-canvas overlay: the list box anchored at `anchor` (canvas
/// coordinates) plus a transparent click-catcher that cancels.
pub fn cycle_popup_overlay(
    anchor: iced::Point,
    items: Vec<(acadrust::Handle, String)>,
) -> Element<'static, Message> {
    let rows: Vec<Element<'static, Message>> = items
        .into_iter()
        .map(|(handle, label)| item_row(handle, label))
        .collect();

    let panel = container(column(rows))
        .style(container::bordered_box)
        .width(Length::Fixed(150.0));

    let positioned = crate::ui::pin_at(anchor, opaque(panel));

    mouse_area(positioned).on_press(Message::CycleCancel).into()
}

fn item_row(handle: acadrust::Handle, label: String) -> Element<'static, Message> {
    let content = text(label).size(11).align_y(iced::Center);
    let btn = button(content)
        .on_press(Message::CycleSelect(handle))
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10]);
    // Highlight the underlying object while the cursor is over this row.
    mouse_area(btn)
        .on_enter(Message::CycleHover(Some(handle)))
        .on_exit(Message::CycleHoverExit(handle))
        .into()
}
