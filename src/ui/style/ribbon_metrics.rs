//! Sizes of the ribbon and the bars above it, in logical pixels, at the
//! reference proportions (100 % scale). Ribbon widget code takes its
//! numbers from here; the colours are in `chrome`.

// ── Tab row ────────────────────────────────────────────────────────────────

/// Height of the tab row; the tabs stand at its bottom.
pub const TAB_ROW_H: f32 = 27.0;
/// Height of a tab, and the room under its text: the text stands where it
/// did in a 25 px tab, with less of the tab above it.
pub const TAB_H: f32 = 23.0;
pub const TAB_TEXT_RAISE: f32 = 2.0;

/// A Quick Access button, square, and the gap between buttons.
pub const QAT_BUTTON: f32 = 22.0;
pub const QAT_GAP: f32 = 2.0;
/// Room on each side of the name of a Quick Access command with no icon.
pub const QAT_TEXT_PAD: f32 = 5.0;
/// Width of the arrow part of a Quick Access split button.
pub const QAT_SPLIT_ARROW_W: f32 = 11.0;
/// The Quick Access layer combo: as high as a Quick Access button, this
/// wide, with this margin on each side.
pub const QAT_COMBO_W: f32 = 210.0;
pub const QAT_COMBO_MARGIN: f32 = 3.0;
/// The toolbar's copies of Home > Properties' lists (colour, linetype,
/// lineweight), as wide as on the ribbon.
pub const QAT_PROP_COMBO_W: f32 = 130.0;
/// The toolbar's copies of the Annotate style and layer lists.
pub const QAT_STYLE_COMBO_W: f32 = 150.0;
/// A toolbar list's colour swatch.
pub const QAT_COMBO_SWATCH: f32 = 12.0;
/// A Quick Access separator: a line this high, with this margin on each
/// side.
pub const QAT_SEPARATOR_H: f32 = 16.0;
pub const QAT_SEPARATOR_MARGIN: f32 = 4.0;
/// The Quick Access menu buttons (Switch Windows, User Interface): icon
/// and arrow.
pub const QAT_MENU_BUTTON_W: f32 = QAT_BUTTON + QAT_SPLIT_ARROW_W;
/// The Quick Access Toolbar's own row below the ribbon: its height and
/// the room at either end.
pub const QAT_ROW_H: f32 = 26.0;
pub const QAT_ROW_PAD: f32 = 6.0;
/// The customize button at the end of the Quick Access Toolbar.
pub const QAT_CUSTOMIZE_W: f32 = 20.0;
/// The title of a Quick Access menu.
pub const QAT_MENU_TITLE_SIZE: f32 = 11.5;
/// The Quick Access menus: the title's room, a separator's room, the
/// size of a check mark, the size of plain menu text (and of a command
/// shown by its name), and the gap between an icon and its ▾.
pub const QAT_MENU_TITLE_PAD: [f32; 2] = [4.0, 12.0];
pub const QAT_MENU_SEP_PAD: [f32; 2] = [3.0, 6.0];
pub const QAT_MENU_CHECK: f32 = 10.0;
pub const QAT_TEXT_SIZE: f32 = 11.0;
pub const QAT_ARROW_GAP: f32 = 2.0;
/// More Commands: its width, the most its result list grows to, a result
/// row's height and room, the gap after a result's icon, its name and
/// place sizes, the search field's text size and room, and the room
/// around "no matches".
pub const QAT_PICKER_W: f32 = 340.0;
pub const QAT_PICKER_LIST_H: f32 = 320.0;
pub const QAT_PICKER_ROW_H: f32 = 34.0;
pub const QAT_PICKER_ROW_PAD: [f32; 2] = [3.0, 10.0];
pub const QAT_PICKER_ICON_GAP: f32 = 8.0;
pub const QAT_PICKER_LABEL_SIZE: f32 = 12.0;
pub const QAT_PICKER_PATH_SIZE: f32 = 10.0;
pub const QAT_PICKER_INPUT_SIZE: f32 = 11.5;
pub const QAT_PICKER_INPUT_PAD: [f32; 2] = [4.0, 6.0];
pub const QAT_PICKER_EMPTY_PAD: [f32; 2] = [6.0, 10.0];
/// Gap between two tabs: none, so a hovered tab meets the active one.
pub const TAB_GAP: f32 = 0.0;
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
/// Padding of the content area around the tools: the 9 px the 70 px tools
/// leave split as evenly as whole pixels allow.
pub const CONTENT_PAD_TOP: f32 = 4.0;
pub const CONTENT_PAD_SIDE: f32 = 4.0;
pub const CONTENT_PAD_BOTTOM: f32 = 5.0;
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

// ── Floating panels ────────────────────────────────────────────────────────

/// How far above or below the tool area the pointer goes before a
/// dragged panel leaves the ribbon to float.
pub const FLOAT_DRAG_MARGIN: f32 = 24.0;
/// How far the pointer moves from a press on a floating panel's title
/// strip before the press moves the panel instead of clicking.
pub const FLOAT_MOVE_THRESHOLD: f32 = 3.0;
/// The least width of a floating panel's extra tools, before the panel
/// has been drawn.
pub const FLOAT_EXTRAS_MIN_W: f32 = 120.0;
/// Size of a floating panel's title in its footer and side strip.
pub const FLOAT_TITLE_SIZE: f32 = 11.5;
/// A floating panel's band of extra tools, to the right of a side title:
/// its buttons, the room at its ends and above and below the lines
/// between its groups, the gap on either side of those lines, and the
/// width of a list in it (the text or center layer list).
pub const FLOAT_BAND_BUTTON: f32 = 21.0;
pub const FLOAT_BAND_PAD: f32 = 6.0;
pub const FLOAT_BAND_GROUP_GAP: f32 = 6.0;
pub const FLOAT_BAND_LIST_W: f32 = 180.0;
/// The side title strip: the room above the title, the arrow under it and
/// the gap around that arrow.
pub const FLOAT_SIDE_TEXT_PAD: f32 = 5.0;
pub const FLOAT_SIDE_ARROW: f32 = 6.0;
pub const FLOAT_SIDE_ARROW_GAP: f32 = 4.0;
/// How near a moved floating panel's edge comes to another edge before
/// it snaps there.
pub const FLOAT_SNAP: f32 = 8.0;
/// A floating panel's hover-only buttons (Return to ribbon, the
/// orientation toggle, the pin in the corner tab): the icon, its hit area,
/// the gap between two, their inset from the strip's left end, and the
/// room inside the corner tab.
pub const FLOAT_HOVER_ICON: f32 = 14.0;
pub const FLOAT_HOVER_HIT: f32 = 16.0;
pub const FLOAT_HOVER_GAP: f32 = 2.0;
pub const FLOAT_HOVER_INSET: f32 = 3.0;
pub const FLOAT_TAB_PAD: f32 = 2.0;
/// The footer of a floating panel's extra tools: the room around its pin,
/// at its ends, and between its title and ▴.
pub const FLOAT_FOOTER_PIN_PAD: [f32; 2] = [1.0, 4.0];
pub const FLOAT_FOOTER_PAD: [f32; 2] = [0.0, 3.0];
pub const FLOAT_TITLE_GAP: f32 = 4.0;
/// Width of a floating panel's title strip at its side, and of the footer
/// at the end of its extra tools band.
pub const FLOAT_SIDE_STRIP_W: f32 = 20.0;

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

// The tools fit the content area with its padding, and a collapsed panel the
// tool area.
const _: () = {
    assert!(CONTENT_PAD_TOP + LARGE_H + CONTENT_PAD_BOTTOM == CONTENT_H);
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
