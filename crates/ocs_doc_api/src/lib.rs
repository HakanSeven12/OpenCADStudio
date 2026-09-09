//! `ocs_doc_api` — Object-oriented, transport-agnostic CAD entity API for
//! OpenCADStudio plugins and hosts (v2).
//!
//! Core vocabulary: **Entity**, **ObjectId** (u64), **Operation** (typed write),
//! **Query** (typed read), **Transport** (the only plugin↔host boundary).
//! Every write op is one atomic host call = one undo step (no multi-op write
//! transactions — removed after the security/integrity review; see the plan §13).
//!
//! The facade is object-centric: [`DocApi`] (root) → [`Document`] → collections
//! ([`SolidCollection`], [`CurveCollection`], [`EntityCollection`]) → typed
//! handles ([`Solid`], [`Line`], …) carrying geometric methods. The same typed
//! requests run in-process and over IPC; only the [`Transport`] differs.

pub mod error;
pub mod id;
pub mod revision;

#[cfg(feature = "host")]
pub mod backend;
#[cfg(feature = "host")]
pub mod executor;
#[cfg(feature = "host")]
pub mod geom;
#[cfg(feature = "host")]
pub mod convert;

pub mod envelope;
pub mod facade;
pub mod ops;
pub mod query;
pub mod transport;

// Generated (build.rs) — committed snapshots under src/gen/; do NOT edit by hand.
pub mod gen {
    pub mod ops {
        include!("gen/ops_gen.rs");
    }
    pub mod query {
        include!("gen/query_gen.rs");
    }
    pub use ops::Operation;
    pub use query::Query;
}

pub use error::{ApiError, ApiResult, GeometryErrorKind};
pub use id::ObjectId;
pub use revision::GeometryRevision;

pub use envelope::{DocApiEnvelope, EnvelopeBody, OpOutcome, Receipt};
pub use facade::{
    ArcCurve, Circle, CurveCollection, Dimension, DocApi, Document, Ellipse, Entity,
    EntityCollection, HasId, Line, MText, OpGroup, Point, Polyline, QueryBatch, QueryResults, Ray,
    Solid, SolidCollection, Spline, Text, XLine,
};
pub use ops::{BoolOp, Curve2Spec, EntitySpec, Operation, PlacementSpec, SolidPrimitive};
pub use query::{Aabb, EntityView, Query, QueryResult};

#[cfg(feature = "host")]
pub use convert::{
    bulge_arc_segment, curve_spec_to_entity, entity_bounds, entity_kind_name,
    entity_to_profile_curves, transform_entity_geometry,
};
pub use transport::Transport;

/// The crate's own wire-envelope protocol version (`DocApiEnvelope::version`).
/// Bump when the envelope layout or `Operation`/`Query` discriminants change.
/// Bridges must check this before speaking (see `bindings/README.md`).
pub const ENVELOPE_VERSION: u16 = 1;

/// The generated binding handover schema (plan §10.2): object model + method
/// signatures + op/query mapping with traced wire layouts inlined. Single,
/// self-contained, versioned with [`ENVELOPE_VERSION`].
pub fn binding_schema_json() -> &'static str {
    include_str!("gen/binding_schema.json")
}
