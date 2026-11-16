//! Sizes of the Start page. It is a welcome screen, larger than the
//! palettes, so it keeps a scale of its own; its small text comes from
//! [`super::text`].

/// The "Open CAD Studio" headline, a section title, an action's caption.
pub const HEADLINE: f32 = 40.0;
pub const SECTION_TITLE: f32 = 15.0;
pub const ACTION_TEXT: f32 = 14.0;

/// An action button's padding and the gap between action buttons.
pub const ACTION_PAD: [f32; 2] = [10.0, 22.0];
pub const ACTION_GAP: f32 = 12.0;
pub const ACTION_RADIUS: f32 = 6.0;
/// The brand icon in an action button.
pub const ACTION_ICON: f32 = 20.0;

/// Gaps, smallest to largest.
pub const GAP_XS: f32 = 2.0;
pub const GAP_S: f32 = 4.0;
pub const GAP: f32 = 6.0;
pub const GAP_M: f32 = 8.0;
pub const GAP_L: f32 = 10.0;
pub const GAP_XL: f32 = 16.0;
/// Room under a section's title.
pub const SECTION_GAP: f32 = 12.0;

/// A card's and a panel's padding.
pub const CARD_PAD: f32 = 16.0;
pub const PANEL_PAD: f32 = 20.0;
/// A list row's and a chip's padding.
pub const ROW_PAD: [f32; 2] = [4.0, 10.0];
pub const CHIP_PAD: [f32; 2] = [2.0, 6.0];
pub const BADGE_PAD: [f32; 2] = [3.0, 6.0];
pub const TAB_PAD: [f32; 2] = [6.0, 10.0];

/// A recent drawing's thumbnail in the list view, and the tile's preview.
pub const THUMB_W: f32 = 46.0;
pub const THUMB_H: f32 = 34.0;
pub const TILE_PREVIEW_H: f32 = 180.0;
pub const SMALL_TILE_W: f32 = 72.0;
pub const SMALL_TILE_H: f32 = 54.0;
/// The templates list beside "New from template" and its row.
pub const TEMPLATE_LIST_W: f32 = 260.0;
pub const TEMPLATE_ROW_H: f32 = 26.0;
/// The sponsor logo's height.
pub const SPONSOR_H: f32 = 120.0;
