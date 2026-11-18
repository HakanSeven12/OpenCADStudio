//! Filling a polygon with triangles.
//!
//! Every surface that has to be drawn or measured by area ends up needing
//! this: a face's boundary in its own parameter space is a ring of points,
//! and a renderer wants triangles.
//!
//! # Ear clipping, and why holes are bridged rather than special-cased
//!
//! The method is the simple one: find a corner whose triangle contains no
//! other vertex, cut it off, repeat. Candidate corners are selected by shape
//! score before testing containment, so the first valid candidate is the
//! preferred ear. Candidates wait in a heap and the containment test only
//! looks at points near the ear, so a ring with hundreds of bridged holes
//! stays fast; a ring where most candidates are blocked can still need many
//! tests per cut.
//!
//! Holes are joined to the outer ring by a bridge: a pair of coincident edges
//! run out to the hole, round it, and back. The result is one ring with a
//! slit in it, which ear clipping then handles with no cases of its own. The
//! slit's two edges are zero-width and vanish in the triangulation.
//!
//! # What it will not do
//!
//! A single self-intersecting ring given to [`polygon`] has no inside to
//! fill, and no triangulation of it is right. That returns nothing rather
//! than a plausible mesh, on the principle that a caller can draw nothing but
//! cannot un-draw a wrong shape. [`rings`] fills rings by the even-odd rule
//! instead, so a ring that crosses itself is first split into the faces it
//! covers an odd number of times (see [`split_self_crossing`]).

use super::vec::Vec2;

#[cfg(test)]
thread_local! {
    /// How many times this thread has set one ring against another: to ask
    /// whether one holds the other, whether their boundaries cross, or
    /// which outer ring they hang from.
    static RING_PAIRS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn count_ring_pairs(pairs: usize) {
    RING_PAIRS.with(|count| count.set(count.get() + pairs));
}

#[cfg(test)]
thread_local! {
    /// Makes [`RingGrid::overlapping`] go through every ring, as the pair
    /// loops did before the grid: the parity reference for it.
    static EVERY_RING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Clone, Copy)]
struct RingBounds {
    min: Vec2,
    max: Vec2,
}

impl RingBounds {
    fn of(ring: &[[f64; 2]]) -> Option<Self> {
        let mut points = ring.iter().copied().map(Vec2::from);
        let first = points.next()?;
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for point in points {
            bounds.min.x = bounds.min.x.min(point.x);
            bounds.min.y = bounds.min.y.min(point.y);
            bounds.max.x = bounds.max.x.max(point.x);
            bounds.max.y = bounds.max.y.max(point.y);
        }
        Some(bounds)
    }

    fn overlaps(self, other: Self) -> bool {
        self.max.x >= other.min.x
            && other.max.x >= self.min.x
            && self.max.y >= other.min.y
            && other.max.y >= self.min.y
    }
}

/// The boxes of a set of rings by place, for finding the rings whose box
/// overlaps a given one without going through them all.
///
/// Whether one ring holds another and whether two boundaries cross were
/// asked of every pair of rings. Both need the two boxes to overlap, so a
/// fill of thousands of separate loops spent its time on pairs that are
/// nowhere near each other.
struct RingGrid<'a> {
    bounds: &'a [Option<RingBounds>],
    origin: Vec2,
    cell: f64,
    /// The rings with a box in each cell.
    cells: std::collections::HashMap<(i64, i64), Vec<usize>>,
    /// Rings whose box spans too many cells to be listed in each. Every
    /// search reads these.
    wide: Vec<usize>,
}

impl<'a> RingGrid<'a> {
    /// A ring that spans more cells than this is read by every search.
    const MAX_CELLS: i128 = 16;

    fn new(bounds: &'a [Option<RingBounds>]) -> Self {
        let mut min = Vec2::new(f64::INFINITY, f64::INFINITY);
        let mut max = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut placed = 0usize;
        for ring in bounds.iter().flatten() {
            placed += 1;
            min.x = min.x.min(ring.min.x);
            min.y = min.y.min(ring.min.y);
            max.x = max.x.max(ring.max.x);
            max.y = max.y.max(ring.max.y);
        }
        // About one ring to a cell when they are spread evenly.
        let span = (max.x - min.x).max(max.y - min.y);
        let per_side = (placed as f64).sqrt().ceil().max(1.0);
        let cell = if span.is_finite() && span > 0.0 {
            span / per_side
        } else {
            1.0
        };
        let mut grid = Self {
            bounds,
            origin: if placed == 0 {
                Vec2::new(0.0, 0.0)
            } else {
                min
            },
            cell,
            cells: std::collections::HashMap::new(),
            wide: Vec::new(),
        };
        for (index, ring) in bounds.iter().enumerate() {
            let Some(ring) = ring else {
                continue;
            };
            let (x, y) = grid.cell_ranges(*ring);
            if Self::cell_count(x, y) > Self::MAX_CELLS {
                grid.wide.push(index);
                continue;
            }
            for cell_x in x.0..=x.1 {
                for cell_y in y.0..=y.1 {
                    grid.cells.entry((cell_x, cell_y)).or_default().push(index);
                }
            }
        }
        grid
    }

    fn cell_ranges(&self, bounds: RingBounds) -> ((i64, i64), (i64, i64)) {
        let cell = |value: f64, origin: f64| ((value - origin) / self.cell).floor() as i64;
        (
            (
                cell(bounds.min.x, self.origin.x),
                cell(bounds.max.x, self.origin.x),
            ),
            (
                cell(bounds.min.y, self.origin.y),
                cell(bounds.max.y, self.origin.y),
            ),
        )
    }

    fn cell_count(x: (i64, i64), y: (i64, i64)) -> i128 {
        (i128::from(x.1) - i128::from(x.0) + 1) * (i128::from(y.1) - i128::from(y.0) + 1)
    }

    /// The rings whose box overlaps `bounds`, lowest index first.
    fn overlapping(&self, bounds: RingBounds) -> Vec<usize> {
        #[cfg(test)]
        if EVERY_RING.with(std::cell::Cell::get) {
            return (0..self.bounds.len())
                .filter(|index| self.bounds[*index].is_some_and(|other| bounds.overlaps(other)))
                .collect();
        }
        let mut found = self.wide.clone();
        let (x, y) = self.cell_ranges(bounds);
        if Self::cell_count(x, y) > self.cells.len() as i128 {
            // A box over most of the rings: go by the cells there are.
            for ((cell_x, cell_y), listed) in &self.cells {
                if (x.0..=x.1).contains(cell_x) && (y.0..=y.1).contains(cell_y) {
                    found.extend(listed);
                }
            }
        } else {
            for cell_x in x.0..=x.1 {
                for cell_y in y.0..=y.1 {
                    if let Some(listed) = self.cells.get(&(cell_x, cell_y)) {
                        found.extend(listed);
                    }
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        #[cfg(test)]
        count_ring_pairs(found.len());
        found.retain(|index| self.bounds[*index].is_some_and(|other| bounds.overlaps(other)));
        found
    }
}

struct CheckedRing {
    points: Vec<[f64; 2]>,
    area: f64,
    bounds: RingBounds,
}

/// A validated polygon and its axis-aligned local frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PolygonFrame {
    pub origin: [f64; 2],
    pub size: [f64; 2],
    pub points: Vec<[f64; 2]>,
}

/// Cleans and validates one simple polygon.
pub fn polygon_frame(input: &[[f64; 2]], tolerance: super::Tolerance) -> Option<PolygonFrame> {
    if input.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let limit = tolerance.linear();
    let mut points = Vec::with_capacity(input.len());
    for &point in input {
        if points
            .last()
            .is_none_or(|previous| Vec2::from(*previous).distance(Vec2::from(point)) > limit)
        {
            points.push(point);
        }
    }
    while points.len() > 1
        && Vec2::from(points[0]).distance(Vec2::from(*points.last().unwrap())) <= limit
    {
        points.pop();
    }
    let bounds = RingBounds::of(&points)?;
    let size = [bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y];
    let scale = size[0].max(size[1]);
    let area = signed_area_arrays(&points).abs();
    if size[0] <= limit
        || size[1] <= limit
        || area <= limit * scale.max(limit)
        || !simple(&points, limit)
    {
        return None;
    }
    Some(PolygonFrame {
        origin: [bounds.min.x, bounds.min.y],
        size,
        points,
    })
}

/// Containment depth of each input ring. Even depths are filled exteriors.
pub fn nesting_depths(input: &[Vec<[f64; 2]>]) -> Vec<usize> {
    let rings: Vec<Vec<[f64; 2]>> = input.iter().map(|ring| sanitize(ring)).collect();
    let areas: Vec<f64> = rings
        .iter()
        .map(|ring| signed_area_arrays(ring).abs())
        .collect();

    // A ring is held only by rings whose box overlaps its own: the point of
    // it that is tested lies in both.
    let bounds: Vec<Option<RingBounds>> = rings.iter().map(|ring| RingBounds::of(ring)).collect();
    let grid = RingGrid::new(&bounds);
    rings
        .iter()
        .enumerate()
        .map(|(index, ring)| {
            let Some(own) = bounds[index] else {
                return 0;
            };
            grid.overlapping(own)
                .into_iter()
                .filter(|other| {
                    *other != index
                        && areas[*other] > areas[index]
                        && ring_strictly_contains(&rings[*other], ring)
                })
                .count()
        })
        .collect()
}

/// Triangulates rings using even-odd containment.
///
/// Disjoint outer rings, holes, and nested islands may be mixed in any order.
/// A ring that crosses itself is split into the faces it covers an odd
/// number of times (see [`split_self_crossing`]), and simple rings whose
/// boundaries cross each other fill their even-odd region (see [`uncross`]).
/// A ring that still cannot be filled, and what it touches, is omitted.
pub fn rings(input: &[Vec<[f64; 2]>]) -> (Vec<[f64; 2]>, Vec<[usize; 3]>) {
    rings_from(uncross(&split_self_crossing(input)))
}

/// [`rings`] without splitting self-crossing rings: such a ring, and what it
/// touches, is omitted. Boundary recovery uses it to tell a crossed sample
/// ring, which it can resample, from one that fills as it is.
pub fn simple_rings(input: &[Vec<[f64; 2]>]) -> (Vec<[f64; 2]>, Vec<[usize; 3]>) {
    rings_from(uncross(input))
}

fn rings_from(input: Vec<Vec<[f64; 2]>>) -> (Vec<[f64; 2]>, Vec<[usize; 3]>) {
    let mut rings = Vec::new();
    let mut invalid = Vec::new();
    for raw in &input {
        let points = sanitize(raw);
        let Some(bounds) = RingBounds::of(&points) else {
            continue;
        };
        let area = signed_area_arrays(&points);
        if !simple(&points, 0.0) || !area.is_finite() || area == 0.0 {
            invalid.push(CheckedRing {
                points,
                area: area.abs(),
                bounds,
            });
            continue;
        }
        rings.push(CheckedRing {
            points,
            area: area.abs(),
            bounds,
        });
    }

    let mut bad = vec![false; rings.len()];
    {
        let bounds: Vec<Option<RingBounds>> = rings.iter().map(|ring| Some(ring.bounds)).collect();
        let grid = RingGrid::new(&bounds);
        for first in 0..rings.len() {
            for second in grid.overlapping(rings[first].bounds) {
                if second > first
                    && ring_boundaries_properly_cross(&rings[first].points, &rings[second].points)
                {
                    bad[first] = true;
                    bad[second] = true;
                }
            }
        }
    }
    for (index, ring) in rings.iter().enumerate() {
        if invalid.iter().any(|other| rings_interact(ring, other)) {
            bad[index] = true;
        }
    }
    loop {
        let active_bad: Vec<usize> = bad
            .iter()
            .enumerate()
            .filter_map(|(index, bad)| bad.then_some(index))
            .collect();
        let mut changed = false;
        for (index, ring) in rings.iter().enumerate() {
            if !bad[index]
                && active_bad
                    .iter()
                    .any(|other| rings_interact(ring, &rings[*other]))
            {
                bad[index] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let valid: Vec<CheckedRing> = rings
        .into_iter()
        .zip(bad)
        .filter_map(|(ring, bad)| (!bad).then_some(ring))
        .collect();
    let mut parent = vec![None; valid.len()];
    {
        let bounds: Vec<Option<RingBounds>> = valid.iter().map(|ring| Some(ring.bounds)).collect();
        let grid = RingGrid::new(&bounds);
        for index in 0..valid.len() {
            parent[index] = grid
                .overlapping(valid[index].bounds)
                .into_iter()
                .filter(|other| {
                    *other != index
                        && valid[*other].area > valid[index].area
                        && ring_strictly_contains(&valid[*other].points, &valid[index].points)
                })
                .min_by(|a, b| valid[*a].area.total_cmp(&valid[*b].area));
        }
    }
    let depth: Vec<usize> = (0..valid.len())
        .map(|index| {
            let mut depth = 0;
            let mut next = parent[index];
            while let Some(ancestor) = next {
                depth += 1;
                next = parent[ancestor];
            }
            depth
        })
        .collect();

    let mut points = Vec::new();
    let mut triangles = Vec::new();
    // The rings under each outermost ring, itself included.
    let mut members_of: Vec<Vec<usize>> = vec![Vec::new(); valid.len()];
    for index in 0..valid.len() {
        #[cfg(test)]
        count_ring_pairs(1);
        members_of[root_of(index, &parent)].push(index);
    }
    for root in (0..valid.len()).filter(|index| parent[*index].is_none()) {
        let members = std::mem::take(&mut members_of[root]);
        let mut component_points = Vec::new();
        let mut component_triangles = Vec::new();
        let mut complete = true;
        for outer in members
            .iter()
            .copied()
            .filter(|index| depth[*index] % 2 == 0)
        {
            let holes: Vec<Vec<[f64; 2]>> = members
                .iter()
                .copied()
                .filter(|index| parent[*index] == Some(outer) && depth[*index] % 2 == 1)
                .map(|index| valid[index].points.clone())
                .collect();
            let expected = valid[outer].area
                - holes
                    .iter()
                    .map(|hole| signed_area_arrays(hole).abs())
                    .sum::<f64>();
            let (local_points, local_triangles) = polygon(&valid[outer].points, &holes);
            if !triangulation_matches(&local_points, &local_triangles, expected) {
                complete = false;
                break;
            }
            let base = component_points.len();
            component_points.extend(local_points);
            component_triangles.extend(
                local_triangles
                    .into_iter()
                    .map(|triangle| [triangle[0] + base, triangle[1] + base, triangle[2] + base]),
            );
        }
        // Holes whose boundaries touch or overlap (a column against a beam)
        // defeat the polygon triangulator, and the whole component used to
        // be dropped. Fill it by the even-odd rule directly instead.
        if !complete {
            let rings: Vec<&[[f64; 2]]> = members
                .iter()
                .map(|index| valid[*index].points.as_slice())
                .collect();
            (component_points, component_triangles) = even_odd_slabs(&rings);
            complete = !component_triangles.is_empty();
        }
        if complete {
            let base = points.len();
            points.extend(component_points);
            triangles.extend(
                component_triangles
                    .into_iter()
                    .map(|triangle| [triangle[0] + base, triangle[1] + base, triangle[2] + base]),
            );
        }
    }
    (points, triangles)
}

/// Even-odd fill of rings whose boundaries may touch or overlap: vertical
/// slabs between every vertex and crossing, each cut into trapezoids
/// between alternate edges. Shared edges count twice and so cancel.
// ponytail: one slab per distinct x, so a curved boundary with thousands of
// vertices makes thousands of thin slabs; only used when the polygon
// triangulator gave up on a component.
fn even_odd_slabs(rings: &[&[[f64; 2]]]) -> (Vec<[f64; 2]>, Vec<[usize; 3]>) {
    let segments: Vec<super::curve::Line> = rings
        .iter()
        .flat_map(|ring| {
            (0..ring.len()).map(move |i| super::curve::Line {
                start: ring[i],
                end: ring[(i + 1) % ring.len()],
            })
        })
        .collect();
    let extent = rings
        .iter()
        .flat_map(|ring| ring.iter())
        .flatten()
        .fold(0.0_f64, |extent, value| extent.max(value.abs()))
        .max(1.0);
    let tolerance = extent * 1e-12;
    let mut edges: Vec<([f64; 2], [f64; 2])> =
        super::arrangement::split_at_crossings(&segments, tolerance)
            .into_iter()
            .filter(|line| (line.end[0] - line.start[0]).abs() > tolerance)
            .map(|line| {
                if line.start[0] < line.end[0] {
                    (line.start, line.end)
                } else {
                    (line.end, line.start)
                }
            })
            .collect();
    edges.sort_by(|a, b| a.0[0].total_cmp(&b.0[0]));
    let mut xs: Vec<f64> = edges.iter().flat_map(|(a, b)| [a[0], b[0]]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() <= tolerance);
    let y_at =
        |(a, b): ([f64; 2], [f64; 2]), x: f64| a[1] + (x - a[0]) * (b[1] - a[1]) / (b[0] - a[0]);

    let mut points = Vec::new();
    let mut triangles = Vec::new();
    let mut active: Vec<usize> = Vec::new();
    let mut next = 0;
    for slab in xs.windows(2) {
        let (x0, x1) = (slab[0], slab[1]);
        while next < edges.len() && edges[next].0[0] <= x0 + tolerance {
            active.push(next);
            next += 1;
        }
        active.retain(|edge| edges[*edge].1[0] >= x1 - tolerance);
        let mut spans: Vec<(f64, f64)> = active
            .iter()
            .map(|edge| (y_at(edges[*edge], x0), y_at(edges[*edge], x1)))
            .collect();
        spans.sort_by(|a, b| (a.0 + a.1).total_cmp(&(b.0 + b.1)));
        for pair in spans.chunks_exact(2) {
            let ((b0, b1), (t0, t1)) = (pair[0], pair[1]);
            if (t0 - b0).abs() <= tolerance && (t1 - b1).abs() <= tolerance {
                continue;
            }
            let base = points.len();
            points.extend([[x0, b0], [x1, b1], [x1, t1], [x0, t0]]);
            triangles.push([base, base + 1, base + 2]);
            triangles.push([base, base + 2, base + 3]);
        }
    }
    (points, triangles)
}

fn rings_interact(first: &CheckedRing, second: &CheckedRing) -> bool {
    first.bounds.overlaps(second.bounds)
        && (ring_boundaries_intersect(&first.points, &second.points)
            || ring_strictly_contains(&first.points, &second.points)
            || ring_strictly_contains(&second.points, &first.points))
}

fn sanitize(ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if ring.iter().flatten().any(|value| !value.is_finite()) {
        return Vec::new();
    }
    let mut points = Vec::with_capacity(ring.len());
    for point in ring {
        if points.last() != Some(point) {
            points.push(*point);
        }
    }
    while points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    if points.len() < 3 {
        return points;
    }
    // A corner whose two edges fold back onto each other (a zero-area spike)
    // encloses nothing, but leaves the ring non-simple: drop it, and the
    // duplicate it may leave behind, until none remain.
    //
    // The sweep is single-pass: each candidate pops the accepted points while
    // they close over it as a spike, so a ring with millions of points and
    // hundreds of thousands of folded-back spikes (GIS-exported parcel
    // boundaries do exactly that) is processed in linear time. The old
    // per-index `Vec::remove` scan was quadratic and took minutes per hatch.
    let spike = |before: [f64; 2], here: [f64; 2], after: [f64; 2]| -> bool {
        let incoming = Vec2::from(before) - Vec2::from(here);
        let outgoing = Vec2::from(after) - Vec2::from(here);
        let length = incoming.length().max(outgoing.length()).max(1.0);
        let epsilon = 64.0 * f64::EPSILON * length * length;
        incoming.cross(outgoing).abs() <= epsilon && incoming.dot(outgoing) > 0.0
    };
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for &point in &points {
        while out.len() >= 2 && spike(out[out.len() - 2], out[out.len() - 1], point) {
            out.pop();
        }
        if out.last() == Some(&point) {
            // The duplicate a spike removal left behind: dropped.
            continue;
        }
        out.push(point);
    }
    // The sweep above treats the ring as an open strip, so the head/tail seam
    // can still hide spikes. Settle the junction with O(1) end removals.
    let mut accepted = std::collections::VecDeque::from(out);
    loop {
        if accepted.len() < 3 {
            break;
        }
        let tail = accepted[accepted.len() - 1];
        let head = accepted[0];
        let after_head = accepted[1];
        if spike(accepted[accepted.len() - 2], tail, head) {
            accepted.pop_back();
            continue;
        }
        if spike(tail, head, after_head) {
            accepted.pop_front();
            continue;
        }
        break;
    }
    accepted.into_iter().collect()
}

fn signed_area_arrays(ring: &[[f64; 2]]) -> f64 {
    let points: Vec<Vec2> = ring.iter().copied().map(Vec2::from).collect();
    signed_area(&points)
}

fn simple(ring: &[[f64; 2]], tolerance: f64) -> bool {
    if ring.len() < 3 {
        return false;
    }
    for first in 0..ring.len() {
        if (first + 1..ring.len())
            .any(|second| Vec2::from(ring[first]).distance(Vec2::from(ring[second])) <= tolerance)
        {
            return false;
        }
    }
    for index in 0..ring.len() {
        let before = Vec2::from(ring[(index + ring.len() - 1) % ring.len()]);
        let here = Vec2::from(ring[index]);
        let after = Vec2::from(ring[(index + 1) % ring.len()]);
        let incoming = before - here;
        let outgoing = after - here;
        let length = incoming.length().max(outgoing.length()).max(1.0);
        let epsilon = (64.0 * f64::EPSILON * length * length).max(tolerance * length);
        if incoming.cross(outgoing).abs() <= epsilon && incoming.dot(outgoing) > 0.0 {
            return false;
        }
    }
    for first in 0..ring.len() {
        let first_next = (first + 1) % ring.len();
        for second in first + 1..ring.len() {
            let second_next = (second + 1) % ring.len();
            if first_next == second || second_next == first {
                continue;
            }
            if segments_intersect_with_tolerance(
                ring[first],
                ring[first_next],
                ring[second],
                ring[second_next],
                tolerance,
            ) {
                return false;
            }
        }
    }
    true
}

/// A point well inside a simple ring: the centre of the largest triangle of
/// its triangulation. `None` when the ring does not triangulate.
fn interior_sample(face: &[[f64; 2]]) -> Option<[f64; 2]> {
    let (points, triangles) = polygon(face, &[]);
    let corners = |t: &[usize; 3]| t.map(|vertex| Vec2::from(points[vertex]));
    let area = |[a, b, c]: [Vec2; 3]| (b - a).cross(c - a).abs();
    let [a, b, c] = triangles
        .iter()
        .map(corners)
        .max_by(|p, q| area(*p).total_cmp(&area(*q)))?;
    Some(((a + b + c) * (1.0 / 3.0)).to_array())
}

/// Each ring that crosses itself, replaced by the faces of its own
/// arrangement that it covers an odd number of times: a bow tie becomes its
/// two lobes. As one invalid ring it was dropped together with every ring it
/// touched, so a hatch with a self-crossing island lost its whole fill. A
/// ring whose arrangement gives no such face passes through unchanged, and
/// so does every simple ring.
fn split_self_crossing(input: &[Vec<[f64; 2]>]) -> Vec<Vec<[f64; 2]>> {
    use super::{bounded_faces, Line, Tolerance};
    let mut out = Vec::with_capacity(input.len());
    for raw in input {
        let ring = sanitize(raw);
        let crossing = ring.len() >= 4 && !simple(&ring, 0.0);
        let Some(bounds) = RingBounds::of(&ring).filter(|_| crossing) else {
            out.push(raw.clone());
            continue;
        };
        let extent = (bounds.max - bounds.min).length();
        if !(extent > 0.0) {
            out.push(raw.clone());
            continue;
        }
        let lines: Vec<Line> = (0..ring.len())
            .map(|k| Line {
                start: ring[k],
                end: ring[(k + 1) % ring.len()],
            })
            .collect();
        let faces: Vec<Vec<[f64; 2]>> = bounded_faces(&lines, Tolerance::new(extent * 1e-9))
            .into_iter()
            .filter(|face| interior_sample(face).is_some_and(|sample| ring_contains(&ring, sample)))
            .collect();
        if faces.is_empty() {
            out.push(raw.clone());
        } else {
            out.extend(faces);
        }
    }
    out
}

/// Each group of simple rings whose boundaries cross, replaced by the faces
/// of their arrangement that an odd number of them cover: the region they
/// fill under the even-odd rule, as rings that at most touch. A hatch whose
/// loops were stretched across each other keeps its fill instead of losing
/// it. Other rings pass through unchanged.
fn uncross(input: &[Vec<[f64; 2]>]) -> Vec<Vec<[f64; 2]>> {
    use super::{bounded_faces, Line, Tolerance};
    let rings: Vec<Vec<[f64; 2]>> = input.iter().map(|ring| sanitize(ring)).collect();
    let bounds: Vec<Option<RingBounds>> = rings
        .iter()
        .map(|ring| RingBounds::of(ring).filter(|_| ring.len() >= 3 && simple(ring, 0.0)))
        .collect();
    let root = |group: &[usize], mut index: usize| {
        while group[index] != index {
            index = group[index];
        }
        index
    };
    let mut group: Vec<usize> = (0..rings.len()).collect();
    let grid = RingGrid::new(&bounds);
    for first in 0..rings.len() {
        let Some(own) = bounds[first] else {
            continue;
        };
        for second in grid.overlapping(own) {
            if second > first && ring_boundaries_properly_cross(&rings[first], &rings[second]) {
                let (a, b) = (root(&group, first), root(&group, second));
                group[a.max(b)] = a.min(b);
            }
        }
    }
    let roots: Vec<usize> = (0..rings.len()).map(|index| root(&group, index)).collect();
    if roots.iter().enumerate().all(|(index, root)| index == *root) {
        return input.to_vec();
    }
    let roots = &roots;
    let members = |root: usize| (0..rings.len()).filter(move |index| roots[*index] == root);
    let mut out: Vec<Vec<[f64; 2]>> = input
        .iter()
        .enumerate()
        .filter(|(index, _)| members(roots[*index]).count() == 1)
        .map(|(_, ring)| ring.clone())
        .collect();
    for root in (0..rings.len()).filter(|index| roots[*index] == *index) {
        let group: Vec<usize> = members(root).collect();
        if group.len() < 2 {
            continue;
        }
        let lines: Vec<Line> = group
            .iter()
            .flat_map(|&index| {
                let ring = &rings[index];
                (0..ring.len()).map(move |k| Line {
                    start: ring[k],
                    end: ring[(k + 1) % ring.len()],
                })
            })
            .collect();
        let extent = group
            .iter()
            .filter_map(|index| bounds[*index])
            .map(|b| (b.max - b.min).length())
            .fold(0.0, f64::max);
        if !(extent > 0.0) {
            continue;
        }
        for face in bounded_faces(&lines, Tolerance::new(extent * 1e-9)) {
            let Some(sample) = interior_sample(&face) else {
                continue;
            };
            let cover = group
                .iter()
                .filter(|index| ring_contains(&rings[**index], sample))
                .count();
            if cover % 2 == 1 {
                out.push(face);
            }
        }
    }
    out
}

fn ring_boundaries_intersect(first: &[[f64; 2]], second: &[[f64; 2]]) -> bool {
    (0..first.len()).any(|a| {
        (0..second.len()).any(|b| {
            segments_intersect(
                first[a],
                first[(a + 1) % first.len()],
                second[b],
                second[(b + 1) % second.len()],
            )
        })
    })
}

fn ring_boundaries_properly_cross(first: &[[f64; 2]], second: &[[f64; 2]]) -> bool {
    (0..first.len()).any(|a| {
        (0..second.len()).any(|b| {
            segments_properly_cross(
                first[a],
                first[(a + 1) % first.len()],
                second[b],
                second[(b + 1) % second.len()],
            )
        })
    })
}

fn segments_properly_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (a, b, c, d) = (Vec2::from(a), Vec2::from(b), Vec2::from(c), Vec2::from(d));
    let turn = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let values = [turn(a, b, c), turn(a, b, d), turn(c, d, a), turn(c, d, b)];
    let scale = (b - a)
        .length_squared()
        .max((d - c).length_squared())
        .max(1.0);
    let epsilon = 64.0 * f64::EPSILON * scale;
    (values[0] > epsilon && values[1] < -epsilon || values[0] < -epsilon && values[1] > epsilon)
        && (values[2] > epsilon && values[3] < -epsilon
            || values[2] < -epsilon && values[3] > epsilon)
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    segments_intersect_with_tolerance(a, b, c, d, 0.0)
}

fn segments_intersect_with_tolerance(
    a: [f64; 2],
    b: [f64; 2],
    c: [f64; 2],
    d: [f64; 2],
    tolerance: f64,
) -> bool {
    let (a, b, c, d) = (Vec2::from(a), Vec2::from(b), Vec2::from(c), Vec2::from(d));
    let turn = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let values = [turn(a, b, c), turn(a, b, d), turn(c, d, a), turn(c, d, b)];
    let length = (b - a).length().max((d - c).length()).max(1.0);
    let epsilon = (64.0 * f64::EPSILON * length * length).max(tolerance * length);
    let on = |p: Vec2, q: Vec2, r: Vec2, value: f64| {
        value.abs() <= epsilon
            && r.x >= p.x.min(q.x) - tolerance
            && r.x <= p.x.max(q.x) + tolerance
            && r.y >= p.y.min(q.y) - tolerance
            && r.y <= p.y.max(q.y) + tolerance
    };
    (values[0] > epsilon && values[1] < -epsilon || values[0] < -epsilon && values[1] > epsilon)
        && (values[2] > epsilon && values[3] < -epsilon
            || values[2] < -epsilon && values[3] > epsilon)
        || on(a, b, c, values[0])
        || on(a, b, d, values[1])
        || on(c, d, a, values[2])
        || on(c, d, b, values[3])
}

fn ring_contains(ring: &[[f64; 2]], point: [f64; 2]) -> bool {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .fold(false, |inside, (from, to)| {
            inside
                ^ ((from[1] > point[1]) != (to[1] > point[1])
                    && point[0]
                        < (to[0] - from[0]) * (point[1] - from[1]) / (to[1] - from[1]) + from[0])
        })
}

fn ring_strictly_contains(outer: &[[f64; 2]], inner: &[[f64; 2]]) -> bool {
    inner
        .iter()
        .copied()
        .find(|point| !point_on_boundary(outer, *point))
        .is_some_and(|point| ring_contains(outer, point))
}

fn point_on_boundary(ring: &[[f64; 2]], point: [f64; 2]) -> bool {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .any(|(&start, &end)| point_on_segment(start, end, point))
}

fn point_on_segment(start: [f64; 2], end: [f64; 2], point: [f64; 2]) -> bool {
    let (start, end, point) = (Vec2::from(start), Vec2::from(end), Vec2::from(point));
    let edge = end - start;
    let offset = point - start;
    let epsilon = 64.0 * f64::EPSILON * edge.length_squared().max(1.0);
    edge.cross(offset).abs() <= epsilon
        && point.x >= start.x.min(end.x) - epsilon
        && point.x <= start.x.max(end.x) + epsilon
        && point.y >= start.y.min(end.y) - epsilon
        && point.y <= start.y.max(end.y) + epsilon
}

fn root_of(mut index: usize, parent: &[Option<usize>]) -> usize {
    while let Some(next) = parent[index] {
        index = next;
    }
    index
}

fn triangulation_matches(points: &[[f64; 2]], triangles: &[[usize; 3]], expected: f64) -> bool {
    if triangles.is_empty() || !expected.is_finite() || expected <= 0.0 {
        return false;
    }
    let mut area = 0.0;
    for triangle in triangles {
        let Some((&a, &b, &c)) = points
            .get(triangle[0])
            .zip(points.get(triangle[1]))
            .zip(points.get(triangle[2]))
            .map(|((a, b), c)| (a, b, c))
        else {
            return false;
        };
        let twice = (Vec2::from(b) - Vec2::from(a)).cross(Vec2::from(c) - Vec2::from(a));
        if !twice.is_finite() || twice <= 0.0 {
            return false;
        }
        area += twice * 0.5;
    }
    (area - expected).abs() <= expected.max(1.0) * 1.0e-9
}

/// Triangles filling `outer`, with `holes` removed.
///
/// Each triangle is three indices into a single point list, which is returned
/// alongside: the outer ring first, then each hole in turn. A caller mapping
/// the result somewhere else — onto a surface, into a vertex buffer — then
/// transforms the points once rather than once per triangle.
///
/// Rings need not be closed by repeating their first point, and their winding
/// does not matter: the outer is taken counter-clockwise and holes clockwise,
/// whichever way they arrive.
pub fn polygon(outer: &[[f64; 2]], holes: &[Vec<[f64; 2]>]) -> (Vec<[f64; 2]>, Vec<[usize; 3]>) {
    let mut points: Vec<Vec2> = orient(outer, true);
    if points.len() < 3 {
        return (Vec::new(), Vec::new());
    }
    let mut ring: Vec<usize> = (0..points.len()).collect();

    // Holes are bridged into the outer ring in order of how far right they
    // reach, so an inner hole is joined to the ring the outer one already
    // became rather than to the original.
    let mut pending: Vec<Vec<Vec2>> = holes
        .iter()
        .map(|hole| orient(hole, false))
        .filter(|hole| hole.len() >= 3)
        .collect();
    pending.sort_by(|a, b| rightmost(b).1.x.total_cmp(&rightmost(a).1.x));
    for hole in pending {
        bridge(&mut points, &mut ring, hole);
    }

    let triangles = clip_ears(&points, ring);
    (points.into_iter().map(Vec2::to_array).collect(), triangles)
}

/// A ring as points, wound the way asked for.
fn orient(ring: &[[f64; 2]], counter_clockwise: bool) -> Vec<Vec2> {
    let mut points: Vec<Vec2> = ring.iter().copied().map(Vec2::from).collect();
    // A ring given closed carries its first point twice, which every step
    // below would treat as a zero-length edge.
    if points.len() > 1 && points[0] == points[points.len() - 1] {
        points.pop();
    }
    if (signed_area(&points) > 0.0) != counter_clockwise {
        points.reverse();
    }
    points
}

fn signed_area(points: &[Vec2]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let origin = points[0];
    (1..points.len() - 1)
        .map(|index| (points[index] - origin).cross(points[index + 1] - origin))
        .sum::<f64>()
        * 0.5
}

/// The index and position of a ring's rightmost point.
fn rightmost(ring: &[Vec2]) -> (usize, Vec2) {
    let mut best = 0;
    for (index, point) in ring.iter().enumerate() {
        if point.x > ring[best].x {
            best = index;
        }
    }
    (best, ring[best])
}

/// Joins a hole into the ring with a two-edge bridge.
///
/// The bridge runs from the hole's rightmost point to a vertex of the ring
/// that can see it. Rightmost because every other vertex of the hole is to
/// its left, so nothing of the hole itself is in the way.
///
/// Finding the far end is the part that has to be done properly. "The nearest
/// vertex nothing crosses" is not enough: with two holes it happily picks a
/// vertex of the other one and runs the bridge straight through it, crossing
/// no edge and enclosing the wrong region — which shows up as a ring with no
/// ear anywhere and no triangles at all.
///
/// So cast a ray to the right, insert its first crossing into the ring, and
/// bridge straight to that point. Giving each hole its own crossing also keeps
/// aligned holes from sharing a distant ring vertex and producing overlapping
/// bridge slits.
fn bridge(points: &mut Vec<Vec2>, ring: &mut Vec<usize>, hole: Vec<Vec2>) {
    let (start, from) = rightmost(&hole);
    let Some(at) = bridge_target(points, ring, from) else {
        return;
    };

    let base = points.len();
    points.extend(hole.iter().copied());

    // Out along the bridge, once round the hole, and back. The two coincident
    // edges are a slit of no width, which ear clipping then treats like any
    // other part of the ring.
    let mut spliced: Vec<usize> = Vec::with_capacity(ring.len() + hole.len() + 2);
    spliced.extend_from_slice(&ring[..=at]);
    for step in 0..hole.len() {
        spliced.push(base + (start + step) % hole.len());
    }
    spliced.push(base + start);
    spliced.push(ring[at]);
    spliced.extend_from_slice(&ring[at + 1..]);
    *ring = spliced;
}

/// Inserts the first point where a ray to the right meets the ring.
fn bridge_target(points: &mut Vec<Vec2>, ring: &mut Vec<usize>, from: Vec2) -> Option<usize> {
    let count = ring.len();
    let mut nearest = f64::INFINITY;
    let mut hit_edge = None;
    for index in 0..count {
        let next = (index + 1) % count;
        let (a, b) = (points[ring[index]], points[ring[next]]);
        // Only an edge straddling the ray's height can be hit, and one lying
        // along it cannot be hit at a single place.
        if (a.y > from.y) == (b.y > from.y) {
            continue;
        }
        let along = (from.y - a.y) / (b.y - a.y);
        let x = a.x + (b.x - a.x) * along;
        if x >= from.x && x < nearest {
            nearest = x;
            hit_edge = Some((index, next));
        }
    }
    let (first, second) = hit_edge?;
    let crossing = Vec2::new(nearest, from.y);
    if points[ring[first]] == crossing {
        return Some(first);
    }
    if points[ring[second]] == crossing {
        return Some(second);
    }
    let index = points.len();
    points.push(crossing);
    ring.insert(first + 1, index);
    Some(first + 1)
}

/// Cuts corners off until nothing is left.
/// Cuts ears until three corners remain.
///
/// The ear cut each round is the one with the smallest (shape score, ring
/// position) among all corners that are ears, the score normalised by the
/// bounds of the ring as it is at that moment. Finding it by rescanning the
/// ring every round is cubic, which a hatch fill with a few hundred bridged
/// holes (tens of thousands of corners) turns into minutes. The same choice
/// is made here with three structures:
///
/// * a linked ring, so cutting a corner does not shift the rest;
/// * a heap of convex corners by (score, position) with stale entries skipped
///   — a cut only changes its two neighbours, and only a change of the ring's
///   bounds rescales every score, which rebuilds the heap;
/// * a grid of the ring's points, so the containment test of a candidate
///   looks at the points near its triangle instead of all of them.
///
/// The predicate per candidate is unchanged, so the result is the same
/// triangulation, triangle for triangle ([`clip_ears_reference`] in the tests).
fn clip_ears(points: &[Vec2], ring: Vec<usize>) -> Vec<[usize; 3]> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    let total = ring.len();
    if total < 3 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(total - 2);
    let mut previous: Vec<usize> = (0..total).map(|slot| (slot + total - 1) % total).collect();
    let mut next: Vec<usize> = (0..total).map(|slot| (slot + 1) % total).collect();
    let mut alive = vec![true; total];
    let mut version = vec![0u32; total];
    let mut remaining = total;
    let point = |slot: usize| points[ring[slot]];

    let bounds_of = |alive: &[bool]| {
        let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
        for slot in (0..total).filter(|slot| alive[*slot]) {
            let p = point(slot);
            bounds[0][0] = bounds[0][0].min(p.x);
            bounds[0][1] = bounds[0][1].max(p.x);
            bounds[1][0] = bounds[1][0].min(p.y);
            bounds[1][1] = bounds[1][1].max(p.y);
        }
        bounds
    };
    let scale_of = |bounds: [[f64; 2]; 2]| {
        [
            (bounds[0][1] - bounds[0][0]).max(f64::MIN_POSITIVE),
            (bounds[1][1] - bounds[1][0]).max(f64::MIN_POSITIVE),
        ]
    };
    let mut bounds = bounds_of(&alive);
    let mut scale = scale_of(bounds);

    // Uniform grid over the initial bounds; points never move, cut ones are skipped.
    let side = (total as f64).sqrt().ceil().max(1.0) as usize;
    let origin = [bounds[0][0], bounds[1][0]];
    let extent = [
        (bounds[0][1] - bounds[0][0]).max(f64::MIN_POSITIVE),
        (bounds[1][1] - bounds[1][0]).max(f64::MIN_POSITIVE),
    ];
    let cell_of = |value: f64, axis: usize| -> usize {
        let at = ((value - origin[axis]) / extent[axis] * side as f64).floor();
        if at.is_nan() || at < 0.0 {
            0
        } else {
            (at as usize).min(side - 1)
        }
    };
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); side * side];
    for slot in 0..total {
        let p = point(slot);
        grid[cell_of(p.y, 1) * side + cell_of(p.x, 0)].push(slot);
    }

    #[derive(PartialEq)]
    struct Entry {
        score: f64,
        slot: usize,
        version: u32,
    }
    impl Eq for Entry {}
    impl PartialOrd for Entry {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for Entry {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.score
                .total_cmp(&other.score)
                .then_with(|| self.slot.cmp(&other.slot))
        }
    }

    let corners = |slot: usize, previous: &[usize], next: &[usize]| {
        [point(previous[slot]), point(slot), point(next[slot])]
    };
    let score_of = |[a, b, c]: [Vec2; 3], scale: [f64; 2]| {
        let corners = [a, b, c];
        (0..3)
            .map(|index| {
                let following = (index + 1) % 3;
                let dx = (corners[following].x - corners[index].x) / scale[0];
                let dy = (corners[following].y - corners[index].y) / scale[1];
                dx * dx + dy * dy
            })
            .fold(0.0, f64::max)
    };

    let mut heap: BinaryHeap<Reverse<Entry>> = BinaryHeap::new();
    let fill = |heap: &mut BinaryHeap<Reverse<Entry>>,
                alive: &[bool],
                version: &[u32],
                previous: &[usize],
                next: &[usize],
                scale: [f64; 2]| {
        heap.clear();
        for slot in (0..total).filter(|slot| alive[*slot]) {
            let shape = corners(slot, previous, next);
            if convex_corner(shape) {
                heap.push(Reverse(Entry {
                    score: score_of(shape, scale),
                    slot,
                    version: version[slot],
                }));
            }
        }
    };
    fill(&mut heap, &alive, &version, &previous, &next, scale);

    let mut deferred = Vec::new();
    while remaining > 3 {
        let mut cut = None;
        while let Some(Reverse(entry)) = heap.pop() {
            if !alive[entry.slot] || entry.version != version[entry.slot] {
                continue;
            }
            let [a, b, c] = corners(entry.slot, &previous, &next);
            let (before, after) = (previous[entry.slot], next[entry.slot]);
            let reach = 1e-9 * (extent[0] + extent[1]);
            let (low_x, high_x) = (a.x.min(b.x).min(c.x) - reach, a.x.max(b.x).max(c.x) + reach);
            let (low_y, high_y) = (a.y.min(b.y).min(c.y) - reach, a.y.max(b.y).max(c.y) + reach);
            let blocked = (cell_of(low_y, 1)..=cell_of(high_y, 1)).any(|row| {
                (cell_of(low_x, 0)..=cell_of(high_x, 0)).any(|column| {
                    grid[row * side + column].iter().any(|&other| {
                        if !alive[other] || other == entry.slot || other == before || other == after
                        {
                            return false;
                        }
                        let p = point(other);
                        p != a && p != b && p != c && inside(a, b, c, p)
                    })
                })
            });
            if blocked {
                deferred.push(entry);
            } else {
                cut = Some(entry.slot);
                break;
            }
        }
        // No ear anywhere: the ring crosses itself and there is no
        // triangulation to find.
        let Some(at) = cut else {
            return Vec::new();
        };
        let (before, after) = (previous[at], next[at]);
        out.push([ring[before], ring[at], ring[after]]);
        alive[at] = false;
        remaining -= 1;
        next[before] = after;
        previous[after] = before;
        version[before] += 1;
        version[after] += 1;
        let gone = point(at);
        if gone.x == bounds[0][0]
            || gone.x == bounds[0][1]
            || gone.y == bounds[1][0]
            || gone.y == bounds[1][1]
        {
            bounds = bounds_of(&alive);
            let rescaled = scale_of(bounds);
            if rescaled != scale {
                scale = rescaled;
                deferred.clear();
                fill(&mut heap, &alive, &version, &previous, &next, scale);
                continue;
            }
        }
        heap.extend(deferred.drain(..).map(Reverse));
        for slot in [before, after] {
            let shape = corners(slot, &previous, &next);
            if convex_corner(shape) {
                heap.push(Reverse(Entry {
                    score: score_of(shape, scale),
                    slot,
                    version: version[slot],
                }));
            }
        }
    }
    let last: Vec<usize> = (0..total).filter(|slot| alive[*slot]).collect();
    if last.len() == 3 {
        out.push([ring[last[0]], ring[last[1]], ring[last[2]]]);
    }
    out
}

/// The original selection loop, kept as the oracle for [`clip_ears`].
#[cfg(test)]
fn clip_ears_reference(points: &[Vec2], mut ring: Vec<usize>) -> Vec<[usize; 3]> {
    let mut out = Vec::with_capacity(ring.len().saturating_sub(2));
    let mut candidates = Vec::new();
    // Every ear removes one vertex, so the loop cannot run longer than the
    // ring — but a polygon with no ear at all (self-intersecting) would spin,
    // so the failures are counted rather than assumed away.
    let mut failures = 0;
    while ring.len() > 3 {
        if failures > ring.len() {
            // No ear anywhere: the ring crosses itself and there is no
            // triangulation to find.
            return Vec::new();
        }
        let count = ring.len();
        let bounds = ring.iter().fold(
            [[f64::INFINITY, f64::NEG_INFINITY]; 2],
            |mut bounds, index| {
                let point = points[*index];
                bounds[0][0] = bounds[0][0].min(point.x);
                bounds[0][1] = bounds[0][1].max(point.x);
                bounds[1][0] = bounds[1][0].min(point.y);
                bounds[1][1] = bounds[1][1].max(point.y);
                bounds
            },
        );
        let scale = [
            (bounds[0][1] - bounds[0][0]).max(f64::MIN_POSITIVE),
            (bounds[1][1] - bounds[1][0]).max(f64::MIN_POSITIVE),
        ];
        let cut = if count <= 4 {
            // Triangles and quads do not need a candidate allocation.
            (0..count)
                .filter(|at| is_ear(points, &ring, *at))
                .min_by(|a, b| {
                    ear_score(points, &ring, *a, scale)
                        .total_cmp(&ear_score(points, &ring, *b, scale))
                })
        } else {
            candidates.clear();
            candidates.extend(
                (0..count)
                    .filter(|&at| convex_corner(ear_corners(points, &ring, at)))
                    .map(|at| (at, ear_score(points, &ring, at, scale))),
            );
            let mut cut = None;
            while let Some(candidate_index) = (0..candidates.len()).min_by(|&a, &b| {
                // swap_remove changes storage order; ties still use ring order.
                candidates[a]
                    .1
                    .total_cmp(&candidates[b].1)
                    .then_with(|| candidates[a].0.cmp(&candidates[b].0))
            }) {
                let (at, _) = candidates.swap_remove(candidate_index);
                if is_ear(points, &ring, at) {
                    cut = Some(at);
                    break;
                }
            }
            cut
        };
        match cut {
            Some(at) => {
                let count = ring.len();
                out.push([
                    ring[(at + count - 1) % count],
                    ring[at],
                    ring[(at + 1) % count],
                ]);
                ring.remove(at);
                failures = 0;
            }
            None => failures += 1,
        }
    }
    if ring.len() == 3 {
        out.push([ring[0], ring[1], ring[2]]);
    }
    out
}

fn ear_score(points: &[Vec2], ring: &[usize], at: usize, scale: [f64; 2]) -> f64 {
    let corners = ear_corners(points, ring, at);
    (0..3)
        .map(|index| {
            let next = (index + 1) % 3;
            let dx = (corners[next].x - corners[index].x) / scale[0];
            let dy = (corners[next].y - corners[index].y) / scale[1];
            dx * dx + dy * dy
        })
        .fold(0.0, f64::max)
}

fn ear_corners(points: &[Vec2], ring: &[usize], at: usize) -> [Vec2; 3] {
    let count = ring.len();
    [
        points[ring[(at + count - 1) % count]],
        points[ring[at]],
        points[ring[(at + 1) % count]],
    ]
}

fn convex_corner([a, b, c]: [Vec2; 3]) -> bool {
    (b - a).cross(c - b) > 0.0
}

/// Whether the corner at `at` can be cut off.
fn is_ear(points: &[Vec2], ring: &[usize], at: usize) -> bool {
    let count = ring.len();
    let [a, b, c] = ear_corners(points, ring, at);
    // A reflex corner is not an ear, and neither is a collapsed one: cutting
    // either produces a triangle outside the polygon or none at all.
    if !convex_corner([a, b, c]) {
        return false;
    }
    // And nothing else may be inside the triangle, or the cut would swallow
    // part of the polygon.
    //
    // A vertex that *is* one of the corners is skipped by position and by
    // place. Bridging a hole leaves the ring visiting two points twice, and
    // those copies sit exactly on the triangle's edges — counted as inside,
    // they reject every candidate and nothing is ever an ear.
    (0..count).all(|other| {
        if other == at || other == (at + count - 1) % count || other == (at + 1) % count {
            return true;
        }
        let point = points[ring[other]];
        if point == a || point == b || point == c {
            return true;
        }
        !inside(a, b, c, point)
    })
}

/// Whether a point is within a triangle, edges included.
fn inside(a: Vec2, b: Vec2, c: Vec2, point: Vec2) -> bool {
    let side = |p: Vec2, q: Vec2| (q - p).cross(point - p);
    side(a, b) >= 0.0 && side(b, c) >= 0.0 && side(c, a) >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(points: &[[f64; 2]], triangles: &[[usize; 3]]) -> f64 {
        triangles
            .iter()
            .map(|triangle| {
                let a = Vec2::from(points[triangle[0]]);
                let b = Vec2::from(points[triangle[1]]);
                let c = Vec2::from(points[triangle[2]]);
                (b - a).cross(c - a).abs() * 0.5
            })
            .sum()
    }

    fn square(size: f64) -> Vec<[f64; 2]> {
        vec![[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]]
    }

    /// Filling many separate rings sets each ring against its neighbours,
    /// not against every other ring. Going through every pair made a fill
    /// of ten thousand loops take seconds, and four times as long for twice
    /// as many.
    #[test]
    fn separate_rings_are_set_against_their_neighbours_only() {
        const RINGS: usize = 400;
        let size = |index: usize| (4.0 + (index % 5) as f64, 3.0 + (index % 4) as f64);
        let input: Vec<Vec<[f64; 2]>> = (0..RINGS)
            .map(|index| {
                let (x, y) = ((index % 20) as f64 * 10.0, (index / 20) as f64 * 10.0);
                let (w, h) = size(index);
                vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
            })
            .collect();
        let pairs = || RING_PAIRS.with(std::cell::Cell::get);
        let before = pairs();
        let depths = nesting_depths(&input);
        let (points, triangles) = rings(&input);
        let compared = pairs() - before;

        assert_eq!(depths, vec![0; RINGS]);
        assert_eq!(triangles.len(), 2 * RINGS);
        let expected: f64 = (0..RINGS).map(size).map(|(w, h)| w * h).sum();
        assert!((area(&points, &triangles) - expected).abs() < 1.0e-6);
        assert!(
            compared <= 40 * RINGS,
            "{compared} pairs of rings were compared to fill {RINGS} separate rings"
        );
    }

    /// Bridged ring for an outer ring and holes, as `polygon` builds it.
    fn bridged(outer: &[[f64; 2]], holes: &[Vec<[f64; 2]>]) -> (Vec<Vec2>, Vec<usize>) {
        let mut points: Vec<Vec2> = orient(outer, true);
        let mut ring: Vec<usize> = (0..points.len()).collect();
        let mut pending: Vec<Vec<Vec2>> = holes
            .iter()
            .map(|hole| orient(hole, false))
            .filter(|hole| hole.len() >= 3)
            .collect();
        pending.sort_by(|a, b| rightmost(b).1.x.total_cmp(&rightmost(a).1.x));
        for hole in pending {
            bridge(&mut points, &mut ring, hole);
        }
        (points, ring)
    }

    /// The pair loop [`nesting_depths`] ran before it asked [`RingGrid`]:
    /// every ring against every other. Kept as its parity reference.
    fn nesting_depths_reference(input: &[Vec<[f64; 2]>]) -> Vec<usize> {
        let rings: Vec<Vec<[f64; 2]>> = input.iter().map(|ring| sanitize(ring)).collect();
        let areas: Vec<f64> = rings
            .iter()
            .map(|ring| signed_area_arrays(ring).abs())
            .collect();
        rings
            .iter()
            .enumerate()
            .map(|(index, ring)| {
                rings
                    .iter()
                    .enumerate()
                    .filter(|(other, outer)| {
                        *other != index
                            && areas[*other] > areas[index]
                            && ring_strictly_contains(outer, ring)
                    })
                    .count()
            })
            .collect()
    }

    /// Rings of very different sizes dropped at random over the same patch:
    /// some apart, some inside others, some across each other, and some
    /// sharing a corner or an edge with the one before.
    fn scattered_rings(random: &mut Lcg, count: usize) -> Vec<Vec<[f64; 2]>> {
        let mut rings: Vec<Vec<[f64; 2]>> = Vec::with_capacity(count);
        for index in 0..count {
            let size = match index % 4 {
                0 => 30.0 + 40.0 * random.next(),
                1 => 5.0 + 10.0 * random.next(),
                _ => 0.5 + 3.0 * random.next(),
            };
            let centre = [100.0 * random.next(), 100.0 * random.next()];
            let ring = match index % 5 {
                // A box that starts at a corner of the ring before it.
                0 if index > 0 => {
                    let [x, y] = rings[index - 1][0];
                    vec![[x, y], [x + size, y], [x + size, y + size], [x, y + size]]
                }
                1 => star(random, centre, size, 7, 0.4),
                _ => {
                    let [x, y] = centre;
                    let (w, h) = (size, size * (0.3 + random.next()));
                    vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
                }
            };
            rings.push(ring);
        }
        rings
    }

    #[test]
    fn nesting_depths_match_the_pair_loop() {
        let mut random = Lcg(0x2e5c_91d7);
        let mut nested = 0;
        for round in 0..200 {
            let input = scattered_rings(&mut random, 4 + round % 60);
            let depths = nesting_depths(&input);
            assert_eq!(depths, nesting_depths_reference(&input), "round {round}");
            nested += depths.iter().filter(|depth| **depth > 0).count();
        }
        assert!(nested > 1_000, "the rings must nest: {nested} did");
        // Rings the grid cannot place, and none at all.
        let odd = vec![
            Vec::new(),
            vec![[1.0, 1.0]],
            vec![[0.0, 0.0], [f64::NAN, 1.0], [1.0, 1.0]],
            vec![
                [-1.0e300, -1.0e300],
                [1.0e300, -1.0e300],
                [1.0e300, 1.0e300],
                [-1.0e300, 1.0e300],
            ],
            square(4.0),
        ];
        assert_eq!(nesting_depths(&odd), nesting_depths_reference(&odd));
        assert_eq!(nesting_depths(&[]), Vec::<usize>::new());
    }

    #[test]
    fn the_fill_is_the_same_with_the_grid_as_over_every_ring() {
        let mut random = Lcg(0x71c3_0b5f);
        let mut filled = 0;
        for round in 0..120 {
            let input = scattered_rings(&mut random, 4 + round % 50);
            let with_grid = rings(&input);
            EVERY_RING.with(|every| every.set(true));
            let over_every_ring = rings(&input);
            EVERY_RING.with(|every| every.set(false));
            assert_eq!(with_grid, over_every_ring, "round {round}");
            filled += usize::from(!with_grid.1.is_empty());
        }
        assert!(filled > 100, "the rings must fill: {filled} rounds did");
    }

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn star(
        random: &mut Lcg,
        centre: [f64; 2],
        radius: f64,
        corners: usize,
        jitter: f64,
    ) -> Vec<[f64; 2]> {
        (0..corners)
            .map(|index| {
                let angle = index as f64 / corners as f64 * std::f64::consts::TAU;
                let r = radius * (1.0 - jitter * random.next());
                [centre[0] + r * angle.cos(), centre[1] + r * angle.sin()]
            })
            .collect()
    }

    /// The spike sweep as it was written before it became single-pass: an
    /// index walk with a `Vec::remove` per spike and a one-step backtrack.
    /// Kept as the parity reference for the linear sweep in [`super::sanitize`].
    fn sanitize_reference(ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
        if ring.iter().flatten().any(|value| !value.is_finite()) {
            return Vec::new();
        }
        let mut points = Vec::with_capacity(ring.len());
        for point in ring {
            if points.last() != Some(point) {
                points.push(*point);
            }
        }
        while points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < 3 {
            return points;
        }
        let spike = |before: [f64; 2], here: [f64; 2], after: [f64; 2]| -> bool {
            let incoming = Vec2::from(before) - Vec2::from(here);
            let outgoing = Vec2::from(after) - Vec2::from(here);
            let length = incoming.length().max(outgoing.length()).max(1.0);
            let epsilon = 64.0 * f64::EPSILON * length * length;
            !(incoming.cross(outgoing).abs() <= epsilon && incoming.dot(outgoing) > 0.0)
        };
        let mut index = 0;
        while points.len() >= 3 && index < points.len() {
            let count = points.len();
            let before = points[(index + count - 1) % count];
            let here = points[index];
            let after = points[(index + 1) % count];
            if spike(before, here, after) {
                index += 1;
                continue;
            }
            points.remove(index);
            let count = points.len();
            let previous = (index + count - 1) % count;
            if count > 1 && points[previous] == points[index % count] {
                points.remove(index % count);
            }
            index = index.saturating_sub(1);
        }
        points
    }

    #[test]
    fn linear_sanitize_matches_the_reference() {
        // Plain shapes: no spikes, output equals input (modulo dedup).
        for ring in [
            vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
            vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
            // Consecutive duplicates and a closing duplicate.
            vec![
                [0.0, 0.0],
                [0.0, 0.0],
                [4.0, 0.0],
                [4.0, 0.0],
                [4.0, 4.0],
                [0.0, 4.0],
                [0.0, 4.0],
                [0.0, 0.0],
            ],
            // Zero-area spikes in the middle.
            vec![[0.0, 0.0], [2.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
            // A spike chain that crosses the seam.
            vec![
                [4.0, 4.0],
                [4.0, 0.0],
                [4.0, 0.0],
                [6.0, 0.0],
                [8.0, 0.0],
                [8.0, 0.0],
                [0.0, 0.0],
                [0.0, 4.0],
            ],
            // A spike at the head and at the tail.
            vec![[2.0, 0.0], [0.0, 0.0], [4.0, 0.0], [4.0, 4.0]],
            // Degenerate: fewer than three distinct points.
            vec![[0.0, 0.0], [1.0, 1.0]],
            // Non-finite input.
            vec![[0.0, 0.0], [f64::NAN, 0.0], [4.0, 4.0]],
        ] {
            assert_eq!(
                super::sanitize(&ring),
                sanitize_reference(&ring),
                "mismatch on {ring:?}"
            );
        }

        // Randomised shapes with injected folds. Each point is followed by a
        // fold back to itself (…a, b, a…), so every second corner is a
        // zero-area spike and the reference's quadratic sweep is exercised.
        let mut random = Lcg(17);
        for case in 0..200 {
            let corners = 3 + (case * 7) % 400;
            let clean = star(&mut random, [0.0, 0.0], 100.0, corners, 0.3);
            let mut folded: Vec<[f64; 2]> = Vec::with_capacity(clean.len() * 3);
            for index in 0..clean.len() {
                let next = clean[(index + 1) % clean.len()];
                folded.push(clean[index]);
                folded.push(next);
                folded.push(clean[index]);
            }
            assert_eq!(
                super::sanitize(&folded),
                sanitize_reference(&folded),
                "mismatch on case {case} ring of {} points",
                folded.len()
            );
        }

        // A ring shaped like the cadastral parcel boundary that made opens
        // take minutes: about a million points, a third of them folded-back
        // spikes. The old sweep needs over two minutes on such a ring; the
        // sweep in `sanitize` must answer in a bounded, near-linear time and
        // keep the covered area.
        let clean = star(&mut Lcg(31), [0.0, 0.0], 1000.0, 269, 0.2);
        let mut spiky: Vec<[f64; 2]> = Vec::with_capacity(1_310_970);
        for index in 0..clean.len() {
            let step = (index + 1) % clean.len();
            let [x0, y0] = clean[index];
            let [x1, y1] = clean[step];
            for sub in 0..1000usize {
                let t = sub as f64 / 1000.0;
                let point = [x0 + (x1 - x0) * t, y0 + (y1 - y0) * t];
                let back = [
                    x0 + (x1 - x0) * (t - 1.0 / 1000.0),
                    y0 + (y1 - y0) * (t - 1.0 / 1000.0),
                ];
                spiky.push(point);
                spiky.push(back);
                spiky.push(point);
            }
        }
        let started = std::time::Instant::now();
        let sanitized = super::sanitize(&spiky);
        let elapsed = started.elapsed();
        assert!(elapsed.as_secs() < 6, "sanitize is not linear: {elapsed:?}");
        // No spike and no duplicate survives.
        for index in 0..sanitized.len() {
            let count = sanitized.len();
            let before = sanitized[(index + count - 1) % count];
            let here = sanitized[index];
            let after = sanitized[(index + 1) % count];
            let incoming = Vec2::from(before) - Vec2::from(here);
            let outgoing = Vec2::from(after) - Vec2::from(here);
            assert!(incoming.dot(outgoing) <= 0.0 || incoming.cross(outgoing).abs() > 0.0);
        }
        // The spikes contributed no area: the sanitized ring covers the same
        // region as the clean star it was built from.
        let clean_area =
            signed_area(&clean.iter().map(|p| Vec2::from(*p)).collect::<Vec<_>>()).abs();
        let sanitized_area =
            signed_area(&sanitized.iter().map(|p| Vec2::from(*p)).collect::<Vec<_>>()).abs();
        assert!(
            (clean_area - sanitized_area).abs() <= clean_area * 0.02,
            "area changed: {clean_area} -> {sanitized_area}"
        );
    }

    #[test]
    fn faster_ear_clipping_cuts_the_same_ears_as_the_reference() {
        let mut random = Lcg(7);
        for case in 0..60 {
            let corners = 12 + case * 7;
            let outer = star(
                &mut random,
                [0.0, 0.0],
                100.0,
                corners,
                if case % 3 == 0 { 0.0 } else { 0.5 },
            );
            let mut holes = Vec::new();
            let per_side = case % 6;
            for row in 0..per_side {
                for column in 0..per_side {
                    let span = 80.0 / per_side.max(1) as f64;
                    let centre = [
                        -40.0 + span * (column as f64 + 0.5),
                        -40.0 + span * (row as f64 + 0.5),
                    ];
                    // Aligned holes on even cases, jittered ones on odd cases.
                    let shift = if case % 2 == 0 {
                        0.0
                    } else {
                        (random.next() - 0.5) * span * 0.2
                    };
                    holes.push(star(
                        &mut random,
                        [centre[0] + shift, centre[1]],
                        span * 0.3,
                        5 + (row + column) % 7,
                        0.4,
                    ));
                }
            }
            let (points, ring) = bridged(&outer, &holes);
            assert_eq!(
                clip_ears(&points, ring.clone()),
                clip_ears_reference(&points, ring),
                "case {case}"
            );
        }
    }

    #[test]
    fn faster_ear_clipping_matches_the_reference_on_ties_and_slivers() {
        // Equal scores everywhere (regular grid ring) and a nearly degenerate sliver.
        let grid: Vec<[f64; 2]> = (0..10)
            .map(|x| [x as f64, 0.0])
            .chain((0..10).map(|y| [9.0, y as f64]))
            .chain((0..10).map(|x| [9.0 - x as f64, 9.0]))
            .chain((0..10).map(|y| [0.0, 9.0 - y as f64]))
            .collect();
        let sliver = vec![
            [0.0, 0.0],
            [1000.0, 0.0],
            [1000.0, 1e-6],
            [500.0, 2e-6],
            [0.0, 1e-6],
        ];
        for outer in [grid, sliver] {
            let (points, ring) = bridged(&outer, &[]);
            assert_eq!(
                clip_ears(&points, ring.clone()),
                clip_ears_reference(&points, ring)
            );
        }
    }

    #[test]
    fn hundreds_of_holes_are_filled_without_blowing_up() {
        // A hatch fill with a few hundred bridged holes used to take minutes.
        let mut random = Lcg(11);
        let outer = star(&mut random, [0.0, 0.0], 1000.0, 2000, 0.02);
        let mut holes = Vec::new();
        for row in 0..18 {
            for column in 0..18 {
                let centre = [-600.0 + 70.0 * column as f64, -600.0 + 70.0 * row as f64];
                holes.push(star(&mut random, centre, 20.0, 24, 0.3));
            }
        }
        let start = std::time::Instant::now();
        let (points, triangles) = polygon(&outer, &holes);
        let expected = signed_area_arrays(&outer).abs()
            - holes
                .iter()
                .map(|hole| signed_area_arrays(hole).abs())
                .sum::<f64>();
        assert!(triangulation_matches(&points, &triangles, expected));
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_square_becomes_two_triangles_covering_it() {
        let (points, triangles) = polygon(&square(10.0), &[]);
        assert_eq!(triangles.len(), 2);
        assert!((area(&points, &triangles) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn equal_scores_keep_the_first_valid_ear() {
        let points: Vec<Vec2> = square(10.0).into_iter().map(Vec2::from).collect();
        assert_eq!(
            clip_ears(&points, vec![0, 1, 2, 3]),
            vec![[3, 0, 1], [1, 2, 3]]
        );
        // Check the complete output, including the final triangle/quad scan.
        let corners = square(10.0);
        let points: Vec<Vec2> = (0..4)
            .flat_map(|side| {
                let a = corners[side];
                let b = corners[(side + 1) % 4];
                (0..5).map(move |i| {
                    Vec2::from([
                        a[0] + (b[0] - a[0]) * i as f64 / 5.0,
                        a[1] + (b[1] - a[1]) * i as f64 / 5.0,
                    ])
                })
            })
            .collect();
        assert_eq!(
            clip_ears(&points, (0..20).collect()),
            [
                [19, 0, 1],
                [4, 5, 6],
                [9, 10, 11],
                [14, 15, 16],
                [19, 1, 2],
                [3, 4, 6],
                [8, 9, 11],
                [13, 14, 16],
                [3, 6, 7],
                [8, 11, 12],
                [13, 16, 17],
                [18, 19, 2],
                [18, 2, 3],
                [3, 7, 8],
                [8, 12, 13],
                [13, 17, 18],
                [18, 3, 8],
                [8, 13, 18],
            ]
        );
    }

    #[test]
    fn winding_does_not_matter() {
        let mut backwards = square(10.0);
        backwards.reverse();
        let (points, triangles) = polygon(&backwards, &[]);
        assert!((area(&points, &triangles) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_ring_closed_by_repeating_its_first_point_is_not_counted_twice() {
        let mut closed = square(10.0);
        closed.push(closed[0]);
        let (points, triangles) = polygon(&closed, &[]);
        assert_eq!(triangles.len(), 2, "a repeated point is not a vertex");
        assert!((area(&points, &triangles) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_concave_polygon_is_filled_without_leaving_its_outline() {
        // An L, whose reflex corner is not an ear and must not be cut.
        let shape = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 4.0],
            [4.0, 4.0],
            [4.0, 10.0],
            [0.0, 10.0],
        ];
        let (points, triangles) = polygon(&shape, &[]);
        assert_eq!(triangles.len(), 4, "six vertices, four triangles");
        assert!((area(&points, &triangles) - 64.0).abs() < 1e-9);
    }

    #[test]
    fn a_hole_is_left_empty() {
        let hole = vec![[3.0, 3.0], [7.0, 3.0], [7.0, 7.0], [3.0, 7.0]];
        let (points, triangles) = polygon(&square(10.0), &[hole]);
        assert!(!triangles.is_empty());
        // A hundred less the sixteen the hole takes out.
        assert!(
            (area(&points, &triangles) - 84.0).abs() < 1e-9,
            "{}",
            area(&points, &triangles)
        );
    }

    #[test]
    fn two_holes_are_both_left_empty() {
        let holes = vec![
            vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]],
            vec![[6.0, 6.0], [9.0, 6.0], [9.0, 9.0], [6.0, 9.0]],
        ];
        let (points, triangles) = polygon(&square(10.0), &holes);
        assert!((area(&points, &triangles) - (100.0 - 4.0 - 9.0)).abs() < 1e-9);
    }

    #[test]
    fn two_aligned_holes_fit_inside_a_narrow_concave_arm() {
        let outer = vec![
            [0.0, 0.0],
            [12.0, 0.0],
            [12.0, 8.0],
            [11.0, 8.0],
            [11.0, 2.0],
            [2.0, 2.0],
            [1.8, 2.1],
            [1.8, 12.0],
            [0.0, 12.0],
        ];
        let circle = |centre: [f64; 2]| {
            (0..24)
                .map(|step| {
                    let angle = std::f64::consts::TAU * step as f64 / 24.0;
                    [
                        centre[0] + 0.08 * angle.cos(),
                        centre[1] + 0.08 * angle.sin(),
                    ]
                })
                .collect::<Vec<_>>()
        };
        let holes = vec![circle([1.68, 6.0]), circle([1.68, 9.0])];
        let expected = signed_area_arrays(&outer).abs()
            - holes
                .iter()
                .map(|hole| signed_area_arrays(hole).abs())
                .sum::<f64>();

        let (points, triangles) = polygon(&outer, &holes);

        assert!(!triangles.is_empty());
        assert!((area(&points, &triangles) - expected).abs() < 1e-9);
    }

    #[test]
    fn every_triangle_stays_inside_the_polygon() {
        // The property area alone would not catch: a triangulation can have
        // the right total and still put a triangle outside and another twice
        // over.
        let shape = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 4.0],
            [4.0, 4.0],
            [4.0, 10.0],
            [0.0, 10.0],
        ];
        let (points, triangles) = polygon(&shape, &[]);
        for triangle in &triangles {
            let centre = (Vec2::from(points[triangle[0]])
                + Vec2::from(points[triangle[1]])
                + Vec2::from(points[triangle[2]]))
                / 3.0;
            let inside_l =
                (centre.x <= 10.0 && centre.y <= 4.0) || (centre.x <= 4.0 && centre.y <= 10.0);
            assert!(inside_l && centre.x >= 0.0 && centre.y >= 0.0, "{centre:?}");
        }
    }

    #[test]
    fn crossing_rings_fill_their_even_odd_region() {
        // Two 4x4 squares overlapping by 2x2: the overlap is left empty and
        // the rest filled, instead of both rings being dropped.
        let square = |x: f64, y: f64| vec![[x, y], [x + 4.0, y], [x + 4.0, y + 4.0], [x, y + 4.0]];
        let (points, triangles) = rings(&[square(0.0, 0.0), square(2.0, 2.0)]);
        let area: f64 = triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|vertex| Vec2::from(points[vertex]));
                (b - a).cross(c - a).abs() / 2.0
            })
            .sum();
        assert!((area - 24.0).abs() < 1e-9, "{area}");
    }

    #[test]
    fn a_self_crossing_ring_fills_its_lobes() {
        // A bow tie covers its two triangular lobes once each.
        let bowtie = vec![[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
        let (points, triangles) = rings(&[bowtie]);
        let filled = area(&points, &triangles);
        assert!((filled - 50.0).abs() < 1e-9, "{filled}");
    }

    #[test]
    fn a_self_crossing_island_leaves_the_rest_of_the_fill() {
        // As one invalid ring the bow tie took the square round it down too,
        // and nothing was filled. Now its lobes are holes in the square.
        let square = vec![[-5.0, -5.0], [15.0, -5.0], [15.0, 15.0], [-5.0, 15.0]];
        let bowtie = vec![[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
        let (points, triangles) = rings(&[bowtie, square]);
        let filled = area(&points, &triangles);
        assert!((filled - 350.0).abs() < 1e-9, "{filled}");
    }

    #[test]
    fn a_thin_self_crossing_island_at_survey_coordinates_leaves_the_fill() {
        // The roof hatch of OpenCADStudio#2549: a 100 x 50 face with an
        // island under four units high whose third edge crosses its first,
        // all half a million units from the origin.
        let (x, y) = (551_000.0, 4_246_500.0);
        let face = vec![[x, y], [x + 100.0, y], [x + 100.0, y + 50.0], [x, y + 50.0]];
        let island = vec![
            [x + 20.0, y + 40.0],
            [x + 55.0, y + 43.8],
            [x + 80.0, y + 42.6],
            [x + 0.5, y + 42.6],
        ];
        let (points, triangles) = rings(&[island, face]);
        let filled = area(&points, &triangles);
        // 5000 less the two lobes, 56.48 and 21.63.
        assert!((filled - 4921.887).abs() < 1e-2, "{filled}");
    }

    #[test]
    fn a_self_crossing_ring_terminates_rather_than_spinning() {
        // A bowtie has no inside, so no triangulation of it is right. What
        // matters is that ear clipping stops looking rather than searching a
        // ring it can never reduce — the loop counts its failures for this.
        let bowtie = vec![[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
        let (points, triangles) = polygon(&bowtie, &[]);
        for triangle in &triangles {
            let a = Vec2::from(points[triangle[0]]);
            let b = Vec2::from(points[triangle[1]]);
            let c = Vec2::from(points[triangle[2]]);
            assert!((b - a).cross(c - a).abs() > 0.0, "a collapsed triangle");
        }
    }

    #[test]
    fn too_few_points_is_no_polygon() {
        assert_eq!(polygon(&[], &[]).1.len(), 0);
        assert_eq!(polygon(&[[0.0, 0.0], [1.0, 0.0]], &[]).1.len(), 0);
    }

    #[test]
    fn a_polygon_at_survey_coordinates_still_covers_itself() {
        let origin = [512_345.678, 4_512_345.678];
        let shifted: Vec<[f64; 2]> = square(10.0)
            .iter()
            .map(|p| [origin[0] + p[0], origin[1] + p[1]])
            .collect();
        let (points, triangles) = polygon(&shifted, &[]);
        assert!((area(&points, &triangles) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn a_many_sided_ring_is_fully_triangulated() {
        // A circle's worth of vertices, which is what a curved face's
        // boundary comes in as.
        let ring: Vec<[f64; 2]> = (0..64)
            .map(|step| {
                let angle = std::f64::consts::TAU * step as f64 / 64.0;
                [10.0 * angle.cos(), 10.0 * angle.sin()]
            })
            .collect();
        let (points, triangles) = polygon(&ring, &[]);
        assert_eq!(triangles.len(), 62, "n − 2 triangles");
        let expected = 0.5 * 64.0 * 100.0 * (std::f64::consts::TAU / 64.0).sin();
        assert!((area(&points, &triangles) - expected).abs() < 1e-6);
    }
}
