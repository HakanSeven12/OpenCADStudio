//! Annotation / viewport scale status menu.

use iced::widget::{button, row, text};
use iced::{Element, Fill};

use crate::app::Message;
use crate::ui::statusbar::status_menu::Entry;

/// - `is_model`: true = model space (dispatches SetAnnotationScale), false = paper space (SetViewportScale).
/// - `current_anno_scale`: current annotation_scale from Scene (used to highlight active row in model space).
/// - `viewport_scale`: current effective vp scale, view_height-first (used to highlight in paper space).
/// - `file_scales`: scale list read from the drawing (`ACAD_SCALELIST`). Only
///   scales actually stored in the file are shown; the picker never injects
///   scales of its own.
pub fn menu_entries(
    is_model: bool,
    current_anno_scale: f32,
    viewport_scale: Option<f64>,
    file_scales: Vec<(String, f32, f64)>,
) -> Vec<Entry<'static>> {
    let mut entries: Vec<Entry<'static>> = file_scales
        .into_iter()
        .map(|(label, anno_scale, vp_scale)| {
            let active = if is_model {
                (current_anno_scale - anno_scale).abs() < 0.001 * current_anno_scale.max(0.001)
            } else {
                viewport_scale
                    .map(|vs| (vs - vp_scale).abs() < 0.001 * vp_scale.max(0.001))
                    .unwrap_or(false)
            };
            let msg = if is_model {
                Message::SetAnnotationScale(anno_scale)
            } else {
                Message::SetViewportScale(vp_scale)
            };
            Entry::close(scale_row(label, active, msg))
        })
        .collect();

    if is_model {
        entries.push(Entry::close(manage_row()));
    }
    entries
}

fn scale_row(label: String, active: bool, msg: Message) -> Element<'static, Message> {
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

fn manage_row() -> Element<'static, Message> {
    button(text("Manage...").size(11))
        .on_press(Message::ScaleManagerOpen)
        .style(button::primary)
        .width(Fill)
        .padding([5, 10])
        .into()
}
