use codec::entities::attribute_definition::{
    HorizontalAlignment as AHA, MTextFlag, VerticalAlignment as AVA,
};
use codec::entities::{AttributeDefinition, AttributeEntity};
use codec::types::Vector3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_angle_prop as edit_angle, edit_prop as edit, parse_f64, ro_prop as ro, square_grip};
use crate::entities::text_support::{
    layout_mtext, resolve_dxf_special_chars, resolve_text_style, text_local_bounds,
    MTextRenderOpts, MTextVAnchor, ResolvedTextStyle,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{GlyphRun, TextStroke, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;
use crate::scene::text::lff;
use crate::scene::view::transform;
use crate::t;

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Bundle of the fields both attribute kinds carry. Lets the render builder
/// stay generic over ATTDEF vs ATTRIB.
struct AttrTextInputs<'a> {
    value: &'a str,
    insertion_point: Vector3,
    alignment_point: Vector3,
    height: f64,
    rotation: f64,
    width_factor: f64,
    oblique_angle: f64,
    text_style: &'a str,
    text_generation_flags: i16,
    horizontal_alignment: AHA,
    vertical_alignment: AVA,
    normal: Vector3,
    mtext_flag: MTextFlag,
    is_multiline: bool,
    line_count: i16,
}

fn halign_str(a: AHA) -> &'static str {
    match a {
        AHA::Left => "Left",
        AHA::Center => "Center",
        AHA::Right => "Right",
        AHA::Aligned => "Aligned",
        AHA::Middle => "Middle",
        AHA::Fit => "Fit",
    }
}

fn valign_str(a: AVA) -> &'static str {
    match a {
        AVA::Baseline => "Baseline",
        AVA::Bottom => "Bottom",
        AVA::Middle => "Middle",
        AVA::Top => "Top",
    }
}

fn parse_halign(s: &str) -> Option<AHA> {
    Some(match s {
        "Left" => AHA::Left,
        "Center" => AHA::Center,
        "Right" => AHA::Right,
        "Aligned" => AHA::Aligned,
        "Middle" => AHA::Middle,
        "Fit" => AHA::Fit,
        _ => return None,
    })
}

fn parse_valign(s: &str) -> Option<AVA> {
    Some(match s {
        "Baseline" => AVA::Baseline,
        "Bottom" => AVA::Bottom,
        "Middle" => AVA::Middle,
        "Top" => AVA::Top,
        _ => return None,
    })
}

fn bool_yn(b: bool) -> &'static str {
    if b {
        "Yes"
    } else {
        "No"
    }
}

fn mtext_flag_str(f: MTextFlag) -> &'static str {
    match f {
        MTextFlag::SingleLine => "SingleLine",
        MTextFlag::MultiLine => "MultiLine",
        MTextFlag::ConstantMultiLine => "ConstantMultiLine",
    }
}

/// Render text strokes for an attribute, honouring alignment, oblique angle,
/// width factor, generation flags (backward / upside-down), text-style
/// resolution, and basic multiline splitting on `\n` / `\\P`.
/// Laid-out character cells of a multi-line (MTEXT) attribute: the layout's
/// glyph boxes, the point they are relative to and the layout rotation.
pub(crate) type AttrCells = (Vec<crate::entities::text_support::GlyphBox>, [f64; 2], f32);

fn build_attr_render(input: AttrTextInputs<'_>, document: &codec::CadDocument) -> RenderEntity {
    build_attr_layout(input, document, None)
}

/// [`build_attr_render`], also handing back a multi-line attribute's
/// character cells in `cells` (left `None` for a single-line one).
fn build_attr_layout(
    mut input: AttrTextInputs<'_>,
    document: &codec::CadDocument,
    mut cells: Option<&mut Option<AttrCells>>,
) -> RenderEntity {
    let normal = (input.normal.x, input.normal.y, input.normal.z);
    let (wsx, wsy, wsz) = transform::ocs_point_to_wcs(
        (
            input.insertion_point.x,
            input.insertion_point.y,
            input.insertion_point.z,
        ),
        normal,
    );
    let snap_pt = glam::DVec3::new(wsx, wsy, wsz);

    let resolved = resolve_text_style(input.text_style, document);

    // The entity stores the FINAL width factor / oblique angle (same rule
    // as TEXT). Use it as-is; fall back to the style only when the parser
    // reports a default-omitted 0.0.
    let base_wf = if input.width_factor.abs() > 1e-9 {
        (input.width_factor as f32).clamp(0.01, 100.0)
    } else {
        resolved.width_factor.max(0.01)
    };
    // text_generation_flags bit 2 (backward) flips width-factor sign; the
    // TextStyle's own is_backward is XOR-combined so mirror-twice cancels.
    let attr_backward = (input.text_generation_flags & 2) != 0;
    let mut width_factor = base_wf;
    if attr_backward ^ resolved.is_backward {
        width_factor = -width_factor;
    }

    // Upside-down (bit 4 / TextStyle.is_upside_down) rotates by π around the
    // insertion point. Combined with rotation we get a 180° flip about the
    // anchor — same as Text.
    let attr_upside_down = (input.text_generation_flags & 4) != 0;
    let upside_down = attr_upside_down ^ resolved.is_upside_down;
    let rotation = if upside_down {
        input.rotation as f32 + std::f32::consts::PI
    } else {
        input.rotation as f32
    };
    let oblique_angle = if input.oblique_angle.abs() > 1e-9 {
        input.oblique_angle as f32
    } else {
        resolved.oblique_angle
    };

    // Align / Fit run the text along the baseline between the two points:
    // Fit stretches its width, Align scales its height too.
    let (mut rotation, mut width_factor) = (rotation, width_factor);
    let span = (
        input.alignment_point.x - input.insertion_point.x,
        input.alignment_point.y - input.insertion_point.y,
    );
    let length = span.0.hypot(span.1);
    if matches!(input.horizontal_alignment, AHA::Aligned | AHA::Fit)
        && length > 1.0e-9
        && !input.value.contains("\\P")
    {
        if let Some(b) = text_local_bounds(
            &resolved.font_name,
            &resolve_dxf_special_chars(input.value),
            input.height as f32,
            width_factor.abs(),
            oblique_angle,
        )
        .filter(|b| b.advance > 1.0e-6)
        {
            let scale = length / b.advance as f64;
            rotation = span.1.atan2(span.0) as f32;
            if matches!(input.horizontal_alignment, AHA::Aligned) {
                input.height *= scale;
            } else {
                width_factor *= scale as f32;
            }
        }
    }

    // Anchor selection mirrors Text: only Left/Baseline uses insertion_point;
    // every other alignment uses alignment_point.
    let needs_align_pt = !(matches!(input.horizontal_alignment, AHA::Left)
        && matches!(input.vertical_alignment, AVA::Baseline));
    let anchor_f64 = if needs_align_pt {
        [input.alignment_point.x, input.alignment_point.y]
    } else {
        [input.insertion_point.x, input.insertion_point.y]
    };

    // MText-flag attributes (`mtext_flag = MultiLine | ConstantMultiLine`)
    // route through the shared MText pipeline so every inline format code
    // (`\f`, `\C`/`\c`, `\H`, `\W`, `\Q`, `\T`, `\A`, `\p…`, decorations,
    // stacked fractions, …) reaches the stroke output. SingleLine attribs
    // keep the Text-style anchor math below — they don't accept MText codes
    // in the DXF spec.
    if matches!(
        input.mtext_flag,
        MTextFlag::MultiLine | MTextFlag::ConstantMultiLine
    ) {
        let display_value = if input.value.is_empty() {
            String::new()
        } else {
            input.value.to_string()
        };
        let attach_h_anchor: f32 = match input.horizontal_alignment {
            AHA::Left => 0.0,
            AHA::Center | AHA::Middle => 0.5,
            AHA::Right | AHA::Aligned | AHA::Fit => 1.0,
        };
        let v_anchor = match input.vertical_alignment {
            AVA::Top => MTextVAnchor::Top,
            AVA::Middle => MTextVAnchor::Middle,
            AVA::Baseline | AVA::Bottom => MTextVAnchor::Bottom,
        };
        let needs_align_pt = !(matches!(input.horizontal_alignment, AHA::Left)
            && matches!(input.vertical_alignment, AVA::Baseline));
        let anchor_pt = if needs_align_pt {
            input.alignment_point
        } else {
            input.insertion_point
        };
        // Compose a ResolvedTextStyle that carries the merged width-factor
        // sign (entity backward XOR style backward) and the
        // entity-overridden oblique. is_upside_down is false because the
        // backwards / upside-down flips are already folded into `rotation`
        // and `width_factor`.
        let style_for_mtext = ResolvedTextStyle {
            font_name: resolved.font_name.clone(),
            width_factor: width_factor.abs(),
            oblique_angle,
            is_backward: width_factor < 0.0,
            is_upside_down: false,
            is_vertical: false,
        };
        let layout = layout_mtext(&MTextRenderOpts {
            // Not an MTEXT: text in a fixed box, never columnar.
            columns: Default::default(),
            value: &display_value,
            insertion: [anchor_pt.x, anchor_pt.y, anchor_pt.z],
            height: input.height as f32,
            rect_w: 0.0,
            rotation,
            style: &style_for_mtext,
            attach_h_anchor,
            v_anchor,
            line_spacing_factor: 1.0,
            exact_line_spacing: false,
            rectangle_height: 0.0,
            vertical_text: false,
            want_glyph_boxes: cells.is_some(),
        });
        if let Some(out) = cells.as_deref_mut() {
            *out = Some((layout.glyph_boxes.clone(), [anchor_pt.x, anchor_pt.y], rotation));
        }
        let _ = input.line_count;
        let _ = input.is_multiline;
        return RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Text(layout.strokes),
            snap_pts: vec![(snap_pt, SnapHint::Insertion)],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    // SingleLine path — anchor maths uses glyph bounds for accurate
    // horizontal / vertical positioning against alignment_point.
    let raw_value = input.value.to_string();
    let plain: Vec<String> = raw_value
        .replace("\\P", "\n")
        .split('\n')
        .map(|l| l.to_string())
        .collect();
    // An empty attribute value renders nothing — matching AutoCAD, where a
    // filled-in INSERT attribute left blank shows no text. The old `[value]`
    // fallback only ever fired when the value was already empty (and never
    // carried the tag), so it drew stray "[]" brackets for every blank
    // attribute. An ATTDEF preview passes its tag as the value (non-empty), so
    // it never reached this branch and is unaffected.
    let lines: Vec<String> = if plain.iter().all(|l| l.is_empty()) {
        Vec::new()
    } else {
        plain
    };

    let line_height = (input.height as f32) * 1.4; // typical CXF inter-line gap
    let (cos_r, sin_r) = (rotation.cos() as f64, rotation.sin() as f64);

    let mut strokes_all = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        // For width calculations strip MText decorations / DXF specials.
        let value_for_bounds = resolve_dxf_special_chars(line);
        let bounds = text_local_bounds(
            &resolved.font_name,
            &value_for_bounds,
            input.height as f32,
            width_factor,
            oblique_angle,
        );
        let (anchor_local_x, anchor_local_y) =
            if let Some(b) = bounds {
                // Horizontal anchor uses the pen advance box so leading /
                // trailing spaces keep their width.
                let ax = match input.horizontal_alignment {
                    AHA::Left => 0.0,
                    AHA::Center | AHA::Middle => b.advance * 0.5,
                    AHA::Right | AHA::Aligned | AHA::Fit => b.advance,
                };
                let ay = match input.vertical_alignment {
                    AVA::Baseline => 0.0,
                    AVA::Bottom => b.ink_min[1],
                    AVA::Middle => (b.ink_min[1] + b.ink_max[1]) * 0.5,
                    AVA::Top => b.ink_max[1],
                };
                (ax, ay)
            } else {
                (0.0, 0.0)
            };
        let line_offset_y = -(i as f32) * line_height;
        let local_y_for_line = anchor_local_y - line_offset_y;
        let origin: [f64; 2] = [
            anchor_f64[0] - (anchor_local_x as f64 * cos_r - local_y_for_line as f64 * sin_r),
            anchor_f64[1] - (anchor_local_x as f64 * sin_r + local_y_for_line as f64 * cos_r),
        ];
        // Parse `%%` codes through opencadcodec (same as TEXT), re-encoded for the
        // stroke tessellator, so attribute text shares the one parser.
        let encoded = crate::entities::text::acad_text_encode(line);
        let (strokes, fill_tris) = lff::tessellate_text_ex(
            [0.0, 0.0],
            input.height as f32,
            rotation,
            width_factor,
            oblique_angle,
            &resolved.font_name,
            &encoded,
        );
        strokes_all.push(TextStroke {
            strokes,
            origin,
            color: None,
            fill_tris,
            plane: None,
            // Carry a GlyphRun so the attribute renders as SDF glyph quads
            // (same as TEXT); the strokes above are the historical fallback and
            // are suppressed by the per-group SDF path.
            run: Some(GlyphRun {
                text: encoded,
                font: resolved.font_name.clone(),
                height: input.height as f32,
                rotation,
                width_factor,
                oblique: oblique_angle,
                tracking: 1.0,
                bold: false,
            }),
        });
    }
    let _ = input.line_count; // round-trip only — recomputed above

    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Text(strokes_all),
        snap_pts: vec![(snap_pt, SnapHint::Insertion)],
        tangent_geoms: vec![],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

// ── AttributeDefinition ───────────────────────────────────────────────────────

impl RenderConvertible for AttributeDefinition {
    fn to_render(&self, document: &codec::CadDocument) -> Option<RenderEntity> {
        // A definition shows its tag; the value appears only on the block
        // references (block content renders a constant one's value itself).
        Some(build_attr_render(
            AttrTextInputs {
                value: &self.tag,
                insertion_point: self.insertion_point,
                alignment_point: self.alignment_point,
                height: self.height,
                rotation: self.rotation,
                width_factor: self.width_factor,
                oblique_angle: self.oblique_angle,
                text_style: &self.text_style,
                text_generation_flags: self.text_generation_flags,
                horizontal_alignment: self.horizontal_alignment,
                vertical_alignment: self.vertical_alignment,
                normal: self.normal,
                // The tag is one line even on a multi-line definition.
                mtext_flag: MTextFlag::SingleLine,
                is_multiline: false,
                line_count: 1,
            },
            document,
        ))
    }
}

fn is_plain_left(a: &AttributeDefinition) -> bool {
    a.is_multiline
        || matches!(
            (a.horizontal_alignment, a.vertical_alignment),
            (AHA::Left, AVA::Baseline)
        )
}

fn is_two_point(a: &AttributeDefinition) -> bool {
    !a.is_multiline && matches!(a.horizontal_alignment, AHA::Aligned | AHA::Fit)
}

fn dv(p: Vector3) -> glam::DVec3 {
    glam::DVec3::new(p.x, p.y, p.z)
}

fn shift(p: &mut Vector3, d: glam::DVec3) {
    p.x += d.x;
    p.y += d.y;
    p.z += d.z;
}

impl Grippable for AttributeDefinition {
    /// Left text has one grip at its start; every other justification adds
    /// the alignment point. Lock position only pins attributes on block
    /// references, never the definition itself.
    fn grips(&self) -> Vec<GripDef> {
        let mut grips = vec![square_grip(0, dv(self.insertion_point))];
        if !is_plain_left(self) {
            grips.push(square_grip(1, dv(self.alignment_point)));
        }
        grips
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        let current = if grip_id == 1 { self.alignment_point } else { self.insertion_point };
        let d = match apply {
            GripApply::Translate(d) => glam::DVec3::new(d.x as f64, d.y as f64, d.z as f64),
            GripApply::Absolute(p) => glam::DVec3::new(p.x as f64, p.y as f64, p.z as f64) - dv(current),
        };
        // A two-point definition stretches the moved endpoint; any other
        // grip moves the whole text.
        if is_two_point(self) {
            if grip_id == 1 {
                shift(&mut self.alignment_point, d);
            } else {
                shift(&mut self.insertion_point, d);
            }
            return;
        }
        shift(&mut self.insertion_point, d);
        shift(&mut self.alignment_point, d);
        if let Some(m) = self.embedded_mtext.as_mut() {
            shift(&mut m.insertion_point, d);
        }
    }
}

const JUSTIFY_OPTIONS: [&str; 15] = [
    "Left",
    "Align",
    "Fit",
    "Center",
    "Middle",
    "Right",
    "Top left",
    "Top center",
    "Top right",
    "Middle left",
    "Middle center",
    "Middle right",
    "Bottom left",
    "Bottom center",
    "Bottom right",
];

fn justify_label(a: &AttributeDefinition) -> &'static str {
    match (a.horizontal_alignment, a.vertical_alignment) {
        (AHA::Aligned, _) => "Align",
        (AHA::Fit, _) => "Fit",
        (AHA::Middle, _) => "Middle",
        (AHA::Left, AVA::Baseline) => "Left",
        (AHA::Center, AVA::Baseline) => "Center",
        (AHA::Right, AVA::Baseline) => "Right",
        (AHA::Left, AVA::Top) => "Top left",
        (AHA::Center, AVA::Top) => "Top center",
        (AHA::Right, AVA::Top) => "Top right",
        (AHA::Left, AVA::Middle) => "Middle left",
        (AHA::Center, AVA::Middle) => "Middle center",
        (AHA::Right, AVA::Middle) => "Middle right",
        (AHA::Left, AVA::Bottom) => "Bottom left",
        (AHA::Center, AVA::Bottom) => "Bottom center",
        (AHA::Right, AVA::Bottom) => "Bottom right",
    }
}

fn parse_justify(s: &str) -> Option<(AHA, AVA)> {
    Some(match s {
        "Left" => (AHA::Left, AVA::Baseline),
        "Align" | "Aligned" => (AHA::Aligned, AVA::Baseline),
        "Fit" => (AHA::Fit, AVA::Baseline),
        "Center" => (AHA::Center, AVA::Baseline),
        "Middle" => (AHA::Middle, AVA::Baseline),
        "Right" => (AHA::Right, AVA::Baseline),
        "Top left" => (AHA::Left, AVA::Top),
        "Top center" => (AHA::Center, AVA::Top),
        "Top right" => (AHA::Right, AVA::Top),
        "Middle left" => (AHA::Left, AVA::Middle),
        "Middle center" => (AHA::Center, AVA::Middle),
        "Middle right" => (AHA::Right, AVA::Middle),
        "Bottom left" => (AHA::Left, AVA::Bottom),
        "Bottom center" => (AHA::Center, AVA::Bottom),
        "Bottom right" => (AHA::Right, AVA::Bottom),
        _ => return None,
    })
}

/// Offset of the justification anchor from the baseline start, in the
/// text's own frame (the shown tag measured with its style's font).
fn justify_offset(a: &AttributeDefinition) -> Option<(f64, f64)> {
    let b = text_local_bounds(
        &crate::entities::common::style_font(&a.text_style),
        &a.tag,
        a.height as f32,
        a.width_factor as f32,
        a.oblique_angle as f32,
    )?;
    let ax = match a.horizontal_alignment {
        AHA::Left => 0.0,
        AHA::Center | AHA::Middle => b.advance as f64 * 0.5,
        AHA::Right | AHA::Aligned | AHA::Fit => b.advance as f64,
    };
    let ay = match a.vertical_alignment {
        AVA::Baseline => 0.0,
        AVA::Bottom => b.ink_min[1] as f64,
        AVA::Middle => (b.ink_min[1] + b.ink_max[1]) as f64 * 0.5,
        AVA::Top => b.ink_max[1] as f64,
    };
    Some((ax, ay))
}

fn rotate(a: &AttributeDefinition, (x, y): (f64, f64)) -> (f64, f64) {
    let (sin, cos) = a.rotation.sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

/// The baseline start a justified definition stores as its insertion point
/// (the alignment point minus the justification offset).
pub fn definition_text_start(a: &AttributeDefinition) -> Vector3 {
    let Some(offset) = justify_offset(a) else {
        return a.alignment_point;
    };
    let (dx, dy) = rotate(a, offset);
    let p = a.alignment_point;
    Vector3::new(p.x - dx, p.y - dy, p.z)
}

fn yes_no(flag: bool) -> PropValue {
    PropValue::Choice {
        selected: if flag { t!("Yes") } else { t!("No") }.into_owned(),
        options: vec![t!("Yes").into_owned(), t!("No").into_owned()],
    }
}

fn is_yes(value: &str) -> bool {
    value == t!("Yes").as_ref() || value.eq_ignore_ascii_case("yes")
}

fn row(label: &str, field: &'static str, value: PropValue) -> Property {
    Property { label: label.into(), field, value }
}

fn set_bit(flags: i16, bit: i16, on: bool) -> i16 {
    if on {
        flags | bit
    } else {
        flags & !bit
    }
}

/// What the Properties palette refuses for a definition, with the message
/// the reference shows.
pub fn validate_definition_property(field: &str, value: &str) -> Result<(), &'static str> {
    match field {
        "att_tag" if value.trim().is_empty() || value.trim().contains(char::is_whitespace) => {
            Err("Invalid argument Tag in setting TagString")
        }
        "att_h" | "att_wf" if !parse_f64(value).is_some_and(|v| v > 0.0) => Err("Invalid input"),
        _ => Ok(()),
    }
}

/// Multiple lines on: the value moves into an embedded MText anchored at the
/// start point. Off: back to single-line Left text.
fn set_multiline(a: &mut AttributeDefinition, on: bool) {
    if on == a.is_multiline {
        return;
    }
    a.is_multiline = on;
    if on {
        a.mtext_flag = MTextFlag::ConstantMultiLine;
        a.horizontal_alignment = AHA::Left;
        a.vertical_alignment = AVA::Top;
        a.alignment_point = a.insertion_point;
        a.line_count = a.default_value.split("\\P").count().max(1) as i16;
        let mut mtext = codec::entities::MText::new();
        mtext.value = a.default_value.clone();
        mtext.insertion_point = a.insertion_point;
        mtext.height = a.height;
        mtext.rotation = a.rotation;
        mtext.style = a.text_style.clone();
        a.embedded_mtext = Some(Box::new(mtext));
    } else {
        a.mtext_flag = MTextFlag::SingleLine;
        a.horizontal_alignment = AHA::Left;
        a.vertical_alignment = AVA::Baseline;
        a.line_count = 1;
        a.default_value = a.default_value.replace("\\P", " ");
        a.embedded_mtext = None;
    }
}

impl PropertyEditable for AttributeDefinition {
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection> {
        let left = is_plain_left(self);
        // Text alignment is the anchor of every justification but Left.
        let alignment = |label: std::borrow::Cow<'static, str>, field: &'static str, v: f64| {
            if left {
                ro(label.as_ref(), field, String::new())
            } else {
                edit(label.as_ref(), field, v)
            }
        };
        let text_props = vec![
            row(t!("Tag").as_ref(), "att_tag", PropValue::PlainText(self.tag.clone())),
            row(t!("Annotative").as_ref(), "att_annotative", yes_no(self.flags.annotative)),
            if self.flags.constant {
                ro(t!("Prompt").as_ref(), "att_prompt", self.prompt.clone())
            } else {
                row(t!("Prompt").as_ref(), "att_prompt", PropValue::PlainText(self.prompt.clone()))
            },
            row(t!("Value").as_ref(), "att_default", PropValue::PlainText(self.default_value.clone())),
            row(
                t!("Style").as_ref(),
                "att_style",
                PropValue::Choice {
                    selected: if self.text_style.trim().is_empty() {
                        "Standard".into()
                    } else {
                        self.text_style.clone()
                    },
                    options: text_style_names.to_vec(),
                },
            ),
            if self.is_multiline {
                ro(t!("Justify").as_ref(), "att_justify", t!(justify_label(self)).into_owned())
            } else {
                row(
                    t!("Justify").as_ref(),
                    "att_justify",
                    PropValue::Choice {
                        selected: justify_label(self).to_string(),
                        options: JUSTIFY_OPTIONS.into_iter().map(str::to_string).collect(),
                    },
                )
            },
            // Fixed by the style when the style fixes its height.
            crate::entities::common::num_prop(
                t!("Height").as_ref(),
                "att_h",
                self.height,
                crate::entities::common::style_fixed_height(&self.text_style).is_none(),
            ),
            edit_angle(t!("Rotation").as_ref(), "att_rot", self.rotation.to_degrees()),
            edit(t!("Width factor").as_ref(), "att_wf", self.width_factor),
            edit_angle(t!("Obliquing").as_ref(), "att_ob", self.oblique_angle.to_degrees()),
            ro(t!("Direction").as_ref(), "att_direction", t!("By style").into_owned()),
            if self.is_multiline {
                edit(
                    t!("Boundary width").as_ref(),
                    "att_bwidth",
                    self.embedded_mtext.as_ref().map_or(0.0, |m| m.rectangle_width),
                )
            } else {
                ro(t!("Boundary width").as_ref(), "att_bwidth", String::new())
            },
            alignment(t!("Text alignment X"), "att_ax", self.alignment_point.x),
            alignment(t!("Text alignment Y"), "att_ay", self.alignment_point.y),
            alignment(t!("Text alignment Z"), "att_az", self.alignment_point.z),
        ];
        vec![
            PropSection { title: t!("Text").into_owned(), props: text_props },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    row(t!("Upside down").as_ref(), "att_upside_down", yes_no(self.text_generation_flags & 0x4 != 0)),
                    row(t!("Backward").as_ref(), "att_backward", yes_no(self.text_generation_flags & 0x2 != 0)),
                    row(t!("Invisible").as_ref(), "att_invisible", yes_no(self.flags.invisible)),
                    row(t!("Constant").as_ref(), "att_constant", yes_no(self.flags.constant)),
                    row(t!("Verify").as_ref(), "att_verify", yes_no(self.flags.verify)),
                    row(t!("Preset").as_ref(), "att_preset", yes_no(self.flags.preset)),
                    row(t!("Multiple lines").as_ref(), "att_multiline", yes_no(self.is_multiline)),
                    row(
                        t!("Lock position").as_ref(),
                        "att_lock_pos",
                        yes_no(self.lock_position || self.flags.locked_position),
                    ),
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        if validate_definition_property(field, value).is_err() {
            return;
        }
        match field {
            "att_tag" => self.tag = value.trim().to_uppercase(),
            "att_prompt" if !self.flags.constant => self.prompt = value.to_string(),
            "att_default" => {
                self.default_value = value.to_string();
                if let Some(m) = self.embedded_mtext.as_mut() {
                    m.value = value.to_string();
                }
            }
            "att_style" => {
                self.text_style = value.to_string();
                if let Some(m) = self.embedded_mtext.as_mut() {
                    m.style = value.to_string();
                }
            }
            "att_annotative" => self.flags.annotative = is_yes(value),
            "att_invisible" => self.flags.invisible = is_yes(value),
            "att_constant" => {
                self.flags.constant = is_yes(value);
                if self.flags.constant {
                    self.prompt.clear();
                }
            }
            "att_verify" => self.flags.verify = is_yes(value),
            "att_preset" => self.flags.preset = is_yes(value),
            "att_lock_pos" => {
                self.lock_position = is_yes(value);
                self.flags.locked_position = self.lock_position;
            }
            "att_upside_down" => {
                self.text_generation_flags = set_bit(self.text_generation_flags, 0x4, is_yes(value))
            }
            "att_backward" => {
                self.text_generation_flags = set_bit(self.text_generation_flags, 0x2, is_yes(value))
            }
            "att_multiline" => set_multiline(self, is_yes(value)),
            "att_justify" => {
                let Some((h, v)) = parse_justify(value) else {
                    return;
                };
                let was_two_point = is_two_point(self);
                self.horizontal_alignment = h;
                self.vertical_alignment = v;
                if is_plain_left(self) {
                    return;
                }
                // The text stays where it is: the baseline start is kept and
                // the alignment point moves to the new anchor.
                if !is_two_point(self) {
                    let (dx, dy) = justify_offset(self).map_or((0.0, 0.0), |o| rotate(self, o));
                    let s = self.insertion_point;
                    self.alignment_point = Vector3::new(s.x + dx, s.y + dy, s.z);
                    return;
                }
                let span = (self.alignment_point.x - self.insertion_point.x)
                    .hypot(self.alignment_point.y - self.insertion_point.y);
                if !was_two_point || span < 1.0e-9 {
                    // A baseline as long as the tag.
                    let length = justify_offset(self).map_or(self.height, |(advance, _)| advance);
                    let (sin, cos) = self.rotation.sin_cos();
                    self.alignment_point = Vector3::new(
                        self.insertion_point.x + cos * length,
                        self.insertion_point.y + sin * length,
                        self.insertion_point.z,
                    );
                }
            }
            _ => {
                let Some(v) = parse_f64(value) else {
                    return;
                };
                match field {
                    "att_ax" => self.alignment_point.x = v,
                    "att_ay" => self.alignment_point.y = v,
                    "att_az" => self.alignment_point.z = v,
                    "att_h" => {
                        self.height = v;
                        if let Some(m) = self.embedded_mtext.as_mut() {
                            m.height = v;
                        }
                    }
                    "att_rot" => {
                        self.rotation = v.to_radians();
                        if let Some(m) = self.embedded_mtext.as_mut() {
                            m.rotation = self.rotation;
                        }
                    }
                    "att_wf" => self.width_factor = v,
                    "att_ob" => self.oblique_angle = v.to_radians(),
                    "att_bwidth" if v >= 0.0 => {
                        if let Some(m) = self.embedded_mtext.as_mut() {
                            m.rectangle_width = v;
                        }
                    }
                    _ => {}
                }
            }
        }
        // A one-point justified definition keeps its stored baseline start in
        // step with the anchor, height, style and rotation.
        if !is_plain_left(self) && !is_two_point(self) {
            self.insertion_point = definition_text_start(self);
        }
    }
}

impl Transformable for AttributeDefinition {
    fn apply_transform(&mut self, t: &EntityTransform) {
        transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
            transform::reflect_xy_point(
                &mut entity.alignment_point.x,
                &mut entity.alignment_point.y,
                p1,
                p2,
            );
        });
    }
}

// ── AttributeEntity ───────────────────────────────────────────────────────────

/// The laid-out character cells of a multi-line (MTEXT) attribute, the
/// point they are relative to and the layout rotation; `None` for a
/// single-line attribute.
pub(crate) fn attribute_cells(a: &AttributeEntity, document: &codec::CadDocument) -> Option<AttrCells> {
    let mut out = None;
    build_attr_layout(attr_inputs(a), document, Some(&mut out));
    out
}

fn attr_inputs(a: &AttributeEntity) -> AttrTextInputs<'_> {
    AttrTextInputs {
        value: &a.value,
        insertion_point: a.insertion_point,
        alignment_point: a.alignment_point,
        height: a.height,
        rotation: a.rotation,
        width_factor: a.width_factor,
        oblique_angle: a.oblique_angle,
        text_style: &a.text_style,
        text_generation_flags: a.text_generation_flags,
        horizontal_alignment: a.horizontal_alignment,
        vertical_alignment: a.vertical_alignment,
        normal: a.normal,
        mtext_flag: a.mtext_flag,
        is_multiline: a.is_multiline,
        line_count: a.line_count,
    }
}

impl RenderConvertible for AttributeEntity {
    fn to_render(&self, document: &codec::CadDocument) -> Option<RenderEntity> {
        Some(build_attr_render(
            AttrTextInputs {
                value: &self.value,
                insertion_point: self.insertion_point,
                alignment_point: self.alignment_point,
                height: self.height,
                rotation: self.rotation,
                width_factor: self.width_factor,
                oblique_angle: self.oblique_angle,
                text_style: &self.text_style,
                text_generation_flags: self.text_generation_flags,
                horizontal_alignment: self.horizontal_alignment,
                vertical_alignment: self.vertical_alignment,
                normal: self.normal,
                mtext_flag: self.mtext_flag,
                is_multiline: self.is_multiline,
                line_count: self.line_count,
            },
            document,
        ))
    }
}

impl Grippable for AttributeEntity {
    fn grips(&self) -> Vec<GripDef> {
        if self.lock_position || self.flags.locked_position {
            return vec![];
        }
        vec![square_grip(
            0,
            glam::DVec3::new(
                self.insertion_point.x,
                self.insertion_point.y,
                self.insertion_point.z,
            ),
        )]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if self.lock_position || self.flags.locked_position {
            return;
        }
        if grip_id == 0 {
            match apply {
                GripApply::Translate(d) => {
                    self.insertion_point.x += d.x as f64;
                    self.insertion_point.y += d.y as f64;
                    self.insertion_point.z += d.z as f64;
                    self.alignment_point.x += d.x as f64;
                    self.alignment_point.y += d.y as f64;
                    self.alignment_point.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    self.insertion_point.x = p.x as f64;
                    self.insertion_point.y = p.y as f64;
                    self.insertion_point.z = p.z as f64;
                    self.alignment_point.x = p.x as f64;
                    self.alignment_point.y = p.y as f64;
                    self.alignment_point.z = p.z as f64;
                }
            }
        }
    }
}

impl PropertyEditable for AttributeEntity {
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection> {
        vec![
            PropSection {
                title: t!("Text").into_owned(),
                props: vec![
                    Property {
                        label: t!("Tag").into_owned(),
                        field: "atte_tag",
                        value: PropValue::PlainText(self.tag.clone()),
                    },
                    ro(t!("Prompt").as_ref(), "atte_prompt", String::new()),
                    Property {
                        label: t!("Value").into_owned(),
                        field: "atte_val",
                        value: PropValue::PlainText(self.value.clone()),
                    },
                    Property {
                        label: t!("Style").into_owned(),
                        field: "atte_style",
                        value: PropValue::Choice {
                            selected: if self.text_style.trim().is_empty() {
                                "Standard".into()
                            } else {
                                self.text_style.clone()
                            },
                            options: text_style_names.to_vec(),
                        },
                    },
                    Property {
                        label: t!("Justify").into_owned(),
                        field: "atte_halign",
                        value: PropValue::Choice {
                            selected: halign_str(self.horizontal_alignment).to_string(),
                            options: ["Left", "Center", "Right", "Aligned", "Middle", "Fit"]
                                .into_iter()
                                .map(str::to_string)
                                .collect(),
                        },
                    },
                    Property {
                        label: t!("V-Align").into_owned(),
                        field: "atte_valign",
                        value: PropValue::Choice {
                            selected: valign_str(self.vertical_alignment).to_string(),
                            options: ["Baseline", "Bottom", "Middle", "Top"]
                                .into_iter()
                                .map(str::to_string)
                                .collect(),
                        },
                    },
                    ro(
                        t!("Annotative").as_ref(),
                        "atte_annotative",
                        bool_yn(self.flags.annotative),
                    ),
                    // Fixed by the style when the style fixes its height.
                    crate::entities::common::num_prop(
                        t!("Height").as_ref(),
                        "atte_h",
                        self.height,
                        crate::entities::common::style_fixed_height(&self.text_style).is_none(),
                    ),
                    edit_angle(t!("Rotation").as_ref(), "atte_rot", self.rotation.to_degrees()),
                    edit(t!("Width factor").as_ref(), "atte_wf", self.width_factor),
                    edit_angle(t!("Obliquing").as_ref(), "atte_ob", self.oblique_angle.to_degrees()),
                    edit(t!("Text alignment X").as_ref(), "atte_ax", self.alignment_point.x),
                    edit(t!("Text alignment Y").as_ref(), "atte_ay", self.alignment_point.y),
                    edit(t!("Text alignment Z").as_ref(), "atte_az", self.alignment_point.z),
                    ro(
                        t!("Boundary width").as_ref(),
                        "atte_field_len",
                        self.field_length.to_string(),
                    ),
                ],
            },
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    edit(t!("Position X").as_ref(), "atte_ix", self.insertion_point.x),
                    edit(t!("Position Y").as_ref(), "atte_iy", self.insertion_point.y),
                    edit(t!("Position Z").as_ref(), "atte_iz", self.insertion_point.z),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    ro(
                        t!("Upside down").as_ref(),
                        "atte_upside_down",
                        bool_yn(self.text_generation_flags & 0x4 != 0),
                    ),
                    ro(
                        t!("Backward").as_ref(),
                        "atte_backward",
                        bool_yn(self.text_generation_flags & 0x2 != 0),
                    ),
                    ro(t!("Invisible").as_ref(), "atte_invisible", bool_yn(self.flags.invisible)),
                    ro(
                        t!("Multiple lines").as_ref(),
                        "atte_mtext_flag",
                        mtext_flag_str(self.mtext_flag),
                    ),
                    ro(t!("Constant").as_ref(), "atte_constant", bool_yn(self.flags.constant)),
                    ro(t!("Verify").as_ref(), "atte_verify", bool_yn(self.flags.verify)),
                    ro(t!("Preset").as_ref(), "atte_preset", bool_yn(self.flags.preset)),
                    ro(
                        t!("Lock position").as_ref(),
                        "atte_lock_pos",
                        bool_yn(self.lock_position),
                    ),
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        if field == "atte_tag" {
            self.tag = value.to_string();
            return;
        }
        if field == "atte_val" {
            if !self.flags.constant {
                self.value = value.to_string();
            }
            return;
        }
        if field == "atte_style" {
            self.text_style = value.to_string();
            return;
        }
        if field == "atte_halign" {
            if let Some(a) = parse_halign(value) {
                self.horizontal_alignment = a;
            }
            return;
        }
        if field == "atte_valign" {
            if let Some(a) = parse_valign(value) {
                self.vertical_alignment = a;
            }
            return;
        }
        let Some(v) = parse_f64(value) else {
            return;
        };
        match field {
            "atte_ix" => self.insertion_point.x = v,
            "atte_iy" => self.insertion_point.y = v,
            "atte_iz" => self.insertion_point.z = v,
            "atte_ax" => self.alignment_point.x = v,
            "atte_ay" => self.alignment_point.y = v,
            "atte_az" => self.alignment_point.z = v,
            "atte_h" if v > 0.0 => self.height = v,
            "atte_rot" => self.rotation = v.to_radians(),
            "atte_wf" if v.abs() > 1e-9 => self.width_factor = v,
            "atte_ob" => self.oblique_angle = v.to_radians(),
            _ => {}
        }
    }
}

impl Transformable for AttributeEntity {
    fn apply_transform(&mut self, t: &EntityTransform) {
        transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
            transform::reflect_xy_point(
                &mut entity.alignment_point.x,
                &mut entity.alignment_point.y,
                p1,
                p2,
            );
        });
    }
}

impl crate::entities::traits::TextContent for codec::entities::AttributeDefinition {
    fn text_content(&self) -> Option<String> {
        Some(self.default_value.clone())
    }
    fn replace_text(&mut self, search: &str, rep: &str) {
        let search_lc = search.to_lowercase();
        if self.default_value.to_lowercase().contains(&search_lc) {
            self.default_value = self.default_value.replace(search, rep);
        }
    }
}

impl crate::entities::traits::TextContent for codec::entities::AttributeEntity {
    fn text_content(&self) -> Option<String> {
        Some(self.get_value().to_string())
    }
    fn replace_text(&mut self, search: &str, rep: &str) {
        let search_lc = search.to_lowercase();
        let cur = self.get_value().to_string();
        if cur.to_lowercase().contains(&search_lc) {
            self.set_value(cur.replace(search, rep));
        }
    }
}
