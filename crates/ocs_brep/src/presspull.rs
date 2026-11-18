//! Planar face selection, extrusion, and topology-preserving face offsets.
//!
//! Extrusion adds or removes a prism. Offset instead keeps the neighbouring
//! surfaces and moves their intersections with the selected plane. Neither
//! operation reconstructs a solid from an intersection of half-spaces: doing
//! that changes concave solids, holes, and unrelated lumps into another shape.

use super::nurbs_builder::RationalCurve2;
use super::{
    Body, CoedgeKey, Curve3, EdgeKey, FaceKey, LoopKey, Meeting, Operation, Placement, Surface,
};
use ocs_geom::geom2d::{Arc, Curve, EllipseArc, Line, Polyline, PolylineVertex, Tolerance, Transform};
use ocs_geom::space::{Plane, Vec3};
use std::collections::{HashMap, HashSet};
use std::f64::consts::{FRAC_PI_2, TAU};

/// An exact planar boundary in the face's own coordinates.
#[derive(Debug, Clone)]
pub struct PlanarFaceProfile {
    pub plane: Plane,
    /// First loop encloses the face; subsequent loops cut holes.
    pub loops: Vec<Vec<Curve>>,
    /// Unit outward normal, independent of the parameter plane's handedness.
    pub outward: [f64; 3],
}

/// The two intentionally distinct face editing operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresspullMode {
    /// Add or remove a prism without extending neighbouring surfaces.
    Extrude,
    /// Extend or trim the neighbouring surfaces to the moved face plane.
    Offset,
}

/// The positive-area and zero-area outcomes of a planar intersection.
#[derive(Debug, Clone)]
pub enum PlanarIntersection {
    /// The inputs share a bounded area represented by this open sheet body.
    Area(Body),
    /// The inputs meet only along a boundary point or edge.
    Touching,
    /// The inputs have no point in common.
    Disjoint,
}

/// Extracts every trimmed loop, retaining curved boundaries and holes.
pub fn planar_face_profile(body: &Body, key: FaceKey) -> Option<PlanarFaceProfile> {
    let face = body.faces.get(key)?;
    let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
        return None;
    };
    let tolerance = super::operation_tolerance(&[body]);
    let parts = super::pcurve::face_boundary_parts(body, key, tolerance)?;
    let mut loops = Vec::new();
    for ring in &face.loops {
        let coedges = &body.loops.get(*ring)?.coedges;
        let curves = coedges
            .iter()
            .map(|key| {
                parts
                    .iter()
                    .find(|(candidate, _)| candidate == key)
                    .map(|(_, curve)| curve.clone())
            })
            .collect::<Option<Vec<_>>>()?;
        if curves.is_empty() {
            return None;
        }
        loops.push(curves);
    }
    // A face's loops come in no order; the one enclosing the rest is the
    // one enclosing the most.
    let area =
        |ring: &Vec<Curve>| ring.iter().map(|curve| curve.chain_area()).sum::<f64>().abs();
    let outer = (0..loops.len()).max_by(|a, b| area(&loops[*a]).total_cmp(&area(&loops[*b])));
    if let Some(outer) = outer {
        loops.swap(0, outer);
    }
    let normal = Vec3::from(plane.normal()?);
    Some(PlanarFaceProfile {
        plane: *plane,
        loops,
        outward: (normal * if face.forward { 1.0 } else { -1.0 }).to_array(),
    })
}

/// Finds a face actually containing the pick, never an infinite supporting plane.
/// Picks within `tolerance` of a trimmed boundary are accepted; holes are not.
pub fn planar_face_at_point(body: &Body, point: [f64; 3], tolerance: f64) -> Option<FaceKey> {
    if !tolerance.is_finite() || tolerance < 0.0 || point.iter().any(|v| !v.is_finite()) {
        return None;
    }
    body.face_keys()
        .filter_map(|key| {
            let profile = planar_face_profile(body, key)?;
            let distance = profile.plane.distance_to(point)?.abs();
            if distance > tolerance {
                return None;
            }
            let local = profile.plane.project(point)?;
            let boundary = profile.loops.into_iter().flatten().collect::<Vec<_>>();
            ocs_geom::geom2d::contains(&boundary, local, Tolerance::new(tolerance.max(1e-12)))
                .then_some((key, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(key, _)| key)
}

/// Applies a signed edit along the selected face's outward normal.
/// The original is never changed, including on unsupported geometry or collapse.
pub fn presspull_face(
    body: &Body,
    key: FaceKey,
    distance: f64,
    mode: PresspullMode,
) -> Option<Body> {
    if !distance.is_finite() || distance.abs() <= f64::EPSILON {
        return None;
    }
    let profile = planar_face_profile(body, key)?;
    match mode {
        PresspullMode::Extrude => glued_pull(body, key, &profile, distance)
            .or_else(|| glued_clear_pull(body, key, &profile, distance))
            .or_else(|| sliced_push(body, key, &profile, distance))
            .or_else(|| presspull_region(body, &profile, distance)),
        PresspullMode::Offset => offset_face(body, key, &profile, distance, true),
    }
}

/// An offset by whichever of the ways to make one applies first; `fillets`
/// lets a face rounded into its neighbours be filled back sharp, offset and
/// rounded again.
fn offset_face(
    body: &Body,
    key: FaceKey,
    profile: &PlanarFaceProfile,
    distance: f64,
    fillets: bool,
) -> Option<Body> {
    // Working near the edited face avoids cancellation in intersections of
    // unit-sized solids located far from the world origin.
    let origin = Vec3::from(profile.plane.origin);
    let local = super::transform(body, &Placement::at((-origin).to_array()))?;
    let edited = offset_local(&local, key, distance)
        .and_then(|edited| super::transform(&edited, &Placement::at(origin.to_array())));
    edited
        .or_else(|| upright_offset(body, key, profile, distance))
        .or_else(|| convex_offset(body, key, profile, distance))
        .or_else(|| region_offset(body, key, profile, distance))
        .or_else(|| sliced_offset(body, key, profile, distance))
        .or_else(|| fillets.then(|| filleted_offset(body, key, profile, distance)).flatten())
}

/// A face rounded into neighbouring planes by cylinder fillets: each fillet
/// is filled back to its sharp corner, the face offset there, and the new
/// corners rounded again at the same radii.
fn filleted_offset(
    body: &Body,
    key: FaceKey,
    profile: &PlanarFaceProfile,
    distance: f64,
) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[body]);
    let normal = Vec3::from(profile.outward);
    let ring = *body.faces.get(key)?.loops.first()?;
    let across = |coedge: CoedgeKey, from: FaceKey| -> Option<FaceKey> {
        let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
        edge.coedges.iter().find_map(|other| {
            let face = body.loops.get(body.coedges.get(*other)?.owner)?.owner;
            (face != from).then_some(face)
        })
    };
    let line_of = |coedge: CoedgeKey| -> Option<(Vec3, Vec3)> {
        let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
        let Curve3::Line(_) = body.curves.get(edge.curve)? else {
            return None;
        };
        Some((
            Vec3::from(body.vertices.get(edge.start)?.point),
            Vec3::from(body.vertices.get(edge.end)?.point),
        ))
    };
    let plane_of = |face: FaceKey| -> Option<(Vec3, Vec3)> {
        let node = body.faces.get(face)?;
        let Surface::Plane(plane) = body.surfaces.get(node.surface)? else {
            return None;
        };
        let outward = Vec3::from(plane.normal()?) * if node.forward { 1.0 } else { -1.0 };
        Some((Vec3::from(plane.origin), outward))
    };
    // Each fillet: the body filled to its corner, the plane it rounds into
    // and its radius.
    let mut corners: Vec<(Body, bool, (Vec3, Vec3), f64)> = Vec::new();
    let mut seen = HashSet::new();
    for coedge in body.loops.get(ring)?.coedges.clone() {
        let fillet = across(coedge, key)?;
        let node = body.faces.get(fillet)?;
        let Surface::Cylinder(cylinder) = body.surfaces.get(node.surface)? else {
            continue;
        };
        let axis = Vec3::from(cylinder.base.normal()?);
        if axis.dot(normal).abs() > 1e-9 || !seen.insert(fillet) {
            continue;
        }
        let centre = Vec3::from(cylinder.base.origin);
        let radius = cylinder.radius;
        // Off the axis, at a point along it.
        let radial = |point: Vec3| {
            let along = centre + axis * (point - centre).dot(axis);
            point - along
        };
        let (start, end) = line_of(coedge)?;
        // Tangent to the face along the edge.
        if radial(start).normalize()?.dot(normal).abs() < 1.0 - 1e-6 {
            return None;
        }
        // The fillet's other straight edge and the plane across it.
        let mut far = None;
        for other in body.loops.get(*node.loops.first()?)?.coedges.clone() {
            let Some((a, b)) = line_of(other) else {
                continue;
            };
            let same = |p: Vec3| p.distance(start) <= tolerance || p.distance(end) <= tolerance;
            if same(a) && same(b) {
                continue;
            }
            let Some(next) = across(other, fillet) else {
                continue;
            };
            if next == key {
                continue;
            }
            let (origin, outward) = plane_of(next)?;
            if radial(a).normalize()?.dot(outward).abs() < 1.0 - 1e-6 {
                return None;
            }
            far = Some((a, (origin, outward)));
        }
        let (touch, (origin, outward)) = far?;
        // The cross-section at the edge's start, with the face's normal as x.
        let (low, high) = {
            let (s, e) = ((start - centre).dot(axis), (end - centre).dot(axis));
            (s.min(e), s.max(e))
        };
        let base = centre + axis * low;
        let section = Plane::orthonormal(base.to_array(), normal.to_array(), axis.to_array())?;
        let flat = |point: Vec3| section.project(point.to_array());
        let near_point = flat(start)?;
        let far_point = flat(touch)?;
        // Where the face's trace and the other plane's meet: the corner.
        let corner = {
            let d1 = axis.cross(normal);
            let d2 = axis.cross(outward);
            let (p1, p2) = (start, touch - axis * (touch - start).dot(axis));
            // p1 + t d1 = p2 + u d2, solved in the section.
            let w = p2 - p1;
            let cross = d1.cross(d2).dot(axis);
            if cross.abs() <= 1e-12 {
                return None;
            }
            let t = w.cross(d2).dot(axis) / cross;
            p1 + d1 * t
        };
        let corner_point = flat(corner)?;
        let angle = |p: [f64; 2]| p[1].atan2(p[0]);
        let (mut from, mut to) = (angle(near_point), angle(far_point));
        if (to - from).rem_euclid(std::f64::consts::TAU) > std::f64::consts::PI {
            std::mem::swap(&mut from, &mut to);
        }
        let point_at = |a: f64| [radius * a.cos(), radius * a.sin()];
        let line = |start, end| Curve::Line(Line { start, end });
        let arc = Curve::Arc(Arc {
            centre: [0.0, 0.0],
            radius,
            start_angle: from,
            end_angle: from + (to - from).rem_euclid(std::f64::consts::TAU),
        });
        let loop_curves = vec![
            line(corner_point, point_at(from)),
            arc,
            line(point_at(to), corner_point),
        ];
        let run = (axis * (high - low)).to_array();
        let filled = super::extrude_region(section, &[loop_curves], run)?;
        // A convex fillet took material away from its corner; a concave one
        // added it.
        let chord = (start + (touch - axis * (touch - start).dot(axis))) * 0.5;
        let probe = corner + (chord - corner) * 0.25;
        let convex = super::classify::contains_point(body, probe.to_array(), tolerance)
            == super::Containment::Outside;
        corners.push((filled, convex, (origin, outward), radius));
    }
    if corners.is_empty() {
        return None;
    }
    let mut sharp = body.clone();
    for (filled, convex, _, _) in &corners {
        let how = if *convex { Operation::Union } else { Operation::Difference };
        sharp = super::combine(sharp, filled.clone(), how, tolerance).ok()?;
    }
    // The fill leaves the face and its walls in pieces along the fillets'
    // edges; one plane each again, the offset sees whole walls. Their
    // parameter-space curves, which keep the pieces apart, go: a plane's are
    // its edges laid flat, found again from them.
    let planar: Vec<CoedgeKey> = sharp
        .face_keys()
        .filter(|face| {
            sharp
                .faces
                .get(*face)
                .is_some_and(|node| {
                    matches!(sharp.surfaces.get(node.surface), Some(Surface::Plane(_)))
                })
        })
        .flat_map(|face| sharp.face_coedges(face))
        .collect();
    for coedge in planar {
        if let Some(coedge) = sharp.coedges.get_mut(coedge) {
            coedge.pcurve = None;
        }
    }
    super::boolean::merge_coplanar_pieces(&mut sharp, tolerance);
    let point = super::boolean::interior_point(body, key, tolerance)?;
    let face = planar_face_at_point(&sharp, point, tolerance)?;
    let sharp_profile = planar_face_profile(&sharp, face)?;
    let mut moved = offset_face(&sharp, face, &sharp_profile, distance, false)?;
    // Each new corner, between the moved face and the plane it met, rounded
    // again.
    let level = Vec3::from(profile.plane.origin) + normal * distance;
    for (_, _, (origin, outward), radius) in corners {
        let on = |point: [f64; 3], (at, along): (Vec3, Vec3)| {
            (Vec3::from(point) - at).dot(along).abs() <= tolerance * 100.0
        };
        let edge = moved.edges.iter().find_map(|(edge_key, edge)| {
            let Curve3::Line(_) = moved.curves.get(edge.curve)? else {
                return None;
            };
            let ends = [moved.vertices.get(edge.start)?.point, moved.vertices.get(edge.end)?.point];
            ends.iter()
                .all(|point| on(*point, (level, normal)) && on(*point, (origin, outward)))
                .then_some(edge_key)
        })?;
        moved = super::fillet_edges(&moved, &[edge], radius).ok()?;
    }
    Some(moved)
}

/// An offset of a convex face whose every neighbour round its outside is a
/// plane, and whose holes have upright walls: the slab
/// between the face and where it moves to, on the face's side of each
/// neighbour's plane, is what the edit takes away or adds — each neighbour
/// running on along its own plane, and dropping out where the others close
/// over it. Taken away, the slab reaches a little past the face and its
/// neighbours, so none of it lies on the body's own faces.
fn convex_offset(
    body: &Body,
    key: FaceKey,
    profile: &PlanarFaceProfile,
    distance: f64,
) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[body]);
    let normal = Vec3::from(profile.outward);
    let rings = &body.faces.get(key)?.loops;
    if rings.len() != profile.loops.len() {
        return None;
    }
    let coedges_of = |ring: LoopKey| -> Option<Vec<CoedgeKey>> {
        Some(body.loops.get(ring)?.coedges.clone())
    };
    let neighbour_of = |coedge: CoedgeKey| -> Option<FaceKey> {
        let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
        edge.coedges.iter().find_map(|other| {
            let face = body.loops.get(body.coedges.get(*other)?.owner)?.owner;
            (face != key).then_some(face)
        })
    };
    // A hole's walls must stand square to the face, so the hole runs on
    // through whatever the face moves through.
    for ring in &rings[1..] {
        for coedge in coedges_of(*ring)? {
            let node = body.faces.get(neighbour_of(coedge)?)?;
            let upright = match body.surfaces.get(node.surface)? {
                Surface::Plane(plane) => Vec3::from(plane.normal()?).dot(normal).abs() <= 1e-9,
                Surface::Cylinder(cylinder) => {
                    Vec3::from(cylinder.base.normal()?).dot(normal).abs() >= 1.0 - 1e-9
                }
                _ => false,
            };
            if !upright {
                return None;
            }
        }
    }
    let own: Vec<Vec3> = coedges_of(rings[0])?
        .into_iter()
        .map(|coedge| {
            let (start, _) = body.coedge_vertices(coedge)?;
            Some(Vec3::from(body.vertices.get(start)?.point))
        })
        .collect::<Option<_>>()?;
    let mut sides: Vec<(Vec3, Vec3, bool)> = Vec::new();
    for coedge in coedges_of(rings[0])? {
        let node = body.faces.get(neighbour_of(coedge)?)?;
        let Surface::Plane(plane) = body.surfaces.get(node.surface)? else {
            return None;
        };
        let outward = Vec3::from(plane.normal()?) * if node.forward { 1.0 } else { -1.0 };
        let origin = Vec3::from(plane.origin);
        // The slab keeps to the side of each neighbour's plane the face is on:
        // behind a neighbour it meets at a convex edge, in front of one at a
        // concave edge, a step's riser. A face not wholly on one side is not
        // convex there, and the slab would cut into it.
        let behind = |normal: Vec3| {
            own.iter().all(|point| (*point - origin).dot(normal) <= tolerance)
        };
        let convex = behind(outward);
        let side = if convex {
            outward
        } else if behind(-outward) {
            -outward
        } else {
            return None;
        };
        sides.push((origin, side, convex));
    }
    let lift = tolerance * 1e3;
    let at = |height: f64| Vec3::from(profile.plane.origin) + normal * height;
    let (low, high) = if distance < 0.0 { (distance, lift) } else { (-lift, distance) };
    // The end the slab starts from first: it always bounds it, where the far
    // end may not once the neighbours close over the face.
    let start = if distance < 0.0 { (at(high), normal) } else { (at(low), -normal) };
    let end = if distance < 0.0 { (at(low), -normal) } else { (at(high), normal) };
    let mut planes = vec![start, end];
    // Past a convex edge is outside the body; past a concave one is the
    // neighbour's own material, so the slab stops on its plane there.
    for (origin, side, convex) in sides {
        let grown = if distance < 0.0 && convex { origin + side * lift } else { origin };
        planes.push((grown, side));
    }
    let mut slab = super::blend::bounded_by(&planes, tolerance)?;
    // Each hole, a column through the slab and a little past both ends.
    for ring in &profile.loops[1..] {
        let mut base = profile.plane;
        base.origin = at(low - lift).to_array();
        let reach = normal * (high - low + 2.0 * lift);
        let column = super::extrude_region(base, &[split_closed_curves(ring)], reach.to_array())?;
        slab = super::combine(slab, column, Operation::Difference, tolerance).ok()?;
    }
    let how = if distance < 0.0 { Operation::Difference } else { Operation::Union };
    let edited = super::combine(body.clone(), slab, how, tolerance).ok()?;
    (!edited.roots.is_empty() && edited.validate().is_empty()).then_some(edited)
}

/// A face moved along its normal by the region between where it was and
/// where it goes, added to the body or cut from it: the old face, the face
/// at its new height, and each neighbour's surface carried on between them,
/// its sides running along where it meets the next neighbour. Unlike moving
/// the face's corners along the body's own edges, a corner four or more
/// faces share is left to the boolean, which trims whatever else meets it.
fn region_offset(
    body: &Body,
    key: FaceKey,
    profile: &PlanarFaceProfile,
    distance: f64,
) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[body]);
    let normal = Vec3::from(profile.outward).normalize()?;
    let node = body.faces.get(key)?;
    let Surface::Plane(base) = body.surfaces.get(node.surface)? else {
        return None;
    };
    let mut moved = *base;
    moved.origin = (Vec3::from(base.origin) + normal * distance).to_array();
    let pull = distance > 0.0;
    let mut region = Body::new();
    let lump = region.lumps.insert(super::Lump {
        shells: Vec::new(),
        provenance: super::Provenance::Synthesized,
    });
    let shell = region.shells.insert(super::Shell {
        faces: Vec::new(),
        owner: lump,
        provenance: super::Provenance::Synthesized,
    });
    region.lumps.get_mut(lump)?.shells = vec![shell];
    region.roots = vec![lump];
    let mut corners: HashMap<super::VertexKey, super::VertexKey> = HashMap::new();
    let mut old_rings = Vec::new();
    let mut new_rings = Vec::new();
    for ring in &node.loops {
        // Each use in the loop's direction: the edge, which way, its first
        // corner and the neighbour across it.
        let mut pieces = Vec::new();
        for coedge in &body.loops.get(*ring)?.coedges {
            let use_ = body.coedges.get(*coedge)?;
            let edge = body.edges.get(use_.edge)?;
            let from = if use_.forward { edge.start } else { edge.end };
            let across = edge.coedges.iter().find_map(|other| {
                let face = body.loops.get(body.coedges.get(*other)?.owner)?.owner;
                (face != key).then_some(face)
            })?;
            let neighbour = body.faces.get(across)?;
            pieces.push((use_.edge, use_.forward, from, neighbour.surface, neighbour.forward));
        }
        let count = pieces.len();
        // Neighbours on one surface run on into each other: one side between
        // them, no corner.
        let same = |a: usize, b: usize| -> Option<bool> {
            let (one, other) = (pieces[a].3, pieces[b].3);
            Some(
                one == other
                    || matches!(
                        super::intersect_surfaces(
                            body.surfaces.get(one)?,
                            body.surfaces.get(other)?,
                            tolerance,
                        ),
                        Meeting::Coincident
                    ),
            )
        };
        let mut starts = Vec::new();
        for k in 0..count {
            if !same((k + count - 1) % count, k)? {
                starts.push(k);
            }
        }
        let mut old_uses = Vec::with_capacity(count);
        for (edge_key, forward, ..) in &pieces {
            let edge = body.edges.get(*edge_key)?;
            let curve = region.curves.insert(body.curves.get(edge.curve)?.clone());
            let mut corner = |vertex: super::VertexKey| -> Option<super::VertexKey> {
                if let Some(known) = corners.get(&vertex) {
                    return Some(*known);
                }
                let point = body.vertices.get(vertex)?.point;
                let made = region.vertices.insert(super::Vertex {
                    point,
                    provenance: super::Provenance::Synthesized,
                });
                corners.insert(vertex, made);
                Some(made)
            };
            let (start, end) = (corner(edge.start)?, corner(edge.end)?);
            let copy = region.edges.insert(super::Edge {
                curve,
                start_parameter: edge.start_parameter,
                end_parameter: edge.end_parameter,
                start,
                end,
                coedges: Vec::new(),
                provenance: super::Provenance::Synthesized,
            });
            old_uses.push((copy, *forward));
        }
        // Where each corner goes: along the curve its two neighbours meet
        // in, to the moved plane.
        let mut rails = Vec::with_capacity(starts.len());
        for &k in &starts {
            let before = body.surfaces.get(pieces[(k + count - 1) % count].3)?;
            let after = body.surfaces.get(pieces[k].3)?;
            let point = body.vertices.get(pieces[k].2)?.point;
            let Meeting::Curves(curves) = super::intersect_surfaces(before, after, tolerance)
            else {
                return None;
            };
            let gap = |curve: &Curve3| {
                let on = curve.point_at(curve.parameter_at(point));
                Vec3::from(on).distance(Vec3::from(point))
            };
            let rail = curves
                .into_iter()
                .filter(|curve| gap(curve) <= tolerance * 10.0)
                .min_by(|a, b| gap(a).total_cmp(&gap(b)))?;
            let from = rail.parameter_at(point);
            let to = plane_curve_parameters(&moved, &rail)?
                .into_iter()
                .filter(|t| t.is_finite())
                .min_by(|a, b| (a - from).abs().total_cmp(&(b - from).abs()))?;
            let reached = rail.point_at(to);
            let towards = Vec3::from(reached) - Vec3::from(point);
            if towards.dot(normal) * distance <= 0.0 {
                return None;
            }
            let start = *corners.get(&pieces[k].2)?;
            let end = region.vertices.insert(super::Vertex {
                point: reached,
                provenance: super::Provenance::Synthesized,
            });
            let (edge, forward) = span_edge(&mut region, rail, start, end, towards)?;
            rails.push((edge, forward, end));
        }
        // Each run of neighbours on one surface carried on to where it meets
        // the moved plane, beside its old edges.
        let runs: Vec<(usize, usize)> = if starts.is_empty() {
            vec![(0, count)]
        } else {
            (0..starts.len())
                .map(|index| {
                    let first = starts[index];
                    let next = starts[(index + 1) % starts.len()];
                    (first, (next + count - first - 1) % count + 1)
                })
                .collect()
        };
        let mut new_uses = Vec::with_capacity(runs.len());
        for (index, &(first, length)) in runs.iter().enumerate() {
            let (edge_key, forward, from, surface_key, sense) = pieces[first];
            let edge = body.edges.get(edge_key)?;
            let old = body.curves.get(edge.curve)?;
            let surface = body.surfaces.get(surface_key)?;
            let Meeting::Curves(curves) =
                super::intersect_surfaces(&Surface::Plane(moved), surface, tolerance)
            else {
                return None;
            };
            let at = if forward { edge.start_parameter } else { edge.end_parameter };
            let corner = Vec3::from(body.vertices.get(from)?.point);
            let middle = Vec3::from(old.point_at(0.5 * (edge.start_parameter + edge.end_parameter)))
                + normal * distance;
            let away = |curve: &Curve3| {
                let on = curve.point_at(curve.parameter_at(middle.to_array()));
                Vec3::from(on).distance(middle)
            };
            let section = curves.into_iter().min_by(|a, b| away(a).total_cmp(&away(b)))?;
            let heading = Vec3::from(old.tangent_at(at)) * if forward { 1.0 } else { -1.0 };
            let (start, end) = if starts.is_empty() {
                let point = section.point_at(
                    section.parameter_at((corner + normal * distance).to_array()),
                );
                let only = region.vertices.insert(super::Vertex {
                    point,
                    provenance: super::Provenance::Synthesized,
                });
                (only, only)
            } else {
                (rails[index].2, rails[(index + 1) % rails.len()].2)
            };
            let new = span_edge(&mut region, section, start, end, heading)?;
            new_uses.push(new);
            let olds: Vec<(EdgeKey, bool)> =
                (0..length).map(|step| old_uses[(first + step) % count]).collect();
            let backwards = |uses: &[(EdgeKey, bool)]| -> Vec<(EdgeKey, bool)> {
                uses.iter().rev().map(|(edge, forward)| (*edge, !*forward)).collect()
            };
            let rings = if starts.is_empty() {
                if pull {
                    vec![olds, vec![(new.0, !new.1)]]
                } else {
                    vec![backwards(&olds), vec![new]]
                }
            } else {
                let here = rails[index];
                let next = rails[(index + 1) % rails.len()];
                let mut ring = Vec::with_capacity(length + 3);
                if pull {
                    ring.extend(olds);
                    ring.extend([(next.0, next.1), (new.0, !new.1), (here.0, !here.1)]);
                } else {
                    ring.extend(backwards(&olds));
                    ring.extend([(here.0, here.1), new, (next.0, !next.1)]);
                }
                vec![ring]
            };
            let side = region.surfaces.insert(surface.clone());
            add_face(&mut region, shell, side, sense, rings)?;
        }
        let backwards = |uses: &[(EdgeKey, bool)]| -> Vec<(EdgeKey, bool)> {
            uses.iter().rev().map(|(edge, forward)| (*edge, !*forward)).collect()
        };
        if pull {
            old_rings.push(backwards(&old_uses));
            new_rings.push(new_uses);
        } else {
            old_rings.push(old_uses);
            new_rings.push(backwards(&new_uses));
        }
    }
    let old_surface = region.surfaces.insert(Surface::Plane(*base));
    let new_surface = region.surfaces.insert(Surface::Plane(moved));
    add_face(&mut region, shell, old_surface, node.forward != pull, old_rings)?;
    let top = add_face(&mut region, shell, new_surface, node.forward == pull, new_rings)?;
    // The moved face winds as the old one did, or the walls crossed over
    // on the way and the region turned inside out.
    let area = |body: &Body, face| {
        Some(boundary_area(&super::pcurve::face_boundary(body, face, tolerance)?))
    };
    let (before, after) = (area(body, key)?, area(&region, top)?);
    let turned = if pull { after } else { -after };
    let flat = after.abs() <= tolerance * tolerance;
    if before * turned <= 0.0 || flat || !region.validate().is_empty() {
        return None;
    }
    let how = if pull { Operation::Union } else { Operation::Difference };
    let edited = super::combine(body.clone(), region, how, tolerance).ok()?;
    (!edited.roots.is_empty() && edited.validate().is_empty()).then_some(edited)
}

/// An edge along `curve` from `start` to `end` the way `heading` points at
/// the start, and whether that runs with the edge.
fn span_edge(
    body: &mut Body,
    curve: Curve3,
    start: super::VertexKey,
    end: super::VertexKey,
    heading: Vec3,
) -> Option<(EdgeKey, bool)> {
    let (a, b) = (body.vertices.get(start)?.point, body.vertices.get(end)?.point);
    let from = curve.parameter_at(a);
    let mut to = curve.parameter_at(b);
    let up = Vec3::from(curve.tangent_at(from)).dot(heading) >= 0.0;
    if let Curve3::Circle(_) | Curve3::Ellipse(_) = curve {
        // Round the closed curve the way it heads, a whole turn back to a
        // start it ends at.
        if up {
            to = from + (to - from).rem_euclid(TAU);
            if to - from <= 1e-12 {
                to += TAU;
            }
        } else {
            to = from - (from - to).rem_euclid(TAU);
            if from - to <= 1e-12 {
                to -= TAU;
            }
        }
    } else if (to > from) != up {
        return None;
    }
    let curve = body.curves.insert(curve);
    let (low, high, first, last) =
        if to >= from { (from, to, start, end) } else { (to, from, end, start) };
    let edge = body.edges.insert(super::Edge {
        curve,
        start_parameter: low,
        end_parameter: high,
        start: first,
        end: last,
        coedges: Vec::new(),
        provenance: super::Provenance::Synthesized,
    });
    Some((edge, to >= from))
}

/// A face of `surface` bounded by `rings`, each a run of edge uses.
fn add_face(
    body: &mut Body,
    shell: super::ShellKey,
    surface: super::SurfaceKey,
    forward: bool,
    rings: Vec<Vec<(EdgeKey, bool)>>,
) -> Option<FaceKey> {
    let face = body.faces.insert(super::Face {
        surface,
        forward,
        loops: Vec::new(),
        owner: shell,
        provenance: super::Provenance::Synthesized,
    });
    for uses in rings {
        let ring = body.loops.insert(super::Loop {
            coedges: Vec::new(),
            owner: face,
            provenance: super::Provenance::Synthesized,
        });
        for (edge, forward) in uses {
            let coedge = body.coedges.insert(super::Coedge {
                edge,
                forward,
                pcurve: None,
                owner: ring,
                provenance: super::Provenance::Synthesized,
            });
            body.edges.get_mut(edge)?.coedges.push(coedge);
            body.loops.get_mut(ring)?.coedges.push(coedge);
        }
        body.faces.get_mut(face)?.loops.push(ring);
    }
    body.shells.get_mut(shell)?.faces.push(face);
    Some(face)
}

/// An offset among walls that all stand square to the face is the same edit
/// as the extrusion: each wall runs on along its own surface either way. It
/// is taken only where the volume moved is exactly the face's area over the
/// distance — where the extrusion reached nothing else on its way.
fn upright_offset(
    body: &Body,
    key: FaceKey,
    profile: &PlanarFaceProfile,
    distance: f64,
) -> Option<Body> {
    let normal = Vec3::from(profile.outward);
    for coedge in body.face_coedges(key) {
        let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
        let upright = match adjacent_surface(body, edge, key)? {
            Surface::Plane(plane) => Vec3::from(plane.normal()?).dot(normal).abs() <= 1e-9,
            Surface::Cylinder(cylinder) => {
                Vec3::from(cylinder.base.normal()?).dot(normal).abs() >= 1.0 - 1e-9
            }
            // A spline wall stands square where its normal does, all along
            // the edge.
            wall @ (Surface::Nurbs(_) | Surface::Offset(_)) => {
                let curve = body.curves.get(edge.curve)?;
                (0..=8).all(|step| {
                    let t = step as f64 / 8.0;
                    let at = edge.start_parameter * (1.0 - t) + edge.end_parameter * t;
                    wall.parameters_at(curve.point_at(at))
                        .and_then(|(u, v)| wall.normal_at(u, v))
                        .is_some_and(|across| Vec3::from(across).dot(normal).abs() <= 1e-6)
                })
            }
            _ => false,
        };
        if !upright {
            return None;
        }
    }
    let tolerance = super::operation_tolerance(&[body]);
    let area = boundary_area(&super::pcurve::face_boundary(body, key, tolerance)?).abs();
    let result = presspull_face(body, key, distance, PresspullMode::Extrude)?;
    let volume = |body: &Body| super::mass_properties(body).map(|mass| mass.volume);
    let moved = volume(&result)? - volume(body)?;
    ((moved - area * distance).abs() <= 1e-3 * (area * distance).abs()).then_some(result)
}

/// A whole face pushed in among walls standing square to it is the body cut
/// across at the new depth, the part behind the cut kept: no boolean has to
/// run a wall of the prism along a wall of the body, which a spline wall
/// refuses. Taken only where what is cut off is the face's area over the
/// distance — the walls run straight in that far and nothing else of the
/// body lies there.
fn sliced_push(
    body: &Body,
    key: FaceKey,
    region: &PlanarFaceProfile,
    distance: f64,
) -> Option<Body> {
    if distance >= 0.0 {
        return None;
    }
    let normal = Vec3::from(region.outward).normalize()?;
    let origin = Vec3::from(region.plane.origin) + normal * distance;
    let plane = Plane::orthonormal(origin.to_array(), region.plane.x_axis, normal.to_array())?;
    let cut = super::slice_by_plane(body, plane).ok()??;
    let tolerance = super::operation_tolerance(&[body]);
    let area = boundary_area(&super::pcurve::face_boundary(body, key, tolerance)?).abs();
    let removed = super::mass_properties(&cut.positive)?.volume;
    let expected = area * distance.abs();
    let kept = cut.negative;
    ((removed - expected).abs() <= 1e-4 * expected
        && !kept.roots.is_empty()
        && kept.validate().is_empty())
    .then_some(kept)
}

/// A face offset into the body by slicing it off at the moved plane, where
/// all the slice takes away is the face, its plane and the neighbours
/// running down to it: those neighbours trimmed short is the offset, however
/// they lean. Anything else in the slab — a step, a far wall — means the
/// slice is cutting more than the face's own neighbourhood.
fn sliced_offset(
    body: &Body,
    key: FaceKey,
    region: &PlanarFaceProfile,
    distance: f64,
) -> Option<Body> {
    if distance >= 0.0 {
        return None;
    }
    let normal = Vec3::from(region.outward).normalize()?;
    let origin = Vec3::from(region.plane.origin) + normal * distance;
    let plane = Plane::orthonormal(origin.to_array(), region.plane.x_axis, normal.to_array())?;
    let cut = super::slice_by_plane(body, plane).ok()??;
    let tolerance = super::operation_tolerance(&[body]);
    let mut near = vec![key];
    for ring in &body.faces.get(key)?.loops {
        for coedge in &body.loops.get(*ring)?.coedges {
            for other in &body.edges.get(body.coedges.get(*coedge)?.edge)?.coedges {
                near.push(body.loops.get(body.coedges.get(*other)?.owner)?.owner);
            }
        }
    }
    let removed = &cut.positive;
    let slack = tolerance * 10.0;
    for face in removed.face_keys() {
        let point = super::boolean::interior_point(removed, face, tolerance)?;
        let on_plane = plane.distance_to(point)?.abs() <= slack;
        let on_near = near.iter().any(|f| {
            super::classify::face_distance(body, *f, point, tolerance)
                .is_some_and(|gap| gap <= slack)
        });
        if !on_plane && !on_near {
            return None;
        }
    }
    let kept = cut.negative;
    (!kept.roots.is_empty() && kept.validate().is_empty()).then_some(kept)
}

/// A whole face pulled out of a body lying wholly behind it: the face gives
/// way to the prism's walls and top, joined along its own edges. A union
/// reached the same through each wall meeting its own extension — tangent
/// all along the rim, where tracing the meeting gives out (a spline wall
/// pulled up). `None` where the body reaches past the face's plane and the
/// prism might run into it.
fn glued_pull(
    body: &Body,
    key: FaceKey,
    region: &PlanarFaceProfile,
    distance: f64,
) -> Option<Body> {
    glued(body, key, region, distance, false)
}

/// [`glued_pull`] for a body not wholly behind the face, where the prism
/// the face is pulled through, started a little off the face, meets none of
/// it: the walls and top still join the body along the face's edges alone.
fn glued_clear_pull(
    body: &Body,
    key: FaceKey,
    region: &PlanarFaceProfile,
    distance: f64,
) -> Option<Body> {
    if distance <= 0.0 {
        return None;
    }
    let normal = Vec3::from(region.outward).normalize()?;
    let origin = Vec3::from(region.plane.origin);
    let local = super::transform(body, &Placement::at((-origin).to_array()))?;
    let tolerance = super::operation_tolerance(&[&local]);
    let lift = tolerance * 1e3;
    let mut plane = region.plane;
    plane.origin = (normal * lift).to_array();
    let loops: Vec<Vec<Curve>> =
        region.loops.iter().map(|ring| split_closed_curves(ring)).collect();
    let tool = super::extrude_region(plane, &loops, (normal * (distance - lift)).to_array())?;
    let tolerance = tolerance.max(super::operation_tolerance(&[&tool]));
    let met = super::combine(local, tool, Operation::Intersection, tolerance).ok()?;
    if !met.faces.is_empty() {
        return None;
    }
    glued(body, key, region, distance, true)
}

fn glued(
    body: &Body,
    key: FaceKey,
    region: &PlanarFaceProfile,
    distance: f64,
    clear: bool,
) -> Option<Body> {
    if distance <= 0.0 {
        return None;
    }
    let normal = Vec3::from(region.outward).normalize()?;
    let origin = Vec3::from(region.plane.origin);
    let mut local = super::transform(body, &Placement::at((-origin).to_array()))?;
    let tolerance = super::operation_tolerance(&[&local]);
    // Behind the plane by every edge and every curved face's box.
    let behind = |point: [f64; 3]| Vec3::from(point).dot(normal) <= tolerance;
    let edges_behind = local.edges.iter().all(|(_, edge)| {
        local.curves.get(edge.curve).is_some_and(|curve| {
            (0..=16).all(|step| {
                let t = step as f64 / 16.0;
                behind(curve.point_at(edge.start_parameter * (1.0 - t) + edge.end_parameter * t))
            })
        })
    });
    let fit = tolerance.max(local.worst_vertex_gap() * 2.0);
    let faces_behind = local.face_keys().all(|face| {
        match local.faces.get(face).and_then(|node| local.surfaces.get(node.surface)) {
            // Ruled by straight lines, each running from edge to edge of
            // the face: how far out a ruling reaches is linear along it, so
            // the face is furthest out on its edges, already behind.
            Some(Surface::Plane(_) | Surface::Cylinder(_)) => true,
            // A cone's rulings may run to its apex instead, which no edge
            // marks: behind as well, unless the axis is square to the face
            // and the rims are furthest out whatever the apex does.
            Some(Surface::Cone(cone)) => cone.base.normal().is_some_and(|axis| {
                let axis = Vec3::from(axis);
                axis.dot(normal).abs() >= 1.0 - 1e-9 || {
                    let slope = cone.half_angle.tan();
                    slope.abs() <= f64::EPSILON
                        || behind(
                            (Vec3::from(cone.base.origin) + axis * (cone.radius / slope))
                                .to_array(),
                        )
                }
            }),
            // A spline lies within its control net, which a fitted wall
            // reaches past its edges by no more than the fit.
            Some(Surface::Nurbs(spline)) => spline
                .control_points()
                .iter()
                .flatten()
                .all(|point| Vec3::from(*point).dot(normal) <= fit),
            _ => super::face_bounds(&local, face).is_some_and(|bounds| {
                (0..8).all(|bits| {
                    behind(std::array::from_fn(|axis| match bits >> axis & 1 {
                        0 => bounds.min[axis],
                        _ => bounds.max[axis],
                    }))
                })
            }),
        }
    });
    if !clear && (!edges_behind || !faces_behind) {
        return None;
    }
    let mut plane = region.plane;
    plane.origin = [0.0; 3];
    let loops = region
        .loops
        .iter()
        .map(|ring| split_closed_curves(ring))
        .collect::<Vec<_>>();
    let mut tool = super::extrude_region(plane, &loops, (normal * distance).to_array())?;
    // The prism's base runs its curves in pieces; the face's own edges are
    // cut at the same corners so the two meet edge for edge.
    super::imprint::align_edge_vertices(&tool, &mut local, tolerance);
    super::imprint::align_edge_vertices(&local, &mut tool, tolerance);
    let on_base = |face: FaceKey| {
        tool.face_coedges(face).iter().all(|coedge| {
            tool.coedge_vertices(*coedge)
                .and_then(|(from, _)| tool.vertices.get(from))
                .is_some_and(|vertex| Vec3::from(vertex.point).dot(normal).abs() <= tolerance)
        })
    };
    let mut result = Body::new();
    let lump = result.lumps.insert(super::Lump {
        shells: Vec::new(),
        provenance: super::Provenance::Synthesized,
    });
    let shell = result.shells.insert(super::Shell {
        faces: Vec::new(),
        owner: lump,
        provenance: super::Provenance::Synthesized,
    });
    result.lumps.get_mut(lump)?.shells = vec![shell];
    result.roots = vec![lump];
    // Joined to within the body's own fit: a file's corners sit a fit off
    // their curves, and the prism's base starts where the curves do.
    let glue = fit;
    let copy = |result: &mut Body, source: &Body, face: FaceKey| {
        super::boolean::copy_face_with_tolerance(result, source, face, shell, false, glue).ok()
    };
    for face in local.face_keys().filter(|face| *face != key) {
        copy(&mut result, &local, face)?;
    }
    for face in tool.face_keys().filter(|face| !on_base(*face)) {
        copy(&mut result, &tool, face)?;
    }
    super::boolean::orient_shell(&mut result).ok()?;
    if result.edges.iter().any(|(_, edge)| edge.coedges.len() != 2)
        || !result.validate().is_empty()
    {
        return None;
    }
    super::boolean::merge_coplanar_faces(&mut result, tolerance);
    super::transform(&result, &Placement::at(origin.to_array()))
}

/// Adds an outward bounded-region extrusion, or removes an inward extrusion.
/// `region.outward` must be a unit normal pointing out of the hosting face.
pub fn presspull_region(body: &Body, region: &PlanarFaceProfile, distance: f64) -> Option<Body> {
    if !distance.is_finite() || distance.abs() <= f64::EPSILON || region.loops.is_empty() {
        return None;
    }
    let normal = Vec3::from(region.outward).normalize()?;
    if normal.dot(Vec3::from(region.plane.normal()?)).abs() < 1.0 - 1e-9 {
        return None;
    }
    let origin = Vec3::from(region.plane.origin);
    let local = super::transform(body, &Placement::at((-origin).to_array()))?;
    let mut plane = region.plane;
    plane.origin = [0.0; 3];
    let loops = region
        .loops
        .iter()
        .map(|ring| split_closed_curves(ring))
        .collect::<Vec<_>>();
    let floor = f64::EPSILON * origin.length().max(1.0) * 64.0;
    let attempt = |lift: f64| -> Option<Body> {
        let mut plane = plane;
        plane.origin = (normal * lift).to_array();
        let tool = super::extrude_region(plane, &loops, (normal * (distance - lift)).to_array())?;
        let tolerance = super::operation_tolerance(&[&local, &tool]).max(floor);
        let how = if distance > 0.0 { Operation::Union } else { Operation::Difference };
        let edited = super::combine(local.clone(), tool, how, tolerance).ok()?;
        (!edited.roots.is_empty() && edited.validate().is_empty()).then_some(edited)
    };
    // A pushed face whose tool on its own plane reads as one face on both
    // bodies' sides goes again with the tool started a little outside it:
    // outside the face is outside the body. (Sunk under a pulled face the
    // same way, the tool's sides leave slivers along the walls.)
    let lift = super::operation_tolerance(&[&local]).max(floor) * 1e3;
    let edited = attempt(0.0).or_else(|| (distance < 0.0).then(|| attempt(lift)).flatten())?;
    super::transform(&edited, &Placement::at(origin.to_array()))
}

/// Builds one bounded planar sheet face, including exact curved inner loops.
pub fn planar_region(plane: Plane, loops: &[Vec<Curve>]) -> Option<Body> {
    let loops = loops
        .iter()
        .map(|ring| split_closed_curves(ring))
        .collect::<Vec<_>>();
    let solid = super::extrude_region(plane, &loops, plane.normal()?)?;
    let face = solid.face_keys().find(|key| {
        planar_face_profile(&solid, *key).is_some_and(|profile| {
            plane
                .distance_to(profile.plane.origin)
                .is_some_and(|gap| gap.abs() < 1e-9)
                && Vec3::from(profile.outward).dot(Vec3::from(plane.normal().unwrap())) < 0.0
        })
    })?;
    let mut result = Body::new();
    let lump = result.lumps.insert(super::Lump {
        shells: Vec::new(),
        provenance: super::Provenance::Synthesized,
    });
    let shell = result.shells.insert(super::Shell {
        faces: Vec::new(),
        owner: lump,
        provenance: super::Provenance::Synthesized,
    });
    result.lumps.get_mut(lump)?.shells.push(shell);
    result.roots.push(lump);
    super::boolean::copy_face(&mut result, &solid, face, shell, true).ok()?;
    result.validate().is_empty().then_some(result)
}

/// Unites coplanar bounded sheets while preserving exact curved boundaries.
///
/// The sheets are lifted into equal-depth temporary solids so the regular
/// Boolean owns all overlap and hole decisions. The bottom caps of the
/// result are then copied back into one open sheet body.
pub fn union_planar_regions(bodies: &[Body], tolerance: f64) -> Result<Body, super::Snag> {
    if bodies.len() < 2 || !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(super::Snag::CutRefused);
    }

    planar_regions_boolean(bodies, &[], Operation::Union, tolerance)
}

/// Intersects coplanar bounded sheets while preserving exact curved boundaries.
pub fn intersect_planar_regions(
    bodies: &[Body],
    tolerance: f64,
) -> Result<PlanarIntersection, super::Snag> {
    if bodies.len() < 2 || !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(super::Snag::CutRefused);
    }

    let profiles = bodies
        .iter()
        .map(|body| {
            body.face_keys()
                .map(|face| planar_face_profile(body, face).ok_or(super::Snag::NoClosedForm))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let base = profiles
        .first()
        .and_then(|group| group.first())
        .ok_or(super::Snag::CutRefused)?
        .plane;
    let normal = Vec3::from(base.normal().ok_or(super::Snag::CutRefused)?);
    let profiles = planar_intersection_profiles(&profiles, &base, normal, tolerance)?;

    let mut solids = profiles
        .iter()
        .map(|group| planar_intersection_profile_solid(group, &base, normal, tolerance))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let mut result = solids.next().ok_or(super::Snag::CutRefused)?;
    for solid in solids {
        result = super::combine(result, solid, Operation::Intersection, tolerance)?;
        if result.faces.is_empty() {
            return Ok(if planar_profiles_share_point(&profiles, tolerance) {
                PlanarIntersection::Touching
            } else {
                PlanarIntersection::Disjoint
            });
        }
    }

    Ok(PlanarIntersection::Area(planar_bottom_sheets(
        &result, &base, normal, tolerance,
    )?))
}

fn planar_intersection_profiles(
    profiles: &[Vec<PlanarFaceProfile>],
    base: &Plane,
    normal: Vec3,
    tolerance: f64,
) -> Result<Vec<Vec<Vec<Vec<Curve>>>>, super::Snag> {
    profiles
        .iter()
        .map(|group| {
            if group.is_empty() {
                return Err(super::Snag::CutRefused);
            }
            group
                .iter()
                .map(|profile| {
                    let profile_normal =
                        Vec3::from(profile.plane.normal().ok_or(super::Snag::CutRefused)?);
                    if normal.dot(profile_normal).abs() < 1.0 - 1e-9
                        || base
                            .distance_to(profile.plane.origin)
                            .is_none_or(|distance| distance.abs() > tolerance)
                    {
                        return Err(super::Snag::NoClosedForm);
                    }
                    let transform =
                        plane_transform(base, &profile.plane).ok_or(super::Snag::CutRefused)?;
                    profile
                        .loops
                        .iter()
                        .map(|ring| {
                            ring.iter()
                                .map(|curve| {
                                    curve.transformed(&transform).ok_or(super::Snag::CutRefused)
                                })
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect()
}

fn planar_intersection_profile_solid(
    profiles: &[Vec<Vec<Curve>>],
    base: &Plane,
    normal: Vec3,
    tolerance: f64,
) -> Result<Body, super::Snag> {
    let mut solids = profiles
        .iter()
        .map(|loops| {
            super::extrude_region(*base, loops, normal.to_array()).ok_or(super::Snag::CutRefused)
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let mut result = solids.next().ok_or(super::Snag::CutRefused)?;
    for solid in solids {
        result = super::combine(result, solid, Operation::Union, tolerance)?;
    }
    Ok(result)
}

/// Subtracts coplanar bounded sheets while preserving exact curved boundaries.
///
/// Every base sheet is united first, every cutter sheet is united second, and
/// the two temporary solids are differenced. The surviving bottom caps are
/// copied back into a bounded open sheet body. A fully consumed base is a
/// successful empty body, not an operation failure.
pub fn subtract_planar_regions(
    bases: &[Body],
    cutters: &[Body],
    tolerance: f64,
) -> Result<Body, super::Snag> {
    if bases.is_empty() || cutters.is_empty() || !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(super::Snag::CutRefused);
    }

    planar_regions_boolean(bases, cutters, Operation::Difference, tolerance)
}

fn planar_regions_boolean(
    bases: &[Body],
    cutters: &[Body],
    operation: Operation,
    tolerance: f64,
) -> Result<Body, super::Snag> {
    let bodies = bases.iter().chain(cutters);

    let profiles = bodies
        .flat_map(|body| body.face_keys().map(move |face| (body, face)))
        .map(|(body, face)| planar_face_profile(body, face).ok_or(super::Snag::NoClosedForm))
        .collect::<Result<Vec<_>, _>>()?;
    let base = profiles.first().ok_or(super::Snag::CutRefused)?.plane;
    let normal = Vec3::from(base.normal().ok_or(super::Snag::CutRefused)?);
    let base_count = bases
        .iter()
        .map(|body| body.face_keys().count())
        .sum::<usize>();
    let (base_profiles, cutter_profiles) = profiles.split_at(base_count);
    let mut result = unite_planar_profile_solids(base_profiles, &base, normal, tolerance)?;
    if operation == Operation::Difference {
        let cutters = unite_planar_profile_solids(cutter_profiles, &base, normal, tolerance)?;
        result = super::combine(result, cutters, Operation::Difference, tolerance)?;
    }
    if result.faces.is_empty() {
        return Ok(Body::new());
    }

    planar_bottom_sheets(&result, &base, normal, tolerance)
}

fn unite_planar_profile_solids(
    profiles: &[PlanarFaceProfile],
    base: &Plane,
    normal: Vec3,
    tolerance: f64,
) -> Result<Body, super::Snag> {
    let mut solids = Vec::with_capacity(profiles.len());
    for profile in profiles {
        let profile_normal = Vec3::from(profile.plane.normal().ok_or(super::Snag::CutRefused)?);
        if normal.dot(profile_normal).abs() < 1.0 - 1e-9
            || base
                .distance_to(profile.plane.origin)
                .is_none_or(|distance| distance.abs() > tolerance)
        {
            return Err(super::Snag::NoClosedForm);
        }
        let transform = plane_transform(base, &profile.plane).ok_or(super::Snag::CutRefused)?;
        let loops = profile
            .loops
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|curve| curve.transformed(&transform).ok_or(super::Snag::CutRefused))
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        solids.push(
            super::extrude_region(*base, &loops, normal.to_array())
                .ok_or(super::Snag::CutRefused)?,
        );
    }
    let mut solids = solids.into_iter();
    let mut united = solids.next().ok_or(super::Snag::CutRefused)?;
    for solid in solids {
        united = super::combine(united, solid, Operation::Union, tolerance)?;
    }
    Ok(united)
}

fn planar_profiles_share_point(profiles: &[Vec<Vec<Vec<Curve>>>], tolerance: f64) -> bool {
    let tolerance = Tolerance::new(tolerance);
    let boundary = |group: &Vec<Vec<Vec<Curve>>>| {
        group
            .iter()
            .flat_map(|profile| profile.iter())
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
    };
    let boundaries = profiles.iter().map(boundary).collect::<Vec<_>>();
    let Some(first) = boundaries.first() else {
        return false;
    };
    let mut candidates = Vec::new();
    for curve in first {
        candidates.push(curve.point_at(0.0));
        candidates.push(curve.point_at(1.0));
    }
    for other in boundaries.iter().skip(1) {
        for a in first {
            for b in other {
                candidates.extend(
                    ocs_geom::geom2d::intersect(a, b, tolerance)
                        .into_iter()
                        .map(|crossing| crossing.point),
                );
                for point in [a.point_at(0.0), a.point_at(1.0)] {
                    if ocs_geom::geom2d::distance_to(b, point) <= tolerance.linear() {
                        candidates.push(point);
                    }
                }
                for point in [b.point_at(0.0), b.point_at(1.0)] {
                    if ocs_geom::geom2d::distance_to(a, point) <= tolerance.linear() {
                        candidates.push(point);
                    }
                }
            }
        }
    }
    candidates.into_iter().any(|point| {
        boundaries
            .iter()
            .all(|curves| ocs_geom::geom2d::contains(curves, point, tolerance))
    })
}

fn planar_bottom_sheets(
    body: &Body,
    base: &Plane,
    normal: Vec3,
    tolerance: f64,
) -> Result<Body, super::Snag> {
    let bottom = body
        .face_keys()
        .filter(|face| {
            planar_face_profile(body, *face).is_some_and(|profile| {
                base.distance_to(profile.plane.origin)
                    .is_some_and(|distance| distance.abs() <= tolerance * 4.0)
                    && Vec3::from(profile.outward).dot(normal) < -1.0 + 1e-9
            })
        })
        .collect::<Vec<_>>();
    if bottom.is_empty() {
        return Err(super::Snag::CutRefused);
    }

    let components = super::sweep::face_components(body, &bottom).ok_or(super::Snag::CutRefused)?;
    let mut result = Body::new();
    for component in components {
        let loops = component_boundary_loops(body, &component, base, tolerance)
            .ok_or(super::Snag::CutRefused)?;
        let sheet = planar_region(*base, &loops).ok_or(super::Snag::CutRefused)?;
        let lump = result.lumps.insert(super::Lump {
            shells: Vec::new(),
            provenance: super::Provenance::Synthesized,
        });
        let shell = result.shells.insert(super::Shell {
            faces: Vec::new(),
            owner: lump,
            provenance: super::Provenance::Synthesized,
        });
        result
            .lumps
            .get_mut(lump)
            .ok_or(super::Snag::CutRefused)?
            .shells
            .push(shell);
        result.roots.push(lump);
        for face in sheet.face_keys() {
            super::boolean::copy_face(&mut result, &sheet, face, shell, false)?;
        }
    }
    if result.validate().is_empty() {
        Ok(result)
    } else {
        Err(super::Snag::CutRefused)
    }
}

fn plane_transform(base: &Plane, source: &Plane) -> Option<Transform> {
    Some(Transform {
        origin: base.project(source.origin)?.into(),
        x_axis: base.project_vector(source.x_axis)?.into(),
        y_axis: base.project_vector(source.y_axis)?.into(),
    })
}

fn component_boundary_loops(
    body: &Body,
    faces: &[FaceKey],
    base: &Plane,
    tolerance: f64,
) -> Option<Vec<Vec<Curve>>> {
    let face_set = faces.iter().copied().collect::<HashSet<_>>();
    let mut pending = Vec::new();
    for face in faces {
        let node = body.faces.get(*face)?;
        let Surface::Plane(plane) = body.surfaces.get(node.surface)? else {
            return None;
        };
        let transform = plane_transform(base, plane)?;
        for (coedge, curve) in super::pcurve::face_boundary_parts(body, *face, tolerance)? {
            let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
            let internal = edge.coedges.iter().any(|candidate| {
                if *candidate == coedge {
                    return false;
                }
                body.coedges
                    .get(*candidate)
                    .and_then(|node| body.loops.get(node.owner))
                    .is_some_and(|ring| face_set.contains(&ring.owner))
            });
            if !internal {
                pending.push(curve.transformed(&transform)?);
            }
        }
    }

    let mut loops = Vec::new();
    while !pending.is_empty() {
        let order = closed_curve_order(&pending, tolerance)?;
        let ring = order
            .iter()
            .map(|(index, forward)| {
                if *forward {
                    Some(pending[*index].clone())
                } else {
                    reversed_curve(&pending[*index])
                }
            })
            .collect::<Option<Vec<_>>>()?;
        let mut remove = order
            .into_iter()
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        remove.sort_unstable();
        for index in remove.into_iter().rev() {
            pending.remove(index);
        }
        loops.push(ring);
    }
    loops.sort_by(|a, b| boundary_area(b).abs().total_cmp(&boundary_area(a).abs()));
    Some(loops)
}

fn reversed_curve(curve: &Curve) -> Option<Curve> {
    match curve {
        Curve::Line(line) => Some(Curve::Line(Line {
            start: line.end,
            end: line.start,
        })),
        Curve::Polyline(polyline) => {
            let count = polyline.vertices.len();
            let vertices = (0..count)
                .rev()
                .map(|index| {
                    let bulge = if index > 0 {
                        -polyline.vertices[index - 1].bulge
                    } else if polyline.closed && count > 0 {
                        -polyline.vertices[count - 1].bulge
                    } else {
                        0.0
                    };
                    PolylineVertex {
                        position: polyline.vertices[index].position,
                        bulge,
                    }
                })
                .collect();
            Some(Curve::Polyline(Polyline {
                vertices,
                closed: polyline.closed,
            }))
        }
        Curve::Nurbs(curve) => Some(Curve::Nurbs(curve.reversed())),
        Curve::Circle(_) | Curve::Arc(_) | Curve::Ellipse(_) => Some(Curve::Nurbs(
            RationalCurve2::from_curve(curve)?.reversed().curve()?,
        )),
        Curve::Ray(_) | Curve::XLine(_) => None,
    }
}

fn closed_curve_order(curves: &[Curve], tolerance: f64) -> Option<Vec<(usize, bool)>> {
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= tolerance * 4.0;
    [true, false].into_iter().find_map(|first_forward| {
        let first = curves.first()?;
        let start = first.point_at(if first_forward { 0.0 } else { 1.0 });
        let mut head = first.point_at(if first_forward { 1.0 } else { 0.0 });
        let mut used = vec![false; curves.len()];
        used[0] = true;
        let mut order = vec![(0, first_forward)];
        while !near(head, start) {
            let (next, forward) = curves.iter().enumerate().find_map(|(index, curve)| {
                if used[index] {
                    return None;
                }
                if near(head, curve.point_at(0.0)) {
                    Some((index, true))
                } else if near(head, curve.point_at(1.0)) {
                    Some((index, false))
                } else {
                    None
                }
            })?;
            used[next] = true;
            order.push((next, forward));
            head = curves[next].point_at(if forward { 1.0 } else { 0.0 });
        }
        Some(order)
    })
}

/// A spline that is exactly a circle or an arc of one, as that: a loop
/// running clockwise is read off a face as a spline (an arc only turns
/// counter-clockwise), and extruded as one its wall became a spline sheet
/// that no closed form meets.
fn circular_piece(curve: &Curve) -> Option<Curve> {
    let Curve::Nurbs(_) = curve else {
        return None;
    };
    let at = |t: f64| Vec3::new(curve.point_at(t)[0], curve.point_at(t)[1], 0.0);
    let (a, b, c) = (at(0.0), at(1.0 / 3.0), at(2.0 / 3.0));
    let end = at(1.0);
    // Measured against the piece itself, not its radius: a straight piece's
    // radius runs to 1e16, and a millionth of that took it for a whole
    // circle — one quarter of it reaching across the world.
    let size = [b, c, end].iter().map(|point| point.distance(a)).fold(0.0, f64::max);
    let fit = size * 1e-9;
    if (0..=32).all(|step| at(step as f64 / 32.0).distance_to_segment(a, end) <= fit) {
        return (a.distance(end) > fit)
            .then_some(Curve::Line(Line { start: [a.x, a.y], end: [end.x, end.y] }));
    }
    // The centre is where the perpendicular bisectors of ab and bc meet.
    let (ab, bc) = (b - a, c - b);
    let det = 2.0 * (ab.x * bc.y - ab.y * bc.x);
    if det.abs() <= f64::EPSILON * ab.length() * bc.length() {
        return None;
    }
    let (ka, kb) = (ab.dot(a + b), bc.dot(b + c));
    let centre = Vec3::new((ka * bc.y - kb * ab.y) / det, (kb * ab.x - ka * bc.x) / det, 0.0);
    let radius = a.distance(centre);
    let round = (0..=32).all(|step| {
        (at(step as f64 / 32.0).distance(centre) - radius).abs() <= fit.max(radius * 1e-12)
    });
    if !round {
        return None;
    }
    let angle = |p: Vec3| (p.y - centre.y).atan2(p.x - centre.x);
    if end.distance(a) <= fit {
        return Some(Curve::Circle(ocs_geom::geom2d::Circle {
            centre: [centre.x, centre.y],
            radius,
        }));
    }
    // Which way it turns: the arc from start to end counter-clockwise
    // passes through the middle, or the curve runs the other way.
    let (start, end, middle) = (angle(a), angle(end), angle(at(0.5)));
    let turning = (middle - start).rem_euclid(TAU) < (end - start).rem_euclid(TAU);
    let (from, to) = if turning { (start, end) } else { (end, start) };
    Some(Curve::Arc(Arc {
        centre: [centre.x, centre.y],
        radius,
        start_angle: from,
        end_angle: from + (to - from).rem_euclid(TAU),
    }))
}

/// Splits complete conics into exact bounded pieces for extrusion builders.
pub fn extrusion_profile_pieces(ring: &[Curve]) -> Vec<Curve> {
    ring.iter()
        .map(|curve| circular_piece(curve).unwrap_or_else(|| curve.clone()))
        .collect::<Vec<_>>()
        .iter()
        .flat_map(|curve| match curve {
            Curve::Circle(circle) => (0..4)
                .map(|i| {
                    Curve::Arc(Arc {
                        centre: circle.centre,
                        radius: circle.radius,
                        start_angle: i as f64 * FRAC_PI_2,
                        end_angle: (i + 1) as f64 * FRAC_PI_2,
                    })
                })
                .collect(),
            Curve::Arc(arc) if arc.sweep() >= TAU - 1e-12 => (0..4)
                .map(|i| {
                    Curve::Arc(Arc {
                        centre: arc.centre,
                        radius: arc.radius,
                        start_angle: arc.start_angle + i as f64 * FRAC_PI_2,
                        end_angle: arc.start_angle + (i + 1) as f64 * FRAC_PI_2,
                    })
                })
                .collect(),
            Curve::Ellipse(arc) if arc.sweep() >= TAU - 1e-12 => (0..4)
                .map(|i| {
                    Curve::Ellipse(EllipseArc {
                        ellipse: arc.ellipse,
                        start_parameter: arc.start_parameter + i as f64 * FRAC_PI_2,
                        end_parameter: arc.start_parameter + (i + 1) as f64 * FRAC_PI_2,
                    })
                })
                .collect(),
            Curve::Polyline(_) => curve.segments(),
            _ => vec![curve.clone()],
        })
        .collect()
}

fn split_closed_curves(ring: &[Curve]) -> Vec<Curve> {
    extrusion_profile_pieces(ring)
}

/// Whether a face's boundary runs into itself anywhere but where one piece
/// hands over to the next: a neighbour cut short past a notch in it, its
/// corners slid along their edges straight through the notch's sides.
fn crosses_itself(boundary: &[Curve], tolerance: f64) -> bool {
    let ends = |curve: &Curve| [curve.point_at(0.0), curve.point_at(1.0)];
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= tolerance;
    boundary.iter().enumerate().any(|(index, one)| {
        boundary[index + 1..].iter().any(|other| {
            ocs_geom::geom2d::intersect(one, other, Tolerance::new(tolerance)).iter().any(|hit| {
                !(ends(one).iter().any(|end| near(*end, hit.point))
                    && ends(other).iter().any(|end| near(*end, hit.point)))
            })
        })
    })
}

fn offset_local(body: &Body, key: FaceKey, distance: f64) -> Option<Body> {
    let profile = planar_face_profile(body, key)?;
    let normal = Vec3::from(profile.outward);
    let mut plane = profile.plane;
    plane.origin = (Vec3::from(plane.origin) + normal * distance).to_array();
    let tolerance = super::operation_tolerance(&[body]);
    if distance.abs() <= tolerance {
        return None;
    }
    let boundary: HashSet<EdgeKey> = body
        .face_coedges(key)
        .into_iter()
        .map(|coedge| body.coedges.get(coedge).map(|coedge| coedge.edge))
        .collect::<Option<HashSet<_>>>()?;
    let mut vertices = HashSet::new();
    for edge in &boundary {
        let edge = body.edges.get(*edge)?;
        vertices.insert(edge.start);
        vertices.insert(edge.end);
    }
    let mut moved = HashMap::new();
    for vertex in &vertices {
        let original = Vec3::from(body.vertices.get(*vertex)?.point);
        let rails = body
            .edges
            .iter()
            .filter(|(key, edge)| {
                !boundary.contains(key) && (edge.start == *vertex || edge.end == *vertex)
            })
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        for (_, edge) in rails {
            let curve = body.curves.get(edge.curve)?;
            let parameters = plane_curve_parameters(&plane, curve)?;
            let start = edge.start == *vertex;
            let old = if start {
                edge.start_parameter
            } else {
                edge.end_parameter
            };
            let other = if start {
                edge.end_parameter
            } else {
                edge.start_parameter
            };
            let parameter = parameters
                .into_iter()
                .filter(|t| {
                    t.is_finite()
                        && if start {
                            *t < other - tolerance
                        } else {
                            *t > other + tolerance
                        }
                })
                .min_by(|a, b| (a - old).abs().total_cmp(&(b - old).abs()))?;
            // A curved rail the moved plane only grazes ends there in a point
            // the face cannot reach along it: a fillet's end arc, met at its
            // lowest point once the face goes below the fillet's axis.
            if !matches!(curve, Curve3::Line(_)) {
                let along = Vec3::from(curve.tangent_at(parameter)).normalize()?;
                if along.dot(normal).abs() <= 1e-6 {
                    return None;
                }
            }
            candidates.push(Vec3::from(curve.point_at(parameter)));
        }
        // A closed circular seam sometimes has no rail. Its radial parameter
        // on the neighbouring analytic surface identifies the same seam.
        let point = if let Some(first) = candidates.first().copied() {
            if candidates
                .iter()
                .any(|other| first.distance(*other) > tolerance * 4.0)
            {
                return None;
            }
            first
        } else {
            let edge = boundary.iter().find_map(|edge| {
                let edge = body.edges.get(*edge)?;
                (edge.start == *vertex && edge.end == *vertex).then_some(edge)
            })?;
            let curve = body.curves.get(edge.curve)?;
            let other = adjacent_surface(body, edge, key)?;
            let next = intersect_curve(&plane, other, curve, tolerance)?;
            let estimated = original + normal * distance;
            Vec3::from(next.point_at(next.parameter_at(estimated.to_array())))
        };
        if plane.distance_to(point.to_array())?.abs() > tolerance * 4.0 {
            return None;
        }
        moved.insert(*vertex, point.to_array());
    }
    let mut result = body.clone();
    for (vertex, point) in &moved {
        result.vertices.get_mut(*vertex)?.point = *point;
        result.soil_vertex(*vertex);
    }
    let surface_key = result.surfaces.insert(Surface::Plane(plane));
    let selected = result.faces.get_mut(key)?;
    selected.surface = surface_key;
    selected.provenance.soil();
    let mut affected = HashSet::from([key]);
    for (edge_key, edge) in body.edges.iter() {
        if !vertices.contains(&edge.start) && !vertices.contains(&edge.end) {
            continue;
        }
        let original_curve = body.curves.get(edge.curve)?;
        let start = result.vertices.get(edge.start)?.point;
        let end = result.vertices.get(edge.end)?.point;
        let curve = if boundary.contains(&edge_key) {
            intersect_curve(
                &plane,
                adjacent_surface(body, edge, key)?,
                original_curve,
                tolerance,
            )?
        } else {
            original_curve.clone()
        };
        let (start_parameter, end_parameter) = curve_span(&curve, start, end, edge, tolerance)?;
        let curve_key = result.curves.insert(curve);
        let edited = result.edges.get_mut(edge_key)?;
        edited.curve = curve_key;
        edited.start_parameter = start_parameter;
        edited.end_parameter = end_parameter;
        edited.provenance.soil();
        for coedge in &edge.coedges {
            let node = result.coedges.get_mut(*coedge)?;
            node.pcurve = None;
            node.provenance.soil();
            affected.insert(result.loops.get(node.owner)?.owner);
        }
    }
    for face in affected {
        let node = result.faces.get(face)?;
        let surface = result.surfaces.get(node.surface)?;
        if let Surface::Plane(_) = surface {
            let before = super::pcurve::face_boundary(body, face, tolerance)?;
            let after = super::pcurve::face_boundary(&result, face, tolerance)?;
            let old_area = boundary_area(&before);
            let new_area = boundary_area(&after);
            if old_area * new_area <= 0.0
                || new_area.abs() <= tolerance * tolerance
                || crosses_itself(&after, tolerance)
            {
                return None;
            }
        }
        for coedge in result.face_coedges(face) {
            let edge = result.edges.get(result.coedges.get(coedge)?.edge)?;
            let curve = result.curves.get(edge.curve)?;
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let point = curve.point_at(
                    edge.start_parameter + fraction * (edge.end_parameter - edge.start_parameter),
                );
                if !surface.distance_to(point).is_finite()
                    || surface.distance_to(point).abs() > tolerance * 8.0
                {
                    return None;
                }
            }
        }
    }
    result.provenance.soil();
    (result.validate().is_empty() && result.worst_vertex_gap() <= tolerance * 4.0).then_some(result)
}

fn adjacent_surface<'a>(
    body: &'a Body,
    edge: &super::Edge,
    selected: FaceKey,
) -> Option<&'a Surface> {
    let other = edge.coedges.iter().find_map(|coedge| {
        let owner = body.loops.get(body.coedges.get(*coedge)?.owner)?.owner;
        (owner != selected).then_some(owner)
    })?;
    body.surfaces.get(body.faces.get(other)?.surface)
}

fn intersect_curve(plane: &Plane, other: &Surface, old: &Curve3, tolerance: f64) -> Option<Curve3> {
    let Meeting::Curves(curves) =
        super::intersect_surfaces(&Surface::Plane(*plane), other, tolerance)
    else {
        return None;
    };
    if curves.len() != 1 {
        return None;
    }
    let mut curve = curves.into_iter().next()?;
    match (&mut curve, old) {
        (Curve3::Line(new), Curve3::Line(old)) => {
            if Vec3::from(new.direction).dot(Vec3::from(old.direction)) < 0.0 {
                new.direction = (-Vec3::from(new.direction)).to_array();
            }
        }
        (Curve3::Circle(new), Curve3::Circle(old)) => {
            if Vec3::from(new.plane.normal()?).dot(Vec3::from(old.plane.normal()?)) < 0.0 {
                new.plane.y_axis = (-Vec3::from(new.plane.y_axis)).to_array();
            }
        }
        (Curve3::Ellipse(new), Curve3::Ellipse(old)) => {
            if Vec3::from(new.plane.normal()?).dot(Vec3::from(old.plane.normal()?)) < 0.0 {
                new.plane.y_axis = (-Vec3::from(new.plane.y_axis)).to_array();
            }
        }
        _ => return None,
    }
    Some(curve)
}

fn plane_curve_parameters(plane: &Plane, curve: &Curve3) -> Option<Vec<f64>> {
    let normal = Vec3::from(plane.normal()?);
    match curve {
        Curve3::Line(line) => {
            let along = normal.dot(Vec3::from(line.direction));
            if along.abs() <= 1e-12 * Vec3::from(line.direction).length() {
                return None;
            }
            Some(vec![
                normal.dot(Vec3::from(plane.origin) - Vec3::from(line.origin)) / along,
            ])
        }
        Curve3::Circle(circle) => {
            trigonometric_parameters(plane, &circle.plane, circle.radius, circle.radius)
        }
        Curve3::Ellipse(ellipse) => trigonometric_parameters(
            plane,
            &ellipse.plane,
            ellipse.major_radius,
            ellipse.minor_radius,
        ),
        _ => None,
    }
}

fn trigonometric_parameters(plane: &Plane, frame: &Plane, x: f64, y: f64) -> Option<Vec<f64>> {
    let normal = Vec3::from(plane.normal()?);
    let a = normal.dot(Vec3::from(frame.x_axis)) * x;
    let b = normal.dot(Vec3::from(frame.y_axis)) * y;
    let c = normal.dot(Vec3::from(plane.origin) - Vec3::from(frame.origin));
    let radius = a.hypot(b);
    if radius <= 1e-12 || c.abs() > radius {
        return None;
    }
    let phase = b.atan2(a);
    let angle = (c / radius).clamp(-1.0, 1.0).acos();
    Some(
        (-2..=2)
            .flat_map(|turn| {
                [
                    phase - angle + turn as f64 * TAU,
                    phase + angle + turn as f64 * TAU,
                ]
            })
            .collect(),
    )
}

fn curve_span(
    curve: &Curve3,
    start: [f64; 3],
    end: [f64; 3],
    original: &super::Edge,
    tolerance: f64,
) -> Option<(f64, f64)> {
    let from = curve.parameter_at(start);
    let mut to = curve.parameter_at(end);
    if matches!(curve, Curve3::Circle(_) | Curve3::Ellipse(_)) {
        to = from + (to - from).rem_euclid(TAU);
        if original.start == original.end {
            to = from + TAU;
        }
    }
    if !from.is_finite()
        || !to.is_finite()
        || to <= from
        || Vec3::from(curve.point_at(from)).distance(Vec3::from(start)) > tolerance * 4.0
        || Vec3::from(curve.point_at(to)).distance(Vec3::from(end)) > tolerance * 4.0
    {
        return None;
    }
    Some((from, to))
}

fn boundary_area(curves: &[Curve]) -> f64 {
    curves.iter().map(Curve::chain_area).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(min: [f64; 2], max: [f64; 2]) -> Body {
        let plane =
            Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).expect("an XY plane");
        let corners = [
            [min[0], min[1]],
            [max[0], min[1]],
            [max[0], max[1]],
            [min[0], max[1]],
        ];
        let boundary = (0..corners.len())
            .map(|index| {
                Curve::Line(Line {
                    start: corners[index],
                    end: corners[(index + 1) % corners.len()],
                })
            })
            .collect::<Vec<_>>();
        planar_region(plane, &[boundary]).expect("a rectangular sheet")
    }

    #[test]
    fn planar_booleans_return_valid_sheets() {
        let left = rectangle([0.0, 0.0], [2.0, 2.0]);
        let right = rectangle([1.0, 0.0], [3.0, 2.0]);

        let united = union_planar_regions(&[left.clone(), right.clone()], 1e-9).unwrap();
        assert_eq!(united.face_keys().count(), 1);
        assert!(united.validate().is_empty());

        let subtracted = subtract_planar_regions(&[left.clone()], &[right.clone()], 1e-9).unwrap();
        assert_eq!(subtracted.face_keys().count(), 1);
        assert!(subtracted.validate().is_empty());

        let PlanarIntersection::Area(intersection) =
            intersect_planar_regions(&[left, right], 1e-9).unwrap()
        else {
            panic!("overlapping sheets must have an area intersection");
        };
        assert_eq!(intersection.face_keys().count(), 1);
        assert!(intersection.validate().is_empty());
    }

    #[test]
    fn planar_intersection_distinguishes_contact_from_separation() {
        let left = rectangle([0.0, 0.0], [1.0, 1.0]);
        let touching = rectangle([1.0, 0.0], [2.0, 1.0]);
        let separate = rectangle([2.0, 0.0], [3.0, 1.0]);

        assert!(matches!(
            intersect_planar_regions(&[left.clone(), touching], 1e-9).unwrap(),
            PlanarIntersection::Touching
        ));
        assert!(matches!(
            intersect_planar_regions(&[left, separate], 1e-9).unwrap(),
            PlanarIntersection::Disjoint
        ));
    }

    #[test]
    fn a_straight_rational_piece_is_a_line_not_a_world_wide_circle() {
        // A file's straight edge written as a rational quadratic: its
        // control points in a row, its "radius" near 1e16.
        let straight = Curve::Nurbs(
            ocs_geom::geom2d::nurbs::NurbsCurve::new(
                2,
                vec![[15.0, -39.9], [15.0, -12.7], [15.0, 14.4]],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                Some(vec![1.0, 0.7, 1.0]),
            )
            .unwrap(),
        );
        let pieces = extrusion_profile_pieces(&[straight]);
        assert!(matches!(pieces[..], [Curve::Line(_)]), "{pieces:?}");
    }

    #[test]
    fn an_elbows_end_pushed_in_is_the_elbow_cut_short() {
        // A quarter torus: its end's only neighbour is the tube, which no
        // prism runs along; moved in, the end trims the tube where it lands.
        let plane = Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]).unwrap();
        let half = |start_angle: f64| {
            let end_angle = start_angle + std::f64::consts::PI;
            Curve::Arc(Arc { centre: [5.0, 0.0], radius: 1.0, start_angle, end_angle })
        };
        let ring = [half(0.0), half(std::f64::consts::PI)];
        let elbow = crate::revolve(plane, &ring, [0.0; 3], [0.0, 0.0, 1.0], FRAC_PI_2)
            .unwrap();
        let end = elbow
            .face_keys()
            .find(|face| {
                planar_face_profile(&elbow, *face)
                    .is_some_and(|profile| profile.outward[1] < -0.99)
            })
            .unwrap();
        let short = presspull_face(&elbow, end, -0.5, PresspullMode::Offset).unwrap();
        assert!(short.validate().is_empty());
        let lowest = short.vertices.iter().map(|(_, v)| v.point[1]).fold(f64::INFINITY, f64::min);
        assert!((lowest - 0.5).abs() < 1e-6, "{lowest}");
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        assert!(volume(&short) < volume(&elbow));
    }

    #[test]
    fn a_face_moved_by_its_region_matches_the_convex_slab() {
        // A pyramid's side meets the other sides at the apex, six faces at
        // one corner: moving its corners along the body's edges cannot, the
        // region carried on between its neighbours can, and lands where the
        // slab of the neighbours' planes does.
        let pyramid = crate::make::pyramid([0.0; 3], 3.0, 4.0, 6).unwrap();
        let side = pyramid
            .face_keys()
            .find(|face| {
                let node = pyramid.faces.get(*face).unwrap();
                let Some(Surface::Plane(plane)) = pyramid.surfaces.get(node.surface) else {
                    return false;
                };
                plane.normal().is_some_and(|normal| normal[2].abs() < 0.99)
            })
            .unwrap();
        let profile = planar_face_profile(&pyramid, side).unwrap();
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        for distance in [0.3, -0.3] {
            let region = region_offset(&pyramid, side, &profile, distance).unwrap();
            let slab = convex_offset(&pyramid, side, &profile, distance).unwrap();
            assert!(region.validate().is_empty());
            assert!((volume(&region) - volume(&slab)).abs() < 1e-9 * volume(&slab));
        }
    }
}
