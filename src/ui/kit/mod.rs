//! The UI kit: the widgets the palettes, menus, dropdowns, ribbon lists,
//! command line and tooltips are built from, so they share one look (the
//! Properties palette's) and nothing styles a row, a bar, a field or a menu
//! by hand.
//!
//! A palette is the same stack, top to bottom; every band but the title bar
//! and the content is optional:
//!
//! | Band | Widget |
//! |---|---|
//! | Title bar | [`crate::ui::dock::title_bar_with`] |
//! | Toolbar | [`toolbar`] |
//! | Selector / search | [`select_box`], [`search_field`], [`dropdown`] |
//! | Tabs | [`tabs`] |
//! | Content | property grid ([`row_view`]), list ([`list_row`] under a [`column_header`]), tree ([`tree_row`]), cards ([`card_grid`]) or a canvas of its own |
//! | Category bars | [`category_bar`] |
//! | Scroll area | [`scroll_area`] |
//! | Empty state | [`empty_state`] |
//! | Footer | [`footer`] |
//! | Frame | [`palette`], [`band`], [`crate::ui::dock::SplitterGrip`] |
//! | Menus, tooltips | [`crate::ui::kit::menu`], [`tooltip`] |
//!
//! Dropdowns are [`dropdown`] (a field-style head over a list as wide as
//! its longest option); menus are [`menu`], whose frame
//! ([`menu::frame_style`], [`menu::inset`]) every popup list shares.
//!
//! Rules for code built on the kit:
//!
//! - Colours come from [`crate::ui::style::chrome`] (`surface` and
//!   `surface_raised` for palette and field backgrounds), never from
//!   `theme.palette()` or a literal colour.
//! - Text sizes come from [`crate::ui::style::text`]; sizes from
//!   [`crate::ui::style::kit_metrics`] (imported as `km`), or
//!   `ribbon_metrics` / `start_metrics` for the ribbon and the Start page.
//! - Drawn lines are [`crate::ui::style::kit_metrics::line`], one physical
//!   pixel.
//! - Context menus press their rows through a `mouse_area`: a menu overlay
//!   is rebuilt every view pass and a button would lose its press.
//! - `docs/ui/audit_counts.sh` counts generic colours and literal sizes in
//!   the kit's users; CI reports a file that goes above its baseline in
//!   `docs/ui/audit_counts.txt`, and the baseline only goes down.

mod dropdown;
mod field;
mod frame;
mod grid;
mod list;
pub mod menu;
mod press;
mod status;
mod tabs;
mod toolbar;
mod tooltip;
mod tree;

pub use dropdown::*;
pub use field::*;
pub use frame::*;
pub use grid::*;
pub use list::*;
pub use press::*;
pub use status::*;
pub use tabs::*;
pub use toolbar::*;
pub use tooltip::*;
pub use tree::*;
