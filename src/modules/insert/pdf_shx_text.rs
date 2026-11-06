// PDFSHXTEXT — recognise text that was drawn as SHX stroke geometry (a PDF
// plotted with SHX fonts and imported back) and replace it with MTEXT.
//
//   Select geometry to convert to text...
//   Select objects or [SEttings]:
//   Converting geometry to text...
//   Group 1: Succeeded - simplex 100%
//   Group 2: Failed to meet success threshold - simplex 64%
//   1 of 2 groups converted to text
//   2 text objects created
//
// The strokes are clustered into characters, the characters into text lines
// and the lines into groups (text blocks: lines of one size stacked close
// together, or side by side). Each line is matched against the glyphs of the
// fonts to compare, in list order: a group converts with the first font (or,
// with "Use best matching font", the best) whose share of recognised objects
// reaches the threshold. Every recognised line becomes a bottom-left MTEXT in
// a style named after the font, a group's lines top down; the geometry of its
// recognised characters is erased.

use document::Handle;
use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::text::font_face::Face;

/// The fonts the reference lists, in its order.
pub const FONT_NAMES: [&str; 8] = [
    "simplex", "romans", "txt", "romand", "romanc", "complex", "romant", "isocp",
];
/// Enabled by default.
const DEFAULT_ENABLED: [&str; 3] = ["simplex", "txt", "isocp"];

/// One font of the comparison list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShxFontEntry {
    /// The font name ("simplex"), or the full path of an added .shx file.
    pub name: String,
    pub enabled: bool,
}

impl ShxFontEntry {
    /// The name shown and given to the text style: the file stem.
    pub fn label(&self) -> String {
        font_label(&self.name)
    }
}

pub fn font_label(name: &str) -> String {
    std::path::Path::new(&name.replace('\\', "/"))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string())
}

/// The PDF Text Recognition Settings (profile settings).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShxTextSettings {
    pub fonts: Vec<ShxFontEntry>,
    /// Recognition threshold in percent (1–100).
    pub threshold: u8,
    /// PDFSHXLAYER: the text goes on the current layer (else the geometry's).
    pub current_layer: bool,
    /// PDFSHXBESTFONT: compare every enabled font and keep the best.
    pub best_font: bool,
    /// PDFSHXTEXTSETTINGS: the next PDFSHXTEXT opens the settings first.
    pub show_settings: bool,
}

impl Default for ShxTextSettings {
    fn default() -> Self {
        Self {
            fonts: FONT_NAMES
                .iter()
                .map(|name| ShxFontEntry {
                    name: name.to_string(),
                    enabled: DEFAULT_ENABLED.contains(name),
                })
                .collect(),
            threshold: 95,
            current_layer: true,
            best_font: false,
            show_settings: false,
        }
    }
}

impl ShxTextSettings {
    /// Out-of-range values from a hand-edited profile.
    pub fn sanitize(&mut self) {
        self.threshold = self.threshold.clamp(1, 100);
        self.fonts.retain(|f| !f.name.trim().is_empty());
        let mut seen = std::collections::HashSet::new();
        self.fonts
            .retain(|f| seen.insert(f.label().to_ascii_lowercase()));
        if self.fonts.is_empty() {
            self.fonts = Self::default().fonts;
        }
    }
}

static SETTINGS: std::sync::Mutex<Option<ShxTextSettings>> = std::sync::Mutex::new(None);

pub fn settings() -> ShxTextSettings {
    SETTINGS
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_default()
}

pub fn set_settings(mut settings: ShxTextSettings) {
    settings.sanitize();
    if let Ok(mut s) = SETTINGS.lock() {
        *s = Some(settings);
    }
}

/// What `Face::resolve` loads for a listed font: an added file as it is; a
/// font name's .shx next to the drawing (`drawing` is its path) or in the
/// fonts folder; else the embedded stroke font of that very name. Nothing
/// otherwise: no alias stand-in and no TrueType face — measured, a listed
/// font without its .shx file is not compared at all ("-  0%").
pub fn resolve_font(name: &str, drawing: Option<&str>) -> Option<String> {
    if name.to_ascii_lowercase().ends_with(".shx") && std::path::Path::new(name).is_file() {
        return Some(name.to_string());
    }
    let label = font_label(name);
    let file = format!("{label}.shx");
    let base = drawing.map(std::path::Path::new).and_then(|p| p.parent());
    crate::io::resolve_image_file(&file, base)
        .or_else(|| {
            crate::io::font_repo::local_font_file(&file).map(|p| p.to_string_lossy().into_owned())
        })
        .or_else(|| crate::scene::text::lff::is_embedded_font(&label).then_some(label))
}

// ── Recognition ────────────────────────────────────────────────────────────

/// One curve of the selection, flattened to world XY.
#[derive(Clone)]
pub struct Stroke {
    pub handle: Handle,
    pub layer: String,
    pub points: Vec<[f64; 2]>,
}

/// A recognised line of text.
pub struct TextLine {
    /// MTEXT contents (with a `\W` width factor when it is not 1).
    pub value: String,
    /// Bottom-left corner of the recognised ink.
    pub insertion: [f64; 2],
    pub height: f64,
    /// The font label, also the text style's name.
    pub font: String,
    /// The font as listed (a name or the path of an added file).
    pub font_spec: String,
    /// The layer of the line's first stroke.
    pub layer: String,
    /// The strokes the line replaces.
    pub handles: Vec<Handle>,
}

pub struct GroupReport {
    pub font: String,
    pub percent: u32,
    pub converted: bool,
}

pub struct Recognition {
    pub groups: Vec<GroupReport>,
    pub lines: Vec<TextLine>,
}

type P = [f64; 2];

#[derive(Clone, Copy)]
struct BBox {
    min: P,
    max: P,
}

impl BBox {
    fn empty() -> Self {
        Self {
            min: [f64::INFINITY; 2],
            max: [f64::NEG_INFINITY; 2],
        }
    }
    fn of(points: &[P]) -> Self {
        let mut b = Self::empty();
        for p in points {
            b.add(*p);
        }
        b
    }
    fn add(&mut self, p: P) {
        self.min = [self.min[0].min(p[0]), self.min[1].min(p[1])];
        self.max = [self.max[0].max(p[0]), self.max[1].max(p[1])];
    }
    fn union(&mut self, o: &BBox) {
        self.add(o.min);
        self.add(o.max);
    }
    fn w(&self) -> f64 {
        self.max[0] - self.min[0]
    }
    fn h(&self) -> f64 {
        self.max[1] - self.min[1]
    }
    fn size(&self) -> f64 {
        self.w().max(self.h())
    }
    fn cx(&self) -> f64 {
        (self.min[0] + self.max[0]) * 0.5
    }
    fn near(&self, o: &BBox, eps: f64) -> bool {
        self.min[0] <= o.max[0] + eps
            && o.min[0] <= self.max[0] + eps
            && self.min[1] <= o.max[1] + eps
            && o.min[1] <= self.max[1] + eps
    }
}

fn seg_dist(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
    (x * x + y * y).sqrt()
}

fn dist_to(p: P, strokes: &[Vec<P>]) -> f64 {
    let mut best = f64::INFINITY;
    for s in strokes {
        match s.len() {
            0 => {}
            1 => best = best.min(seg_dist(p, s[0], s[0])),
            _ => {
                for w in s.windows(2) {
                    best = best.min(seg_dist(p, w[0], w[1]));
                }
            }
        }
    }
    best
}

fn cross(o: P, a: P, b: P) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn segments_cross(a: P, b: P, c: P, d: P) -> bool {
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// Least distance between two polylines (0 where they cross).
fn polyline_dist(a: &[P], b: &[P]) -> f64 {
    let segs = |s: &[P]| -> Vec<(P, P)> {
        if s.len() == 1 {
            vec![(s[0], s[0])]
        } else {
            s.windows(2).map(|w| (w[0], w[1])).collect()
        }
    };
    let (sa, sb) = (segs(a), segs(b));
    let mut best = f64::INFINITY;
    for &(p, q) in &sa {
        for &(r, t) in &sb {
            if segments_cross(p, q, r, t) {
                return 0.0;
            }
            best = best
                .min(seg_dist(p, r, t))
                .min(seg_dist(q, r, t))
                .min(seg_dist(r, p, q))
                .min(seg_dist(t, p, q));
        }
    }
    best
}

/// Points along the strokes no more than `step` apart (vertices included).
fn sample(strokes: &[Vec<P>], step: f64) -> Vec<P> {
    let mut out = Vec::new();
    for s in strokes {
        if let Some(first) = s.first() {
            out.push(*first);
        }
        for w in s.windows(2) {
            let len = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
            let n = if step > 0.0 {
                (len / step).ceil().clamp(1.0, 400.0) as usize
            } else {
                1
            };
            for k in 1..=n {
                let t = k as f64 / n as f64;
                out.push([
                    w[0][0] + (w[1][0] - w[0][0]) * t,
                    w[0][1] + (w[1][1] - w[0][1]) * t,
                ]);
            }
        }
    }
    out
}

/// Mean distance both ways between two stroke sets.
fn chamfer(a_samples: &[P], a: &[Vec<P>], b_samples: &[P], b: &[Vec<P>]) -> f64 {
    if a_samples.is_empty() || b_samples.is_empty() {
        return f64::INFINITY;
    }
    let ab: f64 = a_samples.iter().map(|p| dist_to(*p, b)).sum::<f64>() / a_samples.len() as f64;
    let ba: f64 = b_samples.iter().map(|p| dist_to(*p, a)).sum::<f64>() / b_samples.len() as f64;
    0.5 * (ab + ba)
}

/// Scale factors that map a box to the unit square; a near-flat side takes
/// the other side's scale so a dash or a bar keeps its proportions.
fn unit_scales(b: &BBox) -> (f64, f64) {
    let m = b.size();
    if m <= 0.0 {
        return (1.0, 1.0);
    }
    let sx = if b.w() < 0.15 * m { m } else { b.w() };
    let sy = if b.h() < 0.15 * m { m } else { b.h() };
    (sx, sy)
}

fn normalized(strokes: &[Vec<P>], b: &BBox) -> Vec<Vec<P>> {
    let (sx, sy) = unit_scales(b);
    strokes
        .iter()
        .map(|s| {
            s.iter()
                .map(|p| [(p[0] - b.min[0]) / sx, (p[1] - b.min[1]) / sy])
                .collect()
        })
        .collect()
}

/// A glyph in units of the font's cap height.
struct GlyphShape {
    ch: char,
    strokes: Vec<Vec<P>>,
    bbox: BBox,
    /// Pen advance including the spacing after it.
    advance: f64,
    norm: Vec<Vec<P>>,
    norm_samples: Vec<P>,
}

struct FontShapes {
    glyphs: Vec<GlyphShape>,
    word: f64,
}

/// Sampling step of the comparisons, in text heights.
const STEP: f64 = 0.04;
/// Largest mean distance (in text heights) of a recognised character.
/// The geometry of a font matches its glyphs within a few thousandths; a
/// circle read as the 0 of simplex (as the reference reads it) at 0.0121; a
/// look-alike glyph of another font (txt's O as a D) from about 0.015.
const MATCH_TOLERANCE: f64 = 0.013;

/// The characters compared: printable ASCII, then Latin-1 and Latin
/// Extended (°, ±, accented letters), Greek, Cyrillic and the diameter sign
/// ⌀ (U+2205, what the reference reads %%c as) — those a font has a glyph of
/// its own for.
fn compared_chars() -> impl Iterator<Item = char> {
    (0x21u32..=0x7e)
        .chain(0xa1..=0x24f)
        .chain(0x370..=0x4ff)
        .chain([0x2205])
        .filter_map(char::from_u32)
}

fn font_shapes(spec: &str) -> Option<FontShapes> {
    let face = Face::resolve(spec);
    let to_p = |s: &Vec<[f32; 2]>| -> Vec<P> { s.iter().map(|p| [p[0] as f64, p[1] as f64]).collect() };
    let cap = face
        .glyph('H')
        .map(|g| {
            let pts: Vec<P> = g.strokes.iter().flat_map(to_p).collect();
            BBox::of(&pts).h()
        })
        .filter(|h| *h > 0.0)?;
    let mut glyphs = Vec::new();
    // A character the font lacks is no candidate (the text fallback would
    // offer another font's glyph for it).
    for ch in compared_chars() {
        let Some(g) = face.own_glyph(ch) else { continue };
        let strokes: Vec<Vec<P>> = g
            .strokes
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.iter().map(|p| [p[0] as f64 / cap, p[1] as f64 / cap]).collect())
            .collect();
        if strokes.is_empty() {
            continue;
        }
        let bbox = BBox::of(&strokes.iter().flatten().copied().collect::<Vec<_>>());
        let norm = normalized(&strokes, &bbox);
        let norm_samples = sample(&norm, STEP);
        glyphs.push(GlyphShape {
            ch,
            advance: (g.advance + face.spacing_after(ch)) as f64 / cap,
            strokes,
            bbox,
            norm,
            norm_samples,
        });
    }
    Some(FontShapes {
        glyphs,
        word: face.word_spacing() as f64 / cap,
    })
}

/// Strokes that touch or cross, one character (or part of one).
struct Piece {
    strokes: Vec<usize>,
    bbox: BBox,
}

/// A character: pieces that share a column of a line.
struct Glyphed {
    strokes: Vec<usize>,
    bbox: BBox,
    geometry: Vec<Vec<P>>,
}

struct Line {
    chars: Vec<Glyphed>,
    bbox: BBox,
    first: usize,
    height: f64,
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

/// Pairs of boxes (by index) whose x-ranges, widened by `pad(i)`, overlap.
fn x_sweep(boxes: &[BBox], pad: impl Fn(usize) -> f64, mut visit: impl FnMut(usize, usize)) {
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|a, b| boxes[*a].min[0].total_cmp(&boxes[*b].min[0]));
    for (k, &a) in order.iter().enumerate() {
        let reach = boxes[a].max[0] + pad(a);
        for &b in &order[k + 1..] {
            if boxes[b].min[0] > reach {
                break;
            }
            visit(a, b);
        }
    }
}

fn pieces_of(strokes: &[Stroke]) -> Vec<Piece> {
    let boxes: Vec<BBox> = strokes.iter().map(|s| BBox::of(&s.points)).collect();
    let tiny = boxes
        .iter()
        .map(|b| b.size())
        .fold(0.0f64, f64::max)
        * 1e-9;
    let eps = |a: usize, b: usize| 0.03 * boxes[a].size().min(boxes[b].size()) + tiny;
    let mut parent: Vec<usize> = (0..strokes.len()).collect();
    let widest = boxes.iter().map(|b| b.size()).fold(0.0f64, f64::max);
    x_sweep(&boxes, |_| 0.03 * widest + tiny, |a, b| {
        let e = eps(a, b);
        if !boxes[a].near(&boxes[b], e) {
            return;
        }
        if polyline_dist(&strokes[a].points, &strokes[b].points) <= e {
            union(&mut parent, a, b);
        }
    });
    let mut map: std::collections::BTreeMap<usize, Piece> = Default::default();
    for (i, b) in boxes.iter().enumerate() {
        let root = find(&mut parent, i);
        let piece = map.entry(root).or_insert_with(|| Piece {
            strokes: Vec::new(),
            bbox: BBox::empty(),
        });
        piece.strokes.push(i);
        piece.bbox.union(b);
    }
    map.into_values().collect()
}

/// Pieces of comparable size side by side on one band make a line; smaller
/// pieces (dots, commas, the dot of an i) join the line of the character
/// they sit closest to.
fn lines_of(strokes: &[Stroke], pieces: Vec<Piece>) -> Vec<Line> {
    let boxes: Vec<BBox> = pieces.iter().map(|p| p.bbox).collect();
    let comparable = |a: usize, b: usize| {
        let (sa, sb) = (boxes[a].size(), boxes[b].size());
        sa.min(sb) >= 0.35 * sa.max(sb) && sa.max(sb) > 0.0
    };
    let mut parent: Vec<usize> = (0..pieces.len()).collect();
    let tallest = boxes.iter().map(|b| b.h()).fold(0.0f64, f64::max);
    x_sweep(&boxes, |_| 2.0 * tallest, |a, b| {
        if !comparable(a, b) {
            return;
        }
        let (ba, bb) = (&boxes[a], &boxes[b]);
        let overlap = ba.max[1].min(bb.max[1]) - ba.min[1].max(bb.min[1]);
        if overlap < 0.3 * ba.h().min(bb.h()) {
            return;
        }
        // Measured: two words 1.8 heights apart are one line, 2.2 apart two.
        let gap = (ba.min[0].max(bb.min[0]) - ba.max[0].min(bb.max[0])).max(0.0);
        if gap <= 2.0 * ba.h().max(bb.h()) {
            union(&mut parent, a, b);
        }
    });
    // A piece with no comparable neighbour is a "small" one when something
    // at least three times its size is close by.
    let mut members = std::collections::HashMap::<usize, usize>::new();
    for i in 0..pieces.len() {
        *members.entry(find(&mut parent, i)).or_default() += 1;
    }
    let alone: Vec<bool> = (0..pieces.len())
        .map(|i| members[&find(&mut parent, i)] == 1)
        .collect();
    // ponytail: every lone piece is tested against every piece (boxes first);
    // a spatial grid if selections of many thousand pieces get slow.
    for i in 0..pieces.len() {
        if !alone[i] {
            continue;
        }
        let si = boxes[i].size();
        let mut best: Option<(f64, usize)> = None;
        for j in 0..pieces.len() {
            let sj = boxes[j].size();
            // A frame or a border around the text is no character to join.
            if j == i
                || si >= 0.35 * sj
                || (alone[j] && sj > 20.0 * si)
                || !boxes[i].near(&boxes[j], sj)
            {
                continue;
            }
            let mut d = f64::INFINITY;
            for m in &pieces[i].strokes {
                for t in &pieces[j].strokes {
                    d = d.min(polyline_dist(&strokes[*m].points, &strokes[*t].points));
                }
            }
            if d <= 0.6 * sj && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, j));
            }
        }
        if let Some((_, j)) = best {
            union(&mut parent, i, j);
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..pieces.len() {
        groups.entry(find(&mut parent, i)).or_default().push(i);
    }
    let mut lines = Vec::new();
    for members in groups.into_values() {
        // Pieces sharing a column are one character.
        let mut sorted = members;
        sorted.sort_by(|a, b| boxes[*a].min[0].total_cmp(&boxes[*b].min[0]));
        let mut chars: Vec<Glyphed> = Vec::new();
        for p in sorted {
            let b = boxes[p];
            let joins = chars.last().is_some_and(|c| {
                let overlap = c.bbox.max[0].min(b.max[0]) - c.bbox.min[0].max(b.min[0]);
                overlap >= 0.5 * c.bbox.w().min(b.w())
                    || (b.cx() >= c.bbox.min[0] && b.cx() <= c.bbox.max[0])
                    || (c.bbox.cx() >= b.min[0] && c.bbox.cx() <= b.max[0])
            });
            if joins {
                let c = chars.last_mut().expect("checked");
                c.strokes.extend(pieces[p].strokes.iter().copied());
                c.bbox.union(&b);
            } else {
                chars.push(Glyphed {
                    strokes: pieces[p].strokes.clone(),
                    bbox: b,
                    geometry: Vec::new(),
                });
            }
        }
        let mut bbox = BBox::empty();
        let mut first = usize::MAX;
        let mut height = 0.0f64;
        for c in &mut chars {
            c.geometry = c.strokes.iter().map(|s| strokes[*s].points.clone()).collect();
            bbox.union(&c.bbox);
            first = first.min(c.strokes.iter().copied().min().unwrap_or(usize::MAX));
            height = height.max(c.bbox.h());
        }
        lines.push(Line {
            chars,
            bbox,
            first,
            height,
        });
    }
    lines
}

/// Lines of one text block (a group). Lines side by side (their bands touch
/// or nearly: rotated text breaks into such lines at its spaces) join when
/// they are close; lines stacked above each other when they are of one size
/// and no more than twice the smaller line's height apart. Measured: lines
/// two heights apart are one block, 2.5 apart two; 4.6 high under 5 one
/// block, 4 high under 5 (two heights apart) two.
fn groups_of(lines: &[Line]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..lines.len()).collect();
    for a in 0..lines.len() {
        for b in a + 1..lines.len() {
            let (la, lb) = (&lines[a], &lines[b]);
            let (small, large) = (la.height.min(lb.height), la.height.max(lb.height));
            if small <= 0.0 {
                continue;
            }
            let x_gap = (la.bbox.min[0].max(lb.bbox.min[0]) - la.bbox.max[0].min(lb.bbox.max[0]))
                .max(0.0);
            let y_gap = (la.bbox.min[1].max(lb.bbox.min[1]) - la.bbox.max[1].min(lb.bbox.max[1]))
                .max(0.0);
            let joins = if y_gap <= 0.5 * small {
                small >= 0.35 * large && x_gap <= 2.0 * small
            } else {
                small >= 0.85 * large && x_gap <= 2.0 * large && y_gap <= 2.0 * small
            };
            if joins {
                union(&mut parent, a, b);
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..lines.len() {
        groups.entry(find(&mut parent, i)).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    // In the order the geometry was drawn; a block's lines top down.
    out.sort_by_key(|g| g.iter().map(|l| lines[*l].first).min().unwrap_or(0));
    for g in &mut out {
        g.sort_by(|a, b| lines[*b].bbox.max[1].total_cmp(&lines[*a].bbox.max[1]));
    }
    out
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    // The lower middle: round letters overshoot the cap height.
    Some(v[(v.len() - 1) / 2])
}

/// Height the reference gives a line that has nothing with a height in it
/// (a lone dash), measured.
const FLAT_LINE_HEIGHT: f64 = 0.15;

struct LineMatch {
    /// Recognised glyph per character (None: not recognised).
    glyphs: Vec<Option<usize>>,
    height: f64,
    /// The created text's height: the mean of the characters' sizes, a
    /// round letter's overshoot included (measured: nine letters 4.996 and
    /// an O 5.138 high read 5.0102).
    text_height: f64,
    width_factor: f64,
}

fn match_line(line: &Line, font: &FontShapes) -> LineMatch {
    // Pass 1: shapes alone, each in its own box. A shape that fits several
    // glyphs (o / O, x / X) keeps them all as candidates.
    let mut first: Vec<Vec<(usize, f64)>> = Vec::with_capacity(line.chars.len());
    for c in &line.chars {
        let norm = normalized(&c.geometry, &c.bbox);
        let samples = sample(&norm, STEP);
        let mut scored: Vec<(usize, f64)> = font
            .glyphs
            .iter()
            .enumerate()
            .map(|(gi, g)| (gi, chamfer(&samples, &norm, &g.norm_samples, &g.norm)))
            .collect();
        scored.sort_by(|a, b| a.1.total_cmp(&b.1));
        let best = scored.first().map_or(f64::INFINITY, |b| b.1);
        // Wide enough to keep the 0 of a lone circle among the rounder
        // readings (° and o fit it closer when they are compared).
        scored.retain(|(_, d)| *d <= (1.3 * best + 0.015).min(0.12));
        first.push(scored);
    }
    // The line's scale: the one most characters agree with through one of
    // their candidates (those with a height of their own).
    let hyps: Vec<Vec<(usize, f64)>> = line
        .chars
        .iter()
        .zip(&first)
        .map(|(c, cands)| {
            cands
                .iter()
                .filter(|(gi, _)| font.glyphs[*gi].bbox.h() > 0.2)
                .map(|(gi, _)| (*gi, c.bbox.h() / font.glyphs[*gi].bbox.h()))
                .collect()
        })
        .collect();
    let agree = |s: f64| -> Vec<(usize, usize)> {
        hyps.iter()
            .enumerate()
            .filter_map(|(ci, hs)| {
                hs.iter()
                    .filter(|(_, h)| (h / s - 1.0).abs() <= 0.06)
                    // Readings of one height (0 / O) go to the lower code.
                    .min_by(|a, b| {
                        (a.1 / s - 1.0)
                            .abs()
                            .total_cmp(&(b.1 / s - 1.0).abs())
                            .then(font.glyphs[a.0].ch.cmp(&font.glyphs[b.0].ch))
                    })
                    .map(|(gi, _)| (ci, *gi))
            })
            .collect()
    };
    // On a tie the reading at cap height wins: a lone round shape is an O
    // (or a 0) rather than an o, a row of bars I rather than |.
    let mut support: Vec<(usize, usize)> = Vec::new();
    let mut support_cap = f64::INFINITY;
    for hs in &hyps {
        for (gi, h) in hs {
            let votes = agree(*h);
            let cap = (font.glyphs[*gi].bbox.h() - 1.0).abs();
            if votes.len() > support.len()
                || (votes.len() == support.len() && cap < support_cap - 0.05)
            {
                support = votes;
                support_cap = cap;
            }
        }
    }
    let sizes: Vec<f64> = support
        .iter()
        .map(|(ci, gi)| line.chars[*ci].bbox.h() / font.glyphs[*gi].bbox.h())
        .collect();
    let text_height = sizes.iter().sum::<f64>() / sizes.len().max(1) as f64;
    let height = median(sizes);
    let Some(height) = height.filter(|h| *h > 0.0) else {
        // Nothing with a height: the reference's flat-line height, flat
        // glyphs (a dash) taken as they are. Anything else here (a frame
        // that looks like a scaled-up dot) is no character.
        let flat = |b: &BBox| b.h() <= 0.15 * b.w();
        let glyphs = line
            .chars
            .iter()
            .zip(&first)
            .map(|(c, cands)| {
                cands
                    .first()
                    .filter(|(gi, d)| {
                        *d <= MATCH_TOLERANCE && flat(&c.bbox) && flat(&font.glyphs[*gi].bbox)
                    })
                    .map(|(gi, _)| *gi)
            })
            .collect();
        return LineMatch {
            glyphs,
            height: FLAT_LINE_HEIGHT,
            text_height: FLAT_LINE_HEIGHT,
            width_factor: 1.0,
        };
    };
    let baseline = median(
        support
            .iter()
            .map(|(ci, gi)| line.chars[*ci].bbox.min[1] - height * font.glyphs[*gi].bbox.min[1])
            .collect(),
    )
    .unwrap_or(line.bbox.min[1]);
    let mut width_factor = median(
        support
            .iter()
            .filter(|(_, gi)| font.glyphs[*gi].bbox.w() > 0.2)
            .map(|(ci, gi)| line.chars[*ci].bbox.w() / (height * font.glyphs[*gi].bbox.w()))
            .collect(),
    )
    .unwrap_or(1.0);
    if (width_factor - 1.0).abs() < 0.03 {
        width_factor = 1.0;
    }
    // Pass 2: every glyph at the line's size, on its baseline, centred on
    // the character.
    let step = STEP * height;
    let glyphs = line
        .chars
        .iter()
        .map(|c| {
            let samples = sample(&c.geometry, step);
            let mut scores: Vec<(usize, f64)> = Vec::new();
            for (gi, g) in font.glyphs.iter().enumerate() {
                let gh = g.bbox.h() * height;
                let gw = g.bbox.w() * height * width_factor;
                if (gh - c.bbox.h()).abs() > 0.25 * height
                    || (gw - c.bbox.w()).abs() > 0.25 * height
                {
                    continue;
                }
                let gcx = g.bbox.cx();
                let placed: Vec<Vec<P>> = g
                    .strokes
                    .iter()
                    .map(|s| {
                        s.iter()
                            .map(|p| {
                                [
                                    c.bbox.cx() + (p[0] - gcx) * height * width_factor,
                                    baseline + p[1] * height,
                                ]
                            })
                            .collect()
                    })
                    .collect();
                let d = chamfer(&samples, &c.geometry, &sample(&placed, step), &placed) / height;
                scores.push((gi, d));
            }
            // Look-alikes (H and Greek Eta, y and a Cyrillic u drawn from
            // another font) go to the lowest code, Latin before the rest.
            let least = scores.iter().map(|(_, d)| *d).fold(f64::INFINITY, f64::min);
            scores
                .iter()
                .find(|(_, d)| *d <= least + 0.003)
                .filter(|(_, d)| *d <= MATCH_TOLERANCE)
                .map(|(gi, _)| *gi)
        })
        .collect();
    LineMatch {
        glyphs,
        height,
        text_height,
        width_factor,
    }
}

fn escape_mtext(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '{' | '}') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The text of a matched line: recognised characters, with as many spaces
/// as fit the pen gaps between them (an unrecognised character is a gap).
fn line_text(line: &Line, m: &LineMatch, font: &FontShapes) -> Option<(String, BBox)> {
    let scale = m.height * m.width_factor;
    let mut text = String::new();
    let mut ink = BBox::empty();
    let mut pen_end: Option<f64> = None;
    for (c, g) in line.chars.iter().zip(&m.glyphs) {
        let Some(gi) = g else { continue };
        let g = &font.glyphs[*gi];
        let origin = c.bbox.cx() - g.bbox.cx() * scale;
        if let Some(end) = pen_end {
            let space = font.word * scale;
            if space > 0.0 {
                let n = ((origin - end) / space).round();
                for _ in 0..(n.max(0.0) as usize).min(64) {
                    text.push(' ');
                }
            }
        }
        text.push(g.ch);
        pen_end = Some(origin + g.advance * scale);
        ink.union(&c.bbox);
    }
    if text.is_empty() {
        return None;
    }
    let body = escape_mtext(&text);
    let value = if m.width_factor != 1.0 {
        format!("{{\\W{:.6};{}}}", m.width_factor, body)
    } else {
        body
    };
    Some((value, ink))
}

/// The strokes of `strokes` but those of an object (a frame, a border line)
/// whose extents' diagonal is more than five times the mean diagonal of the
/// selected objects: such an object is no character and joins no group.
/// Measured: with a 200 × 100 frame among 66 text curves a line 39 long
/// still becomes "-" and one 39.2 long is left; twice the drawing, 78 and
/// 78.4; one text line alone (the line counted in), five times the mean.
fn without_oversized(strokes: &[Stroke]) -> Vec<Stroke> {
    let mut boxes: std::collections::HashMap<Handle, BBox> = Default::default();
    for s in strokes {
        boxes.entry(s.handle).or_insert_with(BBox::empty).union(&BBox::of(&s.points));
    }
    let diagonal = |b: &BBox| b.w().hypot(b.h());
    let mean = boxes.values().map(diagonal).sum::<f64>() / boxes.len().max(1) as f64;
    strokes
        .iter()
        .filter(|s| diagonal(&boxes[&s.handle]) <= 5.0 * mean)
        .cloned()
        .collect()
}

fn percent(recognised: usize, total: usize) -> u32 {
    if total == 0 {
        0
    } else {
        (100.0 * recognised as f64 / total as f64).round() as u32
    }
}

/// Recognise the text in `strokes` with the fonts of `settings`; `resolve`
/// turns a listed font into what `Face::resolve` loads (`None`: the font
/// is not available and is not compared).
pub fn recognize(
    strokes: &[Stroke],
    settings: &ShxTextSettings,
    resolve: impl Fn(&str) -> Option<String>,
) -> Recognition {
    let strokes = &without_oversized(strokes);
    let pieces = pieces_of(strokes);
    let lines = lines_of(strokes, pieces);
    let groups = groups_of(&lines);
    let fonts: Vec<(String, FontShapes)> = settings
        .fonts
        .iter()
        .filter(|f| f.enabled)
        .filter_map(|f| {
            let spec = resolve(&f.name)?;
            font_shapes(&spec).map(|shapes| (f.name.clone(), shapes))
        })
        .collect();
    let mut out = Recognition {
        groups: Vec::new(),
        lines: Vec::new(),
    };
    for group in groups {
        let chars: usize = group.iter().map(|l| lines[*l].chars.len()).sum();
        // The share is of the selected objects a font recognises (measured:
        // 9 of 14 curves read 64%, 12 of 15 read 80%), not of characters.
        let objects = |recognised: &dyn Fn(usize, usize) -> bool| -> usize {
            let mut handles: Vec<Handle> = group
                .iter()
                .enumerate()
                .flat_map(|(k, l)| {
                    lines[*l].chars.iter().enumerate().map(move |(c, ch)| (k, c, ch))
                })
                .filter(|(k, c, _)| recognised(*k, *c))
                .flat_map(|(_, _, ch)| ch.strokes.iter().map(|s| strokes[*s].handle))
                .collect();
            handles.sort();
            handles.dedup();
            handles.len()
        };
        let total = objects(&|_, _| true);
        let mut best: Option<(usize, u32, Vec<LineMatch>)> = None;
        let mut chosen: Option<(usize, u32, Vec<LineMatch>)> = None;
        for (fi, (_, shapes)) in fonts.iter().enumerate() {
            let matches: Vec<LineMatch> = group.iter().map(|l| match_line(&lines[*l], shapes)).collect();
            let recognised = objects(&|k, c| matches[k].glyphs[c].is_some());
            let pct = percent(recognised, total);
            if pct >= u32::from(settings.threshold) && !settings.best_font {
                chosen = Some((fi, pct, matches));
                break;
            }
            if best.as_ref().is_none_or(|(_, bp, _)| pct > *bp) {
                best = Some((fi, pct, matches));
            }
        }
        // A lone shape no font recognises (a frame, a boundary) is not text:
        // it is no group.
        if chosen.is_none() && chars == 1 && best.as_ref().is_none_or(|(_, pct, _)| *pct == 0) {
            continue;
        }
        if chosen.is_none()
            && best
                .as_ref()
                .is_some_and(|(_, pct, _)| *pct >= u32::from(settings.threshold))
        {
            chosen = best.take();
        }
        match chosen {
            Some((fi, pct, matches)) => {
                let (spec, shapes) = &fonts[fi];
                out.groups.push(GroupReport {
                    font: font_label(spec),
                    percent: pct,
                    converted: true,
                });
                for (l, m) in group.iter().zip(&matches) {
                    let line = &lines[*l];
                    let Some((value, ink)) = line_text(line, m, shapes) else {
                        continue;
                    };
                    let handles: Vec<Handle> = line
                        .chars
                        .iter()
                        .zip(&m.glyphs)
                        .filter(|(_, g)| g.is_some())
                        .flat_map(|(c, _)| c.strokes.iter().map(|s| strokes[*s].handle))
                        .collect();
                    out.lines.push(TextLine {
                        value,
                        insertion: ink.min,
                        height: m.text_height,
                        font: font_label(spec),
                        font_spec: spec.clone(),
                        layer: strokes[line.first].layer.clone(),
                        handles,
                    });
                }
            }
            None => {
                let (font, pct) = best
                    .map(|(fi, pct, _)| (font_label(&fonts[fi].0), pct))
                    .unwrap_or_else(|| {
                        (
                            fonts.first().map(|f| font_label(&f.0)).unwrap_or_default(),
                            0,
                        )
                    });
                out.groups.push(GroupReport {
                    font,
                    percent: pct,
                    converted: false,
                });
            }
        }
    }
    out
}

/// The curves of `entity` as world-XY strokes (text and other objects: none).
pub fn entity_strokes(entity: &document::EntityType) -> Vec<Vec<P>> {
    use document::EntityType as E;
    match entity {
        E::Line(_)
        | E::Arc(_)
        | E::Circle(_)
        | E::Ellipse(_)
        | E::LwPolyline(_)
        | E::Polyline2D(_)
        | E::Spline(_) => crate::entities::curve::entity_curve(entity)
            .map(|c| {
                vec![crate::entities::curve::curve_points(&c)
                    .into_iter()
                    .map(|p| [p[0], p[1]])
                    .collect()]
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

// ── Commands ───────────────────────────────────────────────────────────────

/// FILEDIA 0 form of the settings, between PDFSHXTEXT's selection prompts.
pub struct ShxSettingsCommand {
    step: SettingsStep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsStep {
    Option,
    Fonts,
    Threshold,
    Layer,
    BestFont,
}

impl ShxSettingsCommand {
    pub fn new() -> Self {
        Self {
            step: SettingsStep::Option,
        }
    }

    fn back(&mut self) -> CmdResult {
        self.step = SettingsStep::Option;
        CmdResult::NeedPoint
    }
}

impl CadCommand for ShxSettingsCommand {
    fn name(&self) -> &'static str {
        "PDFSHXTEXT"
    }

    fn prompt(&self) -> String {
        let s = settings();
        match self.step {
            SettingsStep::Option => {
                "Enter a recognition setting [Fonts/Threshold/Layer/Bestfont] <exit>:".to_string()
            }
            SettingsStep::Fonts => format!(
                "Enter the fonts to compare, in order <{}>:",
                s.fonts
                    .iter()
                    .filter(|f| f.enabled)
                    .map(|f| f.label())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            SettingsStep::Threshold => {
                format!("Enter recognition threshold (1-100) <{}>:", s.threshold)
            }
            SettingsStep::Layer => format!(
                "Create text on [Current/Geometry] layer <{}>:",
                if s.current_layer { "Current" } else { "Geometry" }
            ),
            SettingsStep::BestFont => format!(
                "Use best matching font? [Yes/No] <{}>:",
                if s.best_font { "Yes" } else { "No" }
            ),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            SettingsStep::Option => vec![
                CmdOption::new("Fonts", "F"),
                CmdOption::new("Threshold", "T"),
                CmdOption::new("Layer", "L"),
                CmdOption::new("Bestfont", "B"),
            ],
            SettingsStep::Layer => vec![
                CmdOption::new("Current", "C"),
                CmdOption::new("Geometry", "G"),
            ],
            SettingsStep::BestFont => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let token = text.trim().trim_start_matches('_').to_ascii_uppercase();
        if token.is_empty() {
            return Some(self.on_enter());
        }
        let mut s = settings();
        let invalid = || Some(CmdResult::ReportError("Invalid option keyword.".to_string()));
        match self.step {
            SettingsStep::Option => {
                self.step = match token.as_str() {
                    "F" | "FONTS" => SettingsStep::Fonts,
                    "T" | "THRESHOLD" => SettingsStep::Threshold,
                    "L" | "LAYER" => SettingsStep::Layer,
                    "B" | "BESTFONT" => SettingsStep::BestFont,
                    _ => return invalid(),
                };
                return Some(CmdResult::NeedPoint);
            }
            SettingsStep::Fonts => {
                let wanted: Vec<String> = text
                    .split([',', ' ', ';'])
                    .map(|n| n.trim().to_ascii_lowercase())
                    .filter(|n| !n.is_empty())
                    .collect();
                if let Some(bad) = wanted
                    .iter()
                    .find(|n| !s.fonts.iter().any(|f| f.label().eq_ignore_ascii_case(n)))
                {
                    return Some(CmdResult::ReportError(format!("Font \"{bad}\" is not in the list.")));
                }
                // The named fonts first, in the order given and enabled; the
                // rest after them, off.
                let mut fonts: Vec<ShxFontEntry> = Vec::new();
                for n in &wanted {
                    if let Some(f) = s.fonts.iter().find(|f| f.label().eq_ignore_ascii_case(n)) {
                        if !fonts.iter().any(|g| g.name == f.name) {
                            fonts.push(ShxFontEntry {
                                name: f.name.clone(),
                                enabled: true,
                            });
                        }
                    }
                }
                for f in &s.fonts {
                    if !fonts.iter().any(|g| g.name == f.name) {
                        fonts.push(ShxFontEntry {
                            name: f.name.clone(),
                            enabled: false,
                        });
                    }
                }
                s.fonts = fonts;
            }
            SettingsStep::Threshold => match token.parse::<f64>() {
                Ok(v) if (1.0..=100.0).contains(&v) => s.threshold = v.round() as u8,
                _ => {
                    return Some(CmdResult::ReportError(
                        "Requires an integer between 1 and 100.".to_string(),
                    ))
                }
            },
            SettingsStep::Layer => match token.as_str() {
                "C" | "CURRENT" => s.current_layer = true,
                "G" | "GEOMETRY" => s.current_layer = false,
                _ => return invalid(),
            },
            SettingsStep::BestFont => match token.as_str() {
                "Y" | "YES" => s.best_font = true,
                "N" | "NO" => s.best_font = false,
                _ => return invalid(),
            },
        }
        set_settings(s);
        // The host saves the profile and asks for the next setting.
        Some(CmdResult::Dispatch(SAVE_SETTINGS.to_string()))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.step == SettingsStep::Option {
            CmdResult::Dispatch(RESUME.to_string())
        } else {
            self.back()
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Private commands: back to the selection prompt after the settings, and
/// persisting a setting typed on the command line.
pub const RESUME: &str = "_PDFSHXTEXTRESUME";
pub const SAVE_SETTINGS: &str = "_PDFSHXTEXTSAVE";
pub const OPEN_SETTINGS: &str = "_PDFSHXTEXTSETUP";
pub const APPLY: &str = "_PDFSHXTEXTAPPLY";

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFSHXTEXT", "PDFSHXLAYER", "PDFSHXBESTFONT", "PDFSHXTEXTSETTINGS"]
});
