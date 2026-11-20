use super::*;

pub(crate) fn hatch_is_standalone_circle(hatch: &document::entities::Hatch) -> bool {
    if hatch.paths.len() != 1 {
        return false;
    }
    let edges = &hatch.paths[0].edges;
    if edges.len() != 1 {
        return false;
    }
    let document::entities::BoundaryEdge::CircularArc(arc) = &edges[0] else {
        return false;
    };
    arc.radius.is_finite()
        && arc.radius > 0.0
        && ((arc.end_angle - arc.start_angle) - std::f64::consts::TAU).abs() < 1.0e-6
}

/// Cut a polygonal HATCH with an exact CIRCLE: create an internal round
/// opening, or replace part of its outer boundary with an analytic arc.
/// Do not approximate the final arc with chords (DWG must retain a true arc).
/// Return None for complex topology that would require a region boolean.

pub(crate) fn trim_hatch_circle(
    hatch: &document::entities::Hatch,
    rings: &[Vec<[f64; 2]>],
    outer_index: usize,
    circle: &Geo,
    pick: [f64; 2],
) -> Option<document::entities::Hatch> {
    use document::entities::{
        BoundaryEdge, BoundaryPath, BoundaryPathFlags, CircularArcEdge, LineEdge,
    };
    use document::types::Vector2;
    use std::f64::consts::TAU;
    let Geo::Circle { handle, cx, cy, r } = *circle else {
        return None;
    };
    if handle == hatch.common.handle || !r.is_finite() || r <= 1.0e-9 {
        return None;
    }
    let centre = [cx, cy];
    let d2 = |p: [f64; 2]| (p[0] - cx).powi(2) + (p[1] - cy).powi(2);
    let tolerance = 1.0e-8_f64.max(r * 1.0e-9);
    let r2 = r * r;
    // A click INSIDE the cutter removes the circular portion of the HATCH.
    // Clicking outside must not unexpectedly remove the entire hatch.
    if d2(pick) >= (r - tolerance).max(0.0).powi(2) {
        return None;
    }
    let polygon_contains = |ring: &[[f64; 2]], p: [f64; 2]| {
        let mut inside = false;
        for (a, b) in ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .take(ring.len())
        {
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let cross_x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
                if p[0] < cross_x {
                    inside = !inside;
                }
            }
        }
        inside
    };
    let ring = &rings[outer_index];
    if ring.len() < 3 || !polygon_contains(ring, pick) {
        return None;
    }
    // The incoming tessellation can repeat the first point at the end.
    let mut points = ring.clone();
    if points.len() >= 2
        && (points[0][0] - points.last().unwrap()[0])
            .hypot(points[0][1] - points.last().unwrap()[1])
            <= tolerance
    {
        points.pop();
    }
    if points.len() < 3 {
        return None;
    }
    #[derive(Clone, Copy)]
    struct Hit {
        at: f64,
        point: [f64; 2],
    }
    let hits_on = |polygon: &[[f64; 2]]| -> Vec<Hit> {
        let mut hits = Vec::new();
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            let dir = [b[0] - a[0], b[1] - a[1]];
            let f = [a[0] - cx, a[1] - cy];
            let aa = dir[0] * dir[0] + dir[1] * dir[1];
            if aa <= tolerance * tolerance {
                continue;
            }
            let bb = 2.0 * (dir[0] * f[0] + dir[1] * f[1]);
            let cc = f[0] * f[0] + f[1] * f[1] - r2;
            let discriminant = bb * bb - 4.0 * aa * cc;
            if discriminant <= tolerance * tolerance {
                continue;
            }
            for t in [
                (-bb - discriminant.sqrt()) / (2.0 * aa),
                (-bb + discriminant.sqrt()) / (2.0 * aa),
            ] {
                if t < -tolerance || t > 1.0 + tolerance {
                    continue;
                }
                let t = t.clamp(0.0, 1.0);
                let point = [a[0] + t * dir[0], a[1] + t * dir[1]];
                if !hits.iter().any(|prev: &Hit| {
                    (point[0] - prev.point[0]).hypot(point[1] - prev.point[1]) <= tolerance
                }) {
                    hits.push(Hit {
                        at: i as f64 + t,
                        point,
                    });
                }
            }
        }
        hits.sort_by(|a, b| a.at.total_cmp(&b.at));
        hits
    };
    let hits = hits_on(&points);
    // Don't attempt to combine a cutter with existing holes. The hole must
    // remain completely separate from this disk.
    for (index, hole) in rings.iter().enumerate() {
        if index == outer_index {
            continue;
        }
        if !hits_on(hole).is_empty()
            || hole.iter().any(|p| d2(*p) <= (r + tolerance).powi(2))
            || polygon_contains(hole, centre)
        {
            return None;
        }
    }
    let mut result = hatch.clone();
    result.common.handle = Handle::NULL;
    result.is_associative = false;
    match hits.len() {
        0 => {
            // A circle completely inside the outer region becomes an exact
            // circular island/hole. The circle itself is left untouched.
            if !polygon_contains(&points, centre) {
                return None;
            }
            for offset in [[r, 0.0], [-r, 0.0], [0.0, r], [0.0, -r]] {
                if !polygon_contains(&points, [cx + offset[0], cy + offset[1]]) {
                    return None;
                }
            }
            let mut hole = BoundaryPath::new();
            hole.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
                center: Vector2::new(cx, cy),
                radius: r,
                start_angle: 0.0,
                end_angle: TAU,
                counter_clockwise: true,
            }));
            result.paths.push(hole);
        }
        2 => {
            let n = points.len() as f64;
            let forward_chain = |a: Hit, b: Hit| -> Vec<[f64; 2]> {
                let finish = if b.at <= a.at { b.at + n } else { b.at };
                let mut chain = vec![a.point];
                let mut vertex = a.at.floor() + 1.0;
                while vertex < finish - tolerance && chain.len() <= points.len() + 2 {
                    chain.push(points[vertex as usize % points.len()]);
                    vertex += 1.0;
                }
                chain.push(b.point);
                chain
            };
            let a = forward_chain(hits[0], hits[1]);
            let b = forward_chain(hits[1], hits[0]);
            let outside = |chain: &[[f64; 2]]| {
                chain.windows(2).all(|pair| {
                    let p = [
                        (pair[0][0] + pair[1][0]) * 0.5,
                        (pair[0][1] + pair[1][1]) * 0.5,
                    ];
                    d2(p) >= (r - tolerance).max(0.0).powi(2)
                })
            };
            let chain = match (outside(&a), outside(&b)) {
                (true, false) => a,
                (false, true) => b,
                _ => return None,
            };
            if chain.len() < 2 || chain.len() > points.len() + 2 {
                return None;
            }
            // Close the outer path with the arc on the INSIDE of the old
            // polygon, NOT the opposite circle arc outside the filled area.
            let arc_start = *chain.last().unwrap();
            let arc_end = chain[0];
            let t0 = (arc_start[1] - cy).atan2(arc_start[0] - cx);
            let t1 = (arc_end[1] - cy).atan2(arc_end[0] - cx);
            let ccw_sweep = (t1 - t0).rem_euclid(TAU);
            let cw_sweep = (t0 - t1).rem_euclid(TAU);
            let midpoint = |angle: f64| [cx + r * angle.cos(), cy + r * angle.sin()];
            let ccw_inside = polygon_contains(&points, midpoint(t0 + ccw_sweep * 0.5));
            let cw_inside = polygon_contains(&points, midpoint(t0 - cw_sweep * 0.5));
            let counter_clockwise = match (ccw_inside, cw_inside) {
                (true, false) => true,
                (false, true) => false,
                _ => return None,
            };
            let sweep = if counter_clockwise {
                ccw_sweep
            } else {
                cw_sweep
            };
            if sweep <= tolerance || sweep >= TAU - tolerance {
                return None;
            }
            let stored_start = if counter_clockwise { t0 } else { -t0 }.rem_euclid(TAU);
            let mut path = BoundaryPath::with_flags(BoundaryPathFlags::OUTERMOST);
            for pair in chain.windows(2) {
                if (pair[0][0] - pair[1][0]).hypot(pair[0][1] - pair[1][1]) > tolerance {
                    path.add_edge(BoundaryEdge::Line(LineEdge {
                        start: Vector2::new(pair[0][0], pair[0][1]),
                        end: Vector2::new(pair[1][0], pair[1][1]),
                    }));
                }
            }
            path.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
                center: Vector2::new(cx, cy),
                radius: r,
                start_angle: stored_start,
                end_angle: stored_start + sweep,
                counter_clockwise,
            }));
            result.paths[outer_index] = path;
        }
        // More intersections can split the hatch into several disconnected
        // pieces or merge holes. Until a full region boolean exists, refuse.
        _ => return None,
    }
    for path in &mut result.paths {
        path.boundary_handles.clear();
        path.flags.set_external(false);
    }
    Some(result)
}

/// Trim a boundary with mixed exact LINE and CIRCULAR ARC edges against
/// one straight cutting segment. Work in parameter space: retain the chain
/// on the opposite side of the click and close it along the cutter. A new
/// CircularArcEdge is emitted for the surviving portion of any split arc.

pub(crate) fn trim_hatch_curved_outer(
    path: &document::entities::BoundaryPath,
    p1: [f64; 2],
    p2: [f64; 2],
    keep_sign: f64,
) -> Option<document::entities::BoundaryPath> {
    use document::entities::{
        BoundaryEdge, BoundaryPath, BoundaryPathFlags, CircularArcEdge, LineEdge,
    };
    use document::types::Vector2;
    const EPS: f64 = 1.0e-8;
    const TAU: f64 = std::f64::consts::TAU;
    #[derive(Clone, Copy)]
    enum Segment {
        Line {
            a: [f64; 2],
            b: [f64; 2],
        },
        Arc {
            center: [f64; 2],
            radius: f64,
            start: f64,
            sweep: f64,
            ccw: bool,
        },
    }
    impl Segment {
        fn point(self, t: f64) -> [f64; 2] {
            match self {
                Segment::Line { a, b } => [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])],
                Segment::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                    ccw,
                } => {
                    let angle = start + if ccw { sweep * t } else { -sweep * t };
                    [
                        center[0] + radius * angle.cos(),
                        center[1] + radius * angle.sin(),
                    ]
                }
            }
        }
        fn portion(self, t0: f64, t1: f64) -> Option<BoundaryEdge> {
            if t1 - t0 <= 1.0e-10 {
                return None;
            }
            match self {
                Segment::Line { .. } => {
                    let a = self.point(t0);
                    let b = self.point(t1);
                    ((a[0] - b[0]).hypot(a[1] - b[1]) > 1.0e-10).then_some(BoundaryEdge::Line(
                        LineEdge {
                            start: Vector2::new(a[0], a[1]),
                            end: Vector2::new(b[0], b[1]),
                        },
                    ))
                }
                Segment::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                    ccw,
                } => {
                    let actual_start = start + if ccw { sweep * t0 } else { -sweep * t0 };
                    let stored = if ccw { actual_start } else { -actual_start }.rem_euclid(TAU);
                    Some(BoundaryEdge::CircularArc(CircularArcEdge {
                        center: Vector2::new(center[0], center[1]),
                        radius,
                        start_angle: stored,
                        end_angle: stored + sweep * (t1 - t0),
                        counter_clockwise: ccw,
                    }))
                }
            }
        }
    }
    let mut edges = Vec::new();
    for edge in &path.edges {
        match edge {
            BoundaryEdge::Line(line) => edges.push(Segment::Line {
                a: [line.start.x, line.start.y],
                b: [line.end.x, line.end.y],
            }),
            BoundaryEdge::Polyline(poly)
                if poly.is_closed
                    && poly.vertices.len() >= 3
                    && poly.vertices.iter().all(|v| v.z.abs() <= 1.0e-12) =>
            {
                for j in 0..poly.vertices.len() {
                    let a = &poly.vertices[j];
                    let b = &poly.vertices[(j + 1) % poly.vertices.len()];
                    edges.push(Segment::Line {
                        a: [a.x, a.y],
                        b: [b.x, b.y],
                    });
                }
            }
            BoundaryEdge::CircularArc(arc) => {
                let sweep = arc.end_angle - arc.start_angle;
                if !sweep.is_finite() || sweep <= 0.0 || sweep > TAU + EPS {
                    return None;
                }
                edges.push(Segment::Arc {
                    center: [arc.center.x, arc.center.y],
                    radius: arc.radius,
                    start: if arc.counter_clockwise {
                        arc.start_angle
                    } else {
                        -arc.start_angle
                    },
                    sweep,
                    ccw: arc.counter_clockwise,
                });
            }
            _ => return None,
        }
    }
    if edges.len() < 2 || !edges.iter().any(|edge| matches!(edge, Segment::Arc { .. })) {
        return None;
    }
    let n = edges.len();
    for i in 0..n {
        let a = edges[i].point(1.0);
        let b = edges[(i + 1) % n].point(0.0);
        if (a[0] - b[0]).hypot(a[1] - b[1]) > EPS {
            return None; // malformed or unordered DXF path
        }
    }
    let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
    let length = dx.hypot(dy);
    if length <= EPS {
        return None;
    }
    let (ux, uy) = (dx / length, dy / length);
    let side = |p: [f64; 2]| ux * (p[1] - p1[1]) - uy * (p[0] - p1[0]);
    #[derive(Clone, Copy)]
    struct Hit {
        at: f64,
        point: [f64; 2],
    }
    let mut hits: Vec<Hit> = Vec::new();
    let mut record = |edge_index: usize, t: f64, edge: Segment| {
        let p = edge.point(t.clamp(0.0, 1.0));
        let proj = (p[0] - p1[0]) * ux + (p[1] - p1[1]) * uy;
        if proj < -EPS || proj > length + EPS {
            return;
        }
        if hits
            .iter()
            .any(|h| (h.point[0] - p[0]).hypot(h.point[1] - p[1]) <= EPS)
        {
            return;
        }
        let position = (edge_index as f64 + t).rem_euclid(n as f64);
        hits.push(Hit {
            at: position,
            point: p,
        });
    };
    for (i, edge) in edges.iter().copied().enumerate() {
        match edge {
            Segment::Line { a, b } => {
                let da = side(a);
                let db = side(b);
                if da.abs() <= EPS && db.abs() <= EPS {
                    return None;
                }
                if da.abs() <= EPS {
                    record(i, 0.0, edge);
                }
                if db.abs() <= EPS {
                    record(i, 1.0, edge);
                }
                if da * db < -EPS * EPS {
                    record(i, da / (da - db), edge);
                }
            }
            Segment::Arc {
                center,
                radius,
                start,
                sweep,
                ccw,
            } => {
                let signed = side(center);
                if signed.abs() > radius + EPS {
                    continue;
                }
                let along = (radius * radius - signed * signed).max(0.0).sqrt();
                for proj in [-along, along] {
                    let p = [
                        center[0] - signed * (-uy) + proj * ux,
                        center[1] - signed * ux + proj * uy,
                    ];
                    let angle = (p[1] - center[1]).atan2(p[0] - center[0]);
                    let angle_delta = if ccw {
                        (angle - start).rem_euclid(TAU)
                    } else {
                        (start - angle).rem_euclid(TAU)
                    };
                    if angle_delta <= sweep + EPS {
                        let t = (angle_delta / sweep).clamp(0.0, 1.0);
                        record(i, t, edge);
                    }
                }
            }
        }
    }
    if hits.len() != 2 {
        return None;
    }
    hits.sort_by(|a, b| a.at.total_cmp(&b.at));
    let create = |from: Hit, to: Hit| -> Option<BoundaryPath> {
        let end = if to.at <= from.at {
            to.at + n as f64
        } else {
            to.at
        };
        let mut position = from.at;
        let mut parts: Vec<BoundaryEdge> = Vec::new();
        // Every surviving edge midpoint must be strictly inside the retained
        // halfplane; this rejects disjoint/concave portions without rewriting
        // the hatch into a self-intersecting polygon.
        while position < end - 1.0e-10 {
            let segment_index = position.floor() as usize;
            let next = ((segment_index + 1) as f64).min(end);
            let edge = edges[segment_index % n];
            let begin_t = position - segment_index as f64;
            let end_t = next - segment_index as f64;
            if keep_sign * side(edge.point((begin_t + end_t) * 0.5)) < -EPS {
                return None;
            }
            if let Some(edge) = edge.portion(begin_t, end_t) {
                parts.push(edge);
            }
            position = next;
            if parts.len() > n + 3 {
                return None;
            }
        }
        if parts.is_empty() {
            return None;
        }
        parts.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(to.point[0], to.point[1]),
            end: Vector2::new(from.point[0], from.point[1]),
        }));
        let mut result = BoundaryPath::with_flags(BoundaryPathFlags::OUTERMOST);
        result.edges = parts;
        Some(result)
    };
    let first = create(hits[0], hits[1]);
    let second = create(hits[1], hits[0]);
    match (first, second) {
        (Some(path), None) | (None, Some(path)) => Some(path),
        _ => None,
    }
}

/// TRIM a planar hatch as a REGION, not as an individual hatch boundary edge.
///
/// The picked point chooses the half-plane to REMOVE. Only a finite straight
/// cutting edge intersecting the outer ring exactly twice is supported here.
/// Disconnected cuts, curved original edges, and cutters through holes are
/// deliberately refused rather than damaging the hatch topology. An edited
/// hatch is detached from its original associative source handles: these
/// source curves have not changed to match the new boundary.

pub(crate) fn trim_hatch_at(
    hatch: &document::entities::Hatch,
    geos: &[Geo],
    px: f64,
    py: f64,
) -> Option<document::entities::Hatch> {
    use document::entities::{BoundaryEdge, BoundaryPath, BoundaryPathFlags, PolylineEdge};
    use document::types::Vector2;
    // The working plane of TRIM is XY. Do not quietly clip a tilted OCS
    // boundary using an unrelated XY cutting edge.
    if hatch.normal.x.abs() > 1.0e-9
        || hatch.normal.y.abs() > 1.0e-9
        || (hatch.normal.z - 1.0).abs() > 1.0e-9
        || hatch.paths.is_empty()
    {
        return None;
    }
    // Inner paths (including exact circles) do not need to be clipped when
    // entirely on one side of the cutter. Preserve those analytic curves as
    // they are rather than refusing to trim the hatch again.
    let rings: Vec<Vec<[f64; 2]>> = hatch
        .paths
        .iter()
        .map(crate::scene::hatch_path_ring)
        .collect::<Option<Vec<_>>>()?;
    let depths = ocs_geom::geom2d::ring_nesting_depths(&rings);
    let outer: Vec<usize> = depths
        .iter()
        .enumerate()
        .filter_map(|(index, depth)| (*depth == 0).then_some(index))
        .collect();
    // A single, connected outer ring can safely be clipped by one line.
    if outer.len() != 1 {
        return None;
    }
    let outer_index = outer[0];
    // Only edit outer curves for which we can reconstruct exact HATCH edges.
    // Never silently convert an imported SPLINE/ELLIPSE into a sampled
    // polyline when TRIM is run a second time.
    if !hatch.paths[outer_index]
        .edges
        .iter()
        .all(|edge| match edge {
            BoundaryEdge::Line(_) | BoundaryEdge::CircularArc(_) => true,
            BoundaryEdge::Polyline(poly) => {
                poly.is_closed
                    && poly.vertices.len() >= 3
                    && poly.vertices.iter().all(|vertex| vertex.z.abs() <= 1.0e-12)
            }
            _ => false,
        })
    {
        return None;
    }
    // Preserve exact outer curves; the mixed-edge solver below handles
    // successive straight cuts even after a curved notch has been made.
    let ring = &rings[outer_index];
    const EPS: f64 = 1.0e-8;
    let ring_area = |points: &[[f64; 2]]| -> f64 {
        points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
            .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
            .sum::<f64>()
            .abs()
            * 0.5
    };
    let old_area = ring_area(ring);
    if !old_area.is_finite() || old_area <= EPS {
        return None;
    }
    let mut best: Option<(f64, document::entities::Hatch)> = None;
    // A click inside a circle should cut that disk out of the hatch. Do
    // not let an unrelated nearby straight edge win this pick.
    for circle in geos.iter().filter(|geo| matches!(geo, Geo::Circle { .. })) {
        if let Some(clipped) = trim_hatch_circle(hatch, &rings, outer_index, circle, [px, py]) {
            if let Geo::Circle { cx, cy, r, .. } = *circle {
                let distance = (r - (px - cx).hypot(py - cy)).abs();
                if best.as_ref().is_none_or(|(d, _)| distance < *d) {
                    best = Some((distance, clipped));
                }
            }
        }
    }
    if best.is_some() {
        return best.map(|(_, result)| result);
    }
    for geo in geos {
        let Geo::Line { handle, p1, p2 } = geo else {
            continue;
        };
        if *handle == hatch.common.handle {
            continue;
        }
        let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
        let len2 = dx * dx + dy * dy;
        if !len2.is_finite() || len2 < EPS * EPS {
            continue;
        }
        let len = len2.sqrt();
        let side = |p: [f64; 2]| (dx * (p[1] - p1[1]) - dy * (p[0] - p1[0])) / len;
        let clicked_side = side([px, py]);
        if clicked_side.abs() <= EPS {
            continue;
        }
        let keep_sign = -clicked_side.signum();
        let distances: Vec<_> = ring.iter().copied().map(side).collect();
        // The original ring must cross the cutter at exactly two distinct
        // points, both ON the finite cutting line (no implicit Edge: Extend).
        let mut intersections: Vec<[f64; 2]> = Vec::new();
        for j in 0..ring.len() {
            let next = (j + 1) % ring.len();
            let a = ring[j];
            let b = ring[next];
            let sa = distances[j];
            let sb = distances[next];
            let mut crossing = None;
            if sa.abs() <= EPS {
                crossing = Some(a);
            } else if sa * sb < -EPS * EPS {
                let t = sa / (sa - sb);
                crossing = Some([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
            }
            if let Some(p) = crossing {
                if !intersections
                    .iter()
                    .any(|prev| (prev[0] - p[0]).hypot(prev[1] - p[1]) <= EPS)
                {
                    intersections.push(p);
                }
            }
        }
        if intersections.len() != 2
            || intersections.iter().any(|p| {
                let t = ((p[0] - p1[0]) * dx + (p[1] - p1[1]) * dy) / len2;
                t < -EPS || t > 1.0 + EPS
            })
        {
            continue;
        }
        // Sutherland-Hodgman against the retained half-plane. Exactly two
        // outer intersections ensure this creates one connected closed ring.
        let inside = |p: [f64; 2]| keep_sign * side(p) >= -EPS;
        let mut clipped = Vec::new();
        for j in 0..ring.len() {
            let next = (j + 1) % ring.len();
            let (a, b) = (ring[j], ring[next]);
            let (a_inside, b_inside) = (inside(a), inside(b));
            if a_inside != b_inside {
                let sa = side(a);
                let sb = side(b);
                if (sa - sb).abs() > EPS {
                    let t = sa / (sa - sb);
                    clipped.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
                }
            }
            if b_inside {
                clipped.push(b);
            }
        }
        let mut cleaned: Vec<[f64; 2]> = Vec::new();
        for p in clipped {
            if cleaned.last().is_none_or(|prev| {
                let prev: &[f64; 2] = prev;
                (prev[0] - p[0]).hypot(prev[1] - p[1]) > EPS
            }) {
                cleaned.push(p);
            }
        }
        if cleaned.len() > 1
            && (cleaned[0][0] - cleaned[cleaned.len() - 1][0])
                .hypot(cleaned[0][1] - cleaned[cleaned.len() - 1][1])
                <= EPS
        {
            cleaned.pop();
        }
        let new_area = ring_area(&cleaned);
        if cleaned.len() < 3
            || !new_area.is_finite()
            || new_area <= EPS
            || new_area >= old_area - EPS
        {
            continue;
        }
        // On a previously circular-notched hatch, use exact original arc
        // segments rather than tessellating the curved boundary to polylines.
        let curved_outer = hatch.paths[outer_index]
            .edges
            .iter()
            .any(|edge| matches!(edge, BoundaryEdge::CircularArc(_)));
        let analytic_outer = if curved_outer {
            let Some(path) =
                trim_hatch_curved_outer(&hatch.paths[outer_index], *p1, *p2, keep_sign)
            else {
                continue;
            };
            Some(path)
        } else {
            None
        };
        let mut new_paths = Vec::with_capacity(hatch.paths.len());
        let mut safe = true;
        for (index, path) in hatch.paths.iter().enumerate() {
            if index == outer_index {
                if let Some(ref path) = analytic_outer {
                    new_paths.push(path.clone());
                } else {
                    let mut replacement = BoundaryPath::with_flags(BoundaryPathFlags::OUTERMOST);
                    replacement.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
                        cleaned.iter().map(|p| Vector2::new(p[0], p[1])).collect(),
                        true,
                    )));
                    new_paths.push(replacement);
                }
            } else {
                // A hole entirely retained keeps its exact path; a hole
                // entirely removed disappears. Straddling holes need a
                // topological boolean operation, so do NOT modify the hatch.
                let ring_sides: Vec<f64> = rings[index].iter().copied().map(side).collect();
                let keeps = ring_sides.iter().all(|s| keep_sign * *s > EPS);
                let removes = ring_sides.iter().all(|s| keep_sign * *s < -EPS);
                if !keeps && !removes {
                    safe = false;
                    break;
                }
                if keeps {
                    let mut path = path.clone();
                    path.boundary_handles.clear();
                    path.flags.set_external(false);
                    new_paths.push(path);
                }
            }
        }
        if !safe {
            continue;
        }
        let mut result = hatch.clone();
        result.common.handle = Handle::NULL;
        result.paths = new_paths;
        result.is_associative = false;
        // Clearing associations must apply to every path, not just the outer
        // one, including inner holes inherited from the original hatch.
        for path in &mut result.paths {
            path.boundary_handles.clear();
            path.flags.set_external(false);
        }
        let distance = clicked_side.abs();
        if best.as_ref().is_none_or(|(d, _)| distance < *d) {
            best = Some((distance, result));
        }
    }
    best.map(|(_, result)| result)
}
