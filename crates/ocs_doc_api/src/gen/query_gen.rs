// HAND-MAINTAINED wire vocabulary (the Query enum). NOT derived from the spec by
// build.rs — the enum is the canonical append-only wire vocabulary; the spec's
// `query` names must each map to a variant here (enforced by a test). Append new
// variants at the END only (bincode discriminant stability).

use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

/// A typed read query. Read-only: no mutation, no undo, no revision
/// bump. Safe to batch (`EnvelopeBody::Queries`). Append new variants at the END only.
/// (Not `Copy`: some variants carry owned data like a block name.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Query {
    GetEntity {
        id: ObjectId,
    },
    GetBounds {
        id: ObjectId,
    },
    GetCentroid {
        id: ObjectId,
    },
    GetVolume {
        id: ObjectId,
    },
    /// Whether two entities' bounds overlap (two-phase conditional modelling).
    GetIntersects {
        a: ObjectId,
        b: ObjectId,
    },
    GetGeometryRevision,
    /// The text content of a Text/MText annotation.
    GetTextContent {
        id: ObjectId,
    },
    /// The boundary loops of a Hatch (outer + islands) as polylines.
    GetHatchBoundary {
        id: ObjectId,
    },
    /// The measured value of a Dimension (distance or angle).
    GetDimensionMeasurement {
        id: ObjectId,
    },
    /// An insert's attributes as (tag, value) pairs.
    GetAttributes {
        id: ObjectId,
    },
    /// The entity ids + kinds inside a block definition (read-only traversal).
    GetBlockEntities {
        block_name: String,
    },
    /// A viewport's view (target + zoom height).
    GetViewportView {
        id: ObjectId,
    },
}
