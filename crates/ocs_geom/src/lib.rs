//! OpenCADStudio's 2D curve algebra and the 3D space a curve lives in, taken
//! from opencadkernel when its B-rep layer went to Solidcore.
//!
//! ```text
//! geom2d  curves, intersection, offset, containment
//!   │
//! space   the plane a 2D curve lives on, and the map to it
//! ```
//!
//! # Coordinate policy
//!
//! Every entry point lifts its input into a local frame before doing any
//! math, and shifts results back to world coordinates on the way out. See
//! [`geom2d::frame`] for why this is an invariant rather than a convenience.

/// Unconditional: a plane and a vector depend on nothing, and a caller
/// bridging a file format still has to say where a face's frame points.
pub mod space;

pub mod tessellation;

#[cfg(feature = "geom2d")]
pub mod geom2d;

/// 2D curves of every kind as rational B-splines, and placed in space.
#[cfg(feature = "geom2d")]
pub mod rational;
