//! PolyfaceMesh entity implementation.
//!
//! The PolyfaceMesh entity represents a mesh defined by vertices and face
//! records, where each face references vertices by 1-based indices.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

use bitflags::bitflags;

// ============================================================================
// Flags
// ============================================================================

bitflags! {
    /// Polyline flags for polyface mesh.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct PolyfaceMeshFlags: i16 {
        /// No flags.
        const NONE = 0;
        /// Closed polyline.
        const CLOSED = 1;
        /// Curve-fit vertices added.
        const CURVE_FIT = 2;
        /// Spline-fit vertices added.
        const SPLINE_FIT = 4;
        /// 3D polyline.
        const POLYLINE_3D = 8;
        /// 3D polygon mesh.
        const POLYGON_MESH = 16;
        /// Polygon mesh closed in N direction.
        const CLOSED_N = 32;
        /// Polyface mesh (always set for PolyfaceMesh).
        const POLYFACE_MESH = 64;
        /// Continuous linetype pattern.
        const CONTINUOUS_LINETYPE = 128;
    }
}

bitflags! {
    /// Vertex flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct PolyfaceVertexFlags: i16 {
        /// No flags.
        const NONE = 0;
        /// Extra vertex created by curve-fitting.
        const CURVE_FIT_EXTRA = 1;
        /// Curve-fit tangent defined.
        const CURVE_FIT_TANGENT = 2;
        /// Spline vertex.
        const SPLINE_VERTEX = 8;
        /// Spline control point.
        const SPLINE_CONTROL = 16;
        /// 3D polyline vertex.
        const POLYLINE_3D = 32;
        /// 3D polygon mesh vertex.
        const POLYGON_MESH = 64;
        /// Polyface mesh vertex (always set for mesh vertices).
        const POLYFACE_MESH = 128;
    }
}

/// Smooth surface type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum PolyfaceSmoothType {
    /// No smooth surface.
    #[default]
    None = 0,
    /// Quadratic B-spline surface.
    Quadratic = 5,
    /// Cubic B-spline surface.
    Cubic = 6,
    /// Bezier surface.
    Bezier = 8,
}

impl From<i16> for PolyfaceSmoothType {
    fn from(value: i16) -> Self {
        match value {
            5 => Self::Quadratic,
            6 => Self::Cubic,
            8 => Self::Bezier,
            _ => Self::None,
        }
    }
}

// ============================================================================
// Vertex Face Mesh (3D Vertex Position)
// ============================================================================

/// A vertex in a polyface mesh (3D position).
///
/// Represents a 3D vertex position in the mesh.
/// DXF subclass: AcDbPolyFaceMeshVertex
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PolyfaceVertex {
    /// Common entity data (for DXF compatibility).
    pub common: EntityCommon,
    /// 3D position of the vertex.
    /// DXF codes: 10, 20, 30
    pub location: Vector3,
    /// Vertex flags.
    /// DXF code: 70
    pub flags: PolyfaceVertexFlags,
    /// Bulge value (for curve fitting).
    /// DXF code: 42
    pub bulge: f64,
    /// Starting width.
    /// DXF code: 40
    pub start_width: f64,
    /// Ending width.
    /// DXF code: 41
    pub end_width: f64,
    /// Curve fit tangent direction.
    /// DXF code: 50
    pub curve_tangent: f64,
    /// Vertex identifier.
    /// DXF code: 91
    pub id: i32,
}

impl PolyfaceVertex {
    /// Entity type name.
    pub const ENTITY_NAME: &'static str = "VERTEX";

    /// DXF subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbPolyFaceMeshVertex";

    /// Object type code (DWG).
    pub const OBJECT_TYPE: u16 = 0x0D; // 13

    /// Creates a new vertex at the given location.
    pub fn new(location: Vector3) -> Self {
        PolyfaceVertex {
            common: EntityCommon::default(),
            location,
            flags: PolyfaceVertexFlags::POLYFACE_MESH,
            bulge: 0.0,
            start_width: 0.0,
            end_width: 0.0,
            curve_tangent: 0.0,
            id: 0,
        }
    }

    /// Creates a vertex from coordinates.
    pub fn from_xyz(x: f64, y: f64, z: f64) -> Self {
        Self::new(Vector3::new(x, y, z))
    }
}

impl Default for PolyfaceVertex {
    fn default() -> Self {
        Self::new(Vector3::ZERO)
    }
}

// ============================================================================
// Face Record
// ============================================================================

/// A face record in a polyface mesh.
///
/// References vertices by 1-based indices. Negative indices indicate
/// that the edge starting from that vertex is invisible.
///
/// DXF subclass: AcDbFaceRecord
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PolyfaceFace {
    /// Common entity data (for DXF compatibility).
    pub common: EntityCommon,
    /// Vertex flags.
    /// DXF code: 70
    pub flags: PolyfaceVertexFlags,
    /// First vertex index (1-based, negative = invisible edge, 0 = unused).
    /// DXF code: 71
    pub index1: i16,
    /// Second vertex index (1-based, negative = invisible edge, 0 = unused).
    /// DXF code: 72
    pub index2: i16,
    /// Third vertex index (1-based, negative = invisible edge, 0 = unused).
    /// DXF code: 73
    pub index3: i16,
    /// Fourth vertex index (1-based, 0 = triangle, negative = invisible edge).
    /// DXF code: 74
    pub index4: i16,
    /// Face color (optional per-face color).
    pub color: Option<Color>,
}

impl PolyfaceFace {
    /// Entity type name.
    pub const ENTITY_NAME: &'static str = "VERTEX";

    /// DXF subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbFaceRecord";

    /// Object type code (DWG).
    pub const OBJECT_TYPE: u16 = 0x0E; // 14

    /// Creates a new triangle face.
    pub fn triangle(v1: i16, v2: i16, v3: i16) -> Self {
        PolyfaceFace {
            common: EntityCommon::default(),
            flags: PolyfaceVertexFlags::NONE,
            index1: v1,
            index2: v2,
            index3: v3,
            index4: 0,
            color: None,
        }
    }

    /// Creates a new quad face.
    pub fn quad(v1: i16, v2: i16, v3: i16, v4: i16) -> Self {
        PolyfaceFace {
            common: EntityCommon::default(),
            flags: PolyfaceVertexFlags::NONE,
            index1: v1,
            index2: v2,
            index3: v3,
            index4: v4,
            color: None,
        }
    }

    /// Creates a triangle with invisible edges specified.
    pub fn triangle_with_visibility(
        v1: i16,
        v2: i16,
        v3: i16,
        edge1_invisible: bool,
        edge2_invisible: bool,
        edge3_invisible: bool,
    ) -> Self {
        PolyfaceFace {
            common: EntityCommon::default(),
            flags: PolyfaceVertexFlags::NONE,
            index1: if edge1_invisible { -v1 } else { v1 },
            index2: if edge2_invisible { -v2 } else { v2 },
            index3: if edge3_invisible { -v3 } else { v3 },
            index4: 0,
            color: None,
        }
    }

    /// Returns true if this is a triangle (3 vertices).
    pub fn is_triangle(&self) -> bool {
        self.index4 == 0 || self.index4.abs() == self.index3.abs()
    }

    /// Returns true if this is a quad (4 vertices).
    pub fn is_quad(&self) -> bool {
        self.index4 != 0 && self.index4.abs() != self.index3.abs()
    }

    /// Returns the number of vertices in this face (3 or 4).
    pub fn vertex_count(&self) -> usize {
        if self.is_triangle() {
            3
        } else {
            4
        }
    }

    /// Returns the vertex indices as absolute values (ignoring visibility).
    pub fn vertex_indices(&self) -> Vec<i16> {
        let mut indices = vec![self.index1.abs(), self.index2.abs(), self.index3.abs()];
        if self.is_quad() {
            indices.push(self.index4.abs());
        }
        indices
    }

    /// Returns true if edge 1 (from vertex 1 to vertex 2) is invisible.
    pub fn is_edge1_invisible(&self) -> bool {
        self.index1 < 0
    }

    /// Returns true if edge 2 (from vertex 2 to vertex 3) is invisible.
    pub fn is_edge2_invisible(&self) -> bool {
        self.index2 < 0
    }

    /// Returns true if edge 3 (from vertex 3 to vertex 4 or back to 1) is invisible.
    pub fn is_edge3_invisible(&self) -> bool {
        self.index3 < 0
    }

    /// Returns true if edge 4 (from vertex 4 back to 1) is invisible.
    pub fn is_edge4_invisible(&self) -> bool {
        self.index4 < 0
    }

    /// Sets edge visibility.
    pub fn set_edge_visibility(&mut self, edge: usize, visible: bool) {
        let make_visible = |idx: i16| idx.abs();
        let make_invisible = |idx: i16| -idx.abs();

        match edge {
            0 => {
                self.index1 = if visible {
                    make_visible(self.index1)
                } else {
                    make_invisible(self.index1)
                }
            }
            1 => {
                self.index2 = if visible {
                    make_visible(self.index2)
                } else {
                    make_invisible(self.index2)
                }
            }
            2 => {
                self.index3 = if visible {
                    make_visible(self.index3)
                } else {
                    make_invisible(self.index3)
                }
            }
            3 => {
                self.index4 = if visible {
                    make_visible(self.index4)
                } else {
                    make_invisible(self.index4)
                }
            }
            _ => {}
        }
    }

    /// Reverses the winding order of the face.
    pub fn reverse(&mut self) {
        if self.is_triangle() {
            std::mem::swap(&mut self.index1, &mut self.index3);
        } else {
            std::mem::swap(&mut self.index1, &mut self.index4);
            std::mem::swap(&mut self.index2, &mut self.index3);
        }
    }
}

impl Default for PolyfaceFace {
    fn default() -> Self {
        Self::triangle(1, 2, 3)
    }
}

// ============================================================================
// Polyface Mesh Entity
// ============================================================================

/// Polyface mesh entity.
///
/// A mesh defined by vertices and face records. Each face references
/// vertices by 1-based indices (first vertex = index 1).
///
/// # DXF Information
/// - Entity type: POLYLINE (with POLYFACE_MESH flag)
/// - Subclass marker: AcDbPolyFaceMesh
/// - Object type code: 0x1D (29)
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::{PolyfaceMesh, PolyfaceVertex, PolyfaceFace};
/// use acadrust::types::Vector3;
///
/// // Create a simple pyramid
/// let mut mesh = PolyfaceMesh::new();
///
/// // Add vertices (indices will be 1, 2, 3, 4, 5)
/// mesh.add_vertex(PolyfaceVertex::from_xyz(0.0, 0.0, 0.0));  // 1: base corner
/// mesh.add_vertex(PolyfaceVertex::from_xyz(1.0, 0.0, 0.0));  // 2: base corner
/// mesh.add_vertex(PolyfaceVertex::from_xyz(1.0, 1.0, 0.0));  // 3: base corner
/// mesh.add_vertex(PolyfaceVertex::from_xyz(0.0, 1.0, 0.0));  // 4: base corner
/// mesh.add_vertex(PolyfaceVertex::from_xyz(0.5, 0.5, 1.0));  // 5: apex
///
/// // Add faces (using 1-based vertex indices)
/// mesh.add_face(PolyfaceFace::quad(1, 2, 3, 4));   // base
/// mesh.add_face(PolyfaceFace::triangle(1, 2, 5)); // side 1
/// mesh.add_face(PolyfaceFace::triangle(2, 3, 5)); // side 2
/// mesh.add_face(PolyfaceFace::triangle(3, 4, 5)); // side 3
/// mesh.add_face(PolyfaceFace::triangle(4, 1, 5)); // side 4
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PolyfaceMesh {
    /// Common entity data.
    pub common: EntityCommon,
    /// Elevation (Z value for 2D operations).
    /// DXF code: 30
    pub elevation: f64,
    /// Polyline flags.
    /// DXF code: 70
    pub flags: PolyfaceMeshFlags,
    /// Extrusion normal.
    /// DXF codes: 210, 220, 230
    pub normal: Vector3,
    /// Default start width.
    /// DXF code: 40
    pub start_width: f64,
    /// Default end width.
    /// DXF code: 41
    pub end_width: f64,
    /// Smooth surface type.
    /// DXF code: 75
    pub smooth_surface: PolyfaceSmoothType,
    /// Extrusion thickness.
    /// DXF code: 39
    pub thickness: f64,
    /// Mesh vertices (3D positions).
    pub vertices: Vec<PolyfaceVertex>,
    /// Face records (referencing vertices by index).
    pub faces: Vec<PolyfaceFace>,
    /// SEQEND handle (for DXF/DWG compatibility).
    pub seqend_handle: Option<Handle>,
}

impl PolyfaceMesh {
    /// Entity type name.
    pub const ENTITY_NAME: &'static str = "POLYLINE";

    /// DXF subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbPolyFaceMesh";

    /// Object type code (DWG).
    pub const OBJECT_TYPE: u16 = 0x1D; // 29

    /// Creates a new empty polyface mesh.
    pub fn new() -> Self {
        PolyfaceMesh {
            common: EntityCommon::default(),
            elevation: 0.0,
            flags: PolyfaceMeshFlags::POLYFACE_MESH,
            normal: Vector3::UNIT_Z,
            start_width: 0.0,
            end_width: 0.0,
            smooth_surface: PolyfaceSmoothType::None,
            thickness: 0.0,
            vertices: Vec::new(),
            faces: Vec::new(),
            seqend_handle: None,
        }
    }

    /// Creates a mesh from vertices and faces.
    pub fn from_data(vertices: Vec<PolyfaceVertex>, faces: Vec<PolyfaceFace>) -> Self {
        PolyfaceMesh {
            vertices,
            faces,
            ..Self::new()
        }
    }

    /// Adds a vertex and returns its 1-based index.
    pub fn add_vertex(&mut self, vertex: PolyfaceVertex) -> i16 {
        self.vertices.push(vertex);
        self.vertices.len() as i16 // 1-based index
    }

    /// Adds a vertex from coordinates and returns its 1-based index.
    pub fn add_vertex_xyz(&mut self, x: f64, y: f64, z: f64) -> i16 {
        self.add_vertex(PolyfaceVertex::from_xyz(x, y, z))
    }

    /// Adds a face to the mesh.
    pub fn add_face(&mut self, face: PolyfaceFace) {
        self.faces.push(face);
    }

    /// Adds a triangle face.
    pub fn add_triangle(&mut self, v1: i16, v2: i16, v3: i16) {
        self.add_face(PolyfaceFace::triangle(v1, v2, v3));
    }

    /// Adds a quad face.
    pub fn add_quad(&mut self, v1: i16, v2: i16, v3: i16, v4: i16) {
        self.add_face(PolyfaceFace::quad(v1, v2, v3, v4));
    }

    /// Returns the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Returns the number of faces.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Gets a vertex by 1-based index.
    pub fn vertex(&self, index: i16) -> Option<&PolyfaceVertex> {
        if index > 0 {
            self.vertices.get((index - 1) as usize)
        } else {
            None
        }
    }

    /// Gets a mutable vertex by 1-based index.
    pub fn vertex_mut(&mut self, index: i16) -> Option<&mut PolyfaceVertex> {
        if index > 0 {
            self.vertices.get_mut((index - 1) as usize)
        } else {
            None
        }
    }

    /// Gets the vertex positions for a face.
    pub fn face_vertices(&self, face: &PolyfaceFace) -> Vec<Vector3> {
        face.vertex_indices()
            .iter()
            .filter_map(|&idx| self.vertex(idx))
            .map(|v| v.location)
            .collect()
    }

    /// Calculates the normal for a face.
    pub fn face_normal(&self, face: &PolyfaceFace) -> Option<Vector3> {
        let verts = self.face_vertices(face);
        if verts.len() < 3 {
            return None;
        }

        let v0 = verts[0];
        let v1 = verts[1];
        let v2 = verts[2];

        let edge1 = v1 - v0;
        let edge2 = v2 - v0;

        let normal = Vector3::new(
            edge1.y * edge2.z - edge1.z * edge2.y,
            edge1.z * edge2.x - edge1.x * edge2.z,
            edge1.x * edge2.y - edge1.y * edge2.x,
        );

        let len = normal.length();
        if len > 1e-10 {
            Some(normal / len)
        } else {
            None
        }
    }

    /// Calculates the area of a face.
    pub fn face_area(&self, face: &PolyfaceFace) -> f64 {
        let verts = self.face_vertices(face);
        if verts.len() < 3 {
            return 0.0;
        }

        if face.is_triangle() {
            let v0 = verts[0];
            let v1 = verts[1];
            let v2 = verts[2];

            let edge1 = v1 - v0;
            let edge2 = v2 - v0;

            let cross = Vector3::new(
                edge1.y * edge2.z - edge1.z * edge2.y,
                edge1.z * edge2.x - edge1.x * edge2.z,
                edge1.x * edge2.y - edge1.y * edge2.x,
            );

            cross.length() / 2.0
        } else {
            // Quad - split into two triangles
            let v0 = verts[0];
            let v1 = verts[1];
            let v2 = verts[2];
            let v3 = verts[3];

            let area1 = {
                let edge1 = v1 - v0;
                let edge2 = v2 - v0;
                let cross = Vector3::new(
                    edge1.y * edge2.z - edge1.z * edge2.y,
                    edge1.z * edge2.x - edge1.x * edge2.z,
                    edge1.x * edge2.y - edge1.y * edge2.x,
                );
                cross.length() / 2.0
            };

            let area2 = {
                let edge1 = v2 - v0;
                let edge2 = v3 - v0;
                let cross = Vector3::new(
                    edge1.y * edge2.z - edge1.z * edge2.y,
                    edge1.z * edge2.x - edge1.x * edge2.z,
                    edge1.x * edge2.y - edge1.y * edge2.x,
                );
                cross.length() / 2.0
            };

            area1 + area2
        }
    }

    /// Returns the total surface area of the mesh.
    pub fn total_area(&self) -> f64 {
        self.faces.iter().map(|f| self.face_area(f)).sum()
    }

    /// Flips all face normals by reversing winding order.
    pub fn flip_normals(&mut self) {
        for face in &mut self.faces {
            face.reverse();
        }
    }

    /// Validates that all face indices are within the vertex range.
    pub fn validate(&self) -> bool {
        let max_index = self.vertices.len() as i16;

        for face in &self.faces {
            for idx in face.vertex_indices() {
                if idx < 1 || idx > max_index {
                    return false;
                }
            }
        }

        true
    }

    /// Creates a simple box mesh.
    pub fn create_box(min: Vector3, max: Vector3) -> Self {
        let mut mesh = Self::new();

        // 8 corners
        mesh.add_vertex_xyz(min.x, min.y, min.z); // 1
        mesh.add_vertex_xyz(max.x, min.y, min.z); // 2
        mesh.add_vertex_xyz(max.x, max.y, min.z); // 3
        mesh.add_vertex_xyz(min.x, max.y, min.z); // 4
        mesh.add_vertex_xyz(min.x, min.y, max.z); // 5
        mesh.add_vertex_xyz(max.x, min.y, max.z); // 6
        mesh.add_vertex_xyz(max.x, max.y, max.z); // 7
        mesh.add_vertex_xyz(min.x, max.y, max.z); // 8

        // 6 faces (quads, outward normals)
        mesh.add_quad(4, 3, 2, 1); // bottom
        mesh.add_quad(5, 6, 7, 8); // top
        mesh.add_quad(1, 2, 6, 5); // front
        mesh.add_quad(3, 4, 8, 7); // back
        mesh.add_quad(2, 3, 7, 6); // right
        mesh.add_quad(4, 1, 5, 8); // left

        mesh
    }

    /// Creates a simple pyramid mesh.
    pub fn create_pyramid(base_center: Vector3, base_size: f64, height: f64) -> Self {
        let mut mesh = Self::new();
        let half = base_size / 2.0;

        // Base vertices
        mesh.add_vertex_xyz(base_center.x - half, base_center.y - half, base_center.z);
        mesh.add_vertex_xyz(base_center.x + half, base_center.y - half, base_center.z);
        mesh.add_vertex_xyz(base_center.x + half, base_center.y + half, base_center.z);
        mesh.add_vertex_xyz(base_center.x - half, base_center.y + half, base_center.z);
        // Apex
        mesh.add_vertex_xyz(base_center.x, base_center.y, base_center.z + height);

        // Faces
        mesh.add_quad(1, 2, 3, 4); // base
        mesh.add_triangle(1, 2, 5); // side 1
        mesh.add_triangle(2, 3, 5); // side 2
        mesh.add_triangle(3, 4, 5); // side 3
        mesh.add_triangle(4, 1, 5); // side 4

        mesh
    }

    /// Triangulates all quad faces.
    pub fn triangulate(&mut self) {
        let mut new_faces = Vec::with_capacity(self.faces.len() * 2);

        for face in &self.faces {
            if face.is_quad() {
                // Split quad into two triangles
                new_faces.push(PolyfaceFace::triangle(
                    face.index1,
                    face.index2,
                    face.index3,
                ));
                new_faces.push(PolyfaceFace::triangle(
                    face.index1,
                    face.index3,
                    face.index4,
                ));
            } else {
                new_faces.push(face.clone());
            }
        }

        self.faces = new_faces;
    }

    /// Returns the number of triangle faces.
    pub fn triangle_count(&self) -> usize {
        self.faces.iter().filter(|f| f.is_triangle()).count()
    }

    /// Returns the number of quad faces.
    pub fn quad_count(&self) -> usize {
        self.faces.iter().filter(|f| f.is_quad()).count()
    }
}

impl Default for PolyfaceMesh {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for PolyfaceMesh {
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
            return BoundingBox3D::default();
        }

        let first = self.vertices[0].location;
        let mut min = first;
        let mut max = first;

        for v in &self.vertices[1..] {
            let loc = v.location;
            min.x = min.x.min(loc.x);
            min.y = min.y.min(loc.y);
            min.z = min.z.min(loc.z);
            max.x = max.x.max(loc.x);
            max.y = max.y.max(loc.y);
            max.z = max.z.max(loc.z);
        }

        BoundingBox3D::new(min, max)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_polyface_mesh(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        Self::ENTITY_NAME
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_polyface_mesh(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_polyface_mesh(self, transform);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mesh_creation() {
        let mesh = PolyfaceMesh::new();
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.face_count(), 0);
        assert!(mesh.flags.contains(PolyfaceMeshFlags::POLYFACE_MESH));
    }

    #[test]
    fn test_add_vertex() {
        let mut mesh = PolyfaceMesh::new();
        let idx = mesh.add_vertex_xyz(1.0, 2.0, 3.0);
        assert_eq!(idx, 1); // 1-based
        assert_eq!(mesh.vertex_count(), 1);

        let v = mesh.vertex(1).unwrap();
        assert_eq!(v.location.x, 1.0);
        assert_eq!(v.location.y, 2.0);
        assert_eq!(v.location.z, 3.0);
    }

    #[test]
    fn test_add_triangle() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.add_vertex_xyz(0.5, 1.0, 0.0);
        mesh.add_triangle(1, 2, 3);

        assert_eq!(mesh.face_count(), 1);
        assert!(mesh.faces[0].is_triangle());
        assert_eq!(mesh.triangle_count(), 1);
        assert_eq!(mesh.quad_count(), 0);
    }

    #[test]
    fn test_add_quad() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 1.0, 0.0);
        mesh.add_vertex_xyz(0.0, 1.0, 0.0);
        mesh.add_quad(1, 2, 3, 4);

        assert_eq!(mesh.face_count(), 1);
        assert!(mesh.faces[0].is_quad());
        assert_eq!(mesh.quad_count(), 1);
    }

    #[test]
    fn test_face_vertex_indices() {
        let face = PolyfaceFace::triangle(1, 2, 3);
        let indices = face.vertex_indices();
        assert_eq!(indices, vec![1, 2, 3]);

        let face = PolyfaceFace::quad(1, 2, 3, 4);
        let indices = face.vertex_indices();
        assert_eq!(indices, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_invisible_edges() {
        let face = PolyfaceFace::triangle_with_visibility(1, 2, 3, true, false, true);
        assert!(face.is_edge1_invisible());
        assert!(!face.is_edge2_invisible());
        assert!(face.is_edge3_invisible());

        // Indices should be absolute in vertex_indices()
        let indices = face.vertex_indices();
        assert_eq!(indices, vec![1, 2, 3]);
    }

    #[test]
    fn test_face_vertices() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.add_vertex_xyz(0.5, 1.0, 0.0);
        mesh.add_triangle(1, 2, 3);

        let verts = mesh.face_vertices(&mesh.faces[0]);
        assert_eq!(verts.len(), 3);
        assert_eq!(verts[0], Vector3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn test_face_normal() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.add_vertex_xyz(0.0, 1.0, 0.0);
        mesh.add_triangle(1, 2, 3);

        let normal = mesh.face_normal(&mesh.faces[0]).unwrap();
        assert!((normal.z - 1.0).abs() < 1e-10 || (normal.z + 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_face_area() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(2.0, 0.0, 0.0);
        mesh.add_vertex_xyz(0.0, 2.0, 0.0);
        mesh.add_triangle(1, 2, 3);

        let area = mesh.face_area(&mesh.faces[0]);
        assert!((area - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_create_box() {
        let mesh = PolyfaceMesh::create_box(Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0));
        assert_eq!(mesh.vertex_count(), 8);
        assert_eq!(mesh.face_count(), 6);
        assert!(mesh.validate());
    }

    #[test]
    fn test_create_pyramid() {
        let mesh = PolyfaceMesh::create_pyramid(Vector3::ZERO, 2.0, 1.0);
        assert_eq!(mesh.vertex_count(), 5);
        assert_eq!(mesh.face_count(), 5);
        assert!(mesh.validate());
    }

    #[test]
    fn test_triangulate() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 1.0, 0.0);
        mesh.add_vertex_xyz(0.0, 1.0, 0.0);
        mesh.add_quad(1, 2, 3, 4);

        assert_eq!(mesh.quad_count(), 1);
        mesh.triangulate();
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.quad_count(), 0);
    }

    #[test]
    fn test_validate() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.add_vertex_xyz(0.5, 1.0, 0.0);
        mesh.add_triangle(1, 2, 3);
        assert!(mesh.validate());

        // Add invalid face
        mesh.add_triangle(1, 2, 10); // Index 10 doesn't exist
        assert!(!mesh.validate());
    }

    #[test]
    fn test_bounding_box() {
        let mesh = PolyfaceMesh::create_box(Vector3::ZERO, Vector3::new(2.0, 3.0, 4.0));
        let bb = mesh.bounding_box();
        assert_eq!(bb.min, Vector3::ZERO);
        assert_eq!(bb.max, Vector3::new(2.0, 3.0, 4.0));
    }

    #[test]
    fn test_translate() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.translate(Vector3::new(10.0, 20.0, 30.0));

        assert_eq!(mesh.vertex(1).unwrap().location.x, 10.0);
        assert_eq!(mesh.vertex(2).unwrap().location.x, 11.0);
    }

    #[test]
    fn test_flip_normals() {
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.add_vertex_xyz(0.5, 1.0, 0.0);
        mesh.add_triangle(1, 2, 3);

        let normal_before = mesh.face_normal(&mesh.faces[0]).unwrap();
        mesh.flip_normals();
        let normal_after = mesh.face_normal(&mesh.faces[0]).unwrap();

        // Normals should be opposite
        assert!((normal_before.z + normal_after.z).abs() < 1e-10);
    }

    #[test]
    fn test_total_area() {
        // Unit square (two triangles make a square with area 1.0)
        let mut mesh = PolyfaceMesh::new();
        mesh.add_vertex_xyz(0.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 0.0, 0.0);
        mesh.add_vertex_xyz(1.0, 1.0, 0.0);
        mesh.add_vertex_xyz(0.0, 1.0, 0.0);
        mesh.add_quad(1, 2, 3, 4);

        let area = mesh.total_area();
        assert!((area - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_face_reverse() {
        let mut face = PolyfaceFace::triangle(1, 2, 3);
        face.reverse();
        assert_eq!(face.index1, 3);
        assert_eq!(face.index2, 2);
        assert_eq!(face.index3, 1);
    }
}
