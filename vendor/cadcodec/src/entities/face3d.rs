//! Face3D entity (3D face)

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Invisible edge flags for Face3D
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InvisibleEdgeFlags {
    bits: u8,
}

impl InvisibleEdgeFlags {
    pub const NONE: Self = Self { bits: 0 };
    pub const FIRST: Self = Self { bits: 1 };
    pub const SECOND: Self = Self { bits: 2 };
    pub const THIRD: Self = Self { bits: 4 };
    pub const FOURTH: Self = Self { bits: 8 };

    pub fn new() -> Self {
        Self::NONE
    }

    /// Create from raw bits value
    pub fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    /// Get the raw bits value
    pub fn bits(&self) -> u8 {
        self.bits
    }

    pub fn is_first_invisible(&self) -> bool {
        self.bits & 1 != 0
    }

    pub fn is_second_invisible(&self) -> bool {
        self.bits & 2 != 0
    }

    pub fn is_third_invisible(&self) -> bool {
        self.bits & 4 != 0
    }

    pub fn is_fourth_invisible(&self) -> bool {
        self.bits & 8 != 0
    }

    pub fn set_first_invisible(&mut self, value: bool) {
        if value {
            self.bits |= 1;
        } else {
            self.bits &= !1;
        }
    }

    pub fn set_second_invisible(&mut self, value: bool) {
        if value {
            self.bits |= 2;
        } else {
            self.bits &= !2;
        }
    }

    pub fn set_third_invisible(&mut self, value: bool) {
        if value {
            self.bits |= 4;
        } else {
            self.bits &= !4;
        }
    }

    pub fn set_fourth_invisible(&mut self, value: bool) {
        if value {
            self.bits |= 8;
        } else {
            self.bits &= !8;
        }
    }
}

impl Default for InvisibleEdgeFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// Face3D entity - a 3D face with 3 or 4 vertices
///
/// A Face3D entity is a 3D surface defined by 3 or 4 corner points.
/// Individual edges can be marked as invisible.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Face3D {
    pub common: EntityCommon,
    /// First corner point (in WCS)
    pub first_corner: Vector3,
    /// Second corner point (in WCS)
    pub second_corner: Vector3,
    /// Third corner point (in WCS)
    pub third_corner: Vector3,
    /// Fourth corner point (in WCS) - same as third if only 3 corners
    pub fourth_corner: Vector3,
    /// Invisible edge flags
    pub invisible_edges: InvisibleEdgeFlags,
}

impl Face3D {
    /// Create a new 3D face with four corners
    pub fn new(first: Vector3, second: Vector3, third: Vector3, fourth: Vector3) -> Self {
        Self {
            common: EntityCommon::default(),
            first_corner: first,
            second_corner: second,
            third_corner: third,
            fourth_corner: fourth,
            invisible_edges: InvisibleEdgeFlags::new(),
        }
    }

    /// Create a triangular 3D face (3 corners)
    pub fn triangle(first: Vector3, second: Vector3, third: Vector3) -> Self {
        Self::new(first, second, third, third)
    }

    /// Builder: Set invisible edge flags
    pub fn with_invisible_edges(mut self, flags: InvisibleEdgeFlags) -> Self {
        self.invisible_edges = flags;
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

    /// Calculate the area of the face
    pub fn area(&self) -> f64 {
        if self.is_triangle() {
            // Triangle area using cross product
            let v1 = self.second_corner - self.first_corner;
            let v2 = self.third_corner - self.first_corner;
            v1.cross(&v2).length() * 0.5
        } else {
            // Quadrilateral area (sum of two triangles)
            let v1 = self.second_corner - self.first_corner;
            let v2 = self.third_corner - self.first_corner;
            let area1 = v1.cross(&v2).length() * 0.5;

            let v3 = self.third_corner - self.first_corner;
            let v4 = self.fourth_corner - self.first_corner;
            let area2 = v3.cross(&v4).length() * 0.5;

            area1 + area2
        }
    }
}

impl Entity for Face3D {
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
        BoundingBox3D::from_points(&self.corners()).unwrap_or_else(|| {
            BoundingBox3D::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 0.0))
        })
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_face3d(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "3DFACE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_face3d(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_face3d(self, transform);
    }
}
