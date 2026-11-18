//! Angle normalisation and arc parameterisation.
//!
//! Arcs are stored as a centre, a radius and a pair of angles, which leaves
//! every consumer to answer the same three questions: what is this angle
//! reduced to one turn, does it fall inside the arc, and how far along the
//! arc is it. Getting the wrap-around cases wrong is the usual source of arcs
//! that trim from the wrong end.

use std::f64::consts::TAU;

/// Angles closer than this are treated as the same direction.
///
/// Deliberately not the crate's [`Tolerance`](crate::geom2d::Tolerance):
/// that one is a distance in world units, and an angular comparison is not a
/// distance. Mixing them makes the tolerance meaningless at both ends.
const ANGULAR_EPSILON: f64 = 1e-9;

/// Reduces an angle to `[0, TAU)`.
///
/// Handles negative input, which a bare `%` does not.
pub fn normalize_angle(angle: f64) -> f64 {
    ((angle % TAU) + TAU) % TAU
}

/// The counter-clockwise sweep from `start` to `end`, in `(0, TAU]`.
///
/// A full circle comes back as `TAU` rather than zero, so a closed arc
/// tessellates as a circle instead of collapsing to a point.
pub fn arc_span(start: f64, end: f64) -> f64 {
    let span = normalize_angle(end) - normalize_angle(start);
    if span <= 0.0 {
        span + TAU
    } else {
        span
    }
}

/// Whether `angle` lies on the counter-clockwise arc from `start` to `end`.
///
/// Treats a zero or full-turn sweep as covering everything, matching how a
/// closed arc behaves elsewhere.
pub fn angle_within_arc(angle: f64, start: f64, end: f64) -> bool {
    let (angle, start, end) = (
        normalize_angle(angle),
        normalize_angle(start),
        normalize_angle(end),
    );
    let sweep = end - start;
    if sweep.abs() < ANGULAR_EPSILON || (sweep - TAU).abs() < ANGULAR_EPSILON {
        return true;
    }
    if start <= end {
        angle >= start - ANGULAR_EPSILON && angle <= end + ANGULAR_EPSILON
    } else {
        // The arc crosses zero, so the inside is the union of two spans.
        angle >= start - ANGULAR_EPSILON || angle <= end + ANGULAR_EPSILON
    }
}

/// Where `angle` sits along the arc from `start` to `end`, as `0.0..=1.0`.
///
/// An angle in the gap outside the arc reports the nearer endpoint, so a
/// point just before the start reads 0 rather than 1 and nothing indexes off
/// the end of a tessellation.
pub fn arc_parameter(angle: f64, start: f64, end: f64) -> f64 {
    let span = arc_span(start, end);
    if span < 1e-12 {
        return 0.0;
    }
    let travelled = (normalize_angle(angle) - normalize_angle(start)).rem_euclid(TAU);
    if travelled <= span {
        (travelled / span).clamp(0.0, 1.0)
    } else if travelled - span <= (TAU - span) * 0.5 {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    #[test]
    fn normalisation_folds_both_directions_into_one_turn() {
        assert!((normalize_angle(0.0) - 0.0).abs() < 1e-12);
        assert!((normalize_angle(TAU) - 0.0).abs() < 1e-12);
        assert!((normalize_angle(-FRAC_PI_2) - (TAU - FRAC_PI_2)).abs() < 1e-12);
        assert!((normalize_angle(3.0 * TAU + PI) - PI).abs() < 1e-12);
    }

    #[test]
    fn a_closed_arc_spans_a_full_turn_not_nothing() {
        assert!((arc_span(0.0, 0.0) - TAU).abs() < 1e-12);
        assert!((arc_span(PI, PI) - TAU).abs() < 1e-12);
    }

    #[test]
    fn span_is_measured_counter_clockwise() {
        assert!((arc_span(0.0, FRAC_PI_2) - FRAC_PI_2).abs() < 1e-12);
        // Backwards in stored order means the long way round.
        assert!((arc_span(FRAC_PI_2, 0.0) - (TAU - FRAC_PI_2)).abs() < 1e-12);
    }

    #[test]
    fn containment_handles_an_arc_crossing_zero() {
        // From 315° to 45°, through zero.
        let (start, end) = (7.0 * PI / 4.0, PI / 4.0);
        assert!(angle_within_arc(0.0, start, end));
        assert!(angle_within_arc(0.1, start, end));
        assert!(angle_within_arc(TAU - 0.1, start, end));
        assert!(!angle_within_arc(PI, start, end));
    }

    #[test]
    fn containment_handles_an_arc_that_does_not_cross_zero() {
        assert!(angle_within_arc(FRAC_PI_2, 0.0, PI));
        assert!(!angle_within_arc(3.0 * PI / 2.0, 0.0, PI));
    }

    #[test]
    fn a_closed_arc_contains_every_angle() {
        assert!(angle_within_arc(PI, 0.0, 0.0));
        assert!(angle_within_arc(0.3, 1.0, 1.0));
    }

    #[test]
    fn parameter_runs_from_zero_to_one_across_the_arc() {
        assert!((arc_parameter(0.0, 0.0, PI) - 0.0).abs() < 1e-12);
        assert!((arc_parameter(FRAC_PI_2, 0.0, PI) - 0.5).abs() < 1e-12);
        assert!((arc_parameter(PI, 0.0, PI) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn parameter_clamps_outside_the_arc() {
        // Past the end of a half turn, so it saturates rather than exceeding 1.
        assert_eq!(arc_parameter(3.0 * PI / 2.0, 0.0, PI), 1.0);
    }

    #[test]
    fn parameter_is_monotonic_across_a_zero_crossing() {
        let (start, end) = (7.0 * PI / 4.0, PI / 4.0);
        let before = arc_parameter(7.1 * PI / 4.0, start, end);
        let at_zero = arc_parameter(0.0, start, end);
        let after = arc_parameter(0.1, start, end);
        assert!(before < at_zero, "{before} !< {at_zero}");
        assert!(at_zero < after, "{at_zero} !< {after}");
    }
}
