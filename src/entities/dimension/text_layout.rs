// Dimension text placement and layout.

use super::*;

/// DIMTMOVE 1 text the user placed on a linear, aligned or angular
/// dimension, laid out from its stored point.
pub(crate) struct MovedText {
    /// The text's middle.
    pub(crate) center: Vector3,
    /// The leader from the dimension and the hook along the text; none while
    /// the text stays on its dimension line.
    pub(crate) lines: Vec<(Vector3, Vector3)>,
}

/// How far DIMTMOVE 1 text may move off its dimension line before it hangs
/// on a leader: `max(DIMASZ + DIMGAP, DIMTXT / 2 + DIMGAP + DIMEXE)`.
pub(crate) fn moved_text_reach(style: &DimStyle, text_height: f64, dim_scale: f64) -> f64 {
    let gap = style.dimgap.abs() * dim_scale;
    (style.dimasz * dim_scale + gap).max(text_height * 0.5 + gap + style.dimexe * dim_scale)
}

/// How close (as the sine of the angle) the arc's tangent at its middle must
/// be to the text direction for the text to stand like a linear dimension's
/// (measured: lifted at 0.00001 degrees off, not at 0.0001).
pub(crate) const ANGULAR_LEVEL_TOLERANCE: f64 = 1e-6;

/// DIMTMOVE 1 angular text the user placed.
pub(crate) fn angular_text_moved(dim: &Dimension, style: Option<&DimStyle>) -> bool {
    style.is_some_and(|s| s.dimtmove == 1)
        && matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_))
        && dim.base().text_user_positioned
}

/// DIMTMOVE 1 angular text placed off its arc, on a leader.
pub(crate) fn angular_text_on_leader(
    dim: &Dimension,
    style: Option<&DimStyle>,
    document: Option<&Document>,
) -> bool {
    let Some(s) = style.filter(|s| s.dimtmove == 1) else {
        return false;
    };
    if !matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_))
        || !dim.base().text_user_positioned
    {
        return false;
    }
    let (Some(point), Some((vertex, start, end, radius))) =
        (stored_text_point(dim), angular_dimension_frame(dim))
    else {
        return false;
    };
    let scale = if s.dimscale > 1e-9 { s.dimscale } else { 1.0 };
    let height = s.dimtxt * scale;
    let width = dimension_text_width(dim, style, height, document);
    let mid = ((start + end) * 0.5) as f64;
    let direction = dim.base().horizontal_direction;
    !angular_text_meets_arc(
        [vertex.x as f64, vertex.y as f64],
        radius as f64,
        mid,
        [point.x, point.y],
        [direction.cos(), direction.sin()],
        width,
        height,
        s,
        scale,
    )
}

/// Whether DIMTMOVE 1 angular text at `point` still sits on its arc: the
/// text box, with the hook (a gap and an arrow) on the side facing the arc's
/// middle, comes within DIMEXE of the arc's circle. The box is the text's
/// drawn width (its ink in the drawing's font) by its height. Measured
/// against the reference for moves away from and towards the arc's centre
/// (any arc direction, txt/simplex/TrueType, heights 2.5 and 5); for moves
/// along the arc the reference keeps the text on the arc longer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn angular_text_meets_arc(
    vertex: [f64; 2],
    radius: f64,
    mid: f64,
    point: [f64; 2],
    u: [f64; 2],
    width: f64,
    height: f64,
    style: &DimStyle,
    dim_scale: f64,
) -> bool {
    let hook = (style.dimasz + style.dimgap.abs()) * dim_scale;
    let exe = style.dimexe * dim_scale;
    let v = [-u[1], u[0]];
    let to_mid = [
        vertex[0] + radius * mid.cos() - point[0],
        vertex[1] + radius * mid.sin() - point[1],
    ];
    let half = width * 0.5;
    let (u0, u1) = if to_mid[0] * u[0] + to_mid[1] * u[1] <= 0.0 {
        (-half - hook, half)
    } else {
        (-half, half + hook)
    };
    let (v0, v1) = (-height * 0.5, height * 0.5);
    let o = [vertex[0] - point[0], vertex[1] - point[1]];
    let ou = o[0] * u[0] + o[1] * u[1];
    let ov = o[0] * v[0] + o[1] * v[1];
    let near = (ou - ou.clamp(u0, u1)).hypot(ov - ov.clamp(v0, v1));
    let far = [(u0, v0), (u0, v1), (u1, v0), (u1, v1)]
        .iter()
        .map(|(cu, cv)| (ou - cu).hypot(ov - cv))
        .fold(0.0_f64, f64::max);
    near < radius + exe && far > radius - exe
}

/// Where DIMTMOVE 1 puts user-placed text and its leader.
///
/// Within `max(DIMASZ + DIMGAP, DIMTXT / 2 + DIMGAP + DIMEXE)` of the
/// dimension line the text stays on the line: the stored point is its
/// middle, lifted by DIMTAD. Further away a leader runs from the middle of
/// the dimension line (of the arc for an angular dimension) to a hook along
/// the text:
/// * linear and aligned: the stored point is where the text's near edge
///   meets the hook line. The hook starts a gap before it and, with the text
///   above or below it (DIMTAD), runs a gap past the far edge; with centred
///   text (DIMTAD 0) it is two gaps long and ends a gap before the text.
///   Text turned by its own rotation starts at the stored point unlifted.
/// * horizontal text beyond the extension lines of a dimension that is not
///   horizontal: the dimension line runs on to the hook's height and the hook
///   turns along it to a gap past the far edge of the text, which is centred
///   on the stored point but keeps an arrow and a gap from the turn. Text
///   turned by its own rotation stays centred on the stored point, unlifted,
///   and the hook stops a gap before it.
/// * angular: the text leaves its arc once its box, with the hook, clears the
///   arc's circle by DIMEXE (`angular_text_meets_arc`); the stored point is
///   the text's middle and a hook one arrow long ends a gap before it.
pub(crate) fn dimtmove_one_text(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
    document: Option<&Document>,
) -> Option<MovedText> {
    let s = style.filter(|s| s.dimtmove == 1)?;
    let base = dim.base();
    if !base.text_user_positioned {
        return None;
    }
    let point = stored_text_point(dim)?;
    // Measured in the drawing's font only for text the user placed.
    let text_width = dimension_text_width(dim, style, text_height, document);
    let gap = s.dimgap.abs() * dim_scale;
    let arrow = s.dimasz * dim_scale;
    let reach = moved_text_reach(s, text_height, dim_scale);
    let lift = match s.dimtad {
        0 => 0.0,
        tad => {
            let half = dimension_text_extent_height(dim, style, text_height) * 0.5 + gap;
            if tad == 4 {
                -half
            } else {
                half
            }
        }
    };
    let turned = base.text_rotation.abs() > 1e-9;
    // Text turned by its own rotation hangs from a hook along the
    // dimension's horizontal direction, like horizontal text does.
    let angle = if turned {
        base.horizontal_direction
    } else {
        dimension_text_rotation(dim, style)
    };
    let u = [angle.cos(), angle.sin()];
    let v = [-u[1], u[0]];
    let p = [point.x, point.y];
    let at = |o: [f64; 2], d: [f64; 2], k: f64| [o[0] + d[0] * k, o[1] + d[1] * k];
    let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    let v3 = |q: [f64; 2]| Vector3::new(q[0], q[1], point.z);
    let lifted = |q: [f64; 2]| v3(at(q, v, lift));

    if let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) {
        if !matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)) {
            return None;
        }
        let mid = ((start + end) * 0.5) as f64;
        let m = [
            vertex.x as f64 + radius as f64 * mid.cos(),
            vertex.y as f64 + radius as f64 * mid.sin(),
        ];
        // With the arc's tangent at its middle along the text, the text
        // stands like a linear dimension's: on the arc it is lifted by DIMTAD
        // (below for 2 and 4), on a leader it stands on a hook line running
        // from a gap past its near edge to its far edge.
        let tangent = [-mid.sin(), mid.cos()];
        let along = (tangent[0] * u[1] - tangent[1] * u[0]).abs() < ANGULAR_LEVEL_TOLERANCE;
        let level_lift = if along {
            let half = dimension_text_extent_height(dim, style, text_height) * 0.5 + gap;
            match s.dimtad {
                0 => 0.0,
                2 | 4 => -half,
                _ => half,
            }
        } else {
            0.0
        };
        if !angular_text_on_leader(dim, style, document) {
            return Some(MovedText {
                center: v3(at(p, v, level_lift)),
                lines: Vec::new(),
            });
        }
        // Text straight above or below the middle hangs its hook on the right.
        let side = if dot(sub(p, m), u) > 0.0 { 1.0 } else { -1.0 };
        if level_lift != 0.0 {
            let line = at(p, v, -level_lift);
            let hook_start = at(line, u, -side * (text_width * 0.5 + gap));
            let hook_end = at(line, u, side * text_width * 0.5);
            return Some(MovedText {
                center: point,
                lines: vec![(v3(m), v3(hook_start)), (v3(hook_start), v3(hook_end))],
            });
        }
        let hook_end = at(p, u, -side * (text_width * 0.5 + gap));
        let hook_start = at(hook_end, u, -side * arrow);
        return Some(MovedText {
            center: point,
            lines: vec![(v3(m), v3(hook_start)), (v3(hook_start), v3(hook_end))],
        });
    }

    let (first, second, definition, axis) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            [d.rotation.cos(), d.rotation.sin()],
        ),
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = dx.hypot(dy);
            if len < 1e-12 {
                return None;
            }
            (
                d.first_point,
                d.second_point,
                d.definition_point,
                [dx / len, dy / len],
            )
        }
        _ => return None,
    };
    let dline = [definition.x, definition.y];
    let foot = |q: Vector3| at(dline, axis, dot(sub([q.x, q.y], dline), axis));
    let (d1, d2) = (foot(first), foot(second));
    let m = [(d1[0] + d2[0]) * 0.5, (d1[1] + d2[1]) * 0.5];
    let normal = [-axis[1], axis[0]];
    if dot(sub(p, dline), normal).abs() < reach {
        return Some(MovedText {
            center: lifted(p),
            lines: Vec::new(),
        });
    }
    let span = sub(d2, d1);
    let length = dot(span, span).sqrt();
    let along = if length > 1e-12 {
        dot(sub(p, d1), span) / length
    } else {
        0.0
    };
    let crosses = (u[0] * axis[1] - u[1] * axis[0]).abs() > 1e-3;
    if crosses && length > 1e-12 && (along < 0.0 || along > length) {
        let (end, out) = if along < 0.0 {
            (d1, [-span[0] / length, -span[1] / length])
        } else {
            (d2, [span[0] / length, span[1] / length])
        };
        let rise = dot(out, v);
        let travel = dot(sub(p, end), v) / rise;
        if rise.abs() > 1e-9 && travel >= 0.0 {
            let elbow = at(end, out, travel);
            let mut offset = dot(sub(p, elbow), u);
            let side = if offset >= 0.0 { 1.0 } else { -1.0 };
            let clear = arrow + gap;
            if side * offset - text_width * 0.5 < clear {
                offset = side * (clear + text_width * 0.5);
            }
            // Turned text sits on the stored point unlifted and the hook
            // stops a gap before it; level text sits on the hook.
            let (reach_past, center) = if turned {
                (-side * (text_width * 0.5 + gap), v3(at(elbow, u, offset)))
            } else {
                (
                    side * (text_width * 0.5 + gap),
                    lifted(at(elbow, u, offset)),
                )
            };
            let hook_end = at(elbow, u, offset + reach_past);
            return Some(MovedText {
                center,
                lines: vec![(v3(end), v3(elbow)), (v3(elbow), v3(hook_end))],
            });
        }
    }
    let side = if dot(sub(p, m), u) >= 0.0 { 1.0 } else { -1.0 };
    let (hook_start, hook_end) = if s.dimtad == 0 {
        (at(p, u, -side * 3.0 * gap), at(p, u, -side * gap))
    } else {
        (at(p, u, -side * gap), at(p, u, side * (text_width + gap)))
    };
    let center = if turned {
        let r = [base.text_rotation.cos(), base.text_rotation.sin()];
        v3(at(p, r, side * text_width * 0.5))
    } else {
        lifted(at(p, u, side * text_width * 0.5))
    };
    Some(MovedText {
        center,
        lines: vec![(v3(m), v3(hook_start)), (v3(hook_start), v3(hook_end))],
    })
}

/// Endpoints for the DIMTMOVE=1 leader of a jogged radius
/// (linear, aligned and angular text: [`dimtmove_one_text`]).
pub(crate) fn dimtmove_leader_endpoints(
    dim: &Dimension,
    text: DimensionTextLayout,
) -> Option<(Vec3, Vec3, bool)> {
    let lv = |v| vec3_local(v);
    let txt = text.position;
    let anchor = match dim {
        Dimension::LargeRadial(d) => {
            let chord = lv(d.chord_point);
            let jog = lv(d.jog_point);
            let override_center = lv(d.override_center);
            let (near, far) = jogged_radial_break(chord, jog, override_center, d.jog_angle as f32);
            let curves =
                [(chord, near), (near, far), (far, override_center)].map(|(start, end)| {
                    ocs_geom::geom2d::Curve::Line(ocs_geom::geom2d::Line {
                        start: [start.x as f64, start.y as f64],
                        end: [end.x as f64, end.y as f64],
                    })
                });
            let (_, closest) =
                ocs_geom::geom2d::nearest_of(curves.iter(), [txt.x as f64, txt.y as f64])?;
            Vec3::new(closest.point[0] as f32, closest.point[1] as f32, chord.z)
        }
        _ => return None,
    };
    Some((anchor, txt, false))
}

/// Build a rectangle of filled triangles sitting under the dim text, used
/// when DIMTFILL = 2 (explicit fill colour). The rect width is estimated
/// from the formatted text length × character-cell width; an absolutely
/// correct box would need full text metrics from the font cache.
pub(crate) fn text_fill_rect(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
    document: Option<&Document>,
) -> Option<Vec<[f32; 3]>> {
    let value = dimension_text_value(dim, style)?;
    if value.is_empty() {
        return None;
    }
    let pos = dimension_text_pos_f64(dim, style, text_height, dim_scale, document);
    let dimgap = style.map(|s| s.dimgap.abs()).unwrap_or(0.0) * dim_scale;
    // CELL_WIDTH × text_height per character approximates the average glyph
    // aspect of the bundled stick fonts. Inflate by 1 DIMGAP on each side.
    let stack_scale = style.map(dimtfac_or_one).unwrap_or(1.0);
    let approx_w = text_cells(&value, stack_scale) * text_height * CELL_WIDTH + dimgap * 2.0;
    let approx_h = dimension_text_extent_height(dim, style, text_height) + dimgap * 2.0;
    let rot = if dim.base().text_rotation.abs() > 1e-9 {
        dim.base().text_rotation
    } else {
        dimension_text_natural_rotation(dim)
    };
    let (sr, cr) = rot.sin_cos();
    let hx = approx_w * 0.5;
    let hy = approx_h * 0.5;
    let cx = (pos.x) as f32;
    let cy = (pos.y) as f32;
    let cz = (pos.z) as f32;
    let corner = |dx: f64, dy: f64| -> [f32; 3] {
        let lx = dx * cr - dy * sr;
        let ly = dx * sr + dy * cr;
        [cx + lx as f32, cy + ly as f32, cz]
    };
    let p1 = corner(-hx, -hy);
    let p2 = corner(hx, -hy);
    let p3 = corner(hx, hy);
    let p4 = corner(-hx, hy);
    Some(vec![p1, p2, p3, p1, p3, p4])
}

pub(crate) fn arc_length_symbol_points(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
    symbol_position: i16,
    document: Option<&Document>,
) -> Option<Vec<Vec3>> {
    if !matches!(dim, Dimension::Arc(_)) || symbol_position == 2 {
        return None;
    }
    let value = dimension_text_value(dim, style)?;
    if value.is_empty() || text_height <= 1.0e-12 {
        return None;
    }

    let position = dimension_text_pos_f64(dim, style, text_height, dim_scale, document);
    let rotation = dimension_text_rotation(dim, style);
    let (sin_rotation, cos_rotation) = rotation.sin_cos();
    let stack_scale = style.map(dimtfac_or_one).unwrap_or(1.0);
    let text_width = text_cells(&value, stack_scale) * text_height * CELL_WIDTH;
    let symbol_width = text_height * 0.62;
    let symbol_height = text_height * 0.20;
    let (center_x, center_y) = if symbol_position == 1 {
        (0.0, text_height * 0.72)
    } else {
        (
            -(text_width * 0.5 + symbol_width * 0.70),
            text_height * 0.02,
        )
    };
    let transform = |x: f64, y: f64| {
        let local_x = center_x + x;
        let local_y = center_y + y;
        Vec3::new(
            (position.x + local_x * cos_rotation - local_y * sin_rotation) as f32,
            (position.y + local_x * sin_rotation + local_y * cos_rotation) as f32,
            position.z as f32,
        )
    };

    let steps = 8usize;
    Some(
        (0..=steps)
            .map(|index| {
                let t = index as f64 / steps as f64;
                let x = (t - 0.5) * symbol_width;
                let normalized = x / (symbol_width * 0.5);
                let y = symbol_height * (1.0 - normalized * normalized);
                transform(x, y)
            })
            .collect(),
    )
}

pub(crate) struct SuppressFlags {
    pub(crate) ext1: bool,
    pub(crate) ext2: bool,
    pub(crate) dim1: bool,
    pub(crate) dim2: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct TextBreak {
    pub(crate) center: Vec3,
    pub(crate) half_width: f32,
    pub(crate) half_height: f32,
    pub(crate) padding: f32,
    pub(crate) rotation: f64,
    /// `\X` text: the line runs between its parts, unbroken.
    pub(crate) through: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct DimensionTextLayout {
    pub(crate) position: Vec3,
    pub(crate) width: f32,
    pub(crate) break_box: Option<TextBreak>,
    pub(crate) horizontal: bool,
}

pub(crate) fn dimension_text_layout(
    dimension: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
    document: Option<&Document>,
) -> DimensionTextLayout {
    let gap = style
        .map(|style| (style.dimgap.abs() * dim_scale) as f32)
        .unwrap_or(0.09);
    let width = dimension_text_cells(dimension, style)
        .map(|cells| cells as f32 * text_height as f32 * CELL_WIDTH as f32 + gap * 2.0)
        .unwrap_or(0.0);
    let position = vec3_local(dimension_text_pos_f64(
        dimension,
        style,
        text_height,
        dim_scale,
        document,
    ));
    let bare_width = (width - gap * 2.0).max(0.0);
    let break_box = (bare_width > 0.0).then_some(TextBreak {
        center: position,
        half_width: bare_width * 0.5,
        // A limits stack is two lines tall; the break and the arrow fit both
        // have to clear the whole block.
        half_height: dimension_text_extent_height(dimension, style, text_height) as f32 * 0.5,
        padding: gap,
        rotation: dimension_text_rotation(dimension, style),
        through: text_splits_at_line(dimension),
    });
    let outside = dimension_text_is_outside(dimension, style);
    let horizontal =
        style.is_some_and(|style| (outside && style.dimtoh) || (!outside && style.dimtih));
    DimensionTextLayout {
        position,
        width,
        break_box,
        horizontal,
    }
}

/// Cheap heuristic: does this string contain anything the MText parser would
/// interpret? Used by `dimension_text_entity` to pick between a synthetic
/// `Text` (plain DXF special chars only) and a synthetic `MText` (full inline
/// format-code pipeline) for the dim text override.
pub(crate) fn value_has_mtext_codes(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' || c == '}' {
            return true;
        }
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                // Any backslash followed by a known MText escape letter.
                if matches!(
                    next,
                    'H' | 'W'
                        | 'Q'
                        | 'T'
                        | 'A'
                        | 'C'
                        | 'c'
                        | 'f'
                        | 'F'
                        | 'p'
                        | 'L'
                        | 'l'
                        | 'O'
                        | 'o'
                        | 'K'
                        | 'k'
                        | 'S'
                        | 's'
                        | 'P'
                        | 'n'
                        | 'N'
                        | 't'
                        | 'U'
                        | 'u'
                        | 'M'
                        | 'X'
                        | '~'
                        | '{'
                        | '}'
                ) {
                    return true;
                }
            }
        }
    }
    false
}

/// The text style a dimension's text draws with: DIMTXSTY by handle (it
/// survives a rename), then DIMTXSTY by name, then the dimension's own style
/// name.
pub(crate) fn dimension_text_style_name(
    dim: &Dimension,
    style: Option<&DimStyle>,
    document: &Document,
) -> String {
    style
        .and_then(|s| {
            if !s.dimtxsty_handle.is_null() {
                document
                    .text_styles
                    .iter()
                    .find(|ts| ts.handle == s.dimtxsty_handle)
                    .map(|ts| ts.name.clone())
            } else {
                None
            }
        })
        .or_else(|| {
            style
                .map(|s| s.dimtxsty.clone())
                .filter(|n| !n.trim().is_empty())
        })
        .unwrap_or_else(|| dim.base().style_name.clone())
}

/// A dimension's text box as drawn — its middle point, width, height and
/// reading angle — for markers placed beside the text (ANNOMONITOR).
pub(crate) fn text_box(dim: &Dimension, document: &Document) -> Option<(Vector3, f64, f64, f64)> {
    let source = dimension_source_style(document, dim)?;
    let style = resolved_dimension_style(source, dim, document);
    let scale = if style.dimscale > 1e-9 {
        style.dimscale
    } else {
        1.0
    };
    let height = style.dimtxt * scale;
    let width = dimension_text_width(dim, Some(&style), height, Some(document));
    // An automatic text point is not stored: use where the text draws.
    let middle = stored_text_point(dim).or_else(|| automatic_text_position(document, dim))?;
    Some((
        middle,
        width,
        height,
        dimension_text_rotation(dim, Some(&style)),
    ))
}

/// The width of a dimension's text as it draws: the extent of its ink in
/// the drawing's text style font. Without the drawing (or for formatted
/// multi-line text) the cell estimate stands in.
pub(crate) fn dimension_text_width(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    document: Option<&Document>,
) -> f64 {
    let estimate = || {
        dimension_text_cells(dim, style)
            .map(|cells| cells * text_height * CELL_WIDTH)
            .unwrap_or(0.0)
    };
    let Some(document) = document else {
        return estimate();
    };
    let Some(value) = dimension_text_value(dim, style) else {
        return 0.0;
    };
    if value_has_mtext_codes(&value) {
        return estimate();
    }
    let text = crate::entities::text_support::resolve_dxf_special_chars(&value);
    let font = crate::entities::text_support::resolve_text_style(
        &dimension_text_style_name(dim, style, document),
        document,
    );
    crate::entities::text_support::text_local_bounds(
        &font.font_name,
        &text,
        text_height as f32,
        font.width_factor,
        font.oblique_angle,
    )
    .map_or_else(estimate, |bounds| {
        (bounds.ink_max[0] - bounds.ink_min[0]).max(0.0) as f64
    })
}

pub(crate) fn dimension_text_entity(
    dim: &Dimension,
    text_height: f64,
    style: Option<&DimStyle>,
    document: &Document,
    dim_scale: f64,
) -> Option<EntityType> {
    // Deviations are emitted by `dimension_tolerance_entity` at their own
    // height and alignment; keep the primary entity free of duplicate text.
    let (value, _) = dimension_text_parts(dim, style)?;
    // Use f64 position directly to avoid f32 round-trip precision loss at large
    // coordinates (e.g. Turkish UTM ~4,000,000 m). tessellate() will apply
    // world_offset when rendering this synthetic entity.
    let pos_f64 = dimension_text_pos_f64(dim, style, text_height, dim_scale, Some(document));
    // A limits stack draws each half at DIMTFAC × DIMTXT, like the source application.
    let text_height = match style {
        Some(s) if s.dimlim => text_height * dimtfac_or_one(s),
        _ => text_height,
    };
    let base = dim.base();

    let rotation = dimension_text_rotation(dim, style);

    let style_name = dimension_text_style_name(dim, style, document);

    // Route through MText whenever the value carries inline format codes
    // (`\f`, `\C`, `\H`, `\S`, brace scopes, …). Otherwise stay on the Text
    // path — single-line dim text doesn't need the full MText pipeline.
    if value_has_mtext_codes(&value) {
        use document::entities::dimension::AttachmentPointType as DA;
        use document::entities::AttachmentPoint as MA;
        let attachment_point = match base.attachment_point {
            DA::TopLeft => MA::TopLeft,
            DA::TopCenter => MA::TopCenter,
            DA::TopRight => MA::TopRight,
            DA::MiddleLeft => MA::MiddleLeft,
            DA::MiddleCenter => MA::MiddleCenter,
            DA::MiddleRight => MA::MiddleRight,
            DA::BottomLeft => MA::BottomLeft,
            DA::BottomCenter => MA::BottomCenter,
            DA::BottomRight => MA::BottomRight,
        };
        let mut mtext = MText::with_value(value, pos_f64);
        mtext.height = text_height;
        mtext.rotation = rotation;
        mtext.style = style_name;
        mtext.attachment_point = attachment_point;
        if base.line_spacing_factor.abs() > 1e-9 {
            mtext.line_spacing_factor = base.line_spacing_factor;
        }
        mtext.normal = base.normal;
        mtext.common = synthesized_text_common(base);
        return Some(EntityType::MText(Box::new(mtext)));
    }

    let value = if style.is_some_and(|style| style.dimtxtdirection) {
        value.chars().rev().collect()
    } else {
        value
    };
    let mut text = Text::with_value(value, pos_f64)
        .with_height(text_height)
        .with_rotation(rotation);
    text.width_factor = style_width_factor(&style_name, document);
    text.style = style_name;

    // Map AttachmentPointType (1..9 grid) to Text horizontal + vertical
    // alignments. 1=TopLeft … 9=BottomRight (column-major).
    let (ha, va) = attachment_to_text_align(base.attachment_point);
    text.horizontal_alignment = ha;
    text.vertical_alignment = va;
    // line_spacing_factor controls multi-line text spacing in MText. Our
    // synthetic Text is single-line so this is a no-op, but pass through
    // for completeness.
    let _ = base.line_spacing_factor;
    // normal would rotate the dim plane out of XY. The local 2D pipeline
    // assumes XY, so non-XY normals are read but not applied here.
    let _ = base.normal;

    text.common = synthesized_text_common(base);
    Some(EntityType::Text(text))
}

/// Width factor for a synthesized dimension TEXT. Dimension text is drawn at
/// its text style's width factor; a TEXT's own factor (1.0 when synthesized)
/// would otherwise win over the style's.
pub(crate) fn style_width_factor(style_name: &str, document: &Document) -> f64 {
    f64::from(crate::entities::text_support::resolve_text_style(style_name, document).width_factor)
}

/// The dimension's properties for its synthesized text, less its extension
/// dictionary and annotative markers. The text height already carries the
/// annotation scale; with the dimension's per-scale contexts or flag the text
/// would resolve as annotative itself and scale a second time at every scale
/// but the default one. (#1939)
pub(crate) fn synthesized_text_common(base: &DimensionBase) -> document::entities::EntityCommon {
    let mut common = base.common.clone();
    common.xdictionary_handle = None;
    for marker in ["AcadAnnotative", "AcAnnoPO", "AcAnnotativeData"] {
        common.extended_data.remove_record(marker);
    }
    common
}

pub(crate) fn attachment_to_text_align(
    attach: document::entities::dimension::AttachmentPointType,
) -> (
    document::entities::text::TextHorizontalAlignment,
    document::entities::text::TextVerticalAlignment,
) {
    use document::entities::dimension::AttachmentPointType as A;
    use document::entities::text::{TextHorizontalAlignment as H, TextVerticalAlignment as V};
    match attach {
        A::TopLeft => (H::Left, V::Top),
        A::TopCenter => (H::Center, V::Top),
        A::TopRight => (H::Right, V::Top),
        A::MiddleLeft => (H::Left, V::Middle),
        A::MiddleCenter => (H::Center, V::Middle),
        A::MiddleRight => (H::Right, V::Middle),
        A::BottomLeft => (H::Left, V::Bottom),
        A::BottomCenter => (H::Center, V::Bottom),
        A::BottomRight => (H::Right, V::Bottom),
    }
}

/// Reading rotation for a dimension's measurement text, resolving the style
/// flags the same way the live renderer does: explicit text_rotation, then
/// DIMTIH/DIMTOH force-horizontal (along horizontal_direction), else the
/// natural dim-line angle (+90° for DIMJUST 3/4). Shared by the live text entity and
/// the bake so a reloaded dimension's text reads at the same angle. (#181)
pub(crate) fn dimension_text_rotation(dim: &Dimension, style: Option<&DimStyle>) -> f64 {
    let base = dim.base();
    let dimtih = style.map(|s| s.dimtih).unwrap_or(false);
    let dimtoh = style.map(|s| s.dimtoh).unwrap_or(false);
    let dimjust = style.map(|s| s.dimjust).unwrap_or(0);
    let outside = dimension_text_is_outside(dim, style);
    if base.text_rotation.abs() > 1e-9 {
        base.text_rotation
    } else if matches!(dim, Dimension::Ordinate(_)) {
        dimension_text_natural_rotation(dim)
    } else if angular_text_moved(dim, style) {
        // DIMTMOVE 1 angular text a user placed reads horizontally, on its
        // arc as on a leader (measured).
        base.horizontal_direction
    } else if (outside && dimtoh) || (!outside && dimtih) {
        // Text forced horizontal reads along the dimension's horizontal
        // direction; text aligned with the dimension line ignores it.
        base.horizontal_direction
    } else {
        let mut r = dimension_text_natural_rotation(dim);
        if dimjust == 3 || dimjust == 4 {
            r += std::f64::consts::FRAC_PI_2;
        }
        r
    }
}

pub(crate) fn dimension_text_is_outside(dim: &Dimension, style: Option<&DimStyle>) -> bool {
    let Some(style) = style else {
        return false;
    };
    if let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) {
        if let Some(point) = stored_text_point(dim) {
            let text = vec3_local(point);
            let angle = (text.y - vertex.y).atan2(text.x - vertex.x);
            return (angle - start).rem_euclid(std::f32::consts::TAU) > end - start + 1.0e-6;
        }
        if style.dimtix {
            return false;
        }
        let scale = if style.dimscale > 1e-9 {
            style.dimscale
        } else {
            1.0
        };
        let height = style.dimtxt * scale;
        let gap = style.dimgap.abs() * scale;
        let text_width = dimension_text_cells(dim, Some(style))
            .map(|cells| cells * height * CELL_WIDTH + gap * 2.0)
            .unwrap_or(0.0);
        let arrow = style.dimasz * scale;
        let span = radius as f64 * (end - start).abs() as f64;
        let insufficient = text_width + arrow * 2.0 > span;
        return insufficient
            && match style.dimatfit {
                0 | 2 => true,
                1 | 3 => text_width > span,
                _ => text_width > span,
            };
    }
    if let Dimension::LargeRadial(radial) = dim {
        let radius = radial.definition_point.distance(&radial.chord_point);
        if let Some(point) = stored_text_point(dim) {
            return radial.definition_point.distance(&point) > radius + 1.0e-9;
        }
        if style.dimtix {
            return false;
        }
        let available = radial.override_center.distance(&radial.chord_point);
        let scale = if style.dimscale > 1.0e-9 {
            style.dimscale
        } else {
            1.0
        };
        let height = style.dimtxt * scale;
        let gap = style.dimgap.abs() * scale;
        let text_width = dimension_text_cells(dim, Some(style))
            .map(|cells| cells * height * CELL_WIDTH + gap * 2.0)
            .unwrap_or(0.0);
        let arrow = style.dimasz * scale;
        let insufficient = text_width + arrow > available;
        return insufficient
            && match style.dimatfit {
                0 | 2 => true,
                1 | 3 => text_width > available,
                _ => text_width > available,
            };
    }
    if let Dimension::Radius(radius) = dim {
        let dx = radius.definition_point.x - radius.angle_vertex.x;
        let dy = radius.definition_point.y - radius.angle_vertex.y;
        let available = dx.hypot(dy);
        if let Some(text) = stored_text_point(dim) {
            return (text.x - radius.angle_vertex.x).hypot(text.y - radius.angle_vertex.y)
                > available + 1e-9;
        }
        if style.dimtix {
            return false;
        }
        let scale = if style.dimscale > 1e-9 {
            style.dimscale
        } else {
            1.0
        };
        let height = style.dimtxt * scale;
        let gap = style.dimgap.abs() * scale;
        let text_width = dimension_text_cells(dim, Some(style))
            .map(|cells| cells * height * CELL_WIDTH + gap * 2.0)
            .unwrap_or(0.0);
        let arrow = style.dimasz * scale;
        let insufficient = text_width + arrow > available;
        return insufficient
            && match style.dimatfit {
                0 | 2 => true,
                1 | 3 => text_width > available,
                _ => text_width > available,
            };
    }
    let (first, second, axis) = match dim {
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            Vector3::new(d.rotation.cos(), d.rotation.sin(), 0.0),
        ),
        Dimension::Aligned(d) => {
            let delta = d.second_point - d.first_point;
            let length = (delta.x * delta.x + delta.y * delta.y).sqrt().max(1e-12);
            (d.first_point, d.second_point, delta / length)
        }
        Dimension::Diameter(d) => {
            let delta = d.definition_point - d.angle_vertex;
            let length = (delta.x * delta.x + delta.y * delta.y).sqrt().max(1e-12);
            (d.angle_vertex, d.definition_point, delta / length)
        }
        _ => return false,
    };
    let first_axis = first.x * axis.x + first.y * axis.y;
    let second_axis = second.x * axis.x + second.y * axis.y;
    let lo = first_axis.min(second_axis);
    let hi = first_axis.max(second_axis);
    if let Some(point) = stored_text_point(dim) {
        let position = point.x * axis.x + point.y * axis.y;
        return position < lo || position > hi;
    }
    if style.dimtix {
        return false;
    }
    let scale = if style.dimscale > 1e-9 {
        style.dimscale
    } else {
        1.0
    };
    let height = style.dimtxt * scale;
    let gap = style.dimgap.abs() * scale;
    let text_width = dimension_text_cells(dim, Some(style))
        .map(|cells| cells * height * CELL_WIDTH + gap * 2.0)
        .unwrap_or(0.0);
    let arrow = style.dimasz * scale;
    let span = hi - lo;
    let insufficient = text_width + arrow * 2.0 > span;
    insufficient
        && match style.dimatfit {
            0 | 2 => true,
            1 | 3 => text_width > span,
            _ => text_width > span,
        }
}

/// Where a dynamic dimension's lock mark sits: just after the text on its
/// baseline, with the direction that points away from the text.
pub(crate) fn dynamic_dimension_lock_anchor(
    document: &Document,
    dim: &Dimension,
    anno_scale: f64,
) -> Option<(Vector3, Vector3)> {
    let source_style = dimension_source_style(document, dim);
    let effective_style = source_style.map(|style| resolved_dimension_style(style, dim, document));
    let style = effective_style.as_ref();
    let dim_scale = style
        .map(|s| {
            if s.dimscale > 1e-6 {
                s.dimscale
            } else {
                anno_scale
            }
        })
        .unwrap_or(1.0);
    let text_height = style.map(|s| s.dimtxt * dim_scale).unwrap_or(2.5);
    let value = dimension_text_value(dim, style)?;
    let stack_scale = style.map(dimtfac_or_one).unwrap_or(1.0);
    let half_width = text_cells(&value, stack_scale) * text_height * CELL_WIDTH * 0.5;
    let pos = dimension_text_pos_f64(dim, style, text_height, dim_scale, Some(document));
    // The text angle the renderer draws (DIMTIH/DIMTOH overrides make a
    // dynamic dimension's text horizontal).
    let (sr, cr) = dimension_text_rotation(dim, style).sin_cos();
    let outward = Vector3::new(cr, sr, 0.0);
    // A glyph-sized gap keeps the lock clear of the last digit.
    let reach = half_width + text_height * 0.9;
    Some((
        Vector3::new(pos.x + outward.x * reach, pos.y + outward.y * reach, pos.z),
        outward,
    ))
}

/// The two ends of an angular dimension's arc, each with the direction
/// pointing away from the arc: where a dynamic angle's triangle grips sit.
pub(crate) fn angular_arc_ends(dim: &Dimension) -> Option<[(glam::DVec3, glam::DVec3); 2]> {
    let (vertex, start, end, radius) = angular_dimension_frame(dim)?;
    let point = |angle: f32| {
        let p = vertex + Vec3::new(angle.cos(), angle.sin(), 0.0) * radius;
        glam::DVec3::new(p.x as f64, p.y as f64, p.z as f64)
    };
    let away = |angle: f32, sign: f32| {
        glam::DVec3::new(
            (-angle.sin() * sign) as f64,
            (angle.cos() * sign) as f64,
            0.0,
        )
    };
    Some([
        (point(start), away(start, -1.0)),
        (point(end), away(end, 1.0)),
    ])
}

pub(crate) fn dimension_text_natural_rotation(dim: &Dimension) -> f64 {
    let angle =
        match dim {
            Dimension::Linear(d) => d.rotation,
            Dimension::Aligned(d) => {
                let dx = d.second_point.x - d.first_point.x;
                let dy = d.second_point.y - d.first_point.y;
                dy.atan2(dx)
            }
            Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) | Dimension::Arc(_) => {
                angular_dimension_frame(dim)
                    .map(|(_, start, end, _)| {
                        ((start + end) * 0.5 + std::f32::consts::FRAC_PI_2) as f64
                    })
                    .unwrap_or(0.0)
            }
            Dimension::Radius(d) => (d.definition_point.y - d.angle_vertex.y)
                .atan2(d.definition_point.x - d.angle_vertex.x),
            Dimension::Diameter(d) => (d.definition_point.y - d.angle_vertex.y)
                .atan2(d.definition_point.x - d.angle_vertex.x),
            Dimension::LargeRadial(d) => {
                (d.chord_point.y - d.override_center.y).atan2(d.chord_point.x - d.override_center.x)
            }
            Dimension::Ordinate(d) => {
                let axis_rotation = -d.base.horizontal_direction;
                if d.is_ordinate_type_x {
                    axis_rotation + std::f64::consts::FRAC_PI_2
                } else {
                    axis_rotation
                }
            }
        };
    // Clamp to (-π/2, π/2] so text never appears upside-down.
    let pi = std::f64::consts::PI;
    if angle > pi / 2.0 {
        angle - pi
    } else if angle <= -pi / 2.0 {
        angle + pi
    } else {
        angle
    }
}

/// Where a dimension's text sits by its style in this drawing (its
/// overrides applied) when the user has not placed it. Text beside an
/// extension line (DIMJUST 1/2) is measured by the glyphs of its font, not
/// by character cells.
pub(crate) fn automatic_text_position(document: &Document, dim: &Dimension) -> Option<Vector3> {
    let source = dimension_source_style(document, dim)?;
    let style = resolved_dimension_style(source, dim, document);
    let scale = if style.dimscale > 1e-6 {
        style.dimscale
    } else {
        1.0
    };
    let height = style.dimtxt * scale;
    let mut position = dimension_text_pos_f64(dim, Some(&style), height, scale, Some(document));
    // Text beside an extension line, or a radius's text, which sits out past
    // its arc point two arrows and a gap from it.
    let outside = dimension_text_is_outside(dim, Some(&style));
    let measured = match dim {
        Dimension::Radius(_) => true,
        _ => matches!(style.dimjust, 1 | 2) && !outside,
    };
    if !measured {
        return Some(position);
    }
    let Some(ink) = dimension_text_ink_width(document, dim, &style, height) else {
        return Some(position);
    };
    // The layout spaced the text by character cells; it moves by half the
    // difference to its inked width, away from the line it sits beside
    // (or from the arc).
    let cells = dimension_text_cells(dim, Some(&style)).unwrap_or(0.0) * height * CELL_WIDTH;
    let half = (ink - cells) * 0.5;
    let axis = |first: Vector3, second: Vector3, defpt: Vector3, ax: f64, ay: f64| {
        let t1 = (first.x - defpt.x) * ax + (first.y - defpt.y) * ay;
        let t2 = (second.x - defpt.x) * ax + (second.y - defpt.y) * ay;
        let towards_second = if t2 >= t1 { 1.0 } else { -1.0 };
        let inward = if style.dimjust == 1 {
            towards_second
        } else {
            -towards_second
        };
        (ax * half * inward, ay * half * inward)
    };
    match dim {
        Dimension::Linear(d) => {
            let (dx, dy) = axis(
                d.first_point,
                d.second_point,
                d.definition_point,
                d.rotation.cos(),
                d.rotation.sin(),
            );
            position.x += dx;
            position.y += dy;
        }
        Dimension::Aligned(d) => {
            let (ex, ey) = (
                d.second_point.x - d.first_point.x,
                d.second_point.y - d.first_point.y,
            );
            let len = ex.hypot(ey).max(1e-12);
            let (dx, dy) = axis(
                d.first_point,
                d.second_point,
                d.definition_point,
                ex / len,
                ey / len,
            );
            position.x += dx;
            position.y += dy;
        }
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => {
            if let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) {
                let toward_end = if end >= start { 1.0 } else { -1.0 };
                let sign = if style.dimjust == 1 {
                    -toward_end
                } else {
                    toward_end
                };
                let (sin, cos) = (half * sign / f64::from(radius).max(1e-12)).sin_cos();
                let (cx, cy) = (f64::from(vertex.x), f64::from(vertex.y));
                let (px, py) = (position.x - cx, position.y - cy);
                position.x = cx + px * cos - py * sin;
                position.y = cy + px * sin + py * cos;
            }
        }
        Dimension::Radius(d) => {
            let (dx, dy) = (
                d.definition_point.x - d.angle_vertex.x,
                d.definition_point.y - d.angle_vertex.y,
            );
            let len = dx.hypot(dy).max(1e-12);
            let reach = (2.0 * style.dimasz + style.dimgap.abs()) * scale + ink * 0.5;
            position.x = d.definition_point.x + dx / len * reach;
            position.y = d.definition_point.y + dy / len * reach;
        }
        _ => {}
    }
    Some(position)
}

/// The inked width of a dimension's text: its characters (formatting codes
/// left out) in its font (an `\F` font in the text, else the style's text
/// style) at `height`.
pub(crate) fn dimension_text_ink_width(
    document: &Document,
    dim: &Dimension,
    style: &DimStyle,
    height: f64,
) -> Option<f64> {
    let value = dimension_text_value(dim, Some(style))?;
    let mut plain = String::new();
    let mut font = None;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('F' | 'f') => {
                    let body: String = chars.by_ref().take_while(|c| *c != ';').collect();
                    font.get_or_insert_with(|| body.split('|').next().unwrap_or("").to_string());
                }
                Some(
                    'C' | 'c' | 'H' | 'h' | 'W' | 'w' | 'Q' | 'q' | 'T' | 't' | 'A' | 'a' | 'p',
                ) => {
                    chars.by_ref().find(|c| *c == ';');
                }
                Some('P') => plain.push(' '),
                Some('~') => plain.push(' '),
                Some(other @ ('\\' | '{' | '}')) => plain.push(other),
                _ => {}
            },
            '~' => plain.push(' '),
            other => plain.push(other),
        }
    }
    let font = font.filter(|f| !f.is_empty()).unwrap_or_else(|| {
        crate::entities::text_support::resolve_text_style(&style.dimtxsty, document).font_name
    });
    let bounds = crate::entities::text_support::text_local_bounds(
        &font,
        plain.trim(),
        height as f32,
        1.0,
        0.0,
    )?;
    Some(f64::from(bounds.ink_max[0] - bounds.ink_min[0]))
}

/// Clear an automatic text position so the dimension is laid out from its
/// current settings again. A loaded point is kept so a drawing looks as it
/// was drawn; a new dimension, or one whose style or overrides just changed,
/// has no valid one. Text the user placed keeps its point (#1412).
pub(crate) fn reset_automatic_text_position(base: &mut DimensionBase) {
    if !base.text_user_positioned {
        base.text_middle_point = Vector3::new(0.0, 0.0, 0.0);
    }
}

/// The point on the circle a radial leader leaves from: the arc point of a
/// radius, or whichever end of a diameter's chord is nearer the text.
pub(crate) fn radial_leader_tip(dim: &Dimension, text: Vector3) -> Vector3 {
    match dim {
        Dimension::Radius(d) => d.definition_point,
        Dimension::Diameter(d) => {
            if d.angle_vertex.distance(&text) <= d.definition_point.distance(&text) {
                d.angle_vertex
            } else {
                d.definition_point
            }
        }
        _ => dim.base().text_middle_point,
    }
}

/// A dimensional constraint's dynamic dimension: the reference keeps it on
/// its own hidden layer and draws it by its own rules.
pub(crate) fn dynamic_constraint_dimension(dim: &Dimension) -> bool {
    crate::scene::parametric_constraints::is_dynamic_dimension_layer(&dim.base().common.layer)
}

/// How far a radial leader runs out from the rim before its hook, and how
/// long that hook is: about one text height, so the text always clears the
/// circle.
pub(crate) fn radial_leader_run(text_height: f64) -> f64 {
    text_height
}

/// Where a radius or diameter dimension's text sits once it no longer fits
/// inside: beside the circle, level with the end of the leader's run, its
/// near edge one arrow away so the hook has room.
pub(crate) fn radial_outside_text(
    tip: Vector3,
    ux: f64,
    uy: f64,
    text_height: f64,
    text_width: f64,
) -> Vector3 {
    let run = radial_leader_run(text_height);
    let elbow = Vector3::new(tip.x + ux * run, tip.y + uy * run, tip.z);
    let side = if ux >= 0.0 { 1.0 } else { -1.0 };
    Vector3::new(elbow.x + side * (run + text_width * 0.5), elbow.y, elbow.z)
}

/// Whether `point` lies on the diameter's leader line — the line through its
/// two defining points — rather than a DIMTAD offset away from it.
pub(crate) fn diameter_point_on_leader(
    d: &document::entities::dimension::DimensionDiameter,
    point: Vector3,
    offset: f64,
) -> bool {
    point_on_line(d.angle_vertex, d.definition_point, point, offset)
}

/// Whether `point` lies on the line through `a` and `b` rather than an
/// `offset` (a DIMTAD lift) away from it.
/// A radius with horizontal text placed by a point on its line through the
/// centre, as DIMRADIUS stores the click: the point is where the leader's
/// hook starts, so the leader runs straight from the arc point along that
/// line and the text sits beside the hook (#3149). A file saved elsewhere
/// stores the text's own middle, which stands off that line; a leader within
/// 15° of horizontal runs straight to the text either way.
pub(crate) fn radius_hook_point(
    dim: &Dimension,
    style: Option<&DimStyle>,
    point: Vector3,
    text_height: f64,
) -> bool {
    let Dimension::Radius(d) = dim else {
        return false;
    };
    let (dx, dy) = (
        point.x - d.definition_point.x,
        point.y - d.definition_point.y,
    );
    dimension_text_reads_horizontal(dim, style)
        && d.angle_vertex.distance(&point) > d.angle_vertex.distance(&d.definition_point)
        && dy.abs().atan2(dx.abs()) > 15.0_f64.to_radians()
        && point_on_line(d.angle_vertex, d.definition_point, point, text_height * 0.1)
}

pub(crate) fn point_on_line(a: Vector3, b: Vector3, point: Vector3, offset: f64) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx.hypot(dy);
    if length < 1e-12 {
        return true;
    }
    let across = ((point.x - a.x) * dy - (point.y - a.y) * dx) / length;
    across.abs() < offset.abs() * 0.5
}

/// `point` on the line through `a` and `b`, lifted `offset` to the side the
/// line's readable text stands on (below for DIMTAD 4).
pub(crate) fn lift_off_line(
    a: Vector3,
    b: Vector3,
    point: Vector3,
    offset: f64,
    dimtad: i16,
) -> Vector3 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt().max(1e-12);
    // Match the readable orientation used by text_on_dim_line().
    let (mut nx, mut ny) = (dx / len, dy / len);
    if nx < 0.0 || (nx == 0.0 && ny < 0.0) {
        nx = -nx;
        ny = -ny;
    }
    let (px, py) = (-ny, nx);
    let sign = if dimtad == 4 { -1.0 } else { 1.0 };
    Vector3::new(
        point.x + px * offset * sign,
        point.y + py * offset * sign,
        point.z,
    )
}

/// DIMTOH / DIMTIH turn this dimension's text horizontal where it sits.
pub(crate) fn dimension_text_reads_horizontal(dim: &Dimension, style: Option<&DimStyle>) -> bool {
    let outside = dimension_text_is_outside(dim, style);
    style.is_some_and(|style| (outside && style.dimtoh) || (!outside && style.dimtih))
}

pub(crate) fn dimension_text_pos_f64(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
    dim_scale: f64,
    document: Option<&Document>,
) -> Vector3 {
    let base = dim.base();

    // DIMTAD: 0=centred (on the line), 1=above, 4=below. 2 (outside, i.e. the
    // side farthest from the defining points) and 3 (JIS) both resolve to the
    // away-from-object side, which is the same as "above" for 2-D linear dims.
    let dimtad = style.map(|s| s.dimtad).unwrap_or(1);
    // DIMGAP scales with DIMSCALE just like DIMTXT, so the text-to-line gap
    // stays consistent when DIMSCALE != 1.
    let dimgap = style.map(|s| s.dimgap.abs()).unwrap_or(0.0) * dim_scale;
    let dimjust = style.map(|s| s.dimjust).unwrap_or(0);
    // DIMTIX forces the text to stay between the extension lines.
    let dimtix = style.map(|s| s.dimtix).unwrap_or(false);
    let dimatfit = style.map(|s| s.dimatfit).unwrap_or(3);
    // DIMTVP vertical-position multiplier (units of dimtxt). Only honoured when
    // DIMTAD == 0; offsets text perpendicular to the dim line.
    let dimtvp = style.map(|s| s.dimtvp).unwrap_or(0.0);
    let perp_off = if dimtad == 0 {
        dimtvp * text_height
    } else if text_splits_at_line(dim) {
        // `\X`: the first part above the line, the rest below it.
        0.0
    } else {
        dimension_text_extent_height(dim, style, text_height) * 0.5 + dimgap
    };
    // Rough text width + arrow allowance, used to decide text-outside fit.
    let text_w = dimension_text_cells(dim, style)
        .map(|cells| cells * text_height * CELL_WIDTH + 2.0 * dimgap)
        .unwrap_or(0.0);
    let arrow = style
        .map(|style| style.dimasz * dim_scale)
        .filter(|arrow| *arrow > 1e-9)
        .unwrap_or(text_height);
    // DIMTOH: text that lands outside reads horizontally.
    let horizontal_outside = style.is_some_and(|style| style.dimtoh);

    if let Some(moved) = dimtmove_one_text(dim, style, text_height, dim_scale, document) {
        return moved.center;
    }
    if let Some(point) = stored_text_point(dim) {
        return match dim {
            // DIMTMOVE 1 stores where the leader's hook starts: one arrow of
            // hook, a gap, then the text, all running away from the arc.
            // Measured on 39 such dimensions the stored point sits 2.4 text
            // heights before the text, which is DIMASZ plus DIMGAP.
            Dimension::Radius(_) | Dimension::Diameter(_)
                if style.is_some_and(|s| s.dimtmove == 1)
                    || radius_hook_point(dim, style, point, text_height) =>
            {
                let tip = radial_leader_tip(dim, point);
                let side = if point.x >= tip.x { 1.0 } else { -1.0 };
                Vector3::new(point.x + side * (arrow + text_w * 0.5), point.y, point.z)
            }
            // A radius places its text where it was clicked, on the line
            // through the centre and the arc point; DIMTAD lifts aligned text
            // off that line as it does on a diameter, and the line runs on
            // under it. A point already off the line is the text's own
            // middle, as a file saved elsewhere writes it. (#1675)
            Dimension::Radius(d)
                if dimtad != 0
                    && !dimension_text_reads_horizontal(dim, style)
                    && point_on_line(d.angle_vertex, d.definition_point, point, perp_off) =>
            {
                lift_off_line(d.angle_vertex, d.definition_point, point, perp_off, dimtad)
            }
            // A diameter whose text the user dragged keeps the grip point on
            // the leader and lifts the text by the DIMTAD offset (#1323).
            // A stored point already standing off the leader is the text's
            // own middle, as a file saved elsewhere writes it; lifting that
            // again floated the text a text height clear of its line (#898).
            Dimension::Diameter(d)
                if base.text_user_positioned
                    && dimtad != 0
                    && diameter_point_on_leader(d, point, perp_off) =>
            {
                lift_off_line(d.angle_vertex, d.definition_point, point, perp_off, dimtad)
            }
            _ => point,
        };
    }

    match dim {
        Dimension::Linear(d) => {
            let ax = d.rotation.cos();
            let ay = d.rotation.sin();
            text_on_dim_line(
                d.first_point,
                d.second_point,
                d.definition_point,
                ax,
                ay,
                dimjust,
                perp_off,
                text_w,
                dimgap,
                arrow,
                dimtix,
                dimatfit,
                dimtad,
                horizontal_outside,
            )
        }
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            let len = (dx * dx + dy * dy).sqrt().max(1e-12);
            text_on_dim_line(
                d.first_point,
                d.second_point,
                d.definition_point,
                dx / len,
                dy / len,
                dimjust,
                perp_off,
                text_w,
                dimgap,
                arrow,
                dimtix,
                dimatfit,
                dimtad,
                horizontal_outside,
            )
        }
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => {
            let Some((vertex, start, end, radius)) = angular_dimension_frame(dim) else {
                return base.text_middle_point;
            };
            let span = radius as f64 * (end - start).abs() as f64;
            let insufficient = text_w + arrow * 2.0 > span;
            let move_outside = !dimtix
                && insufficient
                && match dimatfit {
                    0 | 2 => true,
                    1 | 3 => text_w > span,
                    _ => text_w > span,
                };
            // Beside an extension line the text starts an arrow and three
            // gaps along the arc from it when it breaks the arc (one gap off
            // the arc): DIMJUST 1 by the second line, 2 by the first
            // (`text_w` carries a gap each side).
            let gaps = if dimtad == 0 { 2.0 } else { 0.0 };
            let beside = (arrow + gaps * dimgap + text_w * 0.5) / (radius as f64).max(1.0e-12);
            let toward_end = if end >= start { 1.0 } else { -1.0 };
            let angle = if move_outside {
                end as f64 + (text_w * 0.5 + dimgap + arrow) / (radius as f64).max(1.0e-12)
            } else if dimjust == 1 {
                end as f64 - toward_end * beside
            } else if dimjust == 2 {
                start as f64 + toward_end * beside
            } else {
                ((start + end) * 0.5) as f64
            };
            let radial_offset = if dimtad == 4 { -perp_off } else { perp_off };
            let text_radius = (radius as f64 + radial_offset).max(0.0);
            Vector3::new(
                vertex.x as f64 + angle.cos() * text_radius,
                vertex.y as f64 + angle.sin() * text_radius,
                vertex.z as f64,
            )
        }
        Dimension::Radius(d) => {
            let dx = d.definition_point.x - d.angle_vertex.x;
            let dy = d.definition_point.y - d.angle_vertex.y;
            let radius = dx.hypot(dy).max(1e-12);
            let ux = dx / radius;
            let uy = dy / radius;
            let outside = dimension_text_is_outside(dim, style);
            if outside && dynamic_constraint_dimension(dim) {
                // A constraint's text moves beside the circle at the end of
                // its leader; an ordinary radius keeps its measured offset.
                return radial_outside_text(d.definition_point, ux, uy, text_height, text_w);
            }
            let (mut x, mut y) = if outside {
                let distance = arrow + text_w * 0.5 + dimgap;
                (
                    d.definition_point.x + ux * distance,
                    d.definition_point.y + uy * distance,
                )
            } else {
                (
                    d.angle_vertex.x + ux * radius * 0.5,
                    d.angle_vertex.y + uy * radius * 0.5,
                )
            };
            if dimtad != 0 {
                let sign = if dimtad == 4 { -1.0 } else { 1.0 };
                x += -uy * perp_off * sign;
                y += ux * perp_off * sign;
            }
            Vector3::new(x, y, d.definition_point.z)
        }
        Dimension::LargeRadial(d) => {
            let dx = d.chord_point.x - d.override_center.x;
            let dy = d.chord_point.y - d.override_center.y;
            let length = dx.hypot(dy).max(1.0e-12);
            text_on_single_arrow_dim_line(
                d.override_center,
                d.chord_point,
                d.definition_point,
                dx / length,
                dy / length,
                perp_off,
                text_w,
                arrow,
                dimtix,
                dimatfit,
                dimtad,
            )
        }
        Dimension::Diameter(d) => {
            let dx = d.definition_point.x - d.angle_vertex.x;
            let dy = d.definition_point.y - d.angle_vertex.y;
            let len = (dx * dx + dy * dy).sqrt().max(1e-12);
            if dimension_text_is_outside(dim, style) && dynamic_constraint_dimension(dim) {
                // Beyond the end the dimension line was placed towards.
                return radial_outside_text(
                    d.angle_vertex,
                    -dx / len,
                    -dy / len,
                    text_height,
                    text_w,
                );
            }
            text_on_dim_line(
                d.angle_vertex,
                d.definition_point,
                d.angle_vertex,
                dx / len,
                dy / len,
                // DIMJUST places linear and angular text only.
                0,
                perp_off,
                text_w,
                dimgap,
                arrow,
                dimtix,
                dimatfit,
                dimtad,
                horizontal_outside,
            )
        }
        Dimension::Ordinate(d) => {
            let (x_axis, y_axis) = d.local_axes();
            let text_axis = if d.is_ordinate_type_x { y_axis } else { x_axis };
            let perpendicular = if d.is_ordinate_type_x { x_axis } else { y_axis };
            let delta = d.leader_endpoint - d.feature_location;
            let direction_sign = if delta.dot(&text_axis) < 0.0 {
                -1.0
            } else {
                1.0
            };
            let perpendicular_sign = if delta.dot(&perpendicular) < 0.0 {
                -1.0
            } else {
                1.0
            };
            let vertical = match dimtad {
                0 => dimtvp * text_height,
                4 => -perp_off,
                _ => perp_off,
            };
            d.leader_endpoint
                + text_axis * (direction_sign * (text_w * 0.5 + dimgap))
                + perpendicular * (perpendicular_sign * vertical)
        }
        _ => {
            // Auto-placed text follows the style offset.
            let mid = match dim {
                Dimension::Angular2Ln(d) => d.dimension_arc,
                Dimension::Angular3Pt(d) => d.definition_point,
                Dimension::Arc(d) => d.definition_point,
                _ => base.text_middle_point,
            };
            Vector3::new(mid.x, mid.y + perp_off * perp_sign_default(), mid.z)
        }
    }
}

pub(crate) fn perp_sign_default() -> f64 {
    1.0
}
