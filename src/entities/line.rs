use acadrust::{entities::Line, Entity};
use glam::Vec3;
use truck_modeling::{builder, Point3};

use crate::command::EntityTransform;
use crate::entities::common::{
    diamond_grip, edit_prop as edit, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::TangentGeom;

fn to_truck(line: &Line) -> TruckEntity {
    let normal = (line.normal.x, line.normal.y, line.normal.z);
    let (sx, sy, sz) = crate::scene::transform::ocs_point_to_wcs(
        (line.start.x, line.start.y, line.start.z),
        normal,
    );
    let (ex, ey, ez) =
        crate::scene::transform::ocs_point_to_wcs((line.end.x, line.end.y, line.end.z), normal);
    let p0 = Point3::new(sx, sy, sz);
    let p1 = Point3::new(ex, ey, ez);
    let kv: Vec<[f64; 3]> = vec![[p0.x, p0.y, p0.z], [p1.x, p1.y, p1.z]];
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
        return TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts: vec![],
            tangent_geoms: vec![tangent],
            key_vertices: kv,
            fill_tris: vec![],
        };
    }

    let v0 = builder::vertex(p0);
    let v1 = builder::vertex(p1);
    let edge = builder::line(&v0, &v1);
    TruckEntity {
        object: TruckObject::Curve(edge),
        snap_pts: vec![],
        tangent_geoms: vec![tangent],
        key_vertices: kv,
        fill_tris: vec![],
    }
}

fn grips(line: &Line) -> Vec<GripDef> {
    let s = Vec3::new(
        line.start.x as f32,
        line.start.y as f32,
        line.start.z as f32,
    );
    let e = Vec3::new(line.end.x as f32, line.end.y as f32, line.end.z as f32);
    let m = (s + e) * 0.5;
    vec![square_grip(0, s), square_grip(1, e), diamond_grip(2, m)]
}

fn properties(line: &Line) -> PropSection {
    PropSection {
        title: "Geometry".into(),
        props: vec![
            edit("Start X", "start_x", line.start.x),
            edit("Start Y", "start_y", line.start.y),
            edit("Start Z", "start_z", line.start.z),
            edit("End X", "end_x", line.end.x),
            edit("End Y", "end_y", line.end.y),
            edit("End Z", "end_z", line.end.z),
            ro("Length", "length", format!("{:.4}", line.length())),
        ],
    }
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
        EntityTransform::Rotate { center, angle_rad } => {
            crate::scene::transform::apply_standard_transform(line, *center, *angle_rad);
        }
        EntityTransform::Scale { center, factor } => {
            crate::scene::transform::apply_standard_scale(line, *center, *factor);
        }
        EntityTransform::Mirror { p1, p2 } => {
            crate::scene::transform::mirror_xy_line(line, *p1, *p2);
        }
    }
}

impl TruckConvertible for Line {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        Some(to_truck(self))
    }
}

impl Grippable for Line {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for Line {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Line {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
