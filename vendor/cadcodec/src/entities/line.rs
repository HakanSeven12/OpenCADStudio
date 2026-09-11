//! Line entity

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transform, Transparency, Vector3};

/// A line entity defined by two endpoints
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Line {
    /// Common entity data
    pub common: EntityCommon,
    /// Start point of the line
    pub start: Vector3,
    /// End point of the line
    pub end: Vector3,
    /// Thickness (extrusion in Z direction)
    pub thickness: f64,
    /// Normal vector
    pub normal: Vector3,
}

impl Line {
    /// Create a new line from origin to origin
    pub fn new() -> Self {
        Line {
            common: EntityCommon::new(),
            start: Vector3::ZERO,
            end: Vector3::ZERO,
            thickness: 0.0,
            normal: Vector3::UNIT_Z,
        }
    }

    /// Create a new line between two points
    pub fn from_points(start: Vector3, end: Vector3) -> Self {
        Line {
            start,
            end,
            ..Self::new()
        }
    }

    /// Create a new line from coordinates
    pub fn from_coords(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Self {
        Line::from_points(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2))
    }

    /// Get the length of the line
    pub fn length(&self) -> f64 {
        self.start.distance(&self.end)
    }

    /// Get the direction vector (normalized)
    pub fn direction(&self) -> Vector3 {
        (self.end - self.start).normalize()
    }

    /// Get the midpoint of the line
    pub fn midpoint(&self) -> Vector3 {
        Vector3::new(
            (self.start.x + self.end.x) / 2.0,
            (self.start.y + self.end.y) / 2.0,
            (self.start.z + self.end.z) / 2.0,
        )
    }
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Line {
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
        BoundingBox3D::from_points(&[self.start, self.end]).unwrap()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_line(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "LINE"
    }

    fn apply_transform(&mut self, transform: &Transform) {
        super::transform::transform_line(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_creation() {
        let line = Line::new();
        assert_eq!(line.start, Vector3::ZERO);
        assert_eq!(line.end, Vector3::ZERO);
        assert_eq!(line.entity_type(), "LINE");
    }

    #[test]
    fn test_line_from_points() {
        let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(line.length(), 10.0);
    }

    #[test]
    fn test_line_length() {
        let line = Line::from_coords(0.0, 0.0, 0.0, 3.0, 4.0, 0.0);
        assert_eq!(line.length(), 5.0);
    }

    #[test]
    fn test_line_midpoint() {
        let line = Line::from_coords(0.0, 0.0, 0.0, 10.0, 20.0, 30.0);
        assert_eq!(line.midpoint(), Vector3::new(5.0, 10.0, 15.0));
    }

    #[test]
    fn test_line_translate() {
        let mut line = Line::from_coords(0.0, 0.0, 0.0, 10.0, 0.0, 0.0);
        line.translate(Vector3::new(5.0, 5.0, 5.0));
        assert_eq!(line.start, Vector3::new(5.0, 5.0, 5.0));
        assert_eq!(line.end, Vector3::new(15.0, 5.0, 5.0));
    }

    #[test]
    fn test_line_mirror_x() {
        let mut line = Line::from_coords(1.0, 2.0, 0.0, 4.0, 6.0, 0.0);
        line.mirror_x();
        assert!((line.start.x - (-1.0)).abs() < 1e-10);
        assert!((line.start.y - 2.0).abs() < 1e-10);
        assert!((line.end.x - (-4.0)).abs() < 1e-10);
        assert!((line.end.y - 6.0).abs() < 1e-10);
    }

    #[test]
    fn test_line_mirror_about_line() {
        // Mirror a horizontal line across x-axis — should negate Y
        let mut line = Line::from_coords(0.0, 5.0, 0.0, 10.0, 5.0, 0.0);
        line.mirror_about_line(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
        assert!((line.start.y - (-5.0)).abs() < 1e-10);
        assert!((line.end.y - (-5.0)).abs() < 1e-10);
    }
}
