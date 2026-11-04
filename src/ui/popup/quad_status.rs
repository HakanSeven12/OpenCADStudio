//! The status bar's QUAD field: a small anchored popup listing the quad
//! cursor menu's display options (docs/quad-menu.md §3.7). Mirrors the
//! `snap_popup.rs` pattern — one `status_menu::Entry` row per option, each a
//! checkbox reporting a `Message::QuadOption`.

use iced::widget::{checkbox, row, text};
use iced::Element;

use crate::app::Message;
use crate::t;
use crate::ui::statusbar::status_menu::Entry;

/// The quad display flags the status bar pill mirrors. The app refreshes one
/// assigned copy of these per message (see `update()`), so the pill never
/// needs a path into the app's settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuadStatusFlags {
    /// QUAD: the quad cursor menu exists at all.
    pub on: bool,
    /// QUADDISPLAY: hovering an entity offers the quad.
    pub on_hover: bool,
    /// QUADDISPLAY: a selection set offers the quad.
    pub on_select: bool,
    /// QUADDISPLAY: a targetless right-click opens the No Selection quad.
    pub on_right_click: bool,
    /// QUADDISPLAY: hovering is suppressed while something is selected.
    pub suppress_hover_when_selection: bool,
}

/// The popup rows: the master switch, then the four QUADDISPLAY display
/// options. Every row keeps the menu open (`Entry::stay`), like the snap
/// type list it mirrors.
pub fn menu_entries<'a>(flags: QuadStatusFlags) -> Vec<Entry<'a>> {
    let master = quad_row("Quad cursor menu", flags.on, |on| {
        Message::QuadOption(crate::QuadOptionMsg::Quad(on))
    });
    let on_hover = quad_row("Offer the quad while hovering", flags.on_hover, |on| {
        Message::QuadOption(crate::QuadOptionMsg::OnHover(on))
    });
    let on_select = quad_row("Offer the quad for a selection", flags.on_select, |on| {
        Message::QuadOption(crate::QuadOptionMsg::OnSelect(on))
    });
    let on_right_click = quad_row("Right-click in the void opens it", flags.on_right_click, |on| {
        Message::QuadOption(crate::QuadOptionMsg::OnRightClick(on))
    });
    let suppress = quad_row(
        "Suppress the hover quad while selected",
        flags.suppress_hover_when_selection,
        |on| Message::QuadOption(crate::QuadOptionMsg::SuppressHoverWhenSelection(on)),
    );
    vec![
        Entry::stay(master),
        Entry::stay(divider()),
        Entry::stay(on_hover),
        Entry::stay(on_select),
        Entry::stay(on_right_click),
        Entry::stay(suppress),
    ]
}

/// One checkbox option row (the snap popup's drafting block layout).
fn quad_row<'a>(
    label: &'a str,
    on: bool,
    on_toggle: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    row![
        checkbox(on).on_toggle(on_toggle).size(14),
        text(t!(label)).size(11),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

/// A 1px hairline between the master switch and the display options.
fn divider() -> Element<'static, Message> {
    iced::widget::container(iced::widget::Space::new().height(1))
        .style(|theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(
                theme.palette().background.weak.color,
            )),
            ..Default::default()
        })
        .width(iced::Fill)
        .padding([0, 4])
        .into()
}

/// Column layout helper kept symmetric with `snap_popup.rs`'s drafting
/// block: the entries stack inside one menu width.
pub(crate) fn menu_width() -> f32 {
    260.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_status_popup_lists_master_and_four_display_options() {
        let flags = QuadStatusFlags {
            on: false,
            on_hover: true,
            on_select: true,
            on_right_click: true,
            suppress_hover_when_selection: true,
        };
        // Master + divider + the four display options.
        assert_eq!(menu_entries(flags).len(), 6);
        assert_eq!(menu_width(), 260.0);
    }

    #[test]
    fn quad_status_flags_default_to_a_disabled_quad() {
        // Default = everything off, matching `UserSettings::default()`'s
        // opt-in quad.
        let flags = QuadStatusFlags::default();
        assert!(!flags.on);
        assert!(!flags.on_hover);
    }
}
