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

/// The ends of `curves` that almost meet other geometry: each within
/// `tolerance`, and more than `weld` away (closer counts as meeting).
///
/// Lines, arcs and polylines are read; any other curve is ignored. A closed
/// polyline has no ends but is still geometry that ends can meet. The
/// errors come in the order of their ends.
pub fn end_errors(curves: &[Curve], tolerance: f64, weld: Tolerance) -> Vec<EndError> {
    let weld = weld.linear();
    if !tolerance.is_finite() || tolerance <= weld {
        return Vec::new();
    }
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
        // end of the same polyline.
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
                    if gap > weld && gap <= tolerance {
                        candidates.push(EndError {
                            kind: EndErrorKind::Gap,
                            end: end.end,
                            location: end.point,
                            target: other.point,
                            other: Some(other.end),
                            corner: corner(end, other, tolerance, weld),
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
                if d > weld && d <= tolerance {
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
                if d > weld && d <= tolerance && d < end.piece_length - weld {
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
