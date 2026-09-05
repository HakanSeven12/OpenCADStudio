//! Read queries — typed, transport-agnostic reads of entity state (plan §5).
//! The `Query` enum is GENERATED into `crate::gen` by `build.rs`; this module
//! re-exports it and holds the hand-written result DTOs.

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
    pub fn overlaps(&self, other: &Aabb) -> bool {
        (0..3).all(|i| self.min[i] <= other.max[i] && other.min[i] <= self.max[i])
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
        }
    }
}
