// HAND-MAINTAINED wire vocabulary (the Query enum). NOT derived from the spec by
// build.rs — the enum is the canonical append-only wire vocabulary; the spec's
// `query` names must each map to a variant here (enforced by a test). Append new
// variants at the END only (bincode discriminant stability, plan §7).

use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

/// A typed read query (plan §5). Read-only: no mutation, no undo, no revision
/// bump. Safe to batch (`EnvelopeBody::Queries`). Append new variants at the END only.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Query {
    GetEntity { id: ObjectId },
    GetBounds { id: ObjectId },
    GetCentroid { id: ObjectId },
    GetVolume { id: ObjectId },
    /// Whether two entities' bounds overlap (two-phase conditional modelling).
    GetIntersects { a: ObjectId, b: ObjectId },
    GetGeometryRevision,
    /// The text content of a Text/MText annotation.
    GetTextContent { id: ObjectId },
    /// The boundary loops of a Hatch (outer + islands) as polylines.
    GetHatchBoundary { id: ObjectId },
}
