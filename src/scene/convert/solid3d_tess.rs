// ACIS SAT → MeshModel tessellation for Solid3D (3DSOLID) entities.
//
// Strategy:
//   • plane-surface faces  → collect coedge-loop polygon, fan-triangulate.
//   • cone-surface faces   → sample a parametric grid (handles both cylinders
//                            and true cones).
//   • sphere-surface faces → sample a boundary-clipped UV grid.
//   • torus-surface faces  → sample a boundary-clipped UV grid.
//
// Coverage is tracked explicitly. Unsupported or malformed faces retain
// feature/display wires, and partial shells are marked non-complete so solid
// editing never mistakes them for closed topology.

use rustc_hash::FxHashSet as HashSet;
use rustc_hash::FxHashMap;
use std::f64::consts::TAU;

use acadrust::entities::acis::types::Sense;
use acadrust::entities::acis::{
    SabReader, SatCoedge, SatConeSurface, SatDocument, SatEdge, SatEllipseCurve, SatFace,
    SatIntCurve, SatLoop, SatPlaneSurface, SatPoint, SatPointer, SatSphereSurface,
    SatTorusSurface, SatVertex,
};
use acadrust::entities::{Body, Region, Solid3D};

use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};

// ── Curved-surface sampling — SINGLE TUNING POINT ────────────────────────────
//
// Every curved 3-D face (sphere / cone / torus / spline), curved feature edge
// and isoline samples at a density derived from a chord-height tolerance — the
// same model the 2-D circle/arc/ellipse wires use (see `tess_util::arc_segments`):
// the segment count tracks the arc's own radius and span at a bounded relative
// chord error, so a partial arc samples proportionally and facet error is
// size-independent. Every knob lives in this block — edit here to trade mesh
// density against the triangle budget across the whole solid tessellator.

/// Feature edges & isolines are built once at highest detail; this chord-height
/// fraction of the curve radius sets their sampling density (~0.002 ⇒ ~50
/// segments per full circle).
pub(crate) const EDGE_CHORD_FRAC: f64 = 0.002;
/// Truck's own triangulation chord tolerance for the cone faces still routed
/// through its kernel, as a fraction of the surface radius.
#[cfg(feature = "solid3d")]
pub(crate) const TRUCK_CHORD_FRAC: f64 = 0.1;
/// Boundary-loop sampling for parameter-range classification (which arc of a
/// sphere/torus a face covers): a fine fraction so the classification is
/// accurate; the points are not rendered.
pub(crate) const BOUNDARY_CHORD_FRAC: f64 = 0.002;

/// Per-LOD curved-surface sampling tolerance. A LOD is now just a chord-height
/// tolerance (fraction of the local radius); segment counts derive from it plus
/// the arc's own radius and span, so density is adaptive rather than a fixed
/// grid. Smaller fraction = finer mesh = more triangles.
#[derive(Copy, Clone, Debug)]
pub struct LodConfig {
    /// Chord-height tolerance as a fraction of the local radius.
    pub chord_frac: f64,
}

impl LodConfig {
    /// LOD 0 — full resolution (~0.5 % radius ⇒ ~32 segments per full circle,
    /// matching the pre-tolerance grid baseline).
    pub const HIGH: LodConfig = LodConfig { chord_frac: 0.005 };
    /// LOD 1 — half-resolution. Use between ~50–200 px projected diagonal.
    pub const MID: LodConfig = LodConfig { chord_frac: 0.02 };
    /// LOD 2 — quarter-resolution. Use below ~50 px.
    pub const LOW: LodConfig = LodConfig { chord_frac: 0.08 };
    /// Returns the three LOD configs in `[high, mid, low]` order — matches
    /// the `MeshLodSet::lods` slot ordering.
    pub const fn all() -> [LodConfig; 3] {
        [Self::HIGH, Self::MID, Self::LOW]
    }

    /// Segment count spanning `span_abs` radians of an arc of `radius` at this
    /// LOD's chord tolerance. Floor 2 — an open grid patch needs only a step.
    pub fn arc_segs(&self, radius: f64, span_abs: f64) -> usize {
        crate::scene::convert::tess_util::arc_segments_floored(
            radius.abs(),
            span_abs,
            radius.abs() * self.chord_frac,
            2,
        ) as usize
    }

    /// Segment count around a full circle of `radius` at this LOD. Floor 8 so a
    /// closed cross-section (a tube / minor circle) still reads as round.
    pub fn circle_segs(&self, radius: f64) -> usize {
        crate::scene::convert::tess_util::arc_segments_floored(
            radius.abs(),
            TAU,
            radius.abs() * self.chord_frac,
            8,
        ) as usize
    }
}

/// Segment count for a feature edge / isoline arc of `radius` spanning
/// `span_abs`, at the shared [`EDGE_CHORD_FRAC`] tolerance. Floor 4.
pub(crate) fn edge_arc_segs(radius: f64, span_abs: f64) -> usize {
    crate::scene::convert::tess_util::arc_segments_floored(
        radius.abs(),
        span_abs,
        radius.abs() * EDGE_CHORD_FRAC,
        4,
    ) as usize
}

/// Sample count for a curve with no analytic radius (a spline edge / surface):
/// the unit-circle segment count at chord fraction `frac`, used as a nominal
/// density that still tracks the LOD.
pub(crate) fn nominal_segs(frac: f64) -> usize {
    crate::scene::convert::tess_util::arc_segments_floored(1.0, TAU, frac, 8) as usize
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Tessellate a SAT document into mesh buffers — shared by all ACIS entities.
/// Vertices accumulate in f64 and `finalize_mesh` splits them into the
/// double-single pair with the body placement (`xform`) applied.
fn tessellate_sat(
    sat: &SatDocument,
    name: String,
    color: [f32; 4],
    lod: LodConfig,
    xform: Option<([f64; 9], [f64; 3], f64)>,
) -> Option<(MeshModel, bool)> {
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let face_materials: FxHashMap<i32, acadrust::Handle> = sat
        .records
        .iter()
        .filter(|record| record.entity_type == "material-adesk-attrib")
        .filter_map(|record| {
            let owner = record.token_pointer(2)?.0;
            let handle = record.token(3)?.as_integer()?;
            (owner >= 0 && handle > 0)
                .then(|| (owner, acadrust::Handle::new(handle as u64)))
        })
        .collect();
    let face_colors: FxHashMap<i32, [f32; 4]> = sat
        .records
        .iter()
        .filter(|record| record.entity_type == "color-adesk-attrib")
        .filter_map(|record| {
            let owner = record.token_pointer(2)?.0;
            let value = record.token(3)?.as_integer()?;
            let color_value = if (1..=255).contains(&value) {
                Some(acadrust::Color::from_index(value as i16))
            } else if value > 257 {
                Some(acadrust::Color::from_true_color_value(value as i32))
            } else {
                None
            }?;
            let mut rgba =
                crate::scene::convert::tess_util::aci_to_rgba(&color_value);
            rgba[3] = color[3];
            Some((owner, rgba))
        })
        .collect();
    let mut triangle_material_handles: Vec<Option<acadrust::Handle>> = Vec::new();
    let mut triangle_colors: Vec<Option<[f32; 4]>> = Vec::new();
    let face_record_indices: Vec<i32> = sat
        .records
        .iter()
        .filter(|record| record.is_a("face"))
        .map(|record| record.index)
        .collect();
    let mut complete = true;

    for (face, face_record_index) in sat.faces().into_iter().zip(face_record_indices) {
        let surf_ptr = face.surface();
        let Some(surf_rec) = sat.resolve(surf_ptr) else {
            complete = false;
            continue;
        };
        let before = indices.len();
        match surf_rec.entity_type.as_str() {
            "plane-surface" => {
                if let Some(plane) = SatPlaneSurface::from_record(surf_rec) {
                    tess_plane_face(
                        sat,
                        &face,
                        &plane,
                        lod.chord_frac,
                        &mut verts,
                        &mut normals,
                        &mut indices,
                    );
                }
            }
            "cone-surface" => {
                if let Some(cone) = SatConeSurface::from_record(surf_rec) {
                    tess_cone_face(
                        sat,
                        &face,
                        &cone,
                        lod,
                        &mut verts,
                        &mut normals,
                        &mut indices,
                    );
                }
            }
            "sphere-surface" => {
                if let Some(sphere) = SatSphereSurface::from_record(surf_rec) {
                    tess_sphere_face(
                        sat,
                        &face,
                        &sphere,
                        lod,
                        &mut verts,
                        &mut normals,
                        &mut indices,
                    );
                }
            }
            "torus-surface" => {
                if let Some(torus) = SatTorusSurface::from_record(surf_rec) {
                    tess_torus_face(
                        sat,
                        &face,
                        &torus,
                        lod,
                        &mut verts,
                        &mut normals,
                        &mut indices,
                    );
                }
            }
            "spline-surface" | "meshsurf-surface" | "bs3-surface" => {
                crate::scene::convert::spline_tess::tess_spline_face(
                    sat,
                    &face,
                    lod,
                    &mut verts,
                    &mut normals,
                    &mut indices,
                );
            }
            _ => {}
        }
        if indices.len() == before {
            complete = false;
        } else {
            let material = face_materials.get(&face_record_index).copied();
            triangle_material_handles.extend(
                std::iter::repeat(material).take((indices.len() - before) / 3),
            );
            let face_color = face_colors.get(&face_record_index).copied();
            triangle_colors.extend(
                std::iter::repeat(face_color).take((indices.len() - before) / 3),
            );
        }
    }
    if indices.is_empty() {
        return None;
    }
    Some((
        finalize_mesh(
            name,
            verts,
            normals,
            indices,
            triangle_material_handles,
            triangle_colors,
            color,
            xform,
        ),
        complete,
    ))
}

/// Tessellate an ACIS document, preferring the truck B-rep kernel and falling
/// back to the bespoke per-surface sampler when truck can't rebuild the shell
/// (e.g. an unhandled surface type).
fn tessellate_acis(
    sat: &SatDocument,
    name: String,
    color: [f32; 4],
    facet_res: f64,
    isolines: usize,
) -> Option<MeshLodSet> {
    let truck = crate::scene::convert::acis_to_truck::tessellate_sat_truck(
        sat,
        name.clone(),
        color,
        facet_res,
    );
    let manual = if truck.as_ref().is_some_and(|set| set.complete) {
        None
    } else {
        tessellate_sat_lods(sat, name.clone(), color, facet_res)
    };
    let mut set = match (truck, manual) {
        (Some(set), _) if set.complete => set,
        (_, Some(set)) if set.complete => set,
        (Some(truck), Some(manual)) => {
            let truck_tris = truck
                .lods
                .first()
                .map(|mesh| mesh.indices.len())
                .unwrap_or(0);
            let manual_tris = manual
                .lods
                .first()
                .map(|mesh| mesh.indices.len())
                .unwrap_or(0);
            if manual_tris > truck_tris {
                manual
            } else {
                truck
            }
        }
        (Some(set), None) | (None, Some(set)) => set,
        (None, None) => return None,
    };
    if set.complete {
        if std::env::var_os("OCS_TESS_DEBUG").is_some() {
            let tris = set.lods.first().map(|m| m.indices.len() / 3).unwrap_or(0);
            eprintln!("acis_tess[{name}]: complete ({tris} tris)");
        }
    } else if std::env::var_os("OCS_TESS_DEBUG").is_some() {
        let tris = set.lods.first().map(|m| m.indices.len() / 3).unwrap_or(0);
        eprintln!(
            "acis_tess[{name}]: partial ({tris} tris); feature/display wires retained"
        );
    }
    // Attach the B-rep face-boundary edges plus ISOLINES on curved faces
    // (body-transformed, split into the double-single pair) so the solid's
    // wireframe shows real edges and curved faces read from any angle.
    attach_feature_edges(&mut set, sat, isolines);
    Some(set)
}

/// Collect the ACIS `edge` records as world-space polyline segments (pairs of
/// endpoints), body-transform them, and store them on the set as the
/// double-single `edge_verts` / `edge_verts_low`.
fn attach_feature_edges(set: &mut MeshLodSet, sat: &SatDocument, isolines: usize) {
    let xform = body_transform(sat);
    // World-space curved-face generators for the per-frame silhouette pass.
    set.curved_gens = collect_curved_gens(sat, xform);
    let mut seg_pts = collect_feature_edges(sat);
    // ISOLINES ride the same line list and the same body transform as the
    // feature edges, so they inherit the offset / per-INSTANCE re-split for free.
    seg_pts.extend(collect_isolines(sat, isolines));
    set.edge_verts.reserve(seg_pts.len());
    set.edge_verts_low.reserve(seg_pts.len());
    for p in seg_pts {
        let (mut x, mut y, mut z) = (p[0], p[1], p[2]);
        if let Some((m, tr, scale)) = xform {
            let (lx, ly, lz) = (x, y, z);
            x = scale * (lx * m[0] + ly * m[3] + lz * m[6]) + tr[0];
            y = scale * (lx * m[1] + ly * m[4] + lz * m[7]) + tr[1];
            z = scale * (lx * m[2] + ly * m[5] + lz * m[8]) + tr[2];
        }
        let (hx, hy, hz) = (x as f32, y as f32, z as f32);
        set.edge_verts.push([hx, hy, hz]);
        set.edge_verts_low.push([
            (x - hx as f64) as f32,
            (y - hy as f64) as f32,
            (z - hz as f64) as f32,
        ]);
    }
}

/// ISOLINES line-list endpoints (pairs) for the curved faces of the solid:
/// `count` longitudinal lines spaced across each cone/cylinder face, from its
/// bottom rim to its top rim. These are view-independent tessellation lines
/// (AutoCAD's ISOLINES), so a cylinder reads as a cylinder from any angle
/// rather than showing only its two rim circles. Points are body-local
/// (pre-transform), matching [`collect_feature_edges`], so the caller applies
/// the body transform uniformly.
/// Body-local geometry of one cone/cylinder face: its frame, radius, cone taper
/// and the height/angular extent recovered the same way `tess_cone_face` does.
/// Shared by the ISOLINES and silhouette-generator collectors.
struct ConeFaceGeom {
    center: [f64; 3],
    axis: [f64; 3],
    u_dir: [f64; 3],
    v_dir: [f64; 3],
    radius: f64,
    tan_a: f64,
    h_min: f64,
    h_max: f64,
    theta_min: f64,
    theta_span: f64,
    full: bool,
}

fn cone_face_geom(sat: &SatDocument, face: &SatFace) -> Option<ConeFaceGeom> {
    let surf_rec = sat.resolve(face.surface())?;
    if surf_rec.entity_type != "cone-surface" {
        return None;
    }
    let cone = SatConeSurface::from_record(surf_rec)?;
    let (cx, cy, cz) = cone.center();
    let (ax, ay, az) = cone.axis();
    let (ux, uy, uz) = cone.major_axis();
    let radius = cone.radius();
    let sin_a = cone.sin_half_angle();
    let cos_a = cone.cos_half_angle();
    let axis = norm3([ax, ay, az]);
    let u_dir = norm3([ux, uy, uz]);
    let v_dir = cross3(axis, u_dir);

    let poly = collect_face_polygon(sat, face, BOUNDARY_CHORD_FRAC);
    let (mut h_min, mut h_max, mut theta_min, mut theta_max, full) =
        angular_range(cx, cy, cz, axis, u_dir, v_dir, &poly);
    if (h_max - h_min).abs() < 1e-9 {
        if let Some((vmin, vmax)) = cone_axis_span(sat, face, &cone, axis, [cx, cy, cz]) {
            h_min = vmin;
            h_max = vmax;
            if full {
                theta_min = 0.0;
                theta_max = TAU;
            }
        }
    }
    let theta_span = if full { TAU } else { theta_max - theta_min };
    if (h_max - h_min).abs() < 1e-10 || theta_span.abs() < 1e-10 {
        return None;
    }
    let tan_a = if cos_a.abs() > 1e-9 {
        sin_a / cos_a
    } else {
        0.0
    };
    Some(ConeFaceGeom {
        center: [cx, cy, cz],
        axis,
        u_dir,
        v_dir,
        radius,
        tan_a,
        h_min,
        h_max,
        theta_min,
        theta_span,
        full,
    })
}

/// Pick `count` parameter values across `[t_min, t_min + span]`. A closed
/// revolution (`full`) is divided into `count` values around the full turn (the
/// line at `t` and `t + span` coincide); a bounded arc gets `count` interior
/// values, its two ends already drawn as rim edges.
fn iso_params(t_min: f64, span: f64, full: bool, count: usize) -> Vec<f64> {
    (0..count)
        .map(|k| {
            if full {
                t_min + span * (k as f64 / count as f64)
            } else {
                t_min + span * ((k as f64 + 1.0) / (count as f64 + 1.0))
            }
        })
        .collect()
}

fn collect_isolines(sat: &SatDocument, count: usize) -> Vec<[f64; 3]> {
    if count == 0 {
        return Vec::new();
    }
    let mut out: Vec<[f64; 3]> = Vec::new();
    for face in sat.faces() {
        let Some(surf) = sat.resolve(face.surface()) else {
            continue;
        };
        match surf.entity_type.as_str() {
            "cone-surface" => cone_isolines(sat, &face, count, &mut out),
            "sphere-surface" => sphere_isolines(sat, &face, count, &mut out),
            "torus-surface" => torus_isolines(sat, &face, count, &mut out),
            _ => {}
        }
    }
    out
}

/// Longitudinal lines up a cone/cylinder face, bottom rim to top rim.
fn cone_isolines(sat: &SatDocument, face: &SatFace, count: usize, out: &mut Vec<[f64; 3]>) {
    let Some(g) = cone_face_geom(sat, face) else {
        return;
    };
    let [cx, cy, cz] = g.center;
    let (r0, r1) = (g.radius + g.h_min * g.tan_a, g.radius + g.h_max * g.tan_a);
    for a in iso_params(g.theta_min, g.theta_span, g.full, count) {
        out.push(cone_pt(
            cx, cy, cz, g.axis, g.u_dir, g.v_dir, r0, a, g.h_min,
        ));
        out.push(cone_pt(
            cx, cy, cz, g.axis, g.u_dir, g.v_dir, r1, a, g.h_max,
        ));
    }
}

/// Meridian lines on a sphere face — the standard "how a sphere reads" isolines,
/// each running pole-ward across the face's colatitude span at `count` evenly
/// spaced longitudes within the face's own longitude span.
fn sphere_isolines(sat: &SatDocument, face: &SatFace, count: usize, out: &mut Vec<[f64; 3]>) {
    let Some(surf) = sat.resolve(face.surface()) else {
        return;
    };
    let Some(sphere) = SatSphereSurface::from_record(surf) else {
        return;
    };
    let (cx, cy, cz) = sphere.center();
    let r = sphere.radius();
    let pole = norm3([sphere.pole().0, sphere.pole().1, sphere.pole().2]);
    let u = norm3([
        sphere.u_direction().0,
        sphere.u_direction().1,
        sphere.u_direction().2,
    ]);
    let v = cross3(pole, u);
    let poly = collect_face_polygon(sat, face, BOUNDARY_CHORD_FRAC);
    let (theta_min, theta_span, full, phi_min, phi_max) =
        sphere_param_range(&poly, [cx, cy, cz], pole, u, v);
    // Meridian subdivisions from the sphere radius and colatitude span (a great
    // circle of radius `r`) at the shared edge chord tolerance.
    let m = edge_arc_segs(r, phi_max - phi_min);
    let sphere_pt = |theta: f64, phi: f64| {
        let d = sphere_dir(pole, u, v, theta, phi);
        [cx + r * d[0], cy + r * d[1], cz + r * d[2]]
    };
    for theta in iso_params(theta_min, theta_span, full, count) {
        for k in 0..m {
            let p0 = phi_min + (phi_max - phi_min) * (k as f64 / m as f64);
            let p1 = phi_min + (phi_max - phi_min) * ((k + 1) as f64 / m as f64);
            out.push(sphere_pt(theta, p0));
            out.push(sphere_pt(theta, p1));
        }
    }
}

/// Minor (cross-section) circles on a torus face at `count` revolution angles
/// spanning the face — how a torus tube reads.
fn torus_isolines(sat: &SatDocument, face: &SatFace, count: usize, out: &mut Vec<[f64; 3]>) {
    let Some(surf) = sat.resolve(face.surface()) else {
        return;
    };
    let Some(torus) = SatTorusSurface::from_record(surf) else {
        return;
    };
    let (cx, cy, cz) = torus.center();
    let axis = norm3([torus.normal().0, torus.normal().1, torus.normal().2]);
    let u = norm3([
        torus.u_direction().0,
        torus.u_direction().1,
        torus.u_direction().2,
    ]);
    let v = cross3(axis, u);
    let major = torus.major_radius();
    let minor = torus.minor_radius();
    let (phi_min, phi_span, full) =
        torus_phi_range(sat, face, [cx, cy, cz], u, v, major);
    let phi_total = if full { TAU } else { phi_span };
    let (theta_min, theta_span, theta_full) =
        torus_theta_range(sat, face, [cx, cy, cz], axis, major, minor);
    let theta_total = if theta_full { TAU } else { theta_span };

    // Minor (cross-section) arcs — constant revolution angle, clipped to the
    // face's tube-angle window.
    // Segment count from the tube (minor) radius at the shared edge tolerance.
    let m = edge_arc_segs(minor, theta_total);
    for phi in iso_params(phi_min, phi_span, full, count) {
        for t in 0..m {
            let t0 = theta_min + theta_total * (t as f64 / m as f64);
            let t1 = theta_min + theta_total * ((t + 1) as f64 / m as f64);
            out.push(torus_pt(cx, cy, cz, axis, u, v, major, minor, t0, phi));
            out.push(torus_pt(cx, cy, cz, axis, u, v, major, minor, t1, phi));
        }
    }

    // Major (ring-direction) arcs — constant tube angle, swept along the face's
    // revolution arc. The outer (θ=0) and inner (θ=π) circles are the torus's
    // defining profile — the ring outline; without them it reads as disconnected
    // cross-sections. `count.max(2)` guarantees outer + inner even at ISOLINES=1.
    let ring_segs = edge_arc_segs(major, phi_total).max(2);
    let n_ring = count.max(2);
    for theta in iso_params(theta_min, theta_span, theta_full, n_ring) {
        for s in 0..ring_segs {
            let p0 = phi_min + phi_total * (s as f64 / ring_segs as f64);
            let p1 = phi_min + phi_total * ((s + 1) as f64 / ring_segs as f64);
            out.push(torus_pt(cx, cy, cz, axis, u, v, major, minor, theta, p0));
            out.push(torus_pt(cx, cy, cz, axis, u, v, major, minor, theta, p1));
        }
    }
}

/// Longitude/colatitude span of a sphere face from its boundary polygon.
/// Returns `(theta_min, theta_span, full, phi_min, phi_max)`; an empty boundary
/// (a lone full sphere) spans the whole surface.
fn sphere_param_range(
    poly: &[[f64; 3]],
    center: [f64; 3],
    pole: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
) -> (f64, f64, bool, f64, f64) {
    use std::f64::consts::PI;
    if poly.len() < 2 {
        return (0.0, TAU, true, 0.0, PI);
    }
    let mut thetas: Vec<f64> = Vec::new();
    let (mut phi_min, mut phi_max) = (f64::MAX, f64::MIN);
    for &p in poly {
        let d = norm3([p[0] - center[0], p[1] - center[1], p[2] - center[2]]);
        let cphi = dot3(d, pole).clamp(-1.0, 1.0);
        let phi = cphi.acos();
        phi_min = phi_min.min(phi);
        phi_max = phi_max.max(phi);
        thetas.push(dot3(d, v).atan2(dot3(d, u)));
    }
    let (theta_min, theta_span, full) = angular_span(&thetas);
    // Meridians converge at the poles, so pad the colatitude a touch toward each
    // pole the face reaches so the lines meet the rim rather than stopping short.
    (
        theta_min,
        theta_span,
        full,
        (phi_min - 0.05).max(0.0),
        (phi_max + 0.05).min(PI),
    )
}

/// Revolution-angle arc a torus face spans, walking all its boundary loops.
///
/// A partial tube ends in two oriented minor-circle boundary loops sitting in
/// constant-φ planes. Each loop's coedge and edge senses, combined with the
/// face sense, identify the side that belongs to the trimmed face. That makes
/// both short elbows and long open tubes follow their recorded boundary rather
/// than choosing an angular span by size. If that trim topology is absent or
/// ambiguous, the face remains untrimmed. Returns
/// `(body_start, body_span, full)`.
pub(crate) fn torus_phi_range(
    sat: &SatDocument,
    face: &SatFace,
    center: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    major: f64,
) -> (f64, f64, bool) {
    const GEOM_EPS: f64 = 1e-6;
    let axis = norm3(cross3(u, v));
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut lp = face.first_loop();
    let mut seen: HashSet<i32> = HashSet::default();
    while !lp.is_null() && seen.insert(lp.0) {
        let Some(lr) = sat.resolve(lp) else { break };
        let Some(sl) = SatLoop::from_record(lr) else {
            break;
        };
        lp = sl.next_loop();
        let Some((boundary_center, inward_direction)) =
            circle_loop_inward_direction(sat, face, &sl)
        else {
            continue;
        };
        let rel = [
            boundary_center[0] - center[0],
            boundary_center[1] - center[1],
            boundary_center[2] - center[2],
        ];
        let axial = dot3(rel, axis);
        let radial_u = dot3(rel, u);
        let radial_v = dot3(rel, v);
        let radial_len = (radial_u * radial_u + radial_v * radial_v).sqrt();
        let scale = major.abs().max(radial_len).max(1.0);
        if axial.abs() > scale * GEOM_EPS
            || (radial_len - major.abs()).abs() > scale * GEOM_EPS
        {
            continue;
        }
        let physical_phi = radial_v.atan2(radial_u);
        let phi = if major < 0.0 {
            (physical_phi - std::f64::consts::PI).rem_euclid(TAU)
        } else {
            physical_phi.rem_euclid(TAU)
        };
        let major_sign = if major < 0.0 { -1.0 } else { 1.0 };
        let tangent = norm3([
            major_sign * (-u[0] * phi.sin() + v[0] * phi.cos()),
            major_sign * (-u[1] * phi.sin() + v[1] * phi.cos()),
            major_sign * (-u[2] * phi.sin() + v[2] * phi.cos()),
        ]);
        let alignment = dot3(tangent, inward_direction);
        if (alignment.abs() - 1.0).abs() > GEOM_EPS {
            continue;
        }
        if alignment > 0.0 {
            starts.push(phi);
        } else {
            ends.push(phi);
        }
    }

    starts.sort_by(|left, right| left.partial_cmp(right).unwrap());
    ends.sort_by(|left, right| left.partial_cmp(right).unwrap());
    starts.dedup_by(|left, right| (*left - *right).abs() < GEOM_EPS);
    ends.dedup_by(|left, right| (*left - *right).abs() < GEOM_EPS);
    if let ([start], [end]) = (starts.as_slice(), ends.as_slice()) {
        let span = (end - start).rem_euclid(TAU);
        if span > GEOM_EPS && span < TAU - GEOM_EPS {
            return (*start, span, false);
        }
    }
    (0.0, TAU, true)
}

/// Analytic circle data carried by an ellipse edge in a boundary loop.
fn loop_circle_geometry(
    sat: &SatDocument,
    sat_loop: &SatLoop,
) -> Option<([f64; 3], [f64; 3], f64)> {
    let first = sat_loop.first_coedge();
    let mut current = first;
    let mut seen: HashSet<i32> = HashSet::default();
    while !current.is_null() && seen.insert(current.0) {
        let coedge = SatCoedge::from_record(sat.resolve(current)?)?;
        if let Some(ellipse) = sat
            .resolve(coedge.edge())
            .and_then(SatEdge::from_record)
            .and_then(|edge| sat.resolve(edge.curve()))
            .and_then(SatEllipseCurve::from_record)
        {
            if (ellipse.ratio() - 1.0).abs() <= 1e-6 {
                let (cx, cy, cz) = ellipse.center();
                let (nx, ny, nz) = ellipse.normal();
                let (mx, my, mz) = ellipse.major_axis();
                let radius = (mx * mx + my * my + mz * mz).sqrt();
                return Some(([cx, cy, cz], norm3([nx, ny, nz]), radius));
            }
        }
        current = coedge.next();
        if current == first {
            break;
        }
    }
    None
}

/// Circle centre and the in-surface direction lying inside an oriented loop.
/// The direction comes directly from the boundary curve winding and face sense.
fn circle_loop_inward_direction(
    sat: &SatDocument,
    face: &SatFace,
    sat_loop: &SatLoop,
) -> Option<([f64; 3], [f64; 3])> {
    const GEOM_EPS: f64 = 1e-6;
    let first = sat_loop.first_coedge();
    let mut current = first;
    let mut seen: HashSet<i32> = HashSet::default();
    while !current.is_null() && seen.insert(current.0) {
        let coedge = SatCoedge::from_record(sat.resolve(current)?)?;
        let Some(edge) = sat.resolve(coedge.edge()).and_then(SatEdge::from_record) else {
            break;
        };
        let Some(ellipse) = sat
            .resolve(edge.curve())
            .and_then(SatEllipseCurve::from_record)
        else {
            current = coedge.next();
            if current == first {
                break;
            }
            continue;
        };
        if (ellipse.ratio() - 1.0).abs() > GEOM_EPS {
            current = coedge.next();
            if current == first {
                break;
            }
            continue;
        }

        let center = ellipse.center();
        let ellipse_frame = ellipse_frame(&ellipse, matches!(edge.sense(), Sense::Reversed))?;
        let coedge_forward = matches!(coedge.sense(), Sense::Forward);
        let parameter = if coedge_forward {
            edge.start_param()
        } else {
            edge.end_param()
        };
        let edge_span = edge.end_param() - edge.start_param();
        let edge_direction = if edge_span < -GEOM_EPS { -1.0 } else { 1.0 };
        let traversal_direction = if coedge_forward {
            edge_direction
        } else {
            -edge_direction
        };
        let (point, curve_tangent) = ellipse_frame.point_tangent(parameter);
        let boundary_tangent = norm3([
            traversal_direction * curve_tangent[0],
            traversal_direction * curve_tangent[1],
            traversal_direction * curve_tangent[2],
        ]);
        let face_sign = if matches!(face.sense(), Sense::Reversed) {
            -1.0
        } else {
            1.0
        };
        let face_normal = norm3([
            face_sign * (point[0] - center.0),
            face_sign * (point[1] - center.1),
            face_sign * (point[2] - center.2),
        ]);
        let inward = norm3(cross3(face_normal, boundary_tangent));
        return Some(([center.0, center.1, center.2], inward));
    }
    None
}

/// Tube-angle span of a torus face from its boundary loops.
///
/// Ring tori normally cover the complete minor circle, while a spindle-torus
/// cap terminates at the surface's axial singularity and owns just one
/// constant-θ boundary ring. Sampling the complete minor circle in that case
/// duplicates the cap across the rest of the self-intersecting analytic
/// surface, producing a ball much larger than the bounded face.
fn torus_theta_range(
    sat: &SatDocument,
    face: &SatFace,
    center: [f64; 3],
    axis: [f64; 3],
    major: f64,
    minor: f64,
) -> (f64, f64, bool) {
    const GEOM_EPS: f64 = 1e-6;

    if minor <= 1e-9 || major.abs() >= minor {
        return (0.0, TAU, true);
    }

    let mut rims = Vec::new();
    let mut lp = face.first_loop();
    let mut seen: HashSet<i32> = HashSet::default();
    while !lp.is_null() && seen.insert(lp.0) {
        let Some(sat_loop) = sat.resolve(lp).and_then(SatLoop::from_record) else {
            break;
        };
        lp = sat_loop.next_loop();
        let Some((circle_center, circle_normal, circle_radius)) =
            loop_circle_geometry(sat, &sat_loop)
        else {
            continue;
        };
        let rel = [
            circle_center[0] - center[0],
            circle_center[1] - center[1],
            circle_center[2] - center[2],
        ];
        let axial = dot3(rel, axis);
        let planar = [
            rel[0] - axial * axis[0],
            rel[1] - axial * axis[1],
            rel[2] - axial * axis[2],
        ];
        let scale = major.abs().max(minor).max(circle_radius).max(1.0);
        if dot3(planar, planar).sqrt() > scale * GEOM_EPS
            || (dot3(circle_normal, axis).abs() - 1.0).abs() > GEOM_EPS
        {
            continue;
        }

        // The principal spindle branch has non-negative revolution radius:
        // circle_radius = major + minor·cos(θ). Together with the circle
        // plane's axial coordinate this recovers θ directly.
        let sin_theta = axial / minor;
        let cos_theta = (circle_radius - major) / minor;
        if (sin_theta * sin_theta + cos_theta * cos_theta - 1.0).abs() > GEOM_EPS {
            continue;
        }
        rims.push(sin_theta.atan2(cos_theta));
    }

    rims.sort_by(|left, right| left.partial_cmp(right).unwrap());
    rims.dedup_by(|left, right| (*left - *right).abs() < GEOM_EPS);
    if rims.len() != 1 || rims[0].abs() < GEOM_EPS {
        return (0.0, TAU, true);
    }

    let rim = rims[0];
    let singular = (-major / minor).clamp(-1.0, 1.0).acos();
    if rim > 0.0 && rim < singular {
        (rim, singular - rim, false)
    } else if rim < 0.0 && rim > -singular {
        (-singular, singular + rim, false)
    } else {
        (0.0, TAU, true)
    }
}

/// Reduce a set of angles to a `(min, span, full)` arc. Mirrors `angular_range`'s
/// gap detection: the largest gap between sorted angles is the arc's *outside*,
/// so the arc runs from the gap's end round to its start; a small largest gap
/// means the angles wrap the whole circle.
fn angular_span(angles: &[f64]) -> (f64, f64, bool) {
    if angles.is_empty() {
        return (0.0, TAU, true);
    }
    let mut a: Vec<f64> = angles.iter().map(|x| x.rem_euclid(TAU)).collect();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let mut gap_max = 0.0;
    let mut gap_at = 0usize;
    for i in 0..a.len() {
        let next = if i + 1 < a.len() {
            a[i + 1]
        } else {
            a[0] + TAU
        };
        let gap = next - a[i];
        if gap > gap_max {
            gap_max = gap;
            gap_at = i;
        }
    }
    if gap_max < TAU / 12.0 {
        return (0.0, TAU, true); // wraps the full circle
    }
    let start = a[(gap_at + 1) % a.len()];
    (start, TAU - gap_max, false)
}

/// World-space silhouette generators for each cone/cylinder face — the params a
/// per-frame DISPSILH pass needs. `xform` is the solid's body transform (or
/// `None`); directions are rotated by it, the base point is placed by it and
/// split into the double-single pair.
fn collect_curved_gens(
    sat: &SatDocument,
    xform: Option<([f64; 9], [f64; 3], f64)>,
) -> Vec<crate::scene::model::mesh_model::CurvedGen> {
    use crate::scene::model::mesh_model::CurvedGen;
    let rot_dir = |d: [f64; 3]| -> [f32; 3] {
        let w = match xform {
            Some((m, _, _)) => norm3([
                d[0] * m[0] + d[1] * m[3] + d[2] * m[6],
                d[0] * m[1] + d[1] * m[4] + d[2] * m[7],
                d[0] * m[2] + d[1] * m[5] + d[2] * m[8],
            ]),
            None => d,
        };
        [w[0] as f32, w[1] as f32, w[2] as f32]
    };
    let scale = xform.map(|(_, _, s)| s).unwrap_or(1.0);
    // Place a world point and split it into the double-single (high, low) pair.
    let place = |p: [f64; 3]| -> ([f32; 3], [f32; 3]) {
        let (wx, wy, wz) = match xform {
            Some((m, tr, s)) => (
                s * (p[0] * m[0] + p[1] * m[3] + p[2] * m[6]) + tr[0],
                s * (p[0] * m[1] + p[1] * m[4] + p[2] * m[7]) + tr[1],
                s * (p[0] * m[2] + p[1] * m[5] + p[2] * m[8]) + tr[2],
            ),
            None => (p[0], p[1], p[2]),
        };
        let (hx, hy, hz) = (wx as f32, wy as f32, wz as f32);
        (
            [hx, hy, hz],
            [
                (wx - hx as f64) as f32,
                (wy - hy as f64) as f32,
                (wz - hz as f64) as f32,
            ],
        )
    };
    let mut out = Vec::new();
    for face in sat.faces() {
        let Some(surf) = sat.resolve(face.surface()) else {
            continue;
        };
        match surf.entity_type.as_str() {
            "cone-surface" => {
                let Some(g) = cone_face_geom(sat, &face) else {
                    continue;
                };
                let base_local = [
                    g.center[0] + g.h_min * g.axis[0],
                    g.center[1] + g.h_min * g.axis[1],
                    g.center[2] + g.h_min * g.axis[2],
                ];
                let (base, base_low) = place(base_local);
                out.push(CurvedGen::Cone {
                    base,
                    base_low,
                    axis: rot_dir(g.axis),
                    u_dir: rot_dir(g.u_dir),
                    v_dir: rot_dir(g.v_dir),
                    // `radius` is the cone radius at `base`, which sits at h_min —
                    // NOT at the surface's h=0 root. `cone_face_geom.radius` is the
                    // root radius, so add the h_min offset (`radius + h_min·tan_a`)
                    // or the silhouette's `r0 = radius` lands at the wrong radius
                    // and its top `r1 = radius + span·tan_a` overshoots the apex.
                    radius: ((g.radius + g.h_min * g.tan_a) * scale) as f32,
                    tan_a: g.tan_a as f32,
                    h_max: ((g.h_max - g.h_min) * scale) as f32,
                    theta_min: g.theta_min as f32,
                    theta_span: g.theta_span as f32,
                    full: g.full,
                });
            }
            "sphere-surface" => {
                let Some(sphere) = SatSphereSurface::from_record(surf) else {
                    continue;
                };
                let (cx, cy, cz) = sphere.center();
                let pole = norm3([sphere.pole().0, sphere.pole().1, sphere.pole().2]);
                let u = norm3([
                    sphere.u_direction().0,
                    sphere.u_direction().1,
                    sphere.u_direction().2,
                ]);
                let v = cross3(pole, u);
                let poly = collect_face_polygon(sat, &face, BOUNDARY_CHORD_FRAC);
                let (tmin, tspan, full, pmin, pmax) =
                    sphere_param_range(&poly, [cx, cy, cz], pole, u, v);
                let (center, center_low) = place([cx, cy, cz]);
                out.push(CurvedGen::Sphere {
                    center,
                    center_low,
                    pole: rot_dir(pole),
                    u_dir: rot_dir(u),
                    v_dir: rot_dir(v),
                    radius: (sphere.radius() * scale) as f32,
                    theta_min: tmin as f32,
                    theta_span: tspan as f32,
                    full,
                    phi_min: pmin as f32,
                    phi_max: pmax as f32,
                });
            }
            "torus-surface" => {
                let Some(torus) = SatTorusSurface::from_record(surf) else {
                    continue;
                };
                let (cx, cy, cz) = torus.center();
                let axis = norm3([torus.normal().0, torus.normal().1, torus.normal().2]);
                let u = norm3([
                    torus.u_direction().0,
                    torus.u_direction().1,
                    torus.u_direction().2,
                ]);
                let v = cross3(axis, u);
                let (pmin, pspan, full) = torus_phi_range(
                    sat,
                    &face,
                    [cx, cy, cz],
                    u,
                    v,
                    torus.major_radius(),
                );
                let (tmin, tspan, tfull) = torus_theta_range(
                    sat,
                    &face,
                    [cx, cy, cz],
                    axis,
                    torus.major_radius(),
                    torus.minor_radius(),
                );
                let (center, center_low) = place([cx, cy, cz]);
                out.push(CurvedGen::Torus {
                    center,
                    center_low,
                    axis: rot_dir(axis),
                    u_dir: rot_dir(u),
                    v_dir: rot_dir(v),
                    major: (torus.major_radius() * scale) as f32,
                    minor: (torus.minor_radius() * scale) as f32,
                    phi_min: pmin as f32,
                    phi_span: pspan as f32,
                    full,
                    theta_min: tmin as f32,
                    theta_span: tspan as f32,
                    theta_full: tfull,
                });
            }
            _ => {}
        }
    }
    out
}

/// Line-list endpoints (pairs) for every `edge` record: straight edges emit
/// their two vertex endpoints; ellipse/circle edges are sampled along their
/// bounded parametric arc. Points are in body-local space (pre-transform).
fn collect_feature_edges(sat: &SatDocument) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::new();
    for er in sat.records_of_type("edge") {
        let Some(edge) = SatEdge::from_record(er) else {
            continue;
        };
        // Ordered points along the edge (≥2).
        let mut pts: Vec<[f64; 3]> = Vec::new();
        if let Some(cr) = sat.resolve(edge.curve()) {
            if let Some(ellipse) = SatEllipseCurve::from_record(cr) {
                let reversed = matches!(edge.sense(), Sense::Reversed);
                pts = sample_ellipse_arc(
                    &ellipse,
                    edge.start_param(),
                    edge.end_param(),
                    EDGE_CHORD_FRAC,
                    reversed,
                );
                // sample_ellipse_arc drops the end param; append the true end so
                // the polyline closes onto the shared vertex.
                if let Some(p) = vertex_point(sat, edge.end_vertex()) {
                    pts.push(p);
                }
            } else if let Some(ic) = SatIntCurve::from_record(cr) {
                // Spline edge — sample the actual curve instead of the straight
                // chord the fallback below would draw (or nothing, for a closed
                // loop whose endpoints coincide).
                pts = ic
                    .sample_range(
                        edge.start_param(),
                        edge.end_param(),
                        nominal_segs(EDGE_CHORD_FRAC),
                    )
                    .into_iter()
                    .map(|(x, y, z)| [x, y, z])
                    .collect();
            }
        }
        if pts.len() < 2 {
            // Straight edge (or unsampled curve): connect the two vertices.
            pts.clear();
            if let (Some(a), Some(b)) = (
                vertex_point(sat, edge.start_vertex()),
                vertex_point(sat, edge.end_vertex()),
            ) {
                pts.push(a);
                pts.push(b);
            }
        }
        // Emit consecutive points as line-list segment pairs.
        for w in pts.windows(2) {
            out.push(w[0]);
            out.push(w[1]);
        }
    }
    out
}

/// Resolve a vertex pointer to its point coordinates.
fn vertex_point(sat: &SatDocument, vptr: SatPointer) -> Option<[f64; 3]> {
    let vrec = sat.resolve(vptr)?;
    let vertex = SatVertex::from_record(vrec)?;
    let prec = sat.resolve(vertex.point())?;
    let point = SatPoint::from_record(prec)?;
    let (x, y, z) = point.position();
    Some([x, y, z])
}

/// Tessellate a SAT document at all three LODs and bundle them into a
/// `MeshLodSet` ready for the render pipeline to pick a level per frame.
fn tessellate_sat_lods(
    sat: &SatDocument,
    name: String,
    color: [f32; 4],
    facet_res: f64,
) -> Option<MeshLodSet> {
    let configs = LodConfig::all();
    let xform = body_transform(sat);
    let mut lods: Vec<MeshModel> = Vec::with_capacity(3);
    let mut complete = true;
    for lod in configs {
        let scaled = scale_lod(lod, facet_res);
        if let Some((m, lod_complete)) = tessellate_sat(sat, name.clone(), color, scaled, xform) {
            lods.push(m);
            complete &= lod_complete;
        } else {
            complete = false;
        }
    }
    if lods.is_empty() {
        return None;
    }
    let mut set = MeshLodSet::from_lods(lods);
    set.complete = complete;
    Some(set)
}

/// Extract the body's placement transform from the SAT document: a row-major
/// 3×3 affine, a translation, and the uniform scale. ACIS keeps a solid's
/// geometry in body-local space and records the placement in a `transform`
/// record (`<3×3> <tx ty tz> <scale> rotate reflect shear`). `None` when the
/// document has no transform (treated as identity).
pub(crate) fn body_transform(sat: &SatDocument) -> Option<([f64; 9], [f64; 3], f64)> {
    let transform = sat
        .records
        .iter()
        .find(|record| record.entity_type == "transform")?;
    let mut values = Vec::with_capacity(13);
    for token in &transform.tokens {
        if values.len() >= 13 {
            break;
        }
        if let Some((components, len)) = token.coordinate_components() {
            values.extend_from_slice(&components[..len]);
        } else if let Some(value) = token.as_float() {
            values.push(value);
        } else if let Some(text) = token.as_string() {
            // Some ASM SAB bodies pack the complete SAT transform payload in
            // one long-string token instead of individual numeric tokens.
            for word in text.split_ascii_whitespace() {
                let Ok(value) = word.parse::<f64>() else {
                    break;
                };
                values.push(value);
                if values.len() >= 13 {
                    break;
                }
            }
        }
    }
    (values.len() >= 13).then(|| {
        (
            [
                values[0], values[1], values[2], values[3], values[4], values[5], values[6],
                values[7], values[8],
            ],
            [values[9], values[10], values[11]],
            values[12],
        )
    })
}

/// Apply a body placement transform to a mesh. ACIS treats points as row
/// vectors (`p' = scale·(p·M) + T`), so the 3×3 is indexed transposed relative
/// to a column-vector multiply. Normals get the rotation only, renormalized.
/// Build a `MeshModel` from f64 accumulation buffers. This is the ONLY place a
/// solid mesh vertex becomes f32: the world coordinate is computed in f64 (the
/// body placement applied, or identity when the solid stores absolute geometry)
/// and split into the double-single (high, low) pair the mesh shader
/// reconstructs relative to the eye — exactly the treatment the feature edges
/// get in `attach_feature_edges`. Casting to f32 any earlier quantizes a solid
/// placed at UTM scale to a ~0.06 m grid, so its shaded faces crawl against
/// their own (double-single) wireframe as the camera moves. The split runs
/// unconditionally: many solids store their geometry in absolute coordinates
/// with no body transform, and those need it just as much as placed ones.
pub(crate) fn finalize_mesh(
    name: String,
    verts: Vec<[f64; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
    triangle_material_handles: Vec<Option<acadrust::Handle>>,
    triangle_colors: Vec<Option<[f32; 4]>>,
    color: [f32; 4],
    xform: Option<([f64; 9], [f64; 3], f64)>,
) -> MeshModel {
    let mut hi: Vec<[f32; 3]> = Vec::with_capacity(verts.len());
    let mut lo: Vec<[f32; 3]> = Vec::with_capacity(verts.len());
    for [x, y, z] in verts {
        let (wx, wy, wz) = match &xform {
            Some((m, tr, scale)) => (
                scale * (x * m[0] + y * m[3] + z * m[6]) + tr[0],
                scale * (x * m[1] + y * m[4] + z * m[7]) + tr[1],
                scale * (x * m[2] + y * m[5] + z * m[8]) + tr[2],
            ),
            None => (x, y, z),
        };
        let (hx, hy, hz) = (wx as f32, wy as f32, wz as f32);
        hi.push([hx, hy, hz]);
        lo.push([
            (wx - hx as f64) as f32,
            (wy - hy as f64) as f32,
            (wz - hz as f64) as f32,
        ]);
    }
    let normals = match &xform {
        Some((m, _, _)) => normals
            .iter()
            .map(|n| {
                let (x, y, z) = (n[0] as f64, n[1] as f64, n[2] as f64);
                let nx = x * m[0] + y * m[3] + z * m[6];
                let ny = x * m[1] + y * m[4] + z * m[7];
                let nz = x * m[2] + y * m[5] + z * m[8];
                let len = (nx * nx + ny * ny + nz * nz).sqrt();
                if len > 1e-9 {
                    [(nx / len) as f32, (ny / len) as f32, (nz / len) as f32]
                } else {
                    *n
                }
            })
            .collect(),
        None => normals,
    };
    MeshModel {
        name,
        verts: hi,
        verts_low: lo,
        normals,
        indices,
        triangle_material_handles,
        triangle_colors,
        color,
        selected: false,
    }
}

/// Tighten/loosen a LOD's chord tolerance by FACETRES (clamped to the
/// documented [0.01, 10.0] range). A higher FACETRES means a finer mesh, so it
/// *divides* the chord fraction; 1.0 is the unchanged baseline.
fn scale_lod(base: LodConfig, facet_res: f64) -> LodConfig {
    let m = facet_res.clamp(0.01, 10.0);
    LodConfig {
        chord_frac: (base.chord_frac / m).clamp(1e-4, 0.5),
    }
}

/// World-XY AABB of the mesh — used by the render-pipeline LOD selector
/// to pick a level based on projected pixel diagonal.
#[allow(dead_code)] // superseded by mesh_model::compute_mesh_aabb (3D); kept for reference
pub(crate) fn mesh_aabb(mesh: &MeshModel) -> [f32; 4] {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &[x, y, _] in &mesh.verts {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        if x < min_x {
            min_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if x > max_x {
            max_x = x;
        }
        if y > max_y {
            max_y = y;
        }
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

fn remap_acis_material_bindings(
    set: &mut MeshLodSet,
    acis: &acadrust::entities::AcisData,
) {
    if acis.materials.is_empty() {
        return;
    }
    for lod in &mut set.lods {
        for handle in lod.triangle_material_handles.iter_mut().flatten() {
            let reference = handle.value() as i32;
            if let Some(material_handle) = acis.materials.iter().find_map(|binding| {
                (binding.absolute_reference == reference || binding.array_index == reference)
                    .then_some(binding.material_handle)
                    .flatten()
            }) {
                *handle = material_handle;
            }
        }
    }
}

fn attach_stored_silhouettes(
    set: &mut MeshLodSet,
    silhouettes: &[acadrust::entities::Silhouette],
) {
    use crate::scene::model::mesh_model::StoredSilhouette;
    set.stored_silhouettes = silhouettes
        .iter()
        .filter_map(|silhouette| {
            let mut edge_verts = Vec::new();
            let mut edge_verts_low = Vec::new();
            for wire in &silhouette.wires {
                for segment in wire.points.windows(2) {
                    for point in segment {
                        let point = crate::entities::solid3d::wire_point(wire, point);
                        let high = [point.x as f32, point.y as f32, point.z as f32];
                        edge_verts.push(high);
                        edge_verts_low.push([
                            (point.x - high[0] as f64) as f32,
                            (point.y - high[1] as f64) as f32,
                            (point.z - high[2] as f64) as f32,
                        ]);
                    }
                }
            }
            (!edge_verts.is_empty()).then_some(StoredSilhouette {
                viewport_id: silhouette.viewport_id,
                view_direction: [
                    silhouette.view_direction.x as f32,
                    silhouette.view_direction.y as f32,
                    silhouette.view_direction.z as f32,
                ],
                up_vector: [
                    silhouette.up_vector.x as f32,
                    silhouette.up_vector.y as f32,
                    silhouette.up_vector.z as f32,
                ],
                target: [
                    silhouette.target.x as f32,
                    silhouette.target.y as f32,
                    silhouette.target.z as f32,
                ],
                is_perspective: silhouette.is_perspective,
                edge_verts,
                edge_verts_low,
            })
        })
        .collect();
}

/// Tessellate a `Region` entity (2D planar ACIS body) at all three LOD levels.
pub fn tessellate_region(
    region: &Region,
    color: [f32; 4],
    facet_res: f64,
    isolines: usize,
) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || region.parse_sat(),
        region.acis_data.is_binary,
        &region.acis_data.sab_data,
    )?;
    let name = region.common.handle.value().to_string();
    let mut set = tessellate_acis(&sat, name, color, facet_res, isolines)?;
    remap_acis_material_bindings(&mut set, &region.acis_data);
    attach_stored_silhouettes(&mut set, &region.silhouettes);
    Some(set)
}

/// Tessellate a `Body` entity (3D ACIS body) at all three LOD levels.
pub fn tessellate_body(
    body: &Body,
    color: [f32; 4],
    facet_res: f64,
    isolines: usize,
) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || body.parse_sat(),
        body.acis_data.is_binary,
        &body.acis_data.sab_data,
    )?;
    let name = body.common.handle.value().to_string();
    let mut set = tessellate_acis(&sat, name, color, facet_res, isolines)?;
    remap_acis_material_bindings(&mut set, &body.acis_data);
    attach_stored_silhouettes(&mut set, &body.silhouettes);
    Some(set)
}

/// Tessellate a `Surface` entity (ACAD_SURFACE family) at all three LOD
/// levels. Surfaces are ACIS-backed just like bodies, so the same SAT/SAB
/// path applies — including the B-spline `spline-surface` faces that loft /
/// sweep / revolve produce.
pub fn tessellate_surface(
    surface: &acadrust::entities::Surface,
    color: [f32; 4],
    facet_res: f64,
    isolines: usize,
) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || surface.parse_sat(),
        surface.acis_data.is_binary,
        &surface.acis_data.sab_data,
    )?;
    let name = surface.common.handle.value().to_string();
    let surface_isolines = usize::from(surface.u_isolines.max(surface.v_isolines).max(0) as u16);
    let mut set =
        tessellate_acis(&sat, name, color, facet_res, surface_isolines.max(isolines))?;
    remap_acis_material_bindings(&mut set, &surface.acis_data);
    attach_stored_silhouettes(&mut set, &surface.silhouettes);
    Some(set)
}

/// Tessellate a `Solid3D` entity at all three LOD levels.
///
/// Returns `None` when the entity has no parseable SAT data or produces no
/// triangles (e.g. the solid uses only unsupported surface types).
/// `facet_res` mirrors the header FACETRES variable (0.01–10.0).
pub fn tessellate_solid3d(
    solid: &Solid3D,
    color: [f32; 4],
    facet_res: f64,
    isolines: usize,
) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || solid.parse_sat(),
        solid.acis_data.is_binary,
        &solid.acis_data.sab_data,
    )?;
    let name = solid.common.handle.value().to_string();
    let mut set = tessellate_acis(&sat, name, color, facet_res, isolines)?;
    remap_acis_material_bindings(&mut set, &solid.acis_data);
    attach_stored_silhouettes(&mut set, &solid.silhouettes);
    Some(set)
}

// ── Topology helpers ──────────────────────────────────────────────────────────

/// Walk a face's outer coedge loop and collect ordered 3-D boundary points.
///
/// Straight edges contribute their start vertex; curved (ellipse / circle)
/// edges are sampled into several points so circular boundaries — e.g. the
/// cap of a cylinder or the rim of a cone — produce a real polygon instead of
/// a single degenerate vertex. `chord_frac` is the chord-height tolerance (as a
/// fraction of each edge's own radius); the segment count per edge derives from
/// it plus that edge's radius and span.
///
/// Returns an empty `Vec` when the loop topology is broken or has fewer than
/// three distinct points.
pub(crate) fn collect_face_polygon(
    sat: &SatDocument,
    face: &SatFace,
    chord_frac: f64,
) -> Vec<[f64; 3]> {
    let Some(loop_rec) = sat.resolve(face.first_loop()) else {
        return vec![];
    };
    let Some(sat_loop) = SatLoop::from_record(loop_rec) else {
        return vec![];
    };
    collect_loop_polygon(sat, &sat_loop, chord_frac)
}

/// Boundary points of a single coedge loop, in order.
pub(crate) fn collect_loop_polygon(
    sat: &SatDocument,
    sat_loop: &SatLoop,
    chord_frac: f64,
) -> Vec<[f64; 3]> {
    let first_ptr = sat_loop.first_coedge();
    let mut cur = first_ptr;
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut visited: HashSet<i32> = HashSet::default();

    loop {
        if cur.is_null() || visited.contains(&cur.0) {
            break;
        }
        visited.insert(cur.0);

        if let Some(ce_rec) = sat.resolve(cur) {
            if let Some(coedge) = SatCoedge::from_record(ce_rec) {
                append_coedge_points(sat, &coedge, chord_frac, &mut pts);
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

/// All loops of a face: the outer boundary first, then any inner hole loops.
/// Each loop is returned as an ordered 3-D polygon (≥ 3 points).
pub(crate) fn collect_face_loops(
    sat: &SatDocument,
    face: &SatFace,
    chord_frac: f64,
) -> Vec<Vec<[f64; 3]>> {
    let mut loops: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut loop_ptr = face.first_loop();
    let mut seen: HashSet<i32> = HashSet::default();
    while !loop_ptr.is_null() && seen.insert(loop_ptr.0) {
        let Some(loop_rec) = sat.resolve(loop_ptr) else {
            break;
        };
        let Some(sat_loop) = SatLoop::from_record(loop_rec) else {
            break;
        };
        let poly = collect_loop_polygon(sat, &sat_loop, chord_frac);
        if poly.len() >= 3 {
            loops.push(poly);
        }
        loop_ptr = sat_loop.next_loop();
    }
    loops
}

/// Append a coedge's boundary points to `pts`. Ellipse/circle curves are
/// sampled along their parametric arc (excluding the end param so the next
/// coedge's start point provides the junction); all other curve types fall
/// back to the single start vertex, respecting coedge sense.
pub(crate) fn append_coedge_points(
    sat: &SatDocument,
    coedge: &SatCoedge,
    chord_frac: f64,
    pts: &mut Vec<[f64; 3]>,
) {
    let fwd = matches!(coedge.sense(), Sense::Forward);
    let Some(edge_rec) = sat.resolve(coedge.edge()) else {
        return;
    };
    let Some(edge) = SatEdge::from_record(edge_rec) else {
        return;
    };

    if let Some(curve_rec) = sat.resolve(edge.curve()) {
        if let Some(ellipse) = SatEllipseCurve::from_record(curve_rec) {
            // The edge's own sense (relative to its curve) decides the ellipse
            // winding; a reversed edge samples the opposite handedness.
            let reversed = matches!(edge.sense(), Sense::Reversed);
            // Orient the parameter range before sampling. Reversing an
            // endpoint-exclusive sample afterwards would drop the traversal
            // start and retain its end, duplicating the next coedge's point and
            // cutting the connector corner out of the face boundary.
            let (start, end) = if fwd {
                (edge.start_param(), edge.end_param())
            } else {
                (edge.end_param(), edge.start_param())
            };
            let sampled = sample_ellipse_arc(
                &ellipse,
                start,
                end,
                chord_frac,
                reversed,
            );
            if !sampled.is_empty() {
                pts.extend(sampled);
                return;
            }
        }
        // Spline (intcurve) edge: sample its own arc so a curved face boundary
        // (fillet/blend) is a real loop rather than a straight chord — without
        // this the face's parametric extent collapses and it can't be trimmed.
        if let Some(ic) = SatIntCurve::from_record(curve_rec) {
            let mut sampled: Vec<[f64; 3]> = ic
                .sample_range(
                    edge.start_param(),
                    edge.end_param(),
                    nominal_segs(chord_frac),
                )
                .into_iter()
                .map(|(x, y, z)| [x, y, z])
                .collect();
            // Orient the inclusive sample first, then drop the traversal end so
            // the adjacent coedge contributes that shared point.
            if !fwd {
                sampled.reverse();
            }
            sampled.pop();
            if sampled.len() >= 2 {
                pts.extend(sampled);
                return;
            }
        }
    }

    // Straight / unsupported curve: keep the single start vertex.
    let v_ptr = if fwd {
        edge.start_vertex()
    } else {
        edge.end_vertex()
    };
    if let Some(pt) = resolve_point(sat, v_ptr) {
        pts.push(pt);
    }
}

struct EllipseFrame {
    center: [f64; 3],
    major: [f64; 3],
    minor: [f64; 3],
    major_len: f64,
}

impl EllipseFrame {
    fn point_tangent(&self, parameter: f64) -> ([f64; 3], [f64; 3]) {
        let (sin_parameter, cos_parameter) = parameter.sin_cos();
        (
            [
                self.center[0]
                    + self.major[0] * cos_parameter
                    + self.minor[0] * sin_parameter,
                self.center[1]
                    + self.major[1] * cos_parameter
                    + self.minor[1] * sin_parameter,
                self.center[2]
                    + self.major[2] * cos_parameter
                    + self.minor[2] * sin_parameter,
            ],
            [
                -self.major[0] * sin_parameter + self.minor[0] * cos_parameter,
                -self.major[1] * sin_parameter + self.minor[1] * cos_parameter,
                -self.major[2] * sin_parameter + self.minor[2] * cos_parameter,
            ],
        )
    }
}

/// Analytic ellipse frame shared by boundary sampling and trim orientation.
fn ellipse_frame(ellipse: &SatEllipseCurve, curve_reversed: bool) -> Option<EllipseFrame> {
    let center = ellipse.center();
    let major = ellipse.major_axis();
    let major_len = (major.0 * major.0 + major.1 * major.1 + major.2 * major.2).sqrt();
    if major_len < 1e-12 {
        return None;
    }
    let major_u = [
        major.0 / major_len,
        major.1 / major_len,
        major.2 / major_len,
    ];
    let normal = norm3([ellipse.normal().0, ellipse.normal().1, ellipse.normal().2]);
    let minor_u = cross3(normal, major_u);
    let minor_len = major_len * ellipse.ratio();
    let hand = if curve_reversed { -1.0 } else { 1.0 };
    Some(EllipseFrame {
        center: [center.0, center.1, center.2],
        major: [major_u[0] * major_len, major_u[1] * major_len, major_u[2] * major_len],
        minor: [
            minor_u[0] * minor_len * hand,
            minor_u[1] * minor_len * hand,
            minor_u[2] * minor_len * hand,
        ],
        major_len,
    })
}

/// Sample points along an ellipse/circle arc from `sp` to `ep` (radians).
/// Returns points at the start of each segment (the end param is omitted so
/// adjacent coedges don't double up the shared junction point). Segment count
/// scales with the arc's angular span relative to a full circle.
fn sample_ellipse_arc(
    ellipse: &SatEllipseCurve,
    sp: f64,
    ep: f64,
    chord_frac: f64,
    curve_reversed: bool,
) -> Vec<[f64; 3]> {
    let span = ep - sp;
    if span.abs() < 1e-9 {
        return vec![];
    }
    let Some(frame) = ellipse_frame(ellipse, curve_reversed) else {
        return vec![];
    };

    // Segment count from this ellipse's own radius and arc span at the requested
    // chord tolerance — the same model the 2-D circle/arc/ellipse wires use, so
    // a big rim samples finer than a small one and a short arc proportionally
    // less than a full turn. `major_len` is the reference radius.
    let segs = crate::scene::convert::tess_util::arc_segments_floored(
        frame.major_len,
        span.abs(),
        frame.major_len * chord_frac,
        2,
    ) as usize;
    let mut out = Vec::with_capacity(segs);
    for i in 0..segs {
        let t = sp + span * (i as f64 / segs as f64);
        out.push(frame.point_tangent(t).0);
    }
    out
}

/// Resolve a vertex pointer all the way to its `[x, y, z]` coordinate.
pub(crate) fn resolve_point(sat: &SatDocument, v_ptr: SatPointer) -> Option<[f64; 3]> {
    let v_rec = sat.resolve(v_ptr)?;
    let vertex = SatVertex::from_record(v_rec)?;
    let pt_rec = sat.resolve(vertex.point())?;
    let point = SatPoint::from_record(pt_rec)?;
    let (x, y, z) = point.position();
    Some([x, y, z])
}

// ── Mesh builder helpers ──────────────────────────────────────────────────────

/// Append one quad (two triangles) to the mesh buffers. Vertices stay in f64
/// world/local space until `finalize_mesh` splits them into the double-single
/// (high, low) pair — casting here would quantize a solid placed at UTM scale to
/// the ~0.06 m f32 grid.
#[inline]
fn push_quad(
    verts: &mut Vec<[f64; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    p: [[f64; 3]; 4],
    n: [f64; 3],
) {
    let base = verts.len() as u32;
    let nf = [n[0] as f32, n[1] as f32, n[2] as f32];
    for &pt in &p {
        verts.push(pt);
        normals.push(nf);
    }
    // Two CCW triangles: (0,1,2) and (0,2,3)
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

// ── Planar face ───────────────────────────────────────────────────────────────

pub(crate) fn tess_plane_face(
    sat: &SatDocument,
    face: &SatFace,
    plane: &SatPlaneSurface,
    chord_frac: f64,
    verts: &mut Vec<[f64; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    let mut poly = collect_face_polygon(sat, face, chord_frac);
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

    if dot3(newell_normal(&poly), [nx, ny, nz]) < 0.0 {
        poly.reverse();
    }

    let base = verts.len() as u32;
    for &pt in &poly {
        verts.push(pt);
        normals.push(nf);
    }

    // Fan triangulation from vertex 0 (outer loop only; holes are handled by
    // the truck B-rep path in `acis_to_truck`).
    let n = poly.len() as u32;
    for i in 1..(n - 1) {
        indices.extend_from_slice(&[base, base + i, base + i + 1]);
    }
}

// ── Cone / cylinder face ──────────────────────────────────────────────────────

pub(crate) fn tess_cone_face(
    sat: &SatDocument,
    face: &SatFace,
    cone: &SatConeSurface,
    lod: LodConfig,
    verts: &mut Vec<[f64; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    // Determine the height range and angular span from the boundary polygon.
    let poly = collect_face_polygon(sat, face, lod.chord_frac);

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
    let (mut h_min, mut h_max, mut theta_min, mut theta_max, full_circle) =
        angular_range(cx, cy, cz, axis, u_dir, v_dir, &poly);

    // A full cylinder/cone face is bounded by a single closed rim, so the
    // boundary alone can't span the height — the second extent (top rim or
    // apex) lives on a different face. When the boundary collapses to one
    // height, recover the span from the solid's coaxial circle rims plus the
    // analytic apex of a true cone. Only sweep the full revolution when the
    // boundary really is a closed rim; a bounded arc face (e.g. a curved
    // mullion bar) keeps its own angular span, else it balloons to a circle.
    if (h_max - h_min).abs() < 1e-9 {
        if let Some((vmin, vmax)) = cone_axis_span(sat, face, cone, axis, [cx, cy, cz]) {
            h_min = vmin;
            h_max = vmax;
            if full_circle {
                theta_min = 0.0;
                theta_max = TAU;
            }
        }
    }

    let theta_span = if full_circle {
        TAU
    } else {
        theta_max - theta_min
    };
    let h_span = h_max - h_min;

    if h_span.abs() < 1e-10 || theta_span.abs() < 1e-10 {
        return;
    }

    // Angular divisions from the rim radius and arc span at the LOD's chord
    // tolerance — a short boundary arc (a curved wall face) samples proportionally
    // less than a whole rim. Use the wider rim so the density bounds chord error
    // at both ends. The height direction is a straight generator (a cone/cylinder
    // is ruled), so it carries no curvature: one division is geometrically exact.
    let r_ref = if cos_a.abs() > 1e-9 {
        (radius + h_min * sin_a / cos_a)
            .abs()
            .max((radius + h_max * sin_a / cos_a).abs())
    } else {
        radius.abs()
    };
    let segs_u = lod.arc_segs(r_ref, theta_span).max(1);
    let segs_v = 1; // straight generator — no curvature along the height

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

            // Wind the quad so its face (CCW) normal points radially outward,
            // matching the supplied per-vertex normal `n` below. This keeps
            // flat-shaded mode (which derives the normal from winding) and
            // Gouraud mode (which uses `n`) consistent.
            let p = [
                cone_pt(cx, cy, cz, axis, u_dir, v_dir, r0, a0, t0),
                cone_pt(cx, cy, cz, axis, u_dir, v_dir, r0, a1, t0),
                cone_pt(cx, cy, cz, axis, u_dir, v_dir, r1, a1, t1),
                cone_pt(cx, cy, cz, axis, u_dir, v_dir, r1, a0, t1),
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

    // Find the arc from the LARGEST angular gap, not raw min/max. `atan2`
    // returns [-π, π]; a short arc that straddles the ±π seam splits its points
    // between ≈+π and ≈-π, so naive `max - min` reads ≈2π and the face balloons
    // into a full revolution (a curved wall of radius R drawn as a whole
    // R-cylinder). The empty region a face doesn't cover is its largest gap, so
    // the real arc is the complement: it starts just after the gap and runs to
    // just before it (wrapping past the seam when needed).
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = angles.len();
    let (mut max_gap, mut gap_i) = (0.0_f64, 0_usize);
    for i in 0..n {
        let a = angles[i];
        let b = if i + 1 < n {
            angles[i + 1]
        } else {
            angles[0] + TAU
        };
        let gap = b - a;
        if gap > max_gap {
            max_gap = gap;
            gap_i = i;
        }
    }

    // A genuine full circle has points spread all the way round, so its largest
    // gap is small. A real arc leaves a wide empty wedge.
    let full = max_gap < TAU * 0.05;

    let theta_min = angles[(gap_i + 1) % n];
    let mut theta_max = angles[gap_i];
    if theta_max <= theta_min {
        theta_max += TAU;
    }

    (h_min, h_max, theta_min, theta_max, full)
}

/// Recover a cone/cylinder face's height span (along its axis) from the solid's
/// circular rims or B-rep vertices when the face boundary collapses to a
/// single height.
///
/// Uses this face's own boundary loops first. If that topology is incomplete,
/// matching coaxial rims and points on the analytic surface provide a fallback.
/// For a true cone with a single rim, the tip is added analytically. Returns
/// `None` when no span is recoverable.
pub(crate) fn cone_axis_span(
    sat: &SatDocument,
    face: &SatFace,
    cone: &SatConeSurface,
    axis: [f64; 3],
    center: [f64; 3],
) -> Option<(f64, f64)> {
    let mut heights: Vec<f64> = Vec::new();

    // Prefer the boundaries owned by this face. A body may contain several
    // coaxial cylinders, so document-wide rims cannot identify this face's
    // axial extent reliably.
    for point in collect_face_loops(sat, face, BOUNDARY_CHORD_FRAC)
        .into_iter()
        .flatten()
    {
        heights.push(dot3(
            [
                point[0] - center[0],
                point[1] - center[1],
                point[2] - center[2],
            ],
            axis,
        ));
    }
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap());
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    if heights.len() >= 2 {
        let h_min = heights[0];
        let h_max = *heights.last().unwrap();
        if (h_max - h_min).abs() >= 1e-9 {
            return Some((h_min, h_max));
        }
    }

    let local_height = heights.first().copied();
    heights.clear();
    let sin_a = cone.sin_half_angle();
    let cos_a = cone.cos_half_angle();
    let tangent = if cos_a.abs() > 1e-9 {
        sin_a / cos_a
    } else {
        0.0
    };
    for rec in &sat.records {
        if rec.entity_type != "ellipse-curve" {
            continue;
        }
        let Some(e) = SatEllipseCurve::from_record(rec) else {
            continue;
        };
        let ec = e.center();
        let d = [ec.0 - center[0], ec.1 - center[1], ec.2 - center[2]];
        let h = dot3(d, axis);
        // Radial offset from the axis line: must be ~0 to be coaxial.
        let radial = [d[0] - h * axis[0], d[1] - h * axis[1], d[2] - h * axis[2]];
        let radial_len = dot3(radial, radial).sqrt();
        let n = e.normal();
        let n_dot = dot3(norm3([n.0, n.1, n.2]), axis).abs();
        let major = e.major_axis();
        let curve_radius = dot3([major.0, major.1, major.2], [major.0, major.1, major.2])
            .sqrt();
        let expected_radius = (cone.radius() + h * tangent).abs();
        let scale = curve_radius.max(expected_radius).max(1.0);
        if radial_len < scale * 1e-6
            && n_dot > 0.999
            && (curve_radius - expected_radius).abs() < scale * 1e-5
        {
            heights.push(h);
        }
    }
    if heights.len() < 2 {
        for rec in &sat.records {
            let Some(point) = SatPoint::from_record(rec) else {
                continue;
            };
            let position = point.position();
            let d = [
                position.0 - center[0],
                position.1 - center[1],
                position.2 - center[2],
            ];
            let h = dot3(d, axis);
            let radial = [
                d[0] - h * axis[0],
                d[1] - h * axis[1],
                d[2] - h * axis[2],
            ];
            let radial_len = dot3(radial, radial).sqrt();
            let expected = (cone.radius() + h * tangent).abs();
            let tolerance = expected.max(cone.radius().abs()).max(1.0) * 1e-5;
            if (radial_len - expected).abs() <= tolerance {
                heights.push(h);
            }
        }
    }
    if heights.is_empty() {
        return None;
    }
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap());
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

    if let Some(anchor) = local_height {
        if let Some(mate) = heights
            .iter()
            .copied()
            .filter(|h| (*h - anchor).abs() >= 1e-6)
            .min_by(|a, b| {
                (a - anchor)
                    .abs()
                    .partial_cmp(&(b - anchor).abs())
                    .unwrap()
            })
        {
            return Some((anchor.min(mate), anchor.max(mate)));
        }
        if sin_a.abs() > 1e-6 {
            let apex = -cone.radius() * cos_a / sin_a;
            if (apex - anchor).abs() >= 1e-9 {
                return Some((anchor.min(apex), anchor.max(apex)));
            }
        }
        return None;
    }

    let mut h_min = heights[0];
    let mut h_max = *heights.last().unwrap();

    // True cone with a single rim: close the surface at its apex (r = 0).
    if sin_a.abs() > 1e-6 && heights.len() <= 1 {
        let apex = -cone.radius() * cos_a / sin_a;
        h_min = h_min.min(apex);
        h_max = h_max.max(apex);
    }

    if (h_max - h_min).abs() < 1e-9 {
        return None;
    }
    Some((h_min, h_max))
}

// ── Sphere face ───────────────────────────────────────────────────────────────

pub(crate) fn tess_sphere_face(
    sat: &SatDocument,
    face: &SatFace,
    sphere: &SatSphereSurface,
    lod: LodConfig,
    verts: &mut Vec<[f64; 3]>,
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

    // Mesh only the part of the sphere the face covers — its boundary loop's
    // longitude/colatitude window. A partial sphere (a fillet cap) otherwise
    // builds as a full ball floating where the solid is open.
    let poly = collect_face_polygon(sat, face, BOUNDARY_CHORD_FRAC);
    let (t_min, t_span, full, p_min, p_max) =
        sphere_param_range(&poly, [cx, cy, cz], pole, u_dir, v_dir);
    let (theta_lo, theta_hi) = if full {
        (0.0, TAU)
    } else {
        (t_min, t_min + t_span)
    };
    let (phi_lo, phi_hi) = if full {
        (0.0, std::f64::consts::PI)
    } else {
        (p_min, p_max)
    };

    // Longitude / colatitude divisions from the sphere radius and the covered
    // spans at the LOD's chord tolerance — a small cap samples far fewer than a
    // full ball. Every circle of latitude is at most radius `r` (the equator).
    let nu = lod.arc_segs(r, theta_hi - theta_lo).max(1);
    let nv = lod.arc_segs(r, phi_hi - phi_lo).max(1);

    for j in 0..nv {
        let phi0 = phi_lo + (phi_hi - phi_lo) * (j as f64 / nv as f64);
        let phi1 = phi_lo + (phi_hi - phi_lo) * ((j + 1) as f64 / nv as f64);

        for i in 0..nu {
            let theta0 = theta_lo + (theta_hi - theta_lo) * (i as f64 / nu as f64);
            let theta1 = theta_lo + (theta_hi - theta_lo) * ((i + 1) as f64 / nu as f64);

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

pub(crate) fn tess_torus_face(
    sat: &SatDocument,
    face: &SatFace,
    torus: &SatTorusSurface,
    lod: LodConfig,
    verts: &mut Vec<[f64; 3]>,
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
    let (theta_start, theta_arc, theta_full) =
        torus_theta_range(sat, face, [cx, cy, cz], axis, major_r, minor_r);
    let theta_total = if theta_full { TAU } else { theta_arc };
    let nu = if theta_full {
        lod.circle_segs(minor_r).max(3)
    } else {
        lod.arc_segs(minor_r, theta_total).max(1)
    };

    // Mesh only the revolution arc the face covers. A partial tube (an open "C")
    // otherwise builds as a full closed ring where the solid is open.
    let (phi_start, phi_arc, full) =
        torus_phi_range(sat, face, [cx, cy, cz], u_dir, v_dir, major_r);
    let phi_total = if full { TAU } else { phi_arc };
    // Along-length divisions from the ring (major) radius and the covered arc at
    // the LOD's chord tolerance — a short arc samples proportionally less.
    let nv = lod.arc_segs(major_r, phi_total).max(2);

    for j in 0..nv {
        let phi0 = phi_start + phi_total * (j as f64 / nv as f64);
        let phi1 = phi_start + phi_total * ((j + 1) as f64 / nv as f64);

        for i in 0..nu {
            let theta0 = theta_start + theta_total * (i as f64 / nu as f64);
            let theta1 = theta_start + theta_total * ((i + 1) as f64 / nu as f64);

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
    // Parametric radial direction at angle phi. Keep this independent from the
    // signed major radius: a zero-major spindle is still well-defined, while
    // normalizing `ring - center` would collapse it and would flip the tube
    // frame whenever the stored major radius is negative.
    let radial = [
        u_dir[0] * phi.cos() + v_dir[0] * phi.sin(),
        u_dir[1] * phi.cos() + v_dir[1] * phi.sin(),
        u_dir[2] * phi.cos() + v_dir[2] * phi.sin(),
    ];
    let ring = [
        cx + major_r * radial[0],
        cy + major_r * radial[1],
        cz + major_r * radial[2],
    ];
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

/// Area-weighted polygon normal via Newell's method. Robust for non-planar or
/// slightly noisy loops; its sign encodes the winding direction.
fn newell_normal(poly: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0f64; 3];
    let len = poly.len();
    for i in 0..len {
        let a = poly[i];
        let b = poly[(i + 1) % len];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
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
