//! Blends that only touch the faces around the edge, so the rest of the body
//! may be concave or carry other curved faces.
//!
//! A rim is a full circle where a plane meets a coaxial cylinder: the edge of
//! a drilled hole, or the top and foot of a boss. The plane's loop moves out
//! to the blend's far rim, the cylinder is shortened to its near rim, and a
//! torus (fillet) or cone (chamfer) band is stitched in between.
//!
//! A line is a straight edge between two planes whose ends are plain
//! three-edge corners. Both planes are set back to the blend's tangent
//! lines, the end faces gain the blend's section, and a cylinder (fillet)
//! or plane (chamfer) band fills the gap.

use super::{
    Body, Circle3, Coedge, CoedgeKey, Cone, Curve3, Cylinder, Edge, EdgeKey, Face, FaceKey, Line3,
    Loop, Provenance, Surface, Torus, Vertex, VertexKey,
};
use ocs_geom::space::{Plane, Vec3};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Why a local blend was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalError {
    /// The band would reach another boundary of a face it trims.
    TooLarge,
    /// The chamfer's base face is neither face at the edge.
    OutsideBaseFace(EdgeKey),
    /// The stitched body did not validate.
    InvalidResult,
}

/// How each edge is blended.
#[derive(Debug, Clone, Copy)]
pub(super) enum LocalBlend {
    Fillet(f64),
    Chamfer { base_face: FaceKey, base: f64, other: f64 },
}

impl LocalBlend {
    /// The setbacks on `first` and `second`, or `None` when a chamfer's base
    /// face is neither.
    fn distances(self, first: FaceKey, second: FaceKey) -> Option<(f64, f64)> {
        match self {
            Self::Fillet(radius) => Some((radius, radius)),
            Self::Chamfer { base_face, base, other } if base_face == first => Some((base, other)),
            Self::Chamfer { base_face, base, other } if base_face == second => Some((other, base)),
            Self::Chamfer { .. } => None,
        }
    }
}

/// `None` unless every selected edge is such a rim, leaving the selection to
/// the other solvers.
pub(super) fn blend_rims(
    body: &Body,
    selected: &[EdgeKey],
    blend: LocalBlend,
) -> Option<Result<Body, LocalError>> {
    let tolerance = super::operation_tolerance(&[body]);
    if selected.iter().any(|edge| rim(body, *edge, tolerance).is_none()) {
        return None;
    }
    let mut result = body.clone();
    for edge in selected {
        // Earlier rims may have moved this one's neighbours.
        let Some(rim) = rim(&result, *edge, tolerance) else {
            return Some(Err(LocalError::TooLarge));
        };
        let Some((along_plane, along_cylinder)) =
            blend.distances(rim.plane_face, rim.cylinder_face)
        else {
            return Some(Err(LocalError::OutsideBaseFace(*edge)));
        };
        let round = matches!(blend, LocalBlend::Fillet(_));
        if let Err(error) = stitch(&mut result, &rim, along_plane, along_cylinder, round, tolerance)
        {
            return Some(Err(error));
        }
    }
    if !result.validate().is_empty() || result.worst_vertex_gap() > tolerance {
        return Some(Err(LocalError::InvalidResult));
    }
    Some(Ok(result))
}

struct Rim {
    edge: EdgeKey,
    vertex: VertexKey,
    plane_face: FaceKey,
    cylinder_face: FaceKey,
    cylinder_coedge: CoedgeKey,
    seam: EdgeKey,
    circle: Circle3,
    axis: Vec3,
    centre: Vec3,
    /// +1 when the plane face lies outside the circle, -1 inside.
    along_plane: f64,
    /// +1 when the cylinder face runs up the axis from the circle, -1 down.
    along_cylinder: f64,
    convex: bool,
}

fn rim(body: &Body, key: EdgeKey, tolerance: f64) -> Option<Rim> {
    let edge = body.edges.get(key)?;
    let Curve3::Circle(circle) = *body.curves.get(edge.curve)? else {
        return None;
    };
    if ((edge.end_parameter - edge.start_parameter) - TAU).abs() > 1e-8
        || edge.start != edge.end
        || edge.coedges.len() != 2
    {
        return None;
    }
    let face_of = |coedge: CoedgeKey| {
        let owner = body.coedges.get(coedge)?.owner;
        Some(body.loops.get(owner)?.owner)
    };
    let mut sides = [
        (edge.coedges[0], face_of(edge.coedges[0])?),
        (edge.coedges[1], face_of(edge.coedges[1])?),
    ];
    let surface = |face: FaceKey| body.surfaces.get(body.faces.get(face)?.surface);
    if matches!(surface(sides[0].1)?, Surface::Cylinder(_)) {
        sides.swap(0, 1);
    }
    let [(_, plane_face), (cylinder_coedge, cylinder_face)] = sides;
    let (Surface::Plane(plane), Surface::Cylinder(cylinder)) =
        (surface(plane_face)?, surface(cylinder_face)?)
    else {
        return None;
    };
    let axis = Vec3::from(circle.plane.normal()?);
    let centre = Vec3::from(circle.plane.origin);
    let cylinder_axis = Vec3::from(cylinder.base.normal()?);
    let offset = centre - Vec3::from(cylinder.base.origin);
    if Vec3::from(plane.normal()?).dot(axis).abs() < 1.0 - 1e-9
        || plane.distance_to(centre.to_array())?.abs() > tolerance
        || cylinder_axis.dot(axis).abs() < 1.0 - 1e-9
        || (offset - cylinder_axis * offset.dot(cylinder_axis)).length() > tolerance
        || (cylinder.radius - circle.radius).abs() > tolerance
    {
        return None;
    }

    // The rim's vertex may only carry the cylinder's straight seam.
    let vertex = edge.start;
    let mut seam = None;
    for (other, candidate) in body.edges.iter() {
        if other == key || (candidate.start != vertex && candidate.end != vertex) {
            continue;
        }
        let in_cylinder = candidate
            .coedges
            .iter()
            .all(|coedge| face_of(*coedge) == Some(cylinder_face));
        if seam.is_some()
            || !in_cylinder
            || !matches!(body.curves.get(candidate.curve)?, Curve3::Line(_))
        {
            return None;
        }
        seam = Some(other);
    }
    let seam = seam?;
    let seam_edge = body.edges.get(seam)?;
    let far = if seam_edge.start == vertex { seam_edge.end } else { seam_edge.start };
    let at = |key: VertexKey| Some(Vec3::from(body.vertices.get(key)?.point));
    let rise = (at(far)? - at(vertex)?).dot(axis);
    if rise.abs() <= tolerance {
        return None;
    }

    let plane_out = Vec3::from(plane.normal()?)
        * if body.faces.get(plane_face)?.forward { 1.0 } else { -1.0 };
    let cylinder_out = if body.faces.get(cylinder_face)?.forward { 1.0 } else { -1.0 };
    let along_cylinder = rise.signum();
    // Convex when the cylinder runs away from the plane's outside.
    let convex = along_cylinder * plane_out.dot(axis) < 0.0;
    let along_plane = if convex { -cylinder_out } else { cylinder_out };
    Some(Rim {
        edge: key,
        vertex,
        plane_face,
        cylinder_face,
        cylinder_coedge,
        seam,
        circle,
        axis,
        centre,
        along_plane,
        along_cylinder,
        convex,
    })
}

/// Points along every boundary of `face` except the rim and its seam.
fn other_boundary_points(body: &Body, face: FaceKey, skip: &[EdgeKey]) -> Option<Vec<Vec3>> {
    let mut points = Vec::new();
    for ring in &body.faces.get(face)?.loops {
        for coedge in &body.loops.get(*ring)?.coedges {
            let key = body.coedges.get(*coedge)?.edge;
            if skip.contains(&key) {
                continue;
            }
            let edge = body.edges.get(key)?;
            let curve = body.curves.get(edge.curve)?;
            // ponytail: sampled clearance; a boundary bulging between
            // samples toward the band can still slip past.
            for step in 0..=32 {
                let t = edge.start_parameter
                    + (edge.end_parameter - edge.start_parameter) * f64::from(step) / 32.0;
                points.push(Vec3::from(curve.point_at(t)));
            }
        }
    }
    Some(points)
}

fn stitch(
    body: &mut Body,
    rim: &Rim,
    along_plane: f64,
    along_cylinder: f64,
    round: bool,
    tolerance: f64,
) -> Result<(), LocalError> {
    let radius = rim.circle.radius;
    let outer = radius + along_plane * rim.along_plane;
    let drop = along_cylinder * rim.along_cylinder;
    if outer <= tolerance {
        return Err(LocalError::TooLarge);
    }
    let clear_plane = other_boundary_points(body, rim.plane_face, &[rim.edge])
        .ok_or(LocalError::InvalidResult)?
        .into_iter()
        .all(|point| {
            let offset = point - rim.centre;
            let reach = (offset - rim.axis * offset.dot(rim.axis)).length();
            (reach - outer) * rim.along_plane > tolerance
        });
    let clear_cylinder = other_boundary_points(body, rim.cylinder_face, &[rim.edge, rim.seam])
        .ok_or(LocalError::InvalidResult)?
        .into_iter()
        .all(|point| {
            (point - rim.centre).dot(rim.axis) * rim.along_cylinder > along_cylinder + tolerance
        });
    if !clear_plane || !clear_cylinder {
        return Err(LocalError::TooLarge);
    }

    let corner = Vec3::from(body.vertices.get(rim.vertex).ok_or(LocalError::InvalidResult)?.point);
    let radial = (corner - rim.centre).normalize().ok_or(LocalError::InvalidResult)?;
    let near = rim.centre + rim.axis * drop;
    let far_point = rim.centre + radial * outer;
    let near_point = corner + rim.axis * drop;
    let mut near_frame = rim.circle.plane;
    near_frame.origin = near.to_array();

    let (surface, band_curve, band_range) = if round {
        let tube = along_plane;
        let centre = rim.centre + radial * outer + rim.axis * drop;
        let torus = Torus { frame: near_frame, major_radius: outer, minor_radius: tube };
        // Measured from the radial direction toward the side that turns the
        // plane's tangent point into the cylinder's in a quarter turn.
        let side = -rim.along_plane * rim.along_cylinder;
        let meridian = Plane::orthonormal(
            centre.to_array(),
            radial.to_array(),
            radial.cross(rim.axis * side).to_array(),
        )
        .ok_or(LocalError::InvalidResult)?;
        let start = rim.along_plane * FRAC_PI_2;
        (
            Surface::Torus(torus),
            Curve3::Circle(Circle3 { plane: meridian, radius: tube }),
            (start, start + FRAC_PI_2),
        )
    } else {
        let cone = Cone {
            base: rim.circle.plane,
            radius: outer,
            half_angle: ((outer - radius) / drop).atan(),
        };
        (
            Surface::Cone(cone),
            Curve3::Line(Line3 {
                origin: far_point.to_array(),
                direction: (near_point - far_point).to_array(),
            }),
            (0.0, 1.0),
        )
    };

    // The band's outward normal leaves the material: toward the old corner
    // on a convex rim, away from it on a concave one.
    let middle = Vec3::from(band_curve.point_at((band_range.0 + band_range.1) * 0.5));
    let (u, v) = surface.parameters_at(middle.to_array()).ok_or(LocalError::InvalidResult)?;
    let normal = Vec3::from(surface.normal_at(u, v).ok_or(LocalError::InvalidResult)?);
    let outward = (corner - middle) * if rim.convex { 1.0 } else { -1.0 };
    let forward = normal.dot(outward) > 0.0;

    let edge = body.edges.get(rim.edge).ok_or(LocalError::InvalidResult)?.clone();
    let cylinder_coedge =
        body.coedges.get(rim.cylinder_coedge).ok_or(LocalError::InvalidResult)?.clone();
    let cylinder_loop = cylinder_coedge.owner;
    let shell = body.faces.get(rim.cylinder_face).ok_or(LocalError::InvalidResult)?.owner;

    // The plane keeps the rim edge, now on the band's far circle.
    let vertex = body.vertices.get_mut(rim.vertex).ok_or(LocalError::InvalidResult)?;
    vertex.point = far_point.to_array();
    let far_curve = body.curves.insert(Curve3::Circle(Circle3 {
        plane: rim.circle.plane,
        radius: outer,
    }));
    body.edges.get_mut(rim.edge).ok_or(LocalError::InvalidResult)?.curve = far_curve;

    // The cylinder gets a new near circle and a shorter seam.
    let near_vertex = body.vertices.insert(Vertex {
        point: near_point.to_array(),
        provenance: Provenance::Synthesized,
    });
    let near_curve = body.curves.insert(Curve3::Circle(Circle3 { plane: near_frame, radius }));
    let near_edge = body.edges.insert(Edge {
        curve: near_curve,
        start_parameter: edge.start_parameter,
        end_parameter: edge.end_parameter,
        start: near_vertex,
        end: near_vertex,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let cylinder_near = body.coedges.insert(Coedge {
        edge: near_edge,
        forward: cylinder_coedge.forward,
        pcurve: None,
        owner: cylinder_loop,
        provenance: Provenance::Synthesized,
    });
    let ring = body.loops.get_mut(cylinder_loop).ok_or(LocalError::InvalidResult)?;
    let slot = ring
        .coedges
        .iter()
        .position(|coedge| *coedge == rim.cylinder_coedge)
        .ok_or(LocalError::InvalidResult)?;
    ring.coedges[slot] = cylinder_near;
    {
        let seam_curve = body.edges.get(rim.seam).ok_or(LocalError::InvalidResult)?.curve;
        let Some(&Curve3::Line(line)) = body.curves.get(seam_curve) else {
            return Err(LocalError::InvalidResult);
        };
        let direction = Vec3::from(line.direction);
        let parameter =
            (near_point - Vec3::from(line.origin)).dot(direction) / direction.length_squared();
        let seam = body.edges.get_mut(rim.seam).ok_or(LocalError::InvalidResult)?;
        if seam.start == rim.vertex {
            seam.start = near_vertex;
            seam.start_parameter = parameter;
        } else {
            seam.end = near_vertex;
            seam.end_parameter = parameter;
        }
    }

    // The band: rim edge, its seam down, the near circle back, seam up.
    let surface = body.surfaces.insert(surface);
    let face = body.faces.insert(Face {
        surface,
        forward,
        loops: Vec::new(),
        owner: shell,
        provenance: Provenance::Synthesized,
    });
    let band_loop = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let band_curve = body.curves.insert(band_curve);
    let band_seam = body.edges.insert(Edge {
        curve: band_curve,
        start_parameter: band_range.0,
        end_parameter: band_range.1,
        start: rim.vertex,
        end: near_vertex,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let mut coedge = |edge: EdgeKey, forward: bool| {
        body.coedges.insert(Coedge {
            edge,
            forward,
            pcurve: None,
            owner: band_loop,
            provenance: Provenance::Synthesized,
        })
    };
    let down = coedge(band_seam, true);
    let back = coedge(near_edge, !cylinder_coedge.forward);
    let up = coedge(band_seam, false);
    body.coedges.get_mut(rim.cylinder_coedge).ok_or(LocalError::InvalidResult)?.owner = band_loop;
    body.loops.get_mut(band_loop).ok_or(LocalError::InvalidResult)?.coedges =
        vec![rim.cylinder_coedge, down, back, up];
    body.faces.get_mut(face).ok_or(LocalError::InvalidResult)?.loops.push(band_loop);
    body.edges.get_mut(near_edge).ok_or(LocalError::InvalidResult)?.coedges =
        vec![cylinder_near, back];
    body.edges.get_mut(band_seam).ok_or(LocalError::InvalidResult)?.coedges = vec![down, up];
    body.shells.get_mut(shell).ok_or(LocalError::InvalidResult)?.faces.push(face);
    Ok(())
}

/// `None` unless every selected edge is such a line and no two share a
/// corner, leaving the selection to the caller's own error.
pub(super) fn blend_lines(
    body: &Body,
    selected: &[EdgeKey],
    blend: LocalBlend,
) -> Option<Result<Body, LocalError>> {
    let mut corners = Vec::new();
    for edge in selected {
        corners.extend(line(body, *edge)?.ends.map(|end| end.vertex));
    }
    let count = corners.len();
    corners.sort_by_key(VertexKey::slot);
    corners.dedup();
    if corners.len() != count {
        return None;
    }
    let tolerance = super::operation_tolerance(&[body]);
    let mut result = body.clone();
    for edge in selected {
        // An earlier cut may have trimmed this line's faces.
        let Some(line) = line(&result, *edge) else {
            return Some(Err(LocalError::TooLarge));
        };
        let Some(setbacks) = blend.distances(line.faces[0], line.faces[1]) else {
            return Some(Err(LocalError::OutsideBaseFace(*edge)));
        };
        let round = matches!(blend, LocalBlend::Fillet(_));
        if let Err(error) = cut_line(&mut result, &line, setbacks, round, tolerance) {
            return Some(Err(error));
        }
    }
    if !result.validate().is_empty() || result.worst_vertex_gap() > tolerance {
        return Some(Err(LocalError::InvalidResult));
    }
    Some(Ok(result))
}

struct Line {
    edge: EdgeKey,
    coedges: [CoedgeKey; 2],
    faces: [FaceKey; 2],
    /// Into each face, square to the edge.
    into: [Vec3; 2],
    start: Vec3,
    direction: Vec3,
    length: f64,
    convex: bool,
    ends: [Corner; 2],
}

/// A three-edge corner at one end of a line.
#[derive(Clone, Copy)]
struct Corner {
    vertex: VertexKey,
    /// The other edge of each line face at this corner.
    sides: [EdgeKey; 2],
    face: FaceKey,
    plane: Plane,
}

fn face_of(body: &Body, coedge: CoedgeKey) -> Option<FaceKey> {
    Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner)
}

fn line(body: &Body, key: EdgeKey) -> Option<Line> {
    let edge = body.edges.get(key)?;
    if !matches!(body.curves.get(edge.curve)?, Curve3::Line(_)) || edge.coedges.len() != 2 {
        return None;
    }
    let coedges = [edge.coedges[0], edge.coedges[1]];
    let faces = [face_of(body, coedges[0])?, face_of(body, coedges[1])?];
    let start = Vec3::from(body.vertices.get(edge.start)?.point);
    let end = Vec3::from(body.vertices.get(edge.end)?.point);
    let direction = (end - start).normalize()?;
    let mut outward = [Vec3::from([0.0; 3]); 2];
    let mut into = outward;
    for side in 0..2 {
        let face = body.faces.get(faces[side])?;
        let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
            return None;
        };
        outward[side] = Vec3::from(plane.normal()?) * if face.forward { 1.0 } else { -1.0 };
        let along = if body.coedges.get(coedges[side])?.forward { direction } else { -direction };
        // A loop keeps its face on the left, looking down the outward normal.
        into[side] = outward[side].cross(along).normalize()?;
    }
    if into[0].dot(into[1]).abs() > 1.0 - 1e-9 {
        return None;
    }
    let corner = |vertex: VertexKey| -> Option<Corner> {
        let mut sides = [None, None];
        let mut others = [None, None];
        for (other, candidate) in body.edges.iter() {
            if other == key || (candidate.start != vertex && candidate.end != vertex) {
                continue;
            }
            if !matches!(body.curves.get(candidate.curve)?, Curve3::Line(_))
                || candidate.coedges.len() != 2
            {
                return None;
            }
            let owners =
                [face_of(body, candidate.coedges[0])?, face_of(body, candidate.coedges[1])?];
            let side = faces.iter().position(|face| owners.contains(face))?;
            if sides[side].replace(other).is_some() {
                return None;
            }
            others[side] = owners.into_iter().find(|owner| *owner != faces[side]);
        }
        let face = others[0]?;
        if others[1]? != face || faces.contains(&face) {
            return None;
        }
        let Surface::Plane(plane) = body.surfaces.get(body.faces.get(face)?.surface)? else {
            return None;
        };
        Some(Corner { vertex, sides: [sides[0]?, sides[1]?], face, plane: *plane })
    };
    Some(Line {
        edge: key,
        coedges,
        faces,
        into,
        start,
        direction,
        length: end.distance(start),
        convex: into[1].dot(outward[0]) < 0.0,
        ends: [corner(edge.start)?, corner(edge.end)?],
    })
}

fn cut_line(
    body: &mut Body,
    line: &Line,
    setbacks: (f64, f64),
    round: bool,
    tolerance: f64,
) -> Result<(), LocalError> {
    let invalid = LocalError::InvalidResult;
    let half = line.into[0].dot(line.into[1]).clamp(-1.0, 1.0).acos() * 0.5;
    let setbacks = if round {
        [setbacks.0 / half.tan(), setbacks.1 / half.tan()]
    } else {
        [setbacks.0, setbacks.1]
    };
    // Nothing else on either face may come within the setback.
    for (side, setback) in setbacks.iter().enumerate() {
        let skip = [line.edge, line.ends[0].sides[side], line.ends[1].sides[side]];
        let crowded = other_boundary_points(body, line.faces[side], &skip)
            .ok_or(invalid)?
            .into_iter()
            .any(|point| {
                let offset = point - line.start;
                let along = offset.dot(line.direction);
                along > -tolerance
                    && along < line.length + tolerance
                    && offset.dot(line.into[side]) <= setback + tolerance
            });
        if crowded {
            return Err(LocalError::TooLarge);
        }
    }

    // Where each tangent line leaves the band, on the corner's side edges.
    let mut points = [[Vec3::from([0.0; 3]); 2]; 2];
    for (end, corner) in line.ends.iter().enumerate() {
        let normal = Vec3::from(corner.plane.normal().ok_or(invalid)?);
        let rate = normal.dot(line.direction);
        if rate.abs() < 1e-8 {
            return Err(invalid);
        }
        for side in 0..2 {
            let base = line.start + line.into[side] * setbacks[side];
            let along = normal.dot(Vec3::from(corner.plane.origin) - base) / rate;
            let point = base + line.direction * along;
            let edge = body.edges.get(corner.sides[side]).ok_or(invalid)?;
            let far = if edge.start == corner.vertex { edge.end } else { edge.start };
            let corner_point = Vec3::from(body.vertices.get(corner.vertex).ok_or(invalid)?.point);
            let far_point = Vec3::from(body.vertices.get(far).ok_or(invalid)?.point);
            let span = far_point - corner_point;
            let reach = (point - corner_point).dot(span) / span.length();
            if reach <= tolerance || reach >= span.length() - tolerance {
                return Err(LocalError::TooLarge);
            }
            points[end][side] = point;
        }
    }

    let outward = (line.into[0] + line.into[1]).normalize().ok_or(invalid)?
        * if line.convex { -1.0 } else { 1.0 };
    let (surface, forward) = if round {
        let radius = setbacks[0] * half.tan();
        let axis = line.start
            + (line.into[0] + line.into[1]).normalize().ok_or(invalid)? * (radius / half.sin());
        let base = Plane::orthonormal(
            axis.to_array(),
            (-(line.into[0] + line.into[1])).to_array(),
            line.direction.to_array(),
        )
        .ok_or(invalid)?;
        (Surface::Cylinder(Cylinder { base, radius }), line.convex)
    } else {
        let mut normal =
            (points[0][1] - points[0][0]).cross(line.direction).normalize().ok_or(invalid)?;
        if normal.dot(outward) < 0.0 {
            normal = -normal;
        }
        let plane = Plane::orthonormal(
            points[0][0].to_array(),
            line.direction.to_array(),
            normal.to_array(),
        )
        .ok_or(invalid)?;
        (Surface::Plane(plane), true)
    };

    let corners = line.ends.map(|corner| corner.vertex);
    let mut vertices = [[corners[0]; 2]; 2];
    for end in 0..2 {
        for side in 0..2 {
            let vertex = body.vertices.insert(Vertex {
                point: points[end][side].to_array(),
                provenance: Provenance::Synthesized,
            });
            vertices[end][side] = vertex;
            let key = line.ends[end].sides[side];
            let Some(&Curve3::Line(curve)) =
                body.curves.get(body.edges.get(key).ok_or(invalid)?.curve)
            else {
                return Err(invalid);
            };
            let direction = Vec3::from(curve.direction);
            let parameter = (points[end][side] - Vec3::from(curve.origin)).dot(direction)
                / direction.length_squared();
            let edge = body.edges.get_mut(key).ok_or(invalid)?;
            if edge.start == corners[end] {
                edge.start = vertex;
                edge.start_parameter = parameter;
            } else {
                edge.end = vertex;
                edge.end_parameter = parameter;
            }
        }
    }
    let straight = |from: Vec3, to: Vec3| {
        Curve3::Line(Line3 { origin: from.to_array(), direction: (to - from).to_array() })
    };

    // The first face keeps the edge, now on its tangent line.
    let first_curve = body.curves.insert(straight(points[0][0], points[1][0]));
    {
        let edge = body.edges.get_mut(line.edge).ok_or(invalid)?;
        edge.curve = first_curve;
        edge.start_parameter = 0.0;
        edge.end_parameter = 1.0;
        edge.start = vertices[0][0];
        edge.end = vertices[1][0];
    }
    for corner in corners {
        body.vertices.remove(corner);
    }

    // The second face gets a new edge on its own tangent line.
    let moved = body.coedges.get(line.coedges[1]).ok_or(invalid)?.clone();
    let second_curve = body.curves.insert(straight(points[0][1], points[1][1]));
    let second = body.edges.insert(Edge {
        curve: second_curve,
        start_parameter: 0.0,
        end_parameter: 1.0,
        start: vertices[0][1],
        end: vertices[1][1],
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let second_use = body.coedges.insert(Coedge {
        edge: second,
        forward: moved.forward,
        pcurve: None,
        owner: moved.owner,
        provenance: Provenance::Synthesized,
    });
    let ring = body.loops.get_mut(moved.owner).ok_or(invalid)?;
    let slot = ring.coedges.iter().position(|key| *key == line.coedges[1]).ok_or(invalid)?;
    ring.coedges[slot] = second_use;

    // Each corner face gains the band's section between its side edges.
    let mut sections = [line.edge; 2];
    let mut corner_uses = [line.coedges[0]; 2];
    for (end, corner) in line.ends.iter().enumerate() {
        let curve = if let Surface::Cylinder(cylinder) = &surface {
            super::blend::cylinder_section(cylinder, &corner.plane).ok_or(invalid)?
        } else {
            straight(points[end][0], points[end][1])
        };
        let placed = body.curves.insert(curve.clone());
        let section = body.edges.insert(Edge {
            curve: placed,
            start_parameter: 0.0,
            end_parameter: 1.0,
            start: vertices[end][0],
            end: vertices[end][1],
            coedges: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        if round {
            super::blend::set_round_edge(body, section, curve, tolerance).ok_or(invalid)?;
        }
        let ring_key = body
            .faces
            .get(corner.face)
            .ok_or(invalid)?
            .loops
            .iter()
            .copied()
            .find(|ring| {
                body.loops.get(*ring).is_some_and(|ring| {
                    ring.coedges.iter().any(|coedge| {
                        body.coedges
                            .get(*coedge)
                            .is_some_and(|coedge| coedge.edge == corner.sides[0])
                    })
                })
            })
            .ok_or(invalid)?;
        let ring = body.loops.get(ring_key).ok_or(invalid)?.coedges.clone();
        let edge_of = |coedge: CoedgeKey| body.coedges.get(coedge).map(|coedge| coedge.edge);
        let slot = (0..ring.len())
            .find(|slot| {
                let pair = [edge_of(ring[*slot]), edge_of(ring[(slot + 1) % ring.len()])];
                pair.contains(&Some(corner.sides[0])) && pair.contains(&Some(corner.sides[1]))
            })
            .ok_or(invalid)?;
        let use_ = body.coedges.insert(Coedge {
            edge: section,
            forward: edge_of(ring[slot]) == Some(corner.sides[0]),
            pcurve: None,
            owner: ring_key,
            provenance: Provenance::Synthesized,
        });
        body.loops.get_mut(ring_key).ok_or(invalid)?.coedges.insert(slot + 1, use_);
        sections[end] = section;
        corner_uses[end] = use_;
    }

    // The band: the first tangent line, a section, the second tangent line
    // back, the other section.
    let shell = body.faces.get(line.faces[0]).ok_or(invalid)?.owner;
    let surface = body.surfaces.insert(surface);
    let face = body.faces.insert(Face {
        surface,
        forward,
        loops: Vec::new(),
        owner: shell,
        provenance: Provenance::Synthesized,
    });
    let band = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    // The moved coedge runs the first tangent line toward `first`'s corner.
    let [first, last] = if moved.forward { [1, 0] } else { [0, 1] };
    let mut band_use = |edge: EdgeKey, forward: bool| {
        body.coedges.insert(Coedge {
            edge,
            forward,
            pcurve: None,
            owner: band,
            provenance: Provenance::Synthesized,
        })
    };
    let opening = band_use(sections[first], true);
    let across = band_use(second, !moved.forward);
    let closing = band_use(sections[last], false);
    // Each section's corner face must run it the other way.
    if body.coedges.get(corner_uses[first]).ok_or(invalid)?.forward
        || !body.coedges.get(corner_uses[last]).ok_or(invalid)?.forward
    {
        return Err(invalid);
    }
    body.coedges.get_mut(line.coedges[1]).ok_or(invalid)?.owner = band;
    body.loops.get_mut(band).ok_or(invalid)?.coedges =
        vec![line.coedges[1], opening, across, closing];
    body.faces.get_mut(face).ok_or(invalid)?.loops.push(band);
    body.edges.get_mut(second).ok_or(invalid)?.coedges = vec![second_use, across];
    body.edges.get_mut(sections[first]).ok_or(invalid)?.coedges =
        vec![corner_uses[first], opening];
    body.edges.get_mut(sections[last]).ok_or(invalid)?.coedges = vec![corner_uses[last], closing];
    body.shells.get_mut(shell).ok_or(invalid)?.faces.push(face);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{chamfer_edges, combine, fillet_edges, make, operation_tolerance, Operation};
    use crate::{Body, Curve3, EdgeKey, Surface};

    fn drilled() -> (Body, EdgeKey) {
        let block = make::cuboid([0.0; 3], [10.0, 6.0, 4.0]).unwrap();
        let drill = make::cylinder([5.0, 3.0, -1.0], 1.0, 6.0).unwrap();
        let tolerance = operation_tolerance(&[&block, &drill]);
        let body = combine(block, drill, Operation::Difference, tolerance).unwrap();
        let rim = body
            .edges
            .iter()
            .find_map(|(key, edge)| match body.curves.get(edge.curve) {
                Some(Curve3::Circle(circle)) if circle.plane.origin[2] > 3.0 => Some(key),
                _ => None,
            })
            .unwrap();
        (body, rim)
    }

    fn has(body: &Body, kind: fn(&Surface) -> bool) -> bool {
        body.faces
            .iter()
            .any(|(_, face)| body.surfaces.get(face.surface).is_some_and(kind))
    }

    #[test]
    fn edges_of_a_stepped_block_are_rounded_and_chamfered() {
        let base = make::cuboid([0.0; 3], [10.0, 6.0, 2.0]).unwrap();
        let step = make::cuboid([0.0, 0.0, 2.0], [3.0, 6.0, 4.0]).unwrap();
        let tolerance = operation_tolerance(&[&base, &step]);
        let body = combine(base, step, Operation::Union, tolerance).unwrap();
        assert_eq!(body.faces.len(), 8);
        let edge_at = |x: f64, z: f64| {
            body.edges
                .iter()
                .find_map(|(key, _)| {
                    let (start, end) = body.edge_endpoints(key)?;
                    let at = |point: [f64; 3]| {
                        (point[0] - x).abs() < 1e-9 && (point[2] - z).abs() < 1e-9
                    };
                    (at(start) && at(end)).then_some(key)
                })
                .unwrap()
        };
        let (inside, outside) = (edge_at(3.0, 2.0), edge_at(10.0, 2.0));
        let upper = body
            .face_keys()
            .find(|face| {
                crate::planar_face_profile(&body, *face).is_some_and(|profile| {
                    profile.outward[2] > 0.5 && (profile.plane.origin[2] - 2.0).abs() < 1e-9
                })
            })
            .unwrap();

        for result in [
            fillet_edges(&body, &[inside], 0.5).unwrap(),
            fillet_edges(&body, &[outside], 0.5).unwrap(),
            fillet_edges(&body, &[inside, outside], 0.5).unwrap(),
            chamfer_edges(&body, &[inside], upper, 0.5, 0.5).unwrap(),
        ] {
            assert!(result.validate().is_empty());
            assert!(result.faces.len() > body.faces.len());
        }
    }

    #[test]
    fn hole_rims_are_rounded_and_chamfered() {
        let (body, rim) = drilled();
        let rounded = fillet_edges(&body, &[rim], 0.5).unwrap();
        let top = body
            .face_keys()
            .find(|face| {
                crate::planar_face_profile(&body, *face)
                    .is_some_and(|profile| profile.outward[2] > 0.5)
            })
            .unwrap();
        let chamfered = chamfer_edges(&body, &[rim], top, 0.5, 0.25).unwrap();

        assert!(rounded.validate().is_empty());
        assert!(chamfered.validate().is_empty());
        assert_eq!(rounded.faces.len(), body.faces.len() + 1);
        assert!(has(&rounded, |surface| matches!(surface, Surface::Torus(_))));
        assert!(has(&chamfered, |surface| matches!(surface, Surface::Cone(_))));
        // The hole wall is 2 from the block side: a round that wide runs off the top over the
        // side, which is refused, or (Solidcore's overflow) a valid solid.
        if let Ok(over) = fillet_edges(&body, &[rim], 2.5) {
            assert!(over.validate().is_empty());
            let block = crate::mass_properties(&body).unwrap().volume;
            assert!(crate::mass_properties(&over).unwrap().volume < block);
        }
    }
}
