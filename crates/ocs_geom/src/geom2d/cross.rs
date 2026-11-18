//! Where any two curves meet.
//!
//! The per-shape functions in [`intersect`](super::intersect) answer one pair
//! each and speak in whatever parameters that pair naturally produces. This
//! module is the dispatch over [`Curve`], and it answers in the shared `0..=1`
//! parameter, so a caller holding two curves whose types it learns at runtime
//! does not need a match of its own.
//!
//! # How the pairs are solved
//!
//! Straight against straight, circular or elliptical, and circle-or-arc
//! against circle-or-arc, are solved in closed form.
//!
//! A polyline is taken apart and each of its segments solved exactly, rather
//! than being approximated as a whole — its pieces are lines and arcs, which
//! this already answers precisely.
//!
//! What is left is an ellipse or a NURBS curve against something else curved,
//! where no closed form is worth having. Those are subdivided: the parameter
//! range of each curve is halved until both pieces are straight to within
//! tolerance, with pairs whose bounds cannot overlap discarded on the way
//! down, and every hit that survives is then refined against the true curves.
//!
//! So there is no sampling step and no density to choose. Every crossing this
//! returns is either exact or converged to [`Tolerance`], which is what makes
//! the tolerance argument mean something.
//!
//! # How a crossing is kept
//!
//! Each pair produces candidate *points*. Both parameters then come from
//! [`Curve::parameter_at`], and the candidate survives only if it genuinely
//! lies on both curves — the parameter is inside the curve's own extent, and
//! evaluating it lands back where the candidate was. That second check is what
//! rejects a point on an arc's circle but outside the arc, without every case
//! having to test containment for itself.

use super::curve::{Curve, Extent};
use super::intersect::{circle_circle_points, ellipse_circle, line_circle, line_ellipse, line_line};
use super::vec::Vec2;
use super::Tolerance;

/// How deep the subdivision may go before giving up on a branch.
///
/// Halving 40 times takes a parameter interval below 1e-12 of the curve, which
/// is past the point where `f64` parameters distinguish anything. A branch that
/// has not resolved by then is a tangency the refinement will handle or a
/// degeneracy no depth would fix.
const MAX_DEPTH: usize = 40;

/// A point where two curves meet, with where it falls on each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// Where they meet.
    pub point: [f64; 2],
    /// Parameter on the first curve, `0..=1`.
    pub t_a: f64,
    /// Parameter on the second.
    pub t_b: f64,
}

/// Every point where `a` and `b` cross, ordered along `a`.
///
/// Solved in a translation-only frame near the two curves (see
/// [`super::frame`]): the closed forms survive survey coordinates, but the
/// converged path for splines does not — at 1e6 its Newton steps lose the
/// digits the tolerance test needs and crossings drop out. A translation
/// leaves every curve parameter as it was, so only the points move back.
pub fn intersect(a: &Curve, b: &Curve, tolerance: Tolerance) -> Vec<Crossing> {
    let anchors = [a.point_at(0.0), b.point_at(0.0)].map(|p| [p[0], p[1], 0.0]);
    let origin = super::Frame::around(anchors.iter()).origin();
    if origin[0].abs().max(origin[1].abs()) < 1e3 {
        return intersect_local(a, b, tolerance);
    }
    let shift = super::Transform::translation([-origin[0], -origin[1]]);
    let (Some(local_a), Some(local_b)) = (a.transformed(&shift), b.transformed(&shift)) else {
        return intersect_local(a, b, tolerance);
    };
    let mut crossings = intersect_local(&local_a, &local_b, tolerance);
    for crossing in &mut crossings {
        crossing.point = [crossing.point[0] + origin[0], crossing.point[1] + origin[1]];
    }
    crossings
}

fn intersect_local(a: &Curve, b: &Curve, tolerance: Tolerance) -> Vec<Crossing> {
    let mut crossings: Vec<Crossing> = candidates(a, b, tolerance)
        .into_iter()
        .filter_map(|point| {
            let t_a = on_curve(a, point, tolerance)?;
            let t_b = on_curve(b, point, tolerance)?;
            Some(Crossing { point, t_a, t_b })
        })
        .collect();

    crossings.sort_by(|x, y| x.t_a.partial_cmp(&y.t_a).unwrap_or(std::cmp::Ordering::Equal));
    crossings.dedup_by(|x, y| Vec2::from(x.point).distance(Vec2::from(y.point)) <= tolerance.linear());
    crossings
}

/// Local shorthand for [`Vec2::distance`], since this file asks for it seven
/// times with expressions for arguments.
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    Vec2::from(a).distance(Vec2::from(b))
}

/// The parameter at `point` if it really lies on `curve`, else `None`.
fn on_curve(curve: &Curve, point: [f64; 2], tolerance: Tolerance) -> Option<f64> {
    let t = curve.parameter_at(point);
    // A parameter outside the curve's extent means the crossing is on its
    // extension, not on it. A ray is the case a plain "bounded or not" would
    // get wrong: it has to reject what lies behind its origin while accepting
    // anything ahead.
    if !curve.is_closed() && !curve.extent().holds(t) {
        return None;
    }
    // Evaluating the parameter has to land back where we started. For a point
    // on an arc's circle but outside the arc, `parameter_at` clamps and this
    // is what catches it.
    if distance(curve.point_at(t), point) > tolerance.linear() {
        return None;
    }
    Some(match curve.extent() {
        Extent::Bounded => t.clamp(0.0, 1.0),
        _ => t,
    })
}

/// Candidate crossing points: exact where a closed form exists, converged
/// where one does not.
fn candidates(a: &Curve, b: &Curve, tolerance: Tolerance) -> Vec<[f64; 2]> {
    use Curve::{Arc, Circle, Ellipse, Polyline};

    // Segments, rays and infinite lines share every closed form below: they
    // differ only in how far their parameter is allowed to run, which the
    // containment test settles afterwards.
    let round = |c: &Curve| matches!(c, Circle(_) | Arc(_));

    match (a, b) {
        _ if a.is_straight() && b.is_straight() => {
            let (p, d) = a.as_ray().expect("straight");
            let (q, e) = b.as_ray().expect("straight");
            line_line(p, d, q, e)
                .map(|(t, _)| vec![[p[0] + t * d[0], p[1] + t * d[1]]])
                .unwrap_or_default()
        }

        _ if a.is_straight() && round(b) => {
            let (p, d) = a.as_ray().expect("straight");
            let (centre, radius) = circle_of(b);
            line_circle(p, d, centre, radius)
                .into_iter()
                .map(|t| [p[0] + t * d[0], p[1] + t * d[1]])
                .collect()
        }
        _ if round(a) && b.is_straight() => candidates(b, a, tolerance),

        (_, Ellipse(arc)) if a.is_straight() => {
            let (p, d) = a.as_ray().expect("straight");
            line_ellipse(p, d, &arc.ellipse)
                .into_iter()
                .map(|(_, parameter)| arc.ellipse.point_at(parameter))
                .collect()
        }
        (Ellipse(_), _) if b.is_straight() => candidates(b, a, tolerance),

        _ if round(a) && round(b) => {
            let (c1, r1) = circle_of(a);
            let (c2, r2) = circle_of(b);
            circle_circle_points(c1, r1, c2, r2)
        }

        (Ellipse(arc), _) if round(b) => {
            let (centre, radius) = circle_of(b);
            ellipse_circle(&arc.ellipse, centre, radius)
                .into_iter()
                .map(|(_, parameter)| arc.ellipse.point_at(parameter))
                .collect()
        }
        (_, Ellipse(_)) if round(a) => candidates(b, a, tolerance),

        // A polyline is lines and arcs, all of which are answered exactly
        // above. Take it apart rather than approximating it whole — but only
        // the pieces near `b`: every pair of straight pieces meets somewhere
        // on their extensions, and two long chains handed each other's every
        // piece made a boolean between filleted parts take minutes.
        (Polyline(_), _) => {
            let near = super::bounds::analytic_curve_bounds(std::slice::from_ref(b));
            let reach = tolerance.linear();
            a.segments()
                .iter()
                .filter(|piece| {
                    let (Some((low, high)), Some((from, to))) =
                        (near, super::bounds::analytic_curve_bounds(std::slice::from_ref(*piece)))
                    else {
                        return true;
                    };
                    (0..2).all(|axis| {
                        from[axis] <= high[axis] + reach && to[axis] >= low[axis] - reach
                    })
                })
                .flat_map(|piece| candidates(piece, b, tolerance))
                .collect()
        }
        (_, Polyline(_)) => candidates(b, a, tolerance),

        // An ellipse or a NURBS curve against something else curved. No closed
        // form worth having, so subdivide down to straightness and refine.
        _ => subdivided(a, b, tolerance),
    }
}

/// The circle a circle or arc lies on.
fn circle_of(curve: &Curve) -> ([f64; 2], f64) {
    match curve {
        Curve::Circle(circle) => (circle.centre, circle.radius),
        Curve::Arc(arc) => (arc.centre, arc.radius),
        _ => unreachable!("circle_of is only called for circles and arcs"),
    }
}

/// A parameter interval on a curve, with the box and straightness of the piece
/// it covers.
struct Piece {
    from: f64,
    to: f64,
    start: [f64; 2],
    end: [f64; 2],
    /// Corners of the box holding the piece, allowing for how far it bows.
    low: [f64; 2],
    high: [f64; 2],
    /// Furthest the piece strays from the chord between its ends.
    bow: f64,
}

impl Piece {
    fn of(curve: &Curve, from: f64, to: f64) -> Self {
        let start = curve.point_at(from);
        let end = curve.point_at(to);
        // Three interior samples are enough to tell a straight piece from a
        // bowed one; the subdivision handles anything they understate by
        // halving again.
        let mut bow: f64 = 0.0;
        let mut low = [start[0].min(end[0]), start[1].min(end[1])];
        let mut high = [start[0].max(end[0]), start[1].max(end[1])];
        for step in 1..4 {
            let point = curve.point_at(from + (to - from) * step as f64 / 4.0);
            bow = bow.max(Vec2::from(point).distance_to_segment(Vec2::from(start), Vec2::from(end)));
            low = [low[0].min(point[0]), low[1].min(point[1])];
            high = [high[0].max(point[0]), high[1].max(point[1])];
        }
        Self {
            from,
            to,
            start,
            end,
            low: [low[0] - bow, low[1] - bow],
            high: [high[0] + bow, high[1] + bow],
            bow,
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.low[0] <= other.high[0]
            && other.low[0] <= self.high[0]
            && self.low[1] <= other.high[1]
            && other.low[1] <= self.high[1]
    }

    fn middle(&self) -> f64 {
        (self.from + self.to) * 0.5
    }
}

/// Halves the parameter ranges until both pieces are straight within
/// tolerance, then crosses them as segments and refines the result.
///
/// A straight curve is never subdivided. Splitting one would be pointless
/// work, and for a ray or an infinite line it would be wrong: their `0..=1`
/// piece is one unit long, so cutting them down would look for the crossing in
/// the wrong place entirely.
fn subdivided(a: &Curve, b: &Curve, tolerance: Tolerance) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    if let Some((origin, direction)) = a.as_ray() {
        against_straight(
            Vec2::from(origin),
            Vec2::from(direction),
            b,
            &Piece::of(b, 0.0, 1.0),
            a,
            tolerance,
            0,
            &mut out,
        );
    } else if let Some((origin, direction)) = b.as_ray() {
        against_straight(
            Vec2::from(origin),
            Vec2::from(direction),
            a,
            &Piece::of(a, 0.0, 1.0),
            b,
            tolerance,
            0,
            &mut out,
        );
    } else {
        descend(
            a,
            &Piece::of(a, 0.0, 1.0),
            b,
            &Piece::of(b, 0.0, 1.0),
            tolerance,
            0,
            &mut out,
        );
    }
    out
}

/// Subdivides only the curved side, keeping the straight one whole.
#[allow(clippy::too_many_arguments)]
fn against_straight(
    origin: Vec2,
    direction: Vec2,
    curve: &Curve,
    piece: &Piece,
    straight: &Curve,
    tolerance: Tolerance,
    depth: usize,
    out: &mut Vec<[f64; 2]>,
) {
    // Which side of the line each corner of the piece's box falls on. All on
    // one side means the line misses it.
    let side = |p: [f64; 2]| direction.cross(Vec2::from(p) - origin);
    let corners = [
        side([piece.low[0], piece.low[1]]),
        side([piece.high[0], piece.low[1]]),
        side([piece.low[0], piece.high[1]]),
        side([piece.high[0], piece.high[1]]),
    ];
    if corners.iter().all(|s| *s > 0.0) || corners.iter().all(|s| *s < 0.0) {
        return;
    }

    if depth >= MAX_DEPTH || piece.bow <= tolerance.linear() {
        let chord = (Vec2::from(piece.end) - Vec2::from(piece.start)).to_array();
        // The straight side's parameter is left unbounded — a ray or infinite
        // line reaches as far as it reaches, and `on_curve` decides afterwards
        // whether that is on it.
        let Some((_, u)) = line_line(origin.to_array(), direction.to_array(), piece.start, chord)
        else {
            return;
        };
        let slack = 1e-9;
        if !(-slack..=1.0 + slack).contains(&u) {
            return;
        }
        let guess = piece.from + u * (piece.to - piece.from);
        if let Some(point) = refine(curve, straight, guess, tolerance) {
            out.push(point);
        }
        return;
    }

    let middle = piece.middle();
    for half in [
        Piece::of(curve, piece.from, middle),
        Piece::of(curve, middle, piece.to),
    ] {
        against_straight(
            origin,
            direction,
            curve,
            &half,
            straight,
            tolerance,
            depth + 1,
            out,
        );
    }
}

fn descend(
    a: &Curve,
    piece_a: &Piece,
    b: &Curve,
    piece_b: &Piece,
    tolerance: Tolerance,
    depth: usize,
    out: &mut Vec<[f64; 2]>,
) {
    if !piece_a.overlaps(piece_b) {
        return;
    }

    let limit = tolerance.linear();
    if depth >= MAX_DEPTH || (piece_a.bow <= limit && piece_b.bow <= limit) {
        // Both pieces are chords now, so the exact segment crossing is the
        // answer — up to how far each piece bows, which the refinement below
        // takes out.
        let d = (Vec2::from(piece_a.end) - Vec2::from(piece_a.start)).to_array();
        let e = (Vec2::from(piece_b.end) - Vec2::from(piece_b.start)).to_array();
        let Some((t, u)) = line_line(piece_a.start, d, piece_b.start, e) else {
            return;
        };
        let slack = 1e-9;
        if !(-slack..=1.0 + slack).contains(&t) || !(-slack..=1.0 + slack).contains(&u) {
            return;
        }
        let guess = piece_a.from + t * (piece_a.to - piece_a.from);
        if let Some(point) = refine(a, b, guess, tolerance) {
            out.push(point);
        }
        return;
    }

    // Halve whichever piece bows more, so the work goes where the shape is.
    if piece_a.bow >= piece_b.bow {
        let middle = piece_a.middle();
        for half in [
            Piece::of(a, piece_a.from, middle),
            Piece::of(a, middle, piece_a.to),
        ] {
            descend(a, &half, b, piece_b, tolerance, depth + 1, out);
        }
    } else {
        let middle = piece_b.middle();
        for half in [
            Piece::of(b, piece_b.from, middle),
            Piece::of(b, middle, piece_b.to),
        ] {
            descend(a, piece_a, b, &half, tolerance, depth + 1, out);
        }
    }
}

/// Walks a rough crossing onto both curves by projecting back and forth.
///
/// Each step puts the point on one curve and then on the other; where they
/// genuinely cross, the two land on each other and it settles. Cheap, needs no
/// derivatives, and cannot run away the way Newton can near a tangency.
///
/// `None` when the point stops moving without the curves agreeing, which is
/// the near-miss case.
fn refine(a: &Curve, b: &Curve, guess: f64, tolerance: Tolerance) -> Option<[f64; 2]> {
    let limit = tolerance.linear();
    let mut point = a.point_at(guess);
    for _ in 0..40 {
        let on_b = b.point_at(b.parameter_at(point));
        let on_a = a.point_at(a.parameter_at(on_b));
        let gap = Vec2::from(on_a).distance(Vec2::from(on_b));
        let moved = Vec2::from(on_a).distance(Vec2::from(point));
        point = on_a;
        if gap <= limit {
            return Some(point);
        }
        if moved <= limit * 1e-3 {
            // Settled somewhere the curves do not meet.
            return None;
        }
    }
    (distance(a.point_at(a.parameter_at(point)), b.point_at(b.parameter_at(point))) <= limit)
        .then_some(point)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::curve::{
        Arc as ArcCurve, Circle as CircleShape, EllipseArc, Line as Seg, Ray as RayShape,
        XLine as XLineShape,
    };
    use crate::geom2d::nurbs::{NurbsCurve, Parameterization};
    use crate::geom2d::polyline::{Polyline as Chain, PolylineVertex};
    use crate::geom2d::Ellipse;
    use std::f64::consts::FRAC_PI_2;

    fn tol() -> Tolerance {
        Tolerance::new(1e-6)
    }

    fn segment(start: [f64; 2], end: [f64; 2]) -> Curve {
        Curve::Line(Seg { start, end })
    }

    fn circle(centre: [f64; 2], radius: f64) -> Curve {
        Curve::Circle(CircleShape { centre, radius })
    }

    fn ellipse(centre: [f64; 2], major: f64, minor: f64) -> Curve {
        Curve::Ellipse(EllipseArc::full(Ellipse {
            centre,
            major_radius: major,
            minor_radius: minor,
            major_axis: [1.0, 0.0],
        }))
    }

    #[test]
    fn two_crossing_segments_meet_once() {
        let hits = intersect(
            &segment([0.0, 0.0], [10.0, 0.0]),
            &segment([5.0, -5.0], [5.0, 5.0]),
            tol(),
        );
        assert_eq!(hits.len(), 1);
        assert!((hits[0].point[0] - 5.0).abs() < 1e-9);
        assert!((hits[0].t_a - 0.5).abs() < 1e-9);
        assert!((hits[0].t_b - 0.5).abs() < 1e-9);
    }

    #[test]
    fn segments_that_would_only_meet_if_extended_do_not_count() {
        let hits = intersect(
            &segment([0.0, 0.0], [10.0, 0.0]),
            &segment([50.0, -5.0], [50.0, 5.0]),
            tol(),
        );
        assert!(hits.is_empty(), "got {hits:?}");
    }

    #[test]
    fn a_segment_through_a_circle_meets_it_twice_in_order() {
        let hits = intersect(&segment([-10.0, 0.0], [10.0, 0.0]), &circle([0.0, 0.0], 5.0), tol());
        assert_eq!(hits.len(), 2);
        assert!(hits[0].t_a < hits[1].t_a);
        assert!((hits[0].point[0] + 5.0).abs() < 1e-6);
        assert!((hits[1].point[0] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn the_order_of_the_arguments_does_not_change_the_hits() {
        let line = segment([-10.0, 0.0], [10.0, 0.0]);
        let round = circle([0.0, 0.0], 5.0);
        let forward = intersect(&line, &round, tol());
        let backward = intersect(&round, &line, tol());
        assert_eq!(forward.len(), backward.len());
        for hit in &forward {
            assert!(
                backward
                    .iter()
                    .any(|other| Vec2::from(other.point).distance(Vec2::from(hit.point)) < 1e-6),
                "missing {hit:?}"
            );
        }
    }

    #[test]
    fn a_point_on_an_arcs_circle_but_off_the_arc_is_rejected() {
        let arc = Curve::Arc(ArcCurve {
            centre: [0.0, 0.0],
            radius: 5.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        });
        let hits = intersect(&segment([-10.0, 3.0], [10.0, 3.0]), &arc, tol());
        assert_eq!(hits.len(), 1, "got {hits:?}");
        assert!(hits[0].point[0] > 0.0);
    }

    #[test]
    fn two_circles_cross_at_two_points() {
        let hits = intersect(&circle([0.0, 0.0], 1.0), &circle([1.0, 0.0], 1.0), tol());
        assert_eq!(hits.len(), 2);
        for hit in hits {
            assert!((hit.point[0] - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn a_segment_across_an_ellipse_meets_it_twice() {
        let hits = intersect(
            &segment([-10.0, 1.0], [10.0, 1.0]),
            &ellipse([0.0, 0.0], 5.0, 2.0),
            tol(),
        );
        assert_eq!(hits.len(), 2, "got {hits:?}");
        for hit in hits {
            assert!((hit.point[1] - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn a_polyline_is_taken_apart_rather_than_approximated() {
        // A vertical line cuts the middle of three unit segments. Solved
        // segment by segment, the crossing is exact rather than converged.
        let chain = Curve::Polyline(Chain {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([1.0, 0.0]),
                PolylineVertex::straight([2.0, 0.0]),
                PolylineVertex::straight([3.0, 0.0]),
            ],
            closed: false,
        });
        let hits = intersect(&chain, &segment([1.5, -1.0], [1.5, 1.0]), tol());
        assert_eq!(hits.len(), 1, "got {hits:?}");
        assert!(
            (hits[0].point[0] - 1.5).abs() < 1e-12,
            "an exact crossing should be exact, got {}",
            hits[0].point[0]
        );
        assert!((hits[0].t_a - 0.5).abs() < 1e-9, "t_a = {}", hits[0].t_a);
    }

    #[test]
    fn a_polyline_arc_segment_is_solved_as_an_arc() {
        // Half circle from (0,0) to (2,0) dipping to (1,-1), cut by y = -0.5.
        let chain = Curve::Polyline(Chain {
            vertices: vec![
                PolylineVertex::curved([0.0, 0.0], 1.0),
                PolylineVertex::straight([2.0, 0.0]),
            ],
            closed: false,
        });
        let hits = intersect(&chain, &segment([-1.0, -0.5], [3.0, -0.5]), tol());
        assert_eq!(hits.len(), 2, "got {hits:?}");
        for hit in hits {
            // On the unit circle centred at (1, 0).
            let radius = distance(hit.point, [1.0, 0.0]);
            assert!((radius - 1.0).abs() < 1e-9, "off the arc: {radius}");
        }
    }

    #[test]
    fn two_ellipses_cross_where_they_should() {
        // Crossed 5x2 and 2x5 ellipses meet at four points, symmetric about
        // both axes.
        let hits = intersect(&ellipse([0.0, 0.0], 5.0, 2.0), &ellipse([0.0, 0.0], 2.0, 5.0), tol());
        assert_eq!(hits.len(), 4, "got {hits:?}");
        for hit in &hits {
            assert!(
                (hit.point[0].abs() - hit.point[1].abs()).abs() < 1e-5,
                "should sit on a diagonal: {:?}",
                hit.point
            );
        }
    }

    #[test]
    fn a_converged_crossing_lands_on_both_curves() {
        let a = ellipse([0.0, 0.0], 5.0, 2.0);
        let b = ellipse([3.0, 1.0], 4.0, 3.0);
        let hits = intersect(&a, &b, tol());
        assert!(!hits.is_empty());
        for hit in hits {
            assert!(distance(a.point_at(hit.t_a), hit.point) < 1e-5, "off a");
            assert!(distance(b.point_at(hit.t_b), hit.point) < 1e-5, "off b");
        }
    }

    #[test]
    fn a_tighter_tolerance_gives_a_closer_crossing() {
        // The knob has to do something: the same pair, solved twice.
        let a = ellipse([0.0, 0.0], 5.0, 2.0);
        let b = ellipse([0.0, 0.0], 2.0, 5.0);
        let error = |t: Tolerance| {
            intersect(&a, &b, t)
                .iter()
                .map(|hit| distance(a.point_at(hit.t_a), b.point_at(hit.t_b)))
                .fold(0.0f64, f64::max)
        };
        let loose = error(Tolerance::new(1e-3));
        let tight = error(Tolerance::new(1e-10));
        assert!(tight <= loose, "tightening made it worse: {tight} vs {loose}");
        assert!(tight < 1e-9, "tight tolerance did not converge: {tight}");
    }

    #[test]
    fn a_spline_crossing_is_found_and_converged() {
        let spline = Curve::Nurbs(
            NurbsCurve::interpolate(
                &[[0.0, 0.0], [2.0, 4.0], [6.0, -2.0], [9.0, 3.0]],
                None,
                None,
                Parameterization::Chord,
            )
            .unwrap(),
        );
        let line = segment([-1.0, 1.0], [10.0, 1.0]);
        let hits = intersect(&spline, &line, tol());
        assert!(!hits.is_empty(), "expected the line to cut the spline");
        for hit in hits {
            assert!((hit.point[1] - 1.0).abs() < 1e-5, "not on the line");
            assert!(
                distance(spline.point_at(hit.t_a), hit.point) < 1e-5,
                "not on the spline"
            );
        }
    }

    #[test]
    fn curves_that_miss_report_nothing() {
        assert!(intersect(
            &segment([-10.0, 9.0], [10.0, 9.0]),
            &circle([0.0, 0.0], 5.0),
            tol()
        )
        .is_empty());
        assert!(intersect(&ellipse([0.0, 0.0], 1.0, 1.0), &ellipse([50.0, 0.0], 1.0, 1.0), tol())
            .is_empty());
    }

    #[test]
    fn parallel_segments_never_cross() {
        assert!(intersect(
            &segment([0.0, 0.0], [10.0, 0.0]),
            &segment([0.0, 3.0], [10.0, 3.0]),
            tol()
        )
        .is_empty());
    }

    #[test]
    fn a_tangent_line_touches_once_rather_than_twice() {
        let hits = intersect(&segment([-10.0, 5.0], [10.0, 5.0]), &circle([0.0, 0.0], 5.0), tol());
        assert_eq!(hits.len(), 1, "got {hits:?}");
    }

    fn ray(origin: [f64; 2], direction: [f64; 2]) -> Curve {
        Curve::Ray(RayShape { origin, direction })
    }

    fn xline(base: [f64; 2], direction: [f64; 2]) -> Curve {
        Curve::XLine(XLineShape { base, direction })
    }

    #[test]
    fn a_ray_crosses_ahead_of_its_origin() {
        let hits = intersect(
            &ray([0.0, 0.0], [1.0, 0.0]),
            &segment([5.0, -5.0], [5.0, 5.0]),
            tol(),
        );
        assert_eq!(hits.len(), 1);
        assert!((hits[0].point[0] - 5.0).abs() < 1e-9);
        // Past the direction vector's own length, which a bounded curve would
        // have refused.
        assert!((hits[0].t_a - 5.0).abs() < 1e-9, "t_a = {}", hits[0].t_a);
    }

    #[test]
    fn a_ray_does_not_cross_behind_its_origin() {
        // The line is on the far side from where the ray points.
        let hits = intersect(
            &ray([0.0, 0.0], [1.0, 0.0]),
            &segment([-5.0, -5.0], [-5.0, 5.0]),
            tol(),
        );
        assert!(hits.is_empty(), "a ray does not run backwards: {hits:?}");
    }

    #[test]
    fn an_infinite_line_crosses_on_both_sides() {
        let line = xline([0.0, 0.0], [1.0, 0.0]);
        let ahead = intersect(&line, &segment([5.0, -1.0], [5.0, 1.0]), tol());
        let behind = intersect(&line, &segment([-5.0, -1.0], [-5.0, 1.0]), tol());
        assert_eq!(ahead.len(), 1);
        assert_eq!(behind.len(), 1);
        assert!(ahead[0].t_a > 0.0 && behind[0].t_a < 0.0, "signs should differ");
    }

    #[test]
    fn a_ray_through_a_circle_reports_only_what_is_ahead() {
        // Origin inside the circle, so one crossing is ahead and one behind.
        let hits = intersect(&ray([0.0, 0.0], [1.0, 0.0]), &circle([0.0, 0.0], 5.0), tol());
        assert_eq!(hits.len(), 1, "got {hits:?}");
        assert!((hits[0].point[0] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn an_infinite_line_through_a_circle_reports_both() {
        let hits = intersect(&xline([0.0, 0.0], [1.0, 0.0]), &circle([0.0, 0.0], 5.0), tol());
        assert_eq!(hits.len(), 2);
        assert!((hits[0].point[0] + 5.0).abs() < 1e-6);
        assert!((hits[1].point[0] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn an_unbounded_curve_crosses_a_spline_without_being_sampled() {
        // The straight side stays exact however far the crossing is; only the
        // spline is subdivided.
        let spline = Curve::Nurbs(
            NurbsCurve::interpolate(
                &[[0.0, 0.0], [2.0, 4.0], [6.0, -2.0], [9.0, 3.0]],
                None,
                None,
                Parameterization::Chord,
            )
            .unwrap(),
        );
        let hits = intersect(&xline([-100.0, 1.0], [1.0, 0.0]), &spline, tol());
        assert!(!hits.is_empty());
        for hit in hits {
            assert!((hit.point[1] - 1.0).abs() < 1e-5, "not on the line");
        }
    }

    #[test]
    fn survey_coordinates_still_cross_where_they_should() {
        let origin = [512_345.678, 4_512_345.678];
        let hits = intersect(
            &segment([origin[0] - 10.0, origin[1]], [origin[0] + 10.0, origin[1]]),
            &circle(origin, 5.0),
            tol(),
        );
        assert_eq!(hits.len(), 2);
        assert!((hits[0].point[0] - (origin[0] - 5.0)).abs() < 1e-6);
        assert!((hits[1].point[0] - (origin[0] + 5.0)).abs() < 1e-6);
    }

    #[test]
    fn tangent_circles_intersect_reports_single_crossing() {
        let c1 = circle([0.0, 0.0], 10.0);
        let c2 = circle([20.0, 0.0], 10.0);
        let hits = intersect(&c1, &c2, tol());
        assert_eq!(hits.len(), 1, "tangent circles must have 1 crossing");
        assert!((hits[0].point[0] - 10.0).abs() < 1e-5);
        assert!(hits[0].point[1].abs() < 1e-5);
        assert!((hits[0].t_a - 0.0).abs() < 1e-5);
    }

    #[test]
    fn tangent_arcs_intersect_reports_single_crossing() {
        use super::super::Arc as KernelArc;
        let a1 = Curve::Arc(KernelArc {
            centre: [0.0, 0.0],
            radius: 10.0,
            start_angle: -1.0,
            end_angle: 1.0,
        });
        let a2 = circle([20.0, 0.0], 10.0);
        let hits = intersect(&a1, &a2, tol());
        assert_eq!(hits.len(), 1);
        assert!((hits[0].point[0] - 10.0).abs() < 1e-5);
        assert!((hits[0].t_a - 0.5).abs() < 1e-5);
    }

    #[test]
    fn skinny_ellipse_crossing_arc_detected() {
        use super::super::{Ellipse as KernelEllipse, EllipseArc as KernelEllipseArc, Arc as KernelArc};
        let ell = Curve::Ellipse(KernelEllipseArc {
            ellipse: KernelEllipse {
                centre: [0.0, 0.0],
                major_radius: 100.0,
                minor_radius: 5.0,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 0.0,
            end_parameter: std::f64::consts::TAU,
        });
        let arc = Curve::Arc(KernelArc {
            centre: [0.0, 0.0],
            radius: 95.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        });
        let hits = intersect(&ell, &arc, tol());
        assert_eq!(hits.len(), 2, "half-circle arc of radius 95 must cross upper half of skinny ellipse twice");
    }
}
