use acadrust::entities::{AttachmentPoint, DrawingDirection, MText};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, ro_prop as ro, square_grip, triangle_grip};
use crate::entities::text_support::{
    measure_mtext_chars, resolve_text_style, split_mtext_lines, strip_mtext_codes, word_wrap,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TextStroke, TruckEntity, TruckObject};
use crate::scene::cxf;
use crate::scene::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::wire_model::SnapHint;

fn attachment_str(a: &AttachmentPoint) -> &'static str {
    match a {
        AttachmentPoint::TopLeft => "Top Left",
        AttachmentPoint::TopCenter => "Top Center",
        AttachmentPoint::TopRight => "Top Right",
        AttachmentPoint::MiddleLeft => "Middle Left",
        AttachmentPoint::MiddleCenter => "Middle Center",
        AttachmentPoint::MiddleRight => "Middle Right",
        AttachmentPoint::BottomLeft => "Bottom Left",
        AttachmentPoint::BottomCenter => "Bottom Center",
        AttachmentPoint::BottomRight => "Bottom Right",
    }
}

fn mtext_halign_str(a: &AttachmentPoint) -> &'static str {
    match a {
        AttachmentPoint::TopLeft | AttachmentPoint::MiddleLeft | AttachmentPoint::BottomLeft => {
            "Left"
        }
        AttachmentPoint::TopCenter
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::BottomCenter => "Center",
        AttachmentPoint::TopRight | AttachmentPoint::MiddleRight | AttachmentPoint::BottomRight => {
            "Right"
        }
    }
}

fn mtext_valign_str(a: &AttachmentPoint) -> &'static str {
    match a {
        AttachmentPoint::TopLeft | AttachmentPoint::TopCenter | AttachmentPoint::TopRight => "Top",
        AttachmentPoint::MiddleLeft
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::MiddleRight => "Middle",
        AttachmentPoint::BottomLeft
        | AttachmentPoint::BottomCenter
        | AttachmentPoint::BottomRight => "Bottom",
    }
}

fn mtext_attachment_from_align(h: &str, v: &str) -> Option<AttachmentPoint> {
    Some(match (h, v) {
        ("Left", "Top") => AttachmentPoint::TopLeft,
        ("Center", "Top") => AttachmentPoint::TopCenter,
        ("Right", "Top") => AttachmentPoint::TopRight,
        ("Left", "Middle") => AttachmentPoint::MiddleLeft,
        ("Center", "Middle") => AttachmentPoint::MiddleCenter,
        ("Right", "Middle") => AttachmentPoint::MiddleRight,
        ("Left", "Bottom") => AttachmentPoint::BottomLeft,
        ("Center", "Bottom") => AttachmentPoint::BottomCenter,
        ("Right", "Bottom") => AttachmentPoint::BottomRight,
        _ => return None,
    })
}

fn drawing_dir_str(d: &DrawingDirection) -> &'static str {
    match d {
        DrawingDirection::LeftToRight => "Left to Right",
        DrawingDirection::TopToBottom => "Top to Bottom",
        DrawingDirection::ByStyle => "By Style",
    }
}

fn to_truck(t: &MText, document: &acadrust::CadDocument) -> TruckEntity {
    let resolved_style = resolve_text_style(&t.style, document);
    let font_name = resolved_style.font_name;
    let font = cxf::get_font(&font_name);
    let style_width_factor = resolved_style.width_factor.max(0.01);
    let style_oblique = resolved_style.oblique_angle;
    let plain = strip_mtext_codes(&t.value);
    let explicit_lines = split_mtext_lines(&plain);
    let lines: Vec<String> = if t.rectangle_width > 0.0 {
        let scale = t.height as f32 / 9.0 * style_width_factor;
        let max_w = t.rectangle_width as f32;
        explicit_lines
            .iter()
            .flat_map(|line| word_wrap(line, max_w, scale, font))
            .collect()
    } else {
        explicit_lines
    };
    let n_lines = lines.len().max(1) as f32;
    let ls_factor = if t.line_spacing_factor > 0.0 {
        t.line_spacing_factor as f32
    } else {
        1.0
    };
    // AutoCAD DXF code 44 is a multiplier on the *default* baseline-to-baseline
    // distance, which is 5/3 × text_height (≈ 1.667).  factor = 1.0 → single
    // spacing, factor = 2.0 → double spacing, etc.
    let line_h = t.height as f32 * ls_factor * (5.0 / 3.0) * font.line_spacing;
    let h = t.height as f32;
    // CXF glyphs sit on the baseline (y=0) and extend UP by `h` (cap height).
    // Block top   = line-0 baseline + h
    // Block bottom = last-line baseline = line-0 baseline − (n_lines−1)·line_h
    //
    // v_offset is the Y of line-0 baseline relative to the insertion point, so:
    //   Top    attachment → block top    at insertion → v_offset = −h
    //   Bottom attachment → block bottom at insertion → v_offset = (n_lines−1)·line_h
    //   Middle attachment → block center at insertion → midpoint of the two above
    let v_offset = match t.attachment_point {
        AttachmentPoint::TopLeft | AttachmentPoint::TopCenter | AttachmentPoint::TopRight => -h,
        AttachmentPoint::MiddleLeft
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::MiddleRight => ((n_lines - 1.0) * line_h - h) * 0.5,
        AttachmentPoint::BottomLeft
        | AttachmentPoint::BottomCenter
        | AttachmentPoint::BottomRight => (n_lines - 1.0) * line_h,
    };
    let h_anchor = match t.attachment_point {
        AttachmentPoint::TopCenter
        | AttachmentPoint::MiddleCenter
        | AttachmentPoint::BottomCenter => 0.5,
        AttachmentPoint::TopRight | AttachmentPoint::MiddleRight | AttachmentPoint::BottomRight => {
            1.0
        }
        _ => 0.0,
    };
    let vertical_text = matches!(t.drawing_direction, DrawingDirection::TopToBottom);
    let rot = t.rotation as f32;
    let (cos_r, sin_r) = (rot.cos(), rot.sin());
    let ins_x = t.insertion_point.x;
    let ins_y = t.insertion_point.y;
    let insertion = Vec3::new(ins_x as f32, ins_y as f32, t.insertion_point.z as f32);
    let mut all_strokes: Vec<TextStroke> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let li = i as f32;
        // Compute line offset as f32 (small relative values), apply to f64 insertion point.
        let (small_x, small_y) = if vertical_text {
            let col_offset = li * t.height as f32 * 1.2;
            (
                col_offset * cos_r + v_offset * (-sin_r),
                col_offset * sin_r + v_offset * cos_r,
            )
        } else {
            let line_y = -(li * line_h) + v_offset;
            (line_y * (-sin_r), line_y * cos_r)
        };
        let line_w = if h_anchor > 0.0 {
            let scale = t.height as f32 / 9.0 * style_width_factor;
            measure_mtext_chars(line, scale, font)
        } else {
            0.0
        };
        let h_shift = -line_w * h_anchor;
        let origin: [f64; 2] = [
            ins_x + (small_x + h_shift * cos_r) as f64,
            ins_y + (small_y + h_shift * sin_r) as f64,
        ];
        let strokes = cxf::tessellate_text_ex(
            [0.0, 0.0],
            t.height as f32,
            rot,
            style_width_factor,
            style_oblique,
            &font_name,
            line,
        );
        all_strokes.push(TextStroke { strokes, origin });
    }
    TruckEntity {
        object: TruckObject::Text(all_strokes),
        snap_pts: vec![(insertion, SnapHint::Insertion)],
        tangent_geoms: vec![],
        key_vertices: vec![],
    }
}

fn grips(t: &MText) -> Vec<GripDef> {
    let p = Vec3::new(
        t.insertion_point.x as f32,
        t.insertion_point.y as f32,
        t.insertion_point.z as f32,
    );
    let dir = Vec3::new((t.rotation as f32).cos(), (t.rotation as f32).sin(), 0.0);
    let width_grip = p + dir * t.rectangle_width.max(0.0) as f32;
    vec![square_grip(0, p), triangle_grip(1, width_grip)]
}

fn properties(t: &MText, text_style_names: &[String]) -> PropSection {
    PropSection {
        title: "Geometry".into(),
        props: vec![
            edit("Insert X", "ins_x", t.insertion_point.x),
            edit("Insert Y", "ins_y", t.insertion_point.y),
            edit("Insert Z", "ins_z", t.insertion_point.z),
            edit("Height", "height", t.height),
            edit("Width", "rect_w", t.rectangle_width),
            edit("Rect Height", "rect_h", t.rectangle_height.unwrap_or(0.0)),
            edit("Rotation", "rotation", t.rotation.to_degrees()),
            edit("Line Spacing", "line_spacing", t.line_spacing_factor),
            Property {
                label: "H-Align".into(),
                field: "h_align",
                value: PropValue::Choice {
                    selected: mtext_halign_str(&t.attachment_point).to_string(),
                    options: ["Left", "Center", "Right"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                },
            },
            Property {
                label: "V-Align".into(),
                field: "v_align",
                value: PropValue::Choice {
                    selected: mtext_valign_str(&t.attachment_point).to_string(),
                    options: ["Top", "Middle", "Bottom"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                },
            },
            ro(
                "Attachment",
                "attachment",
                attachment_str(&t.attachment_point).to_string(),
            ),
            ro(
                "Direction",
                "direction",
                drawing_dir_str(&t.drawing_direction).to_string(),
            ),
            Property {
                label: "Content".into(),
                field: "content",
                value: PropValue::EditText(t.value.clone()),
            },
            Property {
                label: "Style".into(),
                field: "style",
                value: PropValue::Choice {
                    selected: if t.style.trim().is_empty() {
                        "Standard".into()
                    } else {
                        t.style.clone()
                    },
                    options: text_style_names.to_vec(),
                },
            },
        ],
    }
}

fn apply_geom_prop(t: &mut MText, field: &str, value: &str) {
    match field {
        "content" => {
            t.value = value.to_string();
            return;
        }
        "style" => {
            t.style = value.to_string();
            return;
        }
        "h_align" => {
            if let Some(next) =
                mtext_attachment_from_align(value, mtext_valign_str(&t.attachment_point))
            {
                t.attachment_point = next;
            }
            return;
        }
        "v_align" => {
            if let Some(next) =
                mtext_attachment_from_align(mtext_halign_str(&t.attachment_point), value)
            {
                t.attachment_point = next;
            }
            return;
        }
        _ => {}
    }
    let Some(v) = crate::entities::common::parse_f64(value) else {
        return;
    };
    match field {
        "ins_x" => t.insertion_point.x = v,
        "ins_y" => t.insertion_point.y = v,
        "ins_z" => t.insertion_point.z = v,
        "height" if v > 0.0 => t.height = v,
        "rect_w" if v > 0.0 => t.rectangle_width = v,
        "rect_h" if v > 0.0 => t.rectangle_height = Some(v),
        "rotation" => t.rotation = v.to_radians(),
        "line_spacing" if v > 0.0 => t.line_spacing_factor = v,
        _ => {}
    }
}

fn apply_grip(t: &mut MText, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Absolute(p)) => {
            t.insertion_point.x = p.x as f64;
            t.insertion_point.y = p.y as f64;
            t.insertion_point.z = p.z as f64;
        }
        (0, GripApply::Translate(d)) => {
            t.insertion_point.x += d.x as f64;
            t.insertion_point.y += d.y as f64;
            t.insertion_point.z += d.z as f64;
        }
        (1, GripApply::Absolute(p)) => {
            let dir_x = t.rotation.cos();
            let dir_y = t.rotation.sin();
            let dx = p.x as f64 - t.insertion_point.x;
            let dy = p.y as f64 - t.insertion_point.y;
            let projected = dx * dir_x + dy * dir_y;
            t.rectangle_width = projected.max(0.01);
        }
        _ => {}
    }
}

fn apply_transform(t: &mut MText, tr: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(t, tr, |entity, p1, p2| {
        crate::scene::transform::reflect_xy_point(
            &mut entity.insertion_point.x,
            &mut entity.insertion_point.y,
            p1,
            p2,
        );
        let dx = (p2.x - p1.x) as f64;
        let dy = (p2.y - p1.y) as f64;
        let line_angle = dy.atan2(dx);
        entity.rotation = 2.0 * line_angle - entity.rotation;
    });
}

impl TruckConvertible for MText {
    fn to_truck(&self, document: &acadrust::CadDocument) -> Option<TruckEntity> {
        Some(to_truck(self, document))
    }
}

impl Grippable for MText {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for MText {
    fn geometry_properties(&self, text_style_names: &[String]) -> PropSection {
        properties(self, text_style_names)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for MText {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
