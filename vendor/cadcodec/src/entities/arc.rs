//! Arc entity

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Matrix3, Transparency, Vector3};

/// An arc entity (portion of a circle)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Arc {
    /// Common entity data
    pub common: EntityCommon,
    /// Center point of the arc
    pub center: Vector3,
    /// Radius of the arc
    pub radius: f64,
    /// Start angle in radians
    pub start_angle: f64,
    /// End angle in radians
    pub end_angle: f64,
    /// Thickness (extrusion in Z direction)
    pub thickness: f64,
    /// Normal vector
    pub normal: Vector3,
}

impl Arc {
    /// Create a new arc at the origin
    pub fn new() -> Self {
        Arc {
            common: EntityCommon::new(),
            center: Vector3::ZERO,
            radius: 1.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI / 2.0, // 90 degrees
            thickness: 0.0,
            normal: Vector3::UNIT_Z,
        }
    }

    /// Create a new arc with center, radius, and angles
    pub fn from_center_radius_angles(
        center: Vector3,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> Self {
        Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..Self::new()
        }
    }

    /// Create a new arc from coordinates, radius, and angles
    pub fn from_coords(
        x: f64,
        y: f64,
        z: f64,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> Self {
        Arc::from_center_radius_angles(Vector3::new(x, y, z), radius, start_angle, end_angle)
    }

    /// Get the sweep angle (angular extent) in radians
    pub fn sweep_angle(&self) -> f64 {
        let mut sweep = self.end_angle - self.start_angle;
        if sweep < 0.0 {
            sweep += 2.0 * std::f64::consts::PI;
        }
        sweep
    }

    /// Get the arc length
    pub fn arc_length(&self) -> f64 {
        self.radius * self.sweep_angle()
    }

    /// Get the start point of the arc
    pub fn start_point(&self) -> Vector3 {
        Vector3::new(
            self.center.x + self.radius * self.start_angle.cos(),
            self.center.y + self.radius * self.start_angle.sin(),
            self.center.z,
        )
    }

    /// Get the end point of the arc
    pub fn end_point(&self) -> Vector3 {
        Vector3::new(
            self.center.x + self.radius * self.end_angle.cos(),
            self.center.y + self.radius * self.end_angle.sin(),
            self.center.z,
        )
    }

    /// Get the midpoint of the arc
    pub fn midpoint(&self) -> Vector3 {
        let mid_angle = self.start_angle + self.sweep_angle() / 2.0;
        Vector3::new(
            self.center.x + self.radius * mid_angle.cos(),
            self.center.y + self.radius * mid_angle.sin(),
            self.center.z,
        )
    }

    /// Centre converted from the entity coordinate system to world space.
    pub fn center_wcs(&self) -> Vector3 {
        Matrix3::arbitrary_axis(self.normal) * self.center
    }

    /// Point at an arc angle converted to world space.
    pub fn point_at_angle_wcs(&self, angle: f64) -> Vector3 {
        let local =
            self.center + Vector3::new(self.radius * angle.cos(), self.radius * angle.sin(), 0.0);
        Matrix3::arbitrary_axis(self.normal) * local
    }

    /// Start point in world space.
    pub fn start_point_wcs(&self) -> Vector3 {
        self.point_at_angle_wcs(self.start_angle)
    }

    /// End point in world space.
    pub fn end_point_wcs(&self) -> Vector3 {
        self.point_at_angle_wcs(self.end_angle)
    }

    /// Arc-length midpoint in world space.
    pub fn midpoint_wcs(&self) -> Vector3 {
        self.point_at_angle_wcs(self.start_angle + self.sweep_angle() * 0.5)
    }

    fn contains_angle(&self, angle: f64) -> bool {
        let sweep = self.sweep_angle();
        sweep >= std::f64::consts::TAU - 1.0e-12
            || (angle - self.start_angle).rem_euclid(std::f64::consts::TAU) <= sweep + 1.0e-12
    }
}

impl Default for Arc {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Arc {
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
        let basis = Matrix3::arbitrary_axis(self.normal);
        let ax = basis * Vector3::new(1.0, 0.0, 0.0);
        let ay = basis * Vector3::new(0.0, 1.0, 0.0);
        let mut angles = vec![self.start_angle, self.end_angle];
        for angle in [ay.x.atan2(ax.x), ay.y.atan2(ax.y), ay.z.atan2(ax.z)] {
            for candidate in [angle, angle + std::f64::consts::PI] {
                if self.contains_angle(candidate) {
                    angles.push(candidate);
                }
            }
        }

        let normal = self.normal.normalize();
        let mut points = Vec::with_capacity(angles.len() * 2);
        for angle in angles {
            let base = self.point_at_angle_wcs(angle);
            points.push(base);
            if self.thickness != 0.0 {
                points.push(base + normal * self.thickness);
            }
        }
        BoundingBox3D::from_points(&points).unwrap_or_default()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_arc(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "ARC"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_arc(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_arc(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arc_creation() {
        let arc = Arc::new();
        assert_eq!(arc.center, Vector3::ZERO);
        assert_eq!(arc.radius, 1.0);
        assert_eq!(arc.entity_type(), "ARC");
    }

    #[test]
    fn test_arc_sweep_angle() {
        let arc = Arc::from_coords(0.0, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI);
        assert!((arc.sweep_angle() - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn test_arc_length() {
        let arc = Arc::from_coords(0.0, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI);
        let expected = 5.0 * std::f64::consts::PI;
        assert!((arc.arc_length() - expected).abs() < 1e-10);
    }

    #[test]
    fn test_arc_endpoints() {
        let arc = Arc::from_coords(0.0, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI / 2.0);
        let start = arc.start_point();
        let end = arc.end_point();
        assert!((start.x - 5.0).abs() < 1e-10);
        assert!((start.y - 0.0).abs() < 1e-10);
        assert!((end.x - 0.0).abs() < 1e-10);
        assert!((end.y - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_arc_translate() {
        let mut arc = Arc::from_coords(0.0, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI);
        arc.translate(Vector3::new(10.0, 20.0, 30.0));
        assert_eq!(arc.center, Vector3::new(10.0, 20.0, 30.0));
        assert_eq!(arc.radius, 5.0);
    }

    #[test]
    fn test_arc_mirror_x() {
        use std::f64::consts::PI;
        // Arc from 0° to 90° at origin, radius 5
        let mut arc = Arc::from_coords(0.0, 0.0, 0.0, 5.0, 0.0, PI / 2.0);

        // Save original endpoints
        let orig_start = arc.start_point();
        let orig_end = arc.end_point();

        arc.mirror_x();

        // Center should be mirrored (x negated)
        assert!((arc.center.x - 0.0).abs() < 1e-10);

        // New endpoints should match mirrored original endpoints (swapped)
        let new_start = arc.start_point();
        let new_end = arc.end_point();
        // Mirrored original end → new start
        assert!((new_start.x - (-orig_end.x)).abs() < 1e-8);
        assert!((new_start.y - orig_end.y).abs() < 1e-8);
        // Mirrored original start → new end
        assert!((new_end.x - (-orig_start.x)).abs() < 1e-8);
        assert!((new_end.y - orig_start.y).abs() < 1e-8);
    }

    #[test]
    fn test_arc_mirror_y() {
        use std::f64::consts::PI;
        let mut arc = Arc::from_coords(0.0, 0.0, 0.0, 5.0, 0.0, PI / 2.0);
        let orig_start = arc.start_point();
        let orig_end = arc.end_point();

        arc.mirror_y();

        let new_start = arc.start_point();
        let new_end = arc.end_point();
        // Mirrored original end → new start
        assert!((new_start.x - orig_end.x).abs() < 1e-8);
        assert!((new_start.y - (-orig_end.y)).abs() < 1e-8);
        // Mirrored original start → new end
        assert!((new_end.x - orig_start.x).abs() < 1e-8);
        assert!((new_end.y - (-orig_start.y)).abs() < 1e-8);
    }
}
