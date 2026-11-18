//! 2D curve algebra.
//!
//! This layer answers the questions the rest of the kernel asks constantly:
//! where do two curves meet, what does this loop offset to, is this point
//! inside, which fragments chain into a closed boundary. Editing commands
//! sit directly on it, and the B-rep layer reaches down into it whenever it
//! has to work in a face's parameter space.
//!
//! Everything here obeys the coordinate policy in [`frame`].

pub mod angle;
pub mod arclength;
pub mod area;
pub mod arrangement;
pub mod band;
mod boundary_recovery;
pub mod clip;
pub mod centerline;
pub mod containment;
pub mod construct;
pub mod constrained;
pub mod constraint_inference;
pub mod cross;
pub mod curve;
pub mod curve_offset;
pub mod deviation;
pub mod dimension;
pub mod fillet;
pub mod frame;
pub mod gradient;
pub mod intersect;
pub mod nurbs;
#[cfg(feature = "offset")]
pub mod offset;
pub mod polyline;
pub mod polyline_edit;
pub mod snap;
pub mod tessellate;
pub mod triangulate;
pub mod transform;
pub mod vec;

pub use angle::{angle_within_arc, arc_parameter, arc_span, normalize_angle};
pub use arrangement::{bounded_faces, gap_bridges, segment_crossing, signed_area, SegmentCrossing};
pub use band::{
    polyline_band_boundary, BandBoundaryEdge, BandStationPiece, PolylineBandBoundary,
};
pub use boundary_recovery::refine_spline_boundary;
pub use clip::{break_spans, inside_pieces, inside_spans, trim_spans};
pub use centerline::{centerline_between, CenterLineGeometry};
pub use containment::{closest_point, contains, distance_to, nearest_of, Closest};
pub use constraint_inference::{
    infer_constraints, infer_constraints_with_settings, Endpoint as ConstraintEndpoint,
    InferenceKind, InferenceSettings, InferredConstraint, ParametricPrimitive,
};
pub use dimension::linear_dimension_leader;
pub use construct::{
    arc_from_endpoints_angle, arc_from_endpoints_radius, arc_from_sagitta,
    arc_from_start_tangent, arc_sweep_from_chord, arc_through_points, bounded_arc,
};
pub use cross::{intersect, Crossing};
pub use curve::{Arc, Circle, Curve, EllipseArc, Extent, Line, Ray, XLine};
pub use curve_offset::offset_curve;
pub use nurbs::{NurbsCurve, Parameterization};
pub use frame::Frame;
pub use fillet::{fillet_between_rays, fillets_between, Fillet};
pub use gradient::{gradient_frame, GradientFrame};
pub use intersect::{
    circle_circle_angles, circle_circle_points, ellipse_circle, line_circle, line_ellipse,
    line_line,
};
#[cfg(feature = "offset")]
pub use offset::{offset_polyline, offset_polyline_with_gap, OffsetGap};
pub use polyline::{
    BulgeArc, Polyline, PolylineRange, PolylineRangeSegment, PolylineVertex, RectangleFrame,
};
pub use polyline_edit::{
    move_polyline_segment_parallel, polyline_arc_radius_from_point,
    polyline_segment_parallel_offset, resize_polyline_arc,
};
pub use snap::{
    characteristic_points, nearest_to, perpendicular_from, tangent_from, SnapKind, SnapPoint,
};
pub use tessellate::{arc, ellipse_arc, lerp, DEFAULT_SEGMENTS_PER_RADIAN};
pub use transform::Transform;
pub use triangulate::{
    nesting_depths as ring_nesting_depths, polygon as triangulate, polygon_frame,
    rings as triangulate_rings, PolygonFrame,
};
pub use vec::Vec2;

/// An ellipse in the plane.
///
/// Grouped rather than passed as loose scalars because the four fields are
/// only meaningful together, and because a parameter is not interpretable
/// without the axis it is measured from — see [`Ellipse::point_at`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    /// Centre in world coordinates.
    pub centre: [f64; 2],
    /// Half-length along [`major_axis`](Self::major_axis).
    pub major_radius: f64,
    /// Half-length across it.
    pub minor_radius: f64,
    /// Unit vector along the major axis. The minor direction is this turned a
    /// quarter turn counter-clockwise.
    pub major_axis: [f64; 2],
}

impl Ellipse {
    /// The unit minor direction: [`major_axis`](Self::major_axis) rotated a
    /// quarter turn counter-clockwise.
    pub fn minor_axis(&self) -> [f64; 2] {
        [-self.major_axis[1], self.major_axis[0]]
    }

    /// The point at parameter `t`.
    ///
    /// `t` is the ellipse's own parameter, not an angle measured at the
    /// centre: the two agree only when the radii are equal.
    pub fn point_at(&self, t: f64) -> [f64; 2] {
        let along = self.major_radius * t.cos();
        let across = self.minor_radius * t.sin();
        let minor = self.minor_axis();
        [
            self.centre[0] + along * self.major_axis[0] + across * minor[0],
            self.centre[1] + along * self.major_axis[1] + across * minor[1],
        ]
    }

    /// Whether the radii are large enough to describe a curve at all.
    pub fn is_degenerate(&self) -> bool {
        self.major_radius.abs() < 1e-20 || self.minor_radius.abs() < 1e-20
    }

    /// The parameter of the point on the ellipse nearest to `point`: its
    /// orthogonal projection, not the parameter read off the squashed
    /// direction, which drifts by tens of degrees for a point just off the
    /// curve.
    pub fn closest_parameter(&self, point: [f64; 2]) -> f64 {
        let rx = point[0] - self.centre[0];
        let ry = point[1] - self.centre[1];
        let (nx, ny) = (self.major_axis[0], self.major_axis[1]);
        ellipse_closest_parameter(
            self.major_radius,
            self.minor_radius,
            rx * nx + ry * ny,
            -rx * ny + ry * nx,
        )
    }
}

/// The parameter of the point nearest to `(u, v)` on the origin-centred,
/// axis-aligned ellipse `(x/a)² + (y/b)² = 1` (Eberly's orthogonal
/// projection: a bracketed Newton solve on the distance function).
pub fn ellipse_closest_parameter(a: f64, b: f64, u: f64, v: f64) -> f64 {
    if a < 1e-12 || b < 1e-12 {
        return 0.0;
    }
    if (a - b).abs() < 1e-9 {
        return v.atan2(u);
    }
    if a < b {
        return std::f64::consts::FRAC_PI_2 - ellipse_closest_parameter(b, a, v, u);
    }

    let sign_x = if u >= 0.0 { 1.0 } else { -1.0 };
    let sign_y = if v >= 0.0 { 1.0 } else { -1.0 };
    let (u_abs, v_abs) = (u.abs(), v.abs());

    if v_abs < 1e-12 {
        let x = if u_abs < (a * a - b * b) / a {
            a * a * u_abs / (a * a - b * b)
        } else {
            a
        };
        let y = b * (1.0 - (x / a).powi(2)).max(0.0).sqrt();
        return (sign_y * y / b).atan2(sign_x * x / a);
    }
    if u_abs < 1e-12 {
        return sign_y.atan2(0.0);
    }

    let (a2, b2) = (a * a, b * b);
    let mut low = -b2 + b * v_abs;
    let mut high = -b2 + (a * u_abs).hypot(b * v_abs);
    let mut t = low;
    for _ in 0..25 {
        let (t_a, t_b) = (t + a2, t + b2);
        if t_a.abs() < 1e-12 || t_b.abs() < 1e-12 {
            break;
        }
        let f = (a * u_abs / t_a).powi(2) + (b * v_abs / t_b).powi(2) - 1.0;
        if f.abs() < 1e-12 {
            break;
        }
        let df = -2.0 * (a2 * u_abs * u_abs / t_a.powi(3) + b2 * v_abs * v_abs / t_b.powi(3));
        let next = t - f / df;
        if next > low && next < high {
            t = next;
        } else {
            if f > 0.0 {
                low = t;
            } else {
                high = t;
            }
            t = 0.5 * (low + high);
        }
    }
    let x = a2 * u_abs / (t + a2);
    let y = b2 * v_abs / (t + b2);
    (sign_y * y / b).atan2(sign_x * x / a)
}

/// Distance below which two positions are treated as one.
///
/// Carried explicitly rather than read from a global, because a drawing in
/// millimetres and a drawing in survey feet do not want the same value, and
/// because a boolean needs to widen it locally without disturbing anything
/// else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    linear: f64,
}

impl Tolerance {
    /// A tolerance of `linear` world units.
    ///
    /// # Panics
    ///
    /// If `linear` is not finite and positive. A non-positive tolerance makes
    /// every comparison in the kernel meaningless, so it is worth refusing at
    /// the boundary rather than producing degenerate topology later.
    pub fn new(linear: f64) -> Self {
        assert!(
            linear.is_finite() && linear > 0.0,
            "tolerance must be finite and positive, got {linear}"
        );
        Self { linear }
    }

    /// The linear tolerance in world units.
    pub const fn linear(&self) -> f64 {
        self.linear
    }

    /// Whether `a` and `b` are within tolerance of each other.
    pub fn are_equal(&self, a: f64, b: f64) -> bool {
        (a - b).abs() <= self.linear
    }
}

impl Default for Tolerance {
    /// 1e-7, matching the point-equality tolerance ACIS records in its
    /// header, so geometry surviving a round trip is judged by the same
    /// yardstick at both ends.
    fn default() -> Self {
        Self::new(1e-7)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_respects_the_configured_tolerance() {
        let tol = Tolerance::new(1e-3);
        assert!(tol.are_equal(1.0, 1.0005));
        assert!(!tol.are_equal(1.0, 1.002));
    }

    #[test]
    #[should_panic(expected = "finite and positive")]
    fn zero_tolerance_is_refused() {
        Tolerance::new(0.0);
    }
}

mod arc_fit;
pub use arc_fit::{fit_arc_chain, ArcFitVertex};
mod bounds;
pub use bounds::analytic_curve_bounds;
