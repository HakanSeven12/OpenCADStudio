//! Turning an open sheet body into a closed solid of constant thickness.
//!
//! Analytic faces retain their exact geometry. Free-form rectangular patches
//! are refined and offset as tolerance-bounded NURBS surfaces, with the source
//! surface retained exactly and every boundary represented as B-rep geometry.

use super::geometry::{Curve3, Line3, Surface};
use super::nurbs_builder::RationalCurve3;
use super::topology::{
    Body, Coedge, Edge, EdgeKey, Face, FaceKey, Loop, Lump, Shell, ShellKey, Vertex, VertexKey,
};
use super::Provenance;
use ocs_geom::geom2d::{Arc, Curve, Line};
use ocs_geom::space::{NurbsSurface3, Plane, Vec3};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Why a surface could not be thickened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThickenError {
    /// The requested distance was zero or not finite.
    InvalidDistance,
    /// The input has no bounded sheet geometry that can become a solid.
    UnsupportedSurface,
    /// The offset collapses or crosses the source surface.
    SelfIntersection,
}

/// Thickens every bounded face of an open sheet by a signed distance.
///
/// Positive distance follows each face's oriented normal. The source body is
/// borrowed and never changed, including when an offset fails.
pub fn thicken(body: &Body, distance: f64) -> Result<Body, ThickenError> {
    if !distance.is_finite() || distance.abs() <= f64::EPSILON {
        return Err(ThickenError::InvalidDistance);
    }
    if !body.validate().is_empty() || body.faces.keys().next().is_none() {
        return Err(ThickenError::UnsupportedSurface);
    }
    // Solidcore's thicken first; this kernel's own for what it refuses.
    if let Ok(solid) = super::bridge::thicken(body, distance) {
        return Ok(solid);
    }

    if !body.edges.iter().any(|(_, edge)| edge.coedges.len() == 1) {
        return Err(ThickenError::UnsupportedSurface);
    }
    let faces = body.face_keys().collect::<Vec<_>>();
    if faces.len() == 1 {
        return thicken_face(body, faces[0], distance);
    }

    let tolerance = super::operation_tolerance(&[body]);
    let mut solids = faces
        .into_iter()
        .map(|face| {
            single_face(body, face).and_then(|sheet| {
                thicken_face(
                    &sheet,
                    sheet
                        .face_keys()
                        .next()
                        .ok_or(ThickenError::UnsupportedSurface)?,
                    distance,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (_, edge) in body.edges.iter() {
        if let Some(connector) = straight_edge_connector(body, edge, distance)? {
            solids.push(connector);
        }
    }

    loop {
        let mut joined = None;
        'pairs: for first in 0..solids.len() {
            for second in first + 1..solids.len() {
                if let Some(body) =
                    join_on_full_planar_face(&solids[first], &solids[second], tolerance)?
                {
                    joined = Some((first, second, body));
                    break 'pairs;
                }
            }
        }
        let Some((first, second, body)) = joined else {
            break;
        };
        solids.swap_remove(second);
        solids.swap_remove(first);
        solids.push(body);
    }

    let mut solids = solids.into_iter();
    let mut result = solids.next().ok_or(ThickenError::UnsupportedSurface)?;
    for solid in solids {
        result = super::boolean::combine(result, solid, super::Operation::Union, tolerance)
            .map_err(|_| ThickenError::SelfIntersection)?;
    }
    (!result.roots.is_empty()
        && result.validate().is_empty()
        && result.edges.iter().all(|(_, edge)| edge.coedges.len() == 2))
    .then_some(result)
    .ok_or(ThickenError::SelfIntersection)
}

fn join_on_full_planar_face(
    first: &Body,
    second: &Body,
    tolerance: f64,
) -> Result<Option<Body>, ThickenError> {
    let shared = first.face_keys().find_map(|one| {
        let first_face = first.faces.get(one)?;
        let Surface::Plane(first_plane) = first.surfaces.get(first_face.surface)? else {
            return None;
        };
        second.face_keys().find_map(|two| {
            let second_face = second.faces.get(two)?;
            let Surface::Plane(second_plane) = second.surfaces.get(second_face.surface)? else {
                return None;
            };
            if !matches!(
                super::intersect_surfaces(
                    &Surface::Plane(*first_plane),
                    &Surface::Plane(*second_plane),
                    tolerance,
                ),
                super::Meeting::Coincident
            ) || !super::imprint::same_ground(first, one, second, two, tolerance)
            {
                return None;
            }
            let first_normal =
                Vec3::from(first_plane.normal()?) * if first_face.forward { 1.0 } else { -1.0 };
            let second_normal =
                Vec3::from(second_plane.normal()?) * if second_face.forward { 1.0 } else { -1.0 };
            (first_normal.dot(second_normal) < -1.0 + 1e-7).then_some((one, two))
        })
    });
    let Some((skip_first, skip_second)) = shared else {
        return Ok(None);
    };

    let mut result = Body::new();
    let lump = result.lumps.insert(Lump {
        shells: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let shell = result.shells.insert(Shell {
        faces: Vec::new(),
        owner: lump,
        provenance: Provenance::Synthesized,
    });
    result.lumps.get_mut(lump).unwrap().shells.push(shell);
    result.roots.push(lump);
    for face in first.face_keys().filter(|face| *face != skip_first) {
        super::boolean::copy_face(&mut result, first, face, shell, false)
            .map_err(|_| ThickenError::SelfIntersection)?;
    }
    for face in second.face_keys().filter(|face| *face != skip_second) {
        super::boolean::copy_face(&mut result, second, face, shell, false)
            .map_err(|_| ThickenError::SelfIntersection)?;
    }
    super::boolean::orient_shell(&mut result).map_err(|_| ThickenError::SelfIntersection)?;
    if !result.validate().is_empty()
        || result.edges.iter().any(|(_, edge)| edge.coedges.len() != 2)
        || result.worst_vertex_gap() > tolerance
    {
        return Err(ThickenError::SelfIntersection);
    }
    Ok(Some(result))
}

fn thicken_face(body: &Body, face_key: FaceKey, distance: f64) -> Result<Body, ThickenError> {
    let face = body
        .faces
        .get(face_key)
        .ok_or(ThickenError::UnsupportedSurface)?;
    let surface = body
        .surfaces
        .get(face.surface)
        .ok_or(ThickenError::UnsupportedSurface)?;
    let oriented = distance * if face.forward { 1.0 } else { -1.0 };

    if matches!(surface, Surface::Plane(_)) {
        let profile =
            super::planar_face_profile(body, face_key).ok_or(ThickenError::UnsupportedSurface)?;
        return super::extrude_region(
            profile.plane,
            &profile.loops,
            (Vec3::from(profile.outward) * distance).to_array(),
        )
        .ok_or(ThickenError::SelfIntersection);
    }

    let patch =
        rectangular_patch(body, face_key, surface).ok_or(ThickenError::UnsupportedSurface)?;
    match surface {
        Surface::Nurbs(nurbs) | Surface::Offset(crate::OffsetSurface { approximation: nurbs, .. }) => {
            thicken_nurbs_patch(body, face_key, nurbs, oriented)
        }
        _ => thicken_revolved_patch(surface, patch, oriented),
    }
}

fn single_face(body: &Body, face: FaceKey) -> Result<Body, ThickenError> {
    let mut result = Body::new();
    let lump = result.lumps.insert(Lump {
        shells: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let shell = result.shells.insert(Shell {
        faces: Vec::new(),
        owner: lump,
        provenance: Provenance::Synthesized,
    });
    result.lumps.get_mut(lump).unwrap().shells.push(shell);
    result.roots.push(lump);
    super::boolean::copy_face(&mut result, body, face, shell, false)
        .map_err(|_| ThickenError::UnsupportedSurface)?;
    result
        .validate()
        .is_empty()
        .then_some(result)
        .ok_or(ThickenError::UnsupportedSurface)
}

fn straight_edge_connector(
    body: &Body,
    edge: &Edge,
    distance: f64,
) -> Result<Option<Body>, ThickenError> {
    let [first, second] = edge.coedges.as_slice() else {
        return Ok(None);
    };
    let face_of = |key| {
        body.coedges
            .get(key)
            .and_then(|coedge| body.loops.get(coedge.owner))
            .and_then(|ring| body.faces.get(ring.owner).map(|face| (ring.owner, face)))
    };
    let Some((first_key, first_face)) = face_of(*first) else {
        return Err(ThickenError::UnsupportedSurface);
    };
    let Some((second_key, second_face)) = face_of(*second) else {
        return Err(ThickenError::UnsupportedSurface);
    };
    if first_key == second_key {
        return Ok(None);
    }
    let constant_offset = |coedge_key, face: &Face| {
        let coedge = body.coedges.get(coedge_key)?;
        let surface = body.surfaces.get(face.surface)?;
        let edge = body.edges.get(coedge.edge)?;
        let curve = body.curves.get(edge.curve)?;
        let normal_at = |parameter: f64| {
            let uv = match &coedge.pcurve {
                Some(pcurve) => pcurve.point_at(parameter),
                None => surface
                    .parameters_at(curve.point_at(
                        edge.start_parameter
                            + (edge.end_parameter - edge.start_parameter) * parameter,
                    ))?
                    .into(),
            };
            Some(
                Vec3::from(surface.normal_at(uv[0], uv[1])?)
                    * if face.forward { 1.0 } else { -1.0 },
            )
        };
        let middle = normal_at(0.5)?;
        [0.0, 0.25, 0.75, 1.0]
            .into_iter()
            .all(|parameter| {
                normal_at(parameter).is_some_and(|normal| normal.dot(middle) >= 1.0 - 1e-7)
            })
            .then_some(middle * distance)
    };
    let Some(first_offset) = constant_offset(*first, first_face) else {
        return Ok(None);
    };
    let Some(second_offset) = constant_offset(*second, second_face) else {
        return Ok(None);
    };
    if first_offset.cross(second_offset).length() <= distance.abs().powi(2) * 1e-10 {
        return Ok(None);
    }
    let cosine = first_offset.dot(second_offset) / distance.powi(2);
    let denominator = 1.0 + cosine;
    if denominator <= 1e-8 {
        return Err(ThickenError::SelfIntersection);
    }
    let miter_offset = (first_offset + second_offset) / denominator;
    let start = Vec3::from(
        body.vertices
            .get(edge.start)
            .ok_or(ThickenError::UnsupportedSurface)?
            .point,
    );
    let end = Vec3::from(
        body.vertices
            .get(edge.end)
            .ok_or(ThickenError::UnsupportedSurface)?
            .point,
    );
    let along = end - start;
    let curve = body
        .curves
        .get(edge.curve)
        .ok_or(ThickenError::UnsupportedSurface)?;
    let middle = Vec3::from(curve.point_at(0.5 * (edge.start_parameter + edge.end_parameter)));
    if middle.distance((start + end) * 0.5) > super::operation_tolerance(&[body]) {
        return Ok(None);
    }
    let plane = Plane::orthonormal(start.to_array(), first_offset.to_array(), along.to_array())
        .ok_or(ThickenError::UnsupportedSurface)?;
    let corners = [
        start,
        start + first_offset,
        start + miter_offset,
        start + second_offset,
    ];
    let points = corners
        .map(|point| plane.project(point.to_array()))
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or(ThickenError::UnsupportedSurface)?;
    let profile = (0..4)
        .map(|index| {
            Curve::Line(Line {
                start: points[index],
                end: points[(index + 1) % 4],
            })
        })
        .collect::<Vec<_>>();
    Ok(super::extrude(plane, &profile, along.to_array()))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Patch {
    pub(crate) u_start: f64,
    pub(crate) u_sweep: f64,
    pub(crate) v_start: f64,
    pub(crate) v_sweep: f64,
}

pub(crate) fn rectangular_patch(
    body: &Body,
    face_key: FaceKey,
    surface: &Surface,
) -> Option<Patch> {
    if body.faces.get(face_key)?.loops.len() != 1 {
        return None;
    }
    let boundary = super::pcurve::face_boundary(body, face_key, 1e-8)?;
    let [Curve::Line(a), Curve::Line(b), Curve::Line(c), Curve::Line(d)] = boundary.as_slice()
    else {
        return None;
    };
    let lines = [a, b, c, d];
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-8;
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for (index, line) in lines.iter().enumerate() {
        if !near(line.end, lines[(index + 1) % 4].start) {
            return None;
        }
        let delta = [line.end[0] - line.start[0], line.end[1] - line.start[1]];
        if (delta[0].abs() <= 1e-8) == (delta[1].abs() <= 1e-8) {
            return None;
        }
        for axis in 0..2 {
            if !line.start[axis].is_finite() {
                return None;
            }
            low[axis] = low[axis].min(line.start[axis]);
            high[axis] = high[axis].max(line.start[axis]);
        }
    }
    let sweep = [high[0] - low[0], high[1] - low[1]];
    // Four distinct corners are required; a retraced boundary encloses nothing.
    for i in 0..4 {
        for j in i + 1..4 {
            if near(lines[i].start, lines[j].start) {
                return None;
            }
        }
    }
    if sweep[0] > TAU + 1e-8 || (matches!(surface, Surface::Torus(_)) && sweep[1] > TAU + 1e-8) {
        return None;
    }
    Some(Patch {
        u_start: low[0],
        u_sweep: sweep[0],
        v_start: low[1],
        v_sweep: sweep[1],
    })
}

fn thicken_revolved_patch(
    surface: &Surface,
    patch: Patch,
    distance: f64,
) -> Result<Body, ThickenError> {
    let (frame, profile) = match surface {
        Surface::Cylinder(cylinder) => {
            let second = cylinder.radius + distance;
            positive_radii(&[cylinder.radius, second])?;
            let v0 = patch.v_start;
            let v1 = patch.v_start + patch.v_sweep;
            (
                cylinder.base,
                line_ring(&[
                    [cylinder.radius, v0],
                    [cylinder.radius, v1],
                    [second, v1],
                    [second, v0],
                ]),
            )
        }
        Surface::Cone(cone) => {
            let sine = cone.half_angle.sin();
            let cosine = cone.half_angle.cos();
            let tangent = cone.half_angle.tan();
            let v0 = patch.v_start;
            let v1 = patch.v_start + patch.v_sweep;
            let source0 = cone.radius - v0 * tangent;
            let source1 = cone.radius - v1 * tangent;
            let offset0 = source0 + distance * cosine;
            let offset1 = source1 + distance * cosine;
            positive_radii(&[source0, source1, offset0, offset1])?;
            (
                cone.base,
                line_ring(&[
                    [source0, v0],
                    [source1, v1],
                    [offset1, v1 + distance * sine],
                    [offset0, v0 + distance * sine],
                ]),
            )
        }
        Surface::Sphere(sphere) => {
            let second = sphere.radius + distance;
            positive_radii(&[sphere.radius, second])?;
            let v0 = patch.v_start;
            let v1 = patch.v_start + patch.v_sweep;
            if v0 < -FRAC_PI_2 - 1e-7 || v1 > FRAC_PI_2 + 1e-7 {
                return Err(ThickenError::UnsupportedSurface);
            }
            (
                sphere.frame,
                arc_ring([0.0, 0.0], sphere.radius, second, v0, v1),
            )
        }
        Surface::Torus(torus) => {
            let second = torus.minor_radius + distance;
            positive_radii(&[torus.minor_radius, second])?;
            if torus.minor_radius >= torus.major_radius - 1e-9
                || second >= torus.major_radius - 1e-9
            {
                return Err(ThickenError::SelfIntersection);
            }
            let v0 = patch.v_start;
            let v1 = patch.v_start + patch.v_sweep;
            (
                torus.frame,
                arc_ring(
                    [torus.major_radius, 0.0],
                    torus.minor_radius,
                    second,
                    v0,
                    v1,
                ),
            )
        }
        Surface::Plane(_) | Surface::Nurbs(_) | Surface::Offset(_) => {
            return Err(ThickenError::UnsupportedSurface)
        }
    };

    let radial = frame.vector_at([patch.u_start.cos(), patch.u_start.sin()]);
    let axis = frame.normal().ok_or(ThickenError::UnsupportedSurface)?;
    let section = Plane::from_axes(frame.origin, radial, axis);
    super::revolve(section, &profile, frame.origin, axis, patch.u_sweep)
        .ok_or(ThickenError::SelfIntersection)
}

type H = [f64; 4];

fn thicken_nurbs_patch(
    source: &Body,
    face_key: FaceKey,
    surface: &NurbsSurface3,
    distance: f64,
) -> Result<Body, ThickenError> {
    let face = source
        .faces
        .get(face_key)
        .ok_or(ThickenError::UnsupportedSurface)?;
    if face.loops.len() != 1 {
        return Err(ThickenError::UnsupportedSurface);
    }
    let parts = super::pcurve::face_boundary_parts(source, face_key, 1e-8)
        .ok_or(ThickenError::UnsupportedSurface)?;
    let lines = parts
        .into_iter()
        .map(|(_, curve)| match curve {
            Curve::Line(line) => Ok(line),
            _ => Err(ThickenError::UnsupportedSurface),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if lines.len() != 4 {
        return Err(ThickenError::UnsupportedSurface);
    }
    let tolerance = (super::operation_tolerance(&[source]) * 8.0).max(distance.abs() * 2e-4);
    let (surface, offset) = offset_nurbs(surface, distance, tolerance)?;

    let mut body = Body::new();
    let lump = body.lumps.insert(Lump {
        shells: Vec::new(),
        provenance: Provenance::Synthesized,
    });
    let shell = body.shells.insert(Shell {
        faces: Vec::new(),
        owner: lump,
        provenance: Provenance::Synthesized,
    });
    body.lumps.get_mut(lump).unwrap().shells.push(shell);
    body.roots.push(lump);

    let mut source_vertices = Vec::with_capacity(4);
    let mut offset_vertices = Vec::with_capacity(4);
    for line in &lines {
        source_vertices.push(add_vertex(
            &mut body,
            surface.point_at_knot(line.start[0], line.start[1]),
        ));
        offset_vertices.push(add_vertex(
            &mut body,
            offset.point_at_knot(line.start[0], line.start[1]),
        ));
    }
    let connectors = source_vertices
        .iter()
        .zip(&offset_vertices)
        .map(|(source, offset)| add_line_edge(&mut body, *source, *offset))
        .collect::<Option<Vec<_>>>()
        .ok_or(ThickenError::SelfIntersection)?;

    let mut source_edges = Vec::with_capacity(4);
    let mut offset_edges = Vec::with_capacity(4);
    let mut side_surfaces = Vec::with_capacity(4);
    for (index, line) in lines.iter().enumerate() {
        let delta = [line.end[0] - line.start[0], line.end[1] - line.start[1]];
        let (axis, varying) = if delta[0].abs() <= 1e-8 && delta[1].abs() > 1e-8 {
            (0, 1)
        } else if delta[1].abs() <= 1e-8 && delta[0].abs() > 1e-8 {
            (1, 0)
        } else {
            return Err(ThickenError::UnsupportedSurface);
        };
        let source_curve = surface
            .isocurve(axis, line.start[axis])
            .ok_or(ThickenError::UnsupportedSurface)?;
        let offset_curve = offset
            .isocurve(axis, line.start[axis])
            .ok_or(ThickenError::UnsupportedSurface)?;
        // A certified offset has knots of its own: the side wall is ruled
        // between the two written alike.
        let (source_curve, offset_curve) = source_curve
            .made_compatible(&offset_curve)
            .ok_or(ThickenError::UnsupportedSurface)?;
        let source_rational = rational_curve(&source_curve);
        let offset_rational = rational_curve(&offset_curve);
        let next = (index + 1) % lines.len();
        source_edges.push(add_nurbs_edge(
            &mut body,
            source_curve,
            line.start[varying],
            line.end[varying],
            source_vertices[index],
            source_vertices[next],
        ));
        offset_edges.push(add_nurbs_edge(
            &mut body,
            offset_curve,
            line.start[varying],
            line.end[varying],
            offset_vertices[index],
            offset_vertices[next],
        ));
        side_surfaces.push((
            source_rational
                .ruled_to(&offset_rational)
                .ok_or(ThickenError::UnsupportedSurface)?,
            varying,
        ));
    }

    let source_forward = distance < 0.0;
    let offset_forward = distance > 0.0;
    let source_same = source_forward == face.forward;
    let offset_same = offset_forward == face.forward;
    let source_surface = body.surfaces.insert(Surface::Nurbs(surface.clone()));
    let offset_surface = body.surfaces.insert(Surface::Nurbs(offset.clone()));
    add_cap(
        &mut body,
        shell,
        source_surface,
        source_forward,
        &source_edges,
        &lines,
        source_same,
    )?;
    add_cap(
        &mut body,
        shell,
        offset_surface,
        offset_forward,
        &offset_edges,
        &lines,
        offset_same,
    )?;

    for (index, (wall, varying)) in side_surfaces.into_iter().enumerate() {
        let a = lines[index].start[varying];
        let b = lines[index].end[varying];
        let wall_source_forward = !source_same;
        let next = (index + 1) % lines.len();
        let (circuit, pcurves) = if wall_source_forward {
            (
                vec![
                    (source_edges[index], true),
                    (connectors[next], true),
                    (offset_edges[index], false),
                    (connectors[index], false),
                ],
                vec![
                    Line {
                        start: [a, 0.0],
                        end: [b, 0.0],
                    },
                    Line {
                        start: [b, 0.0],
                        end: [b, 1.0],
                    },
                    Line {
                        start: [b, 1.0],
                        end: [a, 1.0],
                    },
                    Line {
                        start: [a, 1.0],
                        end: [a, 0.0],
                    },
                ],
            )
        } else {
            (
                vec![
                    (source_edges[index], false),
                    (connectors[index], true),
                    (offset_edges[index], true),
                    (connectors[next], false),
                ],
                vec![
                    Line {
                        start: [b, 0.0],
                        end: [a, 0.0],
                    },
                    Line {
                        start: [a, 0.0],
                        end: [a, 1.0],
                    },
                    Line {
                        start: [a, 1.0],
                        end: [b, 1.0],
                    },
                    Line {
                        start: [b, 1.0],
                        end: [b, 0.0],
                    },
                ],
            )
        };
        let middle = 0.5 * (a + b);
        let uv = [
            lines[index].start[0] + 0.5 * (lines[index].end[0] - lines[index].start[0]),
            lines[index].start[1] + 0.5 * (lines[index].end[1] - lines[index].start[1]),
        ];
        let along = Vec3::from(surface.point_at_knot(
            lines[index].start[0] + 0.51 * (lines[index].end[0] - lines[index].start[0]),
            lines[index].start[1] + 0.51 * (lines[index].end[1] - lines[index].start[1]),
        )) - Vec3::from(surface.point_at_knot(
            lines[index].start[0] + 0.49 * (lines[index].end[0] - lines[index].start[0]),
            lines[index].start[1] + 0.49 * (lines[index].end[1] - lines[index].start[1]),
        ));
        let oriented_normal = Vec3::from(
            surface
                .normal_at_knot(uv[0], uv[1])
                .ok_or(ThickenError::SelfIntersection)?,
        ) * if face.forward { 1.0 } else { -1.0 };
        let wanted = along.cross(oriented_normal);
        let (wall, forward, pcurves) = match planar_nurbs(&wall, tolerance) {
            Some(plane) => {
                let normal = Vec3::from(plane.normal().ok_or(ThickenError::SelfIntersection)?);
                let pcurves = circuit_pcurves(&body, &plane, &circuit)
                    .ok_or(ThickenError::SelfIntersection)?;
                (Surface::Plane(plane), normal.dot(wanted) > 0.0, pcurves)
            }
            None => {
                let native = Vec3::from(
                    wall.normal_at_knot(middle, 0.5)
                        .ok_or(ThickenError::SelfIntersection)?,
                );
                (Surface::Nurbs(wall), native.dot(wanted) > 0.0, pcurves)
            }
        };
        let wall = body.surfaces.insert(wall);
        add_loop_face(&mut body, shell, wall, forward, &circuit, &pcurves)?;
    }

    if !body.validate().is_empty()
        || body.worst_vertex_gap() > tolerance
        || body.edges.iter().any(|(_, edge)| edge.coedges.len() != 2)
    {
        return Err(ThickenError::SelfIntersection);
    }
    Ok(body)
}

fn planar_nurbs(surface: &NurbsSurface3, tolerance: f64) -> Option<Plane> {
    let net = surface.control_points();
    let origin = Vec3::from(*net.first()?.first()?);
    let along = Vec3::from(*net.last()?.first()?) - origin;
    let across = Vec3::from(*net.first()?.last()?) - origin;
    let normal = along.cross(across).normalize()?;
    let plane = Plane::orthonormal(origin.to_array(), along.to_array(), normal.to_array())?;
    net.iter()
        .flatten()
        .all(|point| {
            plane
                .distance_to(*point)
                .is_some_and(|distance| distance.abs() <= tolerance)
        })
        .then_some(plane)
}

fn circuit_pcurves(body: &Body, plane: &Plane, circuit: &[(EdgeKey, bool)]) -> Option<Vec<Line>> {
    circuit
        .iter()
        .map(|(key, forward)| {
            let edge = body.edges.get(*key)?;
            let (start, end) = if *forward {
                (edge.start, edge.end)
            } else {
                (edge.end, edge.start)
            };
            Some(Line {
                start: plane.project(body.vertices.get(start)?.point)?,
                end: plane.project(body.vertices.get(end)?.point)?,
            })
        })
        .collect()
}

/// The surface and its offset: certified within `tolerance` everywhere
/// ([`super::certified_offset`]), refused where it folds, and sampled at the
/// knot spans where no certificate is reached.
fn offset_nurbs(
    source: &NurbsSurface3,
    distance: f64,
    tolerance: f64,
) -> Result<(NurbsSurface3, NurbsSurface3), ThickenError> {
    use super::certified_offset::{certified_offset, OffsetRefusal};
    match certified_offset(source, distance, tolerance) {
        Ok(offset) => Ok((source.clone(), offset.surface)),
        Err(OffsetRefusal::Folds { .. }) => Err(ThickenError::SelfIntersection),
        Err(_) => sampled_offset_nurbs(source, distance, tolerance),
    }
}

fn sampled_offset_nurbs(
    source: &NurbsSurface3,
    distance: f64,
    tolerance: f64,
) -> Result<(NurbsSurface3, NurbsSurface3), ThickenError> {
    let mut refined = source.clone();
    for _ in 0..7 {
        let ((_, _), (v0, v1)) = refined.domain();
        let (u_degree, v_degree) = refined.degrees();
        let (u_knots, v_knots) = refined.knots();
        let u_greville = greville(u_knots, u_degree, refined.control_points().len());
        let v_greville = greville(v_knots, v_degree, refined.control_points()[0].len());
        let periodicity = refined.periodicity();
        let mut points = refined.control_points().to_vec();
        for (u_index, row) in points.iter_mut().enumerate() {
            for (v_index, point) in row.iter_mut().enumerate() {
                let v = if refined.v_reversed() {
                    v0 + v1 - v_greville[v_index]
                } else {
                    v_greville[v_index]
                };
                let normal = Vec3::from(
                    refined
                        .normal_at_knot(u_greville[u_index], v)
                        .ok_or(ThickenError::SelfIntersection)?,
                );
                *point = (Vec3::from(*point) + normal * distance).to_array();
            }
        }
        let build = |points: Vec<Vec<[f64; 3]>>| {
            NurbsSurface3::new_strict(
                u_degree,
                v_degree,
                points,
                u_knots.to_vec(),
                v_knots.to_vec(),
                refined.weights().to_vec(),
            )
            .map(|surface| {
                surface
                    .with_periodicity(periodicity[0], periodicity[1])
                    .with_v_reversed(refined.v_reversed())
            })
        };
        for _ in 0..16 {
            let candidate = build(points.clone()).ok_or(ThickenError::SelfIntersection)?;
            let mut corrections = vec![vec![[0.0; 3]; points[0].len()]; points.len()];
            let mut worst = 0.0_f64;
            for u_index in 0..points.len() {
                for v_index in 0..points[0].len() {
                    let v = if refined.v_reversed() {
                        v0 + v1 - v_greville[v_index]
                    } else {
                        v_greville[v_index]
                    };
                    let source_point = Vec3::from(refined.point_at_knot(u_greville[u_index], v));
                    let normal = Vec3::from(
                        refined
                            .normal_at_knot(u_greville[u_index], v)
                            .ok_or(ThickenError::SelfIntersection)?,
                    );
                    let actual = Vec3::from(candidate.point_at_knot(u_greville[u_index], v));
                    let correction = source_point + normal * distance - actual;
                    worst = worst.max(correction.length());
                    corrections[u_index][v_index] = correction.to_array();
                }
            }
            if worst <= tolerance * 0.25 {
                break;
            }
            for (row, corrections) in points.iter_mut().zip(corrections) {
                for (point, correction) in row.iter_mut().zip(corrections) {
                    *point = (Vec3::from(*point) + Vec3::from(correction) * 0.85).to_array();
                }
            }
        }
        let offset = build(points).ok_or(ThickenError::SelfIntersection)?;
        let error = offset_error(&refined, &offset, distance)?;
        if error <= tolerance {
            return Ok((refined, offset));
        }
        refined = refine_surface(&refined).ok_or(ThickenError::UnsupportedSurface)?;
        if refined.control_points().len() * refined.control_points()[0].len() > 70_000 {
            break;
        }
    }
    Err(ThickenError::UnsupportedSurface)
}

fn offset_error(
    source: &NurbsSurface3,
    offset: &NurbsSurface3,
    distance: f64,
) -> Result<f64, ThickenError> {
    let (u_degree, v_degree) = source.degrees();
    let (u_knots, v_knots) = source.knots();
    let us = span_samples(u_knots, u_degree, source.control_points().len());
    let vs = span_samples(v_knots, v_degree, source.control_points()[0].len());
    let mut worst = 0.0_f64;
    for u in us {
        for &v in &vs {
            let point = Vec3::from(source.point_at_knot(u, v));
            let normal = Vec3::from(
                source
                    .normal_at_knot(u, v)
                    .ok_or(ThickenError::SelfIntersection)?,
            );
            let actual = Vec3::from(offset.point_at_knot(u, v));
            let offset_normal = Vec3::from(
                offset
                    .normal_at_knot(u, v)
                    .ok_or(ThickenError::SelfIntersection)?,
            );
            if offset_normal.dot(normal) <= 0.0 || (actual - point).dot(normal) * distance <= 0.0 {
                return Err(ThickenError::SelfIntersection);
            }
            worst = worst.max(actual.distance(point + normal * distance));
        }
    }
    Ok(worst)
}

fn greville(knots: &[f64], degree: usize, count: usize) -> Vec<f64> {
    (0..count)
        .map(|index| knots[index + 1..=index + degree].iter().sum::<f64>() / degree as f64)
        .collect()
}

fn span_samples(knots: &[f64], degree: usize, count: usize) -> Vec<f64> {
    let mut values = Vec::new();
    for span in degree..count {
        let (a, b) = (knots[span], knots[span + 1]);
        if b - a <= f64::EPSILON {
            continue;
        }
        values.extend([a, a + (b - a) * 0.25, 0.5 * (a + b), a + (b - a) * 0.75, b]);
    }
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
    values
}

fn refine_surface(surface: &NurbsSurface3) -> Option<NurbsSurface3> {
    let (u_degree, v_degree) = surface.degrees();
    let (u_knots, v_knots) = surface.knots();
    let mut net = surface
        .control_points()
        .iter()
        .zip(surface.weights())
        .map(|(row, weights)| {
            row.iter()
                .zip(weights)
                .map(|(point, weight)| {
                    [
                        point[0] * weight,
                        point[1] * weight,
                        point[2] * weight,
                        *weight,
                    ]
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut u_knots = u_knots.to_vec();
    let mut v_knots = v_knots.to_vec();
    for at in span_midpoints(&u_knots, u_degree, net.len()) {
        let columns = net[0].len();
        let mut next = vec![Vec::new(); net.len() + 1];
        let mut next_knots = None;
        for column in 0..columns {
            let curve = net.iter().map(|row| row[column]).collect::<Vec<_>>();
            let (refined, knots) = insert_knot(&curve, &u_knots, u_degree, at)?;
            for (row, point) in refined.into_iter().enumerate() {
                next[row].push(point);
            }
            next_knots = Some(knots);
        }
        net = next;
        u_knots = next_knots?;
    }
    for at in span_midpoints(&v_knots, v_degree, net[0].len()) {
        let mut next = Vec::with_capacity(net.len());
        let mut next_knots = None;
        for row in net {
            let (refined, knots) = insert_knot(&row, &v_knots, v_degree, at)?;
            next.push(refined);
            next_knots = Some(knots);
        }
        net = next;
        v_knots = next_knots?;
    }
    let points = net
        .iter()
        .map(|row| {
            row.iter()
                .map(|point| {
                    [
                        point[0] / point[3],
                        point[1] / point[3],
                        point[2] / point[3],
                    ]
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let weights = net
        .iter()
        .map(|row| row.iter().map(|point| point[3]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let periodicity = surface.periodicity();
    Some(
        NurbsSurface3::new_strict(u_degree, v_degree, points, u_knots, v_knots, weights)?
            .with_periodicity(periodicity[0], periodicity[1])
            .with_v_reversed(surface.v_reversed()),
    )
}

fn span_midpoints(knots: &[f64], degree: usize, count: usize) -> Vec<f64> {
    (degree..count)
        .filter_map(|index| {
            let (a, b) = (knots[index], knots[index + 1]);
            (b - a > f64::EPSILON).then_some(0.5 * (a + b))
        })
        .collect()
}

fn insert_knot(control: &[H], knots: &[f64], degree: usize, at: f64) -> Option<(Vec<H>, Vec<f64>)> {
    let n = control.len().checked_sub(1)?;
    let k = (degree..=n).find(|index| knots[*index] <= at && at < knots[*index + 1])?;
    let multiplicity = knots
        .iter()
        .filter(|knot| (**knot - at).abs() <= 1e-12)
        .count();
    if multiplicity >= degree {
        return None;
    }
    let mut refined = vec![[0.0; 4]; control.len() + 1];
    refined[..=k - degree].copy_from_slice(&control[..=k - degree]);
    refined[k - multiplicity + 1..n + 2].copy_from_slice(&control[k - multiplicity..n + 1]);
    for index in k - degree + 1..=k - multiplicity {
        let width = knots[index + degree] - knots[index];
        if width <= 0.0 {
            return None;
        }
        let alpha = (at - knots[index]) / width;
        refined[index] = std::array::from_fn(|axis| {
            control[index - 1][axis] * (1.0 - alpha) + control[index][axis] * alpha
        });
    }
    let mut knots = knots.to_vec();
    knots.insert(k + 1, at);
    Some((refined, knots))
}

fn rational_curve(curve: &ocs_geom::space::NurbsCurve3) -> RationalCurve3 {
    RationalCurve3 {
        degree: curve.degree(),
        knots: curve.knots().to_vec(),
        points: curve.control_points().to_vec(),
        weights: curve.weights().to_vec(),
    }
}

fn add_vertex(body: &mut Body, point: [f64; 3]) -> VertexKey {
    body.vertices.insert(Vertex {
        point,
        provenance: Provenance::Synthesized,
    })
}

fn add_line_edge(body: &mut Body, start: VertexKey, end: VertexKey) -> Option<EdgeKey> {
    let a = body.vertices.get(start)?.point;
    let b = body.vertices.get(end)?.point;
    let direction = Vec3::from(b) - Vec3::from(a);
    (direction.length() > 1e-12).then_some(())?;
    let curve = body.curves.insert(Curve3::Line(Line3 {
        origin: a,
        direction: direction.to_array(),
    }));
    Some(body.edges.insert(Edge {
        curve,
        start_parameter: 0.0,
        end_parameter: 1.0,
        start,
        end,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    }))
}

fn add_nurbs_edge(
    body: &mut Body,
    curve: ocs_geom::space::NurbsCurve3,
    start_parameter: f64,
    end_parameter: f64,
    start: VertexKey,
    end: VertexKey,
) -> EdgeKey {
    let curve = body.curves.insert(Curve3::Nurbs(curve));
    body.edges.insert(Edge {
        curve,
        start_parameter,
        end_parameter,
        start,
        end,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    })
}

fn add_cap(
    body: &mut Body,
    shell: ShellKey,
    surface: super::SurfaceKey,
    forward: bool,
    edges: &[EdgeKey],
    lines: &[Line],
    same_direction: bool,
) -> Result<(), ThickenError> {
    let (circuit, pcurves) = if same_direction {
        (
            edges.iter().map(|edge| (*edge, true)).collect::<Vec<_>>(),
            lines.to_vec(),
        )
    } else {
        (
            edges.iter().rev().map(|edge| (*edge, false)).collect(),
            lines
                .iter()
                .rev()
                .map(|line| Line {
                    start: line.end,
                    end: line.start,
                })
                .collect(),
        )
    };
    add_loop_face(body, shell, surface, forward, &circuit, &pcurves)
}

fn add_loop_face(
    body: &mut Body,
    shell: ShellKey,
    surface: super::SurfaceKey,
    forward: bool,
    circuit: &[(EdgeKey, bool)],
    pcurves: &[Line],
) -> Result<(), ThickenError> {
    if circuit.len() != pcurves.len() {
        return Err(ThickenError::UnsupportedSurface);
    }
    let face = body.faces.insert(Face {
        surface,
        forward,
        loops: Vec::new(),
        owner: shell,
        provenance: Provenance::Synthesized,
    });
    let ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    for ((edge, forward), pcurve) in circuit.iter().zip(pcurves) {
        let coedge = body.coedges.insert(Coedge {
            edge: *edge,
            forward: *forward,
            pcurve: Some(Curve::Line(*pcurve)),
            owner: ring,
            provenance: Provenance::Synthesized,
        });
        body.edges
            .get_mut(*edge)
            .ok_or(ThickenError::UnsupportedSurface)?
            .coedges
            .push(coedge);
        body.loops.get_mut(ring).unwrap().coedges.push(coedge);
    }
    body.faces.get_mut(face).unwrap().loops.push(ring);
    body.shells.get_mut(shell).unwrap().faces.push(face);
    Ok(())
}

fn line_ring(points: &[[f64; 2]; 4]) -> Vec<Curve> {
    (0..4)
        .map(|index| {
            Curve::Line(Line {
                start: points[index],
                end: points[(index + 1) % 4],
            })
        })
        .collect()
}

fn arc_ring(
    centre: [f64; 2],
    first_radius: f64,
    second_radius: f64,
    start: f64,
    end: f64,
) -> Vec<Curve> {
    let at = |radius: f64, angle: f64| {
        [
            centre[0] + radius * angle.cos(),
            centre[1] + radius * angle.sin(),
        ]
    };
    vec![
        Curve::Arc(Arc {
            centre,
            radius: first_radius,
            start_angle: start,
            end_angle: end,
        }),
        Curve::Line(Line {
            start: at(first_radius, end),
            end: at(second_radius, end),
        }),
        Curve::Arc(Arc {
            centre,
            radius: second_radius,
            start_angle: start,
            end_angle: end,
        }),
        Curve::Line(Line {
            start: at(second_radius, start),
            end: at(first_radius, start),
        }),
    ]
}

fn positive_radii(values: &[f64]) -> Result<(), ThickenError> {
    if values
        .iter()
        .all(|value| value.is_finite() && *value > 1e-9)
    {
        Ok(())
    } else {
        Err(ThickenError::SelfIntersection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analytic_mass_properties, body_bounds, extrude_surface, mesh_body, planar_region,
        revolve_surface,
    };
    use ocs_geom::geom2d::{Circle, NurbsCurve};
    use ocs_geom::space::Parameterization;
    use std::f64::consts::PI;

    fn cylinder_sheet(angle: f64) -> Body {
        revolve_surface(
            Plane::from_axes([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            &[Curve::Line(Line {
                start: [4.0, 0.0],
                end: [4.0, 3.0],
            })],
            [0.0; 3],
            [0.0, 0.0, 1.0],
            angle,
        )
        .unwrap()
    }

    #[test]
    fn planar_thickening_preserves_holes_and_signed_direction() {
        let sheet = planar_region(
            Plane::XY,
            &[
                vec![Curve::Circle(Circle {
                    centre: [0.0; 2],
                    radius: 4.0,
                })],
                vec![Curve::Circle(Circle {
                    centre: [0.0; 2],
                    radius: 2.0,
                })],
            ],
        )
        .unwrap();
        let before = format!("{sheet:?}");
        for distance in [-2.0, 2.0] {
            let solid = thicken(&sheet, distance).unwrap();
            assert!(solid.validate().is_empty());
            assert!(solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
            let bounds = body_bounds(&solid).unwrap();
            assert!((bounds.max[2] - bounds.min[2] - 2.0).abs() < 1e-9);
            let volume = mesh_body(&solid, 0.03, 1e-7).mass_properties().unwrap().0;
            assert!((volume - 24.0 * PI).abs() < 0.1);
        }
        assert_eq!(format!("{sheet:?}"), before);
    }

    #[test]
    fn cylinder_sectors_keep_the_entire_angular_span_and_exact_mass() {
        for angle in [PI * 0.5, PI * 1.5, PI * 1.99, TAU, -PI * 1.5] {
            let sheet = cylinder_sheet(angle);
            for distance in [-0.5, 0.5] {
                let face = sheet.faces.get(sheet.face_keys().next().unwrap()).unwrap();
                let second = 4.0 + distance * if face.forward { 1.0 } else { -1.0 };
                let solid = thicken(&sheet, distance).unwrap();
                assert!(solid.validate().is_empty());
                assert!(solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
                let properties = crate::mass_properties(&solid).unwrap();
                let expected = angle.abs() * 0.5 * (second * second - 16.0_f64).abs() * 3.0;
                assert!(
                    (properties.volume - expected).abs() < 1e-8,
                    "{angle}: {} != {expected}",
                    properties.volume
                );
                assert!((properties.centroid[2] - 1.5).abs() < 1e-8);
                let volume = mesh_body(&solid, 0.02, 1e-7).mass_properties().unwrap().0;
                assert!((volume - expected).abs() < 0.02 * expected);
            }
        }
    }

    #[test]
    fn annular_mass_does_not_replace_a_closed_cylinder_shell() {
        let body = crate::make::cylinder([0.0; 3], 4.0, 6.0).unwrap();
        let shell = crate::shell(&body, &[], 0.5).unwrap();
        let properties = analytic_mass_properties(&shell).unwrap();
        let expected = PI * (16.0 * 6.0 - 3.5_f64.powi(2) * 5.0);
        assert!((properties.volume - expected).abs() < 1e-8);
    }

    #[test]
    fn invalid_and_unsupported_offsets_do_not_create_approximate_solids() {
        let sheet = cylinder_sheet(TAU);
        for distance in [0.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                thicken(&sheet, distance),
                Err(ThickenError::InvalidDistance)
            ));
        }
        let face = sheet.faces.get(sheet.face_keys().next().unwrap()).unwrap();
        let collapsing = if face.forward { -5.0 } else { 5.0 };
        assert!(matches!(
            thicken(&sheet, collapsing),
            Err(ThickenError::SelfIntersection)
        ));
        let solid = crate::make::cuboid([0.0; 3], [1.0; 3]).unwrap();
        assert!(matches!(
            thicken(&solid, 0.2),
            Err(ThickenError::UnsupportedSurface)
        ));
    }
    #[test]
    fn curved_analytic_patches_produce_closed_exact_bodies() {
        let profiles = [
            Curve::Line(Line {
                start: [4.0, 0.0],
                end: [2.0, 3.0],
            }),
            Curve::Arc(Arc {
                centre: [0.0; 2],
                radius: 4.0,
                start_angle: -PI / 4.0,
                end_angle: PI / 4.0,
            }),
            Curve::Arc(Arc {
                centre: [6.0, 0.0],
                radius: 2.0,
                start_angle: -PI / 4.0,
                end_angle: PI / 4.0,
            }),
        ];
        for profile in profiles {
            let sheet = revolve_surface(
                Plane::from_axes([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                &[profile],
                [0.0; 3],
                [0.0, 0.0, 1.0],
                PI * 1.5,
            )
            .unwrap();
            for distance in [-0.2, 0.2] {
                let solid = thicken(&sheet, distance).unwrap();
                assert!(solid.validate().is_empty());
                assert!(solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
                assert!(solid
                    .surfaces
                    .iter()
                    .all(|(_, surface)| !surface.is_spline()));
                assert!(mesh_body(&solid, 0.05, 1e-7).mass_properties().unwrap().0 > 0.0);
            }
        }
    }

    #[test]
    fn free_form_sheet_thickens_to_a_watertight_nurbs_body() {
        let profile = NurbsCurve::interpolate(
            &[[0.0, 0.0], [1.0, 0.8], [2.0, -0.6], [3.0, 0.2]],
            None,
            None,
            Parameterization::Chord,
        )
        .unwrap();
        let sheet = extrude_surface(Plane::XY, &[Curve::Nurbs(profile)], [0.0, 0.0, 2.0]).unwrap();
        assert_eq!(sheet.faces.len(), 1);
        assert!(matches!(
            sheet.surfaces.iter().next().map(|(_, surface)| surface),
            Some(Surface::Nurbs(_))
        ));
        for distance in [-0.15, 0.15] {
            let solid = thicken(&sheet, distance).unwrap();
            assert!(solid.validate().is_empty());
            assert!(solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
            assert_eq!(solid.faces.len(), 6);
            assert!(
                solid
                    .surfaces
                    .iter()
                    .filter(|(_, surface)| surface.is_spline())
                    .count()
                    >= 2
            );
            assert!(mesh_body(&solid, 0.03, 1e-7)
                .mass_properties()
                .is_some_and(|(volume, _)| volume > 0.1));
        }
    }

    #[test]
    fn connected_planar_sheet_faces_thicken_as_one_solid() {
        let profile = [
            Curve::Line(Line {
                start: [0.0, 0.0],
                end: [2.0, 0.0],
            }),
            Curve::Line(Line {
                start: [2.0, 0.0],
                end: [3.0, 2.0],
            }),
            Curve::Line(Line {
                start: [3.0, 2.0],
                end: [4.0, 1.0],
            }),
        ];
        let sheet = extrude_surface(Plane::XY, &profile, [0.0, 0.0, 3.0]).unwrap();
        assert_eq!(sheet.faces.len(), 3);
        let solid = thicken(&sheet, 0.25).unwrap();
        assert!(solid.validate().is_empty());
        assert!(solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
        assert_eq!(solid.roots.len(), 1);
        assert!(mesh_body(&solid, 0.03, 1e-7)
            .mass_properties()
            .is_some_and(|(volume, _)| volume > 2.0));
    }

    #[test]
    fn connected_free_form_and_planar_faces_thicken_as_one_solid() {
        let spline = NurbsCurve::interpolate(
            &[[0.0, 0.0], [0.7, 0.15], [1.3, -0.1], [2.0, 0.0]],
            None,
            None,
            Parameterization::Chord,
        )
        .unwrap();
        let profile = [
            Curve::Nurbs(spline),
            Curve::Line(Line {
                start: [2.0, 0.0],
                end: [2.0, 2.0],
            }),
        ];
        let sheet = extrude_surface(Plane::XY, &profile, [0.0, 0.0, 2.0]).unwrap();
        let solid = thicken(&sheet, 0.15).unwrap();
        assert!(solid.validate().is_empty());
        assert!(solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
        assert_eq!(solid.roots.len(), 1);
    }

    #[test]
    fn free_form_offset_that_folds_is_rejected() {
        let profile = NurbsCurve::interpolate(
            &[[0.0, 0.0], [0.7, 0.6], [1.3, -0.4], [2.0, 0.0]],
            None,
            None,
            Parameterization::Chord,
        )
        .unwrap();
        let sheet = extrude_surface(Plane::XY, &[Curve::Nurbs(profile)], [0.0, 0.0, 2.0]).unwrap();
        assert!(matches!(
            thicken(&sheet, 0.15),
            Err(ThickenError::SelfIntersection)
        ));
    }
}
