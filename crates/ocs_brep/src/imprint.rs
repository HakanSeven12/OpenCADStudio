//! Cutting each solid's faces where the other's pass through them.
//!
//! The step between finding the intersection curves and deciding what to
//! keep. Afterwards, every face of either solid is wholly inside the other or
//! wholly outside it — never partly both — so classifying a piece is one
//! question with one answer instead of a face that would have to be described
//! as "inside over here".
//!
//! Both solids are imprinted, not one. A union has to keep the outer part of
//! *each*, so each needs the other's curves cut into it.
//!
//! # Why it can fail, and why that is reported
//!
//! A face pair whose intersection has no closed form, a pair of coincident
//! faces, a cut this kernel cannot make: each leaves the imprint incomplete
//! in a way the caller cannot see by looking at the result. A boolean run on
//! a half-imprinted body produces a solid — one with a wall missing. So the
//! failures are returned, and a boolean refuses on them.

use super::bounds::{face_bounds, Aabb};
use super::geometry::{Curve3, Surface};
use super::intersect::{surfaces, Meeting};
use super::split::{split_edge, split_face};
use super::Provenance;
use super::topology::{
    Body, Coedge, CoedgeKey, Edge, Face, FaceKey, Loop, LoopKey, Vertex, VertexKey,
};
use ocs_geom::space::{NurbsCurve3, Parameterization, Vec3};
use std::collections::{HashMap, HashSet};

/// Why an imprint could not be completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snag {
    /// Two faces meet along something this kernel has no closed form for.
    NoClosedForm,
    /// Two faces lie on the same surface. Deciding what that leaves needs
    /// their overlap worked out in parameter space, which is its own
    /// operation.
    Coincident,
    /// The curves were found but a face could not be cut along one of them —
    /// a cut crossing the boundary more than twice, or closing inside the
    /// face.
    CutRefused,
}

/// What an imprint did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imprint {
    /// How many faces were cut in two, across both solids.
    pub cuts: usize,
    /// How many face pairs were found to meet at all.
    pub meetings: usize,
}

/// Cuts each body's faces along the curves it shares with the other.
///
/// Neither body's shape changes — an imprint only adds edges — so the result
/// still passes [`Body::validate`] and still has the same volume.
///
/// `tolerance` is passed through to the intersection and the cutting.
pub fn imprint(a: &mut Body, b: &mut Body, tolerance: f64) -> Result<Imprint, Snag> {
    imprint_reaching(a, b, tolerance, false)
}

/// [`imprint`], with `reach` a line found on a curved face cutting each
/// body's face across all of that face rather than only where the two
/// faces' boxes overlap: the meeting may end inside a face, where another
/// one takes over, and a cut has to reach the face's edges. Extended so,
/// a line also cuts slivers off beside a round's tangent edge; it is the
/// second try.
pub(super) fn imprint_reaching(
    a: &mut Body,
    b: &mut Body,
    tolerance: f64,
    reach: bool,
) -> Result<Imprint, Snag> {
    let meetings = shared_curves(a, b, tolerance, reach)?;
    let count = meetings.len();
    // Islands first, while the faces they go into are still the ones found;
    // the largest first, so one lying inside another goes into the face the
    // first made rather than beside it.
    let mut islands: Vec<(bool, FaceKey, LoopKey, f64)> = Vec::new();
    for (into_a, face, _, ring) in meetings.iter().flat_map(|meeting| &meeting.islands) {
        if islands.iter().any(|(side, _, seen, _)| side == into_a && seen == ring) {
            continue;
        }
        let from: &Body = if *into_a { b } else { a };
        let size = loop_walk(from, *ring, 4).map_or(0.0, |walk| {
            walk.iter().map(|(point, _)| point.distance(walk[0].0)).fold(0.0, f64::max)
        });
        islands.push((*into_a, *face, *ring, size));
    }
    islands.sort_by(|x, y| y.3.total_cmp(&x.3));
    let mut made: Vec<(bool, FaceKey, FaceKey)> = Vec::new();
    for (into_a, face, ring, _) in islands {
        let (into, from): (&mut Body, &Body) = if into_a { (a, b) } else { (b, a) };
        let probe = loop_walk(from, ring, 4).and_then(|walk| walk.first().map(|(point, _)| *point));
        let fit = loop_walk(from, ring, 8).map_or(tolerance, |walk| loop_fit(&walk, tolerance));
        // The innermost face made so far from this one that holds the loop.
        let target = made
            .iter()
            .rev()
            .filter(|(side, origin, _)| *side == into_a && *origin == face)
            .map(|(_, _, island)| *island)
            .find(|island| {
                probe.is_some_and(|point| {
                    super::classify::face_distance(into, *island, point.to_array(), fit)
                        .is_some_and(|gap| gap <= fit)
                })
            })
            .unwrap_or(face);
        let island = copy_island(into, target, from, ring).ok_or(Snag::CutRefused)?;
        made.push((into_a, face, island));
    }
    // Face pairs on the same two surfaces all report the same curve — every
    // part of a divided sphere meets a plane in one circle — and each copy
    // would be tried against every face again for nothing.
    // Where each curve was found, too: between which faces of each body.
    let mut curves: Vec<Curve3> = Vec::new();
    let mut found: Vec<[Option<Aabb>; 2]> = Vec::new();
    for meeting in &meetings {
        for curve in &meeting.curves {
            match curves.iter().position(|seen| same_curve(seen, curve, tolerance)) {
                Some(index) => {
                    for side in 0..2 {
                        found[index][side] = match (found[index][side], meeting.boxes[side]) {
                            (Some(mut known), Some(more)) => {
                                known.merge(more);
                                Some(known)
                            }
                            _ => None,
                        };
                    }
                }
                None => {
                    curves.push(curve.clone());
                    found.push(meeting.boxes);
                }
            }
        }
    }
    // One intersection reached two ways — a file's spline for a shared
    // wall's edge, and the same curve found between the faces that meet
    // there — is two splines a fit apart, which cross and recross each other
    // and cut slivers between them. Only the longest of such copies cuts.
    let mut splines: Vec<(f64, Vec<[f64; 3]>, usize)> = curves
        .iter()
        .enumerate()
        .filter_map(|(index, curve)| {
            let Curve3::Nurbs(spline) = curve else {
                return None;
            };
            let (start, end) = spline.domain();
            let points: Vec<[f64; 3]> = (0..=32)
                .map(|step| curve.point_at(start + (end - start) * step as f64 / 32.0))
                .collect();
            let length = points
                .windows(2)
                .map(|pair| Vec3::from(pair[0]).distance(Vec3::from(pair[1])))
                .sum::<f64>();
            Some((length, points, index))
        })
        .collect();
    splines.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut dropped = Vec::new();
    // Copies only for the same body: a line kept to one body's face and the
    // same line kept to the other's each cut their own.
    let sides = |index: usize| found[index].map(|bounds| bounds != Some(Aabb::EMPTY));
    for (position, (length, points, index)) in splines.iter().enumerate() {
        let fit = tolerance.max(length * 1e-3);
        let copied = splines[..position].iter().any(|(_, _, longer)| {
            !dropped.contains(longer)
                && sides(*longer) == sides(*index)
                && points.iter().all(|point| {
                    let curve = &curves[*longer];
                    Vec3::from(curve.point_at(curve.parameter_at(*point)))
                        .distance(Vec3::from(*point))
                        <= fit
                })
        });
        if copied {
            dropped.push(*index);
        }
    }
    let mut kept: Vec<(Curve3, [Option<Aabb>; 2])> = curves
        .into_iter()
        .zip(found)
        .enumerate()
        .filter_map(|(index, pair)| (!dropped.contains(&index)).then_some(pair))
        .collect();
    // Lines before circles. A circle cut first is an island, and a line
    // tangent to it afterwards — a wall running into the round of a corner —
    // lands on the island's rim and bridges it to the face's rim; cut after
    // the lines, the circle touches the walls at two places and divides the
    // corner off as it should.
    kept.sort_by_key(|(curve, _)| matches!(curve, Curve3::Circle(_) | Curve3::Ellipse(_)));
    let (mut curves, mut found): (Vec<Curve3>, Vec<[Option<Aabb>; 2]>) =
        kept.into_iter().unzip();
    let (more, boxes) = chained_meetings(a, b, &curves, &found, tolerance);
    curves.extend(more);
    found.extend(boxes);
    let (curves, found) = crossing_loops_apart(curves, found, tolerance);
    // A curve cuts one body only where the other body's faces it was found
    // with are. A traced curve runs round its whole surface, and on a part
    // whose fillets share one torus it otherwise cut every fillet, wherever
    // the other body was or was not: faces by the hundred, minutes of work.
    let beside =
        |side: usize| -> Vec<Option<Aabb>> { found.iter().map(|boxes| boxes[side]).collect() };
    let (beside_b, beside_a) = (beside(1), beside(0));
    // A curve that only touches a face's boundary lands on a corner, and the
    // corner may be one the other body lends this one only once both are
    // cut; so the curves go round again until they cut nothing new.
    // After the first round only faces the last one changed — new, or with
    // an edge split under them — can cut differently; the rest were tried
    // with every curve already.
    let mut cuts = 0;
    let mut changed: Option<[HashSet<FaceKey>; 2]> = None;
    let mut last = usize::MAX;
    for _ in 0..4 {
        let before = [outlines(a), outlines(b)];
        let only = changed.as_ref();
        let made = cut_along(a, &curves, &beside_b, only.map(|sets| &sets[0]), tolerance)?
            + cut_along(b, &curves, &beside_a, only.map(|sets| &sets[1]), tolerance)?;
        cuts += made;
        // Stitching needs matching edge partitions on both bodies.
        // A round that cut nothing is the last: corners may settle then.
        align_corners(a, b, tolerance, made == 0);
        align_corners(b, a, tolerance, made == 0);
        if made == 0 {
            break;
        }
        // Rounds settle what the first cut left at its corners, each with
        // less to do. One doing much more than the last is cutting its own
        // cuts: it will not settle, and the four rounds would only take
        // longer. A few corners more is still settling.
        if made > 32 && made * 2 > last.saturating_mul(3) {
            return Err(Snag::CutRefused);
        }
        last = made;
        let differs = |body: &Body, before: &HashMap<FaceKey, usize>| -> HashSet<FaceKey> {
            outlines(body)
                .into_iter()
                .filter(|(face, count)| before.get(face) != Some(count))
                .map(|(face, _)| face)
                .collect()
        };
        changed = Some([differs(a, &before[0]), differs(b, &before[1])]);
    }
    // Cuts landing on a spline put their corners where that spline runs —
    // a fit off where the other body's own corner is. Within that fit, a
    // corner the imprint made is the same corner as one already there. A
    // traced spline is settled to the tolerance only at its own samples and
    // runs a fit off between them, some millionths of the body or more.
    let reach = [&*a, &*b]
        .iter()
        .filter_map(|body| super::bounds::body_bounds(body))
        .map(|bounds| Vec3::from(bounds.min).distance(Vec3::from(bounds.max)))
        .fold(tolerance, |reach, size| reach.max(size * 1e-5));
    snap_cut_vertices(a, b, tolerance, reach);
    snap_cut_vertices(b, a, tolerance, reach);
    fuse_cut_vertices(a, tolerance, reach);
    fuse_cut_vertices(b, tolerance, reach);
    // Each body fused its own pair of near corners and may have kept the
    // other one of the two; the other body's corner settles which.
    snap_cut_vertices(a, b, tolerance, reach);
    snap_cut_vertices(b, a, tolerance, reach);
    split_splines_at_corners(b, a, reach);
    split_splines_at_corners(a, b, reach);
    separate_pinched_loops(a, tolerance);
    separate_pinched_loops(b, tolerance);
    collapse_lenses(a, tolerance);
    collapse_lenses(b, tolerance);
    Ok(Imprint {
        cuts,
        meetings: count,
    })
}

/// Two traced meetings that end at one point on a curved face of one body,
/// joined: each was kept to where both bodies are, so a corner of the other
/// body standing in the face ends both there — a box's corner in a cone —
/// and neither alone runs from rim to rim to divide the face. Joined they
/// do; each joined pair cuts only the body whose face carries it.
fn chained_meetings(
    a: &Body,
    b: &Body,
    curves: &[Curve3],
    found: &[[Option<Aabb>; 2]],
    tolerance: f64,
) -> (Vec<Curve3>, Vec<[Option<Aabb>; 2]>) {
    let size = [a, b]
        .iter()
        .filter_map(|body| super::bounds::body_bounds(body))
        .map(|bounds| Vec3::from(bounds.min).distance(Vec3::from(bounds.max)))
        .fold(tolerance, f64::max);
    let fit = tolerance.max(size * 1e-4);
    let carried = |body: &Body, curve: &Curve3| {
        body.face_keys().any(|face| {
            let node = body.faces.get(face);
            let Some(surface) = node.and_then(|node| body.surfaces.get(node.surface)) else {
                return false;
            };
            !matches!(surface, Surface::Plane(_))
                && (0..=16).all(|step| {
                    surface.distance_to(curve.point_at(step as f64 / 16.0)).abs() <= fit
                })
        })
    };
    let merge = |one: Option<Aabb>, other: Option<Aabb>| match (one, other) {
        (Some(mut one), Some(other)) => {
            one.merge(other);
            Some(one)
        }
        _ => None,
    };
    let mut joined = Vec::new();
    let mut boxes = Vec::new();
    for i in 0..curves.len() {
        for j in i + 1..curves.len() {
            let (Curve3::Nurbs(one), Curve3::Nurbs(other)) = (&curves[i], &curves[j]) else {
                continue;
            };
            let pair = crossing_join(one, other, fit, &[a, b]);
            let Some(path) = pair.map(Curve3::Nurbs) else {
                continue;
            };
            let empty = Some(Aabb::EMPTY);
            for (side, body) in [(0, a), (1, b)] {
                if !carried(body, &path) {
                    continue;
                }
                // `found[..][1]` is where body b met it, which is what limits
                // the cut of body a, and the other way round.
                let other = merge(found[i][1 - side], found[j][1 - side]);
                joined.push(path.clone());
                boxes.push(if side == 0 { [empty, other] } else { [other, empty] });
            }
        }
    }
    (joined, boxes)
}

/// Two splines that cross — each running a little past the corner where
/// the other ends — joined at the crossing: the longer piece of each, the
/// first ending where the second starts. `None` where they do not come
/// within `fit` of each other, or meet at an end of either.
fn crossing_join(
    one: &NurbsCurve3,
    other: &NurbsCurve3,
    fit: f64,
    bodies: &[&Body; 2],
) -> Option<NurbsCurve3> {
    let (one_from, one_to) = one.domain();
    let (other_from, other_to) = other.domain();
    let at = |from: f64, to: f64, step: usize| from + (to - from) * step as f64 / 64.0;
    let mut best = (f64::INFINITY, one_from, other_from);
    for i in 0..=64 {
        let u = at(one_from, one_to, i);
        let point = Vec3::from(one.point_at_knot(u));
        for k in 0..=64 {
            let v = at(other_from, other_to, k);
            let gap = point.distance(Vec3::from(other.point_at_knot(v)));
            if gap < best.0 {
                best = (gap, u, v);
            }
        }
    }
    let (_, mut u, mut v) = best;
    for _ in 0..8 {
        u = one.parameter_at(other.point_at_knot(v));
        v = other.parameter_at(one.point_at_knot(u));
    }
    let gap = Vec3::from(one.point_at_knot(u)).distance(Vec3::from(other.point_at_knot(v)));
    // The corner itself, where the other body's edge pierces the curved face
    // both splines lie on: each spline stops a fit short of it, and joined
    // between their ends the path kinks there.
    let middle = (Vec3::from(one.point_at_knot(u)) + Vec3::from(other.point_at_knot(v))) * 0.5;
    let corner = pierced_corner(bodies, middle, fit);
    if let Some(corner) = corner {
        u = one.parameter_at(corner.to_array());
        v = other.parameter_at(corner.to_array());
    }
    let inner = |value: f64, from: f64, to: f64| {
        let margin = (to - from) * 1e-6;
        value > from + margin && value < to - margin
    };
    if gap > fit || !inner(u, one_from, one_to) || !inner(v, other_from, other_to) {
        return None;
    }
    let length = |curve: &NurbsCurve3| {
        let (from, to) = curve.domain();
        (0..16)
            .map(|step| {
                Vec3::from(curve.point_at_knot(at(from, to, step * 4)))
                    .distance(Vec3::from(curve.point_at_knot(at(from, to, step * 4 + 4))))
            })
            .sum::<f64>()
    };
    let (one_before, one_after) = one.split_at(u)?;
    let (other_before, other_after) = other.split_at(v)?;
    // Into the crossing along the one, out of it along the other.
    let into = if length(&one_before) >= length(&one_after) {
        one_before
    } else {
        one_after.reversed()?
    };
    let out = if length(&other_after) >= length(&other_before) {
        other_after
    } else {
        other_before.reversed()?
    };
    into.joined_through(&out, corner.map(Vec3::to_array))
}

/// Where a straight edge of either body, passing within `fit` of `near`,
/// pierces a curved face of the other near there: a corner of one body
/// standing in a face of the other.
fn pierced_corner(bodies: &[&Body; 2], near: Vec3, fit: f64) -> Option<Vec3> {
    let mut best: Option<(f64, Vec3)> = None;
    for (index, body) in bodies.iter().enumerate() {
        let curved: Vec<&Surface> = bodies[1 - index]
            .faces
            .iter()
            .filter_map(|(_, face)| bodies[1 - index].surfaces.get(face.surface))
            .filter(|surface| !matches!(surface, Surface::Plane(_)))
            .collect();
        for (_, edge) in body.edges.iter() {
            let Some(Curve3::Line(line)) = body.curves.get(edge.curve) else {
                continue;
            };
            let origin = Vec3::from(line.origin);
            let along = Vec3::from(line.direction);
            let squared = along.length_squared();
            if squared <= 0.0 {
                continue;
            }
            let point = |t: f64| origin + along * t;
            let start = (near - origin).dot(along) / squared;
            if point(start).distance(near) > fit * 10.0 {
                continue;
            }
            for surface in &curved {
                // Secant steps on the signed distance along the line.
                let step = fit / squared.sqrt();
                let (mut t0, mut t1) = (start - step, start + step);
                let mut f0 = surface.distance_to(point(t0).to_array());
                let mut f1 = surface.distance_to(point(t1).to_array());
                for _ in 0..32 {
                    if (f1 - f0).abs() < 1e-300 {
                        break;
                    }
                    let t2 = t1 - f1 * (t1 - t0) / (f1 - f0);
                    (t0, f0) = (t1, f1);
                    t1 = t2;
                    f1 = surface.distance_to(point(t1).to_array());
                    if f1.abs() <= 1e-12 * (1.0 + near.length()) {
                        break;
                    }
                }
                let hit = point(t1);
                let gap = hit.distance(near);
                let closer = best.is_none_or(|(seen, _)| gap < seen);
                if f1.abs() <= fit * 1e-3 && gap <= fit * 10.0 && closer {
                    best = Some((gap, hit));
                }
            }
        }
    }
    best.map(|(_, corner)| corner)
}

/// A face whose one loop passes a corner twice, with no edge between the two
/// passes, is two regions touching at that point: cuts crossing where two
/// faces of the other body meet tangentially, a fillet running into its
/// wall. The two lobes may lie on opposite sides of the other body, and one
/// face has only one side; each lobe becomes a face of its own.
fn separate_pinched_loops(body: &mut Body, tolerance: f64) {
    let mut pending: Vec<FaceKey> = body.face_keys().collect();
    while let Some(face) = pending.pop() {
        let Some(node) = body.faces.get(face).cloned() else {
            continue;
        };
        let [ring] = node.loops[..] else {
            continue;
        };
        let Some(coedges) = body.loops.get(ring).map(|ring| ring.coedges.clone()) else {
            continue;
        };
        let starts: Option<Vec<VertexKey>> = coedges
            .iter()
            .map(|coedge| body.coedge_vertices(*coedge).map(|(start, _)| start))
            .collect();
        let Some(starts) = starts else {
            continue;
        };
        let Some(edges) = coedges
            .iter()
            .map(|coedge| body.coedges.get(*coedge).map(|coedge| coedge.edge))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        // A loop using an edge twice — a seam, a bridge to a hole — comes
        // back through its corners by that edge, not round a second region:
        // a part taking one use of it and leaving the other is no lobe.
        let keeps_pairs = |first: usize, second: usize| {
            edges.iter().enumerate().all(|(index, edge)| {
                let inside = |at: usize| (first..second).contains(&at);
                edges
                    .iter()
                    .enumerate()
                    .filter(|(other, twin)| *other != index && *twin == edge)
                    .all(|(other, _)| inside(other) == inside(index))
            })
        };
        // Each corner passed twice is a place the loop may come apart;
        // the first where both parts are regions of their own is taken.
        let pairs: Vec<(usize, usize)> = (0..starts.len())
            .flat_map(|first| (first + 1..starts.len()).map(move |second| (first, second)))
            .filter(|(first, second)| starts[*first] == starts[*second])
            .filter(|(first, second)| keeps_pairs(*first, *second))
            .collect();
        let mut split = None;
        for (first, second) in pairs {
            let pair = (first, second);
            if let Some(other) = split_lobe(body, face, &node, ring, &coedges, pair, tolerance) {
                split = Some(other);
                break;
            }
        }
        // A cutter touching an edge of the face at two points, a lens
        // between: the loop passes each touch twice, the lens's two sides
        // apart in it, with the rest of the loop between them on one side
        // and round the far side of a seam on the other.
        if split.is_none() {
            let count = starts.len();
            let mut nested = Vec::new();
            for p1 in 0..count {
                for p2 in p1 + 3..count {
                    if starts[p1] != starts[p2] {
                        continue;
                    }
                    for q1 in p1 + 1..p2 {
                        for q2 in q1 + 1..p2 {
                            if starts[q1] == starts[q2] && starts[q1] != starts[p1] {
                                nested.push([p1, q1, q2, p2]);
                            }
                        }
                    }
                }
            }
            split = nested.into_iter().find_map(|corners| {
                lens_round_seam(body, face, ring, &coedges, corners, tolerance)
            });
        }
        if let Some(other) = split {
            pending.push(face);
            pending.push(other);
        }
    }
}

/// The lens of a loop passing two corners twice each, nested, cut off as a
/// face of its own; the two runs left join round the seam they each cross
/// once, the seam edge going. Nothing changes and `None` comes back unless
/// both parts are regions wound alike.
fn lens_round_seam(
    body: &mut Body,
    face: FaceKey,
    ring: LoopKey,
    coedges: &[CoedgeKey],
    [p1, q1, q2, p2]: [usize; 4],
    tolerance: f64,
) -> Option<FaceKey> {
    let lens: Vec<CoedgeKey> = coedges[p1..q1].iter().chain(&coedges[q2..p2]).copied().collect();
    let inner: Vec<CoedgeKey> = coedges[q1..q2].to_vec();
    let outer: Vec<CoedgeKey> = coedges[p2..].iter().chain(&coedges[..p1]).copied().collect();
    let edge_of = |coedge: &CoedgeKey| body.coedges.get(*coedge).map(|coedge| coedge.edge);
    // The seam: one edge, used once in each run.
    let (outer_at, inner_at) = outer.iter().enumerate().find_map(|(index, coedge)| {
        let edge = edge_of(coedge)?;
        let other = inner.iter().position(|twin| edge_of(twin) == Some(edge))?;
        let uses = body.edges.get(edge)?.coedges.len();
        (uses == 2).then_some((index, other))
    })?;
    let seam = edge_of(&outer[outer_at])?;
    // The inner run from just past its seam use round to just before it
    // goes where the outer run's seam use went.
    let mut joined: Vec<CoedgeKey> = outer[..outer_at].to_vec();
    joined.extend(inner[inner_at + 1..].iter().chain(&inner[..inner_at]));
    joined.extend(&outer[outer_at + 1..]);
    let mut trial = body.clone();
    let node = trial.faces.get(face)?.clone();
    let lens_face = trial.faces.insert(Face {
        loops: Vec::new(),
        provenance: Provenance::Synthesized,
        ..node.clone()
    });
    let lens_ring = trial.loops.insert(Loop {
        coedges: lens.clone(),
        owner: lens_face,
        provenance: Provenance::Synthesized,
    });
    for coedge in &lens {
        trial.coedges.get_mut(*coedge)?.owner = lens_ring;
    }
    trial.faces.get_mut(lens_face)?.loops = vec![lens_ring];
    trial.loops.get_mut(ring)?.coedges = joined;
    for coedge in [outer[outer_at], inner[inner_at]] {
        trial.coedges.remove(coedge);
    }
    trial.edges.remove(seam);
    trial.shells.get_mut(node.owner)?.faces.push(lens_face);
    let area = |face: FaceKey| {
        super::pcurve::face_boundary_parts(&trial, face, tolerance).map_or(0.0, |parts| {
            parts.iter().map(|(_, part)| part.chain_area()).sum::<f64>()
        })
    };
    if area(face) * area(lens_face) <= 0.0 || !trial.validate().is_empty() {
        return None;
    }
    if let Some(node) = trial.faces.get_mut(face) {
        node.provenance.soil();
    }
    *body = trial;
    Some(lens_face)
}

/// [`separate_pinched_loops`] at one corner the loop passes twice: the
/// stretch between the two passes becomes a face of its own, or nothing
/// changes and `None` comes back where the two parts are not both regions.
fn split_lobe(
    body: &mut Body,
    face: FaceKey,
    node: &Face,
    ring: LoopKey,
    coedges: &[CoedgeKey],
    (first, second): (usize, usize),
    tolerance: f64,
) -> Option<FaceKey> {
    let lobe: Vec<CoedgeKey> = coedges[first..second].to_vec();
    let rest: Vec<CoedgeKey> =
        coedges[second..].iter().chain(&coedges[..first]).copied().collect();
    let owner = node.owner;
    body.shells.get(owner)?;
    let other = body.faces.insert(Face {
        loops: Vec::new(),
        provenance: Provenance::Synthesized,
        ..node.clone()
    });
    let other_ring = body.loops.insert(Loop {
        coedges: lobe.clone(),
        owner: other,
        provenance: Provenance::Synthesized,
    });
    for coedge in &lobe {
        if let Some(coedge) = body.coedges.get_mut(*coedge) {
            coedge.owner = other_ring;
        }
    }
    if let Some(face) = body.faces.get_mut(other) {
        face.loops = vec![other_ring];
    }
    if let Some(ring) = body.loops.get_mut(ring) {
        ring.coedges = rest.clone();
    }
    // On a closed surface a lobe may run round it rather than round a
    // region; then the loop was one region after all. A lobe wound the
    // other way is a hole the loop dips into, not a second region.
    let area = |face: FaceKey| {
        super::pcurve::face_boundary_parts(body, face, tolerance).map_or(0.0, |parts| {
            parts.iter().map(|(_, part)| part.chain_area()).sum::<f64>()
        })
    };
    let closes = |face: FaceKey| {
        let Some(surface) = body.faces.get(face).and_then(|f| body.surfaces.get(f.surface))
        else {
            return false;
        };
        let gap = super::pcurve::periods(surface)
            .iter()
            .flatten()
            .fold(tolerance, |gap, period| gap.max(period * 1e-3));
        super::pcurve::face_boundary_parts(body, face, tolerance).is_some_and(|parts| {
            let (Some((_, head)), Some((_, tail))) = (parts.first(), parts.last()) else {
                return false;
            };
            let (from, to) = (head.point_at(0.0), tail.point_at(1.0));
            (from[0] - to[0]).hypot(from[1] - to[1]) <= gap
        })
    };
    if !(closes(face) && closes(other) && area(face) * area(other) > 0.0) {
        for coedge in &lobe {
            if let Some(coedge) = body.coedges.get_mut(*coedge) {
                coedge.owner = ring;
            }
        }
        if let Some(ring) = body.loops.get_mut(ring) {
            ring.coedges = coedges.to_vec();
        }
        body.loops.remove(other_ring);
        body.faces.remove(other);
        return None;
    }
    if let Some(shell) = body.shells.get_mut(owner) {
        shell.faces.push(other);
    }
    if let Some(node) = body.faces.get_mut(face) {
        node.provenance.soil();
    }
    Some(other)
}

/// A face bounded by two edges a tolerance apart all along — a chord and the
/// arc it cuts off a hair from a corner, two curves through one point — has
/// no inside to classify. It goes, and the two edges become one, joining
/// the faces either side of it.
fn collapse_lenses(body: &mut Body, tolerance: f64) {
    let faces: Vec<FaceKey> = body.face_keys().collect();
    for face in faces {
        let Some(node) = body.faces.get(face) else {
            continue;
        };
        let [ring] = node.loops[..] else {
            continue;
        };
        let shell = node.owner;
        let Some(&[first, second]) = body.loops.get(ring).map(|ring| &ring.coedges[..]) else {
            continue;
        };
        let (Some(one), Some(other)) = (body.coedges.get(first), body.coedges.get(second)) else {
            continue;
        };
        let (keep, drop) = (one.edge, other.edge);
        let (Some(kept), Some(dropped)) = (body.edges.get(keep), body.edges.get(drop)) else {
            continue;
        };
        let ends = |edge: &Edge| [edge.start, edge.end];
        let same_ends = ends(kept) == ends(dropped)
            || ends(kept) == [dropped.end, dropped.start];
        if keep == drop || !same_ends || kept.coedges.len() != 2 || dropped.coedges.len() != 2 {
            continue;
        }
        // A lens wide enough to hold a point is a face like any other, and
        // the other body has its edges too.
        if super::boolean::interior_point(body, face, tolerance).is_some() {
            continue;
        }
        let (Some(along), Some(beside)) =
            (body.curves.get(kept.curve), body.curves.get(dropped.curve))
        else {
            continue;
        };
        let close = (0..=8).all(|step| {
            let t = dropped.start_parameter
                + (dropped.end_parameter - dropped.start_parameter) * step as f64 / 8.0;
            let point = beside.point_at(t);
            Vec3::from(along.point_at(along.parameter_at(point))).distance(Vec3::from(point))
                <= tolerance
        });
        if !close {
            continue;
        }
        // The use of each edge outside the lens.
        let outer = |edge: &Edge, inner: CoedgeKey| {
            edge.coedges.iter().copied().find(|coedge| *coedge != inner)
        };
        let (Some(stays), Some(moves)) = (outer(kept, first), outer(dropped, second)) else {
            continue;
        };
        let start = kept.start;
        let Some((from, _)) = body.coedge_vertices(moves) else {
            continue;
        };
        if let Some(coedge) = body.coedges.get_mut(moves) {
            coedge.edge = keep;
            coedge.forward = from == start;
            coedge.pcurve = None;
            coedge.provenance.soil();
        }
        if let Some(edge) = body.edges.get_mut(keep) {
            edge.coedges = vec![stays, moves];
            edge.provenance.soil();
        }
        body.edges.remove(drop);
        body.coedges.remove(first);
        body.coedges.remove(second);
        body.loops.remove(ring);
        body.faces.remove(face);
        if let Some(shell) = body.shells.get_mut(shell) {
            shell.faces.retain(|other| *other != face);
        }
    }
}

/// The corners on a spline edge: the only ones a fit can have put off.
fn on_splines(body: &Body) -> HashMap<VertexKey, bool> {
    // A straight spline — a line kept to where two faces meet — is exact
    // but for where it was cut off; a curved one is a fit throughout.
    let mut out: HashMap<VertexKey, bool> = HashMap::new();
    for (_, edge) in body.edges.iter() {
        let Some(Curve3::Nurbs(spline)) = body.curves.get(edge.curve) else {
            continue;
        };
        for key in [edge.start, edge.end] {
            *out.entry(key).or_insert(false) |= spline.degree() > 1;
        }
    }
    out
}

/// How far a corner on a spline may be from the one it is: a fit's reach,
/// or a hundredth of it on a straight spline alone — two nearly parallel
/// lines on a large part meet a few hundredths apart and are two corners.
fn spline_reach(
    splined: &HashMap<VertexKey, bool>,
    key: VertexKey,
    tolerance: f64,
    reach: f64,
) -> f64 {
    match splined.get(&key) {
        Some(true) => reach,
        Some(false) => (reach * 1e-2).max(tolerance),
        None => tolerance,
    }
}

/// Moves each corner the imprint made in `target` onto the nearest corner of
/// `source` within `reach`, so the two bodies' copies of it coincide.
fn snap_cut_vertices(target: &mut Body, source: &Body, tolerance: f64, reach: f64) {
    let corners: Vec<Vec3> = source
        .vertices
        .iter()
        .map(|(_, vertex)| Vec3::from(vertex.point))
        .collect();
    let keys: Vec<VertexKey> = target.vertices.keys().collect();
    let splined = on_splines(target);
    for key in keys {
        if !splined.contains_key(&key) {
            continue;
        }
        let Some(vertex) = target.vertices.get_mut(key) else {
            continue;
        };
        if !matches!(vertex.provenance, Provenance::Synthesized) {
            continue;
        }
        let here = Vec3::from(vertex.point);
        let nearest = corners
            .iter()
            .map(|corner| (corner.distance(here), *corner))
            .min_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((gap, corner)) = nearest {
            if gap > tolerance && gap <= spline_reach(&splined, key, tolerance, reach) {
                vertex.point = corner.to_array();
            }
        }
    }
}

/// Merges each corner the imprint made into another corner of the same body
/// within `reach` (on a spline) or `tolerance`, and drops the edges left
/// running from a corner to itself.
/// Two of the source's own corners are never merged: a file may well have an
/// edge that short.
fn fuse_cut_vertices(body: &mut Body, tolerance: f64, reach: f64) {
    let keys: Vec<VertexKey> = body.vertices.keys().collect();
    let splined = on_splines(body);
    let made = |body: &Body, key: VertexKey| {
        body.vertices
            .get(key)
            .is_some_and(|vertex| matches!(vertex.provenance, Provenance::Synthesized))
    };
    // How far a made corner may be from the one it is: a spline's fit, or
    // for exact curves the modelling tolerance two cuts landing on one
    // crossing still miss each other by.
    let reach_of = |key: VertexKey| spline_reach(&splined, key, tolerance, reach);
    let mut into: std::collections::HashMap<VertexKey, VertexKey> = Default::default();
    for key in &keys {
        if !made(body, *key) || into.contains_key(key) {
            continue;
        }
        let Some(here) = body.vertices.get(*key).map(|vertex| Vec3::from(vertex.point)) else {
            continue;
        };
        // A source corner first, if one is near; otherwise another made one.
        let target = keys
            .iter()
            .filter(|other| *other != key && !into.contains_key(*other))
            .filter_map(|other| {
                let gap = Vec3::from(body.vertices.get(*other)?.point).distance(here);
                (gap <= reach_of(*key)).then_some((made(body, *other), gap, *other))
            })
            .min_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
        if let Some((_, _, other)) = target {
            into.insert(*key, other);
        }
    }
    if into.is_empty() {
        return;
    }
    // Each corner merges into one not yet merged away, so the chains have no
    // cycles; followed to the end they always land on a corner that stays.
    let resolve = |mut key: VertexKey| {
        while let Some(next) = into.get(&key) {
            key = *next;
        }
        key
    };
    let edges: Vec<super::topology::EdgeKey> = body.edge_keys().collect();
    // Only a loop this emptied goes: a cone's apex loop never had an edge.
    let mut emptied: HashSet<LoopKey> = HashSet::new();
    for key in edges {
        let Some(edge) = body.edges.get_mut(key) else {
            continue;
        };
        let was_closed = edge.start == edge.end;
        edge.start = resolve(edge.start);
        edge.end = resolve(edge.end);
        if was_closed || edge.start != edge.end {
            continue;
        }
        // An open edge now starting and ending at one corner spans nothing —
        // unless it runs off and back: the rest of a closed curve whose
        // other piece was the hair between the two corners merged.
        let (corner, curve) = (edge.start, edge.curve);
        let middle = 0.5 * (edge.start_parameter + edge.end_parameter);
        let runs_off = body.curves.get(curve).zip(body.vertices.get(corner)).is_some_and(
            |(curve, corner)| {
                Vec3::from(curve.point_at(middle)).distance(Vec3::from(corner.point))
                    > reach.max(tolerance) * 4.0
            },
        );
        if runs_off {
            continue;
        }
        let Some(edge) = body.edges.get(key) else {
            continue;
        };
        let coedges = edge.coedges.clone();
        for coedge in coedges {
            if let Some(owner) = body.coedges.get(coedge).map(|coedge| coedge.owner) {
                if let Some(ring) = body.loops.get_mut(owner) {
                    ring.coedges.retain(|kept| *kept != coedge);
                    emptied.insert(owner);
                }
            }
            body.coedges.remove(coedge);
        }
        body.edges.remove(key);
    }
    for key in into.keys() {
        body.vertices.remove(*key);
    }
    // A loop that was nothing but such edges is gone with them, and a face
    // left with no loop at all was a sliver between two of them.
    let faces: Vec<FaceKey> = body.face_keys().collect();
    for face in faces {
        let Some(node) = body.faces.get(face) else {
            continue;
        };
        let empty: Vec<LoopKey> = node
            .loops
            .iter()
            .copied()
            .filter(|ring| emptied.contains(ring))
            .filter(|ring| body.loops.get(*ring).is_none_or(|ring| ring.coedges.is_empty()))
            .collect();
        if empty.is_empty() {
            continue;
        }
        for ring in &empty {
            body.loops.remove(*ring);
        }
        let Some(node) = body.faces.get_mut(face) else {
            continue;
        };
        node.loops.retain(|ring| !empty.contains(ring));
        if node.loops.is_empty() {
            let shell = node.owner;
            body.faces.remove(face);
            if let Some(shell) = body.shells.get_mut(shell) {
                shell.faces.retain(|kept| *kept != face);
            }
        }
    }
}

pub(super) fn align_edge_vertices(source: &Body, target: &mut Body, tolerance: f64) {
    align_corners(source, target, tolerance, true);
}

/// [`align_edge_vertices`]; a corner ending a spline moves onto a point only
/// once `settled`, with no cut left to land on where the spline ends.
fn align_corners(source: &Body, target: &mut Body, tolerance: f64, settled: bool) {
    let points: Vec<[f64; 3]> = source.vertices.iter().map(|(_, vertex)| vertex.point).collect();
    // Each point is offered only to the edges whose box holds it: the
    // nearest-point search on a spline is the costly part, and every vertex
    // against every edge made it most of a boolean between filleted parts.
    let mut boxes: Vec<(super::topology::EdgeKey, Option<EdgeReach>)> =
        target.edge_keys().map(|key| (key, edge_reach(target, key, tolerance))).collect();
    for &point in &points {
        let edges: Vec<super::topology::EdgeKey> = boxes
            .iter()
            .filter(|(_, reach)| reach.as_ref().is_none_or(|reach| reach.holds(point)))
            .map(|(key, _)| *key)
            .collect();
        for key in edges {
            let Some(edge) = target.edges.get(key) else {
                continue;
            };
            if edge.coedges.is_empty() {
                continue;
            }
            let splittable = edge.coedges.iter().all(|coedge| {
                target.coedges.get(*coedge).is_some_and(|coedge| {
                    matches!(
                        coedge.pcurve.as_ref(),
                        None
                            | Some(&ocs_geom::geom2d::Curve::Line(_))
                            | Some(&ocs_geom::geom2d::Curve::Circle(_))
                            | Some(&ocs_geom::geom2d::Curve::Arc(_))
                            | Some(&ocs_geom::geom2d::Curve::Ellipse(_))
                            | Some(&ocs_geom::geom2d::Curve::Nurbs(_))
                    )
                })
            });
            if !splittable {
                continue;
            }
            let Some(curve) = target.curves.get(edge.curve) else {
                continue;
            };
            let mut parameter = curve.parameter_at(point);
            let period = match curve {
                Curve3::Circle(_) | Curve3::Ellipse(_) => Some(std::f64::consts::TAU),
                Curve3::Nurbs(spline) if spline.periodicity() => {
                    let (start, end) = spline.domain();
                    Some(end - start)
                }
                _ => None,
            };
            if let Some(period) = period {
                let middle = 0.5 * (edge.start_parameter + edge.end_parameter);
                parameter += period * ((middle - parameter) / period).round();
            }
            let span = edge.end_parameter - edge.start_parameter;
            if span == 0.0 {
                continue;
            }
            let across = (parameter - edge.start_parameter) / span;
            // A point at one of the edge's corners is that corner already: it
            // moves onto the point, where a split would leave a hairline edge
            // and a second corner beside it. Unless the other body has a
            // corner of its own there as well, and that hairline edge too;
            // or the corner ends a spline while cuts are still to come, which
            // must land where that fit ends.
            let near = |corner: super::topology::VertexKey, other: &[f64; 3]| {
                target.vertices.get(corner).is_some_and(|corner| {
                    Vec3::from(corner.point).distance(Vec3::from(*other)) <= tolerance
                })
            };
            let corner = [edge.start, edge.end].into_iter().find(|corner| {
                near(*corner, &point)
                    && !points.iter().any(|other| *other != point && near(*corner, other))
            });
            if let Some(corner) = corner {
                let spline_end = !settled
                    && target.edges.iter().any(|(_, other)| {
                        (other.start == corner || other.end == corner)
                            && matches!(target.curves.get(other.curve), Some(Curve3::Nurbs(_)))
                    });
                if !spline_end {
                    if let Some(corner) = target.vertices.get_mut(corner) {
                        corner.point = point;
                    }
                    continue;
                }
            }
            // A point a corner of this edge already answers — the point
            // that corner is nearest — needs no other, a spline's end apart.
            // Splitting beside it anyway gave the other body a corner to
            // split beside in turn: the two traded hairline edges round
            // after round, doubling their corners each time.
            let answered = corner.is_none()
                && [edge.start, edge.end].into_iter().any(|end| {
                    target.vertices.get(end).is_some_and(|vertex| {
                        let gap = Vec3::from(vertex.point).distance(Vec3::from(point));
                        gap <= tolerance
                            && points.iter().all(|other| {
                                Vec3::from(vertex.point).distance(Vec3::from(*other)) >= gap
                            })
                    })
                });
            if answered {
                continue;
            }
            if !(1e-9..=1.0 - 1e-9).contains(&across)
                || Vec3::from(curve.point_at(parameter)).distance(Vec3::from(point)) > tolerance
            {
                continue;
            }
            if let Some((_, far)) = split_edge(target, key, parameter) {
                boxes.push((far, edge_reach(target, far, tolerance)));
            }
        }
    }
}

/// A spline edge running past a corner of the other body, within the fit a
/// traced spline is good to, is divided there: a cut joined across a corner
/// of the other body — see [`chained_meetings`] — passes it as one edge,
/// where the other body's faces meet it as two, and a result stitched from
/// both needs the same corners on each side.
fn split_splines_at_corners(source: &Body, target: &mut Body, reach: f64) {
    for point in source.vertices.iter().map(|(_, vertex)| vertex.point).collect::<Vec<_>>() {
        let keys: Vec<_> = target.edge_keys().collect();
        for key in keys {
            let Some(edge) = target.edges.get(key) else {
                continue;
            };
            let Some(curve @ Curve3::Nurbs(_)) = target.curves.get(edge.curve) else {
                continue;
            };
            let at_end = [edge.start, edge.end].iter().any(|corner| {
                target.vertices.get(*corner).is_some_and(|corner| {
                    Vec3::from(corner.point).distance(Vec3::from(point)) <= reach
                })
            });
            let parameter = curve.parameter_at(point);
            let on = Vec3::from(curve.point_at(parameter)).distance(Vec3::from(point)) <= reach;
            if at_end || !on || edge.coedges.is_empty() {
                continue;
            }
            let (low, high) = (
                edge.start_parameter.min(edge.end_parameter),
                edge.start_parameter.max(edge.end_parameter),
            );
            if parameter <= low || parameter >= high {
                continue;
            }
            if let Some((near, _)) = split_edge(target, key, parameter) {
                if let Some(corner) = target
                    .edges
                    .get(near)
                    .map(|edge| edge.end)
                    .and_then(|corner| target.vertices.get_mut(corner))
                {
                    corner.point = point;
                }
            }
            break;
        }
    }
}

/// Where an edge runs, to pass over points nowhere near it before the
/// nearest-point search: a box from samples along it, grown by a tenth of
/// its size for what bulges between them, and the chain of those samples
/// with how far the edge strays from it.
struct EdgeReach {
    bounds: Aabb,
    chain: Vec<Vec3>,
    slack: f64,
}

impl EdgeReach {
    fn holds(&self, point: [f64; 3]) -> bool {
        let point = Vec3::from(point);
        self.bounds.holds(point.to_array())
            && self
                .chain
                .windows(2)
                .any(|pair| point.distance_to_segment(pair[0], pair[1]) <= self.slack)
    }
}

/// The [`EdgeReach`] of an edge. Its straying is read off halfway between
/// samples and taken four times over, with a thousandth of its size, so a
/// point a tolerance from the edge is never passed over.
fn edge_reach(body: &Body, key: super::topology::EdgeKey, tolerance: f64) -> Option<EdgeReach> {
    let edge = body.edges.get(key)?;
    let curve = body.curves.get(edge.curve)?;
    let span = edge.end_parameter - edge.start_parameter;
    let at = |step: f64| Vec3::from(curve.point_at(edge.start_parameter + span * step / 16.0));
    let chain: Vec<Vec3> = (0..=16).map(|step| at(f64::from(step))).collect();
    let bounds = Aabb::around(chain.iter().map(|point| point.to_array()))?;
    let size = Vec3::from(bounds.min).distance(Vec3::from(bounds.max));
    let bow = (0..16)
        .map(|step: usize| at(step as f64 + 0.5).distance_to_segment(chain[step], chain[step + 1]))
        .fold(0.0, f64::max);
    Some(EdgeReach {
        bounds: bounds.grown(size * 0.1 + tolerance),
        chain,
        slack: bow * 4.0 + size * 1e-3 + tolerance,
    })
}

/// Whether two curves trace the same points: one circle, or one line.
fn same_curve(one: &Curve3, other: &Curve3, tolerance: f64) -> bool {
    match (one, other) {
        (Curve3::Circle(_), Curve3::Circle(_)) => super::split::same_circle(one, other, tolerance),
        (Curve3::Line(one), Curve3::Line(other)) => {
            let direction = Vec3::from(one.direction);
            let Some(unit) = direction.normalize() else {
                return false;
            };
            let offset = Vec3::from(other.origin) - Vec3::from(one.origin);
            direction.is_parallel_to(Vec3::from(other.direction), tolerance)
                && (offset - unit * offset.dot(unit)).length() <= tolerance
        }
        _ => false,
    }
}

/// Whether two faces share any area rather than merely a plane.
///
/// Measured on their boxes, shrunk by the tolerance so a pair meeting along
/// an edge — which is what coplanar side walls of stacked solids do — reads
/// as apart rather than as an overlap with nothing in it.
///
/// A face that cannot be bounded is treated as overlapping, since "cannot
/// exclude" is the only safe answer a box test may give.
fn overlap(a: &Body, one: FaceKey, b: &Body, other: FaceKey, tolerance: f64) -> bool {
    let (Some(near), Some(far)) = (face_bounds(a, one), face_bounds(b, other)) else {
        return true;
    };
    shrunk(near, tolerance).overlaps(&shrunk(far, tolerance))
}

/// A box pulled in on every axis that has room to spare.
///
/// A face's box is flat in one direction by definition, and pulling that one
/// in turns it inside out — every coplanar pair then reads as apart, and a
/// wall that genuinely overlaps is passed over without a word.
fn shrunk(bounds: Aabb, by: f64) -> Aabb {
    let mut out = bounds;
    for axis in 0..3 {
        if bounds.max[axis] - bounds.min[axis] > 2.0 * by {
            out.min[axis] += by;
            out.max[axis] -= by;
        }
    }
    out
}

/// Whether two faces on the same surface cover the same region of it.
///
/// Compared by their corners rather than by area: two rings enclosing the
/// same area can be different shapes, and what matters here is that neither
/// face has any part the other does not.
pub fn same_ground(
    a: &Body,
    one: FaceKey,
    b: &Body,
    other: FaceKey,
    tolerance: f64,
) -> bool {
    let corners = |body: &Body, face: FaceKey| -> Vec<Vec3> {
        body.face_coedges(face)
            .iter()
            .filter_map(|coedge| body.coedge_vertices(*coedge))
            .filter_map(|(from, _)| Some(Vec3::from(body.vertices.get(from)?.point)))
            .collect()
    };
    let near = corners(a, one);
    let far = corners(b, other);
    if near.is_empty() || near.len() != far.len() {
        return false;
    }
    near.iter().all(|point| {
        far.iter()
            .any(|other| point.distance(*other) <= tolerance)
    }) && far.iter().all(|point| {
        near.iter()
            .any(|other| point.distance(*other) <= tolerance)
    })
}

/// Curves shared by a pair of faces.
struct Shared {
    curves: Vec<Curve3>,
    /// The boxes of the two faces the curves were found between, `[a, b]`;
    /// `None` where a box could not be had.
    boxes: [Option<Aabb>; 2],
    /// Loops of one body's face to copy whole into the other body's face:
    /// `(into a, the face there, the loop's own face, the loop)`.
    islands: Vec<(bool, FaceKey, FaceKey, LoopKey)>,
}

/// The curves one loop of a face's boundary runs along.
///
/// What cuts a partly shared wall into its shared and unshared parts. They
/// are used as whole curves rather than as the segments the edges cover, so
/// a boundary line may cut somewhere the edge itself does not reach — which
/// leaves an extra edge and never a different shape, since an imprint only
/// ever adds them.
///
/// Splines are the exception. A file keeps one intersection loop as several
/// edges, each with its own spline for the whole loop, fitted a little
/// differently; cutting with every one of them peels slivers off between
/// the copies. A run of spline edges is cut with once, as one spline through
/// the stretch the edges actually cover.
fn loop_curves(
    body: &Body,
    ring: LoopKey,
    reaching: &std::collections::HashSet<CoedgeKey>,
) -> Vec<Curve3> {
    const STEPS: usize = 64;
    let spline_edge = |coedge: &CoedgeKey| {
        let edge = body.coedges.get(*coedge).and_then(|coedge| body.edges.get(coedge.edge));
        edge.and_then(|edge| body.curves.get(edge.curve))
            .is_some_and(|curve| matches!(curve, Curve3::Nurbs(_)))
    };
    let mut out = Vec::new();
    let Some(ring) = body.loops.get(ring) else {
        return out;
    };
    // Start the walk past a spline run, so no run is split in two by
    // where the loop happens to begin.
    let start = ring.coedges.iter().position(|coedge| !spline_edge(coedge)).unwrap_or(0);
    let walk = ring.coedges[start..].iter().chain(&ring.coedges[..start]);
    // A loop of splines only comes back round to where it began.
    let closed = ring.coedges.iter().all(spline_edge);
    let mut run: Vec<[f64; 3]> = Vec::new();
    let flush = |run: &mut Vec<[f64; 3]>, out: &mut Vec<Curve3>| {
        // Evenly spaced: a cubic through points bunched on a tiny edge
        // overshoots between them by far more than any fit.
        let length: f64 = run
            .windows(2)
            .map(|pair| Vec3::from(pair[0]).distance(Vec3::from(pair[1])))
            .sum();
        let spacing = length / 256.0;
        let mut even: Vec<[f64; 3]> = Vec::with_capacity(run.len());
        for (index, point) in run.iter().enumerate() {
            let last = index + 1 == run.len();
            match even.last() {
                Some(kept)
                    if !last && Vec3::from(*kept).distance(Vec3::from(*point)) < spacing => {}
                Some(kept)
                    if last
                        && even.len() > 1
                        && Vec3::from(*kept).distance(Vec3::from(*point)) < spacing =>
                {
                    *even.last_mut().expect("checked") = *point;
                }
                _ => even.push(*point),
            }
        }
        *run = even;
        if run.len() >= 2 {
            if let Some(spline) =
                NurbsCurve3::interpolate_fit(run, None, None, Parameterization::Chord)
            {
                out.push(Curve3::Nurbs(spline.with_periodicity(closed)));
            }
        }
        run.clear();
    };
    for key in walk {
        let Some(coedge) = body.coedges.get(*key) else {
            continue;
        };
        let Some(edge) = body.edges.get(coedge.edge) else {
            continue;
        };
        let Some(curve) = body.curves.get(edge.curve) else {
            continue;
        };
        // An edge running only along the other face's boundary, or off it,
        // has nothing there to cut.
        if !reaching.contains(key) {
            flush(&mut run, &mut out);
            continue;
        }
        if !matches!(curve, Curve3::Nurbs(_)) {
            flush(&mut run, &mut out);
            out.push(curve.clone());
            continue;
        }
        let (from, to) = if coedge.forward {
            (edge.start_parameter, edge.end_parameter)
        } else {
            (edge.end_parameter, edge.start_parameter)
        };
        let skip = usize::from(!run.is_empty());
        run.extend((skip..=STEPS).map(|step| {
            curve.point_at(from + (to - from) * step as f64 / STEPS as f64)
        }));
    }
    flush(&mut run, &mut out);
    out
}

/// [`loop_walk`] with the coedge each point lies on.
fn loop_walk_of(
    body: &Body,
    coedges: &[CoedgeKey],
    samples: usize,
) -> Option<Vec<(CoedgeKey, Vec3, Vec3)>> {
    let mut out = Vec::new();
    for key in coedges {
        let coedge = body.coedges.get(*key)?;
        let edge = body.edges.get(coedge.edge)?;
        let curve = body.curves.get(edge.curve)?;
        let sense = if coedge.forward { 1.0 } else { -1.0 };
        for step in 0..samples {
            let t = (step as f64 + 0.5) / samples as f64;
            let parameter = if coedge.forward {
                edge.start_parameter + (edge.end_parameter - edge.start_parameter) * t
            } else {
                edge.end_parameter + (edge.start_parameter - edge.end_parameter) * t
            };
            let along = Vec3::from(curve.tangent_at(parameter)) * sense;
            out.push((*key, Vec3::from(curve.point_at(parameter)), along));
        }
    }
    Some(out)
}

/// Points along a loop with the way the loop runs at each, `samples` to an
/// edge.
fn loop_walk(body: &Body, ring: LoopKey, samples: usize) -> Option<Vec<(Vec3, Vec3)>> {
    let walk = loop_walk_of(body, &body.loops.get(ring)?.coedges, samples)?;
    Some(walk.into_iter().map(|(_, point, along)| (point, along)).collect())
}

/// Whether a loop of one face lies wholly inside another face on the same
/// surface, clear of its boundary, and closes there rather than going round
/// the surface — a slot's opening, not a rim.
fn island_fits(from: &Body, ring: LoopKey, into: &Body, face: FaceKey, tolerance: f64) -> bool {
    let Some(walk) = loop_walk(from, ring, 8) else {
        return false;
    };
    let Some(surface) = into.faces.get(face).and_then(|node| into.surfaces.get(node.surface))
    else {
        return false;
    };
    // A plane cuts cleanly along whole lines and circles; only on a curved
    // wall do they run on round it, far past the loop.
    if matches!(surface, Surface::Plane(_)) {
        return false;
    }
    let periods = super::pcurve::periods(surface);
    let mut previous: Option<[f64; 2]> = None;
    let mut first: Option<[f64; 2]> = None;
    for (point, _) in &walk {
        let Some((mut u, mut v)) = surface.parameters_at(point.to_array()) else {
            return false;
        };
        if let Some(last) = previous {
            if let Some(period) = periods[0] {
                u -= period * ((u - last[0]) / period).round();
            }
            if let Some(period) = periods[1] {
                v -= period * ((v - last[1]) / period).round();
            }
        }
        first.get_or_insert([u, v]);
        previous = Some([u, v]);
    }
    let (Some(first), Some(last)) = (first, previous) else {
        return false;
    };
    let wraps = periods
        .iter()
        .zip([first[0] - last[0], first[1] - last[1]])
        .any(|(period, gap)| period.is_some_and(|period| gap.abs() > period * 0.5));
    if wraps {
        return false;
    }
    // Clear of the face's own edges, by more than a hair.
    let edges: Vec<Vec<Vec3>> = into
        .face_coedges(face)
        .iter()
        .filter_map(|coedge| {
            let edge = into.edges.get(into.coedges.get(*coedge)?.edge)?;
            let curve = into.curves.get(edge.curve)?;
            Some(
                (0..=64)
                    .map(|step| {
                        Vec3::from(curve.point_at(
                            edge.start_parameter
                                + (edge.end_parameter - edge.start_parameter) * step as f64
                                    / 64.0,
                        ))
                    })
                    .collect(),
            )
        })
        .collect();
    let clear = tolerance * 1e3;
    let fit = loop_fit(&walk, tolerance);
    walk.iter().all(|(point, _)| {
        super::classify::face_distance(into, face, point.to_array(), fit)
            .is_some_and(|gap| gap <= fit)
            && edges.iter().all(|edge| {
                edge.windows(2)
                    .all(|pair| point.distance_to_segment(pair[0], pair[1]) > clear)
            })
    })
}

/// Whether any of a loop comes onto another face — the only place its
/// curves have anything to cut there.
fn loop_reaches(from: &Body, ring: LoopKey, into: &Body, face: FaceKey, tolerance: f64) -> bool {
    let Some(walk) = loop_walk(from, ring, 8) else {
        return true;
    };
    let fit = loop_fit(&walk, tolerance);
    // Touching the face's own edges is not reaching into it: gear teeth in
    // mesh meet along a line on a shared plane and overlap nowhere.
    let edges = edge_samples(into, face);
    walk.iter().any(|(point, _)| {
        super::classify::face_distance(into, face, point.to_array(), fit)
            .is_none_or(|gap| gap <= fit)
            && edges.iter().all(|edge| {
                edge.windows(2)
                    .all(|pair| point.distance_to_segment(pair[0], pair[1]) > fit * 10.0)
            })
    })
}

/// The coedges of a loop that run strictly inside another face somewhere.
fn edges_reaching(
    from: &Body,
    ring: LoopKey,
    into: &Body,
    face: FaceKey,
    tolerance: f64,
) -> std::collections::HashSet<CoedgeKey> {
    let mut out = std::collections::HashSet::new();
    let Some(ring) = from.loops.get(ring) else {
        return out;
    };
    let Some(walk) = loop_walk_of(from, &ring.coedges, 8) else {
        return out;
    };
    let points: Vec<(Vec3, Vec3)> = walk.iter().map(|(_, point, along)| (*point, *along)).collect();
    let fit = loop_fit(&points, tolerance);
    let edges = edge_samples(into, face);
    for (coedge, point, _) in walk {
        if out.contains(&coedge) {
            continue;
        }
        let on_face = super::classify::face_distance(into, face, point.to_array(), fit)
            .is_none_or(|gap| gap <= fit);
        let clear = edges.iter().all(|edge| {
            edge.windows(2)
                .all(|pair| point.distance_to_segment(pair[0], pair[1]) > fit * 10.0)
        });
        if on_face && clear {
            out.insert(coedge);
        }
    }
    out
}

/// Points along each edge of a face, for distances to its boundary.
fn edge_samples(body: &Body, face: FaceKey) -> Vec<Vec<Vec3>> {
    body.face_coedges(face)
        .iter()
        .filter_map(|coedge| {
            let edge = body.edges.get(body.coedges.get(*coedge)?.edge)?;
            let curve = body.curves.get(edge.curve)?;
            Some(
                (0..=16)
                    .map(|step| {
                        Vec3::from(curve.point_at(
                            edge.start_parameter
                                + (edge.end_parameter - edge.start_parameter) * step as f64
                                    / 16.0,
                        ))
                    })
                    .collect(),
            )
        })
        .collect()
}

/// How far a file's loop may stand off the surface it bounds: its splines
/// are fits, good to a small part of the loop's size, not to the modelling
/// tolerance.
fn loop_fit(walk: &[(Vec3, Vec3)], tolerance: f64) -> f64 {
    let extent = walk
        .iter()
        .map(|(point, _)| point.distance(walk[0].0))
        .fold(0.0, f64::max);
    tolerance.max(extent * 1e-4)
}

/// Copies a loop of `from` into `face` of `body` as an island: the loop
/// becomes a hole in the face and the boundary of a new face inside it, on
/// the same surface, wound so each keeps its own side.
fn copy_island(
    body: &mut Body,
    face: FaceKey,
    from: &Body,
    ring: LoopKey,
) -> Option<FaceKey> {
    let node = body.faces.get(face)?.clone();
    let surface = body.surfaces.get(node.surface)?.clone();
    let walk = loop_walk(from, ring, 4)?;
    // Which way round the loop runs about the face's own normal: inward on
    // the left of its way is an outer boundary's sense.
    let count = walk.len() as f64;
    let centre = walk.iter().fold(Vec3::new(0.0, 0.0, 0.0), |sum, (point, _)| sum + *point)
        * (1.0 / count);
    let sense = if node.forward { 1.0 } else { -1.0 };
    let turning: f64 = walk
        .iter()
        .filter_map(|(point, along)| {
            let (u, v) = surface.parameters_at(point.to_array())?;
            let normal = Vec3::from(surface.normal_at(u, v)?) * sense;
            Some(normal.cross(*along).dot(centre - *point))
        })
        .sum();
    let outer = turning > 0.0;

    let source = from.loops.get(ring)?.coedges.clone();
    let mut vertices: std::collections::HashMap<VertexKey, VertexKey> = Default::default();
    let mut vertex = |body: &mut Body, key: VertexKey| -> Option<VertexKey> {
        if let Some(made) = vertices.get(&key) {
            return Some(*made);
        }
        let made = body.vertices.insert(Vertex {
            point: from.vertices.get(key)?.point,
            provenance: Provenance::Synthesized,
        });
        vertices.insert(key, made);
        Some(made)
    };
    let island_ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let hole_ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let island = body.faces.insert(Face {
        surface: node.surface,
        forward: node.forward,
        loops: vec![island_ring],
        owner: node.owner,
        provenance: Provenance::Synthesized,
    });
    body.loops.get_mut(island_ring)?.owner = island;
    let (mut island_coedges, mut hole_coedges) = (Vec::new(), Vec::new());
    for key in &source {
        let coedge = from.coedges.get(*key)?;
        let edge = from.edges.get(coedge.edge)?;
        let curve = body.curves.insert(from.curves.get(edge.curve)?.clone());
        let (start, end) = (vertex(body, edge.start)?, vertex(body, edge.end)?);
        let made = body.edges.insert(Edge {
            curve,
            start_parameter: edge.start_parameter,
            end_parameter: edge.end_parameter,
            start,
            end,
            coedges: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        // The island runs the loop the way that bounds it; the hole the
        // other way.
        let along = coedge.forward == outer;
        let island_use = body.coedges.insert(Coedge {
            edge: made,
            forward: along,
            pcurve: None,
            owner: island_ring,
            provenance: Provenance::Synthesized,
        });
        let hole_use = body.coedges.insert(Coedge {
            edge: made,
            forward: !along,
            pcurve: None,
            owner: hole_ring,
            provenance: Provenance::Synthesized,
        });
        body.edges.get_mut(made)?.coedges = vec![island_use, hole_use];
        island_coedges.push(island_use);
        hole_coedges.push(hole_use);
    }
    if !outer {
        island_coedges.reverse();
    } else {
        hole_coedges.reverse();
    }
    body.loops.get_mut(island_ring)?.coedges = island_coedges;
    body.loops.get_mut(hole_ring)?.coedges = hole_coedges;
    body.shells.get_mut(node.owner)?.faces.push(island);
    // The face's own holes inside the island are the island's holes now.
    let inner: Vec<LoopKey> = node
        .loops
        .iter()
        .skip(1)
        .copied()
        .filter(|ring| {
            loop_walk(body, *ring, 2).is_some_and(|walk| {
                let fit = loop_fit(&walk, 1e-9);
                walk.first().is_some_and(|(point, _)| {
                    super::classify::face_distance(body, island, point.to_array(), fit)
                        .is_some_and(|gap| gap <= fit)
                })
            })
        })
        .collect();
    for ring in &inner {
        body.loops.get_mut(*ring)?.owner = island;
    }
    body.faces.get_mut(island)?.loops.extend(inner.iter().copied());
    let kept = body.faces.get_mut(face)?;
    kept.loops.retain(|ring| !inner.contains(ring));
    kept.loops.push(hole_ring);
    kept.provenance.soil();
    Some(island)
}

/// Each closed spline that crosses itself once, as its two loops from the
/// crossing back to it: Viviani's curve, both loops through the point where
/// the cylinder touches the sphere. Cut whole, its two passes through that
/// point land on one seam corner at once; as two loops, each cuts the face
/// it crosses from the corner round and back like any other.
fn crossing_loops_apart(
    curves: Vec<Curve3>,
    found: Vec<[Option<Aabb>; 2]>,
    tolerance: f64,
) -> (Vec<Curve3>, Vec<[Option<Aabb>; 2]>) {
    curves
        .into_iter()
        .zip(found)
        .flat_map(|(curve, boxes)| {
            let loops = match &curve {
                Curve3::Nurbs(spline) if spline.periodicity() => {
                    crossing_loops(spline, tolerance).unwrap_or_default()
                }
                _ => Vec::new(),
            };
            match loops.is_empty() {
                true => vec![(curve, boxes)],
                false => loops.into_iter().map(|piece| (Curve3::Nurbs(piece), boxes)).collect(),
            }
        })
        .unzip()
}

/// A closed spline that crosses itself once, as its two loops from the
/// crossing back to it. `None` where it does not cross itself, or crosses
/// more than once.
fn crossing_loops(spline: &NurbsCurve3, tolerance: f64) -> Option<Vec<NurbsCurve3>> {
    const SAMPLES: usize = 256;
    let (start, end) = spline.domain();
    let at = |step: f64| start + (end - start) * step / SAMPLES as f64;
    let points: Vec<Vec3> =
        (0..SAMPLES).map(|step| Vec3::from(spline.point_at_knot(at(step as f64)))).collect();
    let steps: Vec<f64> =
        (0..SAMPLES).map(|index| points[index].distance(points[(index + 1) % SAMPLES])).collect();
    let spacing = steps.iter().copied().fold(0.0, f64::max);
    let fit = tolerance.max(steps.iter().sum::<f64>() * 1e-5);
    let gap = |s: f64, t: f64| {
        Vec3::from(spline.point_at_knot(s)).distance(Vec3::from(spline.point_at_knot(t)))
    };
    // Samples a few steps apart along the curve that come within a step of
    // each other, each pair settled to the parameters nearest each other.
    let mut crossings: Vec<(f64, f64)> = Vec::new();
    for i in 0..SAMPLES {
        for j in i + 8..SAMPLES {
            if SAMPLES - (j - i) < 8 || points[i].distance(points[j]) > spacing {
                continue;
            }
            let (mut s, mut t) = (at(i as f64), at(j as f64));
            let mut step = (end - start) / SAMPLES as f64;
            for _ in 0..60 {
                let moves = [(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step)];
                let best = moves
                    .iter()
                    .map(|(ds, dt)| (s + ds, t + dt))
                    .filter(|(s, t)| *s >= start && *t <= end && s < t)
                    .min_by(|x, y| gap(x.0, x.1).total_cmp(&gap(y.0, y.1)));
                match best {
                    Some(next) if gap(next.0, next.1) < gap(s, t) => (s, t) = next,
                    _ => step *= 0.5,
                }
            }
            let near = (end - start) * 2.0 / SAMPLES as f64;
            if gap(s, t) <= fit
                && !crossings.iter().any(|(a, b)| (a - s).abs() < near && (b - t).abs() < near)
            {
                crossings.push((s, t));
            }
        }
    }
    let [(first, second)] = crossings[..] else {
        return None;
    };
    let open = spline.clone().with_periodicity(false);
    // A curve traced from the crossing itself has it at an end already.
    let near = (end - start) * 2.0 / SAMPLES as f64;
    // Each loop is closed, round from the crossing and back, with a corner
    // there: a face it runs through crosses it like any closed cut, and a
    // face whose seam it starts on takes it as one cut round.
    let loops = if first - start < near || end - second < near {
        let at = if first - start < near { second } else { first };
        let (one, other) = open.split_at(at)?;
        vec![one, other]
    } else {
        let (head, rest) = open.split_at(first)?;
        let (middle, tail) = rest.split_at(second)?;
        vec![middle, tail.joined(&head)?]
    };
    Some(loops.into_iter().map(|piece| piece.with_periodicity(true)).collect())
}

/// Whether a circle or an ellipse runs over both faces at once: the
/// stretches of it inside each face, cut where its image in that face's
/// parameters (or a copy a turn away) crosses the face's boundary, sharing
/// some of it or touching. `None` for any other curve, or where an image or
/// a boundary cannot be had.
fn conic_meets_both(
    a: &Body,
    one: FaceKey,
    b: &Body,
    other: FaceKey,
    curve: &Curve3,
    tolerance: f64,
) -> Option<bool> {
    use std::f64::consts::TAU;
    let radius = match curve {
        Curve3::Circle(circle) => circle.radius,
        Curve3::Ellipse(ellipse) => ellipse.minor_radius.min(ellipse.major_radius),
        _ => return None,
    };
    let fit = ocs_geom::geom2d::Tolerance::new(tolerance);
    let spans = |body: &Body, face: FaceKey| -> Option<Vec<(f64, f64)>> {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        let flat = super::pcurve::project(surface, curve, tolerance)?;
        let boundary = super::pcurve::face_boundary(body, face, tolerance)?;
        let inside = |at: f64| {
            let point = curve.point_at(at);
            surface.distance_to(point).abs() <= tolerance
                && surface.parameters_at(point).is_some_and(|(u, v)| {
                    super::pcurve::contains_parameter_facing(
                        surface,
                        &boundary,
                        [u, v],
                        fit,
                        node.forward,
                    )
                })
        };
        let mut cuts: Vec<f64> = Vec::new();
        for image in super::split::periodic_images(&flat, super::pcurve::periods(surface)) {
            for edge in &boundary {
                for crossing in ocs_geom::geom2d::intersect(&image, edge, fit) {
                    let point = surface.point_at(crossing.point[0], crossing.point[1]);
                    cuts.push(curve.parameter_at(point).rem_euclid(TAU));
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|next, kept| (*next - *kept).abs() <= 1e-9);
        if cuts.is_empty() {
            return Some(if inside(0.0) { vec![(0.0, TAU)] } else { Vec::new() });
        }
        Some(
            (0..cuts.len())
                .map(|index| {
                    let from = cuts[index];
                    let to = cuts.get(index + 1).copied().unwrap_or(cuts[0] + TAU);
                    (from, to)
                })
                .filter(|(from, to)| inside(0.5 * (from + to)))
                .collect(),
        )
    };
    let (first, second) = (spans(a, one)?, spans(b, other)?);
    // Stretches within an angle the tolerance covers of each other touch.
    let slack = tolerance / radius.max(f64::MIN_POSITIVE);
    Some(first.iter().any(|(from, to)| {
        second.iter().any(|(start, end)| {
            [-TAU, 0.0, TAU].into_iter().any(|turn| {
                from - slack <= end + turn && start + turn <= to + slack
            })
        })
    }))
}

/// Whether a few points of one of `curves` lie on both faces — proof that
/// the faces meet, found without meshing either to prove they do not. Silent
/// on a spline, and on a meeting short enough to fall between the points:
/// [`super::near::apart`] still settles those.
fn seen_meeting(
    a: &Body,
    one: FaceKey,
    b: &Body,
    other: FaceKey,
    curves: &[Curve3],
    boxes: [Option<Aabb>; 2],
    tolerance: f64,
) -> bool {
    // As `classify::face_distance` asks it, each face's boundary built once
    // for all the points rather than once a point.
    let fit = ocs_geom::geom2d::Tolerance::new(tolerance);
    let boundaries = [(a, one), (b, other)].map(|(body, face)| {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        Some((surface, node.forward, super::pcurve::face_boundary(body, face, tolerance)?))
    });
    let on = |side: usize, point: [f64; 3]| {
        let Some((surface, forward, boundary)) = &boundaries[side] else {
            return false;
        };
        surface.distance_to(point).abs() <= tolerance
            && surface.parameters_at(point).is_some_and(|(u, v)| {
                super::pcurve::contains_parameter_facing(surface, boundary, [u, v], fit, *forward)
            })
    };
    curves.iter().any(|curve| {
        let points: Vec<[f64; 3]> = match curve {
            Curve3::Circle(_) | Curve3::Ellipse(_) => (0..8)
                .map(|step| curve.point_at(step as f64 * std::f64::consts::TAU / 8.0))
                .collect(),
            Curve3::Line(line) => match boxes {
                [Some(near), Some(far)] => match clipped_line(line, &near, &far) {
                    Some(Curve3::Nurbs(piece)) => {
                        let (start, end) = piece.domain();
                        (1..8)
                            .map(|step| {
                                piece.point_at_knot(start + (end - start) * step as f64 / 8.0)
                            })
                            .collect()
                    }
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        points.into_iter().any(|point| on(1, point) && on(0, point))
    })
}

/// The stretch of a line inside both boxes, widened a little so it still
/// crosses the faces' edges, as a straight spline. `None` when it misses.
fn clipped_line(line: &super::geometry::Line3, near: &Aabb, far: &Aabb) -> Option<Curve3> {
    let low: [f64; 3] = std::array::from_fn(|axis| near.min[axis].max(far.min[axis]));
    let high: [f64; 3] = std::array::from_fn(|axis| near.max[axis].min(far.max[axis]));
    let margin = Vec3::from(low).distance(Vec3::from(high)) * 0.01 + 1e-6;
    let (mut from, mut to) = (f64::NEG_INFINITY, f64::INFINITY);
    for axis in 0..3 {
        let (start, along) = (line.origin[axis], line.direction[axis]);
        let (low, high) = (low[axis] - margin, high[axis] + margin);
        if along.abs() <= f64::EPSILON {
            if start < low || start > high {
                return None;
            }
            continue;
        }
        let (a, b) = ((low - start) / along, (high - start) / along);
        from = from.max(a.min(b));
        to = to.min(a.max(b));
    }
    if !(from < to) {
        return None;
    }
    let at = |t: f64| (Vec3::from(line.origin) + Vec3::from(line.direction) * t).to_array();
    NurbsCurve3::new_strict(1, vec![at(from), at(to)], vec![from, from, to, to], vec![1.0, 1.0])
        .map(Curve3::Nurbs)
}

/// A face's box in its surface's parameters, from its boundary.
fn face_parameter_box(body: &Body, face: FaceKey, tolerance: f64) -> Option<[[f64; 2]; 2]> {
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for curve in super::pcurve::face_boundary(body, face, tolerance)? {
        for step in 0..=32 {
            let point = curve.point_at(step as f64 / 32.0);
            for axis in 0..2 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
    }
    (low.iter().chain(&high).all(|value| value.is_finite()) && low[0] < high[0] && low[1] < high[1])
        .then_some([low, high])
}

/// Whether two faces are proved apart on their surfaces' Bézier pieces over
/// the faces' parameter boxes ([`super::ssi::apart`]): no mesh of either is
/// made for a pair that never comes near. The boxes, read off sampled
/// boundary images, are grown a little so a bulge between samples stays in.
fn proved_apart(
    a: &Body,
    one: FaceKey,
    one_surface: &Surface,
    b: &Body,
    other: FaceKey,
    other_surface: &Surface,
    tolerance: f64,
) -> bool {
    let grown = |[low, high]: [[f64; 2]; 2]| {
        let pad = (high[0] - low[0]).hypot(high[1] - low[1]) * 0.02;
        ([low[0] - pad, high[0] + pad], [low[1] - pad, high[1] + pad])
    };
    let (Some((u, v)), Some((s, t))) = (
        face_parameter_box(a, one, tolerance).map(grown),
        face_parameter_box(b, other, tolerance).map(grown),
    ) else {
        return false;
    };
    let first = super::ssi::Bounded { surface: one_surface, u, v };
    let second = super::ssi::Bounded { surface: other_surface, u: s, v: t };
    super::ssi::apart(&first, &second, tolerance * 10.0)
}

/// The curves two faces share, from the full surface/surface intersection of
/// their bounded pieces, fitted as the march's are. `None` when either face
/// has no box or the intersection is refused.
fn bounded_meeting(
    a: &Body,
    one: FaceKey,
    one_surface: &Surface,
    b: &Body,
    other: FaceKey,
    other_surface: &Surface,
    tolerance: f64,
) -> Option<Vec<Curve3>> {
    let ([u0, v0], [u1, v1]) = face_parameter_box(a, one, tolerance).map(|[l, h]| (l, h))?;
    let ([s0, t0], [s1, t1]) = face_parameter_box(b, other, tolerance).map(|[l, h]| (l, h))?;
    let first = super::ssi::Bounded { surface: one_surface, u: [u0, u1], v: [v0, v1] };
    let second = super::ssi::Bounded { surface: other_surface, u: [s0, s1], v: [t0, t1] };
    let found = super::ssi::meet(&first, &second, tolerance.max(1e-9)).ok()?;
    found
        .branches
        .iter()
        .map(|branch| {
            let points: Vec<[f64; 3]> = branch.points.iter().map(|shared| shared.point).collect();
            if branch.closed {
                NurbsCurve3::interpolate_periodic(&points, ocs_geom::space::Parameterization::Chord)
            } else {
                NurbsCurve3::interpolate_fit(&points, None, None, ocs_geom::space::Parameterization::Chord)
            }
            .map(Curve3::Nurbs)
        })
        .collect()
}

/// Whether a closed conic two surfaces cross along bounds a cap lying
/// within `tolerance` of both — a ball sunk a hair into the face it rests
/// on, two balls a hair into each other. Near a contact the surfaces' gap
/// is to second order a quadratic whose zero set is the curve; the gap at
/// the middle of the cap is at most the largest `θ / (2κ)` along it, where
/// the normals part by `θ` and the curve bends by `κ`. Within the tolerance
/// everywhere, and the cap's centre (the mean centre of curvature) within it
/// of both surfaces, the two only touch there.
///
/// Ported from Solidcore's `cap_centre_within_tolerance`
/// (`topology/face/p3.rs`), Apache-2.0, see THIRD_PARTY.md.
fn tangent_cap(one: &Surface, other: &Surface, curve: &Curve3, tolerance: f64) -> bool {
    const SAMPLES: usize = 8;
    if !matches!(curve, Curve3::Circle(_) | Curve3::Ellipse(_)) {
        return false;
    }
    let normal = |surface: &Surface, point: [f64; 3]| {
        surface
            .parameters_at(point)
            .and_then(|(u, v)| surface.normal_at(u, v))
            .and_then(|normal| Vec3::from(normal).normalize())
    };
    let mut centre = Vec3::new(0.0, 0.0, 0.0);
    for sample in 0..SAMPLES {
        let t = std::f64::consts::TAU * (sample as f64 + 0.5) / SAMPLES as f64;
        // The parameter is an angle on both conics; differences a
        // thousandth of a radian apart read the bend to a millionth.
        let step = 1e-3;
        let (before, at, after) = (
            Vec3::from(curve.point_at(t - step)),
            Vec3::from(curve.point_at(t)),
            Vec3::from(curve.point_at(t + step)),
        );
        let first = (after - before) * (0.5 / step);
        let second = (after + before - at * 2.0) * (1.0 / (step * step));
        let speed = first.length_squared();
        if speed <= f64::EPSILON {
            return false;
        }
        let bend = (second - first * (second.dot(first) / speed)) * (1.0 / speed);
        let kappa = bend.length();
        if kappa <= f64::EPSILON {
            return false;
        }
        let (Some(a), Some(b)) = (normal(one, at.to_array()), normal(other, at.to_array())) else {
            return false;
        };
        if a.cross(b).length() > 2.0 * kappa * tolerance {
            return false;
        }
        centre = centre + at + bend * (1.0 / (kappa * kappa));
    }
    let centre = (centre * (1.0 / SAMPLES as f64)).to_array();
    one.distance_to(centre).abs() <= tolerance && other.distance_to(centre).abs() <= tolerance
}

/// Every curve the two bodies' faces share.
fn shared_curves(a: &Body, b: &Body, tolerance: f64, reach: bool) -> Result<Vec<Shared>, Snag> {
    let near: Vec<(FaceKey, Option<Aabb>)> =
        a.face_keys().map(|key| (key, face_bounds(a, key))).collect();
    let far: Vec<(FaceKey, Option<Aabb>)> =
        b.face_keys().map(|key| (key, face_bounds(b, key))).collect();

    let mut out = Vec::new();
    let mut soups = None;
    let mut marched: Vec<(Surface, Surface, Vec<Vec<Vec3>>)> = Vec::new();
    let mut untraced: Vec<usize> = Vec::new();
    // Where a traced curve may run: inside both bodies' boxes, a little over
    // — and over either body, for one that comes back round to close.
    let boxes = match (super::bounds::body_bounds(a), super::bounds::body_bounds(b)) {
        (Some(first), Some(second)) => [true, false].map(|both| {
            let low = Vec3::from(std::array::from_fn(|i| match both {
                true => first.min[i].max(second.min[i]),
                false => first.min[i].min(second.min[i]),
            }));
            let high = Vec3::from(std::array::from_fn(|i| match both {
                true => first.max[i].min(second.max[i]),
                false => first.max[i].max(second.max[i]),
            }));
            let margin = tolerance + low.distance(high) * 1e-3;
            (low - Vec3::new(margin, margin, margin), high + Vec3::new(margin, margin, margin))
        }),
        _ => [(Vec3::new(f64::MIN, f64::MIN, f64::MIN), Vec3::new(f64::MAX, f64::MAX, f64::MAX));
            2],
    };
    for (one, one_box) in &near {
        for (other, other_box) in &far {
            // A box that could not be computed means "cannot exclude", so
            // the pair is tested rather than skipped.
            if let (Some(first), Some(second)) = (one_box, other_box) {
                if !first.grown(tolerance).overlaps(second) {
                    continue;
                }
            }
            let (Some(one_face), Some(other_face)) = (a.faces.get(*one), b.faces.get(*other))
            else {
                continue;
            };
            let (Some(one_surface), Some(other_surface)) = (
                a.surfaces.get(one_face.surface),
                b.surfaces.get(other_face.surface),
            ) else {
                continue;
            };
            match surfaces(one_surface, other_surface, tolerance) {
                Meeting::None | Meeting::Points(_) => {}
                // A line two curved faces share runs on for ever, and cut as
                // one it divides faces of either body far from where the
                // two meet — a slot's side, run down the whole of the hole
                // and the shaft in it. Only the stretch where both faces
                // are is kept.
                Meeting::Curves(curves) => {
                    // A closed curve round a cap that stays within the
                    // tolerance of both surfaces is a touch there, not a
                    // crossing: cut along, it would part faces no side test
                    // can tell apart.
                    let curves: Vec<Curve3> = curves
                        .into_iter()
                        .filter(|curve| {
                            !tangent_cap(one_surface, other_surface, curve, tolerance)
                        })
                        .collect();
                    if curves.is_empty() {
                        continue;
                    }
                    // Surfaces that meet, faces that may not: boxes overlap
                    // round many pairs whose faces stay well apart, and the
                    // whole curves of those would cut both bodies for
                    // nothing.
                    let pair = [*one_box, *other_box];
                    let seen = seen_meeting(a, *one, b, *other, &curves, pair, tolerance);
                    // A circle or an ellipse settles it exactly: where it
                    // runs inside each face, from its crossings with the
                    // faces' boundaries, and whether those stretches share
                    // any of it.
                    let settled: Option<Vec<bool>> = (!seen)
                        .then(|| {
                            curves
                                .iter()
                                .map(|curve| conic_meets_both(a, *one, b, *other, curve, tolerance))
                                .collect()
                        })
                        .flatten();
                    if settled.as_ref().is_some_and(|meets| meets.iter().all(|meet| !meet)) {
                        continue;
                    }
                    let seen = seen || settled.is_some();
                    if !seen
                        && proved_apart(a, *one, one_surface, b, *other, other_surface, tolerance)
                    {
                        continue;
                    }
                    if !seen {
                        let soups = soups.get_or_insert_with(|| {
                            (
                                super::near::FaceSoups::of(a, tolerance),
                                super::near::FaceSoups::of(b, tolerance),
                            )
                        });
                        if super::near::apart(&soups.0, *one, &soups.1, *other, tolerance) {
                            continue;
                        }
                    }
                    let curved = !matches!(one_surface, Surface::Plane(_))
                        || !matches!(other_surface, Surface::Plane(_));
                    // A line on a curved face is kept to where the two faces'
                    // boxes overlap; reaching, to each body's own face — the
                    // other body's piece of it cutting nothing there.
                    let mut kept = Vec::new();
                    for curve in curves {
                        match (&curve, one_box, other_box, curved, reach) {
                            (Curve3::Line(line), Some(near), Some(far), true, false) => {
                                kept.extend(clipped_line(line, near, far));
                            }
                            (Curve3::Line(line), Some(near), Some(far), true, true) => {
                                let empty = Some(Aabb::EMPTY);
                                let sides =
                                    [(near, [empty, *other_box]), (far, [*one_box, empty])];
                                for (own, boxes) in sides {
                                    if let Some(piece) = clipped_line(line, own, own) {
                                        let curves = vec![piece];
                                        out.push(Shared { curves, boxes, islands: Vec::new() });
                                    }
                                }
                            }
                            _ => kept.push(curve),
                        }
                    }
                    let boxes = [*one_box, *other_box];
                    out.push(Shared { curves: kept, boxes, islands: Vec::new() });
                }
                // Two faces on one surface. Where they cover exactly the same
                // ground there is nothing to imprint — the boolean decides
                // which copy of the shared wall survives from the two
                // normals.
                //
                // Where one covers more than the other, what separates the
                // shared part from the rest is the *other face's own
                // boundary*, already sitting in the shared plane. Cutting
                // each along it is the imprint: afterwards every piece is
                // either the whole of a shared wall or none of one, which is
                // the only case the boolean has an answer for.
                Meeting::Coincident => {
                    // Coplanar is not the same as overlapping: two boxes
                    // stacked face to face have four pairs of side walls on
                    // one plane each, meeting along a line and sharing no
                    // area at all. Only a genuine overlap has anything to
                    // decide.
                    if overlap(a, *one, b, *other, tolerance)
                        && !same_ground(a, *one, b, *other, tolerance)
                    {
                        // A loop of one face lying wholly inside the other —
                        // a slot's opening in a hole wall, over the shaft in
                        // it — is copied across as it is, corners and all.
                        // Its curves would cut the other face only as whole
                        // lines and loops running on far past the opening.
                        let mut curves = Vec::new();
                        let mut islands = Vec::new();
                        for (from, from_face, into, into_face, into_a) in
                            [(&*a, *one, &*b, *other, false), (&*b, *other, &*a, *one, true)]
                        {
                            let rings = from
                                .faces
                                .get(from_face)
                                .map(|node| node.loops.clone())
                                .unwrap_or_default();
                            for ring in &rings {
                                if island_fits(from, *ring, into, into_face, tolerance) {
                                    islands.push((into_a, into_face, from_face, *ring));
                                } else if loop_reaches(from, *ring, into, into_face, tolerance) {
                                    let reaching =
                                        edges_reaching(from, *ring, into, into_face, tolerance);
                                    curves.extend(loop_curves(from, *ring, &reaching));
                                }
                            }
                        }
                        let loopless = [(&*a, *one), (&*b, *other)].iter().all(|(body, face)| {
                            body.faces.get(*face).is_some_and(|node| node.loops.is_empty())
                        });
                        if loopless {
                            return Err(Snag::Coincident);
                        }
                        // Boxes that overlap round faces that do not: nothing
                        // of either reaches the other.
                        if !curves.is_empty() || !islands.is_empty() {
                            out.push(Shared { curves, boxes: [*one_box, *other_box], islands });
                        }
                    }
                }
                // No closed form for the pair. Their meshes may still show
                // the two faces never come near; where they do cross, the
                // crossing is traced onto both surfaces instead.
                Meeting::Unknown => {
                    if proved_apart(a, *one, one_surface, b, *other, other_surface, tolerance) {
                        continue;
                    }
                    let soups = soups.get_or_insert_with(|| {
                        (
                            super::near::FaceSoups::of(a, tolerance),
                            super::near::FaceSoups::of(b, tolerance),
                        )
                    });
                    if super::near::apart(&soups.0, *one, &soups.1, *other, tolerance) {
                        continue;
                    }
                    let known = match marched.iter().position(|(s, t, _)| {
                        same_surface(s, one_surface, tolerance)
                            && same_surface(t, other_surface, tolerance)
                    })
                    {
                        Some(index) => index,
                        None => {
                            marched.push((
                                one_surface.clone(),
                                other_surface.clone(),
                                Vec::new(),
                            ));
                            marched.len() - 1
                        }
                    };
                    let curves = soups
                        .0
                        .triangles(*one)
                        .zip(soups.1.triangles(*other))
                        .and_then(|(first, second)| {
                            super::march::traced(
                                one_surface,
                                first,
                                other_surface,
                                second,
                                &mut marched[known].2,
                                boxes,
                                tolerance,
                            )
                        });
                    // Where the march cannot follow the crossing (a tangency
                    // on the way, a branch point), the faces' bounded pieces
                    // go through the full surface/surface intersection.
                    let curves = curves.or_else(|| {
                        bounded_meeting(a, *one, one_surface, b, *other, other_surface, tolerance)
                    });
                    // Faces that come near without a crossing to start from
                    // are settled once every pair is done: a meeting traced
                    // from another pair of the same surfaces already runs
                    // past them, whole.
                    let Some(curves) = curves else {
                        untraced.push(known);
                        continue;
                    };
                    let curves = curves
                        .into_iter()
                        .flat_map(|curve| {
                            along_edges(a, &curve, tolerance)
                                .or_else(|| along_edges(b, &curve, tolerance))
                                .unwrap_or_else(|| vec![curve])
                        })
                        .collect();
                    out.push(Shared { curves, boxes: [*one_box, *other_box], islands: Vec::new() });
                }
            }
        }
    }
    if untraced.iter().any(|known| marched[*known].2.is_empty()) {
        return Err(Snag::NoClosedForm);
    }
    Ok(out)
}

/// One surface whatever frame it is measured in: the eight faces of a
/// divided sphere each carry their own, and a curve traced on one of them
/// is the same curve on all.
fn same_surface(one: &Surface, other: &Surface, tolerance: f64) -> bool {
    let near = |a: [f64; 3], b: [f64; 3]| Vec3::from(a).distance(Vec3::from(b)) <= tolerance;
    match (one, other) {
        (Surface::Sphere(a), Surface::Sphere(b)) => {
            near(a.frame.origin, b.frame.origin) && (a.radius - b.radius).abs() <= tolerance
        }
        // The patches of a divided torus turn only their frames round the
        // axis (see `make::torus_in_patches`).
        (Surface::Torus(a), Surface::Torus(b)) => {
            let axis = |torus: &super::geometry::Torus| torus.frame.normal().map(Vec3::from);
            near(a.frame.origin, b.frame.origin)
                && (a.major_radius - b.major_radius).abs() <= tolerance
                && (a.minor_radius - b.minor_radius).abs() <= tolerance
                && axis(a)
                    .zip(axis(b))
                    .is_some_and(|(one, other)| one.distance(other) <= tolerance)
        }
        // And so do the quarters of a divided cone.
        (Surface::Cone(a), Surface::Cone(b)) => {
            let axis = |cone: &super::geometry::Cone| cone.base.normal().map(Vec3::from);
            near(a.base.origin, b.base.origin)
                && (a.radius - b.radius).abs() <= tolerance
                && (a.half_angle - b.half_angle).abs() <= 1e-12
                && axis(a)
                    .zip(axis(b))
                    .is_some_and(|(one, other)| one.distance(other) <= tolerance)
        }
        _ => one == other,
    }
}

/// The curves of a body's own edges that a traced curve runs along, if it
/// runs along them all the way. A shaft fitted in a hole meets the hole's
/// fillet where the hole already does, along an edge the file saved as a
/// spline: the traced curve is that edge again, a fit apart, and cutting
/// with both peels hairline slivers off between them. The edge's own curve
/// cuts the same and agrees with the vertices already there.
fn along_edges(body: &Body, curve: &Curve3, tolerance: f64) -> Option<Vec<Curve3>> {
    const SAMPLES: usize = 16;
    let Curve3::Nurbs(spline) = curve else {
        return None;
    };
    let (start, end) = spline.domain();
    let points: Vec<Vec3> = (0..=SAMPLES)
        .map(|index| {
            Vec3::from(curve.point_at(start + (end - start) * index as f64 / SAMPLES as f64))
        })
        .collect();
    let extent = points
        .iter()
        .map(|point| point.distance(points[0]))
        .fold(0.0, f64::max);
    let fit = tolerance.max(extent * 1e-3);
    let reach = |samples: &[Vec3]| {
        let low = samples.iter().fold(samples[0], |low, p| {
            Vec3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z))
        });
        let high = samples.iter().fold(samples[0], |high, p| {
            Vec3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z))
        });
        (low, high)
    };
    let (low, high) = reach(&points);
    // Only edges near the curve at all are worth the nearest-point search.
    let edges: Vec<&Curve3> = body
        .edges
        .iter()
        .filter_map(|(_, edge)| {
            let curve = body.curves.get(edge.curve)?;
            let samples: Vec<Vec3> = (0..=8)
                .map(|index| {
                    Vec3::from(curve.point_at(
                        edge.start_parameter
                            + (edge.end_parameter - edge.start_parameter) * index as f64 / 8.0,
                    ))
                })
                .collect();
            let (edge_low, edge_high) = reach(&samples);
            let margin = edge_low.distance(edge_high) * 0.25 + fit;
            let apart = edge_low.x > high.x + margin
                || edge_low.y > high.y + margin
                || edge_low.z > high.z + margin
                || edge_high.x < low.x - margin
                || edge_high.y < low.y - margin
                || edge_high.z < low.z - margin;
            (!apart).then_some(curve)
        })
        .collect();
    let mut used = vec![false; edges.len()];
    for point in &points {
        // Along the edge's whole curve, not just its span: a file keeps the
        // whole of an intersection loop as the curve of the part its face
        // still has, and the rest of the loop is the same curve again.
        let nearest = edges.iter().position(|edge| {
            Vec3::from(edge.point_at(edge.parameter_at(point.to_array()))).distance(*point) <= fit
        })?;
        used[nearest] = true;
    }
    let mut out: Vec<Curve3> = Vec::new();
    for (edge, used) in edges.into_iter().zip(used) {
        if used && !out.contains(edge) {
            out.push(edge.clone());
        }
    }
    Some(out)
}

/// A box round a bounded curve; `None` for a line, which has no end.
fn curve_bounds(curve: &Curve3) -> Option<Aabb> {
    match curve {
        Curve3::Circle(circle) => {
            let r = circle.radius.abs();
            let c = circle.plane.origin;
            Aabb::around([[c[0] - r, c[1] - r, c[2] - r], [c[0] + r, c[1] + r, c[2] + r]])
        }
        Curve3::Ellipse(ellipse) => {
            let r = ellipse.major_radius.abs().max(ellipse.minor_radius.abs());
            let c = ellipse.plane.origin;
            Aabb::around([[c[0] - r, c[1] - r, c[2] - r], [c[0] + r, c[1] + r, c[2] + r]])
        }
        // Positive weights keep a spline inside its control hull.
        Curve3::Nurbs(spline) if spline.weights().iter().all(|weight| *weight > 0.0) => {
            Aabb::around(spline.control_points().iter().copied())
        }
        _ => None,
    }
}

/// Cuts every face of `body` that one of `curves` crosses.
///
/// A face split in two may still be crossed by the next curve, and by the
/// same one where a curve enters and leaves more than once, so the halves go
/// back into the list to be tried again.
/// How many coedges bound each face: what a cut or a split edge changes.
fn outlines(body: &Body) -> HashMap<FaceKey, usize> {
    body.faces
        .iter()
        .map(|(key, face)| {
            let count = face
                .loops
                .iter()
                .filter_map(|ring| body.loops.get(*ring))
                .map(|ring| ring.coedges.len() + 1)
                .sum();
            (key, count)
        })
        .collect()
}

fn cut_along(
    body: &mut Body,
    curves: &[Curve3],
    beside: &[Option<Aabb>],
    only: Option<&HashSet<FaceKey>>,
    tolerance: f64,
) -> Result<usize, Snag> {
    let mut cuts = 0;
    // Face boxes are read for every curve against every face; measured once
    // each and again only for a face a cut changes.
    let mut boxes: HashMap<FaceKey, Option<Aabb>> = HashMap::new();
    for (curve, beside) in curves.iter().zip(beside) {
        // Only a face whose box the curve reaches can it cut; trying the
        // others copies the whole body for nothing.
        let reach = curve_bounds(curve).map(|bounds| bounds.grown(tolerance * 10.0));
        let faces: Vec<(FaceKey, Option<Aabb>)> = body
            .face_keys()
            .filter(|face| only.is_none_or(|only| only.contains(face)))
            .map(|face| (face, *boxes.entry(face).or_insert_with(|| face_bounds(body, face))))
            .collect();
        // And only a face whose surface the curve lies on: a meeting curve
        // is on both surfaces it came from and crosses any other at points,
        // which divide nothing. One point near the face settles it, where
        // trying the cut would project the whole curve first.
        let lies_on = |face: FaceKey, bounds: Option<Aabb>| -> bool {
            let (Some(node), Some(bounds)) = (body.faces.get(face), bounds) else {
                return true;
            };
            let Some(surface) = body.surfaces.get(node.surface) else {
                return true;
            };
            let centre = std::array::from_fn(|axis| 0.5 * (bounds.min[axis] + bounds.max[axis]));
            let size = reach.as_ref().unwrap_or(&bounds);
            let size = Vec3::from(size.min).distance(Vec3::from(size.max));
            let point = curve.point_at(curve.parameter_at(centre));
            let gap = surface.distance_to(point).abs();
            !(gap > tolerance.max(size * 1e-3))
        };
        let mut pending: Vec<FaceKey> = faces
            .into_iter()
            .filter(|(_, bounds)| match (&reach, bounds) {
                (Some(reach), Some(bounds)) => reach.overlaps(bounds),
                _ => true,
            })
            .filter(|(_, bounds)| match (beside, bounds) {
                (Some(beside), Some(bounds)) => beside.grown(tolerance * 10.0).overlaps(bounds),
                _ => true,
            })
            .filter(|(face, bounds)| lies_on(*face, *bounds))
            .map(|(face, _)| face)
            .collect();
        // Each cut adds at most one face, so the work is bounded by however
        // many faces the curve can produce. The cap is a backstop against a
        // cut that somehow keeps splitting the same face rather than a limit
        // anything real reaches.
        let ceiling = body.faces.len() * 4 + 16;
        let mut done = 0;
        while let Some(face) = pending.pop() {
            done += 1;
            if done > ceiling {
                return Err(Snag::CutRefused);
            }
            if !body.faces.contains(face) {
                continue;
            }
            if let Some([kept, made]) = split_face(body, face, curve, tolerance) {
                cuts += 1;
                boxes.remove(&kept);
                boxes.remove(&made);
                pending.push(kept);
                pending.push(made);
            }
        }
    }
    Ok(cuts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cap_within_the_tolerance_is_a_touch() {
        use crate::geometry::{Circle3, Sphere};
        // A unit ball and a plane `sink` below its top: they cross in a
        // circle round a cap `sink` high.
        let tolerance = 1e-6;
        let ball = Surface::Sphere(Sphere {
            frame: ocs_geom::space::Plane::XY,
            radius: 1.0,
        });
        for (sink, touch) in [(tolerance * 0.25, true), (tolerance * 100.0, false)] {
            let height = 1.0 - sink;
            let plane = Surface::Plane(ocs_geom::space::Plane::from_axes(
                [0.0, 0.0, height],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            ));
            let circle = Curve3::Circle(Circle3 {
                plane: ocs_geom::space::Plane::from_axes(
                    [0.0, 0.0, height],
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                ),
                radius: (1.0 - height * height).sqrt(),
            });
            assert_eq!(tangent_cap(&plane, &ball, &circle, tolerance), touch, "{sink}");
        }
    }
    use crate::make::cuboid;

    const TOL: f64 = 1e-9;

    /// Two boxes overlapping in a corner.
    fn overlapping() -> (Body, Body) {
        (
            cuboid([0.0; 3], [10.0, 10.0, 10.0]).unwrap(),
            cuboid([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap(),
        )
    }

    #[test]
    fn imprinting_leaves_both_bodies_consistent() {
        let (mut a, mut b) = overlapping();
        imprint(&mut a, &mut b, TOL).expect("two boxes meet in planes");
        for (name, body) in [("a", &a), ("b", &b)] {
            let flaws = body.validate();
            assert!(flaws.is_empty(), "{name}: {flaws:?}");
            assert_eq!(body.euler_characteristic(), 2, "{name}");
        }
    }

    #[test]
    fn imprinting_adds_edges_and_faces_but_no_volume() {
        // An imprint only writes the shape down differently.
        let (mut a, mut b) = overlapping();
        let before = crate::body_bounds(&a).unwrap();
        let faces = a.faces.len();
        let result = imprint(&mut a, &mut b, TOL).unwrap();
        assert!(result.cuts > 0, "the boxes overlap, so something is cut");
        assert!(a.faces.len() > faces);
        let after = crate::body_bounds(&a).unwrap();
        assert_eq!(before, after, "the solid did not move or grow");
    }

    #[test]
    fn every_piece_is_wholly_in_or_wholly_out_afterwards() {
        // The property the whole step exists for. Before the imprint, the
        // faces of A that the corner of B passes through are partly inside
        // it; afterwards no face is.
        use crate::{contains_point, Containment};
        let (mut a, mut b) = overlapping();
        imprint(&mut a, &mut b, TOL).unwrap();
        for face in a.face_keys() {
            let bounds = crate::face_bounds(&a, face).unwrap();
            let mut seen = Vec::new();
            // The corners of the face's own box, nudged onto the face, are
            // enough to catch a face that straddles the boundary.
            for coedge in a.face_coedges(face) {
                let Some((from, _)) = a.coedge_vertices(coedge) else {
                    continue;
                };
                let point = a.vertices.get(from).unwrap().point;
                seen.push(contains_point(&b, point, 1e-6));
            }
            let _ = bounds;
            let inside = seen.iter().filter(|c| **c == Containment::Inside).count();
            let outside = seen.iter().filter(|c| **c == Containment::Outside).count();
            assert!(
                inside == 0 || outside == 0,
                "face {face:?} has corners both in and out: {seen:?}"
            );
        }
    }

    #[test]
    fn boxes_that_do_not_touch_are_left_alone() {
        let mut a = cuboid([0.0; 3], [1.0, 1.0, 1.0]).unwrap();
        let mut b = cuboid([50.0, 50.0, 50.0], [1.0, 1.0, 1.0]).unwrap();
        let faces = (a.faces.len(), b.faces.len());
        let result = imprint(&mut a, &mut b, TOL).unwrap();
        assert_eq!(result.cuts, 0);
        assert_eq!((a.faces.len(), b.faces.len()), faces);
    }

    #[test]
    fn faces_meeting_on_one_plane_need_nothing_when_they_cover_it_alike() {
        // Stacked exactly: the two that meet cover the same ground and the
        // four pairs of side walls share a plane without sharing any area.
        // Neither needs cutting, so there is nothing to imprint.
        let mut a = cuboid([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
        let mut b = cuboid([0.0, 0.0, 10.0], [10.0, 10.0, 10.0]).unwrap();
        let result = imprint(&mut a, &mut b, TOL).expect("nothing to cut");
        assert_eq!(result.cuts, 0);
    }

    #[test]
    fn a_wall_shared_in_part_is_cut_where_the_sharing_stops() {
        // A smaller box on top: its bottom covers a corner of the other's
        // top. What separates the shared part from the rest is the small
        // box's own boundary, so cutting the big face along it leaves pieces
        // that are each wholly shared or wholly not — which is the only
        // shape the boolean has an answer for.
        let mut a = cuboid([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
        let mut b = cuboid([0.0, 0.0, 10.0], [4.0, 4.0, 4.0]).unwrap();
        let before = (a.faces.len(), b.faces.len());
        let result = imprint(&mut a, &mut b, TOL).expect("a wall to cut");
        assert!(result.cuts > 0, "{result:?}");
        assert!(a.faces.len() > before.0, "the big face was divided");
        assert_eq!(b.faces.len(), before.1, "the small one had nothing to lose");
        // An imprint only adds edges, so both are still solids.
        assert!(a.validate().is_empty());
        assert!(b.validate().is_empty());
        assert_eq!(a.euler_characteristic(), 2);
        assert_eq!(b.euler_characteristic(), 2);

        // And the piece that matches the small box's footprint really is one
        // face now, rather than a corner of a larger one.
        let footprint = a.face_keys().filter(|face| {
            face_bounds(&a, *face).is_some_and(|box_| {
                (box_.min[2] - 10.0).abs() < TOL
                    && (box_.max[0] - 4.0).abs() < TOL
                    && (box_.max[1] - 4.0).abs() < TOL
                    && box_.min[0].abs() < TOL
                    && box_.min[1].abs() < TOL
            })
        });
        assert_eq!(footprint.count(), 1);
    }

    #[test]
    fn a_pair_whose_meshes_never_cross_is_left_uncut() {
        let mut a = cuboid([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
        let mut b = cuboid([5.0; 3], [10.0, 10.0, 10.0]).unwrap();
        // Turn one of B's faces into a torus, which no pair here has a closed
        // form for and whose face, still bounded by the box's edges, meets
        // nothing of A's: it only comes near, so it is left as it is while
        // the planes either side still cut.
        let face = b.face_keys().next().unwrap();
        let surface = b.faces.get(face).unwrap().surface;
        *b.surfaces.get_mut(surface).unwrap() =
            crate::Surface::Torus(crate::Torus {
                frame: ocs_geom::space::Plane::XY,
                major_radius: 4.0,
                minor_radius: 1.0,
            });
        assert!(imprint(&mut a, &mut b, TOL).is_ok_and(|report| report.cuts > 0));
    }

    #[test]
    fn the_prefilter_does_not_lose_a_real_meeting() {
        // Boxes that share only an edge region: most face pairs are apart,
        // and the box test has to keep the few that are not.
        let (mut a, mut b) = overlapping();
        let result = imprint(&mut a, &mut b, TOL).unwrap();
        assert!(result.meetings > 0);
        assert!(
            result.meetings < 36,
            "the prefilter rejected nothing: {}",
            result.meetings
        );
    }

    #[test]
    fn imprinting_at_survey_coordinates_works_the_same() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let mut a = cuboid(origin, [10.0, 10.0, 10.0]).unwrap();
        let mut b = cuboid(
            [origin[0] + 5.0, origin[1] + 5.0, origin[2] + 5.0],
            [10.0, 10.0, 10.0],
        )
        .unwrap();
        imprint(&mut a, &mut b, 1e-6).expect("the same two boxes, further out");
        assert!(a.validate().is_empty());
        assert!(b.validate().is_empty());
        assert!(a.worst_vertex_gap() < 1e-6);
    }
}
