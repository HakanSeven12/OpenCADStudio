//! CPU-side signed-distance-field (SDF) glyph atlas.
//!
//! Phase 1 of the text-shader initiative: bake each glyph into a single-channel
//! SDF tile and pack the tiles into one growable atlas texture. Later phases
//! draw text as per-glyph quads that sample this atlas in an SDF fragment
//! shader, instead of tessellating glyph outlines into wire segments.
//!
//! Both glyph kinds feed one unified field so a single shader renders both:
//!   * Filled (TrueType): the glyph carries closed contours (`strokes`) plus a
//!     fill triangulation (`fill_tris`). The field is the **signed** distance to
//!     the outline — positive inside the fill, negative outside.
//!   * Stroke (LFF pen fonts): the glyph carries open polylines and no fill. The
//!     field is a **band** around the strokes: `pen_half - dist_to_polyline`,
//!     positive inside the nominal pen width.
//!
//! In both cases the boundary sits at field value 0, mapped to 0.5 in the u8
//! texel so the shader can threshold with `smoothstep(0.5 ± aa, value)`.
//!
//! Uses only crates already in the tree (no image/SDF crate): the distance
//! field is computed by brute-force point-to-segment distance over the glyph's
//! own polylines. Tiles are small and baked once per (font, char), then cached.

// Phase 1 lands the CPU atlas builder; it is exercised by tests but not yet
// wired into the render path — that is Phase 2 (the text quad pipeline). Drop
// this allow once `GlyphAtlas` is consumed by the renderer.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::scene::text::font_face::Face;
use crate::scene::text::lff::Glyph;

/// Process-wide glyph atlas, shared by the text collector (which bakes glyphs
/// while building draw data) and the GPU upload (which reads the texels).
/// Mirrors the existing global font-glyph caches (see `ttf_glyph`).
pub fn text_atlas() -> &'static Mutex<GlyphAtlas> {
    static ATLAS: OnceLock<Mutex<GlyphAtlas>> = OnceLock::new();
    // 2048² (~780 glyphs) covers real text-heavy drawings (many fonts + CJK /
    // Vietnamese diacritics) without ever growing; `pack()` still grows as a
    // fallback beyond that. See issue #347.
    ATLAS.get_or_init(|| Mutex::new(GlyphAtlas::new(2048, 2048)))
}

/// Bumped whenever already-baked entries' UVs stop addressing their tile:
/// `grow_height` rescales every entry's V and `reset` rewinds the packer, but
/// glyph quads built earlier keep the UVs they were laid out with. Those stale
/// quads sample the wrong tile on screen and miss the export table entirely
/// (silently dropping text from the PDF — issue #385 all over again, under
/// issue #347's grow-the-atlas conditions). The tessellation memo guard folds
/// this in, so a growth re-lays-out the text instead of leaving stale quads.
static ATLAS_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Current atlas UV generation — see [`ATLAS_GENERATION`]. Anything that caches
/// glyph quads must treat a change here as "re-lay-out the text".
pub fn generation() -> u64 {
    ATLAS_GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

fn bump_generation() {
    ATLAS_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// TEXTFILL system variable — `true` fills TrueType glyphs (default), `false`
/// draws them hollow (outline only). Global, like the AutoCAD system variable.
static TEXTFILL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn textfill() -> bool {
    TEXTFILL.load(std::sync::atomic::Ordering::Relaxed)
}

/// Set TEXTFILL and drop the cached glyphs so they re-bake filled / hollow.
/// Callers must re-tessellate text (`bump_geometry`) so quads pick up the
/// re-baked tiles.
pub fn set_textfill(on: bool) {
    TEXTFILL.store(on, std::sync::atomic::Ordering::Relaxed);
    reset_font_entries();
}

pub fn reset_font_entries() {
    if let Ok(mut atlas) = text_atlas().lock() {
        atlas.reset();
    }
}

/// Debug: when env `OCS_TEXT_BOX` is set, each SDF text run also emits a
/// rectangle outline around its glyph bounds so the text pick box is visible.
/// Read once.
pub fn text_box_debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OCS_TEXT_BOX").is_some())
}

// ── Bake configuration ──────────────────────────────────────────────────────

/// Texels per glyph unit. Glyphs live in a 9-unit cap-height space, so a cap
/// letter is ~9 * PX_PER_UNIT texels tall. SDF is resolution-independent when
/// sampled, so this only sets the baked field's fidelity, not on-screen size.
const PX_PER_UNIT: f32 = 6.0;

/// Half-range of the distance field, in glyph units. Field values are clamped
/// to ±SPREAD_UNITS and linearly mapped to [0, 1] (0 -> 0.5). Also the tile
/// margin: the tile extends this far past the ink so the field/AA is not
/// clipped at the glyph edge.
const SPREAD_UNITS: f32 = 1.5;

/// LFF stroke half-width, in glyph units. Stroke fonts have no intrinsic width
/// (plotted at the current lineweight); this is the nominal pen the SDF band is
/// built around. Scales with text height like the strokes themselves.
const PEN_HALF_UNITS: f32 = 0.35;

/// Nominal half-width of the SDF pen for a stroke glyph.
///
/// CPU exporters cannot sample the atlas, so they use this to stroke the same
/// LFF/SHX centrelines at the thickness shown on screen.
pub(crate) fn stroke_pen_half_units(bold: bool) -> f32 {
    if bold {
        PEN_HALF_UNITS * 1.7
    } else {
        PEN_HALF_UNITS
    }
}

/// Upper bound on a single glyph tile's dimension (texels), a guard against a
/// pathologically wide/tall glyph blowing up the atlas.
const MAX_TILE_PX: u32 = 512;

/// Largest atlas dimension the packer will grow the texture to, kept within the
/// common wgpu `max_texture_dimension_2d` limit (8192). The atlas starts at
/// 1024² and doubles its height on demand up to this bound.
const MAX_ATLAS_PX: u32 = 8192;

// ── Public types ─────────────────────────────────────────────────────────────

/// Placement of one baked glyph within the atlas.
#[derive(Clone, Copy, Debug)]
pub struct AtlasEntry {
    /// Atlas UV of the tile's top-left corner (normalized 0..1).
    pub uv_min: [f32; 2],
    /// Atlas UV of the tile's bottom-right corner (normalized 0..1).
    pub uv_max: [f32; 2],
    /// Glyph-space (9-unit) rectangle the tile covers — the ink bounding box
    /// expanded by the SDF spread. The render quad must span exactly this rect
    /// (in glyph-local coords) so texels line up with world positions.
    pub plane_min: [f32; 2],
    pub plane_max: [f32; 2],
    /// Glyph advance width (9-unit space).
    pub advance: f32,
}

/// A glyph's vector geometry recovered for CPU export (PDF / print), where the
/// SDF atlas texture can't be sampled. All coordinates are in the 9-unit glyph
/// space; `plane_min`/`plane_max` is the same tile rect the render quad spans,
/// so the exporter can map the geometry into a rendered quad by affine interp.
#[derive(Clone, Debug)]
pub struct GlyphExport {
    pub plane_min: [f32; 2],
    pub plane_max: [f32; 2],
    /// Outline / centreline polylines (glyph units).
    pub strokes: Vec<Vec<[f32; 2]>>,
    /// Fill triangulation (glyph units, flat triples). Non-empty only for a
    /// filled TrueType glyph — the exporter fills these and ignores `strokes`;
    /// empty means "stroke `strokes`" (LFF pen font, or a hollow TTF glyph).
    pub fill_tris: Vec<[f32; 2]>,
    /// Baked with the 1.7× bold pen; the exporter widens its own pen to match.
    pub bold: bool,
}

/// Pack a glyph quad's `uv_min` corner into a stable hash key for the export
/// table below. The two f32 UVs are exact copies of the baked `AtlasEntry`
/// values (via `GlyphQuad`), so bit-equality is a reliable identity as long as
/// the atlas hasn't been re-scaled by a growth since the quads were built —
/// which, for an export of an already-viewed drawing, it hasn't.
pub fn uv_key(uv_min: [f32; 2]) -> u64 {
    ((uv_min[0].to_bits() as u64) << 32) | uv_min[1].to_bits() as u64
}

/// A single-channel SDF glyph atlas plus a shelf packer.
pub struct GlyphAtlas {
    width: u32,
    height: u32,
    /// R8 SDF texels, row-major, row 0 = top.
    data: Vec<u8>,
    /// Cache keyed by (font family, char, bold). `None` = a whitespace/empty
    /// glyph with no ink (nothing to draw), cached so we do not re-resolve it.
    /// Bold bakes the same glyph with a wider pen (a separate atlas entry).
    entries: HashMap<(String, char, bool), Option<AtlasEntry>>,
    // Shelf packer cursor.
    cursor_x: u32,
    cursor_y: u32,
    shelf_h: u32,
    /// UV of a fully-inside (value 255) texel, reserved at the atlas corner.
    /// A quad whose corners all map here samples a constant "inside" field, so
    /// the SDF shader fills it solid — used to draw decoration lines
    /// (underline / overline / strikethrough) through the glyph pipeline.
    solid_uv: [f32; 2],
    /// Set when `data` changed since the last upload; the GPU side clears it.
    dirty: bool,
}

/// A freshly baked, not-yet-packed glyph tile.
struct BakedTile {
    w: u32,
    h: u32,
    data: Vec<u8>,
    plane_min: [f32; 2],
    plane_max: [f32; 2],
    advance: f32,
}

/// Side of the reserved solid (all-255) block at the atlas corner.
const SOLID_PX: u32 = 4;

impl GlyphAtlas {
    pub fn new(width: u32, height: u32) -> Self {
        let mut data = vec![0u8; (width * height) as usize];
        // Reserve a small solid block at (0,0) for decoration lines; the shelf
        // packer starts past it so glyphs never overwrite it.
        let solid = SOLID_PX.min(width).min(height);
        for y in 0..solid {
            for x in 0..solid {
                data[(y * width + x) as usize] = 255;
            }
        }
        Self {
            width,
            height,
            data,
            entries: HashMap::new(),
            cursor_x: solid,
            cursor_y: 0,
            shelf_h: solid,
            // Centre of the solid block, safely inside it under bilinear filtering.
            solid_uv: [
                (solid as f32 * 0.5) / width as f32,
                (solid as f32 * 0.5) / height as f32,
            ],
            dirty: true,
        }
    }

    /// UV of a fully-inside texel; a quad with all corners at this UV renders
    /// solid. Used for decoration lines.
    pub fn solid_uv(&self) -> [f32; 2] {
        self.solid_uv
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    /// Raw R8 texel data (row-major, row 0 = top) for GPU upload.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Drop every cached glyph and re-init the texture, so glyphs re-bake on
    /// next use (e.g. after TEXTFILL toggles between fill and outline).
    pub fn reset(&mut self) {
        *self = GlyphAtlas::new(self.width, self.height);
        // The packer rewound, so every previously handed-out UV is now stale.
        bump_generation();
    }

    /// Look up a glyph's atlas placement, baking + packing it on first use.
    /// Returns `None` for whitespace/empty glyphs (nothing to draw) and when the
    /// atlas is full (Phase 1 is single-page; callers should treat `None` as
    /// "skip / fall back").
    pub fn get_or_insert(&mut self, family: &str, ch: char, bold: bool) -> Option<AtlasEntry> {
        let key = (family.to_string(), ch, bold);
        if let Some(cached) = self.entries.get(&key) {
            return *cached;
        }
        // Bold stroke glyphs bake with a wider pen band (same shapes, thicker).
        let pen_half = stroke_pen_half_units(bold);
        let face = Face::resolve(family);
        let entry = face
            .glyph(ch)
            .and_then(|g| bake_glyph(&g, pen_half))
            .and_then(|tile| self.pack(tile));
        self.entries.insert(key, entry);
        entry
    }

    /// Snapshot every baked glyph's vector geometry, keyed by its tile
    /// `uv_min` ([`uv_key`]). CPU exporters (PDF / print) walk a text run's SDF
    /// glyph quads, look each quad's `uv_min` up here, and re-emit the glyph as
    /// vector strokes / fills instead of sampling the SDF texture. Re-resolves
    /// each entry's outline through `Face` (the same source `get_or_insert`
    /// baked from), gated on TEXTFILL so a hollow-on-screen glyph exports hollow.
    pub fn export_table(&self) -> HashMap<u64, GlyphExport> {
        let fill_on = textfill();
        let mut out = HashMap::with_capacity(self.entries.len());
        // `Face::resolve` reparses a font, so resolve once per family rather than
        // once per baked glyph — a CJK drawing holds thousands of entries over a
        // handful of families, and the atlas mutex is held for this whole walk.
        let mut faces: HashMap<&str, Face> = HashMap::new();
        for ((family, ch, bold), entry) in &self.entries {
            let Some(entry) = entry else { continue };
            let face = faces
                .entry(family.as_str())
                .or_insert_with(|| Face::resolve(family));
            let Some(g) = face.glyph(*ch) else { continue };
            out.insert(
                uv_key(entry.uv_min),
                GlyphExport {
                    plane_min: entry.plane_min,
                    plane_max: entry.plane_max,
                    strokes: g.strokes.clone(),
                    // `bake_glyph` gates the texture fill on TEXTFILL the same way
                    // (`textfill() && !g.fill_tris.is_empty()`); `Face::glyph` does
                    // not, so without this the export ignores TEXTFILL 0 and prints
                    // solid glyphs where the screen shows outlines.
                    fill_tris: if fill_on {
                        g.fill_tris.clone()
                    } else {
                        Vec::new()
                    },
                    // Bold stroke glyphs bake at a 1.7× pen (`get_or_insert`) but
                    // share the centrelines, so carry the flag and let the exporter
                    // widen its own pen by the same factor.
                    bold: *bold,
                },
            );
        }
        out
    }

    /// Grow the atlas taller (keeping the width, so packed X positions and their
    /// normalized U stay valid) and rescale every cached entry's V for the new
    /// height. Returns false once the height hits `MAX_ATLAS_PX` — text-heavy
    /// drawings would otherwise silently drop glyphs once the single page filled
    /// (issue #347). Width is unchanged, so the row-major data just grows with
    /// zeroed rows appended at the bottom.
    fn grow_height(&mut self) -> bool {
        if self.height >= MAX_ATLAS_PX {
            return false;
        }
        let old_h = self.height;
        let new_h = (self.height * 2).min(MAX_ATLAS_PX);
        self.data.resize((self.width * new_h) as usize, 0);
        let sy = old_h as f32 / new_h as f32;
        for entry in self.entries.values_mut().flatten() {
            entry.uv_min[1] *= sy;
            entry.uv_max[1] *= sy;
        }
        self.solid_uv[1] *= sy;
        self.height = new_h;
        self.dirty = true;
        // Every already-emitted quad's V now addresses the wrong tile.
        bump_generation();
        true
    }

    /// Blit a baked tile into the atlas via a simple shelf packer. Grows the
    /// atlas when a tile doesn't fit; returns `None` only once it can grow no
    /// further (or the tile is wider than the atlas, which the tile cap prevents).
    fn pack(&mut self, tile: BakedTile) -> Option<AtlasEntry> {
        // Wrap to a new shelf if the tile overflows the current row.
        if self.cursor_x + tile.w > self.width {
            self.cursor_y += self.shelf_h;
            self.cursor_x = 0;
            self.shelf_h = 0;
        }
        if self.cursor_x + tile.w > self.width {
            return None; // tile wider than the atlas (MAX_TILE_PX guards this)
        }
        while self.cursor_y + tile.h > self.height {
            if !self.grow_height() {
                return None; // atlas full and cannot grow further
            }
        }
        let (ox, oy) = (self.cursor_x, self.cursor_y);
        for row in 0..tile.h {
            let dst = ((oy + row) * self.width + ox) as usize;
            let src = (row * tile.w) as usize;
            self.data[dst..dst + tile.w as usize]
                .copy_from_slice(&tile.data[src..src + tile.w as usize]);
        }
        self.cursor_x += tile.w;
        self.shelf_h = self.shelf_h.max(tile.h);
        self.dirty = true;

        let (fw, fh) = (self.width as f32, self.height as f32);
        Some(AtlasEntry {
            // Sample the centers of the edge texels rather than the tile boundaries.
            // This prevents bilinear filtering from bleeding into neighbouring tiles.
            uv_min: [(ox as f32 + 0.5) / fw, (oy as f32 + 0.5) / fh],
            uv_max: [
                ((ox + tile.w) as f32 - 0.5) / fw,
                ((oy + tile.h) as f32 - 0.5) / fh,
            ],
            plane_min: tile.plane_min,
            plane_max: tile.plane_max,
            advance: tile.advance,
        })
    }
}

// ── Baking ─────────────────────────────────────────────────────────────────

/// Rasterize a glyph into an SDF tile. `None` if the glyph has no ink.
fn bake_glyph(g: &Glyph, pen_half: f32) -> Option<BakedTile> {
    // TEXTFILL 0 renders TrueType glyphs hollow: bake them as a stroke field
    // (outline) instead of a signed fill field. Stroke fonts (no fill tris) are
    // unaffected.
    let filled = textfill() && !g.fill_tris.is_empty();

    // Ink bounding box over all strokes (and fill triangles, for safety).
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut acc = |x: f32, y: f32| {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    };
    for s in &g.strokes {
        for &[x, y] in s {
            acc(x, y);
        }
    }
    for &[x, y] in &g.fill_tris {
        acc(x, y);
    }
    if !min_x.is_finite() || max_x < min_x || max_y < min_y {
        return None; // no geometry (whitespace)
    }

    // Expand by the spread so the field is not clipped at the ink edge.
    let plane_min = [min_x - SPREAD_UNITS, min_y - SPREAD_UNITS];
    let plane_max = [max_x + SPREAD_UNITS, max_y + SPREAD_UNITS];

    let w = (((plane_max[0] - plane_min[0]) * PX_PER_UNIT).ceil() as u32)
        .clamp(1, MAX_TILE_PX);
    let h = (((plane_max[1] - plane_min[1]) * PX_PER_UNIT).ceil() as u32)
        .clamp(1, MAX_TILE_PX);

    let mut data = vec![0u8; (w * h) as usize];
    let inv = 1.0 / PX_PER_UNIT;
    for py in 0..h {
        // Row 0 = top of the tile; glyph Y is up, so flip.
        let gy = plane_max[1] - (py as f32 + 0.5) * inv;
        for px in 0..w {
            let gx = plane_min[0] + (px as f32 + 0.5) * inv;
            let d = min_dist_to_strokes(gx, gy, &g.strokes);
            let sd = if filled {
                // Signed: positive inside the fill.
                if point_in_fill(gx, gy, &g.fill_tris) {
                    d
                } else {
                    -d
                }
            } else {
                // Band around the pen path (wider for bold).
                pen_half - d
            };
            let t = (0.5 + sd / (2.0 * SPREAD_UNITS)).clamp(0.0, 1.0);
            data[(py * w + px) as usize] = (t * 255.0).round() as u8;
        }
    }

    Some(BakedTile {
        w,
        h,
        data,
        plane_min,
        plane_max,
        advance: g.advance,
    })
}

/// Minimum distance from `(x, y)` to any segment of any polyline. Treats a
/// single-point polyline as a point. Returns `+inf` if there is no geometry.
fn min_dist_to_strokes(x: f32, y: f32, strokes: &[Vec<[f32; 2]>]) -> f32 {
    let mut best = f32::INFINITY;
    for s in strokes {
        if s.len() == 1 {
            best = best.min(dist(x, y, s[0][0], s[0][1]));
            continue;
        }
        for w in s.windows(2) {
            best = best.min(dist_point_seg(x, y, w[0], w[1]));
        }
    }
    best
}

/// True if `(x, y)` lies in any triangle of a flat triangle-vertex list
/// (3 vertices per triangle).
fn point_in_fill(x: f32, y: f32, tris: &[[f32; 2]]) -> bool {
    for t in tris.chunks_exact(3) {
        if point_in_tri(x, y, t[0], t[1], t[2]) {
            return true;
        }
    }
    false
}

fn dist(px: f32, py: f32, ax: f32, ay: f32) -> f32 {
    ((px - ax) * (px - ax) + (py - ay) * (py - ay)).sqrt()
}

fn dist_point_seg(px: f32, py: f32, a: [f32; 2], b: [f32; 2]) -> f32 {
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let (apx, apy) = (px - a[0], py - a[1]);
    let ab2 = abx * abx + aby * aby;
    let t = if ab2 <= 1e-12 {
        0.0
    } else {
        ((apx * abx + apy * aby) / ab2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a[0] + abx * t, a[1] + aby * t);
    dist(px, py, cx, cy)
}

fn point_in_tri(px: f32, py: f32, a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    // Sign of the cross products; inside if all the same sign (allowing edges).
    let d1 = edge_sign(px, py, a, b);
    let d2 = edge_sign(px, py, b, c);
    let d3 = edge_sign(px, py, c, a);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

fn edge_sign(px: f32, py: f32, a: [f32; 2], b: [f32; 2]) -> f32 {
    (px - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (py - b[1])
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample the SDF value at a glyph-space point of a baked tile (0..255).
    fn sample(tile: &BakedTile, gx: f32, gy: f32) -> u8 {
        let px = (((gx - tile.plane_min[0]) * PX_PER_UNIT) as i32)
            .clamp(0, tile.w as i32 - 1) as u32;
        let py = (((tile.plane_max[1] - gy) * PX_PER_UNIT) as i32)
            .clamp(0, tile.h as i32 - 1) as u32;
        tile.data[(py * tile.w + px) as usize]
    }

    /// A filled 4x4 square (fill_tris + closed contour).
    fn filled_square() -> Glyph {
        let sq = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]];
        Glyph {
            strokes: vec![sq.to_vec()],
            advance: 4.0,
            fill_tris: vec![
                [0.0, 0.0],
                [4.0, 0.0],
                [4.0, 4.0],
                [0.0, 0.0],
                [4.0, 4.0],
                [0.0, 4.0],
            ],
        }
    }

    /// A single diagonal stroke, no fill (LFF-style).
    fn diagonal_stroke() -> Glyph {
        Glyph {
            strokes: vec![vec![[0.0, 0.0], [4.0, 4.0]]],
            advance: 4.0,
            fill_tris: vec![],
        }
    }

    #[test]
    fn filled_glyph_inside_is_high_outside_is_low() {
        let tile = bake_glyph(&filled_square(), PEN_HALF_UNITS).expect("square bakes");
        // Deep inside the fill -> well above the 0.5 (128) edge.
        assert!(sample(&tile, 2.0, 2.0) > 200, "center should read as inside");
        // Well outside the fill -> below the edge.
        assert!(
            sample(&tile, -1.0, -1.0) < 60,
            "far corner should read as outside"
        );
    }

    #[test]
    fn stroke_glyph_on_line_is_high_off_line_is_low() {
        let tile = bake_glyph(&diagonal_stroke(), PEN_HALF_UNITS).expect("stroke bakes");
        // On the diagonal -> inside the pen band.
        assert!(sample(&tile, 2.0, 2.0) > 128, "on-stroke should be inside band");
        // Off the diagonal by more than the pen half-width -> outside.
        assert!(
            sample(&tile, 3.0, 0.5) < 128,
            "off-stroke should be outside band"
        );
    }

    #[test]
    fn empty_glyph_has_no_tile() {
        let space = Glyph {
            strokes: vec![],
            advance: 4.5,
            fill_tris: vec![],
        };
        assert!(bake_glyph(&space, PEN_HALF_UNITS).is_none());
    }

    #[test]
    fn packing_places_tiles_without_overlap() {
        let mut atlas = GlyphAtlas::new(256, 256);
        let a = atlas.pack(bake_glyph(&filled_square(), PEN_HALF_UNITS).unwrap()).unwrap();
        let b = atlas.pack(bake_glyph(&diagonal_stroke(), PEN_HALF_UNITS).unwrap()).unwrap();
        assert!(atlas.is_dirty());
        // UVs are within the atlas.
        for e in [&a, &b] {
            assert!(e.uv_min[0] >= 0.0 && e.uv_max[0] <= 1.0);
            assert!(e.uv_min[1] >= 0.0 && e.uv_max[1] <= 1.0);
        }
        // Two tiles on the same shelf must not overlap in U.
        let overlap_u = a.uv_min[0] < b.uv_max[0] && b.uv_min[0] < a.uv_max[0];
        let overlap_v = a.uv_min[1] < b.uv_max[1] && b.uv_min[1] < a.uv_max[1];
        assert!(!(overlap_u && overlap_v), "tiles overlap in the atlas");
    }

    #[test]
    fn full_atlas_returns_none() {
        // A tiny atlas cannot fit even one padded tile (wider than the atlas —
        // the width the packer does not grow).
        let mut atlas = GlyphAtlas::new(4, 4);
        assert!(atlas.pack(bake_glyph(&filled_square(), PEN_HALF_UNITS).unwrap()).is_none());
    }

    #[test]
    fn atlas_grows_instead_of_dropping_glyphs() {
        // Text-heavy drawings need more unique glyphs than one 1024² page holds;
        // the packer must grow taller rather than start returning None, which
        // silently dropped text and dimensions (issue #347).
        let mut atlas = GlyphAtlas::new(1024, 1024);
        let start_h = atlas.height();
        let mut packed = 0;
        for _ in 0..1000 {
            match atlas.pack(bake_glyph(&filled_square(), PEN_HALF_UNITS).unwrap()) {
                Some(e) => {
                    // UVs stay normalized after any growth-driven rescale.
                    assert!(e.uv_min[1] >= 0.0 && e.uv_max[1] <= 1.0);
                    packed += 1;
                }
                None => break,
            }
        }
        assert_eq!(packed, 1000, "every tile must fit once the atlas grows");
        assert!(atlas.height() > start_h, "atlas should have grown taller");
        assert!(atlas.height() <= MAX_ATLAS_PX);
    }

    // Full path over a real, embedded stroke font (no system fonts needed):
    // Face::resolve -> glyph -> bake -> pack, plus the (font, char) cache.
    #[test]
    fn real_lff_glyph_bakes_and_packs() {
        let mut atlas = GlyphAtlas::new(512, 512);
        let e = atlas.get_or_insert("txt", 'A', false).expect("LFF 'A' bakes");
        assert!(atlas.is_dirty());
        assert!(
            e.plane_max[0] > e.plane_min[0] && e.plane_max[1] > e.plane_min[1],
            "tile covers a positive-area glyph rect"
        );
        assert!(
            e.uv_max[0] > e.uv_min[0]
                && e.uv_max[1] > e.uv_min[1]
                && e.uv_max[0] <= 1.0
                && e.uv_max[1] <= 1.0,
            "uv rect non-degenerate and within the atlas"
        );
        assert!(e.advance > 0.0);
        // Second lookup is cached: identical placement, no re-pack.
        let e2 = atlas.get_or_insert("txt", 'A', false).expect("cached");
        assert_eq!(e.uv_min, e2.uv_min);
        assert_eq!(e.uv_max, e2.uv_max);
    }
}
