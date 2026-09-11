//! Attribute entity - Block attribute instance with actual values

use crate::entities::attribute_definition::{
    AttributeFlags, HorizontalAlignment, MTextFlag, VerticalAlignment,
};
use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};
use std::f64::consts::PI;

/// Attribute entity - contains the actual value for a block attribute
///
/// Attribute entities are created when a block with attribute definitions
/// (ATTDEF) is inserted. Each ATTRIB corresponds to an ATTDEF in the block
/// and contains the user-entered or default value.
///
/// # DXF Entity Type
/// ATTRIB
///
/// # Example
/// ```ignore
/// use acadrust::entities::AttributeEntity;
/// use acadrust::types::Vector3;
///
/// let mut attrib = AttributeEntity::new(
///     "PART_NUMBER".to_string(),
///     "PN-12345".to_string(),
/// );
/// attrib.insertion_point = Vector3::new(10.0, 20.0, 0.0);
/// attrib.height = 2.5;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttributeEntity {
    /// Common entity properties
    pub common: EntityCommon,
    /// Attribute tag (must match ATTDEF tag)
    pub tag: String,
    /// Actual attribute value
    pub value: String,
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
    /// Field length
    pub field_length: i16,
    /// Extrusion direction
    pub normal: Vector3,
    /// Multiline text flag
    pub mtext_flag: MTextFlag,
    /// Is this really a multiline attribute
    pub is_multiline: bool,
    /// Line count for multiline
    pub line_count: i16,
    /// Embedded MTEXT payload used by R2018+ multiline attributes.
    ///
    /// The plain attribute text only carries the first line. Keeping the
    /// embedded object preserves its layout, style, background and full value.
    pub embedded_mtext: Option<Box<crate::entities::MText>>,
    /// Handle of the attribute definition this was created from
    pub attdef_handle: Handle,
    /// Lock position in block
    pub lock_position: bool,
}

impl AttributeEntity {
    /// Create a new attribute with tag and value
    pub fn new(tag: String, value: String) -> Self {
        Self {
            common: EntityCommon::default(),
            tag,
            value,
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
            attdef_handle: Handle::NULL,
            lock_position: false,
        }
    }

    /// Create a simple attribute with just tag and value
    pub fn simple(tag: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(tag.into(), value.into())
    }

    /// Create an attribute from an attribute definition
    pub fn from_definition(
        attdef: &crate::entities::AttributeDefinition,
        value: Option<String>,
    ) -> Self {
        Self {
            common: EntityCommon {
                layer: attdef.common.layer.clone(),
                color: attdef.common.color,
                color_name: attdef.common.color_name.clone(),
                color_book_handle: attdef.common.color_book_handle,
                ..Default::default()
            },
            tag: attdef.tag.clone(),
            value: value.unwrap_or_else(|| attdef.default_value.clone()),
            insertion_point: attdef.insertion_point,
            alignment_point: attdef.alignment_point,
            height: attdef.height,
            rotation: attdef.rotation,
            width_factor: attdef.width_factor,
            oblique_angle: attdef.oblique_angle,
            text_style: attdef.text_style.clone(),
            text_generation_flags: attdef.text_generation_flags,
            horizontal_alignment: attdef.horizontal_alignment,
            vertical_alignment: attdef.vertical_alignment,
            flags: attdef.flags,
            field_length: attdef.field_length,
            normal: attdef.normal,
            mtext_flag: attdef.mtext_flag,
            is_multiline: attdef.is_multiline,
            line_count: attdef.line_count,
            embedded_mtext: attdef.embedded_mtext.clone(),
            attdef_handle: attdef.common.handle,
            lock_position: attdef.lock_position,
        }
    }

    /// Set the attribute value
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
    }

    /// Get the attribute value
    pub fn get_value(&self) -> &str {
        &self.value
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
        let char_width = self.height * 0.6 * self.width_factor;
        self.value.len() as f64 * char_width
    }

    /// Apply a transformation from an INSERT entity
    pub fn apply_insert_transform(&mut self, insert_point: Vector3, scale: Vector3, rotation: f64) {
        // Scale the position relative to origin
        let scaled = Vector3::new(
            self.insertion_point.x * scale.x,
            self.insertion_point.y * scale.y,
            self.insertion_point.z * scale.z,
        );

        // Rotate around origin
        let cos_r = rotation.cos();
        let sin_r = rotation.sin();
        let rotated = Vector3::new(
            scaled.x * cos_r - scaled.y * sin_r,
            scaled.x * sin_r + scaled.y * cos_r,
            scaled.z,
        );

        // Translate to insert point
        self.insertion_point = rotated + insert_point;

        // Scale the alignment point similarly
        let scaled_align = Vector3::new(
            self.alignment_point.x * scale.x,
            self.alignment_point.y * scale.y,
            self.alignment_point.z * scale.z,
        );
        let rotated_align = Vector3::new(
            scaled_align.x * cos_r - scaled_align.y * sin_r,
            scaled_align.x * sin_r + scaled_align.y * cos_r,
            scaled_align.z,
        );
        self.alignment_point = rotated_align + insert_point;

        // Add insert rotation to text rotation
        self.rotation += rotation;

        // Scale the height (using average of X and Y scale for uniform text)
        let text_scale = (scale.x.abs() + scale.y.abs()) / 2.0;
        self.height *= text_scale;
    }

    /// Builder: Set value
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
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

impl Default for AttributeEntity {
    fn default() -> Self {
        Self::new("ATTRIBUTE".to_string(), String::new())
    }
}

impl Entity for AttributeEntity {
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
        super::translate::translate_attribute_entity(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "ATTRIB"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_attribute_entity(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::AttributeDefinition;

    #[test]
    fn test_attrib_creation() {
        let attrib = AttributeEntity::new("TAG".to_string(), "value".to_string());
        assert_eq!(attrib.tag, "TAG");
        assert_eq!(attrib.value, "value");
    }

    #[test]
    fn test_attrib_simple() {
        let attrib = AttributeEntity::simple("PART_NO", "12345");
        assert_eq!(attrib.tag, "PART_NO");
        assert_eq!(attrib.value, "12345");
    }

    #[test]
    fn test_attrib_from_definition() {
        let attdef = AttributeDefinition::new(
            "TAG".to_string(),
            "Enter:".to_string(),
            "default".to_string(),
        );

        // With custom value
        let attrib = AttributeEntity::from_definition(&attdef, Some("custom".to_string()));
        assert_eq!(attrib.tag, "TAG");
        assert_eq!(attrib.value, "custom");

        // With default value
        let attrib2 = AttributeEntity::from_definition(&attdef, None);
        assert_eq!(attrib2.value, "default");
    }

    #[test]
    fn test_attrib_set_value() {
        let mut attrib = AttributeEntity::simple("TAG", "old");
        attrib.set_value("new");
        assert_eq!(attrib.get_value(), "new");
    }

    #[test]
    fn test_attrib_rotation() {
        let mut attrib = AttributeEntity::default();
        attrib.set_rotation_degrees(90.0);
        assert!((attrib.rotation_degrees() - 90.0).abs() < 1e-10);
    }

    #[test]
    fn test_attrib_translate() {
        let mut attrib = AttributeEntity::default();
        attrib.insertion_point = Vector3::new(10.0, 20.0, 0.0);
        attrib.alignment_point = Vector3::new(10.0, 20.0, 0.0);

        attrib.translate(Vector3::new(5.0, 5.0, 0.0));

        assert_eq!(attrib.insertion_point, Vector3::new(15.0, 25.0, 0.0));
        assert_eq!(attrib.alignment_point, Vector3::new(15.0, 25.0, 0.0));
    }

    #[test]
    fn test_attrib_insert_transform() {
        let mut attrib = AttributeEntity::simple("TAG", "value");
        attrib.insertion_point = Vector3::new(10.0, 0.0, 0.0);
        attrib.alignment_point = Vector3::new(10.0, 0.0, 0.0);
        attrib.height = 2.5;

        // Apply scale of 2x and translate to (100, 100, 0)
        attrib.apply_insert_transform(
            Vector3::new(100.0, 100.0, 0.0),
            Vector3::new(2.0, 2.0, 1.0),
            0.0,
        );

        // Position should be (10 * 2 + 100, 0 * 2 + 100, 0) = (120, 100, 0)
        assert!((attrib.insertion_point.x - 120.0).abs() < 1e-10);
        assert!((attrib.insertion_point.y - 100.0).abs() < 1e-10);

        // Height should be scaled by average of X and Y scale = 2
        assert!((attrib.height - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_attrib_insert_transform_with_rotation() {
        let mut attrib = AttributeEntity::simple("TAG", "value");
        attrib.insertion_point = Vector3::new(10.0, 0.0, 0.0);
        attrib.alignment_point = Vector3::new(10.0, 0.0, 0.0);

        // Apply 90 degree rotation
        let rotation = PI / 2.0;
        attrib.apply_insert_transform(Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0), rotation);

        // After 90 degree rotation, (10, 0) -> (0, 10)
        assert!(attrib.insertion_point.x.abs() < 1e-10);
        assert!((attrib.insertion_point.y - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_attrib_visibility() {
        let mut attrib = AttributeEntity::default();
        assert!(attrib.is_visible());

        attrib.flags.invisible = true;
        assert!(!attrib.is_visible());
    }

    #[test]
    fn test_attrib_builder() {
        let attrib = AttributeEntity::simple("TAG", "")
            .with_value("test_value")
            .with_position(Vector3::new(10.0, 10.0, 0.0))
            .with_height(5.0)
            .with_rotation_degrees(45.0)
            .with_layer("ATTRIBUTES");

        assert_eq!(attrib.value, "test_value");
        assert_eq!(attrib.insertion_point, Vector3::new(10.0, 10.0, 0.0));
        assert_eq!(attrib.height, 5.0);
        assert_eq!(attrib.common.layer, "ATTRIBUTES");
    }

    #[test]
    fn test_attrib_estimated_width() {
        let attrib = AttributeEntity::simple("TAG", "Hello");
        let width = attrib.estimated_width();
        // 5 chars * 2.5 height * 0.6 factor * 1.0 width_factor = 7.5
        assert!((width - 7.5).abs() < 1e-10);
    }
}
