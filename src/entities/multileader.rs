use acadrust::entities::{
    LeaderContentType, MultiLeader, MultiLeaderPathType, TextAttachmentPointType, TextAttachmentType,
};

/// Local-frame vertical baseline offset for line 0 given a vertical attachment
/// and the overall block geometry. The baseline is at y=0 for `BottomOfTopLine*`
/// (i.e. text_location coincides with line-0 baseline); other variants offset
/// the block above/below/around text_location.
pub(crate) fn v_offset_for_attachment(
    attach: TextAttachmentType,
    n_lines: f32,
    h: f32,
    line_h: f32,
) -> f32 {
    match attach {
        TextAttachmentType::TopOfTopLine => -h,
        TextAttachmentType::MiddleOfTopLine => -h * 0.5,
        TextAttachmentType::MiddleOfText
        | TextAttachmentType::CenterOfText
        | TextAttachmentType::CenterOfTextOverline => ((n_lines - 1.0) * line_h - h) * 0.5,
        TextAttachmentType::MiddleOfBottomLine => (n_lines - 1.0) * line_h - h * 0.5,
        TextAttachmentType::BottomOfBottomLine | TextAttachmentType::BottomLine => {
            (n_lines - 1.0) * line_h
        }
        TextAttachmentType::BottomOfTopLineUnderlineBottomLine
        | TextAttachmentType::BottomOfTopLineUnderlineTopLine
        | TextAttachmentType::BottomOfTopLineUnderlineAll => 0.0,
    }
}
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{diamond_grip, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::wire_model::{SnapHint, TangentGeom};

// ── TruckConvertible ────────────────────────────────────────────────────────

/// Catmull-Rom spline tessellation through `ctrl` points, `segs_per_span` segments each.
/// Operates in f64 so it can be applied to either WCS-direct coordinates (entity path)
/// or offset-relative coordinates (scene path) without precision loss.
pub(crate) fn catmull_rom_pts(ctrl: &[[f64; 3]], segs_per_span: u32) -> Vec<[f64; 3]> {
    let n = ctrl.len();
    let mut out = Vec::new();
    for i in 0..n.saturating_sub(1) {
        let p0 = if i == 0 { ctrl[0] } else { ctrl[i - 1] };
        let p1 = ctrl[i];
        let p2 = ctrl[i + 1];
        let p3 = if i + 2 < n { ctrl[i + 2] } else { ctrl[n - 1] };
        for j in 0..=segs_per_span {
            let t = j as f64 / segs_per_span as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            let mut pt = [0.0_f64; 3];
            for k in 0..3 {
                pt[k] = 0.5
                    * ((2.0 * p1[k])
                        + (-p0[k] + p2[k]) * t
                        + (2.0 * p0[k] - 5.0 * p1[k] + 4.0 * p2[k] - p3[k]) * t2
                        + (-p0[k] + 3.0 * p1[k] - 3.0 * p2[k] + p3[k]) * t3);
            }
            out.push(pt);
        }
    }
    out
}

fn to_truck(ml: &MultiLeader, document: &acadrust::CadDocument) -> Option<TruckEntity> {
    let nan = [f64::NAN; 3];
    let p3 = |v: &acadrust::types::Vector3| -> [f64; 3] { [v.x, v.y, v.z] };

    let arrow_size = ml.arrowhead_size;
    let draw_arrow = arrow_size > 0.0;
    let invisible = ml.path_type == MultiLeaderPathType::Invisible;

    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut tangents: Vec<TangentGeom> = Vec::new();
    let mut key_verts: Vec<[f64; 3]> = Vec::new();
    let mut snap_pts: Vec<(Vec3, SnapHint)> = Vec::new();
    let mut first = true;

    // snap_pts uses f32 (UI-only); cast at construction.
    let node = |arr: [f64; 3]| {
        (
            Vec3::new(arr[0] as f32, arr[1] as f32, arr[2] as f32),
            SnapHint::Node,
        )
    };

    for root in &ml.context.leader_roots {
        let cp = &root.connection_point;
        let cp_f = p3(cp);
        snap_pts.push(node(cp_f));

        for line in &root.lines {
            if line.points.is_empty() {
                continue;
            }

            if !invisible {
                if !first {
                    points.push(nan);
                }
                first = false;

                // Build the full control-point list: line.points + connection_point
                let mut ctrl: Vec<[f64; 3]> = line.points.iter().map(|p| p3(p)).collect();
                let last_f = *ctrl.last().unwrap_or(&cp_f);
                let dist = ((last_f[0] - cp_f[0]).powi(2) + (last_f[1] - cp_f[1]).powi(2)).sqrt();
                if dist > 1e-9 {
                    ctrl.push(cp_f);
                }
                for &c in &ctrl {
                    key_verts.push(c);
                    snap_pts.push(node(c));
                }

                if ml.path_type == MultiLeaderPathType::Spline && ctrl.len() >= 2 {
                    // Catmull-Rom spline through the bend points.
                    let pts = catmull_rom_pts(&ctrl, 8);
                    for &pt in &pts {
                        points.push(pt);
                    }
                } else {
                    for &c in &ctrl {
                        points.push(c);
                    }
                }

                for i in 0..ctrl.len().saturating_sub(1) {
                    let a = ctrl[i];
                    let b = ctrl[i + 1];
                    tangents.push(TangentGeom::Line {
                        p1: [a[0] as f32, a[1] as f32, a[2] as f32],
                        p2: [b[0] as f32, b[1] as f32, b[2] as f32],
                    });
                }
            }

            // Arrowhead
            if draw_arrow {
                let tip = &line.points[0];
                let tip_f = p3(tip);
                let next = if line.points.len() >= 2 {
                    line.points[1]
                } else {
                    *cp
                };
                let dx = next.x - tip.x;
                let dy = next.y - tip.y;
                let dl = (dx * dx + dy * dy).sqrt().max(1e-9);
                let (dx, dy) = (dx / dl, dy / dl);
                let a = std::f64::consts::PI / 6.0;
                let (s, c) = a.sin_cos();
                points.push(nan);
                points.push([
                    tip_f[0] + (dx * c - dy * s) * arrow_size,
                    tip_f[1] + (dx * s + dy * c) * arrow_size,
                    tip_f[2],
                ]);
                points.push(tip_f);
                points.push([
                    tip_f[0] + (dx * c + dy * s) * arrow_size,
                    tip_f[1] + (-dx * s + dy * c) * arrow_size,
                    tip_f[2],
                ]);
            }
        }

        // Landing shelf at connection_point
        if ml.enable_landing && ml.enable_dogleg && ml.dogleg_length > 0.0 {
            let dir = &root.direction;
            let dl = (dir.x * dir.x + dir.y * dir.y).sqrt().max(1e-9);
            let d = ml.dogleg_length;
            points.push(nan);
            points.push(cp_f);
            points.push([cp.x + dir.x / dl * d, cp.y + dir.y / dl * d, cp.z]);
        }
    }

    // Text strokes (MText content rendered inline). Mirrors the MText pipeline:
    // strip inline format codes, split / word-wrap into lines, then place each
    // line with the multileader's rotation, horizontal & vertical attachment,
    // line spacing and scale_factor applied. Annotation scale is NOT applied
    // here — this path is consumed by snap / truck export which work in WCS.
    if ml.content_type == LeaderContentType::MText && !ml.context.text_string.is_empty() {
        let ctx = &ml.context;
        let raw_height = if ctx.text_height > 0.0 {
            ctx.text_height
        } else {
            ml.text_height
        } as f32;
        let scale_factor = ml.scale_factor as f32;
        let height = raw_height * scale_factor;

        let ins = &ctx.text_location;
        let ins_x = ins.x;
        let ins_y = ins.y;
        let z = ins.z;
        // Prefer text_direction (carries through rotations/mirrors); fall back
        // to text_rotation when no direction has been set.
        let td = ctx.text_direction;
        let rot = if td.x.abs() > 1e-9 || td.y.abs() > 1e-9 {
            (td.y as f32).atan2(td.x as f32)
        } else {
            ctx.text_rotation as f32
        };
        let (cos_r, sin_r) = (rot.cos(), rot.sin());
        snap_pts.push(node([ins_x, ins_y, z]));

        // Resolve text style via handle when available, falling back to STANDARD.
        let style_name = ctx
            .text_style_handle
            .as_ref()
            .and_then(|h| {
                document
                    .text_styles
                    .iter()
                    .find(|s| s.handle == *h)
                    .map(|s| s.name.clone())
            })
            .unwrap_or_else(|| "STANDARD".to_string());
        let style = crate::entities::text_support::resolve_text_style(&style_name, document);
        let font_name = style.font_name;
        let font = crate::scene::cxf::get_font(&font_name);
        let width_factor = style.width_factor.max(0.01);
        let oblique = style.oblique_angle;

        // Strip MText format codes (e.g. `{\fArial Black|b0|i0|c162|p34;...}`),
        // then split on \P / \n / \N and optionally word-wrap to text_width.
        let plain = crate::entities::text_support::strip_mtext_codes(&ctx.text_string);
        let explicit_lines = crate::entities::text_support::split_mtext_lines(&plain);
        let lines: Vec<String> = if ctx.text_width > 0.0 {
            let scale = height / 9.0 * width_factor;
            let max_w = ctx.text_width as f32 * scale_factor;
            explicit_lines
                .iter()
                .flat_map(|line| {
                    crate::entities::text_support::word_wrap(line, max_w, scale, font)
                })
                .collect()
        } else {
            explicit_lines
        };

        let ls_factor = if ctx.line_spacing_factor > 0.0 {
            ctx.line_spacing_factor as f32
        } else {
            1.0
        };
        let line_h = height * ls_factor * (5.0 / 3.0) * font.line_spacing;
        let n_lines = lines.len().max(1) as f32;

        let h_anchor = match ctx.text_attachment_point {
            TextAttachmentPointType::Left => 0.0_f32,
            TextAttachmentPointType::Center => 0.5,
            TextAttachmentPointType::Right => 1.0,
        };
        let v_offset =
            v_offset_for_attachment(ctx.text_left_attachment, n_lines, height, line_h);

        let scale = height / 9.0 * width_factor;
        for (i, line) in lines.iter().enumerate() {
            let li = i as f32;
            let line_y_local = -li * line_h + v_offset;
            let line_w = if h_anchor > 0.0 {
                crate::entities::text_support::measure_mtext_chars(line, scale, font)
            } else {
                0.0
            };
            let h_shift_local = -line_w * h_anchor;
            let wcs_dx = h_shift_local * cos_r - line_y_local * sin_r;
            let wcs_dy = h_shift_local * sin_r + line_y_local * cos_r;
            // tessellate_text_ex emits f32 glyph points around `origin`.
            // For entity-level WCS output, we accept the f32 cast here since
            // glyph offsets are small relative to the insertion point and the
            // entity path doesn't apply a world_offset before render.
            let origin = [ins_x as f32 + wcs_dx, ins_y as f32 + wcs_dy];
            let strokes = crate::scene::cxf::tessellate_text_ex(
                origin,
                height,
                rot,
                width_factor,
                oblique,
                &font_name,
                line,
            );
            for stroke in &strokes {
                if stroke.len() < 2 {
                    continue;
                }
                points.push(nan);
                for &[x, y] in stroke {
                    points.push([x as f64, y as f64, z]);
                }
            }
        }
    }

    if points.is_empty() {
        return None;
    }

    Some(TruckEntity {
        object: TruckObject::Lines(points),
        snap_pts,
        tangent_geoms: tangents,
        key_vertices: key_verts,
        fill_tris: vec![],
    })
}

// ── Grips ──────────────────────────────────────────────────────────────────
//
// IDs are assigned in two passes:
//   0 .. total_line_pts - 1  : square grips on every LeaderLine vertex
//   total_line_pts            : diamond grip on context.text_location (if MText)

fn grips(ml: &MultiLeader) -> Vec<GripDef> {
    let mut result: Vec<GripDef> = Vec::new();
    let mut id = 0usize;

    for root in &ml.context.leader_roots {
        for line in &root.lines {
            for p in &line.points {
                result.push(square_grip(
                    id,
                    Vec3::new(p.x as f32, p.y as f32, p.z as f32),
                ));
                id += 1;
            }
        }
    }

    if ml.content_type == LeaderContentType::MText {
        let tl = &ml.context.text_location;
        result.push(diamond_grip(
            id,
            Vec3::new(tl.x as f32, tl.y as f32, tl.z as f32),
        ));
    }

    result
}

fn apply_grip(ml: &mut MultiLeader, grip_id: usize, apply: GripApply) {
    let mut idx = 0usize;

    for root in &mut ml.context.leader_roots {
        for line in &mut root.lines {
            for p in &mut line.points {
                if idx == grip_id {
                    match apply {
                        GripApply::Absolute(a) => {
                            p.x = a.x as f64;
                            p.y = a.y as f64;
                            p.z = a.z as f64;
                        }
                        GripApply::Translate(d) => {
                            p.x += d.x as f64;
                            p.y += d.y as f64;
                            p.z += d.z as f64;
                        }
                    }
                    return;
                }
                idx += 1;
            }
        }
    }

    // Text location grip
    if ml.content_type == LeaderContentType::MText && idx == grip_id {
        let tl = &mut ml.context.text_location;
        match apply {
            GripApply::Absolute(a) => {
                tl.x = a.x as f64;
                tl.y = a.y as f64;
                tl.z = a.z as f64;
            }
            GripApply::Translate(d) => {
                tl.x += d.x as f64;
                tl.y += d.y as f64;
                tl.z += d.z as f64;
            }
        }
    }
}

// ── Properties ─────────────────────────────────────────────────────────────

fn content_type_str(ct: &LeaderContentType) -> &'static str {
    match ct {
        LeaderContentType::None => "None",
        LeaderContentType::Block => "Block",
        LeaderContentType::MText => "MText",
        LeaderContentType::Tolerance => "Tolerance",
    }
}

fn path_type_str(pt: &MultiLeaderPathType) -> &'static str {
    match pt {
        MultiLeaderPathType::Invisible => "Invisible",
        MultiLeaderPathType::StraightLineSegments => "Straight",
        MultiLeaderPathType::Spline => "Spline",
    }
}

fn attachment_str(a: &TextAttachmentType) -> &'static str {
    match a {
        TextAttachmentType::TopOfTopLine => "Top of Top",
        TextAttachmentType::MiddleOfTopLine => "Mid of Top",
        TextAttachmentType::MiddleOfText => "Mid of Text",
        TextAttachmentType::MiddleOfBottomLine => "Mid of Bot",
        TextAttachmentType::BottomOfBottomLine => "Bot of Bot",
        TextAttachmentType::BottomLine => "Bottom Line",
        _ => "Other",
    }
}

fn bool_toggle(label: &str, field: &'static str, value: bool) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::BoolToggle { field, value },
    }
}

fn choice(label: &str, field: &'static str, selected: &str, opts: &[&str]) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::Choice {
            selected: selected.to_string(),
            options: opts.iter().map(|s| s.to_string()).collect(),
        },
    }
}

fn properties(ml: &MultiLeader) -> PropSection {
    let ctx = &ml.context;
    let total_pts: usize = ctx
        .leader_roots
        .iter()
        .flat_map(|r| r.lines.iter())
        .map(|l| l.points.len())
        .sum();

    let mut props = vec![
        // Content
        choice(
            "Content Type",
            "content_type",
            content_type_str(&ml.content_type),
            &["None", "MText", "Block", "Tolerance"],
        ),
        Property {
            label: "Text".into(),
            field: "text_string",
            value: PropValue::EditText(ctx.text_string.clone()),
        },
        edit("Text Height", "text_height", ml.text_height),
        edit("Text X", "text_x", ctx.text_location.x),
        edit("Text Y", "text_y", ctx.text_location.y),
        edit("Text Z", "text_z", ctx.text_location.z),
        bool_toggle("Text Frame", "text_frame", ml.text_frame),
        // Leader line
        choice(
            "Path Type",
            "path_type",
            path_type_str(&ml.path_type),
            &["Straight", "Spline", "Invisible"],
        ),
        bool_toggle("Landing", "enable_landing", ml.enable_landing),
        bool_toggle("Dogleg", "enable_dogleg", ml.enable_dogleg),
        edit("Dogleg Length", "dogleg_length", ml.dogleg_length),
        edit("Arrow Size", "arrowhead_size", ml.arrowhead_size),
        edit("Scale", "scale_factor", ml.scale_factor),
        bool_toggle(
            "Annotation Scale",
            "enable_annotation_scale",
            ml.enable_annotation_scale,
        ),
        // Text attachment
        choice(
            "Left Attach",
            "text_left_attachment",
            attachment_str(&ml.text_left_attachment),
            &[
                "Top of Top",
                "Mid of Top",
                "Mid of Text",
                "Mid of Bot",
                "Bot of Bot",
                "Bottom Line",
            ],
        ),
        choice(
            "Right Attach",
            "text_right_attachment",
            attachment_str(&ml.text_right_attachment),
            &[
                "Top of Top",
                "Mid of Top",
                "Mid of Text",
                "Mid of Bot",
                "Bot of Bot",
                "Bottom Line",
            ],
        ),
        // Stats
        ro("Leader Pts", "total_pts", total_pts.to_string()),
        ro("Roots", "root_count", ctx.leader_roots.len().to_string()),
    ];

    // Connection point for first root (most common case)
    if let Some(root) = ctx.leader_roots.first() {
        props.push(edit("Root Conn X", "conn_x", root.connection_point.x));
        props.push(edit("Root Conn Y", "conn_y", root.connection_point.y));
        props.push(edit("Root Conn Z", "conn_z", root.connection_point.z));
    }

    PropSection {
        title: "Geometry".into(),
        props,
    }
}

fn apply_geom_prop(ml: &mut MultiLeader, field: &str, value: &str) {
    let f64 = |s: &str| -> Option<f64> { s.trim().parse().ok() };

    match field {
        "content_type" => {
            ml.content_type = match value {
                "Block" => LeaderContentType::Block,
                "MText" => LeaderContentType::MText,
                "Tolerance" => LeaderContentType::Tolerance,
                _ => LeaderContentType::None,
            };
        }
        "text_string" => ml.context.text_string = value.to_string(),
        "text_height" => {
            if let Some(v) = f64(value) {
                ml.text_height = v;
                ml.context.text_height = v;
            }
        }
        "text_x" => {
            if let Some(v) = f64(value) {
                ml.context.text_location.x = v;
            }
        }
        "text_y" => {
            if let Some(v) = f64(value) {
                ml.context.text_location.y = v;
            }
        }
        "text_z" => {
            if let Some(v) = f64(value) {
                ml.context.text_location.z = v;
            }
        }
        "text_frame" => {
            ml.text_frame = if value == "toggle" {
                !ml.text_frame
            } else {
                value == "true"
            }
        }
        "path_type" => {
            ml.path_type = match value {
                "Spline" => MultiLeaderPathType::Spline,
                "Invisible" => MultiLeaderPathType::Invisible,
                _ => MultiLeaderPathType::StraightLineSegments,
            };
        }
        "enable_landing" => {
            ml.enable_landing = if value == "toggle" {
                !ml.enable_landing
            } else {
                value == "true"
            }
        }
        "enable_dogleg" => {
            ml.enable_dogleg = if value == "toggle" {
                !ml.enable_dogleg
            } else {
                value == "true"
            }
        }
        "enable_annotation_scale" => {
            ml.enable_annotation_scale = if value == "toggle" {
                !ml.enable_annotation_scale
            } else {
                value == "true"
            }
        }
        "dogleg_length" => {
            if let Some(v) = f64(value) {
                ml.dogleg_length = v;
            }
        }
        "arrowhead_size" => {
            if let Some(v) = f64(value) {
                ml.arrowhead_size = v;
            }
        }
        "scale_factor" => {
            if let Some(v) = f64(value) {
                ml.scale_factor = v;
            }
        }
        "conn_x" => {
            if let (Some(v), Some(root)) = (f64(value), ml.context.leader_roots.first_mut()) {
                root.connection_point.x = v;
            }
        }
        "conn_y" => {
            if let (Some(v), Some(root)) = (f64(value), ml.context.leader_roots.first_mut()) {
                root.connection_point.y = v;
            }
        }
        "conn_z" => {
            if let (Some(v), Some(root)) = (f64(value), ml.context.leader_roots.first_mut()) {
                root.connection_point.z = v;
            }
        }
        "text_left_attachment" => {
            ml.text_left_attachment = parse_attachment(value);
            ml.context.text_left_attachment = parse_attachment(value);
        }
        "text_right_attachment" => {
            ml.text_right_attachment = parse_attachment(value);
            ml.context.text_right_attachment = parse_attachment(value);
        }
        _ => {}
    }
}

fn parse_attachment(s: &str) -> TextAttachmentType {
    match s {
        "Top of Top" => TextAttachmentType::TopOfTopLine,
        "Mid of Top" => TextAttachmentType::MiddleOfTopLine,
        "Mid of Bot" => TextAttachmentType::MiddleOfBottomLine,
        "Bot of Bot" => TextAttachmentType::BottomOfBottomLine,
        "Bottom Line" => TextAttachmentType::BottomLine,
        _ => TextAttachmentType::MiddleOfText,
    }
}

// ── Transform ──────────────────────────────────────────────────────────────

fn apply_transform(ml: &mut MultiLeader, t: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(ml, t, |entity, p1, p2| {
        // Reflect every point on the leader (line points, connection points,
        // break-point endpoints) AND every direction vector that drives the
        // text orientation. Without the direction reflection text would keep
        // its original rotation while the leader appears mirrored.
        for root in &mut entity.context.leader_roots {
            for line in &mut root.lines {
                for p in &mut line.points {
                    crate::scene::transform::reflect_xy_point(&mut p.x, &mut p.y, p1, p2);
                }
                for bp in &mut line.break_points {
                    crate::scene::transform::reflect_xy_point(
                        &mut bp.start_point.x,
                        &mut bp.start_point.y,
                        p1,
                        p2,
                    );
                    crate::scene::transform::reflect_xy_point(
                        &mut bp.end_point.x,
                        &mut bp.end_point.y,
                        p1,
                        p2,
                    );
                }
            }
            crate::scene::transform::reflect_xy_point(
                &mut root.connection_point.x,
                &mut root.connection_point.y,
                p1,
                p2,
            );
            reflect_xy_direction(&mut root.direction.x, &mut root.direction.y, p1, p2);
            for bp in &mut root.break_points {
                crate::scene::transform::reflect_xy_point(
                    &mut bp.start_point.x,
                    &mut bp.start_point.y,
                    p1,
                    p2,
                );
                crate::scene::transform::reflect_xy_point(
                    &mut bp.end_point.x,
                    &mut bp.end_point.y,
                    p1,
                    p2,
                );
            }
        }
        crate::scene::transform::reflect_xy_point(
            &mut entity.context.text_location.x,
            &mut entity.context.text_location.y,
            p1,
            p2,
        );
        reflect_xy_direction(
            &mut entity.context.text_direction.x,
            &mut entity.context.text_direction.y,
            p1,
            p2,
        );
        reflect_xy_direction(
            &mut entity.context.base_direction.x,
            &mut entity.context.base_direction.y,
            p1,
            p2,
        );
    });
}

/// Reflect a direction (not position) vector across the mirror line p1→p2.
/// Reflecting a direction is the same as reflecting `p1 + dir` then subtracting
/// the reflection of `p1`, which simplifies to reflecting around the origin.
fn reflect_xy_direction(dx: &mut f64, dy: &mut f64, p1: Vec3, p2: Vec3) {
    let zero = Vec3::ZERO;
    let p2_rel = Vec3::new(p2.x - p1.x, p2.y - p1.y, 0.0);
    let mut tip_x = *dx;
    let mut tip_y = *dy;
    crate::scene::transform::reflect_xy_point(&mut tip_x, &mut tip_y, zero, p2_rel);
    *dx = tip_x;
    *dy = tip_y;
}

// ── Trait impls ────────────────────────────────────────────────────────────

impl TruckConvertible for MultiLeader {
    fn to_truck(&self, document: &acadrust::CadDocument) -> Option<TruckEntity> {
        to_truck(self, document)
    }
}

impl Grippable for MultiLeader {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for MultiLeader {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }
    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for MultiLeader {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
