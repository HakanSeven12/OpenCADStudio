//! Point entity

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transform, Transparency, Vector3};

/// A point entity in 3D space
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Point {
    /// Common entity data
    pub common: EntityCommon,
    /// Location of the point
    pub location: Vector3,
    /// Thickness (extrusion in Z direction)
    pub thickness: f64,
    /// Normal vector
    pub normal: Vector3,
    /// UCS X-axis angle for point display (DXF 50, radians) — orients the
    /// point marker (PDMODE glyph) within the plane.
    pub x_axis_angle: f64,
}

impl Point {
    /// Create a new point at the origin
    pub fn new() -> Self {
        Point {
            common: EntityCommon::new(),
            location: Vector3::ZERO,
            thickness: 0.0,
            normal: Vector3::UNIT_Z,
            x_axis_angle: 0.0,
        }
    }

    /// Create a new point at a specific location
    pub fn at(location: Vector3) -> Self {
        Point {
            location,
            ..Self::new()
        }
    }

    /// Create a new point with coordinates
    pub fn from_coords(x: f64, y: f64, z: f64) -> Self {
        Point::at(Vector3::new(x, y, z))
    }
}

impl Default for Point {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Point {
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
        BoundingBox3D::from_point(self.location)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_point(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "POINT"
    }

    fn apply_transform(&mut self, transform: &Transform) {
        super::transform::transform_point(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_creation() {
        let point = Point::new();
        assert_eq!(point.location, Vector3::ZERO);
        assert_eq!(point.entity_type(), "POINT");
    }

    #[test]
    fn test_point_at_location() {
        let point = Point::at(Vector3::new(10.0, 20.0, 30.0));
        assert_eq!(point.location.x, 10.0);
        assert_eq!(point.location.y, 20.0);
        assert_eq!(point.location.z, 30.0);
    }

    #[test]
    fn test_point_from_coords() {
        let point = Point::from_coords(5.0, 10.0, 15.0);
        assert_eq!(point.location, Vector3::new(5.0, 10.0, 15.0));
    }

    #[test]
    fn test_point_translate() {
        let mut point = Point::at(Vector3::new(1.0, 2.0, 3.0));
        point.translate(Vector3::new(10.0, 20.0, 30.0));
        assert_eq!(point.location, Vector3::new(11.0, 22.0, 33.0));
    }

    #[test]
    fn test_point_bounding_box() {
        let point = Point::at(Vector3::new(5.0, 10.0, 15.0));
        let bbox = point.bounding_box();
        assert_eq!(bbox.min, Vector3::new(5.0, 10.0, 15.0));
        assert_eq!(bbox.max, Vector3::new(5.0, 10.0, 15.0));
    }
}
