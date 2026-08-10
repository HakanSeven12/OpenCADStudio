use acadrust::entities::{Text, TextHorizontalAlignment as HA, TextVerticalAlignment as VA};

use crate::command::EntityTransform;
use crate::entities::common::{
    edit_angle_prop as edit_angle, edit_prop as edit, num_prop as num_row, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::text_support::{
    resolve_dxf_special_chars, resolve_text_style, text_local_bounds,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{GlyphRun, TextStroke, RenderEntity, RenderObject};
use crate::scene::text::lff;
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;
use crate::t;

/// Combined single-line-text justification (horizontal × vertical) shown as one
/// dropdown value. Horizontal-only modes (Aligned/Middle/Fit) ignore the
/// vertical component the way the underlying alignment does.
fn text_justify_str(h: &HA, v: &VA) -> &'static str {
    match (h, v) {
        (HA::Aligned, _) => "Aligned",
        (HA::Fit, _) => "Fit",
        (HA::Middle, _) => "Middle",
        (HA::Left, VA::Baseline) => "Left",
        (HA::Center, VA::Baseline) => "Center",
        (HA::Right, VA::Baseline) => "Right",
        (HA::Left, VA::Top) => "Top left",
        (HA::Center, VA::Top) => "Top center",
        (HA::Right, VA::Top) => "Top right",
        (HA::Left, VA::Middle) => "Middle left",
        (HA::Center, VA::Middle) => "Middle center",
        (HA::Right, VA::Middle) => "Middle right",
        (HA::Left, VA::Bottom) => "Bottom left",
        (HA::Center, VA::Bottom) => "Bottom center",
        (HA::Right, VA::Bottom) => "Bottom right",
    }
}

pub(crate) fn sync_text_alignment_point(t: &mut Text) {
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

/// Resolved placement of a TEXT run: the baseline-anchored run origin (WCS xy)
/// plus every parameter needed to lay the glyphs out. Shared by `to_render` (the
/// stroke path) and the SDF-quad text collector so both place text identically.
pub struct TextPlacement {
    /// Run-local origin (glyph space `[0,0]` maps here), WCS xy, kept f64.
    pub origin: [f64; 2],
    pub height: f32,
    pub rotation: f32,
    pub width_factor: f32,
    pub oblique_angle: f32,
    pub font: String,
    /// Raw entity value (as passed to the tessellator).
    pub value: String,
    /// Full WCS insertion point, for the Insertion snap.
    pub wcs_insertion: [f64; 3],
}

/// Parse a TEXT value's `%%` control codes through acadrust's `parse_plain_text`
/// (the same parser MTEXT uses), then re-encode into the stroke tessellator's
/// inline grammar: specials arrive resolved to Unicode, and `%%u`/`%%o`
/// underline/overline become `\L…\l` / `\O…\o` decoration markers. This keeps
/// TEXT parsing in acadrust rather than OCS's own tokenizer.
pub(crate) fn acad_text_encode(value: &str) -> String {
    use acadrust::entities::mtext_format::parse_plain_text;
    let doc = parse_plain_text(value);
    let mut out = String::new();
    for para in &doc.paragraphs {
        for span in &para.spans {
            let (u, o) = (span.properties.underline(), span.properties.overline());
            if u {
                out.push_str("\\L");
            }
            if o {
                out.push_str("\\O");
            }
            out.push_str(&span.text);
            if o {
                out.push_str("\\o");
            }
            if u {
                out.push_str("\\l");
            }
        }
    }
    out
}

fn to_render(t: &Text, document: &acadrust::CadDocument) -> RenderEntity {
    let p = text_run_placement(t, document);
    let snap_pt = glam::DVec3::new(p.wcs_insertion[0], p.wcs_insertion[1], p.wcs_insertion[2]);
    // Parse `%%` codes via acadrust, re-encoded for the stroke tessellator.
    let value = acad_text_encode(&p.value);
    // Strokes are in glyph-local space (origin = [0,0]).
    let (strokes, fill_tris) = lff::tessellate_text_ex(
        [0.0, 0.0],
        p.height,
        p.rotation,
        p.width_factor,
        p.oblique_angle,
        &p.font,
        &value,
    );
    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Text(vec![TextStroke {
            strokes,
            origin: p.origin,
            color: None,
            fill_tris,
            run: Some(GlyphRun {
                text: value,
                font: p.font.clone(),
                height: p.height,
                rotation: p.rotation,
                width_factor: p.width_factor,
                oblique: p.oblique_angle,
                tracking: 1.0,
                bold: false,
            }),
        }]),
        snap_pts: vec![(snap_pt, SnapHint::Insertion)],
        tangent_geoms: vec![],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

/// Compute a TEXT entity's run placement (origin + layout params). Extracted
/// from `to_render` verbatim so the stroke and SDF-quad paths agree exactly.
pub fn text_run_placement(t: &Text, document: &acadrust::CadDocument) -> TextPlacement {
    let normal = (t.normal.x, t.normal.y, t.normal.z);
    let (wsx, wsy, wsz) = crate::scene::view::transform::ocs_point_to_wcs(
        (
            t.insertion_point.x,
            t.insertion_point.y,
            t.insertion_point.z,
        ),
        normal,
    );
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
    // Backward mirrors text left-right (negative width factor); upside-down
    // rotates 180° about the anchor. The effective state is the TextStyle flag
    // XOR the entity's own generation flags (DXF group 71: bit 2 = backward,
    // bit 4 = upside-down). A MIRROR toggles the entity bit for a true glyph
    // mirror, and XOR keeps a double mirror an involution.
    let eff_backward = resolved_style.is_backward ^ (t.generation_flags & 0x2 != 0);
    let eff_upside = resolved_style.is_upside_down ^ (t.generation_flags & 0x4 != 0);
    let width_factor = if eff_backward { -base_wf } else { base_wf };
    let rotation = if eff_upside {
        t.rotation as f32 + std::f32::consts::PI
    } else {
        t.rotation as f32
    };
    let oblique_angle = if t.oblique_angle.abs() > 1e-9 {
        t.oblique_angle as f32
    } else {
        resolved_style.oblique_angle
    };
    // Anchor stays f64: large coordinates (UTM etc.) lose ~0.5 units of
    // precision when cast to f32, which snaps text baselines onto a coarse
    // grid and makes adjacent rows collide. Only the small local offsets
    // below are computed in f32.
    let anchor: [f64; 2] = match (
        &t.horizontal_alignment,
        &t.vertical_alignment,
        &t.alignment_point,
    ) {
        (HA::Aligned | HA::Middle | HA::Fit, _, Some(a)) => [a.x, a.y],
        (HA::Center | HA::Right, _, Some(a)) => [a.x, a.y],
        (_, VA::Bottom | VA::Middle | VA::Top, Some(a)) => [a.x, a.y],
        _ => [t.insertion_point.x, t.insertion_point.y],
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
    let (anchor_local_x, anchor_local_y) = if let Some(b) = bounds {
        // Horizontal anchor uses the pen advance box [0, advance] so leading /
        // trailing spaces keep their width instead of snapping to the inked
        // glyphs. Signed by the width factor: backward text grows in −x, so its
        // Center/Right offset flips sign too, otherwise the anchor pushes the
        // box one way while the strokes run the other (the bounds advance is
        // always positive — it uses |width_factor|). Left keeps its 0 reference.
        let sign = width_factor.signum();
        let ax = match t.horizontal_alignment {
            HA::Left => 0.0,
            HA::Center | HA::Middle => b.advance * 0.5 * sign,
            HA::Right | HA::Aligned | HA::Fit => b.advance * sign,
        };
        // Vertical anchor uses the inked extent (cap / baseline geometry).
        let ay = match t.vertical_alignment {
            VA::Baseline => 0.0,
            VA::Bottom => b.ink_min[1],
            VA::Middle => (b.ink_min[1] + b.ink_max[1]) * 0.5,
            VA::Top => b.ink_max[1],
        };
        (ax, ay)
    } else {
        (0.0, 0.0)
    };
    let (cos_r, sin_r) = (rotation.cos() as f64, rotation.sin() as f64);
    // Keep origin as f64 — large coordinates (UTM etc.) must not be cast to
    // f32 here; world_offset subtraction happens later in tessellate.rs.
    let anchor_f64 = anchor;
    let origin: [f64; 2] = [
        anchor_f64[0] - (anchor_local_x as f64 * cos_r - anchor_local_y as f64 * sin_r),
        anchor_f64[1] - (anchor_local_x as f64 * sin_r + anchor_local_y as f64 * cos_r),
    ];
    TextPlacement {
        origin,
        height: t.height as f32,
        rotation,
        width_factor,
        oblique_angle,
        font: font_name,
        value: t.value.clone(),
        wcs_insertion: [wsx, wsy, wsz],
    }
}

fn grips(t: &Text) -> Vec<GripDef> {
    let p = glam::DVec3::new(
        t.insertion_point.x,
        t.insertion_point.y,
        t.insertion_point.z,
    );
    vec![square_grip(0, p)]
}

fn properties(t: &Text, text_style_names: &[String]) -> Vec<PropSection> {
    // Which geometry rows are live depends on justification. Plain Left text is
    // anchored by the insertion point and has no second alignment point; every
    // other justification anchors on the alignment point (and recomputes the
    // insertion point), except Aligned/Fit which are true two-point spans where
    // both points are live.
    let is_plain_left = matches!(t.horizontal_alignment, HA::Left)
        && matches!(t.vertical_alignment, VA::Baseline);
    let pos_editable = is_plain_left || matches!(t.horizontal_alignment, HA::Aligned | HA::Fit);
    let align_editable = !is_plain_left;
    // The alignment point is meaningless (reset to the origin) for plain-Left text.
    let ap = t.alignment_point.unwrap_or(t.insertion_point);
    let (ax, ay, az) = if align_editable {
        (ap.x, ap.y, ap.z)
    } else {
        (0.0, 0.0, 0.0)
    };
    vec![
        PropSection {
            title: t!("Text").into_owned(),
            props: vec![
                Property {
                    label: t!("Contents").into_owned(),
                    field: "content",
                    value: PropValue::EditText(t.value.clone()),
                },
                Property {
                    label: t!("Style").into_owned(),
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
                ro(t!("Annotative").as_ref(), "annotative", "No"),
                Property {
                    label: t!("Justify").into_owned(),
                    field: "justify",
                    value: PropValue::Choice {
                        selected: text_justify_str(
                            &t.horizontal_alignment,
                            &t.vertical_alignment,
                        )
                        .to_string(),
                        options: [
                            "Left",
                            "Center",
                            "Right",
                            "Aligned",
                            "Middle",
                            "Fit",
                            "Top left",
                            "Top center",
                            "Top right",
                            "Middle left",
                            "Middle center",
                            "Middle right",
                            "Bottom left",
                            "Bottom center",
                            "Bottom right",
                        ]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    },
                },
                // A style that fixes its height fixes this text's too, so the
                // value shows but cannot be changed here.
                crate::entities::common::num_prop(
                    t!("Height").as_ref(),
                    "height",
                    t.height,
                    crate::entities::common::style_fixed_height(&t.style).is_none(),
                ),
                edit_angle(t!("Rotation").as_ref(), "rotation", t.rotation.to_degrees()),
                edit(t!("Width factor").as_ref(), "width_factor", t.width_factor),
                edit_angle(t!("Obliquing").as_ref(), "oblique_angle", t.oblique_angle.to_degrees()),
                num_row(t!("Text alignment X").as_ref(), "align_x", ax, align_editable),
                num_row(t!("Text alignment Y").as_ref(), "align_y", ay, align_editable),
                num_row(t!("Text alignment Z").as_ref(), "align_z", az, align_editable),
            ],
        },
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                num_row(t!("Position X").as_ref(), "ins_x", t.insertion_point.x, pos_editable),
                num_row(t!("Position Y").as_ref(), "ins_y", t.insertion_point.y, pos_editable),
                num_row(t!("Position Z").as_ref(), "ins_z", t.insertion_point.z, pos_editable),
            ],
        },
        PropSection {
            title: t!("Misc").into_owned(),
            props: vec![
                Property {
                    label: t!("Upside down").into_owned(),
                    field: "upside_down",
                    value: PropValue::BoolToggle {
                        field: "upside_down",
                        value: t.generation_flags & 0x4 != 0,
                    },
                },
                Property {
                    label: t!("Backward").into_owned(),
                    field: "backward",
                    value: PropValue::BoolToggle {
                        field: "backward",
                        value: t.generation_flags & 0x2 != 0,
                    },
                },
            ],
        },
    ]
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
        "justify" => {
            let (h, v) = match value {
                "Left" => (HA::Left, VA::Baseline),
                "Center" => (HA::Center, VA::Baseline),
                "Right" => (HA::Right, VA::Baseline),
                "Aligned" => (HA::Aligned, VA::Baseline),
                "Middle" => (HA::Middle, VA::Baseline),
                "Fit" => (HA::Fit, VA::Baseline),
                "Top left" => (HA::Left, VA::Top),
                "Top center" => (HA::Center, VA::Top),
                "Top right" => (HA::Right, VA::Top),
                "Middle left" => (HA::Left, VA::Middle),
                "Middle center" => (HA::Center, VA::Middle),
                "Middle right" => (HA::Right, VA::Middle),
                "Bottom left" => (HA::Left, VA::Bottom),
                "Bottom center" => (HA::Center, VA::Bottom),
                "Bottom right" => (HA::Right, VA::Bottom),
                _ => return,
            };
            t.horizontal_alignment = h;
            t.vertical_alignment = v;
            sync_text_alignment_point(t);
            return;
        }
        "upside_down" => {
            let set = if value == "toggle" {
                t.generation_flags & 0x4 == 0
            } else {
                value == "true"
            };
            if set {
                t.generation_flags |= 0x4;
            } else {
                t.generation_flags &= !0x4;
            }
            return;
        }
        "backward" => {
            let set = if value == "toggle" {
                t.generation_flags & 0x2 == 0
            } else {
                value == "true"
            };
            if set {
                t.generation_flags |= 0x2;
            } else {
                t.generation_flags &= !0x2;
            }
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
        "align_x" | "align_y" | "align_z" => {
            let ins = t.insertion_point;
            let ap = t.alignment_point.get_or_insert(ins);
            match field {
                "align_x" => ap.x = v,
                "align_y" => ap.y = v,
                _ => ap.z = v,
            }
        }
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
    crate::scene::view::transform::apply_standard_entity_transform(t, tr, |entity, p1, p2| {
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.insertion_point.x,
            &mut entity.insertion_point.y,
            p1,
            p2,
        );
        if let Some(ref mut a) = entity.alignment_point {
            crate::scene::view::transform::reflect_xy_point(&mut a.x, &mut a.y, p1, p2);
        }
        let dx = (p2.x - p1.x) as f64;
        let dy = (p2.y - p1.y) as f64;
        let line_angle = dy.atan2(dx);
        entity.rotation = 2.0 * line_angle - entity.rotation;
        entity.oblique_angle = -entity.oblique_angle;
    });
}

impl RenderConvertible for Text {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self, document))
    }
}

impl Grippable for Text {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }

    fn grip_menu(&self, _grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Move with Text",
                action: GripMenuAction::MoveWithText,
            },
            GripMenuItem {
                label: "Rotate",
                action: GripMenuAction::RotateText,
            },
        ]
    }

    fn apply_grip_menu(&mut self, _grip_id: usize, _action: crate::scene::model::object::GripMenuAction) {
        // Move-with-Text falls through to Stretch (single grip moves
        // the whole text); Rotate needs a follow-up angle handled by
        // `apply_grip_menu_value`.
    }

    fn grip_menu_value_prompt(
        &self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::model::object::GripMenuAction as A;
        match action {
            A::RotateText => Some("Rotation (deg)"),
            _ => None,
        }
    }

    fn apply_grip_menu_value(
        &mut self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        if matches!(action, A::RotateText) {
            self.rotation = value.to_radians();
        }
    }
}

impl PropertyEditable for Text {
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection> {
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
