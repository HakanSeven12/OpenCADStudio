//! Selection-filter status menu — choose which entity types are selectable.
//! A checked row means that type can be picked; unchecking it excludes the
//! type from interactive selection. Opened from the FILTER status pill.

use rustc_hash::FxHashSet as HashSet;

use iced::widget::{button, container, row, text};
use iced::{Background, Element, Fill, Theme};

use crate::app::Message;
use crate::ui::statusbar::status_menu::Entry;

/// - `types`: entity-type names present in the current layout.
/// - `excluded`: types currently filtered out (unchecked).
pub fn menu_entries(
    types: Vec<String>,
    excluded: &HashSet<String>,
) -> Vec<Entry<'static>> {
    // "Select All / Clear All" header, mirroring the OSNAP popup: Select All
    // clears every exclusion, Clear All excludes every present type.
    let has_types = !types.is_empty();
    let all_included = excluded.is_empty();
    let all_excluded = has_types && types.iter().all(|t| excluded.contains(t));
    let header = row![
        header_btn(
            "Select All",
            Message::SelectionFilterSelectAll,
            has_types && !all_included,
        ),
        header_btn(
            "Clear All",
            Message::SelectionFilterClearAll,
            has_types && !all_excluded,
        ),
    ]
    .spacing(1)
    .padding([4u16, 8]);

    let divider = container(iced::widget::Space::new().height(1))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weak.color,
            )),
            ..Default::default()
        })
        .width(Fill)
        .padding([0, 4]);

    let rows: Vec<Entry<'static>> = if types.is_empty() {
        vec![Entry::stay(empty_row())]
    } else {
        types
            .into_iter()
            .map(|name| {
                let included = !excluded.contains(&name);
                Entry::stay(type_row(name, included))
            })
            .collect()
    };

    let mut entries = vec![Entry::stay(header), Entry::stay(divider)];
    entries.extend(rows);
    entries
}

fn type_row(name: String, included: bool) -> Element<'static, Message> {
    let check = crate::ui::icons::themed_check_cell(included);

    let lbl = text(name.clone()).size(11);

    let content = row![check, lbl].spacing(6).align_y(iced::Center);

    button(content)
        .on_press(Message::ToggleSelectionFilterType(name))
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}

fn empty_row() -> Element<'static, Message> {
    container(
        text("No objects").size(11).style(|theme: &Theme| text::Style {
            color: Some(
                theme
                    .extended_palette()
                    .background
                    .base
                    .text
                    .scale_alpha(0.42),
            ),
        }),
    )
        .padding([4, 10])
        .into()
}

fn header_btn(label: &str, msg: Message, enabled: bool) -> Element<'_, Message> {
    let b = button(text(label).size(10));
    let b = if enabled { b.on_press(msg) } else { b };
    b.style(button::secondary)
    .padding([3, 8])
    .into()
}
