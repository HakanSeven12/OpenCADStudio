use acadrust::entities::{RasterImage, Wipeout};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{diamond_grip, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection, PropValue, Property};

// ── Shared geometry helpers ───────────────────────────────────────────────────

/// Compute the four world-space corners of an image/wipeout from its
/// insertion_point, u_vector, v_vector and pixel size.
///
/// Returns (p0, p1, p2, p3) in counter-clockwise order:
///   p0 = origin
///   p1 = origin + U*W
///   p2 = origin + U*W + V*H
///   p3 = origin + V*H
fn image_corners(
    origin: &acadrust::types::Vector3,
    u: &acadrust::types::Vector3,
    v: &acadrust::types::Vector3,
    w: f64,
    h: f64,
) -> [[f64; 3]; 4] {
    let ox = origin.x;
    let oy = origin.y;
    let oz = origin.z;
    let ux = u.x * w;
    let uy = u.y * w;
    let uz = u.z * w;
    let vx = v.x * h;
    let vy = v.y * h;
    let vz = v.z * h;

    [
        [ox, oy, oz],
        [ox + ux, oy + uy, oz + uz],
        [ox + ux + vx, oy + uy + vy, oz + uz + vz],
        [ox + vx, oy + vy, oz + vz],
    ]
}

fn to_f32_3(p: [f64; 3]) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

/// Rectangle border + X diagonals — used as a placeholder for images.
fn image_wire(corners: [[f64; 3]; 4], with_x: bool) -> Vec<[f64; 3]> {
    let [p0, p1, p2, p3] = corners;
    let mut pts = vec![p0, p1, p2, p3, p0];
    if with_x {
        pts.push([f64::NAN; 3]);
        pts.push(p0);
        pts.push(p2);
        pts.push([f64::NAN; 3]);
        pts.push(p1);
        pts.push(p3);
    }
    pts
}

fn reflect_vec3(vx: &mut f64, vy: &mut f64, ax: f64, ay: f64, len2: f64) {
    let dot = *vx * ax + *vy * ay;
    *vx = 2.0 * dot * ax / len2 - *vx;
    *vy = 2.0 * dot * ay / len2 - *vy;
}

// ── RasterImage ───────────────────────────────────────────────────────────────

impl TruckConvertible for RasterImage {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        let corners = image_corners(
            &self.insertion_point,
            &self.u_vector,
            &self.v_vector,
            self.size.x,
            self.size.y,
        );

        // Helper: pixel-space → world-space point.
        let ox = self.insertion_point.x;
        let oy = self.insertion_point.y;
        let oz = self.insertion_point.z;
        let px_to_world = |px: f64, py: f64| -> [f64; 3] {
            [
                ox + self.u_vector.x * px + self.v_vector.x * py,
                oy + self.u_vector.y * px + self.v_vector.y * py,
                oz + self.u_vector.z * px + self.v_vector.z * py,
            ]
        };

        let pts = if self.clipping_enabled {
            let cb = &self.clip_boundary;
            match cb.clip_type {
                acadrust::entities::ClipType::Polygonal if cb.vertices.len() >= 3 => {
                    let mut poly: Vec<[f64; 3]> =
                        cb.vertices.iter().map(|v| px_to_world(v.x, v.y)).collect();
                    if let Some(&first) = poly.first() {
                        poly.push(first);
                    }
                    poly
                }
                acadrust::entities::ClipType::Rectangular if cb.vertices.len() >= 2 => {
                    let v0 = &cb.vertices[0];
                    let v1 = &cb.vertices[1];
                    let (xa, xb) = (v0.x.min(v1.x), v0.x.max(v1.x));
                    let (ya, yb) = (v0.y.min(v1.y), v0.y.max(v1.y));
                    let c0 = px_to_world(xa, ya);
                    let c1 = px_to_world(xb, ya);
                    let c2 = px_to_world(xb, yb);
                    let c3 = px_to_world(xa, yb);
                    vec![c0, c1, c2, c3, c0]
                }
                _ => image_wire(corners, true),
            }
        } else {
            image_wire(corners, true)
        };

        Some(TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: corners.to_vec(),
            fill_tris: vec![],
        })
    }
}

impl Grippable for RasterImage {
    fn grips(&self) -> Vec<GripDef> {
        let corners = image_corners(
            &self.insertion_point,
            &self.u_vector,
            &self.v_vector,
            self.size.x,
            self.size.y,
        );
        vec![
            square_grip(0, Vec3::from(to_f32_3(corners[0]))),
            diamond_grip(1, Vec3::from(to_f32_3(corners[1]))),
            diamond_grip(2, Vec3::from(to_f32_3(corners[2]))),
            diamond_grip(3, Vec3::from(to_f32_3(corners[3]))),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id == 0 {
            match apply {
                GripApply::Translate(d) => {
                    self.insertion_point.x += d.x as f64;
                    self.insertion_point.y += d.y as f64;
                    self.insertion_point.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    self.insertion_point.x = p.x as f64;
                    self.insertion_point.y = p.y as f64;
                    self.insertion_point.z = p.z as f64;
                }
            }
        }
        // Corner grips 1-3 are display-only (resizing changes u/v vectors,
        // which requires careful normalization — deferred).
    }
}

impl PropertyEditable for RasterImage {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        PropSection {
            title: "Geometry".into(),
            props: vec![
                ro("File", "ri_file", self.file_path.clone()),
                edit("Insert X", "ri_ox", self.insertion_point.x),
                edit("Insert Y", "ri_oy", self.insertion_point.y),
                edit("Insert Z", "ri_oz", self.insertion_point.z),
                edit("Brightness", "ri_bright", self.brightness as f64),
                edit("Contrast", "ri_contrast", self.contrast as f64),
                edit("Fade", "ri_fade", self.fade as f64),
                Property {
                    label: "Clipping".into(),
                    field: "ri_clip",
                    value: PropValue::BoolToggle {
                        field: "ri_clip",
                        value: self.clipping_enabled,
                    },
                },
            ],
        }
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "ri_clip" => {
                self.clipping_enabled = if value == "toggle" {
                    !self.clipping_enabled
                } else {
                    value == "true"
                };
                return;
            }
            _ => {}
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "ri_ox" => self.insertion_point.x = v,
            "ri_oy" => self.insertion_point.y = v,
            "ri_oz" => self.insertion_point.z = v,
            "ri_bright" => self.brightness = v.clamp(0.0, 100.0) as u8,
            "ri_contrast" => self.contrast = v.clamp(0.0, 100.0) as u8,
            "ri_fade" => self.fade = v.clamp(0.0, 100.0) as u8,
            _ => {}
        }
    }
}

impl Transformable for RasterImage {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            crate::scene::transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
            let ax = (p2.x - p1.x) as f64;
            let ay = (p2.y - p1.y) as f64;
            let len2 = ax * ax + ay * ay;
            if len2 > 1e-12 {
                reflect_vec3(&mut entity.u_vector.x, &mut entity.u_vector.y, ax, ay, len2);
                reflect_vec3(&mut entity.v_vector.x, &mut entity.v_vector.y, ax, ay, len2);
            }
        });
    }
}

// ── Wipeout ───────────────────────────────────────────────────────────────────

impl TruckConvertible for Wipeout {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        let corners = image_corners(
            &self.insertion_point,
            &self.u_vector,
            &self.v_vector,
            self.size.x,
            self.size.y,
        );

        // If clipping is enabled and there's a polygon boundary, show that.
        let pts = if self.clipping_enabled
            && self.clip_boundary_vertices.len() >= 3
            && matches!(
                self.clip_type,
                acadrust::entities::WipeoutClipType::Polygonal
            ) {
            // Convert pixel-space boundary vertices to world space:
            // world = insertion_point + u_vector * v.x * size.x + v_vector * v.y * size.y
            let ox = self.insertion_point.x;
            let oy = self.insertion_point.y;
            let oz = self.insertion_point.z;
            let mut poly: Vec<[f64; 3]> = self
                .clip_boundary_vertices
                .iter()
                .map(|v| {
                    let wx = self.u_vector.x * v.x * self.size.x
                        + self.v_vector.x * v.y * self.size.y;
                    let wy = self.u_vector.y * v.x * self.size.x
                        + self.v_vector.y * v.y * self.size.y;
                    let wz = self.u_vector.z * v.x * self.size.x
                        + self.v_vector.z * v.y * self.size.y;
                    [ox + wx, oy + wy, oz + wz]
                })
                .collect();
            // Close the polygon.
            if let Some(&first) = poly.first() {
                poly.push(first);
            }
            poly
        } else {
            // Rectangular boundary — just the border, no diagonals (mask area).
            image_wire(corners, false)
        };

        Some(TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: corners.to_vec(),
            fill_tris: vec![],
        })
    }
}

impl Grippable for Wipeout {
    fn grips(&self) -> Vec<GripDef> {
        // If polygonal clipping is active, expose individual polygon vertices as grips.
        let is_polygon = self.clipping_enabled
            && self.clip_boundary_vertices.len() >= 3
            && matches!(
                self.clip_type,
                acadrust::entities::WipeoutClipType::Polygonal
            );

        if is_polygon {
            let ox = self.insertion_point.x as f32;
            let oy = self.insertion_point.y as f32;
            let oz = self.insertion_point.z as f32;
            self.clip_boundary_vertices
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let wx = (self.u_vector.x * v.x * self.size.x
                        + self.v_vector.x * v.y * self.size.y) as f32;
                    let wy = (self.u_vector.y * v.x * self.size.x
                        + self.v_vector.y * v.y * self.size.y) as f32;
                    let wz = (self.u_vector.z * v.x * self.size.x
                        + self.v_vector.z * v.y * self.size.y) as f32;
                    if i == 0 {
                        square_grip(i, Vec3::new(ox + wx, oy + wy, oz + wz))
                    } else {
                        diamond_grip(i, Vec3::new(ox + wx, oy + wy, oz + wz))
                    }
                })
                .collect()
        } else {
            let corners = image_corners(
                &self.insertion_point,
                &self.u_vector,
                &self.v_vector,
                self.size.x,
                self.size.y,
            );
            vec![
                square_grip(0, Vec3::from(to_f32_3(corners[0]))),
                diamond_grip(1, Vec3::from(to_f32_3(corners[1]))),
                diamond_grip(2, Vec3::from(to_f32_3(corners[2]))),
                diamond_grip(3, Vec3::from(to_f32_3(corners[3]))),
            ]
        }
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        let is_polygon = self.clipping_enabled
            && self.clip_boundary_vertices.len() >= 3
            && matches!(
                self.clip_type,
                acadrust::entities::WipeoutClipType::Polygonal
            );

        if is_polygon {
            // Move the clicked polygon vertex in world space → back-project to pixel space.
            if let Some(v) = self.clip_boundary_vertices.get_mut(grip_id) {
                // Compute current world position of this vertex.
                let ox = self.insertion_point.x;
                let oy = self.insertion_point.y;
                let oz = self.insertion_point.z;
                let cur_wx =
                    ox + self.u_vector.x * v.x * self.size.x + self.v_vector.x * v.y * self.size.y;
                let cur_wy =
                    oy + self.u_vector.y * v.x * self.size.x + self.v_vector.y * v.y * self.size.y;
                let cur_wz =
                    oz + self.u_vector.z * v.x * self.size.x + self.v_vector.z * v.y * self.size.y;
                let new_w = match apply {
                    GripApply::Translate(d) => [
                        cur_wx + d.x as f64,
                        cur_wy + d.y as f64,
                        cur_wz + d.z as f64,
                    ],
                    GripApply::Absolute(p) => [p.x as f64, p.y as f64, p.z as f64],
                };
                // Back-project: solve for pixel coords using u_vector and v_vector.
                // In 2D (XY plane): new_w - insertion_point = u_vec * vx * sx + v_vec * vy * sy
                let dx = new_w[0] - self.insertion_point.x;
                let dy = new_w[1] - self.insertion_point.y;
                let ux = self.u_vector.x * self.size.x;
                let uy = self.u_vector.y * self.size.x;
                let vx = self.v_vector.x * self.size.y;
                let vy = self.v_vector.y * self.size.y;
                let det = ux * vy - uy * vx;
                if det.abs() > 1e-12 {
                    v.x = (dx * vy - dy * vx) / det;
                    v.y = (ux * dy - uy * dx) / det;
                }
            }
        } else if grip_id == 0 {
            match apply {
                GripApply::Translate(d) => {
                    self.insertion_point.x += d.x as f64;
                    self.insertion_point.y += d.y as f64;
                    self.insertion_point.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    self.insertion_point.x = p.x as f64;
                    self.insertion_point.y = p.y as f64;
                    self.insertion_point.z = p.z as f64;
                }
            }
        }
    }
}

impl PropertyEditable for Wipeout {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        PropSection {
            title: "Geometry".into(),
            props: vec![
                edit("Insert X", "wo_ox", self.insertion_point.x),
                edit("Insert Y", "wo_oy", self.insertion_point.y),
                edit("Insert Z", "wo_oz", self.insertion_point.z),
                edit("Brightness", "wo_bright", self.brightness as f64),
                edit("Contrast", "wo_contrast", self.contrast as f64),
                edit("Fade", "wo_fade", self.fade as f64),
                Property {
                    label: "Clipping".into(),
                    field: "wo_clip",
                    value: PropValue::BoolToggle {
                        field: "wo_clip",
                        value: self.clipping_enabled,
                    },
                },
            ],
        }
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "wo_clip" => {
                self.clipping_enabled = if value == "toggle" {
                    !self.clipping_enabled
                } else {
                    value == "true"
                };
                return;
            }
            _ => {}
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "wo_ox" => self.insertion_point.x = v,
            "wo_oy" => self.insertion_point.y = v,
            "wo_oz" => self.insertion_point.z = v,
            "wo_bright" => self.brightness = v.clamp(0.0, 100.0) as u8,
            "wo_contrast" => self.contrast = v.clamp(0.0, 100.0) as u8,
            "wo_fade" => self.fade = v.clamp(0.0, 100.0) as u8,
            _ => {}
        }
    }
}

impl Transformable for Wipeout {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            crate::scene::transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
            let ax = (p2.x - p1.x) as f64;
            let ay = (p2.y - p1.y) as f64;
            let len2 = ax * ax + ay * ay;
            if len2 > 1e-12 {
                reflect_vec3(&mut entity.u_vector.x, &mut entity.u_vector.y, ax, ay, len2);
                reflect_vec3(&mut entity.v_vector.x, &mut entity.v_vector.y, ax, ay, len2);
            }
        });
    }
}
