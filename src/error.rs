//! Structured error model. Serializable so IPC returns the
//! same error the in-process executor produced.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::id::ObjectId;

/// Which kind of kernel failure occurred (a mapped `cadkernel` `Snag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeometryErrorKind {
    /// The operation produced an empty / degenerate body.
    Empty,
    /// Boolean refused (coincident geometry, unsupported cut, …).
    BooleanFailed,
    /// Invalid input to a kernel constructor (e.g. non-positive radius).
    InvalidInput,
    /// ACIS lift/parse/lower failed.
    Acis,
    /// Any other kernel error (message carries detail).
    Other,
}

/// The single error type for the whole API. `commit`/ops return
/// `Result<_, ApiError>`; on a failed op nothing is applied and no undo step is
/// recorded (per-op atomicity).
#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ApiError {
    /// Input failed validation before any mutation (names the op + reason).
    #[error("validation failed on {op}: {reason}")]
    Validation { op: String, reason: String },

    /// The geometry kernel reported a failure (a mapped `Snag`).
    #[error("geometry kernel: {kind:?}: {msg}")]
    Geometry {
        kind: GeometryErrorKind,
        msg: String,
    },

    /// An `ObjectId` does not resolve to a live entity (stale/deleted/never existed).
    #[error("unknown ObjectId {0:?}")]
    UnknownId(ObjectId),

    /// The requested capability is not supported (e.g. over-cap bulk op, unimplemented family).
    #[error("unsupported capability: {0}")]
    Unsupported(String),

    /// The transport (IPC / channel) failed — disconnected, oversized, timeout.
    #[error("transport: {0}")]
    Transport(String),
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn validation(op: &'static str, reason: impl Into<String>) -> Self {
        ApiError::Validation {
            op: op.to_string(),
            reason: reason.into(),
        }
    }
    pub fn geometry(kind: GeometryErrorKind, msg: impl Into<String>) -> Self {
        ApiError::Geometry {
            kind,
            msg: msg.into(),
        }
    }
}
