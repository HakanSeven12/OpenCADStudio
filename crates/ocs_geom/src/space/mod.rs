//! Where plane geometry sits in space.
//!
//! [`geom2d`](crate::geom2d) is deliberately two-dimensional: an
//! intersection, an offset or a containment test is a plane problem, and
//! answering it in three coordinates would mean carrying a component that is
//! either zero or a source of error. This module is the other half of that
//! bargain — the frame that says *which* plane, and the map in and out of it.
//!
//! [`Plane`] is the whole of it, and [`PlanarCurve`] is the pairing every
//! consumer actually wants: a shape plus where it lives. A drawing's arcs,
//! circles, ellipses and polylines are stored exactly this way, and so is a
//! planar B-rep face.

pub mod alignment;
pub mod arclength;
pub mod bernstein;
pub mod arc_union;
pub mod curve;
pub mod dimension;
pub mod endpoint_join;
pub mod helix;
pub mod line_union;
pub mod lengthen;
pub mod nurbs;
#[cfg(feature = "nlib")]
pub mod nurbs_tools;
mod knot_compaction;
mod polyline_approximation;
pub mod plane;
pub mod polygon;
pub mod rigid_constraint;
pub mod spline;
pub mod smooth;
pub mod source_join;
pub mod vec;

#[cfg(feature = "geom2d")]
pub mod planar;

pub use alignment::{BoundsAlignment, align_aabbs_2d, align_point_pairs};
pub use arclength::ArcLengthCurve3;
pub use dimension::{
    default_dimension_jog_position, dimension_jog_points, dimension_spacing_frame,
    move_to_dimension_spacing, nearest_segment_point, segment_break_gap_xy,
    DimensionSpacingFrame,
};
pub use helix::{HelixCurve, HelixDirection};
pub use line_union::{line_union, simplify_linear_chain, LineUnion, LineUnionKind};
pub use nurbs::{NurbsCurve3, NurbsSurface3};
pub use plane::{are_coplanar, coplanarity_tolerance, reorient_axis_to_plane, Plane};
pub use polyline_approximation::SplinePolyline;
pub use rigid_constraint::solve_rigid_point_coincidence;
pub use smooth::{smooth_nurbs_endpoint, CurveJet, SplineEnd};
pub use spline::{clamped_uniform_knots, de_boor, Parameterization};
pub use vec::Vec3;

#[cfg(feature = "geom2d")]
pub use planar::{common_curve_plane, PlanarCurve};
