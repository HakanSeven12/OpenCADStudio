//! Annotation / viewport scale status menu.

use crate::t;
use iced::Element;

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
    current_scale_name: &str,
    viewport_scale: Option<f64>,
    file_scales: &[(String, f32, f64)],
) -> Vec<Entry<'static>> {
    let mut entries: Vec<Entry<'static>> = file_scales
        .iter()
        .map(|(label, _anno_scale, vp_scale)| {
            let vp_scale = *vp_scale;
            let active = if is_model {
                label.eq_ignore_ascii_case(current_scale_name)
            } else {
                label.eq_ignore_ascii_case(current_scale_name)
                    || (current_scale_name.is_empty()
                        && viewport_scale
                            .map(|vs| (vs - vp_scale).abs() < 0.001 * vp_scale.max(0.001))
                            .unwrap_or(false))
            };
            let msg = if is_model {
                Message::SetAnnotationScale(label.clone())
            } else {
                Message::SetViewportScale(label.clone())
            };
            Entry::close(scale_row(label.clone(), active, msg))
        })
        .collect();

    if is_model {
        entries.push(Entry::close(manage_row()));
    }
    entries
}

/// A scale, the current one checked: a palette menu item.
fn scale_row(label: String, active: bool, msg: Message) -> Element<'static, Message> {
    crate::ui::palette_menu::entry(
        crate::ui::palette_menu::Item::new(label, msg)
            .mark(crate::ui::palette_menu::Mark::Check(active)),
    )
}

/// Manage..., after a separator.
fn manage_row() -> Element<'static, Message> {
    use crate::ui::palette_menu::{entry, Item};
    iced::widget::column![
        entry(Item::Separator),
        entry(Item::new(t!("Manage..."), Message::ScaleManagerOpen)),
    ]
    .into()
}
