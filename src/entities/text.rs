use acadrust::entities::{Text, TextHorizontalAlignment as HA, TextVerticalAlignment as VA};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, parse_f64, square_grip};
use crate::entities::text_support::{
    resolve_dxf_special_chars, resolve_text_style, text_local_bounds,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TextStroke, TruckEntity, TruckObject};
use crate::scene::lff;
use crate::scene::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::wire_model::SnapHint;

fn text_halign_str(a: &acadrust::entities::TextHorizontalAlignment) -> &'static str {
    use acadrust::entities::TextHorizontalAlignment::*;
    match a {
        Left => "Left",
        Center => "Center",
        Right => "Right",
        Aligned => "Aligned",
        Middle => "Middle",
        Fit => "Fit",
    }
}

fn text_valign_str(a: &acadrust::entities::TextVerticalAlignment) -> &'static str {
    use acadrust::entities::TextVerticalAlignment::*;
    match a {
        Baseline => "Baseline",
        Bottom => "Bottom",
        Middle => "Middle",
        Top => "Top",
    }
}

fn sync_text_alignment_point(t: &mut Text) {
    let needs_alignment_point = !matches!(
        (t.horizontal_alignment, t.vertical_alignment),
        (HA::Left, VA::Baseline)
    );
    if needs_alignment_point {
        if t.alignment_point.is_none() {
            t.alignment_point = Some(t.insertion_point);
        }
    } else {
        t.alignment_point = None;
    }
}

fn to_truck(t: &Text, document: &acadrust::CadDocument) -> TruckEntity {
    let normal = (t.normal.x, t.normal.y, t.normal.z);
    let (wsx, wsy, wsz) = crate::scene::transform::ocs_point_to_wcs(
        (
            t.insertion_point.x,
            t.insertion_point.y,
            t.insertion_point.z,
        ),
        normal,
    );
    let snap_pt = Vec3::new(wsx as f32, wsy as f32, wsz as f32);
    let resolved_style = resolve_text_style(&t.style, document);
    let font_name = resolved_style.font_name;
    // AutoCAD text geometry rule: the entity stores the FINAL width factor /
    // oblique angle, copied from the style at creation and persisting through
    // style edits. Use it as-is. Only fall back to the style when the entity
    // value is missing (the parser reports 0.0 for default-omitted fields).
    let base_wf = if t.width_factor.abs() > 1e-9 {
        (t.width_factor as f32).clamp(0.01, 100.0)
    } else {
        resolved_style.width_factor.max(0.01)
    };
    // is_backward mirrors text left-right via negative width factor.
    let width_factor = if resolved_style.is_backward {
        -base_wf
    } else {
        base_wf
    };
    // is_upside_down rotates 180° around the insertion point.
    let rotation = if resolved_style.is_upside_down {
        t.rotation as f32 + std::f32::consts::PI
    } else {
        t.rotation as f32
    };
    let oblique_angle = if t.oblique_angle.abs() > 1e-9 {
        t.oblique_angle as f32
    } else {
        resolved_style.oblique_angle
    };
    let anchor = match (
        &t.horizontal_alignment,
        &t.vertical_alignment,
        &t.alignment_point,
    ) {
        (HA::Aligned | HA::Middle | HA::Fit, _, Some(a)) => [a.x as f32, a.y as f32],
        (HA::Center | HA::Right, _, Some(a)) => [a.x as f32, a.y as f32],
        (_, VA::Bottom | VA::Middle | VA::Top, Some(a)) => [a.x as f32, a.y as f32],
        _ => [t.insertion_point.x as f32, t.insertion_point.y as f32],
    };
    // Strip %%u/%%o for bounds (they add no width); resolve %%d/%%c/%%p for correct advance.
    let value_for_bounds = resolve_dxf_special_chars(&t.value);
    let bounds = text_local_bounds(
        &font_name,
        &value_for_bounds,
        t.height as f32,
        width_factor,
        oblique_angle,
    );
    let (anchor_local_x, anchor_local_y) = if let Some(([min_x, min_y], [max_x, max_y])) = bounds {
        let ax = match t.horizontal_alignment {
            HA::Left => min_x,
            HA::Center | HA::Middle => (min_x + max_x) * 0.5,
            HA::Right | HA::Aligned | HA::Fit => max_x,
        };
        let ay = match t.vertical_alignment {
            VA::Baseline => 0.0,
            VA::Bottom => min_y,
            VA::Middle => (min_y + max_y) * 0.5,
            VA::Top => max_y,
        };
        (ax, ay)
    } else {
        (0.0, 0.0)
    };
    let (cos_r, sin_r) = (rotation.cos() as f64, rotation.sin() as f64);
    // Keep origin as f64 — large coordinates (UTM etc.) must not be cast to
    // f32 here; world_offset subtraction happens later in tessellate.rs.
    let anchor_f64 = [anchor[0] as f64, anchor[1] as f64];
    let origin: [f64; 2] = [
        anchor_f64[0] - (anchor_local_x as f64 * cos_r - anchor_local_y as f64 * sin_r),
        anchor_f64[1] - (anchor_local_x as f64 * sin_r + anchor_local_y as f64 * cos_r),
    ];
    // Strokes are in glyph-local space (origin = [0,0]).
    let strokes = lff::tessellate_text_ex(
        [0.0, 0.0],
        t.height as f32,
        rotation,
        width_factor,
        oblique_angle,
        &font_name,
        &t.value,
    );
    TruckEntity {
        object: TruckObject::Text(vec![TextStroke { strokes, origin, color: None }]),
        snap_pts: vec![(snap_pt, SnapHint::Insertion)],
        tangent_geoms: vec![],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

fn grips(t: &Text) -> Vec<GripDef> {
    let p = Vec3::new(
        t.insertion_point.x as f32,
        t.insertion_point.y as f32,
        t.insertion_point.z as f32,
    );
    vec![square_grip(0, p)]
}

fn properties(t: &Text, text_style_names: &[String]) -> PropSection {
    PropSection {
        title: "Geometry".into(),
        props: vec![
            edit("Insert X", "ins_x", t.insertion_point.x),
            edit("Insert Y", "ins_y", t.insertion_point.y),
            edit("Insert Z", "ins_z", t.insertion_point.z),
            edit("Height", "height", t.height),
            edit("Rotation", "rotation", t.rotation.to_degrees()),
            edit("Width Factor", "width_factor", t.width_factor),
            edit(
                "Oblique Angle",
                "oblique_angle",
                t.oblique_angle.to_degrees(),
            ),
            Property {
                label: "H-Align".into(),
                field: "h_align",
                value: PropValue::Choice {
                    selected: text_halign_str(&t.horizontal_alignment).to_string(),
                    options: ["Left", "Center", "Right", "Aligned", "Middle", "Fit"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                },
            },
            Property {
                label: "V-Align".into(),
                field: "v_align",
                value: PropValue::Choice {
                    selected: text_valign_str(&t.vertical_alignment).to_string(),
                    options: ["Baseline", "Bottom", "Middle", "Top"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                },
            },
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

fn apply_geom_prop(t: &mut Text, field: &str, value: &str) {
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
            t.horizontal_alignment = match value {
                "Left" => HA::Left,
                "Center" => HA::Center,
                "Right" => HA::Right,
                "Aligned" => HA::Aligned,
                "Middle" => HA::Middle,
                "Fit" => HA::Fit,
                _ => return,
            };
            sync_text_alignment_point(t);
            return;
        }
        "v_align" => {
            t.vertical_alignment = match value {
                "Baseline" => VA::Baseline,
                "Bottom" => VA::Bottom,
                "Middle" => VA::Middle,
                "Top" => VA::Top,
                _ => return,
            };
            sync_text_alignment_point(t);
            return;
        }
        _ => {}
    }
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "ins_x" => t.insertion_point.x = v,
        "ins_y" => t.insertion_point.y = v,
        "ins_z" => t.insertion_point.z = v,
        "height" if v > 0.0 => t.height = v,
        "rotation" => t.rotation = v.to_radians(),
        "width_factor" if v > 0.0 => t.width_factor = v,
        "oblique_angle" => t.oblique_angle = v.to_radians(),
        _ => {}
    }
}

fn apply_grip(t: &mut Text, _grip_id: usize, apply: GripApply) {
    match apply {
        GripApply::Absolute(p) => {
            t.insertion_point.x = p.x as f64;
            t.insertion_point.y = p.y as f64;
            t.insertion_point.z = p.z as f64;
        }
        GripApply::Translate(d) => {
            t.insertion_point.x += d.x as f64;
            t.insertion_point.y += d.y as f64;
            t.insertion_point.z += d.z as f64;
        }
    }
}

fn apply_transform(t: &mut Text, tr: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(t, tr, |entity, p1, p2| {
        crate::scene::transform::reflect_xy_point(
            &mut entity.insertion_point.x,
            &mut entity.insertion_point.y,
            p1,
            p2,
        );
        if let Some(ref mut a) = entity.alignment_point {
            crate::scene::transform::reflect_xy_point(&mut a.x, &mut a.y, p1, p2);
        }
        let dx = (p2.x - p1.x) as f64;
        let dy = (p2.y - p1.y) as f64;
        let line_angle = dy.atan2(dx);
        entity.rotation = 2.0 * line_angle - entity.rotation;
        entity.oblique_angle = -entity.oblique_angle;
    });
}

impl TruckConvertible for Text {
    fn to_truck(&self, document: &acadrust::CadDocument) -> Option<TruckEntity> {
        Some(to_truck(self, document))
    }
}

impl Grippable for Text {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }

    fn grip_menu(
        &self,
        _grip_id: usize,
    ) -> Vec<crate::scene::object::GripMenuItem> {
        use crate::scene::object::{GripMenuAction, GripMenuItem};
        vec![
            GripMenuItem { label: "Stretch", action: GripMenuAction::Stretch },
            GripMenuItem { label: "Move with Text", action: GripMenuAction::MoveWithText },
            GripMenuItem { label: "Rotate", action: GripMenuAction::RotateText },
        ]
    }

    fn apply_grip_menu(
        &mut self,
        _grip_id: usize,
        _action: crate::scene::object::GripMenuAction,
    ) {
        // Move-with-Text falls through to Stretch (single grip moves
        // the whole text); Rotate needs a follow-up angle handled by
        // `apply_grip_menu_value`.
    }

    fn grip_menu_value_prompt(
        &self,
        _grip_id: usize,
        action: crate::scene::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::object::GripMenuAction as A;
        match action {
            A::RotateText => Some("Rotation (deg)"),
            _ => None,
        }
    }

    fn apply_grip_menu_value(
        &mut self,
        _grip_id: usize,
        action: crate::scene::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::object::GripMenuAction as A;
        if matches!(action, A::RotateText) {
            self.rotation = value.to_radians();
        }
    }
}

impl PropertyEditable for Text {
    fn geometry_properties(&self, text_style_names: &[String]) -> PropSection {
        properties(self, text_style_names)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Text {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

impl crate::entities::traits::TextContent for acadrust::entities::Text {
    fn text_content(&self) -> Option<String> {
        Some(self.value.clone())
    }
    fn replace_text(&mut self, search: &str, rep: &str) {
        let search_lc = search.to_lowercase();
        if self.value.to_lowercase().contains(&search_lc) {
            self.value = self.value.replace(search, rep);
        }
    }
}
