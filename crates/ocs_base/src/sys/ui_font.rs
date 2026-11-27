//! Discovery of the desktop's configured interface (UI) font.
//!
//! Platform settings are read and normalised into a [`SystemUiFont`];
//! [`apply`] then makes its family what generic sans-serif means in iced's
//! font database, so every widget and canvas text drawn with `Font::DEFAULT`
//! (including its bold and italic variants) follows it without call-site
//! changes.
//!
//! Sizes are reported in logical pixels (device-independent pixels at
//! 96 DPI). Display scaling is left to the window system — winit already
//! folds it into the window scale factor — so it must never be applied here
//! as well.
//!
//! | Platform | Source                                                        |
//! |----------|---------------------------------------------------------------|
//! | Linux    | GNOME-family gsettings, KDE `kdeglobals`, Xfce xfconf, the XDG |
//! |          | Settings portal, GTK `settings.ini`, then a Fontconfig match  |
//! | Windows  | `SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS)` at 96 DPI |
//! | macOS    | `NSFont.systemFontOfSize:` / `+systemFontSize`                |
//!
//! Detection is best-effort: every failure path returns `None` (or a
//! lower-confidence [`Source`]) and the caller keeps iced's default font.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Where a detected font came from. Used for logging and to tell a real
/// desktop setting apart from a generic fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `org.gnome.desktop.interface` (also Unity, Budgie, Pantheon) via gsettings.
    GnomeSettings,
    /// `org.cinnamon.desktop.interface` via gsettings.
    CinnamonSettings,
    /// `org.mate.interface` via gsettings.
    MateSettings,
    /// Xfce's xsettings channel via xfconf-query.
    XfceSettings,
    /// KDE Plasma's `kdeglobals` `[General] font=` entry.
    KdeGlobals,
    /// Plasma's built-in default when `kdeglobals` names no font.
    KdeDefault,
    /// `org.freedesktop.portal.Settings` (works inside Flatpak/Snap sandboxes).
    Portal,
    /// `gtk-font-name` from a GTK 3/4 `settings.ini`.
    GtkSettingsIni,
    /// Fontconfig's `sans-serif` match. Not proof of a desktop choice.
    Fontconfig,
    /// Windows' message-box font from the non-client metrics.
    WindowsNonClientMetrics,
    /// AppKit's system font.
    MacSystemFont,
}

impl Source {
    /// Whether the font reflects an explicit desktop UI-font setting rather
    /// than a generic family resolution.
    pub fn is_desktop_setting(self) -> bool {
        !matches!(self, Source::Fontconfig)
    }
}

/// A font size as written in a platform setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSize {
    /// Typographic points (1/72 in).
    Points(f32),
    /// Logical pixels.
    Pixels(f32),
}

impl FontSize {
    /// Logical pixels at the 96 DPI reference resolution, which is what
    /// iced's text sizes are expressed in before the window scale factor.
    pub fn to_logical_px(self) -> f32 {
        match self {
            FontSize::Points(pt) => points_to_px(pt),
            FontSize::Pixels(px) => px,
        }
    }
}

/// Convert typographic points to logical pixels (96 px per inch, 72 pt per
/// inch). Desktops that express the UI font in points (GNOME, KDE, Windows'
/// "9 pt Segoe UI") all assume this reference resolution; the display's
/// actual scaling is applied separately by the window system.
pub fn points_to_px(pt: f32) -> f32 {
    pt * 96.0 / 72.0
}

/// Convert a Win32 `LOGFONTW::lfHeight` measured at `dpi` to logical pixels.
/// A negative height is the em (character) height — the convention every
/// system font uses; a positive one is the cell height including internal
/// leading, which is taken as an approximate em. Zero means "default size".
pub fn logfont_height_to_px(lf_height: i32, dpi: u32) -> Option<f32> {
    if lf_height == 0 || dpi == 0 {
        return None;
    }
    Some(lf_height.unsigned_abs() as f32 * 96.0 / dpi as f32)
}

/// The interface font a desktop is configured to use.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemUiFont {
    /// Family name to request first. `None` when the desktop names a generic
    /// alias that could not be resolved to a concrete family.
    pub family: Option<String>,
    /// Further family names to try, in order, if `family` is not installed
    /// (macOS reports a private family name that font databases expose
    /// under a different one).
    pub alternates: Vec<String>,
    /// CSS-style weight, 100–900 (400 regular, 700 bold).
    pub weight: u16,
    /// Whether the UI font itself is italic.
    pub italic: bool,
    /// Configured size in logical pixels, if the platform reports one.
    pub size_px: Option<f32>,
    /// Where the setting was read from.
    pub source: Source,
}

impl SystemUiFont {
    #[cfg(target_os = "linux")]
    fn from_parsed(parsed: ParsedFont, source: Source) -> Self {
        Self {
            family: parsed.family,
            alternates: Vec::new(),
            weight: parsed.weight,
            italic: parsed.italic,
            size_px: parsed.size.map(FontSize::to_logical_px),
            source,
        }
    }
}

/// Read the desktop's UI font. Returns `None` when nothing usable could be
/// read; never panics and never blocks for long (subprocess and D-Bus reads
/// are bounded by short timeouts).
pub fn detect() -> Option<SystemUiFont> {
    #[cfg(target_os = "linux")]
    return linux::detect();
    #[cfg(target_os = "windows")]
    return windows::detect();
    #[cfg(target_os = "macos")]
    return macos::detect();
    #[allow(unreachable_code)]
    None
}

/// The family [`apply`] last mapped generic sans-serif to, and the one iced
/// chose itself (restored when the desktop font is unusable).
#[cfg(not(target_arch = "wasm32"))]
struct Applied {
    iced_default: String,
    current: String,
}

#[cfg(not(target_arch = "wasm32"))]
static APPLIED: std::sync::Mutex<Option<Applied>> = std::sync::Mutex::new(None);

/// Bumped each time [`apply`] changes the UI font.
static GENERATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Counter that changes whenever the UI font does, for caches of measured
/// UI text.
pub fn generation() -> u32 {
    GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Make `system`'s family the generic sans-serif family in iced's font
/// database. iced's `Font::DEFAULT` is `Family::SansSerif`, so all UI text
/// picks it up. The family must be installed; otherwise (or with no desktop
/// font) iced's own sans-serif choice is kept or restored, so text never ends
/// up in per-glyph fallback faces. The font's weight and style are ignored:
/// regular UI text stays regular. Returns whether the mapping changed.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply(system: Option<&SystemUiFont>) -> bool {
    use iced::advanced::graphics::text::{cosmic_text::fontdb::Family, font_system};

    let mut font_system = font_system()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut applied = APPLIED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let at_startup = applied.is_none();
    let db = font_system.raw().db();
    let applied = applied.get_or_insert_with(|| {
        let name = db.family_name(&Family::SansSerif).to_string();
        Applied {
            iced_default: name.clone(),
            current: name,
        }
    });

    // Family lookups are exact, so take the database's own spelling.
    let installed = system
        .into_iter()
        .flat_map(|font| font.family.iter().chain(&font.alternates))
        .filter(|name| !is_generic_alias(name))
        .find_map(|wanted| {
            db.faces()
                .flat_map(|face| &face.families)
                .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
                .map(|(name, _)| name.clone())
        });
    if installed.is_none() {
        if let Some(family) = system.and_then(|font| font.family.as_deref()) {
            log::info!("System UI font \"{family}\" is not installed; using iced's default");
        }
    }
    let family = installed.unwrap_or_else(|| applied.iced_default.clone());
    if family == applied.current {
        return false;
    }

    // `db_mut` also clears cosmic-text's font-match cache.
    font_system
        .raw()
        .db_mut()
        .set_sans_serif_family(family.clone());
    if !at_startup {
        // Laid-out paragraphs only reshape when the font system's version
        // changes, and loading a font is the only way to bump it. The empty
        // source adds no face (fontdb logs a warning for it).
        font_system.load_font(std::borrow::Cow::Owned(Vec::new()));
    }
    applied.current = family;
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    true
}

/// Yields each time the desktop reports a changed UI-font setting. Linux
/// only; the stream is empty when no Settings portal is reachable, in which
/// case new settings apply after a restart.
#[cfg(target_os = "linux")]
pub fn change_signals() -> impl iced::futures::Stream<Item = ()> + Send + 'static {
    linux::change_signals()
}

/// A platform font description reduced to the parts the UI uses.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFont {
    pub family: Option<String>,
    pub weight: u16,
    pub italic: bool,
    pub size: Option<FontSize>,
}

/// Generic family aliases that name no concrete font. Fontconfig resolves
/// them (on Linux) to the configured default sans face.
pub fn is_generic_alias(family: &str) -> bool {
    matches!(
        family.trim().to_ascii_lowercase().as_str(),
        "sans" | "sans-serif" | "sans serif" | "system-ui" | "ui-sans-serif" | "default"
    )
}

/// Parse a Pango font description such as `"Cantarell 11"`,
/// `"Noto Sans Semi-Bold Italic 10"` or `"Inter, Sans 14px"` — the format of
/// GNOME's `font-name`, Xfce's `/Gtk/FontName` and GTK's `gtk-font-name`.
pub fn parse_pango(desc: &str) -> Option<ParsedFont> {
    let mut words: Vec<&str> = desc.split_whitespace().collect();
    let mut size = None;
    if let Some(last) = words.last() {
        if let Some(px) = last.strip_suffix("px") {
            if let Ok(v) = px.parse::<f32>() {
                size = Some(FontSize::Pixels(v));
                words.pop();
            }
        } else if let Ok(v) = last.parse::<f32>() {
            size = Some(FontSize::Points(v));
            words.pop();
        }
    }
    let mut weight = 400;
    let mut italic = false;
    // Style words trail the family; consume them from the end so a family
    // that merely contains one ("Bold Sans Display") keeps its leading words.
    while words.len() > 1 {
        let word = words.last().unwrap().to_ascii_lowercase();
        if let Some(w) = pango_weight(&word) {
            weight = w;
        } else if word == "italic" || word == "oblique" {
            italic = true;
        } else if !is_pango_modifier(&word) {
            break;
        }
        words.pop();
    }
    let family_list = words.join(" ");
    let family = family_list
        .split(',')
        .map(str::trim)
        .find(|f| !f.is_empty())
        .map(str::to_string);
    if family.is_none() && size.is_none() {
        return None;
    }
    Some(ParsedFont {
        family,
        weight,
        italic,
        size: size.filter(|s| s.to_logical_px() > 0.0),
    })
}

fn pango_weight(word: &str) -> Option<u16> {
    Some(match word {
        "thin" | "hairline" => 100,
        "ultra-light" | "ultralight" | "extra-light" | "extralight" => 200,
        "light" => 300,
        "semi-light" | "semilight" | "demi-light" | "demilight" => 350,
        "book" => 380,
        "regular" | "normal" => 400,
        "medium" => 500,
        "semi-bold" | "semibold" | "demi-bold" | "demibold" => 600,
        "bold" => 700,
        "ultra-bold" | "ultrabold" | "extra-bold" | "extrabold" => 800,
        "heavy" | "black" => 900,
        "ultra-heavy" | "ultraheavy" | "extra-black" => 1000,
        _ => return None,
    })
}

/// Pango stretch, variant and gravity words — consumed but not used.
fn is_pango_modifier(word: &str) -> bool {
    matches!(
        word,
        "ultra-condensed"
            | "extra-condensed"
            | "condensed"
            | "semi-condensed"
            | "semi-expanded"
            | "expanded"
            | "extra-expanded"
            | "ultra-expanded"
            | "small-caps"
            | "all-small-caps"
            | "petite-caps"
            | "all-petite-caps"
            | "unicase"
            | "title-caps"
            | "not-rotated"
            | "south"
            | "upside-down"
            | "north"
            | "rotated-left"
            | "east"
            | "rotated-right"
            | "west"
            | "roman"
    )
}

/// Parse a `QFont::toString()` value — KDE's `kdeglobals` `font=` entry and
/// the KDE portal's value. Qt 5 writes 10–11 fields with weights on a 0–99
/// scale; Qt 6 writes 16–17 fields with CSS-style 1–1000 weights:
///
/// `family,pointSizeF,pixelSize,styleHint,weight,style,underline,strikeOut,fixedPitch,…`
pub fn parse_qfont(desc: &str) -> Option<ParsedFont> {
    let fields: Vec<&str> = desc.trim().trim_matches('"').split(',').collect();
    if fields.len() < 6 {
        return None;
    }
    let family = Some(fields[0].trim().to_string()).filter(|f| !f.is_empty());
    let point_size = fields[1].trim().parse::<f32>().ok().filter(|v| *v > 0.0);
    let pixel_size = fields[2].trim().parse::<f32>().ok().filter(|v| *v > 0.0);
    let size = point_size
        .map(FontSize::Points)
        .or(pixel_size.map(FontSize::Pixels));
    let raw_weight = fields[4].trim().parse::<u16>().ok();
    let qt6 = fields.len() >= 16;
    let weight = match raw_weight {
        Some(w) if qt6 => w.clamp(100, 1000),
        Some(w) => qt5_weight_to_css(w),
        None => 400,
    };
    // Field 5 is QFont::Style: 0 normal, 1 italic, 2 oblique.
    let italic = matches!(fields[5].trim(), "1" | "2");
    family.as_ref()?;
    Some(ParsedFont {
        family,
        weight,
        italic,
        size,
    })
}

/// Qt 5's `QFont::Weight` (0–99) to the CSS scale, using Qt's own table.
fn qt5_weight_to_css(w: u16) -> u16 {
    const TABLE: [(u16, u16); 9] = [
        (0, 100),
        (12, 200),
        (25, 300),
        (50, 400),
        (57, 500),
        (63, 600),
        (75, 700),
        (81, 800),
        (87, 900),
    ];
    TABLE
        .iter()
        .min_by_key(|(qt, _)| qt.abs_diff(w))
        .map(|&(_, css)| css)
        .unwrap_or(400)
}

/// Value of `key` inside `[group]` of an INI/KConfig style file. KConfig
/// keys may carry a locale suffix (`font[de]`) or `[$e]`-style flags; only
/// the plain key is read.
pub fn ini_value(text: &str, group: &str, key: &str) -> Option<String> {
    let mut in_group = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line.trim_start_matches('[').trim_end_matches(']') == group;
            continue;
        }
        if !in_group || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let k = k.strip_suffix("[$e]").unwrap_or(k).trim();
        if k == key {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(v);
            return Some(v.to_string());
        }
    }
    None
}

/// Unquote a GVariant string printed by `gsettings get` (`'Cantarell 11'`).
pub fn parse_gvariant_string(out: &str) -> Option<String> {
    let s = out.trim();
    let s = s.strip_prefix("@s ").unwrap_or(s);
    let inner = s
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .or_else(|| s.strip_prefix('"').and_then(|s| s.strip_suffix('"')))?;
    Some(inner.replace("\\'", "'").replace("\\\\", "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pango_plain_family_and_points() {
        let f = parse_pango("Cantarell 11").unwrap();
        assert_eq!(f.family.as_deref(), Some("Cantarell"));
        assert_eq!(f.size, Some(FontSize::Points(11.0)));
        assert_eq!(f.weight, 400);
        assert!(!f.italic);
    }

    #[test]
    fn pango_style_words_and_multiword_family() {
        let f = parse_pango("Noto Sans Semi-Bold Italic 10.5").unwrap();
        assert_eq!(f.family.as_deref(), Some("Noto Sans"));
        assert_eq!(f.weight, 600);
        assert!(f.italic);
        assert_eq!(f.size, Some(FontSize::Points(10.5)));
    }

    #[test]
    fn pango_pixel_size_and_family_list() {
        let f = parse_pango("Inter, Cantarell, Sans 14px").unwrap();
        assert_eq!(f.family.as_deref(), Some("Inter"));
        assert_eq!(f.size, Some(FontSize::Pixels(14.0)));
    }

    #[test]
    fn pango_family_only_and_keyword_only_family() {
        let f = parse_pango("Ubuntu").unwrap();
        assert_eq!(f.family.as_deref(), Some("Ubuntu"));
        assert_eq!(f.size, None);
        // A lone style word is the family, not a modifier.
        let f = parse_pango("Bold 9").unwrap();
        assert_eq!(f.family.as_deref(), Some("Bold"));
        assert_eq!(f.weight, 400);
    }

    #[test]
    fn pango_rejects_empty() {
        assert!(parse_pango("").is_none());
        assert!(parse_pango("   ").is_none());
    }

    #[test]
    fn qfont_qt5_format() {
        let f = parse_qfont("Noto Sans,10,-1,5,50,0,0,0,0,0").unwrap();
        assert_eq!(f.family.as_deref(), Some("Noto Sans"));
        assert_eq!(f.size, Some(FontSize::Points(10.0)));
        assert_eq!(f.weight, 400);
        assert!(!f.italic);
        let f = parse_qfont("Hack,9,-1,5,75,1,0,0,0,0,Bold Italic").unwrap();
        assert_eq!(f.weight, 700);
        assert!(f.italic);
    }

    #[test]
    fn qfont_qt6_format_and_pixel_size() {
        let f = parse_qfont("Inter,10.5,-1,5,500,0,0,0,0,0,0,0,0,0,0,1").unwrap();
        assert_eq!(f.family.as_deref(), Some("Inter"));
        assert_eq!(f.weight, 500);
        assert_eq!(f.size, Some(FontSize::Points(10.5)));
        let f = parse_qfont("Inter,-1,15,5,400,0,0,0,0,0,0,0,0,0,0,1").unwrap();
        assert_eq!(f.size, Some(FontSize::Pixels(15.0)));
    }

    #[test]
    fn qfont_rejects_garbage() {
        assert!(parse_qfont("").is_none());
        assert!(parse_qfont("Noto Sans").is_none());
        assert!(parse_qfont(",10,-1,5,50,0,0,0,0,0").is_none());
    }

    #[test]
    fn unit_conversion() {
        assert_eq!(points_to_px(9.0), 12.0);
        assert_eq!(FontSize::Points(10.0).to_logical_px(), 40.0 / 3.0);
        assert_eq!(FontSize::Pixels(13.0).to_logical_px(), 13.0);
    }

    #[test]
    fn logfont_heights() {
        // Segoe UI 9 pt as reported at 96 DPI and at 144 DPI (150 %).
        assert_eq!(logfont_height_to_px(-12, 96), Some(12.0));
        assert_eq!(logfont_height_to_px(-18, 144), Some(12.0));
        assert_eq!(logfont_height_to_px(16, 96), Some(16.0));
        assert_eq!(logfont_height_to_px(0, 96), None);
        assert_eq!(logfont_height_to_px(-12, 0), None);
    }

    #[test]
    fn kdeglobals_general_font() {
        let text = "[Colors:View]\nfont=Wrong,1,-1,5,50,0,0,0,0,0\n\n\
                    [General]\nColorScheme=BreezeDark\nfont[$e]=Fira Sans,11,-1,5,50,0,0,0,0,0\n";
        assert_eq!(
            ini_value(text, "General", "font").as_deref(),
            Some("Fira Sans,11,-1,5,50,0,0,0,0,0")
        );
        assert_eq!(ini_value(text, "General", "missing"), None);
    }

    #[test]
    fn gtk_settings_ini() {
        let text = "[Settings]\ngtk-theme-name=Adwaita\ngtk-font-name = \"DejaVu Sans 10\"\n";
        assert_eq!(
            ini_value(text, "Settings", "gtk-font-name").as_deref(),
            Some("DejaVu Sans 10")
        );
    }

    #[test]
    fn gvariant_strings() {
        assert_eq!(
            parse_gvariant_string("'Cantarell 11'\n").as_deref(),
            Some("Cantarell 11")
        );
        assert_eq!(
            parse_gvariant_string("@s 'Ubuntu 11'").as_deref(),
            Some("Ubuntu 11")
        );
        assert_eq!(parse_gvariant_string("1.25"), None);
    }

    #[test]
    fn generic_aliases() {
        assert!(is_generic_alias("Sans"));
        assert!(is_generic_alias(" sans-serif "));
        assert!(!is_generic_alias("Noto Sans"));
    }

    #[test]
    fn only_fontconfig_is_not_a_desktop_setting() {
        assert!(!Source::Fontconfig.is_desktop_setting());
        assert!(Source::GnomeSettings.is_desktop_setting());
        assert!(Source::WindowsNonClientMetrics.is_desktop_setting());
    }
}
