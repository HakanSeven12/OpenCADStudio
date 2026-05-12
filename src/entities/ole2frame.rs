use acadrust::entities::{Ole2Frame, OleObjectType};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{diamond_grip, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::SnapHint;

fn to_truck(ole: &Ole2Frame) -> TruckEntity {
    let x0 = ole.upper_left_corner.x;
    let y0 = ole.lower_right_corner.y;
    let x1 = ole.lower_right_corner.x;
    let y1 = ole.upper_left_corner.y;
    let z = ole.upper_left_corner.z;

    if (x1 - x0).abs() < 1e-6 && (y1 - y0).abs() < 1e-6 {
        let s = 0.5_f64;
        return TruckEntity {
            object: TruckObject::Lines(vec![[-s, 0.0, z], [s, 0.0, z]]),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let pts: Vec<[f64; 3]> = vec![
        // Rectangle
        [x0, y0, z],
        [x1, y0, z],
        [x1, y0, z],
        [x1, y1, z],
        [x1, y1, z],
        [x0, y1, z],
        [x0, y1, z],
        [x0, y0, z],
        // Diagonal X
        [x0, y0, z],
        [x1, y1, z],
        [f64::NAN; 3],
        [x1, y0, z],
        [x0, y1, z],
    ];
    let center = Vec3::new(cx as f32, cy as f32, z as f32);
    TruckEntity {
        object: TruckObject::Lines(pts),
        snap_pts: vec![(center, SnapHint::Center)],
        tangent_geoms: vec![],
        key_vertices: vec![[x0, y0, z], [x1, y1, z]],
        fill_tris: vec![],
    }
}

fn grips(ole: &Ole2Frame) -> Vec<GripDef> {
    let ul = Vec3::new(
        ole.upper_left_corner.x as f32,
        ole.upper_left_corner.y as f32,
        ole.upper_left_corner.z as f32,
    );
    let lr = Vec3::new(
        ole.lower_right_corner.x as f32,
        ole.lower_right_corner.y as f32,
        ole.lower_right_corner.z as f32,
    );
    let center = (ul + lr) * 0.5;
    vec![
        square_grip(0, ul),
        square_grip(1, lr),
        diamond_grip(2, center),
    ]
}

fn properties(ole: &Ole2Frame) -> PropSection {
    let type_str = match ole.ole_object_type {
        OleObjectType::Link => "Link",
        OleObjectType::Embedded => "Embedded",
        OleObjectType::Static => "Static",
    };
    PropSection {
        title: "Geometry".into(),
        props: vec![
            ro("Type", "ole_type", type_str),
            edit("Upper Left X", "ole_ulx", ole.upper_left_corner.x),
            edit("Upper Left Y", "ole_uly", ole.upper_left_corner.y),
            edit("Lower Right X", "ole_lrx", ole.lower_right_corner.x),
            edit("Lower Right Y", "ole_lry", ole.lower_right_corner.y),
        ],
    }
}

fn apply_geom_prop(ole: &mut Ole2Frame, field: &str, value: &str) {
    let Ok(v) = value.trim().parse::<f64>() else {
        return;
    };
    match field {
        "ole_ulx" => ole.upper_left_corner.x = v,
        "ole_uly" => ole.upper_left_corner.y = v,
        "ole_lrx" => ole.lower_right_corner.x = v,
        "ole_lry" => ole.lower_right_corner.y = v,
        _ => {}
    }
}

fn apply_grip(ole: &mut Ole2Frame, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Absolute(p)) => {
            ole.upper_left_corner.x = p.x as f64;
            ole.upper_left_corner.y = p.y as f64;
        }
        (1, GripApply::Absolute(p)) => {
            ole.lower_right_corner.x = p.x as f64;
            ole.lower_right_corner.y = p.y as f64;
        }
        (2, GripApply::Translate(d)) => {
            ole.upper_left_corner.x += d.x as f64;
            ole.upper_left_corner.y += d.y as f64;
            ole.lower_right_corner.x += d.x as f64;
            ole.lower_right_corner.y += d.y as f64;
        }
        _ => {}
    }
}

fn apply_transform(ole: &mut Ole2Frame, t: &EntityTransform) {
    match t {
        EntityTransform::Translate(d) => {
            ole.upper_left_corner.x += d.x as f64;
            ole.upper_left_corner.y += d.y as f64;
            ole.upper_left_corner.z += d.z as f64;
            ole.lower_right_corner.x += d.x as f64;
            ole.lower_right_corner.y += d.y as f64;
            ole.lower_right_corner.z += d.z as f64;
        }
        EntityTransform::Scale { center, factor } => {
            let scale = |v: f64, c: f64| c + (v - c) * (*factor as f64);
            ole.upper_left_corner.x = scale(ole.upper_left_corner.x, center.x as f64);
            ole.upper_left_corner.y = scale(ole.upper_left_corner.y, center.y as f64);
            ole.lower_right_corner.x = scale(ole.lower_right_corner.x, center.x as f64);
            ole.lower_right_corner.y = scale(ole.lower_right_corner.y, center.y as f64);
        }
        _ => {}
    }
}

impl TruckConvertible for Ole2Frame {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        Some(to_truck(self))
    }
}

impl Grippable for Ole2Frame {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for Ole2Frame {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }
    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Ole2Frame {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
