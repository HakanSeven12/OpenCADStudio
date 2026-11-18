//! A ball rolled along a closed chain of edges between two faces of any
//! surfaces with an exact offset: the saddle where two cylinders join, a
//! ring where a cone meets a sphere. The ball's centre runs where the two
//! surfaces moved by the radius meet; it touches each face along a curve,
//! and the round between those two curves replaces the chain. The faces
//! keep their own surfaces and are trimmed back to the touching curves; an
//! edge meeting the chain — a seam crossing it — is followed to where it
//! meets the touching curve instead.

use super::shell::{least_norm_step, moved_surface};
use super::topology::{
    Coedge, CoedgeKey, CurveKey, Edge, EdgeKey, Face, FaceKey, Loop, Vertex, VertexKey,
};
use super::{Body, Curve3, Provenance, Surface};
use ocs_geom::space::spline::{interpolate_open, interpolate_periodic};
use ocs_geom::space::{NurbsCurve3, NurbsSurface3, Parameterization, Vec3};
use std::collections::{HashMap, HashSet};

/// Points the ball's path is followed through, round each closed chain.
const SAMPLES: usize = 160;

/// Rounds every closed chain the selected edges lie on. `None` where a
/// chain is open, its faces' surfaces have no exact offset, the ball cannot
/// be placed along it, or the rounded solid does not come out sound.
pub(super) fn fillet_rolling(body: &Body, selected: &[EdgeKey], radius: f64) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[body]);
    let sides = sides_of(body, tolerance);
    let face_of = |body: &Body, coedge: CoedgeKey| -> Option<FaceKey> {
        Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner)
    };
    // The sides an edge runs between: faces on one surface — a cylinder a
    // boolean parted along its seam — are one side.
    let pair_of = |body: &Body, edge: EdgeKey| -> Option<(FaceKey, FaceKey)> {
        let node = body.edges.get(edge)?;
        let [one, other] = node.coedges[..] else { return None };
        let (one, other) = (sides[&face_of(body, one)?], sides[&face_of(body, other)?]);
        (one != other).then_some((one, other))
    };
    // Each pair of sides the selection runs between, with every edge they
    // share: one selected edge of a chain rounds the whole chain.
    let mut pairs: Vec<(FaceKey, FaceKey)> = Vec::new();
    for edge in selected {
        let (one, other) = pair_of(body, *edge)?;
        if !pairs.iter().any(|pair| *pair == (one, other) || *pair == (other, one)) {
            pairs.push((one, other));
        }
    }
    // Each chain, and which way its corner turns, read off the body as it
    // came: a round made already would be asked through its own spline.
    let mut chains = Vec::new();
    for (one, other) in pairs {
        let chain: Vec<EdgeKey> = body
            .edges
            .keys()
            .filter(|edge| {
                pair_of(body, *edge)
                    .is_some_and(|pair| pair == (one, other) || pair == (other, one))
            })
            .collect();
        // The pair may meet along more than one loop — a hole's two ends
        // between a wall and the bore — each its own chain.
        for chain in loops_of(body, &chain) {
            let concave = concave_chain(body, &chain, radius, tolerance)?;
            chains.push((chain, one, other, concave));
        }
    }
    let mut result = body.clone();
    for (chain, one, other, concave) in chains {
        round_chain(&mut result, &chain, [one, other], &sides, radius, concave, tolerance)?;
    }
    if !result.validate().is_empty() {
        return None;
    }
    // A round adds what it fills into a concave chain and takes what it cuts
    // off a convex one, a sliver either way: a face turned inside out shows
    // as a large change even on a coarse mesh, which is all this asks.
    let volume = |body: &Body| -> Option<f64> {
        let mesh =
            super::mesh::tessellate(body, super::mesh::TessellationTolerance::new(0.3, tolerance));
        mesh.missing_faces.is_empty().then(|| mesh.mesh.mass_properties().map(|(v, _)| v))?
    };
    let (before, after) = (volume(body)?, volume(&result)?);
    ((after - before).abs() <= before.abs() * 0.1).then_some(result)
}

/// Each face mapped to the first face lying on the same surface, facing the
/// same way: the side of a chain it belongs to.
fn sides_of(body: &Body, tolerance: f64) -> HashMap<FaceKey, FaceKey> {
    let mut firsts: Vec<FaceKey> = Vec::new();
    let mut sides = HashMap::new();
    for (key, face) in body.faces.iter() {
        let same = |other: &FaceKey| {
            let Some(node) = body.faces.get(*other) else { return false };
            if node.forward != face.forward {
                return false;
            }
            if node.surface == face.surface {
                return true;
            }
            match (body.surfaces.get(node.surface), body.surfaces.get(face.surface)) {
                (Some(a), Some(b)) => a == b || coincide(a, b, tolerance),
                _ => false,
            }
        };
        let first = firsts.iter().copied().find(same).unwrap_or_else(|| {
            firsts.push(key);
            key
        });
        sides.insert(key, first);
    }
    sides
}

/// Two surfaces of a kind that are one surface framed differently: a
/// cylinder or a sphere with the same axis or centre and radius.
fn coincide(a: &Surface, b: &Surface, tolerance: f64) -> bool {
    let on_line = |point: Vec3, origin: Vec3, axis: Vec3| {
        let offset = point - origin;
        (offset - axis * offset.dot(axis)).length() <= tolerance
    };
    match (a, b) {
        (Surface::Cylinder(a), Surface::Cylinder(b)) => {
            let (Some(axis), Some(other)) = (a.base.normal(), b.base.normal()) else {
                return false;
            };
            let (axis, other) = (Vec3::from(axis), Vec3::from(other));
            (a.radius - b.radius).abs() <= tolerance
                && axis.cross(other).length() <= 1e-9
                && on_line(Vec3::from(b.base.origin), Vec3::from(a.base.origin), axis)
        }
        (Surface::Sphere(a), Surface::Sphere(b)) => {
            (a.radius - b.radius).abs() <= tolerance
                && Vec3::from(a.frame.origin).distance(Vec3::from(b.frame.origin)) <= tolerance
        }
        _ => false,
    }
}

/// `edges` parted into the groups joined end to end.
fn loops_of(body: &Body, edges: &[EdgeKey]) -> Vec<Vec<EdgeKey>> {
    let mut left: Vec<EdgeKey> = edges.to_vec();
    let mut out = Vec::new();
    while let Some(seed) = left.pop() {
        let mut group = vec![seed];
        let mut grew = true;
        while grew {
            grew = false;
            let ends: HashSet<VertexKey> = group
                .iter()
                .filter_map(|edge| body.edges.get(*edge))
                .flat_map(|edge| [edge.start, edge.end])
                .collect();
            if let Some(at) = left.iter().position(|edge| {
                body.edges
                    .get(*edge)
                    .is_some_and(|node| ends.contains(&node.start) || ends.contains(&node.end))
            }) {
                group.push(left.remove(at));
                grew = true;
            }
        }
        out.push(group);
    }
    out
}

/// The chain's edges in order round it, each with whether it is walked
/// along its own direction. `None` unless they close into one loop.
fn ordered(body: &Body, chain: &[EdgeKey]) -> Option<Vec<(EdgeKey, bool)>> {
    let first = *chain.first()?;
    let mut out = vec![(first, true)];
    let mut at = body.edges.get(first)?.end;
    let start = body.edges.get(first)?.start;
    let mut used: HashSet<EdgeKey> = HashSet::from([first]);
    while at != start || out.len() < chain.len() {
        let next = chain.iter().copied().find_map(|edge| {
            let node = body.edges.get(edge)?;
            if used.contains(&edge) {
                return None;
            }
            if node.start == at {
                Some((edge, true))
            } else if node.end == at {
                Some((edge, false))
            } else {
                None
            }
        })?;
        used.insert(next.0);
        let node = body.edges.get(next.0)?;
        at = if next.1 { node.end } else { node.start };
        out.push(next);
        if out.len() > chain.len() {
            return None;
        }
    }
    (out.len() == chain.len()).then_some(out)
}

/// Concave where the solid takes in a point off one face's outside and the
/// other's inside — the union of the two sides — so the ball rolls outside
/// it and fills; convex, where the solid is only what is inside both, so it
/// rolls inside and cuts.
fn concave_chain(body: &Body, chain: &[EdgeKey], radius: f64, tolerance: f64) -> Option<bool> {
    let edge = body.edges.get(*chain.first()?)?;
    let [one, other] = edge.coedges[..] else { return None };
    let face_of = |coedge: CoedgeKey| Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner);
    let (one, other) = (face_of(one)?, face_of(other)?);
    let curve = body.curves.get(edge.curve)?;
    let probe = Vec3::from(curve.point_at(0.5 * (edge.start_parameter + edge.end_parameter)));
    let outward = |face: FaceKey| -> Option<Vec3> {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        let (u, v) = surface.parameters_at(probe.to_array())?;
        let sense = if node.forward { 1.0 } else { -1.0 };
        Some(Vec3::from(surface.normal_at(u, v)?).normalize()? * sense)
    };
    let across = outward(one)? - outward(other)?;
    let at = probe + across * (radius * 0.01);
    let side = super::classify::contains_point(body, at.to_array(), tolerance);
    Some(side == super::Containment::Inside)
}

/// A stretch of a chain one round face is built along: the whole closed
/// chain, or, where the two faces meet tangentially at corners of it — the
/// sides of an equal-radius tee — the stretch from one such pinch to the
/// next, the round closing to a point at each.
struct Stretch {
    edges: Vec<(EdgeKey, bool)>,
    closed: bool,
}

/// The round along one stretch: its surface, the curves it touches each
/// side along, and the ball's centre and the round's point half-way.
struct Round {
    surface: NurbsSurface3,
    touches: [NurbsCurve3; 2],
    middle: (Vec3, f64),
}

#[allow(clippy::too_many_arguments)]
fn round_chain(
    body: &mut Body,
    chain: &[EdgeKey],
    [one, other]: [FaceKey; 2],
    sides: &HashMap<FaceKey, FaceKey>,
    radius: f64,
    concave: bool,
    tolerance: f64,
) -> Option<()> {
    let ring = ordered(body, chain)?;
    let surface_of = |face: FaceKey| -> Option<(Surface, f64)> {
        let node = body.faces.get(face)?;
        Some((body.surfaces.get(node.surface)?.clone(), if node.forward { 1.0 } else { -1.0 }))
    };
    let faces = [surface_of(one)?, surface_of(other)?];
    // Corners where the faces meet tangentially: the round has no width
    // there, and is parted into one face each side.
    let start_of = |body: &Body, (edge, forward): (EdgeKey, bool)| -> Option<VertexKey> {
        let node = body.edges.get(edge)?;
        Some(if forward { node.start } else { node.end })
    };
    let mut pinches = HashSet::new();
    for step in &ring {
        let corner = start_of(body, *step)?;
        let point = Vec3::from(body.vertices.get(corner)?.point);
        let normals = (outward(&faces[0], point)?, outward(&faces[1], point)?);
        if normals.0.dot(normals.1) > 1.0 - 1e-9 {
            pinches.insert(corner);
        }
    }
    let stretches = if pinches.is_empty() {
        vec![Stretch { edges: ring, closed: true }]
    } else {
        let begin = ring
            .iter()
            .position(|step| start_of(body, *step).is_some_and(|at| pinches.contains(&at)))?;
        let mut out: Vec<Stretch> = Vec::new();
        for index in 0..ring.len() {
            let step = ring[(begin + index) % ring.len()];
            if pinches.contains(&start_of(body, step)?) {
                out.push(Stretch { edges: Vec::new(), closed: false });
            }
            out.last_mut()?.edges.push(step);
        }
        out
    };
    let rounds: Vec<Round> = stretches
        .iter()
        .map(|stretch| round_along(body, stretch, &faces, radius, concave, tolerance))
        .collect::<Option<_>>()?;

    // Each face of each side trimmed back to its touching curves.
    let stretch_of: HashMap<EdgeKey, usize> = stretches
        .iter()
        .enumerate()
        .flat_map(|(index, stretch)| stretch.edges.iter().map(move |(edge, _)| (*edge, index)))
        .collect();
    let mut touched: [Vec<FaceKey>; 2] = [Vec::new(), Vec::new()];
    for edge in chain {
        for coedge in &body.edges.get(*edge)?.coedges {
            let face = body.loops.get(body.coedges.get(*coedge)?.owner)?.owner;
            let side = if sides.get(&face) == Some(&one) { 0 } else { 1 };
            if !touched[side].contains(&face) {
                touched[side].push(face);
            }
        }
    }
    let shell = body.faces.get(one)?.owner;
    let mut made: [Vec<(usize, EdgeKey, bool, f64)>; 2] = [Vec::new(), Vec::new()];
    for side in 0..2 {
        let curves: Vec<Touch> = rounds
            .iter()
            .zip(&stretches)
            .map(|(round, stretch)| Touch {
                key: body.curves.insert(Curve3::Nurbs(round.touches[side].clone())),
                curve: round.touches[side].clone(),
                closed: stretch.closed,
            })
            .collect();
        let mut moved = HashMap::new();
        for face in &touched[side] {
            let trim = Trim { stretch_of: &stretch_of, curves: &curves, pinches: &pinches };
            made[side].extend(retrim(body, *face, &trim, &mut moved, tolerance)?);
        }
        made[side].sort_by(|a, b| a.3.total_cmp(&b.3));
    }
    // The chain's own edges go, and its corners nothing else still uses.
    let mut corners = HashSet::new();
    for edge in chain {
        if let Some(node) = body.edges.remove(*edge) {
            corners.extend([node.start, node.end]);
            for coedge in node.coedges {
                body.coedges.remove(coedge);
            }
        }
    }
    let used: HashSet<VertexKey> =
        body.edges.iter().flat_map(|(_, edge)| [edge.start, edge.end]).collect();
    for vertex in corners {
        if !used.contains(&vertex) {
            body.vertices.remove(vertex);
        }
    }

    for (index, (round, stretch)) in rounds.into_iter().zip(&stretches).enumerate() {
        // The round's face, its outward side away from the material: toward
        // the ball's centre where it fills, away from it where it cuts.
        let (centre, u) = round.middle;
        let normal = Vec3::from(round.surface.normal_at_knot(u, 0.5)?);
        let point = Vec3::from(round.surface.point_at_knot(u, 0.5));
        let toward_centre = (centre - point).dot(normal) > 0.0;
        let forward = toward_centre == concave;
        let surface = body.surfaces.insert(Surface::Nurbs(round.surface));
        let face = body.faces.insert(Face {
            surface,
            forward,
            loops: Vec::new(),
            owner: shell,
            provenance: Provenance::Synthesized,
        });
        // The other use of each touching edge, the opposite way, in order
        // round it: a closed round has a loop each side, an open one runs
        // up one side and back down the other.
        let parts = made.iter().map(|side| {
            let mut edges: Vec<(EdgeKey, bool)> = side
                .iter()
                .filter(|made| made.0 == index)
                .map(|made| (made.1, made.2))
                .collect();
            if edges.first().is_some_and(|(_, forward)| *forward) {
                edges.reverse();
            }
            edges
        });
        let loops: Vec<Vec<(EdgeKey, bool)>> = if stretch.closed {
            parts.collect()
        } else {
            vec![parts.flatten().collect()]
        };
        for edges in loops {
            let ring = body.loops.insert(Loop {
                coedges: Vec::new(),
                owner: face,
                provenance: Provenance::Synthesized,
            });
            let mut coedges = Vec::with_capacity(edges.len());
            for (edge, forward) in edges {
                let coedge = body.coedges.insert(Coedge {
                    edge,
                    forward: !forward,
                    pcurve: None,
                    owner: ring,
                    provenance: Provenance::Synthesized,
                });
                body.edges.get_mut(edge)?.coedges.push(coedge);
                coedges.push(coedge);
            }
            body.loops.get_mut(ring)?.coedges = coedges;
            body.faces.get_mut(face)?.loops.push(ring);
        }
        body.shells.get_mut(shell)?.faces.push(face);
    }
    Some(())
}

/// A face's surface's outward normal at a point near it.
fn outward((surface, sense): &(Surface, f64), point: Vec3) -> Option<Vec3> {
    let (u, v) = surface.parameters_at(point.to_array())?;
    Some(Vec3::from(surface.normal_at(u, v)?).normalize()? * *sense)
}

/// The ball rolled along one stretch between the two faces.
fn round_along(
    body: &Body,
    stretch: &Stretch,
    faces: &[(Surface, f64); 2],
    radius: f64,
    concave: bool,
    tolerance: f64,
) -> Option<Round> {
    let closed = stretch.closed;
    // Points along the stretch, evenly by length — the round is fitted as
    // if they were, and a short edge given its own share bunched them —
    // and which way it runs there.
    let mut path: Vec<(Vec3, f64, EdgeKey, bool)> = Vec::new();
    for (edge, forward) in &stretch.edges {
        let node = body.edges.get(*edge)?;
        let span = node.end_parameter - node.start_parameter;
        let curve = body.curves.get(node.curve)?;
        for step in 0..64 {
            let fraction = step as f64 / 64.0;
            let fraction = if *forward { fraction } else { 1.0 - fraction };
            let t = node.start_parameter + span * fraction;
            path.push((Vec3::from(curve.point_at(t)), t, *edge, *forward));
        }
    }
    if !closed {
        // An open stretch ends on its last corner.
        let (edge, forward) = *stretch.edges.last()?;
        let node = body.edges.get(edge)?;
        let t = if forward { node.end_parameter } else { node.start_parameter };
        path.push((Vec3::from(body.curves.get(node.curve)?.point_at(t)), t, edge, forward));
    }
    let pieces = if closed { path.len() } else { path.len() - 1 };
    let mut lengths = vec![0.0];
    for index in 0..pieces {
        let next = &path[(index + 1) % path.len()];
        lengths.push(lengths[index] + path[index].0.distance(next.0));
    }
    let total = *lengths.last()?;
    let count = if closed { SAMPLES } else { SAMPLES + 1 };
    let mut along: Vec<(Vec3, Vec3)> = Vec::with_capacity(count);
    let mut index = 0;
    for sample in 0..count {
        let target = total * sample as f64 / SAMPLES as f64;
        while index + 1 < pieces && lengths[index + 1] < target {
            index += 1;
        }
        let (_, t, edge, forward) = path[index];
        let node = body.edges.get(edge)?;
        let curve = body.curves.get(node.curve)?;
        // Within one sampled step of its edge, the parameter in proportion.
        let next = &path[(index + 1) % path.len()];
        let piece = lengths[index + 1] - lengths[index];
        let share = if piece > 0.0 { ((target - lengths[index]) / piece).min(1.0) } else { 0.0 };
        let t = if next.2 == edge {
            t + (next.1 - t) * share
        } else {
            let end = if forward { node.end_parameter } else { node.start_parameter };
            t + (end - t) * share
        };
        let point = Vec3::from(curve.point_at(t));
        let step = (node.end_parameter - node.start_parameter).abs() * 1e-5;
        let ahead = Vec3::from(curve.point_at(t + step)) - Vec3::from(curve.point_at(t - step));
        let tangent = ahead.normalize()? * if forward { 1.0 } else { -1.0 };
        along.push((point, tangent));
    }
    let side = if concave { 1.0 } else { -1.0 };
    let [(first, first_sense), (second, second_sense)] = faces;
    let first_moved = moved_surface(first, radius * side * first_sense)?;
    let second_moved = moved_surface(second, radius * side * second_sense)?;
    // The ball's centre at each point, square to the chain there, and where
    // it touches each face; at a pinch, the ball on the one tangent plane.
    let mut centres = Vec::with_capacity(along.len());
    let mut touches = Vec::with_capacity(along.len());
    for (sample, (point, tangent)) in along.iter().enumerate() {
        let lean = outward(&faces[0], *point)? + outward(&faces[1], *point)?;
        let mut centre = *point + lean.normalize()? * (radius * side);
        if !closed && (sample == 0 || sample == count - 1) {
            centres.push(centre);
            touches.push((*point, *point));
            continue;
        }
        for _ in 0..40 {
            let gap = |surface: &Surface| -> Option<(Vec3, f64)> {
                let (u, v) = surface.parameters_at(centre.to_array())?;
                let normal = Vec3::from(surface.normal_at(u, v)?).normalize()?;
                Some((normal, surface.distance_to(centre.to_array())))
            };
            let square = (*tangent, (centre - *point).dot(*tangent));
            let rows = [gap(&first_moved)?, gap(&second_moved)?, square];
            if rows.iter().all(|(_, gap)| gap.abs() <= tolerance * 1e-3) {
                break;
            }
            centre = centre - least_norm_step(&rows)?;
        }
        let foot = |surface: &Surface| -> Option<Vec3> {
            let (u, v) = surface.parameters_at(centre.to_array())?;
            Some(Vec3::from(surface.point_at(u, v)))
        };
        let (near, far) = (foot(first)?, foot(second)?);
        if (near.distance(centre) - radius).abs() > radius * 1e-6
            || (far.distance(centre) - radius).abs() > radius * 1e-6
        {
            return None;
        }
        centres.push(centre);
        touches.push((near, far));
    }
    // The round: at each point the arc of the ball from one touch to the
    // other facing the chain, as a rational quadratic; the arcs joined
    // along the stretch in homogeneous coordinates, so the weights follow.
    let mut rows: [Vec<[f64; 4]>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (centre, (near, far)) in centres.iter().zip(&touches) {
        let (a, b) = (*near - *centre, *far - *centre);
        let half = 0.5 * a.cross(b).length().atan2(a.dot(b));
        // Only an open stretch closes to a point, at its ends and nowhere
        // else: a closed one with no width somewhere is not a round.
        if (closed && half <= 1e-6) || half >= std::f64::consts::FRAC_PI_2 - 1e-6 {
            return None;
        }
        let weight = half.cos();
        let middle = *centre + (a + b).normalize()? * (radius / weight);
        let lift = |point: Vec3, weight: f64| {
            [point.x * weight, point.y * weight, point.z * weight, weight]
        };
        rows[0].push(lift(*near, 1.0));
        rows[1].push(lift(middle, weight));
        rows[2].push(lift(*far, 1.0));
    }
    let fitted: Vec<(Vec<[f64; 4]>, Vec<f64>)> = rows
        .iter()
        .map(|row| {
            if closed {
                interpolate_periodic(row, Parameterization::Uniform)
            } else {
                interpolate_open(row, None, None, Parameterization::Uniform)
            }
        })
        .collect::<Option<_>>()?;
    let knots = fitted[0].1.clone();
    let count = fitted[0].0.len();
    let mut net = Vec::with_capacity(count);
    let mut weights = Vec::with_capacity(count);
    for index in 0..count {
        let mut points = Vec::with_capacity(3);
        let mut row_weights = Vec::with_capacity(3);
        for (control, _) in &fitted {
            let [x, y, z, w] = control[index];
            points.push([x / w, y / w, z / w]);
            row_weights.push(w);
        }
        net.push(points);
        weights.push(row_weights);
    }
    let surface = NurbsSurface3::new(
        3,
        2,
        net,
        knots,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        Some(weights),
    )?
    .with_periodicity(closed, false);
    let touch_curve = |side: usize| -> Option<NurbsCurve3> {
        let points: Vec<[f64; 3]> = touches
            .iter()
            .map(|(near, far)| if side == 0 { near.to_array() } else { far.to_array() })
            .collect();
        if closed {
            NurbsCurve3::interpolate_periodic(&points, Parameterization::Uniform)
        } else {
            NurbsCurve3::interpolate_fit(&points, None, None, Parameterization::Uniform)
        }
    };
    // Spaced evenly, sample `i` is at `u = i`.
    let half_way = centres.len() / 2;
    Some(Round {
        surface,
        touches: [touch_curve(0)?, touch_curve(1)?],
        middle: (centres[half_way], half_way as f64),
    })
}

/// One stretch's touching curve on a side, as the side's faces are
/// trimmed to it.
struct Touch {
    key: CurveKey,
    curve: NurbsCurve3,
    closed: bool,
}

/// What [`retrim`] trims a face by: the stretch each chain edge is on, the
/// touching curve of each stretch, and the pinches between stretches.
struct Trim<'a> {
    stretch_of: &'a HashMap<EdgeKey, usize>,
    curves: &'a [Touch],
    pinches: &'a HashSet<VertexKey>,
}

/// Rebuilds `face`'s loops with each run of the chain through them
/// replaced by its stretch's touching curve between the same neighbours,
/// each neighbour's end moved to where it meets that curve — a pinch, on
/// both faces' curves, staying where it is. Returns the new edges, each
/// with its stretch, the way the face walks it and where it starts on the
/// curve.
fn retrim(
    body: &mut Body,
    face: FaceKey,
    trim: &Trim,
    moved: &mut HashMap<VertexKey, (f64, VertexKey)>,
    tolerance: f64,
) -> Option<Vec<(usize, EdgeKey, bool, f64)>> {
    let mut made = Vec::new();
    let loops = body.faces.get(face)?.loops.clone();
    for ring in loops {
        let coedges = body.loops.get(ring)?.coedges.clone();
        let stretch: Vec<Option<usize>> = coedges
            .iter()
            .map(|coedge| {
                let edge = body.coedges.get(*coedge)?.edge;
                trim.stretch_of.get(&edge).copied()
            })
            .collect();
        if stretch.iter().all(Option::is_none) {
            continue;
        }
        let walk = |body: &Body, coedge: CoedgeKey| -> Option<(VertexKey, VertexKey)> {
            body.coedge_vertices(coedge)
        };
        let count = coedges.len();
        // Whether a run of the chain stops before coedge `at`.
        let parts = |body: &Body, at: usize| -> bool {
            let before = stretch[(at + count - 1) % count];
            before.is_none()
                || stretch[at].is_none()
                || before != stretch[at]
                || walk(body, coedges[at]).is_some_and(|(start, _)| trim.pinches.contains(&start))
        };
        let Some(begin) = (0..count).find(|at| parts(body, *at)) else {
            // The whole loop one closed stretch: one edge round it.
            let touch = trim.curves.get(stretch[0]?)?;
            if !touch.closed {
                return None;
            }
            let (start, end) = touch.curve.domain();
            let point = touch.curve.point_at_knot(start);
            let vertex = body.vertices.insert(Vertex { point, provenance: Provenance::Synthesized });
            let probe = probe_of(body, coedges[0])?;
            let along = touch_runs_along(&touch.curve, probe);
            let edge = body.edges.insert(Edge {
                curve: touch.key,
                start_parameter: start,
                end_parameter: end,
                start: vertex,
                end: vertex,
                coedges: Vec::new(),
                provenance: Provenance::Synthesized,
            });
            let coedge = body.coedges.insert(Coedge {
                edge,
                forward: along,
                pcurve: None,
                owner: ring,
                provenance: Provenance::Synthesized,
            });
            body.edges.get_mut(edge)?.coedges.push(coedge);
            body.loops.get_mut(ring)?.coedges = vec![coedge];
            made.push((stretch[0]?, edge, along, start));
            continue;
        };
        let mut rebuilt: Vec<CoedgeKey> = Vec::new();
        let mut index = 0;
        while index < count {
            let at = (begin + index) % count;
            let Some(which) = stretch[at] else {
                rebuilt.push(coedges[at]);
                index += 1;
                continue;
            };
            let mut run = vec![coedges[at]];
            index += 1;
            while index < count && !parts(body, (begin + index) % count) {
                run.push(coedges[(begin + index) % count]);
                index += 1;
            }
            let touch = trim.curves.get(which)?;
            let (start, end) = touch.curve.domain();
            let period = end - start;
            // The way the face walks the run against the curve's own way.
            let along = touch_runs_along(&touch.curve, probe_of(body, run[0])?);
            let (a, _) = walk(body, run[0])?;
            let (_, b) = walk(body, *run.last()?)?;
            let before = coedges[(at + count - 1) % count];
            let after = coedges[(begin + index) % count];
            // A pinch is an end of its stretch's curve: its start where the
            // face walks the curve's way from it.
            let mut place = |vertex: VertexKey, neighbour: CoedgeKey, first: bool| {
                if trim.pinches.contains(&vertex) {
                    Some((if first == along { start } else { end }, vertex))
                } else {
                    settle(body, moved, &touch.curve, vertex, neighbour, tolerance)
                }
            };
            let from = place(a, before, true)?;
            let to = place(b, after, false)?;
            // The stretch from the run's first corner to its last, the way
            // the face walks it.
            let (low, mut high, low_vertex, high_vertex) = if along {
                (from.0, to.0, from.1, to.1)
            } else {
                (to.0, from.0, to.1, from.1)
            };
            if touch.closed {
                high = low + (high - low).rem_euclid(period);
                if high - low <= period * 1e-9 {
                    high = low + period;
                }
            } else if high <= low {
                return None;
            }
            let edge = body.edges.insert(Edge {
                curve: touch.key,
                start_parameter: low,
                end_parameter: high,
                start: low_vertex,
                end: high_vertex,
                coedges: Vec::new(),
                provenance: Provenance::Synthesized,
            });
            let coedge = body.coedges.insert(Coedge {
                edge,
                forward: along,
                pcurve: None,
                owner: ring,
                provenance: Provenance::Synthesized,
            });
            body.edges.get_mut(edge)?.coedges.push(coedge);
            rebuilt.push(coedge);
            made.push((which, edge, along, low));
        }
        body.loops.get_mut(ring)?.coedges = rebuilt;
    }
    Some(made)
}

/// A point half-way along a coedge's edge and the way the coedge runs there.
fn probe_of(body: &Body, coedge: CoedgeKey) -> Option<(Vec3, Vec3)> {
    let node = body.coedges.get(coedge)?;
    let edge = body.edges.get(node.edge)?;
    let source = body.curves.get(edge.curve)?;
    let t = 0.5 * (edge.start_parameter + edge.end_parameter);
    let sense = if node.forward { 1.0 } else { -1.0 };
    Some((Vec3::from(source.point_at(t)), Vec3::from(source.tangent_at(t)) * sense))
}

/// Whether a touching curve runs the way the probe does where it passes it.
fn touch_runs_along(curve: &NurbsCurve3, (point, tangent): (Vec3, Vec3)) -> bool {
    let s = curve.parameter_at(point.to_array());
    Vec3::from(curve.derivative_at_knot(s)).dot(tangent) > 0.0
}

/// Where corner `vertex` of the chain goes on the touching curve: where
/// the curve meets the curve of the edge `neighbour` walks, the meeting
/// nearest the corner. The neighbour's edge is moved onto it with a new
/// corner, made once for each old one.
fn settle(
    body: &mut Body,
    moved: &mut HashMap<VertexKey, (f64, VertexKey)>,
    curve: &NurbsCurve3,
    vertex: VertexKey,
    neighbour: CoedgeKey,
    tolerance: f64,
) -> Option<(f64, VertexKey)> {
    if let Some(known) = moved.get(&vertex) {
        return Some(*known);
    }
    let edge_key = body.coedges.get(neighbour)?.edge;
    let edge = body.edges.get(edge_key)?.clone();
    let other = body.curves.get(edge.curve)?.clone();
    let corner = Vec3::from(body.vertices.get(vertex)?.point);
    let (start, end) = curve.domain();
    let gap = |s: f64| {
        let point = curve.point_at_knot(s);
        let on = other.point_at(other.parameter_at(point));
        Vec3::from(point).distance(Vec3::from(on))
    };
    // Every place the touching curve comes to the neighbour, each settled,
    // the one nearest the corner kept.
    const STEPS: usize = 512;
    let at = |step: usize| start + (end - start) * step as f64 / STEPS as f64;
    let mut best: Option<(f64, f64)> = None;
    for step in 0..STEPS {
        let here = gap(at(step));
        let (before, after) = (gap(at((step + STEPS - 1) % STEPS)), gap(at(step + 1)));
        if !(here <= before && here <= after) {
            continue;
        }
        let reach = (end - start) / STEPS as f64;
        let (mut low, mut high) = (at(step) - reach, at(step) + reach);
        for _ in 0..80 {
            let third = (high - low) / 3.0;
            if gap(low + third) <= gap(high - third) {
                high -= third;
            } else {
                low += third;
            }
        }
        let s = 0.5 * (low + high);
        if gap(s) > tolerance * 10.0 {
            continue;
        }
        let distance = Vec3::from(curve.point_at_knot(s)).distance(corner);
        if best.is_none_or(|(_, kept)| distance < kept) {
            best = Some((s, distance));
        }
    }
    let (s, _) = best?;
    let point = curve.point_at_knot(s);
    let new_vertex =
        body.vertices.insert(Vertex { point, provenance: Provenance::Synthesized });
    let node = body.edges.get_mut(edge_key)?;
    let parameter = other.parameter_at(point);
    if node.start == vertex {
        node.start = new_vertex;
        node.start_parameter = parameter;
    }
    if node.end == vertex {
        node.end = new_vertex;
        node.end_parameter = parameter;
    }
    // A line's parameter runs from its origin; a closed curve's may need
    // a turn added to stay after the start.
    if node.end_parameter < node.start_parameter {
        if let Curve3::Circle(_) | Curve3::Ellipse(_) = other {
            node.end_parameter += std::f64::consts::TAU;
        }
    }
    moved.insert(vertex, (s, new_vertex));
    Some((s, new_vertex))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make::cylinder;
    use crate::place::{transform, Placement};
    use crate::{combine, mass_properties, Operation};

    /// A tee of two pipes of one radius: a main pipe along `x`, a branch
    /// rising along `z` from its axis. The pipes are tangent where the
    /// branch's sides meet the main pipe's, the corners the saddle pinches
    /// at.
    fn equal_tee() -> Body {
        let main = cylinder([0.0, 0.0, -5.0], 1.0, 10.0).unwrap();
        let along_x = Placement {
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
            z_axis: [1.0, 0.0, 0.0],
            origin: [0.0; 3],
        };
        let main = transform(&main, &along_x).unwrap();
        let branch = cylinder([0.0, 0.0, 0.0], 1.0, 4.0).unwrap();
        combine(main, branch, Operation::Union, 1e-7).unwrap()
    }

    #[test]
    fn an_equal_tee_saddle_rounds_closing_to_a_point_at_its_sides() {
        let tee = equal_tee();
        // The saddle: edges between the two pipes, not along one pipe's seam.
        let axis_of = |face: FaceKey| match tee.surfaces.get(tee.faces.get(face)?.surface)? {
            Surface::Cylinder(cylinder) => cylinder.base.normal().map(Vec3::from),
            _ => None,
        };
        let saddle: Vec<EdgeKey> = tee
            .edges
            .iter()
            .filter(|(_, edge)| {
                let axes: Vec<Vec3> = edge
                    .coedges
                    .iter()
                    .filter_map(|coedge| axis_of(tee.loops.get(tee.coedges.get(*coedge)?.owner)?.owner))
                    .collect();
                axes.len() == 2 && axes[0].dot(axes[1]).abs() < 0.5
            })
            .map(|(key, _)| key)
            .collect();
        let rounded = fillet_rolling(&tee, &saddle[..1], 0.2).unwrap();
        assert!(rounded.validate().is_empty());
        // One round each side of the branch, each closing to a point at the
        // two corners where the pipes are tangent.
        let rounds = rounded
            .faces
            .iter()
            .filter(|(_, face)| matches!(rounded.surfaces.get(face.surface), Some(Surface::Nurbs(_))))
            .count();
        assert_eq!(rounds, 2);
        // The round fills the concave saddle: at its widest, at the top of
        // the branch's sides, the corner is square, and what a ball of
        // radius r leaves in a square corner, r^2 (1 - pi / 4) across, run
        // round the two half-ellipses of the saddle (semi-axes 1 and the
        // square root of 2, 3.82 each), bounds what it adds.
        let before = mass_properties(&tee).unwrap().volume;
        let after = mass_properties(&rounded).unwrap().volume;
        let widest = 0.04 * (1.0 - std::f64::consts::FRAC_PI_4) * 2.0 * 3.82;
        assert!(after > before && after - before < widest, "{before} -> {after}");
        // Every face meshes finely too: points by the pinches project back
        // onto the round they lie on.
        let mesh = crate::mesh::tessellate(
            &rounded,
            crate::mesh::TessellationTolerance::new(0.05, 1e-4),
        );
        assert!(mesh.missing_faces.is_empty());
    }
}
