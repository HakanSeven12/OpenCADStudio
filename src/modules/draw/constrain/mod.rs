//! Ribbon tools and typed-value commands for persistent constraints.
//!
//! Every one adds a [`crate::scene::sketch_constraints::SketchConstraint`]
//! to the current scope and lets `Scene::bump_entities` solve it, the same
//! path any later edit to the constrained entities re-solves through
//! (`src/scene/sketch_solve.rs`). By how they gather input:
//! - **Plain selection, no picking** — Horizontal, Vertical, Fixed (select
//!   one entity); Parallel, Perpendicular, Equal, Tangent, Colinear,
//!   Concentric, Normal (select two); Symmetric (select three: the
//!   mirrored pair, then the mirror line) — dispatch straight to
//!   `CmdResult::AddSketchConstraint` from `src/app/commands/draw.rs`.
//! - **A typed value** — Distance, Angle (`value.rs`): the target value
//!   comes from the command line after the tool is clicked.
//! - **A picked point, entity pre-selected** — CenterPoint, Midpoint,
//!   PointOnCurve (`point_on_entity.rs`): the entity side has no ambiguity
//!   (select it first, like Distance/Angle's target), but the point side
//!   does (which of a line's two endpoints?), so it's picked interactively
//!   and resolved by the host.
//! - **Picked points only, nothing pre-selected** — Coincident
//!   (`coincident.rs`, two points), EqualDistance (`equal_distance.rs`,
//!   four points): every side is a point, so plain selection can't express
//!   it at all.

mod coincident;
mod equal_distance;
mod point_on_entity;
mod tools;
mod value;
pub use coincident::{coincident_tool, CoincidentConstraintCommand};
pub use equal_distance::{equal_distance_tool, EqualDistanceConstraintCommand};
pub use point_on_entity::{
    center_point_tool, midpoint_tool, point_on_curve_tool, PointOnEntityConstraintCommand,
};
pub use tools::{
    colinear, concentric, equal, fixed, horizontal, normal, parallel, perpendicular, symmetric,
    tangent, vertical,
};
pub use value::{angle_tool, distance_tool, AngleConstraintCommand, DistanceConstraintCommand};
