//! macOS UI-font discovery.
//!
//! AppKit's system font (`+[NSFont systemFontOfSize:0]`) at
//! `+[NSFont systemFontSize]`. macOS expresses sizes in points that are
//! already logical pixels (the Retina backing scale is separate), so the
//! size is used as-is.
//!
//! The family AppKit reports is a private name (`.AppleSystemUIFont`) that
//! font databases list differently, so the names San Francisco is exposed
//! under are carried as alternates; the typography layer picks the first
//! one the renderer actually has.

use super::{Source, SystemUiFont};
use objc2_app_kit::NSFont;

const SYSTEM_FONT_ALIASES: [&str; 5] = [
    ".SF NS",
    "System Font",
    "SF Pro Text",
    "SF Pro",
    "Helvetica Neue",
];

pub(super) fn detect() -> Option<SystemUiFont> {
    // SAFETY: these class methods have no preconditions and may be called
    // from any thread.
    let (family, size) = unsafe {
        let size = NSFont::systemFontSize();
        let font = NSFont::systemFontOfSize(0.0);
        (font.familyName().map(|name| name.to_string()), size)
    };
    let size = size as f32;
    Some(SystemUiFont {
        family,
        alternates: SYSTEM_FONT_ALIASES.iter().map(|s| s.to_string()).collect(),
        weight: 400,
        italic: false,
        size_px: (size.is_finite() && size > 0.0).then_some(size),
        source: Source::MacSystemFont,
    })
}
