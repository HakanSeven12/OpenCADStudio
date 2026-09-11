//! MLineStyle object - Multiline style definition

use crate::types::{Color, Handle};

/// Multiline style flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineStyleFlags {
    /// Fill is on
    pub fill_on: bool,
    /// Display miters at joints
    pub display_joints: bool,
    /// Start square (line) cap
    pub start_square_cap: bool,
    /// Start inner arcs cap
    pub start_inner_arcs_cap: bool,
    /// Start round (outer arcs) cap
    pub start_round_cap: bool,
    /// End square (line) cap
    pub end_square_cap: bool,
    /// End inner arcs cap
    pub end_inner_arcs_cap: bool,
    /// End round (outer arcs) cap
    pub end_round_cap: bool,
}

impl MLineStyleFlags {
    /// Create from DXF bit value
    pub fn from_bits(bits: i32) -> Self {
        Self {
            fill_on: (bits & 1) != 0,
            display_joints: (bits & 2) != 0,
            start_square_cap: (bits & 16) != 0,
            start_inner_arcs_cap: (bits & 32) != 0,
            start_round_cap: (bits & 64) != 0,
            end_square_cap: (bits & 256) != 0,
            end_inner_arcs_cap: (bits & 512) != 0,
            end_round_cap: (bits & 1024) != 0,
        }
    }

    /// Convert to DXF bit value
    pub fn to_bits(&self) -> i32 {
        let mut bits = 0;
        if self.fill_on {
            bits |= 1;
        }
        if self.display_joints {
            bits |= 2;
        }
        if self.start_square_cap {
            bits |= 16;
        }
        if self.start_inner_arcs_cap {
            bits |= 32;
        }
        if self.start_round_cap {
            bits |= 64;
        }
        if self.end_square_cap {
            bits |= 256;
        }
        if self.end_inner_arcs_cap {
            bits |= 512;
        }
        if self.end_round_cap {
            bits |= 1024;
        }
        bits
    }

    /// Create flags for a simple style (square caps)
    pub fn simple() -> Self {
        Self {
            start_square_cap: true,
            end_square_cap: true,
            ..Default::default()
        }
    }

    /// Create flags with round caps
    pub fn round_caps() -> Self {
        Self {
            start_round_cap: true,
            end_round_cap: true,
            ..Default::default()
        }
    }
}

/// Multiline style element
///
/// Each element represents one line in the multiline, with its offset,
/// color, and linetype.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineStyleElement {
    /// Element offset from the center line
    pub offset: f64,
    /// Element color
    pub color: Color,
    /// Element linetype name
    pub linetype: String,
}

impl MLineStyleElement {
    /// Create a new element with offset
    pub fn new(offset: f64) -> Self {
        Self {
            offset,
            color: Color::ByLayer,
            linetype: "BYLAYER".to_string(),
        }
    }

    /// Create an element with offset and color
    pub fn with_color(offset: f64, color: Color) -> Self {
        Self {
            offset,
            color,
            linetype: "BYLAYER".to_string(),
        }
    }

    /// Create an element with all properties
    pub fn full(offset: f64, color: Color, linetype: impl Into<String>) -> Self {
        Self {
            offset,
            color,
            linetype: linetype.into(),
        }
    }

    /// Set the color
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }

    /// Set the linetype
    pub fn set_linetype(&mut self, linetype: impl Into<String>) {
        self.linetype = linetype.into();
    }

    /// Clone this element
    pub fn duplicate(&self) -> Self {
        self.clone()
    }
}

impl Default for MLineStyleElement {
    fn default() -> Self {
        Self::new(0.0)
    }
}

/// Multiline style object
///
/// Defines the appearance of multiline entities, including the number of
/// lines, their offsets, colors, linetypes, and cap styles.
///
/// # DXF Object Type
/// MLINESTYLE
///
/// # Example
/// ```ignore
/// use acadrust::objects::{MLineStyle, MLineStyleElement};
/// use acadrust::types::Color;
///
/// let mut style = MLineStyle::new("Custom");
/// style.description = "Custom multiline style".to_string();
/// style.add_element(MLineStyleElement::new(-0.5));
/// style.add_element(MLineStyleElement::new(0.5));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineStyle {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Style name (DXF code 2)
    pub name: String,
    /// Style description (DXF code 3, max 255 characters)
    pub description: String,
    /// Line elements in the style (DXF code 71 for count)
    pub elements: Vec<MLineStyleElement>,
    /// Start cap angle in radians (DXF code 51, default: π/2)
    pub start_angle: f64,
    /// End cap angle in radians (DXF code 52, default: π/2)
    pub end_angle: f64,
    /// Fill color (DXF code 62)
    pub fill_color: Color,
    /// Style flags (DXF code 70)
    pub flags: MLineStyleFlags,
}

impl MLineStyle {
    /// Object type name
    pub const OBJECT_TYPE: &'static str = "MLINESTYLE";

    /// Default style name
    pub const DEFAULT_NAME: &'static str = "Standard";

    /// Create a new multiline style
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            name: name.into(),
            description: String::new(),
            elements: Vec::new(),
            start_angle: std::f64::consts::FRAC_PI_2, // π/2
            end_angle: std::f64::consts::FRAC_PI_2,   // π/2
            fill_color: Color::ByLayer,
            flags: MLineStyleFlags::default(),
        }
    }

    /// Create the default "Standard" style with two elements at ±0.5 offset
    pub fn standard() -> Self {
        let mut style = Self::new(Self::DEFAULT_NAME);
        style.add_element(MLineStyleElement::new(0.5));
        style.add_element(MLineStyleElement::new(-0.5));
        style
    }

    /// Add an element to the style
    pub fn add_element(&mut self, element: MLineStyleElement) {
        self.elements.push(element);
    }

    /// Add an element at a specific offset
    pub fn add_element_at_offset(&mut self, offset: f64) {
        self.elements.push(MLineStyleElement::new(offset));
    }

    /// Insert an element at a specific index
    pub fn insert_element(&mut self, index: usize, element: MLineStyleElement) {
        if index <= self.elements.len() {
            self.elements.insert(index, element);
        }
    }

    /// Remove an element at a specific index
    pub fn remove_element(&mut self, index: usize) -> Option<MLineStyleElement> {
        if index < self.elements.len() {
            Some(self.elements.remove(index))
        } else {
            None
        }
    }

    /// Get the number of elements
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Check if the style has no elements
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Get an element by index
    pub fn get_element(&self, index: usize) -> Option<&MLineStyleElement> {
        self.elements.get(index)
    }

    /// Get a mutable element by index
    pub fn get_element_mut(&mut self, index: usize) -> Option<&mut MLineStyleElement> {
        self.elements.get_mut(index)
    }

    /// Calculate the total width of the multiline style
    pub fn width(&self) -> f64 {
        if self.elements.is_empty() {
            return 0.0;
        }

        let mut min_offset = f64::MAX;
        let mut max_offset = f64::MIN;

        for element in &self.elements {
            if element.offset < min_offset {
                min_offset = element.offset;
            }
            if element.offset > max_offset {
                max_offset = element.offset;
            }
        }

        max_offset - min_offset
    }

    /// Sort elements by offset (ascending)
    pub fn sort_elements(&mut self) {
        self.elements
            .sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap());
    }

    /// Set the start angle in degrees
    pub fn set_start_angle_degrees(&mut self, degrees: f64) {
        self.start_angle = degrees.to_radians();
    }

    /// Set the end angle in degrees
    pub fn set_end_angle_degrees(&mut self, degrees: f64) {
        self.end_angle = degrees.to_radians();
    }

    /// Get the start angle in degrees
    pub fn start_angle_degrees(&self) -> f64 {
        self.start_angle.to_degrees()
    }

    /// Get the end angle in degrees
    pub fn end_angle_degrees(&self) -> f64 {
        self.end_angle.to_degrees()
    }

    /// Enable fill
    pub fn enable_fill(&mut self, color: Color) {
        self.flags.fill_on = true;
        self.fill_color = color;
    }

    /// Disable fill
    pub fn disable_fill(&mut self) {
        self.flags.fill_on = false;
    }

    /// Set round caps at start and end
    pub fn set_round_caps(&mut self) {
        self.flags.start_round_cap = true;
        self.flags.end_round_cap = true;
        self.flags.start_square_cap = false;
        self.flags.end_square_cap = false;
    }

    /// Set square caps at start and end
    pub fn set_square_caps(&mut self) {
        self.flags.start_square_cap = true;
        self.flags.end_square_cap = true;
        self.flags.start_round_cap = false;
        self.flags.end_round_cap = false;
    }

    /// Iterate over elements
    pub fn iter(&self) -> impl Iterator<Item = &MLineStyleElement> {
        self.elements.iter()
    }

    /// Iterate over elements mutably
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut MLineStyleElement> {
        self.elements.iter_mut()
    }

    /// Builder: Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Builder: Add element
    pub fn with_element(mut self, element: MLineStyleElement) -> Self {
        self.add_element(element);
        self
    }

    /// Builder: Set fill color
    pub fn with_fill(mut self, color: Color) -> Self {
        self.enable_fill(color);
        self
    }

    /// Builder: Set flags
    pub fn with_flags(mut self, flags: MLineStyleFlags) -> Self {
        self.flags = flags;
        self
    }
}

impl Default for MLineStyle {
    fn default() -> Self {
        Self::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn test_mlinestyle_creation() {
        let style = MLineStyle::new("Custom");
        assert_eq!(style.name, "Custom");
        assert!(style.is_empty());
        assert!((style.start_angle - FRAC_PI_2).abs() < 1e-10);
    }

    #[test]
    fn test_mlinestyle_standard() {
        let style = MLineStyle::standard();
        assert_eq!(style.name, "Standard");
        assert_eq!(style.element_count(), 2);

        // Check offsets are ±0.5
        let offsets: Vec<f64> = style.elements.iter().map(|e| e.offset).collect();
        assert!(offsets.contains(&0.5));
        assert!(offsets.contains(&-0.5));
    }

    #[test]
    fn test_mlinestyle_add_element() {
        let mut style = MLineStyle::new("Test");
        style.add_element(MLineStyleElement::new(0.0));
        style.add_element(MLineStyleElement::new(1.0));

        assert_eq!(style.element_count(), 2);
    }

    #[test]
    fn test_mlinestyle_remove_element() {
        let mut style = MLineStyle::standard();
        assert_eq!(style.element_count(), 2);

        let removed = style.remove_element(0);
        assert!(removed.is_some());
        assert_eq!(style.element_count(), 1);
    }

    #[test]
    fn test_mlinestyle_width() {
        let mut style = MLineStyle::new("Test");
        style.add_element(MLineStyleElement::new(-1.0));
        style.add_element(MLineStyleElement::new(0.0));
        style.add_element(MLineStyleElement::new(1.5));

        assert!((style.width() - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_mlinestyle_sort() {
        let mut style = MLineStyle::new("Test");
        style.add_element(MLineStyleElement::new(1.0));
        style.add_element(MLineStyleElement::new(-1.0));
        style.add_element(MLineStyleElement::new(0.0));

        style.sort_elements();

        assert_eq!(style.elements[0].offset, -1.0);
        assert_eq!(style.elements[1].offset, 0.0);
        assert_eq!(style.elements[2].offset, 1.0);
    }

    #[test]
    fn test_mlinestyle_element() {
        let mut element = MLineStyleElement::new(0.5);
        assert_eq!(element.offset, 0.5);
        assert_eq!(element.color, Color::ByLayer);

        element.set_color(Color::from_index(1));
        assert_eq!(element.color, Color::from_index(1));

        element.set_linetype("DASHED");
        assert_eq!(element.linetype, "DASHED");
    }

    #[test]
    fn test_mlinestyle_element_full() {
        let element = MLineStyleElement::full(0.25, Color::from_index(2), "CENTER");
        assert_eq!(element.offset, 0.25);
        assert_eq!(element.color, Color::from_index(2));
        assert_eq!(element.linetype, "CENTER");
    }

    #[test]
    fn test_mlinestyle_flags() {
        let flags = MLineStyleFlags::from_bits(1 | 16 | 256);
        assert!(flags.fill_on);
        assert!(flags.start_square_cap);
        assert!(flags.end_square_cap);
        assert!(!flags.display_joints);

        assert_eq!(flags.to_bits(), 1 | 16 | 256);
    }

    #[test]
    fn test_mlinestyle_caps() {
        let mut style = MLineStyle::new("Test");

        style.set_round_caps();
        assert!(style.flags.start_round_cap);
        assert!(style.flags.end_round_cap);
        assert!(!style.flags.start_square_cap);

        style.set_square_caps();
        assert!(style.flags.start_square_cap);
        assert!(style.flags.end_square_cap);
        assert!(!style.flags.start_round_cap);
    }

    #[test]
    fn test_mlinestyle_fill() {
        let mut style = MLineStyle::new("Test");

        style.enable_fill(Color::from_index(5));
        assert!(style.flags.fill_on);
        assert_eq!(style.fill_color, Color::from_index(5));

        style.disable_fill();
        assert!(!style.flags.fill_on);
    }

    #[test]
    fn test_mlinestyle_builder() {
        let style = MLineStyle::new("Test")
            .with_description("A test style")
            .with_element(MLineStyleElement::new(-0.5))
            .with_element(MLineStyleElement::new(0.5))
            .with_fill(Color::from_index(3));

        assert_eq!(style.description, "A test style");
        assert_eq!(style.element_count(), 2);
        assert!(style.flags.fill_on);
    }

    #[test]
    fn test_mlinestyle_angles() {
        let mut style = MLineStyle::new("Test");

        style.set_start_angle_degrees(45.0);
        style.set_end_angle_degrees(60.0);

        assert!((style.start_angle_degrees() - 45.0).abs() < 1e-10);
        assert!((style.end_angle_degrees() - 60.0).abs() < 1e-10);
    }
}
