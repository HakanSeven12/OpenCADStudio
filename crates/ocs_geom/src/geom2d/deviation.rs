//! Sampling a curve to a chord tolerance rather than to a fixed density.
//!
//! [`Curve::tessellate`](super::Curve::tessellate) asks for so many segments
//! per radian, which is a look-and-feel figure: it produces the same polyline
//! for a circle a millimetre across and one the size of a city block. That is
//! fine for a preview and wrong for a drawing, where what matters is how far
//! the chord departs from the curve in the units the drawing is in — and, on
//! screen, how far that is in pixels.
//!
//! So the question this module answers is the other one: *given that no chord
//! may sag more than `tolerance` away from the curve, where are the points?*
//! A small circle gets its floor of segments and a large one gets as many as
//! it needs, from the same call.
//!
//! # How each kind is bounded
//!
//! For a circular arc the answer is closed form. A chord spanning angle `θ`
//! on a circle of radius `r` sags by `r(1 − cos(θ/2))`, so the largest step
//! that stays inside the tolerance is `θ = 2·acos(1 − tolerance/r)`.
//!
//! An ellipse has no single radius, and its parameter is not an angle, so
//! the closed form does not carry over — a step of `Δt` covers a different
//! amount of arc depending on where it is taken. What does hold for any
//! parameterisation is that a chord spanning `Δt` departs from the curve by
//! at most `‖P″‖·Δt²/8`, which for an ellipse bounds the step at
//! `√(8·tolerance/major)`. It over-samples the flatter stretches, which is
//! the safe direction to be wrong in, and on a circle it agrees with the
//! closed form to within a segment.
//!
//! A NURBS curve has neither, and is cut by subdivision instead: a span is
//! kept whole while its midpoint lies within the tolerance of its own chord,
//! and halved when it does not. That measures the actual departure rather
//! than bounding it, so a spline is cut finely exactly where it bends.

use super::curve::{Arc, Circle, Curve, EllipseArc};
use super::nurbs::NurbsCurve;
use super::vec::Vec2;
use super::Ellipse;
use std::f64::consts::TAU;

/// No curved span is cut into fewer pieces than this, however slack the
/// tolerance.
///
/// Without a floor a whole circle collapses to a triangle as soon as the
/// tolerance exceeds its radius, and a shape that coarse stops reading as
/// the thing it is.
const MINIMUM_SEGMENTS: usize = 8;

/// Nor into more than this, however tight.
///
/// A tolerance of nothing — or a curve whose radius rounds to nothing — asks
/// for an unbounded number of points, and the caller downstream is usually a
/// vertex buffer. The cap is high enough that reaching it means the request
/// was unreasonable rather than merely demanding.
const MAXIMUM_SEGMENTS: usize = 16_384;

/// How deep the NURBS subdivision will go before accepting a span.
///
/// Each level halves the span, so this is a floor on span size of 2⁻¹⁶ of the
/// domain. A curve that is still not flat there has a cusp in it, and cutting
/// further only spends points on something no tolerance will satisfy.
const MAX_DEPTH: u32 = 16;

impl Curve {
    /// Samples the curve so its direction changes by at most `max_angle`
    /// radians across each segment.
    pub fn tessellate_angle(&self, max_angle: f64) -> Vec<[f64; 2]> {
        let max_angle = crate::tessellation::angle(max_angle);
        match self {
            Self::Line(line) => vec![line.start, line.end],
            Self::Ray(_) | Self::XLine(_) => vec![self.point_at(0.0), self.point_at(1.0)],
            Self::Circle(_) => sample_uniformly(self, angular_steps(TAU, max_angle)),
            Self::Arc(arc) => sample_uniformly(self, angular_steps(arc.sweep(), max_angle)),
            Self::Ellipse(_) => angular_curve(self, &[0.0, 1.0], max_angle, 0),
            Self::Polyline(_) => self.polyline_angle(max_angle),
            Self::Nurbs(curve) => {
                let wrapped = Self::Nurbs(curve.clone());
                angular_curve(&wrapped, &span_boundaries(curve), max_angle, 2)
            }
        }
    }

    fn polyline_angle(&self, max_angle: f64) -> Vec<[f64; 2]> {
        let Self::Polyline(polyline) = self else {
            return Vec::new();
        };
        let count = polyline.vertices.len();
        let segments = if polyline.closed {
            count
        } else {
            count.saturating_sub(1)
        };
        if segments == 0 {
            return polyline.vertices.iter().map(|vertex| vertex.position).collect();
        }
        let mut out = Vec::new();
        for index in 0..segments {
            let next = (index + 1) % count;
            let mut points = match polyline.segment_arc(index) {
                Some(arc) => arc.tessellate_angle(max_angle),
                None => vec![
                    polyline.vertices[index].position,
                    polyline.vertices[next].position,
                ],
            };
            if let (Some(first), Some(vertex)) =
                (points.first_mut(), polyline.vertices.get(index))
            {
                *first = vertex.position;
            }
            if let (Some(last), Some(vertex)) = (
                points.last_mut(),
                polyline.vertices.get(next),
            ) {
                *last = vertex.position;
            }
            let skip = usize::from(out.last() == points.first());
            out.extend_from_slice(&points[skip..]);
        }
        out
    }

    /// Samples the curve so that no chord departs from it by more than
    /// `tolerance`, in the curve's own units.
    ///
    /// Both endpoints are included. A straight curve comes back as its two
    /// ends whatever the tolerance, since a chord of a line is the line.
    ///
    /// A non-positive or non-finite tolerance is treated as "as fine as the
    /// cap allows" rather than rejected, so a caller that computed one from a
    /// zoom level does not have to guard the degenerate frame.
    pub fn tessellate_within(&self, tolerance: f64) -> Vec<[f64; 2]> {
        match self {
            Self::Line(line) => vec![line.start, line.end],
            Self::Ray(_) | Self::XLine(_) => vec![self.point_at(0.0), self.point_at(1.0)],
            Self::Circle(circle) => sample_uniformly(self, circle_steps(circle, tolerance)),
            Self::Arc(arc) => sample_uniformly(self, arc_steps(arc, tolerance)),
            Self::Ellipse(arc) => sample_uniformly(self, ellipse_steps(arc, tolerance)),
            Self::Polyline(_) => self.polyline_within(tolerance),
            Self::Nurbs(curve) => nurbs_within(curve, tolerance),
        }
    }

    /// A polyline, each of its segments held to the tolerance in turn.
    ///
    /// Done through [`segments`](Self::segments) rather than by walking the
    /// vertices so that a bulged span is an [`Arc`] here too, and gets the
    /// same closed-form step as a standalone one.
    ///
    /// The joints are then pinned back to the stored vertices. An arc span is
    /// reconstructed from its bulge, so its ends land a rounding away from
    /// the vertex they were built from; left alone, a closed loop does not
    /// quite close and a boundary traced through it leaks.
    fn polyline_within(&self, tolerance: f64) -> Vec<[f64; 2]> {
        let Self::Polyline(polyline) = self else {
            return Vec::new();
        };
        if polyline.vertices.len() < 2 {
            return polyline.vertices.iter().map(|vertex| vertex.position).collect();
        }
        let count = polyline.vertices.len();
        let mut out: Vec<[f64; 2]> = Vec::new();
        for (index, segment) in self.segments().into_iter().enumerate() {
            let mut points = segment.tessellate_within(tolerance);
            if let (Some(first), Some(vertex)) =
                (points.first_mut(), polyline.vertices.get(index))
            {
                *first = vertex.position;
            }
            if let (Some(last), Some(vertex)) = (
                points.last_mut(),
                polyline.vertices.get((index + 1) % count.max(1)),
            ) {
                *last = vertex.position;
            }
            // Consecutive segments share a vertex; emitting it twice would put
            // a zero-length piece into whatever consumes the result.
            let skip = usize::from(out.last() == points.first());
            out.extend_from_slice(&points[skip..]);
        }
        out
    }
}

fn angular_steps(sweep: f64, max_angle: f64) -> usize {
    (sweep.abs() / max_angle).ceil().max(1.0) as usize
}

fn angular_curve(
    curve: &Curve,
    boundaries: &[f64],
    max_angle: f64,
    minimum_depth: u32,
) -> Vec<[f64; 2]> {
    let Some(first) = boundaries.first().copied() else {
        return Vec::new();
    };
    let mut out = vec![curve.point_at(first)];
    for pair in boundaries.windows(2) {
        subdivide_angle(
            curve,
            pair[0],
            pair[1],
            max_angle,
            minimum_depth,
            0,
            &mut out,
        );
    }
    out
}

fn subdivide_angle(
    curve: &Curve,
    from: f64,
    to: f64,
    max_angle: f64,
    minimum_depth: u32,
    depth: u32,
    out: &mut Vec<[f64; 2]>,
) {
    let directions = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]
        .map(|unit| curve.tangent_at(from + (to - from) * unit));
    let split = depth < minimum_depth
        || crate::tessellation::max_direction_angle(&directions) > max_angle;
    if split && depth < MAX_DEPTH {
        let middle = 0.5 * (from + to);
        subdivide_angle(
            curve,
            from,
            middle,
            max_angle,
            minimum_depth,
            depth + 1,
            out,
        );
        subdivide_angle(
            curve,
            middle,
            to,
            max_angle,
            minimum_depth,
            depth + 1,
            out,
        );
    } else {
        out.push(curve.point_at(to));
    }
}

/// Evaluates the curve at `steps + 1` evenly spaced parameters.
fn sample_uniformly(curve: &Curve, steps: usize) -> Vec<[f64; 2]> {
    (0..=steps)
        .map(|i| curve.point_at(i as f64 / steps as f64))
        .collect()
}

/// Whether a tolerance is one this module can work to.
///
/// NaN falls out through `is_finite`, so the caller that computed a tolerance
/// from a zoom level does not have to guard the degenerate frame.
fn usable(tolerance: f64) -> bool {
    tolerance.is_finite() && tolerance > 0.0
}

/// The number of chords a sweep of `sweep` radians on a circle of `radius`
/// needs to stay within `tolerance`.
fn circular_steps(radius: f64, sweep: f64, tolerance: f64) -> usize {
    let radius = radius.abs();
    let sweep = sweep.abs();
    if !usable(tolerance) || radius <= tolerance {
        // Either the caller wants everything the cap allows, or the curve is
        // smaller than the tolerance and any chord already satisfies it. The
        // floor decides both, and the second is the common one — a tiny
        // fillet in a drawing measured in metres.
        return if radius <= tolerance {
            MINIMUM_SEGMENTS
        } else {
            MAXIMUM_SEGMENTS
        };
    }
    // sag = r(1 − cos(θ/2)) ≤ tolerance  ⟹  θ ≤ 2·acos(1 − tolerance/r)
    let step = 2.0 * (1.0 - tolerance / radius).clamp(-1.0, 1.0).acos();
    if step <= 0.0 {
        return MAXIMUM_SEGMENTS;
    }
    ((sweep / step).ceil() as usize).clamp(MINIMUM_SEGMENTS, MAXIMUM_SEGMENTS)
}

fn circle_steps(circle: &Circle, tolerance: f64) -> usize {
    circular_steps(circle.radius, TAU, tolerance)
}

fn arc_steps(arc: &Arc, tolerance: f64) -> usize {
    circular_steps(arc.radius, arc.sweep(), tolerance)
}

/// An ellipse, bounded by how fast its parameterisation can turn.
///
/// The circle's closed form does not carry over: an ellipse's parameter is
/// not an angle, so a step of `Δt` sweeps a different amount of arc depending
/// on where along the ellipse it is taken. Reading the radius of curvature as
/// if it were an angular step under-samples the tight ends badly — a 100 × 5
/// ellipse came out with twelve segments where it needs two hundred.
///
/// The bound that does hold for any parameterisation: a chord spanning `Δt`
/// departs from the curve by at most `‖P″‖·Δt²/8`. For `P(t) = (a·cos t,
/// b·sin t)` the second derivative is `(−a·cos t, −b·sin t)`, never longer
/// than the major radius, so `Δt ≤ √(8·tolerance/a)`.
///
/// The same bound applied to a circle agrees with its closed form to within
/// a segment, which is the check that it is tight rather than merely safe.
fn ellipse_steps(arc: &EllipseArc, tolerance: f64) -> usize {
    let Ellipse {
        major_radius,
        minor_radius,
        ..
    } = arc.ellipse;
    let curvature = major_radius.abs().max(minor_radius.abs());
    let sweep = arc.sweep().abs();
    // A collapsed ellipse is a segment traced back and forth: there is no
    // curvature to resolve, so the floor stands.
    if curvature <= 0.0 {
        return MINIMUM_SEGMENTS;
    }
    if !usable(tolerance) {
        return MAXIMUM_SEGMENTS;
    }
    let step = (8.0 * tolerance / curvature).sqrt();
    if step <= 0.0 {
        return MAXIMUM_SEGMENTS;
    }
    ((sweep / step).ceil() as usize).clamp(MINIMUM_SEGMENTS, MAXIMUM_SEGMENTS)
}

/// A NURBS curve cut where it actually bends.
///
/// The knot values come first, since a curve is only as smooth as its knots
/// and a corner between spans must land on a point rather than inside a
/// chord. Each span is then subdivided until its midpoint sits within the
/// tolerance of its chord.
fn nurbs_within(curve: &NurbsCurve, tolerance: f64) -> Vec<[f64; 2]> {
    let tolerance = if usable(tolerance) { tolerance } else { 0.0 };
    let mut out = vec![curve.point_at(0.0)];
    let spans = span_boundaries(curve);
    for pair in spans.windows(2) {
        subdivide(curve, pair[0], pair[1], tolerance, 0, &mut out);
    }
    out
}

/// The curve's own parameter breaks, in `0..=1`, ends included.
fn span_boundaries(curve: &NurbsCurve) -> Vec<f64> {
    let (start, end) = curve.domain();
    let width = end - start;
    let mut values: Vec<f64> = vec![0.0];
    if width > 0.0 {
        for knot in curve.knots() {
            let normalised = (knot - start) / width;
            if normalised > 1e-12 && normalised < 1.0 - 1e-12 {
                let last = *values.last().expect("seeded with zero");
                if normalised - last > 1e-12 {
                    values.push(normalised);
                }
            }
        }
    }
    values.push(1.0);
    values
}

/// Appends the points of `from..=to`, excluding `from`, cutting where the
/// curve leaves its own chord by more than `tolerance`.
fn subdivide(
    curve: &NurbsCurve,
    from: f64,
    to: f64,
    tolerance: f64,
    depth: u32,
    out: &mut Vec<[f64; 2]>,
) {
    let middle = 0.5 * (from + to);
    let flat = depth >= MAX_DEPTH || {
        let start = Vec2::from(curve.point_at(from));
        let end = Vec2::from(curve.point_at(to));
        let centre = Vec2::from(curve.point_at(middle));
        centre.distance_to_segment(start, end) <= tolerance
    };
    // Two levels are forced whatever the measurement. A span whose midpoint
    // happens to fall on its chord is not necessarily straight — an S-shaped
    // one crosses there — and stopping on that coincidence would flatten the
    // bend on either side of it.
    if flat && depth >= 2 {
        out.push(curve.point_at(to));
        return;
    }
    subdivide(curve, from, middle, tolerance, depth + 1, out);
    subdivide(curve, middle, to, tolerance, depth + 1, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::polyline::{Polyline, PolylineVertex};
    use crate::geom2d::{Line, NurbsCurve};
    use std::f64::consts::{FRAC_PI_2, PI};

    /// The largest distance any sampled chord's midpoint sits from the curve,
    /// measured against the curve itself rather than against another sampling
    /// of it.
    fn worst_sag(curve: &Curve, points: &[[f64; 2]]) -> f64 {
        points
            .windows(2)
            .map(|pair| {
                let middle = Vec2::from(pair[0]).lerp(Vec2::from(pair[1]), 0.5);
                let on_curve = Vec2::from(curve.point_at(curve.parameter_at(middle.to_array())));
                middle.distance(on_curve)
            })
            .fold(0.0, f64::max)
    }

    fn circle(radius: f64) -> Curve {
        Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius,
        })
    }

    #[test]
    fn a_chord_never_sags_further_than_asked() {
        for radius in [0.5, 10.0, 1_000.0, 40_000.0] {
            for tolerance in [1.0, 0.05, 0.001] {
                let curve = circle(radius);
                let points = curve.tessellate_within(tolerance);
                assert!(
                    points.len() <= MAXIMUM_SEGMENTS,
                    "r={radius} tol={tolerance} needs the cap; \
                     that case belongs in the cap test"
                );
                let sag = worst_sag(&curve, &points);
                assert!(
                    sag <= tolerance * 1.000_001,
                    "r={radius} tol={tolerance}: sagged {sag} over {} points",
                    points.len()
                );
            }
        }
    }

    #[test]
    fn the_cap_is_what_stops_an_unreasonable_request() {
        // A quarter-million-unit radius held to a millimetre wants some
        // thirty-five thousand chords. The cap refuses, and the tolerance is
        // then not met — which is the documented behaviour, not a silent
        // success. Stated here so the limit is not discovered downstream.
        let curve = circle(250_000.0);
        let points = curve.tessellate_within(0.001);
        assert_eq!(points.len(), MAXIMUM_SEGMENTS + 1);
        assert!(worst_sag(&curve, &points) > 0.001);
    }

    #[test]
    fn a_bigger_circle_gets_more_points_at_the_same_tolerance() {
        // The whole reason for this over a fixed density: the two would
        // otherwise come back with identical polylines.
        let small = circle(1.0).tessellate_within(0.01).len();
        let large = circle(10_000.0).tessellate_within(0.01).len();
        assert!(large > small * 50, "{small} vs {large}");
    }

    #[test]
    fn a_slacker_tolerance_gets_fewer_points() {
        let fine = circle(100.0).tessellate_within(0.001).len();
        let coarse = circle(100.0).tessellate_within(0.5).len();
        assert!(fine > coarse * 10, "{fine} vs {coarse}");
    }

    #[test]
    fn a_curve_smaller_than_the_tolerance_still_looks_round() {
        // A millimetre fillet in a drawing measured in metres. Any chord
        // already satisfies the tolerance, so nothing but the floor stops it
        // collapsing to a triangle.
        let points = circle(0.001).tessellate_within(1.0);
        assert_eq!(points.len(), MINIMUM_SEGMENTS + 1);
    }

    #[test]
    fn a_degenerate_tolerance_is_capped_rather_than_unbounded() {
        for tolerance in [0.0, -1.0, f64::NAN] {
            let points = circle(10.0).tessellate_within(tolerance);
            assert_eq!(points.len(), MAXIMUM_SEGMENTS + 1, "tolerance {tolerance}");
        }
    }

    #[test]
    fn a_straight_curve_is_two_points_however_fine_the_tolerance() {
        let line = Curve::Line(Line {
            start: [0.0, 0.0],
            end: [1_000.0, 500.0],
        });
        assert_eq!(line.tessellate_within(1e-12).len(), 2);
    }

    #[test]
    fn an_arc_spends_points_only_on_the_sweep_it_has() {
        let quarter = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 100.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        });
        let whole = circle(100.0);
        let (a, b) = (
            quarter.tessellate_within(0.01).len(),
            whole.tessellate_within(0.01).len(),
        );
        // A quarter turn, so roughly a quarter of the points, give or take
        // the endpoint each carries.
        assert!((a as f64 - b as f64 / 4.0).abs() < 4.0, "{a} vs {b}");
    }

    #[test]
    fn an_ellipse_is_held_to_its_tightest_bend() {
        let arc = EllipseArc {
            ellipse: Ellipse {
                centre: [0.0, 0.0],
                major_radius: 100.0,
                minor_radius: 5.0,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 0.0,
            end_parameter: TAU,
        };
        let curve = Curve::Ellipse(arc);
        let tolerance = 0.01;
        let points = curve.tessellate_within(tolerance);
        // Measured by a local search around each midpoint, not by scanning a
        // fixed sampling of the whole ellipse: at these proportions the
        // curve runs at eighty-odd units of length per unit of parameter, so
        // a global scan fine enough to resolve a hundredth of a unit would
        // need hundreds of thousands of points — and a coarser one reports
        // its own spacing as the answer. `parameter_at` is no good either;
        // the tight ends are where inverting it is least well conditioned,
        // so it would be measuring itself.
        let at = |t: f64| Vec2::new(100.0 * t.cos(), 5.0 * t.sin());
        for pair in points.windows(2) {
            let middle = Vec2::from(pair[0]).lerp(Vec2::from(pair[1]), 0.5);
            let guess = (middle.y / 5.0).atan2(middle.x / 100.0);
            let sag = (0..=4_000)
                .map(|i| {
                    let t = guess - 0.05 + 0.1 * (i as f64 / 4_000.0);
                    middle.distance(at(t))
                })
                .fold(f64::INFINITY, f64::min);
            assert!(sag <= tolerance * 1.05, "{middle:?} sagged {sag}");
        }
    }

    #[test]
    fn the_ellipse_bound_agrees_with_the_circle_it_degenerates_to() {
        // The check that the second-derivative bound is tight rather than
        // merely safe: on a circle it must land within a segment of the
        // closed-form answer, not several times it.
        let radius = 100.0;
        let tolerance = 0.01;
        let as_circle = circle(radius).tessellate_within(tolerance).len();
        let as_ellipse = Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [0.0, 0.0],
                major_radius: radius,
                minor_radius: radius,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 0.0,
            end_parameter: TAU,
        })
        .tessellate_within(tolerance)
        .len();
        assert!(
            (as_circle as i64 - as_ellipse as i64).abs() <= 2,
            "{as_circle} vs {as_ellipse}"
        );
    }

    #[test]
    fn a_polyline_holds_each_of_its_segments_and_shares_their_vertices() {
        let polyline = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex {
                    position: [0.0, 0.0],
                    bulge: 0.0,
                },
                PolylineVertex {
                    position: [100.0, 0.0],
                    bulge: 1.0,
                },
                PolylineVertex {
                    position: [200.0, 0.0],
                    bulge: 0.0,
                },
            ],
            closed: false,
        });
        let points = polyline.tessellate_within(0.01);
        // The straight run contributes two points, the semicircle many.
        assert!(points.len() > 20, "{}", points.len());
        // No vertex appears twice in a row where two segments meet.
        for pair in points.windows(2) {
            assert!(pair[0] != pair[1], "duplicated {:?}", pair[0]);
        }
        assert_eq!(points.first(), Some(&[0.0, 0.0]));
        assert_eq!(points.last(), Some(&[200.0, 0.0]));
    }

    /// A NURBS half-circle, which has a known answer to measure against.
    fn nurbs_arc() -> NurbsCurve {
        let weight = (FRAC_PI_2 / 2.0).cos();
        NurbsCurve::new(
            2,
            vec![[100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            Some(vec![1.0, weight, 1.0]),
        )
        .expect("a valid rational quarter circle")
    }

    #[test]
    fn a_spline_is_cut_where_it_bends() {
        let curve = nurbs_arc();
        let tolerance = 0.05;
        let points = Curve::Nurbs(curve).tessellate_within(tolerance);
        // Every chord midpoint should be within tolerance of the true circle
        // of radius 100 the rational quarter traces.
        for pair in points.windows(2) {
            let middle = Vec2::from(pair[0]).lerp(Vec2::from(pair[1]), 0.5);
            let error = (middle.length() - 100.0).abs();
            assert!(error <= tolerance * 1.5, "{middle:?} off by {error}");
        }
    }

    #[test]
    fn a_spline_spends_fewer_points_when_the_tolerance_is_slack() {
        let fine = Curve::Nurbs(nurbs_arc()).tessellate_within(0.001).len();
        let coarse = Curve::Nurbs(nurbs_arc()).tessellate_within(1.0).len();
        assert!(fine > coarse, "{fine} vs {coarse}");
        assert!(coarse >= 5, "two forced levels leave at least four spans");
    }

    #[test]
    fn a_spline_keeps_both_ends() {
        let curve = nurbs_arc();
        let points = Curve::Nurbs(curve.clone()).tessellate_within(0.01);
        assert_eq!(points.first(), Some(&curve.point_at(0.0)));
        assert_eq!(points.last(), Some(&curve.point_at(1.0)));
    }

    #[test]
    fn survey_coordinates_are_sampled_as_finely_as_local_ones() {
        // The same circle, once near the origin and once at a UTM easting.
        // A tolerance is a length, so the two must come back with the same
        // number of points.
        let local = circle(50.0).tessellate_within(0.01).len();
        let remote = Curve::Circle(Circle {
            centre: [512_345.678, 4_512_345.678],
            radius: 50.0,
        })
        .tessellate_within(0.01)
        .len();
        assert_eq!(local, remote);
    }

    #[test]
    fn a_half_turn_of_a_big_arc_matches_the_hand_computation() {
        // r = 1000, tol = 0.1 ⟹ θ = 2·acos(1 − 1e-4) ≈ 0.028284, and π/θ
        // rounds up to 112.
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 1_000.0,
            start_angle: 0.0,
            end_angle: PI,
        });
        assert_eq!(arc.tessellate_within(0.1).len(), 112 + 1);
    }

    #[test]
    fn constant_nurbs_spans_do_not_explode_angular_tessellation() {
        // The last two non-empty knot spans have four identical supporting
        // control points apiece. Their derivative is zero everywhere, so the
        // curve is stationary rather than bending at every sampled point.
        let curve = NurbsCurve::new(
            3,
            vec![
                [0.0, 0.0],
                [0.0, 0.1],
                [0.0, 1.0],
                [0.0, 1.0],
                [0.0, 1.0],
                [0.0, 1.0],
                [0.0, 1.0],
                [0.0, 1.0],
            ],
            vec![
                0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5001, 1.0, 1.0, 1.0, 1.0,
            ],
            None,
        )
        .expect("valid cubic with two constant spans");
        let points = Curve::Nurbs(curve).tessellate_angle(crate::tessellation::DEFAULT_ANGLE);

        assert_eq!(points.first(), Some(&[0.0, 0.0]));
        assert_eq!(points.last(), Some(&[0.0, 1.0]));
        assert!(
            points.len() <= 64,
            "two stationary spans manufactured {} points",
            points.len()
        );
    }
}
