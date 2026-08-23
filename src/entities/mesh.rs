use acadrust::entities::{mesh::Mesh, polygon_mesh::PolygonMesh, Face3D, PolyfaceMesh};
use cadkernel::geom2d::{triangulate, Tolerance};
use cadkernel::space::{polygon, Plane, Vec3 as KernelVec3};
use glam::Vec3;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{parse_f64, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;

/// Triangulate a planar (possibly concave) polygon into a flat triangle-soup
/// (3 vertices per triangle), preserving the polygon's winding. A simple fan
/// from vertex 0 is only valid for convex faces — a concave face (e.g. an
/// L-shaped mesh face) fans into triangles that spill outside the outline. Ear
/// clipping handles both. Falls back to a fan when the polygon is degenerate.
pub(crate) fn triangulate_planar(poly: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![poly[0], poly[1], poly[2]];
    }
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let fan = || {
        let mut out = Vec::new();
        for i in 1..n - 1 {
            out.push(poly[0]);
            out.push(poly[i]);
            out.push(poly[i + 1]);
        }
        out
    };
    // Face normal via Newell's method (robust for near-planar polygons).
    let mut normal = [0.0f64; 3];
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
        normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
        normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let nlen = dot(normal, normal).sqrt();
    if nlen < 1e-12 {
        return fan();
    }
    let normal = [normal[0] / nlen, normal[1] / nlen, normal[2] / nlen];
    // Orthonormal in-plane basis.
    let seed = if normal[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let mut u = cross(seed, normal);
    let ul = dot(u, u).sqrt();
    if ul < 1e-12 {
        return fan();
    }
    u = [u[0] / ul, u[1] / ul, u[2] / ul];
    let v = cross(normal, u);
    let p2: Vec<[f64; 2]> = poly.iter().map(|&p| [dot(p, u), dot(p, v)]).collect();
    // Signed area → winding (CCW when positive in the (u, v) frame).
    let mut area = 0.0;
    for i in 0..n {
        let a = p2[i];
        let b = p2[(i + 1) % n];
        area += a[0] * b[1] - b[0] * a[1];
    }
    let ccw = area > 0.0;
    let tri_area2 = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let in_tri = |p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        let d1 = tri_area2(a, b, p);
        let d2 = tri_area2(b, c, p);
        let d3 = tri_area2(c, a, p);
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    };
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out: Vec<[f64; 3]> = Vec::with_capacity((n - 2) * 3);
    let mut guard = 0usize;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let i0 = idx[(k + m - 1) % m];
            let i1 = idx[k];
            let i2 = idx[(k + 1) % m];
            let (a, b, c) = (p2[i0], p2[i1], p2[i2]);
            let convex = if ccw { tri_area2(a, b, c) > 0.0 } else { tri_area2(a, b, c) < 0.0 };
            if !convex {
                continue;
            }
            let mut contains = false;
            for &j in &idx {
                if j == i0 || j == i1 || j == i2 {
                    continue;
                }
                if in_tri(p2[j], a, b, c) {
                    contains = true;
                    break;
                }
            }
            if contains {
                continue;
            }
            out.push(poly[i0]);
            out.push(poly[i1]);
            out.push(poly[i2]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            // No ear found (self-intersecting / numerically degenerate) — bail
            // to a fan of the remainder rather than loop forever.
            for i in 1..idx.len() - 1 {
                out.push(poly[idx[0]]);
                out.push(poly[idx[i]]);
                out.push(poly[idx[i + 1]]);
            }
            return out;
        }
    }
    if idx.len() == 3 {
        out.push(poly[idx[0]]);
        out.push(poly[idx[1]]);
        out.push(poly[idx[2]]);
    }
    out
}

// ── Face3D ────────────────────────────────────────────────────────────────────

fn v3(v: &acadrust::types::Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn dvec3(v: &acadrust::types::Vector3) -> glam::DVec3 {
    glam::DVec3::new(v.x, v.y, v.z)
}

fn v3f32(v: &acadrust::types::Vector3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

fn face3d_fill(corners: [[f64; 3]; 4]) -> Vec<[f64; 3]> {
    let tolerance = Tolerance::default();
    let mut ring = Vec::with_capacity(4);
    for corner in corners {
        if ring.last().is_none_or(|previous| {
            KernelVec3::from(*previous).distance(KernelVec3::from(corner))
                > tolerance.linear()
        }) {
            ring.push(corner);
        }
    }
    if ring.len() > 1
        && KernelVec3::from(ring[0]).distance(KernelVec3::from(*ring.last().unwrap()))
            <= tolerance.linear()
    {
        ring.pop();
    }
    if ring.len() < 3 {
        return Vec::new();
    }

    let Some(normal) = polygon::normal(&ring) else {
        return Vec::new();
    };
    let axis = (KernelVec3::from(ring[1]) - KernelVec3::from(ring[0])).to_array();
    let Some(plane) = Plane::orthonormal(ring[0], axis, normal) else {
        return Vec::new();
    };
    let projected: Vec<[f64; 2]> = ring
        .iter()
        .filter_map(|&point| plane.project(point))
        .collect();
    if projected.len() != ring.len() {
        return Vec::new();
    }
    let (points, triangles) = triangulate(&projected, &[]);
    let source: Vec<usize> = points
        .iter()
        .filter_map(|point| projected.iter().position(|candidate| candidate == point))
        .collect();
    if source.len() != points.len() {
        return Vec::new();
    }
    triangles
        .into_iter()
        .flat_map(|triangle| triangle.map(|index| ring[source[index]]))
        .collect()
}

impl RenderConvertible for Face3D {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        let p0 = v3(&self.first_corner);
        let p1 = v3(&self.second_corner);
        let p2 = v3(&self.third_corner);
        let p3 = v3(&self.fourth_corner);
        let fill_tris = face3d_fill([p0, p1, p2, p3]);
        let p0f = v3f32(&self.first_corner);
        let p1f = v3f32(&self.second_corner);
        let p2f = v3f32(&self.third_corner);
        let p3f = v3f32(&self.fourth_corner);
        let inv = self.invisible_edges;

        // Add edge as a line segment (separated by NaN from previous edges).
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut add_edge = |a: [f64; 3], b: [f64; 3]| {
            if !pts.is_empty() {
                pts.push([f64::NAN; 3]);
            }
            pts.push(a);
            pts.push(b);
        };

        if !inv.is_first_invisible() {
            add_edge(p0, p1);
        }
        if !inv.is_second_invisible() {
            add_edge(p1, p2);
        }
        if !inv.is_third_invisible() {
            add_edge(p2, p3);
        }
        if !inv.is_fourth_invisible() {
            add_edge(p3, p0);
        }

        if pts.is_empty() {
            // All edges invisible — show a tiny cross at centroid.
            let cx = (p0[0] + p1[0] + p2[0] + p3[0]) / 4.0;
            let cy = (p0[1] + p1[1] + p2[1] + p3[1]) / 4.0;
            let cz = (p0[2] + p1[2] + p2[2] + p3[2]) / 4.0;
            let s = 0.1_f64;
            pts = vec![[cx - s, cy, cz], [cx + s, cy, cz]];
        }

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(pts),
            snap_pts: vec![
                (Vec3::from(p0f).as_dvec3(), SnapHint::Node),
                (Vec3::from(p1f).as_dvec3(), SnapHint::Node),
                (Vec3::from(p2f).as_dvec3(), SnapHint::Node),
                (Vec3::from(p3f).as_dvec3(), SnapHint::Node),
            ],
            tangent_geoms: vec![],
            key_vertices: vec![p0, p1, p2, p3],
            fill_tris,
        })
    }
}

impl Grippable for Face3D {
    fn grips(&self) -> Vec<GripDef> {
        vec![
            square_grip(0, dvec3(&self.first_corner)),
            square_grip(1, dvec3(&self.second_corner)),
            square_grip(2, dvec3(&self.third_corner)),
            square_grip(3, dvec3(&self.fourth_corner)),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        let corner = match grip_id {
            0 => &mut self.first_corner,
            1 => &mut self.second_corner,
            2 => &mut self.third_corner,
            3 => &mut self.fourth_corner,
            _ => return,
        };
        match apply {
            GripApply::Translate(d) => {
                corner.x += d.x as f64;
                corner.y += d.y as f64;
                corner.z += d.z as f64;
            }
            GripApply::Absolute(p) => {
                corner.x = p.x as f64;
                corner.y = p.y as f64;
                corner.z = p.z as f64;
            }
        }
    }
}

impl PropertyEditable for Face3D {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        use crate::entities::common::edit_prop as edit;
        let inv = self.invisible_edges;
        let edge = |hidden: bool| if hidden { "Invisible" } else { "Visible" };
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                ro(t!("Current vertex").as_ref(), "f3_current", String::new()),
                edit(t!("Vertex 1 X").as_ref(), "f3_p1x", self.first_corner.x),
                edit(t!("Vertex 1 Y").as_ref(), "f3_p1y", self.first_corner.y),
                edit(t!("Vertex 1 Z").as_ref(), "f3_p1z", self.first_corner.z),
                edit(t!("Vertex 2 X").as_ref(), "f3_p2x", self.second_corner.x),
                edit(t!("Vertex 2 Y").as_ref(), "f3_p2y", self.second_corner.y),
                edit(t!("Vertex 2 Z").as_ref(), "f3_p2z", self.second_corner.z),
                edit(t!("Vertex 3 X").as_ref(), "f3_p3x", self.third_corner.x),
                edit(t!("Vertex 3 Y").as_ref(), "f3_p3y", self.third_corner.y),
                edit(t!("Vertex 3 Z").as_ref(), "f3_p3z", self.third_corner.z),
                edit(t!("Vertex 4 X").as_ref(), "f3_p4x", self.fourth_corner.x),
                edit(t!("Vertex 4 Y").as_ref(), "f3_p4y", self.fourth_corner.y),
                edit(t!("Vertex 4 Z").as_ref(), "f3_p4z", self.fourth_corner.z),
                ro(t!("Edge 1").as_ref(), "f3_edge1", edge(inv.is_first_invisible())),
                ro(t!("Edge 2").as_ref(), "f3_edge2", edge(inv.is_second_invisible())),
                ro(t!("Edge 3").as_ref(), "f3_edge3", edge(inv.is_third_invisible())),
                ro(t!("Edge 4").as_ref(), "f3_edge4", edge(inv.is_fourth_invisible())),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "f3_p1x" => self.first_corner.x = v,
            "f3_p1y" => self.first_corner.y = v,
            "f3_p1z" => self.first_corner.z = v,
            "f3_p2x" => self.second_corner.x = v,
            "f3_p2y" => self.second_corner.y = v,
            "f3_p2z" => self.second_corner.z = v,
            "f3_p3x" => self.third_corner.x = v,
            "f3_p3y" => self.third_corner.y = v,
            "f3_p3z" => self.third_corner.z = v,
            "f3_p4x" => self.fourth_corner.x = v,
            "f3_p4y" => self.fourth_corner.y = v,
            "f3_p4z" => self.fourth_corner.z = v,
            _ => {}
        }
    }
}

impl Transformable for Face3D {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for corner in [
                &mut entity.first_corner,
                &mut entity.second_corner,
                &mut entity.third_corner,
                &mut entity.fourth_corner,
            ] {
                crate::scene::view::transform::reflect_xy_point(&mut corner.x, &mut corner.y, p1, p2);
            }
        });
    }
}

// ── PolygonMesh (N×M grid) ────────────────────────────────────────────────────

impl RenderConvertible for PolygonMesh {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        let m = self.m_vertex_count as usize;
        let n = self.n_vertex_count as usize;
        if m == 0 || n == 0 || self.vertices.len() < m * n {
            return None;
        }

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(Vec::new()),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        })
    }
}

impl Grippable for PolygonMesh {
    fn grips(&self) -> Vec<GripDef> {
        self.vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                square_grip(
                    i,
                    glam::DVec3::new(v.location.x, v.location.y, v.location.z),
                )
            })
            .collect()
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if let Some(v) = self.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Translate(d) => {
                    v.location.x += d.x as f64;
                    v.location.y += d.y as f64;
                    v.location.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    v.location.x = p.x as f64;
                    v.location.y = p.y as f64;
                    v.location.z = p.z as f64;
                }
            }
        }
    }
}

impl PropertyEditable for PolygonMesh {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let smooth = match self.smooth_type {
            acadrust::entities::polygon_mesh::SurfaceSmoothType::NoSmooth => "None",
            acadrust::entities::polygon_mesh::SurfaceSmoothType::Quadratic => "Quadratic",
            acadrust::entities::polygon_mesh::SurfaceSmoothType::Cubic => "Cubic",
            acadrust::entities::polygon_mesh::SurfaceSmoothType::Bezier => "Bezier",
        };
        let yesno = |b: bool| if b { "Yes" } else { "No" };
        let first = self.vertices.first();
        // Grid faces: one quad per cell; closed direction adds a wrap row/column.
        let m = self.m_vertex_count.max(0) as i64;
        let n = self.n_vertex_count.max(0) as i64;
        let cells_m = if self.is_closed_m() { m } else { (m - 1).max(0) };
        let cells_n = if self.is_closed_n() { n } else { (n - 1).max(0) };
        let face_count = cells_m * cells_n;
        vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    ro(t!("Vertex").as_ref(), "pm_vertex", String::new()),
                    ro(t!("Vertex X").as_ref(),
                        "pm_vx",
                        first.map(|v| format!("{:.4}", v.location.x)).unwrap_or_default(),
                    ),
                    ro(t!("Vertex Y").as_ref(),
                        "pm_vy",
                        first.map(|v| format!("{:.4}", v.location.y)).unwrap_or_default(),
                    ),
                    ro(t!("Vertex Z").as_ref(),
                        "pm_vz",
                        first.map(|v| format!("{:.4}", v.location.z)).unwrap_or_default(),
                    ),
                    ro(t!("M vertex count").as_ref(), "pm_m", self.m_vertex_count.to_string()),
                    ro(t!("N vertex count").as_ref(), "pm_n", self.n_vertex_count.to_string()),
                    ro(t!("M closed").as_ref(), "pm_closed_m", yesno(self.is_closed_m())),
                    ro(t!("N closed").as_ref(), "pm_closed_n", yesno(self.is_closed_n())),
                    ro(t!("M density").as_ref(), "pm_smooth_m", self.m_smooth_density.to_string()),
                    ro(t!("N density").as_ref(), "pm_smooth_n", self.n_smooth_density.to_string()),
                    ro(t!("Vertex count").as_ref(), "pm_v", self.vertices.len().to_string()),
                    ro(t!("Face count").as_ref(), "pm_faces", face_count.to_string()),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![ro(t!("Fit/smooth").as_ref(), "pm_smooth", smooth)],
            },
        ]
    }

    fn apply_geom_prop(&mut self, _field: &str, _value: &str) {}
}

impl Transformable for PolygonMesh {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::view::transform::reflect_xy_point(
                    &mut v.location.x,
                    &mut v.location.y,
                    p1,
                    p2,
                );
            }
        });
    }
}

// ── PolyfaceMesh (arbitrary faces with 1-based vertex indices) ────────────────

impl RenderConvertible for PolyfaceMesh {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        if self.vertices.is_empty() || self.faces.is_empty() {
            return None;
        }

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(Vec::new()),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        })
    }
}

impl Grippable for PolyfaceMesh {
    fn grips(&self) -> Vec<GripDef> {
        self.vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                square_grip(
                    i,
                    glam::DVec3::new(v.location.x, v.location.y, v.location.z),
                )
            })
            .collect()
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if let Some(v) = self.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Translate(d) => {
                    v.location.x += d.x as f64;
                    v.location.y += d.y as f64;
                    v.location.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    v.location.x = p.x as f64;
                    v.location.y = p.y as f64;
                    v.location.z = p.z as f64;
                }
            }
        }
    }
}

impl PropertyEditable for PolyfaceMesh {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let smooth = match self.smooth_surface {
            acadrust::entities::PolyfaceSmoothType::None => "None",
            acadrust::entities::PolyfaceSmoothType::Quadratic => "Quadratic",
            acadrust::entities::PolyfaceSmoothType::Cubic => "Cubic",
            acadrust::entities::PolyfaceSmoothType::Bezier => "Bezier",
        };
        let first = self.vertices.first();
        vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    ro(t!("Vertex").as_ref(), "pfm_vertex", String::new()),
                    ro(t!("Vertex X").as_ref(),
                        "pfm_vx",
                        first.map(|v| format!("{:.4}", v.location.x)).unwrap_or_default(),
                    ),
                    ro(t!("Vertex Y").as_ref(),
                        "pfm_vy",
                        first.map(|v| format!("{:.4}", v.location.y)).unwrap_or_default(),
                    ),
                    ro(t!("Vertex Z").as_ref(),
                        "pfm_vz",
                        first.map(|v| format!("{:.4}", v.location.z)).unwrap_or_default(),
                    ),
                    // Polyface meshes store an explicit vertex/face list rather
                    // than an M×N grid, so the grid-only rows are not applicable.
                    ro(t!("M vertex count").as_ref(), "pfm_m", String::new()),
                    ro(t!("N vertex count").as_ref(), "pfm_n", String::new()),
                    ro(t!("M closed").as_ref(), "pfm_closed_m", String::new()),
                    ro(t!("N closed").as_ref(), "pfm_closed_n", String::new()),
                    ro(t!("M density").as_ref(), "pfm_density_m", String::new()),
                    ro(t!("N density").as_ref(), "pfm_density_n", String::new()),
                    ro(t!("Vertex count").as_ref(), "pfm_v", self.vertices.len().to_string()),
                    ro(t!("Face count").as_ref(), "pfm_f", self.faces.len().to_string()),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![ro(t!("Fit/smooth").as_ref(), "pfm_smooth", smooth)],
            },
        ]
    }

    fn apply_geom_prop(&mut self, _field: &str, _value: &str) {}
}

impl Transformable for PolyfaceMesh {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::view::transform::reflect_xy_point(
                    &mut v.location.x,
                    &mut v.location.y,
                    p1,
                    p2,
                );
            }
        });
    }
}

// ── Mesh (SubD mesh) ──────────────────────────────────────────────────────────
//
// Modern subdivision mesh — distinct from PolygonMesh. The render path emits
// the refined per-edge wireframe and triangulates each face into fill_tris so
// solid views draw the same Catmull-Clark surface described by the DWG.

#[derive(Clone)]
struct RefinedMesh {
    vertices: Vec<[f64; 3]>,
    faces: Vec<Vec<usize>>,
    creases: std::collections::HashMap<(usize, usize), f64>,
}

fn edge_key(a: usize, b: usize) -> (usize, usize) {
    if a < b { (a, b) } else { (b, a) }
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul3(a: [f64; 3], scale: f64) -> [f64; 3] {
    [a[0] * scale, a[1] * scale, a[2] * scale]
}

fn mix3(a: [f64; 3], b: [f64; 3], amount: f64) -> [f64; 3] {
    add3(mul3(a, 1.0 - amount), mul3(b, amount))
}

fn mean_points<'a>(points: impl Iterator<Item = &'a [f64; 3]>) -> [f64; 3] {
    let mut sum = [0.0; 3];
    let mut count = 0usize;
    for point in points {
        sum = add3(sum, *point);
        count += 1;
    }
    if count == 0 {
        sum
    } else {
        mul3(sum, 1.0 / count as f64)
    }
}

fn base_refined_mesh(mesh: &Mesh) -> RefinedMesh {
    let vertices: Vec<[f64; 3]> = mesh.vertices.iter().map(|v| [v.x, v.y, v.z]).collect();
    let faces = mesh
        .faces
        .iter()
        .filter_map(|face| {
            let valid: Vec<usize> = face
                .vertices
                .iter()
                .copied()
                .filter(|&index| index < vertices.len())
                .collect();
            (valid.len() >= 3).then_some(valid)
        })
        .collect();
    let creases = mesh
        .edges
        .iter()
        .filter_map(|edge| {
            let sharpness = edge.crease?;
            (edge.start < vertices.len() && edge.end < vertices.len() && sharpness > 0.0)
                .then_some((edge_key(edge.start, edge.end), sharpness))
        })
        .collect();
    RefinedMesh {
        vertices,
        faces,
        creases,
    }
}

fn subdivide_catmull_clark(mesh: &RefinedMesh, blend_crease: bool) -> RefinedMesh {
    use std::collections::{HashMap, HashSet};

    if mesh.faces.is_empty() {
        return mesh.clone();
    }

    let face_points: Vec<[f64; 3]> = mesh
        .faces
        .iter()
        .map(|face| mean_points(face.iter().filter_map(|&index| mesh.vertices.get(index))))
        .collect();

    let mut edge_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    let mut vertex_faces: Vec<Vec<usize>> = vec![Vec::new(); mesh.vertices.len()];
    let mut vertex_edges: Vec<HashSet<(usize, usize)>> = vec![HashSet::new(); mesh.vertices.len()];
    for (face_index, face) in mesh.faces.iter().enumerate() {
        for &vertex in face {
            if let Some(faces) = vertex_faces.get_mut(vertex) {
                faces.push(face_index);
            }
        }
        for corner in 0..face.len() {
            let a = face[corner];
            let b = face[(corner + 1) % face.len()];
            if a == b {
                continue;
            }
            let key = edge_key(a, b);
            edge_faces.entry(key).or_default().push(face_index);
            if let Some(edges) = vertex_edges.get_mut(a) {
                edges.insert(key);
            }
            if let Some(edges) = vertex_edges.get_mut(b) {
                edges.insert(key);
            }
        }
    }

    let mut vertex_points = Vec::with_capacity(mesh.vertices.len());
    for (vertex_index, &point) in mesh.vertices.iter().enumerate() {
        let incident_edges = &vertex_edges[vertex_index];
        let incident_faces = &vertex_faces[vertex_index];
        if incident_edges.is_empty() || incident_faces.is_empty() {
            vertex_points.push(point);
            continue;
        }

        let face_average =
            mean_points(incident_faces.iter().filter_map(|&index| face_points.get(index)));
        let edge_midpoints: Vec<[f64; 3]> = incident_edges
            .iter()
            .filter_map(|&(a, b)| {
                let other = if a == vertex_index { b } else { a };
                mesh.vertices
                    .get(other)
                    .map(|&p| mul3(add3(point, p), 0.5))
            })
            .collect();
        let edge_average = mean_points(edge_midpoints.iter());
        let n = incident_faces.len() as f64;
        let smooth = mul3(
            add3(add3(face_average, mul3(edge_average, 2.0)), mul3(point, n - 3.0)),
            1.0 / n,
        );

        let mut sharp_neighbours: Vec<(f64, [f64; 3])> = incident_edges
            .iter()
            .filter_map(|&key| {
                let boundary = edge_faces.get(&key).is_some_and(|faces| faces.len() == 1);
                let sharpness = if boundary {
                    f64::INFINITY
                } else {
                    mesh.creases.get(&key).copied().unwrap_or(0.0)
                };
                if sharpness <= 0.0 {
                    return None;
                }
                let other = if key.0 == vertex_index { key.1 } else { key.0 };
                mesh.vertices.get(other).copied().map(|p| (sharpness, p))
            })
            .collect();
        sharp_neighbours.sort_by(|a, b| b.0.total_cmp(&a.0));

        let crease_point = match sharp_neighbours.as_slice() {
            [(_, first), (_, second), ..] => mul3(
                add3(add3(mul3(point, 6.0), *first), *second),
                1.0 / 8.0,
            ),
            _ => smooth,
        };
        let corner_point = if sharp_neighbours.len() >= 3 {
            point
        } else {
            crease_point
        };
        let sharpness = sharp_neighbours
            .get(1)
            .map(|item| item.0)
            .unwrap_or(0.0);
        let amount = if blend_crease {
            sharpness.clamp(0.0, 1.0)
        } else if sharpness > 0.0 {
            1.0
        } else {
            0.0
        };
        vertex_points.push(mix3(smooth, corner_point, amount));
    }

    let mut edge_point_indices = HashMap::with_capacity(edge_faces.len());
    let mut vertices = vertex_points;
    for (&key, adjacent_faces) in &edge_faces {
        let Some((&a, &b)) = mesh.vertices.get(key.0).zip(mesh.vertices.get(key.1)) else {
            continue;
        };
        let midpoint = mul3(add3(a, b), 0.5);
        let smooth = if adjacent_faces.len() == 2 {
            let f0 = face_points[adjacent_faces[0]];
            let f1 = face_points[adjacent_faces[1]];
            mul3(add3(add3(a, b), add3(f0, f1)), 0.25)
        } else {
            midpoint
        };
        let boundary = adjacent_faces.len() != 2;
        let sharpness = mesh.creases.get(&key).copied().unwrap_or(0.0);
        let amount = if boundary {
            1.0
        } else if blend_crease {
            sharpness.clamp(0.0, 1.0)
        } else if sharpness > 0.0 {
            1.0
        } else {
            0.0
        };
        edge_point_indices.insert(key, vertices.len());
        vertices.push(mix3(smooth, midpoint, amount));
    }

    let face_point_start = vertices.len();
    vertices.extend(face_points.iter().copied());
    let mut faces = Vec::new();
    let mut creases = HashMap::new();
    for (face_index, face) in mesh.faces.iter().enumerate() {
        for corner in 0..face.len() {
            let vertex = face[corner];
            let next_key = edge_key(vertex, face[(corner + 1) % face.len()]);
            let prev_key = edge_key(face[(corner + face.len() - 1) % face.len()], vertex);
            let (Some(&next_edge), Some(&prev_edge)) = (
                edge_point_indices.get(&next_key),
                edge_point_indices.get(&prev_key),
            ) else {
                continue;
            };
            faces.push(vec![
                vertex,
                next_edge,
                face_point_start + face_index,
                prev_edge,
            ]);
        }
    }

    for (&key, &sharpness) in &mesh.creases {
        let Some(&edge_point) = edge_point_indices.get(&key) else {
            continue;
        };
        let child_sharpness = (sharpness - 1.0).max(0.0);
        if child_sharpness > 0.0 {
            creases.insert(edge_key(key.0, edge_point), child_sharpness);
            creases.insert(edge_key(edge_point, key.1), child_sharpness);
        }
    }

    RefinedMesh {
        vertices,
        faces,
        creases,
    }
}

fn display_mesh(mesh: &Mesh) -> RefinedMesh {
    let mut refined = base_refined_mesh(mesh);
    for _ in 0..mesh.subdivision_level.clamp(0, 4) {
        refined = subdivide_catmull_clark(&refined, mesh.blend_crease);
    }
    refined
}

fn face_triangle_indices(
    vertices: &[[f64; 3]],
    faces: &[Vec<usize>],
) -> (Vec<u32>, Vec<usize>) {
    let mut indices = Vec::new();
    let mut triangle_faces = Vec::new();
    for (face_index, face) in faces.iter().enumerate() {
        let polygon: Vec<[f64; 3]> = face
            .iter()
            .filter_map(|&index| vertices.get(index).copied())
            .collect();
        if polygon.len() < 3 {
            continue;
        }
        for triangle in triangulate_planar(&polygon).chunks_exact(3) {
            let mut mapped = [0u32; 3];
            let mut valid = true;
            for corner in 0..3 {
                let Some(local) = polygon.iter().position(|point| *point == triangle[corner]) else {
                    valid = false;
                    break;
                };
                mapped[corner] = face[local] as u32;
            }
            if valid && mapped[0] != mapped[1] && mapped[1] != mapped[2] && mapped[2] != mapped[0] {
                indices.extend(mapped);
                triangle_faces.push(face_index);
            }
        }
    }
    (indices, triangle_faces)
}

fn vertex_normals(vertices: &[[f64; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f64; 3]; vertices.len()];
    for triangle in indices.chunks_exact(3) {
        let (Some(&a), Some(&b), Some(&c)) = (
            vertices.get(triangle[0] as usize),
            vertices.get(triangle[1] as usize),
            vertices.get(triangle[2] as usize),
        ) else {
            continue;
        };
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let normal = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        for &index in triangle {
            if let Some(sum) = normals.get_mut(index as usize) {
                *sum = add3(*sum, normal);
            }
        }
    }
    normals
        .into_iter()
        .map(|normal| {
            let length =
                (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            if length > 1e-15 {
                [
                    (normal[0] / length) as f32,
                    (normal[1] / length) as f32,
                    (normal[2] / length) as f32,
                ]
            } else {
                [0.0, 0.0, 1.0]
            }
        })
        .collect()
}

fn append_feature_edge(
    edge_verts: &mut Vec<[f32; 3]>,
    edge_verts_low: &mut Vec<[f32; 3]>,
    vertices: &[[f64; 3]],
    a: usize,
    b: usize,
) {
    for index in [a, b] {
        let Some(&[x, y, z]) = vertices.get(index) else {
            continue;
        };
        let high = [x as f32, y as f32, z as f32];
        edge_verts.push(high);
        edge_verts_low.push([
            (x - high[0] as f64) as f32,
            (y - high[1] as f64) as f32,
            (z - high[2] as f64) as f32,
        ]);
    }
}

fn make_mesh_lod_set(
    name: String,
    color: [f32; 4],
    vertices: Vec<[f64; 3]>,
    faces: Vec<Vec<usize>>,
    visible_edges: std::collections::HashSet<(usize, usize)>,
    face_colors: &[Option<[f32; 4]>],
) -> Option<crate::scene::model::mesh_model::MeshLodSet> {
    let (indices, triangle_faces) = face_triangle_indices(&vertices, &faces);
    if indices.is_empty() {
        return None;
    }
    let normals = vertex_normals(&vertices, &indices);
    let triangle_colors = triangle_faces
        .iter()
        .map(|&face| face_colors.get(face).copied().flatten())
        .collect();
    let model = crate::scene::convert::solid3d_tess::finalize_mesh(
        name,
        vertices.clone(),
        normals,
        indices,
        Vec::new(),
        triangle_colors,
        color,
        None,
    );
    let mut set = crate::scene::model::mesh_model::MeshLodSet::from_lods(vec![model]);
    for (a, b) in visible_edges {
        append_feature_edge(
            &mut set.edge_verts,
            &mut set.edge_verts_low,
            &vertices,
            a,
            b,
        );
    }
    Some(set)
}

/// Convert every standard DWG mesh family to the material-aware shaded mesh
/// pipeline. The retained wire entity carries snaps/grips only; fill, normals,
/// depth, materials and face colours live in `MeshLodSet`.
pub(crate) fn tessellate_shaded_mesh(
    entity: &acadrust::EntityType,
    color: [f32; 4],
) -> Option<crate::scene::model::mesh_model::MeshLodSet> {
    use std::collections::HashSet;

    match entity {
        acadrust::EntityType::Mesh(mesh) => {
            let display = display_mesh(mesh);
            let mut edges = HashSet::new();
            for face in &display.faces {
                for corner in 0..face.len() {
                    edges.insert(edge_key(face[corner], face[(corner + 1) % face.len()]));
                }
            }
            make_mesh_lod_set(
                mesh.common.handle.value().to_string(),
                color,
                display.vertices,
                display.faces,
                edges,
                &[],
            )
        }
        acadrust::EntityType::PolygonMesh(mesh) => {
            let m = mesh.m_vertex_count.max(0) as usize;
            let n = mesh.n_vertex_count.max(0) as usize;
            if m == 0 || n == 0 || mesh.vertices.len() < m.saturating_mul(n) {
                return None;
            }
            let vertices: Vec<[f64; 3]> = mesh
                .vertices
                .iter()
                .take(m * n)
                .map(|vertex| {
                    [
                        vertex.location.x,
                        vertex.location.y,
                        vertex.location.z,
                    ]
                })
                .collect();
            let closed_m = mesh.is_closed_m();
            let closed_n = mesh.is_closed_n();
            let m_cells = if closed_m { m } else { m.saturating_sub(1) };
            let n_cells = if closed_n { n } else { n.saturating_sub(1) };
            let mut faces = Vec::with_capacity(m_cells.saturating_mul(n_cells));
            let mut edges = HashSet::new();
            for i in 0..m_cells {
                for j in 0..n_cells {
                    let face = vec![
                        i * n + j,
                        ((i + 1) % m) * n + j,
                        ((i + 1) % m) * n + (j + 1) % n,
                        i * n + (j + 1) % n,
                    ];
                    for corner in 0..4 {
                        edges.insert(edge_key(face[corner], face[(corner + 1) % 4]));
                    }
                    faces.push(face);
                }
            }
            make_mesh_lod_set(
                mesh.common.handle.value().to_string(),
                color,
                vertices,
                faces,
                edges,
                &[],
            )
        }
        acadrust::EntityType::PolyfaceMesh(mesh) => {
            let vertices: Vec<[f64; 3]> = mesh
                .vertices
                .iter()
                .map(|vertex| {
                    [
                        vertex.location.x,
                        vertex.location.y,
                        vertex.location.z,
                    ]
                })
                .collect();
            let mut faces = Vec::new();
            let mut face_colors = Vec::new();
            let mut edges = HashSet::new();
            for face in &mesh.faces {
                let raw = [face.index1, face.index2, face.index3, face.index4];
                let indices: Vec<usize> = raw
                    .iter()
                    .filter(|&&index| index != 0)
                    .filter_map(|&index| (index.unsigned_abs() as usize).checked_sub(1))
                    .filter(|&index| index < vertices.len())
                    .collect();
                if indices.len() < 3 {
                    continue;
                }
                for corner in 0..indices.len() {
                    if raw[corner] > 0 {
                        edges.insert(edge_key(indices[corner], indices[(corner + 1) % indices.len()]));
                    }
                }
                faces.push(indices);
                face_colors.push(face.color.as_ref().map(crate::scene::convert::tess_util::aci_to_rgba));
            }
            make_mesh_lod_set(
                mesh.common.handle.value().to_string(),
                color,
                vertices,
                faces,
                edges,
                &face_colors,
            )
        }
        _ => None,
    }
}

impl RenderConvertible for Mesh {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        if self.vertices.is_empty() {
            return None;
        }

        // Per-vertex snap tables cost ~56 B/vertex and are retained in every
        // copy of the wire set. On NWD-scale imports (#358: tens of millions
        // of mesh vertices) that is GBs for snap targets far too dense to
        // pick apart on screen — so past this size ship none, which is what
        // PolyfaceMesh / PolygonMesh already do at any size.
        const SNAP_TABLE_MAX_VERTICES: usize = 50_000;
        let (snap_pts, key_vertices) = if self.vertices.len() > SNAP_TABLE_MAX_VERTICES {
            (Vec::new(), Vec::new())
        } else {
            (
                self.vertices
                    .iter()
                    .map(|v| (glam::DVec3::new(v.x, v.y, v.z), SnapHint::Node))
                    .collect(),
                self.vertices.iter().map(|v| [v.x, v.y, v.z]).collect(),
            )
        };

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(Vec::new()),
            snap_pts,
            tangent_geoms: vec![],
            key_vertices,
            fill_tris: vec![],
        })
    }
}

impl Grippable for Mesh {
    fn grips(&self) -> Vec<GripDef> {
        Vec::new()
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if let Some(v) = self.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Translate(d) => {
                    v.x += d.x as f64;
                    v.y += d.y as f64;
                    v.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    v.x = p.x as f64;
                    v.y = p.y as f64;
                    v.z = p.z as f64;
                }
            }
        }
    }
}

impl PropertyEditable for Mesh {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        // Watertight when every face edge is shared by exactly two faces
        // (closed manifold). Empty meshes are not watertight.
        let mut edge_use: std::collections::HashMap<(usize, usize), u32> =
            std::collections::HashMap::new();
        for face in &self.faces {
            let vs = &face.vertices;
            for i in 0..vs.len() {
                let a = vs[i];
                let b = vs[(i + 1) % vs.len()];
                let key = if a < b { (a, b) } else { (b, a) };
                *edge_use.entry(key).or_insert(0) += 1;
            }
        }
        let watertight =
            !self.faces.is_empty() && edge_use.values().all(|&c| c == 2);
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                Property {
                    label: t!("Level of Smoothness").into_owned(),
                    field: "msh_subdiv_edit",
                    value: PropValue::EditText(self.subdivision_level.to_string()),
                },
                Property {
                    label: t!("Blend Creases").into_owned(),
                    field: "msh_blend_crease",
                    value: PropValue::BoolToggle {
                        field: "msh_blend_crease",
                        value: self.blend_crease,
                    },
                },
                ro(t!("Number of Faces").as_ref(), "msh_f", self.faces.len().to_string()),
                ro(t!("Number of Vertices").as_ref(), "msh_v", self.vertices.len().to_string()),
                ro(t!("Number of Edges").as_ref(), "msh_e", self.edges.len().to_string()),
                ro(t!("Creased Edges").as_ref(),
                    "msh_creased",
                    self.edges
                        .iter()
                        .filter(|edge| edge.crease.is_some_and(|value| value > 0.0))
                        .count()
                        .to_string(),
                ),
                ro(t!("Override Option").as_ref(),
                    "msh_override",
                    self.override_option.to_string(),
                ),
                ro(t!("Number of Grips").as_ref(), "msh_grips", self.vertices.len().to_string()),
                ro(t!("Watertight").as_ref(),
                    "msh_watertight",
                    if watertight { "Yes" } else { "No" },
                ),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "msh_subdiv_edit" => {
                if let Some(value) = parse_f64(value) {
                    self.subdivision_level = (value.round() as i32).clamp(0, 4);
                }
            }
            "msh_blend_crease" => {
                self.blend_crease = match value {
                    "toggle" => !self.blend_crease,
                    "On" | "Yes" | "true" | "1" => true,
                    "Off" | "No" | "false" | "0" => false,
                    _ => self.blend_crease,
                };
            }
            _ => {}
        }
    }
}

impl Transformable for Mesh {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::view::transform::reflect_xy_point(&mut v.x, &mut v.y, p1, p2);
            }
        });
    }
}
