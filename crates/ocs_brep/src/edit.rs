//! Editing a solid face by face: moving, turning, offsetting, tapering,
//! extruding and deleting faces, splitting a body into its pieces, cleaning
//! it up, and copying faces out of it.
//!
//! # Moving faces keeps the topology
//!
//! A face is edited by giving it a new surface and finding again where it
//! meets its neighbours: a vertex goes where its faces' surfaces now cross,
//! and an edge is laid along what its two faces' surfaces share. No face
//! appears or vanishes, so an edit that would need one to — a face moved
//! past the end of its neighbour — fails the checks at the end and is
//! refused, rather than being approximated into a different shape.
//!
//! # Deleting a face changes it
//!
//! Two kinds of face can go: a feature cut into or standing on other faces
//! (a hole, a pocket, a boss), whose rims are holes in the faces around it,
//! and a blend between two faces (a fillet or a chamfer), whose two sides
//! are extended until they meet again. Anything else leaves a gap nothing
//! here can close, and is refused.

use super::geometry::{Curve3, Line3, Surface};
use super::topology::{Body, CoedgeKey, EdgeKey, FaceKey, LoopKey, VertexKey};
use super::{Meeting, Operation, Placement, PresspullMode};
use ocs_geom::space::Vec3;
use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;

/// The face a picked point lies on: the one whose surface passes nearest it,
/// among the faces whose extent reaches it.
pub fn face_at_point(body: &Body, point: [f64; 3]) -> Option<FaceKey> {
    if point.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let tolerance = super::operation_tolerance(&[body]);
    let reach = super::body_bounds(body)
        .map(|bounds| {
            (0..3)
                .map(|axis| bounds.max[axis] - bounds.min[axis])
                .fold(0.0_f64, f64::max)
        })
        .unwrap_or(1.0)
        * 1e-3;
    body.face_keys()
        .filter_map(|key| {
            let bounds = super::face_bounds(body, key)?;
            if !bounds.grown(reach.max(tolerance * 10.0)).holds(point) {
                return None;
            }
            let face = body.faces.get(key)?;
            let distance = body.surfaces.get(face.surface)?.distance_to(point).abs();
            distance.is_finite().then_some((key, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(key, _)| key)
}

/// A rotation by `angle` radians about the line through `origin` along
/// `axis`, right-handed.
pub fn rotation(origin: [f64; 3], axis: [f64; 3], angle: f64) -> Option<Placement> {
    let axis = Vec3::from(axis).normalize()?;
    let (sin, cos) = angle.sin_cos();
    let turn = |v: Vec3| v * cos + axis.cross(v) * sin + axis * axis.dot(v) * (1.0 - cos);
    let x = turn(Vec3::new(1.0, 0.0, 0.0));
    let y = turn(Vec3::new(0.0, 1.0, 0.0));
    let z = turn(Vec3::new(0.0, 0.0, 1.0));
    let moved = x * origin[0] + y * origin[1] + z * origin[2];
    Some(Placement {
        x_axis: x.to_array(),
        y_axis: y.to_array(),
        z_axis: z.to_array(),
        origin: (Vec3::from(origin) - moved).to_array(),
    })
}

/// Moves `faces` rigidly by `place`, a rotation and translation, refitting
/// the faces around them.
pub fn move_faces(body: &Body, faces: &[FaceKey], place: &Placement) -> Option<Body> {
    if place.scale().is_none_or(|scale| (scale - 1.0).abs() > 1e-9) || place.reflects() {
        return None;
    }
    let mut changes = HashMap::new();
    for face in distinct(faces) {
        let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
        let moved = super::place::move_surface(surface, place, 1.0)?;
        changes.insert(face, Change { surface: moved, motion: Some(*place) });
    }
    tweak(body, &changes)
}

/// Moves each of `faces` along its own outward normal by `distance`: a
/// positive distance grows the solid, a negative one shrinks it.
pub fn offset_faces(body: &Body, faces: &[FaceKey], distance: f64) -> Option<Body> {
    if !distance.is_finite() || distance == 0.0 {
        return None;
    }
    // Solidcore's face offset first; this kernel's own for what it refuses.
    if let Ok(moved) = super::bridge::offset_faces(body, faces, distance) {
        return Some(moved);
    }
    let tolerance = super::operation_tolerance(&[body]);
    let mut changes = HashMap::new();
    for face in distinct(faces) {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        let along = if node.forward { distance } else { -distance };
        let (surface, motion) = offset_surface(surface, along, tolerance)?;
        changes.insert(face, Change { surface, motion });
    }
    tweak(body, &changes)
}

/// Tilts each face in `faces` by `angle` radians about where it crosses the
/// plane through `base` square to `axis`, pulling along `axis`: see
/// [`draft_faces`]. A positive angle leans the face into the solid going
/// along the axis from `base`.
pub fn taper_faces(
    body: &Body,
    faces: &[FaceKey],
    base: [f64; 3],
    axis: [f64; 3],
    angle: f64,
) -> Option<Body> {
    draft_faces(body, faces, base, axis, axis, angle)
}

/// Drafts each face in `faces` by `angle` radians against the `pull`
/// direction, hinged on the neutral plane through `base` square to
/// `neutral`, so that it leans in going along the pull.
///
/// A plane along the pull turns about its line on the neutral plane until
/// it makes `angle` with the pull, whatever the neutral plane: with `α` the
/// angle between that line and the pull, it turns by `φ`,
/// `sin φ = sin θ / sin α`, and a draft steeper than the line allows is
/// refused. With the neutral plane square to the pull a plane already
/// slanted to it is turned by the angle itself.
///
/// A cylinder or a cone about the pull keeps its axis and is hinged on its
/// circle on the neutral plane: with `R0` the radius there and `t` the slope
/// of the radius along the pull, the radius at height `h` becomes
/// `R0 + h tan(atan t − sθ)`, `s` 1 for a face looking away from its axis (a
/// pin) and −1 for one looking towards it (a hole).
///
/// Adapted from Solidcore's `offset/draft.rs` (`plane_draft`, `cone_draft`).
pub fn draft_faces(
    body: &Body,
    faces: &[FaceKey],
    base: [f64; 3],
    neutral: [f64; 3],
    pull: [f64; 3],
    angle: f64,
) -> Option<Body> {
    if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 {
        return None;
    }
    let neutral = Vec3::from(neutral).normalize()?;
    let pull = Vec3::from(pull).normalize()?;
    // A pull in the neutral plane never crosses it.
    if pull.dot(neutral).abs() < 1e-12 {
        return None;
    }
    // Solidcore's draft first; this kernel's own for what it refuses.
    if let Ok(drafted) =
        super::bridge::draft(body, faces, base, neutral.to_array(), pull.to_array(), angle)
    {
        return Some(drafted);
    }
    let base = Vec3::from(base);
    let mut changes = HashMap::new();
    for face in distinct(faces) {
        let node = body.faces.get(face)?;
        let outward = if node.forward { 1.0 } else { -1.0 };
        let change = match body.surfaces.get(node.surface)? {
            Surface::Plane(plane) => {
                let normal = Vec3::from(plane.normal()?) * outward;
                let place = plane_draft(Vec3::from(plane.origin), normal, base, neutral, pull, angle)?;
                let surface = super::place::move_surface(&Surface::Plane(*plane), &place, 1.0)?;
                Change { surface, motion: Some(place) }
            }
            surface @ (Surface::Cylinder(_) | Surface::Cone(_)) => {
                let surface = cone_draft(surface, outward, base, neutral, pull, angle)?;
                Change { surface, motion: None }
            }
            _ => return None,
        };
        changes.insert(face, change);
    }
    tweak(body, &changes)
}

/// The turn that drafts the plane through `origin` with outward unit normal
/// `n`.
fn plane_draft(origin: Vec3, n: Vec3, base: Vec3, neutral: Vec3, pull: Vec3, angle: f64) -> Option<Placement> {
    let square = neutral.cross(pull).length() <= 1e-12;
    if n.dot(pull).abs() > 1e-6 {
        // Slanted to the pull: only where the neutral plane is square to
        // it, by the angle itself about the pull's cross with the normal.
        if !square {
            return None;
        }
        let hinge = pull.cross(n).normalize()?;
        let slant = pull.dot(n);
        let rise = (origin - base).dot(n) / (1.0 - slant * slant);
        let pivot = base + n * rise - pull * (rise * slant);
        return rotation(pivot.to_array(), hinge.to_array(), -angle);
    }
    // The hinge: the line along n × N where the face's plane meets the
    // neutral plane, reached from the face's origin along m, the neutral
    // normal's part in the face's plane.
    let m = neutral - n * n.dot(neutral);
    let reach = m.dot(neutral);
    if reach <= 1e-12 {
        return None;
    }
    let hinge = origin + m * ((base - origin).dot(neutral) / reach);
    let mut along = n.cross(neutral).normalize()?;
    // Turned so that d × n runs with the pull: the side the face leans to.
    let lean = along.cross(n).dot(pull);
    if lean.abs() <= 1e-12 {
        return None;
    }
    if lean < 0.0 {
        along = along * -1.0;
    }
    let ratio = angle.sin() / along.cross(n).dot(pull);
    if !(ratio.abs() <= 1.0) {
        return None;
    }
    rotation(hinge.to_array(), along.to_array(), ratio.asin())
}

/// The cylinder or cone `surface` drafted about its circle on the neutral
/// plane; `outward` is 1 where the face runs with the surface's normal.
fn cone_draft(surface: &Surface, outward: f64, base: Vec3, neutral: Vec3, pull: Vec3, angle: f64) -> Option<Surface> {
    let (frame, radius, half_angle) = match surface {
        Surface::Cylinder(cylinder) => (cylinder.base, cylinder.radius, 0.0),
        Surface::Cone(cone) => (cone.base, cone.radius, cone.half_angle),
        _ => return None,
    };
    let z = Vec3::from(frame.normal()?).normalize()?;
    let sense = z.dot(pull);
    if (sense.abs() - 1.0).abs() > 1e-9 {
        return None;
    }
    let sense = sense.signum();
    let toward = z.dot(neutral);
    if toward.abs() < 1e-12 {
        return None;
    }
    let axis = Vec3::from(frame.origin);
    // The hinge circle: the axis's height `v0` where it crosses the neutral
    // plane, and the radius there.
    let v0 = (base - axis).dot(neutral) / toward;
    let r0 = radius - v0 * half_angle.tan();
    // Both surfaces' normals point away from the axis where the radius is
    // positive: a face with them is a pin.
    let pin = outward * r0.signum();
    let slope = -half_angle.tan() * sense;
    let turned = (slope.atan() - pin * angle).tan();
    if !(r0.abs() > 0.0 && turned.is_finite()) {
        return None;
    }
    // Along the axis rather than the pull, and from the frame's origin.
    let rate = turned * sense;
    let at_origin = r0 - v0 * rate;
    let mut frame = frame;
    if rate.abs() <= 1e-15 {
        return Some(Surface::Cylinder(super::Cylinder { base: frame, radius: at_origin }));
    }
    frame.origin = axis.to_array();
    Some(Surface::Cone(super::Cone { base: frame, radius: at_origin, half_angle: (-rate).atan() }))
}

/// Extrudes each planar face in `faces` along its outward normal by
/// `height`, tapering the sides by `taper` radians: a positive taper draws
/// the far end in. A negative height cuts into the solid.
pub fn extrude_faces(body: &Body, faces: &[FaceKey], height: f64, taper: f64) -> Option<Body> {
    if !height.is_finite() || height == 0.0 || !taper.is_finite() {
        return None;
    }
    let tolerance = super::operation_tolerance(&[body]);
    // Keys do not survive the boolean each face is pulled by, so each face is
    // found again from a point inside it.
    let points = distinct(faces)
        .into_iter()
        .map(|face| super::boolean::interior_point(body, face, tolerance))
        .collect::<Option<Vec<_>>>()?;
    let mut result = body.clone();
    for point in points {
        let tolerance = super::operation_tolerance(&[&result]);
        let face = super::planar_face_at_point(&result, point, tolerance * 10.0)?;
        result = if taper == 0.0 {
            super::presspull_face(&result, face, height, PresspullMode::Extrude)?
        } else {
            tapered_pull(&result, face, height, taper, tolerance)?
        };
    }
    Some(result)
}

#[cfg(feature = "offset")]
fn tapered_pull(body: &Body, face: FaceKey, height: f64, taper: f64, tolerance: f64) -> Option<Body> {
    let profile = super::planar_face_profile(body, face)?;
    let direction = (Vec3::from(profile.outward) * height).to_array();
    let tool = super::extrude_region_tapered(profile.plane, &profile.loops, direction, taper)?;
    let how = if height > 0.0 { Operation::Union } else { Operation::Difference };
    let mut result = super::combine(body.clone(), tool, how, tolerance).ok()?;
    super::boolean::merge_coplanar_faces(&mut result, tolerance);
    Some(result)
}

#[cfg(not(feature = "offset"))]
fn tapered_pull(_: &Body, _: FaceKey, _: f64, _: f64, _: f64) -> Option<Body> {
    None
}

/// Deletes `faces`, closing the gap they leave: a hole or a boss goes with
/// the faces around its rim made whole again, and a fillet or a chamfer goes
/// with its two sides extended to meet.
pub fn delete_faces(body: &Body, faces: &[FaceKey]) -> Option<Body> {
    let selected = distinct(faces);
    if selected.is_empty() {
        return None;
    }
    if let Some(result) = remove_feature(body, &selected.iter().copied().collect()) {
        return Some(result);
    }
    let mut result = body.clone();
    for face in selected {
        result = collapse_blend(&result, face)?;
    }
    Some(result)
}

/// One body per connected piece.
pub fn separate(body: &Body) -> Vec<Body> {
    if body.roots.len() <= 1 {
        return vec![body.clone()];
    }
    body.roots
        .iter()
        .filter_map(|lump| {
            let mut piece = body.clone();
            piece.roots = vec![*lump];
            prune(&mut piece);
            piece.provenance.soil();
            piece.validate().is_empty().then_some(piece)
        })
        .collect()
}

/// Removes the edges and vertices that divide nothing: faces split on one
/// plane become one face, and edges split along one line become one edge.
pub fn clean(body: &Body) -> Body {
    let mut result = body.clone();
    let tolerance = super::operation_tolerance(&[body]);
    super::boolean::clean_body(&mut result, tolerance);
    result
}

/// What a body's own checks find wrong with it; empty when it is a valid,
/// closed solid of positive volume.
pub fn check(body: &Body) -> Vec<String> {
    let mut problems = body
        .validate()
        .into_iter()
        .map(|flaw| format!("{flaw:?}"))
        .collect::<Vec<_>>();
    let tolerance = super::operation_tolerance(&[body]);
    if body.worst_vertex_gap() > tolerance * 100.0 {
        problems.push("vertices lie off the edges ending at them".into());
    }
    if body.edges.iter().any(|(_, edge)| edge.coedges.len() != 2) {
        problems.push("the shell is open or not manifold".into());
    }
    problems.extend(super::audit::geometry_flaws(body, tolerance * 100.0));
    if let Some(properties) = super::mass_properties(body) {
        if !(properties.volume > 0.0) {
            problems.push("the volume is not positive".into());
        }
    }
    problems
}

/// A copy of `faces` as an open sheet, sharing the edges they share.
pub fn face_sheet(body: &Body, faces: &[FaceKey]) -> Option<Body> {
    let wanted: HashSet<FaceKey> = distinct(faces).into_iter().collect();
    if wanted.is_empty() {
        return None;
    }
    let mut sheet = body.clone();
    let lump = *sheet.roots.first()?;
    let shell = *sheet.lumps.get(lump)?.shells.first()?;
    sheet.roots = vec![lump];
    sheet.lumps.get_mut(lump)?.shells = vec![shell];
    sheet.shells.get_mut(shell)?.faces = wanted.iter().copied().collect();
    for face in &wanted {
        sheet.faces.get_mut(*face)?.owner = shell;
    }
    prune(&mut sheet);
    let kept: HashSet<CoedgeKey> = sheet.coedges.keys().collect();
    for edge in sheet.edges.values_mut() {
        edge.coedges.retain(|coedge| kept.contains(coedge));
    }
    for face in sheet.faces.values_mut() {
        face.provenance = super::Provenance::Synthesized;
    }
    sheet.provenance = super::Provenance::Synthesized;
    sheet.validate().is_empty().then_some(sheet)
}

/// Cuts every face `curve` lies on along it, between parameters `span`:
/// the faces it divides become two. A cut that would run past the span —
/// a short segment ending inside a face — is not made. `None` when nothing
/// was cut.
pub fn imprint_curve(body: &Body, curve: &Curve3, span: (f64, f64)) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[body]);
    let whole = matches!(curve, Curve3::Circle(_) | Curve3::Ellipse(_)) && span.1 - span.0 >= TAU - 1e-9;
    let inside = |point: [f64; 3]| {
        if whole {
            return true;
        }
        let mut parameter = curve.parameter_at(point);
        if matches!(curve, Curve3::Circle(_) | Curve3::Ellipse(_)) {
            parameter = span.0 + (parameter - span.0).rem_euclid(TAU);
        }
        let slack = tolerance / Vec3::from(curve.tangent_at(parameter)).length().max(1e-300) * 4.0;
        parameter >= span.0 - slack
            && parameter <= span.1 + slack
            && Vec3::from(curve.point_at(parameter)).distance(Vec3::from(point)) <= tolerance * 4.0
    };
    let mut result = body.clone();
    let mut cut = false;
    for face in body.face_keys().collect::<Vec<_>>() {
        let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
        if !lies_on(curve, span, surface, tolerance) {
            continue;
        }
        let before: HashSet<EdgeKey> = result.edge_keys().collect();
        let mut trial = result.clone();
        if super::split::split_face(&mut trial, face, curve, tolerance).is_none() {
            continue;
        }
        // Only the new edges along the curve: splitting the face's rim where
        // the cut meets it makes new edges there too.
        let along = |point: [f64; 3]| {
            Vec3::from(curve.point_at(curve.parameter_at(point))).distance(Vec3::from(point))
                <= tolerance * 4.0
        };
        let within = trial
            .edge_keys()
            .filter(|edge| !before.contains(edge))
            .filter_map(|edge| {
                let node = trial.edges.get(edge)?;
                let middle = trial
                    .curves
                    .get(node.curve)?
                    .point_at(0.5 * (node.start_parameter + node.end_parameter));
                along(middle).then(|| trial.edge_endpoints(edge))?
            })
            .all(|(start, end)| inside(start) && inside(end));
        if within {
            result = trial;
            cut = true;
        }
    }
    result.provenance.soil();
    (cut && result.validate().is_empty()).then_some(result)
}

/// A face's new surface, and the rigid motion that took it there when there
/// was one — which is where its vertices are looked for first.
struct Change {
    surface: Surface,
    motion: Option<Placement>,
}

fn distinct(faces: &[FaceKey]) -> Vec<FaceKey> {
    let mut seen = HashSet::new();
    faces.iter().copied().filter(|face| seen.insert(*face)).collect()
}

/// The largest distance `faces` can each be moved along their outward
/// normals, the way `distance`'s sign says, before one of them folds over
/// itself or closes up: the smallest radius of curvature bending that way.
/// Infinite where nothing bends that way; `None` for a face whose surface
/// cannot be measured.
pub fn offset_limit(body: &Body, faces: &[FaceKey], distance: f64) -> Option<f64> {
    let mut limit = f64::INFINITY;
    for face in distinct(faces) {
        let node = body.faces.get(face)?;
        let along = if node.forward { distance } else { -distance };
        let shrinking = along < 0.0;
        let here = match body.surfaces.get(node.surface)? {
            Surface::Plane(_) => f64::INFINITY,
            Surface::Cylinder(cylinder) if shrinking => cylinder.radius.abs(),
            Surface::Sphere(sphere) if shrinking => sphere.radius.abs(),
            Surface::Torus(torus) if shrinking => torus.minor_radius.abs(),
            // Grown past the major radius, the tube meets itself on the axis.
            Surface::Torus(torus) => torus.major_radius.abs() - torus.minor_radius.abs(),
            Surface::Cylinder(_) | Surface::Sphere(_) | Surface::Cone(_) => f64::INFINITY,
            Surface::Nurbs(surface) => super::certified_offset::largest_offset(surface, along)?,
            Surface::Offset(offset) => {
                super::certified_offset::largest_offset(&offset.base, offset.distance + along)?
                    - offset.distance.abs()
            }
        };
        limit = limit.min(here);
    }
    Some(limit)
}

/// A surface moved along its own normal by `along`, with the motion when the
/// move is a translation. A spline's offset is certified within `tolerance`.
fn offset_surface(surface: &Surface, along: f64, tolerance: f64) -> Option<(Surface, Option<Placement>)> {
    let grown = |radius: f64| (radius + along > 0.0).then_some(radius + along);
    Some(match surface {
        Surface::Plane(plane) => {
            let shift = Vec3::from(plane.normal()?) * along;
            let mut moved = *plane;
            moved.origin = (Vec3::from(plane.origin) + shift).to_array();
            (Surface::Plane(moved), Some(Placement::at(shift.to_array())))
        }
        Surface::Cylinder(cylinder) => {
            let mut moved = *cylinder;
            moved.radius = grown(cylinder.radius)?;
            (Surface::Cylinder(moved), None)
        }
        Surface::Sphere(sphere) => {
            let mut moved = *sphere;
            moved.radius = grown(sphere.radius)?;
            (Surface::Sphere(moved), None)
        }
        Surface::Torus(torus) => {
            let mut moved = *torus;
            moved.minor_radius = grown(torus.minor_radius)?;
            (Surface::Torus(moved), None)
        }
        Surface::Cone(cone) => {
            let mut moved = *cone;
            moved.radius = cone.radius + along / cone.half_angle.cos();
            (Surface::Cone(moved), None)
        }
        Surface::Nurbs(spline) => {
            let offset = super::certified_offset::certified_offset(spline, along, tolerance).ok()?;
            (Surface::Offset(super::OffsetSurface { base: spline.clone(), distance: along, approximation: offset.surface }), None)
        }
        Surface::Offset(offset) => {
            let distance = offset.distance + along;
            let approximation =
                super::certified_offset::certified_offset(&offset.base, distance, tolerance).ok()?.surface;
            (Surface::Offset(super::OffsetSurface { base: offset.base.clone(), distance, approximation }), None)
        }
    })
}

fn owner_face(body: &Body, coedge: CoedgeKey) -> Option<FaceKey> {
    Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner)
}

fn edge_faces(body: &Body, edge: EdgeKey) -> Option<Vec<FaceKey>> {
    body.edges
        .get(edge)?
        .coedges
        .iter()
        .map(|coedge| owner_face(body, *coedge))
        .collect()
}

/// The faces across each of `face`'s edges.
fn neighbours(body: &Body, face: FaceKey) -> HashSet<FaceKey> {
    body.face_coedges(face)
        .into_iter()
        .filter_map(|coedge| body.partner(coedge))
        .filter_map(|coedge| owner_face(body, coedge))
        .filter(|other| *other != face)
        .collect()
}

/// Gives the changed faces their new surfaces and refits everything they
/// touch.
fn tweak(body: &Body, changes: &HashMap<FaceKey, Change>) -> Option<Body> {
    if changes.is_empty() {
        return None;
    }
    let mut result = body.clone();
    for (face, change) in changes {
        let surface = result.surfaces.insert(change.surface.clone());
        let node = result.faces.get_mut(*face)?;
        node.surface = surface;
        node.provenance.soil();
    }
    let motions: HashMap<FaceKey, Option<Placement>> =
        changes.iter().map(|(face, change)| (*face, change.motion)).collect();
    refit(body, result, &motions, &HashSet::new(), &HashSet::new())
}

/// Puts every vertex and edge touching a changed face, or named as moved,
/// back where the surfaces now say, and checks the result.
///
/// `original` is the body before any edit, to measure faces against.
/// `changed` names the faces given new surfaces, with their motions; `seeds`
/// are vertices to solve again whatever their faces, and `fresh` edges to lay
/// along their faces' meeting rather than keep their curves.
fn refit(
    original: &Body,
    mut body: Body,
    changed: &HashMap<FaceKey, Option<Placement>>,
    seeds: &HashSet<VertexKey>,
    fresh: &HashSet<EdgeKey>,
) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[original]);
    let mut vertices: HashSet<VertexKey> = seeds.clone();
    for (key, _) in body.edges.iter() {
        let faces = edge_faces(&body, key)?;
        if fresh.contains(&key) || faces.iter().any(|face| changed.contains_key(face)) {
            let edge = body.edges.get(key)?;
            vertices.insert(edge.start);
            vertices.insert(edge.end);
        }
    }
    let edges: Vec<EdgeKey> = body
        .edges
        .iter()
        .filter(|(key, edge)| {
            fresh.contains(key) || vertices.contains(&edge.start) || vertices.contains(&edge.end)
        })
        .map(|(key, _)| key)
        .collect();

    // Each vertex where its faces' surfaces now cross.
    let mut solved = HashMap::new();
    for vertex in &vertices {
        let mut faces = HashSet::new();
        for (key, edge) in body.edges.iter() {
            if edge.start == *vertex || edge.end == *vertex {
                faces.extend(edge_faces(&body, key)?);
            }
        }
        let motions: Vec<Option<Placement>> = faces
            .iter()
            .filter_map(|face| changed.get(face).copied())
            .collect();
        let old = body.vertices.get(*vertex)?.point;
        let start = match motions.first() {
            Some(Some(first)) if motions.iter().all(|motion| *motion == Some(*first)) => {
                first.point(old)
            }
            _ => old,
        };
        let mut surfaces: Vec<&Surface> = Vec::new();
        for face in &faces {
            let surface = body.surfaces.get(body.faces.get(*face)?.surface)?;
            if !surfaces.iter().any(|known| *known == surface) {
                surfaces.push(surface);
            }
        }
        solved.insert(*vertex, meet_point(&surfaces, start, tolerance)?);
    }
    for (vertex, point) in &solved {
        body.vertices.get_mut(*vertex)?.point = *point;
        body.soil_vertex(*vertex);
    }

    // Each edge along what its faces now share.
    let mut touched = HashSet::new();
    for key in edges {
        let edge = body.edges.get(key)?.clone();
        let faces = edge_faces(&body, key)?;
        let surfaces = faces
            .iter()
            .map(|face| body.surfaces.get(body.faces.get(*face)?.surface).cloned())
            .collect::<Option<Vec<_>>>()?;
        let old = body.curves.get(edge.curve)?.clone();
        let motions: Vec<Option<Placement>> = faces
            .iter()
            .filter_map(|face| changed.get(face).copied())
            .collect();
        let start = body.vertices.get(edge.start)?.point;
        let end = body.vertices.get(edge.end)?.point;
        let closed = edge.start == edge.end;
        let heading = Vec3::from(old.tangent_at(edge.start_parameter));
        let (curve, span) = if motions.is_empty() && !fresh.contains(&key) {
            let span = if closed && !matches!(old, Curve3::Circle(_) | Curve3::Ellipse(_)) {
                (edge.start_parameter, edge.end_parameter)
            } else {
                span_of(&old, start, end, closed, tolerance)?
            };
            (old, span)
        } else {
            let mut candidates = Vec::new();
            if let Some(Some(first)) = motions.first() {
                if motions.iter().all(|motion| *motion == Some(*first)) {
                    if let Some(moved) = super::place::move_curve(&old, first, 1.0) {
                        candidates.push((moved, Vec3::from(first.vector(heading.to_array()))));
                    }
                }
            }
            if !fresh.contains(&key) {
                candidates.push((old.clone(), heading));
            }
            if !closed && Vec3::from(start).distance(Vec3::from(end)) > tolerance {
                candidates.push((
                    Curve3::Line(Line3 {
                        origin: start,
                        direction: (Vec3::from(end) - Vec3::from(start)).to_array(),
                    }),
                    heading,
                ));
            }
            if let [one, two] = surfaces.as_slice() {
                if let Meeting::Curves(curves) = super::intersect_surfaces(one, two, tolerance) {
                    candidates.extend(curves.into_iter().map(|curve| (curve, heading)));
                }
            }
            candidates
                .into_iter()
                .find_map(|(curve, heading)| {
                    let curve = facing(curve, heading, start)?;
                    let span = span_of(&curve, start, end, closed, tolerance)?;
                    surfaces
                        .iter()
                        .all(|surface| lies_on(&curve, span, surface, tolerance))
                        .then_some((curve, span))
                })?
        };
        let curve = body.curves.insert(curve);
        let node = body.edges.get_mut(key)?;
        node.curve = curve;
        node.start_parameter = span.0;
        node.end_parameter = span.1;
        node.provenance.soil();
        for coedge in &edge.coedges {
            let node = body.coedges.get_mut(*coedge)?;
            node.pcurve = None;
            node.provenance.soil();
        }
        touched.extend(faces);
    }

    // Every face it changed still bounds the same side of its surface, and
    // its edges lie on it.
    for face in touched {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        for coedge in body.face_coedges(face) {
            let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
            let curve = body.curves.get(edge.curve)?;
            if !lies_on(curve, (edge.start_parameter, edge.end_parameter), surface, tolerance) {
                return None;
            }
        }
        if let (Surface::Plane(_), Some(_)) = (surface, original.faces.get(face)) {
            let before = super::pcurve::face_boundary(original, face, tolerance)?;
            let after = super::pcurve::face_boundary(&body, face, tolerance)?;
            let old_area: f64 = before.iter().map(ocs_geom::geom2d::Curve::chain_area).sum();
            let new_area: f64 = after.iter().map(ocs_geom::geom2d::Curve::chain_area).sum();
            if old_area * new_area <= 0.0 || new_area.abs() <= tolerance * tolerance {
                return None;
            }
        }
    }
    body.provenance.soil();
    if !body.validate().is_empty() || body.worst_vertex_gap() > tolerance * 4.0 {
        return None;
    }
    if let Some(properties) = super::mass_properties(&body) {
        if !(properties.volume > 0.0) {
            return None;
        }
    }
    Some(body)
}

/// The point nearest `start` on every one of `surfaces`: the corner they
/// make, or the nearest point of the curve two of them share.
fn meet_point(surfaces: &[&Surface], start: [f64; 3], tolerance: f64) -> Option<[f64; 3]> {
    let mut point = Vec3::from(start);
    for _ in 0..64 {
        let rows = surfaces
            .iter()
            .map(|surface| {
                let at = point.to_array();
                let value = surface.distance_to(at);
                let step = 1e-7 * (1.0 + point.length());
                let slope = |axis: usize| {
                    let mut ahead = at;
                    let mut behind = at;
                    ahead[axis] += step;
                    behind[axis] -= step;
                    (surface.distance_to(ahead) - surface.distance_to(behind)) / (2.0 * step)
                };
                (Vec3::new(slope(0), slope(1), slope(2)), value)
            })
            .collect::<Vec<_>>();
        if rows.iter().any(|(slope, value)| !slope.is_finite() || !value.is_finite()) {
            return None;
        }
        let worst = rows.iter().map(|(_, value)| value.abs()).fold(0.0, f64::max);
        if worst <= tolerance * 1e-3 {
            return Some(point.to_array());
        }
        // The smallest step putting every surface's linearisation through
        // zero: Δ = −Gᵀ (G Gᵀ + λ)⁻¹ d. The damping keeps a corner of more
        // than three faces, whose rows depend on each other, solvable.
        let count = rows.len();
        let mut gram = vec![vec![0.0; count]; count];
        for (i, (left, _)) in rows.iter().enumerate() {
            for (j, (right, _)) in rows.iter().enumerate() {
                gram[i][j] = left.dot(*right);
            }
            gram[i][i] += 1e-12;
        }
        let values = rows.iter().map(|(_, value)| *value).collect();
        let weights = solve(gram, values)?;
        let step = rows
            .iter()
            .zip(weights)
            .fold(Vec3::new(0.0, 0.0, 0.0), |sum, ((slope, _), weight)| sum + *slope * weight);
        point = point - step;
    }
    surfaces
        .iter()
        .all(|surface| surface.distance_to(point.to_array()).abs() <= tolerance * 4.0)
        .then_some(point.to_array())
}

/// Gaussian elimination with partial pivoting.
fn solve(mut matrix: Vec<Vec<f64>>, mut values: Vec<f64>) -> Option<Vec<f64>> {
    let size = values.len();
    for column in 0..size {
        let pivot = (column..size).max_by(|a, b| matrix[*a][column].abs().total_cmp(&matrix[*b][column].abs()))?;
        if matrix[pivot][column].abs() <= 1e-300 {
            return None;
        }
        matrix.swap(column, pivot);
        values.swap(column, pivot);
        for row in column + 1..size {
            let factor = matrix[row][column] / matrix[column][column];
            for k in column..size {
                matrix[row][k] -= factor * matrix[column][k];
            }
            values[row] -= factor * values[column];
        }
    }
    let mut result = vec![0.0; size];
    for row in (0..size).rev() {
        let known: f64 = (row + 1..size).map(|k| matrix[row][k] * result[k]).sum();
        result[row] = (values[row] - known) / matrix[row][row];
    }
    result.iter().all(|value| value.is_finite()).then_some(result)
}

/// `curve` turned, if need be, to run along `heading` where it passes `at`.
fn facing(curve: Curve3, heading: Vec3, at: [f64; 3]) -> Option<Curve3> {
    let along = Vec3::from(curve.tangent_at(curve.parameter_at(at)));
    if along.dot(heading) >= 0.0 {
        return Some(curve);
    }
    Some(match curve {
        Curve3::Line(line) => Curve3::Line(Line3 {
            origin: line.origin,
            direction: (-Vec3::from(line.direction)).to_array(),
        }),
        Curve3::Circle(mut circle) => {
            circle.plane.y_axis = (-Vec3::from(circle.plane.y_axis)).to_array();
            Curve3::Circle(circle)
        }
        Curve3::Ellipse(mut ellipse) => {
            ellipse.plane.y_axis = (-Vec3::from(ellipse.plane.y_axis)).to_array();
            Curve3::Ellipse(ellipse)
        }
        _ => return None,
    })
}

/// The parameters an edge from `start` to `end` runs between along `curve`.
fn span_of(
    curve: &Curve3,
    start: [f64; 3],
    end: [f64; 3],
    closed: bool,
    tolerance: f64,
) -> Option<(f64, f64)> {
    let from = curve.parameter_at(start);
    let mut to = curve.parameter_at(end);
    if matches!(curve, Curve3::Circle(_) | Curve3::Ellipse(_)) {
        to = from + (to - from).rem_euclid(TAU);
        if closed || to - from <= 1e-12 {
            to = from + TAU;
        }
    }
    let near = |parameter: f64, point: [f64; 3]| {
        Vec3::from(curve.point_at(parameter)).distance(Vec3::from(point)) <= tolerance * 4.0
    };
    (from.is_finite() && to.is_finite() && to > from && near(from, start) && near(to, end))
        .then_some((from, to))
}

fn lies_on(curve: &Curve3, span: (f64, f64), surface: &Surface, tolerance: f64) -> bool {
    (0..=4).all(|step| {
        let parameter = span.0 + (span.1 - span.0) * step as f64 / 4.0;
        let distance = surface.distance_to(curve.point_at(parameter));
        distance.is_finite() && distance.abs() <= tolerance * 8.0
    })
}

/// Drops every node the roots no longer reach.
fn prune(body: &mut Body) {
    let lumps: HashSet<_> = body.roots.iter().copied().collect();
    let shells: HashSet<_> = lumps
        .iter()
        .filter_map(|lump| body.lumps.get(*lump))
        .flat_map(|lump| lump.shells.iter().copied())
        .collect();
    let faces: HashSet<_> = shells
        .iter()
        .filter_map(|shell| body.shells.get(*shell))
        .flat_map(|shell| shell.faces.iter().copied())
        .collect();
    let loops: HashSet<LoopKey> = faces
        .iter()
        .filter_map(|face| body.faces.get(*face))
        .flat_map(|face| face.loops.iter().copied())
        .collect();
    let coedges: HashSet<CoedgeKey> = loops
        .iter()
        .filter_map(|ring| body.loops.get(*ring))
        .flat_map(|ring| ring.coedges.iter().copied())
        .collect();
    let edges: HashSet<EdgeKey> = coedges
        .iter()
        .filter_map(|coedge| body.coedges.get(*coedge).map(|coedge| coedge.edge))
        .collect();
    let vertices: HashSet<VertexKey> = edges
        .iter()
        .filter_map(|edge| body.edges.get(*edge))
        .flat_map(|edge| [edge.start, edge.end])
        .collect();
    let surfaces: HashSet<_> = faces
        .iter()
        .filter_map(|face| body.faces.get(*face).map(|face| face.surface))
        .collect();
    let curves: HashSet<_> = edges
        .iter()
        .filter_map(|edge| body.edges.get(*edge).map(|edge| edge.curve))
        .collect();
    macro_rules! keep {
        ($arena:ident, $set:ident) => {
            let gone: Vec<_> = body.$arena.keys().filter(|key| !$set.contains(key)).collect();
            for key in gone {
                body.$arena.remove(key);
            }
        };
    }
    keep!(lumps, lumps);
    keep!(shells, shells);
    keep!(faces, faces);
    keep!(loops, loops);
    keep!(coedges, coedges);
    keep!(edges, edges);
    keep!(vertices, vertices);
    keep!(surfaces, surfaces);
    keep!(curves, curves);
}

/// Deletes faces whose whole rim is holes in the faces around them — a
/// hole, a pocket, a boss — together with any face left bounded only by
/// them, such as a pocket's floor, and makes those faces whole again.
fn remove_feature(body: &Body, selected: &HashSet<FaceKey>) -> Option<Body> {
    let mut gone = selected.clone();
    loop {
        let enclosed: Vec<FaceKey> = body
            .face_keys()
            .filter(|face| !gone.contains(face))
            .filter(|face| {
                let around = neighbours(body, *face);
                !around.is_empty() && around.iter().all(|other| gone.contains(other))
            })
            .collect();
        if enclosed.is_empty() {
            break;
        }
        gone.extend(enclosed);
    }
    if gone.len() >= body.faces.len() {
        return None;
    }
    // Every coedge of a kept face across from a deleted one must be in a
    // hole loop made only of such coedges.
    let mut holes = Vec::new();
    for (face_key, face) in body.faces.iter() {
        if gone.contains(&face_key) {
            continue;
        }
        for (index, ring) in face.loops.iter().enumerate() {
            let coedges = &body.loops.get(*ring)?.coedges;
            let across = coedges
                .iter()
                .filter(|coedge| {
                    body.partner(**coedge)
                        .and_then(|partner| owner_face(body, partner))
                        .is_some_and(|other| gone.contains(&other))
                })
                .count();
            if across == 0 {
                continue;
            }
            if index == 0 || across != coedges.len() {
                return None;
            }
            holes.push((face_key, *ring));
        }
    }
    if holes.is_empty() {
        return None;
    }
    let mut result = body.clone();
    for (face, ring) in &holes {
        let node = result.faces.get_mut(*face)?;
        node.loops.retain(|other| other != ring);
        node.provenance.soil();
    }
    for face in &gone {
        let shell = result.faces.get(*face)?.owner;
        result.shells.get_mut(shell)?.faces.retain(|other| other != face);
    }
    prune(&mut result);
    let kept: HashSet<CoedgeKey> = result.coedges.keys().collect();
    for edge in result.edges.values_mut() {
        edge.coedges.retain(|coedge| kept.contains(coedge));
    }
    result.provenance.soil();
    (result.validate().is_empty() && result.edges.iter().all(|(_, edge)| edge.coedges.len() == 2))
        .then_some(result)
}

/// Deletes a blend — a strip of four sides between two faces, or a ring
/// between two closed edges — and extends the two faces it joined until
/// they meet.
fn collapse_blend(body: &Body, face: FaceKey) -> Option<Body> {
    let node = body.faces.get(face)?;
    let rings = node
        .loops
        .iter()
        .map(|ring| body.loops.get(*ring).map(|ring| ring.coedges.clone()))
        .collect::<Option<Vec<_>>>()?;
    match rings.as_slice() {
        [strip] if strip.len() == 4 => [0, 1]
            .into_iter()
            .find_map(|first| collapse_strip(body, face, strip, first)),
        [one, two] if one.len() == 1 && two.len() == 1 => collapse_ring(body, face, one[0], two[0]),
        _ => None,
    }
}

/// The coedge across an edge from `coedge`, and its face.
fn across(body: &Body, coedge: CoedgeKey) -> Option<(CoedgeKey, FaceKey)> {
    let partner = body.partner(coedge)?;
    Some((partner, owner_face(body, partner)?))
}

fn surfaces_meet(body: &Body, one: FaceKey, two: FaceKey, tolerance: f64) -> bool {
    let surface = |face: FaceKey| body.faces.get(face).and_then(|f| body.surfaces.get(f.surface));
    match (surface(one), surface(two)) {
        (Some(a), Some(b)) => matches!(super::intersect_surfaces(a, b, tolerance), Meeting::Curves(_)),
        _ => false,
    }
}

fn collapse_strip(body: &Body, face: FaceKey, strip: &[CoedgeKey], first: usize) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[body]);
    let [side_a, end_c, side_b, end_d] = [0, 1, 2, 3].map(|k| strip[(first + k) % 4]);
    let (keep_a, face_a) = across(body, side_a)?;
    let (keep_b, face_b) = across(body, side_b)?;
    let (cap_c, face_c) = across(body, end_c)?;
    let (cap_d, face_d) = across(body, end_d)?;
    let around = [face_a, face_b, face_c, face_d];
    if around.contains(&face) || face_a == face_b || !surfaces_meet(body, face_a, face_b, tolerance) {
        return None;
    }
    // Round the strip: side a runs p0 → p1, end c p1 → p2, side b p2 → p3,
    // end d p3 → p0. Closing it merges p2 into p1 and p3 into p0.
    let (p0, p1) = body.coedge_vertices(side_a)?;
    let (_, p2) = body.coedge_vertices(end_c)?;
    let (_, p3) = body.coedge_vertices(side_b)?;
    let estimate = |one: VertexKey, two: VertexKey| -> Option<[f64; 3]> {
        let a = Vec3::from(body.vertices.get(one)?.point);
        let b = Vec3::from(body.vertices.get(two)?.point);
        Some(a.lerp(b, 0.5).to_array())
    };
    let near_c = estimate(p1, p2)?;
    let near_d = estimate(p0, p3)?;
    let edge_a = body.coedges.get(side_a)?.edge;
    let edge_b = body.coedges.get(side_b)?.edge;
    let edge_c = body.coedges.get(end_c)?.edge;
    let edge_d = body.coedges.get(end_d)?.edge;

    let mut result = body.clone();
    // Face b now meets face a along edge a.
    let forward = !result.coedges.get(keep_a)?.forward;
    let coedge = result.coedges.get_mut(keep_b)?;
    coedge.edge = edge_a;
    coedge.forward = forward;
    result.edges.get_mut(edge_a)?.coedges = vec![keep_a, keep_b];
    // The ends lose their edge along the strip.
    for cap in [cap_c, cap_d] {
        let ring = result.coedges.get(cap)?.owner;
        result.loops.get_mut(ring)?.coedges.retain(|other| *other != cap);
        result.coedges.remove(cap);
    }
    for (gone, kept) in [(p2, p1), (p3, p0)] {
        if gone == kept {
            continue;
        }
        for edge in result.edges.values_mut() {
            if edge.start == gone {
                edge.start = kept;
            }
            if edge.end == gone {
                edge.end = kept;
            }
        }
        result.vertices.remove(gone);
    }
    for edge in [edge_b, edge_c, edge_d] {
        result.edges.remove(edge);
    }
    drop_face(&mut result, face)?;
    result.vertices.get_mut(p1)?.point = near_c;
    result.vertices.get_mut(p0)?.point = near_d;
    refit(body, result, &HashMap::new(), &HashSet::from([p0, p1]), &HashSet::from([edge_a]))
}

fn collapse_ring(body: &Body, face: FaceKey, one: CoedgeKey, two: CoedgeKey) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[body]);
    let (keep_a, face_a) = across(body, one)?;
    let (keep_b, face_b) = across(body, two)?;
    if face_a == face || face_b == face || face_a == face_b || !surfaces_meet(body, face_a, face_b, tolerance) {
        return None;
    }
    let edge_a = body.coedges.get(one)?.edge;
    let edge_b = body.coedges.get(two)?.edge;
    let kept = body.edges.get(edge_a)?.start;
    let gone = body.edges.get(edge_b)?.start;
    let estimate = Vec3::from(body.vertices.get(kept)?.point)
        .lerp(Vec3::from(body.vertices.get(gone)?.point), 0.5)
        .to_array();
    let mut result = body.clone();
    let forward = !result.coedges.get(keep_a)?.forward;
    let coedge = result.coedges.get_mut(keep_b)?;
    coedge.edge = edge_a;
    coedge.forward = forward;
    result.edges.get_mut(edge_a)?.coedges = vec![keep_a, keep_b];
    result.edges.remove(edge_b);
    if gone != kept {
        result.vertices.remove(gone);
    }
    drop_face(&mut result, face)?;
    result.vertices.get_mut(kept)?.point = estimate;
    refit(body, result, &HashMap::new(), &HashSet::from([kept]), &HashSet::from([edge_a]))
}

/// Removes a face, its loops and its coedges.
fn drop_face(body: &mut Body, face: FaceKey) -> Option<()> {
    let node = body.faces.remove(face)?;
    body.shells.get_mut(node.owner)?.faces.retain(|other| *other != face);
    for ring in node.loops {
        for coedge in body.loops.remove(ring)?.coedges {
            body.coedges.remove(coedge);
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Body {
        super::super::make::cuboid([0.0; 3], [10.0, 10.0, 10.0]).expect("a cube")
    }

    fn top(body: &Body) -> FaceKey {
        face_at_point(body, [5.0, 5.0, 10.0]).expect("the top face")
    }

    fn volume(body: &Body) -> f64 {
        super::super::mass_properties(body).expect("mass properties").volume
    }

    #[test]
    fn a_spline_face_has_a_certified_offset_and_a_limit() {
        use ocs_geom::geom2d::{Curve as Curve2, Line, NurbsCurve};
        let profile = vec![
            Curve2::Nurbs(NurbsCurve::new(2, vec![[0.0, 0.0], [5.0, 5.0], [10.0, 0.0]], Vec::new(), None).unwrap()),
            Curve2::Line(Line { start: [10.0, 0.0], end: [0.0, 0.0] }),
        ];
        let prism = super::super::sweep::extrude(ocs_geom::space::Plane::XY, &profile, [0.0, 0.0, 1.0]).unwrap();
        let wall = prism
            .face_keys()
            .find(|face| prism.surfaces.get(prism.faces.get(*face).unwrap().surface).unwrap().is_spline())
            .unwrap();
        // y = x - x² / 10 bends inward most at its crown, 1 / κ = 5.
        let limit = offset_limit(&prism, &[wall], -1.0).unwrap();
        assert!((limit - 5.0).abs() < 1e-6, "{limit}");
        assert!(offset_limit(&prism, &[wall], 1.0).unwrap().is_infinite());
        // Its offset surface is the base pushed out, certified to the
        // tolerance: the crown at y = 2.5 + 0.5.
        let surface = prism.surfaces.get(prism.faces.get(wall).unwrap().surface).unwrap();
        let along = if prism.faces.get(wall).unwrap().forward { 0.5 } else { -0.5 };
        let (Surface::Offset(offset), None) = offset_surface(surface, along, 1e-7).unwrap() else {
            panic!("an offset surface");
        };
        assert_eq!(offset.distance, along);
        let crown = Surface::Offset(offset.clone()).parameters_at([5.0, 3.0, 0.5]).unwrap();
        let point = Surface::Offset(offset).point_at(crown.0, crown.1);
        assert!(Vec3::from(point).distance(Vec3::new(5.0, 3.0, 0.5)) < 1e-7);
        // Pulled in past its crown's radius it folds, and is refused.
        assert!(offset_surface(surface, -6.0 * along / 0.5, 1e-7).is_none());
    }

    #[test]
    fn an_oblique_pull_drafts_a_side_to_the_angle_itself() {
        let body = cube();
        let side = face_at_point(&body, [10.0, 5.0, 5.0]).expect("the side");
        let (tilt, angle) = (0.4_f64, 0.1_f64);
        let pull = [0.0, tilt.sin(), tilt.cos()];
        let drafted = draft_faces(&body, &[side], [0.0; 3], [0.0, 0.0, 1.0], pull, angle).expect("draft");
        let face = face_at_point(&drafted, [9.9, 5.0, 5.0]).expect("the drafted side");
        let node = drafted.faces.get(face).unwrap();
        let Some(Surface::Plane(plane)) = drafted.surfaces.get(node.surface) else {
            panic!("a plane");
        };
        let normal = Vec3::from(plane.normal().unwrap()) * if node.forward { 1.0 } else { -1.0 };
        assert!((normal.dot(Vec3::from(pull)) - angle.sin()).abs() < 1e-12);
        // Hinged on z = 0, turned by φ with sin φ = sin θ / cos(tilt).
        let turn = (angle.sin() / tilt.cos()).asin();
        assert!((volume(&drafted) - (1000.0 - 500.0 * turn.tan())).abs() < 1e-8);
    }

    #[test]
    fn a_drafted_pin_becomes_a_frustum_and_a_drafted_hole_widens() {
        let pin = super::super::make::cylinder([0.0; 3], 2.0, 4.0).expect("a pin");
        let wall = face_at_point(&pin, [2.0, 0.0, 2.0]).expect("the wall");
        let angle = 0.1_f64;
        let drafted = taper_faces(&pin, &[wall], [0.0; 3], [0.0, 0.0, 1.0], angle).expect("draft");
        let top = 2.0 - 4.0 * angle.tan();
        let frustum = std::f64::consts::PI * 4.0 / 3.0 * (4.0 + 2.0 * top + top * top);
        assert!((volume(&drafted) - frustum).abs() < 1e-8, "{}", volume(&drafted));

        let block = super::super::make::cuboid([-5.0, -5.0, 0.0], [10.0, 10.0, 4.0]).unwrap();
        let bore = super::super::make::cylinder([0.0, 0.0, -1.0], 2.0, 6.0).unwrap();
        let holed = super::super::combine(block, bore, Operation::Difference, 1e-9).unwrap();
        let wall = face_at_point(&holed, [2.0, 0.0, 2.0]).expect("the bore");
        let drafted = taper_faces(&holed, &[wall], [0.0; 3], [0.0, 0.0, 1.0], angle).expect("draft");
        let top = 2.0 + 4.0 * angle.tan();
        let frustum = std::f64::consts::PI * 4.0 / 3.0 * (4.0 + 2.0 * top + top * top);
        assert!((volume(&drafted) - (400.0 - frustum)).abs() < 1e-8, "{}", volume(&drafted));
    }

    #[test]
    fn moving_offsetting_and_tapering_a_face_refits_its_neighbours() {
        let body = cube();
        let raised = move_faces(&body, &[top(&body)], &Placement::at([0.0, 0.0, 5.0])).expect("moved");
        assert!((volume(&raised) - 1500.0).abs() < 1e-6);
        let thinner = offset_faces(&body, &[top(&body)], -4.0).expect("offset");
        assert!((volume(&thinner) - 600.0).abs() < 1e-6);
        let side = face_at_point(&body, [10.0, 5.0, 5.0]).expect("a side");
        let tapered = taper_faces(&body, &[side], [0.0; 3], [0.0, 0.0, 1.0], 0.1).expect("taper");
        let lean = 10.0 * 0.1f64.tan();
        assert!((volume(&tapered) - (1000.0 - 0.5 * lean * 10.0 * 10.0)).abs() < 1e-6);
    }

    #[test]
    fn a_face_moved_past_its_neighbours_is_refused() {
        let body = cube();
        assert!(offset_faces(&body, &[top(&body)], -12.0).is_none());
    }

    fn edge_near(body: &Body, point: [f64; 3]) -> EdgeKey {
        body.edge_keys()
            .min_by(|a, b| {
                let gap = |edge: &EdgeKey| {
                    let (start, end) = body.edge_endpoints(*edge).expect("an edge");
                    Vec3::from(point).distance_to_segment(Vec3::from(start), Vec3::from(end))
                };
                gap(a).total_cmp(&gap(b))
            })
            .expect("edges")
    }

    #[test]
    fn deleting_a_fillet_or_a_hole_gives_the_plain_block_back() {
        let body = cube();
        let rounded = super::super::fillet_edges(&body, &[edge_near(&body, [5.0, 10.0, 10.0])], 2.0)
            .expect("a fillet");
        let round = face_at_point(&rounded, [5.0, 10.0 - 2.0 + 2.0 * 0.5f64.sqrt(), 10.0 - 2.0 + 2.0 * 0.5f64.sqrt()])
            .expect("the fillet face");
        let sharp = delete_faces(&rounded, &[round]).expect("fillet deleted");
        assert!((volume(&sharp) - 1000.0).abs() < 1e-6);
        assert_eq!(sharp.faces.len(), 6);

        let drill = super::super::make::cylinder([5.0, 5.0, -1.0], 2.0, 12.0).expect("a drill");
        let tolerance = super::super::operation_tolerance(&[&body]);
        let drilled = super::super::combine(body.clone(), drill, Operation::Difference, tolerance)
            .expect("a hole");
        let wall = face_at_point(&drilled, [7.0, 5.0, 5.0]).expect("the hole's wall");
        let filled = delete_faces(&drilled, &[wall]).expect("hole deleted");
        assert!((volume(&filled) - 1000.0).abs() < 1e-6);
        let drilled_volume = volume(&drilled);
        let shifted = move_faces(&drilled, &[wall], &Placement::at([2.0, 1.0, 0.0])).expect("hole moved");
        assert!((volume(&shifted) - drilled_volume).abs() < 1e-6);
        let wider = offset_faces(&drilled, &[wall], -1.0).expect("hole widened");
        let expected = 1000.0 - std::f64::consts::PI * 9.0 * 10.0;
        assert!((volume(&wider) - expected).abs() < 1e-3, "{}", volume(&wider));
        let side = face_at_point(&body, [10.0, 5.0, 5.0]).expect("a side");
        let turned = rotation([10.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.1).expect("a turn");
        assert!(move_faces(&body, &[side], &turned).is_some());
    }

    #[test]
    fn separating_extruding_and_copying_faces() {
        let body = cube();
        let tolerance = super::super::operation_tolerance(&[&body]);
        let apart = super::super::make::cuboid([20.0, 0.0, 0.0], [5.0, 5.0, 5.0]).expect("a block");
        let both = super::super::combine(body.clone(), apart, Operation::Union, tolerance).expect("two lumps");
        let pieces = separate(&both);
        assert_eq!(pieces.len(), 2);
        let mut volumes: Vec<f64> = pieces.iter().map(volume).collect();
        volumes.sort_by(f64::total_cmp);
        assert!((volumes[0] - 125.0).abs() < 1e-6 && (volumes[1] - 1000.0).abs() < 1e-6);

        let taller = extrude_faces(&body, &[top(&body)], 5.0, 0.0).expect("extruded");
        assert!((volume(&taller) - 1500.0).abs() < 1e-6);
        let sheet = face_sheet(&body, &[top(&body)]).expect("a copied face");
        assert_eq!(sheet.faces.len(), 1);
        assert!(check(&body).is_empty());
    }

    #[test]
    fn a_line_across_a_face_imprints_it() {
        let body = cube();
        let line = Curve3::Line(Line3 { origin: [0.0, 5.0, 10.0], direction: [1.0, 0.0, 0.0] });
        let cut = imprint_curve(&body, &line, (0.0, 10.0)).expect("imprinted");
        assert_eq!(cut.faces.len(), 7);
        assert!(imprint_curve(&body, &line, (2.0, 8.0)).is_none());
        assert_eq!(clean(&cut).faces.len(), 6);
    }
}
