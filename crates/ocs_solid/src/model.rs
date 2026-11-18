//! What the app asks of Solidcore about the records it holds: each record opened as Solidcore's
//! working body ([`open`]), an operation on it, and the result recorded again ([`record`]).
//! Faces, edges and vertices are named by their record indices throughout.

pub use solidcore_ops::{BooleanOp, Body, Error, Order};
pub use solidcore_record::brep::{self as r, Brep};

use crate::geometry::{cross, dot, points_on, scale, unit};
use ocs_geom::geom2d::{Arc, Circle, Curve, Ellipse, EllipseArc, Line, NurbsCurve};
use ocs_geom::space::{Plane, Vec3};

/// `brep` as Solidcore's working body, with the order naming its faces, edges and vertices.
pub fn open(brep: &Brep) -> Result<(Body, Order), Error> {
    Body::from_record(brep, true)
}

/// `body` recorded.
/// Faces whose spline is a cylinder, cone or sphere, and edges whose spline is a circle or an
/// ellipse (as Solidcore gives the quadrics and conics its operations make), are labelled so,
/// their spline kept as the NURBS form they are parameterised on.
pub fn record(body: &Body) -> Result<Brep, Error> {
    let (mut record, _) = body.to_record()?;
    for face in &mut record.faces {
        if face.nurbs.is_none() {
            if let Some(quadric) = crate::geometry::quadric_of(&face.surface, face.domain) {
                face.nurbs = Some(std::mem::replace(&mut face.surface, quadric));
            }
        }
    }
    for edge in &mut record.edges {
        if edge.nurbs.is_none() {
            if let Some(circle) = crate::geometry::conic_of(&edge.curve, edge.interval) {
                edge.nurbs = Some(std::mem::replace(&mut edge.curve, circle));
            }
        }
    }
    Ok(record)
}

/// An axis-aligned box from its lower corner.
pub fn make_box(origin: [f64; 3], size: [f64; 3]) -> Result<Brep, Error> {
    record(&solidcore_ops::make_box(origin, size)?)
}

/// A cylinder about +Z from its base centre.
pub fn make_cylinder(origin: [f64; 3], radius: f64, height: f64) -> Result<Brep, Error> {
    record(&solidcore_ops::make_cylinder(origin, radius, height)?)
}

/// A sphere.
pub fn make_sphere(center: [f64; 3], radius: f64) -> Result<Brep, Error> {
    record(&solidcore_ops::make_sphere(center, radius)?)
}

/// A cone or frustum about +Z from its base centre.
pub fn make_cone(origin: [f64; 3], base_radius: f64, top_radius: f64, height: f64) -> Result<Brep, Error> {
    record(&solidcore_ops::make_cone(origin, base_radius, top_radius, height)?)
}

/// A ring torus about +Z.
pub fn make_torus(center: [f64; 3], major_radius: f64, minor_radius: f64) -> Result<Brep, Error> {
    record(&solidcore_ops::make_torus(center, major_radius, minor_radius)?)
}

/// A closed profile extruded `distance` along `direction`, capped (a solid) or not (a sheet).
pub fn extrude(profile: &[solidcore_ops::sweep::Curve], direction: [f64; 3], distance: f64, draft: f64, cap: bool) -> Result<Brep, Error> {
    record(&solidcore_ops::sweep::extrude(
        profile,
        direction,
        distance,
        draft,
        solidcore_ops::sweep::DraftCorner::default(),
        cap,
    )?)
}

/// Sections skinned in order.
pub fn loft(sections: &[Vec<solidcore_ops::sweep::Curve>], degree: u32, cap: bool) -> Result<Brep, Error> {
    record(&solidcore_ops::sweep::loft(sections, degree, cap)?)
}

/// A section skinned to a point.
pub fn loft_to_point(section: &[solidcore_ops::sweep::Curve], apex: [f64; 3]) -> Result<Brep, Error> {
    record(&solidcore_ops::sweep::loft_to_point(section, apex)?)
}

/// `a` and `b` combined; `None` where nothing is left.
pub fn boolean(a: &Brep, b: &Brep, op: BooleanOp) -> Result<Option<Brep>, Error> {
    let (a, _) = open(a)?;
    let (b, _) = open(b)?;
    solidcore_ops::boolean::boolean(a, b, op)?
        .map(|body| record(&body))
        .transpose()
}

/// `brep` moved by the affine `matrix`: rows the images of x, y and z, then the translation
/// (`p' = p·A + t`).
pub fn transform(brep: &Brep, matrix: &[[f64; 4]; 4]) -> Result<Brep, Error> {
    let (body, _) = open(brep)?;
    body.transform(matrix)?;
    record(&body)
}

/// The affine taking x, y and z to `x`, `y` and `z` and the origin to `origin`.
pub fn affine(x: [f64; 3], y: [f64; 3], z: [f64; 3], origin: [f64; 3]) -> [[f64; 4]; 4] {
    [
        [x[0], x[1], x[2], 0.0],
        [y[0], y[1], y[2], 0.0],
        [z[0], z[1], z[2], 0.0],
        [origin[0], origin[1], origin[2], 1.0],
    ]
}

/// The affine of a column-major 4×4.
pub fn affine_of_columns(m: [f64; 16]) -> [[f64; 4]; 4] {
    affine([m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]], [m[12], m[13], m[14]])
}

/// The box `brep` occupies, lowest corner then highest.
pub fn bounds(brep: &Brep) -> Result<([f64; 3], [f64; 3]), Error> {
    open(brep)?.0.bounds()
}

/// The diagonal of the box `brep` occupies, the size its tolerances and meshes scale with.
pub fn size(brep: &Brep) -> f64 {
    match bounds(brep) {
        Ok((low, high)) => (0..3).map(|k| (high[k] - low[k]).powi(2)).sum::<f64>().sqrt(),
        Err(_) => brep
            .vertices
            .iter()
            .flat_map(|a| brep.vertices.iter().map(move |b| crate::geometry::length(crate::geometry::sub(a.point, b.point))))
            .fold(0.0, f64::max),
    }
}

/// How much material `brep` encloses.
pub fn volume(brep: &Brep) -> Result<f64, Error> {
    open(brep)?.0.volume(1e-6)
}

/// `brep`'s area, volume, centroid and moments.
pub fn mass_properties(brep: &Brep) -> Result<solidcore_ops::measure::MassPropertiesEstimate, Error> {
    open(brep)?.0.mass_properties(1e-6)
}

/// Whether `brep` is material: a region of it bounded.
pub fn is_solid(brep: &Brep) -> bool {
    brep.regions.iter().skip(1).any(|region| !region.void)
}

/// The side each face is used on by the first shell using it: its normal is the natural normal
/// of the surface its (u, v) are on where true.
pub fn face_sides(brep: &Brep) -> Vec<bool> {
    let mut sides = vec![true; brep.faces.len()];
    let mut seen = vec![false; brep.faces.len()];
    for shell in &brep.shells {
        if let r::Shell::Faces { faces, .. } = shell {
            for used in faces {
                let at = used.face as usize;
                if at < seen.len() && !seen[at] {
                    seen[at] = true;
                    sides[at] = used.forward;
                }
            }
        }
    }
    sides
}

/// The plane of a planar face: a point on it and its outward normal.
pub fn face_plane(brep: &Brep, face: u32) -> Option<([f64; 3], [f64; 3])> {
    let node = brep.faces.get(face as usize)?;
    let (origin, normal) = match node.parameters() {
        r::Surface::Plane { origin, u_axis, v_axis } => (*origin, cross(*u_axis, *v_axis)),
        r::Surface::PlaneThrough { origin, u_point, v_point } => (
            *origin,
            cross(crate::geometry::sub(*u_point, *origin), crate::geometry::sub(*v_point, *origin)),
        ),
        _ => return None,
    };
    let normal = unit(normal)?;
    let side = face_sides(brep).get(face as usize).copied().unwrap_or(true);
    Some((origin, if side { normal } else { scale(normal, -1.0) }))
}

/// The edges a face's loops use, in loop order, each with whether the loop runs along it.
pub fn face_edges(brep: &Brep, face: u32) -> Vec<(u32, bool)> {
    brep.faces
        .get(face as usize)
        .map(|node| {
            node.loops
                .iter()
                .flat_map(|ring| match ring {
                    r::Loop::Edges { edges } => edges.iter().map(|u| (u.edge, u.forward)).collect(),
                    r::Loop::Vertex { .. } => Vec::new(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The faces each edge bounds.
pub fn edge_faces(brep: &Brep) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); brep.edges.len()];
    for (f, face) in brep.faces.iter().enumerate() {
        for ring in &face.loops {
            if let r::Loop::Edges { edges } = ring {
                for u in edges {
                    if let Some(list) = out.get_mut(u.edge as usize) {
                        if !list.contains(&(f as u32)) {
                            list.push(f as u32);
                        }
                    }
                }
            }
        }
    }
    out
}

/// An edge's points at `count + 1` even steps of its interval, from its first vertex.
pub fn edge_points(brep: &Brep, edge: u32, count: usize) -> Option<Vec<[f64; 3]>> {
    let node = brep.edges.get(edge as usize)?;
    let count = if matches!(node.curve, r::Curve::Line { .. }) { 1 } else { count };
    points_on(node.parameters(), node.interval, count)
}

/// The distance from `p` to the segment `a`–`b`.
pub fn segment_distance(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let ab = crate::geometry::sub(b, a);
    let length2 = dot(ab, ab);
    let t = if length2 > 0.0 { (dot(crate::geometry::sub(p, a), ab) / length2).clamp(0.0, 1.0) } else { 0.0 };
    crate::geometry::length(crate::geometry::sub(p, crate::geometry::add(a, scale(ab, t))))
}

/// The edge of `brep` nearest `p`, by its points.
pub fn nearest_edge(brep: &Brep, p: [f64; 3]) -> Option<u32> {
    (0..brep.edges.len() as u32)
        .filter_map(|edge| {
            let points = edge_points(brep, edge, 64)?;
            let distance = points
                .windows(2)
                .map(|pair| segment_distance(p, pair[0], pair[1]))
                .fold(f64::INFINITY, f64::min);
            Some((edge, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(edge, _)| edge)
}

/// The face of `brep` `p` lies on (within `tolerance`), the nearest one if several; with an
/// infinite `tolerance`, the face nearest `p`.
pub fn face_at_point(brep: &Brep, p: [f64; 3], tolerance: f64) -> Option<u32> {
    let all: Vec<u32> = (0..brep.faces.len() as u32).collect();
    nearest_face(brep, &all, p).filter(|(_, distance)| *distance <= tolerance).map(|(face, _)| face)
}

/// Of `faces`, the one nearest `p` as it is trimmed, and how far.
pub fn nearest_face(brep: &Brep, faces: &[u32], p: [f64; 3]) -> Option<(u32, f64)> {
    let (body, order) = open(brep).ok()?;
    faces
        .iter()
        .filter_map(|&face| {
            let distance = body.face_distances(&order, face as usize, &[p]).ok()?.first().copied()??;
            Some((face, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// A placement: the images of x, y and z (scaled alike) and of the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub z_axis: [f64; 3],
    pub origin: [f64; 3],
}

impl Placement {
    pub const IDENTITY: Placement = Placement {
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
        origin: [0.0; 3],
    };

    /// The placement of a column-major 4×4; `None` when it is not affine.
    pub fn of_columns(m: [f64; 16]) -> Option<Placement> {
        if m.iter().any(|v| !v.is_finite()) || m[3].abs() > 1e-9 || m[7].abs() > 1e-9 || m[11].abs() > 1e-9 || (m[15] - 1.0).abs() > 1e-9 {
            return None;
        }
        Some(Placement {
            x_axis: [m[0], m[1], m[2]],
            y_axis: [m[4], m[5], m[6]],
            z_axis: [m[8], m[9], m[10]],
            origin: [m[12], m[13], m[14]],
        })
    }

    pub fn point(&self, p: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|k| self.origin[k] + self.x_axis[k] * p[0] + self.y_axis[k] * p[1] + self.z_axis[k] * p[2])
    }

    pub fn vector(&self, v: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|k| self.x_axis[k] * v[0] + self.y_axis[k] * v[1] + self.z_axis[k] * v[2])
    }

    /// The common length of its axes when they are square and alike (a similarity).
    pub fn scale(&self) -> Option<f64> {
        let lengths = [self.x_axis, self.y_axis, self.z_axis].map(crate::geometry::length);
        let s = lengths[0];
        let alike = lengths.iter().all(|l| (l - s).abs() <= 1e-9 * s.max(1.0));
        let square = dot(self.x_axis, self.y_axis).abs() <= 1e-9 * s * s
            && dot(self.y_axis, self.z_axis).abs() <= 1e-9 * s * s
            && dot(self.z_axis, self.x_axis).abs() <= 1e-9 * s * s;
        (s.is_finite() && s > 0.0 && alike && square).then_some(s)
    }

    /// `outer` after `self`.
    pub fn then(&self, outer: &Placement) -> Placement {
        Placement {
            origin: outer.point(self.origin),
            x_axis: outer.vector(self.x_axis),
            y_axis: outer.vector(self.y_axis),
            z_axis: outer.vector(self.z_axis),
        }
    }

    /// As Solidcore's affine (rows the images of x, y and z, then the translation).
    pub fn affine(&self) -> [[f64; 4]; 4] {
        affine(self.x_axis, self.y_axis, self.z_axis, self.origin)
    }
}

/// `brep` placed: moved as it stands where the placement is rigid, else read again through
/// Solidcore's assembler with every point, curve and surface placed, so its pcurves are made
/// for the placed geometry.
pub fn placed(brep: &Brep, placement: &Placement) -> Result<Brep, Error> {
    if *placement == Placement::IDENTITY {
        return Ok(brep.clone());
    }
    let matrix = placement.affine();
    if let Some(moved) = crate::geometry::rigidly_moved(brep, &matrix) {
        return Ok(moved);
    }
    crate::geometry::reassembled(brep, &matrix).ok_or(Error::Argument)
}

fn sweep_line(start: [f64; 3], end: [f64; 3]) -> solidcore_ops::sweep::Curve {
    solidcore_ops::sweep::Curve::Line { start, end }
}

/// A closed polygon through `corners`, as sweep curves.
pub fn polygon(corners: &[[f64; 3]]) -> Vec<solidcore_ops::sweep::Curve> {
    (0..corners.len()).map(|i| sweep_line(corners[i], corners[(i + 1) % corners.len()])).collect()
}

/// A full ellipse about `centre` in the plane z = centre.z, as sweep curves.
pub fn ellipse(centre: [f64; 3], x_radius: f64, y_radius: f64) -> Vec<solidcore_ops::sweep::Curve> {
    let frame = r::Frame { origin: centre, x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] };
    let curve = if (x_radius - y_radius).abs() <= 1e-12 * x_radius.abs().max(1.0) {
        r::Curve::Circle { frame, radius: x_radius }
    } else {
        r::Curve::Ellipse { frame, radii: [x_radius, y_radius] }
    };
    solidcore_ops::sweep::Curve::from_record(&curve, [0.0, std::f64::consts::TAU]).unwrap_or_default()
}

/// Right triangular prism: a right triangle in XZ from `origin` (full height at x = 0, nothing
/// at x = length), extruded along +Y.
pub fn make_wedge(origin: [f64; 3], length: f64, width: f64, height: f64) -> Result<Brep, Error> {
    let at = |x: f64, z: f64| [origin[0] + x, origin[1], origin[2] + z];
    extrude(&polygon(&[at(0.0, 0.0), at(length, 0.0), at(0.0, height)]), [0.0, 1.0, 0.0], width, 0.0, true)
}

/// A circular or elliptical cylinder about +Z from its base centre.
pub fn make_elliptical_cylinder(origin: [f64; 3], x_radius: f64, y_radius: f64, height: f64) -> Result<Brep, Error> {
    if (x_radius - y_radius).abs() <= 1e-12 * x_radius.abs().max(1.0) {
        return make_cylinder(origin, x_radius, height);
    }
    extrude(&ellipse(origin, x_radius, y_radius), [0.0, 0.0, 1.0], height, 0.0, true)
}

/// A circular or elliptical cone or frustum about +Z from its base centre; an elliptical one
/// keeps its base's proportions up to the top.
pub fn make_frustum(origin: [f64; 3], base_x_radius: f64, base_y_radius: f64, top_radius: f64, height: f64) -> Result<Brep, Error> {
    let scale = base_x_radius.abs().max(base_y_radius.abs()).max(1.0);
    if (base_x_radius - base_y_radius).abs() <= 1e-12 * scale {
        return make_cone(origin, base_x_radius, top_radius, height);
    }
    // The circular frustum on the x radius, squeezed along y about the axis: exact, a cone's
    // wall stays a quadric.
    let k = base_y_radius / base_x_radius;
    let circular = make_cone(origin, base_x_radius, top_radius, height)?;
    let squeeze = [[1.0, 0.0, 0.0, 0.0], [0.0, k, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, origin[1] * (1.0 - k), 0.0, 1.0]];
    transform(&circular, &squeeze)
}

fn regular_polygon(centre: [f64; 3], radius: f64, sides: usize) -> Vec<[f64; 3]> {
    (0..sides)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / sides as f64;
            [centre[0] + radius * angle.cos(), centre[1] + radius * angle.sin(), centre[2]]
        })
        .collect()
}

/// A pyramid or polygonal frustum about +Z on a regular polygon of `sides` corners.
pub fn make_pyramid_frustum(origin: [f64; 3], base_radius: f64, top_radius: f64, height: f64, sides: usize) -> Result<Brep, Error> {
    if sides < 3 || !(base_radius > 0.0) || !(height > 0.0) || !(top_radius >= 0.0) {
        return Err(Error::Argument);
    }
    let base = polygon(&regular_polygon(origin, base_radius, sides));
    let top = [origin[0], origin[1], origin[2] + height];
    if top_radius <= 1e-12 {
        return loft_to_point(&base, top);
    }
    loft(&[base, polygon(&regular_polygon(top, top_radius, sides))], 1, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scaled_or_reflected_body_keeps_its_material() {
        let cylinder = make_cylinder([1.0, 2.0, 3.0], 2.0, 5.0).unwrap();
        let before = volume(&cylinder).unwrap();
        let doubled = Placement { x_axis: [0.0, 2.0, 0.0], y_axis: [-2.0, 0.0, 0.0], z_axis: [0.0, 0.0, 2.0], origin: [5.0, 0.0, 0.0] };
        let after = volume(&placed(&cylinder, &doubled).unwrap()).unwrap();
        assert!((after - 8.0 * before).abs() < 1e-6 * after, "{before} {after}");
        let wedge = make_wedge([0.0; 3], 4.0, 2.0, 3.0).unwrap();
        let mirror = Placement { x_axis: [-1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0], z_axis: [0.0, 0.0, 1.0], origin: [0.0; 3] };
        let reflected = placed(&wedge, &mirror).unwrap();
        assert!((volume(&reflected).unwrap() - 12.0).abs() < 1e-6);
        let (low, _) = bounds(&reflected).unwrap();
        assert!((low[0] + 4.0).abs() < 1e-9, "{low:?}");
    }
}

/// `brep` cut by the plane through `origin` across unit `normal` (reaching `reach` each way of
/// `origin`): the part behind the plane, then the part in front; `None` when the plane misses or
/// a side is empty.
pub fn split_by_plane(brep: &Brep, origin: [f64; 3], normal: [f64; 3], reach: f64) -> Result<Option<(Brep, Brep)>, Error> {
    let x = crate::geometry::unit(cross(normal, if normal[0].abs() < 0.8 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] })).ok_or(Error::Argument)?;
    let f = crate::geometry::frame(origin, x, normal).ok_or(Error::Argument)?;
    let tool = solidcore_ops::split::Tool {
        surface: r::Surface::Plane { origin: f.origin, u_axis: f.x_axis, v_axis: f.y_axis },
        domain: [[-reach, reach], [-reach, reach]],
    };
    let (body, _) = open(brep)?;
    match solidcore_ops::split::split_body(&body, &tool)? {
        Some((back, front)) => Ok(Some((record(&back)?, record(&front)?))),
        None => Ok(None),
    }
}

/// Runs an operation that edits Solidcore's working body in place, and records the result.
fn edited(brep: &Brep, f: impl FnOnce(&Body, &Order) -> Result<(), Error>) -> Result<Brep, Error> {
    let (body, order) = open(brep)?;
    f(&body, &order)?;
    record(&body)
}

fn indices(list: &[u32]) -> Vec<usize> {
    list.iter().map(|i| *i as usize).collect()
}

/// `edges` rounded at `radius` by Solidcore's rolling ball, their corners filled.
pub fn fillet(brep: &Brep, edges: &[u32], radius: f64) -> Result<Brep, Error> {
    use solidcore_ops::blend::{blend_ordered, Blend, BlendOptions, Corners};
    edited(brep, |body, order| {
        blend_ordered(body, order, Blend::Round, &indices(edges), &[radius], &[], Corners::Filled, BlendOptions::default()).map(|_| ())
    })
}

/// `edges` chamfered `base` back along `base_face` and `other` along each edge's other face.
pub fn chamfer(brep: &Brep, edges: &[u32], base_face: u32, base: f64, other: f64) -> Result<Brep, Error> {
    use solidcore_ops::blend::{chamfer_sides_ordered, ChamferSecond};
    edited(brep, |body, order| {
        let along = vec![base_face as usize; edges.len()];
        chamfer_sides_ordered(body, order, &indices(edges), &along, base, ChamferSecond::Distance(other)).map(|_| ())
    })
}

/// `brep` hollowed into a wall `thickness` thick (negative: inward), opened at `open`.
pub fn shell(brep: &Brep, open_faces: &[u32], thickness: f64) -> Result<Brep, Error> {
    edited(brep, |body, order| solidcore_ops::offset::shell(body, order, &indices(open_faces), thickness))
}

/// A sheet thickened by `distance` along its faces' normals into a solid.
pub fn thicken(brep: &Brep, distance: f64) -> Result<Brep, Error> {
    edited(brep, |body, _| solidcore_ops::offset::thicken(body, distance, false))
}

/// `faces` moved `distance` along their outward normals, their neighbours following.
pub fn offset_faces(brep: &Brep, faces: &[u32], distance: f64) -> Result<Brep, Error> {
    edited(brep, |body, order| solidcore_ops::offset::offset_faces(body, order, &indices(faces), distance))
}

/// `faces` drafted `angle` about the neutral plane through `base` across `neutral`, pulled
/// along `pull`.
pub fn draft(brep: &Brep, faces: &[u32], base: [f64; 3], neutral: [f64; 3], pull: [f64; 3], angle: f64) -> Result<Brep, Error> {
    edited(brep, |body, order| solidcore_ops::offset::draft(body, order, &indices(faces), base, neutral, pull, angle))
}

/// `faces` taken out of `brep` (left open, or healed where Solidcore can close the gap).
pub fn remove_faces(brep: &Brep, faces: &[u32]) -> Result<Brep, Error> {
    let (body, order) = open(brep)?;
    record(&solidcore_ops::trim::remove_faces(&body, &order, &indices(faces))?)
}

/// The solid `brep` cut by the sheet `cutter` into the part behind its face `face`'s normal
/// and the part in front.
pub fn split_by_sheet(brep: &Brep, cutter: &Brep, face: u32) -> Result<Option<(Brep, Brep)>, Error> {
    let (body, _) = open(brep)?;
    let (sheet, order) = open(cutter)?;
    match solidcore_ops::split::split_body_by_sheet(&body, &sheet, &order, face as usize)? {
        Some((back, front)) => Ok(Some((record(&back)?, record(&front)?))),
        None => Ok(None),
    }
}

/// `tool` imprinted on `brep`: its faces split where `tool` meets them; whether it did.
pub fn imprint(brep: &Brep, tool: &Brep) -> Result<(Brep, bool), Error> {
    let (body, _) = open(brep)?;
    let (tool, _) = open(tool)?;
    let (body, changed) = solidcore_ops::boolean::imprint(body, tool)?;
    Ok((record(&body)?, changed))
}

/// The solids of `brep`, each its own record.
pub fn separated(brep: &Brep) -> Vec<Brep> {
    let material: Vec<usize> = brep
        .regions
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, region)| !region.void)
        .map(|(i, _)| i)
        .collect();
    if material.len() <= 1 {
        return vec![brep.clone()];
    }
    material
        .into_iter()
        .filter_map(|region| {
            let faces: Vec<u32> = brep.regions[region]
                .shells
                .iter()
                .filter_map(|s| match brep.shells.get(*s as usize) {
                    Some(r::Shell::Faces { faces, .. }) => Some(faces.iter().map(|f| f.face).collect::<Vec<_>>()),
                    _ => None,
                })
                .flatten()
                .collect();
            face_sheet(brep, &faces).and_then(|sheet| sewn(&[&sheet], brep.tolerance.max(1e-6)).ok())
        })
        .collect()
}

/// `faces` of `brep` as a sheet of their own, each on the side it had.
pub fn face_sheet(brep: &Brep, faces: &[u32]) -> Option<Brep> {
    let mut sheet = crate::display::subset(brep, faces)?;
    let sides = face_sides(brep);
    if let Some(r::Shell::Faces { faces: used, .. }) = sheet.shells.first_mut() {
        for (used, original) in used.iter_mut().zip(faces) {
            used.forward = *sides.get(*original as usize)?;
        }
    }
    // The subset's edge uses still name the faces they had: the assembler makes them again.
    crate::geometry::reassembled(&sheet, &crate::geometry::IDENTITY)
}

/// Sheets sewn along their shared edges into one body (a solid where they close).
pub fn sewn(breps: &[&Brep], tolerance: f64) -> Result<Brep, Error> {
    let bodies: Vec<Body> = breps.iter().map(|b| open(b).map(|(body, _)| body)).collect::<Result<_, _>>()?;
    let refs: Vec<&Body> = bodies.iter().collect();
    let sewn = solidcore_ops::stitch::sew(&refs, tolerance)?;
    record(&sewn.body)
}

/// An edge of a planar face as a 2D curve in `plane`: a line, an arc, or its NURBS form put in
/// the plane, cut to the edge.
pub fn edge_on_plane(plane: &Plane, edge: &r::Edge) -> Option<Curve> {
    let project = |p: [f64; 3]| plane.project(p);
    let at = |t: f64| crate::geometry::point_on(edge.parameters(), t);
    let [a, b] = edge.interval;
    match &edge.curve {
        r::Curve::Line { .. } => Some(Curve::Line(Line { start: project(at(a)?)?, end: project(at(b)?)? })),
        r::Curve::Circle { frame, radius } if plane.is_orthonormal() => {
            let centre = project(frame.origin)?;
            let angle = |p: [f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
            let (start, middle, end) = (project(at(a)?)?, project(at(0.5 * (a + b))?)?, project(at(b)?)?);
            let (s, m, e) = (angle(start), angle(middle), angle(end));
            let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
            let closed = (start[0] - end[0]).hypot(start[1] - end[1]) <= 1e-9 * radius.max(1.0);
            let scale = Vec3::from(plane.x_axis).length();
            let radius = radius / scale;
            Some(if closed {
                Curve::Circle(Circle { centre, radius })
            } else if ccw(s, m) < ccw(s, e) {
                Curve::Arc(Arc { centre, radius, start_angle: s, end_angle: s + ccw(s, e) })
            } else {
                Curve::Arc(Arc { centre, radius, start_angle: e, end_angle: e + ccw(e, s) })
            })
        }
        r::Curve::Ellipse { frame, radii } if plane.is_orthonormal() => {
            let centre = project(frame.origin)?;
            let tip = project(crate::geometry::add(frame.origin, scale(frame.x_axis, radii[0])))?;
            let major = [tip[0] - centre[0], tip[1] - centre[1]];
            let length = major[0].hypot(major[1]);
            let major = [major[0] / length, major[1] / length];
            let minor = [-major[1], major[0]];
            let scale_2d = Vec3::from(plane.x_axis).length();
            let ellipse = Ellipse { centre, major_radius: radii[0] / scale_2d, minor_radius: radii[1] / scale_2d, major_axis: major };
            let parameter = |p: [f64; 2]| {
                let d = [p[0] - centre[0], p[1] - centre[1]];
                ((d[0] * minor[0] + d[1] * minor[1]) / ellipse.minor_radius).atan2((d[0] * major[0] + d[1] * major[1]) / ellipse.major_radius)
            };
            let (start, middle, end) = (project(at(a)?)?, project(at(0.5 * (a + b))?)?, project(at(b)?)?);
            let (s, m, e) = (parameter(start), parameter(middle), parameter(end));
            let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
            let closed = (start[0] - end[0]).hypot(start[1] - end[1]) <= 1e-9 * radii[0].max(1.0);
            Some(Curve::Ellipse(if closed {
                EllipseArc::full(ellipse)
            } else if ccw(s, m) < ccw(s, e) {
                EllipseArc { ellipse, start_parameter: s, end_parameter: s + ccw(s, e) }
            } else {
                EllipseArc { ellipse, start_parameter: e, end_parameter: e + ccw(e, s) }
            }))
        }
        _ => {
            let (degree, points, weights, knots) = match edge.parameters() {
                r::Curve::Spline { knots, points } => (knots.len() - points.len() - 1, points.clone(), vec![1.0; points.len()], knots.clone()),
                r::Curve::RationalSpline { knots, homogeneous } => (
                    knots.len() - homogeneous.len() - 1,
                    homogeneous.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).collect(),
                    homogeneous.iter().map(|h| h[3]).collect(),
                    knots.clone(),
                ),
                _ => return None,
            };
            let flat = points.iter().map(|p| project(*p)).collect::<Option<Vec<_>>>()?;
            let (low, high) = (knots[degree], knots[knots.len() - degree - 1]);
            let curve = NurbsCurve::new(degree, flat, knots, Some(weights))?;
            let (from, to) = ((a - low) / (high - low), (b - low) / (high - low));
            let whole = from <= 1e-12 && to >= 1.0 - 1e-12;
            Some(Curve::Nurbs(if whole { curve } else { curve.trimmed(from, to)? }))
        }
    }
}


/// A planar face's plane, and its loops as 2D curves in it.
#[derive(Debug, Clone)]
pub struct PlanarFaceProfile {
    pub plane: Plane,
    /// The first loop encloses the face; the others cut holes.
    pub loops: Vec<Vec<Curve>>,
    /// The face's unit outward normal.
    pub outward: [f64; 3],
}

/// A planar face's profile; `None` for a face on another surface.
pub fn planar_face_profile(brep: &Brep, face: u32) -> Option<PlanarFaceProfile> {
    let node = brep.faces.get(face as usize)?;
    let (origin, normal) = face_plane(brep, face)?;
    let (u, _) = match node.parameters() {
        r::Surface::Plane { u_axis, v_axis, .. } => (*u_axis, *v_axis),
        r::Surface::PlaneThrough { origin: o, u_point, .. } => (crate::geometry::sub(*u_point, *o), [0.0; 3]),
        _ => return None,
    };
    let f = crate::geometry::frame(origin, u, normal)?;
    let plane = Plane::from_axes(f.origin, f.x_axis, f.y_axis);
    let wires = node
        .loops
        .iter()
        .filter_map(|ring| match ring {
            r::Loop::Edges { edges } => Some(edges),
            r::Loop::Vertex { .. } => None,
        })
        .map(|uses| uses.iter().map(|u| edge_on_plane(&plane, brep.edges.get(u.edge as usize)?)).collect::<Option<Vec<_>>>())
        .collect::<Option<Vec<_>>>()?;
    Some(PlanarFaceProfile { plane, loops: wires, outward: normal })
}

/// A planar sheet bounded by `loops` in `plane` (the first the outer, the rest holes).
pub fn planar_region(plane: Plane, loops: &[Vec<Curve>]) -> Option<Brep> {
    let wires = crate::sweep::profile_loops(&plane, loops)?;
    let curves: Vec<solidcore_ops::sweep::Curve> = wires.into_iter().flat_map(|(curves, _)| curves).collect();
    record(&solidcore_ops::sweep::planar_faces(&curves).ok()?).ok()
}

/// What two planar regions share.
pub enum PlanarIntersection {
    Area(Brep),
    Touching,
    Disjoint,
}

/// Planar regions combined, two at a time.
pub fn region_boolean(a: &Brep, b: &Brep, op: BooleanOp) -> Result<Option<Brep>, Error> {
    let (a, _) = open(a)?;
    let (b, _) = open(b)?;
    solidcore_ops::boolean::boolean2d(a, b, op)?.map(|body| record(&body)).transpose()
}

/// Coplanar regions united.
pub fn union_regions(bodies: &[Brep]) -> Result<Brep, Error> {
    let mut iter = bodies.iter();
    let mut out = iter.next().ok_or(Error::Empty)?.clone();
    for next in iter {
        out = region_boolean(&out, next, BooleanOp::Union)?.ok_or(Error::Empty)?;
    }
    Ok(out)
}

/// Coplanar regions less `cutters`.
pub fn subtract_regions(bases: &[Brep], cutters: &[Brep]) -> Result<Brep, Error> {
    let mut out = union_regions(bases)?;
    for cutter in cutters {
        out = region_boolean(&out, cutter, BooleanOp::Subtract)?.ok_or(Error::Empty)?;
    }
    Ok(out)
}

/// What coplanar regions all share.
pub fn intersect_regions(bodies: &[Brep]) -> Result<PlanarIntersection, Error> {
    let mut iter = bodies.iter();
    let mut out = iter.next().ok_or(Error::Empty)?.clone();
    for next in iter {
        match region_boolean(&out, next, BooleanOp::Intersect)? {
            Some(shared) => out = shared,
            None => return Ok(PlanarIntersection::Disjoint),
        }
    }
    Ok(PlanarIntersection::Area(out))
}

/// Solids united into one.
pub fn union_all(bodies: &[Brep]) -> Result<Brep, Error> {
    let opened = bodies.iter().map(|b| open(b).map(|(body, _)| body)).collect::<Result<Vec<_>, _>>()?;
    record(&solidcore_ops::boolean::union_all(opened)?)
}

/// `base` less every cutter (united first, so a plate drilled with a hundred holes is cut once).
pub fn subtract_all(base: &Brep, cutters: &[Brep]) -> Result<Option<Brep>, Error> {
    if cutters.is_empty() {
        return Ok(Some(base.clone()));
    }
    let tools = union_all(cutters)?;
    boolean(base, &tools, BooleanOp::Subtract)
}

/// `brep` cut by the plane through `origin` across `normal`, far enough to cross it whole.
pub fn slice(brep: &Brep, origin: [f64; 3], normal: [f64; 3]) -> Result<Option<(Brep, Brep)>, Error> {
    let reach = size(brep) * 4.0 + crate::geometry::length(crate::geometry::sub(origin, bounds(brep).map(|(l, _)| l).unwrap_or(origin))) + 1.0;
    split_by_plane(brep, origin, normal, reach)
}

/// How far `p` lies in front of the sheet `sheet` (along its first face's normal), negative
/// behind it.
pub fn sheet_side(sheet: &Brep, p: [f64; 3]) -> Option<f64> {
    let surface = solidcore_ops::face_surface::FaceSurface::of(sheet, 0).ok()?;
    let [[u0, u1], [v0, v1]] = surface.domain();
    let closest = surface.closest_point(p, [0.5 * (u0 + u1), 0.5 * (v0 + v1)]).ok()?;
    let normal = surface.normal(closest.uv).ok()?;
    let side = face_sides(sheet).first().copied().unwrap_or(true);
    let d = dot(crate::geometry::sub(p, closest.point), normal);
    Some(if side { d } else { -d })
}

/// What is wrong with `brep`, in Solidcore's words; empty for a sound record (a solid also
/// checked as one).
pub fn check(brep: &Brep) -> Vec<String> {
    let mut problems = Vec::new();
    if let Err(e) = solidcore_ops::record_check::validate(brep) {
        problems.push(format!("{e:?}"));
        return problems;
    }
    match open(brep) {
        Ok((body, _)) if is_solid(brep) => {
            if let Err(e) = body.check_solid() {
                problems.push(format!("{e:?}"));
            }
        }
        Ok(_) => {}
        Err(e) => problems.push(format!("{e:?}")),
    }
    problems
}

/// Planar `faces` extruded `height` along their outward normals (tapered by `taper`), added
/// to the body (or taken from it for a negative height).
pub fn extrude_faces(brep: &Brep, faces: &[u32], height: f64, taper: f64) -> Result<Brep, Error> {
    let mut out = brep.clone();
    for face in faces {
        let profile = planar_face_profile(brep, *face).ok_or(Error::Argument)?;
        out = pull_region(&out, &profile, height, taper)?;
    }
    Ok(out)
}

/// The region `profile` extruded `height` along its outward normal (tapered by `taper`),
/// added to the body (or taken from it for a negative height).
pub fn pull_region(brep: &Brep, profile: &PlanarFaceProfile, height: f64, taper: f64) -> Result<Brep, Error> {
    if !height.is_finite() || height == 0.0 || profile.loops.is_empty() {
        return Err(Error::Argument);
    }
    let direction = if height >= 0.0 { profile.outward } else { scale(profile.outward, -1.0) };
    let loops = crate::sweep::profile_loops(&profile.plane, &profile.loops).ok_or(Error::Argument)?;
    let mut tool = extrude(&loops[0].0, direction, height.abs(), taper, true)?;
    for (hole, _) in &loops[1..] {
        let cut = extrude(hole, direction, height.abs(), taper, true)?;
        tool = boolean(&tool, &cut, BooleanOp::Subtract)?.ok_or(Error::Empty)?;
    }
    let op = if height >= 0.0 { BooleanOp::Union } else { BooleanOp::Subtract };
    boolean(brep, &tool, op)?.ok_or(Error::Empty)
}

/// The planar face of `brep` whose region holds `p` (within `tolerance`).
pub fn planar_face_at_point(brep: &Brep, p: [f64; 3], tolerance: f64) -> Option<u32> {
    let planar: Vec<u32> = (0..brep.faces.len() as u32).filter(|face| face_plane(brep, *face).is_some()).collect();
    nearest_face(brep, &planar, p).filter(|(_, distance)| *distance <= tolerance).map(|(face, _)| face)
}

/// `curves` imprinted on `brep`: each cuts the face its middle lies on. Whether any did.
// ponytail: a curve crossing several faces cuts only the one its middle lies on.
pub fn imprint_curves(brep: &Brep, curves: &[(solidcore_ops::sweep::Curve, [f64; 3])]) -> Result<(Brep, bool), Error> {
    let tolerance = size(brep).max(1.0) * 1e-6;
    let mut out = brep.clone();
    let mut cut = false;
    for (curve, middle) in curves {
        let Some(face) = face_at_point(&out, *middle, tolerance) else {
            continue;
        };
        if let Ok((next, made)) = split_face(&out, face, std::slice::from_ref(curve), tolerance) {
            cut |= made.len() > 1;
            out = next;
        }
    }
    Ok((out, cut))
}

/// Faces merged with their like neighbours where an edge between them divides one surface.
pub fn clean(brep: &Brep) -> Result<Brep, Error> {
    let (body, order) = open(brep)?;
    record(&solidcore_ops::stitch::merge_faces(body, &order, &[])?)
}

/// A sheet offset `distance` along its normals.
pub fn offset_sheet(brep: &Brep, distance: f64) -> Result<Brep, Error> {
    edited(brep, |body, _| solidcore_ops::offset::offset_sheet(body, distance))
}

/// `face` of `brep` split along `curves` lying on it; the regions it became, each a face of
/// the result with a point inside.
pub fn split_face(brep: &Brep, face: u32, curves: &[solidcore_ops::sweep::Curve], tolerance: f64) -> Result<(Brep, Vec<(u32, [f64; 3])>), Error> {
    let (body, order) = open(brep)?;
    let (cut, regions) = solidcore_ops::trim::split_face_by_curves(&body, &order, face as usize, curves, tolerance)?;
    Ok((record(&cut)?, regions.into_iter().map(|region| (region.face as u32, region.inside)).collect()))
}

/// The box `brep` occupies, as an [`Aabb`](crate::geometry::Aabb).
pub fn aabb(brep: &Brep) -> Option<crate::geometry::Aabb> {
    bounds(brep).ok().map(|(min, max)| crate::geometry::Aabb { min, max })
}

/// A closed polyhedron: `faces` polygons (vertex indices running counter-clockwise seen from
/// outside) on `vertices`, assembled into a solid.
pub fn faceted_solid(vertices: &[[f64; 3]], faces: &[Vec<usize>]) -> Option<Brep> {
    use solidcore_step::assembly::{Assembly, Bound, Kind};
    static TRACED: crate::geometry::Traced = crate::geometry::Traced;
    let mut assembly = Assembly::new(1e-6, Some(&TRACED));
    let ids: Vec<u32> = vertices.iter().map(|p| assembly.add_vertex(*p).ok()).collect::<Option<_>>()?;
    let mut edges: std::collections::HashMap<(usize, usize), u32> = std::collections::HashMap::new();
    let mut shell = Vec::with_capacity(faces.len());
    for (index, face) in faces.iter().enumerate() {
        if face.len() < 3 {
            return None;
        }
        let points: Vec<[f64; 3]> = face.iter().map(|v| vertices.get(*v).copied()).collect::<Option<_>>()?;
        // Newell's normal, the polygon's outward side.
        let normal = (0..points.len()).fold([0.0; 3], |n, i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            [n[0] + (a[1] - b[1]) * (a[2] + b[2]), n[1] + (a[2] - b[2]) * (a[0] + b[0]), n[2] + (a[0] - b[0]) * (a[1] + b[1])]
        });
        let f = crate::geometry::frame(points[0], crate::geometry::sub(points[1], points[0]), normal)?;
        let mut uses = Vec::with_capacity(face.len());
        for i in 0..face.len() {
            let (a, b) = (face[i], face[(i + 1) % face.len()]);
            let key = (a.min(b), a.max(b));
            let edge = match edges.get(&key) {
                Some(edge) => *edge,
                None => {
                    let curve = r::Curve::Line { start: vertices[key.0], end: vertices[key.1] };
                    let (edge, _) = assembly.add_edge(index as u64, curve, [0.0, 1.0], [ids[key.0], ids[key.1]]).ok()?;
                    edges.insert(key, edge);
                    edge
                }
            };
            uses.push((edge, a == key.0));
        }
        let surface = r::Surface::Plane { origin: f.origin, u_axis: f.x_axis, v_axis: f.y_axis };
        let added = assembly.add_face(index as u64, surface, vec![Bound::Edges(uses)], Some(0), true).ok()?;
        shell.push((added, true));
    }
    assembly.finish(0, String::new(), vec![shell], Kind::Solid).ok().map(|solid| solid.brep)
}
