//! Color representation for CAD entities

use super::aci_table;
use std::fmt;

/// Represents a color in AutoCAD
///
/// Colors can be represented in multiple ways:
/// - By index (0-256): AutoCAD Color Index (ACI)
/// - By RGB values: True color
/// - By layer: Use the layer's color
/// - By block: Use the block's color
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Color {
    /// Color by layer (index 256)
    #[default]
    ByLayer,
    /// No color (index 257 / CMC method 0xC8).
    None,
    /// Color by block (index 0)
    ByBlock,
    /// AutoCAD Color Index (1-255)
    Index(u8),
    /// True color with RGB values
    Rgb { r: u8, g: u8, b: u8 },
}

impl Color {
    /// Create a color from an AutoCAD Color Index
    pub fn from_index(index: i16) -> Self {
        match index {
            0 => Color::ByBlock,
            256 => Color::ByLayer,
            257 => Color::None,
            1..=255 => Color::Index(index as u8),
            _ if index < 0 => Color::Index((-index).min(255) as u8), // Negative means layer is off
            _ => Color::Index(7),                                    // Default to white
        }
    }

    /// Create a true color from RGB values
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Color::Rgb { r, g, b }
    }

    /// Create a color from a packed 24-bit true color value (as in DXF code 420).
    ///
    /// The value is `(r << 16) | (g << 8) | b`.
    pub fn from_true_color_value(value: i32) -> Self {
        let r = ((value >> 16) & 0xFF) as u8;
        let g = ((value >> 8) & 0xFF) as u8;
        let b = (value & 0xFF) as u8;
        Color::Rgb { r, g, b }
    }

    /// Pack this color as a 24-bit integer for DXF code 420.
    ///
    /// Returns `None` for non-RGB colors.
    pub fn to_true_color_value(&self) -> Option<i32> {
        match self {
            Color::Rgb { r, g, b } => Some(((*r as i32) << 16) | ((*g as i32) << 8) | (*b as i32)),
            _ => None,
        }
    }

    /// Get the color index (if applicable)
    pub fn index(&self) -> Option<u16> {
        match self {
            Color::ByBlock => Some(0),
            Color::Index(i) => Some(*i as u16),
            Color::ByLayer => Some(256),
            Color::None => Some(257),
            Color::Rgb { .. } => None,
        }
    }

    /// Get RGB values.
    ///
    /// For `Index` colors, looks up the canonical ACI table.
    /// For `Rgb` colors, returns the stored values directly.
    /// Returns `None` for `ByLayer` and `ByBlock`.
    pub fn rgb(&self) -> Option<(u8, u8, u8)> {
        match self {
            Color::Rgb { r, g, b } => Some((*r, *g, *b)),
            Color::Index(i) => aci_table::aci_to_rgb(*i),
            _ => None,
        }
    }

    /// Get RGB values only if this is a true color (not an index lookup).
    pub fn true_color_rgb(&self) -> Option<(u8, u8, u8)> {
        match self {
            Color::Rgb { r, g, b } => Some((*r, *g, *b)),
            _ => None,
        }
    }

    /// Whether this is a true color (RGB) rather than an index color.
    pub fn is_true_color(&self) -> bool {
        matches!(self, Color::Rgb { .. })
    }

    /// Common color constants
    pub const RED: Color = Color::Index(1);
    pub const YELLOW: Color = Color::Index(2);
    pub const GREEN: Color = Color::Index(3);
    pub const CYAN: Color = Color::Index(4);
    pub const BLUE: Color = Color::Index(5);
    pub const MAGENTA: Color = Color::Index(6);
    pub const WHITE: Color = Color::Index(7);
    pub const GRAY: Color = Color::Index(8);
    pub const LIGHT_GRAY: Color = Color::Index(9);

    /// Find the nearest ACI index for this color.
    ///
    /// For `Index` colors returns the index directly.
    /// For `Rgb` colors uses nearest-neighbor search against the canonical
    /// 256-entry ACI table.
    pub fn approximate_index(&self) -> i16 {
        match self {
            Color::ByBlock => 0,
            Color::ByLayer => 256,
            Color::None => 257,
            Color::Index(i) => *i as i16,
            Color::Rgb { r, g, b } => aci_table::nearest_aci(*r, *g, *b) as i16,
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Color::ByLayer => write!(f, "ByLayer"),
            Color::None => write!(f, "None"),
            Color::ByBlock => write!(f, "ByBlock"),
            Color::Index(i) => write!(f, "Index({})", i),
            Color::Rgb { r, g, b } => write!(f, "RGB({}, {}, {})", r, g, b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_from_index() {
        assert_eq!(Color::from_index(0), Color::ByBlock);
        assert_eq!(Color::from_index(256), Color::ByLayer);
        assert_eq!(Color::from_index(1), Color::Index(1));
    }

    #[test]
    fn test_color_rgb() {
        let color = Color::from_rgb(255, 128, 64);
        assert_eq!(color.rgb(), Some((255, 128, 64)));
        assert_eq!(color.true_color_rgb(), Some((255, 128, 64)));
        assert_eq!(color.index(), None);
    }

    #[test]
    fn test_color_index() {
        let color = Color::Index(5);
        assert_eq!(color.index(), Some(5));
        // Index colors now resolve via ACI table
        assert_eq!(color.rgb(), Some((0, 0, 255)));
        // But true_color_rgb returns None for index colors
        assert_eq!(color.true_color_rgb(), None);
    }

    #[test]
    fn test_color_constants() {
        assert_eq!(Color::RED, Color::Index(1));
        assert_eq!(Color::BLUE, Color::Index(5));
    }

    #[test]
    fn test_color_display() {
        assert_eq!(Color::ByLayer.to_string(), "ByLayer");
        assert_eq!(Color::from_rgb(255, 0, 0).to_string(), "RGB(255, 0, 0)");
    }

    #[test]
    fn test_default_color() {
        assert_eq!(Color::default(), Color::ByLayer);
    }

    #[test]
    fn test_from_true_color_value() {
        let color = Color::from_true_color_value(0xFF8040);
        assert_eq!(
            color,
            Color::Rgb {
                r: 255,
                g: 128,
                b: 64
            }
        );
    }

    #[test]
    fn test_to_true_color_value() {
        let color = Color::from_rgb(255, 128, 64);
        assert_eq!(color.to_true_color_value(), Some(0xFF8040));
        assert_eq!(Color::Index(1).to_true_color_value(), None);
    }

    #[test]
    fn test_approximate_index_nearest_neighbor() {
        // Pure red → ACI 1
        assert_eq!(Color::from_rgb(255, 0, 0).approximate_index(), 1);
        // Pure blue → ACI 5 (or 170, both are (0,0,255))
        let idx = Color::from_rgb(0, 0, 255).approximate_index();
        assert!(idx == 5 || idx == 170);
    }

    #[test]
    fn test_is_true_color() {
        assert!(Color::from_rgb(10, 20, 30).is_true_color());
        assert!(!Color::Index(1).is_true_color());
        assert!(!Color::ByLayer.is_true_color());
    }
}
