// ACIS SAT → MeshModel tessellation for Solid3D (3DSOLID) entities.
//
// Strategy:
//   • plane-surface faces  → collect coedge-loop polygon, fan-triangulate.
//   • cone-surface faces   → sample a parametric grid (handles both cylinders
//                            and true cones).
//   • sphere-surface faces → sample a full UV grid.
//   • torus-surface faces  → sample a full UV grid.
//
// All other surface types are silently skipped; partial results are still
// returned so the solid renders with at least its planar faces.

use std::collections::HashSet;
use std::f64::consts::TAU;

use acadrust::entities::acis::types::Sense;
use acadrust::entities::acis::{
    SabReader, SatCoedge, SatConeSurface, SatDocument, SatEdge, SatFace, SatLoop, SatPlaneSurface,
    SatPoint, SatPointer, SatSphereSurface, SatTorusSurface, SatVertex,
};
use acadrust::entities::{Body, Region, Solid3D};

use crate::scene::mesh_model::{MeshLodSet, MeshModel};

/// Per-LOD sampling density. Higher values = finer mesh = more triangles.
#[derive(Copy, Clone, Debug)]
pub struct LodConfig {
    /// Arc segments per full circle for curved-surface sampling.
    pub circ_segs: usize,
    /// Longitudinal grid count for sphere / torus surfaces.
    pub grid_u: usize,
    /// Latitudinal grid count for sphere / torus surfaces.
    pub grid_v: usize,
}

impl LodConfig {
    /// LOD 0 — full resolution. The pre-Phase-3.4 baseline.
    pub const HIGH: LodConfig = LodConfig {
        circ_segs: 48,
        grid_u: 32,
        grid_v: 16,
    };
    /// LOD 1 — half-resolution. Use between ~50–200 px projected diagonal.
    pub const MID: LodConfig = LodConfig {
        circ_segs: 24,
        grid_u: 16,
        grid_v: 8,
    };
    /// LOD 2 — quarter-resolution. Use below ~50 px.
    pub const LOW: LodConfig = LodConfig {
        circ_segs: 12,
        grid_u: 8,
        grid_v: 4,
    };
    /// Returns the three LOD configs in `[high, mid, low]` order — matches
    /// the `MeshLodSet::lods` slot ordering.
    pub const fn all() -> [LodConfig; 3] {
        [Self::HIGH, Self::MID, Self::LOW]
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Tessellate a SAT document into mesh buffers — shared by all ACIS entities.
fn tessellate_sat(
    sat: &SatDocument,
    name: String,
    color: [f32; 4],
    lod: LodConfig,
) -> Option<MeshModel> {
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for face in sat.faces() {
        let surf_ptr = face.surface();
        let Some(surf_rec) = sat.resolve(surf_ptr) else {
            continue;
        };
        match surf_rec.entity_type.as_str() {
            "plane-surface" => {
                if let Some(plane) = SatPlaneSurface::from_record(surf_rec) {
                    tess_plane_face(sat, &face, &plane, &mut verts, &mut normals, &mut indices);
                }
            }
            "cone-surface" => {
                if let Some(cone) = SatConeSurface::from_record(surf_rec) {
                    tess_cone_face(sat, &face, &cone, lod, &mut verts, &mut normals, &mut indices);
                }
            }
            "sphere-surface" => {
                if let Some(sphere) = SatSphereSurface::from_record(surf_rec) {
                    tess_sphere_face(&sphere, lod, &mut verts, &mut normals, &mut indices);
                }
            }
            "torus-surface" => {
                if let Some(torus) = SatTorusSurface::from_record(surf_rec) {
                    tess_torus_face(&torus, lod, &mut verts, &mut normals, &mut indices);
                }
            }
            _ => {}
        }
    }
    if indices.is_empty() {
        return None;
    }
    Some(MeshModel {
        name,
        verts,
        normals,
        indices,
        color,
        selected: false,
    })
}

/// Tessellate a SAT document at all three LODs and bundle them into a
/// `MeshLodSet` ready for the render pipeline to pick a level per frame.
fn tessellate_sat_lods(
    sat: &SatDocument,
    name: String,
    color: [f32; 4],
) -> Option<MeshLodSet> {
    let configs = LodConfig::all();
    let mut lods: Vec<MeshModel> = Vec::with_capacity(3);
    for lod in configs {
        if let Some(m) = tessellate_sat(sat, name.clone(), color, lod) {
            lods.push(m);
        }
    }
    if lods.is_empty() {
        return None;
    }
    let world_aabb = mesh_aabb(&lods[0]);
    Some(MeshLodSet { lods, world_aabb })
}

/// World-XY AABB of the mesh — used by the render-pipeline LOD selector
/// to pick a level based on projected pixel diagonal.
fn mesh_aabb(mesh: &MeshModel) -> [f32; 4] {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &[x, y, _] in &mesh.verts {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        if x < min_x { min_x = x; }
        if y < min_y { min_y = y; }
        if x > max_x { max_x = x; }
        if y > max_y { max_y = y; }
    }
    [min_x, min_y, max_x, max_y]
}

fn parse_acis(
    sat_fn: impl FnOnce() -> Option<SatDocument>,
    is_binary: bool,
    sab_data: &[u8],
) -> Option<SatDocument> {
    if let Some(doc) = sat_fn() {
        return Some(doc);
    }
    if is_binary && !sab_data.is_empty() {
        return SabReader::read(sab_data).ok();
    }
    None
}

/// Tessellate a `Region` entity (2D planar ACIS body) at all three LOD levels.
pub fn tessellate_region(region: &Region, color: [f32; 4]) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || region.parse_sat(),
        region.acis_data.is_binary,
        &region.acis_data.sab_data,
    )?;
    let name = region.common.handle.value().to_string();
    tessellate_sat_lods(&sat, name, color)
}

/// Tessellate a `Body` entity (3D ACIS body) at all three LOD levels.
pub fn tessellate_body(body: &Body, color: [f32; 4]) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || body.parse_sat(),
        body.acis_data.is_binary,
        &body.acis_data.sab_data,
    )?;
    let name = body.common.handle.value().to_string();
    tessellate_sat_lods(&sat, name, color)
}

/// Tessellate a `Solid3D` entity at all three LOD levels.
///
/// Returns `None` when the entity has no parseable SAT data or produces no
/// triangles (e.g. the solid uses only unsupported surface types).
pub fn tessellate_solid3d(solid: &Solid3D, color: [f32; 4]) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || solid.parse_sat(),
        solid.acis_data.is_binary,
        &solid.acis_data.sab_data,
    )?;
    let name = solid.common.handle.value().to_string();
    tessellate_sat_lods(&sat, name, color)
}

// ── Topology helpers ──────────────────────────────────────────────────────────

/// Walk a face's outer coedge loop and collect ordered 3-D vertex positions.
///
/// Returns an empty `Vec` when the loop topology is broken or has fewer than
/// three distinct points.
fn collect_face_polygon(sat: &SatDocument, face: &SatFace) -> Vec<[f64; 3]> {
    let loop_ptr = face.first_loop();
    let Some(loop_rec) = sat.resolve(loop_ptr) else {
        return vec![];
    };
    let Some(sat_loop) = SatLoop::from_record(loop_rec) else {
        return vec![];
    };

    let first_ptr = sat_loop.first_coedge();
    let mut cur = first_ptr;
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut visited: HashSet<i32> = HashSet::new();

    loop {
        if cur.is_null() {
            break;
        }
        if visited.contains(&cur.0) {
            break;
        }
        visited.insert(cur.0);

        if let Some(ce_rec) = sat.resolve(cur) {
            if let Some(coedge) = SatCoedge::from_record(ce_rec) {
                // Pick the vertex that this coedge *starts from*, respecting sense.
                if let Some(edge_rec) = sat.resolve(coedge.edge()) {
                    if let Some(edge) = SatEdge::from_record(edge_rec) {
                        let v_ptr = if matches!(coedge.sense(), Sense::Forward) {
                            edge.start_vertex()
                        } else {
                            edge.end_vertex()
                        };

                        if let Some(pt) = resolve_point(sat, v_ptr) {
                            pts.push(pt);
                        }
                    }
                }

                let next = coedge.next();
                if next == first_ptr {
                    break;
                }
                cur = next;
                continue;
            }
        }
        break;
    }

    pts
}

/// Resolve a vertex pointer all the way to its `[x, y, z]` coordinate.
fn resolve_point(sat: &SatDocument, v_ptr: SatPointer) -> Option<[f64; 3]> {
    let v_rec = sat.resolve(v_ptr)?;
    let vertex = SatVertex::from_record(v_rec)?;
    let pt_rec = sat.resolve(vertex.point())?;
    let point = SatPoint::from_record(pt_rec)?;
    let (x, y, z) = point.position();
    Some([x, y, z])
}

// ── Mesh builder helpers ──────────────────────────────────────────────────────

/// Append one quad (two triangles) to the mesh buffers.
#[inline]
fn push_quad(
    verts: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    p: [[f64; 3]; 4],
    n: [f64; 3],
) {
    let base = verts.len() as u32;
    let nf = [n[0] as f32, n[1] as f32, n[2] as f32];
    for &pt in &p {
        verts.push([pt[0] as f32, pt[1] as f32, pt[2] as f32]);
        normals.push(nf);
    }
    // Two CCW triangles: (0,1,2) and (0,2,3)
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

// ── Planar face ───────────────────────────────────────────────────────────────

fn tess_plane_face(
    sat: &SatDocument,
    face: &SatFace,
    plane: &SatPlaneSurface,
    verts: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    let poly = collect_face_polygon(sat, face);
    if poly.len() < 3 {
        return;
    }

    let (nx, ny, nz) = plane.normal();
    // Flip normal outward if the face sense is reversed.
    let (nx, ny, nz) = if matches!(face.sense(), Sense::Reversed) {
        (-nx, -ny, -nz)
    } else {
        (nx, ny, nz)
    };
    let nf = [nx as f32, ny as f32, nz as f32];

    let base = verts.len() as u32;
    for &pt in &poly {
        verts.push([pt[0] as f32, pt[1] as f32, pt[2] as f32]);
        normals.push(nf);
    }

    // Fan triangulation from vertex 0.
    let n = poly.len() as u32;
    for i in 1..(n - 1) {
        indices.extend_from_slice(&[base, base + i, base + i + 1]);
    }
}

// ── Cone / cylinder face ──────────────────────────────────────────────────────

fn tess_cone_face(
    sat: &SatDocument,
    face: &SatFace,
    cone: &SatConeSurface,
    lod: LodConfig,
    verts: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    // Determine the height range and angular span from the boundary polygon.
    let poly = collect_face_polygon(sat, face);

    let (cx, cy, cz) = cone.center();
    let (ax, ay, az) = cone.axis(); // axis direction (unit)
    let (ux, uy, uz) = cone.major_axis(); // u=0 direction
    let radius = cone.radius();
    let sin_a = cone.sin_half_angle();
    let cos_a = cone.cos_half_angle(); // ≈1 for cylinder, <1 for cone

    // Build an orthonormal frame: axis_dir, u_dir, v_dir.
    let axis = norm3([ax, ay, az]);
    let u_dir = norm3([ux, uy, uz]);
    let v_dir = cross3(axis, u_dir);

    // Determine height and angle range from boundary vertices.
    let (h_min, h_max, theta_min, theta_max, full_circle) =
        angular_range(cx, cy, cz, axis, u_dir, v_dir, &poly);

    let segs_u = lod.circ_segs;
    let segs_v = (segs_u / 4).max(1); // height subdivisions

    let theta_span = if full_circle {
        TAU
    } else {
        theta_max - theta_min
    };
    let h_span = h_max - h_min;

    if h_span.abs() < 1e-10 || theta_span.abs() < 1e-10 {
        return;
    }

    for j in 0..segs_v {
        let t0 = h_min + h_span * (j as f64 / segs_v as f64);
        let t1 = h_min + h_span * ((j + 1) as f64 / segs_v as f64);

        for i in 0..segs_u {
            let a0 = theta_min + theta_span * (i as f64 / segs_u as f64);
            let a1 = theta_min + theta_span * ((i + 1) as f64 / segs_u as f64);

            // Cone radius at height t: r(t) = radius + t * sin_a / cos_a
            let r0 = if cos_a.abs() > 1e-9 {
                radius + t0 * sin_a / cos_a
            } else {
                radius
            };
            let r1 = if cos_a.abs() > 1e-9 {
                radius + t1 * sin_a / cos_a
            } else {
                radius
            };

            let p = [
                cone_pt(cx, cy, cz, axis, u_dir, v_dir, r0, a0, t0),
                cone_pt(cx, cy, cz, axis, u_dir, v_dir, r1, a0, t1),
                cone_pt(cx, cy, cz, axis, u_dir, v_dir, r1, a1, t1),
                cone_pt(cx, cy, cz, axis, u_dir, v_dir, r0, a1, t0),
            ];

            // Outward normal: perpendicular to axis in the radial direction,
            // tilted by the cone half-angle.
            let mid_a = (a0 + a1) * 0.5;
            let rad_dir = [
                u_dir[0] * mid_a.cos() + v_dir[0] * mid_a.sin(),
                u_dir[1] * mid_a.cos() + v_dir[1] * mid_a.sin(),
                u_dir[2] * mid_a.cos() + v_dir[2] * mid_a.sin(),
            ];
            let n = norm3([
                rad_dir[0] * cos_a - axis[0] * sin_a,
                rad_dir[1] * cos_a - axis[1] * sin_a,
                rad_dir[2] * cos_a - axis[2] * sin_a,
            ]);

            push_quad(verts, normals, indices, p, n);
        }
    }
}

/// Compute a point on a cone/cylinder surface.
#[inline]
fn cone_pt(
    cx: f64,
    cy: f64,
    cz: f64,
    axis: [f64; 3],
    u_dir: [f64; 3],
    v_dir: [f64; 3],
    r: f64,
    theta: f64,
    h: f64,
) -> [f64; 3] {
    [
        cx + r * (u_dir[0] * theta.cos() + v_dir[0] * theta.sin()) + h * axis[0],
        cy + r * (u_dir[1] * theta.cos() + v_dir[1] * theta.sin()) + h * axis[1],
        cz + r * (u_dir[2] * theta.cos() + v_dir[2] * theta.sin()) + h * axis[2],
    ]
}

/// Determine the height range and angular range of a curved face's boundary.
///
/// Returns `(h_min, h_max, theta_min, theta_max, full_circle)`.
/// `full_circle` is true when there are no boundary vertices (e.g. a sphere or
/// a cylinder with no seam edge).
fn angular_range(
    cx: f64,
    cy: f64,
    cz: f64,
    axis: [f64; 3],
    u_dir: [f64; 3],
    v_dir: [f64; 3],
    poly: &[[f64; 3]],
) -> (f64, f64, f64, f64, bool) {
    if poly.is_empty() {
        return (0.0, 0.0, 0.0, TAU, true);
    }

    let mut h_min = f64::MAX;
    let mut h_max = f64::MIN;
    let mut angles: Vec<f64> = Vec::new();

    for &pt in poly {
        let dx = pt[0] - cx;
        let dy = pt[1] - cy;
        let dz = pt[2] - cz;
        let h = dot3([dx, dy, dz], axis);
        h_min = h_min.min(h);
        h_max = h_max.max(h);
        let rv = dot3([dx, dy, dz], v_dir);
        // Project onto the plane perpendicular to the axis.
        let ru = dx * u_dir[0] + dy * u_dir[1] + dz * u_dir[2]
            - h * (axis[0] * u_dir[0] + axis[1] * u_dir[1] + axis[2] * u_dir[2]);
        angles.push(rv.atan2(ru));
    }

    // Normalise angles to a contiguous range.
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let theta_min = *angles.first().unwrap();
    let theta_max = *angles.last().unwrap();

    // If the angular span is almost 2π, treat as full circle.
    let full = (theta_max - theta_min) > TAU * 0.95;

    (h_min, h_max, theta_min, theta_max, full)
}

// ── Sphere face ───────────────────────────────────────────────────────────────

fn tess_sphere_face(
    sphere: &SatSphereSurface,
    lod: LodConfig,
    verts: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    let (cx, cy, cz) = sphere.center();
    let r = sphere.radius();
    let (px, py, pz) = sphere.pole(); // north-pole direction
    let pole = norm3([px, py, pz]);
    let (ux, uy, uz) = sphere.u_direction();
    let u_dir = norm3([ux, uy, uz]);
    let v_dir = cross3(pole, u_dir);

    let nu = lod.grid_u.max(3);
    let nv = lod.grid_v.max(2);

    for j in 0..nv {
        let phi0 = std::f64::consts::PI * (j as f64 / nv as f64); // 0..π
        let phi1 = std::f64::consts::PI * ((j + 1) as f64 / nv as f64);

        for i in 0..nu {
            let theta0 = TAU * (i as f64 / nu as f64);
            let theta1 = TAU * ((i + 1) as f64 / nu as f64);

            let n00 = sphere_dir(pole, u_dir, v_dir, theta0, phi0);
            let n10 = sphere_dir(pole, u_dir, v_dir, theta0, phi1);
            let n11 = sphere_dir(pole, u_dir, v_dir, theta1, phi1);
            let n01 = sphere_dir(pole, u_dir, v_dir, theta1, phi0);

            let p = [
                [cx + r * n00[0], cy + r * n00[1], cz + r * n00[2]],
                [cx + r * n10[0], cy + r * n10[1], cz + r * n10[2]],
                [cx + r * n11[0], cy + r * n11[1], cz + r * n11[2]],
                [cx + r * n01[0], cy + r * n01[1], cz + r * n01[2]],
            ];

            // Average outward normal for the quad.
            let nav = norm3([
                n00[0] + n10[0] + n11[0] + n01[0],
                n00[1] + n10[1] + n11[1] + n01[1],
                n00[2] + n10[2] + n11[2] + n01[2],
            ]);

            push_quad(verts, normals, indices, p, nav);
        }
    }
}

#[inline]
fn sphere_dir(pole: [f64; 3], u_dir: [f64; 3], v_dir: [f64; 3], theta: f64, phi: f64) -> [f64; 3] {
    let sin_phi = phi.sin();
    let cos_phi = phi.cos();
    let cos_theta = theta.cos();
    let sin_theta = theta.sin();
    // pole × cos_phi + (u*cos_theta + v*sin_theta) × sin_phi
    [
        pole[0] * cos_phi + (u_dir[0] * cos_theta + v_dir[0] * sin_theta) * sin_phi,
        pole[1] * cos_phi + (u_dir[1] * cos_theta + v_dir[1] * sin_theta) * sin_phi,
        pole[2] * cos_phi + (u_dir[2] * cos_theta + v_dir[2] * sin_theta) * sin_phi,
    ]
}

// ── Torus face ────────────────────────────────────────────────────────────────

fn tess_torus_face(
    torus: &SatTorusSurface,
    lod: LodConfig,
    verts: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    let (cx, cy, cz) = torus.center();
    let (nx, ny, nz) = torus.normal();
    let axis = norm3([nx, ny, nz]); // revolution axis
    let (ux, uy, uz) = torus.u_direction();
    let u_dir = norm3([ux, uy, uz]);
    let v_dir = cross3(axis, u_dir);
    let major_r = torus.major_radius();
    let minor_r = torus.minor_radius();

    let nu = lod.grid_u.max(3); // around the tube
    let nv = lod.grid_v.max(3); // around the torus

    for j in 0..nv {
        let phi0 = TAU * (j as f64 / nv as f64);
        let phi1 = TAU * ((j + 1) as f64 / nv as f64);

        for i in 0..nu {
            let theta0 = TAU * (i as f64 / nu as f64);
            let theta1 = TAU * ((i + 1) as f64 / nu as f64);

            let p = [
                torus_pt(
                    cx, cy, cz, axis, u_dir, v_dir, major_r, minor_r, theta0, phi0,
                ),
                torus_pt(
                    cx, cy, cz, axis, u_dir, v_dir, major_r, minor_r, theta0, phi1,
                ),
                torus_pt(
                    cx, cy, cz, axis, u_dir, v_dir, major_r, minor_r, theta1, phi1,
                ),
                torus_pt(
                    cx, cy, cz, axis, u_dir, v_dir, major_r, minor_r, theta1, phi0,
                ),
            ];

            // Outward tube normal.
            let mid_phi = (phi0 + phi1) * 0.5;
            let mid_theta = (theta0 + theta1) * 0.5;
            // Direction from tube center to surface point.
            let radial = [
                u_dir[0] * mid_phi.cos() + v_dir[0] * mid_phi.sin(),
                u_dir[1] * mid_phi.cos() + v_dir[1] * mid_phi.sin(),
                u_dir[2] * mid_phi.cos() + v_dir[2] * mid_phi.sin(),
            ];
            let n = norm3([
                radial[0] * mid_theta.cos() + axis[0] * mid_theta.sin(),
                radial[1] * mid_theta.cos() + axis[1] * mid_theta.sin(),
                radial[2] * mid_theta.cos() + axis[2] * mid_theta.sin(),
            ]);

            push_quad(verts, normals, indices, p, n);
        }
    }
}

#[inline]
fn torus_pt(
    cx: f64,
    cy: f64,
    cz: f64,
    axis: [f64; 3],
    u_dir: [f64; 3],
    v_dir: [f64; 3],
    major_r: f64,
    minor_r: f64,
    theta: f64, // tube angle
    phi: f64,   // revolution angle
) -> [f64; 3] {
    // Ring center at angle phi.
    let ring = [
        cx + major_r * (u_dir[0] * phi.cos() + v_dir[0] * phi.sin()),
        cy + major_r * (u_dir[1] * phi.cos() + v_dir[1] * phi.sin()),
        cz + major_r * (u_dir[2] * phi.cos() + v_dir[2] * phi.sin()),
    ];
    // Radial direction from torus axis to ring center.
    let radial = norm3([ring[0] - cx, ring[1] - cy, ring[2] - cz]);
    // Point on tube.
    [
        ring[0] + minor_r * (radial[0] * theta.cos() + axis[0] * theta.sin()),
        ring[1] + minor_r * (radial[1] * theta.cos() + axis[1] * theta.sin()),
        ring[2] + minor_r * (radial[2] * theta.cos() + axis[2] * theta.sin()),
    ]
}

// ── Math helpers ──────────────────────────────────────────────────────────────

#[inline]
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
fn norm3(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-12 {
        [0.0, 0.0, 1.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}
