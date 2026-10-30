//! Sizes of the ribbon and the bars above it, in logical pixels, at
//! AutoCAD's proportions (100 % scale). Ribbon widget code takes its
//! numbers from here; the colours are in `chrome`.

// ── Tab row ────────────────────────────────────────────────────────────────

/// Height of the tab row; the tabs stand at its bottom.
pub const TAB_ROW_H: f32 = 27.0;
/// Height of a tab.
pub const TAB_H: f32 = 25.0;

/// A Quick Access button, square, and the gap between buttons.
pub const QAT_BUTTON: f32 = 22.0;
pub const QAT_GAP: f32 = 2.0;
/// Width of the arrow part of a Quick Access split button.
pub const QAT_SPLIT_ARROW_W: f32 = 11.0;
/// The search box shrunk to its magnifier: a square button with the icon
/// `SEARCH_BUTTON_PAD` from each side.
pub const SEARCH_BUTTON: f32 = SMALL_ICON + 2.0 * SEARCH_BUTTON_PAD;
pub const SEARCH_BUTTON_PAD: f32 = 2.0;
/// Gap between two tabs.
pub const TAB_GAP: f32 = 2.0;
/// The minimize split button after the last tab: this far from the tab,
/// its face this wide with a small icon in it, its arrow part
/// `SPLIT_ARROW_W` wide, both `QAT_BUTTON` high and standing this far above
/// the bottom of the tab row.
pub const MINIMIZE_MARGIN: f32 = 6.0;
pub const MINIMIZE_FACE_W: f32 = 24.0;
pub const MINIMIZE_BOTTOM: f32 = 1.0;

// ── Tool area ──────────────────────────────────────────────────────────────

/// Height of the tool area with panel titles.
pub const TOOL_AREA_H: f32 = CONTENT_H + TITLE_STRIP_H;
/// Height of the tool area without panel titles: a thin strip stands for
/// the title strip.
pub const TOOL_AREA_UNTITLED_H: f32 = CONTENT_H + THIN_STRIP_H;
/// Height of a panel's content area, above its title strip.
pub const CONTENT_H: f32 = 79.0;
/// Padding of the content area around the tools.
pub const CONTENT_PAD_TOP: f32 = 3.0;
pub const CONTENT_PAD_SIDE: f32 = 4.0;
pub const CONTENT_PAD_BOTTOM: f32 = 4.0;
/// Height of a panel's title strip.
pub const TITLE_STRIP_H: f32 = 20.0;
/// The dialog launcher at the right end of the title strip.
pub const LAUNCHER_W: f32 = 16.0;
pub const LAUNCHER_H: f32 = 18.0;
/// How far above a panel's bottom its slide-out starts: over the title
/// strip and the line on top of it.
pub const SLIDE_OUT_TOP: f32 = TITLE_STRIP_H + 1.0;
/// Without panel titles: the thin strip without text that stands for the
/// title strip, its dialog launcher, and where the slide-out starts.
pub const THIN_STRIP_H: f32 = 10.0;
pub const THIN_LAUNCHER_H: f32 = 9.0;
pub const SLIDE_OUT_TOP_THIN: f32 = THIN_STRIP_H + 1.0;

// ── Buttons ────────────────────────────────────────────────────────────────

/// Height of a small button and of a row of small buttons.
pub const ROW_H: f32 = 22.0;
/// Height of a combo box.
pub const COMBO_H: f32 = 22.0;
/// Gap between stacked rows and between grid buttons.
pub const GAP: f32 = 2.0;
/// Height of a large button: exactly three small rows.
pub const LARGE_H: f32 = 3.0 * ROW_H + 2.0 * GAP;
/// Width of an icon-only small button, and of the face of a small split
/// button; the icon sits `SMALL_PAD` from every side.
pub const SMALL_W: f32 = SMALL_ICON + 2.0 * SMALL_PAD;
pub const SMALL_PAD: f32 = 3.0;
/// A small button with a label: the icon `SMALL_PAD` from its left edge,
/// then this gap, the label and this right padding.
pub const LABEL_GAP: f32 = 6.0;
pub const LABEL_PAD_RIGHT: f32 = 7.0;
/// Width of the arrow part of a small split button.
pub const SPLIT_ARROW_W: f32 = 12.0;
/// Top padding of a large button, above its icon.
pub const LARGE_PAD_TOP: f32 = 3.0;
/// Where a large split button's arrow part starts; its face lights from
/// the top to a pixel above.
pub const LARGE_FACE_H: f32 = 36.0;
/// Distance from the bottom of a large split button to its arrow.
pub const LARGE_ARROW_BOTTOM: f32 = 10.0;
/// Height of a collapsed panel's button, with and without panel titles.
pub const COLLAPSED_H: f32 = 93.0;
pub const COLLAPSED_UNTITLED_H: f32 = 84.0;

// ── Minimized ribbon ───────────────────────────────────────────────────────

/// Height of the row of panel titles that stands for the tool area.
pub const MINIMIZED_TITLES_H: f32 = 20.0;
/// Height of the row of panel buttons, and the size of their icons.
pub const MINIMIZED_BUTTONS_H: f32 = 32.0;
pub const MINIMIZED_BUTTON_ICON: f32 = 22.0;

// ── Icons ──────────────────────────────────────────────────────────────────

/// Size small icons are drawn at: ribbon, menu, slide-out, search-result
/// and Quick Access icons.
pub const SMALL_ICON: f32 = 16.0;
/// Size large icons are drawn at.
pub const LARGE_ICON: f32 = 32.0;
/// Size of the icons of tiny strip buttons, such as the search box's
/// collapse button.
pub const STRIP_ICON: f32 = 14.0;
/// Size of the pin in the footer of a panel's extra tools.
pub const PIN_ICON: f32 = 11.0;
/// The dropdown arrow: a solid triangle this wide and high.
pub const DROPDOWN_ARROW_W: f32 = 5.0;
pub const DROPDOWN_ARROW_H: f32 = 3.0;
/// The chevron of a panel title strip: a solid triangle this wide and high.
pub const CHEVRON_W: f32 = 9.0;
pub const CHEVRON_H: f32 = 5.0;

// The tools fit the content area with its padding, and a collapsed panel the
// tool area.
const _: () = {
    assert!(CONTENT_PAD_TOP + LARGE_H + CONTENT_PAD_BOTTOM <= CONTENT_H);
    assert!(LARGE_H <= TOOL_AREA_UNTITLED_H);
    assert!(COLLAPSED_H <= TOOL_AREA_H && COLLAPSED_UNTITLED_H <= TOOL_AREA_UNTITLED_H);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sizes_add_up() {
        assert_eq!(TOOL_AREA_H, 99.0);
        assert_eq!(LARGE_H, 70.0);
        assert_eq!(SLIDE_OUT_TOP, 21.0);
        assert_eq!(TOOL_AREA_UNTITLED_H, 89.0);
        assert_eq!(SLIDE_OUT_TOP_THIN, 11.0);
    }
}
