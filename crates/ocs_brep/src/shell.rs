//! Hollowing exact analytic solids.
//!
//! A shell is the material between an exterior and an offset cavity.  A
//! removed face opens that cavity through the corresponding side instead of
//! leaving a cap.  Construction stays in the kernel so every application and
//! file-format consumer receives the same topology and refusal rules.

use super::{Body, FaceKey, Operation, Placement, Snag, Surface};
use ocs_geom::space::Vec3;
use std::collections::{HashMap, HashSet};

/// Why a solid could not be hollowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellError {
    InvalidDistance,
    UnsupportedSolid,
    UnknownFace,
    NoMaterial,
    Kernel(Snag),
}

impl From<Snag> for ShellError {
    fn from(value: Snag) -> Self {
        Self::Kernel(value)
    }
}

#[derive(Clone, Copy)]
struct BoxFace {
    axis: usize,
    maximum: bool,
}

struct BoxShape {
    origin: [f64; 3],
    axes: [[f64; 3]; 3],
    size: [f64; 3],
    faces: HashMap<FaceKey, BoxFace>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CylinderFace {
    Side,
    Bottom,
    Top,
}

struct CylinderShape {
    origin: [f64; 3],
    axes: [[f64; 3]; 3],
    radius: f64,
    height: f64,
    faces: HashMap<FaceKey, CylinderFace>,
}

struct SphereShape {
    centre: [f64; 3],
    radius: f64,
    face: FaceKey,
}

/// Hollow `body` by `distance`, opening the cavity through `removed_faces`.
///
/// Positive distances keep the exterior fixed and offset the cavity inward.
/// Negative distances keep the original body as the cavity and grow a new
/// exterior.  Exact rectangular boxes, circular cylinders and spheres are
/// supported; other surface sets are refused without changing the input.
pub fn shell(
    body: &Body,
    removed_faces: &[FaceKey],
    distance: f64,
) -> Result<Body, ShellError> {
    if !distance.is_finite() || distance == 0.0 {
        return Err(ShellError::InvalidDistance);
    }
    let removed = removed_faces.iter().copied().collect::<HashSet<_>>();
    if removed.len() != removed_faces.len()
        || removed.iter().any(|face| !body.faces.contains(*face))
    {
        return Err(ShellError::UnknownFace);
    }
    // Solidcore's shell first, which takes any solid; this kernel's boxes, cylinders and
    // spheres for what it refuses. Its thickness is negative inward: this one's distance is
    // positive inward.
    if let Ok(hollow) = super::bridge::shell(body, removed_faces, -distance) {
        return Ok(hollow);
    }
    if body.roots.len() != 1 || body.lumps.len() != 1 || body.shells.len() != 1 {
        return Err(ShellError::UnsupportedSolid);
    }

    let result = if let Some(shape) = recognize_box(body) {
        shell_box(body, &shape, &removed, distance)?
    } else if let Some(shape) = recognize_cylinder(body) {
        shell_cylinder(body, &shape, &removed, distance)?
    } else if let Some(shape) = recognize_sphere(body) {
        shell_sphere(body, &shape, &removed, distance)?
    } else if let Some(result) = shell_prismatic(body, &removed, distance) {
        result?
    } else if let Some(result) = shell_revolved(body, &removed, distance) {
        result?
    } else {
        match shell_polyhedral(body, &removed, distance) {
            Err(ShellError::UnsupportedSolid) => {
                shell_by_offset(body, &removed, distance).ok_or(ShellError::UnsupportedSolid)?
            }
            result => result?,
        }
    };
    if result.face_keys().next().is_none() || result.roots.is_empty() {
        return Err(ShellError::NoMaterial);
    }
    Ok(result)
}

/// Resolve a surface pick to a face supported by [`shell`].
pub fn shell_face_at_point(body: &Body, point: [f64; 3]) -> Option<FaceKey> {
    if point.iter().any(|value| !value.is_finite())
        || (recognize_box(body).is_none()
            && recognize_cylinder(body).is_none()
            && recognize_sphere(body).is_none())
    {
        return None;
    }
    body.face_keys()
        .filter_map(|key| {
            let face = body.faces.get(key)?;
            let surface = body.surfaces.get(face.surface)?;
            Some((key, surface.distance_to(point).abs()))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(key, _)| key)
}

fn shell_box(
    body: &Body,
    shape: &BoxShape,
    removed: &HashSet<FaceKey>,
    distance: f64,
) -> Result<Body, ShellError> {
    let mut open = [[false; 2]; 3];
    for face in removed {
        let Some(tag) = shape.faces.get(face) else {
            return Err(ShellError::UnknownFace);
        };
        open[tag.axis][usize::from(tag.maximum)] = true;
    }
    if removed.len() == shape.faces.len() {
        return Err(ShellError::NoMaterial);
    }
    let thickness = distance.abs();
    let span = shape.size.iter().copied().fold(0.0, f64::max);
    let extension = span + thickness * 2.0;

    let (outer_min, outer_size, cavity_min, cavity_size) = if distance > 0.0 {
        let mut cavity_min = [0.0; 3];
        let mut cavity_max = [0.0; 3];
        for axis in 0..3 {
            cavity_min[axis] = if open[axis][0] { -extension } else { thickness };
            cavity_max[axis] = if open[axis][1] {
                shape.size[axis] + extension
            } else {
                shape.size[axis] - thickness
            };
            if cavity_max[axis] <= cavity_min[axis] {
                return Err(ShellError::NoMaterial);
            }
        }
        (
            [0.0; 3],
            shape.size,
            cavity_min,
            std::array::from_fn(|axis| cavity_max[axis] - cavity_min[axis]),
        )
    } else {
        let outer_min = [-thickness; 3];
        let outer_size = std::array::from_fn(|axis| shape.size[axis] + 2.0 * thickness);
        let mut cavity_min = [0.0; 3];
        let mut cavity_max = shape.size;
        for axis in 0..3 {
            if open[axis][0] {
                cavity_min[axis] = -extension;
            }
            if open[axis][1] {
                cavity_max[axis] = shape.size[axis] + extension;
            }
        }
        (
            outer_min,
            outer_size,
            cavity_min,
            std::array::from_fn(|axis| cavity_max[axis] - cavity_min[axis]),
        )
    };

    let outer = placed_box(shape, outer_min, outer_size)?;
    let cavity = placed_box(shape, cavity_min, cavity_size)?;
    subtract(outer, cavity, body)
}

fn placed_box(shape: &BoxShape, local_origin: [f64; 3], size: [f64; 3]) -> Result<Body, ShellError> {
    let local = super::make::cuboid(local_origin, size).ok_or(ShellError::NoMaterial)?;
    super::place::transform(
        &local,
        &Placement {
            x_axis: shape.axes[0],
            y_axis: shape.axes[1],
            z_axis: shape.axes[2],
            origin: shape.origin,
        },
    )
    .ok_or(ShellError::UnsupportedSolid)
}

fn shell_cylinder(
    body: &Body,
    shape: &CylinderShape,
    removed: &HashSet<FaceKey>,
    distance: f64,
) -> Result<Body, ShellError> {
    let mut side_open = false;
    let mut bottom_open = false;
    let mut top_open = false;
    for face in removed {
        match shape.faces.get(face).copied().ok_or(ShellError::UnknownFace)? {
            CylinderFace::Side => side_open = true,
            CylinderFace::Bottom => bottom_open = true,
            CylinderFace::Top => top_open = true,
        }
    }
    if side_open && bottom_open && top_open {
        return Err(ShellError::NoMaterial);
    }
    let thickness = distance.abs();
    let extension = shape.radius.max(shape.height) + thickness * 2.0;
    let (outer_radius, outer_z, outer_height, cavity_radius, cavity_z, cavity_height) =
        if distance > 0.0 {
            let cavity_radius = if side_open {
                shape.radius + extension
            } else {
                shape.radius - thickness
            };
            let cavity_z = if bottom_open { -extension } else { thickness };
            let cavity_top = if top_open {
                shape.height + extension
            } else {
                shape.height - thickness
            };
            (
                shape.radius,
                0.0,
                shape.height,
                cavity_radius,
                cavity_z,
                cavity_top - cavity_z,
            )
        } else {
            let cavity_radius = if side_open {
                shape.radius + thickness + extension
            } else {
                shape.radius
            };
            let cavity_z = if bottom_open { -extension } else { 0.0 };
            let cavity_top = if top_open {
                shape.height + extension
            } else {
                shape.height
            };
            (
                shape.radius + thickness,
                -thickness,
                shape.height + 2.0 * thickness,
                cavity_radius,
                cavity_z,
                cavity_top - cavity_z,
            )
        };
    if outer_radius <= 0.0
        || outer_height <= 0.0
        || cavity_radius <= 0.0
        || cavity_height <= 0.0
    {
        return Err(ShellError::NoMaterial);
    }
    let outer = placed_cylinder(shape, outer_radius, outer_z, outer_height)?;
    let cavity = placed_cylinder(shape, cavity_radius, cavity_z, cavity_height)?;
    subtract(outer, cavity, body)
}

fn placed_cylinder(
    shape: &CylinderShape,
    radius: f64,
    local_z: f64,
    height: f64,
) -> Result<Body, ShellError> {
    let local = super::make::cylinder([0.0, 0.0, local_z], radius, height)
        .ok_or(ShellError::NoMaterial)?;
    super::place::transform(
        &local,
        &Placement {
            x_axis: shape.axes[0],
            y_axis: shape.axes[1],
            z_axis: shape.axes[2],
            origin: shape.origin,
        },
    )
    .ok_or(ShellError::UnsupportedSolid)
}

fn shell_sphere(
    body: &Body,
    shape: &SphereShape,
    removed: &HashSet<FaceKey>,
    distance: f64,
) -> Result<Body, ShellError> {
    if !removed.is_empty() {
        return if removed.contains(&shape.face) {
            Err(ShellError::NoMaterial)
        } else {
            Err(ShellError::UnknownFace)
        };
    }
    let thickness = distance.abs();
    let (outer_radius, cavity_radius) = if distance > 0.0 {
        (shape.radius, shape.radius - thickness)
    } else {
        (shape.radius + thickness, shape.radius)
    };
    if outer_radius <= 0.0 || cavity_radius <= 0.0 {
        return Err(ShellError::NoMaterial);
    }
    let outer = super::make::sphere(shape.centre, outer_radius).ok_or(ShellError::NoMaterial)?;
    let cavity = super::make::sphere(shape.centre, cavity_radius).ok_or(ShellError::NoMaterial)?;
    subtract(outer, cavity, body)
}

/// A solid run straight between two parallel caps — its walls planes and
/// cylinders square to them, a plate with holes, a wall bent round an arc:
/// the cavity is the cap's region offset in by the thickness, run between
/// the caps moved in, or out through a removed cap. Offset in the plane,
/// a wall narrower than the shell closes up there and a hole's wall grows
/// round it as the region allows. Not taken (`None`) for any other solid, or
/// where a wall is to be opened.
#[cfg(feature = "offset")]
fn shell_prismatic(
    body: &Body,
    removed: &HashSet<FaceKey>,
    distance: f64,
) -> Option<Result<Body, ShellError>> {
    use ocs_geom::geom2d::{Curve, Polyline};
    let outward = |face: FaceKey| -> Option<Vec3> {
        let node = body.faces.get(face)?;
        let Some(Surface::Plane(plane)) = body.surfaces.get(node.surface) else {
            return None;
        };
        Some(Vec3::from(plane.normal()?) * if node.forward { 1.0 } else { -1.0 })
    };
    // The caps: a plane, the one plane facing the other way, and walls
    // square to both.
    let caps = body.face_keys().find_map(|top| {
        let normal = outward(top)?;
        let mut bottom = None;
        for face in body.face_keys().filter(|face| *face != top) {
            match body.faces.get(face).and_then(|node| body.surfaces.get(node.surface))? {
                Surface::Plane(_) => {
                    let along = outward(face)?.dot(normal);
                    if along < -1.0 + 1e-9 && bottom.is_none() {
                        bottom = Some(face);
                    } else if along.abs() > 1e-9 {
                        return None;
                    }
                }
                Surface::Cylinder(cylinder) => {
                    if Vec3::from(cylinder.base.normal()?).dot(normal).abs() < 1.0 - 1e-9 {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        Some((top, bottom?, normal))
    });
    let (top, bottom, normal) = caps?;
    if removed.iter().any(|face| *face != top && *face != bottom) {
        return None;
    }
    let profile = super::planar_face_profile(body, top)?;
    let floor = body.faces.get(bottom).and_then(|node| body.surfaces.get(node.surface));
    let Some(Surface::Plane(floor)) = floor else {
        return None;
    };
    let height = (Vec3::from(profile.plane.origin) - Vec3::from(floor.origin)).dot(normal);
    let rings = profile.loops.iter().map(|ring| ring_polyline(ring)).collect::<Option<Vec<_>>>()?;
    let tolerance = super::operation_tolerance(&[body]);
    let thickness = distance.abs();
    let region: Vec<Curve> = rings.iter().cloned().map(Curve::Polyline).collect();
    let in_region = |point: [f64; 2]| {
        ocs_geom::geom2d::contains(&region, point, ocs_geom::geom2d::Tolerance::new(tolerance))
    };
    // Each ring moved off its material, or into it.
    let moved = |ring: &Polyline, into: bool| -> Vec<Polyline> {
        let curve = Curve::Polyline(ring.clone());
        let pieces = curve.segment_count().max(1) as f64;
        let (at, ahead) = (curve.point_at(0.5 / pieces), curve.point_at(0.5 / pieces + 1e-6));
        let along = [ahead[0] - at[0], ahead[1] - at[1]];
        let length = along[0].hypot(along[1]);
        if length == 0.0 {
            return Vec::new();
        }
        let reach = thickness * 1e-3;
        let left = [at[0] - along[1] / length * reach, at[1] + along[0] / length * reach];
        let right = [at[0] + along[1] / length * reach, at[1] - along[0] / length * reach];
        let side = if in_region(left) == into { left } else { right };
        ocs_geom::geom2d::offset_polyline(ring, thickness, side)
    };
    // A region's ring run along the cap's normal between two depths below
    // the top cap.
    let run = |ring: &Polyline, from: f64, to: f64| -> Option<Body> {
        let mut plane = profile.plane;
        plane.origin = (Vec3::from(plane.origin) - normal * from).to_array();
        let along = (normal * (from - to)).to_array();
        super::extrude_region(plane, &[vec![Curve::Polyline(ring.clone())]], along)
    };
    // The region of `outer` rings less `holes` rings, run between depths.
    let solid = |outer: &[Polyline], holes: &[Polyline], from: f64, to: f64| -> Option<Body> {
        let mut pieces = outer.iter().map(|ring| run(ring, from, to));
        let mut result = pieces.next()??;
        for piece in pieces {
            result = super::combine(result, piece?, Operation::Union, tolerance).ok()?;
        }
        for hole in holes {
            result = super::combine(result, run(hole, from, to)?, Operation::Difference, tolerance)
                .ok()?;
        }
        Some(result)
    };
    let (open_top, open_bottom) = (removed.contains(&top), removed.contains(&bottom));
    let through = thickness * 2.0;
    let built = if distance > 0.0 {
        let outer = moved(&rings[0], true);
        if outer.is_empty() {
            return Some(Err(ShellError::NoMaterial));
        }
        let holes: Vec<Polyline> = rings[1..].iter().flat_map(|ring| moved(ring, true)).collect();
        let (from, to) = (
            if open_top { -through } else { thickness },
            if open_bottom { height + through } else { height - thickness },
        );
        if to <= from {
            return Some(Err(ShellError::NoMaterial));
        }
        solid(&outer, &holes, from, to).map(|cavity| (body.clone(), cavity))
    } else {
        let outer = moved(&rings[0], false);
        let holes: Vec<Polyline> = rings[1..].iter().flat_map(|ring| moved(ring, false)).collect();
        let exterior = solid(&outer, &holes, -thickness, height + thickness);
        let cavity = if removed.is_empty() {
            Some(body.clone())
        } else {
            solid(
                &rings[..1],
                &rings[1..],
                if open_top { -through - thickness } else { 0.0 },
                if open_bottom { height + through + thickness } else { height },
            )
        };
        exterior.zip(cavity)
    };
    let Some((exterior, cavity)) = built else {
        return Some(Err(ShellError::UnsupportedSolid));
    };
    Some(hollowed(exterior, cavity, body, removed.is_empty()))
}

#[cfg(not(feature = "offset"))]
fn shell_prismatic(_: &Body, _: &HashSet<FaceKey>, _: f64) -> Option<Result<Body, ShellError>> {
    None
}

/// A solid of revolution — planes square to one axis, and cylinders, cones,
/// spheres and tori round it — hollowed in its half section: the section
/// offset in the plane through the axis and turned back round it. The
/// section is offset whole, mirrored across the axis, so the axis is no wall
/// to move off; the half on the near side is what turns. A removed face,
/// which must be a plane square to the axis, runs the cavity out through it.
#[cfg(feature = "offset")]
fn shell_revolved(
    body: &Body,
    removed: &HashSet<FaceKey>,
    distance: f64,
) -> Option<Result<Body, ShellError>> {
    use ocs_geom::geom2d::{Curve, Polyline, PolylineVertex};
    let (origin, along) = body.face_keys().find_map(|face| {
        match body.surfaces.get(body.faces.get(face)?.surface)? {
            Surface::Cylinder(cylinder) => Some((cylinder.base.origin, cylinder.base.normal()?)),
            Surface::Cone(cone) => Some((cone.base.origin, cone.base.normal()?)),
            Surface::Torus(torus) => Some((torus.frame.origin, torus.frame.normal()?)),
            _ => None,
        }
    })?;
    let (origin, along) = (Vec3::from(origin), Vec3::from(along).normalize()?);
    let bounds = super::body_bounds(body)?;
    let size = Vec3::from(bounds.min).distance(Vec3::from(bounds.max));
    let fit = super::operation_tolerance(&[body]).max(size * 1e-7);
    let section = |point: [f64; 3]| -> [f64; 2] {
        let offset = Vec3::from(point) - origin;
        let height = offset.dot(along);
        [(offset - along * height).length(), height]
    };
    let square = |normal: Option<[f64; 3]>| {
        normal
            .and_then(|normal| Vec3::from(normal).normalize())
            .is_some_and(|normal| normal.cross(along).length() <= 1e-9)
    };
    let on_axis = |point: [f64; 3]| section(point)[0] <= fit;
    // Each face's stretch of the section: its rims, joined along the face.
    struct Piece {
        ends: [[f64; 2]; 2],
        // The circle a round stretch lies on, and a point of it between the
        // ends, which says which way round.
        round: Option<([f64; 2], [f64; 2])>,
        face: FaceKey,
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for face in body.face_keys() {
        let node = body.faces.get(face)?;
        let surface = body.surfaces.get(node.surface)?;
        let mut rims: Vec<[f64; 2]> = Vec::new();
        for coedge in body.face_coedges(face) {
            let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
            // A seam, the face meeting itself along a meridian, is no rim.
            let seam = edge.coedges.len() == 2
                && edge.coedges.iter().all(|use_| {
                    body.coedges
                        .get(*use_)
                        .and_then(|use_| body.loops.get(use_.owner))
                        .is_some_and(|ring| ring.owner == face)
                });
            if seam {
                continue;
            }
            let super::Curve3::Circle(circle) = body.curves.get(edge.curve)? else {
                return None;
            };
            if !square(circle.plane.normal()) || !on_axis(circle.plane.origin) {
                return None;
            }
            let rim = [circle.radius, section(circle.plane.origin)[1]];
            if !rims.iter().any(|seen| (seen[0] - rim[0]).hypot(seen[1] - rim[1]) <= fit) {
                rims.push(rim);
            }
        }
        let inside = || super::boolean::interior_point(body, face, fit).map(section);
        let piece = |ends: [[f64; 2]; 2], round: Option<([f64; 2], [f64; 2])>| Piece {
            ends,
            round,
            face,
        };
        pieces.push(match (surface, rims.as_slice()) {
            (Surface::Plane(plane), [rim, rest @ ..]) if square(plane.normal()) => {
                let other = rest.first().copied().unwrap_or([0.0, rim[1]]);
                if rest.len() > 1 || (other[1] - rim[1]).abs() > fit {
                    return None;
                }
                piece([*rim, other], None)
            }
            (Surface::Cylinder(cylinder), [one, two])
                if square(cylinder.base.normal()) && on_axis(cylinder.base.origin) =>
            {
                piece([*one, *two], None)
            }
            (Surface::Cone(cone), [one, rest @ ..])
                if square(cone.base.normal()) && on_axis(cone.base.origin) && rest.len() <= 1 =>
            {
                let other = match rest.first() {
                    Some(two) => *two,
                    None => {
                        // The apex, on whichever side the rim's slope runs to.
                        let base = section(cone.base.origin)[1];
                        let slope = cone.half_angle.tan();
                        let sign = Vec3::from(cone.base.normal()?).dot(along).signum();
                        let apex = base + sign * cone.radius / slope;
                        let along_rim = cone.radius - (one[1] - base) * sign * slope;
                        if (along_rim - one[0]).abs() > fit * 10.0 || !apex.is_finite() {
                            return None;
                        }
                        [0.0, apex]
                    }
                };
                piece([*one, other], None)
            }
            (Surface::Sphere(sphere), [one, rest @ ..])
                if on_axis(sphere.frame.origin) && rest.len() <= 1 =>
            {
                let centre = [0.0, section(sphere.frame.origin)[1]];
                let through = inside()?;
                let other = match rest.first() {
                    Some(two) => *two,
                    None => {
                        let up = through[1] > one[1];
                        [0.0, centre[1] + if up { sphere.radius } else { -sphere.radius }]
                    }
                };
                piece([*one, other], Some((centre, through)))
            }
            (Surface::Torus(torus), [one, two])
                if square(torus.frame.normal()) && on_axis(torus.frame.origin) =>
            {
                let centre = [torus.major_radius.abs(), section(torus.frame.origin)[1]];
                piece([*one, *two], Some((centre, inside()?)))
            }
            _ => return None,
        });
    }
    if removed.iter().any(|face| {
        !pieces
            .iter()
            .any(|piece| piece.face == *face && piece.round.is_none() && {
                let [a, b] = piece.ends;
                (a[1] - b[1]).abs() <= fit
            })
    }) {
        return None;
    }
    // The pieces chained into one walk: axis to axis, or round on itself.
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= fit * 10.0;
    let start = pieces
        .iter()
        .position(|piece| piece.ends.iter().any(|end| end[0] <= fit))
        .unwrap_or(0);
    let mut used = vec![false; pieces.len()];
    let mut walk: Vec<(usize, bool)> = Vec::new();
    let first = &pieces[start];
    let forward = first.ends[0][0] <= fit || first.ends[1][0] > fit;
    used[start] = true;
    walk.push((start, forward));
    let mut at = first.ends[usize::from(forward)];
    let touches_axis = first.ends.iter().any(|end| end[0] <= fit);
    loop {
        if touches_axis && at[0] <= fit {
            break;
        }
        let Some((next, forward)) = pieces.iter().enumerate().find_map(|(index, piece)| {
            if used[index] {
                return None;
            }
            if near(piece.ends[0], at) {
                Some((index, true))
            } else if near(piece.ends[1], at) {
                Some((index, false))
            } else {
                None
            }
        }) else {
            break;
        };
        used[next] = true;
        walk.push((next, forward));
        at = pieces[next].ends[usize::from(forward)];
    }
    let closed_ring = !touches_axis && near(at, pieces[start].ends[usize::from(!forward)]);
    if used.iter().any(|done| !done) || !(touches_axis && at[0] <= fit || closed_ring) {
        return None;
    }
    // The walk as a polyline, rounds as bulges.
    let angle =
        |centre: [f64; 2], point: [f64; 2]| (point[1] - centre[1]).atan2(point[0] - centre[0]);
    let mut half: Vec<PolylineVertex> = Vec::new();
    for (index, forward) in &walk {
        let piece = &pieces[*index];
        let (from, to) = if *forward {
            (piece.ends[0], piece.ends[1])
        } else {
            (piece.ends[1], piece.ends[0])
        };
        let bulge = match piece.round {
            None => 0.0,
            Some((centre, through)) => {
                let turn = std::f64::consts::TAU;
                let start = angle(centre, from);
                let sweep = (angle(centre, to) - start).rem_euclid(turn);
                let passed = (angle(centre, through) - start).rem_euclid(turn);
                (if passed < sweep { sweep } else { sweep - turn } / 4.0).tan()
            }
        };
        half.push(PolylineVertex { position: from, bulge });
    }
    let ring = if touches_axis {
        // Mirrored across the axis and walked back: a reflection and a
        // reversal leave each bulge as it was.
        let mut vertices = half.clone();
        vertices.push(PolylineVertex { position: at, bulge: half[half.len() - 1].bulge });
        for index in (1..half.len()).rev() {
            let [r, z] = half[index].position;
            vertices.push(PolylineVertex { position: [-r, z], bulge: half[index - 1].bulge });
        }
        Polyline { vertices, closed: true }
    } else {
        Polyline { vertices: half.clone(), closed: true }
    };
    let thickness = distance.abs();
    let region = [Curve::Polyline(ring.clone())];
    let tolerance = ocs_geom::geom2d::Tolerance::new(fit);
    // A side of the ring a hair off its first stretch: in, or out.
    let side = |into: bool| -> [f64; 2] {
        let curve = Curve::Polyline(ring.clone());
        let pieces = curve.segment_count().max(1) as f64;
        let (at, ahead) = (curve.point_at(0.5 / pieces), curve.point_at(0.5 / pieces + 1e-6));
        let along = [ahead[0] - at[0], ahead[1] - at[1]];
        let length = along[0].hypot(along[1]).max(f64::MIN_POSITIVE);
        let reach = thickness * 1e-3;
        let left = [at[0] - along[1] / length * reach, at[1] + along[0] / length * reach];
        let right = [at[0] + along[1] / length * reach, at[1] - along[0] / length * reach];
        if ocs_geom::geom2d::contains(&region, left, tolerance) == into { left } else { right }
    };
    // The near half of a ring mirrored across the axis, closed along it.
    let near_half = |ring: &Polyline| -> Option<Polyline> {
        let count = ring.vertices.len();
        // Where a stretch leaves the near side for the far one or comes
        // back, and the bulges of its two parts there.
        let crossing = |index: usize| -> Option<([f64; 2], f64, f64)> {
            let from = ring.vertices[index];
            let to = ring.vertices[(index + 1) % count].position;
            if (from.position[0] > 0.0) == (to[0] > 0.0) {
                return None;
            }
            let Some(arc) = ring.segment_arc(index) else {
                let t = from.position[0] / (from.position[0] - to[0]);
                return Some(([0.0, from.position[1] + (to[1] - from.position[1]) * t], 0.0, 0.0));
            };
            if to[0] == 0.0 {
                return Some((to, from.bulge, 0.0));
            }
            if from.position[0] == 0.0 {
                return Some((from.position, 0.0, from.bulge));
            }
            let cosine = (-arc.center[0] / arc.radius).clamp(-1.0, 1.0);
            let turn = std::f64::consts::TAU;
            [cosine.acos(), -cosine.acos()].into_iter().find_map(|at| {
                let into = if arc.sweep > 0.0 {
                    (at - arc.start_angle).rem_euclid(turn)
                } else {
                    -(arc.start_angle - at).rem_euclid(turn)
                };
                (into.abs() < arc.sweep.abs()).then(|| {
                    let point = [0.0, arc.center[1] + arc.radius * at.sin()];
                    (point, (into / 4.0).tan(), ((arc.sweep - into) / 4.0).tan())
                })
            })
        };
        let crossings: Vec<usize> = (0..count).filter(|index| crossing(*index).is_some()).collect();
        match crossings.as_slice() {
            [] => ring.vertices.iter().all(|vertex| vertex.position[0] > 0.0).then(|| ring.clone()),
            [enter, leave] => {
                // Out of the far side at one, back from the near side at the
                // other: the near stretch between them, closed on the axis.
                let (one, two) = (crossing(*enter)?, crossing(*leave)?);
                let near_first = ring.vertices[(*enter + 1) % count].position[0] > 0.0;
                let (from, to, start, end) = if near_first {
                    (*enter, *leave, one, two)
                } else {
                    (*leave, *enter, two, one)
                };
                let mut vertices = vec![PolylineVertex { position: start.0, bulge: start.2 }];
                let mut index = (from + 1) % count;
                while index != to {
                    vertices.push(ring.vertices[index]);
                    index = (index + 1) % count;
                }
                vertices.push(PolylineVertex {
                    position: ring.vertices[to].position,
                    bulge: end.1,
                });
                vertices.push(PolylineVertex::straight(end.0));
                Some(Polyline { vertices, closed: true })
            }
            _ => None,
        }
    };
    // Vertices a fit off the axis are on it; the near side is past it.
    let snapped = |mut ring: Polyline| {
        for vertex in &mut ring.vertices {
            if vertex.position[0].abs() <= fit {
                vertex.position[0] = 0.0;
            }
        }
        ring
    };
    // A removed face's stretch, wherever `lift` of it now lies, run out to
    // `out` past the face: the cavity opens there.
    let opened = |mut ring: Polyline, lift: f64, out: f64| -> Option<Polyline> {
        for face in removed {
            let piece = pieces.iter().find(|piece| piece.face == *face)?;
            let node = body.faces.get(*face)?;
            let Some(Surface::Plane(plane)) = body.surfaces.get(node.surface) else {
                return None;
            };
            let outward = Vec3::from(plane.normal()?).dot(along)
                * if node.forward { 1.0 } else { -1.0 };
            let level = piece.ends[0][1];
            let (at, past) = (level - outward.signum() * lift, level + outward.signum() * out);
            let count = ring.vertices.len();
            let index = (0..count).find(|index| {
                let (from, to) = (ring.vertices[*index], ring.vertices[(index + 1) % count]);
                from.bulge == 0.0
                    && (from.position[1] - at).abs() <= fit * 10.0
                    && (to.position[1] - at).abs() <= fit * 10.0
            })?;
            let next = ring.vertices[(index + 1) % count].position;
            let here = ring.vertices[index].position;
            ring.vertices.splice(
                index + 1..index + 1,
                [
                    PolylineVertex::straight([here[0], past]),
                    PolylineVertex::straight([next[0], past]),
                ],
            );
        }
        Some(ring)
    };
    let other = if along.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let perpendicular = along.cross(other).normalize()?;
    let meridian = ocs_geom::space::Plane::orthonormal(
        origin.to_array(),
        perpendicular.to_array(),
        perpendicular.cross(along).to_array(),
    )?;
    let turned = |rings: &[Polyline]| -> Option<Body> {
        let mut result: Option<Body> = None;
        for ring in rings {
            let mut profile = Vec::new();
            let count = ring.vertices.len();
            for index in 0..count {
                let from = ring.vertices[index].position;
                let to = ring.vertices[(index + 1) % count].position;
                profile.push(match ring.segment_arc(index) {
                    None => Curve::Line(ocs_geom::geom2d::Line { start: from, end: to }),
                    Some(arc) => {
                        let start = if arc.sweep > 0.0 { arc.start_angle } else { arc.end_angle };
                        Curve::Arc(ocs_geom::geom2d::Arc {
                            centre: arc.center,
                            radius: arc.radius,
                            start_angle: start,
                            end_angle: start + arc.sweep.abs(),
                        })
                    }
                });
            }
            let turn = super::revolve_region(
                meridian,
                &[profile],
                origin.to_array(),
                along.to_array(),
                std::f64::consts::TAU,
            )?;
            result = Some(match result {
                None => turn,
                Some(sofar) => super::combine(sofar, turn, Operation::Union, fit).ok()?,
            });
        }
        result
    };
    let halves = |rings: Vec<Polyline>| -> Option<Vec<Polyline>> {
        rings
            .into_iter()
            .map(|ring| if touches_axis { near_half(&snapped(ring)) } else { Some(ring) })
            .collect()
    };
    let moved = ocs_geom::geom2d::offset_polyline(&ring, thickness, side(distance > 0.0));
    if moved.is_empty() {
        return Some(Err(ShellError::NoMaterial));
    }
    let built = if distance > 0.0 {
        let cavity = halves(moved)
            .and_then(|rings| {
                rings
                    .into_iter()
                    .map(|ring| opened(ring, thickness, thickness))
                    .collect::<Option<Vec<_>>>()
            })
            .and_then(|rings| turned(&rings));
        cavity.map(|cavity| (body.clone(), cavity))
    } else {
        let exterior = halves(moved).and_then(|rings| turned(&rings));
        let cavity = if removed.is_empty() {
            Some(body.clone())
        } else {
            let own = if touches_axis {
                let mut vertices = half.clone();
                vertices.push(PolylineVertex::straight(at));
                Polyline { vertices, closed: true }
            } else {
                ring.clone()
            };
            opened(own, 0.0, thickness * 2.0).and_then(|own| turned(&[own]))
        };
        exterior.zip(cavity)
    };
    let Some((exterior, cavity)) = built else {
        return Some(Err(ShellError::UnsupportedSolid));
    };
    Some(hollowed(exterior, cavity, body, removed.is_empty()))
}

#[cfg(not(feature = "offset"))]
fn shell_revolved(_: &Body, _: &HashSet<FaceKey>, _: f64) -> Option<Result<Body, ShellError>> {
    None
}

/// A profile ring as one closed polyline, arcs as bulges. `None` for a
/// piece neither straight nor round, or a ring that does not close.
#[cfg(feature = "offset")]
fn ring_polyline(ring: &[ocs_geom::geom2d::Curve]) -> Option<ocs_geom::geom2d::Polyline> {
    use ocs_geom::geom2d::{Curve, PolylineVertex};
    let pieces = super::extrusion_profile_pieces(ring);
    let ends = |piece: &Curve| (piece.point_at(0.0), piece.point_at(1.0));
    let size = pieces
        .iter()
        .flat_map(|piece| [ends(piece).0, ends(piece).1])
        .fold(0.0f64, |big, p| big.max(p[0].abs()).max(p[1].abs()));
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-9 * (1.0 + size);
    let first = pieces.first()?;
    let next = pieces.get(1).unwrap_or(first);
    let (a, b) = ends(first);
    let mut at = if near(b, ends(next).0) || near(b, ends(next).1) { a } else { b };
    let start = at;
    let mut vertices = Vec::with_capacity(pieces.len());
    for piece in &pieces {
        let (from, to) = ends(piece);
        let forward = if near(from, at) {
            true
        } else if near(to, at) {
            false
        } else {
            return None;
        };
        let bulge = match piece {
            Curve::Line(_) => 0.0,
            Curve::Arc(arc) => {
                let turn = (arc.sweep() / 4.0).tan();
                if forward { turn } else { -turn }
            }
            _ => return None,
        };
        vertices.push(PolylineVertex { position: at, bulge });
        at = if forward { to } else { from };
    }
    near(at, start).then_some(ocs_geom::geom2d::Polyline { vertices, closed: true })
}

/// The exterior less the cavity, a closed one checked to take exactly its
/// own volume out.
fn hollowed(
    exterior: Body,
    cavity: Body,
    reference: &Body,
    closed: bool,
) -> Result<Body, ShellError> {
    let hollow = subtract(exterior.clone(), cavity.clone(), reference)?;
    if closed {
        let volume = |body: &Body| super::mass_properties(body).map(|mass| mass.volume);
        let (whole, inner, left) = (
            volume(&exterior).ok_or(ShellError::NoMaterial)?,
            volume(&cavity).ok_or(ShellError::NoMaterial)?,
            volume(&hollow).ok_or(ShellError::NoMaterial)?,
        );
        if (whole - inner - left).abs() > 1e-6 * whole.abs() {
            return Err(ShellError::NoMaterial);
        }
    }
    Ok(hollow)
}

/// Any solid bounded by planes alone: the cavity is the solid with every
/// face moved in by the thickness — each corner where its faces' moved
/// planes meet — and every removed face moved out instead, so the cavity
/// runs out through it. Grown outward, the exterior is the solid moved out
/// and the cavity the solid itself, run out through the removed faces.
/// Refused where a moved face would turn over, cross itself or lose a
/// corner — the thickness eating a narrower part of the solid.
fn shell_polyhedral(
    body: &Body,
    removed: &HashSet<FaceKey>,
    distance: f64,
) -> Result<Body, ShellError> {
    let planar = body.faces.iter().all(|(_, face)| {
        matches!(body.surfaces.get(face.surface), Some(Surface::Plane(_)))
    });
    if !planar {
        return Err(ShellError::UnsupportedSolid);
    }
    if removed.len() == body.faces.len() {
        return Err(ShellError::NoMaterial);
    }
    let thickness = distance.abs();
    let through = |face: FaceKey| if removed.contains(&face) { thickness * 2.0 } else { 0.0 };
    let (outer, cavity) = if distance > 0.0 {
        let cavity = offset_planes(body, &|face| {
            if removed.contains(&face) { through(face) } else { -thickness }
        })
        .ok_or(ShellError::NoMaterial)?;
        // Every corner of the cavity not run out through an open face lies
        // inside the solid; one outside is a wall thinner than the shell.
        let tolerance = super::operation_tolerance(&[body]);
        let faces_at = corner_faces(&cavity);
        let inside = cavity.vertices.iter().all(|(key, vertex)| {
            let open = faces_at
                .get(&key)
                .is_some_and(|faces| faces.iter().any(|face| removed.contains(face)));
            open || super::contains_point(body, vertex.point, tolerance)
                == super::Containment::Inside
        });
        if !inside {
            return Err(ShellError::NoMaterial);
        }
        (body.clone(), cavity)
    } else {
        let outer = offset_planes(body, &|_| thickness).ok_or(ShellError::NoMaterial)?;
        let cavity = offset_planes(body, &through).ok_or(ShellError::NoMaterial)?;
        (outer, cavity)
    };
    hollowed(outer, cavity, body, removed.is_empty())
}

/// `body`, every face moved along its outward normal by `shift` of it and
/// every corner put where its faces' moved planes meet, keeping the
/// topology. `None` where three planes do not settle a corner, or a face
/// turns over or crosses itself on the way.
fn offset_planes(body: &Body, shift: &dyn Fn(FaceKey) -> f64) -> Option<Body> {
    let tolerance = super::operation_tolerance(&[body]);
    let mut result = body.clone();
    let mut planes: HashMap<FaceKey, (Vec3, f64)> = HashMap::new();
    for (key, face) in body.faces.iter() {
        let Some(Surface::Plane(plane)) = body.surfaces.get(face.surface) else {
            return None;
        };
        let normal = Vec3::from(plane.normal()?) * if face.forward { 1.0 } else { -1.0 };
        let moved = Vec3::from(plane.origin) + normal * shift(key);
        planes.insert(key, (normal, normal.dot(moved)));
        let mut surface = *plane;
        surface.origin = moved.to_array();
        let slot = result.surfaces.insert(Surface::Plane(surface));
        result.faces.get_mut(key)?.surface = slot;
    }
    // Each corner: the point nearest every moved plane round it, which must
    // lie on all of them.
    let faces_at = corner_faces(body);
    for (key, _) in body.vertices.iter() {
        let around: Vec<(Vec3, f64)> = faces_at
            .get(&key)
            .into_iter()
            .flatten()
            .filter_map(|face| planes.get(face).copied())
            .collect();
        if around.len() < 3 {
            return None;
        }
        let mut normal = [[0.0; 3]; 3];
        let mut right = [0.0; 3];
        for (n, c) in &around {
            let n = n.to_array();
            for row in 0..3 {
                for column in 0..3 {
                    normal[row][column] += n[row] * n[column];
                }
                right[row] += n[row] * c;
            }
        }
        let point = Vec3::from(solve3(normal, right)?);
        let scale = 1.0 + point.length();
        if around.iter().any(|(n, c)| (n.dot(point) - c).abs() > tolerance.max(1e-12 * scale)) {
            return None;
        }
        result.vertices.get_mut(key)?.point = point.to_array();
    }
    // Straight edges between the new corners.
    for (key, edge) in body.edges.iter() {
        let from = Vec3::from(result.vertices.get(edge.start)?.point);
        let to = Vec3::from(result.vertices.get(edge.end)?.point);
        let along = to - from;
        if along.length() <= tolerance {
            return None;
        }
        let curve = result.curves.insert(super::Curve3::Line(super::geometry::Line3 {
            origin: from.to_array(),
            direction: along.to_array(),
        }));
        let node = result.edges.get_mut(key)?;
        node.curve = curve;
        node.start_parameter = 0.0;
        node.end_parameter = 1.0;
        for coedge in node.coedges.clone() {
            result.coedges.get_mut(coedge)?.pcurve = None;
        }
    }
    // Each face the same way round as before, and simple.
    for (key, _) in body.faces.iter() {
        let (before, _) = super::split::planar_measure(body, key)?;
        let (after, _) = super::split::planar_measure(&result, key)?;
        if before * after <= 0.0 || after.abs() <= tolerance * tolerance {
            return None;
        }
        if folds(&result, key, tolerance)? {
            return None;
        }
    }
    result.validate().is_empty().then_some(result)
}

/// The faces meeting at each corner.
fn corner_faces(body: &Body) -> HashMap<super::VertexKey, HashSet<FaceKey>> {
    let mut faces: HashMap<super::VertexKey, HashSet<FaceKey>> = HashMap::new();
    for face in body.face_keys() {
        for coedge in body.face_coedges(face) {
            if let Some((from, to)) = body.coedge_vertices(coedge) {
                faces.entry(from).or_default().insert(face);
                faces.entry(to).or_default().insert(face);
            }
        }
    }
    faces
}

/// Whether two straight edges of a face cross anywhere but at a corner they
/// share.
fn folds(body: &Body, face: FaceKey, tolerance: f64) -> Option<bool> {
    let node = body.faces.get(face)?;
    let Some(Surface::Plane(plane)) = body.surfaces.get(node.surface) else {
        return None;
    };
    let mut segments: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for coedge in body.face_coedges(face) {
        let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
        let from = plane.project(body.vertices.get(edge.start)?.point)?;
        let to = plane.project(body.vertices.get(edge.end)?.point)?;
        segments.push((from, to));
    }
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= tolerance;
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    for (index, (a, b)) in segments.iter().enumerate() {
        for (c, d) in &segments[index + 1..] {
            if near(*a, *c) || near(*a, *d) || near(*b, *c) || near(*b, *d) {
                continue;
            }
            let (d1, d2) = (cross(*a, *b, *c), cross(*a, *b, *d));
            let (d3, d4) = (cross(*c, *d, *a), cross(*c, *d, *b));
            if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                return Some(true);
            }
        }
    }
    Some(false)
}

/// The solution of a 3×3 system by Cramer's rule, `None` when singular.
fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let whole = det(m);
    let scale = m.iter().flatten().fold(0.0f64, |big, value| big.max(value.abs()));
    if whole.abs() <= 1e-12 * scale.powi(3) {
        return None;
    }
    Some(std::array::from_fn(|column| {
        let mut swapped = m;
        for row in 0..3 {
            swapped[row][column] = r[row];
        }
        det(swapped) / whole
    }))
}

fn subtract(outer: Body, cavity: Body, reference: &Body) -> Result<Body, ShellError> {
    let tolerance = super::operation_tolerance(&[reference, &outer, &cavity]);
    super::combine(outer, cavity, Operation::Difference, tolerance).map_err(ShellError::Kernel)
}

fn recognize_box(body: &Body) -> Option<BoxShape> {
    if body.faces.len() != 6 || body.edges.len() != 12 || body.vertices.len() != 8 {
        return None;
    }
    if body.face_keys().any(|key| {
        body.faces
            .get(key)
            .and_then(|face| body.surfaces.get(face.surface))
            .is_none_or(|surface| !matches!(surface, Surface::Plane(_)))
    }) {
        return None;
    }
    let (corner_key, corner) = body.vertices.iter().next()?;
    let origin = corner.point;
    let mut spans = Vec::new();
    for (_, edge) in body.edges.iter() {
        let other = if edge.start == corner_key {
            Some(edge.end)
        } else if edge.end == corner_key {
            Some(edge.start)
        } else {
            None
        };
        let Some(other) = other else { continue };
        let delta = Vec3::from(body.vertices.get(other)?.point) - Vec3::from(origin);
        spans.push((delta.normalize()?.to_array(), delta.length()));
    }
    if spans.len() != 3 {
        return None;
    }
    let axes = [spans[0].0, spans[1].0, spans[2].0];
    let size = [spans[0].1, spans[1].1, spans[2].1];
    let scale = size.iter().copied().fold(1.0, f64::max);
    let tolerance = scale * 1e-8;
    for one in 0..3 {
        for other in one + 1..3 {
            if Vec3::from(axes[one]).dot(Vec3::from(axes[other])).abs() > 1e-8 {
                return None;
            }
        }
    }
    for (_, vertex) in body.vertices.iter() {
        let offset = Vec3::from(vertex.point) - Vec3::from(origin);
        for axis in 0..3 {
            let coordinate = offset.dot(Vec3::from(axes[axis]));
            if coordinate.abs() > tolerance && (coordinate - size[axis]).abs() > tolerance {
                return None;
            }
        }
    }
    let mut faces = HashMap::new();
    for key in body.face_keys() {
        let face = body.faces.get(key)?;
        let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
            return None;
        };
        let mut normal = Vec3::from(plane.normal()?);
        if !face.forward {
            normal = -normal;
        }
        let (axis, projection) = (0..3)
            .map(|axis| (axis, normal.dot(Vec3::from(axes[axis]))))
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
        if projection.abs() < 1.0 - 1e-8 {
            return None;
        }
        faces.insert(key, BoxFace { axis, maximum: projection > 0.0 });
    }
    (faces.len() == 6).then_some(BoxShape { origin, axes, size, faces })
}

fn recognize_cylinder(body: &Body) -> Option<CylinderShape> {
    if body.faces.len() != 3 {
        return None;
    }
    let mut cylinder = None;
    let mut caps = Vec::new();
    for key in body.face_keys() {
        let face = body.faces.get(key)?;
        match body.surfaces.get(face.surface)? {
            Surface::Cylinder(surface) if cylinder.is_none() => cylinder = Some((key, *surface)),
            Surface::Plane(plane) => caps.push((key, *plane)),
            _ => return None,
        }
    }
    let (side, cylinder) = cylinder?;
    if caps.len() != 2 || !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
        return None;
    }
    let axis = Vec3::from(cylinder.base.normal()?).normalize()?;
    let x = Vec3::from(cylinder.base.x_axis).normalize()?;
    let x = (x - axis * x.dot(axis)).normalize()?;
    let y = axis.cross(x).normalize()?;
    let mut cap_positions = caps
        .iter()
        .map(|(key, plane)| {
            let normal = Vec3::from(plane.normal()?);
            if normal.dot(axis).abs() < 1.0 - 1e-8 {
                return None;
            }
            Some((*key, (Vec3::from(plane.origin) - Vec3::from(cylinder.base.origin)).dot(axis)))
        })
        .collect::<Option<Vec<_>>>()?;
    cap_positions.sort_by(|a, b| a.1.total_cmp(&b.1));
    let height = cap_positions[1].1 - cap_positions[0].1;
    if !height.is_finite() || height <= 0.0 {
        return None;
    }
    let origin = (Vec3::from(cylinder.base.origin) + axis * cap_positions[0].1).to_array();
    let faces = HashMap::from([
        (side, CylinderFace::Side),
        (cap_positions[0].0, CylinderFace::Bottom),
        (cap_positions[1].0, CylinderFace::Top),
    ]);
    Some(CylinderShape {
        origin,
        axes: [x.to_array(), y.to_array(), axis.to_array()],
        radius: cylinder.radius,
        height,
        faces,
    })
}

fn recognize_sphere(body: &Body) -> Option<SphereShape> {
    if body.faces.len() != 1 {
        return None;
    }
    let face = body.face_keys().next()?;
    let node = body.faces.get(face)?;
    let Surface::Sphere(sphere) = body.surfaces.get(node.surface)? else {
        return None;
    };
    (sphere.radius.is_finite() && sphere.radius > 0.0).then_some(SphereShape {
        centre: sphere.frame.origin,
        radius: sphere.radius,
        face,
    })
}

/// Any solid of planes, cylinders, cones, spheres and tori hollowed by
/// taking its own offset away from it: the same faces, edges and corners,
/// each surface moved `distance` in — a removed face's out, so the cavity
/// runs through it. A corner goes where its moved surfaces meet again, an
/// edge along the moved pair's own meeting. `None` where an offset changes
/// the solid's shape — a face shrinking to nothing, a pair no longer meeting
/// — or a surface has no exact offset here.
fn shell_by_offset(body: &Body, removed: &HashSet<FaceKey>, distance: f64) -> Option<Body> {
    if distance <= 0.0 {
        return None;
    }
    let tolerance = super::operation_tolerance(&[body]);
    // A wall as thick as the body leaves no cavity to cut.
    let open: Vec<FaceKey> = removed.iter().copied().collect();
    if !matches!(super::wall_room::shell_wall_room(body, &open, distance), super::wall_room::WallRoom::Room(_)) {
        return None;
    }
    let inner = offset_inward(body, removed, distance, tolerance)?;
    let volume = |body: &Body| super::mass_properties(body).map(|mass| mass.volume);
    let (outer, cavity) = (volume(body)?, volume(&inner)?);
    if !(cavity > 0.0) || removed.is_empty() && cavity >= outer {
        return None;
    }
    let hollow = super::combine(body.clone(), inner, Operation::Difference, tolerance).ok()?;
    let left = volume(&hollow)?;
    (hollow.validate().is_empty() && left > 0.0 && left < outer).then_some(hollow)
}

/// `body` with every face moved `distance` along its inward normal, those
/// in `opened` outward, and its corners and edges found again where the
/// moved surfaces meet.
fn offset_inward(
    body: &Body,
    opened: &HashSet<FaceKey>,
    distance: f64,
    tolerance: f64,
) -> Option<Body> {
    use super::geometry::{Circle3, Curve3, Ellipse3, Line3};
    let mut inner = body.clone();
    // Each face's surface moved; a surface two faces share moves once, the
    // same way for both or not at all.
    let mut moved: HashMap<super::SurfaceKey, Surface> = HashMap::new();
    for (key, face) in body.faces.iter() {
        let along = if opened.contains(&key) { distance } else { -distance };
        let sense = if face.forward { 1.0 } else { -1.0 };
        let surface = moved_surface(body.surfaces.get(face.surface)?, along * sense)?;
        if moved.get(&face.surface).is_some_and(|known| *known != surface) {
            return None;
        }
        moved.insert(face.surface, surface);
    }
    for (key, surface) in &moved {
        *inner.surfaces.get_mut(*key)? = surface.clone();
    }
    let faces_at = |vertex: super::VertexKey| -> Vec<super::SurfaceKey> {
        let mut out: Vec<super::SurfaceKey> = Vec::new();
        for (_, edge) in body.edges.iter() {
            if edge.start != vertex && edge.end != vertex {
                continue;
            }
            for coedge in &edge.coedges {
                let face = body
                    .coedges
                    .get(*coedge)
                    .and_then(|coedge| body.loops.get(coedge.owner))
                    .and_then(|ring| body.faces.get(ring.owner));
                if let Some(face) = face {
                    if !out.contains(&face.surface) {
                        out.push(face.surface);
                    }
                }
            }
        }
        out
    };
    // Each corner settled onto its moved surfaces by Gauss–Newton, the step
    // the shortest that meets them all: a corner on two surfaces moves
    // square to both, not along their meeting.
    for (key, vertex) in body.vertices.iter() {
        let surfaces: Vec<&Surface> =
            faces_at(key).iter().filter_map(|surface| moved.get(surface)).collect();
        if surfaces.is_empty() {
            return None;
        }
        let mut point = Vec3::from(vertex.point);
        for _ in 0..32 {
            let mut rows: Vec<(Vec3, f64)> = Vec::with_capacity(surfaces.len());
            for surface in &surfaces {
                let gap = surface.distance_to(point.to_array());
                let (u, v) = surface.parameters_at(point.to_array())?;
                let normal = Vec3::from(surface.normal_at(u, v)?).normalize()?;
                // Two patches of one surface ask the same thing once.
                let same = rows.iter().any(|(seen, was)| {
                    seen.dot(normal).abs() > 1.0 - 1e-12 && (was.abs() - gap.abs()).abs() <= 1e-12
                });
                if !same {
                    rows.push((normal, gap));
                }
            }
            if rows.iter().all(|(_, gap)| gap.abs() <= tolerance * 1e-3) {
                break;
            }
            point = point - least_norm_step(&rows)?;
        }
        if surfaces.iter().any(|surface| surface.distance_to(point.to_array()).abs() > tolerance) {
            return None;
        }
        inner.vertices.get_mut(key)?.point = point.to_array();
    }
    // Each edge along where its two moved surfaces meet, nearest where it
    // was; a seam, one surface on both sides, along its own ruling.
    for (key, edge) in body.edges.iter() {
        let start = Vec3::from(inner.vertices.get(edge.start)?.point);
        let end = Vec3::from(inner.vertices.get(edge.end)?.point);
        let curve = body.curves.get(edge.curve)?;
        let sides: Vec<super::SurfaceKey> = edge
            .coedges
            .iter()
            .filter_map(|coedge| {
                let ring = body.loops.get(body.coedges.get(*coedge)?.owner)?;
                Some(body.faces.get(ring.owner)?.surface)
            })
            .collect();
        let guess = start.lerp(end, 0.5);
        let new_curve = match curve {
            Curve3::Line(_) => {
                let along = end - start;
                if along.length() <= tolerance {
                    return None;
                }
                Curve3::Line(Line3 { origin: start.to_array(), direction: along.to_array() })
            }
            Curve3::Circle(_) | Curve3::Ellipse(_) => {
                let [one, other] = sides[..] else { return None };
                let (first, second) = (moved.get(&one)?, moved.get(&other)?);
                let found = match super::intersect::surfaces(first, second, tolerance) {
                    super::intersect::Meeting::Curves(found) => found,
                    // Two patches of one sphere: the line between them is
                    // only where the sphere was divided, its corners going
                    // wherever the real edges took them. A great circle runs
                    // on through its two corners round the moved sphere; a
                    // lesser one shrinks about the centre.
                    super::intersect::Meeting::Coincident => {
                        let (Surface::Sphere(was), Surface::Sphere(now), Curve3::Circle(circle)) =
                            (body.surfaces.get(one)?, first, curve)
                        else {
                            return None;
                        };
                        let (centre, moved_centre) =
                            (Vec3::from(was.frame.origin), Vec3::from(now.frame.origin));
                        let mut circle = *circle;
                        let great = Vec3::from(circle.plane.origin).distance(centre)
                            <= was.radius * 1e-9;
                        let normal = Vec3::from(circle.plane.normal()?);
                        let through = (start - moved_centre).cross(end - moved_centre);
                        match through.normalize() {
                            Some(across) if great && edge.start != edge.end => {
                                let across =
                                    if across.dot(normal) < 0.0 { -across } else { across };
                                let x = (start - moved_centre).normalize()?;
                                circle.plane = ocs_geom::space::Plane {
                                    origin: moved_centre.to_array(),
                                    x_axis: x.to_array(),
                                    y_axis: across.cross(x).to_array(),
                                };
                            }
                            _ => {
                                let scale = now.radius / was.radius;
                                circle.plane.origin = (moved_centre
                                    + (Vec3::from(circle.plane.origin) - centre) * scale)
                                    .to_array();
                            }
                        }
                        circle.radius = if great {
                            now.radius
                        } else {
                            circle.radius * now.radius / was.radius
                        };
                        vec![Curve3::Circle(circle)]
                    }
                    _ => return None,
                };
                // The meeting curve passing nearest the edge's own corner,
                // or, round a closed edge, its middle moved alike.
                let reference = if edge.start == edge.end { start } else { guess };
                let nearest = found.into_iter().min_by(|a, b| {
                    let gap = |c: &Curve3| {
                        Vec3::from(c.point_at(c.parameter_at(reference.to_array())))
                            .distance(reference)
                    };
                    gap(a).total_cmp(&gap(b))
                })?;
                // Turning the way the edge's own curve turned.
                let normal_of = |c: &Curve3| match c {
                    Curve3::Circle(Circle3 { plane, .. })
                    | Curve3::Ellipse(Ellipse3 { plane, .. }) => plane.normal().map(Vec3::from),
                    _ => None,
                };
                let flip = normal_of(curve)?.dot(normal_of(&nearest)?) < 0.0;
                match nearest {
                    Curve3::Circle(mut circle) => {
                        if flip {
                            circle.plane.y_axis = (-Vec3::from(circle.plane.y_axis)).to_array();
                        }
                        Curve3::Circle(circle)
                    }
                    Curve3::Ellipse(mut ellipse) => {
                        if flip {
                            ellipse.plane.y_axis = (-Vec3::from(ellipse.plane.y_axis)).to_array();
                        }
                        Curve3::Ellipse(ellipse)
                    }
                    _ => return None,
                }
            }
            _ => return None,
        };
        let (from, to) = match &new_curve {
            Curve3::Line(_) => (0.0, 1.0),
            _ => {
                let from = new_curve.parameter_at(start.to_array());
                let span = if edge.start == edge.end {
                    std::f64::consts::TAU
                } else {
                    let to = new_curve.parameter_at(end.to_array());
                    (to - from).rem_euclid(std::f64::consts::TAU)
                };
                (from, from + span)
            }
        };
        let off = |point: &Vec3| {
            let on = new_curve.point_at(new_curve.parameter_at(point.to_array()));
            Vec3::from(on).distance(*point)
        };
        if [start, end].iter().any(|point| off(point) > tolerance * 10.0) {
            return None;
        }
        let curve_key = inner.curves.insert(new_curve);
        let node = inner.edges.get_mut(key)?;
        node.curve = curve_key;
        node.start_parameter = from;
        node.end_parameter = to;
    }
    for key in inner.coedges.keys().collect::<Vec<_>>() {
        inner.coedges.get_mut(key)?.pcurve = None;
    }
    inner.validate().is_empty().then_some(inner)
}

/// `surface` moved `along` its own normal: a plane shifted, a cylinder,
/// sphere or torus tube grown, a cone widened square to its side. `None`
/// for a spline, and for one moved past its own axis or centre.
pub(super) fn moved_surface(surface: &Surface, along: f64) -> Option<Surface> {
    use super::geometry::{Cone, Cylinder, Sphere, Torus};
    Some(match surface {
        Surface::Plane(plane) => {
            let normal = Vec3::from(plane.normal()?).normalize()?;
            let mut plane = *plane;
            plane.origin = (Vec3::from(plane.origin) + normal * along).to_array();
            Surface::Plane(plane)
        }
        Surface::Cylinder(cylinder) => {
            let radius = cylinder.radius + along;
            (radius > 0.0).then_some(Surface::Cylinder(Cylinder { radius, ..*cylinder }))?
        }
        Surface::Sphere(sphere) => {
            let radius = sphere.radius + along;
            (radius > 0.0).then_some(Surface::Sphere(Sphere { radius, ..*sphere }))?
        }
        Surface::Cone(cone) => Surface::Cone(Cone {
            radius: cone.radius + along / cone.half_angle.cos(),
            ..*cone
        }),
        Surface::Torus(torus) => {
            let minor_radius = torus.minor_radius + along;
            (minor_radius > 0.0).then_some(Surface::Torus(Torus { minor_radius, ..*torus }))?
        }
        Surface::Nurbs(_) | Surface::Offset(_) => return None,
    })
}

/// The shortest step taking a point onto every surface whose unit normal
/// and signed gap are given: the gaps' combination of the normals that
/// closes each, two or three of them; more read in the least-squares sense.
pub(super) fn least_norm_step(rows: &[(Vec3, f64)]) -> Option<Vec3> {
    // Normal equations of J Jᵀ y = g, then the step Jᵀ y.
    let n = rows.len();
    let mut gram = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            gram[i][j] = rows[i].0.dot(rows[j].0);
        }
    }
    let mut rhs: Vec<f64> = rows.iter().map(|(_, gap)| *gap).collect();
    // Gauss–Jordan with partial pivoting; a normal the others already span
    // gets no share of the step.
    let mut solved = vec![false; n];
    for column in 0..n {
        let pivot = (column..n)
            .max_by(|a, b| gram[*a][column].abs().total_cmp(&gram[*b][column].abs()))?;
        if gram[pivot][column].abs() <= 1e-9 {
            continue;
        }
        gram.swap(column, pivot);
        rhs.swap(column, pivot);
        for row in 0..n {
            if row != column {
                let factor = gram[row][column] / gram[column][column];
                for k in 0..n {
                    gram[row][k] -= factor * gram[column][k];
                }
                rhs[row] -= factor * rhs[column];
            }
        }
        solved[column] = true;
    }
    let mut step = Vec3::new(0.0, 0.0, 0.0);
    for column in 0..n {
        if solved[column] {
            step = step + rows[column].0 * (rhs[column] / gram[column][column]);
        }
    }
    Some(step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analytic_mass_properties, make};
    use std::f64::consts::PI;

    #[test]
    fn a_plate_with_two_holes_hollows_round_them() {
        // A 10 × 6 plate a unit thick with two unit holes. Moved in a
        // tenth, its region is 9.8 × 5.8 less two holes of 1.1; grown, its
        // walls moved out to meet sharp, 10.2 × 6.2 less holes of 0.9.
        let mut plate = make::cuboid([0.0, 0.0, 0.0], [10.0, 6.0, 1.0]).unwrap();
        for x in [3.0, 7.0] {
            let hole = make::cylinder([x, 3.0, -1.0], 1.0, 3.0).unwrap();
            plate = crate::combine(plate, hole, Operation::Difference, 1e-9).unwrap();
        }
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        let solid = 60.0 - 2.0 * PI;
        let shrunk = 9.8 * 5.8 - 2.0 * PI * 1.21;
        let grown = 10.2 * 6.2 - 2.0 * PI * 0.81;
        let top = plate
            .face_keys()
            .find(|face| {
                let node = plate.faces.get(*face).unwrap();
                matches!(plate.surfaces.get(node.surface), Some(Surface::Plane(plane))
                    if (plane.origin[2] - 1.0).abs() < 1e-9)
            })
            .unwrap();
        for (removed, distance, expected) in [
            (vec![], 0.1, solid - shrunk * 0.8),
            (vec![top], 0.1, solid - shrunk * 0.9),
            (vec![], -0.1, grown * 1.2 - solid),
        ] {
            let hollow = shell(&plate, &removed, distance).unwrap();
            assert!(hollow.validate().is_empty());
            let got = volume(&hollow);
            assert!((got - expected).abs() < 1e-4 * expected, "{distance}: {got} {expected}");
        }
    }

    #[test]
    fn an_l_shaped_block_hollows_closed_open_and_outward() {
        // The L's footprint is 12, its cavity's moved in a tenth 10.44 and
        // its grown exterior's 13.64.
        let long = make::cuboid([0.0; 3], [4.0, 2.0, 1.0]).unwrap();
        let wing = make::cuboid([0.0; 3], [2.0, 4.0, 1.0]).unwrap();
        let block = crate::combine(long, wing, Operation::Union, 1e-9).unwrap();
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        let top = block
            .face_keys()
            .find(|face| {
                let node = block.faces.get(*face).unwrap();
                matches!(block.surfaces.get(node.surface), Some(Surface::Plane(plane))
                    if (plane.origin[2] - 1.0).abs() < 1e-9)
            })
            .unwrap();
        for (removed, distance, expected) in [
            (vec![], 0.1, 12.0 - 10.44 * 0.8),
            (vec![top], 0.1, 12.0 - 10.44 * 0.9),
            (vec![], -0.1, 13.64 * 1.2 - 12.0),
        ] {
            let hollow = shell(&block, &removed, distance).unwrap();
            assert!(hollow.validate().is_empty());
            assert!((volume(&hollow) - expected).abs() < 1e-6 * expected, "{distance}");
        }
    }

    #[test]
    fn a_capsule_hollows_through_its_round_end_and_its_flat_one() {
        // A cylinder of radius 5 and height 10 under a half ball. A half
        // unit in, the cavity is the same shape of radius 4.5 from 0.5 up;
        // opened through the floor it runs from the floor; grown, radius
        // 5.5 from -0.5. Each volume a multiple of π.
        use ocs_geom::geom2d::{Arc, Curve, Line};
        let profile = vec![
            Curve::Line(Line { start: [0.0, 0.0], end: [5.0, 0.0] }),
            Curve::Line(Line { start: [5.0, 0.0], end: [5.0, 10.0] }),
            Curve::Arc(Arc {
                centre: [0.0, 10.0],
                radius: 5.0,
                start_angle: 0.0,
                end_angle: PI / 2.0,
            }),
            Curve::Line(Line { start: [0.0, 15.0], end: [0.0, 0.0] }),
        ];
        let plane = ocs_geom::space::Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0])
            .unwrap();
        let capsule =
            crate::revolve_region(plane, &[profile], [0.0; 3], [0.0, 0.0, 1.0], 2.0 * PI)
                .unwrap();
        let volume = |body: &Body| crate::mass_properties(body).unwrap().volume;
        let floor = capsule
            .face_keys()
            .find(|face| {
                let node = capsule.faces.get(*face).unwrap();
                matches!(capsule.surfaces.get(node.surface), Some(Surface::Plane(_)))
                    && crate::face_bounds(&capsule, *face).unwrap().max[2].abs() < 1e-9
            })
            .unwrap();
        let solid = 250.0 + 250.0 / 3.0;
        for (removed, distance, expected) in [
            (vec![], 0.5, solid - 20.25 * 9.5 - 60.75),
            (vec![floor], 0.5, solid - 20.25 * 10.0 - 60.75),
            (vec![], -0.5, 30.25 * 10.5 + 2.0 * 166.375 / 3.0 - solid),
        ] {
            let hollow = shell(&capsule, &removed, distance).unwrap();
            assert!(hollow.validate().is_empty());
            let got = volume(&hollow) / PI;
            assert!((got - expected).abs() < 1e-3 * expected, "{distance}: {got} {expected}");
        }
    }

    #[test]
    fn a_sphere_shell_has_the_exact_annular_volume() {
        let solid = make::sphere([3.0, -2.0, 5.0], 4.0).unwrap();
        let result = shell(&solid, &[], 1.0).unwrap();

        assert!(result.validate().is_empty());
        assert_eq!(result.shells.len(), 2);
        let properties = analytic_mass_properties(&result).unwrap();
        let expected = 4.0 * PI * (4.0_f64.powi(3) - 3.0_f64.powi(3)) / 3.0;
        assert!((properties.volume - expected).abs() < 1e-8);
        assert_eq!(properties.centroid, [3.0, -2.0, 5.0]);
    }

    #[test]
    fn removing_a_box_face_opens_a_valid_shell() {
        let solid = make::cuboid([0.0; 3], [6.0, 5.0, 4.0]).unwrap();
        let removed = shell_face_at_point(&solid, [3.0, 2.5, 4.0]).unwrap();
        let result = shell(&solid, &[removed], 0.5).unwrap();

        assert!(result.validate().is_empty());
        assert_eq!(result.roots.len(), 1);
        assert_eq!(result.lumps.len(), 1);
    }
}
