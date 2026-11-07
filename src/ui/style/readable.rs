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
use super::common::{readable_accent_at, wcag_contrast, wcag_luminance};

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

/// The least contrast a hint (an input's placeholder, an empty-list note)
/// keeps: WCAG AA for body text. Lower than [`MUTED_MIN_CONTRAST`] so a
/// placeholder still looks different from a typed value.
pub const HINT_MIN_CONTRAST: f32 = 4.5;
/// How far a hint is mixed towards the background at most: about the 0.48
/// alpha placeholders used to be drawn at.
pub const HINT_MAX_FADE: f32 = 0.52;

/// The surfaces interface text is read on in `theme`: the window
/// background and its weak shade, and the menu bar, panel title and ribbon
/// panel OCS derives from them.
pub fn reading_surfaces(theme: &Theme) -> [Color; 5] {
    let background = theme.palette().background;
    [
        background.base.color,
        background.weak.color,
        chrome::bar(theme),
        chrome::title_bar(theme),
        chrome::panel(theme),
    ]
}

/// The surfaces text sits on only in a state: hovered and pressed
/// controls, an opened panel and the strong background shade.
pub fn state_surfaces(theme: &Theme) -> [Color; 3] {
    [
        theme.palette().background.strong.color,
        chrome::open_panel(theme),
        chrome::hover(theme),
    ]
}

/// Every surface interface text is drawn on in `theme`:
/// [`reading_surfaces`] and [`state_surfaces`].
pub fn text_surfaces(theme: &Theme) -> [Color; 8] {
    let [a, b, c, d, e] = reading_surfaces(theme);
    let [f, g, h] = state_surfaces(theme);
    [a, b, c, d, e, f, g, h]
}

/// The lowest contrast `color` has on any of `surfaces`.
pub fn worst_contrast_on(color: Color, surfaces: &[Color]) -> f32 {
    surfaces
        .iter()
        .map(|surface| wcag_contrast(color, *surface))
        .fold(f32::MAX, f32::min)
}

/// The lowest contrast `color` has on any of `theme`'s [`text_surfaces`].
pub fn worst_contrast(color: Color, theme: &Theme) -> f32 {
    worst_contrast_on(color, &text_surfaces(theme))
}

/// The theme's text mixed towards its background, opaque, as far as
/// `max_fade` but never below `min_contrast` on any reading surface.
fn faded_text(theme: &Theme, max_fade: f32, min_contrast: f32) -> Color {
    let background = theme.palette().background.base;
    let surfaces = reading_surfaces(theme);
    const STEPS: u8 = 32;
    (0..=STEPS)
        .rev()
        .map(|step| max_fade * f32::from(step) / f32::from(STEPS))
        .map(|fade| chrome::mix(background.text, background.color, fade))
        .find(|color| worst_contrast_on(*color, &surfaces) >= min_contrast)
        .unwrap_or(background.text)
}

/// Secondary text (labels, section headings, values shown but not
/// edited): the theme's text mixed towards its background, opaque, as far
/// as [`MUTED_MAX_FADE`] but never below [`MUTED_MIN_CONTRAST`] on any
/// reading surface. Use it instead of fading text with alpha.
pub fn muted_text(theme: &Theme) -> Color {
    faded_text(theme, MUTED_MAX_FADE, MUTED_MIN_CONTRAST)
}

/// A hint (an input's placeholder, an empty-list note): fainter than
/// [`muted_text`] but never below [`HINT_MIN_CONTRAST`].
pub fn hint_text(theme: &Theme) -> Color {
    faded_text(theme, HINT_MAX_FADE, HINT_MIN_CONTRAST)
}

/// `color` as secondary text: itself when it already reads at
/// [`MUTED_MIN_CONTRAST`] on every reading surface, otherwise
/// [`muted_text`].
pub fn at_least_muted(color: Color, theme: &Theme) -> Color {
    if worst_contrast_on(color, &reading_surfaces(theme)) >= MUTED_MIN_CONTRAST {
        color
    } else {
        muted_text(theme)
    }
}

/// A text style in [`muted_text`].
pub fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(muted_text(theme)),
    }
}

/// A text style in [`hint_text`].
pub fn hint_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(hint_text(theme)),
    }
}

/// An accent drawn in an icon on the ribbon: readable at
/// [`ACCENT_MIN_CONTRAST`] on the panel surface, in the theme's hue (see
/// [`readable_accent_at`]). The theme's own accent is left as it is for
/// buttons, selections and fills.
pub fn icon_accent(accent: Color, theme: &Theme) -> Color {
    let text = theme.palette().background.base.text;
    readable_accent_at(accent, chrome::panel(theme), text, ACCENT_MIN_CONTRAST)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(color: Color) -> String {
        let [r, g, b, _] = color.into_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// Text on the window background and the menu bar.
    const PAGE_TEXT_MIN: f32 = 10.0;
    /// Text on any reading surface (also the weak shade, panel title and
    /// ribbon panel).
    const READING_TEXT_MIN: f32 = 7.0;
    /// Text on a hovered, pressed or opened surface.
    const STATE_TEXT_MIN: f32 = 4.5;

    fn every_theme() -> Vec<Theme> {
        crate::app::config::all_themes()
    }

    #[test]
    fn text_reads_on_every_surface_of_every_theme() {
        for base in every_theme() {
            let ui = ui_theme(&base);
            let text = ui.palette().background.base.text;
            let [page, _, bar, ..] = reading_surfaces(&ui);
            for surface in [page, bar] {
                let ratio = wcag_contrast(text, surface);
                assert!(
                    ratio >= PAGE_TEXT_MIN,
                    "{base}: text {ratio:.1}:1 on the page"
                );
            }
            let reading = worst_contrast_on(text, &reading_surfaces(&ui));
            assert!(reading >= READING_TEXT_MIN, "{base}: text {reading:.1}:1");
            let state = worst_contrast_on(text, &state_surfaces(&ui));
            assert!(
                state >= STATE_TEXT_MIN,
                "{base}: text {state:.1}:1 in a state"
            );
        }
    }

    #[test]
    fn muted_and_hint_text_read_and_stay_opaque() {
        for base in every_theme() {
            let ui = ui_theme(&base);
            let surfaces = reading_surfaces(&ui);
            let muted = muted_text(&ui);
            let hint = hint_text(&ui);
            assert_eq!((muted.a, hint.a), (1.0, 1.0), "{base}");
            let ratio = worst_contrast_on(muted, &surfaces);
            assert!(ratio >= MUTED_MIN_CONTRAST, "{base}: muted {ratio:.1}:1");
            let ratio = worst_contrast_on(hint, &surfaces);
            assert!(ratio >= HINT_MIN_CONTRAST, "{base}: hint {ratio:.1}:1");
            // The ribbon's own muted levels are no weaker.
            for color in [
                chrome::muted(&ui),
                chrome::title_text(&ui),
                chrome::tab_text(&ui),
            ] {
                let ratio = worst_contrast_on(color, &surfaces);
                assert!(ratio >= MUTED_MIN_CONTRAST, "{base}: chrome {ratio:.1}:1");
            }
        }
    }

    #[test]
    fn icon_accents_read_on_the_ribbon() {
        for base in every_theme() {
            let ui = ui_theme(&base);
            let p = ui.palette();
            for accent in [
                p.primary.base.color,
                p.success.base.color,
                p.warning.base.color,
                p.danger.base.color,
            ] {
                let ratio = wcag_contrast(icon_accent(accent, &ui), chrome::panel(&ui));
                assert!(ratio >= ACCENT_MIN_CONTRAST, "{base}: accent {ratio:.1}:1");
            }
        }
    }

    #[test]
    fn ui_theme_keeps_background_accents_and_name() {
        for base in every_theme() {
            let ui = ui_theme(&base);
            let (before, after) = (base.seed(), ui.seed());
            assert_eq!(after.background, before.background, "{base}");
            assert_eq!(after.primary, before.primary, "{base}");
            assert_eq!(after.success, before.success, "{base}");
            assert_eq!(after.warning, before.warning, "{base}");
            assert_eq!(after.danger, before.danger, "{base}");
            assert_eq!(ui.to_string(), base.to_string());
            assert_eq!(after.text, text_for(before.background), "{base}");
        }
    }

    #[test]
    fn the_chosen_theme_is_found_again_and_keeps_its_canvas_and_selection() {
        for base in every_theme() {
            let ui = ui_theme(&base);
            assert_eq!(base_theme(&ui), base, "{base}");
            // Saving keeps the name; loading it gives the same theme.
            assert_eq!(
                crate::app::config::builtin_theme(&ui.to_string()),
                Some(base.clone())
            );
            assert_eq!(
                crate::app::config::theme_canvas_background(&ui),
                crate::app::config::theme_canvas_background(&base),
                "{base}"
            );
            assert_eq!(
                crate::ui::overlay::theme_selection_colors(&ui),
                crate::ui::overlay::theme_selection_colors(&base),
                "{base}"
            );
        }
    }

    #[test]
    fn the_users_own_theme_keeps_its_text() {
        let seed = Seed {
            text: Color::from_rgb(0.5, 0.2, 0.2),
            ..Theme::Dark.seed()
        };
        let custom = Theme::custom(USER_THEME, seed);
        assert_eq!(ui_theme(&custom), custom);
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
