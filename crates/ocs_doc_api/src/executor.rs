//! The op executor (plan §6, decision #11): crate-owned logic mapping each
//! `Operation`/`Query` to `DocApiBackend` calls. Both the in-process transport
//! and the host's IPC executor use this — one implementation, versioned in the
//! crate. Per-op atomicity: a write op validates its inputs and computes its
//! geometry BEFORE calling `push_undo`, mutates, then `finalize_op` once.

use crate::backend::DocApiBackend;
use crate::envelope::{OpOutcome, Receipt};
use crate::error::{ApiError, ApiResult};
use crate::gen::{Operation, Query};
use crate::id::ObjectId;
use crate::ops::{BoolOp, EntitySpec, BULK_ITEM_CAP};
use crate::query::QueryResult;

/// Apply ONE write op atomically. On success: one undo step + one revision bump.
/// On failure: nothing applied, no undo, no bump (the op validates before mutating).
pub fn apply_op<B: DocApiBackend>(b: &mut B, op: Operation) -> ApiResult<Receipt> {
    let name = op.op_name();
    let outcome = match &op {
        Operation::CreateSolid(prim) => {
            let body = crate::geom::make_solid(prim)?;
            b.push_undo(name);
            let id = b.store_solid(&body)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
        Operation::CreateCurve(spec) => {
            b.push_undo(name);
            let id = b.add_curve(spec)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
        Operation::Extrude { profile, direction } => {
            let curves = profile_curves(b, *profile, name)?;
            let body = crate::geom::extrude(&curves, *direction)?;
            b.push_undo(name);
            let id = b.store_solid(&body)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
        Operation::Revolve { profile, axis, angle } => {
            let curves = profile_curves(b, *profile, name)?;
            let (pivot, axis_dir) = *axis;
            let body = crate::geom::revolve(&curves, pivot, axis_dir, *angle)?;
            b.push_undo(name);
            let id = b.store_solid(&body)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
        Operation::Transform { id, placement } => {
            require_exists(b, *id, name)?;
            b.push_undo(name);
            b.transform_entity(*id, placement)?;
            b.finalize_op();
            OpOutcome::Updated(*id)
        }
        Operation::SolidBoolean { op: bop, a, b: bid, erase_sources } => {
            // Resolve + combine BEFORE push_undo: pure computation, no mutation.
            let ba = b.resolve_body(*a).map_err(|_| ApiError::UnknownId(*a))?;
            let bb = b.resolve_body(*bid).map_err(|_| ApiError::UnknownId(*bid))?;
            let result = crate::geom::boolean(&ba, &bb, *bop)?;
            // Validate the erase BEFORE mutating: if `b` cannot be removed (e.g.
            // locked layer), fail now rather than after `update_solid(a)` has
            // already committed a partial mutation (plan review: erase path).
            if *erase_sources {
                b.can_remove(*bid)?;
            }
            b.push_undo(name);
            // `erase_sources`: the result lives at `a` (updated in place), `b` erased.
            // Otherwise store a fresh solid and leave both inputs untouched.
            let out_id = if *erase_sources {
                b.update_solid(*a, &result)?;
                b.remove_entity(*bid)?;
                *a
            } else {
                b.store_solid(&result)?
            };
            b.finalize_op();
            OpOutcome::NewId(out_id)
        }
        Operation::AddVertex { id, at, point } => {
            require_exists(b, *id, name)?;
            b.push_undo(name);
            b.add_vertex(*id, *at, *point)?;
            b.finalize_op();
            OpOutcome::Updated(*id)
        }
        Operation::Delete { id } => {
            // can_remove validates existence AND removability (locked layer) before
            // any mutation, so a locked entity errors cleanly instead of a no-op
            // reported as Deleted (plan review: erase path).
            b.can_remove(*id).map_err(|e| match e {
                ApiError::UnknownId(_) => ApiError::validation(name, format!("unknown ObjectId {id:?}")),
                other => other,
            })?;
            b.push_undo(name);
            b.remove_entity(*id)?;
            b.finalize_op();
            OpOutcome::Deleted(vec![*id])
        }
        Operation::CreateMany(specs) => {
            if specs.len() > BULK_ITEM_CAP {
                return Err(over_cap(name, specs.len()));
            }
            // Upfront validation: compute/validate EVERY payload before any insert
            // (all-or-nothing without rollback, plan §5.3).
            let mut prepared = Vec::with_capacity(specs.len());
            for (i, spec) in specs.iter().enumerate() {
                let prep = match spec {
                    EntitySpec::Solid(p) => {
                        Prepared::Solid(crate::geom::make_solid(p).map_err(|e| at_index(name, i, e))?)
                    }
                    EntitySpec::Curve(c) => Prepared::Curve(c.clone()),
                };
                prepared.push(prep);
            }
            b.push_undo(name);
            let mut ids = Vec::with_capacity(prepared.len());
            for prep in prepared {
                let id = match prep {
                    Prepared::Solid(body) => b.store_solid(&body)?,
                    Prepared::Curve(c) => b.add_curve(&c)?,
                };
                ids.push(id);
            }
            b.finalize_op();
            OpOutcome::NewIds(ids)
        }
        Operation::TransformMany { ids, placement } => {
            if ids.len() > BULK_ITEM_CAP {
                return Err(over_cap(name, ids.len()));
            }
            // Pre-validate EVERYTHING fallible before push_undo (all-or-nothing,
            // plan §5.3): every id must exist AND be transformable, so the apply
            // loop below cannot fail part-way. `transform_entity` after this is
            // infallible-by-construction for the validated ids.
            for (i, id) in ids.iter().enumerate() {
                b.ensure_transformable(*id).map_err(|e| match e {
                    ApiError::UnknownId(_) => stale_index(name, i, *id),
                    other => match other {
                        ApiError::Unsupported(msg) => ApiError::Validation {
                            op: name.to_string(),
                            reason: format!("index {i}: {msg}"),
                        },
                        o => o,
                    },
                })?;
            }
            b.push_undo(name);
            for id in ids {
                b.transform_entity(*id, placement)?;
            }
            b.finalize_op();
            OpOutcome::Updated(*ids.first().unwrap_or(&ObjectId::NULL))
        }
        Operation::DeleteMany(ids) => {
            if ids.len() > BULK_ITEM_CAP {
                return Err(over_cap(name, ids.len()));
            }
            // Pre-validate existence AND removability (locked layer) for every id
            // before push_undo, so the apply loop cannot fail part-way.
            for (i, id) in ids.iter().enumerate() {
                b.can_remove(*id).map_err(|e| match e {
                    ApiError::UnknownId(_) => stale_index(name, i, *id),
                    ApiError::Unsupported(msg) => ApiError::Validation {
                        op: name.to_string(),
                        reason: format!("index {i}: {msg}"),
                    },
                    other => other,
                })?;
            }
            b.push_undo(name);
            for id in ids {
                b.remove_entity(*id)?;
            }
            b.finalize_op();
            OpOutcome::Deleted(ids.clone())
        }
        Operation::CreateInsert(spec) => {
            // Validate the block name exists before committing (no rollback needed).
            b.push_undo(name);
            let id = b.add_insert(spec)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
        Operation::CreateViewport(spec) => {
            b.push_undo(name);
            let id = b.add_viewport(spec)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
        Operation::CreateText(spec) => {
            b.push_undo(name);
            let id = b.add_text(spec)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
        Operation::CreateMText(spec) => {
            b.push_undo(name);
            let id = b.add_mtext(spec)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
        Operation::SetTextContent { id, value } => {
            b.can_modify(*id).map_err(|e| match e {
                ApiError::UnknownId(_) => ApiError::validation(name, format!("unknown ObjectId {id:?}")),
                other => other,
            })?;
            b.push_undo(name);
            b.set_text_content(*id, value)?;
            b.finalize_op();
            OpOutcome::Updated(*id)
        }
        Operation::CreateHatch(spec) => {
            // Validate the boundary (>= 3 points) before committing.
            if spec.boundary.len() < 3 {
                return Err(ApiError::validation(name, "hatch boundary needs >= 3 points"));
            }
            b.push_undo(name);
            let id = b.add_hatch(spec)?;
            b.finalize_op();
            OpOutcome::NewId(id)
        }
    };
    Ok(Receipt {
        outcome: Some(outcome),
        query_results: Vec::new(),
        new_revision: b.revision(),
    })
}

enum Prepared {
    Solid(crate::backend::KernelBody),
    Curve(crate::ops::Curve2Spec),
}

/// Apply a batch of read-only queries (no undo, no bump; `&mut` because solid
/// resolution may populate the kernel body cache on miss). Bounded like the bulk
/// ops: a batch over `BULK_ITEM_CAP` is rejected before any work (the 64 MiB frame
/// cap bounds message size, not count).
pub fn apply_queries<B: DocApiBackend>(b: &mut B, queries: Vec<Query>) -> ApiResult<Receipt> {
    if queries.len() > BULK_ITEM_CAP {
        return Err(ApiError::validation(
            "Queries",
            format!("query batch over cap: {} > {}", queries.len(), BULK_ITEM_CAP),
        ));
    }
    let mut results = Vec::with_capacity(queries.len());
    for q in &queries {
        let r = match *q {
            Query::GetEntity { id } => QueryResult::Entity(b.get_entity(id)?),
            Query::GetBounds { id } => QueryResult::Bounds(b.bounds(id)?),
            Query::GetCentroid { id } => QueryResult::Centroid(b.centroid(id)?),
            Query::GetVolume { id } => QueryResult::Volume(b.volume(id)?),
            Query::GetIntersects { a, b: bid } => {
                let ba = b.bounds(a)?;
                let bb = b.bounds(bid)?;
                QueryResult::Intersects(ba.overlaps(&bb))
            }
            Query::GetGeometryRevision => QueryResult::Revision(b.revision()),
            Query::GetTextContent { id } => QueryResult::TextContent(b.text_content(id)?),
            Query::GetHatchBoundary { id } => QueryResult::HatchBoundary(b.hatch_boundary(id)?),
        };
        results.push(r);
    }
    Ok(Receipt {
        outcome: None,
        query_results: results,
        new_revision: b.revision(),
    })
}

fn profile_curves<B: DocApiBackend>(
    b: &B,
    profile: ObjectId,
    op: &'static str,
) -> ApiResult<Vec<cadkernel::geom2d::Curve>> {
    if !b.entity_exists(profile) {
        return Err(ApiError::validation(op, format!("unknown ObjectId {profile:?}")));
    }
    b.profile_curves(profile)
}

fn require_exists<B: DocApiBackend>(b: &B, id: ObjectId, op: &'static str) -> ApiResult<()> {
    if b.entity_exists(id) {
        Ok(())
    } else {
        Err(ApiError::validation(op, format!("unknown ObjectId {id:?}")))
    }
}

fn over_cap(op: &'static str, n: usize) -> ApiError {
    ApiError::validation(op, format!("bulk op over cap: {n} > {BULK_ITEM_CAP}"))
}

fn stale_index(op: &'static str, i: usize, id: ObjectId) -> ApiError {
    ApiError::validation(op, format!("stale ObjectId at index {i}: {id:?}"))
}

fn at_index(op: &'static str, i: usize, e: ApiError) -> ApiError {
    match e {
        ApiError::Validation { reason, .. } => ApiError::validation(op, format!("index {i}: {reason}")),
        other => other,
    }
}

/// Boolean op kind → kernel `Operation` (host feature only).
#[cfg(feature = "host")]
pub(crate) fn kernel_bool_op(op: BoolOp) -> cadkernel::brep::Operation {
    match op {
        BoolOp::Union => cadkernel::brep::Operation::Union,
        BoolOp::Intersection => cadkernel::brep::Operation::Intersection,
        BoolOp::Difference => cadkernel::brep::Operation::Difference,
    }
}
