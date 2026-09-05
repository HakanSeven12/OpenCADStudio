//! The wire envelope (plan §7). One request = ONE write op OR a query batch
//! (read-only batching is safe: no mutation, no undo, no rollback).

use serde::{Deserialize, Serialize};

use crate::gen::{Operation, Query};
use crate::id::ObjectId;
use crate::query::QueryResult;
use crate::revision::GeometryRevision;
use crate::ENVELOPE_VERSION;

/// The single request unit crossing the transport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocApiEnvelope {
    /// Crate protocol version (`ENVELOPE_VERSION`). Bridges must check this.
    pub version: u16,
    pub body: EnvelopeBody,
}

impl DocApiEnvelope {
    pub fn op(operation: Operation) -> Self {
        Self { version: ENVELOPE_VERSION, body: EnvelopeBody::Op(operation) }
    }
    pub fn queries(queries: Vec<Query>) -> Self {
        Self { version: ENVELOPE_VERSION, body: EnvelopeBody::Queries(queries) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EnvelopeBody {
    /// Exactly ONE write op (write batching removed, plan §13). Atomic by itself.
    Op(Operation),
    /// One-or-more read-only queries answered together (safe: no mutation/undo).
    Queries(Vec<Query>),
}

/// The outcome of an applied write op.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpOutcome {
    /// A new entity was created; carries its fresh `ObjectId`.
    NewId(ObjectId),
    /// An existing entity was updated in place (same `ObjectId` preserved).
    Updated(ObjectId),
    /// Entities were deleted; carries the deleted ids (one for `Delete`, N for `DeleteMany`).
    Deleted(Vec<ObjectId>),
    /// Many entities were created (bulk `CreateMany`); carries all fresh ids in order.
    NewIds(Vec<ObjectId>),
}

impl OpOutcome {
    /// The single fresh id, for ops that create exactly one entity.
    pub fn new_id(&self) -> Option<ObjectId> {
        match self {
            OpOutcome::NewId(id) => Some(*id),
            _ => None,
        }
    }
    /// All fresh ids, for bulk creates.
    pub fn new_ids(&self) -> &[ObjectId] {
        match self {
            OpOutcome::NewIds(ids) => ids,
            OpOutcome::NewId(id) => std::slice::from_ref(id),
            _ => &[],
        }
    }
}

/// The per-request result (and the query-batch answer carrier).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    /// Present for `EnvelopeBody::Op`; absent for pure queries.
    pub outcome: Option<OpOutcome>,
    /// One `QueryResult` per `Query` in a `Queries` batch (in order).
    pub query_results: Vec<QueryResult>,
    /// The geometry revision after this request (== before, for pure queries).
    pub new_revision: GeometryRevision,
}
