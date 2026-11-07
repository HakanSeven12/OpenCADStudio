//! Readable interface text and icon colours in every theme.
//!
//! iced's themes (other than Light and Dark) are code-editor colour schemes
//! whose foreground was chosen to be soft for long reading, not for small
//! interface labels. Desktop and CAD applications keep interface text near
//! white on dark and near black on light; a theme only changes background
//! and accent. [`ui_theme`] does the same for OCS: it keeps a theme's
//! background and its primary, success, warning and danger colours, and
//! replaces only its text colour by [`text_for`].
//!
//! Secondary text is not faded with alpha but mixed towards the background
//! only as far as it stays clearly readable ([`muted_text`]), and accents
//! drawn as text or icons are pushed towards the text colour until they
//! read ([`readable_accent_on`]).

use iced::theme::palette::Seed;
use iced::{Color, Theme};

use super::chrome;
use super::common::{wcag_contrast, wcag_luminance};

/// Interface text on a dark background: near white, since pure white glares
/// on a very dark background.
pub const TEXT_ON_DARK: Color = Color::from_rgb(
    0xF0 as f32 / 255.0,
    0xF0 as f32 / 255.0,
    0xF0 as f32 / 255.0,
);
/// Interface text on a light background: near black.
pub const TEXT_ON_LIGHT: Color = Color::from_rgb(
    0x1A as f32 / 255.0,
    0x1A as f32 / 255.0,
    0x1A as f32 / 255.0,
);
/// A background with a WCAG relative luminance above this is light.
pub const LIGHT_BACKGROUND_LUMINANCE: f32 = 0.5;
/// The least contrast secondary text keeps on every surface it is drawn on.
pub const MUTED_MIN_CONTRAST: f32 = 7.0;
/// How far secondary text is mixed towards the background at most: the
/// 0.68 alpha it used to be drawn at.
pub const MUTED_MAX_FADE: f32 = 0.32;
/// The least contrast an accent drawn as text or as an icon keeps (WCAG AA
/// for body text).
pub const ACCENT_MIN_CONTRAST: f32 = 4.5;
/// The name of the theme the user edits colour by colour: its text colour
/// is the user's own choice and is left as it is.
const USER_THEME: &str = "Custom";

/// The interface text colour for a theme whose background is `background`.
pub fn text_for(background: Color) -> Color {
    if wcag_luminance(background) > LIGHT_BACKGROUND_LUMINANCE {
        TEXT_ON_LIGHT
    } else {
        TEXT_ON_DARK
    }
}

/// `base` with its text colour replaced by [`text_for`] its background, so
/// every iced widget and every OCS style that reads the palette gets
/// readable text. Background and accents are kept; the name is kept, so the
/// theme is still saved, listed and recognised as `base` (see
/// [`base_theme`]). The user's own Custom theme is returned unchanged.
pub fn ui_theme(base: &Theme) -> Theme {
    let name = base.to_string();
    if name == USER_THEME {
        return base.clone();
    }
    let seed = base.seed();
    let text = text_for(seed.background);
    if seed.text == text {
        return base.clone();
    }
    Theme::custom(name, Seed { text, ..seed })
}

/// The theme the user chose, for a theme that [`ui_theme`] may have made:
/// the built-in or Fusion theme of the same name, or `theme` itself. Match
/// on this, not on the theme iced hands a style function.
pub fn base_theme(theme: &Theme) -> Theme {
    match theme {
        Theme::Custom(_) => {
            crate::app::config::builtin_theme(&theme.to_string()).unwrap_or_else(|| theme.clone())
        }
        builtin => builtin.clone(),
    }
}

/// Every surface interface text is drawn on in `theme`: iced's background
/// shades and the ribbon, panel and hover surfaces OCS derives from them.
pub fn text_surfaces(theme: &Theme) -> [Color; 8] {
    let background = theme.palette().background;
    [
        background.base.color,
        background.weak.color,
        background.strong.color,
        chrome::bar(theme),
        chrome::title_bar(theme),
        chrome::panel(theme),
        chrome::open_panel(theme),
        chrome::hover(theme),
    ]
}

/// The lowest contrast `color` has on any of `theme`'s [`text_surfaces`].
pub fn worst_contrast(color: Color, theme: &Theme) -> f32 {
    text_surfaces(theme)
        .into_iter()
        .map(|surface| wcag_contrast(color, surface))
        .fold(f32::MAX, f32::min)
}

/// Secondary text: the theme's text mixed towards its background, opaque,
/// as far as [`MUTED_MAX_FADE`] but never below [`MUTED_MIN_CONTRAST`] on
/// any surface. Use it instead of fading text with alpha.
pub fn muted_text(theme: &Theme) -> Color {
    let background = theme.palette().background.base;
    const STEPS: u8 = 32;
    (0..=STEPS)
        .rev()
        .map(|step| MUTED_MAX_FADE * f32::from(step) / f32::from(STEPS))
        .map(|fade| chrome::mix(background.text, background.color, fade))
        .find(|color| worst_contrast(*color, theme) >= MUTED_MIN_CONTRAST)
        .unwrap_or(background.text)
}

/// `accent` moved towards `text` only as far as it takes to reach `ratio`
/// against `bg`, so it keeps its hue in a light and a dark theme alike.
pub fn readable_accent_on(accent: Color, bg: Color, text: Color, ratio: f32) -> Color {
    (0..=20)
        .map(|step| chrome::mix(accent, text, step as f32 / 20.0))
        .find(|color| wcag_contrast(*color, bg) >= ratio)
        .unwrap_or(text)
}

/// An accent drawn in an icon on the ribbon: readable at
/// [`ACCENT_MIN_CONTRAST`] on the panel surface, in the theme's hue.
pub fn icon_accent(accent: Color, theme: &Theme) -> Color {
    let text = theme.palette().background.base.text;
    readable_accent_on(accent, chrome::panel(theme), text, ACCENT_MIN_CONTRAST)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(color: Color) -> String {
        let [r, g, b, _] = color.into_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// Contrast of the new text per surface. Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "prints contrast per surface"]
    fn print_contrast_per_surface() {
        println!("| Theme | base | weak | strong | bar | title bar | panel | open panel | hover |");
        println!("|---|---|---|---|---|---|---|---|---|");
        for base in crate::app::config::all_themes() {
            let ui = ui_theme(&base);
            let text = ui.palette().background.base.text;
            let cells: Vec<String> = text_surfaces(&ui)
                .into_iter()
                .map(|s| format!("{:.1}", wcag_contrast(text, s)))
                .collect();
            println!("| {base} | {} |", cells.join(" | "));
        }
    }

    /// The contrast table of the report: text, muted text and icon accents,
    /// worst surface, before and after. Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "prints the contrast table"]
    fn print_contrast_table() {
        println!(
            "| Theme | text before | text after | muted before | muted after | primary | success | warning | danger |"
        );
        println!("|---|---|---|---|---|---|---|---|---|");
        for base in crate::app::config::all_themes() {
            let ui = ui_theme(&base);
            let before_text = base.palette().background.base.text;
            let after_text = ui.palette().background.base.text;
            // Muted text used to be the text at 0.68 alpha over the surface:
            // its worst case is the text mixed 32 % towards each surface.
            let before_muted = text_surfaces(&base)
                .into_iter()
                .map(|s| wcag_contrast(chrome::mix(before_text, s, MUTED_MAX_FADE), s))
                .fold(f32::MAX, f32::min);
            let accent = |c: Color| {
                let panel = chrome::panel(&ui);
                format!(
                    "{:.1} → {:.1}",
                    wcag_contrast(c, chrome::panel(&base)),
                    wcag_contrast(icon_accent(c, &ui), panel)
                )
            };
            let p = ui.palette();
            println!(
                "| {base} | {} {:.1} | {} {:.1} | {before_muted:.1} | {:.1} | {} | {} | {} | {} |",
                hex(before_text),
                worst_contrast(before_text, &base),
                hex(after_text),
                worst_contrast(after_text, &ui),
                worst_contrast(muted_text(&ui), &ui),
                accent(p.primary.base.color),
                accent(p.success.base.color),
                accent(p.warning.base.color),
                accent(p.danger.base.color),
            );
        }
    }
}
