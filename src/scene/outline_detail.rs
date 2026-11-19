//! The edge outline that highlights a solid, without the detail too small
//! for the view to show.
//!
//! A solid's outline is a 2 px segment for every piece of every tessellated
//! edge, and each of them is drawn on every frame: a circular edge of 64
//! pieces is 64 segments whether it spans the view or two pixels of it. A
//! drawing of tubes and fittings is mostly such edges, and with thousands of
//! them selected their outlines are most of the frame (#1872).
//!
//! Under an orthographic view the size an edge shows at is known when the
//! outline is built. An edge line that covers no more than [`SMALL_LINE`]
//! pixels is drawn with the points that size can tell apart; a longer one
//! keeps every piece, so the edges a solid is read by look as they did. The
//! view's scale is taken in steps of a factor of two ([`step`]): an outline is
//! built again when the zoom crosses a step, not while the view pans or turns.

use rustc_hash::FxHashMap;

/// The size on screen, in pixels, up to which an edge line is thinned.
pub(super) const SMALL_LINE: f32 = 8.0;

/// The part of its own size the points of a thinned line may move by: half a
/// pixel for a line of [`SMALL_LINE`] pixels, less for a smaller one.
pub(super) const SMALL_LINE_DIVISOR: f64 = 16.0;

/// The step of a scale: `pixels_per_unit` rounded up to a power of two, as
/// its exponent. `None` for a scale that is not a positive number.
pub(super) fn step(pixels_per_unit: f64) -> Option<i32> {
    (pixels_per_unit.is_finite() && pixels_per_unit > 0.0)
        .then(|| pixels_per_unit.log2().ceil() as i32)
}

/// The longest an edge line may be, in the units of the outline, to count as
/// small at the scale of `step`.
pub(super) fn small_line(step: i32) -> f64 {
    SMALL_LINE as f64 / 2f64.powi(step)
}

/// `segments` (the two ends of each, one after the other) with the lines no
/// longer than `small` thinned: the points left out that move no part of the
/// line by more than its size over [`SMALL_LINE_DIVISOR`].
///
/// Segments that follow one another through points only two of them share
/// are one line. A point where an edge ends or three meet stays, and so does
/// at least one segment of every line. The points that are kept are points
/// of `segments`, and segments that lose none come back as they were given.
pub(super) fn simplified(segments: &[[f64; 3]], small: f64) -> Vec<[f64; 3]> {
    if !(small > 0.0) || segments.len() < 4 {
        return segments.to_vec();
    }
    let count = segments.len() / 2;
    let mut ids: FxHashMap<[u64; 3], u32> = FxHashMap::default();
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut id_of = |point: [f64; 3]| -> u32 {
        // `+ 0.0` so that -0.0 and 0.0 are one point.
        let key = point.map(|value| (value + 0.0).to_bits());
        *ids.entry(key).or_insert_with(|| {
            points.push(point);
            points.len() as u32 - 1
        })
    };
    let ends: Vec<[u32; 2]> = (0..count)
        .map(|at| [id_of(segments[2 * at]), id_of(segments[2 * at + 1])])
        .collect();

    // The segments at each point. One that starts and ends on a point is
    // drawn as it is and takes no part in the lines.
    let mut used: Vec<bool> = ends.iter().map(|[a, b]| a == b).collect();
    let mut first = vec![0u32; points.len() + 1];
    for (at, [a, b]) in ends.iter().enumerate() {
        if !used[at] {
            first[*a as usize + 1] += 1;
            first[*b as usize + 1] += 1;
        }
    }
    for point in 0..points.len() {
        first[point + 1] += first[point];
    }
    let mut at_point = vec![0u32; first[points.len()] as usize];
    let mut next = first.clone();
    for (at, [a, b]) in ends.iter().enumerate() {
        if !used[at] {
            for point in [*a, *b] {
                at_point[next[point as usize] as usize] = at as u32;
                next[point as usize] += 1;
            }
        }
    }
    let segments_at = |point: u32| -> &[u32] {
        &at_point[first[point as usize] as usize..first[point as usize + 1] as usize]
    };

    let mut out: Vec<[f64; 3]> = Vec::with_capacity(segments.len());
    for (at, [a, _]) in ends.iter().enumerate() {
        if used[at] {
            out.extend([points[*a as usize], points[*a as usize]]);
        }
    }
    // A line from `start` along `segment`, as far as a point that is not
    // shared by exactly two segments.
    let line_from = |start: u32, segment: u32, used: &mut Vec<bool>| -> Vec<u32> {
        let mut line = vec![start];
        let (mut point, mut segment) = (start, segment);
        loop {
            used[segment as usize] = true;
            let [a, b] = ends[segment as usize];
            point = if a == point { b } else { a };
            line.push(point);
            let here = segments_at(point);
            if here.len() != 2 {
                break;
            }
            match here.iter().find(|other| !used[**other as usize]) {
                Some(other) => segment = *other,
                None => break,
            }
        }
        line
    };
    let mut lines: Vec<Vec<u32>> = Vec::new();
    for point in 0..points.len() as u32 {
        if segments_at(point).len() == 2 {
            continue;
        }
        for &segment in segments_at(point) {
            if !used[segment as usize] {
                lines.push(line_from(point, segment, &mut used));
            }
        }
    }
    // What is left closes on itself through points two segments share.
    for at in 0..count {
        if !used[at] {
            lines.push(line_from(ends[at][0], at as u32, &mut used));
        }
    }

    let mut kept: Vec<u32> = Vec::new();
    let mut own: Vec<[f64; 3]> = Vec::new();
    for line in &lines {
        kept.clear();
        own.clear();
        own.extend(line.iter().map(|point| points[*point as usize]));
        let size = extent(&own);
        if line.len() <= 2 || size > small {
            // A long line, or a single segment: every piece of it.
            kept.extend_from_slice(line);
        } else if line.first() == line.last() {
            // A closed line: thinned as two halves, from its first point to
            // the one farthest from it and back.
            let tolerance = size / SMALL_LINE_DIVISOR;
            let far = (1..line.len() - 1)
                .max_by(|a, b| {
                    distance_squared(own[*a], own[0]).total_cmp(&distance_squared(own[*b], own[0]))
                })
                .unwrap_or(1);
            thin(&points, &line[..=far], tolerance, &mut kept);
            kept.pop();
            thin(&points, &line[far..], tolerance, &mut kept);
            if kept.len() == 3 {
                // There and back along one segment.
                kept.pop();
            }
        } else {
            thin(&points, line, size / SMALL_LINE_DIVISOR, &mut kept);
        }
        for pair in kept.windows(2) {
            out.extend([points[pair[0] as usize], points[pair[1] as usize]]);
        }
    }
    if out.len() == segments.len() {
        // Nothing to leave out: the segments as they came, in their order.
        return segments.to_vec();
    }
    out
}

/// The diagonal of the box that holds `points`.
fn extent(points: &[[f64; 3]]) -> f64 {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let squared: f64 = (0..3).map(|axis| (high[axis] - low[axis]).powi(2)).sum();
    if squared.is_finite() {
        squared.sqrt()
    } else {
        f64::INFINITY
    }
}

/// The points of `line` to keep so that none of the others is farther than
/// `tolerance` from the kept line, appended to `kept`.
fn thin(points: &[[f64; 3]], line: &[u32], tolerance: f64, kept: &mut Vec<u32>) {
    let Some((&first, rest)) = line.split_first() else {
        return;
    };
    kept.push(first);
    if rest.is_empty() {
        return;
    }
    let limit = tolerance * tolerance;
    // Spans still to settle, the last pushed is taken first: the nearest to
    // the start goes last, so the points come out in order.
    let mut spans = vec![(0usize, line.len() - 1)];
    while let Some((from, to)) = spans.pop() {
        let (a, b) = (points[line[from] as usize], points[line[to] as usize]);
        let mut farthest = (limit, None);
        for at in from + 1..to {
            let distance = distance_to_segment_squared(points[line[at] as usize], a, b);
            if distance > farthest.0 {
                farthest = (distance, Some(at));
            }
        }
        match farthest.1 {
            Some(at) => {
                spans.push((at, to));
                spans.push((from, at));
            }
            None => kept.push(line[to]),
        }
    }
}

fn distance_squared(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|axis| (a[axis] - b[axis]).powi(2)).sum()
}

fn distance_to_segment_squared(point: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let along: [f64; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
    let length: f64 = along.iter().map(|value| value * value).sum();
    if !(length > 0.0) {
        return distance_squared(point, a);
    }
    let dot: f64 = (0..3)
        .map(|axis| (point[axis] - a[axis]) * along[axis])
        .sum();
    let t = (dot / length).clamp(0.0, 1.0);
    let nearest: [f64; 3] = std::array::from_fn(|axis| a[axis] + along[axis] * t);
    distance_squared(point, nearest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A circle of `pieces` segments about `centre`, parallel to the XY plane.
    fn circle(centre: [f64; 3], radius: f64, pieces: usize) -> Vec<[f64; 3]> {
        let at = |index: usize| {
            let angle = std::f64::consts::TAU * (index % pieces) as f64 / pieces as f64;
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
                centre[2],
            ]
        };
        (0..pieces)
            .flat_map(|index| [at(index), at(index + 1)])
            .collect()
    }

    /// The largest distance from a point of `from` to the segments of `to`.
    fn farthest(from: &[[f64; 3]], to: &[[f64; 3]]) -> f64 {
        from.iter()
            .map(|point| {
                to.chunks_exact(2)
                    .map(|pair| distance_to_segment_squared(*point, pair[0], pair[1]))
                    .fold(f64::INFINITY, f64::min)
                    .sqrt()
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_small_circle_keeps_the_points_its_size_can_show() {
        // Radius 1: its box measures 2.83 across, under the 3 that counts as
        // small here.
        let whole = circle([0.0; 3], 1.0, 64);
        let thinned = simplified(&whole, 3.0);
        assert!(
            (4..=12).contains(&(thinned.len() / 2)),
            "{} segments of 64",
            thinned.len() / 2
        );
        // No point of the circle is farther from what is left than its size
        // over the divisor, and what is left is made of the circle's points.
        let size = extent(&whole);
        assert!(farthest(&whole, &thinned) <= size / SMALL_LINE_DIVISOR + 1e-12);
        assert!(thinned.iter().all(|point| whole.contains(point)));
        // It still closes: every point is the end of two segments.
        for point in &thinned {
            assert_eq!(thinned.iter().filter(|other| *other == point).count(), 2);
        }
    }

    #[test]
    fn a_line_longer_than_small_keeps_every_piece() {
        let whole = circle([0.0; 3], 1.0, 64);
        assert_eq!(simplified(&whole, 2.8), whole, "2.83 across is not small");
        assert_eq!(simplified(&whole, 0.0), whole, "nothing counts as small");
        assert_eq!(simplified(&whole, f64::NAN), whole);
    }

    /// A tube: two small circles at its ends and a long line between them.
    /// The circles are thinned, each on its own, and the line stays whole.
    #[test]
    fn the_small_lines_of_a_solid_are_thinned_and_its_long_ones_are_not() {
        let mut edges = circle([0.0, 0.0, 0.0], 1.0, 48);
        edges.extend(circle([0.0, 0.0, 300.0], 1.0, 48));
        // The seam, tessellated in ten pieces from one circle to the other.
        let seam: Vec<[f64; 3]> = (0..10)
            .flat_map(|piece| {
                [
                    [1.0, 0.0, 30.0 * piece as f64],
                    [1.0, 0.0, 30.0 * (piece + 1) as f64],
                ]
            })
            .collect();
        edges.extend(seam.iter().copied());
        let thinned = simplified(&edges, 4.0);
        // Every piece of the seam is there.
        for pair in seam.chunks_exact(2) {
            assert!(
                thinned.chunks_exact(2).any(|kept| kept == pair),
                "a piece of the seam is gone: {pair:?}"
            );
        }
        let circles = thinned.len() / 2 - 10;
        assert!(
            (8..=24).contains(&circles),
            "{circles} segments for two circles of 48"
        );
        // The point where the seam meets each circle is still on the circle.
        for point in [[1.0, 0.0, 0.0], [1.0, 0.0, 300.0]] {
            assert_eq!(thinned.iter().filter(|other| **other == point).count(), 3);
        }
        assert!(
            farthest(&edges, &thinned) <= extent(&circle([0.0; 3], 1.0, 48)) / SMALL_LINE_DIVISOR
        );
    }

    #[test]
    fn straight_edges_and_corners_stay() {
        // A box: twelve edges, three at each corner.
        let corner = |index: usize| {
            [
                (index & 1) as f64 * 4.0,
                ((index >> 1) & 1) as f64 * 2.0,
                ((index >> 2) & 1) as f64,
            ]
        };
        let mut edges = Vec::new();
        for a in 0..8usize {
            for axis in 0..3 {
                let b = a | (1 << axis);
                if b != a {
                    edges.extend([corner(a), corner(b)]);
                }
            }
        }
        assert_eq!(edges.len(), 24);
        assert_eq!(simplified(&edges, 100.0), edges, "every edge, in its order");
    }

    #[test]
    fn the_ends_of_an_arc_stay() {
        // A quarter circle from (10, 0) to (0, 10).
        let pieces = 16;
        let at = |index: usize| {
            let angle = std::f64::consts::FRAC_PI_2 * index as f64 / pieces as f64;
            [10.0 * angle.cos(), 10.0 * angle.sin(), 0.0]
        };
        let edges: Vec<[f64; 3]> = (0..pieces)
            .flat_map(|index| [at(index), at(index + 1)])
            .collect();
        let thinned = simplified(&edges, 20.0);
        assert!(thinned.len() < edges.len());
        assert!(thinned.contains(&at(0)) && thinned.contains(&at(16)));
        assert!(farthest(&edges, &thinned) <= extent(&edges) / SMALL_LINE_DIVISOR + 1e-12);
    }

    #[test]
    fn a_scale_rounds_up_to_its_step() {
        assert_eq!(step(1.0), Some(0));
        assert_eq!(step(1.01), Some(1));
        assert_eq!(step(2.0), Some(1));
        assert_eq!(step(0.3), Some(-1));
        assert_eq!(step(0.0), None);
        assert_eq!(step(f64::NAN), None);
        for scale in [0.013f64, 0.5, 1.0, 3.7, 900.0] {
            // A line that is small at the step is small at the scale itself.
            let small = small_line(step(scale).unwrap());
            assert!(small * scale <= SMALL_LINE as f64 * 1.0001, "{scale}");
            assert!(small * scale > SMALL_LINE as f64 * 0.4999, "{scale}");
        }
        // At its largest a thinned line moves half a pixel.
        assert_eq!(SMALL_LINE as f64 / SMALL_LINE_DIVISOR, 0.5);
    }
}
