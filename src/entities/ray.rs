use acadrust::entities::{Ray, XLine};
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{center_grip, edit_prop as edit, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};

/// Display length used when rendering semi-infinite / infinite lines.
const DISPLAY_EXTENT: f64 = 1_000_000.0;

// ── Ray (semi-infinite line) ──────────────────────────────────────────────────

impl RenderConvertible for Ray {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        let bp = self.base_point;
        let dir = self.direction;
        // Normalize direction to avoid f32 overflow when DXF stores
        // unnormalized direction vectors (garbage data in some exporters).
        let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
        if len < 1e-10 {
            return None;
        }
        let (nx, ny, nz) = (dir.x / len, dir.y / len, dir.z / len);
        let far: [f64; 3] = [
            bp.x + nx * DISPLAY_EXTENT,
            bp.y + ny * DISPLAY_EXTENT,
            bp.z + nz * DISPLAY_EXTENT,
        ];
        let start: [f64; 3] = [bp.x, bp.y, bp.z];
        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(vec![start, far]),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![start],
            fill_tris: vec![],
        })
    }
}

impl Grippable for Ray {
    fn grips(&self) -> Vec<GripDef> {
        let bp = &self.base_point;
        let dir = &self.direction;
        // Grip 0: base point (movable)
        // Grip 1: a point along the direction (changes direction)
        let guide_dist = 10.0_f64;
        vec![
            square_grip(0, glam::DVec3::new(bp.x, bp.y, bp.z)),
            center_grip(
                1,
                glam::DVec3::new(
                    bp.x + dir.x * guide_dist,
                    bp.y + dir.y * guide_dist,
                    bp.z + dir.z * guide_dist,
                ),
            ),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        match (grip_id, apply) {
            (0, GripApply::Translate(d)) => {
                self.base_point.x += d.x as f64;
                self.base_point.y += d.y as f64;
                self.base_point.z += d.z as f64;
            }
            (0, GripApply::Absolute(p)) => {
                self.base_point.x = p.x as f64;
                self.base_point.y = p.y as f64;
                self.base_point.z = p.z as f64;
            }
            (1, GripApply::Absolute(p)) => {
                // New direction = grip point - base point, normalized.
                let dx = p.x as f64 - self.base_point.x;
                let dy = p.y as f64 - self.base_point.y;
                let dz = p.z as f64 - self.base_point.z;
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                if len > 1e-9 {
                    self.direction.x = dx / len;
                    self.direction.y = dy / len;
                    self.direction.z = dz / len;
                }
            }
            _ => {}
        }
    }
}

impl PropertyEditable for Ray {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Start X").as_ref(), "ray_bx", self.base_point.x),
                edit(t!("Start Y").as_ref(), "ray_by", self.base_point.y),
                edit(t!("Start Z").as_ref(), "ray_bz", self.base_point.z),
                edit(t!("Direction vector X").as_ref(), "ray_dx", self.direction.x),
                edit(t!("Direction vector Y").as_ref(), "ray_dy", self.direction.y),
                edit(t!("Direction vector Z").as_ref(), "ray_dz", self.direction.z),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "ray_bx" => self.base_point.x = v,
            "ray_by" => self.base_point.y = v,
            "ray_bz" => self.base_point.z = v,
            "ray_dx" => {
                self.direction.x = v;
            }
            "ray_dy" => {
                self.direction.y = v;
            }
            "ray_dz" => {
                self.direction.z = v;
            }
            _ => {}
        }
    }
}

impl Transformable for Ray {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            crate::scene::view::transform::reflect_xy_point(
                &mut entity.base_point.x,
                &mut entity.base_point.y,
                p1,
                p2,
            );
            // Mirror the direction: negate the component perpendicular to mirror axis.
            let ax = (p2.x - p1.x) as f64;
            let ay = (p2.y - p1.y) as f64;
            let len2 = ax * ax + ay * ay;
            if len2 > 1e-12 {
                let d = &mut entity.direction;
                let dot = d.x * ax + d.y * ay;
                d.x = 2.0 * dot * ax / len2 - d.x;
                d.y = 2.0 * dot * ay / len2 - d.y;
            }
        });
    }
}

// ── XLine (construction line, infinite) ──────────────────────────────────────

impl RenderConvertible for XLine {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        let bp = self.base_point;
        let dir = self.direction;
        let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
        if len < 1e-10 {
            return None;
        }
        let (nx, ny, nz) = (dir.x / len, dir.y / len, dir.z / len);
        let far_pos: [f64; 3] = [
            bp.x + nx * DISPLAY_EXTENT,
            bp.y + ny * DISPLAY_EXTENT,
            bp.z + nz * DISPLAY_EXTENT,
        ];
        let far_neg: [f64; 3] = [
            bp.x - nx * DISPLAY_EXTENT,
            bp.y - ny * DISPLAY_EXTENT,
            bp.z - nz * DISPLAY_EXTENT,
        ];
        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(vec![far_neg, far_pos]),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![[bp.x, bp.y, bp.z]],
            fill_tris: vec![],
        })
    }
}

impl Grippable for XLine {
    fn grips(&self) -> Vec<GripDef> {
        let bp = &self.base_point;
        let dir = &self.direction;
        let guide_dist = 10.0_f64;
        vec![
            square_grip(0, glam::DVec3::new(bp.x, bp.y, bp.z)),
            center_grip(
                1,
                glam::DVec3::new(
                    bp.x + dir.x * guide_dist,
                    bp.y + dir.y * guide_dist,
                    bp.z + dir.z * guide_dist,
                ),
            ),
        ]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        match (grip_id, apply) {
            (0, GripApply::Translate(d)) => {
                self.base_point.x += d.x as f64;
                self.base_point.y += d.y as f64;
                self.base_point.z += d.z as f64;
            }
            (0, GripApply::Absolute(p)) => {
                self.base_point.x = p.x as f64;
                self.base_point.y = p.y as f64;
                self.base_point.z = p.z as f64;
            }
            (1, GripApply::Absolute(p)) => {
                let dx = p.x as f64 - self.base_point.x;
                let dy = p.y as f64 - self.base_point.y;
                let dz = p.z as f64 - self.base_point.z;
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                if len > 1e-9 {
                    self.direction.x = dx / len;
                    self.direction.y = dy / len;
                    self.direction.z = dz / len;
                }
            }
            _ => {}
        }
    }
}

impl PropertyEditable for XLine {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Base X").as_ref(), "xl_bx", self.base_point.x),
                edit(t!("Base Y").as_ref(), "xl_by", self.base_point.y),
                edit(t!("Base Z").as_ref(), "xl_bz", self.base_point.z),
                edit(t!("Direction vector X").as_ref(), "xl_dx", self.direction.x),
                edit(t!("Direction vector Y").as_ref(), "xl_dy", self.direction.y),
                edit(t!("Direction vector Z").as_ref(), "xl_dz", self.direction.z),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "xl_bx" => self.base_point.x = v,
            "xl_by" => self.base_point.y = v,
            "xl_bz" => self.base_point.z = v,
            "xl_dx" => {
                self.direction.x = v;
            }
            "xl_dy" => {
                self.direction.y = v;
            }
            "xl_dz" => {
                self.direction.z = v;
            }
            _ => {}
        }
    }
}

impl Transformable for XLine {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            crate::scene::view::transform::reflect_xy_point(
                &mut entity.base_point.x,
                &mut entity.base_point.y,
                p1,
                p2,
            );
            let ax = (p2.x - p1.x) as f64;
            let ay = (p2.y - p1.y) as f64;
            let len2 = ax * ax + ay * ay;
            if len2 > 1e-12 {
                let d = &mut entity.direction;
                let dot = d.x * ax + d.y * ay;
                d.x = 2.0 * dot * ax / len2 - d.x;
                d.y = 2.0 * dot * ay / len2 - d.y;
            }
        });
    }
}
