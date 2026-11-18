//! How much a curve encloses.
//!
//! Not the area of the polygon a curve's tessellation makes — the area of the
//! curve itself. A circle of radius one encloses π, not the 3.1358 that a
//! twenty-sided approximation of it does, and a polyline with a bulge in it
//! encloses the arc's own bite rather than its chord's.
//!
//! # Green's theorem is what makes this composable
//!
//! The enclosed area of a closed curve is `½∮(x dy − y dx)`, an integral
//! *along* the boundary rather than over the region inside it. That is the
//! whole reason this can be written per segment and summed: a polyline's area
//! is its segments' contributions added up, and adding a bulge to one of them
//! changes only that one's term.
//!
//! The same integral over an open curve is not an area — nothing is enclosed
//! — but it is still the piece a chain contributes, so it is what
//! [`Curve::enclosed_area`] returns for one. Sum the pieces of a closed chain
//! and the answer is the area; that is the contract the polyline case relies
//! on internally, and a caller assembling a boundary out of separate entities
//! needs the same thing.
//!
//! # Sign
//!
//! Positive counter-clockwise, matching
//! [`signed_area`](super::arrangement::signed_area) for rings of points. A
//! caller that wants a magnitude takes the absolute value; a caller deciding
//! whether a loop is a hole wants the sign.

use super::arrangement::{bounded_faces, signed_area};
use super::curve::{Curve, Line};
use super::Tolerance;
use super::polyline::{BulgeArc, Polyline};
use super::transform::Transform;
use super::vec::Vec2;
use std::f64::consts::TAU;

/// Gauss–Legendre nodes and weights, five points on `[-1, 1]`.
const NODES: [(f64, f64); 5] = [
    (-0.906_179_845_938_664, 0.236_926_885_056_189),
    (-0.538_469_310_105_683, 0.478_628_670_499_366),
    (0.0, 0.568_888_888_888_889),
    (0.538_469_310_105_683, 0.478_628_670_499_366),
    (0.906_179_845_938_664, 0.236_926_885_056_189),
];

/// Panels the integrated cases are split into. The integrand here is a
/// polynomial in the trigonometric terms rather than a square root, so it
/// converges faster than the length integral and needs fewer.
const PANELS: usize = 16;

impl Curve {
    /// The signed area the curve encloses, or contributes to a chain.
    ///
    /// `½∮(x dy − y dx)` along the curve. For a closed curve that is the area
    /// inside it, positive counter-clockwise. For an open one it is what the
    /// curve contributes to any closed chain it is part of, so the pieces of
    /// a boundary can be measured separately and added.
    ///
    /// Zero for a ray and a construction line: an unbounded curve encloses
    /// nothing, and reporting an infinity would poison a sum.
    pub fn enclosed_area(&self) -> f64 {
        match self {
            // ½ (p × q), the triangle the chord makes with the origin.
            Self::Line(line) => 0.5 * Vec2::from(line.start).cross(Vec2::from(line.end)),
            Self::Ray(_) | Self::XLine(_) => 0.0,
            Self::Circle(circle) => circular_area(circle.centre, circle.radius, 0.0, TAU),
            Self::Arc(arc) => {
                let start = super::angle::normalize_angle(arc.start_angle);
                circular_area(arc.centre, arc.radius, start, start + arc.sweep())
            }
            Self::Ellipse(_) | Self::Nurbs(_) => self.integrated_area(),
            Self::Polyline(polyline) => polyline_area(polyline),
        }
    }

    /// This curve's share of a closed chain's area: [`Self::enclosed_area`],
    /// except that an open polyline is not closed by its own chord. A face's
    /// boundary pieces, summed round each loop, give the area inside it.
    pub fn chain_area(&self) -> f64 {
        match self {
            Self::Polyline(polyline) if !polyline.closed && polyline.vertices.len() >= 2 => {
                let (first, last) = (
                    polyline.vertices[0].position,
                    polyline.vertices[polyline.vertices.len() - 1].position,
                );
                polyline_area(polyline) - 0.5 * Vec2::from(last).cross(Vec2::from(first))
            }
            _ => self.enclosed_area(),
        }
    }

    /// Area obtained by closing a bounded curve with the straight chord from
    /// its end point back to its start point.
    ///
    /// This is the useful area convention for an open arc, spline or
    /// polyline shown in an object-properties palette. It deliberately stays
    /// separate from [`Self::enclosed_area`], whose open-curve contribution is
    /// composable when several entities form a boundary.
    ///
    /// A curve that crosses itself encloses each place it covers once: a
    /// figure eight is its two loops added, not their signed sum of nearly
    /// zero, and a pentagram counts its centre once. The sign is the signed
    /// integral's.
    pub fn chord_closed_area(&self) -> Option<f64> {
        if matches!(self, Self::Ray(_) | Self::XLine(_)) {
            return None;
        }
        let start = self.point_at(0.0);
        let end = self.point_at(1.0);
        // `chain_area`: an open polyline's own integral already closes it,
        // and adding the chord on top would count the closing edge twice.
        let signed = self.chain_area() + line_moments(end, start).0;
        Some(match covered_area(self) {
            Some(covered) => covered.copysign(signed),
            None => signed,
        })
    }

    /// Centroid of the region closed by this curve.
    pub fn enclosed_centroid(&self) -> Option<[f64; 2]> {
        match self {
            Self::Polyline(polyline) if polyline.vertices.len() < 2 => return None,
            Self::Polyline(_) => {}
            _ if !self.is_closed() => return None,
            _ => {}
        }
        let origin = self.point_at(0.0);
        let local = self.transformed(&Transform::translation([-origin[0], -origin[1]]))?;
        let (area, first_x, first_y) = local.enclosed_moments();
        (area.is_finite()
            && area != 0.0
            && first_x.is_finite()
            && first_y.is_finite())
        .then_some([origin[0] + first_x / area, origin[1] + first_y / area])
    }

    /// Centroid of the region obtained by closing a bounded curve with its
    /// end-to-start chord.
    pub fn chord_closed_centroid(&self) -> Option<[f64; 2]> {
        if matches!(self, Self::Ray(_) | Self::XLine(_)) {
            return None;
        }
        let origin = self.point_at(0.0);
        let local = self.transformed(&Transform::translation([-origin[0], -origin[1]]))?;
        let start = local.point_at(0.0);
        let end = local.point_at(1.0);
        let (area, first_x, first_y) =
            add_moments(local.enclosed_moments(), line_moments(end, start));
        (area.is_finite()
            && area != 0.0
            && first_x.is_finite()
            && first_y.is_finite())
        .then_some([origin[0] + first_x / area, origin[1] + first_y / area])
    }

    fn enclosed_moments(&self) -> (f64, f64, f64) {
        match self {
            Self::Polyline(polyline) => polyline_moments(polyline),
            _ => self.moment_contribution(),
        }
    }

    fn moment_contribution(&self) -> (f64, f64, f64) {
        let (first_x, first_y) = self.integrated_first_moments();
        (self.enclosed_area(), first_x, first_y)
    }

    fn integrated_first_moments(&self) -> (f64, f64) {
        integrated_first_moments(|t| self.point_at(t), |t| self.tangent_at(t))
    }

    /// The area contribution by numeric integration, for the kinds with no
    /// closed form.
    fn integrated_area(&self) -> f64 {
        let width = 1.0 / PANELS as f64;
        (0..PANELS)
            .map(|panel| {
                let from = width * panel as f64;
                let half = 0.5 * width;
                let middle = from + half;
                half * NODES
                    .iter()
                    .map(|(node, weight)| {
                        let t = middle + half * node;
                        let point = Vec2::from(self.point_at(t));
                        let step = Vec2::from(self.tangent_at(t));
                        weight * point.cross(step)
                    })
                    .sum::<f64>()
            })
            .sum::<f64>()
            * 0.5
    }
}

fn add_moments(a: (f64, f64, f64), b: (f64, f64, f64)) -> (f64, f64, f64) {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

fn polyline_area(polyline: &Polyline) -> f64 {
    let vertices = polyline.vertices.len();
    let segments = if vertices < 2 {
        0
    } else if polyline.closed {
        vertices
    } else {
        vertices - 1
    };
    let mut area = 0.0;
    for index in 0..segments {
        let start = polyline.vertices[index].position;
        let end = polyline.vertices[(index + 1) % vertices].position;
        area += match polyline.segment_arc(index) {
            Some(arc) => circular_area(
                arc.center,
                arc.radius,
                arc.start_angle,
                arc.start_angle + arc.sweep,
            ),
            None => 0.5 * Vec2::from(start).cross(Vec2::from(end)),
        };
    }
    if !polyline.closed && vertices >= 2 {
        area += 0.5
            * Vec2::from(polyline.vertices[vertices - 1].position)
                .cross(Vec2::from(polyline.vertices[0].position));
    }
    area
}

fn polyline_moments(polyline: &Polyline) -> (f64, f64, f64) {
    let vertices = polyline.vertices.len();
    let segments = if vertices < 2 {
        0
    } else if polyline.closed {
        vertices
    } else {
        vertices - 1
    };
    let mut total = (0.0, 0.0, 0.0);
    for index in 0..segments {
        let start = polyline.vertices[index].position;
        let end = polyline.vertices[(index + 1) % vertices].position;
        let contribution = match polyline.segment_arc(index) {
            Some(arc) => bulge_moments(&arc),
            None => line_moments(start, end),
        };
        total = add_moments(total, contribution);
    }
    if !polyline.closed && vertices >= 2 {
        total = add_moments(
            total,
            line_moments(
                polyline.vertices[vertices - 1].position,
                polyline.vertices[0].position,
            ),
        );
    }
    total
}

fn bulge_moments(arc: &BulgeArc) -> (f64, f64, f64) {
    let first = integrated_first_moments(
        |t| arc.sample(t),
        |t| {
            let angle = arc.start_angle + arc.sweep * t;
            [
                -arc.radius * angle.sin() * arc.sweep,
                arc.radius * angle.cos() * arc.sweep,
            ]
        },
    );
    (
        circular_area(
            arc.center,
            arc.radius,
            arc.start_angle,
            arc.start_angle + arc.sweep,
        ),
        first.0,
        first.1,
    )
}

/// The area a self-crossing chord-closed curve covers, counting every place
/// it winds round once; `None` when it winds round nothing more than once,
/// so the exact integral already is that area.
///
/// Measured on the curve's tessellation split into the regions it bounds,
/// keeping those it winds round. Straight pieces are exact; curved ones are
/// sampled at two densities and extrapolated, since a chord's lost sliver
/// shrinks with the square of the step.
fn covered_area(curve: &Curve) -> Option<f64> {
    let coarse = covered_ring_area(curve, 64.0)?;
    let fine = covered_ring_area(curve, 128.0)?;
    Some(fine + (fine - coarse) / 3.0)
}

fn covered_ring_area(curve: &Curve, segments_per_radian: f64) -> Option<f64> {
    let mut ring = curve.tessellate(segments_per_radian);
    if ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    let segments: Vec<Line> = ring
        .windows(2)
        .filter(|pair| pair[0] != pair[1])
        .map(|pair| Line {
            start: pair[0],
            end: pair[1],
        })
        .collect();
    let extent = ring.iter().fold(0.0_f64, |size, point| {
        size.max((point[0] - ring[0][0]).abs())
            .max((point[1] - ring[0][1]).abs())
    });
    if segments.len() < 3 || !(extent > 0.0) || !extent.is_finite() {
        return None;
    }
    let faces = bounded_faces(&segments, Tolerance::new(extent * 1.0e-12));
    let covered: f64 = faces
        .iter()
        .filter(|face| winding_inside(face, &ring) != 0)
        .map(|face| signed_area(face).abs())
        .sum();
    // Wound round at most once everywhere: the integral is the area.
    ((covered - signed_area(&ring).abs()).abs() > covered * 1.0e-9).then_some(covered)
}

/// How many times `ring` winds round a point just inside `face` (a
/// counter-clockwise region of the arrangement the ring makes).
fn winding_inside(face: &[[f64; 2]], ring: &[[f64; 2]]) -> i32 {
    // Off the middle of the face's longest edge, a hair to its left.
    let Some((a, b)) = face
        .iter()
        .zip(face.iter().cycle().skip(1))
        .max_by(|(a, b), (c, d)| {
            Vec2::from(**a)
                .distance(Vec2::from(**b))
                .total_cmp(&Vec2::from(**c).distance(Vec2::from(**d)))
        })
    else {
        return 0;
    };
    let (a, b) = (Vec2::from(*a), Vec2::from(*b));
    let along = b - a;
    let left = Vec2::new(-along.y, along.x) * 1.0e-6;
    let point = (a + b) * 0.5 + left;
    let mut winding = 0;
    for pair in ring.windows(2) {
        let (p, q) = (Vec2::from(pair[0]), Vec2::from(pair[1]));
        let side = (q - p).cross(point - p);
        if p.y <= point.y && q.y > point.y && side > 0.0 {
            winding += 1;
        } else if q.y <= point.y && p.y > point.y && side < 0.0 {
            winding -= 1;
        }
    }
    winding
}

fn line_moments(start: [f64; 2], end: [f64; 2]) -> (f64, f64, f64) {
    let step = [end[0] - start[0], end[1] - start[1]];
    let first = integrated_first_moments(
        |t| [start[0] + step[0] * t, start[1] + step[1] * t],
        |_| step,
    );
    (
        0.5 * Vec2::from(start).cross(Vec2::from(end)),
        first.0,
        first.1,
    )
}

fn integrated_first_moments(
    point_at: impl Fn(f64) -> [f64; 2],
    tangent_at: impl Fn(f64) -> [f64; 2],
) -> (f64, f64) {
    let width = 1.0 / PANELS as f64;
    (0..PANELS)
        .flat_map(|panel| {
            let half = 0.5 * width;
            let middle = width * panel as f64 + half;
            let point_at = &point_at;
            let tangent_at = &tangent_at;
            NODES.iter().map(move |(node, weight)| {
                let t = middle + half * node;
                let point = point_at(t);
                let step = tangent_at(t);
                (
                    half * weight * 0.5 * point[0] * point[0] * step[1],
                    -half * weight * 0.5 * point[1] * point[1] * step[0],
                )
            })
        })
        .fold((0.0, 0.0), |(ax, ay), (bx, by)| (ax + bx, ay + by))
}

/// `½∮(x dy − y dx)` along a circular arc from `start` to `end` radians.
///
/// Worked out rather than integrated: with `x = cx + r·cos θ` and
/// `y = cy + r·sin θ`, the integrand is `r(cx·cos θ + cy·sin θ) + r²`, whose
/// antiderivative is elementary. The `r²(end − start)` term is the sector's
/// own area; the rest is the triangle the chord makes with the origin.
fn circular_area(centre: [f64; 2], radius: f64, start: f64, end: f64) -> f64 {
    let centre = Vec2::from(centre);
    let sweep = end - start;
    let along = radius * (centre.x * (end.sin() - start.sin()) - centre.y * (end.cos() - start.cos()));
    0.5 * (along + radius * radius * sweep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::polyline::{Polyline, PolylineVertex};
    use crate::geom2d::{Arc, Circle, Ellipse, EllipseArc, Line, NurbsCurve};
    use std::f64::consts::{FRAC_PI_2, PI};

    fn circle(centre: [f64; 2], radius: f64) -> Curve {
        Curve::Circle(Circle { centre, radius })
    }

    #[test]
    fn a_circle_encloses_pi_r_squared() {
        for centre in [[0.0, 0.0], [100.0, -250.0]] {
            for radius in [0.5, 7.0, 1_000.0] {
                let area = circle(centre, radius).enclosed_area();
                let expected = PI * radius * radius;
                assert!(
                    (area - expected).abs() < 1e-9 * expected,
                    "centre {centre:?} r {radius}: {area} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn a_circle_beats_the_polygon_that_approximates_it() {
        // The reason this exists rather than measuring a tessellation: at the
        // density a drawing renders at, the polygon is visibly short.
        let curve = circle([0.0, 0.0], 1.0);
        let sampled = crate::geom2d::signed_area(&curve.tessellate(20.0));
        assert!(sampled < PI - 1e-4, "the polygon should read short: {sampled}");
        assert!((curve.enclosed_area() - PI).abs() < 1e-12);
    }

    fn closed_ring(points: &[[f64; 2]]) -> Curve {
        Curve::Polyline(Polyline {
            vertices: points
                .iter()
                .map(|&position| PolylineVertex {
                    position,
                    bulge: 0.0,
                })
                .collect(),
            closed: true,
        })
    }

    #[test]
    fn a_self_crossing_ring_covers_each_place_once() {
        let measured = |points: &[[f64; 2]]| closed_ring(points).chord_closed_area().unwrap().abs();
        // Figure eight: two loops of opposite sense, signed sum zero.
        assert!((measured(&[[0.0, 0.0], [100.0, 100.0], [100.0, 0.0], [0.0, 100.0]]) - 5000.0).abs() < 1e-6);
        // A square traced twice still covers one square.
        let twice = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]].repeat(2);
        assert!((measured(&twice) - 10_000.0).abs() < 1e-6);
        // A loop that leaves a hole the curve does not wind round.
        let hole = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [40.0, 100.0], [40.0, -50.0], [70.0, -50.0], [70.0, 20.0], [0.0, 20.0]];
        assert!((measured(&hole) - 8300.0).abs() < 1e-6);
        // An open chain is closed by its chord once, not twice.
        let open = Curve::Polyline(Polyline {
            vertices: [[10.0, 10.0], [20.0, 10.0], [20.0, 30.0]]
                .into_iter()
                .map(|position| PolylineVertex { position, bulge: 0.0 })
                .collect(),
            closed: false,
        });
        assert!((open.chord_closed_area().unwrap().abs() - 100.0).abs() < 1e-9);
        // A simple square is untouched.
        assert!((measured(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_square_given_as_a_closed_polyline_measures_its_side_squared() {
        let square = Curve::Polyline(Polyline {
            vertices: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .into_iter()
                .map(|position| PolylineVertex {
                    position,
                    bulge: 0.0,
                })
                .collect(),
            closed: true,
        });
        assert!((square.enclosed_area() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn an_open_polyline_is_closed_by_its_own_chord() {
        // The same square with the last edge left off. What a caller means by
        // "the area of this" is the region, so the chain is closed.
        let three_sides = Curve::Polyline(Polyline {
            vertices: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .into_iter()
                .map(|position| PolylineVertex {
                    position,
                    bulge: 0.0,
                })
                .collect(),
            closed: false,
        });
        assert!((three_sides.enclosed_area() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn an_open_polyline_shares_a_chain_without_its_chord() {
        // Three sides of the square as one piece and the fourth as another:
        // their shares of the chain add up to the square.
        let vertex = |position| PolylineVertex { position, bulge: 0.0 };
        let three_sides = Curve::Polyline(Polyline {
            vertices: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .into_iter()
                .map(vertex)
                .collect(),
            closed: false,
        });
        let last = Curve::Line(Line { start: [0.0, 10.0], end: [0.0, 0.0] });
        assert!((three_sides.chain_area() + last.chain_area() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_bulge_adds_its_own_bite_rather_than_its_chord() {
        // A square whose top edge bulges out into a half circle of radius 5.
        let mut vertices: Vec<PolylineVertex> =
            [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .into_iter()
                .map(|position| PolylineVertex {
                    position,
                    bulge: 0.0,
                })
                .collect();
        vertices[2].bulge = 1.0; // from (10,10) to (0,10), bulging up
        let curve = Curve::Polyline(Polyline {
            vertices,
            closed: true,
        });
        let expected = 100.0 + PI * 25.0 / 2.0;
        assert!(
            (curve.enclosed_area() - expected).abs() < 1e-9,
            "{} vs {expected}",
            curve.enclosed_area()
        );
    }

    #[test]
    fn the_sign_says_which_way_the_loop_runs() {
        let clockwise = Curve::Polyline(Polyline {
            vertices: [[0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0]]
                .into_iter()
                .map(|position| PolylineVertex {
                    position,
                    bulge: 0.0,
                })
                .collect(),
            closed: true,
        });
        assert!(clockwise.enclosed_area() < 0.0);
        assert!((clockwise.enclosed_area() + 100.0).abs() < 1e-9);
    }

    #[test]
    fn an_ellipse_encloses_pi_a_b() {
        let ellipse = Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [3.0, -4.0],
                major_radius: 20.0,
                minor_radius: 5.0,
                major_axis: [0.6, 0.8],
            },
            start_parameter: 0.0,
            end_parameter: TAU,
        });
        let expected = PI * 20.0 * 5.0;
        assert!(
            (ellipse.enclosed_area() - expected).abs() < 1e-6 * expected,
            "{} vs {expected}",
            ellipse.enclosed_area()
        );
    }

    #[test]
    fn a_half_circle_arc_contributes_its_sector_plus_the_chord_triangle() {
        // Closing a half circle with its own diameter gives a half disc.
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 10.0,
            start_angle: 0.0,
            end_angle: PI,
        });
        let closing = Curve::Line(Line {
            start: [-10.0, 0.0],
            end: [10.0, 0.0],
        });
        let total = arc.enclosed_area() + closing.enclosed_area();
        let expected = PI * 100.0 / 2.0;
        assert!((total - expected).abs() < 1e-9, "{total} vs {expected}");
    }

    #[test]
    fn the_pieces_of_a_boundary_add_up_to_the_whole() {
        // The property the whole approach rests on: a boundary assembled out
        // of separate entities is measured by summing them.
        let quarter = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 4.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        });
        let up = Curve::Line(Line {
            start: [0.0, 4.0],
            end: [0.0, 0.0],
        });
        let out = Curve::Line(Line {
            start: [0.0, 0.0],
            end: [4.0, 0.0],
        });
        let total = quarter.enclosed_area() + up.enclosed_area() + out.enclosed_area();
        assert!((total - PI * 16.0 / 4.0).abs() < 1e-9, "{total}");
    }

    #[test]
    fn an_unbounded_curve_encloses_nothing() {
        let ray = Curve::Ray(crate::geom2d::Ray {
            origin: [1.0, 2.0],
            direction: [3.0, 4.0],
        });
        assert_eq!(ray.enclosed_area(), 0.0);
    }

    #[test]
    fn a_spline_tracing_a_circle_encloses_the_circle() {
        // Four rational quarters make a NURBS circle of radius 100; one of
        // them plus the two radii closes a quarter disc.
        let weight = std::f64::consts::FRAC_PI_4.cos();
        let quarter = Curve::Nurbs(
            NurbsCurve::new(
                2,
                vec![[100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                Some(vec![1.0, weight, 1.0]),
            )
            .unwrap(),
        );
        let back = Curve::Line(Line {
            start: [0.0, 100.0],
            end: [0.0, 0.0],
        });
        let out = Curve::Line(Line {
            start: [0.0, 0.0],
            end: [100.0, 0.0],
        });
        let total = quarter.enclosed_area() + back.enclosed_area() + out.enclosed_area();
        let expected = PI * 10_000.0 / 4.0;
        assert!(
            (total - expected).abs() < 1e-4 * expected,
            "{total} vs {expected}"
        );
    }

    #[test]
    fn survey_coordinates_measure_the_same_area() {
        let local = circle([0.0, 0.0], 50.0).enclosed_area();
        let remote = circle([512_345.678, 4_512_345.678], 50.0).enclosed_area();
        // The remote form subtracts two numbers around 10^12, so it cannot be
        // exact — but it must still land on the right answer to a part in a
        // billion rather than losing the area entirely.
        assert!(
            (local - remote).abs() < 1e-6 * local,
            "{local} vs {remote}"
        );
    }
}
