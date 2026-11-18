//! Making a solid by moving a profile.
//!
//! The two constructions every modeller is built on. An extrusion drags a
//! closed outline along a straight line; a revolution turns it about an axis.
//! Between them they account for most of what a drawing's solids are, and
//! every primitive here is one of the two applied to the right outline.
//!
//! # The profile is a plane curve, and stays one
//!
//! The outline is given in a plane's own coordinates, as the chain of curves
//! [`geom2d`](ocs_geom::geom2d) already speaks in. That is what lets a side wall
//! know what it is: a straight run sweeps into a plane, an arc into a
//! cylinder, and anything else has no analytic surface to sweep into — so it
//! is refused rather than approximated into a spline nobody asked for.
//!
//! # Why the caps are not just the profile twice
//!
//! They face opposite ways. A cap built by copying the other and moving it
//! points the same way as the original, which leaves a solid with two lids
//! and no bottom — it passes every local check and encloses nothing.
//!
//! # Which way is out
//!
//! Two things decide it, and neither can be assumed. The sweep may run along
//! the profile plane's normal or against it, and the profile itself may be
//! wound either way — a face lifted out of a file arrives however the file
//! had it. Their product is what says whether the frame's own directions
//! already point outwards or need turning round, and it is worked out here
//! rather than required of the caller: a builder that quietly demands a
//! counter-clockwise profile swept upwards produces an inside-out solid for
//! every other input, and an inside-out solid still passes validation.
//!
//! Which way a *face* runs against its own surface is then asked of the
//! surface rather than tabulated per kind — see [`face_sense`]. Five kinds
//! times two constructions is ten rules to get right and one place for each
//! to be wrong; a signed distance answers all of them at once.

use super::geometry::{Circle3, Cone, Curve3, Cylinder, Ellipse3, Line3, Sphere, Surface, Torus};
use super::nurbs_builder::RationalCurve2;
use super::topology::{
    Body, Coedge, CoedgeKey, Edge, EdgeKey, Face, FaceKey, Loop, LoopKey, Lump, LumpKey, Shell,
    ShellKey, SurfaceKey, Vertex, VertexKey,
};
use super::Provenance;
use ocs_geom::geom2d::Curve as Curve2;
use ocs_geom::space::{Plane, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};
use std::f64::consts::TAU;

/// Extrudes a closed profile along `direction`.
///
/// The profile is a chain of curves in `plane`'s coordinates, joined end to
/// end and closing onto the first. Straight and circular pieces are
/// supported; a spline profile has no analytic side wall and is refused.
///
/// A piece may be given running either way round — [`Curve::segments`] hands
/// back a clockwise bulge exactly that way, and a concave arc cannot be
/// written any other way, since an [`Arc`](ocs_geom::geom2d::Arc) always
/// parameterises counter-clockwise. So the chain is followed by which ends
/// meet rather than assumed to run head to tail.
///
/// The profile may be wound either way and the sweep may run either way
/// across the plane; the result is a solid with its normals out regardless.
///
/// `None` for a direction lying in the plane — an extrusion of no thickness
/// is not a solid — for a profile that does not close, and for one that
/// encloses nothing.
pub fn extrude(plane: Plane, profile: &[Curve2], direction: [f64; 3]) -> Option<Body> {
    let along = Vec3::from(direction);
    let normal = Vec3::from(plane.normal()?);
    // A direction across the plane sweeps the profile through itself.
    let up = along.dot(normal);
    if up.abs() <= 1e-12 * along.length().max(1.0) {
        return None;
    }
    // Which way round each piece is written, and where the loop turns.
    let senses = profile_senses(profile)?;
    let corners: Vec<[f64; 2]> = profile
        .iter()
        .zip(&senses)
        .map(|(piece, forwards)| piece.point_at(if *forwards { 0.0 } else { 1.0 }))
        .collect();

    // Positive when the profile runs counter-clockwise in its own frame. A
    // piece written backwards contributes the negative of its own integral,
    // which is what makes this the loop's area rather than the pieces'.
    //
    // A profile that encloses nothing has no inside to sweep, and is refused
    // here rather than becoming a body with faces on top of each other.
    let handed: f64 = profile
        .iter()
        .zip(&senses)
        .map(|(piece, forwards)| {
            if *forwards {
                piece.chain_area()
            } else {
                -piece.chain_area()
            }
        })
        .sum();
    if handed == 0.0 || handed.is_nan() {
        return None;
    }
    // The canonical case is a counter-clockwise profile swept along the
    // normal. Everything below turns round when this is negative.
    let outward = (up * handed) > 0.0;

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

    // Both rings of vertices, and the rails between them.
    let base: Vec<VertexKey> = corners
        .iter()
        .map(|uv| add_vertex(&mut body, plane.point_at(*uv)))
        .collect();
    let top: Vec<VertexKey> = corners
        .iter()
        .map(|uv| add_vertex(&mut body, (Vec3::from(plane.point_at(*uv)) + along).to_array()))
        .collect();

    let base_edges = profile_edges(&mut body, &plane, profile, &senses, &base)?;
    let far = Plane::from_axes(
        (Vec3::from(plane.origin) + along).to_array(),
        plane.x_axis,
        plane.y_axis,
    );
    let top_edges = profile_edges(&mut body, &far, profile, &senses, &top)?;
    let rails: Vec<EdgeKey> = (0..corners.len())
        .map(|index| add_line_edge(&mut body, base[index], top[index]))
        .collect::<Option<Vec<_>>>()?;

    // The two caps. Each looks away from the solid, so which of them agrees
    // with the shared plane's own normal is decided by the sweep's direction
    // alone; how its loop runs is decided by the profile's winding as well.
    add_cap(&mut body, shell, plane, &base_edges, &senses, up < 0.0, !outward)?;
    add_cap(&mut body, shell, far, &top_edges, &senses, up > 0.0, outward)?;

    // One wall per profile piece: up the rail, along the top, down the next
    // rail, back along the bottom.
    for index in 0..profile.len() {
        let next = (index + 1) % profile.len();
        let piece = &profile[index];
        let (surface, spline) = wall_surface(&mut body, &plane, piece, along)?;
        let out = piece_outward(&plane, piece, senses[index], handed > 0.0);
        let on = plane.point_at(piece.point_at(0.5));
        let forward = face_sense(body.surfaces.get(surface)?, on, out)?;
        let circuit = vec![
            (base_edges[index], senses[index]),
            (rails[next], true),
            (top_edges[index], !senses[index]),
            (rails[index], false),
        ];
        if spline {
            let start = if senses[index] { 0.0 } else { 1.0 };
            let end = 1.0 - start;
            let pcurves = vec![
                ([start, 0.0], [end, 0.0]),
                ([end, 0.0], [end, 1.0]),
                ([end, 1.0], [start, 1.0]),
                ([start, 1.0], [start, 0.0]),
            ];
            let (circuit, pcurves) = reorder_with_pcurves(circuit, pcurves, outward);
            add_wall_with_pcurves(&mut body, shell, surface, forward, &circuit, &pcurves)?;
        } else {
            add_wall(&mut body, shell, surface, forward, &reorder(circuit, outward))?;
        }
    }

    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    body.validate().is_empty().then_some(body)
}

/// Extrudes a connected open or closed curve chain into an open sheet body.
///
/// Unlike [`extrude`], no end caps are added. An open source therefore makes
/// one face per curve piece, while a closed source makes the lateral skin of
/// an extrusion. Boundary edges intentionally have one coedge: that is the
/// valid topology of an open shell rather than a damaged solid.
pub fn extrude_surface(
    plane: Plane,
    profile: &[Curve2],
    direction: [f64; 3],
) -> Option<Body> {
    let along = Vec3::from(direction);
    if along.length() <= 1e-12 || !along.length().is_finite() {
        return None;
    }
    let closed = profile_senses(profile);
    let (senses, corners) = match closed {
        Some(senses) => {
            let corners = profile
                .iter()
                .zip(&senses)
                .map(|(piece, forward)| piece.point_at(if *forward { 0.0 } else { 1.0 }))
                .collect::<Vec<_>>();
            (senses, corners)
        }
        None => {
            let senses = path_senses(profile)?;
            let mut corners = profile
                .iter()
                .zip(&senses)
                .map(|(piece, forward)| piece.point_at(if *forward { 0.0 } else { 1.0 }))
                .collect::<Vec<_>>();
            let last = profile.last()?.point_at(if *senses.last()? { 1.0 } else { 0.0 });
            corners.push(last);
            (senses, corners)
        }
    };
    if profile.is_empty() || corners.len() < 2 {
        return None;
    }
    let is_closed = corners.len() == profile.len();

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
    let base = corners
        .iter()
        .map(|point| add_vertex(&mut body, plane.point_at(*point)))
        .collect::<Vec<_>>();
    let top = corners
        .iter()
        .map(|point| add_vertex(&mut body, (Vec3::from(plane.point_at(*point)) + along).to_array()))
        .collect::<Vec<_>>();
    let far = Plane::from_axes(
        (Vec3::from(plane.origin) + along).to_array(),
        plane.x_axis,
        plane.y_axis,
    );

    let mut base_edges = Vec::with_capacity(profile.len());
    let mut top_edges = Vec::with_capacity(profile.len());
    for (index, piece) in profile.iter().enumerate() {
        let next = if is_closed { (index + 1) % corners.len() } else { index + 1 };
        let (base_from, base_to) = if senses[index] {
            (base[index], base[next])
        } else {
            (base[next], base[index])
        };
        let (top_from, top_to) = if senses[index] {
            (top[index], top[next])
        } else {
            (top[next], top[index])
        };
        base_edges.push(add_profile_edge(&mut body, &plane, piece, base_from, base_to)?);
        top_edges.push(add_profile_edge(&mut body, &far, piece, top_from, top_to)?);
    }
    let rails = (0..corners.len())
        .map(|index| add_line_edge(&mut body, base[index], top[index]))
        .collect::<Option<Vec<_>>>()?;

    for (index, piece) in profile.iter().enumerate() {
        let next = if is_closed { (index + 1) % corners.len() } else { index + 1 };
        let (surface, spline) = wall_surface(&mut body, &plane, piece, along)?;
        let tangent = Vec3::from(plane.vector_at(piece.tangent_at(0.5)));
        let out = tangent * if senses[index] { 1.0 } else { -1.0 };
        let out = out.cross(along);
        let on = plane.point_at(piece.point_at(0.5));
        let forward = face_sense(body.surfaces.get(surface)?, on, out)?;
        let circuit = vec![
            (base_edges[index], senses[index]),
            (rails[next], true),
            (top_edges[index], !senses[index]),
            (rails[index], false),
        ];
        if spline {
            let start = if senses[index] { 0.0 } else { 1.0 };
            let end = 1.0 - start;
            add_wall_with_pcurves(
                &mut body,
                shell,
                surface,
                forward,
                &circuit,
                &[
                    ([start, 0.0], [end, 0.0]),
                    ([end, 0.0], [end, 1.0]),
                    ([end, 1.0], [start, 1.0]),
                    ([start, 1.0], [start, 0.0]),
                ],
            )?;
        } else {
            add_wall(&mut body, shell, surface, forward, &circuit)?;
        }
    }

    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    body.validate().is_empty().then_some(body)
}

/// Extrudes a closed line-and-arc profile with a constant taper angle.
///
/// A positive angle moves the far outline inward and a negative angle moves
/// it outward. The offset distance is measured perpendicular to the profile
/// plane, so an oblique extrusion keeps the requested wall angle.
#[cfg(feature = "offset")]
pub fn extrude_tapered(
    plane: Plane,
    profile: &[Curve2],
    direction: [f64; 3],
    taper_angle: f64,
) -> Option<Body> {
    if !taper_angle.is_finite()
        || taper_angle.abs() >= std::f64::consts::FRAC_PI_2
    {
        return None;
    }
    if taper_angle.abs() <= 1e-12 {
        return extrude(plane, profile, direction);
    }
    // A circular draft remains an analytic cone. A ruled spline loft loses
    // that identity and makes its otherwise ordinary cap uneditable.
    if let Some((centre, radius)) = complete_circular_profile(profile) {
        let normal = Vec3::from(plane.normal()?);
        let along = Vec3::from(direction);
        let height = along.dot(normal);
        if along.cross(normal).length() <= 1e-10 * along.length().max(1.0) {
            let top = radius - height.abs() * taper_angle.tan();
            if top <= 1e-12 || height.abs() <= 1e-12 { return None; }
            let origin = plane.point_at(centre);
            let section = Plane::from_axes(origin, plane.x_axis, normal.to_array());
            let points = [[0.0, 0.0], [radius, 0.0], [top, height], [0.0, height]];
            let edges = (0..4).map(|index| Curve2::Line(ocs_geom::geom2d::Line {
                start: points[index], end: points[(index + 1) % 4],
            })).collect::<Vec<_>>();
            return revolve(section, &edges, origin, normal.to_array(), TAU);
        }
    }
    let senses = profile_senses(profile)?;
    let along = Vec3::from(direction);
    let height = along.dot(Vec3::from(plane.normal()?)).abs();
    let offset = height * taper_angle.tan().abs();
    if !offset.is_finite() || offset <= 1e-12 {
        return None;
    }

    let mut vertices = Vec::with_capacity(profile.len());
    for (piece, forward) in profile.iter().zip(&senses) {
        let position = piece.point_at(if *forward { 0.0 } else { 1.0 });
        let bulge = match piece {
            Curve2::Line(_) => 0.0,
            Curve2::Arc(arc) => {
                let signed = arc.sweep() * if *forward { 1.0 } else { -1.0 };
                (signed * 0.25).tan()
            }
            _ => return None,
        };
        vertices.push(ocs_geom::geom2d::PolylineVertex { position, bulge });
    }
    let source = ocs_geom::geom2d::Polyline {
        vertices,
        closed: true,
    };
    let signed_area = profile
        .iter()
        .zip(&senses)
        .map(|(piece, forward)| piece.chain_area() * if *forward { 1.0 } else { -1.0 })
        .sum::<f64>();
    if !signed_area.is_finite() || signed_area.abs() <= 1e-12 {
        return None;
    }
    let first = profile.first()?;
    let forward = *senses.first()?;
    let point = first.point_at(0.5);
    let tangent = first.tangent_at(0.5);
    let tangent = if forward {
        tangent
    } else {
        [-tangent[0], -tangent[1]]
    };
    let tangent_length = tangent[0].hypot(tangent[1]);
    if !tangent_length.is_finite() || tangent_length <= 1e-12 {
        return None;
    }
    let tangent = [
        tangent[0] / tangent_length,
        tangent[1] / tangent_length,
    ];
    let inward = if signed_area > 0.0 {
        [-tangent[1], tangent[0]]
    } else {
        [tangent[1], -tangent[0]]
    };
    let side_sign = if taper_angle > 0.0 { 1.0 } else { -1.0 };
    let side = [
        point[0] + inward[0] * offset * side_sign,
        point[1] + inward[1] * offset * side_sign,
    ];
    let top = ocs_geom::geom2d::offset_polyline(&source, offset, side)
        .into_iter()
        .max_by(|a, b| {
            let area = |polyline: &ocs_geom::geom2d::Polyline| {
                Curve2::Polyline(polyline.clone())
                    .segments()
                    .iter()
                    .map(Curve2::chain_area)
                    .sum::<f64>()
                    .abs()
            };
            area(a).total_cmp(&area(b))
        })?;
    let top_profile = Curve2::Polyline(top).segments();
    if top_profile.len() != profile.len() {
        return None;
    }
    // A closed inward offset can survive numerically after the outline has
    // collapsed: the offset library then returns a loop on the far side of
    // the medial axis. That loop grows again as the requested inset grows,
    // which makes an interactive draft grip reverse from narrowing to
    // widening. A valid parallel outline remains exactly `offset` away from
    // its source and encloses less area. Do not compare winding here:
    // `profile_senses` may legitimately choose the reverse traversal while
    // the offset polyline keeps its source storage order.
    if taper_angle > 0.0 {
        let coordinate_scale = source
            .vertices
            .iter()
            .flat_map(|vertex| vertex.position)
            .map(f64::abs)
            .fold(offset.max(1.0), f64::max);
        let tolerance = ocs_geom::geom2d::Tolerance::new(
            (offset * 1e-5).max(coordinate_scale * 1e-10).max(1e-9),
        );
        if top_profile.iter().any(|piece| {
            let point = piece.point_at(0.5);
            let distance = profile
                .iter()
                .map(|source_piece| ocs_geom::geom2d::distance_to(source_piece, point))
                .fold(f64::INFINITY, f64::min);
            !distance.is_finite() || (distance - offset).abs() > tolerance.linear()
        }) {
            return None;
        }
        let top_senses = profile_senses(&top_profile)?;
        let top_area = top_profile
            .iter()
            .zip(top_senses)
            .map(|(piece, forward)| piece.chain_area() * if forward { 1.0 } else { -1.0 })
            .sum::<f64>();
        if !top_area.is_finite() || top_area.abs() >= signed_area.abs() {
            return None;
        }
    }
    let far = Plane::from_axes(
        (Vec3::from(plane.origin) + along).to_array(),
        plane.x_axis,
        plane.y_axis,
    );
    super::loft::loft(&[(plane, profile.to_vec()), (far, top_profile)])
}

/// Extrudes a closed profile into a tapered lateral sheet without end caps.
#[cfg(feature = "offset")]
pub fn extrude_surface_tapered(
    plane: Plane,
    profile: &[Curve2],
    direction: [f64; 3],
    taper_angle: f64,
) -> Option<Body> {
    if !taper_angle.is_finite()
        || taper_angle.abs() >= std::f64::consts::FRAC_PI_2
    {
        return None;
    }
    if taper_angle.abs() <= 1e-12 {
        return extrude_surface(plane, profile, direction);
    }
    if profile_senses(profile).is_none() {
        return extrude_open_surface_tapered(plane, profile, direction, taper_angle);
    }
    let mut body = extrude_tapered(plane, profile, direction, taper_angle)?;
    let lump = *body.roots.first()?;
    let shell = *body.lumps.get(lump)?.shells.first()?;
    let far = Plane::from_axes((Vec3::from(plane.origin) + Vec3::from(direction)).to_array(), plane.x_axis, plane.y_axis);
    let caps = vec![extrusion_cap(&body, &plane)?, extrusion_cap(&body, &far)?];
    if caps.len() != 2
        || caps.iter().any(|face| {
            body.faces
                .get(*face)
                .and_then(|face| body.surfaces.get(face.surface))
                .is_none_or(|surface| !matches!(surface, Surface::Plane(_)))
        })
    {
        return None;
    }
    for face_key in caps {
        let face = body.faces.get(face_key)?.clone();
        for loop_key in face.loops {
            let ring = body.loops.get(loop_key)?.clone();
            for coedge_key in ring.coedges {
                let edge_key = body.coedges.get(coedge_key)?.edge;
                body.edges
                    .get_mut(edge_key)?
                    .coedges
                    .retain(|candidate| *candidate != coedge_key);
                body.coedges.remove(coedge_key)?;
            }
            body.loops.remove(loop_key)?;
        }
        body.faces.remove(face_key)?;
        body.shells
            .get_mut(shell)?
            .faces
            .retain(|candidate| *candidate != face_key);
        if !body.faces.iter().any(|(_, remaining)| remaining.surface == face.surface) {
            body.surfaces.remove(face.surface);
        }
    }
    body.validate().is_empty().then_some(body)
}

#[cfg(feature = "offset")]
fn extrude_open_surface_tapered(
    plane: Plane,
    profile: &[Curve2],
    direction: [f64; 3],
    taper_angle: f64,
) -> Option<Body> {
    let senses = path_senses(profile)?;
    let along = Vec3::from(direction);
    let height = along.dot(Vec3::from(plane.normal()?)).abs();
    let offset = height * taper_angle.tan().abs();
    if !offset.is_finite() || offset <= 1e-12 {
        return None;
    }
    let mut vertices = Vec::with_capacity(profile.len() + 1);
    for (piece, forward) in profile.iter().zip(&senses) {
        let position = piece.point_at(if *forward { 0.0 } else { 1.0 });
        let bulge = match piece {
            Curve2::Line(_) => 0.0,
            Curve2::Arc(arc) => {
                let signed = arc.sweep() * if *forward { 1.0 } else { -1.0 };
                (signed * 0.25).tan()
            }
            _ => return None,
        };
        vertices.push(ocs_geom::geom2d::PolylineVertex { position, bulge });
    }
    vertices.push(ocs_geom::geom2d::PolylineVertex {
        position: profile
            .last()?
            .point_at(if *senses.last()? { 1.0 } else { 0.0 }),
        bulge: 0.0,
    });
    let source = ocs_geom::geom2d::Polyline {
        vertices,
        closed: false,
    };
    let first = profile.first()?;
    let forward = *senses.first()?;
    let point = first.point_at(0.5);
    let tangent = first.tangent_at(0.5);
    let tangent = if forward {
        tangent
    } else {
        [-tangent[0], -tangent[1]]
    };
    let tangent_length = tangent[0].hypot(tangent[1]);
    if tangent_length <= 1e-12 {
        return None;
    }
    let left = [-tangent[1] / tangent_length, tangent[0] / tangent_length];
    let side_sign = if taper_angle > 0.0 { 1.0 } else { -1.0 };
    let side = [
        point[0] + left[0] * offset * side_sign,
        point[1] + left[1] * offset * side_sign,
    ];
    let top = ocs_geom::geom2d::offset_polyline(&source, offset, side)
        .into_iter()
        .filter(|polyline| !polyline.closed)
        .max_by(|first, second| {
            let length = |polyline: &ocs_geom::geom2d::Polyline| {
                Curve2::Polyline(polyline.clone())
                    .segments()
                    .iter()
                    .map(Curve2::length)
                    .sum::<f64>()
            };
            length(first).total_cmp(&length(second))
        })?;
    let top_profile = Curve2::Polyline(top).segments();
    if top_profile.len() != profile.len() {
        return None;
    }
    let far = Plane::from_axes(
        (Vec3::from(plane.origin) + along).to_array(),
        plane.x_axis,
        plane.y_axis,
    );
    ruled_open_surface(plane, profile, far, &top_profile)
}

fn ruled_open_surface(
    base_plane: Plane,
    base_profile: &[Curve2],
    top_plane: Plane,
    top_profile: &[Curve2],
) -> Option<Body> {
    if base_profile.is_empty() || base_profile.len() != top_profile.len() {
        return None;
    }
    let base_senses = path_senses(base_profile)?;
    let top_senses = path_senses(top_profile)?;
    let chain_points = |profile: &[Curve2], senses: &[bool]| {
        let mut points = profile
            .iter()
            .zip(senses)
            .map(|(piece, forward)| piece.point_at(if *forward { 0.0 } else { 1.0 }))
            .collect::<Vec<_>>();
        points.push(
            profile
                .last()?
                .point_at(if *senses.last()? { 1.0 } else { 0.0 }),
        );
        Some(points)
    };
    let base_points = chain_points(base_profile, &base_senses)?;
    let top_points = chain_points(top_profile, &top_senses)?;

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
    let base_vertices = base_points
        .iter()
        .map(|point| add_vertex(&mut body, base_plane.point_at(*point)))
        .collect::<Vec<_>>();
    let top_vertices = top_points
        .iter()
        .map(|point| add_vertex(&mut body, top_plane.point_at(*point)))
        .collect::<Vec<_>>();
    let mut base_edges = Vec::with_capacity(base_profile.len());
    let mut top_edges = Vec::with_capacity(top_profile.len());
    for index in 0..base_profile.len() {
        let (base_from, base_to) = if base_senses[index] {
            (base_vertices[index], base_vertices[index + 1])
        } else {
            (base_vertices[index + 1], base_vertices[index])
        };
        let (top_from, top_to) = if top_senses[index] {
            (top_vertices[index], top_vertices[index + 1])
        } else {
            (top_vertices[index + 1], top_vertices[index])
        };
        base_edges.push(add_profile_edge(
            &mut body,
            &base_plane,
            &base_profile[index],
            base_from,
            base_to,
        )?);
        top_edges.push(add_profile_edge(
            &mut body,
            &top_plane,
            &top_profile[index],
            top_from,
            top_to,
        )?);
    }
    let rails = (0..base_vertices.len())
        .map(|index| add_line_edge(&mut body, base_vertices[index], top_vertices[index]))
        .collect::<Option<Vec<_>>>()?;

    for index in 0..base_profile.len() {
        let base = RationalCurve2::from_curve(&base_profile[index])?;
        let base = if base_senses[index] {
            base
        } else {
            base.reversed()
        }
        .lifted(&base_plane);
        let top = RationalCurve2::from_curve(&top_profile[index])?;
        let top = if top_senses[index] {
            top
        } else {
            top.reversed()
        }
        .lifted(&top_plane);
        if !base.compatible_with(&top) {
            return None;
        }
        let surface = Surface::Nurbs(base.ruled_to(&top)?);
        let on = surface.point_at(0.5, 0.5);
        let expected = Vec3::from(surface.normal_at(0.5, 0.5)?);
        let surface = body.surfaces.insert(surface);
        let forward = face_sense(body.surfaces.get(surface)?, on, expected)?;
        add_wall_with_pcurves(
            &mut body,
            shell,
            surface,
            forward,
            &[
                (base_edges[index], base_senses[index]),
                (rails[index + 1], true),
                (top_edges[index], !top_senses[index]),
                (rails[index], false),
            ],
            &[
                ([0.0, 0.0], [1.0, 0.0]),
                ([1.0, 0.0], [1.0, 1.0]),
                ([1.0, 1.0], [0.0, 1.0]),
                ([0.0, 1.0], [0.0, 0.0]),
            ],
        )?;
    }
    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    body.validate().is_empty().then_some(body)
}

/// Extrudes an outer loop and removes each following loop as a hole.
pub fn extrude_region(
    plane: Plane,
    profiles: &[Vec<Curve2>],
    direction: [f64; 3],
) -> Option<Body> {
    extrusion_region(plane, profiles, direction, |profile, _| extrude(plane, profile, direction))
}

#[cfg(feature = "offset")]
fn complete_circular_profile(profile: &[Curve2]) -> Option<([f64; 2], f64)> {
    let first = profile.first()?;
    let (centre, radius) = match first {
        Curve2::Circle(circle) if profile.len() == 1 => return Some((circle.centre, circle.radius)),
        Curve2::Arc(arc) => (arc.centre, arc.radius),
        _ => return None,
    };
    let mut sweep = 0.0;
    for curve in profile {
        let Curve2::Arc(arc) = curve else { return None; };
        if (arc.centre[0] - centre[0]).hypot(arc.centre[1] - centre[1]) > 1e-9
            || (arc.radius - radius).abs() > 1e-9 { return None; }
        sweep += arc.sweep();
    }
    ((sweep - TAU).abs() < 1e-9 && profile_senses(profile).is_some()).then_some((centre, radius))
}

/// Extrudes all region loops as lateral sheets, without Boolean operations.
pub fn extrude_surface_region(plane: Plane, profiles: &[Vec<Curve2>], direction: [f64; 3]) -> Option<Body> {
    let mut result = Body::new();
    for profile in profiles {
        let pieces = super::extrusion_profile_pieces(profile);
        append_body(&mut result, &extrude_surface(plane, &pieces, direction)?)?;
    }
    (!result.roots.is_empty() && result.validate().is_empty()).then_some(result)
}

/// Tapers the outer wall inward and hole walls outward for a positive angle.
#[cfg(feature = "offset")]
pub fn extrude_region_tapered(plane: Plane, profiles: &[Vec<Curve2>], direction: [f64; 3], angle: f64) -> Option<Body> {
    extrusion_region(plane, profiles, direction, |profile, hole| {
        extrude_tapered(plane, profile, direction, if hole { -angle } else { angle })
    })
}

/// Tapered lateral sheets for all loops of a bounded region.
#[cfg(feature = "offset")]
pub fn extrude_surface_region_tapered(plane: Plane, profiles: &[Vec<Curve2>], direction: [f64; 3], angle: f64) -> Option<Body> {
    let mut result = Body::new();
    for (index, profile) in profiles.iter().enumerate() {
        let pieces = super::extrusion_profile_pieces(profile);
        append_body(&mut result, &extrude_surface_tapered(plane, &pieces, direction, if index == 0 { angle } else { -angle })?)?;
    }
    (!result.roots.is_empty() && result.validate().is_empty()).then_some(result)
}

fn extrusion_region(
    plane: Plane,
    profiles: &[Vec<Curve2>],
    direction: [f64; 3],
    build: impl Fn(&[Curve2], bool) -> Option<Body>,
) -> Option<Body> {
    let profiles = profiles.iter().map(|ring| super::extrusion_profile_pieces(ring)).collect::<Vec<_>>();
    valid_region_loops(&profiles)?;
    let mut result = build(profiles.first()?, false)?;
    if profiles.len() == 1 {
        return Some(result);
    }
    let cap_planes = [plane, Plane::from_axes((Vec3::from(plane.origin) + Vec3::from(direction)).to_array(), plane.x_axis, plane.y_axis)];
    let caps = cap_planes.iter().map(|plane| extrusion_cap(&result, plane)).collect::<Option<Vec<_>>>()?;
    let shell = result.faces.get(caps[0])?.owner;
    for hole in profiles.iter().skip(1) {
        let cutter = build(hole, true)?;
        let hole_caps = cap_planes.iter().map(|plane| extrusion_cap(&cutter, plane)).collect::<Option<Vec<_>>>()?;
        for face in cutter.face_keys() {
            super::boolean::copy_face(&mut result, &cutter, face, shell, true).ok()?;
            if let Some(index) = hole_caps.iter().position(|cap| *cap == face) {
                let copied = *result.shells.get(shell)?.faces.last()?;
                let loops = result.faces.get(copied)?.loops.clone();
                for ring in &loops {
                    let coedges = result.loops.get(*ring)?.coedges.clone();
                    result.loops.get_mut(*ring)?.owner = caps[index];
                    for coedge in coedges {
                        result.coedges.get_mut(coedge)?.pcurve = None;
                    }
                }
                result.faces.get_mut(caps[index])?.loops.extend(loops);
                result.shells.get_mut(shell)?.faces.retain(|key| *key != copied);
                result.faces.remove(copied)?;
            }
        }
    }
    // The top can collapse before the base does when holes widen under draft.
    for cap in caps {
        valid_region_loops(&super::planar_face_profile(&result, cap)?.loops)?;
    }
    (!result.roots.is_empty() && result.validate().is_empty()
        && result.edges.iter().all(|(_, edge)| edge.coedges.len() == 2)).then_some(result)
}

fn extrusion_cap(body: &Body, plane: &Plane) -> Option<FaceKey> {
    let normal = Vec3::from(plane.normal()?);
    let tolerance = super::operation_tolerance(&[body]);
    body.face_keys().find(|key| {
        let Some(face) = body.faces.get(*key) else { return false; };
        let Some(Surface::Plane(candidate)) = body.surfaces.get(face.surface) else { return false; };
        candidate.normal().is_some_and(|candidate_normal| normal.dot(Vec3::from(candidate_normal)).abs() > 1.0 - 1e-9)
            && plane.distance_to(candidate.origin).is_some_and(|gap| gap.abs() <= tolerance)
    })
}

fn valid_region_loops(profiles: &[Vec<Curve2>]) -> Option<()> {
    let outer = profiles.first()?;
    if outer.is_empty() { return None; }
    let tolerance = ocs_geom::geom2d::Tolerance::new(1e-9);
    for (index, hole) in profiles.iter().enumerate().skip(1) {
        if hole.is_empty() || !ocs_geom::geom2d::contains(outer, hole[0].point_at(0.5), tolerance) {
            return None;
        }
        for previous in &profiles[..index] {
            if hole.iter().any(|curve| previous.iter().any(|other| {
                !ocs_geom::geom2d::intersect(curve, other, tolerance).is_empty()
                    || ocs_geom::geom2d::distance_to(other, curve.point_at(0.5)) <= tolerance.linear()
            })) {
                return None;
            }
        }
        for previous in &profiles[1..index] {
            if ocs_geom::geom2d::contains(previous, hole[0].point_at(0.5), tolerance)
                || ocs_geom::geom2d::contains(hole, previous[0].point_at(0.5), tolerance) {
                return None;
            }
        }
    }
    Some(())
}

/// Sweeps a profile along a connected chain of straight and circular pieces.
pub fn sweep_along(
    mut profile_plane: Plane,
    profile: &[Curve2],
    path_plane: Plane,
    path: &[Curve2],
) -> Option<Body> {
    let senses = path_senses(path)?;
    if let Some(body) = mitered_rectangular_line_sweep(
        profile_plane,
        profile,
        path_plane,
        path,
        &senses,
    ) {
        return Some(body);
    }
    let mut result: Option<Body> = None;
    let mut previous_tangent: Option<Vec3> = None;
    for (piece, forward) in path.iter().zip(senses) {
        let start_tangent = path_tangent(&path_plane, piece, forward, false)?;
        if let Some(previous) = previous_tangent {
            let axis = Vec3::from(path_plane.normal()?);
            let angle = axis.dot(previous.cross(start_tangent)).atan2(previous.dot(start_tangent));
            if angle.abs() > 1e-10 {
                let joint = path_plane.point_at(piece.point_at(if forward { 0.0 } else { 1.0 }));
                profile_plane = turned_plane(profile_plane, Vec3::from(joint), axis, angle);
            }
        }
        let segment = match piece {
            Curve2::Line(line) => {
                let start = path_plane.point_at(if forward { line.start } else { line.end });
                let end = path_plane.point_at(if forward { line.end } else { line.start });
                let direction = Vec3::from(end) - Vec3::from(start);
                let body = extrude(profile_plane, profile, direction.to_array())?;
                profile_plane.origin = (Vec3::from(profile_plane.origin) + direction).to_array();
                body
            }
            Curve2::Arc(arc) => {
                let pivot = path_plane.point_at(arc.centre);
                let axis = path_plane.normal()?;
                let angle = arc.sweep() * if forward { 1.0 } else { -1.0 };
                let turn = Turn::new(&profile_plane, profile, pivot, axis, angle)?;
                let body = revolve(profile_plane, profile, pivot, axis, angle)?;
                profile_plane = turn.far_plane(&profile_plane);
                body
            }
            _ => return None,
        };
        previous_tangent = Some(path_tangent(&path_plane, piece, forward, true)?);
        result = Some(match result {
            None => segment,
            Some(previous) => {
                let tolerance = super::operation_tolerance(&[&previous, &segment]);
                super::boolean::combine(
                    previous,
                    segment,
                    super::boolean::Operation::Union,
                    tolerance,
                )
                .ok()?
            }
        });
    }
    result.filter(|body| !body.roots.is_empty() && body.validate().is_empty())
}

/// Sweeps a closed profile along a connected three-dimensional line path.
///
/// The path start is translated to the profile centre. At each non-collinear
/// corner the profile frame follows the minimal turn between the adjacent
/// segments, extending the planar path behaviour without projecting a 3D
/// polyline onto an arbitrary plane.
pub fn sweep_along_polyline3d(
    mut profile_plane: Plane,
    profile: &[Curve2],
    path: &[[f64; 3]],
) -> Option<Body> {
    if path.len() < 2 || profile_senses(profile).is_none() {
        return None;
    }
    let profile_center = profile
        .iter()
        .map(|piece| Vec3::from(profile_plane.point_at(piece.point_at(0.0))))
        .fold(Vec3::ZERO, |sum, point| sum + point)
        / profile.len() as f64;
    let shift = profile_center - Vec3::from(path[0]);
    let points = path
        .iter()
        .map(|point| Vec3::from(*point) + shift)
        .collect::<Vec<_>>();
    if points.iter().any(|point| !point.length().is_finite()) {
        return None;
    }

    let mut result: Option<Body> = None;
    let mut previous_direction: Option<Vec3> = None;
    for (index, pair) in points.windows(2).enumerate() {
        let direction = pair[1] - pair[0];
        let tangent = direction.normalize()?;
        if let Some(previous) = previous_direction {
            let cross = previous.cross(tangent);
            let cross_length = cross.length();
            let dot = previous.dot(tangent).clamp(-1.0, 1.0);
            if cross_length <= 1e-12 {
                if dot <= 0.0 {
                    return None;
                }
            } else {
                profile_plane = turned_plane(
                    profile_plane,
                    points[index],
                    cross / cross_length,
                    cross_length.atan2(dot),
                );
            }
        }
        let segment = extrude(profile_plane, profile, direction.to_array())?;
        profile_plane.origin = (Vec3::from(profile_plane.origin) + direction).to_array();
        result = Some(match result {
            None => segment,
            Some(previous) => {
                let tolerance = super::operation_tolerance(&[&previous, &segment]);
                super::boolean::combine(
                    previous,
                    segment,
                    super::boolean::Operation::Union,
                    tolerance,
                )
                .ok()?
            }
        });
        previous_direction = Some(tangent);
    }
    result.filter(|body| !body.roots.is_empty() && body.validate().is_empty())
}

/// Sweeps a polygon profile while rotating and scaling it from the beginning
/// to the end of an open planar path.
///
/// Zero rotation and twist with unit scale retain the analytic sweep path.
/// Deformed profiles are rebuilt as compatible section rings, so every
/// exposed history value changes the resulting B-rep rather than remaining
/// display-only metadata.
pub fn sweep_along_deformed(
    profile_plane: Plane,
    profile: &[Curve2],
    path_plane: Plane,
    path: &[Curve2],
    profile_rotation: f64,
    twist_angle: f64,
    scale_factor: f64,
) -> Option<Body> {
    if !profile_rotation.is_finite()
        || !twist_angle.is_finite()
        || !scale_factor.is_finite()
        || scale_factor <= 1e-9
    {
        return None;
    }
    if profile_rotation.abs() <= 1e-12
        && twist_angle.abs() <= 1e-12
        && (scale_factor - 1.0).abs() <= 1e-12
    {
        return sweep_along(profile_plane, profile, path_plane, path);
    }
    if profile.iter().any(|piece| !matches!(piece, Curve2::Line(_))) {
        return None;
    }
    let senses = path_senses(path)?;
    let path_normal = Vec3::from(path_plane.normal()?).normalize()?;
    let stations = deformation_stations(&path_plane, path, &senses, twist_angle)?;
    if stations.len() < 2 || (stations.last()?.point - stations[0].point).length() <= 1e-9 {
        return None;
    }

    let profile_senses = profile_senses(profile)?;
    let start = &stations[0];
    let start_left = path_normal.cross(start.tangent).normalize()?;
    let profile_coordinates = profile
        .iter()
        .zip(profile_senses)
        .map(|(piece, forward)| {
            let point = Vec3::from(
                profile_plane.point_at(piece.point_at(if forward { 0.0 } else { 1.0 })),
            );
            let delta = point - start.point;
            if delta.dot(start.tangent).abs() > 1e-8 * delta.length().max(1.0) {
                return None;
            }
            Some([delta.dot(start_left), delta.dot(path_normal)])
        })
        .collect::<Option<Vec<_>>>()?;

    let sections = stations
        .iter()
        .map(|station| {
            let left = path_normal.cross(station.tangent).normalize()?;
            let angle = profile_rotation + twist_angle * station.fraction;
            let section_scale = 1.0 + (scale_factor - 1.0) * station.fraction;
            if !section_scale.is_finite() || section_scale <= 1e-9 {
                return None;
            }
            let (sin, cos) = angle.sin_cos();
            let coordinates = profile_coordinates
                .iter()
                .map(|coordinate| {
                    let across = (coordinate[0] * cos - coordinate[1] * sin)
                        * section_scale
                        * station.miter;
                    let height = (coordinate[0] * sin + coordinate[1] * cos) * section_scale;
                    [across, height]
                })
                .collect::<Vec<_>>();
            let pieces = (0..coordinates.len())
                .map(|index| {
                    Curve2::Line(ocs_geom::geom2d::Line {
                        start: coordinates[index],
                        end: coordinates[(index + 1) % coordinates.len()],
                    })
                })
                .collect::<Vec<_>>();
            Some((
                Plane::from_axes(station.point.to_array(), left.to_array(), path_normal.to_array()),
                pieces,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    super::loft::polygon_loft_ordered(&sections)
}

struct DeformationStation {
    point: Vec3,
    tangent: Vec3,
    fraction: f64,
    miter: f64,
}

fn deformation_stations(
    plane: &Plane,
    path: &[Curve2],
    senses: &[bool],
    twist_angle: f64,
) -> Option<Vec<DeformationStation>> {
    if path.len() != senses.len() || path.is_empty() || !twist_angle.is_finite() {
        return None;
    }
    let total_length = path.iter().map(Curve2::length).sum::<f64>();
    if !total_length.is_finite() || total_length <= 1e-12 {
        return None;
    }
    let angle_step = std::f64::consts::PI / 18.0;
    if (twist_angle.abs() / angle_step).ceil() > 4096.0 {
        return None;
    }
    let first_point = path[0].point_at(if senses[0] { 0.0 } else { 1.0 });
    let last_point = path.last()?.point_at(if *senses.last()? { 1.0 } else { 0.0 });
    if meets(first_point, last_point) {
        return None;
    }

    let mut stations = Vec::new();
    let mut traversed = 0.0;
    for (index, (piece, forward)) in path.iter().zip(senses).enumerate() {
        let piece_length = piece.length();
        if !piece_length.is_finite() || piece_length <= 1e-12 {
            return None;
        }
        let path_subdivisions = match piece {
            Curve2::Line(_) => 1,
            Curve2::Arc(value) => (value.sweep().abs() / (std::f64::consts::PI / 18.0))
                .ceil()
                .max(1.0) as usize,
            _ => return None,
        };
        let twist_subdivisions = (twist_angle.abs() * piece_length / total_length / angle_step)
            .ceil()
            .max(1.0) as usize;
        let subdivisions = path_subdivisions.max(twist_subdivisions);
        for step in 0..=subdivisions {
            if index > 0 && step == 0 {
                continue;
            }
            let local = step as f64 / subdivisions as f64;
            let parameter = if *forward { local } else { 1.0 - local };
            let point = Vec3::from(plane.point_at(piece.point_at(parameter)));
            let mut tangent = Vec3::from(plane.vector_at(piece.tangent_at(parameter)));
            if !*forward {
                tangent = -tangent;
            }
            tangent = tangent.normalize()?;
            let mut miter = 1.0;
            if step == subdivisions && index + 1 < path.len() {
                let next = path_tangent(plane, &path[index + 1], senses[index + 1], false)?;
                let bisector = (tangent + next).normalize()?;
                let normal = Vec3::from(plane.normal()?).normalize()?;
                let left = normal.cross(bisector).normalize()?;
                let previous_left = normal.cross(tangent).normalize()?;
                let cosine = left.dot(previous_left);
                if cosine <= 1e-6 {
                    return None;
                }
                tangent = bisector;
                miter = 1.0 / cosine;
            }
            stations.push(DeformationStation {
                point,
                tangent,
                fraction: (traversed + piece_length * local) / total_length,
                miter,
            });
        }
        traversed += piece_length;
    }
    Some(stations)
}

/// Builds a rectangular sweep along a straight planar chain as one footprint.
///
/// Sweeping every run separately gives each run its own perpendicular end
/// cap. At a corner those caps overlap instead of meeting on the angle
/// bisector, so a later union has to recover the intended wall from
/// intersecting solids and may retain the old caps as internal seams. A
/// rectangular profile along straight runs has an exact, simpler answer: the
/// two signed parallel rails meet at their infinite-line intersections. Their
/// joined outline is extruded once, producing mitered corners and no
/// intermediate faces.
fn mitered_rectangular_line_sweep(
    profile_plane: Plane,
    profile: &[Curve2],
    path_plane: Plane,
    path: &[Curve2],
    senses: &[bool],
) -> Option<Body> {
    if profile.len() != 4
        || profile.iter().any(|piece| !matches!(piece, Curve2::Line(_)))
        || path.len() != senses.len()
        || path.iter().any(|piece| !matches!(piece, Curve2::Line(_)))
    {
        return None;
    }

    let mut path_points = Vec::with_capacity(path.len() + 1);
    for (index, (piece, forward)) in path.iter().zip(senses).enumerate() {
        let Curve2::Line(line) = piece else {
            return None;
        };
        let (start, end) = if *forward {
            (line.start, line.end)
        } else {
            (line.end, line.start)
        };
        if index == 0 {
            path_points.push(start);
        } else if !meets(*path_points.last()?, start) {
            return None;
        }
        if meets(start, end) {
            return None;
        }
        path_points.push(end);
    }
    if path_points.len() < 2 || meets(path_points[0], *path_points.last()?) {
        return None;
    }

    let first_direction = unit2(sub2(path_points[1], path_points[0]))?;
    let path_normal = Vec3::from(path_plane.normal()?).normalize()?;
    let tangent = Vec3::from(path_plane.vector_at(first_direction)).normalize()?;
    let left = path_normal.cross(tangent).normalize()?;
    let path_start = Vec3::from(path_plane.point_at(path_points[0]));
    let profile_senses = profile_senses(profile)?;
    let profile_corners = profile
        .iter()
        .zip(profile_senses)
        .map(|(piece, forward)| {
            Vec3::from(profile_plane.point_at(piece.point_at(if forward { 0.0 } else { 1.0 })))
        })
        .collect::<Vec<_>>();
    let scale = profile_corners
        .iter()
        .map(|point| (*point - path_start).length())
        .fold(1.0_f64, f64::max);
    let tolerance = scale * 1e-9;
    let mut across_min = f64::INFINITY;
    let mut across_max = f64::NEG_INFINITY;
    let mut height_min = f64::INFINITY;
    let mut height_max = f64::NEG_INFINITY;
    let mut coordinates = Vec::with_capacity(profile_corners.len());
    for corner in profile_corners {
        let delta = corner - path_start;
        if delta.dot(tangent).abs() > tolerance {
            return None;
        }
        let across = delta.dot(left);
        let height = delta.dot(path_normal);
        across_min = across_min.min(across);
        across_max = across_max.max(across);
        height_min = height_min.min(height);
        height_max = height_max.max(height);
        coordinates.push((across, height));
    }
    if across_max - across_min <= tolerance || height_max - height_min <= tolerance {
        return None;
    }
    if coordinates.iter().any(|(across, height)| {
        !near_either(*across, across_min, across_max, tolerance)
            || !near_either(*height, height_min, height_max, tolerance)
    }) {
        return None;
    }

    let high_rail = offset_line_rail(&path_points, across_max)?;
    let low_rail = offset_line_rail(&path_points, across_min)?;
    let mut outline_points = high_rail;
    outline_points.extend(low_rail.into_iter().rev());
    let mut outline = Vec::with_capacity(outline_points.len());
    for index in 0..outline_points.len() {
        let start = outline_points[index];
        let end = outline_points[(index + 1) % outline_points.len()];
        if meets(start, end) {
            return None;
        }
        outline.push(Curve2::Line(ocs_geom::geom2d::Line { start, end }));
    }

    let base_origin = Vec3::from(path_plane.origin) + path_normal * height_min;
    let base_plane = Plane::from_axes(
        base_origin.to_array(),
        path_plane.x_axis,
        path_plane.y_axis,
    );
    extrude(
        base_plane,
        &outline,
        (path_normal * (height_max - height_min)).to_array(),
    )
}

fn offset_line_rail(points: &[[f64; 2]], distance: f64) -> Option<Vec<[f64; 2]>> {
    let directions = points
        .windows(2)
        .map(|pair| unit2(sub2(pair[1], pair[0])))
        .collect::<Option<Vec<_>>>()?;
    let offset_point = |point: [f64; 2], direction: [f64; 2]| {
        [
            point[0] - direction[1] * distance,
            point[1] + direction[0] * distance,
        ]
    };
    let mut rail = Vec::with_capacity(points.len());
    rail.push(offset_point(points[0], directions[0]));
    for index in 1..points.len() - 1 {
        let previous = directions[index - 1];
        let next = directions[index];
        let previous_point = offset_point(points[index], previous);
        let next_point = offset_point(points[index], next);
        let cross = cross2(previous, next);
        let joint = if cross.abs() <= 1e-12 {
            if dot2(previous, next) <= 0.0 {
                return None;
            }
            [
                (previous_point[0] + next_point[0]) * 0.5,
                (previous_point[1] + next_point[1]) * 0.5,
            ]
        } else {
            let along = cross2(sub2(next_point, previous_point), next) / cross;
            [
                previous_point[0] + previous[0] * along,
                previous_point[1] + previous[1] * along,
            ]
        };
        rail.push(joint);
    }
    rail.push(offset_point(*points.last()?, *directions.last()?));
    Some(rail)
}

fn sub2(first: [f64; 2], second: [f64; 2]) -> [f64; 2] {
    [first[0] - second[0], first[1] - second[1]]
}

fn unit2(vector: [f64; 2]) -> Option<[f64; 2]> {
    let length = vector[0].hypot(vector[1]);
    (length > 1e-12).then_some([vector[0] / length, vector[1] / length])
}

fn cross2(first: [f64; 2], second: [f64; 2]) -> f64 {
    first[0] * second[1] - first[1] * second[0]
}

fn dot2(first: [f64; 2], second: [f64; 2]) -> f64 {
    first[0] * second[0] + first[1] * second[1]
}

fn near_either(value: f64, first: f64, second: f64, tolerance: f64) -> bool {
    (value - first).abs() <= tolerance || (value - second).abs() <= tolerance
}

fn path_tangent(plane: &Plane, piece: &Curve2, forward: bool, at_end: bool) -> Option<Vec3> {
    let parameter = if at_end == forward { 1.0 } else { 0.0 };
    let tangent = Vec3::from(plane.vector_at(piece.tangent_at(parameter)));
    (tangent * if forward { 1.0 } else { -1.0 }).normalize()
}

fn turned_plane(plane: Plane, pivot: Vec3, axis: Vec3, angle: f64) -> Plane {
    let rotate = |vector: Vec3| {
        let (sin, cos) = angle.sin_cos();
        vector * cos + axis.cross(vector) * sin + axis * axis.dot(vector) * (1.0 - cos)
    };
    Plane::from_axes(
        (pivot + rotate(Vec3::from(plane.origin) - pivot)).to_array(),
        rotate(Vec3::from(plane.x_axis)).to_array(),
        rotate(Vec3::from(plane.y_axis)).to_array(),
    )
}

fn path_senses(path: &[Curve2]) -> Option<Vec<bool>> {
    if path.is_empty() {
        return None;
    }
    [true, false].into_iter().find_map(|first| {
        let mut senses = vec![first];
        let mut head = path[0].point_at(if first { 1.0 } else { 0.0 });
        for piece in &path[1..] {
            let forward = meets(head, piece.point_at(0.0));
            if !forward && !meets(head, piece.point_at(1.0)) {
                return None;
            }
            head = piece.point_at(if forward { 1.0 } else { 0.0 });
            senses.push(forward);
        }
        Some(senses)
    })
}

/// Whether each piece of a closed profile is written the way the loop runs.
///
/// The chain is followed by which ends meet, so a piece stored backwards is
/// recognised rather than rejected. The first piece has no predecessor to be
/// judged against, so both of its directions are tried and whichever closes
/// the loop is the answer.
///
/// `None` when the chain does not close either way. Whether the closed chain
/// encloses an area is a separate question: one full conic can be a valid
/// outline, while three backtracking lines can enclose nothing.
pub(super) fn profile_senses(profile: &[Curve2]) -> Option<Vec<bool>> {
    if profile.is_empty() {
        return None;
    }
    [true, false].into_iter().find_map(|first| {
        let mut senses = Vec::with_capacity(profile.len());
        senses.push(first);
        let mut head = profile[0].point_at(if first { 1.0 } else { 0.0 });
        for piece in &profile[1..] {
            let forwards = meets(head, piece.point_at(0.0));
            if !forwards && !meets(head, piece.point_at(1.0)) {
                return None;
            }
            head = piece.point_at(if forwards { 1.0 } else { 0.0 });
            senses.push(forwards);
        }
        // Back where it started, or it is not a loop.
        let start = profile[0].point_at(if first { 0.0 } else { 1.0 });
        meets(head, start).then_some(senses)
    })
}

fn meets(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-9
}

/// The edges of one copy of the profile, on `plane`.
///
/// An edge runs the way its own curve does, which for a piece written
/// backwards is against the loop — the same split ACIS makes between an edge
/// and the coedges that use it.
fn profile_edges(
    body: &mut Body,
    plane: &Plane,
    profile: &[Curve2],
    senses: &[bool],
    corners: &[VertexKey],
) -> Option<Vec<EdgeKey>> {
    let mut out = Vec::with_capacity(profile.len());
    for (index, piece) in profile.iter().enumerate() {
        let next = (index + 1) % profile.len();
        let (from, to) = if senses[index] {
            (corners[index], corners[next])
        } else {
            (corners[next], corners[index])
        };
        out.push(add_profile_edge(body, plane, piece, from, to)?);
    }
    Some(out)
}

/// A profile piece as a space curve on the plane, with the parameters its
/// ends sit at.
fn lift_piece(plane: &Plane, piece: &Curve2) -> Option<(Curve3, f64, f64)> {
    match piece {
        Curve2::Line(line) => {
            let start = plane.point_at(line.start);
            let end = plane.point_at(line.end);
            Some((
                Curve3::Line(Line3 {
                    origin: start,
                    direction: (Vec3::from(end) - Vec3::from(start)).to_array(),
                }),
                0.0,
                1.0,
            ))
        }
        Curve2::Arc(arc) => {
            let centre = plane.point_at(arc.centre);
            let frame = Plane::from_axes(centre, plane.x_axis, plane.y_axis);
            let start = ocs_geom::geom2d::normalize_angle(arc.start_angle);
            Some((
                Curve3::Circle(Circle3 {
                    plane: frame,
                    radius: arc.radius,
                }),
                start,
                start + arc.sweep(),
            ))
        }
        Curve2::Ellipse(arc) => {
            let centre = plane.point_at(arc.ellipse.centre);
            let frame = Plane::orthonormal(
                centre,
                plane.vector_at(arc.ellipse.major_axis),
                plane.normal()?,
            )?;
            Some((
                Curve3::Ellipse(Ellipse3 {
                    plane: frame,
                    major_radius: arc.ellipse.major_radius,
                    minor_radius: arc.ellipse.minor_radius,
                }),
                arc.start_parameter,
                arc.start_parameter + arc.sweep(),
            ))
        }
        Curve2::Nurbs(curve) => Some((
            Curve3::PlanarSpline {
                plane: *plane,
                curve: curve.clone(),
            },
            0.0,
            1.0,
        )),
        _ => None,
    }
}

/// The surface a profile piece sweeps into.
fn wall_surface(
    body: &mut Body,
    plane: &Plane,
    piece: &Curve2,
    along: Vec3,
) -> Option<(SurfaceKey, bool)> {
    let (surface, spline) = match piece {
        Curve2::Line(line) => {
            let start = Vec3::from(plane.point_at(line.start));
            let end = Vec3::from(plane.point_at(line.end));
            // The wall's own frame: along the run, and up the sweep.
            let normal = (end - start).cross(along).normalize()?;
            (
                Surface::Plane(Plane::orthonormal(
                    start.to_array(),
                    (end - start).to_array(),
                    normal.to_array(),
                )?),
                false,
            )
        }
        Curve2::Arc(arc) => {
            let centre = plane.point_at(arc.centre);
            (
                Surface::Cylinder(Cylinder {
                    base: Plane::orthonormal(centre, plane.x_axis, along.to_array())?,
                    radius: arc.radius,
                }),
                false,
            )
        }
        Curve2::Ellipse(_) | Curve2::Nurbs(_) => {
            let base = RationalCurve2::from_curve(piece)?.lifted(plane);
            let top = base.translated(along);
            (Surface::Nurbs(base.ruled_to(&top)?), true)
        }
        _ => return None,
    };
    Some((body.surfaces.insert(surface), spline))
}

/// Whether a face on `surface` runs with it or against it.
///
/// Asked of the geometry rather than worked out per surface kind. `on` is a
/// point of the face and `out` a direction leading out of the solid there;
/// every surface here reports a signed distance that grows on the side its
/// own parameters face, so stepping outwards and reading it settles the sense
/// for all five kinds at once.
///
/// The step is taken both ways and the difference used, so the answer does
/// not rest on `on` sitting exactly on the surface — which, at survey
/// coordinates, it will not.
fn face_sense(surface: &Surface, on: [f64; 3], out: Vec3) -> Option<bool> {
    if matches!(surface, Surface::Torus(_)) {
        // A horn or spindle torus has two parametrised sheets in the same
        // radial half-plane. Its global signed-distance field has a medial
        // cusp between them, so a symmetric distance probe can report zero
        // slope even though the local surface normal is unambiguous.
        let (u, v) = surface.parameters_at(on)?;
        let normal = Vec3::from(surface.normal_at(u, v)?);
        let slope = normal.dot(out);
        return (slope != 0.0 && slope.is_finite()).then_some(slope > 0.0);
    }
    if let Some(spline) = surface.spline() {
        let ((u0, u1), (v0, v1)) = spline.domain();
        let normal = Vec3::from(surface.normal_at((u0 + u1) * 0.5, (v0 + v1) * 0.5)?);
        let slope = normal.dot(out);
        return (slope != 0.0 && slope.is_finite()).then_some(slope > 0.0);
    }
    let step = out.normalize()? * 1e-6;
    let ahead = (Vec3::from(on) + step).to_array();
    let behind = (Vec3::from(on) - step).to_array();
    let slope = surface.distance_to(ahead) - surface.distance_to(behind);
    (slope != 0.0 && slope.is_finite()).then_some(slope > 0.0)
}

/// Which way a profile piece's own outward direction points, in space.
///
/// The solid lies inside the loop, so out of it is to the right of the way
/// the loop runs — and the loop runs along the piece or against it depending
/// on how the piece was written.
fn piece_outward(plane: &Plane, piece: &Curve2, forwards: bool, counter_clockwise: bool) -> Vec3 {
    let tangent = piece.tangent_at(0.5);
    let along = if forwards { 1.0 } else { -1.0 };
    let right = if counter_clockwise { along } else { -along };
    Vec3::from(plane.vector_at([tangent[1] * right, -tangent[0] * right]))
}

/// A loop's circuit, turned round when the solid's outward direction is the
/// other one.
///
/// Reversing a loop means walking it backwards *and* running each edge the
/// other way, which is what keeps every edge shared by two coedges of
/// opposite sense — the property that makes the result a solid.
fn reorder(circuit: Vec<(EdgeKey, bool)>, outward: bool) -> Vec<(EdgeKey, bool)> {
    if outward {
        return circuit;
    }
    let mut turned = circuit;
    turned.reverse();
    for step in &mut turned {
        step.1 = !step.1;
    }
    turned
}

fn reorder_with_pcurves(
    mut circuit: Vec<(EdgeKey, bool)>,
    mut pcurves: Vec<([f64; 2], [f64; 2])>,
    outward: bool,
) -> (Vec<(EdgeKey, bool)>, Vec<([f64; 2], [f64; 2])>) {
    if !outward {
        circuit.reverse();
        pcurves.reverse();
        for step in &mut circuit {
            step.1 = !step.1;
        }
        for (start, end) in &mut pcurves {
            std::mem::swap(start, end);
        }
    }
    (circuit, pcurves)
}

fn add_vertex(body: &mut Body, point: [f64; 3]) -> VertexKey {
    body.vertices.insert(Vertex {
        point,
        provenance: Provenance::Synthesized,
    })
}

fn add_line_edge(body: &mut Body, from: VertexKey, to: VertexKey) -> Option<EdgeKey> {
    let start = body.vertices.get(from)?.point;
    let end = body.vertices.get(to)?.point;
    let curve = body.curves.insert(Curve3::Line(Line3 {
        origin: start,
        direction: (Vec3::from(end) - Vec3::from(start)).to_array(),
    }));
    Some(body.edges.insert(Edge {
        curve,
        start_parameter: 0.0,
        end_parameter: 1.0,
        start: from,
        end: to,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    }))
}

/// One end of the sweep: a planar face bounded by that copy of the profile.
///
/// `forward` is whether the face agrees with the shared plane's normal — the
/// near cap and the far one never both do. `along_profile` is whether its
/// loop runs the profile's own way; that is a separate question, because a
/// profile wound clockwise already traverses backwards.
fn add_cap(
    body: &mut Body,
    shell: ShellKey,
    plane: Plane,
    edges: &[EdgeKey],
    senses: &[bool],
    forward: bool,
    along_profile: bool,
) -> Option<FaceKey> {
    let surface = body.surfaces.insert(Surface::Plane(plane));
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
    // Running the profile backwards means walking its edges in reverse and
    // traversing each against the way the loop would — which is also what
    // makes each profile edge shared by a cap and a wall running it opposite
    // ways.
    let mut order: Vec<(EdgeKey, bool)> = edges
        .iter()
        .zip(senses)
        .map(|(edge, sense)| (*edge, *sense == along_profile))
        .collect();
    if !along_profile {
        order.reverse();
    }
    let mut coedges = Vec::with_capacity(order.len());
    for (edge, sense) in order {
        coedges.push(add_coedge(body, ring, edge, sense)?);
    }
    body.loops.get_mut(ring)?.coedges = coedges;
    body.faces.get_mut(face)?.loops = vec![ring];
    body.shells.get_mut(shell)?.faces.push(face);
    Some(face)
}

/// One side wall, from the edges bounding it.
fn add_wall(
    body: &mut Body,
    shell: ShellKey,
    surface: SurfaceKey,
    forward: bool,
    edges: &[(EdgeKey, bool)],
) -> Option<FaceKey> {
    let face = add_face(body, shell, surface, forward);
    add_ring(body, face, edges)?;
    body.shells.get_mut(shell)?.faces.push(face);
    Some(face)
}

fn add_wall_with_pcurves(
    body: &mut Body,
    shell: ShellKey,
    surface: SurfaceKey,
    forward: bool,
    edges: &[(EdgeKey, bool)],
    pcurves: &[([f64; 2], [f64; 2])],
) -> Option<FaceKey> {
    if edges.len() != pcurves.len() {
        return None;
    }
    let face = add_face(body, shell, surface, forward);
    let ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let mut coedges = Vec::with_capacity(edges.len());
    for ((edge, sense), (start, end)) in edges.iter().zip(pcurves) {
        let coedge = body.coedges.insert(Coedge {
            edge: *edge,
            forward: *sense,
            pcurve: Some(Curve2::Line(ocs_geom::geom2d::Line {
                start: *start,
                end: *end,
            })),
            owner: ring,
            provenance: Provenance::Synthesized,
        });
        body.edges.get_mut(*edge)?.coedges.push(coedge);
        coedges.push(coedge);
    }
    body.loops.get_mut(ring)?.coedges = coedges;
    body.faces.get_mut(face)?.loops = vec![ring];
    body.shells.get_mut(shell)?.faces.push(face);
    Some(face)
}

/// A face with no loops on it yet.
fn add_face(body: &mut Body, shell: ShellKey, surface: SurfaceKey, forward: bool) -> FaceKey {
    body.faces.insert(Face {
        surface,
        forward,
        loops: Vec::new(),
        owner: shell,
        provenance: Provenance::Synthesized,
    })
}

/// Adds one loop to a face, from the edges and senses that walk it.
fn add_ring(body: &mut Body, face: FaceKey, edges: &[(EdgeKey, bool)]) -> Option<LoopKey> {
    let ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let mut coedges = Vec::with_capacity(edges.len());
    for (edge, forward) in edges {
        coedges.push(add_coedge(body, ring, *edge, *forward)?);
    }
    body.loops.get_mut(ring)?.coedges = coedges;
    body.faces.get_mut(face)?.loops.push(ring);
    Some(ring)
}

fn add_ring_with_pcurves(
    body: &mut Body,
    face: FaceKey,
    edges: &[(EdgeKey, bool)],
    pcurves: &[([f64; 2], [f64; 2])],
) -> Option<LoopKey> {
    if edges.len() != pcurves.len() {
        return None;
    }
    let ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let mut coedges = Vec::with_capacity(edges.len());
    for ((edge, forward), (start, end)) in edges.iter().zip(pcurves) {
        let coedge = body.coedges.insert(Coedge {
            edge: *edge,
            forward: *forward,
            pcurve: Some(Curve2::Line(ocs_geom::geom2d::Line {
                start: *start,
                end: *end,
            })),
            owner: ring,
            provenance: Provenance::Synthesized,
        });
        body.edges.get_mut(*edge)?.coedges.push(coedge);
        coedges.push(coedge);
    }
    body.loops.get_mut(ring)?.coedges = coedges;
    body.faces.get_mut(face)?.loops.push(ring);
    Some(ring)
}

fn add_coedge(
    body: &mut Body,
    ring: LoopKey,
    edge: EdgeKey,
    forward: bool,
) -> Option<CoedgeKey> {
    let coedge = body.coedges.insert(Coedge {
        edge,
        forward,
        pcurve: None,
        owner: ring,
        provenance: Provenance::Synthesized,
    });
    body.edges.get_mut(edge)?.coedges.push(coedge);
    Some(coedge)
}

// ── Revolution ──────────────────────────────────────────────────────────

/// How far from the axis a point is, and how far along it.
///
/// The half plane every revolution is really described in: turning is what
/// the third coordinate would have been, so a profile reduces to a curve in
/// `(radius, height)` and each of its pieces to the surface that curve
/// sweeps.
#[derive(Debug, Clone, Copy)]
struct Station {
    radius: f64,
    height: f64,
}

#[derive(Clone)]
enum PieceSurface {
    CollapsedAxis,
    Surface(Surface, bool),
    Unsupported,
}

#[derive(Clone, Copy)]
struct RevolvedArcParameters {
    around: f64,
    start: f64,
    end: f64,
}

#[derive(Clone, Copy, Default)]
struct RadialReach {
    negative: bool,
    positive: bool,
}

impl RadialReach {
    fn from_bounds(low: f64, high: f64, tolerance: f64) -> Option<Self> {
        if !low.is_finite() || !high.is_finite() || low > high {
            return None;
        }
        Some(Self {
            negative: low < -tolerance,
            positive: high > tolerance,
        })
    }

    fn include(&mut self, other: Self) {
        self.negative |= other.negative;
        self.positive |= other.positive;
    }

    fn collapsed(self) -> bool {
        !self.negative && !self.positive
    }

    fn straddles(self) -> bool {
        self.negative && self.positive
    }
}

#[derive(Clone, Copy)]
struct ProjectionBounds {
    low: f64,
    high: f64,
}

impl ProjectionBounds {
    fn new(first: f64, second: f64) -> Option<Self> {
        if !first.is_finite() || !second.is_finite() {
            return None;
        }
        Some(Self {
            low: first.min(second),
            high: first.max(second),
        })
    }

    fn include(&mut self, other: Self) {
        self.low = self.low.min(other.low);
        self.high = self.high.max(other.high);
    }

    fn magnitude(self) -> f64 {
        self.low.abs().max(self.high.abs())
    }

    fn span(self) -> f64 {
        self.high - self.low
    }
}

/// The frame a revolution turns in.
struct Turn {
    /// A point on the axis.
    pivot: Vec3,
    /// Unit vector along the axis. A negative angle is folded into this, so
    /// the turn always runs the right-hand way about it.
    axis: Vec3,
    /// The profile's own side of the axis, in the profile plane. Zero angle.
    radial: Vec3,
    /// Whether turning leads the way the profile plane's normal points. The
    /// revolution's answer to which way the sweep goes.
    spin: bool,
    /// How far round, in `(0, TAU]`.
    angle: f64,
    /// Whether that is the whole way.
    full: bool,
    /// What counts as being on the axis, scaled to the profile's own size.
    tolerance: f64,
}

impl Turn {
    /// Where a point of the profile plane sits in the half plane.
    fn station(&self, plane: &Plane, uv: [f64; 2]) -> Station {
        let offset = Vec3::from(plane.point_at(uv)) - self.pivot;
        Station {
            radius: offset.dot(self.radial),
            height: offset.dot(self.axis),
        }
    }

    /// The frame the circle at `height` is measured in: zero angle on the
    /// profile, and the axis as its normal.
    fn circle(&self, height: f64) -> Option<Plane> {
        Plane::orthonormal(
            (self.pivot + self.axis * height).to_array(),
            self.radial.to_array(),
            self.axis.to_array(),
        )
    }

    /// A vector turned about the axis.
    fn turned(&self, vector: Vec3, angle: f64) -> Vec3 {
        let (sin, cos) = angle.sin_cos();
        vector * cos + self.axis.cross(vector) * sin + self.axis * (self.axis.dot(vector) * (1.0 - cos))
    }

    /// A point turned about the axis.
    fn moved(&self, point: [f64; 3], angle: f64) -> [f64; 3] {
        (self.pivot + self.turned(Vec3::from(point) - self.pivot, angle)).to_array()
    }

    /// The profile plane, turned to where the far cap sits.
    fn far_plane(&self, plane: &Plane) -> Plane {
        Plane::from_axes(
            self.moved(plane.origin, self.angle),
            self.turned(Vec3::from(plane.x_axis), self.angle).to_array(),
            self.turned(Vec3::from(plane.y_axis), self.angle).to_array(),
        )
    }

    /// Whether an arc off the axis sweeps a spindle torus: its circle's
    /// centre across the axis from the arc, or no farther from the axis than
    /// the circle's radius. A centre on the axis is a sphere, not a spindle.
    fn spindle(&self, plane: &Plane, arc: &ocs_geom::geom2d::Arc) -> bool {
        let centre = self.station(plane, arc.centre).radius;
        centre.abs() > self.tolerance && (centre < 0.0 || centre <= arc.radius + self.tolerance)
    }

    /// The rectangular surface parameters traced by one circular profile arc.
    ///
    /// A circle centre on the negative side of the axis is represented by a
    /// positive-major-radius torus using a negative ring. That sheet needs a
    /// half-turn longitude shift and the complementary tube angle; retaining
    /// those values as pcurves is what keeps a chord-bounded arc trimmed.
    fn revolved_arc_parameters(
        &self,
        plane: &Plane,
        arc: &ocs_geom::geom2d::Arc,
    ) -> Option<RevolvedArcParameters> {
        let sweep = arc.sweep();
        let middle = arc.start_angle + sweep * 0.5;
        let (sin, cos) = middle.sin_cos();
        let offset = Vec3::from(plane.vector_at([
            arc.radius * cos,
            arc.radius * sin,
        ]));
        let tangent = Vec3::from(plane.vector_at([
            -arc.radius * sin,
            arc.radius * cos,
        ]));
        let radial = offset.dot(self.radial);
        let height = offset.dot(self.axis);
        let radial_tangent = tangent.dot(self.radial);
        let height_tangent = tangent.dot(self.axis);
        let orientation = radial * height_tangent - height * radial_tangent;
        if !sweep.is_finite()
            || !radial.is_finite()
            || !height.is_finite()
            || !orientation.is_finite()
            || orientation == 0.0
        {
            return None;
        }
        let middle = height.atan2(radial);
        let direction = orientation.signum();
        let gamma_start = middle - direction * sweep * 0.5;
        let gamma_end = middle + direction * sweep * 0.5;
        let centre = self.station(plane, arc.centre);
        if centre.radius < -self.tolerance {
            Some(RevolvedArcParameters {
                around: std::f64::consts::PI,
                start: std::f64::consts::PI - gamma_start,
                end: std::f64::consts::PI - gamma_end,
            })
        } else {
            Some(RevolvedArcParameters {
                around: 0.0,
                start: gamma_start,
                end: gamma_end,
            })
        }
    }

    /// Face orientation sampled halfway through the turn. NURBS normals are
    /// evaluated at their parameter-domain midpoint, so their comparison
    /// direction and point must be at that same angular station.
    fn face_sense(&self, surface: &Surface, on: [f64; 3], out: Vec3) -> Option<bool> {
        let halfway = self.angle * 0.5;
        face_sense(
            surface,
            self.moved(on, halfway),
            self.turned(out, halfway),
        )
    }

    fn trigonometric_projection_bounds(
        centre: f64,
        cosine: f64,
        sine: f64,
        start: f64,
        sweep: f64,
    ) -> Option<ProjectionBounds> {
        let end = start + sweep;
        if !centre.is_finite()
            || !cosine.is_finite()
            || !sine.is_finite()
            || !start.is_finite()
            || !sweep.is_finite()
            || !end.is_finite()
            || sweep <= 0.0
        {
            return None;
        }
        let at = |parameter: f64| centre + cosine * parameter.cos() + sine * parameter.sin();
        let mut bounds = ProjectionBounds::new(at(start), at(end))?;
        let maximum = sine.atan2(cosine);
        for parameter in [maximum, maximum + TAU * 0.5] {
            let offset = (parameter - start).rem_euclid(TAU);
            if sweep >= TAU - 1e-12 || offset <= sweep + 1e-12 {
                let value = at(parameter);
                bounds.include(ProjectionBounds::new(value, value)?);
            }
        }
        Some(bounds)
    }

    fn trigonometric_radial_reach(
        &self,
        centre: f64,
        cosine: f64,
        sine: f64,
        start: f64,
        sweep: f64,
    ) -> Option<RadialReach> {
        let bounds = Self::trigonometric_projection_bounds(centre, cosine, sine, start, sweep)?;
        RadialReach::from_bounds(bounds.low, bounds.high, self.tolerance)
    }

    fn valid_nurbs(curve: &ocs_geom::geom2d::NurbsCurve) -> bool {
        let (domain_start, domain_end) = curve.domain();
        domain_start.is_finite()
            && domain_end.is_finite()
            && domain_end > domain_start
            && curve.knots().iter().all(|value| value.is_finite())
            && curve.knots().windows(2).all(|pair| pair[0] <= pair[1])
            && curve
                .control_points()
                .iter()
                .flatten()
                .all(|value| value.is_finite())
            && curve
                .weights()
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
    }

    fn nurbs_control_projection_bounds(
        plane: &Plane,
        curve: &ocs_geom::geom2d::NurbsCurve,
        origin: Vec3,
        direction: Vec3,
    ) -> Option<ProjectionBounds> {
        let mut values = curve
            .control_points()
            .iter()
            .map(|point| (Vec3::from(plane.point_at(*point)) - origin).dot(direction));
        let first = values.next()?;
        let mut bounds = ProjectionBounds::new(first, first)?;
        for value in values {
            bounds.include(ProjectionBounds::new(value, value)?);
        }
        Some(bounds)
    }

    /// Tightens positive-weight control-hull bounds without sampling away
    /// narrow knot features. A refinement limit falls back to the enclosing
    /// hull, so the result remains conservative.
    fn nurbs_projection_bounds(
        plane: &Plane,
        curve: &ocs_geom::geom2d::NurbsCurve,
        origin: Vec3,
        direction: Vec3,
    ) -> Option<ProjectionBounds> {
        if !Self::valid_nurbs(curve) {
            return None;
        }

        const MAX_DEPTH: usize = 48;
        const MAX_SPLITS: usize = 8192;
        let mut pending = vec![(curve.clone(), 0_usize)];
        let mut splits = 0_usize;
        let mut result: Option<ProjectionBounds> = None;
        while let Some((span, depth)) = pending.pop() {
            let hull =
                Self::nurbs_control_projection_bounds(plane, &span, origin, direction)?;
            let mut samples: Option<ProjectionBounds> = None;
            for parameter in [0.0, 0.5, 1.0] {
                let point = Vec3::from(plane.point_at(span.point_at(parameter)));
                let value = (point - origin).dot(direction);
                let value = ProjectionBounds::new(value, value)?;
                match &mut samples {
                    Some(bounds) => bounds.include(value),
                    None => samples = Some(value),
                }
            }
            let samples = samples?;
            let slack = (samples.low - hull.low)
                .max(hull.high - samples.high)
                .max(0.0);
            let refinement_scale = hull.magnitude().max(hull.span()).max(1.0);
            let resolved = slack <= 1e-10 * refinement_scale;

            if !resolved && depth < MAX_DEPTH && splits < MAX_SPLITS {
                if let Some((first, second)) = span.split_at(0.5) {
                    splits += 1;
                    pending.push((second, depth + 1));
                    pending.push((first, depth + 1));
                    continue;
                }
            }

            match &mut result {
                Some(bounds) => bounds.include(hull),
                None => result = Some(hull),
            }
        }
        result
    }

    fn piece_projection_bounds(
        plane: &Plane,
        piece: &Curve2,
        origin: Vec3,
        direction: Vec3,
    ) -> Option<ProjectionBounds> {
        let project = |point: [f64; 2]| {
            (Vec3::from(plane.point_at(point)) - origin).dot(direction)
        };
        match piece {
            Curve2::Line(line) => ProjectionBounds::new(project(line.start), project(line.end)),
            Curve2::Arc(arc) => Self::trigonometric_projection_bounds(
                project(arc.centre),
                Vec3::from(plane.vector_at([arc.radius, 0.0])).dot(direction),
                Vec3::from(plane.vector_at([0.0, arc.radius])).dot(direction),
                arc.start_angle,
                arc.sweep(),
            ),
            Curve2::Ellipse(arc) => {
                let ellipse = arc.ellipse;
                let minor = ellipse.minor_axis();
                Self::trigonometric_projection_bounds(
                    project(ellipse.centre),
                    Vec3::from(plane.vector_at([
                        ellipse.major_axis[0] * ellipse.major_radius,
                        ellipse.major_axis[1] * ellipse.major_radius,
                    ]))
                    .dot(direction),
                    Vec3::from(plane.vector_at([
                        minor[0] * ellipse.minor_radius,
                        minor[1] * ellipse.minor_radius,
                    ]))
                    .dot(direction),
                    arc.start_parameter,
                    arc.sweep(),
                )
            }
            Curve2::Nurbs(curve) => {
                Self::nurbs_projection_bounds(plane, curve, origin, direction)
            }
            _ => None,
        }
    }

    fn nurbs_control_reach(
        &self,
        plane: &Plane,
        curve: &ocs_geom::geom2d::NurbsCurve,
    ) -> Option<RadialReach> {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for point in curve.control_points() {
            let radius = self.station(plane, *point).radius;
            low = low.min(radius);
            high = high.max(radius);
        }
        RadialReach::from_bounds(low, high, self.tolerance)
    }

    /// Conservative convex-hull classification refined by exact subdivision.
    /// Positive rational weights keep each span inside its control polygon.
    fn nurbs_radial_reach(
        &self,
        plane: &Plane,
        curve: &ocs_geom::geom2d::NurbsCurve,
    ) -> Option<RadialReach> {
        if !Self::valid_nurbs(curve) {
            return None;
        }

        const MAX_DEPTH: usize = 48;
        const MAX_SPANS: usize = 8192;
        let mut pending = vec![(curve.clone(), 0_usize)];
        let mut visited = 0_usize;
        let mut reach = RadialReach::default();
        while let Some((span, depth)) = pending.pop() {
            visited += 1;
            let possible = self.nurbs_control_reach(plane, &span)?;
            if possible.collapsed() {
                continue;
            }

            let mut proven = RadialReach::default();
            for parameter in [0.0, 0.5, 1.0] {
                let radius = self.station(plane, span.point_at(parameter)).radius;
                proven.include(RadialReach::from_bounds(
                    radius,
                    radius,
                    self.tolerance,
                )?);
            }
            reach.include(proven);
            if reach.straddles() {
                return Some(reach);
            }

            let unresolved = (possible.negative && !proven.negative)
                || (possible.positive && !proven.positive);
            if !unresolved {
                continue;
            }
            if depth >= MAX_DEPTH || visited >= MAX_SPANS {
                // The hull still permits a crossing. Refuse rather than let a
                // narrow self-intersection through on an arbitrary sample.
                return None;
            }
            let (first, second) = span.split_at(0.5)?;
            pending.push((second, depth + 1));
            pending.push((first, depth + 1));
        }
        Some(reach)
    }

    fn piece_radial_reach(&self, plane: &Plane, piece: &Curve2) -> Option<RadialReach> {
        match piece {
            Curve2::Line(line) => {
                let first = self.station(plane, line.start).radius;
                let second = self.station(plane, line.end).radius;
                RadialReach::from_bounds(first.min(second), first.max(second), self.tolerance)
            }
            Curve2::Arc(arc) => {
                let centre = self.station(plane, arc.centre).radius;
                let cosine = Vec3::from(plane.vector_at([arc.radius, 0.0])).dot(self.radial);
                let sine = Vec3::from(plane.vector_at([0.0, arc.radius])).dot(self.radial);
                self.trigonometric_radial_reach(
                    centre,
                    cosine,
                    sine,
                    arc.start_angle,
                    arc.sweep(),
                )
            }
            Curve2::Ellipse(arc) => {
                let ellipse = arc.ellipse;
                let centre = self.station(plane, ellipse.centre).radius;
                let minor = ellipse.minor_axis();
                let cosine = Vec3::from(plane.vector_at([
                    ellipse.major_axis[0] * ellipse.major_radius,
                    ellipse.major_axis[1] * ellipse.major_radius,
                ]))
                .dot(self.radial);
                let sine = Vec3::from(plane.vector_at([
                    minor[0] * ellipse.minor_radius,
                    minor[1] * ellipse.minor_radius,
                ]))
                .dot(self.radial);
                self.trigonometric_radial_reach(
                    centre,
                    cosine,
                    sine,
                    arc.start_parameter,
                    arc.sweep(),
                )
            }
            Curve2::Nurbs(curve) => self.nurbs_radial_reach(plane, curve),
            _ => None,
        }
    }

    /// The surface a profile piece sweeps into, and whether it is flat.
    ///
    /// This is the whole of why a revolution produces analytic geometry: a
    /// line parallel to the axis traces a cylinder, one across it a plane,
    /// one at a slant a cone, and an arc a sphere or a torus depending on
    /// whether its centre is on the axis. Axis-collapsed and unsupported
    /// pieces stay distinct so callers can omit only the former; `None`
    /// remains reserved for invalid geometry or a failed construction.
    fn piece_surface(&self, plane: &Plane, piece: &Curve2) -> Option<PieceSurface> {
        if !matches!(
            piece,
            Curve2::Line(_) | Curve2::Arc(_) | Curve2::Ellipse(_) | Curve2::Nurbs(_)
        ) {
            return Some(PieceSurface::Unsupported);
        }
        if self.piece_radial_reach(plane, piece)?.collapsed() {
            return Some(PieceSurface::CollapsedAxis);
        }
        match piece {
            Curve2::Line(line) => {
                let (a, b) = (self.station(plane, line.start), self.station(plane, line.end));
                if (b.height - a.height).abs() <= self.tolerance {
                    Some(PieceSurface::Surface(
                        Surface::Plane(self.circle(a.height)?),
                        true,
                    ))
                } else if (b.radius - a.radius).abs() <= self.tolerance {
                    Some(PieceSurface::Surface(
                        Surface::Cylinder(Cylinder {
                            base: self.circle(a.height)?,
                            radius: a.radius,
                        }),
                        false,
                    ))
                } else {
                    Some(PieceSurface::Surface(
                        Surface::Cone(Cone {
                            base: self.circle(a.height)?,
                            radius: a.radius,
                            // The radius falls by this much per unit along the
                            // axis, which is what the half-angle records.
                            half_angle: ((a.radius - b.radius) / (b.height - a.height)).atan(),
                        }),
                        false,
                    ))
                }
            }
            // A circle whose centre lies across the axis, or nearer it than
            // its radius, sweeps a spindle torus: two sheets in one radial
            // half-plane, which its parameters cannot tell apart. Such an
            // arc (a nearly straight one always is) goes round as the exact
            // rational surface of revolution instead.
            Curve2::Arc(arc)
                if !self.spindle(plane, arc) =>
            {
                let centre = self.station(plane, arc.centre);
                if centre.radius.abs() <= self.tolerance {
                    Some(PieceSurface::Surface(
                        Surface::Sphere(Sphere {
                            frame: self.circle(centre.height)?,
                            radius: arc.radius,
                        }),
                        false,
                    ))
                } else {
                    Some(PieceSurface::Surface(
                        Surface::Torus(Torus {
                            frame: self.circle(centre.height)?,
                            major_radius: centre.radius.abs(),
                            minor_radius: arc.radius,
                        }),
                        false,
                    ))
                }
            }
            Curve2::Arc(_) | Curve2::Ellipse(_) | Curve2::Nurbs(_) => {
                let profile = RationalCurve2::from_curve(piece)?;
                let angular = RationalCurve2::unit_arc(self.angle)?;
                let around = self.axis.cross(self.radial);
                let mut points = Vec::with_capacity(profile.points.len());
                let mut weights = Vec::with_capacity(profile.points.len());
                for (point, profile_weight) in profile.points.iter().zip(&profile.weights) {
                    let station = self.station(plane, *point);
                    points.push(
                        angular
                            .points
                            .iter()
                            .map(|control| {
                                (self.pivot
                                    + self.axis * station.height
                                    + self.radial * (station.radius * control[0])
                                    + around * (station.radius * control[1]))
                                    .to_array()
                            })
                            .collect::<Vec<_>>(),
                    );
                    weights.push(
                        angular
                            .weights
                            .iter()
                            .map(|angular_weight| profile_weight * angular_weight)
                            .collect::<Vec<_>>(),
                    );
                }
                let surface = ocs_geom::space::NurbsSurface3::new_strict(
                    profile.degree,
                    angular.degree,
                    points,
                    profile.knots,
                    angular.knots,
                    weights,
                )?
                .with_periodicity(false, self.full);
                Some(PieceSurface::Surface(Surface::Nurbs(surface), false))
            }
            _ => Some(PieceSurface::Unsupported),
        }
    }
}

/// Revolves a closed profile about an axis.
///
/// The profile is a chain of curves in `plane`'s coordinates, as for
/// [`extrude`], and the axis must lie in that plane — a profile and an axis
/// that do not share a plane sweep into surfaces with no analytic form.
///
/// `angle` is how far round, in radians; a full turn produces a closed body
/// with seams and no caps, and anything less produces one capped by the
/// profile at each end. Its sign says which way, so a caller need not
/// reverse the axis to turn the other way.
///
/// Straight and circular pieces are supported, and between them give every
/// surface ACIS has an analytic record for. `None` for a profile that does
/// not close, encloses nothing, crosses the axis — a solid swept through
/// itself — or holds a piece with no analytic answer.
pub fn revolve(
    plane: Plane,
    profile: &[Curve2],
    pivot: [f64; 3],
    axis: [f64; 3],
    angle: f64,
) -> Option<Body> {
    let senses = profile_senses(profile)?;
    let corners: Vec<[f64; 2]> = profile
        .iter()
        .zip(&senses)
        .map(|(piece, forwards)| piece.point_at(if *forwards { 0.0 } else { 1.0 }))
        .collect();
    let handed: f64 = profile
        .iter()
        .zip(&senses)
        .map(|(piece, forwards)| {
            if *forwards {
                piece.chain_area()
            } else {
                -piece.chain_area()
            }
        })
        .sum();
    if handed == 0.0 || handed.is_nan() {
        return None;
    }

    // An axis outside the profile's plane turns it into rotational spline surfaces.
    if super::sweep_path::axis_off_plane(&plane, pivot, axis) {
        return super::sweep_path::revolve_off_plane(plane, profile, pivot, axis, angle, false);
    }
    let turn = Turn::new(&plane, profile, pivot, axis, angle)?;
    let stations: Vec<Station> = corners.iter().map(|uv| turn.station(&plane, *uv)).collect();

    // The circuits below run the profile first and the turn second for a
    // part revolution, and the other way round for a whole one — so what
    // counts as already outward is the opposite in the two cases.
    let outward = if turn.full {
        turn.spin != (handed > 0.0)
    } else {
        turn.spin == (handed > 0.0)
    };

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

    if turn.full {
        revolve_whole(
            &mut body, shell, &plane, profile, &senses, &stations, &turn, handed, outward,
        )?;
    } else {
        revolve_part(
            &mut body, shell, &plane, profile, &senses, &stations, &turn, handed, outward,
        )?;
    }

    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    body.validate().is_empty().then_some(body)
}

/// Revolves a connected open or closed curve chain into a sheet body.
///
/// No end caps are added. A partial turn leaves the source and destination
/// copies as boundary edges, while a full turn closes each swept piece on
/// itself. Open profile ends remain valid one-coedge shell boundaries.
pub fn revolve_surface(
    plane: Plane,
    profile: &[Curve2],
    pivot: [f64; 3],
    axis: [f64; 3],
    angle: f64,
) -> Option<Body> {
    let closed = profile_senses(profile);
    let (senses, corners) = match closed {
        Some(senses) => {
            let corners = profile
                .iter()
                .zip(&senses)
                .map(|(piece, forward)| piece.point_at(if *forward { 0.0 } else { 1.0 }))
                .collect::<Vec<_>>();
            (senses, corners)
        }
        None => {
            let senses = path_senses(profile)?;
            let mut corners = profile
                .iter()
                .zip(&senses)
                .map(|(piece, forward)| piece.point_at(if *forward { 0.0 } else { 1.0 }))
                .collect::<Vec<_>>();
            let last = profile.last()?.point_at(if *senses.last()? { 1.0 } else { 0.0 });
            corners.push(last);
            (senses, corners)
        }
    };
    if profile.is_empty() || corners.len() < 2 {
        return None;
    }
    let is_closed = corners.len() == profile.len();
    // An axis outside the profile's plane turns it into rotational spline surfaces.
    if super::sweep_path::axis_off_plane(&plane, pivot, axis) {
        return super::sweep_path::revolve_off_plane(plane, profile, pivot, axis, angle, true);
    }
    let turn = Turn::new(&plane, profile, pivot, axis, angle)?;
    let stations = corners
        .iter()
        .map(|corner| turn.station(&plane, *corner))
        .collect::<Vec<_>>();

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

    if turn.full {
        revolve_surface_whole(
            &mut body,
            shell,
            &plane,
            profile,
            &senses,
            &corners,
            &stations,
            &turn,
            is_closed,
        )?;
    } else {
        revolve_surface_part(
            &mut body,
            shell,
            &plane,
            profile,
            &senses,
            &corners,
            &stations,
            &turn,
            is_closed,
        )?;
    }

    split_revolve_surface_components(&mut body, lump, shell)?;
    body.validate().is_empty().then_some(body)
}

/// Revolves an outer loop and removes every following loop as a hole.
pub fn revolve_region(
    plane: Plane,
    profiles: &[Vec<Curve2>],
    pivot: [f64; 3],
    axis: [f64; 3],
    angle: f64,
) -> Option<Body> {
    let mut profiles = profiles.iter();
    let mut result = revolve(plane, profiles.next()?, pivot, axis, angle)?;
    let full = (TAU - angle.abs()).abs() <= 1e-9;
    for hole in profiles {
        let cutter = revolve(plane, hole, pivot, axis, angle)?;
        if full {
            append_void_shell(&mut result, &cutter)?;
            continue;
        }
        let tolerance = super::operation_tolerance(&[&result, &cutter]);
        result = super::boolean::combine(
            result,
            cutter,
            super::boolean::Operation::Difference,
            tolerance,
        )
        .ok()?;
        split_revolve_region_shells(&mut result)?;
    }
    (!result.roots.is_empty() && result.validate().is_empty()).then_some(result)
}

/// A full revolution has no end caps for a hole to cut through. Its revolved
/// inner loop is therefore already the exact closed void shell we need.
fn append_void_shell(target: &mut Body, source: &Body) -> Option<()> {
    let [owner] = target.roots.as_slice() else {
        return None;
    };
    let owner = *owner;
    let shell = target.shells.insert(Shell {
        faces: Vec::new(),
        owner,
        provenance: Provenance::Synthesized,
    });
    for face in source.face_keys().collect::<Vec<_>>() {
        super::boolean::copy_face(target, source, face, shell, true).ok()?;
    }
    target.lumps.get_mut(owner)?.shells.push(shell);
    Some(())
}

/// Revolves every loop of a region into one multi-lump sheet body.
pub fn revolve_surface_region(
    plane: Plane,
    profiles: &[Vec<Curve2>],
    pivot: [f64; 3],
    axis: [f64; 3],
    angle: f64,
) -> Option<Body> {
    let mut result = Body::new();
    for profile in profiles {
        append_body(
            &mut result,
            &revolve_surface(plane, profile, pivot, axis, angle)?,
        )?;
    }
    (!result.roots.is_empty() && result.validate().is_empty()).then_some(result)
}

/// Connected face components, with adjacency defined by sharing an edge.
///
/// Seams name the same face twice and do not create adjacency. Preserving the
/// input face order keeps the component containing the provisional shell's
/// first face first as well.
pub(crate) fn face_components(body: &Body, faces: &[FaceKey]) -> Option<Vec<Vec<FaceKey>>> {
    let included = faces.iter().copied().collect::<HashSet<_>>();
    let mut adjacent = HashMap::<FaceKey, Vec<FaceKey>>::new();
    for (_, edge) in body.edges.iter() {
        let mut edge_faces = Vec::new();
        for coedge in &edge.coedges {
            let ring = body.coedges.get(*coedge)?.owner;
            let face = body.loops.get(ring)?.owner;
            if included.contains(&face) && !edge_faces.contains(&face) {
                edge_faces.push(face);
            }
        }
        for first in 0..edge_faces.len() {
            for second in first + 1..edge_faces.len() {
                adjacent
                    .entry(edge_faces[first])
                    .or_default()
                    .push(edge_faces[second]);
                adjacent
                    .entry(edge_faces[second])
                    .or_default()
                    .push(edge_faces[first]);
            }
        }
    }

    let mut seen = HashSet::new();
    let mut components = Vec::new();
    for seed in faces {
        if !seen.insert(*seed) {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = VecDeque::from([*seed]);
        while let Some(face) = pending.pop_front() {
            component.push(face);
            for neighbour in adjacent.get(&face).into_iter().flatten() {
                if seen.insert(*neighbour) {
                    pending.push_back(*neighbour);
                }
            }
        }
        components.push(component);
    }
    Some(components)
}

fn set_shell_faces(body: &mut Body, shell: ShellKey, faces: Vec<FaceKey>) -> Option<()> {
    body.shells.get_mut(shell)?.faces = faces.clone();
    for face in faces {
        body.faces.get_mut(face)?.owner = shell;
    }
    Some(())
}

/// A sheet component is its own connected top-level body piece.
fn split_revolve_surface_components(
    body: &mut Body,
    first_lump: LumpKey,
    first_shell: ShellKey,
) -> Option<()> {
    let faces = body.shells.get(first_shell)?.faces.clone();
    let components = face_components(body, &faces)?;
    let first = components.first()?.clone();

    set_shell_faces(body, first_shell, first)?;
    {
        let shell = body.shells.get_mut(first_shell)?;
        shell.owner = first_lump;
        shell.provenance = Provenance::Synthesized;
    }
    {
        let lump = body.lumps.get_mut(first_lump)?;
        lump.shells = vec![first_shell];
        lump.provenance = Provenance::Synthesized;
    }
    body.roots = vec![first_lump];

    for component in components.into_iter().skip(1) {
        let lump = body.lumps.insert(Lump {
            shells: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        let shell = body.shells.insert(Shell {
            faces: component.clone(),
            owner: lump,
            provenance: Provenance::Synthesized,
        });
        set_shell_faces(body, shell, component)?;
        body.lumps.get_mut(lump)?.shells = vec![shell];
        body.roots.push(lump);
    }
    Some(())
}

/// Boolean difference returns one provisional shell. Keep its first connected
/// component as the exterior shell and attach later components as void shells
/// of the same lump.
fn split_revolve_region_shells(body: &mut Body) -> Option<()> {
    let [first_lump] = body.roots.as_slice() else {
        return None;
    };
    let first_lump = *first_lump;
    let shells = body.lumps.get(first_lump)?.shells.clone();
    let [first_shell] = shells.as_slice() else {
        return None;
    };
    let first_shell = *first_shell;
    let faces = body.shells.get(first_shell)?.faces.clone();
    let components = face_components(body, &faces)?;
    let first = components.first()?.clone();

    set_shell_faces(body, first_shell, first)?;
    {
        let shell = body.shells.get_mut(first_shell)?;
        shell.owner = first_lump;
        shell.provenance = Provenance::Synthesized;
    }
    let mut regrouped = vec![first_shell];
    for component in components.into_iter().skip(1) {
        let shell = body.shells.insert(Shell {
            faces: component.clone(),
            owner: first_lump,
            provenance: Provenance::Synthesized,
        });
        set_shell_faces(body, shell, component)?;
        regrouped.push(shell);
    }
    {
        let lump = body.lumps.get_mut(first_lump)?;
        lump.shells = regrouped;
        lump.provenance = Provenance::Synthesized;
    }
    body.roots = vec![first_lump];
    Some(())
}

pub(crate) fn append_body(target: &mut Body, source: &Body) -> Option<()> {
    let vertices = source
        .vertices
        .iter()
        .map(|(key, value)| (key, target.vertices.insert(value.clone())))
        .collect::<HashMap<_, _>>();
    let curves = source
        .curves
        .iter()
        .map(|(key, value)| (key, target.curves.insert(value.clone())))
        .collect::<HashMap<_, _>>();
    let surfaces = source
        .surfaces
        .iter()
        .map(|(key, value)| (key, target.surfaces.insert(value.clone())))
        .collect::<HashMap<_, _>>();
    let lumps = source
        .lumps
        .iter()
        .map(|(key, value)| {
            (
                key,
                target.lumps.insert(Lump {
                    shells: Vec::new(),
                    provenance: value.provenance,
                }),
            )
        })
        .collect::<HashMap<_, _>>();
    let shells = source
        .shells
        .iter()
        .map(|(key, value)| {
            Some((
                key,
                target.shells.insert(Shell {
                    faces: Vec::new(),
                    owner: *lumps.get(&value.owner)?,
                    provenance: value.provenance,
                }),
            ))
        })
        .collect::<Option<HashMap<_, _>>>()?;
    let faces = source
        .faces
        .iter()
        .map(|(key, value)| {
            Some((
                key,
                target.faces.insert(Face {
                    surface: *surfaces.get(&value.surface)?,
                    forward: value.forward,
                    loops: Vec::new(),
                    owner: *shells.get(&value.owner)?,
                    provenance: value.provenance,
                }),
            ))
        })
        .collect::<Option<HashMap<_, _>>>()?;
    let loops = source
        .loops
        .iter()
        .map(|(key, value)| {
            Some((
                key,
                target.loops.insert(Loop {
                    coedges: Vec::new(),
                    owner: *faces.get(&value.owner)?,
                    provenance: value.provenance,
                }),
            ))
        })
        .collect::<Option<HashMap<_, _>>>()?;
    let edges = source
        .edges
        .iter()
        .map(|(key, value)| {
            Some((
                key,
                target.edges.insert(Edge {
                    curve: *curves.get(&value.curve)?,
                    start_parameter: value.start_parameter,
                    end_parameter: value.end_parameter,
                    start: *vertices.get(&value.start)?,
                    end: *vertices.get(&value.end)?,
                    coedges: Vec::new(),
                    provenance: value.provenance,
                }),
            ))
        })
        .collect::<Option<HashMap<_, _>>>()?;
    let coedges = source
        .coedges
        .iter()
        .map(|(key, value)| {
            Some((
                key,
                target.coedges.insert(Coedge {
                    edge: *edges.get(&value.edge)?,
                    forward: value.forward,
                    pcurve: value.pcurve.clone(),
                    owner: *loops.get(&value.owner)?,
                    provenance: value.provenance,
                }),
            ))
        })
        .collect::<Option<HashMap<_, _>>>()?;

    for (key, value) in source.edges.iter() {
        target.edges.get_mut(*edges.get(&key)?)?.coedges = value
            .coedges
            .iter()
            .map(|key| coedges.get(key).copied())
            .collect::<Option<Vec<_>>>()?;
    }
    for (key, value) in source.loops.iter() {
        target.loops.get_mut(*loops.get(&key)?)?.coedges = value
            .coedges
            .iter()
            .map(|key| coedges.get(key).copied())
            .collect::<Option<Vec<_>>>()?;
    }
    for (key, value) in source.faces.iter() {
        target.faces.get_mut(*faces.get(&key)?)?.loops = value
            .loops
            .iter()
            .map(|key| loops.get(key).copied())
            .collect::<Option<Vec<_>>>()?;
    }
    for (key, value) in source.shells.iter() {
        target.shells.get_mut(*shells.get(&key)?)?.faces = value
            .faces
            .iter()
            .map(|key| faces.get(key).copied())
            .collect::<Option<Vec<_>>>()?;
    }
    for (key, value) in source.lumps.iter() {
        target.lumps.get_mut(*lumps.get(&key)?)?.shells = value
            .shells
            .iter()
            .map(|key| shells.get(key).copied())
            .collect::<Option<Vec<_>>>()?;
    }
    target.roots.extend(
        source
            .roots
            .iter()
            .map(|key| lumps.get(key).copied())
            .collect::<Option<Vec<_>>>()?,
    );
    Some(())
}

impl Turn {
    /// Works out the frame, and checks the profile can be turned in it.
    fn new(
        plane: &Plane,
        profile: &[Curve2],
        pivot: [f64; 3],
        axis: [f64; 3],
        angle: f64,
    ) -> Option<Self> {
        let normal = Vec3::from(plane.normal()?);
        let mut direction = Vec3::from(axis).normalize()?;
        let mut angle = angle;
        // A backwards turn is the same as a forwards one about the other way,
        // and saying so once here spares every step below a sign.
        if angle < 0.0 {
            direction = -direction;
            angle = -angle;
        }
        if angle <= 0.0 || angle > TAU + 1e-12 {
            return None;
        }
        // The axis has to lie in the profile's plane, or the pieces sweep
        // into surfaces none of the analytic kinds describe.
        if direction.dot(normal).abs() > 1e-9 || plane.distance_to(pivot)?.abs() > 1e-9 {
            return None;
        }
        let radial = normal.cross(direction).normalize()?;

        let mut turn = Self {
            pivot: Vec3::from(pivot),
            axis: direction,
            radial,
            spin: normal.dot(direction.cross(radial)) > 0.0,
            angle,
            full: (TAU - angle).abs() <= 1e-9,
            tolerance: 0.0,
        };

        // Measure radius from the axis and height from the profile itself.
        // This keeps tolerance independent of where the pivot lies along the
        // axis. Every bound below encloses the whole piece without a fixed
        // parameter grid.
        let first = profile.iter().find(|piece| {
            matches!(
                piece,
                Curve2::Line(_) | Curve2::Arc(_) | Curve2::Ellipse(_) | Curve2::Nurbs(_)
            )
        })?;
        let scale_origin = Vec3::from(plane.point_at(first.point_at(0.0)));
        let mut radial_bounds: Option<ProjectionBounds> = None;
        let mut height_bounds: Option<ProjectionBounds> = None;
        for piece in profile {
            if !matches!(
                piece,
                Curve2::Line(_) | Curve2::Arc(_) | Curve2::Ellipse(_) | Curve2::Nurbs(_)
            ) {
                continue;
            }
            let piece_radial =
                Self::piece_projection_bounds(plane, piece, turn.pivot, turn.radial)?;
            let piece_height =
                Self::piece_projection_bounds(plane, piece, scale_origin, turn.axis)?;
            match &mut radial_bounds {
                Some(bounds) => bounds.include(piece_radial),
                None => radial_bounds = Some(piece_radial),
            }
            match &mut height_bounds {
                Some(bounds) => bounds.include(piece_height),
                None => height_bounds = Some(piece_height),
            }
        }
        let profile_scale = radial_bounds?.magnitude().max(height_bounds?.span());
        turn.tolerance = 1e-9 * profile_scale.max(1.0);
        if profile_scale <= turn.tolerance {
            return None;
        }

        let mut reach = RadialReach::default();
        for piece in profile {
            if !matches!(
                piece,
                Curve2::Line(_) | Curve2::Arc(_) | Curve2::Ellipse(_) | Curve2::Nurbs(_)
            ) {
                continue;
            }
            reach.include(turn.piece_radial_reach(plane, piece)?);
            if reach.straddles() {
                break;
            }
        }
        if reach.straddles() {
            // It straddles the axis, so the solid would be swept through
            // itself. Refusing says so rather than handing back a shape whose
            // inside is not a region.
            return None;
        }
        if reach.collapsed() {
            return None;
        }
        if !reach.positive {
            // All on the far side. Zero angle belongs on the profile, so the
            // frame turns to meet it rather than the radii being negated.
            turn.radial = -turn.radial;
            turn.spin = !turn.spin;
        }
        Some(turn)
    }
}

/// A revolution that stops short: capped by the profile at each end, and one
/// lateral face per piece, exactly as an extrusion is.
#[allow(clippy::too_many_arguments)]
fn revolve_part(
    body: &mut Body,
    shell: ShellKey,
    plane: &Plane,
    profile: &[Curve2],
    senses: &[bool],
    stations: &[Station],
    turn: &Turn,
    handed: f64,
    outward: bool,
) -> Option<()> {
    let far = turn.far_plane(plane);
    // A corner on the axis is the same point at every angle, so the two caps
    // share the vertex rather than each having their own.
    let near: Vec<VertexKey> = (0..profile.len())
        .map(|index| add_vertex(body, plane.point_at(profile_corner(profile, senses, index))))
        .collect();
    let far_ring: Vec<VertexKey> = stations
        .iter()
        .enumerate()
        .map(|(index, station)| {
            if station.radius <= turn.tolerance {
                near[index]
            } else {
                add_vertex(body, far.point_at(profile_corner(profile, senses, index)))
            }
        })
        .collect();

    let near_edges = profile_edges(body, plane, profile, senses, &near)?;
    // A piece on the axis does not move, so its two copies are one edge —
    // and the caps share it. Building a second would leave two coincident
    // edges each used once, which is an open shell wearing a closed one's
    // face count.
    let mut far_edges = Vec::with_capacity(profile.len());
    for (index, piece) in profile.iter().enumerate() {
        let next = (index + 1) % profile.len();
        match turn.piece_surface(plane, piece)? {
            PieceSurface::CollapsedAxis => {
                far_edges.push(near_edges[index]);
                continue;
            }
            PieceSurface::Unsupported => return None,
            PieceSurface::Surface(_, _) => {}
        }
        let (from, to) = if senses[index] {
            (far_ring[index], far_ring[next])
        } else {
            (far_ring[next], far_ring[index])
        };
        far_edges.push(add_profile_edge(body, &far, piece, from, to)?);
    }

    // The tracks each corner follows round. A corner on the axis stays put
    // and has none.
    let rails: Vec<Option<EdgeKey>> = stations
        .iter()
        .enumerate()
        .map(|(index, station)| {
            if station.radius <= turn.tolerance {
                return Some(None);
            }
            let curve = body.curves.insert(Curve3::Circle(Circle3 {
                plane: turn.circle(station.height)?,
                radius: station.radius,
            }));
            Some(Some(body.edges.insert(Edge {
                curve,
                start_parameter: 0.0,
                end_parameter: turn.angle,
                start: near[index],
                end: far_ring[index],
                coedges: Vec::new(),
                provenance: Provenance::Synthesized,
            })))
        })
        .collect::<Option<Vec<_>>>()?;

    add_cap(body, shell, *plane, &near_edges, senses, !turn.spin, !outward)?;
    add_cap(body, shell, far, &far_edges, senses, turn.spin, outward)?;

    for (index, piece) in profile.iter().enumerate() {
        let surface = match turn.piece_surface(plane, piece)? {
            PieceSurface::CollapsedAxis => continue,
            PieceSurface::Surface(surface, _) => surface,
            PieceSurface::Unsupported => return None,
        };
        let next = (index + 1) % profile.len();
        let spline = surface.is_spline();
        let revolved_arc = match (&surface, piece) {
            (Surface::Sphere(_) | Surface::Torus(_), Curve2::Arc(arc)) => {
                Some(turn.revolved_arc_parameters(plane, arc)?)
            }
            _ => None,
        };
        let surface = body.surfaces.insert(surface);
        let out = piece_outward(plane, piece, senses[index], handed > 0.0);
        let on = plane.point_at(piece.point_at(0.5));
        let forward = turn.face_sense(body.surfaces.get(surface)?, on, out)?;
        let mut circuit = vec![(near_edges[index], senses[index])];
        if let Some(rail) = rails[next] {
            circuit.push((rail, true));
        }
        circuit.push((far_edges[index], !senses[index]));
        if let Some(rail) = rails[index] {
            circuit.push((rail, false));
        }
        if spline {
            let start = if senses[index] { 0.0 } else { 1.0 };
            let end = 1.0 - start;
            let pcurves = vec![
                ([start, 0.0], [end, 0.0]),
                ([end, 0.0], [end, 1.0]),
                ([end, 1.0], [start, 1.0]),
                ([start, 1.0], [start, 0.0]),
            ];
            let (circuit, pcurves) = reorder_with_pcurves(circuit, pcurves, outward);
            add_wall_with_pcurves(body, shell, surface, forward, &circuit, &pcurves)?;
        } else if let Some(parameters) = revolved_arc {
            let (start, end) = if senses[index] {
                (parameters.start, parameters.end)
            } else {
                (parameters.end, parameters.start)
            };
            let near = parameters.around;
            let far = near + turn.angle;
            let mut pcurves = vec![([near, start], [near, end])];
            if rails[next].is_some() {
                pcurves.push(([near, end], [far, end]));
            }
            pcurves.push(([far, end], [far, start]));
            if rails[index].is_some() {
                pcurves.push(([far, start], [near, start]));
            }
            let (circuit, pcurves) = reorder_with_pcurves(circuit, pcurves, outward);
            add_wall_with_pcurves(body, shell, surface, forward, &circuit, &pcurves)?;
        } else {
            add_wall(body, shell, surface, forward, &reorder(circuit, outward))?;
        }
    }
    Some(())
}

#[allow(clippy::too_many_arguments)]
fn revolve_surface_part(
    body: &mut Body,
    shell: ShellKey,
    plane: &Plane,
    profile: &[Curve2],
    senses: &[bool],
    corners: &[[f64; 2]],
    stations: &[Station],
    turn: &Turn,
    closed: bool,
) -> Option<()> {
    let far = turn.far_plane(plane);
    let near = corners
        .iter()
        .map(|corner| add_vertex(body, plane.point_at(*corner)))
        .collect::<Vec<_>>();
    let far_ring = corners
        .iter()
        .zip(stations)
        .enumerate()
        .map(|(index, (corner, station))| {
            if station.radius <= turn.tolerance {
                near[index]
            } else {
                add_vertex(body, far.point_at(*corner))
            }
        })
        .collect::<Vec<_>>();

    let mut near_edges = Vec::with_capacity(profile.len());
    let mut far_edges = Vec::with_capacity(profile.len());
    for (index, piece) in profile.iter().enumerate() {
        match turn.piece_surface(plane, piece)? {
            PieceSurface::CollapsedAxis => {
                near_edges.push(None);
                far_edges.push(None);
                continue;
            }
            PieceSurface::Unsupported => return None,
            PieceSurface::Surface(_, _) => {}
        }
        let next = if closed { (index + 1) % corners.len() } else { index + 1 };
        let (near_from, near_to) = if senses[index] {
            (near[index], near[next])
        } else {
            (near[next], near[index])
        };
        let near_edge = add_profile_edge(body, plane, piece, near_from, near_to)?;
        near_edges.push(Some(near_edge));
        let (far_from, far_to) = if senses[index] {
            (far_ring[index], far_ring[next])
        } else {
            (far_ring[next], far_ring[index])
        };
        far_edges.push(Some(add_profile_edge(body, &far, piece, far_from, far_to)?));
    }

    let rails = stations
        .iter()
        .enumerate()
        .map(|(index, station)| {
            if station.radius <= turn.tolerance {
                return Some(None);
            }
            let curve = body.curves.insert(Curve3::Circle(Circle3 {
                plane: turn.circle(station.height)?,
                radius: station.radius,
            }));
            Some(Some(body.edges.insert(Edge {
                curve,
                start_parameter: 0.0,
                end_parameter: turn.angle,
                start: near[index],
                end: far_ring[index],
                coedges: Vec::new(),
                provenance: Provenance::Synthesized,
            })))
        })
        .collect::<Option<Vec<_>>>()?;

    for (index, piece) in profile.iter().enumerate() {
        let surface = match turn.piece_surface(plane, piece)? {
            PieceSurface::CollapsedAxis => continue,
            PieceSurface::Surface(surface, _) => surface,
            PieceSurface::Unsupported => return None,
        };
        let next = if closed { (index + 1) % corners.len() } else { index + 1 };
        let spline = surface.is_spline();
        let revolved_arc = match (&surface, piece) {
            (Surface::Sphere(_) | Surface::Torus(_), Curve2::Arc(arc)) => {
                Some(turn.revolved_arc_parameters(plane, arc)?)
            }
            _ => None,
        };
        let surface = body.surfaces.insert(surface);
        let out = piece_outward(plane, piece, senses[index], true);
        let on = plane.point_at(piece.point_at(0.5));
        let forward = turn.face_sense(body.surfaces.get(surface)?, on, out)?;
        let mut circuit = vec![(near_edges[index]?, senses[index])];
        if let Some(rail) = rails[next] {
            circuit.push((rail, true));
        }
        circuit.push((far_edges[index]?, !senses[index]));
        if let Some(rail) = rails[index] {
            circuit.push((rail, false));
        }
        if spline {
            let start = if senses[index] { 0.0 } else { 1.0 };
            let end = 1.0 - start;
            let mut pcurves = vec![([start, 0.0], [end, 0.0])];
            if rails[next].is_some() {
                pcurves.push(([end, 0.0], [end, 1.0]));
            }
            pcurves.push(([end, 1.0], [start, 1.0]));
            if rails[index].is_some() {
                pcurves.push(([start, 1.0], [start, 0.0]));
            }
            add_wall_with_pcurves(body, shell, surface, forward, &circuit, &pcurves)?;
        } else if let Some(parameters) = revolved_arc {
            let (start, end) = if senses[index] {
                (parameters.start, parameters.end)
            } else {
                (parameters.end, parameters.start)
            };
            let near = parameters.around;
            let far = near + turn.angle;
            let mut pcurves = vec![([near, start], [near, end])];
            if rails[next].is_some() {
                pcurves.push(([near, end], [far, end]));
            }
            pcurves.push(([far, end], [far, start]));
            if rails[index].is_some() {
                pcurves.push(([far, start], [near, start]));
            }
            add_wall_with_pcurves(body, shell, surface, forward, &circuit, &pcurves)?;
        } else {
            add_wall(body, shell, surface, forward, &circuit)?;
        }
    }
    Some(())
}

/// A revolution that closes on itself: no caps, and each piece's face bounded
/// by the rims its corners trace plus a seam where it started.
///
/// That is the shape ACIS gives a cylinder, a cone and a sphere, and it is
/// the reason the seam is an edge traversed twice rather than two edges: a
/// surface closed the whole way round has to be cut open somewhere for its
/// face to have a boundary at all, and where it is cut is not a real edge of
/// the solid.
#[allow(clippy::too_many_arguments)]
fn revolve_whole(
    body: &mut Body,
    shell: ShellKey,
    plane: &Plane,
    profile: &[Curve2],
    senses: &[bool],
    stations: &[Station],
    turn: &Turn,
    handed: f64,
    outward: bool,
) -> Option<()> {
    let count = profile.len();
    let kinds = profile
        .iter()
        .map(|piece| turn.piece_surface(plane, piece))
        .collect::<Option<Vec<_>>>()?;
    if kinds
        .iter()
        .any(|kind| matches!(kind, PieceSurface::Unsupported))
    {
        return None;
    }
    let previous = |index: usize| (index + count - 1) % count;

    // A rim exists where a corner is off the axis and a face next to it needs
    // it; a seam only where the face it cuts is closed the whole way round.
    // A flat face is not — its parameters are the plane's own, and a disc in
    // them is bounded by its rim alone.
    let has_rim: Vec<bool> = (0..count)
        .map(|index| {
            stations[index].radius > turn.tolerance
                && (matches!(&kinds[index], PieceSurface::Surface(_, _))
                    || matches!(&kinds[previous(index)], PieceSurface::Surface(_, _)))
        })
        .collect();
    let has_seam: Vec<bool> = kinds
        .iter()
        .map(|kind| matches!(kind, PieceSurface::Surface(_, false)))
        .collect();

    // Only the corners something ends at become vertices. A disc's centre is
    // named by nothing, and inventing a vertex there would leave one with no
    // edges on it.
    let vertices: Vec<Option<VertexKey>> = (0..count)
        .map(|index| {
            (has_rim[index] || has_seam[index] || has_seam[previous(index)]).then(|| {
                add_vertex(body, plane.point_at(profile_corner(profile, senses, index)))
            })
        })
        .collect();

    let rims: Vec<Option<EdgeKey>> = (0..count)
        .map(|index| {
            if !has_rim[index] {
                return Some(None);
            }
            let corner = vertices[index]?;
            let curve = body.curves.insert(Curve3::Circle(Circle3 {
                plane: turn.circle(stations[index].height)?,
                radius: stations[index].radius,
            }));
            Some(Some(body.edges.insert(Edge {
                curve,
                start_parameter: 0.0,
                end_parameter: TAU,
                start: corner,
                end: corner,
                coedges: Vec::new(),
                provenance: Provenance::Synthesized,
            })))
        })
        .collect::<Option<Vec<_>>>()?;

    let seams: Vec<Option<EdgeKey>> = (0..count)
        .map(|index| {
            if !has_seam[index] {
                return Some(None);
            }
            let next = (index + 1) % count;
            let (from, to) = if senses[index] {
                (vertices[index]?, vertices[next]?)
            } else {
                (vertices[next]?, vertices[index]?)
            };
            Some(Some(add_profile_edge(
                body,
                plane,
                &profile[index],
                from,
                to,
            )?))
        })
        .collect::<Option<Vec<_>>>()?;

    for (index, piece) in profile.iter().enumerate() {
        let (surface, flat) = match kinds[index].clone() {
            PieceSurface::CollapsedAxis => continue,
            PieceSurface::Surface(surface, flat) => (surface, flat),
            PieceSurface::Unsupported => return None,
        };
        let next = (index + 1) % count;
        let spline = surface.is_spline();
        let revolved_arc = match (&surface, piece) {
            (Surface::Torus(_), Curve2::Arc(arc)) => {
                Some(turn.revolved_arc_parameters(plane, arc)?)
            }
            _ => None,
        };
        let surface = body.surfaces.insert(surface);
        let out = piece_outward(plane, piece, senses[index], handed > 0.0);
        let on = plane.point_at(piece.point_at(0.5));
        let forward = turn.face_sense(body.surfaces.get(surface)?, on, out)?;
        let face = add_face(body, shell, surface, forward);

        if flat {
            // A disc or an annulus: one loop per rim, the wider one bounding
            // and the narrower cutting a hole out of it.
            let mut edges: Vec<(EdgeKey, bool, f64)> = Vec::new();
            if let Some(rim) = rims[index] {
                edges.push((rim, outward, stations[index].radius));
            }
            if let Some(rim) = rims[next] {
                edges.push((rim, !outward, stations[next].radius));
            }
            edges.sort_by(|a, b| b.2.total_cmp(&a.2));
            for (rim, sense, _) in edges {
                add_ring(body, face, &[(rim, sense)])?;
            }
        } else {
            let seam = seams[index]?;
            let mut circuit: Vec<(EdgeKey, bool)> = Vec::new();
            if let Some(rim) = rims[index] {
                circuit.push((rim, true));
            }
            circuit.push((seam, senses[index]));
            if let Some(rim) = rims[next] {
                circuit.push((rim, false));
            }
            circuit.push((seam, !senses[index]));
            if spline {
                let start = if senses[index] { 0.0 } else { 1.0 };
                let end = 1.0 - start;
                let mut pcurves = Vec::new();
                if rims[index].is_some() {
                    pcurves.push(([start, 0.0], [start, 1.0]));
                }
                pcurves.push(([start, 1.0], [end, 1.0]));
                if rims[next].is_some() {
                    pcurves.push(([end, 1.0], [end, 0.0]));
                }
                pcurves.push(([end, 0.0], [start, 0.0]));
                let (circuit, pcurves) = reorder_with_pcurves(circuit, pcurves, outward);
                add_ring_with_pcurves(body, face, &circuit, &pcurves)?;
            } else if let Some(parameters) = revolved_arc {
                let (start, end) = if senses[index] {
                    (parameters.start, parameters.end)
                } else {
                    (parameters.end, parameters.start)
                };
                let near = parameters.around;
                let far = near + TAU;
                let mut pcurves = Vec::new();
                if rims[index].is_some() {
                    pcurves.push(([near, start], [far, start]));
                }
                pcurves.push(([far, start], [far, end]));
                if rims[next].is_some() {
                    pcurves.push(([far, end], [near, end]));
                }
                pcurves.push(([near, end], [near, start]));
                let (circuit, pcurves) = reorder_with_pcurves(circuit, pcurves, outward);
                add_ring_with_pcurves(body, face, &circuit, &pcurves)?;
            } else {
                add_ring(body, face, &reorder(circuit, outward))?;
            }
        }
        body.shells.get_mut(shell)?.faces.push(face);
    }
    Some(())
}

#[allow(clippy::too_many_arguments)]
fn revolve_surface_whole(
    body: &mut Body,
    shell: ShellKey,
    plane: &Plane,
    profile: &[Curve2],
    senses: &[bool],
    corners: &[[f64; 2]],
    stations: &[Station],
    turn: &Turn,
    closed: bool,
) -> Option<()> {
    let piece_count = profile.len();
    let corner_count = corners.len();
    let kinds = profile
        .iter()
        .map(|piece| turn.piece_surface(plane, piece))
        .collect::<Option<Vec<_>>>()?;
    if kinds
        .iter()
        .any(|kind| matches!(kind, PieceSurface::Unsupported))
    {
        return None;
    }
    let before = |corner: usize| -> Option<usize> {
        if corner > 0 {
            Some(corner - 1)
        } else if closed {
            Some(piece_count - 1)
        } else {
            None
        }
    };
    let after = |corner: usize| -> Option<usize> {
        if corner < piece_count {
            Some(corner)
        } else if closed {
            Some(0)
        } else {
            None
        }
    };
    let has_kind = |piece: Option<usize>| {
        piece.is_some_and(|index| matches!(&kinds[index], PieceSurface::Surface(_, _)))
    };
    let has_seam = |piece: Option<usize>| {
        piece.is_some_and(|index| matches!(&kinds[index], PieceSurface::Surface(_, false)))
    };

    let has_rim = (0..corner_count)
        .map(|index| {
            stations[index].radius > turn.tolerance
                && (has_kind(before(index)) || has_kind(after(index)))
        })
        .collect::<Vec<_>>();
    let vertices = (0..corner_count)
        .map(|index| {
            (has_rim[index] || has_seam(before(index)) || has_seam(after(index)))
                .then(|| add_vertex(body, plane.point_at(corners[index])))
        })
        .collect::<Vec<_>>();
    let rims = (0..corner_count)
        .map(|index| {
            if !has_rim[index] {
                return Some(None);
            }
            let corner = vertices[index]?;
            let curve = body.curves.insert(Curve3::Circle(Circle3 {
                plane: turn.circle(stations[index].height)?,
                radius: stations[index].radius,
            }));
            Some(Some(body.edges.insert(Edge {
                curve,
                start_parameter: 0.0,
                end_parameter: TAU,
                start: corner,
                end: corner,
                coedges: Vec::new(),
                provenance: Provenance::Synthesized,
            })))
        })
        .collect::<Option<Vec<_>>>()?;
    let seams = (0..piece_count)
        .map(|index| {
            if !matches!(&kinds[index], PieceSurface::Surface(_, false)) {
                return Some(None);
            }
            let next = if closed { (index + 1) % corner_count } else { index + 1 };
            let (from, to) = if senses[index] {
                (vertices[index]?, vertices[next]?)
            } else {
                (vertices[next]?, vertices[index]?)
            };
            Some(Some(add_profile_edge(body, plane, &profile[index], from, to)?))
        })
        .collect::<Option<Vec<_>>>()?;

    for (index, piece) in profile.iter().enumerate() {
        let (surface, flat) = match kinds[index].clone() {
            PieceSurface::CollapsedAxis => continue,
            PieceSurface::Surface(surface, flat) => (surface, flat),
            PieceSurface::Unsupported => return None,
        };
        let next = if closed { (index + 1) % corner_count } else { index + 1 };
        let spline = surface.is_spline();
        let revolved_arc = match (&surface, piece) {
            (Surface::Torus(_), Curve2::Arc(arc)) => {
                Some(turn.revolved_arc_parameters(plane, arc)?)
            }
            _ => None,
        };
        let surface = body.surfaces.insert(surface);
        let out = piece_outward(plane, piece, senses[index], true);
        let on = plane.point_at(piece.point_at(0.5));
        let forward = turn.face_sense(body.surfaces.get(surface)?, on, out)?;
        let face = add_face(body, shell, surface, forward);

        if flat {
            let mut edges = Vec::new();
            if let Some(rim) = rims[index] {
                edges.push((rim, true, stations[index].radius));
            }
            if let Some(rim) = rims[next] {
                edges.push((rim, false, stations[next].radius));
            }
            edges.sort_by(|a, b| b.2.total_cmp(&a.2));
            for (rim, sense, _) in edges {
                add_ring(body, face, &[(rim, sense)])?;
            }
        } else {
            let seam = seams[index]?;
            let mut circuit = Vec::new();
            if let Some(rim) = rims[index] {
                circuit.push((rim, true));
            }
            circuit.push((seam, senses[index]));
            if let Some(rim) = rims[next] {
                circuit.push((rim, false));
            }
            circuit.push((seam, !senses[index]));
            if spline {
                let start = if senses[index] { 0.0 } else { 1.0 };
                let end = 1.0 - start;
                let mut pcurves = Vec::new();
                if rims[index].is_some() {
                    pcurves.push(([start, 0.0], [start, 1.0]));
                }
                pcurves.push(([start, 1.0], [end, 1.0]));
                if rims[next].is_some() {
                    pcurves.push(([end, 1.0], [end, 0.0]));
                }
                pcurves.push(([end, 0.0], [start, 0.0]));
                add_ring_with_pcurves(body, face, &circuit, &pcurves)?;
            } else if let Some(parameters) = revolved_arc {
                let (start, end) = if senses[index] {
                    (parameters.start, parameters.end)
                } else {
                    (parameters.end, parameters.start)
                };
                let near = parameters.around;
                let far = near + TAU;
                let mut pcurves = Vec::new();
                if rims[index].is_some() {
                    pcurves.push(([near, start], [far, start]));
                }
                pcurves.push(([far, start], [far, end]));
                if rims[next].is_some() {
                    pcurves.push(([far, end], [near, end]));
                }
                pcurves.push(([near, end], [near, start]));
                add_ring_with_pcurves(body, face, &circuit, &pcurves)?;
            } else {
                add_ring(body, face, &circuit)?;
            }
        }
        body.shells.get_mut(shell)?.faces.push(face);
    }
    Some(())
}

/// Where a piece begins, in the direction the loop runs it.
fn profile_corner(profile: &[Curve2], senses: &[bool], index: usize) -> [f64; 2] {
    profile[index].point_at(if senses[index] { 0.0 } else { 1.0 })
}

/// One profile piece as an edge on `plane`, between vertices already made.
fn add_profile_edge(
    body: &mut Body,
    plane: &Plane,
    piece: &Curve2,
    from: VertexKey,
    to: VertexKey,
) -> Option<EdgeKey> {
    let (curve, start, end) = lift_piece(plane, piece)?;
    let curve = body.curves.insert(curve);
    Some(body.edges.insert(Edge {
        curve,
        start_parameter: start,
        end_parameter: end,
        start: from,
        end: to,
        coedges: Vec::new(),
        provenance: Provenance::Synthesized,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_geom::geom2d::{Arc, Line};

    fn square(size: f64) -> Vec<Curve2> {
        let corners = [[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]];
        (0..4)
            .map(|index| {
                Curve2::Line(Line {
                    start: corners[index],
                    end: corners[(index + 1) % 4],
                })
            })
            .collect()
    }

    #[cfg(feature = "offset")]
    #[test]
    fn an_open_surface_rejects_invalid_taper_angles() {
        let profile = [Curve2::Line(Line {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
        })];
        for angle in [
            std::f64::consts::FRAC_PI_2,
            -std::f64::consts::FRAC_PI_2,
            f64::NAN,
        ] {
            assert!(extrude_surface_tapered(Plane::XY, &profile, [0.0, 0.0, 5.0], angle)
                .is_none());
        }
    }

    #[cfg(feature = "offset")]
    #[test]
    fn a_tapered_profile_stops_before_inverting() {
        assert!(extrude_tapered(
            Plane::XY,
            &square(100.0),
            [0.0, 0.0, 1.0],
            45.0_f64.to_radians(),
        )
        .is_some());

        let profile = square(10.0);
        assert!(extrude_tapered(
            Plane::XY,
            &profile,
            [0.0, 0.0, 10.0],
            20.0_f64.to_radians(),
        )
        .is_some());
        assert!(extrude_tapered(
            Plane::XY,
            &profile,
            [0.0, 0.0, 10.0],
            30.0_f64.to_radians(),
        )
        .is_none());
        assert!(extrude_tapered(
            Plane::XY,
            &profile,
            [0.0, 0.0, 10.0],
            -45.0_f64.to_radians(),
        )
        .is_some());
    }

    #[test]
    fn a_square_extruded_is_a_box() {
        let solid = extrude(Plane::XY, &square(10.0), [0.0, 0.0, 4.0]).expect("a prism");
        assert_eq!(solid.faces.len(), 6, "two caps and four walls");
        assert_eq!(solid.edges.len(), 12);
        assert_eq!(solid.vertices.len(), 8);
        assert_eq!(solid.euler_characteristic(), 2);
        let flaws = solid.validate();
        assert!(flaws.is_empty(), "{flaws:?}");
    }

    #[test]
    fn every_edge_of_an_extrusion_is_shared_the_two_ways() {
        let solid = extrude(Plane::XY, &square(5.0), [0.0, 0.0, 3.0]).unwrap();
        for (key, edge) in solid.edges.iter() {
            assert_eq!(edge.coedges.len(), 2, "edge {key:?}");
            let senses: Vec<bool> = edge
                .coedges
                .iter()
                .map(|c| solid.coedges.get(*c).unwrap().forward)
                .collect();
            assert_ne!(senses[0], senses[1], "edge {key:?} runs one way twice");
        }
    }

    #[test]
    fn an_extrusion_reaches_from_its_profile_to_the_far_end() {
        let solid = extrude(Plane::XY, &square(10.0), [0.0, 0.0, 4.0]).unwrap();
        let bounds = crate::body_bounds(&solid).unwrap();
        assert_eq!(bounds.min, [0.0, 0.0, 0.0]);
        assert_eq!(bounds.max, [10.0, 10.0, 4.0]);
    }

    #[test]
    fn every_wall_faces_out() {
        let solid = extrude(Plane::XY, &square(10.0), [0.0, 0.0, 4.0]).unwrap();
        let mesh = crate::mesh::body(
            &solid,
            ocs_geom::tessellation::DEFAULT_ANGLE,
            1e-9,
        );
        let centre = Vec3::new(5.0, 5.0, 2.0);
        for triangle in &mesh.triangles {
            let corner = Vec3::from(mesh.positions[triangle[0]]);
            let normal = Vec3::from(mesh.normals[triangle[0]]);
            assert!(
                normal.dot(corner - centre) > 0.0,
                "a face pointed inwards at {corner:?}"
            );
        }
    }

    /// How much a meshed solid encloses, by the divergence theorem.
    ///
    /// The check that a solid is not inside out, and the one worth making:
    /// it comes back negative when the faces are wound inwards, and — unlike
    /// comparing each normal against a point known to be inside — it holds
    /// for a shape that is not convex. A slice of a tube has an inner wall
    /// whose normal leads *towards* the middle of the material, and a point
    /// test calls that a fault when it is the whole idea.
    ///
    /// It also says how much, so a test can check the shape as well as the
    /// direction.
    fn volume(solid: &Body) -> f64 {
        let mesh = crate::mesh::body(solid, ocs_geom::tessellation::DEFAULT_ANGLE, 1e-9);
        assert!(!mesh.is_empty(), "nothing meshed");
        mesh.triangles
            .iter()
            .map(|triangle| {
                let corner = |index: usize| Vec3::from(mesh.positions[triangle[index]]);
                corner(0).cross(corner(1)).dot(corner(2)) / 6.0
            })
            .sum()
    }

    /// Whether a measured volume is the one expected, to within what the
    /// tessellation's own flat facets lose.
    fn measures(got: f64, want: f64) -> bool {
        (got - want).abs() <= 0.01 * want.abs()
    }

    #[test]
    fn a_sweep_against_the_planes_normal_is_not_inside_out() {
        // The same box, made by pointing the profile's frame the other way and
        // sweeping backwards through it. Nothing about the solid should differ
        // — but every cap and wall takes its direction from the frame, so a
        // builder that assumes the sweep runs along the normal turns all six
        // of them round and produces a box that is inside out.
        let plane = Plane::orthonormal([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0])
            .expect("a downward frame");
        let solid = extrude(plane, &square(10.0), [0.0, 0.0, -4.0]).expect("a prism");
        assert!(solid.validate().is_empty());
        assert_eq!(solid.euler_characteristic(), 2);
        // 10 x 10 x 4, whichever way the frame and the sweep point.
        assert!(measures(volume(&solid), 400.0), "{}", volume(&solid));
    }

    #[test]
    fn a_clockwise_profile_makes_the_same_solid_as_a_counter_clockwise_one() {
        // A face lifted out of a file arrives wound however the file had it,
        // and a boolean hands back loops in whichever order the split left
        // them. Demanding one winding would mean every such caller had to
        // measure and reverse first, and the one that forgot would get a
        // solid that validates and lights black.
        let mut backwards = square(10.0);
        backwards.reverse();
        for piece in &mut backwards {
            let Curve2::Line(line) = piece else {
                unreachable!("a square is straight")
            };
            std::mem::swap(&mut line.start, &mut line.end);
        }
        let solid = extrude(Plane::XY, &backwards, [0.0, 0.0, 4.0]).expect("a prism");
        assert!(solid.validate().is_empty());
        assert_eq!(solid.faces.len(), 6);
        assert!(measures(volume(&solid), 400.0), "{}", volume(&solid));
    }

    #[test]
    fn a_profile_that_encloses_nothing_is_refused() {
        // Out and back along the same line. It closes, it has three pieces,
        // and it bounds no region at all — swept, it would give a body whose
        // walls sit on top of each other.
        let flat = vec![
            Curve2::Line(Line {
                start: [0.0, 0.0],
                end: [10.0, 0.0],
            }),
            Curve2::Line(Line {
                start: [10.0, 0.0],
                end: [4.0, 0.0],
            }),
            Curve2::Line(Line {
                start: [4.0, 0.0],
                end: [0.0, 0.0],
            }),
        ];
        assert!(extrude(Plane::XY, &flat, [0.0, 0.0, 1.0]).is_none());
    }

    /// The one cylindrical wall of a solid, and which way its face runs.
    fn only_cylinder(solid: &Body) -> bool {
        let walls: Vec<bool> = solid
            .faces
            .iter()
            .filter(|(_, face)| {
                matches!(solid.surfaces.get(face.surface), Some(Surface::Cylinder(_)))
            })
            .map(|(_, face)| face.forward)
            .collect();
        assert_eq!(walls.len(), 1, "one arc, one cylinder");
        walls[0]
    }

    /// A 10 × 6 rectangle whose bottom edge carries a semicircle of `radius`,
    /// either bitten into the rectangle or bulging out of it.
    ///
    /// Both run the same way round and meet at the same two points; the
    /// difference is which half of the circle joins them. The notch's half
    /// arches back over the material, so — an [`Arc`] only ever
    /// parameterising counter-clockwise — it can only be written with its
    /// ends the other way round.
    fn bitten(radius: f64, notch: bool) -> Vec<Curve2> {
        use std::f64::consts::{PI, TAU};
        let (left, right) = ([5.0 - radius, 0.0], [5.0 + radius, 0.0]);
        let arc = Curve2::Arc(Arc {
            centre: [5.0, 0.0],
            radius,
            // Over the top for the notch, which the loop then walks
            // backwards; under the bottom for the bump, walked forwards.
            start_angle: if notch { 0.0 } else { PI },
            end_angle: if notch { PI } else { TAU },
        });
        vec![
            Curve2::Line(Line {
                start: [0.0, 0.0],
                end: left,
            }),
            arc,
            Curve2::Line(Line {
                start: right,
                end: [10.0, 0.0],
            }),
            Curve2::Line(Line {
                start: [10.0, 0.0],
                end: [10.0, 6.0],
            }),
            Curve2::Line(Line {
                start: [10.0, 6.0],
                end: [0.0, 6.0],
            }),
            Curve2::Line(Line {
                start: [0.0, 6.0],
                end: [0.0, 0.0],
            }),
        ]
    }

    #[test]
    fn an_arc_written_backwards_is_followed_rather_than_refused() {
        // `Curve::segments` hands a clockwise bulge back with its ends
        // swapped, because an `Arc` only ever parameterises one way. A
        // profile straight out of a polyline therefore does not run head to
        // tail, and demanding that it did would refuse every bulged outline
        // a drawing has — and refuse every concave arc outright, since there
        // is no other way to write one.
        let solid =
            extrude(Plane::XY, &bitten(2.0, true), [0.0, 0.0, 3.0]).expect("a notched prism");
        assert!(solid.validate().is_empty());
        assert_eq!(solid.euler_characteristic(), 2);
        assert_eq!(solid.faces.len(), 8, "six walls and two caps");
        // The bite is missing rather than added: the solid stays inside the
        // rectangle its straight sides describe.
        let bounds = crate::body_bounds(&solid).unwrap();
        assert!(bounds.min[1] > -1e-9, "{bounds:?}");
    }

    #[test]
    fn a_notch_sweeps_into_a_wall_facing_the_other_way_from_a_bump() {
        // The same cylinder either way; what differs is which side of it the
        // material is on. Cut into the profile it lies outside, and the face
        // has to say so — a wall that always agrees with its own surface
        // lights the notch inside out.
        let notched = extrude(Plane::XY, &bitten(2.0, true), [0.0, 0.0, 3.0]).unwrap();
        let bumped = extrude(Plane::XY, &bitten(2.0, false), [0.0, 0.0, 3.0]).unwrap();
        assert!(bumped.validate().is_empty());
        assert!(only_cylinder(&bumped), "a bump agrees with its cylinder");
        assert!(!only_cylinder(&notched), "a notch runs against it");
        // And the bump really does stand out past the straight sides.
        let bounds = crate::body_bounds(&bumped).unwrap();
        assert!((bounds.min[1] + 2.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn a_profile_with_an_arc_in_it_sweeps_into_a_cylinder_wall() {
        // A square with one side bowed out, which is the shape a slot or a
        // rounded end is.
        let profile = vec![
            Curve2::Line(Line {
                start: [0.0, 0.0],
                end: [10.0, 0.0],
            }),
            Curve2::Arc(Arc {
                centre: [10.0, 5.0],
                radius: 5.0,
                start_angle: -std::f64::consts::FRAC_PI_2,
                end_angle: std::f64::consts::FRAC_PI_2,
            }),
            Curve2::Line(Line {
                start: [10.0, 10.0],
                end: [0.0, 10.0],
            }),
            Curve2::Line(Line {
                start: [0.0, 10.0],
                end: [0.0, 0.0],
            }),
        ];
        let solid = extrude(Plane::XY, &profile, [0.0, 0.0, 3.0]).expect("a bowed prism");
        assert!(solid.validate().is_empty());
        let cylinders = solid
            .surfaces
            .iter()
            .filter(|(_, s)| matches!(s, Surface::Cylinder(_)))
            .count();
        assert_eq!(cylinders, 1, "the bowed side became a cylinder");
        // And it reaches past where the straight sides do.
        let bounds = crate::body_bounds(&solid).unwrap();
        assert!((bounds.max[0] - 15.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn an_extrusion_on_a_tilted_plane_goes_where_the_plane_points() {
        let plane = Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let solid = extrude(plane, &square(2.0), [0.0, 5.0, 0.0]).expect("a tilted prism");
        assert!(solid.validate().is_empty());
        let bounds = crate::body_bounds(&solid).unwrap();
        assert!((bounds.max[1] - 5.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn a_sweep_along_the_profiles_own_plane_is_refused() {
        // It would drag the outline through itself and enclose nothing.
        assert!(extrude(Plane::XY, &square(10.0), [1.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn a_profile_that_does_not_close_is_refused() {
        let open = vec![
            Curve2::Line(Line {
                start: [0.0, 0.0],
                end: [10.0, 0.0],
            }),
            Curve2::Line(Line {
                start: [10.0, 0.0],
                end: [10.0, 10.0],
            }),
            Curve2::Line(Line {
                start: [10.0, 10.0],
                end: [5.0, 20.0],
            }),
        ];
        assert!(extrude(Plane::XY, &open, [0.0, 0.0, 1.0]).is_none());
        assert!(extrude(Plane::XY, &square(1.0)[..2], [0.0, 0.0, 1.0]).is_none());
    }

    #[test]
    fn a_spline_profile_extrudes_as_an_exact_nurbs_wall() {
        let profile = vec![
            Curve2::Nurbs(
                ocs_geom::geom2d::NurbsCurve::new(
                    2,
                    vec![[0.0, 0.0], [5.0, 5.0], [10.0, 0.0]],
                    Vec::new(),
                    None,
                )
                .unwrap(),
            ),
            Curve2::Line(Line {
                start: [10.0, 0.0],
                end: [0.0, 0.0],
            }),
        ];
        let solid = extrude(Plane::XY, &profile, [0.0, 0.0, 1.0])
            .expect("an exact spline extrusion");
        assert!(solid.validate().is_empty());
        assert!(solid
            .surfaces
            .iter()
            .any(|(_, surface)| surface.is_spline()));
    }

    #[test]
    fn an_extrusion_at_survey_coordinates_is_the_same_solid() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let plane = Plane::from_axes(origin, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let solid = extrude(plane, &square(0.5), [0.0, 0.0, 0.5]).expect("a small prism");
        assert!(solid.validate().is_empty());
        assert_eq!(solid.euler_characteristic(), 2);
        assert!(solid.worst_vertex_gap() < 1e-6);
    }

    // ── Revolution ──────────────────────────────────────────────────────

    /// The XZ plane, which is where a profile turning about Z belongs: `u` is
    /// the radius and `v` the height.
    fn half_plane() -> Plane {
        Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]).unwrap()
    }

    /// A closed chain through `points`, given as `(radius, height)`.
    fn ring(points: &[[f64; 2]]) -> Vec<Curve2> {
        (0..points.len())
            .map(|index| {
                Curve2::Line(Line {
                    start: points[index],
                    end: points[(index + 1) % points.len()],
                })
            })
            .collect()
    }

    fn kinds(solid: &Body) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = solid
            .faces
            .iter()
            .map(|(_, face)| match solid.surfaces.get(face.surface) {
                Some(Surface::Plane(_)) => "plane",
                Some(Surface::Cylinder(_)) => "cylinder",
                Some(Surface::Cone(_)) => "cone",
                Some(Surface::Sphere(_)) => "sphere",
                Some(Surface::Torus(_)) => "torus",
                Some(Surface::Nurbs(_)) => "nurbs",
                Some(Surface::Offset(_)) => "offset",
                None => "missing",
            })
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn a_rectangle_turned_the_whole_way_is_a_cylinder() {
        // The shape ACIS gives one, arrived at rather than written out: two
        // discs and a wall, three edges, two vertices. The piece of the
        // profile lying on the axis sweeps into nothing and leaves nothing
        // behind — no face, and no vertex at the centre of either disc.
        let profile = ring(&[[0.0, 0.0], [3.0, 0.0], [3.0, 6.0], [0.0, 6.0]]);
        let solid = revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU)
            .expect("a cylinder");
        assert_eq!(solid.faces.len(), 3, "two discs and a wall");
        assert_eq!(solid.edges.len(), 3, "two rims and a seam");
        assert_eq!(solid.vertices.len(), 2);
        assert_eq!(solid.euler_characteristic(), 2);
        assert_eq!(kinds(&solid), ["cylinder", "plane", "plane"]);
        assert!(solid.validate().is_empty());
    }

    #[test]
    fn the_whole_way_round_builds_the_primitives_at_their_own_size() {
        // The check that the topology above is not merely consistent but the
        // right shape. A face left out, or one turned inside out, changes
        // what the mesh encloses; matching the closed form for each says it
        // did neither.
        let pi = std::f64::consts::PI;
        let cases: [(Vec<Curve2>, f64); 3] = [
            // A cylinder, radius 3 by 6 high.
            (
                ring(&[[0.0, 0.0], [3.0, 0.0], [3.0, 6.0], [0.0, 6.0]]),
                pi * 9.0 * 6.0,
            ),
            // A cone, radius 5 by 12 high.
            (ring(&[[0.0, 0.0], [5.0, 0.0], [0.0, 12.0]]), pi * 25.0 * 12.0 / 3.0),
            // A ring of square section: outer 7, inner 4, two tall. Pappus
            // again — the section's area times its centroid's journey.
            (
                ring(&[[4.0, 0.0], [7.0, 0.0], [7.0, 2.0], [4.0, 2.0]]),
                3.0 * 2.0 * TAU * 5.5,
            ),
        ];
        for (profile, expected) in cases {
            let solid = revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU)
                .expect("a solid of revolution");
            let got = volume(&solid);
            assert!(got > 0.0, "wound inwards: {got}");
            assert!(measures(got, expected), "{got} vs {expected}");
        }
    }

    #[test]
    fn a_triangle_turned_the_whole_way_is_a_cone() {
        let profile = ring(&[[0.0, 0.0], [5.0, 0.0], [0.0, 12.0]]);
        let solid =
            revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU).expect("a cone");
        assert_eq!(solid.faces.len(), 2, "the wall and the base");
        assert_eq!(solid.edges.len(), 2, "the rim and the seam");
        assert_eq!(solid.vertices.len(), 2, "the seam's foot and the apex");
        assert_eq!(solid.euler_characteristic(), 2);
        assert_eq!(kinds(&solid), ["cone", "plane"]);
        assert!(solid.validate().is_empty());
        // The slope really does reach the apex rather than stopping short.
        let cone = solid
            .surfaces
            .iter()
            .find_map(|(_, s)| matches!(s, Surface::Cone(_)).then_some(s))
            .unwrap();
        assert!(cone.contains([0.0, 0.0, 12.0], 1e-9), "the apex is off it");
    }

    #[test]
    fn a_half_disc_turned_the_whole_way_is_a_sphere() {
        // An arc from pole to pole, closed by the diameter along the axis.
        // The diameter sweeps into nothing, so what is left is one face
        // bounded by a seam traversed twice — ACIS's own sphere.
        let profile = vec![
            Curve2::Arc(Arc {
                centre: [0.0, 0.0],
                radius: 4.0,
                start_angle: -std::f64::consts::FRAC_PI_2,
                end_angle: std::f64::consts::FRAC_PI_2,
            }),
            Curve2::Line(Line {
                start: [0.0, 4.0],
                end: [0.0, 0.0],
            }),
            Curve2::Line(Line {
                start: [0.0, 0.0],
                end: [0.0, -4.0],
            }),
        ];
        let solid =
            revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU).expect("a sphere");
        assert_eq!(kinds(&solid), ["sphere"]);
        assert_eq!(solid.edges.len(), 1, "one seam");
        assert_eq!(solid.vertices.len(), 2, "two poles");
        assert_eq!(solid.euler_characteristic(), 2);
        assert!(solid.validate().is_empty());
    }

    #[test]
    fn a_rectangle_held_away_from_the_axis_is_a_ring() {
        // Nothing touches the axis, so every corner traces a rim and every
        // piece becomes a face. The hole through the middle is the point:
        // the two flat faces are annuli, each bounded by an outer rim with
        // an inner one cut out of it.
        let profile = ring(&[[4.0, 0.0], [7.0, 0.0], [7.0, 2.0], [4.0, 2.0]]);
        let solid =
            revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU).expect("a ring");
        assert_eq!(solid.faces.len(), 4);
        assert_eq!(kinds(&solid), ["cylinder", "cylinder", "plane", "plane"]);
        assert!(solid.validate().is_empty());
        let holed = solid
            .faces
            .iter()
            .filter(|(_, face)| face.loops.len() == 2)
            .count();
        assert_eq!(holed, 2, "each flat face has a hole in it");
        let bounds = crate::body_bounds(&solid).unwrap();
        assert!((bounds.max[0] - 7.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn an_arc_held_away_from_the_axis_sweeps_into_a_torus() {
        // A D-shaped section: the round half bulging outwards, the flat side
        // closed against the axis. The arc is what makes the torus; the two
        // straight pieces run parallel to the axis at one radius, so each of
        // them is a cylinder, and the profile is closed by all three.
        let profile = vec![
            Curve2::Arc(Arc {
                centre: [10.0, 0.0],
                radius: 2.0,
                start_angle: -std::f64::consts::FRAC_PI_2,
                end_angle: std::f64::consts::FRAC_PI_2,
            }),
            Curve2::Line(Line {
                start: [10.0, 2.0],
                end: [10.0, 0.0],
            }),
            Curve2::Line(Line {
                start: [10.0, 0.0],
                end: [10.0, -2.0],
            }),
        ];
        let solid = revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU)
            .expect("a D-sectioned ring");
        assert_eq!(kinds(&solid), ["cylinder", "cylinder", "torus"]);
        assert!(solid.validate().is_empty());
        // Not `body_bounds`: a torus face wraps its surface both ways and
        // bulges past every edge bounding it, so the box refuses rather than
        // reporting one that is too small.
        //
        // Pappus: the section's area times how far its centroid travels. A
        // half disc's centroid sits 4r/3π out from the flat side.
        let area = 0.5 * std::f64::consts::PI * 4.0;
        let travel = TAU * (10.0 + 4.0 * 2.0 / (3.0 * std::f64::consts::PI));
        assert!(measures(volume(&solid), area * travel), "{}", volume(&solid));
    }

    #[test]
    fn a_part_turn_is_capped_by_the_profile_at_each_end() {
        // What REVOLVE with an angle produces. The piece on the axis does not
        // move, so both caps run along the same edge rather than each having
        // a copy of it.
        let profile = ring(&[[0.0, 0.0], [3.0, 0.0], [3.0, 6.0], [0.0, 6.0]]);
        let solid = revolve(
            half_plane(),
            &profile,
            [0.0; 3],
            [0.0, 0.0, 1.0],
            std::f64::consts::FRAC_PI_2,
        )
        .expect("a quarter cylinder");
        assert_eq!(solid.faces.len(), 5, "two caps and three lateral faces");
        assert_eq!(solid.euler_characteristic(), 2);
        assert!(solid.validate().is_empty());
        assert_eq!(kinds(&solid), ["cylinder", "plane", "plane", "plane", "plane"]);
        // A quarter turn from the profile reaches +y but not −y.
        let bounds = crate::body_bounds(&solid).unwrap();
        assert!((bounds.max[1] - 3.0).abs() < 1e-9, "{bounds:?}");
        assert!(bounds.min[1] > -1e-9, "{bounds:?}");
    }

    #[test]
    fn a_part_turn_lights_the_right_way_out() {
        let profile = ring(&[[2.0, 0.0], [5.0, 0.0], [5.0, 4.0], [2.0, 4.0]]);
        for angle in [std::f64::consts::FRAC_PI_2, 2.0] {
            let solid = revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], angle)
                .expect("a slice");
            assert!(solid.validate().is_empty(), "{angle}");
            // A slice of a tube: the ring's area times its height times the
            // fraction of a turn it covers. Negative would mean inside out.
            let want = std::f64::consts::PI * (25.0 - 4.0) * 4.0 * angle / TAU;
            assert!(measures(volume(&solid), want), "{angle}: {}", volume(&solid));
        }
    }

    #[test]
    fn turning_the_other_way_gives_the_same_solid_the_other_side() {
        let profile = ring(&[[0.0, 0.0], [3.0, 0.0], [3.0, 6.0], [0.0, 6.0]]);
        let quarter = std::f64::consts::FRAC_PI_2;
        let forwards =
            revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], quarter).unwrap();
        let backwards =
            revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], -quarter).unwrap();
        assert!(backwards.validate().is_empty());
        assert_eq!(forwards.faces.len(), backwards.faces.len());
        let there = crate::body_bounds(&forwards).unwrap();
        let back = crate::body_bounds(&backwards).unwrap();
        assert!((there.max[1] - 3.0).abs() < 1e-9, "{there:?}");
        assert!((back.min[1] + 3.0).abs() < 1e-9, "{back:?}");
    }

    #[test]
    fn a_profile_that_straddles_the_axis_is_refused() {
        // It would sweep through itself, and the result would have an inside
        // that is not a region. Saying so beats handing back a shape whose
        // volume depends on how it is asked.
        let profile = ring(&[[-2.0, 0.0], [3.0, 0.0], [3.0, 4.0], [-2.0, 4.0]]);
        assert!(revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU).is_none());
    }

    #[test]
    fn a_profile_on_the_far_side_of_the_axis_turns_just_the_same() {
        // Zero angle belongs on the profile wherever it is, so the frame
        // turns to meet it rather than the radii coming out negative.
        let profile = ring(&[[-7.0, 0.0], [-4.0, 0.0], [-4.0, 2.0], [-7.0, 2.0]]);
        let solid =
            revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU).expect("a ring");
        assert!(solid.validate().is_empty());
        let bounds = crate::body_bounds(&solid).unwrap();
        assert!((bounds.max[0] - 7.0).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.min[0] + 7.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn an_axis_off_the_profiles_plane_sweeps_rotational_surfaces() {
        // Tilted against the plane, or parallel to it at a distance: each
        // piece sweeps an exact rational surface of revolution.
        let profile = ring(&[[1.0, 0.0], [3.0, 0.0], [3.0, 4.0], [1.0, 4.0]]);
        for (pivot, axis) in [([0.0; 3], [0.0, 1.0, 1.0]), ([0.0, 5.0, 0.0], [0.0, 0.0, 1.0])] {
            let solid = revolve(half_plane(), &profile, pivot, axis, TAU).expect("turned");
            assert!(solid.validate().is_empty(), "{:?}", solid.validate());
            let mass = crate::mass_properties(&solid).expect("mass");
            assert!(mass.volume > 0.0, "{}", mass.volume);
        }
    }

    #[test]
    fn a_spline_profile_revolves_as_an_exact_nurbs_wall() {
        let profile = vec![
            Curve2::Nurbs(
                ocs_geom::geom2d::NurbsCurve::new(
                    2,
                    vec![[1.0, 0.0], [4.0, 2.0], [1.0, 4.0]],
                    Vec::new(),
                    None,
                )
                .unwrap(),
            ),
            Curve2::Line(Line {
                start: [1.0, 4.0],
                end: [1.0, 0.0],
            }),
        ];
        let solid = revolve(half_plane(), &profile, [0.0; 3], [0.0, 0.0, 1.0], TAU)
            .expect("an exact spline revolution");
        assert!(solid.validate().is_empty());
        assert!(solid
            .surfaces
            .iter()
            .any(|(_, surface)| surface.is_spline()));
    }

    #[test]
    fn an_open_profile_revolves_into_a_sheet() {
        let profile = [Curve2::Line(Line {
            start: [2.0, 0.0],
            end: [2.0, 4.0],
        })];
        let sheet = revolve_surface(
            half_plane(),
            &profile,
            [0.0; 3],
            [0.0, 0.0, 1.0],
            std::f64::consts::FRAC_PI_2,
        )
        .expect("a cylindrical sheet");
        assert_eq!(sheet.faces.len(), 1);
        assert!(sheet.validate().is_empty());
    }

    #[test]
    fn a_revolved_region_keeps_its_hole() {
        let outer = ring(&[[2.0, 0.0], [5.0, 0.0], [5.0, 4.0], [2.0, 4.0]]);
        let hole = ring(&[[3.0, 1.0], [4.0, 1.0], [4.0, 3.0], [3.0, 3.0]]);
        let solid = revolve_region(
            half_plane(),
            &[outer, hole],
            [0.0; 3],
            [0.0, 0.0, 1.0],
            TAU,
        )
        .expect("a revolved region with a void");
        assert!(solid.validate().is_empty());
        assert_eq!(solid.roots.len(), 1);
        let shells = &solid.lumps.get(solid.roots[0]).unwrap().shells;
        assert_eq!(shells.len(), 2, "an exterior and a void shell");
        let expected = std::f64::consts::PI * ((25.0 - 4.0) * 4.0 - (16.0 - 9.0) * 2.0);
        assert!(measures(volume(&solid), expected), "{}", volume(&solid));
    }

    #[test]
    fn a_partial_revolved_region_opens_its_hole_through_both_caps() {
        let outer = ring(&[[2.0, 0.0], [5.0, 0.0], [5.0, 4.0], [2.0, 4.0]]);
        let hole = ring(&[[3.0, 1.0], [4.0, 1.0], [4.0, 3.0], [3.0, 3.0]]);
        let solid = revolve_region(
            half_plane(),
            &[outer, hole],
            [0.0; 3],
            [0.0, 0.0, 1.0],
            std::f64::consts::FRAC_PI_2,
        )
        .expect("a partial revolved region with an open hole");
        assert!(solid.validate().is_empty());
        assert_eq!(solid.roots.len(), 1);
    }

    #[test]
    fn every_loop_of_a_surface_region_remains_a_separate_sheet() {
        let outer = ring(&[[2.0, 0.0], [5.0, 0.0], [5.0, 4.0], [2.0, 4.0]]);
        let hole = ring(&[[3.0, 1.0], [4.0, 1.0], [4.0, 3.0], [3.0, 3.0]]);
        let sheets = revolve_surface_region(
            half_plane(),
            &[outer, hole],
            [0.0; 3],
            [0.0, 0.0, 1.0],
            TAU,
        )
        .expect("two revolved sheets");
        assert!(sheets.validate().is_empty());
        assert_eq!(sheets.roots.len(), 2);
    }

    #[test]
    fn a_revolution_at_survey_coordinates_is_the_same_solid() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let plane = Plane::orthonormal(origin, [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]).unwrap();
        let profile = ring(&[[0.0, 0.0], [0.4, 0.0], [0.4, 0.9], [0.0, 0.9]]);
        let solid = revolve(plane, &profile, origin, [0.0, 0.0, 1.0], TAU)
            .expect("a small cylinder");
        assert!(solid.validate().is_empty());
        assert_eq!(solid.euler_characteristic(), 2);
        assert!(solid.worst_vertex_gap() < 1e-6);
    }

}
