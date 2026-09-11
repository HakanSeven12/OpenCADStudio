//! AttributeDefinition entity - Block attribute template

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};
use std::f64::consts::PI;

/// Attribute flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttributeFlags {
    /// Attribute is invisible
    pub invisible: bool,
    /// Attribute is constant (value cannot be changed)
    pub constant: bool,
    /// Verification required on input
    pub verify: bool,
    /// Attribute is preset (no prompt during insertion)
    pub preset: bool,
    /// Attribute may not be moved
    pub locked_position: bool,
    /// Attribute is in annotative block
    pub annotative: bool,
}

impl AttributeFlags {
    /// Create from DXF bit flag value
    pub fn from_bits(bits: i32) -> Self {
        Self {
            invisible: (bits & 1) != 0,
            constant: (bits & 2) != 0,
            verify: (bits & 4) != 0,
            preset: (bits & 8) != 0,
            locked_position: (bits & 16) != 0,
            annotative: (bits & 128) != 0,
        }
    }

    /// Convert to DXF bit flag value
    pub fn to_bits(&self) -> i32 {
        let mut bits = 0;
        if self.invisible {
            bits |= 1;
        }
        if self.constant {
            bits |= 2;
        }
        if self.verify {
            bits |= 4;
        }
        if self.preset {
            bits |= 8;
        }
        if self.locked_position {
            bits |= 16;
        }
        if self.annotative {
            bits |= 128;
        }
        bits
    }
}

/// Text horizontal alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HorizontalAlignment {
    /// Left alignment
    #[default]
    Left = 0,
    /// Center alignment
    Center = 1,
    /// Right alignment
    Right = 2,
    /// Aligned (fit between two points)
    Aligned = 3,
    /// Middle alignment
    Middle = 4,
    /// Fit (stretch to fit between two points)
    Fit = 5,
}

impl HorizontalAlignment {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            1 => HorizontalAlignment::Center,
            2 => HorizontalAlignment::Right,
            3 => HorizontalAlignment::Aligned,
            4 => HorizontalAlignment::Middle,
            5 => HorizontalAlignment::Fit,
            _ => HorizontalAlignment::Left,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }
}

/// Text vertical alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VerticalAlignment {
    /// Baseline alignment
    #[default]
    Baseline = 0,
    /// Bottom alignment
    Bottom = 1,
    /// Middle alignment
    Middle = 2,
    /// Top alignment
    Top = 3,
}

impl VerticalAlignment {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            1 => VerticalAlignment::Bottom,
            2 => VerticalAlignment::Middle,
            3 => VerticalAlignment::Top,
            _ => VerticalAlignment::Baseline,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }
}

/// Multiline attribute type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MTextFlag {
    /// Single-line attribute
    #[default]
    SingleLine = 0,
    /// Multiline attribute
    MultiLine = 2,
    /// Constant multiline attribute
    ConstantMultiLine = 4,
}

impl MTextFlag {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            2 => MTextFlag::MultiLine,
            4 => MTextFlag::ConstantMultiLine,
            _ => MTextFlag::SingleLine,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }
}

/// AttributeDefinition entity - defines a template for block attributes
///
/// Attribute definitions are placed inside block definitions and specify
/// the tag name, prompt, default value, and display properties for block
/// attributes. When a block is inserted, ATTRIB entities are created from
/// these definitions.
///
/// # DXF Entity Type
/// ATTDEF
///
/// # Example
/// ```ignore
/// use acadrust::entities::AttributeDefinition;
/// use acadrust::types::Vector3;
///
/// let mut attdef = AttributeDefinition::new(
///     "PART_NUMBER".to_string(),
///     "Enter part number:".to_string(),
///     "PN-0001".to_string(),
/// );
/// attdef.insertion_point = Vector3::new(0.0, 0.0, 0.0);
/// attdef.height = 2.5;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttributeDefinition {
    /// Common entity properties
    pub common: EntityCommon,
    /// Attribute tag (identifier)
    pub tag: String,
    /// Prompt string for user input
    pub prompt: String,
    /// Default value
    pub default_value: String,
    /// Text insertion point
    pub insertion_point: Vector3,
    /// Second alignment point (for non-left alignments)
    pub alignment_point: Vector3,
    /// Text height
    pub height: f64,
    /// Rotation angle in radians
    pub rotation: f64,
    /// Relative X scale factor (width factor)
    pub width_factor: f64,
    /// Oblique angle in radians
    pub oblique_angle: f64,
    /// Text style name
    pub text_style: String,
    /// Text generation flags
    pub text_generation_flags: i16,
    /// Horizontal alignment
    pub horizontal_alignment: HorizontalAlignment,
    /// Vertical alignment
    pub vertical_alignment: VerticalAlignment,
    /// Attribute flags
    pub flags: AttributeFlags,
    /// Field length (optional)
    pub field_length: i16,
    /// Extrusion direction
    pub normal: Vector3,
    /// Multiline text flag
    pub mtext_flag: MTextFlag,
    /// Is this really a multiline attribute
    pub is_multiline: bool,
    /// Number of lines for multiline attribute
    pub line_count: i16,
    /// Embedded MTEXT payload used by R2018+ multiline attributes.
    ///
    /// The plain attribute text only carries the first line. Keeping the
    /// embedded object preserves its layout, style, background and full value.
    pub embedded_mtext: Option<Box<crate::entities::MText>>,
    /// Lock position in block
    pub lock_position: bool,
}

impl AttributeDefinition {
    /// Create a new attribute definition
    pub fn new(tag: String, prompt: String, default_value: String) -> Self {
        Self {
            common: EntityCommon::default(),
            tag,
            prompt,
            default_value,
            insertion_point: Vector3::ZERO,
            alignment_point: Vector3::ZERO,
            height: 2.5,
            rotation: 0.0,
            width_factor: 1.0,
            oblique_angle: 0.0,
            text_style: "Standard".to_string(),
            text_generation_flags: 0,
            horizontal_alignment: HorizontalAlignment::Left,
            vertical_alignment: VerticalAlignment::Baseline,
            flags: AttributeFlags::default(),
            field_length: 0,
            normal: Vector3::UNIT_Z,
            mtext_flag: MTextFlag::SingleLine,
            is_multiline: false,
            line_count: 1,
            embedded_mtext: None,
            lock_position: false,
        }
    }

    /// Create a simple attribute definition with just a tag
    pub fn simple(tag: impl Into<String>) -> Self {
        let tag = tag.into();
        Self::new(tag.clone(), format!("Enter {}:", tag), String::new())
    }

    /// Create an invisible attribute definition
    pub fn invisible(tag: impl Into<String>, default_value: impl Into<String>) -> Self {
        let mut attdef = Self::simple(tag);
        attdef.default_value = default_value.into();
        attdef.flags.invisible = true;
        attdef
    }

    /// Create a constant attribute definition
    pub fn constant(tag: impl Into<String>, value: impl Into<String>) -> Self {
        let mut attdef = Self::simple(tag);
        attdef.default_value = value.into();
        attdef.flags.constant = true;
        attdef
    }

    /// Set the text insertion point
    pub fn set_position(&mut self, point: Vector3) {
        self.insertion_point = point;
        self.alignment_point = point;
    }

    /// Set the text height
    pub fn set_height(&mut self, height: f64) {
        self.height = height;
    }

    /// Set the rotation angle in degrees
    pub fn set_rotation_degrees(&mut self, degrees: f64) {
        self.rotation = degrees * PI / 180.0;
    }

    /// Get the rotation angle in degrees
    pub fn rotation_degrees(&self) -> f64 {
        self.rotation * 180.0 / PI
    }

    /// Set the text style
    pub fn set_text_style(&mut self, style: impl Into<String>) {
        self.text_style = style.into();
    }

    /// Set the alignment
    pub fn set_alignment(&mut self, horizontal: HorizontalAlignment, vertical: VerticalAlignment) {
        self.horizontal_alignment = horizontal;
        self.vertical_alignment = vertical;
    }

    /// Center the text horizontally
    pub fn center_horizontal(&mut self) {
        self.horizontal_alignment = HorizontalAlignment::Center;
    }

    /// Center the text both horizontally and vertically
    pub fn center(&mut self) {
        self.horizontal_alignment = HorizontalAlignment::Center;
        self.vertical_alignment = VerticalAlignment::Middle;
    }

    /// Make the attribute invisible
    pub fn set_invisible(&mut self, invisible: bool) {
        self.flags.invisible = invisible;
    }

    /// Make the attribute constant (unchangeable)
    pub fn set_constant(&mut self, constant: bool) {
        self.flags.constant = constant;
    }

    /// Make the attribute preset (no prompt during insertion)
    pub fn set_preset(&mut self, preset: bool) {
        self.flags.preset = preset;
    }

    /// Require verification on input
    pub fn set_verify(&mut self, verify: bool) {
        self.flags.verify = verify;
    }

    /// Check if this is a visible attribute
    pub fn is_visible(&self) -> bool {
        !self.flags.invisible
    }

    /// Check if this is a constant attribute
    pub fn is_constant(&self) -> bool {
        self.flags.constant
    }

    /// Estimate the width of the text
    pub fn estimated_width(&self) -> f64 {
        // Approximate width based on character count and height
        let char_width = self.height * 0.6 * self.width_factor;
        self.default_value.len() as f64 * char_width
    }

    /// Builder: Set position
    pub fn with_position(mut self, point: Vector3) -> Self {
        self.set_position(point);
        self
    }

    /// Builder: Set height
    pub fn with_height(mut self, height: f64) -> Self {
        self.height = height;
        self
    }

    /// Builder: Set rotation in degrees
    pub fn with_rotation_degrees(mut self, degrees: f64) -> Self {
        self.set_rotation_degrees(degrees);
        self
    }

    /// Builder: Set text style
    pub fn with_text_style(mut self, style: impl Into<String>) -> Self {
        self.text_style = style.into();
        self
    }

    /// Builder: Set invisible
    pub fn with_invisible(mut self) -> Self {
        self.flags.invisible = true;
        self
    }

    /// Builder: Set constant
    pub fn with_constant(mut self) -> Self {
        self.flags.constant = true;
        self
    }

    /// Builder: Set preset
    pub fn with_preset(mut self) -> Self {
        self.flags.preset = true;
        self
    }

    /// Builder: Set layer
    pub fn with_layer(mut self, layer: impl Into<String>) -> Self {
        self.common.layer = layer.into();
        self
    }

    /// Builder: Set color
    pub fn with_color(mut self, color: Color) -> Self {
        self.common.color = color;
        self
    }
}

impl Default for AttributeDefinition {
    fn default() -> Self {
        Self::simple("ATTRIBUTE")
    }
}

impl Entity for AttributeDefinition {
    fn handle(&self) -> Handle {
        self.common.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.common.handle = handle;
    }

    fn layer(&self) -> &str {
        &self.common.layer
    }

    fn set_layer(&mut self, layer: String) {
        self.common.layer = layer;
    }

    fn color(&self) -> Color {
        self.common.color
    }

    fn set_color(&mut self, color: Color) {
        self.common.color = color;
    }

    fn line_weight(&self) -> LineWeight {
        self.common.line_weight
    }

    fn set_line_weight(&mut self, weight: LineWeight) {
        self.common.line_weight = weight;
    }

    fn transparency(&self) -> Transparency {
        self.common.transparency
    }

    fn set_transparency(&mut self, transparency: Transparency) {
        self.common.transparency = transparency;
    }

    fn is_invisible(&self) -> bool {
        self.common.invisible || self.flags.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }

    fn bounding_box(&self) -> BoundingBox3D {
        let width = self.estimated_width();
        let height = self.height;

        // Simple bounding box (doesn't account for rotation)
        BoundingBox3D::new(
            self.insertion_point,
            Vector3::new(
                self.insertion_point.x + width,
                self.insertion_point.y + height,
                self.insertion_point.z,
            ),
        )
        .ocs_to_wcs(self.normal)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_attribute_definition(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "ATTDEF"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_attribute_definition(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attdef_creation() {
        let attdef = AttributeDefinition::new(
            "TAG".to_string(),
            "Enter value:".to_string(),
            "default".to_string(),
        );
        assert_eq!(attdef.tag, "TAG");
        assert_eq!(attdef.prompt, "Enter value:");
        assert_eq!(attdef.default_value, "default");
    }

    #[test]
    fn test_attdef_simple() {
        let attdef = AttributeDefinition::simple("PART_NO");
        assert_eq!(attdef.tag, "PART_NO");
        assert_eq!(attdef.prompt, "Enter PART_NO:");
    }

    #[test]
    fn test_attdef_invisible() {
        let attdef = AttributeDefinition::invisible("HIDDEN", "secret");
        assert!(attdef.flags.invisible);
        assert_eq!(attdef.default_value, "secret");
    }

    #[test]
    fn test_attdef_constant() {
        let attdef = AttributeDefinition::constant("VERSION", "1.0");
        assert!(attdef.flags.constant);
        assert_eq!(attdef.default_value, "1.0");
    }

    #[test]
    fn test_attdef_rotation() {
        let mut attdef = AttributeDefinition::default();
        attdef.set_rotation_degrees(45.0);
        assert!((attdef.rotation_degrees() - 45.0).abs() < 1e-10);
    }

    #[test]
    fn test_attdef_flags() {
        let flags = AttributeFlags::from_bits(7); // invisible + constant + verify
        assert!(flags.invisible);
        assert!(flags.constant);
        assert!(flags.verify);
        assert!(!flags.preset);

        assert_eq!(flags.to_bits(), 7);
    }

    #[test]
    fn test_attdef_alignment() {
        let mut attdef = AttributeDefinition::default();
        attdef.set_alignment(HorizontalAlignment::Center, VerticalAlignment::Middle);
        assert_eq!(attdef.horizontal_alignment, HorizontalAlignment::Center);
        assert_eq!(attdef.vertical_alignment, VerticalAlignment::Middle);
    }

    #[test]
    fn test_attdef_translate() {
        let mut attdef = AttributeDefinition::default();
        attdef.insertion_point = Vector3::new(10.0, 20.0, 0.0);
        attdef.alignment_point = Vector3::new(10.0, 20.0, 0.0);

        attdef.translate(Vector3::new(5.0, 5.0, 0.0));

        assert_eq!(attdef.insertion_point, Vector3::new(15.0, 25.0, 0.0));
        assert_eq!(attdef.alignment_point, Vector3::new(15.0, 25.0, 0.0));
    }

    #[test]
    fn test_attdef_builder() {
        let attdef = AttributeDefinition::simple("TEST")
            .with_position(Vector3::new(10.0, 10.0, 0.0))
            .with_height(5.0)
            .with_rotation_degrees(90.0)
            .with_invisible()
            .with_layer("ATTRIBUTES");

        assert_eq!(attdef.insertion_point, Vector3::new(10.0, 10.0, 0.0));
        assert_eq!(attdef.height, 5.0);
        assert!(attdef.flags.invisible);
        assert_eq!(attdef.common.layer, "ATTRIBUTES");
    }

    #[test]
    fn test_horizontal_alignment() {
        assert_eq!(
            HorizontalAlignment::from_value(0),
            HorizontalAlignment::Left
        );
        assert_eq!(
            HorizontalAlignment::from_value(1),
            HorizontalAlignment::Center
        );
        assert_eq!(
            HorizontalAlignment::from_value(2),
            HorizontalAlignment::Right
        );
        assert_eq!(HorizontalAlignment::Center.to_value(), 1);
    }

    #[test]
    fn test_vertical_alignment() {
        assert_eq!(
            VerticalAlignment::from_value(0),
            VerticalAlignment::Baseline
        );
        assert_eq!(VerticalAlignment::from_value(2), VerticalAlignment::Middle);
        assert_eq!(VerticalAlignment::Middle.to_value(), 2);
    }
}
