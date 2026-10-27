//! Sizes of the ribbon and the bars above it, in logical pixels, at
//! AutoCAD's proportions (100 % scale). Ribbon widget code takes its
//! numbers from here; the colours are in `chrome`.

// ── Tab row ────────────────────────────────────────────────────────────────

/// Height of the tab row; the tabs stand at its bottom.
pub const TAB_ROW_H: f32 = 27.0;
/// Height of a tab.
pub const TAB_H: f32 = 25.0;

// ── Tool area ──────────────────────────────────────────────────────────────

/// Height of the tool area with panel titles.
pub const TOOL_AREA_H: f32 = CONTENT_H + TITLE_STRIP_H;
/// Height of the tool area without panel titles.
pub const TOOL_AREA_UNTITLED_H: f32 = 89.0;
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

// ── Buttons ────────────────────────────────────────────────────────────────

/// Height of a small button and of a row of small buttons.
pub const ROW_H: f32 = 22.0;
/// Height of a combo box.
pub const COMBO_H: f32 = 22.0;
/// Gap between stacked rows and between grid buttons.
pub const GAP: f32 = 2.0;
/// Height of a large button: exactly three small rows.
pub const LARGE_H: f32 = 3.0 * ROW_H + 2.0 * GAP;
/// Height of a collapsed panel's button, with and without panel titles.
pub const COLLAPSED_H: f32 = 93.0;
pub const COLLAPSED_UNTITLED_H: f32 = 84.0;

// ── Icons ──────────────────────────────────────────────────────────────────

/// Size small icons are drawn at: ribbon, menu, slide-out, search-result
/// and Quick Access icons.
pub const SMALL_ICON: f32 = 16.0;
/// Size large icons are drawn at.
pub const LARGE_ICON: f32 = 32.0;

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
    }
}
