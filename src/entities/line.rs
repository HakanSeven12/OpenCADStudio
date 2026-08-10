use acadrust::{entities::Line, Entity};
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_prop as edit, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{extrusion_wall_tris, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::TangentGeom;

fn to_render(line: &Line) -> RenderEntity {
    // LINE endpoints are stored in WCS — unlike the planar OCS entities
    // (ARC/CIRCLE/LWPOLYLINE/TEXT), the extrusion normal on a LINE only
    // orients its thickness sweep. Remapping the endpoints through the
    // arbitrary-axis OCS mirrored every line carried over from a MIRROR
    // (normal 0,0,-1) to the wrong side of the drawing.
    let normal = (line.normal.x, line.normal.y, line.normal.z);
    let (sx, sy, sz) = (line.start.x, line.start.y, line.start.z);
    let (ex, ey, ez) = (line.end.x, line.end.y, line.end.z);
    let kv: Vec<[f64; 3]> = vec![[sx, sy, sz], [ex, ey, ez]];
    let tangent = TangentGeom::Line {
        p1: [kv[0][0] as f32, kv[0][1] as f32, kv[0][2] as f32],
        p2: [kv[1][0] as f32, kv[1][1] as f32, kv[1][2] as f32],
    };

    if line.thickness.abs() > 1e-10 {
        let t = line.thickness;
        let (nx, ny, nz) = normal;
        let p0t = [sx + t * nx, sy + t * ny, sz + t * nz];
        let p1t = [ex + t * nx, ey + t * ny, ez + t * nz];
        let pts: Vec<[f64; 3]> = vec![
            kv[0],
            kv[1],
            [f64::NAN; 3],
            p0t,
            p1t,
            [f64::NAN; 3],
            kv[0],
            p0t,
            [f64::NAN; 3],
            kv[1],
            p1t,
        ];
        return RenderEntity {
            pick_tris: extrusion_wall_tris(&kv, [t * nx, t * ny, t * nz]),
            object: RenderObject::Lines(pts),
            snap_pts: vec![],
            tangent_geoms: vec![tangent],
            key_vertices: kv,
            fill_tris: vec![],
        };
    }

    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(kv.clone()),
        snap_pts: vec![],
        tangent_geoms: vec![tangent],
        key_vertices: kv,
        fill_tris: vec![],
    }
}

fn grips(line: &Line) -> Vec<GripDef> {
    let s = glam::DVec3::new(line.start.x, line.start.y, line.start.z);
    let e = glam::DVec3::new(line.end.x, line.end.y, line.end.z);
    let m = (s + e) * 0.5;
    vec![square_grip(0, s), square_grip(1, e), center_grip(2, m)]
}

fn properties(line: &Line) -> Vec<PropSection> {
    let dx = line.end.x - line.start.x;
    let dy = line.end.y - line.start.y;
    let dz = line.end.z - line.start.z;
    let angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            edit(t!("Start X").as_ref(), "start_x", line.start.x),
            edit(t!("Start Y").as_ref(), "start_y", line.start.y),
            edit(t!("Start Z").as_ref(), "start_z", line.start.z),
            edit(t!("End X").as_ref(), "end_x", line.end.x),
            edit(t!("End Y").as_ref(), "end_y", line.end.y),
            edit(t!("End Z").as_ref(), "end_z", line.end.z),
            ro(t!("Delta X").as_ref(), "delta_x", format!("{dx:.4}")),
            ro(t!("Delta Y").as_ref(), "delta_y", format!("{dy:.4}")),
            ro(t!("Delta Z").as_ref(), "delta_z", format!("{dz:.4}")),
            ro(t!("Length").as_ref(), "length", format!("{:.4}", line.length())),
            ro(t!("Angle").as_ref(), "angle", format!("{angle:.2}")),
        ],
    }]
}

fn apply_geom_prop(line: &mut Line, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "start_x" => line.start.x = v,
        "start_y" => line.start.y = v,
        "start_z" => line.start.z = v,
        "end_x" => line.end.x = v,
        "end_y" => line.end.y = v,
        "end_z" => line.end.z = v,
        _ => {}
    }
}

fn apply_grip(line: &mut Line, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Absolute(p)) => {
            line.start.x = p.x as f64;
            line.start.y = p.y as f64;
            line.start.z = p.z as f64;
        }
        (1, GripApply::Absolute(p)) => {
            line.end.x = p.x as f64;
            line.end.y = p.y as f64;
            line.end.z = p.z as f64;
        }
        (2, GripApply::Translate(d)) => {
            line.start.x += d.x as f64;
            line.start.y += d.y as f64;
            line.start.z += d.z as f64;
            line.end.x += d.x as f64;
            line.end.y += d.y as f64;
            line.end.z += d.z as f64;
        }
        _ => {}
    }
}

fn apply_transform(line: &mut Line, t: &EntityTransform) {
    match t {
        EntityTransform::Translate(d) => {
            line.translate(acadrust::types::Vector3::new(
                d.x as f64, d.y as f64, d.z as f64,
            ));
        }
        EntityTransform::Rotate { center, axis, angle_rad } => {
            crate::scene::view::transform::apply_standard_transform(line, *center, *axis, *angle_rad);
        }
        EntityTransform::Scale { center, factor } => {
            crate::scene::view::transform::apply_standard_scale(line, *center, *factor);
        }
        EntityTransform::Mirror { p1, p2, working_normal } => {
            acadrust::Entity::apply_transform(
                line,
                &crate::scene::view::transform::reflection_about_working_line(
                    *p1,
                    *p2,
                    *working_normal,
                ),
            );
        }
        EntityTransform::Affine(transform) => {
            acadrust::Entity::apply_transform(line, transform);
        }
    }
}

impl RenderConvertible for Line {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self))
    }
}

impl crate::entities::traits::Grippable for Line {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        if grip_id == 2 {
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
        } else {
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Lengthen",
                    action: GripMenuAction::Lengthen,
                },
            ]
        }
    }
    fn apply_grip_menu(&mut self, _grip_id: usize, _action: crate::scene::model::object::GripMenuAction) {
        // Lengthen needs a follow-up distance — handled by
        // `apply_grip_menu_value`.
    }

    fn grip_menu_value_prompt(
        &self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::model::object::GripMenuAction as A;
        match action {
            A::Lengthen => Some("Distance"),
            _ => None,
        }
    }

    fn grip_menu_point_value(
        &self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        point: glam::DVec3,
    ) -> Option<f64> {
        use crate::scene::model::object::GripMenuAction as A;
        if !matches!(action, A::Lengthen) {
            return None;
        }
        let direction = glam::DVec3::new(
            self.end.x - self.start.x,
            self.end.y - self.start.y,
            self.end.z - self.start.z,
        );
        let length = direction.length();
        if length < 1.0e-12 {
            return None;
        }
        let unit = direction / length;
        let value = match grip_id {
            0 => (glam::DVec3::new(self.start.x, self.start.y, self.start.z) - point).dot(unit),
            1 => (point - glam::DVec3::new(self.end.x, self.end.y, self.end.z)).dot(unit),
            _ => return None,
        };
        (length + value > 1.0e-9).then_some(value)
    }

    fn apply_grip_menu_value(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        if !matches!(action, A::Lengthen) {
            return;
        }
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let dz = self.end.z - self.start.z;
        let len = (dx * dx + dy * dy + dz * dz).sqrt();
        if len < 1e-12 {
            return;
        }
        let (ux, uy, uz) = (dx / len, dy / len, dz / len);
        match grip_id {
            0 => {
                // Move start endpoint backward along the line by `value`
                // (positive = lengthen; negative = shorten).
                self.start.x -= ux * value;
                self.start.y -= uy * value;
                self.start.z -= uz * value;
            }
            1 => {
                self.end.x += ux * value;
                self.end.y += uy * value;
                self.end.z += uz * value;
            }
            _ => {}
        }
    }
}

impl crate::entities::traits::PropertyEditable for Line {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }
    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl crate::entities::traits::Transformable for Line {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

impl crate::entities::traits::MassPropsCalc for acadrust::entities::Line {
    fn mass_props(&self) -> crate::entities::traits::MassProps {
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let len = (dx * dx + dy * dy).sqrt();
        crate::entities::traits::MassProps {
            area: 0.0,
            perimeter: len,
            cx: (self.start.x + self.end.x) / 2.0,
            cy: (self.start.y + self.end.y) / 2.0,
        }
    }
}
