//! PolygonMesh entity — 3D polygon mesh (DXF POLYLINE with flag bit 16)

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transform, Transparency, Vector3};

/// Smooth surface type for polygon meshes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum SurfaceSmoothType {
    /// No smooth surface fitting
    NoSmooth = 0,
    /// Quadratic B-spline surface
    Quadratic = 5,
    /// Cubic B-spline surface
    Cubic = 6,
    /// Bézier surface
    Bezier = 8,
}

impl SurfaceSmoothType {
    /// Create from DXF code value
    pub fn from_i16(v: i16) -> Self {
        match v {
            5 => SurfaceSmoothType::Quadratic,
            6 => SurfaceSmoothType::Cubic,
            8 => SurfaceSmoothType::Bezier,
            _ => SurfaceSmoothType::NoSmooth,
        }
    }
}

/// A vertex in a polygon mesh
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PolygonMeshVertex {
    /// Common entity data
    pub common: EntityCommon,
    /// 3D location
    pub location: Vector3,
    /// Vertex flags (code 70)
    pub flags: i16,
}

impl PolygonMeshVertex {
    /// Create a new polygon mesh vertex
    pub fn new() -> Self {
        PolygonMeshVertex {
            common: EntityCommon::new(),
            location: Vector3::ZERO,
            flags: 0,
        }
    }

    /// Create a vertex at a specific location
    pub fn at(location: Vector3) -> Self {
        PolygonMeshVertex {
            location,
            ..Self::new()
        }
    }
}

impl Default for PolygonMeshVertex {
    fn default() -> Self {
        Self::new()
    }
}

bitflags::bitflags! {
    // Flags for polygon mesh (code 70)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct PolygonMeshFlags: i16 {
        /// Closed in M direction
        const CLOSED_M = 1;
        /// Curve-fit vertices added
        const CURVE_FIT = 2;
        /// Spline-fit vertices added
        const SPLINE_FIT = 4;
        /// Polygon mesh (always set for PolygonMesh)
        const POLYGON_MESH = 16;
        /// Closed in N direction
        const CLOSED_N = 32;
        /// Continuous linetype pattern
        const CONTINUOUS_LINETYPE = 128;
    }
}

/// A 3D polygon mesh entity.
///
/// In DXF this is a POLYLINE entity with flag bit 16 set
/// and subclass marker `AcDbPolygonMesh`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PolygonMesh {
    /// Common entity data
    pub common: EntityCommon,
    /// Mesh flags (always includes POLYGON_MESH)
    pub flags: PolygonMeshFlags,
    /// Number of vertices in the M direction
    pub m_vertex_count: i16,
    /// Number of vertices in the N direction
    pub n_vertex_count: i16,
    /// Smooth surface density in the M direction
    pub m_smooth_density: i16,
    /// Smooth surface density in the N direction
    pub n_smooth_density: i16,
    /// Surface smooth type
    pub smooth_type: SurfaceSmoothType,
    /// Elevation (code 30)
    pub elevation: f64,
    /// Normal vector
    pub normal: Vector3,
    /// Mesh vertices (M × N grid)
    pub vertices: Vec<PolygonMeshVertex>,
}

impl PolygonMesh {
    /// Create a new empty polygon mesh
    pub fn new() -> Self {
        PolygonMesh {
            common: EntityCommon::new(),
            flags: PolygonMeshFlags::POLYGON_MESH,
            m_vertex_count: 0,
            n_vertex_count: 0,
            m_smooth_density: 0,
            n_smooth_density: 0,
            smooth_type: SurfaceSmoothType::NoSmooth,
            elevation: 0.0,
            normal: Vector3::UNIT_Z,
            vertices: Vec::new(),
        }
    }

    /// Check if mesh is closed in M direction
    pub fn is_closed_m(&self) -> bool {
        self.flags.contains(PolygonMeshFlags::CLOSED_M)
    }

    /// Check if mesh is closed in N direction
    pub fn is_closed_n(&self) -> bool {
        self.flags.contains(PolygonMeshFlags::CLOSED_N)
    }
}

impl Default for PolygonMesh {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for PolygonMesh {
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
            .unwrap_or_else(|| BoundingBox3D::from_point(Vector3::ZERO))
    }
    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_polygon_mesh(self, offset);
    }
    fn entity_type(&self) -> &'static str {
        "POLYLINE"
    }
    fn apply_transform(&mut self, transform: &Transform) {
        super::transform::transform_polygon_mesh(self, transform);
    }
}
