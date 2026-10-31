//! The width a line of UI text takes, measured with the renderer's font
//! metrics, so menus can be as wide as their longest entry. Widths are
//! remembered, since menus are built every frame, until the UI font changes.

use std::cell::RefCell;

use iced::advanced::text::{self, Paragraph as _};
use iced::{Font, Pixels, Size};
use rustc_hash::FxHashMap;

/// Measured widths, by size (its bits) and weight, then by text: a lookup
/// borrows the text instead of copying it.
type Widths = FxHashMap<(u32, bool), FxHashMap<String, f32>>;

thread_local! {
    /// The UI font generation the widths were measured in, and the widths.
    static WIDTHS: RefCell<(u32, Widths)> = RefCell::new((0, Widths::default()));
}

/// Width of `content` on one line at `size`, in the default font, bold when
/// `bold`.
pub fn width(content: &str, size: f32, bold: bool) -> f32 {
    let generation = crate::sys::ui_font::generation();
    WIDTHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.0 != generation {
            *cache = (generation, Widths::default());
        }
        let widths = cache.1.entry((size.to_bits(), bold)).or_default();
        if let Some(width) = widths.get(content) {
            return *width;
        }
        let width = measure(content, size, bold);
        widths.insert(content.to_owned(), width);
        width
    })
}

fn measure(content: &str, size: f32, bold: bool) -> f32 {
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
