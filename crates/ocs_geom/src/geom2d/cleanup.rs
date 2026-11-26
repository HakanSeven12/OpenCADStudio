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
//!
//! [`angle_errors`] finds lines a little off an angle: off a standard angle
//! (**Absolute**), or meeting another line a little off a standard angle
//! (**Relative**).

use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};

use super::angle::normalize_angle;
use super::cross::intersect;
use super::curve::{Arc, Curve, EllipseArc, Line};
use super::polyline::BulgeArc;
use super::vec::Vec2;
use super::{analytic_curve_bounds, Ellipse, Tolerance};

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

/// A piece of a curve: a line, an arc, an elliptical arc, or one segment of
/// a polyline.
#[derive(Debug, Clone, Copy)]
enum Shape {
    Line(Line),
    Arc(Arc),
    Ellipse(EllipseArc),
}

impl Shape {
    fn curve(self) -> Curve {
        match self {
            Shape::Line(line) => Curve::Line(line),
            Shape::Arc(arc) => Curve::Arc(arc),
            Shape::Ellipse(arc) => Curve::Ellipse(arc),
        }
    }
}

struct Piece {
    curve: usize,
    /// The index of the segment, for a piece of a polyline.
    segment: Option<usize>,
    shape: Shape,
}

/// How an end continues past itself: along a straight line, along its
/// circle (counter-clockwise when `turn` is positive), or along its ellipse
/// (to a greater parameter when `turn` is positive).
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
    Elliptic {
        ellipse: Ellipse,
        parameter: f64,
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
        Shape::Ellipse(arc) => ellipse_length(
            &arc.ellipse,
            arc.start_parameter,
            arc.start_parameter + arc.sweep(),
        ),
    }
}

/// The length of `ellipse` between the parameters `from` and `to`, in either
/// order (Simpson's rule on the speed of the parameter).
fn ellipse_length(ellipse: &Ellipse, from: f64, to: f64) -> f64 {
    const STEPS: usize = 64;
    let (a, b) = (ellipse.major_radius, ellipse.minor_radius);
    let speed = |t: f64| (a * t.sin()).hypot(b * t.cos());
    let step = (to - from) / STEPS as f64;
    let inner: f64 = (1..STEPS)
        .map(|k| {
            let weight = if k % 2 == 1 { 4.0 } else { 2.0 };
            weight * speed(from + step * k as f64)
        })
        .sum();
    (step / 3.0 * (speed(from) + inner + speed(to))).abs()
}

/// The pieces of the curves that Geometry Cleanup reads. A curve of any
/// other kind gives none.
fn pieces_of(curves: &[Curve]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for (curve, item) in curves.iter().enumerate() {
        match item {
            Curve::Line(line) => pieces.push(Piece {
                curve,
                segment: None,
                shape: Shape::Line(*line),
            }),
            Curve::Arc(arc) => pieces.push(Piece {
                curve,
                segment: None,
                shape: Shape::Arc(*arc),
            }),
            // An elliptical arc; a whole ellipse has no ends and is not read.
            Curve::Ellipse(arc) if arc.sweep() < TAU - 1e-9 && !arc.ellipse.is_degenerate() => {
                pieces.push(Piece {
                    curve,
                    segment: None,
                    shape: Shape::Ellipse(*arc),
                })
            }
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
                    pieces.push(Piece {
                        curve,
                        segment: Some(index),
                        shape,
                    });
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
            Curve::Line(_) | Curve::Arc(_) | Curve::Ellipse(_) => (true, 1.0, 1.0),
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
                // The start goes on to smaller parameters, the end to
                // greater ones.
                Shape::Ellipse(arc) => {
                    let (parameter, turn) = if at_start {
                        (arc.start_parameter, -1.0)
                    } else {
                        (arc.start_parameter + arc.sweep(), 1.0)
                    };
                    (
                        arc.ellipse.point_at(parameter),
                        Reach::Elliptic {
                            ellipse: arc.ellipse,
                            parameter,
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
        // The parameter moves at least the minor radius for each unit of
        // it, so this sweep reaches `length` along the ellipse.
        Reach::Elliptic {
            ellipse,
            parameter,
            turn,
        } => {
            let sweep = (length / ellipse.minor_radius).min(PI);
            let (start_parameter, end_parameter) = if turn > 0.0 {
                (parameter, parameter + sweep)
            } else {
                (parameter - sweep, parameter)
            };
            Curve::Ellipse(EllipseArc {
                ellipse,
                start_parameter,
                end_parameter,
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
        Reach::Elliptic {
            ellipse,
            parameter,
            turn,
        } => {
            let to = ellipse.closest_parameter(point);
            let sign = if ahead { turn } else { -turn };
            let turned = (sign * (to - parameter)).rem_euclid(TAU);
            ellipse_length(&ellipse, parameter, parameter + sign * turned)
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
/// Lines, arcs, elliptical arcs and polylines are read; any other curve is
/// ignored. A closed polyline has no ends but is still geometry that ends
/// can meet. The
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

/// What is wrong with the angle of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AngleErrorKind {
    /// The line is a little off a standard angle.
    Absolute,
    /// The line meets another line a little off a standard angle.
    Relative,
}

/// A standard angle and how far from it a line is an error, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngleTolerance {
    pub angle: f64,
    pub tolerance: f64,
}

/// The standard angles of each angle type. An empty list: the scan does
/// not look for that type.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AngleTolerances {
    pub absolute: Vec<AngleTolerance>,
    pub relative: Vec<AngleTolerance>,
}

/// A line, or a straight segment of a polyline, a little off an angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngleError {
    pub kind: AngleErrorKind,
    /// The index of the line or polyline.
    pub curve: usize,
    /// The index of the segment, when `curve` is a polyline: segment `n`
    /// runs from vertex `n` to the next one.
    pub segment: Option<usize>,
    /// The line or polyline that it meets (Relative).
    pub other: Option<usize>,
    /// The segment of `other`, when it is a polyline.
    pub other_segment: Option<usize>,
    /// The middle of the line, or the point where the two lines meet.
    pub location: [f64; 2],
    /// The standard angle, in degrees.
    pub angle: f64,
    /// How far the line is off it, in degrees.
    pub deviation: f64,
    /// The turn, in radians counter-clockwise, that puts `curve` on the
    /// angle. Relative: the opposite turn of `other` does it as well.
    pub turn: f64,
    /// Whether the start and the end of `curve` meet other geometry.
    pub meets: [bool; 2],
}

/// Nearer than this to the angle, in degrees, is on it.
const ON_ANGLE: f64 = 1e-6;

/// `degrees` wrapped into [-90, 90): a line is the same line turned 180°.
fn half_turn(degrees: f64) -> f64 {
    (degrees + 90.0).rem_euclid(180.0) - 90.0
}

/// Of the standard angles with the turn (degrees) that reaches each, the
/// one with the smallest turn within its tolerance and not on the angle.
fn nearest(
    candidates: impl Iterator<Item = (AngleTolerance, f64)>,
) -> Option<(AngleTolerance, f64)> {
    candidates
        .filter(|(s, turn)| turn.abs() > ON_ANGLE && turn.abs() <= s.tolerance)
        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
}

/// The lines among `curves` that are a little off an angle, each within
/// the tolerance of the nearest standard angle and not on it. Angles are
/// measured counter-clockwise from `x_axis` (radians from the X axis, for
/// a UCS turned about Z). Two lines meet where they cross or touch within
/// `weld`; they make two angles, θ and 180° − θ, and a standard angle
/// matches the nearer one.
///
/// Lines and the straight segments of polylines are read; two segments of
/// one polyline can make a Relative error. Absolute errors come first, by
/// line or segment; then the Relative ones, by pair.
pub fn angle_errors(
    curves: &[Curve],
    tolerances: &AngleTolerances,
    x_axis: f64,
    weld: Tolerance,
) -> Vec<AngleError> {
    let weld = weld.linear();
    let pieces = pieces_of(curves);
    // Each line or straight segment: its curve, its segment and the line.
    let lines: Vec<(usize, Option<usize>, Line)> = pieces
        .iter()
        .filter_map(|piece| match piece.shape {
            Shape::Line(line) if line.length() > weld => Some((piece.curve, piece.segment, line)),
            _ => None,
        })
        .collect();
    if lines.is_empty() || (tolerances.absolute.is_empty() && tolerances.relative.is_empty()) {
        return Vec::new();
    }
    let direction = |line: &Line| {
        (line.end[1] - line.start[1])
            .atan2(line.end[0] - line.start[0])
            .to_degrees()
            - x_axis.to_degrees()
    };
    let mean = pieces.iter().map(|p| shape_length(p.shape)).sum::<f64>() / pieces.len() as f64;
    let size = if mean.is_finite() && mean > 0.0 {
        mean
    } else {
        1.0
    };
    let grid = Grid::new(&pieces, size, weld);
    // Whether `point` meets geometry other than the line or segment: the
    // next segment of the same polyline counts.
    let meets = |curve: usize, segment: Option<usize>, point: [f64; 2]| {
        grid.near(point).any(|k| {
            (pieces[k].curve, pieces[k].segment) != (curve, segment)
                && super::containment::distance_to(&pieces[k].shape.curve(), point) <= weld
        })
    };

    let mut errors = Vec::new();
    for (curve, segment, line) in &lines {
        let angle = direction(line);
        let candidates = tolerances
            .absolute
            .iter()
            .map(|s| (*s, half_turn(s.angle - angle)));
        if let Some((standard, turn)) = nearest(candidates) {
            errors.push(AngleError {
                kind: AngleErrorKind::Absolute,
                curve: *curve,
                segment: *segment,
                other: None,
                other_segment: None,
                location: [
                    (line.start[0] + line.end[0]) / 2.0,
                    (line.start[1] + line.end[1]) / 2.0,
                ],
                angle: standard.angle,
                deviation: turn.abs(),
                turn: turn.to_radians(),
                meets: [
                    meets(*curve, *segment, line.start),
                    meets(*curve, *segment, line.end),
                ],
            });
        }
    }

    if tolerances.relative.is_empty() {
        return errors;
    }
    // The pairs of lines that share a grid cell.
    let line_grid = Grid::new(
        &lines
            .iter()
            .map(|(curve, segment, line)| Piece {
                curve: *curve,
                segment: *segment,
                shape: Shape::Line(*line),
            })
            .collect::<Vec<_>>(),
        size,
        weld,
    );
    let mut pairs = HashSet::new();
    for members in line_grid.cells.values() {
        for (n, &a) in members.iter().enumerate() {
            for &b in members[n + 1..].iter().chain(&line_grid.everywhere) {
                pairs.insert((a.min(b), a.max(b)));
            }
        }
    }
    for (n, &a) in line_grid.everywhere.iter().enumerate() {
        for &b in &line_grid.everywhere[n + 1..] {
            pairs.insert((a.min(b), a.max(b)));
        }
    }
    let mut pairs: Vec<(usize, usize)> = pairs.into_iter().filter(|(a, b)| a != b).collect();
    pairs.sort_unstable();
    for (a, b) in pairs {
        let ((first, first_segment, one), (second, second_segment, two)) = (lines[a], lines[b]);
        let crossings = intersect(&Curve::Line(one), &Curve::Line(two), Tolerance::new(weld));
        let Some(meet) = crossings.first().map(|crossing| crossing.point) else {
            continue;
        };
        // θ: from the first line to the second. Turning the first line by
        // `turn` makes it θ − turn, which is the standard angle A or
        // 180° − A (that is, −A modulo 180°).
        let theta = direction(&two) - direction(&one);
        let candidates = tolerances.relative.iter().flat_map(|s| {
            [
                (*s, half_turn(theta - s.angle)),
                (*s, half_turn(theta + s.angle)),
            ]
        });
        let Some((standard, turn)) = nearest(candidates) else {
            continue;
        };
        errors.push(AngleError {
            kind: AngleErrorKind::Relative,
            curve: first,
            segment: first_segment,
            other: Some(second),
            other_segment: second_segment,
            location: meet,
            angle: standard.angle,
            deviation: turn.abs(),
            turn: turn.to_radians(),
            meets: [
                meets(first, first_segment, one.start),
                meets(first, first_segment, one.end),
            ],
        });
    }
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

    /// A line from `from`, `length` long, at `degrees`.
    fn at(from: [f64; 2], degrees: f64, length: f64) -> Curve {
        let (sin, cos) = degrees.to_radians().sin_cos();
        line(from, [from[0] + length * cos, from[1] + length * sin])
    }

    fn end_of(curve: &Curve) -> [f64; 2] {
        match curve {
            Curve::Line(line) => line.end,
            _ => unreachable!(),
        }
    }

    fn standards(angles: &[f64]) -> Vec<AngleTolerance> {
        angles
            .iter()
            .map(|&angle| AngleTolerance {
                angle,
                tolerance: 5.0,
            })
            .collect()
    }

    fn angles(
        curves: &[Curve],
        absolute: &[f64],
        relative: &[f64],
        x_axis: f64,
    ) -> Vec<AngleError> {
        let tolerances = AngleTolerances {
            absolute: standards(absolute),
            relative: standards(relative),
        };
        angle_errors(curves, &tolerances, x_axis, Tolerance::new(1e-9))
    }

    fn degrees(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_line_a_little_off_an_angle_is_an_absolute_error() {
        let curves = [
            at([3.2, -1.7], 91.7, 12.4),
            line([7.3, 2.1], [7.3, 15.6]),
            at([-6.1, 4.4], 96.2, 8.3),
        ];
        let errors = angles(&curves, &[0.0, 90.0], &[], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let error = errors[0];
        assert_eq!(error.kind, AngleErrorKind::Absolute);
        assert_eq!(error.curve, 0);
        assert_eq!(error.angle, 90.0);
        assert!(degrees(error.deviation, 1.7), "{error:?}");
        assert!(degrees(error.turn.to_degrees(), -1.7), "{error:?}");
        let end = end_of(&curves[0]);
        assert!(near(
            error.location,
            [(3.2 + end[0]) / 2.0, (-1.7 + end[1]) / 2.0]
        ));
        assert_eq!(error.meets, [false, false]);
    }

    #[test]
    fn an_angle_is_the_same_angle_turned_half_round() {
        let errors = angles(&[at([2.3, 5.9], 178.6, 7.7)], &[0.0], &[], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].angle, 0.0);
        assert!(degrees(errors[0].turn.to_degrees(), 1.4), "{errors:?}");
    }

    #[test]
    fn a_line_near_two_angles_is_reported_for_the_nearer_one() {
        let errors = angles(&[at([1.6, 2.7], 3.1, 9.4)], &[0.0, 4.0, 4.0], &[], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].angle, 4.0);
        assert!(degrees(errors[0].deviation, 0.9), "{errors:?}");
    }

    #[test]
    fn the_absolute_angle_is_measured_from_the_ucs_x_axis() {
        let curves = [at([1.4, 3.6], 31.2, 6.3), at([-8.2, -5.7], 30.0, 4.9)];
        let errors = angles(&curves, &[0.0], &[], 30f64.to_radians());
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].curve, 0);
        assert!(degrees(errors[0].deviation, 1.2), "{errors:?}");
    }

    #[test]
    fn an_absolute_error_tells_which_ends_meet_other_geometry() {
        let wall = line([2.7, -6.1], [2.7, 9.8]);
        let leaning = at([2.7, 1.3], 177.9, 5.6);
        let errors = angles(&[wall, leaning], &[0.0], &[], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].curve, 1);
        assert_eq!(errors[0].meets, [true, false]);
    }

    /// Two lines from `start`: to a corner at `first` degrees, and on from
    /// it at `second` degrees.
    fn corner(start: [f64; 2], first: f64, second: f64) -> [Curve; 2] {
        let one = at(start, first, 9.1);
        let two = at(end_of(&one), second, 6.7);
        [one, two]
    }

    #[test]
    fn lines_that_meet_a_little_off_an_angle_are_a_relative_error() {
        let curves = corner([-4.3, 2.6], 23.0, 111.5);
        let errors = angles(&curves, &[], &[0.0, 90.0], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let error = errors[0];
        assert_eq!(error.kind, AngleErrorKind::Relative);
        assert_eq!((error.curve, error.other), (0, Some(1)));
        assert_eq!(error.angle, 90.0);
        assert!(near(error.location, end_of(&curves[0])), "{error:?}");
        assert!(degrees(error.deviation, 1.5), "{error:?}");
        // The first line turned by `turn` meets the second at 90°.
        assert!(degrees(error.turn.to_degrees(), -1.5), "{error:?}");
    }

    #[test]
    fn lines_that_cross_match_an_angle_or_its_supplement() {
        let one = at([1.3, -2.2], 17.0, 10.0);
        let middle = [
            1.3 + 5.0 * 17f64.to_radians().cos(),
            -2.2 + 5.0 * 17f64.to_radians().sin(),
        ];
        for (between, deviation) in [(46.3, 1.3), (134.0, 1.0)] {
            let direction = (17.0 + between) * std::f64::consts::PI / 180.0;
            let from = [
                middle[0] - 4.0 * direction.cos(),
                middle[1] - 4.0 * direction.sin(),
            ];
            let two = at(from, 17.0 + between, 8.0);
            let errors = angles(&[one.clone(), two], &[], &[45.0], 0.0);
            assert_eq!(errors.len(), 1, "{between}: {errors:?}");
            assert_eq!(errors[0].angle, 45.0);
            assert!(near(errors[0].location, middle), "{between}: {errors:?}");
            assert!(
                degrees(errors[0].deviation, deviation),
                "{between}: {errors:?}"
            );
        }
    }

    #[test]
    fn lines_that_do_not_meet_are_not_a_relative_error() {
        let [one, two] = corner([-4.3, 2.6], 23.0, 111.5);
        let Curve::Line(two) = two else {
            unreachable!()
        };
        let (sin, cos) = 111.5f64.to_radians().sin_cos();
        let apart = line(
            [two.start[0] + 0.01 * cos, two.start[1] + 0.01 * sin],
            two.end,
        );
        assert!(angles(&[one, apart], &[], &[90.0], 0.0).is_empty());
    }

    #[test]
    fn lines_that_cross_almost_along_each_other_are_a_relative_zero_error() {
        // The second line starts 0.2 to the right of the first and crosses
        // it 4.6 on.
        let one = at([-3.9, 1.1], 38.0, 11.0);
        let (sin, cos) = 38f64.to_radians().sin_cos();
        let two = at([-3.9 + cos + 0.2 * sin, 1.1 + sin - 0.2 * cos], 40.5, 9.0);
        let errors = angles(&[one, two], &[], &[0.0, 90.0], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].angle, 0.0);
        assert!(degrees(errors[0].deviation, 2.5), "{errors:?}");
    }

    #[test]
    fn a_type_with_no_angles_finds_nothing() {
        let curves = corner([-4.3, 2.6], 23.0, 111.5);
        assert!(angles(&curves, &[], &[], 0.0).is_empty());
        let errors = angles(&curves, &[], &[90.0, 90.0], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
    }

    #[test]
    fn arcs_have_no_angle_errors() {
        let bow = arc([4.1, -3.3], 2.6, 0.1, 1.6);
        assert!(angles(&[bow], &[90.0], &[90.0], 0.0).is_empty());
    }

    #[test]
    fn a_line_no_longer_than_the_weld_has_no_angle() {
        let speck = at([2.9, -6.1], 91.7, 5e-10);
        assert!(angles(&[speck], &[90.0], &[], 0.0).is_empty());
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
    /// A polyline from `start`: 20 along the X axis, then 10 at `degrees`.
    fn bent(start: [f64; 2], degrees: f64) -> (Curve, [[f64; 2]; 3]) {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let corner = [start[0] + 20.0, start[1]];
        let points = [
            start,
            corner,
            [corner[0] + 10.0 * cos, corner[1] + 10.0 * sin],
        ];
        (polyline(&points, false), points)
    }

    #[test]
    fn a_straight_polyline_segment_off_an_angle_is_an_absolute_error() {
        let (path, _) = bent([2.4, -3.1], 91.7);
        let errors = angles(&[path], &[0.0, 90.0], &[], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let error = errors[0];
        assert_eq!((error.curve, error.segment), (0, Some(1)));
        assert_eq!((error.other, error.other_segment), (None, None));
        assert!(degrees(error.angle, 90.0), "{error:?}");
        assert!(degrees(error.deviation, 1.7), "{error:?}");
        assert!(degrees(error.turn.to_degrees(), -1.7), "{error:?}");
    }

    #[test]
    fn segments_of_one_polyline_that_meet_off_an_angle_are_a_relative_error() {
        let (path, points) = bent([2.4, -3.1], 91.7);
        let errors = angles(&[path], &[], &[90.0], 0.0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let error = errors[0];
        assert_eq!(error.kind, AngleErrorKind::Relative);
        assert_eq!((error.curve, error.segment), (0, Some(0)));
        assert_eq!((error.other, error.other_segment), (Some(0), Some(1)));
        assert!(distance(error.location, points[1]) < 1e-9, "{error:?}");
        assert!(degrees(error.deviation, 1.7), "{error:?}");
    }

    #[test]
    fn a_segment_meets_the_segment_next_to_it() {
        let (path, _) = bent([2.4, -3.1], 91.7);
        let errors = angles(&[path], &[90.0], &[], 0.0);
        assert_eq!(errors[0].meets, [true, false], "{errors:?}");
    }

    #[test]
    fn the_closing_segment_of_a_closed_polyline_is_read() {
        let square = polyline(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.3, 10.0]], true);
        let errors = angles(&[square], &[0.0, 90.0], &[], 0.0);
        let segments: Vec<_> = errors.iter().map(|e| e.segment).collect();
        assert_eq!(segments, [Some(3)], "{errors:?}");
        assert!(degrees(
            errors[0].deviation,
            0.3f64.atan2(10.0).to_degrees()
        ));
    }

    #[test]
    fn an_arc_segment_of_a_polyline_has_no_angle_error() {
        let (sin, cos) = 91.7f64.to_radians().sin_cos();
        let path = Curve::Polyline(Polyline {
            vertices: vec![
                PolylineVertex {
                    position: [0.0, 0.0],
                    bulge: 0.0,
                },
                PolylineVertex {
                    position: [20.0, 0.0],
                    bulge: 0.2,
                },
                PolylineVertex {
                    position: [20.0 + 10.0 * cos, 10.0 * sin],
                    bulge: 0.0,
                },
            ],
            closed: false,
        });
        assert!(angles(&[path], &[0.0, 90.0], &[], 0.0).is_empty());
    }
    /// An elliptical arc on the ellipse with radii 10 (along X) and 5 at the
    /// origin, between two of its parameters.
    fn elliptical(start_parameter: f64, end_parameter: f64) -> Curve {
        Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [0.0, 0.0],
                major_radius: 10.0,
                minor_radius: 5.0,
                major_axis: [1.0, 0.0],
            },
            start_parameter,
            end_parameter,
        })
    }

    /// The length of that ellipse between two parameters, summed over many
    /// short chords: a check of the scan's own integration.
    fn chord_length(from: f64, to: f64) -> f64 {
        let point = |t: f64| [10.0 * t.cos(), 5.0 * t.sin()];
        let steps = 100_000;
        (0..steps)
            .map(|k| {
                let a = from + (to - from) * k as f64 / steps as f64;
                let b = from + (to - from) * (k + 1) as f64 / steps as f64;
                distance(point(a), point(b))
            })
            .sum()
    }

    #[test]
    fn the_length_of_an_elliptical_arc_is_measured_along_it() {
        use std::f64::consts::FRAC_PI_2;
        let Curve::Ellipse(quarter) = elliptical(0.0, FRAC_PI_2) else {
            unreachable!()
        };
        let length = shape_length(Shape::Ellipse(quarter));
        assert!(
            (length - chord_length(0.0, FRAC_PI_2)).abs() < 1e-6,
            "{length}"
        );
        // An arc that is not symmetric about an axis of the ellipse.
        let Curve::Ellipse(part) = elliptical(0.3, FRAC_PI_2) else {
            unreachable!()
        };
        let length = shape_length(Shape::Ellipse(part));
        assert!(
            (length - chord_length(0.3, FRAC_PI_2)).abs() < 1e-6,
            "{length}"
        );
    }

    #[test]
    fn a_thin_elliptical_arc_reaches_a_line_round_its_flattest_end() {
        // At the end of its major axis a thin ellipse turns most for each
        // unit of length: the continuation of the end at (2, 0) must turn
        // more than the tolerance in radians to reach the line.
        let arc = Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre: [0.0, 0.0],
                major_radius: 2.0,
                minor_radius: 0.5,
                major_axis: [1.0, 0.0],
            },
            start_parameter: 1.5 * PI,
            end_parameter: TAU,
        });
        let reached = 0.44f64;
        let target = [2.0 * reached.cos(), 0.5 * reached.sin()];
        let curves = [arc, line([0.0, 0.0], [2.0 * target[0], 2.0 * target[1]])];
        let errors = end_errors(&curves, Tolerances::all(0.4), Tolerance::new(1e-9));
        assert_eq!(errors.len(), 1, "{errors:?}");
        let error = errors[0];
        assert_eq!(error.kind, EndErrorKind::Undershoot);
        assert_eq!(
            error.end,
            CurveEnd {
                curve: 0,
                at_start: false
            }
        );
        assert!(distance(error.target, target) < 1e-6, "{error:?}");
        assert!(error.deviation < 0.4, "{error:?}");
    }

    #[test]
    fn an_elliptical_arc_that_stops_short_is_an_undershoot_along_its_ellipse() {
        use std::f64::consts::FRAC_PI_2;
        // The arc ends at (0, 5); a wall at x = -0.3 is just round the ellipse.
        let curves = [
            elliptical(0.0, FRAC_PI_2),
            line([-0.3, -10.0], [-0.3, 10.0]),
        ];
        let errors = end_errors(&curves, Tolerances::all(0.5), Tolerance::new(1e-9));
        assert_eq!(errors.len(), 1, "{errors:?}");
        let error = errors[0];
        assert_eq!(error.kind, EndErrorKind::Undershoot);
        assert_eq!(
            error.end,
            CurveEnd {
                curve: 0,
                at_start: false
            }
        );
        let reached = (-0.03f64).acos();
        assert!(
            distance(error.target, [-0.3, 5.0 * reached.sin()]) < 1e-6,
            "{error:?}"
        );
        assert!(
            (error.deviation - chord_length(FRAC_PI_2, reached)).abs() < 1e-6,
            "{error:?}"
        );
    }

    #[test]
    fn an_elliptical_arc_that_runs_past_a_line_is_an_overshoot() {
        use std::f64::consts::FRAC_PI_2;
        let curves = [
            elliptical(0.0, FRAC_PI_2 + 0.03),
            line([0.0, -10.0], [0.0, 10.0]),
        ];
        let errors = end_errors(&curves, Tolerances::all(0.5), Tolerance::new(1e-9));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, EndErrorKind::Overshoot);
        assert!(distance(errors[0].target, [0.0, 5.0]) < 1e-6, "{errors:?}");
        assert!(
            (errors[0].deviation - chord_length(FRAC_PI_2, FRAC_PI_2 + 0.03)).abs() < 1e-6,
            "{errors:?}"
        );
    }

    #[test]
    fn an_elliptical_arc_end_near_another_end_is_a_gap() {
        let curves = [elliptical(0.0, 1.0), line([10.2, 0.0], [20.0, 0.0])];
        let errors = end_errors(&curves, Tolerances::all(0.5), Tolerance::new(1e-9));
        assert_eq!(errors.len(), 1, "{errors:?}");
        let error = errors[0];
        assert_eq!(error.kind, EndErrorKind::Gap);
        assert_eq!(
            error.end,
            CurveEnd {
                curve: 0,
                at_start: true
            }
        );
        assert!((error.deviation - 0.2).abs() < 1e-9, "{error:?}");
        assert_eq!(error.corner, None, "an elliptical end has no corner");
    }

    #[test]
    fn a_whole_ellipse_is_not_read() {
        let curves = [elliptical(0.0, TAU), line([10.2, 0.0], [20.0, 0.0])];
        assert!(end_errors(&curves, Tolerances::all(0.5), Tolerance::new(1e-9)).is_empty());
    }

    #[test]
    fn an_elliptical_arc_that_wraps_ends_at_its_end_parameter() {
        use std::f64::consts::FRAC_PI_2;
        // From 3π/2 round through 0 to π/2: it ends at (0, 5).
        let curves = [
            elliptical(3.0 * FRAC_PI_2, FRAC_PI_2),
            line([-0.3, -10.0], [-0.3, 10.0]),
        ];
        let errors = end_errors(&curves, Tolerances::all(0.5), Tolerance::new(1e-9));
        assert_eq!(errors.len(), 2, "{errors:?}");
        for error in &errors {
            assert_eq!(error.kind, EndErrorKind::Undershoot);
            let expected = if error.end.at_start { -5.0 } else { 5.0 };
            assert!(
                distance(error.location, [0.0, expected]) < 1e-9,
                "{error:?}"
            );
        }
    }
}
