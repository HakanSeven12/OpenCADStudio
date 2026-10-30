//! The width a line of UI text takes, measured with the renderer's font
//! metrics, so menus can be as wide as their longest entry.

use iced::advanced::text::{self, Paragraph as _};
use iced::{Font, Pixels, Size};

/// Width of `content` on one line at `size`, in the default font, bold when
/// `bold`.
pub fn width(content: &str, size: f32, bold: bool) -> f32 {
    let font = if bold {
        Font {
            weight: iced::font::Weight::Bold,
            ..Font::DEFAULT
        }
    } else {
        Font::DEFAULT
    };
    let paragraph = <iced::Renderer as text::Renderer>::Paragraph::with_text(text::Text {
        content,
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: Pixels(size),
        line_height: text::LineHeight::default(),
        font,
        align_x: text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::default(),
        wrapping: text::Wrapping::None,
        ellipsis: text::Ellipsis::None,
        hint_factor: None,
    });
    paragraph.min_bounds().width.ceil()
}
