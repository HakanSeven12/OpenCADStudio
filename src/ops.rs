//! Write operations — the typed, transport-agnostic vocabulary for changing the
//! document. The `Operation` enum is **hand-maintained** (append-only)
//! in `src/gen/ops_gen.rs` — it is NOT derived from the spec by build.rs; the
//! spec's `op` names must each map to a variant here (asserted by a test). This
//! module re-exports it and holds the hand-written payload mirrors.

use serde::{Deserialize, Serialize};

pub use crate::gen::Operation;

/// Boolean operation kind for [`Operation::SolidBoolean`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    Union,
    Intersection,
    Difference,
}

/// Construction spec for a B-rep solid primitive (plain-data mirror of the
/// non-serde `cadkernel::brep::make::*` arguments).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SolidPrimitive {
    Cuboid {
        origin: [f64; 3],
        size: [f64; 3],
    },
    Sphere {
        centre: [f64; 3],
        radius: f64,
    },
    Cylinder {
        base: [f64; 3],
        radius: f64,
        height: f64,
    },
    Cone {
        base: [f64; 3],
        radius: f64,
        height: f64,
    },
    Torus {
        centre: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
    },
    Wedge {
        origin: [f64; 3],
        size: [f64; 3],
    },
}

/// Construction spec for a minimal 2D curve entity (phase-1 set; profiles/inputs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Curve2Spec {
    Line {
        start: [f64; 3],
        end: [f64; 3],
    },
    Circle {
        centre: [f64; 3],
        radius: f64,
    },
    Polyline {
        points: Vec<[f64; 3]>,
        closed: bool,
    },
    Point {
        position: [f64; 3],
    },
    /// Circular arc, counter-clockwise from `start_angle` to `end_angle` (radians).
    Arc {
        centre: [f64; 3],
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    /// Ellipse: `major_axis` is the major-axis endpoint relative to `centre`;
    /// `ratio` = minor/major. `start`/`end` parameters 0..2π (full ellipse = 0..2π).
    Ellipse {
        centre: [f64; 3],
        major_axis: [f64; 3],
        ratio: f64,
        start: f64,
        end: f64,
    },
    /// NURBS spline (control points + knots + weights; `degree` typically 3).
    Spline {
        degree: i32,
        control_points: Vec<[f64; 3]>,
        knots: Vec<f64>,
        weights: Vec<f64>,
    },
    /// A ray from `origin` along `direction` (bounded at origin only).
    Ray {
        origin: [f64; 3],
        direction: [f64; 3],
    },
    /// An infinite construction line through `origin` along `direction`.
    XLine {
        origin: [f64; 3],
        direction: [f64; 3],
    },
}

/// A generic entity-construction payload used by the bulk op
/// [`crate::ops::Operation::CreateMany`]. Mirrors the per-family construction
/// specs so one bulk op can carry many homogeneous (or mixed) creations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EntitySpec {
    Curve(Curve2Spec),
    Solid(SolidPrimitive),
}

/// Construction spec for a block reference (`INSERT`): place the block
/// `block_name` (must exist as a BlockRecord) at `insert_point` with uniform
/// `scale` and `rotation` (radians, about Z).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsertSpec {
    pub block_name: String,
    pub insert_point: [f64; 3],
    pub scale: f64,
    pub rotation: f64,
}

/// Construction spec for a paper-space `VIEWPORT`: a `width`×`height` viewport
/// centered at `center` (paper space), looking at `view_target` (model space,
/// WCS) with `view_height` (model-space height visible; the viewport's zoom).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewportSpec {
    pub center: [f64; 3],
    pub width: f64,
    pub height: f64,
    pub view_target: [f64; 3],
    pub view_height: f64,
}

/// Construction spec for a single-line `TEXT` annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpec {
    pub value: String,
    pub insertion_point: [f64; 3],
    pub height: f64,
    pub rotation: f64,
}

/// Construction spec for a multi-line `MTEXT` annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MTextSpec {
    pub value: String,
    pub insertion_point: [f64; 3],
    pub height: f64,
}

/// Construction spec for a `HATCH` with a single closed polyline boundary
/// (straight segments). `solid` selects solid fill vs a pattern (pattern naming
/// is a later refinement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HatchSpec {
    /// Closed boundary as an ordered polyline (first==last implied; closed).
    pub boundary: Vec<[f64; 2]>,
    pub solid: bool,
}

/// Construction spec for a linear `DIMENSION`: the two measured points and the
/// dimension-line definition point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionSpec {
    pub first_point: [f64; 3],
    pub second_point: [f64; 3],
    pub definition_point: [f64; 3],
}

/// Construction spec for a radial/diameter `DIMENSION`: the circle center and a
/// point on the circle (radius) or a chord point (diameter).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionRadialSpec {
    /// Center of the arc/circle (radius) or first chord point (diameter).
    pub center: [f64; 3],
    /// A point on the arc/circle (radius) or the far chord point (diameter).
    pub point: [f64; 3],
}

/// Construction spec for a 3-point angular `DIMENSION`: vertex + one point on
/// each leg, plus the dimension-arc location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionAngularSpec {
    pub vertex: [f64; 3],
    pub first_point: [f64; 3],
    pub second_point: [f64; 3],
    pub arc_location: [f64; 3],
}

/// Construction spec for an `ATTDEF` (attribute definition): the tag, prompt,
/// and default value for an in-block attribute, at an insertion point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeDefinitionSpec {
    pub tag: String,
    pub prompt: String,
    pub default_value: String,
    pub insertion_point: [f64; 3],
    pub height: f64,
    pub rotation: f64,
}

/// Construction spec for a `TABLE`: an insertion point plus a grid of cell
/// strings (`data[row][column]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSpec {
    pub insertion_point: [f64; 3],
    /// Grid of cell text values, rows × columns.
    pub data: Vec<Vec<String>>,
}

/// Construction spec for a `RASTER_IMAGE`: an image file placed at
/// `insertion_point` with per-pixel world-unit vectors `u_vector`/`v_vector` and
/// pixel `size` (width, height). The host auto-registers the ImageDefinition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RasterImageSpec {
    pub file_path: String,
    pub insertion_point: [f64; 3],
    pub u_vector: [f64; 3],
    pub v_vector: [f64; 3],
    /// Pixel size (width, height).
    pub size: [f64; 2],
}

/// A rigid similarity (rotation + translation + optional uniform scale/reflection)
/// as a plain-data mirror of `cadkernel::brep::transform`'s `Placement`
/// (non-serde). Columns are the new basis axes; `origin` is the translation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlacementSpec {
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub z_axis: [f64; 3],
    pub origin: [f64; 3],
}

impl PlacementSpec {
    /// Identity placement (no transform).
    pub const IDENTITY: PlacementSpec = PlacementSpec {
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
        origin: [0.0, 0.0, 0.0],
    };

    /// Pure translation to `origin` (axes unchanged).
    pub fn at(origin: [f64; 3]) -> Self {
        Self {
            origin,
            ..Self::IDENTITY
        }
    }
}

/// The item-count cap for a single bulk op envelope: bounds
/// host-side execution time and payload. Over-cap → `ApiError::Validation`.
pub const BULK_ITEM_CAP: usize = 100_000;

/// Convenience accessors used by the facade/executor.
impl crate::gen::Operation {
    /// The op name used in `ApiError::Validation { op, .. }`.
    pub fn op_name(&self) -> &'static str {
        use crate::gen::Operation::*;
        match self {
            CreateSolid(_) => "CreateSolid",
            CreateCurve(_) => "CreateCurve",
            Extrude { .. } => "Extrude",
            Revolve { .. } => "Revolve",
            Transform { .. } => "Transform",
            SolidBoolean { .. } => "SolidBoolean",
            AddVertex { .. } => "AddVertex",
            Delete { .. } => "Delete",
            CreateMany(_) => "CreateMany",
            TransformMany { .. } => "TransformMany",
            DeleteMany(_) => "DeleteMany",
            CreateInsert(_) => "CreateInsert",
            CreateViewport(_) => "CreateViewport",
            CreateText(_) => "CreateText",
            CreateMText(_) => "CreateMText",
            SetTextContent { .. } => "SetTextContent",
            CreateHatch(_) => "CreateHatch",
            CreateDimensionLinear(_) => "CreateDimensionLinear",
            SetAttribute { .. } => "SetAttribute",
            SetViewportView { .. } => "SetViewportView",
            CreateRasterImage(_) => "CreateRasterImage",
            Loft { .. } => "Loft",
            CreateDimensionRadius(_) => "CreateDimensionRadius",
            CreateDimensionDiameter(_) => "CreateDimensionDiameter",
            CreateDimensionAngular(_) => "CreateDimensionAngular",
            CreateAttributeDefinition(_) => "CreateAttributeDefinition",
            CreateTable(_) => "CreateTable",
            CreateDimensionAngular2Ln(_) => "CreateDimensionAngular2Ln",
        }
    }
}
