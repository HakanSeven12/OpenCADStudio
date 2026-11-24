//! Geometry Cleanup: the ends of lines, arcs and polylines that almost meet
//! other geometry.
//!
//! Three kinds of error, each found within a tolerance and repaired by
//! moving the end:
//!
//! - **Gap**: the end almost meets the end of another curve.
//! - **Overshoot**: the end runs a little past a curve that it crosses.
//! - **Undershoot**: the end stops a little short of a curve.
//!
//! An end that meets other geometry (within the weld tolerance) is not an
//! error. An end with more than one candidate is reported once, as the
//! candidate nearest to it; each end belongs to at most one error.

use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};

use super::angle::normalize_angle;
use super::cross::intersect;
use super::curve::{Arc, Curve, Line};
use super::polyline::BulgeArc;
use super::vec::Vec2;
use super::{analytic_curve_bounds, Tolerance};

/// What is wrong with an end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EndErrorKind {
    Gap,
    Overshoot,
    Undershoot,
}

/// One end of one of the curves given to [`end_errors`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CurveEnd {
    /// The index of the curve.
    pub curve: usize,
    /// The start of the curve (`true`) or its end.
    pub at_start: bool,
}

/// An end that almost meets other geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EndError {
    pub kind: EndErrorKind,
    pub end: CurveEnd,
    /// Where the end is.
    pub location: [f64; 2],
    /// Where the repair puts the end: the other end of a gap, the crossing
    /// of an overshoot, or the point that an undershoot falls short of.
    pub target: [f64; 2],
    /// The other end of a gap.
    pub other: Option<CurveEnd>,
    /// For a gap between two straight ends: the point where their lines
    /// cross, when it is ahead of both ends and within the tolerance of
    /// each.
    pub corner: Option<[f64; 2]>,
    /// How far the end is from `target`, along the curve for an overshoot
    /// or an undershoot.
    pub deviation: f64,
}

/// A straight or circular piece: a line, an arc, or one segment of a
/// polyline.
#[derive(Debug, Clone, Copy)]
enum Shape {
    Line(Line),
    Arc(Arc),
}

impl Shape {
    fn curve(self) -> Curve {
        match self {
            Shape::Line(line) => Curve::Line(line),
            Shape::Arc(arc) => Curve::Arc(arc),
        }
    }
}

struct Piece {
    curve: usize,
    shape: Shape,
}

/// How an end continues past itself: along a straight line or along its
/// circle (counter-clockwise when `turn` is positive).
#[derive(Debug, Clone, Copy)]
enum Reach {
    Straight {
        direction: [f64; 2],
    },
    Round {
        centre: [f64; 2],
        radius: f64,
        angle: f64,
        turn: f64,
    },
}

struct EndInfo {
    end: CurveEnd,
    point: [f64; 2],
    /// The index in `pieces` of the piece that the end is on.
    piece: usize,
    /// The length of that piece.
    piece_length: f64,
    reach: Reach,
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    Vec2::from(a).distance(Vec2::from(b))
}

fn shape_length(shape: Shape) -> f64 {
    match shape {
        Shape::Line(line) => line.length(),
        Shape::Arc(arc) => arc.radius * arc.sweep(),
    }
}

/// The pieces of the curves that Geometry Cleanup reads. A curve of any
/// other kind gives none.
fn pieces_of(curves: &[Curve]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for (curve, item) in curves.iter().enumerate() {
        match item {
            Curve::Line(line) => pieces.push(Piece {
                curve,
                shape: Shape::Line(*line),
            }),
            Curve::Arc(arc) => pieces.push(Piece {
                curve,
                shape: Shape::Arc(*arc),
            }),
            Curve::Polyline(polyline) => {
                let n = polyline.vertices.len();
                let count = if polyline.closed {
                    n
                } else {
                    n.saturating_sub(1)
                };
                for index in 0..count {
                    let from = polyline.vertices[index];
                    let to = polyline.vertices[(index + 1) % n];
                    let shape = match BulgeArc::from_bulge(from.position, to.position, from.bulge) {
                        Some(arc) => {
                            let (start_angle, end_angle) = if arc.sweep > 0.0 {
                                (arc.start_angle, arc.end_angle)
                            } else {
                                (arc.end_angle, arc.start_angle)
                            };
                            Shape::Arc(Arc {
                                centre: arc.center,
                                radius: arc.radius,
                                start_angle,
                                end_angle,
                            })
                        }
                        None => Shape::Line(Line {
                            start: from.position,
                            end: to.position,
                        }),
                    };
                    pieces.push(Piece { curve, shape });
                }
            }
            _ => {}
        }
    }
    pieces
}

/// The two ends of each open curve.
fn ends_of(curves: &[Curve], pieces: &[Piece]) -> Vec<EndInfo> {
    let mut ends = Vec::new();
    let mut first_piece = HashMap::new();
    let mut last_piece = HashMap::new();
    for (k, piece) in pieces.iter().enumerate() {
        first_piece.entry(piece.curve).or_insert(k);
        last_piece.insert(piece.curve, k);
    }
    for (curve, item) in curves.iter().enumerate() {
        let (Some(&first), Some(&last)) = (first_piece.get(&curve), last_piece.get(&curve)) else {
            continue;
        };
        // Whether the curve runs along its pieces the way they are stored
        // (a polyline arc piece is stored counter-clockwise), and the bulge
        // sign of a polyline's end segments.
        let (open, first_turn, last_turn) = match item {
            Curve::Line(_) | Curve::Arc(_) => (true, 1.0, 1.0),
            Curve::Polyline(polyline) => {
                let n = polyline.vertices.len();
                (
                    !polyline.closed && n >= 2,
                    polyline.vertices[0].bulge.signum(),
                    polyline.vertices[n.saturating_sub(2)].bulge.signum(),
                )
            }
            _ => (false, 1.0, 1.0),
        };
        if !open {
            continue;
        }
        for at_start in [true, false] {
            let k = if at_start { first } else { last };
            let piece = &pieces[k];
            let turn = if at_start { first_turn } else { last_turn };
            let (point, reach) = match piece.shape {
                Shape::Line(line) => {
                    let (point, back) = if at_start {
                        (line.start, line.end)
                    } else {
                        (line.end, line.start)
                    };
                    let length = distance(point, back);
                    if length <= 0.0 {
                        continue;
                    }
                    let direction = [(point[0] - back[0]) / length, (point[1] - back[1]) / length];
                    (point, Reach::Straight { direction })
                }
                Shape::Arc(arc) => {
                    // The curve runs counter-clockwise along the piece for a
                    // LINE's ARC and for a positive bulge.
                    let forward = turn > 0.0;
                    // The end of the curve is the piece's end angle when the
                    // curve runs forward and leaves at its end.
                    let at_piece_end = forward != at_start;
                    let angle = if at_piece_end {
                        arc.end_angle
                    } else {
                        arc.start_angle
                    };
                    let point = [
                        arc.centre[0] + arc.radius * angle.cos(),
                        arc.centre[1] + arc.radius * angle.sin(),
                    ];
                    let turn = if at_piece_end { 1.0 } else { -1.0 };
                    (
                        point,
                        Reach::Round {
                            centre: arc.centre,
                            radius: arc.radius,
                            angle,
                            turn,
                        },
                    )
                }
            };
            ends.push(EndInfo {
                end: CurveEnd { curve, at_start },
                point,
                piece: k,
                piece_length: shape_length(piece.shape),
                reach,
            });
        }
    }
    ends
}

/// The continuation of an end, `length` long.
fn extension(end: &EndInfo, length: f64) -> Curve {
    match end.reach {
        Reach::Straight { direction } => Curve::Line(Line {
            start: end.point,
            end: [
                end.point[0] + direction[0] * length,
                end.point[1] + direction[1] * length,
            ],
        }),
        Reach::Round {
            centre,
            radius,
            angle,
            turn,
        } => {
            let sweep = (length / radius).min(PI);
            let (start_angle, end_angle) = if turn > 0.0 {
                (angle, angle + sweep)
            } else {
                (angle - sweep, angle)
            };
            Curve::Arc(Arc {
                centre,
                radius,
                start_angle: normalize_angle(start_angle),
                end_angle: normalize_angle(end_angle),
            })
        }
    }
}

/// How far `point` is from the end, along the end's line or circle; ahead
/// of the end when `ahead`, back along the curve otherwise.
fn along(end: &EndInfo, point: [f64; 2], ahead: bool) -> f64 {
    match end.reach {
        Reach::Straight { direction } => {
            let d =
                (point[0] - end.point[0]) * direction[0] + (point[1] - end.point[1]) * direction[1];
            if ahead {
                d
            } else {
                -d
            }
        }
        Reach::Round {
            centre,
            radius,
            angle,
            turn,
        } => {
            let to = (point[1] - centre[1]).atan2(point[0] - centre[0]);
            let sign = if ahead { turn } else { -turn };
            radius * (sign * (to - angle)).rem_euclid(TAU)
        }
    }
}

type Bounds = ([f64; 2], [f64; 2]);

fn shape_bounds(shape: Shape) -> Option<Bounds> {
    analytic_curve_bounds(&[shape.curve()])
}

/// The pieces near each place, by grid cell.
struct Grid {
    size: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    /// Pieces that cover too many cells to list in each.
    everywhere: Vec<usize>,
}

impl Grid {
    const MAX_CELLS: i64 = 4096;

    fn cell(&self, p: [f64; 2]) -> (i64, i64) {
        (
            (p[0] / self.size).floor() as i64,
            (p[1] / self.size).floor() as i64,
        )
    }

    fn new(pieces: &[Piece], size: f64, reach: f64) -> Self {
        let mut grid = Grid {
            size,
            cells: HashMap::new(),
            everywhere: Vec::new(),
        };
        for (k, piece) in pieces.iter().enumerate() {
            let Some((min, max)) = shape_bounds(piece.shape) else {
                continue;
            };
            let low = grid.cell([min[0] - reach, min[1] - reach]);
            let high = grid.cell([max[0] + reach, max[1] + reach]);
            let count = (high.0 - low.0 + 1).saturating_mul(high.1 - low.1 + 1);
            if !(0..=Self::MAX_CELLS).contains(&count) {
                grid.everywhere.push(k);
                continue;
            }
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    grid.cells.entry((x, y)).or_default().push(k);
                }
            }
        }
        grid
    }

    /// The pieces whose bounds, widened by the reach, hold `p`.
    fn near(&self, p: [f64; 2]) -> impl Iterator<Item = usize> + '_ {
        self.cells
            .get(&self.cell(p))
            .into_iter()
            .flatten()
            .chain(&self.everywhere)
            .copied()
    }
}

/// The tolerance that Geometry Cleanup proposes for a drawing: 1% of the
/// mean length of its lines and polyline segments (of all its pieces when
/// it has neither), and not less than 0.0001.
pub fn proposed_tolerance(curves: &[Curve]) -> f64 {
    const MINIMUM: f64 = 0.0001;
    let pieces = pieces_of(curves);
    let straight: Vec<f64> = pieces
        .iter()
        .filter(|piece| {
            matches!(piece.shape, Shape::Line(_))
                || matches!(curves[piece.curve], Curve::Polyline(_))
        })
        .map(|piece| shape_length(piece.shape))
        .collect();
    let lengths: Vec<f64> = if straight.is_empty() {
        pieces
            .iter()
            .map(|piece| shape_length(piece.shape))
            .collect()
    } else {
        straight
    };
    let lengths: Vec<f64> = lengths.into_iter().filter(|l| l.is_finite()).collect();
    if lengths.is_empty() {
        return MINIMUM;
    }
    let mean = lengths.iter().sum::<f64>() / lengths.len() as f64;
    (mean * 0.01).max(MINIMUM)
}

/// The point where two straight ends' lines cross, when it is ahead of both
/// ends and at most `tolerance` from each.
fn corner(a: &EndInfo, b: &EndInfo, tolerance: f64, weld: f64) -> Option<[f64; 2]> {
    let (Reach::Straight { direction: u }, Reach::Straight { direction: v }) = (a.reach, b.reach)
    else {
        return None;
    };
    let cross = u[0] * v[1] - u[1] * v[0];
    if cross.abs() < 1e-9 {
        return None;
    }
    let w = [b.point[0] - a.point[0], b.point[1] - a.point[1]];
    let s = (w[0] * v[1] - w[1] * v[0]) / cross;
    let t = (w[0] * u[1] - w[1] * u[0]) / cross;
    let reach = -weld..=tolerance;
    (reach.contains(&s) && reach.contains(&t))
        .then(|| [a.point[0] + u[0] * s, a.point[1] + u[1] * s])
}

/// The tolerance of each error type. `None`: the scan does not look for
/// that type.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Tolerances {
    pub gap: Option<f64>,
    pub overshoot: Option<f64>,
    pub undershoot: Option<f64>,
}

impl Tolerances {
    /// The same tolerance for each type.
    pub fn all(tolerance: f64) -> Self {
        Self {
            gap: Some(tolerance),
            overshoot: Some(tolerance),
            undershoot: Some(tolerance),
        }
    }

    pub fn get(&self, kind: EndErrorKind) -> Option<f64> {
        match kind {
            EndErrorKind::Gap => self.gap,
            EndErrorKind::Overshoot => self.overshoot,
            EndErrorKind::Undershoot => self.undershoot,
        }
    }

    /// The largest tolerance of the types that are scanned.
    pub fn largest(&self) -> Option<f64> {
        [self.gap, self.overshoot, self.undershoot]
            .into_iter()
            .flatten()
            .reduce(f64::max)
    }
}

/// The ends of `curves` that almost meet other geometry: each within the
/// tolerance of its error type, and more than `weld` away (closer counts as
/// meeting). A type with no tolerance is not looked for.
///
/// Lines, arcs and polylines are read; any other curve is ignored. A closed
/// polyline has no ends but is still geometry that ends can meet. The
/// errors come in the order of their ends.
pub fn end_errors(curves: &[Curve], tolerances: Tolerances, weld: Tolerance) -> Vec<EndError> {
    let weld = weld.linear();
    // The search reaches as far as the largest tolerance; each candidate
    // is then held to the tolerance of its own type, before the nearest
    // error of each end is picked.
    let Some(tolerance) = tolerances.largest() else {
        return Vec::new();
    };
    if !tolerance.is_finite() || tolerance <= weld {
        return Vec::new();
    }
    let within = |kind: EndErrorKind, d: f64| tolerances.get(kind).is_some_and(|t| d <= t);
    let pieces = pieces_of(curves);
    let ends = ends_of(curves, &pieces);
    let mean = if pieces.is_empty() {
        tolerance
    } else {
        pieces.iter().map(|p| shape_length(p.shape)).sum::<f64>() / pieces.len() as f64
    };
    let size = if mean.is_finite() {
        mean.max(2.0 * tolerance)
    } else {
        2.0 * tolerance
    };
    let grid = Grid::new(&pieces, size, tolerance);

    // An end is loose when no other piece passes within the weld of it.
    let loose: Vec<bool> = ends
        .iter()
        .map(|end| {
            !grid.near(end.point).any(|k| {
                k != end.piece
                    && super::containment::distance_to(&pieces[k].shape.curve(), end.point) <= weld
            })
        })
        .collect();

    let mut candidates = Vec::new();
    let mut by_cell: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let cell = |p: [f64; 2]| {
        (
            (p[0] / tolerance).floor() as i64,
            (p[1] / tolerance).floor() as i64,
        )
    };
    for (e, end) in ends.iter().enumerate() {
        if loose[e] {
            by_cell.entry(cell(end.point)).or_default().push(e);
        }
    }

    for (e, end) in ends.iter().enumerate() {
        if !loose[e] {
            continue;
        }
        // Gaps: another loose end close by, of another curve or the other
        // end of the same polyline. The corner of a gap is held to the gap
        // tolerance.
        let gap_tolerance = tolerances.gap.unwrap_or(0.0);
        let (cx, cy) = cell(end.point);
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                for &f in by_cell.get(&(x, y)).into_iter().flatten() {
                    let other = &ends[f];
                    let same_curve = other.end.curve == end.end.curve;
                    if f <= e
                        || (same_curve && !matches!(curves[end.end.curve], Curve::Polyline(_)))
                    {
                        continue;
                    }
                    let gap = distance(end.point, other.point);
                    if gap > weld && within(EndErrorKind::Gap, gap) {
                        candidates.push(EndError {
                            kind: EndErrorKind::Gap,
                            end: end.end,
                            location: end.point,
                            target: other.point,
                            other: Some(other.end),
                            corner: corner(end, other, gap_tolerance, weld),
                            deviation: gap,
                        });
                    }
                }
            }
        }

        let own = &pieces[end.piece];
        let reach = extension(end, tolerance);
        let mut seen = HashSet::new();
        for k in grid.near(end.point) {
            if k == end.piece || !seen.insert(k) {
                continue;
            }
            let target = pieces[k].shape.curve();
            // Undershoot: the end's continuation reaches the piece.
            for crossing in intersect(&reach, &target, Tolerance::new(weld)) {
                let d = along(end, crossing.point, true);
                if d > weld && within(EndErrorKind::Undershoot, d) {
                    candidates.push(EndError {
                        kind: EndErrorKind::Undershoot,
                        end: end.end,
                        location: end.point,
                        target: crossing.point,
                        other: None,
                        corner: None,
                        deviation: d,
                    });
                }
            }
            // Overshoot: the end's own piece crosses the piece just before
            // the end. A crossing at the far end of the end's piece (where a
            // polyline goes on, or a short line is attached) is not one.
            for crossing in intersect(&own.shape.curve(), &target, Tolerance::new(weld)) {
                let d = along(end, crossing.point, false);
                if d > weld && within(EndErrorKind::Overshoot, d) && d < end.piece_length - weld {
                    candidates.push(EndError {
                        kind: EndErrorKind::Overshoot,
                        end: end.end,
                        location: end.point,
                        target: crossing.point,
                        other: None,
                        corner: None,
                        deviation: d,
                    });
                }
            }
        }
    }

    // Nearest first; at equal distance a gap (end to end) before the
    // others. Each end goes to one error only.
    candidates.sort_by(|a, b| {
        a.deviation
            .total_cmp(&b.deviation)
            .then(a.kind.cmp(&b.kind))
            .then(a.end.cmp(&b.end))
    });
    let mut used = HashSet::new();
    let mut errors: Vec<EndError> = candidates
        .into_iter()
        .filter(|error| {
            if used.contains(&error.end) || error.other.is_some_and(|other| used.contains(&other)) {
                return false;
            }
            used.insert(error.end);
            if let Some(other) = error.other {
                used.insert(other);
            }
            true
        })
        .collect();
    errors.sort_by_key(|error| error.end);
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::{Polyline, PolylineVertex};

    fn line(start: [f64; 2], end: [f64; 2]) -> Curve {
        Curve::Line(Line { start, end })
    }

    fn polyline(points: &[[f64; 2]], closed: bool) -> Curve {
        Curve::Polyline(Polyline {
            vertices: points
                .iter()
                .map(|&position| PolylineVertex {
                    position,
                    bulge: 0.0,
                })
                .collect(),
            closed,
        })
    }

    fn scan(curves: &[Curve], tolerance: f64) -> Vec<EndError> {
        end_errors(curves, Tolerances::all(tolerance), Tolerance::new(1e-9))
    }

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        distance(a, b) < 1e-9
    }

    fn end(curve: usize, at_start: bool) -> CurveEnd {
        CurveEnd { curve, at_start }
    }

    #[test]
    fn ends_that_meet_or_are_far_apart_are_not_errors() {
        let meeting = [
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.0, 0.0], [10.0, 10.0]),
        ];
        assert!(scan(&meeting, 0.5).is_empty());
        let far = [
            line([0.0, 0.0], [10.0, 0.0]),
            line([11.0, 0.0], [20.0, 0.0]),
        ];
        assert!(scan(&far, 0.5).is_empty());
    }

    #[test]
    fn a_gap_between_two_ends_targets_the_other_end_and_its_corner() {
        let curves = [
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.2, 0.1], [10.2, 10.0]),
        ];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let gap = errors[0];
        assert_eq!(gap.kind, EndErrorKind::Gap);
        assert_eq!(gap.end, end(0, false));
        assert_eq!(gap.other, Some(end(1, true)));
        assert!(near(gap.location, [10.0, 0.0]));
        assert!(near(gap.target, [10.2, 0.1]));
        assert!(near(gap.corner.expect("a corner"), [10.2, 0.0]));
        assert!((gap.deviation - 0.05f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn a_gap_between_collinear_ends_has_no_corner() {
        let curves = [
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.3, 0.0], [20.0, 0.0]),
        ];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Gap);
        assert_eq!(errors[0].corner, None);
    }

    #[test]
    fn an_end_short_of_a_line_is_an_undershoot() {
        let curves = [
            line([0.0, 0.0], [9.7, 0.0]),
            line([10.0, -5.0], [10.0, 5.0]),
        ];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let undershoot = errors[0];
        assert_eq!(undershoot.kind, EndErrorKind::Undershoot);
        assert_eq!(undershoot.end, end(0, false));
        assert!(near(undershoot.target, [10.0, 0.0]));
        assert!((undershoot.deviation - 0.3).abs() < 1e-9);
    }

    #[test]
    fn an_end_past_a_line_is_an_overshoot() {
        let curves = [
            line([10.4, 0.0], [0.0, 0.0]),
            line([10.0, -5.0], [10.0, 5.0]),
        ];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let overshoot = errors[0];
        assert_eq!(overshoot.kind, EndErrorKind::Overshoot);
        assert_eq!(overshoot.end, end(0, true));
        assert!(near(overshoot.target, [10.0, 0.0]));
        assert!((overshoot.deviation - 0.4).abs() < 1e-9);
    }

    #[test]
    fn a_short_line_attached_at_its_other_end_is_not_an_overshoot() {
        let curves = [
            line([10.0, 0.0], [10.3, 0.0]),
            line([10.0, -5.0], [10.0, 5.0]),
        ];
        assert!(scan(&curves, 0.5).is_empty());
    }

    #[test]
    fn an_arc_end_reaches_along_its_circle() {
        // A quarter arc of radius 10 about the origin that stops 0.03 rad
        // short of the y axis, where a line runs.
        let stop = std::f64::consts::FRAC_PI_2 - 0.03;
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius: 10.0,
            start_angle: 0.0,
            end_angle: stop,
        });
        let curves = [arc, line([0.0, 0.0], [0.0, 20.0])];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Undershoot);
        assert_eq!(errors[0].end, end(0, false));
        assert!(near(errors[0].target, [0.0, 10.0]));
        assert!((errors[0].deviation - 0.3).abs() < 1e-9);
    }

    #[test]
    fn an_almost_closed_polyline_has_a_gap_between_its_own_ends() {
        let outline = polyline(
            &[
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [0.0, 0.2],
            ],
            false,
        );
        let errors = scan(&[outline], 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Gap);
        assert_eq!(errors[0].end, end(0, true));
        assert_eq!(errors[0].other, Some(end(0, false)));
    }

    #[test]
    fn a_line_shorter_than_the_tolerance_has_no_gap_to_itself() {
        assert!(scan(&[line([0.0, 0.0], [0.2, 0.0])], 0.5).is_empty());
    }

    #[test]
    fn a_closed_polyline_has_no_ends_but_ends_can_fall_short_of_it() {
        let square = polyline(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], true);
        assert!(scan(std::slice::from_ref(&square), 0.5).is_empty());
        let errors = scan(&[square, line([5.0, 20.0], [5.0, 10.2])], 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Undershoot);
        assert!(near(errors[0].target, [5.0, 10.0]));
    }

    #[test]
    fn each_end_is_reported_once_as_its_nearest_error() {
        // The end of the first line is 0.1 short of the vertical line and
        // 0.4 from the end of the third line: the undershoot wins, and the
        // third line's end, left without a partner, is not reported.
        let curves = [
            line([0.0, 0.0], [9.9, 0.0]),
            line([10.0, -5.0], [10.0, 5.0]),
            line([9.9, 0.4], [9.9, 8.0]),
        ];
        let errors = scan(&curves, 0.5);
        let kinds: Vec<_> = errors.iter().map(|e| (e.kind, e.end)).collect();
        assert!(
            kinds.contains(&(EndErrorKind::Undershoot, end(0, false))),
            "{errors:?}"
        );
        assert!(
            !errors.iter().any(|e| e.kind == EndErrorKind::Gap),
            "{errors:?}"
        );
    }

    #[test]
    fn a_polyline_end_reaches_along_its_last_segment() {
        let path = polyline(&[[0.0, 0.0], [5.0, 0.0], [5.0, 4.8]], false);
        let errors = scan(&[path, line([0.0, 5.0], [10.0, 5.0])], 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Undershoot);
        assert_eq!(errors[0].end, end(0, false));
        assert!(near(errors[0].target, [5.0, 5.0]));
    }

    #[test]
    fn the_proposed_tolerance_is_a_hundredth_of_the_mean_length() {
        let curves = [line([0.0, 0.0], [10.0, 0.0]), line([0.0, 0.0], [0.0, 30.0])];
        assert!((proposed_tolerance(&curves) - 0.2).abs() < 1e-12);
        assert_eq!(proposed_tolerance(&[]), 0.0001);
        assert_eq!(
            proposed_tolerance(&[line([0.0, 0.0], [0.001, 0.0])]),
            0.0001
        );
    }

    fn arc(centre: [f64; 2], radius: f64, start_angle: f64, end_angle: f64) -> Curve {
        Curve::Arc(Arc {
            centre,
            radius,
            start_angle,
            end_angle,
        })
    }

    #[test]
    fn an_oblique_corner_is_where_both_lines_cross() {
        // Both lines run towards (6, 5): the first stops 0.2 short of it
        // along (0.6, 0.8), the second 0.3 short along (-0.8, 0.6).
        let curves = [
            line([-0.12, -3.16], [5.88, 4.84]),
            line([14.24, -1.18], [6.24, 4.82]),
        ];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Gap);
        assert_eq!(errors[0].other, Some(end(1, false)));
        let corner = errors[0].corner.expect("a corner");
        assert!(distance(corner, [6.0, 5.0]) < 1e-9, "{corner:?}");
        assert!((errors[0].deviation - 0.36f64.hypot(0.02)).abs() < 1e-9);
    }

    #[test]
    fn a_corner_farther_than_the_tolerance_from_an_end_is_not_offered() {
        // The lines cross 0.525 ahead of the first end.
        let curves = [
            line([-10.0, 0.0], [0.0, 0.0]),
            line([-5.7, 8.3], [0.3, 0.3]),
        ];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Gap);
        assert_eq!(errors[0].corner, None);
    }

    #[test]
    fn both_ends_of_an_arc_reach_along_its_circle() {
        let centre = [3.0, -2.0];
        let quarter = std::f64::consts::FRAC_PI_2;
        let curves = [
            arc(centre, 10.0, 0.03, quarter - 0.03),
            line(centre, [3.0, 20.0]),
            line(centre, [20.0, -2.0]),
        ];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors.iter().all(|e| e.kind == EndErrorKind::Undershoot));
        let at = |at_start| {
            errors
                .iter()
                .find(|e| e.end == end(0, at_start))
                .expect("both ends")
        };
        let start = [3.0 + 10.0 * 0.03f64.cos(), -2.0 + 10.0 * 0.03f64.sin()];
        assert!(near(at(true).location, start), "{errors:?}");
        assert!(near(at(true).target, [13.0, -2.0]), "{errors:?}");
        assert!(near(at(false).target, [3.0, 8.0]), "{errors:?}");
        assert!(errors.iter().all(|e| (e.deviation - 0.3).abs() < 1e-9));
    }

    #[test]
    fn an_arc_end_past_a_line_is_an_overshoot() {
        let curves = [
            arc([3.0, -2.0], 10.0, 0.0, std::f64::consts::FRAC_PI_2 + 0.03),
            line([3.0, 0.0], [3.0, 20.0]),
        ];
        let errors = scan(&curves, 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Overshoot);
        assert_eq!(errors[0].end, end(0, false));
        assert!(near(errors[0].target, [3.0, 8.0]), "{errors:?}");
        assert!((errors[0].deviation - 0.3).abs() < 1e-9);
    }

    #[test]
    fn a_small_arc_reaches_the_tolerance_along_its_circle() {
        // Radius 0.5: the 0.3 shortfall is 0.6 rad of the circle.
        let curves = [
            arc([2.0, 1.0], 0.5, 0.0, std::f64::consts::FRAC_PI_2 - 0.6),
            line([2.0, 0.6], [2.0, 5.0]),
        ];
        let errors = scan(&curves, 0.4);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Undershoot);
        assert!(near(errors[0].target, [2.0, 1.5]), "{errors:?}");
        assert!((errors[0].deviation - 0.3).abs() < 1e-9);
    }

    #[test]
    fn a_clockwise_polyline_arc_reaches_along_its_circle() {
        // From the top of a circle about (3, -2) clockwise to 0.03 rad
        // above its right side.
        let to = [3.0 + 10.0 * 0.03f64.cos(), -2.0 + 10.0 * 0.03f64.sin()];
        let sweep = -(std::f64::consts::FRAC_PI_2 - 0.03);
        let path = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex {
                    position: [3.0, 8.0],
                    bulge: (sweep / 4.0).tan(),
                },
                PolylineVertex {
                    position: to,
                    bulge: 0.0,
                },
            ],
            closed: false,
        });
        let errors = scan(&[path, line([8.0, -2.0], [20.0, -2.0])], 0.5);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Undershoot);
        assert_eq!(errors[0].end, end(0, false));
        assert!(near(errors[0].target, [13.0, -2.0]), "{errors:?}");
        assert!((errors[0].deviation - 0.3).abs() < 1e-9);
    }

    #[test]
    fn a_large_tolerance_reaches_as_far_as_it_says() {
        let gap = [
            line([-50.0, 1.0], [1.0, 1.0]),
            line([4.0, 3.0], [4.0, 50.0]),
        ];
        let errors = scan(&gap, 5.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Gap);
        assert!(near(errors[0].corner.expect("a corner"), [4.0, 1.0]));
        let under = [
            line([-50.0, 0.0], [0.0, 0.0]),
            line([4.0, -50.0], [4.0, 50.0]),
        ];
        let errors = scan(&under, 5.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Undershoot);
        assert!((errors[0].deviation - 4.0).abs() < 1e-9);
    }

    #[test]
    fn a_gap_is_found_across_the_edge_of_a_search_cell() {
        // With a tolerance of 1, the ends at 0.9 and 1.1 are in cells next
        // to each other: along X and along Y, with the lower or the upper
        // cell scanned first.
        let flip = |p: [f64; 2]| [p[1], p[0]];
        let close = [([-10.0, 0.0], [0.9, 0.0]), ([1.1, 0.1], [1.1, 10.0])];
        for swap in [false, true] {
            for reverse in [false, true] {
                let mut curves: Vec<Curve> = close
                    .iter()
                    .map(|&(a, b)| {
                        if swap {
                            line(flip(a), flip(b))
                        } else {
                            line(a, b)
                        }
                    })
                    .collect();
                if reverse {
                    curves.reverse();
                }
                let errors = scan(&curves, 1.0);
                assert_eq!(errors.len(), 1, "swap {swap} reverse {reverse}: {errors:?}");
                assert_eq!(errors[0].kind, EndErrorKind::Gap);
            }
        }
        // Ends in cells next to each other but more than the tolerance
        // apart are not a gap.
        let apart = [
            line([-10.0, 0.0], [0.9, 0.0]),
            line([1.95, 0.5], [1.95, 10.0]),
        ];
        assert!(scan(&apart, 1.0).is_empty());
    }

    #[test]
    fn the_proposed_tolerance_leaves_out_arcs_unless_there_are_only_arcs() {
        let quarter = arc([0.0, 0.0], 10.0, 0.0, std::f64::consts::FRAC_PI_2);
        let expected = 0.01 * 5.0 * std::f64::consts::PI;
        assert!((proposed_tolerance(std::slice::from_ref(&quarter)) - expected).abs() < 1e-12);
        let mixed = [quarter, line([0.0, 0.0], [10.0, 0.0])];
        assert!((proposed_tolerance(&mixed) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn a_type_that_is_not_scanned_is_not_reported() {
        let curves = [
            line([0.0, 0.0], [9.7, 0.0]),
            line([10.0, -5.0], [10.0, 5.0]),
        ];
        let off = Tolerances {
            undershoot: None,
            ..Tolerances::all(0.5)
        };
        assert!(end_errors(&curves, off, Tolerance::new(1e-9)).is_empty());
        assert!(end_errors(&curves, Tolerances::default(), Tolerance::new(1e-9)).is_empty());
    }

    #[test]
    fn each_type_uses_its_own_tolerance() {
        // The end of the first line is 0.5 from the start of the third line
        // (a gap) and 0.6 short of the wall (an undershoot). With a gap
        // tolerance of 0.2, the undershoot is the error of that end.
        let curves = [
            line([-10.0, 0.0], [0.0, 0.0]),
            line([0.6, -5.0], [0.6, 5.0]),
            line([0.0, 0.5], [0.0, 10.0]),
        ];
        let tolerances = Tolerances {
            gap: Some(0.2),
            overshoot: Some(1.0),
            undershoot: Some(1.0),
        };
        let errors = end_errors(&curves, tolerances, Tolerance::new(1e-9));
        let first = errors
            .iter()
            .find(|e| e.end == end(0, false))
            .expect("an error");
        assert_eq!(first.kind, EndErrorKind::Undershoot);
        assert!(near(first.target, [0.6, 0.0]));
        assert!(
            !errors.iter().any(|e| e.kind == EndErrorKind::Gap),
            "{errors:?}"
        );
    }

    #[test]
    fn a_gap_corner_is_held_to_the_gap_tolerance() {
        // The second line ends 0.15 above the end of the first and meets
        // its line 0.9 ahead: within the undershoot tolerance, not within
        // the gap tolerance, so the gap has no corner.
        let to = [0.0, 0.15];
        let dir = [0.9 / 0.9f64.hypot(0.15), -0.15 / 0.9f64.hypot(0.15)];
        let back = [to[0] - 5.0 * dir[0], to[1] - 5.0 * dir[1]];
        let curves = [line([-10.0, 0.0], [0.0, 0.0]), line(back, to)];
        let tolerances = Tolerances {
            gap: Some(0.2),
            overshoot: Some(1.0),
            undershoot: Some(1.0),
        };
        let errors = end_errors(&curves, tolerances, Tolerance::new(1e-9));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Gap);
        assert_eq!(errors[0].corner, None);
    }

    #[test]
    fn no_tolerance_above_the_weld_finds_nothing() {
        let curves = [
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.2, 0.0], [20.0, 0.0]),
        ];
        assert!(end_errors(&curves, Tolerances::all(1e-9), Tolerance::new(1e-9)).is_empty());
        assert!(end_errors(&curves, Tolerances::all(f64::NAN), Tolerance::new(1e-9)).is_empty());
        assert!(end_errors(
            &curves,
            Tolerances::all(f64::INFINITY),
            Tolerance::new(1e-9)
        )
        .is_empty());
    }
}
