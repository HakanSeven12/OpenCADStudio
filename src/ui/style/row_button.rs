//! Row/header button styles shared by the layers and xref list panels.
//!
//! Centralizes the zebra/selected row-button coloring previously duplicated
//! in `src/ui/window/layers.rs` (`layer_cell_button_style` +
//! `layer_header_button_style`) and `src/ui/window/xref_manager.rs`
//! (`row_button_style`). The two cell copies were functionally byte-identical
//! (Hovered → `background.strong`, selected → `primary.weak`, even →
//! `background.base`, else → `background.weak`); a single source keeps hover,
//! selection, and zebra colors consistent across both panels.

use iced::widget::button;
use iced::{Background, Theme};

/// Cell (row) button style — closure form so both `.style(...)` call sites
/// (`layers.rs`, `xref_manager.rs`) keep their existing shape.
pub(crate) fn cell_button_style(
    selected: bool,
    index: usize,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let palette = theme.palette();
        let highlighted = matches!(status, button::Status::Hovered);
        let pair = if highlighted {
            palette.background.strong
        } else if selected {
            palette.primary.weak
        } else if index % 2 == 0 {
            palette.background.base
        } else {
            palette.background.weak
        };
        button::Style {
            background: highlighted.then_some(Background::Color(pair.color)),
            text_color: pair.text,
            ..Default::default()
        }
    }
}

/// Column-header button style (layers panel).
pub(crate) fn header_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let highlighted = matches!(
        status,
        button::Status::Hovered | button::Status::Pressed
    );
    let pair = if highlighted {
        palette.background.strong
    } else {
        palette.background.weak
    };
    button::Style {
        background: highlighted.then_some(Background::Color(pair.color)),
        text_color: pair.text,
        ..Default::default()
    }
}
