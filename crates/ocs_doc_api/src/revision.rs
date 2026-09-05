//! Geometry revision counter (mirrors the host scene's `geometry_epoch`).

use serde::{Deserialize, Serialize};

/// Monotonic revision of a document's geometry. Bumped exactly once per applied
/// write op (`push_undo` + `finalize_op`). Pure queries return the current
/// revision without bumping it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GeometryRevision(pub u64);

impl GeometryRevision {
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}
