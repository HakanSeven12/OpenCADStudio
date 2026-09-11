//! Text entity

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Text horizontal alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextHorizontalAlignment {
    /// Left aligned
    Left,
    /// Center aligned
    Center,
    /// Right aligned
    Right,
    /// Aligned (fit between two points)
    Aligned,
    /// Middle (centered horizontally and vertically)
    Middle,
    /// Fit (fit between two points, adjust height)
    Fit,
}

/// Text vertical alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextVerticalAlignment {
    /// Baseline
    Baseline,
    /// Bottom
    Bottom,
    /// Middle
    Middle,
    /// Top
    Top,
}

/// A single-line text entity
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Text {
    /// Common entity data
    pub common: EntityCommon,
    /// Text content
    pub value: String,
    /// Insertion point (first alignment point)
    pub insertion_point: Vector3,
    /// Second alignment point (for aligned/fit text)
    pub alignment_point: Option<Vector3>,
    /// Text height
    pub height: f64,
    /// Rotation angle in radians
    pub rotation: f64,
    /// Width scale factor
    pub width_factor: f64,
    /// Oblique angle in radians
    pub oblique_angle: f64,
    /// Text style name
    pub style: String,
    /// Horizontal alignment
    pub horizontal_alignment: TextHorizontalAlignment,
    /// Vertical alignment
    pub vertical_alignment: TextVerticalAlignment,
    /// Normal vector
    pub normal: Vector3,
    /// Thickness / extrusion depth (DXF 39).
    pub thickness: f64,
    /// Text generation flags (DXF 71): bit 2 = backward (mirrored in X),
    /// bit 4 = upside-down (mirrored in Y).
    pub generation_flags: i16,
}

impl Text {
    /// Create a new text entity
    pub fn new() -> Self {
        Text {
            common: EntityCommon::new(),
            value: String::new(),
            insertion_point: Vector3::ZERO,
            alignment_point: None,
            height: 1.0,
            rotation: 0.0,
            width_factor: 1.0,
            oblique_angle: 0.0,
            style: "Standard".to_string(),
            horizontal_alignment: TextHorizontalAlignment::Left,
            vertical_alignment: TextVerticalAlignment::Baseline,
            normal: Vector3::UNIT_Z,
            thickness: 0.0,
            generation_flags: 0,
        }
    }

    /// Create a new text with value and position
    pub fn with_value(value: impl Into<String>, position: Vector3) -> Self {
        Text {
            value: value.into(),
            insertion_point: position,
            ..Self::new()
        }
    }

    /// Set the text height
    pub fn with_height(mut self, height: f64) -> Self {
        self.height = height;
        self
    }

    /// Set the rotation angle
    pub fn with_rotation(mut self, rotation: f64) -> Self {
        self.rotation = rotation;
        self
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Text {
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
        self.common.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }

    fn bounding_box(&self) -> BoundingBox3D {
        // Simplified bounding box based on insertion point and height
        let width = self.value.len() as f64 * self.height * 0.6 * self.width_factor;
        BoundingBox3D::new(
            self.insertion_point,
            Vector3::new(
                self.insertion_point.x + width,
                self.insertion_point.y + self.height,
                self.insertion_point.z,
            ),
        )
        .ocs_to_wcs(self.normal)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_text(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "TEXT"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_text(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_text(self, transform);
    }
}
