// B-rep construction for the Model tab's primitives, plus tessellation into
// the renderer's `MeshLodSet`.
//
// The bodies come from the geometry kernel, which builds each primitive the
// way ACIS records it: an analytic surface with singular vertices where the
// surface has them, rather than a mesh or a spline that happens to look like
// one. That is what lets a solid built here be written back out as exact
// geometry instead of a facetted approximation — see `acis_bridge`.
//
// Everything is oriented Z-up with the footprint on the z = base plane, to
// match acadrust's `acis::primitives`.
//
// The resulting `Body` is cached per entity handle on the Scene so the
// Design-group boolean tools can run on it.

use acadrust::kernel::brep::{self, Body};

use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};

/// How far a triangle may sit from the surface it lies on. A drawing is
/// measured in millimetres, and a twentieth of one is past what a screen
/// resolves at any sane zoom.
const SAG: f64 = 0.05;

/// What counts as the same point when the kernel checks a body over.
const TOL: f64 = 1e-9;

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

/// Solid cone standing on the z = base plane, apex `height` above it.
pub fn cone_solid(center: [f64; 3], radius: f64, height: f64) -> Option<Body> {
    brep::make::cone(center, radius, height)
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
    let mesh = brep::mesh::body(body, SAG, TOL);
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
    let mesh = brep::mesh::body(body, SAG, TOL);
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
/// `Solid3D`/result entity so it is click-pickable (the renderer's wire
/// fallback draws these as a wireframe over the shaded mesh, and hit-testing
/// uses their points).
pub fn edge_wires(body: &Body) -> Vec<acadrust::entities::Wire> {
    use acadrust::types::Vector3;
    brep::edge_polylines(body, SAG)
        .into_iter()
        .map(|points| {
            acadrust::entities::Wire::from_points(
                points
                    .into_iter()
                    .map(|p| Vector3::new(p[0], p[1], p[2]))
                    .collect(),
            )
        })
        .collect()
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
    brep::combine(a.clone(), b.clone(), how, TOL).ok()
}

// ── Tessellation ────────────────────────────────────────────────────────────

/// Tessellate a `Body` into a single-LOD `MeshLodSet` (world-space, before
/// world_offset is applied by the caller).
pub fn mesh_from_solid(body: &Body, color: [f32; 4]) -> Option<MeshLodSet> {
    let mesh = brep::mesh::body(body, SAG, TOL);
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
    Some(MeshLodSet::from_single(MeshModel {
        name: String::new(),
        verts,
        verts_low,
        normals,
        indices,
        triangle_material_handles: Vec::new(),
        triangle_colors: Vec::new(),
        color,
        selected: false,
    }))
}

/// The middle of a body, for a caller needing a point to turn or scale about.
///
/// Read off the mesh rather than `body_bounds`, which refuses a face that
/// wraps a closed surface — a sphere is one such face and has no box at all.
pub fn centre(body: &Body) -> Option<[f64; 3]> {
    let mesh = brep::mesh::body(body, SAG, TOL);
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
    use acadrust::kernel::space::Vec3;
    let mesh = brep::mesh::body(body, SAG, TOL);
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
        mesh_from_solid(body, [0.7, 0.7, 0.7, 1.0])
            .map(|m| m.lods[0].indices.len() / 3)
            .unwrap_or(0)
    }

    #[test]
    fn all_primitives_triangulate() {
        let c = [0.0, 0.0, 0.0];
        assert!(tri_count(&box_solid(c, 10.0, 10.0, 10.0).unwrap()) >= 12, "box");
        assert!(tri_count(&wedge_solid(c, 10.0, 10.0, 10.0).unwrap()) >= 6, "wedge");
        assert!(tri_count(&cylinder_solid(c, 5.0, 12.0).unwrap()) > 20, "cylinder");
        assert!(tri_count(&cone_solid(c, 5.0, 12.0).unwrap()) > 10, "cone");
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
            (cone_solid(c, 5.0, 12.0).unwrap(), PI * 25.0 * 12.0 / 3.0),
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
