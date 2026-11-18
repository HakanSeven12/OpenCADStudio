//! A plane curve together with the plane it lives on.
//!
//! Almost every curve a drawing stores is this: a shape defined in two
//! coordinates plus a frame saying where in space those two coordinates
//! point. Keeping the pair together is what lets the 2D layer stay genuinely
//! two-dimensional — intersection, offset, containment and trimming all run
//! in the plane's own coordinates, and only the results come back out into
//! space.
//!
//! It is also what a B-rep face needs. A face is a surface plus loops in its
//! `(u, v)` space; for a planar face that is precisely a [`Plane`] and a set
//! of [`Curve`]s on it.

use super::plane::Plane;
use crate::geom2d::{Curve, Extent};

/// A curve in space, expressed in the coordinates of the plane it lies on.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanarCurve {
    /// The frame the curve's coordinates are read in.
    pub plane: Plane,
    /// The shape, in that frame.
    pub curve: Curve,
}

impl PlanarCurve {
    /// A curve on a plane.
    pub const fn new(plane: Plane, curve: Curve) -> Self {
        Self { plane, curve }
    }

    /// A curve on the world XY plane, which is where most drawing geometry
    /// sits.
    pub const fn flat(curve: Curve) -> Self {
        Self::new(Plane::XY, curve)
    }

    /// The point at parameter `t`, in world coordinates.
    ///
    /// `t` runs `0..=1` across the curve, exactly as it does in the plane;
    /// the frame changes where the point is, never how it is parameterised.
    pub fn point_at(&self, t: f64) -> [f64; 3] {
        self.lower(self.curve.point_at(t))
    }

    /// The parameter at `point`, the inverse of
    /// [`point_at`](Self::point_at).
    ///
    /// A point off the plane is projected onto it first, so a cursor
    /// position or an intersection can be handed over as it is. `None` only
    /// when the plane is degenerate.
    pub fn parameter_at(&self, point: [f64; 3]) -> Option<f64> {
        Some(self.curve.parameter_at(self.lift(point)?))
    }

    /// Samples the curve into a polyline of world-space points.
    ///
    /// `segments_per_radian` sets how finely curved parts are cut, as in
    /// [`Curve::tessellate`].
    pub fn tessellate(&self, segments_per_radian: f64) -> Vec<[f64; 3]> {
        let flat = self.curve.tessellate(segments_per_radian);
        flat.into_iter().map(|uv| self.lower(uv)).collect()
    }

    /// Samples by maximum change of direction, in radians.
    pub fn tessellate_angle(&self, max_angle: f64) -> Vec<[f64; 3]> {
        self.curve
            .tessellate_angle(max_angle)
            .into_iter()
            .map(|uv| self.lower(uv))
            .collect()
    }

    /// Samples the curve into world-space points, cut so that no chord
    /// departs from it by more than `tolerance`.
    ///
    /// The measure a drawing wants: a tolerance is a length, so an arc a
    /// metre across and one a kilometre across each get the points they need
    /// rather than the same count. See
    /// [`Curve::tessellate_within`](crate::geom2d::Curve::tessellate_within).
    ///
    /// Only valid for a frame that preserves length — the tolerance is
    /// measured in the plane's coordinates, and a scaled frame would stretch
    /// it on the way out.
    pub fn tessellate_within(&self, tolerance: f64) -> Vec<[f64; 3]> {
        self.curve
            .tessellate_within(tolerance)
            .into_iter()
            .map(|uv| self.lower(uv))
            .collect()
    }

    /// The curve's length in world units.
    ///
    /// Measured in the plane's coordinates, which is the same thing provided
    /// the frame preserves length — true of every frame a drawing produces,
    /// since an extrusion normal yields unit perpendicular axes. A plane
    /// carrying a scaled parameterisation would report the length in *its*
    /// units, which is what such a plane means.
    pub fn length(&self) -> f64 {
        self.curve.length()
    }

    /// The world point at arc length `distance` from the start.
    ///
    /// The question DIVIDE, MEASURE and a path array all ask. See
    /// [`Curve::point_at_distance`](crate::geom2d::Curve::point_at_distance)
    /// for why it is not the same as evaluating at a fraction of the
    /// parameter.
    pub fn point_at_distance(&self, distance: f64) -> [f64; 3] {
        self.lower(self.curve.point_at_distance(distance))
    }

    /// The parameter at arc length `distance` from the start.
    pub fn parameter_at_distance(&self, distance: f64) -> f64 {
        self.curve.parameter_at_distance(distance)
    }

    /// The direction of travel at `t`, in world coordinates.
    pub fn tangent_at(&self, t: f64) -> [f64; 3] {
        self.plane.vector_at(self.curve.tangent_at(t))
    }

    /// The plane's unit normal, or `None` if its axes do not span one.
    pub fn normal(&self) -> Option<[f64; 3]> {
        self.plane.normal()
    }

    /// How far the curve's parameter runs. Delegates to the shape: a frame
    /// does not bound anything.
    pub fn extent(&self) -> Extent {
        self.curve.extent()
    }

    /// Whether the curve returns to where it started.
    pub fn is_closed(&self) -> bool {
        self.curve.is_closed()
    }

    /// Plane coordinates out into the world.
    ///
    /// The XY-aligned shortcut is the render path's inner loop, and covers
    /// every planar entity stored with the default +Z extrusion whatever its
    /// elevation. Correctness does not depend on the branch — the general arm
    /// produces the same points.
    fn lower(&self, uv: [f64; 2]) -> [f64; 3] {
        let origin = self.plane.origin;
        if self.plane.is_xy_aligned() {
            return [origin[0] + uv[0], origin[1] + uv[1], origin[2]];
        }
        self.plane.point_at(uv)
    }

    /// World coordinates into the plane's.
    fn lift(&self, point: [f64; 3]) -> Option<[f64; 2]> {
        let origin = self.plane.origin;
        if self.plane.is_xy_aligned() {
            return Some([point[0] - origin[0], point[1] - origin[1]]);
        }
        self.plane.project(point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::{Arc, Line};
    use std::f64::consts::{FRAC_PI_2, TAU};

    fn unit_arc() -> Curve {
        Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 1.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        })
    }

    /// The XZ plane: X stays X, the curve's Y becomes world Z. An entity with
    /// an extrusion normal of +Y is stored this way.
    fn upright() -> Plane {
        Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]).unwrap()
    }

    #[test]
    fn a_flat_curve_keeps_its_coordinates_and_gains_a_zero() {
        let curve = PlanarCurve::flat(Curve::Line(Line {
            start: [1.0, 2.0],
            end: [4.0, 6.0],
        }));
        assert_eq!(curve.point_at(0.0), [1.0, 2.0, 0.0]);
        assert_eq!(curve.point_at(1.0), [4.0, 6.0, 0.0]);
        assert_eq!(curve.tessellate(20.0), vec![[1.0, 2.0, 0.0], [4.0, 6.0, 0.0]]);
    }

    #[test]
    fn a_frame_moves_the_curve_without_reparameterising_it() {
        let flat = PlanarCurve::flat(unit_arc());
        let raised = PlanarCurve::new(upright(), unit_arc());
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            let (a, b) = (flat.point_at(t), raised.point_at(t));
            // Same X and same distance from the centre; only which axis the
            // second coordinate points along has changed.
            assert!((a[0] - b[0]).abs() < 1e-12);
            assert!((a[1] - b[2]).abs() < 1e-12, "t={t}: {a:?} vs {b:?}");
            assert!(b[1].abs() < 1e-12);
        }
    }

    #[test]
    fn the_general_arm_agrees_with_the_xy_shortcut() {
        // Guards the branch in `lower`: the fast path must not be a different
        // answer, only a cheaper one. Both planes here are the same plane —
        // one written in the basis the shortcut recognises, one rotated a
        // full turn so it is not.
        let fast_frame = Plane::from_axes([7.0, -3.0, 2.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let slow_frame = Plane::orthonormal([7.0, -3.0, 2.5], [1.0, 1e-17, 0.0], [0.0, 0.0, 1.0])
            .expect("still a plane");
        assert!(fast_frame.is_xy_aligned());
        assert!(!slow_frame.is_xy_aligned(), "this test needs the long way");

        let fast = PlanarCurve::new(fast_frame, unit_arc()).tessellate(20.0);
        let slow = PlanarCurve::new(slow_frame, unit_arc()).tessellate(20.0);
        assert_eq!(fast.len(), slow.len());
        for (a, b) in fast.iter().zip(slow.iter()) {
            assert!(
                (a[0] - b[0]).abs() < 1e-12
                    && (a[1] - b[1]).abs() < 1e-12
                    && (a[2] - b[2]).abs() < 1e-12,
                "{a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn an_elevated_xy_plane_still_takes_the_shortcut() {
        // The case the shortcut exists for: a drawing's planar entities are
        // stored +Z-up and differ only in elevation.
        let plane = Plane::from_axes([0.0, 0.0, 12.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(plane.is_xy_aligned() && !plane.is_xy());
        let curve = PlanarCurve::new(plane, unit_arc());
        assert_eq!(curve.point_at(0.0), [1.0, 0.0, 12.0]);
        assert!(curve.parameter_at([1.0, 0.0, 12.0]).unwrap().abs() < 1e-12);
    }

    #[test]
    fn parameter_at_inverts_point_at_through_the_frame() {
        let curve = PlanarCurve::new(upright(), unit_arc());
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            let back = curve.parameter_at(curve.point_at(t)).unwrap();
            assert!((back - t).abs() < 1e-9, "t={t} came back as {back}");
        }
    }

    #[test]
    fn a_point_off_the_plane_is_projected_onto_it_first() {
        let curve = PlanarCurve::new(upright(), unit_arc());
        // On the arc at t=0, but pushed a metre along the normal.
        let t = curve.parameter_at([1.0, -1.0, 0.0]).unwrap();
        assert!(t.abs() < 1e-9, "got {t}");
    }

    #[test]
    fn a_degenerate_plane_refuses_rather_than_guessing() {
        let flat = Plane::from_axes([0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]);
        let curve = PlanarCurve::new(flat, unit_arc());
        assert!(curve.parameter_at([0.0; 3]).is_none());
        assert!(curve.normal().is_none());
    }

    #[test]
    fn the_shape_answers_for_closure_and_extent_not_the_frame() {
        let arc = PlanarCurve::new(upright(), unit_arc());
        assert!(!arc.is_closed());
        assert_eq!(arc.extent(), Extent::Bounded);

        let circle = PlanarCurve::new(
            upright(),
            Curve::Arc(Arc {
                centre: [0.0, 0.0],
                radius: 1.0,
                start_angle: 0.0,
                end_angle: TAU,
            }),
        );
        assert!(circle.is_closed());
    }

    #[test]
    fn the_normal_comes_from_the_plane() {
        assert_eq!(
            PlanarCurve::flat(unit_arc()).normal(),
            Some([0.0, 0.0, 1.0])
        );
        assert_eq!(
            PlanarCurve::new(upright(), unit_arc()).normal(),
            Some([0.0, -1.0, 0.0])
        );
    }

    #[test]
    fn distance_along_a_tilted_curve_is_measured_in_world_units() {
        // A quarter of a unit circle, standing upright. Its length is π/2
        // whichever way the plane faces, and half of that lands at 45°.
        let flat = PlanarCurve::flat(unit_arc());
        let raised = PlanarCurve::new(upright(), unit_arc());
        assert!((flat.length() - FRAC_PI_2).abs() < 1e-12);
        assert!((raised.length() - FRAC_PI_2).abs() < 1e-12);

        let half = raised.point_at_distance(FRAC_PI_2 * 0.5);
        let root_half = std::f64::consts::FRAC_1_SQRT_2;
        assert!((half[0] - root_half).abs() < 1e-9, "{half:?}");
        assert!((half[2] - root_half).abs() < 1e-9, "{half:?}");
    }

    #[test]
    fn the_tangent_comes_out_in_world_coordinates() {
        // At the start of the upright arc the curve heads along its plane's
        // own +v, which world-side is +Z.
        let raised = PlanarCurve::new(upright(), unit_arc());
        let tangent = raised.tangent_at(0.0);
        let length = (tangent[0] * tangent[0]
            + tangent[1] * tangent[1]
            + tangent[2] * tangent[2])
            .sqrt();
        assert!((tangent[2] / length - 1.0).abs() < 1e-9, "{tangent:?}");
    }

    #[test]
    fn a_tolerance_scales_the_point_count_with_the_size() {
        // The reason this exists beside `tessellate`: the density form gives
        // both of these the same polyline.
        let small = PlanarCurve::flat(Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 1.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        }));
        let large = PlanarCurve::flat(Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 10_000.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        }));
        assert_eq!(small.tessellate(20.0).len(), large.tessellate(20.0).len());
        assert!(large.tessellate_within(0.01).len() > small.tessellate_within(0.01).len() * 20);
    }

    #[test]
    fn a_tolerance_sampling_lands_on_the_plane_too() {
        let plane = Plane::orthonormal([3.0, 4.0, 5.0], [1.0, 1.0, 0.0], [0.0, 1.0, 1.0]).unwrap();
        let curve = PlanarCurve::new(plane, unit_arc());
        let points = curve.tessellate_within(0.001);
        assert!(points.len() > 8);
        for point in points {
            assert!(plane.contains(point, 1e-9), "{point:?}");
        }
    }

    #[test]
    fn tessellating_stays_on_the_plane() {
        let plane = Plane::orthonormal([3.0, 4.0, 5.0], [1.0, 1.0, 0.0], [0.0, 1.0, 1.0]).unwrap();
        let curve = PlanarCurve::new(plane, unit_arc());
        for point in curve.tessellate(20.0) {
            assert!(
                plane.contains(point, 1e-9),
                "{point:?} left the plane by {:?}",
                plane.distance_to(point)
            );
        }
    }
}

/// Infer one plane containing the exact affine support of all supplied curves.
/// Disconnected curves may share a plane; straight segments do not impose their
/// arbitrary storage planes. Returns None for collinear or noncoplanar inputs.
pub fn common_curve_plane(curves: &[PlanarCurve], tolerance: f64) -> Option<Plane> {
    use super::Vec3;
    if !tolerance.is_finite() || tolerance <= 0.0 { return None; }
    let mut points = Vec::new();
    for curve in curves {
        match &curve.curve {
            Curve::Line(_) | Curve::Ray(_) | Curve::XLine(_) => {
                points.push(curve.point_at(0.0));
                points.push(curve.point_at(1.0));
            }
            Curve::Polyline(polyline) if polyline.vertices.iter().all(|vertex| vertex.bulge == 0.0) => {
                points.extend(polyline.vertices.iter().map(|vertex| curve.plane.point_at(vertex.position)));
            }
            Curve::Nurbs(nurbs) => {
                points.extend(nurbs.control_points().iter().map(|point| curve.plane.point_at(*point)));
            }
            _ => {
                // A nondegenerate curved conic or bulged polyline spans its storage plane.
                points.extend([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]].map(|uv| curve.plane.point_at(uv)));
            }
        }
    }
    if !points.iter().flatten().all(|value| value.is_finite()) { return None; }
    let origin = Vec3::from(*points.first()?);
    let offsets: Vec<_> = points.iter().map(|point| Vec3::from(*point) - origin).collect();
    let axis = offsets.iter().copied().max_by(|a,b| a.length_squared().total_cmp(&b.length_squared()))?;
    if axis.length() <= tolerance { return None; }
    let x = axis.normalize()?;
    let cross = offsets.iter().map(|offset| x.cross(*offset))
        .max_by(|a,b| a.length_squared().total_cmp(&b.length_squared()))?;
    if cross.length() <= tolerance { return None; }
    let normal = cross.normalize()?;
    let roundoff = f64::EPSILON * points.iter().flatten().map(|value| value.abs()).fold(1.0, f64::max) * 64.0;
    if offsets.iter().any(|offset| offset.dot(normal).abs() > tolerance + roundoff) { return None; }
    Plane::orthonormal(origin.to_array(), x.to_array(), normal.to_array())
}

#[cfg(test)]
mod common_plane_tests {
    use super::*;
    use crate::geom2d::Line;

    #[test]
    fn disconnected_lines_in_one_plane_share_that_plane() {
        let curves = [
            PlanarCurve::flat(Curve::Line(Line { start: [0.0, 0.0], end: [2.0, 0.0] })),
            PlanarCurve::flat(Curve::Line(Line { start: [0.0, 1.0], end: [0.0, 3.0] })),
        ];
        let plane = common_curve_plane(&curves, 1e-9).unwrap();
        assert!(curves.iter().flat_map(|curve| [curve.point_at(0.0), curve.point_at(1.0)])
            .all(|point| plane.contains(point, 1e-9)));
    }

    #[test]
    fn noncoplanar_curve_support_is_rejected() {
        let raised = Plane::from_axes(
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        );
        let curves = [
            PlanarCurve::flat(Curve::Line(Line { start: [0.0, 0.0], end: [2.0, 0.0] })),
            PlanarCurve::flat(Curve::Line(Line { start: [0.0, 0.0], end: [0.0, 2.0] })),
            PlanarCurve::new(raised, Curve::Line(Line { start: [0.0, 0.0], end: [1.0, 1.0] })),
        ];
        assert!(common_curve_plane(&curves, 1e-9).is_none());
    }
}
