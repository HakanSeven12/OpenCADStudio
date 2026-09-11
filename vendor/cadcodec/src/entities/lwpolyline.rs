//! Lightweight polyline entity (2D polyline with bulges)

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector2, Vector3};

/// A vertex in a lightweight polyline
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LwVertex {
    /// Location of the vertex (2D)
    pub location: Vector2,
    /// Bulge value (for arc segments)
    /// 0 = straight line, positive = counterclockwise arc, negative = clockwise arc
    /// bulge = tan(angle/4) where angle is the included angle
    pub bulge: f64,
    /// Starting width at this vertex
    pub start_width: f64,
    /// Ending width at this vertex
    pub end_width: f64,
    /// Stable vertex identifier (DXF 91, R2010+ DWG).
    pub vertex_id: i32,
}

impl LwVertex {
    /// Create a new vertex
    pub fn new(location: Vector2) -> Self {
        LwVertex {
            location,
            bulge: 0.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 0,
        }
    }

    /// Create a vertex from coordinates
    pub fn from_coords(x: f64, y: f64) -> Self {
        LwVertex::new(Vector2::new(x, y))
    }

    /// Create a vertex with a bulge
    pub fn with_bulge(location: Vector2, bulge: f64) -> Self {
        LwVertex {
            location,
            bulge,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 0,
        }
    }
}

/// A lightweight (2D) polyline entity
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LwPolyline {
    /// Common entity data
    pub common: EntityCommon,
    /// Vertices of the polyline
    pub vertices: Vec<LwVertex>,
    /// Is the polyline closed?
    pub is_closed: bool,
    /// Use PLINEGEN linetype generation pattern across vertices
    pub plinegen: bool,
    /// Constant width (if all segments have same width)
    pub constant_width: f64,
    /// Elevation (Z coordinate)
    pub elevation: f64,
    /// Thickness (extrusion in Z direction)
    pub thickness: f64,
    /// Normal vector
    pub normal: Vector3,
}

impl LwPolyline {
    /// Create a new empty lightweight polyline
    pub fn new() -> Self {
        LwPolyline {
            common: EntityCommon::new(),
            vertices: Vec::new(),
            is_closed: false,
            plinegen: false,
            constant_width: 0.0,
            elevation: 0.0,
            thickness: 0.0,
            normal: Vector3::UNIT_Z,
        }
    }

    /// Create a polyline from a list of 2D points
    pub fn from_points(points: Vec<Vector2>) -> Self {
        LwPolyline {
            vertices: points.into_iter().map(LwVertex::new).collect(),
            ..Self::new()
        }
    }

    /// Add a vertex to the polyline
    pub fn add_vertex(&mut self, vertex: LwVertex) {
        self.vertices.push(vertex);
    }

    /// Add a point to the polyline
    pub fn add_point(&mut self, point: Vector2) {
        self.vertices.push(LwVertex::new(point));
    }

    /// Add a point with bulge
    pub fn add_point_with_bulge(&mut self, point: Vector2, bulge: f64) {
        self.vertices.push(LwVertex::with_bulge(point, bulge));
    }

    /// Get the number of vertices
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Close the polyline
    pub fn close(&mut self) {
        self.is_closed = true;
    }
}

impl Default for LwPolyline {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for LwPolyline {
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

        let points: Vec<Vector3> = self
            .vertices
            .iter()
            .map(|v| Vector3::new(v.location.x, v.location.y, self.elevation))
            .collect();
        BoundingBox3D::from_points(&points)
            .unwrap()
            .ocs_to_wcs(self.normal)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_lwpolyline(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "LWPOLYLINE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_lwpolyline(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_lwpolyline(self, transform);
    }
}
