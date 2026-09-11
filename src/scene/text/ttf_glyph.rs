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
        face.outline_glyph(gid, &mut fl);
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
}

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

/// Web: outline the glyph from the lazily-fetched per-script Noto subset that
/// covers it. Returns `None` while that font is still loading (the char renders
/// once it arrives and the fallback cache is cleared). (#141)
#[cfg(target_arch = "wasm32")]
fn build_fallback(ch: char) -> Option<Arc<Glyph>> {
    let script = crate::scene::text::web_font::script_of(ch)?;
    let bytes = crate::scene::text::web_font::request(script)?;
    let face_count = ttf_parser::fonts_in_collection(&bytes).unwrap_or(1);
    let (face, gid) = (0..face_count).find_map(|face_index| {
        let face = ttf_parser::Face::parse(&bytes, face_index).ok()?;
        let gid = face.glyph_index(ch)?;
        Some((face, gid))
    })?;
    let k = cap_scale(&face);
    let advance = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * k;
    let mut fl = OutlineFlattener::new(k);
    face.outline_glyph(gid, &mut fl);
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

#[cfg(not(target_arch = "wasm32"))]
fn build_fallback(ch: char) -> Option<Arc<Glyph>> {
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
            let k = cap_scale(&face);
            let gid = ttf_parser::GlyphId(g.glyph_id);
            let advance = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * k;
            let mut fl = OutlineFlattener::new(k);
            face.outline_glyph(gid, &mut fl);
            fl.flush();
            let fill_tris = triangulate_contours(&fl.contours);
            return Some(Arc::new(Glyph {
                strokes: fl.contours,
                advance,
                fill_tris,
            }));
        }
    }
    None
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
    buffer.set_text(&mut font_system, text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut font_system, false);

    let mut glyphs = Vec::new();
    let mut advance = 0.0_f32;
    for run in buffer.layout_runs() {
        advance = advance.max(run.line_w * px_to_9);
        for glyph in run.glyphs.iter() {
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
            face.outline_glyph(
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
        Some(ShapedRun { glyphs, advance })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn build_shaped(family: &str, text: &str) -> Option<ShapedRun> {
    use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping};

    // Cap height / units-per-em of the requested family set the normalization:
    // its capital letters become CAP_UNITS tall, and every fallback glyph is
    // scaled into the same pixel-per-unit so sizes stay consistent.
    let (upem_p, cap_p) = sysfont::with_face_data(family, |data, idx| {
        let f = ttf_parser::Face::parse(data, idx).ok()?;
        let upem = f.units_per_em() as f32;
        let cap = f
            .capital_height()
            .filter(|&c| c > 0)
            .map(|c| c as f32)
            .unwrap_or(0.7 * upem);
        Some((upem, cap))
    })
    .flatten()?;
    // Pixel (at SHAPE_FS) → 9-unit cap-height factor.
    let px_to_9 = CAP_UNITS * upem_p / (SHAPE_FS * cap_p);

    let mut fs = font_system().lock().unwrap();
    let attrs = Attrs::new().family(Family::Name(family));
    let mut buf = Buffer::new(&mut fs, Metrics::new(SHAPE_FS, SHAPE_FS));
    // No wrapping: a run is a single line.
    buf.set_size(&mut fs, None, None);
    buf.set_text(&mut fs, text, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(&mut fs, false);

    let mut glyphs: Vec<PlacedGlyph> = Vec::new();
    let mut advance = 0.0_f32;
    for run in buf.layout_runs() {
        advance = advance.max(run.line_w * px_to_9);
        for g in run.glyphs.iter() {
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
            face.outline_glyph(ttf_parser::GlyphId(g.glyph_id), &mut fl);
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
    Some(ShapedRun { glyphs, advance })
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
}
