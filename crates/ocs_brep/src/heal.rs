//! Repairs for a body assembled from an exchange file.
//!
//! A file names its faces' boundaries one face at a time, and a writer that
//! rounds its coordinates, or a format that does not share edges between
//! faces at all, leaves faces whose boundaries meet only within a tolerance.
//! [`sew`] joins them: vertices that lie within the tolerance of each other
//! become one, an edge that a vertex of a neighbour lands inside is split
//! there, and two edges that then run between the same vertices along the
//! same course become one edge used by both faces. Shells joined that way are
//! merged. [`sew_into_solid`] repeats the sewing at a growing tolerance while
//! free edges remain, for a body that is meant to be closed.
//!
//! [`fix_loops`] puts each face's loops in order: coedges chained into the
//! rings they form, the ring enclosing the others first, and a face whose
//! outer ring runs against its normal turned to agree with it.
//!
//! Ported from solidcore (Apache-2.0, a Rust port of NVIDIA's USD BRep SMLib):
//! the sewing follows `heal/stitch/p2.rs` (`Stitch::do_stitching`,
//! `stitch_into_solid`) as its `docs/validity-and-tolerances.md` §7.3
//! describes it, and the loop repairs `topology/trimming_tools.rs`
//! (`order_curves_into_loops`, `fix_trim_loop_order`,
//! `fix_loop_orientation`). Two of that file's repairs work on a face's
//! parameter-space trim curves, which a body here carries only optionally,
//! and are not ported: closing tiny gaps between them and turning an outer
//! loop that runs backwards along a seam.

use std::collections::HashMap;
use std::f64::consts::TAU;

use super::geometry::{Curve3, Surface};
use super::topology::{
    Body, Coedge, CoedgeKey, Edge, EdgeKey, FaceKey, Loop, ShellKey, VertexKey,
};
use super::Provenance;
use ocs_geom::space::Vec3;

/// What a sewing changed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sewing {
    /// Vertices merged into another.
    pub vertices_glued: usize,
    /// Edges split where a neighbour's vertex landed inside them.
    pub edges_split: usize,
    /// Edges merged into another.
    pub edges_glued: usize,
    /// Faces turned over so their shell faces one way.
    pub faces_turned: usize,
    /// Edges still used by one face only.
    pub free_edges: usize,
    /// The tolerance of the last pass.
    pub tolerance: f64,
}

impl Sewing {
    fn add(&mut self, pass: Sewing) {
        self.vertices_glued += pass.vertices_glued;
        self.edges_split += pass.edges_split;
        self.edges_glued += pass.edges_glued;
        self.faces_turned += pass.faces_turned;
        self.free_edges = pass.free_edges;
        self.tolerance = pass.tolerance;
    }

}

/// How many edges only one face uses.
pub fn free_edges(body: &Body) -> usize {
    body.edges
        .iter()
        .filter(|(_, edge)| edge.coedges.len() == 1)
        .count()
}

/// Sews `body` at `tolerance`: glues the vertices of its free edges that
/// lie within it of each other, splits free edges where such a vertex lands
/// inside them, glues free edges that then share both ends and run within
/// the tolerance of each other, turns faces so each joined shell faces one
/// way, and merges the shells that now share an edge.
pub fn sew(body: &mut Body, tolerance: f64) -> Sewing {
    let mut report = Sewing {
        tolerance,
        ..Sewing::default()
    };
    if tolerance.is_finite() && tolerance > 0.0 {
        report.vertices_glued = glue_vertices(body, tolerance);
        report.edges_split = split_edges(body, tolerance);
        report.edges_glued = glue_edges(body, tolerance);
        report.faces_turned = orient_shells(body);
        merge_shells(body);
    }
    report.free_edges = free_edges(body);
    report
}

/// Sews a body meant to be closed: from a tenth of `tolerance`, doubling it
/// while free edges remain, up to 1/200 of the body's box diagonal.
pub fn sew_into_solid(body: &mut Body, tolerance: f64) -> Sewing {
    let mut total = Sewing::default();
    let Some(diagonal) = diagonal(body) else {
        total.free_edges = free_edges(body);
        return total;
    };
    let limit = diagonal / 200.0;
    let mut step = (0.1 * tolerance).min(limit);
    if !(step.is_finite() && step > 0.0) {
        step = limit * 1e-6;
    }
    loop {
        let pass = sew(body, step);
        total.add(pass);
        if pass.free_edges == 0 || step >= limit {
            break;
        }
        step = (2.0 * step).min(limit);
    }
    total
}

/// The diagonal of the box round the body's vertices.
fn diagonal(body: &Body) -> Option<f64> {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for (_, vertex) in body.vertices.iter() {
        for axis in 0..3 {
            low[axis] = low[axis].min(vertex.point[axis]);
            high[axis] = high[axis].max(vertex.point[axis]);
        }
    }
    let length = (Vec3::from(high) - Vec3::from(low)).length();
    (length.is_finite() && length > 0.0).then_some(length)
}

/// The vertices at the ends of free edges.
fn boundary_vertices(body: &Body) -> Vec<VertexKey> {
    let mut out: Vec<VertexKey> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, edge) in body.edges.iter() {
        if edge.coedges.len() == 1 {
            for vertex in [edge.start, edge.end] {
                if seen.insert(vertex) {
                    out.push(vertex);
                }
            }
        }
    }
    out
}

/// Glues boundary vertices within `tolerance` of each other, keeping the
/// first of each pair: sorted along (1, 1, 1), each is compared only with
/// those whose coordinate sum is close enough for them to be that near. Two
/// ends of one edge are never glued, which would collapse it.
fn glue_vertices(body: &mut Body, tolerance: f64) -> usize {
    let mut sorted: Vec<(f64, VertexKey, [f64; 3])> = boundary_vertices(body)
        .into_iter()
        .filter_map(|key| {
            let point = body.vertices.get(key)?.point;
            Some((point[0] + point[1] + point[2], key, point))
        })
        .collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut joined: HashMap<VertexKey, Vec<VertexKey>> = HashMap::new();
    for (_, edge) in body.edges.iter() {
        joined.entry(edge.start).or_default().push(edge.end);
        joined.entry(edge.end).or_default().push(edge.start);
    }
    let reach = tolerance * 3f64.sqrt();
    let mut into: HashMap<VertexKey, VertexKey> = HashMap::new();
    for i in 0..sorted.len() {
        let (sum, keep, point) = sorted[i];
        if into.contains_key(&keep) {
            continue;
        }
        for &(other_sum, gone, other) in &sorted[i + 1..] {
            if other_sum - sum > reach {
                break;
            }
            if into.contains_key(&gone)
                || (Vec3::from(point) - Vec3::from(other)).length() > tolerance
                || joined.get(&keep).is_some_and(|ends| ends.contains(&gone))
            {
                continue;
            }
            into.insert(gone, keep);
        }
    }
    if into.is_empty() {
        return 0;
    }
    for edge in body.edges.values_mut() {
        for end in [&mut edge.start, &mut edge.end] {
            if let Some(&keep) = into.get(end) {
                *end = keep;
                edge.provenance.soil();
            }
        }
    }
    for gone in into.keys() {
        body.vertices.remove(*gone);
    }
    into.len()
}

/// How long a run of a curve's parameter repeats over, if it does.
fn period(curve: &Curve3) -> Option<f64> {
    match curve {
        Curve3::Circle(_) | Curve3::Ellipse(_) => Some(TAU),
        Curve3::Nurbs(spline) if spline.periodicity() => {
            let (from, to) = spline.domain();
            Some(to - from)
        }
        _ => None,
    }
}

/// The parameter of `point` on an edge, inside the edge's run when a
/// periodic curve can put it there.
fn edge_parameter(edge: &Edge, curve: &Curve3, point: [f64; 3]) -> f64 {
    let mut t = curve.parameter_at(point);
    if let Some(period) = period(curve) {
        if period > 0.0 {
            t = edge.start_parameter + (t - edge.start_parameter).rem_euclid(period);
        }
    }
    t
}

/// Splits each free edge where the vertex of another free edge lands inside
/// it, within `tolerance` of its curve. The edge's coedge becomes one per
/// piece, in the order its loop runs them.
fn split_edges(body: &mut Body, tolerance: f64) -> usize {
    let mut by_x: Vec<(f64, VertexKey, [f64; 3])> = boundary_vertices(body)
        .into_iter()
        .filter_map(|key| {
            let point = body.vertices.get(key)?.point;
            Some((point[0], key, point))
        })
        .collect();
    by_x.sort_by(|a, b| a.0.total_cmp(&b.0));
    let free: Vec<EdgeKey> = body
        .edges
        .iter()
        .filter(|(_, edge)| edge.coedges.len() == 1)
        .map(|(key, _)| key)
        .collect();
    let mut cuts: Vec<(EdgeKey, Vec<(f64, VertexKey)>)> = Vec::new();
    for key in free {
        let Some(edge) = body.edges.get(key) else {
            continue;
        };
        let Some(curve) = body.curves.get(edge.curve) else {
            continue;
        };
        let (t0, t1) = (edge.start_parameter, edge.end_parameter);
        let samples: Vec<[f64; 3]> = (0..=16)
            .map(|k| curve.point_at(t0 + (t1 - t0) * k as f64 / 16.0))
            .collect();
        // Room for the curve to bow between samples.
        let sag = samples
            .windows(2)
            .map(|pair| (Vec3::from(pair[1]) - Vec3::from(pair[0])).length())
            .fold(0.0, f64::max);
        let pad = tolerance + sag;
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for point in &samples {
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis] - pad);
                high[axis] = high[axis].max(point[axis] + pad);
            }
        }
        let first = by_x.partition_point(|entry| entry.0 < low[0]);
        let margin = 1e-9 * (t1 - t0).abs();
        // A vertex this near an end is that end, a little off: gluing joins
        // it, where a split would leave a sliver.
        let length: f64 = samples
            .windows(2)
            .map(|pair| (Vec3::from(pair[1]) - Vec3::from(pair[0])).length())
            .sum();
        let near_end = |point: [f64; 3]| {
            [samples[0], samples[16]]
                .iter()
                .any(|end| (Vec3::from(*end) - Vec3::from(point)).length() <= 1e-3 * length)
        };
        let mut here = Vec::new();
        for &(x, vertex, point) in &by_x[first..] {
            if x > high[0] {
                break;
            }
            if vertex == edge.start
                || vertex == edge.end
                || (0..3).any(|axis| point[axis] < low[axis] || point[axis] > high[axis])
            {
                continue;
            }
            let t = edge_parameter(edge, curve, point);
            if t <= t0 + margin || t >= t1 - margin || near_end(point) {
                continue;
            }
            if (Vec3::from(curve.point_at(t)) - Vec3::from(point)).length() <= tolerance {
                here.push((t, vertex));
            }
        }
        if !here.is_empty() {
            here.sort_by(|a, b| a.0.total_cmp(&b.0));
            here.dedup_by(|a, b| a.1 == b.1);
            cuts.push((key, here));
        }
    }
    let mut count = 0;
    for (key, here) in cuts {
        if split_edge(body, key, &here).is_some() {
            count += here.len();
        }
    }
    count
}

/// Splits a free edge at increasing parameters, each at its vertex.
fn split_edge(body: &mut Body, key: EdgeKey, at: &[(f64, VertexKey)]) -> Option<()> {
    let edge = body.edges.get(key)?.clone();
    let [coedge_key] = edge.coedges.as_slice() else {
        return None;
    };
    let coedge = body.coedges.get(*coedge_key)?.clone();
    // The pieces in the curve's direction: the original edge keeps the
    // first.
    let mut pieces = vec![(key, *coedge_key)];
    let mut start = (edge.start_parameter, edge.start);
    let mut bounds: Vec<((f64, VertexKey), (f64, VertexKey))> = Vec::new();
    for &cut in at {
        bounds.push((start, cut));
        start = cut;
    }
    bounds.push((start, (edge.end_parameter, edge.end)));
    for (index, ((t0, v0), (t1, v1))) in bounds.into_iter().enumerate() {
        if index == 0 {
            let first = body.edges.get_mut(key)?;
            first.end_parameter = t1;
            first.end = v1;
            first.provenance.soil();
            continue;
        }
        let piece = body.edges.insert(Edge {
            curve: edge.curve,
            start_parameter: t0,
            end_parameter: t1,
            start: v0,
            end: v1,
            coedges: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        let use_ = body.coedges.insert(Coedge {
            edge: piece,
            forward: coedge.forward,
            pcurve: None,
            owner: coedge.owner,
            provenance: Provenance::Synthesized,
        });
        body.edges.get_mut(piece)?.coedges.push(use_);
        pieces.push((piece, use_));
    }
    // The original coedge's pcurve ran the whole edge.
    body.coedges.get_mut(*coedge_key)?.pcurve = None;
    let ring = body.loops.get_mut(coedge.owner)?;
    let place = ring.coedges.iter().position(|c| c == coedge_key)?;
    let mut uses: Vec<CoedgeKey> = pieces.iter().map(|&(_, use_)| use_).collect();
    if !coedge.forward {
        uses.reverse();
    }
    ring.coedges.splice(place..=place, uses);
    ring.provenance.soil();
    Some(())
}

/// Whether two edges with the same ends run along the same course: points
/// of the second at a quarter, half and three quarters of its run lie
/// within `tolerance` of the first.
fn coincide(body: &Body, a: &Edge, b: &Edge, tolerance: f64) -> Option<bool> {
    let (curve_a, curve_b) = (body.curves.get(a.curve)?, body.curves.get(b.curve)?);
    Some([0.25, 0.5, 0.75].iter().all(|f| {
        let point = curve_b.point_at(b.start_parameter + (b.end_parameter - b.start_parameter) * f);
        let t = edge_parameter(a, curve_a, point);
        t >= a.start_parameter - 1e-9
            && t <= a.end_parameter + 1e-9
            && (Vec3::from(curve_a.point_at(t)) - Vec3::from(point)).length() <= tolerance
    }))
}

/// Whether `b` runs the way `a` does.
fn same_way(body: &Body, a: &Edge, b: &Edge) -> Option<bool> {
    if a.start != a.end {
        return Some(a.start == b.start);
    }
    // A closed edge: compare where each heads from the shared vertex.
    let (curve_a, curve_b) = (body.curves.get(a.curve)?, body.curves.get(b.curve)?);
    let heading = |curve: &Curve3, edge: &Edge| {
        let t = edge.start_parameter + 0.01 * (edge.end_parameter - edge.start_parameter);
        Vec3::from(curve.point_at(t)) - Vec3::from(curve.point_at(edge.start_parameter))
    };
    Some(heading(curve_a, a).dot(heading(curve_b, b)) > 0.0)
}

/// Glues free edges that share both ends and run within `tolerance` of each
/// other: the second's coedge moves onto the first, its sense adjusted, and
/// the second edge goes.
fn glue_edges(body: &mut Body, tolerance: f64) -> usize {
    let mut by_ends: HashMap<(VertexKey, VertexKey), Vec<EdgeKey>> = HashMap::new();
    for (key, edge) in body.edges.iter() {
        if edge.coedges.len() == 1 {
            let ends = if edge.start.slot() <= edge.end.slot() {
                (edge.start, edge.end)
            } else {
                (edge.end, edge.start)
            };
            by_ends.entry(ends).or_default().push(key);
        }
    }
    let mut groups: Vec<Vec<EdgeKey>> = by_ends.into_values().filter(|g| g.len() > 1).collect();
    groups.sort_by_key(|group| group[0].slot());
    let mut glued = 0;
    for group in groups {
        let mut open = group;
        while let Some(keep) = open.pop() {
            let Some(kept) = body.edges.get(keep).cloned() else {
                continue;
            };
            let partner = open.iter().position(|&other| {
                body.edges
                    .get(other)
                    .and_then(|edge| coincide(body, &kept, edge, tolerance))
                    .unwrap_or(false)
            });
            let Some(position) = partner else {
                continue;
            };
            let gone = open.swap_remove(position);
            if glue_edge(body, keep, gone).is_some() {
                glued += 1;
            }
        }
    }
    glued
}

fn glue_edge(body: &mut Body, keep: EdgeKey, gone: EdgeKey) -> Option<()> {
    let kept = body.edges.get(keep)?.clone();
    let other = body.edges.get(gone)?.clone();
    let along = same_way(body, &kept, &other)?;
    for &use_ in &other.coedges {
        let coedge = body.coedges.get_mut(use_)?;
        coedge.edge = keep;
        coedge.forward = coedge.forward == along;
        coedge.pcurve = None;
        coedge.provenance.soil();
    }
    let edge = body.edges.get_mut(keep)?;
    edge.coedges.extend(other.coedges);
    edge.provenance.soil();
    body.edges.remove(gone);
    Some(())
}

/// The coedges of a face.
fn face_coedges(body: &Body, face: FaceKey) -> Vec<CoedgeKey> {
    body.faces
        .get(face)
        .map(|face| {
            face.loops
                .iter()
                .filter_map(|&ring| body.loops.get(ring))
                .flat_map(|ring| ring.coedges.iter().copied())
                .collect()
        })
        .unwrap_or_default()
}

/// The face a coedge bounds.
fn coedge_face(body: &Body, coedge: CoedgeKey) -> Option<FaceKey> {
    Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner)
}

/// Turns a face over: its loops run the other way and its normal flips.
fn turn_face(body: &mut Body, face: FaceKey) -> Option<()> {
    let loops = body.faces.get(face)?.loops.clone();
    for ring in loops {
        let coedges = {
            let ring = body.loops.get_mut(ring)?;
            ring.coedges.reverse();
            ring.provenance.soil();
            ring.coedges.clone()
        };
        for coedge in coedges {
            let coedge = body.coedges.get_mut(coedge)?;
            coedge.forward = !coedge.forward;
            coedge.pcurve = None;
        }
    }
    let face = body.faces.get_mut(face)?;
    face.forward = !face.forward;
    face.provenance.soil();
    Some(())
}

/// Turns faces so that across every edge two faces share, their coedges run
/// it opposite ways: each connected set of faces takes the side of its first
/// face. Only sets with a disagreement are touched, so a body sewn from
/// consistent faces keeps every face as it was.
fn orient_shells(body: &mut Body) -> usize {
    let disagreements = body.edges.iter().any(|(_, edge)| {
        let [a, b] = edge.coedges.as_slice() else {
            return false;
        };
        let (Some(a), Some(b)) = (body.coedges.get(*a), body.coedges.get(*b)) else {
            return false;
        };
        a.forward == b.forward && a.owner != b.owner
    });
    if !disagreements {
        return 0;
    }
    let faces: Vec<FaceKey> = body.faces.keys().collect();
    let mut side: HashMap<FaceKey, bool> = HashMap::new();
    let mut turned = 0;
    for seed in faces {
        if side.contains_key(&seed) {
            continue;
        }
        side.insert(seed, false);
        let mut stack = vec![seed];
        while let Some(face) = stack.pop() {
            let flipped = side[&face];
            for coedge_key in face_coedges(body, face) {
                let Some(coedge) = body.coedges.get(coedge_key) else {
                    continue;
                };
                let Some(edge) = body.edges.get(coedge.edge) else {
                    continue;
                };
                let [a, b] = edge.coedges.as_slice() else {
                    continue;
                };
                let other = if *a == coedge_key { *b } else { *a };
                let (Some(other_coedge), Some(neighbour)) =
                    (body.coedges.get(other), coedge_face(body, other))
                else {
                    continue;
                };
                if neighbour == face || side.contains_key(&neighbour) {
                    continue;
                }
                // Opposite senses agree; the same sense means the neighbour
                // has to turn relative to this face.
                let agree = other_coedge.forward != coedge.forward;
                side.insert(neighbour, flipped != !agree);
                stack.push(neighbour);
            }
        }
    }
    for (face, flip) in side {
        if flip && turn_face(body, face).is_some() {
            turned += 1;
        }
    }
    turned
}

/// Merges shells that share an edge into the first of them, and drops the
/// lumps left empty.
fn merge_shells(body: &mut Body) {
    let shells: Vec<ShellKey> = body.shells.keys().collect();
    let index: HashMap<ShellKey, usize> = shells.iter().enumerate().map(|(i, &s)| (s, i)).collect();
    let mut parent: Vec<usize> = (0..shells.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let shell_of = |body: &Body, coedge: CoedgeKey| {
        let face = coedge_face(body, coedge)?;
        Some(body.faces.get(face)?.owner)
    };
    let mut joined = false;
    for (_, edge) in body.edges.iter() {
        let owners: Vec<usize> = edge
            .coedges
            .iter()
            .filter_map(|&coedge| shell_of(body, coedge))
            .filter_map(|shell| index.get(&shell).copied())
            .collect();
        for pair in owners.windows(2) {
            let (a, b) = (root(&mut parent, pair[0]), root(&mut parent, pair[1]));
            if a != b {
                parent[a.max(b)] = a.min(b);
                joined = true;
            }
        }
    }
    if !joined {
        return;
    }
    for (i, &shell) in shells.iter().enumerate() {
        let r = root(&mut parent, i);
        if r == i {
            continue;
        }
        let target = shells[r];
        let Some(moved) = body.shells.remove(shell) else {
            continue;
        };
        for &face in &moved.faces {
            if let Some(face) = body.faces.get_mut(face) {
                face.owner = target;
            }
        }
        if let Some(target) = body.shells.get_mut(target) {
            target.faces.extend(moved.faces);
        }
        if let Some(lump) = body.lumps.get_mut(moved.owner) {
            lump.shells.retain(|&s| s != shell);
        }
    }
    let empty: Vec<_> = body
        .lumps
        .iter()
        .filter(|(_, lump)| lump.shells.is_empty())
        .map(|(key, _)| key)
        .collect();
    for lump in empty {
        body.lumps.remove(lump);
        body.roots.retain(|&root| root != lump);
    }
}

/// What [`fix_loops`] changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoopRepairs {
    /// Faces whose coedges were chained into new rings.
    pub rechained: usize,
    /// Faces whose outer ring was moved first.
    pub reordered: usize,
    /// Faces turned over because their outer ring ran against them, and
    /// free inner rings turned round.
    pub reoriented: usize,
}

/// Puts every face's loops in order: coedges chained into the rings they
/// form, the ring with the widest reach in the surface's parameters first,
/// a face whose outer ring runs against its normal turned over, and an inner
/// ring running with it, whose edges no other face shares, turned round.
pub fn fix_loops(body: &mut Body) -> LoopRepairs {
    let mut repairs = LoopRepairs::default();
    let faces: Vec<FaceKey> = body.faces.keys().collect();
    for face in faces {
        if rechain(body, face).unwrap_or(false) {
            repairs.rechained += 1;
        }
        let Some(rings) = ring_shapes(body, face) else {
            continue;
        };
        // The first of the widest, so rings of one reach keep their order.
        let mut outer = 0;
        for (index, ring) in rings.iter().enumerate() {
            if ring.reach > rings[outer].reach * (1.0 + 1e-9) {
                outer = index;
            }
        }
        if rings.is_empty() {
            continue;
        }
        if outer != 0 {
            if let Some(face) = body.faces.get_mut(face) {
                face.loops.swap(0, outer);
                repairs.reordered += 1;
            }
        }
        let forward = body.faces.get(face).is_some_and(|face| face.forward);
        // Seen from the face's side, an outer ring turns anticlockwise.
        let outward = |area: f64| if forward { area } else { -area };
        let outer_area = rings[outer].area;
        if outer_area.is_some_and(|area| outward(area) < 0.0) {
            if let Some(face) = body.faces.get_mut(face) {
                face.forward = !face.forward;
                face.provenance.soil();
                repairs.reoriented += 1;
            }
        }
        let forward = body.faces.get(face).is_some_and(|face| face.forward);
        for (index, ring) in rings.iter().enumerate() {
            if index == outer {
                continue;
            }
            let wrong = ring
                .area
                .is_some_and(|area| (if forward { area } else { -area }) > 0.0);
            if wrong && ring_is_free(body, ring.key) && reverse_ring(body, ring.key).is_some() {
                repairs.reoriented += 1;
            }
        }
    }
    repairs
}

/// The vertices a coedge runs from and to.
fn coedge_ends(body: &Body, coedge: CoedgeKey) -> Option<(VertexKey, VertexKey)> {
    let coedge = body.coedges.get(coedge)?;
    let edge = body.edges.get(coedge.edge)?;
    Some(if coedge.forward {
        (edge.start, edge.end)
    } else {
        (edge.end, edge.start)
    })
}

/// Chains a face's coedges into rings when its loops do not close: from each
/// unused coedge, on to one that starts where it ends, until back at the
/// first. Leaves the face alone when every loop already closes, or when the
/// coedges do not chain into closed rings.
fn rechain(body: &mut Body, face: FaceKey) -> Option<bool> {
    let loops = body.faces.get(face)?.loops.clone();
    let closes = |body: &Body, ring: &Loop| {
        let count = ring.coedges.len();
        (0..count).all(|i| {
            match (
                coedge_ends(body, ring.coedges[i]),
                coedge_ends(body, ring.coedges[(i + 1) % count]),
            ) {
                (Some((_, end)), Some((start, _))) => end == start,
                _ => false,
            }
        })
    };
    if loops
        .iter()
        .all(|&ring| body.loops.get(ring).is_none_or(|ring| closes(body, ring)))
    {
        return Some(false);
    }
    let coedges = face_coedges(body, face);
    let mut from: HashMap<VertexKey, Vec<CoedgeKey>> = HashMap::new();
    for &coedge in &coedges {
        from.entry(coedge_ends(body, coedge)?.0).or_default().push(coedge);
    }
    let mut used = std::collections::HashSet::new();
    let mut rings: Vec<Vec<CoedgeKey>> = Vec::new();
    for &first in &coedges {
        if used.contains(&first) {
            continue;
        }
        used.insert(first);
        let (start, mut at) = coedge_ends(body, first)?;
        let mut ring = vec![first];
        while at != start {
            let next = from
                .get(&at)?
                .iter()
                .copied()
                .find(|coedge| !used.contains(coedge))?;
            used.insert(next);
            ring.push(next);
            at = coedge_ends(body, next)?.1;
        }
        rings.push(ring);
    }
    // The rings reuse the face's loops, in order, and add any more needed.
    let mut keys = loops.clone();
    while keys.len() < rings.len() {
        keys.push(body.loops.insert(Loop {
            coedges: Vec::new(),
            owner: face,
            provenance: Provenance::Synthesized,
        }));
    }
    for gone in keys.split_off(rings.len()) {
        body.loops.remove(gone);
    }
    for (&key, ring) in keys.iter().zip(&rings) {
        for &coedge in ring {
            body.coedges.get_mut(coedge)?.owner = key;
        }
        let entry = body.loops.get_mut(key)?;
        entry.coedges = ring.clone();
        entry.provenance.soil();
    }
    let face = body.faces.get_mut(face)?;
    face.loops = keys;
    face.provenance.soil();
    Some(true)
}

/// A ring as the surface's parameters see it.
struct RingShape {
    key: super::topology::LoopKey,
    /// The diagonal of its parameter box.
    reach: f64,
    /// Its signed area in the parameters (anticlockwise positive), or
    /// `None` when it wraps round a periodic direction and encloses nothing.
    area: Option<f64>,
}

/// How long the surface's parameters repeat over, each way.
fn surface_periods(surface: &Surface) -> [Option<f64>; 2] {
    match surface {
        Surface::Cylinder(_) | Surface::Cone(_) | Surface::Sphere(_) => [Some(TAU), None],
        Surface::Torus(_) => [Some(TAU), Some(TAU)],
        Surface::Plane(_) => [None, None],
        Surface::Nurbs(_) | Surface::Offset(_) => {
            let Some(spline) = surface.spline() else {
                return [None, None];
            };
            let [u, v] = spline.periodicity();
            let ((u0, u1), (v0, v1)) = spline.domain();
            [u.then_some(u1 - u0), v.then_some(v1 - v0)]
        }
    }
}

fn ring_shapes(body: &Body, face: FaceKey) -> Option<Vec<RingShape>> {
    let face_ = body.faces.get(face)?;
    let surface = body.surfaces.get(face_.surface)?;
    let periods = surface_periods(surface);
    let mut shapes = Vec::new();
    for &key in &face_.loops {
        let ring = body.loops.get(key)?;
        let mut uv: Vec<[f64; 2]> = Vec::new();
        for &coedge in &ring.coedges {
            let use_ = body.coedges.get(coedge)?;
            let edge = body.edges.get(use_.edge)?;
            let curve = body.curves.get(edge.curve)?;
            for k in 0..8 {
                let f = k as f64 / 8.0;
                let f = if use_.forward { f } else { 1.0 - f };
                let t = edge.start_parameter + (edge.end_parameter - edge.start_parameter) * f;
                let (mut u, mut v) = surface.parameters_at(curve.point_at(t))?;
                // Kept continuous across a periodic direction's seam.
                if let Some(&[pu, pv]) = uv.last() {
                    for (value, previous, period) in
                        [(&mut u, pu, periods[0]), (&mut v, pv, periods[1])]
                    {
                        if let Some(period) = period {
                            *value -= ((*value - previous) / period).round() * period;
                        }
                    }
                }
                uv.push([u, v]);
            }
        }
        if uv.len() < 3 {
            continue;
        }
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for point in &uv {
            for axis in 0..2 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
        let reach = (high[0] - low[0]).hypot(high[1] - low[1]);
        // A ring that comes back a period away wraps round the surface.
        let (first, last) = (uv[0], *uv.last()?);
        let wraps = periods.iter().enumerate().any(|(axis, period)| {
            period.is_some_and(|period| (last[axis] - first[axis]).abs() > 0.5 * period)
        });
        let area = (!wraps).then(|| {
            0.5 * (0..uv.len())
                .map(|i| {
                    let (p, q) = (uv[i], uv[(i + 1) % uv.len()]);
                    p[0] * q[1] - q[0] * p[1]
                })
                .sum::<f64>()
        });
        shapes.push(RingShape { key, reach, area });
    }
    (shapes.len() == face_.loops.len()).then_some(shapes)
}

/// Whether no other face shares an edge of the ring.
fn ring_is_free(body: &Body, ring: super::topology::LoopKey) -> bool {
    body.loops.get(ring).is_some_and(|ring| {
        ring.coedges.iter().all(|&coedge| {
            body.coedges
                .get(coedge)
                .and_then(|coedge| body.edges.get(coedge.edge))
                .is_some_and(|edge| edge.coedges.len() == 1)
        })
    })
}

fn reverse_ring(body: &mut Body, ring: super::topology::LoopKey) -> Option<()> {
    let coedges = {
        let ring = body.loops.get_mut(ring)?;
        ring.coedges.reverse();
        ring.provenance.soil();
        ring.coedges.clone()
    };
    for coedge in coedges {
        let coedge = body.coedges.get_mut(coedge)?;
        coedge.forward = !coedge.forward;
        coedge.pcurve = None;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Line3;
    use crate::topology::{Face, Lump, Shell, Vertex};
    use crate::{make, mass_properties};
    use ocs_geom::space::Plane;

    /// The body with every face on its own: its own vertices (moved by up
    /// to `jitter`), edges and shell, as a format that shares nothing
    /// between faces writes it.
    fn explode(body: &Body, jitter: f64) -> Body {
        let mut out = Body::new();
        for (index, (_, face)) in body.faces.iter().enumerate() {
            // Every other face moved along x.
            let wobble = if index % 2 == 0 { jitter } else { -jitter };
            let lump = out.lumps.insert(Lump {
                shells: Vec::new(),
                provenance: Provenance::Synthesized,
            });
            out.roots.push(lump);
            let shell = out.shells.insert(Shell {
                faces: Vec::new(),
                owner: lump,
                provenance: Provenance::Synthesized,
            });
            out.lumps.get_mut(lump).unwrap().shells.push(shell);
            let surface = out.surfaces.insert(body.surfaces.get(face.surface).unwrap().clone());
            let key = out.faces.insert(Face {
                surface,
                forward: face.forward,
                loops: Vec::new(),
                owner: shell,
                provenance: Provenance::Synthesized,
            });
            out.shells.get_mut(shell).unwrap().faces.push(key);
            let mut vertices: HashMap<VertexKey, VertexKey> = HashMap::new();
            for &ring in &face.loops {
                let loop_key = out.loops.insert(Loop {
                    coedges: Vec::new(),
                    owner: key,
                    provenance: Provenance::Synthesized,
                });
                out.faces.get_mut(key).unwrap().loops.push(loop_key);
                for &coedge in &body.loops.get(ring).unwrap().coedges {
                    let coedge = body.coedges.get(coedge).unwrap();
                    let edge = body.edges.get(coedge.edge).unwrap();
                    let mut vertex = |v: VertexKey| {
                        *vertices.entry(v).or_insert_with(|| {
                            let mut point = body.vertices.get(v).unwrap().point;
                            point[0] += wobble;
                            out.vertices.insert(Vertex {
                                point,
                                provenance: Provenance::Synthesized,
                            })
                        })
                    };
                    let (start, end) = (vertex(edge.start), vertex(edge.end));
                    let curve = out.curves.insert(body.curves.get(edge.curve).unwrap().clone());
                    let new_edge = out.edges.insert(Edge {
                        curve,
                        start_parameter: edge.start_parameter,
                        end_parameter: edge.end_parameter,
                        start,
                        end,
                        coedges: Vec::new(),
                        provenance: Provenance::Synthesized,
                    });
                    let use_ = out.coedges.insert(Coedge {
                        edge: new_edge,
                        forward: coedge.forward,
                        pcurve: None,
                        owner: loop_key,
                        provenance: Provenance::Synthesized,
                    });
                    out.edges.get_mut(new_edge).unwrap().coedges.push(use_);
                    out.loops.get_mut(loop_key).unwrap().coedges.push(use_);
                }
            }
        }
        out
    }

    fn volume(body: &Body) -> f64 {
        mass_properties(body).expect("mass").volume
    }

    #[test]
    fn faces_written_apart_sew_back_into_the_box() {
        let cube = make::cuboid([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
        let mut apart = explode(&cube, 0.0);
        assert_eq!((apart.vertices.len(), apart.edges.len(), apart.shells.len()), (24, 24, 6));
        let sewing = sew(&mut apart, 1e-6);
        assert_eq!(sewing.free_edges, 0);
        assert_eq!((sewing.vertices_glued, sewing.edges_glued), (16, 12));
        assert_eq!((apart.vertices.len(), apart.edges.len()), (8, 12));
        assert_eq!((apart.shells.len(), apart.lumps.len(), apart.roots.len()), (1, 1, 1));
        assert!(apart.validate().is_empty(), "{:?}", apart.validate());
        assert!((volume(&apart) - 6000.0).abs() < 1e-6);
    }

    #[test]
    fn curved_faces_written_apart_sew_back_into_the_cylinder() {
        let cylinder = make::cylinder([0.0, 0.0, 0.0], 5.0, 10.0).unwrap();
        let mut apart = explode(&cylinder, 0.0);
        let sewing = sew(&mut apart, 1e-6);
        assert_eq!(sewing.free_edges, 0, "{sewing:?}");
        assert!(apart.validate().is_empty(), "{:?}", apart.validate());
        let expected = std::f64::consts::PI * 25.0 * 10.0;
        assert!((volume(&apart) - expected).abs() < 1e-3 * expected);
    }

    #[test]
    fn a_gap_wider_than_the_file_says_closes_as_the_tolerance_doubles() {
        let cube = make::cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        let mut apart = explode(&cube, 2e-4);
        // At the stated tolerance some corners stay apart.
        let mut first = apart.clone();
        assert!(sew(&mut first, 1e-6).free_edges > 0);
        let sewing = sew_into_solid(&mut apart, 1e-6);
        assert_eq!(sewing.free_edges, 0, "{sewing:?}");
        assert!(sewing.tolerance >= 4e-4 && sewing.tolerance < 1e-3, "{sewing:?}");
        assert!(apart.validate().is_empty(), "{:?}", apart.validate());
        assert!((volume(&apart) - 1000.0).abs() < 0.1);
    }

    #[test]
    fn a_gap_wider_than_a_two_hundredth_of_the_box_stays_open() {
        let cube = make::cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        // 0.2 apart; the limit is 17.3 / 200 = 0.087.
        let mut apart = explode(&cube, 0.1);
        let limit = diagonal(&apart).unwrap() / 200.0;
        let sewing = sew_into_solid(&mut apart, 1e-6);
        assert!(sewing.free_edges > 0, "{sewing:?}");
        assert!((sewing.tolerance - limit).abs() < 1e-12 && limit < 0.2);
    }

    /// A planar face from a polygon, in its own shell.
    fn polygon(body: &mut Body, corners: &[[f64; 3]]) -> FaceKey {
        let lump = body.lumps.insert(Lump {
            shells: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        body.roots.push(lump);
        let shell = body.shells.insert(Shell {
            faces: Vec::new(),
            owner: lump,
            provenance: Provenance::Synthesized,
        });
        body.lumps.get_mut(lump).unwrap().shells.push(shell);
        let surface = body.surfaces.insert(Surface::Plane(Plane::XY));
        let face = body.faces.insert(Face {
            surface,
            forward: true,
            loops: Vec::new(),
            owner: shell,
            provenance: Provenance::Synthesized,
        });
        body.shells.get_mut(shell).unwrap().faces.push(face);
        add_ring(body, face, corners);
        face
    }

    fn add_ring(body: &mut Body, face: FaceKey, corners: &[[f64; 3]]) {
        let ring = body.loops.insert(Loop {
            coedges: Vec::new(),
            owner: face,
            provenance: Provenance::Synthesized,
        });
        body.faces.get_mut(face).unwrap().loops.push(ring);
        let vertices: Vec<VertexKey> = corners
            .iter()
            .map(|&point| {
                body.vertices.insert(Vertex {
                    point,
                    provenance: Provenance::Synthesized,
                })
            })
            .collect();
        for i in 0..corners.len() {
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            let curve = body.curves.insert(Curve3::Line(Line3 {
                origin: a,
                direction: (Vec3::from(b) - Vec3::from(a)).to_array(),
            }));
            let edge = body.edges.insert(Edge {
                curve,
                start_parameter: 0.0,
                end_parameter: 1.0,
                start: vertices[i],
                end: vertices[(i + 1) % corners.len()],
                coedges: Vec::new(),
                provenance: Provenance::Synthesized,
            });
            let coedge = body.coedges.insert(Coedge {
                edge,
                forward: true,
                pcurve: None,
                owner: ring,
                provenance: Provenance::Synthesized,
            });
            body.edges.get_mut(edge).unwrap().coedges.push(coedge);
            body.loops.get_mut(ring).unwrap().coedges.push(coedge);
        }
    }

    #[test]
    fn an_edge_a_neighbours_corner_lands_inside_is_split_and_glued() {
        // A 2 × 1 strip under two unit squares: the strip's top edge meets
        // both squares' bottom edges and the corner between them.
        let mut body = Body::new();
        polygon(&mut body, &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
        polygon(&mut body, &[[0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 2.0, 0.0]]);
        polygon(&mut body, &[[1.0, 1.0, 0.0], [2.0, 1.0, 0.0], [2.0, 2.0, 0.0], [1.0, 2.0, 0.0]]);
        let sewing = sew(&mut body, 1e-9);
        assert_eq!(sewing.edges_split, 1, "{sewing:?}");
        assert_eq!(sewing.edges_glued, 3, "{sewing:?}");
        assert_eq!(sewing.free_edges, 7);
        assert_eq!((body.edges.len(), body.vertices.len()), (10, 8));
        assert_eq!(body.shells.len(), 1);
        assert!(body.validate().is_empty(), "{:?}", body.validate());
    }

    #[test]
    fn faces_facing_opposite_ways_turn_to_agree_when_sewn() {
        let mut body = Body::new();
        polygon(&mut body, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
        // Its neighbour runs clockwise: the shared edge the same way.
        let other =
            polygon(&mut body, &[[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 1.0, 0.0], [2.0, 0.0, 0.0]]);
        let sewing = sew(&mut body, 1e-9);
        assert_eq!((sewing.edges_glued, sewing.faces_turned), (1, 1), "{sewing:?}");
        assert!(body.validate().is_empty(), "{:?}", body.validate());
        assert!(!body.faces.get(other).unwrap().forward);
    }

    #[test]
    fn a_solid_needs_no_loop_repairs() {
        for body in [
            make::cuboid([0.0; 3], [1.0, 2.0, 3.0]).unwrap(),
            make::cylinder([0.0; 3], 2.0, 3.0).unwrap(),
            make::cone([0.0; 3], 2.0, 3.0).unwrap(),
            make::sphere([0.0; 3], 2.0).unwrap(),
        ] {
            let mut copy = body.clone();
            assert_eq!(fix_loops(&mut copy), LoopRepairs::default());
        }
    }

    #[test]
    fn loops_out_of_order_and_against_the_face_are_put_right() {
        let mut body = Body::new();
        let face = polygon(&mut body, &[[1.0, 1.0, 0.0], [1.0, 2.0, 0.0], [2.0, 2.0, 0.0], [2.0, 1.0, 0.0]]);
        // The hole first, the outline after it, both clockwise seen along
        // the face's normal.
        add_ring(&mut body, face, &[[0.0, 0.0, 0.0], [0.0, 3.0, 0.0], [3.0, 3.0, 0.0], [3.0, 0.0, 0.0]]);
        let repairs = fix_loops(&mut body);
        assert_eq!(repairs.reordered, 1);
        // The face turns to agree with its outline, and the hole, now
        // running with the face, turns round.
        assert_eq!(repairs.reoriented, 2);
        let face = body.faces.get(face).unwrap();
        assert!(!face.forward);
        let first = body.loops.get(face.loops[0]).unwrap();
        let start = body.edges.get(body.coedges.get(first.coedges[0]).unwrap().edge).unwrap();
        assert_eq!(body.vertices.get(start.start).unwrap().point, [0.0, 0.0, 0.0]);
        assert!(body.validate().is_empty(), "{:?}", body.validate());
    }

    #[test]
    fn coedges_out_of_ring_order_are_chained() {
        let mut body = Body::new();
        let face = polygon(&mut body, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
        let ring = body.faces.get(face).unwrap().loops[0];
        body.loops.get_mut(ring).unwrap().coedges.swap(1, 3);
        assert!(!body.validate().is_empty());
        let repairs = fix_loops(&mut body);
        assert_eq!(repairs.rechained, 1);
        assert!(body.validate().is_empty(), "{:?}", body.validate());
    }
}
