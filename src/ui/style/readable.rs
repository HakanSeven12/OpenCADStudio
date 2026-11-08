//! Readable interface text and icon colours in every theme.
//!
//! Every theme keeps its own text colour. Secondary text is not faded with
//! alpha but mixed towards the background only as far as it stays clearly
//! readable ([`muted_text`]), and accents
//! drawn as text or icons are pushed towards the text colour until they
//! read ([`icon_accent`], [`accent_in_hue`]).

use iced::{Color, Theme};

use super::chrome;
use super::common::{wcag_contrast, wcag_luminance};

/// A background with a WCAG relative luminance above this is light.
pub const LIGHT_BACKGROUND_LUMINANCE: f32 = 0.5;
/// The least contrast secondary text (labels, panel titles, tab labels)
/// keeps on every surface it is drawn on: WCAG AA for body text. A theme
/// whose own text reads below that keeps its text unfaded.
pub const MUTED_MIN_CONTRAST: f32 = 4.5;
/// How far secondary text is mixed towards the background at most: the
/// 0.68 alpha it used to be drawn at.
pub const MUTED_MAX_FADE: f32 = 0.32;
/// The least contrast an accent drawn in an icon keeps: WCAG 1.4.11 for
/// graphical objects. (Accents drawn as text keep the 4.5:1 of body text,
/// see `common::readable_accent`.)
pub const ICON_ACCENT_MIN_CONTRAST: f32 = 3.0;
/// The least contrast a hint (an input's placeholder, an empty-list note)
/// keeps: WCAG AA for body text, as for [`MUTED_MIN_CONTRAST`]. A hint may
/// fade further ([`HINT_MAX_FADE`]), so a placeholder still looks different
/// from a typed value; on a theme with little contrast to spare the two can
/// meet at that floor.
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

/// The lowest contrast `color` has on any of `surfaces`.
pub fn worst_contrast_on(color: Color, surfaces: &[Color]) -> f32 {
    surfaces
        .iter()
        .map(|surface| wcag_contrast(color, *surface))
        .fold(f32::MAX, f32::min)
}

/// A colour's exact bits, for a cache key.
type ColorBits = [u32; 4];

fn color_bits(color: Color) -> ColorBits {
    [color.r, color.g, color.b, color.a].map(f32::to_bits)
}

/// What the reading surfaces of a theme are made from, exactly: its
/// background, its text and the weak background shade.
fn surface_key(theme: &Theme) -> [ColorBits; 3] {
    let background = theme.palette().background;
    [
        background.base.color,
        background.base.text,
        background.weak.color,
    ]
    .map(color_bits)
}

/// How many answers a readable-colour cache keeps before it starts over.
/// Each theme needs a handful; editing a custom theme's colours adds new
/// ones for every step.
const CACHE_LIMIT: usize = 256;

/// The answer for `key` from `cache`, worked out by `work_out` the first
/// time.
fn cached<K: std::hash::Hash + Eq>(
    cache: &'static std::thread::LocalKey<std::cell::RefCell<rustc_hash::FxHashMap<K, Color>>>,
    key: K,
    work_out: impl FnOnce() -> Color,
) -> Color {
    if let Some(color) = cache.with(|cache| cache.borrow().get(&key).copied()) {
        return color;
    }
    let color = work_out();
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, color);
    });
    color
}

/// The theme's text mixed towards its background, opaque, as far as
/// `max_fade` but never below `min_contrast` on any reading surface.
///
/// Text styles ask for it on every draw, for every label, so the answer is
/// kept per theme: working it out takes up to 53 mixes (one per 0.01 of
/// fade), each checked on five surfaces.
fn faded_text(theme: &Theme, max_fade: f32, min_contrast: f32) -> Color {
    type Key = ([ColorBits; 3], u32, u32);
    thread_local! {
        static CACHE: std::cell::RefCell<rustc_hash::FxHashMap<Key, Color>> =
            std::cell::RefCell::new(rustc_hash::FxHashMap::default());
    }
    let key = (
        surface_key(theme),
        max_fade.to_bits(),
        min_contrast.to_bits(),
    );
    cached(&CACHE, key, || {
        work_out_faded_text(theme, max_fade, min_contrast)
    })
}

/// [`faded_text`] without the cache.
fn work_out_faded_text(theme: &Theme, max_fade: f32, min_contrast: f32) -> Color {
    let background = theme.palette().background.base;
    let surfaces = reading_surfaces(theme);
    // One step size for every strength, so a hint and a label that both
    // stop at the same contrast floor stop on the same colour.
    const STEP: f32 = 0.01;
    let steps = (max_fade / STEP).round() as u16;
    (0..=steps)
        .rev()
        .map(|step| f32::from(step) * STEP)
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

/// `color` as secondary chrome text (labels, panel titles, tab labels):
/// itself when it already reads at [`MUTED_MIN_CONTRAST`] on every chrome
/// surface it is drawn on, otherwise moved towards the text colour only
/// as far as that takes, so the shades keep their order. Kept per theme
/// and colour, as [`faded_text`] is: the chrome's tab and title text ask
/// for it on every draw.
pub fn at_least_muted(color: Color, theme: &Theme) -> Color {
    type Key = ([ColorBits; 3], ColorBits);
    thread_local! {
        static CACHE: std::cell::RefCell<rustc_hash::FxHashMap<Key, Color>> =
            std::cell::RefCell::new(rustc_hash::FxHashMap::default());
    }
    cached(&CACHE, (surface_key(theme), color_bits(color)), || {
        work_out_at_least_muted(color, theme)
    })
}

/// A chrome surface (bar, panel, hover, border) `step` of OKLCH lightness
/// away from the theme's background: lighter on a dark theme, darker on a
/// light one, past the background for a negative `step`. Hue is the
/// background's; chroma scales with the lightness (see `CHROMA_POWER`),
/// reduced further only as far as sRGB needs. The text colour has no part
/// in it. Kept per background and step, as the chrome asks on every draw.
pub fn surface(theme: &Theme, step: f32) -> Color {
    type Key = (ColorBits, u32);
    thread_local! {
        static CACHE: std::cell::RefCell<rustc_hash::FxHashMap<Key, Color>> =
            std::cell::RefCell::new(rustc_hash::FxHashMap::default());
    }
    let background = theme.palette().background.base.color;
    cached(&CACHE, (color_bits(background), step.to_bits()), || {
        work_out_surface(background, step)
    })
}

/// [`surface`] without the cache.
fn work_out_surface(background: Color, step: f32) -> Color {
    let [from, a, b] = to_oklab(background);
    let towards = if wcag_luminance(background) > LIGHT_BACKGROUND_LUMINANCE {
        -1.0
    } else {
        1.0
    };
    let lightness = (from + towards * step).clamp(0.0, 1.0);
    // Chroma grows and shrinks with the lightness, as the design's shades
    // do (a lighter panel of a blue-grey theme is a bluer grey); the
    // power is fitted to the Studio Dark design, within 2 levels.
    const CHROMA_POWER: f32 = 0.6;
    let scale = if from > 0.01 {
        (lightness / from).powf(CHROMA_POWER)
    } else {
        1.0
    };
    let (a, b) = (a * scale, b * scale);
    const CHROMA_STEPS: u16 = 20;
    (0..=CHROMA_STEPS)
        .rev()
        .map(|k| f32::from(k) / f32::from(CHROMA_STEPS))
        .find_map(|keep| from_oklab([lightness, a * keep, b * keep]))
        .unwrap_or(if towards > 0.0 {
            Color::WHITE
        } else {
            Color::BLACK
        })
}

/// [`at_least_muted`] without the cache.
fn work_out_at_least_muted(color: Color, theme: &Theme) -> Color {
    let surfaces = chrome_text_surfaces(theme);
    let text = theme.palette().background.base.text;
    // Fine steps: a level that has to move stops just past the floor, so
    // two levels that both move keep their order.
    const STEPS: u16 = 200;
    (0..=STEPS)
        .map(|step| chrome::mix(color, text, f32::from(step) / f32::from(STEPS)))
        .find(|color| worst_contrast_on(*color, &surfaces) >= MUTED_MIN_CONTRAST)
        .unwrap_or(text)
}

/// The chrome surfaces secondary chrome text sits on: the bars, the panel
/// title strip, the ribbon and popups, and an opened panel.
fn chrome_text_surfaces(theme: &Theme) -> [Color; 4] {
    [
        chrome::bar(theme),
        chrome::title_bar(theme),
        chrome::panel(theme),
        chrome::open_panel(theme),
    ]
}

/// A text style in [`muted_text`].
pub fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(muted_text(theme)),
    }
}

/// An accent drawn in an icon on the ribbon: the theme's own colour when it
/// already reaches [`ICON_ACCENT_MIN_CONTRAST`] on the panel surface,
/// otherwise lightened (or darkened, on a light theme) in its own hue until
/// it does (see [`accent_in_hue`]). The theme's accent is left as it is for
/// buttons, selections and fills.
pub fn icon_accent(accent: Color, theme: &Theme) -> Color {
    let text = theme.palette().background.base.text;
    accent_in_hue(accent, chrome::panel(theme), text, ICON_ACCENT_MIN_CONTRAST)
}

/// `accent` with its OKLCH lightness moved towards that of `text` until it
/// reaches `ratio` on `bg`. Hue is kept, and chroma as far as sRGB allows,
/// so a red stays a red: mixing towards white instead would wash it out to
/// pink. `accent` itself when it already reaches `ratio`; `text` when no
/// lightness does.
pub fn accent_in_hue(accent: Color, bg: Color, text: Color, ratio: f32) -> Color {
    if wcag_contrast(accent, bg) >= ratio {
        return accent;
    }
    const LIGHTNESS_STEPS: u16 = 100;
    const CHROMA_STEPS: u16 = 40;
    let [lightness, a, b] = to_oklab(accent);
    let chroma = (a * a + b * b).sqrt();
    let hue = b.atan2(a);
    let target = to_oklab(text)[0];
    (1..=LIGHTNESS_STEPS)
        .map(|step| lightness + (target - lightness) * f32::from(step) / f32::from(LIGHTNESS_STEPS))
        .filter_map(|l| {
            // The most chroma sRGB holds at this lightness, up to the accent's.
            (0..=CHROMA_STEPS).rev().find_map(|k| {
                let c = chroma * f32::from(k) / f32::from(CHROMA_STEPS);
                from_oklab([l, c * hue.cos(), c * hue.sin()])
            })
        })
        .find(|color| wcag_contrast(*color, bg) >= ratio)
        .unwrap_or(text)
}

/// sRGB `c` (0 to 1) from linear light.
fn from_linear(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// `color` in OKLab (Björn Ottosson's matrices): lightness, a, b.
fn to_oklab(color: Color) -> [f32; 3] {
    use super::common::to_linear;
    let (r, g, b) = (to_linear(color.r), to_linear(color.g), to_linear(color.b));
    let l = (0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// The sRGB colour of OKLab `[l, a, b]`, or `None` outside sRGB.
fn from_oklab([lightness, a, b]: [f32; 3]) -> Option<Color> {
    let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let rgb = [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_93 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ];
    const EPSILON: f32 = 0.001;
    rgb.iter()
        .all(|v| (-EPSILON..=1.0 + EPSILON).contains(v))
        .then(|| {
            let [r, g, b] = rgb.map(|v| from_linear(v.clamp(0.0, 1.0)));
            Color::from_rgb(r, g, b)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_theme() -> Vec<Theme> {
        crate::app::config::all_themes()
    }

    #[test]
    fn muted_and_hint_text_read_and_stay_opaque() {
        for base in every_theme() {
            let ui = base.clone();
            let surfaces = reading_surfaces(&ui);
            let muted = muted_text(&ui);
            let hint = hint_text(&ui);
            assert_eq!((muted.a, hint.a), (1.0, 1.0), "{base}");
            // A theme whose own text reads below the floor keeps that text.
            let text = ui.palette().background.base.text;
            let floor = |min: f32, on: &[Color]| min.min(worst_contrast_on(text, on)) - 0.01;
            let ratio = worst_contrast_on(muted, &surfaces);
            assert!(
                ratio >= floor(MUTED_MIN_CONTRAST, &surfaces),
                "{base}: muted {ratio:.1}:1"
            );
            let hint_ratio = worst_contrast_on(hint, &surfaces);
            assert!(
                hint_ratio >= floor(HINT_MIN_CONTRAST, &surfaces),
                "{base}: hint {hint_ratio:.1}:1"
            );
            // A hint is never stronger than a label.
            assert!(hint_ratio <= ratio + 0.01, "{base}: hint above muted");
            // The ribbon's own levels read on the chrome they sit on, and
            // keep their order: labels, panel titles, tab labels.
            let chrome_surfaces = chrome_text_surfaces(&ui);
            let levels = [
                chrome::muted(&ui),
                chrome::title_text(&ui),
                chrome::tab_text(&ui),
            ]
            .map(|color| worst_contrast_on(color, &chrome_surfaces));
            for ratio in levels {
                assert!(
                    ratio >= floor(MUTED_MIN_CONTRAST, &chrome_surfaces),
                    "{base}: chrome {ratio:.1}:1"
                );
            }
            assert!(
                levels[0] <= levels[1] + 0.05 && levels[1] <= levels[2] + 0.05,
                "{base}: chrome levels {levels:?}"
            );
        }
    }

    /// The kept answer is the worked-out one, for every theme and both
    /// strengths, also when themes take turns.
    #[test]
    fn faded_text_is_kept_per_theme() {
        let themes: Vec<Theme> = every_theme();
        for _ in 0..2 {
            for theme in &themes {
                for (fade, contrast) in [
                    (MUTED_MAX_FADE, MUTED_MIN_CONTRAST),
                    (HINT_MAX_FADE, HINT_MIN_CONTRAST),
                ] {
                    assert_eq!(
                        faded_text(theme, fade, contrast),
                        work_out_faded_text(theme, fade, contrast)
                    );
                }
                for color in [
                    chrome::title_text(theme),
                    chrome::tab_text(theme),
                    theme.palette().background.base.text,
                ] {
                    assert_eq!(
                        at_least_muted(color, theme),
                        work_out_at_least_muted(color, theme)
                    );
                }
            }
        }
    }

    /// The chrome's surfaces in the order they step away from the bars.
    fn surfaces_in_order(theme: &Theme) -> [Color; 7] {
        [
            chrome::bar(theme),
            chrome::title_bar(theme),
            chrome::panel(theme),
            chrome::open_panel(theme),
            chrome::hover(theme),
            chrome::border(theme),
            chrome::hover_border(theme),
        ]
    }

    #[test]
    fn chrome_surfaces_do_not_follow_the_text_colour() {
        for base in every_theme() {
            let seed = base.seed();
            for text in [
                Color::WHITE,
                Color::BLACK,
                Color::from_rgb8(0x80, 0x40, 0x40),
            ] {
                let other =
                    Theme::custom("Other text", iced::theme::palette::Seed { text, ..seed });
                assert_eq!(
                    surfaces_in_order(&base),
                    surfaces_in_order(&other),
                    "{base}"
                );
                assert_eq!(chrome::page(&base), chrome::page(&other), "{base}");
            }
        }
    }

    /// In every theme each surface is a visible step from the one before:
    /// panels stand off the bars, hover off the panel, borders off both.
    #[test]
    fn chrome_surfaces_step_apart_in_every_theme() {
        for base in every_theme() {
            let theme = base.clone();
            let lightness = surfaces_in_order(&theme).map(|color| to_oklab(color)[0]);
            for pair in lightness.windows(2) {
                assert!((pair[1] - pair[0]).abs() >= 0.02, "{base}: {lightness:?}");
            }
        }
    }

    #[test]
    fn icon_accents_read_on_the_ribbon() {
        for base in every_theme() {
            let ui = base.clone();
            let p = ui.palette();
            for accent in [
                p.primary.base.color,
                p.success.base.color,
                p.warning.base.color,
                p.danger.base.color,
            ] {
                let ratio = wcag_contrast(icon_accent(accent, &ui), chrome::panel(&ui));
                assert!(
                    ratio >= ICON_ACCENT_MIN_CONTRAST,
                    "{base}: accent {ratio:.1}:1"
                );
            }
        }
    }

    #[test]
    fn icon_accents_keep_the_themes_colour_and_hue() {
        /// The hue of `color` in OKLCH, in degrees.
        fn hue(color: Color) -> f32 {
            let [_, a, b] = to_oklab(color);
            b.atan2(a).to_degrees()
        }
        for base in every_theme() {
            let ui = base.clone();
            let panel = chrome::panel(&ui);
            let p = ui.palette();
            for accent in [
                p.primary.base.color,
                p.success.base.color,
                p.warning.base.color,
                p.danger.base.color,
            ] {
                let icon = icon_accent(accent, &ui);
                if wcag_contrast(accent, panel) >= ICON_ACCENT_MIN_CONTRAST {
                    assert_eq!(icon, accent, "{base}: a readable accent is left alone");
                } else {
                    let turn = (hue(icon) - hue(accent) + 540.0) % 360.0 - 180.0;
                    assert!(turn.abs() < 6.0, "{base}: hue turned {turn:.1}°");
                }
            }
        }
        // Oxocarbon's red stays a red where it has to lighten.
        let ui = Theme::Oxocarbon;
        let red = icon_accent(ui.palette().danger.base.color, &ui);
        let [r, g, b, _] = red.into_rgba8();
        assert!(r > 230 && g < 80 && b < 60, "#{r:02x}{g:02x}{b:02x}");
    }

    #[test]
    fn oklab_round_trips() {
        for color in [
            Color::from_rgb8(0xc3, 0x42, 0x3f),
            Color::from_rgb8(0x4a, 0x9e, 0xff),
            Color::from_rgb8(0xf0, 0xc0, 0x40),
            Color::WHITE,
            Color::BLACK,
        ] {
            let back = from_oklab(to_oklab(color)).unwrap();
            assert_eq!(back.into_rgba8(), color.into_rgba8());
        }
    }
}
