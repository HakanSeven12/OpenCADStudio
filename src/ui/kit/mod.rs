//! The palette kit: the widgets every palette is built from, so all
//! palettes share one look (the Properties palette's) and no palette styles
//! a row, a bar or a field by hand.
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
//! | Menus, tooltips | [`crate::ui::palette_menu`], [`tooltip`] |
//!
//! Colours come from [`crate::ui::style::chrome`], text sizes from
//! [`crate::ui::style::text`], sizes from
//! [`crate::ui::style::palette_metrics`].

mod dropdown;
mod field;
mod frame;
mod grid;
mod list;
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
