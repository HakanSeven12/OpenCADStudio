//! Solid lofts through compatible polygon sections.

use super::geometry::{Curve3, Line3, Surface};
use super::nurbs_builder::{RationalCurve2, RationalCurve3};
use super::topology::{
    Body, Coedge, Edge, EdgeKey, Face, Loop, Lump, Shell, ShellKey, Vertex,
    VertexKey,
};
use super::Provenance;
use ocs_geom::geom2d::{Curve, Line};
use ocs_geom::space::{NurbsSurface3, Plane, Vec3};

/// Builds a closed solid through compatible ordered sections.
pub fn loft(sections: &[(Plane, Vec<Curve>)]) -> Option<Body> {
    if sections
        .iter()
        .all(|(_, profile)| profile.iter().all(|piece| matches!(piece, Curve::Line(_))))
    {
        polygon_loft(sections, true)
    } else {
        curved_loft(sections)
    }
}

pub(crate) fn polygon_loft_ordered(sections: &[(Plane, Vec<Curve>)]) -> Option<Body> {
    polygon_loft(sections, false)
}

fn polygon_loft(sections: &[(Plane, Vec<Curve>)], align_rings: bool) -> Option<Body> {
    if sections.len() < 2 {
        return None;
    }
    let mut rings = Vec::with_capacity(sections.len());
    for (plane, profile) in sections {
        if profile.iter().any(|piece| !matches!(piece, Curve::Line(_))) {
            return None;
        }
        let senses = super::sweep::profile_senses(profile)?;
        let points = profile
            .iter()
            .zip(senses)
            .map(|(piece, forward)| plane.point_at(piece.point_at(if forward { 0.0 } else { 1.0 })))
            .collect::<Vec<_>>();
        if points.len() < 3 {
            return None;
        }
        rings.push(points);
    }
    let count = rings[0].len();
    if rings.iter().any(|ring| ring.len() != count) {
        return None;
    }
    if align_rings {
        for index in 1..rings.len() {
            rings[index] = aligned_ring(&rings[index - 1], &rings[index]);
        }
    }

    let centres = rings.iter().map(|ring| centre(ring)).collect::<Vec<_>>();
    if centres
        .windows(2)
        .any(|pair| (pair[1] - pair[0]).length() <= tolerance(&rings))
    {
        return None;
    }

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
    let vertices = rings
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|point| add_vertex(&mut body, *point))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let rims = vertices
        .iter()
        .map(|ring| {
            (0..count)
                .map(|index| add_line_edge(&mut body, ring[index], ring[(index + 1) % count]))
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    let rails = vertices
        .windows(2)
        .map(|pair| {
            (0..count)
                .map(|index| add_line_edge(&mut body, pair[0][index], pair[1][index]))
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;

    let first_winding = polygon_normal(&rings[0])?;
    let first_outward = (centres[0] - centres[1]).normalize()?;
    let first_normal = if first_winding.dot(first_outward) >= 0.0 {
        first_winding
    } else {
        -first_winding
    };
    let last_winding = polygon_normal(rings.last()?)?;
    let last_index = centres.len() - 1;
    let last_outward = (centres[last_index] - centres[last_index - 1]).normalize()?;
    let last_normal = if last_winding.dot(last_outward) >= 0.0 {
        last_winding
    } else {
        -last_winding
    };
    add_cap(
        &mut body,
        shell,
        &rings[0],
        &rims[0],
        first_normal,
        first_winding.dot(first_normal) > 0.0,
    )?;
    add_cap(
        &mut body,
        shell,
        rings.last()?,
        rims.last()?,
        last_normal,
        last_winding.dot(last_normal) > 0.0,
    )?;

    let scale_tolerance = tolerance(&rings);
    for band in 0..rails.len() {
        for index in 0..count {
            let next = (index + 1) % count;
            let a = Vec3::from(rings[band][index]);
            let b = Vec3::from(rings[band][next]);
            let c = Vec3::from(rings[band + 1][next]);
            let d = Vec3::from(rings[band + 1][index]);
            let normal = (b - a).cross(d - a).normalize()?;
            let circuit = [
                (rims[band][index], true),
                (rails[band][next], true),
                (rims[band + 1][index], false),
                (rails[band][index], false),
            ];
            if (c - a).dot(normal).abs() <= scale_tolerance {
                let plane =
                    Plane::orthonormal(a.to_array(), (b - a).to_array(), normal.to_array())?;
                let surface = body.surfaces.insert(Surface::Plane(plane));
                add_face(&mut body, shell, surface, &circuit)?;
            } else {
                let surface = NurbsSurface3::new(
                    1,
                    1,
                    vec![vec![a.to_array(), d.to_array()], vec![b.to_array(), c.to_array()]],
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    None,
                )?;
                let surface = body.surfaces.insert(Surface::Nurbs(surface));
                add_face_with_pcurves(
                    &mut body,
                    shell,
                    surface,
                    true,
                    &circuit,
                    &[
                        ([0.0, 0.0], [1.0, 0.0]),
                        ([1.0, 0.0], [1.0, 1.0]),
                        ([1.0, 1.0], [0.0, 1.0]),
                        ([0.0, 1.0], [0.0, 0.0]),
                    ],
                )?;
            }
        }
    }

    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    body.validate().is_empty().then_some(body)
}

fn curved_loft(sections: &[(Plane, Vec<Curve>)]) -> Option<Body> {
    if sections.len() < 2 {
        return None;
    }
    let first_senses = super::sweep::profile_senses(&sections[0].1)?;
    let first_area = sections[0]
        .1
        .iter()
        .zip(&first_senses)
        .map(|(piece, forward)| piece.chain_area() * if *forward { 1.0 } else { -1.0 })
        .sum::<f64>();
    if !first_area.is_finite() || first_area.abs() <= 1e-12 {
        return None;
    }

    let mut rings = sections
        .iter()
        .map(|(plane, profile)| {
            let senses = super::sweep::profile_senses(profile)?;
            profile
                .iter()
                .zip(senses)
                .map(|(piece, forward)| {
                    let curve = RationalCurve2::from_curve(piece)?;
                    Some(if forward { curve } else { curve.reversed() }.lifted(plane))
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    let count = rings.first()?.len();
    if count < 3 || rings.iter().any(|ring| ring.len() != count) {
        return None;
    }
    for index in 1..rings.len() {
        rings[index] = aligned_curves(&rings[index - 1], &rings[index]);
    }
    if rings.windows(2).any(|pair| {
        pair[0]
            .iter()
            .zip(&pair[1])
            .any(|(a, b)| !a.compatible_with(b))
    }) {
        return None;
    }

    let points = rings
        .iter()
        .map(|ring| ring.iter().map(|curve| curve.points[0]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let centres = points.iter().map(|ring| centre(ring)).collect::<Vec<_>>();
    let direction = (centres.last().copied()? - centres[0]).normalize()?;
    if centres
        .windows(2)
        .any(|pair| (pair[1] - pair[0]).dot(direction) <= tolerance(&points))
    {
        return None;
    }

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
    let vertices = points
        .iter()
        .map(|ring| ring.iter().map(|point| add_vertex(&mut body, *point)).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let rims = rings
        .iter()
        .zip(&vertices)
        .map(|(ring, vertices)| {
            (0..count)
                .map(|index| {
                    add_nurbs_edge(
                        &mut body,
                        &ring[index],
                        vertices[index],
                        vertices[(index + 1) % count],
                    )
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    let rails = vertices
        .windows(2)
        .map(|pair| {
            (0..count)
                .map(|index| add_line_edge(&mut body, pair[0][index], pair[1][index]))
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;

    let winding = Vec3::from(sections[0].0.normal()?) * first_area.signum();
    let forward = winding.dot(direction) > 0.0;
    let first_normal = Vec3::from(sections[0].0.normal()?);
    let first_normal = if first_normal.dot(direction) > 0.0 { -first_normal } else { first_normal };
    let last_normal = Vec3::from(sections.last()?.0.normal()?);
    let last_normal = if last_normal.dot(direction) > 0.0 { last_normal } else { -last_normal };
    add_cap(&mut body, shell, &points[0], &rims[0], first_normal, !forward)?;
    add_cap(
        &mut body,
        shell,
        points.last()?,
        rims.last()?,
        last_normal,
        forward,
    )?;

    for band in 0..rails.len() {
        for index in 0..count {
            let next = (index + 1) % count;
            let spline = rings[band][index].ruled_to(&rings[band + 1][index])?;
            let surface = Surface::Nurbs(spline);
            let on = Vec3::from(surface.point_at(0.5, 0.5));
            let middle = (centres[band] + centres[band + 1]) * 0.5;
            let normal = Vec3::from(surface.normal_at(0.5, 0.5)?);
            let outward = on - middle;
            let sense = normal.dot(outward);
            if !sense.is_finite() || sense == 0.0 {
                return None;
            }
            let surface = body.surfaces.insert(surface);
            add_face_with_pcurves(
                &mut body,
                shell,
                surface,
                sense > 0.0,
                &[
                    (rims[band][index], true),
                    (rails[band][next], true),
                    (rims[band + 1][index], false),
                    (rails[band][index], false),
                ],
                &[
                    ([0.0, 0.0], [1.0, 0.0]),
                    ([1.0, 0.0], [1.0, 1.0]),
                    ([1.0, 1.0], [0.0, 1.0]),
                    ([0.0, 1.0], [0.0, 0.0]),
                ],
            )?;
        }
    }

    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    body.validate().is_empty().then_some(body)
}

fn aligned_curves(previous: &[RationalCurve3], ring: &[RationalCurve3]) -> Vec<RationalCurve3> {
    let mut best = ring.to_vec();
    let mut best_cost = f64::INFINITY;
    for reversed in [false, true] {
        for shift in 0..ring.len() {
            let candidate = (0..ring.len())
                .map(|index| {
                    if reversed {
                        ring[(shift + ring.len() - 1 - index) % ring.len()].reversed()
                    } else {
                        ring[(shift + index) % ring.len()].clone()
                    }
                })
                .collect::<Vec<_>>();
            let cost = previous
                .iter()
                .zip(&candidate)
                .map(|(a, b)| {
                    let delta = Vec3::from(a.points[0]) - Vec3::from(b.points[0]);
                    delta.dot(delta)
                })
                .sum::<f64>();
            if cost < best_cost {
                best = candidate;
                best_cost = cost;
            }
        }
    }
    best
}

fn aligned_ring(previous: &[[f64; 3]], ring: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let mut best = ring.to_vec();
    let mut best_cost = f64::INFINITY;
    for reversed in [false, true] {
        for shift in 0..ring.len() {
            let candidate = (0..ring.len())
                .map(|index| {
                    let at = if reversed {
                        (shift + ring.len() - index) % ring.len()
                    } else {
                        (shift + index) % ring.len()
                    };
                    ring[at]
                })
                .collect::<Vec<_>>();
            let cost = previous
                .iter()
                .zip(&candidate)
                .map(|(a, b)| {
                    let delta = Vec3::from(*a) - Vec3::from(*b);
                    delta.dot(delta)
                })
                .sum::<f64>();
            if cost < best_cost {
                best = candidate;
                best_cost = cost;
            }
        }
    }
    best
}

fn centre(points: &[[f64; 3]]) -> Vec3 {
    points
        .iter()
        .fold(Vec3::ZERO, |sum, point| sum + Vec3::from(*point))
        / points.len() as f64
}

fn tolerance(rings: &[Vec<[f64; 3]>]) -> f64 {
    let scale = rings
        .iter()
        .flatten()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0_f64, f64::max);
    scale * 1e-9
}

fn polygon_normal(points: &[[f64; 3]]) -> Option<Vec3> {
    let origin = Vec3::from(*points.first()?);
    for index in 1..points.len() - 1 {
        let normal = (Vec3::from(points[index]) - origin)
            .cross(Vec3::from(points[index + 1]) - origin);
        if let Some(normal) = normal.normalize() {
            return Some(normal);
        }
    }
    None
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
    if direction.length() <= 1e-12 {
        return None;
    }
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
    source: &RationalCurve3,
    start: VertexKey,
    end: VertexKey,
) -> Option<EdgeKey> {
    let curve = body.curves.insert(Curve3::Nurbs(source.curve()?));
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

fn add_cap(
    body: &mut Body,
    shell: ShellKey,
    points: &[[f64; 3]],
    edges: &[EdgeKey],
    normal: Vec3,
    along_edges: bool,
) -> Option<()> {
    let x = Vec3::from(points[1]) - Vec3::from(points[0]);
    let plane = Plane::orthonormal(points[0], x.to_array(), normal.to_array())?;
    let surface = body.surfaces.insert(Surface::Plane(plane));
    let circuit = if along_edges {
        edges.iter().map(|edge| (*edge, true)).collect::<Vec<_>>()
    } else {
        edges.iter().rev().map(|edge| (*edge, false)).collect::<Vec<_>>()
    };
    add_face(body, shell, surface, &circuit)
}

fn add_face(
    body: &mut Body,
    shell: ShellKey,
    surface: super::SurfaceKey,
    circuit: &[(EdgeKey, bool)],
) -> Option<()> {
    let face = body.faces.insert(Face {
        surface,
        forward: true,
        loops: Vec::new(),
        owner: shell,
        provenance: Provenance::Synthesized,
    });
    let ring = body.loops.insert(Loop {
        coedges: Vec::new(),
        owner: face,
        provenance: Provenance::Synthesized,
    });
    let mut coedges = Vec::with_capacity(circuit.len());
    for (edge, forward) in circuit {
        let coedge = body.coedges.insert(Coedge {
            edge: *edge,
            forward: *forward,
            pcurve: None,
            owner: ring,
            provenance: Provenance::Synthesized,
        });
        body.edges.get_mut(*edge)?.coedges.push(coedge);
        coedges.push(coedge);
    }
    body.loops.get_mut(ring)?.coedges = coedges;
    body.faces.get_mut(face)?.loops = vec![ring];
    body.shells.get_mut(shell)?.faces.push(face);
    Some(())
}

fn add_face_with_pcurves(
    body: &mut Body,
    shell: ShellKey,
    surface: super::SurfaceKey,
    forward: bool,
    circuit: &[(EdgeKey, bool)],
    pcurves: &[([f64; 2], [f64; 2])],
) -> Option<()> {
    if circuit.len() != pcurves.len() {
        return None;
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
    let mut coedges = Vec::with_capacity(circuit.len());
    for ((edge, forward), (start, end)) in circuit.iter().zip(pcurves) {
        let coedge = body.coedges.insert(Coedge {
            edge: *edge,
            forward: *forward,
            pcurve: Some(Curve::Line(Line {
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
    Some(())
}
