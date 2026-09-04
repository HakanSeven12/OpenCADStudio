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

use iced::{Color, Theme};

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

/// Convert sRGB channel [0.0, 1.0] to linear light for WCAG luminance calculations.
pub(crate) fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Calculate standard WCAG 2.1 relative luminance for an opaque color.
pub(crate) fn wcag_luminance(color: Color) -> f32 {
    0.2126 * to_linear(color.r) + 0.7152 * to_linear(color.g) + 0.0722 * to_linear(color.b)
}

/// Calculate standard WCAG contrast ratio between two colors (range: 1.0 to 21.0).
pub(crate) fn wcag_contrast(c1: Color, c2: Color) -> f32 {
    let l1 = wcag_luminance(c1);
    let l2 = wcag_luminance(c2);
    (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05)
}

/// Returns `accent` if it achieves at least `min_contrast` against `bg`,
/// otherwise safely falls back to `fallback`.
pub(crate) fn accessible_accent_threshold(
    accent: Color,
    bg: Color,
    fallback: Color,
    min_contrast: f32,
) -> Color {
    if wcag_contrast(accent, bg) >= min_contrast {
        accent
    } else {
        fallback
    }
}

/// Returns `accent` if it achieves at least 3.0:1 contrast against `bg` (WCAG 1.4.11 for UI components),
/// otherwise safely falls back to `fallback`.
pub(crate) fn accessible_accent(accent: Color, bg: Color, fallback: Color) -> Color {
    accessible_accent_threshold(accent, bg, fallback, 3.0)
}

/// Returns true if the given canvas background color has a WCAG relative luminance > 0.5 (light surface).
pub(crate) fn canvas_is_light(bg: [f32; 4]) -> bool {
    wcag_luminance(Color::from_rgb(bg[0], bg[1], bg[2])) > 0.5
}
