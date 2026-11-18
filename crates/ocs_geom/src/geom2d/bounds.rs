//! Analytic planar curve extents for placement and measurement.

use super::angle::angle_within_arc;
use super::{Curve, Ellipse};
use std::f64::consts::{PI, TAU};

type Bounds = ([f64; 2], [f64; 2]);

fn absorb(bounds: &mut Option<Bounds>, point: [f64; 2]) -> Option<()> {
    if !point.iter().all(|coordinate| coordinate.is_finite()) {
        return None;
    }
    if let Some((min, max)) = bounds {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    } else {
        *bounds = Some((point, point));
    }
    Some(())
}

fn conic(bounds: &mut Option<Bounds>, ellipse: Ellipse, start: f64, end: f64) -> Option<()> {
    if !start.is_finite()
        || !end.is_finite()
        || !ellipse.major_radius.is_finite()
        || !ellipse.minor_radius.is_finite()
        || ellipse.major_radius <= 0.0
        || ellipse.minor_radius <= 0.0
        || ellipse.major_axis.iter().any(|value| !value.is_finite())
    {
        return None;
    }
    absorb(bounds, ellipse.point_at(start))?;
    absorb(bounds, ellipse.point_at(end))?;
    let minor = ellipse.minor_axis();
    for axis in 0..2 {
        // The coordinate derivative is -a*sin(t) + b*cos(t).
        let maximum = (ellipse.minor_radius * minor[axis]).atan2(ellipse.major_radius * ellipse.major_axis[axis]);
        for parameter in [maximum, maximum + PI] {
            if (end - start).abs() >= TAU || angle_within_arc(parameter, start, end) {
                absorb(bounds, ellipse.point_at(parameter))?;
            }
        }
    }
    Some(())
}

fn circular(
    bounds: &mut Option<Bounds>,
    centre: [f64; 2],
    radius: f64,
    start: f64,
    end: f64,
) -> Option<()> {
    conic(
        bounds,
        Ellipse {
            centre,
            major_radius: radius,
            minor_radius: radius,
            major_axis: [1.0, 0.0],
        },
        start,
        end,
    )
}

/// Tight axis-aligned extents of bounded analytic curves, including partial conics
/// and signed polyline bulges. Returns `None` for invalid/unbounded inputs, empty
/// collections or NURBS, whose extrema require a separate root-isolation algorithm.
/// This deliberately never substitutes a tessellation or control hull.
pub fn analytic_curve_bounds(curves: &[Curve]) -> Option<Bounds> {
    let mut bounds = None;
    for curve in curves {
        match curve {
            Curve::Line(line) => {
                absorb(&mut bounds, line.start)?;
                absorb(&mut bounds, line.end)?;
            }
            Curve::Circle(circle) => {
                circular(&mut bounds, circle.centre, circle.radius, 0.0, TAU)?;
            }
            Curve::Arc(arc) => {
                circular(
                    &mut bounds,
                    arc.centre,
                    arc.radius,
                    arc.start_angle,
                    arc.end_angle,
                )?;
            }
            Curve::Ellipse(arc) => {
                conic(
                    &mut bounds,
                    arc.ellipse,
                    arc.start_parameter,
                    arc.end_parameter,
                )?;
            }
            Curve::Polyline(polyline) => {
                for vertex in &polyline.vertices {
                    absorb(&mut bounds, vertex.position)?;
                    if !vertex.bulge.is_finite() {
                        return None;
                    }
                }
                let count = if polyline.closed {
                    polyline.vertices.len()
                } else {
                    polyline.vertices.len().saturating_sub(1)
                };
                for index in 0..count {
                    if let Some(arc) = polyline.segment_arc(index) {
                        let (start, end) = if arc.sweep >= 0.0 {
                            (arc.start_angle, arc.start_angle + arc.sweep)
                        } else {
                            (arc.start_angle + arc.sweep, arc.start_angle)
                        };
                        circular(&mut bounds, arc.center, arc.radius, start, end)?;
                    }
                }
            }
            Curve::Ray(_) | Curve::XLine(_) | Curve::Nurbs(_) => return None,
        }
    }
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::{Arc, Circle, EllipseArc, Line, Polyline, PolylineVertex, XLine};
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

    fn close(actual: Bounds, expected: Bounds) {
        for axis in 0..2 {
            assert!((actual.0[axis] - expected.0[axis]).abs() < 1e-9, "{actual:?}");
            assert!((actual.1[axis] - expected.1[axis]).abs() < 1e-9, "{actual:?}");
        }
    }

    #[test]
    fn partial_conics_include_only_extrema_on_their_spans() {
        close(
            analytic_curve_bounds(&[Curve::Arc(Arc {
                centre: [2.0, 3.0],
                radius: 4.0,
                start_angle: 0.0,
                end_angle: FRAC_PI_2,
            })])
            .unwrap(),
            ([2.0, 3.0], [6.0, 7.0]),
        );

        let extent = 10.0_f64.sqrt();
        close(
            analytic_curve_bounds(&[Curve::Ellipse(EllipseArc {
                ellipse: Ellipse {
                    centre: [1.0, -2.0],
                    major_radius: 4.0,
                    minor_radius: 2.0,
                    major_axis: [FRAC_1_SQRT_2, FRAC_1_SQRT_2],
                },
                start_parameter: 0.0,
                end_parameter: TAU,
            })])
            .unwrap(),
            ([1.0 - extent, -2.0 - extent], [1.0 + extent, -2.0 + extent]),
        );
    }

    #[test]
    fn signed_bulges_expand_bounds_on_the_correct_side() {
        let polyline = |bulge| {
            Curve::Polyline(Polyline {
                vertices: vec![
                    PolylineVertex::curved([-1.0, 0.0], bulge),
                    PolylineVertex::straight([1.0, 0.0]),
                ],
                closed: false,
            })
        };
        close(
            analytic_curve_bounds(&[polyline(1.0)]).unwrap(),
            ([-1.0, -1.0], [1.0, 0.0]),
        );
        close(
            analytic_curve_bounds(&[polyline(-1.0)]).unwrap(),
            ([-1.0, 0.0], [1.0, 1.0]),
        );
    }

    #[test]
    fn collections_combine_and_invalid_or_unbounded_curves_fail() {
        close(
            analytic_curve_bounds(&[
                Curve::Line(Line {
                    start: [-2.0, 1.0],
                    end: [3.0, 4.0],
                }),
                Curve::Circle(Circle {
                    centre: [8.0, 0.0],
                    radius: 2.0,
                }),
            ])
            .unwrap(),
            ([-2.0, -2.0], [10.0, 4.0]),
        );
        assert!(analytic_curve_bounds(&[]).is_none());
        assert!(analytic_curve_bounds(&[Curve::XLine(XLine {
            base: [0.0, 0.0],
            direction: [1.0, 0.0],
        })])
        .is_none());
        assert!(analytic_curve_bounds(&[Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 0.0,
        })])
        .is_none());
    }
}
