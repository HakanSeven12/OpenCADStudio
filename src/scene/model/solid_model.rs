// Kernel B-rep construction and display tessellation.

use cadkernel::brep::{self, Body, Curve3, EdgeKey, FaceKey, Surface};

use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};
use crate::scene::model::wire_model::WireModel;

/// What counts as the same point when the kernel checks a body over.
const TOL: f64 = 1e-9;

fn tessellation(body: &Body) -> brep::mesh::BodyMesh {
    brep::mesh::tessellate(
        body,
        brep::mesh::TessellationTolerance::new(
            cadkernel::tessellation::DEFAULT_ANGLE,
            TOL,
        ),
    )
}

fn display_tessellation(
    body: &Body,
    facet_resolution: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
) -> brep::mesh::BodyMesh {
    let resolution = if facet_resolution.is_finite() && facet_resolution > 0.0 {
        facet_resolution.clamp(0.01, 10.0)
    } else {
        1.0
    };
    let max_angle = chordal_deflection.map_or_else(
        || cadkernel::tessellation::angle_for_resolution(resolution),
        |_| cadkernel::tessellation::display_angle_for_resolution(resolution),
    );
    let mut tolerance = brep::mesh::TessellationTolerance::new(max_angle, TOL)
        .with_isolines(isolines);
    if let Some(deflection) = chordal_deflection {
        tolerance = tolerance.with_chordal_deflection(deflection);
    }
    brep::mesh::tessellate(body, tolerance)
}

/// Axis-aligned box from its center and full extents.
pub fn box_solid(center: [f64; 3], length: f64, width: f64, height: f64) -> Option<Body> {
    brep::make::cuboid(
        [
            center[0] - length / 2.0,
            center[1] - width / 2.0,
            center[2] - height / 2.0,
        ],
        [length, width, height],
    )
}

/// Right triangular prism (wedge): right-triangle cross-section in XZ,
/// extruded along Y. `origin` is the min corner, ramp rising in +X/+Z.
pub fn wedge_solid(origin: [f64; 3], length: f64, width: f64, height: f64) -> Option<Body> {
    brep::make::wedge(origin, length, width, height)
}

/// Solid cylinder standing on the z = base plane.
pub fn cylinder_solid(center: [f64; 3], radius: f64, height: f64) -> Option<Body> {
    brep::make::cylinder(center, radius, height)
}

/// Circular or elliptical cone/frustum standing on the z = base plane.
pub fn cone_frustum_solid(
    center: [f64; 3],
    base_x_radius: f64,
    base_y_radius: f64,
    top_radius: f64,
    height: f64,
) -> Option<Body> {
    brep::make::frustum(
        center,
        base_x_radius,
        base_y_radius,
        top_radius,
        height,
    )
}

/// Circular or elliptical cylinder standing on the local z = base plane.
pub fn elliptical_cylinder_solid(
    center: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
    height: f64,
) -> Option<Body> {
    brep::make::elliptical_cylinder(center, major_radius, minor_radius, height)
}

/// Solid sphere about `center`.
pub fn sphere_solid(center: [f64; 3], radius: f64) -> Option<Body> {
    brep::make::sphere(center, radius)
}

/// Solid torus in the z = base plane (tube revolved about the Z axis).
pub fn torus_solid(center: [f64; 3], major: f64, minor: f64) -> Option<Body> {
    brep::make::torus(center, major, minor)
}

/// Solid pyramid on a regular polygon of `sides` corners.
pub fn pyramid_solid(center: [f64; 3], radius: f64, height: f64, sides: usize) -> Option<Body> {
    brep::make::pyramid(center, radius, height, sides)
}

/// Solid pyramid or polygonal frustum on a regular polygon of `sides` corners.
pub fn pyramid_frustum_solid(
    center: [f64; 3],
    base_radius: f64,
    top_radius: f64,
    height: f64,
    sides: usize,
) -> Option<Body> {
    brep::make::pyramid_frustum(center, base_radius, top_radius, height, sides)
}

// ── Placement ───────────────────────────────────────────────────────────────

/// Moves a body by a rigid transform, given as three axes and an origin.
///
/// The Model tab builds every primitive in its own upright frame and then
/// puts it on the working plane, which is the only reason this exists. A
/// body carries analytic surfaces, so moving it moves their frames rather
/// than any points.
pub fn placed(
    body: &Body,
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
    origin: [f64; 3],
) -> Option<Body> {
    brep::transform(
        body,
        &brep::Placement {
            x_axis: x,
            y_axis: y,
            z_axis: z,
            origin,
        },
    )
}

/// Turns a body about one of the world axes, through the point `about`.
pub fn turned(body: &Body, axis: usize, angle: f64, about: [f64; 3]) -> Option<Body> {
    let (sin, cos) = angle.sin_cos();
    // The rotation's columns, written out per axis rather than assembled from
    // a general formula: three cases are shorter than the axis-angle one and
    // there is nothing to get subtly wrong in them.
    let (x, y, z) = match axis {
        0 => ([1.0, 0.0, 0.0], [0.0, cos, sin], [0.0, -sin, cos]),
        1 => ([cos, 0.0, -sin], [0.0, 1.0, 0.0], [sin, 0.0, cos]),
        _ => ([cos, sin, 0.0], [-sin, cos, 0.0], [0.0, 0.0, 1.0]),
    };
    placed(body, x, y, z, about_origin(x, y, z, about))
}

/// Reflects a body in the plane across one of the world axes, through `about`.
///
/// The kernel puts the mirrored solid back the right way out; a reflection
/// left alone lights black.
pub fn mirrored(body: &Body, axis: usize, about: [f64; 3]) -> Option<Body> {
    let mut columns = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    columns[axis][axis] = -1.0;
    let [x, y, z] = columns;
    placed(body, x, y, z, about_origin(x, y, z, about))
}

/// Moves a body by a transform given as a column-major 4×4, which is what a
/// frame-to-frame solve produces.
pub fn by_matrix(body: &Body, matrix: [f64; 16]) -> Option<Body> {
    placed(
        body,
        [matrix[0], matrix[1], matrix[2]],
        [matrix[4], matrix[5], matrix[6]],
        [matrix[8], matrix[9], matrix[10]],
        [matrix[12], matrix[13], matrix[14]],
    )
}

/// Where a transform's origin has to sit for it to act about `about` rather
/// than about the world origin: `about − M·about`.
fn about_origin(x: [f64; 3], y: [f64; 3], z: [f64; 3], about: [f64; 3]) -> [f64; 3] {
    let mut origin = about;
    for axis in 0..3 {
        origin[axis] -= x[axis] * about[0] + y[axis] * about[1] + z[axis] * about[2];
    }
    origin
}

/// The box a body occupies, from its mesh.
pub fn extent(body: &Body) -> Option<([f64; 3], [f64; 3])> {
    let mesh = tessellation(body).mesh;
    if mesh.positions.is_empty() {
        return None;
    }
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in &mesh.positions {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    Some((low, high))
}

/// Where an axis-aligned plane cuts a body, as line segments.
///
/// Taken off the mesh rather than the surfaces: a section of a cone by a
/// slanted plane is a conic, of a torus a quartic, and the answer wanted here
/// is a set of Line entities either way. Each triangle the plane crosses
/// contributes the one segment where it does.
pub fn section(body: &Body, axis: usize, value: f64) -> Vec<([f64; 3], [f64; 3])> {
    let mesh = tessellation(body).mesh;
    let mut out = Vec::new();
    for triangle in &mesh.triangles {
        let corners: Vec<[f64; 3]> = triangle.iter().map(|i| mesh.positions[*i]).collect();
        // Where each edge of the triangle meets the plane. A triangle with a
        // corner exactly on it contributes that corner twice, which collapses
        // to nothing and is dropped below.
        let mut hits: Vec<[f64; 3]> = Vec::new();
        for step in 0..3 {
            let (from, to) = (corners[step], corners[(step + 1) % 3]);
            let (a, b) = (from[axis] - value, to[axis] - value);
            if (a > 0.0) == (b > 0.0) || a == b {
                continue;
            }
            let along = a / (a - b);
            hits.push([
                from[0] + (to[0] - from[0]) * along,
                from[1] + (to[1] - from[1]) * along,
                from[2] + (to[2] - from[2]) * along,
            ]);
        }
        if hits.len() == 2 {
            let span = (hits[0][0] - hits[1][0]).abs()
                + (hits[0][1] - hits[1][1]).abs()
                + (hits[0][2] - hits[1][2]).abs();
            if span > 1e-9 {
                out.push((hits[0], hits[1]));
            }
        }
    }
    out
}

// ── Edge extraction (pick geometry + wireframe overlay) ─────────────────────

/// Tessellate the solid's B-rep edges into acadrust `Wire`s. Stored on the
/// `Solid3D`/result entity for picking.
pub fn edge_wires(body: &Body) -> Vec<acadrust::entities::Wire> {
    use acadrust::types::Vector3;
    tessellation(body)
        .edges
        .iter()
        .map(|edge| {
            acadrust::entities::Wire::from_points(
                edge.positions
                    .iter()
                    .map(|p| Vector3::new(p[0], p[1], p[2]))
                    .collect(),
            )
        })
        .collect()
}

/// White wireframe used while a solid-history grip is hot.
///
/// This deliberately does not touch the resident solid mesh or its entity
/// wires: the selected source stays visible in blue while the candidate body
/// is presented as a separate, non-pickable outline until placement.
pub fn grip_preview_wires(body: &Body, handle: acadrust::Handle) -> Vec<WireModel> {
    tessellation(body)
        .edges
        .into_iter()
        .filter(|edge| edge.positions.len() >= 2)
        .map(|edge| {
            WireModel::solid_f64(
                format!("{}-GRIP-PREVIEW", handle.value()),
                edge.positions,
                WireModel::WHITE,
                false,
            )
        })
        .collect()
}

/// B-rep edge nearest a world-space surface pick.
pub fn nearest_edge(body: &Body, pick: [f64; 3]) -> Option<EdgeKey> {
    let pick = cadkernel::space::Vec3::from(pick);
    body.edge_keys()
        .filter_map(|key| {
            let edge = body.edges.get(key)?;
            let curve = body.curves.get(edge.curve)?;
            let nearest = if matches!(curve, Curve3::Line(_)) {
                let start = cadkernel::space::Vec3::from(curve.point_at(edge.start_parameter));
                let end = cadkernel::space::Vec3::from(curve.point_at(edge.end_parameter));
                let span = end - start;
                let length2 = span.dot(span);
                let along = if length2 > 0.0 {
                    (pick - start).dot(span) / length2
                } else {
                    0.0
                }
                .clamp(0.0, 1.0);
                pick.distance(start + span * along)
            } else {
                (0..=64)
                    .map(|step| {
                        let t = edge.start_parameter
                            + (edge.end_parameter - edge.start_parameter) * step as f64 / 64.0;
                        pick.distance(cadkernel::space::Vec3::from(curve.point_at(t)))
                    })
                    .fold(f64::INFINITY, f64::min)
            };
            Some((key, nearest))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(key, _)| key)
}

/// Planar face nearest a world-space surface pick.
pub fn nearest_planar_face(body: &Body, pick: [f64; 3]) -> Option<FaceKey> {
    let face = body
        .face_keys()
        .filter_map(|key| {
            let face = body.faces.get(key)?;
            let surface = body.surfaces.get(face.surface)?;
            Some((key, surface.distance_to(pick).abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(key, _)| key)?;
    let node = body.faces.get(face)?;
    matches!(body.surfaces.get(node.surface)?, Surface::Plane(_)).then_some(face)
}

/// Outward normal of a planar face.
pub fn planar_face_normal(body: &Body, face: FaceKey) -> Option<[f64; 3]> {
    let face = body.faces.get(face)?;
    let Surface::Plane(plane) = body.surfaces.get(face.surface)? else {
        return None;
    };
    let normal = cadkernel::space::Vec3::from(plane.normal()?);
    Some(if face.forward { normal } else { -normal }.to_array())
}

// ── Boolean operations ──────────────────────────────────────────────────────

/// Which CSG to apply. Mirrors `model::boolean_cmd::BoolOp` but kept local so
/// this scene module has no dependency on the UI module.
#[derive(Clone, Copy)]
pub enum Bool {
    Union,
    Subtract,
    Intersect,
}

/// Combine two solids. `Subtract` removes `b` from `a`.
///
/// `None` when the kernel refuses — a face pair it has no closed form for, a
/// cut it cannot make. It refuses rather than returning a solid with a wall
/// missing, and passing that on unchanged is the point: a half-done boolean
/// looks finished.
pub fn boolean(op: Bool, a: &Body, b: &Body) -> Option<Body> {
    let how = match op {
        Bool::Union => brep::Operation::Union,
        Bool::Subtract => brep::Operation::Difference,
        Bool::Intersect => brep::Operation::Intersection,
    };
    let tolerance = brep::operation_tolerance(&[a, b]);
    brep::combine(a.clone(), b.clone(), how, tolerance).ok()
}

// ── Tessellation ────────────────────────────────────────────────────────────

fn mesh_from_tessellation(
    tessellation: brep::mesh::BodyMesh,
    color: [f32; 4],
) -> Option<MeshLodSet> {
    let silhouette = tessellation.silhouette_source();
    let mesh = tessellation.mesh;
    if mesh.is_empty() {
        return None;
    }
    // The renderer holds each position as a coarse float plus a fine
    // correction, so a survey coordinate keeps its last millimetres instead
    // of losing them to f32.
    let mut verts = Vec::with_capacity(mesh.positions.len());
    let mut verts_low = Vec::with_capacity(mesh.positions.len());
    for point in &mesh.positions {
        let high = [point[0] as f32, point[1] as f32, point[2] as f32];
        verts.push(high);
        verts_low.push([
            (point[0] - high[0] as f64) as f32,
            (point[1] - high[1] as f64) as f32,
            (point[2] - high[2] as f64) as f32,
        ]);
    }
    let normals = mesh
        .normals
        .iter()
        .map(|n| [n[0] as f32, n[1] as f32, n[2] as f32])
        .collect();
    let indices = mesh
        .triangles
        .iter()
        .flat_map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
        .collect();
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
    for positions in tessellation
        .edges
        .into_iter()
        .map(|edge| edge.positions)
        .chain(tessellation.isolines.into_iter().map(|line| line.positions))
    {
        for segment in positions.windows(2) {
            for point in segment {
                let high = [point[0] as f32, point[1] as f32, point[2] as f32];
                set.edge_verts.push(high);
                set.edge_verts_low.push([
                    (point[0] - high[0] as f64) as f32,
                    (point[1] - high[1] as f64) as f32,
                    (point[2] - high[2] as f64) as f32,
                ]);
            }
        }
    }
    set.complete = tessellation.missing_faces.is_empty();
    set.curved_gens.push(super::mesh_model::CurvedGen { source: silhouette });
    Some(set)
}

pub fn display_from_solid(
    body: &Body,
    color: [f32; 4],
    facet_resolution: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
) -> Option<(MeshLodSet, Vec<acadrust::entities::Wire>, [f64; 3])> {
    use acadrust::types::Vector3;
    let tessellation = display_tessellation(
        body,
        facet_resolution,
        chordal_deflection,
        isolines,
    );
    let center = mesh_center(&tessellation.mesh)?;
    let wires = tessellation
        .edges
        .iter()
        .map(|edge| {
            acadrust::entities::Wire::from_points(
                edge.positions
                    .iter()
                    .map(|point| Vector3::new(point[0], point[1], point[2]))
                    .collect(),
            )
        })
        .collect();
    Some((mesh_from_tessellation(tessellation, color)?, wires, center))
}

/// The middle of a body, for a caller needing a point to turn or scale about.
///
/// Read off the mesh rather than `body_bounds`, which refuses a face that
/// wraps a closed surface — a sphere is one such face and has no box at all.
pub fn centre(body: &Body) -> Option<[f64; 3]> {
    mesh_center(&tessellation(body).mesh)
}

fn mesh_center(mesh: &brep::mesh::Mesh) -> Option<[f64; 3]> {
    if mesh.positions.is_empty() {
        return None;
    }
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in &mesh.positions {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    Some([
        (low[0] + high[0]) * 0.5,
        (low[1] + high[1]) * 0.5,
        (low[2] + high[2]) * 0.5,
    ])
}

/// How much a body encloses, from its mesh.
///
/// The divergence theorem over triangles wound outwards, which is what makes
/// it a check rather than only a measurement: a solid built inside out
/// reports a negative volume rather than a plausible one, and one missing a
/// face reports far too little. Nothing in the app measures volume yet, so it
/// exists to test with.
#[cfg(test)]
pub fn volume(body: &Body) -> f64 {
    use cadkernel::space::Vec3;
    let mesh = tessellation(body).mesh;
    let Some(middle) = centre(body) else {
        return 0.0;
    };
    // About the body's own middle: at survey coordinates the tetrahedra
    // reaching back to the origin are enormous and nearly cancel, and a
    // cubic millimetre read off a sum of billions is noise.
    let middle = Vec3::from(middle);
    mesh.triangles
        .iter()
        .map(|triangle| {
            let at = |index: usize| Vec3::from(mesh.positions[triangle[index]]) - middle;
            at(0).cross(at(1)).dot(at(2)) / 6.0
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri_count(body: &Body) -> usize {
        display_from_solid(body, [0.7, 0.7, 0.7, 1.0], 1.0, None, 0)
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
    fn every_primitive_is_the_size_it_was_asked_for() {
        // Triangle counts say a mesh exists; the volume says it is the right
        // shape and the right way out. A face left out reads far too small
        // and one wound inwards reads negative, and neither shows up in a
        // count.
        use std::f64::consts::PI;
        let c = [0.0, 0.0, 0.0];
        let cases: [(Body, f64); 5] = [
            (box_solid(c, 10.0, 4.0, 6.0).unwrap(), 240.0),
            (cylinder_solid(c, 5.0, 12.0).unwrap(), PI * 25.0 * 12.0),
            (
                cone_frustum_solid(c, 5.0, 5.0, 0.0, 12.0).unwrap(),
                PI * 25.0 * 12.0 / 3.0,
            ),
            (sphere_solid(c, 5.0).unwrap(), 4.0 / 3.0 * PI * 125.0),
            (torus_solid(c, 8.0, 2.0).unwrap(), 2.0 * PI * PI * 8.0 * 4.0),
        ];
        for (body, expected) in cases {
            let got = volume(&body);
            assert!(got > 0.0, "wound inwards: {got}");
            // Close either way, rather than short and never over. A chord does
            // lie inside the surface it spans, so a convex solid can only read
            // short — but a torus is not convex, and across the inside of its
            // tube the chords fall outside the material and add a little. What
            // is being checked is that the mesh is the shape asked for, and a
            // per cent covers both.
            assert!(
                (got - expected).abs() < 0.01 * expected,
                "{got} vs {expected}"
            );
        }
    }

    #[test]
    fn booleans_produce_solids() {
        let a = box_solid([0.0, 0.0, 0.0], 10.0, 10.0, 10.0).unwrap();
        let b = box_solid([5.0, 5.0, 5.0], 10.0, 10.0, 10.0).unwrap();
        for (op, label) in [
            (Bool::Union, "union"),
            (Bool::Subtract, "subtract"),
            (Bool::Intersect, "intersect"),
        ] {
            let r = boolean(op, &a, &b);
            let n = r.as_ref().map(tri_count).unwrap_or(0);
            assert!(r.is_some() && n > 0, "{label} produced nothing");
        }
    }

    #[test]
    fn box_exposes_edges() {
        assert!(edge_wires(&box_solid([0.0, 0.0, 0.0], 10.0, 10.0, 10.0).unwrap()).len() >= 12);
    }

    #[test]
    fn placing_a_body_moves_it_without_changing_its_size() {
        let body = box_solid([0.0, 0.0, 0.0], 10.0, 4.0, 6.0).unwrap();
        // A quarter turn about Z, then five along X.
        let moved = placed(
            &body,
            [0.0, 1.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [5.0, 0.0, 0.0],
        )
        .expect("a turned box");
        assert!((volume(&moved) - 240.0).abs() < 1e-6, "{}", volume(&moved));
        // Centred on the origin to begin with, so the turn leaves it there
        // and the move puts it five along x.
        let middle = centre(&moved).unwrap();
        assert!((middle[0] - 5.0).abs() < 1e-9, "{middle:?}");
        // And ten along x really did become ten along y.
        let (low, high) = extent(&moved).unwrap();
        assert!((high[1] - low[1] - 10.0).abs() < 1e-9, "{low:?} {high:?}");
        assert!((high[0] - low[0] - 4.0).abs() < 1e-9, "{low:?} {high:?}");
    }
}
