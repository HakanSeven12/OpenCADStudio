//! Studio Dark: the theme of the OCS interface design. A blue-grey chrome,
//! light grey text and a blue accent, around a canvas a shade darker than
//! the chrome.
//!
//! The chrome's surfaces and text shades are tuned on this theme (see
//! [`super::chrome`]), so it shows the design as drawn.
//!
//! An `iced::Theme::Custom`, so it is absent from `iced::Theme::ALL`;
//! anything enumerating themes must go through
//! [`crate::app::config::all_themes`].

use iced::theme::palette::Seed;
use iced::{Color, Theme};

/// Display name. Persisted verbatim in `settings.json` as `theme.name`, so
/// changing it orphans a user's choice.
pub const STUDIO_DARK: &str = "Studio Dark";

/// Model-space canvas for [`STUDIO_DARK`]: a shade darker than the chrome.
pub const STUDIO_DARK_CANVAS: [u8; 3] = [0x21, 0x28, 0x30];

fn rgb(hex: u32) -> Color {
    Color::from_rgb8(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

/// Blue-grey chrome, light grey text, blue accent.
#[must_use]
pub fn studio_dark() -> Theme {
    Theme::custom(
        STUDIO_DARK,
        Seed {
            background: rgb(0x2A_313C),
            text: rgb(0xDF_E3E9),
            primary: rgb(0x3E_A6FF),
            success: rgb(0x3E_CF6E),
            warning: rgb(0xF5_C84B),
            danger: rgb(0xF2_645F),
        },
    )
}

/// The Studio themes, in the order the theme picker should show them.
#[must_use]
pub fn studio_themes() -> Vec<Theme> {
    vec![studio_dark()]
}

/// The canvas colour a Studio theme asks for, or `None` for any other theme.
/// Keyed by name because `Theme::Custom` carries no discriminant to match on.
#[must_use]
pub fn studio_canvas(theme: &Theme) -> Option<[u8; 3]> {
    match theme.to_string().as_str() {
        STUDIO_DARK => Some(STUDIO_DARK_CANVAS),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::style::chrome;

    fn hex(color: Color) -> String {
        let [r, g, b, _] = color.into_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// The chrome shows the design's colours, within a few levels: the
    /// surfaces step in OKLCH lightness where the design mixed towards the
    /// text, and keep the background's hue.
    #[test]
    fn studio_dark_shows_the_design() {
        let theme = studio_dark();
        assert_eq!(hex(theme.palette().background.base.text), "#dfe3e9");
        for (name, color, design) in [
            ("page", chrome::page(&theme), 0x1b2027),
            ("bar", chrome::bar(&theme), 0x2a313c),
            ("title_bar", chrome::title_bar(&theme), 0x333b47),
            ("panel", chrome::panel(&theme), 0x3b4453),
            ("open_panel", chrome::open_panel(&theme), 0x434d5d),
            ("hover", chrome::hover(&theme), 0x4a5566),
            ("border", chrome::border(&theme), 0x56606f),
            ("hover_border", chrome::hover_border(&theme), 0x5b6779),
        ] {
            let [r, g, b, _] = color.into_rgba8();
            let want = [(design >> 16) as u8, (design >> 8) as u8, design as u8];
            for (now, was) in [r, g, b].into_iter().zip(want) {
                assert!(
                    now.abs_diff(was) <= 2,
                    "{name} is {} where the design has #{design:06x}",
                    hex(color)
                );
            }
        }
    }

    #[test]
    fn studio_canvas_is_keyed_by_name() {
        assert_eq!(studio_canvas(&studio_dark()), Some(STUDIO_DARK_CANVAS));
        assert_eq!(studio_canvas(&Theme::Dark), None);
    }
}
