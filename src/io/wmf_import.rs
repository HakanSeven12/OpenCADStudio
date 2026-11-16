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
        matches!(
            data.get(at..at + 4),
            Some([_, 0, 0, 0]) | Some([_, 1, 1, 1])
        )
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
    [font.face.as_str(), "Arial"]
        .into_iter()
        .find_map(|family| {
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
                    .map_or(-f64::from(face.descender()), |os2| {
                        -f64::from(os2.windows_descender())
                    });
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
    [
        font.face.as_str(),
        "Arial",
        "Liberation Sans",
        "DejaVu Sans",
    ]
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
        Vector3::new(
            (x - self.origin[0]) * self.scale[0],
            (y - self.origin[1]) * self.scale[1],
            0.0,
        )
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
    const STEP_COS: f64 = 0.997_986_676_471_884_4; // cos(2π/99)
    const STEP_SIN: f64 = 0.063_423_919_656_564_5; // sin(2π/99)
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
        Some(to) => (first..=ARC_STEPS)
            .chain((0..).take_while(|k| under(k, to)))
            .collect(),
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
        // `None` when the record holds fewer points than it claims: a
        // record cut short is skipped, so a count read from the file
        // never makes more points than the file has.
        let points = |from: usize, count: usize| -> Option<Vec<[f64; 2]>> {
            (from + 2 * count <= p.len()).then(|| {
                (0..count)
                    .map(|k| [s(from + 2 * k), s(from + 2 * k + 1)])
                    .collect()
            })
        };
        let create = |table: &mut Vec<Option<Object>>, object: Object| match table
            .iter()
            .position(Option::is_none)
        {
            Some(slot) => table[slot] = Some(object),
            None => table.push(Some(object)),
        };
        match function {
            // CREATEPENINDIRECT
            0x02FA => create(
                &mut table,
                Object::Pen(Pen {
                    null: p.first().is_some_and(|style| style & 0x0f == 5),
                    width: s(1),
                    rgb: colorref(
                        p.get(3).copied().unwrap_or(0),
                        p.get(4).copied().unwrap_or(0),
                    ),
                }),
            ),
            // CREATEBRUSHINDIRECT
            0x02FC => create(
                &mut table,
                Object::Brush(Brush {
                    null: p.first() == Some(&1),
                    rgb: colorref(
                        p.get(1).copied().unwrap_or(0),
                        p.get(2).copied().unwrap_or(0),
                    ),
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
            0x0209 => {
                text_rgb = colorref(
                    p.first().copied().unwrap_or(0),
                    p.get(1).copied().unwrap_or(0),
                )
            }
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
                    if let Some(line) = points(1, s(0) as usize) {
                        player.polyline(&line, false, pen.width, pen.rgb);
                    }
                }
            }
            // POLYGON
            0x0324 => {
                if let Some(ring) = points(1, s(0) as usize) {
                    player.shape(&ring, &pen, &brush);
                }
            }
            // POLYPOLYGON: counts, then every ring's points; one ring cut
            // short skips them all
            0x0538 => {
                let rings = s(0) as usize;
                let mut from = 1 + rings;
                let shapes: Option<Vec<_>> = (0..rings)
                    .map(|k| {
                        let count = s(1 + k) as usize;
                        let ring = points(from, count);
                        from += 2 * count;
                        ring
                    })
                    .collect();
                for ring in shapes.unwrap_or_default() {
                    player.shape(&ring, &pen, &brush);
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

#[cfg(test)]
mod tests {
    use super::*;

    const POLYGON: u16 = 0x0324;
    const POLYLINE: u16 = 0x0325;
    const POLYPOLYGON: u16 = 0x0538;
    /// Three points (x, y): (0, 0), (500, 0), (500, 500).
    const TRIANGLE: [u16; 6] = [0, 0, 500, 0, 500, 500];

    /// A record: its size in words, its function, its parameters.
    fn record(function: u16, params: &[u16]) -> Vec<u8> {
        let mut out = (3 + params.len() as u32).to_le_bytes().to_vec();
        out.extend(function.to_le_bytes());
        for p in params {
            out.extend(p.to_le_bytes());
        }
        out
    }

    /// A metafile: the header, a 1000 by 1000 window, `records`, the end.
    fn metafile(records: &[Vec<u8>]) -> Vec<u8> {
        bare_metafile(&[&[record(0x020C, &[1000, 1000])][..], records].concat())
    }

    /// A metafile: the header, `records`, the end.
    fn bare_metafile(records: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for r in records {
            body.extend(r);
        }
        body.extend(record(0x0000, &[]));
        let mut out = Vec::new();
        for word in [1u16, 9, 0x0300] {
            out.extend(word.to_le_bytes());
        }
        out.extend(((18 + body.len() as u32) / 2).to_le_bytes());
        out.extend([0; 8]); // objects, largest record, parameters: not read
        out.extend(body);
        out
    }

    /// The vertex count of each polyline the file gives, in order.
    fn polylines(bytes: &[u8]) -> Vec<usize> {
        read(bytes, [100.0, 100.0])
            .expect("a metafile")
            .entities
            .iter()
            .filter_map(|e| match e {
                EntityType::Polyline2D(p) => Some(p.vertices.len()),
                _ => None,
            })
            .collect()
    }

    fn with_count(count: u16, points: &[u16]) -> Vec<u16> {
        [&[count][..], points].concat()
    }

    #[test]
    fn records_that_hold_their_points_are_drawn() {
        let rings = [&[2, 3, 3][..], &TRIANGLE, &TRIANGLE].concat();
        let file = metafile(&[
            record(POLYLINE, &with_count(3, &TRIANGLE)),
            record(POLYGON, &with_count(3, &TRIANGLE)),
            record(POLYPOLYGON, &rings),
        ]);
        assert_eq!(polylines(&file), [3, 3, 3, 3]);
    }

    #[test]
    fn a_record_cut_short_is_skipped() {
        // Each claims more points than it holds; the triangle after them
        // is still drawn.
        let file = metafile(&[
            record(POLYLINE, &with_count(4, &TRIANGLE)),
            record(POLYGON, &with_count(4, &TRIANGLE)),
            record(POLYLINE, &[32767]),
            record(POLYLINE, &with_count(3, &TRIANGLE)),
        ]);
        assert_eq!(polylines(&file), [3]);
    }

    #[test]
    fn a_polypolygon_cut_short_is_skipped_whole() {
        // The second ring is one point short; then ring counts that are
        // not all there.
        let short_ring = [&[2, 3, 4][..], &TRIANGLE, &TRIANGLE].concat();
        let file = metafile(&[
            record(POLYPOLYGON, &short_ring),
            record(POLYPOLYGON, &[3, 3]),
        ]);
        assert_eq!(polylines(&file), [0usize; 0]);
    }

    #[test]
    fn no_file_gives_more_points_than_it_holds() {
        let mut records = vec![record(POLYLINE, &[32767]); 100];
        records.push(record(POLYPOLYGON, &[&[100][..], &[32767; 100]].concat()));
        let file = metafile(&records);
        let vertices: usize = polylines(&file).iter().sum();
        assert!(
            vertices <= file.len() / 4,
            "{vertices} vertices from {} bytes",
            file.len()
        );
    }

    // Exports of independent writers; `tests/fixtures/wmf/README.md` says
    // what each holds.
    const SHAPES: &[u8] = include_bytes!("../../tests/fixtures/wmf/shapes.wmf");
    const RINGS: &[u8] = include_bytes!("../../tests/fixtures/wmf/rings.wmf");
    const CURVES: &[u8] = include_bytes!("../../tests/fixtures/wmf/curves.wmf");
    const TEXT: &[u8] = include_bytes!("../../tests/fixtures/wmf/text.wmf");

    const ARC: u16 = 0x0817;
    const CREATEBRUSHINDIRECT: u16 = 0x02FC;
    const CREATEPALETTE: u16 = 0x00F7;
    const CREATEPENINDIRECT: u16 = 0x02FA;
    const DELETEOBJECT: u16 = 0x01F0;
    const ELLIPSE: u16 = 0x0418;
    const EXTTEXTOUT: u16 = 0x0A32;
    const LINETO: u16 = 0x0213;
    const MOVETO: u16 = 0x0214;
    const PIE: u16 = 0x081A;
    const RECTANGLE: u16 = 0x041B;
    const ROUNDRECT: u16 = 0x061C;
    const SELECTOBJECT: u16 = 0x012D;
    const SETTEXTALIGN: u16 = 0x012E;
    const SETWINDOWEXT: u16 = 0x020C;
    const SETWINDOWORG: u16 = 0x020B;
    const TEXTOUT: u16 = 0x0521;

    /// A pen: style (0 solid, 5 none), width, colour.
    fn pen(style: u16, width: u16, [r, g, b]: [u8; 3]) -> Vec<u8> {
        record(
            CREATEPENINDIRECT,
            &[style, width, 0, u16::from_le_bytes([r, g]), u16::from(b)],
        )
    }

    /// A brush: style (0 solid, 1 none), colour.
    fn brush(style: u16, [r, g, b]: [u8; 3]) -> Vec<u8> {
        record(
            CREATEBRUSHINDIRECT,
            &[style, u16::from_le_bytes([r, g]), u16::from(b), 0],
        )
    }

    /// A string as the words a text record holds, padded to a whole word.
    fn string_words(value: &str) -> Vec<u16> {
        value
            .as_bytes()
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
            .collect()
    }

    /// A TEXTOUT record: count, string, y, x.
    fn textout(value: &str, x: u16, y: u16) -> Vec<u8> {
        let mut params = vec![value.len() as u16];
        params.extend(string_words(value));
        params.extend([y, x]);
        record(TEXTOUT, &params)
    }

    /// An EXTTEXTOUT record: `head` (y, x, count, options and the
    /// rectangle, if any), then the string.
    fn exttextout(head: &[u16], value: &str) -> Vec<u8> {
        record(EXTTEXTOUT, &[head, &string_words(value)[..]].concat())
    }

    /// A placeable header with the given bounds, before the metafile.
    fn placeable(bounds: [i16; 4], file: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for w in [0xCDD7u16, 0x9AC6, 0] {
            out.extend(w.to_le_bytes());
        }
        for b in bounds {
            out.extend(b.to_le_bytes());
        }
        out.extend([0; 8]); // units per inch, reserved, checksum: not read
        out.extend(file);
        out
    }

    fn picture(bytes: &[u8], fit: [f64; 2]) -> Picture {
        read(bytes, fit).expect("a metafile")
    }

    fn lines(picture: &Picture) -> Vec<&Polyline2D> {
        picture
            .entities
            .iter()
            .filter_map(|e| match e {
                EntityType::Polyline2D(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    fn texts(picture: &Picture) -> Vec<&Text> {
        picture
            .entities
            .iter()
            .filter_map(|e| match e {
                EntityType::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    fn points(line: &Polyline2D) -> Vec<[f64; 2]> {
        line.vertices
            .iter()
            .map(|v| [v.location.x, v.location.y])
            .collect()
    }

    fn assert_near(actual: [f64; 2], expected: [f64; 2], tolerance: f64, what: &str) {
        assert!(
            (actual[0] - expected[0]).abs() <= tolerance
                && (actual[1] - expected[1]).abs() <= tolerance,
            "{what}: {actual:?}, expected {expected:?}"
        );
    }

    /// The smallest and largest x and y of the points.
    fn extent(points: &[[f64; 2]]) -> [f64; 4] {
        points.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |[x0, y0, x1, y1], [x, y]| [x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)],
        )
    }

    /// Inkscape's unit, 1/1200 inch, in millimetres: what it rounds to.
    const INKSCAPE_UNIT: f64 = 25.4 / 1200.0;

    /// The points, in drawing units, of a picture fitted to the size it
    /// was drawn at (Inkscape writes 1/1200 inch; a fit of the drawing's
    /// own size in millimetres gives millimetres back, y up).
    #[test]
    fn an_inkscape_drawing_is_read_at_its_size_with_its_pens_and_brushes() {
        // 200 × 120 mm.
        let picture = picture(SHAPES, [200.0, 120.0]);
        let lines = lines(&picture);
        // A polyline, a triangle, a rectangle, a circle, and the seven
        // letters of "WMF text" Inkscape wrote as outlines (the "e" with
        // its hole: two rings).
        assert_eq!(lines.len(), 12);
        assert!(texts(&picture).is_empty());

        // The polyline: open, in its pen (black, 1 mm).
        let polyline = lines[0];
        assert!(!polyline.is_closed());
        let expected = [[10.0, -10.0], [60.0, -10.0], [60.0, -50.0], [90.0, -30.0]];
        assert_eq!(polyline.vertices.len(), expected.len());
        for (p, e) in points(polyline).into_iter().zip(expected) {
            assert_near(p, e, 0.05, "polyline vertex");
        }
        assert!((polyline.start_width - 1.0).abs() <= INKSCAPE_UNIT);
        assert_eq!(polyline.common.color, color([0, 0, 0]));

        // The triangle: filled red, outlined blue 2 mm; drawn as its fill,
        // closed, at the pen's width.
        let triangle = lines[1];
        assert!(triangle.is_closed());
        assert_eq!(triangle.common.color, color([204, 0, 0]));
        assert_ne!(triangle.common.color, color([0, 0, 204]));
        assert!((triangle.start_width - 2.0).abs() <= INKSCAPE_UNIT);
        let [x0, y0, x1, y1] = extent(&points(triangle));
        assert_near([x0, y0], [110.0, -60.0], 0.05, "triangle corner");
        assert_near([x1, y1], [190.0, -10.0], 0.05, "triangle corner");

        // The rectangle, filled green.
        let rectangle = lines[2];
        assert!(rectangle.is_closed());
        assert_eq!(rectangle.common.color, color([0, 170, 0]));
        let [x0, y0, x1, y1] = extent(&points(rectangle));
        assert_near([x0, y0], [10.0, -100.0], 0.05, "rectangle corner");
        assert_near([x1, y1], [50.0, -70.0], 0.05, "rectangle corner");

        // The circle has no fill: its outline in the pen's purple.
        let circle = lines[3];
        assert!(circle.is_closed());
        assert_eq!(circle.common.color, color([170, 0, 170]));
        assert!((circle.start_width - 1.5).abs() <= INKSCAPE_UNIT);
        for [x, y] in points(circle) {
            let r = (x - 90.0).hypot(y + 90.0);
            assert!((r - 20.0).abs() < 0.1, "circle point at radius {r}");
        }

        // The letters, filled black, with no outline.
        for letter in &lines[4..] {
            assert!(letter.is_closed());
            assert_eq!(letter.common.color, color([0, 0, 0]));
            assert_eq!(letter.start_width, 0.0);
        }
    }

    #[test]
    fn a_polypolygon_gives_each_ring_and_freed_slots_are_reused() {
        // 100 × 100 mm. A square with a square hole, filled dark blue and
        // outlined black; then three triangles in brown, with no outline.
        // The brown brush goes into the slot the blue one left.
        let picture = picture(RINGS, [100.0, 100.0]);
        let lines = lines(&picture);
        assert_eq!(lines.len(), 5);
        let squares = [
            ([10.0, -90.0], [90.0, -10.0]),
            ([30.0, -70.0], [70.0, -30.0]),
        ];
        for (ring, (low, high)) in lines[..2].iter().zip(squares) {
            assert!(ring.is_closed());
            assert_eq!(ring.common.color, color([0, 68, 136]));
            assert!((ring.start_width - 1.0).abs() <= INKSCAPE_UNIT);
            let [x0, y0, x1, y1] = extent(&points(ring));
            assert_near([x0, y0], low, 0.05, "square corner");
            assert_near([x1, y1], high, 0.05, "square corner");
        }
        let tips = [[20.0, -80.0], [55.0, -80.0], [90.0, -80.0]];
        for (triangle, tip) in lines[2..].iter().zip(tips) {
            assert!(triangle.is_closed());
            assert_eq!(triangle.vertices.len(), 3);
            assert_eq!(triangle.common.color, color([136, 68, 0]));
            assert_eq!(triangle.start_width, 0.0);
            assert_near(points(triangle)[1], tip, 0.05, "triangle tip");
        }
    }

    #[test]
    fn curves_are_read_as_the_writer_flattened_them() {
        // 150 × 80 mm: a Bézier curve, and an ellipse filled yellow.
        let picture = picture(CURVES, [150.0, 80.0]);
        let lines = lines(&picture);
        assert_eq!(lines.len(), 2);
        let curve = points(lines[0]);
        assert!(!lines[0].is_closed());
        assert_eq!(curve.len(), 79);
        assert_near(curve[0], [10.0, -70.0], 0.05, "curve start");
        assert_near(curve[78], [145.0, -40.0], 0.05, "curve end");
        assert!((lines[0].start_width - 0.8).abs() <= INKSCAPE_UNIT);

        let ellipse = lines[1];
        assert!(ellipse.is_closed());
        assert_eq!(ellipse.common.color, color([255, 255, 0]));
        let [x0, y0, x1, y1] = extent(&points(ellipse));
        assert_near([x0, y0], [10.0, -55.0], 0.05, "ellipse extent");
        assert_near([x1, y1], [70.0, -25.0], 0.05, "ellipse extent");
    }

    /// LibreOffice writes each text as a TEXTOUT on the baseline, in units
    /// of 1/100 mm, cropped to what it draws.
    #[test]
    fn text_from_libreoffice_keeps_its_string_font_colour_and_angle() {
        let picture = picture(TEXT, [92.62, 72.62]);
        assert!(lines(&picture).is_empty());
        let texts = texts(&picture);
        let values: Vec<&str> = texts.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(values, ["Plain", "Bold italic", "Tilted"]);
        assert_eq!(
            picture.fonts,
            [
                Face {
                    name: "Arial".into(),
                    bold: false,
                    italic: false,
                },
                Face {
                    name: "Times New Roman".into(),
                    bold: true,
                    italic: true,
                },
            ]
        );
        assert_eq!(picture.text_fonts, [(0, 0), (1, 1), (2, 0)]);
        let colors: Vec<Color> = texts.iter().map(|t| t.common.color).collect();
        assert_eq!(
            colors,
            [color([0, 0, 0]), color([0, 0, 204]), color([204, 0, 0])]
        );

        // The file holds the escapement 29.9°.
        assert_eq!(texts[0].rotation, 0.0);
        assert_eq!(texts[1].rotation, 0.0);
        assert!((texts[2].rotation - 29.9f64.to_radians()).abs() < 1e-9);

        // On the baseline, each text sits on its point, so the texts are
        // as far apart as in the drawing: 20 mm down, then 40 mm right
        // and 50 mm down (to the unit LibreOffice rounds to).
        let at: Vec<[f64; 2]> = texts
            .iter()
            .map(|t| [t.insertion_point.x, t.insertion_point.y])
            .collect();
        let apart = |k: usize| [at[k][0] - at[0][0], at[k][1] - at[0][1]];
        assert_near(apart(1), [0.0, -20.0], 0.011, "second text");
        assert_near(apart(2), [40.0, -50.0], 0.011, "third text");

        // Sizes 8 and 6 mm: heights in about that ratio, whichever faces
        // this system has.
        assert!(texts.iter().all(|t| t.height > 0.0));
        let ratio = texts[0].height / texts[1].height;
        assert!((1.2..1.5).contains(&ratio), "height ratio {ratio}");
    }

    // The records below are ones neither Inkscape nor LibreOffice writes
    // (both draw shapes as polygons); their meanings were checked by having
    // LibreOffice read the same records.

    #[test]
    fn rectangles_and_ellipses_fill_their_bounds() {
        let file = metafile(&[
            pen(0, 10, [255, 0, 0]),
            record(SELECTOBJECT, &[0]),
            brush(1, [0, 0, 0]),
            record(SELECTOBJECT, &[1]),
            // Bottom, right, top, left.
            record(RECTANGLE, &[400, 300, 100, 100]),
            // The corner ellipse's height and width first.
            record(ROUNDRECT, &[50, 50, 400, 800, 100, 500]),
            record(ELLIPSE, &[900, 900, 600, 100]),
        ]);
        // A 1000-unit window fitted to 1000: drawing units are metafile
        // units, y up.
        let picture = picture(&file, [1000.0, 1000.0]);
        let lines = lines(&picture);
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert_eq!(line.common.color, color([255, 0, 0]));
            assert_eq!(line.start_width, 10.0);
        }
        assert!(lines[0].is_closed());
        assert_eq!(extent(&points(lines[0])), [100.0, -400.0, 300.0, -100.0]);
        assert!(lines[1].is_closed());
        assert_eq!(extent(&points(lines[1])), [500.0, -400.0, 800.0, -100.0]);

        // The ellipse round (500, 750), 400 by 150, back to its start.
        let ring = points(lines[2]);
        assert_near(ring[0], *ring.last().unwrap(), 1e-9, "ellipse closes");
        for [x, y] in &ring {
            let r = ((x - 500.0) / 400.0).hypot((y + 750.0) / 150.0);
            assert!((r - 1.0).abs() < 1e-9, "ellipse point {x}, {y}");
        }
        let [x0, y0, x1, y1] = extent(&ring);
        assert_near([x0, y0], [100.0, -900.0], 1.0, "ellipse extent");
        assert_near([x1, y1], [900.0, -600.0], 1.0, "ellipse extent");
    }

    /// The points of an arc record round the ellipse in `(0, 0)–(1000,
    /// 500)`: from the ray through `start` counter-clockwise (as seen on
    /// the screen) to the ray through `end`.
    fn arc(function: u16, start: [i16; 2], end: [i16; 2]) -> Picture {
        let w = |v: i16| v as u16;
        picture(
            &metafile(&[
                pen(0, 1, [0, 0, 255]),
                record(SELECTOBJECT, &[0]),
                brush(0, [0, 255, 0]),
                record(SELECTOBJECT, &[1]),
                // End y, x; start y, x; bottom, right, top, left.
                record(
                    function,
                    &[
                        w(end[1]),
                        w(end[0]),
                        w(start[1]),
                        w(start[0]),
                        500,
                        1000,
                        0,
                        0,
                    ],
                ),
            ]),
            [1000.0, 1000.0],
        )
    }

    /// Whether a point (drawing units) lies on the ellipse round (500, 250),
    /// 500 by 250, to the whole unit the points are cut to.
    fn on_the_ellipse([x, y]: [f64; 2]) -> bool {
        (((x - 500.0) / 500.0).hypot((y + 250.0) / 250.0) - 1.0).abs() < 0.01
    }

    #[test]
    fn an_arc_runs_counter_clockwise_from_its_start_to_its_end() {
        // One step of the arc is 1/99 turn, about 32 units on this ellipse.
        let step = 35.0;

        // From the top to the left: the upper left quarter.
        let picture = arc(ARC, [500, -100], [0, 250]);
        let drawn = lines(&picture);
        assert_eq!(drawn.len(), 1);
        assert!(!drawn[0].is_closed());
        assert_eq!(drawn[0].common.color, color([0, 0, 255]));
        let quarter = points(drawn[0]);
        assert!(quarter.len() > 20, "{} points", quarter.len());
        assert_near(quarter[0], [500.0, 0.0], step, "arc start");
        assert_near(*quarter.last().unwrap(), [0.0, -250.0], step, "arc end");
        for p in &quarter {
            assert!(on_the_ellipse(*p), "{p:?} is not on the ellipse");
            assert!(
                p[0] <= 500.0 && p[1] >= -250.0,
                "{p:?} is outside the quarter"
            );
        }
        // Counter-clockwise: leftwards along the top.
        assert!(quarter[1][0] < quarter[0][0]);

        // From the bottom past zero degrees to the top: the right half.
        let picture = arc(ARC, [500, 600], [500, -100]);
        let half = points(lines(&picture)[0]);
        assert_near(half[0], [500.0, -500.0], step, "arc start");
        assert_near(*half.last().unwrap(), [500.0, 0.0], step, "arc end");
        for p in &half {
            assert!(on_the_ellipse(*p), "{p:?} is not on the ellipse");
            assert!(p[0] >= 500.0, "{p:?} is outside the right half");
        }
        assert!(half.iter().any(|p| p[0] == 1000.0));
    }

    #[test]
    fn a_pie_is_its_arc_closed_through_the_centre_in_its_brush() {
        let picture = arc(PIE, [500, -100], [0, 250]);
        let drawn = lines(&picture);
        assert_eq!(drawn.len(), 1);
        let pie = drawn[0];
        assert!(pie.is_closed());
        assert_eq!(pie.common.color, color([0, 255, 0]));
        assert_eq!(pie.start_width, 1.0);
        let ring = points(pie);
        assert_eq!(ring[0], [500.0, -250.0]);
        // The rest is the arc of the same record.
        let quarter = points(lines(&arc(ARC, [500, -100], [0, 250]))[0]);
        assert_eq!(ring[1..], quarter[..]);
    }

    #[test]
    fn lines_run_on_from_the_current_point() {
        // MOVETO and LINETO take y, then x.
        let file = metafile(&[
            record(MOVETO, &[100, 100]),
            record(LINETO, &[100, 900]),
            record(LINETO, &[900, 900]),
        ]);
        let picture = picture(&file, [1000.0, 1000.0]);
        let segments: Vec<Vec<[f64; 2]>> = lines(&picture).into_iter().map(points).collect();
        assert_eq!(
            segments,
            [
                [[100.0, -100.0], [900.0, -100.0]],
                [[900.0, -100.0], [900.0, -900.0]],
            ]
        );
        // The default pen: black, one unit wide.
        assert!(lines(&picture)
            .iter()
            .all(|l| l.common.color == color([0, 0, 0]) && l.start_width == 1.0));
    }

    #[test]
    fn null_pens_and_brushes_leave_out_what_they_would_draw() {
        let triangle = record(POLYGON, &with_count(3, &TRIANGLE));
        let file = metafile(&[
            pen(5, 1, [0, 0, 0]),   // slot 0: no pen
            brush(1, [0, 0, 0]),    // slot 1: no brush
            pen(0, 3, [0, 0, 255]), // slot 2: blue
            brush(0, [255, 0, 0]),  // slot 3: red
            record(SELECTOBJECT, &[0]),
            record(SELECTOBJECT, &[1]),
            triangle.clone(),                            // nothing
            record(POLYLINE, &with_count(3, &TRIANGLE)), // nothing
            record(LINETO, &[500, 500]),                 // nothing, but moves
            record(SELECTOBJECT, &[3]),
            triangle.clone(), // red, no outline
            record(SELECTOBJECT, &[2]),
            record(SELECTOBJECT, &[1]),
            triangle,                // blue outline
            record(LINETO, &[0, 0]), // from (500, 500)
        ]);
        let picture = picture(&file, [1000.0, 1000.0]);
        let lines = lines(&picture);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].common.color, color([255, 0, 0]));
        assert_eq!(lines[0].start_width, 0.0);
        assert_eq!(lines[1].common.color, color([0, 0, 255]));
        assert_eq!(lines[1].start_width, 3.0);
        assert!(lines[0].is_closed() && lines[1].is_closed());
        assert_eq!(points(lines[2]), [[500.0, -500.0], [0.0, 0.0]]);
    }

    #[test]
    fn a_new_object_takes_the_first_free_slot() {
        let triangle = record(POLYGON, &with_count(3, &TRIANGLE));
        let file = metafile(&[
            record(CREATEPALETTE, &[0x0300, 0]), // slot 0
            pen(0, 1, [255, 0, 0]),              // slot 1
            brush(1, [0, 0, 0]),                 // slot 2: no brush
            record(SELECTOBJECT, &[2]),
            record(SELECTOBJECT, &[1]),
            triangle.clone(), // red
            record(DELETEOBJECT, &[0]),
            pen(0, 1, [0, 0, 255]), // slot 0, where the palette was
            record(SELECTOBJECT, &[0]),
            triangle.clone(), // blue
            record(DELETEOBJECT, &[1]),
            record(SELECTOBJECT, &[1]), // empty: the pen stays blue
            triangle,
        ]);
        let colors: Vec<Color> = lines(&picture(&file, [1000.0, 1000.0]))
            .iter()
            .map(|l| l.common.color)
            .collect();
        assert_eq!(
            colors,
            [color([255, 0, 0]), color([0, 0, 255]), color([0, 0, 255])]
        );
    }

    /// The baseline of a text whose point is at its top lies the font's
    /// ascent below it; the text is four fifths of the ascent tall.
    #[test]
    fn text_is_placed_by_the_alignment_its_record_has() {
        let file = metafile(&[
            // A fresh device context aligns text by its top.
            textout("Top", 100, 200),
            record(SETTEXTALIGN, &[24]),
            textout("Baseline", 100, 400),
            // EXTTEXTOUT: y, x, count, options, then the string, after a
            // rectangle when the options ask for one.
            exttextout(&[600, 100, 5, 0], "Plain"),
            exttextout(&[800, 100, 7, 4, 500, 50, 700, 900], "Clipped"),
        ]);
        let picture = picture(&file, [1000.0, 1000.0]);
        let texts = texts(&picture);
        let values: Vec<&str> = texts.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(values, ["Top", "Baseline", "Plain", "Clipped"]);
        // No font was selected: the device context's default face.
        assert_eq!(picture.fonts.len(), 1);
        assert!(texts.iter().all(|t| t.insertion_point.x == 100.0));
        let ascent = texts[0].height / 0.8;
        assert!(ascent > 0.0);
        assert!((texts[0].insertion_point.y - (-200.0 - ascent)).abs() < 1e-9);
        for (text, y) in texts[1..].iter().zip([400.0, 600.0, 800.0]) {
            assert_eq!(text.insertion_point.y, -y, "{}", text.value);
            assert_eq!(text.height, texts[0].height);
        }
    }

    #[test]
    fn the_window_sets_the_scale_and_the_placeable_bounds_stand_in_for_it() {
        let diagonal = [record(MOVETO, &[200, 100]), record(LINETO, &[400, 500])];
        let end = |file: &[u8], fit: [f64; 2]| points(lines(&picture(file, fit))[0]);

        // No window: the placeable bounds (left, top, right, bottom) give
        // its origin and size; 400 × 200 fitted to 100 × 100 is a quarter.
        let file = placeable([100, 200, 500, 400], &bare_metafile(&diagonal));
        assert_eq!(end(&file, [100.0, 100.0]), [[0.0, 0.0], [100.0, -50.0]]);

        // A window origin and size override the bounds; the narrower fit
        // sets the scale.
        let window = [
            record(SETWINDOWORG, &[0, 100]),
            record(SETWINDOWEXT, &[1000, 2000]),
        ];
        let file = placeable(
            [100, 200, 500, 400],
            &bare_metafile(&[&window[..], &diagonal].concat()),
        );
        assert_eq!(end(&file, [100.0, 100.0]), [[0.0, -10.0], [20.0, -20.0]]);

        // A window extent with y negative runs y up: no flip.
        let file = bare_metafile(
            &[
                &[record(SETWINDOWEXT, &[(-1000i16) as u16, 1000])][..],
                &diagonal,
            ]
            .concat(),
        );
        assert_eq!(
            end(&file, [1000.0, 1000.0]),
            [[100.0, 200.0], [500.0, 400.0]]
        );
    }

    #[test]
    fn what_is_not_a_metafile_gives_none() {
        let fit = [100.0, 100.0];
        assert!(read(&[], fit).is_none());
        assert!(read(b"not a metafile at all", fit).is_none());
        // A header of type 3.
        let mut wrong_type = metafile(&[]);
        wrong_type[0] = 3;
        assert!(read(&wrong_type, fit).is_none());
        // No window and no placeable bounds: no size to fit.
        assert!(read(&bare_metafile(&[]), fit).is_none());
        // A window of no width.
        assert!(read(&bare_metafile(&[record(SETWINDOWEXT, &[1000, 0])]), fit).is_none());
        // The smallest metafile there is.
        assert!(read(&metafile(&[]), fit).is_some_and(|p| p.entities.is_empty()));
    }

    /// Every record kind the reader plays, in one file.
    fn every_record() -> Vec<u8> {
        let w = |v: i16| v as u16;
        placeable(
            [0, 0, 1000, 1000],
            &metafile(&[
                record(SETWINDOWORG, &[0, 0]),
                record(CREATEPALETTE, &[0x0300, 0]),
                pen(0, 2, [255, 0, 0]),
                brush(0, [0, 255, 0]),
                record(
                    0x02FB,
                    &[w(-12), 0, 300, 0, 700, 1, 0, 0, 0, 0x7241, 0x6169, 0x006C],
                ),
                record(SELECTOBJECT, &[1]),
                record(SELECTOBJECT, &[2]),
                record(SELECTOBJECT, &[3]),
                record(0x0209, &[0x00FF, 0]),
                record(MOVETO, &[10, 10]),
                record(LINETO, &[20, 30]),
                record(POLYLINE, &with_count(3, &TRIANGLE)),
                record(POLYGON, &with_count(3, &TRIANGLE)),
                record(
                    POLYPOLYGON,
                    &[&[2, 3, 3][..], &TRIANGLE, &TRIANGLE].concat(),
                ),
                record(RECTANGLE, &[400, 300, 100, 100]),
                record(ROUNDRECT, &[50, 50, 400, 800, 100, 500]),
                record(ELLIPSE, &[900, 900, 600, 100]),
                record(ARC, &[250, 0, w(-100), 500, 500, 1000, 0, 0]),
                record(PIE, &[w(-100), 500, 600, 500, 500, 1000, 0, 0]),
                textout("Top", 100, 200),
                record(SETTEXTALIGN, &[6 | 8]),
                exttextout(&[800, 100, 7, 4, 500, 50, 700, 900], "Clipped"),
                record(DELETEOBJECT, &[1]),
            ]),
        )
    }

    /// Every truncation of every file, and every byte of each set to 0,
    /// to 0xFF and with its top bit flipped, reads without a panic.
    #[test]
    fn damaged_files_never_panic() {
        let mut cases = 0;
        let mut check = |bytes: &[u8]| {
            cases += 1;
            let _ = read(bytes, [100.0, 100.0]);
        };
        let every_record = every_record();
        for file in [SHAPES, RINGS, CURVES, TEXT, &every_record] {
            for end in 0..file.len() {
                check(&file[..end]);
            }
            let mut damaged = file.to_vec();
            for at in 0..file.len() {
                for value in [0x00, 0xFF, file[at] ^ 0x80] {
                    damaged[at] = value;
                    check(&damaged);
                }
                damaged[at] = file[at];
            }
        }
        assert!(cases > 25_000, "{cases} damaged files");
    }
}
