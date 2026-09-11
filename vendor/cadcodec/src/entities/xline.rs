//! XLine (construction line) entity - infinite line in both directions

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// XLine entity - an infinite construction line extending in both directions
///
/// An xline passes through a base point and extends infinitely in both
/// the positive and negative direction of the specified direction vector.
/// Also known as a construction line.
///
/// # DXF Group Codes
/// - 10, 20, 30: Base point (first point)
/// - 11, 21, 31: Direction vector (unit vector)
///
/// # Example
/// ```ignore
/// use acadrust::entities::XLine;
/// use acadrust::types::Vector3;
///
/// let xline = XLine::new(
///     Vector3::new(0.0, 0.0, 0.0),
///     Vector3::new(1.0, 1.0, 0.0),
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XLine {
    /// Common entity properties
    pub common: EntityCommon,
    /// Base point (a point on the line)
    pub base_point: Vector3,
    /// Direction vector (should be a unit vector)
    pub direction: Vector3,
}

impl XLine {
    /// Create a new xline from a base point and direction
    ///
    /// The direction vector will be normalized automatically.
    pub fn new(base_point: Vector3, direction: Vector3) -> Self {
        Self {
            common: EntityCommon::default(),
            base_point,
            direction: direction.normalize(),
        }
    }

    /// Create an xline from two points
    pub fn from_points(point1: Vector3, point2: Vector3) -> Self {
        let direction = (point2 - point1).normalize();
        Self::new(point1, direction)
    }

    /// Create a horizontal xline through a point
    pub fn horizontal(through: Vector3) -> Self {
        Self::new(through, Vector3::UNIT_X)
    }

    /// Create a vertical xline through a point
    pub fn vertical(through: Vector3) -> Self {
        Self::new(through, Vector3::UNIT_Y)
    }

    /// Create an xline along the Z axis through a point
    pub fn along_z(through: Vector3) -> Self {
        Self::new(through, Vector3::UNIT_Z)
    }

    /// Create an xline at an angle in the XY plane
    pub fn at_angle(through: Vector3, angle_radians: f64) -> Self {
        let direction = Vector3::new(angle_radians.cos(), angle_radians.sin(), 0.0);
        Self::new(through, direction)
    }

    /// Get a point on the xline at parameter t
    ///
    /// Returns base_point + t * direction (t can be negative or positive)
    pub fn point_at(&self, t: f64) -> Vector3 {
        self.base_point + self.direction * t
    }

    /// Calculate the closest point on the xline to a given point
    pub fn closest_point(&self, point: Vector3) -> Vector3 {
        let v = point - self.base_point;
        let t = v.dot(&self.direction);
        self.point_at(t)
    }

    /// Calculate the distance from a point to this xline
    pub fn distance_to_point(&self, point: Vector3) -> f64 {
        let closest = self.closest_point(point);
        closest.distance(&point)
    }

    /// Check if a point lies on the xline (within tolerance)
    pub fn contains_point(&self, point: Vector3, tolerance: f64) -> bool {
        self.distance_to_point(point) <= tolerance
    }

    /// Get the angle of the xline in the XY plane (in radians)
    pub fn angle_xy(&self) -> f64 {
        self.direction.y.atan2(self.direction.x)
    }

    /// Get the angle of the xline from the XY plane (in radians)
    pub fn angle_from_xy(&self) -> f64 {
        let xy_length = (self.direction.x.powi(2) + self.direction.y.powi(2)).sqrt();
        self.direction.z.atan2(xy_length)
    }

    /// Check if this xline is parallel to another xline
    pub fn is_parallel_to(&self, other: &XLine, tolerance: f64) -> bool {
        let cross = self.direction.cross(&other.direction);
        cross.length() <= tolerance
    }

    /// Check if this xline is perpendicular to another xline
    pub fn is_perpendicular_to(&self, other: &XLine, tolerance: f64) -> bool {
        self.direction.dot(&other.direction).abs() <= tolerance
    }

    /// Find the intersection point with another xline (if they intersect)
    ///
    /// Returns None if the lines are parallel or skew (don't intersect in 3D)
    pub fn intersection(&self, other: &XLine, tolerance: f64) -> Option<Vector3> {
        // Check if parallel
        let cross = self.direction.cross(&other.direction);
        if cross.length() <= tolerance {
            return None;
        }

        // Solve for intersection using parametric equations
        // P1 + t1*D1 = P2 + t2*D2
        let w = self.base_point - other.base_point;
        let a = self.direction.dot(&self.direction);
        let b = self.direction.dot(&other.direction);
        let c = other.direction.dot(&other.direction);
        let d = self.direction.dot(&w);
        let e = other.direction.dot(&w);

        let denom = a * c - b * b;
        if denom.abs() <= tolerance {
            return None;
        }

        let t1 = (b * e - c * d) / denom;
        let t2 = (a * e - b * d) / denom;

        let p1 = self.point_at(t1);
        let p2 = other.point_at(t2);

        // Check if the points are close enough (lines might be skew in 3D)
        if p1.distance(&p2) <= tolerance {
            Some(p1)
        } else {
            None
        }
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

impl Default for XLine {
    fn default() -> Self {
        Self::new(Vector3::ZERO, Vector3::UNIT_X)
    }
}

impl Entity for XLine {
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
        // XLines are infinite, so we return a very large bounding box
        let far_positive = self.point_at(1e10);
        let far_negative = self.point_at(-1e10);
        BoundingBox3D::from_points(&[far_negative, far_positive]).unwrap_or_default()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_xline(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "XLINE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_xline(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xline_creation() {
        let xline = XLine::new(Vector3::new(1.0, 2.0, 3.0), Vector3::new(2.0, 0.0, 0.0));
        assert_eq!(xline.base_point, Vector3::new(1.0, 2.0, 3.0));
        assert!((xline.direction.length() - 1.0).abs() < 1e-10);
        assert!((xline.direction.x - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_xline_from_points() {
        let xline = XLine::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(xline.base_point, Vector3::ZERO);
        assert!((xline.direction.x - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_xline_point_at() {
        let xline = XLine::horizontal(Vector3::ZERO);

        let point_pos = xline.point_at(5.0);
        assert_eq!(point_pos, Vector3::new(5.0, 0.0, 0.0));

        let point_neg = xline.point_at(-5.0);
        assert_eq!(point_neg, Vector3::new(-5.0, 0.0, 0.0));
    }

    #[test]
    fn test_xline_closest_point() {
        let xline = XLine::horizontal(Vector3::ZERO);

        // Point above the line
        let closest = xline.closest_point(Vector3::new(5.0, 3.0, 0.0));
        assert_eq!(closest, Vector3::new(5.0, 0.0, 0.0));

        // Point behind the base point (should still work for xlines)
        let closest = xline.closest_point(Vector3::new(-5.0, 3.0, 0.0));
        assert_eq!(closest, Vector3::new(-5.0, 0.0, 0.0));
    }

    #[test]
    fn test_xline_distance_to_point() {
        let xline = XLine::horizontal(Vector3::ZERO);
        let distance = xline.distance_to_point(Vector3::new(5.0, 3.0, 4.0));
        assert!((distance - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_xline_parallel() {
        let xline1 = XLine::horizontal(Vector3::ZERO);
        let xline2 = XLine::horizontal(Vector3::new(0.0, 5.0, 0.0));
        assert!(xline1.is_parallel_to(&xline2, 1e-10));
    }

    #[test]
    fn test_xline_perpendicular() {
        let xline1 = XLine::horizontal(Vector3::ZERO);
        let xline2 = XLine::vertical(Vector3::ZERO);
        assert!(xline1.is_perpendicular_to(&xline2, 1e-10));
    }

    #[test]
    fn test_xline_intersection() {
        let xline1 = XLine::horizontal(Vector3::ZERO);
        let xline2 = XLine::vertical(Vector3::new(5.0, 0.0, 0.0));

        let intersection = xline1.intersection(&xline2, 1e-10);
        assert!(intersection.is_some());
        let point = intersection.unwrap();
        assert!((point.x - 5.0).abs() < 1e-10);
        assert!(point.y.abs() < 1e-10);
    }

    #[test]
    fn test_xline_no_intersection_parallel() {
        let xline1 = XLine::horizontal(Vector3::ZERO);
        let xline2 = XLine::horizontal(Vector3::new(0.0, 5.0, 0.0));

        let intersection = xline1.intersection(&xline2, 1e-10);
        assert!(intersection.is_none());
    }

    #[test]
    fn test_xline_translate() {
        let mut xline = XLine::horizontal(Vector3::ZERO);
        xline.translate(Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(xline.base_point, Vector3::new(1.0, 2.0, 3.0));
        // Direction should not change
        assert_eq!(xline.direction, Vector3::UNIT_X);
    }

    #[test]
    fn test_xline_at_angle() {
        let xline = XLine::at_angle(Vector3::ZERO, std::f64::consts::FRAC_PI_4);
        assert!((xline.direction.x - xline.direction.y).abs() < 1e-10);
    }
}
