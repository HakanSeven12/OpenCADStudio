//! Ribbon tools and typed-value commands for the persistent "Constraints"
//! group (design doc `docs/parametric_system_design.md` §6.1).
//!
//! All eight — Horizontal, Vertical, Parallel, Perpendicular, Equal,
//! Tangent, Distance, Angle — add a [`crate::scene::sketch_constraints::SketchConstraint`]
//! to the current scope and let `Scene::bump_entities` solve it, the same
//! path any later edit to the constrained entities re-solves through
//! (`src/scene/sketch_solve.rs`). Horizontal/Vertical/Parallel/Perpendicular/
//! Equal/Tangent dispatch straight to `CmdResult::AddSketchConstraint` from
//! `src/app/commands/draw.rs` (no typed value needed, so no `CadCommand` of
//! their own); Distance and Angle need one (`value.rs`), since the target
//! value comes from the command line after the tool is clicked; Coincident
//! needs one too (`coincident.rs`), since it addresses a *point* on an
//! entity rather than the whole thing, so it can't reuse plain selection.

mod coincident;
mod tools;
mod value;
pub use coincident::{coincident_tool, CoincidentConstraintCommand};
pub use tools::{equal, horizontal, parallel, perpendicular, tangent, vertical};
pub use value::{angle_tool, distance_tool, AngleConstraintCommand, DistanceConstraintCommand};
