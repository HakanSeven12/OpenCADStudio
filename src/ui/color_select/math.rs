//! Colour maths and formatting for the Select Color panel.
//!
//! Everything here is pure so it can be unit tested without a UI: HSB/RGB
//! conversion, hex formatting and parsing, nearest-index lookup and the
//! short names the panel shows.

use document::types::Color as AcadColor;

/// Hue in degrees (0–360), saturation and brightness in 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsb {
    pub h: f32,
    pub s: f32,
    pub b: f32,
}

impl Hsb {
    pub fn to_rgb(self) -> (u8, u8, u8) {
        hsb_to_rgb(self.h, self.s, self.b)
    }
}

/// Convert hue (degrees), saturation and brightness (0–1) to 8-bit RGB.
pub fn hsb_to_rgb(h: f32, s: f32, b: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let b = b.clamp(0.0, 1.0);
    let channel = |n: f32| {
        let k = (n + h / 60.0) % 6.0;
        let v = b - b * s * k.min(4.0 - k).clamp(0.0, 1.0);
        (v * 255.0).round() as u8
    };
    (channel(5.0), channel(3.0), channel(1.0))
}

/// Convert 8-bit RGB to HSB. Grays have no hue; they report 0.
pub fn rgb_to_hsb(r: u8, g: u8, b: u8) -> Hsb {
    let (r, g, b) = (
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let mut h = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    let s = if max == 0.0 { 0.0 } else { delta / max };
    Hsb { h, s, b: max }
}

/// Cyan, magenta, yellow and black in percent, by the plain device-free
/// formula (no ICC profile): screen values, not press values.
pub fn rgb_to_cmyk(r: u8, g: u8, b: u8) -> [f32; 4] {
    let max = r.max(g).max(b);
    if max == 0 {
        return [0.0, 0.0, 0.0, 100.0];
    }
    let max = f32::from(max);
    let ink = |v: u8| (1.0 - f32::from(v) / max) * 100.0;
    [ink(r), ink(g), ink(b), (1.0 - max / 255.0) * 100.0]
}

pub fn cmyk_to_rgb([c, m, y, k]: [f32; 4]) -> (u8, u8, u8) {
    let white = 255.0 * (1.0 - k.clamp(0.0, 100.0) / 100.0);
    let channel = |ink: f32| (white * (1.0 - ink.clamp(0.0, 100.0) / 100.0)).round() as u8;
    (channel(c), channel(m), channel(y))
}

/// RGB of an ACI entry, or `None` outside 1–255.
pub fn aci_rgb(index: u8) -> Option<(u8, u8, u8)> {
    document::types::aci_table::aci_to_rgb(index)
}

/// The RGB a concrete colour displays as. Logical colours have none.
pub fn color_rgb(color: AcadColor) -> Option<(u8, u8, u8)> {
    match color {
        AcadColor::Index(i) => aci_rgb(i),
        AcadColor::Rgb { r, g, b } => Some((r, g, b)),
        _ => None,
    }
}

/// The closest index colour to an RGB value.
pub fn nearest_aci(r: u8, g: u8, b: u8) -> u8 {
    document::types::aci_table::nearest_aci(r, g, b)
}

/// `#RRGGBB`, upper case — the same format the Options hex fields store.
pub fn hex_string(r: u8, g: u8, b: u8) -> String {
    crate::app::config::rgb_to_hex([r, g, b])
}

/// Parse `#RRGGBB`, `RRGGBB`, `#RGB` or `RGB`. Unlike `config::parse_hex`,
/// which Options uses, this also accepts the short three-digit form.
pub fn parse_hex(text: &str) -> Option<(u8, u8, u8)> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let expanded: String = match digits.len() {
        3 => digits.chars().flat_map(|c| [c, c]).collect(),
        6 => digits.to_string(),
        _ => return None,
    };
    let value = u32::from_str_radix(&expanded, 16).ok()?;
    Some(((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

/// The short name shown on the Current / New swatch.
pub fn short_name(color: AcadColor) -> String {
    match color {
        AcadColor::Index(i) => format!("ACI {i}"),
        AcadColor::Rgb { r, g, b } => hex_string(r, g, b),
        AcadColor::ByLayer => "ByLayer".to_string(),
        AcadColor::ByBlock => "ByBlock".to_string(),
        AcadColor::None => "None".to_string(),
    }
}

/// One-line description for the panel's info line.
pub fn describe(color: AcadColor) -> String {
    match color {
        AcadColor::Index(i) => {
            let (r, g, b) = aci_rgb(i).unwrap_or((128, 128, 128));
            format!("ACI {i} · RGB {r}, {g}, {b}")
        }
        AcadColor::Rgb { r, g, b } => format!("RGB {r}, {g}, {b} · {}", hex_string(r, g, b)),
        other => short_name(other),
    }
}

/// Black or white text, whichever reads better on the given background.
pub fn ink_is_dark(r: u8, g: u8, b: u8) -> bool {
    0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) > 150.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsb_round_trips_primary_and_mixed_colours() {
        for (r, g, b) in [
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (57, 191, 252),
            (120, 200, 80),
            (128, 128, 128),
            (0, 0, 0),
            (255, 255, 255),
        ] {
            let hsb = rgb_to_hsb(r, g, b);
            assert_eq!(hsb.to_rgb(), (r, g, b), "round trip of {r},{g},{b}");
        }
    }

    #[test]
    fn hue_of_pure_colours() {
        assert_eq!(rgb_to_hsb(255, 0, 0).h, 0.0);
        assert!((rgb_to_hsb(0, 255, 0).h - 120.0).abs() < 0.01);
        assert!((rgb_to_hsb(0, 0, 255).h - 240.0).abs() < 0.01);
        assert_eq!(rgb_to_hsb(80, 80, 80).s, 0.0);
    }

    #[test]
    fn hue_wraps_around() {
        assert_eq!(hsb_to_rgb(360.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsb_to_rgb(-120.0, 1.0, 1.0), hsb_to_rgb(240.0, 1.0, 1.0));
    }

    #[test]
    fn nearest_aci_comes_from_the_codec_table() {
        assert_eq!(nearest_aci(255, 0, 0), 1);
        assert_eq!(nearest_aci(250, 5, 5), 1);
    }

    #[test]
    fn short_names() {
        assert_eq!(short_name(AcadColor::Index(140)), "ACI 140");
        assert_eq!(
            short_name(AcadColor::Rgb {
                r: 57,
                g: 191,
                b: 252
            }),
            "#39BFFC"
        );
        assert_eq!(short_name(AcadColor::ByLayer), "ByLayer");
    }

    #[test]
    fn hex_formats_and_parses() {
        assert_eq!(hex_string(57, 191, 252), "#39BFFC");
        assert_eq!(parse_hex("#39bffc"), Some((57, 191, 252)));
        assert_eq!(parse_hex("39BFFC"), Some((57, 191, 252)));
        assert_eq!(parse_hex("#3bf"), Some((0x33, 0xBB, 0xFF)));
        assert_eq!(parse_hex("#39BFF"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
    }
}
