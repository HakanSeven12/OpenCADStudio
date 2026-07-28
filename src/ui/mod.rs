/// Single source of truth for UI row height (px).
/// Change this to scale the ribbon, layer manager rows, and property panel rows uniformly.
pub const ROW_H: f32 = 26.0;

/// Place `content` at fixed top-left coordinates inside a fill-sized layer.
/// Negative coordinates clamp to the layer edge.
pub fn pin_at<'a, Message: 'a>(
    position: iced::Point,
    content: impl Into<iced::Element<'a, Message>>,
) -> iced::Element<'a, Message> {
    iced::widget::pin(content)
        .position(iced::Point::new(position.x.max(0.0), position.y.max(0.0)))
        .into()
}

pub mod color_select;
pub mod command_line;
pub mod icons;
pub mod modal;
pub mod overlay;
pub mod popup;
pub mod properties;
pub mod ribbon;
pub mod side_toolbar;
pub mod statusbar;
pub mod style;
pub mod text_util;
pub mod window;
pub mod wrap_bar;

pub use command_line::CommandLine;
pub use properties::PropertiesPanel;
pub use ribbon::Ribbon;
pub use statusbar::StatusBar;
pub use window::layers::LayerPanel;
