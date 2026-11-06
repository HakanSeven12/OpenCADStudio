//! Windows metafile (WMF) reading: the records a metafile plays, as the
//! drawing objects of a block — outlines as polylines in their pen's colour
//! and width, filled shapes as closed polylines in their brush's colour,
//! ellipses as 99-segment polylines, arcs and pies on whole units, text as
//! single-line text placed and sized by its font's own ascent. Chords are
//! not read.
//!
//! The picture is scaled to fit half the view it is imported into, with its
//! top-left corner at the block's origin and y running down the screen as
//! a metafile's does.

use document::entities::{Polyline2D, Text, Vertex2D};
use document::types::{Color, Vector3};
use document::EntityType;

/// A font face as the text uses it: its name, and whether bold and italic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    pub name: String,
    pub bold: bool,
    pub italic: bool,
}

/// A picture: its objects, in block coordinates, and the font of each
/// text (an index into `fonts`).
pub struct Picture {
    pub entities: Vec<EntityType>,
    /// Each face the text uses, in the order first used; each becomes a
    /// text style.
    pub fonts: Vec<Face>,
    /// For each text in `entities` (by position), its font index.
    pub text_fonts: Vec<(usize, usize)>,
}

#[derive(Clone, Copy)]
struct Pen {
    null: bool,
    width: f64,
    rgb: [u8; 3],
}

#[derive(Clone, Copy)]
struct Brush {
    null: bool,
    rgb: [u8; 3],
}

#[derive(Clone)]
struct Font {
    /// The logical height: negative for the character height, positive for
    /// the cell height.
    height: f64,
    escapement: f64,
    weight: i32,
    italic: bool,
    face: String,
}

/// A font's ascent, in metafile units, as the system lays the font out at
/// its logical height: the baseline sits this far below a text's reference
/// point. Elsewhere it is read from the font file.
#[cfg(target_os = "windows")]
fn ascent(font: &Font) -> f64 {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateFontIndirectW, DeleteDC, DeleteObject, GetTextMetricsW,
        SelectObject, LOGFONTW, TEXTMETRICW,
    };
    // SAFETY: a private memory device context and font, released before
    // returning; the structures are plain data the calls fill.
    unsafe {
        let mut logfont: LOGFONTW = std::mem::zeroed();
        logfont.lfHeight = font.height as i32;
        logfont.lfWeight = font.weight;
        logfont.lfItalic = u8::from(font.italic);
        for (slot, unit) in logfont
            .lfFaceName
            .iter_mut()
            .zip(font.face.encode_utf16().take(31))
        {
            *slot = unit;
        }
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return 0.9 * font.height.abs();
        }
        let handle = CreateFontIndirectW(&logfont);
        let previous = SelectObject(dc, handle);
        let mut metrics: TEXTMETRICW = std::mem::zeroed();
        let ok = GetTextMetricsW(dc, &mut metrics) != 0;
        SelectObject(dc, previous);
        DeleteObject(handle);
        DeleteDC(dc);
        if ok {
            f64::from(metrics.tmAscent)
        } else {
            0.9 * font.height.abs()
        }
    }
}

/// A font's ascent from its file, as the system's font engine sets it: the
/// font's own per-size maximum (its `VDMX` table, 1:1 aspect ratio) where
/// it has one, otherwise the Windows ascent scaled to the size. A negative
/// height is the em size; a positive one the cell height (ascent plus
/// descent), the largest size that fits it.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn file_ascent(face: &ttf_parser::Face, height: f64) -> Option<f64> {
    let upem = f64::from(face.units_per_em());
    let os2 = face.tables().os2?;
    let (win_ascent, win_descent) = (
        f64::from(os2.windows_ascender()),
        -f64::from(os2.windows_descender()),
    );
    let sizes = face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"VDMX"))
        .map(vdmx_square)
        .unwrap_or_default();
    if height < 0.0 {
        let ppem = height.abs().round();
        return Some(match sizes.iter().find(|(p, ..)| f64::from(*p) == ppem) {
            Some(&(_, max, _)) => f64::from(max),
            None => (win_ascent * ppem / upem).round(),
        });
    }
    let fitting = sizes
        .iter()
        .filter(|(_, max, min)| f64::from(*max) - f64::from(*min) <= height)
        .max_by_key(|(p, ..)| *p);
    Some(match fitting {
        Some(&(_, max, _)) if !sizes.is_empty() => f64::from(max),
        _ => (win_ascent * height / (win_ascent + win_descent)).round(),
    })
}

/// The `(ppem, yMax, yMin)` records of a `VDMX` table's group for a 1:1 (or
/// any) aspect ratio.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn vdmx_square(data: &[u8]) -> Vec<(u16, i16, i16)> {
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let (Some(_), Some(ratios)) = (u16_at(2), u16_at(4)) else {
        return Vec::new();
    };
    let ratios = usize::from(ratios);
    let pick = (0..ratios).find(|&r| {
        let at = 6 + 4 * r;
        matches!(data.get(at..at + 4), Some([_, 0, 0, 0]) | Some([_, 1, 1, 1]))
    });
    let Some(group) = pick.and_then(|r| u16_at(6 + 4 * ratios + 2 * r)) else {
        return Vec::new();
    };
    let group = usize::from(group);
    let count = u16_at(group).map_or(0, usize::from);
    (0..count)
        .filter_map(|i| {
            let at = group + 4 + 6 * i;
            Some((u16_at(at)?, u16_at(at + 2)? as i16, u16_at(at + 4)? as i16))
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
fn ascent(font: &Font) -> f64 {
    ascent_from_file(font)
}

/// The installed face's Windows descent and the advance of `value`, both
/// over its cap height (a text's height): what a text of the font
/// reaches below its baseline and how long it runs, per unit of height.
fn run_metrics(font: &Font, value: &str) -> Option<(f64, f64)> {
    let weight = fontdb::Weight(font.weight.clamp(1, 1000) as u16);
    [font.face.as_str(), "Arial"].into_iter().find_map(|family| {
        crate::scene::text::sysfont::with_face_data_weighted(family, weight, |data, index| {
            let face = ttf_parser::Face::parse(data, index).ok()?;
            let upem = f64::from(face.units_per_em());
            let cap = face
                .capital_height()
                .filter(|&c| c > 0)
                .map_or(0.7 * upem, f64::from);
            let descent = face
                .tables()
                .os2
                .map_or(-f64::from(face.descender()), |os2| -f64::from(os2.windows_descender()));
            let advance: f64 = value
                .chars()
                .filter_map(|ch| face.glyph_index(ch))
                .filter_map(|glyph| face.glyph_hor_advance(glyph))
                .map(f64::from)
                .sum();
            Some((descent / cap, advance / cap))
        })
        .flatten()
    })
}

/// [`file_ascent`] of the font's installed face.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn ascent_from_file(font: &Font) -> f64 {
    let weight = fontdb::Weight(font.weight.clamp(1, 1000) as u16);
    // A face that is not installed falls back to the usual sans faces.
    [font.face.as_str(), "Arial", "Liberation Sans", "DejaVu Sans"]
        .into_iter()
        .find_map(|family| {
            crate::scene::text::sysfont::with_face_data_weighted(family, weight, |data, index| {
                file_ascent(&ttf_parser::Face::parse(data, index).ok()?, font.height)
            })
            .flatten()
        })
        .unwrap_or(0.9 * font.height.abs())
}

#[derive(Clone)]
enum Object {
    Pen(Pen),
    Brush(Brush),
    Font(Font),
    Other,
}

/// Segments of a whole ellipse.
const ELLIPSE_SEGMENTS: usize = 99;

/// A colour reference as the nearest index colour; black and white both
/// draw as colour 7.
fn color(rgb: [u8; 3]) -> Color {
    if rgb == [0, 0, 0] || rgb == [255, 255, 255] {
        return Color::Index(7);
    }
    Color::Index(document::types::nearest_aci(rgb[0], rgb[1], rgb[2]))
}

fn colorref(lo: u16, hi: u16) -> [u8; 3] {
    [(lo & 0xff) as u8, (lo >> 8) as u8, (hi & 0xff) as u8]
}

struct Player {
    origin: [f64; 2],
    scale: [f64; 2],
    entities: Vec<EntityType>,
    fonts: Vec<Face>,
    text_fonts: Vec<(usize, usize)>,
}

impl Player {
    fn at(&self, x: f64, y: f64) -> Vector3 {
        Vector3::new((x - self.origin[0]) * self.scale[0], (y - self.origin[1]) * self.scale[1], 0.0)
    }

    fn polyline(&mut self, points: &[[f64; 2]], closed: bool, width: f64, rgb: [u8; 3]) {
        if points.len() < 2 {
            return;
        }
        let width = width * self.scale[0].abs();
        let mut polyline = Polyline2D::new();
        polyline.start_width = width;
        polyline.end_width = width;
        for p in points {
            let mut vertex = Vertex2D::new(self.at(p[0], p[1]));
            vertex.start_width = width;
            vertex.end_width = width;
            polyline.add_vertex(vertex);
        }
        if closed {
            polyline.close();
        }
        polyline.common.color = color(rgb);
        self.entities.push(EntityType::Polyline2D(polyline));
    }

    /// An outline in the pen, or a filled area as its brush's outline.
    fn shape(&mut self, points: &[[f64; 2]], pen: &Pen, brush: &Brush) {
        let width = if pen.null { 0.0 } else { pen.width };
        if !brush.null {
            self.polyline(points, true, width, brush.rgb);
        } else if !pen.null {
            self.polyline(points, true, width, pen.rgb);
        }
    }
}

/// Points round an ellipse inside `(left, top)–(right, bottom)`, from
/// `start` to `end` (radians, counter-clockwise in drawing coordinates).
fn ellipse_points(bounds: [f64; 4], start: f64, sweep: f64) -> Vec<[f64; 2]> {
    let [left, top, right, bottom] = bounds;
    let (cx, cy) = (0.5 * (left + right), 0.5 * (top + bottom));
    let (rx, ry) = (0.5 * (right - left).abs(), 0.5 * (bottom - top).abs());
    let step = std::f64::consts::TAU / ELLIPSE_SEGMENTS as f64;
    let count = ((sweep.abs() / step).round() as usize).max(1);
    (0..=count)
        .map(|k| {
            let a = start + sweep * k as f64 / count as f64;
            // Metafile y runs down: counter-clockwise on the drawing is
            // up the page.
            [cx + rx * a.cos(), cy - ry * a.sin()]
        })
        .collect()
}

/// Turns an arc is cut into: its points lie on whole multiples of this
/// share of a turn.
const ARC_STEPS: i64 = 99;

/// The angle of a radial point round an ellipse, in degrees in `(0, 360]`
/// counter-clockwise in drawing orientation — or `None` for a point inside
/// the arc's outline (`grid`), which marks no angle.
fn radial_angle(bounds: [f64; 4], grid: &[[f64; 2]], x: f64, y: f64) -> Option<f64> {
    if inside_outline(grid, x, y) {
        return None;
    }
    let [left, top, right, bottom] = bounds;
    let (cx, cy) = (0.5 * (left + right), 0.5 * (top + bottom));
    let (rx, ry) = (0.5 * (right - left).abs(), 0.5 * (bottom - top).abs());
    let (u, v) = ((x - cx) / rx, -(y - cy) / ry);
    let degrees = v.atan2(u).to_degrees().rem_euclid(360.0);
    Some(if degrees == 0.0 { 360.0 } else { degrees })
}

/// Whether a point lies strictly inside the closed polygon `outline`; a
/// point on its boundary is outside.
fn inside_outline(outline: &[[f64; 2]], x: f64, y: f64) -> bool {
    let n = outline.len();
    let mut inside = false;
    for i in 0..n {
        let ([xi, yi], [xj, yj]) = (outline[i], outline[(i + n - 1) % n]);
        let cross = (xj - xi) * (y - yi) - (yj - yi) * (x - xi);
        if cross == 0.0
            && (xi.min(xj)..=xi.max(xj)).contains(&x)
            && (yi.min(yj)..=yi.max(yj)).contains(&y)
        {
            return false;
        }
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
    }
    inside
}

/// An arc's points: round the ellipse's whole-unit centre and radii at
/// whole multiples of 1/99 turn, cut
/// to whole units (toward zero), from the start angle up to the end angle — past
/// 360 degrees back from zero when the end comes first. A start inside the
/// outline those whole-unit points make round the whole ellipse starts at
/// zero; an end inside it runs on to a whole turn; with both inside there
/// is no arc. A point on the outline is outside it.
fn arc_points(bounds: [f64; 4], start: [f64; 2], end: [f64; 2]) -> Vec<[f64; 2]> {
    let [left, top, right, bottom] = bounds.map(|v| v as i64);
    let (cx, cy) = ((left + right).div_euclid(2), (top + bottom).div_euclid(2));
    let (rx, ry) = ((right - left).abs() / 2, (bottom - top).abs() / 2);
    // The unit vectors are stepped out from zero by turning the last one
    // through 1/99 turn in plain doubles, rounding and all, so the error
    // grows from step to step as the points come out.
    const STEP_COS: f64 = 0.99798667647188444; // cos(2π/99)
    const STEP_SIN: f64 = 0.063423919656564506; // sin(2π/99)
    let mut turns = vec![(1.0f64, 0.0f64); ARC_STEPS as usize + 1];
    for k in 1..turns.len() {
        let (c, s) = turns[k - 1];
        turns[k] = (c * STEP_COS - s * STEP_SIN, s * STEP_COS + c * STEP_SIN);
    }
    let point = |k: i64| {
        let (cos, sin) = turns[k as usize];
        [
            (cx as f64 + rx as f64 * cos).trunc(),
            (cy as f64 - ry as f64 * sin).trunc(),
        ]
    };
    let grid: Vec<[f64; 2]> = (0..ARC_STEPS).map(point).collect();
    let (from, to) = (
        radial_angle(bounds, &grid, start[0], start[1]),
        radial_angle(bounds, &grid, end[0], end[1]),
    );
    let (start_angle, end_angle) = match (from, to) {
        (None, None) => return Vec::new(),
        (None, to) => (0.0, to),
        (Some(from), to) => (from, to.filter(|to| *to != from)),
    };
    // `k` steps lie below an angle when k·360 < angle·99, kept exact.
    let under = |k: &i64, angle: f64| ((k * 360) as f64) < angle * ARC_STEPS as f64;
    let first = (start_angle * ARC_STEPS as f64 / 360.0).ceil() as i64;
    let mut steps: Vec<i64> = match end_angle {
        // Up to the end.
        Some(to) if to > start_angle => (first..).take_while(|k| under(k, to)).collect(),
        // Round to a whole turn, then from zero up to the end.
        Some(to) => (first..=ARC_STEPS).chain((0..).take_while(|k| under(k, to))).collect(),
        // Round to a whole turn.
        None => (first..=ARC_STEPS).collect(),
    };
    // The whole-unit points next to the start and end rays are judged by
    // which side of the ray they fall: a point on the start ray is left
    // out, one on the end ray kept.
    let (fx, fy) = (0.5 * (bounds[0] + bounds[2]), 0.5 * (bounds[1] + bounds[3]));
    let side = |ray: [f64; 2], k: i64| {
        let p = point(k);
        let (ux, uy) = (ray[0] - fx, -(ray[1] - fy));
        let (vx, vy) = (p[0] - fx, -(p[1] - fy));
        ux * vy - uy * vx
    };
    if from.is_some() {
        if let Some(&k) = steps.first() {
            if k < ARC_STEPS && side(start, k) <= 0.0 {
                steps.remove(0);
            } else if k > 0 && k < ARC_STEPS && side(start, k - 1) > 0.0 {
                steps.insert(0, k - 1);
            }
        }
    }
    if end_angle.is_some() {
        if let Some(&k) = steps.last() {
            if k > 0 && side(end, k) > 0.0 {
                steps.pop();
            } else if k + 1 < ARC_STEPS && side(end, k + 1) <= 0.0 {
                steps.push(k + 1);
            }
        }
    }
    steps.into_iter().map(point).collect()
}

/// Reads a metafile, scaled so the picture fits within `fit` (width and
/// height, drawing units). `None` when it is not a metafile.
pub fn read(bytes: &[u8], fit: [f64; 2]) -> Option<Picture> {
    let word = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
    };
    let mut at = 0;
    let mut bounds = None;
    if word(0)? == 0xCDD7 && word(2)? == 0x9AC6 {
        let s = |k: usize| word(6 + 2 * k).map(|w| w as i16 as f64);
        bounds = Some([s(0)?, s(1)?, s(2)?, s(3)?]);
        at = 22;
    }
    // The metafile header: type 1 or 2, nine words long.
    if !matches!(word(at)?, 1 | 2) || word(at + 2)? != 9 {
        return None;
    }
    at += 18;
    let mut records = Vec::new();
    while at + 6 <= bytes.len() {
        let size = (u32::from(word(at)?) | (u32::from(word(at + 2)?) << 16)) as usize;
        let function = word(at + 4)?;
        if function == 0 || size < 3 {
            break;
        }
        // A record claims no more words than the file has left.
        let words = (size - 3).min((bytes.len() - (at + 6)) / 2);
        let params: Vec<u16> = (0..words).filter_map(|k| word(at + 6 + 2 * k)).collect();
        records.push((function, params));
        at = at.saturating_add(size.saturating_mul(2));
    }

    // The window first: where the picture is and how big.
    let mut origin = [0.0, 0.0];
    let mut extent = None;
    for (function, p) in &records {
        let s = |k: usize| p.get(k).map(|w| *w as i16 as f64);
        match function {
            0x020B => origin = [s(1)?, s(0)?],
            0x020C => extent = Some([s(1)?, s(0)?]),
            _ => {}
        }
    }
    let extent = extent
        .or_else(|| bounds.map(|[l, t, r, b]| [r - l, b - t]))
        .filter(|[w, h]| *w != 0.0 && *h != 0.0)?;
    if let (None, Some([l, t, _, _])) = (records.iter().find(|(f, _)| *f == 0x020B), bounds) {
        origin = [l, t];
    }
    let scale = (fit[0] / extent[0].abs()).min(fit[1] / extent[1].abs());
    let mut player = Player {
        origin,
        scale: [scale * extent[0].signum(), -scale * extent[1].signum()],
        entities: Vec::new(),
        fonts: Vec::new(),
        text_fonts: Vec::new(),
    };

    // A fresh device context: a one-unit black pen, a white brush.
    let mut pen = Pen {
        null: false,
        width: 1.0,
        rgb: [0, 0, 0],
    };
    let mut brush = Brush {
        null: false,
        rgb: [255, 255, 255],
    };
    let mut font = Font {
        height: 12.0,
        escapement: 0.0,
        weight: 400,
        italic: false,
        face: String::new(),
    };
    let mut text_rgb = [0, 0, 0];
    let mut text_align = 0u16;
    let mut table: Vec<Option<Object>> = Vec::new();
    let mut current = [0.0, 0.0];
    for (function, p) in &records {
        let s = |k: usize| p.get(k).map(|w| *w as i16 as f64).unwrap_or(0.0);
        let points = |from: usize, count: usize| -> Vec<[f64; 2]> {
            (0..count).map(|k| [s(from + 2 * k), s(from + 2 * k + 1)]).collect()
        };
        let create = |table: &mut Vec<Option<Object>>, object: Object| {
            match table.iter().position(Option::is_none) {
                Some(slot) => table[slot] = Some(object),
                None => table.push(Some(object)),
            }
        };
        match function {
            // CREATEPENINDIRECT
            0x02FA => create(
                &mut table,
                Object::Pen(Pen {
                    null: p.first().is_some_and(|style| style & 0x0f == 5),
                    width: s(1),
                    rgb: colorref(p.get(3).copied().unwrap_or(0), p.get(4).copied().unwrap_or(0)),
                }),
            ),
            // CREATEBRUSHINDIRECT
            0x02FC => create(
                &mut table,
                Object::Brush(Brush {
                    null: p.first() == Some(&1),
                    rgb: colorref(p.get(1).copied().unwrap_or(0), p.get(2).copied().unwrap_or(0)),
                }),
            ),
            // CREATEFONTINDIRECT: height, width, escapement, ... face at
            // byte 18.
            0x02FB => {
                let mut face = Vec::new();
                for w in p.iter().skip(9) {
                    face.extend(w.to_le_bytes());
                }
                let end = face.iter().position(|b| *b == 0).unwrap_or(face.len());
                create(
                    &mut table,
                    Object::Font(Font {
                        height: s(0),
                        escapement: s(2) / 10.0,
                        weight: s(4) as i32,
                        italic: p.get(5).is_some_and(|w| w & 0xff != 0),
                        face: String::from_utf8_lossy(&face[..end]).into_owned(),
                    }),
                )
            }
            // Palettes and regions take a slot too.
            0x00F7 | 0x06FF | 0x01F9 | 0x0142 => create(&mut table, Object::Other),
            // SELECTOBJECT
            0x012D => match table.get(p.first().copied().unwrap_or(0) as usize) {
                Some(Some(Object::Pen(x))) => pen = *x,
                Some(Some(Object::Brush(x))) => brush = *x,
                Some(Some(Object::Font(x))) => font = x.clone(),
                _ => {}
            },
            // DELETEOBJECT
            0x01F0 => {
                if let Some(slot) = table.get_mut(p.first().copied().unwrap_or(0) as usize) {
                    *slot = None;
                }
            }
            // SETTEXTALIGN
            0x012E => text_align = p.first().copied().unwrap_or(0),
            // SETTEXTCOLOR
            0x0209 => text_rgb = colorref(p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0)),
            // MOVETO, LINETO (y, x)
            0x0214 => current = [s(1), s(0)],
            0x0213 => {
                let to = [s(1), s(0)];
                if !pen.null {
                    player.polyline(&[current, to], false, pen.width, pen.rgb);
                }
                current = to;
            }
            // POLYLINE
            0x0325 => {
                if !pen.null {
                    let line = points(1, s(0) as usize);
                    player.polyline(&line, false, pen.width, pen.rgb);
                }
            }
            // POLYGON
            0x0324 => {
                let ring = points(1, s(0) as usize);
                player.shape(&ring, &pen, &brush);
            }
            // POLYPOLYGON: counts, then every ring's points
            0x0538 => {
                let rings = s(0) as usize;
                let mut from = 1 + rings;
                for k in 0..rings {
                    let count = s(1 + k) as usize;
                    let ring = points(from, count);
                    player.shape(&ring, &pen, &brush);
                    from += 2 * count;
                }
            }
            // RECTANGLE, ROUNDRECT (bottom, right, top, left; corner
            // ellipse first for the latter)
            0x041B | 0x061C => {
                let o = if *function == 0x061C { 2 } else { 0 };
                let (b, r, t, l) = (s(o), s(o + 1), s(o + 2), s(o + 3));
                player.shape(&[[l, t], [r, t], [r, b], [l, b]], &pen, &brush);
            }
            // ELLIPSE: its outline in the pen
            0x0418 => {
                if !pen.null {
                    let bounds = [s(3), s(2), s(1), s(0)];
                    let ring = ellipse_points(bounds, 0.0, std::f64::consts::TAU);
                    player.polyline(&ring, false, pen.width, pen.rgb);
                }
            }
            // ARC, PIE: (yEnd, xEnd, yStart, xStart, bottom, right, top,
            // left), drawn counter-clockwise; a pie's outline starts at the
            // centre. (CHORD records are not read.)
            0x0817 | 0x081A => {
                let bounds = [s(7), s(6), s(5), s(4)];
                let arc = arc_points(bounds, [s(3), s(2)], [s(1), s(0)]);
                if arc.is_empty() {
                    continue;
                }
                if *function == 0x0817 {
                    if !pen.null {
                        player.polyline(&arc, false, pen.width, pen.rgb);
                    }
                } else {
                    let centre = [
                        ((bounds[0] + bounds[2]) as i64).div_euclid(2) as f64,
                        ((bounds[1] + bounds[3]) as i64).div_euclid(2) as f64,
                    ];
                    let ring: Vec<[f64; 2]> = std::iter::once(centre).chain(arc).collect();
                    player.shape(&ring, &pen, &brush);
                }
            }
            // TEXTOUT (count, string, y, x) and EXTTEXTOUT (y, x, count,
            // options, [rectangle], string)
            0x0521 | 0x0A32 => {
                let (count, text_at, (x, y)) = if *function == 0x0521 {
                    let count = s(0) as usize;
                    let words = count.div_ceil(2);
                    (count, 1, (s(2 + words), s(1 + words)))
                } else {
                    let count = s(2) as usize;
                    let clipped = p.get(3).is_some_and(|o| o & 0x0006 != 0);
                    (count, if clipped { 8 } else { 4 }, (s(1), s(0)))
                };
                let mut raw = Vec::new();
                for w in p.iter().skip(text_at) {
                    raw.extend(w.to_le_bytes());
                }
                raw.truncate(count);
                let value: String = raw.iter().map(|b| char::from(*b)).collect();
                // A run of spaces is still a text object; only no text is none.
                if value.is_empty() {
                    continue;
                }
                // The text is four fifths of the font's ascent tall. The
                // alignment places it as a text of the full ascent's height
                // would sit: a top reference point has the baseline that
                // height below it, a bottom one the face's Windows descent
                // at that height above it, a baseline one on it; a right
                // reference point ends the run's advance at it, a centre one
                // halves it.
                let rotation = font.escapement.to_radians();
                let height = ascent(&font);
                let (descent, width) = run_metrics(&font, &value).unwrap_or((0.0, 0.0));
                let drop = match text_align & 0x18 {
                    0x08 => -descent * height,
                    0x18 => 0.0,
                    _ => height,
                };
                let along = match text_align & 0x06 {
                    0x02 => -width * height,
                    0x06 => -0.5 * width * height,
                    _ => 0.0,
                };
                let base = player.at(
                    x + drop * rotation.sin() + along * rotation.cos(),
                    y + drop * rotation.cos() - along * rotation.sin(),
                );
                let face = Face {
                    name: font.face.clone(),
                    bold: font.weight >= 600,
                    italic: font.italic,
                };
                let index = match player.fonts.iter().position(|f| *f == face) {
                    Some(k) => k,
                    None => {
                        player.fonts.push(face);
                        player.fonts.len() - 1
                    }
                };
                let mut text = Text::new();
                text.value = value;
                text.insertion_point = base;
                text.height = 0.8 * height * scale;
                text.rotation = rotation;
                text.common.color = color(text_rgb);
                player.text_fonts.push((player.entities.len(), index));
                player.entities.push(EntityType::Text(text));
            }
            _ => {}
        }
    }
    Some(Picture {
        entities: player.entities,
        fonts: player.fonts,
        text_fonts: player.text_fonts,
    })
}
