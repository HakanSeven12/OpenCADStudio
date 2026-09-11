//! Polyline3D entity - 3D polyline with full 3D vertex coordinates

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Polyline3D flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polyline3DFlags {
    /// Polyline is closed
    pub closed: bool,
    /// Spline fit vertices have been added
    pub spline_fit: bool,
    /// 3D polyline (always true for Polyline3D)
    pub is_3d: bool,
    /// 3D polygon mesh
    pub is_3d_mesh: bool,
    /// Mesh is closed in N direction
    pub mesh_closed_n: bool,
    /// Polyface mesh
    pub is_polyface_mesh: bool,
    /// Linetype pattern generated continuously
    pub linetype_continuous: bool,
}

impl Polyline3DFlags {
    /// Create flags for a simple 3D polyline
    pub fn polyline_3d() -> Self {
        Self {
            is_3d: true,
            ..Default::default()
        }
    }

    /// Create flags from DXF bit flag value
    pub fn from_bits(bits: i32) -> Self {
        Self {
            closed: (bits & 1) != 0,
            spline_fit: (bits & 4) != 0,
            is_3d: (bits & 8) != 0,
            is_3d_mesh: (bits & 16) != 0,
            mesh_closed_n: (bits & 32) != 0,
            is_polyface_mesh: (bits & 64) != 0,
            linetype_continuous: (bits & 128) != 0,
        }
    }

    /// Convert to DXF bit flag value
    pub fn to_bits(&self) -> i32 {
        let mut bits = 0;
        if self.closed {
            bits |= 1;
        }
        if self.spline_fit {
            bits |= 4;
        }
        if self.is_3d {
            bits |= 8;
        }
        if self.is_3d_mesh {
            bits |= 16;
        }
        if self.mesh_closed_n {
            bits |= 32;
        }
        if self.is_polyface_mesh {
            bits |= 64;
        }
        if self.linetype_continuous {
            bits |= 128;
        }
        bits
    }
}

/// Smooth surface type for 3D polylines
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmoothSurfaceType {
    /// No smooth surface
    #[default]
    None = 0,
    /// Quadratic B-spline surface
    QuadraticBSpline = 5,
    /// Cubic B-spline surface
    CubicBSpline = 6,
    /// Bezier surface
    Bezier = 8,
}

impl SmoothSurfaceType {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            5 => SmoothSurfaceType::QuadraticBSpline,
            6 => SmoothSurfaceType::CubicBSpline,
            8 => SmoothSurfaceType::Bezier,
            _ => SmoothSurfaceType::None,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }
}

/// 3D Vertex for Polyline3D
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vertex3DPolyline {
    /// Vertex handle
    pub handle: Handle,
    /// Layer name
    pub layer: String,
    /// Vertex position
    pub position: Vector3,
    /// Vertex flags
    pub flags: i32,
}

impl Vertex3DPolyline {
    /// Create a new 3D vertex
    pub fn new(position: Vector3) -> Self {
        Self {
            handle: Handle::NULL,
            layer: "0".to_string(),
            position,
            flags: 32, // 3D polyline vertex flag
        }
    }

    /// Create with XYZ coordinates
    pub fn from_xyz(x: f64, y: f64, z: f64) -> Self {
        Self::new(Vector3::new(x, y, z))
    }
}

impl Default for Vertex3DPolyline {
    fn default() -> Self {
        Self::new(Vector3::ZERO)
    }
}

/// Polyline3D entity - a 3D polyline with vertices in 3D space
///
/// Unlike 2D polylines, 3D polylines have vertices with full 3D coordinates
/// and no bulge or width properties. They are commonly used for representing
/// 3D curves and paths.
///
/// # DXF Structure
/// POLYLINE entity followed by VERTEX entities, terminated by SEQEND
///
/// # Example
/// ```ignore
/// use acadrust::entities::Polyline3D;
/// use acadrust::types::Vector3;
///
/// let mut polyline = Polyline3D::new();
/// polyline.add_vertex(Vector3::new(0.0, 0.0, 0.0));
/// polyline.add_vertex(Vector3::new(10.0, 0.0, 5.0));
/// polyline.add_vertex(Vector3::new(10.0, 10.0, 10.0));
/// polyline.close();
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polyline3D {
    /// Common entity properties
    pub common: EntityCommon,
    /// Polyline flags
    pub flags: Polyline3DFlags,
    /// Smooth surface type
    pub smooth_type: SmoothSurfaceType,
    /// Starting width (default 0)
    pub default_start_width: f64,
    /// Ending width (default 0)
    pub default_end_width: f64,
    /// Mesh M vertex count (for meshes)
    pub mesh_m_count: u16,
    /// Mesh N vertex count (for meshes)
    pub mesh_n_count: u16,
    /// Smooth surface M density
    pub smooth_m_density: u16,
    /// Smooth surface N density
    pub smooth_n_density: u16,
    /// Elevation (for 2D polylines, not used in 3D)
    pub elevation: f64,
    /// Normal vector (extrusion direction)
    pub normal: Vector3,
    /// Vertex list
    pub vertices: Vec<Vertex3DPolyline>,
}

impl Polyline3D {
    /// Create a new empty 3D polyline
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            flags: Polyline3DFlags::polyline_3d(),
            smooth_type: SmoothSurfaceType::None,
            default_start_width: 0.0,
            default_end_width: 0.0,
            mesh_m_count: 0,
            mesh_n_count: 0,
            smooth_m_density: 0,
            smooth_n_density: 0,
            elevation: 0.0,
            normal: Vector3::UNIT_Z,
            vertices: Vec::new(),
        }
    }

    /// Create a 3D polyline from a list of points
    pub fn from_points(points: Vec<Vector3>) -> Self {
        let mut polyline = Self::new();
        for point in points {
            polyline.add_vertex(point);
        }
        polyline
    }

    /// Add a vertex at the given position
    pub fn add_vertex(&mut self, position: Vector3) {
        self.vertices.push(Vertex3DPolyline::new(position));
    }

    /// Add a vertex with full vertex structure
    pub fn add_vertex_full(&mut self, vertex: Vertex3DPolyline) {
        self.vertices.push(vertex);
    }

    /// Get the number of vertices
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Check if the polyline is closed
    pub fn is_closed(&self) -> bool {
        self.flags.closed
    }

    /// Close the polyline
    pub fn close(&mut self) {
        self.flags.closed = true;
    }

    /// Open the polyline
    pub fn open(&mut self) {
        self.flags.closed = false;
    }

    /// Get the vertex at the given index
    pub fn get_vertex(&self, index: usize) -> Option<&Vertex3DPolyline> {
        self.vertices.get(index)
    }

    /// Get a mutable reference to the vertex at the given index
    pub fn get_vertex_mut(&mut self, index: usize) -> Option<&mut Vertex3DPolyline> {
        self.vertices.get_mut(index)
    }

    /// Calculate the total length of the polyline
    pub fn length(&self) -> f64 {
        if self.vertices.len() < 2 {
            return 0.0;
        }

        let mut total = 0.0;
        for i in 0..self.vertices.len() - 1 {
            total += self.vertices[i]
                .position
                .distance(&self.vertices[i + 1].position);
        }

        // Add closing segment if closed
        if self.is_closed() && self.vertices.len() >= 2 {
            total += self
                .vertices
                .last()
                .unwrap()
                .position
                .distance(&self.vertices.first().unwrap().position);
        }

        total
    }

    /// Get the centroid of the polyline
    pub fn centroid(&self) -> Vector3 {
        if self.vertices.is_empty() {
            return Vector3::ZERO;
        }

        let sum: Vector3 = self
            .vertices
            .iter()
            .fold(Vector3::ZERO, |acc, v| acc + v.position);
        sum * (1.0 / self.vertices.len() as f64)
    }

    /// Get all vertex positions as a vector
    pub fn positions(&self) -> Vec<Vector3> {
        self.vertices.iter().map(|v| v.position).collect()
    }

    /// Get the start point
    pub fn start_point(&self) -> Option<Vector3> {
        self.vertices.first().map(|v| v.position)
    }

    /// Get the end point
    pub fn end_point(&self) -> Option<Vector3> {
        self.vertices.last().map(|v| v.position)
    }

    /// Reverse the direction of the polyline
    pub fn reverse(&mut self) {
        self.vertices.reverse();
    }

    /// Remove a vertex at the given index
    pub fn remove_vertex(&mut self, index: usize) -> Option<Vertex3DPolyline> {
        if index < self.vertices.len() {
            Some(self.vertices.remove(index))
        } else {
            None
        }
    }

    /// Insert a vertex at the given index
    pub fn insert_vertex(&mut self, index: usize, position: Vector3) {
        if index <= self.vertices.len() {
            self.vertices.insert(index, Vertex3DPolyline::new(position));
        }
    }

    /// Clear all vertices
    pub fn clear(&mut self) {
        self.vertices.clear();
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

    /// Builder: Set as closed
    pub fn with_closed(mut self) -> Self {
        self.flags.closed = true;
        self
    }
}

impl Default for Polyline3D {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Polyline3D {
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
        let positions: Vec<Vector3> = self.vertices.iter().map(|v| v.position).collect();
        BoundingBox3D::from_points(&positions).unwrap_or_default()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_polyline3d(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "POLYLINE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_polyline3d(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polyline3d_creation() {
        let polyline = Polyline3D::new();
        assert_eq!(polyline.vertex_count(), 0);
        assert!(!polyline.is_closed());
        assert!(polyline.flags.is_3d);
    }

    #[test]
    fn test_polyline3d_from_points() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 5.0),
            Vector3::new(10.0, 10.0, 10.0),
        ];
        let polyline = Polyline3D::from_points(points);
        assert_eq!(polyline.vertex_count(), 3);
    }

    #[test]
    fn test_polyline3d_add_vertex() {
        let mut polyline = Polyline3D::new();
        polyline.add_vertex(Vector3::new(0.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(10.0, 0.0, 5.0));
        assert_eq!(polyline.vertex_count(), 2);
    }

    #[test]
    fn test_polyline3d_close() {
        let mut polyline = Polyline3D::new();
        assert!(!polyline.is_closed());
        polyline.close();
        assert!(polyline.is_closed());
        polyline.open();
        assert!(!polyline.is_closed());
    }

    #[test]
    fn test_polyline3d_length() {
        let mut polyline = Polyline3D::new();
        polyline.add_vertex(Vector3::new(0.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(10.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(10.0, 10.0, 0.0));

        // Open polyline: 10 + 10 = 20
        assert!((polyline.length() - 20.0).abs() < 1e-10);

        // Closed polyline: 10 + 10 + sqrt(200) ≈ 34.14
        polyline.close();
        let expected = 20.0 + (200.0_f64).sqrt();
        assert!((polyline.length() - expected).abs() < 1e-10);
    }

    #[test]
    fn test_polyline3d_centroid() {
        let mut polyline = Polyline3D::new();
        polyline.add_vertex(Vector3::new(0.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(10.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(10.0, 10.0, 0.0));
        polyline.add_vertex(Vector3::new(0.0, 10.0, 0.0));

        let centroid = polyline.centroid();
        assert!((centroid.x - 5.0).abs() < 1e-10);
        assert!((centroid.y - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_polyline3d_reverse() {
        let mut polyline = Polyline3D::new();
        polyline.add_vertex(Vector3::new(0.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(10.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(20.0, 0.0, 0.0));

        polyline.reverse();

        assert_eq!(polyline.vertices[0].position.x, 20.0);
        assert_eq!(polyline.vertices[2].position.x, 0.0);
    }

    #[test]
    fn test_polyline3d_translate() {
        let mut polyline = Polyline3D::new();
        polyline.add_vertex(Vector3::new(0.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(10.0, 0.0, 0.0));

        polyline.translate(Vector3::new(5.0, 5.0, 5.0));

        assert_eq!(polyline.vertices[0].position, Vector3::new(5.0, 5.0, 5.0));
        assert_eq!(polyline.vertices[1].position, Vector3::new(15.0, 5.0, 5.0));
    }

    #[test]
    fn test_polyline3d_flags() {
        let flags = Polyline3DFlags::from_bits(9); // closed + 3D
        assert!(flags.closed);
        assert!(flags.is_3d);
        assert!(!flags.spline_fit);

        assert_eq!(flags.to_bits(), 9);
    }

    #[test]
    fn test_polyline3d_start_end_points() {
        let mut polyline = Polyline3D::new();
        assert!(polyline.start_point().is_none());
        assert!(polyline.end_point().is_none());

        polyline.add_vertex(Vector3::new(0.0, 0.0, 0.0));
        polyline.add_vertex(Vector3::new(10.0, 0.0, 0.0));

        assert_eq!(polyline.start_point(), Some(Vector3::new(0.0, 0.0, 0.0)));
        assert_eq!(polyline.end_point(), Some(Vector3::new(10.0, 0.0, 0.0)));
    }
}
