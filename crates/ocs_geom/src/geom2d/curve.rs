//! One type for every plane curve, and one parameterisation across all of
//! them.
//!
//! Without this, each operation grows a function per shape and every caller
//! grows a `match` deciding which to call. The intersection dispatch is the
//! case that matters: trimming, extending, breaking, snapping and hatch
//! boundary resolution all ask "where do these two meet" about curves whose
//! types they learn at runtime, and each of them was answering it with its own
//! copy of the same nested match.
//!
//! # The shared parameter
//!
//! Every curve here is parameterised `0..=1` from its own start to its own
//! end, whatever that means for its shape. A trim cuts at a parameter, a snap
//! reports one, an intersection returns one per curve — and none of them has
//! to know whether it is holding an arc or a polyline to compare or order
//! them.
//!
//! A closed curve still runs `0..=1`; it simply returns to where it started.
//!
//! # Not entities
//!
//! These carry geometry and nothing else — no handle, no layer, no colour, no
//! elevation, no extrusion normal. Converting an application's entity into one
//! of these is the application's job, and keeps this layer usable by anything
//! that has curves regardless of where they came from.

use std::f64::consts::TAU;

use super::angle::{arc_parameter, arc_span, normalize_angle};
use super::nurbs::NurbsCurve;
use super::polyline::Polyline;
use super::vec::Vec2;
use super::Ellipse;

/// A straight segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    /// Where it begins.
    pub start: [f64; 2],
    /// Where it ends.
    pub end: [f64; 2],
}

impl Line {
    /// Start-to-end vector. Not normalised — its length is the line's.
    pub fn direction(&self) -> [f64; 2] {
        (Vec2::from(self.end) - Vec2::from(self.start)).to_array()
    }

    /// Length.
    pub fn length(&self) -> f64 {
        Vec2::from(self.start).distance(Vec2::from(self.end))
    }
}

/// A full circle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    /// Centre.
    pub centre: [f64; 2],
    /// Radius.
    pub radius: f64,
}

/// A circular arc, running counter-clockwise from `start_angle` to
/// `end_angle`.
///
/// Equal angles mean a full turn rather than nothing, matching [`arc_span`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    /// Centre of the circle it lies on.
    pub centre: [f64; 2],
    /// Its radius.
    pub radius: f64,
    /// Angle at the start, measured at the centre.
    pub start_angle: f64,
    /// Angle at the end.
    pub end_angle: f64,
}

impl Arc {
    /// How far it sweeps, always positive.
    pub fn sweep(&self) -> f64 {
        arc_span(self.start_angle, self.end_angle)
    }
}

/// A piece of an ellipse, between two of the ellipse's own parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EllipseArc {
    /// The ellipse it lies on.
    pub ellipse: Ellipse,
    /// Parameter at the start.
    pub start_parameter: f64,
    /// Parameter at the end. Not above the start means the arc wraps, and a
    /// full turn is added.
    pub end_parameter: f64,
}

impl EllipseArc {
    /// A whole ellipse.
    pub fn full(ellipse: Ellipse) -> Self {
        Self {
            ellipse,
            start_parameter: 0.0,
            end_parameter: TAU,
        }
    }

    /// How far it sweeps in parameter, always positive.
    pub fn sweep(&self) -> f64 {
        let raw = self.end_parameter - self.start_parameter;
        if raw <= 0.0 {
            raw + TAU
        } else {
            raw
        }
    }
}

/// A ray: everything from `origin` onward along `direction`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Where it starts.
    pub origin: [f64; 2],
    /// Which way it goes. Its length sets the parameter's scale, exactly as a
    /// line's start-to-end vector does.
    pub direction: [f64; 2],
}

/// An infinite construction line, unbounded in both directions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XLine {
    /// A point it passes through, and where its parameter reads zero.
    pub base: [f64; 2],
    /// Which way it runs.
    pub direction: [f64; 2],
}

/// How far a curve's parameter is allowed to run.
///
/// A single "is it bounded" flag would not do: a ray is bounded at one end and
/// not the other, and treating it as unbounded would accept crossings behind
/// its origin, where the ray does not go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extent {
    /// `0..=1`, the whole curve.
    Bounded,
    /// `0..=∞`. A ray.
    Forward,
    /// Everything. An infinite line.
    Infinite,
}

impl Extent {
    /// Whether `t` is on the curve rather than on its extension.
    pub fn holds(&self, t: f64) -> bool {
        const SLACK: f64 = 1e-9;
        match self {
            Self::Bounded => (-SLACK..=1.0 + SLACK).contains(&t),
            Self::Forward => t >= -SLACK,
            Self::Infinite => true,
        }
    }
}

/// Any plane curve this kernel understands.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// A straight segment.
    Line(Line),
    /// A full circle.
    Circle(Circle),
    /// A circular arc.
    Arc(Arc),
    /// An elliptical arc, up to the whole ellipse.
    Ellipse(EllipseArc),
    /// A chain of straight and arc segments.
    Polyline(Polyline),
    /// A NURBS curve — a drawing's SPLINE.
    Nurbs(NurbsCurve),
    /// A ray, bounded at its origin only.
    Ray(Ray),
    /// An infinite construction line.
    XLine(XLine),
}

impl Curve {
    /// How far this curve's parameter runs.
    pub fn extent(&self) -> Extent {
        match self {
            Self::Ray(_) => Extent::Forward,
            Self::XLine(_) => Extent::Infinite,
            _ => Extent::Bounded,
        }
    }

    /// Whether the curve is straight, and so answered in closed form against
    /// anything else this module knows.
    pub fn is_straight(&self) -> bool {
        matches!(self, Self::Line(_) | Self::Ray(_) | Self::XLine(_))
    }

    pub fn rectangle_side(&self, bounds: [[f64; 2]; 2]) -> Option<usize> {
        let endpoints = [self.point_at(0.0), self.point_at(1.0)];
        for fixed in 0..2 {
            let varying = 1 - fixed;
            for side in 0..2 {
                let fixed_value = bounds[fixed][side];
                let on_side = match self {
                    Self::Line(line) => [line.start, line.end]
                        .iter()
                        .all(|point| parameter_near(point[fixed], fixed_value)),
                    Self::Nurbs(curve) => curve.control_points().iter().all(|point| {
                        parameter_near(point[fixed], fixed_value)
                            && parameter_within(point[varying], bounds[varying])
                    }),
                    _ => return None,
                };
                let covers = (parameter_near(endpoints[0][varying], bounds[varying][0])
                    && parameter_near(endpoints[1][varying], bounds[varying][1]))
                    || (parameter_near(endpoints[1][varying], bounds[varying][0])
                        && parameter_near(endpoints[0][varying], bounds[varying][1]));
                if on_side && covers {
                    return Some(fixed * 2 + side);
                }
            }
        }
        None
    }

    /// A point on the curve and the vector its parameter advances by, for the
    /// straight kinds.
    pub fn as_ray(&self) -> Option<([f64; 2], [f64; 2])> {
        match self {
            Self::Line(line) => Some((line.start, line.direction())),
            Self::Ray(ray) => Some((ray.origin, ray.direction)),
            Self::XLine(line) => Some((line.base, line.direction)),
            _ => None,
        }
    }

    /// A polyline taken apart into the lines and arcs it is made of.
    ///
    /// Empty for everything else, which is what lets a caller ask without
    /// checking first.
    pub fn segments(&self) -> Vec<Curve> {
        let Self::Polyline(polyline) = self else {
            return Vec::new();
        };
        let count = polyline_segment_count(polyline);
        let vertices = polyline.vertices.len();
        (0..count)
            .map(|index| {
                let start = polyline.vertices[index].position;
                let end = polyline.vertices[(index + 1) % vertices].position;
                match polyline.segment_arc(index) {
                    Some(arc) => {
                        // Arcs here run counter-clockwise, so a clockwise bulge
                        // is stored with its ends the other way round. Only the
                        // shape matters to the caller, not the direction.
                        let (from, to) = if arc.sweep >= 0.0 {
                            (arc.start_angle, arc.start_angle + arc.sweep)
                        } else {
                            (arc.end_angle, arc.end_angle - arc.sweep)
                        };
                        Curve::Arc(Arc {
                            centre: arc.center,
                            radius: arc.radius,
                            start_angle: from,
                            end_angle: to,
                        })
                    }
                    None => Curve::Line(Line { start, end }),
                }
            })
            .collect()
    }

    /// Whether the curve returns to where it started.
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Line(_) => false,
            Self::Circle(_) => true,
            Self::Arc(arc) => (arc.sweep() - TAU).abs() < 1e-9,
            Self::Ellipse(arc) => (arc.sweep() - TAU).abs() < 1e-9,
            Self::Polyline(polyline) => polyline.closed,
            Self::Nurbs(curve) => curve.is_closed(),
            Self::Ray(_) | Self::XLine(_) => false,
        }
    }

    /// The point at parameter `t`, which runs `0..=1` from start to end.
    ///
    /// Outside that range the result continues along the curve's own
    /// extension where one exists, so an extend can ask where a parameter past
    /// the end would land.
    pub fn point_at(&self, t: f64) -> [f64; 2] {
        match self {
            Self::Line(line) => {
                let d = line.direction();
                [line.start[0] + t * d[0], line.start[1] + t * d[1]]
            }
            Self::Circle(circle) => {
                let angle = t * TAU;
                [
                    circle.centre[0] + circle.radius * angle.cos(),
                    circle.centre[1] + circle.radius * angle.sin(),
                ]
            }
            Self::Arc(arc) => {
                let angle = normalize_angle(arc.start_angle) + t * arc.sweep();
                [
                    arc.centre[0] + arc.radius * angle.cos(),
                    arc.centre[1] + arc.radius * angle.sin(),
                ]
            }
            Self::Ellipse(arc) => arc
                .ellipse
                .point_at(arc.start_parameter + t * arc.sweep()),
            Self::Polyline(polyline) => point_on_polyline(polyline, t),
            Self::Nurbs(curve) => curve.point_at(t),
            Self::Ray(ray) => [
                ray.origin[0] + t * ray.direction[0],
                ray.origin[1] + t * ray.direction[1],
            ],
            Self::XLine(line) => [
                line.base[0] + t * line.direction[0],
                line.base[1] + t * line.direction[1],
            ],
        }
    }

    /// The parameter at `point`, the inverse of [`point_at`](Self::point_at).
    ///
    /// The point is assumed to lie on the curve; one that does not is
    /// projected, which is what makes this usable for turning an intersection
    /// into a pair of parameters without every case having to carry them.
    pub fn parameter_at(&self, point: [f64; 2]) -> f64 {
        match self {
            Self::Line(line) => {
                let along = Vec2::from(line.direction());
                let squared = along.length_squared();
                if squared < 1e-24 {
                    return 0.0;
                }
                (Vec2::from(point) - Vec2::from(line.start)).dot(along) / squared
            }
            Self::Circle(circle) => {
                let angle = (point[1] - circle.centre[1]).atan2(point[0] - circle.centre[0]);
                normalize_angle(angle) / TAU
            }
            Self::Arc(arc) => {
                let angle = (point[1] - arc.centre[1]).atan2(point[0] - arc.centre[0]);
                arc_parameter(angle, arc.start_angle, arc.end_angle)
            }
            Self::Ellipse(arc) => {
                let parameter = arc.ellipse.closest_parameter(point);
                let travelled = (parameter - arc.start_parameter).rem_euclid(TAU);
                travelled / arc.sweep()
            }
            Self::Polyline(polyline) => parameter_on_polyline(polyline, point),
            Self::Nurbs(curve) => curve.parameter_at(point),
            Self::Ray(_) | Self::XLine(_) => {
                let (origin, direction) = self.as_ray().expect("straight");
                let along = Vec2::from(direction);
                let squared = along.length_squared();
                if squared < 1e-24 {
                    return 0.0;
                }
                (Vec2::from(point) - Vec2::from(origin)).dot(along) / squared
            }
        }
    }

    /// The number of segments a polyline has, or 1 for everything else.
    ///
    /// Parameters on a polyline are shared out evenly between its segments, so
    /// this is what converts between a segment-local position and a curve
    /// parameter.
    pub fn segment_count(&self) -> usize {
        match self {
            Self::Polyline(polyline) => polyline_segment_count(polyline),
            _ => 1,
        }
    }

    /// Samples the curve into a polyline of points.
    ///
    /// `segments_per_radian` sets how finely curved parts are cut; straight
    /// ones ignore it. Both endpoints are included.
    pub fn tessellate(&self, segments_per_radian: f64) -> Vec<[f64; 2]> {
        let max_angle = if segments_per_radian.is_finite() && segments_per_radian > 0.0 {
            1.0 / segments_per_radian
        } else {
            crate::tessellation::DEFAULT_ANGLE
        };
        self.tessellate_angle(max_angle)
    }
}

fn parameter_near(a: f64, b: f64) -> bool {
    (a - b).abs() <= f64::EPSILON * 128.0 * a.abs().max(b.abs()).max(1.0)
}

fn parameter_within(value: f64, bounds: [f64; 2]) -> bool {
    let scale = value
        .abs()
        .max(bounds[0].abs())
        .max(bounds[1].abs())
        .max(1.0);
    value >= bounds[0] - f64::EPSILON * 128.0 * scale
        && value <= bounds[1] + f64::EPSILON * 128.0 * scale
}

/// Segments in a polyline: one per vertex when closed, one fewer when open.
fn polyline_segment_count(polyline: &Polyline) -> usize {
    let vertices = polyline.vertices.len();
    if vertices < 2 {
        0
    } else if polyline.closed {
        vertices
    } else {
        vertices - 1
    }
}

/// Splits a curve parameter into a segment index and a position within it.
fn split_parameter(polyline: &Polyline, t: f64) -> Option<(usize, f64)> {
    let count = polyline_segment_count(polyline);
    if count == 0 {
        return None;
    }
    let scaled = (t * count as f64).clamp(0.0, count as f64);
    let index = (scaled.floor() as usize).min(count - 1);
    Some((index, scaled - index as f64))
}

fn point_on_polyline(polyline: &Polyline, t: f64) -> [f64; 2] {
    let Some((index, local)) = split_parameter(polyline, t) else {
        return polyline
            .vertices
            .first()
            .map(|v| v.position)
            .unwrap_or([0.0, 0.0]);
    };
    let count = polyline.vertices.len();
    let start = polyline.vertices[index].position;
    let end = polyline.vertices[(index + 1) % count].position;
    match polyline.segment_arc(index) {
        Some(arc) => arc.sample(local),
        None => Vec2::from(start).lerp(Vec2::from(end), local).to_array(),
    }
}

/// Nearest point on a polyline, as a curve parameter.
fn parameter_on_polyline(polyline: &Polyline, point: [f64; 2]) -> f64 {
    let count = polyline_segment_count(polyline);
    if count == 0 {
        return 0.0;
    }
    let vertices = polyline.vertices.len();
    let mut best = (f64::INFINITY, 0.0);
    for index in 0..count {
        let start = polyline.vertices[index].position;
        let end = polyline.vertices[(index + 1) % vertices].position;
        let local = match polyline.segment_arc(index) {
            Some(arc) => {
                let angle = (point[1] - arc.center[1]).atan2(point[0] - arc.center[0]);
                // `atan2` lands in -π..=π, so the difference has to be wrapped
                // into the direction the arc actually travels before it means
                // anything. Without this a point past the wrap reads as
                // negative and clamps to the arc's start.
                let travelled = if arc.sweep >= 0.0 {
                    (angle - arc.start_angle).rem_euclid(TAU)
                } else {
                    -((arc.start_angle - angle).rem_euclid(TAU))
                };
                (travelled / arc.sweep).clamp(0.0, 1.0)
            }
            None => {
                let along = Vec2::from(end) - Vec2::from(start);
                let squared = along.length_squared();
                if squared < 1e-24 {
                    0.0
                } else {
                    ((Vec2::from(point) - Vec2::from(start)).dot(along) / squared)
                        .clamp(0.0, 1.0)
                }
            }
        };
        let candidate = (index as f64 + local) / count as f64;
        let distance =
            Vec2::from(point_on_polyline(polyline, candidate)).distance_squared(Vec2::from(point));
        if distance < best.0 {
            best = (distance, candidate);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::polyline::PolylineVertex;
    use std::f64::consts::FRAC_PI_2;

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn every_curve_runs_from_zero_to_one() {
        let curves = [
            Curve::Line(Line {
                start: [1.0, 2.0],
                end: [5.0, 7.0],
            }),
            Curve::Arc(Arc {
                centre: [0.0, 0.0],
                radius: 3.0,
                start_angle: 0.0,
                end_angle: FRAC_PI_2,
            }),
            Curve::Ellipse(EllipseArc {
                ellipse: Ellipse {
                    centre: [2.0, 2.0],
                    major_radius: 4.0,
                    minor_radius: 1.0,
                    major_axis: [1.0, 0.0],
                },
                start_parameter: 0.0,
                end_parameter: 1.0,
            }),
        ];
        for curve in curves {
            let start = curve.point_at(0.0);
            let mid = curve.point_at(0.5);
            let end = curve.point_at(1.0);
            assert!(!close(start, end), "start and end should differ");
            assert!(!close(start, mid) && !close(mid, end));
        }
    }

    #[test]
    fn a_line_parameter_is_the_fraction_along_it() {
        let line = Curve::Line(Line {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
        });
        assert!(close(line.point_at(0.0), [0.0, 0.0]));
        assert!(close(line.point_at(0.25), [2.5, 0.0]));
        assert!(close(line.point_at(1.0), [10.0, 0.0]));
    }

    #[test]
    fn a_parameter_past_the_end_continues_the_line() {
        // What EXTEND needs: where would the line reach if it kept going.
        let line = Curve::Line(Line {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
        });
        assert!(close(line.point_at(1.5), [15.0, 0.0]));
        assert!(close(line.point_at(-0.5), [-5.0, 0.0]));
    }

    #[test]
    fn an_arc_parameter_walks_its_own_sweep() {
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 1.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        });
        assert!(close(arc.point_at(0.0), [1.0, 0.0]));
        assert!(close(arc.point_at(1.0), [0.0, 1.0]));
        let mid = arc.point_at(0.5);
        assert!((mid[0] - mid[1]).abs() < 1e-9, "should be at 45°");
    }

    #[test]
    fn closedness_is_reported_per_shape() {
        assert!(!Curve::Line(Line {
            start: [0.0, 0.0],
            end: [1.0, 0.0]
        })
        .is_closed());
        assert!(Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 1.0
        })
        .is_closed());
        assert!(Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 1.0,
            start_angle: 1.0,
            end_angle: 1.0,
        })
        .is_closed());
        assert!(!Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 1.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        })
        .is_closed());
    }

    #[test]
    fn a_polyline_shares_its_parameter_between_segments() {
        // Three straight segments; the parameter is a third each.
        let polyline = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([1.0, 0.0]),
                PolylineVertex::straight([2.0, 0.0]),
                PolylineVertex::straight([3.0, 0.0]),
            ],
            closed: false,
        });
        assert_eq!(polyline.segment_count(), 3);
        assert!(close(polyline.point_at(0.0), [0.0, 0.0]));
        assert!(close(polyline.point_at(1.0 / 3.0), [1.0, 0.0]));
        assert!(close(polyline.point_at(0.5), [1.5, 0.0]));
        assert!(close(polyline.point_at(1.0), [3.0, 0.0]));
    }

    #[test]
    fn a_closed_polyline_has_a_segment_back_to_the_start() {
        let polyline = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([1.0, 0.0]),
                PolylineVertex::straight([1.0, 1.0]),
            ],
            closed: true,
        });
        assert_eq!(polyline.segment_count(), 3);
        assert!(polyline.is_closed());
        assert!(close(polyline.point_at(1.0), [0.0, 0.0]), "should return home");
    }

    #[test]
    fn a_polyline_arc_segment_bulges() {
        let polyline = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::curved([0.0, 0.0], 1.0),
                PolylineVertex::straight([2.0, 0.0]),
            ],
            closed: false,
        });
        // Halfway along the single segment is the top of the half circle,
        // which for a positive bulge sits below the chord.
        let mid = polyline.point_at(0.5);
        assert!((mid[0] - 1.0).abs() < 1e-9);
        assert!((mid[1] + 1.0).abs() < 1e-9, "expected the arc, got {mid:?}");
    }

    #[test]
    fn tessellation_keeps_both_endpoints() {
        for curve in [
            Curve::Line(Line {
                start: [0.0, 0.0],
                end: [4.0, 3.0],
            }),
            Curve::Arc(Arc {
                centre: [0.0, 0.0],
                radius: 2.0,
                start_angle: 0.0,
                end_angle: 1.0,
            }),
        ] {
            let points = curve.tessellate(20.0);
            assert!(close(*points.first().unwrap(), curve.point_at(0.0)));
            assert!(close(*points.last().unwrap(), curve.point_at(1.0)));
        }
    }

    #[test]
    fn a_straight_line_needs_only_its_endpoints() {
        let line = Curve::Line(Line {
            start: [0.0, 0.0],
            end: [100.0, 0.0],
        });
        assert_eq!(line.tessellate(50.0).len(), 2);
    }

    #[test]
    fn a_tessellated_arc_stays_on_its_circle() {
        let arc = Curve::Arc(Arc {
            centre: [3.0, -4.0],
            radius: 2.5,
            start_angle: 0.5,
            end_angle: 4.0,
        });
        for point in arc.tessellate(20.0) {
            let dx = point[0] - 3.0;
            let dy = point[1] + 4.0;
            assert!(((dx * dx + dy * dy).sqrt() - 2.5).abs() < 1e-9);
        }
    }

    #[test]
    fn a_degenerate_polyline_does_not_panic() {
        let lone = Curve::Polyline(Polyline {
            vertices: vec![PolylineVertex::straight([2.0, 3.0])],
            closed: false,
        });
        assert_eq!(lone.segment_count(), 0);
        assert!(close(lone.point_at(0.5), [2.0, 3.0]));
        assert_eq!(lone.tessellate(20.0).len(), 1);

        let empty = Curve::Polyline(Polyline::new());
        assert!(close(empty.point_at(0.5), [0.0, 0.0]));
        assert!(empty.tessellate(20.0).is_empty());
    }
}
