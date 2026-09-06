// HAND-MAINTAINED wire vocabulary (the Operation enum). This file is NOT derived
// from spec/entities.toml by build.rs — the enum is the canonical append-only
// wire vocabulary; the spec's `op` names must each map to a variant here
// (enforced by a test). Append new variants at the END only (bincode discriminant
// stability, plan §7). Do NOT reorder or remove variants.

use serde::{Deserialize, Serialize};

use crate::id::ObjectId;
use crate::ops::{BoolOp, Curve2Spec, DimensionAngularSpec, DimensionRadialSpec, DimensionSpec, EntitySpec, HatchSpec, InsertSpec, MTextSpec, PlacementSpec, RasterImageSpec, SolidPrimitive, TextSpec, ViewportSpec};

/// A typed write operation (plan §5). Each variant is ONE atomic host call = one
/// undo step. Append new variants at the END only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Operation {
    // construction
    CreateSolid(SolidPrimitive),
    CreateCurve(Curve2Spec),
    Extrude { profile: ObjectId, direction: [f64; 3] },
    Revolve { profile: ObjectId, axis: ([f64; 3], [f64; 3]), angle: f64 },
    // modification
    Transform { id: ObjectId, placement: PlacementSpec },
    SolidBoolean { op: BoolOp, a: ObjectId, b: ObjectId, erase_sources: bool },
    AddVertex { id: ObjectId, at: usize, point: [f64; 3] },
    Delete { id: ObjectId },
    // Bulk ops (single op, upfront-validation all-or-nothing — NOT a transaction, §5.3)
    CreateMany(Vec<EntitySpec>),
    TransformMany { ids: Vec<ObjectId>, placement: PlacementSpec },
    DeleteMany(Vec<ObjectId>),
    // containers
    CreateInsert(InsertSpec),
    // paper-space
    CreateViewport(ViewportSpec),
    // annotations
    CreateText(TextSpec),
    CreateMText(MTextSpec),
    SetTextContent { id: ObjectId, value: String },
    // hatch
    CreateHatch(HatchSpec),
    // dimensions
    CreateDimensionLinear(DimensionSpec),
    // attributes
    SetAttribute { id: ObjectId, tag: String, value: String },
    // viewport view
    SetViewportView { id: ObjectId, view_target: [f64; 3], view_height: f64 },
    // media
    CreateRasterImage(RasterImageSpec),
    // multi-profile solid
    Loft { profiles: Vec<ObjectId> },
    // dimension sub-types
    CreateDimensionRadius(DimensionRadialSpec),
    CreateDimensionDiameter(DimensionRadialSpec),
    CreateDimensionAngular(DimensionAngularSpec),
}
