//! Windows UI-font discovery.
//!
//! The UI font is the non-client metrics' message font (`lfMessageFont`,
//! Segoe UI 9 pt by default) — the face Windows itself uses for dialogs and
//! message boxes. It is read with `SystemParametersInfoForDpi` at 96 DPI so
//! the height comes back in logical pixels: the window's DPI scale is
//! applied by winit, and reading at the monitor DPI would apply it twice.

use super::{logfont_height_to_px, Source, SystemUiFont};
use windows_sys::Win32::Graphics::Gdi::LOGFONTW;
use windows_sys::Win32::UI::HiDpi::SystemParametersInfoForDpi;
use windows_sys::Win32::UI::WindowsAndMessaging::{NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS};

/// The DPI at which one logical pixel is one physical pixel.
const REFERENCE_DPI: u32 = 96;

pub(super) fn detect() -> Option<SystemUiFont> {
    // SAFETY: NONCLIENTMETRICSW is plain old data; all-zero is a valid value.
    let mut metrics: NONCLIENTMETRICSW = unsafe { std::mem::zeroed() };
    metrics.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
    // SAFETY: `metrics` is a properly sized, writable NONCLIENTMETRICSW and
    // `cbSize`/`uiParam` both carry its size, as the API requires.
    let ok = unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            (&mut metrics as *mut NONCLIENTMETRICSW).cast(),
            0,
            REFERENCE_DPI,
        )
    };
    if ok == 0 {
        return None;
    }
    from_logfont(&metrics.lfMessageFont)
}

fn from_logfont(font: &LOGFONTW) -> Option<SystemUiFont> {
    let len = font
        .lfFaceName
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(font.lfFaceName.len());
    let family = String::from_utf16(&font.lfFaceName[..len]).ok()?;
    let family = family.trim();
    if family.is_empty() {
        return None;
    }
    // FW_DONTCARE (0) means the face's regular weight.
    let weight = match font.lfWeight {
        w if w <= 0 => 400,
        w => w.clamp(100, 900) as u16,
    };
    Some(SystemUiFont {
        family: Some(family.to_string()),
        alternates: without_style_words(family),
        weight,
        italic: font.lfItalic != 0,
        size_px: logfont_height_to_px(font.lfHeight, REFERENCE_DPI),
        source: Source::WindowsNonClientMetrics,
    })
}

/// GDI names a font by its legacy family, which carries the style of a face
/// outside the regular/bold/italic four ("Artifakt Element Semi Bold"), while
/// font databases list the typographic family ("Artifakt Element"). The
/// shorter names, dropping trailing style words one at a time, are the
/// alternates (#2160).
fn without_style_words(family: &str) -> Vec<String> {
    const STYLE_WORDS: &[&str] = &[
        "thin",
        "hairline",
        "extralight",
        "ultralight",
        "light",
        "semilight",
        "demilight",
        "regular",
        "normal",
        "book",
        "medium",
        "semibold",
        "demibold",
        "semi",
        "demi",
        "bold",
        "extrabold",
        "ultrabold",
        "extra",
        "ultra",
        "black",
        "heavy",
        "italic",
        "oblique",
        "condensed",
        "cond",
    ];
    let mut words: Vec<&str> = family.split_whitespace().collect();
    let mut alternates = Vec::new();
    while words.len() > 1
        && words
            .last()
            .is_some_and(|w| STYLE_WORDS.iter().any(|s| w.eq_ignore_ascii_case(s)))
    {
        words.pop();
        alternates.push(words.join(" "));
    }
    alternates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_message_font() {
        let font = detect().expect("non-client metrics are always available");
        assert!(font.family.as_deref().is_some_and(|f| !f.is_empty()));
        let px = font.size_px.expect("system fonts carry an explicit height");
        // Logical pixels at 96 DPI: Segoe UI 9 pt is 12 px; anything sane is
        // well inside this range whatever the display scaling.
        assert!((6.0..=48.0).contains(&px), "{px}");
        assert_eq!(font.source, Source::WindowsNonClientMetrics);
    }

    #[test]
    fn a_styled_legacy_family_falls_back_to_the_typographic_one() {
        assert_eq!(
            without_style_words("Artifakt Element Semi Bold"),
            ["Artifakt Element Semi", "Artifakt Element"]
        );
        assert_eq!(without_style_words("Segoe UI Semibold"), ["Segoe UI"]);
        assert!(without_style_words("Segoe UI").is_empty());
        assert!(without_style_words("Black").is_empty());
    }
}
