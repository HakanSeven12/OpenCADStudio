//! Display meshes of B-rep records, by Solidcore's mesher: the shaded triangles, the edges a
//! wireframe draws, and the isolines a solid or a surface shows.

use std::collections::HashMap;

use solidcore_record::brep as r;
use solidcore_tess as tess;

/// What a display mesh is asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// The most a facet turns, radians.
    pub angle: f64,
    /// The farthest a facet may stand off its surface; `None`: a thousandth of the body.
    pub chord: Option<f64>,
    /// Isolines across and along each face.
    pub isolines: [usize; 2],
    /// Planar faces get isolines too.
    pub planar_isolines: bool,
    /// Isolines spaced as a solid draws them (see [`solid_isolines`]).
    pub solid_isolines: bool,
}

/// A record's display mesh.
#[derive(Clone, Debug, Default)]
pub struct RecordMesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub triangles: Vec<[usize; 3]>,
    /// The record face of each triangle.
    pub triangle_faces: Vec<u32>,
    /// Every edge but a seam of one face, by record edge: what a drawing view draws.
    pub drawing_edges: Vec<(u32, Vec<[f64; 3]>)>,
    /// `drawing_edges` less the smooth joins of two surfaces: what a wireframe draws.
    pub edges: Vec<(u32, Vec<[f64; 3]>)>,
    /// Isolines, by record face.
    pub isolines: Vec<(u32, Vec<[f64; 3]>)>,
    /// The faces the mesher left out.
    pub missing_faces: Vec<u32>,
}

fn point(v: &solidcore_record::Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 1e-300).then(|| a.map(|x| x / length))
}

fn tolerance(display: &Display, size: f64) -> tess::MeshTolerance {
    let chord = display
        .chord
        .filter(|c| c.is_finite() && *c > 0.0)
        .unwrap_or_else(|| tess::MeshTolerance::display_of(size).chord);
    tess::MeshTolerance {
        chord,
        angle: Some(display.angle),
    }
}

/// How many times each face uses each edge.
fn edge_uses(record: &r::Brep) -> Vec<HashMap<u32, usize>> {
    let mut uses = vec![HashMap::new(); record.edges.len()];
    for (index, face) in record.faces.iter().enumerate() {
        for ring in &face.loops {
            if let r::Loop::Edges { edges } = ring {
                for used in edges {
                    if let Some(map) = uses.get_mut(used.edge as usize) {
                        *map.entry(index as u32).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    uses
}

fn is_plane(surface: &r::Surface) -> bool {
    matches!(surface, r::Surface::Plane { .. } | r::Surface::PlaneThrough { .. })
}

/// The record of `faces` of `record` alone, as one open shell, its edges and vertices
/// renumbered; `None` when a face is not in it.
pub fn subset(record: &r::Brep, faces: &[u32]) -> Option<r::Brep> {
    let mut edges: HashMap<u32, u32> = HashMap::new();
    let mut vertices: HashMap<u32, u32> = HashMap::new();
    let mut out = r::Brep {
        version: record.version,
        tolerance: record.tolerance,
        regions: Vec::new(),
        shells: Vec::new(),
        faces: Vec::new(),
        edges: Vec::new(),
        vertices: Vec::new(),
    };
    let mut vertex = |index: u32, out: &mut r::Brep| -> Option<u32> {
        if let Some(found) = vertices.get(&index) {
            return Some(*found);
        }
        out.vertices.push(record.vertices.get(index as usize)?.clone());
        let new = (out.vertices.len() - 1) as u32;
        vertices.insert(index, new);
        Some(new)
    };
    for &index in faces {
        let mut face = record.faces.get(index as usize)?.clone();
        for ring in &mut face.loops {
            match ring {
                r::Loop::Edges { edges: used } => {
                    for oriented in used {
                        let new = match edges.get(&oriented.edge) {
                            Some(new) => *new,
                            None => {
                                let mut edge = record.edges.get(oriented.edge as usize)?.clone();
                                edge.vertices = [vertex(edge.vertices[0], &mut out)?, vertex(edge.vertices[1], &mut out)?];
                                out.edges.push(edge);
                                let new = (out.edges.len() - 1) as u32;
                                edges.insert(oriented.edge, new);
                                new
                            }
                        };
                        oriented.edge = new;
                    }
                }
                r::Loop::Vertex { vertex: at } => *at = vertex(*at, &mut out)?,
            }
        }
        out.faces.push(face);
    }
    out.shells.push(r::Shell::Faces {
        faces: (0..out.faces.len() as u32)
            .map(|face| r::OrientedFace { face, forward: true })
            .collect(),
        wires: Vec::new(),
    });
    out.regions.push(r::Region {
        shells: vec![0],
        void: false,
    });
    Some(out)
}

/// A planar section of a face's triangles, chained into polylines: the pieces of `triangles`
/// (corner points) on the plane through `origin` across `normal`, kept where `keep` holds of
/// a piece's middle.
fn section(
    triangles: &[[[f64; 3]; 3]],
    origin: [f64; 3],
    normal: [f64; 3],
    keep: &dyn Fn([f64; 3]) -> bool,
    precision: f64,
) -> Vec<Vec<[f64; 3]>> {
    let mut pieces: Vec<[[f64; 3]; 2]> = Vec::new();
    for corners in triangles {
        let side = corners.map(|p| dot(sub(p, origin), normal));
        let mut ends = Vec::with_capacity(2);
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (da, db) = (side[a], side[b]);
            // The half-open rule takes a crossing at a corner once.
            if (da <= 0.0) != (db <= 0.0) {
                let t = da / (da - db);
                ends.push(std::array::from_fn(|k| corners[a][k] + t * (corners[b][k] - corners[a][k])));
            }
        }
        if let [a, b] = ends[..] {
            let middle = std::array::from_fn(|k| 0.5 * (a[k] + b[k]));
            if keep(middle) && dot(sub(a, b), sub(a, b)) > precision * precision {
                pieces.push([a, b]);
            }
        }
    }
    chain(pieces, precision)
}

/// Joins `pieces` end to end where their ends meet within `precision`.
fn chain(pieces: Vec<[[f64; 3]; 2]>, precision: f64) -> Vec<Vec<[f64; 3]>> {
    let key = |p: [f64; 3]| p.map(|x| (x / precision).round() as i64);
    let mut at: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for (index, piece) in pieces.iter().enumerate() {
        for end in piece {
            at.entry(key(*end)).or_default().push(index);
        }
    }
    let mut used = vec![false; pieces.len()];
    let mut lines = Vec::new();
    for start in 0..pieces.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut line = std::collections::VecDeque::from(pieces[start].to_vec());
        for forward in [true, false] {
            loop {
                let end = if forward { *line.back().unwrap() } else { *line.front().unwrap() };
                let next = at
                    .get(&key(end))
                    .and_then(|found| found.iter().copied().find(|i| !used[*i]));
                let Some(next) = next else { break };
                used[next] = true;
                let [a, b] = pieces[next];
                let far = if key(a) == key(end) { b } else { a };
                if forward {
                    line.push_back(far);
                } else {
                    line.push_front(far);
                }
            }
        }
        lines.push(line.into_iter().collect());
    }
    lines
}

/// The angles, from the surface's zero angle, of the lines a solid draws round an axis on a
/// face spanning `range` of the turn: `count` a whole turn apart for a full turn, else the
/// span cut into `count` times its share of a turn (rounded, plus one) equal pieces.
fn turn_values(range: [f64; 2], count: usize) -> Vec<f64> {
    let turn = std::f64::consts::TAU;
    let span = range[1] - range[0];
    if span >= turn - 1e-6 {
        return (0..count).map(|index| turn * index as f64 / count as f64).collect();
    }
    let parts = (count as f64 * span / turn + 0.5).floor() as usize + 1;
    (1..parts).map(|index| range[0] + span * index as f64 / parts as f64).collect()
}

/// The angle of `p` round the frame's axis from its x axis, in `[0, 2π)`.
fn angle_of(frame: &r::Frame, p: [f64; 3]) -> f64 {
    let d = sub(p, frame.origin);
    dot(d, frame.y_axis).atan2(dot(d, frame.x_axis)).rem_euclid(std::f64::consts::TAU)
}

/// The angular span a face covers round its axis: the gap the face leaves, read off its mesh
/// points, is the part of the turn outside it.
fn angular_range(frame: &r::Frame, points: &[[f64; 3]]) -> [f64; 2] {
    let mut angles: Vec<f64> = points.iter().map(|p| angle_of(frame, *p)).collect();
    angles.sort_by(f64::total_cmp);
    let turn = std::f64::consts::TAU;
    let Some((&first, &last)) = angles.first().zip(angles.last()) else {
        return [0.0, turn];
    };
    let mut gap = (first + turn - last, last);
    for pair in angles.windows(2) {
        if pair[1] - pair[0] > gap.0 {
            gap = (pair[1] - pair[0], pair[0]);
        }
    }
    // Facets are well under a sixth of a turn: a smaller gap is a full turn.
    if gap.0 < turn / 6.0 {
        [0.0, turn]
    } else {
        let start = gap.1 + gap.0;
        [start, start + turn - gap.0]
    }
}

/// The isolines a solid draws on an analytic face, as sections of its triangles: lines along
/// the axis at the turn's angles on a cylinder, cone, sphere or torus, and on a sphere the
/// latitudes too, its span cut into `count` pieces, and on a torus the tube's circles. `None`
/// for a face of another surface.
fn solid_isolines(surface: &r::Surface, triangles: &[[[f64; 3]; 3]], count: usize, precision: f64) -> Option<Vec<Vec<[f64; 3]>>> {
    let frame = match surface {
        r::Surface::Cylinder { frame, .. }
        | r::Surface::Cone { frame, .. }
        | r::Surface::Sphere { frame, .. }
        | r::Surface::Torus { frame, .. } => frame,
        _ => return None,
    };
    let axis = unit(cross(frame.x_axis, frame.y_axis))?;
    let x = unit(frame.x_axis)?;
    let y = cross(axis, x);
    let points: Vec<[f64; 3]> = triangles.iter().flatten().copied().collect();
    let mut lines = Vec::new();
    if count == 0 {
        return Some(lines);
    }
    for angle in turn_values(angular_range(frame, &points), count) {
        let direction: [f64; 3] = std::array::from_fn(|k| x[k] * angle.cos() + y[k] * angle.sin());
        let normal = cross(axis, direction);
        let origin = frame.origin;
        let keep = move |p: [f64; 3]| dot(sub(p, origin), direction) > 0.0;
        lines.extend(section(triangles, origin, normal, &keep, precision));
    }
    match surface {
        r::Surface::Sphere { frame, radius } => {
            let heights: Vec<f64> = points.iter().map(|p| dot(sub(*p, frame.origin), axis)).collect();
            let low = heights.iter().copied().fold(f64::INFINITY, f64::min).max(-radius);
            let high = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max).min(*radius);
            let [from, to] = [low, high].map(|h| (h / radius).clamp(-1.0, 1.0).asin());
            for index in 1..count {
                let latitude = from + (to - from) * index as f64 / count as f64;
                let origin: [f64; 3] = std::array::from_fn(|k| frame.origin[k] + axis[k] * radius * latitude.sin());
                lines.extend(section(triangles, origin, axis, &|_| true, precision));
            }
        }
        r::Surface::Torus { frame, major_radius, .. } => {
            // The tube's circles: sections across the tube are the turn's lines above; round
            // the axis, the tube's angles are sections by cones, drawn here as the planes
            // through the tube's centre circle at each angle of the tube.
            let centre_of = |p: [f64; 3]| {
                let d = sub(p, frame.origin);
                let h = dot(d, axis);
                let radial = unit(sub(d, axis.map(|a| a * h))).unwrap_or(x);
                std::array::from_fn::<f64, 3, _>(|k| frame.origin[k] + radial[k] * major_radius)
            };
            let tube_angle = |p: [f64; 3]| {
                let c = centre_of(p);
                let out = unit(sub(c, frame.origin)).unwrap_or(x);
                let d = sub(p, c);
                dot(d, axis).atan2(dot(d, out)).rem_euclid(std::f64::consts::TAU)
            };
            let tube_frame = r::Frame {
                origin: [0.0; 3],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            };
            let flat: Vec<[f64; 3]> = points.iter().map(|p| {
                let a = tube_angle(*p);
                [a.cos(), a.sin(), 0.0]
            }).collect();
            for angle in turn_values(angular_range(&tube_frame, &flat), count) {
                // Points of the tube at `angle`: keep the triangles' crossings of the surface
                // of revolution where the tube angle passes `angle`.
                let mut pieces = Vec::new();
                for corners in triangles {
                    let side = corners.map(|p| (tube_angle(p) - angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI);
                    let mut ends = Vec::with_capacity(2);
                    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                        let (da, db) = (side[a], side[b]);
                        if (da <= 0.0) != (db <= 0.0) && (da - db).abs() < std::f64::consts::PI {
                            let t = da / (da - db);
                            ends.push(std::array::from_fn(|k| corners[a][k] + t * (corners[b][k] - corners[a][k])));
                        }
                    }
                    if let [a, b] = ends[..] {
                        pieces.push([a, b]);
                    }
                }
                lines.extend(chain(pieces, precision));
            }
        }
        _ => {}
    }
    Some(lines)
}

/// The display mesh of `record`, a body of size `size` (its box diagonal).
pub fn mesh(record: &r::Brep, display: &Display, size: f64) -> Result<RecordMesh, String> {
    let size = if size.is_finite() && size > 0.0 { size } else { 1.0 };
    let tolerance = tolerance(display, size);
    let (plan, _) = tess::plan(record, &tolerance, size).map_err(|e| e.to_string())?;
    let (mesh, refused) = plan.evaluate();
    let mut out = RecordMesh {
        positions: mesh.positions.iter().map(point).collect(),
        normals: mesh.normals.iter().map(point).collect(),
        triangles: mesh.triangles.iter().map(|t| t.map(|i| i as usize)).collect(),
        triangle_faces: mesh.tri_face.clone(),
        missing_faces: refused.iter().map(|f| f.face as u32).collect(),
        ..RecordMesh::default()
    };
    let with_triangles = plan.faces_with_triangles(record.faces.len());
    for (index, has) in with_triangles.iter().enumerate() {
        if !has && !out.missing_faces.contains(&(index as u32)) {
            out.missing_faces.push(index as u32);
        }
    }
    out.missing_faces.sort_unstable();
    let precision = (size * 1e-9).max(1e-12);

    // A vertex's normal on each face, by its position: an edge's two faces meet smoothly
    // where they agree all along it.
    let key = |p: [f64; 3]| p.map(|x| (x / precision).round() as i64);
    let mut normal_at: HashMap<(u32, [i64; 3]), [f64; 3]> = HashMap::new();
    for (triangle, face) in out.triangles.iter().zip(&out.triangle_faces) {
        for &corner in triangle {
            normal_at.insert((*face, key(out.positions[corner])), out.normals[corner]);
        }
    }
    let uses = edge_uses(record);
    for (index, polyline) in mesh.edges.iter().enumerate() {
        if polyline.len() < 2 {
            continue;
        }
        let used = uses.get(index).cloned().unwrap_or_default();
        // A seam: one face uses the edge twice.
        if used.len() == 1 && used.values().all(|count| *count >= 2) {
            continue;
        }
        let points: Vec<[f64; 3]> = polyline.iter().map(point).collect();
        let faces: Vec<u32> = used.keys().copied().collect();
        let smooth = match faces[..] {
            [a, b] => {
                let planes = [a, b].map(|f| record.faces.get(f as usize).is_some_and(|face| is_plane(&face.surface)));
                // An edge dividing planes was put there on purpose (an imprint, a split).
                !(planes[0] && planes[1])
                    && points.iter().all(|p| {
                        match (normal_at.get(&(a, key(*p))), normal_at.get(&(b, key(*p)))) {
                            (Some(n), Some(m)) => dot(*n, *m) >= 1.0 - 1e-8,
                            _ => false,
                        }
                    })
            }
            _ => false,
        };
        out.drawing_edges.push((index as u32, points.clone()));
        if !smooth {
            out.edges.push((index as u32, points));
        }
    }

    if display.isolines.iter().any(|c| *c > 0) {
        isolines(record, display, &tolerance, size, precision, &mut out)?;
    }
    Ok(out)
}

fn isolines(
    record: &r::Brep,
    display: &Display,
    tolerance: &tess::MeshTolerance,
    size: f64,
    precision: f64,
    out: &mut RecordMesh,
) -> Result<(), String> {
    let mut by_face: HashMap<u32, Vec<[[f64; 3]; 3]>> = HashMap::new();
    for (triangle, face) in out.triangles.iter().zip(&out.triangle_faces) {
        by_face.entry(*face).or_default().push(triangle.map(|i| out.positions[i]));
    }
    let mut parametric = Vec::new();
    for (index, face) in record.faces.iter().enumerate() {
        let index = index as u32;
        if is_plane(&face.surface) && !display.planar_isolines {
            continue;
        }
        if display.solid_isolines {
            let triangles = by_face.get(&index).map(Vec::as_slice).unwrap_or(&[]);
            if let Some(lines) = solid_isolines(&face.surface, triangles, display.isolines[0], precision) {
                out.isolines.extend(lines.into_iter().map(|line| (index, line)));
                continue;
            }
        }
        parametric.push(index);
    }
    for index in parametric {
        let Some(alone) = subset(record, &[index]) else {
            out.missing_faces.push(index);
            continue;
        };
        match tess::isoparms(&alone, display.isolines[0], display.isolines[1], *tolerance, size) {
            Ok(lines) => out
                .isolines
                .extend(lines.into_iter().map(|line| (index, line))),
            Err(_) => out.missing_faces.push(index),
        }
    }
    out.missing_faces.sort_unstable();
    out.missing_faces.dedup();
    Ok(())
}

