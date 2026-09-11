//! Ray entity - semi-infinite line starting from a point

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Ray entity - a semi-infinite line extending from a base point in a direction
///
/// A ray has a starting point (base_point) and extends infinitely in
/// the direction specified by the direction vector.
///
/// # DXF Group Codes
/// - 10, 20, 30: Base point (start point)
/// - 11, 21, 31: Direction vector (unit vector)
///
/// # Example
/// ```ignore
/// use acadrust::entities::Ray;
/// use acadrust::types::Vector3;
///
/// let ray = Ray::new(
///     Vector3::new(0.0, 0.0, 0.0),
///     Vector3::new(1.0, 0.0, 0.0),
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ray {
    /// Common entity properties
    pub common: EntityCommon,
    /// Base point (starting point of the ray)
    pub base_point: Vector3,
    /// Direction vector (should be a unit vector)
    pub direction: Vector3,
}

impl Ray {
    /// Create a new ray from a base point and direction
    ///
    /// The direction vector will be normalized automatically.
    pub fn new(base_point: Vector3, direction: Vector3) -> Self {
        Self {
            common: EntityCommon::default(),
            base_point,
            direction: direction.normalize(),
        }
    }

    /// Create a ray from a start point through another point
    pub fn from_points(start: Vector3, through: Vector3) -> Self {
        let direction = (through - start).normalize();
        Self::new(start, direction)
    }

    /// Create a ray along the X axis from a point
    pub fn along_x(base_point: Vector3) -> Self {
        Self::new(base_point, Vector3::UNIT_X)
    }

    /// Create a ray along the Y axis from a point
    pub fn along_y(base_point: Vector3) -> Self {
        Self::new(base_point, Vector3::UNIT_Y)
    }

    /// Create a ray along the Z axis from a point
    pub fn along_z(base_point: Vector3) -> Self {
        Self::new(base_point, Vector3::UNIT_Z)
    }

    /// Get a point on the ray at parameter t (t >= 0)
    ///
    /// Returns base_point + t * direction
    pub fn point_at(&self, t: f64) -> Vector3 {
        self.base_point + self.direction * t.max(0.0)
    }

    /// Calculate the closest point on the ray to a given point
    pub fn closest_point(&self, point: Vector3) -> Vector3 {
        let v = point - self.base_point;
        let t = v.dot(&self.direction).max(0.0);
        self.point_at(t)
    }

    /// Calculate the distance from a point to this ray
    pub fn distance_to_point(&self, point: Vector3) -> f64 {
        let closest = self.closest_point(point);
        closest.distance(&point)
    }

    /// Check if a point lies on the ray (within tolerance)
    pub fn contains_point(&self, point: Vector3, tolerance: f64) -> bool {
        self.distance_to_point(point) <= tolerance
    }

    /// Get the angle of the ray in the XY plane (in radians)
    pub fn angle_xy(&self) -> f64 {
        self.direction.y.atan2(self.direction.x)
    }

    /// Get the angle of the ray from the XY plane (in radians)
    pub fn angle_from_xy(&self) -> f64 {
        let xy_length = (self.direction.x.powi(2) + self.direction.y.powi(2)).sqrt();
        self.direction.z.atan2(xy_length)
    }

    /// Check if this ray is parallel to another ray
    pub fn is_parallel_to(&self, other: &Ray, tolerance: f64) -> bool {
        let cross = self.direction.cross(&other.direction);
        cross.length() <= tolerance
    }

    /// Check if this ray is perpendicular to another ray
    pub fn is_perpendicular_to(&self, other: &Ray, tolerance: f64) -> bool {
        self.direction.dot(&other.direction).abs() <= tolerance
    }

    /// Set the direction vector (will be normalized)
    pub fn set_direction(&mut self, direction: Vector3) {
        self.direction = direction.normalize();
    }

    /// Builder: Set the layer
    pub fn with_layer(mut self, layer: impl Into<String>) -> Self {
        self.common.layer = layer.into();
        self
    }

    /// Builder: Set the color
    pub fn with_color(mut self, color: Color) -> Self {
        self.common.color = color;
        self
    }
}

impl Default for Ray {
    fn default() -> Self {
        Self::new(Vector3::ZERO, Vector3::UNIT_X)
    }
}

impl Entity for Ray {
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
        // Rays are semi-infinite, so we return a large bounding box
        // extending from the base point in the direction
        let far_point = self.point_at(1e10);
        BoundingBox3D::from_points(&[self.base_point, far_point]).unwrap_or_default()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_ray(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "RAY"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_ray(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ray_creation() {
        let ray = Ray::new(Vector3::new(1.0, 2.0, 3.0), Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(ray.base_point, Vector3::new(1.0, 2.0, 3.0));
        assert!((ray.direction.length() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_ray_from_points() {
        let ray = Ray::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(ray.base_point, Vector3::ZERO);
        assert!((ray.direction.x - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_ray_point_at() {
        let ray = Ray::along_x(Vector3::ZERO);
        let point = ray.point_at(5.0);
        assert_eq!(point, Vector3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn test_ray_point_at_negative() {
        let ray = Ray::along_x(Vector3::ZERO);
        // Negative t should clamp to 0 (ray only extends in positive direction)
        let point = ray.point_at(-5.0);
        assert_eq!(point, Vector3::ZERO);
    }

    #[test]
    fn test_ray_closest_point() {
        let ray = Ray::along_x(Vector3::ZERO);

        // Point above the ray
        let closest = ray.closest_point(Vector3::new(5.0, 3.0, 0.0));
        assert_eq!(closest, Vector3::new(5.0, 0.0, 0.0));

        // Point behind the base point
        let closest = ray.closest_point(Vector3::new(-5.0, 0.0, 0.0));
        assert_eq!(closest, Vector3::ZERO);
    }

    #[test]
    fn test_ray_distance_to_point() {
        let ray = Ray::along_x(Vector3::ZERO);
        let distance = ray.distance_to_point(Vector3::new(5.0, 3.0, 4.0));
        assert!((distance - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_ray_parallel() {
        let ray1 = Ray::along_x(Vector3::ZERO);
        let ray2 = Ray::along_x(Vector3::new(0.0, 5.0, 0.0));
        assert!(ray1.is_parallel_to(&ray2, 1e-10));
    }

    #[test]
    fn test_ray_perpendicular() {
        let ray1 = Ray::along_x(Vector3::ZERO);
        let ray2 = Ray::along_y(Vector3::ZERO);
        assert!(ray1.is_perpendicular_to(&ray2, 1e-10));
    }

    #[test]
    fn test_ray_translate() {
        let mut ray = Ray::along_x(Vector3::ZERO);
        ray.translate(Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(ray.base_point, Vector3::new(1.0, 2.0, 3.0));
        // Direction should not change
        assert_eq!(ray.direction, Vector3::UNIT_X);
    }
}
