// Solidcore records built, moved, measured and meshed for display.

use glam::DVec3;
use ocs_solid::model::{self, r, BooleanOp, Brep};

use crate::scene::model::mesh_model::{MassProperties, MeshLodSet, MeshModel};
use crate::scene::model::silhouette::SilhouetteSource;
use crate::scene::model::wire_model::WireModel;

/// The facet angle a display asks for by default (7.5°).
pub const DEFAULT_ANGLE: f64 = std::f64::consts::PI / 24.0;
/// Coarsest chordal display angle: 20 degrees.
const DISPLAY_ANGLE: f64 = std::f64::consts::PI / 9.0;
/// Finest display angle: 5 degrees.
const FINE_ANGLE: f64 = std::f64::consts::PI / 36.0;

fn resolution(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value.clamp(0.01, 10.0)
    } else {
        1.0
    }
}

/// The facet angle a display resolution asks for: finer for a higher resolution.
pub fn angle_for_resolution(value: f64) -> f64 {
    (DEFAULT_ANGLE / resolution(value).sqrt()).clamp(FINE_ANGLE, std::f64::consts::FRAC_PI_2)
}

/// The facet angle of a display with a chordal deflection.
pub fn display_angle_for_resolution(value: f64) -> f64 {
    (DISPLAY_ANGLE / resolution(value).max(1.0).sqrt()).clamp(FINE_ANGLE, std::f64::consts::FRAC_PI_2)
}

/// What a display mesh shows: the facet angle, the chordal deflection, the isoline counts, and
/// whether they are a surface's own U/V lines (`Some`, with whether planes get them too) or a
/// solid's (`None`), spaced as the drawing's isoline setting spaces them.
#[derive(Clone, Copy, Debug)]
pub struct DisplayTolerance {
    pub angle: f64,
    pub chord: Option<f64>,
    pub isolines: [usize; 2],
    pub planar_isolines: Option<bool>,
}

impl DisplayTolerance {
    pub fn new(angle: f64) -> Self {
        Self {
            angle,
            chord: None,
            isolines: [0; 2],
            planar_isolines: Some(false),
        }
    }

    /// With a body's isolines: a surface's own U/V counts and planar switch, or
    /// (`planar_isolines` `None`) a solid's.
    pub fn with_isolines(mut self, isolines: [usize; 2], planar_isolines: Option<bool>) -> Self {
        self.isolines = isolines;
        self.planar_isolines = planar_isolines;
        self
    }

    pub fn with_chord(mut self, chord: Option<f64>) -> Self {
        self.chord = chord.filter(|c| c.is_finite() && *c > 0.0);
        self
    }
}

/// The display mesh of `brep`.
pub fn tessellate(brep: &Brep, tolerance: DisplayTolerance) -> Option<ocs_solid::display::RecordMesh> {
    let display = ocs_solid::display::Display {
        angle: tolerance.angle,
        chord: tolerance.chord,
        isolines: tolerance.isolines,
        planar_isolines: tolerance.planar_isolines.unwrap_or(false),
        solid_isolines: tolerance.planar_isolines.is_none(),
    };
    ocs_solid::display::mesh(brep, &display, model::size(brep)).ok()
}

/// A preview cage of `brep`: its wireframe edges, then `isolines` solid isolines.
pub fn preview_curves(brep: &Brep, isolines: usize) -> Vec<Vec<[f64; 3]>> {
    let Some(mesh) = tessellate(
        brep,
        DisplayTolerance::new(DEFAULT_ANGLE).with_isolines([isolines, isolines], None),
    ) else {
        return Vec::new();
    };
    mesh.edges
        .into_iter()
        .chain(mesh.isolines)
        .map(|(_, points)| points)
        .filter(|points| points.len() >= 2)
        .collect()
}

fn tessellation(brep: &Brep) -> Option<ocs_solid::display::RecordMesh> {
    tessellate(brep, DisplayTolerance::new(DEFAULT_ANGLE))
}

fn display_tessellation(
    brep: &Brep,
    facet_resolution: f64,
    chordal_deflection: Option<f64>,
    isolines: [usize; 2],
    planar_isolines: Option<bool>,
) -> Option<ocs_solid::display::RecordMesh> {
    let angle = match chordal_deflection {
        Some(_) => display_angle_for_resolution(facet_resolution),
        None => angle_for_resolution(facet_resolution),
    };
    tessellate(
        brep,
        DisplayTolerance::new(angle)
            .with_isolines(isolines, planar_isolines)
            .with_chord(chordal_deflection),
    )
}

// ── Primitives ──────────────────────────────────────────────────────────────

/// Axis-aligned box from its center and full extents.
pub fn box_solid(center: [f64; 3], length: f64, width: f64, height: f64) -> Option<Brep> {
    model::make_box(
        [center[0] - length / 2.0, center[1] - width / 2.0, center[2] - height / 2.0],
        [length, width, height],
    )
    .ok()
}

fn line(start: [f64; 3], end: [f64; 3]) -> solidcore_ops::sweep::Curve {
    solidcore_ops::sweep::Curve::Line { start, end }
}

/// A closed polygon through `corners`, as sweep curves.
fn polygon(corners: &[[f64; 3]]) -> Vec<solidcore_ops::sweep::Curve> {
    (0..corners.len())
        .map(|i| line(corners[i], corners[(i + 1) % corners.len()]))
        .collect()
}

/// A full ellipse about `centre` in the plane z = centre.z, as sweep curves.
fn ellipse(centre: [f64; 3], x_radius: f64, y_radius: f64) -> Vec<solidcore_ops::sweep::Curve> {
    let frame = r::Frame {
        origin: centre,
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    let curve = if (x_radius - y_radius).abs() <= 1e-12 * x_radius.abs().max(1.0) {
        r::Curve::Circle { frame, radius: x_radius }
    } else {
        r::Curve::Ellipse { frame, radii: [x_radius, y_radius] }
    };
    solidcore_ops::sweep::Curve::from_record(&curve, [0.0, std::f64::consts::TAU]).unwrap_or_default()
}

/// Right triangular prism (wedge): right-triangle cross-section in XZ, extruded along Y.
/// `origin` is the min corner, the ramp falling from the full height at x = 0 to nothing at
/// x = length.
pub fn wedge_solid(origin: [f64; 3], length: f64, width: f64, height: f64) -> Option<Brep> {
    let at = |x: f64, z: f64| [origin[0] + x, origin[1], origin[2] + z];
    model::extrude(&polygon(&[at(0.0, 0.0), at(length, 0.0), at(0.0, height)]), [0.0, 1.0, 0.0], width, 0.0, true).ok()
}

/// Solid cylinder standing on the z = base plane.
pub fn cylinder_solid(center: [f64; 3], radius: f64, height: f64) -> Option<Brep> {
    model::make_cylinder(center, radius, height).ok()
}

/// Circular or elliptical cone/frustum standing on the z = base plane; an elliptical one keeps
/// its base's proportions up to the top.
pub fn cone_frustum_solid(
    center: [f64; 3],
    base_x_radius: f64,
    base_y_radius: f64,
    top_radius: f64,
    height: f64,
) -> Option<Brep> {
    let scale = base_x_radius.abs().max(base_y_radius.abs()).max(1.0);
    let circular = (base_x_radius - base_y_radius).abs() <= 1e-12 * scale;
    if circular {
        return model::make_cone(center, base_x_radius, top_radius, height).ok();
    }
    let top = [center[0], center[1], center[2] + height];
    let base = ellipse(center, base_x_radius, base_y_radius);
    if top_radius <= 1e-12 * scale {
        return model::loft_to_point(&base, top).ok();
    }
    let ratio = top_radius / base_x_radius;
    model::loft(&[base, ellipse(top, top_radius, base_y_radius * ratio)], 1, true).ok()
}

/// Circular or elliptical cylinder standing on the local z = base plane.
pub fn elliptical_cylinder_solid(center: [f64; 3], major_radius: f64, minor_radius: f64, height: f64) -> Option<Brep> {
    if (major_radius - minor_radius).abs() <= 1e-12 * major_radius.abs().max(1.0) {
        return cylinder_solid(center, major_radius, height);
    }
    model::extrude(&ellipse(center, major_radius, minor_radius), [0.0, 0.0, 1.0], height, 0.0, true).ok()
}

/// Solid sphere about `center`.
pub fn sphere_solid(center: [f64; 3], radius: f64) -> Option<Brep> {
    model::make_sphere(center, radius).ok()
}

/// Solid torus in the z = base plane (tube revolved about the Z axis).
pub fn torus_solid(center: [f64; 3], major: f64, minor: f64) -> Option<Brep> {
    model::make_torus(center, major, minor).ok()
}

fn regular_polygon(centre: [f64; 3], radius: f64, sides: usize) -> Vec<[f64; 3]> {
    (0..sides)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / sides as f64;
            [centre[0] + radius * angle.cos(), centre[1] + radius * angle.sin(), centre[2]]
        })
        .collect()
}

/// Solid pyramid on a regular polygon of `sides` corners.
pub fn pyramid_solid(center: [f64; 3], radius: f64, height: f64, sides: usize) -> Option<Brep> {
    pyramid_frustum_solid(center, radius, 0.0, height, sides)
}

/// Solid pyramid or polygonal frustum on a regular polygon of `sides` corners.
pub fn pyramid_frustum_solid(center: [f64; 3], base_radius: f64, top_radius: f64, height: f64, sides: usize) -> Option<Brep> {
    if sides < 3 || !(base_radius > 0.0) || !(height > 0.0) || !(top_radius >= 0.0) {
        return None;
    }
    let base = polygon(&regular_polygon(center, base_radius, sides));
    let top = [center[0], center[1], center[2] + height];
    if top_radius <= 1e-12 {
        return model::loft_to_point(&base, top).ok();
    }
    model::loft(&[base, polygon(&regular_polygon(top, top_radius, sides))], 1, true).ok()
}

// ── Placement ───────────────────────────────────────────────────────────────

/// Moves a body by a rigid transform, given as three axes and an origin.
pub fn placed(body: &Brep, x: [f64; 3], y: [f64; 3], z: [f64; 3], origin: [f64; 3]) -> Option<Brep> {
    model::transform(body, &model::affine(x, y, z, origin)).ok()
}

/// Turns a body about one of the world axes, through the point `about`.
pub fn turned(body: &Brep, axis: usize, angle: f64, about: [f64; 3]) -> Option<Brep> {
    let (sin, cos) = angle.sin_cos();
    let (x, y, z) = match axis {
        0 => ([1.0, 0.0, 0.0], [0.0, cos, sin], [0.0, -sin, cos]),
        1 => ([cos, 0.0, -sin], [0.0, 1.0, 0.0], [sin, 0.0, cos]),
        _ => ([cos, sin, 0.0], [-sin, cos, 0.0], [0.0, 0.0, 1.0]),
    };
    placed(body, x, y, z, about_origin(x, y, z, about))
}

/// Reflects a body in the plane across one of the world axes, through `about`.
pub fn mirrored(body: &Brep, axis: usize, about: [f64; 3]) -> Option<Brep> {
    let mut columns = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    columns[axis][axis] = -1.0;
    let [x, y, z] = columns;
    placed(body, x, y, z, about_origin(x, y, z, about))
}

/// Moves a body by a transform given as a column-major 4×4.
pub fn by_matrix(body: &Brep, matrix: [f64; 16]) -> Option<Brep> {
    model::transform(body, &model::affine_of_columns(matrix)).ok()
}

/// Where a transform's origin has to sit for it to act about `about`: `about − M·about`.
fn about_origin(x: [f64; 3], y: [f64; 3], z: [f64; 3], about: [f64; 3]) -> [f64; 3] {
    let mut origin = about;
    for axis in 0..3 {
        origin[axis] -= x[axis] * about[0] + y[axis] * about[1] + z[axis] * about[2];
    }
    origin
}

/// The box a body occupies.
pub fn extent(body: &Brep) -> Option<([f64; 3], [f64; 3])> {
    if let Ok(found) = model::bounds(body) {
        return Some(found);
    }
    let mesh = tessellation(body)?;
    bounds_of(&mesh.positions)
}

fn bounds_of(points: &[[f64; 3]]) -> Option<([f64; 3], [f64; 3])> {
    if points.is_empty() {
        return None;
    }
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    Some((low, high))
}

/// Where an axis-aligned plane cuts a body, as line segments off its mesh: a section of a cone
/// by a slanted plane is a conic, of a torus a quartic, and the answer wanted here is a set of
/// Line entities either way.
pub fn section(body: &Brep, axis: usize, value: f64) -> Vec<([f64; 3], [f64; 3])> {
    let Some(mesh) = tessellation(body) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for triangle in &mesh.triangles {
        let corners = triangle.map(|i| mesh.positions[i]);
        let mut hits: Vec<[f64; 3]> = Vec::new();
        for step in 0..3 {
            let (from, to) = (corners[step], corners[(step + 1) % 3]);
            let (a, b) = (from[axis] - value, to[axis] - value);
            if (a > 0.0) == (b > 0.0) || a == b {
                continue;
            }
            let along = a / (a - b);
            hits.push([0, 1, 2].map(|k| from[k] + (to[k] - from[k]) * along));
        }
        if hits.len() == 2 {
            let span: f64 = (0..3).map(|k| (hits[0][k] - hits[1][k]).abs()).sum();
            if span > 1e-9 {
                out.push((hits[0], hits[1]));
            }
        }
    }
    out
}

// ── Edges and faces ─────────────────────────────────────────────────────────

/// The body's edges a wireframe draws, as document `Wire`s: stored on the entity for picking.
pub fn edge_wires(body: &Brep) -> Vec<document::entities::Wire> {
    use document::types::Vector3;
    tessellation(body)
        .map(|mesh| {
            mesh.edges
                .into_iter()
                .map(|(_, points)| {
                    document::entities::Wire::from_points(points.iter().map(|p| Vector3::new(p[0], p[1], p[2])).collect())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// White wireframe used while a solid-history grip is hot. Surface previews include the
/// requested construction isolines so their cage follows the edit.
pub fn grip_preview_wires(body: &Brep, handle: document::Handle, isolines: [usize; 2], planar_isolines: bool) -> Vec<WireModel> {
    let Some(mesh) = tessellate(body, DisplayTolerance::new(DEFAULT_ANGLE).with_isolines(isolines, Some(planar_isolines))) else {
        return Vec::new();
    };
    mesh.edges
        .into_iter()
        .map(|(_, points)| points)
        .chain(mesh.isolines.into_iter().map(|(_, points)| points))
        .filter(|points| points.len() >= 2)
        .map(|points| WireModel::solid_f64(format!("{}-GRIP-PREVIEW", handle.value()), points, WireModel::WHITE, false))
        .collect()
}

/// B-rep edge nearest a world-space surface pick.
pub fn nearest_edge(body: &Brep, pick: [f64; 3]) -> Option<u32> {
    model::nearest_edge(body, pick)
}

/// The boundary edges of `face` as world-space polylines.
pub fn face_outline(body: &Brep, face: u32) -> Vec<Vec<[f64; 3]>> {
    model::face_edges(body, face)
        .into_iter()
        .filter_map(|(edge, _)| model::edge_points(body, edge, 48))
        .collect()
}

/// Planar face nearest a world-space surface pick.
pub fn nearest_planar_face(body: &Brep, pick: [f64; 3]) -> Option<u32> {
    let p = DVec3::from_array(pick);
    (0..body.faces.len() as u32)
        .filter_map(|face| {
            let (origin, normal) = model::face_plane(body, face)?;
            Some((face, (p - DVec3::from_array(origin)).dot(DVec3::from_array(normal)).abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(face, _)| face)
}

/// Outward normal of a planar face.
pub fn planar_face_normal(body: &Brep, face: u32) -> Option<[f64; 3]> {
    model::face_plane(body, face).map(|(_, normal)| normal)
}

/// The vertices a face's loops pass, in loop order (each edge's start as the loop runs it).
fn face_vertices(body: &Brep, face: u32) -> Vec<(u32, [f64; 3])> {
    model::face_edges(body, face)
        .into_iter()
        .filter_map(|(edge, forward)| {
            let node = body.edges.get(edge as usize)?;
            let v = node.vertices[if forward { 0 } else { 1 }];
            Some((v, body.vertices.get(v as usize)?.point))
        })
        .collect()
}

/// Construct a UCS aligned with a solid's planar face: its origin on the face vertex closest to
/// the pick point, its X along the boundary edge from that vertex closest to the pick, its Z
/// the face's outward normal.
pub fn planar_face_ucs(body: &Brep, face: u32, pick: [f64; 3]) -> Option<document::tables::Ucs> {
    let normal = planar_face_normal(body, face)?;
    let z = DVec3::from_array(normal).normalize_or_zero();
    if z.length_squared() < 1e-12 {
        return None;
    }
    let pick_vec = DVec3::from_array(pick);
    let origin = face_vertices(body, face)
        .into_iter()
        .filter(|(_, p)| p.iter().all(|c| c.is_finite()))
        .min_by(|a, b| DVec3::from_array(a.1).distance_squared(pick_vec).total_cmp(&DVec3::from_array(b.1).distance_squared(pick_vec)));
    let (origin_vertex, origin) = match origin {
        Some((v, p)) => (Some(v), DVec3::from_array(p)),
        None => (None, pick_vec),
    };
    let mut best: Option<(f64, DVec3)> = None;
    if let Some(vertex) = origin_vertex {
        for (edge, _) in model::face_edges(body, face) {
            let Some(node) = body.edges.get(edge as usize) else { continue };
            if !node.vertices.contains(&vertex) {
                continue;
            }
            let Some(points) = model::edge_points(body, edge, 16) else { continue };
            // The direction the edge leaves the vertex in, and how near its first span runs
            // to the pick.
            let (a, b) = if node.vertices[0] == vertex {
                (points[0], points[1])
            } else {
                (points[points.len() - 1], points[points.len() - 2])
            };
            let direction = DVec3::from_array(b) - DVec3::from_array(a);
            let distance = model::segment_distance(pick, a, b);
            if direction.length_squared() > 1e-24 && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, direction));
            }
        }
    }
    let x = best
        .and_then(|(_, dir)| {
            let in_plane = dir - z * dir.dot(z);
            let unit_x = in_plane.normalize_or_zero();
            (unit_x.length_squared() > 1e-6).then_some(unit_x)
        })
        .unwrap_or_else(|| {
            let ((xx, xy, xz), _) = crate::scene::view::transform::ocs_axes((z.x, z.y, z.z));
            DVec3::new(xx, xy, xz)
        });
    let y = z.cross(x).normalize_or_zero();
    let mut ucs = document::tables::Ucs::new("*ACTIVE*");
    ucs.origin = document::types::Vector3::new(origin.x, origin.y, origin.z);
    ucs.x_axis = document::types::Vector3::new(x.x, x.y, x.z);
    ucs.y_axis = document::types::Vector3::new(y.x, y.y, y.z);
    Some(ucs)
}

/// Centre of every B-rep face: the average of its boundary-loop vertices. Exact for planar
/// faces; for curved faces the loop average floats slightly inside the surface. Capped like the
/// edge snaps.
pub fn face_centers(body: &Brep) -> Vec<[f64; 3]> {
    const MAX_FACE_CENTERS: usize = 2048;
    let mut out = Vec::new();
    for face in 0..body.faces.len() as u32 {
        let corners: Vec<[f64; 3]> = face_vertices(body, face).into_iter().map(|(_, p)| p).collect();
        if corners.len() >= 3 {
            let n = corners.len() as f64;
            out.push([0, 1, 2].map(|k| corners.iter().map(|p| p[k]).sum::<f64>() / n));
        }
    }
    if out.len() > MAX_FACE_CENTERS {
        let step = out.len().div_ceil(MAX_FACE_CENTERS);
        out = out.into_iter().step_by(step).collect();
    }
    out
}

// ── Boolean operations ──────────────────────────────────────────────────────

/// Which CSG to apply. Mirrors `model::boolean_cmd::BoolOp` but kept local so this scene module
/// has no dependency on the UI module.
#[derive(Clone, Copy)]
pub enum Bool {
    Union,
    Subtract,
    Intersect,
}

/// Combine two solids. `Subtract` removes `b` from `a`; `None` when Solidcore refuses or
/// nothing is left.
pub fn boolean(op: Bool, a: &Brep, b: &Brep) -> Option<Brep> {
    boolean_result(op, a, b).ok().flatten()
}

/// Combine two solids while retaining Solidcore's refusal.
pub fn boolean_result(op: Bool, a: &Brep, b: &Brep) -> Result<Option<Brep>, model::Error> {
    let how = match op {
        Bool::Union => BooleanOp::Union,
        Bool::Subtract => BooleanOp::Subtract,
        Bool::Intersect => BooleanOp::Intersect,
    };
    model::boolean(a, b, how)
}

// ── Tessellation ────────────────────────────────────────────────────────────

fn split(point: [f64; 3]) -> ([f32; 3], [f32; 3]) {
    let high = point.map(|v| v as f32);
    let low = [0, 1, 2].map(|k| (point[k] - high[k] as f64) as f32);
    (high, low)
}

fn mesh_from_tessellation(mesh: ocs_solid::display::RecordMesh, color: [f32; 4]) -> Option<MeshLodSet> {
    if mesh.triangles.is_empty() {
        return None;
    }
    let silhouette = SilhouetteSource::of_mesh(&mesh.positions, &mesh.normals, &mesh.triangles, &mesh.triangle_faces);
    // The renderer holds each position as a coarse float plus a fine correction, so a survey
    // coordinate keeps its last millimetres instead of losing them to f32.
    let (verts, verts_low): (Vec<_>, Vec<_>) = mesh.positions.iter().map(|p| split(*p)).unzip();
    let normals = mesh.normals.iter().map(|n| n.map(|v| v as f32)).collect();
    let indices = mesh.triangles.iter().flat_map(|t| t.map(|i| i as u32)).collect();
    let mut set = MeshLodSet::from_single(MeshModel {
        name: String::new(),
        verts,
        verts_low,
        normals,
        indices,
        triangle_material_handles: Vec::new(),
        triangle_colors: Vec::new(),
        color,
        selected: false,
    });
    for (_, points) in mesh.edges.iter().chain(mesh.isolines.iter()) {
        for segment in points.windows(2) {
            for point in segment {
                let (high, low) = split(*point);
                set.edge_verts.push(high);
                set.edge_verts_low.push(low);
            }
        }
    }
    set.complete = mesh.missing_faces.is_empty();
    set.curved_gens.push(super::mesh_model::CurvedGen { source: silhouette });
    set.build_display_lods();
    Some(set)
}

pub fn display_from_solid(
    body: &Brep,
    color: [f32; 4],
    facet_resolution: f64,
    chordal_deflection: Option<f64>,
    isolines: [usize; 2],
    planar_isolines: Option<bool>,
) -> Option<(MeshLodSet, Vec<document::entities::Wire>, [f64; 3])> {
    use document::types::Vector3;
    let tessellation = display_tessellation(body, facet_resolution, chordal_deflection, isolines, planar_isolines)?;
    let (low, high) = bounds_of(&tessellation.positions)?;
    let center = [0, 1, 2].map(|k| (low[k] + high[k]) * 0.5);
    let wires = tessellation
        .edges
        .iter()
        .map(|(_, points)| document::entities::Wire::from_points(points.iter().map(|p| Vector3::new(p[0], p[1], p[2])).collect()))
        .collect();
    let mut mesh = mesh_from_tessellation(tessellation, color)?;
    if model::is_solid(body) {
        if let Ok(estimate) = model::mass_properties(body) {
            mesh.apply_mass_properties(MassProperties::from_estimate(&estimate));
        }
    }
    Some((mesh, wires, center))
}

/// The middle of a body, for a caller needing a point to turn or scale about.
pub fn centre(body: &Brep) -> Option<[f64; 3]> {
    let (low, high) = extent(body)?;
    Some([0, 1, 2].map(|k| (low[k] + high[k]) * 0.5))
}

/// How much a body encloses.
pub fn volume(body: &Brep) -> f64 {
    model::volume(body).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri_count(body: &Brep) -> usize {
        display_from_solid(body, [0.7, 0.7, 0.7, 1.0], 1.0, None, [0; 2], None)
            .map(|(m, _, _)| m.lods[0].indices.len() / 3)
            .unwrap_or(0)
    }

    #[test]
    fn all_primitives_triangulate() {
        let c = [0.0, 0.0, 0.0];
        assert!(tri_count(&box_solid(c, 10.0, 10.0, 10.0).unwrap()) >= 12, "box");
        assert!(tri_count(&wedge_solid(c, 10.0, 10.0, 10.0).unwrap()) >= 6, "wedge");
        assert!(tri_count(&cylinder_solid(c, 5.0, 12.0).unwrap()) > 20, "cylinder");
        assert!(tri_count(&cone_frustum_solid(c, 5.0, 5.0, 0.0, 12.0).unwrap()) > 10, "cone");
        assert!(tri_count(&sphere_solid(c, 5.0).unwrap()) > 50, "sphere");
        assert!(tri_count(&torus_solid(c, 8.0, 2.0).unwrap()) > 50, "torus");
        assert!(tri_count(&pyramid_solid(c, 5.0, 9.0, 6).unwrap()) >= 8, "pyramid");
    }

    #[test]
    fn box_face_centers_are_six() {
        let body = box_solid([0.0, 0.0, 0.0], 10.0, 10.0, 10.0).unwrap();
        let centers = face_centers(&body);
        assert_eq!(centers.len(), 6, "a box has six faces: {centers:?}");
        for expected in [
            [5.0, 0.0, 0.0],
            [-5.0, 0.0, 0.0],
            [0.0, 5.0, 0.0],
            [0.0, -5.0, 0.0],
            [0.0, 0.0, 5.0],
            [0.0, 0.0, -5.0],
        ] {
            assert!(
                centers.iter().any(|c| (0..3).all(|k| (c[k] - expected[k]).abs() < 1e-9)),
                "missing face center {expected:?} in {centers:?}"
            );
        }
    }

    #[test]
    fn every_primitive_is_the_size_it_was_asked_for() {
        use std::f64::consts::PI;
        let c = [0.0, 0.0, 0.0];
        let cases: [(Brep, f64); 7] = [
            (box_solid(c, 10.0, 4.0, 6.0).unwrap(), 240.0),
            (cylinder_solid(c, 5.0, 12.0).unwrap(), PI * 25.0 * 12.0),
            (cone_frustum_solid(c, 5.0, 5.0, 0.0, 12.0).unwrap(), PI * 25.0 * 12.0 / 3.0),
            (sphere_solid(c, 5.0).unwrap(), 4.0 / 3.0 * PI * 125.0),
            (torus_solid(c, 8.0, 2.0).unwrap(), 2.0 * PI * PI * 8.0 * 4.0),
            (wedge_solid(c, 10.0, 4.0, 6.0).unwrap(), 120.0),
            (elliptical_cylinder_solid(c, 5.0, 3.0, 2.0).unwrap(), PI * 15.0 * 2.0),
        ];
        for (body, expected) in cases {
            let got = volume(&body);
            assert!((got - expected).abs() < 1e-6 * expected, "{got} vs {expected}");
        }
    }

    #[test]
    fn booleans_produce_solids() {
        let a = box_solid([0.0, 0.0, 0.0], 10.0, 10.0, 10.0).unwrap();
        let b = box_solid([5.0, 5.0, 5.0], 10.0, 10.0, 10.0).unwrap();
        for (op, label) in [(Bool::Union, "union"), (Bool::Subtract, "subtract"), (Bool::Intersect, "intersect")] {
            let r = boolean(op, &a, &b);
            let n = r.as_ref().map(tri_count).unwrap_or(0);
            assert!(r.is_some() && n > 0, "{label} produced nothing");
        }
    }

    #[test]
    fn box_exposes_edges() {
        assert_eq!(edge_wires(&box_solid([0.0, 0.0, 0.0], 10.0, 10.0, 10.0).unwrap()).len(), 12);
    }

    #[test]
    fn placing_a_body_moves_it_without_changing_its_size() {
        let body = box_solid([0.0, 0.0, 0.0], 10.0, 4.0, 6.0).unwrap();
        let moved = placed(&body, [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [5.0, 0.0, 0.0]).expect("a turned box");
        assert!((volume(&moved) - 240.0).abs() < 1e-6, "{}", volume(&moved));
        let middle = centre(&moved).unwrap();
        assert!((middle[0] - 5.0).abs() < 1e-9, "{middle:?}");
        let (low, high) = extent(&moved).unwrap();
        assert!((high[1] - low[1] - 10.0).abs() < 1e-9, "{low:?} {high:?}");
        assert!((high[0] - low[0] - 4.0).abs() < 1e-9, "{low:?} {high:?}");
    }
}
