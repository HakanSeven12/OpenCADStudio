//! Writing a Solidcore record into a document as new ACIS records.
//!
//! Records are allocated before their circular topology pointers are filled. Analytic faces are
//! written on their exact ACIS surfaces (a plane, cone, sphere or torus) without pcurves; spline
//! faces on their splines, with each use's pcurve put on the edge's ACIS parameter range.

use std::collections::HashMap;
use std::f64::consts::TAU;

use document::entities::acis::types::{SatDocument, SatPointer, SatRecord, SatToken};
use solidcore_record::brep as r;

use crate::geometry::{cross, dot, length, scale, sub, turn_near, unit};

/// Why a body could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unappendable {
    /// A curve has no SAT record form.
    Curve,
    /// A surface whose frame is degenerate, so there is no normal to write.
    Surface,
    /// The record's own topology does not hold together.
    Inconsistent,
}

/// Where the body ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Record index of the `body` record.
    pub body: i32,
    /// How many records were added in total.
    pub records: usize,
}

const NULL: i32 = -1;

/// Appends `brep` to `document` as a fresh set of records beside what it already has.
pub fn append(brep: &r::Brep, document: &mut SatDocument) -> Result<Written, Unappendable> {
    if solidcore_ops::record_check::validate(brep).is_err() {
        return Err(Unappendable::Inconsistent);
    }
    let before = document.record_count();

    // Each shell once, under the material region it bounds; open shells (sheets) each a lump.
    let mut lumps: Vec<Vec<usize>> = Vec::new();
    // Whether each lump is material (a region's shells) or a sheet, whose faces are two-sided.
    let mut material: Vec<bool> = Vec::new();
    let mut placed = vec![false; brep.shells.len()];
    for region in brep.regions.iter().skip(1).filter(|region| !region.void) {
        let shells: Vec<usize> = region
            .shells
            .iter()
            .map(|s| *s as usize)
            .filter(|s| matches!(brep.shells.get(*s), Some(r::Shell::Faces { .. })) && !placed[*s])
            .collect();
        for s in &shells {
            placed[*s] = true;
        }
        if !shells.is_empty() {
            lumps.push(shells);
            material.push(true);
        }
    }
    for (s, shell) in brep.shells.iter().enumerate() {
        if !placed[s] && matches!(shell, r::Shell::Faces { .. }) {
            placed[s] = true;
            lumps.push(vec![s]);
            material.push(false);
        }
    }
    if lumps.is_empty() {
        return Err(Unappendable::Inconsistent);
    }
    // The side each face is used on, by the first shell that uses it.
    let mut face_shell: Vec<Option<(usize, bool)>> = vec![None; brep.faces.len()];
    for shells in &lumps {
        for &s in shells {
            if let Some(r::Shell::Faces { faces, .. }) = brep.shells.get(s) {
                for f in faces {
                    if let Some(slot) = face_shell.get_mut(f.face as usize) {
                        slot.get_or_insert((s, f.forward));
                    }
                }
            }
        }
    }

    // First pass: every record, with geometry but no pointers.
    let points: Vec<i32> = brep
        .vertices
        .iter()
        .map(|v| add(document, "point", vec![position(v.point)]))
        .collect();
    let vertices: Vec<i32> = brep.vertices.iter().map(|_| add(document, "vertex", Vec::new())).collect();
    let mut edge_params = Vec::with_capacity(brep.edges.len());
    let mut curves = Vec::with_capacity(brep.edges.len());
    for edge in &brep.edges {
        let (id, params) = add_edge_curve(document, brep, edge).ok_or(Unappendable::Curve)?;
        curves.push(id);
        edge_params.push(params);
    }
    let edges: Vec<i32> = brep.edges.iter().map(|_| add(document, "edge", Vec::new())).collect();
    let mut surfaces = Vec::with_capacity(brep.faces.len());
    let mut senses = Vec::with_capacity(brep.faces.len());
    for (f, face) in brep.faces.iter().enumerate() {
        let (id, spline) = add_surface(document, face).ok_or(Unappendable::Surface)?;
        surfaces.push((id, spline));
        // The face's side against its ACIS surface's normal: the shell's side is about the
        // surface its (u, v) are on, whose normal an analytic surface's need not share.
        let side = face_shell[f].map_or(true, |(_, forward)| forward);
        senses.push(side == crate::geometry::form_agrees(face));
    }

    // Each face's loops as ACIS runs them: counter-clockwise about the face's normal, the
    // record's about the natural normal of the surface its (u, v) are on.
    struct Use {
        edge: usize,
        forward: bool,
        pcurve: Option<r::Curve2>,
    }
    let mut face_loops: Vec<Vec<Vec<Use>>> = Vec::with_capacity(brep.faces.len());
    for (f, face) in brep.faces.iter().enumerate() {
        let side = face_shell[f].map_or(true, |(_, forward)| forward);
        let mut rings = Vec::new();
        for ring in &face.loops {
            let r::Loop::Edges { edges: uses } = ring else { continue };
            let mut ring: Vec<Use> = uses
                .iter()
                .map(|u| Use {
                    edge: u.edge as usize,
                    forward: u.forward,
                    pcurve: Some(u.pcurve.clone()),
                })
                .collect();
            if !side {
                ring.reverse();
                for u in &mut ring {
                    u.forward = !u.forward;
                }
            }
            rings.push(ring);
        }
        face_loops.push(rings);
    }

    let mut coedges: Vec<Vec<Vec<i32>>> = Vec::new();
    let mut pcurves: Vec<Vec<Vec<i32>>> = Vec::new();
    let mut edge_coedges: Vec<Vec<i32>> = vec![Vec::new(); brep.edges.len()];
    for (f, rings) in face_loops.iter().enumerate() {
        let (surface_id, spline) = surfaces[f];
        let mut face_coedges = Vec::new();
        let mut face_pcurves = Vec::new();
        for ring in rings {
            let mut ids = Vec::new();
            let mut pcs = Vec::new();
            for u in ring {
                let pc = if spline {
                    u.pcurve
                        .as_ref()
                        .and_then(|pc| acis_pcurve(pc, edge_params[u.edge], u.forward))
                        .and_then(|(degree, knots, controls, weights)| {
                            let rational = weights.iter().any(|w| (w - 1.0).abs() > 1e-14);
                            let closed = controls.first() == controls.last();
                            Some(document.add_pcurve(
                                rational,
                                degree as i32,
                                closed,
                                &distinct_knots(&knots),
                                &controls,
                                rational.then_some(weights.as_slice()),
                                0.0,
                                surface_id,
                                (0.0, 0.0),
                            ))
                        })
                        .unwrap_or(NULL)
                } else {
                    NULL
                };
                pcs.push(pc);
                let id = add(document, "coedge", Vec::new());
                edge_coedges[u.edge].push(id);
                ids.push(id);
            }
            face_coedges.push(ids);
            face_pcurves.push(pcs);
        }
        coedges.push(face_coedges);
        pcurves.push(face_pcurves);
    }
    let loops: Vec<Vec<i32>> = face_loops
        .iter()
        .map(|rings| rings.iter().map(|_| add(document, "loop", Vec::new())).collect())
        .collect();
    let faces: Vec<i32> = brep.faces.iter().map(|_| add(document, "face", Vec::new())).collect();
    let shells: HashMap<usize, i32> = lumps
        .iter()
        .flatten()
        .map(|s| (*s, add(document, "shell", Vec::new())))
        .collect();
    let lump_ids: Vec<i32> = lumps.iter().map(|_| add(document, "lump", Vec::new())).collect();
    let body_id = add(document, "body", Vec::new());

    // Second pass: the pointers, now that every index exists.
    for (v, id) in vertices.iter().enumerate() {
        let edge = brep
            .edges
            .iter()
            .position(|e| e.vertices.contains(&(v as u32)))
            .map_or(NULL, |e| edges[e]);
        set(document, *id, vec![null(), pointer(edge), pointer(points[v])]);
    }
    for (e, edge) in brep.edges.iter().enumerate() {
        let [a, b] = edge_params[e];
        let first = edge_coedges[e].first().copied().unwrap_or(NULL);
        set(
            document,
            edges[e],
            vec![
                null(),
                pointer(vertices[edge.vertices[0] as usize]),
                SatToken::Float(a),
                pointer(vertices[edge.vertices[1] as usize]),
                SatToken::Float(b),
                pointer(first),
                pointer(curves[e]),
                sense(true),
            ],
        );
    }
    // A coedge's partner is the next use of its edge, round the edge.
    let partner = |id: i32, edge: usize| -> i32 {
        let around = &edge_coedges[edge];
        match around.iter().position(|c| *c == id) {
            Some(at) if around.len() > 1 => around[(at + 1) % around.len()],
            _ => NULL,
        }
    };
    for (f, rings) in face_loops.iter().enumerate() {
        for (l, ring) in rings.iter().enumerate() {
            let ids = &coedges[f][l];
            let count = ids.len();
            for (k, u) in ring.iter().enumerate() {
                set(
                    document,
                    ids[k],
                    vec![
                        null(),
                        pointer(ids[(k + 1) % count]),
                        pointer(ids[(k + count - 1) % count]),
                        pointer(partner(ids[k], u.edge)),
                        pointer(edges[u.edge]),
                        sense(u.forward),
                        pointer(loops[f][l]),
                        pointer(pcurves[f][l][k]),
                    ],
                );
            }
            set(
                document,
                loops[f][l],
                vec![
                    null(),
                    pointer(loops[f].get(l + 1).copied().unwrap_or(NULL)),
                    pointer(ids.first().copied().unwrap_or(NULL)),
                    pointer(faces[f]),
                ],
            );
        }
    }
    for (lump, shell_list) in lumps.iter().enumerate() {
        for (k, &s) in shell_list.iter().enumerate() {
            let Some(r::Shell::Faces { faces: used, .. }) = brep.shells.get(s) else { continue };
            // A face is written in the first shell that uses it.
            let mine: Vec<usize> = used
                .iter()
                .map(|f| f.face as usize)
                .filter(|f| face_shell[*f].is_some_and(|(owner, _)| owner == s))
                .collect();
            for (i, &f) in mine.iter().enumerate() {
                let mut tokens = vec![
                    null(),
                    pointer(mine.get(i + 1).map_or(NULL, |next| faces[*next])),
                    pointer(loops[f].first().copied().unwrap_or(NULL)),
                    pointer(shells[&s]),
                    null(),
                    pointer(surfaces[f].0),
                    sense(senses[f]),
                ];
                if material[lump] {
                    tokens.push(SatToken::Ident("single".to_string()));
                } else {
                    tokens.push(SatToken::Ident("double".to_string()));
                    tokens.push(SatToken::Ident("out".to_string()));
                }
                set(document, faces[f], tokens);
            }
            set(
                document,
                shells[&s],
                vec![
                    null(),
                    pointer(shell_list.get(k + 1).map_or(NULL, |next| shells[next])),
                    null(),
                    pointer(mine.first().map_or(NULL, |f| faces[*f])),
                    null(),
                    pointer(lump_ids[lump]),
                ],
            );
        }
        set(
            document,
            lump_ids[lump],
            vec![
                null(),
                pointer(lump_ids.get(lump + 1).copied().unwrap_or(NULL)),
                pointer(shell_list.first().map_or(NULL, |s| shells[s])),
                pointer(body_id),
            ],
        );
    }
    set(document, body_id, vec![null(), pointer(lump_ids[0]), null(), null()]);

    Ok(Written {
        body: body_id,
        records: document.record_count() - before,
    })
}

/// The SAT curve record of an edge, and the edge's ACIS parameter range on it.
fn add_edge_curve(document: &mut SatDocument, brep: &r::Brep, edge: &r::Edge) -> Option<(i32, [f64; 2])> {
    let ends = edge.vertices.map(|v| brep.vertices.get(v as usize).map(|v| v.point));
    let [Some(start), Some(end)] = ends else { return None };
    let [a, b] = edge.interval;
    match &edge.curve {
        r::Curve::Line { start: s, end: e } => {
            let span = sub(*e, *s);
            let l = length(span);
            let direction = unit(span)?;
            let id = add(document, "straight-curve", vec![position(*s), position(direction)]);
            // On a line's NURBS form the interval is on the form; the vertices say where.
            let (a, b) = if edge.nurbs.is_some() {
                (dot(sub(start, *s), direction), dot(sub(end, *s), direction))
            } else {
                (a * l, b * l)
            };
            Some((id, [a, b]))
        }
        r::Curve::Circle { frame, radius } => {
            ellipse_edge(document, frame, [*radius, *radius], edge, start, end)
        }
        r::Curve::Ellipse { frame, radii } => ellipse_edge(document, frame, *radii, edge, start, end),
        r::Curve::Spline { knots, points } => {
            let id = document.add_spline_curve(false, degree(knots.len(), points.len())? as i32, false, &distinct_knots(knots), points, None, 0.0);
            Some((id, [a, b]))
        }
        r::Curve::RationalSpline { knots, homogeneous } => {
            let points: Vec<[f64; 3]> = homogeneous.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).collect();
            let weights: Vec<f64> = homogeneous.iter().map(|h| h[3]).collect();
            let id = document.add_spline_curve(true, degree(knots.len(), points.len())? as i32, false, &distinct_knots(knots), &points, Some(&weights), 0.0);
            Some((id, [a, b]))
        }
        r::Curve::Polyline { points } => {
            let knots: Vec<f64> = std::iter::once(0.0)
                .chain((0..points.len()).map(|i| i as f64))
                .chain(std::iter::once((points.len() - 1) as f64))
                .collect();
            let id = document.add_spline_curve(false, 1, false, &distinct_knots(&knots), points, None, 0.0);
            Some((id, [a, b]))
        }
        // A traced intersection is written as the spline it was traced from.
        r::Curve::SurfaceIntersection { leader, .. } => {
            let nested = r::Edge {
                curve: (**leader).clone(),
                nurbs: None,
                ..edge.clone()
            };
            add_edge_curve(document, brep, &nested)
        }
    }
}

/// An ellipse (or circle) edge: the ACIS ellipse on its longer axis, and the edge's angles on
/// it read off its vertices (an edge on the ellipse's NURBS form has its interval on the form).
fn ellipse_edge(
    document: &mut SatDocument,
    frame: &r::Frame,
    radii: [f64; 2],
    edge: &r::Edge,
    start: [f64; 3],
    end: [f64; 3],
) -> Option<(i32, [f64; 2])> {
    let normal = unit(cross(frame.x_axis, frame.y_axis))?;
    // ACIS's major axis is the longer one.
    let (major_dir, minor_dir, major, minor) = if radii[0] >= radii[1] {
        (frame.x_axis, frame.y_axis, radii[0], radii[1])
    } else {
        (frame.y_axis, scale(frame.x_axis, -1.0), radii[1], radii[0])
    };
    let id = add(
        document,
        "ellipse-curve",
        vec![
            position(frame.origin),
            position(normal),
            position(scale(major_dir, major)),
            SatToken::Float(minor / major),
        ],
    );
    let angle = |p: [f64; 3]| {
        let d = sub(p, frame.origin);
        (dot(d, minor_dir) / minor).atan2(dot(d, major_dir) / major)
    };
    // The edge runs along the curve (anticlockwise about its normal) from its first vertex.
    let closed = edge.vertices[0] == edge.vertices[1];
    let low = angle(start);
    let mut high = if closed { low + TAU } else { turn_near(angle(end), low) };
    while high <= low + 1e-12 {
        high += TAU;
    }
    Some((id, [low, high]))
}

/// The degree of a spline with `knots` knots and `points` control points.
fn degree(knots: usize, points: usize) -> Option<usize> {
    knots.checked_sub(points + 1)
}

/// A face's SAT surface record, and whether it is a spline (whose uses need pcurves).
fn add_surface(document: &mut SatDocument, face: &r::Face) -> Option<(i32, bool)> {
    let surface = match &face.surface {
        // No ACIS form of their own: their NURBS net is the face's geometry.
        r::Surface::BallPatch { .. } | r::Surface::Blend { .. } | r::Surface::PlacedRevolution { .. } => {
            face.nurbs.as_ref()?
        }
        s => s,
    };
    let analytic = |kind: &str, tokens: Vec<SatToken>, document: &mut SatDocument| Some((add(document, kind, tokens), false));
    match surface {
        r::Surface::Plane { origin, u_axis, v_axis } => {
            let normal = unit(cross(*u_axis, *v_axis))?;
            analytic("plane-surface", vec![position(*origin), position(normal), position(unit(*u_axis)?)], document)
        }
        r::Surface::PlaneThrough { origin, u_point, v_point } => {
            let (u, v) = (sub(*u_point, *origin), sub(*v_point, *origin));
            let normal = unit(cross(u, v))?;
            analytic("plane-surface", vec![position(*origin), position(normal), position(unit(u)?)], document)
        }
        r::Surface::Cylinder { frame, radius } => {
            analytic("cone-surface", cone_tokens(frame.origin, axis(frame)?, frame.x_axis, *radius, 0.0), document)
        }
        r::Surface::Cone { frame, radius_at_origin, half_angle } => {
            let z = axis(frame)?;
            // ACIS has no cone of zero radius at its centre: the centre moves a unit along.
            let (origin, radius) = if radius_at_origin.abs() < 1e-9 {
                let step = half_angle.tan().signum();
                (crate::geometry::add(frame.origin, scale(z, step)), half_angle.tan().abs())
            } else {
                (frame.origin, *radius_at_origin)
            };
            analytic("cone-surface", cone_tokens(origin, z, frame.x_axis, radius, *half_angle), document)
        }
        r::Surface::Sphere { frame, radius } => analytic(
            "sphere-surface",
            vec![position(frame.origin), SatToken::Float(*radius), position(frame.x_axis), position(axis(frame)?)],
            document,
        ),
        r::Surface::Torus { frame, major_radius, minor_radius, .. } => analytic(
            "torus-surface",
            vec![
                position(frame.origin),
                position(axis(frame)?),
                SatToken::Float(*major_radius),
                SatToken::Float(*minor_radius),
                position(frame.x_axis),
            ],
            document,
        ),
        r::Surface::Spline { u_knots, v_knots, points } => {
            let (nu, nv) = (points.len(), points.first()?.len());
            let control: Vec<[f64; 3]> = (0..nv).flat_map(|v| (0..nu).map(move |u| (u, v))).map(|(u, v)| points[u][v]).collect();
            let id = document.add_spline_surface(
                false,
                false,
                degree(u_knots.len(), nu)? as i32,
                degree(v_knots.len(), nv)? as i32,
                false,
                false,
                &distinct_knots(u_knots),
                &distinct_knots(v_knots),
                &control,
                None,
                0.0,
            );
            Some((id, true))
        }
        r::Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
            let (nu, nv) = (homogeneous.len(), homogeneous.first()?.len());
            let at: Vec<[f64; 4]> = (0..nv).flat_map(|v| (0..nu).map(move |u| (u, v))).map(|(u, v)| homogeneous[u][v]).collect();
            let control: Vec<[f64; 3]> = at.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).collect();
            let weights: Vec<f64> = at.iter().map(|h| h[3]).collect();
            let id = document.add_spline_surface(
                false,
                true,
                degree(u_knots.len(), nu)? as i32,
                degree(v_knots.len(), nv)? as i32,
                false,
                false,
                &distinct_knots(u_knots),
                &distinct_knots(v_knots),
                &control,
                Some(&weights),
                0.0,
            );
            Some((id, true))
        }
        r::Surface::BallPatch { .. } | r::Surface::Blend { .. } | r::Surface::PlacedRevolution { .. } => None,
    }
}

fn axis(frame: &r::Frame) -> Option<[f64; 3]> {
    unit(cross(frame.x_axis, frame.y_axis))
}

/// A use's pcurve as ACIS takes it: over the edge's own ACIS range on a forward coedge, over
/// the negated range (running against the edge) on a reversed one. The record's runs over the
/// edge's interval, min to max, either way.
#[allow(clippy::type_complexity)]
fn acis_pcurve(pcurve: &r::Curve2, [low, high]: [f64; 2], forward: bool) -> Option<(usize, Vec<f64>, Vec<[f64; 2]>, Vec<f64>)> {
    let (mut knots, mut controls, mut weights) = match pcurve {
        r::Curve2::Spline { knots, points } => (knots.clone(), points.clone(), vec![1.0; points.len()]),
        r::Curve2::RationalSpline { knots, homogeneous } => (
            knots.clone(),
            homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
            homogeneous.iter().map(|h| h[2]).collect(),
        ),
    };
    let degree = degree(knots.len(), controls.len())?;
    let (t0, t1) = (knots[degree], knots[knots.len() - degree - 1]);
    if !(t1 > t0) {
        return None;
    }
    let (low, high) = if forward {
        (low, high)
    } else {
        controls.reverse();
        weights.reverse();
        knots = knots.iter().rev().map(|k| t0 + t1 - k).collect();
        (-high, -low)
    };
    let s = (high - low) / (t1 - t0);
    let knots = knots.iter().map(|k| low + (k - t0) * s).collect();
    Some((degree, knots, controls, weights))
}

/// Appends a straight-segment polyline as a wire body, the form the reference modeler keeps a
/// polyline sweep path in. A closed polyline ends where it starts; an open one's end coedges
/// point at themselves.
pub fn append_polyline_wire(
    points: &[[f64; 3]],
    closed: bool,
    document: &mut SatDocument,
) -> Result<Written, Unappendable> {
    let mut points = points.to_vec();
    // A closed list may repeat its first point at the end, to rounding.
    let extent = points.iter().map(|p| length(sub(*p, points[0]))).fold(0.0, f64::max);
    let repeated = |a: [f64; 3], b: [f64; 3]| length(sub(a, b)) <= extent * 1e-12;
    if closed && points.len() > 2 && repeated(points[0], points[points.len() - 1]) {
        points.pop();
    }
    let spans = if closed { points.len() } else { points.len().saturating_sub(1) };
    if spans == 0 || (closed && points.len() < 3) || points.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Unappendable::Inconsistent);
    }
    let before = document.record_count();
    let mut vertex_ids = Vec::new();
    let mut point_ids = Vec::new();
    for point in &points {
        point_ids.push(add(document, "point", vec![position(*point)]));
        vertex_ids.push(add(document, "vertex", Vec::new()));
    }
    let mut curves = Vec::new();
    let mut lengths = Vec::new();
    for index in 0..spans {
        let (start, end) = (points[index], points[(index + 1) % points.len()]);
        let direction = unit(sub(end, start)).ok_or(Unappendable::Inconsistent)?;
        curves.push(add(document, "straight-curve", vec![position(start), position(direction)]));
        lengths.push(length(sub(end, start)));
    }
    let edges = (0..spans).map(|_| add(document, "edge", Vec::new())).collect::<Vec<_>>();
    let coedges = (0..spans).map(|_| add(document, "coedge", Vec::new())).collect::<Vec<_>>();
    let wire = add(document, "wire", Vec::new());
    let shell = add(document, "shell", Vec::new());
    let lump = add(document, "lump", Vec::new());
    let body = add(document, "body", Vec::new());
    for (index, vertex) in vertex_ids.iter().enumerate() {
        let edge = edges[index.min(spans - 1)];
        set(document, *vertex, vec![null(), pointer(edge), pointer(point_ids[index])]);
    }
    for index in 0..spans {
        set(document, edges[index], vec![
            null(),
            pointer(vertex_ids[index]),
            SatToken::Float(0.0),
            pointer(vertex_ids[(index + 1) % points.len()]),
            SatToken::Float(lengths[index]),
            pointer(coedges[index]),
            pointer(curves[index]),
            sense(true),
        ]);
        let next = if index + 1 < spans { coedges[index + 1] } else if closed { coedges[0] } else { coedges[index] };
        let previous = if index > 0 { coedges[index - 1] } else if closed { coedges[spans - 1] } else { coedges[0] };
        set(document, coedges[index], vec![
            null(),
            pointer(next),
            pointer(previous),
            null(),
            pointer(edges[index]),
            sense(true),
            pointer(wire),
            null(),
        ]);
    }
    set(document, wire, vec![null(), null(), pointer(coedges[0]), pointer(shell), null(), SatToken::Ident("out".to_string())]);
    set(document, shell, vec![null(), null(), null(), null(), pointer(wire), pointer(lump)]);
    set(document, lump, vec![null(), null(), pointer(shell), pointer(body)]);
    set(document, body, vec![null(), pointer(lump), null(), null()]);
    Ok(Written { body, records: document.record_count() - before })
}

fn add(document: &mut SatDocument, kind: &str, mut tokens: Vec<SatToken>) -> i32 {
    // Every record leads with its attribute pointer; the accessors index from one.
    let mut all = vec![null()];
    all.append(&mut tokens);
    document.add_record(SatRecord {
        index: -1,
        entity_type: kind.to_string(),
        sub_type: None,
        attribute: SatPointer::new(NULL),
        subtype_id: -1,
        tokens: all,
        raw_text: None,
    })
}

fn set(document: &mut SatDocument, id: i32, tokens: Vec<SatToken>) {
    if let Some(record) = document.record_mut(id as usize) {
        record.tokens = tokens;
    }
}

fn distinct_knots(knots: &[f64]) -> Vec<(f64, i32)> {
    let mut out: Vec<(f64, i32)> = Vec::new();
    for knot in knots {
        match out.last_mut() {
            Some((value, multiplicity)) if *value == *knot => *multiplicity += 1,
            _ => out.push((*knot, 1)),
        }
    }
    out
}

/// A cone record, cylinder included: ACIS has no separate cylinder record, only a cone whose
/// half angle is zero. The radius is carried as the length of the major axis, which is how it
/// is read back; the two continuation tokens before the half angle are not decoration: the
/// reader looks for the sine at thirteen.
fn cone_tokens(origin: [f64; 3], axis: [f64; 3], u: [f64; 3], radius: f64, half_angle: f64) -> Vec<SatToken> {
    let (sine, cosine) = half_angle.sin_cos();
    vec![
        position(origin),
        position(axis),
        position(scale(u, radius)),
        SatToken::Float(1.0),
        SatToken::Ident("I".to_string()),
        SatToken::Ident("I".to_string()),
        SatToken::Float(sine),
        SatToken::Float(cosine),
        SatToken::Float(radius),
    ]
}

pub(crate) fn position(value: [f64; 3]) -> SatToken {
    SatToken::Position(value[0], value[1], value[2])
}

fn pointer(id: i32) -> SatToken {
    SatToken::Pointer(SatPointer::new(id))
}

pub(crate) fn null() -> SatToken {
    pointer(NULL)
}

fn sense(forward: bool) -> SatToken {
    SatToken::Ident(if forward { "forward" } else { "reversed" }.to_string())
}
