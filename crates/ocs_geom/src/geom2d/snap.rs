//! The points a curve offers to snap to.
//!
//! A snapper has two halves and they belong in different places. Deciding
//! *which* curve the cursor is near is a screen-space question — the tolerance
//! is in pixels, only what is visible can be picked, and in a rotated view
//! "near" means near on screen rather than in the drawing. That half stays with
//! the application.
//!
//! Deciding *where* on that curve is this half, and it has to come from the
//! curve rather than from a polyline standing in for it. The centre of a circle
//! is not a vertex of its tessellation; the perpendicular foot on an arc is not
//! the perpendicular foot on one of its chords. At twenty segments per radian a
//! chord sits about `3e-4 · r` from the arc, which on a hundred-metre radius is
//! thirty centimetres of snapping to the wrong place — and it moves when the
//! view is zoomed, so the same pick twice gives two answers.

use super::angle::angle_within_arc;
use super::containment::closest_point;
use super::curve::{Curve, Extent};
use super::vec::Vec2;
use std::f64::consts::{FRAC_PI_2, TAU};

/// What kind of point a snap candidate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    /// An end of the curve, or a vertex of a polyline.
    Endpoint,
    /// Halfway along, by parameter.
    Midpoint,
    /// The centre a circle, arc or ellipse turns about.
    Centre,
    /// Where a circular or elliptical curve crosses one of its own axes.
    Quadrant,
    /// The foot of a perpendicular dropped from somewhere else.
    Perpendicular,
    /// Where a line from somewhere else touches the curve without crossing it.
    Tangent,
}

/// A point worth snapping to, and where it sits on the curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapPoint {
    /// Why this point is offered.
    pub kind: SnapKind,
    /// Where it is.
    pub point: [f64; 2],
    /// Its parameter on the curve. `Centre` has none — it is not on the curve
    /// at all — and reports zero.
    pub t: f64,
}

impl SnapPoint {
    fn new(kind: SnapKind, point: [f64; 2], t: f64) -> Self {
        Self { kind, point, t }
    }
}

/// Every point a curve offers without being asked about a cursor position:
/// ends, middles, centres and quadrants.
///
/// A closed curve has no ends to offer. A full circle has no midpoint either,
/// since every point on it is as much the middle as any other.
pub fn characteristic_points(curve: &Curve) -> Vec<SnapPoint> {
    let mut out = Vec::new();

    match curve {
        Curve::Polyline(polyline) => {
            // Every vertex is the end of a segment, and every segment has its
            // own middle — which is what a polyline offers in a drawing, not
            // just the two points at the far ends of the chain.
            let segments = curve.segments();
            let count = segments.len();
            for (index, segment) in segments.iter().enumerate() {
                let span = |t: f64| (index as f64 + t) / count as f64;
                out.push(SnapPoint::new(
                    SnapKind::Endpoint,
                    segment.point_at(0.0),
                    span(0.0),
                ));
                out.push(SnapPoint::new(
                    SnapKind::Midpoint,
                    segment.point_at(0.5),
                    span(0.5),
                ));
                if let Curve::Arc(arc) = segment {
                    out.push(SnapPoint::new(SnapKind::Centre, arc.centre, 0.0));
                    // An arc segment has the quadrants its sweep covers, as
                    // an arc does.
                    let arc_quadrants =
                        quadrants(segment, arc.centre, Some((arc.start_angle, arc.end_angle)));
                    out.extend(arc_quadrants.into_iter().map(|q| SnapPoint { t: span(q.t), ..q }));
                }
            }
            if !polyline.closed {
                if let Some(last) = segments.last() {
                    out.push(SnapPoint::new(SnapKind::Endpoint, last.point_at(1.0), 1.0));
                }
            }
        }

        _ => {
            if !curve.is_closed() && curve.extent() == Extent::Bounded {
                out.push(SnapPoint::new(SnapKind::Endpoint, curve.point_at(0.0), 0.0));
                out.push(SnapPoint::new(SnapKind::Endpoint, curve.point_at(1.0), 1.0));
                out.push(SnapPoint::new(SnapKind::Midpoint, curve.point_at(0.5), 0.5));
            } else if curve.extent() == Extent::Forward {
                // A ray has the one end it starts from.
                out.push(SnapPoint::new(SnapKind::Endpoint, curve.point_at(0.0), 0.0));
            }

            match curve {
                Curve::Circle(circle) => {
                    out.push(SnapPoint::new(SnapKind::Centre, circle.centre, 0.0));
                    out.extend(quadrants(curve, circle.centre, None));
                }
                Curve::Arc(arc) => {
                    out.push(SnapPoint::new(SnapKind::Centre, arc.centre, 0.0));
                    out.extend(quadrants(
                        curve,
                        arc.centre,
                        Some((arc.start_angle, arc.end_angle)),
                    ));
                }
                Curve::Ellipse(arc) => {
                    out.push(SnapPoint::new(SnapKind::Centre, arc.ellipse.centre, 0.0));
                    out.extend(ellipse_quadrants(arc));
                }
                _ => {}
            }
        }
    }

    out
}

/// The four axis crossings of a circle, keeping those the arc actually covers.
fn quadrants(curve: &Curve, centre: [f64; 2], arc: Option<(f64, f64)>) -> Vec<SnapPoint> {
    let centre = Vec2::from(centre);
    (0..4)
        .filter_map(|i| {
            let angle = i as f64 * FRAC_PI_2;
            if let Some((start, end)) = arc {
                if !angle_within_arc(angle, start, end) {
                    return None;
                }
            }
            // Recovering the parameter from the point keeps this working for
            // both a full circle and an arc without a case for each.
            let radius = Vec2::from(curve.point_at(0.0)).distance(centre);
            let point = (centre + Vec2::new(angle.cos(), angle.sin()) * radius).to_array();
            Some(SnapPoint::new(
                SnapKind::Quadrant,
                point,
                curve.parameter_at(point),
            ))
        })
        .collect()
}

/// The ends of an ellipse's own axes, keeping those its arc covers.
fn ellipse_quadrants(arc: &super::curve::EllipseArc) -> Vec<SnapPoint> {
    let sweep = arc.sweep();
    (0..4)
        .filter_map(|i| {
            let parameter = i as f64 * FRAC_PI_2;
            let travelled = (parameter - arc.start_parameter).rem_euclid(TAU);
            (travelled <= sweep + 1e-9).then(|| {
                SnapPoint::new(
                    SnapKind::Quadrant,
                    arc.ellipse.point_at(parameter),
                    travelled / sweep,
                )
            })
        })
        .collect()
}

/// Where a perpendicular dropped from `from` meets the curve.
///
/// A perpendicular foot is a place where the line back to `from` meets the
/// curve at a right angle, which is the same as saying the distance to `from`
/// stops changing there. A line has one, a circle has two — the near side and
/// the far — and a wandering curve can have several.
///
/// Feet that fall outside the curve's own extent are dropped: a perpendicular
/// onto the extension of a segment is not on the segment.
pub fn perpendicular_from(curve: &Curve, from: [f64; 2]) -> Vec<SnapPoint> {
    let feet: Vec<f64> = match curve {
        // The projection, in closed form.
        Curve::Line(_) | Curve::Ray(_) | Curve::XLine(_) => vec![curve.parameter_at(from)],

        // Along the radius, so the near and far sides of the circle.
        Curve::Circle(_) | Curve::Arc(_) => {
            let (centre, radius) = match curve {
                Curve::Circle(c) => (c.centre, c.radius),
                Curve::Arc(a) => (a.centre, a.radius),
                _ => unreachable!(),
            };
            let centre = Vec2::from(centre);
            match (Vec2::from(from) - centre).normalize() {
                // `from` is the centre itself: every direction is
                // perpendicular, so none is worth offering.
                None => Vec::new(),
                Some(direction) => [1.0, -1.0]
                    .into_iter()
                    .map(|side| (centre + direction * (radius * side)).to_array())
                    .map(|point| curve.parameter_at(point))
                    .collect(),
            }
        }

        // No closed form worth having: the feet are where the distance stops
        // changing, so look for those.
        _ => stationary_distances(curve, from),
    };

    feet.into_iter()
        .filter(|t| curve.extent().holds(*t))
        .map(|t| {
            let t = match curve.extent() {
                Extent::Bounded => t.clamp(0.0, 1.0),
                _ => t,
            };
            SnapPoint::new(SnapKind::Perpendicular, curve.point_at(t), t)
        })
        .collect()
}

/// Parameters where the distance to `from` stops changing — the perpendicular
/// feet of a curve with no closed form.
///
/// Sampled to bracket each turning point, then narrowed. Sampling only decides
/// *where* to look; the answer itself is refined until it stops moving.
fn stationary_distances(curve: &Curve, from: [f64; 2]) -> Vec<f64> {
    const SAMPLES: usize = 128;
    let from = Vec2::from(from);
    // Rate of change of distance along the curve, by finite difference.
    let slope = |t: f64| {
        let step = 1e-7;
        let before = Vec2::from(curve.point_at((t - step).max(0.0))).distance(from);
        let after = Vec2::from(curve.point_at((t + step).min(1.0))).distance(from);
        after - before
    };

    let mut out = Vec::new();
    let mut previous = slope(0.0);
    for i in 1..=SAMPLES {
        let t = i as f64 / SAMPLES as f64;
        let current = slope(t);
        if previous == 0.0 || (previous < 0.0) != (current < 0.0) {
            // A turning point between the last sample and this one.
            let (mut low, mut high) = ((i - 1) as f64 / SAMPLES as f64, t);
            for _ in 0..60 {
                let middle = (low + high) * 0.5;
                if (slope(low) < 0.0) == (slope(middle) < 0.0) {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            out.push((low + high) * 0.5);
        }
        previous = current;
    }
    out
}

/// Where a line from `from` touches the curve without crossing it.
///
/// Only circles and arcs have tangents worth offering, and only from outside:
/// from within a circle no line touches it, and from a point on it the tangent
/// is the point itself, which a snapper already offers by other means.
pub fn tangent_from(curve: &Curve, from: [f64; 2]) -> Vec<SnapPoint> {
    let (centre, radius) = match curve {
        Curve::Circle(circle) => (circle.centre, circle.radius),
        Curve::Arc(arc) => (arc.centre, arc.radius),
        _ => return Vec::new(),
    };
    let centre = Vec2::from(centre);
    let away = Vec2::from(from) - centre;
    let distance = away.length();
    if distance <= radius + 1e-12 {
        return Vec::new();
    }

    // The tangent points sit at ±acos(r / d) from the direction back to
    // `from`: the radius, the tangent and the line to `from` make a right
    // triangle with the radius adjacent.
    let base = away.angle();
    let offset = (radius / distance).acos();
    [base + offset, base - offset]
        .into_iter()
        .filter_map(|angle| {
            let point = (centre + Vec2::new(angle.cos(), angle.sin()) * radius).to_array();
            let t = curve.parameter_at(point);
            // An arc only offers the tangent points it actually covers.
            let on_curve = Vec2::from(curve.point_at(t)).distance(Vec2::from(point)) < 1e-6;
            on_curve.then(|| SnapPoint::new(SnapKind::Tangent, point, t))
        })
        .collect()
}

/// The point on the curve nearest `from`, as a snap candidate.
///
/// The same answer [`closest_point`] gives, in the shape the rest of this
/// module speaks in.
pub fn nearest_to(curve: &Curve, from: [f64; 2]) -> SnapPoint {
    let found = closest_point(curve, from);
    SnapPoint::new(SnapKind::Perpendicular, found.point, found.t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::curve::{Arc, Circle, EllipseArc, Line, Ray};
    use crate::geom2d::nurbs::{NurbsCurve, Parameterization};
    use crate::geom2d::polyline::{Polyline, PolylineVertex};
    use crate::geom2d::Ellipse;
    use std::f64::consts::PI;

    fn segment(start: [f64; 2], end: [f64; 2]) -> Curve {
        Curve::Line(Line { start, end })
    }

    fn of_kind(points: &[SnapPoint], kind: SnapKind) -> Vec<[f64; 2]> {
        points
            .iter()
            .filter(|p| p.kind == kind)
            .map(|p| p.point)
            .collect()
    }

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        Vec2::from(a).distance(Vec2::from(b)) < 1e-9
    }

    #[test]
    fn a_segment_offers_its_ends_and_middle() {
        let points = characteristic_points(&segment([0.0, 0.0], [10.0, 4.0]));
        let ends = of_kind(&points, SnapKind::Endpoint);
        assert_eq!(ends.len(), 2);
        assert!(ends.iter().any(|p| near(*p, [0.0, 0.0])));
        assert!(ends.iter().any(|p| near(*p, [10.0, 4.0])));
        assert!(near(of_kind(&points, SnapKind::Midpoint)[0], [5.0, 2.0]));
    }

    #[test]
    fn a_polyline_arc_segment_offers_its_centre_and_quadrants() {
        use crate::geom2d::{Polyline, PolylineVertex};
        // A straight run, then a half turn bulging below the chord.
        let chain = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex { position: [-2.0, 0.0], bulge: 0.0 },
                PolylineVertex { position: [0.0, 0.0], bulge: 1.0 },
                PolylineVertex { position: [2.0, 0.0], bulge: 0.0 },
            ],
            closed: false,
        });
        let points = characteristic_points(&chain);
        assert!(near(of_kind(&points, SnapKind::Centre)[0], [1.0, 0.0]));
        let quadrants = of_kind(&points, SnapKind::Quadrant);
        assert!(quadrants.iter().any(|p| near(*p, [1.0, -1.0])), "{quadrants:?}");
        assert!(!quadrants.iter().any(|p| near(*p, [1.0, 1.0])), "{quadrants:?}");
    }

    #[test]
    fn a_circle_offers_a_centre_and_four_quadrants_but_no_ends() {
        let circle = Curve::Circle(Circle {
            centre: [3.0, 4.0],
            radius: 2.0,
        });
        let points = characteristic_points(&circle);
        assert!(near(of_kind(&points, SnapKind::Centre)[0], [3.0, 4.0]));
        assert_eq!(of_kind(&points, SnapKind::Quadrant).len(), 4);
        assert!(of_kind(&points, SnapKind::Endpoint).is_empty(), "a circle has no ends");
        assert!(
            of_kind(&points, SnapKind::Midpoint).is_empty(),
            "nor a middle — every point is as much the middle as any other"
        );
    }

    #[test]
    fn the_quadrants_are_on_the_circle_and_on_its_axes() {
        let circle = Curve::Circle(Circle {
            centre: [3.0, 4.0],
            radius: 2.0,
        });
        for point in of_kind(&characteristic_points(&circle), SnapKind::Quadrant) {
            assert!((Vec2::from(point).distance(Vec2::new(3.0, 4.0)) - 2.0).abs() < 1e-9);
            let on_axis = (point[0] - 3.0).abs() < 1e-9 || (point[1] - 4.0).abs() < 1e-9;
            assert!(on_axis, "{point:?} is not on an axis");
        }
    }

    #[test]
    fn an_arc_offers_only_the_quadrants_it_covers() {
        // First quadrant only: the crossing at 0° and the one at 90°.
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 5.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        });
        let quadrants = of_kind(&characteristic_points(&arc), SnapKind::Quadrant);
        assert_eq!(quadrants.len(), 2, "got {quadrants:?}");
        assert!(quadrants.iter().any(|p| near(*p, [5.0, 0.0])));
        assert!(quadrants.iter().any(|p| near(*p, [0.0, 5.0])));
    }

    #[test]
    fn an_arc_still_has_ends_and_a_middle() {
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 5.0,
            start_angle: 0.0,
            end_angle: PI,
        });
        let points = characteristic_points(&arc);
        assert_eq!(of_kind(&points, SnapKind::Endpoint).len(), 2);
        assert!(near(of_kind(&points, SnapKind::Midpoint)[0], [0.0, 5.0]));
    }

    #[test]
    fn a_polyline_offers_every_vertex_and_every_segment_middle() {
        let chain = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::straight([0.0, 0.0]),
                PolylineVertex::straight([2.0, 0.0]),
                PolylineVertex::straight([2.0, 2.0]),
            ],
            closed: false,
        });
        let points = characteristic_points(&chain);
        let ends = of_kind(&points, SnapKind::Endpoint);
        assert_eq!(ends.len(), 3, "all three vertices, got {ends:?}");
        let middles = of_kind(&points, SnapKind::Midpoint);
        assert_eq!(middles.len(), 2, "one per segment");
        assert!(middles.iter().any(|p| near(*p, [1.0, 0.0])));
        assert!(middles.iter().any(|p| near(*p, [2.0, 1.0])));
    }

    #[test]
    fn a_polyline_arc_segment_offers_its_centre() {
        let chain = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex::curved([0.0, 0.0], 1.0),
                PolylineVertex::straight([2.0, 0.0]),
            ],
            closed: false,
        });
        let centres = of_kind(&characteristic_points(&chain), SnapKind::Centre);
        assert_eq!(centres.len(), 1);
        assert!(near(centres[0], [1.0, 0.0]));
    }

    #[test]
    fn a_ray_offers_the_one_end_it_has() {
        let ray = Curve::Ray(Ray {
            origin: [1.0, 2.0],
            direction: [1.0, 0.0],
        });
        let ends = of_kind(&characteristic_points(&ray), SnapKind::Endpoint);
        assert_eq!(ends.len(), 1);
        assert!(near(ends[0], [1.0, 2.0]));
    }

    #[test]
    fn the_perpendicular_onto_a_segment_is_the_foot() {
        let feet = perpendicular_from(&segment([0.0, 0.0], [10.0, 0.0]), [4.0, 3.0]);
        assert_eq!(feet.len(), 1);
        assert!(near(feet[0].point, [4.0, 0.0]));
    }

    #[test]
    fn a_perpendicular_that_would_land_past_the_end_is_not_offered() {
        // The foot is at x = 50, well off the segment.
        assert!(perpendicular_from(&segment([0.0, 0.0], [10.0, 0.0]), [50.0, 3.0]).is_empty());
    }

    #[test]
    fn a_circle_has_two_perpendicular_feet() {
        let circle = Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 5.0,
        });
        let feet = perpendicular_from(&circle, [20.0, 0.0]);
        assert_eq!(feet.len(), 2, "the near side and the far, got {feet:?}");
        assert!(feet.iter().any(|f| near(f.point, [5.0, 0.0])));
        assert!(feet.iter().any(|f| near(f.point, [-5.0, 0.0])));
    }

    #[test]
    fn the_centre_of_a_circle_has_no_perpendicular_to_offer() {
        // Every direction is perpendicular, so none of them is a snap.
        let circle = Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 5.0,
        });
        assert!(perpendicular_from(&circle, [0.0, 0.0]).is_empty());
    }

    #[test]
    fn a_perpendicular_onto_a_spline_meets_it_at_a_right_angle() {
        let spline = Curve::Nurbs(
            NurbsCurve::interpolate(
                &[[0.0, 0.0], [2.0, 4.0], [6.0, -2.0], [9.0, 3.0]],
                None,
                None,
                Parameterization::Chord,
            )
            .unwrap(),
        );
        let from = [4.0, 6.0];
        let feet = perpendicular_from(&spline, from);
        assert!(!feet.is_empty());
        for foot in feet {
            // The curve's own direction there, against the direction back to
            // `from`: perpendicular means they barely agree.
            let step = 1e-5;
            let before = Vec2::from(spline.point_at((foot.t - step).max(0.0)));
            let after = Vec2::from(spline.point_at((foot.t + step).min(1.0)));
            let Some(along) = (after - before).normalize() else {
                continue;
            };
            let Some(back) = (Vec2::from(from) - Vec2::from(foot.point)).normalize() else {
                continue;
            };
            assert!(
                along.dot(back).abs() < 1e-4,
                "not perpendicular: {}",
                along.dot(back)
            );
        }
    }

    #[test]
    fn an_ellipse_offers_the_axis_ends_it_covers() {
        let whole = Curve::Ellipse(EllipseArc::full(Ellipse {
            centre: [0.0, 0.0],
            major_radius: 5.0,
            minor_radius: 2.0,
            major_axis: [1.0, 0.0],
        }));
        let quadrants = of_kind(&characteristic_points(&whole), SnapKind::Quadrant);
        assert_eq!(quadrants.len(), 4);
        assert!(quadrants.iter().any(|p| near(*p, [5.0, 0.0])));
        assert!(quadrants.iter().any(|p| near(*p, [0.0, 2.0])));
    }

    #[test]
    fn tangents_are_offered_from_outside_a_circle_only() {
        let circle = Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 3.0,
        });
        let touches = tangent_from(&circle, [5.0, 0.0]);
        assert_eq!(touches.len(), 2);
        for touch in &touches {
            // On the circle, and the radius is square to the line back.
            let radial = Vec2::from(touch.point);
            assert!((radial.length() - 3.0).abs() < 1e-9);
            let back = Vec2::new(5.0, 0.0) - radial;
            assert!(radial.dot(back).abs() < 1e-9, "not tangent");
        }
        // From inside, nothing touches.
        assert!(tangent_from(&circle, [1.0, 0.0]).is_empty());
        assert!(tangent_from(&segment([0.0, 0.0], [1.0, 1.0]), [5.0, 5.0]).is_empty());
    }

    #[test]
    fn survey_coordinates_snap_to_the_same_places() {
        let origin = [512_345.678, 4_512_345.678];
        let circle = Curve::Circle(Circle {
            centre: origin,
            radius: 2.0,
        });
        let points = characteristic_points(&circle);
        assert!(near(of_kind(&points, SnapKind::Centre)[0], origin));
        for quadrant in of_kind(&points, SnapKind::Quadrant) {
            assert!((Vec2::from(quadrant).distance(Vec2::from(origin)) - 2.0).abs() < 1e-9);
        }
    }
}
