//! TXT2MTXT (Combine Text): TEXT and MTEXT objects merged into one MTEXT, or
//! each TEXT (and each line of an MTEXT) turned into an MTEXT of its own.
//!
//! The combined MTEXT sits at the top-left corner of the first object (the
//! topmost one unless the selection order is kept) and takes its layer, style,
//! height and rotation. With word-wrap the objects run on separated by spaces
//! in a box as wide as their bounding box (plus half a percent), and an even
//! pitch between them becomes the line spacing factor. Without it each object
//! starts a paragraph; unless line spacing is forced uniform, each paragraph
//! keeps its own pitch (`\pxsm` / `\psm`). A height change starts `\H<h>;`.

use codec::entities::{
    AttachmentPoint, DrawingDirection, LineSpacingStyle, MText, TextHorizontalAlignment as HA,
};
use codec::types::Vector3;
use codec::{Document, EntityType, Handle};

use crate::app::settings::Txt2MtxtSettings;
use crate::entities::text_support::{resolve_dxf_special_chars, text_local_bounds};

/// What the command does to the drawing.
pub struct Txt2MtxtPlan {
    pub erase: Vec<Handle>,
    pub add: Vec<EntityType>,
}

/// One TEXT or MTEXT of the selection, in its own frame.
struct Piece<'a> {
    handle: Handle,
    entity: &'a EntityType,
    /// MTEXT-encoded value.
    content: String,
    height: f64,
    /// The frame's origin (WCS) and direction.
    origin: [f64; 2],
    rotation: f64,
    /// Box in the frame: left, right, last baseline, top.
    rect: [f64; 4],
}

impl Piece<'_> {
    fn point(&self, x: f64, y: f64) -> [f64; 2] {
        let (s, c) = self.rotation.sin_cos();
        [
            self.origin[0] + x * c - y * s,
            self.origin[1] + x * s + y * c,
        ]
    }

    fn top_left(&self) -> [f64; 2] {
        self.point(self.rect[0], self.rect[3])
    }

    fn corners(&self) -> [[f64; 2]; 4] {
        let [l, r, b, t] = self.rect;
        [
            self.point(l, b),
            self.point(r, b),
            self.point(r, t),
            self.point(l, t),
        ]
    }

    /// First and last baselines, as heights in a frame turned by `angle`.
    fn baselines(&self, angle: f64) -> (f64, f64) {
        let [l, _, b, t] = self.rect;
        (
            up(self.point(l, t - self.height), angle),
            up(self.point(l, b), angle),
        )
    }
}

/// Height of a WCS point in a frame turned by `angle`.
fn up(p: [f64; 2], angle: f64) -> f64 {
    let (s, c) = angle.sin_cos();
    -p[0] * s + p[1] * c
}

/// A TEXT value as MTEXT content: `%%u` / `%%o` become underline / overline
/// switches, braces are escaped, other `%%` codes stay, and a backslash is
/// dropped unless it starts a `\U+` / `\M+` character code.
fn text_content(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let (mut under, mut over) = (false, false);
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let next = chars.get(i + 1).copied();
        if ch == '%' && next == Some('%') {
            match chars.get(i + 2).map(|c| c.to_ascii_lowercase()) {
                Some('u') => {
                    out.push_str(if under { "\\l" } else { "\\L" });
                    under = !under;
                    i += 3;
                    continue;
                }
                Some('o') => {
                    out.push_str(if over { "\\o" } else { "\\O" });
                    over = !over;
                    i += 3;
                    continue;
                }
                _ => {}
            }
        }
        match ch {
            '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '\\' if matches!(next, Some('U' | 'M')) && chars.get(i + 2) == Some(&'+') => {
                out.push(ch)
            }
            '\\' => {}
            _ => out.push(ch),
        }
        i += 1;
    }
    out
}

fn piece<'a>(handle: Handle, entity: &'a EntityType, doc: &Document) -> Option<Piece<'a>> {
    match entity {
        EntityType::Text(t) => {
            let p = crate::entities::text::text_run_placement_at_scale(t, doc, 1.0);
            // The text runs to its last inked stroke, not to the pen's end
            // (measured: "Second line" in txt at 2.5 spans 23.75, the
            // advance less the last glyph's gap).
            let right = text_local_bounds(
                &p.font,
                &resolve_dxf_special_chars(&t.value),
                p.height,
                p.width_factor,
                p.oblique_angle,
            )
            .map_or(0.0, |b| b.ink_max[0] as f64);
            Some(Piece {
                handle,
                entity,
                content: text_content(&t.value),
                height: p.height as f64,
                origin: p.origin,
                rotation: p.rotation as f64,
                rect: [0.0, right, 0.0, p.height as f64],
            })
        }
        EntityType::MText(m) => {
            let layout = crate::entities::mtext::layout_of(m, doc, false);
            let width = layout.line_widths.iter().fold(0.0_f32, |a, w| a.max(*w)) as f64;
            let anchor = match m.attachment_point {
                AttachmentPoint::TopCenter
                | AttachmentPoint::MiddleCenter
                | AttachmentPoint::BottomCenter => 0.5,
                AttachmentPoint::TopRight
                | AttachmentPoint::MiddleRight
                | AttachmentPoint::BottomRight => 1.0,
                _ => 0.0,
            };
            // The block runs from the first line's cap height down to the
            // last baseline, hung from the attachment point.
            let lines = layout.line_count.max(1);
            let block = m.height + (lines - 1) as f64 * layout.line_height as f64;
            let top = match m.attachment_point {
                AttachmentPoint::MiddleLeft
                | AttachmentPoint::MiddleCenter
                | AttachmentPoint::MiddleRight => block / 2.0,
                AttachmentPoint::BottomLeft
                | AttachmentPoint::BottomCenter
                | AttachmentPoint::BottomRight => block,
                _ => 0.0,
            };
            let left = -anchor * width;
            Some(Piece {
                handle,
                entity,
                content: m.value.clone(),
                height: m.height,
                origin: [m.insertion_point.x, m.insertion_point.y],
                rotation: m.rotation,
                rect: [left, left + width, top - block, top],
            })
        }
        _ => None,
    }
}

/// A new MTEXT with `source`'s layer, colour and style.
fn mtext_from(source: &EntityType, value: String, at: [f64; 2], height: f64, rotation: f64) -> MText {
    let mut m = MText::new();
    let (common, style, z, normal) = match source {
        EntityType::Text(t) => (&t.common, &t.style, t.insertion_point.z, t.normal),
        EntityType::MText(t) => (&t.common, &t.style, t.insertion_point.z, t.normal),
        _ => unreachable!("pieces are TEXT or MTEXT"),
    };
    m.common = common.clone();
    m.common.handle = Handle::NULL;
    m.common.reactors.clear();
    m.common.xdictionary_handle = None;
    m.style = style.clone();
    m.normal = normal;
    m.value = value;
    m.insertion_point = Vector3::new(at[0], at[1], z);
    m.height = height;
    m.rotation = rotation;
    m.rectangle_width = 0.0;
    m.attachment_point = AttachmentPoint::TopLeft;
    m.drawing_direction = DrawingDirection::ByStyle;
    m.line_spacing_style = LineSpacingStyle::AtLeast;
    m.line_spacing_factor = 1.0;
    m
}

/// Each TEXT and each line of an MTEXT as an MTEXT of its own. A TEXT keeps
/// its horizontal justification as a top attachment.
fn separate(p: &Piece, doc: &Document) -> Vec<EntityType> {
    match p.entity {
        EntityType::Text(t) => {
            let (x, attachment) = match t.horizontal_alignment {
                HA::Center | HA::Middle => (0.5, AttachmentPoint::TopCenter),
                HA::Right => (1.0, AttachmentPoint::TopRight),
                _ => (0.0, AttachmentPoint::TopLeft),
            };
            let at = p.point(p.rect[0] + x * (p.rect[1] - p.rect[0]), p.rect[3]);
            let mut m = mtext_from(p.entity, p.content.clone(), at, p.height, p.rotation);
            m.attachment_point = attachment;
            vec![EntityType::MText(Box::new(m))]
        }
        EntityType::MText(m) => {
            // The drawn words, grouped into lines by their baseline.
            // ponytail: a line keeps only its words, not inline formatting.
            let words = crate::entities::mtext::explode_mtext(m, doc);
            let mut lines: Vec<(f64, Vec<(f64, &codec::entities::Text)>)> = Vec::new();
            for word in &words {
                let EntityType::Text(t) = word else { continue };
                let at = [t.insertion_point.x, t.insertion_point.y];
                let (s, c) = m.rotation.sin_cos();
                let along = at[0] * c + at[1] * s;
                let v = up(at, m.rotation);
                match lines
                    .iter_mut()
                    .find(|(base, _)| (base - v).abs() < m.height * 0.25)
                {
                    Some((_, line)) => line.push((along, t)),
                    None => lines.push((v, vec![(along, t)])),
                }
            }
            lines.sort_by(|a, b| b.0.total_cmp(&a.0));
            // Line k hangs one line step below line k-1 from the block's top.
            let step = (p.rect[3] - p.rect[2] - p.height) / (lines.len().max(2) - 1) as f64;
            let origin_along = {
                let (s, c) = m.rotation.sin_cos();
                p.origin[0] * c + p.origin[1] * s
            };
            lines
                .into_iter()
                .enumerate()
                .map(|(k, (_, mut line))| {
                    line.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let value = line
                        .iter()
                        .map(|(_, t)| t.value.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let at = p.point(line[0].0 - origin_along, p.rect[3] - k as f64 * step);
                    let height = line[0].1.height;
                    EntityType::MText(Box::new(mtext_from(p.entity, value, at, height, m.rotation)))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// The edit for `selected` (in selection order); `None` when nothing in it is
/// text.
pub fn plan(
    selected: &[(Handle, &EntityType)],
    doc: &Document,
    settings: Txt2MtxtSettings,
) -> Option<Txt2MtxtPlan> {
    let mut pieces: Vec<Piece> = selected
        .iter()
        .filter_map(|(h, e)| piece(*h, e, doc))
        .collect();
    let frame = pieces.first()?.rotation;
    if !settings.combine {
        return Some(Txt2MtxtPlan {
            erase: pieces.iter().map(|p| p.handle).collect(),
            add: pieces.iter().flat_map(|p| separate(p, doc)).collect(),
        });
    }
    if !settings.select_order {
        // Top-down; a tie keeps the selection order.
        let top = |p: &Piece| p.corners().map(|c| up(c, frame)).into_iter().fold(f64::MIN, f64::max);
        pieces.sort_by(|a, b| top(b).total_cmp(&top(a)));
    }
    let first = &pieces[0];
    let frame = first.rotation;
    // Each paragraph's pitch to the next one, as a factor of the default
    // step (5/3 of its text height).
    let pitches: Vec<f64> = pieces
        .windows(2)
        .map(|w| {
            let gap = w[0].baselines(frame).1 - w[1].baselines(frame).0;
            (gap / (w[0].height * 5.0 / 3.0)).clamp(0.25, 4.0)
        })
        .collect();
    let per_paragraph = !settings.word_wrap && !settings.uniform_spacing;
    let mut value = String::new();
    let mut current = first.height;
    for (i, p) in pieces.iter().enumerate() {
        if i > 0 {
            value.push_str(if settings.word_wrap { " " } else { "\\P" });
        }
        if let (true, Some(pitch)) = (per_paragraph, pitches.get(i)) {
            let x = if i == 0 { "x" } else { "" };
            value.push_str(&format!("\\p{x}sm{pitch:.2};"));
        }
        if (p.height - current).abs() > 1e-9 {
            value.push_str(&format!("\\H{:.4};", p.height));
            current = p.height;
        }
        value.push_str(&p.content);
    }
    let mut m = mtext_from(first.entity, value, first.top_left(), first.height, frame);
    if settings.word_wrap {
        // The objects' WCS bounding box, measured along the first one.
        let mut bounds = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for c in pieces.iter().flat_map(|p| p.corners()) {
            bounds = [
                bounds[0].min(c[0]),
                bounds[1].min(c[1]),
                bounds[2].max(c[0]),
                bounds[3].max(c[1]),
            ];
        }
        let (s, c) = frame.sin_cos();
        let along = [
            [bounds[0], bounds[1]],
            [bounds[2], bounds[1]],
            [bounds[2], bounds[3]],
            [bounds[0], bounds[3]],
        ]
        .map(|p| p[0] * c + p[1] * s);
        let span = along.iter().fold(f64::MIN, |a, v| a.max(*v))
            - along.iter().fold(f64::MAX, |a, v| a.min(*v));
        m.rectangle_width = span * 1.005;
        // An even pitch carries over; an uneven one falls back to single.
        if let Some(&pitch) = pitches.first() {
            if pitches.iter().all(|p| (p - pitch).abs() < 1e-4) {
                m.line_spacing_factor = pitch;
            }
        }
    }
    Some(Txt2MtxtPlan {
        erase: pieces.iter().map(|p| p.handle).collect(),
        add: vec![EntityType::MText(Box::new(m))],
    })
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["TXT2MTXT"]
});
