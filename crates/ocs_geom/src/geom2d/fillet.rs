//! Rounding the corner where two lines meet.
//!
//! A fillet replaces a corner with an arc of a given radius tangent to both
//! sides. The geometry is small but has two things that are easy to get wrong:
//! which of the four corners around an intersection is being rounded, and
//! which way the arc sweeps once it is placed.
//!
//! Both are settled by the *keep directions* — unit vectors pointing from the
//! corner along the parts of each line that survive. They carry the choice of
//! corner, so this module never has to guess it, and the caller resolves them
//! from whatever it has: a pick point, a segment's own extent, a rule.
//!
//! # Curves that do not meet at a corner
//!
//! A line and a circle usually do not cross at all, and two circles need not
//! either, yet both pairs still have arcs tangent to them. There is no apex
//! to start from, so [`fillets_between`] takes the other route: a circle of
//! the right radius tangent to a curve has its centre on one of that curve's
//! offsets, so every fillet is where an offset of one crosses an offset of
//! the other. That covers the corner case too, and the two agree where they
//! overlap.

use super::angle::normalize_angle;
use super::cross::intersect;
use super::curve::{Circle, Curve, XLine};
use super::vec::Vec2;
use super::Tolerance;

/// The arc that rounds a corner, and where it meets the two sides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fillet {
    /// Where the arc meets the first line.
    pub tangent1: [f64; 2],
    /// Where it meets the second.
    pub tangent2: [f64; 2],
    /// Centre of the arc.
    pub centre: [f64; 2],
    /// Start angle, normalised to `0..TAU`, sweeping counter-clockwise to
    /// [`end_angle`](Self::end_angle).
    pub start_angle: f64,
    /// End angle, normalised the same way.
    pub end_angle: f64,
}

/// The fillet of `radius` at the corner where two rays leave `apex`.
///
/// `direction1` and `direction2` must be unit vectors pointing along the parts
/// that survive; they select which of the four corners around the intersection
/// gets rounded.
///
/// `None` when there is no corner to round: the directions are parallel or
/// exactly opposed, so no arc of finite radius is tangent to both.
///
/// A radius of zero is a corner, not a fillet, and is the caller's business —
/// it wants the apex itself, and there is no arc to report.
pub fn fillet_between_rays(
    apex: [f64; 2],
    direction1: [f64; 2],
    direction2: [f64; 2],
    radius: f64,
) -> Option<Fillet> {
    let (apex, direction1, direction2) = (
        Vec2::from(apex),
        Vec2::from(direction1),
        Vec2::from(direction2),
    );
    let corner = direction1.dot(direction2).clamp(-1.0, 1.0).acos();
    if corner < 1e-6 || (corner - std::f64::consts::PI).abs() < 1e-6 {
        return None;
    }
    if radius < 1e-9 {
        return None;
    }

    let half = corner / 2.0;
    // Along each ray, the tangent point sits r / tan(half) from the apex, and
    // the centre sits r / sin(half) along the bisector.
    let along = radius / half.tan();
    let tangent1 = apex + direction1 * along;
    let tangent2 = apex + direction2 * along;

    // Opposed directions have no bisector to place the centre on, which the
    // parallel check above already refused; this is the belt to its braces.
    let bisector = (direction1 + direction2).normalize()?;
    let centre = apex + bisector * (radius / half.sin());

    let angle1 = (tangent1 - centre).angle();
    let angle2 = (tangent2 - centre).angle();

    // Arcs are stored sweeping counter-clockwise, so the endpoints go in the
    // order that fills the corner rather than the one that goes the long way
    // round the outside. Which order that is follows the turn from the first
    // direction to the second.
    let turn = direction1.cross(direction2);
    let (start_angle, end_angle) = if turn <= 0.0 {
        (angle1, angle2)
    } else {
        (angle2, angle1)
    };

    Some(Fillet {
        tangent1: tangent1.to_array(),
        tangent2: tangent2.to_array(),
        centre: centre.to_array(),
        start_angle: normalize_angle(start_angle),
        end_angle: normalize_angle(end_angle),
    })
}

/// Every arc of `radius` tangent to both curves.
///
/// The general form of [`fillet_between_rays`], which only knows about two
/// rays leaving a shared corner. Two curves need not share one — a line and a
/// circle usually do not meet at all — so the corner cannot be the starting
/// point.
///
/// # The construction
///
/// A circle of `radius` tangent to a curve has its centre `radius` away from
/// it, which is to say on one of the curve's two offsets. So a circle tangent
/// to *both* has its centre where an offset of one crosses an offset of the
/// other, and every fillet is one of those crossings. Each side has two
/// offsets and so there are up to four combinations, each of which may cross
/// more than once; all of them come back, and choosing between them is the
/// caller's — it is the one holding the pick points that say which corner was
/// meant.
///
/// Tangent points then follow from the centre: the nearest point of each
/// original curve to it.
///
/// Empty when neither offset pair crosses, and for curves whose offset is not
/// itself a line or a circle — a spline, an ellipse. Those need the general
/// offset machinery rather than this closed form.
pub fn fillets_between(
    a: &Curve,
    b: &Curve,
    radius: f64,
    tolerance: Tolerance,
) -> Vec<Fillet> {
    if radius.is_nan() || radius <= 0.0 || !radius.is_finite() {
        return Vec::new();
    }
    // Survey coordinates: solve near the origin and move the fillets back.
    // The converged path below cannot reach its tolerance at 1e6 otherwise.
    let anchors = [a.point_at(0.0), b.point_at(0.0)].map(|p| [p[0], p[1], 0.0]);
    let origin = super::Frame::around(anchors.iter()).origin();
    if origin[0].abs().max(origin[1].abs()) >= 1e3 {
        let shift = super::Transform::translation([-origin[0], -origin[1]]);
        if let (Some(local_a), Some(local_b)) = (a.transformed(&shift), b.transformed(&shift)) {
            let back = |p: [f64; 2]| [p[0] + origin[0], p[1] + origin[1]];
            return fillets_between(&local_a, &local_b, radius, tolerance)
                .into_iter()
                .map(|f| Fillet {
                    tangent1: back(f.tangent1),
                    tangent2: back(f.tangent2),
                    centre: back(f.centre),
                    ..f
                })
                .collect();
        }
    }
    // A polyline fillets segment by segment, each a line or an arc with a
    // closed form; taken whole, its corners would seed false centres.
    if matches!(a, Curve::Polyline(_)) || matches!(b, Curve::Polyline(_)) {
        let pieces = |c: &Curve| match c {
            Curve::Polyline(_) => c.segments(),
            other => vec![other.clone()],
        };
        let mut out: Vec<Fillet> = Vec::new();
        for first in pieces(a) {
            for second in pieces(b) {
                for fillet in fillets_between(&first, &second, radius, tolerance) {
                    let centre = Vec2::from(fillet.centre);
                    let near = |kept: &Fillet| Vec2::from(kept.centre).distance(centre);
                    if !out.iter().any(|kept| near(kept) <= tolerance.linear()) {
                        out.push(fillet);
                    }
                }
            }
        }
        return out;
    }
    let (Some(a_offsets), Some(b_offsets)) = (offsets(a, radius), offsets(b, radius)) else {
        return general_fillets(a, b, radius, tolerance);
    };
    let mut out: Vec<Fillet> = Vec::new();
    for first in &a_offsets {
        for second in &b_offsets {
            for crossing in intersect(first, second, tolerance) {
                let centre = Vec2::from(crossing.point);
                let Some(tangent1) = touch_point(a, centre, radius, tolerance) else {
                    continue;
                };
                let Some(tangent2) = touch_point(b, centre, radius, tolerance) else {
                    continue;
                };
                let fillet = arc_through(centre, tangent1, tangent2);
                // The four offset combinations meet at the same place where
                // the two curves are tangent to each other, so the same
                // centre can arrive more than once.
                if !out.iter().any(|kept| {
                    Vec2::from(kept.centre).distance(centre) <= tolerance.linear()
                }) {
                    out.push(fillet);
                }
            }
        }
    }
    out
}

/// How far along a curve a fillet may land, as a parameterised path: the
/// straight kinds run on as their infinite line and the round ones as their
/// whole circle or ellipse — a fillet extends them as well as trims — while
/// a spline or polyline keeps to the part that is drawn.
struct Extended {
    curve: Curve,
    from: f64,
    to: f64,
}

impl Extended {
    fn of(curve: &Curve, reach: f64) -> Option<Self> {
        use super::curve::EllipseArc;
        Some(match curve {
            Curve::Line(_) | Curve::Ray(_) | Curve::XLine(_) => {
                let (origin, along) = curve.as_ray()?;
                let unit = Vec2::from(along).normalize()?;
                // Unit speed, centred on the drawn origin, out to `reach`.
                let line = Curve::Line(super::curve::Line {
                    start: (Vec2::from(origin) - unit * reach).to_array(),
                    end: (Vec2::from(origin) + unit * reach).to_array(),
                });
                Self { curve: line, from: 0.0, to: 1.0 }
            }
            Curve::Arc(arc) => Self {
                curve: Curve::Circle(Circle { centre: arc.centre, radius: arc.radius }),
                from: 0.0,
                to: 1.0,
            },
            Curve::Ellipse(arc) => Self {
                curve: Curve::Ellipse(EllipseArc::full(arc.ellipse)),
                from: 0.0,
                to: 1.0,
            },
            other => Self { curve: other.clone(), from: 0.0, to: 1.0 },
        })
    }

    fn point(&self, t: f64) -> Vec2 {
        Vec2::from(self.curve.point_at(t))
    }

    /// Unit left normal at `t`, by a central difference kept inside the span.
    fn normal(&self, t: f64) -> Option<Vec2> {
        let h = 1e-6 * (self.to - self.from);
        let (lo, hi) = ((t - h).max(self.from), (t + h).min(self.to));
        (self.point(hi) - self.point(lo)).normalize().map(|d| d.perpendicular())
    }

    /// The offset point at `t` on `side` (+1 left, -1 right).
    fn offset(&self, t: f64, side: f64, radius: f64) -> Option<Vec2> {
        Some(self.point(t) + self.normal(t)? * (radius * side))
    }
}

/// Fillets between curves whose offsets have no closed form — an ellipse, a
/// spline, a polyline — and anything they are paired with.
///
/// A fillet centre lies on an offset of each curve, so it is where the two
/// offset paths cross: sampled to find each crossing, then converged by
/// Newton on `a(t) + r·n_a(t) = b(u) + r·n_b(u)`. The tangent points are the
/// curve points the converged parameters name, so they lie on the curves to
/// the solver's precision rather than to the sampling's.
fn general_fillets(a: &Curve, b: &Curve, radius: f64, tolerance: Tolerance) -> Vec<Fillet> {
    const SAMPLES: usize = 256;
    let size = |c: &Curve| {
        let pts = c.tessellate(4.0);
        pts.iter().fold(0.0_f64, |m, p| m.max(Vec2::from(*p).distance(Vec2::from(pts[0]))))
    };
    let reach = 4.0 * (size(a) + size(b) + radius);
    let (Some(ea), Some(eb)) = (Extended::of(a, reach), Extended::of(b, reach)) else {
        return Vec::new();
    };
    let params = |e: &Extended| -> Vec<f64> {
        (0..=SAMPLES).map(|i| e.from + (e.to - e.from) * i as f64 / SAMPLES as f64).collect()
    };
    let (ta, tb) = (params(&ea), params(&eb));
    let mut out: Vec<Fillet> = Vec::new();
    for side_a in [1.0, -1.0] {
        for side_b in [1.0, -1.0] {
            let pa: Vec<Option<Vec2>> = ta.iter().map(|t| ea.offset(*t, side_a, radius)).collect();
            let pb: Vec<Option<Vec2>> = tb.iter().map(|t| eb.offset(*t, side_b, radius)).collect();
            for i in 0..SAMPLES {
                let (Some(a0), Some(a1)) = (pa[i], pa[i + 1]) else { continue };
                for j in 0..SAMPLES {
                    let (Some(b0), Some(b1)) = (pb[j], pb[j + 1]) else { continue };
                    let Some((s, w)) = super::intersect::line_line(
                        a0.to_array(),
                        (a1 - a0).to_array(),
                        b0.to_array(),
                        (b1 - b0).to_array(),
                    ) else {
                        continue;
                    };
                    if !(0.0..=1.0).contains(&s) || !(0.0..=1.0).contains(&w) {
                        continue;
                    }
                    let seed = (ta[i] + (ta[i + 1] - ta[i]) * s, tb[j] + (tb[j + 1] - tb[j]) * w);
                    let converged = converge(&ea, &eb, side_a, side_b, radius, seed, tolerance);
                    let Some((t, u)) = converged else { continue };
                    let (Some(centre), Some(tangent1), tangent2) =
                        (ea.offset(t, side_a, radius), Some(ea.point(t)), eb.point(u))
                    else {
                        continue;
                    };
                    if (centre.distance(tangent2) - radius).abs() > tolerance.linear() {
                        continue;
                    }
                    let near = |kept: &Fillet| Vec2::from(kept.centre).distance(centre);
                    if !out.iter().any(|kept| near(kept) <= tolerance.linear()) {
                        out.push(arc_through(centre, tangent1, tangent2));
                    }
                }
            }
        }
    }
    out
}

/// Newton on the offset crossing, from a sampled seed.
fn converge(
    a: &Extended,
    b: &Extended,
    side_a: f64,
    side_b: f64,
    radius: f64,
    seed: (f64, f64),
    tolerance: Tolerance,
) -> Option<(f64, f64)> {
    let residual = |t: f64, u: f64| -> Option<Vec2> {
        Some(a.offset(t, side_a, radius)? - b.offset(u, side_b, radius)?)
    };
    let (mut t, mut u) = seed;
    let (ha, hb) = (1e-7 * (a.to - a.from), 1e-7 * (b.to - b.from));
    for _ in 0..40 {
        let f = residual(t, u)?;
        if f.length() <= tolerance.linear() * 1e-3 {
            return Some((t, u));
        }
        let dt = (residual(t + ha, u)? - residual(t - ha, u)?) * (0.5 / ha);
        let du = (residual(t, u + hb)? - residual(t, u - hb)?) * (0.5 / hb);
        let det = dt.x * du.y - dt.y * du.x;
        if det.abs() < 1e-300 {
            return None;
        }
        t -= (f.x * du.y - f.y * du.x) / det;
        u -= (dt.x * f.y - dt.y * f.x) / det;
        t = t.clamp(a.from, a.to);
        u = u.clamp(b.from, b.to);
    }
    let f = residual(t, u)?;
    (f.length() <= tolerance.linear()).then_some((t, u))
}

/// The two curves parallel to `curve` at `radius`, or `None` where that is
/// not itself a line or a circle.
///
/// A line's offsets are the two lines beside it; a circle's are the two
/// concentric circles, and the inner one only exists when the radius leaves
/// something of it. The straight kinds are offset as infinite lines rather
/// than as segments, because a fillet's tangent point routinely falls past
/// the drawn end — which is what makes a fillet extend as well as trim.
fn offsets(curve: &Curve, radius: f64) -> Option<Vec<Curve>> {
    match curve {
        Curve::Line(_) | Curve::Ray(_) | Curve::XLine(_) => {
            let (origin, along) = curve.as_ray()?;
            let across = Vec2::from(along).normalize()?.perpendicular();
            Some(
                [1.0, -1.0]
                    .into_iter()
                    .map(|side| {
                        Curve::XLine(XLine {
                            base: (Vec2::from(origin) + across * (radius * side)).to_array(),
                            direction: along,
                        })
                    })
                    .collect(),
            )
        }
        Curve::Circle(circle) => Some(concentric(circle.centre, circle.radius, radius)),
        Curve::Arc(arc) => Some(concentric(arc.centre, arc.radius, radius)),
        _ => None,
    }
}

fn concentric(centre: [f64; 2], own: f64, radius: f64) -> Vec<Curve> {
    [own + radius, (own - radius).abs()]
        .into_iter()
        .filter(|r| *r > 1e-12)
        .map(|r| Curve::Circle(Circle { centre, radius: r }))
        .collect()
}

/// Where a circle centred at `centre` touches `curve`.
///
/// The nearest point of the curve's own infinite extent, which is where a
/// tangent circle meets it. `None` if that turns out not to be `radius` away
/// after all, which weeds out a crossing of the wrong pair of offsets.
fn touch_point(curve: &Curve, centre: Vec2, radius: f64, tolerance: Tolerance) -> Option<Vec2> {
    let touch = match curve {
        Curve::Line(_) | Curve::Ray(_) | Curve::XLine(_) => {
            let (origin, along) = curve.as_ray()?;
            let (origin, along) = (Vec2::from(origin), Vec2::from(along));
            let squared = along.length_squared();
            if squared <= 0.0 {
                return None;
            }
            origin + along * ((centre - origin).dot(along) / squared)
        }
        Curve::Circle(circle) => circle_touch(
            Vec2::from(circle.centre),
            circle.radius,
            centre,
            radius,
        )?,
        Curve::Arc(arc) => circle_touch(
            Vec2::from(arc.centre),
            arc.radius,
            centre,
            radius,
        )?,
        _ => return None,
    };
    let off_by = (touch.distance(centre) - radius).abs();
    (off_by <= tolerance.linear()).then_some(touch)
}

fn circle_touch(middle: Vec2, own: f64, centre: Vec2, radius: f64) -> Option<Vec2> {
    let out = (centre - middle).normalize()?;
    [middle + out * own, middle - out * own]
        .into_iter()
        .min_by(|a, b| {
            (a.distance(centre) - radius)
                .abs()
                .total_cmp(&(b.distance(centre) - radius).abs())
        })
}

/// The arc from one tangent point to the other about `centre`, taking the
/// shorter of the two ways round.
///
/// A fillet is the short way by definition: it rounds a corner rather than
/// travelling the long way about it. Which of the two arcs between the same
/// endpoints that is depends on the turn, so it is read off rather than
/// assumed.
fn arc_through(centre: Vec2, tangent1: Vec2, tangent2: Vec2) -> Fillet {
    let angle1 = (tangent1 - centre).angle();
    let angle2 = (tangent2 - centre).angle();
    let forward = super::angle::arc_span(angle1, angle2);
    let (start_angle, end_angle) = if forward <= std::f64::consts::PI {
        (angle1, angle2)
    } else {
        (angle2, angle1)
    };
    Fillet {
        tangent1: tangent1.to_array(),
        tangent2: tangent2.to_array(),
        centre: centre.to_array(),
        start_angle: normalize_angle(start_angle),
        end_angle: normalize_angle(end_angle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::angle::arc_span;
    use super::super::curve::Line;
    use std::f64::consts::{FRAC_PI_2, PI};

    /// A right-angle corner at the origin, opening into the first quadrant.
    fn right_angle(radius: f64) -> Fillet {
        fillet_between_rays([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], radius).unwrap()
    }

    fn tol() -> Tolerance {
        Tolerance::new(1e-9)
    }

    /// Every fillet the general form finds, checked for the property that
    /// defines one: the centre is exactly `radius` from both curves, and each
    /// tangent point is on its own curve and on the fillet arc.
    fn check_tangency(a: &Curve, b: &Curve, radius: f64) -> Vec<Fillet> {
        let found = fillets_between(a, b, radius, tol());
        for fillet in &found {
            let centre = Vec2::from(fillet.centre);
            for (curve, tangent) in [(a, fillet.tangent1), (b, fillet.tangent2)] {
                let tangent = Vec2::from(tangent);
                assert!(
                    (centre.distance(tangent) - radius).abs() < 1e-6,
                    "tangent point {tangent:?} is not {radius} from {centre:?}"
                );
                // On the curve's own infinite extent: a fillet's touch may
                // fall past a drawn end, which is what lets it extend.
                let on_curve = match curve {
                    Curve::Circle(circle) => {
                        (tangent.distance(Vec2::from(circle.centre)) - circle.radius).abs()
                    }
                    Curve::Arc(arc) => {
                        (tangent.distance(Vec2::from(arc.centre)) - arc.radius).abs()
                    }
                    _ => {
                        let (origin, along) = curve.as_ray().expect("straight");
                        let along = Vec2::from(along).normalize().unwrap();
                        (tangent - Vec2::from(origin)).cross(along).abs()
                    }
                };
                assert!(on_curve < 1e-6, "{tangent:?} is not on its curve");
            }
        }
        found
    }

    #[test]
    fn a_line_and_a_circle_have_four_fillets_of_a_given_radius() {
        // A circle of 10 at the origin and a line 20 below it. Rolling a
        // circle of radius 5 between them can sit inside or outside the
        // circle, and above or below the line — four places in all, though
        // only some exist for any given geometry.
        let circle = Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 10.0,
        });
        let line = Curve::Line(Line {
            start: [-100.0, -20.0],
            end: [100.0, -20.0],
        });
        let found = check_tangency(&circle, &line, 5.0);
        assert!(!found.is_empty(), "expected at least one fillet");
        for fillet in &found {
            // Every centre sits five above or five below the line.
            assert!(
                (fillet.centre[1] + 15.0).abs() < 1e-6 || (fillet.centre[1] + 25.0).abs() < 1e-6,
                "{:?}",
                fillet.centre
            );
        }
    }

    #[test]
    fn a_line_tangent_to_a_circle_still_yields_a_fillet() {
        // The line just touches the circle. The degenerate case a
        // discriminant test gets wrong by a sign.
        let circle = Curve::Circle(Circle {
            centre: [0.0, 0.0],
            radius: 10.0,
        });
        let line = Curve::Line(Line {
            start: [-50.0, -10.0],
            end: [50.0, -10.0],
        });
        assert!(!check_tangency(&circle, &line, 3.0).is_empty());
    }

    #[test]
    fn two_circles_give_fillets_touching_both() {
        let left = Curve::Circle(Circle {
            centre: [-20.0, 0.0],
            radius: 10.0,
        });
        let right = Curve::Circle(Circle {
            centre: [20.0, 0.0],
            radius: 10.0,
        });
        // A radius large enough to bridge the gap between them.
        let found = check_tangency(&left, &right, 30.0);
        assert!(!found.is_empty(), "expected a bridging fillet");
    }

    #[test]
    fn a_larger_circle_can_enclose_two_tangent_circles() {
        let left = Curve::Circle(Circle {
            centre: [-3.0, 0.0],
            radius: 1.0,
        });
        let right = Curve::Circle(Circle {
            centre: [3.0, 0.0],
            radius: 1.0,
        });
        let found = check_tangency(&left, &right, 5.0);
        let y = 7.0f64.sqrt();
        assert!(found.iter().any(|fillet| {
            (fillet.centre[0].abs() < 1e-9)
                && ((fillet.centre[1] - y).abs() < 1e-9
                    || (fillet.centre[1] + y).abs() < 1e-9)
        }));
    }

    #[test]
    fn a_larger_circle_can_enclose_a_circle_beside_a_line() {
        let line = Curve::Line(Line {
            start: [-10.0, 0.0],
            end: [10.0, 0.0],
        });
        let circle = Curve::Circle(Circle {
            centre: [0.0, 2.0],
            radius: 1.0,
        });
        let found = check_tangency(&line, &circle, 5.0);
        let x = 7.0f64.sqrt();
        assert!(found.iter().any(|fillet| {
            (fillet.centre[1] - 5.0).abs() < 1e-9
                && ((fillet.centre[0] - x).abs() < 1e-9
                    || (fillet.centre[0] + x).abs() < 1e-9)
        }));
    }

    #[test]
    fn two_lines_agree_with_the_corner_form() {
        // The general construction has to land where the closed form does,
        // or the two would disagree about the same corner.
        let a = Curve::Line(Line {
            start: [-10.0, 0.0],
            end: [10.0, 0.0],
        });
        let b = Curve::Line(Line {
            start: [0.0, -10.0],
            end: [0.0, 10.0],
        });
        let corner = right_angle(2.0);
        let found = check_tangency(&a, &b, 2.0);
        // Four corners around the crossing, and the closed form's is one.
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(found.iter().any(|fillet| {
            Vec2::from(fillet.centre).distance(Vec2::from(corner.centre)) < 1e-9
        }));
    }

    #[test]
    fn circles_too_far_apart_have_no_fillet_of_that_radius() {
        let left = Curve::Circle(Circle {
            centre: [-1000.0, 0.0],
            radius: 10.0,
        });
        let right = Curve::Circle(Circle {
            centre: [1000.0, 0.0],
            radius: 10.0,
        });
        assert!(fillets_between(&left, &right, 1.0, tol()).is_empty());
    }

    #[test]
    fn a_curve_with_no_closed_form_offset_declines() {
        let ellipse = Curve::Ellipse(super::super::EllipseArc {
            ellipse: super::super::Ellipse {
                centre: [0.0, 0.0],
                major_radius: 10.0,
                minor_radius: 5.0,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 0.0,
            end_parameter: std::f64::consts::TAU,
        });
        let line = Curve::Line(Line {
            start: [-50.0, -20.0],
            end: [50.0, -20.0],
        });
        assert!(fillets_between(&ellipse, &line, 2.0, tol()).is_empty());
    }

    #[test]
    fn a_nonsense_radius_is_refused_rather_than_producing_a_point() {
        let a = Curve::Line(Line {
            start: [-10.0, 0.0],
            end: [10.0, 0.0],
        });
        let b = Curve::Line(Line {
            start: [0.0, -10.0],
            end: [0.0, 10.0],
        });
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(fillets_between(&a, &b, radius, tol()).is_empty(), "{radius}");
        }
    }

    #[test]
    fn a_fillet_takes_the_short_way_round() {
        let a = Curve::Line(Line {
            start: [-10.0, 0.0],
            end: [10.0, 0.0],
        });
        let b = Curve::Line(Line {
            start: [0.0, -10.0],
            end: [0.0, 10.0],
        });
        for fillet in fillets_between(&a, &b, 2.0, tol()) {
            let sweep = arc_span(fillet.start_angle, fillet.end_angle);
            assert!(sweep <= PI + 1e-9, "swept {sweep} the long way");
        }
    }

    #[test]
    fn the_tangent_points_sit_on_their_rays() {
        let fillet = right_angle(1.0);
        // Along +X and +Y respectively, at r / tan(45°) = r.
        assert!((fillet.tangent1[0] - 1.0).abs() < 1e-12 && fillet.tangent1[1].abs() < 1e-12);
        assert!(fillet.tangent2[0].abs() < 1e-12 && (fillet.tangent2[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_centre_is_the_radius_from_both_tangent_points() {
        let fillet = fillet_between_rays([3.0, -2.0], [1.0, 0.0], [0.0, 1.0], 2.5).unwrap();
        for tangent in [fillet.tangent1, fillet.tangent2] {
            let dx = tangent[0] - fillet.centre[0];
            let dy = tangent[1] - fillet.centre[1];
            assert!(((dx * dx + dy * dy).sqrt() - 2.5).abs() < 1e-9);
        }
    }

    #[test]
    fn the_arc_is_tangent_to_both_rays() {
        // Tangency means the centre-to-tangent vector is perpendicular to the
        // ray it touches.
        let d1 = [1.0, 0.0];
        let d2 = [0.6, 0.8];
        let fillet = fillet_between_rays([0.0, 0.0], d1, d2, 1.5).unwrap();
        for (tangent, direction) in [(fillet.tangent1, d1), (fillet.tangent2, d2)] {
            let radial = [tangent[0] - fillet.centre[0], tangent[1] - fillet.centre[1]];
            let dot = radial[0] * direction[0] + radial[1] * direction[1];
            assert!(dot.abs() < 1e-9, "not perpendicular, dot = {dot}");
        }
    }

    #[test]
    fn the_arc_fills_the_corner_rather_than_going_the_long_way() {
        // A right angle should be rounded by a quarter turn, not three.
        let fillet = right_angle(1.0);
        let span = arc_span(fillet.start_angle, fillet.end_angle);
        assert!(
            (span - FRAC_PI_2).abs() < 1e-9,
            "expected a quarter turn, swept {span}"
        );
    }

    #[test]
    fn the_sweep_stays_short_whichever_way_the_corner_turns() {
        // Same corner, rays given in the other order: still a quarter turn.
        let fillet = fillet_between_rays([0.0, 0.0], [0.0, 1.0], [1.0, 0.0], 1.0).unwrap();
        let span = arc_span(fillet.start_angle, fillet.end_angle);
        assert!((span - FRAC_PI_2).abs() < 1e-9, "swept {span}");
    }

    #[test]
    fn a_sharper_corner_needs_a_longer_sweep() {
        // 60° between the rays leaves 120° of arc to fill it.
        let d2 = [0.5, 3f64.sqrt() / 2.0];
        let fillet = fillet_between_rays([0.0, 0.0], [1.0, 0.0], d2, 1.0).unwrap();
        let span = arc_span(fillet.start_angle, fillet.end_angle);
        assert!((span - 2.0 * PI / 3.0).abs() < 1e-9, "swept {span}");
    }

    #[test]
    fn a_bigger_radius_pushes_the_tangent_points_further_out() {
        let small = right_angle(1.0);
        let large = right_angle(4.0);
        assert!(large.tangent1[0] > small.tangent1[0]);
        assert!(large.centre[1] > small.centre[1]);
    }

    #[test]
    fn parallel_and_opposed_rays_have_no_fillet() {
        assert!(fillet_between_rays([0.0, 0.0], [1.0, 0.0], [1.0, 0.0], 1.0).is_none());
        assert!(fillet_between_rays([0.0, 0.0], [1.0, 0.0], [-1.0, 0.0], 1.0).is_none());
    }

    #[test]
    fn a_zero_radius_is_a_corner_not_a_fillet() {
        assert!(fillet_between_rays([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], 0.0).is_none());
    }

    #[test]
    fn survey_coordinates_keep_the_radius() {
        let apex = [512_345.678, 4_512_345.678];
        let fillet = fillet_between_rays(apex, [1.0, 0.0], [0.0, 1.0], 0.75).unwrap();
        let dx = fillet.tangent1[0] - fillet.centre[0];
        let dy = fillet.tangent1[1] - fillet.centre[1];
        assert!(((dx * dx + dy * dy).sqrt() - 0.75).abs() < 1e-9);
    }
}
