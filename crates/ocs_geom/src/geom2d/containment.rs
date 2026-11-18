//! Nearest points, and what is inside what.
//!
//! Two questions that look unrelated and share their machinery. Snapping asks
//! the first — where on this curve is the cursor, how far. Hit testing,
//! selection and hatch filling ask the second — is this point inside that
//! boundary.
//!
//! Both are answered from the curve dispatch rather than by sampling. Nearest
//! points come from [`Curve::parameter_at`] pulled back into the curve's own
//! extent; containment comes from casting a ray and counting what it crosses,
//! which reuses the intersection dispatch and so inherits its exactness.

use super::cross::intersect;
use super::curve::{Curve, Extent, Ray};
use super::vec::Vec2;
use super::Tolerance;

/// The point on a curve nearest to somewhere else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Closest {
    /// Where it falls on the curve, in the curve's own parameter.
    pub t: f64,
    /// The point itself.
    pub point: [f64; 2],
    /// How far it was from what was asked about.
    pub distance: f64,
}

/// The point on `curve` nearest to `to`.
///
/// The result is always *on* the curve: a point past the end of a segment
/// comes back as that segment's endpoint rather than as somewhere on its
/// extension, which is what a snap or a hit test means by nearest. A ray keeps
/// its origin as a floor for the same reason, and an infinite line has no ends
/// to be pulled back to.
pub fn closest_point(curve: &Curve, to: [f64; 2]) -> Closest {
    let at = |t: f64| {
        let point = curve.point_at(t);
        Closest {
            t,
            point,
            distance: Vec2::from(point).distance(Vec2::from(to)),
        }
    };

    let raw = curve.parameter_at(to);
    match curve.extent() {
        Extent::Infinite => at(raw),
        Extent::Forward => at(raw.max(0.0)),
        Extent::Bounded => {
            // Pulling the parameter back into range is not enough on its own.
            // An arc reports a parameter that is already clamped, so a point
            // sitting just before its start comes back as the far end — near
            // in parameter, nowhere near in space. Whenever the answer is an
            // endpoint it has to be the nearer endpoint, so both are measured.
            let interior = at(raw.clamp(0.0, 1.0));
            [interior, at(0.0), at(1.0)]
                .into_iter()
                .reduce(|best, next| if next.distance < best.distance { next } else { best })
                .expect("three candidates")
        }
    }
}

/// How far `point` is from the nearest place on `curve`.
pub fn distance_to(curve: &Curve, point: [f64; 2]) -> f64 {
    closest_point(curve, point).distance
}

/// Whichever of `curves` passes nearest to `point`, with where on it.
///
/// `None` for an empty slice. Ties go to the earlier curve, so a caller that
/// has ordered its candidates by preference gets that preference back.
pub fn nearest_of<'a>(
    curves: impl IntoIterator<Item = &'a Curve>,
    point: [f64; 2],
) -> Option<(usize, Closest)> {
    curves
        .into_iter()
        .enumerate()
        .map(|(index, curve)| (index, closest_point(curve, point)))
        .reduce(|best, next| if next.1.distance < best.1.distance { next } else { best })
}

/// Directions tried when casting a containment ray, as unit vectors.
///
/// A ray that grazes a vertex or runs along a tangent counts a crossing twice
/// or not at all, and no single direction avoids that for every boundary. These
/// are mutually unrelated angles, so a boundary that is degenerate for one is
/// almost certainly not for the next.
const CAST_DIRECTIONS: [[f64; 2]; 4] = [
    [1.0, 0.0],
    [0.523_47, 0.852_04],
    [-0.707_106_781_186_547_5, 0.707_106_781_186_547_5],
    [0.317_37, -0.948_31],
];

/// Whether `point` lies inside the region `boundary` encloses.
///
/// The boundary is given as the curves that make it up, in any order — a
/// single closed curve, or the pieces of a hatch loop. A point on the boundary
/// itself counts as inside, since a pick that lands on the edge is asking to
/// be let in.
///
/// Works by counting how many times a ray from the point crosses the boundary:
/// an odd number means inside. If the ray runs into a degeneracy — through a
/// corner where two pieces meet, or along a tangent — the count is unreliable,
/// so another direction is tried.
pub fn contains(boundary: &[Curve], point: [f64; 2], tolerance: Tolerance) -> bool {
    if boundary.is_empty() {
        return false;
    }
    // Each piece's box, grown by the tolerance: a face with a hundred holes
    // asks only the few a ray or the point comes near, not every one.
    let boxes: Vec<Option<[[f64; 2]; 2]>> = boundary
        .iter()
        .map(|curve| {
            curve_box(curve).map(|[low, high]| {
                let grow = tolerance.linear();
                [[low[0] - grow, low[1] - grow], [high[0] + grow, high[1] + grow]]
            })
        })
        .collect();
    // On the edge is in.
    if boundary.iter().zip(&boxes).any(|(curve, bounds)| {
        bounds.is_none_or(|[low, high]| {
            (0..2).all(|axis| point[axis] >= low[axis] && point[axis] <= high[axis])
        }) && distance_to(curve, point) <= tolerance.linear()
    }) {
        return true;
    }

    for direction in CAST_DIRECTIONS {
        let reached: Vec<&Curve> = boundary
            .iter()
            .zip(&boxes)
            .filter(|(_, bounds)| {
                bounds.is_none_or(|bounds| ray_meets_box(point, direction, &bounds))
            })
            .map(|(curve, _)| curve)
            .collect();
        if let Some(inside) = cast(&reached, point, direction, tolerance) {
            return inside;
        }
    }
    // Every direction was degenerate, which takes a boundary built to defeat
    // this. Outside is the safer answer: it declines to fill or select rather
    // than doing so wrongly.
    false
}

/// A box round a curve, or `None` where there is none to hand: a circle's,
/// an arc's or an ellipse's whole round, a line's ends, a spline's control
/// points with positive weights.
fn curve_box(curve: &Curve) -> Option<[[f64; 2]; 2]> {
    let around = |points: &mut dyn Iterator<Item = [f64; 2]>| {
        points.fold(None, |bounds: Option<[[f64; 2]; 2]>, p| {
            Some(match bounds {
                None => [p, p],
                Some([low, high]) => {
                    [[low[0].min(p[0]), low[1].min(p[1])], [high[0].max(p[0]), high[1].max(p[1])]]
                }
            })
        })
    };
    let round = |centre: [f64; 2], radius: f64| {
        let radius = radius.abs();
        Some([[centre[0] - radius, centre[1] - radius], [centre[0] + radius, centre[1] + radius]])
    };
    match curve {
        Curve::Line(line) => around(&mut [line.start, line.end].into_iter()),
        Curve::Circle(circle) => round(circle.centre, circle.radius),
        Curve::Arc(arc) => round(arc.centre, arc.radius),
        Curve::Ellipse(arc) => {
            let ellipse = &arc.ellipse;
            round(ellipse.centre, ellipse.major_radius.abs().max(ellipse.minor_radius.abs()))
        }
        Curve::Nurbs(spline) if spline.weights().iter().all(|weight| *weight > 0.0) => {
            around(&mut spline.control_points().iter().copied())
        }
        _ => None,
    }
}

/// Whether a ray from `origin` along `direction` reaches a box.
fn ray_meets_box(origin: [f64; 2], direction: [f64; 2], [low, high]: &[[f64; 2]; 2]) -> bool {
    let (mut near, mut far) = (0.0f64, f64::INFINITY);
    for axis in 0..2 {
        if direction[axis].abs() <= f64::EPSILON {
            if origin[axis] < low[axis] || origin[axis] > high[axis] {
                return false;
            }
            continue;
        }
        let a = (low[axis] - origin[axis]) / direction[axis];
        let b = (high[axis] - origin[axis]) / direction[axis];
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    near <= far
}

/// One ray cast. `None` when the result cannot be trusted.
fn cast(
    boundary: &[&Curve],
    point: [f64; 2],
    direction: [f64; 2],
    tolerance: Tolerance,
) -> Option<bool> {
    let ray = Curve::Ray(Ray {
        origin: point,
        direction,
    });
    let mut hits: Vec<Vec2> = Vec::new();
    for &curve in boundary {
        // Keep polyline corners visible to the endpoint-degeneracy check.
        let segments = curve.segments();
        let pieces = if matches!(curve, Curve::Polyline(_)) {
            segments.as_slice()
        } else {
            std::slice::from_ref(curve)
        };
        for piece in pieces {
            // A straight piece the ray runs along reports no crossing, and
            // its neighbours' ends a hair either side of the ray then decide
            // the count by which side of the hair they were put.
            if let Curve::Line(line) = piece {
                let along = Vec2::from(line.end) - Vec2::from(line.start);
                let offset = Vec2::from(line.start) - Vec2::from(point);
                if along.cross(Vec2::from(direction)).abs() <= 1e-12 * along.length()
                    && offset.cross(Vec2::from(direction)).abs() <= tolerance.linear()
                {
                    return None;
                }
            }
            for crossing in intersect(&ray, piece, tolerance) {
                let at_end = crossing.t_b <= 1e-6 || crossing.t_b >= 1.0 - 1e-6;
                let tangent = Vec2::from(piece.tangent_at(crossing.t_b));
                // A corner or tangent touch needs a different casting direction.
                if (at_end && !piece.is_closed())
                    || tangent.cross(Vec2::from(direction)).abs() <= 1e-10 * tangent.length()
                {
                    return None;
                }
                hits.push(Vec2::from(crossing.point));
            }
        }
    }

    // Two pieces meeting at a corner both report the same point; counting it
    // once is right, but it also means the ray grazed rather than passed
    // through, so the parity is not to be trusted either.
    let mut unique: Vec<Vec2> = Vec::with_capacity(hits.len());
    for hit in hits {
        if unique
            .iter()
            .any(|kept| kept.distance(hit) <= tolerance.linear())
        {
            return None;
        }
        unique.push(hit);
    }

    Some(unique.len() % 2 == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::curve::{Arc, Circle, Line, XLine};
    use crate::geom2d::polyline::{Polyline, PolylineVertex};
    use std::f64::consts::FRAC_PI_2;

    fn tol() -> Tolerance {
        Tolerance::new(1e-6)
    }

    fn segment(start: [f64; 2], end: [f64; 2]) -> Curve {
        Curve::Line(Line { start, end })
    }

    /// A 10 x 6 rectangle at the origin, as one closed polyline.
    fn rectangle() -> Curve {
        Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([10.0, 0.0]),
                PolylineVertex::straight([10.0, 6.0]),
                PolylineVertex::straight([0.0, 6.0]),
            ],
            closed: true,
        })
    }

    #[test]
    fn the_nearest_point_on_a_segment_is_the_foot_of_the_perpendicular() {
        let found = closest_point(&segment([0.0, 0.0], [10.0, 0.0]), [4.0, 3.0]);
        assert!((found.point[0] - 4.0).abs() < 1e-12 && found.point[1].abs() < 1e-12);
        assert!((found.distance - 3.0).abs() < 1e-12);
        assert!((found.t - 0.4).abs() < 1e-12);
    }

    #[test]
    fn a_point_past_a_segment_snaps_to_its_end() {
        // Not to somewhere on the line's extension, which is what a snap or a
        // hit test would find useless.
        let found = closest_point(&segment([0.0, 0.0], [10.0, 0.0]), [50.0, 0.0]);
        assert_eq!(found.t, 1.0);
        assert!((found.point[0] - 10.0).abs() < 1e-12);
        assert!((found.distance - 40.0).abs() < 1e-12);
    }

    #[test]
    fn a_ray_keeps_its_origin_as_a_floor_but_no_ceiling() {
        let ray = Curve::Ray(Ray {
            origin: [0.0, 0.0],
            direction: [1.0, 0.0],
        });
        // Behind the origin: pulled back to it.
        assert_eq!(closest_point(&ray, [-5.0, 0.0]).t, 0.0);
        // Far ahead: kept, since a ray goes there.
        assert!((closest_point(&ray, [500.0, 0.0]).t - 500.0).abs() < 1e-9);
    }

    #[test]
    fn an_infinite_line_has_no_end_to_be_pulled_back_to() {
        let line = Curve::XLine(XLine {
            base: [0.0, 0.0],
            direction: [1.0, 0.0],
        });
        assert!(closest_point(&line, [-500.0, 3.0]).t < 0.0);
        assert!((distance_to(&line, [-500.0, 3.0]) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn the_nearest_point_on_a_circle_is_along_the_radius() {
        let circle = Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 5.0,
        });
        assert!((distance_to(&circle, [10.0, 0.0]) - 5.0).abs() < 1e-9);
        // Inside counts as near too — three units short of the rim.
        assert!((distance_to(&circle, [2.0, 0.0]) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_point_beyond_an_arc_snaps_to_the_nearer_end() {
        // Quarter arc in the first quadrant; the point sits below the start.
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 5.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        });
        let found = closest_point(&arc, [5.0, -10.0]);
        assert!((found.point[0] - 5.0).abs() < 1e-6 && found.point[1].abs() < 1e-6);
    }

    #[test]
    fn the_nearest_of_several_curves_is_reported_with_its_index() {
        let curves = [
            segment([0.0, 10.0], [10.0, 10.0]),
            segment([0.0, 0.0], [10.0, 0.0]),
            segment([0.0, -20.0], [10.0, -20.0]),
        ];
        let (index, found) = nearest_of(curves.iter(), [5.0, 1.0]).unwrap();
        assert_eq!(index, 1);
        assert!((found.distance - 1.0).abs() < 1e-12);
        assert!(nearest_of([].iter(), [0.0, 0.0]).is_none());
    }

    #[test]
    fn a_point_inside_a_rectangle_is_inside() {
        assert!(contains(&[rectangle()], [5.0, 3.0], tol()));
        assert!(contains(&[rectangle()], [0.5, 0.5], tol()));
    }

    #[test]
    fn a_point_outside_a_rectangle_is_outside() {
        assert!(!contains(&[rectangle()], [-1.0, 3.0], tol()));
        assert!(!contains(&[rectangle()], [5.0, 20.0], tol()));
        assert!(!contains(&[rectangle()], [50.0, 50.0], tol()));
    }

    #[test]
    fn a_point_on_the_boundary_counts_as_inside() {
        // A pick that lands on the edge is asking to be let in.
        assert!(contains(&[rectangle()], [5.0, 0.0], tol()));
        assert!(contains(&[rectangle()], [0.0, 0.0], tol()), "a corner too");
    }

    #[test]
    fn a_boundary_given_as_separate_pieces_works_the_same() {
        // The same rectangle, as four segments rather than one polyline —
        // which is how a hatch loop arrives.
        let pieces = [
            segment([0.0, 0.0], [10.0, 0.0]),
            segment([10.0, 0.0], [10.0, 6.0]),
            segment([10.0, 6.0], [0.0, 6.0]),
            segment([0.0, 6.0], [0.0, 0.0]),
        ];
        assert!(contains(&pieces, [5.0, 3.0], tol()));
        assert!(!contains(&pieces, [15.0, 3.0], tol()));
    }

    #[test]
    fn a_circle_contains_what_is_within_its_radius() {
        let circle = Curve::Circle(Circle {
            centre: [3.0, 4.0],
            radius: 2.0,
        });
        assert!(contains(std::slice::from_ref(&circle), [3.0, 4.0], tol()));
        assert!(contains(std::slice::from_ref(&circle), [4.5, 4.0], tol()));
        assert!(!contains(std::slice::from_ref(&circle), [6.0, 4.0], tol()));
        assert!(!contains(std::slice::from_ref(&circle), [-1.0, 6.0], tol()), "a tangent ray does not enter the circle");
    }

    #[test]
    fn a_point_level_with_a_corner_is_still_judged_correctly() {
        // A ray cast straight out from here grazes two corners at once, which
        // is exactly the case a single fixed direction gets wrong.
        assert!(contains(&[rectangle()], [5.0, 6.0 - 1e-9], tol()));
        assert!(!contains(&[rectangle()], [-5.0, 6.0], tol()));
        assert!(!contains(&[rectangle()], [-5.0, 0.0], tol()));
    }

    #[test]
    fn a_concave_boundary_excludes_its_notch() {
        // An L shape: the notch in the top right is outside.
        let l_shape = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([10.0, 0.0]),
                PolylineVertex::straight([10.0, 4.0]),
                PolylineVertex::straight([4.0, 4.0]),
                PolylineVertex::straight([4.0, 10.0]),
                PolylineVertex::straight([0.0, 10.0]),
            ],
            closed: true,
        });
        assert!(contains(std::slice::from_ref(&l_shape), [2.0, 8.0], tol()), "in the arm");
        assert!(contains(std::slice::from_ref(&l_shape), [8.0, 2.0], tol()), "in the foot");
        assert!(!contains(std::slice::from_ref(&l_shape), [8.0, 8.0], tol()), "in the notch");
        assert!(!contains(std::slice::from_ref(&l_shape), [-1.0, 4.0], tol()), "outside, level with the notch");
    }

    #[test]
    fn an_empty_boundary_contains_nothing() {
        assert!(!contains(&[], [0.0, 0.0], tol()));
    }

    #[test]
    fn survey_coordinates_are_judged_the_same() {
        let origin = [512_345.678, 4_512_345.678];
        let far = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::straight(origin),
                PolylineVertex::straight([origin[0] + 10.0, origin[1]]),
                PolylineVertex::straight([origin[0] + 10.0, origin[1] + 6.0]),
                PolylineVertex::straight([origin[0], origin[1] + 6.0]),
            ],
            closed: true,
        });
        assert!(contains(std::slice::from_ref(&far), [origin[0] + 5.0, origin[1] + 3.0], tol()));
        assert!(!contains(std::slice::from_ref(&far), [origin[0] - 5.0, origin[1] + 3.0], tol()));
    }
}
