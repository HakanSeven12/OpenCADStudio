// TrueType/OpenType glyph → CAD wire strokes.
//
// Pulls a system font's bytes from `sysfont`, extracts a glyph's outline with
// `ttf-parser`, and flattens the quadratic/cubic Bézier contours into polyline
// strokes — the same `lff::Glyph` shape the LFF stroke engine produces, so the
// text layout code can consume either without caring which font kind it is.
//
// Coordinates are normalized to the **same 9-unit cap-height space the LFF
// engine uses**: a capital letter is 9 units tall with the baseline at y = 0.
// That makes a TTF glyph a drop-in for an LFF one — the existing text layout
// scales both by `height / 9.0`, so TTF and stroke text share one pipeline.
// Cap height comes from the font's OS/2 table; if absent we approximate it as
// 0.7 × units-per-em.
//
// Unlike LFF single-stroke glyphs, TTF contours are closed outlines (the glyph
// boundary). Counters — the hole in "O" or "A" — come through as separate
// closed contours; the wire renderer simply draws every contour.

use crate::scene::text::lff::Glyph;
use crate::scene::text::sysfont;
use rustc_hash::FxHashMap as HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use lyon_tessellation::math::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};

/// Bézier flattening step counts. Outlines are small on screen most of the
/// time; these are a fixed budget that keeps curves smooth without exploding
/// vertex counts. Cubic gets more steps because OTF/CFF curves swing wider.
const QUAD_STEPS: usize = 8;
const CUBIC_STEPS: usize = 12;

/// Glyph-unit cap height the layout scales by `height / 9.0`.
const CAP_UNITS: f32 = 9.0;

/// Collects `ttf-parser` outline callbacks into closed contours, normalized so
/// the font's cap height equals [`CAP_UNITS`].
struct OutlineFlattener {
    /// Font-unit → 9-unit-cap-height scale factor.
    k: f32,
    /// Pen position (9-unit) added to every vertex — lets a shaped glyph carry
    /// its run offset. Zero for a standalone glyph.
    offset: [f32; 2],
    contours: Vec<Vec<[f32; 2]>>,
    cur: Vec<[f32; 2]>,
    start: [f32; 2],
    pos: [f32; 2],
}

impl OutlineFlattener {
    fn new(k: f32) -> Self {
        OutlineFlattener {
            k,
            offset: [0.0, 0.0],
            contours: Vec::new(),
            cur: Vec::new(),
            start: [0.0, 0.0],
            pos: [0.0, 0.0],
        }
    }

    fn n(&self, x: f32, y: f32) -> [f32; 2] {
        [x * self.k + self.offset[0], y * self.k + self.offset[1]]
    }

    fn flush(&mut self) {
        if self.cur.len() >= 2 {
            self.contours.push(std::mem::take(&mut self.cur));
        } else {
            self.cur.clear();
        }
    }
}

impl ttf_parser::OutlineBuilder for OutlineFlattener {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        let p = self.n(x, y);
        self.start = p;
        self.pos = p;
        self.cur.push(p);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.n(x, y);
        self.pos = p;
        self.cur.push(p);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let p0 = self.pos;
        let c = self.n(cx, cy);
        let p = self.n(x, y);
        for i in 1..=QUAD_STEPS {
            let t = i as f32 / QUAD_STEPS as f32;
            let u = 1.0 - t;
            let bx = u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p[0];
            let by = u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p[1];
            self.cur.push([bx, by]);
        }
        self.pos = p;
    }

    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let p0 = self.pos;
        let c1 = self.n(c1x, c1y);
        let c2 = self.n(c2x, c2y);
        let p = self.n(x, y);
        for i in 1..=CUBIC_STEPS {
            let t = i as f32 / CUBIC_STEPS as f32;
            let u = 1.0 - t;
            let bx = u * u * u * p0[0]
                + 3.0 * u * u * t * c1[0]
                + 3.0 * u * t * t * c2[0]
                + t * t * t * p[0];
            let by = u * u * u * p0[1]
                + 3.0 * u * u * t * c1[1]
                + 3.0 * u * t * t * c2[1]
                + t * t * t * p[1];
            self.cur.push([bx, by]);
        }
        self.pos = p;
    }

    fn close(&mut self) {
        // Close the ring back to its start so the wire forms a loop.
        if self.cur.first().map_or(false, |f| *f != self.pos) {
            self.cur.push(self.start);
        }
        self.flush();
    }
}

// ── Hint-reliant ("tricky") fonts ───────────────────────────────────────────
//
//
// DFKai-SB, MingLiU and other early DynaLab fonts build each ideograph from
// stroke components that only the TrueType instructions move into place, so
// the raw `glyf` outline is missing strokes. FreeType calls them "tricky".
// For those fonts we run skrifa's TrueType interpreter at a large ppem and
// scale the result back to font units; every other font keeps ttf-parser.

/// ppem the interpreter hints at: large enough that grid rounding is only
/// 1/512 em, so the hinted outline stays close to the design.
const HINT_PPEM: f32 = 512.0;

/// Identifies one face without hashing its bytes. fontdb may hand out a fresh
/// buffer on every `with_face_data`, so the slice address is not stable.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FaceKey {
    len: usize,
    index: u32,
    checksum_adjustment: u32,
}

/// `Some(instance)` for a font that needs the interpreter, `None` otherwise.
type HintCache = HashMap<FaceKey, Option<Arc<skrifa::outline::HintingInstance>>>;

fn hint_cache() -> &'static Mutex<HintCache> {
    static CACHE: OnceLock<Mutex<HintCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::default()))
}

/// Hinting instance for `font`, or `None` when it renders correctly unhinted.
/// `require_interpreter` reads the name table and may checksum tables, so the
/// answer (and the instance) is cached per face.
fn hinter_for(
    font: &skrifa::FontRef,
    key: FaceKey,
) -> Option<Arc<skrifa::outline::HintingInstance>> {
    use skrifa::instance::{LocationRef, Size};
    use skrifa::outline::{Engine, HintingInstance, HintingOptions, Target};
    use skrifa::MetadataProvider;

    if let Some(hit) = hint_cache().lock().unwrap().get(&key) {
        return hit.clone();
    }
    let outlines = font.outline_glyphs();
    let built = if outlines.require_interpreter() {
        // FreeType's recipe for tricky fonts: the bytecode interpreter in
        // monochrome mode.
        let options = HintingOptions {
            engine: Engine::Interpreter,
            target: Target::Mono,
        };
        match HintingInstance::new(
            &outlines,
            Size::new(HINT_PPEM),
            LocationRef::default(),
            options,
        ) {
            Ok(instance) => Some(Arc::new(instance)),
            Err(e) => {
                // Logged once per face (the `None` is cached): the font falls
                // back to its raw outlines, which may be missing strokes.
                log::warn!(
                    "TrueType hinting failed for a hint-reliant font; using raw outlines: {e:?}"
                );
                None
            }
        }
    } else {
        None
    };
    hint_cache().lock().unwrap().insert(key, built.clone());
    built
}

/// One recorded path command, already scaled to font units.
enum PathCmd {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// skrifa pen that records hinted pixel coordinates as font units, so a draw
/// that fails halfway never leaves a partial glyph in the real builder.
struct RecordingPen {
    /// Pixel → font-unit factor (`upem / HINT_PPEM`).
    k: f32,
    cmds: Vec<PathCmd>,
}

impl skrifa::outline::OutlinePen for RecordingPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.cmds.push(PathCmd::Move(x * self.k, y * self.k));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.cmds.push(PathCmd::Line(x * self.k, y * self.k));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let k = self.k;
        self.cmds.push(PathCmd::Quad(cx * k, cy * k, x * k, y * k));
    }

    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let k = self.k;
        self.cmds.push(PathCmd::Cubic(
            c1x * k,
            c1y * k,
            c2x * k,
            c2y * k,
            x * k,
            y * k,
        ));
    }

    fn close(&mut self) {
        self.cmds.push(PathCmd::Close);
    }
}

/// Hinted outline of `gid` in font units, or `None` when the font needs no
/// interpreter (or hinting failed) and the caller should use ttf-parser.
fn hinted_outline(data: &[u8], index: u32, gid: ttf_parser::GlyphId) -> Option<Vec<PathCmd>> {
    use skrifa::outline::DrawSettings;
    use skrifa::raw::TableProvider;
    use skrifa::MetadataProvider;

    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let head = font.head().ok()?;
    let key = FaceKey {
        len: data.len(),
        index,
        checksum_adjustment: head.checksum_adjustment(),
    };
    let hinter = hinter_for(&font, key)?;
    let glyph = font
        .outline_glyphs()
        .get(skrifa::GlyphId::new(gid.0 as u32))?;
    let mut pen = RecordingPen {
        k: head.units_per_em().max(1) as f32 / HINT_PPEM,
        cmds: Vec::new(),
    };
    glyph
        .draw(DrawSettings::hinted(&hinter, false), &mut pen)
        .ok()?;
    Some(pen.cmds)
}

/// Outline glyph `gid` of face `index` in `data` into `builder` (font units,
/// y-up) — the drop-in replacement for `face.outline_glyph` used everywhere in
/// this file. Hint-reliant fonts go through the TrueType interpreter first.
fn outline_glyph_into(
    data: &[u8],
    index: u32,
    face: &ttf_parser::Face,
    gid: ttf_parser::GlyphId,
    builder: &mut impl ttf_parser::OutlineBuilder,
) {
    let Some(cmds) = hinted_outline(data, index, gid) else {
        face.outline_glyph(gid, builder);
        return;
    };
    for cmd in cmds {
        match cmd {
            PathCmd::Move(x, y) => builder.move_to(x, y),
            PathCmd::Line(x, y) => builder.line_to(x, y),
            PathCmd::Quad(cx, cy, x, y) => builder.quad_to(cx, cy, x, y),
            PathCmd::Cubic(c1x, c1y, c2x, c2y, x, y) => builder.curve_to(c1x, c1y, c2x, c2y, x, y),
            PathCmd::Close => builder.close(),
        }
    }
}

// ── Cache ────────────────────────────────────────────────────────────────────

type GlyphCache = HashMap<(String, char), Option<Arc<Glyph>>>;

fn cache() -> &'static Mutex<GlyphCache> {
    static CACHE: OnceLock<Mutex<GlyphCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::default()))
}

/// Em-normalized outline for one character of a system font family. Returns
/// `None` if the family is not installed or the glyph is missing. Result is
/// cached per `(family, char)`.
pub fn glyph(family: &str, ch: char) -> Option<Arc<Glyph>> {
    let key = (family.to_string(), ch);
    if let Some(hit) = cache().lock().unwrap().get(&key) {
        return hit.clone();
    }

    let built = sysfont::with_face_data(family, |data, index| {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let gid = face.glyph_index(ch)?;
        let k = cap_scale(&face);

        let advance = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * k;
        let mut fl = OutlineFlattener::new(k);
        // A glyph with no outline (e.g. space) still has a valid advance.
        outline_glyph_into(data, index, &face, gid, &mut fl);
        fl.flush();
        let fill_tris = triangulate_contours(&fl.contours);

        Some(Arc::new(Glyph {
            strokes: fl.contours,
            advance,
            fill_tris,
        }))
    })
    .flatten();

    cache().lock().unwrap().insert(key, built.clone());
    built
}

/// Font-unit → 9-unit-em-box factor: the whole em square maps onto the text
/// height. This is how an SHX big font (`chineset.shx`, `hztxt.shx`, …)
/// sizes its ideographs — a CJK glyph is as tall as the text height and one
/// text height wide — so a TrueType glyph standing in for a missing big-font
/// glyph must use the same box, not the Latin cap height. Cap-height scaling
/// makes ideographs ~1.3–1.5× too big (1 em ≈ 1.3–1.5 cap heights in CJK
/// fonts), so every line of substituted Chinese text ran past its frame.
fn em_scale(face: &ttf_parser::Face) -> f32 {
    CAP_UNITS / face.units_per_em().max(1) as f32
}

/// Ideographic / full-width characters: the ones a big font would supply and
/// that sit on an em box rather than the Latin cap height.
pub(crate) fn is_full_width(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF       // Hangul Jamo
        | 0x2E80..=0x2FDF     // CJK / Kangxi radicals
        | 0x2FF0..=0x303F     // ideographic description, CJK symbols & punctuation
        | 0x3040..=0x30FF     // Hiragana, Katakana
        | 0x3100..=0x312F     // Bopomofo
        | 0x3130..=0x318F     // Hangul compatibility Jamo
        | 0x3190..=0x31FF     // Kanbun, Bopomofo ext., CJK strokes, Katakana ext.
        | 0x3200..=0x33FF     // enclosed CJK, CJK compatibility
        | 0x3400..=0x4DBF     // CJK ext. A
        | 0x4E00..=0x9FFF     // CJK unified ideographs
        | 0xA960..=0xA97F     // Hangul Jamo ext. A
        | 0xAC00..=0xD7FF     // Hangul syllables, Jamo ext. B
        | 0xF900..=0xFAFF     // CJK compatibility ideographs
        | 0xFE30..=0xFE4F     // CJK compatibility forms
        | 0xFF01..=0xFF60     // full-width ASCII variants
        | 0xFFE0..=0xFFE6     // full-width symbols
        | 0x20000..=0x3FFFF   // CJK ext. B–H
    )
}

/// Font-unit → 9-unit-cap-height factor for a parsed face. Cap height comes
/// from the OS/2 table; absent, we approximate it as 0.7 × units-per-em.
fn cap_scale(face: &ttf_parser::Face) -> f32 {
    let upem = face.units_per_em() as f32;
    let cap = face
        .capital_height()
        .filter(|&c| c > 0)
        .map(|c| c as f32)
        .unwrap_or(0.7 * upem);
    CAP_UNITS / cap
}

fn triangulate_contours(contours: &[Vec<[f32; 2]>]) -> Vec<[f32; 2]> {
    if contours.is_empty() {
        return Vec::new();
    }

    let mut builder = Path::builder();
    for contour in contours {
        if contour.len() < 3 {
            continue;
        }
        builder.begin(point(contour[0][0], contour[0][1]));
        for p in &contour[1..] {
            builder.line_to(point(p[0], p[1]));
        }
        builder.end(true);
    }
    let path = builder.build();

    let mut geometry: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let mut tessellator = FillTessellator::new();
    // TrueType outlines use the nonzero winding rule; lyon's default even-odd
    // rule mis-fills glyphs whose contours overlap. On a tessellation failure
    // the glyph falls back to an empty fill silently — per-glyph logging would
    // otherwise spam stderr once per character for a malformed font.
    if tessellator
        .tessellate_path(
            &path,
            &FillOptions::default().with_fill_rule(FillRule::NonZero),
            &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                vertex.position().to_array()
            }),
        )
        .is_err()
    {
        return Vec::new();
    }

    let mut tris = Vec::with_capacity(geometry.indices.len());
    for &idx in &geometry.indices {
        if let Some(&p) = geometry.vertices.get(idx as usize) {
            tris.push(p);
        }
    }
    tris
}

// ── Glyph source (for exporters that embed the font) ─────────────────────────

/// One font file's bytes, shared by every glyph taken from it.
#[derive(Debug)]
pub struct FontBlob {
    /// The whole font file (a collection keeps all its faces).
    pub data: Vec<u8>,
    /// Face index within `data` (0 for a plain .ttf/.otf).
    pub index: u32,
    /// Stable identity: byte length, face index, `head.checkSumAdjustment`.
    pub id: (usize, u32, u32),
}

/// The TrueType face and glyph a filled glyph was outlined from.
///
/// The PDF exporter embeds `font` and draws the glyph as text instead of
/// filling the tessellated outline: a glyph of a stroke-heavy CJK face
/// (DFKai-SB) tessellates into hundreds of triangles, so a text-heavy sheet
/// exported as hundreds of megabytes that viewers crawl through.
#[derive(Clone, Debug)]
pub struct GlyphSource {
    pub font: Arc<FontBlob>,
    pub gid: u16,
    /// Font-unit -> 9-unit factor the outline was normalised with
    /// ([`cap_scale`] or [`em_scale`]) -- the exporter needs it to place the
    /// embedded glyph exactly over the outline's tile.
    pub k: f32,
    pub units_per_em: u16,
}

#[cfg(not(target_arch = "wasm32"))]
fn face_key(data: &[u8], index: u32) -> Option<FaceKey> {
    use skrifa::raw::TableProvider;
    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let head = font.head().ok()?;
    Some(FaceKey {
        len: data.len(),
        index,
        checksum_adjustment: head.checksum_adjustment(),
    })
}

/// Copy each font file once per process, however many glyphs come from it.
#[cfg(not(target_arch = "wasm32"))]
fn blob_for(data: &[u8], index: u32) -> Option<Arc<FontBlob>> {
    static BLOBS: OnceLock<Mutex<HashMap<FaceKey, Arc<FontBlob>>>> = OnceLock::new();
    let key = face_key(data, index)?;
    let mut blobs = BLOBS
        .get_or_init(|| Mutex::new(HashMap::default()))
        .lock()
        .unwrap();
    Some(
        blobs
            .entry(key)
            .or_insert_with(|| {
                Arc::new(FontBlob {
                    data: data.to_vec(),
                    index,
                    id: (key.len, key.index, key.checksum_adjustment),
                })
            })
            .clone(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn source_for(
    data: &[u8],
    index: u32,
    face: &ttf_parser::Face,
    gid: ttf_parser::GlyphId,
    k: f32,
) -> Option<GlyphSource> {
    Some(GlyphSource {
        font: blob_for(data, index)?,
        gid: gid.0,
        k,
        units_per_em: face.units_per_em(),
    })
}

/// Source of [`glyph`]`(family, ch)`: same face, same glyph, same scale.
/// Cached per `(family, char)`, like the glyph itself.
#[cfg(not(target_arch = "wasm32"))]
pub fn glyph_source(family: &str, ch: char) -> Option<GlyphSource> {
    type SourceCache = HashMap<(String, char), Option<GlyphSource>>;
    static CACHE: OnceLock<Mutex<SourceCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::default()));
    let key = (family.to_string(), ch);
    if let Some(hit) = cache.lock().unwrap().get(&key) {
        return hit.clone();
    }
    let built = sysfont::with_face_data(family, |data, index| {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let gid = face.glyph_index(ch)?;
        source_for(data, index, &face, gid, cap_scale(&face))
    })
    .flatten();
    cache.lock().unwrap().insert(key, built.clone());
    built
}

/// Source of [`fallback_glyph`]`(ch)`: the face the fallback search settles on.
/// Cached per character, like the glyph itself.
#[cfg(not(target_arch = "wasm32"))]
pub fn fallback_source(ch: char) -> Option<GlyphSource> {
    static CACHE: OnceLock<Mutex<HashMap<char, Option<GlyphSource>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::default()));
    if let Some(hit) = cache.lock().unwrap().get(&ch) {
        return hit.clone();
    }
    let built = pick_fallback(ch, true).and_then(|(_, source)| source);
    cache.lock().unwrap().insert(ch, built.clone());
    built
}

// ── Shaping ────────────────────────────────────────────────────────────────

/// One shaped glyph, positioned within its run. Strokes are in 9-unit space and
/// already carry the glyph's pen position (cumulative advance + shaping offset),
/// so the caller only applies the run's own transform.
pub struct PlacedGlyph {
    pub strokes: Vec<Vec<[f32; 2]>>,
    pub fill_tris: Vec<[f32; 2]>,
}

/// A fully shaped run.
pub struct ShapedRun {
    pub glyphs: Vec<PlacedGlyph>,
    /// Total pen advance of the run (9-unit).
    pub advance: f32,
    /// Every laid-out glyph (blank ones too) as the byte range of the text it
    /// shows and its visual [left, right] pen cell, 9-unit.
    pub cells: Vec<(usize, usize, f32, f32)>,
}

/// Each character's visual [left, right] cell (9-unit) in logical order:
/// the cell of the glyph that shows it; a glyph showing several characters
/// shares its cell among them in reading order.
pub fn char_cells(run: &ShapedRun, text: &str) -> Vec<(f32, f32)> {
    text.char_indices()
        .map(|(byte, _)| {
            let Some(&(start, end, x0, x1)) =
                run.cells.iter().find(|(start, end, _, _)| (*start..(*end).max(start + 1)).contains(&byte))
            else {
                return (0.0, 0.0);
            };
            let chars: Vec<usize> = text[start..end.min(text.len())].char_indices().map(|(i, _)| start + i).collect();
            let (n, k) = (chars.len().max(1), chars.iter().position(|&b| b == byte).unwrap_or(0));
            let rtl = text[start..end.min(text.len())].chars().any(crate::entities::text_support::is_rtl_char);
            let slot = (x1 - x0) / n as f32;
            let k = if rtl { n - 1 - k } else { k };
            (x0 + slot * k as f32, x0 + slot * (k + 1) as f32)
        })
        .collect()
}

/// Left-to-right mark, shaped ahead of every run so its base direction is
/// left to right; layout also uses it to close a right-to-left run.
pub(crate) const LRM: char = '\u{200E}';

type ShapeCache = HashMap<(String, String), Option<Arc<ShapedRun>>>;

fn shape_cache() -> &'static Mutex<ShapeCache> {
    static CACHE: OnceLock<Mutex<ShapeCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::default()))
}

/// Pixel font size the layout runs at. Positions and outlines are normalized
/// out of pixel space afterwards, so the exact value only affects rounding.
#[allow(dead_code)]
const SHAPE_FS: f32 = 1000.0;

/// Shared cosmic-text layout engine, loaded once with the system fonts. Held
/// behind a mutex because `set_text`/`shape` need `&mut FontSystem`.
///
/// Native only: on the web there are no system fonts, so cosmic-text would
/// panic with "no default font found" — the shaping / fallback paths are
/// disabled there instead (LFF stroke fonts still render).
#[cfg(not(target_arch = "wasm32"))]
fn font_system() -> &'static Mutex<cosmic_text::FontSystem> {
    static FS: OnceLock<Mutex<cosmic_text::FontSystem>> = OnceLock::new();
    FS.get_or_init(|| Mutex::new(cosmic_text::FontSystem::new()))
}

/// Lay out and shape `text` in `family` with cosmic-text — ligatures, Arabic
/// joining, kerning, bidi reordering, and automatic font fallback (a glyph the
/// chosen family lacks is taken from another installed font). Each resulting
/// glyph is outlined from its *resolved* font and normalized into 9-unit
/// cap-height space (cap height of the primary family). Cached per
/// `(family, text)`; `None` for a non-system family.
pub fn shape_run(family: &str, text: &str) -> Option<Arc<ShapedRun>> {
    if text.is_empty() {
        return None;
    }
    let key = (family.to_string(), text.to_string());
    if let Some(hit) = shape_cache().lock().unwrap().get(&key) {
        return hit.clone();
    }

    let built = build_shaped(family, text).map(Arc::new);
    shape_cache().lock().unwrap().insert(key, built.clone());
    built
}

type FallbackCache = HashMap<char, Option<Arc<Glyph>>>;

fn fallback_cache() -> &'static Mutex<FallbackCache> {
    static CACHE: OnceLock<Mutex<FallbackCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::default()))
}

/// Last-resort glyph for a character missing from a stroke (LFF) font: let
/// cosmic-text pick whichever installed font covers it, then outline it from
/// that font normalized to 9-unit cap height. Returns `None` if no system font
/// has the character. Cached per character.
///
/// The result is a filled-outline glyph, so it visually differs from the
/// surrounding single-stroke text — accepted as the price of covering scripts
/// no stroke font provides.
pub fn fallback_glyph(ch: char) -> Option<Arc<Glyph>> {
    if let Some(hit) = fallback_cache().lock().unwrap().get(&ch) {
        return hit.clone();
    }
    let built = build_fallback(ch);
    fallback_cache().lock().unwrap().insert(ch, built.clone());
    built
}

/// Drop the cached fallback glyphs so the next lookup re-resolves. The web
/// build calls this when a per-script font finishes loading: glyphs that
/// resolved to `None` while the font was still in flight then get a real
/// outline. (#141)
pub fn clear_fallback_cache() {
    fallback_cache().lock().unwrap().clear();
    shape_cache().lock().unwrap().clear();
    #[cfg(target_arch = "wasm32")]
    crate::scene::text::sdf_atlas::reset_font_entries();
}

#[cfg(test)]
mod fallback_tests {
    use super::*;

    /// A fallback that comes back with an id but no outline draws nothing while
    /// believing it drew something: the stroke fonts hand every letter they
    /// lack to this path, so on macOS a Cyrillic drawing came out blank.
    #[test]
    fn a_fallback_glyph_is_never_empty() {
        for ch in ['Ğ', 'ş', 'б', 'Я', 'Ω', '中', 'A'] {
            let Some(glyph) = fallback_glyph(ch) else {
                continue; // no font on this host covers it at all
            };
            assert!(
                !glyph.strokes.is_empty() || !glyph.fill_tris.is_empty(),
                "'{ch}' resolved to a glyph with no outline"
            );
        }
    }
}

/// Last-resort glyph for a character missing from a stroke (LFF) font: whatever
/// this machine can draw it with, normalized to 9-unit cap height, or `None`
/// when nothing installed has the character. Cached per character.
///
/// The result is a filled-outline glyph, so it visually differs from the
/// surrounding single-stroke text — accepted as the price of covering scripts
/// no stroke font provides.
#[cfg(not(target_arch = "wasm32"))]
fn build_fallback(ch: char) -> Option<Arc<Glyph>> {
    pick_fallback(ch, false).map(|(glyph, _)| glyph)
}

/// The fallback glyph for `ch` together with the face it came from -- the
/// same search for both, so the glyph drawn on screen and the font an
/// exporter embeds can never disagree.
///
/// The source holds a copy of the whole font file, so it is built only when
/// asked for (`with_source`, an exporter): drawing on screen never keeps one.
#[cfg(not(target_arch = "wasm32"))]
fn pick_fallback(ch: char, with_source: bool) -> Option<(Arc<Glyph>, Option<GlyphSource>)> {
    outline_from_fallback_face(ch, with_source).or_else(|| installed_family_glyph(ch, with_source))
}

/// Outline `ch` from a parsed face, in the 9-unit text space.
///
/// A fallback glyph stands in for a stroke / SHX font that lacks the character.
/// For an ideograph that font would have been a big font, whose glyphs fill the
/// text height — so size the substitute by its em box. Everything else keeps the
/// cap-height normalisation that lines it up with the Latin stroke glyphs.
#[cfg(not(target_arch = "wasm32"))]
fn outline_char(
    data: &[u8],
    index: u32,
    face: &ttf_parser::Face,
    ch: char,
    with_source: bool,
) -> Option<(Glyph, Option<GlyphSource>)> {
    let gid = face.glyph_index(ch)?;
    let k = if is_full_width(ch) {
        em_scale(face)
    } else {
        cap_scale(face)
    };
    let advance = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * k;
    let mut fl = OutlineFlattener::new(k);
    outline_glyph_into(data, index, face, gid, &mut fl);
    fl.flush();
    let fill_tris = triangulate_contours(&fl.contours);
    let glyph = Glyph {
        strokes: fl.contours,
        advance,
        fill_tris,
    };
    let source = with_source.then(|| source_for(data, index, face, gid, k)).flatten();
    Some((glyph, source))
}

/// Outline `ch` from the face cosmic-text picks for it: it knows the platform's
/// fallback order and shapes its way through ligatures and joining.
///
/// A face can answer with a glyph id and no outline at all — macOS's system font
/// does, for every character, because ttf-parser reads no contours out of it —
/// so an empty outline is not an answer and the caller has to keep looking.
#[cfg(not(target_arch = "wasm32"))]
fn outline_from_fallback_face(
    ch: char,
    with_source: bool,
) -> Option<(Arc<Glyph>, Option<GlyphSource>)> {
    use cosmic_text::{Attrs, Buffer, Metrics, Shaping};
    let mut fs = font_system().lock().unwrap();
    // Default family → cosmic's own fallback search chooses a covering font.
    let attrs = Attrs::new();
    let mut buf = Buffer::new(&mut fs, Metrics::new(SHAPE_FS, SHAPE_FS));
    buf.set_size(&mut fs, None, None);
    let s = ch.to_string();
    buf.set_text(&mut fs, &s, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(&mut fs, false);

    for run in buf.layout_runs() {
        for g in run.glyphs.iter() {
            if g.glyph_id == 0 {
                continue; // .notdef — this font doesn't really cover it
            }
            let face_index = fs.db_mut().face(g.font_id).map(|f| f.index).unwrap_or(0);
            let font = fs.get_font(g.font_id, g.font_weight)?;
            let face = ttf_parser::Face::parse(font.data(), face_index).ok()?;
            let Some((glyph, source)) =
                outline_char(font.data(), face_index, &face, ch, with_source)
            else {
                continue;
            };
            if glyph.strokes.is_empty() && glyph.fill_tris.is_empty() {
                continue;
            }
            return Some((Arc::new(glyph), source));
        }
    }
    None
}

/// The first installed family that actually draws `ch` — the net under
/// cosmic-text, whose fallback list is the platform's own handful of names (on
/// macOS: `.SF NS`, `Menlo`, `Apple Color Emoji`, `Geneva`, `Arial Unicode MS`)
/// and covers neither Cyrillic nor Greek, however many fonts the machine has
/// that do.
#[cfg(not(target_arch = "wasm32"))]
fn installed_family_glyph(
    ch: char,
    with_source: bool,
) -> Option<(Arc<Glyph>, Option<GlyphSource>)> {
    crate::scene::text::sysfont::families()
        .iter()
        .find_map(|family| {
            let (glyph, source) = sysfont::with_face_data(family, |data, index| {
                let face = ttf_parser::Face::parse(data, index).ok()?;
                outline_char(data, index, &face, ch, with_source)
            })
            .flatten()?;
            // A face that yields no outline (the system font, a space) is not a
            // font that draws this character.
            (!glyph.strokes.is_empty() || !glyph.fill_tris.is_empty())
                .then(|| (Arc::new(glyph), source))
        })
}

/// Web: outline the glyph from the lazily-fetched per-script Noto subset that
/// covers it. Returns `None` while that font is still loading (the char renders
/// once it arrives and the fallback cache is cleared). (#141)
#[cfg(target_arch = "wasm32")]
fn build_fallback(ch: char) -> Option<Arc<Glyph>> {
    let script = crate::scene::text::web_font::script_of(ch)?;
    let bytes = crate::scene::text::web_font::request(script)?;
    let face_count = ttf_parser::fonts_in_collection(&bytes).unwrap_or(1);
    let (face_index, face, gid) = (0..face_count).find_map(|face_index| {
        let face = ttf_parser::Face::parse(&bytes, face_index).ok()?;
        let gid = face.glyph_index(ch)?;
        Some((face_index, face, gid))
    })?;
    let k = cap_scale(&face);
    let advance = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * k;
    let mut fl = OutlineFlattener::new(k);
    outline_glyph_into(&bytes, face_index, &face, gid, &mut fl);
    fl.flush();
    if fl.contours.is_empty() {
        return None;
    }
    let fill_tris = triangulate_contours(&fl.contours);
    Some(Arc::new(Glyph {
        strokes: fl.contours,
        advance,
        fill_tris,
    }))
}

#[cfg(target_arch = "wasm32")]
fn build_shaped(_family: &str, text: &str) -> Option<ShapedRun> {
    use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping};

    let mut fonts: Vec<Arc<Vec<u8>>> = Vec::new();
    let mut waiting = false;
    for script in text
        .chars()
        .filter_map(crate::scene::text::web_font::script_of)
    {
        match crate::scene::text::web_font::request(script) {
            Some(bytes) => {
                if !fonts.iter().any(|loaded| Arc::ptr_eq(loaded, &bytes)) {
                    fonts.push(bytes);
                }
            }
            None => waiting = true,
        }
    }
    if waiting || fonts.is_empty() {
        return None;
    }

    let primary_face = ttf_parser::Face::parse(&fonts[0], 0).ok()?;
    let upem = primary_face.units_per_em() as f32;
    let cap = primary_face
        .capital_height()
        .filter(|height| *height > 0)
        .map(|height| height as f32)
        .unwrap_or(0.7 * upem);
    let px_to_9 = CAP_UNITS * upem / (SHAPE_FS * cap);

    let mut database = fontdb::Database::new();
    for bytes in fonts {
        database.load_font_data((*bytes).clone());
    }
    database.set_sans_serif_family(crate::scene::text::web_font::primary_script().family());
    let mut font_system = FontSystem::new_with_locale_and_db(
        crate::i18n::active_language_tag(),
        database,
    );
    let attrs = Attrs::new().family(Family::SansSerif);
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(SHAPE_FS, SHAPE_FS));
    buffer.set_size(&mut font_system, None, None);
    // A leading left-to-right mark keeps the paragraph direction left to
    // right, as the reference lays text out (right-to-left runs still read
    // backwards); `cells` stay relative to `text`.
    let marked = format!("{LRM}{text}");
    buffer.set_text(&mut font_system, &marked, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut font_system, false);

    let mut glyphs = Vec::new();
    let mut cells = Vec::new();
    let mut advance = 0.0_f32;
    for run in buffer.layout_runs() {
        advance = advance.max(run.line_w * px_to_9);
        for glyph in run.glyphs.iter() {
            // A cluster the shaper merged into the mark keeps its own part.
            let m = LRM.len_utf8();
            if glyph.end > m {
                let start = glyph.start.saturating_sub(m);
                cells.push((start, glyph.end - m, glyph.x * px_to_9, (glyph.x + glyph.w) * px_to_9));
            }
            let face_index = font_system
                .db_mut()
                .face(glyph.font_id)
                .map(|face| face.index)
                .unwrap_or(0);
            let Some(font) = font_system.get_font(glyph.font_id, glyph.font_weight) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(font.data(), face_index) else {
                continue;
            };
            let scale = (SHAPE_FS / face.units_per_em() as f32) * px_to_9;
            let pen_x = (glyph.x + SHAPE_FS * glyph.x_offset) * px_to_9;
            let pen_y = -(SHAPE_FS * glyph.y_offset) * px_to_9;
            let mut flattener = OutlineFlattener::new(scale);
            flattener.offset = [pen_x, pen_y];
            outline_glyph_into(
                font.data(),
                face_index,
                &face,
                ttf_parser::GlyphId(glyph.glyph_id),
                &mut flattener,
            );
            flattener.flush();
            if flattener.contours.is_empty() {
                continue;
            }
            let fill_tris = triangulate_contours(&flattener.contours);
            glyphs.push(PlacedGlyph {
                strokes: flattener.contours,
                fill_tris,
            });
        }
    }

    if advance <= 0.0 && glyphs.is_empty() {
        None
    } else {
        Some(ShapedRun { glyphs, advance, cells })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn build_shaped(family: &str, text: &str) -> Option<ShapedRun> {
    use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping};

    let mut primary_metrics = sysfont::with_face_data(family, |data, idx| {
        let f = ttf_parser::Face::parse(data, idx).ok()?;
        let upem = f.units_per_em() as f32;
        let cap = f
            .capital_height()
            .filter(|&c| c > 0)
            .map(|c| c as f32)
            .unwrap_or(0.7 * upem);
        Some((upem, cap))
    })
    .flatten();

    let mut fs = font_system().lock().unwrap();
    let attrs = if family.is_empty() {
        Attrs::new()
    } else {
        Attrs::new().family(Family::Name(family))
    };
    let mut buf = Buffer::new(&mut fs, Metrics::new(SHAPE_FS, SHAPE_FS));
    // No wrapping: a run is a single line.
    buf.set_size(&mut fs, None, None);
    // A leading left-to-right mark keeps the paragraph direction left to
    // right, as the reference lays text out (right-to-left runs still read
    // backwards); `cells` stay relative to `text`.
    let marked = format!("{LRM}{text}");
    buf.set_text(&mut fs, &marked, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(&mut fs, false);

    if primary_metrics.is_none() {
        for run in buf.layout_runs() {
            for g in run.glyphs.iter() {
                if g.glyph_id == 0 {
                    continue;
                }
                let face_index = fs.db_mut().face(g.font_id).map(|f| f.index).unwrap_or(0);
                if let Some(font) = fs.get_font(g.font_id, g.font_weight) {
                    if let Ok(face) = ttf_parser::Face::parse(font.data(), face_index) {
                        let upem = face.units_per_em() as f32;
                        let cap = face
                            .capital_height()
                            .filter(|&c| c > 0)
                            .map(|c| c as f32)
                            .unwrap_or(0.7 * upem);
                        primary_metrics = Some((upem, cap));
                        break;
                    }
                }
            }
            if primary_metrics.is_some() {
                break;
            }
        }
    }

    let (upem_p, cap_p) = primary_metrics.unwrap_or((1000.0, 700.0));
    // Pixel (at SHAPE_FS) → 9-unit cap-height factor.
    let px_to_9 = CAP_UNITS * upem_p / (SHAPE_FS * cap_p);

    let mut glyphs: Vec<PlacedGlyph> = Vec::new();
    let mut cells = Vec::new();
    let mut advance = 0.0_f32;
    for run in buf.layout_runs() {
        advance = advance.max(run.line_w * px_to_9);
        for g in run.glyphs.iter() {
            // A cluster the shaper merged into the mark keeps its own part.
            let m = LRM.len_utf8();
            if g.end > m {
                cells.push((g.start.saturating_sub(m), g.end - m, g.x * px_to_9, (g.x + g.w) * px_to_9));
            }
            let face_index = fs.db_mut().face(g.font_id).map(|f| f.index).unwrap_or(0);
            let Some(font) = fs.get_font(g.font_id, g.font_weight) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(font.data(), face_index) else {
                continue;
            };
            let upem_g = face.units_per_em() as f32;
            // Glyph font-units → 9-unit: to pixels (at SHAPE_FS) then to units.
            let scale_g = (SHAPE_FS / upem_g) * px_to_9;
            // Absolute pen position (px) → 9-unit. `physical()` shows x_offset /
            // y_offset are fractions of font size.
            let pen_x = (g.x + SHAPE_FS * g.x_offset) * px_to_9;
            let pen_y = -(SHAPE_FS * g.y_offset) * px_to_9; // outlines are y-up

            let mut fl = OutlineFlattener::new(scale_g);
            fl.offset = [pen_x, pen_y];
            outline_glyph_into(
                font.data(),
                face_index,
                &face,
                ttf_parser::GlyphId(g.glyph_id),
                &mut fl,
            );
            fl.flush();
            if !fl.contours.is_empty() {
                let fill_tris = triangulate_contours(&fl.contours);
                glyphs.push(PlacedGlyph {
                    strokes: fl.contours,
                    fill_tris,
                });
            }
        }
    }

    if advance <= 0.0 && glyphs.is_empty() {
        return None;
    }
    Some(ShapedRun { glyphs, advance, cells })
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn shape_run_falls_back_for_missing_script() {
        let fams = sysfont::families();
        if fams.is_empty() {
            eprintln!("no system fonts; skipping");
            return;
        }
        // A typical Latin family lacks CJK; cosmic-text should still resolve the
        // ideograph from a fallback font. If the machine has no CJK font at all
        // the count stays 1 — tolerated, but never a panic or empty run.
        let fam = fams
            .iter()
            .find(|f| glyph(f, 'A').is_some())
            .expect("family");
        let run = shape_run(fam, "A中").expect("shaped");
        eprintln!("fallback run glyphs={}", run.glyphs.len());
        assert!(!run.glyphs.is_empty());
    }

    #[test]
    fn fallback_ideograph_fills_the_text_height_like_a_big_font() {
        // An SHX big font draws an ideograph one text height tall and one
        // text height wide. The TrueType stand-in must match that box (9
        // units), not the ~12–14 units the Latin cap-height normalisation
        // gives a CJK em square — that overrun pushed every substituted
        // Chinese line past its frame. Tolerated when the machine has no
        // CJK font at all.
        let Some(g) = fallback_glyph('中') else {
            eprintln!("no CJK system font; skipping");
            return;
        };
        assert!(
            (g.advance - CAP_UNITS).abs() < 0.6,
            "ideograph advance must be about one text height: got {}",
            g.advance
        );
        let ink_h = g
            .strokes
            .iter()
            .flatten()
            .map(|p| p[1])
            .fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        assert!(
            ink_h.1 - ink_h.0 <= CAP_UNITS * 1.05,
            "ideograph ink must fit the text height: {:?}",
            ink_h
        );
        // Latin fallback keeps the cap-height convention (shares a baseline
        // with the stroke glyphs around it).
        assert!(!is_full_width('A') && is_full_width('中') && is_full_width('，'));
    }

    #[test]
    fn shape_run_positions_glyphs() {
        let fams = sysfont::families();
        if fams.is_empty() { eprintln!("no fonts; skip"); return; }
        let fam = fams.iter().find(|f| glyph(f, 'A').is_some()).expect("family");
        let run = shape_run(fam, "AVA").expect("shaped");
        eprintln!("glyphs={} advance={:.3}", run.glyphs.len(), run.advance);
        assert_eq!(run.glyphs.len(), 3);
        assert!(run.advance > 0.0);
        // glyphs must be laid out left-to-right: each glyph's strokes sit at a
        // greater x than the previous glyph's start.
        let xs: Vec<f32> = run.glyphs.iter()
            .filter_map(|g| g.strokes.iter().flatten().map(|p| p[0]).fold(None, |m,x| Some(m.map_or(x, |mm:f32| mm.min(x)))))
            .collect();
        assert!(xs.windows(2).all(|w| w[1] >= w[0] - 1.0), "glyphs not L->R: {:?}", xs);
    }

    #[test]
    fn smoke_outline_first_family() {
        let fams = sysfont::families();
        if fams.is_empty() {
            eprintln!("no system fonts; skipping");
            return;
        }
        // Find any family that yields an 'A' outline.
        let mut ok = false;
        for fam in fams.iter().take(20) {
            if let Some(g) = glyph(fam, 'A') {
                eprintln!(
                    "{}: contours={} advance={:.4} verts={}",
                    fam,
                    g.strokes.len(),
                    g.advance,
                    g.strokes.iter().map(|s| s.len()).sum::<usize>()
                );
                assert!(g.advance > 0.0);
                assert!(!g.strokes.is_empty());
                ok = true;
                break;
            }
        }
        assert!(ok, "no family produced an 'A' outline");
    }

    // ── Hint-reliant fonts ─────────────────────────────────

    /// Bytes of a font in the Windows font folder; `None` (→ skip) elsewhere.
    fn windows_font(file: &str) -> Option<Vec<u8>> {
        let dir = std::env::var_os("WINDIR")?;
        std::fs::read(std::path::Path::new(&dir).join("Fonts").join(file)).ok()
    }

    /// Contours of `ch` in font units (flattener scale 1), either straight from
    /// ttf-parser or through `outline_glyph_into`.
    fn contours(data: &[u8], index: u32, ch: char, new_path: bool) -> Vec<Vec<[f32; 2]>> {
        let face = ttf_parser::Face::parse(data, index).expect("face");
        let gid = face.glyph_index(ch).expect("glyph");
        let mut fl = OutlineFlattener::new(1.0);
        if new_path {
            outline_glyph_into(data, index, &face, gid, &mut fl);
        } else {
            face.outline_glyph(gid, &mut fl);
        }
        fl.flush();
        fl.contours
    }

    fn require_interpreter(data: &[u8], index: u32) -> bool {
        use skrifa::MetadataProvider;
        skrifa::FontRef::from_index(data, index)
            .expect("font")
            .outline_glyphs()
            .require_interpreter()
    }

    #[test]
    fn tricky_font_detection_kaiu_yes_msjh_no() {
        // DFKai-SB is on FreeType's / skrifa's tricky list; Microsoft JhengHei
        // is not. The cached decision (`hinted_outline`) must agree.
        if let Some(kaiu) = windows_font("kaiu.ttf") {
            assert!(
                require_interpreter(&kaiu, 0),
                "kaiu.ttf must need the interpreter"
            );
            let gid = ttf_parser::Face::parse(&kaiu, 0)
                .unwrap()
                .glyph_index('自')
                .unwrap();
            assert!(
                hinted_outline(&kaiu, 0, gid).is_some(),
                "kaiu must take the hinted path"
            );
        } else {
            eprintln!("kaiu.ttf not installed; skipping");
        }
        if let Some(msjh) = windows_font("msjh.ttc") {
            assert!(
                !require_interpreter(&msjh, 0),
                "msjh.ttc must not need the interpreter"
            );
            let gid = ttf_parser::Face::parse(&msjh, 0)
                .unwrap()
                .glyph_index('自')
                .unwrap();
            assert!(
                hinted_outline(&msjh, 0, gid).is_none(),
                "msjh must keep ttf-parser"
            );
        } else {
            eprintln!("msjh.ttc not installed; skipping");
        }
        if let Some(mingliu) = windows_font("mingliu.ttc") {
            // MingLiU / PMingLiU are on the same list (informational coverage).
            assert!(
                require_interpreter(&mingliu, 0),
                "mingliu.ttc must need the interpreter"
            );
        }
    }

    #[test]
    fn kaiu_zi_goes_through_the_interpreter_and_stays_in_the_em() {
        // Raw DFKai-SB outlines drop strokes ('自' renders as '目'); the hinted
        // path must produce a different outline, in font units (not 512-px
        // pixels), filling most of the em square.
        let Some(kaiu) = windows_font("kaiu.ttf") else {
            eprintln!("kaiu.ttf not installed; skipping");
            return;
        };
        let raw = contours(&kaiu, 0, '自', false);
        let hinted = contours(&kaiu, 0, '自', true);
        assert!(!hinted.is_empty(), "hinted '自' has no contours");
        assert_ne!(raw, hinted, "hinted '自' must differ from the raw outline");

        let face = ttf_parser::Face::parse(&kaiu, 0).unwrap();
        let upem = face.units_per_em() as f32;
        let (asc, desc) = (face.ascender() as f32, face.descender() as f32);
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in hinted.iter().flatten() {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        eprintln!(
            "kaiu '自': upem={upem} asc={asc} desc={desc} raw contours={} hinted contours={} bbox=({x0},{y0})-({x1},{y1})",
            raw.len(),
            hinted.len()
        );
        let tol = 0.02 * upem;
        assert!(
            x0 >= -tol && x1 <= upem + tol,
            "x outside the em: {x0}..{x1}"
        );
        assert!(
            y0 >= desc - tol && y1 <= asc + tol,
            "y outside the em: {y0}..{y1}"
        );
        // A forgotten pixel → font-unit conversion would halve the ideograph
        // (512 px vs 1024 upem). '自' is narrow (~0.47 em) but ~0.8 em tall.
        assert!(y1 - y0 > 0.6 * upem, "ideograph too short: {}", y1 - y0);
        assert!(x1 - x0 > 0.3 * upem, "ideograph too narrow: {}", x1 - x0);

        // The public entry point must use the hinted path too.
        if let Some(g) = glyph("DFKai-SB", '自') {
            let gid = face.glyph_index('自').unwrap();
            let mut fl = OutlineFlattener::new(cap_scale(&face));
            face.outline_glyph(gid, &mut fl);
            fl.flush();
            assert_ne!(
                g.strokes, fl.contours,
                "glyph() still returns the raw outline"
            );
        } else {
            eprintln!("DFKai-SB family not resolvable via sysfont; public-path check skipped");
        }
    }

    #[test]
    fn non_tricky_font_outlines_are_unchanged() {
        // Fonts that don't need the interpreter keep the exact ttf-parser outline.
        let Some(msjh) = windows_font("msjh.ttc") else {
            eprintln!("msjh.ttc not installed; skipping");
            return;
        };
        for ch in "自保持接觸器線圈AgQ1".chars() {
            assert_eq!(
                contours(&msjh, 0, ch, true),
                contours(&msjh, 0, ch, false),
                "msjh '{ch}' changed"
            );
        }
    }
}
