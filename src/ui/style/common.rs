//! Style helpers shared across the style windows.
//!
//! Centralizes `muted_style` (and its alias `muted_text_style`) which were
//! previously duplicated in 7 style files with byte-identical bodies (alpha
//! 0.68). A single source ensures a theme change updates one place.
//!
//! **`btn_s` is intentionally NOT deduped here.** The two `btn_s` locals
//! (`plotstyle.rs:41` and `style_manager.rs:299`) have divergent bodies:
//! `style_manager` handles `Status::Disabled` (text alpha 0.45) while
//! `plotstyle` does not. Unifying them would silently change the plotstyle
//! disabled appearance. See `cargo test --lib ui::style::common_tests`.

use iced::Theme;

/// Muted secondary text style — `background.base.text` at 68% opacity.
pub(crate) fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

/// Alias for call sites that used `muted_text_style` (e.g. `style_manager`).
/// Same implementation as [`muted_style`]; provided so existing imports
/// can be unified without renaming at the call site.
#[allow(dead_code)]
pub(crate) fn muted_text_style(theme: &Theme) -> iced::widget::text::Style {
    muted_style(theme)
}
