//! DGN V8 design files: an OLE compound file whose models (Dgn-Md/#n)
//! keep their graphic elements in a compressed stream (Dgn^G/$1), and whose
//! non-model stream (Dgn^Nm/$1) holds the colour table, the levels and the
//! shared cell definitions. Element fields are little-endian; coordinates
//! are UORs.

use std::collections::HashMap;
use std::io::Read;

use super::model::{
    arc_cubics, bspline_points, dgn_color_table, paths_bounds, scaled_elements, By, DgnCollector,
    DgnColor, DgnDimLook, DgnDimStyle, DgnElement, DgnGeom, DgnImport, DgnReference, Path,
    PathBuilder, Segment, Sheet, SubPath, Text, DGN_DEFAULT_COLORS,
};

// ── Compound file ───────────────────────────────────────────────────────────

struct Cfb<'a> {
    b: &'a [u8],
    sector: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini: Vec<u8>,
    cutoff: u32,
    entries: Vec<Entry>,
}

struct Entry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u32,
}

const END: u32 = 0xFFFF_FFFA;

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl<'a> Cfb<'a> {
    fn open(b: &'a [u8]) -> Option<Self> {
        // The 512-byte header, with the only two sector sizes the format
        // has (512 and 4096 bytes); anything else is not a compound file.
        if b.len() < 512 || !b.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
            return None;
        }
        let sector_shift = u16::from_le_bytes([b[30], b[31]]);
        if sector_shift != 9 && sector_shift != 12 {
            return None;
        }
        let sector = 1usize << sector_shift;
        let mini_sector_shift = u16::from_le_bytes([b[32], b[33]]);
        if mini_sector_shift != 6 {
            return None;
        }
        let mut difat: Vec<u32> = (0..109)
            .filter_map(|i| u32_at(b, 76 + i * 4))
            .filter(|&v| v < END)
            .collect();
        // The DIFAT chain cannot hold more sectors than the file does, so a
        // looping chain stops there.
        let (mut next, mut count) = (
            u32_at(b, 68)?,
            u32_at(b, 72)?.min((b.len() / sector) as u32),
        );
        while count > 0 && next < END {
            let at = (next as usize + 1) * sector;
            for i in 0..sector / 4 - 1 {
                if let Some(v) = u32_at(b, at + i * 4).filter(|&v| v < END) {
                    difat.push(v);
                }
            }
            next = u32_at(b, at + sector - 4)?;
            count -= 1;
        }
        let mut fat = Vec::new();
        for s in difat {
            let at = (s as usize + 1) * sector;
            for i in 0..sector / 4 {
                fat.push(u32_at(b, at + i * 4).unwrap_or(END));
            }
        }
        let mut cfb = Cfb {
            b,
            sector,
            fat,
            mini_fat: Vec::new(),
            mini: Vec::new(),
            cutoff: u32_at(b, 56)?,
            entries: Vec::new(),
        };
        let dir = cfb.chain(u32_at(b, 48)?, None);
        for e in dir.chunks_exact(128) {
            let len = u16::from_le_bytes([e[64], e[65]]) as usize;
            let name16: Vec<u16> = e[..len.saturating_sub(2).min(64)]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            cfb.entries.push(Entry {
                name: String::from_utf16_lossy(&name16),
                kind: e[66],
                left: u32_at(e, 68)?,
                right: u32_at(e, 72)?,
                child: u32_at(e, 76)?,
                start: u32_at(e, 116)?,
                size: u32_at(e, 120)?,
            });
        }
        let root = cfb.entries.first()?;
        let (root_start, root_size) = (root.start, root.size);
        cfb.mini = cfb.chain(root_start, Some(root_size as usize));
        let mini_fat = cfb.chain(u32_at(b, 60)?, None);
        cfb.mini_fat = mini_fat
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Some(cfb)
    }

    fn chain(&self, mut s: u32, size: Option<usize>) -> Vec<u8> {
        // A FAT entry can point at itself (`fat[s] = s`): the old fixed
        // 1_000_000-iteration guard still let every call accumulate
        // `1e6 × sector` bytes (4.1 GB at sector_shift = 12) — directory,
        // mini stream and miniFAT chains are alive at once (~12 GB peak →
        // allocation-failure abort). A chain can only read in-file sectors,
        // and after that many steps it must have repeated one; the walk is
        // deterministic, so a repeat means a cycle that yields nothing new.
        // Bounding by the file's own sector count never truncates a
        // legitimate chain (each sector read at most once).
        let max_sectors = self.b.len() / self.sector;
        let mut out = Vec::new();
        let mut guard = 0;
        while s < END && guard < max_sectors {
            let at = (s as usize + 1) * self.sector;
            let Some(block) = self.b.get(at..at + self.sector) else {
                break;
            };
            out.extend_from_slice(block);
            s = self.fat.get(s as usize).copied().unwrap_or(END);
            guard += 1;
        }
        if let Some(n) = size {
            out.truncate(n);
        }
        out
    }

    fn stream(&self, e: &Entry) -> Vec<u8> {
        if e.size >= self.cutoff {
            return self.chain(e.start, Some(e.size as usize));
        }
        let mut out = Vec::new();
        let mut s = e.start;
        let mut guard = 0;
        // Same cycle argument as `chain`, over the mini sectors: a loop in
        // the miniFAT used to spin the full 1_000_000-iteration budget
        // (64 MB per call) before `truncate` shortened the length.
        let max_sectors = self.mini.len() / 64;
        while s < END && guard < max_sectors {
            let at = s as usize * 64;
            let Some(block) = self.mini.get(at..at + 64) else {
                break;
            };
            out.extend_from_slice(block);
            s = self.mini_fat.get(s as usize).copied().unwrap_or(END);
            guard += 1;
        }
        out.truncate(e.size as usize);
        out
    }

    /// Every stream with its full path ("Dgn-Md/#000000/Dgn^G/$1").
    fn streams(&self) -> Vec<(String, &Entry)> {
        let mut out = Vec::new();
        fn walk<'b>(
            c: &'b Cfb,
            idx: u32,
            path: &str,
            out: &mut Vec<(String, &'b Entry)>,
            depth: usize,
        ) {
            let Some(e) = c.entries.get(idx as usize) else {
                return;
            };
            if depth > 64 {
                return;
            }
            walk(c, e.left, path, out, depth + 1);
            let p = if path.is_empty() {
                e.name.clone()
            } else {
                format!("{path}/{}", e.name)
            };
            if e.kind == 2 {
                out.push((p.clone(), e));
            } else if e.kind == 1 {
                walk(c, e.child, &p, out, depth + 1);
            }
            walk(c, e.right, path, out, depth + 1);
        }
        if let Some(root) = self.entries.first() {
            walk(self, root.child, "", &mut out, 0);
        }
        out
    }
}

/// Far above any real element stream; stops a crafted one from inflating
/// until memory runs out.
const STREAM_LIMIT: u64 = 512 << 20;

/// A DGN stream: raw, or zlib data after a 16-byte header.
fn inflate(data: &[u8]) -> Vec<u8> {
    inflate_at_most(data, STREAM_LIMIT)
}

/// [`inflate`] stopping after `limit` bytes.
fn inflate_at_most(data: &[u8], limit: u64) -> Vec<u8> {
    for skip in [16usize, 0] {
        if let Some(z) = data.get(skip..) {
            let mut out = Vec::new();
            if flate2::read::ZlibDecoder::new(z)
                .take(limit)
                .read_to_end(&mut out)
                .is_ok()
                && !out.is_empty()
            {
                return out;
            }
        }
    }
    data.to_vec()
}

// ── Elements ────────────────────────────────────────────────────────────────

/// Element records of an element stream (after its 4-byte count).
fn elements(d: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 4;
    while at + 8 <= d.len() {
        let Some(words) = u32_at(d, at + 4) else {
            break;
        };
        let len = 4 + words as usize * 2;
        if len < 8 {
            break;
        }
        // The stream can end inside its last element (its trailing words are
        // not stored); what is there is still read.
        let e = &d[at..(at + len).min(d.len())];
        if !is_deleted(e) {
            out.push(e);
        }
        at += len;
    }
    out
}

fn kind(e: &[u8]) -> u8 {
    e[0]
}

fn flags(e: &[u8]) -> u16 {
    u16::from_le_bytes([e[2], e[3]])
}

/// A deleted element (flag 0x80 of its second byte), kept in the stream but
/// no longer part of the file.
fn is_deleted(e: &[u8]) -> bool {
    e[1] & 0x80 != 0
}

/// A component of a complex element, cell or shared cell definition.
fn is_component(e: &[u8]) -> bool {
    flags(e) & 0x4000 != 0
}

fn f64_at(e: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(e.get(at..at + 8)?.try_into().ok()?))
}

/// The text style a text element uses: the id in its text style linkage
/// (the four bytes after `04 20 e6 04 00 00`).
fn text_style_id(e: &[u8]) -> Option<u32> {
    let at = e.windows(6).position(|w| w == [0x04, 0x20, 0xe6, 0x04, 0x00, 0x00])? + 6;
    u32_at(e, at)
}

/// The text style table (the table entries after a header of table 3):
/// each style's id and name.
fn text_style_table(nm: &[&[u8]]) -> Vec<(u32, String, String)> {
    let fonts = font_table(nm);
    let mut out: Vec<(u32, String, String)> = Vec::new();
    let mut table = 0;
    for e in nm {
        match kind(e) {
            96 => table = u32_at(e, 12).unwrap_or(0),
            95 if table == 3 => {
                if let (Some(id), Some(name)) = (u32_at(e, 32), marked_string(e, 32)) {
                    if !out.iter().any(|(i, ..)| *i == id) {
                        // The style's font: its number and kind at byte 40.
                        let font = e.get(40..42).and_then(|f| {
                            fonts.iter().find(|(n, k, _)| [*n, *k] == [f[0], f[1]]).map(|(_, k, name)| (*k, name))
                        });
                        out.push((id, name, font_file(font)));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The dimension style table (table 5), lengths in master units (`s` per
/// UOR). One style can have more than one entry. An entry holds its
/// element id (16), text style id (56), accuracy (72), flags (85: 0x10
/// text in the line, 0x20 horizontal text; 184: 0x01 a line between the
/// extension lines, 0x10 trailing zeros shown), text height (112),
/// extension line offset and extension (152, 160), text gap (360) and
/// terminator width (408) as text heights, stack offset (288), centre
/// mark (296), angle decimal places (307, plus one), its master and sub
/// units (numerator and denominator per metre at 520 and 544) and name
/// (after 104); the linkage `de 56` holds its scale 54 bytes in.
fn dim_style_table(nm: &[&[u8]], s: f64) -> Vec<DgnDimStyle> {
    // Text style id (32) → its font number and kind (40); the text style
    // table can follow the dimension style table.
    let mut fonts = HashMap::new();
    let mut table = 0;
    for e in nm {
        match kind(e) {
            96 => table = u32_at(e, 12).unwrap_or(0),
            95 if table == 3 => {
                if let (Some(id), Some(f)) = (u32_at(e, 32), e.get(40..42)) {
                    fonts.entry(id).or_insert((f[0], f[1]));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for e in nm {
        match kind(e) {
            96 => table = u32_at(e, 12).unwrap_or(0),
            95 if table == 5 => {
                let (Some(id), Some(name)) = (u32_at(e, 16), marked_string(e, 104)) else {
                    continue;
                };
                let real = |at: usize| f64_at(e, at).filter(|v| v.is_finite()).unwrap_or(0.0);
                let bit = |at: usize, mask: u8| e.get(at).is_some_and(|b| b & mask != 0);
                let text_height = real(112) * s;
                // Metres per unit: denominator over numerator.
                let metres = |at: usize| {
                    let n = real(at);
                    (n > 0.0).then(|| real(at + 8) / n)
                };
                let near = |m: Option<f64>, v: f64| m.is_some_and(|m| (m - v).abs() < 1e-9);
                let scale = (600..e.len().saturating_sub(4))
                    .find(|&k| e[k + 1] == 0x10 && e[k + 2] == 0xde && e[k + 3] == 0x56)
                    .and_then(|k| f64_at(e, k + 54))
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .unwrap_or(1.0);
                out.push(DgnDimStyle {
                    id,
                    name,
                    text_style: u32_at(e, 56).unwrap_or(0),
                    font: u32_at(e, 56).and_then(|t| fonts.get(&t).copied()).unwrap_or((0, 0)),
                    text_height,
                    arrow: real(408) * text_height,
                    ext_offset: real(152) * text_height,
                    ext_extension: real(160) * text_height,
                    gap: real(360) * text_height,
                    stack_offset: real(288) * s,
                    center: real(296) * s,
                    accuracy: e.get(72).copied().unwrap_or(0x84),
                    angle_places: e.get(307).map_or(0, |b| b.saturating_sub(1)),
                    trailing_zeros: bit(184, 0x10),
                    inline_text: bit(85, 0x10),
                    horizontal_text: bit(85, 0x20),
                    line_inside: bit(184, 0x01),
                    scale,
                    two_units: near(metres(520), 0.3048) && near(metres(544), 0.0254),
                });
            }
            _ => {}
        }
    }
    out
}

/// The shared cell definitions by element id (offset 16), named as their
/// type-34 header names them.
fn cell_ids(nm: &[&[u8]]) -> HashMap<u32, String> {
    nm.iter()
        .filter(|e| kind(e) == 34 && !is_component(e))
        .filter_map(|e| Some((u32_at(e, 16)?, marked_string(e, 248)?)))
        .collect()
}

/// The font table (table 2): each font's number, kind (2 SHX, 4 TrueType)
/// and name.
fn font_table(nm: &[&[u8]]) -> Vec<(u8, u8, String)> {
    let mut out = Vec::new();
    let mut table = 0;
    for e in nm {
        match kind(e) {
            96 => table = u32_at(e, 12).unwrap_or(0),
            95 if table == 2 => {
                let (Some(&number), Some(&kind), Some(len)) =
                    (e.get(40), e.get(41), e.get(44..46).map(|b| u16::from_le_bytes([b[0], b[1]])))
                else {
                    continue;
                };
                let units: Vec<u16> = e
                    .get(46..46 + usize::from(len))
                    .unwrap_or_default()
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                out.push((number, kind, String::from_utf16_lossy(&units).trim_end_matches(char::from(0)).to_string()));
            }
            _ => {}
        }
    }
    out
}

/// The font file a text style imports with: an SHX font's own file, a
/// TrueType family's installed file (else its name with `.ttf`), and the
/// `txt.shx` default for any other font.
fn font_file(font: Option<(u8, &String)>) -> String {
    match font {
        Some((2, name)) => format!("{name}.shx"),
        Some((4, name)) => crate::scene::text::sysfont::face_file(name, false, false)
            .unwrap_or_else(|| format!("{}.ttf", name.to_lowercase())),
        _ => "txt.shx".to_string(),
    }
}

/// A string stored after the "ff fe 01 00" marker, when the element has one.
fn marked_string(e: &[u8], from: usize) -> Option<String> {
    let i = e
        .get(from..)?
        .windows(4)
        .position(|w| w == [0xff, 0xfe, 0x01, 0x00])?
        + from
        + 4;
    let end = e[i..]
        .iter()
        .position(|&c| c == 0 || c < 0x20)
        .map(|n| i + n)
        .unwrap_or(e.len());
    Some(String::from_utf8_lossy(&e[i..end]).to_string())
}

struct Styles {
    colors: Vec<[u8; 3]>,
    /// Level id → colour index.
    levels: HashMap<u32, u32>,
    /// Levels the underlay turns off: their elements are left out.
    hidden: std::collections::HashSet<u32>,
    /// True colours, in the order colour words number them (1 first).
    extended: Vec<[u8; 3]>,
    /// Shared cell definition names by element id.
    cells: HashMap<u32, String>,
}

/// The true colours of the file: a zlib-packed UTF-16 record in the
/// non-model attributes stream reading
/// `<ExtendedColors><Entry Color="(r,g,b)"/>…</ExtendedColors>`.
fn extended_colors(cfb: &Cfb) -> Vec<[u8; 3]> {
    let Some(attrs) = cfb
        .streams()
        .iter()
        .find(|(p, _)| p == "Dgn^NmA/$1")
        .map(|(_, e)| inflate(&cfb.stream(e)))
    else {
        return Vec::new();
    };
    for at in 0..attrs.len().saturating_sub(2) {
        // A zlib header: deflate, and a check value divisible by 31.
        let (a, b) = (attrs[at], attrs[at + 1]);
        if a & 0x0f != 8 || ((a as u16) << 8 | b as u16) % 31 != 0 {
            continue;
        }
        // The record is a short XML list; the cap keeps a stream that
        // inflates without end (or a crafted one) from exhausting memory.
        const RECORD_LIMIT: u64 = 4 << 20;
        let mut out = Vec::new();
        if flate2::read::ZlibDecoder::new(&attrs[at..])
            .take(RECORD_LIMIT)
            .read_to_end(&mut out)
            .is_err()
            || out.len() < 2
        {
            continue;
        }
        let units: Vec<u16> = out
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16_lossy(&units);
        if !text.contains("<ExtendedColors") {
            continue;
        }
        return text
            .split("Color=\"(")
            .skip(1)
            .filter_map(|rest| {
                let v: Vec<u8> = rest
                    .split(')')
                    .next()?
                    .split(',')
                    .filter_map(|n| n.trim().parse().ok())
                    .collect();
                (v.len() == 3).then(|| [v[0], v[1], v[2]])
            })
            .collect();
    }
    Vec::new()
}

impl Styles {
    fn color(&self, e: &[u8]) -> [u8; 3] {
        // The colour word: an index into the colour table, 0xFFFFFFFF for
        // the level's colour, or true colour n (from 1) × 256 plus the index
        // of its nearest table colour, drawn in its own RGB when the file
        // lists it.
        let explicit = u32_at(e, 52).unwrap_or(u32::MAX);
        let index = if explicit == u32::MAX {
            self.levels
                .get(&u32_at(e, 12).unwrap_or(0))
                .copied()
                .unwrap_or(0)
        } else {
            if let Some(rgb) = (explicit >> 8)
                .checked_sub(1)
                .and_then(|n| self.extended.get(n as usize))
            {
                return *rgb;
            }
            explicit & 0xff
        };
        self.colors
            .get(index as usize)
            .copied()
            .unwrap_or([255, 255, 255])
    }

    /// The colour word as an element's colour: the level's (0xFFFFFFFF),
    /// the enclosing cell's (0xFFFFFFFE), or a table index (low byte) with
    /// the table entry and the colour drawn.
    fn color_word(&self, e: &[u8]) -> By<DgnColor> {
        match u32_at(e, 52) {
            None | Some(u32::MAX) => By::Level,
            Some(0xFFFF_FFFE) => By::Cell,
            Some(word) => {
                let index = word & 0xff;
                By::Own(DgnColor {
                    index,
                    table: self
                        .colors
                        .get(index as usize)
                        .copied()
                        .unwrap_or([255, 255, 255]),
                    rgb: self.color(e),
                })
            }
        }
    }
}

/// A B-spline curve from its order and poles, with uniform knots. A closed
/// curve (its last pole repeating the first) is periodic: its distinct
/// poles wrap around with knots (i + 1) / (count + 1), and the curve is
/// clamped at both ends of its domain by knot insertion.
fn bspline_geom(order: usize, poles: Vec<[f64; 3]>, closed: bool) -> DgnGeom {
    let degree = order - 1;
    let distinct = poles.len() - usize::from(closed && poles.first() == poles.last());
    if !closed || distinct < order {
        let interior = poles.len() - order;
        let mut knots = vec![0.0; order];
        knots.extend((1..=interior).map(|k| k as f64 / (interior + 1) as f64));
        knots.extend(vec![1.0; order]);
        return DgnGeom::Spline {
            degree,
            knots,
            poles,
            closed,
        };
    }
    let mut ctrl: Vec<[f64; 3]> = (0..distinct + degree).map(|i| poles[i % distinct]).collect();
    let count = ctrl.len() + order;
    let mut knots: Vec<f64> = (0..count).map(|i| (i + 1) as f64 / (count + 1) as f64).collect();
    let (lo, hi) = (knots[degree], knots[ctrl.len()]);
    let same = |a: f64, b: f64| (a - b).abs() < 1e-12;
    // Knot insertion (Boehm) of t until it is a degree-fold knot.
    let mut insert = |t: f64| loop {
        let mult = knots.iter().filter(|k| same(**k, t)).count();
        if mult >= degree {
            return;
        }
        let Some(k) = knots.iter().rposition(|x| *x <= t + 1e-12) else {
            return;
        };
        let mut next = Vec::with_capacity(ctrl.len() + 1);
        for i in 0..=ctrl.len() {
            if i + degree <= k {
                next.push(ctrl[i]);
            } else if i + mult > k {
                next.push(ctrl[i - 1]);
            } else {
                let a = (t - knots[i]) / (knots[i + degree] - knots[i]);
                next.push(std::array::from_fn(|c| (1.0 - a) * ctrl[i - 1][c] + a * ctrl[i][c]));
            }
        }
        ctrl = next;
        knots.insert(k + 1, t);
    };
    insert(lo);
    insert(hi);
    // The domain clamped: an order-fold knot at each end, the poles that
    // span it.
    let first = knots.iter().position(|k| same(*k, lo)).unwrap_or(1).max(1);
    let last = knots.iter().rposition(|k| same(*k, hi)).unwrap_or(knots.len() - 1);
    let mut clamped = vec![lo];
    clamped.extend_from_slice(&knots[first..=last]);
    clamped.push(hi);
    let poles: Vec<[f64; 3]> = ctrl
        .iter()
        .skip(first - 1)
        .take(clamped.len() - order)
        .copied()
        .collect();
    let knots = clamped;
    DgnGeom::Spline {
        degree,
        knots,
        poles,
        closed: true,
    }
}

/// Plan geometry in UORs, and the shared cells it places.
#[derive(Default, Clone)]
struct Raw {
    paths: Vec<Path>,
    texts: Vec<Text>,
    instances: Vec<Instance>,
    /// The same run element by element, for DGNIMPORT.
    elements: Vec<DgnElement>,
}

#[derive(Clone)]
struct Instance {
    name: String,
    /// The definition's placement, with every enclosing one applied.
    xf: Xf,
}

/// A placement in UORs: x' = r[0]·(x, y, z, 1), y' = r[1]·…, z' = r[2]·….
type Xf = [[f64; 4]; 3];

const IDENTITY: Xf = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

fn xf_vector(m: &Xf, v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

fn xf_point(m: &Xf, p: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3])
}

fn xf_compose(outer: &Xf, inner: &Xf) -> Xf {
    let mut out = [[0.0; 4]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| outer[i][k] * inner[k][j]).sum::<f64>()
                + if j == 3 { outer[i][3] } else { 0.0 };
        }
    }
    out
}

/// A 3D element (flag 0x0800 of its properties word): three coordinates a
/// point, orientations as quaternions. Its plan view is drawn.
fn is_3d(e: &[u8]) -> bool {
    u32_at(e, 40).is_some_and(|v| v & 0x0800 != 0)
}

/// The in-plane x and y axes of a 3D element's orientation quaternion w, x,
/// y, z stored at `at` (the rows of its rotation matrix).
fn quat_axes(e: &[u8], at: usize) -> Option<([f64; 3], [f64; 3])> {
    let (w, x, y, z) = (
        f64_at(e, at)?,
        f64_at(e, at + 8)?,
        f64_at(e, at + 16)?,
        f64_at(e, at + 24)?,
    );
    Some((
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
    ))
}

/// The in-plane axes of a 2D element turned by `rotation` (radians).
fn turned_axes(rotation: f64) -> ([f64; 3], [f64; 3]) {
    let (c, s) = (rotation.cos(), rotation.sin());
    ([c, s, 0.0], [-s, c, 0.0])
}

/// An elliptical arc on axes `u` and `v` (plan projections) as cubics.
fn projected_arc(
    c: [f64; 2],
    a: f64,
    b: f64,
    u: [f64; 2],
    v: [f64; 2],
    start: f64,
    sweep: f64,
) -> Vec<Segment> {
    let map = |p: [f64; 2]| {
        [
            c[0] + a * p[0] * u[0] + b * p[1] * v[0],
            c[1] + a * p[0] * u[1] + b * p[1] * v[1],
        ]
    };
    arc_cubics([0.0, 0.0], 1.0, 1.0, 0.0, start, sweep)
        .into_iter()
        .map(|[p0, p1, p2, p3]| Segment::Cubic(map(p0), map(p1), map(p2), map(p3)))
        .collect()
}

/// Reads a run of elements placed through `xf` into plan geometry.
fn read_run(run: &[&[u8]], styles: &Styles, xf: &Xf) -> Raw {
    let mut raw = Raw::default();
    // A complex shape or chain collects its components into one path.
    let mut complex: Option<(
        usize,
        PathBuilder,
        Option<([u8; 3], f64)>,
        Option<[u8; 3]>,
        bool,
    )> = None;
    fn push(
        raw: &mut Raw,
        complex: &mut Option<(
            usize,
            PathBuilder,
            Option<([u8; 3], f64)>,
            Option<[u8; 3]>,
            bool,
        )>,
        path: Path,
    ) {
        if let Some((left, pb, _, _, _)) = complex.as_mut() {
            for sp in &path.subpaths {
                for seg in &sp.segments {
                    let (a, b) = match seg {
                        Segment::Line(a, b) | Segment::Cubic(a, _, _, b) => (*a, *b),
                    };
                    if pb.current() != Some(a) {
                        if pb.current().is_none() {
                            pb.move_to(a)
                        } else {
                            pb.line_to(a)
                        }
                    }
                    match seg {
                        Segment::Line(..) => pb.line_to(b),
                        Segment::Cubic(_, c1, c2, _) => pb.cubic_to(*c1, *c2, b),
                    }
                }
            }
            *left = left.saturating_sub(1);
            if *left == 0 {
                let (_, mut pb, stroke, fill, closed) = complex.take().unwrap();
                if closed {
                    pb.close();
                }
                raw.paths.push(Path {
                    subpaths: pb.finish(),
                    stroke,
                    fill,
                });
            }
        } else {
            raw.paths.push(path);
        }
    }
    let plan = |p: [f64; 3]| {
        let q = xf_point(xf, p);
        [q[0], q[1]]
    };
    let axis = |v: [f64; 3]| {
        let q = xf_vector(xf, v);
        [q[0], q[1]]
    };
    let mut collect = DgnCollector::default();
    // An open B-spline curve: order, pole count, closure and its header.
    let mut spline: Option<(usize, usize, bool, DgnElement)> = None;
    for (index, e) in run.iter().enumerate() {
        // An element that is no component ends the complex elements still
        // open.
        if !is_component(e) {
            collect.close();
        }
        if e.len() < 104 || styles.hidden.contains(&u32_at(e, 12).unwrap_or(0)) {
            continue;
        }
        let color = styles.color(e);
        // Colour and weight words: 0xFFFFFFFF takes the level's, 0xFFFFFFFE
        // the enclosing cell's; line style 0x7FFFFFFF (or 0xFFFFFFFF) the
        // level's, 0x7FFFFFFE the cell's.
        let element = |geom: DgnGeom| DgnElement {
            level: u32_at(e, 12).unwrap_or(0),
            color: styles.color_word(e),
            weight: match u32_at(e, 48) {
                None | Some(u32::MAX) => By::Level,
                Some(0xFFFF_FFFE) => By::Cell,
                Some(w) => By::Own(w & 31),
            },
            style: match u32_at(e, 44) {
                None | Some(u32::MAX) | Some(0x7FFF_FFFF) => By::Level,
                Some(0xFFFF_FFFE) | Some(0x7FFF_FFFE) => By::Cell,
                Some(s) => By::Own(s),
            },
            geom,
            filled: matches!(kind(e), 6 | 14) && e.windows(4).any(|w| w == [0x07, 0x10, 0x41, 0x00]),
            construction: false,
        };
        let stroke = Some((color, -1.0));
        let d3 = is_3d(e);
        // A point: x, y (and z in a 3D element).
        let p3 = |at: usize| -> Option<[f64; 3]> {
            Some([
                f64_at(e, at)?,
                f64_at(e, at + 8)?,
                if d3 { f64_at(e, at + 16)? } else { 0.0 },
            ])
        };
        // A point or vector placed in the model (3D kept for the import).
        let at3 = |p: [f64; 3]| xf_point(xf, p);
        let dir3 = |v: [f64; 3]| xf_vector(xf, v);
        match kind(e) {
            // A text node: grouped when its count is followed by that many
            // text components.
            7 => {
                let n = u32_at(e, 104).unwrap_or(0) as usize;
                let texts = run.get(index + 1..index + 1 + n).is_some_and(|parts| {
                    parts.iter().all(|p| p.len() >= 104 && kind(p) == 17 && is_component(p))
                });
                if texts {
                    // 2D: justification 118, origin 152 (the node's
                    // justification point).
                    let origin = (!d3).then(|| p3(152)).flatten().map(at3);
                    collect.open(
                        n,
                        element(DgnGeom::TextNode {
                            lines: Vec::new(),
                            origin,
                            just: if d3 { 0 } else { e.get(118).copied().unwrap_or(0) },
                            line_spacing: None,
                        }),
                    );
                }
            }
            // A cell (not shared): its components follow, flagged as such.
            2 => {
                let n = u32_at(e, 104).unwrap_or(0) as usize;
                let parts = run
                    .get(index + 1..index + 1 + n)
                    .is_some_and(|parts| parts.iter().all(|p| is_component(p)));
                if parts && n > 0 {
                    collect.open(
                        n,
                        // The origin follows the transformation: 176
                        // (232 in 3D).
                        element(DgnGeom::CellGroup {
                            name: marked_string(e, 104).unwrap_or_default(),
                            origin: p3(if d3 { 232 } else { 176 }).map(at3).unwrap_or([0.0; 3]),
                            parts: Vec::new(),
                        }),
                    );
                }
            }
            12 | 14 => {
                let n = u32_at(e, 104).unwrap_or(0) as usize;
                collect.open(n, element(DgnGeom::Chain(Vec::new())));
                if n > 0 {
                    let fill = (kind(e) == 14
                        && e.windows(4).any(|w| w == [0x07, 0x10, 0x41, 0x00]))
                    .then_some(color);
                    complex = Some((n, PathBuilder::default(), stroke, fill, kind(e) == 14));
                }
            }
            3 => {
                if let (Some(a), Some(b)) = (p3(104), p3(if d3 { 128 } else { 120 })) {
                    collect.push(element(DgnGeom::Line(at3(a), at3(b))));
                    push(
                        &mut raw,
                        &mut complex,
                        Path {
                            subpaths: vec![SubPath {
                                segments: vec![Segment::Line(plan(a), plan(b))],
                                closed: false,
                            }],
                            stroke,
                            fill: None,
                        },
                    );
                }
            }
            4 | 6 | 11 => {
                let n = u32_at(e, 104).unwrap_or(0) as usize;
                let stride = if d3 { 24 } else { 16 };
                let pts3: Vec<[f64; 3]> = (0..n).filter_map(|k| p3(112 + k * stride)).collect();
                let pts: Vec<[f64; 2]> = pts3.iter().map(|p| plan(*p)).collect();
                if pts.len() >= 2 {
                    collect.push(element(DgnGeom::Points(pts3.iter().map(|p| at3(*p)).collect())));
                    let mut pb = PathBuilder::default();
                    pb.move_to(pts[0]);
                    for p in &pts[1..] {
                        pb.line_to(*p);
                    }
                    if kind(e) == 6 {
                        pb.close();
                    }
                    push(
                        &mut raw,
                        &mut complex,
                        Path {
                            subpaths: pb.finish(),
                            stroke,
                            fill: None,
                        },
                    );
                }
            }
            15 | 16 => {
                // Radii, orientation and centre. 3D: ellipse a 104, b 112,
                // quaternion 120, centre 152; arc start 104, sweep 112, a 120,
                // b 128, quaternion 136, centre 168. 2D: the orientation is an
                // angle, ellipse centre 128, arc centre 144.
                let arc = match (kind(e), d3) {
                    (15, true) => (|| {
                        Some((
                            f64_at(e, 104)?,
                            f64_at(e, 112)?,
                            quat_axes(e, 120)?,
                            p3(152)?,
                            0.0,
                            0.0,
                        ))
                    })(),
                    (_, true) => (|| {
                        Some((
                            f64_at(e, 120)?,
                            f64_at(e, 128)?,
                            quat_axes(e, 136)?,
                            p3(168)?,
                            f64_at(e, 104)?,
                            f64_at(e, 112)?,
                        ))
                    })(),
                    (15, false) => (|| {
                        Some((
                            f64_at(e, 104)?,
                            f64_at(e, 112)?,
                            turned_axes(f64_at(e, 120)?),
                            p3(128)?,
                            0.0,
                            0.0,
                        ))
                    })(),
                    _ => (|| {
                        Some((
                            f64_at(e, 120)?,
                            f64_at(e, 128)?,
                            turned_axes(f64_at(e, 136)?),
                            p3(144)?,
                            f64_at(e, 104)?,
                            f64_at(e, 112)?,
                        ))
                    })(),
                };
                let Some((a, b, (u, v), c, start, sweep)) = arc else {
                    continue;
                };
                let sweep = if sweep == 0.0 {
                    std::f64::consts::TAU
                } else {
                    sweep
                };
                let closed = sweep.abs() >= std::f64::consts::TAU - 1e-9;
                {
                    let (du, dv) = (dir3(u), dir3(v));
                    collect.push(element(DgnGeom::Arc {
                        center: at3(c),
                        u: du.map(|x| a * x),
                        v: dv.map(|x| b * x),
                        start,
                        sweep,
                    }));
                }
                push(
                    &mut raw,
                    &mut complex,
                    Path {
                        subpaths: vec![SubPath {
                            segments: projected_arc(plan(c), a, b, axis(u), axis(v), start, sweep),
                            closed,
                        }],
                        stroke,
                        fill: None,
                    },
                );
            }
            17 => {
                // 3D: width 112, height 120, quaternion 144, origin 176, the
                // characters after the string marker, as many as the byte
                // count at 110 holds. 2D: angle 144, origin
                // 152, a byte count (with its 4-byte marker) at 110 and the
                // characters from 174.
                let placed = if d3 {
                    (|| {
                        Some((
                            f64_at(e, 112)?,
                            f64_at(e, 120)?,
                            quat_axes(e, 144)?,
                            p3(176)?,
                            // The byte count at 110 (with the 4-byte marker)
                            // ends the characters.
                            {
                                let count =
                                    u16::from_le_bytes(e.get(110..112)?.try_into().ok()?) as usize;
                                let mark = e
                                    .get(200..)?
                                    .windows(4)
                                    .position(|w| w == [0xff, 0xfe, 0x01, 0x00])?
                                    + 200;
                                marked_string(&e[..(mark + count.max(4)).min(e.len())], mark)?
                            },
                        ))
                    })()
                } else {
                    (|| {
                        let count = u16::from_le_bytes(e.get(110..112)?.try_into().ok()?) as usize;
                        let chars = e.get(174..170 + count.max(4))?;
                        let text = String::from_utf8_lossy(chars)
                            .trim_end_matches(char::from(0))
                            .to_string();
                        Some((
                            f64_at(e, 112)?,
                            f64_at(e, 120)?,
                            turned_axes(f64_at(e, 144)?),
                            p3(152)?,
                            text,
                        ))
                    })()
                };
                let Some((width, height, (u, v), origin, text)) = placed else {
                    continue;
                };
                // 2D: the justification (108) and the length of the text box
                // along the baseline (128).
                let (just, length) = if d3 {
                    (2, 0.0)
                } else {
                    (
                        e.get(108).copied().unwrap_or(2),
                        f64_at(e, 128).filter(|l| l.is_finite()).unwrap_or(0.0),
                    )
                };
                // The text's axes as the plan shows them: its baseline sets the
                // angle, and each axis's plan length scales its size.
                let (pu, pv) = (axis(u), axis(v));
                let (su, sv) = (pu[0].hypot(pu[1]), pv[0].hypot(pv[1]));
                if su < 1e-9 || sv < 1e-9 {
                    continue;
                }
                let (height, width) = (height * 6.0 / 1000.0 * sv, width * 6.0 / 1000.0 * su);
                let width_factor = if height > 0.0 { width / height } else { 1.0 };
                collect.push(element(DgnGeom::Text {
                    text: text.clone(),
                    origin: at3(origin),
                    height,
                    width_factor,
                    length: length * su,
                    rotation: pu[1].atan2(pu[0]),
                    just,
                    font: None,
                    style: text_style_id(e),
                }));
                raw.texts.push(Text {
                    text,
                    origin: plan(origin),
                    height,
                    width_factor,
                    rotation: pu[1].atan2(pu[0]),
                    color,
                    font: "txt".to_string(),
                });
            }
            // A dimension: its type (133), point count (135), axes (the
            // columns of a 2×2 matrix by rows from 272; in 3D a quaternion
            // there, its first two rows) and points from 304, 48 bytes
            // apart, the first point's +24 holding the dimension line's
            // offset. It is drawn by the shared cell its linkage names (the
            // id after `13 27 07 00 00 03 01 00`). A note (53) is a leader
            // through its points.
            33 => {
                let cell = e
                    .windows(8)
                    .position(|w| w == [0x13, 0x27, 0x07, 0x00, 0x00, 0x03, 0x01, 0x00])
                    .and_then(|at| u32_at(e, at + 8))
                    .and_then(|id| styles.cells.get(&id));
                let dim = e.get(133).copied().unwrap_or(0);
                let count = e.get(135).copied().unwrap_or(0) as usize;
                let points: Option<Vec<[f64; 3]>> =
                    (0..count).map(|k| p3(304 + 48 * k).map(at3)).collect();
                let style = u32_at(e, 104).unwrap_or(0);
                // Only a dimension in the XY plane comes in as a dimension.
                let axes = if d3 {
                    quat_axes(e, 272).filter(|(x, y)| x[2].abs() < 1e-9 && y[2].abs() < 1e-9)
                } else {
                    (0..4)
                        .map(|k| f64_at(e, 272 + 8 * k))
                        .collect::<Option<Vec<f64>>>()
                        .map(|m| ([m[0], m[2], 0.0], [m[1], m[3], 0.0]))
                };
                match (points, axes) {
                    (Some(points), _) if dim == 53 && points.len() >= 2 => {
                        collect.push(element(DgnGeom::Leader { points, style }));
                    }
                    (Some(points), Some((x, y)))
                        if matches!(dim, 1 | 5 | 11 | 12) && points.len() >= 2 =>
                    {
                        let x = dir3(x);
                        let y = dir3(y);
                        // Its own settings: text height (152), font number
                        // and kind (168), text off the line (180, 0x40),
                        // extension line offset and extension (192, 200),
                        // text gap and margin (208, 216), terminator width
                        // (232).
                        let real = |at: usize| f64_at(e, at).unwrap_or(0.0);
                        let byte = |at: usize| e.get(at).copied().unwrap_or(0);
                        let look = DgnDimLook {
                            font: (byte(168), byte(169)),
                            text_height: real(152),
                            ext_offset: real(192),
                            ext_extension: real(200),
                            gap: real(208),
                            margin: real(216),
                            arrow: real(232),
                            text_outside: byte(180) & 0x40 != 0,
                        };
                        collect.push(element(DgnGeom::Dimension {
                            kind: dim,
                            points,
                            offset: f64_at(e, 328).unwrap_or(0.0),
                            axes: [[x[0], x[1]], [y[0], y[1]]],
                            style,
                            look,
                        }));
                    }
                    // Otherwise the dimension comes in as its drawing.
                    _ => {
                        if let Some(name) = cell {
                            collect.push(element(DgnGeom::Cell {
                                name: name.clone(),
                                m: [1.0, 0.0, 0.0, 1.0],
                                origin: [0.0; 3],
                                z_scale: 1.0,
                            }));
                        }
                    }
                }
                if let Some(name) = cell {
                    raw.instances.push(Instance {
                        name: name.to_ascii_uppercase(),
                        xf: *xf,
                    });
                }
            }
            // A mesh: its component count (104), its style (108, 1 face
            // loops) and its matrices, each a header (101; what it holds at
            // 116: 1 point numbers, 2 coordinates) and the data after it
            // (102 integers, 103 doubles: the count at 104, the values from
            // 120).
            105 if u32_at(e, 108) == Some(1) => {
                let n = u32_at(e, 104).unwrap_or(0) as usize;
                let parts = &run[index + 1..(index + 1 + n).min(run.len())];
                // A large matrix does not fit one element: its values run on
                // through every data element up to the next header.
                let data = |family: u32, data_kind: u8, size: usize| {
                    let header = parts
                        .iter()
                        .position(|&p| kind(p) == 101 && u32_at(p, 116) == Some(family))?;
                    let mut values = Vec::new();
                    for &d in parts[header + 1..].iter().take_while(|&&p| kind(p) == data_kind) {
                        let count = u32_at(d, 104)? as usize;
                        values.extend(d.get(120..)?.chunks_exact(size).take(count));
                    }
                    (!values.is_empty()).then_some(values)
                };
                let (Some(numbers), Some(coords)) = (data(1, 102, 4), data(2, 103, 8)) else {
                    continue;
                };
                let coords: Vec<f64> = coords
                    .iter()
                    .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                    .collect();
                let points: Vec<[f64; 3]> =
                    coords.chunks_exact(3).map(|c| at3([c[0], c[1], c[2]])).collect();
                let numbers: Vec<i32> = numbers
                    .iter()
                    .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                // A loop ends at a 0; one naming a point the mesh lacks is
                // left out.
                let loops: Vec<Vec<i32>> = numbers
                    .split(|&k| k == 0)
                    .filter(|l| {
                        l.len() >= 3
                            && l.iter().all(|k| (k.unsigned_abs() as usize) <= points.len())
                    })
                    .map(|l| l.to_vec())
                    .collect();
                if !loops.is_empty() {
                    collect.push(element(DgnGeom::Mesh { points, loops }));
                }
            }
            35 => {
                // Shared cell instance: a 3×3 matrix by rows from 160, the
                // origin at 232 (z at 248 in 3D), the definition's name after
                // the string marker.
                let m: Option<Vec<f64>> = (0..9).map(|k| f64_at(e, 160 + k * 8)).collect();
                let (Some(m), Some(o)) = (m, p3(232)) else {
                    continue;
                };
                let Some(name) = marked_string(e, 248) else {
                    continue;
                };
                let local: Xf = [
                    [m[0], m[1], m[2], o[0]],
                    [m[3], m[4], m[5], o[1]],
                    [m[6], m[7], m[8], o[2]],
                ];
                collect.push(element(DgnGeom::Cell {
                    name: name.clone(),
                    m: [m[0], m[1], m[3], m[4]],
                    origin: o,
                    z_scale: m[8],
                }));
                raw.instances.push(Instance {
                    name: name.to_ascii_uppercase(),
                    xf: xf_compose(xf, &local),
                });
            }
            27 => {
                // B-spline curve header: order and closure in the word at
                // 108, pole count 112; its poles follow (type 21).
                let Some(&[lo, hi]) = e.get(108..110) else {
                    continue;
                };
                let flags = u16::from_le_bytes([lo, hi]);
                // A curve before it that never got its poles still takes
                // its place in an open cell.
                let unread = spline.replace((
                    ((flags & 0x0f) + 2) as usize,
                    u32_at(e, 112).unwrap_or(0) as usize,
                    flags & 0x0180 != 0,
                    element(DgnGeom::Chain(Vec::new())),
                ));
                if unread.is_some() {
                    collect.skip();
                }
            }
            21 => {
                let n = u32_at(e, 104).unwrap_or(0) as usize;
                let stride = if d3 { 24 } else { 16 };
                let poles: Vec<[f64; 3]> = (0..n).filter_map(|k| p3(112 + k * stride)).collect();
                if let Some((order, count, closed, header)) = spline.take() {
                    if poles.len() == count && count >= order && order >= 2 {
                        let poles: Vec<[f64; 3]> = poles.iter().map(|p| at3(*p)).collect();
                        let geom = bspline_geom(order, poles, closed);
                        if let DgnGeom::Spline { degree, knots, poles, .. } = &geom {
                            let plan_poles: Vec<[f64; 2]> =
                                poles.iter().map(|p| [p[0], p[1]]).collect();
                            let pts = bspline_points(degree + 1, &plan_poles, knots, 24 * count);
                            let mut pb = PathBuilder::default();
                            pb.move_to(pts[0]);
                            for p in &pts[1..] {
                                pb.line_to(*p);
                            }
                            push(
                                &mut raw,
                                &mut complex,
                                Path {
                                    subpaths: pb.finish(),
                                    stroke,
                                    fill: None,
                                },
                            );
                        }
                        collect.push(DgnElement { geom, ..header });
                    } else {
                        collect.skip();
                    }
                }
            }
            _ => {}
        }
    }
    // A complex element cut short keeps the components it has.
    if let Some((_, mut pb, stroke, fill, closed)) = complex.take() {
        if closed {
            pb.close();
        }
        raw.paths.push(Path {
            subpaths: pb.finish(),
            stroke,
            fill,
        });
    }
    collect.close();
    raw.elements = collect.elements;
    raw
}

type Affine = [f64; 6];

fn apply(m: &Affine, p: [f64; 2]) -> [f64; 2] {
    [
        m[0] * p[0] + m[1] * p[1] + m[4],
        m[2] * p[0] + m[3] * p[1] + m[5],
    ]
}

/// Places plan geometry through the sheet mapping `m`, and the shared cells
/// it instances (each definition read through its own placement).
fn place(
    raw: &Raw,
    m: &Affine,
    defs: &HashMap<String, Vec<&[u8]>>,
    styles: &Styles,
    depth: usize,
    out: &mut Sheet,
) {
    let map = |p: [f64; 2]| apply(m, p);
    for path in &raw.paths {
        out.paths.push(Path {
            subpaths: path
                .subpaths
                .iter()
                .map(|sp| SubPath {
                    segments: sp
                        .segments
                        .iter()
                        .map(|s| match s {
                            Segment::Line(a, b) => Segment::Line(map(*a), map(*b)),
                            Segment::Cubic(a, b, c, d) => {
                                Segment::Cubic(map(*a), map(*b), map(*c), map(*d))
                            }
                        })
                        .collect(),
                    closed: sp.closed,
                })
                .collect(),
            stroke: path.stroke,
            fill: path.fill,
        });
    }
    let scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
    let turn = m[2].atan2(m[0]);
    for t in &raw.texts {
        out.texts.push(Text {
            origin: map(t.origin),
            height: t.height * scale,
            rotation: t.rotation + turn,
            ..t.clone()
        });
    }
    if depth >= 8 {
        return;
    }
    for inst in &raw.instances {
        let Some(def) = defs.get(&inst.name) else {
            continue;
        };
        place(
            &read_run(def, styles, &inst.xf),
            m,
            defs,
            styles,
            depth + 1,
            out,
        );
    }
}

/// Units of a model header: UORs per master and per sub unit.
fn units(header: &[u8]) -> (f64, f64) {
    // Unit definitions are numerator/denominator pairs per metre: master
    // (4180/4188), sub (4196/4204), storage (4332/4340), and the UORs per
    // storage unit (4324).
    let get = |at: usize| f64_at(header, at).filter(|v| v.is_finite() && *v > 0.0);
    let (mn, md, sn, sd) = (get(4180), get(4188), get(4196), get(4204));
    let (uor, stn, std) = (get(4324), get(4332), get(4340));
    let uor_per_metre = match (uor, stn, std) {
        (Some(u), Some(n), Some(d)) => u * n / d,
        _ => 1_000_000.0,
    };
    let master = match (mn, md) {
        (Some(n), Some(d)) => uor_per_metre * d / n,
        _ => uor_per_metre,
    };
    let sub = match (sn, sd) {
        (Some(n), Some(d)) => uor_per_metre * d / n,
        _ => master / 1000.0,
    };
    (master, sub)
}

/// How many bytes the streams of one file may inflate to in all.
fn inflate_budget(cfb: &Cfb) -> usize {
    cfb.b.len().saturating_mul(64).max(256 << 20)
}

/// Models of the file: (name, graphic stream, header stream).
fn models(cfb: &Cfb) -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let streams = cfb.streams();
    let mut dirs: Vec<String> = streams
        .iter()
        .filter_map(|(p, _)| p.strip_suffix("/Dgn^G/$1").map(|d| d.to_string()))
        .collect();
    dirs.sort();
    // Each stream may inflate to `STREAM_LIMIT` (512 MB) and every model
    // dir inflates its own — entries can all point at the same payload, so
    // N dirs × 512 MB was reachable from one small stream. The budget
    // scales with the file (×64 covers any real compression ratio, so
    // legitimate files never trip it) with a 256 MiB floor for small ones;
    // models past the budget are dropped, the earlier ones still load.
    let total_limit = inflate_budget(cfb);
    let mut total = 0usize;
    let mut out = Vec::new();
    for dir in dirs {
        if total >= total_limit {
            break;
        }
        let get = |suffix: &str| {
            streams
                .iter()
                .find(|(p, _)| *p == format!("{dir}/{suffix}"))
                .map(|(_, e)| inflate(&cfb.stream(e)))
                .unwrap_or_default()
        };
        let graphics = get("Dgn^G/$1");
        let header = get("Dgn~Mh");
        total = total
            .saturating_add(graphics.len())
            .saturating_add(header.len());
        let name = marked_string(&header, 0).unwrap_or_else(|| "Default".to_string());
        out.push((name, graphics, header));
    }
    out
}

pub fn model_names(bytes: &[u8]) -> Option<Vec<String>> {
    let cfb = Cfb::open(bytes)?;
    let names: Vec<String> = models(&cfb).into_iter().map(|m| m.0).collect();
    (!names.is_empty()).then_some(names)
}

/// The level table (the table entries after a header of table 1): id,
/// name and colour index. The default level (64) is listed as "0", as the
/// reference names it.
fn level_table(nm: &[&[u8]]) -> Vec<(u32, String, u32)> {
    let mut out = level_table_raw(nm);
    for (id, name, _) in &mut out {
        if *id == 64 && name.eq_ignore_ascii_case("Default") {
            *name = "0".to_string();
        }
    }
    out
}

/// The level table with the file's own names: the first one (the file's;
/// a later level table holds a reference attachment's levels).
fn level_table_raw(nm: &[&[u8]]) -> Vec<(u32, String, u32)> {
    let mut out = Vec::new();
    let mut in_levels = false;
    for e in nm {
        match kind(e) {
            96 if u32_at(e, 12) == Some(1) && !out.is_empty() => break,
            96 => in_levels = u32_at(e, 12) == Some(1),
            95 if in_levels && e.len() >= 80 => {
                let (Some(id), Some(color)) = (u32_at(e, 32), u32_at(e, 72)) else {
                    continue;
                };
                let name = marked_string(e, 32).unwrap_or_default();
                out.push((id, name, color));
            }
            _ => {}
        }
    }
    out
}

fn non_model(cfb: &Cfb) -> Vec<u8> {
    cfb.streams()
        .iter()
        .find(|(p, _)| p == "Dgn^Nm/$1")
        .map(|(_, e)| inflate(&cfb.stream(e)))
        .unwrap_or_default()
}

/// Level names of the file, for the underlay layers list.
pub fn layer_names(bytes: &[u8]) -> Option<Vec<String>> {
    let cfb = Cfb::open(bytes)?;
    let nm = non_model(&cfb);
    Some(
        level_table(&elements(&nm))
            .into_iter()
            .map(|l| l.1)
            .filter(|n| !n.is_empty())
            .collect(),
    )
}

pub fn model(bytes: &[u8], name: &str, hidden: &[String]) -> Option<Sheet> {
    let cfb = Cfb::open(bytes)?;
    let all = models(&cfb);
    let (_, graphics, header) = all
        .iter()
        .find(|m| m.0.eq_ignore_ascii_case(name))
        .or_else(|| all.first())?;
    let non_model = non_model(&cfb);
    let nm = elements(&non_model);
    // Colours 0-254 from byte 37, the background (255) opening the table.
    let color_table = nm
        .iter()
        .find(|e| kind(e) == 5)
        .and_then(|e| dgn_color_table(e, 37));
    let table_colors = color_table.is_some();
    let colors = color_table.unwrap_or_else(|| DGN_DEFAULT_COLORS.to_vec());
    let table = level_table(&nm);
    let levels = table.iter().map(|l| (l.0, l.2)).collect();
    let hidden = table
        .iter()
        .filter(|l| hidden.iter().any(|h| h.eq_ignore_ascii_case(&l.1)))
        .map(|l| l.0)
        .collect();
    let styles = Styles {
        colors,
        levels,
        hidden,
        extended: extended_colors(&cfb),
        cells: cell_ids(&nm),
    };
    // Shared cell definitions: a type-34 element and its components.
    let mut defs: HashMap<String, Vec<&[u8]>> = HashMap::new();
    let mut i = 0;
    while i < nm.len() {
        if kind(nm[i]) == 34 && !is_component(nm[i]) {
            let name = marked_string(nm[i], 248)
                .unwrap_or_default()
                .to_ascii_uppercase();
            let mut j = i + 1;
            while j < nm.len() && is_component(nm[j]) {
                j += 1;
            }
            defs.insert(name, nm[i + 1..j].to_vec());
            i = j;
        } else {
            i += 1;
        }
    }
    let graphics = elements(graphics);
    let raw = read_run(&graphics, &styles, &IDENTITY);
    let (per_unit, sub_uor) = units(header);
    let origin = global_origin(header);
    let m: Affine = [
        1.0 / per_unit,
        0.0,
        0.0,
        1.0 / per_unit,
        -origin[0] / per_unit,
        -origin[1] / per_unit,
    ];
    let mut sheet = Sheet {
        sub_per_master: per_unit / sub_uor,
        table_colors,
        ..Default::default()
    };
    place(&raw, &m, &defs, &styles, 0, &mut sheet);
    // The model's extent is its elements' stored ranges (text by its full
    // box), as the reference sizes a model.
    sheet.rect = match stored_range(&graphics) {
        Some([x0, y0, x1, y1]) => {
            let (lo, hi) = (apply(&m, [x0, y0]), apply(&m, [x1, y1]));
            [lo[0], lo[1], hi[0], hi[1]]
        }
        None => paths_bounds(&sheet.paths)?,
    };
    Some(sheet)
}

/// The model's global origin (UORs; the design plane point drawn at the
/// model's 0,0).
fn global_origin(header: &[u8]) -> [f64; 2] {
    let get = |at: usize| f64_at(header, at).filter(|v| v.is_finite()).unwrap_or(0.0);
    [get(4212), get(4220)]
}

/// The union of the top-level elements' ranges (UORs): each graphic element
/// stores its low corner at 56 and its size at 80, as 64-bit integers.
fn stored_range(graphics: &[&[u8]]) -> Option<[f64; 4]> {
    let i64_at =
        |e: &[u8], at: usize| Some(i64::from_le_bytes(e.get(at..at + 8)?.try_into().ok()?) as f64);
    let mut r = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for e in graphics
        .iter()
        .filter(|e| e.len() >= 104 && !is_component(e))
    {
        let (Some(x), Some(y), Some(w), Some(h)) =
            (i64_at(e, 56), i64_at(e, 64), i64_at(e, 80), i64_at(e, 88))
        else {
            continue;
        };
        r = [r[0].min(x), r[1].min(y), r[2].max(x + w), r[3].max(y + h)];
    }
    (r[0] <= r[2]).then_some(r)
}

/// The short name of a unit of `metres` per unit ("m", "mm", "ft" …), empty
/// for one without a common name.
fn unit_name(metres: f64) -> String {
    const NAMES: [(f64, &str); 8] = [
        (1.0, "m"),
        (0.001, "mm"),
        (0.01, "cm"),
        (1000.0, "km"),
        (0.0254, "in"),
        (0.3048, "ft"),
        (0.9144, "yd"),
        (1609.344, "mi"),
    ];
    NAMES
        .iter()
        .find(|(m, _)| (metres / m - 1.0).abs() < 1e-6)
        .map(|(_, n)| n.to_string())
        .unwrap_or_default()
}

/// A design model (not a sheet): the model type byte of its header (4105)
/// is 2 for a sheet.
fn is_design(header: &[u8]) -> bool {
    header.get(4105) != Some(&2)
}

/// The models DGNIMPORT offers: the design models (sheets are not
/// imported).
pub fn design_model_names(bytes: &[u8]) -> Option<Vec<String>> {
    let cfb = Cfb::open(bytes)?;
    let names: Vec<String> = models(&cfb)
        .into_iter()
        .filter(|m| is_design(&m.2))
        .map(|m| m.0)
        .collect();
    (!names.is_empty()).then_some(names)
}

/// The control stream (Dgn^C) of model `k` in the order `models` lists
/// them, inflated within what is left of the file's budget (`used` bytes
/// are taken by the models already read).
fn control_stream(cfb: &Cfb, k: usize, used: usize) -> Vec<u8> {
    let streams = cfb.streams();
    let mut dirs: Vec<&str> = streams
        .iter()
        .filter_map(|(p, _)| p.strip_suffix("/Dgn^G/$1"))
        .collect();
    dirs.sort();
    let Some(dir) = dirs.get(k) else {
        return Vec::new();
    };
    let left = inflate_budget(cfb).saturating_sub(used) as u64;
    if left == 0 {
        return Vec::new();
    }
    streams
        .iter()
        .find(|(p, _)| *p == format!("{dir}/Dgn^C/$1"))
        .map(|(_, e)| inflate_at_most(&cfb.stream(e), left.min(STREAM_LIMIT)))
        .unwrap_or_default()
}

/// Every string of an element stored after the "ff fe 01 00" marker, with
/// the subtype of the string linkage before it (2 file name, 3 full path,
/// 4 logical name).
fn linked_strings(e: &[u8]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = e
        .get(from..)
        .and_then(|rest| rest.windows(4).position(|w| w == [0xff, 0xfe, 0x01, 0x00]))
    {
        let at = from + i;
        let end = e[at + 4..]
            .iter()
            .position(|&c| c == 0 || c < 0x20)
            .map(|n| at + 4 + n)
            .unwrap_or(e.len());
        let subtype = at.checked_sub(8).and_then(|p| u32_at(e, p)).unwrap_or(0);
        out.push((subtype, String::from_utf8_lossy(&e[at + 4..end]).to_string()));
        from = end.max(at + 4);
    }
    out
}

/// The reference attachments (type 100) of a model's control stream: the
/// file (full path, else file name), origin 192, rotation rows from 216,
/// scale 288, in UORs of the model.
fn references(control: &[u8]) -> Vec<DgnReference> {
    elements(control)
        .into_iter()
        .filter(|e| kind(e) == 100 && e.len() >= 296)
        .filter_map(|e| {
            let strings = linked_strings(e);
            let file = strings
                .iter()
                .find(|s| s.0 == 3)
                .or_else(|| strings.iter().find(|s| s.0 == 2))?
                .1
                .clone();
            Some(DgnReference {
                file,
                model: strings
                    .iter()
                    .find(|s| s.0 == 1)
                    .map(|s| s.1.clone())
                    .unwrap_or_default(),
                origin: [f64_at(e, 192)?, f64_at(e, 200)?, f64_at(e, 208)?],
                scale: f64_at(e, 288).filter(|s| *s > 0.0).unwrap_or(1.0),
                rotation: f64_at(e, 240)?.atan2(f64_at(e, 216)?),
            })
        })
        .collect()
}

/// The named model element by element (DGNIMPORT): in master units with
/// the global origin at 0,0, shared cell definitions in master units, and
/// every level by id with its own name and colour.
pub fn import(bytes: &[u8], name: &str) -> Option<DgnImport> {
    let cfb = Cfb::open(bytes)?;
    let all = models(&cfb);
    let designs: Vec<usize> = (0..all.len()).filter(|k| is_design(&all[*k].2)).collect();
    let k = designs
        .iter()
        .copied()
        .find(|k| all[*k].0.eq_ignore_ascii_case(name))
        .or_else(|| designs.first().copied())?;
    let (_, graphics, header) = &all[k];
    let non_model = non_model(&cfb);
    let nm = elements(&non_model);
    let colors = nm
        .iter()
        .find(|e| kind(e) == 5)
        .and_then(|e| dgn_color_table(e, 37))
        .unwrap_or_else(|| DGN_DEFAULT_COLORS.to_vec());
    let table = level_table_raw(&nm);
    let styles = Styles {
        levels: table.iter().map(|l| (l.0, l.2)).collect(),
        hidden: Default::default(),
        extended: extended_colors(&cfb),
        cells: cell_ids(&nm),
        colors,
    };
    let levels = table
        .iter()
        .map(|(id, name, color)| {
            let rgb = styles.colors.get(*color as usize).copied();
            (*id, (name.clone(), rgb))
        })
        .collect();
    let level_order = table.iter().map(|l| l.0).collect();
    let (per_unit, sub_uor) = units(header);
    let s = 1.0 / per_unit;
    let mut cells = Vec::new();
    let mut i = 0;
    while i < nm.len() {
        if kind(nm[i]) == 34 && !is_component(nm[i]) {
            let cell = marked_string(nm[i], 248).unwrap_or_default();
            let mut j = i + 1;
            while j < nm.len() && is_component(nm[j]) {
                j += 1;
            }
            // The cell that draws points belongs to the writing program,
            // not to the drawing: it is left out.
            if !cell.eq_ignore_ascii_case("POINT_DISPLAY") {
                let def = read_run(&nm[i + 1..j], &styles, &IDENTITY).elements;
                cells.push((cell, scaled_elements(&def, s, [0.0; 3])));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    let raw = read_run(&elements(graphics), &styles, &IDENTITY);
    let origin = global_origin(header);
    // Unit definitions: numerator / denominator per metre, master at 4180,
    // sub at 4196.
    let metres = |at: usize| match (f64_at(header, at), f64_at(header, at + 8)) {
        (Some(n), Some(d)) if n > 0.0 && d > 0.0 => unit_name(d / n),
        _ => String::new(),
    };
    let used = all.iter().map(|m| m.1.len() + m.2.len()).sum();
    let references = references(&control_stream(&cfb, k, used))
        .into_iter()
        .map(|r| DgnReference {
            origin: [
                (r.origin[0] - origin[0]) * s,
                (r.origin[1] - origin[1]) * s,
                r.origin[2] * s,
            ],
            ..r
        })
        .collect();
    Some(DgnImport {
        elements: scaled_elements(&raw.elements, s, [-origin[0] * s, -origin[1] * s, 0.0]),
        cells,
        levels,
        level_order,
        sub_per_master: per_unit / sub_uor,
        unit_names: (metres(4180), metres(4196)),
        v7: false,
        references,
        text_styles: text_style_table(&nm),
        dim_styles: dim_style_table(&nm, s),
        fonts: font_table(&nm)
            .into_iter()
            .map(|(n, k, name)| {
                let file = font_file(Some((k, &name)));
                (n, k, name, file)
            })
            .collect(),
        three_d: false,
        construction_layers: false,
    })
}

#[cfg(test)]
mod cfb_memory_tests {
    use super::*;

    const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
    const NONE: u32 = 0xFFFF_FFFF;

    /// Directory entries start at the first sector — `(0 + 1) * sector`,
    /// so the header occupies a full sector, not just its first 512 bytes.
    fn put_entry(
        b: &mut [u8],
        base: usize,
        idx: usize,
        name: &str,
        kind: u8,
        left: u32,
        right: u32,
        child: u32,
        start: u32,
        size: u32,
    ) {
        let e = base + idx * 128;
        let utf16: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        for (i, ch) in utf16.iter().enumerate() {
            b[e + i * 2..e + i * 2 + 2].copy_from_slice(&ch.to_le_bytes());
        }
        b[e + 64..e + 66].copy_from_slice(&((utf16.len() * 2) as u16).to_le_bytes());
        b[e + 66] = kind;
        b[e + 68..e + 72].copy_from_slice(&left.to_le_bytes());
        b[e + 72..e + 76].copy_from_slice(&right.to_le_bytes());
        b[e + 76..e + 80].copy_from_slice(&child.to_le_bytes());
        b[e + 116..e + 120].copy_from_slice(&start.to_le_bytes());
        b[e + 120..e + 124].copy_from_slice(&size.to_le_bytes());
    }

    fn header(b: &mut [u8], sector_shift: u16, dir_start: u32, minifat_start: u32, difat0: u32) {
        b[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        b[30..32].copy_from_slice(&sector_shift.to_le_bytes());
        b[32..34].copy_from_slice(&6u16.to_le_bytes()); // mini sector shift
        b[48..52].copy_from_slice(&dir_start.to_le_bytes());
        b[56..60].copy_from_slice(&4096u32.to_le_bytes()); // mini cutoff
        b[60..64].copy_from_slice(&minifat_start.to_le_bytes());
        b[68..72].copy_from_slice(&ENDOFCHAIN.to_le_bytes()); // DIFAT chain: none
        b[72..76].copy_from_slice(&0u32.to_le_bytes()); // DIFAT chain count: 0
        b[76..80].copy_from_slice(&difat0.to_le_bytes()); // header DIFAT slot 0
        for slot in 1..109 {
            b[76 + slot * 4..80 + slot * 4].copy_from_slice(&NONE.to_le_bytes());
        }
    }

    /// ~1.5 KB CFB (sector_shift = 9): directory and miniFAT both start at
    /// sector 0, whose FAT entry (sector 1, all zeros) points at itself —
    /// `chain()` loops its full 1_000_000-iteration budget → 512 MB per
    /// call, three calls alive at once.
    fn hostile_self_fat_cfb() -> Vec<u8> {
        let mut b = vec![0u8; 3 * 512];
        header(&mut b, 9, 0, 0, 1);
        // Sector 0 = directory: entry 0 is the root; its start field also
        // points at sector 0 (self-loop), size = u32::MAX so the mini chain
        // is never truncated down to a small length.
        put_entry(&mut b, 512, 0, "Root", 1, NONE, NONE, NONE, 0, u32::MAX);
        // Sector 1 = FAT, already zero: fat[0] = 0 (self-referential).
        b
    }

    /// Write the model storages' left/right sibling links as a balanced
    /// binary tree over entry indices `2, 4, …, 2·n` (walk depth O(log n)).
    fn link_models(b: &mut [u8], base: usize, model_count: usize) {
        fn build(b: &mut [u8], base: usize, lo: usize, hi: usize) -> u32 {
            if lo > hi {
                return NONE;
            }
            let mid = (lo + hi) / 2;
            let left = if mid > lo {
                build(b, base, lo, mid - 1)
            } else {
                NONE
            };
            let right = if mid < hi {
                build(b, base, mid + 1, hi)
            } else {
                NONE
            };
            let e = base + (2 + mid * 2) * 128;
            b[e + 68..e + 72].copy_from_slice(&left.to_le_bytes());
            b[e + 72..e + 76].copy_from_slice(&right.to_le_bytes());
            (2 + mid * 2) as u32
        }
        build(b, base, 0, model_count - 1);
    }

    /// 300 model directories all sharing one 4 MB stream: `models()`
    /// retains every model simultaneously with no total budget → 1.2 GB
    /// from a 4 MB file (inflate() falls back to a raw copy of the zeros).
    fn multi_model_cfb() -> Vec<u8> {
        let sector = 4096usize;
        let model_count = 300usize;
        let entry_count = 2 + model_count * 2;
        let dir_sectors = (entry_count * 128 + sector - 1) / sector;
        let payload_len = 4 << 20;
        let payload_sectors = payload_len / sector;
        // Sectors: 0..dir_sectors directory; then 2 FAT; then payload.
        let fat_start = dir_sectors;
        let payload_start = fat_start + 2;
        let mut b = vec![0u8; (payload_start + payload_sectors + 1) * sector];
        header(&mut b, 12, 0, ENDOFCHAIN, fat_start as u32);
        b[80..84].copy_from_slice(&((fat_start + 1) as u32).to_le_bytes()); // DIFAT slot 1
                                                                            // Directory: root → storage → model storages, sibling links as a
                                                                            // balanced tree (real CFB directories are red-black; a linear chain
                                                                            // would hit streams()' existing depth-64 guard). Each storage has
                                                                            // one "Dgn^G/$1" stream pointing at the shared payload.
        let root_model = (model_count - 1) / 2;
        put_entry(&mut b, sector, 0, "Root", 1, NONE, NONE, 1, ENDOFCHAIN, 0);
        put_entry(
            &mut b,
            sector,
            1,
            "Dgn-Md",
            1,
            NONE,
            NONE,
            (2 + root_model * 2) as u32,
            ENDOFCHAIN,
            0,
        );
        for i in 0..model_count {
            let model = 2 + i * 2;
            let stream = model + 1;
            put_entry(
                &mut b,
                sector,
                model,
                &format!("Model{i}"),
                1,
                NONE,
                NONE,
                stream as u32,
                ENDOFCHAIN,
                0,
            );
            put_entry(
                &mut b,
                sector,
                stream,
                "Dgn^G/$1",
                2,
                NONE,
                NONE,
                NONE,
                payload_start as u32,
                payload_len as u32,
            );
        }
        link_models(&mut b, sector, model_count);
        // Directory chain across its sectors.
        let fat_at = (fat_start + 1) * sector;
        for i in 0..dir_sectors {
            let at = fat_at + i * 4;
            let next = if i + 1 < dir_sectors {
                (i + 1) as u32
            } else {
                ENDOFCHAIN
            };
            b[at..at + 4].copy_from_slice(&next.to_le_bytes());
        }
        // Both FAT sectors are not part of any chain; payload chain follows.
        for i in 0..2 {
            let at = fat_at + (fat_start + i) * 4;
            b[at..at + 4].copy_from_slice(&ENDOFCHAIN.to_le_bytes());
        }
        for i in 0..payload_sectors {
            let at = fat_at + (payload_start + i) * 4;
            let next = if i + 1 < payload_sectors {
                (payload_start + i + 1) as u32
            } else {
                ENDOFCHAIN
            };
            b[at..at + 4].copy_from_slice(&next.to_le_bytes());
        }
        b
    }

    /// A self-referential FAT must not let `chain()` (directory, mini
    /// stream, miniFAT) grow past the file it was read from: the old fixed
    /// 1_000_000-iteration guard built 512 MB per call from 1.5 KB.
    #[test]
    fn a_self_referential_fat_builds_no_more_bytes_than_the_file() {
        let bytes = hostile_self_fat_cfb();
        let cfb = Cfb::open(&bytes).expect("hostile CFB parses");
        assert!(
            cfb.entries.len() * 128 <= bytes.len(),
            "directory grew to {} entries from a {} byte file",
            cfb.entries.len(),
            bytes.len()
        );
        assert!(
            cfb.mini.len() <= bytes.len(),
            "mini stream grew to {} bytes from a {} byte file",
            cfb.mini.len(),
            bytes.len()
        );
        assert!(
            cfb.mini_fat.len() * 4 <= bytes.len(),
            "miniFAT grew to {} bytes from a {} byte file",
            cfb.mini_fat.len() * 4,
            bytes.len()
        );
    }

    /// Hundreds of model dirs sharing one stream: `models()` inflates all
    /// of them at once. The total must stay within the file-scaled budget,
    /// while the first models still load.
    #[test]
    fn models_keep_their_total_inflated_size_within_budget() {
        let bytes = multi_model_cfb();
        let cfb = Cfb::open(&bytes).expect("multi-model CFB parses");
        let out = models(&cfb);
        let total: usize = out.iter().map(|(_, g, h)| g.len() + h.len()).sum();
        let budget = (bytes.len() * 64).max(256 << 20);
        assert!(
            total <= budget + (512 << 20),
            "models inflated to {} bytes from a {} byte file (budget {budget})",
            total,
            bytes.len()
        );
        assert!(!out.is_empty(), "the first models still load");
        assert!(out.len() < 300, "models past the budget are dropped");
        assert!(model_names(&bytes).is_some());
    }
}

#[cfg(test)]
mod mesh_tests {
    use super::*;

    /// An element of `kind` and `len` bytes, a component when `part`.
    fn element(kind: u8, len: usize, part: bool) -> Vec<u8> {
        let mut e = vec![0u8; len];
        e[0] = kind;
        e[2..4].copy_from_slice(&(if part { 0x4000u16 } else { 0 }).to_le_bytes());
        e[4..8].copy_from_slice(&(((len - 4) / 2) as u32).to_le_bytes());
        e
    }

    fn put(e: &mut [u8], at: usize, v: u32) {
        e[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// A face-loop mesh: one loop with a hidden edge, one naming a point
    /// the mesh lacks (left out).
    #[test]
    fn face_loop_mesh_reads_its_points_and_loops() {
        let mut header = element(105, 116, false);
        put(&mut header, 104, 4);
        put(&mut header, 108, 1);
        let matrix = |family: u32| {
            let mut m = element(101, 132, true);
            put(&mut m, 116, family);
            m
        };
        let numbers = [1i32, 2, -3, 0, 2, 3, 9, 0];
        let mut ints = element(102, 120 + 4 * numbers.len(), true);
        put(&mut ints, 104, numbers.len() as u32);
        for (k, n) in numbers.iter().enumerate() {
            put(&mut ints, 120 + 4 * k, *n as u32);
        }
        let coords = [0.0f64, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 2.0];
        let mut doubles = element(103, 120 + 8 * coords.len(), true);
        put(&mut doubles, 104, coords.len() as u32);
        for (k, c) in coords.iter().enumerate() {
            doubles[120 + 8 * k..128 + 8 * k].copy_from_slice(&c.to_le_bytes());
        }
        let (m1, m2) = (matrix(1), matrix(2));
        let run: Vec<&[u8]> = vec![&header, &m1, &ints, &m2, &doubles];
        let styles = Styles {
            colors: DGN_DEFAULT_COLORS.to_vec(),
            levels: HashMap::new(),
            hidden: Default::default(),
            extended: Vec::new(),
            cells: HashMap::new(),
        };
        let raw = read_run(&run, &styles, &IDENTITY);
        let meshes: Vec<_> = raw
            .elements
            .iter()
            .filter_map(|e| match &e.geom {
                DgnGeom::Mesh { points, loops } => Some((points.clone(), loops.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            meshes,
            vec![(
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 2.0]],
                vec![vec![1, 2, -3]]
            )]
        );
    }
}
