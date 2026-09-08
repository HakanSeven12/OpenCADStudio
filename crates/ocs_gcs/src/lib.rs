//! 2D geometric constraint solver for OpenCADStudio sketches.
//!
//! A Rust port of FreeCAD's planegcs (LGPL-2.1-or-later), staged bottom-up:
//! see `/Users/felix/.claude/plans/snoopy-finding-matsumoto.md` for the full
//! port plan and stage order.

pub mod constraints;
pub mod diagnosis;
pub mod geo;
pub mod graph;
pub mod qp_eq;
pub mod solvers;
pub mod subsystem;
pub mod system;
pub mod util;
