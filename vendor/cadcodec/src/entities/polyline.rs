//! Polyline entities (2D and 3D polylines)

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector2, Vector3};

/// Polyline flags (matches DXF group code 70)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PolylineFlags {
    bits: u16,
}

impl PolylineFlags {
    pub const CLOSED: Self = Self { bits: 1 };
    pub const CURVE_FIT: Self = Self { bits: 2 };
    pub const SPLINE_FIT: Self = Self { bits: 4 };
    pub const POLYLINE_3D: Self = Self { bits: 8 };
    pub const POLYGON_MESH: Self = Self { bits: 16 };
    pub const CLOSED_N: Self = Self { bits: 32 };
    pub const POLYFACE_MESH: Self = Self { bits: 64 };
    pub const LINETYPE_CONTINUOUS: Self = Self { bits: 128 };

    pub fn new() -> Self {
        Self { bits: 0 }
    }

    pub fn from_bits(bits: u16) -> Self {
        Self { bits }
    }

    pub fn bits(&self) -> u16 {
        self.bits
    }

    pub fn is_closed(&self) -> bool {
        self.bits & 1 != 0
    }

    pub fn is_3d(&self) -> bool {
        self.bits & 8 != 0
    }

    pub fn is_spline_fit(&self) -> bool {
        self.bits & 4 != 0
    }

    pub fn set_closed(&mut self, value: bool) {
        if value {
            self.bits |= 1;
        } else {
            self.bits &= !1;
        }
    }

    pub fn set_3d(&mut self, value: bool) {
        if value {
            self.bits |= 8;
        } else {
            self.bits &= !8;
        }
    }
}

impl std::ops::BitOr for PolylineFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            bits: self.bits | rhs.bits,
        }
    }
}

impl std::ops::BitOrAssign for PolylineFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

/// Vertex flags (matches DXF group code 70)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VertexFlags {
    bits: u8,
}

impl VertexFlags {
    pub const EXTRA_VERTEX: Self = Self { bits: 1 };
    pub const CURVE_FIT_TANGENT: Self = Self { bits: 2 };
    pub const SPLINE_VERTEX: Self = Self { bits: 8 };
    pub const SPLINE_CONTROL: Self = Self { bits: 16 };
    pub const POLYLINE_3D: Self = Self { bits: 32 };
    pub const POLYGON_MESH: Self = Self { bits: 64 };
    pub const POLYFACE_FACE: Self = Self { bits: 128 };

    pub fn new() -> Self {
        Self { bits: 0 }
    }

    pub fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    pub fn bits(&self) -> u8 {
        self.bits
    }
}

/// Smooth surface type (matches DXF group code 75)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmoothSurfaceType {
    #[default]
    None = 0,
    QuadraticBSpline = 5,
    CubicBSpline = 6,
    Bezier = 8,
}

impl From<i16> for SmoothSurfaceType {
    fn from(value: i16) -> Self {
        match value {
            5 => SmoothSurfaceType::QuadraticBSpline,
            6 => SmoothSurfaceType::CubicBSpline,
            8 => SmoothSurfaceType::Bezier,
            _ => SmoothSurfaceType::None,
        }
    }
}

/// A vertex in a 2D polyline
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vertex2D {
    /// Location of the vertex (X, Y in OCS, Z is elevation)
    pub location: Vector3,
    /// Vertex flags
    pub flags: VertexFlags,
    /// Start width (0 = use default)
    pub start_width: f64,
    /// End width (0 = use default)
    pub end_width: f64,
    /// Bulge (0 = straight segment, <0 = clockwise arc, >0 = counter-clockwise arc)
    pub bulge: f64,
    /// Curve fit tangent direction
    pub curve_tangent: f64,
    /// Vertex ID (R2010+)
    pub id: i32,
}

impl Vertex2D {
    pub fn new(location: Vector3) -> Self {
        Self {
            location,
            flags: VertexFlags::new(),
            start_width: 0.0,
            end_width: 0.0,
            bulge: 0.0,
            curve_tangent: 0.0,
            id: 0,
        }
    }

    pub fn from_point(point: Vector2) -> Self {
        Self::new(Vector3::new(point.x, point.y, 0.0))
    }

    pub fn with_bulge(mut self, bulge: f64) -> Self {
        self.bulge = bulge;
        self
    }

    pub fn with_width(mut self, start_width: f64, end_width: f64) -> Self {
        self.start_width = start_width;
        self.end_width = end_width;
        self
    }
}

/// A vertex in a 3D polyline
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vertex3D {
    /// Location of the vertex
    pub location: Vector3,
    /// Vertex flags
    pub flags: VertexFlags,
}

impl Vertex3D {
    /// Create a new vertex
    pub fn new(location: Vector3) -> Self {
        Self {
            location,
            flags: VertexFlags::new(),
        }
    }

    /// Create a vertex from coordinates
    pub fn from_coords(x: f64, y: f64, z: f64) -> Self {
        Vertex3D::new(Vector3::new(x, y, z))
    }
}

/// A 2D polyline entity (heavy polyline with vertices)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polyline2D {
    /// Common entity data
    pub common: EntityCommon,
    /// Polyline flags
    pub flags: PolylineFlags,
    /// Smooth surface type
    pub smooth_surface: SmoothSurfaceType,
    /// Default start width
    pub start_width: f64,
    /// Default end width
    pub end_width: f64,
    /// Thickness (extrusion height)
    pub thickness: f64,
    /// Elevation (Z coordinate in OCS)
    pub elevation: f64,
    /// Normal vector (extrusion direction)
    pub normal: Vector3,
    /// Vertices
    pub vertices: Vec<Vertex2D>,
}

impl Polyline2D {
    pub fn new() -> Self {
        Self {
            common: EntityCommon::new(),
            flags: PolylineFlags::new(),
            smooth_surface: SmoothSurfaceType::None,
            start_width: 0.0,
            end_width: 0.0,
            thickness: 0.0,
            elevation: 0.0,
            normal: Vector3::new(0.0, 0.0, 1.0),
            vertices: Vec::new(),
        }
    }

    pub fn add_vertex(&mut self, vertex: Vertex2D) {
        self.vertices.push(vertex);
    }

    pub fn is_closed(&self) -> bool {
        self.flags.is_closed()
    }

    pub fn close(&mut self) {
        self.flags.set_closed(true);
    }
}

impl Default for Polyline2D {
    fn default() -> Self {
        Self::new()
    }
}

/// A 3D polyline entity
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polyline {
    /// Common entity data
    pub common: EntityCommon,
    /// Polyline flags
    pub flags: PolylineFlags,
    /// Vertices of the polyline
    pub vertices: Vec<Vertex3D>,
}

impl Polyline {
    /// Create a new empty polyline
    pub fn new() -> Self {
        let mut flags = PolylineFlags::new();
        flags.set_3d(true);
        Polyline {
            common: EntityCommon::new(),
            flags,
            vertices: Vec::new(),
        }
    }

    /// Create a polyline from a list of points
    pub fn from_points(points: Vec<Vector3>) -> Self {
        Polyline {
            vertices: points.into_iter().map(Vertex3D::new).collect(),
            ..Self::new()
        }
    }

    /// Add a vertex to the polyline
    pub fn add_vertex(&mut self, vertex: Vertex3D) {
        self.vertices.push(vertex);
    }

    /// Add a point to the polyline
    pub fn add_point(&mut self, point: Vector3) {
        self.vertices.push(Vertex3D::new(point));
    }

    /// Get the number of vertices
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Check if closed
    pub fn is_closed(&self) -> bool {
        self.flags.is_closed()
    }

    /// Close the polyline
    pub fn close(&mut self) {
        self.flags.set_closed(true);
    }
}

impl Default for Polyline {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Polyline2D {
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
        if self.vertices.is_empty() {
            return BoundingBox3D::from_point(Vector3::ZERO);
        }

        let points: Vec<Vector3> = self.vertices.iter().map(|v| v.location).collect();
        BoundingBox3D::from_points(&points)
            .unwrap()
            .ocs_to_wcs(self.normal)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_polyline2d(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "POLYLINE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_polyline2d(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_polyline2d(self, transform);
    }
}

impl Entity for Polyline {
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
        if self.vertices.is_empty() {
            return BoundingBox3D::from_point(Vector3::ZERO);
        }

        let points: Vec<Vector3> = self.vertices.iter().map(|v| v.location).collect();
        BoundingBox3D::from_points(&points).unwrap()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_polyline(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "POLYLINE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_polyline(self, transform);
    }
}
