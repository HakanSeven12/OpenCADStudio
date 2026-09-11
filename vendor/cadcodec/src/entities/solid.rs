//! Solid entity (filled quadrilateral)

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Solid entity - a filled quadrilateral (3 or 4 vertices)
///
/// A Solid entity is a filled shape defined by 3 or 4 corner points.
/// If only 3 corners are provided, the fourth corner is the same as the third.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Solid {
    pub common: EntityCommon,
    /// First corner point (in OCS)
    pub first_corner: Vector3,
    /// Second corner point (in OCS)
    pub second_corner: Vector3,
    /// Third corner point (in OCS)
    pub third_corner: Vector3,
    /// Fourth corner point (in OCS) - same as third if only 3 corners
    pub fourth_corner: Vector3,
    /// Normal vector (extrusion direction)
    pub normal: Vector3,
    /// Thickness (extrusion distance)
    pub thickness: f64,
    /// Preserve the legacy TRACE entity identity. TRACE and SOLID share the
    /// same geometry but use different DWG/DXF type codes.
    pub is_trace: bool,
}

impl Solid {
    /// Create a new solid with four corners
    pub fn new(first: Vector3, second: Vector3, third: Vector3, fourth: Vector3) -> Self {
        Self {
            common: EntityCommon::default(),
            first_corner: first,
            second_corner: second,
            third_corner: third,
            fourth_corner: fourth,
            normal: Vector3::new(0.0, 0.0, 1.0),
            thickness: 0.0,
            is_trace: false,
        }
    }

    /// Create a triangular solid (3 corners)
    pub fn triangle(first: Vector3, second: Vector3, third: Vector3) -> Self {
        Self::new(first, second, third, third)
    }

    /// Builder: Set the normal vector
    pub fn with_normal(mut self, normal: Vector3) -> Self {
        self.normal = normal;
        self
    }

    /// Builder: Set the thickness
    pub fn with_thickness(mut self, thickness: f64) -> Self {
        self.thickness = thickness;
        self
    }

    /// Check if this is a triangle (3 vertices)
    pub fn is_triangle(&self) -> bool {
        (self.third_corner - self.fourth_corner).length() < 1e-10
    }

    /// Get all corner points
    pub fn corners(&self) -> Vec<Vector3> {
        if self.is_triangle() {
            vec![self.first_corner, self.second_corner, self.third_corner]
        } else {
            vec![
                self.first_corner,
                self.second_corner,
                self.third_corner,
                self.fourth_corner,
            ]
        }
    }

    /// Get corners in visible boundary order.
    /// Stored quadrilaterals use first, second, fourth, third.
    pub fn boundary_corners(&self) -> Vec<Vector3> {
        if self.is_triangle() {
            vec![self.first_corner, self.second_corner, self.third_corner]
        } else {
            vec![
                self.first_corner,
                self.second_corner,
                self.fourth_corner,
                self.third_corner,
            ]
        }
    }
}

impl Entity for Solid {
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
        BoundingBox3D::from_points(&self.corners())
            .unwrap_or_else(|| {
                BoundingBox3D::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 0.0))
            })
            .ocs_to_wcs(self.normal)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_solid(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "SOLID"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_solid(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_solid(self, transform);
    }
}
