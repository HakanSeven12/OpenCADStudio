use acadrust::entities::{HooklineDirection, Leader, LeaderCreationType, LeaderPathType};
use acadrust::Entity;
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{diamond_grip, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::wire_model::TangentGeom;

// ── TruckConvertible (used for snap/grip key-vertices) ─────────────────────

fn to_truck(leader: &Leader) -> TruckEntity {
    let verts = &leader.vertices;
    let nan = [f64::NAN; 3];
    let p3 = |v: &acadrust::types::Vector3| -> [f64; 3] { [v.x, v.y, v.z] };
    let p3f = |v: &acadrust::types::Vector3| -> [f32; 3] {
        [v.x as f32, v.y as f32, v.z as f32]
    };

    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut tangents: Vec<TangentGeom> = Vec::new();
    let mut key_verts: Vec<[f64; 3]> = Vec::new();

    // Main leader path
    for v in verts {
        points.push(p3(v));
        key_verts.push(p3(v));
    }
    for i in 0..verts.len().saturating_sub(1) {
        // TangentGeom uses f32 (UI-only); cast at construction.
        tangents.push(TangentGeom::Line {
            p1: p3f(&verts[i]),
            p2: p3f(&verts[i + 1]),
        });
    }

    // Arrowhead at vertex[0]
    if leader.arrow_enabled && verts.len() >= 2 {
        let tip = &verts[0];
        let next = &verts[1];
        let dx = next.x - tip.x;
        let dy = next.y - tip.y;
        let len = (dx * dx + dy * dy).sqrt().max(1e-9);
        let (dx, dy) = (dx / len, dy / len);
        let sz = (leader.text_height).max(1.0) * 0.8;
        let a = std::f64::consts::PI / 6.0;
        let (s, c) = a.sin_cos();
        let tip_f = p3(tip);
        points.push(nan);
        points.push([
            tip_f[0] + (dx * c - dy * s) * sz,
            tip_f[1] + (dx * s + dy * c) * sz,
            tip_f[2],
        ]);
        points.push(tip_f);
        points.push([
            tip_f[0] + (dx * c + dy * s) * sz,
            tip_f[1] + (-dx * s + dy * c) * sz,
            tip_f[2],
        ]);
    }

    // Landing line at last vertex
    if leader.hookline_enabled && verts.len() >= 2 {
        let last = verts.last().unwrap();
        let prev = &verts[verts.len() - 2];
        let sign = if (last.x - prev.x) >= 0.0 { 1.0_f64 } else { -1.0_f64 };
        let len = leader.text_height * 1.5;
        let last_f = p3(last);
        points.push(nan);
        points.push(last_f);
        points.push([last_f[0] + sign * len, last_f[1], last_f[2]]);
    }

    TruckEntity {
        object: TruckObject::Lines(points),
        snap_pts: vec![],
        tangent_geoms: tangents,
        key_vertices: key_verts,
        fill_tris: vec![],
    }
}

// ── Grips ──────────────────────────────────────────────────────────────────

fn grips(leader: &Leader) -> Vec<GripDef> {
    let n = leader.vertices.len();
    let mut grips: Vec<GripDef> = leader
        .vertices
        .iter()
        .enumerate()
        .map(|(i, v)| square_grip(i, Vec3::new(v.x as f32, v.y as f32, v.z as f32)))
        .collect();

    if n >= 2 {
        let sum = leader.vertices.iter().fold(Vec3::ZERO, |acc, v| {
            acc + Vec3::new(v.x as f32, v.y as f32, v.z as f32)
        });
        grips.push(diamond_grip(n, sum / n as f32));
    }

    grips
}

fn apply_grip(leader: &mut Leader, grip_id: usize, apply: GripApply) {
    let n = leader.vertices.len();
    if grip_id < n {
        if let Some(v) = leader.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Absolute(p) => {
                    v.x = p.x as f64;
                    v.y = p.y as f64;
                    v.z = p.z as f64;
                }
                GripApply::Translate(d) => {
                    v.x += d.x as f64;
                    v.y += d.y as f64;
                    v.z += d.z as f64;
                }
            }
        }
    } else if let GripApply::Translate(d) = apply {
        leader.translate(acadrust::types::Vector3::new(
            d.x as f64, d.y as f64, d.z as f64,
        ));
    }
}

// ── Properties ─────────────────────────────────────────────────────────────

fn bool_toggle(label: &str, field: &'static str, value: bool) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::BoolToggle { field, value },
    }
}

fn choice_prop(label: &str, field: &'static str, selected: &str, options: &[&str]) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::Choice {
            selected: selected.to_string(),
            options: options.iter().map(|s| s.to_string()).collect(),
        },
    }
}

fn path_type_str(pt: &LeaderPathType) -> &'static str {
    match pt {
        LeaderPathType::StraightLine => "Straight",
        LeaderPathType::Spline => "Spline",
    }
}

fn creation_type_str(ct: &LeaderCreationType) -> &'static str {
    match ct {
        LeaderCreationType::WithText => "With Text",
        LeaderCreationType::WithTolerance => "With Tolerance",
        LeaderCreationType::WithBlock => "With Block",
        LeaderCreationType::NoAnnotation => "No Annotation",
    }
}

fn hookline_dir_str(hd: &HooklineDirection) -> &'static str {
    match hd {
        HooklineDirection::Opposite => "Opposite",
        HooklineDirection::Same => "Same",
    }
}

fn properties(leader: &Leader) -> PropSection {
    let n = leader.vertices.len();
    let mut props = vec![
        // Style
        Property {
            label: "Dim Style".into(),
            field: "dimension_style",
            value: PropValue::EditText(leader.dimension_style.clone()),
        },
        // Path type
        choice_prop(
            "Path Type",
            "path_type",
            path_type_str(&leader.path_type),
            &["Straight", "Spline"],
        ),
        // Creation type
        choice_prop(
            "Creation Type",
            "creation_type",
            creation_type_str(&leader.creation_type),
            &["With Text", "With Tolerance", "With Block", "No Annotation"],
        ),
        // Arrow
        bool_toggle("Arrow", "arrow_enabled", leader.arrow_enabled),
        // Hookline
        bool_toggle("Hookline", "hookline_enabled", leader.hookline_enabled),
        choice_prop(
            "Hookline Dir",
            "hookline_direction",
            hookline_dir_str(&leader.hookline_direction),
            &["Opposite", "Same"],
        ),
        // Text dims
        edit("Text Height", "text_height", leader.text_height),
        edit("Text Width", "text_width", leader.text_width),
        // Normal
        edit("Normal X", "normal_x", leader.normal.x),
        edit("Normal Y", "normal_y", leader.normal.y),
        edit("Normal Z", "normal_z", leader.normal.z),
        // Horizontal direction
        edit("H Dir X", "h_dir_x", leader.horizontal_direction.x),
        edit("H Dir Y", "h_dir_y", leader.horizontal_direction.y),
        edit("H Dir Z", "h_dir_z", leader.horizontal_direction.z),
        // Block offset
        edit("Block Offset X", "block_offset_x", leader.block_offset.x),
        edit("Block Offset Y", "block_offset_y", leader.block_offset.y),
        edit("Block Offset Z", "block_offset_z", leader.block_offset.z),
        // Annotation offset
        edit("Ann Offset X", "ann_offset_x", leader.annotation_offset.x),
        edit("Ann Offset Y", "ann_offset_y", leader.annotation_offset.y),
        edit("Ann Offset Z", "ann_offset_z", leader.annotation_offset.z),
        // Stats
        ro("Vertices", "vertex_count", n.to_string()),
        ro("Length", "length", format!("{:.4}", leader.length())),
    ];

    // Arrow point (vertex[0])
    if let Some(a) = leader.arrow_point() {
        props.push(edit("Arrow X", "arrow_x", a.x));
        props.push(edit("Arrow Y", "arrow_y", a.y));
        props.push(edit("Arrow Z", "arrow_z", a.z));
    }

    // End point (last vertex)
    if n >= 2 {
        if let Some(e) = leader.end_point() {
            props.push(edit("End X", "end_x", e.x));
            props.push(edit("End Y", "end_y", e.y));
            props.push(edit("End Z", "end_z", e.z));
        }
    }

    PropSection {
        title: "Geometry".into(),
        props,
    }
}

fn apply_geom_prop(leader: &mut Leader, field: &str, value: &str) {
    let f64 = |s: &str| -> Option<f64> { s.trim().parse().ok() };

    match field {
        "dimension_style" => leader.dimension_style = value.to_string(),
        "path_type" => {
            leader.path_type = match value {
                "Spline" => LeaderPathType::Spline,
                _ => LeaderPathType::StraightLine,
            };
        }
        "creation_type" => {
            leader.creation_type = match value {
                "With Tolerance" => LeaderCreationType::WithTolerance,
                "With Block" => LeaderCreationType::WithBlock,
                "No Annotation" => LeaderCreationType::NoAnnotation,
                _ => LeaderCreationType::WithText,
            };
        }
        "arrow_enabled" => {
            leader.arrow_enabled = if value == "toggle" {
                !leader.arrow_enabled
            } else {
                value == "true"
            }
        }
        "hookline_enabled" => {
            leader.hookline_enabled = if value == "toggle" {
                !leader.hookline_enabled
            } else {
                value == "true"
            }
        }
        "hookline_direction" => {
            leader.hookline_direction = match value {
                "Same" => HooklineDirection::Same,
                _ => HooklineDirection::Opposite,
            };
        }
        "text_height" => {
            if let Some(v) = f64(value) {
                leader.text_height = v;
            }
        }
        "text_width" => {
            if let Some(v) = f64(value) {
                leader.text_width = v;
            }
        }
        "normal_x" => {
            if let Some(v) = f64(value) {
                leader.normal.x = v;
            }
        }
        "normal_y" => {
            if let Some(v) = f64(value) {
                leader.normal.y = v;
            }
        }
        "normal_z" => {
            if let Some(v) = f64(value) {
                leader.normal.z = v;
            }
        }
        "h_dir_x" => {
            if let Some(v) = f64(value) {
                leader.horizontal_direction.x = v;
            }
        }
        "h_dir_y" => {
            if let Some(v) = f64(value) {
                leader.horizontal_direction.y = v;
            }
        }
        "h_dir_z" => {
            if let Some(v) = f64(value) {
                leader.horizontal_direction.z = v;
            }
        }
        "block_offset_x" => {
            if let Some(v) = f64(value) {
                leader.block_offset.x = v;
            }
        }
        "block_offset_y" => {
            if let Some(v) = f64(value) {
                leader.block_offset.y = v;
            }
        }
        "block_offset_z" => {
            if let Some(v) = f64(value) {
                leader.block_offset.z = v;
            }
        }
        "ann_offset_x" => {
            if let Some(v) = f64(value) {
                leader.annotation_offset.x = v;
            }
        }
        "ann_offset_y" => {
            if let Some(v) = f64(value) {
                leader.annotation_offset.y = v;
            }
        }
        "ann_offset_z" => {
            if let Some(v) = f64(value) {
                leader.annotation_offset.z = v;
            }
        }
        "arrow_x" => {
            if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(0)) {
                vert.x = v;
            }
        }
        "arrow_y" => {
            if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(0)) {
                vert.y = v;
            }
        }
        "arrow_z" => {
            if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(0)) {
                vert.z = v;
            }
        }
        "end_x" => {
            let last = leader.vertices.len().saturating_sub(1);
            if last > 0 {
                if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(last)) {
                    vert.x = v;
                }
            }
        }
        "end_y" => {
            let last = leader.vertices.len().saturating_sub(1);
            if last > 0 {
                if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(last)) {
                    vert.y = v;
                }
            }
        }
        "end_z" => {
            let last = leader.vertices.len().saturating_sub(1);
            if last > 0 {
                if let (Some(v), Some(vert)) = (f64(value), leader.vertices.get_mut(last)) {
                    vert.z = v;
                }
            }
        }
        _ => {}
    }
}

// ── Transform ──────────────────────────────────────────────────────────────

fn apply_transform(leader: &mut Leader, t: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(leader, t, |entity, p1, p2| {
        for v in &mut entity.vertices {
            crate::scene::transform::reflect_xy_point(&mut v.x, &mut v.y, p1, p2);
        }
        crate::scene::transform::reflect_xy_point(
            &mut entity.block_offset.x,
            &mut entity.block_offset.y,
            p1,
            p2,
        );
        crate::scene::transform::reflect_xy_point(
            &mut entity.annotation_offset.x,
            &mut entity.annotation_offset.y,
            p1,
            p2,
        );
    });
}

// ── Trait impls ────────────────────────────────────────────────────────────

impl TruckConvertible for Leader {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        if self.vertices.is_empty() {
            return None;
        }
        Some(to_truck(self))
    }
}

impl Grippable for Leader {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for Leader {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Leader {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
