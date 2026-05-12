// acadrust -> truck topology conversion layer.

use acadrust::{CadDocument, EntityType};
use glam::Vec3;
use truck_modeling::{Edge, Solid, Vertex, Wire};

use crate::entities::traits::EntityTypeOps;
use crate::scene::wire_model::{SnapHint, TangentGeom};

/// One group of glyph strokes with its world-space origin stored in f64.
/// Strokes are in glyph-local space (origin = [0,0]) so that the large
/// world offset can be subtracted with f64 precision in tessellate.rs.
pub struct TextStroke {
    pub strokes: Vec<Vec<[f32; 2]>>,
    pub origin: [f64; 2],
}

#[allow(dead_code)]
pub enum TruckObject {
    Point(Vertex),
    Curve(Edge),
    Contour(Wire),
    Text(Vec<TextStroke>),
    /// Pre-computed NaN-separated 3-D point list (leader lines, arrowheads, etc.).
    /// Points are stored in WCS as **f64** so the large world_offset can be
    /// subtracted in full precision in tessellate.rs before the f32 cast.
    /// Casting WCS coordinates to f32 in the entity converters used to wreck
    /// rotated sub-glyph precision on drawings far from origin.
    Lines(Vec<[f64; 3]>),
    /// Like Lines but linetype pattern restarts at each NaN-separated segment (plinegen=false).
    SegmentedLines(Vec<[f64; 3]>),
    Volume(Solid),
}

pub struct TruckEntity {
    pub object: TruckObject,
    pub snap_pts: Vec<(Vec3, SnapHint)>,
    pub tangent_geoms: Vec<TangentGeom>,
    /// Polyline vertex positions in WCS f64; converted to offset-relative f32
    /// at the wire-model boundary.
    pub key_vertices: Vec<[f64; 3]>,
    /// Pre-triangulated fill geometry: flat list of WCS f64 vertices, 3 per
    /// triangle. Non-empty for mesh-like entities (PolyfaceMesh, PolygonMesh)
    /// that need solid fill.
    pub fill_tris: Vec<[f64; 3]>,
}

pub fn convert(entity: &EntityType, document: &CadDocument) -> Option<TruckEntity> {
    entity.to_truck_entity(document)
}
