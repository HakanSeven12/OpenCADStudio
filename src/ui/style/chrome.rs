//! Colour tokens of the window chrome (ribbon, menu bar and their popups).
//! Every token mixes the theme's background and text colours, so the chrome
//! follows any iced or Fusion theme: `mix(bg, fg, t)` per channel, clamped.
//! A negative `t` goes past the background, darker on a dark theme and
//! lighter on a light one.

use iced::{Color, Theme};

/// `bg` moved towards `fg` by `t` per channel, clamped to the colour range.
pub fn mix(bg: Color, fg: Color, t: f32) -> Color {
    let channel = |b: f32, f: f32| (b + (f - b) * t).clamp(0.0, 1.0);
    Color {
        r: channel(bg.r, fg.r),
        g: channel(bg.g, fg.g),
        b: channel(bg.b, fg.b),
        a: 1.0,
    }
}

fn token(theme: &Theme, t: f32) -> Color {
    let background = theme.palette().background.base;
    mix(background.color, background.text, t)
}

/// Lines between bars; the darkest recess.
pub fn page(theme: &Theme) -> Color {
    token(theme, -0.10)
}

/// Menu-bar and tab-row background.
pub fn bar(theme: &Theme) -> Color {
    token(theme, 0.0)
}

/// Panel title strip.
pub fn title_bar(theme: &Theme) -> Color {
    token(theme, 0.056)
}

/// Ribbon tool area, active tab, popups.
pub fn panel(theme: &Theme) -> Color {
    token(theme, 0.111)
}

/// Panel body while its slide-out is open and unpinned.
pub fn open_panel(theme: &Theme) -> Color {
    token(theme, 0.162)
}

/// Hover and pressed background.
pub fn hover(theme: &Theme) -> Color {
    token(theme, 0.207)
}

/// Popup and control borders.
pub fn border(theme: &Theme) -> Color {
    token(theme, 0.267)
}

/// Hover outline of buttons.
pub fn hover_border(theme: &Theme) -> Color {
    token(theme, 0.308)
}

/// Section labels, shortcut text.
pub fn muted(theme: &Theme) -> Color {
    token(theme, 0.652)
}

/// Panel title text.
pub fn title_text(theme: &Theme) -> Color {
    token(theme, 0.714)
}

/// Inactive tab and menu-bar text.
pub fn tab_text(theme: &Theme) -> Color {
    token(theme, 0.859)
}

/// Button labels.
pub fn text(theme: &Theme) -> Color {
    token(theme, 1.0)
}

/// Active and hovered text.
pub fn strong(theme: &Theme) -> Color {
    token(theme, 1.154)
}

pub fn accent(theme: &Theme) -> Color {
    theme.palette().primary.base.color
}

pub fn danger(theme: &Theme) -> Color {
    theme.palette().danger.base.color
}

/// Background of a lit button: the command it runs is running, or the
/// switch it shows is on. The panel tinted with the accent.
pub fn lit(theme: &Theme) -> Color {
    mix(panel(theme), accent(theme), 0.25)
}

/// Outline of a lit button.
pub fn lit_border(theme: &Theme) -> Color {
    mix(panel(theme), accent(theme), 0.7)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(color: Color) -> String {
        let [r, g, b, _] = color.into_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    #[test]
    fn tokens_mix_each_channel_and_clamp() {
        // The dark reference design: bg #2a313c, fg #dfe3e9.
        let bg = Color::from_rgb8(0x2a, 0x31, 0x3c);
        let fg = Color::from_rgb8(0xdf, 0xe3, 0xe9);
        let at = |t| hex(mix(bg, fg, t));
        assert_eq!(at(0.0), "#2a313c");
        assert_eq!(at(1.0), "#dfe3e9");
        assert_eq!(at(-0.10), "#181f2b");
        // Past the text colour the blue channel clamps at white.
        assert_eq!(at(1.154), "#fbfeff");
    }

    #[test]
    fn a_negative_amount_goes_past_the_background() {
        // On a light theme `page` is lighter than the background.
        let bg = Color::from_rgb8(0xe0, 0xe0, 0xe0);
        let fg = Color::from_rgb8(0x20, 0x20, 0x20);
        assert!(mix(bg, fg, -0.1).r > bg.r);
    }
}
