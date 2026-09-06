//! The thin host hook (plan §6, decision #11). The host implements this trait;
//! the crate's [`crate::executor`] drives it. Both the in-process transport and
//! the host's IPC executor call into it. Logic lives in the versioned crate; the
//! host only provides scene primitives.

use crate::error::{ApiError, ApiResult};
use crate::id::ObjectId;
use crate::ops::Curve2Spec;
use crate::query::{Aabb, EntityView};
use crate::revision::GeometryRevision;

/// A kernel B-rep solid body, opaque to the wire. On `host` this is
/// `cadkernel::brep::Body`; kernel keys never cross the API (decision #7).
pub type KernelBody = cadkernel::brep::Body;

/// The per-op host hook. Every method maps to an already-present scene primitive.
/// The executor calls `push_undo` BEFORE applying a write op and `finalize_op`
/// AFTER — recording exactly one undo step + one geometry bump per applied op.
pub trait DocApiBackend {
    // ── identity / solid lookup ────────────────────────────────────────────
    /// Resolve a live B-rep body for `id` (solid cache; re-lift from AcisData on
    /// miss). `UnknownId` if `id` is not a live solid.
    fn resolve_body(&mut self, id: ObjectId) -> ApiResult<KernelBody>;

    /// Store a new solid body as a `Solid3D` entity; returns the fresh `ObjectId`.
    /// Mirrors host `add_solid_model` (edge wires + `solid_to_sat` +
    /// `set_sat_document` + `register_solid_model`).
    fn store_solid(&mut self, body: &KernelBody) -> ApiResult<ObjectId>;

    /// Replace the body of an existing solid in place (same `ObjectId`).
    /// Re-embeds SAT and refreshes the solid cache.
    fn update_solid(&mut self, id: ObjectId, body: &KernelBody) -> ApiResult<()>;

    // ── plain (2D) entities ────────────────────────────────────────────────
    /// Add a 2D curve entity; returns the fresh `ObjectId`.
    fn add_curve(&mut self, spec: &Curve2Spec) -> ApiResult<ObjectId>;

    /// Add a block reference (`INSERT`). Validates `block_name` exists; returns
    /// the fresh `ObjectId`. `Validation` if the block is unknown.
    fn add_insert(&mut self, spec: &crate::ops::InsertSpec) -> ApiResult<ObjectId>;

    /// Add a paper-space `VIEWPORT`; returns the fresh `ObjectId`.
    fn add_viewport(&mut self, spec: &crate::ops::ViewportSpec) -> ApiResult<ObjectId>;

    /// Add a single-line `TEXT` annotation; returns the fresh `ObjectId`.
    fn add_text(&mut self, spec: &crate::ops::TextSpec) -> ApiResult<ObjectId>;

    /// Add a multi-line `MTEXT` annotation; returns the fresh `ObjectId`.
    fn add_mtext(&mut self, spec: &crate::ops::MTextSpec) -> ApiResult<ObjectId>;

    /// Set the text content of a Text/MText annotation in place.
    fn set_text_content(&mut self, id: ObjectId, value: &str) -> ApiResult<()>;

    /// The text content of a Text/MText annotation. `Unsupported` for other kinds.
    fn text_content(&self, id: ObjectId) -> ApiResult<String>;

    /// Can `id` be modified in place right now (exists, is the expected family,
    /// not on a locked layer)? Read-only pre-check used before mutations.
    /// Default: existence + not-locked (backends narrow the family check).
    fn can_modify(&self, id: ObjectId) -> ApiResult<()> {
        if self.entity_exists(id) {
            Ok(())
        } else {
            Err(ApiError::UnknownId(id))
        }
    }

    /// Remove any first-level entity by id. Returns `false` if absent; returns
    /// `Err` if the entity exists but cannot be removed (e.g. locked layer).
    fn remove_entity(&mut self, id: ObjectId) -> ApiResult<bool>;

    /// Can `id` be removed right now (exists and is not blocked, e.g. by a locked
    /// layer)? Read-only; used by the boolean-erase path to validate BEFORE any
    /// mutation so a mid-op failure cannot leave partial state. Default: existence-only.
    fn can_remove(&self, id: ObjectId) -> ApiResult<()> {
        if self.entity_exists(id) {
            Ok(())
        } else {
            Err(ApiError::UnknownId(id))
        }
    }

    /// A generic, untyped view of any entity (id + kind + coarse bounds).
    fn get_entity(&mut self, id: ObjectId) -> ApiResult<EntityView>;

    /// The 2D curve geometry of a profile entity (polyline/closed curve) for
    /// sweep ops (`Extrude`/`Revolve`). `UnknownId`/`Unsupported` if not a profile.
    fn profile_curves(&self, id: ObjectId) -> ApiResult<Vec<cadkernel::geom2d::Curve>>;

    /// Apply a rigid similarity to any entity in place (same `ObjectId`).
    fn transform_entity(&mut self, id: ObjectId, placement: &crate::ops::PlacementSpec) -> ApiResult<()>;

    /// Validate that `id` exists and can be transformed, WITHOUT mutating.
    /// Used by `TransformMany` to pre-validate all ids before `push_undo` so the
    /// apply loop cannot fail part-way (all-or-nothing, plan §5.3). Default:
    /// existence-only (backends that restrict transform to a family override).
    fn ensure_transformable(&mut self, id: ObjectId) -> ApiResult<()> {
        if self.entity_exists(id) {
            Ok(())
        } else {
            Err(ApiError::UnknownId(id))
        }
    }

    /// Insert a vertex into a polyline at index `at` (polyline family only).
    fn add_vertex(&mut self, id: ObjectId, at: usize, point: [f64; 3]) -> ApiResult<()>;

    // ── queries (no undo, no bump; `&mut` because solid resolution may
    //    populate the kernel body cache on miss) ─────────────────────────────
    fn bounds(&mut self, id: ObjectId) -> ApiResult<Aabb>;
    fn centroid(&mut self, id: ObjectId) -> ApiResult<[f64; 3]>;
    fn volume(&mut self, id: ObjectId) -> ApiResult<f64>;
    fn entity_exists(&self, id: ObjectId) -> bool;
    fn revision(&self) -> GeometryRevision;

    // ── per-op boundary ────────────────────────────────────────────────────
    /// Called BEFORE applying a write op (records the before-state undo snapshot).
    fn push_undo(&mut self, label: &str);
    /// Called AFTER a successful write op (one DeltaSnapshot + one geometry bump
    /// + one publish). NOT called on failure (a failed op records nothing).
    fn finalize_op(&mut self);
}
