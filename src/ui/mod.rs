/// Single source of truth for UI row height (px).
/// Change this to scale the ribbon, layer manager rows, and property panel rows uniformly.
pub const ROW_H: f32 = 26.0;

pub mod color_select;
pub mod command_line;
pub mod dock;
pub mod icons;
pub mod modal;
pub mod overlay;
pub mod popup;
pub mod properties;
pub mod read_only;
pub mod ribbon;
pub mod side_toolbar;
pub mod statusbar;
pub mod style;
pub mod text_util;
pub mod wide_menu;
pub mod window;
pub mod wrap_bar;

pub use command_line::CommandLine;
pub use properties::PropertiesPanel;
pub use ribbon::Ribbon;
pub use statusbar::StatusBar;
pub use window::layers::LayerPanel;

#[cfg(test)]
mod theme_accessibility_tests;
