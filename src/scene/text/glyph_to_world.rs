//! The glyph-quad maths the PDF and SVG plot text paths share: the atlas'
//! export snapshot, uv-key → tile lookup and the affine lift from a placed
//! quad's glyph plane into world coordinates. One copy, so the two exporters
//! cannot drift apart on plane mapping, glyph units or the atlas' nominal pen.

use super::sdf_atlas::{self, GlyphAtlas, GlyphExport};
use crate::scene::pipeline::text_gpu::TextVertex;
use std::collections::HashMap;

/// The atlas' baked-glyph geometry, taken once per export. Table lookups key
/// on [`sdf_atlas::uv_key`] of a quad's `uv` minimum corner; `solid_key` marks
/// the solid-texel tile (underline / overline / strike bars).
pub struct GlyphSnapshot {
    pub table: HashMap<u64, GlyphExport>,
    pub solid_key: u64,
}

impl GlyphSnapshot {
    pub fn from_atlas(atlas: &GlyphAtlas) -> Self {
        Self {
            table: atlas.export_table(),
            solid_key: sdf_atlas::uv_key(atlas.solid_uv()),
        }
    }

    /// A snapshot of the process-global atlas; `None` when its lock is poisoned.
    pub fn snapshot() -> Option<Self> {
        let atlas = sdf_atlas::text_atlas().lock().ok()?;
        Some(Self::from_atlas(&atlas))
    }

    pub fn glyph(&self, key: u64) -> Option<&GlyphExport> {
        self.table.get(&key)
    }
}

/// The affine basis of one placed glyph quad: the glyph plane's `plane_min`
/// sits at `bl`, its +x at `br`, its +y at `tl` and its +x+y at `tr` (verts
/// run [bl, br, tr, bl, tr, tl] — `tl` carries the uv-min corner and so the
/// tile key).
///
/// The glyph-space normalisation stays f32 (the step the atlas' own maths
/// takes); the lift into world coordinates widens to f64, so UTM-scale text
/// keeps its local glyph detail.
pub struct QuadBasis {
    pub bl: [f64; 2],
    pub br: [f64; 2],
    pub tr: [f64; 2],
    pub tl: [f64; 2],
    pub plane_min: [f32; 2],
    sx: f64,
    sy: f64,
}

impl QuadBasis {
    /// The basis of a quad against its tile's plane measure; `None` when the
    /// measure is degenerate — a width or height under 1e-9 glyph units draws
    /// nothing, and both exporters skip or reject such a quad.
    pub fn of(quad: &[TextVertex; 6], glyph: &GlyphExport) -> Option<Self> {
        let xy = |v: &TextVertex| {
            [
                v.pos[0] as f64 + v.pos_low[0] as f64,
                v.pos[1] as f64 + v.pos_low[1] as f64,
            ]
        };
        let (pmin, pmax) = (glyph.plane_min, glyph.plane_max);
        // Exactly the f32 plane size the backend maths always divided with.
        let (sx, sy) = ((pmax[0] - pmin[0]) as f64, (pmax[1] - pmin[1]) as f64);
        if sx.abs() < 1e-9 || sy.abs() < 1e-9 {
            return None;
        }
        Some(Self {
            bl: xy(&quad[0]),
            br: xy(&quad[1]),
            tr: xy(&quad[2]),
            tl: xy(&quad[5]),
            plane_min: pmin,
            sx,
            sy,
        })
    }

    /// A glyph-plane point lifted into world XY.
    pub fn world(&self, p: &[f32; 2]) -> [f64; 2] {
        let u = ((p[0] - self.plane_min[0]) / self.sx as f32) as f64;
        let v = ((p[1] - self.plane_min[1]) / self.sy as f32) as f64;
        [
            self.bl[0] + u * (self.br[0] - self.bl[0]) + v * (self.tl[0] - self.bl[0]),
            self.bl[1] + u * (self.br[1] - self.bl[1]) + v * (self.tl[1] - self.bl[1]),
        ]
    }

    /// The placed glyph's height unit (world mm of one glyph-plane unit) —
    /// the scale the atlas' nominal pen width rides on.
    pub fn glyph_unit_mm(&self) -> f64 {
        ((self.tl[0] - self.bl[0]).powi(2) + (self.tl[1] - self.bl[1]).powi(2)).sqrt()
            / self.sy.abs()
    }

    /// The glyph-plane size the tile's points normalise against.
    pub fn plane_size(&self) -> [f64; 2] {
        [self.sx, self.sy]
    }
}

/// The tile a quad resolves to, through its uv-min corner (`tl`).
pub fn quad_uv_key(quad: &[TextVertex; 6]) -> u64 {
    sdf_atlas::uv_key([quad[5].uv[0], quad[5].uv[1]])
}

/// The atlas' nominal pen width of a stroke glyph, in world millimetres:
/// two pen half-widths wide in glyph-plane units, scaled to the glyph's
/// placed size. The exporters translate this into their own page units and
/// floors; an explicit CTB lineweight still wins over it, gaining the same
/// 1.7× the atlas bakes into bold strokes.
pub fn nominal_pen_world_mm(bold: bool, glyph_unit_mm: f32) -> f64 {
    // f32 exactly as the atlas pens were always computed; nothing behind the
    // cast should move a plotted pixel.
    (2.0 * sdf_atlas::stroke_pen_half_units(bold) * glyph_unit_mm) as f64
}

/// One glyph-quad chunk of a wire's text vertices, for the exporters that
/// iterate and classify quads themselves (the PDF text layer, the outline
/// preflight).
pub fn wire_quads(wire: &crate::scene::WireModel) -> impl Iterator<Item = &[TextVertex; 6]> {
    wire.text_verts
        .chunks_exact(6)
        .map(|chunk| <&[TextVertex; 6]>::try_from(chunk).expect("chunks_exact(6)"))
}
