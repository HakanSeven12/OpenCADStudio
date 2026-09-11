//! Ellipse entity

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// An ellipse entity
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ellipse {
    /// Common entity data
    pub common: EntityCommon,
    /// Center point of the ellipse
    pub center: Vector3,
    /// Major axis endpoint (relative to center)
    pub major_axis: Vector3,
    /// Ratio of minor axis to major axis
    pub minor_axis_ratio: f64,
    /// Start parameter (0 = start of ellipse)
    pub start_parameter: f64,
    /// End parameter (2π = full ellipse)
    pub end_parameter: f64,
    /// Normal vector
    pub normal: Vector3,
}

impl Ellipse {
    /// Create a new ellipse at the origin
    pub fn new() -> Self {
        Ellipse {
            common: EntityCommon::new(),
            center: Vector3::ZERO,
            major_axis: Vector3::UNIT_X,
            minor_axis_ratio: 0.5,
            start_parameter: 0.0,
            end_parameter: 2.0 * std::f64::consts::PI,
            normal: Vector3::UNIT_Z,
        }
    }

    /// Create a new ellipse with center, major axis, and ratio
    pub fn from_center_axes(center: Vector3, major_axis: Vector3, minor_axis_ratio: f64) -> Self {
        Ellipse {
            center,
            major_axis,
            minor_axis_ratio,
            ..Self::new()
        }
    }

    /// Get the major axis length
    pub fn major_axis_length(&self) -> f64 {
        self.major_axis.length()
    }

    /// Get the minor axis length
    pub fn minor_axis_length(&self) -> f64 {
        self.major_axis_length() * self.minor_axis_ratio
    }

    /// Check if this is a full ellipse
    pub fn is_full(&self) -> bool {
        (self.end_parameter - self.start_parameter - 2.0 * std::f64::consts::PI).abs() < 1e-10
    }
}

impl Default for Ellipse {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Ellipse {
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
        let major_len = self.major_axis_length();
        let minor_len = self.minor_axis_length();
        let max_radius = major_len.max(minor_len);

        BoundingBox3D::new(
            Vector3::new(
                self.center.x - max_radius,
                self.center.y - max_radius,
                self.center.z,
            ),
            Vector3::new(
                self.center.x + max_radius,
                self.center.y + max_radius,
                self.center.z,
            ),
        )
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_ellipse(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "ELLIPSE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_ellipse(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_ellipse(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ellipse_creation() {
        let ellipse = Ellipse::new();
        assert_eq!(ellipse.center, Vector3::ZERO);
        assert_eq!(ellipse.entity_type(), "ELLIPSE");
    }

    #[test]
    fn test_ellipse_axis_lengths() {
        let ellipse = Ellipse::from_center_axes(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0), 0.5);
        assert_eq!(ellipse.major_axis_length(), 10.0);
        assert_eq!(ellipse.minor_axis_length(), 5.0);
    }
}
