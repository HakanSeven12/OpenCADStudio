//! Read queries — typed, transport-agnostic reads of entity state.
//! The `Query` enum is **hand-maintained** (append-only) in `src/gen/query_gen.rs`
//! — NOT derived from the spec by build.rs (the spec's `query` names must map to
//! a variant here, asserted by a test). This module re-exports it and holds the
//! hand-written result DTOs.

use serde::{Deserialize, Serialize};

use crate::id::ObjectId;
use crate::revision::GeometryRevision;

pub use crate::gen::Query;

/// Axis-aligned bounding box (plain-data mirror of the kernel's non-serde `Aabb`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Aabb {
    #[cfg(feature = "host")]
    pub fn overlaps(&self, other: &Aabb) -> bool {
        cadkernel::brep::Aabb {
            min: self.min,
            max: self.max,
        }
        .overlaps(&cadkernel::brep::Aabb {
            min: other.min,
            max: other.max,
        })
    }
}

/// A generic, untyped view of an entity returned by [`Query::GetEntity`].
/// Family-specific data lives in the typed construction specs; this carries
/// identity + kind + a coarse bounding box so any first-level entity is readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityView {
    pub id: ObjectId,
    /// The acadrust `EntityType` variant name (e.g. "Solid3D", "Line").
    pub kind: String,
    pub bounds: Option<Aabb>,
}

/// The result of a single [`Query`] (one per query in a `Queries` batch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueryResult {
    Entity(EntityView),
    Bounds(Aabb),
    Centroid([f64; 3]),
    Volume(f64),
    /// Whether two entities' bounds overlap (two-phase conditional modelling).
    Intersects(bool),
    Revision(GeometryRevision),
    /// The text content of a Text/MText annotation.
    TextContent(String),
    /// A hatch's boundary loops (outer + islands), each a closed polyline.
    HatchBoundary(Vec<Vec<[f64; 2]>>),
    /// A dimension's measured value (distance for linear/radius, degrees for angular).
    DimensionMeasurement(f64),
    /// An insert's attributes as (tag, value) pairs.
    Attributes(Vec<(String, String)>),
    /// The entities inside a block definition (read-only traversal).
    BlockEntities(Vec<EntityView>),
    /// A viewport's view: target point (WCS) + zoom height.
    ViewportView {
        target: [f64; 3],
        height: f64,
    },
}

/// Convenience: the query name for diagnostics.
impl crate::gen::Query {
    pub fn query_name(&self) -> &'static str {
        use crate::gen::Query::*;
        match self {
            GetEntity { .. } => "GetEntity",
            GetBounds { .. } => "GetBounds",
            GetCentroid { .. } => "GetCentroid",
            GetVolume { .. } => "GetVolume",
            GetIntersects { .. } => "GetIntersects",
            GetGeometryRevision => "GetGeometryRevision",
            GetTextContent { .. } => "GetTextContent",
            GetHatchBoundary { .. } => "GetHatchBoundary",
            GetDimensionMeasurement { .. } => "GetDimensionMeasurement",
            GetAttributes { .. } => "GetAttributes",
            GetBlockEntities { .. } => "GetBlockEntities",
            GetViewportView { .. } => "GetViewportView",
        }
    }
}
