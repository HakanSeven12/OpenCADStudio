use codec::entities::{
    BlockContentConnectionType, FlowDirectionType, LeaderContentType, LineSpacingStyle,
    MultiLeader, MultiLeaderPathType, MultiLeaderPropertyOverrideFlags, TextAlignmentType,
    TextAttachmentDirectionType, TextAttachmentType,
};
use kernel::geom2d::Transform;

use crate::entities::text_support::{
    layout_mtext, resolve_text_style, MTextRenderOpts, MTextVAnchor, ResolvedTextStyle,
};
use crate::t;

/// How far an underlined text line sits above its underline, as a fraction of
/// the text height. AutoCAD keeps the underlined line's baseline 0.2 × height
/// above the landing that underlines it, whatever the landing gap.
pub(crate) const MLEADER_UNDERLINE_GAP: f32 = 0.2;

/// The text line the landing underlines, or `None` when the attachment draws
/// no underline. The codec names values 5–8 one place off from AutoCAD's:
///
/// | value | codec variant                        | AutoCAD                |
/// |-------|--------------------------------------|------------------------|
/// | 5     | `BottomLine`                         | Underline bottom line  |
/// | 6     | `BottomOfTopLineUnderlineBottomLine` | Underline top line     |
/// | 7     | `BottomOfTopLineUnderlineTopLine`    | Bottom of top line     |
/// | 8     | `BottomOfTopLineUnderlineAll`        | Underline all text     |
///
/// AutoCAD draws a single underline, the landing continued under the text.
/// "Underline all text" underlines the remaining lines through `\L` in the
/// text itself, not with extra rules.
pub(crate) fn underlined_line(attach: TextAttachmentType, line_count: usize) -> Option<usize> {
    match attach {
        TextAttachmentType::BottomLine => Some(line_count.saturating_sub(1)),
        TextAttachmentType::BottomOfTopLineUnderlineBottomLine
        | TextAttachmentType::BottomOfTopLineUnderlineAll => Some(0),
        _ => None,
    }
}

/// Map MLEADER's vertical attachment enum onto the shared `MTextVAnchor`
/// used by `layout_mtext`. Replaces the old `v_offset_for_attachment`
/// (which inlined the offset math); the shared pipeline now derives the
/// offset from the variant + n_lines / line_h.
pub(crate) fn mleader_v_anchor(attach: TextAttachmentType) -> MTextVAnchor {
    match attach {
        TextAttachmentType::TopOfTopLine => MTextVAnchor::Top,
        TextAttachmentType::MiddleOfTopLine => MTextVAnchor::MiddleOfTopLine,
        TextAttachmentType::MiddleOfText
        | TextAttachmentType::CenterOfText
        | TextAttachmentType::CenterOfTextOverline => MTextVAnchor::Middle,
        TextAttachmentType::MiddleOfBottomLine => MTextVAnchor::MiddleOfBottomLine,
        TextAttachmentType::BottomOfBottomLine | TextAttachmentType::BottomLine => {
            MTextVAnchor::Bottom
        }
        TextAttachmentType::BottomOfTopLineUnderlineBottomLine
        | TextAttachmentType::BottomOfTopLineUnderlineTopLine
        | TextAttachmentType::BottomOfTopLineUnderlineAll => MTextVAnchor::BottomOfTopLine,
    }
}
use glam::DVec3;

/// The attachment that governs the text's flow axis: the top/bottom pair for
/// vertically-attached leaders, the left/right pair otherwise — each picked
/// by which side the leader comes from. Shared by the render and the grips.
pub(crate) fn active_vertical_attachment(ml: &MultiLeader) -> TextAttachmentType {
    use codec::entities::multileader::TextAttachmentDirectionType;
    match ml.text_attachment_direction {
        TextAttachmentDirectionType::Vertical => {
            let from_top = ml
                .context
                .leader_roots
                .first()
                .map(|r| r.direction.y < 0.0)
                .unwrap_or(true);
            if from_top {
                ml.text_top_attachment
            } else {
                ml.text_bottom_attachment
            }
        }
        TextAttachmentDirectionType::Horizontal => {
            // A dogleg running along the reading direction reaches the
            // text's left edge; one running against it, the right edge.
            let ctx = &ml.context;
            let td = ctx.text_direction;
            let (tx, ty) = if td.x.abs() > 1e-9 || td.y.abs() > 1e-9 {
                (td.x, td.y)
            } else {
                (ctx.text_rotation.cos(), ctx.text_rotation.sin())
            };
            let from_left = ctx
                .leader_roots
                .first()
                .map_or(true, |r| r.direction.x * tx + r.direction.y * ty >= 0.0);
            if from_left {
                ctx.text_left_attachment
            } else {
                ctx.text_right_attachment
            }
        }
    }
}

use crate::command::EntityTransform;
use crate::entities::common::{
    edit_angle_prop as edit_angle, edit_prop as edit, num_prop as num_row,
    ro_prop as ro, square_grip, triangle_grip,
};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::{SnapHint, TangentGeom};

// ── RenderConvertible ────────────────────────────────────────────────────────

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

fn to_render(ml: &MultiLeader, document: &codec::Document) -> Option<RenderEntity> {
    let nan = [f64::NAN; 3];
    let p3 = |v: &codec::types::Vector3| -> [f64; 3] { [v.x, v.y, v.z] };

    let arrow_size = ml.context.arrowhead_size;
    let draw_arrow = arrow_size > 0.0;
    let invisible = ml.path_type == MultiLeaderPathType::Invisible;

    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut tangents: Vec<TangentGeom> = Vec::new();
    let mut key_verts: Vec<[f64; 3]> = Vec::new();
    let mut snap_pts: Vec<(glam::DVec3, SnapHint)> = Vec::new();
    let mut first = true;

    let node = |arr: [f64; 3]| (glam::DVec3::new(arr[0], arr[1], arr[2]), SnapHint::Node);

    // Text-side geometry, recomputed every frame so dragging the arrow or the
    // text re-mirrors the whole layout. The text block is centred on its grip
    // (text_location); the landing meets the text's near edge and the leader
    // line ends one dogleg before that.
    let text_loc = ml.context.text_location;
    let leader_ref = ml
        .context
        .leader_roots
        .first()
        .and_then(|r| r.lines.first())
        .and_then(|l| l.points.last())
        .copied()
        .or_else(|| ml.context.leader_roots.first().map(|r| r.connection_point))
        .unwrap_or(text_loc);
    // Reading direction of the text / landing — the stored text_direction
    // (UCS X for UCS-placed leaders, world X otherwise), so the whole text-side
    // layout follows the UCS instead of world horizontal.
    let (tdx, tdy) = {
        let td = ml.context.text_direction;
        let l = (td.x * td.x + td.y * td.y).sqrt();
        if l > 1e-9 {
            (td.x / l, td.y / l)
        } else {
            let a = ml.context.text_rotation;
            (a.cos(), a.sin())
        }
    };
    // Which side of the leader the text sits on, measured along that direction.
    let rel_dot = (text_loc.x - leader_ref.x) * tdx + (text_loc.y - leader_ref.y) * tdy;
    let text_sign: f64 = if rel_dot >= 0.0 { 1.0 } else { -1.0 };

    // Lay the text out once, up front: its width fixes where the landing meets
    // the text. The same layout is reused below to emit the glyph strokes.
    let text_layout =
        if ml.content_type == LeaderContentType::MText && !ml.context.text_string.is_empty() {
            let ctx = &ml.context;
            let height = if ctx.text_height > 0.0 {
                ctx.text_height as f32
            } else {
                ml.text_height as f32 * ml.scale_factor as f32
            };
            let td = ctx.text_direction;
            let mut rot = if td.x.abs() > 1e-9 || td.y.abs() > 1e-9 {
                (td.y as f32).atan2(td.x as f32)
            } else {
                ctx.text_rotation as f32
            };
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
            let resolved = resolve_text_style(&style_name, document);
            if resolved.is_upside_down {
                rot += std::f32::consts::PI;
            }
            Some(layout_mtext(&MTextRenderOpts {
                // Not an MTEXT: text in a fixed box, never columnar.
                columns: Default::default(),
                value: &ctx.text_string,
                insertion: [text_loc.x, text_loc.y, text_loc.z],
                height,
                rect_w: ctx.text_width as f32,
                rotation: rot,
                style: &resolved,
                // Side-anchored on the leader-facing edge so the text reads
                // outward; flips live with the leader/text side.
                attach_h_anchor: match ctx.text_attachment_point {
                    codec::entities::multileader::TextAttachmentPointType::Left => 0.0,
                    codec::entities::multileader::TextAttachmentPointType::Center => 0.5,
                    codec::entities::multileader::TextAttachmentPointType::Right => 1.0,
                },
                v_anchor: mleader_v_anchor(if text_sign >= 0.0 {
                    ctx.text_left_attachment
                } else {
                    ctx.text_right_attachment
                }),
                line_spacing_factor: ctx.line_spacing_factor as f32,
                exact_line_spacing: matches!(
                    ctx.line_spacing_style,
                    codec::entities::LineSpacingStyle::Exactly
                ),
                rectangle_height: 0.0,
                vertical_text: false,
                want_glyph_boxes: false,
            }))
        } else {
            None
        };
    let dogleg = if ml.enable_landing && ml.enable_dogleg {
        ml.context
            .leader_roots
            .first()
            .map(|root| root.landing_distance.max(0.0))
            .unwrap_or_else(|| ml.dogleg_length.max(0.0))
    } else {
        0.0
    };

    let landing_gap = ml.context.landing_gap.max(0.0);

    // Landing endpoints.
    let elbow_pt = [
        text_loc.x - text_sign * (dogleg + landing_gap) * tdx,
        text_loc.y - text_sign * (dogleg + landing_gap) * tdy,
        text_loc.z,
    ];

    let text_edge_pt = [
        text_loc.x - text_sign * landing_gap * tdx,
        text_loc.y - text_sign * landing_gap * tdy,
        text_loc.z,
    ];

    for root in &ml.context.leader_roots {
        let cp = &root.connection_point;
        let cp_f = p3(cp);
        snap_pts.push(node(cp_f));
        // A root landing on the text's far side keeps its own landing: from
        // its connection point toward the text by its landing distance.
        let far_side = ml.content_type == LeaderContentType::MText
            && ((cp.x - text_loc.x) * tdx + (cp.y - text_loc.y) * tdy) * text_sign > 0.0;
        let (elbow_pt, text_edge_pt) = if far_side {
            let d = &root.direction;
            let dl = (d.x * d.x + d.y * d.y).sqrt().max(1e-12);
            let reach = if dogleg > 0.0 { root.landing_distance.max(0.0) } else { 0.0 };
            (
                cp_f,
                [cp.x + d.x / dl * reach, cp.y + d.y / dl * reach, cp.z],
            )
        } else {
            (elbow_pt, text_edge_pt)
        };

        for line in &root.lines {
            if line.points.is_empty() {
                continue;
            }

            if !invisible {
                if !first {
                    points.push(nan);
                }
                first = false;

                // Build the full control-point list: line.points + landing point
                let mut ctrl: Vec<[f64; 3]> = line.points.iter().map(|p| p3(p)).collect();

                let last_f = *ctrl.last().unwrap_or(&elbow_pt);

                let dist =
                    ((last_f[0] - elbow_pt[0]).powi(2) + (last_f[1] - elbow_pt[1]).powi(2)).sqrt();

                if dist > 1e-9 {
                    ctrl.push(elbow_pt);
                }
                for &c in &ctrl {
                    key_verts.push(c);
                    snap_pts.push(node(c));
                }

                if line.path_type == MultiLeaderPathType::Spline && ctrl.len() >= 2 {
                    // Catmull-Rom spline through the bend points. Use the leader
                    // line's own path type — a spline-style MultiLeader can carry
                    // straight lines, and splining a straight run bows it.
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

        // Horizontal landing from the leader end to the text's near edge.
        if dogleg > 0.0 {
            points.push(nan);
            points.push(elbow_pt);
            points.push(text_edge_pt);
        }
    }

    // Text strokes, drawn from the layout computed up front (centred on the
    // text grip). The snap node is the grip itself.
    let mut fill_tris = Vec::new();
    if let Some(layout) = &text_layout {
        snap_pts.push(node([text_loc.x, text_loc.y, text_loc.z]));
        for ts in &layout.strokes {
            let ox = ts.origin[0];
            let oy = ts.origin[1];
            for stroke in &ts.strokes {
                if stroke.len() < 2 {
                    continue;
                }
                points.push(nan);
                for &[x, y] in stroke {
                    points.push([ox + x as f64, oy + y as f64, text_loc.z]);
                }
            }
            for &[x, y] in &ts.fill_tris {
                fill_tris.push([ox + x as f64, oy + y as f64, text_loc.z]);
            }
        }
    }

    if points.is_empty() {
        return None;
    }

    Some(RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(points),
        snap_pts,
        tangent_geoms: tangents,
        key_vertices: key_verts,
        fill_tris,
    })
}

// ── Grips ──────────────────────────────────────────────────────────────────
//
// Leader vertices come first, followed by optional elbow, text, and width grips.

/// Side the text reads toward (+1 right / -1 left) and the text box's far
/// edge (the wrap-width grip position). The box width is the explicit wrap
/// width when set, else the natural laid-out width.
fn text_box_geom(ml: &MultiLeader) -> ([f64; 2], [f64; 3]) {
    // Flow axis + far-edge grip point of the MText box: along the rotated
    // baseline for horizontal flow, DOWN the column for vertical — matches
    // the render, so the wrap-width grip rides (and drags) the same limit
    // the layout wraps at.
    let ctx = &ml.context;
    let tl = ctx.text_location;
    let td = ctx.text_direction;
    let mut rot = if td.x.abs() > 1e-9 || td.y.abs() > 1e-9 {
        td.y.atan2(td.x)
    } else {
        ctx.text_rotation
    };
    if ml.text_direction_negative {
        rot += std::f64::consts::PI;
    }
    let vertical = matches!(
        ctx.text_flow_direction,
        codec::entities::multileader::FlowDirectionType::Vertical
    );
    let h_anchor: f32 = match ctx.text_attachment_point {
        codec::entities::multileader::TextAttachmentPointType::Left => 0.0,
        codec::entities::multileader::TextAttachmentPointType::Center => 0.5,
        codec::entities::multileader::TextAttachmentPointType::Right => 1.0,
    };
    let (dir_v, k) = crate::entities::text_support::flow_grip_axis(
        rot,
        vertical,
        h_anchor,
        mleader_v_anchor(active_vertical_attachment(ml)),
    );
    let dir = [dir_v.x, dir_v.y];
    let height = if ctx.text_height > 0.0 {
        ctx.text_height
    } else {
        ml.text_height * ml.scale_factor
    };
    let extent = if ctx.text_width > 1e-6 {
        ctx.text_width
    } else {
        let style = ResolvedTextStyle {
            font_name: "STANDARD".to_string(),
            width_factor: 1.0,
            oblique_angle: 0.0,
            is_backward: false,
            is_upside_down: false,
            is_vertical: false,
        };
        let layout = layout_mtext(&MTextRenderOpts {
            columns: Default::default(),
            value: &ctx.text_string,
            insertion: [0.0, 0.0, 0.0],
            height: height as f32,
            rect_w: 0.0,
            rotation: 0.0,
            style: &style,
            attach_h_anchor: 0.0,
            v_anchor: MTextVAnchor::Top,
            line_spacing_factor: ctx.line_spacing_factor as f32,
            exact_line_spacing: matches!(
                ctx.line_spacing_style,
                codec::entities::LineSpacingStyle::Exactly
            ),
            rectangle_height: 0.0,
            vertical_text: vertical,
            want_glyph_boxes: false,
        });
        let lb = layout.local_bounds;
        let nat = if lb[0] <= lb[2] {
            if vertical {
                (lb[3] - lb[1]) as f64
            } else {
                (lb[2] - lb[0]) as f64
            }
        } else {
            height
        };
        nat.max(height)
    };
    (
        [dir[0] * k, dir[1] * k],
        [tl.x + dir[0] * k * extent, tl.y + dir[1] * k * extent, tl.z],
    )
}
fn mleader_landing_geom(ml: &MultiLeader) -> Option<(DVec3, DVec3, f64, f64, f64)> {
    if ml.content_type != LeaderContentType::MText || !ml.enable_landing || !ml.enable_dogleg {
        return None;
    }

    let text = DVec3::new(
        ml.context.text_location.x,
        ml.context.text_location.y,
        ml.context.text_location.z,
    );

    let td = ml.context.text_direction;

    let axis = {
        let d = DVec3::new(td.x, td.y, td.z);

        if d.length_squared() > 1.0e-18 {
            d.normalize()
        } else {
            let a = ml.context.text_rotation;

            DVec3::new(a.cos(), a.sin(), 0.0)
        }
    };

    let leader_ref = ml
        .context
        .leader_roots
        .first()
        .and_then(|root| root.lines.first())
        .and_then(|line| line.points.last())
        .map(|p| DVec3::new(p.x, p.y, p.z))
        .or_else(|| {
            ml.context.leader_roots.first().map(|root| {
                DVec3::new(
                    root.connection_point.x,
                    root.connection_point.y,
                    root.connection_point.z,
                )
            })
        })
        .unwrap_or(text);

    let sign = if (text - leader_ref).dot(axis) >= 0.0 {
        1.0
    } else {
        -1.0
    };

    let dogleg = ml
        .context
        .leader_roots
        .first()
        .map(|root| root.landing_distance.max(0.0))
        .unwrap_or_else(|| ml.dogleg_length.max(0.0));

    let gap = ml.context.landing_gap.max(0.0);

    if dogleg <= 1.0e-9 {
        return None;
    }

    // The landing starts at the first root's connection point; the text
    // location is the text's corner, off the landing line.
    let elbow = ml
        .context
        .leader_roots
        .first()
        .map(|root| {
            DVec3::new(
                root.connection_point.x,
                root.connection_point.y,
                root.connection_point.z,
            )
        })
        .unwrap_or_else(|| text - axis * (sign * (dogleg + gap)));

    Some((elbow, axis, sign, dogleg, gap))
}

/// Where the text sits off its landing line, across the reading direction.
fn text_offset_across(ml: &MultiLeader, elbow: DVec3, axis: DVec3) -> DVec3 {
    let text = DVec3::new(
        ml.context.text_location.x,
        ml.context.text_location.y,
        ml.context.text_location.z,
    );
    let rel = text - elbow;
    rel - axis * rel.dot(axis)
}

/// Add a leader line ending at `arrow` the way MLEADEREDIT does: onto the
/// leader root on the arrow's side of the text, or onto a new root landing on
/// the text's other side. Other contents take the line on the first root.
pub(crate) fn add_leader_line(ml: &mut MultiLeader, arrow: codec::types::Vector3) {
    let template = ml
        .context
        .leader_roots
        .iter()
        .flat_map(|root| root.lines.first())
        .next()
        .cloned();
    // A new leader line takes the lowest index no line uses. The
    // line count repeats an index once a leader has been removed.
    let used: Vec<i32> = ml
        .context
        .leader_roots
        .iter()
        .flat_map(|root| root.lines.iter().map(|line| line.index))
        .collect();
    let index = (0..).find(|i| !used.contains(i)).unwrap_or(0);
    let mut target = 0usize;
    if ml.content_type == LeaderContentType::MText && !ml.context.leader_roots.is_empty() {
        let tl = ml.context.text_location;
        let td = ml.context.text_direction;
        let (dx, dy) = {
            let l = (td.x * td.x + td.y * td.y).sqrt();
            if l > 1e-9 {
                (td.x / l, td.y / l)
            } else {
                let a = ml.context.text_rotation;
                (a.cos(), a.sin())
            }
        };
        let (_, far) = text_box_geom(ml);
        let span = (far[0] - tl.x) * dx + (far[1] - tl.y) * dy;
        let side = |p: &codec::types::Vector3| {
            let along = (p.x - tl.x) * dx + (p.y - tl.y) * dy;
            if along >= span * 0.5 { 1.0 } else { -1.0 }
        };
        let arrow_side = side(&arrow);
        match ml
            .context
            .leader_roots
            .iter()
            .position(|root| side(&root.connection_point) == arrow_side)
        {
            Some(found) => target = found,
            None => {
                let landing = ml.context.leader_roots[0].landing_distance;
                let reach = landing + ml.context.landing_gap.max(0.0);
                let edge = if arrow_side > 0.0 { span.max(0.0) } else { span.min(0.0) };
                let along = edge + arrow_side * reach;
                let template_root = ml.context.leader_roots[0].clone();
                // Same height across the text as the first root's landing.
                let c0 = template_root.connection_point;
                let across = (c0.x - tl.x) * -dy + (c0.y - tl.y) * dx;
                let root = ml.context.add_leader_root();
                root.connection_point = codec::types::Vector3::new(
                    tl.x + dx * along - dy * across,
                    tl.y + dy * along + dx * across,
                    tl.z,
                );
                root.direction = codec::types::Vector3::new(-arrow_side * dx, -arrow_side * dy, 0.0);
                root.landing_distance = landing;
                root.text_attachment_direction = template_root.text_attachment_direction;
                target = ml.context.leader_roots.len() - 1;
            }
        }
    }
    if ml.context.leader_roots.is_empty() {
        ml.context.add_leader_root();
    }
    let root = &mut ml.context.leader_roots[target];
    let line = root.create_line(vec![arrow]);
    line.index = index;
    if let Some(template) = template {
        line.path_type = template.path_type;
        line.line_color = template.line_color;
        line.line_type_handle = template.line_type_handle;
        line.line_weight = template.line_weight;
        line.arrowhead_handle = template.arrowhead_handle;
        line.arrowhead_size = template.arrowhead_size;
        line.override_flags = template.override_flags;
    } else {
        line.path_type = ml.path_type;
        line.line_color = ml.line_color;
        line.line_type_handle = ml.line_type_handle;
        line.line_weight = ml.line_weight;
        line.arrowhead_handle = ml.arrowhead_handle;
        line.arrowhead_size = ml.arrowhead_size;
    }
}

fn set_mleader_text_location(ml: &mut MultiLeader, location: DVec3) {
    let location = codec::types::Vector3::new(location.x, location.y, location.z);
    ml.context.text_location = location;
    ml.context.content_base_point = location;
}

fn grips(ml: &MultiLeader) -> Vec<GripDef> {
    let mut result = Vec::new();
    let mut id = 0usize;

    for root in &ml.context.leader_roots {
        for line in &root.lines {
            for p in &line.points {
                result.push(square_grip(id, DVec3::new(p.x, p.y, p.z)));

                id += 1;
            }
        }
    }

    if ml.content_type == LeaderContentType::MText {
        // The landing's start, then its end (a triangle pointing at the
        // text: drag it to lengthen the landing).
        if let Some((elbow, axis, sign, dogleg, _)) = mleader_landing_geom(ml) {
            result.push(square_grip(id, elbow));
            id += 1;
            let mut end = triangle_grip(id, elbow + axis * (sign * dogleg));
            end.dir = Some(axis * sign);
            result.push(end);
            id += 1;
        }

        let tl = &ml.context.text_location;

        result.push(square_grip(id, DVec3::new(tl.x, tl.y, tl.z)));

        id += 1;

        // A wrap width shows its own triangle; text without one has none.
        if ml.context.text_width > 1.0e-9 {
            let (_, far) = text_box_geom(ml);
            result.push(triangle_grip(id, DVec3::new(far[0], far[1], far[2])));
        }
    }

    result
}

/// Sentinel grip id meaning "translate the whole multileader" — used by the
/// text grip's "Move with Leader" action so the leader follows the text.
pub(crate) const MOVE_ALL_GRIP: usize = usize::MAX;

fn apply_grip(ml: &mut MultiLeader, grip_id: usize, apply: GripApply) {
    if grip_id == MOVE_ALL_GRIP {
        let (dx, dy, dz) = match apply {
            GripApply::Translate(d) => (d.x as f64, d.y as f64, d.z as f64),

            GripApply::Absolute(a) => (
                a.x as f64 - ml.context.text_location.x,
                a.y as f64 - ml.context.text_location.y,
                a.z as f64 - ml.context.text_location.z,
            ),
        };

        for root in &mut ml.context.leader_roots {
            for line in &mut root.lines {
                for p in &mut line.points {
                    p.x += dx;
                    p.y += dy;
                    p.z += dz;
                }
            }

            root.connection_point.x += dx;
            root.connection_point.y += dy;
            root.connection_point.z += dz;
        }

        set_mleader_text_location(
            ml,
            DVec3::new(
                ml.context.text_location.x + dx,
                ml.context.text_location.y + dy,
                ml.context.text_location.z + dz,
            ),
        );

        return;
    }

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

    if ml.content_type != LeaderContentType::MText {
        return;
    }

    let landing = mleader_landing_geom(ml);

    if let Some((old_elbow, axis, old_sign, dogleg, gap)) = landing {
        if grip_id == idx {
            let target = match apply {
                GripApply::Absolute(a) => DVec3::new(a.x as f64, a.y as f64, a.z as f64),
                GripApply::Translate(d) => {
                    old_elbow + DVec3::new(d.x as f64, d.y as f64, d.z as f64)
                }
            };
            let arrow = ml
                .context
                .leader_roots
                .first()
                .and_then(|root| root.lines.first())
                .and_then(|line| line.points.first())
                .map(|p| DVec3::new(p.x, p.y, p.z))
                .unwrap_or(old_elbow);
            let side_distance = (target - arrow).dot(axis);
            let flip_tol = (dogleg * 0.10).max(1.0e-6);
            let new_sign = if side_distance > flip_tol {
                1.0
            } else if side_distance < -flip_tol {
                -1.0
            } else {
                old_sign
            };
            let across = text_offset_across(ml, old_elbow, axis);
            set_mleader_text_side(ml, new_sign >= 0.0);
            set_mleader_text_location(
                ml,
                target + axis * (new_sign * (dogleg + gap)) + across,
            );

            if let Some(root) = ml.context.leader_roots.first_mut() {
                root.connection_point.x = target.x;
                root.connection_point.y = target.y;
                root.connection_point.z = target.z;
                root.direction.x = axis.x * new_sign;
                root.direction.y = axis.y * new_sign;
                root.direction.z = axis.z * new_sign;
            }
            return;
        }

        idx += 1;

        // Landing end: lengthen the landing along its own line; the text
        // keeps its gap past the new end.
        if grip_id == idx {
            let old_end = old_elbow + axis * (old_sign * dogleg);
            let target = match apply {
                GripApply::Absolute(a) => DVec3::new(a.x as f64, a.y as f64, a.z as f64),
                GripApply::Translate(d) => {
                    old_end + DVec3::new(d.x as f64, d.y as f64, d.z as f64)
                }
            };
            let new_length = ((target - old_elbow).dot(axis) * old_sign).max(1.0e-6);
            let across = text_offset_across(ml, old_elbow, axis);
            set_mleader_text_location(
                ml,
                old_elbow + axis * (old_sign * (new_length + gap)) + across,
            );
            ml.dogleg_length = new_length;
            for root in &mut ml.context.leader_roots {
                root.landing_distance = new_length;
            }
            return;
        }

        idx += 1;
    }

    // Text: the content moves and its landing follows; the arrowheads stay.
    if grip_id == idx {
        let old_text = DVec3::new(
            ml.context.text_location.x,
            ml.context.text_location.y,
            ml.context.text_location.z,
        );
        let target = match apply {
            GripApply::Absolute(a) => DVec3::new(a.x as f64, a.y as f64, a.z as f64),
            GripApply::Translate(d) => old_text + DVec3::new(d.x as f64, d.y as f64, d.z as f64),
        };
        let shift = target - old_text;
        set_mleader_text_location(ml, target);
        for root in &mut ml.context.leader_roots {
            root.connection_point.x += shift.x;
            root.connection_point.y += shift.y;
            root.connection_point.z += shift.z;
        }
        return;
    }

    idx += 1;

    if grip_id == idx {
        let (dir, far) = text_box_geom(ml);

        let (nx, ny) = match apply {
            GripApply::Absolute(a) => (a.x as f64, a.y as f64),

            GripApply::Translate(d) => (far[0] + d.x as f64, far[1] + d.y as f64),
        };

        let tl = &ml.context.text_location;

        let d2 = (dir[0] * dir[0] + dir[1] * dir[1]).max(1.0e-12);

        let proj = ((nx - tl.x) * dir[0] + (ny - tl.y) * dir[1]) / d2;

        let min_w = ml.text_height.max(1.0) * 0.5;

        ml.context.text_width = proj.max(min_w);
    }
}

// ── Properties ─────────────────────────────────────────────────────────────

/// The nine horizontal text-attachment options, indexed 1:1 to
/// `TextAttachmentType` values 0–8 (values 9/10 are the vertical set), named
/// as AutoCAD names each stored value. The codec's variant names for 5–8 are
/// one place off: 6 (`BottomOfTopLineUnderlineBottomLine`) is AutoCAD's
/// "Underline top line" and 7 (`BottomOfTopLineUnderlineTopLine`) its
/// "Bottom of top line".
const ATTACH_LABELS: [&str; 9] = [
    "Top of top line",
    "Middle of top line",
    "Middle of text",
    "Middle of bottom line",
    "Bottom of bottom line",
    "Underline bottom line",
    "Underline top line",
    "Bottom of top line",
    "Underline all text",
];

fn attachment_str(a: &TextAttachmentType) -> &'static str {
    match a {
        TextAttachmentType::TopOfTopLine => ATTACH_LABELS[0],
        TextAttachmentType::MiddleOfTopLine => ATTACH_LABELS[1],
        TextAttachmentType::MiddleOfText => ATTACH_LABELS[2],
        TextAttachmentType::MiddleOfBottomLine => ATTACH_LABELS[3],
        TextAttachmentType::BottomOfBottomLine => ATTACH_LABELS[4],
        TextAttachmentType::BottomLine => ATTACH_LABELS[5],
        TextAttachmentType::BottomOfTopLineUnderlineBottomLine => ATTACH_LABELS[6],
        TextAttachmentType::BottomOfTopLineUnderlineTopLine => ATTACH_LABELS[7],
        TextAttachmentType::BottomOfTopLineUnderlineAll => ATTACH_LABELS[8],
        TextAttachmentType::CenterOfText => "Center of text",
        TextAttachmentType::CenterOfTextOverline => "Center of text (overline)",
    }
}

fn leader_type_str(pt: &MultiLeaderPathType) -> &'static str {
    match pt {
        MultiLeaderPathType::Spline => "Spline",
        MultiLeaderPathType::Invisible => "None",
        MultiLeaderPathType::StraightLineSegments => "Straight",
    }
}

fn text_align_str(a: &TextAlignmentType) -> &'static str {
    match a {
        TextAlignmentType::Left => "Left",
        TextAlignmentType::Center => "Center",
        TextAlignmentType::Right => "Right",
    }
}

fn flow_dir_str(d: &FlowDirectionType) -> &'static str {
    match d {
        FlowDirectionType::Horizontal => "Left to right",
        FlowDirectionType::Vertical => "Top to bottom",
        FlowDirectionType::ByStyle => "By style",
    }
}

fn attach_dir_str(d: &TextAttachmentDirectionType) -> &'static str {
    match d {
        TextAttachmentDirectionType::Horizontal => "Horizontal",
        TextAttachmentDirectionType::Vertical => "Vertical",
    }
}

fn line_style_str(s: &LineSpacingStyle) -> &'static str {
    match s {
        LineSpacingStyle::Exactly => "Exactly",
        _ => "At least",
    }
}

/// A Yes/No list, as the reference shows its on/off rows.
fn bool_toggle(label: &str, field: &'static str, value: bool) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::Choice {
            selected: if value { t!("Yes") } else { t!("No") }.into_owned(),
            options: vec![t!("Yes").into_owned(), t!("No").into_owned()],
        },
    }
}

/// A Yes/No row's pick as the "true" / "false" its field reads.
fn yes_no_value(field: &str, value: &str) -> String {
    let flag = matches!(
        field,
        "text_frame"
            | "enable_landing"
            | "enable_dogleg"
            | "extend_leader_to_text"
            | "enable_annotation_scale"
            | "background_fill_enabled"
    );
    if !flag || value == "toggle" || value == "true" || value == "false" {
        return value.to_string();
    }
    (value.eq_ignore_ascii_case("yes") || value == t!("Yes").as_ref()).to_string()
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

fn hexh(h: Option<codec::Handle>) -> String {
    match h {
        Some(h) if !h.is_null() => format!("{:X}", h.value()),
        _ => "(none)".to_string(),
    }
}

fn properties(ml: &MultiLeader) -> Vec<PropSection> {
    let ctx = &ml.context;

    // ── Misc ─────────────────────────────────────────────────────────────
    let misc = PropSection {
        title: t!("Misc").into_owned(),
        props: vec![
            // Overall scale is grayed when annotative (annotation scale drives sizing).
            num_row(
                t!("Overall scale").as_ref(),
                "scale_factor",
                ml.scale_factor,
                !ml.enable_annotation_scale,
            ),
            // Style name is resolved from style_handle by the panel builder (needs doc).
            ro(
                t!("Multileader style").as_ref(),
                "mleader_style",
                "Standard",
            ),
            bool_toggle(
                t!("Annotative").as_ref(),
                "annotative",
                ml.enable_annotation_scale,
            ),
        ],
    };

    // ── Leaders ──────────────────────────────────────────────────────────
    // Landing rows are folded in here: the standalone "Leader Structure" group
    // is a style-dialog tab, not a palette group.
    let mut leader_props = vec![
        choice(
            t!("Leader type").as_ref(),
            "path_type",
            leader_type_str(&ml.path_type),
            &["Straight", "Spline", "None"],
        ),
        Property {
            label: t!("Leader color").into_owned(),
            field: "line_color",
            value: PropValue::ColorChoice(ml.line_color),
        },
        // Linetype name resolved from line_type_handle by the panel builder.
        ro(
            t!("Leader linetype").as_ref(),
            "line_type_handle",
            "ByBlock",
        ),
        Property {
            label: t!("Leader lineweight").into_owned(),
            field: "line_weight",
            value: PropValue::FieldLwChoice {
                field: "line_weight",
                value: ml.line_weight,
            },
        },
        // Arrowhead block name resolved by the panel builder (default "Closed filled").
        ro(
            t!("Arrowhead").as_ref(),
            "arrowhead_handle",
            "Closed filled",
        ),
        edit(
            t!("Arrowhead Size").as_ref(),
            "arrowhead_size",
            ml.arrowhead_size,
        ),
        bool_toggle(
            t!("Horizontal Landing").as_ref(),
            "enable_dogleg",
            ml.enable_dogleg,
        ),
        num_row(
            t!("Landing distance").as_ref(),
            "landing_distance",
            if ml.enable_dogleg {
                ml.dogleg_length
            } else {
                0.0
            },
            ml.enable_dogleg,
        ),
    ];
    // The extension is an MText-only landing option. It is absent for block
    // content and whenever the horizontal landing itself is disabled.
    if ml.content_type == LeaderContentType::MText && ml.enable_dogleg {
        leader_props.push(bool_toggle(
            t!("Leader extension").as_ref(),
            "extend_leader_to_text",
            ml.extend_leader_to_text,
        ));
    }
    let leaders = PropSection {
        title: t!("Leaders").into_owned(),
        props: leader_props,
    };

    // ── Text (shown only for MText content) ──────────────────────────────
    let text = PropSection {
        title: t!("Text").into_owned(),
        props: vec![
            Property {
                label: t!("Contents").into_owned(),
                field: "text_string",
                value: PropValue::PlainText(ctx.text_string.clone()),
            },
            // Text-style name resolved from text_style_handle by the panel builder.
            ro(t!("Text style").as_ref(), "text_style_handle", "Standard"),
            choice(
                t!("Justify").as_ref(),
                "text_alignment",
                text_align_str(&ml.text_alignment),
                &["Left", "Center", "Right"],
            ),
            choice(
                t!("Direction").as_ref(),
                "text_flow_direction",
                flow_dir_str(&ctx.text_flow_direction),
                &["By style", "Left to right", "Top to bottom"],
            ),
            edit(t!("Width").as_ref(), "text_width", ctx.text_width),
            edit(t!("Height").as_ref(), "text_height", ml.text_height),
            edit_angle(
                t!("Rotation").as_ref(),
                "text_rotation",
                ctx.text_rotation.to_degrees(),
            ),
            edit(
                t!("Line space factor").as_ref(),
                "line_spacing",
                ctx.line_spacing_factor,
            ),
            edit(
                t!("Line space distance").as_ref(),
                "line_space_distance",
                ml.text_height * 1.666_666_666_666_667 * ctx.line_spacing_factor,
            ),
            choice(
                t!("Line space style").as_ref(),
                "line_space_style",
                line_style_str(&ctx.line_spacing_style),
                &["At least", "Exactly"],
            ),
            bool_toggle(
                t!("Background mask").as_ref(),
                "background_fill_enabled",
                ctx.background_fill_enabled,
            ),
            choice(
                t!("Attachment type").as_ref(),
                "text_attachment_direction",
                attach_dir_str(&ml.text_attachment_direction),
                &["Horizontal", "Vertical"],
            ),
            edit(
                t!("Landing gap").as_ref(),
                "landing_gap",
                ctx.landing_gap / ctx.scale_factor.max(1.0e-12),
            ),
            bool_toggle(t!("Text frame").as_ref(), "text_frame", ml.text_frame),
        ],
    };
    let mut text = text;
    let attachment_insert = text.props.len().saturating_sub(2);
    let attachment_rows = match ml.text_attachment_direction {
        TextAttachmentDirectionType::Horizontal => vec![
            choice(
                t!("Left Attachment").as_ref(),
                "text_left_attachment",
                attachment_str(&ml.text_left_attachment),
                &ATTACH_LABELS,
            ),
            choice(
                t!("Right Attachment").as_ref(),
                "text_right_attachment",
                attachment_str(&ml.text_right_attachment),
                &ATTACH_LABELS,
            ),
        ],
        TextAttachmentDirectionType::Vertical => vec![
            choice(
                t!("Top Attachment").as_ref(),
                "text_top_attachment",
                attachment_str(&ml.text_top_attachment),
                &ATTACH_LABELS,
            ),
            choice(
                t!("Bottom Attachment").as_ref(),
                "text_bottom_attachment",
                attachment_str(&ml.text_bottom_attachment),
                &ATTACH_LABELS,
            ),
        ],
    };
    text.props
        .splice(attachment_insert..attachment_insert, attachment_rows);

    // ── Block ────────────────────────────────────────────────────────────
    let block = PropSection {
        title: t!("Block").into_owned(),
        props: vec![
            ro(
                t!("Source block").as_ref(),
                "block_content_handle",
                hexh(ml.block_content_handle),
            ),
            choice(
                t!("Attachment").as_ref(),
                "block_connection_type",
                match ml.block_connection_type {
                    BlockContentConnectionType::BasePoint => "Insertion point",
                    BlockContentConnectionType::BlockExtents => "Extents",
                },
                &["Extents", "Insertion point"],
            ),
            edit(t!("Scale").as_ref(), "block_scale", ml.block_scale.x),
        ],
    };

    // Text and Block groups are mutually exclusive, keyed on the content type;
    // only one is ever shown (neither for None/Tolerance).
    let mut sections = vec![misc, leaders];
    match ml.content_type {
        LeaderContentType::MText => sections.push(text),
        LeaderContentType::Block => sections.push(block),
        _ => {}
    }
    sections
}

fn apply_geom_prop(ml: &mut MultiLeader, field: &str, value: &str) {
    let f64 = |s: &str| -> Option<f64> { s.trim().parse().ok() };
    let value = yes_no_value(field, value);
    let value = value.as_str();

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
                ml.context.text_height = v * ml.context.scale_factor.max(1.0e-12);
                ml.property_override_flags
                    .insert(MultiLeaderPropertyOverrideFlags::TEXT_HEIGHT);
            }
        }
        "text_x" => {
            if let Some(v) = f64(value) {
                ml.context.text_location.x = v;
                ml.context.content_base_point.x = v;
            }
        }
        "text_y" => {
            if let Some(v) = f64(value) {
                ml.context.text_location.y = v;
                ml.context.content_base_point.y = v;
            }
        }
        "text_z" => {
            if let Some(v) = f64(value) {
                ml.context.text_location.z = v;
                ml.context.content_base_point.z = v;
            }
        }
        "text_frame" => {
            ml.text_frame = if value == "toggle" {
                !ml.text_frame
            } else {
                value == "true"
            };
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::TEXT_FRAME);
        }
        "path_type" => {
            ml.path_type = match value {
                "Spline" => MultiLeaderPathType::Spline,
                "None" => MultiLeaderPathType::Invisible,
                _ => MultiLeaderPathType::StraightLineSegments,
            };
            for root in &mut ml.context.leader_roots {
                for line in &mut root.lines {
                    line.path_type = ml.path_type;
                    line.override_flags
                        .insert(codec::entities::LeaderLinePropertyOverrideFlags::PATH_TYPE);
                }
            }
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::PATH_TYPE);
        }
        "enable_landing" => {
            ml.enable_landing = if value == "toggle" {
                !ml.enable_landing
            } else {
                value == "true"
            };
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::ENABLE_LANDING);
        }
        "enable_dogleg" => {
            ml.enable_dogleg = if value == "toggle" {
                !ml.enable_dogleg
            } else {
                value == "true"
            };
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::ENABLE_DOGLEG);
        }
        "extend_leader_to_text" => {
            ml.extend_leader_to_text = if value == "toggle" {
                !ml.extend_leader_to_text
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
        "background_fill_enabled" => {
            ml.context.background_fill_enabled = if value == "toggle" {
                !ml.context.background_fill_enabled
            } else {
                value == "true"
            };
            // Properties offers only this switch, no fill colour, so a mask
            // switched on there uses the drawing background like AutoCAD.
            // Without a colour of its own the fill would inherit the text
            // colour and hide the text (#2131).
            if ml.context.background_fill_enabled
                && ml.context.background_fill_color.rgb().is_none()
            {
                ml.context.background_mask_fill_on = true;
            }
        }
        "background_mask_fill_on" => {
            ml.context.background_mask_fill_on = if value == "toggle" {
                !ml.context.background_mask_fill_on
            } else {
                value == "true"
            }
        }
        "background_scale_factor" => {
            if let Some(v) = f64(value) {
                if v > 0.0 {
                    ml.context.background_scale_factor = v;
                }
            }
        }
        "background_transparency" => {
            if let Ok(v) = value.trim().parse::<i32>() {
                ml.context.background_transparency = v.clamp(0, 90);
            }
        }
        "dogleg_length" => {
            if let Some(v) = f64(value) {
                ml.dogleg_length = v;
                ml.property_override_flags
                    .insert(MultiLeaderPropertyOverrideFlags::LANDING_DISTANCE);
            }
        }
        "arrowhead_size" => {
            if let Some(v) = f64(value) {
                ml.arrowhead_size = v;
                ml.context.arrowhead_size = v * ml.context.scale_factor.max(1.0e-12);
                for root in &mut ml.context.leader_roots {
                    for line in &mut root.lines {
                        line.arrowhead_size = v;
                        line.override_flags.insert(
                            codec::entities::LeaderLinePropertyOverrideFlags::ARROWHEAD_SIZE,
                        );
                    }
                }
                ml.property_override_flags
                    .insert(MultiLeaderPropertyOverrideFlags::ARROWHEAD_SIZE);
            }
        }
        "scale_factor" => {
            if let Some(v) = f64(value) {
                if v > 0.0 {
                    let old = ml.context.scale_factor.max(1.0e-12);
                    let ratio = v / old;
                    ml.scale_factor = v;
                    ml.context.scale_factor = v;
                    ml.context.arrowhead_size *= ratio;
                    ml.context.text_height *= ratio;
                    ml.context.landing_gap *= ratio;
                    ml.block_scale.x *= ratio;
                    ml.block_scale.y *= ratio;
                    ml.block_scale.z *= ratio;
                    ml.context.block_content_scale = ml.block_scale;
                    if let Some(root) = ml.context.leader_roots.first() {
                        let anchor = root.connection_point;
                        for point in [
                            &mut ml.context.content_base_point,
                            &mut ml.context.text_location,
                            &mut ml.context.block_content_location,
                        ] {
                            point.x = anchor.x + (point.x - anchor.x) * ratio;
                            point.y = anchor.y + (point.y - anchor.y) * ratio;
                            point.z = anchor.z + (point.z - anchor.z) * ratio;
                        }
                    }
                    for root in &mut ml.context.leader_roots {
                        root.landing_distance *= ratio;
                    }
                    ml.property_override_flags
                        .insert(MultiLeaderPropertyOverrideFlags::SCALE_FACTOR);
                }
            }
        }
        "landing_distance" => {
            if let Some(v) = f64(value) {
                ml.dogleg_length = v;
                let display = v * ml.context.scale_factor.max(1.0e-12);
                for root in &mut ml.context.leader_roots {
                    root.landing_distance = display;
                }
                place_content_after_landing(ml);
                ml.property_override_flags
                    .insert(MultiLeaderPropertyOverrideFlags::LANDING_DISTANCE);
            }
        }
        "landing_gap" => {
            if let Some(v) = f64(value) {
                ml.context.landing_gap = v * ml.context.scale_factor.max(1.0e-12);
                place_content_after_landing(ml);
                ml.property_override_flags
                    .insert(MultiLeaderPropertyOverrideFlags::LANDING_GAP);
            }
        }
        "block_rotation" => {
            if let Some(v) = f64(value) {
                ml.block_rotation = v.to_radians();
                ml.context.block_rotation = ml.block_rotation;
                ml.property_override_flags
                    .insert(MultiLeaderPropertyOverrideFlags::BLOCK_CONTENT_ROTATION);
            }
        }
        "block_scale" => {
            if let Some(v) = f64(value) {
                if v > 0.0 {
                    ml.block_scale = codec::types::Vector3::new(v, v, v);
                    ml.context.block_content_scale = ml.block_scale;
                    ml.property_override_flags
                        .insert(MultiLeaderPropertyOverrideFlags::BLOCK_CONTENT_SCALE);
                }
            }
        }
        "block_connection_type" => {
            ml.block_connection_type = if value == "Insertion point" {
                BlockContentConnectionType::BasePoint
            } else {
                BlockContentConnectionType::BlockExtents
            };
            ml.context.block_connection_type = ml.block_connection_type;
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::BLOCK_CONTENT_CONNECTION);
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
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::TEXT_LEFT_ATTACHMENT);
        }
        "text_right_attachment" => {
            ml.text_right_attachment = parse_attachment(value);
            ml.context.text_right_attachment = parse_attachment(value);
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::TEXT_RIGHT_ATTACHMENT);
        }
        "text_top_attachment" => {
            ml.text_top_attachment = parse_attachment(value);
            ml.context.text_top_attachment = parse_attachment(value);
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::TEXT_TOP_ATTACHMENT);
        }
        "text_bottom_attachment" => {
            ml.text_bottom_attachment = parse_attachment(value);
            ml.context.text_bottom_attachment = parse_attachment(value);
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::TEXT_BOTTOM_ATTACHMENT);
        }
        "text_alignment" => {
            ml.text_alignment = match value {
                "Center" => TextAlignmentType::Center,
                "Right" => TextAlignmentType::Right,
                _ => TextAlignmentType::Left,
            };
            ml.context.text_alignment = match value {
                "Center" => TextAlignmentType::Center,
                "Right" => TextAlignmentType::Right,
                _ => TextAlignmentType::Left,
            };
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::TEXT_ALIGNMENT);
        }
        "text_flow_direction" => {
            ml.context.text_flow_direction = match value {
                "Left to right" => FlowDirectionType::Horizontal,
                "Top to bottom" => FlowDirectionType::Vertical,
                _ => FlowDirectionType::ByStyle,
            };
        }
        "text_attachment_direction" => {
            ml.text_attachment_direction = match value {
                "Vertical" => TextAttachmentDirectionType::Vertical,
                _ => TextAttachmentDirectionType::Horizontal,
            };
            if ml.text_attachment_direction == TextAttachmentDirectionType::Vertical {
                ml.enable_dogleg = false;
                ml.property_override_flags
                    .insert(MultiLeaderPropertyOverrideFlags::ENABLE_DOGLEG);
            }
            for root in &mut ml.context.leader_roots {
                root.text_attachment_direction = ml.text_attachment_direction;
            }
            ml.property_override_flags
                .insert(MultiLeaderPropertyOverrideFlags::TEXT_ATTACHMENT_DIRECTION);
        }
        "line_space_style" => {
            ml.context.line_spacing_style = match value {
                "Exactly" => LineSpacingStyle::Exactly,
                _ => LineSpacingStyle::AtLeast,
            };
        }
        "text_width" => {
            if let Some(v) = f64(value) {
                ml.context.text_width = v;
            }
        }
        "text_rotation" => {
            if let Some(v) = f64(value) {
                ml.context.text_rotation = v.to_radians();
                ml.context.text_direction = codec::types::Vector3::new(
                    ml.context.text_rotation.cos(),
                    ml.context.text_rotation.sin(),
                    0.0,
                );
                ml.property_override_flags
                    .insert(MultiLeaderPropertyOverrideFlags::TEXT_ANGLE);
            }
        }
        "line_spacing" => {
            if let Some(v) = f64(value) {
                if v > 0.0 {
                    ml.context.line_spacing_factor = v;
                }
            }
        }
        "line_space_distance" => {
            if let Some(v) = f64(value) {
                let denom = ml.text_height * 1.666_666_666_666_667;
                if v > 0.0 && denom > 0.0 {
                    ml.context.line_spacing_factor = v / denom;
                }
            }
        }
        _ => {}
    }
}

fn place_content_after_landing(ml: &mut MultiLeader) {
    let Some(root) = ml.context.leader_roots.first() else {
        return;
    };
    let length = (root.direction.x * root.direction.x + root.direction.y * root.direction.y).sqrt();
    let (ux, uy) = if length > 1.0e-12 {
        (root.direction.x / length, root.direction.y / length)
    } else {
        (1.0, 0.0)
    };
    let distance = root.landing_distance.max(0.0) + ml.context.landing_gap.max(0.0);
    let location = codec::types::Vector3::new(
        root.connection_point.x + ux * distance,
        root.connection_point.y + uy * distance,
        root.connection_point.z,
    );
    ml.context.content_base_point = location;
    ml.context.text_location = location;
    ml.context.block_content_location = location;
}

fn parse_attachment(s: &str) -> TextAttachmentType {
    match s {
        "Top of top line" => TextAttachmentType::TopOfTopLine,
        "Middle of top line" => TextAttachmentType::MiddleOfTopLine,
        "Middle of bottom line" => TextAttachmentType::MiddleOfBottomLine,
        "Bottom of bottom line" => TextAttachmentType::BottomOfBottomLine,
        // Values 5–8 by AutoCAD's names; see `ATTACH_LABELS`.
        "Underline bottom line" => TextAttachmentType::BottomLine,
        "Underline top line" => TextAttachmentType::BottomOfTopLineUnderlineBottomLine,
        "Bottom of top line" => TextAttachmentType::BottomOfTopLineUnderlineTopLine,
        "Underline all text" | "Underline all" => TextAttachmentType::BottomOfTopLineUnderlineAll,
        _ => TextAttachmentType::MiddleOfText,
    }
}

// ── Transform ──────────────────────────────────────────────────────────────

fn apply_transform(ml: &mut MultiLeader, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(ml, t, |entity, p1, p2| {
        // Reflect every point on the leader (line points, connection points,
        // break-point endpoints) and the leader directions; the content
        // orientation follows AutoCAD (`mirror_content_orientation`).
        for root in &mut entity.context.leader_roots {
            for line in &mut root.lines {
                for p in &mut line.points {
                    crate::scene::view::transform::reflect_xy_point(&mut p.x, &mut p.y, p1, p2);
                }
                for bp in &mut line.break_points {
                    crate::scene::view::transform::reflect_xy_point(
                        &mut bp.start_point.x,
                        &mut bp.start_point.y,
                        p1,
                        p2,
                    );
                    crate::scene::view::transform::reflect_xy_point(
                        &mut bp.end_point.x,
                        &mut bp.end_point.y,
                        p1,
                        p2,
                    );
                }
            }
            crate::scene::view::transform::reflect_xy_point(
                &mut root.connection_point.x,
                &mut root.connection_point.y,
                p1,
                p2,
            );
            reflect_xy_direction(&mut root.direction.x, &mut root.direction.y, p1, p2);
            for bp in &mut root.break_points {
                crate::scene::view::transform::reflect_xy_point(
                    &mut bp.start_point.x,
                    &mut bp.start_point.y,
                    p1,
                    p2,
                );
                crate::scene::view::transform::reflect_xy_point(
                    &mut bp.end_point.x,
                    &mut bp.end_point.y,
                    p1,
                    p2,
                );
            }
        }
        for point in [
            &mut entity.context.text_location,
            &mut entity.context.content_base_point,
            &mut entity.context.block_content_location,
        ] {
            crate::scene::view::transform::reflect_xy_point(&mut point.x, &mut point.y, p1, p2);
        }
        mirror_content_orientation(entity, p1, p2);
    });
}

/// MIRROR of the content, as AutoCAD does it. MTEXT stays readable whatever
/// MIRRTEXT says: its direction and rotation are kept and the text moves to
/// the other side of its location. Block content becomes a true mirror
/// image: X scale negated, rotation `2·axis + 180° − rotation`.
fn mirror_content_orientation(ml: &mut MultiLeader, p1: DVec3, p2: DVec3) {
    use codec::entities::multileader::TextAttachmentPointType;
    // AutoCAD changes only the drawn (context) content; the entity-level
    // values are style overrides and keep their values.
    let (attachment, alignment) = match ml.context.text_attachment_point {
        TextAttachmentPointType::Left => (TextAttachmentPointType::Right, TextAlignmentType::Right),
        TextAttachmentPointType::Right => (TextAttachmentPointType::Left, TextAlignmentType::Left),
        TextAttachmentPointType::Center => {
            (TextAttachmentPointType::Center, ml.context.text_alignment)
        }
    };
    ml.context.text_attachment_point = attachment;
    if ml.context.text_alignment != TextAlignmentType::Center {
        ml.context.text_alignment = alignment;
    }
    ml.property_override_flags |= MultiLeaderPropertyOverrideFlags::TEXT_ALIGNMENT;
    let axis = (p2.y - p1.y).atan2(p2.x - p1.x);
    ml.context.block_content_scale.x = -ml.context.block_content_scale.x;
    ml.context.block_rotation = (2.0 * axis + std::f64::consts::PI - ml.context.block_rotation)
        .rem_euclid(std::f64::consts::TAU);
}

/// Reflect a direction (not position) vector across the mirror line p1→p2.
/// Reflecting a direction is the same as reflecting `p1 + dir` then subtracting
/// the reflection of `p1`, which simplifies to reflecting around the origin.
fn reflect_xy_direction(dx: &mut f64, dy: &mut f64, p1: DVec3, p2: DVec3) {
    let zero = DVec3::ZERO;
    let p2_rel = DVec3::new(p2.x - p1.x, p2.y - p1.y, 0.0);
    let mut tip_x = *dx;
    let mut tip_y = *dy;
    crate::scene::view::transform::reflect_xy_point(&mut tip_x, &mut tip_y, zero, p2_rel);
    *dx = tip_x;
    *dy = tip_y;
}

// ── Trait impls ────────────────────────────────────────────────────────────

impl RenderConvertible for MultiLeader {
    fn to_render(&self, document: &codec::Document) -> Option<RenderEntity> {
        to_render(self, document)
    }
}

impl crate::entities::traits::Grippable for MultiLeader {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};

        let n_vertices: usize = self
            .context
            .leader_roots
            .iter()
            .flat_map(|root| root.lines.iter())
            .map(|line| line.points.len())
            .sum();

        if self.content_type == LeaderContentType::MText {
            let has_elbow = mleader_landing_geom(self).is_some();

            let elbow_id = n_vertices;

            let landing_end_id = elbow_id + 1;

            let landing_id = n_vertices + 2 * usize::from(has_elbow);

            let width_id = landing_id + 1;

            if has_elbow && (grip_id == elbow_id || grip_id == landing_end_id) {
                return vec![GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                }];
            }

            if grip_id == landing_id {
                return vec![GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                }];
            }

            if grip_id == width_id {
                return Vec::new();
            }
        }

        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Add Leader",
                action: GripMenuAction::AddLeader,
            },
            GripMenuItem {
                label: "Remove Leader",
                action: GripMenuAction::RemoveLeader,
            },
        ]
    }
    fn apply_grip_menu(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        // Locate the (root, line) and vertex position owning this grip id.
        let mut idx = 0usize;
        let mut loc: Option<(usize, usize, codec::types::Vector3)> = None;
        'find: for (ri, root) in self.context.leader_roots.iter().enumerate() {
            for (li, line) in root.lines.iter().enumerate() {
                let n = line.points.len();
                if grip_id < idx + n {
                    loc = Some((ri, li, line.points[grip_id - idx]));
                    break 'find;
                }
                idx += n;
            }
        }
        let Some((ri, li, vpos)) = loc else { return };
        match action {
            A::RemoveLeader => {
                let total: usize = self
                    .context
                    .leader_roots
                    .iter()
                    .map(|r| r.lines.len())
                    .sum();
                if total > 1 {
                    self.context.leader_roots[ri].lines.remove(li);
                    if self.context.leader_roots[ri].lines.is_empty()
                        && self.context.leader_roots.len() > 1
                    {
                        self.context.leader_roots.remove(ri);
                    }
                }
            }
            A::AddLeader => {
                // Append to the last root so the new arrow is the last grip id
                // (so the caller can immediately grab it for placement). Seed it
                // below the picked vertex; the user drags it to the final spot.
                let _ = ri;
                let off = self.text_height.max(1.0) * 4.0;
                let arrow = codec::types::Vector3::new(vpos.x, vpos.y - off, vpos.z);
                if let Some(root) = self.context.leader_roots.last_mut() {
                    root.create_line(vec![arrow]);
                }
            }
            _ => {}
        }
    }
}

impl crate::entities::traits::PropertyEditable for MultiLeader {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }
    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl crate::entities::traits::Transformable for MultiLeader {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

pub(crate) fn block_content_insert(
    document: &codec::Document,
    ml: &MultiLeader,
) -> Option<codec::entities::Insert> {
    if ml.content_type != LeaderContentType::Block || !ml.context.has_block_contents {
        return None;
    }
    let record = ml.block_content_handle.and_then(|handle| {
        document
            .block_records
            .iter()
            .find(|record| record.handle == handle)
    })?;
    let mut insertion = ml.context.block_content_location;
    if ml.block_connection_type == BlockContentConnectionType::BlockExtents {
        // Other writers store the block's insertion point in the block content
        // location, already moved off the landing by the extents; derive it
        // again from the content base point so it is not moved twice.
        insertion = ml.context.content_base_point;
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for handle in &record.entity_handles {
            if let Some(entity) = document.get_entity(*handle) {
                let aabb = crate::scene::convert::tess::entity_aabb(entity);
                if aabb != crate::scene::model::wire_model::WireModel::UNBOUNDED_AABB {
                    bounds[0] = bounds[0].min(aabb[0]);
                    bounds[1] = bounds[1].min(aabb[1]);
                    bounds[2] = bounds[2].max(aabb[2]);
                    bounds[3] = bounds[3].max(aabb[3]);
                }
            }
        }
        if bounds[0] <= bounds[2] && bounds[1] <= bounds[3] {
            let to_block = Transform::rotation(-ml.block_rotation);
            let from_left = ml.context.leader_roots.first().map_or(true, |root| {
                to_block.apply_vector([root.direction.x, root.direction.y])[0] >= 0.0
            });
            let anchor_x = if from_left { bounds[0] } else { bounds[2] } as f64;
            let anchor_y = ((bounds[1] + bounds[3]) * 0.5) as f64;
            let local_x = (anchor_x - record.base_point.x) * ml.block_scale.x;
            let local_y = (anchor_y - record.base_point.y) * ml.block_scale.y;
            let rotated = Transform::rotation(ml.block_rotation).apply_vector([local_x, local_y]);
            insertion.x -= rotated[0];
            insertion.y -= rotated[1];
        }
    }
    let mut insert = codec::entities::Insert::new(record.name.clone(), insertion);
    insert.set_x_scale(ml.block_scale.x);
    insert.set_y_scale(ml.block_scale.y);
    insert.set_z_scale(ml.block_scale.z);
    insert.rotation = ml.block_rotation;
    insert.normal = ml.context.block_content_normal;
    insert.common = ml.common.clone();
    insert.common.color = ml.block_content_color.clone();
    insert.common.color_name = None;
    insert.common.color_book_handle = None;
    Some(insert)
}

/// Per-entity tessellation entry for `MultiLeader`. Returns multiple
/// `WireModel`s (leader/dogleg + arrow fill, optional block content,
/// text strokes, frame, background fill) so each piece can carry its
/// own colour.
pub trait MultiLeaderTess {
    fn tessellate(
        &self,
        document: &codec::Document,
        handle: codec::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        active_viewport: Option<codec::Handle>,
        annotation_scale_handle: Option<codec::Handle>,
        block_cache: Option<&crate::scene::cache::block_cache::BlockCache>,
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        bg_color: [f32; 4],
    ) -> Vec<crate::scene::model::wire_model::WireModel>;
}

impl MultiLeaderTess for MultiLeader {
    fn tessellate(
        &self,
        document: &codec::Document,
        handle: codec::Handle,
        selected: bool,
        entity_color: [f32; 4],
        line_weight_px: f32,
        anno_scale: f32,
        active_viewport: Option<codec::Handle>,
        annotation_scale_handle: Option<codec::Handle>,
        block_cache: Option<&crate::scene::cache::block_cache::BlockCache>,
        view_aabb: Option<[f32; 4]>,
        world_per_pixel: Option<f32>,
        bg_color: [f32; 4],
    ) -> Vec<crate::scene::model::wire_model::WireModel> {
        use crate::scene::convert::tessellate::{
            append_arrow, arrow_from_block, color_or_inherit, ArrowKind, DimGeom,
        };
        use crate::scene::model::wire_model::{SnapHint, TangentGeom, WireModel};
        use glam::Vec3;
        let ml = self;
        // line_color falls back to the entity colour when the MultiLeader's own
        // colour is ByBlock/ByLayer; otherwise the leader uses its stored hue.
        let line_color = if selected {
            WireModel::SELECTED
        } else {
            color_or_inherit(&ml.line_color, entity_color)
        };
        // ml.line_weight is the leader-line weight override. Negative codes
        // (ByLayer/ByBlock/Default) fall through to the entity's already-resolved
        // pixel width.
        let leader_lw_px = match ml.line_weight {
            codec::types::LineWeight::Value(v) if v >= 0 => (v as f32 / 100.0) * (96.0 / 25.4),
            _ => line_weight_px,
        };
        // ml.line_type_handle — resolve via line_types table by handle and apply
        // the resulting dash pattern to the leader wire.
        let lt_scale = document.header.linetype_scale as f32 * ml.common.linetype_scale as f32;
        let (leader_pat_len, leader_pat) = match ml.line_type_handle {
            Some(h) if !h.is_null() => {
                let name = document
                    .line_types
                    .iter()
                    .find(|lt| lt.handle == h)
                    .map(|lt| lt.name.clone());
                match name {
                    Some(n) => crate::scene::view::render::resolve_pattern(
                        &document.line_types,
                        &n,
                        lt_scale,
                    ),
                    None => (0.0, [0.0; 8]),
                }
            }
            _ => (0.0, [0.0; 8]),
        };

        let name = handle.value().to_string();
        let nan = [f32::NAN; 3];
        let p3 =
            |v: &codec::types::Vector3| -> [f32; 3] { [(v.x) as f32, (v.y) as f32, (v.z) as f32] };

        // ── Scaling ──────────────────────────────────────────────────────────────
        // Used only when a context omits an already-resolved content size.
        let annotative = crate::scene::annotative::mleader_is_annotative(document, ml);

        // Context sizes are resolved at their stored annotation scale.
        let base_scale = if ml.scale_factor.abs() > 1.0e-12 {
            ml.scale_factor as f32
        } else {
            1.0
        };

        let stored_context_scale = ml.context.scale_factor as f32;

        // An annotative MLEADER ignores its overall scale (greyed out in the
        // properties panel); only the annotation scale resizes the context.
        let context_scale_correction = if annotative && stored_context_scale.abs() > 1.0e-12 {
            let correction = anno_scale / stored_context_scale;

            if correction.is_finite() && correction > 0.0 {
                correction
            } else {
                1.0
            }
        } else {
            1.0
        };

        let fallback_content_scale = if annotative { anno_scale } else { base_scale };

        // The active context stores the resolved world-space arrow size.
        // Reapplying the entity scale here makes context-sized arrows grow twice.
        let arrow_size = ml.context.arrowhead_size as f32 * context_scale_correction;
        let draw_arrow = arrow_size > 0.0;
        let invisible = ml.path_type == MultiLeaderPathType::Invisible;
        // arrowhead_handle resolves through the block records to a named arrow
        // block (matches DIMLDRBLK on Dimension). Null handle / unknown name →
        // ClosedFilled triangle.
        let arrow_kind = match ml.arrowhead_handle {
            Some(h) if !h.is_null() => arrow_from_block(document, h, arrow_size.max(0.001)),
            _ => ArrowKind::Triangle {
                size: arrow_size.max(0.001),
                filled: true,
                size_mul: 1.0,
            },
        };

        // ── Leader / arrow / dogleg points ───────────────────────────────────────
        let mut points: Vec<[f32; 3]> = Vec::new();
        let mut key_verts: Vec<[f64; 3]> = Vec::new();
        let mut snap_pts: Vec<(glam::DVec3, SnapHint)> = Vec::new();
        let mut tangents: Vec<TangentGeom> = Vec::new();
        let mut arrow_fill: Vec<[f32; 3]> = Vec::new();
        let mut first = true;

        // Which side the text grip sits on, recomputed every frame so the text
        // alignment and the landing mirror live when the arrow or text moves.
        let mut text_loc_w = ml.context.text_location;
        // Where the first root's landing (dogleg) ends; an underline starts here.
        let mut landing_end_w: Option<[f32; 2]> = None;

        // Scale the annotation-side offset around the fixed elbow.
        if annotative && (context_scale_correction - 1.0).abs() > 1.0e-6 {
            if let Some(root) = ml.context.leader_roots.first() {
                let anchor = root.connection_point;
                let k = context_scale_correction as f64;

                text_loc_w = codec::types::Vector3::new(
                    anchor.x + (text_loc_w.x - anchor.x) * k,
                    anchor.y + (text_loc_w.y - anchor.y) * k,
                    text_loc_w.z,
                );
            }
        }
        let leader_ref_w = ml
            .context
            .leader_roots
            .first()
            .and_then(|r| r.lines.first())
            .and_then(|l| l.points.last())
            .copied()
            .or_else(|| ml.context.leader_roots.first().map(|r| r.connection_point))
            .unwrap_or(text_loc_w);
        let text_sign_w: f64 = if text_loc_w.x >= leader_ref_w.x {
            1.0
        } else {
            -1.0
        };

        for root in &ml.context.leader_roots {
            let cp = &root.connection_point;
            let cp_f = p3(cp);
            snap_pts.push((Vec3::from(cp_f).as_dvec3(), SnapHint::Node));

            for line in &root.lines {
                if line.points.is_empty() {
                    continue;
                }

                if !invisible {
                    if !first {
                        points.push(nan);
                    }
                    first = false;

                    let mut ctrl: Vec<[f32; 3]> = line.points.iter().map(|p| p3(p)).collect();
                    let last_f = *ctrl.last().unwrap_or(&cp_f);
                    let dist =
                        ((last_f[0] - cp_f[0]).powi(2) + (last_f[1] - cp_f[1]).powi(2)).sqrt();
                    if dist > 1e-9 {
                        ctrl.push(cp_f);
                    }
                    for &c in &ctrl {
                        key_verts.push([c[0] as f64, c[1] as f64, c[2] as f64]);
                        snap_pts.push((Vec3::from(c).as_dvec3(), SnapHint::Node));
                    }

                    // Use the leader LINE's own path type (its stored 170 code),
                    // not the MultiLeader's overall/style default: a spline-style
                    // leader can carry straight lines, and drawing those through a
                    // Catmull-Rom bows an otherwise-straight two-point segment.
                    if line.path_type == MultiLeaderPathType::Spline && ctrl.len() >= 2 {
                        let ctrl_f64: Vec<[f64; 3]> = ctrl
                            .iter()
                            .map(|c| [c[0] as f64, c[1] as f64, c[2] as f64])
                            .collect();
                        for pt in catmull_rom_pts(&ctrl_f64, 8) {
                            points.push([pt[0] as f32, pt[1] as f32, pt[2] as f32]);
                        }
                    } else {
                        for &c in &ctrl {
                            points.push(c);
                        }
                    }
                    for i in 0..ctrl.len().saturating_sub(1) {
                        tangents.push(TangentGeom::Line {
                            p1: ctrl[i],
                            p2: ctrl[i + 1],
                        });
                    }
                }

                if draw_arrow {
                    let tip = &line.points[0];
                    let tip_f = p3(tip);
                    let next = if line.points.len() >= 2 {
                        line.points[1]
                    } else {
                        *cp
                    };
                    let dx = (next.x - tip.x) as f32;
                    let dy = (next.y - tip.y) as f32;
                    let dl = (dx * dx + dy * dy).sqrt().max(1e-9);
                    let dir = Vec3::new(dx / dl, dy / dl, 0.0);
                    let tip_v = Vec3::new(tip_f[0], tip_f[1], tip_f[2]);
                    // Reuse the dim/leader arrow emitter so MultiLeader's arrow
                    // matches the block referenced by arrowhead_handle.
                    let mut arrow_geom = DimGeom::new();
                    append_arrow(&mut arrow_geom, tip_v, dir, &arrow_kind);
                    if !arrow_geom.dim_lines.is_empty() {
                        points.push(nan);
                        points.extend(arrow_geom.dim_lines);
                    }
                    arrow_fill.extend(arrow_geom.arrow_fill);
                }
            }

            // Vertically attached leaders meet the text edge directly.
            let vertical_attach = matches!(
                ml.text_attachment_direction,
                codec::entities::multileader::TextAttachmentDirectionType::Vertical
            );
            if ml.enable_landing
                && ml.enable_dogleg
                && root.landing_distance > 0.0
                && !vertical_attach
            {
                // Horizontal landing (dogleg) from the leader elbow (connection
                // point) toward the text side. The stored geometry places the
                // text so its near-bottom edge sits one landing_gap past this
                // dogleg end, so the dogleg stops here — drawing on to
                // text_location (the block's top-left insertion) would streak a
                // stray line up the side of the text.
                // Landing distance belongs to the selected leader-root context
                // and is already resolved in world units.
                let d = root.landing_distance * context_scale_correction as f64;
                // The dogleg runs along the leader root's stored direction —
                // for a rotated leader that is the angled baseline, not world
                // X. Roots without a usable direction keep the legacy
                // horizontal toward the text side.
                let (ux, uy) = Some((root.direction.x, root.direction.y))
                    .filter(|(x, y)| (x * x + y * y).sqrt() > 1e-9)
                    .map(|(x, y)| {
                        let l = (x * x + y * y).sqrt();
                        (x / l, y / l)
                    })
                    .unwrap_or((text_sign_w, 0.0));
                let landing_end = [(cp.x + ux * d) as f32, (cp.y + uy * d) as f32, cp_f[2]];
                landing_end_w.get_or_insert([landing_end[0], landing_end[1]]);
                points.push(nan);
                points.push(cp_f);
                points.push(landing_end);
                // A root landing on the text's other side than the first
                // root meets the text's far edge: it never runs on.
                let opposite_first = ml.context.leader_roots.first().is_some_and(|first| {
                    first.direction.x * root.direction.x + first.direction.y * root.direction.y
                        < 0.0
                });
                if ml.extend_leader_to_text && !opposite_first {
                    // Run on along the landing up to one gap short of the
                    // text. text_location is the text block's corner, often
                    // above the landing, so only its reach along the landing
                    // counts — aiming at the corner itself drew a stray line
                    // up the side of the text.
                    let gap = ml.context.landing_gap.max(0.0) * context_scale_correction as f64;
                    let reach = (text_loc_w.x - cp.x) * ux + (text_loc_w.y - cp.y) * uy - gap;
                    if reach > d {
                        points.push([
                            (cp.x + ux * reach) as f32,
                            (cp.y + uy * reach) as f32,
                            cp_f[2],
                        ]);
                    }
                }
            }
        }

        // The leader/arrow/dogleg wire goes out as a single WireModel. Text, frame,
        // and background fill (each with their own color) are appended as separate
        // WireModels so the renderer respects per-piece coloring.
        let mut wires: Vec<WireModel> = Vec::new();
        wires.push(WireModel {
            bg_adapt: None,
            point_marker: None,
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            snap_only: false,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: name.clone(),
            points,
            points_low: Vec::new(),
            color: line_color,
            selected,
            aci: 0,
            pattern_length: leader_pat_len,
            pattern: leader_pat,
            line_weight_px: leader_lw_px,
            snap_pts,
            tangent_geoms: tangents,
            key_vertices: key_verts,
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: arrow_fill,
            // fill_tris_low intentionally empty: this fill renders on the
            // top-level path, where consumers (face3d_gpu, xclip) treat a short
            // low half as all-zero, so it draws at f32 precision (sub-metre
            // error at UTM scale) — not a crash. Follow-up: double-single-split
            // via points_to_ds to match emit_wire's paired fill path.
            fill_tris_low: Vec::new(),

            ..Default::default()
        });

        let host = codec::EntityType::MultiLeader(Box::new(ml.clone()));
        for block_use in crate::scene::render_graph::entity_render_block_uses(document, &host, 1.0)
            .into_iter()
            .filter(|block_use| {
                block_use.active
                    && block_use.role == crate::scene::render_graph::BlockRole::MultiLeaderContent
            })
        {
            wires.extend(
                crate::scene::convert::tess::expand_block_object(
                    document,
                    &block_use.insert,
                    handle,
                    selected,
                    active_viewport,
                    bg_color,
                    1.0,
                    annotation_scale_handle,
                    block_cache,
                    view_aabb,
                    world_per_pixel,
                    1.0,
                    crate::scene::convert::tess::BlockObjectOptions {
                        scale_policy: block_use.scale_policy,
                        ..crate::scene::convert::tess::BlockObjectOptions::default()
                    },
                )
                .wires,
            );
        }

        // ── Text strokes / frame / background fill ──────────────────────────────
        // Strip inline format codes, split / word-wrap into lines, then place each
        // line according to text_attachment_point (horizontal) and
        // text_left_attachment (vertical), with text_rotation/text_direction applied.
        if ml.content_type == LeaderContentType::MText && !ml.context.text_string.is_empty() {
            let ctx = &ml.context;
            // `ctx.text_height` (when > 0) is the already-resolved WCS text height
            // stored in the per-instance context — style × scale_factor × anno
            // scale are all already baked in. Multiplying by `effective_scale`
            // again would double-scale (e.g., a context height of 100 in a file
            // with scale_factor=20 was rendering at 2000 units — 20× too big).
            // Only the fallback path (file omits the context value) needs
            // scale_factor + annotation scale applied.
            let height = if ctx.text_height > 0.0 {
                ctx.text_height as f32 * context_scale_correction
            } else {
                ml.text_height as f32 * fallback_content_scale
            };

            let ins = &text_loc_w;
            // Subtract world_offset in f64 before casting to f32: drawings often
            // sit at large absolute coordinates and casting first then subtracting
            // throws away the precision needed for the rotated sub-glyph offsets.
            let local_ins_x = (ins.x) as f32;
            let z = (ins.z) as f32;

            // The context stores the resolved display rotation.
            let td = ctx.text_direction;
            let mut rot = if td.x.abs() > 1e-9 || td.y.abs() > 1e-9 {
                (td.y as f32).atan2(td.x as f32)
            } else {
                ctx.text_rotation as f32
            };
            if ml.text_direction_negative {
                rot += std::f32::consts::PI;
            }

            let vertical_attach = active_vertical_attachment(ml);
            let underline_lift = if underlined_line(vertical_attach, 1).is_some() {
                height * MLEADER_UNDERLINE_GAP
            } else {
                0.0
            };
            // Horizontal leaders attach at the connection point: the dogleg runs
            // horizontally at connection_point.y, and the text's attachment line
            // (bottom/middle/top per text_left_attachment) sits there. Anchor the
            // insertion Y to that connection instead of the stored text_location.y
            // so the text meets the landing exactly — the two can disagree by a
            // few percent because our MText line height need not match the writer's.
            // Only for UNROTATED text: an angled context stores its true anchor in
            // text_location, and snapping Y alone would shear it off its baseline.
            // An underlined line sits its underline gap above that connection.
            let local_ins_y = match ml.text_attachment_direction {
                TextAttachmentDirectionType::Horizontal if rot.abs() < 1e-3 => ml
                    .context
                    .leader_roots
                    .first()
                    .map(|r| r.connection_point.y as f32 + underline_lift)
                    .unwrap_or(ins.y as f32),
                _ => ins.y as f32,
            };

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
            let style = resolve_text_style(&style_name, document);
            let mut rot = rot;
            if style.is_upside_down {
                rot += std::f32::consts::PI;
            }
            let (cos_r, sin_r) = (rot.cos(), rot.sin());

            // The stored text attachment point is the insertion's horizontal
            // anchor within the text block (Left/Center/Right) — honour it
            // instead of guessing from the leader side.
            use codec::entities::multileader::TextAttachmentDirectionType;
            // The context's attachment point exists in every DWG version;
            // the entity-level copy only exists from R2010 on.
            let h_anchor: f32 = match ctx.text_attachment_point {
                codec::entities::multileader::TextAttachmentPointType::Left => 0.0,
                codec::entities::multileader::TextAttachmentPointType::Center => 0.5,
                codec::entities::multileader::TextAttachmentPointType::Right => 1.0,
            };
            // Pick the vertical-anchor attachment based on text_attachment_direction:
            //   Horizontal — leader attaches left/right; use ml.text_left_attachment
            //                (matches the file's stored ctx.text_left_attachment).
            //   Vertical   — leader attaches top/bottom; use ml.text_top_attachment
            //                or ml.text_bottom_attachment depending on which side
            //                the leader is coming from (chosen via the first
            //                root.direction.y sign).
            let v_anchor = mleader_v_anchor(vertical_attach);

            // Shared MText pipeline — every inline format code (`\f`, `\C`/`\c`,
            // `\H`, `\W`, `\Q`, `\T`, `\A`, `\p…`, decorations, stacked
            // fractions, …) reaches the stroke output. Stroke origins are
            // already in offset-relative space because we pass local_ins_x/y.
            let layout = layout_mtext(&MTextRenderOpts {
                // Not an MTEXT: text in a fixed box, never columnar.
                columns: Default::default(),
                value: &ctx.text_string,
                insertion: [local_ins_x as f64, local_ins_y as f64, z as f64],
                height,
                rect_w: ctx.text_width as f32 * context_scale_correction,
                rotation: rot,
                style: &style,
                attach_h_anchor: h_anchor,
                v_anchor,
                line_spacing_factor: ctx.line_spacing_factor as f32,
                exact_line_spacing: matches!(
                    ctx.line_spacing_style,
                    codec::entities::LineSpacingStyle::Exactly
                ),
                rectangle_height: 0.0,
                // Vertical flow (top-to-bottom) is stored per-context.
                vertical_text: matches!(
                    ctx.text_flow_direction,
                    codec::entities::multileader::FlowDirectionType::Vertical
                ),
                want_glyph_boxes: false,
            });
            let line_widths = &layout.line_widths;
            let max_line_w = line_widths.iter().cloned().fold(0.0_f32, f32::max);
            let line_h = layout.line_height;
            let v_offset = layout.v_offset;
            let n_lines = layout.line_count.max(1) as f32;

            // Resolve text color (falls back to entity color for ByLayer / ByBlock).
            let text_color = if selected {
                line_color
            } else {
                color_or_inherit(&ctx.text_color, entity_color)
            };

            {
                // SDF text: emit glyph quads instead of strokes (crisp at every
                // zoom, so no baseline/greek LOD). `layout_mtext` already placed
                // each run at its final position/rotation with a `GlyphRun`, so
                // this mirrors the top-level Text arm (anno = 1, origin as-is).
                // Base colour stays neutral — selection / hover recolouring is
                // done by the text-highlight overlay.
                let neutral = color_or_inherit(&ctx.text_color, entity_color);
                let mut sdf_verts: Vec<crate::scene::pipeline::text_gpu::TextVertex> = Vec::new();
                // Run-less stroke groups are decoration geometry (underline /
                // overline / strike-through, stacked-fraction bars) — they have
                // no glyphs to replace them, so they must be drawn as lines.
                // Web runs that require bidi or joined-script shaping keep their
                // already shaped vector geometry because the per-glyph SDF path has no
                // cluster-position data.
                let mut deco_pts: Vec<[f32; 3]> = Vec::new();
                let mut deco_fill: Vec<[f32; 3]> = Vec::new();
                let mut searchable: Vec<crate::scene::model::wire_model::SearchableTextRun> =
                    Vec::new();
                if let Ok(mut atlas) = crate::scene::text::sdf_atlas::text_atlas().lock() {
                    for ts in &layout.strokes {
                        let is_shaped = ts.run.as_ref().is_some_and(|r| {
                            crate::scene::text::web_font::requires_shaping(&r.text)
                        });
                        if ts.run.is_none() || is_shaped {
                            for stroke in &ts.strokes {
                                if stroke.len() < 2 {
                                    continue;
                                }
                                if !deco_pts.is_empty() {
                                    deco_pts.push([f32::NAN; 3]);
                                }
                                for &[x, y] in stroke {
                                    deco_pts.push([
                                        ts.origin[0] as f32 + x,
                                        ts.origin[1] as f32 + y,
                                        z,
                                    ]);
                                }
                            }
                            for &[x, y] in &ts.fill_tris {
                                deco_fill.push([
                                    ts.origin[0] as f32 + x,
                                    ts.origin[1] as f32 + y,
                                    z,
                                ]);
                            }
                            continue;
                        }
                        let run = ts.run.as_ref().unwrap();
                        let (quads, pen_adv) = crate::scene::text::glyph_quads::layout_glyph_quads(
                            &mut atlas,
                            run.height,
                            run.rotation,
                            run.width_factor,
                            run.oblique,
                            run.tracking,
                            &run.font,
                            run.bold,
                            &run.text,
                        );
                        // Inline `\C` / `\c` colour wins over the entity text
                        // colour, matching the top-level Text arm.
                        let gcolor = ts
                            .color
                            .map(|c| [c[0], c[1], c[2], neutral[3]])
                            .unwrap_or(neutral);
                        crate::scene::pipeline::text_gpu::push_glyph_vertices(
                            &mut sdf_verts,
                            &quads,
                            [ts.origin[0], ts.origin[1], z as f64],
                            1.0,
                            gcolor,
                            0.0,
                        );
                        {
                            use crate::scene::model::wire_model::{
                                clean_searchable_text, SearchableTextRun,
                            };
                            let visible = clean_searchable_text(&run.text);
                            if !visible.is_empty() {
                                searchable.push(SearchableTextRun {
                                    text: visible,
                                    origin: [ts.origin[0], ts.origin[1], z as f64],
                                    height: run.height,
                                    rotation: run.rotation,
                                    color: gcolor,
                                    bold: run.bold,
                                    font: run.font.clone(),
                                    adv_width: pen_adv,
                                });
                            }
                        }
                    }
                }
                if !sdf_verts.is_empty() || !deco_pts.is_empty() || !deco_fill.is_empty() {
                    // Pick box from the glyph quads (f64 accumulate → f32).
                    let (mut nx, mut ny, mut xx, mut xy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                    for v in &sdf_verts {
                        let x = v.pos[0] as f64 + v.pos_low[0] as f64;
                        let y = v.pos[1] as f64 + v.pos_low[1] as f64;
                        nx = nx.min(x);
                        xx = xx.max(x);
                        ny = ny.min(y);
                        xy = xy.max(y);
                    }
                    for p in deco_pts.iter().filter(|p| p[0].is_finite()) {
                        nx = nx.min(p[0] as f64);
                        xx = xx.max(p[0] as f64);
                        ny = ny.min(p[1] as f64);
                        xy = xy.max(p[1] as f64);
                    }
                    for p in deco_fill.iter().filter(|p| p[0].is_finite()) {
                        nx = nx.min(p[0] as f64);
                        xx = xx.max(p[0] as f64);
                        ny = ny.min(p[1] as f64);
                        xy = xy.max(p[1] as f64);
                    }
                    wires.push(WireModel {
                        bg_adapt: None,
                        point_marker: None,
                        taper_widths: Vec::new(),
                        pattern_stations: Vec::new(),
                        world_width: 0.0,
                        depth_override: None,
                        display_visible: true,
                        snap_only: false,
                        region_boundary_click: false,
                        reference_only: false,
                        plot_visible: true,
                        fill_is_3d: false,
                        fill_is_2d_solid: false,
                        render_instance: None,
                        stub: None,
                        pick_tris: Vec::new(),
                        pick_tris_low: Vec::new(),
                        dash_from_start: false,
                        dash_align_end: None,
                        text_verts: sdf_verts,
                        searchable_text: searchable,
                        name: name.clone(),
                        points: deco_pts,
                        points_low: Vec::new(),
                        color: neutral,
                        selected,
                        aci: 0,
                        pattern_length: 0.0,
                        pattern: [0.0; 8],
                        line_weight_px,
                        snap_pts: vec![(
                            glam::DVec3::new(local_ins_x as f64, local_ins_y as f64, z as f64),
                            SnapHint::Node,
                        )],
                        tangent_geoms: vec![],
                        key_vertices: vec![],
                        aabb: [nx as f32, ny as f32, xx as f32, xy as f32],
                        plinegen: true,
                        fill_tris: deco_fill,
                        fill_tris_low: Vec::new(),
                    });
                }
            }

            // Underline attachments: the leader visually continues under the
            // text, so draw the landing on under the underlined line, from the
            // landing's end across the gap to the far end of the widest line,
            // in the leader's colour.
            if let Some(li) = underlined_line(vertical_attach, layout.line_count) {
                let (mut x0, mut x1) = (-max_line_w * h_anchor, max_line_w * (1.0 - h_anchor));
                if let Some([x, y]) = landing_end_w {
                    let lx = (x - local_ins_x) * cos_r + (y - local_ins_y) * sin_r;
                    x0 = x0.min(lx);
                    x1 = x1.max(lx);
                }
                let y = v_offset - li as f32 * line_h - height * MLEADER_UNDERLINE_GAP;
                let pts = vec![
                    [
                        local_ins_x + x0 * cos_r - y * sin_r,
                        local_ins_y + x0 * sin_r + y * cos_r,
                        z,
                    ],
                    [
                        local_ins_x + x1 * cos_r - y * sin_r,
                        local_ins_y + x1 * sin_r + y * cos_r,
                        z,
                    ],
                ];
                wires.push(WireModel {
                    bg_adapt: None,
                    point_marker: None,
                    taper_widths: Vec::new(),
                    pattern_stations: Vec::new(),
                    world_width: 0.0,
                    depth_override: None,
                    display_visible: true,
                    snap_only: false,
                    plot_visible: true,
                    fill_is_3d: false,
                    fill_is_2d_solid: false,
                    render_instance: None,
                    pick_tris: Vec::new(),
                    pick_tris_low: Vec::new(),
                    dash_from_start: false,
                    dash_align_end: None,
                    text_verts: Vec::new(),
                    name: name.clone(),
                    points: pts,
                    points_low: Vec::new(),
                    color: line_color,
                    selected,
                    aci: 0,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px,
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    fill_tris: vec![],
                    fill_tris_low: Vec::new(),

                    ..Default::default()
                });
            }

            // Text frame / background-fill rectangle in local frame, then rotated to WCS.
            if ml.text_frame || ctx.background_fill_enabled {
                // Border offset is a scale factor around the laid-out text box.
                let pad = height * ((ctx.background_scale_factor.max(1.0) as f32 - 1.0) * 0.5);
                // Box the glyphs that were actually laid out (valid for
                // vertical flow too); the metric-derived box is only the
                // no-glyph fallback.
                let lb = layout.local_bounds;
                let (block_left, block_bottom, block_right, block_top) = if lb[0] <= lb[2] {
                    (lb[0] - pad, lb[1] - pad, lb[2] + pad, lb[3] + pad)
                } else {
                    (
                        -max_line_w * h_anchor - pad,
                        v_offset - (n_lines - 1.0) * line_h - pad,
                        max_line_w * (1.0 - h_anchor) + pad,
                        v_offset + height + pad,
                    )
                };
                let local_corners: [[f32; 2]; 4] = [
                    [block_left, block_bottom],
                    [block_right, block_bottom],
                    [block_right, block_top],
                    [block_left, block_top],
                ];
                let wcs_corners: [[f32; 3]; 4] = std::array::from_fn(|i| {
                    let lx = local_corners[i][0];
                    let ly = local_corners[i][1];
                    let wx = local_ins_x + lx * cos_r - ly * sin_r;
                    let wy = local_ins_y + lx * sin_r + ly * cos_r;
                    [wx, wy, z]
                });

                // Background fill — emit two triangles; renders under the text strokes.
                if ctx.background_fill_enabled {
                    let mut fill_color = if selected {
                        line_color
                    } else if ctx.background_mask_fill_on {
                        bg_color
                    } else {
                        color_or_inherit(&ctx.background_fill_color, entity_color)
                    };
                    fill_color[3] *= (1.0
                        - ctx.background_transparency.clamp(0, 90) as f32 / 100.0)
                        .clamp(0.1, 1.0);
                    let fill_tris: Vec<[f32; 3]> = vec![
                        wcs_corners[0],
                        wcs_corners[1],
                        wcs_corners[2],
                        wcs_corners[0],
                        wcs_corners[2],
                        wcs_corners[3],
                    ];
                    wires.push(WireModel {
                        bg_adapt: None,
                        point_marker: None,
                        taper_widths: Vec::new(),
                        pattern_stations: Vec::new(),
                        world_width: 0.0,
                        depth_override: None,
                        display_visible: true,
                        snap_only: false,
                        plot_visible: true,
                        fill_is_3d: false,
                        fill_is_2d_solid: false,
                        render_instance: None,
                        pick_tris: Vec::new(),
                        pick_tris_low: Vec::new(),
                        dash_from_start: false,
                        dash_align_end: None,
                        text_verts: Vec::new(),
                        name: name.clone(),
                        points: vec![],
                        points_low: Vec::new(),
                        color: fill_color,
                        selected,
                        aci: 0,
                        pattern_length: 0.0,
                        pattern: [0.0; 8],
                        line_weight_px: 1.0,
                        snap_pts: vec![],
                        tangent_geoms: vec![],
                        key_vertices: vec![],
                        aabb: WireModel::UNBOUNDED_AABB,
                        plinegen: true,
                        fill_tris,
                        fill_tris_low: Vec::new(),

                        ..Default::default()
                    });
                }

                // Text frame — closed rectangle, matches text color.
                if ml.text_frame {
                    let frame_points: Vec<[f32; 3]> = vec![
                        wcs_corners[0],
                        wcs_corners[1],
                        wcs_corners[2],
                        wcs_corners[3],
                        wcs_corners[0],
                    ];
                    wires.push(WireModel {
                        bg_adapt: None,
                        point_marker: None,
                        taper_widths: Vec::new(),
                        pattern_stations: Vec::new(),
                        world_width: 0.0,
                        depth_override: None,
                        display_visible: true,
                        snap_only: false,
                        plot_visible: true,
                        fill_is_3d: false,
                        fill_is_2d_solid: false,
                        render_instance: None,
                        pick_tris: Vec::new(),
                        pick_tris_low: Vec::new(),
                        dash_from_start: false,
                        dash_align_end: None,
                        text_verts: Vec::new(),
                        name,
                        points: frame_points,
                        points_low: Vec::new(),
                        color: text_color,
                        selected,
                        aci: 0,
                        pattern_length: 0.0,
                        pattern: [0.0; 8],
                        line_weight_px,
                        snap_pts: vec![],
                        tangent_geoms: vec![],
                        key_vertices: vec![],
                        aabb: WireModel::UNBOUNDED_AABB,
                        plinegen: true,
                        fill_tris: vec![],
                        fill_tris_low: Vec::new(),

                        ..Default::default()
                    });
                }
            }
        }

        wires
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::Document;

    #[test]
    fn test_mleader_arabic_text_tessellation() {
        let doc = Document::default();
        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::MText;
        ml.context.text_string = "عربي".to_string();
        ml.context.text_height = 10.0;
        ml.path_type = MultiLeaderPathType::StraightLineSegments;

        let wires = ml.tessellate(
            &doc,
            codec::Handle::default(),
            false,
            [1.0, 1.0, 1.0, 1.0],
            1.0,
            1.0,
            None,
            None,
            None,
            None,
            None,
            [0.0, 0.0, 0.0, 1.0],
        );

        // Find the text wire model:
        // Arabic text requires shaping, so it should emit vector fill_tris
        // instead of unshaped SDF glyph quads (text_verts).
        let text_wire = wires
            .iter()
            .find(|w| !w.fill_tris.is_empty() || !w.text_verts.is_empty())
            .expect("Should produce a text wire for mleader");

        assert!(
            text_wire.text_verts.is_empty(),
            "Arabic text must not be emitted as unshaped SDF glyph quads"
        );
        assert!(
            !text_wire.fill_tris.is_empty(),
            "Arabic text must emit shaped vector fill triangles"
        );
    }

    #[test]
    fn test_background_mask_switch_masks_with_background() {
        // Switching "Background mask" on in Properties must mask with the
        // drawing background, not fill with the inherited text colour and
        // hide the text (#2131).
        let doc = Document::default();
        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::MText;
        ml.context.text_string = "123".to_string();
        ml.context.text_height = 10.0;
        ml.path_type = MultiLeaderPathType::StraightLineSegments;
        apply_geom_prop(&mut ml, "background_fill_enabled", "toggle");
        assert!(ml.context.background_fill_enabled);
        assert!(ml.context.background_mask_fill_on);

        let text_color = [1.0, 1.0, 1.0, 1.0];
        let bg_color = [0.1, 0.2, 0.3, 1.0];
        let wires = ml.tessellate(
            &doc,
            codec::Handle::default(),
            false,
            text_color,
            1.0,
            1.0,
            None,
            None,
            None,
            None,
            None,
            bg_color,
        );
        let mask = wires
            .iter()
            .find(|w| w.fill_tris.len() == 6 && w.points.is_empty() && w.text_verts.is_empty())
            .expect("Should produce a background mask quad");
        assert_eq!(mask.color, bg_color);
    }

    /// Vertical extent of everything a tessellated MLEADER draws.
    fn drawn_height(wires: &[crate::scene::model::wire_model::WireModel]) -> f32 {
        let ys = wires.iter().flat_map(|w| {
            w.points
                .iter()
                .chain(w.fill_tris.iter())
                .map(|p| p[1])
                .chain(w.text_verts.iter().map(|v| v.pos[1]))
                .filter(|y| y.is_finite())
                .collect::<Vec<_>>()
        });
        let (lo, hi) = ys.fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        hi - lo
    }

    #[test]
    fn test_attachment_labels_follow_autocad() {
        // A drawing made in AutoCAD 2025, one MLEADER per option picked by
        // name in its Properties palette, stores these values.
        use codec::entities::multileader::TextAttachmentType as TA;
        let autocad = [
            (TA::BottomLine, "Underline bottom line"), // 5
            (TA::BottomOfTopLineUnderlineBottomLine, "Underline top line"), // 6
            (TA::BottomOfTopLineUnderlineTopLine, "Bottom of top line"), // 7
            (TA::BottomOfTopLineUnderlineAll, "Underline all text"), // 8
        ];
        for (value, name) in autocad {
            assert_eq!(attachment_str(&value), name, "{value:?} shows as");
            assert_eq!(parse_attachment(name), value, "{name:?} stores as");
        }
        for label in ATTACH_LABELS {
            assert_eq!(
                attachment_str(&parse_attachment(label)),
                label,
                "round trip"
            );
        }
    }

    #[test]
    fn test_annotative_mleader_ignores_overall_scale() {
        // AutoCAD greys out the overall scale of an annotative MLEADER and
        // draws it at its context height. A file with overall scale 50 at a
        // 1:50 annotation scale rendered its text 50× too large.
        let doc = Document::default();
        let make = |overall: f64| {
            let mut ml = MultiLeader::default();
            ml.content_type = LeaderContentType::MText;
            ml.context.text_string = "22x2.0".to_string();
            ml.enable_annotation_scale = true;
            ml.scale_factor = overall;
            ml.context.scale_factor = 50.0;
            ml.context.text_height = 250.0;
            ml
        };
        let height = |ml: &MultiLeader| {
            drawn_height(&ml.tessellate(
                &doc,
                codec::Handle::default(),
                false,
                [1.0, 1.0, 1.0, 1.0],
                1.0,
                50.0,
                None,
                None,
                None,
                None,
                None,
                [0.0, 0.0, 0.0, 1.0],
            ))
        };

        let unit = height(&make(1.0));
        let scaled = height(&make(50.0));
        assert!(unit > 0.0, "the text must be drawn");
        assert!(
            (scaled - unit).abs() <= unit * 1.0e-3,
            "overall scale must not resize an annotative MLEADER: {unit} vs {scaled}"
        );
    }

    #[test]
    fn test_added_leader_line_takes_lowest_free_index() {
        // Remove the first leader, then add one: the new line must not repeat
        // the index of the line that is left (it takes the free 0).
        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::MText;
        add_leader_line(&mut ml, codec::types::Vector3::new(0.0, 0.0, 0.0));
        add_leader_line(&mut ml, codec::types::Vector3::new(0.0, 10.0, 0.0));
        ml.context.leader_roots[0].lines.remove(0);
        add_leader_line(&mut ml, codec::types::Vector3::new(0.0, -10.0, 0.0));
        let mut indices: Vec<i32> = ml
            .context
            .leader_roots
            .iter()
            .flat_map(|root| root.lines.iter().map(|line| line.index))
            .collect();
        indices.sort();
        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn test_mleader_right_underline_attachment_meets_landing() {
        // Text left of its leader attaches with its RIGHT attachment
        // ("underline bottom line" here, while the left one is "underline top
        // line"): the underline must lie on the landing and run on to it, and
        // extending the leader to the text must not climb to the text corner.
        use codec::types::Vector3;
        let doc = Document::default();
        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::MText;
        ml.path_type = MultiLeaderPathType::StraightLineSegments;
        ml.enable_landing = true;
        ml.enable_dogleg = true;
        ml.extend_leader_to_text = true;
        let ctx = &mut ml.context;
        ctx.text_string = "MULTILEADER\\PTEST".to_string();
        ctx.text_height = 1.0;
        ctx.landing_gap = 0.5;
        ctx.text_location = Vector3::new(7.5, 5.0, 0.0);
        ctx.text_attachment_point = codec::entities::multileader::TextAttachmentPointType::Right;
        ctx.text_left_attachment = TextAttachmentType::BottomOfTopLineUnderlineBottomLine;
        ctx.text_right_attachment = TextAttachmentType::BottomLine;
        let root = ctx.add_leader_root();
        root.connection_point = Vector3::new(10.0, 0.0, 0.0);
        root.direction = Vector3::new(-1.0, 0.0, 0.0);
        root.landing_distance = 2.0;
        root.create_line(vec![Vector3::new(15.0, -5.0, 0.0), Vector3::new(10.0, 0.0, 0.0)]);

        let wires = ml.tessellate(
            &doc,
            codec::Handle::default(),
            false,
            [1.0, 1.0, 1.0, 1.0],
            1.0,
            1.0,
            None,
            None,
            None,
            None,
            None,
            [0.0, 0.0, 0.0, 1.0],
        );
        let leader_top = wires[0]
            .points
            .iter()
            .filter(|p| p[0].is_finite())
            .fold(f32::MIN, |m, p| m.max(p[1]));
        assert!(leader_top <= 1.0e-4, "leader rises above the landing: {leader_top}");
        let underline = wires
            .iter()
            .find(|w| w.points.len() == 2 && w.text_verts.is_empty())
            .expect("an underline is drawn");
        for p in &underline.points {
            assert!(p[1].abs() < 1.0e-3, "underline off the landing: {p:?}");
        }
        let right = underline.points[0][0].max(underline.points[1][0]);
        assert!((right - 8.0).abs() < 1.0e-3, "underline stops short of the landing: {right}");
    }

    use crate::scene::model::wire_model::WireModel;
    use codec::entities::multileader::TextAttachmentType;

    const H: f64 = 125.0;
    const GAP: f64 = 25.0;
    const DOGLEG: f64 = 18.0;

    /// A text-right MLEADER whose landing runs from (0,0) to (DOGLEG,0).
    fn landed_leader(text: &str, attach: TextAttachmentType) -> MultiLeader {
        use codec::entities::multileader::{LeaderLine, LeaderRoot, TextAttachmentPointType};
        use codec::types::Vector3;

        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::MText;
        ml.path_type = MultiLeaderPathType::StraightLineSegments;
        ml.context.text_string = text.to_string();
        ml.context.text_height = H;
        ml.context.landing_gap = GAP;
        ml.context.text_attachment_point = TextAttachmentPointType::Left;
        ml.context.text_left_attachment = attach;
        ml.context.text_location = Vector3::new(DOGLEG + GAP, 1.2 * H, 0.0);
        let mut line = LeaderLine::default();
        line.points = vec![Vector3::new(-341.0, -387.0, 0.0)];
        let mut root = LeaderRoot::default();
        root.connection_point = Vector3::new(0.0, 0.0, 0.0);
        root.direction = Vector3::new(1.0, 0.0, 0.0);
        root.landing_distance = DOGLEG;
        root.lines = vec![line];
        ml.context.leader_roots = vec![root];
        ml
    }

    fn tess(ml: &MultiLeader) -> Vec<WireModel> {
        ml.tessellate(
            &Document::default(),
            codec::Handle::default(),
            false,
            [1.0, 1.0, 1.0, 1.0],
            1.0,
            1.0,
            None,
            None,
            None,
            None,
            None,
            [0.0, 0.0, 0.0, 1.0],
        )
    }

    fn text_bottom(wires: &[WireModel]) -> f32 {
        wires
            .iter()
            .flat_map(|w| w.text_verts.iter().map(|v| v.pos[1]))
            .fold(f32::MAX, f32::min)
    }

    /// The landing's underline: the only bare two-point rule.
    fn underline(wires: &[WireModel]) -> Option<[[f32; 3]; 2]> {
        wires
            .iter()
            .find(|w| w.points.len() == 2 && w.text_verts.is_empty() && w.fill_tris.is_empty())
            .map(|w| [w.points[0], w.points[1]])
    }

    #[test]
    fn test_underline_attachment_runs_from_landing_end() {
        // Measured on AutoCAD 2025 by exploding an MLEADER with "underline"
        // attachment: the underline is the landing continued under the text,
        // from the dogleg's end to the far end of the text, and the text's
        // baseline sits 0.2 × height above it.
        let (h, gap, dogleg) = (H, GAP, DOGLEG);
        let ml = landed_leader(
            "fsfsdfs",
            TextAttachmentType::BottomOfTopLineUnderlineBottomLine,
        );
        let wires = tess(&ml);

        // Single-line text: "bottom of bottom line" puts the same baseline on
        // the landing without an underline, so the lift is the difference.
        let plain = landed_leader("fsfsdfs", TextAttachmentType::BottomOfBottomLine);
        let lift = text_bottom(&wires) - text_bottom(&tess(&plain));
        assert!(
            (lift - 0.2 * h as f32).abs() < 1.0e-2,
            "underlined text sits 0.2 × height above the landing, got {lift}"
        );

        let [a, b] = underline(&wires).expect("an underline rule");
        assert!(
            (a[0] - dogleg as f32).abs() < 1.0e-3,
            "underline starts at the landing end, got x = {}",
            a[0]
        );
        assert!(
            a[1].abs() < 1.0e-3 && b[1].abs() < 1.0e-3,
            "underline runs at the landing level, got y = {} / {}",
            a[1],
            b[1]
        );
        assert!(b[0] > (dogleg + gap) as f32, "underline spans the text");
    }

    #[test]
    fn test_underline_attachment_values_match_autocad() {
        // AutoCAD 2025, three-line text, one MLEADER per attachment value
        // (see `underlined_line`): 5 underlines the bottom line, 6 and 8 the
        // top line, 7 draws no underline. The underlined line meets the
        // landing, so the text sits above it (5) or hangs below it (6, 8).
        let text = "rand unu\\Prand doi\\Prand trei";
        let cases = [
            (TextAttachmentType::BottomLine, true, true),
            (
                TextAttachmentType::BottomOfTopLineUnderlineBottomLine,
                true,
                false,
            ),
            (
                TextAttachmentType::BottomOfTopLineUnderlineTopLine,
                false,
                false,
            ),
            (TextAttachmentType::BottomOfTopLineUnderlineAll, true, false),
        ];
        for (attach, underlined, text_above) in cases {
            let wires = tess(&landed_leader(text, attach));
            match underline(&wires) {
                Some([a, b]) => {
                    assert!(underlined, "{attach:?} must not draw an underline");
                    assert!(
                        (a[0] - DOGLEG as f32).abs() < 1.0e-3 && a[1].abs() < 1.0e-3,
                        "{attach:?}: underline starts at the landing end, got {a:?}"
                    );
                    assert!(b[1].abs() < 1.0e-3, "{attach:?}: underline stays level");
                }
                None => assert!(!underlined, "{attach:?} must draw an underline"),
            }
            let bottom = text_bottom(&wires);
            if text_above {
                assert!(
                    bottom > 0.0,
                    "{attach:?}: text sits above the landing, got {bottom}"
                );
            } else {
                assert!(
                    bottom < -H as f32,
                    "{attach:?}: text hangs below the landing, got {bottom}"
                );
            }
        }
    }

    /// Values from an MLEADER made in the reference program: block LABEL (circle r4 at its
    /// base point), landing at 60,5 pointing right, gap 0.36. It stores
    /// the block's insertion point 64.36,5 as the block content location.
    #[test]
    fn block_content_insert_uses_the_stored_insertion_point() {
        use codec::types::Vector3;
        let mut doc = Document::default();
        let mut record = codec::tables::BlockRecord::new("LABEL");
        record.handle = doc.allocate_handle();
        let circle = codec::entities::Circle::from_center_radius(Vector3::new(0.0, 0.0, 0.0), 4.0);
        let handle = doc.add_entity(codec::EntityType::Circle(circle)).unwrap();
        record.entity_handles.push(handle);
        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::Block;
        ml.block_connection_type = BlockContentConnectionType::BlockExtents;
        ml.block_content_handle = Some(record.handle);
        doc.block_records.add(record).unwrap();
        ml.context.has_block_contents = true;
        ml.context.content_base_point = Vector3::new(60.36, 5.0, 0.0);
        ml.context.block_content_location = Vector3::new(64.36, 5.0, 0.0);
        let root = ml.context.add_leader_root();
        root.connection_point = Vector3::new(60.0, 5.0, 0.0);
        root.direction = Vector3::new(1.0, 0.0, 0.0);
        let insert = block_content_insert(&doc, &ml).expect("block content");
        assert!(
            (insert.insert_point - Vector3::new(64.36, 5.0, 0.0)).length() < 1e-4,
            "insertion {:?}",
            insert.insert_point
        );
    }

    /// MIRROR about x = 0, values as AutoCAD writes them: text stays readable
    /// and moves to the other side, block content is mirrored in place.
    #[test]
    fn mirror_matches_autocad() {
        use codec::entities::multileader::TextAttachmentPointType;
        use codec::types::Vector3;
        let mut ml = MultiLeader::default();
        ml.context.text_location = Vector3::new(60.0, 81.18, 0.0);
        ml.context.text_direction = Vector3::new(1.0, 0.0, 0.0);
        ml.context.base_direction = Vector3::new(1.0, 0.0, 0.0);
        ml.context.text_attachment_point = TextAttachmentPointType::Left;
        ml.context.text_alignment = TextAlignmentType::Left;
        ml.text_attachment_point = TextAttachmentPointType::Left;
        ml.context.content_base_point = Vector3::new(65.0, 41.0, 0.0);
        ml.context.block_content_location = Vector3::new(60.0, 40.0, 0.0);
        let mirror = EntityTransform::Mirror {
            p1: DVec3::ZERO,
            p2: DVec3::Y,
            working_normal: DVec3::Z,
        };
        apply_transform(&mut ml, &mirror);
        let ctx = &ml.context;
        assert_eq!((ctx.text_location.x, ctx.text_location.y), (-60.0, 81.18));
        assert_eq!(
            ctx.text_direction,
            Vector3::new(1.0, 0.0, 0.0),
            "text stays readable"
        );
        assert_eq!(ctx.base_direction, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(ctx.text_attachment_point, TextAttachmentPointType::Right);
        assert_eq!(ctx.text_alignment, TextAlignmentType::Right);
        assert!(ml
            .property_override_flags
            .contains(MultiLeaderPropertyOverrideFlags::TEXT_ALIGNMENT));
        assert_eq!(
            (ctx.content_base_point.x, ctx.content_base_point.y),
            (-65.0, 41.0)
        );
        assert_eq!(
            (ctx.block_content_location.x, ctx.block_content_location.y),
            (-60.0, 40.0)
        );
        assert_eq!(ctx.block_content_scale.x, -1.0);
        let rotation = ctx.block_rotation;
        assert!(
            rotation.sin().abs() < 1e-9 && rotation.cos() > 0.0,
            "rotation {rotation}"
        );
        // AutoCAD keeps the entity-level overrides.
        assert_eq!(ml.block_scale.x, 1.0);
        assert_eq!(ml.block_rotation, 0.0);
        assert_eq!(ml.text_attachment_point, TextAttachmentPointType::Left);
    }
}

/// Put the text on the right (`to_right`) or left of its landing: the text
/// block attaches by its near edge, and left- or right-justified text turns
/// to read away from the leader. Centred text stays centred.
/// How far above its landing the top of a one-line text sits, for the
/// attachment that governs it: the stored text location is the text's
/// top-left corner, the landing meets the attachment line.
pub(crate) fn text_top_offset(ml: &MultiLeader) -> f64 {
    let height = if ml.context.text_height > 0.0 {
        ml.context.text_height
    } else {
        ml.text_height
    };
    let attach = active_vertical_attachment(ml);
    let lift = if underlined_line(attach, 1).is_some() {
        height * f64::from(MLEADER_UNDERLINE_GAP)
    } else {
        0.0
    };
    lift + match mleader_v_anchor(attach) {
        MTextVAnchor::Top => 0.0,
        MTextVAnchor::Bottom | MTextVAnchor::BottomOfTopLine => height,
        _ => height * 0.5,
    }
}

pub(crate) fn set_mleader_text_side(ml: &mut MultiLeader, to_right: bool) {
    use codec::entities::multileader::TextAttachmentPointType;
    let (attachment, alignment) = if to_right {
        (TextAttachmentPointType::Left, TextAlignmentType::Left)
    } else {
        (TextAttachmentPointType::Right, TextAlignmentType::Right)
    };
    ml.text_attachment_point = attachment;
    ml.context.text_attachment_point = attachment;
    if ml.context.text_alignment != TextAlignmentType::Center {
        ml.context.text_alignment = alignment;
    }
    if ml.text_alignment != TextAlignmentType::Center {
        ml.text_alignment = alignment;
    }
}
