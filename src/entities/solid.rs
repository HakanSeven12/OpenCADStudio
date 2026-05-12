// SOLID entity — 2D filled quadrilateral (or triangle when p3 == p4).
//
// Wireframe: the 4 perimeter edges as TruckObject::Lines.
// Filled:    the scene injects a solid-fill HatchModel (like hatch.rs).
// Grips:     4 corner grip points.

use acadrust::entities::Solid;
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::SnapHint;

fn v3(v: &acadrust::types::Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn v3f32(v: &acadrust::types::Vector3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

impl TruckConvertible for Solid {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        let p0 = v3(&self.first_corner);
        let p1 = v3(&self.second_corner);
        let p2 = v3(&self.third_corner);
        let p3 = v3(&self.fourth_corner);

        // DXF SOLID vertex order: 1-2-4-3 (Z-shaped), render as closed quad outline.
        // AutoCAD stores corners in "Z" order: p0-p1 are top edge, p2-p3 are bottom edge,
        // so the visual quad is p0→p1→p3→p2→p0.
        let pts = vec![
            p0,
            p1,
            [f64::NAN; 3],
            p1,
            p3,
            [f64::NAN; 3],
            p3,
            p2,
            [f64::NAN; 3],
            p2,
            p0,
        ];

        let snap = vec![
            (Vec3::from(v3f32(&self.first_corner)), SnapHint::Node),
            (Vec3::from(v3f32(&self.second_corner)), SnapHint::Node),
            (Vec3::from(v3f32(&self.third_corner)), SnapHint::Node),
            (Vec3::from(v3f32(&self.fourth_corner)), SnapHint::Node),
        ];

        Some(TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts: snap,
            tangent_geoms: vec![],
            key_vertices: vec![p0, p1, p2, p3],
            fill_tris: vec![],
        })
    }
}

impl Grippable for Solid {
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

impl PropertyEditable for Solid {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        PropSection {
            title: "Geometry".into(),
            props: vec![
                edit("P1 X", "sl_p1x", self.first_corner.x),
                edit("P1 Y", "sl_p1y", self.first_corner.y),
                edit("P1 Z", "sl_p1z", self.first_corner.z),
                edit("P2 X", "sl_p2x", self.second_corner.x),
                edit("P2 Y", "sl_p2y", self.second_corner.y),
                edit("P2 Z", "sl_p2z", self.second_corner.z),
                edit("P3 X", "sl_p3x", self.third_corner.x),
                edit("P3 Y", "sl_p3y", self.third_corner.y),
                edit("P3 Z", "sl_p3z", self.third_corner.z),
                edit("P4 X", "sl_p4x", self.fourth_corner.x),
                edit("P4 Y", "sl_p4y", self.fourth_corner.y),
                edit("P4 Z", "sl_p4z", self.fourth_corner.z),
                edit("Thickness", "sl_thick", self.thickness),
            ],
        }
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "sl_p1x" => self.first_corner.x = v,
            "sl_p1y" => self.first_corner.y = v,
            "sl_p1z" => self.first_corner.z = v,
            "sl_p2x" => self.second_corner.x = v,
            "sl_p2y" => self.second_corner.y = v,
            "sl_p2z" => self.second_corner.z = v,
            "sl_p3x" => self.third_corner.x = v,
            "sl_p3y" => self.third_corner.y = v,
            "sl_p3z" => self.third_corner.z = v,
            "sl_p4x" => self.fourth_corner.x = v,
            "sl_p4y" => self.fourth_corner.y = v,
            "sl_p4z" => self.fourth_corner.z = v,
            "sl_thick" => self.thickness = v,
            _ => {}
        }
    }
}

impl Transformable for Solid {
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
