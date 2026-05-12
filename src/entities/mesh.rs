use acadrust::entities::{polygon_mesh::PolygonMesh, Face3D, PolyfaceMesh};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::SnapHint;

// ── Face3D ────────────────────────────────────────────────────────────────────

fn v3(v: &acadrust::types::Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn v3f32(v: &acadrust::types::Vector3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

impl TruckConvertible for Face3D {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        let p0 = v3(&self.first_corner);
        let p1 = v3(&self.second_corner);
        let p2 = v3(&self.third_corner);
        let p3 = v3(&self.fourth_corner);
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

        Some(TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts: vec![
                (Vec3::from(p0f), SnapHint::Node),
                (Vec3::from(p1f), SnapHint::Node),
                (Vec3::from(p2f), SnapHint::Node),
                (Vec3::from(p3f), SnapHint::Node),
            ],
            tangent_geoms: vec![],
            key_vertices: vec![p0, p1, p2, p3],
            fill_tris: vec![],
        })
    }
}

impl Grippable for Face3D {
    fn grips(&self) -> Vec<GripDef> {
        vec![
            square_grip(0, Vec3::from(v3f32(&self.first_corner))),
            square_grip(1, Vec3::from(v3f32(&self.second_corner))),
            square_grip(2, Vec3::from(v3f32(&self.third_corner))),
            square_grip(3, Vec3::from(v3f32(&self.fourth_corner))),
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
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        use crate::entities::common::edit_prop as edit;
        PropSection {
            title: "Geometry".into(),
            props: vec![
                edit("P1 X", "f3_p1x", self.first_corner.x),
                edit("P1 Y", "f3_p1y", self.first_corner.y),
                edit("P1 Z", "f3_p1z", self.first_corner.z),
                edit("P2 X", "f3_p2x", self.second_corner.x),
                edit("P2 Y", "f3_p2y", self.second_corner.y),
                edit("P2 Z", "f3_p2z", self.second_corner.z),
                edit("P3 X", "f3_p3x", self.third_corner.x),
                edit("P3 Y", "f3_p3y", self.third_corner.y),
                edit("P3 Z", "f3_p3z", self.third_corner.z),
                edit("P4 X", "f3_p4x", self.fourth_corner.x),
                edit("P4 Y", "f3_p4y", self.fourth_corner.y),
                edit("P4 Z", "f3_p4z", self.fourth_corner.z),
            ],
        }
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
        crate::scene::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for corner in [
                &mut entity.first_corner,
                &mut entity.second_corner,
                &mut entity.third_corner,
                &mut entity.fourth_corner,
            ] {
                crate::scene::transform::reflect_xy_point(&mut corner.x, &mut corner.y, p1, p2);
            }
        });
    }
}

// ── PolygonMesh (N×M grid) ────────────────────────────────────────────────────

impl TruckConvertible for PolygonMesh {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        let m = self.m_vertex_count as usize;
        let n = self.n_vertex_count as usize;
        if m == 0 || n == 0 || self.vertices.len() < m * n {
            return None;
        }

        let closed_m = self
            .flags
            .contains(acadrust::entities::PolygonMeshFlags::CLOSED_M);
        let closed_n = self
            .flags
            .contains(acadrust::entities::PolygonMeshFlags::CLOSED_N);

        let pt = |i: usize, j: usize| -> [f64; 3] {
            let v = &self.vertices[i * n + j];
            [v.location.x, v.location.y, v.location.z]
        };

        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut fill_tris: Vec<[f64; 3]> = Vec::new();

        // Rows (M direction).
        for i in 0..m {
            pts.push([f64::NAN; 3]);
            for j in 0..n {
                pts.push(pt(i, j));
            }
            if closed_n {
                pts.push(pt(i, 0));
            }
        }

        // Columns (N direction).
        for j in 0..n {
            pts.push([f64::NAN; 3]);
            for i in 0..m {
                pts.push(pt(i, j));
            }
            if closed_m {
                pts.push(pt(0, j));
            }
        }

        // Fill: triangulate each grid quad (two triangles per cell).
        let mi = if closed_m { m } else { m - 1 };
        let ni = if closed_n { n } else { n - 1 };
        for i in 0..mi {
            for j in 0..ni {
                let p00 = pt(i, j);
                let p10 = pt((i + 1) % m, j);
                let p01 = pt(i, (j + 1) % n);
                let p11 = pt((i + 1) % m, (j + 1) % n);
                fill_tris.extend_from_slice(&[p00, p10, p11, p00, p11, p01]);
            }
        }

        Some(TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris,
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
                    Vec3::new(
                        v.location.x as f32,
                        v.location.y as f32,
                        v.location.z as f32,
                    ),
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
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        PropSection {
            title: "Geometry".into(),
            props: vec![
                ro("M count", "pm_m", self.m_vertex_count.to_string()),
                ro("N count", "pm_n", self.n_vertex_count.to_string()),
                ro("Vertices", "pm_v", self.vertices.len().to_string()),
            ],
        }
    }

    fn apply_geom_prop(&mut self, _field: &str, _value: &str) {}
}

impl Transformable for PolygonMesh {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::transform::reflect_xy_point(
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

impl TruckConvertible for PolyfaceMesh {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        if self.vertices.is_empty() || self.faces.is_empty() {
            return None;
        }

        let get_v = |idx: i16| -> Option<[f64; 3]> {
            let i = (idx.abs() as usize).checked_sub(1)?;
            let v = self.vertices.get(i)?;
            Some([v.location.x, v.location.y, v.location.z])
        };

        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut fill_tris: Vec<[f64; 3]> = Vec::new();

        for face in &self.faces {
            // Indices: 0 means unused. Negative = invisible edge (still render for wireframe).
            let indices = [face.index1, face.index2, face.index3, face.index4];
            let verts: Vec<[f64; 3]> = indices
                .iter()
                .filter(|&&i| i != 0)
                .filter_map(|&i| get_v(i))
                .collect();

            if verts.len() < 2 {
                continue;
            }
            pts.push([f64::NAN; 3]);
            for &p in &verts {
                pts.push(p);
            }
            // Close the face polygon.
            pts.push(verts[0]);

            // Fan-triangulate the face for solid fill.
            if verts.len() >= 3 {
                for i in 1..(verts.len() - 1) {
                    fill_tris.push(verts[0]);
                    fill_tris.push(verts[i]);
                    fill_tris.push(verts[i + 1]);
                }
            }
        }

        Some(TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris,
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
                    Vec3::new(
                        v.location.x as f32,
                        v.location.y as f32,
                        v.location.z as f32,
                    ),
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
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        PropSection {
            title: "Geometry".into(),
            props: vec![
                ro("Vertices", "pfm_v", self.vertices.len().to_string()),
                ro("Faces", "pfm_f", self.faces.len().to_string()),
            ],
        }
    }

    fn apply_geom_prop(&mut self, _field: &str, _value: &str) {}
}

impl Transformable for PolyfaceMesh {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::transform::reflect_xy_point(
                    &mut v.location.x,
                    &mut v.location.y,
                    p1,
                    p2,
                );
            }
        });
    }
}
