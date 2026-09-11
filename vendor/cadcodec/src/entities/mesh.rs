//! Mesh (SubD mesh) entity implementation.
//!
//! The Mesh entity represents a subdivision surface mesh that can be
//! smoothed at various levels for high-quality curved surface display.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

// ============================================================================
// Mesh Edge
// ============================================================================

/// An edge in a mesh, defined by two vertex indices.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MeshEdge {
    /// Index of the start vertex.
    pub start: usize,
    /// Index of the end vertex.
    pub end: usize,
    /// Edge crease value (affects subdivision smoothing).
    /// None means no crease (fully smooth).
    /// Higher values create sharper edges during subdivision.
    pub crease: Option<f64>,
}

impl MeshEdge {
    /// Creates a new edge between two vertices.
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            crease: None,
        }
    }

    /// Creates a new edge with a crease value.
    pub fn with_crease(start: usize, end: usize, crease: f64) -> Self {
        Self {
            start,
            end,
            crease: Some(crease),
        }
    }

    /// Returns true if this edge has a crease.
    pub fn has_crease(&self) -> bool {
        self.crease.is_some()
    }

    /// Gets the crease value, or 0.0 if no crease.
    pub fn crease_value(&self) -> f64 {
        self.crease.unwrap_or(0.0)
    }

    /// Sets the crease value.
    pub fn set_crease(&mut self, crease: f64) {
        self.crease = Some(crease);
    }

    /// Clears the crease value.
    pub fn clear_crease(&mut self) {
        self.crease = None;
    }

    /// Returns true if this edge connects the given vertices (in either direction).
    pub fn connects(&self, v1: usize, v2: usize) -> bool {
        (self.start == v1 && self.end == v2) || (self.start == v2 && self.end == v1)
    }

    /// Returns the other vertex of this edge.
    pub fn other_vertex(&self, vertex: usize) -> Option<usize> {
        if self.start == vertex {
            Some(self.end)
        } else if self.end == vertex {
            Some(self.start)
        } else {
            None
        }
    }
}

impl Default for MeshEdge {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

// ============================================================================
// Mesh Face
// ============================================================================

/// A face in a mesh, defined by vertex indices.
///
/// Faces can have any number of vertices (3 for triangles, 4 for quads, etc.).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MeshFace {
    /// Indices of vertices that form this face (in order).
    pub vertices: Vec<usize>,
}

impl MeshFace {
    /// Creates a new face from vertex indices.
    pub fn new(vertices: Vec<usize>) -> Self {
        Self { vertices }
    }

    /// Creates a triangular face.
    pub fn triangle(v0: usize, v1: usize, v2: usize) -> Self {
        Self {
            vertices: vec![v0, v1, v2],
        }
    }

    /// Creates a quadrilateral face.
    pub fn quad(v0: usize, v1: usize, v2: usize, v3: usize) -> Self {
        Self {
            vertices: vec![v0, v1, v2, v3],
        }
    }

    /// Returns the number of vertices in this face.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Returns true if this is a triangle.
    pub fn is_triangle(&self) -> bool {
        self.vertices.len() == 3
    }

    /// Returns true if this is a quad.
    pub fn is_quad(&self) -> bool {
        self.vertices.len() == 4
    }

    /// Returns the vertex indices as a slice.
    pub fn indices(&self) -> &[usize] {
        &self.vertices
    }

    /// Returns the edges of this face as pairs of vertex indices.
    pub fn edges(&self) -> Vec<(usize, usize)> {
        if self.vertices.is_empty() {
            return Vec::new();
        }

        let mut edges = Vec::with_capacity(self.vertices.len());
        for i in 0..self.vertices.len() {
            let next = (i + 1) % self.vertices.len();
            edges.push((self.vertices[i], self.vertices[next]));
        }
        edges
    }

    /// Reverses the winding order of the face.
    pub fn reverse(&mut self) {
        self.vertices.reverse();
    }

    /// Returns a reversed copy of this face.
    pub fn reversed(&self) -> Self {
        let mut v = self.vertices.clone();
        v.reverse();
        Self { vertices: v }
    }
}

impl From<Vec<usize>> for MeshFace {
    fn from(vertices: Vec<usize>) -> Self {
        Self::new(vertices)
    }
}

// ============================================================================
// Mesh Entity
// ============================================================================

/// Mesh (SubDMesh) entity.
///
/// A subdivision surface mesh that can be smoothed at various levels.
/// The mesh consists of vertices, edges with optional creases, and
/// faces that can be triangles, quads, or n-gons.
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::{Mesh, MeshFace};
/// use acadrust::types::Vector3;
///
/// // Create a simple cube mesh
/// let mut mesh = Mesh::new();
///
/// // Add vertices (8 corners of a cube)
/// mesh.add_vertex(Vector3::new(0.0, 0.0, 0.0));
/// mesh.add_vertex(Vector3::new(1.0, 0.0, 0.0));
/// mesh.add_vertex(Vector3::new(1.0, 1.0, 0.0));
/// mesh.add_vertex(Vector3::new(0.0, 1.0, 0.0));
/// mesh.add_vertex(Vector3::new(0.0, 0.0, 1.0));
/// mesh.add_vertex(Vector3::new(1.0, 0.0, 1.0));
/// mesh.add_vertex(Vector3::new(1.0, 1.0, 1.0));
/// mesh.add_vertex(Vector3::new(0.0, 1.0, 1.0));
///
/// // Add faces (6 sides of the cube)
/// mesh.add_face(MeshFace::quad(0, 1, 2, 3)); // bottom
/// mesh.add_face(MeshFace::quad(4, 7, 6, 5)); // top
/// mesh.add_face(MeshFace::quad(0, 4, 5, 1)); // front
/// mesh.add_face(MeshFace::quad(2, 6, 7, 3)); // back
/// mesh.add_face(MeshFace::quad(0, 3, 7, 4)); // left
/// mesh.add_face(MeshFace::quad(1, 5, 6, 2)); // right
///
/// // Enable subdivision smoothing
/// mesh.subdivision_level = 2;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mesh {
    /// Common entity data.
    pub common: EntityCommon,
    /// Mesh version (internal use).
    pub version: i16,
    /// Whether creases blend at vertices.
    pub blend_crease: bool,
    /// Number of subdivision levels for smooth display.
    /// 0 = no subdivision (base mesh shown as-is).
    pub subdivision_level: i32,
    /// Mesh vertices.
    pub vertices: Vec<Vector3>,
    /// Mesh faces.
    pub faces: Vec<MeshFace>,
    /// Mesh edges with crease information.
    pub edges: Vec<MeshEdge>,
    /// Trailing mesh override option stored by DWG.
    pub override_option: i32,
}

impl Mesh {
    /// Creates a new empty mesh.
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            version: 2,
            blend_crease: true,
            subdivision_level: 0,
            vertices: Vec::new(),
            faces: Vec::new(),
            edges: Vec::new(),
            override_option: 0,
        }
    }

    /// Creates a mesh from vertices and triangle faces.
    pub fn from_triangles(vertices: Vec<Vector3>, triangles: &[(usize, usize, usize)]) -> Self {
        let mut mesh = Self::new();
        mesh.vertices = vertices;
        for &(v0, v1, v2) in triangles {
            mesh.faces.push(MeshFace::triangle(v0, v1, v2));
        }
        mesh.compute_edges();
        mesh
    }

    /// Creates a mesh from vertices and quad faces.
    pub fn from_quads(vertices: Vec<Vector3>, quads: &[(usize, usize, usize, usize)]) -> Self {
        let mut mesh = Self::new();
        mesh.vertices = vertices;
        for &(v0, v1, v2, v3) in quads {
            mesh.faces.push(MeshFace::quad(v0, v1, v2, v3));
        }
        mesh.compute_edges();
        mesh
    }

    /// Adds a vertex and returns its index.
    pub fn add_vertex(&mut self, vertex: Vector3) -> usize {
        let index = self.vertices.len();
        self.vertices.push(vertex);
        index
    }

    /// Adds multiple vertices and returns the starting index.
    pub fn add_vertices(&mut self, vertices: &[Vector3]) -> usize {
        let start = self.vertices.len();
        self.vertices.extend_from_slice(vertices);
        start
    }

    /// Adds a face.
    pub fn add_face(&mut self, face: MeshFace) {
        self.faces.push(face);
    }

    /// Adds a triangular face.
    pub fn add_triangle(&mut self, v0: usize, v1: usize, v2: usize) {
        self.faces.push(MeshFace::triangle(v0, v1, v2));
    }

    /// Adds a quad face.
    pub fn add_quad(&mut self, v0: usize, v1: usize, v2: usize, v3: usize) {
        self.faces.push(MeshFace::quad(v0, v1, v2, v3));
    }

    /// Adds an edge.
    pub fn add_edge(&mut self, edge: MeshEdge) {
        self.edges.push(edge);
    }

    /// Adds an edge between two vertices.
    pub fn add_edge_between(&mut self, start: usize, end: usize) -> usize {
        let index = self.edges.len();
        self.edges.push(MeshEdge::new(start, end));
        index
    }

    /// Adds an edge with a crease value.
    pub fn add_creased_edge(&mut self, start: usize, end: usize, crease: f64) -> usize {
        let index = self.edges.len();
        self.edges.push(MeshEdge::with_crease(start, end, crease));
        index
    }

    /// Computes edges from faces.
    /// This creates an edge for each unique edge in the face list.
    pub fn compute_edges(&mut self) {
        use std::collections::HashSet;

        let mut edge_set: HashSet<(usize, usize)> = HashSet::new();

        for face in &self.faces {
            for &(v1, v2) in &face.edges() {
                // Normalize edge direction to avoid duplicates
                let key = if v1 < v2 { (v1, v2) } else { (v2, v1) };
                edge_set.insert(key);
            }
        }

        self.edges.clear();
        for (start, end) in edge_set {
            self.edges.push(MeshEdge::new(start, end));
        }
    }

    /// Returns the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Returns the number of faces.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Returns the number of edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Returns the total number of triangles if all faces were triangulated.
    pub fn triangle_count(&self) -> usize {
        self.faces
            .iter()
            .map(|f| {
                if f.vertex_count() < 3 {
                    0
                } else {
                    f.vertex_count() - 2
                }
            })
            .sum()
    }

    /// Returns true if all faces are triangles.
    pub fn is_all_triangles(&self) -> bool {
        self.faces.iter().all(|f| f.is_triangle())
    }

    /// Returns true if all faces are quads.
    pub fn is_all_quads(&self) -> bool {
        self.faces.iter().all(|f| f.is_quad())
    }

    /// Finds an edge between two vertices.
    pub fn find_edge(&self, v1: usize, v2: usize) -> Option<usize> {
        self.edges.iter().position(|e| e.connects(v1, v2))
    }

    /// Gets a vertex by index.
    pub fn vertex(&self, index: usize) -> Option<Vector3> {
        self.vertices.get(index).copied()
    }

    /// Gets a face by index.
    pub fn face(&self, index: usize) -> Option<&MeshFace> {
        self.faces.get(index)
    }

    /// Gets an edge by index.
    pub fn edge(&self, index: usize) -> Option<&MeshEdge> {
        self.edges.get(index)
    }

    /// Sets a crease value on an edge.
    pub fn set_edge_crease(&mut self, edge_index: usize, crease: f64) -> bool {
        if let Some(edge) = self.edges.get_mut(edge_index) {
            edge.set_crease(crease);
            true
        } else {
            false
        }
    }

    /// Clears all creases.
    pub fn clear_creases(&mut self) {
        for edge in &mut self.edges {
            edge.clear_crease();
        }
    }

    /// Returns true if any edge has a crease.
    pub fn has_creases(&self) -> bool {
        self.edges.iter().any(|e| e.has_crease())
    }

    /// Translates all vertices by the given offset.
    pub fn translate(&mut self, offset: Vector3) {
        for vertex in &mut self.vertices {
            *vertex = *vertex + offset;
        }
    }

    /// Scales all vertices relative to a center point.
    pub fn scale(&mut self, center: Vector3, scale: f64) {
        for vertex in &mut self.vertices {
            *vertex = center + (*vertex - center) * scale;
        }
    }

    /// Scales all vertices relative to the origin.
    pub fn scale_uniform(&mut self, scale: f64) {
        for vertex in &mut self.vertices {
            *vertex = *vertex * scale;
        }
    }

    /// Returns the bounding box of the mesh.
    pub fn bounding_box(&self) -> Option<(Vector3, Vector3)> {
        if self.vertices.is_empty() {
            return None;
        }

        let first = self.vertices[0];
        let mut min = first;
        let mut max = first;

        for v in &self.vertices[1..] {
            min.x = min.x.min(v.x);
            min.y = min.y.min(v.y);
            min.z = min.z.min(v.z);
            max.x = max.x.max(v.x);
            max.y = max.y.max(v.y);
            max.z = max.z.max(v.z);
        }

        Some((min, max))
    }

    /// Calculates the center of the bounding box.
    pub fn center(&self) -> Option<Vector3> {
        self.bounding_box().map(|(min, max)| (min + max) * 0.5)
    }

    /// Reverses all face winding orders.
    pub fn flip_normals(&mut self) {
        for face in &mut self.faces {
            face.reverse();
        }
    }

    /// Creates a box mesh.
    pub fn create_box(min: Vector3, max: Vector3) -> Self {
        let vertices = vec![
            Vector3::new(min.x, min.y, min.z), // 0
            Vector3::new(max.x, min.y, min.z), // 1
            Vector3::new(max.x, max.y, min.z), // 2
            Vector3::new(min.x, max.y, min.z), // 3
            Vector3::new(min.x, min.y, max.z), // 4
            Vector3::new(max.x, min.y, max.z), // 5
            Vector3::new(max.x, max.y, max.z), // 6
            Vector3::new(min.x, max.y, max.z), // 7
        ];

        let quads = vec![
            (0, 3, 2, 1), // bottom (-Z)
            (4, 5, 6, 7), // top (+Z)
            (0, 1, 5, 4), // front (-Y)
            (2, 3, 7, 6), // back (+Y)
            (0, 4, 7, 3), // left (-X)
            (1, 2, 6, 5), // right (+X)
        ];

        Self::from_quads(vertices, &quads)
    }

    /// Creates a unit cube centered at origin.
    pub fn create_unit_cube() -> Self {
        Self::create_box(Vector3::new(-0.5, -0.5, -0.5), Vector3::new(0.5, 0.5, 0.5))
    }

    /// Creates a simple plane mesh.
    pub fn create_plane(
        origin: Vector3,
        u_axis: Vector3,
        v_axis: Vector3,
        u_segments: usize,
        v_segments: usize,
    ) -> Self {
        let mut mesh = Self::new();

        // Create vertices
        for v in 0..=v_segments {
            for u in 0..=u_segments {
                let u_ratio = u as f64 / u_segments as f64;
                let v_ratio = v as f64 / v_segments as f64;
                let pos = origin + u_axis * u_ratio + v_axis * v_ratio;
                mesh.add_vertex(pos);
            }
        }

        // Create faces
        let cols = u_segments + 1;
        for v in 0..v_segments {
            for u in 0..u_segments {
                let v0 = v * cols + u;
                let v1 = v0 + 1;
                let v2 = v0 + cols + 1;
                let v3 = v0 + cols;
                mesh.add_quad(v0, v1, v2, v3);
            }
        }

        mesh.compute_edges();
        mesh
    }

    /// Creates a cylinder mesh.
    pub fn create_cylinder(
        base_center: Vector3,
        radius: f64,
        height: f64,
        radial_segments: usize,
        height_segments: usize,
        cap_top: bool,
        cap_bottom: bool,
    ) -> Self {
        use std::f64::consts::PI;

        let mut mesh = Self::new();

        // Generate vertices
        for h in 0..=height_segments {
            let y = (h as f64 / height_segments as f64) * height;
            for r in 0..radial_segments {
                let angle = (r as f64 / radial_segments as f64) * 2.0 * PI;
                let x = radius * angle.cos();
                let z = radius * angle.sin();
                mesh.add_vertex(base_center + Vector3::new(x, y, z));
            }
        }

        // Generate side faces
        for h in 0..height_segments {
            for r in 0..radial_segments {
                let next_r = (r + 1) % radial_segments;
                let v0 = h * radial_segments + r;
                let v1 = h * radial_segments + next_r;
                let v2 = (h + 1) * radial_segments + next_r;
                let v3 = (h + 1) * radial_segments + r;
                mesh.add_quad(v0, v1, v2, v3);
            }
        }

        // Add caps
        if cap_bottom {
            let center_idx = mesh.add_vertex(base_center);
            let base_start = 0;
            for r in 0..radial_segments {
                let next_r = (r + 1) % radial_segments;
                mesh.add_triangle(center_idx, base_start + next_r, base_start + r);
            }
        }

        if cap_top {
            let center_idx = mesh.add_vertex(base_center + Vector3::new(0.0, height, 0.0));
            let top_start = height_segments * radial_segments;
            for r in 0..radial_segments {
                let next_r = (r + 1) % radial_segments;
                mesh.add_triangle(center_idx, top_start + r, top_start + next_r);
            }
        }

        mesh.compute_edges();
        mesh
    }

    /// Merges another mesh into this one.
    pub fn merge(&mut self, other: &Mesh) {
        let vertex_offset = self.vertices.len();

        // Add vertices
        self.vertices.extend(&other.vertices);

        // Add faces with offset indices
        for face in &other.faces {
            let new_vertices: Vec<usize> =
                face.vertices.iter().map(|v| v + vertex_offset).collect();
            self.faces.push(MeshFace::new(new_vertices));
        }

        // Add edges with offset indices
        for edge in &other.edges {
            self.edges.push(MeshEdge {
                start: edge.start + vertex_offset,
                end: edge.end + vertex_offset,
                crease: edge.crease,
            });
        }
    }
}

impl Default for Mesh {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Mesh {
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

    fn set_line_weight(&mut self, line_weight: LineWeight) {
        self.common.line_weight = line_weight;
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

        let first = self.vertices[0];
        let mut min = first;
        let mut max = first;

        for v in &self.vertices[1..] {
            min.x = min.x.min(v.x);
            min.y = min.y.min(v.y);
            min.z = min.z.min(v.z);
            max.x = max.x.max(v.x);
            max.y = max.y.max(v.y);
            max.z = max.z.max(v.z);
        }

        BoundingBox3D::new(min, max)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_mesh(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "MESH"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_mesh(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_mesh(self, transform);
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for Mesh entities.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MeshBuilder {
    mesh: Mesh,
}

impl MeshBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self { mesh: Mesh::new() }
    }

    /// Adds a vertex.
    pub fn vertex(mut self, vertex: Vector3) -> Self {
        self.mesh.add_vertex(vertex);
        self
    }

    /// Adds multiple vertices.
    pub fn vertices(mut self, vertices: &[Vector3]) -> Self {
        self.mesh.add_vertices(vertices);
        self
    }

    /// Adds a triangle face.
    pub fn triangle(mut self, v0: usize, v1: usize, v2: usize) -> Self {
        self.mesh.add_triangle(v0, v1, v2);
        self
    }

    /// Adds a quad face.
    pub fn quad(mut self, v0: usize, v1: usize, v2: usize, v3: usize) -> Self {
        self.mesh.add_quad(v0, v1, v2, v3);
        self
    }

    /// Adds a face.
    pub fn face(mut self, vertices: Vec<usize>) -> Self {
        self.mesh.add_face(MeshFace::new(vertices));
        self
    }

    /// Sets the subdivision level.
    pub fn subdivision_level(mut self, level: i32) -> Self {
        self.mesh.subdivision_level = level;
        self
    }

    /// Sets blend crease.
    pub fn blend_crease(mut self, blend: bool) -> Self {
        self.mesh.blend_crease = blend;
        self
    }

    /// Computes edges from faces.
    pub fn compute_edges(mut self) -> Self {
        self.mesh.compute_edges();
        self
    }

    /// Builds the mesh.
    pub fn build(self) -> Mesh {
        self.mesh
    }
}

impl Default for MeshBuilder {
    fn default() -> Self {
        Self::new()
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
        let mesh = Mesh::new();
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.face_count(), 0);
        assert_eq!(mesh.edge_count(), 0);
        assert_eq!(mesh.subdivision_level, 0);
        assert!(mesh.blend_crease);
    }

    #[test]
    fn test_mesh_add_vertex() {
        let mut mesh = Mesh::new();
        let idx = mesh.add_vertex(Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(idx, 0);
        assert_eq!(mesh.vertex_count(), 1);
        assert_eq!(mesh.vertex(0), Some(Vector3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn test_mesh_add_face() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vector3::ZERO);
        mesh.add_vertex(Vector3::new(1.0, 0.0, 0.0));
        mesh.add_vertex(Vector3::new(0.0, 1.0, 0.0));

        mesh.add_triangle(0, 1, 2);

        assert_eq!(mesh.face_count(), 1);
        assert!(mesh.face(0).unwrap().is_triangle());
    }

    #[test]
    fn test_mesh_face() {
        let face = MeshFace::quad(0, 1, 2, 3);
        assert!(face.is_quad());
        assert!(!face.is_triangle());
        assert_eq!(face.vertex_count(), 4);

        let edges = face.edges();
        assert_eq!(edges.len(), 4);
        assert_eq!(edges[0], (0, 1));
        assert_eq!(edges[1], (1, 2));
        assert_eq!(edges[2], (2, 3));
        assert_eq!(edges[3], (3, 0));
    }

    #[test]
    fn test_mesh_face_reverse() {
        let mut face = MeshFace::quad(0, 1, 2, 3);
        face.reverse();
        assert_eq!(face.vertices, vec![3, 2, 1, 0]);
    }

    #[test]
    fn test_mesh_edge() {
        let mut edge = MeshEdge::new(0, 5);
        assert_eq!(edge.start, 0);
        assert_eq!(edge.end, 5);
        assert!(!edge.has_crease());
        assert_eq!(edge.crease_value(), 0.0);

        edge.set_crease(2.5);
        assert!(edge.has_crease());
        assert_eq!(edge.crease_value(), 2.5);
    }

    #[test]
    fn test_mesh_edge_connects() {
        let edge = MeshEdge::new(0, 5);
        assert!(edge.connects(0, 5));
        assert!(edge.connects(5, 0));
        assert!(!edge.connects(0, 3));
    }

    #[test]
    fn test_mesh_edge_other_vertex() {
        let edge = MeshEdge::new(0, 5);
        assert_eq!(edge.other_vertex(0), Some(5));
        assert_eq!(edge.other_vertex(5), Some(0));
        assert_eq!(edge.other_vertex(3), None);
    }

    #[test]
    fn test_mesh_compute_edges() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vector3::ZERO);
        mesh.add_vertex(Vector3::new(1.0, 0.0, 0.0));
        mesh.add_vertex(Vector3::new(1.0, 1.0, 0.0));
        mesh.add_vertex(Vector3::new(0.0, 1.0, 0.0));

        mesh.add_quad(0, 1, 2, 3);
        mesh.compute_edges();

        assert_eq!(mesh.edge_count(), 4);
    }

    #[test]
    fn test_mesh_from_triangles() {
        let vertices = vec![
            Vector3::ZERO,
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.5, 1.0, 0.0),
        ];
        let mesh = Mesh::from_triangles(vertices, &[(0, 1, 2)]);

        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.face_count(), 1);
        assert_eq!(mesh.edge_count(), 3);
        assert!(mesh.is_all_triangles());
    }

    #[test]
    fn test_mesh_create_box() {
        let mesh = Mesh::create_box(Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0));

        assert_eq!(mesh.vertex_count(), 8);
        assert_eq!(mesh.face_count(), 6);
        assert!(mesh.is_all_quads());
    }

    #[test]
    fn test_mesh_unit_cube() {
        let mesh = Mesh::create_unit_cube();
        let bbox = mesh.bounding_box().unwrap();

        assert_eq!(bbox.0, Vector3::new(-0.5, -0.5, -0.5));
        assert_eq!(bbox.1, Vector3::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn test_mesh_translate() {
        let mut mesh = Mesh::create_unit_cube();
        mesh.translate(Vector3::new(1.0, 2.0, 3.0));

        let center = mesh.center().unwrap();
        assert!((center.x - 1.0).abs() < 1e-10);
        assert!((center.y - 2.0).abs() < 1e-10);
        assert!((center.z - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_mesh_scale() {
        let mut mesh = Mesh::create_unit_cube();
        mesh.scale_uniform(2.0);

        let bbox = mesh.bounding_box().unwrap();
        assert_eq!(bbox.0, Vector3::new(-1.0, -1.0, -1.0));
        assert_eq!(bbox.1, Vector3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn test_mesh_triangle_count() {
        let mut mesh = Mesh::new();
        mesh.add_face(MeshFace::triangle(0, 1, 2));
        mesh.add_face(MeshFace::quad(3, 4, 5, 6));
        mesh.add_face(MeshFace::new(vec![7, 8, 9, 10, 11])); // 5-gon

        // 1 + 2 + 3 = 6 triangles
        assert_eq!(mesh.triangle_count(), 6);
    }

    #[test]
    fn test_mesh_find_edge() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vector3::ZERO);
        mesh.add_vertex(Vector3::new(1.0, 0.0, 0.0));
        mesh.add_vertex(Vector3::new(0.0, 1.0, 0.0));
        mesh.add_triangle(0, 1, 2);
        mesh.compute_edges();

        assert!(mesh.find_edge(0, 1).is_some());
        assert!(mesh.find_edge(1, 0).is_some()); // Works in reverse
        assert!(mesh.find_edge(0, 5).is_none());
    }

    #[test]
    fn test_mesh_creases() {
        let mut mesh = Mesh::create_unit_cube();
        mesh.compute_edges();

        assert!(!mesh.has_creases());

        mesh.set_edge_crease(0, 2.0);
        assert!(mesh.has_creases());

        mesh.clear_creases();
        assert!(!mesh.has_creases());
    }

    #[test]
    fn test_mesh_flip_normals() {
        let mut mesh = Mesh::new();
        mesh.add_face(MeshFace::triangle(0, 1, 2));

        mesh.flip_normals();

        assert_eq!(mesh.face(0).unwrap().vertices, vec![2, 1, 0]);
    }

    #[test]
    fn test_mesh_merge() {
        let mesh1 = Mesh::create_unit_cube();
        let mut mesh2 = Mesh::create_unit_cube();
        mesh2.translate(Vector3::new(2.0, 0.0, 0.0));

        let mut merged = mesh1.clone();
        merged.merge(&mesh2);

        assert_eq!(merged.vertex_count(), 16);
        assert_eq!(merged.face_count(), 12);
    }

    #[test]
    fn test_mesh_create_plane() {
        let mesh = Mesh::create_plane(
            Vector3::ZERO,
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(0.0, 10.0, 0.0),
            2,
            2,
        );

        assert_eq!(mesh.vertex_count(), 9); // 3x3 grid
        assert_eq!(mesh.face_count(), 4); // 2x2 quads
    }

    #[test]
    fn test_mesh_builder() {
        let mesh = MeshBuilder::new()
            .vertex(Vector3::ZERO)
            .vertex(Vector3::new(1.0, 0.0, 0.0))
            .vertex(Vector3::new(0.5, 1.0, 0.0))
            .triangle(0, 1, 2)
            .subdivision_level(2)
            .blend_crease(false)
            .compute_edges()
            .build();

        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.face_count(), 1);
        assert_eq!(mesh.subdivision_level, 2);
        assert!(!mesh.blend_crease);
    }
}
