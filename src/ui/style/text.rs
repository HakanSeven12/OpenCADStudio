//! UI text sizes in logical pixels. Whole numbers only: fractional sizes and
//! positions render soft. Line heights are whole pixels too.
//!
//! Hinting is off in the pinned iced (`iced_graphics::text::hint_factor`
//! returns `None`), so whole sizes and whole-pixel positions are the only
//! lever for sharp small text.

use iced::widget::text::LineHeight;
use iced::Pixels;

/// Rows, values, lists, menus, combos.
pub const UI: f32 = 11.0;
/// Captions, hints.
pub const UI_SMALL: f32 = 10.0;
/// Ribbon tabs, dialog headings.
pub const UI_LARGE: f32 = 12.0;
/// Palette titles (PROPERTIES, LAYERS, ...), shown in capitals.
pub const UI_TITLE: f32 = 13.0;
/// Line height for [`UI`] and [`UI_SMALL`].
pub const UI_LINE: f32 = 14.0;
/// Line height for [`UI_LARGE`] and [`UI_TITLE`].
pub const UI_LARGE_LINE: f32 = 16.0;

/// Whole-pixel line height for [`UI`] / [`UI_SMALL`] text.
pub fn line() -> LineHeight {
    LineHeight::Absolute(Pixels(UI_LINE))
}

/// Whole-pixel line height for [`UI_LARGE`] / [`UI_TITLE`] text.
pub fn large_line() -> LineHeight {
    LineHeight::Absolute(Pixels(UI_LARGE_LINE))
}

/// Snap a logical position to whole physical pixels at the current window
/// and UI scale (see [`crate::ui::icons::pixel_scale`]).
pub fn snap_px(v: f32) -> f32 {
    snap(v, crate::ui::icons::pixel_scale())
}

/// [`snap_px`] for both coordinates of a text position.
pub fn snap_point(p: iced::Point) -> iced::Point {
    iced::Point::new(snap_px(p.x), snap_px(p.y))
}

/// Round a logical position to whole physical pixels at `scale`.
/// Snap positions only, never sizes.
pub fn snap(v: f32, scale: f32) -> f32 {
    if scale > 0.0 {
        (v * scale).round() / scale
    } else {
        v.round()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_line_heights_are_whole_pixels() {
        for v in [UI, UI_SMALL, UI_LARGE, UI_TITLE, UI_LINE, UI_LARGE_LINE] {
            assert_eq!(v.fract(), 0.0, "{v}");
        }
        const {
            assert!(UI_LINE >= UI * 1.2);
            assert!(UI_LARGE_LINE >= UI_TITLE * 1.2);
        }
    }

    #[test]
    fn snap_lands_on_physical_pixels() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            for v in [0.1_f32, 3.4, 10.92, 17.5] {
                let s = snap(v, scale) * scale;
                assert!((s - s.round()).abs() < 1e-4, "{v} @ {scale}");
            }
        }
    }
}
