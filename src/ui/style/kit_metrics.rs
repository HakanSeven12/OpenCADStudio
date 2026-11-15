//! Measurements of the palette area, from the prototype board "palettes up
//! to the ribbon": the palette strip, the palette title bar and splitter,
//! the Properties grid and the drawing-tab bar. Logical pixels, whole
//! numbers. Code in those areas refers to these names, not to numbers.
//!
//! Colours live in [`super::chrome`] (`surface`, `surface_raised`,
//! `strip`, `edge`, ...), text sizes in [`super::text`].

/// A 1 px line: edges, grid lines, dividers. For sizes that count a line
/// in; a line as drawn is [`line`].
pub const LINE: f32 = 1.0;

/// A drawn line (an edge, a grid line, a border, a divider): one physical
/// pixel at any scale, as the ribbon's hairlines, so it stays thin at
/// 125 % and 150 % instead of covering one or two pixels.
pub fn line() -> f32 {
    crate::ui::icons::hairline()
}

// ── Palette strip ─────────────────────────────────────────────────────────

/// The light part of the strip, where the icons and names are centred.
pub const STRIP_BODY_W: f32 = 30.0;
/// The whole strip: its light part plus the 1 px `edge` line.
pub const STRIP_W: f32 = STRIP_BODY_W + LINE;
/// Height of one icon cell.
pub const STRIP_CELL_H: f32 = 28.0;
/// Size of a strip icon.
pub const STRIP_ICON: f32 = 20.0;
/// Room above a group's first cell and below its last one; part of those
/// cells (hover and hit areas run over it).
pub const STRIP_GROUP_PAD: f32 = 2.0;
/// Size of the + icon in the strip.
pub const STRIP_PLUS_ICON: f32 = 14.0;
/// Height of the + cell under the groups.
pub const STRIP_PLUS_H: f32 = STRIP_CELL_H;
/// Room before and after a vertical name in a strip cell.
pub const STRIP_NAME_PAD: f32 = 8.0;
/// Gap between an icon and its name above it.
pub const STRIP_NAME_GAP: f32 = 4.0;

// ── Floating palette ──────────────────────────────────────────────────────

/// Width of a floating palette's vertical title bar.
pub const FLOAT_BAR_W: f32 = 22.0;
/// Size of the icons on that bar (its palette's icon, close, pin).
pub const FLOAT_ICON: f32 = 16.0;

// ── Palette title bar and splitter ────────────────────────────────────────

/// Height of a palette's title bar.
pub const TITLE_H: f32 = 23.0;
/// Top of the title's 16 px line box in the bar, so its capitals are
/// centred on the bar (and on its buttons).
pub const TITLE_TEXT_TOP: f32 = 4.0;
/// Left padding of the title text.
pub const TITLE_PAD_X: f32 = 8.0;
/// Size of a title-bar button (☰, pin, detach, close).
pub const TITLE_BUTTON: f32 = 19.0;
/// Icon size inside a title-bar button.
pub const TITLE_ICON: f32 = 14.0;
/// Height of the splitter between stacked palettes.
pub const SPLITTER_H: f32 = 7.0;
/// Grip dots on the splitter: count, size and gap.
pub const SPLITTER_DOTS: usize = 5;
pub const SPLITTER_DOT: f32 = 2.0;
pub const SPLITTER_DOT_GAP: f32 = 2.0;

// ── Properties grid ───────────────────────────────────────────────────────

/// Height of a property row.
pub const ROW_H: f32 = 18.0;
/// Room above a category bar, parting it from the rows before.
pub const CATEGORY_GAP: f32 = 3.0;
/// Height of a category bar.
pub const CATEGORY_H: f32 = 18.0;
/// Fixed width of the name column (the value column takes the rest).
pub const NAME_COL_W: f32 = 118.0;
/// Indent before the name (no grid lines in it).
pub const INDENT: f32 = 14.0;
/// Inset of the grid from the palette's left edge (aligned with the object
/// list box above it).
pub const GRID_INSET_L: f32 = 8.0;
/// Inset of the grid from the palette's right edge, the scrollbar gutter
/// included, whether or not the scrollbar shows.
pub const GRID_INSET_R: f32 = 10.0;
/// Width of the scrollbar gutter.
pub const SCROLLBAR_W: f32 = 10.0;
/// Width of the scrollbar thumb, centred in the gutter.
pub const SCROLL_THUMB_W: f32 = 6.0;
/// Horizontal padding of text in a grid cell.
pub const CELL_PAD_X: f32 = 4.0;
/// Size of the category caret.
pub const CARET: f32 = 10.0;
/// Gap between controls at the top of a palette (object list, toolbar).
pub const TOOL_GAP: f32 = 4.0;
/// A tooltip's padding, vertical and horizontal: just enough to keep the
/// text off its border (a tooltip lines up with nothing, so no indent).
pub const TIP_PAD: [f32; 2] = [2.0, 5.0];
/// Room above a palette's first band, under its title bar: as wide as the
/// left inset.
pub const TOP_INSET: f32 = GRID_INSET_L;
/// Height of the object list combo and the toolbar buttons above the grid.
pub const TOOL_H: f32 = 22.0;
/// Icon size in a toolbar button and in a list or tree row.
pub const ROW_ICON: f32 = 16.0;
/// Side of a small square button on a category bar (fold all, add, ...).
pub const BAR_BUTTON: f32 = 18.0;
/// Icon size in such a button.
pub const BAR_ICON: f32 = 12.0;
/// Size of a check box in a row.
pub const CHECK: f32 = 14.0;

// ── Drawing-tab bar ───────────────────────────────────────────────────────

/// Height of a tab (it fills the bar above the line).
pub const TAB_H: f32 = 23.0;
/// The `surface` line under the tabs, joined to the active tab.
pub const TAB_LINE: f32 = 2.0;
/// The tab bar: 23 px of tabs and the 2 px line under them.
pub const TAB_BAR_H: f32 = TAB_H + TAB_LINE;
/// Gap between tabs.
pub const TAB_GAP: f32 = 1.0;
/// Horizontal padding inside a tab.
pub const TAB_PAD_X: f32 = 10.0;
/// Gap between a tab's dirty dot, name and ×.
pub const TAB_INNER_GAP: f32 = 6.0;
/// The line a docked palette's resize band shows under the pointer.
pub const DIVIDER_LINE: f32 = 2.0;
/// Size of the dirty dot.
pub const DIRTY_DOT: f32 = 6.0;
/// Width of the + and ☰ buttons in the tab bar.
pub const TAB_BUTTON_W: f32 = 24.0;
/// Delay before the layouts list of a hovered tab opens, in milliseconds.
pub const TAB_HOVER_MS: u64 = 350;

// ── Menus (☰ menu, tab hover list, strip right-click menu) ───────────────

/// Height of a menu item.
pub const MENU_ITEM_H: f32 = 22.0;
/// Width of the check-mark column of a menu.
pub const MENU_CHECK_W: f32 = 20.0;
/// Horizontal padding of a menu item after the check column.
pub const MENU_PAD_X: f32 = 10.0;
/// Vertical padding of a menu frame: none, so the first and last items'
/// highlights run up to the frame.
pub const MENU_PAD_Y: f32 = 0.0;
/// Height of a separator (a 1 px line in it).
pub const MENU_SEPARATOR_H: f32 = 7.0;
/// Width of a menu.
pub const MENU_W: f32 = 220.0;
/// A dropdown option's side padding, and the frame's padding around a
/// dropdown list.
pub const DROPDOWN_ITEM_PAD_X: f32 = 6.0;
pub const DROPDOWN_PAD: f32 = LINE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_measurements() {
        assert_eq!(STRIP_W, 31.0);
        assert_eq!(TAB_H + TAB_LINE, TAB_BAR_H);
        const {
            assert!(STRIP_ICON < STRIP_CELL_H);
            assert!(TITLE_BUTTON <= TITLE_H);
            assert!(SCROLL_THUMB_W < SCROLLBAR_W);
        }
        assert!(
            SPLITTER_DOTS as f32 * (SPLITTER_DOT + SPLITTER_DOT_GAP) < 40.0,
            "the grip stays a small centred mark"
        );
    }

    #[test]
    fn every_measurement_is_whole_pixels() {
        for v in [
            STRIP_BODY_W,
            STRIP_W,
            STRIP_CELL_H,
            STRIP_ICON,
            STRIP_GROUP_PAD,
            TITLE_H,
            SPLITTER_H,
            ROW_H,
            CATEGORY_H,
            NAME_COL_W,
            INDENT,
            GRID_INSET_L,
            GRID_INSET_R,
            SCROLLBAR_W,
            SCROLL_THUMB_W,
            TAB_BAR_H,
            TAB_H,
            MENU_ITEM_H,
        ] {
            assert_eq!(v.fract(), 0.0, "{v}");
        }
    }
}
