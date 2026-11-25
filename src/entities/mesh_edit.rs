//! Edits of a subdivision mesh's base cage (the faces, edges and creases the Mesh tab's
//! commands change, found from points picked on the drawn mesh), the mesh primitives, and the
//! polygon meshes ruled, tabulated, revolved and edge-defined surfaces are made of.

use std::collections::{HashMap, HashSet};

use document::entities::polygon_mesh::{PolygonMesh, PolygonMeshFlags, PolygonMeshVertex};
use document::entities::{Mesh, MeshEdge, MeshFace};
use document::objects::SolidHistoryOperation;
use document::types::Vector3;
use glam::DVec3;

/// The crease value that keeps an edge sharp at every smoothness level.
pub const ALWAYS: f64 = -1.0;

type Key = (usize, usize);

fn key(a: usize, b: usize) -> Key {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

fn at(mesh: &Mesh, index: usize) -> DVec3 {
    let v = mesh.vertices[index];
    DVec3::new(v.x, v.y, v.z)
}

fn vector(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

/// The mesh's edge creases by vertex pair.
fn creases(mesh: &Mesh) -> HashMap<Key, f64> {
    mesh.edges
        .iter()
        .filter_map(|e| Some((key(e.start, e.end), e.crease?)))
        .collect()
}

/// The edge list rebuilt from the faces: every edge once, with the crease it had.
fn rebuild_edges(mesh: &mut Mesh, creases: &HashMap<Key, f64>) {
    let mut seen = HashSet::new();
    mesh.edges.clear();
    for face in &mesh.faces {
        let n = face.vertices.len();
        for k in 0..n {
            let (a, b) = (face.vertices[k], face.vertices[(k + 1) % n]);
            if a != b && seen.insert(key(a, b)) {
                let mut edge = MeshEdge::new(key(a, b).0, key(a, b).1);
                edge.crease = creases.get(&key(a, b)).copied();
                mesh.edges.push(edge);
            }
        }
    }
}

/// The faces with repeated corners dropped and those left with fewer than three removed;
/// the vertices no face uses removed and the rest renumbered, creases kept.
fn tidy(mesh: &mut Mesh, creases: HashMap<Key, f64>) {
    let count = mesh.vertices.len();
    for face in &mut mesh.faces {
        face.vertices.retain(|&v| v < count);
        face.vertices.dedup();
        while face.vertices.len() > 1 && face.vertices.first() == face.vertices.last() {
            face.vertices.pop();
        }
    }
    mesh.faces.retain(|f| f.vertices.len() >= 3);
    let mut map = vec![usize::MAX; count];
    let mut vertices = Vec::new();
    for face in &mut mesh.faces {
        for v in &mut face.vertices {
            if map[*v] == usize::MAX {
                map[*v] = vertices.len();
                vertices.push(mesh.vertices[*v]);
            }
            *v = map[*v];
        }
    }
    mesh.vertices = vertices;
    let creases = creases
        .into_iter()
        .filter(|((a, b), _)| map[*a] != usize::MAX && map[*b] != usize::MAX)
        .map(|((a, b), c)| (key(map[a], map[b]), c))
        .collect();
    rebuild_edges(mesh, &creases);
}

/// A mesh of `faces` (vertex loops, anticlockwise seen from outside) on `vertices`.
pub fn mesh_from(vertices: &[[f64; 3]], faces: Vec<Vec<usize>>) -> Mesh {
    let mut mesh = Mesh::new();
    mesh.vertices = vertices.iter().map(|p| Vector3::new(p[0], p[1], p[2])).collect();
    mesh.faces = faces.into_iter().map(MeshFace::new).collect();
    tidy(&mut mesh, HashMap::new());
    mesh
}

/// Newell's normal of a vertex loop (its length twice the area).
fn newell(points: &[DVec3]) -> DVec3 {
    (0..points.len()).fold(DVec3::ZERO, |n, i| {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        n + DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y))
    })
}

fn face_points(mesh: &Mesh, face: usize) -> Vec<DVec3> {
    mesh.faces[face].vertices.iter().map(|&v| at(mesh, v)).collect()
}

fn segment_distance(p: DVec3, a: DVec3, b: DVec3) -> (f64, f64) {
    let d = b - a;
    let t = if d.length_squared() > 0.0 { ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (p.distance(a + d * t), t)
}

/// How far `p` is from face `face`: across its plane where it lies over the face, else to its
/// nearest edge.
fn face_distance(mesh: &Mesh, face: usize, p: DVec3) -> f64 {
    let points = face_points(mesh, face);
    let edge = (0..points.len())
        .map(|k| segment_distance(p, points[k], points[(k + 1) % points.len()]).0)
        .fold(f64::INFINITY, f64::min);
    let normal = newell(&points).normalize_or_zero();
    if normal == DVec3::ZERO {
        return edge;
    }
    let centre = points.iter().copied().sum::<DVec3>() / points.len() as f64;
    let on_plane = p - normal * (p - centre).dot(normal);
    let inside = (0..points.len()).all(|k| {
        let (a, b) = (points[k], points[(k + 1) % points.len()]);
        (b - a).cross(on_plane - a).dot(normal) >= -1e-12 * (b - a).length_squared()
    });
    if inside {
        (p - centre).dot(normal).abs().min(edge)
    } else {
        edge
    }
}

/// The face of `mesh` nearest `p`.
pub fn pick_face(mesh: &Mesh, p: DVec3) -> Option<usize> {
    (0..mesh.faces.len())
        .filter(|&f| mesh.faces[f].vertices.len() >= 3 && mesh.faces[f].vertices.iter().all(|&v| v < mesh.vertices.len()))
        .min_by(|&a, &b| face_distance(mesh, a, p).total_cmp(&face_distance(mesh, b, p)))
}

/// The face edges of `mesh`, as (face, corner) with the edge running from that corner to the
/// next, nearest `p` first; `only` keeps the ones it accepts.
fn nearest_edge(mesh: &Mesh, p: DVec3, only: impl Fn(Key) -> bool) -> Option<(usize, usize)> {
    let mut best: Option<(f64, usize, usize)> = None;
    for (f, face) in mesh.faces.iter().enumerate() {
        let n = face.vertices.len();
        if face.vertices.iter().any(|&v| v >= mesh.vertices.len()) {
            continue;
        }
        for k in 0..n {
            let (a, b) = (face.vertices[k], face.vertices[(k + 1) % n]);
            if a == b || !only(key(a, b)) {
                continue;
            }
            let (d, _) = segment_distance(p, at(mesh, a), at(mesh, b));
            if best.is_none_or(|(e, _, _)| d < e) {
                best = Some((d, f, k));
            }
        }
    }
    best.map(|(_, f, k)| (f, k))
}

/// The edge of `mesh` nearest `p`.
pub fn pick_edge(mesh: &Mesh, p: DVec3) -> Option<Key> {
    let (f, k) = nearest_edge(mesh, p, |_| true)?;
    let face = &mesh.faces[f].vertices;
    Some(key(face[k], face[(k + 1) % face.len()]))
}

/// What a pick on a mesh names: an edge where it lies near one, else the face under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    Edge(Key),
    Face(usize),
}

/// The edge or face picked at `p`: the edge when `p` is within a quarter of the face's mean
/// edge length of it.
pub fn pick_part(mesh: &Mesh, p: DVec3) -> Option<Part> {
    let face = pick_face(mesh, p)?;
    let points = face_points(mesh, face);
    let n = points.len();
    let mean = (0..n).map(|k| points[k].distance(points[(k + 1) % n])).sum::<f64>() / n as f64;
    let (d, k) = (0..n)
        .map(|k| (segment_distance(p, points[k], points[(k + 1) % n]).0, k))
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let loop_ = &mesh.faces[face].vertices;
    Some(if d <= 0.25 * mean { Part::Edge(key(loop_[k], loop_[(k + 1) % n])) } else { Part::Face(face) })
}

/// The edges of `part`.
pub fn part_edges(mesh: &Mesh, part: Part) -> Vec<Key> {
    match part {
        Part::Edge(e) => vec![e],
        Part::Face(f) => {
            let v = &mesh.faces[f].vertices;
            (0..v.len()).map(|k| key(v[k], v[(k + 1) % v.len()])).collect()
        }
    }
}

/// Sets the crease of `edges` (`None` removes it); how many edges of the mesh changed.
pub fn set_creases(mesh: &mut Mesh, edges: &[Key], value: Option<f64>) -> usize {
    if mesh.edges.is_empty() {
        let creases = creases(mesh);
        rebuild_edges(mesh, &creases);
    }
    let wanted: HashSet<Key> = edges.iter().copied().collect();
    let mut changed = 0;
    for edge in &mut mesh.edges {
        if wanted.contains(&key(edge.start, edge.end)) && edge.crease != value {
            edge.crease = value;
            changed += 1;
        }
    }
    changed
}

/// The smoothed cage made the base cage (MESHREFINE): its faces multiplied by the
/// subdivision, the smoothness level 0. `false` for a mesh not smoothed.
pub fn refine(mesh: &mut Mesh) -> bool {
    if mesh.subdivision_level <= 0 {
        return false;
    }
    let (vertices, faces, creases) = super::mesh::smoothed_cage(mesh);
    mesh.vertices = vertices.iter().map(|p| Vector3::new(p[0], p[1], p[2])).collect();
    mesh.faces = faces.into_iter().map(MeshFace::new).collect();
    mesh.subdivision_level = 0;
    let creases = creases
        .into_iter()
        .map(|(k, c)| (k, if c.is_infinite() { ALWAYS } else { c }))
        .collect();
    tidy(mesh, creases);
    true
}

/// The directed edges bounding a set of faces: each edge of theirs no other of them shares,
/// as its face runs it.
fn boundary<'a>(loops: impl Iterator<Item = &'a [usize]> + Clone) -> Vec<(usize, usize)> {
    let mut count: HashMap<Key, usize> = HashMap::new();
    for v in loops.clone() {
        for k in 0..v.len() {
            *count.entry(key(v[k], v[(k + 1) % v.len()])).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    for v in loops {
        for k in 0..v.len() {
            let (a, b) = (v[k], v[(k + 1) % v.len()]);
            if count[&key(a, b)] == 1 {
                out.push((a, b));
            }
        }
    }
    out
}

fn region_boundary(mesh: &Mesh, faces: &HashSet<usize>) -> Vec<(usize, usize)> {
    boundary(faces.iter().map(|&f| mesh.faces[f].vertices.as_slice()))
}

/// The single loop `edges` (directed) chain into, or `None` where they make several or branch.
fn single_loop(edges: &[(usize, usize)]) -> Option<Vec<usize>> {
    let mut next: HashMap<usize, usize> = HashMap::new();
    for &(a, b) in edges {
        if next.insert(a, b).is_some() {
            return None;
        }
    }
    let &(start, _) = edges.first()?;
    let mut loop_ = vec![start];
    let mut at = next[&start];
    while at != start {
        if loop_.len() > edges.len() {
            return None;
        }
        loop_.push(at);
        at = *next.get(&at)?;
    }
    (loop_.len() == edges.len()).then_some(loop_)
}

/// Faces `faces` extruded by `offset` (MESHEXTRUDE, adjacent faces joined): their corners
/// moved, and a wall along the region's boundary. `false` when no face is given.
pub fn extrude(mesh: &mut Mesh, faces: &[usize], height: f64, direction: Option<DVec3>) -> bool {
    let set: HashSet<usize> = faces.iter().copied().filter(|&f| f < mesh.faces.len()).collect();
    if set.is_empty() {
        return false;
    }
    let creases = creases(mesh);
    // Each corner moves along the mean normal of the picked faces at it.
    let mut normals: HashMap<usize, DVec3> = HashMap::new();
    for &f in &set {
        let n = newell(&face_points(mesh, f)).normalize_or_zero();
        for &v in &mesh.faces[f].vertices {
            *normals.entry(v).or_default() += n;
        }
    }
    let boundary = region_boundary(mesh, &set);
    let mut moved: HashMap<usize, usize> = HashMap::new();
    let mut corners: Vec<usize> = normals.keys().copied().collect();
    corners.sort_unstable();
    for v in corners {
        let step = direction.unwrap_or_else(|| normals[&v].normalize_or_zero() * height);
        let p = at(mesh, v) + step;
        moved.insert(v, mesh.vertices.len());
        mesh.vertices.push(vector(p));
    }
    for &f in &set {
        for v in &mut mesh.faces[f].vertices {
            *v = moved[v];
        }
    }
    for (a, b) in boundary {
        mesh.faces.push(MeshFace::new(vec![a, b, moved[&b], moved[&a]]));
    }
    tidy(mesh, creases);
    true
}

/// A vertex on edge (a, b) at `t` from a: an end when `t` is at it, else a new vertex put into
/// every face that runs the edge.
fn vertex_on_edge(mesh: &mut Mesh, a: usize, b: usize, t: f64) -> usize {
    if t <= 1e-9 {
        return a;
    }
    if t >= 1.0 - 1e-9 {
        return b;
    }
    let p = at(mesh, a).lerp(at(mesh, b), t);
    let new = mesh.vertices.len();
    mesh.vertices.push(vector(p));
    for face in &mut mesh.faces {
        let n = face.vertices.len();
        if let Some(k) = (0..n).find(|&k| {
            let (x, y) = (face.vertices[k], face.vertices[(k + 1) % n]);
            (x, y) == (a, b) || (x, y) == (b, a)
        }) {
            face.vertices.insert(k + 1, new);
        }
    }
    new
}

/// Face `face` split from a point on one of its edges to a point on another (MESHSPLIT); the
/// faces beside take the new corners too. `Err` names why it cannot.
pub fn split(mesh: &mut Mesh, face: usize, from: DVec3, to: DVec3) -> Result<(), &'static str> {
    let creases = creases(mesh);
    let nearest = |mesh: &Mesh, p: DVec3| {
        let v = &mesh.faces[face].vertices;
        let n = v.len();
        (0..n)
            .map(|k| {
                let (d, t) = segment_distance(p, at(mesh, v[k]), at(mesh, v[(k + 1) % n]));
                (d, v[k], v[(k + 1) % n], t)
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
    };
    let (_, a0, b0, t0) = nearest(mesh, from).ok_or("no face")?;
    let first = vertex_on_edge(mesh, a0, b0, t0);
    let (_, a1, b1, t1) = nearest(mesh, to).ok_or("no face")?;
    let second = vertex_on_edge(mesh, a1, b1, t1);
    let v = mesh.faces[face].vertices.clone();
    let n = v.len();
    let i = v.iter().position(|&x| x == first).ok_or("no split point")?;
    let j = v.iter().position(|&x| x == second).ok_or("no split point")?;
    if i == j || (i + 1) % n == j || (j + 1) % n == i {
        return Err("The split points must lie on different, not adjacent, sides of the face.");
    }
    let walk = |from: usize, to: usize| {
        let mut out = vec![v[from]];
        let mut k = from;
        while k != to {
            k = (k + 1) % n;
            out.push(v[k]);
        }
        out
    };
    mesh.faces[face].vertices = walk(i, j);
    mesh.faces.push(MeshFace::new(walk(j, i)));
    tidy(mesh, creases);
    Ok(())
}

/// Adjacent faces `faces` merged into one (MESHMERGE).
pub fn merge(mesh: &mut Mesh, faces: &[usize]) -> Result<(), &'static str> {
    let set: HashSet<usize> = faces.iter().copied().filter(|&f| f < mesh.faces.len()).collect();
    if set.len() < 2 {
        return Err("Select at least two adjacent faces.");
    }
    let loop_ = single_loop(&region_boundary(mesh, &set))
        .ok_or("The faces must be adjacent and bound one area without holes.")?;
    let creases = creases(mesh);
    let mut kept: Vec<MeshFace> = mesh
        .faces
        .iter()
        .enumerate()
        .filter(|(f, _)| !set.contains(f))
        .map(|(_, f)| f.clone())
        .collect();
    kept.push(MeshFace::new(loop_));
    mesh.faces = kept;
    tidy(mesh, creases);
    Ok(())
}

/// The holes of `mesh` the edges nearest `points` bound, each closed with a face (MESHCAP);
/// how many were closed.
pub fn cap(mesh: &mut Mesh, points: &[DVec3]) -> usize {
    let mut use_count: HashMap<Key, usize> = HashMap::new();
    for face in &mesh.faces {
        let v = &face.vertices;
        for k in 0..v.len() {
            *use_count.entry(key(v[k], v[(k + 1) % v.len()])).or_default() += 1;
        }
    }
    let open = |k: Key| use_count.get(&k) == Some(&1);
    // Each open edge as its face runs it.
    let mut next: HashMap<usize, usize> = HashMap::new();
    for face in &mesh.faces {
        let v = &face.vertices;
        for k in 0..v.len() {
            let (a, b) = (v[k], v[(k + 1) % v.len()]);
            if open(key(a, b)) {
                next.insert(a, b);
            }
        }
    }
    let mut capped: Vec<Vec<usize>> = Vec::new();
    let mut used: HashSet<usize> = HashSet::new();
    for &p in points {
        let Some((f, k)) = nearest_edge(mesh, p, open) else { continue };
        let start = mesh.faces[f].vertices[k];
        if used.contains(&start) {
            continue;
        }
        let mut loop_ = vec![start];
        let mut at = next[&start];
        while at != start && loop_.len() <= next.len() {
            loop_.push(at);
            match next.get(&at) {
                Some(&n) => at = n,
                None => break,
            }
        }
        if at != start || loop_.len() < 3 {
            continue;
        }
        used.extend(loop_.iter().copied());
        // The hole's face runs its edges the other way round.
        loop_.reverse();
        capped.push(loop_);
    }
    let count = capped.len();
    if count > 0 {
        let creases = creases(mesh);
        mesh.faces.extend(capped.into_iter().map(MeshFace::new));
        tidy(mesh, creases);
    }
    count
}

/// The vertices of `part` joined at their middle (MESHCOLLAPSE).
pub fn collapse(mesh: &mut Mesh, part: Part) {
    let vertices: Vec<usize> = match part {
        Part::Edge((a, b)) => vec![a, b],
        Part::Face(f) => mesh.faces[f].vertices.clone(),
    };
    let Some(&keep) = vertices.first() else { return };
    let centre = vertices.iter().map(|&v| at(mesh, v)).sum::<DVec3>() / vertices.len() as f64;
    mesh.vertices[keep] = vector(centre);
    let gone: HashSet<usize> = vertices.into_iter().collect();
    let to = |v: usize| if gone.contains(&v) { keep } else { v };
    let creases = creases(mesh)
        .into_iter()
        .filter(|((a, b), _)| to(*a) != to(*b))
        .map(|((a, b), c)| (key(to(a), to(b)), c))
        .collect();
    for face in &mut mesh.faces {
        for v in &mut face.vertices {
            *v = to(*v);
        }
    }
    tidy(mesh, creases);
}

/// The shared edge of two adjacent triangles turned to join their other corners (MESHSPIN).
pub fn spin(mesh: &mut Mesh, first: usize, second: usize) -> Result<(), &'static str> {
    let (t1, t2) = (mesh.faces[first].vertices.clone(), mesh.faces[second].vertices.clone());
    if first == second || t1.len() != 3 || t2.len() != 3 {
        return Err("Select two adjacent triangular faces.");
    }
    // t1 runs a -> b -> c with (a, b) the shared edge; t2 runs b -> a -> d.
    let k = (0..3)
        .find(|&k| t2.contains(&t1[k]) && t2.contains(&t1[(k + 1) % 3]))
        .ok_or("The triangles do not share an edge.")?;
    let (a, b, c) = (t1[k], t1[(k + 1) % 3], t1[(k + 2) % 3]);
    let d = *t2.iter().find(|&&v| v != a && v != b).ok_or("The triangles do not share an edge.")?;
    let creases: HashMap<Key, f64> = creases(mesh).into_iter().filter(|(e, _)| *e != key(a, b)).collect();
    mesh.faces[first].vertices = vec![a, d, c];
    mesh.faces[second].vertices = vec![d, b, c];
    tidy(mesh, creases);
    Ok(())
}

// ── Mesh primitives ─────────────────────────────────────────────────────────

/// The tessellation divisions of the mesh primitives.
#[derive(Clone, Copy, Debug)]
pub struct Divisions {
    pub box_: [usize; 3],
    /// Around, along the height, rings on the base.
    pub cone: [usize; 3],
    pub cylinder: [usize; 3],
    /// Along a base side, along the height, rings on the base.
    pub pyramid: [usize; 3],
    /// Around, from pole to pole.
    pub sphere: [usize; 2],
    /// Along the sweep path, round the section.
    pub torus: [usize; 2],
    /// Length, width, height, along the slope.
    pub wedge: [usize; 4],
}

struct Builder {
    points: Vec<DVec3>,
    faces: Vec<Vec<usize>>,
    welded: HashMap<[i64; 3], usize>,
    unit: f64,
}

impl Builder {
    fn new(size: f64) -> Self {
        Self { points: Vec::new(), faces: Vec::new(), welded: HashMap::new(), unit: size.max(1e-9) * 1e-9 }
    }

    /// The vertex at `p`, one per place.
    fn vertex(&mut self, p: DVec3) -> usize {
        let k = [(p.x / self.unit).round() as i64, (p.y / self.unit).round() as i64, (p.z / self.unit).round() as i64];
        let next = self.points.len();
        *self.welded.entry(k).or_insert_with(|| {
            self.points.push(p);
            next
        })
    }

    /// A face through `corners`, turned to face `outward`.
    fn face(&mut self, corners: &[DVec3], outward: DVec3) {
        let mut ids: Vec<usize> = corners.iter().map(|&p| self.vertex(p)).collect();
        ids.dedup();
        while ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        if ids.len() < 3 {
            return;
        }
        let points: Vec<DVec3> = ids.iter().map(|&i| self.points[i]).collect();
        if newell(&points).dot(outward) < 0.0 {
            ids.reverse();
        }
        self.faces.push(ids);
    }

    /// A grid of quads over `point(u, v)`, `u` and `v` running 0..=1 in `nu` and `nv` steps.
    fn grid(&mut self, nu: usize, nv: usize, point: impl Fn(f64, f64) -> DVec3, outward: impl Fn(DVec3) -> DVec3) {
        let (nu, nv) = (nu.max(1), nv.max(1));
        for i in 0..nu {
            for j in 0..nv {
                let (u0, u1) = (i as f64 / nu as f64, (i + 1) as f64 / nu as f64);
                let (v0, v1) = (j as f64 / nv as f64, (j + 1) as f64 / nv as f64);
                let quad = [point(u0, v0), point(u1, v0), point(u1, v1), point(u0, v1)];
                let centre = quad.iter().copied().sum::<DVec3>() / 4.0;
                self.face(&quad, outward(centre));
            }
        }
    }

    fn mesh(self, transform: glam::DMat4) -> Mesh {
        let points: Vec<[f64; 3]> = self.points.iter().map(|&p| transform.transform_point3(p).to_array()).collect();
        mesh_from(&points, self.faces)
    }
}

/// A disc of `rings` rings and `around` sectors at height `z`, its rim `rim(angle)`, facing
/// `up` or down.
fn disc(b: &mut Builder, around: usize, rings: usize, z: f64, rim: impl Fn(f64) -> DVec3, up: bool) {
    let normal = if up { DVec3::Z } else { DVec3::NEG_Z };
    let centre = DVec3::new(0.0, 0.0, z);
    b.grid(around, rings.max(1), |u, v| centre.lerp(rim(u * std::f64::consts::TAU), v), |_| normal);
}

/// The mesh of a solid primitive's parameters, placed as the primitive is.
pub fn primitive(op: &SolidHistoryOperation, d: &Divisions) -> Option<Mesh> {
    use std::f64::consts::{PI, TAU};
    let base = op.base()?;
    let transform = glam::DMat4::from_cols_array(&base.transform);
    let ring = |rx: f64, ry: f64, z: f64| move |a: f64| DVec3::new(rx * a.cos(), ry * a.sin(), z);
    match op {
        SolidHistoryOperation::Box(p) => {
            let (l, w, h) = (p.length, p.width, p.height);
            let mut b = Builder::new(l.max(w).max(h));
            let half = DVec3::new(l, w, h) / 2.0;
            let [nx, ny, nz] = d.box_;
            let corner = |x: f64, y: f64, z: f64| DVec3::new(x, y, z) * 2.0 * half - half;
            let outward = |c: DVec3| {
                let r = c / half;
                let m = r.abs().max_element();
                DVec3::new(if r.x.abs() == m { r.x.signum() } else { 0.0 }, if r.y.abs() == m { r.y.signum() } else { 0.0 }, if r.z.abs() == m { r.z.signum() } else { 0.0 })
            };
            for s in [0.0, 1.0] {
                b.grid(ny, nz, |u, v| corner(s, u, v), outward);
                b.grid(nx, nz, |u, v| corner(u, s, v), outward);
                b.grid(nx, ny, |u, v| corner(u, v, s), outward);
            }
            Some(b.mesh(transform))
        }
        SolidHistoryOperation::Wedge(p) => {
            let (l, w, h) = (p.length, p.width, p.height);
            let mut b = Builder::new(l.max(w).max(h));
            let o = DVec3::new(-l / 2.0, -w / 2.0, -h / 2.0);
            let [nl, nw, nh, ns] = d.wedge;
            let at = |x: f64, y: f64, z: f64| o + DVec3::new(x * l, y * w, z * h);
            b.grid(nl, nw, |u, v| at(u, v, 0.0), |_| DVec3::NEG_Z);
            b.grid(nw, nh, |u, v| at(0.0, u, v), |_| DVec3::NEG_X);
            b.grid(ns, nw, |u, v| at(u, v, 1.0 - u), |_| DVec3::new(h, 0.0, l));
            // The triangular ends, one face each through every corner on their sides.
            for (y, outward) in [(0.0, DVec3::NEG_Y), (1.0, DVec3::Y)] {
                let mut corners = Vec::new();
                corners.extend((0..nl).map(|i| at(i as f64 / nl as f64, y, 0.0)));
                corners.extend((0..ns).map(|i| at(1.0 - i as f64 / ns as f64, y, i as f64 / ns as f64)));
                corners.extend((0..nh).map(|i| at(0.0, y, 1.0 - i as f64 / nh as f64)));
                b.face(&corners, outward);
            }
            Some(b.mesh(transform))
        }
        SolidHistoryOperation::Cylinder(p) => {
            let (rx, ry, h) = (p.major_radius, p.minor_radius, p.height);
            let [around, high, rings] = d.cylinder;
            let mut b = Builder::new(rx.max(ry).max(h));
            b.grid(around, high, |u, v| ring(rx, ry, v * h)(u * TAU), |c| DVec3::new(c.x, c.y, 0.0));
            disc(&mut b, around, rings, 0.0, ring(rx, ry, 0.0), false);
            disc(&mut b, around, rings, h, ring(rx, ry, h), true);
            Some(b.mesh(transform))
        }
        SolidHistoryOperation::Cone(p) => {
            let (rx, ry, h) = (p.base_x_radius, p.base_y_radius, p.height);
            let top = p.top_radius / rx.max(1e-12);
            let [around, high, rings] = d.cone;
            let mut b = Builder::new(rx.max(ry).max(h));
            b.grid(around, high, |u, v| ring(rx * (1.0 - v + v * top), ry * (1.0 - v + v * top), v * h)(u * TAU), |c| DVec3::new(c.x, c.y, 0.0));
            disc(&mut b, around, rings, 0.0, ring(rx, ry, 0.0), false);
            if top > 1e-12 {
                disc(&mut b, around, rings, h, ring(rx * top, ry * top, h), true);
            }
            Some(b.mesh(transform))
        }
        SolidHistoryOperation::Pyramid(p) => {
            let sides = p.sides.max(3) as usize;
            let (r, h, top) = (p.radius, p.height, p.top_radius / p.radius.max(1e-12));
            let [along, high, rings] = d.pyramid;
            let mut b = Builder::new(r.max(h));
            let corner = |k: usize| {
                let a = TAU * k as f64 / sides as f64;
                DVec3::new(r * a.cos(), r * a.sin(), 0.0)
            };
            // The base outline at fraction `u` of the way round.
            let outline = |u: f64| {
                let s = u * sides as f64;
                let k = (s.floor() as usize).min(sides - 1);
                corner(k).lerp(corner(k + 1), s - k as f64)
            };
            let n = along * sides;
            b.grid(n, high, |u, v| {
                let p = outline(u) * (1.0 - v + v * top);
                DVec3::new(p.x, p.y, v * h)
            }, |c| DVec3::new(c.x, c.y, 0.0));
            disc(&mut b, n, rings, 0.0, |a| outline(a / TAU), false);
            if top > 1e-12 {
                disc(&mut b, n, rings, h, |a| outline(a / TAU) * top + DVec3::Z * h, true);
            }
            Some(b.mesh(transform))
        }
        SolidHistoryOperation::Sphere(p) => {
            let r = p.radius;
            let [around, high] = d.sphere;
            let mut b = Builder::new(r);
            b.grid(around, high, |u, v| {
                let (a, t) = (u * TAU, v * PI - PI / 2.0);
                DVec3::new(r * t.cos() * a.cos(), r * t.cos() * a.sin(), r * t.sin())
            }, |c| c);
            Some(b.mesh(transform))
        }
        SolidHistoryOperation::Torus(p) => {
            let (big, small) = (p.major_radius, p.minor_radius);
            let [path, section] = d.torus;
            let mut b = Builder::new(big + small);
            let at = |u: f64, v: f64| {
                let (a, t) = (u * TAU, v * TAU);
                let r = big + small * t.cos();
                DVec3::new(r * a.cos(), r * a.sin(), small * t.sin())
            };
            b.grid(path, section, at, |c| {
                let centre = DVec3::new(c.x, c.y, 0.0).normalize_or_zero() * big;
                c - centre
            });
            Some(b.mesh(transform))
        }
        _ => None,
    }
}

// ── Polygon meshes through curves ───────────────────────────────────────────

/// An M by N polygon mesh through `rows` (each a row of N points), closed in M or N as given.
pub fn polygon_mesh(rows: &[Vec<DVec3>], closed_m: bool, closed_n: bool) -> Option<PolygonMesh> {
    let n = rows.first()?.len();
    if rows.len() < 2 || n < 2 || rows.iter().any(|r| r.len() != n) || rows.len() > i16::MAX as usize || n > i16::MAX as usize {
        return None;
    }
    let mut mesh = PolygonMesh::new();
    mesh.m_vertex_count = rows.len() as i16;
    mesh.n_vertex_count = n as i16;
    if closed_m {
        mesh.flags |= PolygonMeshFlags::CLOSED_M;
    }
    if closed_n {
        mesh.flags |= PolygonMeshFlags::CLOSED_N;
    }
    mesh.vertices = rows.iter().flatten().map(|&p| PolygonMeshVertex::at(vector(p))).collect();
    Some(mesh)
}

/// The Coons patch of four boundary curves sampled as `a` (v = 0), `c` (v = 1), both running
/// with u, and `d0` (u = 0), `d1` (u = 1), both running with v; the corners shared.
pub fn coons(a: &[DVec3], c: &[DVec3], d0: &[DVec3], d1: &[DVec3]) -> Vec<Vec<DVec3>> {
    let (m, n) = (a.len(), d0.len());
    (0..m)
        .map(|i| {
            let u = i as f64 / (m - 1) as f64;
            (0..n)
                .map(|j| {
                    let v = j as f64 / (n - 1) as f64;
                    let ruled_u = a[i] * (1.0 - v) + c[i] * v;
                    let ruled_v = d0[j] * (1.0 - u) + d1[j] * u;
                    let corners = a[0] * (1.0 - u) * (1.0 - v) + a[m - 1] * u * (1.0 - v) + c[0] * (1.0 - u) * v + c[m - 1] * u * v;
                    ruled_u + ruled_v - corners
                })
                .collect()
        })
        .collect()
}

/// `points` revolved about the axis through `origin` along `axis` from `start` through
/// `sweep` radians in `steps` steps: one row per angle.
pub fn revolved(points: &[DVec3], origin: DVec3, axis: DVec3, start: f64, sweep: f64, steps: usize) -> Vec<Vec<DVec3>> {
    let axis = axis.normalize_or_zero();
    let whole = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
    let count = if whole { steps } else { steps + 1 };
    (0..count)
        .map(|k| {
            let angle = start + sweep * k as f64 / steps as f64;
            let turn = glam::DQuat::from_axis_angle(axis, angle);
            points.iter().map(|&p| origin + turn * (p - origin)).collect()
        })
        .collect()
}

// ── Faces for conversion ────────────────────────────────────────────────────

fn loop_points(points: &[[f64; 3]], face: &[usize]) -> Vec<DVec3> {
    face.iter().map(|&v| DVec3::from_array(points[v])).collect()
}

/// Whether a face's corners lie in one plane, within `tolerance`.
fn planar(points: &[DVec3], tolerance: f64) -> bool {
    let normal = newell(points).normalize_or_zero();
    normal != DVec3::ZERO && points.iter().all(|p| (*p - points[0]).dot(normal).abs() <= tolerance)
}

/// `faces` with every face that is not flat cut into a fan of triangles.
pub fn flat_faces(points: &[[f64; 3]], faces: Vec<Vec<usize>>, tolerance: f64) -> Vec<Vec<usize>> {
    let mut out = Vec::with_capacity(faces.len());
    for face in faces {
        if face.len() <= 3 || planar(&loop_points(points, &face), tolerance) {
            out.push(face);
        } else {
            out.extend((1..face.len() - 1).map(|k| vec![face[0], face[k], face[k + 1]]));
        }
    }
    out
}

/// Adjacent faces that lie in one plane merged into one face where together they bound one
/// area without holes.
pub fn merge_coplanar(points: &[[f64; 3]], faces: Vec<Vec<usize>>, tolerance: f64) -> Vec<Vec<usize>> {
    let planes: Vec<Option<(DVec3, DVec3)>> = faces
        .iter()
        .map(|f| {
            let p = loop_points(points, f);
            planar(&p, tolerance).then(|| (newell(&p).normalize(), p[0]))
        })
        .collect();
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn root(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    let mut by_edge: HashMap<Key, Vec<usize>> = HashMap::new();
    for (f, v) in faces.iter().enumerate() {
        for k in 0..v.len() {
            by_edge.entry(key(v[k], v[(k + 1) % v.len()])).or_default().push(f);
        }
    }
    for users in by_edge.values() {
        if let [a, b] = users[..] {
            if let (Some((na, pa)), Some((nb, pb))) = (planes[a], planes[b]) {
                if na.dot(nb) > 1.0 - 1e-9 && (pb - pa).dot(na).abs() <= tolerance {
                    let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                    parent[ra] = rb;
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for f in 0..faces.len() {
        let r = root(&mut parent, f);
        groups.entry(r).or_default().push(f);
    }
    let mut order: Vec<Vec<usize>> = groups.into_values().collect();
    order.sort_by_key(|g| g[0]);
    let mut out = Vec::new();
    for group in order {
        if group.len() > 1 {
            if let Some(merged) = single_loop(&boundary(group.iter().map(|&f| faces[f].as_slice()))) {
                out.push(merged);
                continue;
            }
        }
        out.extend(group.into_iter().map(|f| faces[f].clone()));
    }
    out
}

/// Triangles paired into quads across their shared edges where both belong to one `group`.
pub fn paired_quads(triangles: &[[usize; 3]], groups: &[u32]) -> Vec<Vec<usize>> {
    let mut by_edge: HashMap<Key, Vec<usize>> = HashMap::new();
    for (t, v) in triangles.iter().enumerate() {
        for k in 0..3 {
            by_edge.entry(key(v[k], v[(k + 1) % 3])).or_default().push(t);
        }
    }
    let mut used = vec![false; triangles.len()];
    let mut out = Vec::new();
    for t in 0..triangles.len() {
        if used[t] {
            continue;
        }
        used[t] = true;
        let v = triangles[t];
        let partner = (0..3).find_map(|k| {
            let (a, b) = (v[k], v[(k + 1) % 3]);
            let u = *by_edge[&key(a, b)].iter().find(|&&u| u != t && !used[u] && groups.get(u) == groups.get(t))?;
            let d = *triangles[u].iter().find(|&&x| x != a && x != b)?;
            Some((u, [a, d, b, v[(k + 2) % 3]]))
        });
        match partner {
            Some((u, quad)) => {
                used[u] = true;
                out.push(quad.to_vec());
            }
            None => out.push(v.to_vec()),
        }
    }
    out
}

/// Whether every edge of `faces` is shared by exactly two of them.
pub fn closed(faces: &[Vec<usize>]) -> bool {
    let mut count: HashMap<Key, usize> = HashMap::new();
    for v in faces {
        for k in 0..v.len() {
            *count.entry(key(v[k], v[(k + 1) % v.len()])).or_default() += 1;
        }
    }
    !count.is_empty() && count.values().all(|&c| c == 2)
}

/// `faces` turned over if they enclose a negative volume (facing inward).
pub fn outward(points: &[[f64; 3]], mut faces: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    let volume: f64 = faces
        .iter()
        .map(|f| {
            let p = loop_points(points, f);
            (1..p.len() - 1).map(|k| p[0].dot(p[k].cross(p[k + 1]))).sum::<f64>()
        })
        .sum();
    if volume < 0.0 {
        for f in &mut faces {
            f.reverse();
        }
    }
    faces
}

/// Points welded where they coincide (within `tolerance`): the kept points and each input
/// point's index among them.
pub fn weld(points: &[[f64; 3]], tolerance: f64) -> (Vec<[f64; 3]>, Vec<usize>) {
    let unit = tolerance.max(1e-12);
    let mut cells: HashMap<[i64; 3], usize> = HashMap::new();
    let mut kept = Vec::new();
    let map = points
        .iter()
        .map(|p| {
            let k = p.map(|x| (x / unit).round() as i64);
            *cells.entry(k).or_insert_with(|| {
                kept.push(*p);
                kept.len() - 1
            })
        })
        .collect();
    (kept, map)
}
