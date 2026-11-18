//! The parallel of a single curve at a signed distance.
//!
//! Lines, rays, infinite lines, circles and arcs have exact parallels of the
//! same kind. An ellipse's or a spline's parallel is neither — an ellipse's is
//! not an ellipse at all — so it comes back as a cubic NURBS fitted through
//! true offset points, refined until it stays within the asked tolerance of
//! the true offset everywhere between them. A polyline is the `offset`
//! feature's [`offset_polyline`](super::offset_polyline), which also resolves
//! the pieces a parallel loses at its corners.
//!
//! Positive distances go to the left of the curve's direction of travel.

use super::curve::{Arc, Circle, Curve, Line, Ray, XLine};
use super::nurbs::{NurbsCurve, Parameterization};
use super::vec::Vec2;

/// The curve parallel to `curve` at `distance` (positive to its left), with
/// a fitted result staying within `tolerance` of the true offset.
///
/// `None` for a polyline (see the module notes), for a circle or arc the
/// offset would shrink to nothing or turn inside out, and for a curve whose
/// direction vanishes along its length.
pub fn offset_curve(curve: &Curve, distance: f64, tolerance: f64) -> Option<Curve> {
    if !distance.is_finite() || !(tolerance > 0.0) {
        return None;
    }
    let shift = |p: [f64; 2], along: [f64; 2]| -> Option<[f64; 2]> {
        let n = Vec2::from(along).normalize()?.perpendicular();
        Some((Vec2::from(p) + n * distance).to_array())
    };
    match curve {
        Curve::Line(line) => {
            let along = [line.end[0] - line.start[0], line.end[1] - line.start[1]];
            Some(Curve::Line(Line {
                start: shift(line.start, along)?,
                end: shift(line.end, along)?,
            }))
        }
        Curve::Ray(ray) => Some(Curve::Ray(Ray {
            origin: shift(ray.origin, ray.direction)?,
            direction: ray.direction,
        })),
        Curve::XLine(line) => Some(Curve::XLine(XLine {
            base: shift(line.base, line.direction)?,
            direction: line.direction,
        })),
        // Travelling counter-clockwise, the left is the centre.
        Curve::Circle(circle) => {
            let radius = circle.radius - distance;
            (radius > 0.0).then_some(Curve::Circle(Circle { centre: circle.centre, radius }))
        }
        Curve::Arc(arc) => {
            let radius = arc.radius - distance;
            (radius > 0.0).then_some(Curve::Arc(Arc { radius, ..*arc }))
        }
        Curve::Polyline(_) => None,
        Curve::Ellipse(_) | Curve::Nurbs(_) => fitted(curve, distance, tolerance).map(Curve::Nurbs),
    }
}

/// A cubic through true offset points, sampled more densely until the fit
/// holds `tolerance` halfway between every pair of samples.
///
/// Chord-length parameters, so a unit end tangent is the fit's true end
/// derivative and each sample's place on the fit is known: the deviation
/// check projects locally from there instead of searching the whole curve.
/// A closed source is fitted open with matching end tangents — C1 at the
/// seam rather than C2, which a drawn spline does not show.
fn fitted(curve: &Curve, distance: f64, tolerance: f64) -> Option<NurbsCurve> {
    // Work near the origin: survey coordinates would spend the digits the
    // tolerance needs. Translation leaves the parameters alone.
    let anchor = curve.point_at(0.0);
    let local = curve.transformed(&super::Transform::translation([-anchor[0], -anchor[1]]))?;
    let closed = local.is_closed();
    let point = |t: f64| Vec2::from(local.point_at(t));
    let tangent = |t: f64| -> Option<Vec2> {
        let h = 1e-7;
        if closed {
            return (point((t + h).rem_euclid(1.0)) - point((t - h).rem_euclid(1.0))).normalize();
        }
        (point((t + h).min(1.0)) - point((t - h).max(0.0))).normalize()
    };
    let offset =
        |t: f64| -> Option<Vec2> { Some(point(t) + tangent(t)?.perpendicular() * distance) };
    let mut samples = 8usize;
    while samples <= 8192 {
        let params: Vec<f64> = (0..=samples).map(|i| i as f64 / samples as f64).collect();
        let points: Vec<Vec2> = params.iter().map(|t| offset(*t)).collect::<Option<_>>()?;
        let raw: Vec<[f64; 2]> = points.iter().map(|p| p.to_array()).collect();
        let fit = NurbsCurve::interpolate(
            &raw,
            Some(tangent(0.0)?.to_array()),
            Some(tangent(1.0)?.to_array()),
            Parameterization::Chord,
        )?;
        // Where each sample sits on the fit, normalised.
        let mut along = vec![0.0f64; points.len()];
        for i in 1..points.len() {
            along[i] = along[i - 1] + points[i].distance(points[i - 1]).max(1e-9);
        }
        let total = along[along.len() - 1];
        let mut worst = 0.0f64;
        for i in 0..samples {
            let Some(truth) = offset((params[i] + params[i + 1]) * 0.5) else {
                worst = f64::INFINITY;
                break;
            };
            let (lo, hi) = (along[i] / total, along[i + 1] / total);
            worst = worst.max(local_distance(&fit, truth, lo, hi));
            if worst > tolerance {
                break;
            }
        }
        if worst <= tolerance {
            let back = super::Transform::translation(anchor);
            return match Curve::Nurbs(fit).transformed(&back)? {
                Curve::Nurbs(curve) => Some(curve),
                _ => None,
            };
        }
        samples *= 2;
    }
    None
}

/// Distance from `target` to the fit between normalised parameters `lo` and
/// `hi`, by a ternary search on that span alone.
fn local_distance(fit: &NurbsCurve, target: Vec2, lo: f64, hi: f64) -> f64 {
    let at = |u: f64| Vec2::from(fit.point_at(u)).distance(target);
    let (mut lo, mut hi) = (lo, hi);
    for _ in 0..60 {
        let third = (hi - lo) / 3.0;
        if third <= 1e-16 {
            break;
        }
        if at(lo + third) < at(hi - third) {
            hi -= third;
        } else {
            lo += third;
        }
    }
    at((lo + hi) * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::{closest_point, EllipseArc};

    fn ellipse(centre: [f64; 2]) -> Curve {
        Curve::Ellipse(EllipseArc::full(crate::geom2d::Ellipse {
            centre,
            major_radius: 7.0,
            minor_radius: 3.0,
            major_axis: [1.0, 0.0],
        }))
    }

    /// Every point of the result sits `|distance|` from the source.
    fn assert_parallel(source: &Curve, result: &Curve, distance: f64, tolerance: f64) {
        for i in 0..=200 {
            let p = result.point_at(i as f64 / 200.0);
            let d = closest_point(source, p).distance;
            assert!((d - distance.abs()).abs() <= tolerance * 2.0, "off by {}", d - distance.abs());
        }
    }

    #[test]
    fn an_ellipse_offsets_to_a_true_parallel_not_a_scaled_ellipse() {
        let source = ellipse([0.0, 0.0]);
        for distance in [1.0, -1.0] {
            let result = offset_curve(&source, distance, 1e-6).expect("offset");
            assert!(matches!(result, Curve::Nurbs(_)));
            assert_parallel(&source, &result, distance, 1e-6);
        }
    }

    #[test]
    fn a_spline_offsets_within_tolerance_at_survey_coordinates() {
        let origin = [1_234_567.0, 4_567_890.0];
        let spline = Curve::Nurbs(
            NurbsCurve::new(
                3,
                [[-9.0, -5.0], [-4.0, 7.0], [0.0, -7.0], [4.0, 8.0], [9.0, -4.0]]
                    .map(|[x, y]| [origin[0] + x, origin[1] + y])
                    .to_vec(),
                vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
                None,
            )
            .unwrap(),
        );
        let result = offset_curve(&spline, 0.5, 1e-5).expect("offset");
        assert_parallel(&spline, &result, 0.5, 1e-5);
    }

    #[test]
    fn arcs_and_circles_keep_their_kind() {
        let arc =
            Curve::Arc(Arc { centre: [0.0, 0.0], radius: 5.0, start_angle: 0.0, end_angle: 1.0 });
        let inner = offset_curve(&arc, 1.0, 1e-6);
        assert!(matches!(inner, Some(Curve::Arc(a)) if (a.radius - 4.0).abs() < 1e-12));
        assert!(offset_curve(&arc, 6.0, 1e-6).is_none());
    }
}
