//! Measuring along a curve, and finding the place a measurement lands.
//!
//! Every operation that spaces something evenly along a curve needs this:
//! DIVIDE and MEASURE placing points, an array laying copies along a path, a
//! linetype stepping its dash pattern, a leader deciding where its arrowhead
//! sits. All of them ask the same two questions — how long is this, and where
//! am I after travelling *d* along it — and neither is answerable from the
//! parameter alone.
//!
//! # Why the parameter is not the distance
//!
//! For a line, a circle and a circular arc the two are proportional, which is
//! why a per-type implementation can get away with treating them as the same
//! thing until it meets an ellipse or a spline. There they part company
//! sharply: an ellipse's parameter runs fastest near the ends of its minor
//! axis and slowest near the major, so points placed at even parameters bunch
//! up at the tight ends. A spline's parameter is whatever its knot vector
//! says, which need not be related to length at all.
//!
//! # How
//!
//! Where a closed form exists it is used — a line, a circle, a circular arc
//! and each span of a polyline all have one, exactly.
//!
//! Where none does, the length is `∫‖C′(t)‖ dt`, integrated by Gauss–Legendre
//! on subintervals. The integrand is smooth away from knots, and a
//! five-point rule is exact for polynomials up to degree nine, so a handful
//! of subintervals resolves it to well past what a drawing can express. The
//! inverse — the parameter at a given distance — then comes from a bisection
//! bracketed on the same cumulative table, which cannot diverge the way a
//! bare Newton iteration does where the curve nearly stops.

use super::curve::{Arc, Circle, Curve, EllipseArc};
use super::polyline::Polyline;
use super::vec::Vec2;
use std::f64::consts::TAU;

/// Abscissae and weights of the five-point Gauss–Legendre rule on `[-1, 1]`.
///
/// Exact for polynomials up to degree nine. The speed `‖C′(t)‖` is not a
/// polynomial — the square root sees to that — but it is smooth, and the
/// error falls off fast enough that the subdivision below carries the rest.
const NODES: [(f64, f64); 5] = [
    (-0.906_179_845_938_664, 0.236_926_885_056_189),
    (-0.538_469_310_105_683, 0.478_628_670_499_366),
    (0.0, 0.568_888_888_888_889),
    (0.538_469_310_105_683, 0.478_628_670_499_366),
    (0.906_179_845_938_664, 0.236_926_885_056_189),
];

/// How many subintervals the numeric cases integrate over.
///
/// The cumulative table this produces is also what the inverse brackets on,
/// so the figure sets both the accuracy of a length and how tightly the
/// bisection starts. Thirty-two five-point panels is far more than a drawing
/// can distinguish and still costs under two hundred evaluations.
const PANELS: usize = 32;

impl Curve {
    /// The derivative with respect to the curve's own `0..=1` parameter.
    ///
    /// The direction of travel, with the magnitude that makes
    /// `∫‖tangent‖ dt` over `0..=1` the curve's length. Zero only where the
    /// curve genuinely stops — a collapsed segment, a zero-radius arc.
    pub fn tangent_at(&self, t: f64) -> [f64; 2] {
        match self {
            Self::Line(line) => line.direction(),
            Self::Ray(ray) => ray.direction,
            Self::XLine(line) => line.direction,
            Self::Circle(circle) => {
                let angle = t * TAU;
                [
                    -circle.radius * angle.sin() * TAU,
                    circle.radius * angle.cos() * TAU,
                ]
            }
            Self::Arc(arc) => {
                let sweep = arc.sweep();
                let angle = super::angle::normalize_angle(arc.start_angle) + t * sweep;
                [
                    -arc.radius * angle.sin() * sweep,
                    arc.radius * angle.cos() * sweep,
                ]
            }
            Self::Ellipse(arc) => {
                let sweep = arc.sweep();
                let parameter = arc.start_parameter + t * sweep;
                let ellipse = &arc.ellipse;
                let (nx, ny) = (ellipse.major_axis[0], ellipse.major_axis[1]);
                // d/dt (centre + a·cos·major + b·sin·minor), with the minor
                // direction being the major turned a quarter counter-clockwise.
                let along = -ellipse.major_radius * parameter.sin() * sweep;
                let across = ellipse.minor_radius * parameter.cos() * sweep;
                [along * nx - across * ny, along * ny + across * nx]
            }
            Self::Polyline(polyline) => {
                let segments = self.segments();
                let count = segments.len();
                if count == 0 {
                    return [0.0, 0.0];
                }
                // A polyline shares its parameter out evenly between its
                // segments, so each one's own tangent is scaled by how many
                // there are.
                let scaled = (t.clamp(0.0, 1.0) * count as f64).min(count as f64 - 1e-12);
                let index = (scaled.floor() as usize).min(count - 1);
                let local = segments[index].tangent_at(scaled - index as f64);
                let _ = polyline;
                [local[0] * count as f64, local[1] * count as f64]
            }
            Self::Nurbs(curve) => curve.derivative_at(t),
        }
    }

    /// The curve's length.
    ///
    /// Infinite for a ray or a construction line, which is the honest answer
    /// and the one that keeps a caller from spacing points along something
    /// unbounded.
    pub fn length(&self) -> f64 {
        match self {
            Self::Line(line) => line.length(),
            Self::Ray(_) | Self::XLine(_) => f64::INFINITY,
            Self::Circle(circle) => circle.radius.abs() * TAU,
            Self::Arc(arc) => arc.radius.abs() * arc.sweep().abs(),
            Self::Ellipse(_) | Self::Nurbs(_) => self.length_between(0.0, 1.0),
            Self::Polyline(polyline) => polyline_lengths(self, polyline).total,
        }
    }

    /// The length of the stretch between two parameters.
    ///
    /// Negative when `to` comes before `from`, so it composes.
    pub fn length_to(&self, t: f64) -> f64 {
        self.length_between(0.0, t)
    }

    fn length_between(&self, from: f64, to: f64) -> f64 {
        if to < from {
            return -self.length_between(to, from);
        }
        match self {
            // The proportional cases, where a fraction of the parameter is
            // the same fraction of the length.
            Self::Line(_)
            | Self::Circle(_)
            | Self::Arc(_)
            | Self::Ray(_)
            | Self::XLine(_) => self.length() * (to - from),
            Self::Polyline(polyline) => {
                let table = polyline_lengths(self, polyline);
                table.at(to) - table.at(from)
            }
            Self::Ellipse(_) | Self::Nurbs(_) => {
                let span = to - from;
                if span <= 0.0 {
                    return 0.0;
                }
                let width = span / PANELS as f64;
                (0..PANELS)
                    .map(|panel| {
                        let lower = from + width * panel as f64;
                        integrate_speed(self, lower, lower + width)
                    })
                    .sum()
            }
        }
    }

    /// The parameter at arc length `distance` from the start.
    ///
    /// Clamped to the curve: a distance past the end reports `1`, a negative
    /// one reports `0`. A caller that needs to know it ran off the end should
    /// compare against [`length`](Self::length) first.
    pub fn parameter_at_distance(&self, distance: f64) -> f64 {
        let total = self.length();
        // NaN needs naming: it compares false against everything, so a
        // length test alone would let a degenerate curve through.
        if total.is_nan() || total <= 0.0 {
            return 0.0;
        }
        if distance <= 0.0 {
            return 0.0;
        }
        if total.is_finite() && distance >= total {
            return 1.0;
        }
        match self {
            Self::Line(_) | Self::Circle(_) | Self::Arc(_) => distance / total,
            // Unbounded, so the parameter is not a fraction of anything: one
            // unit of it advances by the direction vector's own length.
            Self::Ray(_) | Self::XLine(_) => {
                let step = Vec2::from(self.tangent_at(0.0)).length();
                if step > 0.0 {
                    distance / step
                } else {
                    0.0
                }
            }
            Self::Polyline(polyline) => polyline_lengths(self, polyline).parameter_at(distance),
            Self::Ellipse(_) | Self::Nurbs(_) => {
                // Bisection on the cumulative length rather than Newton: the
                // speed drops towards zero at a cusp or a repeated control
                // point, and a Newton step divided by it walks off the curve.
                let (mut low, mut high) = (0.0f64, 1.0f64);
                for _ in 0..60 {
                    let middle = 0.5 * (low + high);
                    if self.length_between(0.0, middle) < distance {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                0.5 * (low + high)
            }
        }
    }

    /// The point at arc length `distance` from the start.
    pub fn point_at_distance(&self, distance: f64) -> [f64; 2] {
        self.point_at(self.parameter_at_distance(distance))
    }
}

/// Five-point Gauss–Legendre on `[from, to]` of the curve's speed.
fn integrate_speed(curve: &Curve, from: f64, to: f64) -> f64 {
    let half = 0.5 * (to - from);
    let middle = 0.5 * (to + from);
    half * NODES
        .iter()
        .map(|(node, weight)| {
            weight * Vec2::from(curve.tangent_at(middle + half * node)).length()
        })
        .sum::<f64>()
}

/// Cumulative length at each of a polyline's segment boundaries.
struct SegmentLengths {
    /// Running total, one entry per segment boundary; `0` first.
    cumulative: Vec<f64>,
    total: f64,
}

impl SegmentLengths {
    /// The length from the start to parameter `t`.
    fn at(&self, t: f64) -> f64 {
        let count = self.cumulative.len().saturating_sub(1);
        if count == 0 {
            return 0.0;
        }
        let t = t.clamp(0.0, 1.0);
        let scaled = t * count as f64;
        let index = (scaled.floor() as usize).min(count - 1);
        let within = scaled - index as f64;
        let span = self.cumulative[index + 1] - self.cumulative[index];
        self.cumulative[index] + span * within
    }

    /// The parameter at length `distance` from the start.
    fn parameter_at(&self, distance: f64) -> f64 {
        let count = self.cumulative.len().saturating_sub(1);
        if count == 0 || self.total.is_nan() || self.total <= 0.0 {
            return 0.0;
        }
        let index = match self
            .cumulative
            .binary_search_by(|value| value.partial_cmp(&distance).expect("finite lengths"))
        {
            Ok(hit) => hit.min(count),
            Err(after) => after.saturating_sub(1).min(count - 1),
        };
        let span = self.cumulative[index + 1] - self.cumulative[index];
        let within = if span > 0.0 {
            (distance - self.cumulative[index]) / span
        } else {
            0.0
        };
        ((index as f64 + within) / count as f64).clamp(0.0, 1.0)
    }
}

/// Each of a polyline's segments measured exactly — a straight span by its
/// chord, a bulged one by `r·θ`.
fn polyline_lengths(curve: &Curve, _polyline: &Polyline) -> SegmentLengths {
    let mut cumulative = vec![0.0];
    let mut total = 0.0;
    for segment in curve.segments() {
        total += segment.length();
        cumulative.push(total);
    }
    SegmentLengths { cumulative, total }
}

/// Bookkeeping so the closed forms above are reachable from one place.
impl Circle {
    /// The circumference.
    pub fn length(&self) -> f64 {
        self.radius.abs() * TAU
    }
}

impl Arc {
    /// The arc's length along the curve, `r·θ`.
    pub fn length(&self) -> f64 {
        self.radius.abs() * self.sweep().abs()
    }
}

impl EllipseArc {
    /// The arc's length, integrated — an ellipse has no closed form.
    pub fn length(&self) -> f64 {
        Curve::Ellipse(*self).length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::polyline::PolylineVertex;
    use crate::geom2d::{Ellipse, Line, NurbsCurve};
    use std::f64::consts::{FRAC_PI_2, PI};

    /// Length measured by walking a very fine tessellation, for checking the
    /// analytic and integrated answers against something independent.
    fn walked(curve: &Curve, steps: usize) -> f64 {
        (0..steps)
            .map(|i| {
                let a = Vec2::from(curve.point_at(i as f64 / steps as f64));
                let b = Vec2::from(curve.point_at((i + 1) as f64 / steps as f64));
                a.distance(b)
            })
            .sum()
    }

    #[test]
    fn a_line_measures_its_own_span() {
        let line = Curve::Line(Line {
            start: [0.0, 0.0],
            end: [3.0, 4.0],
        });
        assert!((line.length() - 5.0).abs() < 1e-12);
        assert_eq!(line.point_at_distance(2.5), [1.5, 2.0]);
        assert!((line.parameter_at_distance(1.25) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn a_circle_and_an_arc_use_their_closed_forms() {
        let circle = Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 10.0,
        });
        assert!((circle.length() - TAU * 10.0).abs() < 1e-12);

        let arc = Curve::Arc(Arc {
            centre: [1.0, 2.0],
            radius: 4.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        });
        assert!((arc.length() - 4.0 * FRAC_PI_2).abs() < 1e-12);
        // A quarter of the way along by distance is a quarter by angle.
        let quarter = arc.point_at_distance(arc.length() * 0.25);
        let expected = arc.point_at(0.25);
        assert!((quarter[0] - expected[0]).abs() < 1e-12);
    }

    #[test]
    fn an_ellipse_is_integrated_and_agrees_with_walking_it() {
        let ellipse = Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [0.0, 0.0],
                major_radius: 100.0,
                minor_radius: 40.0,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 0.0,
            end_parameter: TAU,
        });
        let integrated = ellipse.length();
        let stepped = walked(&ellipse, 200_000);
        assert!(
            (integrated - stepped).abs() < 1e-4 * stepped,
            "{integrated} vs {stepped}"
        );
    }

    #[test]
    fn an_ellipse_spaced_by_distance_is_not_spaced_by_parameter() {
        // The reason this module exists. On a 100 × 10 ellipse the two
        // answers are nowhere near each other.
        let ellipse = Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [0.0, 0.0],
                major_radius: 100.0,
                minor_radius: 10.0,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 0.0,
            end_parameter: TAU,
        });
        let total = ellipse.length();
        // Not at the quarters: an ellipse is symmetric about both axes, so a
        // quarter of its length falls exactly at a quarter of its parameter
        // whatever its proportions. That the two agree there to a part in a
        // million is a check on the integration, not a counter-example.
        let quarter = Vec2::from(ellipse.point_at_distance(total * 0.25));
        assert!(quarter.distance(Vec2::from(ellipse.point_at(0.25))) < 1e-4);

        // Between the quarters they part company badly.
        let by_distance = Vec2::from(ellipse.point_at_distance(total * 0.125));
        let by_parameter = Vec2::from(ellipse.point_at(0.125));
        let gap = by_distance.distance(by_parameter);
        assert!(gap > 10.0, "the two agreed to within {gap}");
    }

    #[test]
    fn even_spacing_really_is_even() {
        let ellipse = Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [5.0, -3.0],
                major_radius: 60.0,
                minor_radius: 15.0,
                major_axis: [0.6, 0.8],
            },
            start_parameter: 0.0,
            end_parameter: TAU,
        });
        let total = ellipse.length();
        let step = total / 12.0;
        let points: Vec<Vec2> = (0..=12)
            .map(|i| Vec2::from(ellipse.point_at_distance(step * i as f64)))
            .collect();
        for pair in points.windows(2) {
            // Each gap is a chord rather than an arc, so it is a little
            // shorter than the step; what matters is that they agree.
            let chord = pair[0].distance(pair[1]);
            assert!(
                (chord - step).abs() < 0.05 * step,
                "chord {chord} against step {step}"
            );
        }
    }

    #[test]
    fn a_polyline_counts_its_bulges_rather_than_its_chords() {
        // Straight run of 100, then a half circle of radius 50.
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
        let expected = 100.0 + PI * 50.0;
        assert!(
            (polyline.length() - expected).abs() < 1e-9,
            "{} vs {expected}",
            polyline.length()
        );
        // Measuring by chords would have said 200 — the error this fixes.
        assert!(polyline.length() > 250.0);

        // Half way along by distance lands on the arc, not at the vertex.
        let middle = polyline.point_at_distance(expected * 0.5);
        assert!(middle[0] > 100.0, "{middle:?}");
    }

    #[test]
    fn a_polyline_walks_to_its_own_vertices() {
        let polyline = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex {
                    position: [0.0, 0.0],
                    bulge: 0.0,
                },
                PolylineVertex {
                    position: [30.0, 0.0],
                    bulge: 0.0,
                },
                PolylineVertex {
                    position: [30.0, 40.0],
                    bulge: 0.0,
                },
            ],
            closed: false,
        });
        assert!((polyline.length() - 70.0).abs() < 1e-12);
        let corner = polyline.point_at_distance(30.0);
        assert!((corner[0] - 30.0).abs() < 1e-9 && corner[1].abs() < 1e-9, "{corner:?}");
        let end = polyline.point_at_distance(70.0);
        assert!((end[1] - 40.0).abs() < 1e-9, "{end:?}");
    }

    #[test]
    fn a_spline_measures_the_curve_and_not_its_control_polygon() {
        // A rational quarter circle of radius 100: the length is known.
        let weight = std::f64::consts::FRAC_PI_4.cos();
        let curve = Curve::Nurbs(
            NurbsCurve::new(
                2,
                vec![[100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                Some(vec![1.0, weight, 1.0]),
            )
            .unwrap(),
        );
        let expected = 100.0 * FRAC_PI_2;
        assert!(
            (curve.length() - expected).abs() < 1e-6 * expected,
            "{} vs {expected}",
            curve.length()
        );
        // The control polygon would have said 200.
        assert!(curve.length() < 190.0);
    }

    #[test]
    fn a_spline_walked_by_distance_stays_on_the_circle_it_traces() {
        let weight = std::f64::consts::FRAC_PI_4.cos();
        let curve = Curve::Nurbs(
            NurbsCurve::new(
                2,
                vec![[100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                Some(vec![1.0, weight, 1.0]),
            )
            .unwrap(),
        );
        let total = curve.length();
        for i in 0..=8 {
            let point = Vec2::from(curve.point_at_distance(total * i as f64 / 8.0));
            assert!((point.length() - 100.0).abs() < 1e-6, "{point:?}");
            // Equal distances are equal angles on a circle.
            let angle = point.angle();
            let expected = FRAC_PI_2 * i as f64 / 8.0;
            assert!((angle - expected).abs() < 1e-6, "{angle} vs {expected}");
        }
    }

    #[test]
    fn an_unbounded_curve_reports_an_infinite_length() {
        let ray = Curve::Ray(crate::geom2d::Ray {
            origin: [0.0, 0.0],
            direction: [3.0, 4.0],
        });
        assert!(ray.length().is_infinite());
        // But it can still be walked: five units along a 3-4-5 direction.
        let point = ray.point_at_distance(5.0);
        assert!((point[0] - 3.0).abs() < 1e-12 && (point[1] - 4.0).abs() < 1e-12);
    }

    #[test]
    fn distances_outside_the_curve_clamp_to_its_ends() {
        let line = Curve::Line(Line {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
        });
        assert_eq!(line.point_at_distance(-5.0), [0.0, 0.0]);
        assert_eq!(line.point_at_distance(1000.0), [10.0, 0.0]);
    }

    #[test]
    fn a_collapsed_curve_has_nothing_to_measure() {
        let line = Curve::Line(Line {
            start: [7.0, 7.0],
            end: [7.0, 7.0],
        });
        assert_eq!(line.length(), 0.0);
        assert_eq!(line.parameter_at_distance(1.0), 0.0);
    }

    #[test]
    fn the_tangent_agrees_with_how_the_point_moves() {
        let curves = [
            Curve::Circle(Circle {
                centre: [1.0, 2.0],
                radius: 7.0,
            }),
            Curve::Arc(Arc {
                centre: [0.0, 0.0],
                radius: 3.0,
                start_angle: 0.5,
                end_angle: 2.0,
            }),
            Curve::Ellipse(EllipseArc {
                ellipse: Ellipse {
                    centre: [0.0, 0.0],
                    major_radius: 9.0,
                    minor_radius: 4.0,
                    major_axis: [0.6, 0.8],
                },
                start_parameter: 0.3,
                end_parameter: 2.9,
            }),
        ];
        for curve in curves {
            for i in 1..10 {
                let t = i as f64 / 10.0;
                let step = 1e-7;
                let before = Vec2::from(curve.point_at(t - step));
                let after = Vec2::from(curve.point_at(t + step));
                let numeric = (after - before) / (2.0 * step);
                let analytic = Vec2::from(curve.tangent_at(t));
                let scale = numeric.length().max(1.0);
                assert!(
                    (analytic - numeric).length() < 1e-4 * scale,
                    "t={t}: {analytic:?} vs {numeric:?}"
                );
            }
        }
    }

    #[test]
    fn survey_coordinates_measure_the_same_length() {
        let local = Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [0.0, 0.0],
                major_radius: 30.0,
                minor_radius: 12.0,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 0.0,
            end_parameter: TAU,
        });
        let remote = Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [512_345.678, 4_512_345.678],
                major_radius: 30.0,
                minor_radius: 12.0,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 0.0,
            end_parameter: TAU,
        });
        assert!((local.length() - remote.length()).abs() < 1e-9);
    }
}
