//! Shared style-manager list row.
//!
//! Every style manager renders the same left-hand list of style names where a
//! single click selects and a double click renames inline.
//!
//! The whole row is a `mouse_area` (not a `button`): an inner interactive
//! widget would capture the press and the parent `mouse_area` would never see
//! the second click, so `on_double_click` would never fire. `mouse_area`
//! carries both `on_press` (select) and `on_double_click` (rename) itself.

use crate::app::{Message, StyleKind};
use crate::ui::style::style_manager::StyleRow;
use iced::widget::{container, mouse_area, row, text, text_input};
use iced::{Background, Element, Fill, Theme};

const LOCK: &[u8] = include_bytes!("../../../assets/icons/ui/lock.svg");

/// Shared id for the inline rename field, so the rename-start handler can focus
/// it the moment the row turns editable.
pub fn rename_input_id() -> iced::widget::Id {
    iced::widget::Id::new("style-rename-input")
}

/// One row of the style list. Renders an editable `text_input` when `name` is
/// the style being renamed (`rename_active`), otherwise a selectable row whose
/// double click starts the rename. The current style gets a green ✓, a
/// referenced style a lock and a dimmed name; `info` adds the usage count.
#[allow(clippy::too_many_arguments)]
pub fn item<'a>(
    name: &str,
    is_current: bool,
    is_selected: bool,
    kind: StyleKind,
    on_select: Message,
    rename_active: Option<&str>,
    rename_buf: &'a str,
    can_rename: bool,
    info: Option<StyleRow>,
) -> Element<'a, Message> {
    if rename_active == Some(name) {
        text_input("", rename_buf)
            .id(iced::widget::Id::new("style-rename-input"))
            .on_input(Message::StyleRenameEdit)
            .on_submit(Message::StyleRenameCommit(kind))
            .size(11)
            .padding([4, 8])
            .width(Fill)
            .into()
    } else {
        let locked = info.is_some_and(|info| info.locked);
        // Fixed-width ✓ column keeps every name left-aligned whether or not the
        // row is current.
        let mark: Element<'a, Message> = if locked {
            container(crate::ui::icons::themed_secondary(LOCK, 11.0))
                .width(16)
                .into()
        } else {
            crate::ui::icons::themed_check_cell(is_current)
        };
        let name_text = text(name.to_string()).size(11).width(Fill);
        let name_text = if locked && !is_selected {
            name_text.style(crate::ui::style::common::muted_style)
        } else {
            name_text
        };
        let mut label = row![mark, name_text].align_y(iced::Center);
        if let Some(info) = info {
            let count = text(info.count.to_string()).size(10);
            label = label.push(if is_selected {
                count
            } else {
                count.style(move |theme: &Theme| text::Style {
                    color: Some(
                        theme
                            .palette()
                            .background
                            .base
                            .text
                            .scale_alpha(if info.count == 0 { 0.4 } else { 0.65 }),
                    ),
                })
            });
        }
        let cell = container(label)
            .padding([4, 8])
            .width(Fill)
            .style(move |theme: &Theme| {
                let pair = theme.palette().primary.strong;
                container::Style {
                    background: is_selected.then_some(Background::Color(pair.color)),
                    text_color: is_selected.then_some(pair.text),
                    ..Default::default()
                }
            });
        let area = mouse_area(cell).on_press(on_select);
        if can_rename {
            area.on_double_click(Message::StyleRenameStart(kind, name.to_string()))
                .into()
        } else {
            area.into()
        }
    }
}
