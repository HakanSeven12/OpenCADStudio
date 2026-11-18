//! Turning a heap of segments into the regions they enclose.
//!
//! Given segments that cross each other anywhere — not just at their ends —
//! this finds every bounded region they close off. It is what answers "the
//! user clicked inside this shape, what is the shape" for a hatch, and it is
//! the same question a B-rep boolean asks in a face's `(u, v)` space once the
//! intersection curves have been projected into it: the two surfaces' loops
//! and the new curves cut each other up, and what comes out is a set of
//! regions to keep or discard.
//!
//! # How
//!
//! Three stages, each with a reason it cannot be skipped.
//!
//! **Split.** Every crossing becomes a vertex, so segments only ever meet end
//! to end afterwards. Found by a sweep over the less congested axis: sorting
//! by one coordinate and keeping an active list turns the all-pairs test into
//! an output-sensitive one, which matters because a drawing's segments are
//! usually spread out and rarely all overlap.
//!
//! **Weld.** Endpoints that land within tolerance of each other become one
//! vertex. Without this a boundary that *looks* closed on screen has a gap of
//! a rounding in it and no face is found at all — the single most common way
//! this kind of search fails.
//!
//! **Trace.** Each undirected edge is two directed half-edges. Walking one and
//! always taking the next neighbour clockwise around the far vertex traces
//! exactly one face, and every half-edge belongs to exactly one. The traversal
//! that comes back clockwise is the unbounded outside, which is why the
//! orientation is what selects the bounded ones.
//!
//! # Tolerance
//!
//! Passed in rather than fixed here. What counts as "the same point" depends
//! on what produced the segments — a tessellation's own chord tolerance, a
//! drawing's units, a face's parameter scale — and a constant chosen for one
//! of those is wrong for the others.

use super::curve::Line;
use super::vec::Vec2;
use super::Tolerance;
use std::collections::{HashMap, HashSet};

/// How nearly parallel two segments must be before they are treated as
/// collinear, measured on the sine of the angle between them.
///
/// Relative, not absolute: the cross product it is compared against scales
/// with both lengths, so a fixed floor would call every pair of long segments
/// parallel.
const PARALLEL_EPS: f64 = 1.0e-12;

/// The smallest signed area a traced ring may have and still count as a
/// region rather than as the fold-back of a spur.
const AREA_EPS: f64 = 1.0e-10;

/// How two segments meet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegmentCrossing {
    /// They do not.
    None,
    /// At one place, at these parameters along each.
    Point {
        /// Parameter along the first segment.
        a: f64,
        /// Parameter along the second.
        b: f64,
    },
    /// Along a shared stretch, because they are collinear. The intervals are
    /// the same stretch expressed on each segment, so `b` may run backwards.
    Overlap {
        /// Start and end along the first segment.
        a: [f64; 2],
        /// The same stretch along the second.
        b: [f64; 2],
    },
}

/// Every bounded region the segments enclose, each as a closed ring of points
/// running counter-clockwise.
///
/// Segments need not be closed, ordered, or connected. What they enclose
/// between them is what comes back, so a hatch boundary can be picked out of
/// a pile of unrelated lines and arcs that merely happen to cross.
///
/// `tolerance` is the distance below which two points are the same one.
pub fn bounded_faces(segments: &[Line], tolerance: Tolerance) -> Vec<Vec<[f64; 2]>> {
    if segments.is_empty() {
        return Vec::new();
    }
    let tolerance = tolerance.linear();
    let pieces = split_at_crossings(segments, tolerance);
    let graph = Graph::weld(&pieces, tolerance);
    graph.trace_faces()
}

/// Straight bridges that close small gaps between loose segment ends, for
/// a boundary search that should treat a nearly closed outline as closed.
///
/// An end is loose when no other segment passes within `tolerance` of it.
/// Loose ends of different segments at most `gap` apart are joined, the
/// closest pairs first, each end at most once. Returns only the bridges;
/// pass them to [`bounded_faces`] together with `segments`. A segment that
/// stops short of the middle of another (a T gap) gets no bridge.
// ponytail: the loose-end test scans every segment, O(ends × segments);
// index the segments by cell if very large drawings make it matter.
pub fn gap_bridges(segments: &[Line], gap: f64, tolerance: Tolerance) -> Vec<Line> {
    let weld = tolerance.linear();
    if !(gap > weld) || !gap.is_finite() {
        return Vec::new();
    }
    type Grid = HashMap<(i64, i64), Vec<usize>>;
    let cell = |p: [f64; 2]| ((p[0] / gap).floor() as i64, (p[1] / gap).floor() as i64);
    let around = |grid: &Grid, p: [f64; 2]| {
        let (x, y) = cell(p);
        (x - 1..=x + 1)
            .flat_map(move |x| (y - 1..=y + 1).map(move |y| (x, y)))
            .filter_map(|key| grid.get(&key))
            .flatten()
            .copied()
            .collect::<Vec<_>>()
    };
    let distance = |a: [f64; 2], b: [f64; 2]| Vec2::from(a).distance(Vec2::from(b));
    // Segment end `k` is `segments[k / 2]`, its start when `k` is even.
    let end = |k: usize| {
        let segment = segments[k / 2];
        if k % 2 == 0 {
            segment.start
        } else {
            segment.end
        }
    };
    let mut ends = Grid::default();
    for k in 0..segments.len() * 2 {
        ends.entry(cell(end(k))).or_default().push(k);
    }
    let loose: Vec<usize> = (0..segments.len() * 2)
        .filter(|&k| {
            let p = end(k);
            // Cheap test first: most ends meet another segment's end.
            !around(&ends, p)
                .into_iter()
                .any(|other| other / 2 != k / 2 && distance(p, end(other)) <= weld)
                && !segments.iter().enumerate().any(|(index, segment)| {
                    index != k / 2
                        && Vec2::from(p)
                            .distance_to_segment(Vec2::from(segment.start), Vec2::from(segment.end))
                            <= weld
                })
        })
        .collect();
    let mut loose_grid = Grid::default();
    for (index, &k) in loose.iter().enumerate() {
        loose_grid.entry(cell(end(k))).or_default().push(index);
    }
    let mut pairs = Vec::new();
    for (a, &k) in loose.iter().enumerate() {
        for b in around(&loose_grid, end(k)) {
            let length = distance(end(k), end(loose[b]));
            if b > a && loose[b] / 2 != k / 2 && length <= gap {
                pairs.push((length, a, b));
            }
        }
    }
    pairs.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut bridged = vec![false; loose.len()];
    pairs
        .into_iter()
        .filter_map(|(_, a, b)| {
            if bridged[a] || bridged[b] {
                return None;
            }
            bridged[a] = true;
            bridged[b] = true;
            Some(Line {
                start: end(loose[a]),
                end: end(loose[b]),
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
pub struct TaggedLine {
    pub line: Line,
    pub tag: usize,
}

pub fn bounded_face_edges(
    segments: &[TaggedLine],
    tolerance: Tolerance,
) -> Vec<Vec<TaggedLine>> {
    if segments.is_empty() {
        return Vec::new();
    }
    let tolerance = tolerance.linear();
    let pieces = split_tagged_at_crossings(segments, tolerance);
    TaggedGraph::weld(&pieces, tolerance).trace_faces()
}

/// The signed area a ring encloses: positive counter-clockwise.
///
/// Summed about the ring's own first point rather than about the origin. At
/// survey coordinates the terms of the origin form are each around 10¹² and
/// cancel to something around 10², which is most of the significant digits
/// spent before the answer appears.
pub fn signed_area(ring: &[[f64; 2]]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let origin = Vec2::from(ring[0]);
    let mut total = 0.0;
    for pair in ring[1..].windows(2) {
        let a = Vec2::from(pair[0]) - origin;
        let b = Vec2::from(pair[1]) - origin;
        total += a.cross(b);
    }
    total * 0.5
}

/// Where two segments meet, as parameters along each.
///
/// Reports a shared stretch rather than a single point when they are
/// collinear and overlap, which a curve-level intersection does not: two
/// coincident edges have no one crossing, and picking a point from the
/// stretch would leave the rest of the overlap unsplit.
pub fn segment_crossing(a: Line, b: Line, tolerance: Tolerance) -> SegmentCrossing {
    let tolerance = tolerance.linear();
    let (a_start, a_end) = (Vec2::from(a.start), Vec2::from(a.end));
    let (b_start, b_end) = (Vec2::from(b.start), Vec2::from(b.end));
    let along_a = a_end - a_start;
    let along_b = b_end - b_start;
    let (a_squared, b_squared) = (along_a.length_squared(), along_b.length_squared());
    if a_squared <= f64::EPSILON || b_squared <= f64::EPSILON {
        return SegmentCrossing::None;
    }
    let (a_len, b_len) = (a_squared.sqrt(), b_squared.sqrt());
    let turn = along_a.cross(along_b);
    let offset = b_start - a_start;

    if turn.abs() <= PARALLEL_EPS * a_len * b_len {
        // Parallel. Collinear only if the offset lies along them too.
        if offset.cross(along_a).abs() > tolerance * a_len {
            return SegmentCrossing::None;
        }
        let from = offset.dot(along_a) / a_squared;
        let to = from + along_b.dot(along_a) / a_squared;
        let low = from.min(to).max(0.0);
        let high = from.max(to).min(1.0);
        if high < low - tolerance / a_len {
            return SegmentCrossing::None;
        }
        let (low, high) = (low.clamp(0.0, 1.0), high.clamp(0.0, 1.0));
        let on_b = |t: f64| (a_start.lerp(a_end, t) - b_start).dot(along_b) / b_squared;
        let (b_low, b_high) = (on_b(low), on_b(high));
        if (high - low) * a_len <= tolerance {
            // They touch end to end rather than sharing a stretch.
            return SegmentCrossing::Point {
                a: (low + high) * 0.5,
                b: (b_low + b_high) * 0.5,
            };
        }
        return SegmentCrossing::Overlap {
            a: [low, high],
            b: [b_low, b_high],
        };
    }

    let t = offset.cross(along_b) / turn;
    let u = offset.cross(along_a) / turn;
    // The slack is a distance turned into a parameter, so each segment gets
    // its own — a short one tolerates proportionally more.
    let (t_slack, u_slack) = (tolerance / a_len, tolerance / b_len);
    if (-t_slack..=1.0 + t_slack).contains(&t) && (-u_slack..=1.0 + u_slack).contains(&u) {
        SegmentCrossing::Point { a: t, b: u }
    } else {
        SegmentCrossing::None
    }
}

/// The axis-aligned box a segment occupies.
#[derive(Clone, Copy)]
struct Bounds {
    min: Vec2,
    max: Vec2,
}

impl Bounds {
    fn of(segment: Line) -> Self {
        let (a, b) = (Vec2::from(segment.start), Vec2::from(segment.end));
        Self {
            min: Vec2::new(a.x.min(b.x), a.y.min(b.y)),
            max: Vec2::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    fn overlaps(self, other: Self, slack: f64) -> bool {
        self.max.x + slack >= other.min.x
            && other.max.x + slack >= self.min.x
            && self.max.y + slack >= other.min.y
            && other.max.y + slack >= self.min.y
    }
}

/// Splits every segment at every crossing, so afterwards they meet only end
/// to end.
pub(super) fn split_at_crossings(segments: &[Line], tolerance: f64) -> Vec<Line> {
    let bounds: Vec<Bounds> = segments.iter().copied().map(Bounds::of).collect();
    let vertical = sweep_on_y(&bounds, tolerance);
    let key = |b: &Bounds| if vertical { b.min.y } else { b.min.x };
    let far = |b: &Bounds| if vertical { b.max.y } else { b.max.x };

    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by(|&a, &b| key(&bounds[a]).total_cmp(&key(&bounds[b])));

    let mut cuts: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; segments.len()];
    let mut active: Vec<usize> = Vec::new();
    for index in order {
        let front = key(&bounds[index]);
        active.retain(|&other| far(&bounds[other]) + tolerance >= front);
        for &other in &active {
            if !bounds[index].overlaps(bounds[other], tolerance) {
                continue;
            }
            match segment_crossing(segments[index], segments[other], Tolerance::new(tolerance)) {
                SegmentCrossing::None => {}
                SegmentCrossing::Point { a, b } => {
                    cuts[index].push(a.clamp(0.0, 1.0));
                    cuts[other].push(b.clamp(0.0, 1.0));
                }
                SegmentCrossing::Overlap { a, b } => {
                    cuts[index].extend(a.iter().map(|t| t.clamp(0.0, 1.0)));
                    cuts[other].extend(b.iter().map(|t| t.clamp(0.0, 1.0)));
                }
            }
        }
        active.push(index);
    }

    let mut pieces = Vec::new();
    for (segment, params) in segments.iter().copied().zip(cuts.iter_mut()) {
        let (start, end) = (Vec2::from(segment.start), Vec2::from(segment.end));
        let length = start.distance(end);
        if length <= tolerance {
            continue;
        }
        let merge = (tolerance / length).min(1.0);
        params.sort_by(|a, b| a.total_cmp(b));
        params.dedup_by(|a, b| (*a - *b).abs() <= merge);
        for pair in params.windows(2) {
            if (pair[1] - pair[0]) * length <= tolerance {
                continue;
            }
            let (a, b) = (start.lerp(end, pair[0]), start.lerp(end, pair[1]));
            if a.distance(b) > tolerance {
                pieces.push(Line {
                    start: a.to_array(),
                    end: b.to_array(),
                });
            }
        }
    }
    pieces
}

fn split_tagged_at_crossings(segments: &[TaggedLine], tolerance: f64) -> Vec<TaggedLine> {
    let lines: Vec<Line> = segments.iter().map(|segment| segment.line).collect();
    let bounds: Vec<Bounds> = lines.iter().copied().map(Bounds::of).collect();
    let vertical = sweep_on_y(&bounds, tolerance);
    let key = |bounds: &Bounds| {
        if vertical {
            bounds.min.y
        } else {
            bounds.min.x
        }
    };
    let far = |bounds: &Bounds| {
        if vertical {
            bounds.max.y
        } else {
            bounds.max.x
        }
    };
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| key(&bounds[a]).total_cmp(&key(&bounds[b])));
    let mut cuts: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; lines.len()];
    let mut active: Vec<usize> = Vec::new();
    for index in order {
        let front = key(&bounds[index]);
        active.retain(|&other| far(&bounds[other]) + tolerance >= front);
        for &other in &active {
            if !bounds[index].overlaps(bounds[other], tolerance) {
                continue;
            }
            match segment_crossing(lines[index], lines[other], Tolerance::new(tolerance)) {
                SegmentCrossing::None => {}
                SegmentCrossing::Point { a, b } => {
                    cuts[index].push(a.clamp(0.0, 1.0));
                    cuts[other].push(b.clamp(0.0, 1.0));
                }
                SegmentCrossing::Overlap { a, b } => {
                    cuts[index].extend(a.map(|value| value.clamp(0.0, 1.0)));
                    cuts[other].extend(b.map(|value| value.clamp(0.0, 1.0)));
                }
            }
        }
        active.push(index);
    }

    let mut pieces = Vec::new();
    for (index, params) in cuts.iter_mut().enumerate() {
        let line = lines[index];
        let start = Vec2::from(line.start);
        let end = Vec2::from(line.end);
        let length = start.distance(end);
        if length <= tolerance {
            continue;
        }
        let merge = (tolerance / length).min(1.0);
        params.sort_by(f64::total_cmp);
        params.dedup_by(|a, b| (*a - *b).abs() <= merge);
        for pair in params.windows(2) {
            let a = start.lerp(end, pair[0]);
            let b = start.lerp(end, pair[1]);
            if a.distance(b) > tolerance {
                pieces.push(TaggedLine {
                    line: Line {
                        start: a.to_array(),
                        end: b.to_array(),
                    },
                    tag: segments[index].tag,
                });
            }
        }
    }
    pieces
}

/// Which axis to sweep along: the one whose intervals overlap least, since
/// that is the one where the active list stays short.
fn sweep_on_y(bounds: &[Bounds], tolerance: f64) -> bool {
    let congestion = |vertical: bool| {
        let (low, high) = bounds.iter().fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(low, high), b| {
                let (min, max) = if vertical {
                    (b.min.y, b.max.y)
                } else {
                    (b.min.x, b.max.x)
                };
                (low.min(min), high.max(max))
            },
        );
        let extent = high - low;
        if !extent.is_finite() || extent <= tolerance {
            return f64::INFINITY;
        }
        let total: f64 = bounds
            .iter()
            .map(|b| {
                if vertical {
                    b.max.y - b.min.y
                } else {
                    b.max.x - b.min.x
                }
            })
            .map(|span| span + tolerance)
            .sum();
        total / extent
    };
    congestion(false) > congestion(true)
}

/// The welded planar graph: vertices, and who each is joined to.
struct Graph {
    vertices: Vec<Vec2>,
    /// Neighbours of each vertex, sorted counter-clockwise about it.
    neighbours: Vec<Vec<usize>>,
    edge_count: usize,
}

impl Graph {
    /// Welds coincident endpoints into shared vertices and builds the
    /// adjacency, with each vertex's neighbours sorted by angle — which is
    /// what makes the half-edge walk below a planar traversal rather than an
    /// arbitrary one.
    fn weld(pieces: &[Line], tolerance: f64) -> Self {
        let mut vertices: Vec<Vec2> = Vec::new();
        // Buckets a tolerance wide, so a lookup only has to search the nine
        // around a point rather than every vertex placed so far.
        let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        let mut edges: HashSet<(usize, usize)> = HashSet::new();

        for piece in pieces {
            let a = weld_point(
                Vec2::from(piece.start),
                &mut vertices,
                &mut buckets,
                tolerance,
            );
            let b = weld_point(Vec2::from(piece.end), &mut vertices, &mut buckets, tolerance);
            if a != b {
                edges.insert(if a < b { (a, b) } else { (b, a) });
            }
        }

        let mut neighbours = vec![Vec::new(); vertices.len()];
        for &(a, b) in &edges {
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
        for (vertex, list) in neighbours.iter_mut().enumerate() {
            let origin = vertices[vertex];
            list.sort_by(|&a, &b| {
                (vertices[a] - origin)
                    .angle()
                    .total_cmp(&(vertices[b] - origin).angle())
            });
            list.dedup();
        }
        Self {
            vertices,
            neighbours,
            edge_count: edges.len(),
        }
    }

    fn trace_faces(&self) -> Vec<Vec<[f64; 2]>> {
        if self.edge_count == 0 {
            return Vec::new();
        }
        let mut walked: HashSet<(usize, usize)> = HashSet::new();
        let mut faces = Vec::new();
        // A face cannot use more directed edges than the graph has, which
        // also bounds the walk if the input is malformed.
        let limit = self.edge_count * 2 + 1;

        for from in 0..self.vertices.len() {
            for &to in &self.neighbours[from] {
                if walked.contains(&(from, to)) {
                    continue;
                }
                let start = (from, to);
                let mut current = start;
                let mut ring: Vec<[f64; 2]> = Vec::new();
                let mut closed = false;

                for _ in 0..limit {
                    if !walked.insert(current) {
                        break;
                    }
                    let (tail, head) = current;
                    ring.push(self.vertices[tail].to_array());
                    let around = &self.neighbours[head];
                    let Some(incoming) = around.iter().position(|&n| n == tail) else {
                        break;
                    };
                    // Neighbours run counter-clockwise, so stepping back one
                    // takes the sharpest right turn — which keeps the face
                    // being traced on the left of every half-edge.
                    let next = around[if incoming == 0 {
                        around.len() - 1
                    } else {
                        incoming - 1
                    }];
                    current = (head, next);
                    if current == start {
                        closed = true;
                        break;
                    }
                }

                // The reverse traversal of the same edges walks the outside,
                // which comes back clockwise. Orientation is what tells the
                // two apart.
                if closed && ring.len() >= 3 && signed_area(&ring) > AREA_EPS {
                    faces.push(ring);
                }
            }
        }
        faces
    }
}

struct TaggedGraph {
    vertices: Vec<Vec2>,
    neighbours: Vec<Vec<usize>>,
    edges: HashMap<(usize, usize), usize>,
}

impl TaggedGraph {
    fn weld(pieces: &[TaggedLine], tolerance: f64) -> Self {
        let mut vertices = Vec::new();
        let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        let mut edges = HashMap::new();
        for piece in pieces {
            let a = weld_point(
                Vec2::from(piece.line.start),
                &mut vertices,
                &mut buckets,
                tolerance,
            );
            let b = weld_point(
                Vec2::from(piece.line.end),
                &mut vertices,
                &mut buckets,
                tolerance,
            );
            if a != b {
                edges.entry(ordered_edge(a, b)).or_insert(piece.tag);
            }
        }
        let mut neighbours = vec![Vec::new(); vertices.len()];
        for &(a, b) in edges.keys() {
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
        for (vertex, list) in neighbours.iter_mut().enumerate() {
            let origin = vertices[vertex];
            list.sort_by(|&a, &b| {
                (vertices[a] - origin)
                    .angle()
                    .total_cmp(&(vertices[b] - origin).angle())
            });
            list.dedup();
        }
        Self {
            vertices,
            neighbours,
            edges,
        }
    }

    fn trace_faces(&self) -> Vec<Vec<TaggedLine>> {
        let mut walked = HashSet::new();
        let mut faces = Vec::new();
        let limit = self.edges.len() * 2 + 1;
        for from in 0..self.vertices.len() {
            for &to in &self.neighbours[from] {
                if walked.contains(&(from, to)) {
                    continue;
                }
                let start = (from, to);
                let mut current = start;
                let mut ring = Vec::new();
                let mut closed = false;
                for _ in 0..limit {
                    if !walked.insert(current) {
                        break;
                    }
                    let (tail, head) = current;
                    ring.push(TaggedLine {
                        line: Line {
                            start: self.vertices[tail].to_array(),
                            end: self.vertices[head].to_array(),
                        },
                        tag: self.edges[&ordered_edge(tail, head)],
                    });
                    let around = &self.neighbours[head];
                    let Some(incoming) = around.iter().position(|&vertex| vertex == tail) else {
                        break;
                    };
                    let next = around[if incoming == 0 {
                        around.len() - 1
                    } else {
                        incoming - 1
                    }];
                    current = (head, next);
                    if current == start {
                        closed = true;
                        break;
                    }
                }
                let points: Vec<[f64; 2]> = ring.iter().map(|edge| edge.line.start).collect();
                if closed && points.len() >= 3 && signed_area(&points) > AREA_EPS {
                    faces.push(ring);
                }
            }
        }
        faces
    }
}

fn ordered_edge(a: usize, b: usize) -> (usize, usize) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

/// The index of the vertex at `point`, creating one if nothing is already
/// there within `tolerance`.
fn weld_point(
    point: Vec2,
    vertices: &mut Vec<Vec2>,
    buckets: &mut HashMap<(i64, i64), Vec<usize>>,
    tolerance: f64,
) -> usize {
    let key = |p: Vec2| {
        (
            (p.x / tolerance).floor() as i64,
            (p.y / tolerance).floor() as i64,
        )
    };
    let cell = key(point);
    for dx in -1..=1 {
        for dy in -1..=1 {
            let neighbour = (cell.0.saturating_add(dx), cell.1.saturating_add(dy));
            if let Some(indices) = buckets.get(&neighbour) {
                if let Some(&found) = indices
                    .iter()
                    .find(|&&i| vertices[i].distance(point) <= tolerance)
                {
                    return found;
                }
            }
        }
    }
    let index = vertices.len();
    vertices.push(point);
    buckets.entry(cell).or_default().push(index);
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_bridges_close_only_gaps_within_reach() {
        // A square whose top-left corner is open by 0.5.
        let line = |start: [f64; 2], end: [f64; 2]| Line { start, end };
        let square = [
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.0, 0.0], [10.0, 10.0]),
            line([10.0, 10.0], [0.5, 10.0]),
            line([0.0, 10.0], [0.0, 0.0]),
        ];
        let tolerance = Tolerance::new(1e-6);
        assert!(gap_bridges(&square, 0.4, tolerance).is_empty());
        let bridges = gap_bridges(&square, 0.6, tolerance);
        assert_eq!(bridges.len(), 1);
        let mut closed = square.to_vec();
        closed.extend(bridges);
        assert_eq!(bounded_faces(&closed, tolerance).len(), 1);
    }

    fn tol() -> Tolerance {
        Tolerance::new(1.0e-6)
    }

    fn line(a: [f64; 2], b: [f64; 2]) -> Line {
        Line { start: a, end: b }
    }

    /// A closed rectangle, given as four separate segments.
    fn rectangle(width: f64, height: f64) -> Vec<Line> {
        vec![
            line([0.0, 0.0], [width, 0.0]),
            line([width, 0.0], [width, height]),
            line([width, height], [0.0, height]),
            line([0.0, height], [0.0, 0.0]),
        ]
    }

    #[test]
    fn four_segments_that_meet_enclose_one_region() {
        let faces = bounded_faces(&rectangle(10.0, 5.0), tol());
        assert_eq!(faces.len(), 1);
        assert!((signed_area(&faces[0]) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_ring_comes_back_counter_clockwise_whichever_way_it_was_given() {
        let mut reversed: Vec<Line> = rectangle(4.0, 3.0)
            .into_iter()
            .map(|l| line(l.end, l.start))
            .collect();
        reversed.reverse();
        let faces = bounded_faces(&reversed, tol());
        assert_eq!(faces.len(), 1);
        assert!(signed_area(&faces[0]) > 0.0, "traced the outside");
    }

    #[test]
    fn segments_that_merely_cross_still_enclose_what_they_bound() {
        // A hash: two horizontals and two verticals, none of them touching
        // end to end. The middle square is the only bounded region.
        let segments = vec![
            line([-1.0, 0.0], [11.0, 0.0]),
            line([-1.0, 10.0], [11.0, 10.0]),
            line([0.0, -1.0], [0.0, 11.0]),
            line([10.0, -1.0], [10.0, 11.0]),
        ];
        let faces = bounded_faces(&segments, tol());
        assert_eq!(faces.len(), 1, "{faces:?}");
        assert!((signed_area(&faces[0]) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn two_regions_sharing_an_edge_are_both_found() {
        let segments = vec![
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.0, 0.0], [10.0, 10.0]),
            line([10.0, 10.0], [0.0, 10.0]),
            line([0.0, 10.0], [0.0, 0.0]),
            // The divider.
            line([5.0, 0.0], [5.0, 10.0]),
        ];
        let mut areas: Vec<f64> = bounded_faces(&segments, tol())
            .iter()
            .map(|ring| signed_area(ring))
            .collect();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas.len(), 2, "{areas:?}");
        assert!((areas[0] - 50.0).abs() < 1e-9 && (areas[1] - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_gap_smaller_than_the_tolerance_is_welded_shut() {
        // The failure this guards: a boundary that looks closed but whose
        // corner misses by a rounding, so nothing is found at all.
        let gap = 1e-9;
        let segments = vec![
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.0, gap], [10.0, 10.0]),
            line([10.0, 10.0], [0.0, 10.0]),
            line([0.0, 10.0], [0.0, 0.0]),
        ];
        assert_eq!(bounded_faces(&segments, tol()).len(), 1);
        // With a tolerance below the gap there is genuinely nothing closed.
        assert!(bounded_faces(&segments, Tolerance::new(1e-12)).is_empty());
    }

    #[test]
    fn an_open_chain_encloses_nothing() {
        let segments = vec![
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.0, 0.0], [10.0, 10.0]),
        ];
        assert!(bounded_faces(&segments, tol()).is_empty());
    }

    #[test]
    fn a_spur_hanging_off_a_ring_does_not_become_its_own_face() {
        let mut segments = rectangle(10.0, 10.0);
        segments.push(line([5.0, 10.0], [5.0, 15.0]));
        let faces = bounded_faces(&segments, tol());
        assert_eq!(faces.len(), 1, "{faces:?}");
        assert!((signed_area(&faces[0]) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn collinear_overlap_is_reported_as_a_stretch_not_a_point() {
        let crossing = segment_crossing(
            line([0.0, 0.0], [10.0, 0.0]),
            line([4.0, 0.0], [14.0, 0.0]),
            tol(),
        );
        match crossing {
            SegmentCrossing::Overlap { a, b } => {
                assert!((a[0] - 0.4).abs() < 1e-12 && (a[1] - 1.0).abs() < 1e-12, "{a:?}");
                assert!((b[0]).abs() < 1e-12 && (b[1] - 0.6).abs() < 1e-12, "{b:?}");
            }
            other => panic!("expected an overlap, got {other:?}"),
        }
    }

    #[test]
    fn segments_touching_end_to_end_report_one_point() {
        let crossing = segment_crossing(
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.0, 0.0], [20.0, 0.0]),
            tol(),
        );
        assert!(
            matches!(crossing, SegmentCrossing::Point { a, .. } if (a - 1.0).abs() < 1e-9),
            "{crossing:?}"
        );
    }

    #[test]
    fn parallel_segments_apart_do_not_meet() {
        assert_eq!(
            segment_crossing(
                line([0.0, 0.0], [10.0, 0.0]),
                line([0.0, 5.0], [10.0, 5.0]),
                tol()
            ),
            SegmentCrossing::None
        );
    }

    #[test]
    fn a_crossing_is_found_at_the_right_place_on_both() {
        let crossing = segment_crossing(
            line([0.0, 0.0], [10.0, 0.0]),
            line([2.0, -1.0], [2.0, 1.0]),
            tol(),
        );
        match crossing {
            SegmentCrossing::Point { a, b } => {
                assert!((a - 0.2).abs() < 1e-12 && (b - 0.5).abs() < 1e-12);
            }
            other => panic!("expected a point, got {other:?}"),
        }
    }

    #[test]
    fn signed_area_is_stable_at_survey_coordinates() {
        // The reason it sums about the ring's own first point: about the
        // origin the terms here are around 10¹² and cancel to 10².
        let origin = [512_345.678, 4_512_345.678];
        let ring: Vec<[f64; 2]> = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]
            .iter()
            .map(|p| [origin[0] + p[0], origin[1] + p[1]])
            .collect();
        assert!((signed_area(&ring) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn a_region_found_at_survey_coordinates_is_the_same_region() {
        let origin = [512_345.678, 4_512_345.678];
        let shifted: Vec<Line> = rectangle(10.0, 5.0)
            .into_iter()
            .map(|l| {
                line(
                    [origin[0] + l.start[0], origin[1] + l.start[1]],
                    [origin[0] + l.end[0], origin[1] + l.end[1]],
                )
            })
            .collect();
        let faces = bounded_faces(&shifted, tol());
        assert_eq!(faces.len(), 1);
        assert!((signed_area(&faces[0]) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn a_cut_on_a_very_long_segment_keeps_its_world_scale() {
        // The trap in merging cut parameters: a tolerance is a distance, so
        // on a segment 10¹² long it is a parameter of 10⁻¹². Merging on a
        // fixed parameter epsilon would collapse the two cuts a unit apart
        // into one and the small region between them would vanish.
        let segments = vec![
            line([0.0, 0.0], [1.0e12, 0.0]),
            line([0.0, 1.0], [1.0e12, 1.0]),
            line([1.0, -1.0], [1.0, 2.0]),
            line([2.0, -1.0], [2.0, 2.0]),
        ];
        let faces = bounded_faces(&segments, tol());
        assert_eq!(faces.len(), 1, "{faces:?}");
        assert!((signed_area(&faces[0]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn two_collinear_runs_that_overlap_still_close_a_ring() {
        // The bottom edge arrives as two pieces that share a stretch rather
        // than meeting end to end — which is what a hatch boundary picked out
        // of overlapping drawn lines looks like.
        let segments = vec![
            line([0.0, 0.0], [7.0, 0.0]),
            line([3.0, 0.0], [10.0, 0.0]),
            line([10.0, 0.0], [10.0, 10.0]),
            line([10.0, 10.0], [0.0, 10.0]),
            line([0.0, 10.0], [0.0, 0.0]),
        ];
        let faces = bounded_faces(&segments, tol());
        assert_eq!(faces.len(), 1, "{faces:?}");
        assert!((signed_area(&faces[0]) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn near_collinear_segments_a_long_way_out_still_read_as_overlapping() {
        // Nearly parallel and nearly touching, over a billion units. The
        // parallel test has to scale with both lengths or this reads as a
        // crossing at some absurd parameter.
        let a = line([0.0, 0.0], [1.0e9, 1.0e-3]);
        let b = line([5.0e8, 5.0e-4 + 5.0e-7], [1.5e9, 1.5e-3 + 5.0e-7]);
        assert!(
            matches!(segment_crossing(a, b, tol()), SegmentCrossing::Overlap { .. }),
            "{:?}",
            segment_crossing(a, b, tol())
        );
    }

    #[test]
    fn a_tolerance_finer_than_the_geometry_still_finds_a_clean_ring() {
        // `Tolerance` refuses a non-positive value at construction, so the
        // degenerate case cannot reach here; what can is one far below what
        // the geometry needs, and exact input still closes.
        let faces = bounded_faces(&rectangle(10.0, 10.0), Tolerance::new(1e-15));
        assert_eq!(faces.len(), 1);
        assert!((signed_area(&faces[0]) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn collapsed_segments_are_dropped_rather_than_welded_into_knots() {
        let mut segments = rectangle(10.0, 10.0);
        segments.push(line([3.0, 3.0], [3.0, 3.0]));
        assert_eq!(bounded_faces(&segments, tol()).len(), 1);
    }
}
